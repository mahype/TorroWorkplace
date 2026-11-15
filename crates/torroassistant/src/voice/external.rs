//! The parts of the listener over the local programs [`super::programs`] found: recording with
//! `parec`/`arecord`/SoX/`ffmpeg` ([`CommandRecorder`]), the wake word with openWakeWord
//! ([`OpenWakeWord`]), speech to text with Parakeet ([`ParakeetPython`]) or whisper.cpp
//! ([`WhisperCli`]), as the settings choose ([`transcriber_of`]), and speech with Piper
//! ([`Piper`]). [`Parts::build`] puts them together for the settings.
//!
//! Every program runs without a shell, its arguments one by one, so a microphone's or a file's
//! name is never read as a command.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use torro_core::settings::{Recognizer, Voice};
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
        let transcriber = SharedTranscriber::new(transcriber_of(found)?);
        let wake: Box<dyn WakeWord> = match (&found.python, &found.wake_model) {
            (Some(python), Some(model)) => Box::new(OpenWakeWord::new(python.clone(), model.clone())),
            _ => Box::new(TranscriptWake::new(Box::new(transcriber.clone()))),
        };
        Ok(Parts {
            recorder: Box::new(recorder),
            wake,
            transcriber: Box::new(transcriber),
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

/// The script Parakeet runs in: loads the model once, then reads sentences from its input, each
/// as 4 bytes (the number of samples, unsigned little endian) and that many 16-bit samples, and
/// writes each sentence's text as one line in UTF-8, whatever the system's encoding. A sentence it
/// cannot recognize is an empty line, so one bad sentence does not end the process (and with it the
/// wake word). The end of its input ends it.
const PARAKEET: &str = r#"
import struct
import sys
import numpy as np
import onnx_asr
sys.stdout.reconfigure(encoding="utf-8")
quantization = sys.argv[2] if len(sys.argv) > 2 else None
model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v3", sys.argv[1], quantization=quantization)
source = sys.stdin.buffer
while True:
    head = source.read(4)
    if len(head) < 4:
        break
    (count,) = struct.unpack("<I", head)
    data = source.read(count * 2)
    if len(data) < count * 2:
        break
    samples = np.frombuffer(data, dtype=np.int16).astype(np.float32) / 32768
    try:
        text = model.recognize(samples, sample_rate=16000)
    except Exception:
        text = ""
    print(" ".join(str(text).split()), flush=True)
"#;

/// Parakeet in a Python process of its own that keeps the model loaded: loading takes seconds, a
/// sentence about one. The process starts at once ([`ParakeetPython::start`]), so the model loads
/// while the first sentence is recorded; one that stopped starts again with the next sentence.
pub struct ParakeetPython {
    command: Vec<String>,
    running: Option<Running>,
}

/// A running Parakeet process: its pipes, and a thread that keeps the last line it wrote to its
/// error output, so a model that does not load says why ([`ParakeetPython::stop`]).
struct Running {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    errors: std::thread::JoinHandle<String>,
}

/// The longest reason from Parakeet's error output a message carries, in characters.
const MAX_REASON_CHARS: usize = 300;

impl ParakeetPython {
    /// Starts `python` with the model folder `model`, its int8 files if `int8`.
    pub fn start(python: &Path, model: &Path, int8: bool) -> Result<ParakeetPython, VoiceError> {
        let mut command = vec![
            python.to_string_lossy().into_owned(),
            "-u".into(),
            "-c".into(),
            PARAKEET.into(),
            model.to_string_lossy().into_owned(),
        ];
        if int8 {
            command.push("int8".into());
        }
        let mut parakeet = ParakeetPython { command, running: None };
        parakeet.run()?;
        Ok(parakeet)
    }

    fn run(&mut self) -> Result<(), VoiceError> {
        let (program, args) = self.command.split_first().ok_or_else(|| VoiceError::program("", "no program"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| VoiceError::program(PARAKEET_NAME, err))?;
        let input = child.stdin.take().ok_or_else(|| VoiceError::program(PARAKEET_NAME, "no input"))?;
        let output = child.stdout.take().ok_or_else(|| VoiceError::program(PARAKEET_NAME, "no output"))?;
        let stderr = child.stderr.take().ok_or_else(|| VoiceError::program(PARAKEET_NAME, "no error output"))?;
        // Read all the time, so a chatty library never fills the pipe and blocks the process.
        let errors = std::thread::spawn(move || {
            let mut last = String::new();
            for line in BufReader::new(stderr).split(b'\n') {
                let Ok(line) = line else { break };
                let line = String::from_utf8_lossy(&line).trim().to_string();
                if !line.is_empty() {
                    last = line;
                }
            }
            last
        });
        self.running = Some(Running { child, input, output: BufReader::new(output), errors });
        Ok(())
    }

    /// Ends the process; the last line it wrote to its error output (a Python error names the
    /// cause there, e.g. a model file that is missing), empty when there is none.
    fn stop(&mut self) -> String {
        let Some(Running { mut child, input, errors, .. }) = self.running.take() else { return String::new() };
        drop(input);
        let _ = child.kill();
        let _ = child.wait();
        // The process is gone, so its error output is closed and the thread ends.
        errors.join().unwrap_or_default().chars().take(MAX_REASON_CHARS).collect()
    }
}

/// The name Parakeet's process goes by in messages.
const PARAKEET_NAME: &str = "onnx-asr";

impl Transcriber for ParakeetPython {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        if self.running.is_none() {
            self.run()?;
        }
        let Some(Running { input, output, .. }) = self.running.as_mut() else { return Ok(String::new()) };
        let count = u32::try_from(samples.len()).map_err(|_| VoiceError::Sound("the sentence is too long".into()))?;
        let mut bytes = Vec::with_capacity(4 + samples.len() * 2);
        bytes.extend(count.to_le_bytes());
        bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
        // Read as bytes: a line that is not UTF-8 after all loses a letter, not the process.
        let mut line = Vec::new();
        let answered =
            input.write_all(&bytes).and_then(|()| input.flush()).and_then(|()| output.read_until(b'\n', &mut line));
        match answered {
            Ok(read) if read > 0 => Ok(String::from_utf8_lossy(&line).trim().to_string()),
            _ => {
                let reason = self.stop();
                let message = if reason.is_empty() { "stopped".to_string() } else { format!("stopped: {reason}") };
                Err(VoiceError::program(PARAKEET_NAME, message))
            }
        }
    }
}

impl Drop for ParakeetPython {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One transcriber shared by the wake word ([`TranscriptWake`]) and the sentences, so Parakeet
/// runs in one process with one model loaded.
#[derive(Clone)]
pub struct SharedTranscriber(Rc<RefCell<Box<dyn Transcriber>>>);

impl SharedTranscriber {
    pub fn new(transcriber: Box<dyn Transcriber>) -> SharedTranscriber {
        SharedTranscriber(Rc::new(RefCell::new(transcriber)))
    }
}

impl Transcriber for SharedTranscriber {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        // The listener runs one part at a time, so the transcriber is never in use twice.
        let mut transcriber =
            self.0.try_borrow_mut().map_err(|_| VoiceError::program("speech to text", "already transcribing"))?;
        transcriber.transcribe(samples)
    }
}

/// The speech to text chosen in the settings, as `found`: whisper.cpp, or Parakeet started at once.
/// What is missing is a [`VoiceError::Missing`] of [`VoicePart::SpeechToText`].
pub fn transcriber_of(found: &Found) -> Result<Box<dyn Transcriber>, VoiceError> {
    match found.recognizer {
        Recognizer::Whisper => Ok(Box::new(WhisperCli::of(found)?)),
        Recognizer::Parakeet => {
            let python =
                found.parakeet.as_deref().ok_or(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Program))?;
            let model = found
                .parakeet_model
                .as_deref()
                .ok_or(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model))?;
            Ok(Box::new(ParakeetPython::start(python, model, found.parakeet_int8)?))
        }
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

/// Piper with a voice and a player: since issue #745 the core reads the answers aloud
/// (`assistant.speak_answer`), so the listener plays only the tone of the wake word with it; without
/// a player nothing sounds at all.
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
        // Without a player the tone is left out (issue #747: the core reads the answers aloud).
        Piper::of(&found).chime();
    }

    /// Issue #749: the chosen speech to text is built, and what it misses is named.
    #[test]
    fn the_chosen_transcriber_names_what_it_misses() {
        let missing = |found: &Found| transcriber_of(found).err();
        let parakeet = Found::default();
        assert_eq!(missing(&parakeet), Some(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Program)));
        let without_model = Found { parakeet: Some(PathBuf::from("/usr/bin/python3")), ..Found::default() };
        assert_eq!(missing(&without_model), Some(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model)));
        let whisper =
            Found { recognizer: Recognizer::Whisper, whisper: Some("/bin/whisper-cli".into()), ..Found::default() };
        assert_eq!(missing(&whisper), Some(VoiceError::Missing(VoicePart::SpeechToText, VoiceMissing::Model)));
    }

    /// Counts the sentences it was given.
    struct Counts(Rc<RefCell<usize>>);

    impl Transcriber for Counts {
        fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
            *self.0.borrow_mut() += 1;
            Ok("heard".into())
        }
    }

    #[test]
    fn a_shared_transcriber_is_one() {
        let count = Rc::new(RefCell::new(0));
        let mut sentences = SharedTranscriber::new(Box::new(Counts(Rc::clone(&count))));
        let mut wake = sentences.clone();
        assert_eq!(wake.transcribe(&[1]).expect("text"), "heard");
        assert_eq!(sentences.transcribe(&[1]).expect("text"), "heard");
        assert_eq!(*count.borrow(), 2, "both went to the same transcriber");
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
        fn the_tone_plays_through_the_player() {
            let dir = tempfile::tempdir().expect("temp dir");
            let played = dir.path().join("played");
            // The player keeps what it played.
            let player = fake_program(dir.path(), "afplay", &format!("cp \"$1\" '{}'", played.display()));
            let mut speaker =
                Piper { piper: None, voice: None, player: Some(Player { program: player, powershell: false }) };
            speaker.chime();
            assert_eq!(fs::read(&played).expect("the tone"), wav_bytes(&chime()));
        }

        /// Issue #749: Parakeet's process gets the model folder and each sentence with its length
        /// in front, answers a line, and starts again after it stopped.
        #[test]
        fn parakeet_answers_a_line_per_sentence_and_starts_again() {
            let dir = tempfile::tempdir().expect("temp dir");
            let (args, head, starts) = (dir.path().join("args"), dir.path().join("head"), dir.path().join("starts"));
            // Answers one sentence, then ends: the next sentence finds it stopped.
            let body = format!(
                "echo \"$4 $5\" > '{}'; echo start >> '{}'; dd bs=4 count=1 of='{}' 2>/dev/null; echo ' Hallo Welt '",
                args.display(),
                starts.display(),
                head.display()
            );
            let python = fake_program(dir.path(), "python3", &body);
            let model = dir.path().join("parakeet-tdt-0.6b-v3");
            let mut parakeet = ParakeetPython::start(&python, &model, true).expect("starts");
            assert_eq!(parakeet.transcribe(&[]).expect("nothing said"), "");
            let samples = fixture("utterance.wav");
            assert_eq!(parakeet.transcribe(&samples).expect("text"), "Hallo Welt");
            assert_eq!(fs::read_to_string(&args).expect("arguments").trim(), format!("{} int8", model.display()));
            let count = u32::try_from(samples.len()).expect("fits");
            assert_eq!(fs::read(&head).expect("the length"), count.to_le_bytes());

            assert_eq!(parakeet.transcribe(&samples), Err(VoiceError::program("onnx-asr", "stopped")));
            assert_eq!(parakeet.transcribe(&samples).expect("started again"), "Hallo Welt");
            assert_eq!(fs::read_to_string(&starts).expect("starts").lines().count(), 2);
        }

        /// Review of #756: a model that does not load says why – the last line of Python's error
        /// output comes with the message.
        #[test]
        fn parakeet_names_why_it_stopped() {
            let dir = tempfile::tempdir().expect("temp dir");
            let body = "echo 'Traceback (most recent call last):' >&2; \
                        echo 'FileNotFoundError: encoder-model.onnx.data' >&2; exit 1";
            let python = fake_program(dir.path(), "python3", body);
            let mut parakeet = ParakeetPython::start(&python, dir.path(), false).expect("starts");
            assert_eq!(
                parakeet.transcribe(&fixture("utterance.wav")),
                Err(VoiceError::program("onnx-asr", "stopped: FileNotFoundError: encoder-model.onnx.data"))
            );
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
