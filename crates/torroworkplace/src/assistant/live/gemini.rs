//! The protocol of Gemini Live (issue #918): JSON messages over one WebSocket
//! (`wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent`),
//! read from Google's documentation on 2026-11-23 (`aistudio.google.com/api-reference/live`, the guides on
//! capabilities, tools and session management). It is not the protocol of [`super::openai`]:
//!
//! - The first message is `setup` and the setup of a connection is final; the client waits for `setupComplete`
//!   before it sends more. So [`GeminiWire`] keeps [`Outgoing::Configure`] until the first message after it: if that
//!   is the conversation's history, `setup` asks for it (`historyConfig.initialHistoryInClientContent`) and the
//!   history goes as `clientContent` after `setupComplete`. Instructions and tools that change later in the same
//!   connection stay as the setup had them; every new connection (the session opens one every few minutes, see
//!   [`torro_core::ai::LiveModel::minutes`]) is set up with the current ones. A tool the model calls that is no more
//!   allowed fails in the core like any other call.
//! - There is no event for the start or end of the person's speech that the guides explain. `voiceActivity` is in
//!   the reference without a guide: it is used when it comes, and the same is found from the transcript of the
//!   person, which grows while they speak and ends with the first thing the answer says ([`State::flush`]).
//! - The transcript of the person comes in pieces (`inputTranscription`) beside the answer's, not before it:
//!   [`Incoming::Heard`] is told when the answer has been going for [`GRACE`], when the person has been quiet for
//!   [`QUIET`], when the model calls a tool or when the turn is complete, whichever comes first.
//! - `interrupted` means the person spoke into the answer: the server dropped what it had not sent and sends nothing
//!   more of that answer, so there is no end to tell; the session takes it as the start of speech.
//! - The model's tool calls come whole in one `toolCall` and its turn waits for the `toolResponse` (functions are
//!   declared `BLOCKING`). The end of an answer is `turnComplete`.
//!
//! Frames from the server hold JSON as text or as binary. Neither text nor sound is logged; an error closes the
//! connection and only the close code is logged.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use torro_core::ai::{AiError, BoxFuture, Role, ToolCall, ToolSpec};

use super::{Incoming, LiveWire, MIC_RATE, Outgoing};
use crate::account::ai::LiveSocket;

/// The voice Gemini speaks in: the one Google's documentation starts with.
const VOICE: &str = "Kore";

/// How long after the first thing the answer says what the person said is told, when it is not told sooner: their
/// transcript may still come.
pub const GRACE: Duration = Duration::from_millis(400);

/// How long the person is quiet (no piece of the transcript, no end of speech) before what they said is told.
pub const QUIET: Duration = Duration::from_millis(1500);

/// After a turn is complete, pieces of a transcript that come late are dropped, and none of them starts a sentence of
/// its own, for this long.
const LATE: Duration = Duration::from_millis(1500);

/// How long the session waits for what the person said after their speech ended: the transcript comes with the
/// answer, which a model with tools may take longer to begin.
pub const HEARD_WAIT: Duration = Duration::from_secs(5);

/// What one connection remembers between messages.
#[derive(Debug, Default)]
pub struct State {
    // Sending.
    /// The instructions and tools [`Outgoing::Configure`] gave, until `setup` is sent with them.
    config: Option<(String, Vec<ToolSpec>)>,
    setup_sent: bool,
    /// `setupComplete` came: more than `setup` may be sent.
    ready: bool,
    /// What was sent before `setupComplete` and goes after it.
    queued: Vec<Value>,
    /// The names of the tools the model called, by the id of the call: the result needs its tool's name.
    names: BTreeMap<String, String>,
    // Reading: one exchange is a sentence of the person and the answer to it.
    /// What the person said so far.
    heard: String,
    /// The person speaks (from their first sign until their words are told).
    speaking: bool,
    /// [`Incoming::SpeechStopped`] was told for this sentence.
    stopped: bool,
    /// [`Incoming::Heard`] was told for this sentence.
    told: bool,
    /// The server sends `voiceActivity`: the transcript does not decide when the person speaks.
    activity: bool,
    /// An answer is under way ([`Incoming::AnswerStarted`] told, no [`Incoming::AnswerDone`] yet).
    answering: bool,
    /// The turn of the session waits for an [`Incoming::AnswerDone`] without calls.
    owed: bool,
    /// The answer's text so far.
    text: String,
    /// When the person's speech last showed (a piece of the transcript, the end of speech).
    heard_at: Option<Instant>,
    /// When the answer began, while what the person said is not told.
    answer_at: Option<Instant>,
    /// Until when a piece of a transcript that comes late is dropped.
    mute_until: Option<Instant>,
}

