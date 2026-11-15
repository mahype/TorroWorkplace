//! The wake word "Hey Torro", heard in one of two ways:
//!
//! - [`ModelWake`] (issue #765): a small model of its own, trained for "Hey Torro" and built into the
//!   app. It scores every frame on the listener's beat and costs a fraction of speech recognition, so
//!   the recogniser only works on the sentence after the wake word.
//! - [`TranscriptWake`], when the built-in model does not load: it turns the last two seconds into
//!   text once a second, while something louder than the room was heard, and looks for "Hey Torro"
//!   ([`is_wake_phrase`]), with the recogniser of the sentences (decision 5 of #762). The checks run
//!   on a thread of their own, so the listener never falls behind the microphone, however slow the
//!   recogniser: Whisper turbo takes about a second for the window on a fast processor. A slower one
//!   checks less often – the next check waits for the last – and a wake word between two checks may be
//!   missed.
//!
//! [`strip_wake_phrase`] takes the wake word off the front of a sentence, for when it was recorded
//! with it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use livekit_wakeword::WakeWordModel;

use super::audio::{FRAMES_PER_SECOND, NoiseFloor, RATE, rms};
use super::{Transcriber, VoiceError, WakeWord};

/// The model "Hey Torro", trained with `livekit-wakeword train` (`docs/assistent.md`, "Weckwort-Modell
/// trainieren").
const MODEL: &[u8] = include_bytes!("../../assets/wake/hey_torro.onnx");

/// Its file name; `livekit-wakeword` names the score after the file stem, `hey_torro`.
const MODEL_FILE: &str = "hey_torro.onnx";

/// The score from which the wake word counts: the trainer calibrates its models to 0.5.
const THRESHOLD: f32 = 0.5;

/// The wake word heard by the built-in model (see the module). It runs on the listener's beat, without a
/// thread of its own and without a gate on loudness: the model keeps a state across frames and needs every
/// one of them.
pub struct ModelWake {
    model: WakeWordModel,
}

impl ModelWake {
    /// Loads the built-in model. `livekit-wakeword` only loads from a file, so the model is written to
    /// `<voice>/wake/hey_torro.onnx` first, when the file is missing or holds anything else (a copy cut off
    /// or damaged, the model of an older version); it goes to a `.part` file of this process first, so a crash
    /// never leaves half a model behind and two listeners starting at once never write into one file. Fails
    /// when the folder cannot be written or the model does not load.
    pub fn load(voice: &Path) -> Result<ModelWake, VoiceError> {
        let path = write_model(voice).map_err(|err| VoiceError::program("wake word", err))?;
        let model = WakeWordModel::new(&[path], RATE).map_err(|err| VoiceError::program("wake word", err))?;
        Ok(ModelWake { model })
    }
}

/// Writes [`MODEL`] into `<voice>/wake/` unless it is there byte for byte; where it lies. The whole file is
/// compared, not its size: a newer model of the same architecture has the same size.
fn write_model(voice: &Path) -> std::io::Result<PathBuf> {
    let folder = voice.join("wake");
    let path = folder.join(MODEL_FILE);
    if is_model(&path) {
        return Ok(path);
    }
    std::fs::create_dir_all(&folder)?;
    let part = folder.join(format!("{MODEL_FILE}.{}.part", std::process::id()));
    let written = std::fs::write(&part, MODEL).and_then(|()| std::fs::rename(&part, &path));
    if let Err(err) = written {
        // No half model stays behind; the name holds the process id, so the next start would not overwrite it.
        let _ = std::fs::remove_file(&part);
        // Another listener may have put the model there meanwhile and hold it open (Windows refuses the
        // rename then).
        if !is_model(&path) {
            return Err(err);
        }
    }
    Ok(path)
}

/// Whether `path` is the built-in model byte for byte.
fn is_model(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == MODEL.len() as u64)
        && std::fs::read(path).is_ok_and(|bytes| bytes == MODEL)
}

impl WakeWord for ModelWake {
    fn hears(&mut self, frame: &[i16]) -> Result<bool, VoiceError> {
        let scores = self.model.predict(frame).map_err(|err| VoiceError::program("wake word", err))?;
        let stem = MODEL_FILE.trim_end_matches(".onnx");
        if scores.get(stem).is_some_and(|score| *score >= THRESHOLD) {
            // The next wake word starts afresh, not on the tail of this one.
            self.reset();
            return Ok(true);
        }
        Ok(false)
    }

