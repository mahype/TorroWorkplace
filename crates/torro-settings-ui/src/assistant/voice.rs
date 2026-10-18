//! Assistant › Voice (task S.17; it was the group "Voice" of the control centre's Assistant page,
//! task C.9, concept `docs/assistent.md`, "Sprache"): talking to the assistant by voice.
//!
//! Talking by voice on or off (`settings.set_setting voice_enabled`), the fixed wake word, "Speak
//! answers", the microphone when the recording program can list them, and below the four programs
//! the listener needs with whether each was found (`settings.voice_status`).
//!
//! Everything runs on this computer. Switching it on is a person's switch (point 6 of "Was nie nach
//! außen geht" in `docs/api.md`): only here and in the tray, never through an agent.
//!
//! How it stands comes with [`Data::voice`], which the host asks for with every load – also after
//! every change, so the listener's state follows by itself. "Installed something?" asks again
//! without changing anything: an [`crate::Action::Send`] with no request is a load.
//!
//! "Say something" ([`Message::TestVoice`]) is the one thing here whose answer is the point:
//! `settings.test_voice` answers with what was understood. It goes out as [`crate::Action::Ask`],
//! and the app hands the answer back to [`answered`], which puts it under the row ([`Heard`]).

use iced::Element;
use iced::widget::{Space, column, toggler};

use torro_core::i18n::{tr, trf};
use torro_gui::theme;
use torro_gui::widgets::{self, Status};
use torro_gui::{layout, style, typo};
use torro_ipc::{Request, Response, SettingKey, VoiceMissing, VoicePart, VoiceProgram, VoiceState, VoiceStatus};

use super::Action;
use crate::Data;
use crate::parts::{choice, setting_row};

/// Section state: what "Say something" is doing. Everything else the section shows comes from
/// [`Data`].
#[derive(Debug, Default)]
pub struct State {
    /// "Say something" is recording.
    testing: bool,
    /// What the last test heard.
    heard: Option<Heard>,
}

/// The outcome of "Say something".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// These words.
    Text(String),
    /// Nothing understood.
    Nothing,
    /// The test failed, with the daemon's message.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// "Talk to the assistant" on or off.
    Enabled(bool),
    /// "Speak answers" on or off.
    SpeakAnswers(bool),
    /// A microphone's row; `None` for the system's default.
    Microphone(Option<String>),
    /// "Installed something?" / "Check again": ask how voice stands again.
    Check,
    /// "Say something": record once and show what was understood.
    TestVoice,
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Enabled(on) => {
            // Only switching on needs the programs; off is always possible.
            if on && data.voice.as_ref().is_some_and(|status| !status.can_listen()) {
                return Action::None;
            }
            set(SettingKey::VoiceEnabled, serde_json::Value::Bool(on))
        }
        Message::SpeakAnswers(speak) => set(SettingKey::SpeakAnswers, serde_json::Value::Bool(speak)),
        Message::Microphone(name) => {
            set(SettingKey::Microphone, name.map_or(serde_json::Value::Null, serde_json::Value::String))
        }
        // Nothing to send: the host loads the sections' data again, the status with it.
        Message::Check => Action::Send(Vec::new()),
        Message::TestVoice => {
            if state.testing {
                return Action::None;
            }
            state.testing = true;
            state.heard = None;
            // The answer is the point, so it comes back to this section ([`answered`]).
            Action::Ask(Request::TestVoice)
        }
    }
}

/// The answer to this section's [`crate::Action::Ask`], handed over by the app: what "Say
/// something" heard, or why it failed. Anything else is not this section's and changes nothing.
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    match result {
        Ok(Response::Transcript { text }) => {
            state.testing = false;
            state.heard =
                Some(if text.trim().is_empty() { Heard::Nothing } else { Heard::Text(text.trim().to_string()) });
        }
        Err(message) if state.testing => {
            state.testing = false;
            state.heard = Some(Heard::Failed(message.clone()));
        }
        _ => {}
    }
}

