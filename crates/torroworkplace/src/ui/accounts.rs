//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`): the accounts as cards (provider,
//! label, status as symbol + word, last sync, chevron), "Add account" in the header bar; a detail
//! page per account with the status ("Sync now"), the calendars and task lists (a switch each
//! for showing them in the apps, read-only ones marked), the connection, and "Remove account…"
//! behind a confirmation. Empty: a sentence about what to add. The add flow is `wizard.rs`
//! (task S.6), shown as a dialog over the page.
//!
//! Requests: `Sync`, `RemoveAccount { id }`, and `SetCollectionShown { collection, shown }` for
//! the switches. The pure parts (relative time, status text, whether a collection is shown,
//! which accounts are listed) are plain functions with unit tests.

use chrono::{DateTime, Utc};
use iced::widget::{button, column, container, row, space, text, toggler};
use iced::{Alignment, Background, Color, Element, Length, border};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_core::theme::{STATUS_ERROR, STATUS_OK};
use torro_gui::theme;
use torro_gui::widgets::{collection_color, color_dot, empty_state, modal, section_title};
use torro_ipc::{AccountInfo, Request};
use torro_pim::{AccountState, Collection, CollectionKind};

use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::{style, widgets};

use super::{Action, Data, Header, wizard};

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
    let page = match selected(state, data) {
        Some(account) => detail(account, data, now),
        None => list(data, now),
    };
    match &state.wizard {
        Some(wizard) => {
            modal(page, wizard::view(wizard, data).map(Message::Wizard), 520.0, Message::Wizard(wizard::Message::Close))
        }
        None if state.confirm_remove => modal(page, remove_dialog(), 420.0, Message::CancelRemove),
        None => page,
    }
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

/// Symbol, word and tone of an account state. Status is never shown by colour alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub symbol: &'static str,
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

pub fn status_line(state: &AccountState) -> StatusLine {
    match state {
        AccountState::NotConnected => {
            StatusLine { symbol: "!", text: tr("Needs sign-in").to_string(), tone: Tone::Attention }
        }
        AccountState::SigningIn { .. } => {
            StatusLine { symbol: "◌", text: tr("Signing in…").to_string(), tone: Tone::Attention }
        }
        AccountState::Connected { problem: Some(problem), .. } => {
            StatusLine { symbol: "✕", text: trf("Problem: {problem}", &[("problem", problem)]), tone: Tone::Error }
        }
        AccountState::Connected { problem: None, .. } => {
            StatusLine { symbol: "✓", text: tr("Connected").to_string(), tone: Tone::Ok }
        }
    }
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
    let body: Element<'a, Message> = if accounts.is_empty() {
        empty_state(tr("No accounts yet."), tr("Add your Nextcloud to see calendars and tasks in the apps."))
    } else {
        column(accounts.into_iter().map(|account| card(account, now))).spacing(8).width(Length::Fill).into()
    };
    body
}

/// One account in the list: provider, label, status, last sync, and a chevron.
fn card<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let mut details = column![
        text(provider_name(&account.kind)).size(12).color(theme::MUTED),
        text(account.label.as_str()).size(16).color(theme::TEXT).wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        status_label(&status_line(&account.state)),
    ]
    .spacing(4)
    .width(Length::Fill);
    if let AccountState::Connected { last_sync, .. } = &account.state {
        details = details.push(text(last_synced_text(*last_sync, now)).size(12).color(theme::MUTED));
    }
    let content = row![details, text("›").size(18).color(theme::MUTED)].spacing(8).align_y(Alignment::Center);
    button(content)
        .style(card_button)
        .padding(16)
        .width(Length::Fill)
        .on_press(Message::Open(account.id.clone()))
        .into()
}

/// The detail page: status, calendars and lists, connection, and removal.
fn detail<'a>(account: &'a AccountInfo, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    let heading = column![
        text(account.label.as_str()).size(18).color(theme::TEXT).wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        text(provider_name(&account.kind)).size(14).color(theme::MUTED),
    ]
    .spacing(4);

    let mut sections = column![status_group(account, now), collections_group(account, &data.settings)].spacing(32);
    if let Some(connection) = connection_group(account) {
        sections = sections.push(connection);
    }
    let remove = button(text(tr("Remove account…"))).style(danger_text).padding([8, 12]).on_press(Message::AskRemove);
    sections = sections.push(remove);

    let page = column![heading, sections].spacing(24).width(Length::Fill);
    container(page).width(Length::Fill).max_width(640).into()
}

fn status_group<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let mut head = row![status_label(&status_line(&account.state))].align_y(Alignment::Center).width(Length::Fill);
    let mut body = column![].spacing(8);
    if let AccountState::Connected { last_sync, .. } = &account.state {
        head = head
            .push(space::horizontal())
            .push(button(text(tr("Sync now"))).style(button::secondary).padding([8, 12]).on_press(Message::SyncNow));
        body = body.push(text(last_synced_text(*last_sync, now)).size(14).color(theme::MUTED));
    }
    group(tr("Status"), column![head, body].spacing(8), None)
}

