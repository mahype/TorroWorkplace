//! The start parameters of the control centre and the sample data of `--demo`.
//!
//! `torroworkplace ui --demo --open wizard --appearance light` shows a state without a service
//! and without clicking, so `torro-design-review` can photograph every state from a script. In
//! demo mode the window never talks to a socket and never stores anything: the pages get
//! [`demo_data`], and every request a page wants to send is dropped (see `Ui::perform`).

use std::fmt;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Local, Utc};
use torro_core::AppId;
use torro_core::ai::{AiError, AiProvider};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_inbox::{AgentKind, Lane, Rule, RuleEffect, SourceInfo, SourceRef};
use torro_ipc::api::{
    ASSISTANT_CLIENT, Area, AreaRights, Confirm, HOOK_CLIENT, Level, Outbound, Rights, RightsTemplate, Selection,
};
use torro_ipc::{
    AccountInfo, ActivityEntry, AiServer, AppInfo, ApprovalInfo, ApprovalState, ClientInfo, DesignInfo, FolderChoice,
    IssuedKey, Key, MailView, OpenRoleView, Request, Response, ServiceState, Status, TorroMailAccount,
};
use torro_mail::{Endpoint, MailSettings, Notify, Role, Security, Services};
use torro_mail_imap::discover::MailUnsupported;
use torro_pim::model::GitHubFilter;
use torro_pim::{AccountState, Collection, CollectionKind};
use torro_setups::TemplateSummary;

use super::general::Pack;
use super::wizard;
use super::{Appearance, Data, Section};

/// A state the control centre can start in (`--demo --open <state>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The add-account dialog.
    Wizard,
    /// The add-account dialog at GitHub's two ways to sign in.
    WizardGithub,
    /// The add-account dialog at the field for a GitHub token.
    WizardToken,
    /// The add-account dialog on the mail account path (tasks M7.2, M5.1), at the state the script
    /// of [`wizard::demo_messages`] leads to; the demo's search answers for a few domains
    /// ([`demo_answer`]).
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
    /// The detail page of the demo's AI account (OpenRouter, task KI.1a).
    AiDetail,
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
    /// The first template's row in rename mode.
    RenameTemplate,
    /// The approvals page without any approvals: its empty state.
    ApprovalsEmpty,
    /// The detail page of the demo agent with its own rights (levels, admin switch, selections).
    AgentDetail,
    /// The detail page of the demo script.
    ScriptDetail,
    /// The detail page of the demo hook client: fixed rights, read-only.
    HookDetail,
    /// The dialog with a new key of the demo agent, shown once.
    NewKey,
    /// The "Disconnect this agent?" confirmation.
    Disconnect,
    /// The agents page without any clients: its empty state.
    AgentsEmpty,
    /// "Connect agent" at its first step: the clients the demo found.
    AgentWizard,
    /// "Connect agent" at the rights of Claude Code.
    AgentWizardRights,
    /// "Connect agent" done: Claude Code connected (nothing is written).
    AgentWizardDone,
    /// "Connect agent" with the key of a new script, shown once.
    AgentWizardKey,
    /// The inbox page with the demo's rules (task E3.7).
    Inbox,
    /// The inbox page without any rules: the group says where they come from.
    InboxEmpty,
    /// General › Design with the demo's sample packs ([`demo_designs`]), one of them broken.
    Design,
    /// The Assistant page with the demo's assistant (task C.6): rights of its own, its engines and
    /// how long it keeps conversations.
    Assistant,
    /// The Assistant page before the assistant has a client: the first start with what it is
    /// offered (`Rights::assistant`).
    AssistantFirstStart,
    /// The Assistant page with the assistant reading only calendars, tasks and the inbox, as its
    /// first start offers.
    AssistantReadOnly,
    /// The "Turn off the assistant?" confirmation.
    AssistantTurnOff,
}