impl State {
    /// The events that send `out`, once the setup is done: what was queued before comes first. Before `setupComplete`
    /// sound is dropped (it is only the first moments) and the rest waits.
    pub fn events(&mut self, out: &Outgoing) -> Vec<Value> {
        let new = match out {
            Outgoing::Configure { .. } | Outgoing::Cancel { .. } => Vec::new(),
            Outgoing::History(items) => history(items).into_iter().collect(),
            Outgoing::Audio(samples) => {
                if !self.ready {
                    return Vec::new();
                }
                let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                vec![json!({"realtimeInput": {"audio": {
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "mimeType": format!("audio/pcm;rate={MIC_RATE}"),
                }}})]
            }
            Outgoing::ToolResults(results) => {
                let responses: Vec<Value> = results
                    .iter()
                    .map(|(id, output)| {
                        json!({"id": id, "name": self.names.get(id).cloned().unwrap_or_default(),
                            "response": response(output)})
                    })
                    .collect();
                vec![json!({"toolResponse": {"functionResponses": responses}})]
            }
        };
        if !self.ready {
            self.queued.extend(new);
            return Vec::new();
        }
        let mut events = std::mem::take(&mut self.queued);
        events.extend(new);
        events
    }

    /// What the message `message` means for the session; `now` is when it came.
    pub fn read(&mut self, message: &Value, now: Instant) -> Vec<Incoming> {
        let mut out = Vec::new();
        if message.get("setupComplete").is_some() {
            self.ready = true;
        }
        if message.get("goAway").is_some() {
            out.push(Incoming::GoAway);
        }
        if let Some(activity) = message.get("voiceActivity") {
            self.activity = true;
            match activity.get("type").and_then(Value::as_str) {
                Some("ACTIVITY_START") => self.begin_speech(&mut out, now),
                Some("ACTIVITY_END") if self.speaking && !self.stopped => {
                    (self.stopped, self.heard_at) = (true, Some(now));
                    out.push(Incoming::SpeechStopped { audio_end_ms: activity.get("audioOffset").and_then(offset_ms) });
                }
                _ => {}
            }
        }
        if let Some(calls) = message.pointer("/toolCall/functionCalls").and_then(Value::as_array) {
            self.on_calls(calls, &mut out, now);
        }
        if let Some(content) = message.get("serverContent") {
            self.on_content(content, &mut out, now);
        }
        out
    }

    /// The person began to speak (or went on after a short pause).
    fn begin_speech(&mut self, out: &mut Vec<Incoming>, now: Instant) {
        match (self.speaking, self.stopped) {
            (true, false) => return,
            (true, true) => self.stopped = false,
            (false, _) => {
                // A new sentence: nothing of the last one stays.
                (self.heard, self.told, self.stopped, self.speaking) = (String::new(), false, false, true);
                self.answer_at = None;
            }
        }
        self.heard_at = Some(now);
        out.push(Incoming::SpeechStarted);
    }

