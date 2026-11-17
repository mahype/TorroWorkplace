//! The system's sound devices through cpal (issue #762): the microphone ([`CpalRecorder`]), the sound
//! output ([`Player`]) and the tone of the wake word ([`ChimeSpeaker`]). Which devices there are goes
//! through the trait [`Devices`], so the status is tested without touching real ones
//! ([`SystemDevices`] asks cpal).
//!
//! A cpal stream is not `Send` on every system, so it stays on the thread that made it: the
//! recorder's stream lives in the [`Stream`] it returns, the player's in the [`Player`]. Their
//! callbacks only move samples; turning them into 16 kHz or into the device's rate happens outside,
//! with sherpa-onnx's [`LinearResampler`].
//!
//! On Linux the microphone speaks the PulseAudio protocol, which PipeWire serves (`pipewire-pulse`): the
//! names are those of the volume control, and a device another program uses can be chosen too. Without
//! that socket cpal takes ALSA. Reading aloud and the tone stay on ALSA.
//!
//! A microphone is named as the system names it ([`name_of`]). A stored name cpal does not know (the
//! `pactl` or ALSA names of earlier versions, a device unplugged) falls back to the system's default, and
//! so does one that cannot be opened (with ALSA, a card PipeWire holds).

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use sherpa_onnx::LinearResampler;
use torro_ipc::{VoiceMissing, VoicePart};

use super::audio::{FRAME, RATE, chime, to_float, to_i16};
use super::{Recorder, Speaker, Stream, VoiceError};

/// The sound devices the system offers. [`SystemDevices`] asks cpal; tests bring their own.
pub trait Devices {
    /// The microphones by name, in the system's order, each name once; the system's default is not
    /// among them unless it is a device of its own.
    fn microphones(&self) -> Vec<String>;

    /// The name of the system's default microphone; `None` when there is no microphone at all.
    fn default_microphone(&self) -> Option<String>;

    /// Whether the system has a sound output to play on.
    fn has_output(&self) -> bool;

    /// Whether the microphone `name` can be opened now: a card PipeWire or another program holds may be
    /// listed and still refuse (review of #762).
    fn opens(&self, name: &str) -> bool;
}

/// The microphone recorded from with the setting `wanted`: that one when the system knows it and it
/// opens ([`CpalRecorder`] falls back the same way), else the default, else the first one there is;
/// `None` without any microphone.
pub fn microphone_in_use(devices: &dyn Devices, wanted: Option<&str>) -> Option<String> {
    let known = devices.microphones();
    if let Some(name) = wanted
        && known.iter().any(|known| known == name)
        && devices.opens(name)
    {
        return Some(name.to_string());
    }
    devices.default_microphone().or_else(|| known.into_iter().next())
}

/// The devices as cpal finds them on this system.
pub struct SystemDevices;

impl Devices for SystemDevices {
    fn microphones(&self) -> Vec<String> {
        offered_inputs(&cpal::default_host()).into_iter().map(|(name, _)| name).collect()
    }

    fn default_microphone(&self) -> Option<String> {
        cpal::default_host().default_input_device().map(|device| name_of(&device))
    }

    fn has_output(&self) -> bool {
        output_host().default_output_device().is_some()
    }

    fn opens(&self, name: &str) -> bool {
        // Building the stream opens the device; it is dropped at once, before anything is recorded.
        offered_inputs(&cpal::default_host())
            .into_iter()
            .find(|(known, _)| known == name)
            .is_some_and(|(_, device)| probe_input(&device))
    }
}

/// Whether `device` gives a stream to record from; the stream is built, never started.
fn probe_input(device: &cpal::Device) -> bool {
    let Ok(config) = device.default_input_config() else { return false };
    device
        .build_input_stream_raw(
            config.config(),
            config.sample_format(),
            |_: &cpal::Data, _: &cpal::InputCallbackInfo| {},
            |_| {},
            None,
        )
        .is_ok()
}

/// The name people see for `device`; its id when it has no description.
pub fn name_of(device: &cpal::Device) -> String {
    // Not `to_string()`: its `Display` fails (and `to_string` panics) for a device that went away.
    match device.description() {
        Ok(description) => description.name().to_string(),
        Err(_) => device.id().map(|id| id.id().to_string()).unwrap_or_default(),
    }
}

