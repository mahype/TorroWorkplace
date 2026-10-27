//! The add-account dialog (task S.6 in `docs/tasks/steuerzentrale.md`, GitHub since G.1, one
//! provider per dialog since S.30).
//!
//! Steps, with the subtitle changing per step, errors bottom left, "Cancel" and the primary
//! button bottom right (like TorroMail's setup wizard):
//!
//! 1. **Where it starts (task S.30):** Accounts has a group per provider, and its "Add" opens the
//!    dialog at that provider's first step ([`State::at`]); "Import from TorroMail…" of the group
//!    Mail opens it at TorroMail's accounts ([`State::torromail`]). That first step has no "Back":
//!    there is no catalog to go back to. "Back" walks only between the steps of one provider.
//!    The old catalog (**Provider**, [`State::new`]) is still the state a dialog is built from, but
//!    no page shows it any more.
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
//!    A server whose certificate the system does not trust (`Response::UntrustedCertificate`, task
//!    M11.4) leads to **Certificate**: host, service and fingerprint, with a clear warning for a
//!    host on the public internet; "Trust certificate" sends `ConnectMail` again with every
//!    certificate trusted in this dialog ([`Certificates`]). Only for this question the dialog
//!    keeps the password; any other answer, "Back" or closing clears it.
//!    The steps and the check boxes are data, so a step or a service is added without a new screen.
//! 5. AI providers (task KI.1a, Open First): **AI** – opening it sends `Request::FindAiServers`;
//!    "On this computer" lists Ollama, LM Studio and Strata, each found one with its models and
//!    "Connect" (one press sends `Request::ConnectAi` for its address, no key), the others as "Not
//!    running on this computer". Below, OpenRouter opens **API key** (a masked field and a link to
//!    OpenRouter's key page) and "Other OpenAI-compatible server" opens **Server** (address and an
//!    optional key). "Connect" sends `Request::ConnectAi` and waits in **Checking**; an error comes
//!    back to the step it was sent from, in the footer.
//! 6. ntfy (task E5.14): **Server** – the address, empty for `ntfy.sh`; **Topics** – a row per
//!    topic with its name and the label the inbox shows ("Add topic", at most
//!    [`crate::account::ntfy::TOPICS_MAX`]); "Connect" sends `Request::ConnectNtfy` and waits in
//!    **Checking**. When the server wants a login (the core's `auth` sentence) the dialog goes on to
//!    **Login** – a user name and password, or an access token, the secret a [`Secret`] taken when
//!    sent – and sends `Request::SetNtfyLogin`. Topic names work like passwords on public servers,
//!    so they live in [`NtfyTopic`] and travel in messages as [`TopicName`]: `Debug` shows neither.
//!    What is wrong with a row shows only after the first "Connect" (the design's rule for forms).
//! 7. **Done:** "Your calendars and task lists are ready." (Nextcloud), "Signed in to GitHub as
//!    …" (GitHub), "… is ready to use." (AI), "Your topics are subscribed." (ntfy) or what the mail
//!    account's servers have, and "Done".
//!
//! **TorroMail** (tasks M7.4, S.30): the step **TorroMail** asks for TorroMail's accounts as it
//! opens (`Request::TorroMailAccounts`) and lists them: one row per account with its address and
//! servers and "Use", which sends `Request::ImportTorroMailAccount` for mail only (pressing it is the
//! consent to copy the password); accounts the suite cannot take (OAuth, JMAP) are greyed out with
//! the reason, those it has already say "Connected". A take-over runs on the step itself, one at a
//! time; when it fails (TorroMail's keychain entry cannot be read, a server refuses), the row shows
//! why and "Enter password", which opens the mail path at the password step, filled with
//! TorroMail's servers and user name ("Back" from its address step leads back to the list). After a
//! take-over the step ends with "Done". [`can_import`] says whether a list has an account to take
//! over, so Accounts shows its button only then.
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

use iced::widget::space as blank;
use iced::widget::text::Wrapping;
use iced::widget::{Space, checkbox, column, container, row, svg, text, text_input, toggler};
use iced::{Alignment, Color, Element, Fill, Theme};
use torro_core::account::{NtfyTopic, Secret};
use torro_core::ai::AiProvider;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, space};
use torro_gui::{dialog, layout, style, typo, widgets};
use torro_ipc::{AiServer, Request, Response, TopicName, TorroMailAccount};
use torro_mail::{Endpoint, Security};
use torro_ntfy::{NtfyError, Topic};

use crate::account::ntfy::{LABEL_MAX, NtfyAccountError, TOPICS_MAX};

use super::accounts::on_axis;
use super::connect::github::{self, GithubSignIn};
use super::connect::nextcloud::{self, Ended, NextcloudSignIn};
use super::server_fields::{Server, ServerFields, imap_titles, server_group, smtp_titles};
use super::{Action, Data, chevron, one_line, pressable, row_frame, setting_row};

/// The step the dialog is on. Each step has its own subtitle and footer button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    /// The catalog of providers: where [`State::new`] starts. No page opens the dialog here any more
    /// (task S.30); [`State::at`] goes on from it to a provider's first step.
    #[default]
    Provider,
    /// TorroMail's accounts to take over (tasks M7.4, S.30), opened by [`State::torromail`].
    TorroMail,
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
    /// Mail provider: waiting for the core to sign in (`ConnectMail`, `ConnectCalDav`).
    MailConnecting,
    /// Mail provider: a server showed a certificate the system does not trust (task M11.4):
    /// "Trust certificate" sends `ConnectMail` again with it, "Back" leads to the password.
    MailCertificate,
    /// AI providers: the servers on this computer, OpenRouter and any other server.
    AiChoose,
    /// AI providers: OpenRouter's API key.
    AiKey,
    /// AI providers: the address (and maybe a key) of another OpenAI-compatible server.
    AiServer,
    /// AI providers: waiting for the core to check the server (`ConnectAi`).
    AiConnecting,
    /// ntfy: the server's address (task E5.14).
    NtfyServer,
    /// ntfy: the topics and their labels.
    NtfyTopics,
    /// ntfy: the login the server asked for.
    NtfyLogin,
    /// ntfy: waiting for the core to check the topics (`ConnectNtfy`, `SetNtfyLogin`).
    NtfyChecking,
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

    /// The step before this one on the mail path; the first leads back to the catalog, which
    /// `Message::Back` turns into where the dialog started (`State::origin`).
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

/// A provider the dialog sets up: one group of the accounts page each (task S.30). Slack comes as
/// a provider of its own once it is built; until then it is listed nowhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Nextcloud,
    /// A mail account: its mail, and the calendars and tasks of the same sign-in (tasks M7.2, M5.1).
    Mail,
    GitHub,
    /// An ntfy server's topics for the inbox (task E5.14).
    Ntfy,
    Ai,
}

impl Provider {
    /// The step after picking the provider: where [`State::at`] opens the dialog.
    fn first_step(self) -> Step {
        match self {
            Provider::GitHub => Step::Github,
            Provider::Mail => MAIL_STEPS[0],
            Provider::Ai => Step::AiChoose,
            Provider::Ntfy => Step::NtfyServer,
            Provider::Nextcloud => Step::Server,
        }
    }

    /// Every provider, in the order of the catalog.
    #[cfg(test)]
    const ALL: [Provider; 5] = [Provider::Nextcloud, Provider::Mail, Provider::GitHub, Provider::Ntfy, Provider::Ai];
}

#[derive(Debug, Default)]
pub struct State {
    step: Step,
    /// The step the dialog opened at (task S.30): a provider's first step, TorroMail's accounts, or
    /// the catalog of [`State::new`]. It has no "Back", and "Back" towards the catalog leads here.
    origin: Step,
    /// The provider being set up; Done says what is ready.
    provider: Provider,
    /// The Nextcloud sign-in: the address as typed and the sign-in while it runs (task W.5).
    nextcloud: NextcloudSignIn,
    /// The GitHub sign-in: the token as pasted and the check (task W.6).
    github: GithubSignIn,
    /// The step a failed GitHub check goes back to: the two ways or the token field.
    back: Step,
    /// The GitHub login once connected, for Done.
    login: Option<String>,
    /// Error text for the footer: the core's answer or the shell's last error.
    error: Option<String>,
    /// What the mail provider path has asked for and learned so far.
    mail: Mail,
    /// TorroMail's accounts on the step TorroMail (tasks M7.4, S.30).
    torromail: Offer,
    /// What the AI provider path has asked for and learned so far (task KI.1a).
    ai: AiSetup,
    /// What the ntfy path has asked for so far (task E5.14).
    ntfy: NtfySetup,
}

/// The ntfy path (task E5.14): the server, the topics as typed and the login once the server asks
/// for one. Topic names are [`NtfyTopic`]s and the secret a [`Secret`], so a `Debug` of the dialog
/// shows neither.
#[derive(Debug, Default)]
struct NtfySetup {
    /// The address as typed; empty means `ntfy.sh`.
    server: String,
    /// One per row of the topics step, empty rows included.
    topics: Vec<NtfyTopic>,
    /// "Connect" was pressed on the topics: from then on the step says what is wrong.
    tried: bool,
    login: NtfyLogin,
}

impl NtfySetup {
    /// The rows that hold something, trimmed; a row left empty is no topic.
    fn filled(&self) -> Vec<NtfyTopic> {
        self.topics
            .iter()
            .map(|row| NtfyTopic { topic: row.topic.trim().to_string(), label: row.label.trim().to_string() })
            .filter(|row| !row.topic.is_empty() || !row.label.is_empty())
            .collect()
    }

    /// What keeps the topics from being sent, in the core's words; `None` when they can go.
    fn problem(&self) -> Option<String> {
        let topics = self.filled();
        if topics.is_empty() {
            return Some(tr("Add at least one topic.").to_string());
        }
        topics.iter().find_map(|row| topic_problem(&row.topic).or_else(|| label_problem(&row.label)))
    }
}

/// A login for an ntfy server as it is being typed (task E5.14), in the add-account dialog and in
/// "Change login" on the account's page: a user name and password, or an access token (`tk_…`)
/// without a user name. The secret is a [`Secret`], so `Debug` never shows it, and
/// [`NtfyLogin::take`] takes it when it is sent, so no copy stays behind.
#[derive(Debug, Default)]
pub struct NtfyLogin {
    /// An access token instead of a user name and password.
    pub(super) token: bool,
    pub(super) user: String,
    pub(super) secret: Secret,
}

impl NtfyLogin {
    /// Something to send: a secret and, for a password, a user name.
    pub(super) fn is_ready(&self) -> bool {
        !self.secret.is_blank() && (self.token || !self.user.trim().is_empty())
    }

    /// The user name and the secret for `Request::SetNtfyLogin`: no user name with a token. The
    /// secret is taken out of the form.
    pub(super) fn take(&mut self) -> (Option<String>, Secret) {
        let user = Some(self.user.trim().to_string()).filter(|user| !self.token && !user.is_empty());
        (user, std::mem::take(&mut self.secret))
    }
}

/// What is wrong with a topic name as typed (trimmed), in the core's sentence so both say the
/// same; `None` for a name `torro_ntfy` takes.
pub(super) fn topic_problem(name: &str) -> Option<String> {
    Topic::parse(name.trim()).err().map(|err| NtfyAccountError::Ntfy(err).message())
}

/// What is wrong with a label as typed: blank, longer than [`LABEL_MAX`] characters or more than
/// one line, as the core checks it; `None` for a label it takes.
pub(super) fn label_problem(label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        return Some(tr("Give the topic a label.").to_string());
    }
    let fits = label.chars().count() <= LABEL_MAX && !label.chars().any(char::is_control);
    (!fits).then(|| trf("Give every topic a label of up to {n} characters.", &[("n", &LABEL_MAX)]))
}

