//! The dialog of a mail filter (issue #339, since issue #481 on the page of the category it sets in
//! Settings › Categorization, a filter without category under "Other filters"): a saved search with what to do with the mail it finds in the inbox – file it under a
//! category, mark it as read, move it to the trash. Mail works with categories only, so a filter
//! moves no mail to the archive or a folder.
//!
//! ```text
//! New filter
//! Search    from:shop.example has:attachment            Account [All accounts ▾]
//!           Finds 342 mails in Inbox now.
//! Then      ☑ Set category [Promotion ▾]   ☐ Mark as read   ☐ Move to trash
//!           ☑ Also apply to the 342 existing mails
//!                                                 [Delete filter]  [Cancel] [Save filter]
//! ```
//!
//! What the search finds comes from `mail.preview_filter`. "Save filter" waits for the core
//! ([`sent`]): only once it stored the filter does the dialog close; a refusal leaves it with what was
//! typed. With "Also apply…" ticked the stored filter then runs once on the mail it finds
//! (`mail.apply_filter`), and the result stands above the list of categories ([`apply_view`]),
//! with "Apply to more" while the core says more are left.
//!
//! Everything is a request; the dialog checks only what a person must fix before saving, and the
//! core checks all of it again. The torroinbox window opens a dialog already filled in, with
//! [`Message::New`]: "Create filter…" at a mail (`from:<address>`) and "Create filter from search…".

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, space as blank, stack};
use iced::{Alignment, Element, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{palette, size, space};
use torro_gui::widgets::{Align, Dropdown, DropdownEvent, MenuItem, MenuSection};
use torro_gui::{style, typo, widgets};
use torro_ipc::{Request, Response};
use torro_mail::{Category, CategoryGroup, FILTER_QUERY_MAX, FilterFile, MailFilter};

use crate::parts::{field, notice, row_frame, setting_row};
use crate::{Action, Data};

/// The id of the search field of the dialog: "Save filter" with a mistake in the search goes there.
pub const QUERY_FIELD: &str = "mail-filter-query";

/// What the dialog keeps between two frames: the filter being typed, its question, what its search
/// finds, the save on its way, and the run of a stored filter.
#[derive(Debug, Default)]
pub struct State {
    form: Option<Form>,
    asking: Option<Ask>,
    preview: Preview,
    saving: Option<Saving>,
    apply: Option<Apply>,
    /// The dropdowns of the dialog, over it (issue #554).
    menus: Dropdown<Menu>,
}

/// The dropdowns of the dialog (issue #554).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Menu {
    /// "Account".
    Account,
    /// "Category" of "Set category".
    Category,
}

impl State {
    /// The dialog is open.
    pub fn is_open(&self) -> bool {
        self.form.is_some()
    }
}

/// A save on its way to the core: the filter it stores, and whether it runs on the mail it found
/// once stored ("Also apply…").
#[derive(Debug, Clone, PartialEq, Eq)]
struct Saving {
    target: Target,
    apply: bool,
}

/// What the dialog asks before it does something that loses what was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Close the dialog and drop what was typed.
    Discard,
    /// Delete the open filter.
    Delete,
}

/// A filter as it is being typed: a new one (`id: None`) or one that is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Form {
    id: Option<String>,
    query: String,
    account: Option<String>,
    /// "Set category" with the category chosen; `None` while it is not ticked.
    category: Option<Category>,
    mark_read: bool,
    trash: bool,
    enabled: bool,
    /// "Also apply to the … existing mails".
    apply: bool,
    /// "Save filter" was pressed once: mistakes show from now on.
    tried: bool,
}

/// What a search finds in the inbox now, as the core answered `mail.preview_filter`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    count: u64,
}

/// The previews asked for the open dialog. Answers carry no search, so only the answer that comes
/// when nothing is on its way any more counts, for the search asked last.
#[derive(Debug, Default)]
struct Preview {
    waiting: u32,
    asked: Option<(String, Option<String>)>,
    found: Option<Result<Found, String>>,
}

/// The run of a stored filter on the mail it finds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Apply {
    /// The run is on its way.
    Running { id: String },
    /// What the run did; `more` offers another.
    Done { id: String, applied: u64, failed: u64, more: bool },
    /// The core refused, with its message.
    Failed(String),
}

/// The filter a save stores. A new filter has no id until the load after the save brings it: it is
/// the one with that search and account that was not there before.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Stored(String),
    New { query: String, account: Option<String>, known: Vec<String> },
}

