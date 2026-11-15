//! The listener's parts as they stand (issue #762): every part is built into the app, so what can be
//! missing is a model not yet downloaded (Settings › Assistant › Voice, `settings.download_voice_model`)
//! or a sound device. The wake word has a model of its own built in (issue #765, [`wake_word`]); only when
//! it does not load is the wake word heard with speech recognition. [`voice_status`] answers `--voice-status`
//! for `settings.voice_status`, [`Parts::build`] puts the parts together for the settings.
//!
//! Models are looked for only in the voice folder of the catalogue (`<data>/torroassistant/voice/models/<id>/`,
//! [`torro_core::voice_models::voice_folder`]); nothing is looked for on `PATH`.

use std::path::Path;

use torro_core::settings::{SpeechProvider, Voice};
use torro_core::voice_models::{self, HEY_TORRO, POCKET_DE};
use torro_ipc::{VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus};

use super::recognize::{SharedTranscriber, SherpaTranscriber};
use super::sound::{ChimeSpeaker, CpalRecorder, Devices, microphone_in_use};
use super::wake::{ModelWake, TranscriptWake};
use super::{Recorder, Speaker, Transcriber, VoiceError, WakeWord};

/// Whether the model `id` of the catalogue is downloaded into the voice folder `folder`.
pub fn model_ready(id: &str, folder: Option<&Path>) -> bool {
    folder.zip(voice_models::model(id)).is_some_and(|(folder, model)| model.ready(folder))
}

/// The parts the listener runs with.
pub struct Parts {
    pub recorder: Box<dyn Recorder>,
    pub wake: Box<dyn WakeWord>,
    pub transcriber: Box<dyn Transcriber>,
    pub speaker: Box<dyn Speaker>,
}

impl Parts {
    /// The parts for `voice` with the models in the voice folder `folder`: the microphone of the settings,
    /// the chosen speech to text, loaded now, and the wake word of [`wake_word`]. The speaker only plays
    /// the tone; answers are read through the core. Fails when the recogniser's model is missing or does
    /// not load.
    pub fn build(voice: &Voice, folder: Option<&Path>) -> Result<Parts, VoiceError> {
        let transcriber = SharedTranscriber::new(Box::new(SherpaTranscriber::load(voice, folder)?));
        let (wake, _) = wake_word(folder, transcriber.clone());
        Ok(Parts {
            recorder: Box::new(CpalRecorder { microphone: voice.microphone.clone() }),
            wake,
            transcriber: Box::new(transcriber),
            speaker: Box::new(ChimeSpeaker),
        })
    }
}

/// The wake word with the models in the voice folder `folder`, and what it runs on: the built-in model
/// ([`ModelWake`], [`HEY_TORRO`]), or – without a folder or when that model does not load – speech
/// recognition on short windows with `transcriber` ([`TranscriptWake`], `"transcript"`), so "Hey Torro"
/// keeps waking the assistant. Why the model did not load goes to the log, without the path.
pub fn wake_word(folder: Option<&Path>, transcriber: SharedTranscriber) -> (Box<dyn WakeWord>, &'static str) {
    if let Some(folder) = folder {
        match ModelWake::load(folder) {
            Ok(model) => return (Box::new(model), HEY_TORRO),
            Err(VoiceError::Program { message, .. }) => {
                let message = message.replace(&*folder.to_string_lossy(), "<voice folder>");
                eprintln!("torroassistant: the wake word model did not load, speech recognition hears it: {message}");
            }
            Err(err) => {
                eprintln!("torroassistant: the wake word model did not load, speech recognition hears it: {err}")
            }
        }
    }
    (Box::new(TranscriptWake::new(Box::new(transcriber))), "transcript")
}

