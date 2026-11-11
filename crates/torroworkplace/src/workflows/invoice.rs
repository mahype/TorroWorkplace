//! Strict, offline invoice copies: bounded tokenization, a closed Paddle profile and measured PDF text.
//! No provider markup, styles, resources, links, forms or actions enter the generated PDF. Profile 2 also reads
//! the page's stylesheet as data and refuses it when it could generate or hide invoice content.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
use printpdf::{Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage, Point, Pt, TextItem};
use sha2::{Digest, Sha256};

use super::download::ResourceKind;

const MAX_HTML: usize = 2 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_TOKEN: usize = 16 * 1024;
const MAX_NODES: usize = 50_000;
const MAX_DEPTH: usize = 64;
const COPY_MARK: &str = "PDF copy generated from HTML · Not the supplier's original PDF";
const TIME: Duration = Duration::from_secs(5);
const MAX_CSS: usize = 512 * 1024;
const MAX_CSS_RULES: usize = 20_000;
const MAX_CSS_DEPTH: usize = 16;
/// The only witnessed selectors whose hiding removes navigation and editing controls, never invoice facts.
const HIDE_ALLOWED: &[&str] = &[".no-print", ".c-header"];
static CONVERTERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
/// Tests that convert hold this turn, so the single global converter never reports `Busy` between parallel tests.
#[cfg(test)]
pub(super) static TEST_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
/// Each profile has its own prefix, so a package bound to one profile never passes as the other.
pub fn binding(profile: u16) -> String {
    static FIRST: OnceLock<String> = OnceLock::new();
    static SECOND: OnceLock<String> = OnceLock::new();
    let (cell, prefix): (_, &[u8]) = match profile {
        1 => (
            &FIRST,
            b"paddle_setapp/1;html5ever/0.39.0;bounded-token-profile/2;no-external-presentation;typesetter/1;A4/15mm/11pt",
        ),
        _ => (
            &SECOND,
            b"paddle_setapp/2;html5ever/0.39.0;bounded-token-profile/2;cssparser/0.37.0;css-rules/1;\
              hide-only:.no-print,.c-header;pinned-scripts-never-executed;typesetter/1;A4/15mm/11pt",
        ),
    };
    cell.get_or_init(|| {
        // These are immutable bytes embedded in this binary, not the person's configured font.
        // Rehashing them before every download chunk would stall otherwise bounded retrievals.
        let fonts = crate::tools::pdf::PdfFonts::standard();
        let mut hash = Sha256::new();
        hash.update(prefix);
        hash.update(&fonts.text);
        hash.update(&fonts.strong);
        hash.update(&fonts.heading);
        hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    })
    .clone()
}

/// A stylesheet or script the page's head names. Only profile 2 accepts any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub kind: ResourceKind,
    pub href: String,
}

/// What profile 2 found in the page besides its text: resources to load, inline scripts to pin and inline
/// styles and page facts for the stylesheet check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presentation {
    pub references: Vec<Reference>,
    /// SHA-256 of each inline script's text, in page order. Never executed.
    pub inline_scripts: Vec<String>,
    inline_styles: Vec<String>,
    facts: PageFacts,
}

impl Presentation {
    /// Inline `<style>` elements and the `style` attributes outside the invoice, through the same allow list.
    fn check_inline(&self, budget: &Budget) -> Result<(), Error> {
        for style in &self.inline_styles {
            check_css(style.as_bytes(), budget, &self.facts)?;
        }
        Ok(())
    }
}

/// A loaded reference. Stylesheets carry their bytes for the rule check; scripts are only named, never run.
pub struct Loaded {
    pub reference: Reference,
    pub stylesheet: Option<Vec<u8>>,
}

