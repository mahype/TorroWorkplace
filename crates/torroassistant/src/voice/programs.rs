//! Which local programs the listener can use, found at runtime ([`Found::detect`]) – nothing is
//! downloaded or built in. The control centre shows the result (`settings.voice_status`, built
//! by [`Found::status`]) with a hint how to install what is missing.
//!
//! Programs are looked for on `PATH` ([`Finder`]); models and voices in the settings' paths or in
//! the voice folder `<data>/torroassistant/voice/` ([`voice_folder`]): `ggml-*.bin` for
//! whisper.cpp, the folder [`PARAKEET_MODEL`] for Parakeet, `*.onnx` with its `*.onnx.json` for
//! Piper, `hey_torro.onnx` (or `.tflite`) for openWakeWord. Only the speech to text the settings
//! choose is looked for ([`Recognizer`]).
//!
//! Parakeet runs in a Python that imports `onnx-asr`. Systems with a protected Python (Arch,
//! Debian) refuse a plain `pip install`, so the Python of a `pipx` or `uv tool` install is found
//! too: it is named in the first line of their program `onnx-asr` ([`script_python`]).
//!
//! Reading aloud (issue #745, [`super::speech`]) also needs Qwen3-TTS: the Python package
//! `faster-qwen3-tts` in the folder's own environment `<voice>/qwen/venv` ([`qwen_folder`]), set up
//! with [`qwen_setup`], or in a Python on `PATH`. Sound is best played raw from a pipe ([`RawPlayer`]),
//! so the first words play while the rest is computed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::settings::{Recognizer, Voice};
use torro_ipc::{VoiceMissing, VoicePart, VoiceProgram};

/// The openWakeWord model of the fixed wake word (decision 11), without its extension.
pub const WAKE_MODEL: &str = "hey_torro";

/// The folder of the Parakeet model in the voice folder (Hugging Face:
/// `istupakov/parakeet-tdt-0.6b-v3-onnx`).
pub const PARAKEET_MODEL: &str = "parakeet-tdt-0.6b-v3";

/// The folder where models and voices are looked for when the settings name none; `None` without
/// a home directory.
pub fn voice_folder() -> Option<PathBuf> {
    AppPaths::for_app(AppId::ASSISTANT).map(|paths| paths.data.join("voice"))
}

/// The folder of Qwen3-TTS in the voice folder: its Python environment `venv` and the voice
/// templates (`warm_woman.wav`).
pub fn qwen_folder(folder: &Path) -> PathBuf {
    folder.join("qwen")
}

/// The Python of the environment in `qwen` ([`qwen_folder`]).
pub fn qwen_python(qwen: &Path) -> PathBuf {
    if cfg!(windows) {
        qwen.join("venv").join("Scripts").join("python.exe")
    } else {
        qwen.join("venv").join("bin").join("python")
    }
}

/// The versions that run together (14 November 2026): `faster-qwen3-tts` 0.5.4 did not load with
/// `transformers` 5.23.0.
pub const QWEN_PACKAGES: &str = "faster-qwen3-tts==0.5.4 transformers==5.15.1";

/// The command that sets up Qwen3-TTS in `qwen` ([`qwen_folder`]), for people to copy: a Python
/// environment of its own with [`QWEN_PACKAGES`]. The models load at the first answer.
pub fn qwen_setup(qwen: &Path) -> String {
    let venv = qwen.join("venv");
    let quoted = |path: &Path| format!("\"{}\"", path.display());
    let (python, pip) = if cfg!(windows) {
        ("python", venv.join("Scripts").join("pip.exe"))
    } else {
        ("python3", venv.join("bin").join("pip"))
    };
    format!("{python} -m venv {} && {} install {QWEN_PACKAGES}", quoted(&venv), quoted(&pip))
}

/// Where programs and files are looked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Finder {
    /// The folders of `PATH`, in order.
    pub dirs: Vec<PathBuf>,
    /// The voice folder ([`voice_folder`]).
    pub folder: Option<PathBuf>,
}

impl Finder {
    /// `PATH` and the user's voice folder.
    pub fn system() -> Finder {
        let dirs = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        Finder { dirs, folder: voice_folder() }
    }

    /// The first of `names` found in one of the folders (`.exe` added on Windows), names first:
    /// `["whisper-cli", "whisper-cpp"]` prefers `whisper-cli` anywhere on `PATH`.
    pub fn program(&self, names: &[&str]) -> Option<PathBuf> {
        names.iter().find_map(|name| {
            let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
            self.dirs.iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
        })
    }

    /// The file the setting names when it exists, else the first file in the voice folder (by
    /// name) that `fits`.
    fn file(&self, setting: Option<&str>, fits: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        if let Some(path) = setting {
            return Some(PathBuf::from(path)).filter(|path| path.is_file());
        }
        let folder = self.folder.as_deref()?;
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_file() && fits(path))
            .collect();
        files.sort();
        files.into_iter().next()
    }
}

