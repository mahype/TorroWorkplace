//! Sets a PDF from Markdown and a page template (task WZ.3, concept "Eigene Werkzeuge" in `docs/assistent.md`).
//!
//! [`render`] follows way A of the spike `spikes/printpdf` (WZ.1): pulldown-cmark reads the Markdown, this module
//! writes the HTML itself from its events, printpdf's HTML layout (azul-layout) sets the pages, and printpdf's text
//! layer then adds on every page what that layout cannot do: header, footer, page number and logo, and the clickable
//! areas of links.
//!
//! The HTML is shaped by the gaps of azul-layout 0.0.16 (the spike's README has the evidence):
//!
//! - Lists, quotes and tables are flex rows. A real `<li>` with a nested list repeats its text, `border-left` draws
//!   nothing, and a real `<table>` has no cell borders and breaks pages too early. Borders work only in `px`.
//! - Bold is a font family of its own: `font-weight` picks no face. Italic falls back upright, since no design
//!   pack ships an italic file.
//! - Only the fonts handed in are used ([`NO_SYSTEM_FONTS`]): otherwise printpdf would take the system's fonts and a
//!   PDF would look different on every computer. Code is in the PDF base font Courier (Windows-1252 only).
//! - printpdf's own header, footer and page number are not drawn with `from_html` and are fixed English.
//!
//! What comes from the Markdown never becomes markup: every text is escaped by pulldown-cmark, raw HTML and pictures
//! fall away (their text stays), links keep only `http`, `https` and `mailto` and never reach the HTML as an
//! address. A line of its own that is only `\pagebreak` starts a new page.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use chrono::NaiveDate;
use printpdf::{
    Actions, Base64OrRaw, BorderArray, Color, GeneratePdfOptions, LinkAnnotation, Mm, Op, ParsedFont, PdfDocument,
    PdfFontHandle, PdfPage, PdfResources, PdfSaveOptions, Point, Pt, RawImage, Rect, Rgb, TextItem, TextMatrix,
    XObjectTransform,
};
use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use torro_core::design::{self, FontRole, FontSet, Weight};
use torro_core::i18n::trf;
use torro_core::settings::{Paper, PdfTemplate};

/// The largest logo a template may bring, in bytes.
pub const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;

/// The faces a PDF is set in: the text and heading family of a font, as the bytes of their files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfFonts {
    /// Running text, lists, tables, header and footer: the text font, Regular.
    pub text: Cow<'static, [u8]>,
    /// `**bold**` and table heads: the text font at Bold, else the next weight it has (see [`PdfFonts::of`]).
    pub strong: Cow<'static, [u8]>,
    /// Headings: the heading font at SemiBold, else the next weight it has.
    pub heading: Cow<'static, [u8]>,
}

impl PdfFonts {
    /// The faces of `font`. A weight the family lacks becomes the next heavier one it has, else its heaviest
    /// (Inter has no Bold: bold text is SemiBold). `None` when the text family's Regular file cannot be read any
    /// more (the font's folder changed since it was checked) or a face does not parse for printpdf: the font's
    /// check reads files with another parser, and printpdf refuses some of them, so no PDF could be set.
    pub fn of(font: &FontSet) -> Option<PdfFonts> {
        let fonts = PdfFonts {
            text: face(font, FontRole::Text, Weight::Regular)?,
            strong: face(font, FontRole::Text, Weight::Bold)?,
            heading: face(font, FontRole::Heading, Weight::Semibold)?,
        };
        // The embedded Inter is known to parse; a pack's file is parsed once to see.
        let readable = [&fonts.text, &fonts.strong, &fonts.heading].into_iter().all(|face| match face {
            Cow::Borrowed(_) => true,
            Cow::Owned(bytes) => ParsedFont::from_bytes(bytes, 0, &mut Vec::new()).is_some(),
        });
        readable.then_some(fonts)
    }

    /// The faces of the font `id` from the fonts folder `dir` (`torro_core::design::font_dir`): the chosen font
    /// (`Settings::font`, task L.22). The standard's when the font cannot be
    /// loaded or printpdf cannot read its files ([`PdfFonts::of`]), so a PDF is always set.
    pub fn of_font(dir: Option<&Path>, id: &str) -> PdfFonts {
        // A font of the computer is never embedded (issue #299): no scan of the installed fonts for it.
        if design::is_system_id(id) {
            return PdfFonts::standard();
        }
        let font = dir.and_then(|dir| design::load_font(dir, id).ok());
        PdfFonts::of_pack(font.as_ref())
    }

    /// The faces of `font`, or the standard's when there is none or [`PdfFonts::of`] finds none.
    fn of_pack(font: Option<&FontSet>) -> PdfFonts {
        font.and_then(PdfFonts::of).unwrap_or_else(PdfFonts::standard)
    }

    /// Inter of the standard font, embedded in the binary: Regular, and SemiBold for bold and headings.
    pub fn standard() -> PdfFonts {
        let [regular, _, semibold, _, _] = torro_gui::fonts();
        PdfFonts { text: Cow::Borrowed(regular), strong: Cow::Borrowed(semibold), heading: Cow::Borrowed(semibold) }
    }
}

/// Why no PDF could be set. Neither carries content: the log may show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfError {
    /// A font file of the design does not parse as a font.
    Font,
    /// printpdf's layout refused the document.
    Layout,
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PdfError::Font => "a font of the design cannot be read",
            PdfError::Layout => "the pages cannot be laid out",
        })
    }
}

impl std::error::Error for PdfError {}