/// Lists the head resources of a profile-2 page after the same complete structural check as the conversion,
/// so nothing is loaded for a page that would be refused anyway.
pub async fn references(
    bytes: Vec<u8>,
    content_type: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Presentation, Error> {
    let budget = Budget::new(cancel);
    run_worker(move || {
        let (_, presentation) = extract_with(&bytes, content_type.as_deref(), &budget, 2)?;
        // Inline styles are checked before anything is loaded; a refused page needs no resources.
        presentation.check_inline(&budget)?;
        Ok(presentation)
    })
    .await
}

/// Admission moves into the actual blocking worker, not the cancelable async waiting future.
/// `loaded` is `None` for profile 1 and the page's checked resources, in page order, for profile 2.
pub async fn convert(
    bytes: Vec<u8>,
    content_type: Option<String>,
    loaded: Option<Vec<Loaded>>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Copy, Error> {
    let budget = Budget::new(cancel);
    run_worker(move || {
        let profile = if loaded.is_some() { 2 } else { 1 };
        let (invoice, presentation) = extract_with(&bytes, content_type.as_deref(), &budget, profile)?;
        let loaded = loaded.unwrap_or_default();
        let references = &presentation.references;
        // The PDF may only follow the exact resources that were loaded and checked for this page.
        if references.len() != loaded.len()
            || references.iter().zip(&loaded).any(|(reference, loaded)| *reference != loaded.reference)
        {
            return Err(Error::Unsupported);
        }
        presentation.check_inline(&budget)?;
        for loaded in &loaded {
            match (loaded.reference.kind, &loaded.stylesheet) {
                (ResourceKind::Stylesheet, Some(css)) => check_css(css, &budget, &presentation.facts)?,
                (ResourceKind::Script, None) => {}
                _ => return Err(Error::Unsupported),
            }
        }
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
    extract_with(bytes, content_type, &Budget::new(None), 1).map(|(invoice, _)| invoice)
}

#[cfg(test)]
pub fn extract_profile_2(bytes: &[u8], content_type: Option<&str>) -> Result<(Invoice, Presentation), Error> {
    extract_with(bytes, content_type, &Budget::new(None), 2)
}

/// Checks a stylesheet against the facts of the given page, as the conversion does.
#[cfg(test)]
pub fn check_stylesheet(css: &[u8], page: &[u8]) -> Result<(), Error> {
    let (_, presentation) = extract_with(page, Some("text/html; charset=utf-8"), &Budget::new(None), 2)?;
    check_css(css, &Budget::new(None), &presentation.facts)
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

fn extract_with(
    bytes: &[u8],
    kind: Option<&str>,
    budget: &Budget,
    profile: u16,
) -> Result<(Invoice, Presentation), Error> {
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
    let presentation = presentation(&tree, *root, profile)?;
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
    // No unaccounted visible text outside the invoice. Only witnessed navigation/editor/metadata is excluded,
    // and the text of the inline styles and scripts profile 2 has accepted, which is code, not visible text.
    for (index, node) in tree.nodes.iter().enumerate() {
        if node.tag.is_empty()
            && !normalize(&node.text).is_empty()
            && !beneath(&tree, index, *root)
            && !known_outside(&tree, index)
            && !(profile == 2 && code_text(&tree, node))
        {
            return Err(Error::Unsupported);
        }
    }
    Ok((invoice, presentation))
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
            // The provider's logo link below the invoice: an image without text.
            || (held.tag == "a" && held.class("logo") && held.attrs.iter().all(|(key, _)| matches!(key.as_str(), "class" | "href" | "target")))
        {
            return true;
        }
        let Some(parent) = held.parent else { return false };
        node = parent;
    }
    false
}

/// The text of a `<script>` or `<style>` element.
fn code_text(tree: &Tree, node: &Node) -> bool {
    node.parent
        .and_then(|parent| tree.nodes.get(parent))
        .is_some_and(|parent| matches!(parent.tag.as_str(), "script" | "style"))
}

/// The concatenated text children of an element; any child element refuses it.
fn element_text(tree: &Tree, node: &Node) -> Result<String, Error> {
    let mut text = String::new();
    for child in &node.children {
        let child = tree.nodes.get(*child).ok_or(Error::Unsupported)?;
        if !child.tag.is_empty() {
            return Err(Error::Unsupported);
        }
        text.push_str(&child.text);
    }
    Ok(text)
}

/// Accepts one witnessed profile-2 element of the page's code and metadata, or says it is not one.
/// Head: `<link rel=stylesheet|icon>`, `<script src>`, `<style>`, `<meta name=robots>`. Anywhere outside the
/// invoice: an inline `<script>`. Module scripts are refused: their imports would be further, unpinned code.
fn profile_two(
    tree: &Tree,
    node: &Node,
    in_head: bool,
    outside: bool,
    found: &mut Presentation,
) -> Result<bool, Error> {
    let only = |keys: &[&str]| node.attrs.iter().all(|(key, _)| keys.contains(&key.as_str()));
    let typed = |allowed: &[&str]| node.attr("type").is_none_or(|kind| allowed.contains(&kind.trim()));
    let href = || {
        node.attr("href")
            .filter(|href| !href.is_empty() && href.len() <= 2048)
            .map(String::from)
            .ok_or(Error::Unsupported)
    };
    let scripts = ["text/javascript", "application/javascript"];
    match node.tag.as_str() {
        "link"
            if in_head
                && only(&["rel", "href", "type"])
                && typed(&["text/css"])
                && node.attr("rel").is_some_and(|rel| rel.trim().eq_ignore_ascii_case("stylesheet")) =>
        {
            found.references.push(Reference { kind: ResourceKind::Stylesheet, href: href()? });
        }
        // The page's icon is never loaded and shows no invoice content.
        "link"
            if in_head
                && only(&["rel", "href", "type", "sizes"])
                && node.attr("rel").is_some_and(|rel| rel.trim().eq_ignore_ascii_case("icon")) =>
        {
            href()?;
        }
        "meta"
            if in_head
                && only(&["name", "content"])
                && node.attr("name") == Some("robots")
                && node.attr("content").is_some_and(|content| content.len() <= 256) => {}
        "style" if in_head && only(&["type"]) && typed(&["text/css"]) => {
            found.inline_styles.push(element_text(tree, node)?);
        }
        "script"
            if node.attr("src").is_some() && in_head && only(&["src", "defer", "async", "type"]) && typed(&scripts) =>
        {
            if !normalize(&element_text(tree, node)?).is_empty() {
                return Err(Error::Unsupported);
            }
            found.references.push(Reference {
                kind: ResourceKind::Script,
                href: node
                    .attr("src")
                    .filter(|src| !src.is_empty() && src.len() <= 2048)
                    .ok_or(Error::Unsupported)?
                    .into(),
            });
        }
        "script" if node.attr("src").is_none() && outside && only(&["type"]) && typed(&scripts) => {
            let text = element_text(tree, node)?;
            found
                .inline_scripts
                .push(Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect());
        }
        _ => return Ok(false),
    }
    if found.references.len() > 8 || found.inline_scripts.len() > 4 || found.inline_styles.len() > 32 {
        return Err(Error::Unsupported);
    }
    Ok(true)
}

/// No external or executable presentation influences are proven by profile 1. A mutable stylesheet
/// URL is not a version pin: even a familiar provider URL can add generated invoice facts tomorrow.
/// Refuse rather than compare a PDF against an extraction that has already lost those facts.
/// Profile 2 returns the page's resources, inline scripts and styles instead; the caller loads, checks and
/// pins each one.
fn presentation(tree: &Tree, root: usize, profile: u16) -> Result<Presentation, Error> {
    let heads: Vec<_> =
        tree.nodes.iter().enumerate().filter(|(_, node)| node.tag == "head").map(|(index, _)| index).collect();
    if heads.len() > 1 {
        return Err(Error::Unsupported);
    }
    let mut found = Presentation::default();
    for (index, node) in tree.nodes.iter().enumerate() {
        let in_head = heads.first().is_some_and(|head| beneath(tree, index, *head));
        if profile == 2 {
            if node.attrs.iter().any(|(key, _)| key.starts_with("on")) {
                return Err(Error::Unsupported);
            }
            // Elsewhere outside the invoice, a `style` attribute could cover it; it gets the stylesheet rules as a
            // rule for every element. Inside, safe_node and the ancestor check already hold it.
            if !in_head
                && !beneath(tree, index, root)
                && !beneath(tree, root, index)
                && let Some(style) = node.attr("style")
            {
                if style.contains(['{', '}', '<', '\\']) || style.contains("/*") || found.inline_styles.len() >= 32 {
                    return Err(Error::Unsupported);
                }
                found.inline_styles.push(format!("* {{{style}}}"));
            }
            if profile_two(tree, node, in_head, !beneath(tree, index, root), &mut found)? {
                continue;
            }
            // Text of an accepted script or style element; profile_two refused any other content.
            if node.tag.is_empty() && code_text(tree, node) {
                continue;
            }
        }
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
        if in_head && !matches!(node.tag.as_str(), "head" | "title" | "meta" | "") {
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
    if profile == 2 {
        found.facts = PageFacts::of(tree);
    }
    Ok(found)
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

/// Classes, ids and tags present anywhere in the page. A selector naming anything else matches nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageFacts {
    classes: BTreeSet<String>,
    ids: BTreeSet<String>,
    tags: BTreeSet<String>,
}

impl PageFacts {
    fn of(tree: &Tree) -> Self {
        let mut facts = Self::default();
        // A browser adds these even when the markup omits them. Names compare without case, as in quirks mode.
        facts.tags.extend(["html", "head", "body", "tbody"].map(String::from));
        for node in tree.nodes.iter().filter(|node| !node.tag.is_empty()) {
            facts.tags.insert(node.tag.to_ascii_lowercase());
            if let Some(id) = node.attr("id") {
                facts.ids.insert(id.to_ascii_lowercase());
            }
            for class in node.attr("class").unwrap_or("").split_ascii_whitespace() {
                facts.classes.insert(class.to_ascii_lowercase());
            }
        }
        facts
    }
}

/// Reads a stylesheet as data under a fixed allow list. A rule whose selectors name a class, id or tag the page
/// does not have cannot change it and is skipped. Every other rule may only use known properties with values
/// that neither generate nor hide text; hiding is allowed only beneath the witnessed navigation selectors.
/// Anything unknown refuses the copy instead of passing silently.
fn check_css(bytes: &[u8], budget: &Budget, facts: &PageFacts) -> Result<(), Error> {
    if bytes.len() > MAX_CSS {
        return Err(Error::Unsupported);
    }
    let text =
        std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)).map_err(|_| Error::Unsupported)?;
    let mut input = cssparser::ParserInput::new(text);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut walk = CssWalk { budget, facts, tokens: 0, rules: 0 };
    rule_list(&mut parser, &mut walk, 0).map_err(|error| match error.kind {
        cssparser::ParseErrorKind::Custom(error) => error,
        cssparser::ParseErrorKind::Basic(_) => Error::Unsupported,
    })
}

struct CssWalk<'b> {
    budget: &'b Budget,
    facts: &'b PageFacts,
    tokens: usize,
    rules: usize,
}

type CssFail<'i> = cssparser::ParseError<'i, Error>;

fn refuse<'i>() -> CssFail<'i> {
    cssparser::ParseError {
        kind: cssparser::ParseErrorKind::Custom(Error::Unsupported),
        location: cssparser::SourceLocation { line: 0, column: 0 },
    }
}

impl CssWalk<'_> {
    fn tick<'i>(&mut self) -> Result<(), CssFail<'i>> {
        self.tokens += 1;
        if self.tokens.is_multiple_of(1000) {
            self.budget.check().map_err(|error| cssparser::ParseError {
                kind: cssparser::ParseErrorKind::Custom(error),
                location: cssparser::SourceLocation { line: 0, column: 0 },
            })?;
        }
        Ok(())
    }

    fn rule<'i>(&mut self) -> Result<(), CssFail<'i>> {
        self.rules += 1;
        if self.rules > MAX_CSS_RULES { Err(refuse()) } else { Ok(()) }
    }
}

/// A simplified declaration value: enough to check it against the allow list, never to apply styles.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Word(String),
    Text(String),
    Number(f32),
    Length(f32, String),
    Function(String, Vec<Value>),
    Hex(String),
    Url(String),
    Slash,
    Other,
}

/// Rules at the top level or inside a grouping at-rule.
fn rule_list<'i>(
    parser: &mut cssparser::Parser<'i, '_>,
    walk: &mut CssWalk<'_>,
    depth: usize,
) -> Result<(), CssFail<'i>> {
    use cssparser::Token;
    if depth > MAX_CSS_DEPTH {
        return Err(refuse());
    }
    loop {
        walk.tick()?;
        let start = parser.position();
        let token = match parser.next() {
            Ok(token) => token.clone(),
            Err(_) => return Ok(()),
        };
        match token {
            Token::CDO | Token::CDC => {}
            Token::AtKeyword(name) => at_rule(parser, walk, &name.to_ascii_lowercase(), depth)?,
            Token::BadUrl(_) | Token::BadString(_) | Token::Semicolon => return Err(refuse()),
            _ => {
                walk.rule()?;
                let mut opened = matches!(token, Token::CurlyBracketBlock);
                while !opened {
                    walk.tick()?;
                    match parser.next().map_err(|_| refuse())? {
                        Token::CurlyBracketBlock => opened = true,
                        Token::Semicolon | Token::BadUrl(_) | Token::BadString(_) => return Err(refuse()),
                        _ => {}
                    }
                }
                let prelude = parser.slice_from(start);
                let selectors = selector_list(prelude.strip_suffix('{').unwrap_or(prelude));
                // An unmatchable rule is skipped as a whole; the parser steps over its block.
                if !selectors.iter().any(|selector| can_match(selector, walk.facts)) {
                    skip_block(parser, walk)?;
                } else {
                    let may_hide = selectors.iter().all(|selector| beneath_navigation(selector));
                    parser.parse_nested_block(|block| declarations(block, walk, may_hide))?;
                }
            }
        }
    }
}

