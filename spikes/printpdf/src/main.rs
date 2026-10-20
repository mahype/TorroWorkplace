//! Spike WZ.1 (issue #225): sets `fixtures/sample.md` as PDFs in `out/` with printpdf 0.12.8.
//!
//! Way A: Markdown → cleaned HTML (pulldown-cmark) → printpdf's HTML layout (azul-layout). Header, footer, page
//! number and logo are drawn afterwards on every page with printpdf's text layer, because printpdf's own page
//! number is fixed English ("Page X of Y"). Each run writes several variants (paper, fonts, built-in decoration),
//! and prints facts the README records. Nothing here is product code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pulldown_cmark::{Alignment, CowStr, Event, Options, Parser, Tag, TagEnd};

/// One output: paper, margins, fonts and which decoration is used.
struct Variant {
    name: &'static str,
    paper: (f32, f32),
    margins: [f32; 4],
    font_pt: f32,
    fonts: FontSet,
    /// printpdf's own header/footer/page number (to see what they do) instead of the drawn ones.
    builtin_decoration: bool,
}

#[derive(Clone, Copy)]
enum FontSet {
    /// Inter from torro-gui: Regular, Medium, SemiBold; no bold, no italic file.
    Inter,
    /// Lato from the design "channelr": Regular and Bold.
    Lato,
}

const A4: (f32, f32) = (210.0, 297.0);
const A5: (f32, f32) = (148.0, 210.0);
const LETTER: (f32, f32) = (215.9, 279.4);

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let markdown = match std::fs::read_to_string(root.join("fixtures/sample.md")) {
        Ok(text) => text,
        Err(error) => return eprintln!("fixture: {error}"),
    };
    let (html_body, links) = markdown_to_html(&markdown);
    let out = root.join("out");
    if let Err(error) = std::fs::create_dir_all(&out) {
        return eprintln!("out: {error}");
    }
    if let Err(error) = std::fs::write(out.join("sample.html"), &html_body) {
        return eprintln!("html: {error}");
    }

    #[cfg(feature = "pdf")]
    {
        // `printpdf-spike html <file> …`: set raw HTML experiments with the same fonts and margins.
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("html") {
            for file in &args[1..] {
                pdf::raw(&out, Path::new(file));
            }
            return;
        }
        pdf::run(&root, &out, &html_body, &links);
    }
    #[cfg(all(feature = "text", not(feature = "pdf")))]
    text_layer_only(&out);
    #[cfg(not(feature = "pdf"))]
    println!("built without printpdf: wrote only out/sample.html ({} bytes, {} links)", html_body.len(), links.len());
}

/// Way B's smallest case: one page with a line of text in Inter, to measure the binary without the HTML layout.
#[cfg(all(feature = "text", not(feature = "pdf")))]
fn text_layer_only(out: &Path) {
    use printpdf_text::*;
    const INTER: &[u8] = include_bytes!("../../../crates/torro-gui/assets/fonts/Inter-Regular.ttf");
    let mut doc = PdfDocument::new("text layer");
    let Some(font) = ParsedFont::from_bytes(INTER, 0, &mut Vec::new()) else {
        return eprintln!("font does not parse");
    };
    let id = doc.add_font(&font);
    let ops = vec![
        Op::StartTextSection,
        Op::SetTextCursor { pos: Point::new(Mm(20.0), Mm(270.0)) },
        Op::SetFont { font: PdfFontHandle::External(id), size: Pt(11.0) },
        Op::ShowText { items: vec![TextItem::Text("Grüße aus der Textschicht".into())] },
        Op::EndTextSection,
    ];
    let bytes =
        doc.with_pages(vec![PdfPage::new(Mm(210.0), Mm(297.0), ops)]).save(&PdfSaveOptions::default(), &mut Vec::new());
    let _ = std::fs::write(out.join("text-layer.pdf"), bytes);
}