/// The program that records ([`Found::recorder`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderKind {
    /// PulseAudio's and PipeWire's `parec` (Linux).
    Parec,
    /// ALSA's `arecord` (Linux).
    Arecord,
    /// SoX, every system.
    Sox,
    /// `ffmpeg`, every system; on Windows only with a microphone named.
    Ffmpeg,
}

impl RecorderKind {
    /// The kinds to try on this system, best first.
    fn order() -> &'static [RecorderKind] {
        if cfg!(target_os = "linux") {
            &[RecorderKind::Parec, RecorderKind::Arecord, RecorderKind::Sox, RecorderKind::Ffmpeg]
        } else {
            &[RecorderKind::Sox, RecorderKind::Ffmpeg]
        }
    }

    fn binary(self) -> &'static str {
        match self {
            RecorderKind::Parec => "parec",
            RecorderKind::Arecord => "arecord",
            RecorderKind::Sox => "sox",
            RecorderKind::Ffmpeg => "ffmpeg",
        }
    }

    /// The arguments that write raw sound (16 kHz, mono, signed 16-bit little endian) to standard
    /// output from `microphone`, or the system's default. `None` when this program cannot record
    /// without a name here (`ffmpeg` on Windows).
    pub fn arguments(self, microphone: Option<&str>) -> Option<Vec<String>> {
        let words = |list: &[&str]| list.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        let mut args = match self {
            RecorderKind::Parec => words(&["--raw", "--format=s16le", "--rate=16000", "--channels=1"]),
            RecorderKind::Arecord => words(&["-q", "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1"]),
            RecorderKind::Sox => words(&["-q"]),
            RecorderKind::Ffmpeg => words(&["-hide_banner", "-loglevel", "error"]),
        };
        match (self, microphone) {
            (RecorderKind::Parec, Some(name)) => args.push(format!("--device={name}")),
            (RecorderKind::Arecord, Some(name)) => args.extend(["-D".to_string(), name.to_string()]),
            (RecorderKind::Parec | RecorderKind::Arecord, None) => {}
            (RecorderKind::Sox, name) => {
                match name {
                    Some(name) => args.extend(["-t".to_string(), sox_driver().to_string(), name.to_string()]),
                    None => args.push("-d".to_string()),
                }
                args.extend(words(&["-t", "raw", "-r", "16000", "-e", "signed-integer", "-b", "16", "-c", "1", "-"]));
            }
            (RecorderKind::Ffmpeg, name) => {
                let (format, input) = ffmpeg_input(name)?;
                args.extend(["-f".to_string(), format.to_string(), "-i".to_string(), input]);
                args.extend(words(&["-ac", "1", "-ar", "16000", "-f", "s16le", "-"]));
            }
        }
        Some(args)
    }
}

/// SoX's driver for a named device on this system.
fn sox_driver() -> &'static str {
    if cfg!(target_os = "macos") {
        "coreaudio"
    } else if cfg!(windows) {
        "waveaudio"
    } else {
        "pulseaudio"
    }
}

/// `ffmpeg`'s input format and device on this system; `None` on Windows without a name, where
/// DirectShow has no default device.
fn ffmpeg_input(name: Option<&str>) -> Option<(&'static str, String)> {
    if cfg!(target_os = "macos") {
        // `:0` is the first audio device; a name works as well.
        Some(("avfoundation", format!(":{}", name.unwrap_or("0"))))
    } else if cfg!(windows) {
        name.map(|name| ("dshow", format!("audio={name}")))
    } else {
        Some(("pulse", name.unwrap_or("default").to_string()))
    }
}

/// A program that plays a WAV file ([`Found::player`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub program: PathBuf,
    /// `powershell` takes the file inside a command, the others as the last argument.
    pub powershell: bool,
}

impl Player {
    /// The command line that plays `file` and returns when it is done.
    pub fn command(&self, file: &Path) -> Vec<String> {
        let program = self.program.to_string_lossy().into_owned();
        if self.powershell {
            // Single quotes in PowerShell are doubled; nothing else is special inside them.
            let quoted = file.to_string_lossy().replace('\'', "''");
            let script = format!("(New-Object Media.SoundPlayer '{quoted}').PlaySync()");
            vec![program, "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script]
        } else {
            vec![program, file.to_string_lossy().into_owned()]
        }
    }
}

/// A program that plays raw sound from its standard input (issue #745): signed 16-bit little endian,
/// mono, at a rate it is told. Plays while the sound still comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKind {
    /// PulseAudio's and PipeWire's `paplay` (Linux).
    Paplay,
    /// PipeWire's `pw-play` (Linux).
    PwPlay,
    /// ALSA's `aplay` (Linux).
    Aplay,
    /// SoX's `play`, every system.
    Play,
    /// `ffplay` of ffmpeg, every system.
    Ffplay,
}