impl Open {
    pub const ALL: [(&'static str, Open); 55] = [
        ("wizard", Open::Wizard),
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
        ("wizard-torromail-failed", Open::WizardMail(wizard::Demo::TorroMailFailed)),
        ("wizard-torromail-password", Open::WizardMail(wizard::Demo::TorroMailPassword)),
        ("wizard-torromail-done", Open::WizardMail(wizard::Demo::TorroMailDone)),
        ("wizard-ai", Open::WizardAi(wizard::Demo::AiProviders)),
        ("wizard-ai-openrouter", Open::WizardAi(wizard::Demo::AiOpenRouter)),
        ("wizard-ai-refused", Open::WizardAi(wizard::Demo::AiRefused)),
        ("wizard-ai-server", Open::WizardAi(wizard::Demo::AiServer)),
        ("wizard-ai-done", Open::WizardAi(wizard::Demo::AiDone)),
        ("change-password", Open::ChangePassword),
        ("account-detail", Open::AccountDetail),
        ("scopes-detail", Open::GithubDetail),
        ("ai-detail", Open::AiDetail),
        ("mail-detail", Open::MailDetail),
        ("mail-detail-problem", Open::MailDetailProblem),
        ("mail-password", Open::MailPassword),
        ("mail-servers", Open::MailServers),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("remove", Open::Remove),
        ("delete-template", Open::DeleteTemplate),
        ("rename-template", Open::RenameTemplate),
        ("approvals-empty", Open::ApprovalsEmpty),
        ("agent-detail", Open::AgentDetail),
        ("script-detail", Open::ScriptDetail),
        ("hook-detail", Open::HookDetail),
        ("new-key", Open::NewKey),
        ("disconnect", Open::Disconnect),
        ("agents-empty", Open::AgentsEmpty),
        ("agent-wizard", Open::AgentWizard),
        ("agent-wizard-rights", Open::AgentWizardRights),
        ("agent-wizard-done", Open::AgentWizardDone),
        ("agent-wizard-key", Open::AgentWizardKey),
        ("inbox", Open::Inbox),
        ("inbox-empty", Open::InboxEmpty),
        ("design", Open::Design),
        ("assistant", Open::Assistant),
        ("assistant-first-start", Open::AssistantFirstStart),
        ("assistant-read-only", Open::AssistantReadOnly),
        ("assistant-turn-off", Open::AssistantTurnOff),
    ];
}

/// The start parameters of `torroworkplace ui`:
///
/// - `--section overview|accounts|workspaces|apps|assistant|agents|approvals|inbox|general`: the page
///   to start on. The states of `--open` that belong to a page choose that page themselves. Other
///   programs open a page this way (`torroworkplace ui --section assistant` from the assistant's
///   chat when it may not use something, task C.6).
/// - `--demo`: sample accounts, templates, settings, apps, agents, approvals and inbox rules in memory instead
///   of the service. Nothing is sent and nothing is stored; [`demo_answer`] answers the few
///   requests whose answer is a state of its own.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--design <id>`: draw with this design pack instead of the stored one (task L.3); needs
///   `--demo`. An id that is not a valid pack is refused with the valid ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub section: Section,
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub design: Option<String>,
}

impl Default for Launch {
    fn default() -> Self {
        Launch { section: Section::Overview, demo: false, open: None, appearance: None, design: None }
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
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
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
    /// Reads the start parameters from `args` (the words after `ui`).
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let section = match flag_value(args, "--section")? {
            None => Section::Overview,
            Some(word) => Section::from_arg(word)
                .ok_or_else(|| unknown("--section", word, Section::ALL.iter().map(|s| s.arg()).collect()))?,
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
        let design = match flag_value(args, "--design")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--design")),
            Some(id) if torro_gui::design::is_choice(id) => Some(id.to_owned()),
            Some(id) => {
                let valid = torro_gui::design::choices();
                return Err(unknown("--design", id, valid.iter().map(String::as_str).collect()));
            }
        };
        Ok(Launch { section, demo, open, appearance, design })
    }
}

