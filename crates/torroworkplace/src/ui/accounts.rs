//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`, several accounts since M1.2): the
//! accounts as rows of one settings group per provider (status dot, label on one line, status and
//! last sync below, chevron), "Add account" in the header bar – always, also with accounts; a
//! detail page per account with groups for the status ("Sync now"), the calendars and task lists
//! (a switch each for showing them in the apps, read-only ones marked) and the connection, and
//! "Remove account…" behind a confirmation that names the account and acts on it alone. A
//! sign-in in progress (an entry without an id) is a "Signing in…" row without a detail page.
//! Empty: a sentence about what to add. The add flow is `wizard.rs` (task S.6); it and the confirmation
//! are dialogs over the whole window ([`overlay`]). The shell hands the answers to requests to
//! [`answered`], so the dialog shows the outcome of its own requests.
//!
//! A GitHub account (task G.1) has no calendars or lists yet: its detail page shows what the
//! token's scopes allow and what is missing ([`github_permissions`]) in place of them.
//!
//! A CalDAV account (a mail provider's, task M7.2) shows "CalDAV" as its kind, the server and user
//! in the connection group, and "Change password…" there: a dialog over the window asks for the
//! new password and sends `ConnectCalDav` for the same server and user again (the core then
//! replaces only the password). A refused password stays in the dialog under its field.
//!
//! Requests: `Sync`, `RemoveAccount { id }`, `ConnectCalDav` for "Change password…", and
//! `SetCollectionShown { collection, shown }` for the switches. The pure parts (relative time, status text, whether a collection is shown,
//! which accounts are listed) are plain functions with unit tests.

use chrono::{DateTime, Utc};
use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, text, toggler};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::account::Secret;
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status, collection_color, color_dot, empty_state};
use torro_gui::{style, typo};
use torro_ipc::{AccountInfo, Request, Response};
use torro_pim::{AccountState, Collection, CollectionKind};

use super::{Action, Data, Header, chevron, confirm, dialog, pressable, row_frame, setting_row, wizard};

#[derive(Debug, Default)]
pub struct State {
    /// The add-account dialog while it is open.
    pub wizard: Option<wizard::State>,
    /// The account whose detail page is open (its `AccountInfo::id`); `None` shows the list.
    pub selected: Option<String>,
    /// The "Remove this account?" confirmation is open.
    pub confirm_remove: bool,
    /// The "Change password" dialog of the open CalDAV account.
    pub password: Option<PasswordChange>,
}

/// The "Change password" dialog: the new password, what went wrong, and whether the request is on
/// its way (then the button waits for the answer).
#[derive(Debug, Default)]
pub struct PasswordChange {
    password: Secret,
    error: Option<String>,
    sending: bool,
}

impl State {
    /// A sign-in is running; the shell reloads more often.
    pub fn busy(&self) -> bool {
        self.wizard.as_ref().is_some_and(wizard::State::busy)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    AddAccount,
    Wizard(wizard::Message),
    /// Opens the detail page of an account.
    Open(String),
    /// Back from a detail page to the list.
    Back,
    SyncNow,
    /// The "Show in apps" switch of one calendar or task list.
    SetShown {
        collection: String,
        shown: bool,
    },
    AskRemove,
    CancelRemove,
    Remove,
    /// "Change password…" on a CalDAV account's page.
    ChangePassword,
    /// The new password was typed. A [`Secret`], so that `Debug` of the message shows none.
    PasswordEdited(Secret),
    /// "Change password" in the dialog, or Enter in its field.
    SubmitPassword,
    ClosePassword,
}

/// What Escape means on this page right now: close the add-account dialog, the password dialog or
/// the remove confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if state.wizard.is_some() {
        Some(Message::Wizard(wizard::Message::Close))
    } else if state.password.is_some() {
        Some(Message::ClosePassword)
    } else if state.confirm_remove {
        Some(Message::CancelRemove)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::AddAccount => {
            state.wizard = Some(wizard::State::new());
            Action::None
        }
        Message::Wizard(wizard::Message::Close) => {
            state.wizard = None;
            Action::None
        }
        Message::Wizard(message) => match state.wizard.as_mut() {
            Some(wizard) => wizard::update(wizard, message, data),
            None => Action::None,
        },
        Message::Open(id) => {
            state.selected = Some(id);
            state.confirm_remove = false;
            state.password = None;
            Action::None
        }
        Message::Back => {
            state.selected = None;
            state.confirm_remove = false;
            state.password = None;
            Action::None
        }
        Message::SyncNow => Action::Send(Request::Sync),
        Message::SetShown { collection, shown } => Action::Send(Request::SetCollectionShown { collection, shown }),
        // Only an open detail page has an account to remove.
        Message::AskRemove => {
            state.confirm_remove = selected(state, data).is_some();
            Action::None
        }
        Message::CancelRemove => {
            state.confirm_remove = false;
            Action::None
        }
        Message::Remove => {
            state.confirm_remove = false;
            match state.selected.take() {
                Some(id) => Action::Send(Request::RemoveAccount { id }),
                None => Action::None,
            }
        }
        // Only a CalDAV account that is connected has a password to change.
        Message::ChangePassword => {
            state.password = selected(state, data).and_then(caldav_connection).map(|_| PasswordChange::default());
            Action::None
        }
        Message::PasswordEdited(password) => {
            if let Some(change) = state.password.as_mut() {
                change.password = password;
                change.error = None;
            }
            Action::None
        }
        Message::SubmitPassword => submit_password(state, data),
        Message::ClosePassword => {
            state.password = None;
            Action::None
        }
    }
}