/// Markdown → HTML body plus the targets of its links. Raw HTML and pictures fall away (their text stays), links
/// keep only `http`, `https` and `mailto`, and a paragraph that is only `\pagebreak` becomes a page break.
/// pulldown-cmark escapes every text; the markup written here never carries text from the Markdown.
///
/// Lists and quotes become flex boxes: azul-layout repeats the text of a list item that holds a nested list, and
/// draws no `border-left`. Bold is a font family of its own: azul does not pick a face by `font-weight`. Each link
/// gets its own colour, so its glyphs can be found after the layout and covered with a link annotation.
fn markdown_to_html(markdown: &str) -> (String, Vec<String>) {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let mut kept = Vec::with_capacity(events.len());
    // Whether each open link was kept, so its end matches.
    let mut links: Vec<bool> = Vec::new();
    let mut targets = Vec::new();
    // Next number of each open list (`None` = bullets).
    let mut lists: Vec<Option<u64>> = Vec::new();
    // Width in percent and alignment of each column of the open table, and the next cell's column.
    let mut columns: Vec<(u32, Alignment)> = Vec::new();
    let mut column = 0;
    let html = |markup: String| Event::Html(CowStr::Boxed(markup.into_boxed_str()));
    let mut index = 0;
    while index < events.len() {
        let event = &events[index];
        // `\pagebreak` alone in a paragraph: Start(Paragraph), Text, End(Paragraph).
        if matches!(event, Event::Start(Tag::Paragraph))
            && matches!(events.get(index + 1), Some(Event::Text(text)) if text.trim() == "\\pagebreak")
            && matches!(events.get(index + 2), Some(Event::End(TagEnd::Paragraph)))
        {
            kept.push(html("<div class=\"pagebreak\"></div>\n".into()));
            index += 3;
            continue;
        }
        match event {
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => {}
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => {}
            Event::Start(Tag::List(first)) => {
                lists.push(*first);
                kept.push(html("<div class=\"list\">".into()));
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                kept.push(html("</div>\n".into()));
            }
            Event::Start(Tag::Item) => {
                let marker = match lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => "•".to_string(),
                };
                kept.push(html(format!(
                    "<div class=\"item\"><div class=\"marker\">{marker}</div><div class=\"content\">"
                )));
            }
            Event::End(TagEnd::Item) => kept.push(html("</div></div>\n".into())),
            Event::Start(Tag::BlockQuote(_)) => {
                kept.push(html("<div class=\"quote\"><div class=\"bar\"></div><div class=\"content\">".into()))
            }
            Event::End(TagEnd::BlockQuote(_)) => kept.push(html("</div></div>\n".into())),
            Event::Start(Tag::Table(alignments)) => {
                columns = table_widths(&events[index..], alignments.len())
                    .into_iter()
                    .zip(alignments.iter().copied())
                    .collect();
                kept.push(html("<div class=\"table\">".into()));
            }
            Event::End(TagEnd::Table) => kept.push(html("</div>\n".into())),
            Event::Start(Tag::TableHead) => {
                column = 0;
                kept.push(html("<div class=\"row head\">".into()));
            }
            Event::Start(Tag::TableRow) => {
                column = 0;
                kept.push(html("<div class=\"row\">".into()));
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => kept.push(html("</div>\n".into())),
            Event::Start(Tag::TableCell) => {
                let (width, alignment) = columns.get(column).copied().unwrap_or((10, Alignment::None));
                let align = match alignment {
                    Alignment::Right => "right",
                    Alignment::Center => "center",
                    Alignment::Left | Alignment::None => "left",
                };
                column += 1;
                kept.push(html(format!("<div class=\"cell\" style=\"width: {width}%; text-align: {align}\">")));
            }
            Event::End(TagEnd::TableCell) => kept.push(html("</div>".into())),
            Event::Start(Tag::Strong) => kept.push(html("<span class=\"strong\">".into())),
            Event::End(TagEnd::Strong) => kept.push(html("</span>".into())),
            Event::Start(Tag::Emphasis) => kept.push(html("<span class=\"em\">".into())),
            Event::End(TagEnd::Emphasis) => kept.push(html("</span>".into())),
            Event::Start(Tag::Link { dest_url, .. }) => {
                let allowed = ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|scheme| dest_url.to_ascii_lowercase().starts_with(scheme));
                // 256 colours that all look like the same blue; later links stay plain text.
                let allowed = allowed && targets.len() < 256;
                links.push(allowed);
                if allowed {
                    let (red, green) = link_colour(targets.len());
                    targets.push(dest_url.to_string());
                    kept.push(html(format!("<span class=\"link\" style=\"color: rgb({red}, {green}, {LINK_BLUE})\">")));
                }
            }
            Event::End(TagEnd::Link) => {
                if links.pop().unwrap_or(false) {
                    kept.push(html("</span>".into()));
                }
            }
            other => kept.push(other.clone()),
        }
        index += 1;
    }
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, kept.into_iter());
    (out, targets)
}

