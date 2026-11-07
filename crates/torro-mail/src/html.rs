//! The HTML of mail the suite writes (issue #125): the small, safe subset of HTML that the compose
//! window makes ([`crate::markup`]) and that a client may send with `mail.save_draft`
//! (`Draft::html`), its plain-text form, and the account's signature put into it.
//!
//! - [`sanitize`] keeps only that subset: paragraphs, line breaks, headings, bold, italic,
//!   underline, strike-through, code, lists, quotes, simple tables, rules and links to `http`,
//!   `https` and `mailto`, and CID-only images with escaped alt text and bounded dimensions.
//!   Scripts, styles, forms, frames, external/data images and event handlers go; nothing is fetched.
//!   The core cleans every HTML a draft brings before it stores or sends it, so what a person
//!   approves is what goes out.
//! - [`to_text`] is the plain text sent beside the HTML (`multipart/alternative`) and shown where
//!   the text of a draft is shown: paragraphs, `> ` before quoted lines, `- ` and `1. ` before
//!   list items, a link's address behind its text.
//! - [`to_markup`] turns such HTML back into the compose window's markup ([`crate::markup`]), so
//!   a saved draft can be written on again (issue #502).
//! - [`sign`] puts the signature (`-- ` and the account's text) in front of the quoted history of
//!   an answer (the element with the class [`QUOTE_CLASS`], which [`crate::markup::to_html`] wraps
//!   it in), else at the end.
//!
//! The parser is tolerant, not a full HTML5 parser. It never needs to be exact: what it hands out
//! is rebuilt from scratch from the allowed elements and escaped text, so a tag it misreads loses
//! formatting but can never let markup through. Pure functions, no I/O.

/// The class of the element that holds the quoted history of an answer (or the forwarded
/// message): the signature goes in front of it ([`sign`]).
pub const QUOTE_CLASS: &str = "torro-quote";

/// The class of the element that holds the signature.
pub const SIGNATURE_CLASS: &str = "torro-signature";

/// The elements kept by [`sanitize`]; every other one is dropped, its text kept (unless it is one
/// of [`DROPPED`]).
const ALLOWED: &[&str] = &[
    "p",
    "div",
    "br",
    "hr",
    "blockquote",
    "ul",
    "ol",
    "li",
    "pre",
    "code",
    "h1",
    "h2",
    "h3",
    "h4",
    "b",
    "strong",
    "i",
    "em",
    "u",
    "s",
    "a",
    "table",
    "thead",
    "tbody",
    "tr",
    "td",
    "th",
    "img",
];

/// Elements without content or closing tag.
const VOID: &[&str] = &["br", "hr", "img"];

/// Elements dropped together with everything inside them: code, styles, metadata, embedded
/// documents and form controls are never text of the message.
const DROPPED: &[&str] = &[
    "script", "style", "head", "title", "iframe", "object", "noscript", "template", "svg", "math", "textarea",
    "select", "button", "frameset", "noframes", "noembed", "xmp", "applet", "audio", "video", "canvas", "picture",
    "map",
];

/// Of [`DROPPED`], the ones whose content is raw text up to their closing tag (no tags inside).
const RAW_TEXT: &[&str] = &["script", "style", "title", "textarea", "xmp", "iframe", "noscript", "noembed", "noframes"];

/// The longest link address kept.
const HREF_MAX: usize = 2048;

/// Cleans `html` to the subset this module describes and answers it as well-formed HTML (every
/// element closed, text and attribute values escaped). Running it again changes nothing. Kept
/// attributes: `href` of `a` (only `http:`, `https:` and `mailto:`, else the link becomes its
/// text), `type="cite"` of `blockquote`, and the classes [`QUOTE_CLASS`] and [`SIGNATURE_CLASS`]
/// of `div`. An empty or blank input answers an empty string.
pub fn sanitize(html: &str) -> String {
    serialize(&parse(html))
}

/// A Content-ID safe in both MIME headers and a `cid:` URL. No whitespace, URL delimiters,
/// controls or quoting; ids are opaque, not file paths or network addresses.
pub fn valid_cid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 250 && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
}

/// Every kept image reference, in document order.
pub fn image_refs(html: &str) -> Vec<String> {
    to_blocks(html)
        .into_iter()
        .filter_map(|block| match block {
            HtmlBlock::Image { cid, .. } => Some(cid),
            _ => None,
        })
        .collect()
}

/// Lossless image boundaries for the block compose editor; text pieces are sanitized HTML.
/// The old `to_markup` interface remains unchanged for text-only callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlBlock {
    Text(String),
    Image { cid: String, alt: String, width: Option<u32>, height: Option<u32>, quoted: bool },
}

pub fn to_blocks(html: &str) -> Vec<HtmlBlock> {
    fn blocks(nodes: &[Node], quoted: bool) -> Vec<HtmlBlock> {
        let mut out = Vec::new();
        for node in nodes {
            let pieces = match node {
                Node::Element(e) if e.name == "img" => vec![HtmlBlock::Image {
                    cid: e.attr("src").unwrap_or_default().strip_prefix("cid:").unwrap_or_default().into(),
                    alt: e.attr("alt").unwrap_or_default().into(),
                    width: e.attr("width").and_then(|v| v.parse().ok()),
                    height: e.attr("height").and_then(|v| v.parse().ok()),
                    quoted,
                }],
                Node::Element(e) if !VOID.contains(&e.name) => blocks(&e.children, quoted || e.has_class(QUOTE_CLASS))
                    .into_iter()
                    .map(|part| match part {
                        HtmlBlock::Text(text) => {
                            let attrs =
                                e.attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", escape(v))).collect::<String>();
                            HtmlBlock::Text(format!("<{}{attrs}>{text}</{}>", e.name, e.name))
                        }
                        image => image,
                    })
                    .collect(),
                _ => vec![HtmlBlock::Text(serialize(std::slice::from_ref(node)))],
            };
            for piece in pieces {
                match (out.last_mut(), piece) {
                    (Some(HtmlBlock::Text(last)), HtmlBlock::Text(text)) => last.push_str(&text),
                    (_, piece) => out.push(piece),
                }
            }
        }
        out
    }
    blocks(&parse(html), false)
}

/// The plain text of `html` (any HTML: it is cleaned first), for the `text/plain` part of a
/// message: blocks as lines, paragraphs, headings and lists separated by a blank line, quotes with
/// `> ` (a bare `>` on an empty line), list items with `- ` or `1. `, a link as `text <address>`
/// (only the address when it is its own text), table cells separated by ` | `, a rule as `---`. A
/// line that is only `--` becomes the signature separator `-- ` (RFC 3676). Line ends `\n`, no
/// white space at the ends of lines, at most one blank line in a row, none at the start or end.
pub fn to_text(html: &str) -> String {
    let lines = lines_of(&parse(html), false);
    lines.into_iter().map(|line| if line == "--" { "-- ".to_owned() } else { line }).collect::<Vec<_>>().join("\n")
}