/// The server and user name of a connected CalDAV account: what "Change password…" signs in at
/// again.
fn caldav_connection(account: &AccountInfo) -> Option<(&str, &str)> {
    match &account.state {
        AccountState::Connected { server, login_name, .. } if account.kind == "caldav" => {
            Some((server.as_str(), login_name.as_str()))
        }
        _ => None,
    }
}

/// Sends the new password: `ConnectCalDav` for the account's own server and user, which the core
/// takes as "replace the password" (the services chosen then are ignored, hidden calendars stay).
/// The dialog keeps no copy of the password.
fn submit_password(state: &mut State, data: &Data) -> Action {
    let Some(account) = selected(state, data) else { return Action::None };
    let Some((server, login_name)) = caldav_connection(account) else { return Action::None };
    let Some(change) = state.password.as_mut().filter(|change| !change.sending && !change.password.is_blank()) else {
        return Action::None;
    };
    change.sending = true;
    change.error = None;
    Action::Send(Request::ConnectCalDav {
        address: account.label.clone(),
        server: Some(server.to_string()),
        login_name: login_name.to_string(),
        password: std::mem::take(&mut change.password),
        calendars: true,
        tasks: true,
    })
}

/// The answer to a request the shell sent: the add-account dialog and the password dialog take the
/// answers to their own requests ([`wizard::answered`]). Returns whether one did; then the shell
/// shows no notice.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    if let Some(wizard) = state.wizard.as_mut() {
        return wizard::answered(wizard, result);
    }
    let Some(change) = state.password.as_mut().filter(|change| change.sending) else { return false };
    match result {
        Ok(Response::Connected { .. }) => state.password = None,
        Ok(Response::Error { message, .. }) | Err(message) => {
            change.sending = false;
            change.error = Some(message.clone());
        }
        Ok(_) => {
            change.sending = false;
            change.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string());
        }
    }
    true
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match selected(state, data) {
        Some(account) => detail(account, data, now),
        None => list(data, now),
    }
}

/// The page's dialog over the whole window: the add-account dialog, else the password dialog,
/// else the "Remove this account?" confirmation; `None` while none is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::FORM_MAX, Message::Wizard(wizard::Message::Close)));
    }
    if let Some(change) = &state.password {
        let account = selected(state, data)?;
        return Some(password_dialog(change, account));
    }
    if !state.confirm_remove {
        return None;
    }
    // With several accounts the dialog says which one goes.
    let account = selected(state, data)?;
    Some(confirm(
        tr("Remove this account?"),
        trf(
            "This removes {account} from Torro Workplace. Your data stays on the server.",
            &[("account", &account.label)],
        ),
        tr("Remove account"),
        Message::Remove,
        Message::CancelRemove,
    ))
}

/// "Change password": who it is for, the masked field, a refusal under it, "Cancel" and the primary
/// button right. A click next to the dialog cancels.
fn password_dialog<'a>(change: &'a PasswordChange, account: &'a AccountInfo) -> Element<'a, Message> {
    let can_send = !change.sending && !change.password.is_blank();
    let input = wizard::field("", change.password.expose())
        .secure(true)
        .on_input(|password| Message::PasswordEdited(Secret::new(password)))
        .on_submit_maybe(can_send.then_some(Message::SubmitPassword));
    let mut body = column![
        typo::text(typo::TITLE, tr("Change password")),
        typo::text(typo::BODY, trf("Enter the new password for {account}.", &[("account", &account.label)]))
            .style(theme::text_2),
        widgets::group(tr("Password"), vec![row_frame(input, false)], None),
    ]
    .spacing(space::L);
    if let Some(error) = &change.error {
        body = body.push(
            typo::text(typo::BODY, error.as_str())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        );
    }
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::ClosePassword)),
        widgets::text_button(
            None,
            tr("Change password"),
            style::button::primary,
            can_send.then_some(Message::SubmitPassword),
        ),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    dialog(body.push(buttons), widgets::POPOVER_WIDTH, Message::ClosePassword)
}

