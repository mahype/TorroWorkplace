//! Settings › Filters in the mail window (issue #339): the mail filters, each a saved search with
//! what to do with the mail it finds in the inbox – move it, mark it as read, file it under a
//! category.
//!
//! ```text
//! Filters   one row per filter, in its order: the search, then what it does   [switch] ›
//!           + New filter
//! ```
//!
//! A filter's page: the search (the language of the search field: `from:`, `to:`, `subject:`,
//! `has:attachment` …) and the account it runs in ("All accounts"), what it finds in the inbox now
//! (`mail.preview_filter`), the actions – "Move" (Don't move, Archive, Folder, Trash) with the
//! folder of the chosen account, "Mark as read", "Category" –, the switch and, for a stored one,
//! "Delete filter" after a question. "Save" stands in the header bar. Saved with mail in the inbox
//! that it finds, the list asks "Apply to the {n} mails in Inbox?" and, on "Apply", runs it once
//! (`mail.apply_filter`), again with "Apply to more" while the core says more are left.
//!
//! Everything is a request (`mail.list_filters`, `mail.create_filter`, `mail.update_filter`,
//! `mail.remove_filter`, `mail.preview_filter`, `mail.apply_filter`); the page checks only what a
//! person must fix before saving, and the core checks all of it again. Its refusals show as a
//! message: on the page for the preview and "Apply", over the page (the host's notice) for a save.
//!
//! The torroinbox window opens a page already filled in, with [`Message::New`]: "Create filter…"
//! at a mail (`from:<address>`) and "Create filter from search…" at the search field.

use chrono::Utc;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, space as blank};
use iced::{Alignment, Element, Theme};
use torro_core::i18n::{tr, trc, trf};
use torro_gui::datetime::format_moment;
use torro_gui::icon::Icon;
use torro_gui::theme::{palette, space};
use torro_gui::widgets::PageAction;
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{Request, Response};
use torro_mail::{Category, CategoryGroup, FILTER_QUERY_MAX, FilterFile, MailFilter, MessageSummary, Role};

use crate::parts::{chevron, field, notice, pressable, row_frame, setting_row, switch};
use crate::{Action, Data, Page};

/// The id of the search field of a filter's page: "Save" with a mistake in the search goes there.
pub const QUERY_FIELD: &str = "mail-filter-query";

/// What the section keeps between two frames: the page that is open, its question, what the open
/// search finds, and the question or the result of "Apply" on the list.
#[derive(Debug, Default)]
pub struct State {
    form: Option<Form>,
    asking: Option<Ask>,
    preview: Preview,
    apply: Option<Apply>,
}

impl State {
    /// A filter's page is open: the section shows it instead of its list.
    pub fn is_open(&self) -> bool {
        self.form.is_some()
    }
}

/// What the page asks before it does something that loses what was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Leave the page and drop what was typed.
    Discard,
    /// Delete the open filter.
    Delete,
}

/// Where a filter moves the mail it finds, as the page offers it ("Move").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// "Don't move": the mail stays in the inbox.
    Stay,
    Archive,
    /// A folder of the filter's account, chosen in the row under it.
    Folder,
    Trash,
}

impl Move {
    const ALL: [Move; 4] = [Move::Stay, Move::Archive, Move::Folder, Move::Trash];

    fn label(self) -> &'static str {
        match self {
            Move::Stay => tr("Don’t move"),
            Move::Archive => trc("action", "Archive"),
            Move::Folder => tr("Folder"),
            Move::Trash => tr("Trash"),
        }
    }
}

/// A filter as it is being typed: a new one (`id: None`) or one that is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Form {
    id: Option<String>,
    query: String,
    account: Option<String>,
    moving: Move,
    /// The folder of [`Move::Folder`] (`f:…`); kept while another move is chosen, sent only with it.
    folder: Option<String>,
    mark_read: bool,
    category: Option<Category>,
    enabled: bool,
    /// "Save" was pressed once: mistakes show from now on.
    tried: bool,
}

/// What a search finds in the inbox now, as the core answered `mail.preview_filter`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    count: u64,
    messages: Vec<MessageSummary>,
}

/// The previews asked for the open page. Answers carry no search, so only the answer that comes
/// when nothing is on its way any more counts, for the search asked last.
#[derive(Debug, Default)]
struct Preview {
    /// Asks on their way.
    waiting: u32,
    /// The search and account asked for last.
    asked: Option<(String, Option<String>)>,
    /// The last answer.
    found: Option<Result<Found, String>>,
}

/// The question and the result of "Apply" on the list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Apply {
    /// A filter was saved that finds `count` mails in the inbox now: run it on them?
    Ask { target: Target, count: u64 },
    /// "Apply" was pressed; the answer is on its way.
    Running { id: String },
    /// What the run did; `more` offers another.
    Done { id: String, applied: u64, failed: u64, more: bool },
    /// The core refused, with its message.
    Failed(String),
}

/// The filter a question of [`Apply`] is about. A new filter has no id until the next load
/// brings it: it is the one with that search and account that was not there before.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Stored(String),
    New { query: String, account: Option<String>, known: Vec<String> },
}

impl Target {
    /// The id of the filter in `data`; `None` until it is loaded.
    fn id(&self, data: &Data) -> Option<String> {
        match self {
            Target::Stored(id) => data.mail_filters.iter().any(|filter| &filter.id == id).then(|| id.clone()),
            Target::New { query, account, known } => data
                .mail_filters
                .iter()
                .find(|filter| !known.contains(&filter.id) && &filter.query == query && &filter.account == account)
                .map(|filter| filter.id.clone()),
        }
    }
}

