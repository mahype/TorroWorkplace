//! The add-account dialog (task S.6 in `docs/tasks/steuerzentrale.md`, GitHub since G.1).
//!
//! Steps, with the subtitle changing per step, errors bottom left, "Cancel" and the primary
//! button bottom right (like TorroMail's setup wizard):
//!
//! 1. **Provider:** a catalog – Nextcloud (calendars, tasks), mail, GitHub (issues, projects) and
//!    AI providers enabled; Slack listed as "Coming soon", disabled. With a single available
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
//! 4. Mail account (tasks M7.2, M5.1): the path of [`MAIL_STEPS`] – **Address** (an e-mail address;
//!    "Find provider" sends `Request::DiscoverServices`, which is only a search and needs no
//!    password), the answer shown on the same step (the provider's name and what it has – mail,
//!    calendars and tasks –, a note where mail needs an app password, or the reason a provider is
//!    not supported and nothing more). "Advanced" opens the IMAP and SMTP servers (host, port,
//!    security) and the CalDAV address, filled from what was found; it opens by itself when
//!    nothing was found or the SMTP server is missing, because typing them is the way on.
//!    **Password** (with "Use an app password" and a link where a provider asks for one; the user
//!    name under "Advanced"), **Services** (check boxes from [`Service::ALL`]: Mail, Calendars,
//!    Tasks – the ones the servers have are ticked, the others say "Not found" – and, with Mail,
//!    the sender name). "Connect" sends `Request::ConnectMail`, or `Request::ConnectCalDav` when
//!    only calendars and tasks are ticked, and waits in **Connecting**. A refused password
//!    (`auth`) comes back to the Password step, under the field, with the server that refused it.
//!    The steps and the check boxes are data, so a step or a service is added without a new screen.
//! 5. AI providers (task KI.1a, Open First): **AI** – opening it sends `Request::FindAiServers`;
//!    "On this computer" lists Ollama and LM Studio, each found one with its models and
//!    "Connect" (one press sends `Request::ConnectAi` for its address, no key), the others as "Not
//!    running on this computer". Below, OpenRouter opens **API key** (a masked field and a link to
//!    OpenRouter's key page) and "Other OpenAI-compatible server" opens **Server** (address and an
//!    optional key). "Connect" sends `Request::ConnectAi` and waits in **Checking**; an error comes
//!    back to the step it was sent from, in the footer.
//! 6. **Done:** "Your calendars and task lists are ready." (Nextcloud), "Signed in to GitHub as
//!    …" (GitHub), "… is ready to use." (AI) or what the mail account's servers have, and "Done".
//!
//! **TorroMail** (task M7.4): the dialog opens with `Request::TorroMailAccounts` ([`State::open`]).
//! When the separate TorroMail app has accounts, the catalog starts with "Use accounts from
//! TorroMail?": one row per account with its address and servers and "Use", which sends
//! `Request::ImportTorroMailAccount` for mail only (pressing it is the consent to copy the password); accounts the
//! suite cannot take (OAuth, JMAP) are greyed out with the reason, those it has already say
//! "Connected". A take-over runs on the catalog itself, one at a time; when it fails (TorroMail's
//! keychain entry cannot be read, a server refuses), the row shows why and "Enter password", which
//! opens the mail path at the password step, filled with TorroMail's servers and user name. After a
//! take-over the catalog ends with "Done". Without TorroMail, or when the list cannot be read, the
//! catalog is as before.
//!
//! How a step learns that it is over:
//!
//! - The shell hands every answer to a request to [`answered`] while the dialog is open. GitHub's
//!   answer is the account or an error; a failed `ConnectNextcloud` (server not found) comes back
//!   the same way, and so do the answers of the mail account's requests. The shell's `send`
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
use torro_core::ai::AiProvider;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, space};
use torro_gui::{dialog, layout, style, typo, widgets};
use torro_ipc::{AccountInfo, AiServer, Request, Response, TorroMailAccount};
use torro_mail::{Endpoint, Security};
use torro_pim::AccountState;

use super::server_fields::{Server, ServerFields, imap_titles, server_group, smtp_titles};
use super::{Action, Data, chevron, one_line, pressable, row_frame, setting_row};

/// GitHub's page for a new classic token, with the scope the suite needs (`repo`: private
/// repositories, closing issues) and a name already filled in.
pub const NEW_TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo&description=Torro%20Workplace";

/// OpenRouter's page for API keys (task KI.1a).
pub const OPENROUTER_KEYS_URL: &str = "https://openrouter.ai/settings/keys";

/// The AI servers "On this computer" lists, in this order (Open First).
const LOCAL_AI: [AiProvider; 2] = [AiProvider::Ollama, AiProvider::LmStudio];

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
    /// AI providers: the servers on this computer, OpenRouter and any other server.
    AiChoose,
    /// AI providers: OpenRouter's API key.
    AiKey,
    /// AI providers: the address (and maybe a key) of another OpenAI-compatible server.
    AiServer,
    /// AI providers: waiting for the core to check the server (`ConnectAi`).
    AiConnecting,
    Done,
}

/// The steps of the mail account path, in order. Back and Continue walk this list; the IMAP and
/// SMTP settings are under "Advanced" of the address step, so a step is added here without a new
/// screen for the others.
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

/// What a mail account can bring to the suite. A row of the services step each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Mail,
    Calendars,
    Tasks,
}

impl Service {
    const ALL: [Service; 3] = [Service::Mail, Service::Calendars, Service::Tasks];

    /// Mail needs both mail servers, calendars and tasks need the CalDAV address; what the search
    /// did not find (and nobody typed under "Advanced") is listed greyed out.
    fn is_available(self, mail: &Mail) -> bool {
        match self {
            Service::Mail => mail.servers() == Servers::Ready,
            Service::Calendars | Service::Tasks => mail.caldav_address().is_some(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Service::Mail => tr("Mail"),
            Service::Calendars => tr("Calendars"),
            Service::Tasks => tr("Tasks"),
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Service::Mail => tr("Read and send email"),
            Service::Calendars => tr("Appointments and events"),
            Service::Tasks => tr("To-dos and reminders"),
        }
    }

    /// The sentence on the last step for this service: whether the server has it.
    fn found_text(self, found: bool) -> &'static str {
        match (self, found) {
            (Service::Mail, _) => tr("Mail is ready."),
            (Service::Calendars, true) => tr("Calendars are ready."),
            (Service::Calendars, false) => tr("This server has no calendars."),
            (Service::Tasks, true) => tr("Tasks are ready."),
            (Service::Tasks, false) => tr("This server has no task lists."),
        }
    }
}

/// A service in the catalog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Nextcloud,
    /// A mail account: its mail, and the calendars and tasks of the same sign-in (tasks M7.2, M5.1).
    Mail,
    GitHub,
    Slack,
    Ai,
}

impl Provider {
    /// Nextcloud, a mail provider, GitHub and AI providers can be set up; Slack is listed so the
    /// catalog shows where the suite is heading.
    fn is_enabled(self) -> bool {
        matches!(self, Provider::Nextcloud | Provider::Mail | Provider::GitHub | Provider::Ai)
    }

    /// The step after picking the provider.
    fn first_step(self) -> Step {
        match self {
            Provider::GitHub => Step::Github,
            Provider::Mail => MAIL_STEPS[0],
            Provider::Ai => Step::AiChoose,
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
    /// TorroMail's accounts on the catalog (task M7.4).
    torromail: Offer,
    /// What the AI provider path has asked for and learned so far (task KI.1a).
    ai: AiSetup,
}

/// The AI provider path (task KI.1a): the servers found on this computer, what was typed, and the
/// account once connected. The key is a [`Secret`], so even a `Debug` of the dialog shows none.
#[derive(Debug, Default)]
struct AiSetup {
    /// `FindAiServers` was sent and has not answered.
    searching: bool,
    /// The servers that answered on this computer; `None` before the answer.
    found: Option<Vec<AiServer>>,
    /// The address typed for another server.
    server: String,
    key: Secret,
    /// The address of the local server being connected, for its row.
    connecting: Option<String>,
    /// The connected account's name, for Done.
    connected: Option<String>,
}

/// TorroMail's accounts on the catalog (task M7.4): asked for when the dialog opens
/// ([`State::open`]), each taken over with one press, one at a time.
#[derive(Debug, Default)]
struct Offer {
    /// The list was asked for and has not come back yet.
    asking: bool,
    /// As the core listed them; empty when TorroMail has none or the list failed.
    accounts: Vec<TorroMailAccount>,
    /// TorroMail's id of the account being taken over.
    importing: Option<String>,
    /// The take-over that failed last: TorroMail's id and the core's message.
    failed: Option<(String, String)>,
    /// An account was taken over in this dialog: the catalog ends with "Done".
    imported: bool,
}

impl Offer {
    /// "Use" on `id` sends something: TorroMail's account can be taken over and is not yet, and no
    /// other take-over runs.
    fn can_use(&self, id: &str) -> bool {
        self.importing.is_none()
            && self
                .accounts
                .iter()
                .any(|account| account.id == id && account.unsupported.is_none() && !account.connected)
    }
}

/// How far the mail servers are filled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Servers {
    /// Neither server: no mail from this account (calendars and tasks only).
    Missing,
    /// One server, or a port that is no port: the person is on the way to mail and must finish it.
    Incomplete,
    /// Both servers, with hosts and ports.
    Ready,
}

/// What the search for a mail account found ([`Request::DiscoverServices`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The provider's name, or the domain when it is not known.
    name: String,
    /// The best CalDAV address of the provider.
    caldav: Option<String>,
    /// The mail servers; the SMTP server can be missing while the IMAP server is there.
    imap: Option<Endpoint>,
    smtp: Option<Endpoint>,
    /// The user name to try, when the provider says which.
    login_hint: Option<String>,
    /// The provider wants an app password instead of the account password.
    app_password: bool,
    /// Why mail or calendars do not work with the usual sign-in, in the core's language. With
    /// servers it is a hint (Gmail with an app password), without them the end of the way.
    note: Option<String>,
}

impl Found {
    /// Something to sign in to: a mail server or a calendar service.
    fn is_usable(&self) -> bool {
        self.imap.is_some() || self.caldav.is_some()
    }

    /// A provider that does not work with Torro Workplace, and why; nothing more to show.
    fn is_unsupported(&self) -> bool {
        !self.is_usable() && self.note.is_some()
    }

    /// Nothing answered for this domain; the servers can still be typed under "Advanced".
    fn is_nothing(&self) -> bool {
        !self.is_usable() && self.note.is_none()
    }

    /// What the provider has, for the line under its name.
    fn summary(&self) -> &'static str {
        match (self.imap.is_some(), self.caldav.is_some()) {
            (true, true) => tr("Mail, calendars and tasks found"),
            (true, false) => tr("Mail server found"),
            _ => tr("Calendar and task service found"),
        }
    }
}

/// What the servers of a signed-in account have ([`Response::Connected`], [`Response::MailConnected`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Connected {
    /// The account as the list shows it, for "Signed in as …".
    account: String,
    calendars: bool,
    tasks: bool,
}

/// The mail account path: everything typed and learned. The password is a [`Secret`], so even a
/// `Debug` of the dialog shows none.
#[derive(Debug)]
struct Mail {
    address: String,
    /// The answer of the search for `address`; any edit of the address clears it.
    found: Option<Found>,
    /// The servers under "Advanced": those found, or the ones typed when nothing was found.
    imap: ServerFields,
    smtp: ServerFields,
    /// The CalDAV address under "Advanced": the one found or one typed; blank for none.
    caldav: String,
    /// The user name under "Advanced"; blank means the one the search suggests, else the e-mail
    /// address.
    login: String,
    password: Secret,
    /// The core refused the password (`auth`): the text under the password field, naming the
    /// server. Cleared by the next edit.
    refused: Option<String>,
    /// "Advanced" is open on the current step.
    advanced: bool,
    /// The name next to the address in sent mail; blank for none.
    sender_name: String,
    /// The services ticked on the last step; `None` until the person ticks one, then all the
    /// servers have are chosen ([`Mail::services`]).
    chosen: Option<Vec<Service>>,
    connected: Option<Connected>,
}

