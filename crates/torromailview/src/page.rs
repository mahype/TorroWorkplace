//! The page the mail window shows, and the rules the window applies to it (task M9.2).
//!
//! Everything here is a pure function, so the security rules can be tested without a web view:
//!
//! - [`mail_page`] builds the whole page: a plain head of our own HTML (subject, sender, date, `To`,
//!   `Cc`, attachments; colours, spacing and fonts of the active design) and below it the mail.
//!   Everything from the mail is escaped ([`escape`]), except its HTML, which the core cleaned
//!   through an allow list (task M9.1) and which only gets its `cid:` pictures pointed at our
//!   protocol ([`rewrite_cids`]). A mail without HTML is shown as text ([`text_html`]).
//! - Every page carries the [`CSP`] (second layer after the cleaning in the core): no scripts, no
//!   forms, no `base`, pictures and styles from the web, inline parts only through `torro-cid:`.
//! - The mail's HTML sits in a declarative shadow root inside a box with layout and paint
//!   containment: its `<style>`, its `id`s and classes cannot reach the head (a mail must not be
//!   able to hide or fake who sent it), and nothing it positions can leave its box.
//! - [`navigation`] decides where the page may go: it stays itself, links (`http`, `https`,
//!   `mailto`) go to the system's browser or mail program, everything else is refused.
//! - [`cid_part`] and [`picture_type`] decide what the `torro-cid` protocol hands out: only the
//!   inline parts of this mail that are PNG, JPEG, GIF or WebP, checked by their bytes (no SVG).

use std::fmt::Write as _;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use torro_core::design::{Design, Face, FontRole, Level, Mode, Palette, Rgba, Weight};
use torro_core::i18n::{Lang, lang, tr};
use torro_core::theme::Appearance;
use torro_mail::{AttachmentInfo, MailAddress, Message};

/// The Content-Security-Policy of every page, as a `<meta>` tag and as the header of the page's
/// response. No script of any kind, no form, no `base`, no frame; pictures from the web, `data:` and
/// the mail's own inline parts; styles inline and from the web; fonts from the web and `data:` (the
/// design's own fonts come as `data:` URLs).
pub const CSP: &str = "default-src 'none'; script-src 'none'; base-uri 'none'; form-action 'none'; \
                       img-src https: http: data: torro-cid:; style-src 'unsafe-inline' https: http:; \
                       font-src https: http: data:";

/// The protocol the page itself is served under, so it can be larger than the 2 MB a web view takes
/// as a string on Windows and gets its CSP as a header too.
pub const PAGE_SCHEME: &str = "torro-page";

/// The protocol of the mail's inline pictures (`cid:`): `torro-cid://localhost/<n>`, `<n>` the
/// index of the part in `Message::attachments`.
pub const CID_SCHEME: &str = "torro-cid";

/// Where the web view finds the page. wry maps a custom protocol to `http://<scheme>.<host>` on
/// Windows, which has none of its own.
#[cfg(not(windows))]
pub const PAGE_URL: &str = "torro-page://localhost/";
#[cfg(windows)]
pub const PAGE_URL: &str = "http://torro-page.localhost/";

/// The start of every inline picture's address, see [`PAGE_URL`] for Windows.
#[cfg(not(windows))]
const CID_BASE: &str = "torro-cid://localhost/";
#[cfg(windows)]
const CID_BASE: &str = "http://torro-cid.localhost/";

/// The MIME types of the pictures the `torro-cid` protocol hands out. No SVG: it can carry script.
pub const PICTURE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// What the window does with a navigation the web view asks about ([`navigation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    /// The page itself (also a jump to an anchor in it): let the web view load it.
    Load,
    /// A link to the web or a mail address: refuse it in the window and hand the address to the
    /// system's browser or mail program.
    Browser,
    /// Anything else (`file:`, `javascript:`, `data:`, another protocol of ours …): refuse it.
    Refuse,
}

/// Where the page may go. Only [`PAGE_URL`] itself (with an anchor) stays in the window; `http`,
/// `https` and `mailto` (any case) go to the system; everything else is refused, so the window never
/// shows anything but the mail.
pub fn navigation(url: &str) -> Navigation {
    let url = url.trim();
    if url == PAGE_URL || url.strip_prefix(PAGE_URL).is_some_and(|rest| rest.starts_with('#')) {
        return Navigation::Load;
    }
    let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
    match scheme.as_deref() {
        Some("http" | "https" | "mailto") => Navigation::Browser,
        _ => Navigation::Refuse,
    }
}

