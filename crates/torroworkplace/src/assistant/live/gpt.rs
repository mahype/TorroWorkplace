//! The protocol of GPT-Live (issue #919): JSON events over one WebSocket (`wss://api.openai.com/v1/live/sessions`, no
//! query), read from OpenAI's documentation on 2026-11-23 (`developers.openai.com/api/docs/guides/live`,
//! `…/live-delegation?delegation-mode=client`, `…/voice-websockets?api=live`, the model page of `gpt-live-1.1`). It is
//! not the protocol of Realtime: its events are `session.…`, and Realtime's endpoint does not offer the model.
//!
//! The session is started in the mode where **the application thinks** (`"delegation": {"type": "client"}`, fixed for
//! the session): GPT-Live is only the voice. It listens and speaks full-duplex and decides itself when the person has
//! finished; then it says so (`session.delegation.created`) and waits for the application, which answers with the
//! assistant of Torro (its chat model, skills and tools, rights, approvals and log: the answer of any message of the
//! conversation) and gives what it says back as `session.commentary.append`, which GPT-Live speaks. So this adapter
//! never calls a tool and has none: [`Outgoing::Configure`] gives only the instructions of the voice, and
//! [`Outgoing::Say`] is the assistant's answer, sentence by sentence.
//!
//! What the protocol gives, and what the adapter makes of it for the session ([`super`]):
//!
//! - There is no event for the start or end of the person's speech and none for "the transcript is complete":
//!   `session.input_transcript.delta` comes in pieces (with the time on the session's line, `start_ms`). The first
//!   piece after a told sentence is [`Incoming::SpeechStarted`], also while the answer sounds: speaking into it is
//!   barge-in. The end is `session.delegation.created` with the time its sentence ended (`offset_ms`); the words are
//!   told [`QUIET`] after the last piece, at most [`CAP`] after it, because the transcript may come after it. Pieces
//!   that began before that time belong to the told sentence and start nothing.
//! - The sound of the answer is `session.output_audio.delta` (24 kHz) and the words it speaks `session.output_transcript.delta`;
//!   there is no event for the end of either, nor for how much was played. GPT-Live stops its own speech when the
//!   person speaks into it; the assistant's answer stops with the session's interruption.
//! - The conversation so far is not sent: the assistant behind the voice has it, GPT-Live only says what it is told.
//!
//! Neither text nor sound is logged; only a short error code.

use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use torro_core::ai::{AiError, BoxFuture};

use super::{ANSWER_RATE, Incoming, LiveWire, Outgoing, resample};
use crate::account::ai::LiveSocket;

/// How long after the last piece of the transcript, once the person's sentence is over, what they said is told.
pub const QUIET: Duration = Duration::from_millis(300);

/// The longest the words of a sentence that is over are waited for: its transcript may come after
/// `session.delegation.created`.
pub const CAP: Duration = Duration::from_millis(1500);

/// How long the person's pieces stop, with no task handed over, before their sentence counts as over: GPT-Live took
/// care of it itself (a thanks, a noise).
pub const IDLE: Duration = Duration::from_secs(4);

/// A piece of the transcript without a time that comes this soon after a sentence was told belongs to it.
const LATE: Duration = Duration::from_millis(800);

/// How many pieces of sound are kept before `session.started` (the first words, after a new connection).
const EARLY_AUDIO: usize = 150;

/// The voices of GPT-Live in the language of the person: the default `marin` for English, a German one otherwise
/// (the documentation lists `nuri`, `juni` and `flitz` as German).
fn voice() -> &'static str {
    match torro_core::i18n::lang() {
        torro_core::i18n::Lang::De => "juni",
        torro_core::i18n::Lang::En => "marin",
    }
}

/// The first event: the model, how it speaks, and that the application thinks.
pub fn start(model: &str, instructions: &str) -> Value {
    json!({"type": "session.start", "session": {
        "model": model,
        "instructions": instructions,
        "audio": {"format": {"type": "audio/pcm", "rate": ANSWER_RATE}, "output": {"voice": voice()}},
        "delegation": {"type": "client"},
    }})
}