impl RawKind {
    /// The kinds to try on this system, best first.
    fn order() -> &'static [RawKind] {
        if cfg!(target_os = "linux") {
            &[RawKind::Paplay, RawKind::PwPlay, RawKind::Aplay, RawKind::Play, RawKind::Ffplay]
        } else {
            &[RawKind::Play, RawKind::Ffplay]
        }
    }

    fn binary(self) -> &'static str {
        match self {
            RawKind::Paplay => "paplay",
            RawKind::PwPlay => "pw-play",
            RawKind::Aplay => "aplay",
            RawKind::Play => "play",
            RawKind::Ffplay => "ffplay",
        }
    }
}

/// A [`RawKind`] found at its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPlayer {
    pub kind: RawKind,
    pub program: PathBuf,
}

impl RawPlayer {
    /// The command line that plays raw sound at `rate` from standard input and ends at its end.
    pub fn command(&self, rate: u32) -> Vec<String> {
        let mut command = vec![self.program.to_string_lossy().into_owned()];
        let words: Vec<String> = match self.kind {
            RawKind::Paplay => {
                vec!["--raw".into(), "--format=s16le".into(), format!("--rate={rate}"), "--channels=1".into()]
            }
            RawKind::PwPlay => {
                vec![
                    // Raw sound, no file container to look for (`-a`).
                    "--raw".into(),
                    "--format".into(),
                    "s16".into(),
                    "--rate".into(),
                    rate.to_string(),
                    "--channels".into(),
                    "1".into(),
                    "-".into(),
                ]
            }
            RawKind::Aplay => {
                vec![
                    "-q".into(),
                    "-t".into(),
                    "raw".into(),
                    "-f".into(),
                    "S16_LE".into(),
                    "-r".into(),
                    rate.to_string(),
                    "-c".into(),
                    "1".into(),
                ]
            }
            RawKind::Play => {
                let mut words: Vec<String> = ["-q", "-t", "raw", "-r"].map(String::from).to_vec();
                words.push(rate.to_string());
                words.extend(["-e", "signed-integer", "-b", "16", "-c", "1", "-"].map(String::from));
                words
            }
            RawKind::Ffplay => {
                let mut words: Vec<String> =
                    ["-hide_banner", "-loglevel", "error", "-nodisp", "-autoexit", "-f", "s16le", "-ar"]
                        .map(String::from)
                        .to_vec();
                words.push(rate.to_string());
                // `-ch_layout`: newer ffplay no longer knows `-ac` for its input.
                words.extend(["-ch_layout", "mono", "-i", "-"].map(String::from));
                words
            }
        };
        command.extend(words);
        command
    }
}

/// What the listener can use now, as found by [`Found::detect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The recording program and its kind.
    pub recorder: Option<(RecorderKind, PathBuf)>,
    /// The speech to text that was looked for: only its parts are filled in.
    pub recognizer: Recognizer,
    /// A Python with `onnx-asr` importable, for Parakeet.
    pub parakeet: Option<PathBuf>,
    /// The Parakeet model folder ([`PARAKEET_MODEL`]) with its vocabulary and model files.
    pub parakeet_model: Option<PathBuf>,
    /// The model folder holds the smaller int8 files, which are loaded instead of the full ones.
    pub parakeet_int8: bool,
    /// `whisper-cli` (whisper.cpp).
    pub whisper: Option<PathBuf>,
    /// The whisper.cpp model.
    pub stt_model: Option<PathBuf>,
    /// A Python with openWakeWord importable.
    pub python: Option<PathBuf>,
    /// The openWakeWord model of "Hey Torro".
    pub wake_model: Option<PathBuf>,
    /// `piper`.
    pub piper: Option<PathBuf>,
    /// The Piper voice.
    pub voice: Option<PathBuf>,
    /// A program that plays sound.
    pub player: Option<Player>,
    /// `pactl` or `arecord`, to list the microphones (Linux).
    pub lister: Option<PathBuf>,
    /// A program that plays raw sound as it comes (issue #745).
    pub raw_player: Option<RawPlayer>,
    /// A Python with `faster-qwen3-tts` (issue #745): the one of [`qwen_folder`], else one on `PATH`.
    pub qwen_python: Option<PathBuf>,
}

