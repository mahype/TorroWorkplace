//! The mail search (issue #136, task M8.2): one field over the list of every view of mail.
//!
//! - **Where:** every folder of every account by default ([`Scope::All`]); from a folder the bar
//!   also offers "This account" and "This folder" ([`ScopeChoice`]).
//! - **When:** while typing, [`DEBOUNCE`] after the last key (the app ticks with
//!   [`Message::SearchTick`]); `Enter` searches at once and remembers the search.
//! - **What:** words and operators as the core reads them (`from:`, `"phrases"`, `-word`, `OR` …,
//!   `mail.search_messages`); the core searches its own index, offline too.
//! - **Suggestions** under the field ([`suggestions`]): searches last made (kept in `ui.json`,
//!   never sent anywhere), senders the window knows, folders and operators; `Tab` takes the first.
//! - **Results** highlighted where the words matched (`list::Hits`), by relevance or by date
//!   ([`Sort`]), with how many there are and – honestly – when some may be missing because the
//!   index is still being built ([`status_line`], `mail.index_status`).
//!
//! The demo has no core: [`demo_find`] searches its sample mail the same way, words and the
//! common operators, and says one account is still indexing, so every state can be photographed.

use std::time::Duration;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, svg, text_input};
use iced::{Alignment, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, space};
use torro_gui::ui_state::SEARCHES_MAX;
use torro_gui::widgets::{self, Mark};
use torro_gui::{style, typo};
use torro_inbox::InboxError;
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::{MailFind, MailFound};
use torro_ipc::mail::{HighlightSpan, IndexState, MailHighlight, MailIndexProgress, MailSort};
use torro_mail::{Flag, Folder, MailAddress, MessageSummary, Order, Role};

use crate::app::Message;
use crate::mail::{self, FolderView};

/// The id of the search field over the list.
pub const FIELD: &str = "mail-search";

/// How long after the last key the search runs.
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// How often the window looks whether a search is due while one waits.
pub const TICK: Duration = Duration::from_millis(50);

/// How many messages one page of a search holds.
pub const PAGE: u32 = 50;

/// How many suggestions show at most.
const SUGGESTIONS_MAX: usize = 6;

/// Where a search looks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every folder of every account (the default).
    All,
    /// Every folder of one account, `mail:<address>`.
    Account(String),
    /// One folder.
    Folder(FolderView),
}

/// How the results are ordered.
pub type Sort = MailSort;

/// A search the list shows: what was typed (trimmed; empty before the first word, then the list
/// says what can be searched for), where, and in which order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchView {
    pub query: String,
    pub scope: Scope,
    pub sort: Sort,
}

impl SearchView {
    /// A search for `query` in `scope`, best matches first.
    pub fn new(query: &str, scope: Scope, sort: Sort) -> SearchView {
        SearchView { query: query.trim().to_owned(), scope, sort }
    }

    /// The request for a page of this search: `limit` messages from `cursor` on, by date in
    /// `order` when sorted by date.
    pub fn find(&self, order: Order, limit: u32, cursor: Option<String>) -> MailFind {
        let (account, folder) = match &self.scope {
            Scope::All => (None, None),
            Scope::Account(account) => (Some(account.clone()), None),
            Scope::Folder(folder) => (Some(folder.account.clone()), Some(folder.id.clone())),
        };
        MailFind { account, folder, query: self.query.clone(), sort: self.sort, order, limit, cursor }
    }
}

/// What the core said about a search beyond its messages, for the line under the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `false` when matches may be missing: the index is still being built.
    pub complete: bool,
    /// About how many messages match in all.
    pub total: Option<u64>,
    /// How far the index of each account is; asked only while matches may be missing.
    pub index: Vec<MailIndexProgress>,
}

impl Default for Found {
    fn default() -> Self {
        Found { complete: true, total: None, index: Vec::new() }
    }
}

/// Which scopes the bar offers: from a folder, its account and the folder itself beside all mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeChoice {
    /// The folder the search started from.
    pub folder: FolderView,
    /// The scope chosen now.
    pub chosen: Scope,
}

/// What kind of suggestion it is: its icon and its word in the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// A search made before.
    Recent,
    /// A sender or recipient the window knows: `from:` them.
    Person,
    /// A folder: `in:` it.
    Folder,
    /// An operator of the query language.
    Operator,
}

impl SuggestionKind {
    fn icon(self) -> Icon {
        match self {
            SuggestionKind::Recent => Icon::History,
            SuggestionKind::Person => Icon::User,
            SuggestionKind::Folder => Icon::Folder,
            SuggestionKind::Operator => Icon::Search,
        }
    }
}

/// A suggestion under the field: what the chip says and the whole query after picking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub label: String,
    /// The query in the field once picked.
    pub query: String,
}

