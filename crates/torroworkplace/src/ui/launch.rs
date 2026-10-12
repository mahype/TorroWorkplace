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
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_inbox::{AgentKind, Lane, Rule, RuleEffect, SourceInfo, SourceRef};
use torro_ipc::api::{Area, AreaRights, Confirm, HOOK_CLIENT, Level, Outbound, Rights, RightsTemplate, Selection};
use torro_ipc::{
    AccountInfo, ActivityEntry, AppInfo, ApprovalInfo, ApprovalState, ClientInfo, IssuedKey, Key, Request, Response,
    Status,
};
use torro_mail::{Endpoint, Security};
use torro_mail_imap::discover::MailUnsupported;
use torro_pim::model::GitHubFilter;
use torro_pim::{AccountState, Collection, CollectionKind};
use torro_setups::TemplateSummary;

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
    /// "Change password" of the demo's CalDAV account.
    ChangePassword,
    /// The detail page of the first demo account.
    AccountDetail,
    /// The detail page of the demo's GitHub account.
    GithubDetail,
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
}

impl Open {
    pub const ALL: [(&'static str, Open); 37] = [
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
        ("change-password", Open::ChangePassword),
        ("account-detail", Open::AccountDetail),
        ("scopes-detail", Open::GithubDetail),
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
    ];
}

/// The start parameters of `torroworkplace ui`:
///
/// - `--section overview|accounts|workspaces|apps|agents|approvals|inbox|general`: the page to start on.
///   The states of `--open` that belong to a page choose that page themselves.
/// - `--demo`: sample accounts, templates, settings, apps, agents, approvals and inbox rules in memory instead
///   of the service. Nothing is sent and nothing is stored; [`demo_answer`] answers the few
///   requests whose answer is a state of its own.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub section: Section,
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
}

impl Default for Launch {
    fn default() -> Self {
        Launch { section: Section::Overview, demo: false, open: None, appearance: None }
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
        Ok(Launch { section, demo, open, appearance })
    }
}

/// The sample content of `--demo`: a healthy Nextcloud account with a shared read-only calendar,
/// a second one that cannot reach its server (so the problem rows show), a GitHub account, three
/// templates, the suite's apps, settings with one list hidden, approvals: two open, one done,
/// one failed, one denied, one expired ([`demo_approvals`]), and four clients with the calendars,
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
        // A mail provider's calendars and tasks (task M7.2).
        AccountInfo {
            id: "caldav:anna@mailbox.org@dav.mailbox.org".into(),
            kind: "caldav".into(),
            label: "anna@mailbox.org".into(),
            state: AccountState::Connected {
                server: "https://dav.mailbox.org/".into(),
                login_name: "anna@mailbox.org".into(),
                last_sync: Some(now - Duration::minutes(9)),
                problem: None,
            },
            collections: vec![
                team("demo:/mailbox/calendars/private/", "Private", CollectionKind::Events, "#3FB984"),
                team("demo:/mailbox/tasks/inbox/", "Inbox", CollectionKind::Todos, "#A78BFA"),
            ],
            scopes: None,
            mail: None,
        },
        // Two mail accounts (task M3.4), so the agents' "Mail accounts" have something to choose
        // from. Their ids are `mail:<address>`, as the core makes them.
        mail_account("anna@mailbox.org", "imap.mailbox.org", now - Duration::minutes(2)),
        mail_account("anna@work.example.net", "imap.work.example.net", now - Duration::minutes(6)),
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

/// A connected mail account of the demo: no calendars or lists, as long as it is mail only.
fn mail_account(address: &str, server: &str, last_sync: DateTime<Utc>) -> AccountInfo {
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
        mail: None,
    }
}

/// The demo's inbox rules: Codex's notices remembered in "For me", the backup script's in "Info",
/// and the nightly build muted, as the demo's sources list has it.
fn demo_rules(now: DateTime<Utc>) -> Vec<Rule> {
    let rule = |id: &str, source: SourceRef, effect: RuleEffect, age: Duration| Rule {
        id: id.into(),
        source,
        sender: None,
        effect,
        created: now - age,
    };
    vec![
        rule("5e0b7c2a", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }, Duration::days(6)),
        rule("a81f3d94", SourceRef::script("nightly-build"), RuleEffect::Muted, Duration::days(3)),
        rule("2c6e9b17", SourceRef::script("backup"), RuleEffect::Lane { lane: Lane::Info }, Duration::days(1)),
    ]
}

/// The demo's clients: Claude Code with the default template, Codex with rights of its own
/// (fewer areas, an admin switch, chosen calendars, repositories and sources, asking before every
/// change), the hooks of Claude Code and a script that has never been used.
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
    ]
}

/// The sample answer `--demo` gives to a request whose answer is a state of its own: an agent's
/// activity, a new key and a new client of "Connect agent" (made-up keys, never working ones; the
/// new client is not added to the list). Every other request gets none and is dropped, so the
/// demo changes nothing.
pub fn demo_answer(request: &Request, data: &Data) -> Option<Response> {
    let client_of = |id: &str| data.clients.iter().find(|client| client.id == id);
    match request {
        Request::DiscoverServices { address } => Some(demo_discovery(address)),
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

/// The demo's approvals: summaries as the core builds them (the same texts, so they are translated
/// too), from two agents.
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
    vec![
        approval(
            "9c1f0a7e3b2d4c58",
            claude,
            delete_event("Team meeting", "Tue, 14 Oct 09:00", "Work"),
            Duration::minutes(2),
            ApprovalState::Pending,
        ),
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
            err.to_string()
                .ends_with("valid values: overview, accounts, workspaces, apps, agents, approvals, inbox, general"),
            "{err}"
        );
        let err = Launch::parse(&args(&["--appearance", "sepia"])).expect_err("unknown appearance");
        assert!(err.to_string().ends_with("valid values: light, dark, system"), "{err}");
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        assert_eq!(Launch::parse(&args(&["--open", "wizard"])), Err(LaunchError::NeedsDemo("--open")));
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
    fn the_demo_has_a_mail_provider_account() {
        let data = demo_data();
        let caldav = data.accounts.iter().find(|account| account.kind == "caldav").expect("a CalDAV account");
        assert!(matches!(caldav.state, AccountState::Connected { problem: None, .. }));
        assert!(caldav.collections.iter().any(|collection| collection.kind.has_events()));
        assert!(caldav.collections.iter().any(|collection| collection.kind.has_todos()));
    }
}