impl Found {
    /// Looks for every part with `finder`, as `voice` sets it; of speech to text only the chosen
    /// one. Starts Python to see whether openWakeWord (and, for Parakeet, `onnx-asr`) is
    /// installed, nothing else.
    pub fn detect(finder: &Finder, voice: &Voice) -> Found {
        let microphone = voice.microphone.as_deref();
        let recorder = RecorderKind::order().iter().find_map(|&kind| {
            kind.arguments(microphone)?;
            finder.program(&[kind.binary()]).map(|path| (kind, path))
        });
        let python = finder.program(&["python3", "python"]).filter(|python| imports_openwakeword(python));
        let wake_model = finder.file(None, |path| {
            path.file_stem().is_some_and(|stem| stem == WAKE_MODEL)
                && path.extension().is_some_and(|ext| ext == "onnx" || ext == "tflite")
        });
        let whisper = voice.recognizer == Recognizer::Whisper;
        let stt_model = finder
            .file(voice.stt_model.as_deref(), |path| {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                name.starts_with("ggml-") && name.ends_with(".bin")
            })
            .filter(|_| whisper);
        let parakeet = if whisper { None } else { parakeet_python(finder) };
        let model_folder = finder.folder.as_deref().filter(|_| !whisper).map(|folder| folder.join(PARAKEET_MODEL));
        let (parakeet_model, parakeet_int8) = match model_folder {
            Some(folder) => match parakeet_model(&folder) {
                Some(int8) => (Some(folder), int8),
                None => (None, false),
            },
            None => (None, false),
        };
        let piper_voice = finder.file(voice.voice.as_deref(), |path| {
            path.extension().is_some_and(|ext| ext == "onnx")
                && path.file_stem().is_none_or(|stem| stem != WAKE_MODEL)
                && path.with_extension("onnx.json").is_file()
        });
        Found {
            recorder,
            recognizer: voice.recognizer,
            parakeet,
            parakeet_model,
            parakeet_int8,
            whisper: finder.program(&["whisper-cli", "whisper-cpp"]).filter(|_| whisper),
            stt_model,
            python,
            wake_model,
            piper: finder.program(&["piper"]),
            voice: piper_voice,
            player: player(finder),
            lister: finder.program(&["pactl", "arecord"]).filter(|_| cfg!(target_os = "linux")),
            raw_player: raw_player(finder),
            qwen_python: qwen_python_in(finder),
        }
    }

    /// Whether some program plays sound, raw or from a file.
    pub fn plays(&self) -> bool {
        self.raw_player.is_some() || self.player.is_some()
    }

    /// Every part as `settings.voice_status` reports it, in the order of [`VoicePart`].
    pub fn status(&self) -> Vec<VoiceProgram> {
        let name = |path: &Path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
        let part = |part, program: Option<String>, missing| VoiceProgram { part, program, missing };
        let wake = match (&self.python, &self.wake_model) {
            (Some(_), Some(_)) => part(VoicePart::WakeWord, Some("openWakeWord".into()), None),
            (Some(_), None) => part(VoicePart::WakeWord, Some("openWakeWord".into()), Some(VoiceMissing::Model)),
            (None, _) => part(VoicePart::WakeWord, None, Some(VoiceMissing::Program)),
        };
        let recorder = match &self.recorder {
            Some((_, path)) => part(VoicePart::Recorder, name(path), None),
            None => part(VoicePart::Recorder, None, Some(VoiceMissing::Program)),
        };
        let stt = match self.recognizer {
            Recognizer::Whisper => match (&self.whisper, &self.stt_model) {
                (Some(path), Some(_)) => part(VoicePart::SpeechToText, name(path), None),
                (Some(path), None) => part(VoicePart::SpeechToText, name(path), Some(VoiceMissing::Model)),
                (None, _) => part(VoicePart::SpeechToText, None, Some(VoiceMissing::Program)),
            },
            Recognizer::Parakeet => match (&self.parakeet, &self.parakeet_model) {
                (Some(_), Some(_)) => part(VoicePart::SpeechToText, Some(ONNX_ASR.into()), None),
                (Some(_), None) => part(VoicePart::SpeechToText, Some(ONNX_ASR.into()), Some(VoiceMissing::Model)),
                (None, _) => part(VoicePart::SpeechToText, None, Some(VoiceMissing::Program)),
            },
        };
        let speech = match (&self.piper, &self.voice, &self.player) {
            (Some(path), Some(_), Some(_)) => part(VoicePart::Speech, name(path), None),
            (Some(path), None, _) => part(VoicePart::Speech, name(path), Some(VoiceMissing::Model)),
            (Some(path), Some(_), None) => part(VoicePart::Speech, name(path), Some(VoiceMissing::Player)),
            (None, _, _) => part(VoicePart::Speech, None, Some(VoiceMissing::Program)),
        };
        vec![wake, recorder, stt, speech]
    }

    /// The microphones the recording program can name; empty when they cannot be listed.
    pub fn microphones(&self) -> Vec<String> {
        let Some(lister) = &self.lister else { return Vec::new() };
        let pactl = lister.file_stem().is_some_and(|stem| stem == "pactl");
        let arguments: &[&str] = if pactl { &["list", "short", "sources"] } else { &["-L"] };
        let Ok(output) = Command::new(lister).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output()
        else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        if pactl { pactl_sources(&text) } else { alsa_devices(&text) }
    }
}

/// The first program that plays sound on this system.
fn player(finder: &Finder) -> Option<Player> {
    let names: &[&str] = if cfg!(target_os = "macos") {
        &["afplay", "play"]
    } else if cfg!(windows) {
        &["powershell", "play"]
    } else {
        &["paplay", "pw-play", "aplay", "play"]
    };
    names.iter().find_map(|name| {
        let program = finder.program(&[name])?;
        Some(Player { program, powershell: *name == "powershell" })
    })
}