/// What the section's rows and pages report.
#[derive(Debug, Clone)]
pub enum Message {
    /// Open a new filter's page with this search and account – "New filter" (empty) on the list,
    /// "Create filter…" at a mail (`from:<address>`, every account) and "Create filter from
    /// search…" at the search field of torroinbox (the search, with the account where the search
    /// looks at "This account"). Asks at once what the search finds.
    New {
        query: String,
        account: Option<String>,
    },
    /// A filter's row: open its page.
    Edit(String),
    /// The switch of a filter's row: stored at once.
    Enabled(String, bool),
    Query(String),
    /// The account the filter runs in; `None`: every account.
    Account(Option<String>),
    Move(Move),
    /// The folder of "Move" › "Folder" (`f:…`).
    Folder(String),
    MarkRead(bool),
    Category(Option<Category>),
    /// "Filter is on" on the page.
    On(bool),
    Save,
    /// "Delete filter" – after the question.
    Delete,
    /// The answer to the question the page asks: go ahead (`true`) or not.
    Answer(bool),
    /// "‹ Back" from a page to the list, and Escape: asks first when something typed would go.
    Back,
    /// "Apply" or "Apply to more" on the list: run the saved filter once on the inbox.
    Apply,
    /// "Not now" or "Done": the question or the result of "Apply" goes.
    Dismiss,
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::New { query, account } => {
            open(state, Form::new(query, account));
            ask_preview(state)
        }
        Message::Edit(id) => {
            let Some(filter) = data.mail_filters.iter().find(|filter| filter.id == id) else { return Action::None };
            open(state, Form::of(filter));
            ask_preview(state)
        }
        Message::Enabled(id, on) => match data.mail_filters.iter().find(|filter| filter.id == id) {
            Some(filter) => Action::send(switched(filter, on)),
            None => Action::None,
        },
        Message::Back => {
            // A question open: Escape and "‹ Back" take it back first.
            if state.asking.take().is_some() {
                return Action::None;
            }
            if state.form.as_ref().is_some_and(|form| form.dirty(data)) {
                state.asking = Some(Ask::Discard);
                return Action::None;
            }
            state.form = None;
            Action::None
        }
        Message::Delete => {
            state.asking = Some(Ask::Delete);
            Action::None
        }
        Message::Answer(go) => {
            let Some(asked) = state.asking.take() else { return Action::None };
            if !go {
                return Action::None;
            }
            let action = match (asked, &state.form) {
                (Ask::Delete, Some(Form { id: Some(id), .. })) => {
                    Action::send(Request::RemoveMailFilter { id: id.clone() })
                }
                (Ask::Delete, _) => return Action::None,
                (Ask::Discard, _) => Action::None,
            };
            state.form = None;
            action
        }
        Message::Save => save(state, data),
        Message::Apply => {
            let id = match &state.apply {
                Some(Apply::Ask { target, .. }) => target.id(data),
                Some(Apply::Done { id, more: true, .. }) => Some(id.clone()),
                _ => None,
            };
            let Some(id) = id else { return Action::None };
            state.apply = Some(Apply::Running { id: id.clone() });
            Action::Ask(Request::ApplyMailFilter { id })
        }
        Message::Dismiss => {
            state.apply = None;
            Action::None
        }
        message => {
            let Some(form) = state.form.as_mut() else { return Action::None };
            let ask = match message {
                Message::Query(query) => {
                    form.query = query;
                    true
                }
                Message::Account(account) => {
                    // A folder belongs to one account: another account starts without one.
                    if form.account != account {
                        form.folder = None;
                    }
                    form.account = account;
                    true
                }
                Message::Move(moving) => {
                    form.moving = moving;
                    false
                }
                Message::Folder(folder) => {
                    form.folder = Some(folder);
                    false
                }
                Message::MarkRead(on) => {
                    form.mark_read = on;
                    false
                }
                Message::Category(category) => {
                    form.category = category;
                    false
                }
                Message::On(on) => {
                    form.enabled = on;
                    false
                }
                _ => false,
            };
            if ask { ask_preview(state) } else { Action::None }
        }
    }
}

/// What Escape means while a filter's page is open: the same as "‹ Back". `None` on the list –
/// then the settings close as before.
pub fn escape(state: &State) -> Option<Message> {
    state.is_open().then_some(Message::Back)
}

/// Takes an answer of a preview or of "Apply" (the host's [`crate::Outcome::Answer`]); `true` when
/// it was this section's, so no other section takes it. A failure is this section's only while
/// one of its asks is on its way.
pub fn answered(state: &mut State, answer: &Result<Response, String>) -> bool {
    match answer {
        Ok(Response::MailFilterPreview { count, messages }) => {
            state.preview.waiting = state.preview.waiting.saturating_sub(1);
            state.preview.found = Some(Ok(Found { count: *count, messages: messages.clone() }));
            true
        }
        Ok(Response::MailFilterApplied { applied, failed, more }) => {
            if let Some(Apply::Running { id }) = &state.apply {
                let id = id.clone();
                state.apply = Some(Apply::Done { id, applied: *applied, failed: *failed, more: *more });
            }
            true
        }
        // "Apply" runs from the list, previews from a page: a failure while no page is open is
        // the run's.
        Err(problem) if state.form.is_none() && matches!(state.apply, Some(Apply::Running { .. })) => {
            state.apply = Some(Apply::Failed(problem.clone()));
            true
        }
        Err(problem) if state.preview.waiting > 0 => {
            state.preview.waiting -= 1;
            state.preview.found = Some(Err(problem.clone()));
            true
        }
        _ => false,
    }
}

/// Opens `form` as the page, without a question and without an answer of an earlier page.
fn open(state: &mut State, form: Form) {
    state.asking = None;
    state.apply = None;
    state.preview.asked = None;
    state.preview.found = None;
    state.form = Some(form);
}