/// Splits a selector list at top-level commas. Comments are removed and brackets counted only outside strings,
/// so a quoted `/*`, `,` or `]` cannot hide a selector from the check.
fn selector_list(prelude: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let (mut depth, mut quote, mut chars) = (0usize, None::<char>, prelude.chars().peekable());
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
                continue;
            }
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '/') if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for next in chars.by_ref() {
                    if last == '*' && next == '/' {
                        break;
                    }
                    last = next;
                }
                current.push(' ');
                continue;
            }
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                parts.push(normalize(&std::mem::take(&mut current)));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    parts.push(normalize(&current));
    parts
}

/// The selector without the contents of `(…)`, `[…]` and strings in them, or `None` when it uses a form the
/// check does not read: escapes, namespaces (`*|p`) or a string outside brackets.
fn plain_selector(selector: &str) -> Option<String> {
    let mut plain = String::new();
    let (mut depth, mut quote) = (0usize, None::<char>);
    for c in selector.chars() {
        match (quote, c) {
            (_, '\\') => return None,
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') if depth > 0 => quote = Some(c),
            (None, '"' | '\'' | '|') if depth == 0 => return None,
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth = depth.saturating_sub(1),
            (None, _) if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    Some(plain)
}

/// Conservative: false only when a compound outside functional pseudo-classes names a class, id or tag the page
/// lacks. Anything the check does not read counts as a possible match.
fn can_match(selector: &str, facts: &PageFacts) -> bool {
    let Some(plain) = plain_selector(selector) else { return true };
    let plain = plain.replace(['>', '+', '~'], " ");
    for compound in plain.split_ascii_whitespace() {
        let mut rest = compound;
        let tag_end = rest.find(['.', '#', ':']).unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        if !tag.is_empty() && tag != "*" && tag != "&" && !facts.tags.contains(&tag.to_ascii_lowercase()) {
            return false;
        }
        rest = &rest[tag_end..];
        while let Some(marker) = rest.chars().next() {
            let tail = &rest[1..];
            let end = tail.find(['.', '#', ':']).unwrap_or(tail.len());
            let name = tail[..end].to_ascii_lowercase();
            let known = match marker {
                '.' => facts.classes.contains(&name),
                '#' => facts.ids.contains(&name),
                _ => true,
            };
            if !known {
                return false;
            }
            // A pseudo-element marker `::x` leaves an empty name before the second colon.
            rest = &tail[end..];
        }
    }
    true
}

/// The selector targets a witnessed navigation element or something beneath it, never a sibling.
fn beneath_navigation(selector: &str) -> bool {
    let Some(plain) = plain_selector(selector) else { return false };
    let plain = normalize(&plain);
    HIDE_ALLOWED.iter().any(|allowed| {
        plain.strip_prefix(allowed).is_some_and(|rest| {
            (rest.is_empty() || rest.starts_with(' ') || rest.starts_with('>'))
                && !rest.contains(['+', '~', ',', '(', ':'])
        })
    })
}

/// Consumes a block that is not checked. Without this, the next rule's prelude would start inside the block.
fn skip_block<'i>(parser: &mut cssparser::Parser<'i, '_>, walk: &mut CssWalk<'_>) -> Result<(), CssFail<'i>> {
    parser.parse_nested_block(|block| {
        while block.next_including_whitespace_and_comments().is_ok() {
            walk.tick()?;
        }
        Ok(())
    })
}