/// The bytes of a PDF: `markdown` set on the pages of `template` in `fonts`.
///
/// - Markdown: headings, paragraphs, bold, italic (set upright), struck through, code and code blocks, nested
///   lists, tables (column widths from the longest cell, alignment from the Markdown), quotes, rules, links
///   (`http`, `https`, `mailto`; clickable) and `\pagebreak` alone on a line. Raw HTML and pictures fall away,
///   their text stays; other links stay text. Quotes and lists nest [`MAX_NESTING`] levels deep, bold, italic,
///   struck through and links as many inside each other; deeper ones go on flat (items as lines with their marker).
/// - `title` becomes the document's title and `{title}` in header and footer; `{date}` is `date` in the suite's
///   language. The page number reads "Page 1 of 3" in that language, right in the footer.
/// - `logo`: a PNG or JPEG of at most [`MAX_LOGO_BYTES`], drawn on the right of the header of every page. Anything
///   else is left out with a warning in the log (which names neither file nor title).
///
/// Pure apart from that warning: no file is read or written. Takes a few hundred milliseconds; callers in async
/// code run it on a blocking thread.
pub fn render(
    title: &str,
    markdown: &str,
    template: &PdfTemplate,
    fonts: &PdfFonts,
    logo: Option<&[u8]>,
    date: NaiveDate,
) -> Result<Vec<u8>, PdfError> {
    let body = body_html(markdown);
    let page = Page::of(template);
    let html = document(&body.html, template.font_size_pt);
    let faces = [(TEXT_FAMILY, &fonts.text), (STRONG_FAMILY, &fonts.strong), (HEADING_FAMILY, &fonts.heading)];
    let raw: BTreeMap<String, Vec<u8>> = faces.iter().map(|(name, bytes)| (name.to_string(), bytes.to_vec())).collect();
    let map: BTreeMap<String, Base64OrRaw> =
        raw.iter().map(|(name, bytes)| (name.clone(), Base64OrRaw::Raw(bytes.clone()))).collect();
    let pool = printpdf::html::build_font_pool(&raw, Some(&[NO_SYSTEM_FONTS]));
    let [top, right, bottom, left] = page.margins;
    let options = GeneratePdfOptions {
        page_width: Some(page.width),
        page_height: Some(page.height),
        margin_top: Some(top),
        margin_right: Some(right),
        margin_bottom: Some(bottom),
        margin_left: Some(left),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    let mut doc = PdfDocument::from_html_with_cache(&html, &BTreeMap::new(), &map, &options, &mut warnings, Some(pool))
        .map_err(|_| PdfError::Layout)?;
    if doc.pages.is_empty() {
        // Empty Markdown lays out no page; a PDF without pages is no document.
        doc.pages.push(PdfPage::new(Mm(page.width), Mm(page.height), Vec::new()));
    }
    let title = one_line(title);
    doc.metadata.info.document_title = title.clone();
    let logo = logo.and_then(|bytes| match logo_image(bytes) {
        Ok(image) => Some(image),
        Err(reason) => {
            eprintln!("torroworkplace: a PDF is made without its logo: the picture {reason}");
            None
        }
    });
    let lines = Lines {
        header: fill_line(&template.header, &title, date),
        footer: fill_line(&template.footer, &title, date),
        page_numbers: template.page_numbers,
        size: f32::from(template.font_size_pt) * DECORATION_SCALE,
    };
    decorate(&mut doc, &page, &lines, &fonts.text, logo)?;
    annotate_links(&mut doc, &body.links);
    Ok(doc.save(&PdfSaveOptions::default(), &mut Vec::new()))
}

/// The logo at `path` for [`render`], read only when it is a file of at most [`MAX_LOGO_BYTES`]. `None`, with a
/// warning in the log that does not name the file, when it is missing, too large or unreadable: the PDF is then
/// made without it.
pub fn read_logo(path: &Path) -> Option<Vec<u8>> {
    let fits = std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_LOGO_BYTES);
    if !fits {
        eprintln!("torroworkplace: a PDF is made without its logo: the file is missing or larger than 2 MB");
        return None;
    }
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            eprintln!("torroworkplace: a PDF is made without its logo: the file cannot be read ({})", err.kind());
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Fonts

/// The weights a family can have, lightest first.
const WEIGHTS: [Weight; 4] = [Weight::Regular, Weight::Medium, Weight::Semibold, Weight::Bold];

/// The file of `role` at `wanted`, else at the next heavier weight, else at the heaviest lighter one.
fn face(font: &FontSet, role: FontRole, wanted: Weight) -> Option<Cow<'static, [u8]>> {
    let at = WEIGHTS.iter().position(|weight| *weight == wanted).unwrap_or(0);
    let heavier = WEIGHTS[at..].iter();
    let lighter = WEIGHTS[..at].iter().rev();
    heavier.chain(lighter).find_map(|weight| torro_gui::design::font_file(font, role, *weight))
}

/// Font map keys the style sheet names. Each face is a family of its own (azul picks no face by `font-weight`).
const TEXT_FAMILY: &str = "torro-text";
const STRONG_FAMILY: &str = "torro-strong";
const HEADING_FAMILY: &str = "torro-heading";

/// A family no system has: printpdf then scans no font from the disk, and only the fonts handed in (and the PDF
/// base fonts) are used.
const NO_SYSTEM_FONTS: &str = "torro-no-system-fonts";

// ---------------------------------------------------------------------------------------------------------------
// Markdown → HTML

/// The HTML body set from the Markdown, and the addresses of its links in the order of their colours.
struct Body {
    html: String,
    links: Vec<String>,
}

/// An event of the Markdown, or a page break found in it.
enum Piece<'a> {
    Event(Event<'a>),
    PageBreak,
}

/// At most this many links are clickable: each needs a colour of its own ([`link_colour`]). Later links stay text.
const MAX_LINKS: usize = 256;

/// At most this many quotes and lists nest as markup, and as many inline tags (bold, italic, struck through,
/// links) inside each other; deeper levels go on flat, their text stays. azul-layout recurses once per level of the
/// HTML, so a document nested hundreds of levels deep would overflow the stack of the blocking thread and end the
/// service. And it lays out the content of nested flex rows again and again: from about 8 levels on, each further
/// one doubles the time a page of text takes (16 levels: minutes).
const MAX_NESTING: usize = 6;

fn body_html(markdown: &str) -> Body {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let pieces = with_page_breaks(Parser::new_ext(markdown, options));
    let mut writer = Writer::default();
    for (index, piece) in pieces.iter().enumerate() {
        match piece {
            Piece::PageBreak => writer.markup("<div class=\"pagebreak\"></div>\n"),
            Piece::Event(event) => writer.event(event, &pieces[index..]),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, writer.kept.into_iter());
    Body { html, links: writer.links }
}

/// The events with every line that is only `\pagebreak` taken out of its paragraph as a page break. Such a line
/// can stand alone between blank lines, or hang on a paragraph without a blank line (a soft break): then the
/// paragraph is split around it.
fn with_page_breaks<'a>(events: impl Iterator<Item = Event<'a>>) -> Vec<Piece<'a>> {
    let mut pieces = Vec::new();
    // The inline events of the open paragraph (paragraphs do not nest).
    let mut paragraph: Option<Vec<Event<'a>>> = None;
    for event in events {
        match event {
            Event::Start(Tag::Paragraph) => paragraph = Some(Vec::new()),
            Event::End(TagEnd::Paragraph) => split_paragraph(paragraph.take().unwrap_or_default(), &mut pieces),
            event => match paragraph.as_mut() {
                Some(inner) => inner.push(event),
                None => pieces.push(Piece::Event(event)),
            },
        }
    }
    pieces
}

/// One paragraph's inline events as paragraphs and page breaks. Lines are split only at breaks outside any inline
/// tag, so emphasis across lines stays whole.
fn split_paragraph<'a>(inner: Vec<Event<'a>>, pieces: &mut Vec<Piece<'a>>) {
    let mut lines: Vec<Vec<Event<'a>>> = vec![Vec::new()];
    // The break before line `i + 1`.
    let mut breaks: Vec<Event<'a>> = Vec::new();
    let mut depth = 0usize;
    for event in inner {
        match event {
            Event::SoftBreak | Event::HardBreak if depth == 0 => {
                breaks.push(event);
                lines.push(Vec::new());
                continue;
            }
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        if let Some(line) = lines.last_mut() {
            line.push(event);
        }
    }
    let mut current: Vec<Event<'a>> = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if is_page_break(&line) {
            flush_paragraph(&mut current, pieces);
            pieces.push(Piece::PageBreak);
            continue;
        }
        if !current.is_empty()
            && let Some(before) = index.checked_sub(1).and_then(|before| breaks.get(before))
        {
            current.push(before.clone());
        }
        current.extend(line);
    }
    flush_paragraph(&mut current, pieces);
}