/// `mail.preview_filter` for the open page's search and account; nothing for an empty or too long
/// search, which the page names as a mistake instead.
fn ask_preview<A>(state: &mut State) -> Action<A> {
    let Some(form) = &state.form else { return Action::None };
    let query = form.query.trim();
    if query.is_empty() || query.chars().count() > FILTER_QUERY_MAX {
        state.preview.asked = None;
        return Action::None;
    }
    state.preview.waiting += 1;
    state.preview.asked = Some((query.to_string(), form.account.clone()));
    Action::Ask(Request::PreviewMailFilter { query: query.to_string(), account: form.account.clone() })
}

/// "Save": the mistakes once pressed, else the request, and the question whether to apply the
/// filter to what it finds now. The page closes; the list shows the stored filter.
fn save<A>(state: &mut State, data: &Data) -> Action<A> {
    let hits = state.form.as_ref().map(|form| hits(&state.preview, form));
    let Some(form) = state.form.as_mut() else { return Action::None };
    form.tried = true;
    let failed = match &hits {
        Some(Hits::Failed(problem)) => Some(problem.to_string()),
        _ => None,
    };
    if form.query_mistake(failed.as_deref()).is_some() {
        return Action::Focus(QUERY_FIELD);
    }
    if form.action_mistake().is_some() {
        return Action::None;
    }
    let request = form.request();
    let count = match hits {
        Some(Hits::Found(found)) if found.count > 0 => Some(found.count),
        _ => None,
    };
    let target = match &form.id {
        Some(id) => Target::Stored(id.clone()),
        None => Target::New {
            query: form.query.trim().to_string(),
            account: form.account.clone(),
            known: data.mail_filters.iter().map(|filter| filter.id.clone()).collect(),
        },
    };
    state.apply = count.map(|count| Apply::Ask { target, count });
    state.form = None;
    Action::send(request)
}

/// `mail.update_filter` that switches `filter` on or off and keeps everything else.
fn switched(filter: &MailFilter, on: bool) -> Request {
    Request::UpdateMailFilter {
        id: filter.id.clone(),
        query: filter.query.clone(),
        account: filter.account.clone(),
        file: filter.file.clone(),
        mark_read: filter.mark_read,
        category: filter.category,
        enabled: Some(on),
    }
}

impl Form {
    fn new(query: String, account: Option<String>) -> Form {
        Form {
            id: None,
            query,
            account,
            moving: Move::Stay,
            folder: None,
            mark_read: false,
            category: None,
            enabled: true,
            tried: false,
        }
    }

    fn of(filter: &MailFilter) -> Form {
        let (moving, folder) = match &filter.file {
            None => (Move::Stay, None),
            Some(FilterFile::Archive) => (Move::Archive, None),
            Some(FilterFile::Trash) => (Move::Trash, None),
            Some(FilterFile::Folder { id }) => (Move::Folder, Some(id.clone())),
        };
        Form {
            id: Some(filter.id.clone()),
            query: filter.query.clone(),
            account: filter.account.clone(),
            moving,
            folder,
            mark_read: filter.mark_read,
            category: filter.category,
            enabled: filter.enabled,
            tried: false,
        }
    }

    /// The move as the core takes it; a folder only with [`Move::Folder`].
    fn file(&self) -> Option<FilterFile> {
        match self.moving {
            Move::Stay => None,
            Move::Archive => Some(FilterFile::Archive),
            Move::Trash => Some(FilterFile::Trash),
            Move::Folder => self.folder.clone().map(|id| FilterFile::Folder { id }),
        }
    }

    /// Something typed on the page is not saved yet.
    fn dirty(&self, data: &Data) -> bool {
        match &self.id {
            None => !self.query.trim().is_empty() || self.has_action(),
            Some(id) => {
                data.mail_filters.iter().find(|filter| &filter.id == id).is_some_and(|stored| !self.same_as(stored))
            }
        }
    }

    /// The page holds what `stored` holds: nothing to save.
    fn same_as(&self, stored: &MailFilter) -> bool {
        let mut page = Form { tried: false, ..self.clone() };
        let held = Form::of(stored);
        // A folder kept for another move is not sent, so it is no change.
        if page.moving != Move::Folder {
            page.folder = held.folder.clone();
        }
        page.query = page.query.trim().to_string();
        page == held
    }

    fn has_action(&self) -> bool {
        self.moving != Move::Stay || self.mark_read || self.category.is_some()
    }

    /// What is wrong with the search, if anything; `failed` is the core's word on it (the preview
    /// refused it: `in:`, `filename:`, a search it cannot read).
    fn query_mistake(&self, failed: Option<&str>) -> Option<String> {
        let query = self.query.trim();
        if query.is_empty() {
            return Some(tr("Type what the filter looks for.").to_string());
        }
        if query.chars().count() > FILTER_QUERY_MAX {
            let most = FILTER_QUERY_MAX.to_string();
            return Some(trf("A filter’s search has at most {most} characters.", &[("most", &most)]));
        }
        failed.map(str::to_string)
    }

    /// What is wrong with the actions, if anything.
    fn action_mistake(&self) -> Option<String> {
        if !self.has_action() {
            return Some(tr("Choose at least one thing the filter does.").to_string());
        }
        if self.moving == Move::Folder {
            if self.account.is_none() {
                return Some(tr("Choose an account to move mail into one of its folders.").to_string());
            }
            if self.folder.is_none() {
                return Some(tr("Choose the folder the mail goes to.").to_string());
            }
        }
        None
    }

    /// The request that saves the page.
    fn request(&self) -> Request {
        let query = self.query.trim().to_string();
        match &self.id {
            None => Request::CreateMailFilter {
                query,
                account: self.account.clone(),
                file: self.file(),
                mark_read: self.mark_read,
                category: self.category,
                enabled: self.enabled,
            },
            Some(id) => Request::UpdateMailFilter {
                id: id.clone(),
                query,
                account: self.account.clone(),
                file: self.file(),
                mark_read: self.mark_read,
                category: self.category,
                enabled: Some(self.enabled),
            },
        }
    }
}