/// The header bar of the page: "Add account" on the list; on an account's detail its label with
/// a back button to the list.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    if let Some(account) = selected(state, data) {
        return Header { sub_page: Some((account.label.clone(), Message::Back)), actions: Vec::new() };
    }
    let add = match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), tr("Add account"), style::button::primary, Some(Message::AddAccount))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, tr("Add account"), style::button::primary, Some(Message::AddAccount))
        }
    };
    Header { sub_page: None, actions: vec![add] }
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The accounts with a detail page. An empty id is a sign-in in progress ([`pending`]).
pub fn listed(accounts: &[AccountInfo]) -> impl Iterator<Item = &AccountInfo> {
    accounts.iter().filter(|account| !account.id.is_empty())
}

/// The sign-ins in progress: entries without an id. The list shows them as a "Signing in…" row;
/// there is no account yet to open or remove.
pub fn pending(accounts: &[AccountInfo]) -> impl Iterator<Item = &AccountInfo> {
    accounts.iter().filter(|account| account.id.is_empty())
}

/// The name of a sign-in in progress: the server without scheme and trailing slash.
fn pending_name(account: &AccountInfo) -> &str {
    let label = account.label.as_str();
    label.split_once("://").map_or(label, |(_, rest)| rest).trim_end_matches('/')
}

/// The provider's display name; unknown providers show their id.
pub fn provider_name(kind: &str) -> String {
    match kind {
        "nextcloud" => tr("Nextcloud").to_string(),
        "caldav" => tr("CalDAV").to_string(),
        "github" => tr("GitHub").to_string(),
        other => other.to_string(),
    }
}

/// Word and tone of an account state. Status is never shown by colour alone: the word goes next
/// to the dot of the tone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    /// Waiting for the user or for the server.
    Attention,
    Error,
}

impl Tone {
    /// The dot that goes with the word.
    fn status(self) -> Status {
        match self {
            Tone::Ok => Status::Ok,
            Tone::Attention => Status::Attention,
            Tone::Error => Status::Error,
        }
    }
}

pub fn status_line(state: &AccountState) -> StatusLine {
    match state {
        AccountState::NotConnected => StatusLine { text: tr("Needs sign-in").to_string(), tone: Tone::Attention },
        AccountState::SigningIn { .. } => StatusLine { text: tr("Signing in…").to_string(), tone: Tone::Attention },
        AccountState::Connected { problem: Some(problem), .. } => {
            StatusLine { text: trf("Problem: {problem}", &[("problem", problem)]), tone: Tone::Error }
        }
        AccountState::Connected { problem: None, .. } => {
            StatusLine { text: tr("Connected").to_string(), tone: Tone::Ok }
        }
    }
}

/// The line under an account's name in the list: its status, and for a healthy connection when it
/// last synced ("Connected · Last synced 5 min ago").
pub fn account_detail(state: &AccountState, now: DateTime<Utc>) -> String {
    let status = status_line(state).text;
    match state {
        AccountState::Connected { last_sync, problem: None, .. } => {
            format!("{status} · {}", last_synced_text(*last_sync, now))
        }
        _ => status,
    }
}

/// The providers of `accounts`, each once, in the order they first appear: one group per provider.
fn providers<'a>(accounts: &[&'a AccountInfo]) -> Vec<&'a str> {
    let mut kinds: Vec<&str> = Vec::new();
    for account in accounts {
        if !kinds.contains(&account.kind.as_str()) {
            kinds.push(&account.kind);
        }
    }
    kinds
}

/// "Calendar", "Task list · Read-only": the line under a collection's name.
fn collection_detail(collection: &Collection) -> String {
    let kind = kind_name(collection.kind);
    if collection.writable { kind.to_string() } else { format!("{kind} · {}", tr("Read-only")) }
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago". A time in the future
/// (clock skew) counts as just now.
pub fn relative_time(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if minutes < 1 {
        tr("just now").to_string()
    } else if minutes < 60 {
        trf("{n} min ago", &[("n", &minutes)])
    } else if hours < 24 {
        trf("{n} h ago", &[("n", &hours)])
    } else if days == 1 {
        tr("yesterday").to_string()
    } else {
        trf("{n} days ago", &[("n", &days)])
    }
}

/// "Last synced 5 min ago", or "Not synced yet" when the account never synced.
pub fn last_synced_text(last_sync: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match last_sync {
        Some(at) => trf("Last synced {when}", &[("when", &relative_time(at, now))]),
        None => tr("Not synced yet").to_string(),
    }
}

/// Whether the apps show a collection: everything is shown unless it is hidden.
pub fn is_shown(settings: &Settings, collection: &str) -> bool {
    !settings.hidden_collections.iter().any(|hidden| hidden == collection)
}

fn kind_name(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Events => tr("Calendar"),
        CollectionKind::Todos => tr("Task list"),
        CollectionKind::Both => tr("Both"),
    }
}

