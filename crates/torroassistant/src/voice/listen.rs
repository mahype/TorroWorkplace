//! The listener's loop ([`Listener`]): wait for the wake word, play the tone, record until a
//! pause, turn it into text, send it ([`Assistant::send`]), have the answer read aloud, and again –
//! while the setting `voice.enabled` stays on or a conversation by voice runs. The microphone is
//! closed while anything is read aloud, whoever asked for it, so no voice from the speakers is taken
//! for the person's words or wakes it.
//!
//! A conversation by voice (issue #747, `assistant.start_talk`): after "Hey Torro" and its answer, or
//! with "Talk" in the chat window, the microphone stays open after each answer, and
//! the person goes on without the wake word ([`Listener::converse`]), however long they think. It ends
//! with a long silence ([`TALK_WAIT_SECONDS`], a guard only), a short
//! closing sentence ("Danke", "Stopp", [`is_closing`]), "End talk" in the window, or an answer that
//! waits for an approval – that is given in the window only.
//!
//! [`CoreAssistant`] is the way to the core: it sends the sentence as `assistant.send_message`
//! (`spoken`, so the answer is short) to the conversation of the conversation by voice with the model
//! chosen there (issue #852), else to "Voice" with the chat model of the settings, the one used last
//! or the first engine ([`CoreAssistant::target`]), opens the quick window, has the answer read aloud sentence by
//! sentence (`assistant.speak_answer`) and follows it with `assistant.get_conversation`. It sends
//! nothing else: no request it builds can decide an approval, so no word spoken ever allows anything
//! ([`CoreAssistant::REQUESTS`]).

use std::time::{Duration, Instant};

use torro_core::ai::Effort;
use torro_core::settings::Voice;
use torro_ipc::assistant::{ConversationSummary, Engine, Entry, EntryContent};
use torro_ipc::{Request, Response, Talk, VoiceState};

use super::audio::{Endpoint, FRAMES_PER_SECOND, Limits, NoiseFloor, rms};
use super::wake::strip_wake_phrase;
use super::{Recorder, Speaker, Transcriber, VoiceError, WakeWord};
use crate::backend::Refusal;

/// What the listener asks of the core.
pub trait Assistant {
    /// The voice settings as the core holds them now (`settings.get_settings`).
    fn settings(&mut self) -> Result<Voice, VoiceError>;

    /// The conversation by voice and whether anything is read aloud (`assistant.talk_status`).
    fn talk(&mut self) -> Result<TalkNow, VoiceError>;

    /// Sends the spoken `text` to `to` (the conversation by voice's, `Some(None)` for a new one), or
    /// with `None` to the conversation "Voice"; answers where it went.
    fn send(&mut self, text: &str, to: Option<Option<String>>) -> Result<Sent, VoiceError>;

    /// Starts the conversation by voice in `conversation` (`assistant.start_talk`), or attaches it there.
    fn start_talk(&mut self, conversation: &str) -> Result<(), VoiceError>;

    /// Ends the conversation by voice (`assistant.end_talk`).
    fn end_talk(&mut self) -> Result<(), VoiceError>;

    /// Shows `conversation` in the quick window.
    fn open(&mut self, conversation: &str);

    /// Reads the answer of `conversation` aloud sentence by sentence (`assistant.speak_answer`),
    /// returning once it was read.
    fn speak_answer(&mut self, conversation: &str) -> Result<(), VoiceError>;

    /// Waits for the answer to what was `sent`, until it is over or waits for an approval.
    fn follow(&mut self, sent: &Sent) -> Result<Reply, VoiceError>;

    /// Whether an answer runs in `conversation` now; a conversation gone runs none.
    fn running(&mut self, conversation: &str) -> Result<bool, VoiceError>;
}

/// The conversation by voice as the core has it (`assistant.talk_status`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TalkNow {
    /// The conversation by voice that runs.
    pub talk: Option<Talk>,
    /// Anything is read aloud now, whoever asked for it.
    pub speaking: bool,
    /// How many readings aloud began: one that began and ended while a sentence was recorded
    /// changes it, though `speaking` is over by then.
    pub readings: u64,
}

/// Where a spoken sentence went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The conversation.
    pub conversation: String,
    /// Its last stored entry before the answer.
    pub after: u64,
}

/// The listener's half of a timed round (issue #828): how long the pause that ended the sentence
/// was and how long speech recognition took, for the message `after` in `conversation`. The core adds
/// the first word and the first sound once the answer was read. Only durations, never text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heard {
    pub conversation: String,
    pub after: u64,
    pub pause_ms: u32,
    pub recognize_ms: u32,
}

/// `frames` of the microphone as milliseconds.
fn frames_ms(frames: usize) -> u32 {
    u32::try_from(frames.saturating_mul(1000) / FRAMES_PER_SECOND).unwrap_or(u32::MAX)
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

/// [`super::speech`]'s text as it is read aloud: without marks (`**`, `#`, `` ` ``, list dashes),
/// links as their text, on one line, at most `most` characters, ending at a sentence where it is cut.
pub fn speakable_up_to(markdown: &str, most: usize) -> String {
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
    if line.chars().count() <= most {
        return line;
    }
    let cut: String = line.chars().take(most).collect();
    match cut.rfind(['.', '!', '?']) {
        Some(end) => cut[..=end].to_string(),
        None => format!("{cut}…"),
    }
}

/// How long a conversation by voice waits in silence for the next sentence before it ends (issue
/// #849), in seconds. Long, so thinking aloud or a pause does not end it; the limit only guards a
/// microphone left open and forgotten.
const TALK_WAIT_SECONDS: usize = 300;

/// Words that end a conversation by voice (issue #747, decision 4), German and English, lower case.
pub const CLOSING: [&str; 14] = [
    "danke",
    "stopp",
    "tschüss",
    "ende",
    "das war's",
    "das wars",
    "thanks",
    "thank you",
    "stop",
    "bye",
    "goodbye",
    "end",
    "that's all",
    "that's it",
];

/// Whether `text` closes a conversation by voice (issue #747, decision 4): at most three words, and
/// one of [`CLOSING`] among them. "Danke, und das Wetter?" goes on.
pub fn is_closing(text: &str) -> bool {
    let lower = text.to_lowercase().replace('’', "'");
    let words: Vec<&str> =
        lower.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|word| !word.is_empty()).collect();
    if words.is_empty() || words.len() > 3 {
        return false;
    }
    let said = format!(" {} ", words.join(" "));
    CLOSING.iter().any(|closing| said.contains(&format!(" {closing} ")))
}

/// How one round of listening ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The setting `voice.enabled` went off and no conversation by voice runs, or the core is gone:
    /// stop.
    VoiceOff,
    /// Another voice setting changed (microphone, model): build the parts anew.
    Changed,
    /// A sentence was sent and answered.
    Answered,
    /// The wake word, but nothing understood, or the round gave way: listen again.
    Nothing,
    /// In a conversation by voice, noise that came to no text (issue #849): it goes on, and the
    /// microphone opens again without a tone.
    Unheard,
    /// The recording program stopped.
    RecordingStopped,
}