fn prelude_until_block<'i>(
    parser: &mut cssparser::Parser<'i, '_>,
    walk: &mut CssWalk<'_>,
) -> Result<bool, CssFail<'i>> {
    use cssparser::Token;
    loop {
        walk.tick()?;
        match parser.next() {
            Ok(Token::CurlyBracketBlock) => return Ok(true),
            Ok(Token::Semicolon) | Err(_) => return Ok(false),
            Ok(Token::BadUrl(_) | Token::BadString(_)) => return Err(refuse()),
            Ok(_) => {}
        }
    }
}

fn at_rule<'i>(
    parser: &mut cssparser::Parser<'i, '_>,
    walk: &mut CssWalk<'_>,
    name: &str,
    depth: usize,
) -> Result<(), CssFail<'i>> {
    walk.rule()?;
    let block = prelude_until_block(parser, walk)?;
    match (name, block) {
        ("charset", false) | ("layer", false) => Ok(()),
        ("media" | "supports" | "layer" | "container", true) => {
            parser.parse_nested_block(|nested| rule_list(nested, walk, depth + 1))
        }
        ("font-face", true) => parser.parse_nested_block(|nested| font_face(nested, walk)),
        // Keyframes act only through `animation`, which no checked rule may use.
        ("keyframes" | "-webkit-keyframes" | "-moz-keyframes", true) => skip_block(parser, walk),
        // Page boxes: size and margins only; margin boxes such as `@bottom-center` could print generated text.
        ("page", true) => parser.parse_nested_block(|nested| page_box(nested, walk)),
        // `@import` loads further, unchecked CSS; every other at-rule is unwitnessed.
        _ => Err(refuse()),
    }
}

