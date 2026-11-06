//! Drawing a mail as a PDF or a picture (issue #501, `mail.save_message_as`): the head (subject,
//! sender, recipients, date) and the content drawn with the reader's own HTML view
//! (`torro-htmlview`), so an HTML mail looks as it does in the mail window (decision 1 of the plan).
//!
//! **Drawing.** The head is HTML of its own, every value escaped, drawn without the network. The
//! content is the mail's cleaned HTML (`mime::read`, as `mail.get_message` hands it out) with its
//! `cid:` pictures from the message's own parts and – like the reader – its external pictures loaded
//! (decision 2); a text mail is its text in a block that keeps its lines. Both are laid out 720
//! logical pixels wide at scale 2 and always in light colours: paper is light. Drawing waits for the
//! pictures at most [`WAIT_FOR_PICTURES`]; a picture that comes later can move what follows, so the
//! heights are measured again on every page and after the picture, and a mail that grew is marked
//! shortened. [`Content`] is not `Send`, so everything happens on one blocking thread, and only one
//! export runs at a time ([`EXPORTS`]): a PNG holds a few dozen MB while it is made, a PDF its pages
//! uncompressed until it is written (about 9 MB a page, at most [`MAX_PAGES`]).
//!
//! **PNG.** One long picture, written band by band (at most [`BAND_ROWS`] rows painted at once), at
//! most [`MAX_PNG_ROWS`] rows; a longer mail is cut (decision 4). **PDF.** A4 pages with 12-mm margins,
//! each one picture of a band of the mail; a page ends at the lowest row of one colour in its last
//! 15 % (a gap between lines), else where the page is full. At most [`MAX_PAGES`] pages, lossless and
//! at full size, and the same mail makes the same bytes (a target's "Skip if identical"). The PDF's
//! text cannot be selected: it is pictures.
//!
//! **Log.** Nothing here writes a subject, an address or any text of the mail anywhere.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use printpdf::{
    ImageCompression, ImageOptimizationOptions, Mm, Op, PdfDocument, PdfPage, PdfSaveOptions, RawImage, RawImageData,
    RawImageFormat, XObject, XObjectId, XObjectTransform,
};
use torro_core::i18n::{tr, trf};
use torro_htmlview::{CidSource, Content, MAX_PICTURE_BYTES, Scheme};
use torro_ipc::mail::ExportFormat;
use torro_mail::{AttachmentInfo, MailAddress};
use torro_mail_imap::mime;

/// The width the mail is laid out at, in logical pixels: a reading column, as wide as a newsletter.
const WIDTH: f32 = 720.0;

/// Device pixels per logical pixel: sharp on paper and on a high-density screen.
const SCALE: f32 = 2.0;

/// The most rows painted at once.
const BAND_ROWS: u32 = 2048;

/// The most rows of a picture (15,000 logical pixels); a longer mail is cut.
pub const MAX_PNG_ROWS: u32 = 30_000;

/// The most pages of a PDF; a longer mail is cut.
pub const MAX_PAGES: usize = 50;

/// How long drawing waits for the mail's pictures, asking every [`POLL`].
const WAIT_FOR_PICTURES: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(50);

/// A4 and its margins, in millimetres.
const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
const MARGIN_MM: f32 = 12.0;

/// The part of a page a cut between lines is looked for in, from its bottom.
const CUT_ROOM: f32 = 0.15;

/// The most addresses shown per line of the head; the rest are "+{n} more".
const MAX_ADDRESSES: usize = 10;

/// One export at a time: each holds a mail drawn in memory.
static EXPORTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The head of the mail as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub subject: String,
    pub from: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub date: DateTime<Utc>,
}

/// The content of the mail: its cleaned HTML with the message for its `cid:` pictures, or its text.
#[derive(Debug, Clone)]
pub enum Body {
    Html {
        html: String,
        /// The whole message, for the pictures it carries.
        raw: Arc<Vec<u8>>,
        /// Its parts, for the Content-IDs.
        parts: Vec<AttachmentInfo>,
    },
    Text(String),
}

/// What [`draw`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// The mail was longer than a file takes; its end is cut off.
    pub shortened: bool,
}