    /// What the person said, told: the end of their speech when it was not, then the words. Nothing when they said
    /// nothing or it was told already.
    pub fn flush(&mut self) -> Vec<Incoming> {
        (self.heard_at, self.answer_at) = (None, None);
        if self.told || (!self.speaking && self.heard.trim().is_empty()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.speaking && !self.stopped {
            out.push(Incoming::SpeechStopped { audio_end_ms: None });
        }
        out.push(Incoming::Heard(std::mem::take(&mut self.heard).trim().to_string()));
        (self.told, self.speaking, self.stopped, self.owed) = (true, false, true, true);
        out
    }

    /// When [`State::flush`] is due, if it is.
    pub fn deadline(&self) -> Option<Instant> {
        if self.told {
            return None;
        }
        let quiet =
            if self.speaking && (self.stopped || !self.activity) { self.heard_at.map(|at| at + QUIET) } else { None };
        let answer = self.answer_at.map(|at| at + GRACE);
        match (quiet, answer) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The answer begins with its first sound or word.
    fn begin_answer(&mut self, out: &mut Vec<Incoming>, now: Instant) {
        if self.answering {
            return;
        }
        (self.answering, self.text) = (true, String::new());
        if !self.told {
            self.answer_at.get_or_insert(now);
        }
        out.push(Incoming::AnswerStarted);
    }

    /// The model calls tools: it has what it needs of the person's words, and waits for the results.
    fn on_calls(&mut self, calls: &[Value], out: &mut Vec<Incoming>, now: Instant) {
        out.extend(self.flush());
        self.begin_answer(out, now);
        let calls: Vec<ToolCall> = calls
            .iter()
            .filter_map(|call| {
                let field = |key: &str| call.get(key).and_then(Value::as_str).map(str::to_string);
                let (id, name) = (field("id")?, field("name")?);
                let arguments = call.get("args").map_or_else(|| "{}".to_string(), Value::to_string);
                self.names.insert(id.clone(), name.clone());
                Some(ToolCall { id, name, arguments })
            })
            .collect();
        // The answer goes on after the results as an answer of its own.
        self.answering = false;
        self.owed = true;
        out.push(Incoming::AnswerDone { text: std::mem::take(&mut self.text), calls });
    }

    /// `serverContent`: sound and words of the answer, the transcript of the person, the end of the turn.
    fn on_content(&mut self, content: &Value, out: &mut Vec<Incoming>, now: Instant) {
        if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
            // The server dropped the rest of the answer; this sentence is the person's.
            (self.answering, self.owed, self.speaking) = (false, false, false);
            self.text.clear();
            self.begin_speech(out, now);
        }
        if let Some(piece) = content.pointer("/inputTranscription/text").and_then(Value::as_str) {
            self.on_heard_piece(piece, out, now);
        }
        let parts = content.pointer("/modelTurn/parts").and_then(Value::as_array);
        for part in parts.into_iter().flatten() {
            let Some(data) = part.pointer("/inlineData/data").and_then(Value::as_str) else { continue };
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { continue };
            let samples = bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect();
            self.begin_answer(out, now);
            out.push(Incoming::AnswerAudio(samples));
        }
        if let Some(piece) = content.pointer("/outputTranscription/text").and_then(Value::as_str)
            && !piece.is_empty()
        {
            self.begin_answer(out, now);
            self.text.push_str(piece);
            out.push(Incoming::AnswerText(piece.to_string()));
        }
        if content.get("turnComplete").and_then(Value::as_bool) == Some(true) {
            out.extend(self.flush());
            if self.owed || self.answering {
                out.push(Incoming::AnswerDone { text: std::mem::take(&mut self.text), calls: Vec::new() });
            }
            (self.answering, self.owed, self.told, self.speaking, self.stopped) = (false, false, false, false, false);
            (self.heard_at, self.answer_at, self.mute_until) = (None, None, Some(now + LATE));
            self.heard.clear();
            self.text.clear();
        }
    }

    /// A piece of what the person said.
    fn on_heard_piece(&mut self, piece: &str, out: &mut Vec<Incoming>, now: Instant) {
        // Late pieces of a sentence already told, or of one whose turn is complete, are none of the next sentence.
        if self.told || self.mute_until.is_some_and(|until| now < until) {
            return;
        }
        if !self.speaking && !self.answering && self.heard.is_empty() {
            // The server tells when speech begins (`voiceActivity`); only without it the transcript does.
            if self.activity {
                return;
            }
            self.begin_speech(out, now);
        }
        self.heard.push_str(piece);
        self.heard_at = Some(now);
    }
}

/// The history as `clientContent`: one turn per run of messages of the same side (the model wants them to
/// alternate), the last turn complete so that the conversation goes on with the person's words.
fn history(items: &[(Role, String)]) -> Option<Value> {
    let mut turns: Vec<(&str, Vec<Value>)> = Vec::new();
    for (role, text) in items {
        let side = match role {
            Role::User => "user",
            Role::Assistant => "model",
            _ => continue,
        };
        let part = json!({"text": text});
        match turns.last_mut() {
            Some((last, parts)) if *last == side => parts.push(part),
            _ => turns.push((side, vec![part])),
        }
    }
    if turns.is_empty() {
        return None;
    }
    let turns: Vec<Value> = turns.into_iter().map(|(role, parts)| json!({"role": role, "parts": parts})).collect();
    Some(json!({"clientContent": {"turns": turns, "turnComplete": true}}))
}

/// The result of a tool as the object the model wants: an object as it is, anything else under `result`.
fn response(output: &str) -> Value {
    match serde_json::from_str::<Value>(output) {
        Ok(value @ Value::Object(_)) => value,
        Ok(value) => json!({"result": value}),
        Err(_) => json!({"result": output}),
    }
}

/// The `setup` message: the model with its voice and answers as sound, the instructions, the tools, the transcript of
/// both sides, and the compression of the context (so that sound alone is not cut off after 15 minutes). With
/// `history` the conversation so far is sent after it.
pub fn setup(model: &str, instructions: &str, tools: &[ToolSpec], history: bool) -> Value {
    let mut setup = json!({
        "model": format!("models/{model}"),
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": VOICE}}},
        },
        "systemInstruction": {"parts": [{"text": instructions}]},
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "contextWindowCompression": {"slidingWindow": {}},
    });
    if !tools.is_empty() {
        setup["tools"] = json!([{"functionDeclarations": tools.iter().map(declaration).collect::<Vec<_>>()}]);
    }
    if history {
        setup["historyConfig"] = json!({"initialHistoryInClientContent": true});
    }
    json!({"setup": setup})
}

