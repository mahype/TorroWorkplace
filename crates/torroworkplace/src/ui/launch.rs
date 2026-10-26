//! The start parameters of the control centre and the sample data of `--demo`.
//!
//! `torroworkplace ui --demo --open wizard-mail --appearance light` shows a state without a service
//! and without clicking, so `torro-design-review` can photograph every state from a script. In
//! demo mode the window never talks to a socket and never stores anything: the pages get
//! [`demo_data`], and every request a page wants to send is dropped (see `Ui::perform`).

use std::fmt;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Local, Utc};
use torro_core::AppId;
use torro_core::ai::{AiError, AiModel, AiProvider};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_inbox::item::SourceHealth;
use torro_inbox::{AgentKind, SourceInfo, SourceRef};
use torro_ipc::api::{
    ASSISTANT_CLIENT, Area, AreaRights, BOT_CLIENT, Confirm, HOOK_CLIENT, Level, Outbound, Rights, RightsTemplate,
    Selection,
};
use torro_ipc::{
    AccountInfo, ActivityEntry, AiServer, AppInfo, ApprovalInfo, ApprovalState, ClientInfo, ColorSchemeInfo,
    DesignInfo, FolderChoice, FontInfo, IssuedKey, Key, MailView, NtfyTopicView, NtfyView, OpenRoleView, Request,
    Response, ScreenInfo, ServerService, ServiceState, Status, TorroMailAccount,
};
use torro_mail::{Endpoint, MailSettings, Notify, Role, Security, Services};
use torro_mail_imap::discover::MailUnsupported;
use torro_pim::model::GitHubFilter;
use torro_pim::{AccountState, Collection, CollectionKind};
use torro_setups::TemplateSummary;

use super::general::{DesignEntry, Entry, FontEntry, Lists, SchemeEntry};
use super::wizard;
use super::{Appearance, Data, Section};
use crate::account::ntfy::NtfyAccountError;
use crate::client_setup::HookNote;
use torro_ntfy::NtfyError;

/// A state the control centre can start in (`--demo --open <state>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// Accounts without any account (task S.30): every group with only its sentence and "Add".
    AccountsEmpty,
    /// The AI section with its accounts, "Add" and the data rule.
    AccountsAi,
    /// The add-account dialog as Nextcloud's "Add" opens it: the server address.
    WizardNextcloud,
    /// The add-account dialog at GitHub's two ways to sign in.
    WizardGithub,
    /// The add-account dialog at the field for a GitHub token.
    WizardToken,
    /// The add-account dialog on the mail account path (tasks M7.2, M5.1), the ntfy path (task
    /// E5.14) or at TorroMail's accounts (task S.30), opened where [`wizard::Demo::start`] says and
    /// at the state the script of [`wizard::demo_messages`] leads to; the demo's search answers for
    /// a few domains, its ntfy server wants a login, and its mail servers on `.lan` and `.example`
    /// show a certificate the system does not trust (task M11.4, `wizard-mail-certificate` and
    /// `wizard-mail-certificate-public`, [`demo_answer`]).
    WizardMail(wizard::Demo),
    /// The add-account dialog on the AI provider path (task KI.1a), at the state the script of
    /// [`wizard::demo_messages`] leads to; the demo finds Ollama on this computer, not LM Studio.
    WizardAi(wizard::Demo),
    /// "Change password" of the demo's CalDAV account.
    ChangePassword,
    /// The detail page of the first demo account.
    AccountDetail,
    /// The detail page of the demo's GitHub account.
    GithubDetail,
    /// The detail page of the demo's AI account (OpenRouter, task KI.1a) with its models, as the
    /// demo lists them (task S.31, [`demo_models`]): search, "Show closed models" and switches.
    AiDetail,
    /// The detail page of the demo's ntfy account (task E5.14): its topics and its login.
    NtfyDetail,
    /// The ntfy account's page with the dialog of its first topic (rename, unsubscribe).
    NtfyTopic,
    /// The ntfy account's page with "Change login…" open.
    NtfyLogin,
    /// The detail page of the first demo mail account (task M5.2): synced mail and calendars, a
    /// signature, and the question for the folder of sent mail.
    MailDetail,
    /// The detail page of the second demo mail account: mail only, with a problem.
    MailDetailProblem,
    /// "Change password" of the first demo mail account.
    MailPassword,
    /// "Server settings" of the first demo mail account.
    MailServers,
    /// The section menu ("Accounts ▾"); only the medium and narrow header have it.
    Menu,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The "Remove this account?" confirmation.
    Remove,
    /// The "Delete this template?" confirmation.
    DeleteTemplate,
    /// The dialog of the template without a key (task WS.6) with key 2 chosen, which another
    /// template holds: the hint that saving moves the key.
    EditTemplate,
    /// "Save workspace…" on the page answered: the dialog in the mode `Save` for the new template,
    /// with the next free key chosen.
    SaveTemplate,
    /// Workspaces without a window manager: the group says which one is expected and what to do.
    WorkspacesUnsupported,
    /// The approvals page without any approvals: its empty state.
    ApprovalsEmpty,
    /// API without any key (task S.35): the group "Keys" with only its sentence.
    ApiEmpty,
    /// API with "Allow API keys" off.
    ApiOff,
    /// The page of the demo script's key: the rights editor with endpoints, its activity.
    ApiDetail,
    /// The dialog with a new key for the demo script, shown once.
    ApiNewKey,
    /// The "Revoke this key?" confirmation of the demo script.
    ApiRevoke,
    /// "Create key" at its first step: name and template.
    CreateKey,
    /// "Create key" done: the key of the new script "nightly-backup", shown once.
    CreateKeyDone,
    /// MCP without any client: "Connected clients" with only its sentence.
    McpEmpty,
    /// MCP with "Allow MCP" off.
    McpOff,
    /// The page of the demo agent Codex with rights of its own (levels, admin switch, selections)
    /// and its calendar area open with one command switched off (task S.33).
    McpDetail,
    /// The page of Claude Code with its hooks on: the group "Notification Hub" with where they are.
    McpHooks,
    /// The dialog with a new key of the demo agent, shown once.
    McpNewKey,
    /// The "Disconnect this client?" confirmation.
    McpDisconnect,
    /// The page of Codex with `bots` and `assistant` switched on: both warnings show.
    McpBots,
    /// MCP without Claude Code, whose hook client stays: the row "Hooks without an agent".
    McpOrphans,
    /// "Connect client" at its first step: the clients the demo found.
    ConnectClient,
    /// "Connect client" at the rights of Claude Code.
    ConnectClientRights,
    /// "Connect client" done: Claude Code connected (nothing is written).
    ConnectClientDone,
    /// "Connect client" with the key of a new "Other MCP client" and its command, shown once.
    ConnectClientOther,
    /// MCP servers (task MA.7) with the PDF tool on, a folder and a logo of its own, the tool `web` on
    /// with DuckDuckGo and Wikipedia, and the demo's servers: two of the catalog (one running, one
    /// stopped after errors) and one of one's own with tools to review ([`demo_connections`]).
    Connections,
    /// The page of the search providers below the tool `web` (task WEB.6, here since #302), as
    /// `connections` sets them up.
    SearchProviders,
    /// MCP servers without any server, the PDF tool as it starts and the assistant without it
    /// ([`demo_connections_empty`]).
    ConnectionsEmpty,
    /// The install dialog of GitHub's entry (task MA.8), the demo's GitHub server taken out first
    /// ([`demo_uninstalled`]): its token field, its tools with their access, "Install".
    CatalogInstall,
    /// The same dialog after "Install" with a token typed: the tools the server reported, which
    /// "Allow tools" allows ([`demo_answer`] installs nothing).
    CatalogInstallTools,
    /// The page of "Time" (task MA.8): running, its tools allowed, its output.
    ExtensionDetail,
    /// The page of "Time" with a tool changed since it was allowed ([`demo_review`]): "Changed since
    /// you allowed it" and "Allow tools".
    ExtensionReview,
    /// The page of "GitHub", stopped after errors, with its stored token and its output.
    ExtensionCrashed,
    /// The page of "WordCraft" (issue #303): the group "Folder" with the allowed folder chosen for it, its blocked
    /// tools.
    ExtensionFolder,
    /// The page of "GridCraft" (issue #303): no folder chosen yet, and that it keeps to no folder itself.
    ExtensionUnbounded,
    /// The form of a server of one's own (task MA.9), filled in from a pasted block
    /// ([`DEMO_PASTE_ONE`]): a command with arguments, its API key marked secret, a plain variable.
    CustomServer,
    /// "Paste configuration" with a block of two servers typed in ([`DEMO_PASTE_TWO`]).
    CustomServerPaste,
    /// The choice among the two servers of that block, after "Fill in".
    CustomServerChoose,
    /// Appearance with the demo's designs, fonts and colour schemes ([`demo_looks`]), one of each
    /// broken; the design `--design` names is drawn, else the standard.
    Design,
    /// Appearance with the colour scheme's dropdown open (task L.37).
    DesignSchemes,
    /// Appearance after Console was chosen: the demo switched the mail view as the core would, and
    /// the page says so ("Also switched: plain-text mail", task L.37).
    DesignSwitched,
    /// Appearance with the font's dropdown open (issue #299).
    DesignFonts,
    /// The font's dropdown with "mo" typed into its search field (issue #299).
    DesignFontsSearch,
    /// The first-run setup at this step (task W.4), with the demo's sample data.
    Setup(super::setup::Step),
    /// The setup's Nextcloud step without a Nextcloud account: the address form (task W.5).
    SetupNextcloudAddress,
    /// The setup's Nextcloud step while the browser signs in, with the link to copy.
    SetupNextcloudWaiting,
    /// The setup's calendar step with the CalDAV form open, an address typed (task W.6).
    SetupCalDav,
    /// The setup's tasks step without the demo's GitHub account: GitHub's two ways.
    SetupGithub,
    /// The setup's mail step with the add-account dialog over it, on its mail path (task W.7).
    SetupMailAdd,
    /// The setup's AI step after its search (task W.8): the demo finds Ollama, not LM Studio.
    SetupAi,
}

