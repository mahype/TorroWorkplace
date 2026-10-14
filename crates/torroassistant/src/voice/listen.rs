//! The listener's loop ([`Listener`]): wait for the wake word, play the tone, record until a
//! pause, turn it into text, send it ([`Assistant::ask`]), speak the answer, and again – while the
//! setting `voice.enabled` stays on. The microphone is closed while the assistant answers, so the
//! spoken answer never wakes it.
//!
//! [`CoreAssistant`] is the way to the core: it sends the sentence as `assistant.send_message` to
//! the conversation "Voice" with the engine used last, opens the quick window and follows the
//! answer with `assistant.get_conversation`. It sends nothing else: no request it builds can
//! decide an approval, so no word spoken ever allows anything ([`CoreAssistant::REQUESTS`]).

use std::time::{Duration, Instant};

use torro_core::i18n::tr;
use torro_core::settings::Voice;
use torro_ipc::assistant::{ConversationSummary, Engine, Entry, EntryContent};
use torro_ipc::{Request, Response, VoiceState};

use super::audio::{Endpoint, Limits, NoiseFloor, rms};
use super::wake::strip_wake_phrase;
use super::{Recorder, Speaker, Transcriber, VoiceError, WakeWord};
use crate::backend::Refusal;

/// What the listener asks of the core.
pub trait Assistant {
    /// The voice settings as the core holds them now (`settings.get_settings`).
    fn settings(&mut self) -> Result<Voice, VoiceError>;

    /// Sends `text` to the assistant, shows the conversation and waits for the answer.
    fn ask(&mut self, text: &str) -> Result<Reply, VoiceError>;
}

/// The assistant's answer to a spoken sentence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    /// The answer's text, as the model wrote it (Markdown).
    pub text: String,
    /// A command of the answer waits for a person's approval: the window shows the card.
    pub approval: bool,
    /// The answer failed, with the message for people.
    pub failed: Option<String>,
}

impl Reply {
    /// What is said aloud: the answer without Markdown, or that it waits for approval in the
    /// window, or that it failed. `None` when there is nothing to say.
    pub fn spoken(&self) -> Option<String> {
        if self.approval {
            return Some(tr("The assistant needs your approval. Please look at the window.").to_string());
        }
        if self.failed.is_some() {
            return Some(tr("The assistant could not answer.").to_string());
        }
        Some(speakable(&self.text)).filter(|text| !text.is_empty())
    }
}

/// The most of an answer that is spoken, in characters; the window shows all of it.
const MOST_SPOKEN: usize = 600;

/// `markdown` as it is read aloud: without marks (`**`, `#`, `` ` ``, list dashes), links as their
/// text, on one line, at most [`MOST_SPOKEN`] characters, ending at a sentence where it is cut.
pub fn speakable(markdown: &str) -> String {
    let mut text = String::new();
    let mut chars = markdown.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '#' | '>' | '|' | '~' => {}
            // `[text](address)` → `text`.
            ']' if chars.peek() == Some(&'(') => {
                for skipped in chars.by_ref() {
                    if skipped == ')' {
                        break;
                    }
                }
            }
            '[' | ']' => {}
            c if c.is_control() => text.push(' '),
            c => text.push(c),
        }
    }
    let words: Vec<&str> = text.split_whitespace().filter(|word| *word != "-").collect();
    let line = words.join(" ");
    if line.chars().count() <= MOST_SPOKEN {
        return line;
    }
    let cut: String = line.chars().take(MOST_SPOKEN).collect();
    match cut.rfind(['.', '!', '?']) {
        Some(end) => cut[..=end].to_string(),
        None => format!("{cut}…"),
    }
}

/// How one round of listening ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The setting `voice.enabled` went off, or the core is gone: stop.
    VoiceOff,
    /// Another voice setting changed (microphone, model): build the parts anew.
    Changed,
    /// A sentence was sent and answered.
    Answered,
    /// The wake word, but nothing understood: listen again.
    Nothing,
    /// The recording program stopped.
    RecordingStopped,
}