/// What the open page's search finds, as far as it is known.
enum Hits<'a> {
    /// Nothing typed: nothing asked.
    Empty,
    /// An answer is on its way, or the search changed since the last one.
    Counting,
    Found(&'a Found),
    /// The core refused the search, with its message.
    Failed(&'a str),
}

fn hits<'a>(preview: &'a Preview, form: &Form) -> Hits<'a> {
    let query = form.query.trim();
    if query.is_empty() {
        return Hits::Empty;
    }
    let current = preview.asked.as_ref().is_some_and(|(asked, account)| asked == query && *account == form.account);
    match (&preview.found, current && preview.waiting == 0) {
        (Some(Ok(found)), true) => Hits::Found(found),
        (Some(Err(problem)), true) => Hits::Failed(problem),
        _ => Hits::Counting,
    }
}

/// The section's page: the list, or the open filter's page with "‹ Back" and "Save".
pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
    let Some(form) = &state.form else { return Page::new(list(state, data)) };
    let name = match form.id {
        None => tr("New filter"),
        Some(_) => tr("Edit filter"),
    };
    let ready = match &form.id {
        None => true,
        Some(id) => data.mail_filters.iter().find(|filter| &filter.id == id).is_none_or(|stored| !form.same_as(stored)),
    };
    let look: widgets::ButtonStyle = if ready { style::button::primary } else { style::button::secondary };
    let save = widgets::text_button(None, tr("Save"), look, ready.then_some(Message::Save));
    let body = filter_page(form, &state.preview, data);
    // The question stands above the page, where it is seen.
    let body = match state.asking {
        Some(asked) => column![question(asked), body].spacing(layout::section_gap()).into(),
        None => body,
    };
    Page::new(body).sub_page(name, Message::Back).actions(vec![save])
}

/// The list: the question or the result of "Apply" on top, then the filters and "New filter".
fn list<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut page = column![].spacing(layout::section_gap());
    if let Some(apply) = &state.apply {
        page = page.push(apply_block(apply, data));
    }
    let mut rows: Vec<Element<'a, Message>> = data
        .mail_filters
        .iter()
        .map(|filter| {
            let id = filter.id.clone();
            let control = row![switch(filter.enabled, move |on| Message::Enabled(id.clone(), on)), chevron()]
                .spacing(space::l())
                .align_y(Alignment::Center);
            let row = setting_row(None, filter.query.clone(), Some(summary(filter, data)), control);
            pressable(row, Some(Message::Edit(filter.id.clone())))
        })
        .collect();
    rows.push(add_row());
    page.push(widgets::group(
        tr("Filters"),
        rows,
        Some(
            tr("Filters look at new mail in Inbox while Torro Workplace runs. Every filter that fits runs; for moving and the category the first one decides.")
                .to_string(),
        ),
    ))
    .into()
}

/// What a filter does, in a line: "anna@mailbox.org · Archive · Mark as read"; "Off · …" while it
/// is switched off. A category that is no longer in the catalogue reads "Unknown category".
pub fn summary(filter: &MailFilter, data: &Data) -> String {
    let mut parts = Vec::new();
    if let Some(account) = &filter.account {
        parts.push(account_label(data, account));
    }
    match &filter.file {
        None => {}
        Some(FilterFile::Archive) => parts.push(trc("action", "Archive").to_string()),
        Some(FilterFile::Trash) => parts.push(tr("Move to Trash").to_string()),
        Some(FilterFile::Folder { id }) => parts.push(trf("Move to {folder}", &[("folder", &folder_name(data, id))])),
    }
    if filter.mark_read {
        parts.push(tr("Mark as read").to_string());
    }
    if let Some(category) = filter.category {
        parts.push(match data.categories.iter().find(|info| info.category == category) {
            Some(info) => trf("File as {category}", &[("category", &info.label)]),
            None => tr("Unknown category").to_string(),
        });
    }
    let line = parts.join(" · ");
    if filter.enabled { line } else { trf("Off · {rule}", &[("rule", &line)]) }
}

/// The name of a mail account (`mail:<address>`): its label, else the address.
fn account_label(data: &Data, id: &str) -> String {
    data.accounts
        .iter()
        .find(|account| account.id == id)
        .map(|account| account.label.clone())
        .unwrap_or_else(|| id.trim_start_matches("mail:").to_string())
}

/// The name of a folder (`f:…`), "Unknown folder" for one the load does not know.
fn folder_name(data: &Data, id: &str) -> String {
    data.mail_folders
        .iter()
        .find(|folder| folder.id == id)
        .map_or_else(|| tr("Unknown folder").to_string(), |folder| folder.name.clone())
}