/// The operators the bar suggests, as typed, with what they do.
fn operators() -> [(&'static str, &'static str); 10] {
    [
        ("from:", tr("Sender")),
        ("to:", tr("Recipient")),
        ("subject:", tr("Subject")),
        ("has:attachment", tr("With attachments")),
        ("filename:", tr("Name of an attachment")),
        ("in:", tr("Folder")),
        ("is:unread", tr("Unread")),
        ("is:flagged", tr("Flagged")),
        ("before:", tr("Before a date, such as 2026-03")),
        ("after:", tr("From a date on, such as 2026-03")),
    ]
}

/// The values `is:` takes.
const IS_VALUES: [&str; 4] = ["unread", "read", "flagged", "unflagged"];

/// `typed` with its last word (after the last white space) replaced by `word`; a space follows
/// unless `word` ends in `:`, where its value comes next.
pub fn complete(typed: &str, word: &str) -> String {
    let start = typed.rfind(char::is_whitespace).map_or(0, |at| at + 1);
    let mut query = format!("{}{word}", &typed[..start]);
    if !word.ends_with(':') {
        query.push(' ');
    }
    query
}

/// The last word of `typed`: what is being typed now (empty after a space).
fn last_word(typed: &str) -> &str {
    typed.rsplit(char::is_whitespace).next().unwrap_or_default()
}

/// `value` as an operator takes it: in quotes when it holds white space.
fn quoted(value: &str) -> String {
    if value.contains(char::is_whitespace) { format!("\"{value}\"") } else { value.to_owned() }
}

/// The word `in:` takes for `folder`: its role for the folders every account has, else its name.
fn folder_word(folder: &Folder) -> String {
    let role = match folder.role {
        Some(Role::Inbox) => Some("inbox"),
        Some(Role::Sent) => Some("sent"),
        Some(Role::Drafts) => Some("drafts"),
        Some(Role::Archive) => Some("archive"),
        Some(Role::Junk) => Some("junk"),
        Some(Role::Trash) => Some("trash"),
        _ => None,
    };
    role.map_or_else(|| quoted(&folder.name), str::to_owned)
}

/// What to suggest for `typed` (issue #136), at most six, each once:
///
/// - nothing typed: the searches last made (`recent`);
/// - an operator being typed (`from:an`, `in:pro`, `is:un`, `has:`): the people, folders or values
///   that fit;
/// - a word: searches last made that start like `typed`, the operators that start with it, the
///   people (from two letters) and folders that hold it.
///
/// `people` are the senders and recipients the window knows (from its lists), `folders` every
/// folder.
pub fn suggestions(typed: &str, recent: &[String], people: &[MailAddress], folders: &[Folder]) -> Vec<Suggestion> {
    let mut found: Vec<Suggestion> = Vec::new();
    let mut add = |kind: SuggestionKind, label: String, query: String| {
        if found.len() < SUGGESTIONS_MAX
            && !found.iter().any(|known| known.query == query)
            && query.trim() != typed.trim()
        {
            found.push(Suggestion { kind, label, query });
        }
    };
    if typed.trim().is_empty() {
        for search in recent {
            add(SuggestionKind::Recent, search.clone(), search.clone());
        }
        return found;
    }
    let word = last_word(typed);
    let lower = word.to_lowercase();
    let person = |value: &str| {
        let value = value.to_lowercase();
        people.iter().filter(move |who| {
            who.address.to_lowercase().contains(&value)
                || who.name.as_deref().is_some_and(|name| name.to_lowercase().contains(&value))
        })
    };
    let folder = |value: &str| {
        let value = value.to_lowercase();
        folders.iter().filter(move |folder| folder.name.to_lowercase().contains(&value))
    };
    if let Some((operator, value)) = lower.split_once(':') {
        match operator {
            "from" | "to" => {
                for who in person(value) {
                    let token = format!("{operator}:{}", who.address);
                    add(SuggestionKind::Person, token.clone(), complete(typed, &token));
                }
            }
            "in" => {
                for folder in folder(value) {
                    let token = format!("in:{}", folder_word(folder));
                    add(SuggestionKind::Folder, token.clone(), complete(typed, &token));
                }
            }
            "is" => {
                for value in IS_VALUES.iter().filter(|known| known.starts_with(value)) {
                    let token = format!("is:{value}");
                    add(SuggestionKind::Operator, token.clone(), complete(typed, &token));
                }
            }
            "has" if "attachment".starts_with(value) => {
                add(SuggestionKind::Operator, "has:attachment".into(), complete(typed, "has:attachment"));
            }
            _ => {}
        }
        return found;
    }
    if lower.is_empty() {
        return found;
    }
    let typed_lower = typed.trim().to_lowercase();
    for search in recent.iter().filter(|search| search.to_lowercase().starts_with(&typed_lower)) {
        add(SuggestionKind::Recent, search.clone(), search.clone());
    }
    for (operator, _) in operators().into_iter().filter(|(operator, _)| operator.starts_with(&lower)) {
        add(SuggestionKind::Operator, operator.to_owned(), complete(typed, operator));
    }
    if lower.chars().count() >= 2 {
        for who in person(&lower) {
            let token = format!("from:{}", who.address);
            add(SuggestionKind::Person, token.clone(), complete(typed, &token));
        }
        for folder in folder(&lower) {
            let token = format!("in:{}", folder_word(folder));
            add(SuggestionKind::Folder, token.clone(), complete(typed, &token));
        }
    }
    found
}

