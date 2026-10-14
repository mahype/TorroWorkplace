//! Sound as the voice parts pass it on: 16 kHz, mono, 16-bit samples in frames of 80 ms (the
//! size openWakeWord takes), WAV files for whisper.cpp and the players, and when a spoken sentence
//! ends ([`Endpoint`]).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::VoiceError;

/// Samples per second.
pub const RATE: u32 = 16_000;

/// Samples per frame: 80 ms, what openWakeWord takes at once.
pub const FRAME: usize = 1280;

/// Frames per second.
pub const FRAMES_PER_SECOND: usize = RATE as usize / FRAME;

/// Below this loudness (root mean square) nothing counts as speech, however quiet the room.
pub const MIN_SPEECH_RMS: f32 = 300.0;

/// The loudness of `samples`: their root mean square, 0 for none.
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// How loud the room is, followed slowly while the listener waits for the wake word, so speech
/// can be told from noise in a quiet room and in a loud one ([`NoiseFloor::speech`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoiseFloor(f32);

impl Default for NoiseFloor {
    fn default() -> NoiseFloor {
        NoiseFloor(MIN_SPEECH_RMS / 3.0)
    }
}

impl NoiseFloor {
    /// Takes the loudness of one more frame. Quiet frames move it quickly, loud ones slowly, so a
    /// sentence does not raise it much.
    pub fn follow(&mut self, frame_rms: f32) {
        let weight = if frame_rms < self.0 { 0.2 } else { 0.02 };
        self.0 += (frame_rms - self.0) * weight;
    }

    /// The loudness from which a frame counts as speech: three times the room, at least
    /// [`MIN_SPEECH_RMS`].
    pub fn speech(self) -> f32 {
        (self.0 * 3.0).max(MIN_SPEECH_RMS)
    }
}

/// How long a sentence may be, in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Nothing said this long after the tone: the recording ends empty.
    pub wait: usize,
    /// A pause this long after speech ends the sentence.
    pub pause: usize,
    /// The longest sentence.
    pub most: usize,
}

impl Default for Limits {
    /// Five seconds to start, a pause of one second, 15 seconds at most.
    fn default() -> Limits {
        Limits { wait: 5 * FRAMES_PER_SECOND, pause: FRAMES_PER_SECOND, most: 15 * FRAMES_PER_SECOND }
    }
}

/// Decides when a spoken sentence ends: after a pause once something was said, when nothing was
/// said for [`Limits::wait`], or at [`Limits::most`].
#[derive(Debug, Clone)]
pub struct Endpoint {
    speech: f32,
    limits: Limits,
    frames: usize,
    spoken: usize,
    quiet: usize,
}

impl Endpoint {
    /// A sentence in a room as loud as `floor`.
    pub fn new(floor: NoiseFloor, limits: Limits) -> Endpoint {
        Endpoint { speech: floor.speech(), limits, frames: 0, spoken: 0, quiet: 0 }
    }

    /// Takes the next frame; `true` when the sentence is over (this frame included).
    pub fn push(&mut self, frame: &[i16]) -> bool {
        self.frames += 1;
        if rms(frame) >= self.speech {
            self.spoken += 1;
            self.quiet = 0;
        } else {
            self.quiet += 1;
        }
        let paused = self.spoken > 0 && self.quiet >= self.limits.pause;
        let never_spoke = self.spoken == 0 && self.frames >= self.limits.wait;
        paused || never_spoke || self.frames >= self.limits.most
    }

    /// Whether anything louder than the room was heard.
    pub fn heard_speech(&self) -> bool {
        self.spoken > 0
    }
}

/// `samples` as a WAV file: PCM, [`RATE`], mono, 16 bit.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// The samples of a WAV file in the form [`wav_bytes`] writes: PCM, [`RATE`], mono, 16 bit.
/// Chunks other than `fmt ` and `data` are skipped. For the sound files of the tests.
#[cfg(test)]
pub fn read_wav(bytes: &[u8]) -> Result<Vec<i16>, VoiceError> {
    let refused = |why: &str| VoiceError::Sound(format!("not a 16 kHz mono 16-bit WAV file: {why}"));
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(refused("no RIFF/WAVE header"));
    }
    let mut at = 12;
    let mut format_ok = false;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or_else(|| refused("a chunk is cut off"))?;
        match id {
            b"fmt " if body.len() >= 16 => {
                let word = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
                let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                format_ok = word(0) == 1 && word(2) == 1 && rate == RATE && word(14) == 16;
                if !format_ok {
                    return Err(refused("another format"));
                }
            }
            b"data" if format_ok => {
                return Ok(body.chunks_exact(2).map(|pair| i16::from_le_bytes([pair[0], pair[1]])).collect());
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        at += 8 + size + size % 2;
    }
    Err(refused("no data"))
}

