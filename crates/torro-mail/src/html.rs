//! The HTML of mail the suite writes (issue #125): the small, safe subset of HTML that the compose
//! window makes ([`crate::markup`]) and that a client may send with `mail.save_draft`
//! (`Draft::html`), its plain-text form, and the account's signature put into it.
//!
//! - [`sanitize`] keeps only that subset: paragraphs, line breaks, headings, bold, italic,
//!   underline, strike-through, code, lists, quotes, simple tables, rules and links to `http`,
//!   `https` and `mailto`. Everything else goes: scripts, styles, forms, frames, images (so no
//!   tracking pixel and nothing fetched from elsewhere), event handlers, every other attribute.
//!   The core cleans every HTML a draft brings before it stores or sends it, so what a person
//!   approves is what goes out.
//! - [`to_text`] is the plain text sent beside the HTML (`multipart/alternative`) and shown where
//!   the text of a draft is shown: paragraphs, `> ` before quoted lines, `- ` and `1. ` before
//!   list items, a link's address behind its text.
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
];

/// Elements without content or closing tag.
const VOID: &[&str] = &["br", "hr"];

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
        if name == "a" && element.attrs.is_empty() {
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
}