impl Open {
    pub const ALL: [(&'static str, Open); 106] = [
        ("accounts-empty", Open::AccountsEmpty),
        ("accounts-ai", Open::AccountsAi),
        ("wizard-nextcloud", Open::WizardNextcloud),
        ("wizard-ways", Open::WizardGithub),
        ("wizard-token", Open::WizardToken),
        ("wizard-mail", Open::WizardMail(wizard::Demo::Address)),
        ("wizard-mail-found", Open::WizardMail(wizard::Demo::Found)),
        ("wizard-mail-advanced", Open::WizardMail(wizard::Demo::Advanced)),
        ("wizard-mail-unsupported", Open::WizardMail(wizard::Demo::Unsupported)),
        ("wizard-mail-gmail", Open::WizardMail(wizard::Demo::Gmail)),
        ("wizard-mail-none", Open::WizardMail(wizard::Demo::NotFound)),
        ("wizard-mail-password", Open::WizardMail(wizard::Demo::Password)),
        ("wizard-mail-refused", Open::WizardMail(wizard::Demo::Refused)),
        ("wizard-mail-refused-smtp", Open::WizardMail(wizard::Demo::RefusedSmtp)),
        ("wizard-mail-services", Open::WizardMail(wizard::Demo::Services)),
        ("wizard-mail-mail-only", Open::WizardMail(wizard::Demo::MailOnly)),
        ("wizard-mail-done", Open::WizardMail(wizard::Demo::Done)),
        ("wizard-mail-certificate", Open::WizardMail(wizard::Demo::Certificate)),
        ("wizard-mail-certificate-public", Open::WizardMail(wizard::Demo::CertificatePublic)),
        ("wizard-torromail", Open::WizardMail(wizard::Demo::TorroMail)),
        ("wizard-torromail-failed", Open::WizardMail(wizard::Demo::TorroMailFailed)),
        ("wizard-torromail-password", Open::WizardMail(wizard::Demo::TorroMailPassword)),
        ("wizard-torromail-done", Open::WizardMail(wizard::Demo::TorroMailDone)),
        ("wizard-ai", Open::WizardAi(wizard::Demo::AiProviders)),
        ("wizard-ai-openrouter", Open::WizardAi(wizard::Demo::AiOpenRouter)),
        ("wizard-ai-refused", Open::WizardAi(wizard::Demo::AiRefused)),
        ("wizard-ai-server", Open::WizardAi(wizard::Demo::AiServer)),
        ("wizard-ai-done", Open::WizardAi(wizard::Demo::AiDone)),
        ("wizard-ntfy", Open::WizardMail(wizard::Demo::NtfyServer)),
        ("wizard-ntfy-topics", Open::WizardMail(wizard::Demo::NtfyTopics)),
        ("wizard-ntfy-invalid", Open::WizardMail(wizard::Demo::NtfyInvalid)),
        ("wizard-ntfy-login", Open::WizardMail(wizard::Demo::NtfyLogin)),
        ("wizard-ntfy-done", Open::WizardMail(wizard::Demo::NtfyDone)),
        ("change-password", Open::ChangePassword),
        ("account-detail", Open::AccountDetail),
        ("scopes-detail", Open::GithubDetail),
        ("account-detail-ai", Open::AiDetail),
        ("ntfy-detail", Open::NtfyDetail),
        ("ntfy-topic", Open::NtfyTopic),
        ("ntfy-login", Open::NtfyLogin),
        ("mail-detail", Open::MailDetail),
        ("mail-detail-problem", Open::MailDetailProblem),
        ("mail-password", Open::MailPassword),
        ("mail-servers", Open::MailServers),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("remove", Open::Remove),
        ("delete-template", Open::DeleteTemplate),
        ("edit-template", Open::EditTemplate),
        ("save-template", Open::SaveTemplate),
        ("workspaces-unsupported", Open::WorkspacesUnsupported),
        ("approvals-empty", Open::ApprovalsEmpty),
        ("api-empty", Open::ApiEmpty),
        ("api-off", Open::ApiOff),
        ("api-detail", Open::ApiDetail),
        ("api-new-key", Open::ApiNewKey),
        ("api-revoke", Open::ApiRevoke),
        ("create-key", Open::CreateKey),
        ("create-key-done", Open::CreateKeyDone),
        ("mcp-empty", Open::McpEmpty),
        ("mcp-off", Open::McpOff),
        ("mcp-detail", Open::McpDetail),
        ("mcp-hooks", Open::McpHooks),
        ("mcp-new-key", Open::McpNewKey),
        ("mcp-disconnect", Open::McpDisconnect),
        ("mcp-bots", Open::McpBots),
        ("mcp-orphans", Open::McpOrphans),
        ("connect-client", Open::ConnectClient),
        ("connect-client-rights", Open::ConnectClientRights),
        ("connect-client-done", Open::ConnectClientDone),
        ("connect-client-other", Open::ConnectClientOther),
        ("connections", Open::Connections),
        ("connections-empty", Open::ConnectionsEmpty),
        ("search-providers", Open::SearchProviders),
        ("catalog-install", Open::CatalogInstall),
        ("catalog-install-tools", Open::CatalogInstallTools),
        ("extension-detail", Open::ExtensionDetail),
        ("extension-review", Open::ExtensionReview),
        ("extension-crashed", Open::ExtensionCrashed),
        ("extension-folder", Open::ExtensionFolder),
        ("extension-unbounded", Open::ExtensionUnbounded),
        ("custom-server", Open::CustomServer),
        ("custom-server-paste", Open::CustomServerPaste),
        ("custom-server-choose", Open::CustomServerChoose),
        ("design", Open::Design),
        ("design-schemes", Open::DesignSchemes),
        ("design-switched", Open::DesignSwitched),
        ("design-fonts", Open::DesignFonts),
        ("design-fonts-search", Open::DesignFontsSearch),
        ("setup", Open::Setup(super::setup::Step::Welcome)),
        ("setup-nextcloud", Open::Setup(super::setup::Step::Nextcloud)),
        ("setup-nextcloud-address", Open::SetupNextcloudAddress),
        ("setup-nextcloud-waiting", Open::SetupNextcloudWaiting),
        ("setup-components", Open::Setup(super::setup::Step::Components)),
        ("setup-calendar", Open::Setup(super::setup::Step::Calendar)),
        ("setup-calendar-caldav", Open::SetupCalDav),
        ("setup-tasks", Open::Setup(super::setup::Step::Tasks)),
        ("setup-tasks-github", Open::SetupGithub),
        ("setup-mail", Open::Setup(super::setup::Step::Mail)),
        ("setup-mail-add", Open::SetupMailAdd),
        ("setup-inbox", Open::Setup(super::setup::Step::Inbox)),
        ("setup-ai", Open::SetupAi),
        ("setup-web", Open::Setup(super::setup::Step::Web)),
        ("setup-documents", Open::Setup(super::setup::Step::Documents)),
        ("setup-done", Open::Setup(super::setup::Step::Done)),
    ];
}

/// The start parameters of `torroworkplace ui`:
///
/// - `--section overview|accounts|ai|api|mcp|connections|approvals|workspaces|appearance|general`: the page to
///   start on. The states of `--open` that belong to a page choose that page themselves. Other
///   programs open a page this way (`torroworkplace ui --section accounts`). The words of pages
///   that moved keep working ([`Section::from_arg`]): `agents` is MCP (task S.35), `apps` and
///   `inbox` are General, where the suite's parts are switched on and opened. `assistant` and
///   `bot:<id>` ([`Launch::client`]) bring the assistant's window to the front instead of a page:
///   what the assistant and its bots may use is set there, under Access (task S.34).
/// - `--demo`: sample accounts, templates, settings, apps, agents, approvals and inbox rules in memory instead
///   of the service. Nothing is sent and nothing is stored; [`demo_answer`] answers the few
///   requests whose answer is a state of its own.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--design <id>`, `--font <id>`, `--color-scheme <id>`: draw with this design, font or colour scheme (task L.22, each from its own folder) instead of the stored one (task L.3); needs
///   `--demo`. An id that is not a valid pack is refused with the valid ones.
/// - `--setup`: open the first-run setup (task W.4, `docs/einrichtung.md`) instead of a section;
///   `--step <step>` starts it at that step (needs `--setup` or `--setup-if-open`). The core opens
///   it so on a first start without accounts.
/// - `--setup-if-open`: open the setup as long as it is not done, else the `--section` (task W.9):
///   what an app's "Open Settings" sends, with the app's step.
/// - `--template=<id>`: open only the dialog of this template in a small window of its own (task
///   WS.6, class `torro-workplace-dialog`) instead of the control centre, in the mode `Edit`; it
///   closes after "Save" or "Cancel". `apps.open_app` with the target `template:<id>` starts this.
/// - `--saved` (needs `--template`): the dialog in the mode `Save`, for a template saved just now;
///   "Cancel" deletes it again. Only the tray's "Save workspace…" (task WS.7) passes it, never the
///   target `template:<id>`, so no program can have a template deleted by opening its dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub section: Section,
    /// `--section assistant` or `--section bot:<id>`: the assistant's window comes to the front
    /// ([`ClientTarget::request`]); `section` is then the overview.
    pub client: Option<ClientTarget>,
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub design: Option<String>,
    /// `--font <id>` (task L.22): draw with this font instead of the stored one; needs `--demo`.
    pub font: Option<String>,
    /// `--color-scheme <id>` (task L.22): draw with this colour scheme instead of the stored one;
    /// needs `--demo`.
    pub color_scheme: Option<String>,
    /// `--setup`: the step the setup starts at; `None` without the setup.
    pub setup: Option<super::setup::Step>,
    /// `--setup-if-open`: [`Launch::setup`] only while the setup is not done (task W.9).
    pub setup_if_open: bool,
    /// `--template=<id>`: the dialog of this template in a window of its own (task WS.6).
    pub template: Option<String>,
    /// `--saved`: that dialog in the mode `Save` ([`super::template_form::Mode::Save`]).
    pub saved: bool,
}

impl Default for Launch {
    fn default() -> Self {
        Launch {
            section: Section::Overview,
            client: None,
            demo: false,
            open: None,
            appearance: None,
            design: None,
            font: None,
            color_scheme: None,
            setup: None,
            setup_if_open: false,
            template: None,
            saved: false,
        }
    }
}

/// Why the start parameters were refused; `main` prints it and exits with 64, as `ctl` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// A flag without its value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the list of valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// `--open` without `--demo`: the states need sample data.
    NeedsDemo(&'static str),
    /// `--step` without `--setup`.
    NeedsSetup(&'static str),
    /// `--saved` without `--template`.
    NeedsTemplate(&'static str),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
            LaunchError::NeedsSetup(flag) => write!(f, "{flag} only works together with --setup or --setup-if-open"),
            LaunchError::NeedsTemplate(flag) => write!(f, "{flag} only works together with --template"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// The value of `--flag value` or `--flag=value`; `None` when the flag is not there.
fn flag_value<'a>(args: &'a [String], flag: &'static str) -> Result<Option<&'a str>, LaunchError> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            return words.next().map(|value| Some(value.as_str())).ok_or(LaunchError::MissingValue(flag));
        }
        if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

impl Launch {
    /// `--design`, `--font` and `--color-scheme` for `torro_gui::design::start` (task L.22).
    pub fn forced(&self) -> torro_gui::design::Forced {
        torro_gui::design::Forced {
            design: self.design.clone(),
            font: self.font.clone(),
            color_scheme: self.color_scheme.clone(),
        }
    }

    /// Reads the start parameters from `args` (the words after `ui`).
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let (section, client) = match flag_value(args, "--section")? {
            None => (Section::Overview, None),
            // What the assistant and a bot may use is set in the assistant's window (task S.34): the
            // words bring it to the front, the control centre shows its overview.
            Some("assistant") => (Section::Overview, Some(ClientTarget::Assistant)),
            Some(word) => match word.strip_prefix("bot:") {
                Some(id) if is_bot_id(id) => (Section::Overview, Some(ClientTarget::Bot(id.to_owned()))),
                _ => {
                    let mut valid: Vec<&str> = Section::ALL.iter().map(|s| s.arg()).collect();
                    valid.extend(["assistant", "bot:<id>", "agents", "apps", "inbox"]);
                    (Section::from_arg(word).ok_or_else(|| unknown("--section", word, valid))?, None)
                }
            },
        };
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--open")),
            Some(word) => match Open::ALL.iter().find(|(name, _)| *name == word) {
                Some(&(_, open)) => Some(open),
                None => return Err(unknown("--open", word, Open::ALL.iter().map(|(name, _)| *name).collect())),
            },
        };
        let appearance = match flag_value(args, "--appearance")? {
            None => None,
            Some("light") => Some(Appearance::Light),
            Some("dark") => Some(Appearance::Dark),
            Some("system") => Some(Appearance::System),
            Some(other) => return Err(unknown("--appearance", other, vec!["light", "dark", "system"])),
        };
        // `--design`, `--font` and `--color-scheme` each take the valid ids of their own folder (task L.22).
        let part = |flag: &'static str, part: torro_gui::design::Part| -> Result<Option<String>, LaunchError> {
            match flag_value(args, flag)? {
                None => Ok(None),
                Some(_) if !demo => Err(LaunchError::NeedsDemo(flag)),
                Some(id) if torro_gui::design::is_choice(part, id) => Ok(Some(id.to_owned())),
                Some(id) => {
                    let valid = torro_gui::design::choices(part);
                    Err(unknown(flag, id, valid.iter().map(String::as_str).collect()))
                }
            }
        };
        let design = part("--design", torro_gui::design::Part::Design)?;
        let font = part("--font", torro_gui::design::Part::Font)?;
        let color_scheme = part("--color-scheme", torro_gui::design::Part::ColorScheme)?;
        let setup_if_open = args.iter().any(|arg| arg == "--setup-if-open");
        let asked = args.iter().any(|arg| arg == "--setup") || setup_if_open;
        let setup = match (asked, flag_value(args, "--step")?) {
            (false, None) => None,
            (false, Some(_)) => return Err(LaunchError::NeedsSetup("--step")),
            (true, None) => Some(super::setup::Step::Welcome),
            (true, Some(word)) => Some(
                super::setup::Step::from_arg(word)
                    .ok_or_else(|| unknown("--step", word, super::setup::Step::args()))?,
            ),
        };
        let template = match flag_value(args, "--template")? {
            Some("") => return Err(LaunchError::MissingValue("--template")),
            other => other.map(str::to_owned),
        };
        let saved = args.iter().any(|arg| arg == "--saved");
        if saved && template.is_none() {
            return Err(LaunchError::NeedsTemplate("--saved"));
        }
        Ok(Launch {
            section,
            client,
            demo,
            open,
            appearance,
            design,
            font,
            color_scheme,
            setup,
            setup_if_open,
            template,
            saved,
        })
    }
}

/// The assistant or one of its bots, named by `--section` (tasks S.21, S.35). Their rights were a
/// page of the control centre until task S.34 moved them into the assistant's window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientTarget {
    /// `--section assistant`.
    Assistant,
    /// `--section bot:<id>`: the bot `id`.
    Bot(String),
}

impl ClientTarget {
    /// What the control centre sends for it once the core answers: `apps.open_app` for the
    /// assistant, which brings its window to the front (or starts it). The window cannot be told
    /// to open Access there yet – that needs a way from the core to the app, `docs/settings.md` ›
    /// "Assistent und Bots" –, so a bot's word opens the same window.
    pub fn request(&self) -> Request {
        Request::OpenApp { app: AppId::ASSISTANT.binary.to_string(), target: None }
    }
}

/// A word that can be a bot's id in `--section bot:<id>`: not empty, within the core's limit, only
/// letters and digits (the core makes 8 hex digits), so a mistyped command opens nothing odd.
fn is_bot_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= torro_ipc::assistant::MAX_ID_BYTES && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// What the demo's samples say about themselves: name, version, author "Torro samples" and a
/// description.
fn sample_about(name: &str, version: &str, description: &str) -> torro_core::design::About {
    torro_core::design::About {
        name: name.into(),
        version: version.into(),
        author: Some("Torro samples".into()),
        description: Some(description.into()),
        ..torro_core::design::About::default()
    }
}

/// The designs, fonts and colour schemes of `--demo` (Appearance, tasks L.4, L.23 and L.37), in memory
/// like all its data, each list with the standard first, samples that can be chosen and are drawn
/// at once, and one that breaks rules and says so:
///
/// - designs: the three built-in ones, Standard, Compact and Console (Console suggests JetBrains
///   Mono, which the demo does not have, and plain-text mail), and "Neon" (broken);
/// - fonts: "Tabular" (Inter's tabular cut as text font: the only other family the demo has without
///   a folder), "Brush" (broken);
/// - colour schemes: "Harbor" (teal accent, cool surfaces), "Paper" (warm light surfaces, blue
///   accent), "Midnight" (dark only), "Neon" (broken).
///
/// A part `--design`, `--font` or `--color-scheme` names (read from its folder at start, so it is the
/// active one) joins its list after the samples, so it carries the check mark.
pub fn demo_looks() -> Lists {
    Lists { designs: Ok(demo_designs()), fonts: Ok(demo_fonts()), schemes: Ok(demo_schemes()) }
}