/// The account whose detail page is open, if it is still listed.
fn selected<'a>(state: &State, data: &'a Data) -> Option<&'a AccountInfo> {
    let id = state.selected.as_deref()?;
    listed(&data.accounts).find(|account| account.id == id)
}

// ----- Views --------------------------------------------------------------------------------

fn list<'a>(data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    // The accounts first, then the sign-ins in progress, so a new row appears below the others.
    let entries: Vec<&AccountInfo> = listed(&data.accounts).chain(pending(&data.accounts)).collect();
    if entries.is_empty() {
        return empty_state(
            tr("No accounts yet."),
            tr("Add your Nextcloud or a mail provider to see calendars and tasks in the apps."),
        );
    }
    let groups = providers(&entries).into_iter().map(|kind| {
        let rows = entries
            .iter()
            .filter(|account| account.kind == kind)
            .map(|account| if account.id.is_empty() { pending_row(account) } else { account_row(account, now) });
        widgets::group(provider_name(kind), rows.collect(), None)
    });
    column(groups).spacing(space::XL).into()
}

/// A sign-in in progress: attention dot, the server and "Signing in…". Not pressable: there is
/// no account yet.
fn pending_row<'a>(account: &'a AccountInfo) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let dot = widgets::status_dot(line.tone.status());
    setting_row(Some(dot), pending_name(account), Some(line.text), Space::new())
}

/// One account in the list: status dot, label, status and last sync, and a chevron; the whole row
/// opens the detail page.
fn account_row<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let dot = widgets::status_dot(status_line(&account.state).tone.status());
    let row = setting_row(Some(dot), account.label.as_str(), Some(account_detail(&account.state, now)), chevron());
    pressable(row, Some(Message::Open(account.id.clone())))
}

/// The detail page: the name, then the groups for status, calendars and lists and the connection,
/// and the removal at the end.
fn detail<'a>(account: &'a AccountInfo, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let name = typo::text(typo::HEADLINE, account.label.as_str()).wrapping(Wrapping::None);
    let heading = column![
        container(name).width(Fill).clip(true),
        typo::text(typo::META, provider_name(&account.kind)).style(theme::text_2),
    ]
    .spacing(space::XS);
    let remove = widgets::text_button(None, tr("Remove account…"), style::button::danger, Some(Message::AskRemove));
    // A GitHub account has no calendars or lists (yet); what its token may do matters instead.
    let middle = match account.kind.as_str() {
        "github" => permissions_group(account.scopes.as_deref()),
        _ => collections_group(account, &data.settings),
    };
    column![heading, status_group(account, now), middle]
        .push(connection_group(account))
        .push(on_axis(remove))
        .spacing(space::XL)
        .into()
}

/// A text button moved left by its own padding, so its label stands on the page's content axis
/// (the name, the groups) instead of 10 to the right of it; the hover surface reaches into the margin.
fn on_axis<'a>(button: Element<'a, Message>) -> Element<'a, Message> {
    row![Space::new(), button].spacing(-style::BUTTON_PADDING.left).into()
}

/// The status as a dot and a word, when it last synced, and "Sync now" for a connected account.
fn status_group<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let (detail, sync) = match &account.state {
        AccountState::Connected { last_sync, .. } => (
            Some(last_synced_text(*last_sync, now)),
            Some(widgets::text_button(
                Some(Icon::RefreshCw),
                tr("Sync now"),
                style::button::secondary,
                Some(Message::SyncNow),
            )),
        ),
        AccountState::NotConnected | AccountState::SigningIn { .. } => (None, None),
    };
    let row = setting_row(Some(widgets::status_dot(line.tone.status())), line.text, detail, sync);
    widgets::group(tr("Status"), vec![row], None)
}

/// The calendars and task lists with their switches; before the first sync a sentence instead
/// (an empty group would be a bare frame).
fn collections_group<'a>(account: &'a AccountInfo, settings: &Settings) -> Element<'a, Message> {
    let title = tr("Calendars and lists");
    if account.collections.is_empty() {
        let hint = tr("No calendars or task lists yet. Use Sync now to fetch them from the server.");
        return column![widgets::section_title(title), typo::text(typo::BODY, hint).style(theme::text_2)]
            .spacing(space::S)
            .into();
    }
    let rows = account.collections.iter().map(|collection| collection_row(collection, settings)).collect();
    let footer = tr("Hidden calendars and lists stay on the server; the apps just don't show them.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// One calendar or task list: colour dot, name, kind and "Read-only", and the "Show in apps" switch.
fn collection_row<'a>(collection: &'a Collection, settings: &Settings) -> Element<'a, Message> {
    let id = collection.id.clone();
    let switch = toggler(is_shown(settings, &collection.id))
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |shown| Message::SetShown { collection: id.clone(), shown });
    let dot = color_dot(collection_color(collection.color.as_deref()), space::S);
    setting_row(Some(dot), collection.name.as_str(), Some(collection_detail(collection)), switch)
}