/// The microphones people choose from, with their names, each name once ([`offered`]).
fn offered_inputs(host: &cpal::Host) -> Vec<(String, cpal::Device)> {
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    let mut found: Vec<(String, cpal::Device)> = Vec::new();
    for device in devices {
        let offer = device.id().is_ok_and(|id| offered(&id));
        let name = name_of(&device);
        if offer && !name.is_empty() && found.iter().all(|(known, _)| *known != name) {
            found.push((name, device));
        }
    }
    found
}

/// Whether the device `id` is one people choose. ALSA lists every plugin and each card many times
/// (`hw:`, `plughw:`, `front:`, `dsnoop:` …); the one per card that shares it with other programs is
/// `sysdefault:CARD=…`. PulseAudio also lists the monitor of each output, which is no microphone. The
/// other systems list each microphone once.
fn offered(id: &cpal::DeviceId) -> bool {
    #[cfg(target_os = "linux")]
    match id.host() {
        cpal::HostId::Alsa => id.id().starts_with("sysdefault:CARD="),
        cpal::HostId::PulseAudio => !id.id().ends_with(".monitor"),
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = id;
        true
    }
}

/// The host to play sound on. On Linux ALSA (the microphone alone moves to PulseAudio, issue #772), else
/// the system's default.
fn output_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::Alsa) {
        return host;
    }
    cpal::default_host()
}

/// A cpal error as a [`VoiceError::Sound`].
fn sound(err: impl std::fmt::Display) -> VoiceError {
    VoiceError::Sound(err.to_string())
}

/// `samples` of interleaved `channels` mixed down to mono floats.
fn mono<T>(samples: &[T], channels: usize) -> Vec<f32>
where
    T: Sample,
    f32: FromSample<T>,
{
    samples
        .chunks(channels.max(1))
        .map(|frame| frame.iter().map(|&sample| f32::from_sample(sample)).sum::<f32>() / frame.len() as f32)
        .collect()
}

/// What a device recorded, as mono floats; `None` for a sample format this app does not take.
fn mono_of(data: &cpal::Data, channels: usize) -> Option<Vec<f32>> {
    match data.sample_format() {
        SampleFormat::F32 => data.as_slice::<f32>().map(|samples| mono(samples, channels)),
        SampleFormat::F64 => data.as_slice::<f64>().map(|samples| mono(samples, channels)),
        SampleFormat::I16 => data.as_slice::<i16>().map(|samples| mono(samples, channels)),
        SampleFormat::I32 => data.as_slice::<i32>().map(|samples| mono(samples, channels)),
        SampleFormat::I8 => data.as_slice::<i8>().map(|samples| mono(samples, channels)),
        SampleFormat::U8 => data.as_slice::<u8>().map(|samples| mono(samples, channels)),
        SampleFormat::U16 => data.as_slice::<u16>().map(|samples| mono(samples, channels)),
        SampleFormat::U32 => data.as_slice::<u32>().map(|samples| mono(samples, channels)),
        _ => None,
    }
}

/// Gathers sound at [`RATE`] and hands it out in frames of [`FRAME`] samples.
#[derive(Debug, Default)]
struct Framer {
    samples: VecDeque<i16>,
}

impl Framer {
    fn push(&mut self, samples: &[f32]) {
        self.samples.extend(samples.iter().map(|&sample| to_i16(sample)));
    }

    /// The next whole frame, once there is one.
    fn take(&mut self) -> Option<Vec<i16>> {
        (self.samples.len() >= FRAME).then(|| self.samples.drain(..FRAME).collect())
    }
}

/// How long the microphone may send nothing before recording counts as failed.
const SILENT_MOST: Duration = Duration::from_secs(5);

/// The microphone named in the setting `microphone` (see the module), or the system's default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpalRecorder {
    pub microphone: Option<String>,
}