/// The listener (see the module) on its parts.
pub struct Listener<'a, A: Assistant> {
    pub recorder: Box<dyn Recorder>,
    pub wake: Box<dyn WakeWord>,
    pub transcriber: Box<dyn Transcriber>,
    pub speaker: Box<dyn Speaker>,
    pub assistant: &'a mut A,
    /// Told every change of state (the core reads it to show it in the tray).
    pub report: Box<dyn FnMut(VoiceState) + 'a>,
    pub limits: Limits,
    /// How many frames go by between two looks at the settings (about two seconds).
    pub check_every: usize,
}

impl<A: Assistant> Listener<'_, A> {
    /// Listens until voice is switched off, another voice setting changes or the recording
    /// stops. `voice` are the settings the parts were built for. Nothing is recorded when voice
    /// is off. Fails when the microphone cannot be opened.
    pub fn run(&mut self, voice: &Voice) -> Result<Ended, VoiceError> {
        loop {
            match self.listen_once(voice)? {
                Ended::Answered | Ended::Nothing => {}
                ended => {
                    (self.report)(VoiceState::Off);
                    return Ok(ended);
                }
            }
        }
    }

    /// One round: waits for the wake word, then records, sends and answers one sentence.
    /// Failures of speech to text or of the core are written to the log and end the round with
    /// [`Ended::Nothing`]; only the microphone's fail it.
    pub fn listen_once(&mut self, voice: &Voice) -> Result<Ended, VoiceError> {
        if let Some(ended) = self.check(voice) {
            return Ok(ended);
        }
        let mut stream = self.recorder.open()?;
        (self.report)(VoiceState::Listening);
        let mut floor = NoiseFloor::default();
        let mut frames = 0usize;
        loop {
            let Some(frame) = stream.frame()? else { return Ok(Ended::RecordingStopped) };
            frames += 1;
            if frames.is_multiple_of(self.check_every.max(1))
                && let Some(ended) = self.check(voice)
            {
                return Ok(ended);
            }
            if self.wake.hears(&frame)? {
                break;
            }
            floor.follow(rms(&frame));
        }
        (self.report)(VoiceState::Recording);
        self.speaker.chime();
        let mut sentence = Vec::new();
        let mut end = Endpoint::new(floor, self.limits);
        while let Some(frame) = stream.frame()? {
            sentence.extend_from_slice(&frame);
            if end.push(&frame) {
                break;
            }
        }
        // The microphone closes before the answer: it is not recorded, and it cannot wake it.
        drop(stream);
        (self.report)(VoiceState::Answering);
        let ended = self.answer(voice, &sentence, end.heard_speech());
        self.wake.reset();
        Ok(ended)
    }

    /// Whether the settings say to stop ([`Ended::VoiceOff`]) or to build the parts anew
    /// ([`Ended::Changed`]); `None` to go on. A core that cannot be asked counts as off: without
    /// it, the person could not switch the microphone off.
    fn check(&mut self, voice: &Voice) -> Option<Ended> {
        match self.assistant.settings() {
            Ok(now) if !now.enabled => Some(Ended::VoiceOff),
            Ok(now) if now != *voice => Some(Ended::Changed),
            Ok(_) => None,
            Err(err) => {
                eprintln!("torroassistant: stops listening: {err}");
                Some(Ended::VoiceOff)
            }
        }
    }

    /// Turns `sentence` into text, sends it and speaks the answer if `voice` says so.
    fn answer(&mut self, voice: &Voice, sentence: &[i16], spoken: bool) -> Ended {
        if !spoken {
            return Ended::Nothing;
        }
        let text = match self.transcriber.transcribe(sentence) {
            Ok(text) => strip_wake_phrase(&text),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return Ended::Nothing;
            }
        };
        if text.is_empty() {
            return Ended::Nothing;
        }
        let reply = match self.assistant.ask(&text) {
            Ok(reply) => reply,
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return Ended::Nothing;
            }
        };
        if voice.speak_answers
            && let Some(words) = reply.spoken()
            && let Err(err) = self.speaker.say(&words)
        {
            eprintln!("torroassistant: cannot speak the answer: {err}");
        }
        Ended::Answered
    }
}

/// The conversation spoken messages go to.
pub fn voice_title() -> &'static str {
    tr("Voice")
}

