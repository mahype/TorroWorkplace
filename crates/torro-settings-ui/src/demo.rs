//! The [`Backend`] of `--demo`: sample data from memory, never a socket.
//!
//! Every app can be started with `--demo` for a look at its settings (design skill, section 9).
//! A window started that way must not change anything real – `torro_ipc::client` reads the app
//! key, so a demo on the socket would really switch rights and settings. The apps therefore hand
//! the [`crate::Host`] this backend instead of their own, and nothing in this library knows the
//! socket at all.
//!
//! Like the control centre's `--demo`, the demo keeps nothing: a change is answered as done and
//! the next load shows the sample data again. It does remember what it was asked to send, which
//! is what the tests of the sections check ([`Demo::sent`]).

use std::sync::Mutex;

use chrono::{TimeZone, Utc};
use torro_core::ai::AiModel;
use torro_core::i18n::tr;
use torro_core::settings::{AssistantDefault, CalendarReminders, Reminders, Settings, ToolId, Tools};
use torro_core::theme::Appearance;
use torro_inbox::item::{Facet, FacetKey};
use torro_inbox::{Lane, Rule, RuleEffect, SourceInfo, SourceRef};
use torro_ipc::api::{ASSISTANT_CLIENT, Access, Area, AreaRights, BOT_CLIENT, Level, Rights, Selection};
use torro_ipc::assistant::{Engine, EngineKind, LETTER_AVATAR, Note, Profile};
use torro_ipc::bots::BotSummary;
use torro_ipc::extensions::{ExtensionInfo, ExtensionTool, ServerSpec, Source, Status, ToolState};
use torro_ipc::inbox::{
    CategoryClearAccount, CategoryClearJob, CategoryClearPreview, CategoryClearProgress, CategoryJob,
    CategoryJobAccount, CategoryJobControl, CategoryJobPreview, CategoryJobScope, CategoryJobState,
};
use torro_ipc::mail::{
    MailAccountStats, MailCacheAction, MailCacheEstimate, MailCacheJob, MailCacheJobState, MailCacheMode,
    MailCachePolicy, MailFolderStats, MailMonthCount, MailQuota,
};
use torro_ipc::tools::ToolInfo;
use torro_ipc::{
    AccountInfo, ClientInfo, FolderChoice, MailView, NtfyTopicView, NtfyView, OpenRoleView, Request, Response,
    RuleHits, ServiceState, VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus,
};
use torro_mail::{Endpoint, Folder, MailFormat, MailSettings, Notify, Role, Security};
use torro_pim::{AccountState, Collection, CollectionKind};

use crate::{Answer, Backend};

/// The calendar the sample reminders single out (task S.19): "Work" of the first account, shown
/// with "Full screen" instead of the default "Notification".
const WORK_CALENDAR: &str = "nextcloud:anna@cloud.example.org/work";

/// The calendar the sample settings hide (task S.19): "Shared" of the second account, so
/// "Calendars" shows a switch turned off.
const SHARED_CALENDAR: &str = "nextcloud:ben@cloud.example.org/shared";

/// The sample mail account (task S.20), for the mail window's "Accounts" and the inbox's
/// "Sources".
const MAIL_ADDRESS: &str = "anna@mailbox.org";

/// One folder of the sample mail account ([`Demo::mail_stats`]): `index` is full, partial or
/// unverified, unavailable and awaiting; `originals` is count and bytes.
struct DemoFolder {
    id: &'static str,
    name: &'static str,
    role: Option<Role>,
    total: u32,
    unread: u32,
    bodies: u64,
    index: [u64; 4],
    oldest: Option<chrono::DateTime<Utc>>,
    server_bytes: u64,
    originals: (u64, u64),
}

/// Six years of the sample account's mail, November 2020 to October 2026, a month at a time:
/// together its 24,813 messages, of which the newest 11,000 are readable offline.
fn demo_months() -> Vec<MailMonthCount> {
    const MESSAGES: u64 = 24_813;
    const OFFLINE: u64 = 11_000;
    let mut months: Vec<MailMonthCount> = (0..72u32)
        .map(|at| {
            // Months counted from January 2020; November 2020 is the tenth.
            let index = 10 + at;
            MailMonthCount {
                year: 2020 + (index / 12) as i32,
                month: index % 12 + 1,
                messages: 300 + u64::from(at * 37 % 90),
                cached_bodies: 0,
            }
        })
        .collect();
    let counted: u64 = months.iter().map(|month| month.messages).sum();
    if let Some(last) = months.last_mut() {
        last.messages = (last.messages + MESSAGES).saturating_sub(counted);
    }
    let mut offline = OFFLINE;
    for month in months.iter_mut().rev() {
        month.cached_bodies = month.messages.min(offline);
        offline -= month.cached_bodies;
    }
    months
}

/// Sample data for the sections, answered from memory.
#[derive(Debug, Default)]
pub struct Demo {
    settings: Settings,
    sent: Mutex<Vec<Request>>,
    category_job: Mutex<Option<CategoryJob>>,
    category_clear_job: Mutex<Option<CategoryClearJob>>,
    cache_policy: Mutex<Option<MailCachePolicy>>,
    cache_job: Mutex<Option<MailCacheJob>>,
    /// Why the sample mail account does not sync ([`Demo::with_mail_problem`]); `None` normally.
    mail_problem: Option<String>,
}

impl Demo {
    /// The sample demo whose mail account does not sync, for `problem` (task DS.6: the open question
    /// of a sync problem on the account's page, `torroinbox --open account-question`).
    pub fn with_mail_problem(problem: impl Into<String>) -> Demo {
        Demo { mail_problem: Some(problem.into()), ..Demo::new() }
    }