/// One voice setting; the host loads again afterwards, so the status follows.
fn set(key: SettingKey, value: serde_json::Value) -> Action {
    Action::send(Request::SetSetting { key, value })
}

// ----- Views --------------------------------------------------------------------------------

/// Voice, the microphone where one can be named, and the programs on this computer.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    if !super::is_on(data) {
        return super::off_group(tr("Voice"));
    }
    let mut page = column![voice_group(state, data)].spacing(layout::section_gap());
    page = page.extend(microphone_group(data));
    page.push(programs_group(data)).into()
}

/// "Voice": on or off with what the listener does, the wake word, "Speak answers" and the test.
fn voice_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings.voice;
    let can_listen = data.voice.as_ref().is_none_or(VoiceStatus::can_listen);
    let (status, detail) = voice_state(settings.enabled, data.voice.as_ref());
    let switch = toggler(settings.enabled)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle_maybe((settings.enabled || can_listen).then_some(Message::Enabled));
    let on = setting_row(Some(widgets::status_dot(status)), tr("Talk to the assistant"), Some(detail), switch);
    let word = typo::text(typo::body(), tr("Hey Torro")).style(theme::text_2);
    let wake = setting_row(None, tr("Wake word"), Some(tr("Your own wake word comes later").to_string()), word);
    let speak =
        toggler(settings.speak_answers).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::SpeakAnswers);
    let speak =
        setting_row(None, tr("Speak answers"), Some(tr("Read aloud with Piper as well as shown").to_string()), speak);
    let test_button = match state.testing {
        true => widgets::busy_button(tr("Listening…"), style::button::secondary),
        false => widgets::text_button(
            None,
            tr("Say something"),
            style::button::secondary,
            can_listen.then_some(Message::TestVoice),
        ),
    };
    let test = setting_row(None, tr("Test the microphone"), Some(heard_text(state)), test_button);
    let footer = tr(
        "Say “Hey Torro” and your question. Everything stays on this computer. Approvals are only given with a click, never by voice.",
    );
    widgets::group(tr("Voice"), vec![on, wake, speak, test], Some(footer.to_string()))
}

/// "Microphone": the system's default and every microphone the recording program lists, a check
/// at the chosen one. None when it lists none.
fn microphone_group(data: &Data) -> Option<Element<'_, Message>> {
    let microphones = &data.voice.as_ref()?.microphones;
    if microphones.is_empty() {
        return None;
    }
    let chosen = data.settings.voice.microphone.as_deref();
    let mut rows = vec![choice(tr("System default"), chosen.is_none(), Message::Microphone(None))];
    rows.extend(
        microphones
            .iter()
            .map(|name| choice(name.as_str(), chosen == Some(name.as_str()), Message::Microphone(Some(name.clone())))),
    );
    Some(widgets::group(tr("Microphone"), rows, None))
}

/// "Programs on this computer": the four parts with whether each was found, and the way to ask
/// again.
fn programs_group(data: &Data) -> Element<'_, Message> {
    let again = widgets::text_button(None, tr("Check again"), style::button::ghost, Some(Message::Check));
    let rows = match &data.voice {
        None => vec![setting_row(None, tr("The programs could not be checked"), None, again)],
        Some(status) if !status.installed => vec![setting_row(
            Some(widgets::status_dot(Status::Error)),
            tr("The assistant app is not installed"),
            Some(tr("Install torroassistant next to Torro Workplace").to_string()),
            again,
        )],
        Some(status) => {
            let mut rows: Vec<Element<'_, Message>> = status.programs.iter().map(program_row).collect();
            rows.push(setting_row(None, tr("Installed something?"), None, again));
            rows
        }
    };
    let footer = match data.voice.as_ref().and_then(|status| status.folder.as_deref()) {
        Some(folder) => {
            trf("Everything runs on this computer. Models and voices go into {folder}.", &[("folder", &folder)])
        }
        None => tr("Everything runs on this computer.").to_string(),
    };
    widgets::group(tr("Programs on this computer"), rows, Some(footer))
}