/// The part a `torro-cid` request asks for: the path of its address is `/<n>`, `<n>` the index of
/// the part in `attachments`. `None` (answer 404) for any other path, an index past the end, a
/// part without a `content_id` (a plain attachment, which a page must not make the core fetch) or
/// one whose declared type is not one of [`PICTURE_TYPES`].
pub fn cid_part<'a>(path: &str, attachments: &'a [AttachmentInfo]) -> Option<&'a AttachmentInfo> {
    let index: usize = path.strip_prefix('/')?.parse().ok()?;
    attachments
        .get(index)
        .filter(|part| part.content_id.is_some() && PICTURE_TYPES.contains(&part.content_type.as_str()))
}

/// The type a picture is served as, told by its first bytes: one of [`PICTURE_TYPES`], or `None`
/// (answer 404) for anything else – an SVG or a page that a part only claims to be a picture.
pub fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// `html` (cleaned by the core) with every picture `src="cid:<id>"` pointed at the `torro-cid`
/// protocol: `torro-cid://localhost/<n>` for the part whose `content_id` is `<id>` (percent-decoded,
/// case ignored), `torro-cid://localhost/none` (404) when the mail has no such part. Nothing else
/// changes. The address carries only the index, never the content id.
///
/// Relies on how the core writes the HTML (html5ever's serializer): tag and attribute names in lower
/// case, every value in double quotes with `"` escaped, so ` src="` inside a tag is always the
/// attribute.
pub fn rewrite_cids(html: &str, attachments: &[AttachmentInfo]) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = find_img(rest) {
        // An `<img` inside a style element is CSS text, not a tag: copy the element through its end
        // unchanged, so an unbalanced quote there cannot swallow the mail.
        if let Some(style) = find_style(&rest[..start]) {
            let end = style_end(&rest[style..]).map_or(rest.len(), |length| style + length);
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        out.push_str(&rest[..start]);
        let tag_and_after = &rest[start..];
        let end = tag_end(tag_and_after);
        out.push_str(&rewrite_tag(&tag_and_after[..end], attachments));
        rest = &tag_and_after[end..];
    }
    out.push_str(rest);
    out
}

/// Where the first `<style` element in `text` starts, ignoring case.
fn find_style(text: &str) -> Option<usize> {
    text.to_ascii_lowercase().find("<style")
}

/// The length of the style element at the start of `text`, through its `</style>`; `None` when it
/// never ends.
fn style_end(text: &str) -> Option<usize> {
    const CLOSE: &str = "</style";
    let lower = text.to_ascii_lowercase();
    let close = lower.find(CLOSE)?;
    Some(close + tag_end(&text[close..]))
}

/// Where the next `<img` tag starts (not `<imgx`).
fn find_img(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find("<img") {
        let at = from + found;
        match text[at + 4..].chars().next() {
            Some(c) if c.is_ascii_whitespace() || c == '>' || c == '/' => return Some(at),
            None => return None,
            _ => from = at + 4,
        }
    }
    None
}

/// The length of the tag at the start of `text`, through its `>`; quotes are skipped, so a `>` in a
/// value does not end it. The whole rest when it never ends.
fn tag_end(text: &str) -> usize {
    let mut quoted = false;
    for (i, c) in text.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '>' if !quoted => return i + 1,
            _ => {}
        }
    }
    text.len()
}

/// One `<img …>` tag with its `src` rewritten when it names an inline part.
fn rewrite_tag(tag: &str, attachments: &[AttachmentInfo]) -> String {
    const SRC: &str = " src=\"";
    let Some(at) = tag.find(SRC) else { return tag.to_string() };
    let value_start = at + SRC.len();
    let Some(length) = tag[value_start..].find('"') else { return tag.to_string() };
    let value = unescape(&tag[value_start..value_start + length]);
    let Some(cid) = value.get(..4).filter(|scheme| scheme.eq_ignore_ascii_case("cid:")).map(|_| &value[4..]) else {
        return tag.to_string();
    };
    let cid = percent_decode(cid);
    let target = attachments
        .iter()
        .position(|part| part.content_id.as_deref().is_some_and(|id| id.eq_ignore_ascii_case(&cid)))
        .map_or_else(|| "none".to_string(), |index| index.to_string());
    format!("{}{CID_BASE}{target}{}", &tag[..value_start], &tag[value_start + length..])
}

/// `value` of an attribute as html5ever writes it, with its character references resolved.
fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
}

