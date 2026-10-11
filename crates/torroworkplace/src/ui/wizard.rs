//! The add-account dialog (task S.6 in `docs/tasks/steuerzentrale.md`).
//!
//! Steps, with the subtitle changing per step, errors bottom left, "Cancel" and the primary
//! button bottom right (like TorroMail's setup wizard):
//!
//! 1. **Provider:** a catalog – Nextcloud (calendars, tasks) enabled; GitHub (issues,
//!    projects), Slack and AI providers listed as "Coming soon", disabled.
//! 2. **Server** (Nextcloud): the server address; Enter or "Continue" sends
//!    `Request::ConnectNextcloud { server }`.
//! 3. **Sign in:** "Nextcloud handles the sign-in. You sign in on its own page; Torro
//!    Workplace never sees your password." The browser opens (the core does that); the login
//!    URL from `data.accounts` (`AccountState::SigningIn`) is a fallback link to copy.
//!    While signing in, [`State::busy`] is true and the shell reloads every second.
//! 4. **Done:** as soon as an account in `data.accounts` is `Connected` that was not connected
//!    when the sign-in started: "Your calendars and task lists are ready." and "Done".
//!
//! Detection: [`update`] calls [`observe`] for every message. The shell also has to call
//! [`observe`] on every reload of `data` while a sign-in runs (reloads do not reach
//! `wizard::update` on their own, so `accounts.rs` must call `observe` from its own update).
//! [`subscription`] is the Escape key; `accounts.rs` adds it while the dialog is open.
//!
//! `Message::Close` is handled by `accounts.rs` (closes the dialog); Cancel, Done, Escape and a
//! click next to the dialog send it.
//!
//! [`view`] is the dialog's content only; `accounts::overlay` puts it on the dialog surface over
//! the whole window (`super::dialog`). The catalog is a settings group: a row per provider, the
//! available one with a chevron, the others with a "Coming soon" tag.

use iced::widget::space as blank;
use iced::widget::{column, row, text, text_input};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::theme::{self, palette, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{AccountInfo, Request};
use torro_pim::AccountState;

use super::{Action, Data, chevron, pressable, setting_row};

/// The step the dialog is on. Each step has its own subtitle and footer button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Provider,
    Server,
    SigningIn,
    Done,
}

/// A service in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Nextcloud,
    GitHub,
    Slack,
    Ai,
}

impl Provider {
    /// Only Nextcloud can be set up so far; the others are listed so the catalog shows where
    /// the suite is heading.
    fn is_enabled(self) -> bool {
        matches!(self, Provider::Nextcloud)
    }
}

#[derive(Debug, Default)]
pub struct State {
    step: Step,
    /// The server address as typed (Nextcloud).
    server: String,
    /// Ids of the accounts that were already connected when the sign-in started. Only an
    /// account that is not in this list finishes the sign-in, so an existing account does not
    /// end it at once.
    connected_before: Vec<String>,
    /// A sign-in was started and has not finished.
    pub signing_in: bool,
    /// Error text for the footer: the shell's last error while signing in.
    error: Option<String>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.signing_in
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Cancel, Done, Escape or a click next to the dialog.
    Close,
    /// A provider card was pressed; disabled providers do nothing.
    ChooseProvider(Provider),
    ServerChanged(String),
    /// "Continue", or Enter in the server field.
    Continue,
    /// "Open the page again".
    OpenLoginPage,
    /// The login link field is read-only: typing in it changes nothing.
    LoginLinkEdited,
}

