//! The add-account dialog (task S.6 in `docs/tasks/steuerzentrale.md`, GitHub since G.1).
//!
//! Steps, with the subtitle changing per step, errors bottom left, "Cancel" and the primary
//! button bottom right (like TorroMail's setup wizard):
//!
//! 1. **Provider:** a catalog – Nextcloud (calendars, tasks) and GitHub (issues, projects)
//!    enabled; Slack and AI providers listed as "Coming soon", disabled. With a single available
//!    provider the step would be skipped. The dialog is the same with accounts already present:
//!    it adds one more.
//! 2. Nextcloud: **Server** – the address; Enter or "Continue" sends
//!    `Request::ConnectNextcloud { server }`. Then **Sign in**: "Nextcloud handles the sign-in.
//!    …" The browser opens (the core does that); the login URL from `data.accounts`
//!    (`AccountState::SigningIn`, the entry with an empty id) is a fallback link to copy.
//! 3. GitHub: **two ways** – "Use GitHub CLI sign-in" sends `Request::ConnectGithub` (pressing it
//!    is the one confirmation; the core runs `gh auth token`, never this window), "Paste a token"
//!    leads to a masked field with a link to GitHub's token page (scope `repo` preselected) and
//!    sends `Request::SetGithubToken`. Both wait in **Checking** for the core's answer.
//! 4. **Done:** "Your calendars and task lists are ready." (Nextcloud) or "Signed in to GitHub as
//!    …" (GitHub), and "Done".
//!
//! How a step learns that it is over:
//!
//! - The shell hands every answer to a request to [`answered`] while the dialog is open. GitHub's
//!   answer is the account or an error; a failed `ConnectNextcloud` (server not found) comes back
//!   the same way.
//! - The Nextcloud sign-in ends in the browser, so [`observe`] follows it through every reload of
//!   `data` (task M1.4's rule): the sign-in has ended once the entry with an empty id has been
//!   seen and is gone again. It succeeded when an affected account changed – a new Nextcloud
//!   account on that server, or an existing one whose `last_sync` is newer than the start of the
//!   sign-in or whose `problem` changed (signing in to an account that exists already); else it
//!   failed and the account is as it was.
//!
//! `Message::Close` is handled by `accounts.rs` (closes the dialog); Cancel, Done, Escape and a
//! click next to the dialog send it. [`view`] is the dialog's content only; `accounts::overlay`
//! puts it on the dialog surface over the whole window (`super::dialog`).

use chrono::{DateTime, Utc};
use iced::widget::space as blank;
use iced::widget::{column, row, text, text_input};
use iced::{Alignment, Color, Element, Fill, Theme};
use torro_core::account::Secret;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, Palette, palette, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{AccountInfo, Request, Response};
use torro_pim::AccountState;

use super::{Action, Data, chevron, one_line, pressable, row_frame, setting_row};

/// GitHub's page for a new classic token, with the scope the suite needs (`repo`: private
/// repositories, closing issues) and a name already filled in.
pub const NEW_TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo&description=Torro%20Workplace";

/// The step the dialog is on. Each step has its own subtitle and footer button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Provider,
    /// Nextcloud: the server address.
    Server,
    /// Nextcloud: Login Flow v2 runs in the browser.
    SigningIn,
    /// GitHub: the CLI's sign-in or a token.
    Github,
    /// GitHub: the token field.
    Token,
    /// GitHub: waiting for the core to check the sign-in.
    Checking,
    Done,
}

/// A service in the catalog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Nextcloud,
    GitHub,
    Slack,
    Ai,
}

impl Provider {
    /// Nextcloud and GitHub can be set up; the others are listed so the catalog shows where the
    /// suite is heading.
    fn is_enabled(self) -> bool {
        matches!(self, Provider::Nextcloud | Provider::GitHub)
    }

    /// The step after picking the provider.
    fn first_step(self) -> Step {
        match self {
            Provider::GitHub => Step::Github,
            _ => Step::Server,
        }
    }

    const ALL: [Provider; 4] = [Provider::Nextcloud, Provider::GitHub, Provider::Slack, Provider::Ai];
}

