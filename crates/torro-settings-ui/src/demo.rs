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
use torro_core::settings::{AssistantDefault, Settings};
use torro_core::theme::Appearance;
use torro_ipc::api::{ASSISTANT_CLIENT, Rights};
use torro_ipc::assistant::{Engine, EngineKind, LETTER_AVATAR, Note, Profile};
use torro_ipc::bots::BotSummary;
use torro_ipc::{ClientInfo, Request, Response, VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus};

use crate::{Answer, Backend};

/// Sample data for the sections, answered from memory.
#[derive(Debug, Default)]
pub struct Demo {
    settings: Settings,
    sent: Mutex<Vec<Request>>,
}

impl Demo {
    /// A demo with the sample settings: dark, so the first look shows the suite's own look.
    pub fn new() -> Demo {
        let settings = Settings {
            appearance: Appearance::Dark,
            // An AI account with one of its models: the load's second round then really asks
            // `ai.list_models`, and the Model and Effort groups of General have rows to show.
            assistant_default: Some(AssistantDefault {
                engine: "ai:ollama@localhost:11434".into(),
                model: Some("qwen3:8b".into()),
                effort: None,
            }),
            ..Settings::default()
        };
        Demo { settings, sent: Mutex::new(Vec::new()) }
    }

    /// The sample settings it answers `settings.settings` with.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The assistant's client, so the sections show the assistant as on. Switching it is the
    /// control centre's; the demo only pretends it was.
    fn clients(&self) -> Vec<ClientInfo> {
        let created = Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).single().unwrap_or_else(Utc::now);
        vec![ClientInfo {
            id: "a51c3e88".into(),
            name: "Assistant".into(),
            kind: ASSISTANT_CLIENT.into(),
            created,
            last_used: None,
            rights: Rights::assistant(),
            agent: None,
        }]
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
            engine: "claude-code".into(),
            model: None,
            client: "c0000001".into(),
        }]
    }

    /// Two sample engines: a coding agent with its own models and an AI account.
    fn engines(&self) -> Vec<Engine> {
        let model = |id: &str, name: &str, reasoning| AiModel {
            id: id.into(),
            name: name.into(),
            tools: Some(true),
            open_weights: None,
            context: None,
            reasoning,
            remote: None,
        };
        vec![
            Engine {
                id: "claude-code".into(),
                kind: EngineKind::Agent,
                name: "Claude Code".into(),
                problem: None,
                models: vec![
                    model("opus", "Opus", Some(true)),
                    model("sonnet", "Sonnet", Some(true)),
                    model("haiku", "Haiku", Some(false)),
                ],
            },
            Engine {
                id: "ai:ollama@localhost:11434".into(),
                kind: EngineKind::Ai,
                name: "Ollama".into(),
                problem: None,
                models: Vec::new(),
            },
        ]
    }

    /// The models the sample AI account offers (`ai.list_models`).
    fn models(&self) -> Vec<AiModel> {
        vec![AiModel {
            id: "qwen3:8b".into(),
            name: "qwen3:8b".into(),
            tools: Some(true),
            open_weights: Some(true),
            context: None,
            reasoning: Some(true),
            remote: Some(false),
        }]
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
            Request::ListEngines => Response::Engines { engines: self.engines() },
            Request::AiModels { .. } => Response::AiModels { models: self.models() },
            Request::VoiceStatus => Response::Voice { voice: self.voice() },
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