/// The compose window's text ([`crate::markup`]) of `html` (any HTML: it is cleaned first) and the
/// line where its quoted history starts, so that `markup::to_html(text, history)` gives the HTML
/// back: for what `markup::to_html` made, exactly (issue #502, writing on a saved draft).
///
/// Own text: paragraphs separated by a blank line, `br` a line break, `strong`/`b` as `**…**`,
/// `em`/`i` as `*…*`, `u` as `__…__`, a link as `[text](address)` (an address that is its own
/// text as it is, as it became a link by itself), `ul`/`ol` items as `- `/`1. ` lines, a
/// `blockquote` with `> ` before its lines. Characters the markup would read as marks are written
/// with `\` in front. The element with the class [`QUOTE_CLASS`] is the history: its lines as
/// [`to_text`] writes them, without marks, and its first line is the second value (`None` without
/// one). Everything else (headings, tables, code, rules) is text as [`to_text`] writes it. Bold,
/// italic and underline right at a letter keep only their text, as the markup has no mark there; a
/// line that starts like a numbered item (`3. `) gets a `\\` before its dot.
pub fn to_markup(html: &str) -> (String, Option<String>) {
    let nodes = parse(html);
    let start = nodes.iter().position(|node| matches!(node, Node::Element(element) if element.has_class(QUOTE_CLASS)));
    let (own, theirs) = nodes.split_at(start.unwrap_or(nodes.len()));
    let mut lines = marked_lines(own);
    let mut history = None;
    if !theirs.is_empty() {
        let quoted: Vec<String> = theirs
            .iter()
            .flat_map(|node| match node {
                Node::Element(element) if element.has_class(QUOTE_CLASS) => lines_of(&element.children, false),
                other => lines_of(std::slice::from_ref(other), false),
            })
            .collect();
        history = quoted.iter().find(|line| !line.trim().is_empty()).cloned();
        lines.push(String::new());
        lines.extend(quoted);
    }
    (tidy(lines).join("\n"), history)
}

/// Plain `text` as the compose window's markup ([`crate::markup`]), so that the marks it reads
/// stay text (issue #502, writing on a draft saved without HTML in an account that writes
/// formatted mail): characters that would be marks get a `\` in front, and a line that would start
/// a list item too. Quoted lines (`>`) stay quotes, their text escaped the same way. White space in
/// a line is collapsed, as HTML shows it.
pub fn plain_to_markup(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let mut out = Vec::new();
    for line in text.split('\n') {
        let rest = line.trim_start();
        let marks = rest.find(|c: char| c != '>' && c != ' ').filter(|_| rest.starts_with('>')).unwrap_or(0);
        let (quote, words) = rest.split_at(if rest.starts_with('>') && marks == 0 { rest.len() } else { marks });
        let mut gathered = marked_text(words);
        let mut lines = Vec::new();
        end_lines(&mut gathered, &mut lines);
        let words = lines.concat();
        let quote = quote.trim_end();
        out.push(if quote.is_empty() { words } else { format!("{quote} {words}").trim_end().to_owned() });
    }
    out.join("\n")
}

/// A clipboard conversion, independent of accounts, disk, network and the OS clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paste {
    pub markup: String,
    pub plain: String,
    pub plain_markup: String,
    pub formatted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteError {
    TooLarge,
    TooDeep,
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "paste inputs and outputs may hold at most 1 MiB each",
            Self::TooDeep => "paste HTML may nest at most 64 elements",
        })
    }
}

impl std::error::Error for PasteError {}

