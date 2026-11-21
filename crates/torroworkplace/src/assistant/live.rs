//! A conversation engine in the cloud (issue #838, way B of `docs/sprachchat.md`): a model that hears and speaks
//! itself over one live connection. The core holds the connection and its key ([`crate::account::ai::AiAccounts::live_socket`]);
//! the listener's microphone goes to it through the core, never to a program of its own.
//!
//! [`LiveWire`] is what the session (`crate::core::live`) talks to: what goes out ([`Outgoing`]) and what comes in
//! ([`Incoming`]), the same for every provider. [`openai`] speaks the protocol of OpenAI Realtime and Grok Voice; the
//! later engines (Gemini Live, GPT-Live, Nova 2 Sonic) are further adapters beside it.
//!
//! The model thinks itself, but its tools run through the core like any other answer's: each answer of the voice chat
//! is a turn of the assistant's loop ([`super::model`]) whose model is a [`LiveTurn`]. It sends the instructions and
//! tools when they change and the results of the tools the loop ran, and gives the loop what the engine said.
//!
//! Neither text nor sound is logged: `Debug` of [`Outgoing`] and [`Incoming`] names only the kind.

pub mod openai;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc;
use torro_core::ai::{AiError, AiModel, BoxFuture, ChatReply, ChatRequest, Finish, Provider, Role, ToolCall, ToolSpec};
use torro_ipc::assistant::{Entry, EntryContent, SHOWN_MARK};

/// The sample rate of the microphone as the listener sends it, and of what goes to the engine before an adapter
/// converts it (16-bit mono).
pub const MIC_RATE: u32 = 16_000;

/// The sample rate of what the engines say (16-bit mono), as the speech helper plays it.
pub const ANSWER_RATE: u32 = 24_000;

/// How many messages and answers of the conversation a new connection is given as text (issue #838): enough to go on
/// where it was, little enough to start fast.
pub const HISTORY_MOST: usize = 40;

/// What goes to the engine.
#[derive(Clone, PartialEq)]
pub enum Outgoing {
    /// How it answers and the tools it may call; sent at the start and whenever they change.
    Configure { instructions: String, tools: Vec<ToolSpec> },
    /// The conversation so far as text, after a new connection: `(Role::User | Role::Assistant, text)`.
    History(Vec<(Role, String)>),
    /// Sound of the microphone, 16-bit mono at [`MIC_RATE`].
    Audio(Vec<i16>),
    /// The results of the tools the engine called, by the id of its call; the engine answers on.
    ToolResults(Vec<(String, String)>),
    /// The person spoke into the answer: it stops, and only what was played (`played_ms`) stays in its memory.
    Cancel { played_ms: u32 },
}

/// What comes from the engine.
#[derive(Clone, PartialEq)]
pub enum Incoming {
    /// The person began to speak.
    SpeechStarted,
    /// The person stopped; `audio_end_ms` is where in the sound sent since the connection began, when it says.
    SpeechStopped { audio_end_ms: Option<u64> },
    /// What the person said, as text; empty when nothing was understood.
    Heard(String),
    /// The engine begins an answer: what comes from here on is this answer's.
    AnswerStarted,
    /// A piece of the answer's text as it is spoken.
    AnswerText(String),
    /// A piece of the answer's sound, 16-bit mono at [`ANSWER_RATE`].
    AnswerAudio(Vec<i16>),
    /// The answer is over: its whole text and the tools it calls (none when it is done).
    AnswerDone { text: String, calls: Vec<ToolCall> },
    /// The connection closed.
    Closed,
    /// The engine failed with its code (never its words, which could repeat what was said).
    Failed { code: String },
}

impl std::fmt::Debug for Outgoing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Outgoing::Configure { .. } => "Configure",
            Outgoing::History(_) => "History",
            Outgoing::Audio(_) => "Audio",
            Outgoing::ToolResults(_) => "ToolResults",
            Outgoing::Cancel { .. } => "Cancel",
        })
    }
}

