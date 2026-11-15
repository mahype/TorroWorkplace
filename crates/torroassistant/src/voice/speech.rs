//! Reading aloud (issue #745, `docs/assistent.md` "Vorlesen"): `torroassistant --speak --voice
//! <settings>`, a helper the core keeps running while it reads (`assistant.speak`), so Pocket TTS loads
//! its model once and not for every answer. Ten minutes without a job it ends ([`IDLE`]).
//!
//! The core talks to it in JSON lines ([`Input`], [`Output`]):
//!
//! - `{"say": {"provider": "pocket", "text": …, "voice": "alba", "speed": 100, "pause": 0}}` reads
//!   `text` (Markdown, at most [`MOST_READ`] characters) with Pocket TTS on this computer (issue #762),
//!   at `speed` percent with `pause` extra milliseconds after each sentence (issue #782; missing for
//!   as before); `null` as the provider is Pocket too, a `model` or `language` from earlier versions
//!   is ignored. `"qwen"` reads with Qwen3-TTS on the graphics card in builds with `qwen-cuda` (issue #764), in
//!   its voice (`"warm_woman"`, `"calm_man"`), without speed and pause, which are Pocket's; other builds read it
//!   with Pocket TTS. With `"openai"` the helper
//!   only answers the pieces to fetch (`{"pieces": [...]}`, whole sentences of at most [`PIECE`]
//!   characters): the key stays in the core, which fetches the sound and sends it as
//!   `{"pcm": {"rate": 24000, "data": "<base64>"}}`, then `"end"`.
//! - `"stop"` ends what is read at once.
//! - The helper answers `"playing"` when the first sound plays, `"done"` when it was read to its
//!   end or stopped, `{"failed": {"code": …, "message": …}}` when it cannot read, with a message
//!   for people.
//!
//! Pocket TTS and Qwen3-TTS compute on a thread of their own ([`Synth`], [`super::pocket::Pocket`],
//! [`super::qwen`]), one at a time: when the other reads, the thread of the first ends, so Qwen's
//! graphics memory is free again. Each hands its sound out piece by piece; it plays at once on the system's output ([`Sound`], [`super::sound::Player`]), so
//! the first words sound after a few seconds while the rest is computed. A stop ends the sound at once
//! and tells the thread to stop computing. Neither text nor sound touch the disk.
//!
//! `torroassistant --speech-status --voice <settings>` says how Pocket TTS stands here ([`status`]) and,
//! in builds with Qwen3-TTS, it and the graphics card ([`qwen_option`]); the core adds OpenAI
//! (`settings.speech_status`).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};
use torro_core::i18n::tr;
use torro_core::settings::{
    POCKET_PAUSES, POCKET_RECOMMENDED, POCKET_SPEEDS, POCKET_VOICES, QWEN_RECOMMENDED, QWEN_VOICES, SpeechProvider,
    Voice,
};
use torro_core::voice_models::{POCKET_DE, QWEN, voice_folder};
use torro_ipc::{SpeechMissing, SpeechOption, SpeechStatus, SpeechVoice, VoiceMissing};

use super::VoiceError;
use super::audio::to_float;
use super::listen::speakable_up_to;
use super::parts::model_ready;
use super::pocket::{Delivery, Pocket, Synth};
use super::sound::{Devices, Player, Sound, SystemDevices};

/// The most of a text that is read, in characters: the window shows all of it.
pub const MOST_READ: usize = 3000;

/// The longest piece fetched from OpenAI at once, in characters: the next one loads while the first
/// plays, and a short piece starts sooner.
pub const PIECE: usize = 500;

/// How long the helper waits for a job before it ends.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How often a job looks whether its sound has ended.
const POLL: Duration = Duration::from_millis(50);

/// A line from the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Read a text.
    Say(Say),
    /// Sound fetched by the core (OpenAI): signed 16-bit mono at `rate`.
    Pcm { rate: u32, data: Vec<u8> },
    /// No more sound comes for this text.
    End,
    /// Stop reading now.
    Stop,
}

/// What to read and how ([`Input::Say`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Say {
    /// `None` for the core's choice, which is Pocket TTS here (#762).
    pub provider: Option<SpeechProvider>,
    pub text: String,
    /// The voice's id of the local provider (Pocket's or Qwen's); `None` for the one of the settings.
    pub voice: Option<String>,
    /// How Pocket speaks (issue #782); the default when the core sends none.
    pub delivery: Delivery,
}

impl Input {
    /// The line `line` from the core; `None` for one that does not read.
    pub fn parse(line: &str) -> Option<Input> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        if let Some(word) = value.as_str() {
            return match word {
                "end" => Some(Input::End),
                "stop" => Some(Input::Stop),
                _ => None,
            };
        }
        let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        if let Some(say) = value.get("say") {
            let provider = match say.get("provider") {
                None | Some(Value::Null) => None,
                Some(provider) => Some(serde_json::from_value(provider.clone()).ok()?),
            };
            let number = |key: &str, default: u16| {
                say.get(key).and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok()).unwrap_or(default)
            };
            let standard = Delivery::default();
            return Some(Input::Say(Say {
                provider,
                text: text(say, "text").unwrap_or_default(),
                voice: text(say, "voice"),
                delivery: Delivery {
                    // Within the ranges of the settings, whoever wrote the line.
                    speed: number("speed", standard.speed).clamp(POCKET_SPEEDS.0, POCKET_SPEEDS.1),
                    pause_ms: number("pause", standard.pause_ms).clamp(POCKET_PAUSES.0, POCKET_PAUSES.1),
                },
            }));
        }
        let pcm = value.get("pcm")?;
        let rate = u32::try_from(pcm.get("rate")?.as_u64()?).ok().filter(|rate| *rate > 0)?;
        let data = base64::engine::general_purpose::STANDARD.decode(pcm.get("data")?.as_str()?).ok()?;
        Some(Input::Pcm { rate, data })
    }

    /// The line for the helper, as the core writes it (`apps::SpeechHelper` in the core; here for the
    /// tests).
    #[cfg(test)]
    pub fn line(&self) -> String {
        match self {
            Input::Say(say) => json!({"say": {
                "provider": say.provider,
                "text": say.text,
                "voice": say.voice,
                "speed": say.delivery.speed,
                "pause": say.delivery.pause_ms,
            }})
            .to_string(),
            Input::Pcm { rate, data } => {
                json!({"pcm": {"rate": rate, "data": base64::engine::general_purpose::STANDARD.encode(data)}})
                    .to_string()
            }
            Input::End => json!("end").to_string(),
            Input::Stop => json!("stop").to_string(),
        }
    }
}

/// A line for the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// The pieces the core fetches from OpenAI, in order; none when there is nothing to read.
    Pieces(Vec<String>),
    /// The first sound plays.
    Playing,
    /// Read to its end, or stopped.
    Done,
    /// It cannot read: `code` as the core answers (`unsupported`, `error`), `message` for people.
    Failed { code: String, message: String },
}

impl Output {
    fn failed(code: &str, message: impl Into<String>) -> Output {
        Output::Failed { code: code.into(), message: message.into() }
    }