/// The last row of the list: "+ New filter".
fn add_row<'a>() -> Element<'a, Message> {
    let plus = torro_gui::icon::icon(Icon::Plus, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let row = setting_row(Some(plus.into()), tr("New filter"), None, blank());
    pressable(row, Some(Message::New { query: String::new(), account: None }))
}

/// The question after saving a filter that finds mail now, or what "Apply" did.
fn apply_block<'a>(apply: &'a Apply, data: &'a Data) -> Element<'a, Message> {
    match apply {
        Apply::Ask { target, count } => {
            let title = match count {
                1 => tr("Apply to the 1 mail in Inbox?").to_string(),
                n => trf("Apply to the {n} mails in Inbox?", &[("n", n)]),
            };
            let actions = vec![
                PageAction::new(tr("Not now"), Some(Message::Dismiss)).style(style::button::ghost),
                // Until the next load brings the new filter, there is nothing to apply yet.
                PageAction::new(tr("Apply"), target.id(data).map(|_| Message::Apply)),
            ];
            widgets::question_card(
                Icon::Filter,
                title,
                tr("From now on the filter runs on new mail. Mail already in Inbox changes only if you apply it."),
                actions,
            )
        }
        Apply::Running { .. } => {
            widgets::group(tr("Apply filter"), vec![setting_row(None, tr("Applying the filter…"), None, blank())], None)
        }
        Apply::Done { applied, failed, more, .. } => {
            let words = match applied {
                1 => tr("Applied to 1 mail.").to_string(),
                n => trf("Applied to {n} mails.", &[("n", n)]),
            };
            let detail =
                (*failed > 0).then(|| trf("{n} could not be changed and stay as they were.", &[("n", failed)]));
            let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
            if *more {
                buttons = buttons.push(widgets::text_button(
                    None,
                    tr("Apply to more"),
                    style::button::secondary,
                    Some(Message::Apply),
                ));
            }
            buttons =
                buttons.push(widgets::text_button(None, tr("Done"), style::button::ghost, Some(Message::Dismiss)));
            widgets::group(tr("Apply filter"), vec![setting_row(None, words, detail, buttons)], None)
        }
        Apply::Failed(problem) => {
            let done = widgets::text_button(None, tr("Done"), style::button::ghost, Some(Message::Dismiss));
            let row = row_frame(
                row![container(notice(problem.clone())).width(iced::Fill), done]
                    .spacing(space::l())
                    .align_y(Alignment::Center),
                false,
            );
            widgets::group(tr("Apply filter"), vec![row], None)
        }
    }
}

/// The question the page asks, with its two answers.
fn question<'a>(asked: Ask) -> Element<'a, Message> {
    let (title, detail, yes) = match asked {
        Ask::Discard => (tr("Discard changes?"), tr("What you typed on this page is not saved."), tr("Discard")),
        Ask::Delete => {
            (tr("Delete this filter?"), tr("Mail it moved or filed stays where it is."), tr("Delete filter"))
        }
    };
    let buttons = row![
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Answer(false))),
        widgets::text_button(None, yes, style::button::danger, Some(Message::Answer(true))),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::group(title, vec![setting_row(None, detail, None, buttons)], None)
}

/// A filter's page.
fn filter_page<'a>(form: &'a Form, preview: &'a Preview, data: &'a Data) -> Element<'a, Message> {
    let found = hits(preview, form);
    let failed = match &found {
        Hits::Failed(problem) => Some(*problem),
        _ => None,
    };
    let query = field("from:shop.example has:attachment", &form.query)
        .id(QUERY_FIELD)
        .on_input(Message::Query)
        .on_submit(Message::Save);
    let account = setting_row(None, tr("Account"), None, account_picker(form, data));
    let search_note = Some(
        tr("Written like a search: from:, to:, subject:, has:attachment … On new mail, plain words are looked for in the subject and the addresses.")
            .to_string(),
    );
    let mut page = column![
        checked_group(
            tr("Search"),
            vec![row_frame(query, false), account],
            note_or_mistake(form.tried, form.query_mistake(failed), search_note),
        ),
        widgets::group(tr("In Inbox now"), hit_rows(&found), None),
        checked_group(
            tr("Actions"),
            action_rows(form, data),
            note_or_mistake(
                form.tried,
                form.action_mistake(),
                Some(
                    tr("When several filters find a mail, the first one that moves it or sets a category decides.")
                        .to_string()
                ),
            ),
        ),
        widgets::group(
            tr("Filter"),
            vec![setting_row(None, tr("Filter is on"), None, switch(form.enabled, Message::On))],
            None
        ),
    ]
    .spacing(layout::section_gap());
    if form.id.is_some() {
        let delete =
            widgets::text_button(Some(Icon::Trash2), tr("Delete filter"), style::button::danger, Some(Message::Delete));
        let row = setting_row(None, tr("Delete this filter"), None, delete);
        page = page.push(widgets::group(tr("Delete"), vec![row], None));
    }
    page.into()
}

/// What the search finds now: a line with the number, then the newest of the mails.
fn hit_rows<'a>(found: &Hits<'_>) -> Vec<Element<'a, Message>> {
    let line = |words: String| setting_row(None, words, None, blank());
    match found {
        Hits::Empty => vec![line(tr("Type a search to see what it finds.").to_string())],
        Hits::Counting => vec![line(tr("Counting…").to_string())],
        Hits::Failed(problem) => vec![row_frame(notice(problem.to_string()), false)],
        Hits::Found(found) => {
            let words = match found.count {
                0 => tr("Finds no mail in Inbox now.").to_string(),
                1 => tr("Finds 1 mail in Inbox now.").to_string(),
                n => trf("Finds {n} mails in Inbox now.", &[("n", &n)]),
            };
            let now = Utc::now();
            let mut rows = vec![line(words)];
            rows.extend(found.messages.iter().map(|message| {
                let from = message.from.as_ref().map(|from| from.name.clone().unwrap_or_else(|| from.address.clone()));
                let when = format_moment(message.date, now);
                let detail = match from {
                    Some(from) => format!("{from} · {when}"),
                    None => when,
                };
                let subject =
                    if message.subject.is_empty() { tr("(no subject)").to_string() } else { message.subject.clone() };
                setting_row(None, subject, Some(detail), blank())
            }));
            rows
        }
    }
}

