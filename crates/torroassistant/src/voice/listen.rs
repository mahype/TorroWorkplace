//! The listener's loop ([`Listener`]): wait for the wake word, play the tone, record until a
//! pause, turn it into text, send it ([`Assistant::send`]), have the answer read aloud, and again –
//! while the setting `voice.enabled` stays on or a conversation by voice runs. While anything is read
//! aloud, whoever asked for it, the microphone stays open only when the echo cancellation runs
//! ([`Listener::echo`], issue #834): it takes the sound read aloud out of the microphone, so the assistant
//! hears the person but not itself. Without it the microphone is closed while anything is read, so no
//! voice from the speakers is taken for the person's words or wakes it. With it the microphone stays open
//! during the listener's own answer too; what the person says meanwhile is still dropped.
//!
//! A conversation by voice (issue #747, `assistant.start_talk`): after "Hey Torro" and its answer, or
//! with "Talk" in the chat window, the microphone stays open after each answer, and
//! the person goes on without the wake word ([`Listener::converse`]), however long they think. It ends
//! with a long silence ([`TALK_WAIT_SECONDS`], a guard only), a short
//! closing sentence ("Danke", "Stopp", [`is_closing`]), "End talk" in the window, or an answer that
//! waits for an approval – that is given in the window only.
//!
//! The start of a sentence is never cut off (issue #829): while it waits for the wake word the
//! listener keeps the last second ([`BEFORE`]), so "Hey Torro, do X" in one breath arrives whole,
//! and the microphone is open while the tone plays; the tone and its echo are recorded, but are no
//! speech ([`Listener::chime`]).
//!
//! With Silero VAD downloaded (issue #833) a model tells speech from noise ([`Listener::vad`]), and in every
//! pause of [`EARLY_AFTER`] the sentence so far is recognised ahead ([`Listener::early`]): is the pause its
//! end, the text is mostly there; does the person go on, it is not used. Without the model, or when it fails,
//! loudness tells speech, and the sentence is recognised once it ended.
//!
//! In that pause the listener also judges whether the sentence is over (issue #836, [`super::turn`]): Smart
//! Turn hears its tone ([`Listener::turn`]), rules read the text recognised ahead. A finished sentence ends
//! after the short pause of the settings, an unfinished one ("and then …", "um") after the long one, and
//! "wait a moment" holds the turn until the person goes on; a hold over without more is not sent.
//!
//! [`CoreAssistant`] is the way to the core: it sends the sentence as `assistant.send_message`
//! (`spoken`, so the answer is short) to the conversation of the conversation by voice with the model
//! chosen there (issue #852), else to "Voice" with the chat model of the settings, the one used last
//! or the first engine ([`CoreAssistant::target`]), opens the quick window, has the answer read aloud sentence by
//! sentence (`assistant.speak_answer`) and follows it with `assistant.get_conversation`. It sends
//! nothing else: no request it builds can decide an approval, so no word spoken ever allows anything
//! ([`CoreAssistant::REQUESTS`]).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use chrono::{Local, TimeDelta, Utc};
use torro_core::ai::Effort;
use torro_core::i18n::trf;
use torro_core::settings::Voice;
use torro_ipc::assistant::{ConversationSummary, Engine, Entry, EntryContent};
use torro_ipc::{Request, Response, Talk, VoiceState};

use super::audio::{Endpoint, FRAMES_PER_SECOND, Limits, NoiseFloor, rms};
use super::early::Early;
use super::echo::Echo;
use super::turn::{Pauses, Turn, decide, only_hold, rules};
use super::wake::{after_wake_phrase, strip_wake_phrase};
use super::{Recorder, Speaker, SpeechDetector, Stream, Transcriber, TurnDetector, VoiceError, WakeWord};
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

    /// Starts reading the answer of `conversation` aloud like [`Assistant::speak_answer`] and returns at
    /// once (issue #834); the result comes over the channel once it was read. The microphone stays open
    /// meanwhile. By default it is read before this returns.
    fn start_answer(&mut self, conversation: &str) -> Receiver<Result<(), VoiceError>> {
        let (done, result) = mpsc::channel();
        let _ = done.send(self.speak_answer(conversation));
        result
    }

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
    /// The listener waited out a pause that ended every sentence before issue #836 ([`Endpoint::saved`]).
    pub held: bool,
}

/// What is known of the end of a sentence in its pause (issue #836): the tone's judgement and the text's,
/// each taken once per pause ([`Listener::judge_end`]).
#[derive(Debug, Default)]
struct Judging {
    /// The probability Smart Turn gave that the sentence is finished.
    model: Option<f32>,
    /// What the text recognised ahead says ([`rules`]); `None` while it is recognised, or for no text.
    text: Option<Turn>,
    /// The text recognised ahead was looked at.
    read: bool,
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

/// How many frames before the wake word was heard begin the sentence (issue #829), about a second:
/// the wake word is heard at its end, whoever goes on in one breath has begun the sentence by then,
/// and the whole "Hey Torro" fits, so no piece of it stays in front of the sentence
/// ([`after_wake_phrase`]).
pub const BEFORE: usize = FRAMES_PER_SECOND;

/// What a recording holds, for [`Listener::answer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    /// Nothing louder than the room.
    Nothing,
    /// Speech, after `before` samples kept from before the wake word was heard (none in a
    /// conversation by voice).
    Speech { before: usize },
    /// Perhaps a short command said in one breath with the wake word: it counts only if the wake
    /// word is in it, else it is the wake word alone or a piece of it.
    WithWake,
}

/// How many frames of silence after speech have the sentence so far recognised ahead (issue #833): 320 ms,
/// a pause within a sentence as well as the beginning of its end.
pub const EARLY_AFTER: usize = 4;

/// How many frames of echo follow the tone, which count as no speech either.
const ECHO: usize = 1;

