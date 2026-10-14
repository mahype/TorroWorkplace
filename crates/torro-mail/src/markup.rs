//! The text of the compose window as HTML (issue #125): a small, Markdown-like markup that the
//! window's toolbar and keys write (`**bold**`, `*italic*`, `__underline__`, `[text](address)`,
//! `- ` and `1. ` lists, `> ` quotes), rendered to the HTML of [`crate::html`] when the draft is
//! saved. People see the marks while writing; recipients get formatted mail, and the plain text
//! beside it ([`crate::html::to_text`]).
//!
//! Unlike Markdown, a single line break stays a line break (`<br>`): mail is written line by line.
//! The quoted history of an answer is someone else's text, so it is not read as markup at all:
//! only its quote marks and lines count ([`to_html`]).

use std::ops::Range;

use crate::html::{QUOTE_CLASS, escape, safe_url};

/// Characters a backslash turns into plain text.
const ESCAPABLE: &str = "\\*_[]()>-#`";

/// The HTML of `text` (line ends `\n` or `\r\n`):
///
/// - **Blocks:** lines separated by blank lines are paragraphs (`<p>`, lines inside joined with
///   `<br>`); lines starting with `- ` or `* ` are items of a bulleted list, with a number and
///   `. ` (`1. `) of a numbered one; lines starting with `>` are a quote (`<blockquote>`), whose
///   lines without the `>` (and one space after it) are blocks again, so `> >` nests.
/// - **Inline:** `**bold**` (`<strong>`), `*italic*` (`<em>`), `__underline__` (`<u>`),
///   `[text](address)` (`<a>`, only `http:`, `https:` and `mailto:` addresses, else the text stays
///   as typed), and addresses starting with `http://` or `https://` become links by themselves.
///   A mark only counts at a word's edge with text right inside it (`2*3*4` and `snake__case`
///   stay), and `\` before a mark keeps the mark as text.
/// - **History:** when `history` is given (the "On … wrote:" line of an answer, or the first line
///   of a forwarded message's header) and a line of `text` is that line (white space at the ends
///   ignored), that line and everything after it is the quoted history: wrapped in
///   `<div class="torro-quote">` ([`QUOTE_CLASS`], the signature goes in front of it), quotes as
///   `<blockquote type="cite">`, all other text escaped as it is, no lists, no inline marks.
///
/// Everything else is escaped; the answer is HTML [`crate::html::sanitize`] leaves unchanged.
/// Blank text answers an empty string.
pub fn to_html(text: &str, history: Option<&str>) -> String {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = history
        .map(str::trim)
        .filter(|history| !history.is_empty())
        .and_then(|history| lines.iter().position(|line| line.trim() == history));
    let (own, theirs) = lines.split_at(start.unwrap_or(lines.len()));
    let mut html = blocks(own, true);
    if !theirs.is_empty() {
        html.push_str(&format!("<div class=\"{QUOTE_CLASS}\">{}</div>", blocks(theirs, false)));
    }
    html
}

/// The blocks of `lines`; `rich` reads lists and inline marks (own text), else only quotes.
fn blocks(lines: &[&str], rich: bool) -> String {
    let mut html = String::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim().is_empty() {
            at += 1;
        } else if quoted(line).is_some() {
            let end = run(lines, at, |line| quoted(line).is_some());
            let inner: Vec<&str> = lines[at..end].iter().filter_map(|line| quoted(line)).collect();
            let open = if rich { "<blockquote>" } else { "<blockquote type=\"cite\">" };
            html.push_str(&format!("{open}{}</blockquote>", blocks(&inner, rich)));
            at = end;
        } else if let Some(kind) = rich.then(|| item(line)).flatten() {
            let end = run(lines, at, |line| item(line).is_some_and(|(other, _)| other == kind.0));
            let items: String = lines[at..end]
                .iter()
                .filter_map(|line| item(line))
                .map(|(_, rest)| format!("<li>{}</li>", inline(rest)))
                .collect();
            let tag = if kind.0 == List::Numbered { "ol" } else { "ul" };
            html.push_str(&format!("<{tag}>{items}</{tag}>"));
            at = end;
        } else {
            let end = run(lines, at, |line| {
                !line.trim().is_empty() && quoted(line).is_none() && !(rich && item(line).is_some())
            });
            let text: Vec<String> = lines[at..end]
                .iter()
                .map(|line| if rich { inline(line.trim_end()) } else { escape(line.trim_end()) })
                .collect();
            html.push_str(&format!("<p>{}</p>", text.join("<br>")));
            at = end;
        }
    }
    html
}

