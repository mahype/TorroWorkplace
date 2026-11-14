//! Reading aloud (issue #745, `docs/assistent.md` "Vorlesen"): `torroassistant --speak --voice
//! <settings>`, a helper the core keeps running while it reads (`assistant.speak`), so Qwen3-TTS loads
//! its model once and not for every answer. Ten minutes without a job it ends, and the graphics
//! card's memory is free again ([`IDLE`]).
//!
//! The core talks to it in JSON lines ([`Input`], [`Output`]):
//!
//! - `{"say": {"provider": "qwen", "text": …, "voice": "warm_woman", "model": …, "language":
//!   "German"}}` reads `text` (Markdown, at most [`MOST_READ`] characters) with Qwen3-TTS or Piper.
//!   With `"openai"` the helper only answers the pieces to fetch (`{"pieces": [...]}`, whole
//!   sentences of at most [`PIECE`] characters): the key stays in the core, which fetches the sound
//!   and sends it as `{"pcm": {"rate": 24000, "data": "<base64>"}}`, then `"end"`.
//! - `"stop"` ends what is read at once.
//! - The helper answers `"playing"` when the first sound plays, `"done"` when it was read to its
//!   end or stopped, `{"failed": {"code": …, "message": …}}` when it cannot read, with a message
//!   for people.
//!
//! Qwen3-TTS runs in Python ([`QWEN_SCRIPT`]): it clones the voice of a template that ships with the
//! assistant ([`TEMPLATES`], written to `<voice>/qwen/` when missing or changed), so every answer
//! sounds alike. Its sound comes in pieces while it computes and plays at once through a program
//! that plays raw sound from a pipe ([`RawPlayer`]); without one it is gathered and played as a
//! file. The text never touches the disk; the sound only as a private temporary file, and only
//! without a raw player.
//!
//! `torroassistant --speech-status --voice <settings>` says what is found here for reading aloud
//! ([`status`]); the core adds the graphics card and the OpenAI account (`settings.speech_status`).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{QWEN_RECOMMENDED, QWEN_VOICES, SpeechProvider, Voice};
use torro_ipc::{SpeechMissing, SpeechOption, SpeechStatus, SpeechVoice};

use super::audio::TempWav;
use super::listen::speakable_up_to;
use super::programs::{Finder, Found, Player, RawPlayer, qwen_folder, qwen_setup};

/// The most of a text that is read, in characters: the window shows all of it.
pub const MOST_READ: usize = 3000;

/// The longest piece fetched from OpenAI at once, in characters: the next one loads while the first
/// plays, and a short piece starts sooner.
pub const PIECE: usize = 500;

/// How long the helper waits for a job before it ends.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How often a job looks whether its sound has ended.
const POLL: Duration = Duration::from_millis(50);

/// The voice templates of Qwen3-TTS (step 4 of the plan of issue #745): made once with
/// `Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign` from a description, 24 kHz mono, about nine seconds, with
/// the words they say. Cloning needs both.
pub const TEMPLATES: [(&str, &[u8], &str); 2] = [
    (
        "warm_woman",
        include_bytes!("../../assets/voices/warm_woman.wav"),
        "Hallo, schön, dass du da bist. Ich lese dir deine Antworten vor, ruhig und deutlich, damit du ganz \
         entspannt zuhören kannst.",
    ),
    (
        "calm_man",
        include_bytes!("../../assets/voices/calm_man.wav"),
        "Guten Tag. Ich bin dein Assistent und lese dir die Antworten vor, in aller Ruhe, deutlich und gut \
         verständlich.",
    ),
];

/// The worker in Python: loads the model once, then reads one job per line from standard input
/// (`{"id", "pieces", "language", "ref_audio", "ref_text"}`) and writes its sound to standard
/// output: the rate as u32 little endian (0 when it failed before any sound; the reason on standard
/// error), then pieces of signed 16-bit mono, each after its length in bytes as u32, and 0 at the end.
/// `stop <id>` ends the job between two pieces. It ends when its input closes. The libraries print
/// to standard output, so the sound goes to a copy of it and everything else to standard error.
pub const QWEN_SCRIPT: &str = r#"
import os, sys, json, struct, threading, queue
# The sound goes to a copy of the output; whatever the libraries print goes to the error output.
out = os.fdopen(os.dup(1), "wb", buffering=0)
os.dup2(2, 1)
sys.stdout = sys.stderr
from faster_qwen3_tts import FasterQwen3TTS
import numpy as np
model = FasterQwen3TTS.from_pretrained(sys.argv[1])
try:
    model.warmup()
except Exception as err:
    print("warmup: %s" % err, file=sys.stderr, flush=True)
jobs = queue.Queue()
stopped = set()
def read():
    for line in sys.stdin:
        line = line.strip()
        if line.startswith("stop "):
            stopped.add(line[5:])
        elif line:
            jobs.put(json.loads(line))
    jobs.put(None)