/// Server and user, and where the app password lives. Nothing to show without a server.
fn connection_group<'a>(account: &'a AccountInfo) -> Option<Element<'a, Message>> {
    let (server, user) = match &account.state {
        AccountState::Connected { server, login_name, .. } => (server, Some(login_name)),
        AccountState::SigningIn { server, .. } => (server, None),
        AccountState::NotConnected => return None,
    };
    let mut rows = vec![value_row(tr("Server"), server)];
    rows.extend(user.map(|user| value_row(tr("User"), user)));
    if account.kind == "caldav" && user.is_some() {
        let change =
            widgets::text_button(None, tr("Change password…"), style::button::secondary, Some(Message::ChangePassword));
        let stored = tr("Stored in the system keychain").to_string();
        rows.push(setting_row(None, tr("Password"), Some(stored), change));
    }
    let stored = match account.kind.as_str() {
        "github" => Some(tr("Token stored in the system keychain")),
        "caldav" => None,
        _ => Some(tr("App password stored in the system keychain")),
    };
    let footer = user.and(stored).map(str::to_string);
    Some(widgets::group(tr("Connection"), rows, footer))
}

/// What a GitHub token allows: a row per permission with a dot and a word (never colour alone),
/// and the scopes GitHub reported. A fine-grained token does not tell, so the group says what
/// it needs instead.
fn permissions_group<'a>(scopes: Option<&[String]>) -> Element<'a, Message> {
    let Some(scopes) = scopes else {
        let row = setting_row(
            None,
            tr("Fine-grained token"),
            Some(tr("GitHub does not report its permissions; they are set per repository.").to_string()),
            Space::new(),
        );
        let footer = tr("Closing issues needs Issues: Read and write. Pull requests need Pull requests: Read.");
        return widgets::group(tr("Permissions"), vec![row], Some(footer.to_string()));
    };
    let rows = github_permissions(scopes)
        .into_iter()
        .map(|permission| {
            let (status, detail) = match permission.allowed {
                true => (Status::Ok, tr("Allowed")),
                false => (Status::Attention, permission.missing),
            };
            setting_row(Some(widgets::status_dot(status)), permission.name, Some(detail.to_string()), Space::new())
        })
        .collect();
    let footer = match scopes {
        [] => tr("The token has no scopes. Change them on GitHub, then use Sync now.").to_string(),
        scopes => {
            trf("Token scopes: {scopes}. Change them on GitHub, then use Sync now.", &[("scopes", &scopes.join(", "))])
        }
    };
    widgets::group(tr("Permissions"), rows, Some(footer))
}

/// One thing a GitHub token allows or not, with the sentence that says what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    pub name: &'static str,
    pub allowed: bool,
    /// What it takes, shown when not allowed.
    pub missing: &'static str,
}

/// What a token with the OAuth `scopes` of a classic token or the GitHub CLI allows
/// (`docs/settings.md`, "GitHub: Rechte"): private repositories with `repo`, closing issues with
/// `repo` or `public_repo` (public repositories only), projects with `read:project` or `project`.
pub fn github_permissions(scopes: &[String]) -> Vec<Permission> {
    let has = |wanted: &[&str]| scopes.iter().any(|scope| wanted.contains(&scope.as_str()));
    vec![
        Permission {
            name: tr("Private repositories"),
            allowed: has(&["repo"]),
            missing: tr("Private repositories need the repo scope"),
        },
        Permission {
            name: tr("Close issues"),
            allowed: has(&["repo", "public_repo"]),
            missing: tr("Closing issues needs write access to issues"),
        },
        Permission {
            name: tr("Projects"),
            allowed: has(&["read:project", "project"]),
            missing: tr("Projects need the read:project permission"),
        },
    ]
}