fn flush_paragraph<'a>(current: &mut Vec<Event<'a>>, pieces: &mut Vec<Piece<'a>>) {
    if current.is_empty() {
        return;
    }
    pieces.push(Piece::Event(Event::Start(Tag::Paragraph)));
    pieces.extend(current.drain(..).map(Piece::Event));
    pieces.push(Piece::Event(Event::End(TagEnd::Paragraph)));
}

/// Whether a line of a paragraph is only `\pagebreak` (pulldown-cmark may hand its text in several pieces).
fn is_page_break(line: &[Event]) -> bool {
    let mut text = String::new();
    for event in line {
        match event {
            Event::Text(part) => text.push_str(part),
            _ => return false,
        }
    }
    text.trim() == "\\pagebreak"
}

/// Turns the events into the ones pulldown-cmark's HTML writer gets: what it writes itself (paragraphs, headings,
/// code, struck through, rules, escaped text) passes, lists, quotes, tables, bold, italic and links become this
/// module's own markup, and everything else falls away.
#[derive(Default)]
struct Writer<'a> {
    kept: Vec<Event<'a>>,
    /// The addresses of the clickable links, in the order of their colours.
    links: Vec<String>,
    /// Whether each open bold, italic, struck through or link became markup, so its end matches (a link only when
    /// it is clickable, any of them only within [`MAX_NESTING`]).
    inline: Vec<bool>,
    /// Whether each open quote and list became markup, so its end (and a list's items) match: only within
    /// [`MAX_NESTING`].
    blocks: Vec<bool>,
    /// The next number of each open list (`None` for bullets).
    lists: Vec<Option<u64>>,
    /// Width in percent and alignment of each column of the open table.
    columns: Vec<(u32, Alignment)>,
    /// The column of the next cell.
    column: usize,
}

impl<'a> Writer<'a> {
    /// Markup of this module. It never carries text from the Markdown.
    fn markup(&mut self, markup: impl Into<String>) {
        self.kept.push(Event::Html(CowStr::Boxed(markup.into().into_boxed_str())));
    }

    /// `event`, the first of `rest` (which a table looks ahead in for its column widths).
    fn event(&mut self, event: &Event<'a>, rest: &[Piece<'a>]) {
        match event {
            Event::Text(_) | Event::Code(_) | Event::SoftBreak | Event::HardBreak | Event::Rule => {
                self.kept.push(event.clone())
            }
            Event::Start(Tag::Paragraph | Tag::Heading { .. })
            | Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) => self.kept.push(event.clone()),
            // Without the info string: it would only become a class name.
            Event::Start(Tag::CodeBlock(_)) => self.kept.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Indented))),
            Event::Start(Tag::Strikethrough) => {
                if self.open_inline(true) {
                    self.kept.push(event.clone());
                }
            }
            Event::End(TagEnd::Strikethrough) => {
                if self.inline.pop().unwrap_or(false) {
                    self.kept.push(event.clone());
                }
            }
            Event::Start(Tag::Strong) => {
                if self.open_inline(true) {
                    self.markup("<span class=\"strong\">");
                }
            }
            Event::Start(Tag::Emphasis) => {
                if self.open_inline(true) {
                    self.markup("<span class=\"em\">");
                }
            }
            Event::Start(Tag::Link { link_type, dest_url, .. }) => self.open_link(*link_type, dest_url),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                if self.inline.pop().unwrap_or(false) {
                    self.markup("</span>");
                }
            }
            Event::Start(Tag::List(first)) => {
                self.lists.push(*first);
                if self.open_block() {
                    self.markup("<div class=\"list\">");
                }
            }
            Event::End(TagEnd::List(_)) => {
                self.lists.pop();
                if self.blocks.pop().unwrap_or(false) {
                    self.markup("</div>\n");
                }
            }
            Event::Start(Tag::Item) => self.open_item(),
            Event::End(TagEnd::Item) => {
                if self.blocks.last().copied().unwrap_or(false) {
                    self.markup("</div></div>\n");
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                if self.open_block() {
                    self.markup("<div class=\"quote\"><div class=\"bar\"></div><div class=\"content\">");
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if self.blocks.pop().unwrap_or(false) {
                    self.markup("</div></div>\n");
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                self.columns =
                    table_widths(rest, alignments.len()).into_iter().zip(alignments.iter().copied()).collect();
                self.markup("<div class=\"table\">");
            }
            Event::End(TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow) => self.markup("</div>\n"),
            Event::Start(Tag::TableHead) => {
                self.column = 0;
                self.markup("<div class=\"row head\">");
            }
            Event::Start(Tag::TableRow) => {
                self.column = 0;
                self.markup("<div class=\"row\">");
            }
            Event::Start(Tag::TableCell) => {
                let (width, alignment) = self.columns.get(self.column).copied().unwrap_or((10, Alignment::None));
                let align = match alignment {
                    Alignment::Right => "right",
                    Alignment::Center => "center",
                    Alignment::Left | Alignment::None => "left",
                };
                self.column += 1;
                self.markup(format!("<div class=\"cell\" style=\"width: {width}%; text-align: {align}\">"));
            }
            Event::End(TagEnd::TableCell) => self.markup("</div>"),
            // Raw HTML, pictures (their alt text passes as text) and whatever this module does not set.
            _ => {}
        }
    }

    /// Opens a quote or list: whether it becomes markup, which it does only within [`MAX_NESTING`].
    fn open_block(&mut self) -> bool {
        let nested = self.blocks.iter().filter(|markup| **markup).count();
        let markup = nested < MAX_NESTING;
        self.blocks.push(markup);
        markup
    }

    /// Opens an inline tag that `wanted` to become markup: whether it does, which it does only within
    /// [`MAX_NESTING`].
    fn open_inline(&mut self, wanted: bool) -> bool {
        let nested = self.inline.iter().filter(|markup| **markup).count();
        let markup = wanted && nested < MAX_NESTING;
        self.inline.push(markup);
        markup
    }

    /// An item of the open list: a marker beside its content, or, in a list too deep for markup, a line of its own
    /// that starts with the marker.
    fn open_item(&mut self) {
        let marker = match self.lists.last_mut() {
            Some(Some(number)) => {
                let marker = format!("{number}.");
                *number += 1;
                marker
            }
            _ => "•".to_string(),
        };
        if self.blocks.last().copied().unwrap_or(false) {
            self.markup(format!("<div class=\"item\"><div class=\"marker\">{marker}</div><div class=\"content\">"));
        } else {
            self.kept.push(Event::HardBreak);
            self.kept.push(Event::Text(CowStr::Boxed(format!("{marker} ").into_boxed_str())));
        }
    }

    /// A link becomes clickable only with `http`, `https` or `mailto`; otherwise its text stays plain. Its address
    /// never reaches the HTML: the link gets a colour of its own, found again after the layout.
    fn open_link(&mut self, link_type: LinkType, dest_url: &str) {
        let target = match link_type {
            LinkType::Email => format!("mailto:{dest_url}"),
            _ => dest_url.to_string(),
        };
        let lower = target.to_ascii_lowercase();
        let wanted = ["http://", "https://", "mailto:"].iter().any(|scheme| lower.starts_with(scheme))
            && self.links.len() < MAX_LINKS;
        let allowed = self.open_inline(wanted);
        if allowed {
            let (red, green) = link_colour(self.links.len());
            self.links.push(target);
            self.markup(format!("<span class=\"link\" style=\"color: rgb({red}, {green}, {LINK_BLUE})\">"));
        }
    }
}