impl Recorder for CpalRecorder {
    fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError> {
        let host = cpal::default_host();
        // The devices are listed only for a named microphone: the listener opens it after every answer.
        if let Some(wanted) = self.microphone.as_deref()
            && let Some((name, device)) = offered_inputs(&host).into_iter().find(|(name, _)| name == wanted)
        {
            match open_input(&device) {
                Ok(stream) => return Ok(Box::new(stream)),
                Err(err) => eprintln!("torroassistant: {name} cannot record ({err}), the default does"),
            }
        }
        let device = host
            .default_input_device()
            .or_else(|| offered_inputs(&host).into_iter().next().map(|(_, device)| device))
            .ok_or(VoiceError::Missing(VoicePart::Recorder, VoiceMissing::Program))?;
        Ok(Box::new(open_input(&device)?))
    }
}

/// Starts recording from `device` in its own format; the samples come as mono floats over a channel.
fn open_input(device: &cpal::Device) -> Result<CpalStream, VoiceError> {
    let config = device.default_input_config().map_err(sound)?;
    let channels = usize::from(config.channels());
    let rate = config.sample_rate();
    let resampler = match rate {
        RATE => None,
        // The rates are a few ten thousand: they fit an i32.
        rate => Some(
            LinearResampler::create(rate as i32, RATE as i32)
                .ok_or_else(|| VoiceError::Sound(format!("cannot resample {rate} Hz")))?,
        ),
    };
    let (sender, captured) = mpsc::channel();
    let stream = device
        .build_input_stream_raw(
            config.config(),
            config.sample_format(),
            move |data: &cpal::Data, _: &cpal::InputCallbackInfo| {
                if let Some(samples) = mono_of(data, channels) {
                    // Gone only when the stream is dropped as well.
                    let _ = sender.send(samples);
                }
            },
            // An overrun or a short glitch: the stream goes on, a microphone that stops shows by its silence.
            |err| eprintln!("torroassistant: microphone: {err}"),
            None,
        )
        .map_err(sound)?;
    stream.play().map_err(sound)?;
    Ok(CpalStream { _stream: stream, captured, resampler, framer: Framer::default() })
}

/// An open microphone. Dropping it stops recording.
struct CpalStream {
    _stream: cpal::Stream,
    captured: Receiver<Vec<f32>>,
    resampler: Option<LinearResampler>,
    framer: Framer,
}