/// The way to the core (see the module): `ask` sends a request, `open` shows a conversation in the
/// quick window.
pub struct CoreAssistant<F, O> {
    ask: F,
    open: O,
    /// How long to wait between two looks at a running answer.
    pause: Duration,
    /// How long to follow an answer at most.
    wait: Duration,
}

impl<F, O> CoreAssistant<F, O>
where
    F: FnMut(Request) -> Result<Response, Refusal>,
    O: FnMut(&str),
{
    /// Every request the listener sends, by its catalogue name. Nothing that decides an approval
    /// (`approvals.decide_approval`) or switches voice on: tests check every request against it.
    pub const REQUESTS: [&'static str; 6] = [
        "settings.get_settings",
        "assistant.list_conversations",
        "assistant.list_engines",
        "assistant.send_message",
        "assistant.rename_conversation",
        "assistant.get_conversation",
    ];

    pub fn new(ask: F, open: O, pause: Duration, wait: Duration) -> Self {
        CoreAssistant { ask, open, pause, wait }
    }

    /// Sends `request`, one of [`CoreAssistant::REQUESTS`]; any other is refused here, before it
    /// leaves, so the voice path can never grow into deciding approvals.
    fn request(&mut self, request: Request) -> Result<Response, VoiceError> {
        let name = request.operation().name();
        if !Self::REQUESTS.contains(&name) {
            return Err(VoiceError::Core(Refusal::new("forbidden", format!("the listener does not send {name}"))));
        }
        (self.ask)(request).map_err(VoiceError::Core)
    }

    /// The conversation "Voice" if there is one, and the engine and model used last.
    fn target(&mut self) -> Result<(Option<String>, String, Option<String>), VoiceError> {
        let conversations = match self.request(Request::ListConversations)? {
            Response::Conversations { conversations } => conversations,
            other => return Err(unexpected(&other)),
        };
        let voice = conversations.iter().find(|conversation| conversation.title == voice_title()).map(|c| c.id.clone());
        if let Some((engine, model)) = last_engine(&conversations) {
            return Ok((voice, engine, model));
        }
        let engines = match self.request(Request::ListEngines)? {
            Response::Engines { engines } => engines,
            other => return Err(unexpected(&other)),
        };
        let engine = first_engine(&engines).ok_or_else(|| {
            VoiceError::Core(Refusal::new("not-found", "no engine: add an AI account in Torro Workplace"))
        })?;
        Ok((voice, engine, None))
    }
}

/// The core answered with something else than asked, named by its kind only: a conversation's
/// text must never reach the log of the listener.
fn unexpected(response: &Response) -> VoiceError {
    let kind = serde_json::to_value(response)
        .ok()
        .and_then(|value| value.get("result").and_then(|result| result.as_str()).map(str::to_string))
        .unwrap_or_default();
    VoiceError::Core(Refusal::new("error", format!("unexpected answer {kind:?}")))
}

/// The engine and model of the conversation used last.
fn last_engine(conversations: &[ConversationSummary]) -> Option<(String, Option<String>)> {
    let last = conversations.iter().max_by_key(|conversation| conversation.updated)?;
    Some((last.engine.clone(), last.model.clone()))
}

/// The first engine with nothing known against it, else the first.
fn first_engine(engines: &[Engine]) -> Option<String> {
    engines.iter().find(|engine| engine.problem.is_none()).or(engines.first()).map(|engine| engine.id.clone())
}

/// The reply in `entries` (after the message): the answers' text, an approval, a failure.
fn reply_of(entries: &[Entry]) -> Reply {
    let mut reply = Reply::default();
    for entry in entries {
        match &entry.content {
            EntryContent::Answer { text, .. } => {
                if !reply.text.is_empty() {
                    reply.text.push('\n');
                }
                reply.text.push_str(text);
            }
            EntryContent::Approval { .. } => reply.approval = true,
            EntryContent::Error { message, .. } => reply.failed = Some(message.clone()),
            _ => {}
        }
    }
    reply
}