/// The end of the run of lines from `start` (which belongs to it) for which `belongs` holds.
fn run(lines: &[&str], start: usize, belongs: impl Fn(&str) -> bool) -> usize {
    lines[start + 1..].iter().position(|line| !belongs(line)).map_or(lines.len(), |found| start + 1 + found)
}

/// The rest of a quoted line without its `>` and one space after it; `None` for other lines.
fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Bulleted,
    Numbered,
}

/// The kind of list item `line` is and its text; `None` for other lines.
fn item(line: &str) -> Option<(List, &str)> {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some((List::Bulleted, rest));
    }
    let digits = line.find(|c: char| !c.is_ascii_digit()).filter(|end| (1..=6).contains(end))?;
    line[digits..].strip_prefix(". ").map(|rest| (List::Numbered, rest))
}

/// How a piece of one line of the compose window's text looks while it is written ([`looks`]): the
/// window's editor shows marks quietly and the text between them as it will look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    /// A mark (`**`, `*`, `__`, `[`, `](address)`, `\`, the `- ` or `1. ` of an item, the `>` of a
    /// quote): it does not show in the message.
    pub mark: bool,
    pub strong: bool,
    pub emphasis: bool,
    pub underline: bool,
    /// The text of a link, or an address that becomes one.
    pub link: bool,
    /// A quoted line.
    pub quote: bool,
}

/// The looks of `line`, one line of the compose window's text, read as [`to_html`] reads own text:
/// byte ranges of `line` in order, none overlapping, each with its [`Look`]. Text without any look
/// is left out. A quoted line is a quote as a whole (its `>` marks), with no inline marks: in an
/// answer it is mostly someone else's text. A list item's `- ` or `1. ` is a mark.
pub fn looks(line: &str) -> Vec<(Range<usize>, Look)> {
    let mut out = Vec::new();
    let start = line.len() - line.trim_start().len();
    if quoted(line).is_some() {
        let marks = line[start..].find(|c: char| c != '>' && c != ' ').map_or(line.len(), |end| start + end);
        out.push((start..marks, Look { mark: true, quote: true, ..Look::default() }));
        if marks < line.len() {
            out.push((marks..line.len(), Look { quote: true, ..Look::default() }));
        }
        return out;
    }
    let mut from = 0;
    if let Some((_, rest)) = item(line) {
        from = line.len() - rest.len();
        out.push((start..from, Look { mark: true, ..Look::default() }));
    }
    let mut events = Vec::new();
    walk(&line[from..], from, &mut events);
    let mut look = Look::default();
    let mut push = |range: Range<usize>, look: Look| {
        if !range.is_empty() && look != Look::default() {
            out.push((range, look));
        }
    };
    for event in events {
        let mark = Look { mark: true, ..look };
        match event {
            Event::Text(range) => push(range, look),
            Event::Bare { range, .. } => push(range, Look { link: true, ..look }),
            Event::Escaped(range, _) => {
                push(range.start..range.start + 1, mark);
                push(range.start + 1..range.end, look);
            }
            Event::Open(tag, range) => {
                push(range, mark);
                set(&mut look, tag, true);
            }
            Event::Close(tag, range) => {
                set(&mut look, tag, false);
                push(range, Look { mark: true, ..look });
            }
            Event::LinkOpen { mark: range, .. } => {
                push(range, mark);
                look.link = true;
            }
            Event::LinkClose { mark: range } => {
                look.link = false;
                push(range, Look { mark: true, ..look });
            }
        }
    }
    out
}

