//! Which local programs the listener can use, found at runtime ([`Found::detect`]) – nothing is
//! downloaded or built in. The control centre shows the result (`settings.voice_status`, built
//! by [`Found::status`]) with a hint how to install what is missing.
//!
//! Programs are looked for on `PATH` ([`Finder`]); models and voices in the settings' paths or in
//! the voice folder `<data>/torroassistant/voice/` ([`voice_folder`]): `ggml-*.bin` for
//! whisper.cpp, `*.onnx` with its `*.onnx.json` for Piper, `hey_torro.onnx` (or `.tflite`) for
//! openWakeWord.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_core::settings::Voice;
use torro_ipc::{VoiceMissing, VoicePart, VoiceProgram};

/// The openWakeWord model of the fixed wake word (decision 11), without its extension.
pub const WAKE_MODEL: &str = "hey_torro";

/// The folder where models and voices are looked for when the settings name none; `None` without
/// a home directory.
pub fn voice_folder() -> Option<PathBuf> {
    AppPaths::for_app(AppId::ASSISTANT).map(|paths| paths.data.join("voice"))
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

/// What the listener can use now, as found by [`Found::detect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The recording program and its kind.
    pub recorder: Option<(RecorderKind, PathBuf)>,
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
}

impl Found {
    /// Looks for every part with `finder`, as `voice` sets it. Starts Python once to see whether
    /// openWakeWord is installed, nothing else.
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
        let stt_model = finder.file(voice.stt_model.as_deref(), |path| {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            name.starts_with("ggml-") && name.ends_with(".bin")
        });
        let piper_voice = finder.file(voice.voice.as_deref(), |path| {
            path.extension().is_some_and(|ext| ext == "onnx")
                && path.file_stem().is_none_or(|stem| stem != WAKE_MODEL)
                && path.with_extension("onnx.json").is_file()
        });
        Found {
            recorder,
            whisper: finder.program(&["whisper-cli", "whisper-cpp"]),
            stt_model,
            python,
            wake_model,
            piper: finder.program(&["piper"]),
            voice: piper_voice,
            player: player(finder),
            lister: finder.program(&["pactl", "arecord"]).filter(|_| cfg!(target_os = "linux")),
        }
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
        let stt = match (&self.whisper, &self.stt_model) {
            (Some(path), Some(_)) => part(VoicePart::SpeechToText, name(path), None),
            (Some(path), None) => part(VoicePart::SpeechToText, name(path), Some(VoiceMissing::Model)),
            (None, _) => part(VoicePart::SpeechToText, None, Some(VoiceMissing::Program)),
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

/// Whether `python` can import openWakeWord: it is installed (`pip install openwakeword`).
fn imports_openwakeword(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import openwakeword"])
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
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
        }
        path
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
        let found = Found::detect(&finder, &Voice::default());
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
        let found = Found::detect(&finder, &Voice::default());
        assert_eq!(found.stt_model, Some(folder.path().join("ggml-base.bin")), "the first by name");
        assert_eq!(found.voice, Some(folder.path().join("de_DE-thorsten-medium.onnx")));
        assert_eq!(found.wake_model, Some(folder.path().join("hey_torro.onnx")));
        assert_eq!(found.status()[2].missing, None);

        // A model the settings name wins; one that is not there is missing.
        let named =
            Voice { stt_model: Some(folder.path().join("ggml-tiny.bin").to_string_lossy().into()), ..Voice::default() };
        assert_eq!(Found::detect(&finder, &named).stt_model, Some(folder.path().join("ggml-tiny.bin")));
        let gone =
            Voice { stt_model: Some(folder.path().join("ggml-gone.bin").to_string_lossy().into()), ..Voice::default() };
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