/// Font descriptors only. Fonts are never loaded by Torro, and an inline `data:` font is refused.
fn font_face<'i>(parser: &mut cssparser::Parser<'i, '_>, walk: &mut CssWalk<'_>) -> Result<(), CssFail<'i>> {
    use cssparser::Token;
    loop {
        walk.tick()?;
        let name = match parser.next() {
            Err(_) => return Ok(()),
            Ok(Token::Semicolon) => continue,
            Ok(Token::Ident(name)) => name.to_ascii_lowercase(),
            Ok(_) => return Err(refuse()),
        };
        parser.expect_colon().map_err(|_| refuse())?;
        let values = parser.parse_until_after(cssparser::Delimiter::Semicolon, |value| values(value, walk, 0))?;
        if !matches!(
            name.as_str(),
            "font-family" | "src" | "font-weight" | "font-style" | "font-display" | "font-stretch" | "unicode-range"
        ) || inline_data(&values)
        {
            return Err(refuse());
        }
    }
}

fn page_box<'i>(parser: &mut cssparser::Parser<'i, '_>, walk: &mut CssWalk<'_>) -> Result<(), CssFail<'i>> {
    use cssparser::Token;
    loop {
        walk.tick()?;
        let name = match parser.next() {
            Err(_) => return Ok(()),
            Ok(Token::Semicolon) => continue,
            Ok(Token::Ident(name)) => name.to_ascii_lowercase(),
            Ok(_) => return Err(refuse()),
        };
        parser.expect_colon().map_err(|_| refuse())?;
        let values = parser.parse_until_after(cssparser::Delimiter::Semicolon, |value| values(value, walk, 0))?;
        if !(matches!(name.as_str(), "size" | "page-orientation" | "marks" | "bleed") || name.starts_with("margin"))
            || values.iter().any(|value| matches!(value, Value::Function(..) | Value::Url(_) | Value::Text(_)))
        {
            return Err(refuse());
        }
    }
}