/// Column widths in percent from the longest text of each column (a table is laid out as flex rows: azul's
/// tables draw no cell borders and break pages too early).
fn table_widths(events: &[Event], count: usize) -> Vec<u32> {
    let mut longest = vec![1usize; count];
    let mut column = 0;
    for event in events {
        match event {
            Event::End(TagEnd::Table) => break,
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => column = 0,
            Event::End(TagEnd::TableCell) => column += 1,
            Event::Text(text) | Event::Code(text) => {
                if let Some(width) = longest.get_mut(column) {
                    *width = (*width).max(text.chars().count());
                }
            }
            _ => {}
        }
    }
    // Long cells wrap instead of squeezing the others below a few letters.
    let capped: Vec<usize> = longest.iter().map(|width| (*width).clamp(4, 40)).collect();
    let total: usize = capped.iter().sum::<usize>().max(1);
    capped.iter().map(|width| (*width * 100 / total) as u32).collect()
}

const LINK_BLUE: u8 = 208;

/// Red and green of link number `index`: 16 × 16 shades of #0b57d0.
fn link_colour(index: usize) -> (u8, u8) {
    let index = (index % 256) as u8;
    (4 + index / 16, 80 + index % 16)
}

#[cfg(feature = "pdf")]
mod pdf {
    use super::*;
    use printpdf::*;

    const INTER_REGULAR: &[u8] = include_bytes!("../../../crates/torro-gui/assets/fonts/Inter-Regular.ttf");
    const INTER_MEDIUM: &[u8] = include_bytes!("../../../crates/torro-gui/assets/fonts/Inter-Medium.ttf");
    const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../crates/torro-gui/assets/fonts/Inter-SemiBold.ttf");
    const LATO_REGULAR: &[u8] = include_bytes!("../../../fonts/channelr/Lato-Regular.ttf");
    const LATO_BOLD: &[u8] = include_bytes!("../../../fonts/channelr/Lato-Bold.ttf");
    const LOGO: &[u8] = include_bytes!("../../../crates/torro-htmlview/tests/fixtures/cid/logo@torro.example.png");

    pub fn run(_root: &Path, out: &Path, body: &str, links: &[String]) {
        let variants = [
            Variant {
                name: "a4-inter",
                paper: A4,
                margins: [20.0, 20.0, 20.0, 20.0],
                font_pt: 11.0,
                fonts: FontSet::Inter,
                builtin_decoration: false,
            },
            Variant {
                name: "a4-inter-builtin",
                paper: A4,
                margins: [20.0, 20.0, 20.0, 20.0],
                font_pt: 11.0,
                fonts: FontSet::Inter,
                builtin_decoration: true,
            },
            Variant {
                name: "a5-lato",
                paper: A5,
                margins: [15.0, 15.0, 15.0, 15.0],
                font_pt: 10.0,
                fonts: FontSet::Lato,
                builtin_decoration: false,
            },
            Variant {
                name: "letter-inter",
                paper: LETTER,
                margins: [25.0, 25.0, 25.0, 25.0],
                font_pt: 11.0,
                fonts: FontSet::Inter,
                builtin_decoration: false,
            },
        ];
        for variant in &variants {
            let started = Instant::now();
            match render(variant, body, links) {
                Ok((bytes, pages, warnings)) => {
                    let path = out.join(format!("{}.pdf", variant.name));
                    if let Err(error) = std::fs::write(&path, &bytes) {
                        eprintln!("{}: {error}", variant.name);
                        continue;
                    }
                    println!(
                        "{}: {} pages, {} bytes, {} ms, {} warnings",
                        variant.name,
                        pages,
                        bytes.len(),
                        started.elapsed().as_millis(),
                        warnings.len()
                    );
                    for warning in warnings.iter().take(12) {
                        println!("  warn: {warning}");
                    }
                }
                Err(error) => eprintln!("{}: {error}", variant.name),
            }
        }
    }