/// The designs of [`demo_looks`].
fn demo_designs() -> Vec<DesignEntry> {
    use torro_core::design::{BUILT_IN_IDS, Design, DesignError};
    let info = |design: &Design| DesignInfo {
        id: design.id.clone(),
        name: design.about.name.clone(),
        version: design.about.version.clone(),
        author: design.about.author.clone(),
        description: design.about.description.clone(),
        standard: design.is_standard(),
        valid: true,
        problems: Vec::new(),
        active: false,
        font: design.about.font.clone(),
        mail_view: design.about.mail_view,
        density: design.density_level(),
    };
    let neon = DesignInfo {
        id: "neon".into(),
        name: "Neon".into(),
        version: "0.3.0".into(),
        author: Some("Torro samples".into()),
        description: None,
        standard: false,
        valid: false,
        problems: [
            DesignError::OutOfRange { key: "type.body.size".into(), value: 9.0, min: 11.0, max: 20.0 },
            DesignError::LineHeight { key: "type.caption".into(), line: 12.0, size: 14.0 },
            DesignError::UnknownKey { key: "space.xxxl".into() },
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        active: false,
        font: None,
        mail_view: None,
        density: Default::default(),
    };
    // The built-in designs need no folder: `load` answers them from the embedded files.
    let mut designs: Vec<Design> =
        BUILT_IN_IDS.iter().filter_map(|id| torro_core::design::load(std::path::Path::new(""), id).ok()).collect();
    let forced = torro_gui::design::active();
    if !designs.iter().any(|design| design.id == forced.id) {
        designs.push((*forced).clone());
    }
    let mut entries: Vec<DesignEntry> =
        designs.into_iter().map(|design| Entry::design(info(&design), Some(design))).collect();
    entries.push(Entry::design(neon, None));
    entries
}

/// The fonts of [`demo_looks`]: the standard (bundled, Sans), Tabular from a folder without a kind
/// ("Other"), Brush broken, and three fonts of the computer (issue #299) – "System", Avenir Next and
/// Monaco – listed without a font to draw with, so their samples are the standard's and the pictures
/// look alike on every computer.
fn demo_fonts() -> Vec<FontEntry> {
    use torro_core::design::{DesignError, FontSet};
    let info = |font: &FontSet| FontInfo {
        id: font.id.clone(),
        family: Some(font.fonts.text.family.clone()),
        name: font.about.name.clone(),
        version: font.about.version.clone(),
        author: font.about.author.clone(),
        description: font.about.description.clone(),
        license: Some(font.license.clone()),
        origin: torro_ipc::FontOrigin::Bundled,
        kind: font.kind,
        standard: font.is_standard(),
        valid: true,
        problems: Vec::new(),
        active: false,
    };
    let standard = FontSet::standard();
    let figures = standard.fonts.numbers.clone();
    let mut tabular = FontSet {
        id: "tabular".into(),
        about: sample_about("Tabular", "1.0.0", "Inter with tabular figures"),
        ..FontSet::standard()
    };
    (tabular.fonts.text, tabular.fonts.heading) = (figures.clone(), figures);
    tabular.kind = None;
    let installed = |id: &str, family: &str, name: &str| FontInfo {
        id: id.into(),
        family: Some(family.into()),
        name: name.into(),
        version: String::new(),
        author: None,
        description: None,
        license: None,
        origin: torro_ipc::FontOrigin::System,
        kind: None,
        standard: false,
        valid: true,
        problems: Vec::new(),
        active: false,
    };
    let system = [
        installed("system", "Helvetica Neue", "System"),
        installed("system:Avenir Next", "Avenir Next", "Avenir Next"),
        installed("system:Monaco", "Monaco", "Monaco"),
    ];
    let brush = FontInfo {
        id: "brush".into(),
        family: None,
        name: "Brush".into(),
        version: "0.2.0".into(),
        author: Some("Torro samples".into()),
        description: None,
        license: None,
        origin: torro_ipc::FontOrigin::Folder,
        kind: None,
        standard: false,
        valid: false,
        problems: [
            DesignError::MissingFile { key: "fonts.text.regular".into(), path: "Brush-Regular.ttf".into() },
            DesignError::FontLicense,
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        active: false,
    };
    let mut fonts = vec![standard, tabular];
    let forced = torro_gui::font::active();
    if !fonts.iter().any(|font| font.id == forced.id) {
        fonts.push((*forced).clone());
    }
    let folder = |mut info: FontInfo| {
        if !info.standard && info.id == "tabular" {
            info.origin = torro_ipc::FontOrigin::Folder;
        }
        info
    };
    let mut entries: Vec<FontEntry> =
        fonts.into_iter().map(|font| Entry::new(folder(info(&font)), Some(font))).collect();
    entries.push(Entry::new(brush, None));
    entries.extend(system.into_iter().map(|info| Entry::new(info, None)));
    entries
}

/// The colour schemes of [`demo_looks`].
fn demo_schemes() -> Vec<SchemeEntry> {
    use torro_core::design::{ColorScheme, DesignError, Mode, Rgba};
    let info = |scheme: &ColorScheme| ColorSchemeInfo {
        id: scheme.id.clone(),
        name: scheme.about.name.clone(),
        version: scheme.about.version.clone(),
        author: scheme.about.author.clone(),
        description: scheme.about.description.clone(),
        standard: scheme.is_standard(),
        valid: true,
        problems: Vec::new(),
        active: false,
        dark_only: scheme.dark_only,
    };
    let sample = |id: &str, about| ColorScheme { id: id.into(), about, ..ColorScheme::standard() };
    let mut harbor = sample("harbor", sample_about("Harbor", "1.2.0", "Teal accent, cool surfaces"));
    let dark = &mut harbor.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x0A0E10), Rgba::hex(0x101518), Rgba::hex(0x151B1F));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x14B8A6), Rgba::hex(0x042F2C));
    let light = &mut harbor.colors.light;
    (light.bg, light.panel) = (Rgba::hex(0xEEF3F4), Rgba::hex(0xFFFFFF));
    (light.accent, light.on_accent) = (Rgba::hex(0x0B7A72), Rgba::hex(0xFFFFFF));
    let mut paper = sample("paper", sample_about("Paper", "0.9.1", "Warm paper, ink blue"));
    let dark = &mut paper.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x12100D), Rgba::hex(0x1A1714), Rgba::hex(0x201C18));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x7C9CF5), Rgba::hex(0x0B1530));
    let light = &mut paper.colors.light;
    (light.bg, light.panel, light.raised) = (Rgba::hex(0xF3EEE4), Rgba::hex(0xFFFCF6), Rgba::hex(0xF8F4EC));
    (light.accent, light.on_accent) = (Rgba::hex(0x2A4FC4), Rgba::hex(0xFFFFFF));
    let neon = ColorSchemeInfo {
        id: "neon".into(),
        name: "Neon".into(),
        version: "0.3.0".into(),
        author: Some("Torro samples".into()),
        description: None,
        standard: false,
        valid: false,
        problems: [
            DesignError::TextContrast { fg: "colors.dark.text_3".into(), on: "panel".into(), ratio: 3.12 },
            DesignError::UnknownKey { key: "colors.dark.acent".into() },
            DesignError::AccentContrast { mode: Mode::Light, ratio: 2.4 },
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        active: false,
        dark_only: false,
    };
    // Dark only: the light mode is a copy of the dark one, as the core reads such a scheme.
    let mut midnight = sample("midnight", sample_about("Midnight", "1.0.0", "Deep blue, dark only"));
    let dark = &mut midnight.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x070B14), Rgba::hex(0x0C1220), Rgba::hex(0x111A2C));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x6E8BFF), Rgba::hex(0x0A1030));
    midnight.colors.light = midnight.colors.dark;
    midnight.dark_only = true;
    let mut schemes = vec![ColorScheme::standard(), harbor, paper, midnight];
    let forced = torro_gui::scheme::active();
    if !schemes.iter().any(|scheme| scheme.id == forced.id) {
        schemes.push((*forced).clone());
    }
    let mut entries: Vec<SchemeEntry> =
        schemes.into_iter().map(|scheme| Entry::new(info(&scheme), Some(scheme))).collect();
    entries.push(Entry::new(neon, None));
    entries
}

/// The entry the core lists while a Nextcloud sign-in runs (empty id, the login link), for the
/// setup's waiting state (`--open setup-nextcloud-waiting`).
pub fn demo_signing_in() -> AccountInfo {
    AccountInfo {
        id: String::new(),
        kind: "nextcloud".into(),
        label: "cloud.example.org".into(),
        state: AccountState::SigningIn {
            server: "https://cloud.example.org".into(),
            login_url: "https://cloud.example.org/login/v2/flow/demo".into(),
        },
        collections: Vec::new(),
        scopes: None,
        mail: None,
        ntfy: None,
        services: None,
    }
}

/// The sample content of `--demo`: a healthy Nextcloud account with a shared read-only calendar,
/// a second one that cannot reach its server (so the problem rows show), a GitHub account, three
/// templates, the suite's apps, settings with one list hidden, approvals: two open, one done,
/// one failed, one denied, one expired ([`demo_approvals`]), and five clients with the calendars,
/// lists (one a GitHub filter list) and inbox sources their selections choose from
/// ([`demo_clients`]), and inbox rules: two remembered lanes, the muted source of the sources
/// list, a hidden sender and domain and two muted traits, with what each caught ([`demo_rules`],
/// [`demo_hits`]). Times are relative to now.
pub fn demo_data() -> Data {
    let now = Utc::now();
    let (mut sample, _, _) = torro_pim::demo::sample(Local::now().date_naive());
    // A calendar shared by someone else: shown, but read-only.
    if let Some(shared) = sample.iter_mut().find(|collection| collection.name == "Family") {
        shared.writable = false;
    }
    let hidden_collections: Vec<String> = sample
        .iter()
        .filter(|collection| collection.name == "Groceries")
        .map(|collection| collection.id.clone())
        .collect();
    let team = |id: &str, name: &str, kind, color: &str| Collection {
        id: id.into(),
        name: name.into(),
        kind,
        color: Some(color.into()),
        writable: true,
        source: None,
    };
    let accounts = vec![
        AccountInfo {
            id: "nextcloud:anna@cloud.example.org".into(),
            kind: "nextcloud".into(),
            label: "anna@cloud.example.org".into(),
            state: AccountState::Connected {
                server: "https://cloud.example.org".into(),
                login_name: "anna".into(),
                last_sync: Some(now - Duration::minutes(4)),
                problem: None,
            },
            collections: sample,
            scopes: None,
            mail: None,
            ntfy: None,
            // What the server offers (task W.1): calendars, tasks, notifications, Talk and push; no Deck.
            services: Some(vec![
                ServerService::Calendars,
                ServerService::Tasks,
                ServerService::Notifications,
                ServerService::Talk,
                ServerService::Push,
            ]),
        },
        AccountInfo {
            id: "nextcloud:anna@work.example.net".into(),
            kind: "nextcloud".into(),
            label: "anna@work.example.net".into(),
            state: AccountState::Connected {
                server: "https://work.example.net".into(),
                login_name: "anna".into(),
                last_sync: Some(now - Duration::days(2)),
                problem: Some("The server did not answer".into()),
            },
            collections: vec![
                team("demo:/work/calendars/team/", "Team", CollectionKind::Events, "#5B8CFF"),
                team("demo:/work/calendars/ops/", "Ops", CollectionKind::Todos, "#E8A33D"),
            ],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        },
        // A GitHub account whose token reads private repositories but has no projects scope, so
        // the detail page shows what is allowed and what is missing.
        AccountInfo {
            id: "github:anna-dev".into(),
            kind: "github".into(),
            label: "anna-dev".into(),
            state: AccountState::Connected {
                server: torro_core::account::GITHUB_SERVER.into(),
                login_name: "anna-dev".into(),
                last_sync: Some(now - Duration::minutes(12)),
                problem: None,
            },
            collections: Vec::new(),
            scopes: Some(vec!["repo".into(), "read:org".into(), "workflow".into()]),
            mail: None,
            ntfy: None,
            services: None,
        },
        // A mail provider's calendars and tasks (task M7.2). Another sign-in than the mail accounts
        // below: the core never lists a CalDAV account beside the mail account of the same
        // sign-in, it takes it over (task M3.5).
        AccountInfo {
            id: "caldav:anna@posteo.de@posteo.de".into(),
            kind: "caldav".into(),
            label: "anna@posteo.de".into(),
            state: AccountState::Connected {
                server: "https://posteo.de:8443/".into(),
                login_name: "anna@posteo.de".into(),
                last_sync: Some(now - Duration::minutes(9)),
                problem: None,
            },
            collections: vec![
                team("demo:/posteo/calendars/private/", "Private", CollectionKind::Events, "#3FB984"),
                team("demo:/posteo/tasks/inbox/", "Inbox", CollectionKind::Todos, "#A78BFA"),
            ],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        },
        // Two mail accounts (tasks M3.4, M5.2), so the agents' "Mail accounts" have something to
        // choose from and the account page shows both kinds: one with calendars and tasks, a
        // signature and a folder role the server left open, one that is mail only and cannot
        // reach its server. Their ids are `mail:<address>`, as the core makes them.
        mailbox_account(now),
        work_mail_account(now),
        // An ntfy server with a login and three topics (task E5.14).
        ntfy_account(&[("backups-7f3a", "Backups"), ("ci-anna-91c2", "CI builds"), ("home-4d1e", "Home server")], now),
        // An AI account in the cloud (task KI.1a); the demo finds Ollama on this computer
        // unconnected, so the add-account dialog shows both kinds of local rows.
        AccountInfo {
            id: "ai:openrouter@openrouter.ai".into(),
            kind: "ai".into(),
            label: "OpenRouter".into(),
            state: AccountState::Connected {
                server: "https://openrouter.ai/api/v1".into(),
                login_name: String::new(),
                last_sync: Some(now - Duration::minutes(2)),
                problem: None,
            },
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        },
    ];
    let template = |id: &str, name: &str, windows, age: Duration| TemplateSummary {
        id: id.into(),
        name: name.into(),
        windows,
        updated: now - age,
        shortcut: None,
        startup: None,
        workspace: "3".into(),
        monitor: Some("DP-1".into()),
    };
    // Issue #243: keys 1 and 2, and the first loads at every login.
    let templates = vec![
        TemplateSummary {
            shortcut: Some(1),
            startup: Some(torro_setups::Startup { screen: "DP-1".into(), fallback: Default::default() }),
            ..template("development", "Development", 6, Duration::days(2))
        },
        TemplateSummary { shortcut: Some(2), ..template("writing", "Writing", 3, Duration::days(9)) },
        template("review-call", "Review call", 4, Duration::hours(3)),
    ];
    let apps = AppId::ALL
        .iter()
        .filter(|app| **app != AppId::WORKPLACE)
        .map(|app| AppInfo {
            binary: app.binary.into(),
            name: app.display_name.into(),
            window_class: app.window_class.into(),
            installed: true,
        })
        .collect();
    let calendars: Vec<Collection> = accounts
        .iter()
        .flat_map(|account| &account.collections)
        .filter(|collection| collection.kind.has_events())
        .cloned()
        .collect();
    let mut task_lists: Vec<Collection> = accounts
        .iter()
        .flat_map(|account| &account.collections)
        .filter(|collection| collection.kind.has_todos())
        .cloned()
        .collect();
    task_lists.push(Collection {
        id: "github:anna-dev/1".into(),
        name: "Torro bugs".into(),
        kind: CollectionKind::Todos,
        color: Some("#D96CA8".into()),
        writable: false,
        source: Some(GitHubFilter {
            queries: vec![
                "is:open is:issue repo:anna-dev/torro-workplace label:bug".into(),
                "is:open is:pr org:acme review-requested:@me".into(),
            ],
            pinned: Vec::new(),
            revision: 1,
            truncated: false,
            problem: None,
        }),
    });
    let first_calendar = calendars.first().map(|calendar| calendar.id.clone());
    let sources = vec![
        SourceInfo { source: SourceRef::suite(), open: 2, muted: false, health: None },
        SourceInfo { source: SourceRef::agent("claude-code"), open: 3, muted: false, health: None },
        SourceInfo { source: SourceRef::script("backup"), open: 1, muted: false, health: None },
        SourceInfo { source: SourceRef::script("nightly-build"), open: 0, muted: true, health: None },
        // The remote sources (task E5.14): Talk of the first Nextcloud muted, the second Nextcloud
        // offline like its account.
        SourceInfo { source: SourceRef::github("anna-dev"), open: 4, muted: false, health: None },
        SourceInfo { source: SourceRef::nextcloud("anna@cloud.example.org"), open: 2, muted: false, health: None },
        SourceInfo { source: SourceRef::talk("anna@cloud.example.org"), open: 0, muted: true, health: None },
        SourceInfo {
            source: SourceRef::nextcloud("anna@work.example.net"),
            open: 0,
            muted: false,
            health: Some(SourceHealth::Offline),
        },
        SourceInfo { source: SourceRef::ntfy("push.example.org"), open: 1, muted: false, health: None },
    ];
    Data {
        clients: demo_clients(now, first_calendar),
        calendars,
        task_lists,
        sources,
        tools: demo_tools(&torro_core::settings::Tools::default()),
        status: Some(Status {
            version: env!("CARGO_PKG_VERSION").into(),
            backend: "hyprland".into(),
            workspace: Some("3".into()),
            templates: templates.len(),
            busy: false,
            wm_version: Some("0.56.0".into()),
            shortcuts: Some(torro_ipc::ShortcutStatus {
                modifiers: torro_core::settings::ShortcutModifiers::default(),
                active: true,
                problem: None,
            }),
        }),
        accounts,
        templates,
        settings: Settings {
            start_at_login: true,
            hidden_collections,
            // Issue #131: new chats start with a model of the connected AI account, thinking hard.
            assistant_default: Some(torro_core::settings::AssistantDefault {
                engine: "ai:openrouter@openrouter.ai".into(),
                model: Some("qwen/qwen3-32b".into()),
                effort: Some(torro_core::ai::Effort::High),
            }),
            ..Settings::default()
        },
        apps: Some(apps),
        approvals: demo_approvals(now),
        engines: demo_engines(),
        screens: demo_screens(),
        extensions: super::connections::Listing::Entries(demo_extensions()),
        catalog: super::connections::Listing::Entries(demo_catalog()),
        search_providers: demo_search_providers(),
        error: None,
    }
}

