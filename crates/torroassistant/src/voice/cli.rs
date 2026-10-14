//! The voice modes of `torroassistant`, each without a window:
//!
//! - `--listen`: the listener ([`super::listen::Listener`]) for as long as the setting
//!   `voice.enabled` stays on. The core starts it when voice is switched on and stops it when it
//!   is switched off. Each change of state goes to standard output as one JSON string per line
//!   (`"listening"`, `"recording"`, `"answering"`, `"off"`: [`VoiceState`]), which the core reads
//!   to show it in the tray. It exits 0 when voice went off, 1 when it could not listen.
//! - `--voice-status --voice <settings>`: what the listener would use, as one line of JSON (a
//!   [`Response::Voice`]), for `settings.voice_status`. The settings are `Settings::voice` as
//!   JSON, handed over by the core.
//! - `--voice-test --voice <settings>`: records one sentence and writes it as text (a
//!   [`Response::Transcript`], or a [`Response::Error`]), for `settings.test_voice`.

use std::io::Write;
use std::process::{Child, Command, ExitCode};
use std::time::Duration;

use torro_core::settings::Voice;
use torro_ipc::{Response, VoiceState, VoiceStatus};

use super::audio::{Endpoint, Limits, NoiseFloor};
use super::external::{CommandRecorder, Parts, WhisperCli};
use super::listen::{CoreAssistant, Ended, Listener};
use super::programs::{Finder, Found};
use super::{Recorder, Transcriber, VoiceError};
use crate::backend::Backend;

/// A voice mode from the start parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Listen,
    Status(Voice),
    Test(Voice),
}

impl Mode {
    /// The voice mode `args` ask for; `Ok(None)` when they ask for a window. `--voice-status` and
    /// `--voice-test` need `--voice` with the settings as JSON.
    pub fn parse(args: &[String]) -> Result<Option<Mode>, String> {
        let has = |flag: &str| args.iter().any(|arg| arg == flag);
        let voice = || -> Result<Voice, String> {
            let at = args.iter().position(|arg| arg == "--voice").ok_or("--voice <settings> is missing")?;
            let json = args.get(at + 1).ok_or("--voice needs the settings as JSON")?;
            serde_json::from_str(json).map_err(|err| format!("--voice: {err}"))
        };
        match (has("--listen"), has("--voice-status"), has("--voice-test")) {
            (false, false, false) => Ok(None),
            (true, false, false) => Ok(Some(Mode::Listen)),
            (false, true, false) => voice().map(|voice| Some(Mode::Status(voice))),
            (false, false, true) => voice().map(|voice| Some(Mode::Test(voice))),
            _ => Err("--listen, --voice-status and --voice-test cannot be used together".into()),
        }
    }

    /// Runs the mode to its end.
    pub fn run(self) -> ExitCode {
        match self {
            Mode::Listen => listen(),
            Mode::Status(voice) => answer(&status(&voice)),
            Mode::Test(voice) => answer(&test(&voice)),
        }
    }
}

