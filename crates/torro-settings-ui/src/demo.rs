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
use torro_core::settings::{AssistantDefault, CalendarReminders, PdfTemplateId, Reminders, Settings, ToolId, Tools};
use torro_core::theme::Appearance;
use torro_inbox::item::{Facet, FacetKey};
use torro_inbox::{Lane, Rule, RuleEffect, SourceInfo, SourceRef};
use torro_ipc::api::{ASSISTANT_CLIENT, Area, AreaRights, BOT_CLIENT, Level, Rights, Selection};
use torro_ipc::assistant::{Engine, EngineKind, LETTER_AVATAR, Note, Profile};
use torro_ipc::bots::BotSummary;
use torro_ipc::tools::ToolInfo;
use torro_ipc::{
    AccountInfo, ClientInfo, FolderChoice, MailView, NtfyTopicView, NtfyView, OpenRoleView, Request, Response,
    RuleHits, ServiceState, VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus,
};
use torro_mail::{Endpoint, MailFormat, MailSettings, Notify, Role, Security};
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

/// Sample data for the sections, answered from memory.
#[derive(Debug, Default)]
pub struct Demo {
    settings: Settings,
    sent: Mutex<Vec<Request>>,
}

impl Demo {
    /// A demo with the sample settings: dark, so the first look shows the suite's own look.
    pub fn new() -> Demo {
        let mut reminders = Reminders::default();
        // The PDF tool on, with a folder and a logo of its own (task WZ.6), so Tools shows the
        // switch on, "Use desktop" and "Remove". Paths only: the demo reads no file.
        let mut tools = Tools::default();
        tools.set_on(ToolId::Pdf, true);
        tools.pdf.folder = Some("/home/anna/Documents/PDFs".into());
        tools.pdf.template_mut(PdfTemplateId::Report).logo = Some("/home/anna/Pictures/acme-logo.png".into());
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
        Demo { settings, sent: Mutex::new(Vec::new()) }
    }

    /// The sample settings it answers `settings.settings` with.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The assistant's client, so the sections show the assistant as on, with one command switched
    /// off, so the rights editor shows what that looks like (task S.33); and the client of the
    /// sample bot ([`Demo::bots`]), which reads two calendars, so Access has a bot to open (task
    /// S.34). The demo only pretends to switch or change them.
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
                Rights { denied: ["calendar.list_reminders".to_string()].into(), ..Rights::assistant() },
            ),
            client("c0000001", "Travel planner", BOT_CLIENT, travel),
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
                mail: ServiceState::default(),
                calendars: None,
            }),
            ntfy: None,
            services: None,
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

    /// One sample bot.
    fn bots(&self) -> Vec<BotSummary> {
        vec![BotSummary {
            id: "b0000001".into(),
            name: "Travel planner".into(),
            description: "Plans trips by train".into(),
            avatar: LETTER_AVATAR.into(),
            engine: "ai:ollama@localhost:11434".into(),
            model: None,
            client: "c0000001".into(),
        }]
    }

    /// The assistant's own tools as `tools.list_tools` lists them for the app key: switched on as the
    /// settings say, `allowed` always (the app key may use every tool). The assistant's client has
    /// no access to `tools` ([`Rights::assistant`]), so Tools shows where it is allowed.
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
                },
                enabled: self.settings.tools.is_on(id),
                allowed: true,
            })
            .collect()
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
            Request::ListEngines => Response::Engines { engines: self.engines() },
            Request::AiModels { account, .. } => Response::AiModels { account: account.clone(), models: self.models() },
            Request::AiAssignments => Response::AiAssignments { assignments: self.assignments() },
            Request::GetCategorization => Response::Categorization { categorization: self.categorization() },
            Request::VoiceStatus => Response::Voice { voice: self.voice() },
            Request::Accounts => Response::Accounts { accounts: self.accounts() },
            Request::CalendarCollections => Response::Collections { collections: self.collections(true) },
            Request::TaskCollections => Response::Collections { collections: self.collections(false) },
            Request::ListSources => Response::Sources { sources: self.sources() },
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