/// The name Parakeet's program is shown with, and the program of a `pipx` or `uv tool` install.
const ONNX_ASR: &str = "onnx-asr";

/// The first Python that imports `onnx-asr`: the one of its program `onnx-asr` on `PATH` (a
/// `pipx` or `uv tool` install), then `python3`, then `python`.
fn parakeet_python(finder: &Finder) -> Option<PathBuf> {
    let installed = finder.program(&[ONNX_ASR]).and_then(|script| script_python(&script, finder));
    installed
        .into_iter()
        .chain(finder.program(&["python3"]))
        .chain(finder.program(&["python"]))
        .find(|python| imports(python, "onnx_asr"))
}

/// The Python a Python program runs with, from its first lines: `#!/path/python`,
/// `#!/usr/bin/env python3`, or the `/bin/sh` trampoline `pip` writes for long paths, with the path
/// bare, in single or in double quotes (`'''exec' /path/python "$0" "$@"`). `None` for anything else.
pub fn script_python(script: &Path, finder: &Finder) -> Option<PathBuf> {
    let mut head = Vec::new();
    std::fs::File::open(script).ok()?.take(1024).read_to_end(&mut head).ok()?;
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.lines();
    let mut words = lines.next()?.strip_prefix("#!")?.split_whitespace();
    let interpreter = Path::new(words.next()?);
    let stem = interpreter.file_stem()?.to_string_lossy();
    if stem == "env" {
        return words.next().and_then(|name| finder.program(&[name]));
    }
    if stem == "sh" {
        let exec = lines.find_map(|line| line.strip_prefix("'''exec' "))?.trim_start();
        let path = match exec.chars().next()? {
            quote @ ('\'' | '"') => exec[1..].split(quote).next()?,
            _ => exec.split_whitespace().next()?,
        };
        return Some(PathBuf::from(path)).filter(|path| path.is_file());
    }
    Some(interpreter.to_path_buf()).filter(|path| path.is_file())
}

/// Whether `folder` holds a Parakeet model: its vocabulary and either the int8 files (`Some(true)`)
/// or the full ones (`Some(false)`), whose encoder keeps its weights in `encoder-model.onnx.data`.
fn parakeet_model(folder: &Path) -> Option<bool> {
    let has = |name: &str| folder.join(name).is_file();
    if !has("vocab.txt") {
        return None;
    }
    if has("encoder-model.int8.onnx") && has("decoder_joint-model.int8.onnx") {
        Some(true)
    } else if has("encoder-model.onnx") && has("encoder-model.onnx.data") && has("decoder_joint-model.onnx") {
        Some(false)
    } else {
        None
    }
}

/// The first program that plays raw sound on this system.
fn raw_player(finder: &Finder) -> Option<RawPlayer> {
    RawKind::order()
        .iter()
        .find_map(|&kind| finder.program(&[kind.binary()]).map(|program| RawPlayer { kind, program }))
}

/// The Python of the Qwen environment when it is there, else a Python on `PATH` that has
/// `faster-qwen3-tts` (only looked up, not imported: that would load PyTorch for seconds).
fn qwen_python_in(finder: &Finder) -> Option<PathBuf> {
    if let Some(folder) = &finder.folder {
        // The environment counts only once the package is in it: `pip install` may have failed.
        let own = qwen_python(&qwen_folder(folder));
        if own.is_file() && finds_module(&own, "faster_qwen3_tts") {
            return Some(own);
        }
    }
    finder.program(&["python3", "python"]).filter(|python| finds_module(python, "faster_qwen3_tts"))
}

