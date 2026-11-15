//! Sound as the voice parts pass it on: 16 kHz, mono, 16-bit samples in frames of 80 ms, the voices of
//! Pocket TTS read from WAV files ([`decode_wav`]), the tone of the wake word ([`chime`]) and when a
//! spoken sentence ends ([`Endpoint`]).

use super::VoiceError;

/// Samples per second.
pub const RATE: u32 = 16_000;

/// Samples per frame: 80 ms, the steps in which the listener hears the microphone.
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

/// `sample` (16 bit) as a float between -1 and 1, the form sherpa-onnx and the sound devices take.
pub fn to_float(sample: i16) -> f32 {
    f32::from(sample) / 32768.0
}

/// `sample` (between -1 and 1, louder is clipped) as 16 bit.
pub fn to_i16(sample: f32) -> i16 {
    // The cast saturates as well, so nothing beyond the range wraps around.
    (sample.clamp(-1.0, 1.0) * 32767.0) as i16
}

/// Sound read from a WAV file ([`decode_wav`]): mono, between -1 and 1, at its own rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Wave {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// The format of a WAV file as its `fmt ` chunk names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Format {
    /// 1 for integers, 3 for floats; `WAVE_FORMAT_EXTENSIBLE` is read as its sub-format.
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
}

/// The format and the sound data of a WAV file; chunks other than `fmt ` and `data` are skipped.
fn wav_parts(bytes: &[u8]) -> Result<(Format, &[u8]), VoiceError> {
    let refused = |why: &str| VoiceError::Sound(format!("not a WAV file: {why}"));
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(refused("no RIFF/WAVE header"));
    }
    let mut at = 12;
    let mut format = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = bytes.get(at + 8..at + 8 + size).ok_or_else(|| refused("a chunk is cut off"))?;
        match id {
            b"fmt " if body.len() >= 16 => {
                let word = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
                let tag = match word(0) {
                    0xFFFE if body.len() >= 26 => word(24),
                    tag => tag,
                };
                let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                format = Some(Format { tag, channels: word(2), rate, bits: word(14) });
            }
            b"data" => return format.map(|format| (format, body)).ok_or_else(|| refused("no format before the data")),
            _ => {}
        }
        // Chunks are padded to an even length.
        at += 8 + size + size % 2;
    }
    Err(refused("no data"))
}

/// One sample of `format` from its bytes, between -1 and 1; `None` for a form this app does not read.
fn sample_of(format: Format, b: &[u8]) -> Option<f32> {
    match (format.tag, format.bits, b) {
        (1, 8, [b0, ..]) => Some((f32::from(*b0) - 128.0) / 128.0),
        (1, 16, [b0, b1, ..]) => Some(f32::from(i16::from_le_bytes([*b0, *b1])) / 32768.0),
        // The 24 bits go into the top of an i32, so the sign stays.
        (1, 24, [b0, b1, b2, ..]) => Some(i32::from_le_bytes([0, *b0, *b1, *b2]) as f32 / 2_147_483_648.0),
        (1, 32, [b0, b1, b2, b3, ..]) => Some(i32::from_le_bytes([*b0, *b1, *b2, *b3]) as f32 / 2_147_483_648.0),
        (3, 32, [b0, b1, b2, b3, ..]) => Some(f32::from_le_bytes([*b0, *b1, *b2, *b3])),
        (3, 64, [b0, b1, b2, b3, b4, b5, b6, b7, ..]) => {
            Some(f64::from_le_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7]) as f32)
        }
        _ => None,
    }
}

/// The sound of a WAV file in any common form (8, 16, 24 or 32-bit integers, 32 or 64-bit floats, any number of
/// channels), mixed down to mono: the voices Pocket TTS imitates come as their makers recorded them.
pub fn decode_wav(bytes: &[u8]) -> Result<Wave, VoiceError> {
    let (format, data) = wav_parts(bytes)?;
    let channels = usize::from(format.channels);
    let width = usize::from(format.bits / 8);
    if channels == 0 || format.rate == 0 || width == 0 || sample_of(format, &vec![0; width]).is_none() {
        return Err(VoiceError::Sound(format!(
            "not a WAV file this app reads: format {}, {} channels, {} bits",
            format.tag, format.channels, format.bits
        )));
    }
    let samples = data
        .chunks_exact(width * channels)
        .map(|frame| frame.chunks_exact(width).filter_map(|b| sample_of(format, b)).sum::<f32>() / channels as f32)
        .collect();
    Ok(Wave { rate: format.rate, samples })
}

/// `samples` as a WAV file: PCM, [`RATE`], mono, 16 bit. For the tests.
#[cfg(test)]
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

/// The samples of a WAV file in the form [`wav_bytes`] writes: PCM, [`RATE`], mono, 16 bit. For the sound files
/// of the tests.
#[cfg(test)]
pub fn read_wav(bytes: &[u8]) -> Result<Vec<i16>, VoiceError> {
    let (format, data) = wav_parts(bytes)?;
    if format != (Format { tag: 1, channels: 1, rate: RATE, bits: 16 }) {
        return Err(VoiceError::Sound("not a 16 kHz mono 16-bit WAV file".into()));
    }
    Ok(data.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
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

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use super::*;

    /// The samples of `tests/fixtures/<name>`.
    pub fn fixture(name: &str) -> Vec<i16> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        let bytes = std::fs::read(&path).expect("fixture");
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

    /// Issue #762: the voices come in other forms than the test sounds; each is mixed down to mono at its rate.
    #[test]
    fn wav_files_of_any_common_form_are_read_as_mono() {
        let samples = [0, 16_384, -16_384, i16::MAX];
        let mono = decode_wav(&wav_bytes(&samples)).expect("16 bit");
        assert_eq!(mono.rate, RATE);
        assert_eq!(mono.samples, samples.map(to_float));

        // Stereo 16 bit at 24 kHz: the two channels are averaged.
        let mut stereo = wav_bytes(&[16_384, 0, -16_384, -16_384]);
        stereo[22] = 2;
        stereo[24..28].copy_from_slice(&24_000u32.to_le_bytes());
        assert_eq!(decode_wav(&stereo).expect("stereo"), Wave { rate: 24_000, samples: vec![0.25, -0.5] });

        // 32-bit floats: the format tag 3, four bytes a sample.
        let mut float = wav_bytes(&[0, 0, 0, 0]);
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        float[34..36].copy_from_slice(&32u16.to_le_bytes());
        float[44..48].copy_from_slice(&0.5f32.to_le_bytes());
        float[48..52].copy_from_slice(&(-1.0f32).to_le_bytes());
        assert_eq!(decode_wav(&float).expect("float").samples, [0.5, -1.0]);

        let mut odd = wav_bytes(&[0, 0]);
        odd[34..36].copy_from_slice(&12u16.to_le_bytes());
        assert!(decode_wav(&odd).is_err(), "12 bit");
        assert!(decode_wav(b"not a wav").is_err());
    }

    #[test]
    fn samples_turn_into_floats_and_back() {
        assert_eq!(to_float(0), 0.0);
        assert_eq!(to_float(i16::MIN), -1.0);
        assert_eq!(to_i16(0.5), 16_383);
        assert_eq!(to_i16(2.0), i16::MAX, "clipped");
        assert_eq!(to_i16(-2.0), -i16::MAX);
        assert_eq!(to_i16(f32::NAN), 0);
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
}