impl Target {
    /// The id of the filter in `data`; `None` when it is not there.
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

/// What the dialog and the result of a run report.
#[derive(Debug, Clone)]
pub enum Message {
    /// Open a new filter's dialog with this search and account – "New filter…" (empty), "Create
    /// filter…" at a mail (`from:<address>`) and "Create filter from search…" of torroinbox. Asks at
    /// once what the search finds.
    New {
        query: String,
        account: Option<String>,
    },
    /// "Filter from a search…" on a category's page (issue #481): a new filter's dialog with an empty
    /// search that sets this category.
    NewFor(Category),
    /// A filter's row: open its dialog.
    Edit(String),
    /// The switch of a filter's row: stored at once.
    Enabled(String, bool),
    /// "Apply to existing mail" of a filter's row, and "Apply to more": run the stored filter once.
    ApplyNow(String),
    Query(String),
    /// The account the filter runs in; `None`: every account.
    Account(Option<String>),
    /// "Set category" ticked (`true`) or not.
    SetCategory(bool),
    /// The category of "Set category".
    Category(Category),
    MarkRead(bool),
    Trash(bool),
    /// "Filter is on".
    On(bool),
    /// "Also apply to the … existing mails".
    AlsoApply(bool),
    Save,
    /// "Delete filter" – after the question.
    Delete,
    /// The answer to the question the dialog asks: go ahead (`true`) or not.
    Answer(bool),
    /// "Cancel", a click next to the dialog, and Escape: asks first when something typed would go.
    Close,
    /// "Done" under the result of a run.
    Dismiss,
    /// The trigger or the menu of a dropdown of the dialog (issue #554).
    Menus(DropdownEvent<Menu>),
}

pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    // Any other click of the dialog – a choice of the open dropdown among them – closes it.
    if let Message::Menus(event) = message {
        if state.menus.update(event) {
            return Action::Focus(widgets::DROPDOWN_SEARCH);
        }
        return Action::None;
    }
    state.menus.close();
    match message {
        Message::New { query, account } => {
            open(state, Form::new(query, account));
            ask_preview(state)
        }
        Message::NewFor(category) => {
            open(state, Form { category: Some(category), ..Form::new(String::new(), None) });
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
        Message::ApplyNow(id) => {
            state.apply = Some(Apply::Running { id: id.clone() });
            Action::Ask(Request::ApplyMailFilter { id })
        }
        Message::Close => {
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
                    form.account = account;
                    true
                }
                Message::SetCategory(on) => {
                    form.category = on.then(|| first_category(data));
                    false
                }
                Message::Category(category) => {
                    form.category = Some(category);
                    false
                }
                Message::MarkRead(on) => {
                    form.mark_read = on;
                    false
                }
                Message::Trash(on) => {
                    form.trash = on;
                    false
                }
                Message::On(on) => {
                    form.enabled = on;
                    false
                }
                Message::AlsoApply(on) => {
                    form.apply = on;
                    false
                }
                _ => false,
            };
            if ask { ask_preview(state) } else { Action::None }
        }
    }
}

/// The category "Set category" starts with: the first of the catalogue, by group.
fn first_category(data: &Data) -> Category {
    CategoryGroup::ALL
        .into_iter()
        .flat_map(|group| data.categories.iter().filter(move |info| info.group == group))
        .map(|info| info.category)
        .next()
        .unwrap_or(Category::Promotion)
}

/// What Escape means while the dialog is open: its question first, then "Cancel". `None` without
/// it.
pub fn escape(state: &State) -> Option<Message> {
    if state.menus.open().is_some() {
        return Some(Message::Menus(DropdownEvent::Close));
    }
    state.is_open().then_some(Message::Close)
}

/// Takes an answer of a preview or a run; `true` when it was this dialog's, so no other part of the
/// page takes it. A failure is this dialog's only while one of its asks is on its way.
pub fn answered(state: &mut State, answer: &Result<Response, String>) -> bool {
    match answer {
        Ok(Response::MailFilterPreview { count, .. }) => {
            state.preview.waiting = state.preview.waiting.saturating_sub(1);
            state.preview.found = Some(Ok(Found { count: *count }));
            true
        }
        Ok(Response::MailFilterApplied { applied, failed, more }) => {
            if let Some(Apply::Running { id }) = &state.apply {
                let id = id.clone();
                state.apply = Some(Apply::Done { id, applied: *applied, failed: *failed, more: *more });
            }
            true
        }
        Err(problem) if state.preview.waiting > 0 => {
            state.preview.waiting -= 1;
            state.preview.found = Some(Err(problem.clone()));
            true
        }
        Err(problem) if matches!(state.apply, Some(Apply::Running { .. })) => {
            state.apply = Some(Apply::Failed(problem.clone()));
            true
        }
        _ => false,
    }
}

