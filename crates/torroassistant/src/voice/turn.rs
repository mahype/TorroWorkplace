//! When a spoken sentence is over (issue #836): a pause alone does not say it. "What is the weather
//! tomorrow?" is over after half a second, "I want to … um …" is not after one and a half, and "wait a
//! moment" asks for time. In every pause of [`super::listen::EARLY_AFTER`] the listener judges the sentence
//! so far by two things and waits as long as that says ([`Pauses`]):
//!
//! - the tone of its last seconds, by Smart Turn of Pipecat ([`SmartTurn`], a download of the catalogue,
//!   [`SMART_TURN`]): a small model that hears whether a voice finished, German and English among its
//!   languages,
//! - its text, as it was recognised ahead ([`super::early`]), by rules ([`rules`]): a filler or a connecting
//!   word at the end ("um", "and", "weil") leaves it open, "Moment" or "wait a moment" holds the turn.
//!
//! The text wins where it says more than the tone ([`decide`]): a sentence ending on "and" waits, whatever
//! the model hears. Without the model the text decides, without either the pause of a second of
//! [`super::audio::Limits`] ends the sentence, as before. All of it only with Silero VAD: without it the
//! listener tells speech by loudness, and nothing here is asked.
//!
//! The features Smart Turn takes are those of Whisper's feature extractor ([`features`]): eight seconds,
//! 80 mel bands (Slaney), 100 columns a second, computed here with rustfft; the model runs on tract.

use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use sha2::{Digest, Sha256};
use torro_core::settings::Voice;
use torro_core::voice_models::SMART_TURN;
use torro_ipc::{VoiceMissing, VoicePart};
use tract_onnx::prelude::*;
use tract_onnx::tract_hir::infer::{Factoid, ShapeFactoid};

use super::audio::{FRAMES_PER_SECOND, RATE};
use super::wake::{after_wake_phrase, strip_wake_phrase};
use super::{TurnDetector, VoiceError};

/// The name the end of a sentence goes by in messages.
const NAME: &str = "end of a sentence";

/// What the end of a sentence so far says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The sentence is finished: answer after the short pause ([`Pauses::done`]).
    Done,
    /// Unfinished ("and then …", "um"): wait the longer pause ([`Pauses::open`]).
    Open,
    /// "Wait a moment": hold the turn until the person speaks again or [`Pauses::hold`] is over.
    Hold,
}

/// Phrases that hold the turn (decision 1), German and English, lower case, without punctuation. They
/// count at the end of the sentence: "Moment, wie spät ist es?" is a question.
const HOLDS: [&str; 18] = [
    "warte",
    "warte mal",
    "moment",
    "moment mal",
    "einen moment",
    "sekunde",
    "eine sekunde",
    "lass mich überlegen",
    "lass mich kurz überlegen",
    "kurz überlegen",
    "wait",
    "wait a moment",
    "hold on",
    "one moment",
    "one sec",
    "one second",
    "let me think",
    "give me a second",
];

/// Words that only fill a pause, German and English. Without "er": in German it is "he" ("Wo ist er?").
const FILLERS: [&str; 10] = ["äh", "ähm", "öh", "hm", "hmm", "ehm", "uh", "um", "uhm", "erm"];

/// Words after which a sentence goes on: conjunctions, prepositions, articles, German and English.
const CONNECTORS: [&str; 35] = [
    "und", "oder", "aber", "weil", "dass", "denn", "wenn", "also", "sondern", "bis", "für", "von", "der", "die", "den",
    "dem", "ein", "eine", "einen", "mein", "meine", "and", "or", "but", "because", "that", "if", "then", "with", "for",
    "to", "of", "the", "a", "my",
];

/// Connecting words that also end a sentence: the particle of a German verb ("Mach das Licht an.", "Komm mit!"),
/// "das", "dann" and "so" ("Mach das.", "Bis dann.", "I think so."), and the English article "an". Open only
/// without a full stop or exclamation mark after them.
const PARTICLES: [&str; 7] = ["an", "auf", "zu", "mit", "das", "dann", "so"];

/// The words of `text`, lower case: letters, digits and apostrophes.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `words` end with the words of `phrase`.
fn ends_with(words: &[String], phrase: &str) -> bool {
    let phrase: Vec<&str> = phrase.split(' ').collect();
    words.len() >= phrase.len() && words[words.len() - phrase.len()..].iter().zip(&phrase).all(|(a, b)| a == b)
}

