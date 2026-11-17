//! Echo cancellation for the listener (issue #834): the sound read aloud comes back into the microphone
//! from the speakers. The speech helper sends what it plays (`{"echo": …}`, [`super::speech`]); the core
//! passes it on to the listener's standard input, and it lands here as the reference ([`Echo::push`]).
//! [`EchoRecorder`] takes exactly that sound out of every microphone frame with WebRTC's echo canceller
//! (AEC3, ported to Rust in `sonora`), so the microphone may stay open while something is read aloud: the
//! assistant does not hear itself, but hears the person.
//!
//! The reference stays in memory, at most a second of it ([`KEPT`]); nothing touches the disk. Without a
//! reference for a while ([`IN_USE`]) a frame passes unchanged, and the listener closes the microphone while
//! something is read, as before ([`Echo::running`]). Should the canceller itself fail (it cannot with its
//! fixed format of 16 kHz mono in steps of 10 ms), the frame passes unchanged too, and the failure is logged.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

use super::audio::RATE;
use super::{Recorder, Stream, VoiceError};

/// How much of the reference waits at most: a second; the oldest falls out. More would come too late to
/// match the microphone anyway.
const KEPT: usize = RATE as usize;

/// How recent the last reference must be for the cancellation to count as running ([`Echo::running`]):
/// the helper sends it every few ten milliseconds while its output is open, silence included.
const RUNNING: Duration = Duration::from_millis(300);

/// How recent the last reference must be for a frame to go through the canceller: a little longer than
/// [`RUNNING`], so the room's echo after the last sound is taken out as well.
const IN_USE: Duration = Duration::from_secs(1);

/// The canceller works in steps of 10 ms.
const STEP: usize = RATE as usize / 100;

/// The reference as it arrives, and when it last came.
#[derive(Debug, Default)]
struct Reference {
    samples: VecDeque<i16>,
    last: Option<Instant>,
}

/// The sound read aloud, as the reference for the canceller (see the module). Clones share it: the
/// thread that reads the listener's input pushes, the microphone takes.
#[derive(Debug, Clone, Default)]
pub struct Echo(Arc<Mutex<Reference>>);

impl Echo {
    fn lock(&self) -> std::sync::MutexGuard<'_, Reference> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `samples` were played ([`RATE`], mono): they come back into the microphone soon.
    pub fn push(&self, samples: &[i16]) {
        let mut reference = self.lock();
        reference.samples.extend(samples);
        let over = reference.samples.len().saturating_sub(KEPT);
        reference.samples.drain(..over);
        reference.last = Some(Instant::now());
    }

    /// Whether the cancellation runs: a reference came within [`RUNNING`]. Only then the microphone stays
    /// open while something is read aloud.
    pub fn running(&self) -> bool {
        self.lock().last.is_some_and(|last| last.elapsed() <= RUNNING)
    }

    /// Whether a reference came since `at`: what the microphone heard since then went through the
    /// canceller.
    pub fn heard_since(&self, at: Instant) -> bool {
        self.lock().last.is_some_and(|last| last >= at)
    }

    /// The whole steps of the reference that wait, taken out (the rest stays for the next frame); `None`
    /// when none came within [`IN_USE`], and then what waits is dropped.
    fn take(&self) -> Option<Vec<i16>> {
        let mut reference = self.lock();
        if reference.last.is_none_or(|last| last.elapsed() > IN_USE) {
            reference.samples.clear();
            return None;
        }
        let whole = reference.samples.len() / STEP * STEP;
        Some(reference.samples.drain(..whole).collect())
    }
}

/// The canceller, kept across every opening of the microphone: what it learned of the room stays.
struct Canceller {
    processing: AudioProcessing,
    /// It failed once and said so; it is not logged again.
    failed: bool,
}

impl Canceller {
    fn new() -> Canceller {
        let config = Config { echo_canceller: Some(EchoCanceller::default()), ..Config::default() };
        let stream = StreamConfig::new(RATE, 1);
        let processing = AudioProcessing::builder().config(config).capture_config(stream).render_config(stream).build();
        Canceller { processing, failed: false }
    }