/// What a look at the core says ([`Listener::check`]).
enum Look {
    /// Stop the round with this.
    Stop(Ended),
    /// Go on; the conversation by voice as it stands.
    Go(TalkNow),
}

/// The listener (see the module) on its parts.
pub struct Listener<'a, A: Assistant> {
    pub recorder: Box<dyn Recorder>,
    pub wake: Box<dyn WakeWord>,
    pub transcriber: Box<dyn Transcriber>,
    /// Plays the tone when the wake word was heard and when a conversation by voice listens or ends.
    pub speaker: Box<dyn Speaker>,
    pub assistant: &'a mut A,
    /// Told every change of state (the core reads it to show it in the tray).
    pub report: Box<dyn FnMut(VoiceState) + 'a>,
    /// Told the listener's half of each round whose answer is read aloud ([`Heard`], issue #828).
    pub heard: Box<dyn FnMut(Heard) + 'a>,
    pub limits: Limits,
    /// How many frames go by between two looks at the settings and the conversation by voice (about
    /// half a second).
    pub check_every: usize,
    /// How long to wait between two looks while something is read aloud, and after it, so its echo
    /// fades before the microphone opens again (300 ms).
    pub pause: Duration,
    /// How loud the room is, from the last wait for the wake word: a conversation by voice records
    /// without one.
    pub floor: NoiseFloor,
}

