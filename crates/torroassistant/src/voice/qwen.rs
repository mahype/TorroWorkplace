//! Qwen3-TTS built in (issue #764, `docs/messung-qwen3-tts.md`): the 0.6B Base model through Crane on
//! candle, on an NVIDIA graphics card, without Python. It clones the voice templates of the app
//! ([`TEMPLATES`], "Warm woman" and "Calm man") with their transcripts, piece by piece, so the first
//! sound comes once the first piece is computed. Only in builds with the feature `qwen-cuda`; the
//! parts that need no model (voices, pieces, the silence before the first word) are here in every
//! build, so they are tested without a graphics card.
//!
//! [`Qwen`] runs behind the same [`Synth`] as Pocket TTS, on a thread of its own in the reading helper
//! ([`super::speech`]). It holds about 8 GB of graphics memory while loaded; the helper drops it when
//! another provider reads.

#[cfg(feature = "qwen-cuda")]
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use torro_core::settings::{QWEN_RECOMMENDED, QWEN_VOICES};
use torro_core::voice_models::QWEN;

use super::VoiceError;
use super::pocket::TEMPLATES;
#[cfg(feature = "qwen-cuda")]
use super::pocket::{Chunk, Delivery, Synth};
#[cfg(feature = "qwen-cuda")]
use super::speech::pieces;

/// The name Qwen3-TTS goes by in messages.
#[cfg(feature = "qwen-cuda")]
const NAME: &str = "Qwen3-TTS";

/// The longest piece computed at once, in characters: as for Pocket TTS, a sentence or two, so the
/// first sound comes after about two seconds (measured 2.5 s for 184 characters) and a stop takes
/// effect between two pieces at the latest.
pub const PIECE: usize = 200;

/// What the templates say, for cloning in context: Qwen3-TTS hears how the words sound in the
/// voice. Recorded for issue #745.
pub const TRANSCRIPTS: [(&str, &str); 2] = [
    (
        "warm_woman",
        "Hallo, schön, dass du da bist. Ich lese dir deine Antworten vor, ruhig und deutlich, damit du ganz \
         entspannt zuhören kannst.",
    ),
    (
        "calm_man",
        "Guten Tag. Ich bin dein Assistent und lese dir die Antworten vor, in aller Ruhe, deutlich und gut \
         verständlich.",
    ),
];

/// The template and its transcript for the voice `voice`; one that is no Qwen voice reads as
/// [`QWEN_RECOMMENDED`].
pub fn template(voice: &str) -> Option<(&'static [u8], &'static str)> {
    let voice = if QWEN_VOICES.contains(&voice) { voice } else { QWEN_RECOMMENDED };
    let bytes = TEMPLATES.iter().find(|(id, _)| *id == voice).map(|(_, bytes)| *bytes)?;
    let words = TRANSCRIPTS.iter().find(|(id, _)| *id == voice).map(|(_, words)| *words)?;
    Some((bytes, words))
}

/// The language as Qwen3-TTS names it, for the language code `code` of the suite (`"de"`, `"en"`);
/// `"auto"` for one it does not know.
pub fn language(code: &str) -> &'static str {
    match code {
        "de" => "german",
        "en" => "english",
        "fr" => "french",
        "es" => "spanish",
        "it" => "italian",
        "pt" => "portuguese",
        "ru" => "russian",
        "ja" => "japanese",
        "ko" => "korean",
        "zh" => "chinese",
        _ => "auto",
    }
}

/// The most frames of sound computed for a piece of `chars` characters: twice what is spoken at a
/// calm pace (12 frames a second, about 15 characters a second), so a piece the model does not end
/// by itself (it happens, `docs/messung-qwen3-tts.md`) stops after seconds, not minutes.
pub fn most_frames(chars: usize) -> usize {
    48 + chars.saturating_mul(2)
}

/// How quiet the start of a piece may be and still count as silence (about -40 dB).
const QUIET: f32 = 0.01;

/// How much of the silence before the first word is kept, in seconds: the word does not start
/// abruptly.
const LEAD: f32 = 0.05;

/// `samples` at `rate` without the silence before the first word but [`LEAD`]: Qwen3-TTS often starts
/// a piece with half a second of it, once with eight (`docs/messung-qwen3-tts.md`). Sound that is all
/// silence stays as it is.
pub fn without_lead(samples: &[f32], rate: u32) -> &[f32] {
    let Some(first) = samples.iter().position(|sample| sample.abs() > QUIET) else { return samples };
    // The lead is a few thousand samples at most, far below what loses precision in an `f32`.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let lead = (rate as f32 * LEAD) as usize;
    &samples[first.saturating_sub(lead)..]
}

