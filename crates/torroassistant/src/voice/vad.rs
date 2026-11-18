//! Speech told from noise by Silero VAD (issue #833), through sherpa-onnx's voice activity detector:
//! a small model judges every 32 ms whether someone speaks, so music or a fan in the room does not hold
//! a sentence open as loudness does. It is a download of the catalogue ([`SILERO_VAD`]); without it the
//! listener tells speech by loudness ([`super::audio::Endpoint::push`]).
//!
//! [`vad_config`] builds the configuration without reading a file, [`SileroVad`] keeps the model loaded.

use std::path::Path;

use sha2::{Digest, Sha256};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use torro_core::voice_models::SILERO_VAD;
use torro_ipc::{VoiceMissing, VoicePart};

use super::audio::{RATE, to_float};
use super::{SpeechDetector, VoiceError};

/// The name speech detection goes by in messages.
const NAME: &str = "speech detection";

/// How many seconds of speech the detector keeps for its segments: they are cleared after every frame, so
/// this only has to hold more than its longest piece of speech.
const BUFFER_SECONDS: f32 = 30.0;

/// The detector's configuration with the model in the voice folder `folder`
/// (`<folder>/models/silero-vad/silero_vad.onnx`). Speech ends after 100 ms of silence and counts from
/// 100 ms on, so a sentence ends about as soon as the pause of [`super::audio::Limits`] is over; the window
/// of 512 samples is what Silero VAD takes at 16 kHz. Builds nothing and reads no file, so it is tested
/// without the model.
pub fn vad_config(folder: &Path) -> VadModelConfig {
    let model = folder.join("models").join(SILERO_VAD).join("silero_vad.onnx");
    VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: 0.5,
            min_silence_duration: 0.1,
            min_speech_duration: 0.1,
            window_size: 512,
            max_speech_duration: 20.0,
        },
        sample_rate: RATE as i32,
        num_threads: 1,
        ..VadModelConfig::default()
    }
}

/// Silero VAD, loaded once.
pub struct SileroVad {
    detector: VoiceActivityDetector,
}

impl SileroVad {
    /// Loads the model from the voice folder `folder`. One not downloaded is [`VoiceError::Missing`] of
    /// speech to text, without a try to load it; one whose file is not the catalogue's (damaged on disk) or
    /// that sherpa-onnx cannot load a [`VoiceError::Program`]. The file is checked against its SHA-256 first:
    /// at 1.8 MB that takes milliseconds, and sherpa-onnx, given a damaged file, would write its path to the
    /// log itself.
    pub fn load(folder: Option<&Path>) -> Result<SileroVad, VoiceError> {
        let (folder, model) = match (folder, torro_core::voice_models::model(SILERO_VAD)) {
            (Some(folder), Some(model)) if model.ready(folder) => (folder, model),
            _ => return Err(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model)),
        };
        for file in model.files {
            let bytes = std::fs::read(model.path(folder, file.name)).map_err(|err| {
                VoiceError::program(NAME, format!("the model {SILERO_VAD} cannot be read: {}", err.kind()))
            })?;
            let hash: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
            if hash != file.sha256 {
                return Err(VoiceError::program(NAME, format!("the model {SILERO_VAD} is damaged: download it again")));
            }
        }
        let detector = VoiceActivityDetector::create(&vad_config(folder), BUFFER_SECONDS)
            .ok_or_else(|| VoiceError::program(NAME, format!("the model {SILERO_VAD} did not load")))?;
        Ok(SileroVad { detector })
    }
}

impl SpeechDetector for SileroVad {
    fn speech(&mut self, frame: &[i16]) -> Result<bool, VoiceError> {
        let floats: Vec<f32> = frame.iter().copied().map(to_float).collect();
        self.detector.accept_waveform(&floats);
        let speech = self.detector.detected();
        // Only whether someone speaks counts; the segments it cuts are not needed and would pile up.
        self.detector.clear();
        Ok(speech)
    }

    fn reset(&mut self) {
        self.detector.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::audio::FRAME;
    use super::super::audio::tests::fixture;
    use super::*;

    /// The detector's file is the file of the catalogue's model.
    #[test]
    fn the_configuration_names_the_catalogue_file() {
        let folder = Path::new("/voice");
        let config = vad_config(folder);
        let model = torro_core::voice_models::model(SILERO_VAD).expect("in the catalogue");
        let file = model.path(folder, model.files[0].name);
        assert_eq!(config.silero_vad.model.as_deref(), Some(file.to_string_lossy().as_ref()));
        assert_eq!((config.sample_rate, config.silero_vad.window_size), (RATE as i32, 512));
        assert_eq!(config.ten_vad.model, None, "only Silero VAD");
    }

    /// Without the model nothing is loaded: speech is told by loudness.
    #[test]
    fn without_the_model_it_is_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let missing = Err(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model));
        assert_eq!(SileroVad::load(Some(folder.path())).map(|_| ()), missing);
        assert_eq!(SileroVad::load(None).map(|_| ()), missing);
    }

    /// Whether any frame of the fixture `name` is speech to the model.
    fn speaks(vad: &mut SileroVad, name: &str) -> bool {
        vad.reset();
        let mut heard = false;
        for frame in fixture(name).chunks(FRAME) {
            heard |= vad.speech(frame).expect("judged");
        }
        heard
    }

    /// With the model (`TORRO_SILERO_VAD=<voice folder>` holding `models/silero-vad/silero_vad.onnx`):
    /// "Hey Torro" is speech, silence is none.
    #[test]
    #[ignore = "needs the Silero VAD model: TORRO_SILERO_VAD=<voice folder>"]
    fn the_model_hears_speech_and_not_silence() {
        let folder = std::env::var_os("TORRO_SILERO_VAD").expect("TORRO_SILERO_VAD");
        let mut vad = SileroVad::load(Some(Path::new(&folder))).expect("the model loads");
        assert!(speaks(&mut vad, "hey_torro.wav"), "speech");
        assert!(!speaks(&mut vad, "silence.wav"), "silence");
    }
}