/// The core's answer when the topics need a login (`auth` of `accounts.connect_ntfy`). Compared
/// with the message because the shell passes on messages, not codes; both run in the same language.
fn login_needed(message: &str) -> bool {
    message == NtfyAccountError::Ntfy(NtfyError::Auth).message()
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
    /// The cloud provider whose key is asked for (task W.3); OpenRouter unless another was chosen.
    cloud: Option<AiProvider>,
    /// Use the provider's platform for mainland China ([`AiProvider::china_url`]).
    china: bool,
    /// The connected account's name, for Done.
    connected: Option<String>,
}

/// TorroMail's accounts on the step TorroMail (tasks M7.4, S.30): asked for when it opens
/// ([`State::torromail`]), each taken over with one press, one at a time.
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
    /// An account was taken over in this dialog: the step ends with "Done".
    imported: bool,
}

impl Offer {
    /// "Use" on `id` sends something: TorroMail's account can be taken over and is not yet, and no
    /// other take-over runs.
    fn can_use(&self, id: &str) -> bool {
        self.importing.is_none() && self.accounts.iter().any(|account| account.id == id && takeable(account))
    }
}

/// TorroMail's account can be taken over: the suite supports its sign-in and has no account of
/// that address yet.
fn takeable(account: &TorroMailAccount) -> bool {
    account.unsupported.is_none() && !account.connected
}