impl Default for Mail {
    fn default() -> Mail {
        Mail {
            address: String::new(),
            found: None,
            imap: ServerFields::empty(Server::Imap),
            smtp: ServerFields::empty(Server::Smtp),
            caldav: String::new(),
            login: String::new(),
            password: Secret::default(),
            refused: None,
            advanced: false,
            sender_name: String::new(),
            chosen: None,
            connected: None,
        }
    }
}

impl Mail {
    /// The provider's name for the screens: the one found, else the domain of the address.
    fn provider_name(&self) -> String {
        match &self.found {
            Some(found) if !found.name.is_empty() => found.name.clone(),
            _ => domain_of(&self.address).to_string(),
        }
    }

    /// The server field of `server`.
    fn server_mut(&mut self, server: Server) -> &mut ServerFields {
        match server {
            Server::Imap => &mut self.imap,
            Server::Smtp => &mut self.smtp,
        }
    }

    /// How far the two mail servers are filled in.
    fn servers(&self) -> Servers {
        match (self.imap.is_blank(), self.smtp.is_blank()) {
            (true, true) => Servers::Missing,
            _ if self.imap.endpoint().is_some() && self.smtp.endpoint().is_some() => Servers::Ready,
            _ => Servers::Incomplete,
        }
    }

    /// The two mail servers once both are filled in.
    fn mail_servers(&self) -> Option<(Endpoint, Endpoint)> {
        Some((self.imap.endpoint()?, self.smtp.endpoint()?))
    }

    /// The CalDAV address, if one is there.
    fn caldav_address(&self) -> Option<&str> {
        Some(self.caldav.trim()).filter(|address| !address.is_empty())
    }

    /// The address step has what it needs to go on: mail servers, or a CalDAV address without
    /// them. Half a mail setup is not enough.
    fn is_ready(&self) -> bool {
        match self.servers() {
            Servers::Ready => true,
            Servers::Missing => self.caldav_address().is_some(),
            Servers::Incomplete => false,
        }
    }

    /// After a search, "Advanced" opens by itself where the person has to type something.
    fn needs_input(&self) -> bool {
        self.found.as_ref().is_some_and(|found| !found.is_unsupported()) && !self.is_ready()
    }

    /// The services the account takes: the ones ticked, or all of them as long as none was ticked;
    /// never one the servers lack.
    fn services(&self) -> Vec<Service> {
        let chosen = self.chosen.clone().unwrap_or_else(|| Service::ALL.to_vec());
        Service::ALL.into_iter().filter(|service| chosen.contains(service) && service.is_available(self)).collect()
    }

    /// The CalDAV address to sign in at when the person changed it or nothing was found; `None`
    /// lets the core try every address it finds for the e-mail address, best first.
    fn server_override(&self) -> Option<String> {
        let typed = self.caldav_address()?;
        let found = self.found.as_ref().and_then(|found| found.caldav.as_deref()).unwrap_or_default();
        (typed != found).then(|| typed.to_string())
    }

    /// The user name to sign in with: the one typed under "Advanced", else the one the provider
    /// suggests, else the e-mail address.
    fn login_name(&self) -> String {
        match (self.login.trim(), self.found.as_ref().and_then(|found| found.login_hint.as_deref())) {
            ("", Some(hint)) => hint.to_string(),
            ("", None) => self.address.trim().to_string(),
            (typed, _) => typed.to_string(),
        }
    }
}

/// The part of an e-mail address after the last `@`; a domain typed without one stays whole.
fn domain_of(address: &str) -> &str {
    let address = address.trim();
    address.rsplit_once('@').map_or(address, |(_, domain)| domain)
}

/// The names of the servers in a refusal of the password (`mail/service.rs`): the same in every
/// language.
const SERVER_NAMES: [&str; 3] = ["IMAP", "SMTP", "CalDAV"];

/// The core's text for `auth`: "Password was refused" (`pim.rs`, a CalDAV account) or "The {service}
/// server refused the password." (`accounts.connect_mail`, naming the server). Compared with the
/// message of an error answer because the shell passes on messages, not codes.
fn password_refused(message: &str) -> bool {
    message == tr("Password was refused")
        || SERVER_NAMES
            .iter()
            .any(|service| message == trf("The {service} server refused the password.", &[("service", service)]))
}

