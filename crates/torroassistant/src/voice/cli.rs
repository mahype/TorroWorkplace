//! The voice modes of `torroassistant`, each without a window:
//!
//! - `--listen`: the listener ([`super::listen::Listener`]) for as long as the setting
//!   `voice.enabled` stays on or a conversation by voice runs (issue #747). The core starts it when
//!   voice is switched on or a conversation by voice starts, and stops it when both are over. Each
//!   change of state goes to standard output as one JSON string per line (`"listening"`,
//!   `"recording"`, `"answering"`, `"conversing"`, `"off"`: [`VoiceState`]), which the core reads to
//!   show it in the tray. It exits 0 when voice went off, 1 when it could not listen.
//! - `--voice-status --voice <settings>`: what the listener would use, as one line of JSON (a
//!   [`Response::Voice`]), for `settings.voice_status`. The settings are `Settings::voice` as
//!   JSON, handed over by the core.
//! - `--voice-test --voice <settings>`: records one sentence and writes it as text (a
//!   [`Response::Transcript`], or a [`Response::Error`]), for `settings.test_voice`.
//! - `--dictate --voice <settings>`: records what is said until a pause of two seconds, the end of
//!   standard input (the core closes it for `assistant.stop_dictation`) or a minute, and writes it as
//!   text like `--voice-test`, for `assistant.dictate` (issue #699).
//! - `--speak --voice <settings>`: the helper that reads aloud for `assistant.speak` (issue #745,
//!   [`super::speech`]), as long as the core gives it work.
//! - `--speech-status --voice <settings>`: what is found for reading aloud, as one line of JSON (a
//!   [`Response::Speech`]), for `settings.speech_status`.

use std::io::{Read, Write};
use std::process::{Child, Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use torro_core::settings::Voice;
use torro_core::voice_models::voice_folder;
use torro_ipc::{Response, VoiceState};

use super::audio::{Endpoint, FRAMES_PER_SECOND, Limits, NoiseFloor};
use super::listen::{Assistant, CoreAssistant, Ended, Listener};
use super::parts::{Parts, voice_status};
use super::recognize::{SherpaTranscriber, require};
use super::sound::{CpalRecorder, SystemDevices};
use super::{Recorder, Transcriber, VoiceError};
use crate::backend::Backend;

/// A voice mode from the start parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Listen,
    Status(Voice),
    Test(Voice),
    Dictate(Voice),
    Speak(Voice),
    SpeechStatus(Voice),
}

impl Mode {
    /// The voice mode `args` ask for; `Ok(None)` when they ask for a window. `--voice-status`,
    /// `--voice-test` and `--dictate` need `--voice` with the settings as JSON.
    pub fn parse(args: &[String]) -> Result<Option<Mode>, String> {
        let has = |flag: &str| args.iter().any(|arg| arg == flag);
        let voice = || -> Result<Voice, String> {
            let at = args.iter().position(|arg| arg == "--voice").ok_or("--voice <settings> is missing")?;
            let json = args.get(at + 1).ok_or("--voice needs the settings as JSON")?;
            serde_json::from_str(json).map_err(|err| format!("--voice: {err}"))
        };
        let flags = ["--listen", "--voice-status", "--voice-test", "--dictate", "--speak", "--speech-status"];
        let given: Vec<&str> = flags.into_iter().filter(|flag| has(flag)).collect();
        match given.as_slice() {
            [] => Ok(None),
            ["--listen"] => Ok(Some(Mode::Listen)),
            ["--voice-status"] => voice().map(|voice| Some(Mode::Status(voice))),
            ["--voice-test"] => voice().map(|voice| Some(Mode::Test(voice))),
            ["--dictate"] => voice().map(|voice| Some(Mode::Dictate(voice))),
            ["--speak"] => voice().map(|voice| Some(Mode::Speak(voice))),
            ["--speech-status"] => voice().map(|voice| Some(Mode::SpeechStatus(voice))),
            _ => Err(format!("{} cannot be used together", flags.join(", "))),
        }
    }

    /// Runs the mode to its end.
    pub fn run(self) -> ExitCode {
        match self {
            Mode::Listen => listen(),
            Mode::Status(voice) => {
                answer(&Response::Voice { voice: voice_status(&voice, voice_folder().as_deref(), &SystemDevices) })
            }
            Mode::Test(voice) => answer(&test(&voice, Limits::default(), &AtomicBool::new(false))),
            Mode::Dictate(voice) => answer(&test(&voice, dictation(), &stop_at_end_of_input())),
            Mode::Speak(voice) => {
                super::speech::serve(voice);
                ExitCode::SUCCESS
            }
            Mode::SpeechStatus(voice) => answer(&Response::Speech { speech: super::speech::system_status(&voice) }),
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

/// How long a dictation records: five seconds to start, a pause of two seconds ends it (the one
/// second of a spoken question is too short for a thought), a minute at most.
fn dictation() -> Limits {
    Limits { wait: 5 * FRAMES_PER_SECOND, pause: 2 * FRAMES_PER_SECOND, most: 60 * FRAMES_PER_SECOND }
}

/// A flag that goes up when standard input ends: the core closes it to stop a dictation.
fn stop_at_end_of_input() -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        // Whatever the core writes is ignored; only the end counts, also as an error.
        let _ = std::io::stdin().read_to_end(&mut sink);
        flag.store(true, Ordering::SeqCst);
    });
    stop
}