/// `%XX` sequences decoded (RFC 2392 writes content ids in `cid:` that way); a broken sequence and
/// bytes that are no UTF-8 stay as they are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        match (bytes[i], bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b))) {
            (b'%', Some(high), Some(low)) => {
                // Two hex digits make one byte.
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'%'));
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// `text` safe inside HTML text and double- or single-quoted attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// A mail's plain text as HTML: escaped, with every `http://` or `https://` address a link (to the
/// end of the address; a full stop, comma or closing bracket after it belongs to the sentence).
/// Line breaks stay, through the page's `white-space: pre-wrap`.
pub fn text_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = link_start(rest) {
        let tail = &rest[start..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(tail.len());
        let link = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'']);
        let scheme = if tail.as_bytes()[4].eq_ignore_ascii_case(&b's') { "https://".len() } else { "http://".len() };
        out.push_str(&escape(&rest[..start]));
        if link.len() > scheme {
            let _ = write!(out, "<a href=\"{0}\">{0}</a>", escape(link));
            rest = &rest[start + link.len()..];
        } else {
            out.push_str(&escape(&tail[..end]));
            rest = &rest[start + end..];
        }
    }
    out.push_str(&escape(rest));
    out
}

/// Where the next web address starts (`http://` or `https://`, any case).
fn link_start(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    match (lower.find("http://"), lower.find("https://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

// ---------------------------------------------------------------------------------------------
// The look
// ---------------------------------------------------------------------------------------------

/// The style of our part of the page – head, text and problems – made once from the active design
/// and the appearance: its fonts as `data:` URLs, its tokens as CSS variables and classes.
#[derive(Debug, Clone)]
pub struct Look {
    /// The whole `<style>` content.
    css: String,
    /// The value of `<meta name="color-scheme">`, so scroll bars and the web view's own parts follow.
    color_scheme: &'static str,
}

impl Look {
    /// The look of `design` in `appearance`: light or dark as chosen, or both with the system's
    /// choice (`prefers-color-scheme`) for [`Appearance::System`].
    pub fn new(design: &Design, appearance: Appearance) -> Look {
        let mut css = font_faces(design);
        css.push_str(&scale_css(design));
        let light = palette_vars(design.colors.get(Mode::Light));
        let dark = palette_vars(design.colors.get(Mode::Dark));
        let color_scheme = match appearance {
            Appearance::Light => {
                let _ = write!(css, ":root{{{light}}}");
                "light"
            }
            Appearance::Dark => {
                let _ = write!(css, ":root{{{dark}}}");
                "dark"
            }
            Appearance::System => {
                let _ = write!(css, ":root{{{light}}}@media (prefers-color-scheme: dark){{:root{{{dark}}}}}");
                "light dark"
            }
        };
        css.push_str(BASE_CSS);
        Look { css, color_scheme }
    }
}

/// The CSS name of a font family: only letters, digits, spaces, `-` and `_`, so a pack's name can
/// never close the string or the `<style>` element.
fn family(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect()
}

/// The weights a family can have and their CSS numbers.
const WEIGHTS: [(Weight, u16); 4] =
    [(Weight::Regular, 400), (Weight::Medium, 500), (Weight::Semibold, 600), (Weight::Bold, 700)];

/// `@font-face` rules for the text and heading fonts of `design`, every weight it has, as `data:`
/// URLs. The standard's Inter comes from `torro_gui`; a pack's files are read from its folder (a
/// file that cannot be read is left out, the page then falls back to the system's font). The
/// figures font is not needed: the page asks the text font for tabular figures.
fn font_faces(design: &Design) -> String {
    let mut css = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for role in [FontRole::Text, FontRole::Heading] {
        let font = design.fonts.get(role);
        if seen.contains(&font.family.as_str()) {
            continue;
        }
        seen.push(&font.family);
        for (weight, number) in WEIGHTS {
            let bytes = match font.face(weight) {
                Some(Face::Builtin) => builtin_font(&font.family, weight).map(<[u8]>::to_vec),
                Some(Face::File(path)) => std::fs::read(path).ok(),
                None => None,
            };
            if let Some(bytes) = bytes {
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                let _ = write!(
                    css,
                    "@font-face{{font-family:\"{}\";font-weight:{number};font-style:normal;\
                     src:url(data:font/ttf;base64,{data})}}",
                    family(&font.family)
                );
            }
        }
    }
    css
}

/// The embedded file of one of the standard's faces ([`torro_gui::fonts`]).
fn builtin_font(family: &str, weight: Weight) -> Option<&'static [u8]> {
    let [regular, medium, semibold, ..] = torro_gui::fonts();
    match (family, weight) {
        ("Inter", Weight::Regular) => Some(regular),
        ("Inter", Weight::Medium) => Some(medium),
        ("Inter", Weight::Semibold) => Some(semibold),
        _ => None,
    }
}

/// The design's spacing, radii, sizes and layout gaps as CSS variables, and one class per text level
/// (`.body`, `.meta`, `.headline` …) with its font, weight, size and line height.
fn scale_css(design: &Design) -> String {
    let mut vars = String::new();
    let space = &design.space;
    for (name, value) in
        [("xs", space.xs), ("s", space.s), ("m", space.m), ("l", space.l), ("xl", space.xl), ("xxl", space.xxl)]
    {
        let _ = write!(vars, "--space-{name}:{value}px;");
    }
    let radius = &design.radius;
    for (name, value) in [("xs", radius.xs), ("sm", radius.sm), ("md", radius.md), ("lg", radius.lg)] {
        let _ = write!(vars, "--radius-{name}:{value}px;");
    }
    for (name, value) in design.size.entries() {
        let _ = write!(vars, "--{}:{value}px;", name.replace('_', "-"));
    }
    let layout = &design.layout;
    let _ = write!(
        vars,
        "--page-v:{}px;--page-h:{}px;--control-h:{}px;--section-gap:{}px;--item-gap:{}px;",
        layout.page_padding.vertical,
        layout.page_padding.horizontal,
        layout.control_padding.horizontal,
        layout.section_gap,
        layout.item_gap
    );
    let _ = write!(vars, "--family-text:\"{}\";", family(&design.fonts.text.family));
    let mut css = format!(":root{{{vars}}}");
    for (name, level) in design.typo.levels() {
        let _ = write!(css, ".{}{{{}}}", name.replace('_', "-"), level_css(design, level));
    }
    css
}

/// The font of one text level. The figures font counts as the text font (see [`font_faces`]).
fn level_css(design: &Design, level: Level) -> String {
    let role = match level.font {
        FontRole::Numbers => FontRole::Text,
        role => role,
    };
    let weight = WEIGHTS.iter().find(|(w, _)| *w == level.weight).map_or(400, |(_, number)| *number);
    format!(
        "font-family:\"{}\",system-ui,sans-serif;font-weight:{weight};font-size:{}px;line-height:{}px",
        family(&design.fonts.get(role).family),
        level.size,
        level.line
    )
}

/// The colours of one mode as CSS variables.
fn palette_vars(palette: &Palette) -> String {
    let mut vars = String::new();
    for (name, colour) in [
        ("bg", palette.bg),
        ("panel", palette.panel),
        ("raised", palette.raised),
        ("selected", palette.selected),
        ("text", palette.text),
        ("text-2", palette.text_2),
        ("text-3", palette.text_3),
        ("line", palette.line),
        ("line-strong", palette.line_strong),
        ("focus-ring", palette.focus_ring),
    ] {
        let _ = write!(vars, "--{name}:{};", rgba(colour));
    }
    vars
}

/// A colour in CSS.
fn rgba(colour: Rgba) -> String {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round();
    format!("rgba({},{},{},{:.3})", byte(colour.r), byte(colour.g), byte(colour.b), colour.a.clamp(0.0, 1.0))
}

/// The rules of our part of the page; every value is a token of the design (variables above). The
/// mail's sheet is the one exception: HTML mail is written for white paper, so it gets white paper
/// and black ink in both modes, as in other mail programs.
const BASE_CSS: &str = "\
*{box-sizing:border-box}\
html,body{margin:0;background:var(--panel);color:var(--text)}\
body{font-family:var(--family-text),system-ui,sans-serif;-webkit-text-size-adjust:100%}\
a{color:inherit;text-decoration:underline;text-decoration-color:var(--text-3)}\
a:focus-visible{outline:2px solid var(--focus-ring);outline-offset:1px;border-radius:var(--radius-xs)}\
::selection{background:var(--selected)}\
.head{display:flex;flex-direction:column;gap:var(--space-l);padding:var(--page-v) var(--page-h) var(--section-gap)}\
.subject{margin:0;color:var(--text);overflow-wrap:anywhere}\
.from{display:flex;align-items:center;gap:var(--space-m);min-width:0}\
.avatar{flex:none;display:flex;align-items:center;justify-content:center;width:var(--avatar-lg);\
height:var(--avatar-lg);border-radius:50%;background:var(--selected);color:var(--text)}\
.who{flex:1;min-width:0}\
.who>div{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}\
.address,.date{color:var(--text-3)}\
.date{flex:none;font-variant-numeric:tabular-nums}\
.people{display:grid;grid-template-columns:auto 1fr;column-gap:var(--space-m);row-gap:var(--space-xs);margin:0}\
.people dt{color:var(--text-3)}\
.people dd{margin:0;color:var(--text-2);overflow-wrap:anywhere}\
.files{display:flex;flex-wrap:wrap;gap:var(--item-gap);margin:0;padding:0;list-style:none}\
.files li{display:inline-flex;align-items:center;gap:6px;max-width:100%;height:var(--control-sm);\
padding:0 var(--control-h);border:1px solid var(--line-strong);border-radius:var(--radius-sm);\
background:var(--raised);color:var(--text)}\
.files svg{flex:none;width:14px;height:14px;color:var(--text-3)}\
.files .name{overflow:hidden;white-space:nowrap;text-overflow:ellipsis}\
.files .size{flex:none;color:var(--text-3);font-variant-numeric:tabular-nums}\
.content{padding:var(--page-v) var(--page-h);border-top:1px solid var(--line)}\
.text{max-width:var(--content-max);margin:0;white-space:pre-wrap;overflow-wrap:anywhere;color:var(--text)}\
.sheet{contain:layout paint;overflow:auto;padding:var(--space-l);border:1px solid var(--line);\
border-radius:var(--radius-md);background:#fff;color:#000;color-scheme:light}\
.problem{display:flex;flex-direction:column;gap:var(--space-s);max-width:var(--form-max);margin:0 auto;\
padding:var(--space-xxl) var(--page-h)}\
.problem h1,.problem p{margin:0}.problem p{color:var(--text-2)}\
@media (max-width:599px){.head,.content{padding-left:var(--space-l);padding-right:var(--space-l)}\
.sheet{padding:var(--space-s)}}";

/// What the mail's own HTML gets inside its shadow root before its own styles: a block, the
/// design's text font at reading size and the browser's normal line height (a mail's tables and
/// pictures are laid out for it). Its own `<style>` may change all of that, but only inside.
const MAIL_HOST_CSS: &str = ":host{display:block;font-size:14px;line-height:normal;overflow-wrap:break-word}";

/// The paperclip of the attachments (Lucide, as `torro_gui::icon::Icon::Paperclip`), drawn in the
/// colour of the text around it.
const PAPERCLIP: &str = include_str!("../../torro-gui/assets/icons/paperclip.svg");

// ---------------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------------

/// The page of `mail`: head, then its HTML (rewritten by [`rewrite_cids`], in its own shadow root)
/// or, without HTML, its text ([`text_html`]). Everything from the mail but the cleaned HTML is
/// escaped.
pub fn mail_page(mail: &Message, look: &Look) -> String {
    let subject = subject(mail);
    let mut body = String::new();
    body.push_str(&head(mail, &subject));
    body.push_str("<main class=\"content\">");
    match &mail.html {
        Some(html) => {
            let html = rewrite_cids(html, &mail.attachments);
            let _ = write!(
                body,
                "<div class=\"sheet\"><div class=\"mail\"><template shadowrootmode=\"open\">\
                 <style>{MAIL_HOST_CSS}</style>{html}</template></div></div>"
            );
        }
        None => {
            let text = mail.text.as_deref().unwrap_or_default();
            let _ = write!(body, "<div class=\"text read\">{}</div>", text_html(text));
        }
    }
    body.push_str("</main>");
    document(&subject, &body, look)
}

/// A page that says something went wrong: `title` and a sentence on what to do.
pub fn problem_page(title: &str, text: &str, look: &Look) -> String {
    let body = format!(
        "<main class=\"problem\"><h1 class=\"title\">{}</h1><p class=\"body\">{}</p></main>",
        escape(title),
        escape(text)
    );
    document(title, &body, look)
}

/// The whole document around `body`, with the [`CSP`] first in `<head>`, before anything that could
/// load.
fn document(title: &str, body: &str, look: &Look) -> String {
    let lang = match lang() {
        Lang::De => "de",
        Lang::En => "en",
    };
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\
         <meta name=\"referrer\" content=\"no-referrer\">\
         <meta name=\"color-scheme\" content=\"{}\">\
         <meta name=\"viewport\" content=\"width=device-width\">\
         <title>{}</title><style>{}</style></head><body>{body}</body></html>",
        look.color_scheme,
        escape(title),
        look.css
    )
}

/// The subject on one line, or "(no subject)".
pub fn subject(mail: &Message) -> String {
    let subject = mail.summary.subject.split_whitespace().collect::<Vec<_>>().join(" ");
    if subject.is_empty() { tr("(no subject)").to_string() } else { subject }
}

/// Our head: subject, sender with avatar and date, `To` and `Cc`, the attachments.
fn head(mail: &Message, subject: &str) -> String {
    let mut head = format!("<header class=\"head\"><h1 class=\"subject headline\">{}</h1>", escape(subject));
    let (name, address) = match &mail.summary.from {
        Some(from) => match from.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => (name.to_string(), Some(from.address.clone())),
            None => (from.address.clone(), None),
        },
        None => (tr("Unknown sender").to_string(), None),
    };
    let initial = name.chars().find(|c| c.is_alphanumeric()).map_or_else(String::new, |c| c.to_uppercase().collect());
    let _ = write!(
        head,
        "<div class=\"from\"><span class=\"avatar body-strong\" aria-hidden=\"true\">{}</span>\
         <div class=\"who\"><div class=\"name body-strong\">{}</div>",
        escape(&initial),
        escape(&name)
    );
    if let Some(address) = address {
        let _ = write!(head, "<div class=\"address meta\">{}</div>", escape(&address));
    }
    let _ = write!(head, "</div><time class=\"date meta\">{}</time></div>", escape(&when(mail.summary.date)));
    let mut people = String::new();
    for (label, list) in [(tr("To"), &mail.summary.to), (tr("Cc"), &mail.cc)] {
        if !list.is_empty() {
            let _ = write!(people, "<dt>{}</dt><dd>{}</dd>", escape(label), escape(&addresses(list)));
        }
    }
    if !people.is_empty() {
        let _ = write!(head, "<dl class=\"people meta\">{people}</dl>");
    }
    let files: Vec<&AttachmentInfo> = mail.attachments.iter().filter(|part| !part.inline).collect();
    if !files.is_empty() {
        let _ = write!(head, "<ul class=\"files caption\" aria-label=\"{}\">", escape(tr("Attachments")));
        for file in files {
            let name = file.filename.as_deref().unwrap_or_else(|| tr("Attachment"));
            let _ = write!(
                head,
                "<li title=\"{name}\">{PAPERCLIP}<span class=\"name\">{name}</span><span class=\"size\">{}</span></li>",
                escape(&size_text(file.size)),
                name = escape(name)
            );
        }
        head.push_str("</ul>");
    }
    head.push_str("</header>");
    head
}