/// Providers that ask for an app password instead of the usual one, with the page of their help
/// that says how to create it. Matched with the provider's name from the search.
const APP_PASSWORDS: [(&str, &str); 4] = [
    ("iCloud", "https://support.apple.com/102654"),
    ("Fastmail", "https://www.fastmail.help/hc/en-us/articles/360058752854"),
    ("Yahoo", "https://help.yahoo.com/kb/SLN15241.html"),
    ("Gmail", "https://support.google.com/accounts/answer/185833"),
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

    /// A new dialog and the request that asks for TorroMail's accounts (task M7.4), whose answer
    /// fills the top of the catalog ([`answered`]). How the accounts page opens the dialog.
    pub fn open() -> (State, Action) {
        let mut state = State::new();
        state.torromail.asking = true;
        (state, Action::Send(Request::TorroMailAccounts))
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
    /// The e-mail address changed (mail account).
    AddressChanged(String),
    /// The host of a mail server under "Advanced" changed.
    ServerHostChanged(Server, String),
    /// The port of a mail server under "Advanced" changed.
    ServerPortChanged(Server, String),
    /// A mail server's security was picked under "Advanced".
    ServerSecurityPicked(Server, Security),
    /// The CalDAV address under "Advanced" changed.
    CaldavChanged(String),
    /// The user name under "Advanced" changed.
    LoginChanged(String),
    /// The sender name on the last step changed.
    SenderNameChanged(String),
    /// The password field changed. A [`Secret`] like the token.
    PasswordChanged(Secret),
    /// "Advanced" was pressed.
    ToggleAdvanced,
    /// A service row was pressed; unavailable services do nothing.
    ToggleService(Service),
    /// "Create an app password".
    OpenPasswordHelp,
    /// "Use" on a TorroMail account, by TorroMail's id (task M7.4).
    UseTorroMail(String),
    /// "Enter password" on a TorroMail account whose take-over failed: the mail path, filled in.
    EnterTorroMailPassword(String),
    /// "Connect" on a server found on this computer: its provider and address (task KI.1a).
    ConnectLocalAi(AiProvider, String),
    /// OpenRouter or "Other OpenAI-compatible server" was pressed: its form.
    ChooseAi(AiProvider),
    /// The address of another server changed.
    AiServerChanged(String),
    /// The API key field changed. A [`Secret`] like the token.
    AiKeyChanged(Secret),
    /// "Create a key on OpenRouter".
    OpenAiKeyPage,
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
    if let Some(taken) = offer_answered(state, result) {
        return taken;
    }
    match (state.step, result) {
        (
            Step::MailSearching,
            Ok(Response::Discovery {
                provider,
                urls,
                unsupported,
                imap,
                smtp,
                login_hint,
                app_password,
                mail_unsupported,
            }),
        ) => {
            let found = Found {
                name: provider.clone(),
                caldav: urls.first().cloned(),
                imap: imap.clone(),
                smtp: smtp.clone(),
                login_hint: login_hint.clone(),
                app_password: *app_password,
                // The reason for mail is the one that matters here; the one for calendars only
                // stands in where mail has none.
                note: mail_unsupported.clone().or_else(|| unsupported.clone()),
            };
            let mail = &mut state.mail;
            mail.imap = found.imap.as_ref().map_or_else(|| ServerFields::empty(Server::Imap), ServerFields::of);
            mail.smtp = found.smtp.as_ref().map_or_else(|| ServerFields::empty(Server::Smtp), ServerFields::of);
            mail.caldav = found.caldav.clone().unwrap_or_default();
            mail.found = Some(found);
            // Where nothing or only half of the servers was found, typing them is the way on, so
            // "Advanced" opens by itself.
            mail.advanced = mail.needs_input();
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
        (
            Step::MailConnecting,
            Ok(
                Response::Connected { account, calendars, tasks }
                | Response::MailConnected { account, calendars, tasks, .. },
            ),
        ) => {
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
        // A server on this computer was connected from its row; the catalog waits on that row.
        (Step::AiChoose, Ok(Response::Account { account })) if state.ai.connecting.is_some() => {
            state.ai.connected = Some(account.label.clone());
            state.ai.connecting = None;
            state.step = Step::Done;
            state.error = None;
            true
        }
        (Step::AiChoose, result) if state.ai.connecting.is_some() => {
            state.ai.connecting = None;
            state.error = Some(match result {
                Err(message) => message.clone(),
                Ok(_) => tr("Torro Workplace did not answer as expected. Try again.").to_string(),
            });
            true
        }
        (Step::AiChoose, Ok(Response::AiServers { servers })) if state.ai.searching => {
            state.ai.searching = false;
            state.ai.found = Some(servers.clone());
            true
        }
        // The search is a convenience: without an answer every local server shows as not running.
        (Step::AiChoose, result) if state.ai.searching => {
            state.ai.searching = false;
            state.ai.found = Some(Vec::new());
            state.error = result.as_ref().err().cloned();
            true
        }
        (Step::AiConnecting, Ok(Response::Account { account })) => {
            state.ai.connected = Some(account.label.clone());
            state.step = Step::Done;
            state.error = None;
            true
        }
        (Step::AiConnecting, result) => {
            state.step = state.back;
            state.error = Some(match result {
                Err(message) => message.clone(),
                Ok(_) => tr("Torro Workplace did not answer as expected. Try again.").to_string(),
            });
            true
        }
        _ => false,
    }
}

/// Sends `ConnectAi` for `provider` at `server` with the key typed and waits; an error comes back
/// to the form it was sent from. The dialog keeps no copy of the key.
fn connect_ai(state: &mut State, provider: AiProvider, server: Option<String>) -> Action {
    state.back = state.step;
    state.step = Step::AiConnecting;
    state.error = None;
    let key = std::mem::take(&mut state.ai.key);
    Action::Send(Request::ConnectAi { provider, server, key })
}

/// Whether Connect does something on an AI form: OpenRouter needs a key, another server an address.
fn ai_can_connect(state: &State) -> bool {
    match state.step {
        Step::AiKey => !state.ai.key.is_blank(),
        Step::AiServer => !state.ai.server.trim().is_empty(),
        _ => false,
    }
}

/// The answers to TorroMail's requests (task M7.4): the list whenever it comes, the outcome of a
/// take-over while one runs, and a list that failed while the dialog is still on the catalog –
/// silently, since the offer is a shortcut and the catalog works without it. `None` for every
/// other answer, which the steps take.
fn offer_answered(state: &mut State, result: &Result<Response, String>) -> Option<bool> {
    let offer = &mut state.torromail;
    if let Ok(Response::TorroMailAccounts { accounts }) = result {
        offer.asking = false;
        offer.accounts = accounts.clone();
        return Some(true);
    }
    if let Some(id) = offer.importing.take() {
        match result {
            Ok(Response::MailConnected { .. }) => {
                if let Some(account) = offer.accounts.iter_mut().find(|account| account.id == id) {
                    account.connected = true;
                }
                offer.failed = None;
                offer.imported = true;
            }
            Ok(_) => offer.failed = Some((id, tr("Torro Workplace did not answer as expected. Try again.").into())),
            Err(message) => offer.failed = Some((id, message.clone())),
        }
        return Some(true);
    }
    if offer.asking && state.step == Step::Provider && result.is_err() {
        offer.asking = false;
        return Some(true);
    }
    None
}

/// The mail path at the password step, filled with what TorroMail has for `account`: the way on
/// when its take-over failed (task M7.4). Back leads to the address step with TorroMail's servers
/// as found; a CalDAV address can be added there under "Advanced".
fn fill_from_torromail(state: &mut State, account: &TorroMailAccount) {
    let (Some(imap), Some(smtp)) = (&account.imap, &account.smtp) else { return };
    // TorroMail's generic label says nothing; a provider's name ("iCloud") finds its app password help.
    let name = match account.provider.as_str() {
        "" | "IMAP/SMTP" => domain_of(&account.address).to_string(),
        provider => provider.to_string(),
    };
    let found = Found {
        name,
        caldav: None,
        imap: Some(imap.clone()),
        smtp: Some(smtp.clone()),
        login_hint: Some(account.login_name.clone()).filter(|login| !login.trim().is_empty()),
        app_password: false,
        note: None,
    };
    state.mail = Mail {
        address: account.address.clone(),
        found: Some(found),
        imap: ServerFields::of(imap),
        smtp: ServerFields::of(smtp),
        sender_name: account.sender_name.clone(),
        ..Mail::default()
    };
    state.provider = Provider::Mail;
    state.torromail.failed = None;
    go(state, Step::MailPassword);
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
                Step::AiChoose => state.step = Step::Provider,
                Step::AiKey | Step::AiServer => {
                    state.step = Step::AiChoose;
                    state.ai.key = Secret::default();
                }
                _ => {}
            }
            Action::None
        }
        // While a TorroMail account is taken over, the catalog waits for it.
        Message::ChooseProvider(provider) => {
            if state.step == Step::Provider && provider.is_enabled() && state.torromail.importing.is_none() {
                state.provider = provider;
                state.step = provider.first_step();
                // The servers on this computer are looked for each time the AI providers open.
                if provider == Provider::Ai {
                    state.ai.searching = true;
                    state.ai.found = None;
                    return Action::Send(Request::FindAiServers);
                }
            }
            Action::None
        }
        // Connected from the row itself, which says "Connecting…" meanwhile; no key for a local server.
        Message::ConnectLocalAi(provider, server) if state.step == Step::AiChoose && state.ai.connecting.is_none() => {
            state.ai.connecting = Some(server.clone());
            state.error = None;
            Action::Send(Request::ConnectAi { provider, server: Some(server), key: Secret::default() })
        }
        Message::ConnectLocalAi(..) => Action::None,
        Message::ChooseAi(provider) if state.step == Step::AiChoose => {
            state.step = match provider {
                AiProvider::OpenRouter => Step::AiKey,
                _ => Step::AiServer,
            };
            state.error = None;
            Action::None
        }
        Message::ChooseAi(_) => Action::None,
        Message::AiServerChanged(server) => {
            state.ai.server = server;
            Action::None
        }
        Message::AiKeyChanged(key) => {
            state.ai.key = key;
            Action::None
        }
        Message::OpenAiKeyPage => Action::OpenUrl(OPENROUTER_KEYS_URL.to_string()),
        Message::AddressChanged(address) if state.step == Step::MailAddress => {
            // What was found belonged to the old address.
            state.mail = Mail { address, ..Mail::default() };
            state.error = None;
            Action::None
        }
        Message::ServerHostChanged(server, host) => {
            state.mail.server_mut(server).host = host;
            Action::None
        }
        Message::ServerPortChanged(server, port) => {
            state.mail.server_mut(server).port = port;
            Action::None
        }
        Message::ServerSecurityPicked(server, security) => {
            state.mail.server_mut(server).set_security(server, security);
            Action::None
        }
        Message::CaldavChanged(address) => {
            state.mail.caldav = address;
            Action::None
        }
        Message::LoginChanged(login) => {
            state.mail.login = login;
            Action::None
        }
        Message::SenderNameChanged(name) => {
            state.mail.sender_name = name;
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
            if service.is_available(&state.mail) && state.step == Step::MailServices {
                let mut chosen = state.mail.services();
                match chosen.iter().position(|ticked| *ticked == service) {
                    Some(at) => drop(chosen.remove(at)),
                    None => chosen.push(service),
                }
                state.mail.chosen = Some(chosen);
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
            Step::AiKey if ai_can_connect(state) => connect_ai(state, AiProvider::OpenRouter, None),
            Step::AiServer if ai_can_connect(state) => {
                let server = state.ai.server.trim().to_string();
                connect_ai(state, AiProvider::OpenAiCompatible, Some(server))
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
        Message::UseTorroMail(id) if state.step == Step::Provider && state.torromail.can_use(&id) => {
            state.torromail.importing = Some(id.clone());
            state.torromail.failed = None;
            state.error = None;
            Action::Send(Request::ImportTorroMailAccount { id })
        }
        Message::EnterTorroMailPassword(id) if state.step == Step::Provider && state.torromail.importing.is_none() => {
            if let Some(account) = state.torromail.accounts.iter().find(|account| account.id == id).cloned() {
                fill_from_torromail(state, &account);
            }
            Action::None
        }
        Message::UseTorroMail(_) | Message::EnterTorroMailPassword(_) => Action::None,
        Message::UseGithubCli | Message::PasteToken | Message::AddressChanged(_) => Action::None,
    }
}

/// Moves to `step` of the mail path. "Advanced" starts closed, except where the person has to type
/// the servers (nothing found, or half of them).
fn go(state: &mut State, step: Step) {
    state.step = step;
    state.error = None;
    state.mail.advanced = step == Step::MailAddress && state.mail.needs_input();
}

/// Whether Continue does something on `step` of the mail path.
fn mail_can_continue(state: &State, step: Step) -> bool {
    let mail = &state.mail;
    match step {
        Step::MailAddress => match &mail.found {
            None => !mail.address.trim().is_empty(),
            // A provider that does not work stops here; else the servers must be there (found or
            // typed).
            Some(found) => !found.is_unsupported() && mail.is_ready(),
        },
        Step::MailPassword => !mail.password.is_blank(),
        Step::MailServices => !mail.services().is_empty(),
        _ => false,
    }
}

/// Continue on a step of the mail path: search first, then walk [`MAIL_STEPS`]; at the end send
/// `ConnectMail`, or `ConnectCalDav` when only calendars and tasks are wanted.
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
    let Some(request) = connect_request(&mut state.mail) else { return Action::None };
    state.step = Step::MailConnecting;
    state.error = None;
    Action::Send(request)
}

/// The request for what the last step chose. The password is taken, so the dialog keeps no copy; a
/// refusal asks for it again.
fn connect_request(mail: &mut Mail) -> Option<Request> {
    let services = mail.services();
    let calendars = services.contains(&Service::Calendars);
    let tasks = services.contains(&Service::Tasks);
    let address = mail.address.trim().to_string();
    let login_name = mail.login_name();
    if services.contains(&Service::Mail) {
        let (imap, smtp) = mail.mail_servers()?;
        return Some(Request::ConnectMail {
            address,
            sender_name: mail.sender_name.trim().to_string(),
            imap,
            smtp,
            login_name,
            password: std::mem::take(&mut mail.password),
            calendars,
            tasks,
            // The CalDAV address only goes with the services that use it.
            caldav: mail.caldav_address().filter(|_| calendars || tasks).map(str::to_string),
        });
    }
    Some(Request::ConnectCalDav {
        address,
        server: mail.server_override(),
        login_name,
        password: std::mem::take(&mut mail.password),
        calendars,
        tasks,
    })
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
        typo::text(typo::title(), tr("Add account")),
        typo::text(typo::body(), subtitle(state)).style(theme::text_2)
    ]
    .spacing(space::xs());
    let body = match state.step {
        Step::Provider => provider_step(state),
        Step::Server => server_step(state),
        Step::SigningIn => signing_in_step(data),
        Step::Github => github_step(),
        Step::Token => token_step(state),
        Step::Checking => typo::text(typo::body(), tr("Checking the connection…")).style(theme::text_2).into(),
        Step::MailAddress | Step::MailSearching => mail_address_step(state),
        Step::MailPassword => mail_password_step(state),
        Step::MailServices => mail_services_step(state),
        Step::MailConnecting => typo::text(typo::body(), tr("Signing in…")).style(theme::text_2).into(),
        Step::AiChoose => ai_step(state),
        Step::AiKey => ai_key_step(state),
        Step::AiServer => ai_server_step(state),
        Step::AiConnecting => typo::text(typo::body(), tr("Checking the connection…")).style(theme::text_2).into(),
        Step::Done if state.provider == Provider::Mail => mail_done_step(state),
        Step::Done => typo::text(typo::body(), done_text(state)).into(),
    };
    // Head and foot stay in the window; the steps scroll between them when it is low (D.12).
    dialog::frame(header, body, footer(state), space::xl())
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
        (Step::AiChoose, _) => tr("Choose where your AI models run."),
        (Step::AiKey, _) => tr("Paste an API key from OpenRouter."),
        (Step::AiServer, _) => tr("Enter the address of your AI server."),
        (Step::AiConnecting, _) => tr("Checking the connection with the AI server."),
        (Step::Done, Provider::Ai) => tr("Your AI account is connected."),
        (Step::Done, Provider::GitHub) => tr("GitHub is connected."),
        (Step::Done, Provider::Mail) => tr("Your account is connected."),
        (Step::Done, _) => tr("Nextcloud is connected."),
    }
}

fn done_text(state: &State) -> String {
    match (state.provider, &state.login) {
        (Provider::GitHub, Some(login)) => trf("Signed in to GitHub as {login}.", &[("login", login)]),
        (Provider::Ai, _) => match &state.ai.connected {
            Some(name) => trf("{name} is ready to use.", &[("name", name)]),
            None => tr("Your AI account is ready to use.").to_string(),
        },
        _ => tr("Your calendars and task lists are ready.").to_string(),
    }
}

/// The catalog: TorroMail's accounts first when it has some ([`offer_group`]), then a group with
/// one row per provider, the disabled ones marked "Coming soon". While a TorroMail account is
/// taken over, the providers wait.
fn provider_step(state: &State) -> Element<'_, Message> {
    let open = state.torromail.importing.is_none();
    let rows = vec![
        provider_row(Provider::Nextcloud, "Nextcloud", tr("Calendars and tasks"), open),
        provider_row(Provider::Mail, tr("Mail account"), tr("Mail, calendars and tasks"), open),
        provider_row(Provider::GitHub, "GitHub", tr("Issues and projects"), open),
        provider_row(Provider::Slack, "Slack", tr("Mentions and messages"), open),
        provider_row(Provider::Ai, tr("AI providers"), tr("Summaries and agents"), open),
    ];
    let services = widgets::group(tr("Services"), rows, None);
    match offer_group(&state.torromail) {
        Some(offer) => column![offer, services].spacing(layout::section_gap()).into(),
        None => services,
    }
}

/// A provider: name and what it brings; the available one is a row to press with a chevron (drawn
/// disabled unless `open`), the others are plain rows with a tag.
fn provider_row(
    provider: Provider,
    name: &'static str,
    summary: &'static str,
    open: bool,
) -> Element<'static, Message> {
    if provider.is_enabled() {
        let row = setting_row(None, name, Some(summary.to_string()), chevron());
        pressable(row, open.then_some(Message::ChooseProvider(provider)))
    } else {
        coming_soon_row(None, name, summary, tr("Coming soon"))
    }
}

