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
//! Every part is built into the app (issue #762), so nothing has to be installed beside it:
//!
//! - speech to text with sherpa-onnx ([`recognize`]), as the setting `voice.recognizer` chooses:
//!   Whisper turbo with the language of the setting `voice.language`, or Parakeet,
//! - the wake word "Hey Torro" with a small model of its own, built in ([`wake::ModelWake`], issue #765);
//!   only when it does not load, with the recogniser on short windows ([`wake::TranscriptWake`]),
//! - the microphone and the sound output with cpal ([`sound`]),
//! - reading aloud with Pocket TTS ([`pocket`], in the helper [`speech`]).
//!
//! The models are downloaded in the settings (`settings.download_voice_model`) into the voice folder
//! (`torro_core::voice_models`); what is missing is a model or a sound device ([`parts`]).
//!
//! The parts are small traits ([`Recorder`], [`WakeWord`], [`Transcriber`], [`Speaker`]), so the
//! tests run the listener on sound files and fakes without a microphone or a model
//! ([`listen`]).
//!
//! While it waits for the wake word, the listener keeps a few seconds in memory and nothing on
//! disk; no sentence and no answer is ever written to a file.

pub mod audio;
pub mod cli;
pub mod listen;
pub mod parts;
pub mod pocket;
pub mod recognize;
pub mod sound;
pub mod speech;
pub mod wake;

use std::fmt;

use torro_ipc::{VoiceMissing, VoicePart};

use crate::backend::Refusal;

/// Why a part of the listener failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    /// A part cannot work: its model or its sound device is missing ([`parts::programs`]).
    Missing(VoicePart, VoiceMissing),
    /// A part failed: a model did not load or made nothing (`program` names the part).
    Program { program: String, message: String },
    /// Sound could not be read or written.
    Sound(String),
    /// The core refused a request or could not be reached.
    Core(Refusal),
}

impl VoiceError {
    /// The part `program` failed, with what went wrong.
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
    /// device went away, the file is read to its end).
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

/// Signals: since issue #745 the core reads the answers aloud (`assistant.speak_answer`), the
/// listener only plays its tone.
pub trait Speaker {
    /// The short tone when the wake word was heard, so the person knows recording starts. Returns
    /// once it was played; nothing when there is no way to play sound.
    fn chime(&mut self) {}
}