/// The send of the host came back: `result` is the data loaded after it, or why it failed. While a
/// save is on its way, this is its answer: stored, the dialog closes and – with "Also apply…" – the
/// id of the stored filter comes back, to run it once ([`Message::ApplyNow`]); refused, the dialog
/// stays as it was typed and "Save filter" can be pressed again.
pub fn sent(state: &mut State, result: &Result<Data, String>) -> Option<String> {
    let saving = state.saving.take()?;
    let Ok(data) = result else { return None };
    state.form = None;
    state.asking = None;
    saving.target.id(data).filter(|_| saving.apply)
}

/// Opens `form` as the dialog, without a question and without an answer of an earlier one.
fn open(state: &mut State, form: Form) {
    state.asking = None;
    state.saving = None;
    state.preview.asked = None;
    state.preview.found = None;
    state.form = Some(form);
}

/// `mail.preview_filter` for the dialog's search and account; nothing for an empty or too long
/// search, which the dialog names as a mistake instead.
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

/// "Save filter": the mistakes once pressed, else the request. The dialog stays open until the core
/// answered ([`sent`]); a second "Save filter" meanwhile sends nothing.
fn save<A>(state: &mut State, data: &Data) -> Action<A> {
    if state.saving.is_some() {
        return Action::None;
    }
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
    // Still counting: "Also apply…" holds all the same; only a search known to find nothing skips it.
    let found = !matches!(hits, Some(Hits::Found(found)) if found.count == 0);
    let request = form.request();
    let target = match &form.id {
        Some(id) => Target::Stored(id.clone()),
        None => Target::New {
            query: form.query.trim().to_string(),
            account: form.account.clone(),
            known: data.mail_filters.iter().map(|filter| filter.id.clone()).collect(),
        },
    };
    state.saving = Some(Saving { target, apply: form.apply && found });
    Action::send(request)
}

/// `mail.update_filter` that switches `filter` on or off and keeps everything else.
pub(super) fn switched(filter: &MailFilter, on: bool) -> Request {
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
            category: None,
            mark_read: false,
            trash: false,
            enabled: true,
            apply: true,
            tried: false,
        }
    }

    fn of(filter: &MailFilter) -> Form {
        Form {
            id: Some(filter.id.clone()),
            query: filter.query.clone(),
            account: filter.account.clone(),
            category: filter.category,
            mark_read: filter.mark_read,
            trash: filter.file == Some(FilterFile::Trash),
            enabled: filter.enabled,
            apply: false,
            tried: false,
        }
    }

    /// Something typed in the dialog is not saved yet.
    fn dirty(&self, data: &Data) -> bool {
        match &self.id {
            None => !self.query.trim().is_empty() || self.has_action(),
            Some(id) => {
                data.mail_filters.iter().find(|filter| &filter.id == id).is_some_and(|stored| !self.same_as(stored))
            }
        }
    }

    /// The dialog holds what `stored` holds: nothing to save.
    fn same_as(&self, stored: &MailFilter) -> bool {
        let held = Form::of(stored);
        let page = Form { tried: false, apply: false, query: self.query.trim().to_string(), ..self.clone() };
        page == held
    }

    fn has_action(&self) -> bool {
        self.trash || self.mark_read || self.category.is_some()
    }

    /// What is wrong with the search, if anything; `failed` is the core's word on it.
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
        (!self.has_action()).then(|| tr("Choose at least one thing the filter does.").to_string())
    }

    /// The request that saves the dialog.
    fn request(&self) -> Request {
        let query = self.query.trim().to_string();
        let file = self.trash.then_some(FilterFile::Trash);
        match &self.id {
            None => Request::CreateMailFilter {
                query,
                account: self.account.clone(),
                file,
                mark_read: self.mark_read,
                category: self.category,
                enabled: self.enabled,
            },
            Some(id) => Request::UpdateMailFilter {
                id: id.clone(),
                query,
                account: self.account.clone(),
                file,
                mark_read: self.mark_read,
                category: self.category,
                enabled: Some(self.enabled),
            },
        }
    }
}