/// The senders and recipients of `messages`, each address once, in the order they first appear.
pub fn people(messages: &[MessageSummary]) -> Vec<MailAddress> {
    let mut found: Vec<MailAddress> = Vec::new();
    for who in messages.iter().flat_map(|message| message.from.iter().chain(&message.to)) {
        if !found.iter().any(|known| known.address.eq_ignore_ascii_case(&who.address)) {
            found.push(who.clone());
        }
    }
    found
}

/// `recent` with `query` in front (trimmed, once, case as typed), at most [`SEARCHES_MAX`]; a blank
/// query changes nothing.
pub fn remember(recent: &[String], query: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return recent.to_vec();
    }
    let mut list = vec![query.to_owned()];
    list.extend(recent.iter().filter(|known| !known.eq_ignore_ascii_case(query)).cloned());
    list.truncate(SEARCHES_MAX);
    list
}

/// What the bar over the list needs ([`bar`]).
pub struct Bar<'a> {
    /// What is in the field.
    pub input: &'a str,
    /// The search the list shows, if it shows one.
    pub search: Option<&'a SearchView>,
    /// The scopes to offer, when the search started from a folder.
    pub scopes: Option<ScopeChoice>,
    /// The suggestions under the field.
    pub suggestions: Vec<Suggestion>,
    /// What the line under the bar says, while a search with words is shown ([`status_line`]).
    pub status: Option<Element<'a, Message>>,
}

/// The placeholder of the field: where it searches.
fn placeholder(search: Option<&SearchView>, scopes: Option<&ScopeChoice>) -> String {
    let scope = search.map(|search| &search.scope).or(scopes.map(|choice| &choice.chosen));
    match scope {
        Some(Scope::Folder(folder)) => trf("Search {folder}", &[("folder", &folder.name)]),
        Some(Scope::Account(account)) => trf("Search {account}", &[("account", &mail::address(account))]),
        Some(Scope::All) | None => tr("Search all mail").to_owned(),
    }
}

/// The bar over the list of every view of mail (issue #136): the field (`/`, ⌘F / Ctrl + F and
/// the sidebar's search button focus it; typing searches, `Enter` at once) with "Clear search"
/// while there is something to clear; the suggestions under it ([`suggestions`], `Tab` takes the
/// first); while a search is shown, where it looks (all mail, this account, this folder) and the
/// order (relevance or date), then the line on its results ([`status_line`]).
pub fn bar<'a>(bar: Bar<'a>) -> Element<'a, Message> {
    let field = text_input(&placeholder(bar.search, bar.scopes.as_ref()), bar.input)
        .id(FIELD)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(Message::SearchInput)
        .on_submit(Message::Search);
    let mut top = row![field].spacing(space::s()).align_y(Alignment::Center);
    if bar.search.is_some() || !bar.input.is_empty() {
        top = top.push(widgets::icon_button(Icon::X, tr("Clear search"), Some(Message::ClearSearch)));
    }
    let mut lines = column![top].spacing(space::s());
    if !bar.suggestions.is_empty() {
        lines = lines.push(suggestion_row(bar.suggestions, bar.input.trim().is_empty()));
    }
    if let Some(search) = bar.search.filter(|search| !search.query.is_empty()) {
        lines = lines.push(controls(search, bar.scopes));
    }
    if let Some(status) = bar.status {
        lines = lines.push(status);
    }
    container(lines).width(Fill).padding(Padding::new(space::s()).left(space::l()).right(space::l())).into()
}

/// The suggestions as chips, the first with the `Tab` hint; the searches last made (`recent`, with
/// nothing typed) end with "Clear" to forget them.
fn suggestion_row<'a>(suggestions: Vec<Suggestion>, recent: bool) -> Element<'a, Message> {
    let mut chips: Vec<Element<'a, Message>> = Vec::new();
    for (i, suggestion) in suggestions.into_iter().enumerate() {
        // An operator says what it does in its tooltip ("from:" – "Sender").
        let hint = (suggestion.kind == SuggestionKind::Operator)
            .then(|| operators().into_iter().find(|(operator, _)| *operator == suggestion.label))
            .flatten()
            .map(|(_, hint)| hint);
        let chip = widgets::choice_chip(
            suggestion.label,
            Some(Mark::Icon(suggestion.kind.icon())),
            false,
            Some(Message::Suggest(suggestion.query)),
        );
        chips.push(match hint {
            Some(hint) => widgets::with_tooltip(chip, hint),
            None => chip,
        });
        if i == 0 {
            chips.push(container(widgets::kbd("Tab")).center_y(theme::size::control_sm()).into());
        }
    }
    if recent {
        let forget = widgets::text_button(None, tr("Clear"), style::button::ghost, Some(Message::ForgetSearches));
        chips.push(widgets::with_tooltip(forget, tr("Forget recent searches")));
    }
    widgets::chip_row(chips)
}