/// "Move", the folder under it while "Folder" is chosen, "Mark as read" and "Category".
fn action_rows<'a>(form: &'a Form, data: &'a Data) -> Vec<Element<'a, Message>> {
    let moves: Vec<Choice<Move>> = Move::ALL.into_iter().map(|moving| Choice::new(moving, moving.label())).collect();
    let mut rows = vec![setting_row(None, tr("Move"), None, choose(moves, Some(form.moving), Message::Move, None))];
    if form.moving == Move::Folder {
        let control: Element<'a, Message> = match &form.account {
            None => typo::text(typo::meta(), tr("Choose an account first")).style(torro_gui::theme::text_3).into(),
            Some(account) => {
                let folders: Vec<Choice<String>> = data
                    .mail_folders
                    .iter()
                    .filter(|folder| &folder.account == account && folder.role != Some(Role::Inbox))
                    .map(|folder| Choice::new(folder.id.clone(), folder.name.clone()))
                    .collect();
                choose(folders, form.folder.clone(), Message::Folder, Some(tr("Choose a folder")))
            }
        };
        rows.push(setting_row(None, tr("Folder"), None, control));
    }
    rows.push(setting_row(None, tr("Mark as read"), None, switch(form.mark_read, Message::MarkRead)));
    let mut categories = vec![Choice::new(None, tr("No category"))];
    categories.extend(category_choices(data));
    // A category no longer in the catalogue stays chosen until another is picked.
    if let Some(gone) = form.category.filter(|held| !data.categories.iter().any(|info| info.category == *held)) {
        categories.push(Choice::new(Some(gone), tr("Unknown category")));
    }
    rows.push(setting_row(
        None,
        tr("Category"),
        None,
        choose(categories, Some(form.category), Message::Category, None),
    ));
    rows
}

/// The categories of the catalogue as choices, by group, "Group · Name".
fn category_choices(data: &Data) -> Vec<Choice<Option<Category>>> {
    CategoryGroup::ALL
        .into_iter()
        .flat_map(|group| data.categories.iter().filter(move |info| info.group == group))
        .map(|info| Choice::new(Some(info.category), format!("{} · {}", info.group_label, info.label)))
        .collect()
}

/// "All accounts" and every mail account.
fn account_picker<'a>(form: &'a Form, data: &'a Data) -> Element<'a, Message> {
    let mut accounts = vec![Choice::new(None, tr("All accounts"))];
    accounts.extend(
        data.accounts
            .iter()
            .filter(|account| account.mail.is_some())
            .map(|account| Choice::new(Some(account.id.clone()), account.label.clone())),
    );
    if let Some(gone) =
        form.account.as_ref().filter(|held| !accounts.iter().any(|choice| choice.value.as_ref() == Some(*held)))
    {
        accounts.push(Choice::new(Some(gone.clone()), account_label(data, gone)));
    }
    choose(accounts, Some(form.account.clone()), Message::Account, None)
}

/// One choice of a pick list: what it stands for and its words.
#[derive(Debug, Clone)]
struct Choice<T> {
    value: T,
    words: String,
}

impl<T> Choice<T> {
    fn new(value: T, words: impl Into<String>) -> Self {
        Choice { value, words: words.into() }
    }
}

impl<T: PartialEq> PartialEq for Choice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

/// A pick list of `choices` in the look of a row's choice ([`crate::parts::picker`]), with
/// `chosen` closed in it; `placeholder` while nothing is chosen.
fn choose<'a, T: Clone + PartialEq + 'a>(
    choices: Vec<Choice<T>>,
    chosen: Option<T>,
    on_pick: impl Fn(T) -> Message + 'a,
    placeholder: Option<&'a str>,
) -> Element<'a, Message> {
    let chosen = chosen.and_then(|wanted| choices.iter().find(|held| held.value == wanted).cloned());
    let mut pick = iced::widget::pick_list(choices, chosen, move |choice: Choice<T>| on_pick(choice.value))
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu);
    if let Some(placeholder) = placeholder {
        pick = pick.placeholder(placeholder);
    }
    pick.into()
}

/// A group's note: the mistake in the colour of mistakes once "Save" was pressed, else `note`.
fn note_or_mistake<'a>(tried: bool, mistake: Option<String>, note: Option<String>) -> Option<Element<'a, Message>> {
    match (tried, mistake) {
        (true, Some(mistake)) => Some(
            typo::text(typo::meta(), mistake)
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
                .wrapping(Wrapping::WordOrGlyph)
                .into(),
        ),
        _ => note.map(|note| {
            typo::text(typo::meta(), note).style(torro_gui::theme::text_2).wrapping(Wrapping::WordOrGlyph).into()
        }),
    }
}