/// The text of a sentence without the wake word in front: after it where it is found ([`after_wake_phrase`]),
/// else without one at the start.
fn without_wake(text: &str) -> String {
    after_wake_phrase(text).unwrap_or_else(|| strip_wake_phrase(text))
}

/// What the text of a sentence so far says of its end (decision 1): `None` for no text. A question mark at
/// the end finishes it; a hold phrase at the end holds the turn; an ellipsis, a comma, a filler or a connecting
/// word leaves it open; anything else is finished.
pub fn rules(text: &str) -> Option<Turn> {
    let text = without_wake(text);
    let trimmed = text.trim();
    let said = words(trimmed);
    if said.is_empty() {
        return None;
    }
    // "Was ist das?" ends on an article and "Hast du einen Moment?" on a hold, but both are asked: Whisper sets
    // the question mark only for a question.
    if trimmed.ends_with('?') {
        return Some(Turn::Done);
    }
    if HOLDS.iter().any(|hold| ends_with(&said, hold)) {
        return Some(Turn::Hold);
    }
    if trimmed.ends_with('…') || trimmed.ends_with("...") || trimmed.ends_with(',') {
        return Some(Turn::Open);
    }
    let ended = trimmed.ends_with('.') || trimmed.ends_with('!');
    let open = FILLERS.iter().chain(&CONNECTORS).any(|word| ends_with(&said, word))
        || (!ended && PARTICLES.iter().any(|word| ends_with(&said, word)));
    Some(if open { Turn::Open } else { Turn::Done })
}

/// Whether `text` holds nothing but fillers and a hold phrase ("Ähm, warte mal"): nothing to send when
/// the hold ended without the person going on.
pub fn only_hold(text: &str) -> bool {
    let said: Vec<String> = words(&without_wake(text)).into_iter().filter(|word| !FILLERS.contains(&&**word)).collect();
    said.is_empty() || HOLDS.iter().any(|hold| said.join(" ") == *hold)
}

/// The end of the sentence from the tone (`model`, the probability that it is finished) and the text
/// (`text`, [`rules`]): an open or held text wins, else the model decides, else the text; `None` with
/// neither, so the pause of [`super::audio::Limits`] ends it.
pub fn decide(model: Option<f32>, text: Option<Turn>) -> Option<Turn> {
    match (text, model) {
        (Some(Turn::Hold), _) => Some(Turn::Hold),
        (Some(Turn::Open), _) => Some(Turn::Open),
        (_, Some(finished)) if finished > 0.5 => Some(Turn::Done),
        (_, Some(_)) => Some(Turn::Open),
        (text, None) => text,
    }
}

/// The pauses of the end of a sentence in frames of the microphone, from the settings: rounded up to whole
/// frames of [`FRAMES_PER_SECOND`], counted as the pause of [`super::audio::Limits`] is (twelve frames a
/// second, each 80 ms long, so the pause heard is up to 4 % longer than chosen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pauses {
    /// After a finished sentence.
    pub done: usize,
    /// After an unfinished one.
    pub open: usize,
    /// How long "wait a moment" holds the turn.
    pub hold: usize,
}

impl Pauses {
    /// The pauses `voice` chose, or their defaults.
    pub fn of(voice: &Voice) -> Pauses {
        let frames = |ms: u64| usize::try_from((ms * FRAMES_PER_SECOND as u64).div_ceil(1000)).unwrap_or(usize::MAX);
        Pauses {
            done: frames(u64::from(voice.pause_done_or_default())),
            open: frames(u64::from(voice.pause_open_or_default())),
            hold: frames(u64::from(voice.hold_or_default()) * 1000),
        }
    }

    /// The pause `turn` asks for.
    pub fn of_turn(&self, turn: Turn) -> usize {
        match turn {
            Turn::Done => self.done,
            Turn::Open => self.open,
            Turn::Hold => self.hold,
        }
    }
}

/// How many seconds Smart Turn hears: the last eight of the sentence.
const SECONDS: usize = 8;

/// The samples Smart Turn hears.
const SAMPLES: usize = SECONDS * RATE as usize;

/// The length of a window of Whisper's features: 25 ms.
const N_FFT: usize = 400;

/// The step from one window to the next: 10 ms.
const HOP: usize = 160;

/// The mel bands.
pub const MELS: usize = 80;

/// The columns of the features: one per step, the last of Whisper's 801 left out.
pub const COLUMNS: usize = SAMPLES / HOP;