/// The built-in catalog as the core lists it on a computer without Node.js and with only two of the ArtCraft
/// apps: Sequential Thinking needs `npx` (task MA.8 shows it without "Install…"), DeckCraft and PdfCraft their
/// `<app>-cli` (issue #303). The others need nothing the demo lacks.
fn demo_catalog() -> Vec<torro_ipc::extensions::CatalogEntry> {
    use torro_ipc::extensions::Prerequisite;
    let entries = crate::extensions::catalog::builtin().unwrap_or_default();
    entries
        .iter()
        .map(|entry| {
            let mut info = entry.info.clone();
            if info.requires.contains(&Prerequisite::Npx) || matches!(info.id.as_str(), "deckcraft" | "pdfcraft") {
                info.missing = info.requires.clone();
            }
            info
        })
        .collect()
}

/// The demo's MCP servers (task MA.7): "Time" of the catalog, on and running with its two tools
/// allowed; "GitHub" of the catalog, stopped after errors, its token stored; and "Notes", a server of
/// one's own that waits for its tools to be allowed. Nothing runs: the demo starts no program.
fn demo_extensions() -> Vec<torro_ipc::extensions::ExtensionInfo> {
    use torro_ipc::api::Access;
    use torro_ipc::extensions::{
        CrashReason, ExtensionInfo, ExtensionTool, SecretInfo, ServerSpec, Source, Status, ToolState, Value,
    };
    let catalog = demo_catalog();
    let version = |id: &str| catalog.iter().find(|entry| entry.id == id).map(|entry| entry.version.clone());
    let tool = |server: &str, server_name: &str, name: &str, description: &str, access, state| ExtensionTool {
        extension: server.into(),
        extension_name: server_name.into(),
        name: name.into(),
        tool_name: torro_ipc::extensions::tool_name(server, name),
        description: description.into(),
        input_schema: serde_json::json!({ "type": "object" }),
        access,
        state,
    };
    let program = |command: &str, args: &[&str], env: BTreeMap<String, Value>| ServerSpec::Stdio {
        command: command.into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env,
    };
    vec![
        ExtensionInfo {
            id: "time".into(),
            name: "Time".into(),
            source: Source::Catalog { entry: "time".into(), version: version("time").unwrap_or_default() },
            server: program("uvx", &["mcp-server-time"], BTreeMap::new()),
            enabled: true,
            status: Status::Running,
            tools: vec![
                tool(
                    "time",
                    "Time",
                    "get_current_time",
                    "Get the current time in a time zone",
                    Access::Read,
                    ToolState::Allowed,
                ),
                tool(
                    "time",
                    "Time",
                    "convert_time",
                    "Convert a time between time zones",
                    Access::Read,
                    ToolState::Allowed,
                ),
            ],
            secrets: Vec::new(),
            notes: Vec::new(),
            fingerprint: Some("demo-time".into()),
            folder: None,
            folder_unusable: false,
        },
        ExtensionInfo {
            id: "github".into(),
            name: "GitHub".into(),
            source: Source::Catalog { entry: "github".into(), version: version("github").unwrap_or_default() },
            server: program(
                "docker",
                &["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
                BTreeMap::from([("GITHUB_PERSONAL_ACCESS_TOKEN".into(), Value::Stored { secret: true })]),
            ),
            enabled: true,
            status: Status::Crashed { reason: CrashReason::Exited },
            tools: vec![tool(
                "github",
                "GitHub",
                "list_issues",
                "List issues of a repository",
                Access::Read,
                ToolState::Allowed,
            )],
            secrets: vec![SecretInfo { name: "GITHUB_PERSONAL_ACCESS_TOKEN".into(), stored: true }],
            notes: Vec::new(),
            fingerprint: Some("demo-github".into()),
            folder: None,
            folder_unusable: false,
        },
        ExtensionInfo {
            id: "notes".into(),
            name: "Notes".into(),
            source: Source::Custom,
            server: program("/usr/local/bin/notes-mcp", &["--stdio"], BTreeMap::new()),
            enabled: false,
            status: Status::Review,
            tools: vec![
                tool("notes", "Notes", "search_notes", "Search the notes", Access::Delete, ToolState::Review),
                tool("notes", "Notes", "add_note", "Add a note", Access::Delete, ToolState::Review),
            ],
            secrets: Vec::new(),
            notes: Vec::new(),
            fingerprint: Some("demo-notes".into()),
            folder: None,
            folder_unusable: false,
        },
        // Issue #303: WordCraft works in an allowed folder; GridCraft has none chosen yet.
        ExtensionInfo {
            id: "wordcraft".into(),
            name: "WordCraft".into(),
            source: Source::Catalog { entry: "wordcraft".into(), version: version("wordcraft").unwrap_or_default() },
            server: program("wordcraft-cli", &["mcp"], BTreeMap::new()),
            enabled: true,
            status: Status::Stopped,
            tools: vec![
                tool(
                    "wordcraft",
                    "WordCraft",
                    "get_text",
                    "Read the text of the document",
                    Access::Read,
                    ToolState::Allowed,
                ),
                tool("wordcraft", "WordCraft", "open_document", "Open a .docx file", Access::Read, ToolState::Allowed),
                tool("wordcraft", "WordCraft", "save_document", "Save the document", Access::Write, ToolState::Allowed),
                tool("wordcraft", "WordCraft", "execute", "Run a command", Access::Delete, ToolState::Allowed),
                tool("wordcraft", "WordCraft", "screenshot", "Take a screenshot", Access::Delete, ToolState::Blocked),
                tool("wordcraft", "WordCraft", "click", "Click in the window", Access::Delete, ToolState::Blocked),
            ],
            secrets: Vec::new(),
            notes: vec![demo_blocked_note(&["screenshot", "click"])],
            fingerprint: Some("demo-wordcraft".into()),
            folder: Some("/home/anna/Projects/Invoices".into()),
            folder_unusable: false,
        },
        ExtensionInfo {
            id: "gridcraft".into(),
            name: "GridCraft".into(),
            source: Source::Catalog { entry: "gridcraft".into(), version: version("gridcraft").unwrap_or_default() },
            server: program("gridcraft-cli", &["mcp"], BTreeMap::new()),
            enabled: true,
            status: Status::Stopped,
            tools: vec![
                tool("gridcraft", "GridCraft", "read_range", "Read a range of cells", Access::Read, ToolState::Allowed),
                tool(
                    "gridcraft",
                    "GridCraft",
                    "save_workbook",
                    "Save the workbook",
                    Access::Delete,
                    ToolState::Allowed,
                ),
                tool("gridcraft", "GridCraft", "screenshot", "Take a screenshot", Access::Delete, ToolState::Blocked),
                tool("gridcraft", "GridCraft", "execute_command", "Run a command", Access::Delete, ToolState::Blocked),
            ],
            secrets: Vec::new(),
            notes: vec![demo_blocked_note(&["screenshot", "execute_command"])],
            fingerprint: Some("demo-gridcraft".into()),
            folder: None,
            folder_unusable: false,
        },
    ]
}

/// The note the core gives a server with blocked tools (issue #303), for the demo's ArtCraft apps.
fn demo_blocked_note(tools: &[&str]) -> String {
    trf(
        "Torro blocks these tools: {tools}. No one may use them: they work the app's window, run scripts, reach \
         keys of your keychain, or take paths Torro can't check.",
        &[("tools", &tools.join(", "))],
    )
}

/// `--open connections` (task MA.7): the PDF tool on, a folder of its own and a logo in the report
/// template, so every row of "Built-in tools" and the templates shows; the tool `web` on with DuckDuckGo
/// and Wikipedia (task WEB.6); the tool `files` on with two allowed folders, one to read and one to write
/// (task WD.4). Paths only: the demo reads no file and stores no key.
pub fn demo_connections(data: &mut Data) {
    use torro_core::settings::{AllowedFolder, FolderAccess, PdfTemplateId, ToolId};
    use torro_ipc::web::SearchProviderId;
    let tools = &mut data.settings.tools;
    tools.set_on(ToolId::Pdf, true);
    tools.set_on(ToolId::Web, true);
    tools.set_on(ToolId::Files, true);
    tools.folders = vec![
        AllowedFolder { path: "/home/anna/Documents".into(), access: FolderAccess::Read },
        AllowedFolder { path: "/home/anna/Projects/Invoices".into(), access: FolderAccess::ReadWrite },
    ];
    tools.pdf.folder = Some("/home/anna/Documents/PDFs".into());
    tools.pdf.template_mut(PdfTemplateId::Report).logo = Some("/home/anna/Pictures/acme-logo.png".into());
    data.tools = demo_tools(&data.settings.tools);
    for info in &mut data.search_providers {
        if matches!(info.id, SearchProviderId::Duckduckgo | SearchProviderId::Wikipedia) {
            info.enabled = true;
        }
    }
}

/// `--open connections-empty` (task MA.7): no server installed, the PDF tool off as it starts, and
/// an assistant that may not use it, so the row leads to its Access.
pub fn demo_connections_empty(data: &mut Data) {
    data.extensions = super::connections::Listing::Entries(Vec::new());
    for client in data.clients.iter_mut().filter(|client| client.kind == ASSISTANT_CLIENT) {
        client.rights.set_level(Area::Tools, Level::NoAccess, Vec::new);
    }
}

/// `--open catalog-install` (task MA.8): the demo's GitHub server taken out, so its catalog entry,
/// which needs nothing the demo lacks, can be installed.
pub fn demo_uninstalled(data: &mut Data) {
    if let super::connections::Listing::Entries(servers) = &mut data.extensions {
        servers.retain(|server| server.id != "github");
    }
}

/// `--open setup-documents` (issue #303): the step Documents with one ArtCraft app of each state – WordCraft set
/// up, GridCraft installed but not set up (its server taken out), DeckCraft and PdfCraft missing.
pub fn demo_documents(data: &mut Data) {
    if let super::connections::Listing::Entries(servers) = &mut data.extensions {
        servers.retain(|server| server.id != "gridcraft");
    }
}

/// `--open extension-review` (task MA.8): "Time" reported a changed `convert_time` since its tools were
/// allowed, so that one waits and the others stay allowed.
pub fn demo_review(data: &mut Data) {
    use torro_ipc::extensions::{Status as ServerStatus, ToolState};
    let super::connections::Listing::Entries(servers) = &mut data.extensions else { return };
    let Some(time) = servers.iter_mut().find(|server| server.id == "time") else { return };
    time.status = ServerStatus::Review;
    for tool in time.tools.iter_mut().filter(|tool| tool.name == "convert_time") {
        tool.state = ToolState::Review;
        tool.description = "Convert a time between time zones, also for a date in the past".into();
    }
    time.fingerprint = Some("demo-time-changed".into());
}

/// The `mcpServers` block `--open custom-server` pastes (task MA.9): one server, a command with
/// arguments, an API key (marked secret by its name) and a plain variable. Not a real key.
pub const DEMO_PASTE_ONE: &str = r#"{
  "mcpServers": {
    "weather": {
      "command": "uvx",
      "args": ["mcp-weather", "--units", "metric"],
      "env": { "WEATHER_API_KEY": "demo-not-a-real-key", "WEATHER_REGION": "eu" }
    }
  }
}"#;

/// The block `--open custom-server-paste` and `custom-server-choose` paste: a command and an address
/// with a sign-in header. Not a real token.
pub const DEMO_PASTE_TWO: &str = r#"{
  "mcpServers": {
    "weather": {
      "command": "uvx",
      "args": ["mcp-weather"],
      "env": { "WEATHER_API_KEY": "demo-not-a-real-key" }
    },
    "team-wiki": {
      "type": "http",
      "url": "https://wiki.example.org/mcp",
      "headers": { "Authorization": "Bearer demo-not-a-real-token" }
    }
  }
}"#;

/// What the demo answers to "Add server" for a server of one's own (task MA.9): the server as the
/// core would store it – switched off, every secret stored and never sent back, two tools waiting
/// for a person. Nothing starts and nothing is kept.
fn demo_custom(
    id: &str,
    name: &str,
    server: &torro_ipc::extensions::ServerSpec,
) -> torro_ipc::extensions::ExtensionInfo {
    use torro_ipc::api::Access;
    use torro_ipc::extensions::{
        ExtensionInfo, ExtensionTool, SecretInfo, ServerSpec, Source, Status as ServerStatus, ToolState, Value,
    };
    let stored = |values: &BTreeMap<String, Value>| -> BTreeMap<String, Value> {
        values
            .iter()
            .map(|(name, value)| match value {
                Value::Plain(_) => (name.clone(), value.clone()),
                Value::Secret(_) | Value::Stored { .. } => (name.clone(), Value::Stored { secret: true }),
            })
            .collect()
    };
    let spec = match server {
        ServerSpec::Stdio { command, args, env } => {
            ServerSpec::Stdio { command: command.clone(), args: args.clone(), env: stored(env) }
        }
        ServerSpec::Http { url, headers } => ServerSpec::Http { url: url.clone(), headers: stored(headers) },
    };
    let secrets = server
        .values()
        .iter()
        .filter(|(_, value)| !matches!(value, Value::Plain(_)))
        .map(|(name, _)| SecretInfo { name: name.clone(), stored: true })
        .collect();
    let tool = |tool: &str, description: &str| ExtensionTool {
        extension: id.into(),
        extension_name: name.into(),
        name: tool.into(),
        tool_name: torro_ipc::extensions::tool_name(id, tool),
        description: description.into(),
        input_schema: serde_json::json!({ "type": "object" }),
        access: Access::Delete,
        state: ToolState::Review,
    };
    ExtensionInfo {
        id: id.into(),
        name: name.into(),
        source: Source::Custom,
        server: spec,
        enabled: false,
        status: ServerStatus::Review,
        tools: vec![tool("search", "Search what the server knows"), tool("fetch", "Fetch one entry by its id")],
        secrets,
        notes: Vec::new(),
        fingerprint: Some(format!("demo-{id}")),
        folder: None,
        folder_unusable: false,
    }
}