    /// Font map keys of the text and the bold face: the HTML names them directly.
    fn families(fonts: FontSet) -> (&'static str, &'static str) {
        match fonts {
            FontSet::Inter => ("Inter-Regular", "Inter-SemiBold"),
            FontSet::Lato => ("Lato-Regular", "Lato-Bold"),
        }
    }

    fn font_map(fonts: FontSet) -> BTreeMap<String, Base64OrRaw> {
        let files: &[(&str, &[u8])] = match fonts {
            FontSet::Inter => {
                &[("Inter-Regular", INTER_REGULAR), ("Inter-Medium", INTER_MEDIUM), ("Inter-SemiBold", INTER_SEMIBOLD)]
            }
            FontSet::Lato => &[("Lato-Regular", LATO_REGULAR), ("Lato-Bold", LATO_BOLD)],
        };
        files.iter().map(|(name, bytes)| ((*name).to_string(), Base64OrRaw::Raw(bytes.to_vec()))).collect()
    }

    fn regular(fonts: FontSet) -> &'static [u8] {
        match fonts {
            FontSet::Inter => INTER_REGULAR,
            FontSet::Lato => LATO_REGULAR,
        }
    }

    /// The whole page: style sheet, title with logo (first page only) and the body.
    fn document(variant: &Variant, body: &str) -> String {
        let (regular, bold) = families(variant.fonts);
        let size = variant.font_pt;
        format!(
            r#"<html><head><style>
body {{ font-family: "{regular}"; font-size: {size}pt; line-height: 1.4; color: #1f2328; }}
h1, h2, h3, .strong {{ font-family: "{bold}"; }}
h1 {{ font-size: {h1}pt; margin: 0 0 8pt 0; }}
h2 {{ font-size: {h2}pt; margin: 14pt 0 6pt 0; }}
h3 {{ font-size: {h3}pt; margin: 12pt 0 4pt 0; }}
p {{ margin: 0 0 6pt 0; }}
del {{ text-decoration: line-through; }}
.link {{ text-decoration: underline; }}
code {{ font-family: "Courier"; font-size: {code}pt; }}
pre {{ background-color: #f3f4f6; padding: 6pt; margin: 0 0 8pt 0; white-space: pre; }}
.list {{ margin: 0 0 6pt 0; }}
.item {{ display: flex; flex-direction: row; }}
.marker {{ width: 16pt; flex-shrink: 0; }}
.content {{ flex-grow: 1; }}
.quote {{ display: flex; flex-direction: row; margin: 0 0 8pt 0; color: #4b5563; }}
.bar {{ width: 2pt; background-color: #9aa0a6; flex-shrink: 0; margin-right: 8pt; }}
.table {{ margin: 0 0 8pt 0; border-top: 1px solid #9aa0a6; }}
.row {{ display: flex; flex-direction: row; border-bottom: 1px solid #9aa0a6; }}
.head {{ background-color: #eef0f3; font-family: "{bold}"; }}
.cell {{ padding: 2pt 4pt; }}
hr {{ border: 0; border-top: 1px solid #9aa0a6; margin: 8pt 0; }}
.pagebreak {{ page-break-before: always; }}
.logo {{ width: 40px; height: 40px; }}
</style></head><body>
<img class="logo" src="logo.png" />
{body}
</body></html>"#,
            h1 = size * 1.8,
            h2 = size * 1.4,
            h3 = size * 1.15,
            code = size * 0.9,
        )
    }

    pub fn raw(out: &Path, file: &Path) {
        let html = match std::fs::read_to_string(file) {
            Ok(html) => html,
            Err(error) => return eprintln!("{}: {error}", file.display()),
        };
        let fonts = font_map(FontSet::Inter);
        let options = GeneratePdfOptions {
            margin_top: Some(20.0),
            margin_right: Some(20.0),
            margin_bottom: Some(20.0),
            margin_left: Some(20.0),
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let pool = embedded_only(&fonts);
        match PdfDocument::from_html_with_cache(&html, &BTreeMap::new(), &fonts, &options, &mut warnings, Some(pool)) {
            Ok(doc) => {
                let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if std::env::var_os("DUMP_OPS").is_some() {
                    for op in doc.pages.iter().flat_map(|page| page.ops.iter()) {
                        println!("{op:?}");
                    }
                }
                let bytes = doc.save(&PdfSaveOptions::default(), &mut Vec::new());
                let _ = std::fs::write(out.join(format!("raw-{name}.pdf")), bytes);
                println!("raw-{name}: {} pages", doc.page_count());
            }
            Err(error) => eprintln!("{}: {error}", file.display()),
        }
    }

    /// A font pool with only the embedded fonts: no system font may decide how the PDF looks.
    fn embedded_only(fonts: &BTreeMap<String, Base64OrRaw>) -> html::SharedFontPool {
        let raw: BTreeMap<String, Vec<u8>> = fonts
            .iter()
            .filter_map(|(name, font)| match font {
                Base64OrRaw::Raw(bytes) => Some((name.clone(), bytes.clone())),
                Base64OrRaw::B64(_) => None,
            })
            .collect();
        // A family no system has: rust-fontconfig then scans nothing from disk.
        html::build_font_pool(&raw, Some(&["torro-no-system-fonts"]))
    }

    fn render(variant: &Variant, body: &str, links: &[String]) -> Result<(Vec<u8>, usize, Vec<String>), String> {
        let html = document(variant, body);
        let mut images = BTreeMap::new();
        images.insert("logo.png".to_string(), Base64OrRaw::Raw(LOGO.to_vec()));
        let fonts = font_map(variant.fonts);
        let [top, right, bottom, left] = variant.margins;
        let mut options = GeneratePdfOptions {
            page_width: Some(variant.paper.0),
            page_height: Some(variant.paper.1),
            margin_top: Some(top),
            margin_right: Some(right),
            margin_bottom: Some(bottom),
            margin_left: Some(left),
            ..Default::default()
        };
        if variant.builtin_decoration {
            options.header_text = Some("Angebot für Müller & Söhne".into());
            options.footer_text = Some("Köln, 19.10.2026".into());
            options.show_page_numbers = Some(true);
        }
        let mut warnings = Vec::new();
        let pool = embedded_only(&fonts);
        let mut doc = PdfDocument::from_html_with_cache(&html, &images, &fonts, &options, &mut warnings, Some(pool))?;
        let mut messages: Vec<String> = warnings.iter().map(|w| format!("{w:?}")).collect();
        if !variant.builtin_decoration {
            decorate(&mut doc, variant, &mut messages)?;
        }
        let annotated = annotate_links(&mut doc, links);
        messages.push(format!("links: {annotated} annotations for {} targets", links.len()));
        let pages = doc.page_count();
        let mut save_warnings = Vec::new();
        let bytes = doc.save(&PdfSaveOptions::default(), &mut save_warnings);
        messages.extend(save_warnings.iter().map(|w| format!("save: {w:?}")));
        Ok((bytes, pages, messages))
    }

    /// Header (title + logo) and footer (date + translated page number) drawn on every page with the text layer.
    fn decorate(doc: &mut PdfDocument, variant: &Variant, messages: &mut Vec<String>) -> Result<(), String> {
        let mut font_warnings = Vec::new();
        let font = ParsedFont::from_bytes(regular(variant.fonts), 0, &mut font_warnings)
            .ok_or("decoration font does not parse")?;
        let font_id = doc.add_font(&font);
        let mut image_warnings = Vec::new();
        let logo = RawImage::decode_from_bytes(LOGO, &mut image_warnings)?;
        let logo_id = doc.add_image(&logo);
        messages.extend(image_warnings.iter().map(|w| format!("logo: {w:?}")));

        let (width, height) = variant.paper;
        let [top, right, bottom, left] = variant.margins;
        let total = doc.pages.len();
        let size = 8.0;
        for (index, page) in doc.pages.iter_mut().enumerate() {
            let number = format!("Seite {} von {}", index + 1, total);
            let number_width = text_width(&font, &number, size);
            let header_y = height - top / 2.0;
            let footer_y = bottom / 2.0;
            page.ops.extend([
                Op::SaveGraphicsState,
                Op::UseXobject {
                    id: logo_id.clone(),
                    transform: XObjectTransform {
                        translate_x: Some(Mm(width - right - 8.0).into_pt()),
                        translate_y: Some(Mm(header_y - 3.0).into_pt()),
                        // 40 px at 300 dpi would be 3.4 mm; 8 mm wanted.
                        scale_x: Some(8.0 / (40.0 / 300.0 * 25.4)),
                        scale_y: Some(8.0 / (40.0 / 300.0 * 25.4)),
                        ..Default::default()
                    },
                },
                Op::RestoreGraphicsState,
            ]);
            page.ops.extend(text_at(&font_id, size, left, header_y, "Angebot für Müller & Söhne"));
            page.ops.extend(text_at(&font_id, size, left, footer_y, "Köln, 19.10.2026"));
            let right_x = width - right - number_width;
            page.ops.extend(text_at(&font_id, size, right_x, footer_y, &number));
        }
        Ok(())
    }

    /// Covers the glyphs of each link colour with a link annotation, one per line. Returns how many it added.
    fn annotate_links(doc: &mut PdfDocument, links: &[String]) -> usize {
        let mut added = 0;
        for page in &mut doc.pages {
            // (link, x0, x1, baseline, font size) of the glyphs seen so far on the current line of a link.
            let mut runs: Vec<(usize, f32, f32, f32, f32)> = Vec::new();
            let mut link: Option<usize> = None;
            let mut size = 0.0;
            let mut position = (0.0, 0.0);
            for op in &page.ops {
                match op {
                    Op::SetFillColor { col: Color::Rgb(rgb) } => {
                        link = link_of(rgb, links.len());
                    }
                    Op::SetFont { size: Pt(pt), .. } => size = *pt,
                    Op::SetTextMatrix { matrix: TextMatrix::Raw([_, _, _, _, x, y]) } => position = (*x, *y),
                    Op::ShowText { .. } => {
                        if let Some(index) = link {
                            let (x, y) = position;
                            match runs.last_mut() {
                                Some(run) if run.0 == index && (run.3 - y).abs() < 0.5 => run.2 = x,
                                _ => runs.push((index, x, x, y, size)),
                            }
                        }
                    }
                    _ => {}
                }
            }
            for (index, x0, x1, y, size) in runs {
                // The last glyph's own width is not in the ops; half the font size covers most letters.
                let rect = Rect {
                    x: Pt(x0),
                    y: Pt(y - size * 0.25),
                    width: Pt(x1 - x0 + size * 0.6),
                    height: Pt(size * 1.1),
                    mode: None,
                    winding_order: None,
                };
                page.ops.push(Op::LinkAnnotation {
                    link: LinkAnnotation::new(
                        rect,
                        Actions::Uri(links[index].clone()),
                        // No frame: the default draws a 1 pt box around the link.
                        Some(BorderArray::Solid([0.0, 0.0, 0.0])),
                        None,
                        None,
                    ),
                });
                added += 1;
            }
        }
        added
    }

    fn link_of(rgb: &Rgb, count: usize) -> Option<usize> {
        let channel = |value: f32| (value * 255.0).round() as i32;
        if channel(rgb.b) != i32::from(LINK_BLUE) {
            return None;
        }
        let (red, green) = (channel(rgb.r), channel(rgb.g));
        (0..count).find(|index| {
            let (r, g) = link_colour(*index);
            i32::from(r) == red && i32::from(g) == green
        })
    }

    fn text_at(font: &FontId, size: f32, x_mm: f32, y_mm: f32, text: &str) -> Vec<Op> {
        vec![
            Op::SaveGraphicsState,
            Op::StartTextSection,
            Op::SetTextCursor { pos: Point::new(Mm(x_mm), Mm(y_mm)) },
            Op::SetFont { font: PdfFontHandle::External(font.clone()), size: Pt(size) },
            Op::SetFillColor { col: Color::Rgb(Rgb { r: 0.35, g: 0.38, b: 0.42, icc_profile: None }) },
            Op::ShowText { items: vec![TextItem::Text(text.to_string())] },
            Op::EndTextSection,
            Op::RestoreGraphicsState,
        ]
    }

    /// Width in mm from the glyph advances, so the page number can sit flush right.
    fn text_width(font: &ParsedFont, text: &str, size_pt: f32) -> f32 {
        let units = f32::from(font.font_metrics.units_per_em.max(1));
        let advance: f32 = text
            .chars()
            .filter_map(|c| font.lookup_glyph_index(c as u32))
            .filter_map(|gid| font.get_glyph_width_internal(gid))
            .map(|w| w as f32)
            .sum();
        advance / units * size_pt * 25.4 / 72.0
    }
}