impl std::fmt::Debug for Incoming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Incoming::SpeechStarted => f.write_str("SpeechStarted"),
            Incoming::SpeechStopped { .. } => f.write_str("SpeechStopped"),
            Incoming::Heard(_) => f.write_str("Heard"),
            Incoming::AnswerStarted => f.write_str("AnswerStarted"),
            Incoming::AnswerText(_) => f.write_str("AnswerText"),
            Incoming::AnswerAudio(_) => f.write_str("AnswerAudio"),
            Incoming::AnswerDone { calls, .. } => write!(f, "AnswerDone({} calls)", calls.len()),
            Incoming::Closed => f.write_str("Closed"),
            Incoming::Failed { code } => write!(f, "Failed({code})"),
        }
    }
}

/// One live connection to a conversation engine, as the session uses it. `next` is safe to drop while it waits: the
/// session waits for it beside the microphone.
pub trait LiveWire: Send {
    /// Sends `out` to the engine.
    fn send(&mut self, out: Outgoing) -> BoxFuture<'_, Result<(), AiError>>;

    /// The next thing the engine says; [`Incoming::Closed`] once the connection is gone, for good.
    fn next(&mut self) -> BoxFuture<'_, Incoming>;
}

/// What the session gives the turn of the answer that runs ([`LiveTurn`]).
#[derive(Debug, Clone, PartialEq)]
pub enum TurnEvent {
    /// A piece of the answer's text.
    Text(String),
    /// The answer is over, with the tools it calls.
    Done { text: String, calls: Vec<ToolCall> },
}

/// The instructions and tools the engine was last given; shared by the session and its turns, so a new connection
/// gets them again and a turn sends them only when they changed.
pub type Configured = Arc<Mutex<Option<(String, Vec<ToolSpec>)>>>;

/// The model of one answer of the voice chat (issue #838): the conversation engine behind the session. The
/// assistant's loop asks it like any model ([`Provider::chat`]); it sends what changed and waits for the engine.
pub struct LiveTurn {
    /// The model of the engine, for the answer's entry.
    pub model: String,
    commands: mpsc::UnboundedSender<Outgoing>,
    events: tokio::sync::Mutex<mpsc::UnboundedReceiver<TurnEvent>>,
    configured: Configured,
    /// The calls whose results went to the engine.
    sent: Mutex<BTreeSet<String>>,
}

impl LiveTurn {
    /// A turn of `model` that sends through `commands`, hears through `events` and shares `configured` with the
    /// session.
    pub fn new(
        model: &str,
        commands: mpsc::UnboundedSender<Outgoing>,
        events: mpsc::UnboundedReceiver<TurnEvent>,
        configured: Configured,
    ) -> LiveTurn {
        LiveTurn {
            model: model.to_string(),
            commands,
            events: tokio::sync::Mutex::new(events),
            configured,
            sent: Mutex::default(),
        }
    }

    /// One step of the loop: the instructions and tools when they changed, the results of the tools run since, then
    /// the engine's answer. Its text ends with the line [`SHOWN_MARK`]: everything the engine says is spoken.
    async fn step(&self, request: &ChatRequest, on_text: &mut (dyn FnMut(&str) + Send)) -> Result<ChatReply, AiError> {
        let gone =
            || AiError::Format(torro_core::i18n::tr("The connection to the conversation engine was lost.").into());
        let instructions =
            request.messages.iter().find(|message| message.role == Role::System).map(|message| message.content.clone());
        let wanted = (instructions.unwrap_or_default(), request.tools.clone());
        let changed = {
            let mut configured = self.configured.lock().unwrap_or_else(PoisonError::into_inner);
            let changed = configured.as_ref() != Some(&wanted);
            if changed {
                *configured = Some(wanted.clone());
            }
            changed
        };
        if changed {
            let (instructions, tools) = wanted;
            self.commands.send(Outgoing::Configure { instructions, tools }).map_err(|_| gone())?;
        }
        let results = self.new_results(request);
        if !results.is_empty() {
            self.commands.send(Outgoing::ToolResults(results)).map_err(|_| gone())?;
        }
        let mut events = self.events.lock().await;
        loop {
            match events.recv().await {
                Some(TurnEvent::Text(piece)) => on_text(&piece),
                Some(TurnEvent::Done { text, calls }) => {
                    let finish = if calls.is_empty() { Finish::Stop } else { Finish::ToolCalls };
                    // All of it was spoken: the spoken part is the whole answer, nothing is only shown.
                    let text = if text.trim().is_empty() { text } else { format!("{}\n{SHOWN_MARK}", text.trim_end()) };
                    return Ok(ChatReply { text, tool_calls: calls, finish, usage: None, thinking: Vec::new() });
                }
                None => return Err(gone()),
            }
        }
    }