/// Where the search looks and in which order: the scopes when it started from a folder, then
/// "Relevance | Date"; in a narrow window the order goes below the scopes rather than out of it.
fn controls<'a>(search: &SearchView, scopes: Option<ScopeChoice>) -> Element<'a, Message> {
    let mut line = row![].spacing(space::m()).align_y(Alignment::Center);
    if let Some(choice) = scopes {
        let account = Scope::Account(choice.folder.account.clone());
        let folder = Scope::Folder(choice.folder.clone());
        let options = [Scope::All, account, folder];
        let active = options.iter().position(|scope| *scope == search.scope).unwrap_or(0);
        let labels = [tr("All mail"), tr("This account"), tr("This folder")];
        let segments = labels.into_iter().zip(options.map(Message::SearchScope));
        line = line.push(widgets::segmented(segments, active));
    }
    let sorts =
        [(tr("Relevance"), Message::SearchSort(Sort::Relevance)), (tr("Date"), Message::SearchSort(Sort::Date))];
    let active = usize::from(search.sort == Sort::Date);
    line.push(widgets::segmented(sorts, active)).wrap().vertical_spacing(space::s()).into()
}

/// How many results there are, in words: "12 results", "At least 40 results" while matches may be
/// missing, "More than 50 results" when only a page is known.
pub fn count_text(shown: usize, more: bool, found: &Found) -> String {
    match found.total {
        Some(1) if found.complete => tr("1 result").to_owned(),
        Some(total) if found.complete => trf("{n} results", &[("n", &total)]),
        Some(1) => tr("At least 1 result").to_owned(),
        Some(total) => trf("At least {n} results", &[("n", &total)]),
        None if more => trf("More than {n} results", &[("n", &shown)]),
        None if shown == 1 => tr("1 result").to_owned(),
        None => trf("{n} results", &[("n", &shown)]),
    }
}

/// Why matches may be missing, honestly: how far the index of the accounts is ("Older mail is
/// still being indexed: 1240 of 1800 messages."), and that it goes on by itself when it waits for
/// the server. `None` when every match is there.
pub fn missing_text(found: &Found) -> Option<String> {
    if found.complete {
        return None;
    }
    let open: Vec<&MailIndexProgress> =
        found.index.iter().filter(|account| account.state != IndexState::Complete).collect();
    if open.is_empty() {
        return Some(tr("Some matches may be missing: older mail is still being indexed.").to_owned());
    }
    let indexed: u64 = open.iter().map(|account| account.indexed).sum();
    let messages: u64 = open.iter().map(|account| account.messages).sum();
    let counts = [("indexed", &indexed as &dyn std::fmt::Display), ("total", &messages)];
    if open.iter().all(|account| account.state == IndexState::Waiting) {
        return Some(trf(
            "Some matches may be missing: {indexed} of {total} messages indexed. Indexing goes on once the server is reached.",
            &counts,
        ));
    }
    Some(trf("Some matches may be missing: older mail is being indexed, {indexed} of {total} messages.", &counts))
}

/// The line under the bar while a search with words is shown: how many results, then – with an
/// icon in the attention colour and in words, never by colour alone – why matches may be missing
/// ([`missing_text`]).
pub fn status_line<'a>(shown: usize, more: bool, found: &Found) -> Element<'a, Message> {
    let count = typo::text(typo::meta().tabular(), count_text(shown, more, found)).style(theme::text_3);
    let mut line = column![count].spacing(space::xs());
    if let Some(missing) = missing_text(found) {
        // "Still running" in the attention colour (design: checks that run), with the words beside.
        let mark = icon::icon(Icon::CircleDotDashed, icon::SIZE_SM)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention) });
        let mark = container(mark).center_y(typo::line_height(typo::meta()));
        let words = typo::text(typo::meta(), missing).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        line = line.push(row![mark, words].spacing(space::s()).align_y(Alignment::Start));
    }
    line.into()
}

/// The empty states of a search: before the first word, what can be searched for; without a
/// match, to try other words – and, while the index is incomplete, that older mail may still come.
pub fn empty_texts(search: &SearchView) -> (Icon, &'static str, &'static str) {
    if search.query.is_empty() {
        (
            Icon::Search,
            tr("Search all your mail"),
            tr("Words, \"phrases\", -word, OR, from:, to:, subject:, has:attachment, in:, before:, after:, is:unread."),
        )
    } else {
        (Icon::Search, tr("No messages found."), tr("Try other words."))
    }
}