/// The tone when the wake word was heard: two short notes going up, 180 ms, soft at both ends.
pub fn chime() -> Vec<i16> {
    let notes = [(660.0_f32, 0.09_f32), (880.0, 0.09)];
    let mut samples = Vec::new();
    for (pitch, seconds) in notes {
        let count = (RATE as f32 * seconds) as usize;
        for i in 0..count {
            let t = i as f32 / RATE as f32;
            // A short fade in and out keeps it from clicking.
            let fade = (i.min(count - i) as f32 / (RATE as f32 * 0.01)).min(1.0);
            let value = (2.0 * std::f32::consts::PI * pitch * t).sin() * 6000.0 * fade;
            samples.push(value as i16);
        }
    }
    samples
}

/// A WAV file in the temporary folder that only this user may read, removed when dropped: what
/// whisper.cpp and the players read. Never kept: speech does not stay on disk.
#[derive(Debug)]
pub struct TempWav {
    path: PathBuf,
}

impl TempWav {
    /// Writes `samples` to a new file.
    pub fn write(samples: &[i16]) -> Result<TempWav, VoiceError> {
        TempWav::with_bytes(&wav_bytes(samples))
    }

    /// A new file with the WAV `bytes`.
    pub fn with_bytes(bytes: &[u8]) -> Result<TempWav, VoiceError> {
        let path = TempWav::new_path();
        write_private(&path, bytes)
            .map_err(|err| VoiceError::Sound(format!("cannot write {}: {err}", path.display())))?;
        Ok(TempWav { path })
    }

    /// A name for an empty file that a program writes (Piper's output), removed when dropped.
    pub fn empty() -> TempWav {
        TempWav { path: TempWav::new_path() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `torro-voice-<pid>-<n>.wav`: unique within the process and among processes.
    fn new_path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("torro-voice-{}-{n}.wav", std::process::id()))
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Writes `bytes` to a new file `path` that only this user may read (0600 on Unix; the temporary
/// folder of the user profile on Windows is the user's alone).
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// The samples of `tests/fixtures/<name>`.
    pub fn fixture(name: &str) -> Vec<i16> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        let bytes = fs::read(&path).expect("fixture");
        read_wav(&bytes).expect("a 16 kHz mono WAV")
    }

    #[test]
    fn wav_files_round_trip_and_others_are_refused() {
        let samples = vec![0, 1, -1, i16::MAX, i16::MIN, 1234];
        assert_eq!(read_wav(&wav_bytes(&samples)), Ok(samples));
        assert!(read_wav(b"RIFF....WAVE").is_err(), "no data");
        assert!(read_wav(b"not a wav").is_err());
        let mut stereo = wav_bytes(&[0, 0]);
        stereo[22] = 2;
        assert!(read_wav(&stereo).is_err(), "stereo");
        assert_eq!(fixture("silence.wav").len(), RATE as usize, "one second");
    }

    #[test]
    fn a_sentence_ends_at_a_pause_and_silence_ends_empty() {
        let frames = |name: &str| fixture(name).chunks(FRAME).map(<[i16]>::to_vec).collect::<Vec<_>>();
        let mut floor = NoiseFloor::default();
        for frame in frames("silence.wav") {
            floor.follow(rms(&frame));
        }
        assert!(floor.speech() >= MIN_SPEECH_RMS);

        let mut sentence = Endpoint::new(floor, Limits::default());
        let ended = frames("utterance.wav").iter().position(|frame| sentence.push(frame));
        assert!(sentence.heard_speech());
        // 0.25 s noise, 0.5 s sound, then a pause of a second: over after about 1.75 s.
        let ended = ended.expect("the pause ends it");
        assert!((18..=23).contains(&ended), "ended at frame {ended}");

        let short = Limits { wait: 5, ..Limits::default() };
        let mut quiet = Endpoint::new(floor, short);
        let ended = frames("silence.wav").iter().position(|frame| quiet.push(frame));
        assert_eq!(ended, Some(4), "nothing said within the wait");
        assert!(!quiet.heard_speech());

        let mut long = Endpoint::new(floor, Limits { most: 3, ..Limits::default() });
        assert_eq!(frames("utterance.wav").iter().position(|frame| long.push(frame)), Some(2), "the longest");
    }

    #[test]
    fn temporary_files_are_private_and_go_away() {
        let file = TempWav::write(&chime()).expect("written");
        let path = file.path().to_path_buf();
        assert_eq!(read_wav(&fs::read(&path).expect("there")).map(|s| s.len()), Ok(chime().len()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(file);
        assert!(!path.exists());
        assert_ne!(TempWav::empty().path(), TempWav::empty().path(), "unique names");
    }
}
