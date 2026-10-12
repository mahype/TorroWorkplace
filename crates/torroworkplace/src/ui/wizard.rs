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
//! 4. Mail provider (task M7.2): the path of [`MAIL_STEPS`] – **Address** (an e-mail address;
//!    "Find provider" sends `Request::DiscoverServices`, which is only a search and needs no
//!    password), the answer shown on the same step (the provider's name, "Server" only under
//!    "Advanced", or the reason a provider is not supported and nothing more), **Password** (with
//!    "Use an app password" and a link where a provider asks for one; the user name under
//!    "Advanced"), **Services** (check boxes from [`Service::ALL`], "Mail" greyed out with "Coming
//!    later"); "Connect" sends `Request::ConnectCalDav` and waits in **Connecting**. A refused
//!    password (`auth`) comes back to the Password step, under the field. The steps and the check
//!    boxes are data, so the mail core (M5.1) adds a step or a service without a new screen.
//! 5. **Done:** "Your calendars and task lists are ready." (Nextcloud), "Signed in to GitHub as
//!    …" (GitHub) or what the mail provider's server has, and "Done".
//!
//! How a step learns that it is over:
//!
//! - The shell hands every answer to a request to [`answered`] while the dialog is open. GitHub's
//!   answer is the account or an error; a failed `ConnectNextcloud` (server not found) comes back
//!   the same way, and so do the answers of the mail provider's two requests. The shell's `send`
//!   passes on the message of an error but not its code, so a refused password is recognised by
//!   the core's own text ([`password_refused`]): both run in the same language.
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
use iced::widget::{Space, checkbox, column, container, row, svg, text, text_input};
use iced::{Alignment, Color, Element, Fill, Theme};
use torro_core::account::Secret;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
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
    /// Mail provider: the e-mail address, and below it what the search found.
    MailAddress,
    /// Mail provider: the search runs (`DiscoverServices`).
    MailSearching,
    /// Mail provider: the password or app password.
    MailPassword,
    /// Mail provider: which services to use.
    MailServices,
    /// Mail provider: waiting for the core to sign in (`ConnectCalDav`).
    MailConnecting,
    Done,
}

/// The steps of the mail provider path, in order. Back and Continue walk this list; the mail core
/// (task M5.1) puts its own step into it (IMAP and SMTP settings come under "Advanced" of the
/// address step), the screens of the others stay as they are.
const MAIL_STEPS: [Step; 3] = [Step::MailAddress, Step::MailPassword, Step::MailServices];

impl Step {
    /// The step after this one on the mail path; `None` at its end (the request is sent there).
    fn mail_next(self) -> Option<Step> {
        let at = MAIL_STEPS.iter().position(|step| *step == self)?;
        MAIL_STEPS.get(at + 1).copied()
    }

    /// The step before this one on the mail path; the first leads back to the catalog.
    fn mail_previous(self) -> Step {
        match MAIL_STEPS.iter().position(|step| *step == self) {
            Some(at) if at > 0 => MAIL_STEPS[at - 1],
            _ => Step::Provider,
        }
    }
}

/// What a mail provider's account can bring to the suite. A row of the services step each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Calendars,
    Tasks,
    Mail,
}

impl Service {
    const ALL: [Service; 3] = [Service::Calendars, Service::Tasks, Service::Mail];

    /// Calendars and tasks come over CalDAV; mail comes with the mail core (task M5.1) and is
    /// listed greyed out until then.
    fn is_available(self) -> bool {
        !matches!(self, Service::Mail)
    }

    fn name(self) -> &'static str {
        match self {
            Service::Calendars => tr("Calendars"),
            Service::Tasks => tr("Tasks"),
            Service::Mail => tr("Mail"),
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Service::Calendars => tr("Appointments and events"),
            Service::Tasks => tr("To-dos and reminders"),
            Service::Mail => tr("Read and send email"),
        }
    }

    /// The sentence on the last step for this service: whether the server has it.
    fn found_text(self, found: bool) -> &'static str {
        match (self, found) {
            (Service::Calendars, true) => tr("Calendars are ready."),
            (Service::Calendars, false) => tr("This server has no calendars."),
            (Service::Tasks, true) => tr("Tasks are ready."),
            (Service::Tasks, false) => tr("This server has no task lists."),
            (Service::Mail, _) => tr("Mail comes later."),
        }
    }
}

/// A service in the catalog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Nextcloud,
    /// The calendars and tasks of a mail provider (CalDAV, task M7.2); later its mail as well.
    Mail,
    GitHub,
    Slack,
    Ai,
}

impl Provider {
    /// Nextcloud, a mail provider and GitHub can be set up; the others are listed so the catalog
    /// shows where the suite is heading.
    fn is_enabled(self) -> bool {
        matches!(self, Provider::Nextcloud | Provider::Mail | Provider::GitHub)
    }

    /// The step after picking the provider.
    fn first_step(self) -> Step {
        match self {
            Provider::GitHub => Step::Github,
            Provider::Mail => MAIL_STEPS[0],
            _ => Step::Server,
        }
    }

    const ALL: [Provider; 5] = [Provider::Nextcloud, Provider::Mail, Provider::GitHub, Provider::Slack, Provider::Ai];
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
    /// What the mail provider path has asked for and learned so far.
    mail: Mail,
}