    /// The results of tools in `request` after its last assistant message that did not go to the engine yet.
    fn new_results(&self, request: &ChatRequest) -> Vec<(String, String)> {
        let last = request.messages.iter().rposition(|message| message.role == Role::Assistant);
        let after = last.map_or(&request.messages[..0], |at| &request.messages[at + 1..]);
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        after
            .iter()
            .filter(|message| message.role == Role::Tool)
            .filter_map(|message| Some((message.tool_call_id.clone()?, message.content.clone())))
            .filter(|(id, _)| sent.insert(id.clone()))
            .collect()
    }
}

/// [`LiveTurn`] as the model of an answer ([`super::model::ModelAccess::provider`]).
pub struct LiveProvider(pub Arc<LiveTurn>);

impl Provider for LiveProvider {
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
        Box::pin(async { Ok(vec![AiModel::named(&self.0.model)]) })
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
        Box::pin(self.0.step(request, on_text))
    }
}

/// The last [`HISTORY_MOST`] messages and answers of `entries` as text, for a new connection: what the person said
/// and what was spoken of each answer (its spoken part when it has one).
pub fn history(entries: &[Entry]) -> Vec<(Role, String)> {
    let mut said: Vec<(Role, String)> = entries
        .iter()
        .filter_map(|entry| match &entry.content {
            EntryContent::Message { text, .. } => Some((Role::User, text.clone())),
            // The spoken part says what was heard; an answer in one part was heard whole.
            EntryContent::Answer { text, spoken, .. } => {
                Some((Role::Assistant, spoken.clone().unwrap_or(text.clone())))
            }
            _ => None,
        })
        .filter(|(_, text)| !text.trim().is_empty())
        .collect();
    let skip = said.len().saturating_sub(HISTORY_MOST);
    said.drain(..skip);
    said
}