/// How often the listener looks whether the echo cancellation runs while its answer is read with the
/// microphone closed (issue #834).
const ANSWER_LOOK: Duration = Duration::from_millis(20);

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
    /// The echo cancellation of [`Listener::recorder`] (issue #834): while it runs, the microphone stays
    /// open as something is read aloud. `None` without one: the microphone closes then, as before.
    pub echo: Option<Echo>,
    /// Tells speech from noise (Silero VAD, issue #833); `None` tells it by loudness, as does a detector that
    /// failed, for the rest of the run.
    pub vad: Option<Box<dyn SpeechDetector>>,
    /// Recognises the sentence ahead in its pauses (issue #833); `None` recognises it once it ended.
    pub early: Option<Early>,
    /// Hears in the tone whether a sentence is over (Smart Turn, issue #836); `None` leaves it to the text, as
    /// does a detector that failed, for the rest of the run. Asked only with [`Listener::vad`].
    pub turn: Option<Box<dyn TurnDetector>>,
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
    /// the microphone stays closed, unless the echo cancellation runs (issue #834). Failures of speech to text or of the core are written to the log
    /// and end the round with [`Ended::Nothing`]; only the microphone's fail it.
    ///
    /// `chime` says whether a conversation by voice opens its microphone with the tone (not after
    /// noise that was dropped: the person did not stop talking).
    fn listen_once(&mut self, voice: &Voice, chime: bool) -> Result<Ended, VoiceError> {
        let now = match self.check(voice) {
            Look::Stop(ended) => return Ok(ended),
            Look::Go(now) => now,
        };
        if now.speaking && !self.echo_runs() {
            self.quiet();
            return Ok(Ended::Nothing);
        }
        // A result of the round before never counts for this one.
        if let Some(early) = &mut self.early {
            early.clear();
        }
        if let Some(talk) = now.talk {
            return self.converse(voice, talk, chime);
        }
        let mut stream = self.recorder.open()?;
        let opened = Instant::now();
        (self.report)(VoiceState::Listening);
        let mut floor = NoiseFloor::default();
        // The last frames, the one with the wake word among them: the sentence begins with them.
        let mut before: VecDeque<Vec<i16>> = VecDeque::with_capacity(BEFORE + 1);
        let mut still_speaking = false;
        let mut frames = 0usize;
        loop {
            let Some(frame) = stream.frame()? else { return Ok(Ended::RecordingStopped) };
            frames += 1;
            if frames.is_multiple_of(self.check_every.max(1)) {
                match self.check(voice) {
                    Look::Stop(ended) => return Ok(ended),
                    // Something is read aloud without the echo cancellation, or a conversation by voice
                    // began: the microphone closes, and the next round decides.
                    Look::Go(now) if (now.speaking && !self.echo_runs()) || now.talk.is_some() => {
                        drop(stream);
                        self.wake.reset();
                        return Ok(Ended::Nothing);
                    }
                    Look::Go(_) => {}
                }
            }
            let heard = self.wake.hears(&frame)?;
            if heard {
                // Still speaking when the wake word was heard: "Hey Torro, light on" in one breath.
                still_speaking = rms(&frame) >= floor.speech();
            } else {
                floor.follow(rms(&frame));
            }
            before.push_back(frame);
            if before.len() > BEFORE {
                before.pop_front();
            }
            if heard {
                break;
            }
        }
        self.floor = floor;
        // The sentence begins a second before the wake word: from then on its sound must have gone through
        // the echo cancellation, not from when the microphone opened, maybe minutes ago.
        let begins = Instant::now().checked_sub(Duration::from_secs(1)).map_or(opened, |begins| begins.max(opened));
        let level = floor.speech();
        // Silero VAD hears the frames kept from before the wake word and then the tone: whether the person
        // still speaks is decided at the tone's end, past the 100 ms it holds speech after it ended, so "Hey
        // Torro" alone is no command said in one breath.
        if let Some(vad) = &mut self.vad {
            vad.reset();
        }
        if self.vad.is_some() {
            for frame in &before {
                self.speech(frame, level);
            }
        }
        // Heard while something was read aloud: it may be the reading's voice, not the person's.
        let Some(readings) = self.quiet_now() else {
            drop(stream);
            self.wake.reset();
            self.quiet();
            return Ok(Ended::Nothing);
        };
        (self.report)(VoiceState::Recording);
        let tone = self.chime();
        // What was said before the wake word was heard is part of the sentence, but the end of the
        // sentence is decided by what follows: "Hey Torro" alone is no sentence.
        let mut sentence: Vec<i16> = before.into_iter().flatten().collect();
        let kept = sentence.len();
        // Where the tone and its echo end in the sentence.
        let mut with_tone = sentence.len();
        let mut end = Endpoint::new(floor, self.limits);
        let (mut frames, mut ahead) = (0usize, false);
        let (pauses, mut judging) = (Pauses::of(voice), Judging::default());
        while let Some(frame) = stream.frame()? {
            frames += 1;
            // A reading that begins meanwhile: what the microphone hears is not the person's alone, unless
            // the echo cancellation takes it out.
            if frames.is_multiple_of(self.check_every.max(1)) && self.speaking() && !self.echo_runs() {
                drop(stream);
                self.wake.reset();
                self.quiet();
                return Ok(Ended::Nothing);
            }
            sentence.extend_from_slice(&frame);
            if frames <= tone {
                with_tone = sentence.len();
                if self.vad.is_some() {
                    let speech = self.speech(&frame, level);
                    // A detector that failed just now judged by loudness, and the tone is loud.
                    if frames == tone && self.vad.is_some() {
                        still_speaking = speech;
                    }
                }
                continue;
            }
            // Silero VAD starts afresh after the tone: its speech of the wake word, which it holds a little
            // past its end, must not count as the first word of the sentence.
            if frames == tone + 1
                && let Some(vad) = &mut self.vad
            {
                vad.reset();
            }
            let speech = self.speech(&frame, level);
            let ended = end.push_heard(speech);
            self.look_ahead(speech, &end, &sentence, &mut ahead);
            self.judge_end(speech, &mut end, &sentence, &mut judging, pauses);
            if ended {
                break;
            }
        }
        // The microphone closes before the answer: it is not recorded, and it cannot wake it.
        drop(stream);
        (self.report)(VoiceState::Recognizing);
        // Nothing said after the tone, but the person went on speaking past the wake word: a short
        // command said in one breath fits into the tone. What came up to the tone's end is
        // understood; "Hey Torro" alone comes to no text and sends nothing.
        let (said, recorded) = match (end.heard_speech(), still_speaking) {
            (true, _) => (Said::Speech { before: kept }, &sentence[..]),
            (false, true) => (Said::WithWake, &sentence[..with_tone]),
            (false, false) => (Said::Nothing, &sentence[..]),
        };
        let pause = (end.trailing_quiet(), end.saved());
        let ended = self.answer(voice, recorded, said, pause, None, (readings, begins), ahead);
        self.wake.reset();
        Ok(ended)
    }

    /// Plays the tone with the microphone open (issue #829): the frames that come meanwhile are
    /// recorded, so words said with the tone are kept. Answers how many frames the tone took, with
    /// its echo: they count as no speech, so the tone alone never makes a sentence.
    fn chime(&mut self) -> usize {
        let started = Instant::now();
        self.speaker.chime();
        let played = usize::try_from(started.elapsed().as_millis()).unwrap_or(usize::MAX);
        played.saturating_mul(FRAMES_PER_SECOND).div_ceil(1000) + ECHO
    }

    /// The next sentence of the conversation by voice `talk` (issue #747): the tone, then up to
    /// [`TALK_WAIT_SECONDS`] for the person to speak, until a pause. Only a long silence ends the
    /// conversation, with a tone. Something read aloud meanwhile without the echo cancellation (issue #834), or
    /// the conversation ended or moved elsewhere, closes the microphone, and what was heard is dropped.
    fn converse(&mut self, voice: &Voice, talk: Talk, chime: bool) -> Result<Ended, VoiceError> {
        // An answer still runs there (one longer than the listener waited, or a talk started while it
        // ran): the conversation would refuse the sentence, so the microphone waits for it.
        if let Some(id) = &talk.conversation
            && self.assistant.running(id).unwrap_or(false)
        {
            std::thread::sleep(self.pause);
            return Ok(Ended::Nothing);
        }
        // The microphone opens before the tone (issue #829): whoever speaks with it loses no word once
        // the sentence goes on after it, and the tone itself is no sentence of the person's.
        let mut stream = self.recorder.open()?;
        let opened = Instant::now();
        let tone = if chime { self.chime() } else { 0 };
        let Some(readings) = self.quiet_now() else {
            drop(stream);
            self.quiet();
            return Ok(Ended::Nothing);
        };
        (self.report)(VoiceState::Conversing);
        // The wait for the first word comes on top of the longest sentence.
        let wait = TALK_WAIT_SECONDS * FRAMES_PER_SECOND;
        let limits = Limits { wait, most: wait + self.limits.most, ..self.limits };
        let mut end = Endpoint::new(self.floor, limits);
        let level = self.floor.speech();
        if let Some(vad) = &mut self.vad {
            vad.reset();
        }
        let (mut sentence, mut frames, mut recording, mut ended) = (Vec::new(), 0usize, false, false);
        let mut ahead = false;
        let (pauses, mut judging) = (Pauses::of(voice), Judging::default());
        while let Some(frame) = stream.frame()? {
            frames += 1;
            if frames.is_multiple_of(self.check_every.max(1)) {
                match self.check(voice) {
                    Look::Stop(ended) => return Ok(ended),
                    Look::Go(now) if now.speaking && !self.echo_runs() => {
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
            // The tone is not given to Silero VAD: taken for speech, it would begin a sentence of silence.
            if frames <= tone {
                continue;
            }
            let speech = self.speech(&frame, level);
            ended = end.push_heard(speech);
            if !recording && end.heard_speech() {
                recording = true;
                (self.report)(VoiceState::Recording);
            }
            // Before the end is looked at: speech in the last frame withdraws what was recognised ahead.
            self.look_ahead(speech, &end, &sentence, &mut ahead);
            self.judge_end(speech, &mut end, &sentence, &mut judging, pauses);
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
        let said = Said::Speech { before: 0 };
        let pause = (end.trailing_quiet(), end.saved());
        Ok(self.answer(voice, &sentence, said, pause, Some(talk), (readings, opened), ahead))
    }

    /// Whether someone speaks in `frame`: as Silero VAD judges it, else when it is at least as loud as
    /// `level`. A detector that fails is written to the log and left for loudness, for the rest of the run.
    fn speech(&mut self, frame: &[i16], level: f32) -> bool {
        if let Some(vad) = &mut self.vad {
            match vad.speech(frame) {
                Ok(speech) => return speech,
                Err(err) => {
                    eprintln!("torroassistant: speech detection failed, speech is told by loudness: {err}");
                    self.vad = None;
                    // Recognising ahead and judging the end go with Silero VAD (`Parts::early`, `Parts::turn`): by
                    // loudness all stays as it was.
                    self.early = None;
                    self.turn = None;
                }
            }
        }
        rms(frame) >= level
    }

    /// Recognises the sentence so far ahead (issue #833) once speech was followed by [`EARLY_AFTER`] silent
    /// frames; `ahead` says whether that offer still stands, and speech after it withdraws it.
    fn look_ahead(&mut self, speech: bool, end: &Endpoint, sentence: &[i16], ahead: &mut bool) {
        let Some(early) = &mut self.early else { return };
        if speech {
            *ahead = false;
        } else if end.heard_speech() && end.trailing_quiet() == EARLY_AFTER {
            early.offer(sentence.to_vec());
            *ahead = true;
        }
    }

    /// Judges in the pause of `sentence` whether it is over (issue #836) and has `end` wait as long as that
    /// says ([`Pauses`]): at [`EARLY_AFTER`] quiet frames by its tone ([`Listener::turn`]), and once the text
    /// recognised ahead is there by that text ([`rules`], [`decide`]). Speech forgets what was judged; without
    /// Silero VAD nothing is judged and the pause of [`Limits`] ends the sentence. A turn detector that fails
    /// is written to the log and left out for the rest of the run.
    fn judge_end(&mut self, speech: bool, end: &mut Endpoint, sentence: &[i16], judging: &mut Judging, pauses: Pauses) {
        if speech {
            *judging = Judging::default();
            return;
        }
        if self.vad.is_none() || !end.heard_speech() || end.trailing_quiet() < EARLY_AFTER {
            return;
        }
        let mut new = false;
        if end.trailing_quiet() == EARLY_AFTER
            && let Some(turn) = &mut self.turn
        {
            match turn.complete(sentence) {
                Ok(finished) => {
                    judging.model = Some(finished);
                    new = true;
                }
                Err(err) => {
                    eprintln!("torroassistant: Smart Turn failed, the text tells the end of a sentence: {err}");
                    self.turn = None;
                }
            }
        }
        if !judging.read
            && let Some(text) = self.early.as_ref().and_then(Early::peek)
        {
            judging.read = true;
            judging.text = text.ok().and_then(|text| rules(&text));
            new = true;
        }
        match decide(judging.model, judging.text) {
            Some(Turn::Hold) if new => end.hold(pauses.hold),
            Some(turn) if new => end.wait_for(pauses.of_turn(turn)),
            _ => {}
        }
    }

    /// The text of `sentence`: the one recognised ahead when the offer still stands (`ahead`), else
    /// recognised now. A failure ahead is written to the log and the sentence recognised again.
    fn recognize(&mut self, sentence: &[i16], ahead: bool) -> Result<String, VoiceError> {
        let early = match &mut self.early {
            Some(early) if ahead => early.result(),
            Some(early) => {
                // Nothing ahead counts: an offer not yet begun would only hold the model up.
                early.clear();
                None
            }
            None => None,
        };
        match early {
            Some(Ok(text)) => Ok(text),
            Some(Err(err)) => {
                eprintln!("torroassistant: recognised ahead: {err}");
                self.transcriber.transcribe(sentence)
            }
            None => self.transcriber.transcribe(sentence),
        }
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

    /// Whether the echo cancellation runs now ([`Echo::running`]): the microphone may stay open while
    /// something is read aloud.
    fn echo_runs(&self) -> bool {
        self.echo.as_ref().is_some_and(Echo::running)
    }

    /// Whether what the microphone heard since `at` went through the echo cancellation.
    fn echo_since(&self, at: Instant) -> bool {
        self.echo.as_ref().is_some_and(|echo| echo.heard_since(at))
    }

    /// How many readings began, when nothing is read aloud now or the echo cancellation runs; `None`
    /// while something is read without it. A core that cannot be asked counts as quiet, with no reading.
    fn quiet_now(&mut self) -> Option<u64> {
        match self.assistant.talk() {
            Ok(now) if now.speaking && !self.echo_runs() => None,
            Ok(now) => Some(now.readings),
            Err(_) => Some(0),
        }
    }

    /// Waits until nothing is read aloud any more, then a little longer, so the room is quiet before
    /// the microphone opens again. Once the echo cancellation runs, it waits no longer: the microphone
    /// may open while it reads.
    fn quiet(&mut self) {
        while self.speaking() && !self.echo_runs() {
            std::thread::sleep(self.pause);
        }
        if !self.echo_runs() {
            std::thread::sleep(self.pause);
        }
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
    /// conversation by voice (decision 5). A closing sentence ends it without sending. `began` are the
    /// readings begun when the microphone opened, and when that was: a reading that began since, even one
    /// already over, drops the sentence, unless the echo cancellation took it out of the microphone (issue
    /// #834). `pause`: how many quiet frames ended the sentence and whether a pause that once ended it was
    /// waited out ([`Endpoint::saved`], issue #836); with the time speech recognition took they
    /// go to [`Listener::heard`] once the sentence was sent, if the answer is read aloud. `ahead`: the
    /// sentence was recognised ahead in its last pause ([`Listener::early`]), so that text is taken. With
    /// Silero VAD a sentence of nothing but "um" and "wait a moment" is not sent: the hold ended without
    /// more ([`only_hold`]).
    #[expect(clippy::too_many_arguments, reason = "the round's parts, each used once")]
    fn answer(
        &mut self,
        voice: &Voice,
        sentence: &[i16],
        said: Said,
        pause: (usize, bool),
        talk: Option<Talk>,
        began: (u64, Instant),
        ahead: bool,
    ) -> Ended {
        let (readings, opened) = began;
        if said == Said::Nothing {
            return Ended::Nothing;
        }
        let recognizing = Instant::now();
        let transcribed = match self.recognize(sentence, ahead && matches!(said, Said::Speech { .. })) {
            // After the wake word the sentence begins a second before it (issue #829): what follows
            // it; a short command needs it, else what was heard is the wake word alone.
            Ok(text) if talk.is_none() => match (after_wake_phrase(&text), said) {
                (Some(rest), _) => Ok(rest),
                (None, Said::WithWake) => Ok(String::new()),
                // The wake word not found in the text: what came before it may be another voice (a
                // television), so only what was said after it is understood.
                (None, Said::Speech { before }) if before > 0 && before < sentence.len() => {
                    self.transcriber.transcribe(&sentence[before..]).map(|text| strip_wake_phrase(&text))
                }
                (None, _) => Ok(strip_wake_phrase(&text)),
            },
            Ok(text) => Ok(strip_wake_phrase(&text)),
            Err(err) => Err(err),
        };
        let recognize_ms = u32::try_from(recognizing.elapsed().as_millis()).unwrap_or(u32::MAX);
        let text = match transcribed {
            Ok(text) => text,
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return Ended::Nothing;
            }
        };
        // Noise in a pause comes to no text: it is dropped and the conversation by voice goes on. So does a hold
        // the person did not go on after (issue #836).
        if text.is_empty() || (self.vad.is_some() && only_hold(&text)) {
            return if talk.is_some() { Ended::Unheard } else { Ended::Nothing };
        }
        if is_closing(&text) {
            if talk.is_some() {
                self.close_talk();
            }
            return Ended::Nothing;
        }
        // Read aloud while it was said, or still: it may be the reading's voice, so it is dropped – unless
        // the echo cancellation took that voice out.
        if self.quiet_now() != Some(readings) && !self.echo_since(opened) {
            // Not "recognizing" while it waits (issue #830): the core makes "answering" "speaking" while it reads.
            (self.report)(VoiceState::Answering);
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
                pause_ms: frames_ms(pause.0),
                recognize_ms,
                held: pause.1,
            });
            self.read_answer(&sent.conversation);
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

    /// Reads the answer of `conversation` aloud. Without the echo cancellation the microphone stays
    /// closed, and the room's echo fades before it opens again. With it (issue #834, decision 2) the
    /// microphone is open while it reads and the cancellation runs, so the canceller follows the room, and what it hears is dropped;
    /// the pause after it is left out when the cancellation took part, so a sentence said at once after
    /// the answer keeps its beginning.
    fn read_answer(&mut self, conversation: &str) {
        if self.echo.is_none() {
            if let Err(err) = self.assistant.speak_answer(conversation) {
                eprintln!("torroassistant: cannot speak the answer: {err}");
            }
            // The room's echo of the answer fades before the microphone opens again.
            std::thread::sleep(self.pause);
            return;
        }
        let started = Instant::now();
        let done = self.assistant.start_answer(conversation);
        let mut stream: Option<Box<dyn Stream>> = None;
        // The microphone failed or stopped once: it stays closed for the rest of the answer.
        let mut broken = false;
        let read = loop {
            // Open only while the cancellation runs; without it closed, as before.
            if !self.echo_runs() {
                stream = None;
            } else if stream.is_none() && !broken {
                match self.recorder.open() {
                    Ok(opened) => stream = Some(opened),
                    Err(err) => {
                        eprintln!("torroassistant: {err}");
                        broken = true;
                    }
                }
            }
            match done.try_recv() {
                Ok(read) => break read,
                Err(TryRecvError::Disconnected) => break Ok(()),
                Err(TryRecvError::Empty) => {}
            }
            match stream.as_mut().map(|stream| stream.frame()) {
                // Heard while it reads: dropped.
                Some(Ok(Some(_))) => {}
                Some(heard) => {
                    if let Err(err) = heard {
                        eprintln!("torroassistant: {err}");
                    }
                    stream = None;
                    broken = true;
                }
                None => match done.recv_timeout(ANSWER_LOOK) {
                    Ok(read) => break read,
                    Err(RecvTimeoutError::Disconnected) => break Ok(()),
                    Err(RecvTimeoutError::Timeout) => {}
                },
            }
        };
        drop(stream);
        if let Err(err) = read {
            eprintln!("torroassistant: cannot speak the answer: {err}");
        }
        if !self.echo_since(started) {
            std::thread::sleep(self.pause);
        }
    }
}

/// The conversation spoken messages go to.
pub fn voice_title() -> &'static str {
    torro_core::i18n::tr("Voice")
}

/// Sends `assistant.speak_answer` from a thread of its own ([`CoreAssistant::with_reader`], issue #834),
/// while the listener keeps its microphone open.
pub type Reader = Arc<dyn Fn(Request) -> Result<Response, Refusal> + Send + Sync>;

/// The way to the core (see the module): `ask` sends a request, `open` shows a conversation in the
/// quick window.
pub struct CoreAssistant<F, O> {
    ask: F,
    open: O,
    /// Reads answers aloud on a thread of its own ([`Assistant::start_answer`]); without it they are
    /// read before the listener goes on.
    reader: Option<Reader>,
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
        CoreAssistant { ask, open, reader: None, pause, wait }
    }

    /// Reads answers aloud with `reader` on a thread of its own (issue #834): `ask` stays with the
    /// listener, whose microphone stays open meanwhile.
    pub fn with_reader(mut self, reader: Reader) -> Self {
        self.reader = Some(reader);
        self
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
    /// own choice. Of several "Voice" the newest counts; after a pause it is left for a new one
    /// ([`CoreAssistant::rested`], issue #831), and the second value is the renaming of the old one, to send
    /// once the new one stands.
    fn target(&mut self, conversation: Option<&str>) -> Result<(Target, Option<Request>), VoiceError> {
        let conversations = match self.request(Request::ListConversations)? {
            Response::Conversations { conversations } => conversations,
            other => return Err(unexpected(&other)),
        };
        let mut retire = None;
        let found = match conversation {
            Some(id) => conversations.iter().find(|summary| summary.id == id),
            None => {
                let voice = conversations
                    .iter()
                    .filter(|summary| summary.title == voice_title())
                    .max_by_key(|summary| summary.updated);
                match voice.filter(|voice| self.rested(voice)) {
                    Some(old) => {
                        retire = Some(retired(old));
                        None
                    }
                    None => voice,
                }
            }
        };
        let id = found.map(|summary| summary.id.clone());
        if let Some(summary) = found.filter(|_| conversation.is_some()) {
            return Ok(((id, summary.engine.clone(), summary.model.clone(), summary.effort), retire));
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
            return Ok(((id, slot.engine, slot.model, slot.effort), retire));
        }
        if let Some((engine, model, effort)) = last_engine(&conversations) {
            return Ok(((id, engine, model, effort), retire));
        }
        let engine = first_engine(&engines).ok_or_else(|| {
            VoiceError::Core(Refusal::new("not-found", "no engine: add an AI account in Torro Workplace"))
        })?;
        Ok(((id, engine, None, None), retire))
    }
}

impl<F, O> CoreAssistant<F, O>
where
    F: FnMut(Request) -> Result<Response, Refusal>,
    O: FnMut(&str),
{
    /// Whether "Hey Torro" leaves the conversation "Voice" `voice` for a new one (issue #831): after the
    /// pause of [`Voice::new_after_or_default`] without an entry, and no answer running.
    fn rested(&mut self, voice: &ConversationSummary) -> bool {
        let minutes = match self.settings() {
            Ok(settings) => settings.new_after_or_default(),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                torro_core::settings::VOICE_NEW_AFTER_DEFAULT
            }
        };
        minutes > 0 && !voice.running && Utc::now() - voice.updated >= TimeDelta::minutes(i64::from(minutes))
    }
}

/// The renaming of the old "Voice" `old` (issue #831): it keeps its date and time in its title ("Voice ·
/// 2026-11-15 09:12"), so only the new one is "Voice". Sent only once the new one stands, so a failed send
/// leaves the old one as it was.
fn retired(old: &ConversationSummary) -> Request {
    let date = old.created.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();
    let title = trf("{title} · {date}", &[("title", &voice_title()), ("date", &date)]);
    Request::RenameConversation { id: old.id.clone(), title }
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
        let ((conversation, engine, model, effort), retire) = match &to {
            // A new conversation for a conversation by voice in a new chat: as for "Voice".
            Some(None) => {
                let ((_, engine, model, effort), _) = self.target(Some(""))?;
                ((None, engine, model, effort), None)
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
        // The old "Voice" keeps its date only now the new one stands (issue #831); should that fail, the newest
        // "Voice" still counts next time.
        if let Some(rename) = retire
            && let Err(err) = self.request(rename)
        {
            eprintln!("torroassistant: cannot keep the old conversation by its date: {err}");
        }
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

    fn start_answer(&mut self, conversation: &str) -> Receiver<Result<(), VoiceError>> {
        let (done, result) = mpsc::channel();
        let Some(reader) = self.reader.clone() else {
            let _ = done.send(self.speak_answer(conversation));
            return result;
        };
        // Only this one request goes this way; it is on the list of what the listener sends.
        let request = Request::SpeakAnswer { conversation: conversation.to_string() };
        std::thread::spawn(move || {
            let read = match reader(request) {
                Ok(Response::Ok) => Ok(()),
                Ok(other) => Err(unexpected(&other)),
                Err(refusal) => Err(VoiceError::Core(refusal)),
            };
            let _ = done.send(read);
        });
        result
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

    /// The "file" of the tone as the microphone hears it, then the silence of [`PAUSE`] (issue #829).
    const CHIME: &str = "chime";

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
            let frames = (TALK_WAIT_SECONDS + 2) * super::super::audio::FRAMES_PER_SECOND;
            let silence = (0..frames).map(|_| vec![0; FRAME]);
            if name == PAUSE {
                return Ok(Box::new(Frames(silence.collect())));
            }
            if name == CHIME {
                let mut tone = super::super::audio::chime();
                tone.resize(tone.len().next_multiple_of(FRAME), 0);
                return Ok(Box::new(Frames(tone.chunks(FRAME).map(<[i16]>::to_vec).chain(silence).collect())));
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

    /// What [`Says`] hears to fail as speech recognition fails.
    const FAILS: &str = "<fails>";

    impl Transcriber for Says {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            assert!(!samples.is_empty());
            let text = if self.0.len() > 1 { self.0.pop_front() } else { self.0.front().copied() };
            // The model fails on this sentence (issue #830).
            if text == Some(FAILS) {
                return Err(VoiceError::program("whisper", "made nothing"));
            }
            Ok(text.unwrap_or_default().to_string())
        }
    }

    /// Notes for each tone how often the microphone had been opened by then, and takes `lasts` to play.
    #[derive(Default)]
    struct Tone {
        opened_at: Rc<RefCell<Vec<usize>>>,
        opened: Rc<RefCell<usize>>,
        lasts: Duration,
    }

    impl Speaker for Tone {
        fn chime(&mut self) {
            self.opened_at.borrow_mut().push(*self.opened.borrow());
            std::thread::sleep(self.lasts);
        }
    }

    /// Understands `text` every time and keeps what it was given.
    struct Keeps {
        text: &'static str,
        given: Rc<RefCell<Vec<Vec<i16>>>>,
    }

    impl Transcriber for Keeps {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            self.given.borrow_mut().push(samples.to_vec());
            Ok(self.text.to_string())
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
            context: None,
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
        /// How often the microphone had been opened at each tone.
        tones: Vec<usize>,
        windows: Vec<String>,
        states: Vec<VoiceState>,
        talk: Option<Talk>,
        heard: Vec<Heard>,
        ended: Result<Ended, VoiceError>,
    }

    /// The listener on `files`, waking at frame `wake`, hearing `heard` in order, with the core `core`.
    fn run(core: Core, files: &[&'static str], wake: Option<usize>, heard: &[&'static str]) -> Run {
        run_with(core, files, wake, Box::new(Says(heard.iter().copied().collect())), Duration::ZERO)
    }

    /// [`run`] with the transcriber `transcriber` and a tone that takes `tone` to play.
    fn run_with(
        core: Core,
        files: &[&'static str],
        wake: Option<usize>,
        transcriber: Box<dyn Transcriber>,
        tone: Duration,
    ) -> Run {
        let ahead = Ahead { vad: None, early: None, limits: Limits::default(), echo: None, turn: None };
        run_parts(core, files, wake, transcriber, tone, ahead)
    }

    /// What [`run_parts`] adds to [`run_with`] (issue #833): a speech detector, the recognition ahead and the
    /// limits of a sentence.
    #[derive(Default)]
    struct Ahead {
        vad: Option<Box<dyn SpeechDetector>>,
        early: Option<Early>,
        limits: Limits,
        /// The echo cancellation (issue #834).
        echo: Option<Echo>,
        /// Hears the end of a sentence in its tone (issue #836).
        turn: Option<Box<dyn TurnDetector>>,
    }

    /// [`run_with`] with the parts of `ahead`.
    fn run_parts(
        core: Core,
        files: &[&'static str],
        wake: Option<usize>,
        transcriber: Box<dyn Transcriber>,
        tone: Duration,
        ahead: Ahead,
    ) -> Run {
        let core = Rc::new(RefCell::new(core));
        let windows = Rc::new(RefCell::new(Vec::new()));
        let (ask_core, shown) = (Rc::clone(&core), Rc::clone(&windows));
        let mut assistant = CoreAssistant::new(
            move |request| ask_core.borrow_mut().answer(request),
            move |id: &str| shown.borrow_mut().push(id.to_string()),
            Duration::ZERO,
            Duration::from_secs(5),
        );
        let (chimes, opened) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(0)));
        let states = Rc::new(RefCell::new(Vec::new()));
        let voice = core.borrow().voice.clone();
        let reported = Rc::clone(&states);
        let told = Rc::new(RefCell::new(Vec::new()));
        let rounds = Rc::clone(&told);
        let mut listener = Listener {
            recorder: Box::new(Files { files: files.iter().copied().collect(), opened: Rc::clone(&opened) }),
            wake: Box::new(WakeAt { at: wake, seen: 0 }),
            transcriber,
            speaker: Box::new(Tone { opened_at: Rc::clone(&chimes), opened: Rc::clone(&opened), lasts: tone }),
            assistant: &mut assistant,
            report: Box::new(move |state| reported.borrow_mut().push(state)),
            heard: Box::new(move |round| rounds.borrow_mut().push(round)),
            limits: ahead.limits,
            check_every: 10,
            pause: Duration::ZERO,
            floor: NoiseFloor::default(),
            vad: ahead.vad,
            early: ahead.early,
            echo: ahead.echo,
            turn: ahead.turn,
        };
        let ended = listener.run(&voice);
        drop(listener);
        drop(assistant);
        let core = core.borrow();
        Run {
            sent: core.sent.clone(),
            opened: *opened.borrow(),
            chimes: chimes.borrow().len(),
            tones: chimes.borrow().clone(),
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

    /// Issue #829: "Hey Torro, what is on today?" in one breath arrives whole: the sentence begins a second
    /// before the wake word was heard, and the microphone stays open through the tone.
    #[test]
    fn the_sentence_begins_before_the_wake_word_was_heard() {
        let given = Rc::new(RefCell::new(Vec::new()));
        let keeps = Keeps { text: " Hey Torro, what is on today?", given: Rc::clone(&given) };
        // The fixture speaks in frames 4 to 10; the wake word is heard in the middle of it.
        let run = run_with(Core::new(on()), &["utterance.wav"], Some(6), Box::new(keeps), Duration::ZERO);
        let Request::SendMessage { text, .. } = messages(&run.sent)[0] else { unreachable!() };
        assert_eq!(text, "what is on today?");
        assert_eq!(run.tones[0], 1, "the microphone was open with the tone");
        let file = fixture("utterance.wav");
        let sentence = &given.borrow()[0];
        assert!(sentence.len() > 6 * FRAME, "the sentence goes on after the wake word");
        assert!(BEFORE >= 6 && sentence[..] == file[..sentence.len()], "the file from before the wake word");
    }

    /// Issue #829, review: "Hey Torro, light on" in one breath, the command over before the tone is: what
    /// came up to the tone's end is understood and sent; "Hey Torro" alone sends nothing.
    #[test]
    fn a_short_command_within_the_tone_is_sent() {
        // The fixture speaks in frames 4 to 10; the wake word at frame 6, a tone of 400 ms covers the rest.
        let tone = Duration::from_millis(400);
        let given = Rc::new(RefCell::new(Vec::new()));
        let keeps = Keeps { text: " Hey Torro, light on.", given: Rc::clone(&given) };
        let run = run_with(Core::new(on()), &["utterance.wav"], Some(6), Box::new(keeps), tone);
        let Request::SendMessage { text, .. } = messages(&run.sent)[0] else { unreachable!() };
        assert_eq!(text, "light on.");
        let sentence = given.borrow()[0].len();
        assert!(sentence >= 10 * FRAME, "up to the tone's end at least: {sentence}");

        for alone in ["Hey Torro.", "Ro.", "Thank you for watching."] {
            let keeps = Keeps { text: alone, given: Rc::default() };
            let run = run_with(Core::new(on()), &["utterance.wav"], Some(6), Box::new(keeps), tone);
            assert!(messages(&run.sent).is_empty(), "the wake word alone or a piece of it: {alone}");
        }
    }

    /// Review of PR #868: without the wake word in the text, what came before it may be another voice;
    /// only what was said after the wake word is understood and sent.
    #[test]
    fn without_the_wake_word_in_the_text_only_what_followed_is_sent() {
        let given = Rc::new(RefCell::new(Vec::new()));
        let file = fixture("utterance.wav");
        // The first hearing misses the wake word (a television before it); the second hears the rest.
        struct Twice(Rc<RefCell<Vec<Vec<i16>>>>);
        impl Transcriber for Twice {
            fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
                self.0.borrow_mut().push(samples.to_vec());
                let first = self.0.borrow().len() == 1;
                Ok(if first { "and the news at ten. Hello Toro what is on?" } else { "what is on?" }.into())
            }
        }
        let run =
            run_with(Core::new(on()), &["utterance.wav"], Some(6), Box::new(Twice(Rc::clone(&given))), Duration::ZERO);
        let Request::SendMessage { text, .. } = messages(&run.sent)[0] else { unreachable!() };
        assert_eq!(text, "what is on?");
        let given = given.borrow();
        assert_eq!(given.len(), 2, "understood twice");
        assert!(
            given[1][..] == file[6 * FRAME..6 * FRAME + given[1].len()],
            "the second time from after the wake word"
        );
    }

    /// Issue #829: in a conversation by voice the microphone opens before the tone; the tone and its echo
    /// are recorded but are no speech, so a tone into silence ends the conversation without a sentence.
    #[test]
    fn the_tone_is_heard_but_is_no_speech() {
        let given = Rc::new(RefCell::new(Vec::new()));
        let keeps = Keeps { text: "", given: Rc::clone(&given) };
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let run = run_with(core, &[CHIME], None, Box::new(keeps), Duration::from_millis(200));
        assert_eq!(run.tones, [1, 1], "the microphone was open with the tone, then the closing tone");
        assert!(given.borrow().is_empty(), "nothing to understand");
        assert!(!run.states.contains(&VoiceState::Recording), "{:?}", run.states);
        assert_eq!(count(&run.sent, &Request::EndTalk), 1, "silence ended it");
        assert!(messages(&run.sent).is_empty());
    }

    /// Issue #830: a sentence that comes to no text, or whose recognition fails, is recognized, but nothing is
    /// sent, so the listener never says it answers.
    #[test]
    fn a_sentence_without_text_is_recognized_but_not_answered() {
        for heard in ["", FAILS] {
            let run = run(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &[heard]);
            assert!(messages(&run.sent).is_empty(), "{heard:?}");
            let recognizing = run.states.iter().position(|state| *state == VoiceState::Recognizing);
            assert_eq!(recognizing, Some(2), "{heard:?}: {:?}", run.states);
            assert!(!run.states.contains(&VoiceState::Answering), "{heard:?}: {:?}", run.states);
        }
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

    /// An echo cancellation that runs for as long as the returned guard lives: the reference comes all the
    /// time, as from a helper whose output is open.
    fn echo_running() -> (Echo, impl Drop) {
        echo_running_with(1)
    }

    /// [`echo_running`] with numbered lines, `step` apart: with 2, every other line goes missing.
    fn echo_running_with(step: u64) -> (Echo, impl Drop) {
        struct Stop(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let echo = Echo::default();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (pushed, stopped) = (echo.clone(), Arc::clone(&stop));
        echo.push_line(&[0; 160], Some(1));
        std::thread::spawn(move || {
            let mut number = 1;
            while !stopped.load(std::sync::atomic::Ordering::SeqCst) {
                number += step;
                pushed.push_line(&[0; 160], Some(number));
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        (echo, Stop(stop))
    }

    /// [`run`] with the echo cancellation `echo`. The tone takes 60 ms, so a reference comes after the
    /// microphone opened: the test would be over before the next one otherwise.
    fn run_with_echo(
        core: Core,
        files: &[&'static str],
        wake: Option<usize>,
        heard: &[&'static str],
        echo: Echo,
    ) -> Run {
        let says = Box::new(Says(heard.iter().copied().collect()));
        let ahead = Ahead { vad: None, early: None, limits: Limits::default(), echo: Some(echo), turn: None };
        run_parts(core, files, wake, says, Duration::from_millis(60), ahead)
    }

    /// Issue #834, done 2: while the echo cancellation runs, the microphone stays open as something is read
    /// aloud: the wake word is heard and the sentence sent, also when a reading began while it was said.
    #[test]
    fn with_the_echo_cancellation_the_wake_word_is_heard_while_something_is_read_aloud() {
        let (echo, _running) = echo_running();
        let reading = || Core { speaking: std::iter::repeat_n(true, 10_000).collect(), ..Core::new(on()) };
        // The files: the wake word, the microphone during the answer, the silence that ends the talk.
        let files = ["utterance.wav", "silence.wav", PAUSE];
        let run = run_with_echo(reading(), &files, Some(2), &["Hey Torro, what is on today?"], echo.clone());
        assert_eq!(messages(&run.sent).len(), 1, "heard while it reads: {:?}", run.states);
        assert_eq!(run.states[..2], [VoiceState::Listening, VoiceState::Recording]);
        assert_eq!(count(&run.sent, &Request::SpeakAnswer { conversation: "c9".into() }), 1);

        let began = Core { readings: [0, 0, 1].into(), ..reading() };
        let run = run_with_echo(began, &files, Some(2), &["Hey Torro, what is on today?"], echo);
        assert_eq!(messages(&run.sent).len(), 1, "the reading that began was taken out");
    }

    /// Issue #834 (open point of PR #885): a reading that began while the sentence was said keeps it only
    /// when its reference came whole; with lines missing the sentence is dropped, as without cancellation.
    #[test]
    fn a_sentence_over_a_reading_with_reference_missing_is_dropped() {
        let (echo, _running) = echo_running_with(2);
        let reading = || Core { speaking: std::iter::repeat_n(true, 10_000).collect(), ..Core::new(on()) };
        let files = ["utterance.wav", "silence.wav", PAUSE];
        let began = Core { readings: [0, 0, 1].into(), ..reading() };
        let run = run_with_echo(began, &files, Some(2), &["Hey Torro, what is on today?"], echo.clone());
        assert!(messages(&run.sent).is_empty(), "the reference had gaps: {:?}", run.sent);
        let run = run_with_echo(reading(), &files, Some(2), &["Hey Torro, what is on today?"], echo);
        assert_eq!(messages(&run.sent).len(), 1, "no reading began meanwhile: it counts");
    }

    /// Issue #834, decision 2: with the echo cancellation the microphone is open while the answer is read,
    /// once more than without it; without it nothing changes.
    #[test]
    fn with_the_echo_cancellation_the_microphone_is_open_during_the_answer() {
        let (echo, _running) = echo_running();
        let said = ["Hey Torro, what is on today?"];
        let without = run(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &said);
        assert_eq!(without.opened, 3, "the wake word, the talk, the wake word again");
        let with = run_with_echo(Core::new(on()), &["utterance.wav", "silence.wav", PAUSE], Some(2), &said, echo);
        assert_eq!(with.opened, 4, "and the answer");
        assert_eq!(messages(&with.sent).len(), 1);
        assert_eq!(count(&with.sent, &Request::EndTalk), 1, "the silence after it still ends the talk");
    }

    /// Issue #834: an echo cancellation that does not run (no reference comes) closes the microphone as
    /// before while something is read.
    #[test]
    fn an_echo_cancellation_that_does_not_run_closes_the_microphone_as_before() {
        let core = Core { speaking: [false, true].into(), ..Core::new(on()) };
        let run = run_with_echo(core, &["utterance.wav"], Some(2), &["Hey Torro, what is on today?"], Echo::default());
        assert!(messages(&run.sent).is_empty());
        assert_eq!(run.chimes, 0);
        assert_eq!(run.opened, 2, "closed, then open again once it was quiet");

        // Nor is it open during the listener's own answer.
        let said = ["Hey Torro, what is on today?"];
        let run = run_with_echo(Core::new(on()), &["utterance.wav", PAUSE], Some(2), &said, Echo::default());
        assert_eq!(run.opened, 3, "the wake word, the talk, the wake word again: not the answer");
        assert_eq!(messages(&run.sent).len(), 1);
    }

    /// Issue #834: the reader sends `assistant.speak_answer` on a thread of its own and tells when it was read.
    #[test]
    fn the_reader_reads_the_answer_on_a_thread_of_its_own() {
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let noted = Arc::clone(&asked);
        let reader: Reader = Arc::new(move |request| {
            noted.lock().expect("lock").push(request);
            Ok(Response::Ok)
        });
        let mut assistant = CoreAssistant::new(
            |_| Err(Refusal::new("forbidden", "not this way")),
            |_: &str| {},
            Duration::ZERO,
            Duration::ZERO,
        )
        .with_reader(reader);
        let done = assistant.start_answer("c9");
        assert_eq!(done.recv_timeout(Duration::from_secs(5)), Ok(Ok(())));
        assert_eq!(*asked.lock().expect("lock"), [Request::SpeakAnswer { conversation: "c9".into() }]);
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
            speaker: Box::new(Tone::default()),
            assistant: &mut assistant,
            report: Box::new(|_| {}),
            heard: Box::new(|_| {}),
            limits: Limits::default(),
            check_every: 10,
            pause: Duration::ZERO,
            floor: NoiseFloor::default(),
            vad: None,
            early: None,
            echo: None,
            turn: None,
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

    /// A core with the conversations "Voice" `voices` (id, minutes since their last entry, whether an answer
    /// runs) and the pause `new_after`, and the listener's way to it.
    #[allow(clippy::type_complexity)]
    fn with_voices(
        voices: &[(&str, i64, bool)],
        new_after: Option<u16>,
    ) -> (Rc<RefCell<Core>>, CoreAssistant<impl FnMut(Request) -> Result<Response, Refusal>, impl FnMut(&str)>) {
        let mut core = Core::new(Voice { new_after, ..on() });
        for (id, ago, running) in voices {
            let at = Utc::now() - chrono::TimeDelta::minutes(*ago);
            let voice = summary(id, voice_title(), "ai:openrouter", 14, 3, *running);
            core.conversations.push(ConversationSummary { created: at, updated: at, ..voice });
        }
        let core = Rc::new(RefCell::new(core));
        let ask = Rc::clone(&core);
        let assistant = CoreAssistant::new(
            move |request| ask.borrow_mut().answer(request),
            |_: &str| {},
            Duration::ZERO,
            Duration::ZERO,
        );
        (core, assistant)
    }

    fn renamed(sent: &[Request]) -> Vec<(String, String)> {
        sent.iter()
            .filter_map(|request| match request {
                Request::RenameConversation { id, title } => Some((id.clone(), title.clone())),
                _ => None,
            })
            .collect()
    }

    /// Issue #831: after the pause "Hey Torro" leaves "Voice" with its date in the title and starts a new
    /// "Voice"; the old one stays (a failed send: [`a_failed_send_keeps_the_old_voice_as_it_was`]).
    #[test]
    fn after_a_pause_the_wake_word_starts_a_new_voice_conversation() {
        let (core, mut assistant) = with_voices(&[("v1", 45, false)], None);
        let sent = assistant.send("What is on today?", None).expect("sent");
        assert_eq!(sent.conversation, "c9", "a new conversation");
        let core = core.borrow();
        let Some(Request::SendMessage { conversation, .. }) =
            core.sent.iter().find(|request| matches!(request, Request::SendMessage { .. }))
        else {
            panic!("nothing sent: {:?}", core.sent);
        };
        assert_eq!(*conversation, None);
        let renamed = renamed(&core.sent);
        assert_eq!(renamed.len(), 2, "{renamed:?}");
        assert_eq!(renamed[0].0, "v1");
        let dated = format!("{} · ", voice_title());
        assert!(renamed[0].1.starts_with(&dated) && renamed[0].1.len() == dated.len() + 16, "{renamed:?}");
        assert_eq!(renamed[1], ("c9".to_string(), voice_title().to_string()), "only the new one is \"Voice\"");
        assert!(core.conversations.iter().any(|summary| summary.id == "v1"), "the old one stays");
    }

    /// Issue #831: a send that fails after the pause leaves the old "Voice" as it was.
    #[test]
    fn a_failed_send_keeps_the_old_voice_as_it_was() {
        let (core, _) = with_voices(&[("v1", 45, false)], None);
        let ask = Rc::clone(&core);
        let mut assistant = CoreAssistant::new(
            move |request| match request {
                Request::SendMessage { .. } => Err(Refusal::new("offline", "the core is away")),
                other => ask.borrow_mut().answer(other),
            },
            |_: &str| {},
            Duration::ZERO,
            Duration::ZERO,
        );
        assert!(assistant.send("What is on today?", None).is_err());
        assert!(renamed(&core.borrow().sent).is_empty(), "{:?}", core.borrow().sent);
    }

    /// Issue #831: within the pause, with the pause off (0), or while an answer runs, "Voice" goes on; of two
    /// "Voice" the newest counts.
    #[test]
    fn within_the_pause_the_wake_word_goes_on_in_voice() {
        for (voices, new_after, expected) in [
            (vec![("v1", 10, false)], None, "v1"),
            (vec![("v1", 600, false)], Some(0), "v1"),
            (vec![("v1", 600, true)], None, "v1"),
            (vec![("v1", 300, false), ("v2", 5, false)], None, "v2"),
            (vec![("v1", 20, false)], Some(15), ""),
        ] {
            let (core, mut assistant) = with_voices(&voices, new_after);
            let ((id, _, _, _), retire) = assistant.target(None).expect("a target");
            assert_eq!(retire.is_some(), expected.is_empty(), "{voices:?}");
            assert_eq!(id.unwrap_or_default(), expected, "{voices:?} {new_after:?}");
            assert!(renamed(&core.borrow().sent).is_empty(), "nothing is renamed before the new one stands");
        }
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

    /// How [`Judges`] judges: by how often it was reset and the frame's number since then (from 1).
    type Judge = Box<dyn FnMut(usize, usize) -> bool>;

    /// A speech detector that judges by `judge`, counts its calls and fails at the call `fails_at`.
    struct Judges {
        judge: Judge,
        resets: usize,
        at: usize,
        calls: Rc<RefCell<usize>>,
        fails_at: Option<usize>,
    }

    impl Judges {
        fn new(judge: Judge, fails_at: Option<usize>) -> (Judges, Rc<RefCell<usize>>) {
            let calls = Rc::new(RefCell::new(0));
            (Judges { judge, resets: 0, at: 0, calls: Rc::clone(&calls), fails_at }, calls)
        }

        /// Judges the frames after the first reset by `script`, all others as silence.
        fn first(script: Vec<bool>) -> Judges {
            let judge = move |resets, at: usize| resets == 1 && script.get(at - 1).copied().unwrap_or(false);
            Judges::new(Box::new(judge), None).0
        }
    }

    impl SpeechDetector for Judges {
        fn speech(&mut self, _frame: &[i16]) -> Result<bool, VoiceError> {
            *self.calls.borrow_mut() += 1;
            if Some(*self.calls.borrow()) == self.fails_at {
                return Err(VoiceError::program("speech detection", "broke"));
            }
            self.at += 1;
            Ok((self.judge)(self.resets, self.at))
        }

        fn reset(&mut self) {
            self.resets += 1;
            self.at = 0;
        }
    }

    /// Understands `text` every time, from any thread, and keeps how much it was given.
    struct Counted {
        text: &'static str,
        given: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    }

    impl Counted {
        fn new(text: &'static str) -> (Counted, std::sync::Arc<std::sync::Mutex<Vec<usize>>>) {
            let given = std::sync::Arc::default();
            (Counted { text, given: std::sync::Arc::clone(&given) }, given)
        }
    }

    impl Transcriber for Counted {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            self.given.lock().expect("given").push(samples.len());
            Ok(self.text.to_string())
        }
    }

    fn given(counted: &std::sync::Mutex<Vec<usize>>) -> Vec<usize> {
        counted.lock().expect("given").clone()
    }

    /// The first sentence of a conversation from the window, judged by `script`, with the recognition
    /// ahead: what was sent, what was recognised now and what ahead.
    fn converse_ahead(script: Vec<bool>, limits: Limits) -> (Vec<String>, Vec<usize>, Vec<usize>) {
        converse_ahead_saying(script, limits, "recognised ahead")
    }

    /// [`converse_ahead`] with `text` recognised ahead. A text without words ("…") does not judge the end of
    /// the sentence (issue #836): the pause of the limits ends it.
    fn converse_ahead_saying(
        script: Vec<bool>,
        limits: Limits,
        text: &'static str,
    ) -> (Vec<String>, Vec<usize>, Vec<usize>) {
        let (now, said_now) = Counted::new("recognised now");
        let (ahead, said_ahead) = Counted::new(text);
        let vad = Judges::first(script);
        let parts = Ahead {
            vad: Some(Box::new(vad)),
            early: Some(Early::new(Box::new(ahead)).expect("thread")),
            limits,
            echo: None,
            turn: None,
        };
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let run = run_parts(core, &[PAUSE, PAUSE], None, Box::new(now), Duration::ZERO, parts);
        let texts = messages(&run.sent)
            .into_iter()
            .map(|request| match request {
                Request::SendMessage { text, .. } => text.clone(),
                _ => unreachable!(),
            })
            .collect();
        (texts, given(&said_now), given(&said_ahead))
    }

    /// Issue #833: a sentence whose end pause began with [`EARLY_AFTER`] silent frames was recognised
    /// ahead; at its end that text is sent, and nothing is recognised again.
    #[test]
    fn a_sentence_with_its_end_pause_is_recognised_only_ahead() {
        // Five frames of speech after the tone, then silence.
        let script = vec![true; 5];
        let (sent, now, ahead) = converse_ahead(script, Limits::default());
        assert_eq!(sent, ["recognised ahead"]);
        assert!(now.is_empty(), "recognised once, ahead: {now:?}");
        assert_eq!(ahead, [(1 + 5 + EARLY_AFTER) * FRAME], "up to the fourth silent frame");
    }

    /// Issue #833: speech after the pause withdraws what was recognised ahead; the sentence that then ends
    /// without a pause (at its longest) is recognised once it ended.
    #[test]
    fn speech_after_the_pause_is_recognised_at_the_end() {
        let script = [vec![true; 5], vec![false; EARLY_AFTER], vec![true; 5000]].concat();
        let limits = Limits { most: 10, ..Limits::default() };
        let (sent, now, ahead) = converse_ahead(script, limits);
        assert_eq!(sent, ["recognised now"]);
        assert_eq!(now.len(), 1);
        assert!(ahead.len() <= 1, "the offer of the pause at most: {ahead:?}");
    }

    /// Review of #833: the sentence that ends at its longest with a frame of speech after the offer does not
    /// take what was recognised ahead.
    #[test]
    fn speech_in_the_last_frame_withdraws_the_offer() {
        // In a conversation the longest sentence is the wait plus `most`: 3750 + 10 frames after the tone.
        let longest = TALK_WAIT_SECONDS * FRAMES_PER_SECOND + 10;
        let script = [vec![true; 5], vec![false; longest - 6], vec![true]].concat();
        let limits = Limits { most: 10, pause: 2 * longest, ..Limits::default() };
        // Issue #836: a text that judged the end would end it after the short pause, not at its longest.
        let (sent, now, ahead) = converse_ahead_saying(script, limits, "…");
        assert_eq!(sent, ["recognised now"], "ahead: {ahead:?}");
        assert_eq!(now.len(), 1);
    }

    /// The first round after the wake word at frame 2, judged by `judge`, with the recognition ahead: what was
    /// sent, what was recognised now and what ahead.
    fn wake_ahead(judge: Judge) -> (Vec<String>, Vec<usize>, Vec<usize>) {
        let (now, said_now) = Counted::new("Hey Torro, recognised now");
        let (ahead, said_ahead) = Counted::new("Hey Torro, recognised ahead");
        let (vad, _) = Judges::new(judge, None);
        let early = Some(Early::new(Box::new(ahead)).expect("thread"));
        let parts = Ahead { vad: Some(Box::new(vad)), early, limits: Limits::default(), echo: None, turn: None };
        let run = run_parts(Core::new(on()), &[PAUSE, PAUSE], Some(2), Box::new(now), Duration::ZERO, parts);
        let texts = messages(&run.sent)
            .into_iter()
            .map(|request| match request {
                Request::SendMessage { text, .. } => text.clone(),
                _ => unreachable!(),
            })
            .collect();
        (texts, given(&said_now), given(&said_ahead))
    }

    /// Review of PR #883: after the wake word the sentence judged by Silero VAD after the tone is recognised
    /// ahead in its end pause, and the wake word is taken off it.
    #[test]
    fn after_the_wake_word_the_sentence_is_recognised_ahead() {
        // The two frames before the wake word and the tone, then (after a reset) five frames of speech.
        let (sent, now, ahead) = wake_ahead(Box::new(|resets, at| resets == 2 && at <= 5));
        assert_eq!(sent, ["recognised ahead"]);
        assert!(now.is_empty(), "{now:?}");
        // The two frames from before, the tone, five of speech, four of silence.
        assert_eq!(ahead, [(2 + 1 + 5 + EARLY_AFTER) * FRAME]);
    }

    /// Review of PR #883: Silero VAD still holds the wake word for speech when it is heard; whether the person
    /// goes on is decided at the tone's end, so "Hey Torro" alone is not recognised at all.
    #[test]
    fn the_wake_word_alone_is_not_recognised() {
        // Speech in the two frames before the wake word only, none at the tone's end.
        let (sent, now, ahead) = wake_ahead(Box::new(|resets, at| resets == 1 && at <= 2));
        assert!(sent.is_empty());
        assert!(now.is_empty() && ahead.is_empty(), "nothing recognised: {now:?} {ahead:?}");

        // Still speaking at the tone's end: a command said in one breath is recognised.
        let (_, now, _) = wake_ahead(Box::new(|resets, at| resets == 1 && at <= 3));
        assert_eq!(now.len(), 1, "what came up to the tone's end");
    }

    /// Issue #833: a speech detector that fails is left for loudness: the sentence is heard and sent.
    #[test]
    fn a_failed_speech_detector_falls_back_to_loudness() {
        let (vad, calls) = Judges::new(Box::new(|_, _| true), Some(3));
        let parts = Ahead { vad: Some(Box::new(vad)), early: None, limits: Limits::default(), echo: None, turn: None };
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let says = Box::new(Says(["What is on today?"].into()));
        let run = run_parts(core, &[THINKING, PAUSE], None, says, Duration::ZERO, parts);
        assert_eq!(*calls.borrow(), 3, "not asked after it failed");
        let sent = messages(&run.sent);
        let Request::SendMessage { text, .. } = sent[0] else { unreachable!() };
        assert_eq!((sent.len(), text.as_str()), (1, "What is on today?"));
    }

    /// Recognises ahead the texts in order, the last one again and again, from its thread, and says each time
    /// it recognised (issue #836).
    struct Recognises {
        texts: VecDeque<&'static str>,
        done: mpsc::Sender<()>,
    }

    impl Transcriber for Recognises {
        fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
            let text = if self.texts.len() > 1 { self.texts.pop_front() } else { self.texts.front().copied() };
            let _ = self.done.send(());
            Ok(text.unwrap_or_default().to_string())
        }
    }

    /// A turn detector that hears `finished` (an error for `None`) and counts how often it was asked. It waits
    /// until the text ahead was recognised, so the listener judges by both, as when Whisper is quick: the test's
    /// frames come far faster than a microphone's.
    struct Hears {
        finished: Option<f32>,
        recognised: mpsc::Receiver<()>,
        asked: Rc<RefCell<usize>>,
    }

    impl TurnDetector for Hears {
        fn complete(&mut self, samples: &[i16]) -> Result<f32, VoiceError> {
            assert!(!samples.is_empty());
            *self.asked.borrow_mut() += 1;
            let _ = self.recognised.recv_timeout(Duration::from_secs(10));
            // The result is stored just after the recogniser returned.
            std::thread::sleep(Duration::from_millis(50));
            self.finished.ok_or_else(|| VoiceError::program("smart turn", "broke"))
        }
    }

    /// The first sentence of a conversation from the window, judged by `script` (Silero VAD), recognised ahead
    /// as `texts` and heard by a turn detector that hears `finished`: the run and how often the detector was
    /// asked.
    fn converse_judged(script: Vec<bool>, texts: &[&'static str], finished: Option<f32>) -> (Run, usize) {
        let (done, recognised) = mpsc::channel();
        let early = Early::new(Box::new(Recognises { texts: texts.iter().copied().collect(), done })).expect("thread");
        let asked = Rc::new(RefCell::new(0));
        let turn = Hears { finished, recognised, asked: Rc::clone(&asked) };
        let parts = Ahead {
            vad: Some(Box::new(Judges::first(script))),
            early: Some(early),
            turn: Some(Box::new(turn)),
            ..Ahead::default()
        };
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let says = Box::new(Says(["recognised now"].into()));
        let run = run_parts(core, &[PAUSE, PAUSE], None, says, Duration::ZERO, parts);
        let asked = *asked.borrow();
        (run, asked)
    }

    /// The texts sent.
    fn texts(run: &Run) -> Vec<String> {
        messages(&run.sent)
            .into_iter()
            .map(|request| match request {
                Request::SendMessage { text, .. } => text.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    /// Issue #836: a finished sentence ends after the short pause of the settings, not after a second.
    #[test]
    fn a_finished_sentence_ends_after_the_short_pause() {
        let pauses = Pauses::of(&Voice::default());
        for text in ["Wie wird das Wetter morgen?", "What is the weather tomorrow?"] {
            let (run, asked) = converse_judged(vec![true; 5], &[text], Some(0.9));
            assert_eq!(texts(&run), [text]);
            assert_eq!(asked, 1);
            assert_eq!(run.heard.len(), 1, "{:?}", run.heard);
            assert_eq!(run.heard[0].pause_ms, frames_ms(pauses.done), "{text}");
            assert!(frames_ms(pauses.done) < 1000 && !run.heard[0].held);
        }
    }

    /// Issue #836: "ähm" at the end waits the long pause, whatever the tone says; the person goes on after more
    /// than the old second, and the whole sentence is sent once, counted as waited out.
    #[test]
    fn an_unfinished_sentence_waits_the_long_pause() {
        let pauses = Pauses::of(&Voice::default());
        let (run, _) = converse_judged(vec![true; 5], &["Ich will morgen … ähm"], Some(0.9));
        assert_eq!(run.heard[0].pause_ms, frames_ms(pauses.open), "the long pause ended it");

        // 1.5 s of thinking: more than the second that ended every sentence before, less than the long pause.
        let thinking = FRAMES_PER_SECOND * 3 / 2;
        assert!(thinking > Limits::default().pause && thinking < pauses.open);
        let script = [vec![true; 5], vec![false; thinking], vec![true; 5]].concat();
        let texts_ahead = ["Ich will morgen … ähm", "Ich will morgen ähm ins Kino."];
        let (run, asked) = converse_judged(script, &texts_ahead, Some(0.9));
        assert_eq!(texts(&run), ["Ich will morgen ähm ins Kino."], "one sentence, sent whole");
        assert_eq!(asked, 2, "once in each pause");
        assert!(run.heard[0].held, "a pause that once ended it was waited out");
        assert_eq!(run.heard[0].pause_ms, frames_ms(pauses.done), "the finished sentence ended soon");
    }

    /// Issue #836: "Moment" holds the turn past the long pause; the question after it arrives in one sentence.
    #[test]
    fn wait_a_moment_holds_the_turn() {
        let pauses = Pauses::of(&Voice::default());
        // Eight seconds of silence after "Warte mal".
        let silent = 8 * FRAMES_PER_SECOND;
        assert!(silent > pauses.open && silent < pauses.hold);
        let script = [vec![true; 5], vec![false; silent], vec![true; 5]].concat();
        let texts_ahead = ["Warte mal.", "Warte mal. Wie wird das Wetter morgen?"];
        let (run, _) = converse_judged(script, &texts_ahead, Some(0.9));
        assert_eq!(texts(&run), ["Warte mal. Wie wird das Wetter morgen?"]);
        assert!(run.heard[0].held);
    }

    /// Issue #836: a hold the person does not go on after ends at its limit and sends nothing.
    #[test]
    fn a_hold_alone_is_not_sent() {
        let (run, _) = converse_judged(vec![true; 5], &["Moment"], Some(0.9));
        assert!(texts(&run).is_empty(), "{:?}", run.sent);
        assert!(run.heard.is_empty());
    }

    /// Issue #836: a turn detector that fails is left out, and the text decides alone.
    #[test]
    fn a_failed_turn_detector_leaves_the_text_to_decide() {
        let pauses = Pauses::of(&Voice::default());
        let (run, asked) = converse_judged(vec![true; 5], &["Wie wird das Wetter morgen?"], None);
        assert_eq!(asked, 1);
        assert_eq!(texts(&run), ["Wie wird das Wetter morgen?"]);
        assert_eq!(run.heard[0].pause_ms, frames_ms(pauses.done), "the rules found it finished");

        // Nor without Silero VAD is anything judged: the pause of a second ends the sentence, as before.
        let asked = Rc::new(RefCell::new(0));
        let (_, recognised) = mpsc::channel();
        let turn = Hears { finished: Some(0.9), recognised, asked: Rc::clone(&asked) };
        let parts = Ahead { turn: Some(Box::new(turn)), ..Ahead::default() };
        let core = Core { talk: talk_in(Some("a1")), ..Core::new(Voice::default()) };
        let says = Box::new(Says(["What is on today?"].into()));
        let run = run_parts(core, &["utterance.wav", PAUSE], None, says, Duration::ZERO, parts);
        assert_eq!(*asked.borrow(), 0);
        assert_eq!(run.heard[0].pause_ms, frames_ms(Limits::default().pause));
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