    /// `frame` without the echo of `reference`: the reference first, then the microphone, step by step.
    /// A failure gives the frame unchanged.
    fn cancel(&mut self, reference: &[i16], frame: Vec<i16>) -> Vec<i16> {
        match self.process(reference, &frame) {
            Ok(cleaned) => cleaned,
            Err(err) => {
                if !std::mem::replace(&mut self.failed, true) {
                    eprintln!("torroassistant: echo cancellation: {err}");
                }
                frame
            }
        }
    }

    fn process(&mut self, reference: &[i16], frame: &[i16]) -> Result<Vec<i16>, sonora::Error> {
        let mut played = [0; STEP];
        for step in reference.as_chunks::<STEP>().0 {
            self.processing.process_render_i16(step, &mut played)?;
        }
        let mut cleaned = vec![0; frame.len()];
        let (steps, rest) = frame.as_chunks::<STEP>();
        for (step, out) in steps.iter().zip(cleaned.as_chunks_mut::<STEP>().0) {
            self.processing.process_capture_i16(step, out)?;
        }
        // A frame is whole steps (80 ms); anything after the last step passes as it is.
        let done = steps.len() * STEP;
        cleaned[done..].copy_from_slice(rest);
        Ok(cleaned)
    }
}

/// A microphone with the echo of what is read aloud taken out (see the module).
pub struct EchoRecorder {
    recorder: Box<dyn Recorder>,
    echo: Echo,
    canceller: Rc<RefCell<Canceller>>,
}

impl EchoRecorder {
    /// `recorder` with the reference `echo`.
    pub fn new(recorder: Box<dyn Recorder>, echo: Echo) -> EchoRecorder {
        EchoRecorder { recorder, echo, canceller: Rc::new(RefCell::new(Canceller::new())) }
    }
}

impl Recorder for EchoRecorder {
    fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError> {
        let stream = self.recorder.open()?;
        // What played while the microphone was closed has no echo in what comes now: a heap of it at once
        // would make the canceller start over.
        self.echo.lock().samples.clear();
        Ok(Box::new(EchoStream { stream, echo: self.echo.clone(), canceller: Rc::clone(&self.canceller) }))
    }
}

/// An open microphone of the [`EchoRecorder`].
struct EchoStream {
    stream: Box<dyn Stream>,
    echo: Echo,
    canceller: Rc<RefCell<Canceller>>,
}

