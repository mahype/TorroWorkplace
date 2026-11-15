//! Pocket TTS built in (issue #762): Kyutai's German model through sherpa-onnx, on the processor,
//! without a graphics card or network. It speaks in the voice of a short recording it imitates:
//! Alba, Fantine and Marius come with the model (`torro_core::voice_models::POCKET_VOICE_FILES`),
//! "Warm woman" and "Calm man" with the app ([`TEMPLATES`]). [`tts_config`] builds the configuration,
//! [`Pocket`] keeps the model loaded and hands its sound out piece by piece as it is computed.
//!
//! The reading helper ([`super::speech`]) runs it on a thread of its own behind the small trait
//! [`Synth`], so the tests read with a fake and a stop never waits for the model.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsModelConfig, OfflineTtsPocketModelConfig};
use torro_core::settings::POCKET_RECOMMENDED;
use torro_core::voice_models::{self, POCKET_DE, POCKET_VOICE_FILES};
use torro_ipc::{VoiceMissing, VoicePart};

use super::VoiceError;
use super::audio::{Stretch, Wave, decode_wav};
use super::recognize::threads;
use super::speech::{pieces, sentence_pieces};

/// The voice templates that ship with the assistant (step 4 of the plan of issue #745): made once from a
/// description, 24 kHz mono, about nine seconds.
pub const TEMPLATES: [(&str, &[u8]); 2] = [
    ("warm_woman", include_bytes!("../../assets/voices/warm_woman.wav")),
    ("calm_man", include_bytes!("../../assets/voices/calm_man.wav")),
];

/// The longest piece computed at once, in characters: a sentence or two, so the first sound comes after
/// a few seconds and a stop takes effect between two pieces at the latest.
const PIECE: usize = 200;

/// The name Pocket TTS goes by in messages.
const NAME: &str = "Pocket TTS";

/// Takes computed sound ([`Synth::speak`]); `false` to stop.
pub type Chunk = Box<dyn FnMut(&[f32]) -> bool>;

/// How Pocket TTS speaks (issue #782): `speed` in percent, `pause_ms` the extra silence after each
/// sentence. The default speaks as before the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub speed: u16,
    pub pause_ms: u16,
}

impl Default for Delivery {
    fn default() -> Delivery {
        Delivery { speed: 100, pause_ms: 0 }
    }
}

/// Computes speech: [`Pocket`], a fake in the tests.
pub trait Synth {
    /// The rate of the sound it makes.
    fn rate(&self) -> u32;

    /// Reads `text` in the voice `voice` (an id of `torro_core::settings::POCKET_VOICES`; another reads as
    /// the recommended one), handing the sound to `chunk` piece by piece as it is computed: mono, between
    /// -1 and 1, at [`Synth::rate`], spoken as `delivery` says. Stops soon after `chunk` returns `false`,
    /// and that is no error.
    fn speak(&mut self, text: &str, voice: &str, delivery: Delivery, chunk: Chunk) -> Result<(), VoiceError>;
}

/// The configuration of Pocket TTS with its files in the voice folder `folder` (`<folder>/models/pocket-de/…`).
/// Builds nothing and reads no file, so it is tested without the model.
pub fn tts_config(folder: &Path) -> OfflineTtsConfig {
    let file = |name: &str| Some(folder.join("models").join(POCKET_DE).join(name).to_string_lossy().into_owned());
    OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            pocket: OfflineTtsPocketModelConfig {
                lm_flow: file("lm_flow.int8.onnx"),
                lm_main: file("lm_main.int8.onnx"),
                encoder: file("encoder.onnx"),
                decoder: file("decoder.int8.onnx"),
                text_conditioner: file("text_conditioner.onnx"),
                vocab_json: file("vocab.json"),
                token_scores_json: file("token_scores.json"),
                // The voices met are kept, so a voice's recording is not read again for every piece.
                voice_embedding_cache_capacity: 8,
            },
            num_threads: threads(),
            ..OfflineTtsModelConfig::default()
        },
        ..OfflineTtsConfig::default()
    }
}