/// Draws the mail with `head` and `body` as `format`. `online`: external pictures are loaded, as the
/// reader loads them (the tests draw offline). `Err` when it cannot be drawn or written; the reason
/// goes to the log without anything of the mail.
pub async fn draw(head: Head, body: Body, format: ExportFormat, online: bool) -> Result<Drawn, ()> {
    let _one = EXPORTS.lock().await;
    let drawn = tokio::task::spawn_blocking(move || draw_now(&head, body, format, online)).await;
    match drawn {
        Ok(Ok(drawn)) => Ok(drawn),
        Ok(Err(reason)) => {
            eprintln!("torroworkplace: mail: a mail could not be drawn: {reason}");
            Err(())
        }
        Err(_) => {
            eprintln!("torroworkplace: mail: drawing a mail stopped");
            Err(())
        }
    }
}

/// [`draw`] on this (blocking) thread.
fn draw_now(head: &Head, body: Body, format: ExportFormat, online: bool) -> Result<Drawn, String> {
    let head = Content::offline(&head_html(head), None);
    let body = match body {
        Body::Html { html, raw, parts } => {
            let pictures = Pictures::of(raw, parts).map(|pictures| Arc::new(pictures) as Arc<dyn CidSource>);
            match (online, pictures) {
                (true, Some(pictures)) => Content::with_cid(&html, pictures),
                (true, None) => Content::new(&html),
                (false, pictures) => Content::offline(&html, pictures),
            }
        }
        Body::Text(text) => Content::offline(&text_html(&text), None),
    };
    wait_for_pictures(&body);
    let sheet = Sheet::of(vec![head, body]);
    match format {
        ExportFormat::Png => png(&sheet, MAX_PNG_ROWS),
        ExportFormat::Pdf => pdf(&sheet, MAX_PAGES),
    }
}

