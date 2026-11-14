//! Assistant › Voice (task S.17; it was the group "Voice" of the control centre's Assistant page,
//! task C.9, concept `docs/assistent.md`, "Sprache"): talking to the assistant by voice.
//!
//! Talking by voice on or off (`settings.set_setting voice_enabled`), the fixed wake word, "Speak
//! answers", the speech recognition – Parakeet (recommended) or Whisper, a dropdown as well
//! (`settings.set_setting speech_recognizer`, issue #749) –, the microphone when the recording
//! program can list them – a Torro dropdown in the group "Conversation" (until issue #763 "Voice")
//! since issue #547, as the number of microphones is open ([`State::menus`], [`overlay`]) –, and the
//! four programs the listener needs with whether each was found (`settings.voice_status`).
//!
//! Since issue #763 each of the three groups is a tab of its own ([`VoiceTab`], the row drawn by
//! [`super::page`]): Read aloud, Conversation, Programs, so the page fits the window.
//!
//! Above them "Read aloud" (issue #745, `settings.speech_status`): the switch of the chat windows,
//! the provider and its voice from a dropdown each with the recommended one marked, whether this
//! computer is fast enough for local voices, how to set up Qwen3-TTS (with "Copy"), and "Preview",
//! which reads a sentence ([`Message::Preview`], an [`crate::Action::Ask`] of `assistant.speak`).
//!
//! Everything runs on this computer. Switching it on is a person's switch (point 6 of "Was nie nach
//! außen geht" in `docs/api.md`): only here and in the tray, never through an agent.
//!
//! How it stands comes with [`Data::voice`], which the host asks for with every load – also after
//! every change, so the listener's state follows by itself. "Installed something?" asks again
//! without changing anything: an [`crate::Action::Send`] with no request is a load.
//!
//! One load is not enough right after voice was switched: the daemon starts (or stops) the
//! listener, which takes a moment, so that load still finds it off and the row would read
//! "Starting …" for ever. The section therefore asks again, up to [`VOICE_POLLS`] times every
//! [`VOICE_POLL`], until the listener reports what it does ([`subscription`], [`Message::Poll`]) –
//! the way the control centre's Assistant page did it before task S.17. A load that failed stops
//! the clock at once, as it did there: asking a daemon that does not answer only makes its message
//! blink.
//!
//! "Say something" ([`Message::TestVoice`]) is the one thing here whose answer is the point:
//! `settings.test_voice` answers with what was understood. It goes out as [`crate::Action::Ask`],
//! and the app hands the answer back to [`answered`], which puts it under the row ([`Heard`]).

use std::time::Duration;

use iced::widget::{Space, toggler};
use iced::{Element, Subscription};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Recognizer, SpeechProvider};
use torro_gui::theme;
use torro_gui::widgets::{self, Align, DROPDOWN_SEARCH, Dropdown, DropdownEvent, MenuItem, MenuSection, Status};
use torro_gui::{layout, style, typo};
use torro_ipc::{
    Request, Response, SettingKey, SpeechMissing, SpeechStatus, VoiceMissing, VoicePart, VoiceProgram, VoiceState,
    VoiceStatus,
};

use super::{Action, VoiceTab};
use crate::Data;
use crate::parts::setting_row;

/// How long between two asks while the listener has not followed what was switched.
const VOICE_POLL: Duration = Duration::from_secs(2);

/// How many times the status is asked after voice was switched, until the listener says it runs
/// (or stopped): five times two seconds, as the control centre had it before task S.17. After that
/// the row shows what last came, and "Installed something?" asks by hand.
const VOICE_POLLS: u8 = 5;

/// The dropdowns of the page (issue #547).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// "Speech recognition": Parakeet or Whisper (issue #749).
    Recognizer,
    /// "Microphone": the system's default or one the recording program lists.
    Microphone,
    /// "Provider" of reading aloud (issue #745).
    Provider,
    /// "Voice" of the provider that reads (issue #745).
    SpeechVoice,
}

/// Section state: what "Say something" is doing, what the listener is still expected to follow, and
/// whether the dropdown of the microphone is open. Everything else the section shows comes from
/// [`Data`].
#[derive(Debug, Default)]
pub struct State {
    /// Which dropdown is open (issue #547).
    pub menus: Dropdown<Choice>,
    /// "Say something" is recording.
    testing: bool,
    /// What the last test heard.
    heard: Option<Heard>,
    /// Whether voice was last switched on (`Some(true)`) or off, until the listener follows;
    /// `None` while nothing is waited for.
    expected: Option<bool>,
    /// How many more times the status is asked while the listener has not followed.
    polls: u8,
    /// "Preview" reads its sentence (issue #745).
    previewing: bool,
    /// Why the last preview could not read, from the core.
    preview_failed: Option<String>,
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
    /// The dropdown of the microphone opened, closed or was searched; changes nothing.
    Menus(DropdownEvent<Choice>),
    /// "Talk to the assistant" on or off.
    Enabled(bool),
    /// "Speak answers" on or off.
    SpeakAnswers(bool),
    /// An entry of the dropdown "Speech recognition" (issue #749).
    Recognizer(Recognizer),
    /// An entry of the dropdown "Microphone"; `None` for the system's default.
    Microphone(Option<String>),
    /// "Installed something?" / "Check again": ask how voice stands again.
    Check,
    /// The clock of [`subscription`] after voice was switched: ask how it stands again, until the
    /// listener reports what it does.
    Poll,
    /// "Say something": record once and show what was understood.
    TestVoice,
    /// "Read answers aloud" on or off (issue #745).
    ReadAloud(bool),
    /// An entry of the dropdown "Provider".
    Provider(SpeechProvider),
    /// An entry of the dropdown "Voice": the voice's id for the provider.
    SpeechVoice(SpeechProvider, String),
    /// "Preview": read a sentence with the provider and voice chosen.
    Preview,
    /// "Stop" while the preview reads.
    StopPreview,
    /// "Copy" beside the command that sets up Qwen3-TTS.
    CopySetup(String),
}