/// `samples` at [`MIC_RATE`] as at `rate`, by linear steps: OpenAI hears only 24 kHz.
pub fn resample(samples: &[i16], rate: u32) -> Vec<i16> {
    if rate == MIC_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let out = (samples.len() as u64 * u64::from(rate) / u64::from(MIC_RATE)) as usize;
    (0..out)
        .map(|n| {
            let at = n as f64 * f64::from(MIC_RATE) / f64::from(rate);
            let (i, frac) = (at.floor() as usize, at.fract());
            let a = f64::from(samples[i.min(samples.len() - 1)]);
            let b = f64::from(samples[(i + 1).min(samples.len() - 1)]);
            (a + (b - a) * frac).round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use torro_core::ai::ChatMessage;

    use super::*;

    fn turn() -> (LiveTurn, mpsc::UnboundedReceiver<Outgoing>, mpsc::UnboundedSender<TurnEvent>, Configured) {
        let (commands, sent) = mpsc::unbounded_channel();
        let (events, heard) = mpsc::unbounded_channel();
        let configured = Configured::default();
        (LiveTurn::new("gpt-realtime-2.1", commands, heard, Arc::clone(&configured)), sent, events, configured)
    }

    fn request(messages: Vec<ChatMessage>, tools: Vec<ToolSpec>) -> ChatRequest {
        ChatRequest { model: "gpt-realtime-2.1".into(), messages, tools, max_tokens: None, effort: None }
    }

    fn tool(name: &str) -> ToolSpec {
        ToolSpec { name: name.into(), description: String::new(), parameters: serde_json::json!({"type": "object"}) }
    }

    /// The instructions and tools go once and again only when they change; the results of the tools once each; the
    /// answer's text streams and ends spoken whole.
    #[tokio::test]
    async fn a_turn_sends_what_changed_and_answers_with_the_engine() {
        let (turn, mut sent, events, configured) = turn();
        let first = request(vec![ChatMessage::system("Be brief."), ChatMessage::user("Hi")], vec![tool("a")]);
        events.send(TurnEvent::Text("Hel".into())).expect("open");
        events.send(TurnEvent::Text("lo.".into())).expect("open");
        let call = ToolCall { id: "call_1".into(), name: "a".into(), arguments: "{}".into() };
        events.send(TurnEvent::Done { text: "Hello.".into(), calls: vec![call.clone()] }).expect("open");
        let mut streamed = String::new();
        let reply = turn.step(&first, &mut |piece| streamed.push_str(piece)).await.expect("a reply");
        assert_eq!((streamed.as_str(), reply.tool_calls.clone()), ("Hello.", vec![call.clone()]));
        assert_eq!(reply.text, format!("Hello.\n{SHOWN_MARK}"));
        assert_eq!(
            sent.try_recv().expect("configured"),
            Outgoing::Configure { instructions: "Be brief.".into(), tools: vec![tool("a")] }
        );
        assert!(sent.try_recv().is_err(), "no results yet");
        assert!(configured.lock().expect("lock").is_some());
        let mut second = first.messages.clone();
        second.push(ChatMessage::assistant("Hello.", vec![call]));
        second.push(ChatMessage::tool("call_1", "{\"ok\":true}"));
        events.send(TurnEvent::Done { text: String::new(), calls: Vec::new() }).expect("open");
        let again = request(second, vec![tool("a")]);
        let reply = turn.step(&again, &mut |_| {}).await.expect("a reply");
        assert_eq!(reply.text, "", "nothing more to say");
        assert_eq!(
            sent.try_recv().expect("results"),
            Outgoing::ToolResults(vec![("call_1".into(), "{\"ok\":true}".into())])
        );
        assert!(sent.try_recv().is_err(), "same instructions: not sent again");
        drop(events);
        assert!(turn.step(&again, &mut |_| {}).await.is_err(), "the session went: the answer fails");
        assert!(sent.try_recv().is_err(), "a result goes once");
    }

    /// A new connection gets the conversation's last messages and spoken answers as text.
    #[test]
    fn the_history_is_the_messages_and_what_was_spoken() {
        let entry = |seq, content| Entry { seq, time: chrono::Utc::now(), content };
        let message = |text: &str| EntryContent::Message {
            text: text.into(),
            engine: "e".into(),
            model: None,
            effort: None,
            without: Vec::new(),
            mode: Default::default(),
            from: None,
        };
        let answer = |spoken: &str, shown: &str| EntryContent::Answer {
            text: shown.into(),
            engine: "e".into(),
            model: None,
            spoken: Some(spoken.into()),
        };
        let mut entries: Vec<Entry> = (0..50).map(|n| entry(n, message(&format!("m{n}")))).collect();
        entries.push(entry(50, answer("Said.", "Shown.")));
        let history = history(&entries);
        assert_eq!(history.len(), HISTORY_MOST);
        assert_eq!(history.last(), Some(&(Role::Assistant, "Said.".to_string())));
    }

    #[test]
    fn the_microphone_is_resampled_for_openai() {
        assert_eq!(resample(&[0, 100], MIC_RATE), vec![0, 100]);
        let up = resample(&[0, 300, 600, 900], 24_000);
        assert_eq!(up.len(), 6);
        assert_eq!(&up[..3], &[0, 200, 400]);
    }
}