/// Whether TorroMail's `accounts`, as `accounts.list_torromail_accounts` lists them, hold one the
/// step TorroMail can take over ([`Offer::can_use`] while nothing runs). Accounts shows "Import
/// from TorroMail…" only then (task S.30).
pub fn can_import(accounts: &[TorroMailAccount]) -> bool {
    accounts.iter().any(takeable)
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
    /// The certificates trusted on this path and the one asked about (task M11.4).
    certificates: Certificates,
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
            certificates: Certificates::default(),
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

impl State {
    /// A dialog on the catalog of providers. No page opens it there any more (task S.30): it is
    /// what [`State::at`] starts from, and what the tests of the catalog's own rules use.
    pub fn new() -> State {
        State::default()
    }

    /// A dialog at the first step of `provider`, without "Back" there (task S.30): how a group's
    /// "Add" on the accounts page and the steps of the setup open it. Returns the request that step
    /// sends as it opens – `Request::FindAiServers` for the AI providers, else `Action::None`. The
    /// same as pressing `provider` on the catalog, so every rule of the provider's path holds.
    pub fn at(provider: Provider, data: &Data) -> (State, Action) {
        let mut state = State::new();
        let action = update(&mut state, Message::ChooseProvider(provider), data);
        state.origin = state.step;
        (state, action)
    }

    /// A dialog at the step TorroMail (tasks M7.4, S.30), without "Back" there, and the request
    /// that asks for TorroMail's accounts (`Request::TorroMailAccounts`), whose answer fills the
    /// list ([`answered`]). How "Import from TorroMail…" of the accounts page and "Take over from
    /// TorroMail" of the setup open it.
    pub fn torromail() -> (State, Action) {
        let mut state = State { step: Step::TorroMail, origin: Step::TorroMail, ..State::new() };
        state.torromail.asking = true;
        (state, Action::Send(Request::TorroMailAccounts))
    }

    /// A sign-in or a check runs; the shell reloads more often.
    pub fn busy(&self) -> bool {
        matches!(self.step, Step::Checking | Step::NtfyChecking) || self.nextcloud.running()
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
    /// "Create a key on <provider>".
    OpenAiKeyPage,
    /// "Use the platform for mainland China" on the key step of a provider that has one.
    AiChina(bool),
    /// The ntfy server's address changed (task E5.14).
    NtfyServerChanged(String),
    /// The name of the topic in row `.0` changed. A [`TopicName`], so `Debug` shows no name.
    NtfyTopicChanged(usize, TopicName),
    /// The label of the topic in row `.0` changed.
    NtfyLabelChanged(usize, String),
    /// "Add topic": one more empty row.
    AddNtfyTopic,
    /// The ✕ of a topic's row.
    RemoveNtfyTopic(usize),
    /// "User and password" (`false`) or "Access token" (`true`) on the login step.
    NtfyLoginKind(bool),
    NtfyUserChanged(String),
    /// The password or token changed. A [`Secret`] like the GitHub token.
    NtfySecretChanged(Secret),
}

/// Follows a Nextcloud sign-in through a reload of `data` and ends it as the module doc says.
///
/// Call it on every reload of `data` while the dialog is open; only the sign-in step reacts.
/// While waiting it also copies the shell's error (for example "daemon not running") into the
/// footer.
pub fn observe(state: &mut State, data: &Data) {
    if state.step != Step::SigningIn || !state.nextcloud.running() {
        return;
    }
    match state.nextcloud.observe(data) {
        Some(Ended::SignedIn(_)) => {
            state.step = Step::Done;
            state.error = None;
        }
        Some(Ended::Failed) => {
            state.step = Step::Server;
            state.error = Some(tr("Signing in did not finish. Try again.").to_string());
        }
        // While waiting the footer shows the shell's error (for example "daemon not running").
        None => state.error = data.error.clone(),
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
    // The password stays only while a server's certificate is asked about (task M11.4): then the
    // password did not go out and is needed again for "Trust certificate".
    if state.step == Step::MailConnecting {
        if state.mail.certificates.ask(result) {
            state.step = Step::MailCertificate;
            state.error = None;
            return true;
        }
        state.mail.password = Secret::default();
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
        (Step::Checking, result) => match state.github.answered(result) {
            Some(Ok(login)) => {
                state.login = Some(login);
                state.step = Step::Done;
                state.error = None;
                true
            }
            Some(Err(message)) => {
                state.step = state.back;
                state.error = Some(message);
                true
            }
            None => false,
        },
        // The sign-in could not start (no such server): back to the address.
        (Step::SigningIn, Err(message)) => {
            state.step = Step::Server;
            state.nextcloud.stop();
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
        (Step::NtfyChecking, Ok(Response::Account { .. })) => {
            state.step = Step::Done;
            state.error = None;
            true
        }
        // Topics that need a login lead on to it; the step says why, so the footer stays empty.
        (Step::NtfyChecking, Err(message)) if state.back == Step::NtfyTopics && login_needed(message) => {
            state.step = Step::NtfyLogin;
            state.error = None;
            true
        }
        (Step::NtfyChecking, result) => {
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

/// The answers to TorroMail's requests (tasks M7.4, S.30): the list whenever it comes, the outcome
/// of a take-over while one runs, and a list that failed while the dialog is on the step TorroMail
/// – in its footer, since the list is all the step has. `None` for every other answer, which the
/// steps take.
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
            // The question needs the password, which the mail path asks for (task M11.4).
            Ok(Response::UntrustedCertificate { .. }) => {
                offer.failed = Some((id, tr("The certificate is not trusted. Enter the password to check it.").into()));
            }
            Ok(_) => offer.failed = Some((id, tr("Torro Workplace did not answer as expected. Try again.").into())),
            Err(message) => offer.failed = Some((id, message.clone())),
        }
        return Some(true);
    }
    if let (true, Step::TorroMail, Err(message)) = (offer.asking, state.step, result) {
        offer.asking = false;
        state.error = Some(message.clone());
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
        // The dialog goes away with its state; the password goes first all the same.
        Message::Close => {
            state.mail.password = Secret::default();
            Action::None
        }
        Message::LoginLinkEdited => Action::None,
        // The step the dialog opened at has no way back (task S.30): there is no catalog behind it.
        Message::Back if state.step == state.origin => Action::None,
        Message::Back => {
            state.error = None;
            match state.step {
                Step::Token => state.step = Step::Github,
                Step::Github | Step::Server => state.step = state.origin,
                // The certificate was not trusted: the password is typed again (task M11.4).
                Step::MailCertificate => {
                    state.mail.certificates.dismiss();
                    state.mail.password = Secret::default();
                    go(state, Step::MailPassword);
                }
                step @ (Step::MailAddress | Step::MailPassword | Step::MailServices) => match step.mail_previous() {
                    // TorroMail's "Enter password" leads back to its list.
                    Step::Provider => state.step = state.origin,
                    previous => go(state, previous),
                },
                Step::AiChoose => state.step = state.origin,
                Step::AiKey | Step::AiServer => {
                    state.step = Step::AiChoose;
                    state.ai.key = Secret::default();
                }
                Step::NtfyServer => state.step = state.origin,
                Step::NtfyTopics => state.step = Step::NtfyServer,
                Step::NtfyLogin => {
                    state.step = Step::NtfyTopics;
                    state.ntfy.login.secret = Secret::default();
                }
                _ => {}
            }
            Action::None
        }
        // While a TorroMail account is taken over, the catalog waits for it.
        Message::ChooseProvider(provider) => {
            if state.step == Step::Provider && state.torromail.importing.is_none() {
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
            state.step = if provider.needs_key() { Step::AiKey } else { Step::AiServer };
            state.ai.cloud = provider.needs_key().then_some(provider);
            state.ai.china = false;
            state.error = None;
            Action::None
        }
        Message::AiChina(china) => {
            state.ai.china = china;
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
        Message::OpenAiKeyPage => match cloud(state).key_url() {
            Some(url) => Action::OpenUrl(url.to_string()),
            None => Action::None,
        },
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
            state.nextcloud.server = server;
            Action::None
        }
        Message::Continue => match state.step {
            Step::Server if state.nextcloud.can_start() => {
                state.step = Step::SigningIn;
                state.error = None;
                state.nextcloud.start(data)
            }
            Step::Token if state.github.has_token() => {
                state.back = Step::Token;
                state.step = Step::Checking;
                state.error = None;
                state.github.send_token()
            }
            step @ (Step::MailAddress | Step::MailPassword | Step::MailServices | Step::MailCertificate) => {
                continue_mail(state, step)
            }
            Step::AiKey if ai_can_connect(state) => {
                let provider = cloud(state);
                // The China platform is another address; the international one is the usual.
                let server = state.ai.china.then(|| provider.china_url()).flatten().map(str::to_string);
                connect_ai(state, provider, server)
            }
            Step::AiServer if ai_can_connect(state) => {
                let server = state.ai.server.trim().to_string();
                connect_ai(state, AiProvider::OpenAiCompatible, Some(server))
            }
            Step::NtfyServer => {
                if state.ntfy.topics.is_empty() {
                    state.ntfy.topics.push(empty_topic());
                }
                state.step = Step::NtfyTopics;
                state.error = None;
                Action::None
            }
            Step::NtfyTopics => connect_ntfy(state),
            Step::NtfyLogin if state.ntfy.login.is_ready() => sign_in_ntfy(state),
            _ => Action::None,
        },
        Message::OpenLoginPage => match (state.step, nextcloud::login_url(data)) {
            (Step::SigningIn, Some(url)) => Action::OpenUrl(url.to_string()),
            _ => Action::None,
        },
        Message::UseGithubCli if state.step == Step::Github => {
            state.back = Step::Github;
            state.step = Step::Checking;
            state.error = None;
            state.github.use_cli()
        }
        Message::PasteToken if state.step == Step::Github => {
            state.step = Step::Token;
            state.error = None;
            Action::None
        }
        Message::TokenChanged(token) => {
            state.github.token = token;
            Action::None
        }
        Message::OpenTokenPage => Action::OpenUrl(github::NEW_TOKEN_URL.to_string()),
        Message::UseTorroMail(id) if state.step == Step::TorroMail && state.torromail.can_use(&id) => {
            state.torromail.importing = Some(id.clone());
            state.torromail.failed = None;
            state.error = None;
            Action::Send(Request::ImportTorroMailAccount { id })
        }
        Message::EnterTorroMailPassword(id) if state.step == Step::TorroMail && state.torromail.importing.is_none() => {
            if let Some(account) = state.torromail.accounts.iter().find(|account| account.id == id).cloned() {
                fill_from_torromail(state, &account);
            }
            Action::None
        }
        Message::UseTorroMail(_) | Message::EnterTorroMailPassword(_) => Action::None,
        Message::NtfyServerChanged(server) => {
            state.ntfy.server = server;
            Action::None
        }
        Message::NtfyTopicChanged(row, name) => {
            if let Some(topic) = state.ntfy.topics.get_mut(row) {
                topic.topic = name.as_str().to_string();
            }
            Action::None
        }
        Message::NtfyLabelChanged(row, label) => {
            if let Some(topic) = state.ntfy.topics.get_mut(row) {
                topic.label = label;
            }
            Action::None
        }
        Message::AddNtfyTopic if state.ntfy.topics.len() < TOPICS_MAX => {
            state.ntfy.topics.push(empty_topic());
            Action::None
        }
        // The last row stays: the step always has a place to type a topic.
        Message::RemoveNtfyTopic(row) if row < state.ntfy.topics.len() && state.ntfy.topics.len() > 1 => {
            state.ntfy.topics.remove(row);
            Action::None
        }
        Message::AddNtfyTopic | Message::RemoveNtfyTopic(_) => Action::None,
        Message::NtfyLoginKind(token) => {
            state.ntfy.login.token = token;
            Action::None
        }
        Message::NtfyUserChanged(user) => {
            state.ntfy.login.user = user;
            Action::None
        }
        Message::NtfySecretChanged(secret) => {
            state.ntfy.login.secret = secret;
            Action::None
        }
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

/// A row of the topics step with nothing typed yet.
fn empty_topic() -> NtfyTopic {
    NtfyTopic { topic: String::new(), label: String::new() }
}

/// "Connect" on the topics: from now on the step shows what is wrong; once nothing is, sends
/// `ConnectNtfy` with the rows that hold something and waits.
fn connect_ntfy(state: &mut State) -> Action {
    state.ntfy.tried = true;
    if state.ntfy.problem().is_some() {
        return Action::None;
    }
    state.back = Step::NtfyTopics;
    state.step = Step::NtfyChecking;
    state.error = None;
    Action::Send(Request::ConnectNtfy { server: state.ntfy.server.trim().to_string(), topics: state.ntfy.filled() })
}

/// "Connect" on the login: sends `SetNtfyLogin` with the topics and the login and waits. The
/// dialog keeps no copy of the secret; a refusal asks for it again.
fn sign_in_ntfy(state: &mut State) -> Action {
    let (user, secret) = state.ntfy.login.take();
    state.back = Step::NtfyLogin;
    state.step = Step::NtfyChecking;
    state.error = None;
    Action::Send(Request::SetNtfyLogin {
        server: state.ntfy.server.trim().to_string(),
        topics: state.ntfy.filled(),
        user,
        secret,
    })
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
        Step::MailCertificate => mail.certificates.asking().is_some(),
        _ => false,
    }
}

/// Continue on a step of the mail path: search first, then walk [`MAIL_STEPS`]; at the end send
/// `ConnectMail`, or `ConnectCalDav` when only calendars and tasks are wanted. On the question
/// about a certificate it trusts that certificate and sends `ConnectMail` again with every
/// certificate trusted so far (task M11.4).
fn continue_mail(state: &mut State, step: Step) -> Action {
    if !mail_can_continue(state, step) {
        return Action::None;
    }
    if step == Step::MailCertificate {
        state.mail.certificates.trust();
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

/// The request for what the last step chose. `ConnectMail` gets a copy of the password, which
/// [`answered`] clears with every answer but the question about a certificate, which needs it again
/// (task M11.4); `ConnectCalDav` takes it, so the dialog keeps no copy. A refusal asks for it again.
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
            password: mail.password.clone(),
            calendars,
            tasks,
            // The CalDAV address only goes with the services that use it.
            caldav: mail.caldav_address().filter(|_| calendars || tasks).map(str::to_string),
            trusted_certificates: mail.certificates.trusted(),
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

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    // Opened by "Import from TorroMail…", the dialog is an import all the way, also on the mail
    // path behind "Enter password".
    let title = if state.origin == Step::TorroMail { tr("Import from TorroMail") } else { tr("Add account") };
    let header =
        column![typo::text(typo::title(), title), typo::text(typo::body(), subtitle(state)).style(theme::text_2)]
            .spacing(space::xs());
    let body = match state.step {
        Step::Provider => provider_step(state),
        Step::TorroMail => torromail_step(state),
        Step::Server => server_step(state),
        Step::SigningIn => signing_in_step(data),
        Step::Github => github_step(),
        Step::Token => token_step(state),
        Step::Checking => typo::text(typo::body(), tr("Checking the connection…")).style(theme::text_2).into(),
        Step::MailAddress | Step::MailSearching => mail_address_step(state),
        Step::MailPassword => mail_password_step(state),
        Step::MailServices => mail_services_step(state),
        // After "Trust certificate" the question stays while the request is out (no jump).
        Step::MailConnecting | Step::MailCertificate => match state.mail.certificates.shown() {
            Some(asked) => certificate_question(asked),
            None if state.step == Step::MailConnecting => {
                typo::text(typo::body(), tr("Signing in…")).style(theme::text_2).into()
            }
            None => Space::new().into(),
        },
        Step::AiChoose => ai_step(state),
        Step::AiKey => ai_key_step(state),
        Step::AiServer => ai_server_step(state),
        Step::AiConnecting | Step::NtfyChecking => {
            typo::text(typo::body(), tr("Checking the connection…")).style(theme::text_2).into()
        }
        Step::NtfyServer => ntfy_server_step(state),
        Step::NtfyTopics => ntfy_topics_step(state),
        Step::NtfyLogin => ntfy_login_step(state),
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
        (Step::TorroMail, _) => tr("Choose the mail accounts to take over from TorroMail."),
        (Step::Server, _) => tr("Enter the address of your Nextcloud."),
        (Step::SigningIn, _) => tr("Sign in with Nextcloud in your browser."),
        (Step::Github, _) => tr("Choose how Torro Workplace signs in to GitHub."),
        (Step::Token, _) => tr("Paste a personal access token from GitHub."),
        (Step::Checking, _) => tr("Checking the connection with GitHub."),
        (Step::MailAddress, _) => tr("Enter the email address of your mailbox."),
        (Step::MailSearching, _) => tr("Looking for your provider."),
        (Step::MailPassword, _) => tr("Enter the password of your mailbox."),
        (Step::MailServices, _) => tr("Choose what to use from this account."),
        (Step::MailConnecting, _) if state.mail.certificates.pending().is_some() => certificate_title(),
        (Step::MailConnecting, _) => tr("Signing in to your provider."),
        (Step::MailCertificate, _) => certificate_title(),
        (Step::AiChoose, _) => tr("Choose where your AI models run."),
        (Step::AiKey, _) if cloud(state) == AiProvider::OpenRouter => tr("Paste an API key from OpenRouter."),
        (Step::AiKey, _) => tr("Paste an API key from the provider."),
        (Step::AiServer, _) => tr("Enter the address of your AI server."),
        (Step::AiConnecting, _) => tr("Checking the connection with the AI server."),
        (Step::NtfyServer, _) => tr("Enter the address of your ntfy server."),
        (Step::NtfyTopics, _) => tr("Choose the topics to show in the Notification Hub."),
        (Step::NtfyLogin, _) => tr("This server needs a login for these topics."),
        (Step::NtfyChecking, _) => tr("Checking the topics with the ntfy server."),
        (Step::Done, Provider::Ntfy) => tr("ntfy is connected."),
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
        (Provider::Ntfy, _) => tr("Your topics are subscribed.").to_string(),
        _ => tr("Your calendars and task lists are ready.").to_string(),
    }
}

/// The catalog: a group with one row per provider. No page shows it any more (task S.30); it is
/// the state [`State::at`] starts from.
fn provider_step(state: &State) -> Element<'_, Message> {
    let open = state.torromail.importing.is_none();
    let rows = vec![
        provider_row(Provider::Nextcloud, "Nextcloud", tr("Calendars and tasks"), open),
        provider_row(Provider::Mail, tr("Mail account"), tr("Mail, calendars and tasks"), open),
        provider_row(Provider::GitHub, "GitHub", tr("Issues and projects"), open),
        provider_row(Provider::Ntfy, "ntfy", tr("Notifications from your servers and scripts"), open),
        provider_row(Provider::Ai, tr("AI providers"), tr("Summaries and agents"), open),
    ];
    widgets::group(tr("Services"), rows, None)
}

/// A provider: name and what it brings, a row to press with a chevron (drawn disabled unless
/// `open`).
fn provider_row(
    provider: Provider,
    name: &'static str,
    summary: &'static str,
    open: bool,
) -> Element<'static, Message> {
    let row = setting_row(None, name, Some(summary.to_string()), chevron());
    pressable(row, open.then_some(Message::ChooseProvider(provider)))
}

/// The step TorroMail (task S.30): TorroMail's accounts to take over ([`offer_group`]), or while
/// the list is on its way, or when TorroMail has none, a sentence saying so.
fn torromail_step(state: &State) -> Element<'_, Message> {
    if let Some(group) = offer_group(&state.torromail) {
        return group;
    }
    let sentence = match state.torromail.asking {
        true => tr("Looking for TorroMail's accounts…"),
        false => tr("TorroMail has no mail accounts to take over."),
    };
    typo::text(typo::body(), sentence).style(theme::text_2).into()
}

/// TorroMail's accounts (task M7.4): one row per account of the separate TorroMail app; `None`
/// when it has none.
fn offer_group(offer: &Offer) -> Option<Element<'_, Message>> {
    if offer.accounts.is_empty() {
        return None;
    }
    let rows = offer.accounts.iter().map(|account| offer_row(offer, account)).collect();
    let footer = tr("Copies the mail servers, user name and password. TorroMail keeps its accounts.");
    Some(widgets::group(tr("Accounts in TorroMail"), rows, Some(footer.to_string())))
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
    github::ways(tr("Sign in with"), Message::UseGithubCli, Message::PasteToken)
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
    nextcloud::address(&state.nextcloud, Message::ServerChanged, Message::Continue)
}

/// What the token needs, the way to GitHub's token page, and the masked field.
fn token_step(state: &State) -> Element<'_, Message> {
    github::token_form(
        &state.github,
        |token| Message::TokenChanged(Secret::new(token)),
        Message::Continue,
        Message::OpenTokenPage,
    )
}

/// The explanation, the waiting line, the login link to copy and a button to open it again.
fn signing_in_step(data: &Data) -> Element<'_, Message> {
    nextcloud::waiting(data, Message::OpenLoginPage, |_| Message::LoginLinkEdited)
}

// ----- AI providers (task KI.1a) --------------------------------------------------------------------

/// The AI providers, Open First: the servers on this computer, each found one with "Connect", then
/// OpenRouter and any other server, each a row that opens its form.
fn ai_step(state: &State) -> Element<'_, Message> {
    let local = AiProvider::LOCAL.into_iter().map(|provider| local_ai_row(&state.ai, provider)).collect();
    let footer = tr("Models on this computer run here. Nothing you ask them leaves it.");
    let way = |provider, name: &'static str, summary: &'static str| {
        pressable(setting_row(None, name, Some(summary.to_string()), chevron()), Some(Message::ChooseAi(provider)))
    };
    let mut others: Vec<Element<'_, Message>> = AiProvider::CLOUD
        .into_iter()
        .map(|provider| {
            let summary = match provider {
                AiProvider::OpenRouter => tr("Open models from many makers, with one API key"),
                _ => provider.seat().label(),
            };
            way(provider, provider.name(), summary)
        })
        .collect();
    others.push(way(
        AiProvider::OpenAiCompatible,
        tr("Other OpenAI-compatible server"),
        tr("vLLM, llama.cpp or a server of your own"),
    ));
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
        // Strata is found by its `/health`, with no model loaded it lists none.
        0 if server.provider == AiProvider::Strata => trf("{host} · no model loaded", &[("host", &host)]),
        1 => trf("{host} · 1 model", &[("host", &host)]),
        n => trf("{host} · {n} models", &[("host", &host), ("n", &n)]),
    }
}