/// Crane reads the speech tokenizer from `<model>/speech_tokenizer/`; the catalogue keeps its files flat
/// as `speech_tokenizer.<name>` (`torro_core::voice_models::QWEN`). Links each into place, as a hard
/// link (no copy of 680 MB, and it works on every system); a link that is there with the size of its
/// file is kept. The folder of the model in the voice folder `folder`.
pub fn place_tokenizer(folder: &Path) -> Result<PathBuf, VoiceError> {
    let model = torro_core::voice_models::model(QWEN)
        .ok_or_else(|| VoiceError::program("Qwen3-TTS", "this build has no Qwen3-TTS"))?;
    let home = model.folder(folder);
    let inner = home.join("speech_tokenizer");
    let failed = |err: std::io::Error| VoiceError::program("Qwen3-TTS", format!("{}: {err}", inner.display()));
    std::fs::create_dir_all(&inner).map_err(failed)?;
    for file in model.files {
        let Some(name) = file.name.strip_prefix("speech_tokenizer.") else { continue };
        let (from, to) = (home.join(file.name), inner.join(name));
        if std::fs::metadata(&to).is_ok_and(|meta| meta.len() == file.bytes) {
            continue;
        }
        // A link of a file downloaded again (or cut short) is replaced.
        let _ = std::fs::remove_file(&to);
        std::fs::hard_link(&from, &to).map_err(failed)?;
    }
    Ok(home)
}

/// The NVIDIA card Qwen3-TTS computes on, as the settings show it (`NVIDIA GeForce RTX 4090, 24 GB`);
/// `None` when there is none, or its libraries do not answer.
#[cfg(feature = "qwen-cuda")]
pub fn graphics_card() -> Option<String> {
    use crane_core::models::Device;
    let Ok(Device::Cuda(device)) = Device::new_cuda(0) else { return None };
    let stream = device.cuda_stream();
    let context = stream.context();
    let name = context.name().ok()?;
    let gigabytes = context.total_mem().map(|bytes| bytes.div_ceil(1 << 30)).unwrap_or_default();
    Some(if gigabytes > 0 { format!("{name}, {gigabytes} GB") } else { name })
}

/// Qwen3-TTS loaded once on the graphics card, with the templates it has read.
#[cfg(feature = "qwen-cuda")]
pub struct Qwen {
    model: crane_core::models::qwen3_tts::Model,
    language: &'static str,
    voices: HashMap<String, (Vec<f32>, &'static str)>,
}

#[cfg(feature = "qwen-cuda")]
impl Qwen {
    /// Loads the model from the voice folder `folder` onto the first NVIDIA card, speaking the language
    /// of the suite `code` ([`language`]). It must be downloaded (the reading helper looks first);
    /// without a card it is a [`VoiceError::Program`], never a crash.
    pub fn load(folder: &Path, code: &str) -> Result<Qwen, VoiceError> {
        use crane_core::models::{DType, Device};
        let home = place_tokenizer(folder)?;
        let device =
            Device::new_cuda(0).map_err(|err| VoiceError::program(NAME, format!("no graphics card: {err}")))?;
        let path = home.to_string_lossy();
        let model = crane_core::models::qwen3_tts::Model::new(&path, &device, &DType::BF16)
            .map_err(|err| VoiceError::program(NAME, format!("did not load: {err}")))?;
        Ok(Qwen { model, language: language(code), voices: HashMap::new() })
    }

    /// The template of `voice` at the rate of the speaker encoder, and its transcript.
    fn voice(&mut self, voice: &str) -> Result<(Vec<f32>, &'static str), VoiceError> {
        if let Some(known) = self.voices.get(voice) {
            return Ok(known.clone());
        }
        let (bytes, words) = template(voice).ok_or_else(|| VoiceError::program(NAME, "no voice template"))?;
        let wave = super::audio::decode_wav(bytes)?;
        if wave.rate != self.model.speaker_encoder_sample_rate() {
            return Err(VoiceError::program(NAME, format!("the template is at {} Hz", wave.rate)));
        }
        self.voices.insert(voice.to_string(), (wave.samples.clone(), words));
        Ok((wave.samples, words))
    }
}

#[cfg(feature = "qwen-cuda")]
impl Synth for Qwen {
    fn rate(&self) -> u32 {
        self.model.sample_rate()
    }

