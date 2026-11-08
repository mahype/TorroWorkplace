//! The accounts of `--demo` (issue #410): the sample accounts, their inbox sources and the settings
//! they show, and the answers to what the account pages and the add-account dialog ask. The
//! control centre's demo and every app's ([`crate::demo::Demo`]) take them from here, so an account
//! looks the same in both. Nothing is stored and nothing reaches a server: a sign-in, a connection or
//! a switch is answered as done, and the next load shows the samples again.
//!
//! The rules the answers follow, as the control centre's demo has them since task S.6: the password
//! "wrong" is refused (for mail its IMAP server), "wrong-smtp" only by the SMTP server; the key
//! "wrong" of an AI provider too; the topics of `push.example.org` need a login; an IMAP server at
//! home (`.lan`) or on the public internet (`.example`) shows a certificate the system does not
//! trust ([`DEMO_FINGERPRINT`]); the search finds Ollama on this computer, unconnected.

use chrono::{DateTime, Duration, Local, Utc};
use torro_core::ai::{AiError, AiModel, AiProvider};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_inbox::item::SourceHealth;
use torro_inbox::{SourceInfo, SourceRef};
use torro_ipc::{
    AccountInfo, AiServer, FolderChoice, MailView, NtfyTopicView, NtfyView, OpenRoleView, Request, Response,
    ServerService, ServiceState, TorroMailAccount,
};
use torro_mail::{Endpoint, MailSettings, Notify, Role, Security, Services};
use torro_ntfy::NtfyError;
use torro_pim::{AccountState, Collection, CollectionKind};

use super::Seen;
use crate::Data;

/// The core's reasons why Gmail and Outlook.com cannot sign in to mail
/// (`torro_mail_imap::discover::MailUnsupported::reason`), as the search answers them.
pub const MAIL_UNSUPPORTED_GOOGLE: &str = "Gmail needs a sign-in through Google, which Torro Workplace does not support \
     yet. With 2-Step Verification on, an app password from your Google account works instead.";
pub const MAIL_UNSUPPORTED_MICROSOFT: &str =
    "Outlook.com and Microsoft 365 need a sign-in through Microsoft, which Torro Workplace does not support yet.";

/// The sample accounts with their inbox sources and the settings they show: a healthy Nextcloud
/// account with a shared read-only calendar, a second one that cannot reach its server (so the
/// problem rows show), a GitHub account, a mail provider's CalDAV account, two mail accounts, an
/// ntfy server and an AI account in the cloud. Times are relative to now.
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
        demo_network_account(),
    ];
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
    let settings = Settings {
        start_at_login: true,
        hidden_collections,
        // Issue #131: new chats start with a model of the connected AI account, thinking hard.
        ai_slots: torro_core::ai::AiSlots {
            chat: Some(torro_core::ai::SlotModel {
                engine: "ai:openrouter@openrouter.ai".into(),
                model: Some("qwen/qwen3-32b".into()),
                effort: Some(torro_core::ai::Effort::High),
            }),
            // Issue #325: mail is sorted in the cloud, by a chat model through the fallback, so the
            // group "Models" says where mail goes (with the consent, issue #424).
            decide: Some(torro_core::ai::SlotModel {
                engine: DEMO_OPENROUTER.into(),
                model: Some("qwen/qwen3-235b-a22b".into()),
                effort: None,
            }),
        },
        decide_mail_to_cloud: Some(true),
        ..Settings::default()
    };
    Data { accounts, settings, sources, ..Data::default() }
}

/// The label of an AI account as the core gives it (`account::ai::label` there): the provider's
/// name at its own address, else the provider with the host, a server of one's own by its host.
fn ai_label(account: &torro_core::Account) -> String {
    let host = account.id.split_once('@').map_or(account.server.as_str(), |(_, host)| host);
    match account.ai.unwrap_or(AiProvider::OpenAiCompatible) {
        AiProvider::OpenAiCompatible => host.to_string(),
        provider if provider.default_url() == Some(account.server.as_str()) => provider.name().to_string(),
        provider => format!("{} ({host})", provider.name()),
    }
}