/// The frequency bins of a window.
const BINS: usize = N_FFT / 2 + 1;

/// What computes the features: the FFT, the window and the mel filters, made once.
pub struct Features {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// `MELS` rows of `BINS` weights.
    filters: Vec<Vec<f32>>,
}

impl Default for Features {
    fn default() -> Features {
        Features::new()
    }
}

/// Hertz on the mel scale of Slaney (as librosa and Whisper): linear up to 1 kHz, logarithmic above.
fn hz_to_mel(hz: f64) -> f64 {
    let (step, min_log_hz) = (200.0 / 3.0, 1000.0);
    if hz < min_log_hz { hz / step } else { min_log_hz / step + (hz / min_log_hz).ln() / (6.4f64.ln() / 27.0) }
}

/// [`hz_to_mel`] the other way.
fn mel_to_hz(mel: f64) -> f64 {
    let (step, min_log_hz) = (200.0 / 3.0, 1000.0);
    let min_log_mel = min_log_hz / step;
    if mel < min_log_mel { mel * step } else { min_log_hz * ((mel - min_log_mel) * (6.4f64.ln() / 27.0)).exp() }
}

/// The triangular mel filters of Whisper: `MELS` bands from 0 to 8 kHz, each normalised to its width (Slaney).
fn mel_filters() -> Vec<Vec<f32>> {
    let nyquist = f64::from(RATE) / 2.0;
    let (low, high) = (hz_to_mel(0.0), hz_to_mel(nyquist));
    let edges: Vec<f64> = (0..MELS + 2).map(|i| mel_to_hz(low + (high - low) * i as f64 / (MELS + 1) as f64)).collect();
    let bins: Vec<f64> = (0..BINS).map(|i| nyquist * i as f64 / (BINS - 1) as f64).collect();
    (0..MELS)
        .map(|m| {
            let (left, center, right) = (edges[m], edges[m + 1], edges[m + 2]);
            let norm = 2.0 / (right - left);
            bins.iter()
                .map(|&f| {
                    let down = (f - left) / (center - left);
                    let up = (right - f) / (right - center);
                    (down.min(up).max(0.0) * norm) as f32
                })
                .collect()
        })
        .collect()
}

impl Features {
    pub fn new() -> Features {
        let fft = FftPlanner::new().plan_fft_forward(N_FFT);
        // Periodic Hann, as Whisper takes it.
        let window =
            (0..N_FFT).map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / N_FFT as f32).cos()).collect();
        Features { fft, window, filters: mel_filters() }
    }

    /// Whisper's features of the last eight seconds of `samples` ([`RATE`], mono), as Smart Turn takes
    /// them: shorter sound filled with silence in front, normalised to mean 0 and variance 1, log-mel of
    /// [`MELS`] bands in [`COLUMNS`] columns, row by row (`[MELS][COLUMNS]`).
    pub fn of(&self, samples: &[i16]) -> Vec<f32> {
        let last = &samples[samples.len().saturating_sub(SAMPLES)..];
        let mut sound = vec![0.0f32; SAMPLES - last.len()];
        sound.extend(last.iter().map(|&s| f32::from(s) / 32768.0));
        let mean = sound.iter().map(|&s| f64::from(s)).sum::<f64>() / SAMPLES as f64;
        let variance = sound.iter().map(|&s| (f64::from(s) - mean).powi(2)).sum::<f64>() / SAMPLES as f64;
        let scale = 1.0 / (variance + 1e-7).sqrt();
        let sound: Vec<f32> = sound.iter().map(|&s| ((f64::from(s) - mean) * scale) as f32).collect();

        // Centred windows: the sound mirrored by half a window at both ends ("reflect").
        let half = N_FFT / 2;
        let at = |i: isize| -> f32 {
            let n = SAMPLES as isize;
            let i = if i < 0 {
                -i
            } else if i >= n {
                2 * (n - 1) - i
            } else {
                i
            };
            sound[i.clamp(0, n - 1) as usize]
        };
        let mut mel = vec![0.0f32; MELS * COLUMNS];
        let mut buffer = vec![Complex::new(0.0f32, 0.0); N_FFT];
        for column in 0..COLUMNS {
            let start = (column * HOP) as isize - half as isize;
            for (n, value) in buffer.iter_mut().enumerate() {
                *value = Complex::new(at(start + n as isize) * self.window[n], 0.0);
            }
            self.fft.process(&mut buffer);
            let power: Vec<f32> = buffer[..BINS].iter().map(Complex::norm_sqr).collect();
            for (band, filter) in self.filters.iter().enumerate() {
                let energy: f32 = filter.iter().zip(&power).map(|(w, p)| w * p).sum();
                mel[band * COLUMNS + column] = energy.max(1e-10).log10();
            }
        }
        let top = mel.iter().copied().fold(f32::MIN, f32::max);
        for value in &mut mel {
            *value = ((*value).max(top - 8.0) + 4.0) / 4.0;
        }
        mel
    }
}