/// What one connection remembers between events.
#[derive(Debug, Default)]
pub struct State {
    // Sending.
    started: bool,
    /// `session.started` came: more than `session.start` may be sent.
    ready: bool,
    /// What was sent before it and goes after it, then the last pieces of sound.
    queued: Vec<Value>,
    early_audio: VecDeque<Value>,
    // Reading.
    /// The task GPT-Live waits for the answer to.
    delegation: Option<String>,
    /// The time on the session's line where the last told sentence ended.
    cut_ms: Option<u64>,
    /// What the person said so far.
    heard: String,
    /// The person speaks (from their first piece until their words are told).
    speaking: bool,
    /// When the words are told: [`QUIET`] after the last piece, at most [`CAP`] after the task came.
    due: Option<Instant>,
    cap: Option<Instant>,
    /// Until when a piece without a time is taken for the told sentence's.
    late_until: Option<Instant>,
    /// When the sentence counts as over without a task, [`IDLE`] after the last piece.
    idle_due: Option<Instant>,
}

impl State {
    /// The events that send `out`, once the session started: what was queued comes first.
    pub fn events(&mut self, out: &Outgoing) -> Vec<Value> {
        let id = self.delegation.clone();
        let new = match out {
            Outgoing::Configure { .. } | Outgoing::History(_) | Outgoing::ToolResults(_) | Outgoing::Cancel { .. } => {
                Vec::new()
            }
            Outgoing::Audio(samples) => {
                let samples = resample(samples, ANSWER_RATE);
                let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                let event = json!({"type": "session.input_audio.append",
                    "audio": base64::engine::general_purpose::STANDARD.encode(bytes)});
                if !self.ready {
                    if self.early_audio.len() >= EARLY_AUDIO {
                        self.early_audio.pop_front();
                    }
                    self.early_audio.push_back(event);
                    return Vec::new();
                }
                vec![event]
            }
            // The assistant's answer, a sentence at a time, spoken by GPT-Live; nothing to say lets the task go silently.
            Outgoing::Say(text) if text.trim().is_empty() => {
                vec![json!({"type": "session.thinking.append", "delegation_id": id, "content": ""})]
            }
            Outgoing::Say(text) => {
                vec![json!({"type": "session.commentary.append", "delegation_id": id, "content": text})]
            }
        };
        if !self.ready {
            self.queued.extend(new);
            return Vec::new();
        }
        let mut events = std::mem::take(&mut self.queued);
        events.extend(std::mem::take(&mut self.early_audio));
        events.extend(new);
        events
    }