    /// The line as the helper writes it.
    pub fn line(&self) -> String {
        match self {
            Output::Pieces(pieces) => json!({"pieces": pieces}),
            Output::Playing => json!("playing"),
            Output::Done => json!("done"),
            Output::Failed { code, message } => json!({"failed": {"code": code, "message": message}}),
        }
        .to_string()
    }

    /// The line `line` from the helper, as the core reads it; `None` for one that does not read. For
    /// the tests: the core reads it itself.
    #[cfg(test)]
    pub fn parse(line: &str) -> Option<Output> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        match value.as_str() {
            Some("playing") => return Some(Output::Playing),
            Some("done") => return Some(Output::Done),
            Some(_) => return None,
            None => {}
        }
        if let Some(pieces) = value.get("pieces").and_then(Value::as_array) {
            return Some(Output::Pieces(pieces.iter().filter_map(Value::as_str).map(str::to_string).collect()));
        }
        let failed = value.get("failed")?;
        let field = |key: &str| failed.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        Some(Output::Failed { code: field("code"), message: field("message") })
    }
}

/// What may close a quote or an aside after the sign of a sentence: `„Ja.“ Dann`, `(siehe oben.)`.
const CLOSERS: [char; 12] = [')', ']', '}', '"', '\'', '“', '”', '’', '‘', '«', '»', '›'];

/// The sentences of `text`, each with its sign at the end, and the quotes or brackets that close
/// after it; the last may have none.
fn sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut sentence = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        sentence.push(c);
        if !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        while let Some(closer) = chars.next_if(|next| CLOSERS.contains(next)) {
            sentence.push(closer);
        }
        if chars.peek().is_none_or(|next| next.is_whitespace()) {
            sentences.push(std::mem::take(&mut sentence));
        }
    }
    sentences.push(sentence);
    sentences
}

/// Shortenings after which a sentence goes on, lower case and without their last point (issue #782).
/// Ones that often end a sentence ("etc.", English "no.") are left out: a pause too many there is
/// better than none where a sentence ends.
const ABBREVIATIONS: [&str; 33] = [
    "bzw", "ca", "ggf", "inkl", "exkl", "zzgl", "usw", "evtl", "vgl", "bspw", "sog", "mwst", "dr", "prof", "nr", "str",
    "tel", "jan", "feb", "apr", "jun", "jul", "aug", "sep", "sept", "okt", "nov", "dez", "mr", "mrs", "ms", "vs",
    "approx",
];

/// Months, lower case, after which "3." is a day and not the end of a sentence (issue #782).
const MONTHS: [&str; 24] = [
    "januar",
    "februar",
    "märz",
    "april",
    "mai",
    "juni",
    "juli",
    "august",
    "september",
    "oktober",
    "november",
    "dezember",
    "january",
    "february",
    "march",
    "may",
    "june",
    "july",
    "october",
    "december",
    "jan",
    "feb",
    "apr",
    "okt",
];

/// Whether `sentence` ends with a point that ends no sentence, `next` being the sentence after it: a
/// shortening with a point inside ("z.B.", "e.g.") or one of [`ABBREVIATIONS`] always; a single letter
/// or a number of one or two digits only when what follows shows it goes on – a small letter ("u. a."),
/// another single letter ("z. B.") or a month ("3. Mai"); the second letter of such a shortening
/// (`continued`: the sentence before was kept open) always. So "Plan A. Dann" and "Es sind 12. Danach"
/// end a sentence, and so do "Ja." and a year.
fn goes_on(sentence: &str, next: Option<&str>, continued: bool) -> bool {
    let Some(word) = sentence.trim_end().strip_suffix('.').and_then(|head| head.split_whitespace().last()) else {
        return false;
    };
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    if (word.contains('.') && !word.ends_with('.')) || ABBREVIATIONS.contains(&word.to_lowercase().as_str()) {
        return true;
    }
    let digits = !word.is_empty() && word.chars().all(|c| c.is_ascii_digit());
    let letter = word.chars().count() == 1 && word.chars().all(char::is_alphabetic);
    if letter && continued {
        return true;
    }
    let short = letter || (digits && word.len() <= 2);
    let Some(following) = next.and_then(|next| next.split_whitespace().next()) else { return false };
    let bare = following.trim_end_matches(|c: char| !c.is_alphanumeric());
    short
        && (following.chars().next().is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
            || (bare.chars().count() == 1 && following.ends_with('.'))
            || MONTHS.contains(&bare.to_lowercase().as_str()))
}

/// `text` with each sentence a piece of its own (issue #782: a pause falls after each), a sentence
/// longer than `most` characters cut as in [`pieces`], each piece with whether a sentence ends there:
/// only the last piece of a sentence does, wherever a long one is cut. A point that ends no sentence
/// ([`goes_on`]) keeps it open, so no pause falls inside one.
pub fn sentence_pieces(text: &str, most: usize) -> Vec<(String, bool)> {
    let mut whole: Vec<String> = Vec::new();
    let mut open = false;
    let split = sentences(text);
    for (number, sentence) in split.iter().enumerate() {
        match whole.last_mut() {
            Some(last) if open => last.push_str(sentence),
            _ => whole.push(sentence.clone()),
        }
        open = goes_on(sentence, split.get(number + 1).map(String::as_str), open);
    }
    whole
        .iter()
        .flat_map(|sentence| {
            let cut = pieces(sentence, most);
            let last = cut.len().saturating_sub(1);
            cut.into_iter().enumerate().map(move |(number, piece)| (piece, number == last))
        })
        .collect()
}

/// `text` in pieces of whole sentences, each at most `most` characters; a longer sentence is cut
/// between words.
pub fn pieces(text: &str, most: usize) -> Vec<String> {
    let sentences = sentences(text);
    let mut pieces: Vec<String> = Vec::new();
    let mut piece = String::new();
    let words = sentences.iter().flat_map(|sentence| {
        let sentence = sentence.split_whitespace().collect::<Vec<_>>().join(" ");
        // A sentence that fits stays whole; a longer one goes on word by word.
        match sentence.chars().count() <= most {
            true => vec![sentence],
            false => sentence.split(' ').map(str::to_string).collect(),
        }
    });
    for part in words.filter(|part| !part.is_empty()) {
        let joined = piece.chars().count() + usize::from(!piece.is_empty()) + part.chars().count();
        if joined > most && !piece.is_empty() {
            pieces.push(std::mem::take(&mut piece));
        }
        if !piece.is_empty() {
            piece.push(' ');
        }
        piece.push_str(&part);
        // A single word longer than a piece is cut where it must be.
        while piece.chars().count() > most {
            let head: String = piece.chars().take(most).collect();
            piece = piece.chars().skip(most).collect();
            pieces.push(head);
        }
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// How Pocket TTS stands here (`--speech-status`): what keeps it from reading (no sound output, then its
/// model not downloaded), its voices and its model. The core adds OpenAI.
pub fn status(voice: &Voice, folder: Option<&Path>, devices: &dyn Devices) -> SpeechStatus {
    let missing = if !devices.has_output() {
        Some(SpeechMissing::Player)
    } else if !model_ready(POCKET_DE, folder) {
        Some(SpeechMissing::Model)
    } else {
        None
    };
    let pocket = SpeechOption {
        provider: SpeechProvider::Pocket,
        missing,
        voices: POCKET_VOICES
            .iter()
            .map(|id| SpeechVoice { id: (*id).to_string(), recommended: *id == POCKET_RECOMMENDED })
            .collect(),
        model: Some(POCKET_DE.to_string()),
        setup: None,
    };
    SpeechStatus {
        gpu: None,
        fast_enough: true,
        recommended: SpeechProvider::Pocket,
        current: voice.speech_provider_here().unwrap_or(SpeechProvider::Pocket),
        providers: vec![pocket],
    }
}

/// Qwen3-TTS as it stands here (issue #764), after Pocket TTS in the status: what keeps it from reading
/// (no sound output, then no NVIDIA card, then its model not downloaded), its voices with "Warm woman"
/// recommended and its model. Only builds with `qwen-cuda` list it ([`system_status`]).
#[cfg_attr(
    all(not(feature = "qwen-cuda"), not(test)),
    expect(dead_code, reason = "only builds with Qwen3-TTS list it")
)]
pub fn qwen_option(folder: Option<&Path>, devices: &dyn Devices, card: bool) -> SpeechOption {
    let missing = if !devices.has_output() {
        Some(SpeechMissing::Player)
    } else if !card {
        Some(SpeechMissing::Gpu)
    } else if !model_ready(QWEN, folder) {
        Some(SpeechMissing::Model)
    } else {
        None
    };
    SpeechOption {
        provider: SpeechProvider::Qwen,
        missing,
        voices: QWEN_VOICES
            .iter()
            .map(|id| SpeechVoice { id: (*id).to_string(), recommended: *id == QWEN_RECOMMENDED })
            .collect(),
        model: Some(QWEN.to_string()),
        setup: None,
    }
}