/// What the dialog's search finds, as far as it is known.
enum Hits<'a> {
    Empty,
    Counting,
    Found(&'a Found),
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

/// What a filter does, in a line: "Category · Promotion · Mark as read · Move to Trash", and its
/// account first when it runs in one. A category that is no longer in the catalogue reads "Unknown
/// category".
pub fn actions_line(filter: &MailFilter, data: &Data) -> String {
    let mut parts = Vec::new();
    if let Some(account) = &filter.account {
        parts.push(account_label(data, account));
    }
    if let Some(category) = filter.category {
        parts.push(match data.categories.iter().find(|info| info.category == category) {
            Some(info) => trf("Category · {category}", &[("category", &info.label)]),
            None => tr("Unknown category").to_string(),
        });
    }
    if filter.mark_read {
        parts.push(tr("Mark as read").to_string());
    }
    if let Some(FilterFile::Trash) = &filter.file {
        parts.push(tr("Move to trash").to_string());
    }
    parts.join(" · ")
}

/// The name of a mail account (`mail:<address>`): its label, else the address.
fn account_label(data: &Data, id: &str) -> String {
    data.accounts
        .iter()
        .find(|account| account.id == id)
        .map(|account| account.label.clone())
        .unwrap_or_else(|| id.trim_start_matches("mail:").to_string())
}

/// What a run of a filter did, above the list of rules and filters; `None` without one.
pub fn apply_view(state: &State) -> Option<Element<'_, Message>> {
    let row: Element<'_, Message> = match state.apply.as_ref()? {
        Apply::Running { .. } => setting_row(None, tr("Applying the filter…"), None, blank()),
        Apply::Done { id, applied, failed, more } => {
            let words = match applied {
                1 => tr("Applied to 1 mail.").to_string(),
                n => trf("Applied to {n} mails.", &[("n", n)]),
            };
            let detail =
                (*failed > 0).then(|| trf("{n} could not be changed and stay as they were.", &[("n", failed)]));
            let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
            if *more {
                let again = Some(Message::ApplyNow(id.clone()));
                buttons =
                    buttons.push(widgets::text_button(None, tr("Apply to more"), style::button::secondary, again));
            }
            buttons =
                buttons.push(widgets::text_button(None, tr("Done"), style::button::ghost, Some(Message::Dismiss)));
            setting_row(None, words, detail, buttons)
        }
        Apply::Failed(problem) => {
            let done = widgets::text_button(None, tr("Done"), style::button::ghost, Some(Message::Dismiss));
            row_frame(
                row![container(notice(problem.clone())).width(iced::Fill), done]
                    .spacing(space::l())
                    .align_y(Alignment::Center),
                false,
            )
        }
    };
    Some(widgets::group(tr("Apply filter"), vec![row], None))
}