/// What the search for a mail provider found ([`Request::DiscoverServices`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    /// A calendar service: the provider's name and its best address.
    Provider { name: String, url: String },
    /// A provider that does not work with Torro Workplace, and why (in the core's language).
    Unsupported { name: String, reason: String },
    /// Nothing answered for this domain; the server can still be typed under "Advanced".
    Nothing,
}

impl Found {
    fn of(provider: &str, urls: &[String], unsupported: &Option<String>) -> Found {
        match (unsupported, urls.first()) {
            (Some(reason), _) => Found::Unsupported { name: provider.to_string(), reason: reason.clone() },
            (None, Some(url)) => Found::Provider { name: provider.to_string(), url: url.clone() },
            (None, None) => Found::Nothing,
        }
    }
}

/// What the server of a signed-in account has ([`Response::Connected`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Connected {
    /// The account as the list shows it, for "Signed in as …".
    account: String,
    calendars: bool,
    tasks: bool,
}

/// The mail provider path: everything typed and learned. The password is a [`Secret`], so even a
/// `Debug` of the dialog shows none.
#[derive(Debug)]
struct Mail {
    address: String,
    /// The answer of the search for `address`; any edit of the address clears it.
    found: Option<Found>,
    /// The server under "Advanced": the address found, or one typed when nothing was found.
    server: String,
    /// The user name under "Advanced"; blank means the e-mail address.
    login: String,
    password: Secret,
    /// The core refused the password (`auth`): the text under the password field. Cleared by the
    /// next edit.
    refused: Option<String>,
    /// "Advanced" is open on the current step.
    advanced: bool,
    /// The services ticked on the last step.
    chosen: Vec<Service>,
    connected: Option<Connected>,
}

impl Default for Mail {
    fn default() -> Mail {
        Mail {
            address: String::new(),
            found: None,
            server: String::new(),
            login: String::new(),
            password: Secret::default(),
            refused: None,
            advanced: false,
            chosen: Service::ALL.into_iter().filter(|service| service.is_available()).collect(),
            connected: None,
        }
    }
}

impl Mail {
    /// The provider's name for the screens: the one found, else the domain of the address.
    fn provider_name(&self) -> String {
        match &self.found {
            Some(Found::Provider { name, .. }) => name.clone(),
            _ => domain_of(&self.address).to_string(),
        }
    }

    /// The server to sign in at when the person changed it or nothing was found; `None` lets the
    /// core try every address it finds for the e-mail address, best first.
    fn server_override(&self) -> Option<String> {
        let typed = self.server.trim();
        let found = match &self.found {
            Some(Found::Provider { url, .. }) => url.as_str(),
            _ => "",
        };
        (!typed.is_empty() && typed != found).then(|| typed.to_string())
    }

    /// The user name to sign in with: the one typed under "Advanced", else the e-mail address.
    fn login_name(&self) -> String {
        match self.login.trim() {
            "" => self.address.trim().to_string(),
            typed => typed.to_string(),
        }
    }
}

/// The part of an e-mail address after the last `@`; a domain typed without one stays whole.
fn domain_of(address: &str) -> &str {
    let address = address.trim();
    address.rsplit_once('@').map_or(address, |(_, domain)| domain)
}

/// The core's text for `auth` ("Password was refused", `pim.rs`). Compared with the message of an
/// error answer because the shell passes on messages, not codes.
fn password_refused(message: &str) -> bool {
    message == tr("Password was refused")
}

/// Providers that ask for an app password instead of the usual one, with the page of their help
/// that says how to create it. Matched with the provider's name from the search.
const APP_PASSWORDS: [(&str, &str); 2] = [
    ("iCloud", "https://support.apple.com/102654"),
    ("Fastmail", "https://www.fastmail.help/hc/en-us/articles/360058752854"),
];