/// "Use accounts from TorroMail?" (task M7.4): one row per account of the separate TorroMail app;
/// `None` when it has none, so the catalog is as before.
fn offer_group(offer: &Offer) -> Option<Element<'_, Message>> {
    if offer.accounts.is_empty() {
        return None;
    }
    let rows = offer.accounts.iter().map(|account| offer_row(offer, account)).collect();
    let footer = tr("Copies the mail servers, user name and password. TorroMail keeps its accounts.");
    Some(widgets::group(tr("Use accounts from TorroMail?"), rows, Some(footer.to_string())))
}

/// One TorroMail account: its address and servers, and what can be done with it – "Use", taking
/// over ("Connecting…"), "Connected", greyed out with the reason it cannot be taken, or the reason
/// a take-over failed with "Enter password".
fn offer_row<'a>(offer: &'a Offer, account: &'a TorroMailAccount) -> Element<'a, Message> {
    if let Some(why) = &account.unsupported {
        return unavailable_row(&account.address, why, tr("Not supported"));
    }
    let open = offer.importing.is_none();
    if let Some((_, message)) = offer.failed.as_ref().filter(|(id, _)| *id == account.id) {
        let error = |t: &Theme| text::Style { color: Some(palette(t).error_text) };
        let lines = column![
            one_line(typo::text(typo::body(), account.address.as_str())),
            typo::text(typo::meta(), message.as_str()).style(error),
        ];
        let way_on = widgets::text_button(
            None,
            tr("Enter password"),
            style::button::secondary,
            open.then(|| Message::EnterTorroMailPassword(account.id.clone())),
        );
        return wrapping_row(lines, way_on);
    }
    let control: Element<'a, Message> = if account.connected {
        widgets::pill(None, tr("Connected"))
    } else if offer.importing.as_deref() == Some(account.id.as_str()) {
        typo::text(typo::meta(), tr("Connecting…")).style(theme::text_2).into()
    } else {
        let press = offer.can_use(&account.id).then(|| Message::UseTorroMail(account.id.clone()));
        widgets::text_button(None, tr("Use"), style::button::secondary, press)
    };
    setting_row(None, account.address.as_str(), Some(servers_of(account)), control)
}

/// "imap.mailbox.org · smtp.mailbox.org", or the one host when both servers share it.
fn servers_of(account: &TorroMailAccount) -> String {
    let hosts: Vec<&str> =
        [&account.imap, &account.smtp].into_iter().flatten().map(|server| server.host.as_str()).collect();
    match hosts.as_slice() {
        [imap, smtp] if imap.eq_ignore_ascii_case(smtp) => imap.to_string(),
        hosts => hosts.join(" · "),
    }
}

/// A row whose second line wraps (a reason or an error, in any language), with `control` at its end.
fn wrapping_row<'a>(lines: iced::widget::Column<'a, Message>, control: Element<'a, Message>) -> Element<'a, Message> {
    container(row![lines.width(Fill), control].spacing(space::l()).align_y(Alignment::Center))
        .width(Fill)
        .padding([space::m(), space::l()])
        .into()
}

/// A TorroMail account the suite cannot take: address, the reason below it (wrapping) and `tag`,
/// all at the disabled opacity like [`coming_soon_row`].
fn unavailable_row<'a>(name: &'a str, reason: &'a str, tag: &'static str) -> Element<'a, Message> {
    let faded = |colour: fn(&Palette) -> Color| {
        move |t: &Theme| text::Style { color: Some(colour(&palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let lines = column![
        one_line(typo::text(typo::body(), name).style(faded(|p| p.text))),
        typo::text(typo::meta(), reason).style(faded(|p| p.text_2)),
    ];
    wrapping_row(lines, widgets::pill_disabled(None, tag))
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
        move |t: &Theme| text::Style { color: Some(colour(&palette(t)).scale_alpha(style::DISABLED_ALPHA)) }
    };
    let lines = column![
        one_line(typo::text(typo::body(), name).style(faded(|p| p.text))),
        one_line(typo::text(typo::meta(), summary).style(faded(|p| p.text_2))),
    ];
    let text = row![].push(mark).push(lines.width(Fill)).spacing(space::m()).align_y(Alignment::Center);
    row_frame(
        row![text.width(Fill), widgets::pill_disabled(None, tag)].spacing(space::l()).align_y(Alignment::Center),
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
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
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
            typo::body(),
            tr("Reading private repositories and closing issues needs the repo scope; GitHub's page has it selected.")
        )
        .style(theme::text_2),
        row![create],
        input,
    ]
    .spacing(space::m())
    .into()
}

/// The explanation, the waiting line, the login link to copy and a button to open it again.
fn signing_in_step(data: &Data) -> Element<'_, Message> {
    let login = login_url(data);
    let mut body = column![
        typo::text(
            typo::body(),
            tr("Nextcloud handles the sign-in. You sign in on its own page; Torro Workplace never sees your password.")
        ),
        typo::text(typo::body(), tr("Waiting for the browser …")).style(theme::text_2),
    ]
    .spacing(space::m());
    if let Some(url) = login {
        // `text_input` without an edit handler cannot take focus, so no copying. Editing is
        // allowed but ignored: the field always shows `url`, so the text stays unchanged.
        body = body
            .push(typo::text(typo::body(), tr("If the browser does not open, copy this link:")).style(theme::text_2))
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

// ----- AI providers (task KI.1a) --------------------------------------------------------------------

/// The AI providers, Open First: the servers on this computer, each found one with "Connect", then
/// OpenRouter and any other server, each a row that opens its form.
fn ai_step(state: &State) -> Element<'_, Message> {
    let local = LOCAL_AI.into_iter().map(|provider| local_ai_row(&state.ai, provider)).collect();
    let footer = tr("Models on this computer run here. Nothing you ask them leaves it.");
    let way = |provider, name: &'static str, summary: &'static str| {
        pressable(setting_row(None, name, Some(summary.to_string()), chevron()), Some(Message::ChooseAi(provider)))
    };
    let others = vec![
        way(AiProvider::OpenRouter, "OpenRouter", tr("Open models from many makers, with one API key")),
        way(
            AiProvider::OpenAiCompatible,
            tr("Other OpenAI-compatible server"),
            tr("vLLM, llama.cpp or a server of your own"),
        ),
    ];
    column![
        widgets::group(tr("On this computer"), local, Some(footer.to_string())),
        widgets::group(tr("More providers"), others, None),
    ]
    .spacing(layout::section_gap())
    .into()
}

/// A server on this computer: while looking, "Looking on this computer…"; found, its address and
/// models with "Connect" (or "Connected" when an account is there already); else "Not running on
/// this computer". Never colour alone: the dot goes with the words.
fn local_ai_row(ai: &AiSetup, provider: AiProvider) -> Element<'_, Message> {
    let Some(found) = &ai.found else {
        return setting_row(None, provider.name(), Some(tr("Looking on this computer…").to_string()), Space::new());
    };
    let Some(server) = found.iter().find(|server| server.provider == provider) else {
        let dot = widgets::status_dot(widgets::Status::Inactive);
        return setting_row(
            Some(dot),
            provider.name(),
            Some(tr("Not running on this computer").to_string()),
            Space::new(),
        );
    };
    let control: Element<'_, Message> = if server.account.is_some() {
        widgets::pill(None, tr("Connected"))
    } else if ai.connecting.as_deref() == Some(server.url.as_str()) {
        typo::text(typo::meta(), tr("Connecting…")).style(theme::text_2).into()
    } else {
        let press = ai.connecting.is_none().then(|| Message::ConnectLocalAi(provider, server.url.clone()));
        widgets::text_button(None, tr("Connect"), style::button::secondary, press)
    };
    let dot = widgets::status_dot(widgets::Status::Ok);
    setting_row(Some(dot), provider.name(), Some(local_ai_detail(server)), control)
}

/// "localhost:11434 · 3 models": where it runs and what it has.
fn local_ai_detail(server: &AiServer) -> String {
    let rest = server.url.split_once("://").map_or(server.url.as_str(), |(_, rest)| rest);
    let host = rest.split('/').next().unwrap_or(rest);
    match server.models {
        1 => trf("{host} · 1 model", &[("host", &host)]),
        n => trf("{host} · {n} models", &[("host", &host), ("n", &n)]),
    }
}

/// OpenRouter: what it is, the way to its key page and the masked key field.
fn ai_key_step(state: &State) -> Element<'_, Message> {
    let create = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Create a key on OpenRouter"),
        style::button::secondary,
        Some(Message::OpenAiKeyPage),
    );
    let input = field("sk-or-v1-…", state.ai.key.expose())
        .secure(true)
        .on_input(|key| Message::AiKeyChanged(Secret::new(key)))
        .on_submit_maybe(ai_can_connect(state).then_some(Message::Continue));
    column![
        typo::text(
            typo::body(),
            tr("OpenRouter offers open models such as Qwen, Llama and Mistral through one API key. Torro Workplace lists the models with open weights.")
        )
        .style(theme::text_2),
        row![create],
        field_group(tr("API key"), input, Some(tr("The key stays in the system keychain. Agents never see it."))),
    ]
    .spacing(space::m())
    .into()
}

/// Another OpenAI-compatible server: its address and, when it asks for one, a key.
fn ai_server_step(state: &State) -> Element<'_, Message> {
    let submit = ai_can_connect(state).then_some(Message::Continue);
    let address = field("http://localhost:8000/v1", &state.ai.server)
        .on_input(Message::AiServerChanged)
        .on_submit_maybe(submit.clone());
    let key = field("", state.ai.key.expose())
        .secure(true)
        .on_input(|key| Message::AiKeyChanged(Secret::new(key)))
        .on_submit_maybe(submit);
    column![
        field_group(tr("Address"), address, Some(tr("The address of its OpenAI-compatible API."))),
        field_group(
            tr("API key"),
            key,
            Some(tr("Only if the server asks for one. It stays in the system keychain; agents never see it.")),
        ),
    ]
    .spacing(layout::section_gap())
    .into()
}

// ----- The mail account path (tasks M7.2, M5.1) ------------------------------------------------------

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
    icon::icon(which, icon::SIZE).style(move |t: &Theme, _| svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// A row of a group whose text wraps: a provider's reason or a translation can be long, and the
/// dialog is only as wide as the window. `title` in `BODY`, `text` below it in `META`/`text_2`.
fn note_row<'a>(mark: Element<'a, Message>, title: String, text: String) -> Element<'a, Message> {
    let lines = column![typo::text(typo::body(), title), typo::text(typo::meta(), text).style(theme::text_2)];
    container(row![mark, lines.width(Fill)].spacing(space::m()).align_y(Alignment::Center))
        .width(Fill)
        .padding([space::m(), space::l()])
        .into()
}

/// "Advanced" as a button with a chevron, and `content` below it while it is open.
fn advanced<'a>(open: bool, content: Element<'a, Message>) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let toggle =
        widgets::text_button(Some(chevron), tr("Advanced"), style::button::ghost, Some(Message::ToggleAdvanced));
    // Moved left by its own padding, so the chevron stands on the content axis of the fields.
    let on_axis = row![Space::new(), toggle].spacing(-style::button_padding().left);
    let mut block = column![on_axis].spacing(space::s());
    if open {
        block = block.push(content);
    }
    block.into()
}

/// What "Advanced" holds on the address step: both mail servers and the CalDAV address, filled in
/// with what was found.
fn servers_block(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let submit = mail_can_continue(state, state.step).then_some(Message::Continue);
    let caldav =
        field("https://dav.example.org", &mail.caldav).on_input(Message::CaldavChanged).on_submit_maybe(submit.clone());
    let server = |which: Server, fields, titles| {
        server_group(
            fields,
            titles,
            move |host| Message::ServerHostChanged(which, host),
            move |port| Message::ServerPortChanged(which, port),
            move |security| Message::ServerSecurityPicked(which, security),
            submit.clone(),
        )
    };
    column![
        server(Server::Imap, &mail.imap, imap_titles()),
        server(Server::Smtp, &mail.smtp, smtp_titles()),
        field_group(
            tr("Calendars and tasks (CalDAV)"),
            caldav,
            Some(tr("Leave empty to use this account for mail only.")),
        ),
    ]
    .spacing(layout::section_gap())
    .into()
}