/// Column widths in percent from the longest text of each column in the table that starts `rest`, each counted
/// as 4 to 40 letters, so a long cell wraps instead of squeezing the others to a few letters.
fn table_widths(rest: &[Piece], count: usize) -> Vec<u32> {
    let mut longest = vec![1usize; count];
    let mut column = 0;
    for piece in rest {
        let Piece::Event(event) = piece else { continue };
        match event {
            Event::End(TagEnd::Table) => break,
            Event::Start(Tag::TableHead | Tag::TableRow) => column = 0,
            Event::End(TagEnd::TableCell) => column += 1,
            Event::Text(text) | Event::Code(text) => {
                if let Some(width) = longest.get_mut(column) {
                    *width = (*width).max(text.chars().count());
                }
            }
            _ => {}
        }
    }
    let capped: Vec<usize> = longest.iter().map(|width| (*width).clamp(4, 40)).collect();
    let total = capped.iter().sum::<usize>().max(1);
    capped.iter().map(|width| u32::try_from(width * 100 / total).unwrap_or(100)).collect()
}

/// The blue of every link; red and green tell links apart ([`link_colour`]).
const LINK_BLUE: u8 = 208;

/// Red and green of link `index`: 16 × 16 shades of #0b57d0 that look alike.
fn link_colour(index: usize) -> (u8, u8) {
    let index = u8::try_from(index % MAX_LINKS).unwrap_or(0);
    (4 + index / 16, 80 + index % 16)
}

/// The link whose colour `rgb` is, among the first `count`.
fn link_of(rgb: &Rgb, count: usize) -> Option<usize> {
    let channel = |value: f32| (value * 255.0).round() as i32;
    if channel(rgb.b) != i32::from(LINK_BLUE) {
        return None;
    }
    (0..count).find(|index| {
        let (red, green) = link_colour(*index);
        i32::from(red) == channel(rgb.r) && i32::from(green) == channel(rgb.g)
    })
}