/// The help page for creating an app password at `provider`, if it asks for one.
fn app_password_help(provider: &str) -> Option<&'static str> {
    APP_PASSWORDS.iter().find(|(name, _)| name.eq_ignore_ascii_case(provider)).map(|(_, url)| *url)
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
    /// The e-mail address changed (mail provider).
    AddressChanged(String),
    /// The server under "Advanced" changed (mail provider).
    MailServerChanged(String),
    /// The user name under "Advanced" changed (mail provider).
    LoginChanged(String),
    /// The password field changed. A [`Secret`] like the token.
    PasswordChanged(Secret),
    /// "Advanced" was pressed.
    ToggleAdvanced,
    /// A service row was pressed; unavailable services do nothing.
    ToggleService(Service),
    /// "Create an app password".
    OpenPasswordHelp,
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
    // An error answer that reached us as a response (the demo's) is the same as one the shell
    // turned into text.
    let as_error;
    let result = match result {
        Ok(Response::Error { message, .. }) => {
            as_error = Err(message.clone());
            &as_error
        }
        other => other,
    };
    match (state.step, result) {
        (Step::MailSearching, Ok(Response::Discovery { provider, urls, unsupported })) => {
            let found = Found::of(provider, urls, unsupported);
            state.mail.server = match &found {
                Found::Provider { url, .. } => url.clone(),
                _ => String::new(),
            };
            // Nothing found: the one way on is the server, so "Advanced" opens by itself.
            state.mail.advanced = found == Found::Nothing;
            state.mail.found = Some(found);
            state.step = Step::MailAddress;
            state.error = None;
            true
        }
        (Step::MailSearching, Ok(_)) => {
            state.step = Step::MailAddress;
            state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string());
            true
        }
        (Step::MailSearching, Err(message)) => {
            state.step = Step::MailAddress;
            state.error = Some(message.clone());
            true
        }
        (Step::MailConnecting, Ok(Response::Connected { account, calendars, tasks })) => {
            state.mail.connected =
                Some(Connected { account: account.label.clone(), calendars: *calendars, tasks: *tasks });
            state.step = Step::Done;
            state.error = None;
            true
        }
        (Step::MailConnecting, Ok(_)) => {
            state.step = Step::MailServices;
            state.error = Some(tr("Torro Workplace did not answer as expected. Try again.").to_string());
            true
        }
        // A refused password belongs under the password field; anything else is for the footer.
        (Step::MailConnecting, Err(message)) if password_refused(message) => {
            state.mail.refused = Some(message.clone());
            state.step = Step::MailPassword;
            true
        }
        (Step::MailConnecting, Err(message)) => {
            state.step = Step::MailServices;
            state.error = Some(message.clone());
            true
        }
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
            match state.step {
                Step::Token => state.step = Step::Github,
                Step::Github | Step::Server => state.step = Step::Provider,
                step @ (Step::MailAddress | Step::MailPassword | Step::MailServices) => go(state, step.mail_previous()),
                _ => {}
            }
            Action::None
        }
        Message::ChooseProvider(provider) => {
            if state.step == Step::Provider && provider.is_enabled() {
                state.provider = provider;
                state.step = provider.first_step();
            }
            Action::None
        }
        Message::AddressChanged(address) if state.step == Step::MailAddress => {
            // What was found belonged to the old address.
            state.mail = Mail { address, ..Mail::default() };
            state.error = None;
            Action::None
        }
        Message::MailServerChanged(server) => {
            state.mail.server = server;
            Action::None
        }
        Message::LoginChanged(login) => {
            state.mail.login = login;
            Action::None
        }
        Message::PasswordChanged(password) => {
            state.mail.password = password;
            state.mail.refused = None;
            Action::None
        }
        Message::ToggleAdvanced => {
            state.mail.advanced = !state.mail.advanced;
            Action::None
        }
        Message::ToggleService(service) => {
            if service.is_available() && state.step == Step::MailServices {
                match state.mail.chosen.iter().position(|chosen| *chosen == service) {
                    Some(at) => drop(state.mail.chosen.remove(at)),
                    None => state.mail.chosen.push(service),
                }
            }
            Action::None
        }
        Message::OpenPasswordHelp => match app_password_help(&state.mail.provider_name()) {
            Some(url) if state.step == Step::MailPassword => Action::OpenUrl(url.to_string()),
            _ => Action::None,
        },
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
            step @ (Step::MailAddress | Step::MailPassword | Step::MailServices) => continue_mail(state, step),
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
        Message::UseGithubCli | Message::PasteToken | Message::AddressChanged(_) => Action::None,
    }
}

/// Moves to `step` of the mail path. "Advanced" starts closed, except where nothing was found and
/// the server is the way on.
fn go(state: &mut State, step: Step) {
    state.step = step;
    state.error = None;
    state.mail.advanced = step == Step::MailAddress && state.mail.found == Some(Found::Nothing);
}

/// Whether Continue does something on `step` of the mail path.
fn mail_can_continue(state: &State, step: Step) -> bool {
    let mail = &state.mail;
    match step {
        Step::MailAddress => match &mail.found {
            None => !mail.address.trim().is_empty(),
            Some(Found::Provider { .. }) => true,
            // Nothing found: only a typed server leads on. A provider that does not work stops here.
            Some(Found::Nothing) => !mail.server.trim().is_empty(),
            Some(Found::Unsupported { .. }) => false,
        },
        Step::MailPassword => !mail.password.is_blank(),
        Step::MailServices => mail.chosen.iter().any(|service| service.is_available()),
        _ => false,
    }
}

/// Continue on a step of the mail path: search first, then walk [`MAIL_STEPS`]; at the end send
/// `ConnectCalDav`.
fn continue_mail(state: &mut State, step: Step) -> Action {
    if !mail_can_continue(state, step) {
        return Action::None;
    }
    if step == Step::MailAddress && state.mail.found.is_none() {
        state.step = Step::MailSearching;
        state.error = None;
        return Action::Send(Request::DiscoverServices { address: state.mail.address.trim().to_string() });
    }
    if let Some(next) = step.mail_next() {
        go(state, next);
        return Action::None;
    }
    let mail = &mut state.mail;
    let request = Request::ConnectCalDav {
        address: mail.address.trim().to_string(),
        server: mail.server_override(),
        login_name: mail.login_name(),
        // Taken, so the dialog keeps no copy; a refusal asks for it again.
        password: std::mem::take(&mut mail.password),
        calendars: mail.chosen.contains(&Service::Calendars),
        tasks: mail.chosen.contains(&Service::Tasks),
    };
    state.step = Step::MailConnecting;
    state.error = None;
    Action::Send(request)
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
        Step::MailAddress | Step::MailSearching => mail_address_step(state),
        Step::MailPassword => mail_password_step(state),
        Step::MailServices => mail_services_step(state),
        Step::MailConnecting => typo::text(typo::BODY, tr("Signing in…")).style(theme::text_2).into(),
        Step::Done if state.provider == Provider::Mail => mail_done_step(state),
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
        (Step::MailAddress, _) => tr("Enter the email address of your mailbox."),
        (Step::MailSearching, _) => tr("Looking for your provider."),
        (Step::MailPassword, _) => tr("Enter the password of your mailbox."),
        (Step::MailServices, _) => tr("Choose what to use from this account."),
        (Step::MailConnecting, _) => tr("Signing in to your provider."),
        (Step::Done, Provider::GitHub) => tr("GitHub is connected."),
        (Step::Done, Provider::Mail) => tr("Your account is connected."),
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
        provider_row(Provider::Mail, tr("Mail provider"), tr("Calendars and tasks")),
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
        coming_soon_row(None, name, summary, tr("Coming soon"))
    }
}