/// The sample answer to a request of the account pages or the add-account dialog; `None` for any
/// other request, which the caller answers itself.
pub fn demo_answer(request: &Request, data: &dyn Seen) -> Option<Response> {
    match request {
        Request::ConnectLocalAi { program, server, key } => {
            let Some(program) = torro_core::ai::LocalProgram::from_id(program) else {
                return Some(Response::Error {
                    code: "unsupported".into(),
                    message: tr("Unknown local AI program.").into(),
                });
            };
            let request = Request::ConnectAi {
                provider: AiProvider::OpenAiCompatible,
                server: Some(server.as_deref().unwrap_or(program.default_url()).to_string()),
                key: key.clone(),
            };
            demo_answer(&request, data).map(|response| match response {
                Response::Account { mut account } => {
                    account.label = program.name().into();
                    Response::Account { account }
                }
                other => other,
            })
        }
        Request::DiscoverServices { address } => Some(demo_discovery(address)),
        // AI providers (task KI.1a): Ollama runs on this computer, LM Studio does not; the key
        // "wrong" is refused, every other connection works and stores nothing.
        Request::FindAiServers => Some(Response::AiServers {
            observations: Vec::new(),
            servers: vec![AiServer {
                provider: AiProvider::Ollama,
                program: None,
                url: "http://localhost:11434/v1".into(),
                models: 4,
                account: None,
            }],
        }),
        // The models of an AI account (task S.31), the same for every account but Ollama (task MW.8); a
        // switch is taken, nothing is stored.
        Request::AiModels { account, .. } if account == DEMO_NETWORK => {
            Some(Response::AiModels { account: account.clone(), models: demo_network_models() })
        }
        Request::AiModels { account, .. } if account == DEMO_OLLAMA => {
            Some(Response::AiModels { account: account.clone(), models: demo_ollama_models() })
        }
        Request::AiModels { account, .. } => {
            Some(Response::AiModels { account: account.clone(), models: demo_models() })
        }
        // The slots (task MW.8): a suggestion for this computer, a download that runs, a slot taken.
        Request::AiRecommend => Some(Response::AiRecommendation { recommendation: demo_recommendation(data) }),
        Request::PullAiModel { account, model } => {
            Some(Response::AiPulls { pulls: vec![demo_pull(account, model, torro_ipc::AiPullState::Running)] })
        }
        Request::AiPulls => Some(Response::AiPulls { pulls: demo_pulls(data, torro_ipc::AiPullState::Running) }),
        Request::CancelAiPull { .. } => {
            Some(Response::AiPulls { pulls: demo_pulls(data, torro_ipc::AiPullState::Cancelled) })
        }
        // Assignments too, so a chain of "Use suggestion" that takes an exception back does not hang.
        Request::SetAiSlot { .. } | Request::AiAssignments | Request::SetAiAssignment { .. } => {
            Some(Response::AiAssignments { assignments: Vec::new(), slots: Vec::new() })
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
                    label: ai_label(&account),
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
            Some(Response::Error { code: NtfyError::Auth.code().into(), message: NtfyError::Auth.message() })
        }
        Request::SetNtfyLogin { secret, .. } if secret.expose() == "wrong" => {
            Some(Response::Error { code: NtfyError::Auth.code().into(), message: NtfyError::Auth.message() })
        }
        Request::ConnectNtfy { topics, .. } | Request::SetNtfyLogin { topics, .. } => {
            let topics: Vec<(&str, &str)> =
                topics.iter().map(|topic| (topic.topic.as_str(), topic.label.as_str())).collect();
            Some(Response::Account { account: ntfy_account(&topics, Utc::now()) })
        }
        Request::SetNtfyTopic { .. } => Some(Response::Ok),
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
                data.accounts().iter().find(|account| account.kind == "caldav").cloned().map(|mut account| {
                    account.label = login_name.clone();
                    account
                })?;
            Some(Response::Connected { account, calendars: true, tasks: true })
        }
        _ => None,
    }
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
        AiModel {
            parameters: Some("235B".into()),
            ..model("qwen/qwen3-235b-a22b", "Qwen3 235B A22B", 131_072, true, true)
        },
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

/// The id of the demo's AI account in the cloud.
pub const DEMO_OPENROUTER: &str = "ai:openrouter@openrouter.ai";

/// The id of the demo's Ollama on this computer ([`demo_ollama`], task MW.8).
pub const DEMO_OLLAMA: &str = "ai:ollama@localhost:11434";

/// The same runtime on another device: remote for mail even though its model says remote=false.
pub const DEMO_NETWORK: &str = "ai:ollama@192.168.1.20:11434";