/// The messages of a search with the passage around their first match (`highlights`) as their
/// preview, where the core found one: the list shows where the words matched rather than how the
/// mail starts, and a card is as high as that passage.
pub fn with_passages(messages: &mut [MessageSummary], highlights: &[MailHighlight]) {
    for message in messages {
        let passage = highlights
            .iter()
            .find(|highlight| highlight.id == message.id)
            .map(|highlight| highlight.snippet.iter().map(|span| span.text.as_str()).collect::<String>());
        if let Some(passage) = passage.filter(|passage| !passage.trim().is_empty()) {
            message.snippet = passage;
        }
    }
}

/// `text` cut into pieces at the matches of `words` (ASCII case ignored, so the pieces keep their
/// places): what the core's highlights look like, for the demo, and how a line of a row or card
/// stresses the words that matched (`list::Hits`).
pub fn spans_of(text: &str, words: &[String]) -> Vec<HighlightSpan> {
    let lower = text.to_ascii_lowercase();
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for word in words.iter().filter(|word| !word.is_empty()) {
        let word = word.to_ascii_lowercase();
        hits.extend(lower.match_indices(&word).map(|(at, found)| (at, at + found.len())));
    }
    hits.sort_unstable();
    let mut spans = Vec::new();
    let mut from = 0;
    for (start, end) in hits {
        if start < from || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        if start > from {
            spans.push(HighlightSpan { text: text[from..start].to_owned(), hit: false });
        }
        spans.push(HighlightSpan { text: text[start..end].to_owned(), hit: true });
        from = end;
    }
    if from < text.len() {
        spans.push(HighlightSpan { text: text[from..].to_owned(), hit: false });
    }
    spans
}

/// About 200 characters of `text` around the first match of `words`, with "…" where it is cut.
fn passage(text: &str, words: &[String]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let first = words.iter().filter_map(|word| lower.find(&word.to_ascii_lowercase())).min()?;
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower_line = one_line.to_ascii_lowercase();
    let at = words.iter().filter_map(|word| lower_line.find(&word.to_ascii_lowercase())).min().unwrap_or(first);
    let start = one_line[..at].char_indices().rev().nth(60).map_or(0, |(i, _)| i);
    let end = one_line[at..].char_indices().nth(140).map_or(one_line.len(), |(i, _)| at + i);
    let mut shown = one_line[start..end].to_owned();
    if start > 0 {
        shown.insert(0, '…');
    }
    if end < one_line.len() {
        shown.push('…');
    }
    Some(shown)
}

/// A query as the demo reads it: the words to find, the words to leave out, and the operators it
/// knows (`from:`, `to:`, `subject:`, `in:`, `has:attachment`, `is:unread|read|flagged|unflagged`).
#[derive(Debug, Default)]
struct DemoQuery {
    words: Vec<String>,
    without: Vec<String>,
    from: Vec<String>,
    to: Vec<String>,
    subject: Vec<String>,
    folder: Option<String>,
    attachment: bool,
    flags: Vec<(Flag, bool)>,
}

impl DemoQuery {
    fn parse(query: &str) -> DemoQuery {
        let mut parsed = DemoQuery::default();
        for token in query.split_whitespace() {
            let token = token.trim_matches('"').to_lowercase();
            if token == "or" || token.is_empty() {
                continue;
            }
            if let Some(word) = token.strip_prefix('-') {
                parsed.without.push(word.to_owned());
                continue;
            }
            match token.split_once(':') {
                Some(("from", who)) => parsed.from.push(who.to_owned()),
                Some(("to", who)) => parsed.to.push(who.to_owned()),
                Some(("subject", word)) => parsed.subject.push(word.to_owned()),
                Some(("in", folder)) => parsed.folder = Some(folder.to_owned()),
                Some(("has", "attachment")) => parsed.attachment = true,
                Some(("is", "unread")) => parsed.flags.push((Flag::Seen, false)),
                Some(("is", "read")) => parsed.flags.push((Flag::Seen, true)),
                Some(("is", "flagged")) => parsed.flags.push((Flag::Flagged, true)),
                Some(("is", "unflagged")) => parsed.flags.push((Flag::Flagged, false)),
                // Dates and the rest the demo does not read: they find everything.
                Some(_) => {}
                None => parsed.words.push(token),
            }
        }
        parsed
    }