/// The sentence "Preview" reads.
pub fn preview_text() -> &'static str {
    tr("Hello, I am your assistant. This is how I sound.")
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        // Opening the dropdown asks for the focus of its search field (many microphones).
        Message::Menus(event) => match state.menus.update(event) {
            true => Action::Focus(DROPDOWN_SEARCH),
            false => Action::None,
        },
        Message::Enabled(on) => {
            // Only switching on needs the programs; off is always possible.
            if on && data.voice.as_ref().is_some_and(|status| !status.can_listen()) {
                return Action::None;
            }
            // The daemon starts (or stops) the listener, which takes a moment: the one load after
            // this change still finds it off. Ask again until it reports what it does, else the
            // row stays at "Starting …" ([`subscription`]).
            state.expected = Some(on);
            state.polls = VOICE_POLLS;
            set(SettingKey::VoiceEnabled, serde_json::Value::Bool(on))
        }
        Message::SpeakAnswers(speak) => set(SettingKey::SpeakAnswers, serde_json::Value::Bool(speak)),
        Message::Recognizer(recognizer) => {
            state.menus.close();
            set(SettingKey::SpeechRecognizer, serde_json::json!(recognizer))
        }
        Message::Microphone(name) => {
            state.menus.close();
            set(SettingKey::Microphone, name.map_or(serde_json::Value::Null, serde_json::Value::String))
        }
        // Nothing to send: the host loads the sections' data again, the status with it.
        Message::Check => Action::Send(Vec::new()),
        Message::Poll => {
            let Some(on) = state.expected else { return Action::None };
            // The daemon did not answer: asking again would only fail again, and every ask takes
            // the notice away and puts it back, which makes it blink. The control centre set
            // `polls = 0` on a failed status for the same reason (task C.9). "Installed
            // something?" asks again by hand once the daemon is back.
            if data.error.is_some() {
                state.expected = None;
                state.polls = 0;
                return Action::None;
            }
            // Followed, or asked often enough: let the clock stop and show what last came.
            if followed(on, data.voice.as_ref()) || state.polls == 0 {
                state.expected = None;
                state.polls = 0;
                return Action::None;
            }
            state.polls -= 1;
            Action::Send(Vec::new())
        }
        Message::ReadAloud(on) => set(SettingKey::ReadAloud, serde_json::Value::Bool(on)),
        Message::Provider(provider) => {
            state.menus.close();
            set(SettingKey::SpeechProvider, serde_json::Value::String(provider.id().to_string()))
        }
        Message::SpeechVoice(provider, id) => {
            state.menus.close();
            let key = match provider {
                SpeechProvider::Qwen => SettingKey::QwenVoice,
                SpeechProvider::OpenAi => SettingKey::OpenAiVoice,
                // Piper's voice is a file, chosen in the voice folder.
                SpeechProvider::Piper => return Action::None,
            };
            set(key, serde_json::Value::String(id))
        }
        Message::Preview => {
            if state.previewing {
                return Action::None;
            }
            state.previewing = true;
            state.preview_failed = None;
            Action::Ask(Request::Speak { text: preview_text().to_string() })
        }
        // The preview's own answer comes once it stopped ([`answered`]).
        Message::StopPreview => Action::send(Request::StopSpeaking),
        Message::CopySetup(command) => Action::Copy(command),
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
        // The preview read to its end or was stopped (issue #745).
        Ok(Response::Ok) if state.previewing => state.previewing = false,
        Err(message) if state.previewing && !state.testing => {
            state.previewing = false;
            state.preview_failed = Some(message.clone());
        }
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

/// The clock that asks how voice stands again, while the listener has not followed what was last
/// switched; none otherwise – a section at rest asks nothing. The app batches it into the window's
/// own subscription ([`crate::assistant::subscription`]).
pub fn subscription(state: &State) -> Subscription<Message> {
    match state.expected.is_some() && state.polls > 0 {
        true => iced::time::every(VOICE_POLL).map(|_| Message::Poll),
        false => Subscription::none(),
    }
}

/// Whether the listener has followed what was switched: switched on it is doing something, or says
/// what went wrong; switched off it is off. A status that has not come yet has followed nothing.
fn followed(on: bool, status: Option<&VoiceStatus>) -> bool {
    let Some(status) = status else { return false };
    match on {
        true => status.listener != VoiceState::Off || status.problem.is_some(),
        false => status.listener == VoiceState::Off,
    }
}