/// Waits until `content` has its pictures, at most [`WAIT_FOR_PICTURES`]. A painted row takes in
/// what arrived (`Content::snapshot`), so `is_loading` can turn false.
fn wait_for_pictures(content: &Content) {
    let start = Instant::now();
    loop {
        let _ = content.snapshot(WIDTH, SCALE, Scheme::Light, 0.0, 1.0);
        if !content.is_loading() || start.elapsed() >= WAIT_FOR_PICTURES {
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// The pictures a mail carries with it (`cid:`), from its own parts.
struct Pictures {
    raw: Arc<Vec<u8>>,
    /// `(content id, part, size)` of every part with a Content-ID.
    parts: Vec<(String, String, u64)>,
}

impl Pictures {
    fn of(raw: Arc<Vec<u8>>, parts: Vec<AttachmentInfo>) -> Option<Pictures> {
        let parts: Vec<(String, String, u64)> =
            parts.into_iter().filter_map(|part| Some((part.content_id?, part.part, part.size))).collect();
        (!parts.is_empty()).then_some(Pictures { raw, parts })
    }
}

impl CidSource for Pictures {
    fn picture(&self, content_id: &str) -> Option<Vec<u8>> {
        let (_, part, size) = self.parts.iter().find(|(id, _, _)| id == content_id)?;
        if *size > MAX_PICTURE_BYTES {
            return None;
        }
        mime::attachment(&self.raw, part).ok().map(|attachment| attachment.bytes)
    }
}

// ----- the HTML of the head and of a text mail -----

/// `text` safe inside HTML text and attribute values.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// A colour of the light design as CSS.
fn css(color: iced::Color) -> String {
    let [r, g, b, a] = color.into_rgba8();
    format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0)
}

/// "Stadtwerke Bonn <billing@stadtwerke-bonn.de>", or the address alone.
fn address(address: &MailAddress) -> String {
    match address.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => format!("{name} <{}>", address.address),
        None => address.address.clone(),
    }
}

/// The addresses of one line of the head: at most [`MAX_ADDRESSES`], then "+{n} more".
fn addresses(list: &[MailAddress]) -> String {
    let mut shown: Vec<String> = list.iter().take(MAX_ADDRESSES).map(address).collect();
    if list.len() > MAX_ADDRESSES {
        shown.push(trf("+{n} more", &[("n", &(list.len() - MAX_ADDRESSES))]));
    }
    shown.join(", ")
}

/// The head as HTML: the subject large, then From, To, Cc and Date with their words, a line under
/// it. Every value from the mail is escaped; nothing in it is fetched.
fn head_html(head: &Head) -> String {
    let palette = torro_gui::theme::Palette::light();
    let (text, label, line) = (css(palette.text), css(palette.text_3), css(palette.line_strong));
    let subject = match head.subject.trim() {
        "" => tr("(no subject)").to_string(),
        subject => subject.to_string(),
    };
    let local = head.date.with_timezone(&Local);
    let date = format!(
        "{}, {}",
        torro_gui::datetime::format_long_date(local.date_naive()),
        torro_gui::datetime::format_time(local.time())
    );
    let mut rows = Vec::new();
    if let Some(from) = &head.from {
        rows.push((tr("From"), address(from)));
    }
    if !head.to.is_empty() {
        rows.push((tr("To"), addresses(&head.to)));
    }
    if !head.cc.is_empty() {
        rows.push((tr("Cc"), addresses(&head.cc)));
    }
    rows.push((tr("Date"), date));
    let rows: String = rows
        .into_iter()
        .map(|(word, value)| {
            format!(
                "<div style=\"display:flex;margin-top:2px\"><div style=\"width:72px;flex-shrink:0;color:{label}\">{}</div>\
                 <div style=\"flex:1\">{}</div></div>",
                escape(word),
                escape(&value)
            )
        })
        .collect();
    format!(
        "<html><body style=\"margin:0;padding:24px 8px 16px 8px;font-family:sans-serif;color:{text}\">\
         <div style=\"font-size:20px;font-weight:600;line-height:1.3;margin-bottom:12px\">{}</div>\
         <div style=\"font-size:13px;line-height:1.5\">{rows}</div>\
         <div style=\"border-bottom:1px solid {line};margin-top:16px\"></div></body></html>",
        escape(&subject)
    )
}

/// A text mail as HTML: its lines kept, long ones wrapped.
fn text_html(text: &str) -> String {
    let color = css(torro_gui::theme::Palette::light().text);
    format!(
        "<html><body><div style=\"white-space:pre-wrap;font-family:sans-serif;font-size:14px;line-height:1.5;\
         color:{color}\">{}</div></body></html>",
        escape(text)
    )
}

// ----- painting -----

/// The head and the content one under the other, addressed by device rows.
struct Sheet {
    parts: Vec<(Content, std::cell::Cell<u32>)>,
    width: u32,
}

impl Sheet {
    fn of(contents: Vec<Content>) -> Sheet {
        let parts = contents
            .into_iter()
            .map(|content| {
                let rows = std::cell::Cell::new(rows_of(&content));
                (content, rows)
            })
            .collect();
        Sheet { parts, width: (WIDTH * SCALE).round() as u32 }
    }

    /// Every row of the sheet, as last measured.
    fn rows(&self) -> u32 {
        self.parts.iter().map(|(_, rows)| rows.get()).sum()
    }

    /// Measures every part again – a picture that arrived late can make one taller – and answers
    /// every row of the sheet.
    fn measure(&self) -> u32 {
        for (content, rows) in &self.parts {
            rows.set(rows_of(content));
        }
        self.rows()
    }

    /// The rows `from..to` as RGB, 3 bytes a pixel, row by row; rows a part does not paint are white.
    fn band(&self, from: u32, to: u32) -> Vec<u8> {
        let row_bytes = self.width as usize * 3;
        let mut rgb = Vec::with_capacity((to - from) as usize * row_bytes);
        let mut offset = 0;
        for (content, rows) in &self.parts {
            let rows = rows.get();
            let (start, end) = (from.max(offset), to.min(offset + rows));
            if start < end {
                let top = (start - offset) as f32 / SCALE;
                let height = (end - start) as f32 / SCALE;
                let painted = content.snapshot(WIDTH, SCALE, Scheme::Light, top, height);
                let wanted = (end - start) as usize * row_bytes;
                let before = rgb.len();
                if painted.width == self.width {
                    rgb.extend(painted.rgba.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]));
                }
                rgb.resize(before + wanted, 255);
            }
            offset += rows;
        }
        rgb
    }
}