/// Something that is not available yet: the whole row – `mark`, name, summary and `tag` – at the
/// disabled opacity of 40 %, with the metrics of [`setting_row`]. A provider says "Coming soon",
/// a service of the mail provider "Coming later".
fn coming_soon_row<'a>(
    mark: Option<Element<'a, Message>>,
    name: &'static str,
    summary: &'static str,
    tag: &'static str,
) -> Element<'a, Message> {
    let faded = |colour: fn(&Palette) -> Color| {
        move |t: &Theme| text::Style { color: Some(colour(palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let lines = column![
        one_line(typo::text(typo::BODY, name).style(faded(|p| p.text))),
        one_line(typo::text(typo::META, summary).style(faded(|p| p.text_2))),
    ];
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::M).align_y(Alignment::Center);
    row_frame(
        row![text.width(Fill), widgets::pill_disabled(None, tag)].spacing(space::L).align_y(Alignment::Center),
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
pub(super) fn field<'a, M: Clone + 'a>(placeholder: &str, value: &str) -> text_input::TextInput<'a, M> {
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

// ----- The mail provider path (task M7.2) ---------------------------------------------------------

/// A group with one row of `content`, for a field.
fn field_group<'a>(
    title: &'static str,
    content: impl Into<Element<'a, Message>>,
    footer: Option<&'static str>,
) -> Element<'a, Message> {
    widgets::group(title, vec![row_frame(content, false)], footer.map(str::to_string))
}

/// An icon in one of the palette's colours.
fn status_icon<'a>(which: Icon, colour: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(colour(palette(t))) }).into()
}

/// A row of a group whose text wraps: a provider's reason or a translation can be long, and the
/// dialog is only as wide as the window. `title` in `BODY`, `text` below it in `META`/`text_2`.
fn note_row<'a>(mark: Element<'a, Message>, title: String, text: String) -> Element<'a, Message> {
    let lines = column![typo::text(typo::BODY, title), typo::text(typo::META, text).style(theme::text_2)];
    container(row![mark, lines.width(Fill)].spacing(space::M).align_y(Alignment::Center))
        .width(Fill)
        .padding([space::M, space::L])
        .into()
}

/// "Advanced" as a button with a chevron, and `content` below it while it is open.
fn advanced<'a>(open: bool, content: Element<'a, Message>) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let toggle =
        widgets::text_button(Some(chevron), tr("Advanced"), style::button::ghost, Some(Message::ToggleAdvanced));
    // Moved left by its own padding, so the chevron stands on the content axis of the fields.
    let on_axis = row![Space::new(), toggle].spacing(-style::BUTTON_PADDING.left);
    let mut block = column![on_axis].spacing(space::S);
    if open {
        block = block.push(content);
    }
    block.into()
}

/// The e-mail address and, below it, what the search found: the provider's name and "Advanced"
/// with the server, or the reason a provider does not work and nothing more, or the hint to type
/// the server when nothing answered.
fn mail_address_step(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let searching = state.step == Step::MailSearching;
    let input = field("anna@example.org", &mail.address)
        .on_input_maybe((!searching).then_some(Message::AddressChanged))
        .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
    let hint = tr("Only the part after @ is used to look for your provider.");
    let mut page = column![field_group(tr("Email address"), input, Some(hint))].spacing(space::XL);
    if searching {
        page = page.push(typo::text(typo::BODY, tr("Looking for your provider…")).style(theme::text_2));
    }
    let server = || {
        let input = field("https://dav.example.org", &mail.server)
            .on_input(Message::MailServerChanged)
            .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
        field_group(tr("Server"), input, Some(tr("Change this only if your provider gave you another address.")))
    };
    match &mail.found {
        Some(Found::Provider { name, .. }) => {
            let row = setting_row(
                Some(widgets::status_dot(widgets::Status::Ok)),
                name.as_str(),
                Some(tr("Calendar and task service found").to_string()),
                Space::new(),
            );
            page = page.push(widgets::group(tr("Provider"), vec![row], None));
            page = page.push(advanced(mail.advanced, server()));
        }
        Some(Found::Unsupported { name, reason }) => {
            let row = note_row(status_icon(Icon::CircleAlert, |p| p.attention), name.clone(), reason.clone());
            page = page.push(widgets::group(tr("Provider"), vec![row], None));
        }
        Some(Found::Nothing) => {
            let text = trf(
                "No calendar service found for {domain}. If you know its address, enter it under Advanced.",
                &[("domain", &domain_of(&mail.address))],
            );
            page = page.push(typo::text(typo::BODY, text).style(theme::text_2));
            page = page.push(advanced(mail.advanced, server()));
        }
        None => {}
    }
    page.into()
}