/// The e-mail address and, below it, what the search found: the provider's name and what it has,
/// a note where mail needs an app password, "Advanced" with the servers – or the reason a
/// provider does not work and nothing more, or the hint to type the servers when nothing
/// answered.
fn mail_address_step(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let searching = state.step == Step::MailSearching;
    let input = field("anna@example.org", &mail.address)
        .on_input_maybe((!searching).then_some(Message::AddressChanged))
        .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
    let hint = tr("Only the part after @ is used to look for your provider.");
    let mut page = column![field_group(tr("Email address"), input, Some(hint))].spacing(layout::section_gap());
    if searching {
        page = page.push(typo::text(typo::body(), tr("Looking for your provider…")).style(theme::text_2));
    }
    let Some(found) = &mail.found else { return page.into() };
    if found.is_unsupported() {
        let reason = found.note.clone().unwrap_or_default();
        let row = note_row(status_icon(Icon::CircleAlert, |p| p.attention), found.name.clone(), reason);
        return page.push(widgets::group(tr("Provider"), vec![row], None)).into();
    }
    if found.is_nothing() {
        let text = trf(
            "Nothing found for {domain}. If you know the servers of your provider, enter them under Advanced.",
            &[("domain", &domain_of(&mail.address))],
        );
        page = page.push(typo::text(typo::body(), text).style(theme::text_2));
    } else {
        let name = setting_row(
            Some(widgets::status_dot(widgets::Status::Ok)),
            found.name.as_str(),
            Some(found.summary().to_string()),
            Space::new(),
        );
        let mut rows = vec![name];
        // A provider that works only with an app password says so next to its name.
        if let Some(note) = &found.note {
            rows.push(note_row(status_icon(Icon::CircleAlert, |p| p.attention), tr("Note").to_string(), note.clone()));
        }
        page = page.push(widgets::group(tr("Provider"), rows, None));
        if mail.servers() == Servers::Incomplete {
            let text = tr("Mail needs an incoming and an outgoing server. Enter the missing one under Advanced.");
            page = page.push(typo::text(typo::body(), text).style(theme::text_2));
        }
    }
    page.push(advanced(mail.advanced, servers_block(state))).into()
}

/// The provider found, what to enter, and the masked password field. A provider that asks for an
/// app password says so and links to its help; a refused password stands under the field, naming
/// the server that refused it.
fn mail_password_step(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let provider = mail.provider_name();
    // Who this is for, in one line of its own: the step is already tall with the hint and the field.
    let lines = column![
        typo::text(typo::body(), provider.clone()),
        typo::text(typo::meta(), mail.address.trim().to_string()).style(theme::text_2)
    ];
    let context = row![status_icon(Icon::CircleCheck, |p| p.ok), lines].spacing(space::m()).align_y(Alignment::Center);
    let mut page = column![context].spacing(layout::section_gap());
    let help = app_password_help(&provider);
    let mut hint = column![].spacing(space::s());
    if help.is_some() || mail.found.as_ref().is_some_and(|found| found.app_password) {
        hint = hint.push(typo::text(typo::body_strong(), tr("Use an app password"))).push(
            typo::text(
                typo::body(),
                trf(
                    "{provider} does not accept your usual password here. Create an app password in your account settings.",
                    &[("provider", &provider)],
                ),
            )
            .style(theme::text_2),
        );
        if help.is_some() {
            hint = hint.push(row![widgets::text_button(
                Some(Icon::ArrowUpRight),
                tr("Create an app password"),
                style::button::secondary,
                Some(Message::OpenPasswordHelp),
            )]);
        }
    } else {
        hint = hint.push(
            typo::text(typo::body(), tr("Use your mailbox password, or an app password if your provider offers them."))
                .style(theme::text_2),
        );
    }
    page = page.push(hint);
    let input = field("", mail.password.expose())
        .secure(true)
        .on_input(|password| Message::PasswordChanged(Secret::new(password)))
        .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
    let mut password = column![field_group(tr("Password"), input, None)].spacing(space::s());
    password = match &mail.refused {
        Some(refused) => password.push(
            typo::text(typo::meta(), refused.as_str())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        ),
        None => password.push(
            typo::text(typo::meta(), tr("The password stays in the system keychain. Agents never see it."))
                .style(theme::text_2),
        ),
    };
    page = page.push(password);
    let suggested = mail.login_name();
    let login = field_group(
        tr("User name"),
        field(&suggested, &mail.login).on_input(Message::LoginChanged),
        Some(tr("Leave empty to use the name shown.")),
    );
    page.push(advanced(mail.advanced, login)).into()
}

/// The services as check boxes, the ones the servers have preselected; the whole row ticks it.
/// A service the search did not find (and nobody typed) is greyed out with "Not found". With Mail
/// ticked, the sender name below.
fn mail_services_step(state: &State) -> Element<'_, Message> {
    let mail = &state.mail;
    let ticked = mail.services();
    let rows = Service::ALL
        .into_iter()
        .map(|service| {
            if !service.is_available(mail) {
                // A box without a handler is drawn disabled.
                let off: Element<'_, Message> = checkbox(false).size(style::CHECK_SIZE).style(check_style).into();
                return coming_soon_row(Some(off), service.name(), service.summary(), tr("Not found"));
            }
            let check = checkbox(ticked.contains(&service))
                .size(style::CHECK_SIZE)
                .style(check_style)
                .on_toggle(move |_| Message::ToggleService(service));
            let row =
                setting_row(Some(check.into()), service.name(), Some(service.summary().to_string()), Space::new());
            pressable(row, Some(Message::ToggleService(service)))
        })
        .collect();
    let footer = tr("You can show or hide single calendars and lists on the account's page later.");
    let mut page =
        column![widgets::group(tr("Services"), rows, Some(footer.to_string()))].spacing(layout::section_gap());
    if ticked.contains(&Service::Mail) {
        let name = field("Anna Example", &mail.sender_name)
            .on_input(Message::SenderNameChanged)
            .on_submit_maybe(mail_can_continue(state, state.step).then_some(Message::Continue));
        page = page.push(field_group(
            tr("Sender name"),
            name,
            Some(tr("Shown next to your address in the mail you send. Optional.")),
        ));
    }
    page.into()
}

/// `text_2` for the box, not `text`: a black box would be the heaviest mark of the light dialog.
fn check_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    style::checkbox(palette(theme).text_2)(theme, status)
}

/// Who is signed in and, per chosen service, whether the server has it ([`done_lines`]).
fn mail_done_step(state: &State) -> Element<'_, Message> {
    let Some(connected) = &state.mail.connected else {
        return typo::text(typo::body(), tr("Your calendars and task lists are ready.")).into();
    };
    let mut body =
        column![typo::text(typo::body(), trf("Signed in as {account}.", &[("account", &connected.account)]))]
            .spacing(space::m());
    for (found, text) in done_lines(connected, &state.mail.services()) {
        let which = if found {
            status_icon(Icon::CircleCheck, |p| p.ok)
        } else {
            status_icon(Icon::CircleAlert, |p| p.attention)
        };
        body = body.push(row![which, typo::text(typo::body(), text)].spacing(space::s()).align_y(Alignment::Center));
    }
    body.into()
}

/// One line per service that was chosen: whether the server has it, and the sentence for that.
/// Never colour alone: the sentence says it, the icon repeats it.
fn done_lines(connected: &Connected, chosen: &[Service]) -> Vec<(bool, &'static str)> {
    chosen
        .iter()
        .map(|service| {
            let found = match service {
                Service::Mail => true,
                Service::Calendars => connected.calendars,
                Service::Tasks => connected.tasks,
            };
            (found, service.found_text(found))
        })
        .collect()
}

/// Error text left, "Back" where there is a step to go back to, "Cancel" and the step's primary
/// button right.
fn footer(state: &State) -> Element<'_, Message> {
    let error: Element<'_, Message> = match &state.error {
        Some(error) => typo::text(typo::body(), error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        None => blank::horizontal().into(),
    };
    let back = matches!(
        state.step,
        Step::Github
            | Step::Token
            | Step::MailAddress
            | Step::MailPassword
            | Step::MailServices
            | Step::AiChoose
            | Step::AiKey
            | Step::AiServer
    )
    .then(|| widgets::text_button(None, tr("Back"), style::button::secondary, Some(Message::Back)));
    // A TorroMail account taken over on the catalog is done, not cancelled.
    let taken_over = state.step == Step::Provider && state.torromail.imported;
    let cancel =
        (!taken_over).then(|| widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close)));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match state.step {
        Step::Provider if taken_over => primary(tr("Done"), Some(Message::Close)),
        Step::Provider
        | Step::SigningIn
        | Step::Github
        | Step::Checking
        | Step::MailSearching
        | Step::MailConnecting
        | Step::AiChoose
        | Step::AiConnecting => None,
        Step::AiKey | Step::AiServer => primary(tr("Connect"), ai_can_connect(state).then_some(Message::Continue)),
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
    row![error].push(back).push(cancel).push(next).spacing(space::s()).align_y(Alignment::Center).into()
}

// ----- Demo states (`--demo --open wizard-mail-…`) --------------------------------------------

/// The states of the mail account path that `torroworkplace ui --demo --open …` starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demo {
    /// The empty address step.
    Address,
    /// mailbox.org found (mail, calendars, tasks), "Advanced" closed.
    Found,
    /// The same with "Advanced" open: both servers and the CalDAV address as found.
    Advanced,
    /// Outlook.com: mail is not supported, the reason and nothing more.
    Unsupported,
    /// Gmail: servers found, with the note that an app password works.
    Gmail,
    /// A domain nothing answers for: "Advanced" open with empty servers.
    NotFound,
    /// iCloud: the password step with "Use an app password".
    Password,
    /// The IMAP server refused the password: back at the password step with the text under the field.
    Refused,
    /// The same for the SMTP server.
    RefusedSmtp,
    /// The services step.
    Services,
    /// The services step of a mail-only provider: calendars and tasks "Not found".
    MailOnly,
    /// Connected: what the servers have.
    Done,
    /// The catalog after "Use" on TorroMail's Fastmail account, whose password could not be read
    /// (task M7.4): the reason and "Enter password".
    TorroMailFailed,
    /// "Enter password" pressed there: the password step, filled with TorroMail's servers.
    TorroMailPassword,
    /// The catalog after TorroMail's Posteo account was taken over: "Connected" and "Done".
    TorroMailDone,
    /// The AI providers (task KI.1a): Ollama found on this computer, LM Studio not running.
    AiProviders,
    /// OpenRouter's key field.
    AiOpenRouter,
    /// OpenRouter refused the key: the key field again, the reason in the footer.
    AiRefused,
    /// Another server's address and key, an address typed.
    AiServer,
    /// The Ollama found on this computer connected.
    AiDone,
}

