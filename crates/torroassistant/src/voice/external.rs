//! The parts of the listener over the local programs [`super::programs`] found: recording with
//! `parec`/`arecord`/SoX/`ffmpeg` ([`CommandRecorder`]), the wake word with openWakeWord
//! ([`OpenWakeWord`]), speech to text with whisper.cpp ([`WhisperCli`]) and speech with Piper
//! ([`Piper`]). [`Parts::build`] puts them together for the settings.
//!
//! Every program runs without a shell, its arguments one by one, so a microphone's or a file's
//! name is never read as a command.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use torro_core::settings::Voice;
use torro_ipc::{VoiceMissing, VoicePart};

use super::audio::{FRAME, TempWav, chime};
use super::programs::{Found, Player};
use super::wake::TranscriptWake;
use super::{Recorder, Speaker, Stream, Transcriber, VoiceError, WakeWord};

/// The parts the listener runs with.
pub struct Parts {
    pub recorder: Box<dyn Recorder>,
    pub wake: Box<dyn WakeWord>,
    pub transcriber: Box<dyn Transcriber>,
    pub speaker: Box<dyn Speaker>,
}

impl Parts {
    /// The parts from what was `found`, for `voice`. Recording and speech to text are needed;
    /// without openWakeWord the wake word is found with speech to text, without Piper answers are
    /// only shown.
    pub fn build(found: &Found, voice: &Voice) -> Result<Parts, VoiceError> {
        let recorder = CommandRecorder::of(found, voice.microphone.as_deref())?;
        let whisper = WhisperCli::of(found)?;
        let wake: Box<dyn WakeWord> = match (&found.python, &found.wake_model) {
            (Some(python), Some(model)) => Box::new(OpenWakeWord::new(python.clone(), model.clone())),
            _ => Box::new(TranscriptWake::new(Box::new(whisper.clone()))),
        };
        Ok(Parts {
            recorder: Box::new(recorder),
            wake,
            transcriber: Box::new(whisper),
            speaker: Box::new(Piper::of(found)),
        })
    }
}

/// Starts `command` (program and arguments) with the given pipes; a program that does not start
/// is a [`VoiceError::Program`].
fn spawn(command: &[String], stdin: Stdio, stdout: Stdio) -> Result<Child, VoiceError> {
    let (program, args) = command.split_first().ok_or_else(|| VoiceError::program("", "no program"))?;
    Command::new(program)
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| VoiceError::program(program_name(program), err))
}