threading.Thread(target=read, daemon=True).start()
def put(data):
    out.write(data)
    out.flush()
while True:
    job = jobs.get()
    if job is None:
        break
    ident = str(job["id"])
    started = False
    try:
        for piece in job["pieces"]:
            if ident in stopped:
                break
            for audio, rate, _ in model.generate_voice_clone_streaming(
                text=piece, language=job["language"], ref_audio=job["ref_audio"], ref_text=job["ref_text"],
                chunk_size=8):
                if ident in stopped:
                    break
                if not started:
                    put(struct.pack("<I", int(rate)))
                    started = True
                pcm = (np.clip(np.asarray(audio, dtype=np.float32), -1.0, 1.0) * 32767).astype("<i2").tobytes()
                put(struct.pack("<I", len(pcm)) + pcm)
        if not started:
            put(struct.pack("<I", 24000))
        put(struct.pack("<I", 0))
    except Exception as err:
        # Only the kind of error: its words may quote the text being read.
        print("generation failed: %s" % type(err).__name__, file=sys.stderr, flush=True)
        put(struct.pack("<I", 0))
    stopped.discard(ident)
"#;

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
    /// `None` for the core's choice: Qwen where it is set up and the card is fast enough (a model is
    /// named), else Piper (decision 1 of the plan of issue #745). The helper decides, since it knows
    /// what is installed without starting itself again.
    pub provider: Option<SpeechProvider>,
    pub text: String,
    /// The voice's id (Qwen); `None` for the recommended one.
    pub voice: Option<String>,
    /// Qwen's model, chosen by the core after the graphics card; `None` when it is too small.
    pub model: Option<String>,
    /// The language for Qwen: `German` or `English`.
    pub language: String,
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
            return Some(Input::Say(Say {
                provider,
                text: text(say, "text").unwrap_or_default(),
                voice: text(say, "voice"),
                model: text(say, "model"),
                language: text(say, "language").unwrap_or_else(|| "English".into()),
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
                "model": say.model,
                "language": say.language,
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

/// `text` in pieces of whole sentences, each at most `most` characters; a longer sentence is cut
/// between words.
pub fn pieces(text: &str, most: usize) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut sentence = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        sentence.push(c);
        if matches!(c, '.' | '!' | '?' | '…') && chars.peek().is_none_or(|next| next.is_whitespace()) {
            sentences.push(std::mem::take(&mut sentence));
        }
    }
    sentences.push(sentence);
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

/// What is found here for reading aloud ([`Output`] of `--speech-status`): Qwen3-TTS and Piper with
/// what they miss, their voices and Qwen's setup. The core adds the graphics card, the model and
/// OpenAI's entry, and decides which provider reads.
pub fn status(found: &Found, finder: &Finder) -> SpeechStatus {
    let player = |missing: Option<SpeechMissing>| missing.or((!found.plays()).then_some(SpeechMissing::Player));
    let qwen = SpeechOption {
        provider: SpeechProvider::Qwen,
        missing: player(found.qwen_python.is_none().then_some(SpeechMissing::Program)),
        voices: QWEN_VOICES
            .iter()
            .map(|id| SpeechVoice { id: (*id).to_string(), recommended: *id == QWEN_RECOMMENDED })
            .collect(),
        model: None,
        setup: finder.folder.as_deref().map(|folder| qwen_setup(&qwen_folder(folder))),
    };
    let piper_missing = match (&found.piper, &found.voice) {
        (None, _) => Some(SpeechMissing::Program),
        (Some(_), None) => Some(SpeechMissing::Voice),
        (Some(_), Some(_)) => None,
    };
    let piper_voice = found.voice.as_deref().and_then(Path::file_stem).map(|stem| stem.to_string_lossy().into_owned());
    let piper = SpeechOption {
        provider: SpeechProvider::Piper,
        missing: player(piper_missing),
        voices: piper_voice.into_iter().map(|id| SpeechVoice { id, recommended: true }).collect(),
        model: None,
        setup: None,
    };
    SpeechStatus {
        gpu: None,
        fast_enough: false,
        recommended: SpeechProvider::Piper,
        current: SpeechProvider::Piper,
        providers: vec![qwen, piper],
    }
}

/// One event of the helper's loop: a line from the core or what the Qwen worker wrote.
enum Event {
    Input(Input),
    /// The core closed the helper's input: it ended.
    Closed,
    /// Qwen's sound for the job starts at this rate; 0 when it failed before any sound.
    Rate(u32),
    /// A piece of Qwen's sound.
    Chunk(Vec<u8>),
    /// Qwen's sound for the job ended.
    End,
    /// The Qwen worker ended, with the last it said on its error output.
    Gone(String),
}

/// The sound of one job as it plays.
enum Playback {
    /// A raw player reads what comes on its input; dropping the feed ends its input.
    Raw { child: Child, feed: Option<Sender<Vec<u8>>> },
    /// No raw player: the sound is gathered and played as a file at the end.
    Gather { rate: u32, bytes: Vec<u8>, player: Player },
    /// A file plays.
    File { child: Child, _file: TempWav },
}

impl Playback {
    /// Sound at `rate` that comes in pieces.
    fn stream(found: &Found, rate: u32) -> Result<Playback, Output> {
        if let Some(raw) = &found.raw_player {
            return raw_playback(raw, rate);
        }
        match &found.player {
            Some(player) => Ok(Playback::Gather { rate, bytes: Vec::new(), player: player.clone() }),
            None => Err(no_player()),
        }
    }

    /// The WAV file `file`, at once.
    fn file(player: &Player, file: TempWav) -> Result<Playback, Output> {
        let command = player.command(file.path());
        let child = start(&command, Stdio::null())?;
        Ok(Playback::File { child, _file: file })
    }

    fn push(&mut self, bytes: Vec<u8>) {
        match self {
            Playback::Raw { feed: Some(feed), .. } => {
                let _ = feed.send(bytes);
            }
            Playback::Gather { bytes: gathered, .. } => gathered.extend_from_slice(&bytes),
            Playback::Raw { feed: None, .. } | Playback::File { .. } => {}
        }
    }

    /// No more sound comes: the raw player plays to the end of its input, gathered sound starts.
    fn finish(&mut self) -> Result<(), Output> {
        match self {
            Playback::Raw { feed, .. } => {
                feed.take();
                Ok(())
            }
            Playback::Gather { rate, bytes, player } => {
                let file = TempWav::with_bytes(&wav_at(*rate, bytes))
                    .map_err(|err| Output::failed("error", err.to_string()))?;
                *self = Playback::file(player, file)?;
                Ok(())
            }
            Playback::File { .. } => Ok(()),
        }
    }

    /// Whether the player ended; gathered sound has not started yet.
    fn ended(&mut self) -> bool {
        match self {
            Playback::Raw { child, .. } | Playback::File { child, .. } => !matches!(child.try_wait(), Ok(None)),
            Playback::Gather { .. } => false,
        }
    }

    /// Ends the sound now.
    fn kill(&mut self) {
        if let Playback::Raw { child, .. } | Playback::File { child, .. } = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.kill();
    }
}

/// `raw` playing what comes at `rate`. A thread writes to its input, so a slow player never holds
/// up the loop that would stop it.
fn raw_playback(raw: &RawPlayer, rate: u32) -> Result<Playback, Output> {
    let mut child = start(&raw.command(rate), Stdio::piped())?;
    let Some(mut input) = child.stdin.take() else {
        return Err(Output::failed("error", "the player has no input"));
    };
    let (feed, pieces) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for piece in pieces {
            if input.write_all(&piece).is_err() {
                return;
            }
        }
        // `input` drops here: the player plays what it has and ends.
    });
    Ok(Playback::Raw { child, feed: Some(feed) })
}