impl Stream for EchoStream {
    fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError> {
        let Some(frame) = self.stream.frame()? else { return Ok(None) };
        match self.echo.take() {
            Some(reference) => Ok(Some(self.canceller.borrow_mut().cancel(&reference, frame))),
            None => Ok(Some(frame)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::audio::tests::fixture;
    use super::super::audio::{Endpoint, FRAME, Limits, NoiseFloor, rms};
    use super::*;

    /// The room: the sound from the speakers comes back after 30, 60 and 90 ms, ever quieter.
    const ROOM: [(usize, f32); 3] = [(30, 1.0), (60, 0.4), (90, 0.2)];

    /// `played` as the microphone hears it in [`ROOM`], made `gain` times as loud.
    fn in_the_room(played: &[i16], gain: f32) -> Vec<i16> {
        let mut heard = vec![0.0f32; played.len()];
        for (delay_ms, factor) in ROOM {
            let delay = RATE as usize * delay_ms / 1000;
            for (i, sample) in played.iter().enumerate() {
                if let Some(at) = heard.get_mut(i + delay) {
                    *at += f32::from(*sample) * factor * gain;
                }
            }
        }
        heard.into_iter().map(|sample| sample.clamp(-32768.0, 32767.0) as i16).collect()
    }

    /// How much louder the echo is made, so its loudest frame has at least `rms`.
    fn gain_for(played: &[i16], wanted: f32) -> f32 {
        let loudest = in_the_room(played, 1.0).chunks(FRAME).map(rms).fold(0.0, f32::max);
        (wanted / loudest).max(1.0)
    }

    /// Frames from `samples` as a microphone gives them.
    struct Mic(VecDeque<Vec<i16>>);

    impl Stream for Mic {
        fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError> {
            Ok(self.0.pop_front())
        }
    }

    struct Fixed(Vec<i16>);

    impl Recorder for Fixed {
        fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError> {
            Ok(Box::new(Mic(self.0.chunks(FRAME).map(<[i16]>::to_vec).collect())))
        }
    }

    /// Plays `played` as the reference frame by frame, in time with the microphone that hears `heard`,
    /// and gives what comes out of the canceller.
    fn cancelled(played: &[i16], heard: Vec<i16>) -> Vec<Vec<i16>> {
        let echo = Echo::default();
        let mut recorder = EchoRecorder::new(Box::new(Fixed(heard)), echo.clone());
        let mut stream = recorder.open().expect("opens");
        let mut out = Vec::new();
        let mut played = played.chunks(FRAME);
        loop {
            // What plays now comes back with the room's delay: the reference arrives before its echo.
            echo.push(played.next().unwrap_or(&[0; FRAME]));
            let Some(frame) = stream.frame().expect("a frame") else { break };
            out.push(frame);
        }
        out
    }

    /// Whether [`Endpoint`] (the listener's default) hears speech in `frames`.
    fn speech_in(frames: &[Vec<i16>]) -> bool {
        let mut end = Endpoint::new(NoiseFloor::default(), Limits { most: usize::MAX, ..Limits::default() });
        for frame in frames {
            end.push(frame);
        }
        end.heard_speech()
    }

    /// Issue #834, done 3: "Hey Torro" read aloud and loud in the room is taken out: the listener hears no
    /// speech in the whole file.
    #[test]
    fn what_is_read_aloud_is_not_heard() {
        let mut played = fixture("hey_torro.wav");
        played.extend(vec![0; RATE as usize / 2]);
        let gain = gain_for(&played, 2000.0);
        let heard = in_the_room(&played, gain);
        let loudest = heard.chunks(FRAME).map(rms).fold(0.0, f32::max);
        assert!(loudest >= 2000.0, "a loud echo: {loudest}");
        let frames: Vec<Vec<i16>> = heard.chunks(FRAME).map(<[i16]>::to_vec).collect();
        assert!(speech_in(&frames), "without cancellation the echo counts as speech");
        let out = cancelled(&played, heard);
        let left = out.iter().map(|frame| rms(frame)).fold(0.0, f32::max);
        assert!(!speech_in(&out), "the echo is taken out: loudest frame left {left}");
    }

    /// Issue #834: the person speaking over what is read aloud is still heard.
    #[test]
    fn the_person_is_heard_over_what_is_read_aloud() {
        let mut played = fixture("hey_torro.wav");
        played.extend(vec![0; RATE as usize / 2]);
        let gain = gain_for(&played, 2000.0);
        let mut heard = in_the_room(&played, gain);
        let person = fixture("utterance.wav");
        for (at, sample) in heard.iter_mut().zip(person.iter()) {
            *at = at.saturating_add(*sample);
        }
        let out = cancelled(&played, heard);
        assert!(speech_in(&out), "the person's sentence stays");
    }

    /// Issue #834: without a reference the microphone's frames pass unchanged.
    #[test]
    fn without_a_reference_frames_pass_unchanged() {
        let heard = fixture("utterance.wav");
        let before = Instant::now();
        let echo = Echo::default();
        let mut recorder = EchoRecorder::new(Box::new(Fixed(heard.clone())), echo.clone());
        let mut stream = recorder.open().expect("opens");
        let mut out = Vec::new();
        while let Some(frame) = stream.frame().expect("a frame") {
            out.extend(frame);
        }
        assert_eq!(out, heard[..out.len()]);
        assert!(!echo.running() && !echo.heard_since(before));
    }

    /// The reference keeps a second at most, gives whole steps and counts as running only while it comes.
    #[test]
    fn the_reference_keeps_a_second_in_whole_steps() {
        let echo = Echo::default();
        let before = Instant::now();
        echo.push(&vec![1; KEPT + 500]);
        assert!(echo.running() && echo.heard_since(before));
        assert_eq!(echo.lock().samples.len(), KEPT, "the oldest fell out");
        echo.lock().samples.truncate(STEP * 2 + 7);
        assert_eq!(echo.take().map(|taken| taken.len()), Some(STEP * 2));
        assert_eq!(echo.lock().samples.len(), 7, "the rest waits");
        echo.lock().last = Instant::now().checked_sub(Duration::from_secs(2));
        assert!(!echo.running());
        assert_eq!(echo.take(), None);
        assert!(echo.lock().samples.is_empty(), "an old reference is dropped");
    }
}