/// The design packs of `--demo` (General › Design, task L.4), in memory like all its data: the
/// standard; "Harbor" (teal accent, cool surfaces) and "Paper" (warm light surfaces, blue accent),
/// which can be chosen and are drawn at once; and "Neon", which breaks two rules and says so. The
/// samples use Inter, the only font the demo has without a folder.
pub fn demo_designs() -> Vec<Pack> {
    use torro_core::design::{Design, DesignError, Mode, Rgba};
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
    };
    let pack = |id: &str, name: &str, version: &str, description: &str| {
        let mut design = Design::standard();
        design.id = id.into();
        design.about.name = name.into();
        design.about.version = version.into();
        design.about.author = Some("Torro samples".into());
        design.about.description = Some(description.into());
        design
    };
    let mut harbor = pack("harbor", "Harbor", "1.2.0", "Teal accent, cool surfaces");
    let dark = &mut harbor.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x0A0E10), Rgba::hex(0x101518), Rgba::hex(0x151B1F));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x14B8A6), Rgba::hex(0x042F2C));
    let light = &mut harbor.colors.light;
    (light.bg, light.panel) = (Rgba::hex(0xEEF3F4), Rgba::hex(0xFFFFFF));
    (light.accent, light.on_accent) = (Rgba::hex(0x0B7A72), Rgba::hex(0xFFFFFF));
    let mut paper = pack("paper", "Paper", "0.9.1", "Warm paper, ink blue");
    let dark = &mut paper.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x12100D), Rgba::hex(0x1A1714), Rgba::hex(0x201C18));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x7C9CF5), Rgba::hex(0x0B1530));
    let light = &mut paper.colors.light;
    (light.bg, light.panel, light.raised) = (Rgba::hex(0xF3EEE4), Rgba::hex(0xFFFCF6), Rgba::hex(0xF8F4EC));
    (light.accent, light.on_accent) = (Rgba::hex(0x2A4FC4), Rgba::hex(0xFFFFFF));
    let neon = DesignInfo {
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
    };
    let mut packs: Vec<Pack> =
        [Design::standard(), harbor, paper].into_iter().map(|design| Pack::new(info(&design), Some(design))).collect();
    packs.push(Pack::new(neon, None));
    packs
}