fn collections_group<'a>(account: &'a AccountInfo, settings: &Settings) -> Element<'a, Message> {
    let body: Element<'a, Message> = if account.collections.is_empty() {
        text(tr("No calendars or task lists yet. Use Sync now to fetch them from the server."))
            .size(14)
            .color(theme::MUTED)
            .into()
    } else {
        column(account.collections.iter().map(|collection| collection_row(collection, settings))).spacing(12).into()
    };
    group(
        tr("Calendars and lists"),
        body,
        Some(tr("Hidden calendars and lists stay on the server; the apps just don't show them.")),
    )
}

/// One calendar or task list: colour dot, name, kind, "Read-only", and the "Show in apps" switch.
fn collection_row<'a>(collection: &'a Collection, settings: &Settings) -> Element<'a, Message> {
    let mut meta = row![text(kind_name(collection.kind)).size(12).color(theme::MUTED)].spacing(8);
    if !collection.writable {
        meta = meta.push(text(tr("Read-only")).size(12).color(theme::MUTED));
    }
    let info = column![text(collection.name.as_str()).size(14).color(theme::TEXT), meta].spacing(4).width(Length::Fill);
    let id = collection.id.clone();
    let switch = toggler(is_shown(settings, &collection.id))
        .label(tr("Show in apps"))
        .on_toggle(move |shown| Message::SetShown { collection: id.clone(), shown });
    row![color_dot(collection_color(collection.color.as_deref()), 12.0), info, switch]
        .spacing(12)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// Server and user, and where the app password lives. Nothing to show without a server.
fn connection_group<'a>(account: &'a AccountInfo) -> Option<Element<'a, Message>> {
    let mut rows = column![].spacing(8);
    match &account.state {
        AccountState::Connected { server, login_name, .. } => {
            rows = rows
                .push(key_value(tr("Server"), server.as_str()))
                .push(key_value(tr("User"), login_name.as_str()))
                .push(text(tr("App password stored in the system keychain")).size(14).color(theme::MUTED));
        }
        AccountState::SigningIn { server, .. } => {
            rows = rows.push(key_value(tr("Server"), server.as_str()));
        }
        AccountState::NotConnected => return None,
    }
    Some(group(tr("Connection"), rows, None))
}

fn key_value<'a>(key: &'static str, value: &'a str) -> Element<'a, Message> {
    row![text(key).size(14).color(theme::MUTED).width(96), text(value).size(14).color(theme::TEXT)].spacing(8).into()
}

/// A group: a small heading, a card with the rows, and an optional sentence below it.
fn group<'a>(title: &str, body: impl Into<Element<'a, Message>>, footer: Option<&'static str>) -> Element<'a, Message> {
    let card = container(body).padding(16).width(Length::Fill).style(surface_card);
    let mut group = column![section_title(title), card].spacing(8).width(Length::Fill);
    if let Some(footer) = footer {
        group = group.push(text(footer).size(14).color(theme::MUTED));
    }
    group.into()
}

/// The confirmation for removing the account. The consequence is stated, the button says what
/// it does, and "Cancel" is next to it.
fn remove_dialog<'a>() -> Element<'a, Message> {
    let buttons = row![
        space::horizontal(),
        button(text(tr("Cancel"))).style(button::secondary).padding([8, 12]).on_press(Message::CancelRemove),
        button(text(tr("Remove account"))).style(button::danger).padding([8, 12]).on_press(Message::Remove),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Fill);
    let content = column![
        text(tr("Remove this account?")).size(18).color(theme::TEXT),
        text(tr("This only removes the account from Torro Workplace. Your data stays on the server."))
            .size(14)
            .color(theme::MUTED),
        buttons,
    ]
    .spacing(16)
    .padding(24)
    .width(Length::Fill);
    container(content).style(surface_card).into()
}

/// Symbol and word in the status colour.
fn status_label<'a>(line: &StatusLine) -> Element<'a, Message> {
    text(format!("{}  {}", line.symbol, line.text)).size(14).color(tone_color(line.tone)).into()
}

fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Ok => theme::color(STATUS_OK),
        Tone::Attention => theme::attention(),
        Tone::Error => theme::color(STATUS_ERROR),
    }
}

/// A card: raised surface, hairline border, radius 12.
fn surface_card(_theme: &iced::Theme) -> container::Style {
    container::Style {
        text_color: Some(theme::TEXT),
        background: Some(Background::Color(theme::SURFACE)),
        border: border::rounded(12).color(theme::LINE).width(1),
        ..container::Style::default()
    }
}

/// A list row that is a card itself; it lifts on hover and press.
fn card_button(app_theme: &iced::Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => theme::SURFACE_2,
        _ => theme::SURFACE,
    };
    button::Style {
        background: Some(Background::Color(background)),
        border: border::rounded(12).color(theme::LINE).width(1),
        ..button::text(app_theme, status)
    }
}

/// A text button in the error colour, for "Remove account…".
fn danger_text(app_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style { text_color: theme::color(STATUS_ERROR), ..button::text(app_theme, status) }
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
        assert_eq!(connected_line.symbol, "✓");

        let problem = status_line(&connected(None, Some("Server unreachable")));
        assert_eq!(problem.text, trf("Problem: {problem}", &[("problem", &"Server unreachable")]));
        assert_eq!(problem.tone, Tone::Error);

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
