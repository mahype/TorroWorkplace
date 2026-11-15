//! Speech to text built in (issue #762): sherpa-onnx's offline recogniser with the model the setting
//! `speech_recognizer` chooses, from its folder in the voice folder (`torro_core::voice_models`):
//! Whisper turbo with the language of the setting `language` ([`Voice::language_or_default`],
//! `auto` lets Whisper find it), or Parakeet, which finds the language itself. [`recognizer_config`]
//! builds the configuration, [`SherpaTranscriber`] keeps the model loaded.
//!
//! The wake word has a model of its own (issue #765, [`super::wake::ModelWake`]). Only when that does not
//! load is it heard with the same recogniser on short windows ([`super::wake::TranscriptWake`]), with one
//! model loaded once ([`SharedTranscriber`]). The sound never touches the disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    OfflineWhisperModelConfig,
};
use torro_core::settings::{Recognizer, Voice};
use torro_ipc::{VoiceMissing, VoicePart};

use super::audio::{RATE, to_float};
use super::parts::model_ready;
use super::{Transcriber, VoiceError};

/// The most Whisper hears at once: its window is 30 seconds, a longer dictation goes in parts.
const WHISPER_MOST: usize = 30 * RATE as usize;

/// The name speech to text goes by in messages.
const NAME: &str = "speech to text";

/// The threads the models compute on: up to four, leaving the rest of the processor to the desktop.
pub fn threads() -> i32 {
    std::thread::available_parallelism().map(|count| count.get().min(4)).unwrap_or(2) as i32
}

/// The model `recognizer` takes, when it is downloaded into `folder`; else
/// [`VoiceError::Missing`] of [`VoicePart::SpeechToText`] with [`VoiceMissing::Model`].
pub fn require(recognizer: Recognizer, folder: Option<&Path>) -> Result<PathBuf, VoiceError> {
    match folder {
        Some(folder) if model_ready(recognizer.model(), Some(folder)) => Ok(folder.to_path_buf()),
        _ => Err(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model)),
    }
}

/// The recogniser's configuration for `recognizer` with its files in the voice folder `folder`
/// (`<folder>/models/<id>/…`). Whisper listens for `language` (a Whisper code; `auto` or an empty one
/// lets it find the language) and transcribes, never translates; Parakeet is a NeMo transducer and
/// takes no language. Builds nothing and reads no file, so it is tested without models.
pub fn recognizer_config(recognizer: Recognizer, folder: &Path, language: &str) -> OfflineRecognizerConfig {
    let id = recognizer.model();
    let file = |name: &str| Some(folder.join("models").join(id).join(name).to_string_lossy().into_owned());
    let mut model = OfflineModelConfig { num_threads: threads(), ..OfflineModelConfig::default() };
    match recognizer {
        Recognizer::Whisper => {
            model.whisper = OfflineWhisperModelConfig {
                encoder: file("turbo-encoder.int8.onnx"),
                decoder: file("turbo-decoder.int8.onnx"),
                language: Some(language).filter(|code| !code.is_empty() && *code != "auto").map(str::to_string),
                task: Some("transcribe".into()),
                ..OfflineWhisperModelConfig::default()
            };
            model.tokens = file("turbo-tokens.txt");
        }
        Recognizer::Parakeet => {
            model.transducer = OfflineTransducerModelConfig {
                encoder: file("encoder.int8.onnx"),
                decoder: file("decoder.int8.onnx"),
                joiner: file("joiner.int8.onnx"),
            };
            model.tokens = file("tokens.txt");
            model.model_type = Some("nemo_transducer".into());
        }
    }
    OfflineRecognizerConfig { model_config: model, ..OfflineRecognizerConfig::default() }
}

/// The model the settings choose, loaded once: loading takes seconds, a sentence about one.
pub struct SherpaTranscriber {
    recognizer: OfflineRecognizer,
    kind: Recognizer,
}

impl SherpaTranscriber {
    /// Loads the model `voice` chooses from the voice folder `folder`. A model that is not downloaded
    /// is [`VoiceError::Missing`] with [`VoiceMissing::Model`]; one sherpa-onnx cannot load (a file
    /// damaged on disk) a [`VoiceError::Program`].
    pub fn load(voice: &Voice, folder: Option<&Path>) -> Result<SherpaTranscriber, VoiceError> {
        let folder = require(voice.recognizer, folder)?;
        let config = recognizer_config(voice.recognizer, &folder, voice.language_or_default());
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| VoiceError::program(NAME, format!("the model {} did not load", voice.recognizer.model())))?;
        Ok(SherpaTranscriber { recognizer, kind: voice.recognizer })
    }
}

impl Transcriber for SherpaTranscriber {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        let most = match self.kind {
            Recognizer::Whisper => WHISPER_MOST,
            Recognizer::Parakeet => samples.len().max(1),
        };
        let mut words = Vec::new();
        for part in samples.chunks(most) {
            let stream = self.recognizer.create_stream();
            let floats: Vec<f32> = part.iter().copied().map(to_float).collect();
            stream.accept_waveform(RATE as i32, &floats);
            self.recognizer.decode(&stream);
            let result = stream.get_result().ok_or_else(|| VoiceError::program(NAME, "no result"))?;
            words.push(result.text);
        }
        Ok(transcript_of(&words.join(" ")))
    }
}