/// Whether `python` finds the module `name` without importing it, isolated (`-I`) as the speech
/// helper starts it, so a package only the user's own site has is not counted.
fn finds_module(python: &Path, name: &str) -> bool {
    let script = format!("import importlib.util, sys; sys.exit(importlib.util.find_spec('{name}') is None)");
    Command::new(python)
        .args(["-I", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether `python` can import openWakeWord: it is installed (`pip install openwakeword`).
fn imports_openwakeword(python: &Path) -> bool {
    imports(python, "openwakeword")
}

/// Whether `python` can import `module`.
fn imports(python: &Path, module: &str) -> bool {
    Command::new(python)
        .args(["-c", &format!("import {module}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The sources of `pactl list short sources` without the monitors of outputs: the second column.
fn pactl_sources(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split('\t').nth(1))
        .filter(|name| !name.ends_with(".monitor"))
        .map(str::to_string)
        .collect()
}

/// The devices of `arecord -L`: the lines that do not start with a space (the others describe
/// them), without `null`.
fn alsa_devices(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with(char::is_whitespace))
        .filter(|line| *line != "null")
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
pub mod tests {
    use std::fs;

    use super::*;

    /// Creates the program `name` in `dir`: an empty file (with `.exe` on Windows), or on Unix a
    /// shell script with `body`, executable.
    pub fn fake_program(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        // `--torro-probe` only answers that the program starts (see below), nothing else.
        fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = --torro-probe ] && exit 0\n{body}\n"))
            .expect("write the program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
            ready_to_run(&path);
        }
        path
    }

    /// Waits until `path` can be started (issue #699). A test running beside this one may fork while
    /// the program is written; its child holds the file open for writing until it starts its own
    /// program, and until then starting this one fails with "Text file busy" (ETXTBSY). No new
    /// writer can come once the file is closed, so a probe that starts is enough.
    #[cfg(unix)]
    fn ready_to_run(path: &Path) {
        const TEXT_FILE_BUSY: i32 = 26;
        for _ in 0..200 {
            match std::process::Command::new(path).arg("--torro-probe").status() {
                Err(err) if err.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                _ => return,
            }
        }
    }

    fn missing(found: &Found) -> Vec<(VoicePart, Option<VoiceMissing>)> {
        found.status().into_iter().map(|program| (program.part, program.missing)).collect()
    }

    #[test]
    fn nothing_installed_misses_every_program() {
        let path = tempfile::tempdir().expect("temp dir");
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: None };
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!(found, Found::default());
        assert_eq!(
            missing(&found),
            [
                (VoicePart::WakeWord, Some(VoiceMissing::Program)),
                (VoicePart::Recorder, Some(VoiceMissing::Program)),
                (VoicePart::SpeechToText, Some(VoiceMissing::Program)),
                (VoicePart::Speech, Some(VoiceMissing::Program)),
            ]
        );
        assert!(found.microphones().is_empty());
    }

    /// Fake programs in a temporary `PATH` and models in a temporary voice folder.
    #[test]
    fn programs_are_found_on_path_and_models_in_the_folder() {
        let path = tempfile::tempdir().expect("temp dir");
        let folder = tempfile::tempdir().expect("voice folder");
        for name in ["whisper-cli", "piper", "sox", "ffmpeg"] {
            fake_program(path.path(), name, "exit 0");
        }
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: Some(folder.path().to_path_buf()) };
        let whisper = Voice { recognizer: Recognizer::Whisper, ..Voice::default() };
        let found = Found::detect(&finder, &whisper);
        assert_eq!(
            found.whisper.as_deref(),
            Some(path.path().join(format!("whisper-cli{}", std::env::consts::EXE_SUFFIX)).as_path())
        );
        assert_eq!(found.recorder.as_ref().map(|(kind, _)| *kind), Some(RecorderKind::Sox), "SoX before ffmpeg");
        let stt = found.status()[2].clone();
        assert_eq!((stt.program.as_deref(), stt.missing), (Some("whisper-cli"), Some(VoiceMissing::Model)));
        assert_eq!(found.status()[3].missing, Some(VoiceMissing::Model), "piper without a voice");

        // A model and a voice (with its JSON) in the folder; a voice without JSON does not count.
        fs::write(folder.path().join("ggml-base.bin"), b"model").expect("model");
        fs::write(folder.path().join("ggml-tiny.bin"), b"model").expect("model");
        fs::write(folder.path().join("aa-broken.onnx"), b"voice").expect("voice without config");
        fs::write(folder.path().join("de_DE-thorsten-medium.onnx"), b"voice").expect("voice");
        fs::write(folder.path().join("de_DE-thorsten-medium.onnx.json"), b"{}").expect("config");
        fs::write(folder.path().join("hey_torro.onnx"), b"wake").expect("wake model");
        let found = Found::detect(&finder, &whisper);
        assert_eq!(found.stt_model, Some(folder.path().join("ggml-base.bin")), "the first by name");
        assert_eq!(found.voice, Some(folder.path().join("de_DE-thorsten-medium.onnx")));
        assert_eq!(found.wake_model, Some(folder.path().join("hey_torro.onnx")));
        assert_eq!(found.status()[2].missing, None);

        // A model the settings name wins; one that is not there is missing.
        let named =
            Voice { stt_model: Some(folder.path().join("ggml-tiny.bin").to_string_lossy().into()), ..whisper.clone() };
        assert_eq!(Found::detect(&finder, &named).stt_model, Some(folder.path().join("ggml-tiny.bin")));
        let gone = Voice { stt_model: Some(folder.path().join("ggml-gone.bin").to_string_lossy().into()), ..whisper };
        assert_eq!(Found::detect(&finder, &gone).status()[2].missing, Some(VoiceMissing::Model));
    }

    /// openWakeWord counts only when its Python imports it: the fake `python3` answers like Python
    /// with and without the package.
    #[cfg(unix)]
    #[test]
    fn open_wake_word_needs_python_with_the_package_and_the_model() {
        let path = tempfile::tempdir().expect("temp dir");
        let folder = tempfile::tempdir().expect("voice folder");
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: Some(folder.path().to_path_buf()) };
        fake_program(path.path(), "python3", "exit 1");
        assert_eq!(Found::detect(&finder, &Voice::default()).status()[0].missing, Some(VoiceMissing::Program));
        fake_program(path.path(), "python3", "[ \"$2\" = \"import openwakeword\" ] && exit 0; exit 1");
        assert_eq!(Found::detect(&finder, &Voice::default()).status()[0].missing, Some(VoiceMissing::Model));
        fs::write(folder.path().join("hey_torro.tflite"), b"wake").expect("wake model");
        let wake = Found::detect(&finder, &Voice::default()).status()[0].clone();
        assert_eq!((wake.program.as_deref(), wake.missing), (Some("openWakeWord"), None));
    }

    /// Issue #749: Parakeet needs a Python that imports `onnx-asr` and the model folder with its
    /// vocabulary; the int8 files are preferred. Whisper is not looked for meanwhile.
    #[cfg(unix)]
    #[test]
    fn parakeet_needs_python_with_onnx_asr_and_the_model_folder() {
        let path = tempfile::tempdir().expect("temp dir");
        let folder = tempfile::tempdir().expect("voice folder");
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: Some(folder.path().to_path_buf()) };
        fake_program(path.path(), "whisper-cli", "exit 0");
        let stt = |found: &Found| found.status()[2].clone();
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!(found.whisper, None, "only the chosen one is looked for");
        assert_eq!((stt(&found).program, stt(&found).missing), (None, Some(VoiceMissing::Program)));

        fake_program(path.path(), "python3", "[ \"$2\" = \"import onnx_asr\" ] && exit 0; exit 1");
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!(stt(&found).program.as_deref(), Some("onnx-asr"));
        assert_eq!(stt(&found).missing, Some(VoiceMissing::Model), "no model folder");

        let model = folder.path().join(PARAKEET_MODEL);
        fs::create_dir(&model).expect("model folder");
        for file in ["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "encoder-model.onnx"] {
            fs::write(model.join(file), b"model").expect("model file");
        }
        assert_eq!(stt(&Found::detect(&finder, &Voice::default())).missing, Some(VoiceMissing::Model), "no vocab.txt");
        fs::write(model.join("vocab.txt"), b"a 0").expect("vocabulary");
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!((found.parakeet_model.as_deref(), found.parakeet_int8), (Some(model.as_path()), true));
        assert_eq!(stt(&found).missing, None);

        fs::remove_file(model.join("decoder_joint-model.int8.onnx")).expect("remove");
        assert_eq!(stt(&Found::detect(&finder, &Voice::default())).missing, Some(VoiceMissing::Model));
        fs::write(model.join("decoder_joint-model.onnx"), b"model").expect("full model");
        assert_eq!(stt(&Found::detect(&finder, &Voice::default())).missing, Some(VoiceMissing::Model), "no .data");
        fs::write(model.join("encoder-model.onnx.data"), b"weights").expect("weights");
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!((found.parakeet_model.is_some(), found.parakeet_int8), (true, false), "the full files");

        let whisper = Found::detect(&finder, &Voice { recognizer: Recognizer::Whisper, ..Voice::default() });
        assert_eq!((&whisper.parakeet, &whisper.parakeet_model), (&None, &None));
        assert_eq!(stt(&whisper).program.as_deref(), Some("whisper-cli"));
    }

    /// Issue #749: the Python of a `pipx` or `uv tool` install comes from the first lines of its
    /// program `onnx-asr`, before `python3` on `PATH`.
    #[cfg(unix)]
    #[test]
    fn the_python_of_an_onnx_asr_install_comes_first() {
        let path = tempfile::tempdir().expect("temp dir");
        let venv = tempfile::tempdir().expect("venv");
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: None };
        // python3 on PATH has no onnx-asr; the one of the install has.
        fake_program(path.path(), "python3", "exit 1");
        let python = fake_program(venv.path(), "python", "[ \"$2\" = \"import onnx_asr\" ] && exit 0; exit 1");
        let script = path.path().join("onnx-asr");
        fs::write(&script, format!("#!{}\nimport onnx_asr\n", python.display())).expect("script");
        assert_eq!(script_python(&script, &finder), Some(python.clone()));
        assert_eq!(Found::detect(&finder, &Voice::default()).parakeet, Some(python.clone()));

        // pip's trampoline for long paths, and `env`.
        let trampoline = format!("#!/bin/sh\n'''exec' '{}' \"$0\" \"$@\"\n' '''\nimport onnx_asr\n", python.display());
        fs::write(&script, trampoline).expect("script");
        assert_eq!(script_python(&script, &finder), Some(python.clone()));
        for quoted in [python.display().to_string(), format!("\"{}\"", python.display())] {
            fs::write(&script, format!("#!/bin/sh\n'''exec' {quoted} \"$0\" \"$@\"\n' '''\n")).expect("script");
            assert_eq!(script_python(&script, &finder), Some(python.clone()), "{quoted}");
        }
        fs::write(&script, "#!/usr/bin/env python3\n").expect("script");
        assert_eq!(script_python(&script, &finder), Some(path.path().join("python3")));
        fs::write(&script, "no shebang\n").expect("script");
        assert_eq!(script_python(&script, &finder), None);
        assert_eq!(Found::detect(&finder, &Voice::default()).parakeet, None, "python3 lacks onnx-asr");
    }

    #[test]
    fn recorders_write_raw_16_khz_mono_to_their_output() {
        let parec = RecorderKind::Parec.arguments(Some("alsa_input.usb")).expect("parec");
        assert!(parec.contains(&"--rate=16000".to_string()) && parec.contains(&"--device=alsa_input.usb".to_string()));
        let arecord = RecorderKind::Arecord.arguments(None).expect("arecord");
        assert_eq!(arecord.join(" "), "-q -t raw -f S16_LE -r 16000 -c 1");
        let sox = RecorderKind::Sox.arguments(None).expect("sox");
        assert_eq!(sox.join(" "), "-q -d -t raw -r 16000 -e signed-integer -b 16 -c 1 -");
        let named = RecorderKind::Sox.arguments(Some("USB mic")).expect("sox");
        assert_eq!(&named[1..4], ["-t", sox_driver(), "USB mic"]);
        assert_eq!(RecorderKind::Ffmpeg.arguments(None).is_none(), cfg!(windows), "DirectShow needs a name");
        let ffmpeg = RecorderKind::Ffmpeg.arguments(Some("Mic")).expect("ffmpeg");
        assert!(ffmpeg.ends_with(&["-ac", "1", "-ar", "16000", "-f", "s16le", "-"].map(String::from)));
    }

    #[test]
    fn microphones_are_read_from_pactl_and_arecord() {
        let pactl = "45\talsa_output.pci.analog-stereo.monitor\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n\
                     46\talsa_input.usb-Rode_NT-USB-00.analog-stereo\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n";
        assert_eq!(pactl_sources(pactl), ["alsa_input.usb-Rode_NT-USB-00.analog-stereo"]);
        let alsa =
            "null\n    Discard all samples\ndefault\n    Default Audio Device\nhw:CARD=NTUSB,DEV=0\n    NT-USB\n";
        assert_eq!(alsa_devices(alsa), ["default", "hw:CARD=NTUSB,DEV=0"]);
    }

    /// Issue #745: raw players get the rate and read standard input; Qwen is found in its own
    /// environment first.
    #[test]
    fn raw_players_read_their_input_at_the_rate() {
        let paplay = RawPlayer { kind: RawKind::Paplay, program: PathBuf::from("paplay") };
        assert_eq!(paplay.command(24_000).join(" "), "paplay --raw --format=s16le --rate=24000 --channels=1");
        let play = RawPlayer { kind: RawKind::Play, program: PathBuf::from("play") };
        assert!(play.command(22_050).join(" ").ends_with("-r 22050 -e signed-integer -b 16 -c 1 -"));
        let ffplay = RawPlayer { kind: RawKind::Ffplay, program: PathBuf::from("ffplay") };
        assert!(ffplay.command(24_000).ends_with(&["-ch_layout", "mono", "-i", "-"].map(String::from)));

        let path = tempfile::tempdir().expect("temp dir");
        let folder = tempfile::tempdir().expect("voice folder");
        fake_program(path.path(), "play", "exit 0");
        let finder = Finder { dirs: vec![path.path().to_path_buf()], folder: Some(folder.path().to_path_buf()) };
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!(found.raw_player.map(|player| player.kind), Some(RawKind::Play));
        assert!(found.qwen_python.is_none());
        // An environment without the package does not count; with it, it does (a fake Python that
        // answers like `find_spec`).
        let python = qwen_python(&qwen_folder(folder.path()));
        let bin = python.parent().expect("bin");
        fs::create_dir_all(bin).expect("venv");
        #[cfg(unix)]
        {
            fake_program(bin, "python", "exit 1");
            assert!(Found::detect(&finder, &Voice::default()).qwen_python.is_none(), "pip install failed");
            fake_program(bin, "python", "case \"$3\" in *faster_qwen3_tts*) exit 0 ;; esac; exit 1");
            assert_eq!(Found::detect(&finder, &Voice::default()).qwen_python, Some(python));
        }
        let setup = qwen_setup(&qwen_folder(folder.path()));
        assert!(setup.contains("-m venv") && setup.ends_with(QWEN_PACKAGES), "{setup}");
    }

    #[test]
    fn players_take_the_file() {
        let player = Player { program: PathBuf::from("afplay"), powershell: false };
        assert_eq!(player.command(Path::new("/tmp/a.wav")), ["afplay", "/tmp/a.wav"]);
        let windows = Player { program: PathBuf::from("powershell"), powershell: true };
        let command = windows.command(Path::new("C:/Temp/it's.wav"));
        assert_eq!(
            command.last().map(String::as_str),
            Some("(New-Object Media.SoundPlayer 'C:/Temp/it''s.wav').PlaySync()")
        );
    }
}