/// One part: its name, the program found or what to install, and a dot.
fn program_row(program: &VoiceProgram) -> Element<'_, Message> {
    let (status, detail) = program_state(program);
    setting_row(Some(widgets::status_dot(status)), part_label(program.part), Some(detail), Space::new())
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The line under "Test the microphone": what was heard, or how to test.
pub fn heard_text(state: &State) -> String {
    if state.testing {
        return tr("Speak now, it stops at a pause").to_string();
    }
    match &state.heard {
        None => tr("Shows what the assistant understands").to_string(),
        Some(Heard::Text(text)) => trf("Heard: “{text}”", &[("text", text)]),
        Some(Heard::Nothing) => tr("Nothing understood. Speak a little louder or closer.").to_string(),
        Some(Heard::Failed(_)) => tr("The test failed. Check the programs below.").to_string(),
    }
}

/// The dot and the line under "Talk to the assistant": off, or what the listener does.
pub fn voice_state(enabled: bool, status: Option<&VoiceStatus>) -> (Status, String) {
    let Some(status) = status else {
        return (Status::Inactive, if enabled { tr("On") } else { tr("Off") }.to_string());
    };
    if !status.installed {
        return (Status::Error, tr("The assistant app is not installed").to_string());
    }
    if !enabled {
        let detail = if status.can_listen() { tr("Off") } else { tr("Install the programs below first") };
        return (Status::Inactive, detail.to_string());
    }
    match (status.listener, &status.problem) {
        (VoiceState::Listening, _) => (Status::Ok, tr("Listening for “Hey Torro”").to_string()),
        (VoiceState::Recording, _) => (Status::Ok, tr("Recording …").to_string()),
        (VoiceState::Answering, _) => (Status::Ok, tr("Answering …").to_string()),
        (VoiceState::Off, Some(_)) => {
            (Status::Error, tr("The listener stopped. Check the programs below.").to_string())
        }
        (VoiceState::Off, None) => (Status::Attention, tr("Starting …").to_string()),
    }
}

/// "Speech to text", …
pub fn part_label(part: VoicePart) -> &'static str {
    match part {
        VoicePart::WakeWord => tr("Wake word"),
        VoicePart::Recorder => tr("Recording"),
        VoicePart::SpeechToText => tr("Speech to text"),
        VoicePart::Speech => tr("Speech output"),
    }
}

/// The dot and the line of a part: the program found, or how to get what is missing. The wake word
/// and speech are optional (speech to text stands in for the one, the window for the other), so
/// they are only orange when missing.
pub fn program_state(program: &VoiceProgram) -> (Status, String) {
    let optional = matches!(program.part, VoicePart::WakeWord | VoicePart::Speech);
    let Some(missing) = program.missing else {
        return (Status::Ok, program.program.clone().unwrap_or_default());
    };
    let hint = match (program.part, missing) {
        (VoicePart::WakeWord, VoiceMissing::Program) => {
            tr("Optional: pip install openwakeword. Until then speech to text listens for the wake word.")
        }
        (VoicePart::WakeWord, _) => {
            tr("Put hey_torro.onnx into the voice folder. Until then speech to text listens for the wake word.")
        }
        (VoicePart::Recorder, _) => recorder_hint(),
        (VoicePart::SpeechToText, VoiceMissing::Program) => tr("Install whisper.cpp, so that whisper-cli is found"),
        (VoicePart::SpeechToText, _) => tr("Put a whisper.cpp model such as ggml-base.bin into the voice folder"),
        (VoicePart::Speech, VoiceMissing::Program) => tr("Optional: install Piper to hear the answers"),
        (VoicePart::Speech, VoiceMissing::Model) => {
            tr("Put a Piper voice (.onnx with its .json) into the voice folder")
        }
        (VoicePart::Speech, VoiceMissing::Player) => tr("Install a program that plays sound, such as paplay"),
    };
    (if optional { Status::Attention } else { Status::Error }, hint.to_string())
}