/// The text a recogniser wrote, without the marks for what is no speech (`[BLANK_AUDIO]`, `(music)`)
/// and with single spaces.
pub fn transcript_of(output: &str) -> String {
    let mut text = String::new();
    let mut depth = 0usize;
    for c in output.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(if c.is_control() { ' ' } else { c }),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One transcriber shared by the wake word heard with speech recognition ([`super::wake::TranscriptWake`],
/// on a thread of its own, when the built-in model does not load) and the sentences, so one model is loaded. A sentence waits while a check of the wake word still
/// runs: at most one check, about a second with Whisper turbo.
#[derive(Clone)]
pub struct SharedTranscriber(Arc<Mutex<Box<dyn Transcriber + Send>>>);

impl SharedTranscriber {
    pub fn new(transcriber: Box<dyn Transcriber + Send>) -> SharedTranscriber {
        SharedTranscriber(Arc::new(Mutex::new(transcriber)))
    }
}

impl Transcriber for SharedTranscriber {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).transcribe(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(config: &Option<String>) -> &str {
        config.as_deref().unwrap_or_default()
    }

    /// Issue #762: Whisper gets the language of the settings and its files from its folder.
    #[test]
    fn whisper_listens_for_the_language_of_the_settings() {
        let folder = Path::new("/voice");
        let config = recognizer_config(Recognizer::Whisper, folder, "de");
        let whisper = &config.model_config.whisper;
        let model = folder.join("models").join("whisper-turbo");
        assert_eq!(path(&whisper.encoder), model.join("turbo-encoder.int8.onnx").to_string_lossy());
        assert_eq!(path(&whisper.decoder), model.join("turbo-decoder.int8.onnx").to_string_lossy());
        assert_eq!(path(&config.model_config.tokens), model.join("turbo-tokens.txt").to_string_lossy());
        assert_eq!(whisper.language.as_deref(), Some("de"));
        assert_eq!(whisper.task.as_deref(), Some("transcribe"), "never translated");
        assert_eq!(config.model_config.transducer.encoder, None);
        assert!((1..=4).contains(&config.model_config.num_threads));
        assert_eq!(config.feat_config.sample_rate, RATE as i32);

        for found in ["auto", ""] {
            let config = recognizer_config(Recognizer::Whisper, folder, found);
            assert_eq!(config.model_config.whisper.language, None, "{found:?}: Whisper finds it");
        }
    }

    /// Issue #762: Parakeet is a NeMo transducer without a language.
    #[test]
    fn parakeet_is_a_transducer_without_a_language() {
        let folder = Path::new("/voice");
        let config = recognizer_config(Recognizer::Parakeet, folder, "de");
        let model = folder.join("models").join("parakeet");
        let transducer = &config.model_config.transducer;
        assert_eq!(path(&transducer.encoder), model.join("encoder.int8.onnx").to_string_lossy());
        assert_eq!(path(&transducer.decoder), model.join("decoder.int8.onnx").to_string_lossy());
        assert_eq!(path(&transducer.joiner), model.join("joiner.int8.onnx").to_string_lossy());
        assert_eq!(path(&config.model_config.tokens), model.join("tokens.txt").to_string_lossy());
        assert_eq!(config.model_config.model_type.as_deref(), Some("nemo_transducer"));
        assert_eq!(config.model_config.whisper.encoder, None);
        assert_eq!(config.model_config.whisper.language, None);
    }

    /// Every file the configurations name is a file of the catalogue's model.
    #[test]
    fn the_configurations_name_the_catalogue_files() {
        for recognizer in [Recognizer::Whisper, Recognizer::Parakeet] {
            let config = recognizer_config(recognizer, Path::new("/voice"), "en");
            let model = torro_core::voice_models::model(recognizer.model()).expect("in the catalogue");
            let named = [
                &config.model_config.whisper.encoder,
                &config.model_config.whisper.decoder,
                &config.model_config.transducer.encoder,
                &config.model_config.transducer.decoder,
                &config.model_config.transducer.joiner,
                &config.model_config.tokens,
            ];
            for file in named.into_iter().flatten() {
                assert!(model.files.iter().any(|known| file.ends_with(known.name)), "{file}");
            }
        }
    }

    /// Issue #762: without the model, loading names what is missing and reads nothing.
    #[test]
    fn a_model_not_downloaded_is_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let missing = Some(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model));
        for recognizer in [Recognizer::Whisper, Recognizer::Parakeet] {
            let voice = Voice { recognizer, ..Voice::default() };
            assert_eq!(SherpaTranscriber::load(&voice, Some(folder.path())).err(), missing);
            assert_eq!(SherpaTranscriber::load(&voice, None).err(), missing, "no voice folder");
        }
    }

    #[test]
    fn recogniser_output_becomes_plain_text() {
        assert_eq!(transcript_of(" Hey Torro, what is on today?\n"), "Hey Torro, what is on today?");
        assert_eq!(transcript_of("[BLANK_AUDIO]\n"), "");
        assert_eq!(transcript_of(" (music) Search for\n Anna's mail."), "Search for Anna's mail.");
    }

    /// Counts the sentences it was given.
    struct Counts(Arc<Mutex<usize>>);

    impl Transcriber for Counts {
        fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
            *self.0.lock().expect("count") += 1;
            Ok("heard".into())
        }
    }

    #[test]
    fn a_shared_transcriber_is_one() {
        let count = Arc::new(Mutex::new(0));
        let mut sentences = SharedTranscriber::new(Box::new(Counts(Arc::clone(&count))));
        let mut wake = sentences.clone();
        // The wake word asks from a thread of its own.
        let asked = std::thread::spawn(move || wake.transcribe(&[1]).expect("text")).join().expect("thread");
        assert_eq!(asked, "heard");
        assert_eq!(sentences.transcribe(&[1]).expect("text"), "heard");
        assert_eq!(*count.lock().expect("count"), 2, "both went to the same transcriber");
    }
}