/// What the demo answers for the server `id` of an `extensions.*` request (task MA.8): the server as
/// the demo lists it, else the catalog entry of that id as just installed – switched off, its tools
/// as the catalog names them waiting for a person, its secrets stored. Nothing is kept.
fn demo_server(id: &str, data: &Data) -> Option<torro_ipc::extensions::ExtensionInfo> {
    use torro_ipc::extensions::{
        ExtensionInfo, ExtensionTool, SecretInfo, ServerSpec, Source, Status as ServerStatus, ToolState,
    };
    if let Some(server) = data.extensions.entries().iter().find(|server| server.id == id) {
        return Some(server.clone());
    }
    let entry = data.catalog.entries().iter().find(|entry| entry.id == id)?;
    let tools = entry
        .tools
        .iter()
        .map(|tool| ExtensionTool {
            extension: entry.id.clone(),
            extension_name: entry.name.clone(),
            name: tool.name.clone(),
            tool_name: torro_ipc::extensions::tool_name(&entry.id, &tool.name),
            description: String::new(),
            input_schema: serde_json::json!({ "type": "object" }),
            access: tool.access,
            state: ToolState::Review,
        })
        .collect();
    Some(ExtensionInfo {
        id: entry.id.clone(),
        name: entry.name.clone(),
        source: Source::Catalog { entry: entry.id.clone(), version: entry.version.clone() },
        server: ServerSpec::Stdio { command: "demo".into(), args: Vec::new(), env: BTreeMap::new() },
        enabled: false,
        status: ServerStatus::Review,
        tools,
        secrets: entry.secrets.iter().map(|secret| SecretInfo { name: secret.name.clone(), stored: true }).collect(),
        notes: Vec::new(),
        fingerprint: Some(format!("demo-{}", entry.id)),
        folder: None,
        folder_unusable: false,
    })
}

/// The demo's output of the server `id`: what such a server writes to stderr. GitHub's, stopped after
/// errors, says why: the token was refused.
fn demo_logs(id: &str) -> Vec<String> {
    let lines: &[&str] = match id {
        "github" => &[
            "time=2026-10-24T09:12:03Z level=INFO msg=\"starting server\" version=2.0.3",
            "time=2026-10-24T09:12:04Z level=ERROR msg=\"failed to get user\" error=\"401 Bad credentials\"",
            "GitHub MCP Server running on stdio",
            "Error: unauthorized: check GITHUB_PERSONAL_ACCESS_TOKEN",
        ],
        "time" => &[
            "Starting MCP server for time (local time zone: Europe/Berlin)",
            "Processing request of type ListToolsRequest",
        ],
        _ => &[],
    };
    lines.iter().map(|line| line.to_string()).collect()
}

/// The web search providers as the core lists them for a first setup (task WEB.7): every one off, no key stored,
/// no SearXNG address – what the setup's step "Web search" starts from.
fn demo_search_providers() -> Vec<torro_ipc::web::SearchProviderInfo> {
    let settings = torro_core::settings::WebSearch::default();
    torro_ipc::web::SearchProviderId::ORDER
        .into_iter()
        .map(|id| crate::tools::web::catalogue::info(id, &settings, false, None))
        .collect()
}

/// The demo's screens (task WS.6): the wide main screen the templates were saved on, and a
/// second one beside it, in logical pixels.
pub fn demo_screens() -> Vec<ScreenInfo> {
    vec![
        ScreenInfo { name: "DP-1".into(), width: 3840, height: 1080, scale: 2.0, main: true },
        ScreenInfo { name: "HDMI-A-1".into(), width: 1920, height: 1080, scale: 1.0, main: false },
    ]
}

/// The template the demo's "Save workspace…" saves (task WS.6): workspace 3 on the main screen,
/// named as the core names a template without a name.
pub fn demo_saved() -> TemplateSummary {
    let now = Utc::now();
    TemplateSummary {
        id: "workspace-3".into(),
        name: torro_setups::capture::auto_name("3", Local::now()),
        windows: 5,
        updated: now,
        shortcut: None,
        startup: None,
        workspace: "3".into(),
        monitor: Some("DP-1".into()),
    }
}

/// The assistant's own tools as the core lists them for the app key (task WZ.7), switched on as
/// `settings` says; the demo's settings leave PDF off, so the rights editor shows that state too.
fn demo_tools(settings: &torro_core::settings::Tools) -> Vec<torro_ipc::tools::ToolInfo> {
    match crate::tools::list(settings, crate::pim::Reach::Suite) {
        torro_ipc::Response::Tools { tools } => tools,
        _ => Vec::new(),
    }
}

/// The engines of the demo's assistant (issue #131): the one connected AI account, as the core
/// lists them — without models, which come from `ai.list_models`. The demo's Ollama is found but
/// not connected, so it is no engine. No coding agent either: they are not offered any more (task
/// C.19, issue #219), and the demo shows what the product can do.
fn demo_engines() -> Vec<torro_ipc::assistant::Engine> {
    use torro_ipc::assistant::{Engine, EngineKind};
    vec![Engine {
        id: "ai:openrouter@openrouter.ai".into(),
        kind: EngineKind::Ai,
        name: "OpenRouter".into(),
        problem: None,
        models: Vec::new(),
    }]
}

/// The demo's ntfy account at `push.example.org` (task E5.14), signed in as `anna` with a password,
/// subscribed to `topics` (name, label); the control centre has the app key, so it sees the names.
fn ntfy_account(topics: &[(&str, &str)], now: DateTime<Utc>) -> AccountInfo {
    let server = "https://push.example.org";
    let topics = topics
        .iter()
        .map(|(topic, label)| NtfyTopicView { label: label.to_string(), topic: Some(topic.to_string()) })
        .collect();
    AccountInfo {
        id: "ntfy:push.example.org".into(),
        kind: "ntfy".into(),
        label: "anna@push.example.org".into(),
        state: AccountState::Connected {
            server: server.into(),
            login_name: "anna".into(),
            last_sync: Some(now - Duration::minutes(1)),
            problem: None,
        },
        collections: Vec::new(),
        scopes: None,
        mail: None,
        ntfy: Some(NtfyView { server: server.into(), topics, user: Some("anna".into()), login: true }),
        services: None,
    }
}

/// A connected mail account of the demo: no calendars or lists, as long as it is mail only. Its
/// SMTP server is the IMAP server's name with `smtp` for `imap`.
fn mail_account(address: &str, server: &str, last_sync: DateTime<Utc>) -> AccountInfo {
    let endpoint = |host: String, port, security| Endpoint { host, port, security };
    let view = MailView {
        address: address.into(),
        sender_name: String::new(),
        imap: endpoint(server.into(), 993, Security::Tls),
        smtp: endpoint(server.replacen("imap", "smtp", 1), 587, Security::StartTls),
        caldav: None,
        settings: MailSettings::default(),
        open_roles: Vec::new(),
        mail: ServiceState { last_sync: Some(last_sync), problem: None },
        calendars: None,
        trusted_certificates: Vec::new(),
    };
    AccountInfo {
        id: format!("mail:{address}"),
        kind: "mail".into(),
        label: address.into(),
        state: AccountState::Connected {
            server: format!("https://{server}"),
            login_name: address.into(),
            last_sync: Some(last_sync),
            problem: None,
        },
        collections: Vec::new(),
        scopes: None,
        mail: Some(view),
        ntfy: None,
        services: None,
    }
}

/// The first demo mail account: mail, calendars and tasks all on and synced, a signature, only
/// mail "For me" in the inbox, and the question which folder holds sent mail with two candidates.
/// Its calendars are its own: no CalDAV account of the same sign-in stands beside it (task M3.5).
fn mailbox_account(now: DateTime<Utc>) -> AccountInfo {
    let mut account = mail_account("anna@mailbox.org", "imap.mailbox.org", now - Duration::minutes(2));
    if let Some(mail) = account.mail.as_mut() {
        mail.sender_name = "Anna Example".into();
        mail.caldav = Some("https://dav.mailbox.org/".into());
        mail.settings = MailSettings {
            signature: Some("Anna Example\nExample GmbH".into()),
            notify: Notify::ForMeOnly,
            services: Services { mail: true, calendars: true, tasks: true },
            ..MailSettings::default()
        };
        mail.open_roles = vec![OpenRoleView {
            role: Role::Sent,
            candidates: vec![
                FolderChoice { name: "Sent".into(), path: "INBOX.Sent".into() },
                FolderChoice { name: "Sent Items".into(), path: "INBOX.Sent Items".into() },
            ],
        }];
        mail.calendars = Some(ServiceState { last_sync: Some(now - Duration::minutes(9)), problem: None });
    }
    account
}

/// The second demo mail account: mail only, and the server has not answered for two days. It is
/// the firm's own server, whose certificate a person trusted (task M11.4), so its page shows the row.
fn work_mail_account(now: DateTime<Utc>) -> AccountInfo {
    let mut account = mail_account("anna@work.example.net", "imap.work.example.net", now - Duration::days(2));
    let problem = "The server did not answer".to_string();
    if let AccountState::Connected { problem: state_problem, .. } = &mut account.state {
        *state_problem = Some(problem.clone());
    }
    if let Some(mail) = account.mail.as_mut() {
        mail.mail.problem = Some(problem);
        mail.trusted_certificates = vec![DEMO_FINGERPRINT.into()];
    }
    account
}

/// The demo's clients: Claude Code with the default template, Codex with rights of its own
/// (fewer areas, an admin switch, chosen calendars, repositories and sources, asking before every
/// change), the hooks of Claude Code, a script that has never been used, and the assistant
/// (task C.6): more than its first start offered – writing calendars and tasks, reading one mail
/// account – but no sending.
fn demo_clients(now: DateTime<Utc>, first_calendar: Option<String>) -> Vec<ClientInfo> {
    let client = |id: &str, name: &str, kind: &str, rights: Rights, last_used: Option<DateTime<Utc>>| ClientInfo {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        created: now - Duration::days(20),
        last_used,
        rights,
        agent: (kind == HOOK_CLIENT).then_some(AgentKind::ClaudeCode),
    };
    let mut codex = RightsTemplate::ReadAndWrite.rights();
    let set = |rights: &mut Rights, area, level, admin| {
        rights.areas.insert(area, AreaRights { level, admin });
    };
    set(&mut codex, Area::Calendar, Level::Read, false);
    set(&mut codex, Area::Accounts, Level::NoAccess, false);
    set(&mut codex, Area::Settings, Level::NoAccess, false);
    set(&mut codex, Area::Inbox, Level::Write, true);
    // Mail from one account, which it may send from without asking: the page's warning shows.
    set(&mut codex, Area::Mail, Level::Write, false);
    codex.mail_accounts = Selection::Only(vec!["mail:anna@mailbox.org".into()]);
    codex.outbound = Outbound::Free;
    codex.calendars = Selection::Only(first_calendar.into_iter().collect());
    codex.github_repos = Selection::Only(vec!["anna-dev/torro-workplace".into(), "acme".into()]);
    codex.inbox_sources = Selection::Only(vec!["agent:codex".into(), "script:backup".into()]);
    codex.confirm = Confirm::OnEveryChange;
    let backup = Rights {
        areas: BTreeMap::from([(Area::Inbox, AreaRights { level: Level::Write, admin: false })]),
        calendars: Selection::Only(Vec::new()),
        task_lists: Selection::Only(Vec::new()),
        github_repos: Selection::Only(Vec::new()),
        inbox_sources: Selection::Only(vec!["script:backup".into()]),
        mail_accounts: Selection::Only(Vec::new()),
        tools: Selection::Only(Vec::new()),
        outbound: Outbound::Never,
        confirm: Confirm::Never,
        denied: Default::default(),
        extensions: Default::default(),
    };
    let mut assistant = Rights::assistant();
    set(&mut assistant, Area::Calendar, Level::Write, false);
    set(&mut assistant, Area::Tasks, Level::Write, false);
    set(&mut assistant, Area::Mail, Level::Read, false);
    set(&mut assistant, Area::Workspaces, Level::Read, false);
    assistant.mail_accounts = Selection::Only(vec!["mail:anna@mailbox.org".into()]);
    // Its own tools (task WZ.7), so its page shows the selection of tools.
    set(&mut assistant, Area::Tools, Level::Write, false);
    assistant.tools = Selection::Only(vec!["pdf".into()]);
    // The travel planner (task B.3): reads calendars, writes tasks, nothing else.
    let mut travel = Rights::none();
    set(&mut travel, Area::Calendar, Level::Read, false);
    set(&mut travel, Area::Tasks, Level::Write, false);
    vec![
        client(
            DEMO_CLAUDE,
            "Claude Code",
            "claude-code",
            RightsTemplate::ReadAndWrite.rights(),
            Some(now - Duration::minutes(5)),
        ),
        client("7b2e4f10", "Codex", "codex", codex, Some(now - Duration::days(2))),
        client(DEMO_HOOKS, "Claude Code hooks", HOOK_CLIENT, Rights::hook(), Some(now - Duration::minutes(1))),
        client("e8a07b35", "backup", "script", backup, None),
        client("a51c3e88", "Assistant", ASSISTANT_CLIENT, assistant, Some(now - Duration::hours(3))),
        client("c0000001", tr("Travel planner"), BOT_CLIENT, travel, Some(now - Duration::hours(4))),
        client("c0000002", tr("Release helper"), BOT_CLIENT, Rights::none(), None),
    ]
}

/// The voice status of the demo: openWakeWord without its model, `parec`, whisper.cpp with a model,
/// Piper without a voice; two microphones.
fn demo_voice() -> torro_ipc::VoiceStatus {
    use torro_ipc::{VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus};
    let part = |part, program: &str, missing| VoiceProgram { part, program: Some(program.into()), missing };
    VoiceStatus {
        listener: VoiceState::Off,
        installed: true,
        problem: None,
        programs: vec![
            part(VoicePart::WakeWord, "openWakeWord", Some(VoiceMissing::Model)),
            part(VoicePart::Recorder, "parec", None),
            part(VoicePart::SpeechToText, "whisper-cli", None),
            part(VoicePart::Speech, "piper", Some(VoiceMissing::Model)),
        ],
        microphones: vec![
            "alsa_input.pci-0000_00_1f.3.analog-stereo".into(),
            "alsa_input.usb-RODE_NT-USB-00.analog-stereo".into(),
        ],
        folder: Some("~/.local/share/torroassistant/voice".into()),
    }
}