/// The cloud provider whose key the key step asks for.
fn cloud(state: &State) -> AiProvider {
    state.ai.cloud.unwrap_or(AiProvider::OpenRouter)
}

/// A cloud provider (OpenRouter and those of task W.3): what it is and where it runs, the way to its
/// key page, the masked key field and, where the provider has one, its platform for mainland China.
fn ai_key_step(state: &State) -> Element<'_, Message> {
    let provider = cloud(state);
    let create = widgets::text_button(
        Some(Icon::ArrowUpRight),
        trf("Create a key on {provider}", &[("provider", &provider.name())]),
        style::button::secondary,
        provider.key_url().map(|_| Message::OpenAiKeyPage),
    );
    // How the provider's keys begin, so a pasted one is recognisable; no text for people.
    let placeholder = match provider {
        AiProvider::OpenRouter => "sk-or-v1-…",
        AiProvider::Anthropic => "sk-ant-…",
        _ => "",
    };
    let input = field(placeholder, state.ai.key.expose())
        .secure(true)
        .on_input(|key| Message::AiKeyChanged(Secret::new(key)))
        .on_submit_maybe(ai_can_connect(state).then_some(Message::Continue));
    let about = match provider {
        AiProvider::OpenRouter => tr("OpenRouter offers open models such as Qwen, Llama and Mistral through one API key. Torro Workplace lists the models with open weights.").to_string(),
        _ => trf(
            "{provider} runs its models in its own cloud ({seat}). What you send them goes there.",
            &[("provider", &provider.name()), ("seat", &provider.seat().label())],
        ),
    };
    let mut parts = column![
        typo::text(typo::body(), about).style(theme::text_2),
        row![create],
        field_group(tr("API key"), input, Some(tr("The key stays in the system keychain. Agents never see it."))),
    ]
    .spacing(space::m());
    if provider.china_url().is_some() {
        let switch = toggler(state.ai.china).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::AiChina);
        let note = tr("Keys of the platform for mainland China only work there, and the other way round.");
        parts =
            parts.push(setting_row(None, tr("Use the platform for mainland China"), Some(note.to_string()), switch));
    }
    parts.into()
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

// ----- ntfy (task E5.14) -----------------------------------------------------------------------------

/// The server's address; empty is the public server, as the hint under the field says.
fn ntfy_server_step(state: &State) -> Element<'_, Message> {
    let input = field("ntfy.sh", &state.ntfy.server).on_input(Message::NtfyServerChanged).on_submit(Message::Continue);
    field_group(tr("Server"), input, Some(tr("Leave empty for the public server ntfy.sh.")))
}

/// A row per topic – its name and the label the inbox shows, and ✕ while there is more than one –,
/// "Add topic" under them and, after the first "Connect", what keeps them from being sent.
fn ntfy_topics_step(state: &State) -> Element<'_, Message> {
    let ntfy = &state.ntfy;
    let many = ntfy.topics.len() > 1;
    let rows = ntfy
        .topics
        .iter()
        .enumerate()
        .map(|(at, topic)| {
            let name = field(tr("Topic name"), &topic.topic)
                .on_input(move |name| Message::NtfyTopicChanged(at, TopicName::new(name)))
                .on_submit(Message::Continue);
            let label = field(tr("Label"), &topic.label)
                .on_input(move |label| Message::NtfyLabelChanged(at, label))
                .on_submit(Message::Continue);
            let remove =
                widgets::icon_button(Icon::X, tr("Remove topic"), many.then_some(Message::RemoveNtfyTopic(at)));
            row_frame(row![name, label, remove].spacing(space::s()).align_y(Alignment::Center), false)
        })
        .collect();
    let footer = tr("The Notification Hub shows the label. On a public server a topic name works like a password.");
    let add = widgets::text_button(
        Some(Icon::Plus),
        tr("Add topic"),
        style::button::ghost,
        (ntfy.topics.len() < TOPICS_MAX).then_some(Message::AddNtfyTopic),
    );
    let mut page =
        column![widgets::group(tr("Topics"), rows, Some(footer.to_string())), on_axis(add)].spacing(space::s());
    if let Some(problem) = ntfy.problem().filter(|_| ntfy.tried) {
        page = page.push(error_line(problem));
    }
    page.into()
}

/// Why the login is asked for, then the login itself ([`ntfy_login_fields`]).
fn ntfy_login_step(state: &State) -> Element<'_, Message> {
    let submit = state.ntfy.login.is_ready().then_some(Message::Continue);
    let fields = ntfy_login_fields(
        &state.ntfy.login,
        Message::NtfyLoginKind,
        Message::NtfyUserChanged,
        Message::NtfySecretChanged,
        submit,
    );
    let why = tr("Sign in with a user name and password, or with an access token from the server's account page.");
    column![typo::text(typo::body(), why).style(theme::text_2), fields].spacing(layout::section_gap()).into()
}

/// The login of an ntfy server, for the add-account dialog and "Change login" on the account's
/// page: "User and password" or "Access token" as a segmented control, the user name for a
/// password, and the masked secret with where it stays. `kind`, `user` and `secret` make the
/// messages of the three controls; `submit` is what Enter in a field sends (`None`: nothing yet).
pub(super) fn ntfy_login_fields<'a, M: Clone + 'a>(
    login: &'a NtfyLogin,
    kind: fn(bool) -> M,
    user: fn(String) -> M,
    secret: fn(Secret) -> M,
    submit: Option<M>,
) -> Element<'a, M> {
    let ways = widgets::segmented(
        [(tr("User and password"), kind(false)), (tr("Access token"), kind(true))],
        usize::from(login.token),
    );
    let mut page = column![row![ways]].spacing(layout::section_gap());
    if !login.token {
        let name = field("anna", &login.user).on_input(user).on_submit_maybe(submit.clone());
        page = page.push(widgets::group(tr("User name"), vec![row_frame(name, false)], None));
    }
    let (title, placeholder) = if login.token { (tr("Access token"), "tk_…") } else { (tr("Password"), "") };
    let masked = field(placeholder, login.secret.expose())
        .secure(true)
        .on_input(move |text| secret(Secret::new(text)))
        .on_submit_maybe(submit);
    let footer = tr("It stays in the system keychain. Agents never see it.");
    page.push(widgets::group(title, vec![row_frame(masked, false)], Some(footer.to_string()))).into()
}

/// A sentence in the error colour under a form, as wide as there is room.
fn error_line<'a, M: 'a>(text: String) -> Element<'a, M> {
    typo::text(typo::body(), text).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }).into()
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

/// Whether the footer shows "Back": on the steps a person walks back from, but never on the step
/// the dialog opened at (task S.30), which has nothing behind it.
fn has_back(state: &State) -> bool {
    let walks_back = matches!(
        state.step,
        Step::Github
            | Step::Token
            | Step::MailAddress
            | Step::MailPassword
            | Step::MailServices
            | Step::MailCertificate
            | Step::AiChoose
            | Step::AiKey
            | Step::AiServer
            | Step::NtfyServer
            | Step::NtfyTopics
            | Step::NtfyLogin
    );
    walks_back && state.step != state.origin
}

/// Error text left, "Back" where there is a step to go back to ([`has_back`]), "Cancel" and the
/// step's primary button right.
fn footer(state: &State) -> Element<'_, Message> {
    let error: Element<'_, Message> = match &state.error {
        Some(error) => typo::text(typo::body(), error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        None => blank::horizontal().into(),
    };
    // While a trusted certificate's request is out, "Back" stays in its place, but off.
    let pending = state.step == Step::MailConnecting && state.mail.certificates.pending().is_some();
    let back = (has_back(state) || pending).then(|| {
        let back = (!pending).then_some(Message::Back);
        widgets::text_button(None, tr("Back"), style::button::secondary, back)
    });
    // A TorroMail account taken over on its step is done, not cancelled.
    let taken_over = state.step == Step::TorroMail && state.torromail.imported;
    let cancel =
        (!taken_over).then(|| widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Close)));
    let primary = |label, message| Some(widgets::text_button(None, label, style::button::primary, message));
    let next = match state.step {
        Step::TorroMail if taken_over => primary(tr("Done"), Some(Message::Close)),
        Step::MailConnecting => state.mail.certificates.pending().map(|asked| trust_button(asked, true, None)),
        Step::Provider
        | Step::TorroMail
        | Step::SigningIn
        | Step::Github
        | Step::Checking
        | Step::MailSearching
        | Step::AiChoose
        | Step::AiConnecting
        | Step::NtfyChecking => None,
        Step::NtfyServer => primary(tr("Continue"), Some(Message::Continue)),
        // Stays on, so a press shows what is wrong (the design's rule for forms).
        Step::NtfyTopics => primary(tr("Connect"), Some(Message::Continue)),
        Step::NtfyLogin => primary(tr("Connect"), state.ntfy.login.is_ready().then_some(Message::Continue)),
        Step::AiKey | Step::AiServer => primary(tr("Connect"), ai_can_connect(state).then_some(Message::Continue)),
        Step::Server => primary(tr("Continue"), state.nextcloud.can_start().then_some(Message::Continue)),
        Step::MailAddress => {
            // The first press searches, the second goes on with what was found.
            let label = if state.mail.found.is_none() { tr("Find provider") } else { tr("Continue") };
            primary(label, mail_can_continue(state, state.step).then_some(Message::Continue))
        }
        Step::MailPassword => {
            primary(tr("Continue"), mail_can_continue(state, state.step).then_some(Message::Continue))
        }
        Step::MailServices => primary(tr("Connect"), mail_can_continue(state, state.step).then_some(Message::Continue)),
        Step::MailCertificate => {
            state.mail.certificates.asking().map(|asked| {
                trust_button(asked, false, mail_can_continue(state, state.step).then_some(Message::Continue))
            })
        }
        Step::Token => primary(tr("Continue"), state.github.has_token().then_some(Message::Continue)),
        Step::Done => primary(tr("Done"), Some(Message::Close)),
    };
    row![error].push(back).push(cancel).push(next).spacing(space::s()).align_y(Alignment::Center).into()
}

// ----- A certificate the system does not trust (task M11.4, issue #284) ---------------------------

/// A server certificate the system does not trust, as `Response::UntrustedCertificate` named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Untrusted {
    /// Which server: `IMAP`, `SMTP` or `CalDAV` (the same in every language).
    pub(super) service: String,
    /// The server's host as the request named it.
    pub(super) host: String,
    /// SHA-256 of the certificate, `AB:CD:…`.
    pub(super) sha256: String,
    /// An address of the host is on the public internet: the question warns clearly.
    pub(super) public: bool,
}