    /// A demo with the sample settings: dark, so the first look shows the suite's own look.
    pub fn new() -> Demo {
        let mut reminders = Reminders::default();
        // The PDF tool on (task WZ.6), so Access offers it without the mark of a tool switched off.
        // Its folder and templates are set in the control centre since task MA.7.
        let mut tools = Tools::default();
        tools.set_on(ToolId::Pdf, true);
        reminders.calendars.insert(WORK_CALENDAR.into(), CalendarReminders::FullScreen);
        let settings = Settings {
            appearance: Appearance::Dark,
            // An AI account with one of its models: the load's second round then really asks
            // `ai.list_models`, and the Model and Effort groups of General have rows to show.
            assistant_default: Some(AssistantDefault {
                engine: "ai:ollama@localhost:11434".into(),
                model: Some("qwen3:8b".into()),
                effort: None,
            }),
            // So "Calendars" (task S.19) shows a hidden calendar too, with its switch off.
            hidden_collections: vec![SHARED_CALENDAR.into()],
            reminders,
            tools,
            ..Settings::default()
        };
        Demo {
            settings,
            sent: Mutex::new(Vec::new()),
            category_job: Mutex::new(None),
            category_clear_job: Mutex::new(None),
            cache_policy: Mutex::new(None),
            cache_job: Mutex::new(None),
            mail_problem: None,
        }
    }