    fn reset(&mut self) {
        if let Err(err) = self.model.reset() {
            eprintln!("torroassistant: the wake word could not start afresh: {err}");
        }
    }
}

/// How much is kept and turned into text: two seconds, a little more than "Hey Torro" takes.
const WINDOW_FRAMES: usize = 2 * FRAMES_PER_SECOND;

/// How often the window is turned into text: once a second.
const CHECK_EVERY: usize = FRAMES_PER_SECOND;

/// Words heard for "hey"; speech to text writes the greeting in many ways.
const HEY: &[&str] = &["hey", "hei", "hay", "hej", "he", "hi", "a"];

/// Words heard for "Torro", which no dictionary knows.
const TORRO: &[&str] = &["torro", "toro", "torre", "torru", "turo", "taro", "torah", "torrow", "tauro", "tarro"];

/// Whether `text` holds the wake word: "hey" and "Torro" next to each other in one of the ways
/// speech to text writes them, in any case and with any punctuation.
pub fn is_wake_phrase(text: &str) -> bool {
    let words = words(text);
    words.iter().any(|word| word == "heytorro" || word == "heytoro")
        || words.windows(2).any(|pair| HEY.contains(&pair[0].as_str()) && TORRO.contains(&pair[1].as_str()))
}

/// `text` without the wake word in front ("Hey Torro, what is on today?" → "what is on today?").
pub fn strip_wake_phrase(text: &str) -> String {
    let trimmed = text.trim();
    let mut rest = trimmed;
    // At most the first three words can be the wake word ("Hey, Torro," or "Heytorro").
    for _ in 0..3 {
        let Some(head) = rest.split_whitespace().next() else { break };
        let at = rest.find(head).unwrap_or(0) + head.len();
        let word = words(head).concat();
        if HEY.contains(&word.as_str()) || TORRO.contains(&word.as_str()) || word == "heytorro" {
            rest = &rest[at..];
        } else {
            break;
        }
    }
    if rest.len() == trimmed.len() || !is_wake_phrase(&trimmed[..trimmed.len() - rest.len()]) {
        return trimmed.to_string();
    }
    rest.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation()).trim().to_string()
}

/// The words of `text` in lower case, punctuation dropped.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// What the thread of the checks gets: a window and the round it belongs to.
type Job = (u64, Vec<i16>);

/// What it answers: the round, and whether the window held the wake word.
type Heard = (u64, Result<bool, VoiceError>);

/// The wake word found with speech to text (see the module). The checks run on a thread of their own:
/// the listener only hands the newest window over while that thread is free, and reads its answer at a
/// later frame, so the microphone is read on time however long a check takes.
pub struct TranscriptWake {
    jobs: Sender<Job>,
    answers: Receiver<Heard>,
    window: VecDeque<i16>,
    since: usize,
    floor: NoiseFloor,
    loud: bool,
    /// A check runs on the thread.
    busy: bool,
    /// The round: [`WakeWord::reset`] starts a new one, and a late answer of an older one is dropped.
    round: u64,
}

impl TranscriptWake {
    pub fn new(mut transcriber: Box<dyn Transcriber + Send>) -> TranscriptWake {
        let (jobs, work) = mpsc::channel::<Job>();
        let (tell, answers) = mpsc::channel::<Heard>();
        // Ends when the wake word is dropped: its sender goes, and the loop with it.
        std::thread::spawn(move || {
            for (round, samples) in work {
                let heard = transcriber.transcribe(&samples).map(|text| is_wake_phrase(&text));
                if tell.send((round, heard)).is_err() {
                    return;
                }
            }
        });
        TranscriptWake {
            jobs,
            answers,
            window: VecDeque::new(),
            since: 0,
            floor: NoiseFloor::default(),
            loud: false,
            busy: false,
            round: 0,
        }
    }

    /// The answers of the thread that came: whether the wake word was heard in this round.
    fn answered(&mut self) -> Result<bool, VoiceError> {
        let mut heard = false;
        while let Ok((round, answer)) = self.answers.try_recv() {
            self.busy = false;
            if round == self.round {
                heard |= answer?;
            }
        }
        Ok(heard)
    }
}