/// `Name <address>` for each, comma separated; the address alone without a name.
fn addresses(list: &[MailAddress]) -> String {
    list.iter()
        .map(|who| match who.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => format!("{name} <{}>", who.address),
            None => who.address.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// "Sunday, 11 October 2026 14:30" in the user's time and language.
fn when(at: DateTime<Utc>) -> String {
    let local = torro_gui::datetime::to_local(at);
    format!(
        "{} {}",
        torro_gui::datetime::format_long_date(local.date()),
        torro_gui::datetime::format_time(local.time())
    )
}

/// A size in bytes as people read it: "860 B", "3 KB", "12 MB" (as the inbox shows it).
fn size_text(bytes: u64) -> String {
    use torro_core::i18n::trf;
    const KB: u64 = 1024;
    if bytes < KB {
        trf("{n} B", &[("n", &bytes)])
    } else if bytes < KB * KB {
        trf("{n} KB", &[("n", &bytes.div_ceil(KB))])
    } else {
        trf("{n} MB", &[("n", &bytes.div_ceil(KB * KB))])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_mail::MessageSummary;

    fn part(content_type: &str, content_id: Option<&str>, inline: bool) -> AttachmentInfo {
        AttachmentInfo {
            part: "2".into(),
            filename: Some("logo.png".into()),
            content_type: content_type.into(),
            size: 2_048,
            inline,
            content_id: content_id.map(str::to_string),
        }
    }

    fn mail(html: Option<&str>) -> Message {
        Message {
            summary: MessageSummary {
                id: "m:1".into(),
                account: "mail:me@example.org".into(),
                folder: "f:1".into(),
                thread_id: "t:1".into(),
                from: Some(MailAddress { name: Some("Anna Berg".into()), address: "anna@example.org".into() }),
                to: vec![MailAddress { name: None, address: "me@example.org".into() }],
                subject: "Weekly news".into(),
                date: DateTime::parse_from_rfc3339("2026-10-14T08:30:00Z").expect("date").with_timezone(&Utc),
                snippet: String::new(),
                flags: Vec::new(),
                has_attachments: false,
                invite: None,
                untrusted: true,
            },
            cc: Vec::new(),
            reply_to: Vec::new(),
            text: Some("Hello".into()),
            from_html: false,
            html: html.map(str::to_string),
            attachments: Vec::new(),
            traits: Default::default(),
            invitation: false,
        }
    }

    fn look() -> Look {
        Look::new(&Design::standard(), Appearance::System)
    }

    /// Acceptance: sender and subject with `<script>` are escaped in the head.
    #[test]
    fn the_head_escapes_sender_and_subject() {
        let mut message = mail(None);
        message.summary.subject = "<script>alert(1)</script> news".into();
        message.summary.from =
            Some(MailAddress { name: Some("<script>x</script>".into()), address: "a\"><img src=x>@e.org".into() });
        message.cc = vec![MailAddress { name: Some("<b>Cc</b>".into()), address: "c@e.org".into() }];
        message.attachments = vec![AttachmentInfo {
            filename: Some("<svg onload=x>.pdf".into()),
            ..part("application/pdf", None, false)
        }];
        message.text = Some("<script>body</script>".into());
        let page = mail_page(&message, &look());
        assert!(!page.contains("<script"), "no script tag anywhere");
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt; news"));
        assert!(page.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(page.contains("a&quot;&gt;&lt;img src=x&gt;@e.org"));
        assert!(page.contains("&lt;b&gt;Cc&lt;/b&gt;"));
        assert!(page.contains("&lt;svg onload=x&gt;.pdf"));
        assert!(page.contains("&lt;script&gt;body&lt;/script&gt;"));
    }

    /// Acceptance: the CSP stands in every page, first thing in the head, with what the task names.
    #[test]
    fn every_page_carries_the_policy() {
        let pages = [
            mail_page(&mail(Some("<p>Hi</p>")), &look()),
            mail_page(&mail(None), &look()),
            problem_page(tr("Couldn't open the mail."), "x", &look()),
            mail_page(&mail(None), &Look::new(&Design::standard(), Appearance::Dark)),
        ];
        for page in pages {
            let meta = format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">");
            let at = page.find(&meta).expect("policy in the page");
            assert!(at < page.find("<style>").expect("style"), "before anything that could load");
            assert_eq!(page.matches("<body>").count(), 1);
        }
        for rule in [
            "default-src 'none'",
            "script-src 'none'",
            "base-uri 'none'",
            "form-action 'none'",
            "img-src https: http: data: torro-cid:",
            "style-src 'unsafe-inline' https: http:",
            "font-src https: http: data:",
        ] {
            assert!(CSP.contains(rule), "{rule}");
        }
    }

    /// The mail's HTML stands in its own shadow root inside the contained sheet; without HTML the
    /// text with its links.
    #[test]
    fn html_goes_into_its_own_root_and_text_into_a_block() {
        let page = mail_page(&mail(Some("<style>p{color:red}</style><p id=\"x\">Hi</p>")), &look());
        assert!(
            page.contains("<div class=\"sheet\"><div class=\"mail\"><template shadowrootmode=\"open\"><style>:host{")
        );
        assert!(page.contains("<style>p{color:red}</style><p id=\"x\">Hi</p></template></div></div>"));

        let mut text = mail(None);
        text.text = Some("See https://example.org/a?b=1&c=2.\nBye".into());
        let page = mail_page(&text, &look());
        assert!(page.contains(
            "<div class=\"text read\">See <a href=\"https://example.org/a?b=1&amp;c=2\">\
             https://example.org/a?b=1&amp;c=2</a>.\nBye</div>"
        ));
        assert!(!page.contains("<template"));
    }

    /// Acceptance: `cid:` is pointed at the protocol by the index of the part, case and percent
    /// signs as RFC 2392 writes them; an unknown id goes nowhere; nothing else changes.
    #[test]
    fn cid_pictures_are_pointed_at_the_protocol() {
        let parts = vec![part("application/pdf", None, false), part("image/png", Some("Logo@Example.org"), true)];
        let html = "<p>src=\"cid:logo@example.org\"</p><img src=\"cid:logo%40example.org\" alt=\"a > b\">\
                    <img alt=\"x\" src=\"CID:logo@example.org\"><img src=\"cid:other\">\
                    <img src=\"https://e.org/x.png\">\
                    <imgx src=\"cid:logo@example.org\">";
        let out = rewrite_cids(html, &parts);
        assert_eq!(
            out,
            format!(
                "<p>src=\"cid:logo@example.org\"</p><img src=\"{CID_BASE}1\" alt=\"a > b\">\
                 <img alt=\"x\" src=\"{CID_BASE}1\"><img src=\"{CID_BASE}none\"><img src=\"https://e.org/x.png\">\
                 <imgx src=\"cid:logo@example.org\">"
            )
        );
        assert!(!out.contains("img src=\"cid:"), "no cid address left in a picture");
        let page =
            mail_page(&Message { attachments: parts, ..mail(Some("<img src=\"cid:logo@example.org\">")) }, &look());
        assert!(page.contains(&format!("<img src=\"{CID_BASE}1\">")));
    }

    /// Acceptance: the protocol hands out only inline pictures; a part that is no picture, an SVG,
    /// an index past the end and a path of another form are 404.
    #[test]
    fn only_pictures_come_through_the_protocol() {
        let parts = vec![
            part("application/pdf", Some("doc"), false),
            part("image/png", Some("logo"), true),
            part("image/svg+xml", Some("drawing"), true),
            part("text/html", Some("page"), true),
        ];
        assert_eq!(cid_part("/1", &parts).map(|p| p.content_type.as_str()), Some("image/png"));
        for path in ["/0", "/2", "/3", "/4", "/none", "/-1", "1", "/1/x", ""] {
            assert!(cid_part(path, &parts).is_none(), "{path}");
        }
        for kind in PICTURE_TYPES {
            assert!(cid_part("/0", &[part(kind, Some("x"), true)]).is_some(), "{kind}");
        }
    }

    /// A picture attached without a `content_id` is a plain attachment, never fetched for the page.
    #[test]
    fn attachments_without_a_content_id_stay_out() {
        assert!(cid_part("/0", &[part("image/png", None, false)]).is_none());
        assert!(cid_part("/0", &[part("image/png", Some("x"), false)]).is_some(), "a cid part, even if attached");
    }

    /// An `<img` written inside a style element is CSS text: it is left as it is, and an open quote
    /// there does not swallow the mail after it.
    #[test]
    fn style_elements_are_not_rewritten() {
        let parts = vec![part("image/png", Some("logo"), true)];
        let html = "<STYLE>p::after{content:'<img src=\"cid:logo'}</style><p>Kept</p><img src=\"cid:logo\">";
        assert_eq!(
            rewrite_cids(html, &parts),
            format!("<STYLE>p::after{{content:'<img src=\"cid:logo'}}</style><p>Kept</p><img src=\"{CID_BASE}0\">")
        );
        let open = "<style>a{}<img src=\"cid:logo\">";
        assert_eq!(rewrite_cids(open, &parts), open, "an unclosed style element stays as it is");
    }

    /// Acceptance: an SVG (or a page) that a part only claims to be a picture is not served.
    #[test]
    fn pictures_are_told_by_their_bytes() {
        assert_eq!(picture_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(picture_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(picture_type(b"GIF89a...."), Some("image/gif"));
        assert_eq!(picture_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        for not in
            [&b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"x\"/>"[..], b"<html>", b"", b"RIFF\0\0\0\0WAVE"]
        {
            assert_eq!(picture_type(not), None);
        }
    }

    /// Acceptance: the page stays itself; links go to the browser; everything else is refused.
    #[test]
    fn navigation_keeps_the_page_and_hands_links_to_the_browser() {
        assert_eq!(navigation(PAGE_URL), Navigation::Load);
        assert_eq!(navigation(&format!("{PAGE_URL}#section")), Navigation::Load);
        for link in ["https://example.org/", "http://example.org/x", "HTTPS://EXAMPLE.ORG", "mailto:anna@example.org"] {
            assert_eq!(navigation(link), Navigation::Browser, "{link}");
        }
        for other in [
            &format!("{PAGE_URL}other"),
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>x</script>",
            "torro-cid://localhost/1",
            "about:blank",
            "ftp://example.org",
            "",
        ] {
            assert_eq!(navigation(other), Navigation::Refuse, "{other}");
        }
    }

    /// The look follows the appearance and draws with the design's fonts.
    #[test]
    fn the_look_follows_the_appearance() {
        let design = Design::standard();
        let light = Look::new(&design, Appearance::Light);
        let dark = Look::new(&design, Appearance::Dark);
        let system = Look::new(&design, Appearance::System);
        assert_eq!((light.color_scheme, dark.color_scheme, system.color_scheme), ("light", "dark", "light dark"));
        assert!(light.css.contains(&format!("--panel:{}", rgba(design.colors.light.panel))));
        assert!(dark.css.contains(&format!("--panel:{}", rgba(design.colors.dark.panel))));
        assert!(!light.css.contains("prefers-color-scheme") && system.css.contains("prefers-color-scheme: dark"));
        assert_eq!(light.css.matches("@font-face{font-family:\"Inter\"").count(), 3, "regular, medium, semibold");
        assert!(light.css.contains(".body-strong{font-family:\"Inter\""));
    }

    #[test]
    fn family_names_cannot_leave_the_style() {
        assert_eq!(family("Inter</style><script>"), "Interstylescript");
        assert_eq!(family("IBM Plex Sans"), "IBM Plex Sans");
    }

    #[test]
    fn subjects_and_sizes_read_well() {
        let mut message = mail(None);
        message.summary.subject = "  ".into();
        assert_eq!(subject(&message), tr("(no subject)"));
        message.summary.subject = "Weekly\n  news".into();
        assert_eq!(subject(&message), "Weekly news");
        assert_eq!(size_text(860), torro_core::i18n::trf("{n} B", &[("n", &860)]));
        assert_eq!(size_text(2_340), torro_core::i18n::trf("{n} KB", &[("n", &3)]));
    }

    #[test]
    fn links_in_text_end_where_the_address_ends() {
        assert_eq!(text_html("(see http://a.org/x)."), "(see <a href=\"http://a.org/x\">http://a.org/x</a>).");
        assert_eq!(text_html("only https:// here"), "only https:// here");
        assert_eq!(text_html("a < b & c"), "a &lt; b &amp; c");
    }
}