/// Where the recording of the voice `voice` comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A file of the model in its folder.
    File(&'static str),
    /// A template of the app ([`TEMPLATES`]).
    Template(&'static [u8]),
}

/// The recording the voice `voice` imitates; an id that is no voice reads as [`POCKET_RECOMMENDED`].
pub fn source(voice: &str) -> Source {
    if let Some((_, bytes)) = TEMPLATES.iter().find(|(id, _)| *id == voice) {
        return Source::Template(bytes);
    }
    let file = |wanted: &str| POCKET_VOICE_FILES.iter().find(|(id, _)| *id == wanted).map(|(_, file)| *file);
    // The recommended voice is a file of the model, so the second lookup always finds one.
    Source::File(file(voice).or_else(|| file(POCKET_RECOMMENDED)).unwrap_or("alba.wav"))
}

/// The sound of the voice `voice`, read from the voice folder `folder` or from the app.
pub fn reference(voice: &str, folder: &Path) -> Result<Wave, VoiceError> {
    match source(voice) {
        Source::Template(bytes) => decode_wav(bytes),
        Source::File(name) => {
            let path = voice_models::model(POCKET_DE)
                .map(|model| model.path(folder, name))
                .ok_or(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Model))?;
            let bytes = std::fs::read(&path).map_err(|err| VoiceError::Sound(format!("{}: {err}", path.display())))?;
            decode_wav(&bytes)
        }
    }
}

/// Pocket TTS loaded once, with the voices it has met.
pub struct Pocket {
    tts: OfflineTts,
    folder: PathBuf,
    voices: HashMap<String, Wave>,
}

impl Pocket {
    /// Loads the model from the voice folder `folder`; it must be downloaded (the reading helper looks
    /// first). A model sherpa-onnx cannot load is a [`VoiceError::Program`].
    pub fn load(folder: &Path) -> Result<Pocket, VoiceError> {
        let tts = OfflineTts::create(&tts_config(folder)).ok_or_else(|| VoiceError::program(NAME, "did not load"))?;
        Ok(Pocket { tts, folder: folder.to_path_buf(), voices: HashMap::new() })
    }

    fn voice(&mut self, voice: &str) -> Result<Wave, VoiceError> {
        if let Some(wave) = self.voices.get(voice) {
            return Ok(wave.clone());
        }
        let wave = reference(voice, &self.folder)?;
        self.voices.insert(voice.to_string(), wave.clone());
        Ok(wave)
    }
}

impl Synth for Pocket {
    fn rate(&self) -> u32 {
        u32::try_from(self.tts.sample_rate()).unwrap_or(24_000)
    }