fn set(look: &mut Look, tag: Tag, on: bool) {
    match tag {
        Tag::Strong => look.strong = on,
        Tag::Emphasis => look.emphasis = on,
        Tag::Underline => look.underline = on,
    }
}

/// An inline mark that wraps text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Strong,
    Emphasis,
    Underline,
}

impl Tag {
    fn html(self) -> &'static str {
        match self {
            Tag::Strong => "strong",
            Tag::Emphasis => "em",
            Tag::Underline => "u",
        }
    }
}

/// What reading a line of own text finds, in order; ranges are bytes of the whole line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    /// Text as it is.
    Text(Range<usize>),
    /// A `\` and the character after it, which stays text.
    Escaped(Range<usize>, char),
    /// The opening mark of a tag.
    Open(Tag, Range<usize>),
    /// The closing mark of a tag.
    Close(Tag, Range<usize>),
    /// `[` of a link to `href`; its text follows, then [`Event::LinkClose`].
    LinkOpen { href: String, mark: Range<usize> },
    /// `](address)` of a link.
    LinkClose { mark: Range<usize> },
    /// An address that becomes a link by itself.
    Bare { href: String, range: Range<usize> },
}

/// The inline marks of one line of own text, as HTML.
fn inline(text: &str) -> String {
    let mut events = Vec::new();
    walk(text, 0, &mut events);
    let mut out = String::new();
    for event in events {
        match event {
            Event::Text(range) => out.push_str(&escape(&text[range])),
            Event::Escaped(_, c) => out.push_str(&escape(&c.to_string())),
            Event::Open(tag, _) => out.push_str(&format!("<{}>", tag.html())),
            Event::Close(tag, _) => out.push_str(&format!("</{}>", tag.html())),
            Event::LinkOpen { href, .. } => out.push_str(&format!("<a href=\"{}\">", escape(&href))),
            Event::LinkClose { .. } => out.push_str("</a>"),
            Event::Bare { href, .. } => out.push_str(&format!("<a href=\"{0}\">{0}</a>", escape(&href))),
        }
    }
    out
}

/// Reads `text` (which starts at byte `base` of its line) into `events`.
fn walk(text: &str, base: usize, events: &mut Vec<Event>) {
    let mut at = 0;
    let mut plain = 0;
    let mut before: Option<char> = None;
    let flush = |events: &mut Vec<Event>, from: usize, to: usize| {
        if from < to {
            events.push(Event::Text(base + from..base + to));
        }
    };
    while let Some(c) = text[at..].chars().next() {
        let rest = &text[at..];
        let at_edge = !before.is_some_and(char::is_alphanumeric);
        if c == '\\'
            && let Some(next) = rest[1..].chars().next().filter(|next| ESCAPABLE.contains(*next))
        {
            flush(events, plain, at);
            let used = 1 + next.len_utf8();
            events.push(Event::Escaped(base + at..base + at + used, next));
            at += used;
            plain = at;
            before = Some(next);
            continue;
        }
        if at_edge && let Some((tag, mark, end)) = marked(rest) {
            flush(events, plain, at);
            let inner = at + mark.len();
            events.push(Event::Open(tag, base + at..base + inner));
            walk(&text[inner..inner + end], base + inner, events);
            let close = inner + end;
            events.push(Event::Close(tag, base + close..base + close + mark.len()));
            at = close + mark.len();
            plain = at;
            before = mark.chars().next_back();
            continue;
        }
        if c == '['
            && let Some((href, label_end, used)) = link(rest)
        {
            flush(events, plain, at);
            events.push(Event::LinkOpen { href, mark: base + at..base + at + 1 });
            walk(&text[at + 1..at + label_end], base + at + 1, events);
            events.push(Event::LinkClose { mark: base + at + label_end..base + at + used });
            at += used;
            plain = at;
            before = Some(')');
            continue;
        }
        if at_edge
            && (rest.starts_with("http://") || rest.starts_with("https://"))
            && let Some((href, used)) = bare_link(rest)
        {
            flush(events, plain, at);
            events.push(Event::Bare { href, range: base + at..base + at + used });
            at += used;
            plain = at;
            before = rest[..used].chars().next_back();
            continue;
        }
        before = Some(c);
        at += c.len_utf8();
    }
    flush(events, plain, at);
}