/// Smart Turn v3, loaded once ([`SMART_TURN`]).
pub struct SmartTurn {
    model: TypedRunnableModel<TypedModel>,
    features: Features,
}

impl SmartTurn {
    /// Loads the model from the voice folder `folder`. One not downloaded is [`VoiceError::Missing`] of
    /// speech to text, without a try to load it; one whose file is not the catalogue's (damaged on disk) or
    /// that tract cannot load a [`VoiceError::Program`]. Checked against its SHA-256 first, as Silero VAD.
    pub fn load(folder: Option<&Path>) -> Result<SmartTurn, VoiceError> {
        let (folder, model) = match (folder, torro_core::voice_models::model(SMART_TURN)) {
            (Some(folder), Some(model)) if model.ready(folder) => (folder, model),
            _ => return Err(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model)),
        };
        let file = model.files.first().ok_or_else(|| VoiceError::program(NAME, "the catalogue names no file"))?;
        let bytes = std::fs::read(model.path(folder, file.name)).map_err(|err| {
            VoiceError::program(NAME, format!("the model {SMART_TURN} cannot be read: {}", err.kind()))
        })?;
        let hash: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        if hash != file.sha256 {
            return Err(VoiceError::program(NAME, format!("the model {SMART_TURN} is damaged: download it again")));
        }
        let refused =
            |err: TractError| VoiceError::program(NAME, format!("the model {SMART_TURN} did not load: {err:#}"));
        let model = tract_onnx::onnx()
            .with_ignore_output_shapes(true)
            .model_for_read(&mut bytes.as_slice())
            .and_then(open_shapes)
            .and_then(|model| model.with_input_fact(0, f32::fact([1, MELS, COLUMNS]).into()))
            .and_then(|model| model.into_optimized())
            .and_then(|model| model.into_runnable())
            .map_err(refused)?;
        Ok(SmartTurn { model, features: Features::new() })
    }
}

/// `model` with the shapes between its nodes left open: the file names its batch with a symbol there, which
/// tract cannot unify with the batch of one the listener gives, so tract works them out from the input.
fn open_shapes(mut model: InferenceModel) -> TractResult<InferenceModel> {
    let inputs = model.input_outlets()?.to_vec();
    let outlets: Vec<OutletId> = (0..model.nodes().len())
        .flat_map(|node| (0..model.nodes()[node].outputs.len()).map(move |slot| OutletId::new(node, slot)))
        .filter(|outlet| !inputs.contains(outlet))
        .collect();
    for outlet in outlets {
        let fact = model.outlet_fact(outlet)?.clone();
        // Constants keep their value; only shapes with a symbol are opened.
        let symbolic = fact.shape.dims().any(|dim| dim.concretize().is_some_and(|dim| dim.to_i64().is_err()));
        if fact.value.concretize().is_none() && symbolic {
            model.set_outlet_fact(outlet, InferenceFact { shape: ShapeFactoid::default(), ..fact })?;
        }
    }
    Ok(model)
}