/// One event of the helper's loop: a line from the core or what a local provider computed.
enum Event {
    Input(Input),
    /// The core closed the helper's input: it ended.
    Closed,
    /// Sound of the local job `id`, mono at `rate`.
    Chunk {
        id: u64,
        rate: u32,
        samples: Vec<f32>,
    },
    /// The local job `id` is computed to its end, or stopped.
    Computed {
        id: u64,
    },
    /// The local job `id` could not be computed.
    Failed {
        id: u64,
    },
    /// The thread of a local provider of this number ended: its model did not load, it broke down, or
    /// the other provider took its place.
    WorkerGone(u64),
}

/// A job for the thread of a local provider.
struct Spoken {
    id: u64,
    text: String,
    voice: String,
    delivery: Delivery,
}

/// Makes the [`Synth`] on its thread: [`Pocket::load`] or `Qwen::load` in the helper, a fake in the tests.
type MakeSynth = dyn Fn() -> Result<Box<dyn Synth>, VoiceError> + Send + Sync;

/// Opens the sound output for a job: [`Player::open`] in the helper, a fake in the tests.
type OpenSound = dyn FnMut() -> Result<Box<dyn Sound>, VoiceError>;

/// Sends [`Event::WorkerGone`] when the thread of a local provider ends, also when it breaks down.
struct Gone(Sender<Event>, u64);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = self.0.send(Event::WorkerGone(self.1));
    }
}

/// Starts the thread of a local provider numbered `number`: it makes the synth (loading the model takes
/// seconds), then computes one job after the other while `current` names it, and hands the sound to
/// `events`. It ends when the helper drops its sender, and with it the model.
fn start_worker(make: Arc<MakeSynth>, events: Sender<Event>, current: Arc<AtomicU64>, number: u64) -> Sender<Spoken> {
    let (jobs, inbox) = mpsc::channel::<Spoken>();
    std::thread::spawn(move || {
        let _gone = Gone(events.clone(), number);
        let mut synth = match make() {
            Ok(synth) => synth,
            // The error names the provider.
            Err(err) => return eprintln!("torroassistant: {err}"),
        };
        let rate = synth.rate();
        for job in inbox {
            // A job stopped before its turn is not computed at all.
            if current.load(Ordering::SeqCst) != job.id {
                continue;
            }
            let (to, now, id) = (events.clone(), Arc::clone(&current), job.id);
            let chunk = Box::new(move |samples: &[f32]| {
                now.load(Ordering::SeqCst) == id
                    && to.send(Event::Chunk { id, rate, samples: samples.to_vec() }).is_ok()
            });
            let event = match synth.speak(&job.text, &job.voice, job.delivery, chunk) {
                Ok(()) => Event::Computed { id },
                Err(err) => {
                    // The error names the part, never the text.
                    eprintln!("torroassistant: {err}");
                    Event::Failed { id }
                }
            };
            if events.send(event).is_err() {
                return;
            }
        }
    });
    jobs
}

/// The job that reads now.
enum Job {
    Idle,
    /// A local provider (Pocket TTS, Qwen3-TTS) computes the job `id` and plays it.
    Local {
        id: u64,
        sound: Option<Box<dyn Sound>>,
        computed: bool,
    },
    /// The core sends OpenAI's sound.
    Stream {
        sound: Option<Box<dyn Sound>>,
        ended: bool,
    },
}

/// The helper's state: where the models are, the thread of the local provider and the job.
pub struct Helper<W: FnMut(Output)> {
    folder: Option<PathBuf>,
    /// The Pocket voice read with when the core names none: the one of the settings.
    voice: String,
    open_sound: Box<OpenSound>,
    make_synth: Arc<MakeSynth>,
    /// The Qwen voice read with when the core names none ([`Helper::with_qwen`]).
    qwen_voice: String,
    /// Makes Qwen3-TTS; `None` in a build without it (issue #764).
    make_qwen: Option<Arc<MakeSynth>>,
    /// The thread of the local provider with its number and the provider it reads with; `None` until the
    /// first job and after it ended.
    worker: Option<(u64, SpeechProvider, Sender<Spoken>)>,
    /// Counts the threads started, so the end of one replaced meanwhile fails no job of the next.
    workers: u64,
    /// The local job that may compute; 0 for none. The thread looks at it between two pieces of sound.
    current: Arc<AtomicU64>,
    /// The id of the last local job.
    next: u64,
    job: Job,
    write: W,
}

impl<W: FnMut(Output)> Helper<W> {
    /// A helper with the models in the voice folder `folder`, reading in `voice` when the core names
    /// none, its sound going to `open_sound`, Pocket TTS made by `make_synth`, its answers to `write`.
    pub fn new(
        folder: Option<PathBuf>,
        voice: &str,
        open_sound: Box<OpenSound>,
        make_synth: Arc<MakeSynth>,
        write: W,
    ) -> Helper<W> {
        Helper {
            folder,
            voice: voice.to_string(),
            open_sound,
            make_synth,
            qwen_voice: QWEN_RECOMMENDED.to_string(),
            make_qwen: None,
            worker: None,
            workers: 0,
            current: Arc::new(AtomicU64::new(0)),
            next: 0,
            job: Job::Idle,
            write,
        }
    }