pub fn demo_network_account() -> AccountInfo {
    let mut account = demo_ollama_account();
    account.id = DEMO_NETWORK.into();
    account.label = "Ollama (192.168.1.20:11434)".into();
    account.state = AccountState::Connected {
        server: "http://192.168.1.20:11434/v1".into(),
        login_name: String::new(),
        last_sync: None,
        problem: None,
    };
    account
}

/// Saved switches are retained: nimble was explicitly enabled by the person, Qwen was not.
pub fn demo_network_models() -> Vec<AiModel> {
    demo_ollama_models()
        .into_iter()
        .map(|mut model| {
            model.enabled = model.decision == Some(true);
            model
        })
        .collect()
}

/// Ollama on this computer as an AI account (task MW.8, `--open ai-models`): the suggestions then run
/// there, one after a download. Not in [`demo_data`] itself: the add-account dialog's demo finds Ollama
/// unconnected.
pub fn demo_ollama(data: &mut Data) {
    data.accounts.push(demo_ollama_account());
}

/// The account of [`demo_ollama`], for a demo that keeps its accounts elsewhere (the control centre).
pub fn demo_ollama_account() -> AccountInfo {
    AccountInfo {
        id: DEMO_OLLAMA.into(),
        kind: "ai".into(),
        label: "Ollama".into(),
        state: AccountState::Connected {
            server: "http://localhost:11434/v1".into(),
            login_name: String::new(),
            last_sync: Some(Utc::now()),
            problem: None,
        },
        collections: Vec::new(),
        scopes: None,
        mail: None,
        ntfy: None,
        services: None,
    }
}

/// The demo's account `engine` runs on this computer: its Ollama.
pub fn demo_is_local(engine: &str) -> bool {
    engine == DEMO_OLLAMA
}

/// The models of the demo's Ollama (task MW.8): Qwen3 on, and the decision model nimble downloaded but
/// off, as Ollama lists it (`nimble:latest`).
pub fn demo_ollama_models() -> Vec<AiModel> {
    vec![
        AiModel {
            tools: Some(true),
            remote: Some(false),
            context: Some(40_960),
            parameters: Some("8.2B".into()),
            quantization: Some("Q4_K_M".into()),
            size: Some(5_225_388_164),
            reasoning: Some(true),
            enabled: true,
            ..AiModel::named("qwen3:8b")
        },
        AiModel {
            tools: Some(false),
            decision: Some(true),
            remote: Some(false),
            parameters: Some("3.8B".into()),
            quantization: Some("Q4_K_M".into()),
            size: Some(2_491_000_000),
            ..AiModel::named("nimble:latest")
        },
    ]
}

/// What the demo suggests (task MW.8) on a computer with a graphics card: with its Ollama Qwen3 14B for
/// *Chat* after a download and nimble for *Sorting*, ready; without it the chat in the cloud and
/// *Sorting* waiting for Ollama.
fn demo_recommendation(data: &dyn Seen) -> torro_ipc::AiRecommendation {
    use torro_core::ai::AiSlot;
    use torro_core::ai::hardware::{Gpu, Hardware};
    const GIB: u64 = 1024 * 1024 * 1024;
    let ollama = data.accounts().iter().any(|account| account.id == DEMO_OLLAMA);
    let local =
        |slot, engine: Option<&str>, model: &str, download: Option<u64>, reason: String| torro_ipc::SlotSuggestion {
            slot,
            engine: engine.map(str::to_string),
            provider: AiProvider::Ollama,
            model: Some(model.into()),
            local: true,
            seat: torro_core::ai::Seat::ThisComputer.label().into(),
            download_bytes: download,
            by_chat: false,
            reason,
        };
    let slots = match ollama {
        true => vec![
            local(AiSlot::Chat, Some(DEMO_OLLAMA), "qwen3:14b", Some(9_276_198_565), tr("Downloads first").into()),
            local(AiSlot::Decide, Some(DEMO_OLLAMA), "nimble:latest", None, tr("Ready").into()),
        ],
        false => vec![
            torro_ipc::SlotSuggestion {
                slot: AiSlot::Chat,
                engine: Some(DEMO_OPENROUTER.into()),
                provider: AiProvider::OpenRouter,
                model: Some("qwen/qwen3-235b-a22b".into()),
                local: false,
                seat: AiProvider::OpenRouter.seat().label().into(),
                download_bytes: None,
                by_chat: false,
                reason: tr("No AI server runs on this computer").into(),
            },
            local(
                AiSlot::Decide,
                None,
                "nimble",
                Some(9_527_504_198),
                trf("Needs {program}", &[("program", &AiProvider::Ollama.name())]),
            ),
        ],
    };
    torro_ipc::AiRecommendation {
        hardware: Hardware {
            cpu: Some("AMD Ryzen 9 7950X".into()),
            memory_bytes: Some(64 * GIB),
            gpu: Some(Gpu { name: Some("GeForce RTX 4070".into()), vram_bytes: Some(12 * GIB) }),
            unified_memory: false,
        },
        tier: "gpu".into(),
        tier_label: tr("Graphics card").into(),
        slots,
        preparations: Vec::new(),
    }
}