// ----- Views --------------------------------------------------------------------------------

/// The group of the tab on screen (issue #763): reading aloud, the conversation by voice with the
/// microphone where one can be named, or the programs on this computer. While the assistant is off,
/// the tab says so under its own name.
pub fn view<'a>(state: &'a State, data: &'a Data, tab: VoiceTab) -> Element<'a, Message> {
    if !super::is_on(data) {
        return super::off_group(tab.label());
    }
    match tab {
        VoiceTab::ReadAloud => read_aloud_group(state, data),
        VoiceTab::Conversation => voice_group(state, data),
        VoiceTab::Programs => programs_group(data),
    }
}

/// The open dropdown under its button: the two speech recognitions, Parakeet recommended, or the
/// system's default and every microphone the recording program lists; the chosen one marked.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let key = state.menus.open().filter(|_| super::is_on(data))?;
    if *key == Choice::Recognizer {
        let chosen = data.settings.voice.recognizer;
        let items = vec![
            MenuItem::plain(recognizer_name(Recognizer::Parakeet), Message::Recognizer(Recognizer::Parakeet))
                .detail(tr("Fast, runs on the processor"))
                .tag(tr("Recommended"), false)
                .current(chosen == Recognizer::Parakeet),
            MenuItem::plain(recognizer_name(Recognizer::Whisper), Message::Recognizer(Recognizer::Whisper))
                .detail(tr("whisper.cpp with a ggml model"))
                .current(chosen == Recognizer::Whisper),
        ];
        let sections = vec![MenuSection::new(items)];
        return Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus));
    }
    if *key != Choice::Microphone {
        let sections = vec![MenuSection::new(speech_items(*key, data))];
        return Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus));
    }
    let microphones = data.voice.as_ref().map(|voice| &voice.microphones).filter(|list| !list.is_empty())?;
    let chosen = data.settings.voice.microphone.as_deref();
    let default = MenuItem::plain(tr("System default"), Message::Microphone(None)).current(chosen.is_none());
    let items = std::iter::once(default)
        .chain(microphones.iter().map(|name| {
            MenuItem::plain(name.as_str(), Message::Microphone(Some(name.clone())))
                .current(chosen == Some(name.as_str()))
        }))
        .collect();
    let sections = vec![MenuSection::new(items)];
    Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus))
}

/// Fresh data came ([`super::received`]): with no microphone listed any more its dropdown closes, so
/// it neither takes the next Escape nor opens again by itself later. The speech recognition's stays
/// open: its two entries do not depend on what was found.
pub fn loaded(state: &mut State, data: &Data) {
    let no_microphones = data.voice.as_ref().is_none_or(|voice| voice.microphones.is_empty());
    if !super::is_on(data) || (no_microphones && state.menus.is_open(&Choice::Microphone)) {
        state.menus.close();
    }
}

/// Escape closes the open dropdown first.
pub fn escape(state: &State) -> Option<Message> {
    state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
}

/// The entries of the dropdown `key` of reading aloud: the providers, or the voices of the one that
/// reads, the recommended ones marked.
fn speech_items(key: Choice, data: &Data) -> Vec<MenuItem<'_, Message>> {
    let current = current_provider(data);
    match key {
        Choice::Provider => SpeechProvider::ALL
            .iter()
            .map(|&provider| {
                let recommended = data.speech.as_ref().is_some_and(|speech| speech.recommended == provider);
                MenuItem::plain(marked(provider_name(provider), recommended), Message::Provider(provider))
                    .current(provider == current)
            })
            .collect(),
        Choice::SpeechVoice => {
            let chosen = chosen_voice(data, current);
            voices_of(data, current)
                .into_iter()
                .map(|(id, recommended)| {
                    MenuItem::plain(marked(&voice_name(&id), recommended), Message::SpeechVoice(current, id.clone()))
                        .current(chosen == id)
                })
                .collect()
        }
        Choice::Recognizer | Choice::Microphone => Vec::new(),
    }
}