/// The whole HTML page: the style sheet and `body`. Body text is `size` points, headings scale with it.
fn document(body: &str, size: u8) -> String {
    let size = f32::from(size);
    format!(
        r#"<html><head><style>
body {{ font-family: "{TEXT_FAMILY}"; font-size: {size}pt; line-height: 1.4; color: #1f2328; }}
h1, h2, h3, h4, h5, h6 {{ font-family: "{HEADING_FAMILY}"; }}
.strong, .head {{ font-family: "{STRONG_FAMILY}"; }}
h1 {{ font-size: {h1}pt; margin: 0 0 8pt 0; }}
h2 {{ font-size: {h2}pt; margin: 14pt 0 6pt 0; }}
h3, h4, h5, h6 {{ font-size: {h3}pt; margin: 12pt 0 4pt 0; }}
p {{ margin: 0 0 6pt 0; }}
del {{ text-decoration: line-through; }}
.link {{ text-decoration: underline; }}
code {{ font-family: "Courier"; font-size: {code}pt; }}
pre {{ background-color: #f3f4f6; padding: 6pt; margin: 0 0 8pt 0; white-space: pre; }}
.list {{ margin: 0 0 6pt 0; }}
.item {{ display: flex; flex-direction: row; }}
.marker {{ width: {marker}pt; flex-shrink: 0; }}
.content {{ flex-grow: 1; }}
.quote {{ display: flex; flex-direction: row; margin: 0 0 8pt 0; color: #4b5563; }}
.bar {{ width: 2pt; background-color: #9aa0a6; flex-shrink: 0; margin-right: 8pt; }}
.table {{ margin: 0 0 8pt 0; border-top: 1px solid #9aa0a6; }}
.row {{ display: flex; flex-direction: row; border-bottom: 1px solid #9aa0a6; }}
.head {{ background-color: #eef0f3; }}
.cell {{ padding: 2pt 4pt; }}
hr {{ border: 0; border-top: 1px solid #9aa0a6; margin: 8pt 0; }}
.pagebreak {{ page-break-before: always; }}
</style></head><body>
{body}
</body></html>"#,
        h1 = size * 1.6,
        h2 = size * 1.3,
        h3 = size * 1.1,
        code = size * 0.9,
        marker = size * 1.6,
    )
}

// ---------------------------------------------------------------------------------------------------------------
// Header, footer, page number, logo

/// Millimetres in a point.
const MM_PER_PT: f32 = 25.4 / 72.0;

/// Header and footer are this much of the body text.
const DECORATION_SCALE: f32 = 0.8;

/// The grey of header and footer.
const DECORATION_GREY: (f32, f32, f32) = (0.35, 0.38, 0.42);

/// Room between a line and the logo or the page number, in millimetres.
const GAP_MM: f32 = 4.0;

/// The paper of a template in millimetres.
struct Page {
    width: f32,
    height: f32,
    /// Top, right, bottom, left.
    margins: [f32; 4],
}

impl Page {
    fn of(template: &PdfTemplate) -> Page {
        let (width, height) = match template.paper {
            Paper::A4 => (210.0, 297.0),
            Paper::A5 => (148.0, 210.0),
            Paper::Letter => (215.9, 279.4),
        };
        Page { width, height, margins: template.margins_mm.map(f32::from) }
    }
}

/// What [`decorate`] writes on every page.
struct Lines {
    header: String,
    footer: String,
    page_numbers: bool,
    /// Their size in points.
    size: f32,
}

/// `line` of a template with `{title}` and `{date}` filled in. The title is put in last, so a title holding
/// `{date}` stays as it is.
fn fill_line(line: &str, title: &str, date: NaiveDate) -> String {
    if line.is_empty() {
        return String::new();
    }
    let date = torro_gui::datetime::format_long_date(date);
    let parts: Vec<String> = line.split("{title}").map(|part| part.replace("{date}", &date)).collect();
    one_line(&parts.join(title))
}

/// `text` on one line: control characters become spaces.
fn one_line(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// The logo as printpdf draws it, or why it is left out (for the log, never the file's name).
fn logo_image(bytes: &[u8]) -> Result<RawImage, &'static str> {
    if bytes.len() as u64 > MAX_LOGO_BYTES {
        return Err("is larger than 2 MB");
    }
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
    if !png && !jpeg {
        return Err("is neither PNG nor JPEG");
    }
    RawImage::decode_from_bytes(bytes, &mut Vec::new()).map_err(|_| "cannot be decoded")
}

/// Where the logo sits, in millimetres from the bottom left: right in the header, centred in the top margin, half
/// the margin high (at most 12 mm) and at most a third of the text's width wide.
struct Placed {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn place_logo(image: &RawImage, page: &Page) -> Placed {
    let [top, right, _, left] = page.margins;
    let ratio = image.width.max(1) as f32 / image.height.max(1) as f32;
    let mut height = (top * 0.5).min(12.0);
    let mut width = height * ratio;
    let widest = (page.width - left - right) / 3.0;
    if width > widest {
        width = widest;
        height = width / ratio;
    }
    Placed { x: page.width - right - width, y: page.height - top / 2.0 - height / 2.0, width, height }
}

/// Draws header, logo, footer and page number on every page with printpdf's text layer, in the margins of the
/// template. A line too long for its room ends in "…".
fn decorate(
    doc: &mut PdfDocument,
    page: &Page,
    lines: &Lines,
    text_font: &[u8],
    logo: Option<RawImage>,
) -> Result<(), PdfError> {
    if lines.header.is_empty() && lines.footer.is_empty() && !lines.page_numbers && logo.is_none() {
        return Ok(());
    }
    let font = ParsedFont::from_bytes(text_font, 0, &mut Vec::new()).ok_or(PdfError::Font)?;
    let font_id = doc.add_font(&font);
    let [top, right, bottom, left] = page.margins;
    let content = page.width - left - right;
    let size = lines.size;
    // The baseline that centres a line's capitals in a margin.
    let cap = size * 0.7 * MM_PER_PT;
    let header_y = page.height - top / 2.0 - cap / 2.0;
    let footer_y = bottom / 2.0 - cap / 2.0;

    let logo = logo.map(|image| {
        let placed = place_logo(&image, page);
        (doc.add_image(&image), placed, image.width, image.height)
    });
    let header_room = content - logo.as_ref().map_or(0.0, |(_, placed, _, _)| placed.width + GAP_MM);
    let header = fit(&font, &lines.header, size, header_room);

    let total = doc.pages.len();
    let number = |n: usize| trf("Page {n} of {m}", &[("n", &n), ("m", &total)]);
    // The widest number leaves the footer the same room on every page.
    let number_room = if lines.page_numbers { width_mm(&font, &number(total), size) + GAP_MM } else { 0.0 };
    let footer = fit(&font, &lines.footer, size, content - number_room);

    for (index, pdf_page) in doc.pages.iter_mut().enumerate() {
        if let Some((id, placed, pixels_wide, pixels_high)) = &logo {
            // An image is first drawn at its size at 300 dpi; scale that to the placed size.
            let natural = |pixels: usize| pixels.max(1) as f32 / 300.0 * 72.0;
            pdf_page.ops.extend([
                Op::SaveGraphicsState,
                Op::UseXobject {
                    id: id.clone(),
                    transform: XObjectTransform {
                        translate_x: Some(Mm(placed.x).into_pt()),
                        translate_y: Some(Mm(placed.y).into_pt()),
                        scale_x: Some(Mm(placed.width).into_pt().0 / natural(*pixels_wide)),
                        scale_y: Some(Mm(placed.height).into_pt().0 / natural(*pixels_high)),
                        ..Default::default()
                    },
                },
                Op::RestoreGraphicsState,
            ]);
        }
        if !header.is_empty() {
            pdf_page.ops.extend(text_at(&font_id, size, left, header_y, &header));
        }
        if !footer.is_empty() {
            pdf_page.ops.extend(text_at(&font_id, size, left, footer_y, &footer));
        }
        if lines.page_numbers {
            let number = number(index + 1);
            let x = page.width - right - width_mm(&font, &number, size);
            pdf_page.ops.extend(text_at(&font_id, size, x, footer_y, &number));
        }
    }
    Ok(())
}

/// `text` cut to fit `room` millimetres at `size` points, ending in "…" when it was cut.
fn fit(font: &ParsedFont, text: &str, size: f32, room: f32) -> String {
    if width_mm(font, text, size) <= room {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let cut = format!("{}…", chars.iter().collect::<String>().trim_end());
        if width_mm(font, &cut, size) <= room {
            return cut;
        }
    }
    String::new()
}

/// The width of `text` in millimetres from the glyphs' advances.
fn width_mm(font: &ParsedFont, text: &str, size: f32) -> f32 {
    let advance: usize = text
        .chars()
        .filter_map(|c| font.lookup_glyph_index(u32::from(c)))
        .filter_map(|gid| font.get_glyph_width_internal(gid))
        .sum();
    advance as f32 / units_per_em(font) * size * MM_PER_PT
}

fn units_per_em(font: &ParsedFont) -> f32 {
    f32::from(font.font_metrics.units_per_em.max(1))
}

/// The operations that write `text` at `x`, `y` (millimetres, baseline) in grey.
fn text_at(font: &printpdf::FontId, size: f32, x: f32, y: f32, text: &str) -> Vec<Op> {
    let (r, g, b) = DECORATION_GREY;
    vec![
        Op::SaveGraphicsState,
        Op::StartTextSection,
        Op::SetTextCursor { pos: Point::new(Mm(x), Mm(y)) },
        Op::SetFont { font: PdfFontHandle::External(font.clone()), size: Pt(size) },
        Op::SetFillColor { col: Color::Rgb(Rgb { r, g, b, icc_profile: None }) },
        Op::ShowText { items: vec![TextItem::Text(text.to_string())] },
        Op::EndTextSection,
        Op::RestoreGraphicsState,
    ]
}

// ---------------------------------------------------------------------------------------------------------------
// Clickable links

/// The glyphs of one link on one line, in points.
struct Run {
    link: usize,
    left: f32,
    right: f32,
    baseline: f32,
    size: f32,
}

/// Lays a link annotation over every line of every link: the layout set their glyphs in the link's own colour.
/// Without a frame (a PDF viewer draws one by default).
fn annotate_links(doc: &mut PdfDocument, links: &[String]) {
    if links.is_empty() {
        return;
    }
    for pdf_page in &mut doc.pages {
        let runs = link_runs(&pdf_page.ops, &doc.resources, links.len());
        for run in runs {
            let Some(target) = links.get(run.link) else { continue };
            let rect = Rect {
                x: Pt(run.left),
                y: Pt(run.baseline - run.size * 0.25),
                width: Pt(run.right - run.left),
                height: Pt(run.size * 1.1),
                mode: None,
                winding_order: None,
            };
            pdf_page.ops.push(Op::LinkAnnotation {
                link: LinkAnnotation::new(
                    rect,
                    Actions::Uri(target.clone()),
                    Some(BorderArray::Solid([0.0, 0.0, 0.0])),
                    None,
                    None,
                ),
            });
        }
    }
}

/// The runs of link glyphs in `ops`: consecutive glyphs of one link on one baseline. The right edge is the last
/// glyph's position plus its own advance.
fn link_runs(ops: &[Op], resources: &PdfResources, count: usize) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut link = None;
    let mut size = 0.0;
    let mut font: Option<&ParsedFont> = None;
    let mut at = (0.0, 0.0);
    for op in ops {
        match op {
            Op::SetFillColor { col: Color::Rgb(rgb) } => link = link_of(rgb, count),
            Op::SetFillColor { .. } => link = None,
            Op::SetFont { font: handle, size: Pt(points) } => {
                size = *points;
                font = match handle {
                    PdfFontHandle::External(id) => resources.fonts.map.get(id).map(|found| &found.parsed_font),
                    PdfFontHandle::Builtin(_) => None,
                };
            }
            Op::SetTextMatrix { matrix: TextMatrix::Raw([_, _, _, _, x, y]) } => at = (*x, *y),
            Op::ShowText { items } => {
                let Some(index) = link else { continue };
                let (x, y) = at;
                let right = x + advance_pt(font, items, size);
                match runs.last_mut() {
                    Some(run) if run.link == index && (run.baseline - y).abs() < 0.5 => {
                        run.right = run.right.max(right)
                    }
                    _ => runs.push(Run { link: index, left: x, right, baseline: y, size }),
                }
            }
            _ => {}
        }
    }
    runs
}

/// How far the glyphs of `items` advance, in points; half the font size per glyph when the font is unknown.
fn advance_pt(font: Option<&ParsedFont>, items: &[TextItem], size: f32) -> f32 {
    let Some(font) = font else {
        let glyphs: usize = items
            .iter()
            .map(|item| match item {
                TextItem::Text(text) => text.chars().count(),
                TextItem::GlyphIds(glyphs) => glyphs.len(),
                TextItem::Offset(_) => 0,
            })
            .sum();
        return glyphs as f32 * size * 0.5;
    };
    let units: usize = items
        .iter()
        .flat_map(|item| -> Vec<u16> {
            match item {
                TextItem::Text(text) => text.chars().filter_map(|c| font.lookup_glyph_index(u32::from(c))).collect(),
                TextItem::GlyphIds(glyphs) => glyphs.iter().map(|glyph| glyph.gid).collect(),
                TextItem::Offset(_) => Vec::new(),
            }
        })
        .filter_map(|gid| font.get_glyph_width_internal(gid))
        .sum();
    units as f32 / units_per_em(font) * size
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use printpdf::PdfParseOptions;
    use torro_core::settings::PdfTemplateId;

    use super::*;

    const TITLE: &str = "Angebot für Müller & Söhne";

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tools")
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(fixtures().join(name)).expect("fixture")
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 19).expect("a day")
    }

    fn report() -> PdfTemplate {
        PdfTemplate::standard(PdfTemplateId::Report)
    }

    fn set(markdown: &str, template: &PdfTemplate) -> Vec<u8> {
        render(TITLE, markdown, template, &PdfFonts::standard(), None, day()).expect("a PDF")
    }

    /// The PDF read back with printpdf's parser (lopdf underneath), and the text of each page.
    struct Read {
        doc: PdfDocument,
        pages: Vec<String>,
    }

    impl Read {
        fn all(&self) -> String {
            self.pages.join("\n")
        }

        fn annotations(&self) -> usize {
            let ops = self.doc.pages.iter().flat_map(|page| page.ops.iter());
            ops.filter(|op| matches!(op, Op::LinkAnnotation { .. })).count()
        }

        fn images(&self) -> usize {
            let ops = self.doc.pages.iter().flat_map(|page| page.ops.iter());
            ops.filter(|op| matches!(op, Op::UseXobject { .. })).count()
        }
    }

    fn read(bytes: &[u8]) -> Read {
        let doc = PdfDocument::parse(bytes, &PdfParseOptions::default(), &mut Vec::new()).expect("a valid PDF");
        let pages = doc.extract_text().into_iter().map(|chunks| chunks.join("\n")).collect();
        Read { doc, pages }
    }

    fn page_number(n: usize, m: usize) -> String {
        trf("Page {n} of {m}", &[("n", &n), ("m", &m)])
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }

    #[test]
    fn every_markdown_kind_sets_a_valid_pdf() {
        let kinds = [
            ("# Überschrift eins", "Überschrift eins"),
            ("## Überschrift zwei", "Überschrift zwei"),
            ("### Überschrift drei", "Überschrift drei"),
            ("Ein Absatz mit Grüßen aus Köln.", "Ein Absatz mit Grüßen aus Köln."),
            ("Das ist **fett** gesetzt.", "fett"),
            ("Das ist *kursiv* gesetzt.", "kursiv"),
            ("Das ist ~~gestrichen~~.", "gestrichen"),
            ("Mit `code` darin.", "code"),
            ("```rust\nlet größe = 1;\n```", "größe"),
            ("- außen\n  - innen\n    1. ganz innen", "ganz innen"),
            ("3. dritte\n4. vierte", "4."),
            ("| Leistung | Preis |\n|---|---:|\n| Übergabe | 500,00 € |", "Übergabe"),
            ("> Ein Zitat über Bäume", "Ein Zitat über Bäume"),
            ("oben\n\n---\n\nunten", "unten"),
            ("Ein [Link](https://torro.example/ä) im Satz.", "Link"),
        ];
        for (markdown, shown) in kinds {
            let pdf = read(&set(markdown, &report()));
            assert_eq!(pdf.pages.len(), 1, "{markdown}");
            let text = pdf.all();
            assert!(text.contains(shown), "{markdown}: {text}");
            assert!(text.contains(TITLE), "the title in the header: {markdown}");
            assert!(text.contains(&page_number(1, 1)), "the page number: {markdown}");
        }
    }

    #[test]
    fn the_fixture_keeps_umlauts_title_header_footer_page_numbers_and_links() {
        let mut template = report();
        template.footer = "Köln, {date}".into();
        let started = std::time::Instant::now();
        let bytes = set(&fixture("all.md"), &template);
        // The time a set takes in a debug build, for the report of WZ.3 (`--nocapture`).
        eprintln!("all.md set in {} ms", started.elapsed().as_millis());
        let pdf = read(&bytes);
        assert_eq!(pdf.pages.len(), 2, "the page break before the prices");
        assert_eq!(pdf.doc.metadata.info.document_title, TITLE);
        let date = torro_gui::datetime::format_long_date(day());
        for (index, page) in pdf.pages.iter().enumerate() {
            assert!(page.contains(TITLE), "header on page {}", index + 1);
            assert!(page.contains(&format!("Köln, {date}")), "footer on page {}", index + 1);
            assert!(page.contains(&page_number(index + 1, 2)), "page number on page {}", index + 1);
        }
        let text = pdf.all();
        for word in ["Weiß", "Grüße", "Söhne", "Übergabe", "Durchführung", "Beratung Größe S", "1.200,00 €"] {
            assert!(text.contains(word), "{word}: {text}");
        }
        assert_eq!(pdf.annotations(), 3, "https, mailto and the e-mail autolink are clickable");
        assert!(contains(&bytes, b"https://torro.example/angebot"));
        assert!(contains(&bytes, b"mailto:info@example.org"));
    }

    #[test]
    fn raw_html_pictures_and_other_links_do_not_appear() {
        let markdown = "Text mit <b>Markup</b> und <span style=\"color:red\">Farbe</span>.\n\n\
                        <div onclick=\"steal()\">Block</div>\n\n\
                        ![Ein Bild](https://torro.example/bild.png)\n\n\
                        [klick](javascript:alert(1)) [datei](file:///etc/passwd) [web](https://torro.example/a)";
        let body = body_html(markdown);
        for gone in ["<b>", "<span style", "onclick", "<img", "javascript", "file:", "https://"] {
            assert!(!body.html.contains(gone), "{gone} in {}", body.html);
        }
        assert_eq!(body.links, ["https://torro.example/a"]);

        let bytes = set(markdown, &report());
        assert!(!contains(&bytes, b"javascript"));
        assert!(!contains(&bytes, b"file:///"));
        let pdf = read(&bytes);
        let text = pdf.all();
        for kept in ["Markup", "Farbe", "Ein Bild", "klick", "datei", "web"] {
            assert!(text.contains(kept), "the text of {kept} stays: {text}");
        }
        assert!(!text.contains("<b>") && !text.contains("steal"), "{text}");
        assert_eq!(pdf.annotations(), 1, "only the https link is clickable");
    }

    #[test]
    fn a_page_break_starts_a_new_page() {
        let pdf = read(&set(&fixture("pagebreak.md"), &report()));
        assert_eq!(pdf.pages.len(), 2);
        assert!(pdf.pages[0].contains("Erste Seite mit Grüßen."));
        assert!(pdf.pages[1].contains("Zweite Seite."));

        // Without blank lines the line still breaks the page, and the paragraph around it is split.
        let pdf = read(&set(&fixture("pagebreak-soft.md"), &report()));
        assert_eq!(pdf.pages.len(), 2);
        assert!(pdf.pages[0].contains("noch eine Zeile."), "{}", pdf.pages[0]);
        assert!(pdf.pages[1].contains("Zweite Seite ohne Leerzeile."), "{}", pdf.pages[1]);
        assert!(!pdf.all().contains("pagebreak"));

        for markdown in ["```\n\\pagebreak\n```", "Kein \\pagebreak mitten im Satz."] {
            let pdf = read(&set(markdown, &report()));
            assert_eq!(pdf.pages.len(), 1, "{markdown}");
            assert!(pdf.all().contains("pagebreak"), "{markdown} stays text");
        }
    }

    #[test]
    fn a_page_break_inside_emphasis_is_text() {
        let pieces = with_page_breaks(Parser::new("*schräg\n\\pagebreak\nweiter*"));
        assert!(!pieces.iter().any(|piece| matches!(piece, Piece::PageBreak)), "lines split only outside tags");
    }

    #[test]
    fn every_template_on_every_paper() {
        let papers = [(Paper::A4, 595.3, 841.9), (Paper::A5, 419.5, 595.3), (Paper::Letter, 612.0, 792.0)];
        for id in PdfTemplateId::ALL {
            for (paper, width, height) in papers {
                let mut template = PdfTemplate::standard(id);
                template.paper = paper;
                let pdf = read(&set(&fixture("pagebreak.md"), &template));
                assert_eq!(pdf.pages.len(), 2, "{id:?} on {paper:?}");
                let media = &pdf.doc.pages[0].media_box;
                assert!((media.width.0 - width).abs() < 1.0, "{id:?} on {paper:?}: {}", media.width.0);
                assert!((media.height.0 - height).abs() < 1.0, "{id:?} on {paper:?}: {}", media.height.0);
                let text = pdf.all();
                assert!(text.contains("Grüßen"), "{id:?} on {paper:?}");
                assert_eq!(text.contains(TITLE), !template.header.is_empty(), "{id:?}: header");
                assert_eq!(text.contains(&page_number(2, 2)), template.page_numbers, "{id:?}: page numbers");
            }
        }
    }

    #[test]
    fn an_empty_document_has_one_page() {
        let pdf = read(&set("", &PdfTemplate::standard(PdfTemplateId::Note)));
        assert_eq!(pdf.pages.len(), 1);
    }

    #[test]
    fn a_png_or_jpeg_logo_is_drawn_anything_else_is_left_out() {
        let note = PdfTemplate::standard(PdfTemplateId::Note);
        for name in ["logo.png", "logo.jpg"] {
            let logo = std::fs::read(fixtures().join(name)).expect("logo");
            let bytes = render(TITLE, "Mit Logo.", &note, &PdfFonts::standard(), Some(&logo), day()).expect("a PDF");
            assert_eq!(read(&bytes).images(), 1, "{name} on the one page");
        }
        let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec();
        let mut huge = b"\x89PNG\r\n\x1a\n".to_vec();
        huge.resize(usize::try_from(MAX_LOGO_BYTES).expect("fits") + 1, 0);
        for logo in [gif, huge, b"\x89PNG\r\n\x1a\nbroken".to_vec()] {
            let bytes = render(TITLE, "Ohne Logo.", &note, &PdfFonts::standard(), Some(&logo), day()).expect("a PDF");
            let pdf = read(&bytes);
            assert_eq!(pdf.images(), 0);
            assert!(pdf.all().contains("Ohne Logo."));
        }
    }

    #[test]
    fn a_logo_file_is_read_only_up_to_two_megabytes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let small = dir.path().join("logo.png");
        std::fs::copy(fixtures().join("logo.png"), &small).expect("copy");
        assert_eq!(read_logo(&small), std::fs::read(&small).ok());
        let large = dir.path().join("large.png");
        std::fs::write(&large, vec![0u8; usize::try_from(MAX_LOGO_BYTES).expect("fits") + 1]).expect("large");
        assert_eq!(read_logo(&large), None);
        assert_eq!(read_logo(&dir.path().join("missing.png")), None);
        assert_eq!(read_logo(dir.path()), None, "a folder is no logo");
    }

    #[test]
    fn bold_and_headings_take_the_next_heavier_weight() {
        assert_eq!(PdfFonts::of(&FontSet::standard()), Some(PdfFonts::standard()), "Inter: SemiBold for bold");

        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts");
        let lato = PdfFonts::of_font(Some(&folder), "channelr");
        let fonts = folder.join("channelr");
        let regular = std::fs::read(fonts.join("Lato-Regular.ttf")).expect("Lato");
        let bold = std::fs::read(fonts.join("Lato-Bold.ttf")).expect("Lato Bold");
        assert_eq!(lato.text.as_ref(), regular.as_slice());
        assert_eq!(lato.strong.as_ref(), bold.as_slice());
        assert_eq!(lato.heading.as_ref(), bold.as_slice(), "no SemiBold: the next heavier one");
        let pdf = read(&render(TITLE, "# Grüße\n\n**fett**", &report(), &lato, None, day()).expect("a PDF"));
        assert!(pdf.all().contains("Grüße"));

        assert_eq!(PdfFonts::of_font(Some(&folder), "no-such-pack"), PdfFonts::standard());
        assert_eq!(PdfFonts::of_font(None, "channelr"), PdfFonts::standard());
    }

    #[test]
    fn a_face_printpdf_cannot_read_falls_back_to_the_standard() {
        // The pack's check reads fonts with another parser than printpdf; and its folder may change after it.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("Broken-Regular.ttf");
        std::fs::write(&path, b"\0\x01\0\0 no font").expect("written");
        let mut font = FontSet::standard();
        let face = Some(design::Face::File(path));
        font.fonts.text =
            design::Font { family: "Broken".into(), regular: face, medium: None, semibold: None, bold: None };
        let broken =
            PdfFonts { text: face_bytes(&font), strong: face_bytes(&font), heading: PdfFonts::standard().heading };
        assert_eq!(render(TITLE, "Text.", &report(), &broken, None, day()), Err(PdfError::Font), "no PDF in it");

        assert_eq!(PdfFonts::of(&font), None);
        let fonts = PdfFonts::of_pack(Some(&font));
        assert_eq!(fonts, PdfFonts::standard());
        let pdf = read(&render(TITLE, "Trotzdem gesetzt.", &report(), &fonts, None, day()).expect("a PDF"));
        assert!(pdf.all().contains("Trotzdem gesetzt."));
    }

    /// Issue #299: a font of the computer is never embedded in a PDF – that would hand it on. The PDF is
    /// set in the standard's Inter.
    #[test]
    fn a_font_of_the_computer_is_set_in_inter() {
        let mut font = FontSet::standard();
        let system = design::Font {
            family: "Avenir Next".into(),
            regular: Some(design::Face::System),
            medium: Some(design::Face::System),
            semibold: Some(design::Face::System),
            bold: Some(design::Face::System),
        };
        (font.fonts.text, font.fonts.heading) = (system.clone(), system);
        assert_eq!(PdfFonts::of(&font), None);
        assert_eq!(PdfFonts::of_pack(Some(&font)), PdfFonts::standard());
        assert_eq!(PdfFonts::of_font(None, "system:Torro Test Font That Is Never Installed"), PdfFonts::standard());
    }

    fn face_bytes(font: &FontSet) -> Cow<'static, [u8]> {
        face(font, FontRole::Text, Weight::Regular).expect("the file")
    }

    #[test]
    fn only_the_fonts_handed_in_are_embedded() {
        let bytes = set("Nur eingebettete Schriften, keine des Systems.", &report());
        for system in [&b"Helvetica"[..], b"Arial", b"Times", b"SFNS", b"DejaVu", b"Segoe"] {
            assert!(!contains(&bytes, system), "{}", String::from_utf8_lossy(system));
        }
        assert!(contains(&bytes, b"Inter-Regular"));
    }

    #[test]
    fn header_and_footer_fill_in_title_and_date_and_fit_their_room() {
        let date = torro_gui::datetime::format_long_date(day());
        assert_eq!(fill_line("{title} · {date}", "Plan {date}", day()), format!("Plan {{date}} · {date}"));
        assert_eq!(fill_line("", TITLE, day()), "");
        assert_eq!(one_line("zwei\nZeilen\t"), "zwei Zeilen");

        let font = ParsedFont::from_bytes(&PdfFonts::standard().text, 0, &mut Vec::new()).expect("Inter");
        let long = "Sehr lange Kopfzeile ".repeat(10);
        let cut = fit(&font, &long, 9.0, 60.0);
        assert!(cut.ends_with('…') && width_mm(&font, &cut, 9.0) <= 60.0, "{cut}");
        assert_eq!(fit(&font, "Kurz", 9.0, 60.0), "Kurz");

        let mut template = report();
        template.header = long.trim().to_string();
        let pdf = read(&set("Text.", &template));
        assert!(pdf.all().contains('…'), "a header too long ends in an ellipsis");
    }

    #[test]
    fn tables_take_widths_and_alignment_from_the_markdown() {
        // Longest texts 2 (counted as 4), 25 and 5 letters of 34.
        let body = body_html("| Nr | Leistung mit langem Namen | Preis |\n|:-:|---|--:|\n| 1 | Beratung | 9 € |");
        assert!(body.html.contains("width: 11%; text-align: center"), "{}", body.html);
        assert!(body.html.contains("width: 73%; text-align: left"), "{}", body.html);
        assert!(body.html.contains("width: 14%; text-align: right"), "{}", body.html);
        assert_eq!(body.html.matches("class=\"row").count(), 2, "head and one row");
    }

    /// Markdown nested `depth` levels deep in every way that nests: quotes, lists, both mixed, a table at the
    /// bottom of a list, and emphasis, struck through and links inside each other.
    fn deep_documents(depth: usize) -> Vec<(&'static str, String)> {
        let quotes = format!("{} tief unten", ">".repeat(depth));
        let bullets = format!("{}tief unten", "- ".repeat(depth));
        let numbers = format!("{}tief unten", "1. ".repeat(depth));
        // A list first: with `> - ` 500 times, the last line ends exactly at the foot of page 11, and azul-layout
        // drops a line there under six closing levels (a gap of its paging, not of the nesting).
        let mixed = format!("{}tief unten", "- > ".repeat(depth / 2));
        let table = format!(
            "{}| A | B |\n{}|---|---|\n{}| tief | unten |",
            "- ".repeat(depth),
            "  ".repeat(depth),
            "  ".repeat(depth)
        );
        let emphasis = format!("{}tief unten{}", "*a **b ~~c ".repeat(depth / 3), " c~~ b** a*".repeat(depth / 3));
        let links =
            format!("{}tief unten{}", "[*x ".repeat(depth / 2), " x*](https://torro.example)".repeat(depth / 2));
        vec![
            ("quotes", quotes),
            ("bullets", bullets),
            ("numbers", numbers),
            ("mixed", mixed),
            ("table", table),
            ("emphasis", emphasis),
            ("links", links),
        ]
    }

    #[test]
    fn deep_nesting_sets_a_pdf_on_a_small_stack() {
        // A blocking thread of tokio has 2 MiB of stack; azul-layout recurses once per level of the HTML, so a deep
        // document must not reach it as deep markup, or the whole service dies of a stack overflow.
        let worker = std::thread::Builder::new().stack_size(2 * 1024 * 1024).spawn(|| {
            for (kind, markdown) in deep_documents(1000) {
                let pdf = read(&set(&markdown, &report()));
                let text = pdf.all();
                assert!(text.contains("tief") && text.contains("unten"), "{kind}: the text stays: {text}");
            }
        });
        worker.expect("a thread").join().expect("no panic");
    }

    #[test]
    fn markup_nests_at_most_a_few_levels() {
        for (kind, markdown) in deep_documents(1000) {
            let html = body_html(&markdown).html;
            let mut depth = 0usize;
            let mut deepest = 0usize;
            for tag in html.split('<').skip(1) {
                if tag.starts_with('/') {
                    depth = depth.saturating_sub(1);
                } else if !tag.starts_with("br") && !tag.starts_with("hr") {
                    depth += 1;
                    deepest = deepest.max(depth);
                }
            }
            assert!(deepest <= 3 * MAX_NESTING + 8, "{kind}: {deepest} levels");
        }
    }

    #[test]
    fn lists_keep_their_numbers_and_nesting() {
        let body = body_html("3. drei\n4. vier\n   - innen");
        assert!(body.html.contains(">3.</div>") && body.html.contains(">4.</div>"), "{}", body.html);
        assert!(body.html.contains(">•</div>"));
        assert_eq!(body.html.matches("class=\"list\"").count(), 2);
    }
}