/// The certificates of one dialog that sends `ConnectMail` (the add-account dialog's mail path,
/// "Server settings", "Change password"): the ones the person trusted in it, the one it asks
/// about now, and the one just trusted while that request is on its way. The core stores a
/// trusted certificate only once the whole sign-in succeeded, so when a second server asks too
/// (IMAP, then CalDAV with another certificate) every `ConnectMail` of the dialog has to carry all
/// of them ([`Certificates::trusted`]), not only the last.
///
/// The question stays on screen while the request it led to is out ([`Certificates::shown`],
/// [`Certificates::pending`]): the dialog does not jump back to its form and its width for the
/// moment of signing in, and a second question takes the place of the first one in place.
///
/// The password rule that goes with it lives in each dialog: it keeps the password only while
/// [`Certificates::asking`] is `Some`; any other answer, "Back" ([`Certificates::dismiss`]) or
/// closing clears it.
#[derive(Debug, Default)]
pub(super) struct Certificates {
    /// Fingerprints trusted in this dialog, in the order they were asked about, each once.
    trusted: Vec<String>,
    /// The certificate the dialog asks about now; `None` while it asks nothing.
    asking: Option<Untrusted>,
    /// The certificate just trusted ([`Certificates::trust`]) while the request that carries it
    /// waits for its answer; the next answer ([`Certificates::ask`]) ends it.
    pending: Option<Untrusted>,
}

impl Certificates {
    /// Takes the answer `result` to the dialog's request: it ends a [`Certificates::pending`]
    /// question whatever it is. When it is `Response::UntrustedCertificate` the dialog asks about
    /// that certificate from now on, and `true` comes back; every other answer gives `false`.
    /// Each dialog hands it every answer to its `ConnectMail`.
    pub(super) fn ask(&mut self, result: &Result<Response, String>) -> bool {
        self.pending = None;
        let Ok(Response::UntrustedCertificate { service, host, sha256, public }) = result else { return false };
        self.asking =
            Some(Untrusted { service: service.clone(), host: host.clone(), sha256: sha256.clone(), public: *public });
        true
    }

    /// The certificate the dialog asks about now.
    pub(super) fn asking(&self) -> Option<&Untrusted> {
        self.asking.as_ref()
    }

    /// The certificate just trusted, while the request that carries it is on its way: the dialog
    /// shows the question with "Signing in…" in place of "Trust certificate".
    pub(super) fn pending(&self) -> Option<&Untrusted> {
        self.pending.as_ref()
    }

    /// The question the dialog shows in place of its form: the one it asks, else the one whose
    /// answer is on its way. `None` shows the form.
    pub(super) fn shown(&self) -> Option<&Untrusted> {
        self.asking.as_ref().or(self.pending.as_ref())
    }

    /// "Trust certificate": the fingerprint asked about joins the trusted ones (once) and the
    /// question waits for the answer to the request the caller sends now
    /// ([`Certificates::pending`]). `false` when nothing was asked.
    pub(super) fn trust(&mut self) -> bool {
        let Some(asked) = self.asking.take() else { return false };
        if !self.trusted.contains(&asked.sha256) {
            self.trusted.push(asked.sha256.clone());
        }
        self.pending = Some(asked);
        true
    }

    /// "Back" from the question: it ends without trusting anything.
    pub(super) fn dismiss(&mut self) {
        self.asking = None;
        self.pending = None;
    }

    /// Every fingerprint trusted in this dialog, for `ConnectMail::trusted_certificates`.
    pub(super) fn trusted(&self) -> Vec<String> {
        self.trusted.clone()
    }
}

/// The title of the question, in the add-account dialog its subtitle.
pub(super) fn certificate_title() -> &'static str {
    tr("Trust this certificate?")
}

/// The question about `asked`: for a host on the public internet the warning first (its icon in
/// `attention`, the colour only there, on the warning's first line), then which server's
/// certificate is not trusted – both one block –, and a section below the fingerprint in a group,
/// so it stands right above the dialog's foot.
pub(super) fn certificate_question<'a, M: 'a>(asked: &Untrusted) -> Element<'a, M> {
    let sentence = trf(
        "The certificate of {host} ({service}) is not trusted by this computer. Trust it only if this is your own server.",
        &[("host", &asked.host), ("service", &asked.service)],
    );
    let mut intro = column![].spacing(layout::item_gap());
    if let Some(warning) = certificate_warning(asked) {
        let shield: Element<'a, M> = icon::icon(Icon::ShieldAlert, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention) })
            .into();
        // As high as one line of the text, so the icon stands on its first line however many
        // lines the warning wraps into.
        let shield: Element<'a, M> = container(shield).center_y(typo::line_height(typo::body())).into();
        intro = intro.push(
            row![shield, typo::text(typo::body(), warning).width(Fill)].spacing(space::s()).align_y(Alignment::Start),
        );
    }
    intro = intro.push(typo::text(typo::body(), sentence).style(theme::text_2));
    column![intro, widgets::group(tr("Fingerprint (SHA-256)"), vec![fingerprint_row(&asked.sha256)], None)]
        .spacing(layout::section_gap())
        .into()
}

/// The warning over the question: only for a host on the public internet, where an untrusted
/// certificate more likely means someone in the middle than a server of one's own.
fn certificate_warning(asked: &Untrusted) -> Option<&'static str> {
    asked.public.then(|| tr("This server is on the public internet. An untrusted certificate there can mean that someone is intercepting the connection. Never trust it for a mail provider."))
}

/// The style of "Trust certificate" for `asked`. For a host on the public internet it is not the
/// red primary button, which would invite the press the warning advises against, but
/// `danger` (red text, no surface, as "Open anyway" of a suspicious link): the window then has
/// no primary button at all. A server at home keeps `primary`, since trusting is the way on.
pub(super) fn trust_style(asked: &Untrusted) -> widgets::ButtonStyle {
    if asked.public { style::button::danger } else { style::button::primary }
}

/// The last button of a dialog's foot on the question about `asked`: "Trust certificate", which
/// sends `trust`, or, while the request it sent is out (`sending`, [`Certificates::pending`]),
/// "Signing in…" in the same style, which takes no second press ([`widgets::busy_button`]).
pub(super) fn trust_button<'a, M: Clone + 'a>(asked: &Untrusted, sending: bool, trust: Option<M>) -> Element<'a, M> {
    match sending {
        true => widgets::busy_button(tr("Signing in…"), trust_style(asked)),
        false => widgets::text_button(None, tr("Trust certificate"), trust_style(asked), trust),
    }
}

/// The fingerprint `sha256` as one row of a group, as the question and an account's page show
/// it ([`fingerprint_lines`], in [`typo::body`] with tabular figures, in `text`).
pub(super) fn fingerprint_row<'a, M: 'a>(sha256: &str) -> Element<'a, M> {
    // Where a window is narrower than a line, it breaks between characters rather than cut.
    let text = typo::text(typo::body().tabular(), fingerprint_lines(sha256))
        .style(theme::text)
        .wrapping(Wrapping::WordOrGlyph);
    container(text).width(Fill).padding([space::m(), space::l()]).into()
}

/// `AB:CD:…` in two fixed lines of sixteen bytes each, so the 32 bytes always stand as the same
/// two lines that a person compares with what the server's admin reads out; tabular figures
/// ([`fingerprint_row`]) keep the bytes of both lines in columns.
fn fingerprint_lines(sha256: &str) -> String {
    let bytes: Vec<&str> = sha256.split(':').collect();
    bytes.chunks(16).map(|line| line.join(":")).collect::<Vec<_>>().join("\n")
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
    /// Servers typed for a server at home (`mail.home.lan`), whose certificate the system does
    /// not trust (task M11.4): the question without the warning.
    Certificate,
    /// The same for a host on the public internet (`mail.berg.example`): the question with the
    /// warning.
    CertificatePublic,
    /// The step TorroMail (task S.30): the demo's four TorroMail accounts, one of each kind.
    TorroMail,
    /// The step TorroMail after "Use" on TorroMail's Fastmail account, whose password could not be
    /// read (task M7.4): the reason and "Enter password".
    TorroMailFailed,
    /// "Enter password" pressed there: the password step, filled with TorroMail's servers.
    TorroMailPassword,
    /// The step TorroMail after TorroMail's Posteo account was taken over: "Connected" and "Done".
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
    /// ntfy (task E5.14): the server step, nothing typed.
    NtfyServer,
    /// The topics step with two topics of `push.example.org` typed.
    NtfyTopics,
    /// The same after "Connect" with a wrong name and a missing label: what is wrong shows.
    NtfyInvalid,
    /// The demo's server wants a login for the topics: the login step, a user name typed.
    NtfyLogin,
    /// Signed in: "Your topics are subscribed."
    NtfyDone,
}

/// Where a dialog opens (task S.30): at a provider's first step ([`State::at`]) or at TorroMail's
/// accounts ([`State::torromail`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    At(Provider),
    TorroMail,
}

impl Demo {
    /// Where the dialog of this demo state opens; [`demo_messages`] goes on from there.
    pub fn start(self) -> Start {
        match self {
            Demo::TorroMail | Demo::TorroMailFailed | Demo::TorroMailPassword | Demo::TorroMailDone => Start::TorroMail,
            Demo::AiProviders | Demo::AiOpenRouter | Demo::AiRefused | Demo::AiServer | Demo::AiDone => {
                Start::At(Provider::Ai)
            }
            Demo::NtfyServer | Demo::NtfyTopics | Demo::NtfyInvalid | Demo::NtfyLogin | Demo::NtfyDone => {
                Start::At(Provider::Ntfy)
            }
            _ => Start::At(Provider::Mail),
        }
    }
}

/// The messages that lead to `demo` the way a person would get there, once the dialog is open where
/// [`Demo::start`] says. The demo's answers to the requests come from `launch::demo_answer`; the
/// passwords "wrong" (IMAP) and "wrong-smtp" are the ones it refuses, and an IMAP host on `.lan` or
/// `.example` shows a certificate it does not trust until one is trusted.
pub fn demo_messages(demo: Demo) -> Vec<Message> {
    // TorroMail's accounts are on its step once the dialog is open (`launch::demo_torromail`).
    match demo {
        Demo::TorroMail => return Vec::new(),
        Demo::TorroMailFailed => return vec![Message::UseTorroMail("tm-fastmail".into())],
        Demo::TorroMailPassword => {
            let id = "tm-fastmail".to_string();
            return vec![Message::UseTorroMail(id.clone()), Message::EnterTorroMailPassword(id)];
        }
        Demo::TorroMailDone => return vec![Message::UseTorroMail("tm-posteo".into())],
        _ => {}
    }
    if let Some(steps) = ntfy_demo(demo) {
        return steps;
    }
    // The AI providers search this computer when they open (`launch::demo_answer` finds Ollama).
    let openrouter = Message::ChooseAi(AiProvider::OpenRouter);
    match demo {
        Demo::AiProviders => return Vec::new(),
        Demo::AiOpenRouter => return vec![openrouter],
        // "wrong" is the key the demo refuses.
        Demo::AiRefused => return vec![openrouter, Message::AiKeyChanged(Secret::new("wrong")), Message::Continue],
        Demo::AiServer => {
            let address = Message::AiServerChanged("http://gpu.local:8000/v1".into());
            return vec![Message::ChooseAi(AiProvider::OpenAiCompatible), address];
        }
        Demo::AiDone => {
            let ollama = AiProvider::Ollama.default_url().unwrap_or_default().to_string();
            return vec![Message::ConnectLocalAi(AiProvider::Ollama, ollama)];
        }
        _ => {}
    }
    let address = match demo {
        Demo::TorroMail | Demo::TorroMailFailed | Demo::TorroMailPassword | Demo::TorroMailDone => None,
        Demo::AiProviders | Demo::AiOpenRouter | Demo::AiRefused | Demo::AiServer | Demo::AiDone => None,
        Demo::NtfyServer | Demo::NtfyTopics | Demo::NtfyInvalid | Demo::NtfyLogin | Demo::NtfyDone => None,
        Demo::Address => None,
        Demo::Unsupported => Some("anna@outlook.com"),
        Demo::Gmail => Some("anna@gmail.com"),
        Demo::NotFound => Some("anna@example.org"),
        Demo::Password => Some("anna@icloud.com"),
        Demo::MailOnly => Some("anna@work.example.net"),
        // Nothing is found for these domains: the servers are typed under "Advanced".
        Demo::Certificate => Some("anna@home.lan"),
        Demo::CertificatePublic => Some("anna@berg.example"),
        Demo::Found | Demo::Advanced | Demo::Refused | Demo::RefusedSmtp | Demo::Services | Demo::Done => {
            Some("anna@mailbox.org")
        }
    };
    let mut steps = Vec::new();
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
        Demo::Certificate | Demo::CertificatePublic => {
            let host = if demo == Demo::Certificate { "mail.home.lan" } else { "mail.berg.example" };
            for server in [Server::Imap, Server::Smtp] {
                steps.push(Message::ServerHostChanged(server, host.to_string()));
            }
            steps.extend([Message::Continue, password("demo-password"), Message::Continue, Message::Continue]);
        }
        Demo::Address | Demo::Found | Demo::Unsupported | Demo::Gmail | Demo::NotFound => {}
        // Returned above.
        Demo::TorroMail | Demo::TorroMailFailed | Demo::TorroMailPassword | Demo::TorroMailDone => {}
        Demo::AiProviders | Demo::AiOpenRouter | Demo::AiRefused | Demo::AiServer | Demo::AiDone => {}
        Demo::NtfyServer | Demo::NtfyTopics | Demo::NtfyInvalid | Demo::NtfyLogin | Demo::NtfyDone => {}
    }
    steps
}