/// Where a new dialog starts: at the provider catalog, unless there is only one provider to pick.
fn first_step() -> Step {
    let mut available = Provider::ALL.into_iter().filter(|provider| provider.is_enabled());
    match (available.next(), available.next()) {
        (Some(provider), None) => provider.first_step(),
        _ => Step::Provider,
    }
}

#[derive(Debug, Default)]
pub struct State {
    step: Step,
    /// The provider being set up; Done says what is ready.
    provider: Provider,
    /// The server address as typed (Nextcloud).
    server: String,
    /// The GitHub token as pasted; `Debug` never shows it.
    token: Secret,
    /// The step a failed GitHub check goes back to: the two ways or the token field.
    back: Step,
    /// The GitHub login once connected, for Done.
    login: Option<String>,
    /// The Nextcloud sign-in that runs: its start and the accounts before it.
    sign_in: Option<SignIn>,
    /// Error text for the footer: the core's answer or the shell's last error.
    error: Option<String>,
}

/// What a Nextcloud sign-in compares against to tell how it ended ([`observe`]).
#[derive(Debug, Clone)]
struct SignIn {
    started: DateTime<Utc>,
    /// Every account with its problem when the sign-in started.
    before: Vec<(String, Option<String>)>,
    /// The entry with an empty id was in a reload, so its absence means the end.
    seen_pending: bool,
    /// The server the core signs in at, from that entry; only its accounts count.
    server: Option<String>,
}

impl State {
    /// A dialog on its first step.
    pub fn new() -> State {
        State { step: first_step(), ..State::default() }
    }