/// A group whose note can be a mistake: `widgets::group` without its own note, the line under it.
fn checked_group<'a>(
    title: &'a str,
    rows: Vec<Element<'a, Message>>,
    note: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let group = widgets::group(title, rows, None);
    match note {
        Some(note) => column![group, container(note).padding([0.0, space::l()])].spacing(space::s()).into(),
        None => group,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_ipc::inbox::CategoryInfo;
    use torro_mail::{CategoryDef, CustomCategory};

    use super::*;

    const ACCOUNT: &str = "mail:anna@mailbox.org";

    fn filter(id: &str, query: &str) -> MailFilter {
        MailFilter {
            id: id.into(),
            query: query.into(),
            account: None,
            file: Some(FilterFile::Archive),
            mark_read: true,
            category: None,
            enabled: true,
            created: chrono::DateTime::from_timestamp(0, 0).expect("a time"),
        }
    }

    /// The suite's catalogue and two filters: "f1" archives and marks as read for every account,
    /// "f2" files bills of one account under a category that is no longer in the catalogue.
    fn data() -> Data {
        let gone = Category::Custom(CustomCategory::new(9).expect("a number"));
        Data {
            categories: CategoryDef::defaults().iter().map(|def| CategoryInfo::of(def, 0, 0)).collect(),
            mail_filters: vec![
                filter("f1", "from:news@shop.example"),
                MailFilter {
                    account: Some(ACCOUNT.into()),
                    file: None,
                    mark_read: false,
                    category: Some(gone),
                    enabled: false,
                    ..filter("f2", "from:billing@shop.example")
                },
            ],
            ..Data::default()
        }
    }

    /// Runs `messages` in order and hands back the action of the last.
    fn last(state: &mut State, data: &Data, messages: Vec<Message>) -> Action<()> {
        let mut action = Action::None;
        for message in messages {
            action = update::<()>(state, message, data);
        }
        action
    }

    fn found(count: u64) -> Result<Response, String> {
        Ok(Response::MailFilterPreview { count, messages: Vec::new() })
    }

    /// "Create filter…" opens a new page with the search and the account filled in and asks at
    /// once what it finds; the answer is the page's line.
    #[test]
    fn a_new_filter_opens_filled_in_and_asks_what_it_finds() {
        let (data, mut state) = (data(), State::default());
        let action = last(
            &mut state,
            &data,
            vec![Message::New { query: "from:anna@example.org".into(), account: Some(ACCOUNT.into()) }],
        );
        assert_eq!(
            action,
            Action::Ask(Request::PreviewMailFilter {
                query: "from:anna@example.org".into(),
                account: Some(ACCOUNT.into())
            })
        );
        let form = state.form.as_ref().expect("the page is open");
        assert_eq!(
            (form.query.as_str(), form.account.as_deref(), form.id.as_ref()),
            ("from:anna@example.org", Some(ACCOUNT), None)
        );
        assert!(matches!(hits(&state.preview, form), Hits::Counting));
        assert!(answered(&mut state, &found(3)));
        let form = state.form.as_ref().expect("still open");
        assert!(matches!(hits(&state.preview, form), Hits::Found(Found { count: 3, .. })));
        // An empty "New filter" asks nothing.
        let action = last(&mut state, &data, vec![Message::New { query: String::new(), account: None }]);
        assert_eq!(action, Action::None);
    }

    /// Saving a new filter sends what the page holds; with mail it finds now the list asks whether
    /// to apply it, and "Apply" runs the filter the next load brought, again while more are left.
    #[test]
    fn saving_a_new_filter_makes_it_and_offers_to_apply_it() {
        let (mut data, mut state) = (data(), State::default());
        let _ = last(
            &mut state,
            &data,
            vec![
                Message::New { query: " from:shop.example ".into(), account: None },
                Message::Move(Move::Trash),
                Message::Category(Some(Category::Promotion)),
            ],
        );
        assert!(answered(&mut state, &found(12)));
        let action = last(&mut state, &data, vec![Message::Save]);
        assert_eq!(
            action,
            Action::send(Request::CreateMailFilter {
                query: "from:shop.example".into(),
                account: None,
                file: Some(FilterFile::Trash),
                mark_read: false,
                category: Some(Category::Promotion),
                enabled: true,
            })
        );
        assert!(!state.is_open(), "made once, so a second Save makes no twin");
        assert!(matches!(state.apply, Some(Apply::Ask { count: 12, .. })));
        // Before the load brings it there is nothing to apply.
        assert_eq!(last(&mut state, &data, vec![Message::Apply]), Action::None);
        data.mail_filters.push(MailFilter { file: Some(FilterFile::Trash), ..filter("f7", "from:shop.example") });
        assert_eq!(
            last(&mut state, &data, vec![Message::Apply]),
            Action::Ask(Request::ApplyMailFilter { id: "f7".into() })
        );
        assert!(answered(&mut state, &Ok(Response::MailFilterApplied { applied: 500, failed: 2, more: true })));
        assert_eq!(state.apply, Some(Apply::Done { id: "f7".into(), applied: 500, failed: 2, more: true }));
        assert_eq!(
            last(&mut state, &data, vec![Message::Apply]),
            Action::Ask(Request::ApplyMailFilter { id: "f7".into() })
        );
        assert!(answered(&mut state, &Err("The server refused.".into())));
        assert_eq!(state.apply, Some(Apply::Failed("The server refused.".into())));
        let _ = last(&mut state, &data, vec![Message::Dismiss]);
        assert!(state.apply.is_none());
    }

    /// A filter that finds nothing now is saved without the question.
    #[test]
    fn a_filter_that_finds_nothing_is_not_offered_to_apply() {
        let (data, mut state) = (data(), State::default());
        let _ = last(
            &mut state,
            &data,
            vec![Message::New { query: "from:x".into(), account: None }, Message::MarkRead(true)],
        );
        assert!(answered(&mut state, &found(0)));
        assert!(matches!(last(&mut state, &data, vec![Message::Save]), Action::Send(_)));
        assert!(state.apply.is_none());
    }

    /// Mistakes keep the filter from being sent: no search (the cursor goes there), nothing it does,
    /// a folder without an account or without a folder chosen, a search the core refused.
    #[test]
    fn a_filter_with_mistakes_is_not_sent() {
        let (data, mut state) = (data(), State::default());
        let open = || Message::New { query: String::new(), account: None };
        assert_eq!(
            last(&mut state, &data, vec![open(), Message::MarkRead(true), Message::Save]),
            Action::Focus(QUERY_FIELD)
        );
        assert!(state.is_open());
        assert_eq!(
            last(&mut state, &data, vec![Message::Query("from:x".into()), Message::MarkRead(false), Message::Save]),
            Action::None
        );
        let form = state.form.as_ref().expect("open");
        assert_eq!(form.action_mistake().as_deref(), Some(tr("Choose at least one thing the filter does.")));
        let _ = last(&mut state, &data, vec![Message::Move(Move::Folder), Message::Save]);
        let form = state.form.as_ref().expect("open");
        assert_eq!(
            form.action_mistake().as_deref(),
            Some(tr("Choose an account to move mail into one of its folders."))
        );
        let _ = last(&mut state, &data, vec![Message::Account(Some(ACCOUNT.into())), Message::Save]);
        let form = state.form.as_ref().expect("open");
        assert_eq!(form.action_mistake().as_deref(), Some(tr("Choose the folder the mail goes to.")));
        // The core refuses `in:`: its message is the search's mistake.
        let _ =
            last(&mut state, &data, vec![Message::Folder("f:demo-archive".into()), Message::Query("in:sent".into())]);
        while state.preview.waiting > 0 {
            assert!(answered(&mut state, &Err("A filter looks at the inbox only.".into())));
        }
        assert_eq!(last(&mut state, &data, vec![Message::Save]), Action::Focus(QUERY_FIELD));
        assert!(state.is_open());
    }

    /// A stored filter opens with what it holds; "Save" sends all of it with its switch, the list's
    /// switch keeps everything else, and deleting asks first.
    #[test]
    fn a_stored_filter_changes_switches_and_is_deleted_after_the_question() {
        let (data, mut state) = (data(), State::default());
        assert_eq!(
            last(&mut state, &data, vec![Message::Edit("f1".into())]),
            Action::Ask(Request::PreviewMailFilter { query: "from:news@shop.example".into(), account: None })
        );
        assert_eq!(
            last(&mut state, &data, vec![Message::Move(Move::Stay), Message::On(false), Message::Save]),
            Action::send(Request::UpdateMailFilter {
                id: "f1".into(),
                query: "from:news@shop.example".into(),
                account: None,
                file: None,
                mark_read: true,
                category: None,
                enabled: Some(false),
            })
        );
        assert_eq!(
            last(&mut state, &data, vec![Message::Enabled("f2".into(), true)]),
            Action::send(Request::UpdateMailFilter {
                id: "f2".into(),
                query: "from:billing@shop.example".into(),
                account: Some(ACCOUNT.into()),
                file: None,
                mark_read: false,
                category: data.mail_filters[1].category,
                enabled: Some(true),
            })
        );
        let _ = last(&mut state, &data, vec![Message::Edit("f2".into()), Message::Delete]);
        assert_eq!(state.asking, Some(Ask::Delete));
        assert_eq!(last(&mut state, &data, vec![Message::Answer(false)]), Action::None);
        assert!(state.is_open(), "Cancel keeps the page");
        assert_eq!(
            last(&mut state, &data, vec![Message::Delete, Message::Answer(true)]),
            Action::send(Request::RemoveMailFilter { id: "f2".into() })
        );
        assert!(!state.is_open());
    }

    /// "‹ Back" with something typed asks first; Escape is "‹ Back" while a page is open.
    #[test]
    fn leaving_a_changed_page_asks_first() {
        let (data, mut state) = (data(), State::default());
        assert!(escape(&state).is_none());
        let _ = last(&mut state, &data, vec![Message::Edit("f1".into())]);
        assert!(matches!(escape(&state), Some(Message::Back)));
        let _ = last(&mut state, &data, vec![Message::Back]);
        assert!(!state.is_open(), "nothing changed: back at once");
        let _ = last(&mut state, &data, vec![Message::Edit("f1".into()), Message::MarkRead(false), Message::Back]);
        assert_eq!(state.asking, Some(Ask::Discard));
        let _ = last(&mut state, &data, vec![Message::Answer(true)]);
        assert!(!state.is_open());
    }

    /// The list says what each filter does; a category that is gone reads "Unknown category".
    #[test]
    fn the_list_says_what_a_filter_does() {
        let data = data();
        let first = summary(&data.mail_filters[0], &data);
        assert_eq!(first, format!("{} · {}", trc("action", "Archive"), tr("Mark as read")));
        let second = summary(&data.mail_filters[1], &data);
        let line = format!("anna@mailbox.org · {}", tr("Unknown category"));
        assert_eq!(second, trf("Off · {rule}", &[("rule", &line)]));
    }

    /// Answers of other sections are left to them, failures too while nothing of this one waits.
    #[test]
    fn other_answers_are_left_alone() {
        let mut state = State::default();
        assert!(!answered(&mut state, &Err("Not mine.".into())));
        assert!(!answered(&mut state, &Ok(Response::Ok)));
    }

    /// The list and every page draw, with the question and the results of "Apply".
    #[test]
    fn the_pages_draw() {
        let (data, mut state) = (data(), State::default());
        let _ = page(&state, &data);
        for apply in [
            Apply::Ask { target: Target::Stored("f1".into()), count: 3 },
            Apply::Running { id: "f1".into() },
            Apply::Done { id: "f1".into(), applied: 3, failed: 1, more: true },
            Apply::Failed("No.".into()),
        ] {
            state.apply = Some(apply);
            let _ = page(&state, &data);
        }
        for open in [Message::New { query: String::new(), account: None }, Message::Edit("f2".into())] {
            let _ = update::<()>(&mut state, open, &data);
            let _ = update::<()>(&mut state, Message::Move(Move::Folder), &data);
            let _ = update::<()>(&mut state, Message::Delete, &data);
            let built = page(&state, &data);
            assert!(built.sub_page.is_some(), "a page below the section");
        }
    }

    /// The demo answers every request of the section, so `--demo` shows the list and a preview.
    #[tokio::test]
    async fn the_demo_has_filters() {
        let demo: Arc<dyn crate::Backend> = Arc::new(crate::demo::Demo::new());
        let data = crate::load(Arc::clone(&demo), super::super::wanted()).await;
        assert!(data.error.is_none(), "{:?}", data.error);
        assert_eq!(data.mail_filters.len(), 3);
        assert!(!data.mail_folders.is_empty());
        assert!(data.mail_filters.iter().any(|filter| summary(filter, &data).contains(tr("Unknown category"))));
        let answer = demo.call(Request::PreviewMailFilter { query: "from:x".into(), account: None }).await;
        assert!(matches!(answer, Ok(Response::MailFilterPreview { count: 12, .. })));
    }
}