/// The provider found, what to enter, and the masked password field. A provider that asks for an
/// app password says so and links to its help; a refused password stands under the field.
fn mail_password_step(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let provider = mail.provider_name();
    // Who this is for, in one line of its own: the step is already tall with the hint and the field.
    let lines = column![
        typo::text(typo::BODY, provider.clone()),
        typo::text(typo::META, mail.address.trim().to_string()).style(theme::text_2)
    ];
    let context = row![status_icon(Icon::CircleCheck, |p| p.ok), lines].spacing(space::M).align_y(Alignment::Center);
    let mut page = column![context].spacing(space::XL);
    let mut hint = column![].spacing(space::S);
    if app_password_help(&provider).is_some() {
        hint = hint
            .push(typo::text(typo::BODY_STRONG, tr("Use an app password")))
            .push(
                typo::text(
                    typo::BODY,
                    trf(
                        "{provider} does not accept your usual password here. Create an app password in your account settings.",
                        &[("provider", &provider)],
                    ),
                )
                .style(theme::text_2),
            )
            .push(row![widgets::text_button(
                Some(Icon::ArrowUpRight),
                tr("Create an app password"),
                style::button::secondary,
                Some(Message::OpenPasswordHelp),
            )]);
    } else {
        hint = hint.push(
            typo::text(typo::BODY, tr("Use your mailbox password, or an app password if your provider offers them."))
                .style(theme::text_2),
        );
    }
    page = page.push(hint);
    let input = field("", mail.password.expose())
        .secure(true)
        .on_input(|password| Message::PasswordChanged(Secret::new(password)))
        .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
    let mut password = column![field_group(tr("Password"), input, None)].spacing(space::S);
    password = match &mail.refused {
        Some(refused) => password.push(
            typo::text(typo::META, refused.as_str())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        ),
        None => password.push(
            typo::text(typo::META, tr("The password stays in the system keychain. Agents never see it."))
                .style(theme::text_2),
        ),
    };
    page = page.push(password);
    let login = field_group(
        tr("User name"),
        field(mail.address.trim(), &mail.login).on_input(Message::LoginChanged),
        Some(tr("Leave empty to use your email address.")),
    );
    page.push(advanced(mail.advanced, login)).into()
}

/// The services as check boxes, the ones the server has not been asked about yet preselected;
/// the whole row ticks it. Services that are not available are greyed out with "Coming later".
fn mail_services_step(state: &State) -> Element<'_, Message> {
    let rows = Service::ALL
        .into_iter()
        .map(|service| {
            if !service.is_available() {
                // A box without a handler is drawn disabled.
                let off: Element<'_, Message> = checkbox(false).size(style::CHECK_SIZE).style(check_style).into();
                return coming_soon_row(Some(off), service.name(), service.summary(), tr("Coming later"));
            }
            let on = state.mail.chosen.contains(&service);
            let check = checkbox(on)
                .size(style::CHECK_SIZE)
                .style(check_style)
                .on_toggle(move |_| Message::ToggleService(service));
            let row =
                setting_row(Some(check.into()), service.name(), Some(service.summary().to_string()), Space::new());
            pressable(row, Some(Message::ToggleService(service)))
        })
        .collect();
    let footer = tr("You can show or hide single calendars and lists on the account's page later.");
    widgets::group(tr("Services"), rows, Some(footer.to_string()))
}

/// `text_2` for the box, not `text`: a black box would be the heaviest mark of the light dialog.
fn check_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    style::checkbox(palette(theme).text_2)(theme, status)
}

/// Who is signed in and, per chosen service, whether the server has it ([`done_lines`]).
fn mail_done_step(state: &State) -> Element<'_, Message> {
    let Some(connected) = &state.mail.connected else {
        return typo::text(typo::BODY, tr("Your calendars and task lists are ready.")).into();
    };
    let mut body = column![typo::text(typo::BODY, trf("Signed in as {account}.", &[("account", &connected.account)]))]
        .spacing(space::M);
    for (found, text) in done_lines(connected, &state.mail.chosen) {
        let which = if found {
            status_icon(Icon::CircleCheck, |p| p.ok)
        } else {
            status_icon(Icon::CircleAlert, |p| p.attention)
        };
        body = body.push(row![which, typo::text(typo::BODY, text)].spacing(space::S).align_y(Alignment::Center));
    }
    body.into()
}

/// One line per service that was chosen: whether the server has it, and the sentence for that.
/// Never colour alone: the sentence says it, the icon repeats it.
fn done_lines(connected: &Connected, chosen: &[Service]) -> Vec<(bool, &'static str)> {
    chosen
        .iter()
        .filter(|service| service.is_available())
        .map(|service| {
            let found = match service {
                Service::Calendars => connected.calendars,
                Service::Tasks => connected.tasks,
                Service::Mail => false,
            };
            (found, service.found_text(found))
        })
        .collect()
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
    let back =
        matches!(state.step, Step::Github | Step::Token | Step::MailAddress | Step::MailPassword | Step::MailServices)
            .then(|| widgets::text_button(None, tr("Back"), style::button::secondary, Some(Message::Back)));
    let cancel = widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match state.step {
        Step::Provider
        | Step::SigningIn
        | Step::Github
        | Step::Checking
        | Step::MailSearching
        | Step::MailConnecting => None,
        Step::Server => primary(tr("Continue"), can_continue(state).then_some(Message::Continue)),
        Step::MailAddress => {
            // The first press searches, the second goes on with what was found.
            let label = if state.mail.found.is_none() { tr("Find provider") } else { tr("Continue") };
            primary(label, mail_can_continue(state, state.step).then_some(Message::Continue))
        }
        Step::MailPassword => {
            primary(tr("Continue"), mail_can_continue(state, state.step).then_some(Message::Continue))
        }
        Step::MailServices => primary(tr("Connect"), mail_can_continue(state, state.step).then_some(Message::Continue)),
        Step::Token => primary(tr("Continue"), (!state.token.is_blank()).then_some(Message::Continue)),
        Step::Done => primary(tr("Done"), Some(Message::Close)),
    };
    row![error].push(back).push(cancel).push(next).spacing(space::S).align_y(Alignment::Center).into()
}