/// The messages that lead to `demo` the way a person would get there. The demo's answers to the
/// requests come from `launch::demo_answer`; the passwords "wrong" (IMAP) and "wrong-smtp" are the
/// ones it refuses.
pub fn demo_messages(demo: Demo) -> Vec<Message> {
    // TorroMail's accounts are on the catalog once the dialog is open (`launch::demo_torromail`).
    match demo {
        Demo::TorroMailFailed => return vec![Message::UseTorroMail("tm-fastmail".into())],
        Demo::TorroMailPassword => {
            let id = "tm-fastmail".to_string();
            return vec![Message::UseTorroMail(id.clone()), Message::EnterTorroMailPassword(id)];
        }
        Demo::TorroMailDone => return vec![Message::UseTorroMail("tm-posteo".into())],
        _ => {}
    }
    // The AI providers search this computer when they open (`launch::demo_answer` finds Ollama).
    let ai = |more: Vec<Message>| [vec![Message::ChooseProvider(Provider::Ai)], more].concat();
    let openrouter = Message::ChooseAi(AiProvider::OpenRouter);
    match demo {
        Demo::AiProviders => return ai(Vec::new()),
        Demo::AiOpenRouter => return ai(vec![openrouter]),
        // "wrong" is the key the demo refuses.
        Demo::AiRefused => return ai(vec![openrouter, Message::AiKeyChanged(Secret::new("wrong")), Message::Continue]),
        Demo::AiServer => {
            let address = Message::AiServerChanged("http://gpu.local:8000/v1".into());
            return ai(vec![Message::ChooseAi(AiProvider::OpenAiCompatible), address]);
        }
        Demo::AiDone => {
            let ollama = AiProvider::Ollama.default_url().unwrap_or_default().to_string();
            return ai(vec![Message::ConnectLocalAi(AiProvider::Ollama, ollama)]);
        }
        _ => {}
    }
    let address = match demo {
        Demo::TorroMailFailed | Demo::TorroMailPassword | Demo::TorroMailDone => None,
        Demo::AiProviders | Demo::AiOpenRouter | Demo::AiRefused | Demo::AiServer | Demo::AiDone => None,
        Demo::Address => None,
        Demo::Unsupported => Some("anna@outlook.com"),
        Demo::Gmail => Some("anna@gmail.com"),
        Demo::NotFound => Some("anna@example.org"),
        Demo::Password => Some("anna@icloud.com"),
        Demo::MailOnly => Some("anna@work.example.net"),
        Demo::Found | Demo::Advanced | Demo::Refused | Demo::RefusedSmtp | Demo::Services | Demo::Done => {
            Some("anna@mailbox.org")
        }
    };
    let mut steps = vec![Message::ChooseProvider(Provider::Mail)];
    if let Some(address) = address {
        // Search, then (found) on to the password.
        steps.extend([Message::AddressChanged(address.to_string()), Message::Continue]);
    }
    let password = |text: &str| Message::PasswordChanged(Secret::new(text));
    match demo {
        Demo::Password => steps.push(Message::Continue),
        Demo::Advanced => steps.push(Message::ToggleAdvanced),
        Demo::Refused | Demo::RefusedSmtp => {
            let wrong = if demo == Demo::Refused { "wrong" } else { "wrong-smtp" };
            steps.extend([Message::Continue, password(wrong), Message::Continue, Message::Continue]);
        }
        Demo::Services | Demo::MailOnly | Demo::Done => {
            steps.extend([Message::Continue, password("demo-password"), Message::Continue]);
            if demo == Demo::Done {
                steps.push(Message::Continue);
            }
        }
        Demo::Address | Demo::Found | Demo::Unsupported | Demo::Gmail | Demo::NotFound => {}
        // Returned above.
        Demo::TorroMailFailed | Demo::TorroMailPassword | Demo::TorroMailDone => {}
        Demo::AiProviders | Demo::AiOpenRouter | Demo::AiRefused | Demo::AiServer | Demo::AiDone => {}
    }
    steps
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use torro_mail_imap::discover::MailUnsupported;

    use super::*;

    fn account(id: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: id.to_string(),
            kind: "nextcloud".to_string(),
            label: id.to_string(),
            state,
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: None,
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
    fn four_providers_open_the_catalog() {
        assert_eq!(Provider::ALL.iter().filter(|provider| provider.is_enabled()).count(), 4);
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
        let mut state = at_catalog();
        update(&mut state, Message::ChooseProvider(Provider::Slack), &Data::default());
        assert_eq!(state.step, Step::Provider, "Slack is disabled");
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

    // ----- The mail account path (tasks M7.2, M5.1) ----------------------------------------------

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

    fn send(state: &mut State, message: Message) -> Action {
        update(state, message, &Data::default())
    }

    /// Types the two servers under "Advanced" the way a person does.
    fn type_servers(state: &mut State) {
        send(state, Message::ServerHostChanged(Server::Imap, "imap.example.org".into()));
        send(state, Message::ServerHostChanged(Server::Smtp, " smtp.example.org ".into()));
    }

    /// From the address step through the password to the services step.
    fn to_services(state: &mut State) {
        send(state, Message::Continue);
        send(state, Message::PasswordChanged(Secret::new("pw")));
        send(state, Message::Continue);
        assert_eq!(state.step, Step::MailServices);
    }

    fn found_of(demo: Demo) -> Found {
        at_mail(demo).mail.found.expect("the search answered")
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
    fn the_catalog_offers_the_mail_account() {
        let mut state = State::new();
        drop(view(&state, &Data::default()));
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

        let server = |host: &str, port, security| Endpoint { host: host.into(), port, security };
        let found = Response::Discovery {
            provider: "mailbox.org".into(),
            urls: vec!["https://dav.mailbox.org/".into(), "https://other.mailbox.org/".into()],
            unsupported: None,
            imap: Some(server("imap.mailbox.org", 993, Security::Tls)),
            smtp: Some(server("smtp.mailbox.org", 587, Security::StartTls)),
            login_hint: Some("anna".into()),
            app_password: false,
            mail_unsupported: None,
        };
        assert!(answered(&mut state, &Ok(found)));
        assert_eq!(state.step, Step::MailAddress);
        let mail = &state.mail;
        let found = mail.found.as_ref().expect("found");
        assert_eq!((found.name.as_str(), found.caldav.as_deref()), ("mailbox.org", Some("https://dav.mailbox.org/")));
        assert_eq!(found.summary(), tr("Mail, calendars and tasks found"));
        assert_eq!(
            (mail.imap.host.as_str(), mail.imap.port.as_str(), mail.imap.security),
            ("imap.mailbox.org", "993", Security::Tls),
            "prefilled for Advanced"
        );
        assert_eq!((mail.smtp.port.as_str(), mail.smtp.security), ("587", Security::StartTls));
        assert_eq!(mail.caldav, "https://dav.mailbox.org/");
        assert_eq!(mail.login_name(), "anna", "the provider says which user name");
        assert!(!mail.advanced, "everything is there, the servers are only a click away");

        // The second press goes on, to the password.
        update(&mut state, Message::Continue, &Data::default());
        assert_eq!(state.step, Step::MailPassword);
    }

    #[test]
    fn editing_the_address_forgets_what_was_found() {
        let mut state = at_mail(Demo::Found);
        assert!(state.mail.found.is_some());
        type_address(&mut state, "anna@posteo.de");
        assert_eq!(
            (state.mail.found.as_ref(), state.mail.imap.host.as_str(), state.mail.caldav.as_str()),
            (None, "", "")
        );
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
        let found = state.mail.found.clone().expect("found");
        assert!(found.is_unsupported());
        assert_eq!(found.name, "Outlook.com");
        assert_eq!(found.note.as_deref(), Some(tr(MailUnsupported::Microsoft.reason())));
        assert!(!mail_can_continue(&state, Step::MailAddress));
        assert!(!state.mail.advanced, "there is nothing to type");
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None));
        assert_eq!(state.step, Step::MailAddress, "no way on");
        drop(view(&state, &Data::default()));
        // Another address searches again.
        type_address(&mut state, "anna@mailbox.org");
        assert!(mail_can_continue(&state, Step::MailAddress));
    }

    #[test]
    fn gmail_works_with_an_app_password_and_says_so() {
        let mut state = at_mail(Demo::Gmail);
        let found = state.mail.found.clone().expect("found");
        assert!(found.is_usable() && !found.is_unsupported());
        assert_eq!(
            found.note.as_deref(),
            Some(tr(MailUnsupported::Google.reason())),
            "the mail hint, not the calendar one"
        );
        assert!(found.app_password);
        assert_eq!(found.summary(), tr("Mail server found"));
        assert!(mail_can_continue(&state, Step::MailAddress));
        drop(view(&state, &Data::default()));
        update(&mut state, Message::Continue, &Data::default());
        assert_eq!(state.step, Step::MailPassword);
        match update(&mut state, Message::OpenPasswordHelp, &Data::default()) {
            Action::OpenUrl(url) => assert!(url.starts_with("https://support.google.com/")),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
    }

    #[test]
    fn nothing_found_opens_advanced_and_asks_for_the_servers() {
        let mut state = at_mail(Demo::NotFound);
        assert!(state.mail.found.as_ref().is_some_and(Found::is_nothing));
        assert!(state.mail.advanced, "the servers are the way on");
        assert_eq!((state.mail.imap.port.as_str(), state.mail.smtp.port.as_str()), ("993", "587"), "the usual ports");
        assert!(!mail_can_continue(&state, Step::MailAddress), "until they are typed");

        send(&mut state, Message::ServerHostChanged(Server::Imap, "imap.example.org".into()));
        assert_eq!(state.mail.servers(), Servers::Incomplete);
        assert!(!mail_can_continue(&state, Step::MailAddress), "half of a mail setup is not enough");
        send(&mut state, Message::ServerHostChanged(Server::Smtp, " smtp.example.org ".into()));
        assert!(mail_can_continue(&state, Step::MailAddress));
        send(&mut state, Message::ServerPortChanged(Server::Smtp, "five".into()));
        assert!(!mail_can_continue(&state, Step::MailAddress), "a port is a number");
        send(&mut state, Message::ServerPortChanged(Server::Smtp, " 2525 ".into()));
        assert!(mail_can_continue(&state, Step::MailAddress));
        send(&mut state, Message::Continue);
        assert_eq!(state.step, Step::MailPassword);
        assert!(!state.mail.advanced, "closed again on the next step");
    }

    #[test]
    fn a_calendar_address_alone_is_enough_and_it_stays_calendars_only() {
        let mut state = at_mail(Demo::NotFound);
        send(&mut state, Message::CaldavChanged("  dav.example.org ".into()));
        assert!(mail_can_continue(&state, Step::MailAddress));
        to_services(&mut state);
        assert_eq!(state.mail.services(), [Service::Calendars, Service::Tasks], "no mail servers, no mail");
        match send(&mut state, Message::Continue) {
            Action::Send(Request::ConnectCalDav { server, calendars, tasks, .. }) => {
                assert_eq!(server.as_deref(), Some("dav.example.org"));
                assert!(calendars && tasks);
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
    }

    #[test]
    fn only_half_found_servers_open_advanced_and_ask_for_the_other() {
        let mut state = at_mail(Demo::Address);
        type_address(&mut state, "anna@example.com");
        update(&mut state, Message::Continue, &Data::default());
        let imap = Endpoint { host: "imap.example.com".into(), port: 993, security: Security::Tls };
        let found = Response::Discovery {
            provider: "example.com".into(),
            urls: vec![],
            unsupported: None,
            imap: Some(imap),
            smtp: None,
            login_hint: None,
            app_password: false,
            mail_unsupported: None,
        };
        assert!(answered(&mut state, &Ok(found)));
        assert_eq!(state.mail.servers(), Servers::Incomplete);
        assert!(state.mail.advanced, "the outgoing server has to be typed");
        assert!(!mail_can_continue(&state, Step::MailAddress));
        assert_eq!(state.mail.smtp.port, "587", "with the usual port ready");
        send(&mut state, Message::ServerHostChanged(Server::Smtp, "smtp.example.com".into()));
        assert!(mail_can_continue(&state, Step::MailAddress));
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn another_security_takes_the_usual_port_along_but_not_a_typed_one() {
        let mut state = at_mail(Demo::NotFound);
        send(&mut state, Message::ServerSecurityPicked(Server::Imap, Security::StartTls));
        assert_eq!((state.mail.imap.port.as_str(), state.mail.imap.security), ("143", Security::StartTls));
        send(&mut state, Message::ServerSecurityPicked(Server::Smtp, Security::Tls));
        assert_eq!((state.mail.smtp.port.as_str(), state.mail.smtp.security), ("465", Security::Tls));
        send(&mut state, Message::ServerPortChanged(Server::Imap, "1143".into()));
        send(&mut state, Message::ServerSecurityPicked(Server::Imap, Security::Tls));
        assert_eq!(state.mail.imap.port, "1143", "a port the person chose stays");
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
        let found = Response::Discovery {
            provider: "x".into(),
            urls: vec![],
            unsupported: None,
            imap: None,
            smtp: None,
            login_hint: None,
            app_password: false,
            mail_unsupported: None,
        };
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
    fn the_services_the_servers_have_are_ticked() {
        let mut state = at_mail(Demo::Services);
        assert_eq!(state.step, Step::MailServices);
        assert_eq!(state.mail.services(), [Service::Mail, Service::Calendars, Service::Tasks]);
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        assert_eq!(state.mail.services(), [Service::Mail, Service::Calendars]);
        update(&mut state, Message::ToggleService(Service::Mail), &Data::default());
        update(&mut state, Message::ToggleService(Service::Calendars), &Data::default());
        assert!(!mail_can_continue(&state, Step::MailServices), "at least one service");
        assert!(matches!(update(&mut state, Message::Continue, &Data::default()), Action::None));
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        assert_eq!(state.mail.services(), [Service::Tasks]);
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn a_mail_only_provider_has_no_calendars_to_tick() {
        let mut state = at_mail(Demo::MailOnly);
        assert_eq!(state.mail.services(), [Service::Mail]);
        assert!(Service::Mail.is_available(&state.mail));
        assert!(!Service::Calendars.is_available(&state.mail) && !Service::Tasks.is_available(&state.mail));
        update(&mut state, Message::ToggleService(Service::Calendars), &Data::default());
        assert_eq!(state.mail.services(), [Service::Mail], "a greyed row does nothing");
        match send(&mut state, Message::Continue) {
            Action::Send(Request::ConnectMail { calendars, tasks, caldav, login_name, .. }) => {
                assert_eq!((calendars, tasks, caldav), (false, false, None), "the CalDAV address is not sent unused");
                assert_eq!(login_name, "anna", "the provider's hint");
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn connect_sends_the_mail_account_and_keeps_no_password() {
        let mut state = at_mail(Demo::Services);
        send(&mut state, Message::SenderNameChanged("  Anna Example ".into()));
        let action = send(&mut state, Message::Continue);
        match &action {
            Action::Send(Request::ConnectMail {
                address,
                sender_name,
                imap,
                smtp,
                login_name,
                password,
                calendars,
                tasks,
                caldav,
            }) => {
                assert_eq!((address.as_str(), sender_name.as_str()), ("anna@mailbox.org", "Anna Example"));
                assert_eq!((imap.host.as_str(), imap.port, imap.security), ("imap.mailbox.org", 993, Security::Tls));
                assert_eq!((smtp.host.as_str(), smtp.port, smtp.security), ("smtp.mailbox.org", 465, Security::Tls));
                assert_eq!(login_name, "anna@mailbox.org");
                assert_eq!(password.expose(), "demo-password");
                assert_eq!((*calendars, *tasks), (true, true));
                assert_eq!(caldav.as_deref(), Some("https://dav.mailbox.org/"));
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
        assert!(!format!("{action:?}").contains("demo-password"), "a Secret shows nothing");
        assert!(state.mail.password.is_blank(), "the dialog keeps no copy");
        assert!(!format!("{state:?}").contains("demo-password"));
        assert_eq!(state.step, Step::MailConnecting);
    }

    #[test]
    fn without_mail_only_calendars_and_tasks_are_connected_over_caldav() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::ToggleService(Service::Mail), &Data::default());
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        let action = update(&mut state, Message::Continue, &Data::default());
        match &action {
            Action::Send(Request::ConnectCalDav { address, server, login_name, password, calendars, tasks }) => {
                assert_eq!(address, "anna@mailbox.org");
                assert_eq!(server, &None, "the core tries every address it finds");
                assert_eq!(login_name, "anna@mailbox.org");
                assert_eq!(password.expose(), "demo-password");
                assert_eq!((*calendars, *tasks), (true, false));
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
        assert!(state.mail.password.is_blank());
        assert_eq!(state.step, Step::MailConnecting);
    }

    #[test]
    fn mail_only_leaves_out_the_calendar_address() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::ToggleService(Service::Calendars), &Data::default());
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        match update(&mut state, Message::Continue, &Data::default()) {
            Action::Send(Request::ConnectMail { calendars, tasks, caldav, .. }) => {
                assert_eq!((calendars, tasks, caldav), (false, false, None));
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn typed_servers_and_a_user_name_go_with_the_request() {
        let mut state = at_mail(Demo::NotFound);
        type_servers(&mut state);
        send(&mut state, Message::ServerSecurityPicked(Server::Imap, Security::StartTls));
        send(&mut state, Message::Continue);
        send(&mut state, Message::LoginChanged(" anna ".into()));
        send(&mut state, Message::PasswordChanged(Secret::new("pw")));
        send(&mut state, Message::Continue);
        match send(&mut state, Message::Continue) {
            Action::Send(Request::ConnectMail { imap, smtp, login_name, caldav, .. }) => {
                assert_eq!(
                    (imap.host.as_str(), imap.port, imap.security),
                    ("imap.example.org", 143, Security::StartTls)
                );
                assert_eq!(
                    (smtp.host.as_str(), smtp.port, smtp.security),
                    ("smtp.example.org", 587, Security::StartTls)
                );
                assert_eq!((login_name.as_str(), caldav), ("anna", None));
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn a_calendar_address_changed_under_advanced_goes_with_the_caldav_request() {
        let mut state = at_mail(Demo::Found);
        send(&mut state, Message::CaldavChanged("https://other.example.org/dav/".into()));
        to_services(&mut state);
        update(&mut state, Message::ToggleService(Service::Mail), &Data::default());
        match send(&mut state, Message::Continue) {
            Action::Send(Request::ConnectCalDav { server, .. }) => {
                assert_eq!(server.as_deref(), Some("https://other.example.org/dav/"));
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
    }

    #[test]
    fn a_refused_password_comes_back_under_the_password_field_naming_the_server() {
        for (demo, server) in [(Demo::Refused, "IMAP"), (Demo::RefusedSmtp, "SMTP")] {
            let mut state = at_mail(demo);
            assert_eq!(state.step, Step::MailPassword, "{server}");
            let said = trf("The {service} server refused the password.", &[("service", &server)]);
            assert_eq!(state.mail.refused.as_deref(), Some(said.as_str()), "{server}");
            assert!(state.error.is_none(), "not in the footer");
            assert!(state.mail.password.is_blank(), "to type again");
            drop(view(&state, &Data::default()));
            update(&mut state, Message::PasswordChanged(Secret::new("x")), &Data::default());
            assert_eq!(state.mail.refused, None, "an edit clears it");
        }
    }

    #[test]
    fn every_refusal_of_the_core_is_recognised() {
        assert!(password_refused(tr("Password was refused")), "a CalDAV account");
        for server in ["IMAP", "SMTP", "CalDAV"] {
            assert!(password_refused(&trf("The {service} server refused the password.", &[("service", &server)])));
        }
        assert!(!password_refused(&trf("The {service} server cannot be reached.", &[("service", &"IMAP")])));
        assert!(!password_refused("The server cannot be reached."));
        // The CalDAV-only request is refused the same way as before.
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::ToggleService(Service::Mail), &Data::default());
        update(&mut state, Message::PasswordChanged(Secret::new("wrong")), &Data::default());
        state.step = Step::MailConnecting;
        assert!(answered(&mut state, &Err(tr("Password was refused").into())));
        assert_eq!(state.step, Step::MailPassword);
    }

    #[test]
    fn any_other_error_goes_to_the_footer_of_the_services_step() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::Continue, &Data::default());
        assert!(answered(&mut state, &Err("The server cannot be reached.".into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::MailServices, Some("The server cannot be reached.")));
        assert_eq!(state.mail.refused, None);
        // Untick calendars that were not found and go on.
        assert_eq!(state.mail.services().len(), 3, "the choice stays for the next try");
    }

    #[test]
    fn done_says_what_the_servers_have() {
        let state = at_mail(Demo::Done);
        assert_eq!(state.step, Step::Done);
        assert_eq!(subtitle(&state), tr("Your account is connected."));
        let connected = state.mail.connected.clone().expect("connected");
        assert_eq!(
            done_lines(&connected, &state.mail.services()),
            [(true, tr("Mail is ready.")), (true, tr("Calendars are ready.")), (true, tr("Tasks are ready."))]
        );
        let no_tasks = Connected { account: "anna@mailbox.org".into(), calendars: true, tasks: false };
        assert_eq!(
            done_lines(&no_tasks, &[Service::Tasks, Service::Mail]),
            [(false, tr("This server has no task lists.")), (true, tr("Mail is ready."))],
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
    fn what_was_ticked_survives_a_step_back() {
        let mut state = at_mail(Demo::Services);
        update(&mut state, Message::ToggleService(Service::Tasks), &Data::default());
        update(&mut state, Message::Back, &Data::default());
        update(&mut state, Message::Continue, &Data::default());
        assert_eq!(state.mail.services(), [Service::Mail, Service::Calendars]);
    }

    #[test]
    fn every_demo_state_leads_where_it_says() {
        let at = |demo| at_mail(demo);
        assert_eq!((at(Demo::Address).step, at(Demo::Address).mail.found), (Step::MailAddress, None));
        assert!(found_of(Demo::Found).is_usable());
        assert!(at(Demo::Advanced).mail.advanced && at(Demo::Advanced).mail.found.is_some());
        assert!(found_of(Demo::Unsupported).is_unsupported());
        assert!(found_of(Demo::Gmail).is_usable());
        assert!(found_of(Demo::NotFound).is_nothing());
        assert_eq!(at(Demo::Password).step, Step::MailPassword);
        assert_eq!(at(Demo::Refused).step, Step::MailPassword);
        assert!(at(Demo::Refused).mail.refused.is_some() && at(Demo::RefusedSmtp).mail.refused.is_some());
        assert_eq!(at(Demo::Services).step, Step::MailServices);
        assert_eq!(at(Demo::MailOnly).step, Step::MailServices);
        assert_eq!(at(Demo::Done).step, Step::Done);
    }

    #[test]
    fn the_mail_views_build_in_every_state() {
        for demo in [
            Demo::Address,
            Demo::Found,
            Demo::Advanced,
            Demo::Unsupported,
            Demo::Gmail,
            Demo::NotFound,
            Demo::Password,
            Demo::Refused,
            Demo::RefusedSmtp,
            Demo::Services,
            Demo::MailOnly,
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

    // TorroMail (task M7.4).

    fn torromail_account(id: &str, address: &str) -> TorroMailAccount {
        let server = |host: &str, port, security| Some(Endpoint { host: host.into(), port, security });
        TorroMailAccount {
            id: id.into(),
            address: address.into(),
            sender_name: "Anna Example".into(),
            provider: "IMAP/SMTP".into(),
            imap: server("imap.example.org", 993, Security::Tls),
            smtp: server("smtp.example.org", 587, Security::StartTls),
            login_name: "anna".into(),
            unsupported: None,
            connected: false,
        }
    }

    /// A dialog opened as the accounts page opens it, with TorroMail's `accounts` listed.
    fn with_torromail(accounts: Vec<TorroMailAccount>) -> State {
        let (mut state, ask) = State::open();
        assert!(matches!(ask, Action::Send(Request::TorroMailAccounts)), "the dialog asks as it opens");
        assert!(answered(&mut state, &Ok(Response::TorroMailAccounts { accounts })));
        state
    }

    #[test]
    fn torromail_accounts_are_taken_over_one_at_a_time_and_end_with_done() {
        let data = Data::default();
        let mut gmail = torromail_account("g", "anna@gmail.com");
        gmail.unsupported = Some("Signs in with Google".into());
        let mut known = torromail_account("k", "anna@mailbox.org");
        known.connected = true;
        let accounts =
            vec![torromail_account("p", "anna@posteo.de"), torromail_account("f", "anna@fastmail.com"), gmail, known];
        let mut state = with_torromail(accounts);
        drop(view(&state, &data));
        let press = |id: &str, state: &mut State| update(state, Message::UseTorroMail(id.into()), &data);
        assert!(matches!(press("g", &mut state), Action::None), "not supported");
        assert!(matches!(press("k", &mut state), Action::None), "connected already");
        let action = press("p", &mut state);
        assert!(matches!(action, Action::Send(Request::ImportTorroMailAccount { ref id }) if id == "p"), "{action:?}");
        assert!(matches!(press("f", &mut state), Action::None), "one at a time");
        update(&mut state, Message::ChooseProvider(Provider::Mail), &data);
        assert_eq!(state.step, Step::Provider, "the catalog waits for the take-over");
        drop(view(&state, &data));

        let done = Response::MailConnected {
            account: account("mail:anna@posteo.de", connected()),
            calendars: false,
            tasks: false,
            open_roles: Vec::new(),
        };
        assert!(answered(&mut state, &Ok(done)));
        assert!(state.torromail.accounts[0].connected && state.torromail.imported);
        assert_eq!((state.step, state.error.as_deref()), (Step::Provider, None));
        assert!(matches!(press("f", &mut state), Action::Send(Request::ImportTorroMailAccount { .. })), "the next one");
        drop(view(&state, &data));
    }

    #[test]
    fn a_failed_take_over_offers_the_password_with_torromails_servers() {
        let data = Data::default();
        let mut state = with_torromail(vec![torromail_account("f", "anna@fastmail.com")]);
        update(&mut state, Message::UseTorroMail("f".into()), &data);
        let message = "The password could not be read from TorroMail. Enter it to connect.";
        assert!(answered(&mut state, &Err(message.into())));
        assert_eq!(state.torromail.failed, Some(("f".to_string(), message.to_string())));
        assert_eq!((state.step, state.error.as_deref()), (Step::Provider, None), "the row says it, not the footer");
        drop(view(&state, &data));

        update(&mut state, Message::EnterTorroMailPassword("f".into()), &data);
        assert_eq!((state.step, state.provider), (Step::MailPassword, Provider::Mail));
        update(&mut state, Message::PasswordChanged(Secret::new("typed")), &data);
        update(&mut state, Message::Continue, &data);
        assert_eq!(state.step, Step::MailServices);
        match update(&mut state, Message::Continue, &data) {
            Action::Send(Request::ConnectMail {
                address,
                sender_name,
                imap,
                smtp,
                login_name,
                caldav,
                password,
                ..
            }) => {
                assert_eq!(
                    (address.as_str(), sender_name.as_str(), login_name.as_str()),
                    ("anna@fastmail.com", "Anna Example", "anna")
                );
                assert_eq!((imap.host.as_str(), smtp.port, caldav), ("imap.example.org", 587, None));
                assert_eq!(password.expose(), "typed");
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn without_torromail_the_catalog_is_as_before() {
        let (mut state, _) = State::open();
        assert!(answered(&mut state, &Err("TorroMail's state.json is not valid JSON".into())), "taken quietly");
        assert_eq!(state.error, None);
        assert!(offer_group(&state.torromail).is_none());
        assert!(!answered(&mut state, &Err("no template".into())), "afterwards the steps take their answers");
        let empty = with_torromail(Vec::new());
        assert!(offer_group(&empty.torromail).is_none(), "no TorroMail accounts, no question");
    }

    #[test]
    fn the_torromail_demos_lead_where_they_say() {
        for (demo, step) in [
            (Demo::TorroMailFailed, Step::Provider),
            (Demo::TorroMailPassword, Step::MailPassword),
            (Demo::TorroMailDone, Step::Provider),
        ] {
            let messages = demo_messages(demo);
            assert!(matches!(messages.first(), Some(Message::UseTorroMail(_))), "{demo:?}");
            let mut state = with_torromail(vec![
                torromail_account("tm-posteo", "anna.example@posteo.de"),
                torromail_account("tm-fastmail", "anna@fastmail.com"),
            ]);
            for message in messages {
                let sent = update(&mut state, message, &Data::default());
                if let Action::Send(Request::ImportTorroMailAccount { id }) = sent {
                    let answer = match id.as_str() {
                        "tm-fastmail" => Err("refused".to_string()),
                        _ => Ok(Response::MailConnected {
                            account: account("mail:anna.example@posteo.de", connected()),
                            calendars: false,
                            tasks: false,
                            open_roles: Vec::new(),
                        }),
                    };
                    answered(&mut state, &answer);
                }
            }
            assert_eq!(state.step, step, "{demo:?}");
            drop(view(&state, &Data::default()));
        }
    }

    #[test]
    fn servers_name_a_shared_host_once() {
        let mut posteo = torromail_account("p", "anna@posteo.de");
        assert_eq!(servers_of(&posteo), "imap.example.org · smtp.example.org");
        posteo.smtp = posteo.imap.clone();
        assert_eq!(servers_of(&posteo), "imap.example.org");
    }

    // ----- AI providers (task KI.1a) -----

    fn ai_server(provider: AiProvider, url: &str, account: Option<&str>) -> AiServer {
        AiServer { provider, url: url.into(), models: 3, account: account.map(str::to_string) }
    }

    /// The AI providers, opened from the catalog: the search went out.
    fn at_ai() -> State {
        let mut state = at_catalog();
        let action = update(&mut state, Message::ChooseProvider(Provider::Ai), &Data::default());
        assert!(matches!(action, Action::Send(Request::FindAiServers)), "{action:?}");
        assert_eq!(state.step, Step::AiChoose);
        state
    }

    #[test]
    fn local_servers_are_connected_from_their_row_without_a_key() {
        let mut state = at_ai();
        drop(view(&state, &Data::default()));
        let found = vec![ai_server(AiProvider::Ollama, "http://localhost:11434/v1", None)];
        assert!(answered(&mut state, &Ok(Response::AiServers { servers: found })));
        assert!(state.ai.found.as_ref().is_some_and(|found| found.len() == 1));
        assert_eq!(
            local_ai_detail(&ai_server(AiProvider::Ollama, "http://localhost:11434/v1", None)),
            trf("{host} · {n} models", &[("host", &"localhost:11434"), ("n", &3)])
        );

        let press = Message::ConnectLocalAi(AiProvider::Ollama, "http://localhost:11434/v1".into());
        let sent = update(&mut state, press.clone(), &Data::default());
        let Action::Send(Request::ConnectAi { provider, server, key }) = sent else { panic!("connect") };
        assert_eq!(
            (provider, server.as_deref(), key.is_blank()),
            (AiProvider::Ollama, Some("http://localhost:11434/v1"), true)
        );
        assert_eq!(state.step, Step::AiChoose, "the row says Connecting…");
        assert!(matches!(update(&mut state, press, &Data::default()), Action::None), "one at a time");
        drop(view(&state, &Data::default()));

        let refused = Err("The AI server could not be reached.".to_string());
        assert!(answered(&mut state, &refused));
        assert_eq!((state.step, state.error.as_deref()), (Step::AiChoose, Some("The AI server could not be reached.")));
        let press = Message::ConnectLocalAi(AiProvider::Ollama, "http://localhost:11434/v1".into());
        update(&mut state, press, &Data::default());
        let mut ollama = account("ai:ollama@localhost:11434", connected());
        ollama.label = "Ollama".into();
        assert!(answered(&mut state, &Ok(Response::Account { account: ollama })));
        assert_eq!(state.step, Step::Done);
        assert_eq!(done_text(&state), trf("{name} is ready to use.", &[("name", &"Ollama")]));
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn a_search_that_fails_shows_every_local_server_as_not_running() {
        let mut state = at_ai();
        assert!(answered(&mut state, &Err("daemon not running".into())));
        assert_eq!(state.ai.found, Some(Vec::new()));
        assert_eq!(state.error.as_deref(), Some("daemon not running"));
        assert!(!answered(&mut state, &Ok(Response::Ok)), "nothing waits any more");
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn openrouter_needs_a_key_which_the_dialog_does_not_keep() {
        let mut state = at_ai();
        update(&mut state, Message::ChooseAi(AiProvider::OpenRouter), &Data::default());
        assert_eq!(state.step, Step::AiKey);
        assert!(matches!(send(&mut state, Message::Continue), Action::None), "no key, no request");
        update(&mut state, Message::AiKeyChanged(Secret::new("sk-or-v1-SECRET")), &Data::default());
        assert!(!format!("{state:?}").contains("SECRET"));
        let Action::Send(Request::ConnectAi { provider, server, key }) = send(&mut state, Message::Continue) else {
            panic!("connect")
        };
        assert_eq!((provider, server, key.expose()), (AiProvider::OpenRouter, None, "sk-or-v1-SECRET"));
        assert!(state.ai.key.is_blank(), "taken, not copied");
        assert_eq!(state.step, Step::AiConnecting);
        assert!(answered(&mut state, &Err("The AI server did not accept the API key.".into())));
        assert_eq!(state.step, Step::AiKey, "back to the key, to paste it again");
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::AiChoose);
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::Provider);
        assert!(matches!(send(&mut state, Message::OpenAiKeyPage), Action::OpenUrl(url) if url == OPENROUTER_KEYS_URL));
    }

    #[test]
    fn another_server_needs_an_address_and_takes_a_key() {
        let mut state = at_ai();
        update(&mut state, Message::ChooseAi(AiProvider::OpenAiCompatible), &Data::default());
        assert_eq!(state.step, Step::AiServer);
        update(&mut state, Message::AiServerChanged("   ".into()), &Data::default());
        assert!(matches!(send(&mut state, Message::Continue), Action::None));
        update(&mut state, Message::AiServerChanged(" https://ai.example.org ".into()), &Data::default());
        let Action::Send(Request::ConnectAi { provider, server, key }) = send(&mut state, Message::Continue) else {
            panic!("connect")
        };
        assert_eq!(
            (provider, server.as_deref(), key.is_blank()),
            (AiProvider::OpenAiCompatible, Some("https://ai.example.org"), true)
        );
        let mut custom = account("ai:openai_compatible@ai.example.org", connected());
        custom.label = "ai.example.org".into();
        assert!(answered(&mut state, &Ok(Response::Account { account: custom })));
        assert_eq!((state.step, subtitle(&state)), (Step::Done, tr("Your AI account is connected.")));
    }

    #[test]
    fn the_ai_demos_lead_where_they_say() {
        let data = super::super::launch::demo_data();
        for (demo, step) in [
            (Demo::AiProviders, Step::AiChoose),
            (Demo::AiOpenRouter, Step::AiKey),
            (Demo::AiRefused, Step::AiKey),
            (Demo::AiServer, Step::AiServer),
            (Demo::AiDone, Step::Done),
        ] {
            let mut state = at_catalog();
            for message in demo_messages(demo) {
                if let Action::Send(request) = update(&mut state, message, &data) {
                    let answer = super::super::launch::demo_answer(&request, &data).expect("the demo answers");
                    answered(&mut state, &Ok(answer));
                }
            }
            assert_eq!(state.step, step, "{demo:?}");
            assert_eq!(state.error.is_some(), demo == Demo::AiRefused, "{demo:?}");
            drop(view(&state, &data));
        }
    }
}