/// The checked declarations of a rule that can match the page.
fn declarations<'i>(
    parser: &mut cssparser::Parser<'i, '_>,
    walk: &mut CssWalk<'_>,
    may_hide: bool,
) -> Result<(), CssFail<'i>> {
    use cssparser::Token;
    loop {
        walk.tick()?;
        let name = match parser.next() {
            Err(_) => return Ok(()),
            Ok(Token::Semicolon) => continue,
            Ok(Token::Ident(name)) => name.to_ascii_lowercase(),
            // Nested rules and at-rules inside a declaration block are not part of the checked grammar.
            Ok(_) => return Err(refuse()),
        };
        parser.expect_colon().map_err(|_| refuse())?;
        let values = parser.parse_until_after(cssparser::Delimiter::Semicolon, |value| values(value, walk, 0))?;
        let values: Vec<_> = values.into_iter().filter(|value| *value != Value::Word("important".into())).collect();
        if !allowed(&name, &values, may_hide) {
            return Err(refuse());
        }
    }
}

/// The allow list, witnessed on Paddle's invoice stylesheets. Each entry names the values that cannot generate
/// or hide text; everything else is refused.
fn allowed(name: &str, values: &[Value], may_hide: bool) -> bool {
    let words = |wanted: &[&str]| {
        values.iter().all(|value| matches!(value, Value::Word(word) if wanted.contains(&word.as_str())))
    };
    let functions = |allowed: &[&str]| {
        values.iter().all(|value| match value {
            Value::Function(name, arguments) => {
                allowed.contains(&name.as_str())
                    && arguments.iter().all(|argument| !matches!(argument, Value::Function(..) | Value::Url(_)))
            }
            Value::Url(_) => false,
            _ => true,
        })
    };
    let colours = &["rgb", "rgba", "hsl", "hsla"];
    let global = &["inherit", "initial", "unset", "revert"];
    match name {
        "content" => {
            matches!(values, [Value::Word(word)] if matches!(word.as_str(), "none" | "normal"))
                || matches!(values, [Value::Text(text)] if text.is_empty())
        }
        "display" => {
            words(&[
                "block",
                "inline",
                "inline-block",
                "flex",
                "inline-flex",
                "-ms-flexbox",
                "-ms-inline-flexbox",
                "-webkit-box",
                "grid",
                "inline-grid",
                "table",
                "table-row",
                "table-cell",
                "table-row-group",
                "table-header-group",
                "table-footer-group",
                "table-caption",
                "flow-root",
                "contents",
                "inherit",
                "initial",
                "unset",
            ]) || (may_hide && words(&["none"]))
        }
        "visibility" => words(&["visible", "inherit"]) || (may_hide && words(&["hidden", "collapse"])),
        "position" => {
            words(&["static", "relative", "inherit"])
                || (may_hide && words(&["absolute", "fixed", "sticky", "-webkit-sticky"]))
        }
        "opacity" => {
            may_hide
                || values.iter().all(|value| match value {
                    Value::Number(n) => *n >= 0.5,
                    Value::Length(n, unit) => unit == "%" && *n >= 50.0,
                    Value::Word(word) => global.contains(&word.as_str()),
                    _ => false,
                })
        }
        "color" | "-webkit-text-fill-color" => (may_hide || visible_colour(values)) && functions(colours),
        "font-size" => values.len() == 1 && readable_size(&values[0]),
        // The first length is the size; a length or number after `/` is the line height.
        "font" => {
            let slash = values.iter().position(|value| *value == Value::Slash);
            let (size, line) = values.split_at(slash.unwrap_or(values.len()));
            functions(&[])
                && !size.iter().any(|value| matches!(value, Value::Number(n) if *n == 0.0))
                && size.iter().filter(|value| matches!(value, Value::Length(..))).take(1).all(readable_size)
                && line.get(1).is_none_or(|value| allowed("line-height", std::slice::from_ref(value), false))
        }
        "line-height" => {
            values.len() == 1
                && match &values[0] {
                    Value::Number(n) => *n >= 0.8,
                    Value::Length(n, unit) if unit == "%" => *n >= 80.0,
                    value @ Value::Length(..) => px(value).is_some_and(|n| n >= 8.0),
                    Value::Word(word) => word == "normal" || global.contains(&word.as_str()),
                    _ => false,
                }
        }
        "letter-spacing" | "word-spacing" => {
            values.len() == 1
                && match &values[0] {
                    Value::Word(word) => word == "normal" || global.contains(&word.as_str()),
                    Value::Number(n) => *n == 0.0,
                    value => px(value).is_some_and(|n| (-1.0..=8.0).contains(&n)),
                }
        }
        "text-indent"
        | "margin"
        | "margin-top"
        | "margin-right"
        | "margin-bottom"
        | "margin-left"
        | "margin-inline"
        | "margin-block"
        | "margin-inline-start"
        | "margin-inline-end"
        | "margin-block-start"
        | "margin-block-end"
        | "top"
        | "right"
        | "bottom"
        | "left"
        | "inset" => functions(&["calc"]) && bounded(values, 64.0),
        "padding" | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" | "padding-inline"
        | "padding-block" => functions(&["calc"]) && bounded(values, 200.0),
        "width"
        | "height"
        | "min-width"
        | "min-height"
        | "max-width"
        | "max-height"
        | "flex-basis"
        | "-ms-flex-preferred-size" => functions(&["calc"]) && bounded(values, 2000.0),
        "gap" | "row-gap" | "column-gap" | "border-spacing" => functions(&["calc"]) && bounded(values, 64.0),
        // Lines and shadows may not grow into an overlay: widths, offsets and spreads stay small.
        "border"
        | "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left"
        | "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "outline"
        | "outline-width"
        | "outline-offset" => functions(colours) && small_lengths(values, 8.0),
        "box-shadow" => functions(colours) && small_lengths(values, 32.0),
        "text-decoration" | "text-decoration-line" | "-webkit-text-decoration-line" | "text-underline-offset" => {
            functions(colours) && small_lengths(values, 4.0)
        }
        "z-index" => values.iter().all(|value| match value {
            Value::Number(n) => (0.0..=100.0).contains(n),
            Value::Word(word) => word == "auto" || global.contains(&word.as_str()),
            _ => false,
        }),
        "text-transform" => words(&["none", "uppercase", "lowercase", "capitalize", "inherit"]),
        "unicode-bidi" => words(&["normal", "embed", "isolate", "inherit"]),
        "direction" => words(&["ltr", "inherit"]),
        "list-style" | "list-style-type" => {
            !values.iter().any(|value| matches!(value, Value::Text(_))) && functions(&[])
        }
        "background" => functions(colours) && !values.iter().any(|value| matches!(value, Value::Url(_))),
        "box-sizing"
        | "-ms-flex"
        | "-ms-flex-direction"
        | "-ms-flex-pack"
        | "-ms-flex-wrap"
        | "-ms-flex-align"
        | "-ms-flex-item-align"
        | "-ms-flex-line-pack"
        | "-ms-flex-positive"
        | "-ms-flex-negative"
        | "-ms-flex-order"
        | "flex"
        | "flex-direction"
        | "flex-wrap"
        | "flex-flow"
        | "flex-grow"
        | "flex-shrink"
        | "justify-content"
        | "align-items"
        | "align-content"
        | "align-self"
        | "justify-items"
        | "justify-self"
        | "border-style"
        | "border-color"
        | "border-top-color"
        | "border-right-color"
        | "border-bottom-color"
        | "border-left-color"
        | "border-top-style"
        | "border-right-style"
        | "border-bottom-style"
        | "border-left-style"
        | "border-radius"
        | "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-left-radius"
        | "border-bottom-right-radius"
        | "border-collapse"
        | "outline-style"
        | "outline-color"
        | "background-color"
        | "vertical-align"
        | "text-align"
        | "text-decoration-color"
        | "text-decoration-style"
        | "-webkit-text-decoration-skip"
        | "text-decoration-skip-ink"
        | "cursor"
        | "font-weight"
        | "font-style"
        | "font-family"
        | "font-variant"
        | "white-space"
        | "word-break"
        | "overflow-wrap"
        | "word-wrap"
        | "hyphens"
        | "table-layout"
        | "caption-side"
        | "empty-cells"
        | "transition"
        | "transition-property"
        | "transition-duration"
        | "transition-timing-function"
        | "transition-delay"
        | "quotes"
        | "-webkit-font-smoothing"
        | "-moz-osx-font-smoothing"
        | "text-rendering"
        | "-webkit-text-size-adjust"
        | "-ms-text-size-adjust"
        | "text-size-adjust"
        | "user-select"
        | "-webkit-user-select"
        | "-moz-user-select"
        | "-ms-user-select"
        | "pointer-events"
        | "appearance"
        | "-webkit-appearance"
        | "-moz-appearance"
        | "resize"
        | "float"
        | "clear"
        | "caret-color"
        | "accent-color"
        | "tab-size"
        | "-moz-tab-size"
        | "-webkit-tap-highlight-color"
        | "touch-action"
        | "-ms-overflow-style"
        | "font-kerning" => functions(&["rgb", "rgba", "hsl", "hsla", "cubic-bezier"]),
        _ => false,
    }
}