/// `**…**`, `__…__` or `*…*` at the start of `rest`: the tag, its mark and where the closing mark
/// starts after the opening one.
fn marked(rest: &str) -> Option<(Tag, &'static str, usize)> {
    for (mark, tag) in [("**", Tag::Strong), ("__", Tag::Underline), ("*", Tag::Emphasis)] {
        let Some(inner) = rest.strip_prefix(mark) else { continue };
        if let Some(end) = closing(inner, mark) {
            return Some((tag, mark, end));
        }
    }
    None
}

/// Where the `mark` that closes `inner` (the text after the opening mark) starts: text right
/// inside both marks, no letter or digit right after the closing one, and for `*` not part of
/// `**`. `None` when there is none.
fn closing(inner: &str, mark: &str) -> Option<usize> {
    if inner.starts_with(char::is_whitespace) || inner.starts_with(mark) {
        return None;
    }
    let mut from = 0;
    while let Some(found) = inner[from..].find(mark).map(|found| found + from) {
        let after = &inner[found + mark.len()..];
        let inside_ok = found > 0 && !inner[..found].ends_with(char::is_whitespace);
        let edge_ok = !after.starts_with(char::is_alphanumeric);
        let single_ok = mark != "*" || (!after.starts_with('*') && !inner[..found].ends_with('*'));
        if inside_ok && edge_ok && single_ok {
            return Some(found);
        }
        from = found + mark.len();
    }
    None
}

/// `[text](address)` at the start of `rest`, with text and an address [`safe_url`] takes: the
/// address, where `](` starts and the bytes it all takes.
fn link(rest: &str) -> Option<(String, usize, usize)> {
    let label_end = rest.find("](")?;
    if rest[1..label_end].trim().is_empty() {
        return None;
    }
    let after = &rest[label_end + 2..];
    let url_end = after.find(')')?;
    let href = safe_url(&after[..url_end]).filter(|_| !after[..url_end].contains(char::is_whitespace))?;
    Some((href, label_end, label_end + 2 + url_end + 1))
}