    // Speed and pause are Pocket's (issue #782): Qwen3-TTS speaks at its own pace.
    fn speak(&mut self, text: &str, voice: &str, _delivery: Delivery, mut chunk: Chunk) -> Result<(), VoiceError> {
        use crane_core::generation::SpeechOptions;
        use crane_core::models::DType;
        let (reference, words) = self.voice(voice)?;
        let rate = self.rate();
        for piece in pieces(text, PIECE) {
            // The settings of the measurement, which are those of Crane and of Qwen's own examples.
            let options = SpeechOptions {
                max_new_tokens: most_frames(piece.chars().count()),
                temperature: 0.9,
                top_p: Some(1.0),
                repetition_penalty: 1.05,
                ..SpeechOptions::default()
            };
            let (audio, _) = self
                .model
                .generate_voice_clone(&piece, self.language, &reference, words, &options)
                .map_err(|err| VoiceError::program(NAME, format!("made no sound: {err}")))?;
            let samples: Vec<f32> = audio
                .to_dtype(DType::F32)
                .and_then(|audio| audio.flatten_all())
                .and_then(|audio| audio.to_vec1())
                .map_err(|err| VoiceError::program(NAME, format!("made no sound: {err}")))?;
            // A stop is heard between two pieces: computing one cannot be interrupted.
            if !chunk(without_lead(&samples, rate)) {
                return Ok(());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #764: each Qwen voice has its template and transcript; another id reads as the recommended.
    #[test]
    fn every_voice_has_its_template_and_words() {
        for voice in QWEN_VOICES {
            let (bytes, words) = template(voice).expect("a template");
            assert_eq!(&bytes[..4], b"RIFF", "{voice}");
            assert!(words.ends_with('.'), "{voice}");
        }
        assert_eq!(template("alba"), template(QWEN_RECOMMENDED), "a Pocket voice is no Qwen voice");
        assert!(template("warm_woman").is_some_and(|(_, words)| words.starts_with("Hallo")));
        assert!(template("calm_man").is_some_and(|(_, words)| words.starts_with("Guten Tag")));
    }

    #[test]
    fn languages_and_frames() {
        assert_eq!(
            (language("de"), language("en"), language("auto"), language("xx")),
            ("german", "english", "auto", "auto")
        );
        assert_eq!(most_frames(0), 48);
        // 200 characters are spoken in about 13 seconds, 160 frames: the most is well above.
        assert!(most_frames(PIECE) > 2 * 160);
    }

    /// Issue #764: the silence before the first word goes but 50 ms; sound without any stays whole.
    #[test]
    fn the_silence_before_the_first_word_is_cut() {
        let mut samples = vec![0.0; 24_000];
        samples.extend([0.5, -0.5, 0.2]);
        let cut = without_lead(&samples, 24_000);
        assert_eq!(cut.len(), 1_200 + 3, "50 ms at 24 kHz before the first sound");
        assert_eq!(cut[1_200], 0.5);
        let quiet = vec![0.001; 10];
        assert_eq!(without_lead(&quiet, 24_000), &quiet[..], "all silence stays");
        assert_eq!(without_lead(&[0.3, 0.0], 24_000), &[0.3, 0.0], "nothing before the first word");
        assert!(without_lead(&[], 24_000).is_empty());
    }

    /// Issue #764: the tokenizer's flat files are linked where Crane looks, once; with the feature only,
    /// since the catalogue lists the model only then.
    #[cfg(feature = "qwen-cuda")]
    #[test]
    fn the_tokenizer_is_linked_into_its_folder() {
        let dir = tempfile::tempdir().expect("voice folder");
        let model = torro_core::voice_models::model(QWEN).expect("in the catalogue");
        std::fs::create_dir_all(model.folder(dir.path())).expect("folder");
        for file in model.files.iter().filter(|file| file.name.starts_with("speech_tokenizer.")) {
            let made = std::fs::File::create(model.path(dir.path(), file.name)).expect("file");
            made.set_len(file.bytes).expect("size");
        }
        let home = place_tokenizer(dir.path()).expect("linked");
        for name in ["config.json", "model.safetensors"] {
            assert!(home.join("speech_tokenizer").join(name).is_file(), "{name}");
        }
        place_tokenizer(dir.path()).expect("again, kept");
    }

    /// Issue #764, by hand on a computer with an NVIDIA card: loads the downloaded model from the voice
    /// folder `TORRO_QWEN_VOICE` and computes a sentence in each voice, without playing it. Ignored, since
    /// tests run without a graphics card and a 2.5 GB model:
    /// `TORRO_QWEN_VOICE=<voice folder> cargo test -p torroassistant --features qwen-cuda -- --ignored real_model`.
    #[cfg(feature = "qwen-cuda")]
    #[test]
    #[ignore = "needs an NVIDIA card and the downloaded model"]
    fn reads_with_the_real_model() {
        let folder = PathBuf::from(std::env::var("TORRO_QWEN_VOICE").expect("TORRO_QWEN_VOICE"));
        assert!(graphics_card().is_some(), "an NVIDIA card");
        let mut qwen = Qwen::load(&folder, "de").expect("loaded");
        for voice in QWEN_VOICES {
            let sound = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let heard = std::rc::Rc::clone(&sound);
            let chunk: Chunk = Box::new(move |samples| {
                heard.borrow_mut().extend_from_slice(samples);
                true
            });
            qwen.speak("Gern, ich habe den Termin für morgen eingetragen.", voice, Delivery::default(), chunk)
                .expect("spoken");
            let seconds = sound.borrow().len() as f32 / qwen.rate() as f32;
            assert!((1.5..12.0).contains(&seconds), "{voice}: {seconds} s");
            let first = sound.borrow().iter().position(|sample| sample.abs() > QUIET).unwrap_or(usize::MAX);
            assert!(first as f32 <= qwen.rate() as f32 * LEAD + 1.0, "{voice}: the lead is cut");
        }
    }

    #[cfg(not(feature = "qwen-cuda"))]
    #[test]
    fn without_the_feature_nothing_is_linked() {
        let dir = tempfile::tempdir().expect("voice folder");
        assert!(matches!(place_tokenizer(dir.path()), Err(VoiceError::Program { .. })));
    }
}