    fn speak(&mut self, text: &str, voice: &str, delivery: Delivery, chunk: Chunk) -> Result<(), VoiceError> {
        let reference = self.voice(voice)?;
        let config = GenerationConfig {
            reference_sample_rate: i32::try_from(reference.rate).unwrap_or(24_000),
            reference_audio: Some(reference.samples),
            ..GenerationConfig::default()
        };
        let rate = self.rate();
        // sherpa-onnx takes the callback for `'static`; it lives in these cells across the pieces.
        let chunk = Rc::new(RefCell::new(chunk));
        let stopped = Rc::new(Cell::new(false));
        // A NUL would end the text early for sherpa-onnx's C side (and its binding refuses it).
        let text = text.replace('\0', " ");
        // As before without a choice; with a pause each sentence is a piece of its own, so the pause
        // falls between sentences. sherpa-onnx's own `silence_scale` does not reach streamed sound.
        let pieces = match delivery == Delivery::default() {
            true => pieces(&text, PIECE),
            false => sentence_pieces(&text, PIECE),
        };
        let last = pieces.len().saturating_sub(1);
        for (number, piece) in pieces.into_iter().enumerate() {
            // Each piece is stretched on its own, so its end comes out whole before the pause.
            let stretch = Rc::new(RefCell::new(Some(Stretch::new(rate, delivery.speed))));
            let heard = Rc::new(Cell::new(false));
            let (give, stop, got, stretching) =
                (Rc::clone(&chunk), Rc::clone(&stopped), Rc::clone(&heard), Rc::clone(&stretch));
            let callback = move |samples: &[f32], _progress: f32| {
                if samples.is_empty() {
                    return true;
                }
                got.set(true);
                let sound = match stretching.borrow_mut().as_mut() {
                    Some(stretch) => stretch.push(samples),
                    None => samples.to_vec(),
                };
                let go_on = sound.is_empty() || (give.borrow_mut())(&sound);
                stop.set(!go_on);
                go_on
            };
            let audio = self.tts.generate_with_config(&piece, &config, Some(callback));
            if stopped.get() {
                return Ok(());
            }
            let audio = audio.ok_or_else(|| VoiceError::program(NAME, "made no sound"))?;
            let mut rest = Vec::new();
            let stretch = stretch.borrow_mut().take();
            if let Some(mut stretch) = stretch {
                // A model that does not call back as it computes hands out the whole piece at its end.
                if !heard.get() {
                    rest = stretch.push(audio.samples());
                }
                rest.extend(stretch.finish());
            }
            if number < last {
                rest.extend(std::iter::repeat_n(0.0, rate as usize * usize::from(delivery.pause_ms) / 1000));
            }
            if !rest.is_empty() && !(chunk.borrow_mut())(&rest) {
                return Ok(());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use torro_core::settings::POCKET_VOICES;

    use super::*;

    /// Issue #762: every file the configuration names is a file of the catalogue's model, in its folder.
    #[test]
    fn the_configuration_names_the_model_files() {
        let config = tts_config(Path::new("/voice"));
        let pocket = &config.model.pocket;
        let model = voice_models::model(POCKET_DE).expect("in the catalogue");
        let named = [
            &pocket.lm_flow,
            &pocket.lm_main,
            &pocket.encoder,
            &pocket.decoder,
            &pocket.text_conditioner,
            &pocket.vocab_json,
            &pocket.token_scores_json,
        ];
        let folder = Path::new("/voice").join("models").join(POCKET_DE);
        for file in named {
            let file = file.as_deref().expect("named");
            assert!(Path::new(file).starts_with(&folder), "{file}");
            assert!(model.files.iter().any(|known| file.ends_with(known.name)), "{file}");
        }
        assert!((1..=4).contains(&config.model.num_threads));
    }

    /// Issue #762: each voice of the settings has its recording; an unknown id reads as the recommended.
    #[test]
    fn every_voice_has_its_recording() {
        assert_eq!(source("alba"), Source::File("alba.wav"));
        assert_eq!(source("marius"), Source::File("marius.wav"));
        assert_eq!(source("warm_woman"), Source::Template(TEMPLATES[0].1));
        assert_eq!(source("qwen-voice"), source(POCKET_RECOMMENDED));
        for voice in POCKET_VOICES {
            match source(voice) {
                Source::File(name) => {
                    let model = voice_models::model(POCKET_DE).expect("in the catalogue");
                    assert!(model.files.iter().any(|file| file.name == name), "{voice}: {name}");
                }
                Source::Template(bytes) => assert_eq!(&bytes[..4], b"RIFF", "{voice}"),
            }
        }
    }

    /// The templates are real WAV files at 24 kHz, read without the model; a voice file of the model that is
    /// not downloaded says so.
    #[test]
    fn the_templates_are_read_and_a_missing_file_is_named() {
        let folder = tempfile::tempdir().expect("voice folder");
        for (id, _) in TEMPLATES {
            let wave = reference(id, folder.path()).expect("a template");
            assert_eq!(wave.rate, 24_000, "{id}");
            assert!(wave.samples.len() > 24_000, "{id}: a few seconds");
        }
        assert_eq!(TEMPLATES.map(|(id, _)| id), POCKET_VOICES[3..]);
        assert!(matches!(reference("alba", folder.path()), Err(VoiceError::Sound(_))));
    }
}