/// Escapes literal clipboard text, including quote/list prefixes. Unlike draft reopening,
/// pasted `>` is not a history boundary. Keep line breaks, but discard directional controls.
pub fn paste_plain_markup(text: &str) -> String {
    clean_text(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| {
            // A paste is a substring: its edge whitespace may separate existing words.
            let start = line.len() - line.trim_start().len();
            let words = &line[start..];
            let digits = words.find(|c: char| !c.is_ascii_digit()).unwrap_or(words.len());
            let numbered = (1..=6).contains(&digits) && words[digits..].starts_with(". ");
            let mut out = String::new();
            for (at, c) in line.char_indices() {
                if matches!(c, '\\' | '*' | '_' | '[' | ']')
                    || at == start && matches!(c, '>' | '-' | '#' | '`')
                    || numbered && at == start + digits
                {
                    out.push('\\');
                }
                out.push(c);
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalizes foreign HTML separately from saved drafts. The existing `to_markup` remains
/// authoritative for representable word boundaries and safe links. Images and suite classes
/// never enter this path; no clipboard content can create a signature/history boundary.
pub fn prepare_paste(html: Option<&str>, text: Option<&str>) -> Result<Paste, PasteError> {
    const MAX: usize = 1024 * 1024;
    if [html, text].into_iter().flatten().any(|s| s.len() > MAX) {
        return Err(PasteError::TooLarge);
    }
    let normalized = paste_html(html.unwrap_or_default())?;
    let plain = text.map(clean_text).unwrap_or_else(|| to_text(&normalized));
    let plain_markup = paste_plain_markup(&plain);
    let uses_html = html.is_some_and(|s| !s.trim().is_empty());
    let mut markup = if uses_html { to_markup(&normalized).0 } else { plain_markup.clone() };
    if uses_html && !markup.is_empty() {
        // A clipboard insertion is a substring: preserve whitespace separating existing words.
        let edges = text.map(str::to_owned).unwrap_or_else(|| text_of(&parse(&normalized)));
        if !edges.trim().is_empty() {
            let is_edge = |c: char| if text.is_some() { c.is_whitespace() } else { matches!(c, ' ' | '\t' | '\u{a0}') };
            let leading = &edges[..edges.len() - edges.trim_start_matches(is_edge).len()];
            let trailing = &edges[edges.trim_end_matches(is_edge).len()..];
            markup = format!("{}{}{}", paste_plain_markup(leading), markup, paste_plain_markup(trailing));
        }
    }
    if [&markup, &plain, &plain_markup].iter().any(|s| s.len() > MAX) {
        return Err(PasteError::TooLarge);
    }
    let rendered = crate::markup::to_html(&markup, None);
    let formatted =
        ["<strong>", "<em>", "<u>", "<ul>", "<ol>", "<blockquote", "<a "].iter().any(|tag| rendered.contains(tag))
            && markup != plain_markup;
    Ok(Paste { markup, plain, plain_markup, formatted })
}

/// Count even unknown/discarded elements before any recursive tree traversal or discard.
/// Raw-text contents are not HTML; all other element depth is bounded, including SVG.
fn paste_depth(mut rest: &str) -> Result<(), PasteError> {
    let mut stack = Vec::new();
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        rest = match token(rest) {
            Token::Open { name, after, .. } => {
                if ![
                    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
                    "track", "wbr",
                ]
                .contains(&name.as_str())
                {
                    stack.push(name.clone());
                    if stack.len() > 64 {
                        return Err(PasteError::TooDeep);
                    }
                    if RAW_TEXT.contains(&name.as_str()) {
                        stack.pop();
                        rest = skip_element(&name, after);
                        continue;
                    }
                }
                after
            }
            Token::Close { name, after } => {
                if let Some(at) = stack.iter().rposition(|s| *s == name) {
                    stack.truncate(at);
                }
                after
            }
            Token::Comment(after) | Token::Declaration(after) | Token::Text(after) => after,
        };
    }
    Ok(())
}

fn paste_html(html: &str) -> Result<String, PasteError> {
    paste_depth(html)?;
    let mut rest = html;
    let mut out = String::new();
    // Original names paired with generated closing tags: spans may add semantic wrappers,
    // but neither their styles nor any other foreign attributes reach the resulting HTML.
    let mut stack: Vec<(String, String)> = Vec::new();
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        rest = match token(rest) {
            Token::Open { name, attrs, after } => {
                let style =
                    attrs.iter().find(|(k, _)| k == "style").map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
                let properties: Vec<(&str, &str)> =
                    style.split(';').filter_map(|p| p.split_once(':')).map(|(k, v)| (k.trim(), v.trim())).collect();
                if DROPPED.contains(&name.as_str()) || properties.contains(&("mso-list", "ignore")) {
                    skip_element(&name, after)
                } else if name == "img" {
                    after
                } else {
                    let mut close = String::new();
                    let list = name == "p" && properties.iter().any(|(k, v)| *k == "mso-list" && *v != "none");
                    if list {
                        let kind = office_list_kind(after);
                        out.push_str(&format!("<{kind}><li>"));
                        close.push_str(&format!("</li></{kind}>"));
                    } else if ALLOWED.contains(&name.as_str()) {
                        out.push('<');
                        out.push_str(&name);
                        if name == "a" {
                            for (k, v) in &attrs {
                                if k == "href" {
                                    out.push_str(&format!(" href=\"{}\"", escape(v)));
                                    break;
                                }
                            }
                        }
                        out.push('>');
                        if !VOID.contains(&name.as_str()) {
                            close.push_str(&format!("</{name}>"));
                        }
                    }
                    for (tag, on) in [
                        (
                            "b",
                            properties.iter().any(|(k, v)| {
                                *k == "font-weight" && (*v == "bold" || v.parse::<u16>().is_ok_and(|n| n >= 600))
                            }),
                        ),
                        ("i", properties.contains(&("font-style", "italic"))),
                        (
                            "u",
                            properties.iter().any(|(k, v)| {
                                matches!(*k, "text-decoration" | "text-decoration-line")
                                    && v.split_whitespace().any(|s| s == "underline")
                            }),
                        ),
                    ] {
                        if on
                            && ![
                                "area", "base", "br", "col", "embed", "hr", "input", "link", "meta", "param", "source",
                                "track", "wbr",
                            ]
                            .contains(&name.as_str())
                        {
                            out.push_str(&format!("<{tag}>"));
                            close.insert_str(0, &format!("</{tag}>"));
                        }
                    }
                    if ![
                        "area", "base", "br", "col", "embed", "hr", "input", "link", "meta", "param", "source",
                        "track", "wbr",
                    ]
                    .contains(&name.as_str())
                    {
                        stack.push((name, close));
                    }
                    after
                }
            }
            Token::Close { name, after } => {
                if let Some(at) = stack.iter().rposition(|(n, _)| *n == name) {
                    while stack.len() > at {
                        if let Some((_, close)) = stack.pop() {
                            out.push_str(&close);
                        }
                    }
                }
                after
            }
            Token::Comment(after) | Token::Declaration(after) => after,
            Token::Text(after) => {
                out.push_str("&lt;");
                after
            }
        };
    }
    for (_, close) in stack.into_iter().rev() {
        out.push_str(&close);
    }
    // Semantic wrappers can add levels even when source nesting was shallow. Validate again
    // before constructing/traversing a tree; void elements never leave wrappers open.
    paste_depth(&out)?;
    let mut nodes = parse(&out);
    join_paste_lists(&mut nodes);
    Ok(serialize(&nodes))
}

fn office_list_kind(rest: &str) -> &'static str {
    // The draft parser ignores unknown names, unlike source-depth validation. Never recurse
    // through raw HTML here: mismatched unknown tags can otherwise hide arbitrary depth.
    let mut rest = rest;
    let mut text = String::new();
    while !rest.is_empty() && text.chars().count() < 32 {
        let end = rest.find('<').unwrap_or(rest.len());
        text.extend(decode(&rest[..end]).chars().take(32 - text.chars().count()));
        rest = &rest[end..];
        if rest.is_empty() {
            break;
        }
        rest = match token(rest) {
            Token::Close { name, .. } if name == "p" => break,
            Token::Open { name, after, .. } if DROPPED.contains(&name.as_str()) => skip_element(&name, after),
            Token::Open { after, .. }
            | Token::Close { after, .. }
            | Token::Comment(after)
            | Token::Declaration(after)
            | Token::Text(after) => after,
        };
    }
    let marker = text.trim_start();
    let digits = marker.find(|c: char| !c.is_ascii_digit()).unwrap_or(marker.len());
    if digits > 0 && marker[digits..].starts_with(['.', ')']) { "ol" } else { "ul" }
}

fn join_paste_lists(nodes: &mut Vec<Node>) {
    let mut merged: Vec<Node> = Vec::new();
    for mut node in std::mem::take(nodes) {
        if let Node::Element(element) = &mut node {
            join_paste_lists(&mut element.children);
            if matches!(element.name, "ul" | "ol") {
                let last = merged.iter().rposition(|node| !matches!(node, Node::Text(s) if s.trim().is_empty()));
                if let Some(at) = last
                    && let Node::Element(previous) = &mut merged[at]
                    && previous.name == element.name
                {
                    previous.children.append(&mut element.children);
                    merged.truncate(at + 1);
                    continue;
                }
            }
        }
        merged.push(node);
    }
    *nodes = merged;
}

/// `html`, cleaned ([`sanitize`]), with `signature` as `<div class="torro-signature">-- <br>…</div>`
/// (each line of it escaped, line breaks as `<br>`) in front of the first top-level element with
/// the class [`QUOTE_CLASS`], else at the end. A missing or blank signature adds nothing.
pub fn sign(html: &str, signature: Option<&str>) -> String {
    sign_inner(html, signature, false)
}

/// As [`sign`], with the signature written in the compose editor's safe markup. Rendering runs
/// through the same HTML parser and whitelist as the message body before it enters the mail.
pub fn sign_formatted(html: &str, signature: Option<&str>) -> String {
    sign_inner(html, signature, true)
}

fn sign_inner(html: &str, signature: Option<&str>, formatted: bool) -> String {
    let mut nodes = parse(html);
    let Some(signature) = signature.map(str::trim_end).filter(|signature| !signature.trim().is_empty()) else {
        return serialize(&nodes);
    };
    let mut children = vec![Node::Text("-- ".to_owned()), Node::Element(Element::void("br"))];
    if formatted {
        children.extend(parse(&crate::markup::to_html(signature, None)));
    } else {
        for (at, line) in signature.replace("\r\n", "\n").split('\n').enumerate() {
            if at > 0 {
                children.push(Node::Element(Element::void("br")));
            }
            children.push(Node::Text(clean_text(line)));
        }
    }
    let block = Node::Element(Element { name: "div", attrs: vec![("class", SIGNATURE_CLASS.to_owned())], children });
    let quote = nodes.iter().position(|node| matches!(node, Node::Element(element) if element.has_class(QUOTE_CLASS)));
    match quote {
        Some(at) => nodes.insert(at, block),
        None => nodes.push(block),
    }
    serialize(&nodes)
}

/// `text` with `&`, `<`, `>` and `"` written as entities, for text and attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// `url` when it may be a link of a message: `http:`, `https:` or `mailto:` (in any case), at
/// most 2048 bytes; white space and control characters inside are dropped first (browsers ignore
/// them, so `java\tscript:` would otherwise slip through). `None` for anything else.
pub fn safe_url(url: &str) -> Option<String> {
    let url: String = url.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect();
    let lower = url.to_ascii_lowercase();
    let scheme = ["http://", "https://", "mailto:"].iter().find(|scheme| lower.starts_with(**scheme))?;
    (url.len() > scheme.len() && url.len() <= HREF_MAX).then_some(url)
}

// ---------------------------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Element(Element),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    /// One of [`ALLOWED`].
    name: &'static str,
    /// Only the attributes [`kept_attrs`] lets through, values unescaped.
    attrs: Vec<(&'static str, String)>,
    children: Vec<Node>,
}

impl Element {
    fn void(name: &'static str) -> Element {
        Element { name, attrs: Vec::new(), children: Vec::new() }
    }

    fn has_class(&self, class: &str) -> bool {
        self.attrs.iter().any(|(name, value)| *name == "class" && value == class)
    }

    fn attr(&self, wanted: &str) -> Option<&str> {
        self.attrs.iter().find(|(name, _)| *name == wanted).map(|(_, value)| value.as_str())
    }
}

/// Builds the tree of `html` from its tokens, keeping only [`ALLOWED`] elements.
fn parse(html: &str) -> Vec<Node> {
    let mut tree = Tree::default();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(at) = rest.find('<') else {
            tree.text(rest);
            break;
        };
        tree.text(&rest[..at]);
        rest = &rest[at..];
        rest = match token(rest) {
            Token::Comment(after) | Token::Declaration(after) => after,
            Token::Open { name, attrs, after } => {
                if DROPPED.contains(&name.as_str()) {
                    skip_element(&name, after)
                } else {
                    tree.open(&name, &attrs);
                    after
                }
            }
            Token::Close { name, after } => {
                tree.close(&name);
                after
            }
            Token::Text(after) => {
                tree.text("<");
                after
            }
        };
    }
    tree.finish()
}