/// The dialog over the window while a filter is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let form = state.form.as_ref()?;
    let found = hits(&state.preview, form);
    let failed = match &found {
        Hits::Failed(problem) => Some(*problem),
        _ => None,
    };
    let title = match form.id {
        None => tr("New filter"),
        Some(_) => tr("Edit filter"),
    };
    let query = field("from:shop.example has:attachment", &form.query)
        .id(QUERY_FIELD)
        .on_input(Message::Query)
        .on_submit(Message::Save);
    let count = match &found {
        Hits::Empty => tr("Type a search to see what it finds.").to_string(),
        Hits::Counting => tr("Counting…").to_string(),
        Hits::Failed(problem) => problem.to_string(),
        Hits::Found(found) => match found.count {
            0 => tr("Finds no mail in Inbox now.").to_string(),
            1 => tr("Finds 1 mail in Inbox now.").to_string(),
            n => trf("Finds {n} mails in Inbox now.", &[("n", &torro_gui::units::format_count(n))]),
        },
    };
    let search_note = match (form.tried, form.query_mistake(failed)) {
        (true, Some(mistake)) => mistake_text(mistake),
        _ => typo::text(typo::meta(), count).style(torro_gui::theme::text_2).wrapping(Wrapping::WordOrGlyph).into(),
    };
    let search = widgets::group(
        tr("Search"),
        vec![row_frame(query, false), setting_row(None, tr("Account"), None, super::categorization::choice_box(dropdown_button(&state.menus, Menu::Account, &account_choices(form, data), &form.account)))],
        Some(
            tr("Written like a search: from:, to:, subject:, has:attachment … On new mail, plain words are looked for in the subject and the addresses.")
                .to_string(),
        ),
    );
    let mut then = vec![super::categorization::check_row(
        tr("Set category"),
        None,
        form.category.is_some(),
        Some(Message::SetCategory(form.category.is_none())),
    )];
    if form.category.is_some() {
        then.push(setting_row(
            None,
            tr("Category"),
            None,
            super::categorization::choice_box(dropdown_button(
                &state.menus,
                Menu::Category,
                &category_choices(form, data),
                &form.category.unwrap_or(Category::Promotion),
            )),
        ));
    }
    then.push(super::categorization::check_row(
        tr("Mark as read"),
        None,
        form.mark_read,
        Some(Message::MarkRead(!form.mark_read)),
    ));
    then.push(super::categorization::check_row(
        tr("Move to trash"),
        Some(tr("It can be taken back from there until the trash is emptied.").to_string()),
        form.trash,
        Some(Message::Trash(!form.trash)),
    ));
    if let Hits::Found(Found { count }) = found
        && *count > 0
    {
        let label = match count {
            1 => tr("Also apply to the 1 existing mail").to_string(),
            n => trf("Also apply to the {n} existing mails", &[("n", &torro_gui::units::format_count(*n))]),
        };
        then.push(super::categorization::check_row(label, None, form.apply, Some(Message::AlsoApply(!form.apply))));
    }
    if form.id.is_some() {
        then.push(super::categorization::check_row(
            tr("Filter is on"),
            None,
            form.enabled,
            Some(Message::On(!form.enabled)),
        ));
    }
    let then_note = match (form.tried, form.action_mistake()) {
        (true, Some(mistake)) => Some(mistake),
        _ => None,
    };
    let mut content =
        column![typo::text(typo::title(), title), search, search_note, widgets::group(tr("Then"), then, None),]
            .spacing(space::l());
    if let Some(mistake) = then_note {
        content = content.push(mistake_text(mistake));
    }
    if let Some(asked) = state.asking {
        content = content.push(question(asked));
    }
    let save = match state.saving {
        Some(_) => widgets::busy_button(tr("Saving…"), style::button::primary),
        None => widgets::text_button(None, tr("Save filter"), style::button::primary, Some(Message::Save)),
    };
    let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
    if form.id.is_some() {
        buttons = buttons.push(widgets::text_button(
            Some(Icon::Trash2),
            tr("Delete filter"),
            style::button::danger,
            Some(Message::Delete),
        ));
    }
    buttons = buttons
        .push(blank::horizontal())
        .push(widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close)))
        .push(save);
    let content = content.push(buttons);
    let dialog = crate::parts::dialog(
        iced::widget::scrollable(content).style(style::scrollable),
        size::form_max(),
        Message::Close,
    );
    // An open dropdown stands over the dialog, in the same layer (issue #554).
    Some(match dropdown(state, form, data) {
        Some(menu) => stack![dialog, menu].into(),
        None => dialog,
    })
}

/// The open dropdown of the dialog under its field, the chosen entry marked.
fn dropdown<'a>(state: &'a State, form: &'a Form, data: &'a Data) -> Option<Element<'a, Message>> {
    let key = state.menus.open()?;
    let sections = match key {
        Menu::Account => entries(account_choices(form, data), &form.account, Message::Account),
        Menu::Category => {
            entries(category_choices(form, data), &form.category.unwrap_or(Category::Promotion), Message::Category)
        }
    };
    Some(state.menus.view(key, sections, size::props(), Align::Start, Message::Menus))
}

/// `choices` as the entries of a dropdown, `chosen` marked; a pick sends `on_pick`.
fn entries<'a, T: Clone + PartialEq>(
    choices: Vec<Choice<T>>,
    chosen: &T,
    on_pick: impl Fn(T) -> Message,
) -> Vec<MenuSection<'a, Message>> {
    let items = choices
        .into_iter()
        .map(|choice| {
            let current = choice.value == *chosen;
            MenuItem::plain(choice.words, on_pick(choice.value)).current(current)
        })
        .collect();
    vec![MenuSection::new(items)]
}

/// A mistake under a part of the dialog, in the colour of mistakes.
fn mistake_text<'a>(mistake: String) -> Element<'a, Message> {
    typo::text(typo::meta(), mistake)
        .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// The question the dialog asks, with its two answers.
fn question<'a>(asked: Ask) -> Element<'a, Message> {
    let (title, detail, yes) = match asked {
        Ask::Discard => (tr("Discard changes?"), tr("What you typed is not saved."), tr("Discard")),
        Ask::Delete => (tr("Delete this filter?"), tr("Mail it filed keeps its category."), tr("Delete filter")),
    };
    let buttons = row![
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Answer(false))),
        widgets::text_button(None, yes, style::button::danger, Some(Message::Answer(true))),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::group(title, vec![setting_row(None, detail, None, buttons)], None)
}