    /// Whether the message with `summary`, its `text` and in `folder` matches.
    fn matches(&self, summary: &MessageSummary, text: &str, folder: &Folder) -> bool {
        let who = |address: Option<&MailAddress>| {
            address
                .map_or_else(String::new, |who| format!("{} {}", who.name.as_deref().unwrap_or_default(), who.address))
        };
        let from = who(summary.from.as_ref()).to_lowercase();
        let to = summary.to.iter().map(|who| format!("{} {}", who.name.as_deref().unwrap_or_default(), who.address));
        let to = to.collect::<Vec<_>>().join(" ").to_lowercase();
        let subject = summary.subject.to_lowercase();
        let all = format!("{subject} {from} {to} {}", text.to_lowercase());
        self.words.iter().all(|word| all.contains(word))
            && !self.without.iter().any(|word| all.contains(word))
            && self.from.iter().all(|who| from.contains(who))
            && self.to.iter().all(|who| to.contains(who))
            && self.subject.iter().all(|word| subject.contains(word))
            && self.folder.as_ref().is_none_or(|wanted| {
                folder_word(folder).trim_matches('"').eq_ignore_ascii_case(wanted)
                    || folder.name.eq_ignore_ascii_case(wanted)
            })
            && (!self.attachment || summary.has_attachments)
            && self.flags.iter().all(|(flag, set)| summary.flags.contains(flag) == *set)
    }

    /// How well `summary` and `text` match: words in the subject count three times.
    fn score(&self, summary: &MessageSummary, text: &str) -> usize {
        let (subject, text) = (summary.subject.to_lowercase(), text.to_lowercase());
        self.words
            .iter()
            .map(|word| 3 * subject.matches(word.as_str()).count() + text.matches(word.as_str()).count())
            .sum()
    }
}

/// The account the demo still indexes, so the line on missing matches can be seen.
const DEMO_INDEXING: &str = "mail:anna@example.org";

/// `mail.index_status` of the demo: the work account complete, the private one still indexing.
pub fn demo_index(source: &DemoSource) -> Result<Vec<MailIndexProgress>, InboxError> {
    let folders = source.mail_folders()?;
    let mut accounts: Vec<String> = Vec::new();
    for folder in &folders {
        if !accounts.contains(&folder.account) {
            accounts.push(folder.account.clone());
        }
    }
    Ok(accounts
        .into_iter()
        .map(|account| {
            let messages: u64 = folders.iter().filter(|f| f.account == account).map(|f| u64::from(f.total)).sum();
            if account == DEMO_INDEXING {
                // An older mailbox behind it, as a real one would have.
                MailIndexProgress { account, messages: 1800, indexed: 1240, state: IndexState::Indexing }
            } else {
                MailIndexProgress { account, messages, indexed: messages, state: IndexState::Complete }
            }
        })
        .collect())
}