/// "Read aloud" (issue #745): the switch of the chat windows, the provider with whether this computer
/// is fast enough, the voice with "Preview", and how to set up Qwen3-TTS when it is missing.
fn read_aloud_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings.voice;
    let switch =
        toggler(settings.read_aloud).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::ReadAloud);
    let on = setting_row(None, tr("Read answers aloud"), Some(tr("In the chat windows").to_string()), switch);
    let current = current_provider(data);
    let label =
        marked(provider_name(current), data.speech.as_ref().is_some_and(|speech| speech.recommended == current));
    // The status came with the other voice data, or its check failed: then say so instead of
    // "Checking …" for ever.
    let detail = match (&data.speech, &data.voice) {
        (None, Some(_)) => tr("This computer could not be checked. Use Check again under Programs.").to_string(),
        (speech, _) => provider_detail(speech.as_ref(), current),
    };
    let provider =
        setting_row(None, tr("Provider"), Some(detail), state.menus.button(Choice::Provider, label, Message::Menus));
    let preview = match state.previewing {
        true => widgets::text_button(None, tr("Stop"), style::button::secondary, Some(Message::StopPreview)),
        false => widgets::text_button(None, tr("Preview"), style::button::secondary, Some(Message::Preview)),
    };
    let chosen = chosen_voice(data, current);
    let voices = voices_of(data, current);
    let recommended = voices.iter().any(|(id, recommended)| *recommended && *id == chosen);
    let picker: Element<'a, Message> = match current {
        // Piper speaks with the voice file found; it is chosen in the voice folder.
        // Kept short with "…", the whole name in the tooltip: Piper's file names run long.
        SpeechProvider::Piper => widgets::with_tooltip(
            typo::text(typo::body(), typo::ellipsize(&chosen, typo::body(), torro_gui::theme::size::sidebar()))
                .style(theme::text_2),
            chosen.clone(),
        ),
        _ => state.menus.button(Choice::SpeechVoice, marked(&voice_name(&chosen), recommended), Message::Menus),
    };
    let control = iced::widget::row![picker, preview].spacing(layout::item_gap()).align_y(iced::Alignment::Center);
    let detail = match &state.preview_failed {
        Some(message) => Some(message.clone()),
        None => voice_detail(data.speech.as_ref(), current),
    };
    // "Voice" as the sound of a speaker: in German the page itself is "Sprache".
    // A preview that failed says so with a dot, not only in words (#560).
    let dot = state.preview_failed.as_ref().map(|_| widgets::status_dot(Status::Error));
    let voice = setting_row(dot, trc("speaker", "Voice"), detail, control);
    let mut rows = vec![on, provider, voice];
    if let Some(command) = setup_command(data.speech.as_ref(), current) {
        let copy = widgets::text_button(None, tr("Copy"), style::button::secondary, Some(Message::CopySetup(command)));
        // The command is long (paths): "Copy" takes it, the line only says what to do with it.
        let how = tr("Run the command once in a terminal; Copy takes it.").to_string();
        rows.push(setting_row(None, tr("Set up Qwen3-TTS"), Some(how), copy));
    }
    let footer = tr("Local voices stay on this computer. With OpenAI the text of each answer goes to OpenAI.");
    widgets::group(tr("Read aloud"), rows, Some(footer.to_string()))
}

/// "Conversation": on or off with what the listener does, the wake word, "Speak answers", the speech
/// recognition, the microphone and the test. The page and this group were both "Voice" until issue
/// #763.
fn voice_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings.voice;
    let can_listen = data.voice.as_ref().is_none_or(VoiceStatus::can_listen);
    let (_, detail) = voice_state(settings.enabled, data.voice.as_ref());
    let switch = toggler(settings.enabled)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle_maybe((settings.enabled || can_listen).then_some(Message::Enabled));
    // No dot in front (design review of #547): the rows of the group keep one axis; the line under
    // it says how the listener stands.
    let on = setting_row(None, tr("Talk to the assistant"), Some(detail), switch);
    let word = typo::text(typo::body(), tr("Hey Torro")).style(theme::text_2);
    let wake = setting_row(None, tr("Wake word"), Some(tr("Your own wake word comes later").to_string()), word);
    let speak =
        toggler(settings.speak_answers).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::SpeakAnswers);
    let speak = setting_row(
        None,
        tr("Speak answers"),
        Some(tr("After “Hey Torro”, with the voice under Read aloud").to_string()),
        speak,
    );
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
    let recognizer =
        state.menus.button(Choice::Recognizer, recognizer_name(data.settings.voice.recognizer), Message::Menus);
    let recognizer = setting_row(None, tr("Speech recognition"), None, recognizer);
    let mut rows = vec![on, wake, speak, recognizer];
    rows.extend(microphone_row(state, data));
    rows.push(test);
    widgets::group(tr("Conversation"), rows, Some(footer.to_string()))
}

/// "Microphone" with its dropdown (issue #547): the system's default or the chosen one. None when
/// the recording program lists no microphone.
fn microphone_row<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let microphones = &data.voice.as_ref()?.microphones;
    if microphones.is_empty() {
        return None;
    }
    let label = data.settings.voice.microphone.clone().unwrap_or_else(|| tr("System default").to_string());
    let button = state.menus.button(Choice::Microphone, label, Message::Menus);
    Some(setting_row(None, tr("Microphone"), None, button))
}

