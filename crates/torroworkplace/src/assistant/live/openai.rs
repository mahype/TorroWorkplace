//! The protocol of OpenAI Realtime and xAI's Grok Voice (issue #838): JSON events over one WebSocket
//! (`wss://<host>/v1/realtime?model=<model>`). Both speak it; they differ in a few places, checked against their
//! documentation on 2026-11-20:
//!
//! - OpenAI hears only 24 kHz: the microphone is resampled ([`super::resample`]); xAI hears its 16 kHz as it is.
//! - OpenAI names its session `"type": "realtime"` and finds the end of a sentence by meaning (`semantic_vad`); xAI
//!   has no session type and finds it by the pause (`server_vad`).
//! - OpenAI's transcription of the person comes whole (`…transcription.completed`); xAI's grows, each event the
//!   whole text so far (`…transcription.updated`), and completes the same way.
//! - Tools are called in `response.function_call_arguments.done` (xAI) or as items of `response.done` (OpenAI);
//!   both are read, each call once.
//!
//! The engine finds the end of a sentence and starts its answer itself; an answer the person speaks into it stops
//! by itself too (`interrupt_response`). Torro takes the sound read aloud out of the microphone first. Events the
//! session does not need are skipped. An error on a cancel or truncate (the answer was over already) is only
//! logged, by its code; other errors end the connection's use. Nothing logs text or sound.

use base64::Engine as _;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use torro_core::ai::{AiError, AiProvider, BoxFuture, Role, ToolCall, ToolSpec};

use super::{ANSWER_RATE, Incoming, LiveWire, MIC_RATE, Outgoing, resample};
use crate::account::ai::LiveSocket;

/// The voice OpenAI speaks in: one of its two recommended for Realtime.
const OPENAI_VOICE: &str = "marin";

/// The voice Grok speaks in: the one xAI's documentation starts with.
const XAI_VOICE: &str = "eve";

/// What one connection remembers between events.
#[derive(Debug, Default)]
pub struct State {
    /// xAI: what the person said so far (its transcription grows).
    heard: String,
    /// The answer's text so far.
    text: String,
    /// The tools the answer called so far.
    calls: Vec<ToolCall>,
    /// The item of the answer's sound, to cut it where the person spoke into it.
    item: Option<String>,
}

/// The dialect of one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    xai: bool,
}

impl Dialect {
    /// The dialect of `provider`'s engines: xAI's or OpenAI's.
    pub fn of(provider: AiProvider) -> Dialect {
        Dialect { xai: provider == AiProvider::Xai }
    }

    /// The rate the engine hears at.
    fn rate(self) -> u32 {
        if self.xai { MIC_RATE } else { ANSWER_RATE }
    }

    /// The events that send `out`, with what the connection remembers.
    pub fn events(self, out: &Outgoing, state: &State) -> Vec<Value> {
        match out {
            Outgoing::Configure { instructions, tools } => vec![self.configure(instructions, tools)],
            Outgoing::History(items) => items.iter().map(|(role, text)| history_item(*role, text)).collect(),
            Outgoing::Audio(samples) => {
                let samples = resample(samples, self.rate());
                let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                vec![json!({"type": "input_audio_buffer.append",
                    "audio": base64::engine::general_purpose::STANDARD.encode(bytes)})]
            }
            Outgoing::ToolResults(results) => results
                .iter()
                .map(|(call, output)| {
                    json!({"type": "conversation.item.create",
                        "item": {"type": "function_call_output", "call_id": call, "output": output}})
                })
                .chain(std::iter::once(json!({"type": "response.create"})))
                .collect(),
            // Only an engine that waits for the assistant's answer says one (GPT-Live, issue #919).
            Outgoing::Say(_) => Vec::new(),
            Outgoing::Cancel { played_ms } => {
                let mut events = vec![json!({"type": "response.cancel"})];
                if let Some(item) = &state.item {
                    events.push(json!({"type": "conversation.item.truncate",
                        "item_id": item, "content_index": 0, "audio_end_ms": played_ms}));
                }
                events
            }
        }
    }