impl<A: Assistant> Listener<'_, A> {
    /// Listens until voice is switched off with no conversation by voice running, another voice
    /// setting changes or the recording stops. `voice` are the settings the parts were built for.
    /// Nothing is recorded when voice is off and no conversation by voice runs. Fails when the
    /// microphone cannot be opened.
    pub fn run(&mut self, voice: &Voice) -> Result<Ended, VoiceError> {
        let mut chime = true;
        loop {
            let ended = self.listen_once(voice, chime)?;
            chime = ended != Ended::Unheard;
            match ended {
                Ended::Answered | Ended::Nothing | Ended::Unheard => {}
                ended => {
                    (self.report)(VoiceState::Off);
                    return Ok(ended);
                }
            }
        }
    }

    /// One round: in a conversation by voice the next sentence ([`Listener::converse`]); else waits
    /// for the wake word, then records, sends and answers one sentence. While anything is read aloud
    /// the microphone stays closed. Failures of speech to text or of the core are written to the log
    /// and end the round with [`Ended::Nothing`]; only the microphone's fail it.
    ///
    /// `chime` says whether a conversation by voice opens its microphone with the tone (not after
    /// noise that was dropped: the person did not stop talking).
    fn listen_once(&mut self, voice: &Voice, chime: bool) -> Result<Ended, VoiceError> {
        let now = match self.check(voice) {
            Look::Stop(ended) => return Ok(ended),
            Look::Go(now) => now,
        };
        if now.speaking {
            self.quiet();
            return Ok(Ended::Nothing);
        }
        if let Some(talk) = now.talk {
            return self.converse(voice, talk, chime);
        }
        let mut stream = self.recorder.open()?;
        (self.report)(VoiceState::Listening);
        let mut floor = NoiseFloor::default();
        let mut frames = 0usize;
        loop {
            let Some(frame) = stream.frame()? else { return Ok(Ended::RecordingStopped) };
            frames += 1;
            if frames.is_multiple_of(self.check_every.max(1)) {
                match self.check(voice) {
                    Look::Stop(ended) => return Ok(ended),
                    // Something is read aloud, or a conversation by voice began: the microphone
                    // closes, and the next round decides.
                    Look::Go(now) if now.speaking || now.talk.is_some() => {
                        drop(stream);
                        self.wake.reset();
                        return Ok(Ended::Nothing);
                    }
                    Look::Go(_) => {}
                }
            }
            if self.wake.hears(&frame)? {
                break;
            }
            floor.follow(rms(&frame));
        }
        self.floor = floor;
        // Heard while something was read aloud: it may be the reading's voice, not the person's.
        let Some(readings) = self.quiet_now() else {
            drop(stream);
            self.wake.reset();
            self.quiet();
            return Ok(Ended::Nothing);
        };
        (self.report)(VoiceState::Recording);
        self.speaker.chime();
        let mut sentence = Vec::new();
        let mut end = Endpoint::new(floor, self.limits);
        let mut frames = 0usize;
        while let Some(frame) = stream.frame()? {
            frames += 1;
            // A reading that begins meanwhile: what the microphone hears is not the person's alone.
            if frames.is_multiple_of(self.check_every.max(1)) && self.speaking() {
                drop(stream);
                self.wake.reset();
                self.quiet();
                return Ok(Ended::Nothing);
            }
            sentence.extend_from_slice(&frame);
            if end.push(&frame) {
                break;
            }
        }
        // The microphone closes before the answer: it is not recorded, and it cannot wake it.
        drop(stream);
        (self.report)(VoiceState::Recognizing);
        let ended = self.answer(voice, &sentence, end.heard_speech(), end.trailing_quiet(), None, readings);
        self.wake.reset();
        Ok(ended)
    }

    /// The next sentence of the conversation by voice `talk` (issue #747): the tone, then up to
    /// [`TALK_WAIT_SECONDS`] for the person to speak, until a pause. Only a long silence ends the
    /// conversation, with a tone. Something read aloud meanwhile, or the conversation ended or moved elsewhere,
    /// closes the microphone, and what was heard is dropped.
    fn converse(&mut self, voice: &Voice, talk: Talk, chime: bool) -> Result<Ended, VoiceError> {
        // An answer still runs there (one longer than the listener waited, or a talk started while it
        // ran): the conversation would refuse the sentence, so the microphone waits for it.
        if let Some(id) = &talk.conversation
            && self.assistant.running(id).unwrap_or(false)
        {
            std::thread::sleep(self.pause);
            return Ok(Ended::Nothing);
        }
        // The tone before the microphone opens: it is no sentence of the person's.
        if chime {
            self.speaker.chime();
        }
        let Some(readings) = self.quiet_now() else {
            self.quiet();
            return Ok(Ended::Nothing);
        };
        let mut stream = self.recorder.open()?;
        (self.report)(VoiceState::Conversing);
        // The wait for the first word comes on top of the longest sentence.
        let wait = TALK_WAIT_SECONDS * FRAMES_PER_SECOND;
        let limits = Limits { wait, most: wait + self.limits.most, ..self.limits };
        let mut end = Endpoint::new(self.floor, limits);
        let (mut sentence, mut frames, mut recording, mut ended) = (Vec::new(), 0usize, false, false);
        while let Some(frame) = stream.frame()? {
            frames += 1;
            if frames.is_multiple_of(self.check_every.max(1)) {
                match self.check(voice) {
                    Look::Stop(ended) => return Ok(ended),
                    Look::Go(now) if now.speaking => {
                        drop(stream);
                        self.quiet();
                        return Ok(Ended::Nothing);
                    }
                    Look::Go(now) if now.talk.as_ref().map(|now| &now.conversation) != Some(&talk.conversation) => {
                        return Ok(Ended::Nothing);
                    }
                    Look::Go(_) => {}
                }
            }
            sentence.extend_from_slice(&frame);
            ended = end.push(&frame);
            if !recording && end.heard_speech() {
                recording = true;
                (self.report)(VoiceState::Recording);
            }
            if ended {
                break;
            }
        }
        drop(stream);
        // The recording program stopped before the sentence or the wait was over: no silence, a
        // failure – the conversation by voice ends, and the listener says why.
        if !ended {
            if let Err(err) = self.assistant.end_talk() {
                eprintln!("torroassistant: {err}");
            }
            return Ok(Ended::RecordingStopped);
        }
        if !end.heard_speech() {
            self.close_talk();
            return Ok(Ended::Nothing);
        }
        (self.report)(VoiceState::Recognizing);
        Ok(self.answer(voice, &sentence, true, end.trailing_quiet(), Some(talk), readings))
    }

    /// Looks at the settings and the conversation by voice: stop ([`Ended::VoiceOff`]) when voice is
    /// off and no conversation by voice runs, build the parts anew ([`Ended::Changed`]) when another
    /// voice setting changed, else go on. A core that cannot be asked counts as off: without it, the
    /// person could not switch the microphone off.
    fn check(&mut self, voice: &Voice) -> Look {
        let settings = match self.assistant.settings() {
            Ok(settings) => settings,
            Err(err) => {
                eprintln!("torroassistant: stops listening: {err}");
                return Look::Stop(Ended::VoiceOff);
            }
        };
        let now = match self.assistant.talk() {
            Ok(now) => now,
            Err(err) => {
                eprintln!("torroassistant: stops listening: {err}");
                return Look::Stop(Ended::VoiceOff);
            }
        };
        if !settings.enabled && now.talk.is_none() {
            return Look::Stop(Ended::VoiceOff);
        }
        // Whether "Hey Torro" is on needs no new parts: a conversation by voice listens either way.
        let (now_parts, built_for) = (Voice { enabled: true, ..settings }, Voice { enabled: true, ..voice.clone() });
        if now_parts != built_for {
            return Look::Stop(Ended::Changed);
        }
        Look::Go(now)
    }

    /// Whether anything is read aloud now; a core that cannot be asked reads nothing.
    fn speaking(&mut self) -> bool {
        self.assistant.talk().is_ok_and(|now| now.speaking)
    }

    /// How many readings began, when nothing is read aloud now; `None` while something is. A core
    /// that cannot be asked counts as quiet, with no reading.
    fn quiet_now(&mut self) -> Option<u64> {
        match self.assistant.talk() {
            Ok(now) if now.speaking => None,
            Ok(now) => Some(now.readings),
            Err(_) => Some(0),
        }
    }

    /// Waits until nothing is read aloud any more, then a little longer, so the room is quiet before
    /// the microphone opens again.
    fn quiet(&mut self) {
        while self.speaking() {
            std::thread::sleep(self.pause);
        }
        std::thread::sleep(self.pause);
    }

    /// Ends the conversation by voice, with the tone first: with "Hey Torro" off the core ends the
    /// listener with it.
    fn close_talk(&mut self) {
        self.speaker.chime();
        if let Err(err) = self.assistant.end_talk() {
            eprintln!("torroassistant: {err}");
        }
    }

    /// Turns `sentence` into text and sends it: in the conversation by voice `talk`, else (after the
    /// wake word) to "Voice", where a conversation by voice then starts and the quick window shows it.
    /// The answer is read aloud if `voice` says so; one that waits for an approval ends the
    /// conversation by voice (decision 5). A closing sentence ends it without sending. `readings` were
    /// begun when the microphone opened: a reading that began since, even one already over, drops the
    /// sentence. `quiet` frames of pause ended the sentence; with the time speech recognition took they
    /// go to [`Listener::heard`] once the sentence was sent, if the answer is read aloud.
    fn answer(
        &mut self,
        voice: &Voice,
        sentence: &[i16],
        spoken: bool,
        quiet: usize,
        talk: Option<Talk>,
        readings: u64,
    ) -> Ended {
        if !spoken {
            return Ended::Nothing;
        }
        let recognizing = Instant::now();
        let transcribed = self.transcriber.transcribe(sentence);
        let recognize_ms = u32::try_from(recognizing.elapsed().as_millis()).unwrap_or(u32::MAX);
        let text = match transcribed {
            Ok(text) => strip_wake_phrase(&text),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return Ended::Nothing;
            }
        };
        // Noise in a pause comes to no text: it is dropped and the conversation by voice goes on.
        if text.is_empty() {
            return if talk.is_some() { Ended::Unheard } else { Ended::Nothing };
        }
        if is_closing(&text) {
            if talk.is_some() {
                self.close_talk();
            }
            return Ended::Nothing;
        }
        // Read aloud while it was said, or still: it may be the reading's voice, so it is dropped.
        if self.quiet_now() != Some(readings) {
            self.quiet();
            return Ended::Nothing;
        }
        // Recognized and worth sending (issue #830): the core says more precisely what happens from here.
        (self.report)(VoiceState::Answering);
        let to = talk.as_ref().map(|talk| talk.conversation.clone());
        let sent = match self.assistant.send(&text, to) {
            Ok(sent) => sent,
            Err(err) => {
                eprintln!("torroassistant: {err}");
                // Its conversation is gone (deleted, cleaned up): the conversation by voice ends.
                if talk.is_some() && matches!(&err, VoiceError::Core(refusal) if refusal.code == "not-found") {
                    self.close_talk();
                }
                return Ended::Nothing;
            }
        };
        if talk.as_ref().is_none_or(|talk| talk.conversation.is_none())
            && let Err(err) = self.assistant.start_talk(&sent.conversation)
        {
            eprintln!("torroassistant: {err}");
        }
        // The window opens for the wake word; one with "Talk" shows the conversation already.
        if talk.is_none() {
            self.assistant.open(&sent.conversation);
        }
        if voice.speak_answers {
            // Only a round read aloud is timed (decision 1): without a first sound it is half a round.
            (self.heard)(Heard {
                conversation: sent.conversation.clone(),
                after: sent.after,
                pause_ms: frames_ms(quiet),
                recognize_ms,
            });
            if let Err(err) = self.assistant.speak_answer(&sent.conversation) {
                eprintln!("torroassistant: cannot speak the answer: {err}");
            }
            // The room's echo of the answer fades before the microphone opens again.
            std::thread::sleep(self.pause);
        }
        match self.assistant.follow(&sent) {
            Ok(reply) if reply.approval => {
                if let Err(err) = self.assistant.end_talk() {
                    eprintln!("torroassistant: {err}");
                }
            }
            Ok(_) => {}
            Err(err) => eprintln!("torroassistant: {err}"),
        }
        Ended::Answered
    }
}