/// "Programs on this computer": the four parts with whether each was found, and the way to ask
/// again.
fn programs_group(data: &Data) -> Element<'_, Message> {
    // Like "Say something": one look for the row actions of the page (design review of #547).
    let again = widgets::text_button(None, tr("Check again"), style::button::secondary, Some(Message::Check));
    let rows = match &data.voice {
        None => vec![setting_row(None, tr("The programs could not be checked"), None, again)],
        Some(status) if !status.installed => vec![setting_row(
            Some(widgets::status_dot(Status::Error)),
            tr("The assistant app is not installed"),
            Some(tr("Install torroassistant next to Torro Workplace").to_string()),
            again,
        )],
        Some(status) => {
            let recognizer = data.settings.voice.recognizer;
            let mut rows: Vec<Element<'_, Message>> =
                status.programs.iter().map(|program| program_row(program, recognizer)).collect();
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
fn program_row(program: &VoiceProgram, recognizer: Recognizer) -> Element<'_, Message> {
    let (status, detail) = program_state(program, recognizer);
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

/// The provider that reads now: the core's word (the chosen one, or its choice), else the chosen one,
/// else Piper.
pub fn current_provider(data: &Data) -> SpeechProvider {
    data.speech
        .as_ref()
        .map(|speech| speech.current)
        .or(data.settings.voice.speech_provider)
        .unwrap_or(SpeechProvider::Piper)
}

/// The name of `provider` in the dropdown.
pub fn provider_name(provider: SpeechProvider) -> &'static str {
    match provider {
        SpeechProvider::Qwen => tr("Local: Qwen3-TTS"),
        SpeechProvider::OpenAi => tr("Online: OpenAI"),
        SpeechProvider::Piper => tr("Local: Piper"),
    }
}

/// `name` with "(recommended)" when it is.
pub fn marked(name: &str, recommended: bool) -> String {
    match recommended {
        true => trf("{name} (recommended)", &[("name", &name)]),
        false => name.to_string(),
    }
}

/// The name of a voice: Qwen's templates in words, OpenAI's by their name, capitalised (Piper's
/// file name is shown as it is).
pub fn voice_name(id: &str) -> String {
    match id {
        "warm_woman" => tr("Warm woman").to_string(),
        "calm_man" => tr("Calm man").to_string(),
        _ => {
            let mut chars = id.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

/// The voices of `provider` with whether each is recommended; without a status Qwen's and OpenAI's
/// from the settings' lists.
fn voices_of(data: &Data, provider: SpeechProvider) -> Vec<(String, bool)> {
    use torro_core::settings::{OPENAI_RECOMMENDED, OPENAI_VOICES, QWEN_RECOMMENDED, QWEN_VOICES};
    if let Some(option) = data.speech.as_ref().and_then(|speech| speech.option(provider)) {
        return option.voices.iter().map(|voice| (voice.id.clone(), voice.recommended)).collect();
    }
    match provider {
        SpeechProvider::Qwen => QWEN_VOICES.iter().map(|id| (id.to_string(), *id == QWEN_RECOMMENDED)).collect(),
        SpeechProvider::OpenAi => {
            OPENAI_VOICES.iter().map(|id| (id.to_string(), OPENAI_RECOMMENDED.contains(id))).collect()
        }
        SpeechProvider::Piper => Vec::new(),
    }
}

/// The voice `provider` reads with: the chosen one, else the recommended one; for Piper the voice
/// found.
fn chosen_voice(data: &Data, provider: SpeechProvider) -> String {
    let voice = &data.settings.voice;
    match provider {
        SpeechProvider::Qwen => voice.qwen_voice_or_default().to_string(),
        SpeechProvider::OpenAi => voice.openai_voice_or_default().to_string(),
        SpeechProvider::Piper => {
            voices_of(data, provider).into_iter().next().map(|(id, _)| id).unwrap_or_else(|| tr("None").to_string())
        }
    }
}

/// The line under "Provider": whether this computer is fast enough for local voices, and what keeps
/// the provider chosen from reading.
pub fn provider_detail(speech: Option<&SpeechStatus>, current: SpeechProvider) -> String {
    let Some(speech) = speech else { return tr("Checking this computer…").to_string() };
    let fast = match (&speech.gpu, speech.fast_enough) {
        (Some(gpu), true) => trf("Fast enough for local voices · {gpu}", &[("gpu", gpu)]),
        (Some(gpu), false) => trf("Too slow for local voices, online voices are recommended · {gpu}", &[("gpu", gpu)]),
        (None, _) => tr("Local voices need an NVIDIA graphics card with at least 4 GB; online voices are recommended.")
            .to_string(),
    };
    match speech.option(current).and_then(|option| option.missing) {
        Some(missing) => format!("{fast} {}", missing_text(current, missing)),
        None => fast,
    }
}

/// What keeps `provider` from reading, for people.
pub fn missing_text(provider: SpeechProvider, missing: SpeechMissing) -> &'static str {
    match (provider, missing) {
        (_, SpeechMissing::Gpu) => tr("Qwen3-TTS needs an NVIDIA graphics card with at least 4 GB."),
        (SpeechProvider::Qwen, SpeechMissing::Program) => tr("Qwen3-TTS is not set up yet: run the command below."),
        (_, SpeechMissing::Program) => tr("Piper is not installed."),
        (_, SpeechMissing::Voice) => tr("Piper has no voice yet: put one into the voice folder."),
        (_, SpeechMissing::Player) => tr("No program to play sound was found."),
        (_, SpeechMissing::Account) => tr("Connect an OpenAI account in Settings › AI."),
        (_, SpeechMissing::App) => tr("The assistant app is not installed."),
    }
}

/// The line under "Voice": the model Qwen reads with.
fn voice_detail(speech: Option<&SpeechStatus>, current: SpeechProvider) -> Option<String> {
    let model = speech?.option(current)?.model.as_deref()?;
    Some(trf("Model: {model}", &[("model", &model)]))
}

/// The command that sets up Qwen3-TTS, while it reads and is not set up.
fn setup_command(speech: Option<&SpeechStatus>, current: SpeechProvider) -> Option<String> {
    let option = speech?.option(current).filter(|_| current == SpeechProvider::Qwen)?;
    option.setup.clone().filter(|_| option.missing == Some(SpeechMissing::Program))
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

/// The name of a speech recognition, on its button and in its dropdown: names of programs, not
/// translated.
pub fn recognizer_name(recognizer: Recognizer) -> &'static str {
    match recognizer {
        Recognizer::Parakeet => "Parakeet",
        Recognizer::Whisper => "Whisper",
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

/// The dot and the line of a part: the program found, or how to get what is missing – for speech to
/// text, of the chosen `recognizer`. The wake word and speech are optional (speech to text stands in
/// for the one, the window for the other), so they are only orange when missing.
pub fn program_state(program: &VoiceProgram, recognizer: Recognizer) -> (Status, String) {
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
        (VoicePart::SpeechToText, missing) => match (recognizer, missing) {
            (Recognizer::Parakeet, VoiceMissing::Program) => tr("Install onnx-asr: pipx install \"onnx-asr[cpu]\""),
            (Recognizer::Parakeet, _) => tr(
                "Put the folder parakeet-tdt-0.6b-v3 (Hugging Face: istupakov/parakeet-tdt-0.6b-v3-onnx) into the voice folder",
            ),
            (Recognizer::Whisper, VoiceMissing::Program) => tr("Install whisper.cpp, so that whisper-cli is found"),
            (Recognizer::Whisper, _) => tr("Put a whisper.cpp model such as ggml-base.bin into the voice folder"),
        },
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

    /// Task S.17: switching voice on asks again until the listener reports what it does. The
    /// daemon needs a moment to start it, so the one load of the change still finds it off – and
    /// the row would read "Starting …" for good (the review of task S.22 found it).
    #[tokio::test]
    async fn switching_voice_asks_again_until_the_listener_follows() {
        let starting = Data { voice: Some(status(VoiceState::Off, None)), ..data().await };
        let mut state = State::default();
        assert_eq!(subscription(&state).units(), 0, "a section at rest asks nothing");

        let _ = update(&mut state, Message::Enabled(true), &starting);
        assert_eq!(subscription(&state).units(), 1, "the clock runs");
        assert_eq!(voice_state(true, starting.voice.as_ref()).1, tr("Starting …"));
        // Still starting: ask once more.
        assert_eq!(update(&mut state, Message::Poll, &starting), Act::Send(Vec::new()));
        assert_eq!(subscription(&state).units(), 1);

        // The listener runs: the line says so and the clock stops.
        let listening = Data { voice: Some(status(VoiceState::Listening, None)), ..starting.clone() };
        assert!(matches!(update(&mut state, Message::Poll, &listening), Act::None));
        assert_eq!(subscription(&state).units(), 0, "nothing left to wait for");
        assert_eq!(voice_state(true, listening.voice.as_ref()).1, tr("Listening for “Hey Torro”"));
        assert!(matches!(update(&mut state, Message::Poll, &listening), Act::None), "and no clock of its own");

        // Switched off, it waits for the listener to be off.
        let _ = update(&mut state, Message::Enabled(false), &listening);
        assert_eq!(subscription(&state).units(), 1);
        assert!(matches!(update(&mut state, Message::Poll, &starting), Act::None), "off is off");
        assert_eq!(subscription(&state).units(), 0);
    }

    /// Found in the second review of #210: a load that failed stops the clock at once, the way
    /// the control centre set `polls = 0` on a failed status. Otherwise it would ask five times
    /// over ten seconds at a daemon that does not answer, and the message of every ask would take
    /// the notice away and put it back.
    #[tokio::test]
    async fn a_daemon_that_does_not_answer_stops_the_clock() {
        let starting = Data { voice: Some(status(VoiceState::Off, None)), ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Enabled(true), &starting);
        assert_eq!(subscription(&state).units(), 1);

        // The next load came back with nothing but the problem.
        let offline = Data { error: Some("the core is not running".into()), voice: None, ..starting.clone() };
        assert!(matches!(update(&mut state, Message::Poll, &offline), Act::None), "no second try");
        assert_eq!(subscription(&state).units(), 0, "and no clock");
        // By hand it still asks, for when the daemon is back.
        assert_eq!(update(&mut state, Message::Check, &offline), Act::Send(Vec::new()));
    }

    /// It does not ask for ever: after [`VOICE_POLLS`] tries the row shows what last came, and
    /// "Installed something?" asks by hand.
    #[tokio::test]
    async fn the_asking_stops_after_a_few_tries() {
        let starting = Data { voice: Some(status(VoiceState::Off, None)), ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Enabled(true), &starting);
        for left in 0..VOICE_POLLS {
            assert_eq!(update(&mut state, Message::Poll, &starting), Act::Send(Vec::new()), "try {left}");
        }
        assert!(matches!(update(&mut state, Message::Poll, &starting), Act::None), "enough");
        assert_eq!(subscription(&state).units(), 0);
        assert_eq!(update(&mut state, Message::Check, &starting), Act::Send(Vec::new()), "by hand, any time");
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
        let program_state = |program: &VoiceProgram| super::program_state(program, Recognizer::Whisper);
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

    /// Issue #749: "Speech recognition" is a dropdown of Parakeet and Whisper; a choice closes it and
    /// sends the setting. It stays open over a load, also without microphones.
    #[tokio::test]
    async fn the_speech_recognition_is_a_dropdown() {
        let data = Data { voice: Some(status(VoiceState::Listening, None)), ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Recognizer)), &data);
        assert!(overlay(&state, &data).is_some());
        let none = Data {
            voice: Some(VoiceStatus { microphones: Vec::new(), ..status(VoiceState::Off, None) }),
            ..data.clone()
        };
        loaded(&mut state, &none);
        assert!(state.menus.is_open(&Choice::Recognizer) && overlay(&state, &none).is_some());
        let _ = view(&state, &data);
        let chosen = update(&mut state, Message::Recognizer(Recognizer::Whisper), &data);
        assert_eq!(sent(chosen), (SettingKey::SpeechRecognizer, serde_json::json!("whisper")));
        assert_eq!(state.menus.open(), None, "the choice closes it");
        let chosen = update(&mut state, Message::Recognizer(Recognizer::Parakeet), &data);
        assert_eq!(sent(chosen), (SettingKey::SpeechRecognizer, serde_json::json!("parakeet")));
    }

    /// Issue #749: what is missing for speech to text is told for the chosen recognition.
    #[test]
    fn speech_to_text_hints_follow_the_recognizer() {
        let missing = |missing| VoiceProgram { part: VoicePart::SpeechToText, program: None, missing: Some(missing) };
        let hint = |recognizer, what| program_state(&missing(what), recognizer);
        assert_eq!(
            hint(Recognizer::Parakeet, VoiceMissing::Program),
            (Status::Error, tr("Install onnx-asr: pipx install \"onnx-asr[cpu]\"").to_string())
        );
        assert_eq!(
            hint(Recognizer::Parakeet, VoiceMissing::Model).1,
            tr(
                "Put the folder parakeet-tdt-0.6b-v3 (Hugging Face: istupakov/parakeet-tdt-0.6b-v3-onnx) into the voice folder"
            )
        );
        assert_eq!(
            hint(Recognizer::Whisper, VoiceMissing::Program).1,
            tr("Install whisper.cpp, so that whisper-cli is found")
        );
        let found = VoiceProgram { part: VoicePart::SpeechToText, program: Some("onnx-asr".into()), missing: None };
        assert_eq!(program_state(&found, Recognizer::Parakeet), (Status::Ok, "onnx-asr".to_string()));
    }

    /// Issue #547: the microphone is a dropdown in "Voice" – opening it asks for the focus, Escape
    /// closes it, a choice closes it and sends the setting; without microphones there is no row and
    /// no menu.
    #[tokio::test]
    async fn the_microphone_is_a_dropdown() {
        let data = Data { voice: Some(status(VoiceState::Listening, None)), ..data().await };
        let mut state = State::default();
        assert!(overlay(&state, &data).is_none() && escape(&state).is_none());
        let open = update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Microphone)), &data);
        assert_eq!(open, Act::Focus(DROPDOWN_SEARCH));
        assert!(overlay(&state, &data).is_some());
        assert_eq!(escape(&state), Some(Message::Menus(DropdownEvent::Close)));
        let _ = view(&state, &data, VoiceTab::Conversation);
        let chosen = update(&mut state, Message::Microphone(Some("alsa_input.usb".into())), &data);
        assert_eq!(sent(chosen), (SettingKey::Microphone, serde_json::json!("alsa_input.usb")));
        assert_eq!((state.menus.open(), escape(&state)), (None, None), "the choice closes it");
        // A tick of the clock leaves an open menu open.
        let _ = update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Microphone)), &data);
        let _ = update(&mut state, Message::Poll, &data);
        assert!(state.menus.open().is_some());
        let none =
            Data { voice: Some(VoiceStatus { microphones: Vec::new(), ..status(VoiceState::Off, None) }), ..data };
        assert!(microphone_row(&state, &none).is_none() && overlay(&state, &none).is_none());
        loaded(&mut state, &none);
        assert_eq!(escape(&state), None, "closed with the list gone");
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
        let heard = State { heard: Some(Heard::Text("What is on today?".into())), ..State::default() };
        for state in [State::default(), testing, heard] {
            for voice in [
                Some(status(VoiceState::Listening, None)),
                Some(VoiceStatus::default()),
                Some(VoiceStatus { microphones: Vec::new(), ..status(VoiceState::Off, None) }),
                None,
            ] {
                for tab in VoiceTab::ALL {
                    let _ = view(&state, &Data { voice: voice.clone(), ..on.clone() }, tab);
                    let _ = view(&state, &Data { voice: voice.clone(), ..data.clone() }, tab);
                }
            }
            for tab in VoiceTab::ALL {
                let _ = view(&state, &turned_off(&data), tab);
            }
        }
    }

    /// Issue #745: the switch, provider and voice each send their setting; the recommended entry is
    /// marked; Preview asks to read and Stop stops it; Qwen's setup is offered with Copy.
    #[tokio::test]
    async fn read_aloud_sends_its_settings_and_previews() {
        let data = data().await;
        let speech = data.speech.clone().expect("the demo answers the status");
        let mut state = State::default();
        assert_eq!(
            sent(update(&mut state, Message::ReadAloud(true), &data)),
            (SettingKey::ReadAloud, serde_json::json!(true))
        );
        assert_eq!(
            sent(update(&mut state, Message::Provider(SpeechProvider::OpenAi), &data)),
            (SettingKey::SpeechProvider, serde_json::json!("openai"))
        );
        assert_eq!(
            sent(update(&mut state, Message::SpeechVoice(SpeechProvider::OpenAi, "cedar".into()), &data)),
            (SettingKey::OpenAiVoice, serde_json::json!("cedar"))
        );
        assert_eq!(
            sent(update(&mut state, Message::SpeechVoice(SpeechProvider::Qwen, "calm_man".into()), &data)),
            (SettingKey::QwenVoice, serde_json::json!("calm_man"))
        );
        assert!(matches!(
            update(&mut state, Message::SpeechVoice(SpeechProvider::Piper, "x".into()), &data),
            Act::None
        ));

        assert_eq!(
            update(&mut state, Message::Preview, &data),
            Act::Ask(Request::Speak { text: preview_text().into() })
        );
        assert!(matches!(update(&mut state, Message::Preview, &data), Act::None), "one at a time");
        assert_eq!(update(&mut state, Message::StopPreview, &data), Act::Send(vec![Request::StopSpeaking]));
        answered(&mut state, &Ok(Response::Ok));
        assert!(!state.previewing);
        let _ = update(&mut state, Message::Preview, &data);
        answered(&mut state, &Err("No program to play sound was found.".into()));
        assert_eq!(state.preview_failed.as_deref(), Some("No program to play sound was found."));

        // A status that could not be checked says so, not "Checking …" for ever.
        let failed = Data { speech: None, ..data.clone() };
        let _ = view(&state, &failed, VoiceTab::ReadAloud);
        assert!(failed.voice.is_some());

        let providers = speech_items(Choice::Provider, &data);
        assert_eq!(providers.len(), 3);
        assert_eq!(
            marked(provider_name(SpeechProvider::Qwen), true),
            trf("{name} (recommended)", &[("name", &provider_name(SpeechProvider::Qwen))])
        );
        let qwen =
            Data { speech: Some(SpeechStatus { current: SpeechProvider::Qwen, ..speech.clone() }), ..data.clone() };
        assert_eq!(speech_items(Choice::SpeechVoice, &qwen).len(), 2);
        let openai =
            Data { speech: Some(SpeechStatus { current: SpeechProvider::OpenAi, ..speech.clone() }), ..data.clone() };
        assert_eq!(speech_items(Choice::SpeechVoice, &openai).len(), 13);
        assert!(
            setup_command(qwen.speech.as_ref(), SpeechProvider::Qwen)
                .is_some_and(|command| command.contains("faster-qwen3-tts"))
        );
        assert_eq!(setup_command(openai.speech.as_ref(), SpeechProvider::OpenAi), None);
        let Act::Copy(copied) = update(&mut state, Message::CopySetup("pip install x".into()), &data) else {
            panic!("copied")
        };
        assert_eq!(copied, "pip install x");
        assert_eq!(voice_name("warm_woman"), tr("Warm woman"));
        assert_eq!(voice_name("marin"), "Marin");
        for data in [&data, &qwen, &openai] {
            let _ = view(&state, data, VoiceTab::ReadAloud);
            for key in [Choice::Provider, Choice::SpeechVoice] {
                state.menus = Dropdown::default();
                let _ = update(&mut state, Message::Menus(DropdownEvent::Toggle(key)), data);
                assert!(overlay(&state, data).is_some(), "{key:?}");
            }
        }
    }

    /// Issue #745: the line under "Provider" says whether this computer is fast enough, and what the
    /// chosen provider lacks.
    #[test]
    fn the_provider_line_says_whether_local_voices_fit() {
        let status = |gpu: Option<&str>, fast_enough| SpeechStatus {
            gpu: gpu.map(str::to_string),
            fast_enough,
            recommended: SpeechProvider::Qwen,
            current: SpeechProvider::Qwen,
            providers: Vec::new(),
        };
        let fast = provider_detail(Some(&status(Some("RTX 4090, 24 GB"), true)), SpeechProvider::Qwen);
        assert_eq!(fast, trf("Fast enough for local voices · {gpu}", &[("gpu", &"RTX 4090, 24 GB")]));
        let slow = provider_detail(Some(&status(None, false)), SpeechProvider::Qwen);
        assert!(slow.starts_with(tr(
            "Local voices need an NVIDIA graphics card with at least 4 GB; online voices are recommended."
        )));
        assert_eq!(provider_detail(None, SpeechProvider::Piper), tr("Checking this computer…"));
        assert_eq!(
            missing_text(SpeechProvider::OpenAi, SpeechMissing::Account),
            tr("Connect an OpenAI account in Settings › AI.")
        );
    }
}