/// Checks the sign-in against `data` and moves it to Done when a new account is connected.
///
/// Call it on every reload of `data` while the dialog is open. Only the `SigningIn` step
/// reacts; in the other steps it does nothing. While signing in it also copies the shell's
/// error (for example "daemon not running") into the footer.
pub fn observe(state: &mut State, data: &Data) {
    if state.step != Step::SigningIn {
        return;
    }
    let connected = data.accounts.iter().any(|account| is_connected(account) && !was_connected(state, account));
    if connected {
        state.step = Step::Done;
        state.signing_in = false;
        state.error = None;
        state.connected_before.clear();
    } else {
        state.error = data.error.clone();
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    // Every message first looks at the latest data, so a finished sign-in is noticed at once.
    observe(state, data);
    match message {
        Message::Close => Action::None,
        Message::ChooseProvider(provider) => {
            if state.step == Step::Provider && provider.is_enabled() {
                state.step = Step::Server;
            }
            Action::None
        }
        Message::ServerChanged(server) => {
            state.server = server;
            Action::None
        }
        Message::Continue => {
            if state.step != Step::Server || !can_continue(state) {
                return Action::None;
            }
            let server = state.server.trim().to_string();
            state.connected_before = data.accounts.iter().filter(|a| is_connected(a)).map(|a| a.id.clone()).collect();
            state.step = Step::SigningIn;
            state.signing_in = true;
            state.error = None;
            Action::Send(Request::ConnectNextcloud { server })
        }
        Message::OpenLoginPage => match (state.step, login_url(data)) {
            (Step::SigningIn, Some(url)) => Action::OpenUrl(url.to_string()),
            _ => Action::None,
        },
        Message::LoginLinkEdited => Action::None,
    }
}

/// Continue is only possible with a server address (spaces do not count).
fn can_continue(state: &State) -> bool {
    !state.server.trim().is_empty()
}

fn is_connected(account: &AccountInfo) -> bool {
    matches!(account.state, AccountState::Connected { .. })
}

fn was_connected(state: &State, account: &AccountInfo) -> bool {
    state.connected_before.contains(&account.id)
}

/// The login URL of the account that is signing in, if the core reports one.
fn login_url(data: &Data) -> Option<&str> {
    data.accounts.iter().find_map(|account| match &account.state {
        AccountState::SigningIn { login_url, .. } => Some(login_url.as_str()),
        _ => None,
    })
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let header = column![
        typo::text(typo::TITLE, tr("Add account")),
        typo::text(typo::BODY, subtitle(state.step)).style(theme::text_2)
    ]
    .spacing(space::XS);
    let body = match state.step {
        Step::Provider => provider_step(),
        Step::Server => server_step(state),
        Step::SigningIn => signing_in_step(data),
        Step::Done => typo::text(typo::BODY, tr("Your calendars and task lists are ready.")).into(),
    };
    column![header, body, footer(state)].spacing(space::XL).into()
}

/// The line under the title; it names what the current step asks for.
fn subtitle(step: Step) -> &'static str {
    match step {
        Step::Provider => tr("Choose a service to connect."),
        Step::Server => tr("Enter the address of your Nextcloud."),
        Step::SigningIn => tr("Sign in with Nextcloud in your browser."),
        Step::Done => tr("Nextcloud is connected."),
    }
}

/// The catalog: a group with one row per provider, the disabled ones marked "Coming soon".
fn provider_step() -> Element<'static, Message> {
    let rows = vec![
        provider_row(Provider::Nextcloud, "Nextcloud", tr("Calendars and tasks")),
        provider_row(Provider::GitHub, "GitHub", tr("Issues and projects")),
        provider_row(Provider::Slack, "Slack", tr("Mentions and messages")),
        provider_row(Provider::Ai, tr("AI providers"), tr("Summaries and agents")),
    ];
    widgets::group(tr("Services"), rows, None)
}

/// A provider: name and what it brings; the available one is a row to press with a chevron, the
/// others are plain rows with a tag.
fn provider_row(provider: Provider, name: &'static str, summary: &'static str) -> Element<'static, Message> {
    if provider.is_enabled() {
        let row = setting_row(None, name, Some(summary.to_string()), chevron());
        pressable(row, Some(Message::ChooseProvider(provider)))
    } else {
        setting_row(None, name, Some(summary.to_string()), widgets::pill(None, tr("Coming soon")))
    }
}