/// What is said into the microphone as text ([`Response::Transcript`]), recorded within `limits`
/// or until `stop` goes up.
fn test(voice: &Voice, limits: Limits, stop: &AtomicBool) -> Response {
    let refused = |code: &str, err: VoiceError| Response::Error { code: code.into(), message: err.to_string() };
    let folder = voice_folder();
    if let Err(err) = require(voice.recognizer, folder.as_deref()) {
        return refused("unsupported", err);
    }
    // The model loads on a thread of its own while one speaks: loading takes seconds.
    let loading = {
        let (voice, folder) = (voice.clone(), folder.clone());
        std::thread::spawn(move || SherpaTranscriber::load(&voice, folder.as_deref()))
    };
    let mut recorder = CpalRecorder { microphone: voice.microphone.clone() };
    let sentence = match record_sentence(&mut recorder, limits, stop) {
        Ok(sentence) => sentence,
        Err(err @ VoiceError::Missing(..)) => return refused("unsupported", err),
        Err(err) => return refused("error", err),
    };
    let loaded = loading.join().unwrap_or_else(|_| Err(VoiceError::program("speech to text", "stopped")));
    match loaded.and_then(|mut transcriber| transcriber.transcribe(&sentence)) {
        Ok(text) => Response::Transcript { text },
        Err(err) => refused("error", err),
    }
}

/// Records until a pause after speech, nothing said for a while or the longest sentence
/// (`limits`), or until `stop` goes up; what was recorded until then counts.
fn record_sentence(recorder: &mut dyn Recorder, limits: Limits, stop: &AtomicBool) -> Result<Vec<i16>, VoiceError> {
    let mut stream = recorder.open()?;
    let mut end = Endpoint::new(NoiseFloor::default(), limits);
    let mut sentence = Vec::new();
    while let Some(frame) = stream.frame()? {
        sentence.extend_from_slice(&frame);
        if end.push(&frame) || stop.load(Ordering::SeqCst) {
            break;
        }
    }
    Ok(sentence)
}

/// How long to wait between two looks at a running answer.
const POLL: Duration = Duration::from_millis(500);

/// How long to follow an answer at most; the window shows the rest.
const ANSWER_WAIT: Duration = Duration::from_secs(120);

/// How long to wait between two looks while something is read aloud, and after it, so its echo
/// fades before the microphone opens again (issue #747).
const QUIET: Duration = Duration::from_millis(300);

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
        let talking = core.talk().is_ok_and(|now| now.talk.is_some());
        let voice = match core.settings() {
            Ok(voice) if voice.enabled || talking => voice,
            Ok(_) => return stopped(ExitCode::SUCCESS),
            Err(err) => {
                eprintln!("torroassistant: {err}");
                return stopped(ExitCode::FAILURE);
            }
        };
        let parts = match Parts::build(&voice, voice_folder().as_deref()) {
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
            // Issue #747: twice a second, so the microphone closes soon when something is read aloud.
            check_every: FRAMES_PER_SECOND / 2,
            pause: QUIET,
            floor: NoiseFloor::default(),
        };
        match listener.run(&voice) {
            Ok(Ended::Changed) => continue,
            Ok(Ended::RecordingStopped) => {
                eprintln!("torroassistant: the microphone stopped");
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

    /// Issue #699: dictating is a mode of its own with the settings, never together with another.
    #[test]
    fn dictating_comes_from_the_start_parameters() {
        assert!(matches!(Mode::parse(&args(&["--dictate", "--voice", "{}"])), Ok(Some(Mode::Dictate(_)))));
        assert!(Mode::parse(&args(&["--dictate"])).is_err(), "the settings are needed");
        assert!(Mode::parse(&args(&["--dictate", "--voice-test", "--voice", "{}"])).is_err());
        assert!(Mode::parse(&args(&["--dictate", "--listen"])).is_err());
    }

    /// Issue #745: the helper that reads aloud and its status are modes with the settings.
    #[test]
    fn reading_aloud_comes_from_the_start_parameters() {
        assert!(matches!(Mode::parse(&args(&["--speak", "--voice", "{}"])), Ok(Some(Mode::Speak(_)))));
        assert!(matches!(Mode::parse(&args(&["--speech-status", "--voice", "{}"])), Ok(Some(Mode::SpeechStatus(_)))));
        assert!(Mode::parse(&args(&["--speak"])).is_err(), "the settings are needed");
        assert!(Mode::parse(&args(&["--speak", "--listen"])).is_err());
        let limits = dictation();
        assert_eq!(limits.pause, 2 * FRAMES_PER_SECOND);
        assert_eq!(limits.most, 60 * FRAMES_PER_SECOND);
    }

    #[test]
    fn states_are_written_as_json_strings() {
        assert_eq!(serde_json::to_string(&VoiceState::Listening).expect("json"), "\"listening\"");
    }
}