    /// `session.update` with `instructions` and `tools`.
    fn configure(self, instructions: &str, tools: &[ToolSpec]) -> Value {
        let tools: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({"type": "function", "name": tool.name, "description": tool.description,
                    "parameters": tool.parameters})
            })
            .collect();
        let session = match self.xai {
            true => json!({
                "instructions": instructions,
                "voice": XAI_VOICE,
                "turn_detection": {"type": "server_vad"},
                "tools": tools,
                "audio": {
                    "input": {"format": {"type": "audio/pcm", "rate": MIC_RATE},
                        "transcription": {"model": "grok-transcribe"}},
                    "output": {"format": {"type": "audio/pcm", "rate": ANSWER_RATE}},
                },
            }),
            false => json!({
                "type": "realtime",
                "instructions": instructions,
                "output_modalities": ["audio"],
                "tools": tools,
                "tool_choice": "auto",
                "audio": {
                    "input": {"format": {"type": "audio/pcm", "rate": ANSWER_RATE},
                        "transcription": {"model": "gpt-4o-mini-transcribe"},
                        "turn_detection": {"type": "semantic_vad", "interrupt_response": true}},
                    "output": {"format": {"type": "audio/pcm", "rate": ANSWER_RATE}, "voice": OPENAI_VOICE},
                },
            }),
        };
        json!({"type": "session.update", "session": session})
    }

    /// What the event `event` means for the session; nothing for one it does not need.
    pub fn read(self, event: &Value, state: &mut State) -> Option<Incoming> {
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match event.get("type").and_then(Value::as_str)? {
            "input_audio_buffer.speech_started" => Some(Incoming::SpeechStarted),
            "input_audio_buffer.speech_stopped" => {
                Some(Incoming::SpeechStopped { audio_end_ms: event.get("audio_end_ms").and_then(Value::as_u64) })
            }
            "conversation.item.input_audio_transcription.updated" => {
                state.heard = text("transcript");
                None
            }
            "conversation.item.input_audio_transcription.completed" => {
                let heard = std::mem::take(&mut state.heard);
                let said = text("transcript");
                Some(Incoming::Heard(if said.trim().is_empty() { heard } else { said }))
            }
            "conversation.item.input_audio_transcription.failed" => {
                state.heard.clear();
                Some(Incoming::Heard(String::new()))
            }
            "response.created" => {
                (state.text, state.calls, state.item) = (String::new(), Vec::new(), None);
                Some(Incoming::AnswerStarted)
            }
            "response.output_audio.delta" | "response.audio.delta" => {
                if let Some(item) = event.get("item_id").and_then(Value::as_str) {
                    state.item = Some(item.to_string());
                }
                let bytes = base64::engine::general_purpose::STANDARD.decode(text("delta")).ok()?;
                let samples = bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
                Some(Incoming::AnswerAudio(samples))
            }
            "response.output_audio_transcript.delta"
            | "response.audio_transcript.delta"
            | "response.output_text.delta"
            | "response.text.delta" => {
                let piece = text("delta");
                state.text.push_str(&piece);
                (!piece.is_empty()).then_some(Incoming::AnswerText(piece))
            }
            "response.function_call_arguments.done" => {
                add_call(state, event);
                None
            }
            "response.done" => {
                let output = event.pointer("/response/output").and_then(Value::as_array);
                for item in output.into_iter().flatten() {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        add_call(state, item);
                    }
                }
                let (text, calls) = (std::mem::take(&mut state.text), std::mem::take(&mut state.calls));
                Some(Incoming::AnswerDone { text, calls })
            }
            "error" => {
                let error = event.get("error").unwrap_or(event);
                let field = |key: &str| error.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
                let code = Some(field("code")).filter(|code| !code.is_empty()).unwrap_or_else(|| field("type"));
                // The server chooses the code: only a short word of letters, digits and `_.-` goes on to the log.
                let code: String = code
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
                    .take(64)
                    .collect();
                // A cancel or truncate of an answer that was over already: nothing is wrong.
                if ["cancel", "truncat", "no_active_response"].iter().any(|part| code.contains(part)) {
                    eprintln!("torroworkplace: conversation engine: ignored error {code}");
                    return None;
                }
                Some(Incoming::Failed { code: if code.is_empty() { "error".into() } else { code } })
            }
            _ => None,
        }
    }
}

/// The call in `event` (its `call_id`, `name` and `arguments`), unless the answer has it already.
fn add_call(state: &mut State, event: &Value) {
    let field = |key: &str| event.get(key).and_then(Value::as_str).map(str::to_string);
    let (Some(id), Some(name)) = (field("call_id"), field("name")) else { return };
    if state.calls.iter().any(|call| call.id == id) {
        return;
    }
    state.calls.push(ToolCall { id, name, arguments: field("arguments").unwrap_or_else(|| "{}".into()) });
}

/// A message of the conversation for a new connection.
fn history_item(role: Role, text: &str) -> Value {
    let (role, kind) = match role {
        Role::Assistant => ("assistant", "output_text"),
        _ => ("user", "input_text"),
    };
    json!({"type": "conversation.item.create",
        "item": {"type": "message", "role": role, "content": [{"type": kind, "text": text}]}})
}

/// One live connection to OpenAI Realtime or Grok Voice.
pub struct OpenAiWire {
    socket: LiveSocket,
    dialect: Dialect,
    state: State,
    closed: bool,
}

impl OpenAiWire {
    /// The connection `socket` to an engine of `provider`.
    pub fn new(socket: LiveSocket, provider: AiProvider) -> OpenAiWire {
        OpenAiWire { socket, dialect: Dialect::of(provider), state: State::default(), closed: false }
    }
}