    /// The helper reading with Qwen3-TTS too (issue #764), made by `make_qwen`, in `voice` when the core
    /// names none.
    #[cfg_attr(
        all(not(feature = "qwen-cuda"), not(test)),
        expect(dead_code, reason = "only builds with Qwen3-TTS read with it")
    )]
    pub fn with_qwen(mut self, voice: &str, make_qwen: Arc<MakeSynth>) -> Helper<W> {
        self.qwen_voice = voice.to_string();
        self.make_qwen = Some(make_qwen);
        self
    }

    /// Runs until the core closes the input or [`IDLE`] passes without a job.
    fn run(&mut self, events: &Receiver<Event>, sender: &Sender<Event>) {
        loop {
            let idle = matches!(self.job, Job::Idle);
            match events.recv_timeout(if idle { IDLE } else { POLL }) {
                Ok(Event::Closed) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) if idle => return,
                Err(RecvTimeoutError::Timeout) => {}
                Ok(event) => self.handle(event, sender),
            }
            self.look_at_the_end();
        }
    }

    fn handle(&mut self, event: Event, sender: &Sender<Event>) {
        match event {
            Event::Input(Input::Stop) => {
                if !matches!(self.job, Job::Idle) {
                    self.stop();
                    (self.write)(Output::Done);
                }
            }
            Event::Input(Input::Say(say)) => {
                if !matches!(self.job, Job::Idle) {
                    self.stop();
                    (self.write)(Output::Done);
                }
                self.say(say, sender);
            }
            Event::Input(Input::Pcm { rate, data }) => {
                let samples: Vec<f32> =
                    data.as_chunks::<2>().0.iter().map(|pair| to_float(i16::from_le_bytes(*pair))).collect();
                if matches!(self.job, Job::Stream { ended: false, .. }) {
                    self.play(rate, &samples);
                }
            }
            Event::Input(Input::End) => {
                if let Job::Stream { sound, ended } = &mut self.job {
                    *ended = true;
                    match sound {
                        Some(sound) => sound.finish(),
                        None => self.finished(),
                    }
                }
            }
            Event::Chunk { id, rate, samples } => {
                if matches!(self.job, Job::Local { id: now, .. } if now == id) {
                    self.play(rate, &samples);
                }
            }
            Event::Computed { id } => {
                if let Job::Local { id: now, sound, computed } = &mut self.job
                    && *now == id
                {
                    *computed = true;
                    match sound {
                        Some(sound) => sound.finish(),
                        // Nothing to hear in the text (only signs): it is read.
                        None => self.finished(),
                    }
                }
            }
            Event::Failed { id } => {
                if matches!(self.job, Job::Local { id: now, .. } if now == id) {
                    self.fail(Output::failed("error", tr("Reading aloud failed. Try again.")));
                }
            }
            Event::WorkerGone(number) => {
                if self.worker.as_ref().is_none_or(|(now, _, _)| *now != number) {
                    return;
                }
                self.worker = None;
                if matches!(self.job, Job::Local { .. }) {
                    self.fail(Output::failed("error", tr("Reading aloud failed. Try again.")));
                }
            }
            Event::Closed => {}
        }
    }

    /// Plays `samples` at `rate` in the job's sound, opened with the first of them.
    fn play(&mut self, rate: u32, samples: &[f32]) {
        let (Job::Local { sound, .. } | Job::Stream { sound, .. }) = &mut self.job else { return };
        if sound.is_none() {
            match (self.open_sound)() {
                Ok(opened) => *sound = Some(opened),
                Err(err) => return self.fail(failed_to_play(&err)),
            }
            (self.write)(Output::Playing);
        }
        if let Some(sound) = sound {
            sound.push(rate, samples);
        }
    }

    /// Starts reading `say`.
    fn say(&mut self, say: Say, sender: &Sender<Event>) {
        let text = speakable_up_to(&say.text, MOST_READ);
        match say.provider.unwrap_or(SpeechProvider::Pocket) {
            SpeechProvider::OpenAi => {
                (self.write)(Output::Pieces(pieces(&text, PIECE)));
                self.job = Job::Stream { sound: None, ended: false };
            }
            _ if text.is_empty() => (self.write)(Output::Done),
            local => {
                let voice = say.voice.unwrap_or_else(|| match local {
                    SpeechProvider::Qwen => self.qwen_voice.clone(),
                    _ => self.voice.clone(),
                });
                match self.ask_local(local, text, voice, say.delivery, sender) {
                    Ok(id) => self.job = Job::Local { id, sound: None, computed: false },
                    Err(failed) => (self.write)(failed),
                }
            }
        }
    }

    /// Gives `text` to the thread of the local provider `provider`, started first; the job's id. The thread
    /// of the other one ends first: only one model is loaded at a time.
    fn ask_local(
        &mut self,
        provider: SpeechProvider,
        text: String,
        voice: String,
        delivery: Delivery,
        sender: &Sender<Event>,
    ) -> Result<u64, Output> {
        let (make, model) = match provider {
            SpeechProvider::Qwen => match &self.make_qwen {
                Some(make) => (Arc::clone(make), QWEN),
                None => return Err(Output::failed("unsupported", tr("This build has no Qwen3-TTS."))),
            },
            _ => (Arc::clone(&self.make_synth), POCKET_DE),
        };
        if !model_ready(model, self.folder.as_deref()) {
            return Err(not_downloaded());
        }
        if self.worker.as_ref().is_some_and(|(_, reads, _)| *reads != provider) {
            // Dropping its sender ends the thread once it is between two pieces, and frees its model.
            self.worker = None;
        }
        self.next += 1;
        let id = self.next;
        self.current.store(id, Ordering::SeqCst);
        let mut job = Spoken { id, text, voice, delivery };
        // A thread that broke down is started again, once.
        for _ in 0..2 {
            if self.worker.is_none() {
                self.workers += 1;
                let jobs = start_worker(Arc::clone(&make), sender.clone(), Arc::clone(&self.current), self.workers);
                self.worker = Some((self.workers, provider, jobs));
            }
            let Some((_, _, worker)) = &self.worker else { break };
            match worker.send(job) {
                Ok(()) => return Ok(id),
                Err(mpsc::SendError(returned)) => {
                    job = returned;
                    self.worker = None;
                }
            }
        }
        Err(Output::failed("error", tr("Reading aloud failed. Try again.")))
    }

    /// Ends what is read now: the sound stops at once, the local provider stops computing.
    fn stop(&mut self) {
        self.current.store(0, Ordering::SeqCst);
        // Dropping the sound stops it.
        self.job = Job::Idle;
    }

    fn fail(&mut self, failed: Output) {
        self.stop();
        (self.write)(failed);
    }

    fn finished(&mut self) {
        self.stop();
        (self.write)(Output::Done);
    }

    /// A job whose sound is all there and has played is done.
    fn look_at_the_end(&mut self) {
        let done = match &self.job {
            Job::Local { sound: Some(sound), computed: true, .. } | Job::Stream { sound: Some(sound), ended: true } => {
                sound.ended()
            }
            _ => false,
        };
        if done {
            self.finished();
        }
    }
}

/// Why the sound output did not open, for people.
fn failed_to_play(err: &VoiceError) -> Output {
    match err {
        VoiceError::Missing(_, VoiceMissing::Player) => Output::failed("unsupported", tr("No sound output was found.")),
        err => {
            eprintln!("torroassistant: sound output: {err}");
            Output::failed("error", tr("Reading aloud failed. Try again."))
        }
    }
}