/// Writes `response` as one line of JSON.
fn answer(response: &Response) -> ExitCode {
    match serde_json::to_string(response) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("torroassistant: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What the listener would use with `voice` ([`Response::Voice`]).
fn status(voice: &Voice) -> Response {
    let finder = Finder::system();
    let found = Found::detect(&finder, voice);
    Response::Voice {
        voice: VoiceStatus {
            listener: VoiceState::Off,
            installed: true,
            problem: None,
            programs: found.status(),
            microphones: found.microphones(),
            folder: finder.folder.map(|folder| folder.to_string_lossy().into_owned()),
        },
    }
}

/// One sentence from the microphone as text ([`Response::Transcript`]).
fn test(voice: &Voice) -> Response {
    let found = Found::detect(&Finder::system(), voice);
    let refused = |code: &str, err: VoiceError| Response::Error { code: code.into(), message: err.to_string() };
    let (mut recorder, mut whisper) =
        match (CommandRecorder::of(&found, voice.microphone.as_deref()), WhisperCli::of(&found)) {
            (Ok(recorder), Ok(whisper)) => (recorder, whisper),
            (Err(err), _) | (_, Err(err)) => return refused("unsupported", err),
        };
    match record_sentence(&mut recorder).and_then(|sentence| whisper.transcribe(&sentence)) {
        Ok(text) => Response::Transcript { text },
        Err(err) => refused("error", err),
    }
}

/// Records until a pause after speech, five seconds of nothing or 15 seconds.
fn record_sentence(recorder: &mut dyn Recorder) -> Result<Vec<i16>, VoiceError> {
    let mut stream = recorder.open()?;
    let mut end = Endpoint::new(NoiseFloor::default(), Limits::default());
    let mut sentence = Vec::new();
    while let Some(frame) = stream.frame()? {
        sentence.extend_from_slice(&frame);
        if end.push(&frame) {
            break;
        }
    }
    Ok(sentence)
}

/// How long to wait between two looks at a running answer.
const POLL: Duration = Duration::from_millis(500);

/// How long to follow an answer at most; the window shows the rest.
const ANSWER_WAIT: Duration = Duration::from_secs(120);

/// The listener for as long as voice is on (see the module).
fn listen() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("torroassistant: {err}");
            return ExitCode::FAILURE;
        }
    };
    let backend = Backend::core();
    let mut window = QuickWindow::default();
    let mut core = CoreAssistant::new(
        |request| runtime.block_on(backend.ask(request)),
        |conversation: &str| window.show(conversation),
        POLL,
        ANSWER_WAIT,
    );
    loop {
        let voice = match super::listen::Assistant::settings(&mut core) {
            Ok(voice) if voice.enabled => voice,
            Ok(_) => return stopped(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return stopped(ExitCode::FAILURE);
            }
        };
        let found = Found::detect(&Finder::system(), &voice);
        let parts = match Parts::build(&found, &voice) {
            Ok(parts) => parts,
            Err(err) => {
                eprintln!("torroassistant: cannot listen: {err}");
                return stopped(ExitCode::FAILURE);
            }
        };
        let mut listener = Listener {
            recorder: parts.recorder,
            wake: parts.wake,
            transcriber: parts.transcriber,
            speaker: parts.speaker,
            assistant: &mut core,
            report: Box::new(report),
            limits: Limits::default(),
            check_every: 2 * super::audio::FRAMES_PER_SECOND,
        };
        match listener.run(&voice) {
            Ok(Ended::Changed) => continue,
            Ok(Ended::RecordingStopped) => {
                eprintln!("torroassistant: the recording program stopped");
                return stopped(ExitCode::FAILURE);
            }
            Ok(_) => return stopped(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return stopped(ExitCode::FAILURE);
            }
        }
    }
}

/// Reports "off" and ends with `code`.
fn stopped(code: ExitCode) -> ExitCode {
    report(VoiceState::Off);
    code
}

/// Tells the core the new state, one JSON string per line. When the core no longer reads (it
/// ended), the listener ends too: nobody could switch the microphone off any more.
fn report(state: VoiceState) {
    let line = serde_json::to_string(&state).unwrap_or_default();
    let mut out = std::io::stdout();
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        std::process::exit(0);
    }
}

/// The chat window that shows spoken conversations: one at a time, opened again when it was
/// closed.
#[derive(Default)]
struct QuickWindow {
    child: Option<Child>,
}

impl QuickWindow {
    fn show(&mut self, conversation: &str) {
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(None))
        {
            return;
        }
        let program = std::env::current_exe().unwrap_or_else(|_| "torroassistant".into());
        match Command::new(program).args(["--chat", conversation]).spawn() {
            Ok(child) => self.child = Some(child),
            Err(err) => eprintln!("torroassistant: cannot open the window: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn modes_come_from_the_start_parameters() {
        assert_eq!(Mode::parse(&args(&[])), Ok(None));
        assert_eq!(Mode::parse(&args(&["--chat", "new"])), Ok(None));
        assert_eq!(Mode::parse(&args(&["--listen"])), Ok(Some(Mode::Listen)));
        let json = serde_json::to_string(&Voice { speak_answers: false, ..Voice::default() }).expect("json");
        assert_eq!(
            Mode::parse(&args(&["--voice-status", "--voice", &json])),
            Ok(Some(Mode::Status(Voice { speak_answers: false, ..Voice::default() })))
        );
        assert!(matches!(Mode::parse(&args(&["--voice-test", "--voice", "{}"])), Ok(Some(Mode::Test(_)))));
        assert!(Mode::parse(&args(&["--voice-test"])).is_err(), "the settings are needed");
        assert!(Mode::parse(&args(&["--voice-test", "--voice", "nope"])).is_err());
        assert!(Mode::parse(&args(&["--listen", "--voice-test"])).is_err());
    }

    #[test]
    fn states_are_written_as_json_strings() {
        assert_eq!(serde_json::to_string(&VoiceState::Listening).expect("json"), "\"listening\"");
    }
}