/// The script of an ntfy demo state; `None` for the other paths. `launch::demo_answer` refuses the
/// topics of `push.example.org` without a login and takes any login but the password "wrong".
fn ntfy_demo(demo: Demo) -> Option<Vec<Message>> {
    let topic = |at, name: &str, label: &str| {
        [Message::NtfyTopicChanged(at, TopicName::new(name)), Message::NtfyLabelChanged(at, label.to_string())]
    };
    let mut steps = Vec::new();
    if demo == Demo::NtfyServer {
        return Some(steps);
    }
    if !matches!(demo, Demo::NtfyTopics | Demo::NtfyInvalid | Demo::NtfyLogin | Demo::NtfyDone) {
        return None;
    }
    steps.extend([Message::NtfyServerChanged("push.example.org".into()), Message::Continue]);
    steps.extend(topic(0, "backups-7f3a", "Backups"));
    steps.push(Message::AddNtfyTopic);
    match demo {
        Demo::NtfyInvalid => {
            steps.extend(topic(1, "ci builds", ""));
            steps.push(Message::Continue);
        }
        _ => steps.extend(topic(1, "ci-anna-91c2", "CI builds")),
    }
    if matches!(demo, Demo::NtfyLogin | Demo::NtfyDone) {
        steps.extend([Message::Continue, Message::NtfyUserChanged("anna".into())]);
    }
    if demo == Demo::NtfyDone {
        steps.extend([Message::NtfySecretChanged(Secret::new("demo-password")), Message::Continue]);
    }
    Some(steps)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use torro_ipc::AccountInfo;
    use torro_mail_imap::discover::MailUnsupported;
    use torro_pim::AccountState;

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
            services: None,
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

    /// A dialog at GitHub's two ways, as GitHub's "Add" opens it.
    fn at_github() -> State {
        let (state, action) = State::at(Provider::GitHub, &Data::default());
        assert!(matches!(action, Action::None));
        state
    }

    /// Task S.30: every group's "Add" opens the dialog at the first step of its provider; only the AI
    /// providers ask for something as they open (the servers on this computer). That step has no
    /// "Back", and pressing Back there changes nothing.
    #[test]
    fn every_provider_opens_at_its_first_step_without_back() {
        let data = Data::default();
        let expected = [
            (Provider::Nextcloud, Step::Server),
            (Provider::Mail, Step::MailAddress),
            (Provider::GitHub, Step::Github),
            (Provider::Ntfy, Step::NtfyServer),
            (Provider::Ai, Step::AiChoose),
        ];
        assert_eq!(expected.map(|(provider, _)| provider), Provider::ALL, "five providers, no Slack");
        for (provider, step) in expected {
            let (mut state, action) = State::at(provider, &data);
            assert_eq!((state.step, state.origin, state.provider), (step, step, provider), "{provider:?}");
            assert_eq!(
                matches!(action, Action::Send(Request::FindAiServers)),
                provider == Provider::Ai,
                "{provider:?}"
            );
            assert!(!has_back(&state), "{provider:?}: nothing to go back to");
            assert!(matches!(update(&mut state, Message::Back, &data), Action::None));
            assert_eq!(state.step, step, "{provider:?}: Back stays");
            drop(view(&state, &data));
        }
        // The catalog itself is still where a dialog is built from.
        assert_eq!((State::new().step, State::new().origin), (Step::Provider, Step::Provider));
    }

    /// Task S.30: "Back" walks between the steps of one provider, never out of it.
    #[test]
    fn back_stays_on_the_path_of_the_provider() {
        let data = Data::default();
        let (mut nextcloud, _) = State::at(Provider::Nextcloud, &data);
        update(&mut nextcloud, Message::ServerChanged("cloud.example.org".into()), &data);
        update(&mut nextcloud, Message::Back, &data);
        assert_eq!(nextcloud.step, Step::Server, "the address is the first step");
        let mut github = at_github();
        update(&mut github, Message::PasteToken, &data);
        assert!(has_back(&github));
        update(&mut github, Message::Back, &data);
        assert_eq!(github.step, Step::Github);
        assert!(!has_back(&github));
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
        assert!(!state.github.has_token(), "the dialog keeps no copy");
        assert_eq!(state.step, Step::Checking);
    }

    #[test]
    fn the_token_page_has_the_scope_preselected_and_back_leads_out() {
        let mut state = at_github();
        update(&mut state, Message::PasteToken, &Data::default());
        match update(&mut state, Message::OpenTokenPage, &Data::default()) {
            Action::OpenUrl(url) => assert_eq!(url, github::NEW_TOKEN_URL),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        assert!(github::NEW_TOKEN_URL.contains("scopes=repo"));
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::Github);
        update(&mut state, Message::Back, &Data::default());
        assert_eq!(state.step, Step::Github, "GitHub's two ways are where the dialog opened");
    }

    /// The catalog of [`State::new`] still opens every provider, and only from there: on any other
    /// step a provider is no message to react to.
    #[test]
    fn a_provider_is_chosen_only_on_the_catalog() {
        for provider in Provider::ALL {
            let mut state = at_catalog();
            update(&mut state, Message::ChooseProvider(provider), &Data::default());
            assert_eq!(state.step, provider.first_step(), "{provider:?}");
        }
        let mut state = at_github();
        update(&mut state, Message::ChooseProvider(Provider::Mail), &Data::default());
        assert_eq!(state.step, Step::Github);
    }

    #[test]
    fn empty_server_keeps_continue_disabled() {
        let empty = Data::default();
        let mut state = at_catalog();
        update(&mut state, Message::ChooseProvider(Provider::Nextcloud), &empty);
        for typed in ["", "   ", "\t"] {
            update(&mut state, Message::ServerChanged(typed.into()), &empty);
            assert!(!state.nextcloud.can_start(), "{typed:?} must not enable Continue");
            let action = update(&mut state, Message::Continue, &empty);
            assert!(matches!(action, Action::None));
            assert_eq!(state.step, Step::Server);
            assert!(!state.busy());
        }
        update(&mut state, Message::ServerChanged("cloud.example.org".into()), &empty);
        assert!(state.nextcloud.can_start());
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

    /// The dialog opened where `demo` starts ([`Demo::start`]), the way the accounts page opens it,
    /// with the request it sends as it opens answered by the demo.
    fn opened(demo: Demo, data: &Data) -> State {
        let (mut state, action) = match demo.start() {
            Start::At(provider) => State::at(provider, data),
            Start::TorroMail => State::torromail(),
        };
        if let Action::Send(request) = action
            && let Some(answer) = super::super::launch::demo_answer(&request, data)
        {
            answered(&mut state, &Ok(answer));
        }
        state
    }

    fn at_mail(demo: Demo) -> State {
        let mut state = opened(demo, &data_with_caldav());
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
                trusted_certificates,
            }) => {
                assert!(trusted_certificates.is_empty());
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
        assert!(!format!("{state:?}").contains("demo-password"));
        assert_eq!(state.step, Step::MailConnecting);
        // Task M11.4: the copy is held only until the answer, which is no question here.
        let Action::Send(request) = action else { panic!("expected a request") };
        let answer = super::super::launch::demo_answer(&request, &data_with_caldav()).expect("the demo answers");
        assert!(answered(&mut state, &Ok(answer)));
        assert_eq!(state.step, Step::Done);
        assert!(state.mail.password.is_blank(), "the dialog keeps no copy");
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
        for expected in [Step::MailPassword, Step::MailAddress, Step::MailAddress] {
            update(&mut state, Message::Back, &Data::default());
            assert_eq!(state.step, expected);
        }
        assert!(!has_back(&state), "the address is where Mail's Add opens the dialog");
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
        for (demo, public) in [(Demo::Certificate, false), (Demo::CertificatePublic, true)] {
            let state = at(demo);
            assert_eq!(state.step, Step::MailCertificate, "{demo:?}");
            let asked = state.mail.certificates.asking().expect("asked");
            assert_eq!(asked.public, public, "{demo:?}");
            assert_eq!(asked.sha256, super::super::launch::DEMO_FINGERPRINT);
        }
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
            Demo::Certificate,
            Demo::CertificatePublic,
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

    /// The step TorroMail as "Import from TorroMail…" opens it, with TorroMail's `accounts` listed.
    fn with_torromail(accounts: Vec<TorroMailAccount>) -> State {
        let (mut state, ask) = State::torromail();
        assert!(matches!(ask, Action::Send(Request::TorroMailAccounts)), "the step asks as it opens");
        assert_eq!((state.step, state.origin), (Step::TorroMail, Step::TorroMail));
        assert!(answered(&mut state, &Ok(Response::TorroMailAccounts { accounts })));
        state
    }

    /// Task S.30: Accounts offers the import only while TorroMail has an account the step could
    /// take over – not one the suite cannot sign in to, not one it has already.
    #[test]
    fn the_import_is_offered_only_with_an_account_to_take_over() {
        let mut gmail = torromail_account("g", "anna@gmail.com");
        gmail.unsupported = Some("Signs in with Google".into());
        let mut known = torromail_account("k", "anna@mailbox.org");
        known.connected = true;
        assert!(!can_import(&[]));
        assert!(!can_import(&[gmail.clone(), known.clone()]));
        assert!(can_import(&[gmail, known, torromail_account("p", "anna@posteo.de")]));
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
        assert!(!has_back(&state), "the list is where the dialog opened");
        drop(view(&state, &data));

        let done = Response::MailConnected {
            account: account("mail:anna@posteo.de", connected()),
            calendars: false,
            tasks: false,
            open_roles: Vec::new(),
        };
        assert!(answered(&mut state, &Ok(done)));
        assert!(state.torromail.accounts[0].connected && state.torromail.imported);
        assert_eq!((state.step, state.error.as_deref()), (Step::TorroMail, None));
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
        assert_eq!((state.step, state.error.as_deref()), (Step::TorroMail, None), "the row says it, not the footer");
        drop(view(&state, &data));

        update(&mut state, Message::EnterTorroMailPassword("f".into()), &data);
        assert_eq!((state.step, state.provider), (Step::MailPassword, Provider::Mail));
        // Back leads through the address to TorroMail's list, and on again.
        update(&mut state, Message::Back, &data);
        assert_eq!(state.step, Step::MailAddress);
        assert!(has_back(&state), "the dialog opened at TorroMail's list");
        update(&mut state, Message::Back, &data);
        assert_eq!(state.step, Step::TorroMail);
        update(&mut state, Message::EnterTorroMailPassword("f".into()), &data);
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

    /// The step TorroMail says what it is waiting for, that TorroMail has nothing, or why the list
    /// could not be read – the list is all the step has.
    #[test]
    fn the_torromail_step_says_why_it_lists_nothing() {
        let (mut state, _) = State::torromail();
        assert!(state.torromail.asking);
        drop(view(&state, &Data::default()));
        let broken = "TorroMail's state.json is not valid JSON";
        assert!(answered(&mut state, &Err(broken.into())));
        assert_eq!((state.step, state.error.as_deref()), (Step::TorroMail, Some(broken)));
        assert!(offer_group(&state.torromail).is_none());
        assert!(!answered(&mut state, &Err("no template".into())), "afterwards the steps take their answers");
        let empty = with_torromail(Vec::new());
        assert!(offer_group(&empty.torromail).is_none() && empty.error.is_none(), "no accounts, no list");
        drop(view(&empty, &Data::default()));
    }

    #[test]
    fn the_torromail_demos_lead_where_they_say() {
        for (demo, step) in [
            (Demo::TorroMail, Step::TorroMail),
            (Demo::TorroMailFailed, Step::TorroMail),
            (Demo::TorroMailPassword, Step::MailPassword),
            (Demo::TorroMailDone, Step::TorroMail),
        ] {
            assert_eq!(demo.start(), Start::TorroMail, "{demo:?}");
            let messages = demo_messages(demo);
            assert_eq!(messages.is_empty(), demo == Demo::TorroMail, "{demo:?}");
            assert!(messages.first().is_none_or(|first| matches!(first, Message::UseTorroMail(_))), "{demo:?}");
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
    #[test]
    fn a_cloud_provider_asks_for_its_key_and_may_use_its_china_platform() {
        let mut state = at_ai();
        drop(view(&state, &Data::default()));
        update(&mut state, Message::ChooseAi(AiProvider::Kimi), &Data::default());
        assert_eq!(state.step, Step::AiKey);
        drop(view(&state, &Data::default()));
        let page = send(&mut state, Message::OpenAiKeyPage);
        assert!(matches!(page, Action::OpenUrl(url) if Some(url.as_str()) == AiProvider::Kimi.key_url()));
        update(&mut state, Message::AiKeyChanged(Secret::new("sk-kimi")), &Data::default());
        update(&mut state, Message::AiChina(true), &Data::default());
        let Action::Send(Request::ConnectAi { provider, server, key }) = send(&mut state, Message::Continue) else {
            panic!("connect")
        };
        assert_eq!(
            (provider, server.as_deref(), key.expose()),
            (AiProvider::Kimi, Some("https://api.moonshot.cn/v1"), "sk-kimi")
        );

        let mut usual = at_ai();
        update(&mut usual, Message::ChooseAi(AiProvider::Mistral), &Data::default());
        update(&mut usual, Message::AiKeyChanged(Secret::new("key")), &Data::default());
        let Action::Send(Request::ConnectAi { server, .. }) = send(&mut usual, Message::Continue) else {
            panic!("connect")
        };
        assert_eq!(server, None, "the international address is the usual one");
    }

    /// Task KI.1d: Anthropic is a row like any other, although it speaks its own protocol.
    #[test]
    fn anthropic_asks_for_its_key_like_every_other_cloud_provider() {
        let mut state = at_ai();
        drop(view(&state, &Data::default()));
        update(&mut state, Message::ChooseAi(AiProvider::Anthropic), &Data::default());
        assert_eq!(state.step, Step::AiKey);
        drop(view(&state, &Data::default()));
        let page = send(&mut state, Message::OpenAiKeyPage);
        assert!(matches!(page, Action::OpenUrl(url) if Some(url.as_str()) == AiProvider::Anthropic.key_url()));
        update(&mut state, Message::AiKeyChanged(Secret::new("sk-ant-x")), &Data::default());
        let Action::Send(Request::ConnectAi { provider, server, key }) = send(&mut state, Message::Continue) else {
            panic!("connect")
        };
        // No address: the usual one is Anthropic's, and it has no platform of its own in China.
        assert_eq!((provider, server, key.expose()), (AiProvider::Anthropic, None, "sk-ant-x"));
        assert_eq!(AiProvider::Anthropic.china_url(), None);
    }

    /// The AI providers as AI's "Add" opens them: the search went out.
    fn at_ai() -> State {
        let (state, action) = State::at(Provider::Ai, &Data::default());
        assert!(matches!(action, Action::Send(Request::FindAiServers)), "{action:?}");
        assert_eq!(state.step, Step::AiChoose);
        state
    }

    /// Issue #322: a Strata found with no model loaded says so; Ollama with none keeps the count.
    #[test]
    fn a_strata_without_a_model_says_no_model_is_loaded() {
        let idle = |provider, url| AiServer { models: 0, ..ai_server(provider, url, None) };
        assert_eq!(
            local_ai_detail(&idle(AiProvider::Strata, "http://127.0.0.1:8080/v1")),
            trf("{host} · no model loaded", &[("host", &"127.0.0.1:8080")])
        );
        assert_eq!(
            local_ai_detail(&idle(AiProvider::Ollama, "http://localhost:11434/v1")),
            trf("{host} · {n} models", &[("host", &"localhost:11434"), ("n", &0)])
        );
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
        assert_eq!(state.step, Step::AiChoose, "where AI's Add opened the dialog");
        assert!(
            matches!(send(&mut state, Message::OpenAiKeyPage), Action::OpenUrl(url) if Some(url.as_str()) == AiProvider::OpenRouter.key_url())
        );
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
            assert_eq!(demo.start(), Start::At(Provider::Ai), "{demo:?}");
            let mut state = opened(demo, &data);
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

    // ----- ntfy (task E5.14) ---------------------------------------------------------------------

    /// The topics step of ntfy at `server`, with `topics` (name, label) typed into its rows.
    fn at_ntfy_topics(server: &str, topics: &[(&str, &str)]) -> State {
        let (mut state, _) = State::at(Provider::Ntfy, &Data::default());
        assert_eq!(state.step, Step::NtfyServer);
        send(&mut state, Message::NtfyServerChanged(server.into()));
        send(&mut state, Message::Continue);
        assert_eq!(state.step, Step::NtfyTopics);
        for (at, (name, label)) in topics.iter().enumerate() {
            if at > 0 {
                send(&mut state, Message::AddNtfyTopic);
            }
            send(&mut state, Message::NtfyTopicChanged(at, TopicName::new(*name)));
            send(&mut state, Message::NtfyLabelChanged(at, label.to_string()));
        }
        state
    }

    /// The core's answer when the topics need a login.
    fn needs_login() -> Result<Response, String> {
        Err(NtfyAccountError::Ntfy(NtfyError::Auth).message())
    }

    fn ntfy_account() -> AccountInfo {
        AccountInfo { kind: "ntfy".into(), ..account("ntfy:ntfy.sh", connected()) }
    }

    #[test]
    fn ntfy_sends_the_filled_rows_trimmed_and_ends_with_the_topics_subscribed() {
        let mut state = at_ntfy_topics("", &[(" backups-7f3a ", " Backups "), ("", ""), ("ci-91c2", "CI")]);
        let Action::Send(Request::ConnectNtfy { server, topics }) = send(&mut state, Message::Continue) else {
            panic!("Connect sends the topics")
        };
        // Empty is the public server; the core fills it in.
        assert_eq!(server, "");
        let sent: Vec<(&str, &str)> = topics.iter().map(|t| (t.topic.as_str(), t.label.as_str())).collect();
        assert_eq!(sent, [("backups-7f3a", "Backups"), ("ci-91c2", "CI")], "the empty row is no topic");
        assert_eq!(state.step, Step::NtfyChecking);
        assert!(state.busy());
        assert!(answered(&mut state, &Ok(Response::Account { account: ntfy_account() })));
        assert_eq!(state.step, Step::Done);
        assert_eq!(done_text(&state), tr("Your topics are subscribed."));
        drop(view(&state, &Data::default()));
    }

    #[test]
    fn what_is_wrong_with_a_topic_shows_only_after_the_first_connect() {
        let mut state = at_ntfy_topics("ntfy.sh", &[("ci builds", "")]);
        assert!(!state.ntfy.tried, "nothing is wrong before the first try");
        assert!(matches!(send(&mut state, Message::Continue), Action::None), "a wrong name is not sent");
        assert!(state.ntfy.tried);
        assert_eq!(state.ntfy.problem(), topic_problem("ci builds"));
        assert_eq!(state.step, Step::NtfyTopics);
        send(&mut state, Message::NtfyTopicChanged(0, TopicName::new("ci-builds")));
        assert_eq!(state.ntfy.problem().as_deref(), Some(tr("Give the topic a label.")));
        send(&mut state, Message::NtfyLabelChanged(0, "x".repeat(LABEL_MAX + 1)));
        assert!(state.ntfy.problem().is_some(), "a label longer than the core takes");
        send(&mut state, Message::NtfyLabelChanged(0, "CI".into()));
        assert_eq!(state.ntfy.problem(), None);

        // Rows without anything are no topics; none at all is a problem of its own.
        let empty = at_ntfy_topics("ntfy.sh", &[("", "")]);
        assert_eq!(empty.ntfy.problem().as_deref(), Some(tr("Add at least one topic.")));
    }

    #[test]
    fn rows_can_be_added_up_to_the_limit_and_the_last_one_stays() {
        let mut state = at_ntfy_topics("ntfy.sh", &[("a", "A")]);
        send(&mut state, Message::RemoveNtfyTopic(0));
        assert_eq!(state.ntfy.topics.len(), 1, "the last row stays");
        for _ in 0..TOPICS_MAX + 3 {
            send(&mut state, Message::AddNtfyTopic);
        }
        assert_eq!(state.ntfy.topics.len(), TOPICS_MAX);
        send(&mut state, Message::RemoveNtfyTopic(0));
        assert_eq!(state.ntfy.topics.len(), TOPICS_MAX - 1);
        assert_eq!(state.ntfy.topics[0].topic, "", "the first row went");
    }

    #[test]
    fn topics_that_need_a_login_lead_to_it_and_the_secret_is_sent_once() {
        let mut state = at_ntfy_topics("push.example.org", &[("backups-7f3a", "Backups")]);
        send(&mut state, Message::Continue);
        assert!(answered(&mut state, &needs_login()));
        assert_eq!((state.step, state.error.as_deref()), (Step::NtfyLogin, None), "the step says why");
        send(&mut state, Message::NtfyUserChanged(" anna ".into()));
        assert!(matches!(send(&mut state, Message::Continue), Action::None), "no secret yet");
        send(&mut state, Message::NtfySecretChanged(Secret::new("s3cret-pass")));
        // Neither the secret nor a topic's name is in the dialog's `Debug`.
        let debug = format!("{state:?}");
        assert!(!debug.contains("s3cret-pass") && !debug.contains("backups-7f3a"), "{debug}");
        let Action::Send(Request::SetNtfyLogin { server, topics, user, secret }) = send(&mut state, Message::Continue)
        else {
            panic!("Connect sends the login")
        };
        assert_eq!(
            (server.as_str(), user.as_deref(), secret.expose()),
            ("push.example.org", Some("anna"), "s3cret-pass")
        );
        assert_eq!(topics.len(), 1);
        assert!(state.ntfy.login.secret.is_blank(), "the dialog keeps no copy");
        // Refused again: back at the login with the core's sentence, to type the secret anew.
        assert!(answered(&mut state, &needs_login()));
        assert_eq!(state.step, Step::NtfyLogin);
        assert!(state.error.is_some());
    }

    #[test]
    fn an_access_token_goes_without_a_user_name() {
        let mut state = at_ntfy_topics("push.example.org", &[("backups-7f3a", "Backups")]);
        send(&mut state, Message::Continue);
        answered(&mut state, &needs_login());
        send(&mut state, Message::NtfyUserChanged("anna".into()));
        send(&mut state, Message::NtfyLoginKind(true));
        send(&mut state, Message::NtfySecretChanged(Secret::new("tk_abc")));
        let Action::Send(Request::SetNtfyLogin { user, .. }) = send(&mut state, Message::Continue) else {
            panic!("Connect sends the token")
        };
        assert_eq!(user, None);
    }

    #[test]
    fn another_refusal_goes_back_to_the_topics_with_the_core_message() {
        let mut state = at_ntfy_topics("example.org", &[("backups-7f3a", "Backups")]);
        send(&mut state, Message::Continue);
        let wrong = NtfyAccountError::Ntfy(NtfyError::InvalidServer).message();
        assert!(answered(&mut state, &Err(wrong.clone())));
        assert_eq!((state.step, state.error.clone()), (Step::NtfyTopics, Some(wrong)));
        send(&mut state, Message::Back);
        assert_eq!(state.step, Step::NtfyServer, "back to the address");
        send(&mut state, Message::Back);
        assert_eq!(state.step, Step::NtfyServer, "where ntfy's Add opened the dialog");
    }

    #[test]
    fn the_ntfy_demos_lead_where_they_say() {
        let data = super::super::launch::demo_data();
        for (demo, step) in [
            (Demo::NtfyServer, Step::NtfyServer),
            (Demo::NtfyTopics, Step::NtfyTopics),
            (Demo::NtfyInvalid, Step::NtfyTopics),
            (Demo::NtfyLogin, Step::NtfyLogin),
            (Demo::NtfyDone, Step::Done),
        ] {
            assert_eq!(demo.start(), Start::At(Provider::Ntfy), "{demo:?}");
            let mut state = opened(demo, &data);
            for message in demo_messages(demo) {
                if let Action::Send(request) = update(&mut state, message, &data) {
                    let answer = super::super::launch::demo_answer(&request, &data).expect("the demo answers");
                    answered(&mut state, &Ok(answer));
                }
            }
            assert_eq!(state.step, step, "{demo:?}");
            assert_eq!(state.ntfy.tried && state.ntfy.problem().is_some(), demo == Demo::NtfyInvalid, "{demo:?}");
            drop(view(&state, &data));
        }
    }

    // ----- A certificate the system does not trust (task M11.4) ---------------------------------

    /// A fingerprint in the core's format, each byte `byte`.
    fn fingerprint(byte: &str) -> String {
        vec![byte; 32].join(":")
    }

    fn untrusted(service: &str, host: &str, sha256: &str, public: bool) -> Response {
        Response::UntrustedCertificate { service: service.into(), host: host.into(), sha256: sha256.into(), public }
    }

    /// The services step of mailbox.org with the password typed, "Connect" pressed: the request.
    fn connecting() -> (State, Request) {
        let mut state = at_mail(Demo::Services);
        match send(&mut state, Message::Continue) {
            Action::Send(request) => (state, request),
            other => panic!("expected a request, got {other:?}"),
        }
    }

    /// The `ConnectMail` of `action` with its password and trusted certificates.
    fn connect_mail(action: Action) -> (String, Vec<String>) {
        match action {
            Action::Send(Request::ConnectMail { password, trusted_certificates, .. }) => {
                (password.expose().to_string(), trusted_certificates)
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn an_untrusted_certificate_asks_with_host_service_and_fingerprint() {
        let (mut state, _) = connecting();
        let sha = fingerprint("AB");
        assert!(answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &sha, false))));
        assert_eq!((state.step, state.error.as_deref()), (Step::MailCertificate, None));
        assert_eq!(subtitle(&state), tr("Trust this certificate?"));
        let asked = state.mail.certificates.asking().expect("asked").clone();
        assert_eq!(
            (asked.service.as_str(), asked.host.as_str(), asked.sha256.as_str()),
            ("IMAP", "mail.home.lan", sha.as_str())
        );
        assert_eq!(certificate_warning(&asked), None, "a server at home: no warning");
        assert!(has_back(&state));
        drop(view(&state, &Data::default()));

        let public = Untrusted { public: true, ..asked };
        assert!(certificate_warning(&public).is_some_and(|warning| warning.starts_with(tr(
            "This server is on the public internet. An untrusted certificate there can mean that someone is intercepting the connection. Never trust it for a mail provider."
        ))));
        // The fingerprint stands in two fixed lines of sixteen bytes and keeps every byte.
        let lines = fingerprint_lines(&sha);
        assert!(lines.lines().all(|line| line.split(':').count() == 16), "{lines:?}");
        assert_eq!(lines.lines().count(), 2);
        assert_eq!(lines.replace('\n', ":"), sha);
    }

    /// Issue #284: for a host on the public internet "Trust certificate" is no red primary button
    /// (the window then has none), for a server at home it is.
    #[test]
    fn trust_certificate_is_primary_only_for_a_server_at_home() {
        let at_home = Untrusted {
            service: "IMAP".into(),
            host: "mail.home.lan".into(),
            sha256: fingerprint("AB"),
            public: false,
        };
        let public = Untrusted { public: true, ..at_home.clone() };
        let theme = theme::theme();
        let drawn = |asked: &Untrusted| trust_style(asked)(&theme, iced::widget::button::Status::Active);
        assert_eq!(drawn(&at_home), style::button::primary(&theme, iced::widget::button::Status::Active));
        assert_eq!(drawn(&public), style::button::danger(&theme, iced::widget::button::Status::Active));
        for asked in [&at_home, &public] {
            drop(trust_button::<Message>(asked, false, Some(Message::Continue)));
            drop(trust_button::<Message>(asked, true, None));
        }
    }

    #[test]
    fn trust_certificate_sends_connect_mail_again_with_the_password_and_the_fingerprint() {
        let (mut state, first) = connecting();
        assert!(matches!(&first, Request::ConnectMail { trusted_certificates, .. } if trusted_certificates.is_empty()));
        let sha = fingerprint("AB");
        answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &sha, false)));
        assert!(!state.mail.password.is_blank(), "kept for the question");
        assert!(!format!("{state:?}").contains("demo-password"), "a Secret shows nothing");

        let (password, trusted) = connect_mail(send(&mut state, Message::Continue));
        assert_eq!((password.as_str(), trusted), ("demo-password", vec![sha.clone()]));
        assert_eq!(state.step, Step::MailConnecting);
        assert!(state.mail.certificates.asking().is_none(), "the question is answered");
        // It stays on screen while the request is out, with "Signing in…" in the foot.
        assert_eq!(state.mail.certificates.pending().map(|asked| asked.sha256.as_str()), Some(sha.as_str()));
        assert_eq!(subtitle(&state), tr("Trust this certificate?"));
        drop(view(&state, &Data::default()));
        assert!(matches!(send(&mut state, Message::Continue), Action::None), "once");
        // The answer ends it.
        answered(&mut state, &Err("The server cannot be reached.".to_string()));
        assert!(state.mail.certificates.shown().is_none());
    }

    /// The core stores a certificate only once the whole sign-in succeeded: when CalDAV asks after
    /// IMAP, the next request carries both, not only the last.
    #[test]
    fn every_certificate_trusted_in_the_dialog_goes_with_each_request() {
        let (mut state, _) = connecting();
        let (imap, caldav) = (fingerprint("AB"), fingerprint("CD"));
        answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &imap, false)));
        let (_, trusted) = connect_mail(send(&mut state, Message::Continue));
        assert_eq!(trusted, std::slice::from_ref(&imap));
        assert!(state.mail.certificates.shown().is_some(), "the first question stays while signing in");
        answered(&mut state, &Ok(untrusted("CalDAV", "dav.home.lan", &caldav, false)));
        assert_eq!(state.step, Step::MailCertificate);
        // The second question takes the place of the first.
        assert_eq!(state.mail.certificates.shown().map(|asked| asked.service.as_str()), Some("CalDAV"));
        assert!(state.mail.certificates.pending().is_none());
        let (password, trusted) = connect_mail(send(&mut state, Message::Continue));
        assert_eq!((password.as_str(), trusted), ("demo-password", vec![imap.clone(), caldav.clone()]));
        // The same certificate asked again is trusted once.
        answered(&mut state, &Ok(untrusted("SMTP", "mail.home.lan", &imap, false)));
        let (_, trusted) = connect_mail(send(&mut state, Message::Continue));
        assert_eq!(trusted, [imap, caldav]);
    }

    #[test]
    fn back_from_the_question_clears_the_password() {
        let (mut state, _) = connecting();
        answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &fingerprint("AB"), true)));
        send(&mut state, Message::Back);
        assert_eq!(state.step, Step::MailPassword);
        assert!(state.mail.password.is_blank(), "the field is empty");
        assert!(state.mail.certificates.asking().is_none());
        assert!(!mail_can_continue(&state, Step::MailPassword), "until it is typed again");
        // Typed again, the request trusts nothing: the question was not answered.
        send(&mut state, Message::PasswordChanged(Secret::new("again")));
        send(&mut state, Message::Continue);
        let (password, trusted) = connect_mail(send(&mut state, Message::Continue));
        assert_eq!((password.as_str(), trusted.is_empty()), ("again", true));
    }

    #[test]
    fn every_other_answer_and_closing_clear_the_password() {
        let refused = trf("The {service} server refused the password.", &[("service", &"IMAP")]);
        for (answer, step) in [
            (Err(refused), Step::MailPassword),
            (Err("The server cannot be reached.".to_string()), Step::MailServices),
            (Ok(Response::Ok), Step::MailServices),
        ] {
            let (mut state, _) = connecting();
            answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &fingerprint("AB"), false)));
            send(&mut state, Message::Continue);
            assert!(answered(&mut state, &answer));
            assert!(state.mail.password.is_blank(), "{answer:?}");
            assert_eq!(state.step, step, "{answer:?}");
        }
        let (mut state, _) = connecting();
        answered(&mut state, &Ok(untrusted("IMAP", "mail.home.lan", &fingerprint("AB"), false)));
        send(&mut state, Message::Close);
        assert!(state.mail.password.is_blank(), "closing");
    }

    #[test]
    fn a_take_over_with_an_untrusted_certificate_leads_to_the_password() {
        let data = Data::default();
        let mut state = with_torromail(vec![torromail_account("f", "anna@fastmail.com")]);
        update(&mut state, Message::UseTorroMail("f".into()), &data);
        assert!(answered(&mut state, &Ok(untrusted("IMAP", "imap.example.org", &fingerprint("AB"), true))));
        assert_eq!(
            state.torromail.failed,
            Some(("f".to_string(), tr("The certificate is not trusted. Enter the password to check it.").to_string()))
        );
        assert_eq!(state.step, Step::TorroMail);
        drop(view(&state, &data));
        update(&mut state, Message::EnterTorroMailPassword("f".into()), &data);
        assert_eq!(state.step, Step::MailPassword, "the mail path asks there");
    }
}