fn not_downloaded() -> Output {
    Output::failed("unsupported", tr("Download the voice in Settings › Assistant › Voice first."))
}

/// The helper (see the module) for `voice`, on standard input and output.
pub fn serve(voice: Voice) {
    let folder = voice_folder();
    let (sender, events) = mpsc::channel();
    let lines = sender.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if let Some(input) = Input::parse(&line)
                && lines.send(Event::Input(input)).is_err()
            {
                return;
            }
        }
        let _ = lines.send(Event::Closed);
    });
    let model = folder.clone();
    let make: Arc<MakeSynth> = Arc::new(move || {
        let folder = model.as_deref().ok_or(VoiceError::Missing(torro_ipc::VoicePart::Speech, VoiceMissing::Model))?;
        Ok(Box::new(Pocket::load(folder)?) as Box<dyn Synth>)
    });
    let open: Box<OpenSound> = Box::new(|| Ok(Box::new(Player::open()?) as Box<dyn Sound>));
    let helper = Helper::new(folder.clone(), voice.pocket_voice_or_default(), open, make, |output: Output| {
        let mut out = std::io::stdout();
        // When the core no longer reads, it ended; so does the helper.
        if writeln!(out, "{}", output.line()).and_then(|()| out.flush()).is_err() {
            std::process::exit(0);
        }
    });
    #[cfg(feature = "qwen-cuda")]
    let helper = {
        let code = voice.language_or_default().to_string();
        let make_qwen: Arc<MakeSynth> = Arc::new(move || {
            let folder =
                folder.as_deref().ok_or(VoiceError::Missing(torro_ipc::VoicePart::Speech, VoiceMissing::Model))?;
            Ok(Box::new(super::qwen::Qwen::load(folder, &code)?) as Box<dyn Synth>)
        });
        helper.with_qwen(voice.qwen_voice_or_default(), make_qwen)
    };
    let mut helper = helper;
    helper.run(&events, &sender);
}

