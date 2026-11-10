//! Strict, offline invoice copies: bounded tokenization, a closed Paddle profile and measured PDF text.
//! No provider markup, styles, resources, links, forms or actions enter the generated PDF.

use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
use printpdf::{Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, Point, Pt, TextItem};
use sha2::{Digest, Sha256};

const MAX_HTML: usize = 2 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_TOKEN: usize = 16 * 1024;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const COPY_MARK: &str = "PDF copy generated from HTML · Not the supplier's original PDF";
const TIME: Duration = Duration::from_secs(5);
static CONVERTERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    Cancelled,
    Busy,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "invoice copy cannot be proven complete and safe",
            Self::Cancelled => "invoice conversion cancelled",
            Self::Busy => "another invoice conversion is still running",
        })
    }
}

impl std::error::Error for Error {}

struct Budget {
    start: Instant,
    cancel: Option<Arc<AtomicBool>>,
}

impl Budget {
    fn new(cancel: Option<Arc<AtomicBool>>) -> Self {
        Self { start: Instant::now(), cancel }
    }

    fn check(&self) -> Result<(), Error> {
        if self.cancel.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(Error::Cancelled);
        }
        if self.start.elapsed() > TIME {
            return Err(Error::Unsupported);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Text(String),
    Cells(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    blocks: Vec<Block>,
}

impl Invoice {
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|block| match block {
                Block::Text(text) => text.clone(),
                Block::Cells(cells) => cells.join("\t"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct Copy {
    pub bytes: Vec<u8>,
    pub pages: usize,
}

/// This digest deliberately includes the exact embedded font bytes, parser/profile and typesetter versions.
pub fn binding() -> String {
    static BINDING: OnceLock<String> = OnceLock::new();
    BINDING
        .get_or_init(|| {
            // These are immutable bytes embedded in this binary, not the person's configured font.
            // Rehashing them before every download chunk would stall otherwise bounded retrievals.
            let fonts = crate::tools::pdf::PdfFonts::standard();
            let mut hash = Sha256::new();
            hash.update(b"paddle_setapp/1;html5ever/0.39.0;bounded-token-profile/2;no-external-presentation;typesetter/1;A4/15mm/11pt");
            hash.update(&fonts.text);
            hash.update(&fonts.strong);
            hash.update(&fonts.heading);
            hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .clone()
}

/// Admission moves into the actual blocking worker, not the cancelable async waiting future.
pub async fn convert(
    bytes: Vec<u8>,
    content_type: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Copy, Error> {
    let budget = Budget::new(cancel);
    run_worker(move || {
        let invoice = extract_with(&bytes, content_type.as_deref(), &budget)?;
        render(&invoice, &budget)
    })
    .await
}

async fn run_worker<T: Send + 'static>(work: impl FnOnce() -> Result<T, Error> + Send + 'static) -> Result<T, Error> {
    let turn = CONVERTERS.try_acquire().map_err(|_| Error::Busy)?;
    tokio::task::spawn_blocking(move || {
        let _turn = turn;
        work()
    })
    .await
    .map_err(|_| Error::Unsupported)?
}

#[cfg(test)]
pub(super) async fn verify_worker_admission_after_cancellation() {
    let (started, waiting) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let (finished, done) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(run_worker(move || {
        let _ = started.send(());
        held.recv().map_err(|_| Error::Unsupported)?;
        let _ = finished.send(());
        Ok(())
    }));
    waiting.await.expect("worker started");
    task.abort();
    assert!(task.await.expect_err("waiting future cancelled").is_cancelled());
    assert!(CONVERTERS.try_acquire().is_err(), "aborting the waiting future cannot free a running converter");
    release.send(()).expect("release actual worker");
    done.await.expect("actual worker finished");
    // Await acquisition, rather than race the worker's final permit drop after its completion signal.
    drop(CONVERTERS.acquire().await.expect("permit released by actual worker"));
}

#[cfg(test)]
pub fn extract(bytes: &[u8], content_type: Option<&str>) -> Result<Invoice, Error> {
    extract_with(bytes, content_type, &Budget::new(None))
}

#[derive(Default)]
struct Node {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<usize>,
    parent: Option<usize>,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
    }

    fn class(&self, class: &str) -> bool {
        self.attr("class").is_some_and(|classes| classes.split_ascii_whitespace().any(|value| value == class))
    }
}

struct Tree {
    nodes: Vec<Node>,
    stack: Vec<usize>,
    error: bool,
    text_bytes: usize,
    encoding: Option<String>,
}

struct Sink(RefCell<Tree>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        use html5ever::tokenizer::TagKind;
        let mut tree = self.0.borrow_mut();
        if tree.error {
            return TokenSinkResult::Continue;
        }
        if tree.nodes.len() >= MAX_NODES {
            tree.error = true;
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                if tag.kind == TagKind::EndTag {
                    let last = tree.stack.last().and_then(|index| tree.nodes.get(*index));
                    if last.is_none_or(|node| node.tag != name) {
                        tree.error = true;
                    } else {
                        tree.stack.pop();
                    }
                    return TokenSinkResult::Continue;
                }
                if tag.attrs.len() > 64 || tree.stack.len() > MAX_DEPTH {
                    tree.error = true;
                    return TokenSinkResult::Continue;
                }
                let attrs: Vec<_> =
                    tag.attrs.into_iter().map(|attr| (attr.name.local.to_string(), attr.value.to_string())).collect();
                if attrs
                    .iter()
                    .enumerate()
                    .any(|(at, (key, _))| attrs[..at].iter().any(|(previous, _)| previous == key))
                {
                    tree.error = true;
                }
                if name == "meta" {
                    let direct = attrs.iter().find(|(key, _)| key == "charset").map(|(_, value)| value.clone());
                    let equiv = attrs
                        .iter()
                        .any(|(key, value)| key == "http-equiv" && value.eq_ignore_ascii_case("content-type"));
                    let content = attrs.iter().find(|(key, _)| key == "content").map(|(_, value)| value.as_str());
                    let indirect = if equiv { content.map(header_encoding).transpose() } else { Ok(None) };
                    let indirect = match indirect {
                        Ok(value) => value.flatten(),
                        Err(_) => {
                            tree.error = true;
                            return TokenSinkResult::Continue;
                        }
                    };
                    if direct.as_ref().zip(indirect.as_ref()).is_some_and(|(a, b)| !same_encoding(a, b)) {
                        tree.error = true;
                    }
                    let declared = direct.or(indirect);
                    if let Some(declared) = declared {
                        if tree.encoding.as_ref().is_some_and(|old| !same_encoding(old, &declared)) {
                            tree.error = true;
                        }
                        tree.encoding = Some(declared);
                    }
                }
                let parent = tree.stack.last().copied().unwrap_or(0);
                let index = tree.nodes.len();
                tree.nodes.push(Node { tag: name.clone(), attrs, parent: Some(parent), ..Node::default() });
                if let Some(node) = tree.nodes.get_mut(parent) {
                    node.children.push(index);
                }
                let void = matches!(
                    name.as_str(),
                    "area"
                        | "base"
                        | "br"
                        | "col"
                        | "embed"
                        | "hr"
                        | "img"
                        | "input"
                        | "link"
                        | "meta"
                        | "param"
                        | "source"
                        | "track"
                        | "wbr"
                );
                if !void && tag.self_closing {
                    tree.error = true;
                }
                if !void && !tag.self_closing {
                    tree.stack.push(index);
                }
                if matches!(name.as_str(), "script" | "style") {
                    return TokenSinkResult::RawData(if name == "script" {
                        html5ever::tokenizer::states::RawKind::ScriptData
                    } else {
                        html5ever::tokenizer::states::RawKind::Rawtext
                    });
                }
            }
            Token::CharacterTokens(text) => {
                tree.text_bytes = tree.text_bytes.saturating_add(text.len());
                if text.len() > MAX_TOKEN || tree.text_bytes > MAX_TEXT {
                    tree.error = true;
                    return TokenSinkResult::Continue;
                }
                let parent = tree.stack.last().copied().unwrap_or(0);
                let index = tree.nodes.len();
                tree.nodes.push(Node { text: text.to_string(), parent: Some(parent), ..Node::default() });
                if let Some(node) = tree.nodes.get_mut(parent) {
                    node.children.push(index);
                }
            }
            Token::NullCharacterToken | Token::ParseError(_) => tree.error = true,
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn same_encoding(a: &str, b: &str) -> bool {
    match (encoding_rs::Encoding::for_label(a.as_bytes()), encoding_rs::Encoding::for_label(b.as_bytes())) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn header_encoding(content_type: &str) -> Result<Option<String>, Error> {
    let mut declared: Option<String> = None;
    for parameter in content_type.split(';').skip(1) {
        let Some((key, value)) = parameter.trim().split_once('=') else { continue };
        if !key.trim().eq_ignore_ascii_case("charset") {
            continue;
        }
        let value = value.trim().trim_matches(['\'', '"']);
        if encoding_rs::Encoding::for_label(value.as_bytes()).is_none()
            || declared.as_ref().is_some_and(|old| !same_encoding(old, value))
        {
            return Err(Error::Unsupported);
        }
        declared = Some(value.into());
    }
    Ok(declared)
}

/// Only decoding is normalized. Unknown, inconsistent or malformed encodings never use replacement characters.
fn decode<'a>(bytes: &'a [u8], content_type: Option<&str>) -> Result<(std::borrow::Cow<'a, str>, String), Error> {
    if bytes.len() > MAX_HTML {
        return Err(Error::Unsupported);
    }
    let header = content_type.map(header_encoding).transpose()?.flatten();
    let prefix: String = bytes.iter().take(1024).map(|byte| char::from(*byte)).collect();
    let lower = prefix.to_ascii_lowercase();
    let meta = lower.find("charset=").and_then(|at| {
        let tail = lower.get(at + 8..)?.trim_start_matches(['\'', '"', ' ']);
        Some(tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>())
    });
    let bom = encoding_rs::Encoding::for_bom(bytes);
    let declared = header
        .or_else(|| meta.clone())
        .or_else(|| bom.map(|(encoding, _)| encoding.name().into()))
        .ok_or(Error::Unsupported)?;
    let encoding = encoding_rs::Encoding::for_label(declared.as_bytes()).ok_or(Error::Unsupported)?;
    if meta.as_ref().is_some_and(|meta| !same_encoding(&declared, meta))
        || bom.is_some_and(|(other, _)| other != encoding)
    {
        return Err(Error::Unsupported);
    }
    let bytes = match bom {
        Some((_, length)) => bytes.get(length..).ok_or(Error::Unsupported)?,
        None => bytes,
    };
    let decoded = encoding.decode_without_bom_handling_and_without_replacement(bytes).ok_or(Error::Unsupported)?;
    if decoded.len() > MAX_HTML {
        return Err(Error::Unsupported);
    }
    Ok((decoded, encoding.name().into()))
}

/// Incremental lexical preflight bounds incomplete tags/attributes before the tokenizer buffers them.
fn preflight(html: &str, budget: &Budget) -> Result<(), Error> {
    let mut in_tag = false;
    let mut comment = false;
    let mut quote = None;
    let mut length = 0usize;
    for (at, byte) in html.bytes().enumerate() {
        if at % 8192 == 0 {
            budget.check()?;
        }
        if comment {
            length += 1;
            if length > MAX_TOKEN {
                return Err(Error::Unsupported);
            }
            if html.as_bytes().get(at.saturating_sub(2)..=at) == Some(b"-->") {
                comment = false;
            }
            continue;
        }
        if !in_tag && html.as_bytes().get(at..).is_some_and(|tail| tail.starts_with(b"<!--")) {
            comment = true;
            length = 1;
            continue;
        }
        if !in_tag && byte == b'<' {
            in_tag = true;
            length = 0;
        }
        if in_tag {
            length += 1;
            if length > MAX_TOKEN {
                return Err(Error::Unsupported);
            }
            match (quote, byte) {
                (Some(wanted), found) if wanted == found => quote = None,
                (None, b'\'' | b'"') => quote = Some(byte),
                (None, b'>') => in_tag = false,
                _ => {}
            }
        }
    }
    if in_tag || comment {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn extract_with(bytes: &[u8], kind: Option<&str>, budget: &Budget) -> Result<Invoice, Error> {
    let (html, encoding) = decode(bytes, kind)?;
    preflight(&html, budget)?;
    let sink = Sink(RefCell::new(Tree {
        nodes: vec![Node::default()],
        stack: vec![0],
        error: false,
        text_bytes: 0,
        encoding: Some(encoding),
    }));
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let queue = BufferQueue::default();
    let mut at = 0;
    while at < html.len() {
        budget.check()?;
        let mut end = (at + 8192).min(html.len());
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        queue.push_back(StrTendril::from_slice(html.get(at..end).ok_or(Error::Unsupported)?));
        let _ = tokenizer.feed(&queue);
        if tokenizer.sink.0.borrow().error {
            return Err(Error::Unsupported);
        }
        at = end;
    }
    tokenizer.end();
    let tree = tokenizer.sink.0.into_inner();
    if tree.error || tree.stack != [0] {
        return Err(Error::Unsupported);
    }
    let roots: Vec<_> =
        tree.nodes.iter().enumerate().filter(|(_, node)| node.class("c-sheet")).map(|(index, _)| index).collect();
    let [root] = roots.as_slice() else { return Err(Error::Unsupported) };
    presentation(&tree, *root)?;
    let mut invoice = Invoice { blocks: Vec::new() };
    collect(&tree, *root, &mut invoice.blocks, budget)?;
    let headings = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| {
            node.tag == "h1"
                && beneath(&tree, *index, *root)
                && plain(&tree, *index).is_ok_and(|s| s.starts_with("Invoice"))
        })
        .count();
    let tables = tree.nodes.iter().filter(|node| node.class("receipt-table")).count();
    let totals = tree.nodes.iter().filter(|node| node.class("receipt-table-total")).count();
    if headings != 1 || tables != 1 || totals != 1 || invoice.blocks.is_empty() {
        return Err(Error::Unsupported);
    }
    // No unaccounted visible text outside the invoice. Only witnessed navigation/editor/metadata is excluded.
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.tag.is_empty()
            && !normalize(&node.text).is_empty()
            && !beneath(&tree, index, *root)
            && !known_outside(&tree, index)
        {
            return Err(Error::Unsupported);
        }
    }
    Ok(invoice)
}

fn beneath(tree: &Tree, mut node: usize, root: usize) -> bool {
    for _ in 0..=MAX_DEPTH {
        if node == root {
            return true;
        }
        let Some(parent) = tree.nodes.get(node).and_then(|node| node.parent) else { return false };
        node = parent;
    }
    false
}

fn known_outside(tree: &Tree, mut node: usize) -> bool {
    for _ in 0..=MAX_DEPTH {
        let Some(held) = tree.nodes.get(node) else { return false };
        if held.tag == "head"
            || held.attr("id") == Some("customer-information-modal")
            || (held.tag == "header" && held.class("c-header") && held.class("no-print"))
        {
            return true;
        }
        let Some(parent) = held.parent else { return false };
        node = parent;
    }
    false
}

/// No external or executable presentation influences are proven by profile 1. A mutable stylesheet
/// URL is not a version pin: even a familiar provider URL can add generated invoice facts tomorrow.
/// Refuse rather than compare a PDF against an extraction that has already lost those facts.
fn presentation(tree: &Tree, root: usize) -> Result<(), Error> {
    let heads: Vec<_> =
        tree.nodes.iter().enumerate().filter(|(_, node)| node.tag == "head").map(|(index, _)| index).collect();
    if heads.len() > 1 {
        return Err(Error::Unsupported);
    }
    for (index, node) in tree.nodes.iter().enumerate() {
        if matches!(node.tag.as_str(), "script" | "style" | "link" | "base" | "iframe" | "object" | "embed")
            || node.attrs.iter().any(|(key, _)| key.starts_with("on"))
        {
            return Err(Error::Unsupported);
        }
        if node.tag == "meta"
            && !(node.attrs.len() == 1 && node.attr("charset").is_some_and(|value| same_encoding(value, "utf-8")))
        {
            return Err(Error::Unsupported);
        }
        if matches!(node.tag.as_str(), "head" | "title") && !node.attrs.is_empty() {
            return Err(Error::Unsupported);
        }
        // The invoice's ancestors can affect it too; no hidden, inline-style, custom element or
        // unsupported layout wrapper may bypass safe_node merely by sitting outside c-sheet.
        if index != 0
            && beneath(tree, root, index)
            && index != root
            && (!matches!(node.tag.as_str(), "html" | "body" | "main" | "div")
                || node.attrs.iter().any(|(key, value)| match key.as_str() {
                    "lang" => node.tag != "html" || !matches!(value.as_str(), "en" | "en-US" | "en-GB"),
                    "class" => value.split_ascii_whitespace().any(|class| !matches!(class, "c-main" | "c-container")),
                    _ => true,
                }))
        {
            return Err(Error::Unsupported);
        }
        // The head is metadata only, not a blanket exception for arbitrary DOM or form values.
        if heads.first().is_some_and(|head| beneath(tree, index, *head))
            && !matches!(node.tag.as_str(), "head" | "title" | "meta" | "")
        {
            return Err(Error::Unsupported);
        }
        if !beneath(tree, index, root)
            && !known_outside(tree, index)
            && matches!(
                node.tag.as_str(),
                "img"
                    | "svg"
                    | "canvas"
                    | "video"
                    | "audio"
                    | "form"
                    | "input"
                    | "select"
                    | "textarea"
                    | "template"
                    | "noscript"
            )
        {
            return Err(Error::Unsupported);
        }
    }
    Ok(())
}

fn normalize(text: &str) -> String {
    text.split([' ', '\t', '\r', '\n', '\u{c}']).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn safe_node(node: &Node) -> Result<(), Error> {
    // These are the witnessed semantic/layout classes of profile 1, not an open-ended CSS interpreter.
    const CLASSES: &[&str] = &[
        "c-sheet",
        "vendor-logo",
        "header-link",
        "g-container",
        "g-row",
        "g-col",
        "g-col-sm-6",
        "g-col-xs-12",
        "g-col-6",
        "end-sm",
        "u-tac",
        "u-tar",
        "c-heading",
        "c-heading--light",
        "c-heading--size-2",
        "c-heading--size-3",
        "c-heading--size-5",
        "c-heading--size-6",
        "c-badge",
        "c-badge--success",
        "c-paragraph",
        "c-paragraph--small",
        "paragraph-receipt-footer",
        "receipt-no-plan-name",
        "receipt-table",
        "receipt-table-head",
        "receipt-table-head-product",
        "receipt-table-head-quantity",
        "receipt-table-head-price",
        "receipt-table-row",
        "receipt-table-row-product",
        "receipt-table-row-quantity",
        "receipt-table-row-price",
        "receipt-table-tax-total",
        "receipt-table-tax",
        "receipt-table-tax-left",
        "receipt-table-tax-right",
        "receipt-table-total",
        "receipt-table-total-left",
        "receipt-table-total-right",
        "receipt-table-total-right-tax-message",
    ];
    if node.attr("class").is_some_and(|classes| classes.split_ascii_whitespace().any(|class| !CLASSES.contains(&class)))
        || node.attrs.iter().any(|(key, _)| !matches!(key.as_str(), "class" | "id" | "style" | "href" | "target"))
    {
        return Err(Error::Unsupported);
    }
    if matches!(
        node.tag.as_str(),
        "script"
            | "style"
            | "form"
            | "input"
            | "select"
            | "textarea"
            | "button"
            | "img"
            | "svg"
            | "iframe"
            | "object"
            | "embed"
            | "canvas"
            | "template"
            | "noscript"
            | "video"
            | "audio"
    ) || node.attr("hidden").is_some()
        || node.attr("aria-hidden").is_some()
        || node.attrs.iter().any(|(key, _)| key.starts_with("on"))
        || node.attr("class").is_some_and(|value| {
            value.split_ascii_whitespace().any(|class| matches!(class, "hidden" | "hide" | "no-print"))
        })
    {
        return Err(Error::Unsupported);
    }
    if let Some(style) = node.attr("style") {
        for declaration in style.split(';').map(str::trim).filter(|declaration| !declaration.is_empty()) {
            let Some((key, value)) = declaration.split_once(':') else { return Err(Error::Unsupported) };
            if !matches!(
                (key.trim(), value.trim()),
                ("font-weight", "400") | ("margin-top", "0" | "3rem") | ("margin", "30px 0 36px")
            ) {
                return Err(Error::Unsupported);
            }
        }
    }
    if node.text.chars().any(|c| {
        !(matches!(c, '\n' | '\r' | '\t')
            || (' '..='\u{24f}').contains(&c)
            || ('\u{2000}'..='\u{2027}').contains(&c)
            || matches!(c, '€' | '™'))
            || matches!(c, '\u{7f}'..='\u{9f}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}')
    }) {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn raw_text(tree: &Tree, index: usize, out: &mut String) -> Result<(), Error> {
    let node = tree.nodes.get(index).ok_or(Error::Unsupported)?;
    safe_node(node)?;
    if !node.tag.is_empty()
        && !matches!(
            node.tag.as_str(),
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "span" | "strong" | "em" | "b" | "a" | "br"
        )
    {
        return Err(Error::Unsupported);
    }
    let block = matches!(node.tag.as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    if block && !out.is_empty() {
        out.push('\n');
    }
    if node.tag == "br" {
        out.push('\n');
    }
    out.push_str(&node.text);
    for child in &node.children {
        raw_text(tree, *child, out)?;
    }
    if block {
        out.push('\n');
    }
    Ok(())
}

fn plain(tree: &Tree, index: usize) -> Result<String, Error> {
    let mut text = String::new();
    raw_text(tree, index, &mut text)?;
    Ok(normalize(&text))
}

fn collect(tree: &Tree, index: usize, blocks: &mut Vec<Block>, budget: &Budget) -> Result<(), Error> {
    budget.check()?;
    let node = tree.nodes.get(index).ok_or(Error::Unsupported)?;
    // This one witnessed control contains no invoice facts; arbitrary no-print blocks are refused.
    if node.tag == "p" && node.attr("id") == Some("add-edit-address-container") && node.class("no-print") {
        if node
            .children
            .iter()
            .all(|child| tree.nodes.get(*child).is_some_and(|child| child.tag.is_empty() || child.tag == "a"))
        {
            return Ok(());
        }
        return Err(Error::Unsupported);
    }
    safe_node(node)?;
    if ["receipt-table-head", "receipt-table-row", "receipt-table-tax", "receipt-table-total"]
        .iter()
        .any(|class| node.class(class))
    {
        if node.children.iter().any(|child| {
            tree.nodes.get(*child).is_some_and(|child| child.tag.is_empty() && !normalize(&child.text).is_empty())
        }) {
            return Err(Error::Unsupported);
        }
        let cells: Vec<_> = node
            .children
            .iter()
            .filter(|child| tree.nodes.get(**child).is_some_and(|n| !n.tag.is_empty()))
            .copied()
            .collect();
        if !(2..=3).contains(&cells.len()) {
            return Err(Error::Unsupported);
        }
        let cells: Vec<_> = cells.into_iter().map(|cell| plain(tree, cell)).collect::<Result<_, _>>()?;
        blocks.push(Block::Cells(cells));
        return Ok(());
    }
    if matches!(node.tag.as_str(), "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        let mut text = String::new();
        raw_text(tree, index, &mut text)?;
        for line in text.split('\n').map(normalize).filter(|line| !line.is_empty()) {
            blocks.push(Block::Text(line));
        }
        return Ok(());
    }
    if node.tag.is_empty() {
        let text = normalize(&node.text);
        if !text.is_empty() {
            blocks.push(Block::Text(text));
        }
        return Ok(());
    }
    if !matches!(node.tag.as_str(), "div" | "section" | "span" | "strong" | "em" | "b" | "a" | "br" | "hr") {
        return Err(Error::Unsupported);
    }
    for child in &node.children {
        collect(tree, *child, blocks, budget)?;
    }
    Ok(())
}

fn width(font: &ParsedFont, text: &str) -> Result<f32, Error> {
    let mut total = 0usize;
    for c in text.chars() {
        let glyph = font.lookup_glyph_index(u32::from(c)).filter(|gid| *gid != 0).ok_or(Error::Unsupported)?;
        total = total
            .checked_add(font.get_glyph_width_internal(glyph).ok_or(Error::Unsupported)?)
            .ok_or(Error::Unsupported)?;
    }
    Ok(total as f32 / f32::from(font.font_metrics.units_per_em.max(1)) * 11.0 * 25.4 / 72.0)
}

fn wrap(font: &ParsedFont, text: &str, room: f32, budget: &Budget) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        budget.check()?;
        if width(font, word)? > room {
            return Err(Error::Unsupported);
        }
        let candidate = if line.is_empty() { word.into() } else { format!("{line} {word}") };
        if width(font, &candidate)? > room && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line = word.into();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    Ok(lines)
}

fn text_ops(font: &printpdf::FontId, x: f32, y: f32, text: String) -> Vec<Op> {
    vec![
        Op::StartTextSection,
        Op::SetTextCursor { pos: Point::new(Mm(x), Mm(y)) },
        Op::SetFont { font: PdfFontHandle::External(font.clone()), size: Pt(11.0) },
        Op::ShowText { items: vec![TextItem::Text(text)] },
        Op::EndTextSection,
    ]
}

fn render(invoice: &Invoice, budget: &Budget) -> Result<Copy, Error> {
    budget.check()?;
    let fonts = crate::tools::pdf::PdfFonts::standard();
    let font = ParsedFont::from_bytes(&fonts.text, 0, &mut Vec::new()).ok_or(Error::Unsupported)?;
    let mut doc = PdfDocument::new("Invoice copy generated from HTML");
    let id = doc.add_font(&font);
    let mut pages = Vec::new();
    let mut ops = text_ops(&id, 15.0, 282.0, COPY_MARK.into());
    let mut y = 271.0;
    for block in &invoice.blocks {
        budget.check()?;
        let cells = match block {
            Block::Text(text) => vec![text.as_str()],
            Block::Cells(cells) => cells.iter().map(String::as_str).collect(),
        };
        let widths = match cells.len() {
            1 => vec![180.0],
            2 => vec![99.0, 81.0],
            3 => vec![99.0, 27.0, 54.0],
            _ => return Err(Error::Unsupported),
        };
        let wrapped: Vec<_> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, room)| wrap(&font, cell, room - 3.0, budget))
            .collect::<Result<_, _>>()?;
        let lines = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let height = lines as f32 * 5.2 + 2.0;
        if height > 250.0 {
            return Err(Error::Unsupported);
        }
        if y - height < 15.0 {
            pages.push(PdfPage::new(Mm(210.0), Mm(297.0), std::mem::take(&mut ops)));
            if pages.len() >= 50 {
                return Err(Error::Unsupported);
            }
            ops = text_ops(&id, 15.0, 282.0, COPY_MARK.into());
            y = 271.0;
        }
        let mut x = 15.0;
        for (cell, room) in wrapped.iter().zip(widths) {
            for (line, text) in cell.iter().enumerate() {
                budget.check()?;
                ops.extend(text_ops(&id, x, y - line as f32 * 5.2, text.clone()));
            }
            x += room;
        }
        y -= height;
    }
    pages.push(PdfPage::new(Mm(210.0), Mm(297.0), ops));
    let count = pages.len();
    doc.pages = pages;
    let bytes = doc.save(&printpdf::PdfSaveOptions::default(), &mut Vec::new());
    budget.check()?;
    if bytes.len() > 50 * 1024 * 1024 {
        return Err(Error::Unsupported);
    }
    // Parse our own bounded output, never an untrusted downloaded PDF here. No text may disappear in serialization.
    let parsed = PdfDocument::parse(&bytes, &printpdf::PdfParseOptions { fail_on_error: true }, &mut Vec::new())
        .map_err(|_| Error::Unsupported)?;
    let texts = parsed.extract_text();
    if texts.len() != count {
        return Err(Error::Unsupported);
    }
    let mut actual = Vec::new();
    for page in texts {
        budget.check()?;
        let text = page.join("\n");
        let Some(body) = text.strip_prefix(COPY_MARK) else { return Err(Error::Unsupported) };
        actual.push(body.to_string());
    }
    let expected = invoice.text().replace('\t', " ");
    if normalize(&actual.join("\n")) != normalize(&expected) {
        return Err(Error::Unsupported);
    }
    Ok(Copy { bytes, pages: count })
}
