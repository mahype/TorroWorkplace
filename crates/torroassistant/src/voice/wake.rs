//! The wake word: [`TranscriptWake`] turns the last two seconds into text once a second, while
//! something louder than the room was heard, and looks for "Hey Torro" ([`is_wake_phrase`]). It uses
//! the recogniser of the sentences (decision 5 of #762), so no model of its own has to be downloaded;
//! silence costs nothing. The checks run on a thread of their own, so the listener never falls behind
//! the microphone, however slow the recogniser: Whisper turbo takes about a second for the window on a
//! fast processor. A slower one checks less often – the next check waits for the last – and a wake
//! word between two checks may be missed; an own wake word model is #765.
//!
//! [`strip_wake_phrase`] takes the wake word off the front of a sentence, for when it was recorded
//! with it.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

use super::audio::{FRAMES_PER_SECOND, NoiseFloor, rms};
use super::{Transcriber, VoiceError, WakeWord};

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