/// Text colour with at least 50 % alpha; `transparent` is refused.
fn visible_colour(values: &[Value]) -> bool {
    values.iter().all(|value| match value {
        Value::Word(word) => word != "transparent",
        Value::Hex(hex) => match hex.len() {
            4 => u8::from_str_radix(&hex[3..4], 16).is_ok_and(|alpha| alpha >= 8),
            8 => u8::from_str_radix(&hex[6..8], 16).is_ok_and(|alpha| alpha >= 0x80),
            _ => true,
        },
        Value::Function(_, arguments) => {
            let numbers: Vec<_> = arguments
                .iter()
                .filter_map(|value| match value {
                    Value::Number(n) => Some(*n),
                    Value::Length(n, unit) if unit == "%" => Some(*n / 100.0),
                    Value::Length(n, _) => Some(*n),
                    _ => None,
                })
                .collect();
            arguments.iter().all(|value| !matches!(value, Value::Word(_)))
                && (numbers.len() == 3 || numbers.last().is_some_and(|alpha| *alpha >= 0.5))
        }
        _ => false,
    })
}

/// Font sizes from 8px: absolute units, at least half the parent size, or a keyword.
fn readable_size(value: &Value) -> bool {
    match value {
        Value::Word(word) => matches!(
            word.as_str(),
            "xx-small"
                | "x-small"
                | "small"
                | "medium"
                | "large"
                | "x-large"
                | "xx-large"
                | "xxx-large"
                | "larger"
                | "inherit"
                | "initial"
                | "unset"
        ),
        // `%` and `em` compound through nested elements, so they may not shrink; `rem` is fixed to the root.
        Value::Length(n, unit) => match unit.as_str() {
            "%" => *n >= 100.0,
            "em" => *n >= 1.0,
            "rem" => *n >= 0.5,
            _ => px(value).is_some_and(|n| n >= 8.0),
        },
        _ => false,
    }
}