/// An address at the start of `rest` up to white space or `<`, `>`, `"`, without the punctuation
/// that ends a sentence after it: the link and the bytes it takes; `None` when it is no address.
fn bare_link(rest: &str) -> Option<(String, usize)> {
    let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(rest.len());
    let address = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'']);
    safe_url(address).map(|href| (href, address.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::{sanitize, sign, to_text};

    #[test]
    fn paragraphs_keep_their_line_breaks() {
        assert_eq!(to_html("Hi Anna,\nsee below.\n\n\nJonas", None), "<p>Hi Anna,<br>see below.</p><p>Jonas</p>");
        assert_eq!(to_html("\n\n  \n", None), "");
        assert_eq!(to_html("a < b & \"c\"", None), "<p>a &lt; b &amp; &quot;c&quot;</p>");
    }

    #[test]
    fn marks_become_bold_italic_underline_and_links() {
        assert_eq!(
            to_html("**bold** *italic* __under__ **a *b* c**", None),
            "<p><strong>bold</strong> <em>italic</em> <u>under</u> <strong>a <em>b</em> c</strong></p>"
        );
        assert_eq!(
            to_html("[the sheet](https://example.org/q4?a=1&b=2) and https://example.org/x.", None),
            concat!(
                "<p><a href=\"https://example.org/q4?a=1&amp;b=2\">the sheet</a> and ",
                "<a href=\"https://example.org/x\">https://example.org/x</a>.</p>",
            )
        );
        assert_eq!(
            to_html("[mail](mailto:anna@example.org)", None),
            "<p><a href=\"mailto:anna@example.org\">mail</a></p>"
        );
    }

    #[test]
    fn marks_that_are_no_marks_stay_as_typed() {
        for (typed, html) in [
            ("2*3*4", "<p>2*3*4</p>"),
            ("snake__case__name", "<p>snake__case__name</p>"),
            ("** not bold**", "<p>** not bold**</p>"),
            ("*open", "<p>*open</p>"),
            ("\\*literal\\*", "<p>*literal*</p>"),
            ("[x](javascript:alert(1))", "<p>[x](javascript:alert(1))</p>"),
            ("[x](https://a.example/ b)", "<p>[x](<a href=\"https://a.example/\">https://a.example/</a> b)</p>"),
            ("<b>raw</b>", "<p>&lt;b&gt;raw&lt;/b&gt;</p>"),
        ] {
            assert_eq!(to_html(typed, None), html, "{typed}");
        }
    }

    #[test]
    fn lists_and_quotes_are_blocks() {
        assert_eq!(
            to_html("Items:\n- one\n* two\n1. first\n2. second\nafter", None),
            concat!(
                "<p>Items:</p><ul><li>one</li><li>two</li></ul><ol><li>first</li><li>second</li></ol>",
                "<p>after</p>",
            )
        );
        assert_eq!(
            to_html("> quoted **bold**\n>\n> > deeper\nmine", None),
            concat!(
                "<blockquote><p>quoted <strong>bold</strong></p><blockquote><p>deeper</p></blockquote></blockquote>",
                "<p>mine</p>",
            )
        );
    }

    #[test]
    fn the_history_of_an_answer_is_quoted_as_it_is() {
        let text = "Thanks, **will do**.\n\nOn Monday, Jonas <jonas@example.org> wrote:\n> Hi *Anna*,\n>\n> - see [x](https://x.example)\n> > older";
        let html = to_html(text, Some("On Monday, Jonas <jonas@example.org> wrote:"));
        assert_eq!(
            html,
            concat!(
                "<p>Thanks, <strong>will do</strong>.</p>",
                "<div class=\"torro-quote\"><p>On Monday, Jonas &lt;jonas@example.org&gt; wrote:</p>",
                "<blockquote type=\"cite\"><p>Hi *Anna*,</p><p>- see [x](https://x.example)</p>",
                "<blockquote type=\"cite\"><p>older</p></blockquote></blockquote></div>",
            )
        );
        assert_eq!(sanitize(&html), html, "the renderer writes only what the cleaning keeps");
        // The signature lands above the history, in the HTML and in the plain text beside it.
        let text = to_text(&sign(&html, Some("Anna")));
        assert!(
            text.starts_with(
                "Thanks, will do.\n\n-- \nAnna\n\nOn Monday, Jonas <jonas@example.org> wrote:\n\n> Hi *Anna*,"
            ),
            "{text}"
        );
        // Without the line, all of it is own text.
        assert!(!to_html("a\n> b", Some("On Monday wrote:")).contains("torro-quote"));
    }

    #[test]
    fn the_editor_shows_marks_quietly_and_the_text_as_it_will_look() {
        let mark = Look { mark: true, ..Look::default() };
        let strong = Look { strong: true, ..Look::default() };
        assert_eq!(
            looks("a **b *c* d** e"),
            [
                (2..4, mark),
                (4..6, strong),
                (6..7, Look { mark: true, ..strong }),
                (7..8, Look { emphasis: true, ..strong }),
                (8..9, Look { mark: true, ..strong }),
                (9..11, strong),
                (11..13, mark),
            ]
        );
        let link = Look { link: true, ..Look::default() };
        assert_eq!(
            looks("- [x](https://a.example) https://b.example"),
            [(0..2, mark), (2..3, mark), (3..4, link), (4..24, mark), (25..42, link),]
        );
        assert_eq!(looks("\\*no*"), [(0..1, mark)], "an escaped backslash");
        let quote = Look { quote: true, ..Look::default() };
        assert_eq!(looks("> > **said**"), [(0..4, Look { mark: true, ..quote }), (4..12, quote)]);
        assert_eq!(looks("plain"), []);
    }
}