/// One tool as a function declaration. `BLOCKING`: the model waits for the result and says nothing meanwhile, as the
/// session expects (an answer that begins while a tool runs is dropped).
fn declaration(tool: &ToolSpec) -> Value {
    let mut declaration = json!({"name": tool.name, "description": tool.description, "behavior": "BLOCKING"});
    let parameters = schema(&tool.parameters, &tool.parameters, 0);
    // A function without parameters has none: an object without properties is refused.
    if parameters.get("properties").and_then(Value::as_object).is_some_and(|properties| !properties.is_empty()) {
        declaration["parameters"] = parameters;
    }
    declaration
}

/// How deep `$ref`s are followed.
const REFS: usize = 8;

/// `schema` (JSON Schema, `root` for its `$defs`) as the subset of OpenAPI Gemini takes: its keywords only, `$ref`s
/// resolved, `null` as `nullable`, one `type`.
pub fn schema(schema: &Value, root: &Value, depth: usize) -> Value {
    let Some(object) = schema.as_object() else { return json!({"type": "string"}) };
    if let Some(target) = object.get("$ref").and_then(Value::as_str) {
        let found = target.strip_prefix("#/").and_then(|path| root.pointer(&format!("/{path}")));
        return match found {
            Some(found) if depth < REFS => self::schema(found, root, depth + 1),
            _ => json!({"type": "object"}),
        };
    }
    // A variant list: one alternative besides `null` is that one, nullable.
    for key in ["anyOf", "oneOf"] {
        let Some(variants) = object.get(key).and_then(Value::as_array) else { continue };
        let is_null = |variant: &Value| variant.get("type").and_then(Value::as_str) == Some("null");
        let others: Vec<&Value> = variants.iter().filter(|variant| !is_null(variant)).collect();
        let nullable = others.len() < variants.len();
        let mut out = match others.as_slice() {
            [one] => self::schema(one, root, depth + 1),
            many => {
                json!({"anyOf": many.iter().map(|variant| self::schema(variant, root, depth + 1)).collect::<Vec<_>>()})
            }
        };
        if let Some(description) = object.get("description") {
            out["description"] = description.clone();
        }
        if nullable {
            out["nullable"] = json!(true);
        }
        return out;
    }
    if let Some([one]) = object.get("allOf").and_then(Value::as_array).map(Vec::as_slice) {
        return self::schema(one, root, depth + 1);
    }
    let mut out = Map::new();
    let mut nullable = false;
    match object.get("type") {
        Some(Value::String(kind)) => {
            out.insert("type".into(), json!(kind));
        }
        Some(Value::Array(kinds)) => {
            let names: Vec<&str> = kinds.iter().filter_map(Value::as_str).collect();
            nullable = names.contains(&"null");
            if let Some(kind) = names.iter().find(|kind| **kind != "null") {
                out.insert("type".into(), json!(kind));
            }
        }
        _ => {}
    }
    if let Some(constant) = object.get("const") {
        out.insert("enum".into(), json!([constant]));
    }
    for key in
        ["description", "enum", "minimum", "maximum", "minItems", "maxItems", "minLength", "maxLength", "pattern"]
    {
        if let Some(value) = object.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    // The formats Gemini knows.
    if let Some(format) = object.get("format").and_then(Value::as_str)
        && ["int32", "int64", "float", "double", "enum", "date-time"].contains(&format)
    {
        out.insert("format".into(), json!(format));
    }
    if let Some(items) = object.get("items") {
        out.insert("items".into(), self::schema(items, root, depth + 1));
    }
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        let properties: Map<String, Value> =
            properties.iter().map(|(name, property)| (name.clone(), self::schema(property, root, depth + 1))).collect();
        out.insert("properties".into(), Value::Object(properties));
        if let Some(required) = object.get("required").and_then(Value::as_array) {
            out.insert("required".into(), Value::Array(required.clone()));
        }
    }
    if nullable || object.get("nullable").and_then(Value::as_bool) == Some(true) {
        out.insert("nullable".into(), json!(true));
    }
    Value::Object(out)
}