impl Stream for CpalStream {
    fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError> {
        loop {
            if let Some(frame) = self.framer.take() {
                return Ok(Some(frame));
            }
            match self.captured.recv_timeout(SILENT_MOST) {
                Ok(samples) => match &self.resampler {
                    Some(resampler) => self.framer.push(&resampler.resample(&samples, false)),
                    None => self.framer.push(&samples),
                },
                Err(RecvTimeoutError::Timeout) => {
                    return Err(VoiceError::Sound("the microphone sends no sound".into()));
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }
}

/// Where sound that is read aloud goes: the system's output ([`Player`]), a fake in tests. Dropping it
/// stops at once.
pub trait Sound {
    /// Plays `samples` (mono, between -1 and 1, at `rate`) after what came before, as soon as it can.
    fn push(&mut self, rate: u32, samples: &[f32]);

    /// No more sound comes: what is there plays to its end.
    fn finish(&mut self);

    /// Whether everything has played after [`Sound::finish`].
    fn ended(&self) -> bool;

    /// Whether everything pushed so far has played and the device has played silence since (issue #850):
    /// the sound waits for more, unlike [`Sound::ended`] also before [`Sound::finish`].
    fn drained(&self) -> bool;
}

/// How long a sound output that just opened plays silence before the first sound (issue #829): a device
/// that only starts swallows the beginning of what it gets, and with it the first syllable.
const LEAD_IN_MS: usize = 150;

/// The sound waiting for the device, and whether it has played out.
#[derive(Debug, Default)]
struct Queue {
    samples: VecDeque<f32>,
    /// No more sound comes ([`Sound::finish`]).
    finished: bool,
    /// Whole buffers of silence given to the device since the queue ran empty after the end: the last
    /// sound has left the device's own buffer once there were two.
    drained: u8,
    /// Whole buffers of silence given to the device since the last push (issue #850), finished or not:
    /// two mean the sound has run out and waits for more.
    idle: u8,
}

impl Queue {
    /// The queue of an output at `rate` that just opened: [`LEAD_IN_MS`] of silence before what comes.
    fn opened(rate: u32) -> Queue {
        let lead_in = usize::try_from(rate).unwrap_or(0).saturating_mul(LEAD_IN_MS) / 1000;
        Queue { samples: std::iter::repeat_n(0.0, lead_in).collect(), ..Queue::default() }
    }

    fn ended(&self) -> bool {
        self.finished && self.samples.is_empty() && self.drained >= 2
    }

    /// `samples` play after what is queued; the silence before them no longer counts.
    fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        self.samples.extend(samples);
        self.idle = 0;
    }

    fn drained(&self) -> bool {
        self.samples.is_empty() && self.idle >= 2
    }
}

/// Fills a device buffer of `channels` interleaved from `queue`, the same sample on every channel, and
/// silence once the queue is empty.
fn fill<T>(out: &mut [T], channels: usize, queue: &mut Queue)
where
    T: Sample + FromSample<f32>,
{
    let had_sound = !queue.samples.is_empty();
    for frame in out.chunks_mut(channels.max(1)) {
        let sample = queue.samples.pop_front().unwrap_or(0.0);
        frame.fill(T::from_sample(sample));
    }
    if !had_sound {
        queue.idle = queue.idle.saturating_add(1);
        if queue.finished {
            queue.drained = queue.drained.saturating_add(1);
        }
    }
}

/// Fills a device buffer in its own sample format.
fn fill_data(data: &mut cpal::Data, channels: usize, queue: &mut Queue) {
    match data.sample_format() {
        SampleFormat::F32 => fill_typed::<f32>(data, channels, queue),
        SampleFormat::F64 => fill_typed::<f64>(data, channels, queue),
        SampleFormat::I16 => fill_typed::<i16>(data, channels, queue),
        SampleFormat::I32 => fill_typed::<i32>(data, channels, queue),
        SampleFormat::I8 => fill_typed::<i8>(data, channels, queue),
        SampleFormat::U8 => fill_typed::<u8>(data, channels, queue),
        SampleFormat::U16 => fill_typed::<u16>(data, channels, queue),
        SampleFormat::U32 => fill_typed::<u32>(data, channels, queue),
        // A format this app does not write: the device keeps its silence, and the sound counts as played –
        // its samples are dropped, so the queue ends and nothing waits for it (review of #762).
        _ => {
            queue.samples.clear();
            queue.drained = u8::MAX;
            queue.idle = u8::MAX;
        }
    }
}

fn fill_typed<T>(data: &mut cpal::Data, channels: usize, queue: &mut Queue)
where
    T: SizedSample + FromSample<f32>,
{
    if let Some(out) = data.as_slice_mut::<T>() {
        fill(out, channels, queue);
    }
}

/// The system's sound output while something plays: sound at any rate goes in piece by piece, is
/// resampled to the device's rate and plays as it comes.
pub struct Player {
    _stream: cpal::Stream,
    queue: Arc<Mutex<Queue>>,
    rate: u32,
    /// The resampler for the rate of the sound that comes, made with its first piece.
    resampler: Option<(u32, LinearResampler)>,
}

impl Player {
    /// Opens the default output; without one [`VoiceError::Missing`] with [`VoiceMissing::Player`].
    pub fn open() -> Result<Player, VoiceError> {
        let device = output_host()
            .default_output_device()
            .ok_or(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Player))?;
        let config = device.default_output_config().map_err(sound)?;
        let channels = usize::from(config.channels());
        let queue = Arc::new(Mutex::new(Queue::opened(config.sample_rate())));
        let shared = Arc::clone(&queue);
        let stream = device
            .build_output_stream_raw(
                config.config(),
                config.sample_format(),
                move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
                    let mut queue = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    fill_data(data, channels, &mut queue);
                },
                |err| eprintln!("torroassistant: sound output: {err}"),
                None,
            )
            .map_err(sound)?;
        stream.play().map_err(sound)?;
        Ok(Player { _stream: stream, queue, rate: config.sample_rate(), resampler: None })
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Sound for Player {
    fn push(&mut self, rate: u32, samples: &[f32]) {
        if rate == self.rate {
            self.queue().push(samples.iter().copied());
            return;
        }
        if self.resampler.as_ref().is_none_or(|(from, _)| *from != rate) {
            self.resampler = LinearResampler::create(rate as i32, self.rate as i32).map(|resampler| (rate, resampler));
        }
        let Some((_, resampler)) = &self.resampler else {
            eprintln!("torroassistant: cannot resample {rate} Hz");
            return;
        };
        let resampled = resampler.resample(samples, false);
        self.queue().push(resampled);
    }

    fn finish(&mut self) {
        let rest = self.resampler.as_ref().map(|(_, resampler)| resampler.resample(&[], true)).unwrap_or_default();
        let mut queue = self.queue();
        queue.samples.extend(rest);
        queue.finished = true;
    }

    fn ended(&self) -> bool {
        self.queue().ended()
    }

    fn drained(&self) -> bool {
        self.queue().drained()
    }
}

/// Plays `samples` (mono at `rate`) on the system's output and returns once they have played, after
/// `most` at the latest.
pub fn play(rate: u32, samples: &[f32], most: Duration) -> Result<(), VoiceError> {
    let mut player = Player::open()?;
    player.push(rate, samples);
    player.finish();
    let started = Instant::now();
    while !player.ended() && started.elapsed() < most {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// The listener's own speaker: it only plays the tone of the wake word. Answers are read aloud by the
/// core (`assistant.speak_answer`, issue #747).
pub struct ChimeSpeaker;

impl Speaker for ChimeSpeaker {
    fn chime(&mut self) {
        let samples: Vec<f32> = chime().into_iter().map(to_float).collect();
        if let Err(err) = play(RATE, &samples, Duration::from_secs(2)) {
            eprintln!("torroassistant: no tone: {err}");
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Devices as a test sets them.
    #[derive(Debug, Clone, Default)]
    pub struct FakeDevices {
        pub microphones: Vec<String>,
        pub default: Option<String>,
        pub output: bool,
        /// Microphones that are listed but refuse to open.
        pub held: Vec<String>,
    }

    impl Devices for FakeDevices {
        fn microphones(&self) -> Vec<String> {
            self.microphones.clone()
        }

        fn default_microphone(&self) -> Option<String> {
            self.default.clone()
        }

        fn has_output(&self) -> bool {
            self.output
        }

        fn opens(&self, name: &str) -> bool {
            !self.held.contains(&name.to_string())
        }
    }

    /// Issue #762: a stored microphone cpal does not know (an old `pactl` name) gives the default.
    #[test]
    fn the_microphone_in_use_is_the_named_one_else_the_default() {
        let devices = FakeDevices {
            microphones: vec!["Elgato Wave XLR, USB Audio".into(), "HD Pro Webcam C920, USB Audio".into()],
            default: Some("Default Audio Device".into()),
            output: true,
            held: Vec::new(),
        };
        let in_use = |wanted: Option<&str>| microphone_in_use(&devices, wanted);
        assert_eq!(in_use(Some("HD Pro Webcam C920, USB Audio")).as_deref(), Some("HD Pro Webcam C920, USB Audio"));
        assert_eq!(
            in_use(Some("alsa_input.usb-Rode_NT-USB-00.analog-stereo")).as_deref(),
            Some("Default Audio Device")
        );
        // Issue #772: a card name from the ALSA days is no PulseAudio name; the default takes over.
        let pulse = FakeDevices { microphones: vec!["Elgato Wave XLR Mono".into()], ..devices.clone() };
        assert_eq!(
            microphone_in_use(&pulse, Some("Elgato Wave XLR, USB Audio")).as_deref(),
            Some("Default Audio Device")
        );
        assert_eq!(in_use(None).as_deref(), Some("Default Audio Device"));
        let no_default = FakeDevices { default: None, ..devices.clone() };
        assert_eq!(microphone_in_use(&no_default, None).as_deref(), Some("Elgato Wave XLR, USB Audio"));
        assert_eq!(microphone_in_use(&FakeDevices::default(), Some("USB")), None);
        // Review of #762: one that is listed but held by another program is not the one in use.
        let held = FakeDevices { held: vec!["HD Pro Webcam C920, USB Audio".into()], ..devices.clone() };
        assert_eq!(
            microphone_in_use(&held, Some("HD Pro Webcam C920, USB Audio")).as_deref(),
            Some("Default Audio Device")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn alsa_offers_one_device_per_card() {
        let alsa = |id: &str| cpal::DeviceId::new(cpal::HostId::Alsa, id);
        assert!(offered(&alsa("sysdefault:CARD=XLR")));
        for id in ["default", "null", "pipewire", "hw:CARD=0,DEV=0", "plughw:CARD=0,DEV=0", "front:CARD=XLR,DEV=0"] {
            assert!(!offered(&alsa(id)), "{id}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pulse_offers_sources_without_monitors() {
        let pulse = |id: &str| cpal::DeviceId::new(cpal::HostId::PulseAudio, id);
        assert!(offered(&pulse("alsa_input.usb-Elgato_Wave_XLR-00.mono-fallback")));
        assert!(offered(&pulse("bluez_input.80_C3_BA_12_34_56.headset-head-unit")));
        assert!(!offered(&pulse("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor")));
    }

    #[test]
    fn recorded_sound_becomes_mono_frames() {
        assert_eq!(mono(&[0.5f32, -0.5, 1.0, 0.0], 2), [0.0, 0.5]);
        assert_eq!(mono(&[i16::MIN, 0], 1), [-1.0, 0.0]);
        let mut framer = Framer::default();
        framer.push(&vec![0.5; FRAME - 1]);
        assert_eq!(framer.take(), None, "not a whole frame yet");
        framer.push(&[0.5, -0.5]);
        let frame = framer.take().expect("a frame");
        assert_eq!(frame.len(), FRAME);
        assert_eq!(frame[0], to_i16(0.5));
        assert_eq!(framer.samples, [to_i16(-0.5)], "the rest waits for the next frame");
    }

    /// Issue #850: the sound has run out once two buffers of silence went out since the last push, before
    /// its end too; a push makes it wait again.
    #[test]
    fn the_output_says_when_it_ran_out() {
        let mut queue = Queue::default();
        queue.push([0.5, 0.5]);
        let mut out = [0i16; 2];
        fill(&mut out, 1, &mut queue);
        assert!(!queue.drained(), "it just played");
        fill(&mut out, 1, &mut queue);
        assert!(!queue.drained(), "one buffer of silence");
        fill(&mut out, 1, &mut queue);
        assert!(queue.drained() && !queue.ended(), "ran out, but more may come");
        queue.push([0.25]);
        assert!(!queue.drained(), "more came");
    }

    /// The device gets the same sample on each channel, silence when nothing is queued, and the sound
    /// has ended once two buffers of silence went out after the end.
    #[test]
    fn the_output_plays_the_queue_and_then_ends() {
        let mut queue = Queue::default();
        queue.samples.extend([0.5, -0.5, 0.25]);
        let mut out = [0i16; 4];
        fill(&mut out, 2, &mut queue);
        assert_eq!(out, [16_384, 16_384, -16_384, -16_384]);
        let mut float = [1.0f32; 4];
        fill(&mut float, 2, &mut queue);
        assert_eq!(float, [0.25, 0.25, 0.0, 0.0], "silence after the queue");
        assert!(!queue.ended(), "more may come");
        queue.finished = true;
        fill(&mut out, 2, &mut queue);
        assert!(!queue.ended(), "one buffer of silence");
        fill(&mut out, 2, &mut queue);
        assert!(queue.ended());
    }

    /// Issue #829: an output that just opened plays a short silence first, so the device's start does
    /// not swallow the first syllable.
    #[test]
    fn an_opened_output_begins_with_silence() {
        let mut queue = Queue::opened(48_000);
        assert_eq!(queue.samples.len(), 7_200, "150 ms at 48 kHz");
        queue.samples.push_back(0.5);
        let mut lead_in = vec![1.0f32; 7_200];
        fill(&mut lead_in, 1, &mut queue);
        assert!(lead_in.iter().all(|sample| *sample == 0.0));
        let mut sound = [0.0f32; 1];
        fill(&mut sound, 1, &mut queue);
        assert_eq!(sound, [0.5], "the sound follows the silence");
    }
}
