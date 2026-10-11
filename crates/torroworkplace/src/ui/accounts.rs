//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`): the accounts as rows of one settings
//! group per provider (status dot, label on one line, status and last sync below, chevron), "Add
//! account" in the header bar; a detail page per account with groups for the status ("Sync
//! now"), the calendars and task lists (a switch each for showing them in the apps, read-only
//! ones marked) and the connection, and "Remove account…" behind a confirmation. Empty: a
//! sentence about what to add. The add flow is `wizard.rs` (task S.6); it and the confirmation
//! are dialogs over the whole window ([`overlay`]).
//!
//! Requests: `Sync`, `RemoveAccount { id }`, and `SetCollectionShown { collection, shown }` for
//! the switches. The pure parts (relative time, status text, whether a collection is shown,
//! which accounts are listed) are plain functions with unit tests.

use chrono::{DateTime, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, toggler};
use iced::{Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Status, collection_color, color_dot, empty_state};
use torro_gui::{style, typo};
use torro_ipc::{AccountInfo, Request};
use torro_pim::{AccountState, Collection, CollectionKind};

use super::{Action, Data, Header, chevron, confirm, dialog, pressable, setting_row, wizard};

#[derive(Debug, Default)]
pub struct State {
    /// The add-account dialog while it is open.
    pub wizard: Option<wizard::State>,
    /// The account whose detail page is open (its `AccountInfo::id`); `None` shows the list.
    pub selected: Option<String>,
    /// The "Remove this account?" confirmation is open.
    pub confirm_remove: bool,
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
}

/// What Escape means on this page right now: close the add-account dialog or the remove
/// confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if state.wizard.is_some() {
        Some(Message::Wizard(wizard::Message::Close))
    } else if state.confirm_remove {
        Some(Message::CancelRemove)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::AddAccount => {
            state.wizard = Some(wizard::State::default());
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
            Action::None
        }
        Message::Back => {
            state.selected = None;
            state.confirm_remove = false;
            Action::None
        }
        Message::SyncNow => Action::Send(Request::Sync),
        Message::SetShown { collection, shown } => Action::Send(Request::SetCollectionShown { collection, shown }),
        Message::AskRemove => {
            state.confirm_remove = true;
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
    }
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match selected(state, data) {
        Some(account) => detail(account, data, now),
        None => list(data, now),
    }
}

/// The page's dialog over the whole window: the add-account dialog, else the "Remove this
/// account?" confirmation; `None` while neither is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::FORM_MAX, Message::Wizard(wizard::Message::Close)));
    }
    state.confirm_remove.then(|| {
        confirm(
            tr("Remove this account?"),
            tr("This only removes the account from Torro Workplace. Your data stays on the server.").to_string(),
            tr("Remove account"),
            Message::Remove,
            Message::CancelRemove,
        )
    })
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

/// The accounts the list shows. An empty id is a sign-in in progress; the wizard shows that.
pub fn listed(accounts: &[AccountInfo]) -> impl Iterator<Item = &AccountInfo> {
    accounts.iter().filter(|account| !account.id.is_empty())
}

/// The provider's display name; unknown providers show their id.
pub fn provider_name(kind: &str) -> String {
    match kind {
        "nextcloud" => tr("Nextcloud").to_string(),
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
    let accounts: Vec<&AccountInfo> = listed(&data.accounts).collect();
    if accounts.is_empty() {
        return empty_state(tr("No accounts yet."), tr("Add your Nextcloud to see calendars and tasks in the apps."));
    }
    let groups = providers(&accounts).into_iter().map(|kind| {
        let rows = accounts.iter().filter(|account| account.kind == kind).map(|account| account_row(account, now));
        widgets::group(provider_name(kind), rows.collect(), None)
    });
    column(groups).spacing(space::XL).into()
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
    column![heading, status_group(account, now), collections_group(account, &data.settings)]
        .push(connection_group(account))
        .push(row![remove])
        .spacing(space::XL)
        .into()
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
    let footer = user.map(|_| tr("App password stored in the system keychain").to_string());
    Some(widgets::group(tr("Connection"), rows, footer))
}

/// A row with a value at the right in `text_2`, on one line.
fn value_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    setting_row(None, label, None, typo::text(typo::BODY, value).style(theme::text_2).wrapping(Wrapping::None))
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

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
        };
        assert_eq!(collection_detail(&collection), tr("Calendar"));
        collection.writable = false;
        assert_eq!(collection_detail(&collection), format!("{} · {}", tr("Calendar"), tr("Read-only")));
    }

    #[test]
    fn dialogs_are_window_layers() {
        let data = Data::default();
        let mut state = State::default();
        assert!(overlay(&state, &data).is_none());
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
        }];
        let data =
            Data { accounts: vec![anna, account("nextcloud:ben", AccountState::NotConnected)], ..Data::default() };
        let mut state = State::default();
        drop(view(&state, &data));
        update(&mut state, Message::Open("nextcloud:anna@cloud.example.org".into()), &data);
        drop(view(&state, &data));
        drop(view(&State::default(), &Data::default()));
    }

    #[test]
    fn provider_names() {
        assert_eq!(provider_name("nextcloud"), tr("Nextcloud"));
        assert_eq!(provider_name("github"), "github");
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
}