/// A text field in the look of the suite: `BODY`, 32 high.
fn field<'a>(placeholder: &str, value: &str) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .font(typo::BODY.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
}

/// The server address field and Continue; Enter in the field works like Continue.
fn server_step(state: &State) -> Element<'_, Message> {
    field("cloud.example.org", &state.server)
        .on_input(Message::ServerChanged)
        .on_submit_maybe(can_continue(state).then_some(Message::Continue))
        .into()
}

/// The explanation, the waiting line, the login link to copy and a button to open it again.
fn signing_in_step(data: &Data) -> Element<'_, Message> {
    let login = login_url(data);
    let mut body = column![
        typo::text(
            typo::BODY,
            tr("Nextcloud handles the sign-in. You sign in on its own page; Torro Workplace never sees your password.")
        ),
        typo::text(typo::BODY, tr("Waiting for the browser …")).style(theme::text_2),
    ]
    .spacing(space::M);
    if let Some(url) = login {
        // `text_input` without an edit handler cannot take focus, so no copying. Editing is
        // allowed but ignored: the field always shows `url`, so the text stays unchanged.
        body = body
            .push(typo::text(typo::BODY, tr("If the browser does not open, copy this link:")).style(theme::text_2))
            .push(field("", url).on_input(|_| Message::LoginLinkEdited));
    }
    let again = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Open the page again"),
        style::button::secondary,
        login.map(|_| Message::OpenLoginPage),
    );
    body.push(row![again]).into()
}