/// A row with a value at the right in `text_2`, on one line.
fn value_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    setting_row(None, label, None, typo::text(typo::BODY, value).style(theme::text_2).wrapping(Wrapping::None))
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use torro_core::account::Secret;

    use super::*;

    fn connected(last_sync: Option<DateTime<Utc>>, problem: Option<&str>) -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.org".into(),
            login_name: "anna".into(),
            last_sync,
            problem: problem.map(str::to_string),
        }
    }

    fn account(id: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: id.into(),
            kind: "nextcloud".into(),
            label: "anna@cloud.example.org".into(),
            state,
            collections: vec![],
            scopes: None,
        }
    }

    #[test]
    fn relative_time_text() {
        let now = Utc::now();
        assert_eq!(relative_time(now, now), tr("just now"));
        assert_eq!(relative_time(now - TimeDelta::seconds(30), now), tr("just now"));
        assert_eq!(relative_time(now + TimeDelta::seconds(30), now), tr("just now"));
        assert_eq!(relative_time(now - TimeDelta::minutes(5), now), trf("{n} min ago", &[("n", &5)]));
        assert_eq!(relative_time(now - TimeDelta::hours(3), now), trf("{n} h ago", &[("n", &3)]));
        assert_eq!(relative_time(now - TimeDelta::hours(25), now), tr("yesterday"));
        assert_eq!(relative_time(now - TimeDelta::days(4), now), trf("{n} days ago", &[("n", &4)]));
    }

    #[test]
    fn last_synced_sentence() {
        let now = Utc::now();
        assert_eq!(last_synced_text(None, now), tr("Not synced yet"));
        let five_minutes = trf("{n} min ago", &[("n", &5)]);
        assert_eq!(
            last_synced_text(Some(now - TimeDelta::minutes(5)), now),
            trf("Last synced {when}", &[("when", &five_minutes)])
        );
    }

    #[test]
    fn status_text_per_state() {
        let connected_line = status_line(&connected(None, None));
        assert_eq!(connected_line.text, tr("Connected"));
        assert_eq!(connected_line.tone, Tone::Ok);
        assert_eq!(connected_line.tone.status(), Status::Ok);

        let problem = status_line(&connected(None, Some("Server unreachable")));
        assert_eq!(problem.text, trf("Problem: {problem}", &[("problem", &"Server unreachable")]));
        assert_eq!(problem.tone, Tone::Error);
        assert_eq!(problem.tone.status(), Status::Error);

        let signing_in = status_line(&AccountState::SigningIn {
            server: "https://cloud.example.org".into(),
            login_url: "https://cloud.example.org/login".into(),
        });
        assert_eq!(signing_in.text, tr("Signing in…"));
        assert_eq!(signing_in.tone, Tone::Attention);

        let not_connected = status_line(&AccountState::NotConnected);
        assert_eq!(not_connected.text, tr("Needs sign-in"));
        assert_eq!(not_connected.tone, Tone::Attention);
    }

    #[test]
    fn the_list_row_says_status_and_last_sync() {
        let now = Utc::now();
        let five_minutes = last_synced_text(Some(now - TimeDelta::minutes(5)), now);
        let healthy = connected(Some(now - TimeDelta::minutes(5)), None);
        assert_eq!(account_detail(&healthy, now), format!("{} · {five_minutes}", tr("Connected")));
        let broken = connected(Some(now), Some("Server unreachable"));
        assert_eq!(account_detail(&broken, now), status_line(&broken).text, "a problem needs the whole line");
        assert_eq!(account_detail(&AccountState::NotConnected, now), tr("Needs sign-in"));
    }

    #[test]
    fn one_group_per_provider_in_order_of_appearance() {
        let mut github = account("github:anna", connected(None, None));
        github.kind = "github".into();
        let (a, b) = (account("nextcloud:a", connected(None, None)), account("nextcloud:b", connected(None, None)));
        assert_eq!(providers(&[&a, &github, &b]), ["nextcloud", "github"]);
    }

    #[test]
    fn collections_say_kind_and_read_only() {
        let mut collection = Collection {
            id: "work".into(),
            name: "Work".into(),
            kind: CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        };
        assert_eq!(collection_detail(&collection), tr("Calendar"));
        collection.writable = false;
        assert_eq!(collection_detail(&collection), format!("{} · {}", tr("Calendar"), tr("Read-only")));
    }

    #[test]
    fn dialogs_are_window_layers() {
        let data = Data { accounts: vec![account("nextcloud:anna", connected(None, None))], ..Data::default() };
        let mut state = State::default();
        assert!(overlay(&state, &data).is_none());
        update(&mut state, Message::AskRemove, &data);
        assert!(overlay(&state, &data).is_none(), "nothing to remove without an open detail page");
        update(&mut state, Message::Open("nextcloud:anna".into()), &data);
        update(&mut state, Message::AskRemove, &data);
        assert!(overlay(&state, &data).is_some());
        update(&mut state, Message::CancelRemove, &data);
        update(&mut state, Message::AddAccount, &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Wizard(wizard::Message::Close))));
    }

    #[test]
    fn pages_build_for_list_and_detail() {
        let now = Utc::now();
        let mut anna = account("nextcloud:anna@cloud.example.org", connected(Some(now), None));
        anna.collections = vec![Collection {
            id: "work".into(),
            name: "Work".into(),
            kind: CollectionKind::Todos,
            color: Some("#5B8CFF".into()),
            writable: false,
            source: None,
        }];
        let data =
            Data { accounts: vec![anna, account("nextcloud:ben", AccountState::NotConnected)], ..Data::default() };
        let mut state = State::default();
        drop(view(&state, &data));
        update(&mut state, Message::Open("nextcloud:anna@cloud.example.org".into()), &data);
        drop(view(&state, &data));
        drop(view(&State::default(), &Data::default()));
    }

    fn two_accounts() -> Data {
        Data {
            accounts: vec![
                account("nextcloud:anna@work.example.org", connected(None, None)),
                account("nextcloud:anna@home.example.org", connected(None, None)),
            ],
            ..Data::default()
        }
    }

    #[test]
    fn removing_acts_on_the_open_account_only() {
        let data = two_accounts();
        let mut state = State::default();
        update(&mut state, Message::Open("nextcloud:anna@home.example.org".into()), &data);
        update(&mut state, Message::AskRemove, &data);
        assert!(state.confirm_remove);
        let action = update(&mut state, Message::Remove, &data);
        assert!(
            matches!(action, Action::Send(Request::RemoveAccount { ref id }) if id == "nextcloud:anna@home.example.org"),
            "{action:?}"
        );
        assert!(state.selected.is_none() && !state.confirm_remove, "back on the list");
        let again = update(&mut state, Message::Remove, &data);
        assert!(matches!(again, Action::None), "no account open: nothing to remove");
    }

    #[test]
    fn adding_is_possible_with_accounts() {
        let data = two_accounts();
        let state = State::default();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini] {
            assert_eq!(header(&state, &data, class).actions.len(), 1, "{class:?} offers Add account");
        }
    }

    #[test]
    fn a_sign_in_in_progress_is_a_row_not_an_account() {
        let signing_in = AccountState::SigningIn {
            server: "https://cloud.example.org/".into(),
            login_url: "https://cloud.example.org/login".into(),
        };
        let mut waiting = account("", signing_in);
        waiting.label = "https://cloud.example.org/".into();
        let mut data = two_accounts();
        data.accounts.push(waiting.clone());
        assert_eq!(listed(&data.accounts).count(), 2);
        assert_eq!(pending(&data.accounts).count(), 1);
        assert_eq!(pending_name(&waiting), "cloud.example.org");
        assert_eq!(status_line(&waiting.state).text, tr("Signing in…"));
        drop(view(&State::default(), &data));
        // Only a sign-in: a row instead of the empty state.
        drop(view(&State::default(), &Data { accounts: vec![waiting], ..Data::default() }));
    }

    #[test]
    fn provider_names() {
        assert_eq!(provider_name("nextcloud"), tr("Nextcloud"));
        assert_eq!(provider_name("github"), tr("GitHub"));
        assert_eq!(provider_name("slack"), "slack");
    }

    #[test]
    fn github_permissions_follow_the_scopes() {
        let allowed = |scopes: &[&str]| -> Vec<bool> {
            let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
            github_permissions(&scopes).iter().map(|p| p.allowed).collect()
        };
        assert_eq!(allowed(&[]), [false, false, false]);
        assert_eq!(allowed(&["repo", "read:org"]), [true, true, false]);
        assert_eq!(allowed(&["public_repo"]), [false, true, false]);
        assert_eq!(allowed(&["repo", "read:project"]), [true, true, true]);
        let close = &github_permissions(&[])[1];
        assert_eq!(close.missing, tr("Closing issues needs write access to issues"));
    }

    #[test]
    fn the_github_detail_page_builds_with_and_without_scopes() {
        let mut github = account("github:mahype", connected(Some(Utc::now()), None));
        github.kind = "github".into();
        for scopes in [Some(vec!["repo".to_string()]), Some(vec![]), None] {
            github.scopes = scopes;
            let data = Data { accounts: vec![github.clone()], ..Data::default() };
            let state = State { selected: Some(github.id.clone()), ..State::default() };
            drop(view(&state, &data));
        }
    }

    #[test]
    fn the_dialog_takes_only_its_own_answers() {
        let mut state = State::default();
        assert!(!answered(&mut state, &Err("no template".into())), "no dialog: the shell shows it");
        update(&mut state, Message::AddAccount, &Data::default());
        assert!(!answered(&mut state, &Err("no template".into())), "the dialog waits for nothing yet");
    }

    #[test]
    fn lists_only_accounts_with_an_id() {
        let accounts = vec![
            account("", AccountState::NotConnected),
            account("nextcloud:anna@cloud.example.org", connected(None, None)),
        ];
        let ids: Vec<&str> = listed(&accounts).map(|account| account.id.as_str()).collect();
        assert_eq!(ids, ["nextcloud:anna@cloud.example.org"]);
    }

    #[test]
    fn a_collection_is_shown_unless_hidden() {
        let base = Settings { hidden_collections: vec!["a".into()], ..Settings::default() };
        assert!(!is_shown(&base, "a"));
        assert!(is_shown(&base, "b"));
    }

    #[test]
    fn the_switch_sends_only_its_collection() {
        let data = Data::default();
        for shown in [true, false] {
            let message = Message::SetShown { collection: "a".into(), shown };
            let action = update(&mut State::default(), message, &data);
            assert!(
                matches!(action, Action::Send(Request::SetCollectionShown { ref collection, shown: s })
                    if collection == "a" && s == shown),
                "{action:?}"
            );
        }
    }

    // ----- A mail provider's account (task M7.2) -------------------------------------------------

    const CALDAV: &str = "caldav:anna@mailbox.org@dav.mailbox.org";

    fn caldav_data() -> Data {
        let mut anna = account(
            CALDAV,
            AccountState::Connected {
                server: "https://dav.mailbox.org/".into(),
                login_name: "anna@mailbox.org".into(),
                last_sync: None,
                problem: None,
            },
        );
        anna.kind = "caldav".into();
        anna.label = "anna@mailbox.org".into();
        Data { accounts: vec![anna], ..Data::default() }
    }

    #[test]
    fn a_caldav_account_is_called_caldav() {
        assert_eq!(provider_name("caldav"), tr("CalDAV"));
        let data = caldav_data();
        let mut state = State::default();
        drop(view(&state, &data));
        update(&mut state, Message::Open(CALDAV.into()), &data);
        drop(view(&state, &data));
    }

    #[test]
    fn change_password_asks_first_and_sends_the_same_server_and_user() {
        let data = caldav_data();
        let mut state = State::default();
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_none(), "no account open");
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some() && overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::ClosePassword)));

        assert!(matches!(update(&mut state, Message::SubmitPassword, &data), Action::None), "no password yet");
        update(&mut state, Message::PasswordEdited(Secret::new("  ")), &data);
        assert!(matches!(update(&mut state, Message::SubmitPassword, &data), Action::None), "blank");
        update(&mut state, Message::PasswordEdited(Secret::new("new-secret")), &data);
        let action = update(&mut state, Message::SubmitPassword, &data);
        match &action {
            Action::Send(Request::ConnectCalDav { address, server, login_name, password, .. }) => {
                assert_eq!(address, "anna@mailbox.org");
                assert_eq!(server.as_deref(), Some("https://dav.mailbox.org/"));
                assert_eq!(login_name, "anna@mailbox.org");
                assert_eq!(password.expose(), "new-secret");
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
        assert!(!format!("{action:?}").contains("new-secret") && !format!("{state:?}").contains("new-secret"));
        assert!(
            matches!(update(&mut state, Message::SubmitPassword, &data), Action::None),
            "once; the dialog keeps no copy to send twice"
        );
    }

    #[test]
    fn the_password_dialog_takes_its_answer_and_shows_a_refusal() {
        let data = caldav_data();
        let mut state = State::default();
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(!answered(&mut state, &Err("anything".into())), "it waits for nothing yet");

        update(&mut state, Message::PasswordEdited(Secret::new("wrong")), &data);
        update(&mut state, Message::SubmitPassword, &data);
        assert!(answered(&mut state, &Err(tr("Password was refused").into())));
        let change = state.password.as_ref().expect("the dialog stays open");
        assert_eq!((change.error.as_deref(), change.sending), (Some(tr("Password was refused")), false));
        drop(overlay(&state, &data));

        update(&mut state, Message::PasswordEdited(Secret::new("right")), &data);
        assert!(state.password.as_ref().is_some_and(|change| change.error.is_none()), "an edit clears it");
        update(&mut state, Message::SubmitPassword, &data);
        let connected = Response::Connected { account: data.accounts[0].clone(), calendars: true, tasks: true };
        assert!(answered(&mut state, &Ok(connected)));
        assert!(state.password.is_none(), "done: the dialog closes");
    }

    #[test]
    fn only_a_connected_caldav_account_has_a_password_to_change() {
        let mut data = two_accounts();
        let mut state = State::default();
        update(&mut state, Message::Open("nextcloud:anna@work.example.org".into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_none(), "Nextcloud signs in in the browser");
        data.accounts.extend(caldav_data().accounts);
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some());
        update(&mut state, Message::Back, &data);
        assert!(state.password.is_none(), "leaving the page closes it");
    }
}