    /// What the event `event` means for the session; `now` is when it came.
    pub fn read(&mut self, event: &Value, now: Instant) -> Vec<Incoming> {
        let mut out = Vec::new();
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match event.get("type").and_then(Value::as_str).unwrap_or_default() {
            "session.started" => self.ready = true,
            "session.input_transcript.delta" => self.on_piece(event, &text("delta"), &mut out, now),
            "session.delegation.created" => {
                let task = event.get("delegation");
                if task.and_then(|task| task.get("target")).and_then(Value::as_str) == Some("client") {
                    self.delegation = task.and_then(|task| task.get("id")).and_then(Value::as_str).map(str::to_string);
                    self.cut_ms = task.and_then(|task| task.get("offset_ms")).and_then(Value::as_u64);
                    self.due = Some(now + QUIET);
                    self.cap = Some(now + CAP);
                    self.idle_due = None;
                }
            }
            "session.output_audio.delta" => {
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(text("delta")) else { return out };
                let samples = bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
                out.push(Incoming::AnswerAudio(samples));
            }
            "session.output_transcript.delta" => {
                let piece = text("delta");
                if !piece.is_empty() {
                    out.push(Incoming::AnswerText(piece));
                }
            }
            "session.closed" => out.push(Incoming::Closed),
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
                out.push(Incoming::Failed { code: if code.is_empty() { "error".into() } else { code } });
            }
            _ => {}
        }
        out
    }

    /// A piece of what the person said.
    fn on_piece(&mut self, event: &Value, piece: &str, out: &mut Vec<Incoming>, now: Instant) {
        let start = event.get("start_ms").and_then(Value::as_u64);
        if self.due.is_some() {
            // The sentence is over and its words not told yet: this piece is its own, however late.
            self.heard.push_str(piece);
            self.due = Some((now + QUIET).min(self.cap.unwrap_or(now + QUIET)));
            return;
        }
        // A piece that began before the told sentence ended is that sentence's; without a time, one that comes soon after.
        let stale = match (start, self.cut_ms) {
            (Some(start), Some(cut)) => start < cut,
            _ => self.late_until.is_some_and(|until| now < until),
        };
        if stale || piece.is_empty() {
            return;
        }
        if !self.speaking {
            self.speaking = true;
            out.push(Incoming::SpeechStarted);
        }
        self.heard.push_str(piece);
        self.idle_due = Some(now + IDLE);
    }

    /// What the person said, told: the end of their speech, then the words; or, when no task came for [`IDLE`], that
    /// their sentence is over and nobody waits for it.
    pub fn flush(&mut self, now: Instant) -> Vec<Incoming> {
        if self.due.is_none() {
            if self.idle_due.take().is_some() && std::mem::take(&mut self.speaking) {
                self.heard.clear();
                self.late_until = Some(now + LATE);
                return vec![Incoming::SpeechEnded];
            }
            return Vec::new();
        }
        self.due = None;
        self.cap = None;
        self.late_until = Some(now + LATE);
        let mut out = Vec::new();
        if std::mem::take(&mut self.speaking) {
            out.push(Incoming::SpeechStopped { audio_end_ms: None });
        }
        out.push(Incoming::Heard(std::mem::take(&mut self.heard).trim().to_string()));
        out
    }

    /// When [`State::flush`] is due, if it is.
    pub fn deadline(&self) -> Option<Instant> {
        match (self.due, self.idle_due) {
            (Some(due), _) => Some(due),
            (None, idle) => idle,
        }
    }
}

/// One live connection to GPT-Live.
pub struct GptLiveWire {
    socket: LiveSocket,
    model: String,
    state: State,
    /// What was read and not yet given out: one event may mean several things.
    incoming: VecDeque<Incoming>,
    closed: bool,
}

impl GptLiveWire {
    /// The connection `socket` to the engine `model`.
    pub fn new(socket: LiveSocket, model: &str) -> GptLiveWire {
        GptLiveWire {
            socket,
            model: model.to_string(),
            state: State::default(),
            incoming: VecDeque::new(),
            closed: false,
        }
    }

    async fn write(&mut self, event: Value) -> Result<(), AiError> {
        self.socket.send(Message::text(event.to_string())).await.map_err(|_| AiError::Offline)
    }
}