/// The sample answer `--demo` gives to a request whose answer is a state of its own: a client's
/// activity, a new key and a new client of "Connect client" or "Create key" (made-up keys, never
/// working ones; the new client is not added to the list). Every other request gets none and is
/// dropped, so the demo changes nothing.
pub fn demo_answer(request: &Request, data: &Data) -> Option<Response> {
    let client_of = |id: &str| data.clients.iter().find(|client| client.id == id);
    match request {
        Request::DiscoverServices { address } => Some(demo_discovery(address)),
        // AI providers (task KI.1a): Ollama runs on this computer, LM Studio does not; the key
        // "wrong" is refused, every other connection works and stores nothing.
        Request::FindAiServers => Some(Response::AiServers {
            servers: vec![AiServer {
                provider: AiProvider::Ollama,
                url: "http://localhost:11434/v1".into(),
                models: 4,
                account: None,
            }],
        }),
        // The models of an AI account (task S.31), the same for every account; a switch is
        // taken, nothing is stored.
        Request::AiModels { account, .. } => {
            Some(Response::AiModels { account: account.clone(), models: demo_models() })
        }
        Request::SetModelEnabled { .. } => Some(Response::Ok),
        Request::ConnectAi { key, .. } if key.expose() == "wrong" => {
            let refused = AiError::Auth;
            Some(Response::Error { code: refused.code().into(), message: refused.to_string() })
        }
        Request::ConnectAi { provider, server, .. } => {
            let base = match server {
                Some(server) => torro_core::ai::normalize_base(server).ok()?,
                None => provider.default_url()?.to_string(),
            };
            let account = torro_core::Account::ai(*provider, &base);
            Some(Response::Account {
                account: AccountInfo {
                    label: crate::account::ai::label(&account),
                    id: account.id,
                    kind: "ai".into(),
                    state: AccountState::Connected {
                        server: base,
                        login_name: String::new(),
                        last_sync: Some(Utc::now()),
                        problem: None,
                    },
                    collections: Vec::new(),
                    scopes: None,
                    mail: None,
                    ntfy: None,
                    services: None,
                },
            })
        }
        // "wrong" is the password the demo's provider refuses (for mail its IMAP server), "wrong-smtp"
        // the one only its SMTP server refuses; nothing is stored either way.
        Request::ConnectCalDav { password, .. } if password.expose() == "wrong" => {
            Some(Response::Error { code: "auth".into(), message: tr("Password was refused").into() })
        }
        // ntfy (task E5.14): the topics of `push.example.org` need a login, the password "wrong" is
        // refused, a topic is changed at once; nothing is stored.
        Request::ConnectNtfy { server, .. } if server.contains("push.example.org") => {
            Some(NtfyAccountError::Ntfy(NtfyError::Auth).response())
        }
        Request::SetNtfyLogin { secret, .. } if secret.expose() == "wrong" => {
            Some(NtfyAccountError::Ntfy(NtfyError::Auth).response())
        }
        Request::ConnectNtfy { topics, .. } | Request::SetNtfyLogin { topics, .. } => {
            let topics: Vec<(&str, &str)> =
                topics.iter().map(|topic| (topic.topic.as_str(), topic.label.as_str())).collect();
            Some(Response::Account { account: ntfy_account(&topics, Utc::now()) })
        }
        Request::SetNtfyTopic { .. } => Some(Response::Ok),
        // Workspaces (task WS.6): "Save workspace…" saves a sample, so the dialog opens in the mode
        // `Save`; what the dialog sends is taken without changing anything.
        Request::SaveTemplate { .. } => Some(Response::Saved { template: demo_saved() }),
        Request::RenameTemplate { .. }
        | Request::SetTemplateShortcut { .. }
        | Request::SetTemplateStartup { .. }
        | Request::DeleteTemplate { .. } => Some(Response::Ok),
        // Task M11.4: an IMAP server at home (`.lan`) or on the public internet (`.example`) shows a
        // certificate the system does not trust, as long as none is trusted; the check of a
        // certificate comes before the password, as in the core.
        Request::ConnectMail { imap, trusted_certificates, .. }
            if trusted_certificates.is_empty() && (is_home(&imap.host) || is_public_demo(&imap.host)) =>
        {
            Some(Response::UntrustedCertificate {
                service: "IMAP".into(),
                host: imap.host.clone(),
                sha256: DEMO_FINGERPRINT.into(),
                public: is_public_demo(&imap.host),
            })
        }
        Request::ConnectMail { password, .. } if password.expose() == "wrong" => Some(refused("IMAP")),
        Request::ConnectMail { password, .. } if password.expose() == "wrong-smtp" => Some(refused("SMTP")),
        // What the CalDAV address offers is what was asked for, as long as an address was given.
        Request::ConnectMail { address, imap, calendars, tasks, caldav, .. } => Some(Response::MailConnected {
            account: mail_account(address, &imap.host, Utc::now()),
            calendars: *calendars && caldav.is_some(),
            tasks: *tasks && caldav.is_some(),
            open_roles: Vec::new(),
        }),
        // TorroMail (task M7.4): four accounts, one of them connected already; Fastmail's keychain
        // entry cannot be read, so its take-over fails and offers the password field.
        Request::TorroMailAccounts => Some(Response::TorroMailAccounts { accounts: demo_torromail(data) }),
        // Voice (task C.9): every program found but Piper's voice, two microphones; the test hears
        // a sentence. Nothing records.
        Request::VoiceStatus => Some(Response::Voice { voice: demo_voice() }),
        Request::TestVoice => Some(Response::Transcript { text: "What is on my calendar today?".into() }),
        Request::ImportTorroMailAccount { id } if id == DEMO_LOCKED => Some(Response::Error {
            code: "error".into(),
            message: tr("The password could not be read from TorroMail. Enter it to connect.").into(),
        }),
        Request::ImportTorroMailAccount { id } => {
            let account = demo_torromail(data).into_iter().find(|account| &account.id == id)?;
            let imap = account.imap?;
            Some(Response::MailConnected {
                account: mail_account(&account.address, &imap.host, Utc::now()),
                calendars: false,
                tasks: false,
                open_roles: Vec::new(),
            })
        }
        Request::ConnectCalDav { login_name, .. } => {
            let account =
                data.accounts.iter().find(|account| account.kind == "caldav").cloned().map(|mut account| {
                    account.label = login_name.clone();
                    account
                })?;
            Some(Response::Connected { account, calendars: true, tasks: true })
        }
        // MCP servers (task MA.8): installing, allowing, switching and updating answer with the
        // server as the demo has it (or the catalog entry as just installed); nothing changes.
        Request::InstallExtension { from: torro_ipc::extensions::InstallFrom::Catalog { entry, .. } } => {
            Some(Response::Extension { extension: demo_server(entry, data)? })
        }
        // As the core: an id in use is a conflict.
        Request::InstallExtension { from: torro_ipc::extensions::InstallFrom::Custom { id, name, server } } => {
            if data.extensions.entries().iter().any(|server| &server.id == id) {
                let message = format!("a server with the id {id} is installed already");
                return Some(Response::Error { code: "conflict".into(), message });
            }
            Some(Response::Extension { extension: demo_custom(id, name, server) })
        }
        Request::ConfirmExtensionTools { id, .. }
        | Request::GetExtension { id }
        | Request::SetExtensionEnabled { id, .. }
        | Request::UpdateExtension { id, .. } => Some(Response::Extension { extension: demo_server(id, data)? }),
        Request::SetExtensionSecret { .. } | Request::RemoveExtension { .. } => Some(Response::Ok),
        // As the core, without looking at the folder: the server with the folder chosen; nothing is kept.
        Request::SetExtensionFolder { id, folder } => {
            let mut server = demo_server(id, data)?;
            server.folder = folder.clone();
            Some(Response::Extension { extension: server })
        }
        Request::ReadExtensionLogs { id } => Some(Response::Logs { lines: demo_logs(id) }),
        Request::ClientActivity { id } => {
            let client = client_of(id)?;
            Some(Response::Activity { entries: demo_activity(client, Utc::now()) })
        }
        // As the core: the assistant and the bots run inside it, which holds their keys (#231).
        Request::NewClientKey { id } if client_of(id).is_some_and(|client| lent(&client.kind)) => {
            let message = "the assistant and bots get no key: the core runs their tools itself".into();
            Some(Response::Error { code: "unsupported".into(), message })
        }
        Request::NewClientKey { id } => {
            let client = client_of(id)?.clone();
            let key = IssuedKey::new(Key::new(format!("torro_{id}_{}", "5f2c9a1e7b3d4086".repeat(4))));
            Some(Response::ClientKey { client, key })
        }
        Request::CreateClient { name, kind, template, agent, rights } => {
            // The hook client "Connect client" or a page's switch creates for an agent (tasks E2.3,
            // S.35) gets an id of its own, as it would from the core.
            let id = if kind == HOOK_CLIENT { "4c7a19e3" } else { "9d4b7e21" };
            let client = ClientInfo {
                id: id.into(),
                name: name.clone(),
                kind: kind.clone(),
                created: Utc::now(),
                last_used: None,
                rights: rights.clone().unwrap_or_else(|| template.rights()).for_kind(kind),
                agent: *agent,
            };
            // The assistant gets no key (#231): the core answers with the client alone.
            if kind == torro_ipc::api::ASSISTANT_CLIENT {
                return Some(Response::Client { client });
            }
            let key = IssuedKey::new(Key::new(format!("torro_{id}_{}", "a3e81c0f6d2b9754".repeat(4))));
            Some(Response::ClientKey { client, key })
        }
        _ => None,
    }
}

/// Whether a client of `kind` runs inside the core, which holds its key: the assistant and the
/// bots. The core refuses them a new key (`unsupported`), and so does the demo.
fn lent(kind: &str) -> bool {
    kind == ASSISTANT_CLIENT || kind == BOT_CLIENT
}

/// What the demo's `client_setup.json` notes (task S.35): the hooks of Claude Code, set up with
/// its client, in Claude Code's settings. Read through [`crate::client_setup::Job::HookNotes`],
/// which the demo answers with these.
pub fn demo_hook_notes() -> Vec<HookNote> {
    vec![HookNote {
        client: DEMO_HOOKS.into(),
        owner: Some(DEMO_CLAUDE.into()),
        agent: AgentKind::ClaudeCode,
        place: "~/.claude/settings.json".into(),
    }]
}

/// The demo's Claude Code client and its hook client.
pub const DEMO_CLAUDE: &str = "3fa9c1d2";
pub const DEMO_HOOKS: &str = "c41d9e06";

/// The models the demo's AI account offers (task S.31), as OpenRouter lists them: open models with
/// tools and thinking, one on, one that can neither use tools nor decide, a decision model (#231:
/// it has a switch and says "Decides"), and two with closed weights, which the page shows only with
/// "Show closed models".
pub fn demo_models() -> Vec<AiModel> {
    let model = |id: &str, name: &str, context: u64, reasoning: bool, enabled: bool| AiModel {
        name: name.into(),
        tools: Some(true),
        open_weights: Some(true),
        context: Some(context),
        reasoning: Some(reasoning),
        enabled,
        ..AiModel::named(id)
    };
    vec![
        model("qwen/qwen3-235b-a22b", "Qwen3 235B A22B", 131_072, true, true),
        model("deepseek/deepseek-chat-v3.1", "DeepSeek V3.1", 163_840, true, false),
        model("meta-llama/llama-3.3-70b-instruct", "Llama 3.3 70B Instruct", 131_072, false, false),
        model("mistralai/mistral-small-3.2-24b-instruct", "Mistral Small 3.2 24B", 128_000, false, false),
        AiModel { tools: Some(false), ..model("google/gemma-3-27b-it", "Gemma 3 27B", 96_000, false, false) },
        AiModel {
            tools: Some(false),
            decision: Some(true),
            reasoning: None,
            context: None,
            ..model("nimble", "Nimble", 0, false, false)
        },
        AiModel {
            open_weights: Some(false),
            ..model("anthropic/claude-sonnet-4.5", "Claude Sonnet 4.5", 1_000_000, true, false)
        },
        AiModel { open_weights: Some(false), ..model("openai/gpt-5", "GPT-5", 400_000, true, false) },
    ]
}

/// TorroMail's id of the demo account whose keychain entry cannot be read.
pub const DEMO_LOCKED: &str = "tm-fastmail";

/// The accounts the demo's TorroMail has (task M7.4): one the suite can take (Posteo), one whose
/// password cannot be read ([`DEMO_LOCKED`]), Gmail with OAuth (not supported) and the mailbox.org
/// account the demo has connected already, so every kind of row shows.
fn demo_torromail(data: &Data) -> Vec<TorroMailAccount> {
    let server = |host: &str, port, security| Some(Endpoint { host: host.into(), port, security });
    let account = |id: &str, address: &str, provider: &str, imap: &str, smtp: &str| TorroMailAccount {
        id: id.into(),
        address: address.into(),
        sender_name: "Anna Example".into(),
        provider: provider.into(),
        imap: server(imap, 993, Security::Tls),
        smtp: server(smtp, 587, Security::StartTls),
        login_name: address.into(),
        unsupported: None,
        connected: data.accounts.iter().any(|known| known.id == format!("mail:{address}")),
    };
    let mut gmail = account("tm-gmail", "anna.example@gmail.com", "Gmail", "imap.gmail.com", "smtp.gmail.com");
    gmail.unsupported =
        Some(tr("Signs in with Google or Microsoft, which Torro Workplace does not support yet.").into());
    vec![
        account("tm-posteo", "anna.example@posteo.de", "Posteo", "posteo.de", "posteo.de"),
        account(DEMO_LOCKED, "anna@fastmail.com", "Fastmail", "imap.fastmail.com", "smtp.fastmail.com"),
        gmail,
        account("tm-mailbox", "anna@mailbox.org", "mailbox.org", "imap.mailbox.org", "smtp.mailbox.org"),
    ]
}

/// The fingerprint of the certificate the demo's own mail servers show (task M11.4), made up.
pub const DEMO_FINGERPRINT: &str =
    "3F:9A:61:C2:0B:7E:D4:58:A3:19:E6:42:BC:70:5D:8F:21:C9:04:7A:E3:B6:58:1D:9F:62:A0:C7:35:E8:4B:D1";

/// A host of the demo's server at home: on the local network.
fn is_home(host: &str) -> bool {
    host.ends_with(".lan")
}

/// A host of the demo's own server on the public internet.
fn is_public_demo(host: &str) -> bool {
    host.ends_with(".example")
}

/// The answer of a server that refuses the password, naming the server like the core does.
fn refused(server: &'static str) -> Response {
    Response::Error {
        code: "auth".into(),
        message: trf("The {service} server refused the password.", &[("service", &server)]),
    }
}

