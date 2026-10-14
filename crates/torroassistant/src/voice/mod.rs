//! Talking to the assistant by voice (task C.9, `docs/assistent.md`, "Sprache").
//!
//! `torroassistant --listen` is a process of its own, apart from the core: it holds the
//! microphone and the models, the core only starts and stops it ([`cli`]). While the setting
//! `voice.enabled` is on it waits for the wake word "Hey Torro", plays a short tone, records until
//! a pause, turns the sentence into text and sends it to the assistant as `assistant.send_message`
//! in the conversation "Voice", with the engine used last. The answer shows in a chat window of
//! this app (`--chat <id>`, where an approval is a card a person clicks) and, if wanted, is spoken.
//! No word spoken ever decides an approval: the listener only sends messages and reads answers
//! ([`listen::CoreAssistant`]), a video in the background must not be able to say "Allow".
//!
//! Every part runs on this computer as a program the person installed, found at runtime
//! ([`programs`]), so the suite builds on Linux, macOS and Windows without audio libraries:
//!
//! - wake word: openWakeWord (Python) with the model `hey_torro.onnx`; without it the wake word is
//!   found with speech to text on short windows ([`wake::TranscriptWake`]),
//! - recording: `parec`, `arecord`, SoX or `ffmpeg`, as raw 16 kHz mono sound on their output,
//! - speech to text: whisper.cpp (`whisper-cli`) with a `ggml-*.bin` model,
//! - speech: Piper with a voice, played with `paplay`, `afplay` or the like.
//!
//! The parts are small traits ([`Recorder`], [`WakeWord`], [`Transcriber`], [`Speaker`]), so the
//! tests run the listener on sound files and fakes without a microphone or a model
//! ([`listen`]).
//!
//! While it waits for the wake word, the listener keeps a few seconds in memory and nothing on
//! disk. Only for whisper.cpp, which reads files, a sentence (or, without openWakeWord, a window
//! of two seconds) goes to a private temporary file that is removed at once ([`audio::TempWav`]).

pub mod audio;
pub mod cli;
pub mod external;
pub mod listen;
pub mod programs;
pub mod wake;

use std::fmt;

use torro_ipc::{VoiceMissing, VoicePart};

use crate::backend::Refusal;

/// Why a part of the listener failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    /// A part cannot work: its program, model or player is missing ([`programs::Found`]).
    Missing(VoicePart, VoiceMissing),
    /// A program did not start, failed or ended.
    Program { program: String, message: String },
    /// Sound could not be read or written.
    Sound(String),
    /// The core refused a request or could not be reached.
    Core(Refusal),
}

impl VoiceError {
    /// A failed program `program`, with what went wrong.
    pub fn program(program: impl Into<String>, message: impl fmt::Display) -> VoiceError {
        VoiceError::Program { program: program.into(), message: message.to_string() }
    }
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceError::Missing(part, missing) => write!(f, "{part:?} cannot work: its {missing:?} is missing"),
            VoiceError::Program { program, message } => write!(f, "{program}: {message}"),
            VoiceError::Sound(message) => write!(f, "sound: {message}"),
            VoiceError::Core(refusal) => write!(f, "the core answered {}: {}", refusal.code, refusal.message),
        }
    }
}

impl std::error::Error for VoiceError {}

/// The microphone: sound at [`audio::RATE`], mono, 16 bit.
pub trait Recorder {
    /// Opens the microphone. It records until the stream is dropped; nothing is recorded before
    /// this is called or after the stream is gone.
    fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError>;
}

/// An open microphone ([`Recorder::open`]).
pub trait Stream {
    /// The next [`audio::FRAME`] samples, waiting for them; `None` when the recording ended (the
    /// program stopped, the file is read to its end).
    fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError>;
}

/// Listens for the wake word, frame by frame.
pub trait WakeWord {
    /// Takes the next frame; `true` once the wake word was heard in what came so far.
    fn hears(&mut self, frame: &[i16]) -> Result<bool, VoiceError>;

    /// Forgets what it heard, after an answer: the next wake word starts afresh.
    fn reset(&mut self) {}
}

/// Turns speech into text.
pub trait Transcriber {
    /// What is said in `samples` ([`audio::RATE`], mono), trimmed; empty when nothing is.
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError>;
}

/// Speaks and signals.
pub trait Speaker {
    /// Speaks `text` and returns when it is done.
    fn say(&mut self, text: &str) -> Result<(), VoiceError>;

    /// The short tone when the wake word was heard, so the person knows recording starts. Returns
    /// once it was played; nothing when there is no way to play sound.
    fn chime(&mut self) {}
}