/// The device rows `content` takes at the export's width.
fn rows_of(content: &Content) -> u32 {
    (content.height(WIDTH, SCALE, Scheme::Light) * SCALE).ceil().max(1.0) as u32
}

/// The sheet as one PNG picture, at most `max_rows` rows ([`MAX_PNG_ROWS`]). A mail that grew while
/// it was written (a late picture) counts as shortened: its end may be missing.
fn png(sheet: &Sheet, max_rows: u32) -> Result<Drawn, String> {
    let total = sheet.rows();
    let rows = total.min(max_rows);
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, sheet.width, rows);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    {
        let mut stream = writer.stream_writer().map_err(|err| err.to_string())?;
        let mut from = 0;
        while from < rows {
            let to = (from + BAND_ROWS).min(rows);
            std::io::Write::write_all(&mut stream, &sheet.band(from, to)).map_err(|err| err.to_string())?;
            from = to;
        }
        stream.finish().map_err(|err| err.to_string())?;
    }
    writer.finish().map_err(|err| err.to_string())?;
    let grew = sheet.measure() > total;
    Ok(Drawn { bytes, shortened: total > rows || grew })
}

/// The rows of one A4 page's picture: as wide as the text between the margins.
fn page_rows(width: u32) -> u32 {
    let ratio = (PAGE_HEIGHT_MM - 2.0 * MARGIN_MM) / (PAGE_WIDTH_MM - 2.0 * MARGIN_MM);
    (width as f32 * ratio).round() as u32
}

/// Where a page of `rgb` (`rows` rows of `width` pixels) ends: after the lowest row of one colour in
/// its last [`CUT_ROOM`], else after its last row.
fn cut(rgb: &[u8], width: u32, rows: u32) -> u32 {
    let row_bytes = width as usize * 3;
    let lowest = rows - ((rows as f32 * CUT_ROOM) as u32).min(rows - 1);
    for row in (lowest..rows).rev() {
        let line = &rgb[row as usize * row_bytes..(row as usize + 1) * row_bytes];
        if line.chunks_exact(3).all(|pixel| pixel == &line[..3]) {
            return row + 1;
        }
    }
    rows
}

/// The sheet as a PDF of A4 pages, at most `max_pages` ([`MAX_PAGES`]).
fn pdf(sheet: &Sheet, max_pages: usize) -> Result<Drawn, String> {
    let mut total = sheet.rows();
    let full = page_rows(sheet.width);
    let mut doc = PdfDocument::new(tr("Mail"));
    let mut from = 0;
    while from < total && doc.pages.len() < max_pages {
        // A late picture moves what follows: each page is cut from the mail as it stands now.
        total = sheet.measure();
        let to = (from + full).min(total);
        let mut rgb = sheet.band(from, to);
        let rows = if to < total { cut(&rgb, sheet.width, to - from) } else { to - from };
        rgb.truncate(rows as usize * sheet.width as usize * 3);
        let image = RawImage {
            pixels: RawImageData::U8(rgb),
            width: sheet.width as usize,
            height: rows as usize,
            data_format: RawImageFormat::RGB8,
            tag: Vec::new(),
        };
        // A fixed id per page (printpdf's own is random): the same mail makes the same file, so a target's
        // "Skip if identical" finds it ([`stable_trailer`]).
        let id = XObjectId(format!("page{}", doc.pages.len() + 1));
        doc.resources.xobjects.map.insert(id.clone(), XObject::Image(image));
        let width_mm = PAGE_WIDTH_MM - 2.0 * MARGIN_MM;
        let height_mm = width_mm * rows as f32 / sheet.width as f32;
        // An image is first drawn at its size at 300 dpi; scale that to the room it takes.
        let natural = |pixels: u32| pixels.max(1) as f32 / 300.0 * 72.0;
        let ops = vec![
            Op::SaveGraphicsState,
            Op::UseXobject {
                id,
                transform: XObjectTransform {
                    translate_x: Some(Mm(MARGIN_MM).into_pt()),
                    translate_y: Some(Mm(PAGE_HEIGHT_MM - MARGIN_MM - height_mm).into_pt()),
                    scale_x: Some(Mm(width_mm).into_pt().0 / natural(sheet.width)),
                    scale_y: Some(Mm(height_mm).into_pt().0 / natural(rows)),
                    ..Default::default()
                },
            },
            Op::RestoreGraphicsState,
        ];
        doc.pages.push(PdfPage::new(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), ops));
        from += rows;
    }
    // Lossless and at full size: printpdf's defaults shrink a page over 2 MB and save it as JPEG,
    // which blurs the text.
    let images = ImageOptimizationOptions {
        max_image_size: None,
        format: Some(ImageCompression::Flate),
        ..ImageOptimizationOptions::default()
    };
    let options = PdfSaveOptions { image_optimization: Some(images), ..PdfSaveOptions::default() };
    let bytes = stable_trailer(doc.save(&options, &mut Vec::new()));
    Ok(Drawn { bytes, shortened: from < total.max(sheet.measure()) })
}