/// The open elements and what is finished.
#[derive(Default)]
struct Tree {
    root: Vec<Node>,
    stack: Vec<Element>,
}

impl Tree {
    fn children(&mut self) -> &mut Vec<Node> {
        match self.stack.last_mut() {
            Some(open) => &mut open.children,
            None => &mut self.root,
        }
    }

    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let text = clean_text(&decode(raw));
        let children = self.children();
        if let Some(Node::Text(last)) = children.last_mut() {
            last.push_str(&text);
        } else if !text.is_empty() {
            children.push(Node::Text(text));
        }
    }

    fn open(&mut self, name: &str, attrs: &[(String, String)]) {
        let Some(&name) = ALLOWED.iter().find(|allowed| **allowed == name) else { return };
        // What HTML closes by itself: a paragraph before a block, an item before the next item, a
        // cell before the next cell, a row before the next row.
        if is_block(name) && self.stack.last().is_some_and(|open| open.name == "p") {
            self.pop();
        }
        match name {
            "li" => self.close_within("li", &["ul", "ol"]),
            "td" | "th" => {
                self.close_within("td", &["tr", "table"]);
                self.close_within("th", &["tr", "table"]);
            }
            "tr" => self.close_within("tr", &["table"]),
            _ => {}
        }
        let element = Element { name, attrs: kept_attrs(name, attrs), children: Vec::new() };
        if (name == "a" || name == "img") && element.attrs.is_empty() {
            // A link to nowhere safe is its text.
            return;
        }
        if VOID.contains(&name) {
            self.children().push(Node::Element(element));
        } else {
            self.stack.push(element);
        }
    }

    /// Closes the open `name` if it is open above the nearest of `bounds`.
    fn close_within(&mut self, name: &str, bounds: &[&str]) {
        for (at, open) in self.stack.iter().enumerate().rev() {
            if bounds.contains(&open.name) {
                return;
            }
            if open.name == name {
                while self.stack.len() > at {
                    self.pop();
                }
                return;
            }
        }
    }

    fn close(&mut self, name: &str) {
        if let Some(at) = self.stack.iter().rposition(|open| open.name == name) {
            while self.stack.len() > at {
                self.pop();
            }
        }
    }

    fn pop(&mut self) {
        if let Some(done) = self.stack.pop() {
            self.children().push(Node::Element(done));
        }
    }

    fn finish(mut self) -> Vec<Node> {
        while !self.stack.is_empty() {
            self.pop();
        }
        self.root
    }
}

fn is_block(name: &str) -> bool {
    matches!(name, "p" | "div" | "hr" | "blockquote" | "ul" | "ol" | "pre" | "h1" | "h2" | "h3" | "h4" | "table")
}

/// The attributes of `name` that stay ([`sanitize`]).
fn kept_attrs(name: &str, attrs: &[(String, String)]) -> Vec<(&'static str, String)> {
    let value = |wanted: &str| attrs.iter().find(|(name, _)| name == wanted).map(|(_, value)| value.as_str());
    match name {
        "img" => {
            let Some(cid) = value("src").and_then(|s| s.strip_prefix("cid:")).filter(|id| valid_cid(id)) else {
                return Vec::new();
            };
            let mut kept = vec![("src", format!("cid:{cid}"))];
            if let Some(alt) = value("alt") {
                kept.push(("alt", clean_text(alt)));
            }
            for dimension in ["width", "height"] {
                if let Some(n) =
                    value(dimension).and_then(|s| s.parse::<u32>().ok()).filter(|n| (1..=32768).contains(n))
                {
                    kept.push((dimension, n.to_string()));
                }
            }
            kept
        }
        "a" => value("href").and_then(safe_url).map(|href| vec![("href", href)]).unwrap_or_default(),
        "blockquote" if value("type").is_some_and(|kind| kind.trim().eq_ignore_ascii_case("cite")) => {
            vec![("type", "cite".to_owned())]
        }
        "div" => {
            let classes = value("class").unwrap_or_default();
            [QUOTE_CLASS, SIGNATURE_CLASS]
                .into_iter()
                .find(|class| classes.split_whitespace().any(|given| given == *class))
                .map(|class| vec![("class", class.to_owned())])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Text without control characters (line breaks and tabs stay as white space) and without the
/// characters that turn the direction of text around.
fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .filter(|c| matches!(c, '\n' | '\t' | '\r') || !c.is_control())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------

enum Token<'a> {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        after: &'a str,
    },
    Close {
        name: String,
        after: &'a str,
    },
    Comment(&'a str),
    Declaration(&'a str),
    /// A `<` that starts no tag: it is text.
    Text(&'a str),
}

/// The token at the start of `rest`, which starts with `<`.
fn token(rest: &str) -> Token<'_> {
    let after_lt = &rest[1..];
    if let Some(comment) = after_lt.strip_prefix("!--") {
        return Token::Comment(comment.find("-->").map_or("", |end| &comment[end + 3..]));
    }
    if after_lt.starts_with('!') || after_lt.starts_with('?') {
        return Token::Declaration(after_lt.find('>').map_or("", |end| &after_lt[end + 1..]));
    }
    let (closing, body) = match after_lt.strip_prefix('/') {
        Some(body) => (true, body),
        None => (false, after_lt),
    };
    if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Token::Text(after_lt);
    }
    let name_end = body.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(body.len());
    let name = body[..name_end].to_ascii_lowercase();
    let (attrs, after) = attributes(&body[name_end..]);
    if closing { Token::Close { name, after } } else { Token::Open { name, attrs, after } }
}

/// The attributes of a tag up to its `>`, and what follows it. Names in small letters, values
/// with their entities decoded.
fn attributes(mut rest: &str) -> (Vec<(String, String)>, &str) {
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return (attrs, rest);
        }
        if let Some(after) = rest.strip_prefix('>') {
            return (attrs, after);
        }
        let name_end = rest.find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/')).unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (raw, next) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = &after[1..];
                    match inner.find(quote) {
                        Some(end) => (&inner[..end], &inner[end + 1..]),
                        None => (inner, ""),
                    }
                }
                _ => {
                    let end = after.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            value = decode(raw);
            rest = next;
        }
        if !name.is_empty() {
            attrs.push((name, value));
        }
    }
}

/// What follows the element `name` that starts right before `rest`, its content skipped: raw
/// text up to its closing tag, or tags counted so a nested one of the same name does not end it.
/// Without a closing tag everything up to the end goes.
fn skip_element<'a>(name: &str, rest: &'a str) -> &'a str {
    let closing = format!("</{name}");
    let lower = rest.to_ascii_lowercase();
    if RAW_TEXT.contains(&name) {
        return match lower.find(&closing) {
            Some(at) => rest[at..].find('>').map_or("", |end| &rest[at + end + 1..]),
            None => "",
        };
    }
    let opening = format!("<{name}");
    let mut depth = 1;
    let mut at = 0;
    while depth > 0 {
        let next_open = lower[at..].find(&opening).map(|found| found + at);
        let next_close = lower[at..].find(&closing).map(|found| found + at);
        match (next_open, next_close) {
            (Some(open), Some(close)) if open < close => {
                depth += 1;
                at = open + opening.len();
            }
            (_, Some(close)) => {
                depth -= 1;
                at = close + closing.len();
            }
            (_, None) => return "",
        }
    }
    rest[at..].find('>').map_or("", |end| &rest[at + end + 1..])
}