/// Every part as `settings.voice_status` reports it, in the order of [`VoicePart`]: the wake word names
/// its built-in model and misses nothing (issue #765), speech to text works once the recogniser's model is
/// downloaded (`program` names the model), the
/// recorder once there is a microphone (`program` names the one in use: a stored name the system does
/// not know gives the default), speech once there is a sound output and, for Pocket TTS, its model.
pub fn programs(voice: &Voice, folder: Option<&Path>, devices: &dyn Devices) -> Vec<VoiceProgram> {
    let part = |part, program: Option<String>, missing| VoiceProgram { part, program, missing };
    let recognizer = voice.recognizer.model();
    let heard = match model_ready(recognizer, folder) {
        true => None,
        false => Some(VoiceMissing::Model),
    };
    let wake = part(VoicePart::WakeWord, Some(HEY_TORRO.to_string()), None);
    let stt = part(VoicePart::SpeechToText, Some(recognizer.to_string()), heard);
    let recorder = match microphone_in_use(devices, voice.microphone.as_deref()) {
        Some(name) => part(VoicePart::Recorder, Some(name), None),
        None => part(VoicePart::Recorder, None, Some(VoiceMissing::Program)),
    };
    let provider = voice.speech_provider.unwrap_or(SpeechProvider::Pocket);
    let program = match provider {
        SpeechProvider::Pocket => POCKET_DE,
        SpeechProvider::OpenAi => provider.id(),
    };
    let speech_missing = if !devices.has_output() {
        Some(VoiceMissing::Player)
    } else if provider == SpeechProvider::Pocket && !model_ready(POCKET_DE, folder) {
        Some(VoiceMissing::Model)
    } else {
        None
    };
    let speech = part(VoicePart::Speech, Some(program.to_string()), speech_missing);
    vec![wake, recorder, stt, speech]
}