/// Starts `command` without a shell, its output dropped.
fn start(command: &[String], stdin: Stdio) -> Result<Child, Output> {
    let (program, args) = command.split_first().ok_or_else(no_player)?;
    Command::new(program)
        .args(args)
        .stdin(stdin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| Output::failed("error", format!("{program}: {err}")))
}

fn no_player() -> Output {
    Output::failed("unsupported", tr("No program to play sound was found. Install PulseAudio, PipeWire or SoX."))
}

/// A WAV file of signed 16-bit mono `pcm` at `rate`.
fn wav_at(rate: u32, pcm: &[u8]) -> Vec<u8> {
    let size = u32::try_from(pcm.len()).unwrap_or(u32::MAX - 36);
    let mut bytes = Vec::with_capacity(44 + pcm.len());
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(pcm);
    bytes
}

/// Writes the template `id` to `qwen` ([`qwen_folder`]) when it is missing or differs from the one
/// that ships, and returns its path with its words.
pub fn template(qwen: &Path, id: &str) -> Result<(PathBuf, &'static str), String> {
    let (id, bytes, words) = TEMPLATES
        .iter()
        .find(|(known, _, _)| *known == id)
        .or_else(|| TEMPLATES.first())
        .ok_or("no voice templates")?;
    let path = qwen.join(format!("{id}.wav"));
    if std::fs::read(&path).ok().as_deref() != Some(*bytes) {
        std::fs::create_dir_all(qwen).map_err(|err| format!("{}: {err}", qwen.display()))?;
        std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    Ok((path, words))
}

/// The Qwen worker as it runs, for one model.
struct Qwen {
    model: String,
    child: Child,
    input: ChildStdin,
}

impl Drop for Qwen {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the worker of `model` with `python`; its sound comes as events on `events`.
fn start_qwen(python: &Path, model: &str, events: &Sender<Event>) -> Result<Qwen, String> {
    let mut child = Command::new(python)
        .args(["-I", "-u", "-c", QWEN_SCRIPT, model])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("{}: {err}", python.display()))?;
    let (Some(input), Some(output), Some(errors)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.kill();
        return Err("the worker has no pipes".into());
    };
    let said = Arc::new(Mutex::new(String::new()));
    let last = Arc::clone(&said);
    std::thread::spawn(move || {
        for line in BufReader::new(errors).lines().map_while(Result::ok) {
            eprintln!("torroassistant: qwen: {line}");
            if !line.trim().is_empty() {
                *last.lock().unwrap_or_else(PoisonError::into_inner) = line;
            }
        }
    });
    let events = events.clone();
    std::thread::spawn(move || {
        let mut output = BufReader::new(output);
        let gone = |events: &Sender<Event>| {
            let said = said.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let _ = events.send(Event::Gone(said));
        };
        loop {
            let Some(rate) = read_u32(&mut output) else { return gone(&events) };
            if events.send(Event::Rate(rate)).is_err() {
                return;
            }
            if rate == 0 {
                continue;
            }
            loop {
                let Some(length) = read_u32(&mut output) else { return gone(&events) };
                if length == 0 {
                    let _ = events.send(Event::End);
                    break;
                }
                let mut chunk = vec![0; length as usize];
                if output.read_exact(&mut chunk).is_err() {
                    return gone(&events);
                }
                if events.send(Event::Chunk(chunk)).is_err() {
                    return;
                }
            }
        }
    });
    Ok(Qwen { model: model.to_string(), child, input })
}

fn read_u32(reader: &mut impl Read) -> Option<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// The job that reads now.
enum Job {
    Idle,
    /// Qwen computes and plays.
    Qwen {
        playback: Option<Playback>,
        computed: bool,
        playing: bool,
    },
    /// The core sends OpenAI's sound.
    Stream {
        playback: Option<Playback>,
        ended: bool,
    },
    /// Piper writes its file (`synth`), which then plays.
    Piper {
        synth: Option<(Child, TempWav)>,
        playback: Option<Playback>,
    },
}

/// The helper's state: what it found, the Qwen worker and the job.
pub struct Helper<W: FnMut(Output)> {
    found: Found,
    /// The settings to look for programs again with, when a job lacks one: installed meanwhile
    /// (Qwen set up after "Preview" said it was missing). `None` in tests, which bring what is found.
    voice: Option<Voice>,
    folder: Option<PathBuf>,
    qwen: Option<Qwen>,
    job: Job,
    /// The id of the last job sent to Qwen; its stopped ones still send sound until their end.
    next: u64,
    /// A stopped Qwen job still sends its sound; it is dropped until its end.
    draining: bool,
    /// A Qwen job that waits for the stopped one to end.
    pending: Option<Say>,
    write: W,
}

impl<W: FnMut(Output)> Helper<W> {
    pub fn new(found: Found, folder: Option<PathBuf>, voice: Option<Voice>, write: W) -> Helper<W> {
        Helper { found, folder, voice, qwen: None, job: Job::Idle, next: 0, draining: false, pending: None, write }
    }

    /// The provider that reads `say`: the one named, or the core's choice ([`Say::provider`]).
    fn provider_of(&self, say: &Say) -> SpeechProvider {
        say.provider.unwrap_or(match say.model.is_some() && self.found.qwen_python.is_some() {
            true => SpeechProvider::Qwen,
            false => SpeechProvider::Piper,
        })
    }

    /// Whether what `provider` needs here is missing as found.
    fn lacks(&self, provider: SpeechProvider) -> bool {
        let plays = self.found.plays();
        match provider {
            SpeechProvider::Qwen => self.found.qwen_python.is_none() || !plays,
            SpeechProvider::Piper => self.found.piper.is_none() || self.found.voice.is_none() || !plays,
            SpeechProvider::OpenAi => !plays,
        }
    }

    /// Runs until the core closes the input or [`IDLE`] passes without a job.
    fn run(&mut self, events: &Receiver<Event>, sender: &Sender<Event>) {
        loop {
            let idle = matches!(self.job, Job::Idle) && self.pending.is_none();
            match events.recv_timeout(if idle { IDLE } else { POLL }) {
                Ok(Event::Closed) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) if idle && !self.draining => return,
                Err(RecvTimeoutError::Timeout) => {}
                Ok(event) => self.handle(event, sender),
            }
            self.look_at_the_end();
        }
    }

    fn handle(&mut self, event: Event, sender: &Sender<Event>) {
        match event {
            Event::Input(Input::Stop) => {
                if self.pending.take().is_some() || !matches!(self.job, Job::Idle) {
                    self.stop();
                    (self.write)(Output::Done);
                }
            }
            Event::Input(Input::Say(say)) => {
                if self.pending.take().is_some() || !matches!(self.job, Job::Idle) {
                    self.stop();
                    (self.write)(Output::Done);
                }
                self.say(say, sender);
            }
            Event::Input(Input::Pcm { rate, data }) => {
                if let Job::Stream { playback, ended: false } = &mut self.job {
                    if playback.is_none() {
                        match Playback::stream(&self.found, rate) {
                            Ok(started) => *playback = Some(started),
                            Err(failed) => return self.fail(failed),
                        }
                        (self.write)(Output::Playing);
                    }
                    if let Some(playback) = playback {
                        playback.push(data);
                    }
                }
            }
            Event::Input(Input::End) => {
                if let Job::Stream { playback, ended } = &mut self.job {
                    *ended = true;
                    match playback {
                        Some(playback) => {
                            if let Err(failed) = playback.finish() {
                                self.fail(failed);
                            }
                        }
                        None => self.finished(),
                    }
                }
            }
            Event::Closed => {}
            Event::Rate(rate) if self.draining => {
                // A stopped job that failed sends no end.
                if rate == 0 {
                    self.drained(sender);
                }
            }
            Event::Chunk(_) if self.draining => {}
            Event::End if self.draining => self.drained(sender),
            Event::Rate(0) => {
                // The worker is done with the job: nothing to stop or drain.
                if matches!(self.job, Job::Qwen { .. }) {
                    self.job = Job::Idle;
                    (self.write)(Output::failed("error", tr("Qwen3-TTS could not read the text.")));
                }
            }
            Event::Rate(rate) => {
                if let Job::Qwen { playback, .. } = &mut self.job {
                    match Playback::stream(&self.found, rate) {
                        Ok(started) => *playback = Some(started),
                        Err(failed) => self.fail(failed),
                    }
                }
            }
            Event::Chunk(chunk) => {
                if let Job::Qwen { playback: Some(playback), playing, .. } = &mut self.job {
                    playback.push(chunk);
                    if !*playing {
                        *playing = true;
                        (self.write)(Output::Playing);
                    }
                }
            }
            Event::End => {
                if let Job::Qwen { playback, computed, .. } = &mut self.job {
                    *computed = true;
                    match playback {
                        Some(playback) => {
                            if let Err(failed) = playback.finish() {
                                self.fail(failed);
                            }
                        }
                        None => self.finished(),
                    }
                }
            }
            Event::Gone(said) => {
                self.qwen = None;
                self.draining = false;
                // What it last said stays in the log: a library's words may quote the text read.
                eprintln!("torroassistant: qwen ended: {said}");
                if matches!(self.job, Job::Qwen { .. }) {
                    self.fail(Output::failed("error", tr("Qwen3-TTS stopped. Try again.")));
                }
                if let Some(say) = self.pending.take() {
                    self.say(say, sender);
                }
            }
        }
    }

    /// A stopped Qwen job sent its end: the one that waited may start.
    fn drained(&mut self, sender: &Sender<Event>) {
        self.draining = false;
        if let Some(say) = self.pending.take() {
            self.say(say, sender);
        }
    }

    /// Starts reading `say`.
    fn say(&mut self, say: Say, sender: &Sender<Event>) {
        let text = speakable_up_to(&say.text, MOST_READ);
        // Something missing may have been installed since the helper started: look again, once.
        if self.lacks(self.provider_of(&say))
            && let Some(voice) = &self.voice
        {
            self.found = Found::detect(&Finder::system(), voice);
        }
        match self.provider_of(&say) {
            SpeechProvider::OpenAi => {
                (self.write)(Output::Pieces(pieces(&text, PIECE)));
                self.job = Job::Stream { playback: None, ended: false };
            }
            _ if text.is_empty() => (self.write)(Output::Done),
            SpeechProvider::Qwen if self.draining => self.pending = Some(say),
            SpeechProvider::Qwen => match self.ask_qwen(&say, &text, sender) {
                Ok(()) => self.job = Job::Qwen { playback: None, computed: false, playing: false },
                Err(failed) => (self.write)(failed),
            },
            SpeechProvider::Piper => match self.piper(&text) {
                Ok(synth) => self.job = Job::Piper { synth: Some(synth), playback: None },
                Err(failed) => (self.write)(failed),
            },
        }
    }

    /// Sends `text` to the Qwen worker, started for the model first.
    fn ask_qwen(&mut self, say: &Say, text: &str, sender: &Sender<Event>) -> Result<(), Output> {
        let Some(model) = say.model.as_deref() else {
            return Err(Output::failed(
                "unsupported",
                tr("Local voices need an NVIDIA graphics card with at least 4 GB."),
            ));
        };
        let (Some(python), Some(folder)) = (self.found.qwen_python.clone(), self.folder.clone()) else {
            return Err(not_set_up());
        };
        if !self.found.plays() {
            return Err(no_player());
        }
        let voice = say.voice.as_deref().unwrap_or(QWEN_RECOMMENDED);
        let (reference, words) = template(&qwen_folder(&folder), voice).map_err(|err| Output::failed("error", err))?;
        if self.qwen.as_ref().is_none_or(|qwen| qwen.model != model) {
            self.qwen = None;
            self.qwen = Some(start_qwen(&python, model, sender).map_err(|err| {
                Output::failed("error", trf("Qwen3-TTS could not start: {reason}", &[("reason", &err)]))
            })?);
        }
        self.next += 1;
        let job = json!({
            "id": self.next,
            "pieces": pieces(text, PIECE),
            "language": say.language,
            "ref_audio": reference.to_string_lossy(),
            "ref_text": words,
        });
        let sent = self.qwen.as_mut().map(|qwen| writeln!(qwen.input, "{job}").and_then(|()| qwen.input.flush()));
        if !matches!(sent, Some(Ok(()))) {
            self.qwen = None;
            return Err(Output::failed("error", tr("Qwen3-TTS stopped. Try again.")));
        }
        Ok(())
    }

    /// Starts Piper reading `text` into a file; it plays once Piper is done ([`Helper::look_at_the_end`]),
    /// and a stop meanwhile ends Piper.
    fn piper(&self, text: &str) -> Result<(Child, TempWav), Output> {
        let (Some(piper), Some(voice)) = (&self.found.piper, &self.found.voice) else {
            return Err(Output::failed("unsupported", tr("Piper is not installed or has no voice.")));
        };
        if self.found.player.is_none() {
            return Err(no_player());
        }
        let file = TempWav::empty();
        let mut child = Command::new(piper)
            .arg("--model")
            .arg(voice)
            .arg("--output_file")
            .arg(file.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| Output::failed("error", format!("piper: {err}")))?;
        if let Some(mut input) = child.stdin.take() {
            // Piper reads a line at a time; its input closes here, so it ends after the line.
            let _ = writeln!(input, "{}", text.replace(['\n', '\r'], " "));
        }
        Ok((child, file))
    }

    /// Ends what is read now; a Qwen job still computing is told to stop.
    fn stop(&mut self) {
        if let Job::Qwen { computed: false, .. } = self.job
            && let Some(qwen) = self.qwen.as_mut()
        {
            let told = writeln!(qwen.input, "stop {}", self.next).and_then(|()| qwen.input.flush());
            self.draining = told.is_ok();
        }
        if let Job::Piper { synth: Some((child, _)), .. } = &mut self.job {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Dropping the playback kills the player.
        self.job = Job::Idle;
    }

    fn fail(&mut self, failed: Output) {
        self.stop();
        (self.write)(failed);
    }

    fn finished(&mut self) {
        self.job = Job::Idle;
        (self.write)(Output::Done);
    }

    /// A job whose sound is all there and has played is done.
    fn look_at_the_end(&mut self) {
        // Piper wrote its file: it plays now.
        if let Job::Piper { synth, playback } = &mut self.job
            && let Some((child, _)) = synth
            && !matches!(child.try_wait(), Ok(None))
        {
            let written = child.try_wait().ok().flatten().is_some_and(|status| status.success());
            let started = match (synth.take(), &self.found.player) {
                (Some((_, file)), Some(player)) if written => Playback::file(player, file),
                _ => Err(Output::failed("error", tr("Piper could not read the text."))),
            };
            match started {
                Ok(started) => {
                    *playback = Some(started);
                    (self.write)(Output::Playing);
                }
                Err(failed) => return self.fail(failed),
            }
        }
        let done = match &mut self.job {
            Job::Idle => false,
            Job::Qwen { playback: Some(playback), computed: true, .. }
            | Job::Stream { playback: Some(playback), ended: true } => playback.ended(),
            Job::Piper { playback: Some(playback), .. } => playback.ended(),
            Job::Piper { .. } => false,
            Job::Qwen { .. } | Job::Stream { .. } => false,
        };
        if done {
            self.finished();
        }
    }
}

fn not_set_up() -> Output {
    Output::failed("unsupported", tr("Qwen3-TTS is not set up. Set it up in Settings › Assistant › Voice."))
}

/// The helper (see the module) for `voice`, on standard input and output.
pub fn serve(voice: Voice) {
    let finder = Finder::system();
    let found = Found::detect(&finder, &voice);
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
    let mut helper = Helper::new(found, finder.folder, Some(voice), |output: Output| {
        let mut out = std::io::stdout();
        // When the core no longer reads, it ended; so does the helper.
        if writeln!(out, "{}", output.line()).and_then(|()| out.flush()).is_err() {
            std::process::exit(0);
        }
    });
    helper.run(&events, &sender);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn lines_go_both_ways() {
        let say = Input::Say(Say {
            provider: Some(SpeechProvider::Qwen),
            text: "Hello.".into(),
            voice: Some("calm_man".into()),
            model: Some("Qwen/Qwen3-TTS-12Hz-0.6B-Base".into()),
            language: "German".into(),
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

    #[test]
    fn wav_files_carry_their_rate() {
        let wav = wav_at(24_000, &[0, 1, 2, 3]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 24_000);
        assert_eq!(wav.len(), 48);
        // The templates are real WAV files at 24 kHz.
        for (id, bytes, words) in TEMPLATES {
            assert_eq!(&bytes[..4], b"RIFF", "{id}");
            assert_eq!(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]), 24_000, "{id}");
            assert!(!words.is_empty());
        }
        assert_eq!(TEMPLATES.map(|(id, _, _)| id), QWEN_VOICES);
    }

    #[test]
    fn templates_are_written_when_missing_or_changed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let qwen = dir.path().join("qwen");
        let (path, words) = template(&qwen, "calm_man").expect("written");
        assert_eq!(path, qwen.join("calm_man.wav"));
        assert!(words.starts_with("Guten Tag"));
        std::fs::write(&path, b"changed").expect("write");
        template(&qwen, "calm_man").expect("again");
        assert_eq!(std::fs::read(&path).expect("read"), TEMPLATES[1].1);
        let (fallback, _) = template(&qwen, "gone").expect("the first");
        assert_eq!(fallback, qwen.join("warm_woman.wav"));
    }

    /// What the helper answered.
    type Said = Rc<RefCell<Vec<Output>>>;

    /// A helper on `found`, with what it answers.
    fn helper(found: Found) -> (Helper<impl FnMut(Output)>, Said) {
        let said = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&said);
        let helper = Helper::new(found, None, None, move |output| log.borrow_mut().push(output));
        (helper, said)
    }

    fn say(provider: SpeechProvider, text: &str) -> Event {
        Event::Input(Input::Say(Say {
            provider: Some(provider),
            text: text.into(),
            voice: None,
            model: None,
            language: "English".into(),
        }))
    }

    #[test]
    fn what_is_missing_is_said_for_people() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said) = helper(Found::default());
        helper.handle(say(SpeechProvider::Piper, "Hello."), &sender);
        helper.handle(say(SpeechProvider::Qwen, "Hello."), &sender);
        helper.handle(say(SpeechProvider::Qwen, "  "), &sender);
        let codes: Vec<_> = said
            .borrow()
            .iter()
            .map(|output| match output {
                Output::Failed { code, message } => format!("{code}: {}", !message.is_empty()),
                other => other.line(),
            })
            .collect();
        assert_eq!(codes, ["unsupported: true", "unsupported: true", "\"done\""], "nothing to read is done");
    }

    #[test]
    fn openai_gets_its_pieces_and_ends_with_its_sound() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said) = helper(Found::default());
        helper.handle(say(SpeechProvider::OpenAi, "**One.** Two."), &sender);
        assert_eq!(said.borrow().as_slice(), [Output::Pieces(vec!["One. Two.".into()])]);
        // No sound came: the end is done at once.
        helper.handle(Event::Input(Input::End), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Done));
        // Sound without a player fails with a message, and stopping afterwards says nothing more.
        helper.handle(say(SpeechProvider::OpenAi, "Three."), &sender);
        helper.handle(Event::Input(Input::Pcm { rate: 24_000, data: vec![0; 8] }), &sender);
        assert!(matches!(said.borrow().last(), Some(Output::Failed { code, .. }) if code == "unsupported"));
        let count = said.borrow().len();
        helper.handle(Event::Input(Input::Stop), &sender);
        assert_eq!(said.borrow().len(), count, "nothing reads");
    }

    /// The fake raw player `play` keeps its input open as long as it plays: stopping ends it at once,
    /// the end of the sound lets it finish.
    #[cfg(unix)]
    #[test]
    fn a_stop_ends_the_sound_at_once() {
        let path = tempfile::tempdir().expect("temp dir");
        let program = super::super::programs::tests::fake_program(path.path(), "play", "cat > /dev/null");
        let found = Found {
            raw_player: Some(RawPlayer { kind: super::super::programs::RawKind::Play, program }),
            ..Found::default()
        };
        let (sender, _events) = mpsc::channel();
        let (mut helper, said) = helper(found);
        helper.handle(say(SpeechProvider::OpenAi, "One."), &sender);
        helper.handle(Event::Input(Input::Pcm { rate: 24_000, data: vec![0; 64] }), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Playing));
        helper.handle(Event::Input(Input::Stop), &sender);
        assert_eq!(said.borrow().last(), Some(&Output::Done));
        assert!(matches!(helper.job, Job::Idle));

        helper.handle(say(SpeechProvider::OpenAi, "Two."), &sender);
        helper.handle(Event::Input(Input::Pcm { rate: 24_000, data: vec![0; 64] }), &sender);
        helper.handle(Event::Input(Input::End), &sender);
        let started = std::time::Instant::now();
        while !matches!(helper.job, Job::Idle) && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(POLL);
            helper.look_at_the_end();
        }
        assert_eq!(said.borrow().last(), Some(&Output::Done), "played to its end");
    }

    /// Without a provider named the helper chooses: Qwen where it is found and a model is named,
    /// else Piper.
    #[test]
    fn the_core_s_choice_is_qwen_where_it_is_set_up_else_piper() {
        let auto = |model: Option<&str>| Say {
            provider: None,
            text: "Hello.".into(),
            voice: None,
            model: model.map(str::to_string),
            language: "English".into(),
        };
        let (bare, _) = helper(Found::default());
        assert_eq!(bare.provider_of(&auto(Some("Qwen/Qwen3-TTS-12Hz-1.7B-Base"))), SpeechProvider::Piper);
        let (set_up, _) = helper(Found { qwen_python: Some(PathBuf::from("python3")), ..Found::default() });
        assert_eq!(set_up.provider_of(&auto(Some("Qwen/Qwen3-TTS-12Hz-1.7B-Base"))), SpeechProvider::Qwen);
        assert_eq!(set_up.provider_of(&auto(None)), SpeechProvider::Piper, "no card fast enough");
        assert_eq!(
            Input::parse(r#"{"say": {"provider": null, "text": "Hi"}}"#)
                .map(|input| matches!(input, Input::Say(Say { provider: None, .. }))),
            Some(true)
        );
    }

    /// Piper writes its file outside the loop: a stop meanwhile ends it at once and nothing plays.
    #[cfg(unix)]
    #[test]
    fn a_stop_ends_piper_while_it_writes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let piper = super::super::programs::tests::fake_program(dir.path(), "piper", "sleep 20");
        let player = super::super::programs::tests::fake_program(dir.path(), "aplay", "exit 0");
        let found = Found {
            piper: Some(piper),
            voice: Some(dir.path().join("de_DE-thorsten-medium.onnx")),
            player: Some(Player { program: player, powershell: false }),
            ..Found::default()
        };
        let (sender, _events) = mpsc::channel();
        let (mut helper, said) = helper(found);
        helper.handle(say(SpeechProvider::Piper, "Hello."), &sender);
        assert!(said.borrow().is_empty(), "nothing plays while Piper writes");
        let started = std::time::Instant::now();
        helper.handle(Event::Input(Input::Stop), &sender);
        assert!(started.elapsed() < Duration::from_secs(5), "the stop does not wait for Piper");
        assert_eq!(said.borrow().as_slice(), [Output::Done]);
        assert!(matches!(helper.job, Job::Idle));
    }

    /// A Qwen job stopped while it computes drops its remaining sound; the next one waits for its end.
    #[test]
    fn a_stopped_qwen_job_is_drained_before_the_next() {
        let (sender, _events) = mpsc::channel();
        let (mut helper, said) = helper(Found::default());
        helper.draining = true;
        helper.handle(say(SpeechProvider::Qwen, "Next."), &sender);
        assert!(helper.pending.is_some(), "waits");
        helper.handle(Event::Chunk(vec![0; 4]), &sender);
        assert!(said.borrow().is_empty(), "the stopped job's sound is dropped");
        helper.handle(Event::Input(Input::Stop), &sender);
        assert_eq!(said.borrow().as_slice(), [Output::Done], "a waiting job can be stopped too");
        assert!(helper.pending.is_none());
        helper.handle(Event::End, &sender);
        assert!(!helper.draining);
    }

    #[test]
    fn the_status_names_what_is_missing() {
        let finder = Finder { dirs: Vec::new(), folder: Some(PathBuf::from("/voice")) };
        let status = status(&Found::default(), &finder);
        let qwen = status.option(SpeechProvider::Qwen).expect("qwen");
        assert_eq!(qwen.missing, Some(SpeechMissing::Program));
        assert_eq!(qwen.voices.iter().filter(|voice| voice.recommended).count(), 1);
        assert!(qwen.setup.as_deref().is_some_and(|setup| setup.contains("faster-qwen3-tts")));
        let found = Found { qwen_python: Some(PathBuf::from("python3")), ..Found::default() };
        assert_eq!(super::status(&found, &finder).providers[0].missing, Some(SpeechMissing::Player));
        let piper = status.option(SpeechProvider::Piper).expect("piper");
        assert_eq!(piper.missing, Some(SpeechMissing::Program));
    }
}