impl WakeWord for TranscriptWake {
    fn hears(&mut self, frame: &[i16]) -> Result<bool, VoiceError> {
        if self.answered()? {
            self.window.clear();
            self.since = 0;
            return Ok(true);
        }
        let loudness = rms(frame);
        // Only windows with something louder than the room are turned into text: silence costs
        // nothing.
        self.loud |= loudness >= self.floor.speech();
        self.floor.follow(loudness);
        self.window.extend(frame);
        let most = WINDOW_FRAMES * frame.len().max(1);
        while self.window.len() > most {
            self.window.pop_front();
        }
        self.since += 1;
        // While a check still runs, the next one waits: what is said meanwhile stays in the window.
        if self.since < CHECK_EVERY || self.busy {
            return Ok(false);
        }
        self.since = 0;
        if !std::mem::take(&mut self.loud) {
            return Ok(false);
        }
        let samples: Vec<i16> = self.window.iter().copied().collect();
        if self.jobs.send((self.round, samples)).is_err() {
            return Err(VoiceError::program("wake word", "the checks stopped"));
        }
        self.busy = true;
        Ok(false)
    }

    fn reset(&mut self) {
        self.window.clear();
        self.since = 0;
        self.loud = false;
        self.round += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::audio::FRAME;
    use super::super::audio::tests::fixture;
    use super::*;

    /// Answers with `text` and counts how often it was asked.
    struct Says {
        text: &'static str,
        calls: Arc<Mutex<usize>>,
    }

    impl Transcriber for Says {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            assert!(samples.len() <= WINDOW_FRAMES * FRAME, "at most the window");
            *self.calls.lock().expect("calls") += 1;
            Ok(self.text.to_string())
        }
    }