/// The file name of `program`, for messages.
fn program_name(program: &str) -> String {
    Path::new(program).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Runs `command` to its end; its standard output, or why it failed.
fn run(command: &[String], input: Option<&str>) -> Result<String, VoiceError> {
    let name = command.first().map(|program| program_name(program)).unwrap_or_default();
    let stdin = if input.is_some() { Stdio::piped() } else { Stdio::null() };
    let mut child = spawn(command, stdin, Stdio::piped())?;
    if let (Some(text), Some(mut pipe)) = (input, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).map_err(|err| VoiceError::program(&name, err))?;
    }
    let output = child.wait_with_output().map_err(|err| VoiceError::program(&name, err))?;
    if !output.status.success() {
        return Err(VoiceError::program(&name, format!("ended with {}", output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The microphone through a recording program that writes raw sound to its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecorder {
    /// Program and arguments.
    pub command: Vec<String>,
}

impl CommandRecorder {
    /// The recorder `found`, recording from `microphone` or the default.
    pub fn of(found: &Found, microphone: Option<&str>) -> Result<CommandRecorder, VoiceError> {
        let missing = VoiceError::Missing(VoicePart::Recorder, VoiceMissing::Program);
        let (kind, program) = found.recorder.as_ref().ok_or(missing.clone())?;
        let mut command = vec![program.to_string_lossy().into_owned()];
        command.extend(kind.arguments(microphone).ok_or(missing)?);
        Ok(CommandRecorder { command })
    }
}

impl Recorder for CommandRecorder {
    fn open(&mut self) -> Result<Box<dyn Stream>, VoiceError> {
        let mut child = spawn(&self.command, Stdio::null(), Stdio::piped())?;
        let name = self.command.first().map(|program| program_name(program)).unwrap_or_default();
        let output = child.stdout.take().ok_or_else(|| VoiceError::program(&name, "no output"))?;
        Ok(Box::new(ProcessStream { child, output, name }))
    }
}

/// A running recording program. Dropping it ends the program: the microphone closes.
struct ProcessStream {
    child: Child,
    output: ChildStdout,
    name: String,
}

impl Stream for ProcessStream {
    fn frame(&mut self) -> Result<Option<Vec<i16>>, VoiceError> {
        let mut bytes = vec![0u8; FRAME * 2];
        match self.output.read_exact(&mut bytes) {
            Ok(()) => Ok(Some(bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect())),
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(err) => Err(VoiceError::program(&self.name, err)),
        }
    }
}

impl Drop for ProcessStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The script openWakeWord runs in: reads frames of 1280 samples from its input and writes `wake`
/// when the model is sure enough, then starts afresh.
const OPEN_WAKE_WORD: &str = r#"
import sys
import numpy as np
from openwakeword.model import Model
path = sys.argv[1]
model = Model(wakeword_models=[path], inference_framework="tflite" if path.endswith(".tflite") else "onnx")
size = 1280 * 2
source = sys.stdin.buffer
while True:
    data = source.read(size)
    if len(data) < size:
        break
    scores = model.predict(np.frombuffer(data, dtype=np.int16))
    if max(scores.values()) >= 0.5:
        print("wake", flush=True)
        model.reset()
"#;

/// openWakeWord in a Python process of its own, started with the first frame: frames go to its
/// input, `wake` comes back on its output, read on a thread so a frame never waits for it.
pub struct OpenWakeWord {
    python: PathBuf,
    model: PathBuf,
    running: Option<(Child, ChildStdin, Receiver<String>)>,
}

impl OpenWakeWord {
    pub fn new(python: PathBuf, model: PathBuf) -> OpenWakeWord {
        OpenWakeWord { python, model, running: None }
    }

    fn start(&mut self) -> Result<(), VoiceError> {
        let command = [
            self.python.to_string_lossy().into_owned(),
            "-u".into(),
            "-c".into(),
            OPEN_WAKE_WORD.into(),
            self.model.to_string_lossy().into_owned(),
        ];
        let mut child = spawn(&command, Stdio::piped(), Stdio::piped())?;
        let input = child.stdin.take().ok_or_else(|| VoiceError::program("openWakeWord", "no input"))?;
        let output = child.stdout.take().ok_or_else(|| VoiceError::program("openWakeWord", "no output"))?;
        let (lines, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        self.running = Some((child, input, heard));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some((mut child, input, _)) = self.running.take() {
            drop(input);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl WakeWord for OpenWakeWord {
    fn hears(&mut self, frame: &[i16]) -> Result<bool, VoiceError> {
        if self.running.is_none() {
            self.start()?;
        }
        let Some((_, input, heard)) = self.running.as_mut() else { return Ok(false) };
        let bytes: Vec<u8> = frame.iter().flat_map(|sample| sample.to_le_bytes()).collect();
        if let Err(err) = input.write_all(&bytes) {
            self.stop();
            return Err(VoiceError::program("openWakeWord", err));
        }
        loop {
            match heard.try_recv() {
                Ok(line) if line.trim() == "wake" => return Ok(true),
                Ok(_) => {}
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => {
                    self.stop();
                    return Err(VoiceError::program("openWakeWord", "stopped"));
                }
            }
        }
    }

    /// Starts the model afresh with the next frame: what it heard during the answer is gone.
    fn reset(&mut self) {
        self.stop();
    }
}

impl Drop for OpenWakeWord {
    fn drop(&mut self) {
        self.stop();
    }
}

/// whisper.cpp's `whisper-cli` with a model: the sentence goes to a private temporary file, the
/// text comes from its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperCli {
    pub program: PathBuf,
    pub model: PathBuf,
}

impl WhisperCli {
    /// whisper.cpp as `found`.
    pub fn of(found: &Found) -> Result<WhisperCli, VoiceError> {
        let program =
            found.whisper.clone().ok_or(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Program))?;
        let model = found.stt_model.clone().ok_or(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model))?;
        Ok(WhisperCli { program, model })
    }
}

impl Transcriber for WhisperCli {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        let file = TempWav::write(samples)?;
        let path = |path: &Path| path.to_string_lossy().into_owned();
        // No timestamps, no progress: only the text. The language is found from the speech.
        let command = [
            path(&self.program),
            "-m".into(),
            path(&self.model),
            "-f".into(),
            path(file.path()),
            "-nt".into(),
            "-np".into(),
            "-l".into(),
            "auto".into(),
        ];
        Ok(transcript_of(&run(&command, None)?))
    }
}

/// The text in whisper.cpp's output: its lines joined, without the marks for what is no speech
/// (`[BLANK_AUDIO]`, `(music)`).
pub fn transcript_of(output: &str) -> String {
    let mut text = String::new();
    let mut depth = 0usize;
    for c in output.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(if c.is_control() { ' ' } else { c }),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Piper with a voice, played with a player; also the tone of the wake word. Without Piper or a
/// voice [`Speaker::say`] fails with what is missing; without a player nothing sounds at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piper {
    pub piper: Option<PathBuf>,
    pub voice: Option<PathBuf>,
    pub player: Option<Player>,
}

impl Piper {
    pub fn of(found: &Found) -> Piper {
        Piper { piper: found.piper.clone(), voice: found.voice.clone(), player: found.player.clone() }
    }

    fn play(&self, file: &Path) -> Result<(), VoiceError> {
        let player = self.player.as_ref().ok_or(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Player))?;
        run(&player.command(file), None).map(|_| ())
    }
}

impl Speaker for Piper {
    fn say(&mut self, text: &str) -> Result<(), VoiceError> {
        let piper = self.piper.as_ref().ok_or(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Program))?;
        let voice = self.voice.as_ref().ok_or(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Model))?;
        let file = TempWav::empty();
        let path = |path: &Path| path.to_string_lossy().into_owned();
        let command = [path(piper), "--model".into(), path(voice), "--output_file".into(), path(file.path())];
        // Piper reads the text from its input, a line at a time.
        let line = text.replace(['\n', '\r'], " ");
        run(&command, Some(&format!("{line}\n")))?;
        self.play(file.path())
    }

    fn chime(&mut self) {
        let Ok(file) = TempWav::write(&chime()) else { return };
        if let Err(err) = self.play(file.path()) {
            eprintln!("torroassistant: no tone: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_output_becomes_plain_text() {
        assert_eq!(transcript_of(" Hey Torro, what is on today?\n"), "Hey Torro, what is on today?");
        assert_eq!(transcript_of("[BLANK_AUDIO]\n"), "");
        assert_eq!(transcript_of(" (music) Search for\n Anna's mail.\n"), "Search for Anna's mail.");
    }

    #[test]
    fn missing_parts_are_named() {
        let found = Found::default();
        assert_eq!(
            CommandRecorder::of(&found, None).err(),
            Some(VoiceError::Missing(VoicePart::Recorder, VoiceMissing::Program))
        );
        assert_eq!(
            WhisperCli::of(&found).err(),
            Some(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Program))
        );
        assert!(Parts::build(&found, &Voice::default()).is_err());
        let mut piper = Piper::of(&found);
        assert_eq!(piper.say("Hi"), Err(VoiceError::Missing(VoicePart::Speech, VoiceMissing::Program)));
        piper.chime();
    }

    /// The programs with fake ones in a temporary folder (shell scripts, so Unix only): the
    /// recorder's sound arrives in frames, whisper's text comes back, Piper writes and the player
    /// plays, openWakeWord answers `wake` or stays quiet.
    #[cfg(unix)]
    mod with_fake_programs {
        use std::fs;
        use std::time::Duration;

        use super::super::super::audio::tests::fixture;
        use super::super::super::audio::wav_bytes;
        use super::super::super::programs::tests::fake_program;
        use super::*;

        #[test]
        fn the_recorder_streams_frames_until_it_ends() {
            let dir = tempfile::tempdir().expect("temp dir");
            let raw = dir.path().join("sound.raw");
            let samples = fixture("utterance.wav");
            fs::write(&raw, samples.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<_>>()).expect("raw");
            let program = fake_program(dir.path(), "parec", &format!("cat '{}'", raw.display()));
            let mut recorder = CommandRecorder { command: vec![program.to_string_lossy().into_owned()] };
            let mut stream = recorder.open().expect("opens");
            let mut frames = Vec::new();
            while let Some(frame) = stream.frame().expect("reads") {
                frames.push(frame);
            }
            assert_eq!(frames.len(), samples.len() / FRAME);
            assert_eq!(frames[3], samples[3 * FRAME..4 * FRAME]);
        }

        #[test]
        fn whisper_gets_the_model_and_a_wav_file() {
            let dir = tempfile::tempdir().expect("temp dir");
            let seen = dir.path().join("args");
            // Keeps its arguments and checks the file is there while it runs.
            let body = format!(
                "echo \"$@\" > '{}'; [ -f \"$4\" ] || exit 3; echo ' Hey Torro, what is on today?'",
                seen.display()
            );
            let program = fake_program(dir.path(), "whisper-cli", &body);
            let mut whisper = WhisperCli { program, model: dir.path().join("ggml-base.bin") };
            let text = whisper.transcribe(&fixture("utterance.wav")).expect("text");
            assert_eq!(text, "Hey Torro, what is on today?");
            let args = fs::read_to_string(&seen).expect("arguments");
            assert!(args.starts_with(&format!("-m {} -f ", dir.path().join("ggml-base.bin").display())), "{args}");
            assert!(args.trim_end().ends_with("-nt -np -l auto"), "{args}");
            let file = args.split_whitespace().nth(3).expect("the file");
            assert!(!Path::new(file).exists(), "the sentence does not stay on disk");
        }

        #[test]
        fn piper_speaks_through_the_player() {
            let dir = tempfile::tempdir().expect("temp dir");
            let played = dir.path().join("played");
            // Piper writes its input as the "sound"; the player keeps what it played.
            let piper = fake_program(dir.path(), "piper", "cat > \"$4\"");
            let player = fake_program(dir.path(), "afplay", &format!("cp \"$1\" '{}'", played.display()));
            let mut speaker = Piper {
                piper: Some(piper),
                voice: Some(dir.path().join("voice.onnx")),
                player: Some(Player { program: player, powershell: false }),
            };
            speaker.say("Two events\ntoday.").expect("spoken");
            assert_eq!(fs::read_to_string(&played).expect("played"), "Two events today.\n");
            speaker.chime();
            assert_eq!(fs::read(&played).expect("the tone"), wav_bytes(&chime()));
        }

        /// Feeds frames until `wake` arrives or a second is over.
        fn wakes(body: &str) -> bool {
            let dir = tempfile::tempdir().expect("temp dir");
            let python = fake_program(dir.path(), "python3", body);
            let mut wake = OpenWakeWord::new(python, dir.path().join("hey_torro.onnx"));
            let frame = vec![0i16; FRAME];
            for _ in 0..100 {
                if wake.hears(&frame).expect("hears") {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        }

        #[test]
        fn open_wake_word_wakes_on_its_signal() {
            assert!(wakes("echo wake; cat > /dev/null"));
        }

        #[test]
        fn open_wake_word_stays_quiet_without_it() {
            assert!(!wakes("echo 0.1; cat > /dev/null"));
        }
    }
}