impl LiveWire for GptLiveWire {
    fn send(&mut self, out: Outgoing) -> BoxFuture<'_, Result<(), AiError>> {
        Box::pin(async move {
            if let Outgoing::Configure { instructions, .. } = &out {
                // The session's mode and instructions are final: only the first is used.
                if self.state.started {
                    eprintln!("torroworkplace: conversation engine: the session of this connection stays as it was");
                    return Ok(());
                }
                self.state.started = true;
                return self.write(start(&self.model, instructions)).await;
            }
            for event in self.state.events(&out) {
                self.write(event).await?;
            }
            Ok(())
        })
    }

    fn next(&mut self) -> BoxFuture<'_, Incoming> {
        Box::pin(async move {
            loop {
                if let Some(incoming) = self.incoming.pop_front() {
                    return incoming;
                }
                if self.closed {
                    return Incoming::Closed;
                }
                let frame = match self.state.deadline() {
                    Some(at) => match tokio::time::timeout_at(at, self.socket.next()).await {
                        Ok(frame) => frame,
                        Err(_) => {
                            self.incoming.extend(self.state.flush(Instant::now()));
                            continue;
                        }
                    },
                    None => self.socket.next().await,
                };
                let bytes = match frame {
                    Some(Ok(Message::Text(text))) => text.as_str().as_bytes().to_vec(),
                    Some(Ok(Message::Binary(bytes))) => bytes.to_vec(),
                    // Pings are answered by the socket itself.
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => {
                        self.closed = true;
                        continue;
                    }
                };
                let Ok(event) = serde_json::from_slice::<Value>(&bytes) else { continue };
                let read = self.state.read(&event, Instant::now());
                if read.contains(&Incoming::Closed) {
                    self.closed = true;
                }
                self.incoming.extend(read);
            }
        })
    }

    fn delegates(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *START.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    /// Events of the shapes of the documentation (`tests/fixtures/live/gpt_live_round.jsonl`, not recorded from the
    /// service: nobody had GPT-Live access for it): each line `[milliseconds, event]`.
    fn round() -> Vec<(u64, Value)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/live/gpt_live_round.jsonl");
        std::fs::read_to_string(path)
            .expect("fixture")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect()
    }

    /// A sentence by transcript pieces, the task GPT-Live hands over, the transcript's last piece after it, the answer's
    /// words and sound, and the next sentence: the first piece of it speaks into the answer.
    #[test]
    fn a_round_reads_as_speech_a_task_words_and_sound() {
        let mut state = State::default();
        let mut read = Vec::new();
        for (ms, event) in round() {
            let now = at(ms);
            while state.deadline().is_some_and(|due| due <= now) {
                read.extend(state.flush(now));
            }
            read.extend(state.read(&event, now));
        }
        assert_eq!(
            read,
            vec![
                Incoming::SpeechStarted,
                Incoming::SpeechStopped { audio_end_ms: None },
                Incoming::Heard("What's on today?".into()),
                Incoming::AnswerAudio(vec![1, -2]),
                Incoming::AnswerText("Nothing today.".into()),
                // The late piece of the told sentence started nothing; this one began after it ended.
                Incoming::SpeechStarted,
            ]
        );
        assert_eq!(state.delegation.as_deref(), Some("dlg_1"));
    }

    /// The words of a sentence are told a moment after the last piece, and at the latest [`CAP`] after the task.
    #[test]
    fn the_words_wait_for_the_transcript_but_not_for_long() {
        let mut state = State::default();
        let task = json!({"type": "session.delegation.created",
            "delegation": {"id": "d", "target": "client", "offset_ms": 500}});
        let piece = |text: &str| json!({"type": "session.input_transcript.delta", "delta": text, "start_ms": 100});
        state.read(&piece("Hello"), at(0));
        state.read(&task, at(100));
        assert_eq!(state.deadline(), Some(at(100) + QUIET));
        state.read(&piece(" there"), at(350));
        assert_eq!(state.deadline(), Some(at(350) + QUIET));
        // A transcript that keeps coming is cut at the cap.
        state.read(&piece("."), at(1500));
        assert_eq!(state.deadline(), Some(at(100) + CAP));
        let told = state.flush(at(1600));
        assert_eq!(told, vec![Incoming::SpeechStopped { audio_end_ms: None }, Incoming::Heard("Hello there.".into())]);
        assert_eq!(state.deadline(), None);
        assert!(state.flush(at(1700)).is_empty(), "told once");
    }

    /// A sentence GPT-Live took care of itself (no task): after a while it is over, nothing is told, and what was heard
    /// does not stay for the next sentence.
    #[test]
    fn a_sentence_without_a_task_ends_without_words() {
        let mut state = State::default();
        let piece = |text: &str, start: u64| json!({"type": "session.input_transcript.delta", "delta": text, "start_ms": start});
        assert_eq!(state.read(&piece("Thanks", 100), at(0)), vec![Incoming::SpeechStarted]);
        assert_eq!(state.deadline(), Some(at(0) + IDLE));
        assert_eq!(state.flush(at(4000)), vec![Incoming::SpeechEnded]);
        assert_eq!(state.deadline(), None);
        // The next sentence starts clean.
        state.read(&piece("What time is it?", 9000), at(9000));
        state.read(
            &json!({"type": "session.delegation.created", "delegation": {"id": "d", "target": "client", "offset_ms": 9500}}),
            at(9600),
        );
        assert_eq!(
            state.flush(at(9900)),
            vec![Incoming::SpeechStopped { audio_end_ms: None }, Incoming::Heard("What time is it?".into())]
        );
    }

    /// A task that is none of the application's (`target` is not `client`) is not handed over.
    #[test]
    fn only_a_client_task_waits_for_the_assistant() {
        let mut state = State::default();
        let task = json!({"type": "session.delegation.created", "delegation": {"id": "d", "target": "responses"}});
        state.read(&task, at(0));
        assert_eq!(state.deadline(), None);
        assert_eq!(state.delegation, None);
    }

    /// What goes out: `session.start` with the application as the thinker, sound at 24 kHz after the session started
    /// (what came before waits), the assistant's sentences as commentary for the task, and nothing silently releasing it.
    #[test]
    fn what_goes_out_starts_the_session_and_speaks_the_assistants_words() {
        let started = start("gpt-live-1.1", "Say what the assistant says.");
        assert_eq!(started["type"], json!("session.start"));
        assert_eq!(started["session"]["delegation"], json!({"type": "client"}));
        assert_eq!(started["session"]["audio"]["format"]["rate"], json!(24_000));
        assert_eq!(started["session"]["model"], json!("gpt-live-1.1"));

        let mut state = State::default();
        assert!(state.events(&Outgoing::Audio(vec![0; 160])).is_empty(), "sound waits for the session");
        assert!(state.events(&Outgoing::Say("Hello.".into())).is_empty(), "so does the answer");
        state.read(&json!({"type": "session.started", "session": {"id": "s"}}), at(0));
        state.read(
            &json!({"type": "session.delegation.created", "delegation": {"id": "dlg_7", "target": "client"}}),
            at(1),
        );
        let events = state.events(&Outgoing::Say("It is sunny.".into()));
        // The answer that waited (for no task yet) goes first, with no task; then the sound; then the new.
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["type"], json!("session.commentary.append"));
        assert_eq!(events[1]["type"], json!("session.input_audio.append"));
        assert_eq!(events[2]["delegation_id"], json!("dlg_7"));
        assert_eq!(events[2]["content"], json!("It is sunny."));
        // 160 samples at 16 kHz are 240 at 24 kHz: 480 bytes.
        let audio = base64::engine::general_purpose::STANDARD
            .decode(events[1]["audio"].as_str().expect("audio"))
            .expect("base64");
        assert_eq!(audio.len(), 480);
        let silent = state.events(&Outgoing::Say(String::new()));
        assert_eq!(silent[0]["type"], json!("session.thinking.append"));
        assert_eq!(silent[0]["content"], json!(""));
        // Tools and history have no place here.
        assert!(state.events(&Outgoing::ToolResults(vec![("c".into(), "{}".into())])).is_empty());
        assert!(state.events(&Outgoing::Cancel { played_ms: 5 }).is_empty());
    }

    /// An error shows only its short code; a closed session ends the connection.
    #[test]
    fn errors_show_a_code_and_a_closed_session_closes() {
        let mut state = State::default();
        let error = json!({"type": "error", "error": {"type": "invalid_request_error",
            "code": "immutable_field_update", "message": "secret words", "param": "session.delegation.type"}});
        assert_eq!(state.read(&error, at(0)), vec![Incoming::Failed { code: "immutable_field_update".into() }]);
        let closed = json!({"type": "session.closed", "reason": "expired"});
        assert_eq!(state.read(&closed, at(1)), vec![Incoming::Closed]);
    }
}