// ----- Demo states (`--demo --open wizard-mail-…`) --------------------------------------------

/// The states of the mail provider path that `torroworkplace ui --demo --open …` starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demo {
    /// The empty address step.
    Address,
    /// mailbox.org found, "Advanced" closed.
    Found,
    /// Gmail: the reason and nothing more.
    Unsupported,
    /// A domain nothing answers for: "Advanced" open with the empty server field.
    NotFound,
    /// iCloud: the password step with "Use an app password".
    Password,
    /// The password was refused: back at the password step with the text under the field.
    Refused,
    /// The services step.
    Services,
    /// Connected: what the server has.
    Done,
}

/// The messages that lead to `demo` the way a person would get there. The demo's answers to the
/// two requests come from `launch::demo_answer`; the password "wrong" is the one it refuses.
pub fn demo_messages(demo: Demo) -> Vec<Message> {
    let address = match demo {
        Demo::Address => None,
        Demo::Unsupported => Some("anna@gmail.com"),
        Demo::NotFound => Some("anna@example.org"),
        Demo::Password => Some("anna@icloud.com"),
        Demo::Found | Demo::Refused | Demo::Services | Demo::Done => Some("anna@mailbox.org"),
    };
    let mut steps = vec![Message::ChooseProvider(Provider::Mail)];
    if let Some(address) = address {
        // Search, then (found) on to the password.
        steps.extend([Message::AddressChanged(address.to_string()), Message::Continue]);
    }
    let password = |text: &str| Message::PasswordChanged(Secret::new(text));
    match demo {
        Demo::Password => steps.push(Message::Continue),
        Demo::Refused => steps.extend([Message::Continue, password("wrong"), Message::Continue, Message::Continue]),
        Demo::Services | Demo::Done => {
            steps.extend([Message::Continue, password("demo-password"), Message::Continue]);
            if demo == Demo::Done {
                steps.push(Message::Continue);
            }
        }
        Demo::Address | Demo::Found | Demo::Unsupported | Demo::NotFound => {}
    }
    steps
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
    fn three_providers_open_the_catalog() {
        assert_eq!(Provider::ALL.iter().filter(|provider| provider.is_enabled()).count(), 3);
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

    // ----- The mail provider path (task M7.2) ----------------------------------------------------

    /// Plays `messages` the way the shell does: a request goes to the demo's answers and the answer
    /// comes back through [`answered`].
    fn drive(state: &mut State, messages: Vec<Message>) -> Vec<Request> {
        let data = Data::default();
        let mut sent = Vec::new();
        for message in messages {
            if let Action::Send(request) = update(state, message, &data) {
                if let Some(response) = super::super::launch::demo_answer(&request, &data_with_caldav()) {
                    answered(state, &Ok(response));
                }
                sent.push(request);
            }
        }
        sent
    }

    /// The demo's data, which holds the CalDAV account its `ConnectCalDav` answer is built from.
    fn data_with_caldav() -> Data {
        super::super::launch::demo_data()
    }

    fn at_mail(demo: Demo) -> State {
        let mut state = State::new();
        drive(&mut state, demo_messages(demo));
        state
    }

    fn type_address(state: &mut State, address: &str) {
        update(state, Message::AddressChanged(address.into()), &Data::default());
    }

    #[test]
    fn the_mail_steps_are_a_list() {
        assert_eq!(Step::MailAddress.mail_next(), Some(Step::MailPassword));
        assert_eq!(Step::MailPassword.mail_next(), Some(Step::MailServices));
        assert_eq!(Step::MailServices.mail_next(), None, "the request is sent there");
        assert_eq!(Step::MailServices.mail_previous(), Step::MailPassword);
        assert_eq!(Step::MailAddress.mail_previous(), Step::Provider);
        assert_eq!(Provider::Mail.first_step(), Step::MailAddress);
        assert_eq!(Step::Server.mail_next(), None, "other providers are not on the list");
    }

    #[test]
    fn the_catalog_offers_the_mail_provider() {
        let mut state = State::new();
        update(&mut state, Message::ChooseProvider(Provider::Mail), &Data::default());
        assert_eq!((state.provider, state.step), (Provider::Mail, Step::MailAddress));
        assert_eq!(subtitle(&state), tr("Enter the email address of your mailbox."));
    }

    #[test]
    fn the_search_sends_the_address_and_shows_what_it_found() {
        let mut state = at_mail(Demo::Address);
        type_address(&mut state, "  anna@mailbox.org ");
        let action = update(&mut state, Message::Continue, &Data::default());
        assert!(
            matches!(action, Action::Send(Request::DiscoverServices { ref address }) if address == "anna@mailbox.org"),
            "{action:?}"
        );
        assert_eq!(state.step, Step::MailSearching);
        assert!(!state.busy(), "the answer comes with the request, no reload is needed");

        let found = Response::Discovery {
            provider: "mailbox.org".into(),
            urls: vec!["https://dav.mailbox.org/".into()],
            unsupported: None,
        };
        assert!(answered(&mut state, &Ok(found)));
        assert_eq!(state.step, Step::MailAddress);
        assert_eq!(
            state.mail.found,
            Some(Found::Provider { name: "mailbox.org".into(), url: "https://dav.mailbox.org/".into() })
        );
        assert_eq!(state.mail.server, "https://dav.mailbox.org/", "prefilled for Advanced");
        assert!(!state.mail.advanced, "the server is only a click away");

        // The second press goes on, to the password.
        update(&mut state, Message::Continue, &Data::default());
        assert_eq!(state.step, Step::MailPassword);
    }

    #[test]
    fn editing_the_address_forgets_what_was_found() {
        let mut state = at_mail(Demo::Found);
        assert!(state.mail.found.is_some());
        type_address(&mut state, "anna@posteo.de");
        assert_eq!((state.mail.found.as_ref(), state.mail.server.as_str()), (None, ""));
        // Searching again is what Continue does then.
        let action = update(&mut state, Message::Continue, &Data::default());
        assert!(matches!(action, Action::Send(Request::DiscoverServices { .. })));
    }

    #[test]
    fn an_empty_address_cannot_be_searched() {
        let mut state = at_mail(Demo::Address);
        for typed in ["", "   "] {
            type_address(&mut state, typed);
            assert!(!mail_can_continue(&state, Step::MailAddress));
            assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None));
            assert_eq!(state.step, Step::MailAddress);
        }
    }

    #[test]
    fn an_unsupported_provider_shows_its_reason_and_nothing_more() {
        let mut state = at_mail(Demo::Unsupported);
        let reason = tr("Google calendars need a sign-in through Google, which Torro Workplace does not support yet.");
        assert_eq!(state.mail.found, Some(Found::Unsupported { name: "Gmail".into(), reason: reason.into() }));
        assert!(!mail_can_continue(&state, Step::MailAddress));
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None));
        assert_eq!(state.step, Step::MailAddress, "no way on");
        drop(view(&state, &Data::default()));
        // Another address searches again.
        type_address(&mut state, "anna@mailbox.org");
        assert!(mail_can_continue(&state, Step::MailAddress));
    }

    #[test]
    fn nothing_found_opens_advanced_and_asks_for_the_server() {
        let mut state = at_mail(Demo::NotFound);
        assert_eq!(state.mail.found, Some(Found::Nothing));
        assert!(state.mail.advanced, "the server is the way on");
        assert!(!mail_can_continue(&state, Step::MailAddress), "until one is typed");
        update(&mut state, Message::MailServerChanged("  dav.example.org ".into()), &Data::default());
        assert!(mail_can_continue(&state, Step::MailAddress));
        update(&mut state, Message::Continue, &Data::default());
        assert_eq!(state.step, Step::MailPassword);
        assert_eq!(state.mail.server_override().as_deref(), Some("dav.example.org"));
    }

    #[test]
    fn a_failed_search_stays_on_the_address_with_the_message() {
        let mut state = at_mail(Demo::Address);
        type_address(&mut state, "anna@mailbox.org");
        update(&mut state, Message::Continue, &Data::default());
        assert!(answered(&mut state, &Err("The server cannot be reached.".into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::MailAddress, Some("The server cannot be reached.")));
        assert_eq!(state.mail.found, None, "the person can search again");
        // An answer outside a search is not the dialog's.
        let found = Response::Discovery { provider: "x".into(), urls: vec![], unsupported: None };
        assert!(!answered(&mut State::new(), &Ok(found)));
    }

    #[test]
    fn the_password_step_needs_a_password_and_hints_at_app_passwords() {
        let mut state = at_mail(Demo::Password);
        assert_eq!(state.step, Step::MailPassword);
        assert_eq!(state.mail.provider_name(), "iCloud");
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None), "no password");
        update(&mut state, Message::PasswordChanged(Secret::new("   ")), &Data::default());
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None), "blank");
        match update(&mut state, Message::OpenPasswordHelp, &Data::default()) {
            Action::OpenUrl(url) => assert!(url.starts_with("https://support.apple.com/")),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        assert!(app_password_help("Fastmail").is_some_and(|url| url.starts_with("https://www.fastmail.help/")));
        assert_eq!(app_password_help("mailbox.org"), None);
        let mut other = at_mail(Demo::Found);
        update(&mut other, Message::Continue, &Data::default());
        assert!(matches!(update(&mut other, Message::OpenPasswordHelp, &Data::default()), Action::None));
    }

    #[test]
    fn the_services_are_ticked_but_mail_is_not_available() {
        let mut state = at_mail(Demo::Services);
        assert_eq!(state.step, Step::MailServices);
        assert_eq!(state.mail.chosen, [Service::Calendars, Service::Tasks]);
        assert!(!Service::Mail.is_available());
        update(&mut state, Message::ToggleService(Service::Mail), &Data::default());
        assert_eq!(state.mail.chosen, [Service::Calendars, Service::Tasks], "Mail does nothing yet");
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        assert_eq!(state.mail.chosen, [Service::Calendars]);
        update(&mut state, Message::ToggleService(Service::Calendars), &Data::default());
        assert!(!mail_can_continue(&state, Step::MailServices), "at least one service");
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None));
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        assert_eq!(state.mail.chosen, [Service::Tasks]);
    }

    #[test]
    fn connect_sends_the_choice_and_keeps_no_password() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        let action = update(&mut state, Message::Continue, &Data::default());
        match &action {
            Action::Send(Request::ConnectCalDav { address, server, login_name, password, calendars, tasks }) => {
                assert_eq!(address, "anna@mailbox.org");
                assert_eq!(server, &None, "the core tries every address it finds");
                assert_eq!(login_name, "anna@mailbox.org", "the address is the user name unless changed");
                assert_eq!(password.expose(), "demo-password");
                assert_eq!((*calendars, *tasks), (true, false));
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
        assert!(!format!("{action:?}").contains("demo-password"), "a Secret shows nothing");
        assert!(state.mail.password.is_blank(), "the dialog keeps no copy");
        assert!(!format!("{state:?}").contains("demo-password"));
        assert_eq!(state.step, Step::MailConnecting);
    }

    #[test]
    fn a_typed_user_name_and_server_go_with_the_request() {
        let mut state = at_mail(Demo::Found);
        update(&mut state, Message::Continue, &Data::default());
        update(&mut state, Message::LoginChanged(" anna ".into()), &Data::default());
        update(&mut state, Message::PasswordChanged(Secret::new("pw")), &Data::default());
        update(&mut state, Message::Continue, &Data::default());
        // The server was changed under Advanced, not left as found.
        state.mail.server = "https://other.example.org/dav/".into();
        match update(&mut state, Message::Continue, &Data::default()) {
            Action::Send(Request::ConnectCalDav { server, login_name, .. }) => {
                assert_eq!(server.as_deref(), Some("https://other.example.org/dav/"));
                assert_eq!(login_name, "anna");
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
    }

    #[test]
    fn a_refused_password_comes_back_under_the_password_field() {
        let mut state = at_mail(Demo::Refused);
        assert_eq!(state.step, Step::MailPassword);
        assert_eq!(state.mail.refused.as_deref(), Some(tr("Password was refused")));
        assert!(state.error.is_none(), "not in the footer");
        assert!(state.mail.password.is_blank(), "to type again");
        update(&mut state, Message::PasswordChanged(Secret::new("x")), &Data::default());
        assert_eq!(state.mail.refused, None, "an edit clears it");
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn any_other_error_goes_to_the_footer_of_the_services_step() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::Continue, &Data::default());
        assert!(answered(&mut state, &Err("The server cannot be reached.".into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::MailServices, Some("The server cannot be reached.")));
        assert_eq!(state.mail.refused, None);
    }

    #[test]
    fn done_says_what_the_server_has() {
        let state = at_mail(Demo::Done);
        assert_eq!(state.step, Step::Done);
        assert_eq!(subtitle(&state), tr("Your account is connected."));
        let connected = state.mail.connected.clone().expect("connected");
        assert_eq!(
            done_lines(&connected, &state.mail.chosen),
            [(true, tr("Calendars are ready.")), (true, tr("Tasks are ready."))]
        );
        let no_tasks = Connected { account: "anna@mailbox.org".into(), calendars: true, tasks: false };
        assert_eq!(
            done_lines(&no_tasks, &[Service::Tasks, Service::Mail]),
            [(false, tr("This server has no task lists."))],
            "only what was chosen, and only what exists"
        );
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn back_walks_the_mail_steps_in_reverse() {
        let mut state = at_mail(Demo::Services);
        for expected in [Step::MailPassword, Step::MailAddress, Step::Provider] {
            update(&mut state, Message::Back, &Data::default());
            assert_eq!(state.step, expected);
        }
        update(&mut state, Message::ChooseProvider(Provider::Mail), &Data::default());
        assert_eq!(state.step, Step::MailAddress);
    }

    #[test]
    fn every_demo_state_leads_where_it_says() {
        let at = |demo| at_mail(demo);
        assert_eq!((at(Demo::Address).step, at(Demo::Address).mail.found), (Step::MailAddress, None));
        assert!(matches!(at(Demo::Found).mail.found, Some(Found::Provider { .. })));
        assert!(matches!(at(Demo::Unsupported).mail.found, Some(Found::Unsupported { .. })));
        assert_eq!(at(Demo::NotFound).mail.found, Some(Found::Nothing));
        assert_eq!(at(Demo::Password).step, Step::MailPassword);
        assert_eq!(at(Demo::Refused).step, Step::MailPassword);
        assert!(at(Demo::Refused).mail.refused.is_some());
        assert_eq!(at(Demo::Services).step, Step::MailServices);
        assert_eq!(at(Demo::Done).step, Step::Done);
    }

    #[test]
    fn the_mail_views_build_in_every_state() {
        for demo in [
            Demo::Address,
            Demo::Found,
            Demo::Unsupported,
            Demo::NotFound,
            Demo::Password,
            Demo::Refused,
            Demo::Services,
            Demo::Done,
        ] {
            let mut state = at_mail(demo);
            drop(view(&state, &Data::default()));
            state.mail.advanced = true;
            drop(view(&state, &Data::default()));
        }
        let mut searching = at_mail(Demo::Address);
        type_address(&mut searching, "anna@mailbox.org");
        update(&mut searching, Message::Continue, &Data::default());
        assert_eq!(searching.step, Step::MailSearching);
        drop(view(&searching, &Data::default()));
    }
}