/// How to get a recording program on this system.
fn recorder_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        tr("Install SoX (brew install sox) or ffmpeg")
    } else if cfg!(windows) {
        tr("Install SoX or ffmpeg")
    } else {
        tr("Install pulseaudio-utils (parec) or alsa-utils (arecord)")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    fn status(listener: VoiceState, missing: Option<VoiceMissing>) -> VoiceStatus {
        let part = |part| VoiceProgram { part, program: Some("x".into()), missing: None };
        VoiceStatus {
            listener,
            installed: true,
            problem: None,
            programs: vec![
                VoiceProgram { missing: Some(VoiceMissing::Program), program: None, ..part(VoicePart::WakeWord) },
                part(VoicePart::Recorder),
                VoiceProgram { missing, ..part(VoicePart::SpeechToText) },
                part(VoicePart::Speech),
            ],
            microphones: vec!["alsa_input.usb".into()],
            folder: Some("/home/anna/.local/share/torroassistant/voice".into()),
        }
    }

    fn sent(action: Action) -> (SettingKey, serde_json::Value) {
        match action {
            Act::Send(requests) => match requests.into_iter().next() {
                Some(Request::SetSetting { key, value }) => (key, value),
                other => panic!("expected settings.set_setting, got {other:?}"),
            },
            other => panic!("expected a request, got {other:?}"),
        }
    }

    /// Task C.9: the switch, "Speak answers" and the microphone send `settings.set_setting`.
    #[tokio::test]
    async fn voice_settings_send_set_setting() {
        let data = data().await;
        let mut state = State::default();
        assert_eq!(
            sent(update(&mut state, Message::Enabled(true), &data)),
            (SettingKey::VoiceEnabled, serde_json::json!(true))
        );
        assert_eq!(
            sent(update(&mut state, Message::Enabled(false), &data)),
            (SettingKey::VoiceEnabled, serde_json::json!(false))
        );
        assert_eq!(
            sent(update(&mut state, Message::SpeakAnswers(false), &data)),
            (SettingKey::SpeakAnswers, serde_json::json!(false))
        );
        assert_eq!(
            sent(update(&mut state, Message::Microphone(Some("alsa_input.usb".into())), &data)),
            (SettingKey::Microphone, serde_json::json!("alsa_input.usb"))
        );
        assert_eq!(
            sent(update(&mut state, Message::Microphone(None), &data)),
            (SettingKey::Microphone, serde_json::Value::Null)
        );
        // "Check again" sends nothing; the host loads the status again.
        assert_eq!(update(&mut state, Message::Check, &data), Act::Send(Vec::new()));
    }

    /// Task C.9: "Say something" asks for one answer and says what it heard, whatever came.
    #[tokio::test]
    async fn say_something_shows_what_was_heard() {
        let data = data().await;
        let mut state = State::default();
        assert_eq!(update(&mut state, Message::TestVoice, &data), Act::Ask(Request::TestVoice));
        assert!(matches!(update(&mut state, Message::TestVoice, &data), Act::None), "one test at a time");
        assert_eq!(heard_text(&state), tr("Speak now, it stops at a pause"));
        answered(&mut state, &Ok(Response::Transcript { text: " What is on today? ".into() }));
        assert_eq!(heard_text(&state), trf("Heard: “{text}”", &[("text", &"What is on today?")]));
        assert!(!state.testing, "the answer ends the test");

        let _ = update(&mut state, Message::TestVoice, &data);
        answered(&mut state, &Ok(Response::Transcript { text: String::new() }));
        assert_eq!(heard_text(&state), tr("Nothing understood. Speak a little louder or closer."));

        let _ = update(&mut state, Message::TestVoice, &data);
        answered(&mut state, &Err("no recorder".into()));
        assert_eq!(heard_text(&state), tr("The test failed. Check the programs below."));

        // An answer that is not this section's leaves it alone.
        let before = heard_text(&state);
        answered(&mut state, &Ok(Response::Ok));
        answered(&mut state, &Err("offline".into()));
        assert_eq!(heard_text(&state), before, "nothing was being tested");
    }

    /// Without the programs voice cannot be switched on; off always works.
    #[tokio::test]
    async fn voice_needs_the_programs_to_switch_on() {
        let missing = status(VoiceState::Off, Some(VoiceMissing::Model));
        assert!(!missing.can_listen());
        let data = Data { voice: Some(missing.clone()), ..data().await };
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::Enabled(true), &data), Act::None));
        assert!(matches!(update(&mut state, Message::Enabled(false), &data), Act::Send(_)));
        assert_eq!(voice_state(false, Some(&missing)).1, tr("Install the programs below first"));
    }

    #[test]
    fn voice_states_and_programs_read_as_words() {
        let listening = status(VoiceState::Listening, None);
        assert_eq!(voice_state(true, Some(&listening)), (Status::Ok, tr("Listening for “Hey Torro”").to_string()));
        assert_eq!(voice_state(true, Some(&status(VoiceState::Recording, None))).1, tr("Recording …"));
        assert_eq!(voice_state(true, Some(&status(VoiceState::Off, None))).0, Status::Attention);
        let stopped = VoiceStatus { problem: Some("the listener stopped".into()), ..status(VoiceState::Off, None) };
        assert_eq!(voice_state(true, Some(&stopped)).0, Status::Error);
        let gone = VoiceStatus { installed: false, ..VoiceStatus::default() };
        assert_eq!(voice_state(false, Some(&gone)).1, tr("The assistant app is not installed"));
        assert_eq!(voice_state(false, None).1, tr("Off"));
        assert_eq!(voice_state(true, None).1, tr("On"));

        let program = |part, missing| VoiceProgram { part, program: Some("whisper-cli".into()), missing };
        assert_eq!(program_state(&program(VoicePart::SpeechToText, None)), (Status::Ok, "whisper-cli".to_string()));
        assert_eq!(program_state(&program(VoicePart::SpeechToText, Some(VoiceMissing::Model))).0, Status::Error);
        assert_eq!(program_state(&program(VoicePart::WakeWord, Some(VoiceMissing::Program))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::WakeWord, Some(VoiceMissing::Model))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::Speech, Some(VoiceMissing::Player))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::Speech, Some(VoiceMissing::Program))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::Speech, Some(VoiceMissing::Model))).0, Status::Attention);
        assert_eq!(program_state(&program(VoicePart::Recorder, Some(VoiceMissing::Program))).0, Status::Error);
        assert_eq!(part_label(VoicePart::SpeechToText), tr("Speech to text"));
    }

    /// The section builds with the status known, unknown, the app missing, while a test runs and
    /// while the assistant is off.
    #[tokio::test]
    async fn the_section_builds_in_every_state() {
        let data = data().await;
        let mut on = data.clone();
        on.settings.voice.enabled = true;
        let mut testing = State::default();
        let _ = update(&mut testing, Message::TestVoice, &data);
        let heard = State { testing: false, heard: Some(Heard::Text("What is on today?".into())) };
        for state in [State::default(), testing, heard] {
            for voice in [
                Some(status(VoiceState::Listening, None)),
                Some(VoiceStatus::default()),
                Some(VoiceStatus { microphones: Vec::new(), ..status(VoiceState::Off, None) }),
                None,
            ] {
                let _ = view(&state, &Data { voice: voice.clone(), ..on.clone() });
                let _ = view(&state, &Data { voice, ..data.clone() });
            }
            let _ = view(&state, &turned_off(&data));
        }
    }
}