/// What the demo's search finds for `address`, like the core's table of providers: mailbox.org and
/// iCloud have mail, calendars and tasks, work.example.net only mail, Gmail has servers but wants
/// an app password, Outlook.com is not supported and any other domain answers nothing.
fn demo_discovery(address: &str) -> Response {
    let address = address.trim();
    let domain = address.rsplit_once('@').map_or(address, |(_, domain)| domain).trim().to_lowercase();
    let server = |host: &str, port, security| Some(Endpoint { host: host.into(), port, security });
    let mut provider = domain.clone();
    let mut urls: Vec<String> = Vec::new();
    let (mut imap, mut smtp, mut login_hint) = (None, None, None);
    let (mut app_password, mut unsupported, mut mail_unsupported) = (false, None, None);
    match domain.as_str() {
        "mailbox.org" => {
            provider = "mailbox.org".into();
            urls.push("https://dav.mailbox.org/".into());
            imap = server("imap.mailbox.org", 993, Security::Tls);
            smtp = server("smtp.mailbox.org", 465, Security::Tls);
            login_hint = Some(address.to_string());
        }
        "icloud.com" => {
            provider = "iCloud".into();
            urls.push("https://caldav.icloud.com/".into());
            imap = server("imap.mail.me.com", 993, Security::Tls);
            smtp = server("smtp.mail.me.com", 587, Security::StartTls);
            login_hint = Some(address.to_string());
            app_password = true;
        }
        "work.example.net" => {
            imap = server("imap.work.example.net", 993, Security::Tls);
            smtp = server("smtp.work.example.net", 587, Security::StartTls);
            login_hint = address.split('@').next().map(str::to_string);
        }
        "gmail.com" => {
            provider = "Gmail".into();
            imap = server("imap.gmail.com", 993, Security::Tls);
            smtp = server("smtp.gmail.com", 465, Security::Tls);
            login_hint = Some(address.to_string());
            app_password = true;
            unsupported = Some(tr(CALENDARS_UNSUPPORTED_GOOGLE).to_string());
            mail_unsupported = Some(tr(MailUnsupported::Google.reason()).to_string());
        }
        "outlook.com" => {
            provider = "Outlook.com".into();
            mail_unsupported = Some(tr(MailUnsupported::Microsoft.reason()).to_string());
        }
        _ => {}
    }
    Response::Discovery { provider, urls, unsupported, imap, smtp, login_hint, app_password, mail_unsupported }
}

/// The core's reason why Google's calendars do not work (`pim.rs`); Gmail answers with it next to
/// the one for mail.
const CALENDARS_UNSUPPORTED_GOOGLE: &str =
    "Google calendars need a sign-in through Google, which Torro Workplace does not support yet.";

/// What a demo client did lately, newest first, as the audit log has it: commands and results,
/// no contents. A client that was never used has nothing.
fn demo_activity(client: &ClientInfo, now: DateTime<Utc>) -> Vec<ActivityEntry> {
    let entry = |age: Duration, command: &str, result: &str| ActivityEntry {
        time: now - age,
        client: Some(client.id.clone()),
        command: command.into(),
        ids: Vec::new(),
        result: result.into(),
        approval: (result == "pending" || result == "denied").then(|| "9c1f0a7e3b2d4c58".into()),
    };
    if client.last_used.is_none() {
        return Vec::new();
    }
    match client.kind.as_str() {
        HOOK_CLIENT => vec![
            entry(Duration::minutes(1), "inbox.post_item", "ok"),
            entry(Duration::minutes(14), "inbox.resolve_items", "ok"),
            entry(Duration::minutes(15), "inbox.post_item", "ok"),
        ],
        _ => vec![
            entry(Duration::minutes(5), "calendar.list_events", "ok"),
            entry(Duration::minutes(7), "calendar.delete_event", "pending"),
            entry(Duration::minutes(40), "tasks.save_todo", "ok"),
            entry(Duration::hours(3), "workspaces.delete_template", "denied"),
            entry(Duration::days(1), "accounts.remove_account", "forbidden"),
            entry(Duration::days(1) + Duration::hours(2), "tasks.save_todo", "conflict"),
            entry(Duration::days(4), "calendar.list_calendars", "ok"),
        ],
    }
}

/// Summary and details of the demo's mail that waits to be sent (task M3.5), built by the core's
/// own functions from a draft as its store would hold it: two recipients, a Cc and a Bcc, two
/// attachments and a text.
fn demo_send() -> (String, Vec<String>) {
    use crate::approvals::{DraftFacts, Facts, FileFact, MailFacts};
    let who = |name: Option<&str>, address: &str| torro_mail::MailAddress {
        name: name.map(str::to_string),
        address: address.into(),
    };
    let draft = DraftFacts {
        subject: "Offsite agenda".into(),
        first_recipient: Some("bernd@example.com".into()),
        recipients: 4,
        attachments: 2,
        text_start: "Hi Bernd, hi Dora, here is the agenda for Thursday. Please add what is missing before \
                     Wednesday noon, then I will send it to everyone."
            .into(),
        bcc: vec!["carla@example.net".into()],
        to: vec![who(Some("Bernd Example"), "bernd@example.com"), who(Some("Dora Example"), "dora@example.com")],
        cc: vec![who(None, "team@example.org")],
        files: vec![
            FileFact { name: "agenda.pdf".into(), size: 182_000 },
            FileFact { name: "venue-map.png".into(), size: 2_400_000 },
        ],
    };
    let facts = Facts {
        mail: Some(MailFacts {
            account: Some("anna@mailbox.org".into()),
            count: 1,
            draft: Some(draft),
            ..MailFacts::default()
        }),
        ..Facts::default()
    };
    let send = Request::SendDraft { id: "m:demo".into() };
    (crate::approvals::summary(&send, &facts), crate::approvals::details(&send, &facts))
}