/// What the listener would use with `voice` (`--voice-status`): its [`programs`], the microphones the
/// system offers and the voice folder `folder`.
pub fn voice_status(voice: &Voice, folder: Option<&Path>, devices: &dyn Devices) -> VoiceStatus {
    VoiceStatus {
        listener: VoiceState::Off,
        installed: true,
        problem: None,
        programs: programs(voice, folder, devices),
        microphones: devices.microphones(),
        folder: folder.map(|folder| folder.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs::File;

    use torro_core::settings::Recognizer;
    use torro_core::voice_models::Model;

    use super::super::sound::tests::FakeDevices;
    use super::*;

    /// Writes every file of `model` into the voice folder `folder` with its size, sparse so it costs no
    /// disk: [`Model::ready`] only looks at the sizes.
    pub fn download(folder: &Path, id: &str) {
        let model: &Model = voice_models::model(id).expect("in the catalogue");
        std::fs::create_dir_all(model.folder(folder)).expect("model folder");
        for file in model.files {
            File::create(model.path(folder, file.name)).and_then(|f| f.set_len(file.bytes)).expect("model file");
        }
    }

    fn devices() -> FakeDevices {
        FakeDevices {
            microphones: vec!["HD Pro Webcam C920, USB Audio".into()],
            default: Some("Default Audio Device".into()),
            output: true,
            held: Vec::new(),
        }
    }

    fn missing(programs: &[VoiceProgram]) -> Vec<(VoicePart, Option<&str>, Option<VoiceMissing>)> {
        programs.iter().map(|program| (program.part, program.program.as_deref(), program.missing)).collect()
    }

    /// Issue #762: without models every part but the recorder and the built-in wake word (#765) misses its
    /// model; nothing is looked for on `PATH`.
    #[test]
    fn without_models_the_models_are_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let status = voice_status(&Voice::default(), Some(folder.path()), &devices());
        assert_eq!(
            missing(&status.programs),
            [
                (VoicePart::WakeWord, Some("hey-torro"), None),
                (VoicePart::Recorder, Some("Default Audio Device"), None),
                (VoicePart::SpeechToText, Some("whisper-turbo"), Some(VoiceMissing::Model)),
                (VoicePart::Speech, Some("pocket-de"), Some(VoiceMissing::Model)),
            ]
        );
        assert!(!status.can_listen());
        assert_eq!(status.microphones, ["HD Pro Webcam C920, USB Audio"]);
        assert_eq!(status.folder.as_deref(), Some(folder.path().to_string_lossy().as_ref()));
        assert!(status.installed);
    }

    /// A recogniser that hears nothing, for the wake word without a model.
    struct Hears;

    impl Transcriber for Hears {
        fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
            Ok(String::new())
        }
    }

    /// Issue #765: the wake word runs on the built-in model; when it does not load, speech recognition
    /// hears "Hey Torro" (Erledigt 3).
    #[test]
    fn the_wake_word_runs_on_its_model_and_else_on_speech_recognition() {
        let folder = tempfile::tempdir().expect("voice folder");
        let shared = || SharedTranscriber::new(Box::new(Hears));
        assert_eq!(wake_word(Some(folder.path()), shared()).1, HEY_TORRO);
        let file = tempfile::NamedTempFile::new().expect("not a folder");
        assert_eq!(wake_word(Some(file.path()), shared()).1, "transcript");
        assert_eq!(wake_word(None, shared()).1, "transcript");
    }

    /// Issue #762: the downloaded models make the parts work; only the chosen recogniser counts.
    #[test]
    fn downloaded_models_make_the_parts_work() {
        let folder = tempfile::tempdir().expect("voice folder");
        download(folder.path(), "parakeet");
        download(folder.path(), POCKET_DE);
        let parakeet = Voice { recognizer: Recognizer::Parakeet, ..Voice::default() };
        let status = voice_status(&parakeet, Some(folder.path()), &devices());
        assert!(status.programs.iter().all(VoiceProgram::works), "{:?}", status.programs);
        assert_eq!(status.program(VoicePart::SpeechToText).and_then(|p| p.program.as_deref()), Some("parakeet"));
        assert!(status.can_listen());

        let whisper = programs(&Voice::default(), Some(folder.path()), &devices());
        assert_eq!(whisper[2].missing, Some(VoiceMissing::Model), "Whisper is not downloaded");
    }

    /// Issue #762: the microphone in use is reported; an old `pactl` name gives the default, no microphone
    /// at all misses the recorder.
    #[test]
    fn the_recorder_names_the_microphone_in_use() {
        let named = Voice { microphone: Some("HD Pro Webcam C920, USB Audio".into()), ..Voice::default() };
        assert_eq!(programs(&named, None, &devices())[1].program.as_deref(), Some("HD Pro Webcam C920, USB Audio"));
        let old = Voice { microphone: Some("alsa_input.usb-Rode_NT-USB-00.analog-stereo".into()), ..Voice::default() };
        assert_eq!(programs(&old, None, &devices())[1].program.as_deref(), Some("Default Audio Device"));
        let none = programs(&Voice::default(), None, &FakeDevices::default());
        assert_eq!((none[1].program.as_deref(), none[1].missing), (None, Some(VoiceMissing::Program)));
    }

    /// Issue #762: speech needs a sound output first, then Pocket's model; OpenAI needs no model here.
    #[test]
    fn speech_needs_an_output_and_pocket_its_model() {
        let folder = tempfile::tempdir().expect("voice folder");
        let silent = FakeDevices { output: false, ..devices() };
        let speech = |voice: &Voice, devices: &FakeDevices| programs(voice, Some(folder.path()), devices)[3].clone();
        assert_eq!(speech(&Voice::default(), &silent).missing, Some(VoiceMissing::Player));
        let openai = Voice { speech_provider: Some(SpeechProvider::OpenAi), ..Voice::default() };
        assert_eq!(speech(&openai, &devices()).missing, None);
        assert_eq!(speech(&openai, &devices()).program.as_deref(), Some("openai"));
        assert_eq!(speech(&openai, &silent).missing, Some(VoiceMissing::Player));
        download(folder.path(), POCKET_DE);
        assert_eq!(speech(&Voice::default(), &devices()).missing, None);
    }
}