    /// The sample settings it answers `settings.settings` with.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The assistant's client, so the sections show the assistant as on, with one command switched
    /// off, so the rights editor shows what that looks like (task S.33), and reading the MCP server
    /// "Time" with one of its tools switched off (task MA.10, [`Demo::extensions`]); and the clients of the
    /// sample bots ([`Demo::bots`]): the travel planner reads two calendars, so Access has a bot to
    /// open (task S.34), the release helper may use nothing. The demo only pretends to switch or
    /// change them.
    fn clients(&self) -> Vec<ClientInfo> {
        let created = Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).single().unwrap_or_else(Utc::now);
        let client = |id: &str, name: &str, kind: &str, rights: Rights| ClientInfo {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            created,
            last_used: None,
            rights,
            agent: None,
        };
        let mut travel = Rights::none();
        travel.areas.insert(Area::Calendar, AreaRights { level: Level::Read, admin: false });
        travel.calendars =
            Selection::Only(vec![WORK_CALENDAR.into(), "nextcloud:anna@cloud.example.org/family".into()]);
        vec![
            client(
                "a51c3e88",
                "Assistant",
                ASSISTANT_CLIENT,
                Rights {
                    denied: ["calendar.list_reminders".to_string(), "ext.time.convert_time".to_string()].into(),
                    extensions: [("time".to_string(), Level::Read)].into(),
                    ..Rights::assistant()
                },
            ),
            client("c0000001", tr("Travel planner"), BOT_CLIENT, travel),
            client("c0000002", tr("Release helper"), BOT_CLIENT, Rights::none()),
        ]
    }

    /// Two accounts with calendars and task lists, for "Calendars" (task S.19) and "Lists" to
    /// group by: a Nextcloud account with a calendar, a family calendar and a personal list, and
    /// a second one with a calendar the sample settings hide ([`SHARED_CALENDAR`]). The Shared
    /// calendar is read-only, so a row shows what "Read-only" looks like.
    ///
    /// Built as the structs they are, not parsed from JSON: a sample that cannot be read would
    /// have to be unwrapped, and the workspace allows no `expect` outside tests.
    fn accounts(&self) -> Vec<AccountInfo> {
        let connected = |server: &str, login: &str| AccountState::Connected {
            server: server.to_string(),
            login_name: login.to_string(),
            last_sync: None,
            problem: None,
        };
        let account =
            |id: &str, kind: &str, label: &str, state: AccountState, collections: Vec<Collection>| AccountInfo {
                id: id.to_string(),
                kind: kind.to_string(),
                label: label.to_string(),
                state,
                collections,
                scopes: None,
                mail: None,
                ntfy: None,
                services: None,
            };
        let collection = |id: &str, name: &str, kind: CollectionKind, color: Option<&str>, writable: bool| Collection {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            color: color.map(str::to_string),
            writable,
            source: None,
        };
        vec![
            account(
                "nextcloud:anna@cloud.example.org",
                "nextcloud",
                "anna@cloud.example.org",
                connected("https://cloud.example.org", "anna"),
                vec![
                    collection(WORK_CALENDAR, "Work", CollectionKind::Events, Some("#3B6EF0"), true),
                    collection(
                        "nextcloud:anna@cloud.example.org/family",
                        "Family",
                        CollectionKind::Events,
                        Some("#1F9D68"),
                        true,
                    ),
                    collection(
                        "nextcloud:anna@cloud.example.org/personal",
                        "Personal",
                        CollectionKind::Todos,
                        None,
                        true,
                    ),
                ],
            ),
            account(
                "nextcloud:ben@cloud.example.org",
                "nextcloud",
                "ben@cloud.example.org",
                connected("https://cloud.example.org", "ben"),
                vec![collection(SHARED_CALENDAR, "Shared", CollectionKind::Events, Some("#B06F12"), false)],
            ),
            // The accounts the inbox's "Sources" shows (task S.20): GitHub, an ntfy server and a
            // mail account, the last one also the mail window's "Accounts".
            account(
                "github:anna-berg",
                "github",
                "anna-berg",
                connected("https://github.com", "anna-berg"),
                Vec::new(),
            ),
            self.ntfy_account(),
            self.mail_account(),
            // The local AI, so the mail window's "Categorization" has a model to offer.
            account("ai:ollama@localhost:11434", "ai", "Ollama", connected("http://localhost:11434", ""), Vec::new()),
        ]
    }

    /// The calendars (`events`) or the task lists of the sample accounts the suite shows, as
    /// `calendar.list_calendars` and `tasks.list_lists` answer: without the hidden ones
    /// ([`SHARED_CALENDAR`]). What the rights editor offers to choose from (task S.33).
    fn collections(&self, events: bool) -> Vec<Collection> {
        self.accounts()
            .into_iter()
            .flat_map(|account| account.collections)
            .filter(|collection| if events { collection.kind.has_events() } else { collection.kind.has_todos() })
            .filter(|collection| !self.settings.hidden_collections.contains(&collection.id))
            .collect()
    }

    /// The sample ntfy account: one server with two topics, whose subscriptions stay with the
    /// account in the control centre – "Sources" only mutes the whole server.
    fn ntfy_account(&self) -> AccountInfo {
        let topic = |label: &str| NtfyTopicView { label: label.to_string(), topic: None };
        AccountInfo {
            id: "ntfy:ntfy.sh".into(),
            kind: "ntfy".into(),
            label: "ntfy.sh".into(),
            state: AccountState::Connected {
                server: "https://ntfy.sh".into(),
                login_name: "anna".into(),
                last_sync: None,
                problem: None,
            },
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: Some(NtfyView {
                server: "https://ntfy.sh".into(),
                topics: vec![topic("Backups"), topic("Doorbell")],
                user: Some("anna".into()),
                login: true,
            }),
            services: None,
        }
    }

    /// The sample mail account: a sender name, a signature, formatted mail and one folder role the
    /// server left open, so the mail window's "Accounts" has every row to show (task S.20).
    fn mail_account(&self) -> AccountInfo {
        let endpoint = |host: &str, port: u16| Endpoint { host: host.to_string(), port, security: Security::Tls };
        let folder = |name: &str, path: &str| FolderChoice { name: name.to_string(), path: path.to_string() };
        let settings = MailSettings {
            signature: Some("Kind regards\nAnna".into()),
            notify: Notify::ForMeOnly,
            format: MailFormat::Html,
            ..MailSettings::default()
        };
        AccountInfo {
            id: format!("mail:{MAIL_ADDRESS}"),
            kind: "mail".into(),
            label: MAIL_ADDRESS.into(),
            state: AccountState::Connected {
                server: "imap.mailbox.org".into(),
                login_name: MAIL_ADDRESS.into(),
                last_sync: None,
                problem: None,
            },
            collections: Vec::new(),
            scopes: None,
            mail: Some(MailView {
                address: MAIL_ADDRESS.into(),
                sender_name: "Anna Example".into(),
                imap: endpoint("imap.mailbox.org", 993),
                smtp: endpoint("smtp.mailbox.org", 465),
                caldav: None,
                settings,
                open_roles: vec![OpenRoleView {
                    role: Role::Archive,
                    candidates: vec![folder("Archive", "INBOX.Archive"), folder("All Mail", "INBOX.All")],
                }],
                mail: ServiceState { last_sync: None, problem: self.mail_problem.clone() },
                calendars: None,
                trusted_certificates: Vec::new(),
            }),
            ntfy: None,
            services: None,
        }
    }

    /// Plausible sample numbers for the account inventory; no real mailbox is read in demo mode.
    /// Four folders (DS.1), so the account page has a table and "other folders"; the folders, the
    /// months and the originals add up to the account's numbers, the folder sizes to the quota.
    fn mail_stats(&self) -> Vec<MailAccountStats> {
        let account = format!("mail:{MAIL_ADDRESS}");
        let date = |year, month, day| Utc.with_ymd_and_hms(year, month, day, 9, 14, 0).single();
        let newest = Utc.with_ymd_and_hms(2026, 10, 21, 5, 57, 0).single();
        let months = demo_months();
        let oldest_cached_body = months
            .iter()
            .find(|month| month.cached_bodies > 0)
            .and_then(|month| date(month.year, month.month, if month.cached_bodies < month.messages { 15 } else { 1 }));
        let folder = |sample: DemoFolder| MailFolderStats {
            folder: Folder {
                id: sample.id.into(),
                account: account.clone(),
                name: sample.name.into(),
                path: sample.name.into(),
                role: sample.role,
                unread: sample.unread,
                total: sample.total,
            },
            read: u64::from(sample.total - sample.unread),
            oldest_known: sample.oldest,
            newest_known: newest,
            cached_bodies: sample.bodies,
            indexed_full: sample.index[0],
            indexed_partial_or_unverified: sample.index[1],
            text_unavailable: sample.index[2],
            awaiting_index: sample.index[3],
            server_bytes: Some(sample.server_bytes),
            originals: sample.originals.0,
            original_bytes: sample.originals.1,
        };
        let folders = vec![
            folder(DemoFolder {
                id: "f:demo-inbox",
                name: "Inbox",
                role: Some(Role::Inbox),
                total: 12_000,
                unread: 880,
                bodies: 8_000,
                index: [10_000, 1_500, 100, 400],
                oldest: date(2023, 2, 1),
                server_bytes: 1_524_000_000,
                originals: (2_400, 300_000_000),
            }),
            folder(DemoFolder {
                id: "f:demo-archive",
                name: "Archive",
                role: Some(Role::Archive),
                total: 10_513,
                unread: 348,
                bodies: 2_400,
                index: [9_000, 1_100, 150, 263],
                oldest: date(2020, 11, 3),
                server_bytes: 837_000_000,
                originals: (600, 80_000_000),
            }),
            folder(DemoFolder {
                id: "f:demo-sent",
                name: "Sent",
                role: Some(Role::Sent),
                total: 2_100,
                unread: 0,
                bodies: 600,
                index: [1_900, 150, 30, 20],
                oldest: date(2021, 3, 9),
                server_bytes: 150_000_000,
                originals: (200, 30_000_000),
            }),
            folder(DemoFolder {
                id: "f:demo-junk",
                name: "Junk",
                role: Some(Role::Junk),
                total: 200,
                unread: 20,
                bodies: 0,
                index: [100, 50, 20, 30],
                oldest: date(2026, 9, 1),
                server_bytes: 23_000_000,
                originals: (0, 0),
            }),
        ];
        vec![MailAccountStats {
            account: account.clone(),
            folders,
            messages: 24_813,
            read: 23_565,
            unread: 1_248,
            oldest_known: date(2020, 11, 3),
            newest_known: newest,
            cached_bodies: 11_000,
            oldest_cached_body,
            body_cache_bytes: 440_000_000,
            search_index_bytes: Some(140_000_000),
            last_index_run: newest,
            indexed_full: 21_000,
            indexed_partial_or_unverified: 2_800,
            text_unavailable: 300,
            awaiting_index: 713,
            database_bytes: 1_500_000_000,
            cached_attachments: 42,
            attachment_cache_bytes: Some(260_000_000),
            last_sync: newest,
            sync_problem: self.mail_problem.clone(),
            // 2.36 GB of 10 GB, as a server counting in KiB reports them.
            quota: newest.map(|checked| MailQuota {
                used_bytes: 2_534_000_000,
                limit_bytes: 10_485_760 * 1024,
                checked,
            }),
            months,
            originals: 3_200,
            original_bytes: 410_000_000,
        }]
    }

    /// Deliberately separate all-mail counts from the old 153-message catch-up sample.
    fn category_job_preview(&self, scope: &CategoryJobScope) -> CategoryJobPreview {
        let all = scope.since.is_none() && scope.folders.is_empty();
        let (total, unclassified, automatic, protected) =
            if all { (24_813, 3_012, 19_516, 2_285) } else { (153, 53, 88, 12) };
        CategoryJobPreview {
            total,
            unclassified,
            automatic,
            protected,
            eligible: unclassified + if scope.again { automatic } else { 0 },
            request_scope: Some(scope.clone()),
        }
    }

    /// The sample inbox sources (`inbox.list_sources`, task S.20): Talk is muted, so "Sources"
    /// shows a switch turned off; the others are on.
    fn sources(&self) -> Vec<SourceInfo> {
        let source = |source: SourceRef, open, muted| SourceInfo { source, open, muted, health: None };
        vec![
            source(SourceRef::nextcloud("anna@cloud.example.org"), 3, false),
            source(SourceRef::talk("anna@cloud.example.org"), 0, true),
            source(SourceRef::github("anna-berg"), 2, false),
            source(SourceRef::ntfy("ntfy.sh"), 1, false),
            source(SourceRef::mail(MAIL_ADDRESS), 7, false),
        ]
    }

    /// The sample rules with what they caught (`inbox.list_rules`, task S.20): a remembered lane,
    /// a muted source, a hidden sender and a muted trait – one of each kind the section shows.
    fn rules(&self) -> (Vec<Rule>, Vec<RuleHits>) {
        let made = |day| Utc.with_ymd_and_hms(2026, 10, day, 9, 0, 0).single().unwrap_or_else(Utc::now);
        let rule = |id: &str, source: SourceRef, effect, day| Rule {
            id: id.into(),
            source,
            sender: None,
            domain: None,
            effect,
            created: made(day),
            facet: None,
            until: None,
        };
        let rules = vec![
            rule("r1", SourceRef::script("nightly-build"), RuleEffect::Lane { lane: Lane::Info }, 10),
            rule("r2", SourceRef::script("cron"), RuleEffect::Muted, 11),
            Rule {
                sender: Some("news@shop.example".into()),
                ..rule("r3", SourceRef::mail(MAIL_ADDRESS), RuleEffect::Muted, 12)
            },
            Rule {
                facet: Some(Facet { key: FacetKey::Bulk, value: "bulk".into() }),
                ..rule("r4", SourceRef::mail(MAIL_ADDRESS), RuleEffect::Lane { lane: Lane::Info }, 13)
            },
        ];
        let hits = vec![
            RuleHits { rule: "r1".into(), hits: 9, since: made(10) },
            RuleHits { rule: "r2".into(), hits: 31, since: made(11) },
            RuleHits { rule: "r3".into(), hits: 12, since: made(12) },
            RuleHits { rule: "r4".into(), hits: 0, since: made(13) },
        ];
        (rules, hits)
    }

    /// Who the sample assistant is.
    fn profile(&self) -> Profile {
        Profile {
            name: "Torro".into(),
            description: "Your assistant for this computer".into(),
            avatar: LETTER_AVATAR.into(),
            personality: "Be calm and brief.".into(),
        }
    }

    /// Two notes of the sample memory, oldest first.
    fn memories(&self) -> Vec<Note> {
        let at = |day| Utc.with_ymd_and_hms(2026, 10, day, 9, 0, 0).single().unwrap_or_else(Utc::now);
        vec![
            Note { id: "1a2b3c4d".into(), text: "Anna works in sales.".into(), created: at(12), local_only: false },
            Note { id: "5e6f7a8b".into(), text: "ACME owes 400 €.".into(), created: at(14), local_only: true },
        ]
    }

    /// The sample bots of the assistant's demo (`torroassistant::demo`), so Access lists what its
    /// sidebar does: "Release helper", which may use nothing yet, and "Travel planner".
    fn bots(&self) -> Vec<BotSummary> {
        let release = BotSummary {
            id: "b0000002".into(),
            name: tr("Release helper").into(),
            description: tr("Writes release notes and checks what is open").into(),
            avatar: "radar".into(),
            engine: "ai:openrouter@openrouter.ai".into(),
            model: Some("qwen/qwen3-32b".into()),
            client: "c0000002".into(),
        };
        vec![
            release,
            BotSummary {
                id: "b0000001".into(),
                name: tr("Travel planner").into(),
                description: tr("Plans trips by train").into(),
                avatar: LETTER_AVATAR.into(),
                engine: "ai:ollama@localhost:11434".into(),
                model: None,
                client: "c0000001".into(),
            },
        ]
    }

    /// The assistant's own tools as `tools.list_tools` lists them for the app key: switched on as the
    /// settings say, `allowed` always (the app key may use every tool). The assistant's client has
    /// no access to `tools` ([`Rights::assistant`]), so Access shows the tool not chosen.
    fn tools(&self) -> Vec<ToolInfo> {
        ToolId::ALL
            .into_iter()
            .map(|id| ToolInfo {
                id,
                name: id.title().to_string(),
                description: match id {
                    ToolId::Pdf => {
                        tr("Makes PDFs from the assistant's text, set after the templates in its settings.").to_string()
                    }
                    ToolId::Files => tr(
                        "Lists, finds, copies and moves files in the allowed folders, without reading what they hold.",
                    )
                    .to_string(),
                    ToolId::Web => tr("Searches the web and reads web pages for the assistant, with the providers \
                                       you switch on.")
                    .to_string(),
                },
                enabled: self.settings.tools.is_on(id),
                allowed: true,
            })
            .collect()
    }

    /// The installed MCP servers as `extensions.list_extensions` lists them (task MA.10), for the
    /// rows of Access under "Extensions": "Time", switched on, with two read tools a person allowed;
    /// "Notes", a server of one's own switched off after its tools were allowed, so its row shows the
    /// mark of a server that is off. Nothing runs: the demo starts no program.
    fn extensions(&self) -> Vec<ExtensionInfo> {
        let tool = |server: &str, name: &str, description: &str, access| ExtensionTool {
            extension: server.into(),
            extension_name: server.into(),
            name: name.into(),
            tool_name: torro_ipc::extensions::tool_name(server, name),
            description: description.into(),
            input_schema: serde_json::json!({ "type": "object" }),
            access,
            state: ToolState::Allowed,
        };
        let server = |id: &str, name: &str, command: &str, enabled, tools| ExtensionInfo {
            id: id.into(),
            name: name.into(),
            source: Source::Custom,
            server: ServerSpec::Stdio { command: command.into(), args: Vec::new(), env: Default::default() },
            enabled,
            status: Status::Stopped,
            tools,
            secrets: Vec::new(),
            notes: Vec::new(),
            fingerprint: Some(format!("demo-{id}")),
        };
        vec![
            server(
                "time",
                "Time",
                "uvx",
                true,
                vec![
                    tool("time", "get_current_time", "Get the current time in a time zone.", Access::Read),
                    tool("time", "convert_time", "Convert a time between time zones.", Access::Read),
                ],
            ),
            server(
                "notes",
                "Notes",
                "/usr/local/bin/notes-mcp",
                false,
                vec![
                    tool("notes", "search_notes", "Search the notes.", Access::Read),
                    tool("notes", "add_note", "Add a note.", Access::Write),
                    tool("notes", "delete_note", "Delete a note for good.", Access::Delete),
                ],
            ),
        ]
    }

    /// The sample engine: an AI account. Coding agents are not offered any more (task C.19,
    /// issue #219), and the demo shows what the product can do.
    fn engines(&self) -> Vec<Engine> {
        vec![Engine {
            id: "ai:ollama@localhost:11434".into(),
            kind: EngineKind::Ai,
            name: "Ollama".into(),
            problem: None,
            models: Vec::new(),
        }]
    }

    /// The models the sample AI account offers (`ai.list_models`): one that writes and one that
    /// only decides, which is what sorting mail needs (task E12.10), both turned on; and one turned
    /// off, which no choice offers (task S.26).
    fn models(&self) -> Vec<AiModel> {
        vec![
            AiModel {
                id: "qwen3:8b".into(),
                name: "qwen3:8b".into(),
                tools: Some(true),
                open_weights: Some(true),
                context: None,
                reasoning: Some(true),
                remote: Some(false),
                decision: None,
                enabled: true,
            },
            AiModel {
                id: "nimble".into(),
                name: "nimble".into(),
                tools: Some(false),
                open_weights: Some(true),
                context: None,
                reasoning: Some(false),
                remote: Some(false),
                decision: Some(true),
                enabled: true,
            },
            AiModel {
                id: "llama3.3:70b".into(),
                name: "llama3.3:70b".into(),
                tools: Some(true),
                open_weights: Some(true),
                context: None,
                reasoning: Some(false),
                remote: Some(false),
                decision: Some(true),
                enabled: false,
            },
        ]
    }

    /// What each function of the suite uses (`ai.list_assignments`): sorting mail is set up with
    /// the local deciding model. Every function of the list, so a new one shows up here by
    /// itself – unassigned until the sample says otherwise.
    fn assignments(&self) -> Vec<torro_ipc::AiAssignmentInfo> {
        use torro_core::ai::{AiAssignment, AiFunction};
        AiFunction::ALL
            .into_iter()
            .map(|function| {
                let mut assignment = AiAssignment::none(function);
                if function == AiFunction::InboxCategorize {
                    assignment.engine = Some("ai:ollama@localhost:11434".into());
                    assignment.model = Some("nimble".into());
                    assignment.enabled = true;
                }
                torro_ipc::AiAssignmentInfo {
                    assignment,
                    label: function.label().to_string(),
                    needs_decision: function.needs_decision(),
                }
            })
            .collect()
    }

    /// The catalogue of mail categories (`inbox.list_categories`): the suite's own, one renamed,
    /// and one of the person's own (task E15.5).
    fn categories(&self) -> Vec<torro_ipc::inbox::CategoryInfo> {
        let mut defs = torro_mail::CategoryDef::defaults();
        if let Some(invoice) = defs.iter_mut().find(|def| def.category == torro_mail::Category::Invoice) {
            invoice.name = Some("Bills".into());
        }
        if let Some(tax) = torro_mail::CustomCategory::new(1) {
            defs.push(torro_mail::CategoryDef {
                category: torro_mail::Category::Custom(tax),
                group: torro_mail::CategoryGroup::PaperTrail,
                name: Some("Tax".into()),
                description: Some(
                    "Anything for the tax return: the tax office, the adviser, receipts asked for.".into(),
                ),
                model: true,
                icon: torro_mail::CategoryIcon::Tag,
            });
        }
        defs.iter().map(|def| torro_ipc::inbox::CategoryInfo::of(def, 0, 0)).collect()
    }

    /// The sorting rules (`inbox.list_sort_rules`, task E15.5): a kept sender, a domain with an
    /// exception, and one switched off.
    fn sort_rules(&self) -> Vec<torro_inbox::SortRule> {
        use torro_inbox::{SortCondition, SortField, SortRule};
        let on = |field, value: &str| SortCondition { field, value: value.into() };
        let rule = |id: &str, category, conditions, exceptions, enabled| SortRule {
            id: id.into(),
            category,
            conditions,
            exceptions,
            enabled,
            created: chrono::DateTime::from_timestamp(1_760_000_000, 0).unwrap_or_default(),
        };
        let tax =
            torro_mail::CustomCategory::new(1).map_or(torro_mail::Category::Document, torro_mail::Category::Custom);
        vec![
            rule(
                "r1",
                torro_mail::Category::Invoice,
                vec![on(SortField::Sender, "billing@shop.example")],
                vec![],
                true,
            ),
            rule(
                "r2",
                tax,
                vec![on(SortField::Domain, "finanzamt.example")],
                vec![on(SortField::Subject, "newsletter")],
                true,
            ),
            rule("r3", torro_mail::Category::Promotion, vec![on(SortField::Subject, "sale")], vec![], false),
        ]
    }

    /// How far the sample inbox is sorted (`inbox.get_categorization`): most of it done, the rest
    /// waiting for the backlog run.
    fn categorization(&self) -> torro_ipc::inbox::Categorization {
        torro_ipc::inbox::Categorization {
            enabled: true,
            engine: Some("ai:ollama@localhost:11434".into()),
            model: Some("nimble".into()),
            accounts: vec![torro_ipc::inbox::CategoryProgress {
                account: format!("mail:{MAIL_ADDRESS}"),
                messages: 212,
                done: 184,
                state: torro_ipc::inbox::CatchUpState::NotStarted,
            }],
            again: false,
            clear_job: self.category_clear_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
        }
    }

    /// How voice stands on the sample computer: everything there but the wake word, one
    /// microphone.
    fn voice(&self) -> VoiceStatus {
        let found = |part, program: &str| VoiceProgram { part, program: Some(program.into()), missing: None };
        VoiceStatus {
            listener: VoiceState::Off,
            installed: true,
            problem: None,
            programs: vec![
                VoiceProgram { part: VoicePart::WakeWord, program: None, missing: Some(VoiceMissing::Program) },
                found(VoicePart::Recorder, "parec"),
                found(VoicePart::SpeechToText, "whisper-cli"),
                found(VoicePart::Speech, "piper"),
            ],
            microphones: vec!["alsa_input.usb".into()],
            folder: Some("/home/anna/.local/share/torroassistant/voice".into()),
        }
    }

    /// The save targets (`mail.list_save_targets`, task M12.7): "Invoices" by month and suggested for
    /// invoices and receipts that are PDFs, "Contracts" by sender and never suggested – the two of
    /// the design and of the mail window's demo. Paths only: the demo reads no folder.
    fn save_targets(&self) -> Vec<torro_mail::SaveTarget> {
        use torro_mail::{Category, DateSource, FileKind, OnConflict, SaveTarget, SaveTargetSpec};
        let mut invoices = SaveTargetSpec::new("Invoices", "/home/anna/Documents/Finance/Invoices");
        invoices.date_from = DateSource::FileName;
        invoices.suggest.categories = vec![Category::Invoice, Category::Receipt];
        invoices.suggest.kinds = vec![FileKind::Pdf];
        invoices.on_conflict = OnConflict::SkipIdentical;
        let mut contracts = SaveTargetSpec::new("Contracts", "/home/anna/Documents/Contracts");
        contracts.subfolders = "{sender}".into();
        contracts.file_name = "{date}_{name}.{ext}".into();
        vec![
            SaveTarget { id: "demo-invoices".into(), spec: invoices },
            SaveTarget { id: "demo-contracts".into(), spec: contracts },
        ]
    }

    /// The save targets as `mail.list_save_targets` answers them, with the Downloads folder.
    fn save_targets_answer(&self) -> Response {
        let downloads = self.settings.mail_attachments.downloads.clone();
        Response::SaveTargets {
            targets: self.save_targets(),
            downloads: downloads.unwrap_or_else(|| "/home/anna/Downloads".into()),
            files: Vec::new(),
        }
    }

    /// Every request it was asked to send, in order: what a section's test looks at.
    pub fn sent(&self) -> Vec<Request> {
        match self.sent.lock() {
            Ok(sent) => sent.clone(),
            // A poisoned lock means a test panicked while holding it; there is nothing to report.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Notes `request` as sent.
    fn note(&self, request: Request) {
        match self.sent.lock() {
            Ok(mut sent) => sent.push(request),
            Err(poisoned) => poisoned.into_inner().push(request),
        }
    }
}

impl Backend for Demo {
    /// Answers what the sections read with sample data and everything else as done, after noting
    /// it ([`Demo::sent`]). A request the demo has no sample for is answered as done too: a
    /// section shows its empty state then instead of an error nobody can act on.
    fn call(&self, request: Request) -> Answer {
        let answer = match &request {
            Request::Settings => Response::Settings { settings: self.settings.clone() },
            Request::ListClients => Response::Clients { clients: self.clients() },
            Request::GetProfile => Response::Profile { profile: self.profile() },
            Request::ListMemories => Response::Memories { notes: self.memories() },
            Request::ListBots => Response::Bots { bots: self.bots() },
            Request::ListTools => Response::Tools { tools: self.tools() },
            Request::ListExtensions => Response::Extensions { extensions: self.extensions() },
            Request::ListEngines => Response::Engines { engines: self.engines() },
            Request::AiModels { account, .. } => Response::AiModels { account: account.clone(), models: self.models() },
            Request::AiAssignments => Response::AiAssignments { assignments: self.assignments() },
            Request::GetCategorization => Response::Categorization { categorization: self.categorization() },
            Request::PreviewClearCategories => Response::CategoryClearPreview {
                preview: CategoryClearPreview {
                    accounts: vec![CategoryClearAccount {
                        account: format!("mail:{MAIL_ADDRESS}"),
                        categorized: 21_801,
                    }],
                    total: 21_801,
                },
            },
            Request::ClearCategories { disable_automatic, .. } => {
                let now = Utc::now();
                *self.category_clear_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(CategoryClearJob {
                        id: "category-clear:demo".into(),
                        state: CategoryJobState::Running,
                        accounts: vec![CategoryClearProgress {
                            account: format!("mail:{MAIL_ADDRESS}"),
                            max_sequence: 24_813,
                            cursor: 1_024,
                            categorized: 21_801,
                            removed: 1_024,
                        }],
                        disable_automatic: *disable_automatic,
                        entries_cleared: false,
                        errors: Vec::new(),
                        started_at: now,
                        updated_at: now,
                        generation: 1,
                    });
                self.note(request);
                Response::Categorization { categorization: self.categorization() }
            }
            Request::ControlClearCategories { action, .. } => {
                if let Some(job) =
                    self.category_clear_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut()
                {
                    job.state = match action {
                        CategoryJobControl::Pause => CategoryJobState::Paused,
                        CategoryJobControl::Resume => CategoryJobState::Running,
                        CategoryJobControl::Cancel => CategoryJobState::Cancelled,
                    };
                }
                self.note(request);
                Response::Categorization { categorization: self.categorization() }
            }
            Request::PreviewCategoryJob { scope } => {
                Response::CategoryJobPreview { preview: self.category_job_preview(scope) }
            }
            Request::GetCategoryJob { .. } => Response::CategoryJob {
                job: self.category_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
            },
            Request::StartCategoryJob { scope } => {
                let preview = self.category_job_preview(scope);
                let now = Utc::now();
                let job = CategoryJob {
                    id: "category-job:demo".into(),
                    scope: scope.clone(),
                    state: CategoryJobState::Running,
                    accounts: vec![CategoryJobAccount {
                        account: format!("mail:{MAIL_ADDRESS}"),
                        max_sequence: preview.total as i64,
                        cursor: 11,
                        eligible: preview.eligible,
                        processed: 11,
                        changed: 8,
                        failed: 0,
                    }],
                    errors: Vec::new(),
                    started_at: now,
                    updated_at: now,
                    generation: 1,
                    model_in_cloud: Some(false),
                };
                *self.category_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(job.clone());
                self.note(request);
                Response::CategoryJob { job: Some(job) }
            }
            Request::ControlCategoryJob { action, .. } => {
                let mut stored = self.category_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(job) = stored.as_mut() {
                    job.state = match action {
                        CategoryJobControl::Pause => CategoryJobState::Paused,
                        CategoryJobControl::Resume => CategoryJobState::Running,
                        CategoryJobControl::Cancel => CategoryJobState::Cancelled,
                    };
                }
                let job = stored.clone();
                drop(stored);
                self.note(request);
                Response::CategoryJob { job }
            }
            Request::ListCategories { .. } => Response::Categories { categories: self.categories() },
            Request::ListSortRules => Response::SortRules { rules: self.sort_rules() },
            Request::VoiceStatus => Response::Voice { voice: self.voice() },
            Request::Accounts => Response::Accounts { accounts: self.accounts() },
            Request::MailAccountStats { .. } => Response::MailAccountStats { accounts: self.mail_stats() },
            Request::GetMailCachePolicy { account } => Response::MailCachePolicy {
                policy: self.cache_policy.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone().unwrap_or(
                    MailCachePolicy {
                        account: account.clone(),
                        mode: MailCacheMode::Recent90Days,
                        prefetch_attachments: false,
                        attachment_retention_days: 1,
                    },
                ),
            },
            Request::SetMailCachePolicy { policy } => {
                *self.cache_policy.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(policy.clone());
                self.note(request.clone());
                Response::MailCachePolicy { policy: policy.clone() }
            }
            Request::PreviewMailCache { account, mode } => Response::MailCacheEstimate {
                estimate: MailCacheEstimate {
                    account: account.clone(),
                    current_bodies: 11_000,
                    bodies_to_fetch: match mode {
                        MailCacheMode::HeadersOnly => 0,
                        MailCacheMode::Recent90Days => 800,
                        MailCacheMode::AllOffline => 13_813,
                    },
                    bodies_to_remove: match mode {
                        MailCacheMode::HeadersOnly => 11_000,
                        MailCacheMode::Recent90Days => 4_000,
                        MailCacheMode::AllOffline => 0,
                    },
                    estimated_download_bytes: 210_000_000,
                    estimated_reclaim_bytes: 160_000_000,
                },
            },
            Request::StartMailCacheJob { account, action } => {
                let job = MailCacheJob {
                    id: "cache:demo".into(),
                    account: account.clone(),
                    action: *action,
                    state: MailCacheJobState::Running,
                    total: if *action == MailCacheAction::CleanCache { 4_000 } else { 13_813 },
                    processed: 411,
                    cursor: None,
                    estimated_bytes: 210_000_000,
                    error: None,
                };
                *self.cache_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(job.clone());
                self.note(request);
                Response::MailCacheJob { job: Some(job) }
            }
            Request::MailCacheJobStatus { .. } => Response::MailCacheJob {
                job: self.cache_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
            },
            Request::ControlMailCacheJob { resume, .. } => {
                let mut job = self.cache_job.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(job) = job.as_mut() {
                    job.state = if *resume { MailCacheJobState::Running } else { MailCacheJobState::Paused };
                }
                let answer = Response::MailCacheJob { job: job.clone() };
                drop(job);
                self.note(request);
                answer
            }
            Request::CalendarCollections => Response::Collections { collections: self.collections(true) },
            Request::TaskCollections => Response::Collections { collections: self.collections(false) },
            Request::ListSources => Response::Sources { sources: self.sources() },
            Request::ListSaveTargets { .. } => self.save_targets_answer(),
            // A change of a save target is answered as stored, under a new id or its own, and noted;
            // the next load shows the sample targets again.
            Request::CreateSaveTarget { target } => {
                let target = torro_mail::SaveTarget { id: "demo-new".into(), spec: target.clone() };
                self.note(request);
                Response::SaveTarget { target }
            }
            Request::UpdateSaveTarget { id, target } => {
                let target = torro_mail::SaveTarget { id: id.clone(), spec: target.clone() };
                self.note(request);
                Response::SaveTarget { target }
            }
            Request::RemoveSaveTarget { .. } => {
                self.note(request);
                self.save_targets_answer()
            }
            Request::ListRules => {
                let (rules, hits) = self.rules();
                Response::Rules { rules, hits }
            }
            // The answer is what "Say something" shows, so the demo has words for it; it is still
            // noted, because a test is something the window asked for.
            Request::TestVoice => {
                self.note(request);
                Response::Transcript { text: "What is on today?".into() }
            }
            _ => {
                self.note(request);
                Response::Ok
            }
        };
        Box::pin(async move { Ok(answer) })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Reading answers with the sample data, without any daemon.
    #[tokio::test]
    async fn settings_come_from_memory() {
        let demo = Demo::new();
        let answer = demo.call(Request::Settings).await.expect("the demo answers");
        assert_eq!(answer, Response::Settings { settings: demo.settings().clone() });
        assert!(demo.sent().is_empty(), "reading is not a change");
    }

    /// Task MA.10: the MCP servers come with a load that asks for them, so Access offers each a
    /// level; the assistant's client reads "Time", one of its tools switched off.
    #[tokio::test]
    async fn the_servers_reach_access() {
        let demo: Arc<dyn Backend> = Arc::new(Demo::new());
        let data = crate::load(demo, vec![Request::ListClients, Request::ListExtensions]).await;
        let known = crate::rights::Known::of(&data);
        let ids: Vec<&str> = known.extensions.iter().map(|server| server.id.as_str()).collect();
        assert_eq!(ids, ["time", "notes"]);
        assert!(known.extensions.iter().any(|server| !server.enabled), "one switched off, with its mark");
        let assistant = data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant");
        assert_eq!(assistant.rights.extension("time"), Level::Read);
        let listed = crate::rights::server_tools(&known.extensions[0], &assistant.rights);
        assert_eq!(listed.len(), 2, "both read tools");
        assert_eq!(crate::rights::server_off_count(&known.extensions[0], &assistant.rights), 1);
    }

    /// DS.1 (#314): the sample account's folders, months and originals add up to its numbers, and
    /// the folder sizes to the quota in use.
    #[test]
    fn the_sample_mail_numbers_add_up() {
        let stats = Demo::new().mail_stats().remove(0);
        let folders = &stats.folders;
        assert_eq!(folders.len(), 4);
        assert_eq!(folders.iter().map(|folder| u64::from(folder.folder.total)).sum::<u64>(), stats.messages);
        assert_eq!(folders.iter().map(|folder| u64::from(folder.folder.unread)).sum::<u64>(), stats.unread);
        assert_eq!(folders.iter().map(|folder| folder.cached_bodies).sum::<u64>(), stats.cached_bodies);
        assert_eq!(folders.iter().map(|folder| folder.indexed_full).sum::<u64>(), stats.indexed_full);
        assert_eq!(folders.iter().map(|folder| folder.awaiting_index).sum::<u64>(), stats.awaiting_index);
        assert_eq!(folders.iter().map(|folder| folder.originals).sum::<u64>(), stats.originals);
        assert_eq!(folders.iter().map(|folder| folder.original_bytes).sum::<u64>(), stats.original_bytes);
        let on_server: u64 = folders.iter().filter_map(|folder| folder.server_bytes).sum();
        assert_eq!(Some(on_server), stats.quota.map(|quota| quota.used_bytes));
        assert_eq!(stats.months.len(), 72, "six years");
        assert_eq!(stats.months.iter().map(|month| month.messages).sum::<u64>(), stats.messages);
        assert_eq!(stats.months.iter().map(|month| month.cached_bodies).sum::<u64>(), stats.cached_bodies);
        assert!(stats.months.iter().all(|month| month.cached_bodies <= month.messages && month.messages > 0));
    }

    /// A change is answered as done and remembered, but nothing is kept.
    #[tokio::test]
    async fn a_change_is_remembered_but_not_kept() {
        let demo = Arc::new(Demo::new());
        let request = Request::SetSetting { key: torro_ipc::SettingKey::Appearance, value: serde_json::json!("light") };
        assert_eq!(demo.call(request.clone()).await, Ok(Response::Ok));
        assert_eq!(demo.sent(), vec![request]);
        assert_eq!(demo.settings().appearance, Appearance::Dark, "the demo stores nothing");
    }
}