    /// A sign-in or a check runs; the shell reloads more often.
    pub fn busy(&self) -> bool {
        self.step == Step::Checking || self.sign_in.is_some()
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Cancel, Done, Escape or a click next to the dialog.
    Close,
    /// Back from GitHub's token field to its two ways, and from those to the catalog.
    Back,
    /// A provider card was pressed; disabled providers do nothing.
    ChooseProvider(Provider),
    ServerChanged(String),
    /// "Continue", or Enter in the server or token field.
    Continue,
    /// "Open the page again".
    OpenLoginPage,
    /// The login link field is read-only: typing in it changes nothing.
    LoginLinkEdited,
    /// "Use GitHub CLI sign-in".
    UseGithubCli,
    /// "Paste a token".
    PasteToken,
    /// The token field changed. A [`Secret`], so that the derived `Debug` of this message (iced
    /// can log messages) shows no token.
    TokenChanged(Secret),
    /// "Create a token on GitHub".
    OpenTokenPage,
}

/// Follows a Nextcloud sign-in through a reload of `data` and ends it as the module doc says.
///
/// Call it on every reload of `data` while the dialog is open; only the sign-in step reacts.
/// While waiting it also copies the shell's error (for example "daemon not running") into the
/// footer.
pub fn observe(state: &mut State, data: &Data) {
    let Some(sign_in) = state.sign_in.as_mut().filter(|_| state.step == Step::SigningIn) else { return };
    if let Some(pending) = data.accounts.iter().find(|account| account.id.is_empty()) {
        sign_in.seen_pending = true;
        if let AccountState::SigningIn { server, .. } = &pending.state {
            sign_in.server = Some(server.clone());
        }
        state.error = data.error.clone();
        return;
    }
    if data.accounts.iter().any(|account| signed_in(sign_in, account)) {
        state.step = Step::Done;
        state.sign_in = None;
        state.error = None;
    } else if sign_in.seen_pending {
        state.step = Step::Server;
        state.sign_in = None;
        state.error = Some(tr("Signing in did not finish. Try again.").to_string());
    } else {
        state.error = data.error.clone();
    }
}

/// `account` is a Nextcloud account on the server of `sign_in` that changed since it started:
/// new, synced afterwards, or with another problem than before.
fn signed_in(sign_in: &SignIn, account: &AccountInfo) -> bool {
    let AccountState::Connected { server, last_sync, problem, .. } = &account.state else { return false };
    let same_server = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
    if account.kind != "nextcloud" || !sign_in.server.as_deref().is_none_or(|s| same_server(s, server)) {
        return false;
    }
    match sign_in.before.iter().find(|(id, _)| *id == account.id) {
        None => true,
        Some((_, before)) => last_sync.is_some_and(|at| at > sign_in.started) || problem != before,
    }
}

/// The answer to a request the shell sent. Returns whether the dialog took it: then the shell
/// shows no notice of its own, the dialog shows the error in its footer.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    match (state.step, result) {
        (Step::Checking, Ok(Response::Account { account })) => {
            state.login = Some(account.label.clone());
            state.step = Step::Done;
            state.error = None;
            true
        }
        (Step::Checking, Ok(_)) => {
            state.step = state.back;
            state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string());
            true
        }
        (Step::Checking, Err(message)) => {
            state.step = state.back;
            state.error = Some(message.clone());
            true
        }
        // The sign-in could not start (no such server): back to the address.
        (Step::SigningIn, Err(message)) => {
            state.step = Step::Server;
            state.sign_in = None;
            state.error = Some(message.clone());
            true
        }
        (Step::SigningIn, Ok(_)) => true,
        _ => false,
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    // Every message first looks at the latest data, so a finished sign-in is noticed at once.
    observe(state, data);
    match message {
        Message::Close | Message::LoginLinkEdited => Action::None,
        Message::Back => {
            state.error = None;
            state.step = match state.step {
                Step::Token => Step::Github,
                Step::Github | Step::Server => Step::Provider,
                other => other,
            };
            Action::None
        }
        Message::ChooseProvider(provider) => {
            if state.step == Step::Provider && provider.is_enabled() {
                state.provider = provider;
                state.step = provider.first_step();
            }
            Action::None
        }
        Message::ServerChanged(server) => {
            state.server = server;
            Action::None
        }
        Message::Continue => match state.step {
            Step::Server if can_continue(state) => start_nextcloud(state, data),
            Step::Token if !state.token.is_blank() => {
                state.back = Step::Token;
                state.step = Step::Checking;
                state.error = None;
                Action::Send(Request::SetGithubToken { token: std::mem::take(&mut state.token) })
            }
            _ => Action::None,
        },
        Message::OpenLoginPage => match (state.step, login_url(data)) {
            (Step::SigningIn, Some(url)) => Action::OpenUrl(url.to_string()),
            _ => Action::None,
        },
        Message::UseGithubCli if state.step == Step::Github => {
            state.back = Step::Github;
            state.step = Step::Checking;
            state.error = None;
            Action::Send(Request::ConnectGithub)
        }
        Message::PasteToken if state.step == Step::Github => {
            state.step = Step::Token;
            state.error = None;
            Action::None
        }
        Message::TokenChanged(token) => {
            state.token = token;
            Action::None
        }
        Message::OpenTokenPage => Action::OpenUrl(NEW_TOKEN_URL.to_string()),
        Message::UseGithubCli | Message::PasteToken => Action::None,
    }
}

/// Starts Login Flow v2 and notes what the accounts were, so [`observe`] can tell the end.
fn start_nextcloud(state: &mut State, data: &Data) -> Action {
    let server = state.server.trim().to_string();
    let before = data.accounts.iter().map(|account| (account.id.clone(), problem_of(&account.state))).collect();
    state.sign_in = Some(SignIn { started: Utc::now(), before, seen_pending: false, server: None });
    state.step = Step::SigningIn;
    state.error = None;
    Action::Send(Request::ConnectNextcloud { server })
}

fn problem_of(state: &AccountState) -> Option<String> {
    match state {
        AccountState::Connected { problem, .. } => problem.clone(),
        AccountState::NotConnected | AccountState::SigningIn { .. } => None,
    }
}

/// Continue is only possible with a server address (spaces do not count).
fn can_continue(state: &State) -> bool {
    !state.server.trim().is_empty()
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
        typo::text(typo::BODY, subtitle(state)).style(theme::text_2)
    ]
    .spacing(space::XS);
    let body = match state.step {
        Step::Provider => provider_step(),
        Step::Server => server_step(state),
        Step::SigningIn => signing_in_step(data),
        Step::Github => github_step(),
        Step::Token => token_step(state),
        Step::Checking => typo::text(typo::BODY, tr("Checking the connection…")).style(theme::text_2).into(),
        Step::Done => typo::text(typo::BODY, done_text(state)).into(),
    };
    column![header, body, footer(state)].spacing(space::XL).into()
}

