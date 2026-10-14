//! The wake word without openWakeWord: [`TranscriptWake`] turns the last two seconds into text
//! once a second, while something louder than the room was heard, and looks for "Hey Torro"
//! ([`is_wake_phrase`]). It costs more computing time than a wake word model, which is why
//! openWakeWord comes first when it is installed ([`super::external::OpenWakeWord`]).
//!
//! [`strip_wake_phrase`] takes the wake word off the front of a sentence, for when it was recorded
//! with it.

use std::collections::VecDeque;

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

/// The wake word found with speech to text (see the module).
pub struct TranscriptWake {
    transcriber: Box<dyn Transcriber>,
    window: VecDeque<i16>,
    since: usize,
    floor: NoiseFloor,
    loud: bool,
}

impl TranscriptWake {
    pub fn new(transcriber: Box<dyn Transcriber>) -> TranscriptWake {
        TranscriptWake { transcriber, window: VecDeque::new(), since: 0, floor: NoiseFloor::default(), loud: false }
    }
}

impl WakeWord for TranscriptWake {
    fn hears(&mut self, frame: &[i16]) -> Result<bool, VoiceError> {
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
        if self.since < CHECK_EVERY {
            return Ok(false);
        }
        self.since = 0;
        if !std::mem::take(&mut self.loud) {
            return Ok(false);
        }
        let samples: Vec<i16> = self.window.iter().copied().collect();
        let heard = is_wake_phrase(&self.transcriber.transcribe(&samples)?);
        if heard {
            self.window.clear();
        }
        Ok(heard)
    }

    fn reset(&mut self) {
        self.window.clear();
        self.since = 0;
        self.loud = false;
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
        // Twice through, so the window is checked after the sound as well.
        for frame in fixture(name).chunks(FRAME).chain(fixture(name).chunks(FRAME)) {
            woke |= wake.hears(frame).expect("hears");
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