impl<F, O> Assistant for CoreAssistant<F, O>
where
    F: FnMut(Request) -> Result<Response, Refusal>,
    O: FnMut(&str),
{
    fn settings(&mut self) -> Result<Voice, VoiceError> {
        match self.request(Request::Settings)? {
            Response::Settings { settings } => Ok(settings.voice),
            other => Err(unexpected(&other)),
        }
    }

    fn ask(&mut self, text: &str) -> Result<Reply, VoiceError> {
        let (conversation, engine, model) = self.target()?;
        let new = conversation.is_none();
        let send = Request::SendMessage { conversation, engine, model, text: text.to_string(), without: Vec::new() };
        let (id, mut after) = match self.request(send)? {
            Response::Conversation { conversation, .. } => (conversation.id, conversation.last),
            other => return Err(unexpected(&other)),
        };
        if new {
            let rename = Request::RenameConversation { id: id.clone(), title: voice_title().to_string() };
            if let Err(err) = self.request(rename) {
                eprintln!("torroassistant: cannot name the conversation: {err}");
            }
        }
        (self.open)(&id);
        // Entries by number: a growing answer comes again under the same one.
        let mut entries: Vec<Entry> = Vec::new();
        let until = Instant::now() + self.wait;
        loop {
            let (summary, new_entries) =
                match self.request(Request::GetConversation { id: id.clone(), after: Some(after) })? {
                    Response::Conversation { conversation, entries } => (conversation, entries),
                    other => return Err(unexpected(&other)),
                };
            for entry in new_entries {
                entries.retain(|kept| kept.seq != entry.seq);
                entries.push(entry);
            }
            // Only stored entries move the cursor; the one still written comes again.
            after = after.max(summary.last.min(entries.iter().map(|entry| entry.seq).max().unwrap_or(after)));
            let reply = reply_of(&entries);
            // An approval waits for a person in the window; the voice does not wait with it.
            if !summary.running || reply.approval || Instant::now() >= until {
                return Ok(reply);
            }
            std::thread::sleep(self.pause);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use chrono::{TimeZone, Utc};
    use torro_core::settings::Settings;
    use torro_ipc::assistant::EngineKind;

    use super::super::Stream;
    use super::super::audio::FRAME;
    use super::super::audio::tests::fixture;
    use super::*;

    /// Plays sound files as the microphone and counts how often it was opened.
    struct Files {
        files: VecDeque<&'static str>,
        opened: Rc<RefCell<usize>>,
    }

    struct Frames(VecDeque<Vec<i16>>);

    impl Stream for Frames {
        fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError> {
            Ok(self.0.pop_front())
        }
    }

    impl Recorder for Files {
        fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError> {
            *self.opened.borrow_mut() += 1;
            let name = self.files.pop_front().unwrap_or("silence.wav");
            Ok(Box::new(Frames(fixture(name).chunks(FRAME).map(<[i16]>::to_vec).collect())))
        }
    }

    /// Hears the wake word once, at the `at`-th frame of the first round.
    struct WakeAt {
        at: Option<usize>,
        seen: usize,
    }

    impl WakeWord for WakeAt {
        fn hears(&mut self, _frame: &[i16]) -> Result<bool, VoiceError> {
            self.seen += 1;
            if Some(self.seen) == self.at {
                self.at = None;
                return Ok(true);
            }
            Ok(false)
        }

        fn reset(&mut self) {
            self.seen = 0;
        }
    }

    struct Says(&'static str);

    impl Transcriber for Says {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            assert!(!samples.is_empty());
            Ok(self.0.to_string())
        }
    }

    /// Keeps what it was to say.
    struct Mouth(Rc<RefCell<Vec<String>>>, Rc<RefCell<usize>>);

    impl Speaker for Mouth {
        fn say(&mut self, text: &str) -> Result<(), VoiceError> {
            self.0.borrow_mut().push(text.to_string());
            Ok(())
        }

        fn chime(&mut self) {
            *self.1.borrow_mut() += 1;
        }
    }

    /// A core in memory: the settings, the conversations, every request it got.
    struct Core {
        voice: Voice,
        /// Voice is switched off after this many looks at the settings.
        off_after: usize,
        looks: usize,
        conversations: Vec<ConversationSummary>,
        answer: Vec<EntryContent>,
        sent: Vec<Request>,
    }

    fn summary(id: &str, title: &str, engine: &str, day: u32, last: u64, running: bool) -> ConversationSummary {
        let time = Utc.with_ymd_and_hms(2026, 10, day, 9, 0, 0).single().expect("time");
        ConversationSummary {
            id: id.into(),
            title: title.into(),
            engine: engine.into(),
            model: Some(format!("{engine}-model")),
            created: time,
            updated: time,
            pinned: false,
            running,
            last,
        }
    }

    impl Core {
        fn new(voice: Voice) -> Core {
            Core {
                voice,
                off_after: usize::MAX,
                looks: 0,
                conversations: vec![
                    summary("a1", "Plan for tomorrow", "ai:ollama@localhost:11434", 12, 4, false),
                    summary("b2", "Trip", "ai:openrouter", 13, 2, false),
                ],
                answer: vec![EntryContent::Answer {
                    text: "You have **two** events today: [Standup](https://x) and lunch.".into(),
                    engine: "ai:openrouter".into(),
                    model: None,
                }],
                sent: Vec::new(),
            }
        }

        fn answer(&mut self, request: Request) -> Result<Response, Refusal> {
            self.sent.push(request.clone());
            let entry = |seq, content| Entry { seq, time: Utc::now(), content };
            match request {
                Request::Settings => {
                    self.looks += 1;
                    let enabled = self.voice.enabled && self.looks <= self.off_after;
                    let voice = Voice { enabled, ..self.voice.clone() };
                    Ok(Response::Settings { settings: Settings { voice, ..Settings::default() } })
                }
                Request::ListConversations => Ok(Response::Conversations { conversations: self.conversations.clone() }),
                Request::ListEngines => Ok(Response::Engines {
                    engines: vec![Engine {
                        id: "test".into(),
                        kind: EngineKind::Test,
                        name: "Test".into(),
                        problem: None,
                    }],
                }),
                Request::SendMessage { conversation, engine, text, model, .. } => {
                    let id = conversation.unwrap_or_else(|| "c9".into());
                    let conversation = ConversationSummary { model, ..summary(&id, &text, &engine, 14, 1, true) };
                    let message = EntryContent::Message { text, engine, model: None, without: Vec::new() };
                    Ok(Response::Conversation { conversation, entries: vec![entry(1, message)] })
                }
                Request::RenameConversation { .. } => Ok(Response::Ok),
                Request::GetConversation { id, after } => {
                    let entries: Vec<Entry> = self
                        .answer
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(i, content)| entry(i as u64 + 2, content))
                        .filter(|e| Some(e.seq) > after)
                        .collect();
                    let last = entries.iter().map(|e| e.seq).max().unwrap_or(1);
                    let conversation = summary(&id, "Voice", "ai:openrouter", 14, last, false);
                    Ok(Response::Conversation { conversation, entries })
                }
                other => Err(Refusal::new("forbidden", format!("the listener sent {}", other.operation().name()))),
            }
        }
    }

    struct Run {
        sent: Vec<Request>,
        said: Vec<String>,
        opened: usize,
        chimes: usize,
        windows: Vec<String>,
        states: Vec<VoiceState>,
        ended: Result<Ended, VoiceError>,
    }

    /// The listener on `files`, waking at frame `wake`, hearing `heard`, with the core `core`.
    fn run(core: Core, files: &[&'static str], wake: Option<usize>, heard: &'static str) -> Run {
        let core = Rc::new(RefCell::new(core));
        let windows = Rc::new(RefCell::new(Vec::new()));
        let (ask_core, shown) = (Rc::clone(&core), Rc::clone(&windows));
        let mut assistant = CoreAssistant::new(
            move |request| ask_core.borrow_mut().answer(request),
            move |id: &str| shown.borrow_mut().push(id.to_string()),
            Duration::ZERO,
            Duration::from_secs(5),
        );
        let (said, chimes, opened) =
            (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(0)), Rc::new(RefCell::new(0)));
        let states = Rc::new(RefCell::new(Vec::new()));
        let voice = core.borrow().voice.clone();
        let reported = Rc::clone(&states);
        let mut listener = Listener {
            recorder: Box::new(Files { files: files.iter().copied().collect(), opened: Rc::clone(&opened) }),
            wake: Box::new(WakeAt { at: wake, seen: 0 }),
            transcriber: Box::new(Says(heard)),
            speaker: Box::new(Mouth(Rc::clone(&said), Rc::clone(&chimes))),
            assistant: &mut assistant,
            report: Box::new(move |state| reported.borrow_mut().push(state)),
            limits: Limits::default(),
            check_every: 10,
        };
        let ended = listener.run(&voice);
        drop(listener);
        drop(assistant);
        let sent = core.borrow().sent.clone();
        Run {
            sent,
            said: said.borrow().clone(),
            opened: *opened.borrow(),
            chimes: *chimes.borrow(),
            windows: windows.borrow().clone(),
            states: states.borrow().clone(),
            ended,
        }
    }

    fn on() -> Voice {
        Voice { enabled: true, ..Voice::default() }
    }

    fn messages(sent: &[Request]) -> Vec<&Request> {
        sent.iter().filter(|request| matches!(request, Request::SendMessage { .. })).collect()
    }

    #[test]
    fn nothing_is_recorded_while_voice_is_off() {
        let run = run(Core::new(Voice::default()), &["utterance.wav"], Some(2), "Hey Torro, what is on?");
        assert_eq!(run.ended, Ok(Ended::VoiceOff));
        assert_eq!(run.opened, 0, "the microphone was never opened");
        assert!(messages(&run.sent).is_empty());
        assert_eq!(run.states, [VoiceState::Off]);
    }

    #[test]
    fn the_transcript_is_sent_as_send_message_and_spoken() {
        // The sentence, then a round in silence after which voice is switched off.
        let core = Core { off_after: 2, ..Core::new(on()) };
        let run = run(core, &["utterance.wav", "silence.wav"], Some(2), " Hey Torro, what is on today?");
        assert_eq!(run.ended, Ok(Ended::VoiceOff));
        let sent = messages(&run.sent);
        assert_eq!(sent.len(), 1);
        let Request::SendMessage { conversation, engine, model, text, without } = sent[0] else { unreachable!() };
        assert_eq!(text, "what is on today?", "without the wake word");
        assert_eq!(
            (conversation, engine.as_str()),
            (&None, "ai:openrouter"),
            "a new conversation, the engine used last"
        );
        assert_eq!(model.as_deref(), Some("ai:openrouter-model"));
        assert!(without.is_empty());
        assert!(run.sent.contains(&Request::RenameConversation { id: "c9".into(), title: voice_title().into() }));
        assert_eq!(run.windows, ["c9"], "the quick window shows it");
        assert_eq!(run.chimes, 1);
        assert_eq!(run.said, ["You have two events today: Standup and lunch."]);
        assert_eq!(
            run.states[..4],
            [VoiceState::Listening, VoiceState::Recording, VoiceState::Answering, VoiceState::Listening]
        );
        assert_eq!(run.states.last(), Some(&VoiceState::Off));
    }

    #[test]
    fn the_conversation_voice_is_continued() {
        let mut core = Core { off_after: 2, ..Core::new(on()) };
        core.conversations.push(summary("v7", voice_title(), "ai:ollama@localhost:11434", 11, 6, false));
        let run = run(core, &["utterance.wav", "silence.wav"], Some(2), "Hey Torro, any mail from Anna?");
        let Request::SendMessage { conversation, .. } = messages(&run.sent)[0] else { unreachable!() };
        assert_eq!(conversation.as_deref(), Some("v7"));
        assert!(!run.sent.iter().any(|request| matches!(request, Request::RenameConversation { .. })));
    }

    #[test]
    fn answers_are_not_spoken_when_speech_is_off() {
        let core = Core { off_after: 2, ..Core::new(Voice { speak_answers: false, ..on() }) };
        let run = run(core, &["utterance.wav", "silence.wav"], Some(2), "Hey Torro, what is on today?");
        assert_eq!(messages(&run.sent).len(), 1);
        assert!(run.said.is_empty());
    }

    #[test]
    fn without_the_wake_word_nothing_is_sent() {
        let core = Core { off_after: 3, ..Core::new(on()) };
        let run = run(core, &["utterance.wav"], None, "What is on today?");
        assert_eq!(run.ended, Ok(Ended::RecordingStopped));
        assert!(messages(&run.sent).is_empty());
        assert_eq!(run.chimes, 0);
    }

    #[test]
    fn approvals_are_never_decided_by_voice() {
        let mut core = Core { off_after: 2, ..Core::new(on()) };
        core.answer = vec![EntryContent::Approval { id: "ap1".into() }];
        let run = run(core, &["utterance.wav", "silence.wav"], Some(2), "Hey Torro. Allow. Yes, allow it.");
        assert_eq!(messages(&run.sent).len(), 1, "the words went to the assistant as a message, nothing else");
        for request in &run.sent {
            let name = request.operation().name();
            assert!(
                CoreAssistant::<fn(Request) -> Result<Response, Refusal>, fn(&str)>::REQUESTS.contains(&name),
                "{name}"
            );
            assert!(!matches!(request, Request::DecideApproval { .. }));
        }
        assert_eq!(run.said, [tr("The assistant needs your approval. Please look at the window.")]);
        assert_eq!(run.windows.len(), 1, "the card is in the window, for a click");

        // Nor could it: whatever is not on its list never leaves the listener.
        let sent = Rc::new(RefCell::new(0));
        let counted = Rc::clone(&sent);
        let mut assistant = CoreAssistant::new(
            move |_| {
                *counted.borrow_mut() += 1;
                Ok(Response::Ok)
            },
            |_: &str| {},
            Duration::ZERO,
            Duration::ZERO,
        );
        for request in [
            Request::DecideApproval { id: "ap1".into(), allow: true },
            Request::SetSetting { key: torro_ipc::SettingKey::VoiceEnabled, value: serde_json::json!(true) },
        ] {
            let refused = assistant.request(request);
            assert!(
                matches!(refused, Err(VoiceError::Core(ref refusal)) if refusal.code == "forbidden"),
                "{refused:?}"
            );
        }
        assert_eq!(*sent.borrow(), 0);
    }

    #[test]
    fn a_changed_microphone_builds_the_parts_anew() {
        // The parts were built for the default microphone; the core now names another.
        let core = Rc::new(RefCell::new(Core::new(Voice { microphone: Some("USB".into()), ..on() })));
        let ask_core = Rc::clone(&core);
        let mut assistant = CoreAssistant::new(
            move |request| ask_core.borrow_mut().answer(request),
            |_: &str| {},
            Duration::ZERO,
            Duration::ZERO,
        );
        let opened = Rc::new(RefCell::new(0));
        let mut listener = Listener {
            recorder: Box::new(Files { files: VecDeque::new(), opened: Rc::clone(&opened) }),
            wake: Box::new(WakeAt { at: None, seen: 0 }),
            transcriber: Box::new(Says("")),
            speaker: Box::new(Mouth(Rc::default(), Rc::default())),
            assistant: &mut assistant,
            report: Box::new(|_| {}),
            limits: Limits::default(),
            check_every: 10,
        };
        assert_eq!(listener.run(&on()), Ok(Ended::Changed));
        assert_eq!(*opened.borrow(), 0);
    }

    #[test]
    fn engines_come_from_the_last_conversation_or_the_first_that_works() {
        assert_eq!(last_engine(&[]), None);
        let engines = vec![
            Engine { id: "ai:a".into(), kind: EngineKind::Ai, name: "A".into(), problem: Some("no key".into()) },
            Engine { id: "ai:b".into(), kind: EngineKind::Ai, name: "B".into(), problem: None },
        ];
        assert_eq!(first_engine(&engines).as_deref(), Some("ai:b"));
        assert_eq!(first_engine(&engines[..1]).as_deref(), Some("ai:a"));
        assert_eq!(first_engine(&[]), None);
    }

    #[test]
    fn answers_are_read_without_markdown_and_not_too_long() {
        assert_eq!(speakable("# Today\n- **Standup** at `09:30`\n- Lunch"), "Today Standup at 09:30 Lunch");
        assert_eq!(speakable("See [the plan](https://example.org/plan)."), "See the plan.");
        let long = "One sentence. ".repeat(100);
        let spoken = speakable(&long);
        assert!(spoken.chars().count() <= MOST_SPOKEN && spoken.ends_with('.'));
        assert_eq!(Reply::default().spoken(), None);
        let failed = Reply { failed: Some("offline".into()), ..Reply::default() };
        assert_eq!(failed.spoken().as_deref(), Some(tr("The assistant could not answer.")));
    }
}