/// The demo's approvals: summaries as the core builds them (the same texts, so they are translated
/// too), from two agents; sending a mail with its details (task M3.5).
fn demo_approvals(now: chrono::DateTime<Utc>) -> Vec<ApprovalInfo> {
    let approval = |id: &str, client: (&str, &str), summary: String, age: Duration, state: ApprovalState| {
        let created = now - age;
        let decided = match state {
            ApprovalState::Pending => None,
            ApprovalState::Expired => Some(created + Duration::minutes(10)),
            _ => Some(created + Duration::minutes(1)),
        };
        ApprovalInfo {
            id: id.into(),
            client: client.0.into(),
            client_name: client.1.into(),
            command: "calendar.delete_event".into(),
            summary,
            details: Vec::new(),
            created,
            expires: created + Duration::minutes(10),
            decided,
            state,
        }
    };
    let (claude, codex) = (("claude-code", "Claude Code"), ("codex", "Codex"));
    let delete_event = |title: &str, when: &str, calendar: &str| {
        trf(
            "Delete event \"{title}\" on {when} from \"{calendar}\"",
            &[("title", &title), ("when", &when), ("calendar", &calendar)],
        )
    };
    let failed = Response::Error {
        code: "forbidden".into(),
        message: "The client may not write to this calendar anymore".into(),
    };
    let (send_summary, send_details) = demo_send();
    let send = ApprovalInfo {
        command: "mail.send_draft".into(),
        details: send_details,
        ..approval("3e8b5d1a9c7f2b64", claude, send_summary, Duration::minutes(1), ApprovalState::Pending)
    };
    vec![
        approval(
            "9c1f0a7e3b2d4c58",
            claude,
            delete_event("Team meeting", "Tue, 14 Oct 09:00", "Work"),
            Duration::minutes(2),
            ApprovalState::Pending,
        ),
        send,
        approval(
            "4b7d2e9a0c1f3a66",
            codex,
            trf("Overwrite template \"{name}\" with the active workspace", &[("name", &"Development")]),
            Duration::minutes(7),
            ApprovalState::Pending,
        ),
        approval(
            "1e5a8c3f7b2d9e40",
            claude,
            trf("Delete task \"{title}\" from \"{list}\"", &[("title", &"Renew passport"), ("list", &"Personal")]),
            Duration::minutes(25),
            ApprovalState::Done { response: Box::new(Response::Ok) },
        ),
        approval(
            "7f3c1a9e5d2b8c04",
            claude,
            delete_event("Dentist", "Thu, 16 Oct 15:30", "Family"),
            Duration::minutes(48),
            ApprovalState::Done { response: Box::new(failed) },
        ),
        approval(
            "2a9e4c7b1f5d3e88",
            codex,
            trf("Delete template \"{name}\"", &[("name", &"Writing")]),
            Duration::hours(2),
            ApprovalState::Denied,
        ),
        approval(
            "6d1b8f2a4e7c9a13",
            claude,
            delete_event("Weekly review", "Fri, 17 Oct 16:00", "Work"),
            Duration::hours(5),
            ApprovalState::Expired,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn every_state_of_open_is_parsed_in_both_spellings() {
        for (name, open) in Open::ALL {
            let expected = Launch { demo: true, open: Some(open), ..Launch::default() };
            assert_eq!(Launch::parse(&args(&["--demo", "--open", name])), Ok(expected.clone()), "{name}");
            assert_eq!(Launch::parse(&args(&["--demo", &format!("--open={name}")])), Ok(expected), "{name}");
        }
        assert_eq!(Launch::parse(&[]), Ok(Launch::default()));
    }

    /// Tasks S.21 and S.35: `--section bot:<id>` and `--section assistant` name the assistant's
    /// window, which comes to the front over the overview; a bot id that is no id is refused like
    /// any unknown section. The words of the pages that moved land where they went: `agents` on
    /// MCP, `apps` and `inbox` on General.
    #[test]
    fn the_sections_of_a_program_and_of_the_pages_that_moved() {
        let launch = Launch::parse(&args(&["--section", "bot:1a2b3c4d"])).expect("a bot");
        let bot = Some(ClientTarget::Bot("1a2b3c4d".into()));
        assert_eq!((launch.section, launch.client), (Section::Overview, bot.clone()));
        assert_eq!(Launch::parse(&args(&["--section=bot:1a2b3c4d"])).map(|l| l.client), Ok(bot));
        for bad in ["bot:", "bot:../x", "bot:a b", "bots"] {
            let err = Launch::parse(&args(&["--section", bad])).expect_err(bad);
            assert!(err.to_string().contains("bot:<id>"), "{bad}: {err}");
        }
        let assistant = Launch::parse(&args(&["--section", "assistant"])).expect("the assistant");
        assert_eq!((assistant.section, assistant.client), (Section::Overview, Some(ClientTarget::Assistant)));
        for target in [ClientTarget::Assistant, ClientTarget::Bot("1a2b3c4d".into())] {
            assert!(
                matches!(target.request(), Request::OpenApp { ref app, target: None } if app == AppId::ASSISTANT.binary),
                "{target:?}"
            );
        }
        for word in ["apps", "inbox"] {
            let launch = Launch::parse(&args(&["--section", word])).expect(word);
            assert_eq!((launch.section, launch.client), (Section::General, None), "{word}");
        }
        let agents = Launch::parse(&args(&["--section", "agents"])).expect("the old word");
        assert_eq!((agents.section, agents.client), (Section::Mcp, None));
    }

    /// Task WS.6: `--template=<id>` opens only the template's dialog, `--saved` in the mode `Save`
    /// and only together with a template; neither needs `--demo`.
    #[test]
    fn the_template_dialog_is_named_by_its_id_and_saved_needs_one() {
        let edit = Launch::parse(&args(&["--template=deep-work"])).expect("an id");
        assert_eq!((edit.template.as_deref(), edit.saved), (Some("deep-work"), false));
        let save = Launch::parse(&args(&["--template", "deep-work", "--saved"])).expect("both spellings");
        assert_eq!((save.template.as_deref(), save.saved), (Some("deep-work"), true));
        let demo = Launch::parse(&args(&["--demo", "--template=writing", "--appearance", "light"])).expect("with demo");
        assert_eq!((demo.demo, demo.template.as_deref()), (true, Some("writing")));
        assert_eq!(Launch::parse(&args(&["--saved"])), Err(LaunchError::NeedsTemplate("--saved")));
        assert_eq!(Launch::parse(&args(&["--template"])), Err(LaunchError::MissingValue("--template")));
        assert_eq!(Launch::parse(&args(&["--template="])), Err(LaunchError::MissingValue("--template")));
        assert_eq!(Launch::parse(&[]).map(|launch| (launch.template, launch.saved)), Ok((None, false)));
        // What `apps.open_app` starts for the target `template:<id>` is the mode `Edit`.
        let opened = crate::apps::Opened::Template { id: "deep-work".into() };
        let words = crate::apps::target_command(AppId::WORKPLACE.binary.into(), &opened);
        assert_eq!(words[1], "ui");
        let opened = Launch::parse(&words[2..]).expect("the core's command line");
        assert_eq!((opened.template.as_deref(), opened.saved), (Some("deep-work"), false));
    }

    #[test]
    fn section_and_appearance_are_parsed() {
        for section in Section::ALL {
            let launch = Launch::parse(&args(&["--section", section.arg()])).expect("a known section");
            assert_eq!(launch.section, section);
        }
        let launch = Launch::parse(&args(&["--demo", "--section=api", "--appearance", "light"])).expect("valid");
        assert_eq!((launch.section, launch.appearance), (Section::Api, Some(Appearance::Light)));
        for (word, appearance) in
            [("light", Appearance::Light), ("dark", Appearance::Dark), ("system", Appearance::System)]
        {
            let launch = Launch::parse(&args(&["--appearance", word])).expect("works without --demo");
            assert_eq!(launch.appearance, Some(appearance));
        }
    }

    #[test]
    fn the_setup_starts_at_its_first_step_or_the_one_named() {
        use super::super::setup::Step;
        assert_eq!(Launch::parse(&args(&[])).map(|l| l.setup), Ok(None));
        assert_eq!(Launch::parse(&args(&["--setup"])).map(|l| l.setup), Ok(Some(Step::Welcome)), "no demo needed");
        let launch = Launch::parse(&args(&["--setup", "--step=components"])).expect("a built step");
        assert_eq!(launch.setup, Some(Step::Components));
        assert_eq!(Launch::parse(&args(&["--step", "done"])), Err(LaunchError::NeedsSetup("--step")));
        let app = Launch::parse(&args(&["--section", "accounts", "--setup-if-open", "--step", "calendar"]))
            .expect("an app's");
        assert_eq!((app.section, app.setup, app.setup_if_open), (Section::Accounts, Some(Step::Calendar), true));
        let err = Launch::parse(&args(&["--setup", "--step", "later"])).expect_err("no such step");
        assert!(
            err.to_string().ends_with(
                "valid values: welcome, nextcloud, components, calendar, tasks, mail, inbox, ai, web, documents, done"
            ),
            "{err}"
        );
    }

    #[test]
    fn bad_parameters_name_the_valid_values() {
        let err = Launch::parse(&args(&["--demo", "--open", "popup"])).expect_err("unknown state");
        let text = err.to_string();
        assert!(text.contains("\"popup\"") && text.contains("--open"), "{text}");
        for (name, _) in Open::ALL {
            assert!(text.contains(name), "{name} is listed in: {text}");
        }
        // Task S.30: the catalog is gone, so is its word; the error names the dialog's new states.
        let err = Launch::parse(&args(&["--demo", "--open", "wizard"])).expect_err("no catalog any more");
        let text = err.to_string();
        assert!(text.contains("wizard-torromail") && text.contains("wizard-nextcloud"), "{text}");
        assert!(Open::ALL.iter().all(|(name, _)| *name != "wizard"));
        let err = Launch::parse(&args(&["--section", "mail"])).expect_err("unknown section");
        assert!(
            err.to_string().ends_with(
                "valid values: overview, accounts, ai, api, mcp, connections, approvals, workspaces, appearance, general, assistant, bot:<id>, agents, apps, inbox"
            ),
            "{err}"
        );
        let err = Launch::parse(&args(&["--appearance", "sepia"])).expect_err("unknown appearance");
        assert!(err.to_string().ends_with("valid values: light, dark, system"), "{err}");
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        assert_eq!(Launch::parse(&args(&["--open", "wizard-mail"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert_eq!(Launch::parse(&args(&["--demo", "--design"])), Err(LaunchError::MissingValue("--design")));
        let launch = Launch::parse(&args(&["--demo", "--design", "torro"])).expect("the standard is always there");
        assert_eq!(launch.design.as_deref(), Some("torro"));

        // Task L.22: a font and a colour scheme of their own, each from its folder, also only with the demo.
        let parts = Launch::parse(&args(&["--demo", "--font", "torro", "--color-scheme", "torro"])).expect("standards");
        assert_eq!((parts.font.as_deref(), parts.color_scheme.as_deref()), (Some("torro"), Some("torro")));
        assert_eq!((parts.forced().design, parts.forced().font.as_deref()), (None, Some("torro")));
        assert_eq!(Launch::parse(&args(&["--font", "torro"])), Err(LaunchError::NeedsDemo("--font")));
        assert_eq!(Launch::parse(&args(&["--color-scheme", "torro"])), Err(LaunchError::NeedsDemo("--color-scheme")));
        let unknown = Launch::parse(&args(&["--demo", "--color-scheme", "no-such-scheme"])).expect_err("unknown");
        assert!(unknown.to_string().contains("--color-scheme"), "{unknown}");
    }

    #[test]
    fn the_demo_has_something_to_show_on_every_page() {
        let data = demo_data();
        let status = data.status.as_ref().expect("a status");
        assert_eq!(status.templates, data.templates.len());
        assert!(data.error.is_none());

        let listed: Vec<&AccountInfo> = super::super::accounts::listed(&data.accounts).collect();
        assert_eq!(listed.len(), data.accounts.len(), "no sign-in in progress");
        assert!(
            matches!(listed[0].state, AccountState::Connected { problem: None, .. }),
            "the detail page opens the first account, and it is a healthy one"
        );
        assert!(
            listed.iter().any(|account| matches!(account.state, AccountState::Connected { problem: Some(_), .. })),
            "one account shows a problem"
        );
        assert!(listed[0].collections.iter().any(|collection| !collection.writable), "a read-only calendar");
        assert!(listed[0].collections.iter().any(|collection| collection.kind.has_todos()), "a task list");

        let ids: Vec<&str> =
            listed.iter().flat_map(|account| &account.collections).map(|collection| collection.id.as_str()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<HashSet<_>>().len(), "collection ids are unique");
        assert!(data.settings.hidden_collections.iter().all(|id| ids.contains(&id.as_str())), "a hidden list exists");
        assert!(!data.settings.hidden_collections.is_empty(), "one list is hidden");

        assert!(data.templates.len() >= 2);
        let apps = data.apps.as_ref().expect("apps");
        assert!(apps.iter().all(|app| app.binary != AppId::WORKPLACE.binary), "the core is not one of its apps");
        assert!(apps.len() >= 2);

        let open = data.approvals.iter().filter(|approval| approval.state.is_pending()).count();
        assert!(open >= 2, "the approvals page has open ones to decide");
        assert!(data.approvals.len() > open, "and decided ones below");
        assert!(data.approvals.iter().all(|approval| approval.expires > approval.created));
    }

    #[test]
    fn the_demo_searches_like_the_core_table() {
        let found = |address: &str| match demo_discovery(address) {
            Response::Discovery {
                provider,
                urls,
                unsupported,
                imap,
                smtp,
                login_hint,
                app_password,
                mail_unsupported,
            } => (provider, urls, unsupported, imap, smtp, login_hint, app_password, mail_unsupported),
            other => panic!("expected Discovery, got {other:?}"),
        };
        let mailbox = found("anna@mailbox.org");
        assert_eq!((mailbox.0.as_str(), mailbox.1), ("mailbox.org", vec!["https://dav.mailbox.org/".to_string()]));
        assert_eq!(
            (mailbox.3.map(|s| s.host), mailbox.4.map(|s| s.host)),
            (Some("imap.mailbox.org".into()), Some("smtp.mailbox.org".into()))
        );
        assert_eq!((mailbox.5.as_deref(), mailbox.6, mailbox.7), (Some("anna@mailbox.org"), false, None));

        let icloud = found("ANNA@iCloud.com");
        assert_eq!((icloud.0.as_str(), icloud.6), ("iCloud", true), "an app password");

        let work = found("anna@work.example.net");
        assert!(work.1.is_empty() && work.3.is_some(), "mail only");
        assert_eq!(work.5.as_deref(), Some("anna"), "this provider asks for the part before @");

        let gmail = found("anna@gmail.com");
        assert_eq!((gmail.0.as_str(), gmail.1.is_empty(), gmail.3.is_some(), gmail.6), ("Gmail", true, true, true));
        assert_eq!(gmail.7.as_deref(), Some(tr(MailUnsupported::Google.reason())));
        assert!(gmail.2.is_some(), "calendars do not work either");

        let outlook = found("anna@outlook.com");
        assert!(outlook.3.is_none() && outlook.4.is_none());
        assert_eq!(outlook.7.as_deref(), Some(tr(MailUnsupported::Microsoft.reason())));

        let nothing = found("anna@example.org");
        assert_eq!((nothing.0.as_str(), nothing.1.is_empty(), nothing.3, nothing.7), ("example.org", true, None, None));
    }

    #[test]
    fn the_demo_signs_in_with_any_password_but_the_refused_ones() {
        use torro_core::account::Secret;
        use torro_mail::{Endpoint, Security};
        let data = demo_data();
        let connect = |password: &str| Request::ConnectCalDav {
            address: "anna@mailbox.org".into(),
            server: None,
            login_name: "anna@mailbox.org".into(),
            password: Secret::new(password),
            calendars: true,
            tasks: false,
        };
        assert!(matches!(
            demo_answer(&connect("wrong"), &data),
            Some(Response::Error { ref code, ref message }) if code == "auth" && message == tr("Password was refused")
        ));
        assert!(matches!(
            demo_answer(&connect("anything"), &data),
            Some(Response::Connected { calendars: true, tasks: true, .. })
        ));

        let server = |host: &str| Endpoint { host: host.into(), port: 993, security: Security::Tls };
        let mail = |password: &str, caldav: Option<&str>| Request::ConnectMail {
            address: "anna@mailbox.org".into(),
            sender_name: String::new(),
            imap: server("imap.mailbox.org"),
            smtp: server("smtp.mailbox.org"),
            login_name: "anna@mailbox.org".into(),
            password: Secret::new(password),
            calendars: true,
            tasks: true,
            caldav: caldav.map(str::to_string),
            trusted_certificates: Vec::new(),
        };
        for (password, server) in [("wrong", "IMAP"), ("wrong-smtp", "SMTP")] {
            assert!(matches!(
                demo_answer(&mail(password, None), &data),
                Some(Response::Error { ref code, ref message })
                    if code == "auth" && *message == trf("The {service} server refused the password.", &[("service", &server)])
            ));
        }
        match demo_answer(&mail("anything", Some("https://dav.mailbox.org/")), &data) {
            Some(Response::MailConnected { account, calendars, tasks, open_roles }) => {
                assert_eq!((account.kind.as_str(), account.label.as_str()), ("mail", "anna@mailbox.org"));
                assert!(calendars && tasks && open_roles.is_empty());
            }
            other => panic!("expected MailConnected, got {other:?}"),
        }
        match demo_answer(&mail("anything", None), &data) {
            Some(Response::MailConnected { calendars, tasks, .. }) => {
                assert!(!calendars && !tasks, "no CalDAV, none asked")
            }
            other => panic!("expected MailConnected, got {other:?}"),
        }
    }

    /// Task M11.4: the demo's own servers show a certificate the system does not trust until one
    /// is trusted; a server at home without the warning, one on the public internet with it.
    #[test]
    fn the_demo_asks_about_the_certificate_of_its_own_servers() {
        use torro_core::account::Secret;
        let data = demo_data();
        assert_eq!(torro_mail::account::certificate_fingerprint(DEMO_FINGERPRINT).as_deref(), Some(DEMO_FINGERPRINT));
        let mail = |host: &str, trusted: Vec<String>| Request::ConnectMail {
            address: "anna@home.lan".into(),
            sender_name: String::new(),
            imap: Endpoint { host: host.into(), port: 993, security: Security::Tls },
            smtp: Endpoint { host: host.into(), port: 587, security: Security::StartTls },
            login_name: "anna".into(),
            password: Secret::new("demo-password"),
            calendars: false,
            tasks: false,
            caldav: None,
            trusted_certificates: trusted,
        };
        for (host, public) in [("mail.home.lan", false), ("mail.berg.example", true)] {
            match demo_answer(&mail(host, Vec::new()), &data) {
                Some(Response::UntrustedCertificate { service, host: asked, sha256, public: on_internet }) => {
                    assert_eq!((service.as_str(), asked.as_str(), sha256.as_str()), ("IMAP", host, DEMO_FINGERPRINT));
                    assert_eq!(on_internet, public, "{host}");
                }
                other => panic!("expected UntrustedCertificate, got {other:?}"),
            }
            let trusted = mail(host, vec![DEMO_FINGERPRINT.into()]);
            assert!(matches!(demo_answer(&trusted, &data), Some(Response::MailConnected { .. })), "{host}");
        }
        assert!(matches!(
            demo_answer(&mail("imap.mailbox.org", Vec::new()), &data),
            Some(Response::MailConnected { .. })
        ));
        for name in ["wizard-mail-certificate", "wizard-mail-certificate-public"] {
            assert!(Open::ALL.iter().any(|(state, _)| *state == name), "{name}");
        }
    }

    #[test]
    fn the_demo_has_two_mail_accounts_and_an_agent_that_sends_freely() {
        let data = demo_data();
        let mail: Vec<&str> =
            data.accounts.iter().filter(|account| account.kind == "mail").map(|account| account.id.as_str()).collect();
        assert_eq!(mail, ["mail:anna@mailbox.org", "mail:anna@work.example.net"]);
        assert!(mail.iter().all(|id| torro_mail::is_account_id(id)), "ids as the core makes them");
        let codex = data.clients.iter().find(|client| client.kind == "codex").expect("Codex");
        assert!(torro_settings_ui::rights::sends_freely(&codex.rights));
        assert!(codex.rights.reaches_mail_account(mail[0]) && !codex.rights.reaches_mail_account(mail[1]));
        let default = data.clients.iter().find(|client| client.kind == "claude-code").expect("Claude Code");
        assert_eq!(default.rights.outbound, Outbound::WithApproval, "the templates ask");
    }

    /// Found open after S.34: the demo answers `clients.new_key` for the assistant and a bot as the
    /// core does, with `unsupported`, and hands out a made-up key only to the others.
    #[test]
    fn the_demo_gives_the_assistant_and_bots_no_key() {
        let data = demo_data();
        for kind in [ASSISTANT_CLIENT, BOT_CLIENT] {
            let client = data.clients.iter().find(|client| client.kind == kind).expect("a client of the kind");
            let answer = demo_answer(&Request::NewClientKey { id: client.id.clone() }, &data);
            assert!(matches!(answer, Some(Response::Error { ref code, .. }) if code == "unsupported"), "{kind}");
        }
        let script = data.clients.iter().find(|client| client.kind == "script").expect("the script");
        let answer = demo_answer(&Request::NewClientKey { id: script.id.clone() }, &data);
        assert!(matches!(answer, Some(Response::ClientKey { .. })));
        let notes = demo_hook_notes();
        assert!(notes.iter().all(|note| data.clients.iter().any(|client| client.id == note.client)));
    }

    #[test]
    fn the_demo_mail_accounts_show_every_part_of_their_page() {
        let data = demo_data();
        let mail: Vec<&MailView> = data.accounts.iter().filter_map(|account| account.mail.as_ref()).collect();
        let [first, second] = mail[..] else { panic!("two mail accounts") };
        assert!(first.settings.services.calendars && first.settings.services.tasks && first.caldav.is_some());
        assert!(first.calendars.is_some() && first.settings.signature.is_some());
        let [open] = &first.open_roles[..] else { panic!("one open role") };
        assert_eq!((open.role, open.candidates.len()), (Role::Sent, 2));
        assert!(second.caldav.is_none() && second.mail.problem.is_some(), "mail only, with a problem");
        for (name, state) in [("mail-detail", Open::MailDetail), ("mail-servers", Open::MailServers)] {
            assert!(Open::ALL.contains(&(name, state)), "{name}");
        }
    }

    /// Task M3.5: the demo shows what the core would list: no CalDAV account beside the mail
    /// account of the same sign-in, and a mail waiting to be sent with its details.
    #[test]
    fn the_demo_lists_no_sign_in_twice_and_a_send_with_details() {
        let data = demo_data();
        let caldav_server = |account: &AccountInfo| match &account.state {
            AccountState::Connected { server, login_name, .. } => Some((server.clone(), login_name.clone())),
            _ => None,
        };
        for mail in data.accounts.iter().filter_map(|account| account.mail.as_ref()) {
            let Some(url) = &mail.caldav else { continue };
            let same = data
                .accounts
                .iter()
                .filter(|account| account.kind == "caldav")
                .filter_map(caldav_server)
                .any(|(server, login)| server == *url && login == mail.address);
            assert!(!same, "{} is listed twice", mail.address);
        }
        let send = data.approvals.iter().find(|approval| approval.command == "mail.send_draft").expect("a send");
        assert!(send.state.is_pending() && send.details.len() > 4, "{:?}", send.details);
    }

    #[test]
    fn the_demo_has_a_mail_provider_account() {
        let data = demo_data();
        let caldav = data.accounts.iter().find(|account| account.kind == "caldav").expect("a CalDAV account");
        assert!(matches!(caldav.state, AccountState::Connected { problem: None, .. }));
        assert!(caldav.collections.iter().any(|collection| collection.kind.has_events()));
        assert!(caldav.collections.iter().any(|collection| collection.kind.has_todos()));
    }
}