/// A `Duration` of the server (`"1.840s"`) in milliseconds.
fn offset_ms(value: &Value) -> Option<u64> {
    let seconds: f64 = value.as_str()?.strip_suffix('s')?.parse().ok()?;
    (seconds >= 0.0).then(|| (seconds * 1000.0).round() as u64)
}

/// One live connection to Gemini Live.
pub struct GeminiWire {
    socket: LiveSocket,
    model: String,
    state: State,
    /// What was read and not yet given out: one message may mean several things.
    incoming: VecDeque<Incoming>,
    closed: bool,
}

impl GeminiWire {
    /// The connection `socket` to the engine `model`.
    pub fn new(socket: LiveSocket, model: &str) -> GeminiWire {
        GeminiWire {
            socket,
            model: model.to_string(),
            state: State::default(),
            incoming: VecDeque::new(),
            closed: false,
        }
    }

    async fn write(&mut self, message: Value) -> Result<(), AiError> {
        self.socket.send(Message::text(message.to_string())).await.map_err(|_| AiError::Offline)
    }
}

impl LiveWire for GeminiWire {
    fn send(&mut self, out: Outgoing) -> BoxFuture<'_, Result<(), AiError>> {
        Box::pin(async move {
            if let Outgoing::Configure { instructions, tools } = &out {
                if self.state.setup_sent {
                    eprintln!("torroworkplace: conversation engine: the setup of this connection stays as it was");
                } else {
                    self.state.config = Some((instructions.clone(), tools.clone()));
                }
                return Ok(());
            }
            if !self.state.setup_sent {
                let (instructions, tools) = self.state.config.clone().unwrap_or_default();
                let message = setup(&self.model, &instructions, &tools, matches!(out, Outgoing::History(_)));
                self.state.setup_sent = true;
                self.write(message).await?;
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
                            self.incoming.extend(self.state.flush());
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
                    Some(Ok(Message::Close(frame))) => {
                        // The code only: the reason is the server's words.
                        let code = frame.map_or(0, |frame| u16::from(frame.code));
                        eprintln!("torroworkplace: conversation engine closed the connection: code {code}");
                        self.closed = true;
                        continue;
                    }
                    Some(Err(_)) | None => {
                        self.closed = true;
                        continue;
                    }
                };
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else { continue };
                let read = self.state.read(&message, Instant::now());
                self.incoming.extend(read);
            }
        })
    }

    fn heard_wait(&self) -> Duration {
        HEARD_WAIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Messages of the shapes of the documentation (`tests/fixtures/live/gemini_round.jsonl`, not recorded from the
    /// service: nobody had a Gemini key for it): each line `[milliseconds, message]`.
    fn round() -> Vec<(u64, Value)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/live/gemini_round.jsonl");
        std::fs::read_to_string(path)
            .expect("fixture")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (at, message): (u64, Value) = serde_json::from_str(line).expect("json");
                (at, message)
            })
            .collect()
    }

    /// What reading `messages` gives, each at its time, and what a wait after the last gives (`flush` when due).
    fn read_all(messages: &[(u64, Value)]) -> Vec<Incoming> {
        let start = Instant::now();
        let mut state = State::default();
        let mut out = Vec::new();
        for (at, message) in messages {
            let now = start + Duration::from_millis(*at);
            // What is due before this message comes first.
            while state.deadline().is_some_and(|due| due <= now) {
                out.extend(state.flush());
            }
            out.extend(state.read(message, now));
        }
        out
    }

    /// A sentence by voice activity, its transcript growing, the answer's sound and words with the transcript still
    /// coming; then a tool; then its answer, and the end.
    #[test]
    fn a_round_reads_as_speech_words_sound_and_a_call() {
        let read = read_all(&round());
        let call =
            ToolCall { id: "fc1".into(), name: "calendar_list_events".into(), arguments: "{\"day\":\"today\"}".into() };
        assert_eq!(
            read,
            vec![
                Incoming::SpeechStarted,
                Incoming::SpeechStopped { audio_end_ms: Some(1840) },
                Incoming::AnswerStarted,
                Incoming::AnswerAudio(vec![1, -2]),
                Incoming::AnswerText("Let me ".into()),
                Incoming::AnswerText("look.".into()),
                Incoming::Heard("What's on today?".into()),
                Incoming::AnswerDone { text: "Let me look.".into(), calls: vec![call] },
                Incoming::AnswerStarted,
                Incoming::AnswerText("Nothing.".into()),
                Incoming::GoAway,
                Incoming::AnswerDone { text: "Nothing.".into(), calls: Vec::new() },
            ]
        );
    }

    fn at(ms: u64) -> Instant {
        // One start for all: `Instant::now` differs between calls, but the tests only compare offsets from it.
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *START.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    /// Without `voiceActivity` the transcript says when the person speaks; the words are told when the answer begins
    /// (after the grace), and a piece that comes after the turn is complete starts nothing.
    #[test]
    fn without_voice_activity_the_transcript_decides() {
        let mut state = State::default();
        let piece = |text: &str| json!({"serverContent": {"inputTranscription": {"text": text}}});
        let mut out = state.read(&piece("Was "), at(0));
        out.extend(state.read(&piece("steht an?"), at(300)));
        assert_eq!(out, vec![Incoming::SpeechStarted]);
        assert_eq!(state.deadline(), Some(at(300) + QUIET), "quiet is the only thing to wait for");
        let sound = json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"data": "AQA="}}]}}});
        assert_eq!(state.read(&sound, at(900)), vec![Incoming::AnswerStarted, Incoming::AnswerAudio(vec![1])]);
        assert_eq!(state.deadline(), Some(at(900) + GRACE));
        assert_eq!(
            state.flush(),
            vec![Incoming::SpeechStopped { audio_end_ms: None }, Incoming::Heard("Was steht an?".into())]
        );
        assert_eq!(state.deadline(), None);
        let done = json!({"serverContent": {"turnComplete": true}});
        assert_eq!(state.read(&done, at(2000)), vec![Incoming::AnswerDone { text: String::new(), calls: Vec::new() }]);
        assert!(state.read(&piece("late"), at(2500)).is_empty(), "a late piece starts no sentence");
        assert_eq!(state.deadline(), None);
        assert_eq!(state.read(&piece("Next"), at(4000)), vec![Incoming::SpeechStarted]);
    }

    /// The person speaks into the answer: `interrupted` is the start of speech and the answer is over, with no end.
    #[test]
    fn an_interruption_is_the_start_of_speech() {
        let mut state = State::default();
        let sound = json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"data": "AQA="}}]}}});
        state.read(&sound, at(0));
        let read = state.read(&json!({"serverContent": {"interrupted": true}}), at(500));
        assert_eq!(read, vec![Incoming::SpeechStarted]);
        // What comes next is a new answer.
        assert_eq!(state.read(&sound, at(900)), vec![Incoming::AnswerStarted, Incoming::AnswerAudio(vec![1])]);
    }

    /// A turn whose person said nothing the model heard still ends: the session's turn is told.
    #[test]
    fn a_turn_without_words_still_ends() {
        let mut state = State::default();
        let sound = json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"data": "AQA="}}]}}});
        state.read(&sound, at(0));
        let read = state.read(&json!({"serverContent": {"turnComplete": true}}), at(100));
        assert_eq!(read, vec![Incoming::AnswerDone { text: String::new(), calls: Vec::new() }]);
    }

    /// `setup` comes first, with the history flag only when a history follows; sound before `setupComplete` is
    /// dropped and the rest waits for it; a result carries its tool's name.
    #[test]
    fn what_goes_out_waits_for_the_setup_and_names_its_tool() {
        let tools = vec![ToolSpec {
            name: "tasks_list".into(),
            description: "Lists tasks.".into(),
            parameters: json!({"type": "object", "properties": {"list": {"type": "string"}}, "required": ["list"]}),
        }];
        let with = setup("gemini-3.8-live", "Be brief.", &tools, true);
        assert_eq!(with["setup"]["model"], json!("models/gemini-3.8-live"));
        assert_eq!(with["setup"]["generationConfig"]["responseModalities"], json!(["AUDIO"]));
        assert_eq!(with["setup"]["historyConfig"]["initialHistoryInClientContent"], json!(true));
        assert_eq!(with["setup"]["tools"][0]["functionDeclarations"][0]["behavior"], json!("BLOCKING"));
        assert!(setup("m", "x", &[], false)["setup"].get("historyConfig").is_none());
        assert!(setup("m", "x", &[], false)["setup"].get("tools").is_none());

        let mut state = State::default();
        assert!(state.events(&Outgoing::Audio(vec![0; 160])).is_empty(), "sound before the setup is dropped");
        assert!(state.events(&Outgoing::History(vec![(Role::User, "Hi.".into())])).is_empty(), "history waits");
        state.read(&json!({"setupComplete": {}}), at(0));
        let events = state.events(&Outgoing::Audio(vec![0; 160]));
        assert_eq!(events.len(), 2, "the history first, then the sound");
        assert_eq!(events[0]["clientContent"]["turnComplete"], json!(true));
        assert_eq!(events[1]["realtimeInput"]["audio"]["mimeType"], json!("audio/pcm;rate=16000"));

        let call = json!({"toolCall": {"functionCalls": [{"id": "fc9", "name": "tasks_list", "args": {"list": "a"}}]}});
        state.read(&call, at(10));
        let results = state.events(&Outgoing::ToolResults(vec![("fc9".into(), "[1,2]".into())]));
        assert_eq!(
            results[0]["toolResponse"]["functionResponses"][0],
            json!({"id": "fc9", "name": "tasks_list", "response": {"result": [1, 2]}})
        );
        assert!(state.events(&Outgoing::Cancel { played_ms: 100 }).is_empty(), "the server cut the answer itself");
    }

    /// The history alternates user and model, however many messages of one side come in a row.
    #[test]
    fn the_history_alternates() {
        let items = vec![
            (Role::User, "a".to_string()),
            (Role::User, "b".to_string()),
            (Role::Assistant, "c".to_string()),
            (Role::User, "d".to_string()),
        ];
        let turns = history(&items).expect("turns")["clientContent"]["turns"].clone();
        let roles: Vec<&str> = turns.as_array().expect("array").iter().filter_map(|t| t["role"].as_str()).collect();
        assert_eq!(roles, ["user", "model", "user"]);
        assert_eq!(turns[0]["parts"].as_array().map(Vec::len), Some(2));
        assert!(history(&[]).is_none());
    }

    /// A schema of the catalogue as Gemini takes it: `$ref` resolved, `null` as nullable, unknown words dropped.
    #[test]
    fn a_schema_becomes_the_subset_gemini_takes() {
        let root = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "day": {"type": ["string", "null"], "description": "A day.", "default": "today"},
                "kind": {"$ref": "#/$defs/Kind"},
                "count": {"type": "integer", "format": "uint32", "minimum": 0},
                "tags": {"type": "array", "items": {"type": "string"}},
                "when": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                "more": {"oneOf": [{"type": "string"}, {"type": "number"}]},
            },
            "required": ["day"],
            "$defs": {"Kind": {"type": "string", "enum": ["a", "b"]}},
        });
        let out = schema(&root, &root, 0);
        assert_eq!(out.get("additionalProperties"), None);
        assert_eq!(out.get("$schema"), None);
        assert_eq!(out["required"], json!(["day"]));
        let properties = &out["properties"];
        assert_eq!(properties["day"], json!({"type": "string", "description": "A day.", "nullable": true}));
        assert_eq!(properties["kind"], json!({"type": "string", "enum": ["a", "b"]}));
        assert_eq!(properties["count"], json!({"type": "integer", "minimum": 0}));
        assert_eq!(properties["tags"]["items"], json!({"type": "string"}));
        assert_eq!(properties["when"], json!({"type": "string", "nullable": true}));
        assert_eq!(properties["more"]["anyOf"].as_array().map(Vec::len), Some(2));
        // A `$ref` that loops ends.
        let looping = json!({"$ref": "#/$defs/A", "$defs": {"A": {"$ref": "#/$defs/A"}}});
        assert_eq!(schema(&looping, &looping, 0), json!({"type": "object"}));
    }

    /// A tool without parameters declares none; the `Duration` of the server reads as milliseconds.
    #[test]
    fn small_things() {
        let none = ToolSpec { name: "ping".into(), description: String::new(), parameters: json!({"type": "object"}) };
        assert!(declaration(&none).get("parameters").is_none());
        assert_eq!(offset_ms(&json!("1.840s")), Some(1840));
        assert_eq!(offset_ms(&json!(3)), None);
    }

    /// Neither text nor sound shows in what is logged of an event.
    #[test]
    fn the_debug_form_holds_no_words() {
        let shown = format!("{:?} {:?}", Incoming::Heard("secret words".into()), Incoming::GoAway);
        assert!(!shown.contains("secret") && shown.contains("GoAway"), "{shown}");
    }
}