/// The sample content of `--demo`: a healthy Nextcloud account with a shared read-only calendar,
/// a second one that cannot reach its server (so the problem rows show), a GitHub account, three
/// templates, the suite's apps, settings with one list hidden, approvals: two open, one done,
/// one failed, one denied, one expired ([`demo_approvals`]), and five clients with the calendars,
/// lists (one a GitHub filter list) and inbox sources their selections choose from
/// ([`demo_clients`]), and inbox rules: two remembered lanes and the muted source of the sources
/// list ([`demo_rules`]). Times are relative to now.
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
        },
        // Two mail accounts (tasks M3.4, M5.2), so the agents' "Mail accounts" have something to
        // choose from and the account page shows both kinds: one with calendars and tasks, a
        // signature and a folder role the server left open, one that is mail only and cannot
        // reach its server. Their ids are `mail:<address>`, as the core makes them.
        mailbox_account(now),
        work_mail_account(now),
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
        },
    ];
    let template = |id: &str, name: &str, windows, age: Duration| TemplateSummary {
        id: id.into(),
        name: name.into(),
        windows,
        updated: now - age,
    };
    let templates = vec![
        template("development", "Development", 6, Duration::days(2)),
        template("writing", "Writing", 3, Duration::days(9)),
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
        SourceInfo { source: SourceRef::suite(), open: 2, muted: false },
        SourceInfo { source: SourceRef::agent("claude-code"), open: 3, muted: false },
        SourceInfo { source: SourceRef::script("backup"), open: 1, muted: false },
        SourceInfo { source: SourceRef::script("nightly-build"), open: 0, muted: true },
    ];
    Data {
        clients: demo_clients(now, first_calendar),
        calendars,
        task_lists,
        sources,
        rules: demo_rules(now),
        status: Some(Status {
            version: env!("CARGO_PKG_VERSION").into(),
            backend: "hyprland".into(),
            workspace: Some("3".into()),
            templates: templates.len(),
            busy: false,
        }),
        accounts,
        templates,
        settings: Settings { start_at_login: true, hidden_collections, ..Settings::default() },
        apps: Some(apps),
        approvals: demo_approvals(now),
        error: None,
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

/// The second demo mail account: mail only, and the server has not answered for two days.
fn work_mail_account(now: DateTime<Utc>) -> AccountInfo {
    let mut account = mail_account("anna@work.example.net", "imap.work.example.net", now - Duration::days(2));
    let problem = "The server did not answer".to_string();
    if let AccountState::Connected { problem: state_problem, .. } = &mut account.state {
        *state_problem = Some(problem.clone());
    }
    if let Some(mail) = account.mail.as_mut() {
        mail.mail.problem = Some(problem);
    }
    account
}

/// The demo's inbox rules: Codex's notices remembered in "For me", the backup script's in "Info",
/// the nightly build muted, as the demo's sources list has it, and a sender and a domain hidden.
fn demo_rules(now: DateTime<Utc>) -> Vec<Rule> {
    let rule = |id: &str, source: SourceRef, effect: RuleEffect, age: Duration| Rule {
        id: id.into(),
        source,
        sender: None,
        domain: None,
        effect,
        created: now - age,
    };
    vec![
        rule("5e0b7c2a", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }, Duration::days(6)),
        rule("a81f3d94", SourceRef::script("nightly-build"), RuleEffect::Muted, Duration::days(3)),
        rule("2c6e9b17", SourceRef::script("backup"), RuleEffect::Lane { lane: Lane::Info }, Duration::days(1)),
        // A sender and a domain hidden in the Inbox (task E3.10).
        Rule {
            sender: Some("news@shop.example".into()),
            ..rule("7d41e0b3", SourceRef::mail("anna@example.org"), RuleEffect::Muted, Duration::hours(20))
        },
        Rule {
            domain: Some("deals.example".into()),
            ..rule("e93a5c08", SourceRef::mail("anna@example.org"), RuleEffect::Muted, Duration::hours(2))
        },
    ]
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
        outbound: Outbound::Never,
        confirm: Confirm::Never,
    };
    let mut assistant = Rights::assistant();
    set(&mut assistant, Area::Calendar, Level::Write, false);
    set(&mut assistant, Area::Tasks, Level::Write, false);
    set(&mut assistant, Area::Mail, Level::Read, false);
    set(&mut assistant, Area::Workspaces, Level::Read, false);
    assistant.mail_accounts = Selection::Only(vec!["mail:anna@mailbox.org".into()]);
    vec![
        client(
            "3fa9c1d2",
            "Claude Code",
            "claude-code",
            RightsTemplate::ReadAndWrite.rights(),
            Some(now - Duration::minutes(5)),
        ),
        client("7b2e4f10", "Codex", "codex", codex, Some(now - Duration::days(2))),
        client("c41d9e06", "Claude Code hooks", HOOK_CLIENT, Rights::hook(), Some(now - Duration::minutes(1))),
        client("e8a07b35", "backup", "script", backup, None),
        client("a51c3e88", "Assistant", ASSISTANT_CLIENT, assistant, Some(now - Duration::hours(3))),
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

/// The sample answer `--demo` gives to a request whose answer is a state of its own: an agent's
/// activity, a new key and a new client of "Connect agent" (made-up keys, never working ones; the
/// new client is not added to the list). Every other request gets none and is dropped, so the
/// demo changes nothing.
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
                },
            })
        }
        // "wrong" is the password the demo's provider refuses (for mail its IMAP server), "wrong-smtp"
        // the one only its SMTP server refuses; nothing is stored either way.
        Request::ConnectCalDav { password, .. } if password.expose() == "wrong" => {
            Some(Response::Error { code: "auth".into(), message: tr("Password was refused").into() })
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
        Request::ClientActivity { id } => {
            let client = client_of(id)?;
            Some(Response::Activity { entries: demo_activity(client, Utc::now()) })
        }
        Request::NewClientKey { id } => {
            let client = client_of(id)?.clone();
            let key = IssuedKey::new(Key::new(format!("torro_{id}_{}", "5f2c9a1e7b3d4086".repeat(4))));
            Some(Response::ClientKey { client, key })
        }
        Request::CreateClient { name, kind, template, agent } => {
            // The hook client "Connect agent" creates after the agent (task E2.3) gets an id of
            // its own, as it would from the core.
            let id = if kind == HOOK_CLIENT { "4c7a19e3" } else { "9d4b7e21" };
            let client = ClientInfo {
                id: id.into(),
                name: name.clone(),
                kind: kind.clone(),
                created: Utc::now(),
                last_used: None,
                rights: template.rights().for_kind(kind),
                agent: *agent,
            };
            let key = IssuedKey::new(Key::new(format!("torro_{id}_{}", "a3e81c0f6d2b9754".repeat(4))));
            Some(Response::ClientKey { client, key })
        }
        _ => None,
    }
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

    #[test]
    fn section_and_appearance_are_parsed() {
        for section in Section::ALL {
            let launch = Launch::parse(&args(&["--section", section.arg()])).expect("a known section");
            assert_eq!(launch.section, section);
        }
        let launch = Launch::parse(&args(&["--demo", "--section=apps", "--appearance", "light"])).expect("valid");
        assert_eq!((launch.section, launch.appearance), (Section::Apps, Some(Appearance::Light)));
        for (word, appearance) in
            [("light", Appearance::Light), ("dark", Appearance::Dark), ("system", Appearance::System)]
        {
            let launch = Launch::parse(&args(&["--appearance", word])).expect("works without --demo");
            assert_eq!(launch.appearance, Some(appearance));
        }
    }

    #[test]
    fn bad_parameters_name_the_valid_values() {
        let err = Launch::parse(&args(&["--demo", "--open", "popup"])).expect_err("unknown state");
        let text = err.to_string();
        assert!(text.contains("\"popup\"") && text.contains("--open"), "{text}");
        for (name, _) in Open::ALL {
            assert!(text.contains(name), "{name} is listed in: {text}");
        }
        let err = Launch::parse(&args(&["--section", "mail"])).expect_err("unknown section");
        assert!(
            err.to_string().ends_with(
                "valid values: overview, accounts, workspaces, apps, assistant, agents, approvals, inbox, general"
            ),
            "{err}"
        );
        let err = Launch::parse(&args(&["--appearance", "sepia"])).expect_err("unknown appearance");
        assert!(err.to_string().ends_with("valid values: light, dark, system"), "{err}");
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        assert_eq!(Launch::parse(&args(&["--open", "wizard"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert_eq!(Launch::parse(&args(&["--demo", "--design"])), Err(LaunchError::MissingValue("--design")));
        let launch = Launch::parse(&args(&["--demo", "--design", "torro"])).expect("the standard is always there");
        assert_eq!(launch.design.as_deref(), Some("torro"));
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

    #[test]
    fn the_demo_has_two_mail_accounts_and_an_agent_that_sends_freely() {
        let data = demo_data();
        let mail: Vec<&str> =
            data.accounts.iter().filter(|account| account.kind == "mail").map(|account| account.id.as_str()).collect();
        assert_eq!(mail, ["mail:anna@mailbox.org", "mail:anna@work.example.net"]);
        assert!(mail.iter().all(|id| torro_mail::is_account_id(id)), "ids as the core makes them");
        let codex = data.clients.iter().find(|client| client.kind == "codex").expect("Codex");
        assert!(super::super::agents::sends_freely(&codex.rights));
        assert!(codex.rights.reaches_mail_account(mail[0]) && !codex.rights.reaches_mail_account(mail[1]));
        let default = data.clients.iter().find(|client| client.kind == "claude-code").expect("Claude Code");
        assert_eq!(default.rights.outbound, Outbound::WithApproval, "the templates ask");
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