/// Lengths and `calc()` whose magnitude stays within `limit` CSS pixels, counting every factor at full size.
fn bounded(values: &[Value], limit: f32) -> bool {
    fn size(value: &Value) -> Option<f32> {
        match value {
            Value::Word(word) => matches!(word.as_str(), "auto" | "inherit" | "initial" | "unset").then_some(0.0),
            Value::Number(n) => Some(n.abs()),
            Value::Length(..) => px(value).map(f32::abs),
            // A division could grow a length without bound; only sums and products are counted.
            Value::Function(name, arguments) if name == "calc" && !arguments.contains(&Value::Slash) => {
                let lengths = arguments.iter().filter(|argument| matches!(argument, Value::Length(..))).map(size);
                let factors: f32 = arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Value::Number(n) => Some(n.abs().max(1.0)),
                        _ => None,
                    })
                    .product();
                lengths.sum::<Option<f32>>().map(|sum| sum * factors)
            }
            Value::Other => Some(0.0),
            _ => None,
        }
    }
    values.iter().all(|value| size(value).is_some_and(|n| n <= limit))
}

/// Every length within `limit` CSS pixels; numbers and words pass.
fn small_lengths(values: &[Value], limit: f32) -> bool {
    values.iter().all(|value| match value {
        Value::Length(..) => px(value).is_some_and(|n| n.abs() <= limit),
        _ => true,
    })
}

/// An approximate length in CSS pixels, with font-relative units at 16px and viewport units at 1000px.
fn px(value: &Value) -> Option<f32> {
    let Value::Length(n, unit) = value else { return None };
    let factor = match unit.as_str() {
        "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 37.8,
        "mm" => 3.78,
        "q" => 0.945,
        "em" | "rem" | "ch" | "ex" | "lh" | "rlh" | "cap" | "ic" => 16.0,
        "%" | "vw" | "vh" | "vmin" | "vmax" | "svw" | "svh" | "lvw" | "lvh" | "dvw" | "dvh" | "vi" | "vb" => 10.0,
        _ => return None,
    };
    Some(n * factor)
}

/// `url(data:…)` in any spelling, also nested in functions.
fn inline_data(values: &[Value]) -> bool {
    values.iter().any(|value| match value {
        Value::Url(url) | Value::Text(url) => url.trim_start().to_ascii_lowercase().starts_with("data:"),
        Value::Function(_, arguments) => inline_data(arguments),
        _ => false,
    })
}

fn values<'i>(
    parser: &mut cssparser::Parser<'i, '_>,
    walk: &mut CssWalk<'_>,
    depth: usize,
) -> Result<Vec<Value>, CssFail<'i>> {
    use cssparser::Token;
    if depth > MAX_CSS_DEPTH {
        return Err(refuse());
    }
    let mut out = Vec::new();
    loop {
        walk.tick()?;
        let token = match parser.next() {
            Ok(token) => token.clone(),
            Err(_) => return Ok(out),
        };
        out.push(match token {
            Token::Ident(word) => Value::Word(word.to_ascii_lowercase()),
            Token::QuotedString(text) => Value::Text(text.to_string()),
            Token::Number { value, .. } => Value::Number(value),
            Token::Percentage { unit_value, .. } => Value::Length(unit_value * 100.0, "%".into()),
            Token::Dimension { value, unit, .. } => Value::Length(value, unit.to_ascii_lowercase()),
            Token::Hash(hex) | Token::IDHash(hex) => Value::Hex(hex.to_ascii_lowercase()),
            Token::UnquotedUrl(url) => Value::Url(url.to_string()),
            Token::Function(name) => {
                let name = name.to_ascii_lowercase();
                let arguments = parser.parse_nested_block(|nested| values(nested, walk, depth + 1))?;
                if name == "url" {
                    let target = arguments.iter().find_map(|argument| match argument {
                        Value::Text(text) => Some(text.clone()),
                        _ => None,
                    });
                    Value::Url(target.unwrap_or_default())
                } else {
                    Value::Function(name, arguments)
                }
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock => {
                Value::Function(String::new(), parser.parse_nested_block(|nested| values(nested, walk, depth + 1))?)
            }
            Token::CurlyBracketBlock | Token::BadUrl(_) | Token::BadString(_) => return Err(refuse()),
            // `!important` changes no visible content; its keyword is dropped by the caller.
            Token::Delim('!') => continue,
            Token::Delim('/') => Value::Slash,
            _ => Value::Other,
        });
    }
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