/// The status of Pocket TTS on this system, for `--speech-status`; with Qwen3-TTS and the graphics card in
/// builds with `qwen-cuda` (issue #764).
pub fn system_status(voice: &Voice) -> SpeechStatus {
    let folder = voice_folder();
    #[cfg_attr(not(feature = "qwen-cuda"), expect(unused_mut, reason = "only Qwen3-TTS adds to it"))]
    let mut status = status(voice, folder.as_deref(), &SystemDevices);
    #[cfg(feature = "qwen-cuda")]
    {
        let card = super::qwen::graphics_card();
        status.providers.push(qwen_option(folder.as_deref(), &SystemDevices, card.is_some()));
        status.gpu = card;
    }
    status
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;

    use super::super::parts::tests::download;
    use super::super::pocket::Chunk;
    use super::super::sound::tests::FakeDevices;
    use super::*;

    #[test]
    fn lines_go_both_ways() {
        let say = Input::Say(Say {
            provider: Some(SpeechProvider::Pocket),
            text: "Hello.".into(),
            voice: Some("marius".into()),
            delivery: Delivery { speed: 130, pause_ms: 500 },
        });
        for input in [say, Input::Pcm { rate: 24_000, data: vec![1, 2, 3, 4] }, Input::End, Input::Stop] {
            assert_eq!(Input::parse(&input.line()), Some(input.clone()), "{}", input.line());
        }
        assert_eq!(Input::parse(r#"{"pcm": {"rate": 0, "data": ""}}"#), None, "no rate");
        assert_eq!(Input::parse("nope"), None);
        assert_eq!(Input::line(&Input::Stop), "\"stop\"");
        for output in [
            Output::Pieces(vec!["One.".into(), "Two.".into()]),
            Output::Playing,
            Output::Done,
            Output::failed("unsupported", "No player"),
        ] {
            assert_eq!(Output::parse(&output.line()), Some(output.clone()), "{}", output.line());
        }
        assert_eq!(Output::line(&Output::Done), "\"done\"");
    }

    /// Issue #762: a line of an earlier core with a model and a language still reads; `null` is Pocket's.
    #[test]
    fn old_fields_are_ignored() {
        let line = r#"{"say": {"provider": null, "text": "Hi", "voice": "alba", "model": "x", "language": "German"}}"#;
        assert_eq!(
            Input::parse(line),
            Some(Input::Say(Say {
                provider: None,
                text: "Hi".into(),
                voice: Some("alba".into()),
                delivery: Delivery::default(),
            }))
        );
        assert_eq!(Input::parse(r#"{"say": {"provider": "piper", "text": "Hi"}}"#), None, "a provider that is gone");
        // Qwen3-TTS reads again (#764); a build without it answers so when it is asked to read.
        let qwen = Input::parse(r#"{"say": {"provider": "qwen", "text": "Hi", "voice": "calm_man"}}"#);
        assert_eq!(
            qwen,
            Some(Input::Say(Say {
                provider: Some(SpeechProvider::Qwen),
                text: "Hi".into(),
                voice: Some("calm_man".into())
            }))
        );
    }

    #[test]
    fn pieces_are_whole_sentences_up_to_the_most() {
        assert_eq!(pieces("One. Two!  Three?", 10), ["One. Two!", "Three?"]);
        assert_eq!(pieces("One. Two. Three.", 500), ["One. Two. Three."]);
        assert!(pieces("", 500).is_empty());
        assert_eq!(pieces("3.5 apples", 500), ["3.5 apples"], "a point inside a number ends nothing");
        let long = "word ".repeat(40);
        let cut = pieces(&long, 50);
        assert!(cut.iter().all(|piece| piece.chars().count() <= 50 && !piece.ends_with(' ')), "{cut:?}");
        assert_eq!(cut.join(" "), long.trim());
        assert_eq!(pieces(&"x".repeat(12), 5), ["xxxxx", "xxxxx", "xx"]);
    }

    /// Issue #782: with a pause every sentence is a piece of its own; a long one is still cut.
    #[test]
    fn sentence_pieces_are_one_sentence_each() {
        let texts = |text: &str, most| -> Vec<String> {
            sentence_pieces(text, most).into_iter().map(|(piece, _)| piece).collect()
        };
        assert_eq!(texts("One. Two!  Three?", 500), ["One.", "Two!", "Three?"]);
        assert!(sentence_pieces("", 500).is_empty());
        assert!(sentence_pieces("One. Two!", 500).iter().all(|(_, ends)| *ends), "each ends a sentence");
        let long = format!("{}. Short.", "word ".repeat(40).trim());
        let cut = sentence_pieces(&long, 50);
        assert!(cut.len() > 2 && cut.iter().all(|(piece, _)| piece.chars().count() <= 50), "{cut:?}");
        assert_eq!(cut.last(), Some(&("Short.".to_string(), true)));
        let ends: Vec<_> = cut.iter().filter(|(_, ends)| *ends).map(|(piece, _)| piece.as_str()).collect();
        assert_eq!(ends.len(), 2, "only the end of each sentence, not where a long one is cut: {cut:?}");
        assert_eq!(
            texts("Am 3. Mai kommt z. B. Anna. Gut!", 500),
            ["Am 3. Mai kommt z. B. Anna.", "Gut!"],
            "no sentence ends after a day or a single letter"
        );
        assert_eq!(texts("Ja. Das war 2024. Gut.", 500), ["Ja.", "Das war 2024.", "Gut."]);
        // Review of PR #806: a short word or number at the end of a sentence ends it when a sentence follows.
        assert_eq!(
            texts("Es sind 12. Danach kommt Plan A. Dann Ende.", 500),
            ["Es sind 12.", "Danach kommt Plan A.", "Dann Ende."]
        );
        assert_eq!(texts("Er kam u. a. am 12. Oktober. Ja.", 500), ["Er kam u. a. am 12. Oktober.", "Ja."]);
        assert_eq!(texts("Wir sind 3. Und fertig.", 500), ["Wir sind 3.", "Und fertig."]);
    }

    /// Issue #782 (review): shortenings keep a sentence open, also with a point inside; a sentence
    /// closed by a quote or a bracket ends after it; a long sentence cut after "3." has its pause
    /// only at its end.
    #[test]
    fn shortenings_quotes_and_cuts_keep_the_pause_at_the_end() {
        let texts =
            |text: &str| -> Vec<String> { sentence_pieces(text, 500).into_iter().map(|(piece, _)| piece).collect() };
        assert_eq!(
            texts("Das kostet ca. 5 € inkl. MwSt. pro Monat. Gut."),
            ["Das kostet ca. 5 € inkl. MwSt. pro Monat.", "Gut."]
        );
        assert_eq!(
            texts("Etwa z.B. morgen, bzw. am Okt. danach. Ja."),
            ["Etwa z.B. morgen, bzw. am Okt. danach.", "Ja."]
        );
        assert_eq!(texts("Er sagte „Ja.“ Dann ging er."), ["Er sagte „Ja.“", "Dann ging er."]);
        assert_eq!(texts("Siehe oben (wie gesagt.) Weiter!"), ["Siehe oben (wie gesagt.)", "Weiter!"]);
        assert_eq!(pieces("Er sagte „Ja.“ Dann ging er.", 500), ["Er sagte „Ja.“ Dann ging er."]);
        let long = format!("{} am 3. Mai {}.", "wort ".repeat(30).trim(), "wort ".repeat(30).trim());
        let cut = sentence_pieces(&long, 160);
        assert!(cut.len() > 1, "{cut:?}");
        assert_eq!(cut.iter().filter(|(_, ends)| *ends).count(), 1, "{cut:?}");
        assert!(cut.last().is_some_and(|(_, ends)| *ends));
    }

    /// What a fake sound output played.
    #[derive(Debug, Default)]
    struct Played {
        opened: usize,
        samples: Vec<(u32, f32)>,
        finished: bool,
        /// The output says it has ended once it is finished and this is set.
        drained: bool,
    }

    struct FakeSound(Rc<RefCell<Played>>);

    impl Sound for FakeSound {
        fn push(&mut self, rate: u32, samples: &[f32]) {
            self.0.borrow_mut().samples.extend(samples.iter().map(|sample| (rate, *sample)));
        }

        fn finish(&mut self) {
            self.0.borrow_mut().finished = true;
        }

        fn ended(&self) -> bool {
            let played = self.0.borrow();
            played.finished && played.drained
        }
    }

    /// Reads a word as one piece of sound per letter, a value per letter, waiting `pause` before each; stops
    /// when told.
    struct FakeSynth {
        pause: Duration,
        /// How each job was to be spoken.
        delivered: Arc<std::sync::Mutex<Vec<Delivery>>>,
    }

    impl Synth for FakeSynth {
        fn rate(&self) -> u32 {
            24_000
        }

        fn speak(&mut self, text: &str, voice: &str, delivery: Delivery, mut chunk: Chunk) -> Result<(), VoiceError> {
            self.delivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(delivery);
            if voice == "broken" {
                return Err(VoiceError::program("fake", "broken"));
            }
            for (i, _) in text.chars().enumerate() {
                std::thread::sleep(self.pause);
                if !chunk(&[i as f32]) {
                    return Ok(());
                }
            }
            Ok(())
        }
    }

    /// What the helper answered.
    type Said = Rc<RefCell<Vec<Output>>>;

    /// How the fake synth was told to speak, job by job.
    type Delivered = Arc<std::sync::Mutex<Vec<Delivery>>>;

    /// A helper with the voice folder `folder`, a sound output if `output`, the fake synth.
    fn helper(
        folder: Option<&Path>,
        output: bool,
        pause: Duration,
    ) -> (Helper<impl FnMut(Output)>, Said, Rc<RefCell<Played>>) {
        helper_telling(folder, output, pause, Delivered::default())
    }

    /// [`helper`] with the fake synth noting in `delivered` how it was told to speak.
    fn helper_telling(
        folder: Option<&Path>,
        output: bool,
        pause: Duration,
        delivered: Delivered,
    ) -> (Helper<impl FnMut(Output)>, Said, Rc<RefCell<Played>>) {
        let said = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&said);
        let played = Rc::new(RefCell::new(Played::default()));
        let sound = Rc::clone(&played);
        let open: Box<OpenSound> = Box::new(move || {
            if !output {
                return Err(VoiceError::Missing(torro_ipc::VoicePart::Speech, VoiceMissing::Player));
            }
            sound.borrow_mut().opened += 1;
            Ok(Box::new(FakeSound(Rc::clone(&sound))) as Box<dyn Sound>)
        });
        let make: Arc<MakeSynth> =
            Arc::new(move || Ok(Box::new(FakeSynth { pause, delivered: Arc::clone(&delivered) }) as Box<dyn Synth>));
        let helper =
            Helper::new(folder.map(Path::to_path_buf), "alba", open, make, move |output| log.borrow_mut().push(output));
        (helper, said, played)
    }

    fn say(provider: SpeechProvider, text: &str) -> Event {
        Event::Input(Input::Say(Say {
            provider: Some(provider),
            text: text.into(),
            voice: None,
            delivery: Delivery::default(),
        }))
    }

    /// Feeds what Pocket TTS sends to the helper until `until` holds, ten seconds at most.
    fn pump<W: FnMut(Output)>(
        helper: &mut Helper<W>,
        events: &Receiver<Event>,
        sender: &Sender<Event>,
        until: impl Fn(&Helper<W>) -> bool,
    ) {
        let started = Instant::now();
        while !until(helper) && started.elapsed() < Duration::from_secs(10) {
            if let Ok(event) = events.recv_timeout(POLL) {
                helper.handle(event, sender);
            }
            helper.look_at_the_end();
        }
    }

    fn codes(said: &Said) -> Vec<String> {
        said.borrow()
            .iter()
            .map(|output| match output {
                Output::Failed { code, message } => format!("{code}: {}", !message.is_empty()),
                other => other.line(),
            })
            .collect()
    }

    /// Issue #762: without the model Pocket says to download it; nothing to read is done at once.
    #[test]
    fn what_is_missing_is_said_for_people() {
        let (sender, _events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        let (mut helper, said, _) = helper(Some(folder.path()), true, Duration::ZERO);
        helper.handle(say(SpeechProvider::Pocket, "Hello."), &sender);
        helper.handle(say(SpeechProvider::Pocket, "  "), &sender);
        assert_eq!(codes(&said), ["unsupported: true", "\"done\""]);
        assert_eq!(said.borrow()[0], not_downloaded());
        assert!(matches!(helper.job, Job::Idle));
        let (mut without_folder, said, _) = super::tests::helper(None, true, Duration::ZERO);
        without_folder.handle(say(SpeechProvider::Pocket, "Hello."), &sender);
        assert_eq!(said.borrow().as_slice(), [not_downloaded()]);
    }

    #[test]
    fn openai_gets_its_pieces_and_ends_with_its_sound() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said, played) = helper(None, true, Duration::ZERO);
        helper.handle(say(SpeechProvider::OpenAi, "**One.** Two."), &sender);
        assert_eq!(said.borrow().as_slice(), [Output::Pieces(vec!["One. Two.".into()])]);
        // No sound came: the end is done at once.
        helper.handle(Event::Input(Input::End), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Done));

        // The sound plays as it comes and is done once it has played.
        helper.handle(say(SpeechProvider::OpenAi, "Three."), &sender);
        let data = [16_384i16, -16_384].iter().flat_map(|sample| sample.to_le_bytes()).collect();
        helper.handle(Event::Input(Input::Pcm { rate: 24_000, data }), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Playing));
        assert_eq!(played.borrow().samples, [(24_000, 0.5), (24_000, -0.5)]);
        helper.handle(Event::Input(Input::End), &sender);
        helper.look_at_the_end();
        assert_eq!(said.borrow().last(), Some(&Output::Playing), "still playing");
        played.borrow_mut().drained = true;
        helper.look_at_the_end();
        assert_eq!(said.borrow().last(), Some(&Output::Done), "played to its end");
    }

    /// Issue #762: sound without an output fails with a message, and stopping afterwards says nothing more.
    #[test]
    fn no_sound_output_is_said_for_people() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said, _) = helper(None, false, Duration::ZERO);
        helper.handle(say(SpeechProvider::OpenAi, "Three."), &sender);
        helper.handle(Event::Input(Input::Pcm { rate: 24_000, data: vec![0; 8] }), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::failed("unsupported", tr("No sound output was found."))));
        let count = said.borrow().len();
        helper.handle(Event::Input(Input::Stop), &sender);
        assert_eq!(said.borrow().len(), count, "nothing reads");
    }

    /// Issue #762: Pocket's sound plays piece by piece while it is computed, and the job is done once it has
    /// played.
    #[test]
    fn pocket_plays_while_it_computes() {
        let (sender, events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), POCKET_DE);
        let (mut helper, said, played) = helper(Some(folder.path()), true, Duration::ZERO);
        helper.handle(say(SpeechProvider::Pocket, "Hallo."), &sender);
        pump(&mut helper, &events, &sender, |helper| matches!(helper.job, Job::Local { computed: true, .. }));
        assert_eq!(said.borrow().as_slice(), [Output::Playing]);
        assert_eq!(played.borrow().samples.len(), "Hallo.".len(), "every piece");
        assert!(played.borrow().samples.iter().all(|(rate, _)| *rate == 24_000));
        assert!(played.borrow().finished);
        played.borrow_mut().drained = true;
        helper.look_at_the_end();
        assert_eq!(said.borrow().last(), Some(&Output::Done));
    }

    /// Issue #782: the speed and pause of a `say` reach Pocket TTS; a `say` without them speaks as before.
    #[test]
    fn the_delivery_reaches_pocket() {
        let (sender, events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), POCKET_DE);
        let delivered = Delivered::default();
        let (mut helper, said, _) = helper_telling(Some(folder.path()), true, Duration::ZERO, Arc::clone(&delivered));
        let line = r#"{"say": {"provider": "pocket", "text": "Hallo.", "voice": "alba", "speed": 130, "pause": 500}}"#;
        let plain = r#"{"say": {"provider": "pocket", "text": "Ja.", "voice": "alba"}}"#;
        for line in [line, plain] {
            helper.handle(Event::Input(Input::parse(line).expect("a say")), &sender);
            pump(&mut helper, &events, &sender, |helper| matches!(helper.job, Job::Pocket { computed: true, .. }));
        }
        assert!(said.borrow().contains(&Output::Playing));
        let delivered = delivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        assert_eq!(delivered, [Delivery { speed: 130, pause_ms: 500 }, Delivery::default()]);
    }

    /// Issue #762: a stop ends the sound at once and Pocket stops computing; what it still sends of the
    /// stopped job is not played, and the next job reads.
    #[test]
    fn a_stop_ends_pocket_at_once() {
        let (sender, events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), POCKET_DE);
        let (mut helper, said, played) = helper(Some(folder.path()), true, Duration::from_millis(5));
        helper.handle(say(SpeechProvider::Pocket, &"long ".repeat(400)), &sender);
        pump(&mut helper, &events, &sender, |_| !played.borrow().samples.is_empty());
        assert_eq!(said.borrow().as_slice(), [Output::Playing]);
        helper.handle(Event::Input(Input::Stop), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Done));
        assert!(matches!(helper.job, Job::Idle));
        let heard = played.borrow().samples.len();

        helper.handle(say(SpeechProvider::Pocket, "Ja."), &sender);
        pump(&mut helper, &events, &sender, |helper| matches!(helper.job, Job::Local { computed: true, .. }));
        assert_eq!(played.borrow().samples.len(), heard + "Ja.".len(), "only the new job's sound");
        assert_eq!(played.borrow().opened, 2, "a sound output each");
        assert_eq!(said.borrow().as_slice(), [Output::Playing, Output::Done, Output::Playing]);
    }

    /// A Pocket job that fails, and a model that does not load, say so for people; the next job starts it
    /// again.
    #[test]
    fn a_failed_job_is_said_for_people() {
        let (sender, events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), POCKET_DE);
        let (mut helper, said, _) = helper(Some(folder.path()), true, Duration::ZERO);
        helper.handle(
            Event::Input(Input::Say(Say {
                provider: None,
                text: "Hi.".into(),
                voice: Some("broken".into()),
                delivery: Delivery::default(),
            })),
            &sender,
        );
        pump(&mut helper, &events, &sender, |helper| matches!(helper.job, Job::Idle));
        assert_eq!(codes(&said), ["error: true"]);

        helper.make_synth = Arc::new(|| Err(VoiceError::program("fake", "did not load")));
        helper.worker = None;
        helper.handle(say(SpeechProvider::Pocket, "Hi."), &sender);
        pump(&mut helper, &events, &sender, |helper| matches!(helper.job, Job::Idle));
        assert_eq!(codes(&said), ["error: true", "error: true"]);
        assert!(helper.worker.is_none());
    }

    /// The end of a thread replaced meanwhile fails no job of the next one.
    #[test]
    fn a_replaced_thread_is_not_heard() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said, _) = helper(None, true, Duration::ZERO);
        let (jobs, _inbox) = mpsc::channel();
        helper.worker = Some((2, SpeechProvider::Pocket, jobs));
        helper.job = Job::Local { id: 1, sound: None, computed: false };
        helper.handle(Event::WorkerGone(1), &sender);
        assert!(said.borrow().is_empty());
        assert!(matches!(helper.job, Job::Local { .. }) && helper.worker.is_some(), "the job still reads");
        helper.handle(Event::WorkerGone(2), &sender);
        assert_eq!(codes(&said), ["error: true"]);
        assert!(helper.worker.is_none());
    }

    /// The helper with a fake Qwen3-TTS too, which counts how often it is made; the voice it read with goes
    /// to `voices`.
    fn with_fake_qwen<W: FnMut(Output)>(
        helper: Helper<W>,
    ) -> (Helper<W>, Arc<AtomicU64>, Arc<std::sync::Mutex<Vec<String>>>) {
        struct FakeQwen(Arc<std::sync::Mutex<Vec<String>>>);
        impl Synth for FakeQwen {
            fn rate(&self) -> u32 {
                24_000
            }
            fn speak(&mut self, _text: &str, voice: &str, _: Delivery, mut chunk: Chunk) -> Result<(), VoiceError> {
                if let Ok(mut voices) = self.0.lock() {
                    voices.push(voice.to_string());
                }
                chunk(&[0.25]);
                Ok(())
            }
        }
        let made = Arc::new(AtomicU64::new(0));
        let voices = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (count, heard) = (Arc::clone(&made), Arc::clone(&voices));
        let make: Arc<MakeSynth> = Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeQwen(Arc::clone(&heard))) as Box<dyn Synth>)
        });
        (helper.with_qwen("calm_man", make), made, voices)
    }

    /// Issue #764: a build without Qwen3-TTS says so for people, and with it a model not downloaded asks for
    /// the download, as Pocket's does.
    #[test]
    fn qwen_needs_its_build_and_its_model() {
        let (sender, _events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        let (mut helper, said, _) = helper(Some(folder.path()), true, Duration::ZERO);
        helper.handle(say(SpeechProvider::Qwen, "Hallo."), &sender);
        assert_eq!(said.borrow().as_slice(), [Output::failed("unsupported", tr("This build has no Qwen3-TTS."))]);
        let (mut helper, made, _) = with_fake_qwen(helper);
        helper.handle(say(SpeechProvider::Qwen, "Hallo."), &sender);
        assert_eq!(said.borrow().last(), Some(&not_downloaded()), "the catalogue lists no model to find here");
        assert_eq!(made.load(Ordering::SeqCst), 0, "nothing is loaded");
        assert!(matches!(helper.job, Job::Idle));
    }

    /// Issue #764: Qwen3-TTS reads on a thread of its own in its voice of the settings; when Pocket reads
    /// next, Qwen's thread ends (its graphics memory is free), and the next Qwen job loads it again.
    #[cfg(feature = "qwen-cuda")]
    #[test]
    fn qwen_reads_on_its_own_thread_and_gives_way_to_pocket() {
        let (sender, events) = mpsc::channel();
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), POCKET_DE);
        download(folder.path(), QWEN);
        let (helper, said, played) = helper(Some(folder.path()), true, Duration::ZERO);
        let (mut helper, made, voices) = with_fake_qwen(helper);
        let computed = |helper: &Helper<_>| matches!(helper.job, Job::Local { computed: true, .. });

        helper.handle(say(SpeechProvider::Qwen, "Hallo."), &sender);
        pump(&mut helper, &events, &sender, computed);
        assert_eq!(said.borrow().as_slice(), [Output::Playing]);
        assert_eq!(played.borrow().samples, [(24_000, 0.25)]);
        assert!(matches!(helper.worker, Some((_, SpeechProvider::Qwen, _))));
        assert_eq!(voices.lock().map(|voices| voices.clone()).unwrap_or_default(), ["calm_man"]);

        helper.handle(say(SpeechProvider::Pocket, "Ja."), &sender);
        pump(&mut helper, &events, &sender, computed);
        assert!(matches!(helper.worker, Some((_, SpeechProvider::Pocket, _))), "Qwen's thread gave way");
        helper.handle(
            Event::Input(Input::Say(Say {
                provider: Some(SpeechProvider::Qwen),
                text: "Gut.".into(),
                voice: Some("warm_woman".into()),
            })),
            &sender,
        );
        pump(&mut helper, &events, &sender, computed);
        assert_eq!(made.load(Ordering::SeqCst), 2, "loaded again after Pocket read");
        assert_eq!(voices.lock().map(|voices| voices.clone()).unwrap_or_default(), ["calm_man", "warm_woman"]);
    }

    /// Issue #764: Qwen's status names the sound output first, then the graphics card, then its model, with
    /// "Warm woman" recommended.
    #[test]
    fn the_qwen_status_names_what_is_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let devices = FakeDevices { output: true, ..FakeDevices::default() };
        assert_eq!(
            qwen_option(Some(folder.path()), &FakeDevices::default(), true).missing,
            Some(SpeechMissing::Player)
        );
        assert_eq!(qwen_option(Some(folder.path()), &devices, false).missing, Some(SpeechMissing::Gpu));
        let option = qwen_option(Some(folder.path()), &devices, true);
        assert_eq!(option.missing, Some(SpeechMissing::Model));
        assert_eq!((option.provider, option.model.as_deref()), (SpeechProvider::Qwen, Some(QWEN)));
        let voices: Vec<_> = option.voices.iter().map(|voice| (voice.id.as_str(), voice.recommended)).collect();
        assert_eq!(voices, [("warm_woman", true), ("calm_man", false)]);
        if torro_core::settings::QWEN_BUILT {
            download(folder.path(), QWEN);
            assert_eq!(qwen_option(Some(folder.path()), &devices, true).missing, None);
        }
    }

    /// Issue #762: Pocket's status names what it misses, its voices with Alba recommended and its model.
    #[test]
    fn the_status_names_what_is_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let devices = FakeDevices { output: true, ..FakeDevices::default() };
        let status = status(&Voice::default(), Some(folder.path()), &devices);
        let pocket = status.option(SpeechProvider::Pocket).expect("pocket");
        assert_eq!(pocket.missing, Some(SpeechMissing::Model));
        assert_eq!(pocket.model.as_deref(), Some("pocket-de"));
        assert_eq!(pocket.setup, None);
        assert_eq!(pocket.voices.iter().map(|voice| voice.id.as_str()).collect::<Vec<_>>(), POCKET_VOICES);
        let recommended: Vec<_> =
            pocket.voices.iter().filter(|voice| voice.recommended).map(|voice| voice.id.as_str()).collect();
        assert_eq!(recommended, ["alba"]);
        assert!(status.fast_enough);
        assert_eq!((status.recommended, status.current), (SpeechProvider::Pocket, SpeechProvider::Pocket));
        assert_eq!(status.providers.len(), 1, "the core adds OpenAI");

        let silent = super::status(&Voice::default(), Some(folder.path()), &FakeDevices::default());
        assert_eq!(silent.providers[0].missing, Some(SpeechMissing::Player));
        download(folder.path(), POCKET_DE);
        let openai = Voice { speech_provider: Some(SpeechProvider::OpenAi), ..Voice::default() };
        let ready = super::status(&openai, Some(folder.path()), &devices);
        assert_eq!(ready.providers[0].missing, None);
        assert_eq!(ready.current, SpeechProvider::OpenAi);
    }
}