impl TurnDetector for SmartTurn {
    fn complete(&mut self, samples: &[i16]) -> Result<f32, VoiceError> {
        let failed = |err: TractError| VoiceError::program(NAME, err);
        let features = self.features.of(samples);
        let input = Tensor::from_shape(&[1, MELS, COLUMNS], &features).map_err(failed)?;
        let output = self.model.run(tvec!(input.into())).map_err(failed)?;
        let first = output.first().ok_or_else(|| VoiceError::program(NAME, "the model gave nothing"))?;
        let view = first.to_array_view::<f32>().map_err(failed)?;
        let value = view.iter().next().copied().ok_or_else(|| VoiceError::program(NAME, "the model gave nothing"))?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::super::audio::tests::fixture;
    use super::*;

    /// Decision 1: the rules on German and English sentences, case by case.
    #[test]
    fn the_text_tells_finished_open_and_held_sentences() {
        let cases = [
            // Finished.
            ("Wie wird das Wetter morgen?", Some(Turn::Done)),
            ("What is the weather tomorrow?", Some(Turn::Done)),
            ("Mach das Licht an.", Some(Turn::Done)),
            ("Komm mit!", Some(Turn::Done)),
            ("Ich gehe morgen mit", Some(Turn::Open)),
            ("I'd like an", Some(Turn::Open)),
            ("Turn on the light", Some(Turn::Done)),
            ("Was ist das?", Some(Turn::Done)),
            ("Hey Torro, wie spät ist es", Some(Turn::Done)),
            ("Moment, wie spät ist es?", Some(Turn::Done)),
            ("Hast du einen Moment?", Some(Turn::Done)),
            ("Can you hold on?", Some(Turn::Done)),
            ("Mach das.", Some(Turn::Done)),
            ("Bis dann!", Some(Turn::Done)),
            ("I think so.", Some(Turn::Done)),
            ("Wo ist er", Some(Turn::Done)),
            ("Erst die Mails, dann", Some(Turn::Open)),
            // Open: a filler, a connecting word, an ellipsis or a comma at the end.
            ("Ich will morgen … ähm", Some(Turn::Open)),
            ("I want to go, um", Some(Turn::Open)),
            ("Erst die Mails und dann", Some(Turn::Open)),
            ("First the mail and then …", Some(Turn::Open)),
            ("Schreib Anna, dass", Some(Turn::Open)),
            ("Send it to the", Some(Turn::Open)),
            ("Ich brauche den Termin weil", Some(Turn::Open)),
            ("Ich will morgen...", Some(Turn::Open)),
            ("Erinnere mich,", Some(Turn::Open)),
            // Held.
            ("Warte mal.", Some(Turn::Hold)),
            ("Moment", Some(Turn::Hold)),
            ("Ähm, lass mich kurz überlegen", Some(Turn::Hold)),
            ("Hey Torro, einen Moment", Some(Turn::Hold)),
            ("Wait a moment.", Some(Turn::Hold)),
            ("Hold on", Some(Turn::Hold)),
            ("Let me think…", Some(Turn::Hold)),
            ("Give me a second", Some(Turn::Hold)),
            // No text.
            ("", None),
            ("  …  ", None),
            ("Hey Torro", None),
        ];
        for (text, turn) in cases {
            assert_eq!(rules(text), turn, "{text:?}");
        }
    }

    /// A sentence of nothing but fillers and a hold phrase is not sent; one with more is.
    #[test]
    fn only_a_hold_sends_nothing() {
        for held in ["Warte mal", "Ähm, Moment.", "Hey Torro, wait a moment", "um", ""] {
            assert!(only_hold(held), "{held:?}");
        }
        for said in ["Warte mal, wie spät ist es?", "Moment bitte", "Wait for me"] {
            assert!(!only_hold(said), "{said:?}");
        }
    }

    /// Decision 1: an open or held text wins over the tone, else the tone decides, else the text.
    #[test]
    fn the_text_wins_where_it_says_more_than_the_tone() {
        assert_eq!(decide(Some(0.9), Some(Turn::Hold)), Some(Turn::Hold));
        assert_eq!(decide(Some(0.9), Some(Turn::Open)), Some(Turn::Open));
        assert_eq!(decide(Some(0.9), Some(Turn::Done)), Some(Turn::Done));
        assert_eq!(decide(Some(0.2), Some(Turn::Done)), Some(Turn::Open), "the tone is unfinished");
        assert_eq!(decide(Some(0.7), None), Some(Turn::Done));
        assert_eq!(decide(Some(0.5), None), Some(Turn::Open));
        assert_eq!(decide(None, Some(Turn::Done)), Some(Turn::Done));
        assert_eq!(decide(None, Some(Turn::Open)), Some(Turn::Open));
        assert_eq!(decide(None, None), None);
    }

    /// The pauses come from the settings in frames, rounded up.
    #[test]
    fn the_pauses_are_frames_of_the_settings() {
        // Twelve frames a second.
        assert_eq!(Pauses::of(&Voice::default()), Pauses { done: 6, open: 24, hold: 180 });
        let chosen = Voice { pause_done: Some(800), pause_open: Some(1000), hold: Some(5), ..Voice::default() };
        assert_eq!(Pauses::of(&chosen), Pauses { done: 10, open: 12, hold: 60 });
        assert_eq!(Pauses::of(&chosen).of_turn(Turn::Hold), 60);
    }

    /// The features are 80 × 800, short sound is filled with silence in front, and a tone of 1 kHz is
    /// loudest in its mel band.
    #[test]
    fn the_features_are_whispers_log_mel_of_eight_seconds() {
        let features = Features::new();
        let tone: Vec<i16> = (0..RATE)
            .map(|n| ((2.0 * std::f32::consts::PI * 1000.0 * n as f32 / RATE as f32).sin() * 8000.0) as i16)
            .collect();
        let mel = features.of(&tone);
        assert_eq!(mel.len(), MELS * COLUMNS);
        let column = |at: usize| (0..MELS).map(|band| mel[band * COLUMNS + at]).collect::<Vec<_>>();
        // The last second holds the tone, the seven before are silence.
        let (quiet, loud) = (column(100), column(COLUMNS - 50));
        let band = loud.iter().enumerate().fold(0, |best, (i, v)| if *v > loud[best] { i } else { best });
        let expected = (hz_to_mel(1000.0) / hz_to_mel(8000.0) * (MELS + 1) as f64).round() as usize - 1;
        assert!(band.abs_diff(expected) <= 1, "loudest band {band}, expected about {expected}");
        assert_eq!(quiet, column(200), "the silence in front is the same throughout");
        assert!(loud[band] > quiet[band] + 1.0, "the tone stands out");
        let (low, high) = mel.iter().fold((f32::MAX, f32::MIN), |(low, high), v| (low.min(*v), high.max(*v)));
        assert!(low.is_finite() && high.is_finite() && high - low <= 2.0 + 1e-6, "eight decades, over four");
        // Longer sound: only its last eight seconds count.
        let mut long = vec![0i16; 3 * RATE as usize];
        long.extend(std::iter::repeat_n(0, SAMPLES - tone.len()));
        long.extend(&tone);
        assert_eq!(features.of(&long), mel);
    }

    /// The mel scale of Slaney goes there and back, linear below 1 kHz.
    #[test]
    fn the_mel_scale_goes_there_and_back() {
        assert!((hz_to_mel(1000.0) - 15.0).abs() < 1e-9);
        for hz in [0.0, 300.0, 1000.0, 4000.0, 8000.0] {
            assert!((mel_to_hz(hz_to_mel(hz)) - hz).abs() < 1e-6, "{hz}");
        }
        let filters = mel_filters();
        assert_eq!((filters.len(), filters[0].len()), (MELS, BINS));
        assert!(filters.iter().all(|filter| filter.iter().any(|w| *w > 0.0)), "no empty band");
    }

    /// Without the model nothing is loaded: the text decides alone.
    #[test]
    fn without_the_model_it_is_missing() {
        let folder = tempfile::tempdir().expect("voice folder");
        let missing = Err(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model));
        assert_eq!(SmartTurn::load(Some(folder.path())).map(|_| ()), missing);
        assert_eq!(SmartTurn::load(None).map(|_| ()), missing);
    }