/// The conversation spoken messages go to.
pub fn voice_title() -> &'static str {
    torro_core::i18n::tr("Voice")
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
    /// (`approvals.decide_approval`) or switches voice on: tests check every request against it. It
    /// opens the microphone only for `assistant.start_talk`, which goes on with a conversation the
    /// person began by voice (issue #747).
    pub const REQUESTS: [&'static str; 10] = [
        "settings.get_settings",
        "assistant.list_conversations",
        "assistant.list_engines",
        "assistant.send_message",
        "assistant.rename_conversation",
        "assistant.get_conversation",
        "assistant.start_talk",
        "assistant.end_talk",
        "assistant.talk_status",
        "assistant.speak_answer",
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

    /// `request`, expecting [`Response::Ok`].
    fn done(&mut self, request: Request) -> Result<(), VoiceError> {
        match self.request(request)? {
            Response::Ok => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    /// The conversation "Voice" if there is one (or `conversation` when given), and who answers: in a
    /// conversation given, the engine, model and effort it names (chosen in the window or used last, issue
    /// #852); else the chat model of the settings while its engine is offered, else the one used last in any
    /// conversation, else the first engine. The settings come before "used last": they are the person's
    /// own choice.
    fn target(&mut self, conversation: Option<&str>) -> Result<Target, VoiceError> {
        let conversations = match self.request(Request::ListConversations)? {
            Response::Conversations { conversations } => conversations,
            other => return Err(unexpected(&other)),
        };
        let found = match conversation {
            Some(id) => conversations.iter().find(|summary| summary.id == id),
            None => conversations.iter().find(|summary| summary.title == voice_title()),
        };
        let id = found.map(|summary| summary.id.clone());
        if let Some(summary) = found.filter(|_| conversation.is_some()) {
            return Ok((id, summary.engine.clone(), summary.model.clone(), summary.effort));
        }
        let slot = match self.request(Request::Settings)? {
            Response::Settings { settings } => settings.ai_slots.chat,
            other => return Err(unexpected(&other)),
        };
        let engines = match self.request(Request::ListEngines)? {
            Response::Engines { engines } => engines,
            other => return Err(unexpected(&other)),
        };
        if let Some(slot) = slot.filter(|slot| engines.iter().any(|engine| engine.id == slot.engine)) {
            return Ok((id, slot.engine, slot.model, slot.effort));
        }
        if let Some((engine, model, effort)) = last_engine(&conversations) {
            return Ok((id, engine, model, effort));
        }
        let engine = first_engine(&engines).ok_or_else(|| {
            VoiceError::Core(Refusal::new("not-found", "no engine: add an AI account in Torro Workplace"))
        })?;
        Ok((id, engine, None, None))
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

/// Where [`CoreAssistant::target`] sends a sentence: the conversation (`None` for a new one), the engine,
/// the model and the effort.
type Target = (Option<String>, String, Option<String>, Option<Effort>);

/// The engine, model and effort of the conversation used last.
fn last_engine(conversations: &[ConversationSummary]) -> Option<(String, Option<String>, Option<Effort>)> {
    let last = conversations.iter().max_by_key(|conversation| conversation.updated)?;
    Some((last.engine.clone(), last.model.clone(), last.effort))
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

    fn talk(&mut self) -> Result<TalkNow, VoiceError> {
        match self.request(Request::TalkStatus)? {
            Response::Talk { talk, speaking, readings } => Ok(TalkNow { talk, speaking, readings }),
            other => Err(unexpected(&other)),
        }
    }

    fn send(&mut self, text: &str, to: Option<Option<String>>) -> Result<Sent, VoiceError> {
        let (conversation, engine, model, effort) = match &to {
            // A new conversation for a conversation by voice in a new chat: as for "Voice".
            Some(None) => {
                let (_, engine, model, effort) = self.target(Some(""))?;
                (None, engine, model, effort)
            }
            Some(Some(id)) => self.target(Some(id))?,
            None => self.target(None)?,
        };
        // A conversation by voice in a conversation gone meanwhile: nothing goes elsewhere.
        if let Some(Some(id)) = &to
            && conversation.is_none()
        {
            return Err(VoiceError::Core(Refusal::new("not-found", format!("no conversation {id:?}"))));
        }
        let new = conversation.is_none();
        let send = Request::SendMessage {
            conversation,
            engine,
            model,
            effort,
            text: text.to_string(),
            without: Vec::new(),
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
            about: None,
            spoken: true,
        };
        let (id, after) = match self.request(send)? {
            Response::Conversation { conversation, .. } => (conversation.id, conversation.last),
            other => return Err(unexpected(&other)),
        };
        // Only "Voice" gets its name; a new chat of "Talk" is named by its first sentence.
        if new && to.is_none() {
            let rename = Request::RenameConversation { id: id.clone(), title: voice_title().to_string() };
            if let Err(err) = self.request(rename) {
                eprintln!("torroassistant: cannot name the conversation: {err}");
            }
        }
        Ok(Sent { conversation: id, after })
    }

    fn start_talk(&mut self, conversation: &str) -> Result<(), VoiceError> {
        match self.request(Request::StartTalk { conversation: Some(conversation.to_string()) })? {
            Response::Talk { .. } => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    fn end_talk(&mut self) -> Result<(), VoiceError> {
        self.done(Request::EndTalk)
    }

    fn open(&mut self, conversation: &str) {
        (self.open)(conversation);
    }

    fn speak_answer(&mut self, conversation: &str) -> Result<(), VoiceError> {
        self.done(Request::SpeakAnswer { conversation: conversation.to_string() })
    }

    fn running(&mut self, conversation: &str) -> Result<bool, VoiceError> {
        match self.request(Request::GetConversation { id: conversation.to_string(), after: Some(u64::MAX) })? {
            Response::Conversation { conversation, .. } => Ok(conversation.running),
            _ => Ok(false),
        }
    }

    fn follow(&mut self, sent: &Sent) -> Result<Reply, VoiceError> {
        let mut after = sent.after;
        // Entries by number: a growing answer comes again under the same one.
        let mut entries: Vec<Entry> = Vec::new();
        let until = Instant::now() + self.wait;
        loop {
            let get = Request::GetConversation { id: sent.conversation.clone(), after: Some(after) };
            let (summary, new_entries) = match self.request(get)? {
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
    use torro_core::i18n::tr;
    use torro_core::settings::Settings;
    use torro_ipc::assistant::EngineKind;

    use super::super::Stream;
    use super::super::audio::FRAME;
    use super::super::audio::tests::fixture;
    use super::*;

    /// The "file" of ten seconds of silence: a conversation by voice ends in it.
    const PAUSE: &str = "pause";

    /// The "file" of a minute of silence, then a sentence.
    const THINKING: &str = "thinking";

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
            // Longer than a conversation by voice waits: silence that ends it, not a stopped program.
            if name == PAUSE {
                let frames = (TALK_WAIT_SECONDS + 2) * super::super::audio::FRAMES_PER_SECOND;
                return Ok(Box::new(Frames((0..frames).map(|_| vec![0; FRAME]).collect())));
            }
            // A minute of thinking before the sentence: far longer than the old eight seconds (#849).
            if name == THINKING {
                let quiet = (0..60 * super::super::audio::FRAMES_PER_SECOND).map(|_| vec![0; FRAME]);
                let said = fixture("utterance.wav").chunks(FRAME).map(<[i16]>::to_vec).collect::<Vec<_>>();
                return Ok(Box::new(Frames(quiet.chain(said).collect())));
            }
            Ok(Box::new(Frames(fixture(name).chunks(FRAME).map(<[i16]>::to_vec).collect())))
        }
    }

    /// Hears the wake word once, at the `at`-th frame of a round.
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

    /// Understands the sentences in order, the last one again and again.
    struct Says(VecDeque<&'static str>);

    impl Transcriber for Says {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            assert!(!samples.is_empty());
            let text = if self.0.len() > 1 { self.0.pop_front() } else { self.0.front().copied() };
            Ok(text.unwrap_or_default().to_string())
        }
    }

    /// Counts its tones.
    struct Tone(Rc<RefCell<usize>>);

    impl Speaker for Tone {
        fn chime(&mut self) {
            *self.0.borrow_mut() += 1;
        }
    }

    /// A core in memory: the settings, the conversations, the conversation by voice, every request
    /// it got.
    struct Core {
        voice: Voice,
        conversations: Vec<ConversationSummary>,
        answer: Vec<EntryContent>,
        talk: Option<Talk>,
        /// Whether something is read aloud, one per look at the conversation by voice; then nothing.
        speaking: VecDeque<bool>,
        /// How many readings began, one per look; then the last one.
        readings: VecDeque<u64>,
        read: u64,
        /// Whether an answer runs, one per look at a conversation without entries asked; then none.
        running: VecDeque<bool>,
        sent: Vec<Request>,
        /// The chat model of the settings (`ai_slots.chat`, issue #852).
        chat: Option<torro_core::ai::SlotModel>,
    }

    fn summary(id: &str, title: &str, engine: &str, day: u32, last: u64, running: bool) -> ConversationSummary {
        let time = Utc.with_ymd_and_hms(2026, 10, day, 9, 0, 0).single().expect("time");
        ConversationSummary {
            id: id.into(),
            title: title.into(),
            engine: engine.into(),
            model: Some(format!("{engine}-model")),
            effort: None,
            created: time,
            updated: time,
            pinned: false,
            archived: false,
            running,
            last,
            bot: None,
            bot_deleted: false,
            about: None,
        }
    }

    impl Core {
        fn new(voice: Voice) -> Core {
            Core {
                voice,
                conversations: vec![
                    summary("a1", "Plan for tomorrow", "ai:ollama@localhost:11434", 12, 4, false),
                    summary("b2", "Trip", "ai:openrouter", 13, 2, false),
                ],
                answer: vec![EntryContent::Answer {
                    text: "You have **two** events today: [Standup](https://x) and lunch.".into(),
                    engine: "ai:openrouter".into(),
                    model: None,
                    spoken: None,
                }],
                talk: None,
                speaking: VecDeque::new(),
                readings: VecDeque::new(),
                read: 0,
                running: VecDeque::new(),
                sent: Vec::new(),
                chat: None,
            }
        }

        fn answer(&mut self, request: Request) -> Result<Response, Refusal> {
            self.sent.push(request.clone());
            let entry = |seq, content| Entry { seq, time: Utc::now(), content };
            match request {
                Request::Settings => {
                    let mut settings = Settings { voice: self.voice.clone(), ..Settings::default() };
                    settings.ai_slots.chat = self.chat.clone();
                    Ok(Response::Settings { settings })
                }
                Request::TalkStatus => {
                    let speaking = self.speaking.pop_front().unwrap_or(false);
                    let readings = self.readings.pop_front().unwrap_or(self.read);
                    self.read = readings;
                    Ok(Response::Talk { talk: self.talk.clone(), speaking, readings })
                }
                Request::StartTalk { conversation } => {
                    self.talk = Some(Talk { conversation, state: VoiceState::Conversing, tool: None });
                    Ok(Response::Talk { talk: self.talk.clone(), speaking: false, readings: self.read })
                }
                Request::EndTalk => {
                    self.talk = None;
                    Ok(Response::Ok)
                }
                Request::SpeakAnswer { .. } => Ok(Response::Ok),
                Request::ListConversations => Ok(Response::Conversations { conversations: self.conversations.clone() }),
                Request::ListEngines => Ok(Response::Engines {
                    engines: vec![
                        Engine {
                            id: "test".into(),
                            kind: EngineKind::Test,
                            name: "Test".into(),
                            problem: None,
                            models: Vec::new(),
                        },
                        Engine {
                            id: "ai:openai".into(),
                            kind: EngineKind::Ai,
                            name: "ChatGPT".into(),
                            problem: None,
                            models: Vec::new(),
                        },
                    ],
                }),
                Request::SendMessage { conversation, engine, text, model, .. } => {
                    let id = conversation.unwrap_or_else(|| "c9".into());
                    let conversation = ConversationSummary { model, ..summary(&id, &text, &engine, 14, 1, true) };
                    if !self.conversations.iter().any(|known| known.id == id) {
                        self.conversations.push(conversation.clone());
                    }
                    let message = EntryContent::Message {
                        text,
                        engine,
                        model: None,
                        effort: None,
                        without: Vec::new(),
                        mode: torro_ipc::assistant::Mode::Default,
                        from: None,
                    };
                    Ok(Response::Conversation { conversation, entries: vec![entry(1, message)] })
                }
                Request::RenameConversation { .. } => Ok(Response::Ok),
                Request::GetConversation { id, after: Some(u64::MAX) } => {
                    let running = self.running.pop_front().unwrap_or(false);
                    Ok(Response::Conversation {
                        conversation: summary(&id, "Voice", "ai:openrouter", 14, 1, running),
                        entries: Vec::new(),
                    })
                }
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
        opened: usize,
        chimes: usize,
        windows: Vec<String>,
        states: Vec<VoiceState>,
        talk: Option<Talk>,
        heard: Vec<Heard>,
        ended: Result<Ended, VoiceError>,
    }

    /// The listener on `files`, waking at frame `wake`, hearing `heard` in order, with the core `core`.
    fn run(core: Core, files: &[&'static str], wake: Option<usize>, heard: &[&'static str]) -> Run {
        let core = Rc::new(RefCell::new(core));
        let windows = Rc::new(RefCell::new(Vec::new()));
        let (ask_core, shown) = (Rc::clone(&core), Rc::clone(&windows));
        let mut assistant = CoreAssistant::new(
            move |request| ask_core.borrow_mut().answer(request),
            move |id: &str| shown.borrow_mut().push(id.to_string()),
            Duration::ZERO,
            Duration::from_secs(5),
        );
        let (chimes, opened) = (Rc::new(RefCell::new(0)), Rc::new(RefCell::new(0)));
        let states = Rc::new(RefCell::new(Vec::new()));
        let voice = core.borrow().voice.clone();
        let reported = Rc::clone(&states);
        let told = Rc::new(RefCell::new(Vec::new()));
        let rounds = Rc::clone(&told);
        let mut listener = Listener {
            recorder: Box::new(Files { files: files.iter().copied().collect(), opened: Rc::clone(&opened) }),
            wake: Box::new(WakeAt { at: wake, seen: 0 }),
            transcriber: Box::new(Says(heard.iter().copied().collect())),
            speaker: Box::new(Tone(Rc::clone(&chimes))),
            assistant: &mut assistant,
            report: Box::new(move |state| reported.borrow_mut().push(state)),
            heard: Box::new(move |round| rounds.borrow_mut().push(round)),
            limits: Limits::default(),
            check_every: 10,
            pause: Duration::ZERO,
            floor: NoiseFloor::default(),
        };
        let ended = listener.run(&voice);
        drop(listener);
        drop(assistant);
        let core = core.borrow();
        Run {
            sent: core.sent.clone(),
            opened: *opened.borrow(),
            chimes: *chimes.borrow(),
            windows: windows.borrow().clone(),
            states: states.borrow().clone(),
            talk: core.talk.clone(),
            heard: told.borrow().clone(),
            ended,
        }
    }

    fn on() -> Voice {
        Voice { enabled: true, ..Voice::default() }
    }

    fn messages(sent: &[Request]) -> Vec<&Request> {
        sent.iter().filter(|request| matches!(request, Request::SendMessage { .. })).collect()
    }

    fn count(sent: &[Request], wanted: &Request) -> usize {
        sent.iter().filter(|request| *request == wanted).count()
    }

    fn talk_in(conversation: Option<&str>) -> Option<Talk> {
        Some(Talk { conversation: conversation.map(str::to_string), state: VoiceState::Conversing, tool: None })
    }

    /// Issue #828: a sentence whose answer is read aloud tells its half of the round once, for the
    /// message it went to; without "Speak answers" nothing is timed.
    #[test]
    fn a_round_read_aloud_tells_its_pause_and_recognition() {
        let read = run(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &[" Hey Torro, what is on today?"]);
        assert_eq!(read.heard.len(), 1, "{:?}", read.heard);
        let heard = &read.heard[0];
        assert_eq!((heard.conversation.as_str(), heard.after), ("c9", 1), "the message the send answered");
        assert_eq!(heard.pause_ms, 1000, "the pause of a second ended the sentence");

        let quiet = Voice { speak_answers: false, ..on() };
        let silent = run(Core::new(quiet), &["utterance.wav", PAUSE], Some(2), &[" Hey Torro, what is on today?"]);
        assert!(messages(&silent.sent).len() == 1 && silent.heard.is_empty(), "sent, but not timed");
    }

    /// Issue #830: a sentence that comes to no text is recognized, but nothing is sent, so the listener never
    /// says it answers.
    #[test]
    fn a_sentence_without_text_is_recognized_but_not_answered() {
        let run = run(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &[""]);
        assert!(messages(&run.sent).is_empty());
        let recognizing = run.states.iter().position(|state| *state == VoiceState::Recognizing);
        assert_eq!(recognizing, Some(2), "{:?}", run.states);
        assert!(!run.states.contains(&VoiceState::Answering), "{:?}", run.states);
    }

    #[test]
    fn nothing_is_recorded_while_voice_is_off() {
        let run = run(Core::new(Voice::default()), &["utterance.wav"], Some(2), &["Hey Torro, what is on?"]);
        assert_eq!(run.ended, Ok(Ended::VoiceOff));
        assert_eq!(run.opened, 0, "the microphone was never opened");
        assert!(messages(&run.sent).is_empty());
        assert_eq!(run.states, [VoiceState::Off]);
    }

    /// Issue #747: after the wake word the sentence goes to "Voice" as spoken, a conversation by voice
    /// starts there, the window shows it and the core reads the answer; a long silence ends
    /// the conversation with a tone, and the wake word is needed again.
    #[test]
    fn the_wake_word_starts_a_conversation_by_voice() {
        let run = run(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &[" Hey Torro, what is on today?"]);
        assert_eq!(run.ended, Ok(Ended::RecordingStopped), "listening for the wake word again");
        let sent = messages(&run.sent);
        assert_eq!(sent.len(), 1);
        let Request::SendMessage { conversation, engine, model, text, without, spoken, .. } = sent[0] else {
            unreachable!()
        };
        assert_eq!(text, "what is on today?", "without the wake word");
        assert!(*spoken, "a short answer, to be read aloud");
        assert_eq!(
            (conversation, engine.as_str()),
            (&None, "ai:openrouter"),
            "a new conversation, the engine used last"
        );
        assert_eq!(model.as_deref(), Some("ai:openrouter-model"));
        assert!(without.is_empty());
        assert!(run.sent.contains(&Request::RenameConversation { id: "c9".into(), title: voice_title().into() }));
        assert_eq!(count(&run.sent, &Request::StartTalk { conversation: Some("c9".into()) }), 1);
        assert_eq!(run.windows, ["c9"], "the quick window shows it");
        assert_eq!(count(&run.sent, &Request::SpeakAnswer { conversation: "c9".into() }), 1);
        assert_eq!(count(&run.sent, &Request::EndTalk), 1, "silence ends it");
        assert_eq!(run.chimes, 3, "the wake word, the open microphone, the end");
        assert_eq!(
            run.states[..6],
            [
                VoiceState::Listening,
                VoiceState::Recording,
                VoiceState::Recognizing,
                VoiceState::Answering,
                VoiceState::Conversing,
                VoiceState::Listening
            ]
        );
        assert_eq!(run.talk, None);
        assert_eq!(run.states.last(), Some(&VoiceState::Off));
    }

    /// Issue #747, done 1: the second sentence of a conversation by voice needs no wake word and goes
    /// to the same conversation; the window opens once.
    #[test]
    fn the_second_sentence_needs_no_wake_word() {
        let files = ["utterance.wav", "utterance.wav", PAUSE];
        let run = run(Core::new(on()), &files, Some(2), &["Hey Torro, what is on today?", "And tomorrow?"]);
        let sent = messages(&run.sent);
        assert_eq!(sent.len(), 2);
        let Request::SendMessage { conversation, text, spoken, .. } = sent[1] else { unreachable!() };
        assert_eq!((conversation.as_deref(), text.as_str(), *spoken), (Some("c9"), "And tomorrow?", true));
        assert_eq!(count(&run.sent, &Request::SpeakAnswer { conversation: "c9".into() }), 2);
        assert_eq!(run.sent.iter().filter(|r| matches!(r, Request::StartTalk { .. })).count(), 1);
        assert_eq!(run.windows, ["c9"]);
        assert_eq!(count(&run.sent, &Request::EndTalk), 1);
    }

    /// Issue #849: a minute of silence does not end the conversation by voice; the next sentence still
    /// goes to the same conversation.
    #[test]
    fn a_long_pause_does_not_end_the_conversation() {
        let files = ["utterance.wav", THINKING, PAUSE];
        let run = run(Core::new(on()), &files, Some(2), &["Hey Torro, what is on today?", "And tomorrow?"]);
        let sent = messages(&run.sent);
        assert_eq!(sent.len(), 2, "the sentence after the pause is sent");
        let Request::SendMessage { conversation, .. } = sent[1] else { unreachable!() };
        assert_eq!(conversation.as_deref(), Some("c9"));
        assert_eq!(count(&run.sent, &Request::EndTalk), 1, "only the long silence at the end");
    }

    /// Issue #849: noise in a pause comes to no text; nothing is sent, the conversation goes on without
    /// a new tone, and the next sentence is answered.
    #[test]
    fn noise_in_a_pause_sends_nothing() {
        let files = ["utterance.wav", "utterance.wav", "utterance.wav", PAUSE];
        let run = run(Core::new(on()), &files, Some(2), &["Hey Torro, what is on today?", "", "And tomorrow?"]);
        let sent = messages(&run.sent);
        assert_eq!(sent.len(), 2, "the empty text is dropped");
        let Request::SendMessage { text, .. } = sent[1] else { unreachable!() };
        assert_eq!(text, "And tomorrow?");
        assert_eq!(count(&run.sent, &Request::EndTalk), 1, "the noise did not end it");
        assert_eq!(run.chimes, 4, "the wake word, two open microphones (none after the noise), the end");
    }

    /// Issue #747, done 2 and decision 4: a short closing sentence ends the conversation by voice
    /// without being sent; a longer one goes on.
    #[test]
    fn a_closing_word_ends_the_conversation() {
        let files = ["utterance.wav", "utterance.wav"];
        let run = run(Core::new(on()), &files, Some(2), &["Hey Torro, what is on today?", "Danke."]);
        assert_eq!(messages(&run.sent).len(), 1, "\"Danke\" is not sent");
        assert_eq!(count(&run.sent, &Request::EndTalk), 1);
        assert_eq!(run.talk, None);
        for closing in ["Danke", "Stopp!", "Okay, danke schön.", "Das war’s.", "Tschüss", "That's all", "Thank you."]
        {
            assert!(is_closing(closing), "{closing}");
        }
        for going_on in ["Danke, und das Wetter?", "Wann endet das Meeting?", "", "Stoppuhr"] {
            assert!(!is_closing(going_on), "{going_on}");
        }
    }

    /// Issue #747, decision 1 and done 2: "Talk" in the window listens without the wake word and with
    /// "Hey Torro" off; it opens no window and ends, then the listener stops.
    #[test]
    fn a_conversation_from_the_window_listens_without_the_wake_word() {
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let run = run(core, &["utterance.wav", PAUSE], None, &["What is on today?"]);
        assert_eq!(run.ended, Ok(Ended::VoiceOff), "\"Hey Torro\" is off: no wake word after it");
        let sent = messages(&run.sent);
        let Request::SendMessage { conversation, .. } = sent[0] else { unreachable!() };
        assert_eq!((sent.len(), conversation.as_deref()), (1, Some("a1")));
        assert!(run.windows.is_empty(), "the window that started it shows it");
        assert!(!run.sent.iter().any(|request| matches!(request, Request::StartTalk { .. })));
        assert_eq!(count(&run.sent, &Request::SpeakAnswer { conversation: "a1".into() }), 1);
        assert_eq!(run.chimes, 3, "the open microphone twice, the end");

        // In a new chat the first sentence starts the conversation, and the talk goes on there.
        let core = Core { talk: talk_in(None), ..Core::new(Voice::default()) };
        let run = self::run(core, &["utterance.wav", PAUSE], None, &["What is on today?"]);
        let Request::SendMessage { conversation, .. } = messages(&run.sent)[0] else { unreachable!() };
        assert_eq!(conversation, &None);
        assert_eq!(count(&run.sent, &Request::StartTalk { conversation: Some("c9".into()) }), 1);
        assert!(!run.sent.iter().any(|request| matches!(request, Request::RenameConversation { .. })));
        assert!(run.windows.is_empty());
    }

    /// Review of PR #768: the recording program that stops during a conversation by voice is no
    /// silence: the conversation ends and the listener stops with why, without the closing tone.
    #[test]
    fn a_stopped_recording_ends_the_conversation_as_a_failure() {
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(on()) };
        let run = run(core, &["silence.wav"], None, &[""]);
        assert_eq!(run.ended, Ok(Ended::RecordingStopped));
        assert_eq!(count(&run.sent, &Request::EndTalk), 1);
        assert_eq!(run.talk, None);
        assert_eq!(run.chimes, 1, "the open microphone only");
    }

    /// Review of #747: an answer that still runs in the conversation keeps the microphone closed; the
    /// sentence is said once it is over.
    #[test]
    fn a_conversation_waits_for_the_answer_that_runs() {
        let mut core = Core { talk: talk_in(Some("b2")), ..Core::new(Voice::default()) };
        core.running = [true, true].into();
        let run = run(core, &["utterance.wav", PAUSE], None, &["What is on today?"]);
        assert_eq!(run.opened, 2, "opened only once it was over");
        assert_eq!(messages(&run.sent).len(), 1);
    }

    /// Issue #747, done 4: while anything is read aloud – also what an agent asked for – the microphone
    /// stays closed or closes, and what it heard meanwhile, the wake word too, is dropped.
    #[test]
    fn nothing_is_heard_while_something_is_read_aloud() {
        // Read aloud from the start: the microphone opens only once it is over.
        let core = Core { speaking: [true, true, true].into(), ..Core::new(on()) };
        let run = run(core, &["utterance.wav"], Some(2), &["Hey Torro, what is on today?"]);
        assert_eq!(run.states[0], VoiceState::Listening, "{:?}", run.states);
        assert_eq!(messages(&run.sent).len(), 1, "heard once it was quiet");

        // The wake word came while an agent's reading began: dropped, nothing recorded or sent.
        let core = Core { speaking: [false, true].into(), ..Core::new(on()) };
        let run = self::run(core, &["utterance.wav"], Some(2), &["Hey Torro, what is on today?"]);
        assert!(messages(&run.sent).is_empty());
        assert_eq!(run.chimes, 0);
        assert!(!run.states.contains(&VoiceState::Recording));
        assert_eq!(run.opened, 2, "closed, then open again once it was quiet");

        // A reading that starts while the microphone waits for the wake word closes it.
        let core = Core { speaking: [false, true].into(), ..Core::new(on()) };
        let run = self::run(core, &["utterance.wav", "utterance.wav"], Some(15), &["Hey Torro, what is on?"]);
        assert_eq!(run.opened, 3, "closed at the next look, then open again twice");
        let listening = run.states.iter().filter(|state| **state == VoiceState::Listening).count();
        assert!(listening >= 2 && run.states.contains(&VoiceState::Recording), "{:?}", run.states);
        assert_eq!(run.chimes, 1, "the wake word in the second round");

        // A reading that began and ended while the person spoke after the wake word: the sentence is
        // dropped, though nothing is read aloud by the time it is text (review of #747).
        let core = Core { readings: [0, 0, 1].into(), ..Core::new(on()) };
        let run = self::run(core, &["utterance.wav"], Some(2), &["Hey Torro, what is on today?"]);
        assert!(messages(&run.sent).is_empty(), "{:?}", run.sent);

        // In a conversation by voice too: what was said while it read is not sent.
        let core =
            Core { talk: talk_in(Some("a1")), speaking: [false, false, true].into(), ..Core::new(Voice::default()) };
        let run = self::run(core, &["utterance.wav", PAUSE], None, &["Read this aloud for me."]);
        assert!(messages(&run.sent).is_empty(), "{:?}", run.sent);
    }

    /// Issue #747, done 6 and decision 5: words never decide an approval; an answer that waits for one
    /// ends the conversation by voice, and the window shows the card.
    #[test]
    fn approvals_are_never_decided_by_voice() {
        let mut core = Core::new(on());
        core.answer = vec![EntryContent::Approval { id: "ap1".into() }];
        let run = run(core, &["utterance.wav", "utterance.wav"], Some(2), &["Hey Torro. Allow. Yes, allow it."]);
        assert_eq!(messages(&run.sent).len(), 1, "the words went to the assistant as a message, nothing else");
        for request in &run.sent {
            let name = request.operation().name();
            assert!(
                CoreAssistant::<fn(Request) -> Result<Response, Refusal>, fn(&str)>::REQUESTS.contains(&name),
                "{name}"
            );
            assert!(!matches!(request, Request::DecideApproval { .. }));
        }
        assert_eq!(count(&run.sent, &Request::EndTalk), 1, "the conversation by voice ended");
        assert_eq!(run.talk, None);
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
            Request::DecideApproval { id: "ap1".into(), allow: true, scope: None },
            Request::SetSetting { key: torro_ipc::SettingKey::VoiceEnabled, value: serde_json::json!(true) },
            Request::Speak { text: "Hi".into() },
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
            transcriber: Box::new(Says(VecDeque::new())),
            speaker: Box::new(Tone(Rc::default())),
            assistant: &mut assistant,
            report: Box::new(|_| {}),
            heard: Box::new(|_| {}),
            limits: Limits::default(),
            check_every: 10,
            pause: Duration::ZERO,
            floor: NoiseFloor::default(),
        };
        assert_eq!(listener.run(&on()), Ok(Ended::Changed));
        assert_eq!(*opened.borrow(), 0);
        // "Hey Torro" switched off during a conversation by voice is no new microphone.
        core.borrow_mut().voice.enabled = false;
        core.borrow_mut().talk = talk_in(Some("a1"));
        listener.recorder = Box::new(Files { files: [PAUSE].into(), opened: Rc::clone(&opened) });
        assert_eq!(listener.run(&Voice { microphone: Some("USB".into()), ..on() }), Ok(Ended::VoiceOff));
        assert_eq!(*opened.borrow(), 1, "it listened once, and silence ended the conversation");
    }

    /// What [`CoreAssistant::send`] sends for `to` with the core `core`: engine, model and effort.
    fn sent_with(core: Core, to: Option<Option<String>>) -> (String, Option<String>, Option<Effort>) {
        let core = Rc::new(RefCell::new(core));
        let ask = Rc::clone(&core);
        let mut assistant = CoreAssistant::new(
            move |request| ask.borrow_mut().answer(request),
            |_: &str| {},
            Duration::ZERO,
            Duration::from_secs(5),
        );
        assistant.send("What is on today?", to).expect("sent");
        let sent = core.borrow().sent.clone();
        let Some(Request::SendMessage { engine, model, effort, .. }) =
            sent.into_iter().find(|request| matches!(request, Request::SendMessage { .. }))
        else {
            unreachable!("a message was sent")
        };
        (engine, model, effort)
    }

    /// Issue #852, decision 2: without an open conversation the chat model of the settings answers before
    /// the one used last, while its engine is offered; an open conversation answers with its own choice.
    #[test]
    fn the_settings_come_before_the_model_used_last_and_a_conversation_keeps_its_own() {
        let gpt = || torro_core::ai::SlotModel {
            engine: "ai:openai".into(),
            model: Some("gpt".into()),
            effort: Some(Effort::High),
        };
        let with = |chat| Core { chat, ..Core::new(Voice::default()) };
        let settings = ("ai:openai".to_string(), Some("gpt".to_string()), Some(Effort::High));
        assert_eq!(sent_with(with(Some(gpt())), None), settings, "\"Hey Torro\" into \"Voice\"");
        assert_eq!(sent_with(with(Some(gpt())), Some(None)), settings, "\"Talk\" in a new chat");

        let gone = torro_core::ai::SlotModel { engine: "ai:removed".into(), ..gpt() };
        let last = ("ai:openrouter".to_string(), Some("ai:openrouter-model".to_string()), None);
        assert_eq!(sent_with(with(Some(gone)), None), last, "an engine no longer offered");
        assert_eq!(sent_with(with(None), None), last, "no chat model set");

        let mut core = with(Some(gpt()));
        core.conversations[0].effort = Some(Effort::Low);
        assert_eq!(
            sent_with(core, Some(Some("a1".into()))),
            (
                "ai:ollama@localhost:11434".to_string(),
                Some("ai:ollama@localhost:11434-model".to_string()),
                Some(Effort::Low)
            ),
            "the open conversation's choice, as the window stored it"
        );
    }

    #[test]
    fn engines_come_from_the_last_conversation_or_the_first_that_works() {
        assert_eq!(last_engine(&[]), None);
        let engines = vec![
            Engine {
                id: "ai:a".into(),
                kind: EngineKind::Ai,
                name: "A".into(),
                problem: Some("no key".into()),
                models: Vec::new(),
            },
            Engine { id: "ai:b".into(), kind: EngineKind::Ai, name: "B".into(), problem: None, models: Vec::new() },
        ];
        assert_eq!(first_engine(&engines).as_deref(), Some("ai:b"));
        assert_eq!(first_engine(&engines[..1]).as_deref(), Some("ai:a"));
        assert_eq!(first_engine(&[]), None);
    }

    #[test]
    fn answers_are_read_without_markdown_and_not_too_long() {
        assert_eq!(speakable_up_to("# Today\n- **Standup** at `09:30`\n- Lunch", 600), "Today Standup at 09:30 Lunch");
        assert_eq!(speakable_up_to("See [the plan](https://example.org/plan).", 600), "See the plan.");
        let long = "One sentence. ".repeat(300);
        let read = speakable_up_to(&long, 3000);
        assert!(read.chars().count() <= 3000 && read.ends_with('.'));
        assert_eq!(tr("Voice"), voice_title());
    }
}