/// The line under the title; it names what the current step asks for.
fn subtitle(state: &State) -> &'static str {
    match (state.step, state.provider) {
        (Step::Provider, _) => tr("Choose a service to connect."),
        (Step::Server, _) => tr("Enter the address of your Nextcloud."),
        (Step::SigningIn, _) => tr("Sign in with Nextcloud in your browser."),
        (Step::Github, _) => tr("Choose how Torro Workplace signs in to GitHub."),
        (Step::Token, _) => tr("Paste a personal access token from GitHub."),
        (Step::Checking, _) => tr("Checking the connection with GitHub."),
        (Step::Done, Provider::GitHub) => tr("GitHub is connected."),
        (Step::Done, _) => tr("Nextcloud is connected."),
    }
}

fn done_text(state: &State) -> String {
    match (state.provider, &state.login) {
        (Provider::GitHub, Some(login)) => trf("Signed in to GitHub as {login}.", &[("login", login)]),
        _ => tr("Your calendars and task lists are ready.").to_string(),
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
        coming_soon_row(name, summary)
    }
}

/// A provider that is not available yet: the whole row – name, summary and tag – at the disabled
/// opacity of 40 %, with the metrics of [`setting_row`].
fn coming_soon_row(name: &'static str, summary: &'static str) -> Element<'static, Message> {
    let faded = |colour: fn(&Palette) -> Color| {
        move |t: &Theme| text::Style { color: Some(colour(palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let lines = column![
        one_line(typo::text(typo::BODY, name).style(faded(|p| p.text))),
        one_line(typo::text(typo::META, summary).style(faded(|p| p.text_2))),
    ];
    row_frame(
        row![lines.width(Fill), widgets::pill_disabled(None, tr("Coming soon"))]
            .spacing(space::L)
            .align_y(Alignment::Center),
        true,
    )
}

/// GitHub's two ways in, as rows to press, and where the token stays.
fn github_step() -> Element<'static, Message> {
    let way = |name: &'static str, summary: &'static str, message| {
        pressable(setting_row(None, name, Some(summary.to_string()), chevron()), Some(message))
    };
    let rows = vec![
        way(tr("Use GitHub CLI sign-in"), tr("Uses the sign-in of the gh command."), Message::UseGithubCli),
        way(tr("Paste a token"), tr("A personal access token from your GitHub settings."), Message::PasteToken),
    ];
    let footer = tr("The token stays in the system keychain. Agents never see it.");
    widgets::group(tr("Sign in with"), rows, Some(footer.to_string()))
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

/// What the token needs, the way to GitHub's token page, and the masked field.
fn token_step(state: &State) -> Element<'_, Message> {
    let create = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Create a token on GitHub"),
        style::button::secondary,
        Some(Message::OpenTokenPage),
    );
    let input = field("ghp_… / github_pat_…", state.token.expose())
        .secure(true)
        .on_input(|token| Message::TokenChanged(Secret::new(token)))
        .on_submit_maybe((!state.token.is_blank()).then_some(Message::Continue));
    column![
        typo::text(
            typo::BODY,
            tr("Reading private repositories and closing issues needs the repo scope; GitHub's page has it selected.")
        )
        .style(theme::text_2),
        row![create],
        input,
    ]
    .spacing(space::M)
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

/// Error text left, "Back" where there is a step to go back to, "Cancel" and the step's primary
/// button right.
fn footer(state: &State) -> Element<'_, Message> {
    let error: Element<'_, Message> = match &state.error {
        Some(error) => typo::text(typo::BODY, error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        None => blank::horizontal().into(),
    };
    let back = matches!(state.step, Step::Github | Step::Token)
        .then(|| widgets::text_button(None, tr("Back"), style::button::secondary, Some(Message::Back)));
    let cancel = widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match state.step {
        Step::Provider | Step::SigningIn | Step::Github | Step::Checking => None,
        Step::Server => primary(tr("Continue"), can_continue(state).then_some(Message::Continue)),
        Step::Token => primary(tr("Continue"), (!state.token.is_blank()).then_some(Message::Continue)),
        Step::Done => primary(tr("Done"), Some(Message::Close)),
    };
    row![error].push(back).push(cancel).push(next).spacing(space::S).align_y(Alignment::Center).into()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn account(id: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: id.to_string(),
            kind: "nextcloud".to_string(),
            label: id.to_string(),
            state,
            collections: Vec::new(),
            scopes: None,
        }
    }

    fn signing_in(login_url: &str) -> AccountState {
        AccountState::SigningIn { server: "https://cloud.example.org".into(), login_url: login_url.into() }
    }

    fn connected_at(last_sync: Option<DateTime<Utc>>, problem: Option<&str>) -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.org".into(),
            login_name: "anna".into(),
            last_sync,
            problem: problem.map(str::to_string),
        }
    }

    fn connected() -> AccountState {
        connected_at(None, None)
    }

    fn data_with(accounts: Vec<AccountInfo>) -> Data {
        Data { accounts, ..Data::default() }
    }

    const ANNA: &str = "nextcloud:anna@cloud.example.org";

    /// The pending entry of a sign-in that runs, as the core lists it.
    fn pending() -> AccountInfo {
        account("", signing_in("https://x/login"))
    }

    /// A dialog at the server step with `server` typed, after pressing Continue.
    fn signing_in_state(server: &str, data: &Data) -> State {
        let mut state = at_catalog();
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), data);
        update(&mut state, Message::ServerChanged(server.into()), data);
        update(&mut state, Message::Continue, data);
        state
    }

    fn at_catalog() -> State {
        State::new()
    }

    /// A dialog at GitHub's two ways.
    fn at_github() -> State {
        let mut state = at_catalog();
        update(&mut state, Message::ChooseProvider(Provider::GitHub), &Data::default());
        state
    }

    #[test]
    fn two_providers_open_the_catalog() {
        assert_eq!(Provider::ALL.iter().filter(|provider| provider.is_enabled()).count(), 2);
        assert_eq!(State::new().step, Step::Provider);
    }

    #[test]
    fn provider_then_server_then_signing_in_then_done() {
        let empty = Data::default();
        let mut state = at_catalog();
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
        observe(&mut state, &data_with(vec![pending()]));
        assert_eq!(state.step, Step::SigningIn);

        // The entry is gone and a new account is there.
        observe(&mut state, &data_with(vec![account(ANNA, connected())]));
        assert_eq!(state.step, Step::Done);
        assert!(!state.busy());
    }

    #[test]
    fn update_also_detects_the_connection() {
        let empty = Data::default();
        let mut state = signing_in_state("cloud.example.org", &empty);
        let done = data_with(vec![account(ANNA, connected())]);
        assert!(matches!(update(&mut state, Message::Close, &done), Action::None));
        assert_eq!(state.step, Step::Done);
    }

    #[test]
    fn an_account_that_did_not_change_does_not_finish_it() {
        let existing = data_with(vec![account("nextcloud:old@cloud.example.org", connected())]);
        let mut state = signing_in_state("cloud.example.org", &existing);
        observe(&mut state, &existing);
        assert_eq!(state.step, Step::SigningIn, "nothing changed and no sign-in was seen yet");

        let with_new =
            data_with(vec![account("nextcloud:old@cloud.example.org", connected()), account(ANNA, connected())]);
        observe(&mut state, &with_new);
        assert_eq!(state.step, Step::Done);
    }

    /// Task M1.2's finding: signing in to an account that exists already used to wait forever.
    #[test]
    fn signing_in_again_to_an_existing_account_ends_when_it_syncs() {
        let old = Some(Utc::now() - TimeDelta::hours(2));
        let broken = data_with(vec![account(ANNA, connected_at(old, Some("Sign in again")))]);
        let mut state = signing_in_state("cloud.example.org", &broken);
        observe(&mut state, &data_with(vec![account(ANNA, connected_at(old, Some("Sign in again"))), pending()]));
        assert_eq!(state.step, Step::SigningIn);

        let synced = Some(Utc::now() + TimeDelta::seconds(5));
        observe(&mut state, &data_with(vec![account(ANNA, connected_at(synced, None))]));
        assert_eq!(state.step, Step::Done);
        assert!(!state.busy());
    }

    #[test]
    fn a_changed_problem_also_ends_a_new_sign_in_to_an_existing_account() {
        let old = Some(Utc::now() - TimeDelta::hours(2));
        let before = data_with(vec![account(ANNA, connected_at(old, Some("Sign in again")))]);
        let mut state = signing_in_state("cloud.example.org", &before);
        observe(&mut state, &data_with(vec![account(ANNA, connected_at(old, Some("Sign in again"))), pending()]));
        observe(&mut state, &data_with(vec![account(ANNA, connected_at(old, Some("The server did not answer")))]));
        assert_eq!(state.step, Step::Done);
    }

    #[test]
    fn a_sign_in_that_ends_without_a_change_failed() {
        let old = Some(Utc::now() - TimeDelta::hours(2));
        let before = data_with(vec![account(ANNA, connected_at(old, None))]);
        let mut state = signing_in_state("cloud.example.org", &before);
        observe(&mut state, &data_with(vec![account(ANNA, connected_at(old, None)), pending()]));
        observe(&mut state, &before);
        assert_eq!(state.step, Step::Server, "back to the address to try again");
        assert_eq!(state.error.as_deref(), Some(tr("Signing in did not finish. Try again.")));
        assert!(!state.busy());
    }

    #[test]
    fn only_accounts_on_the_server_signed_in_at_count() {
        let old = Some(Utc::now() - TimeDelta::hours(2));
        let other = "nextcloud:bob@other.example.org";
        let other_state = |sync| AccountState::Connected {
            server: "https://other.example.org".into(),
            login_name: "bob".into(),
            last_sync: sync,
            problem: None,
        };
        let before = data_with(vec![account(other, other_state(old))]);
        let mut state = signing_in_state("cloud.example.org", &before);
        observe(&mut state, &data_with(vec![account(other, other_state(old)), pending()]));
        // Another account synced meanwhile, and a GitHub account was checked: not this sign-in.
        let mut github = account("github:anna", connected_at(Some(Utc::now() + TimeDelta::seconds(5)), None));
        github.kind = "github".into();
        let synced = data_with(vec![account(other, other_state(Some(Utc::now() + TimeDelta::seconds(5)))), github]);
        observe(&mut state, &synced);
        assert_eq!(state.step, Step::Server, "failed: the account at cloud.example.org did not change");
    }

    #[test]
    fn a_sign_in_that_could_not_start_goes_back_to_the_address() {
        let mut state = signing_in_state("nowhere.example", &Data::default());
        assert!(answered(&mut state, &Err("no Nextcloud at nowhere.example".into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::Server, Some("no Nextcloud at nowhere.example")));
        assert!(!state.busy());
        let mut started = signing_in_state("cloud.example.org", &Data::default());
        let login = Ok(Response::LoginStarted { login_url: "https://x/login".into() });
        assert!(answered(&mut started, &login));
        assert_eq!(started.step, Step::SigningIn, "the answer only starts it");
    }

    #[test]
    fn the_cli_sign_in_is_one_press_and_ends_with_the_account() {
        let mut state = at_github();
        assert_eq!(state.step, Step::Github);
        assert!(matches!(
            update(&mut state, Message::UseGithubCli, &Data::default()),
            Action::Send(Request::ConnectGithub)
        ));
        assert_eq!(state.step, Step::Checking);
        assert!(state.busy());
        let mut github = account("github:mahype", connected());
        github.label = "mahype".into();
        assert!(answered(&mut state, &Ok(Response::Account { account: github })));
        assert_eq!((state.step, state.login.as_deref()), (Step::Done, Some("mahype")));
        assert_eq!(done_text(&state), trf("Signed in to GitHub as {login}.", &[("login", &"mahype")]));
    }

    #[test]
    fn a_failed_check_goes_back_with_the_core_message() {
        let mut state = at_github();
        update(&mut state, Message::UseGithubCli, &Data::default());
        let hint = "The GitHub CLI is not signed in. Run `gh auth login` in a terminal, or paste a token instead.";
        assert!(answered(&mut state, &Err(hint.into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::Github, Some(hint)));

        update(&mut state, Message::PasteToken, &Data::default());
        assert_eq!((state.step, state.error.as_deref()), (Step::Token, None));
        update(&mut state, Message::TokenChanged(Secret::new("ghp_wrong")), &Data::default());
        update(&mut state, Message::Continue, &Data::default());
        assert!(answered(&mut state, &Err("GitHub did not accept the token.".into())));
        assert_eq!(state.step, Step::Token, "back at the field to paste it again");
        assert!(!answered(&mut state, &Ok(Response::Ok)), "outside a check the shell keeps its answers");
    }

    #[test]
    fn a_pasted_token_is_sent_once_and_never_shown() {
        let mut state = at_github();
        update(&mut state, Message::PasteToken, &Data::default());
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None), "no token yet");
        update(&mut state, Message::TokenChanged(Secret::new("  ")), &Data::default());
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None), "blank");
        update(&mut state, Message::TokenChanged(Secret::new("ghp_SECRET")), &Data::default());
        assert!(!format!("{state:?}").contains("SECRET"));
        let message = Message::TokenChanged(Secret::new("ghp_SECRET"));
        assert!(!format!("{message:?}").contains("SECRET"), "the message holds the token as a Secret");
        let action = update(&mut state, Message::Continue, &Data::default());
        assert!(
            matches!(action, Action::Send(Request::SetGithubToken { ref token }) if token.expose() == "ghp_SECRET"),
            "{action:?}"
        );
        assert!(!format!("{action:?}").contains("SECRET"));
        assert!(state.token.is_blank(), "the dialog keeps no copy");
        assert_eq!(state.step, Step::Checking);
    }

    #[test]
    fn the_token_page_has_the_scope_preselected_and_back_leads_out() {
        let mut state = at_github();
        update(&mut state, Message::PasteToken, &Data::default());
        match update(&mut state, Message::OpenTokenPage, &Data::default()) {
            Action::OpenUrl(url) => assert_eq!(url, NEW_TOKEN_URL),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        assert!(NEW_TOKEN_URL.contains("scopes=repo"));
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::Github);
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::Provider);
    }

    #[test]
    fn disabled_providers_do_nothing() {
        for provider in [Provider::Slack, Provider::Ai] {
            let mut state = at_catalog();
            update(&mut state, Message::ChooseProvider(provider), &Data::default());
            assert_eq!(state.step, Step::Provider, "{provider:?} is disabled");
        }
    }

    #[test]
    fn empty_server_keeps_continue_disabled() {
        let empty = Data::default();
        let mut state = at_catalog();
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
        let data = data_with(vec![account("", signing_in("https://x/login/v2"))]);
        let mut state = signing_in_state("cloud.example.org", &data);
        match update(&mut state, Message::OpenLoginPage, &data) {
            Action::OpenUrl(url) => assert_eq!(url, "https://x/login/v2"),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
    }

    #[test]
    fn observe_outside_signing_in_does_nothing() {
        let connected_data = data_with(vec![account(ANNA, connected())]);
        let mut state = at_catalog();
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
        let mut state = at_catalog();
        drop(view(&state, &empty));
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), &empty);
        drop(view(&state, &empty));
        let with_link = data_with(vec![pending()]);
        let signing = signing_in_state("cloud.example.org", &with_link);
        drop(view(&signing, &with_link));
        let mut github = at_github();
        drop(view(&github, &empty));
        update(&mut github, Message::PasteToken, &empty);
        drop(view(&github, &empty));
        update(&mut github, Message::TokenChanged(Secret::new("ghp_x")), &empty);
        update(&mut github, Message::Continue, &empty);
        drop(view(&github, &empty));
        answered(&mut github, &Ok(Response::Account { account: account("github:x", connected()) }));
        drop(view(&github, &empty));
    }
}