    /// With the model (`TORRO_SMART_TURN=<voice folder>` holding `models/smart-turn/smart-turn-v3.2-gpu.onnx`):
    /// it loads and judges the sound files as Pipecat's own way does (Whisper's feature extractor of
    /// `transformers` and onnxruntime gave 0.143, 0.987 and 0.917), so the features here are Whisper's. The time
    /// a judgement takes is written out.
    #[test]
    #[ignore = "needs the Smart Turn model: TORRO_SMART_TURN=<voice folder>"]
    fn the_model_judges_a_sentence_as_pipecat_does() {
        let folder = std::env::var_os("TORRO_SMART_TURN").expect("TORRO_SMART_TURN");
        let started = std::time::Instant::now();
        let mut turn = SmartTurn::load(Some(Path::new(&folder))).expect("the model loads");
        eprintln!("loaded in {:?}", started.elapsed());
        for (name, expected) in [("utterance.wav", 0.143), ("hey_torro.wav", 0.987), ("silence.wav", 0.917)] {
            let samples = fixture(name);
            let started = std::time::Instant::now();
            let finished = turn.complete(&samples).expect("judged");
            eprintln!("{name}: {finished:.3} in {:?}", started.elapsed());
            assert!((finished - expected).abs() < 0.01, "{name}: {finished} instead of {expected}");
        }
    }
}