/// `text` with character references decoded: `&amp;` and the other named ones mail uses most,
/// `&#123;` and `&#x7B;`. Anything else stays as it is.
fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';').filter(|end| *end <= 10);
        let decoded = end.and_then(|end| entity(&rest[1..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let named = match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    };
    named.or_else(|| {
        let number = name.strip_prefix('#')?;
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        char::from_u32(code).filter(|c| *c != '\0')
    })
}

// ---------------------------------------------------------------------------------------------
// Writing it out
// ---------------------------------------------------------------------------------------------

fn serialize(nodes: &[Node]) -> String {
    let mut out = String::new();
    write_nodes(nodes, &mut out);
    out
}

fn write_nodes(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(&escape(text)),
            Node::Element(element) => {
                out.push('<');
                out.push_str(element.name);
                for (name, value) in &element.attrs {
                    out.push_str(&format!(" {name}=\"{}\"", escape(value)));
                }
                out.push('>');
                if VOID.contains(&element.name) {
                    continue;
                }
                write_nodes(&element.children, out);
                out.push_str(&format!("</{}>", element.name));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plain text
// ---------------------------------------------------------------------------------------------

/// The lines of `nodes` as blocks ([`to_text`]); `pre` keeps white space as it is.
fn lines_of(nodes: &[Node], pre: bool) -> Vec<String> {
    let mut text = Lines { pre, ..Lines::default() };
    text.walk(nodes);
    text.flush();
    tidy(text.out)
}

#[derive(Default)]
struct Lines {
    out: Vec<String>,
    /// The line being filled with inline text.
    line: String,
    pre: bool,
}

impl Lines {
    fn walk(&mut self, nodes: &[Node]) {
        for node in nodes {
            match node {
                Node::Text(text) => self.push_text(text),
                Node::Element(element) => self.element(element),
            }
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.pre {
            let mut parts = text.split('\n');
            if let Some(first) = parts.next() {
                self.line.push_str(first);
            }
            for part in parts {
                self.out.push(std::mem::take(&mut self.line));
                self.line.push_str(part);
            }
            return;
        }
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !self.line.is_empty() && !self.line.ends_with(' ') {
                    self.line.push(' ');
                }
            } else {
                self.line.push(if c == '\u{a0}' { ' ' } else { c });
            }
        }
    }

    /// Ends the inline line, if it holds anything.
    fn flush(&mut self) {
        let line = std::mem::take(&mut self.line);
        let line = line.trim_end();
        if !line.is_empty() {
            self.out.push(line.to_owned());
        }
    }

    fn block(&mut self, lines: Vec<String>, spaced: bool) {
        self.flush();
        if spaced {
            self.out.push(String::new());
        }
        self.out.extend(lines);
        if spaced {
            self.out.push(String::new());
        }
    }

    fn element(&mut self, element: &Element) {
        match element.name {
            "img" => self.block(vec![element.attr("alt").unwrap_or_default().to_string()], true),
            "br" => {
                let line = std::mem::take(&mut self.line);
                self.out.push(line.trim_end().to_owned());
            }
            "hr" => self.block(vec!["---".to_owned()], true),
            "a" => {
                let before = self.line.len();
                self.walk(&element.children);
                // A block inside the link may have ended the line it started in.
                let shown = self.line.get(before..).unwrap_or_default().trim().to_owned();
                if let Some(href) = element.attr("href") {
                    let bare = href.strip_prefix("mailto:").unwrap_or(href);
                    if shown.is_empty() {
                        self.push_text(href);
                    } else if shown != href && shown != bare {
                        self.push_text(&format!(" <{href}>"));
                    }
                }
            }
            "p" | "h1" | "h2" | "h3" | "h4" | "table" => self.block(lines_of(&element.children, self.pre), true),
            "pre" => self.block(lines_of(&element.children, true), true),
            // The quoted history and the signature stand apart like paragraphs; other divs are lines.
            "div" => {
                let spaced = element.has_class(QUOTE_CLASS) || element.has_class(SIGNATURE_CLASS);
                self.block(lines_of(&element.children, self.pre), spaced);
            }
            "thead" | "tbody" => self.block(lines_of(&element.children, self.pre), false),
            "tr" => {
                let cells: Vec<String> = element
                    .children
                    .iter()
                    .map(|cell| match cell {
                        Node::Element(cell) => lines_of(&cell.children, false).join(" "),
                        Node::Text(text) => text.trim().to_owned(),
                    })
                    .filter(|cell| !cell.is_empty())
                    .collect();
                self.block(vec![cells.join(" | ")], false);
            }
            "blockquote" => {
                let quoted = lines_of(&element.children, self.pre)
                    .into_iter()
                    .map(|line| if line.is_empty() { ">".to_owned() } else { format!("> {line}") })
                    .collect();
                self.block(quoted, true);
            }
            "ul" | "ol" => {
                let numbered = element.name == "ol";
                let mut items = Vec::new();
                let mut number = 0;
                for child in &element.children {
                    let (marker, lines) = match child {
                        Node::Element(item) if item.name == "li" => {
                            number += 1;
                            let marker = if numbered { format!("{number}. ") } else { "- ".to_owned() };
                            (marker, lines_of(&item.children, self.pre))
                        }
                        Node::Element(other) => {
                            ("- ".to_owned(), lines_of(std::slice::from_ref(child), other.name == "pre"))
                        }
                        Node::Text(text) if text.trim().is_empty() => continue,
                        Node::Text(text) => ("- ".to_owned(), vec![text.trim().to_owned()]),
                    };
                    items.extend(item_lines(&marker, lines));
                }
                self.block(items, true);
            }
            "li" => {
                let lines = lines_of(&element.children, self.pre);
                self.block(item_lines("- ", lines), false);
            }
            // b, strong, i, em, u, s, code, td, th outside a row: their text, in line.
            _ => self.walk(&element.children),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The compose window's markup
// ---------------------------------------------------------------------------------------------

/// The characters [`crate::markup`] lets a `\` turn into text.
const MARKS: &str = "\\*_[]()>-#`.";

/// The lines of own text in `nodes` as markup ([`to_markup`]): inline content gathered into lines,
/// each block apart by a blank line.
fn marked_lines(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for (at, node) in nodes.iter().enumerate() {
        match node {
            Node::Text(text) => line.push_str(&marked_text(text)),
            Node::Element(element) if !is_block(element.name) && element.name != "li" => {
                let next = nodes.get(at + 1).and_then(|node| text_of(std::slice::from_ref(node)).chars().next());
                let piece = marked_piece(element, line.chars().next_back(), next);
                line.push_str(&piece);
            }
            Node::Element(element) => {
                end_lines(&mut line, &mut out);
                out.push(String::new());
                out.extend(marked_block(element));
                out.push(String::new());
            }
        }
    }
    end_lines(&mut line, &mut out);
    tidy(out)
}

/// Moves the gathered inline text `line` into `out`, one line per line break, white space
/// collapsed, and a line start the markup would read as a list item or a quote escaped.
fn end_lines(line: &mut String, out: &mut Vec<String>) {
    let gathered = std::mem::take(line);
    for part in gathered.split('\n') {
        let collapsed = part.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ");
        let digits = collapsed.find(|c: char| !c.is_ascii_digit()).filter(|end| (1..=6).contains(end));
        let escaped = if collapsed.starts_with('>') || collapsed.starts_with("- ") {
            format!("\\{collapsed}")
        } else if let Some(end) = digits.filter(|&end| collapsed[end..].starts_with(". ")) {
            // `3. October` would start a numbered list: the dot is text.
            format!("{}\\{}", &collapsed[..end], &collapsed[end..])
        } else {
            collapsed
        };
        out.push(escaped);
    }
}

/// One block element as markup lines.
fn marked_block(element: &Element) -> Vec<String> {
    match element.name {
        "p" | "div" => marked_lines(&element.children),
        "blockquote" => marked_lines(&element.children)
            .into_iter()
            .map(|line| if line.is_empty() { ">".to_owned() } else { format!("> {line}") })
            .collect(),
        "ul" | "ol" => {
            let mut number = 0;
            let mut items = Vec::new();
            for child in &element.children {
                let lines = match child {
                    Node::Element(item) if item.name == "li" => marked_lines(&item.children),
                    Node::Text(text) if text.trim().is_empty() => continue,
                    other => marked_lines(std::slice::from_ref(other)),
                };
                number += 1;
                let marker = if element.name == "ol" { format!("{number}. ") } else { "- ".to_owned() };
                let text = lines.iter().filter(|line| !line.is_empty()).cloned().collect::<Vec<_>>().join(" ");
                items.push(format!("{marker}{text}").trim_end().to_owned());
            }
            items
        }
        "li" => {
            let text = marked_lines(&element.children).into_iter().filter(|line| !line.is_empty());
            vec![format!("- {}", text.collect::<Vec<_>>().join(" ")).trim_end().to_owned()]
        }
        // Headings, tables, code blocks and rules have no markup: their text, as plain text has it.
        _ => lines_of(std::slice::from_ref(&Node::Element(element.clone())), false)
            .into_iter()
            .map(|line| {
                let mut gathered = marked_text(&line);
                let mut out = Vec::new();
                end_lines(&mut gathered, &mut out);
                out.concat()
            })
            .collect(),
    }
}

/// An inline element as markup: the marks around its content, white space at its edges outside
/// them (the markup wants text right inside a mark). `before` and `after` are the characters
/// around it, which its content sees where it has none of its own beside it.
fn marked_inline(element: &Element, before: Option<char>, after: Option<char>) -> String {
    let marked = |inside: Option<char>| marked_children(&element.children, inside, inside);
    let wrap = |mark: &str| {
        let inner = marked(mark.chars().next());
        let trimmed = inner.trim();
        if trimmed.is_empty() || trimmed.contains('\n') {
            return inner.clone();
        }
        let lead = &inner[..inner.len() - inner.trim_start().len()];
        let tail = &inner[inner.trim_end().len()..];
        format!("{lead}{mark}{trimmed}{mark}{tail}")
    };
    match element.name {
        "br" => "\n".to_owned(),
        "strong" | "b" => wrap("**"),
        "em" | "i" => wrap("*"),
        "u" => wrap("__"),
        "a" => {
            let Some(href) = element.attr("href") else { return marked_children(&element.children, before, after) };
            let shown = text_of(&element.children);
            // An address shown as itself became a link by itself (`markup::to_html`).
            if shown.trim() == href && (href.starts_with("http://") || href.starts_with("https://")) {
                return href.to_owned();
            }
            let inner = marked(Some('['));
            let label = inner.trim();
            if label.is_empty() || href.contains([')', ' ']) {
                return marked_children(&element.children, before, after);
            }
            format!("[{label}]({href})")
        }
        // No mark of its own: its content stands where it stands.
        _ => marked_children(&element.children, before, after),
    }
}

/// The markup of inline content between the characters `before` and `after` (`None`: an edge).
fn marked_children(nodes: &[Node], before: Option<char>, after: Option<char>) -> String {
    let mut out = String::new();
    for (at, node) in nodes.iter().enumerate() {
        let next = match nodes.get(at + 1) {
            Some(node) => text_of(std::slice::from_ref(node)).chars().next(),
            None => after,
        };
        let prev = out.chars().next_back().or(before);
        match node {
            Node::Text(text) => out.push_str(&marked_text(text)),
            Node::Element(element) if is_block(element.name) || element.name == "li" => {
                out.push(' ');
                out.push_str(&marked_children(&element.children, None, None));
                out.push(' ');
            }
            Node::Element(element) => {
                let piece = marked_piece(element, prev, next);
                out.push_str(&piece);
            }
        }
    }
    out
}

/// The inline `element` between the characters `before` and `after`: with its marks
/// ([`marked_inline`]), but bold, italic and underline whose text starts or ends right at a letter
/// or digit outside only as their text. The markup reads a mark only at a word's edge, so
/// `**Bold**text` would stay as stars; white space inside the element keeps it apart.
fn marked_piece(element: &Element, before: Option<char>, after: Option<char>) -> String {
    let wrapping = matches!(element.name, "strong" | "b" | "em" | "i" | "u");
    let inner = text_of(&element.children);
    let starts_word = inner.chars().next().is_some_and(|c| !c.is_whitespace());
    let ends_word = inner.chars().next_back().is_some_and(|c| !c.is_whitespace());
    let glued = (starts_word && before.is_some_and(char::is_alphanumeric))
        || (ends_word && after.is_some_and(char::is_alphanumeric));
    if wrapping && glued {
        marked_children(&element.children, before, after)
    } else {
        marked_inline(element, before, after)
    }
}

/// The text inside `nodes`, without any marks.
fn text_of(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            Node::Text(text) => text.clone(),
            Node::Element(element) => text_of(&element.children),
        })
        .collect()
}

/// `text` with white space as single spaces and every character the markup would read as a mark
/// escaped: `\`, `*`, `[`, `]`, and `_` next to another `_` or at the text's edge, where the
/// next node may bring one (only `__` is a mark).
fn marked_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (at, &c) in chars.iter().enumerate() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        let edge = at == 0 || at + 1 == chars.len();
        let doubled = c == '_' && (edge || chars.get(at + 1) == Some(&'_') || at > 0 && chars[at - 1] == '_');
        if (matches!(c, '\\' | '*' | '[' | ']') || doubled) && MARKS.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The lines of one list item: the marker before the first, as many spaces before the others.
fn item_lines(marker: &str, lines: Vec<String>) -> Vec<String> {
    let indent = " ".repeat(marker.len());
    let mut lines = lines.into_iter().filter(|line| !line.is_empty()).peekable();
    let mut out = Vec::new();
    let first = lines.next().unwrap_or_default();
    out.push(format!("{marker}{first}").trim_end().to_owned());
    out.extend(lines.map(|line| format!("{indent}{line}")));
    out
}

/// No blank line at the start or the end, never two in a row, no white space at the end of a line.
fn tidy(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for line in lines {
        let line = line.trim_end().to_owned();
        if line.is_empty() && out.last().is_none_or(String::is_empty) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn rich_paste_keeps_the_plain_alternatives_edge_whitespace() {
        let paste = super::prepare_paste(Some("<b>world</b> "), Some("world ")).expect("paste");
        assert_eq!(paste.markup, "**world** ");
        assert_eq!(
            crate::markup::to_html(&format!("{}again", paste.markup), None),
            "<p><strong>world</strong> again</p>"
        );
        let paste = super::prepare_paste(Some(" <i>world</i> "), None).expect("HTML-only");
        assert_eq!(paste.markup, " *world* ");
        for html in ["", " \n"] {
            let paste = super::prepare_paste(Some(html), Some(" hello ")).expect("text alternative");
            assert_eq!(paste.markup, " hello ");
            assert!(!paste.formatted);
        }
    }

    #[test]
    fn office_marker_detection_never_recurses_through_unknown_tag_mismatches() {
        let html = format!("<p style='mso-list:l0'>{}1. A</p>", "<x><b></x>".repeat(10_000));
        let paste = super::prepare_paste(Some(&html), None).expect("flat normalized tree");
        assert_eq!(paste.markup, "1. 1\\. A");
    }
    #[test]
    fn paste_browser_office_and_mail_are_safe_without_changing_draft_rules() {
        for (input, expected) in [
            (
                include_str!("../tests/fixtures/paste/browser.html"),
                vec!["**世界**", "*team*", "__today__", "- One", "> Quote", "[Website](https://example.org)", "A | B"],
            ),
            (
                include_str!("../tests/fixtures/paste/office.html"),
                vec!["**Bold**", "*Italic*", "__Underlined__", "- First", "- Second"],
            ),
            (
                include_str!("../tests/fixtures/paste/mail.html"),
                vec!["**Signature text**", "*format*", "> Older text", "partwordtail", "\\*\\*literal\\*\\*", "Unsafe"],
            ),
        ] {
            let paste = super::prepare_paste(Some(input), None).expect("paste");
            assert!(paste.formatted);
            for expected in expected {
                assert!(paste.markup.contains(expected), "{expected}: {paste:?}");
            }
            let rendered = crate::markup::to_html(&paste.markup, None);
            for forbidden in ["<img", "cid:", "style=", "class=", "javascript:", "steal()", "Hidden", "bad()"] {
                assert!(!rendered.contains(forbidden), "{forbidden}: {rendered}");
            }
        }
        assert!(!super::prepare_paste(Some("part<b>word</b>tail"), None).expect("paste").formatted);
    }

    #[test]
    fn paste_plain_alternative_masks_quotes_lists_and_unicode_without_becoming_history() {
        let text = "> 世界\n- item\n3. numbered\n**literal** [link](https://example.org)";
        let paste = super::prepare_paste(Some("<b>HTML</b>"), Some(text)).expect("paste");
        assert_eq!(paste.plain, text);
        assert!(paste.plain_markup.starts_with("\\> 世界\n\\- item\n3\\. numbered"));
        let rendered = crate::markup::to_html(&paste.plain_markup, None);
        assert!(!rendered.contains("<blockquote") && !rendered.contains("<li>") && !rendered.contains("<strong>"));
        assert_eq!(
            super::prepare_paste(None, None).expect("empty"),
            super::Paste { markup: String::new(), plain: String::new(), plain_markup: String::new(), formatted: false }
        );
    }

    #[test]
    fn office_numbered_paragraphs_form_one_ordered_list() {
        let html = "<p style='mso-list:l0 level1 lfo1'><span style='mso-list:Ignore'>1.&nbsp;</span>First</p>\n<p style='mso-list:l0 level1 lfo1'><span style='mso-list:Ignore'>2.&nbsp;</span>Second</p>";
        let paste = super::prepare_paste(Some(html), None).expect("office");
        assert_eq!(paste.markup, "1. First\n2. Second");
        assert!(paste.formatted);
    }

    #[test]
    fn styled_void_elements_never_accumulate_open_wrappers() {
        let html = format!("{}x", "<br style='font-weight:bold'>".repeat(20_000));
        let paste = super::prepare_paste(Some(&html), None).expect("flat void elements");
        assert_eq!(paste.markup, "x");
        assert!(!paste.formatted);
        assert_eq!(
            super::prepare_paste(Some(&"<div style='font-weight:bold'>".repeat(40)), None),
            Err(super::PasteError::TooDeep)
        );
    }

    #[test]
    fn paste_depth_is_checked_even_in_unknown_and_discarded_trees_and_outputs_are_bounded() {
        for tag in ["div", "span", "svg", "unknown"] {
            let deep = format!("{}x{}", format!("<{tag}>").repeat(65), format!("</{tag}>").repeat(65));
            assert_eq!(super::prepare_paste(Some(&deep), None), Err(super::PasteError::TooDeep));
        }
        assert_eq!(super::prepare_paste(None, Some(&"*".repeat(600_000))), Err(super::PasteError::TooLarge));
    }
    #[test]
    fn cid_images_are_bounded_escaped_and_stable() {
        let dirty = "<p>Before</p><img src='cid:one@torro' alt='&lt;plan&gt;' width='640' height='99999' style='x' onerror='bad()'><img src='https://tracker/x'><img src='data:image/png;base64,AAAA'><img src='cid:bad%0aid'><p>After</p>";
        let clean = super::sanitize(dirty);
        assert_eq!(clean, "<p>Before</p><img src=\"cid:one@torro\" alt=\"&lt;plan&gt;\" width=\"640\"><p>After</p>");
        assert_eq!(super::sanitize(&clean), clean);
        assert_eq!(super::image_refs(&clean), ["one@torro"]);
        assert_eq!(super::to_text(&clean), "Before\n\n<plan>\n\nAfter");
        let blocks = super::to_blocks("<p>Ü</p><div class='torro-quote'><p>Old</p><img src='cid:one@torro'></div>");
        assert!(matches!(blocks.last(), Some(super::HtmlBlock::Image { quoted: true, .. })));
    }
    use super::*;

    #[test]
    fn scripts_styles_images_and_handlers_go_and_formatting_stays() {
        let dirty = concat!(
            "<html><head><title>T</title><style>p{color:red}</style></head><body onload=\"x()\">",
            "<p class=\"a\" style=\"color:red\" onclick=\"steal()\">Hi <b>Anna</b>,<br>",
            "<img src=\"https://tracker.example/p.gif\"><script>alert(1)</script>",
            "<i>see</i> <u>this</u> <a href=\"javascript:alert(1)\">bad</a> ",
            "<a href=\"https://example.org/x?a=1&amp;b=2\" target=\"_blank\">good</a></p>",
            "<iframe src=\"https://evil.example\"><p>inside</p></iframe>",
            "<form action=\"https://evil.example\"><input name=p>Name</form>",
            "<svg><a href=\"https://x\">svg</a></svg><!-- note --></body></html>",
        );
        let clean = sanitize(dirty);
        assert_eq!(
            clean,
            concat!(
                "<p>Hi <b>Anna</b>,<br><i>see</i> <u>this</u> bad ",
                "<a href=\"https://example.org/x?a=1&amp;b=2\">good</a></p>Name",
            )
        );
        assert_eq!(sanitize(&clean), clean, "cleaning twice changes nothing");
    }

    #[test]
    fn links_only_go_to_the_web_or_to_mail() {
        for (href, kept) in [
            ("https://example.org", true),
            ("HTTP://example.org", true),
            ("mailto:anna@example.org", true),
            ("java\tscript:alert(1)", false),
            (" javascript:alert(1)", false),
            ("data:text/html,<b>x</b>", false),
            ("vbscript:x", false),
            ("/relative", false),
            ("https://", false),
        ] {
            let html = sanitize(&format!("<a href=\"{}\">x</a>", escape(href)));
            assert_eq!(html.contains("href"), kept, "{href:?}: {html}");
        }
        assert_eq!(sanitize("<a href=\"https://a.example/&quot;onmouseover=&quot;x\">x</a>").matches('"').count(), 2);
    }

    #[test]
    fn broken_markup_is_closed_and_text_is_escaped() {
        assert_eq!(sanitize("<b>bold <i>both</b> after"), "<b>bold <i>both</i></b> after");
        assert_eq!(sanitize("1 < 2 & 3 > 2 <3"), "1 &lt; 2 &amp; 3 &gt; 2 &lt;3");
        assert_eq!(sanitize("<ul><li>one<li>two</ul>"), "<ul><li>one</li><li>two</li></ul>");
        assert_eq!(sanitize("<p>one<p>two"), "<p>one</p><p>two</p>");
        assert_eq!(sanitize("</div>text<br/>"), "text<br>");
        assert_eq!(sanitize("<script>never closed <b>x</b>"), "");
        assert_eq!(sanitize("<style>a{}</STYLE >after"), "after");
        assert_eq!(sanitize("<svg><svg></svg>still svg</svg>out"), "out");
        assert_eq!(sanitize("&#x3C;script&#62; &bogus; &#0;"), "&lt;script&gt; &amp;bogus; &amp;#0;");
        assert_eq!(sanitize("a\u{202E}b\u{7}c"), "abc", "direction and control characters go");
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn only_the_cite_type_and_the_suites_classes_stay() {
        assert_eq!(
            sanitize("<blockquote type=\"CITE\" cite=\"mid:1\">q</blockquote><blockquote style=x>r</blockquote>"),
            "<blockquote type=\"cite\">q</blockquote><blockquote>r</blockquote>"
        );
        assert_eq!(
            sanitize("<div class=\"x torro-quote\" id=a>h</div><div class=other>o</div>"),
            "<div class=\"torro-quote\">h</div><div>o</div>"
        );
    }

    #[test]
    fn plain_text_keeps_paragraphs_quotes_lists_and_link_addresses() {
        let html = concat!(
            "<p>Hi Anna,<br>the <b>numbers</b>   are\n in.</p>",
            "<ul><li>one</li><li>two<br>lines</li></ul>",
            "<ol><li>first</li><li>second</li></ol>",
            "<p>See <a href=\"https://example.org/q4\">the sheet</a> or ",
            "<a href=\"https://example.org\">https://example.org</a> or ",
            "<a href=\"mailto:jonas@example.org\">jonas@example.org</a>.</p>",
            "<div class=\"torro-quote\"><p>On Monday, Jonas wrote:</p>",
            "<blockquote type=\"cite\">Hello<br><br>old<blockquote>older</blockquote></blockquote></div>",
        );
        assert_eq!(
            to_text(html),
            [
                "Hi Anna,",
                "the numbers are in.",
                "",
                "- one",
                "- two",
                "  lines",
                "",
                "1. first",
                "2. second",
                "",
                "See the sheet <https://example.org/q4> or https://example.org or jonas@example.org.",
                "",
                "On Monday, Jonas wrote:",
                "",
                "> Hello",
                ">",
                "> old",
                ">",
                "> > older",
            ]
            .join("\n")
        );
        assert_eq!(to_text("<div>a</div><div>b</div>"), "a\nb", "divs are lines");
        assert_eq!(to_text("<pre>  keep\n   this</pre>"), "  keep\n   this");
        assert_eq!(to_text("<table><tr><td>a</td><td>b</td></tr><tr><th>c</th></tr></table>"), "a | b\nc");
        assert_eq!(to_text("<script>x</script>"), "");
    }

    #[test]
    fn the_signature_goes_above_the_quoted_history_or_at_the_end() {
        let reply = "<p>Thanks!</p><div class=\"torro-quote\"><p>On Monday, Jonas wrote:</p><blockquote type=\"cite\">Hi</blockquote></div>";
        let signed = sign(reply, Some("Anna Berg\nProduct <Torro>\n"));
        assert_eq!(
            signed,
            concat!(
                "<p>Thanks!</p><div class=\"torro-signature\">-- <br>Anna Berg<br>Product &lt;Torro&gt;</div>",
                "<div class=\"torro-quote\"><p>On Monday, Jonas wrote:</p><blockquote type=\"cite\">Hi</blockquote></div>",
            )
        );
        assert_eq!(
            to_text(&signed),
            "Thanks!\n\n-- \nAnna Berg\nProduct <Torro>\n\nOn Monday, Jonas wrote:\n\n> Hi",
            "the separator keeps its space"
        );
        assert_eq!(sign("<p>New</p>", Some("Anna")), "<p>New</p><div class=\"torro-signature\">-- <br>Anna</div>");
        assert_eq!(sign("<p>New</p>", Some("  \n")), "<p>New</p>");
        assert_eq!(sign("<p>New<script>x</script></p>", None), "<p>New</p>", "cleaned too");
    }

    #[test]
    fn a_formatted_signature_keeps_safe_marks_and_a_plain_text_copy() {
        let signed = sign_formatted(
            "<p>Thanks!</p><div class=\"torro-quote\">Old</div>",
            Some("**Anna** · [Profile](https://example.org)\n- Support\n<script>bad</script>"),
        );
        assert!(signed.contains("<strong>Anna</strong>"), "{signed}");
        assert!(signed.contains("<a href=\"https://example.org\">Profile</a>"), "{signed}");
        assert!(signed.contains("<ul><li>Support</li></ul>"), "{signed}");
        assert!(!signed.contains("<script>"), "{signed}");
        assert!(signed.find("torro-signature").unwrap() < signed.find("torro-quote").unwrap());
        let plain = to_text(&signed);
        assert!(plain.contains("Anna · Profile <https://example.org>"), "{plain}");
    }

    /// Issue #502: the HTML the compose window made becomes its text again, so a saved draft can
    /// be written on and saved as it was.
    #[test]
    fn the_compose_windows_html_becomes_its_markup_again() {
        use crate::markup::to_html;
        let formatted = concat!(
            "Hi Jonas,\n\nthe numbers look **fine** to me, *except* the travel budget. ",
            "See [the sheet](https://intranet.work.example.org/budget/q4).\n\n",
            "- Flights\n- Hotels\n\n1. Check\n2. __Approve__\n\n> Could you look before Thursday?\n\nAnna",
        );
        let html = to_html(formatted, None);
        let (text, history) = to_markup(&html);
        assert_eq!(history, None);
        assert_eq!(text, formatted, "the same marks");
        assert_eq!(to_html(&text, None), html);

        let answer = concat!(
            "\n\nThanks, **see you** at https://example.org/room.\nAnna\n\n",
            "On Monday, Bernd wrote:\n> Lunch at *noon*?\n>\n> > Earlier text\n> Bernd",
        );
        let attribution = "On Monday, Bernd wrote:";
        let html = to_html(answer, Some(attribution));
        let (text, history) = to_markup(&html);
        assert_eq!(history.as_deref(), Some(attribution));
        assert!(text.contains("Lunch at *noon*?"), "the history keeps its text as it is: {text}");
        assert_eq!(to_html(&text, history.as_deref()), html);
    }

    #[test]
    fn text_that_looks_like_marks_stays_text() {
        use crate::markup::to_html;
        let html = "<p>- not a list, 2*3*4, a__b, [x] and C:\\dir</p><blockquote><p>&gt; not a quote</p></blockquote>";
        let (text, history) = to_markup(html);
        assert_eq!(history, None);
        assert_eq!(to_html(&text, None), html, "{text}");
        let (heading, _) = to_markup("<h1>Title</h1><p><b>Bold</b>text and x<em>y</em></p>");
        assert_eq!(heading, "Title\n\nBoldtext and xy", "marks at a letter would stay stars");
        // Issue #502, second review: nested and wrapped marks see the letters outside them, white
        // space inside keeps them apart, and an underscore before an underline stays text.
        for (html, markup) in [
            ("<p>x<b><i>y</i></b></p>", "xy"),
            ("<p><s><b>y</b></s>z</p>", "yz"),
            ("<p>Hello<b> world</b></p>", "Hello **world**"),
            ("<p><b>x </b>y</p>", "**x** y"),
            ("<p>_<u>x</u></p>", "\\___x__"),
        ] {
            assert_eq!(to_markup(html).0, markup, "{html}");
        }
        assert_eq!(to_html(&to_markup("<p>_<u>x</u></p>").0, None), "<p>_<u>x</u></p>");
        let date = "<p>3. October suits me.</p>";
        assert_eq!(to_markup(date).0, "3\\. October suits me.");
        assert_eq!(to_html(&to_markup(date).0, None), date, "no numbered list");
        let plain = "Price 2 * 3, see *the file*.\n- not a list\n> quoted **text**\n>\n>> deeper";
        let markup = plain_to_markup(plain);
        assert_eq!(
            markup,
            "Price 2 \\* 3, see \\*the file\\*.\n\\- not a list\n> quoted \\*\\*text\\*\\*\n>\n>> deeper"
        );
        assert_eq!(
            to_html(&markup, None),
            concat!(
                "<p>Price 2 * 3, see *the file*.<br>- not a list</p>",
                "<blockquote><p>quoted **text**</p><blockquote><p>deeper</p></blockquote></blockquote>",
            )
        );
        assert_eq!(to_markup("").0, "");
    }
}