impl LiveWire for OpenAiWire {
    fn send(&mut self, out: Outgoing) -> BoxFuture<'_, Result<(), AiError>> {
        Box::pin(async move {
            for event in self.dialect.events(&out, &self.state) {
                self.socket.send(Message::text(event.to_string())).await.map_err(|_| AiError::Offline)?;
            }
            Ok(())
        })
    }

    fn next(&mut self) -> BoxFuture<'_, Incoming> {
        Box::pin(async move {
            if self.closed {
                return Incoming::Closed;
            }
            loop {
                let text = match self.socket.next().await {
                    Some(Ok(Message::Text(text))) => text,
                    // Pings are answered by the socket itself; binary frames are not used.
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_))) => continue,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => {
                        self.closed = true;
                        return Incoming::Closed;
                    }
                };
                let Ok(event) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                if let Some(incoming) = self.dialect.read(&event, &mut self.state) {
                    return incoming;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded with the shapes of the documentation (`tests/fixtures/live/`): each line one event.
    fn events(file: &str) -> Vec<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/live").join(file);
        std::fs::read_to_string(path)
            .expect("fixture")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect()
    }

    fn read_all(dialect: Dialect, file: &str) -> Vec<Incoming> {
        let mut state = State::default();
        events(file).iter().filter_map(|event| dialect.read(event, &mut state)).collect()
    }

    /// A round of OpenAI: speech, its transcript, the spoken answer with a call, and an ignored error.
    #[test]
    fn an_openai_round_reads_as_speech_text_sound_and_calls() {
        let read = read_all(Dialect::of(AiProvider::OpenAi), "openai_round.jsonl");
        let call = ToolCall { id: "call_1".into(), name: "calendar_list_events".into(), arguments: "{}".into() };
        assert_eq!(
            read,
            vec![
                Incoming::SpeechStarted,
                Incoming::SpeechStopped { audio_end_ms: Some(1840) },
                Incoming::AnswerStarted,
                Incoming::AnswerText("Let me ".into()),
                Incoming::AnswerAudio(vec![1, -2]),
                Incoming::AnswerText("look.".into()),
                Incoming::Heard("What's on today?".into()),
                Incoming::AnswerDone { text: "Let me look.".into(), calls: vec![call] },
            ]
        );
    }

    /// A round of Grok: its transcription grows and completes; a call comes before the answer is done, once.
    #[test]
    fn a_grok_round_takes_the_grown_transcript_and_each_call_once() {
        let read = read_all(Dialect::of(AiProvider::Xai), "xai_round.jsonl");
        let call = ToolCall { id: "c7".into(), name: "tasks_list_todos".into(), arguments: "{\"list\":\"a\"}".into() };
        assert_eq!(
            read,
            vec![
                Incoming::SpeechStarted,
                Incoming::SpeechStopped { audio_end_ms: None },
                Incoming::Heard("Was steht an?".into()),
                Incoming::AnswerStarted,
                Incoming::AnswerDone { text: String::new(), calls: vec![call] },
                Incoming::Failed { code: "invalid_api_key".into() },
            ]
        );
    }

    /// What goes out: OpenAI's session with its type and 24 kHz, xAI's without and at 16 kHz; a cancel cuts the item
    /// whose sound played.
    #[test]
    fn what_goes_out_follows_the_dialect() {
        let (openai, xai) = (Dialect::of(AiProvider::OpenAi), Dialect::of(AiProvider::Xai));
        let configure = Outgoing::Configure { instructions: "Be brief.".into(), tools: Vec::new() };
        let session = &openai.events(&configure, &State::default())[0]["session"];
        assert_eq!(
            (session["type"].as_str(), session["audio"]["input"]["format"]["rate"].as_u64()),
            (Some("realtime"), Some(24_000))
        );
        let session = &xai.events(&configure, &State::default())[0]["session"];
        assert_eq!((session.get("type"), session["audio"]["input"]["format"]["rate"].as_u64()), (None, Some(16_000)));
        let audio = Outgoing::Audio(vec![0; 160]);
        let bytes = |events: Vec<Value>| {
            base64::engine::general_purpose::STANDARD
                .decode(events[0]["audio"].as_str().expect("audio"))
                .expect("b64")
                .len()
        };
        assert_eq!(
            (bytes(openai.events(&audio, &State::default())), bytes(xai.events(&audio, &State::default()))),
            (480, 320)
        );
        let state = State { item: Some("item_9".into()), ..State::default() };
        let cancel = openai.events(&Outgoing::Cancel { played_ms: 1200 }, &state);
        assert_eq!(cancel[1]["audio_end_ms"], json!(1200));
        assert_eq!(cancel.len(), 2);
        let results = openai.events(&Outgoing::ToolResults(vec![("c1".into(), "{}".into())]), &state);
        assert_eq!(results.last().and_then(|event| event["type"].as_str()), Some("response.create"));
        let history = openai.events(&Outgoing::History(vec![(Role::Assistant, "Hi.".into())]), &state);
        assert_eq!(history[0]["item"]["content"][0]["type"], json!("output_text"));
    }

    /// Neither text nor sound shows in what is logged of an event.
    #[test]
    fn the_debug_form_holds_no_words() {
        let shown = format!(
            "{:?} {:?}",
            Incoming::Heard("secret words".into()),
            Outgoing::History(vec![(Role::User, "secret".into())])
        );
        assert!(!shown.contains("secret"), "{shown}");
    }
}