/// The categories of the catalogue, by group, "Group · Name"; one that is gone stays chosen as
/// "Unknown category" until another is picked.
fn category_choices(form: &Form, data: &Data) -> Vec<Choice<Category>> {
    let mut categories: Vec<Choice<Category>> = CategoryGroup::ALL
        .into_iter()
        .flat_map(|group| data.categories.iter().filter(move |info| info.group == group))
        .map(|info| Choice::new(info.category, format!("{} · {}", info.group_label, info.label)))
        .collect();
    if let Some(gone) = form.category.filter(|held| !data.categories.iter().any(|info| info.category == *held)) {
        categories.push(Choice::new(gone, tr("Unknown category")));
    }
    categories
}

/// "All accounts" and every mail account.
fn account_choices(form: &Form, data: &Data) -> Vec<Choice<Option<String>>> {
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
    accounts
}

/// One choice of a dropdown: what it stands for and its words.
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

/// The trigger of a dropdown of `choices` as wide as its place, with the words of `chosen` (issue
/// #554).
fn dropdown_button<'a, T: PartialEq>(
    menus: &Dropdown<Menu>,
    key: Menu,
    choices: &[Choice<T>],
    chosen: &T,
) -> Element<'a, Message> {
    let words = choices.iter().find(|held| held.value == *chosen).map(|held| held.words.clone()).unwrap_or_default();
    menus.button_fill(key, words, Message::Menus)
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
            file: Some(FilterFile::Trash),
            mark_read: true,
            category: None,
            enabled: true,
            created: chrono::DateTime::from_timestamp(0, 0).expect("a time"),
        }
    }

    /// The suite's catalogue and two filters: "f1" trashes and marks as read for every account,
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

    /// "Create filter…" opens a new dialog with the search and the account filled in and asks at
    /// once what it finds.
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
        let form = state.form.as_ref().expect("the dialog is open");
        assert!(matches!(hits(&state.preview, form), Hits::Counting));
        assert!(answered(&mut state, &found(3)));
        let form = state.form.as_ref().expect("still open");
        assert!(matches!(hits(&state.preview, form), Hits::Found(Found { count: 3 })));
        assert!(overlay(&state, &data).is_some());
        assert_eq!(last(&mut state, &data, vec![Message::New { query: String::new(), account: None }]), Action::None);
    }

    /// Issue #454: a new filter sets a category, reads or goes to the trash – never a folder – and with
    /// "Also apply…" the stored filter runs once on what it found; "Apply to more" while more are left.
    #[test]
    fn saving_a_new_filter_applies_it_when_asked() {
        let (mut data, mut state) = (data(), State::default());
        let _ = last(
            &mut state,
            &data,
            vec![
                Message::New { query: " from:shop.example ".into(), account: None },
                Message::Trash(true),
                Message::SetCategory(true),
                Message::Category(Category::Promotion),
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
        assert!(state.is_open(), "open until the core answers");
        assert_eq!(last(&mut state, &data, vec![Message::Save]), Action::None, "a second Save makes no twin");
        data.mail_filters.push(MailFilter { file: Some(FilterFile::Trash), ..filter("f7", "from:shop.example") });
        assert_eq!(sent(&mut state, &Ok(data.clone())).as_deref(), Some("f7"), "run it once");
        assert!(!state.is_open());
        assert_eq!(
            last(&mut state, &data, vec![Message::ApplyNow("f7".into())]),
            Action::Ask(Request::ApplyMailFilter { id: "f7".into() })
        );
        assert!(answered(&mut state, &Ok(Response::MailFilterApplied { applied: 500, failed: 2, more: true })));
        assert_eq!(state.apply, Some(Apply::Done { id: "f7".into(), applied: 500, failed: 2, more: true }));
        assert!(apply_view(&state).is_some());
        let _ = last(&mut state, &data, vec![Message::ApplyNow("f7".into())]);
        assert!(answered(&mut state, &Err("The server refused.".into())));
        assert_eq!(state.apply, Some(Apply::Failed("The server refused.".into())));
        let _ = last(&mut state, &data, vec![Message::Dismiss]);
        assert!(state.apply.is_none() && apply_view(&state).is_none());
    }

    /// PR review of #463: saved while the search is still counted, "Also apply…" holds.
    #[test]
    fn a_filter_saved_while_counting_still_applies() {
        let (mut data, mut state) = (data(), State::default());
        let _ = last(
            &mut state,
            &data,
            vec![Message::New { query: "from:z".into(), account: None }, Message::MarkRead(true)],
        );
        assert!(matches!(last(&mut state, &data, vec![Message::Save]), Action::Send(_)), "saved before the count");
        data.mail_filters.push(MailFilter { mark_read: true, file: None, ..filter("f9", "from:z") });
        assert_eq!(sent(&mut state, &Ok(data.clone())).as_deref(), Some("f9"));
    }

    /// Without "Also apply…", or when the search finds nothing, the stored filter does not run.
    #[test]
    fn a_filter_runs_on_existing_mail_only_when_asked() {
        let (mut data, mut state) = (data(), State::default());
        let _ = last(
            &mut state,
            &data,
            vec![
                Message::New { query: "from:x".into(), account: None },
                Message::MarkRead(true),
                Message::AlsoApply(false),
            ],
        );
        assert!(answered(&mut state, &found(4)));
        assert!(matches!(last(&mut state, &data, vec![Message::Save]), Action::Send(_)));
        data.mail_filters.push(MailFilter { mark_read: true, file: None, ..filter("f8", "from:x") });
        assert_eq!(sent(&mut state, &Ok(data.clone())), None);
        let _ = last(
            &mut state,
            &data,
            vec![Message::New { query: "from:y".into(), account: None }, Message::MarkRead(true)],
        );
        assert!(answered(&mut state, &found(0)));
        let _ = last(&mut state, &data, vec![Message::Save]);
        assert_eq!(sent(&mut state, &Ok(data.clone())), None, "nothing found, nothing to apply");
    }

    /// "Save filter" waits for the core: refused, the dialog stays with what was typed.
    #[test]
    fn a_save_waits_for_the_core() {
        let (data, mut state) = (data(), State::default());
        let _ = last(&mut state, &data, vec![Message::Edit("f1".into()), Message::MarkRead(false)]);
        assert!(answered(&mut state, &found(4)));
        assert_eq!(sent(&mut state, &Ok(data.clone())), None, "no save on its way: nothing happens");
        let save = last(&mut state, &data, vec![Message::Save]);
        assert!(matches!(save, Action::Send(_)));
        assert_eq!(sent(&mut state, &Err("The server refused.".into())), None);
        let form = state.form.as_ref().expect("still open");
        assert!(!form.mark_read && form.id.as_deref() == Some("f1"), "what was typed stays");
        assert_eq!(last(&mut state, &data, vec![Message::Save]), save, "saved again");
        assert_eq!(sent(&mut state, &Ok(data.clone())), None, "a change does not run by itself");
        assert!(!state.is_open());
    }

    /// Mistakes keep the filter from being sent: no search (the cursor goes there), nothing it does,
    /// a search the core refused.
    #[test]
    fn a_filter_with_mistakes_is_not_sent() {
        let (data, mut state) = (data(), State::default());
        let open = || Message::New { query: String::new(), account: None };
        assert_eq!(
            last(&mut state, &data, vec![open(), Message::MarkRead(true), Message::Save]),
            Action::Focus(QUERY_FIELD)
        );
        assert_eq!(
            last(&mut state, &data, vec![Message::Query("from:x".into()), Message::MarkRead(false), Message::Save]),
            Action::None
        );
        let form = state.form.as_ref().expect("open");
        assert_eq!(form.action_mistake().as_deref(), Some(tr("Choose at least one thing the filter does.")));
        let _ = last(&mut state, &data, vec![Message::Trash(true), Message::Query("in:sent".into())]);
        while state.preview.waiting > 0 {
            assert!(answered(&mut state, &Err("A filter looks at the inbox only.".into())));
        }
        assert_eq!(last(&mut state, &data, vec![Message::Save]), Action::Focus(QUERY_FIELD));
        assert!(overlay(&state, &data).is_some());
    }

    /// A stored filter opens with what it holds; "Save filter" sends all of it with its switch, the
    /// row's switch keeps everything else, and deleting asks first.
    #[test]
    fn a_stored_filter_changes_switches_and_is_deleted_after_the_question() {
        let (data, mut state) = (data(), State::default());
        assert_eq!(
            last(&mut state, &data, vec![Message::Edit("f1".into())]),
            Action::Ask(Request::PreviewMailFilter { query: "from:news@shop.example".into(), account: None })
        );
        assert_eq!(
            last(&mut state, &data, vec![Message::Trash(false), Message::On(false), Message::Save]),
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
            Action::send(switched(&data.mail_filters[1], true))
        );
        let _ = last(&mut state, &data, vec![Message::Edit("f2".into()), Message::Delete]);
        assert_eq!(state.asking, Some(Ask::Delete));
        assert!(overlay(&state, &data).is_some());
        assert_eq!(last(&mut state, &data, vec![Message::Answer(false)]), Action::None);
        assert!(state.is_open(), "Cancel keeps the dialog");
        assert_eq!(
            last(&mut state, &data, vec![Message::Delete, Message::Answer(true)]),
            Action::send(Request::RemoveMailFilter { id: "f2".into() })
        );
        assert!(!state.is_open());
    }

    /// Closing with something typed asks first; Escape is "Cancel" while the dialog is open.
    #[test]
    fn closing_a_changed_dialog_asks_first() {
        let (data, mut state) = (data(), State::default());
        assert!(escape(&state).is_none());
        let _ = last(&mut state, &data, vec![Message::Edit("f1".into())]);
        assert!(matches!(escape(&state), Some(Message::Close)));
        let _ = last(&mut state, &data, vec![Message::Close]);
        assert!(!state.is_open(), "nothing changed: closed at once");
        let _ = last(&mut state, &data, vec![Message::Edit("f1".into()), Message::MarkRead(false), Message::Close]);
        assert_eq!(state.asking, Some(Ask::Discard));
        let _ = last(&mut state, &data, vec![Message::Answer(true)]);
        assert!(!state.is_open());
    }

    /// The row says what a filter does; a category that is gone reads "Unknown category".
    #[test]
    fn the_row_says_what_a_filter_does() {
        let data = data();
        assert_eq!(
            actions_line(&data.mail_filters[0], &data),
            format!("{} · {}", tr("Mark as read"), tr("Move to trash"))
        );
        let second = actions_line(&data.mail_filters[1], &data);
        assert_eq!(second, format!("anna@mailbox.org · {}", tr("Unknown category")));
    }

    /// Answers of other parts are left to them, failures too while nothing of this dialog waits.
    #[test]
    fn other_answers_are_left_alone() {
        let mut state = State::default();
        assert!(!answered(&mut state, &Err("Not mine.".into())));
        assert!(!answered(&mut state, &Ok(Response::Ok)));
    }

    /// The demo answers every request of the dialog.
    #[tokio::test]
    async fn the_demo_has_filters() {
        let demo: Arc<dyn crate::Backend> = Arc::new(crate::demo::Demo::new());
        let data = crate::load(Arc::clone(&demo), super::super::wanted()).await;
        assert!(data.error.is_none(), "{:?}", data.error);
        assert_eq!(data.mail_filters.len(), 4);
        assert!(data.mail_filters.iter().any(|filter| actions_line(filter, &data).contains(tr("Unknown category"))));
        assert!(data.mail_filters.iter().all(|filter| filter.file.is_none() || filter.file == Some(FilterFile::Trash)));
        let answer = demo.call(Request::PreviewMailFilter { query: "from:x".into(), account: None }).await;
        assert!(matches!(answer, Ok(Response::MailFilterPreview { count: 12, .. })));
    }

    /// Issue #554: the dialog's dropdowns open over it with the focus on their search, Escape closes
    /// the dropdown before the dialog, and a pick closes it.
    #[test]
    fn the_dialogs_dropdowns_open_over_it_and_close() {
        let (data, mut state) = (data(), State::default());
        let _ = last(&mut state, &data, vec![Message::New { query: String::new(), account: None }]);
        let opened = update::<()>(&mut state, Message::Menus(DropdownEvent::Toggle(Menu::Account)), &data);
        assert_eq!(opened, Action::Focus(widgets::DROPDOWN_SEARCH));
        assert!(dropdown(&state, state.form.as_ref().expect("open"), &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Menus(DropdownEvent::Close))));
        let _ = update::<()>(&mut state, Message::Account(Some(ACCOUNT.into())), &data);
        assert!(state.menus.open().is_none(), "a pick closes the dropdown");
        assert!(matches!(escape(&state), Some(Message::Close)), "then Escape closes the dialog");
        let toggle = Message::Menus(DropdownEvent::Toggle(Menu::Category));
        let _ = last(&mut state, &data, vec![Message::SetCategory(true), toggle]);
        assert!(overlay(&state, &data).is_some());
        let _ = update::<()>(&mut state, Message::Category(Category::Invoice), &data);
        assert!(state.menus.open().is_none());
    }
}