/// `mail.search_messages` of the demo (issue #136): the sample mail of the folders in scope that
/// matches the words and the operators the demo reads ([`DemoQuery`]), best first or by date,
/// a page from `cursor` on, highlighted; incomplete while the scope holds the account the demo
/// still indexes.
pub fn demo_find(source: &DemoSource, find: MailFind) -> Result<MailFound, InboxError> {
    let query = DemoQuery::parse(&find.query);
    let folders: Vec<Folder> = source
        .mail_folders()?
        .into_iter()
        .filter(|folder| find.account.as_ref().is_none_or(|account| folder.account == *account))
        .filter(|folder| find.folder.as_ref().is_none_or(|id| folder.id == *id))
        .collect();
    let mut found: Vec<(MessageSummary, String, usize)> = Vec::new();
    for folder in &folders {
        let (messages, _) = source.search_mail(&folder.id, None, u32::MAX, None, find.order)?;
        for summary in messages {
            let text = source.get_mail(&summary.id).ok().and_then(|mail| mail.text).unwrap_or_default();
            if query.matches(&summary, &text, folder) {
                let score = query.score(&summary, &text);
                found.push((summary, text, score));
            }
        }
    }
    found.sort_by(|a, b| find.order.apply(b.0.date.cmp(&a.0.date)));
    if find.sort == MailSort::Relevance {
        found.sort_by_key(|entry| std::cmp::Reverse(entry.2));
    }
    let start = match &find.cursor {
        None => 0,
        Some(cursor) => cursor.parse::<usize>().map_err(|_| InboxError::Other("not a cursor".into()))?,
    };
    let limit = usize::try_from(find.limit).unwrap_or(usize::MAX);
    let total = found.len();
    let page: Vec<(MessageSummary, String, usize)> = found.into_iter().skip(start).take(limit).collect();
    let cursor = (start + page.len() < total).then(|| (start + page.len()).to_string());
    let words: Vec<String> = query.words.iter().chain(&query.subject).cloned().collect();
    let highlights = page
        .iter()
        .map(|(summary, text, _)| MailHighlight {
            id: summary.id.clone(),
            subject: if words.iter().any(|word| summary.subject.to_lowercase().contains(word)) {
                spans_of(&summary.subject, &words)
            } else {
                Vec::new()
            },
            snippet: passage(text, &words).map(|passage| spans_of(&passage, &words)).unwrap_or_default(),
        })
        .filter(|highlight| !highlight.subject.is_empty() || !highlight.snippet.is_empty())
        .collect();
    let complete = !folders.iter().any(|folder| folder.account == DEMO_INDEXING);
    Ok(MailFound {
        messages: page.into_iter().map(|(summary, _, _)| summary).collect(),
        cursor,
        complete,
        highlights,
        total: find.cursor.is_none().then_some(total as u64),
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn folder(id: &str, name: &str, role: Option<Role>) -> Folder {
        Folder {
            id: id.into(),
            account: "mail:anna@work.example.org".into(),
            name: name.into(),
            path: name.into(),
            role,
            unread: 0,
            total: 0,
        }
    }

    fn who(name: &str, address: &str) -> MailAddress {
        MailAddress { name: Some(name.into()), address: address.into() }
    }

    fn queries(found: &[Suggestion]) -> Vec<&str> {
        found.iter().map(|suggestion| suggestion.query.as_str()).collect()
    }

    #[test]
    fn a_suggestion_completes_the_word_being_typed() {
        assert_eq!(complete("budget fr", "from:"), "budget from:");
        assert_eq!(complete("budget from:jo", "from:jonas@example.org"), "budget from:jonas@example.org ");
        assert_eq!(complete("", "has:attachment"), "has:attachment ");
        assert_eq!(complete("budget ", "in:inbox"), "budget in:inbox ");
    }

    #[test]
    fn suggestions_follow_what_is_typed() {
        let people =
            [who("Jonas Weber", "jonas.weber@work.example.org"), who("Lena Hoffmann", "lena@work.example.org")];
        let folders = [
            folder("f:1", "Inbox", Some(Role::Inbox)),
            folder("f:2", "Projects", None),
            folder("f:3", "Old stuff", None),
        ];
        let recent = ["budget review".to_owned(), "lunch".to_owned()];
        // Nothing typed: the searches last made.
        assert_eq!(queries(&suggestions("", &recent, &people, &folders)), ["budget review", "lunch"]);
        // An operator being typed.
        assert_eq!(
            queries(&suggestions("from:jo", &recent, &people, &folders)),
            ["from:jonas.weber@work.example.org "]
        );
        assert_eq!(queries(&suggestions("x in:pro", &recent, &people, &folders)), ["x in:Projects "]);
        assert_eq!(queries(&suggestions("in:old", &recent, &people, &folders)), ["in:\"Old stuff\" "]);
        assert_eq!(queries(&suggestions("in:inb", &recent, &people, &folders)), ["in:inbox "], "a role by its word");
        assert_eq!(queries(&suggestions("is:un", &recent, &people, &folders)), ["is:unread ", "is:unflagged "]);
        assert_eq!(queries(&suggestions("has:", &recent, &people, &folders)), ["has:attachment "]);
        assert!(suggestions("before:20", &recent, &people, &folders).is_empty());
        // A word: recent searches, operators, people, folders.
        assert_eq!(queries(&suggestions("bud", &recent, &people, &folders)), ["budget review"]);
        assert_eq!(queries(&suggestions("f", &recent, &people, &folders)), ["from:", "filename:"]);
        let lena = suggestions("budget lena", &recent, &people, &folders);
        assert_eq!(queries(&lena), ["budget from:lena@work.example.org "]);
        assert_eq!(lena[0].kind, SuggestionKind::Person);
        assert!(suggestions("budget ", &recent, &people, &folders).is_empty(), "nothing after a space");
        let many: Vec<MailAddress> = (0..20).map(|n| who("Anna", &format!("anna{n}@example.org"))).collect();
        assert_eq!(suggestions("anna", &recent, &many, &folders).len(), SUGGESTIONS_MAX);
    }

    #[test]
    fn recent_searches_put_the_last_first_once() {
        let recent = vec!["lunch".to_owned(), "Budget".to_owned()];
        assert_eq!(remember(&recent, " budget "), ["budget", "lunch"]);
        assert_eq!(remember(&recent, "   "), recent, "a blank search is not kept");
        let many: Vec<String> = (0..SEARCHES_MAX).map(|n| n.to_string()).collect();
        assert_eq!(remember(&many, "new").len(), SEARCHES_MAX);
    }

    #[test]
    fn people_are_listed_once() {
        let message = |from: MailAddress, to: Vec<MailAddress>| MessageSummary {
            id: "m".into(),
            account: "mail:anna@work.example.org".into(),
            folder: "f:1".into(),
            thread_id: "t".into(),
            from: Some(from),
            to,
            subject: String::new(),
            date: Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).single().expect("a time"),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            invite: None,
            back: None,
            untrusted: true,
        };
        let jonas = who("Jonas", "jonas@example.org");
        let messages = [
            message(jonas.clone(), vec![who("Anna", "anna@example.org")]),
            message(who("JONAS", "Jonas@Example.org"), vec![jonas]),
        ];
        let found: Vec<String> = people(&messages).into_iter().map(|who| who.address).collect();
        assert_eq!(found, ["jonas@example.org", "anna@example.org"]);
    }

    #[test]
    fn the_scope_goes_into_the_request() {
        let inbox = FolderView::of(&folder("f:1", "Inbox", Some(Role::Inbox)));
        let all = SearchView::new(" budget ", Scope::All, Sort::Relevance).find(Order::NewestFirst, PAGE, None);
        assert_eq!((all.account, all.folder, all.query.as_str()), (None, None, "budget"));
        let account = Scope::Account("mail:anna@work.example.org".into());
        let one = SearchView::new("budget", account, Sort::Date).find(Order::OldestFirst, PAGE, Some("2".into()));
        assert_eq!(
            (one.account.as_deref(), one.folder, one.order),
            (Some("mail:anna@work.example.org"), None, Order::OldestFirst)
        );
        let here = SearchView::new("budget", Scope::Folder(inbox), Sort::Date).find(Order::NewestFirst, PAGE, None);
        assert_eq!(
            (here.account.as_deref(), here.folder.as_deref()),
            (Some("mail:anna@work.example.org"), Some("f:1"))
        );
    }

    #[test]
    fn the_counts_and_the_missing_matches_are_said_honestly() {
        let complete = Found { complete: true, total: Some(12), index: Vec::new() };
        assert_eq!(count_text(12, false, &complete), trf("{n} results", &[("n", &12)]));
        assert_eq!(count_text(1, false, &Found { total: Some(1), ..complete.clone() }), tr("1 result"));
        assert_eq!(
            count_text(50, true, &Found { total: None, ..complete.clone() }),
            trf("More than {n} results", &[("n", &50)])
        );
        assert_eq!(missing_text(&complete), None);
        let progress =
            |state, indexed| MailIndexProgress { account: "mail:a@x.org".into(), messages: 1800, indexed, state };
        let indexing = Found { complete: false, total: Some(40), index: vec![progress(IndexState::Indexing, 1240)] };
        assert_eq!(count_text(40, false, &indexing), trf("At least {n} results", &[("n", &40)]));
        let counts = [("indexed", &1240_u64 as &dyn std::fmt::Display), ("total", &1800_u64)];
        assert_eq!(
            missing_text(&indexing),
            Some(trf(
                "Some matches may be missing: older mail is being indexed, {indexed} of {total} messages.",
                &counts
            ))
        );
        let waiting = Found { index: vec![progress(IndexState::Waiting, 1240)], ..indexing.clone() };
        assert!(missing_text(&waiting).is_some_and(|text| text.contains("1240")));
        let unknown = Found { index: Vec::new(), ..indexing };
        assert_eq!(
            missing_text(&unknown),
            Some(tr("Some matches may be missing: older mail is still being indexed.").to_owned())
        );
    }

    #[test]
    fn highlights_cut_the_text_at_the_matches() {
        let spans = spans_of("Budget review for Q4 budget", &["budget".to_owned()]);
        let hits: Vec<(&str, bool)> = spans.iter().map(|span| (span.text.as_str(), span.hit)).collect();
        assert_eq!(hits, [("Budget", true), (" review for Q4 ", false), ("budget", true)]);
        assert_eq!(spans_of("nothing", &["budget".to_owned()]).len(), 1);
        let long = format!("{} budget {}", "a ".repeat(100), "b ".repeat(100));
        let shown = passage(&long, &["budget".to_owned()]).expect("a passage");
        assert!(shown.starts_with('…') && shown.ends_with('…') && shown.contains("budget"), "{shown}");
    }

    #[test]
    fn the_demo_searches_every_folder_with_operators_and_says_it_is_indexing() {
        let now = Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("a time");
        let source = DemoSource::new(now);
        let find = |query: &str, account: Option<&str>| MailFind {
            account: account.map(str::to_owned),
            folder: None,
            query: query.into(),
            sort: MailSort::Relevance,
            order: Order::NewestFirst,
            limit: PAGE,
            cursor: None,
        };
        let found = demo_find(&source, find("budget", None)).expect("found");
        assert!(!found.messages.is_empty(), "the budget mail");
        assert!(!found.complete, "one account still indexes");
        assert_eq!(found.total, Some(found.messages.len() as u64));
        assert!(found.highlights.iter().any(|h| h.subject.iter().any(|span| span.hit)), "highlighted");
        let work = demo_find(&source, find("budget", Some("mail:anna@work.example.org"))).expect("found");
        assert!(work.complete, "the work account is indexed");
        let attachments = demo_find(&source, find("has:attachment", None)).expect("found");
        assert!(attachments.messages.iter().all(|message| message.has_attachments));
        assert!(demo_find(&source, find("budget -budget", None)).expect("found").messages.is_empty());
        let index = demo_index(&source).expect("the index");
        assert!(index.iter().any(|account| account.state == IndexState::Indexing));
    }
}