    /// Feeds `name` to a wake word whose speech to text hears `text`; whether it woke, and how
    /// often it asked for text.
    fn wakes_on(name: &str, text: &'static str) -> (bool, usize) {
        let calls = Arc::new(Mutex::new(0));
        let mut wake = TranscriptWake::new(Box::new(Says { text, calls: Arc::clone(&calls) }));
        let mut woke = false;
        // Twice through, so the window is checked after the sound as well; each frame waits a little,
        // like the microphone, so the thread of the checks answers in between.
        for frame in fixture(name).chunks(FRAME).chain(fixture(name).chunks(FRAME)) {
            woke |= wake.hears(frame).expect("hears");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // The last answer may still be on its way.
        let quiet = vec![0; FRAME];
        for _ in 0..50 {
            woke |= wake.hears(&quiet).expect("hears");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let calls = *calls.lock().expect("calls");
        (woke, calls)
    }

    #[test]
    fn the_wake_word_is_recognized_in_speech() {
        let (woke, calls) = wakes_on("utterance.wav", "Hey Torro.");
        assert!(woke);
        assert!(calls >= 1);
    }

    #[test]
    fn other_speech_does_not_wake() {
        let (woke, calls) = wakes_on("utterance.wav", "Hello there, how are you?");
        assert!(!woke);
        assert!(calls >= 1, "it was listened to");
    }

    #[test]
    fn silence_is_never_turned_into_text() {
        assert_eq!(wakes_on("silence.wav", "Hey Torro"), (false, 0));
    }

    /// Review of #762: a recogniser slower than the microphone never holds it up – while it checks, the
    /// frames go on and no second check is asked.
    #[test]
    fn a_slow_recogniser_does_not_hold_the_microphone_up() {
        struct Slow(Arc<Mutex<usize>>);
        impl Transcriber for Slow {
            fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
                *self.0.lock().expect("calls") += 1;
                std::thread::sleep(std::time::Duration::from_millis(400));
                Ok("Hey Torro".into())
            }
        }
        let calls = Arc::new(Mutex::new(0));
        let mut wake = TranscriptWake::new(Box::new(Slow(Arc::clone(&calls))));
        let loud = fixture("utterance.wav");
        let started = std::time::Instant::now();
        let mut woke = false;
        for frame in loud.chunks(FRAME).take(4 * FRAMES_PER_SECOND) {
            woke |= wake.hears(frame).expect("hears");
        }
        assert!(started.elapsed() < std::time::Duration::from_millis(300), "the frames were not held up");
        assert_eq!(*calls.lock().expect("calls"), 1, "one check at a time");
        let quiet = vec![0; FRAME];
        for _ in 0..100 {
            woke |= wake.hears(&quiet).expect("hears");
            if woke {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(woke, "the answer came at a later frame");
        wake.reset();
    }

    /// Issue #765: the built-in model is written into the voice folder and loads; silence never wakes it.
    #[test]
    fn the_built_in_model_loads_and_silence_does_not_wake_it() {
        let voice = tempfile::tempdir().expect("voice folder");
        let mut wake = ModelWake::load(voice.path()).expect("loads");
        let written = std::fs::metadata(voice.path().join("wake").join(MODEL_FILE)).expect("written");
        assert_eq!(written.len(), MODEL.len() as u64);
        let silence = fixture("silence.wav");
        for frame in silence.chunks(FRAME).cycle().take(5 * FRAMES_PER_SECOND) {
            assert!(!wake.hears(frame).expect("hears"), "silence");
        }
        wake.reset();
    }

    /// Issue #765: the built-in model hears "Hey Torro" in a clip of the trainer's held-out test set (a TTS
    /// voice, not trained on), with the name of its score and the threshold of the app; it wakes once, then
    /// starts afresh.
    #[test]
    fn the_built_in_model_hears_hey_torro() {
        let voice = tempfile::tempdir().expect("voice folder");
        let mut wake = ModelWake::load(voice.path()).expect("loads");
        let woke = fixture("hey_torro.wav").chunks(FRAME).filter(|frame| wake.hears(frame).expect("hears")).count();
        assert_eq!(woke, 1, "woken once");
    }

    /// Issue #765: a model file cut off, or another model of the same size, is written anew.
    #[test]
    fn a_model_file_cut_off_or_changed_is_written_anew() {
        let voice = tempfile::tempdir().expect("voice folder");
        let folder = voice.path().join("wake");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join(MODEL_FILE), &MODEL[..1024]).expect("cut off");
        ModelWake::load(voice.path()).expect("loads");
        assert!(std::fs::read(folder.join(MODEL_FILE)).expect("model") == MODEL);
        let mut older = MODEL.to_vec();
        if let Some(last) = older.last_mut() {
            *last ^= 0xff;
        }
        std::fs::write(folder.join(MODEL_FILE), &older).expect("same size");
        ModelWake::load(voice.path()).expect("loads");
        assert!(std::fs::read(folder.join(MODEL_FILE)).expect("model") == MODEL, "the built-in model");
        let left: Vec<_> = std::fs::read_dir(&folder).expect("folder").collect();
        assert_eq!(left.len(), 1, "no part left");
    }

    /// Issue #765: a model that cannot be put in place leaves no part file behind.
    #[test]
    fn a_failed_write_leaves_no_part_behind() {
        let voice = tempfile::tempdir().expect("voice folder");
        let folder = voice.path().join("wake");
        std::fs::create_dir_all(folder.join(MODEL_FILE)).expect("a folder where the model goes");
        assert!(ModelWake::load(voice.path()).is_err());
        let left: Vec<_> = std::fs::read_dir(&folder).expect("folder").collect();
        assert_eq!(left.len(), 1, "only the folder in the way");
    }

    /// Issue #765: a voice folder that is a file gives an error, no panic.
    #[test]
    fn a_voice_folder_that_is_a_file_fails() {
        let file = tempfile::NamedTempFile::new().expect("file");
        let err = ModelWake::load(file.path()).err().expect("fails");
        assert!(matches!(err, VoiceError::Program { ref program, .. } if program == "wake word"), "{err:?}");
    }

    #[test]
    fn the_wake_phrase_is_found_in_many_spellings() {
        for text in ["Hey Torro!", "hey, toro", " Hey Torro, what's on today?", "Heytorro", "Hi Torre."] {
            assert!(is_wake_phrase(text), "{text:?}");
        }
        for text in ["Hey there", "Toro y Moi", "Torro hey", "", "They torrow"] {
            assert!(!is_wake_phrase(text), "{text:?}");
        }
    }

    #[test]
    fn the_wake_phrase_is_taken_off_the_sentence() {
        assert_eq!(strip_wake_phrase("Hey Torro, what is on today?"), "what is on today?");
        assert_eq!(strip_wake_phrase(" Hey, Toro. Search for Anna's mail."), "Search for Anna's mail.");
        assert_eq!(strip_wake_phrase("What is on today?"), "What is on today?");
        assert_eq!(strip_wake_phrase("A tomato salad recipe"), "A tomato salad recipe", "not the wake word");
        assert_eq!(strip_wake_phrase("Hey Torro"), "");
    }
}