/// Error text left, "Cancel" and the step's primary button right.
fn footer(state: &State) -> Element<'_, Message> {
    let error: Element<'_, Message> = match &state.error {
        Some(error) => typo::text(typo::BODY, error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        None => blank::horizontal().into(),
    };
    let cancel = widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close));
    let next = match state.step {
        Step::Provider | Step::SigningIn => None,
        Step::Server => Some(widgets::text_button(
            None,
            tr("Continue"),
            style::button::primary,
            can_continue(state).then_some(Message::Continue),
        )),
        Step::Done => Some(widgets::text_button(None, tr("Done"), style::button::primary, Some(Message::Close))),
    };
    row![error, cancel].push(next).spacing(space::S).align_y(Alignment::Center).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: id.to_string(),
            kind: "nextcloud".to_string(),
            label: id.to_string(),
            state,
            collections: Vec::new(),
        }
    }

    fn signing_in(login_url: &str) -> AccountState {
        AccountState::SigningIn { server: "https://cloud.example.org".into(), login_url: login_url.into() }
    }

    fn connected() -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.org".into(),
            login_name: "anna".into(),
            last_sync: None,
            problem: None,
        }
    }

    fn data_with(accounts: Vec<AccountInfo>) -> Data {
        Data { accounts, ..Data::default() }
    }

    /// The state after the user typed `server` and pressed Continue.
    fn signing_in_state(server: &str, data: &Data) -> State {
        let mut state = State::default();
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), data);
        update(&mut state, Message::ServerChanged(server.into()), data);
        update(&mut state, Message::Continue, data);
        state
    }

    #[test]
    fn provider_then_server_then_signing_in_then_done() {
        let empty = Data::default();
        let mut state = State::default();
        assert_eq!(state.step, Step::Provider);

        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), &empty);
        assert_eq!(state.step, Step::Server);

        update(&mut state, Message::ServerChanged("cloud.example.org".into()), &empty);
        let action = update(&mut state, Message::Continue, &empty);
        assert!(matches!(
            action,
            Action::Send(Request::ConnectNextcloud { ref server }) if server == "cloud.example.org"
        ));
        assert_eq!(state.step, Step::SigningIn);
        assert!(state.busy());

        // The core reports the sign-in in progress: still waiting.
        let waiting = data_with(vec![account("nextcloud:anna@cloud.example.org", signing_in("https://x/login"))]);
        observe(&mut state, &waiting);
        assert_eq!(state.step, Step::SigningIn);

        // The account is connected: observe finishes the sign-in.
        let done = data_with(vec![account("nextcloud:anna@cloud.example.org", connected())]);
        observe(&mut state, &done);
        assert_eq!(state.step, Step::Done);
        assert!(!state.busy());
    }

    #[test]
    fn update_also_detects_the_connection() {
        let empty = Data::default();
        let mut state = signing_in_state("cloud.example.org", &empty);
        let done = data_with(vec![account("nextcloud:anna@cloud.example.org", connected())]);
        assert!(matches!(update(&mut state, Message::Close, &done), Action::None));
        assert_eq!(state.step, Step::Done);
    }

    #[test]
    fn account_connected_before_sign_in_does_not_finish_it() {
        let existing = data_with(vec![account("nextcloud:old@cloud.example.org", connected())]);
        let mut state = signing_in_state("cloud.example.org", &existing);
        observe(&mut state, &existing);
        assert_eq!(state.step, Step::SigningIn, "an account that was connected before does not count");

        let with_new = data_with(vec![
            account("nextcloud:old@cloud.example.org", connected()),
            account("nextcloud:anna@cloud.example.org", connected()),
        ]);
        observe(&mut state, &with_new);
        assert_eq!(state.step, Step::Done);
    }

    #[test]
    fn disabled_providers_do_nothing() {
        for provider in [Provider::GitHub, Provider::Slack, Provider::Ai] {
            let mut state = State::default();
            update(&mut state, Message::ChooseProvider(provider), &Data::default());
            assert_eq!(state.step, Step::Provider, "{provider:?} is disabled");
        }
    }

    #[test]
    fn empty_server_keeps_continue_disabled() {
        let empty = Data::default();
        let mut state = State::default();
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), &empty);
        for typed in ["", "   ", "\t"] {
            update(&mut state, Message::ServerChanged(typed.into()), &empty);
            assert!(!can_continue(&state), "{typed:?} must not enable Continue");
            let action = update(&mut state, Message::Continue, &empty);
            assert!(matches!(action, Action::None));
            assert_eq!(state.step, Step::Server);
            assert!(!state.busy());
        }
        update(&mut state, Message::ServerChanged("cloud.example.org".into()), &empty);
        assert!(can_continue(&state));
    }

    #[test]
    fn open_again_opens_the_login_url() {
        let data = data_with(vec![account("nextcloud:anna@cloud.example.org", signing_in("https://x/login/v2"))]);
        let mut state = signing_in_state("cloud.example.org", &data);
        match update(&mut state, Message::OpenLoginPage, &data) {
            Action::OpenUrl(url) => assert_eq!(url, "https://x/login/v2"),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
    }

    #[test]
    fn observe_outside_signing_in_does_nothing() {
        let connected_data = data_with(vec![account("nextcloud:anna@cloud.example.org", connected())]);
        let mut state = State::default();
        observe(&mut state, &connected_data);
        assert_eq!(state.step, Step::Provider);
    }

    #[test]
    fn observe_copies_the_shell_error_while_signing_in() {
        let empty = Data::default();
        let mut state = signing_in_state("cloud.example.org", &empty);
        let failed = Data { error: Some("daemon not running".into()), ..Data::default() };
        observe(&mut state, &failed);
        assert_eq!(state.error.as_deref(), Some("daemon not running"));
        assert_eq!(state.step, Step::SigningIn);
    }

    #[test]
    fn views_build_for_each_step() {
        let empty = Data::default();
        let mut state = State::default();
        drop(view(&state, &empty));
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), &empty);
        drop(view(&state, &empty));
        let with_link = data_with(vec![account("nextcloud:anna@cloud.example.org", signing_in("https://x/login"))]);
        let signing = signing_in_state("cloud.example.org", &with_link);
        let _waiting = view(&signing, &with_link);
        let done = data_with(vec![account("nextcloud:anna@cloud.example.org", connected())]);
        let mut finished = signing_in_state("cloud.example.org", &empty);
        observe(&mut finished, &done);
        let _done = view(&finished, &done);
    }
}