/// The demo's download of `model` into `account` in `state`: 41 % of Qwen3 14B.
fn demo_pull(account: &str, model: &str, state: torro_ipc::AiPullState) -> torro_ipc::AiPull {
    torro_ipc::AiPull {
        id: "p1".into(),
        account: account.into(),
        model: model.into(),
        state,
        completed: Some(3_803_241_411),
        total: Some(9_276_198_565),
        error: None,
    }
}

/// The downloads of the demo's run: with its Ollama the one of Qwen3 14B, in `state`; else none.
fn demo_pulls(data: &dyn Seen, state: torro_ipc::AiPullState) -> Vec<torro_ipc::AiPull> {
    match data.accounts().iter().any(|account| account.id == DEMO_OLLAMA) {
        true => vec![demo_pull(DEMO_OLLAMA, "qwen3:14b", state)],
        false => Vec::new(),
    }
}

/// TorroMail's id of the demo account whose keychain entry cannot be read.
pub const DEMO_LOCKED: &str = "tm-fastmail";

/// The accounts the demo's TorroMail has (task M7.4): one the suite can take (Posteo), one whose
/// password cannot be read ([`DEMO_LOCKED`]), Gmail with OAuth (not supported) and the mailbox.org
/// account the demo has connected already, so every kind of row shows.
pub fn demo_torromail(data: &dyn Seen) -> Vec<TorroMailAccount> {
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
        connected: data.accounts().iter().any(|known| known.id == format!("mail:{address}")),
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
            mail_unsupported = Some(tr(MAIL_UNSUPPORTED_GOOGLE).to_string());
        }
        "outlook.com" => {
            provider = "Outlook.com".into();
            mail_unsupported = Some(tr(MAIL_UNSUPPORTED_MICROSOFT).to_string());
        }
        _ => {}
    }
    Response::Discovery { provider, urls, unsupported, imap, smtp, login_hint, app_password, mail_unsupported }
}

/// The core's reason why Google's calendars do not work (`pim.rs`); Gmail answers with it next to
/// the one for mail.
const CALENDARS_UNSUPPORTED_GOOGLE: &str =
    "Google calendars need a sign-in through Google, which Torro Workplace does not support yet.";

#[cfg(test)]
mod tests {
    use torro_mail_imap::discover::MailUnsupported;

    use super::*;

    /// The search words Gmail and Outlook.com as the core does, so the translation of the core's
    /// sentence fits the demo's too.
    #[test]
    fn the_reasons_for_mail_are_the_cores() {
        assert_eq!(MAIL_UNSUPPORTED_GOOGLE, MailUnsupported::Google.reason());
        assert_eq!(MAIL_UNSUPPORTED_MICROSOFT, MailUnsupported::Microsoft.reason());
    }

    /// The samples have an account of every kind the pages know, and a sign-in that works.
    #[test]
    fn every_kind_has_a_sample_and_connecting_works() {
        let data = demo_data();
        for kind in ["nextcloud", "github", "caldav", "mail", "ntfy", "ai"] {
            assert!(data.accounts.iter().any(|account| account.kind == kind), "{kind}");
        }
        let ai = Request::ConnectAi {
            provider: AiProvider::Ollama,
            server: Some("http://localhost:11434/v1".into()),
            key: torro_core::account::Secret::default(),
        };
        assert!(matches!(demo_answer(&ai, &data), Some(Response::Account { .. })));
        assert!(demo_answer(&Request::Settings, &data).is_none(), "not an account's request");
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
    }
}