/// `pdf` with the file identifiers of its trailer (`/ID`, two strings printpdf makes at random) set to
/// zeros of the same length, so nothing else moves: the same mail makes the same bytes.
fn stable_trailer(mut pdf: Vec<u8>) -> Vec<u8> {
    let Some(at) = pdf.windows(3).rposition(|window| window == b"/ID") else { return pdf };
    let mut strings = 0;
    let mut inside = false;
    for byte in &mut pdf[at..] {
        match (*byte, inside) {
            (b'(', false) => inside = true,
            (b')', true) => {
                inside = false;
                strings += 1;
                if strings == 2 {
                    break;
                }
            }
            (b']', false) => break,
            (_, true) => *byte = b'0',
            _ => {}
        }
    }
    pdf
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn head() -> Head {
        Head {
            subject: "Ihre Rechnung <März>".into(),
            from: Some(MailAddress {
                name: Some("Stadtwerke Bonn".into()),
                address: "billing@stadtwerke-bonn.de".into(),
            }),
            to: (0..12).map(|n| MailAddress { name: None, address: format!("p{n}@example.org") }).collect(),
            cc: Vec::new(),
            date: Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).single().expect("a date"),
        }
    }

    fn html_body(html: &str) -> Body {
        Body::Html { html: html.into(), raw: Arc::new(Vec::new()), parts: Vec::new() }
    }

    /// The PNG's width and height from its header.
    fn png_size(bytes: &[u8]) -> (u32, u32) {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("a PNG");
        (reader.info().width, reader.info().height)
    }

    /// The head escapes what the mail says and shortens a long list of recipients.
    #[test]
    fn the_head_escapes_the_mail_and_shortens_long_lists() {
        let html = head_html(&head());
        assert!(html.contains("Ihre Rechnung &lt;März&gt;") && !html.contains("<März>"), "{html}");
        assert!(html.contains("Stadtwerke Bonn &lt;billing@stadtwerke-bonn.de&gt;"));
        assert!(html.contains(&escape(&trf("+{n} more", &[("n", &2)]))));
        assert!(!html.contains("p10@example.org"), "at most ten addresses");
        assert!(html.contains(&escape(tr("Date"))));
        let text = text_html("a <b> & \"c\"\nnext");
        assert!(text.contains("a &lt;b&gt; &amp; &quot;c&quot;\nnext"), "{text}");
        let blank = Head { subject: " ".into(), ..head() };
        assert!(head_html(&blank).contains(&escape(tr("(no subject)"))));
    }

    /// An HTML mail and a text mail become a PNG as wide as the column at scale 2, as tall as the
    /// head and the content (Erledigt 4).
    #[tokio::test]
    async fn a_mail_becomes_a_picture_of_the_column() {
        let html = html_body("<html><body><h1>Rechnung</h1><p>Betrag: 42 €</p></body></html>");
        let drawn = draw(head(), html, ExportFormat::Png, false).await.expect("drawn");
        assert!(drawn.bytes.starts_with(b"\x89PNG\r\n\x1a\n") && !drawn.shortened);
        let (width, height) = png_size(&drawn.bytes);
        assert_eq!(width, 1440);
        assert!(height > 200, "head and content: {height}");

        let text = Body::Text("Sehr geehrte Damen und Herren,\n\nanbei die Rechnung.".into());
        let drawn = draw(head(), text, ExportFormat::Png, false).await.expect("drawn");
        assert_eq!(png_size(&drawn.bytes).0, 1440);
    }

    /// The sheet of a text mail of `lines` lines, drawn offline.
    fn text_sheet(lines: usize) -> Sheet {
        let text: String = (0..lines).map(|n| format!("Zeile {n}\n")).collect();
        Sheet::of(vec![Content::offline(&head_html(&head()), None), Content::offline(&text_html(&text), None)])
    }

    fn pages(bytes: &[u8]) -> usize {
        PdfDocument::parse(bytes, &Default::default(), &mut Vec::new()).expect("a PDF").pages.len()
    }

    /// A PDF has one A4 page for a short mail, several for a long one, and is cut after its most
    /// pages (Erledigt 3).
    #[tokio::test]
    async fn a_mail_becomes_a_pdf_of_a4_pages() {
        let short = draw(head(), Body::Text("Kurz.".into()), ExportFormat::Pdf, false).await.expect("drawn");
        assert!(short.bytes.starts_with(b"%PDF") && !short.shortened);
        assert_eq!(pages(&short.bytes), 1);
        let html = html_body("<html><body><p>Betrag: 42 €</p></body></html>");
        let drawn = draw(head(), html, ExportFormat::Pdf, false).await.expect("drawn");
        assert_eq!(pages(&drawn.bytes), 1);

        let long = text_sheet(120);
        assert!(long.rows() > page_rows(long.width) * 2, "{} rows", long.rows());
        let drawn = pdf(&long, MAX_PAGES).expect("drawn");
        assert!(pages(&drawn.bytes) >= 3 && !drawn.shortened);
        let cut_off = pdf(&long, 2).expect("drawn");
        assert_eq!((pages(&cut_off.bytes), cut_off.shortened), (2, true));
    }

    /// A picture longer than its most rows is cut and says so.
    #[test]
    fn a_long_picture_is_cut() {
        let sheet = text_sheet(40);
        let drawn = png(&sheet, 600).expect("drawn");
        assert_eq!((png_size(&drawn.bytes), drawn.shortened), ((1440, 600), true));
        let whole = png(&sheet, MAX_PNG_ROWS).expect("drawn");
        assert_eq!((png_size(&whole.bytes), whole.shortened), ((1440, sheet.rows()), false));
    }

    /// The same mail makes the same PDF and the same PNG, so "Skip if identical" holds; the pages are
    /// kept lossless and at their size.
    #[test]
    fn the_same_mail_makes_the_same_file() {
        let first = pdf(&text_sheet(5), MAX_PAGES).expect("drawn");
        let second = pdf(&text_sheet(5), MAX_PAGES).expect("drawn");
        assert!(first.bytes == second.bytes, "PDF bytes differ");
        let text = String::from_utf8_lossy(&first.bytes);
        assert!(!text.contains("DCTDecode"), "no JPEG");
        assert!(text.contains("/Width 1440"), "full width");
        assert_eq!(
            png(&text_sheet(5), MAX_PNG_ROWS).expect("drawn"),
            png(&text_sheet(5), MAX_PNG_ROWS).expect("drawn")
        );
    }

    /// A page ends after the lowest row of one colour in its last part, else where it is full.
    #[test]
    fn a_page_ends_between_lines() {
        let (width, rows) = (4, 20);
        let mut rgb = vec![0u8; (width * rows * 3) as usize];
        // Every row mixed but row 18, which is all white.
        for row in 0..rows {
            let at = (row * width * 3) as usize;
            rgb[at] = 255;
            if row == 18 {
                rgb[at..at + (width * 3) as usize].fill(255);
            }
        }
        assert_eq!(cut(&rgb, width, rows), 19);
        let at = (18 * width * 3) as usize;
        rgb[at + 3] = 0;
        assert_eq!(cut(&rgb, width, rows), rows, "no gap: the page is full");
        assert_eq!(page_rows(1440), 2114);
    }
}
