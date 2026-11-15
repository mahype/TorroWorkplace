//! Assistant › Voice (task S.17; it was the group "Voice" of the control centre's Assistant page,
//! task C.9, concept `docs/assistent.md`, "Sprache"): talking to the assistant by voice.
//!
//! Talking by voice on or off (`settings.set_setting voice_enabled`), the fixed wake word, "Speak
//! answers", the speech recognition – Whisper turbo (recommended) or Parakeet, a dropdown
//! (`settings.set_setting speech_recognizer`, issues #749, #762) –, the language Whisper listens for
//! (`speech_language`, a dropdown with a search: Whisper knows a hundred), the microphone the system
//! lists – a Torro dropdown since issue #547, as the number of microphones is open ([`State::menus`],
//! [`overlay`]) – and "Say something".
//!
//! Above them "Read aloud" (issue #745, `settings.speech_status`): the switch of the chat windows,
//! the provider – Pocket TTS on this computer or OpenAI – and its voice from a dropdown each with the
//! recommended one marked, the credits of the voices, and "Preview", which reads a sentence
//! ([`Message::Preview`], an [`crate::Action::Ask`] of `assistant.speak`).
//!
//! Since issue #763 each of the three groups is a tab of its own ([`VoiceTab`], the row drawn by
//! [`super::page`]): Read aloud, Conversation, Models, so the page fits the window.
//!
//! "Models" (issue #762, `settings.list_voice_models`): every model with what it does, its
//! size and a button – "Download", "Cancel" with the progress while it loads, "Remove" once it is
//! there, and the reason when a download failed. Everything is built into the assistant: nothing to
//! install and no command to copy. While one downloads the section asks for the list every second
//! ([`MODEL_POLL`]), with an [`crate::Action::Ask`], so the rest of the page is not loaded again
//! each time; once a download ends the whole page loads, so "Talk to the assistant" follows.
//!
//! Everything but OpenAI runs on this computer. Switching voice on is a person's switch (point 6 of
//! "Was nie nach außen geht" in `docs/api.md`): only here and in the tray, never through an agent.
//!
//! How it stands comes with [`Data::voice`], which the host asks for with every load – also after
//! every change, so the listener's state follows by itself. "Check again" asks again without
//! changing anything: an [`crate::Action::Send`] with no request is a load.
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

use iced::widget::{row, svg, toggler};
use iced::{Alignment, Element, Subscription};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{Recognizer, SpeechProvider};
use torro_core::voice_models::{VoiceModelKind, WHISPER_LANGUAGES};
use torro_gui::icon::{self, Icon};
use torro_gui::theme;
use torro_gui::theme::space;
use torro_gui::units::format_bytes;
use torro_gui::widgets::{self, Align, DROPDOWN_SEARCH, Dropdown, DropdownEvent, MenuItem, MenuSection, Status};
use torro_gui::{layout, style, typo};
use torro_ipc::{
    Request, Response, SettingKey, SpeechMissing, SpeechStatus, VoiceMissing, VoiceModel, VoiceModelState, VoicePart,
    VoiceState, VoiceStatus,
};

use super::{Action, VoiceTab};
use crate::Data;
use crate::parts::setting_row;

/// How long between two asks while the listener has not followed what was switched.
const VOICE_POLL: Duration = Duration::from_secs(2);

/// How many times the status is asked after voice was switched, until the listener says it runs
/// (or stopped): five times two seconds, as the control centre had it before task S.17. After that
/// the row shows what last came, and "Check again" asks by hand.
const VOICE_POLLS: u8 = 5;

/// How long between two asks for the models while one downloads (issue #762).
const MODEL_POLL: Duration = Duration::from_secs(1);

/// The dropdowns of the page (issue #547).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// "Speech recognition": Whisper or Parakeet (issues #749, #762).
    Recognizer,
    /// "Language" Whisper listens for (issue #762).
    Language,
    /// "Microphone": the system's default or one the system lists.
    Microphone,
    /// "Provider" of reading aloud (issue #745).
    Provider,
    /// "Voice" of the provider that reads (issue #745).
    SpeechVoice,
}

/// Section state: what "Say something" is doing, what the listener is still expected to follow, the
/// open dropdown and the list of models while one downloads. Everything else the section shows comes
/// from [`Data`].
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
    /// The models as the last answer of this section named them, newer than [`Data::voice_models`]
    /// until the next load (issue #762).
    models: Option<Vec<VoiceModel>>,
    /// A request about the models is out; its answer comes to [`answered`].
    asking_models: bool,
    /// "Download", "Cancel" or "Remove" is out. While it is, the preview and the test wait, and while
    /// they run, these buttons wait: [`answered`] learns no more than the answer, so only one of them
    /// may be out to know whose refusal it is (review of #762).
    changing_models: bool,
    /// Why the last request about the models was refused (`busy`, `not-found`), from the core.
    models_failed: Option<String>,
    /// A download ended since the last load: the page loads again, so "Talk to the assistant" and the
    /// providers follow the model that came or went.
    reload: bool,
    /// A model downloads, as the last load or answer said: the list is asked for every second.
    downloading: bool,
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
    /// A dropdown opened, closed or was searched; changes nothing.
    Menus(DropdownEvent<Choice>),
    /// "Talk to the assistant" on or off.
    Enabled(bool),
    /// "Speak answers" on or off.
    SpeakAnswers(bool),
    /// An entry of the dropdown "Speech recognition" (issue #749).
    Recognizer(Recognizer),
    /// An entry of the dropdown "Language": `"auto"` or a code of Whisper's (issue #762).
    Language(String),
    /// An entry of the dropdown "Microphone"; `None` for the system's default.
    Microphone(Option<String>),
    /// "Check again": ask how voice stands again.
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
    /// "Download" of a model, by its id (issue #762); also goes on after "Cancel".
    Download(String),
    /// "Cancel" of a model that downloads.
    CancelDownload(String),
    /// "Remove" of a model that is there.
    Remove(String),
    /// The clock of [`subscription`] while a model downloads: ask for the list again.
    ModelTick,
}

/// The sentence "Preview" reads.
pub fn preview_text() -> &'static str {
    tr("Hello, I am your assistant. This is how I sound.")
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        // Opening the dropdown asks for the focus of its search field (many microphones, languages).
        Message::Menus(event) => match state.menus.update(event) {
            true => Action::Focus(DROPDOWN_SEARCH),
            false => Action::None,
        },
        Message::Enabled(on) => {
            // Only switching on needs the models; off is always possible.
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
        Message::Language(code) => {
            state.menus.close();
            set(SettingKey::SpeechLanguage, serde_json::Value::String(code))
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
            // `polls = 0` on a failed status for the same reason (task C.9). "Check again" asks
            // again by hand once the daemon is back.
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
                SpeechProvider::Pocket => SettingKey::PocketVoice,
                SpeechProvider::OpenAi => SettingKey::OpenAiVoice,
            };
            set(key, serde_json::Value::String(id))
        }
        Message::Preview => {
            if state.previewing || state.changing_models {
                return Action::None;
            }
            state.previewing = true;
            state.preview_failed = None;
            Action::Ask(Request::Speak { text: preview_text().to_string() })
        }
        // The preview's own answer comes once it stopped ([`answered`]).
        Message::StopPreview => Action::send(Request::StopSpeaking),
        Message::TestVoice => {
            if state.testing || state.changing_models {
                return Action::None;
            }
            state.testing = true;
            state.heard = None;
            // The answer is the point, so it comes back to this section ([`answered`]).
            Action::Ask(Request::TestVoice)
        }
        Message::Download(_) | Message::CancelDownload(_) | Message::Remove(_)
            if state.previewing || state.testing || state.changing_models =>
        {
            Action::None
        }
        Message::Download(model) => ask_models(state, Request::DownloadVoiceModel { model }),
        Message::CancelDownload(model) => ask_models(state, Request::CancelVoiceDownload { model }),
        Message::Remove(model) => ask_models(state, Request::RemoveVoiceModel { model }),
        Message::ModelTick => {
            // One ask at a time: a slow answer must not pile up a queue of them.
            if state.asking_models {
                return Action::None;
            }
            if models_of(state, data).iter().any(|model| model.state == VoiceModelState::Running) {
                state.asking_models = true;
                return Action::Ask(Request::ListVoiceModels);
            }
            if state.reload {
                state.reload = false;
                return Action::Send(Vec::new());
            }
            Action::None
        }
    }
}

/// A request about the models; its answer, the list, comes back to [`answered`].
fn ask_models(state: &mut State, request: Request) -> Action {
    state.asking_models = true;
    state.changing_models = true;
    state.models_failed = None;
    Action::Ask(request)
}

/// The answer to this section's [`crate::Action::Ask`], handed over by the app: what "Say
/// something" heard, the list of models, or why it failed. Anything else is not this section's and
/// changes nothing.
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    match result {
        Ok(Response::VoiceModels { models }) => {
            state.asking_models = false;
            state.changing_models = false;
            // A download that ran and stopped: the page loads again on the next tick.
            let before = state.models.as_deref().unwrap_or_default();
            let ended = before.iter().any(|old| {
                old.state == VoiceModelState::Running
                    && models.iter().any(|new| new.id == old.id && new.state != VoiceModelState::Running)
            });
            // A model removed changes what can listen and read just as much.
            let removed = models.iter().any(|new| {
                new.state == VoiceModelState::Missing
                    && before.iter().any(|old| old.id == new.id && old.state == VoiceModelState::Ready)
            });
            state.reload |= ended || removed;
            state.downloading = models.iter().any(|model| model.state == VoiceModelState::Running);
            state.models = Some(models.clone());
        }
        // The preview read to its end or was stopped (issue #745).
        Ok(Response::Ok) if state.previewing => state.previewing = false,
        // A refusal of a change of the models: the preview and the test do not run meanwhile.
        Err(message) if state.changing_models => {
            state.asking_models = false;
            state.changing_models = false;
            state.models_failed = Some(message.clone());
        }
        // Otherwise it is the preview's or the test's: the list of models that may be out as well only
        // fails when the core does not answer at all, and it is asked again a second later.
        Err(message) if state.previewing && !state.testing => {
            state.previewing = false;
            state.asking_models = false;
            state.preview_failed = Some(message.clone());
        }
        Ok(Response::Transcript { text }) => {
            state.testing = false;
            state.heard =
                Some(if text.trim().is_empty() { Heard::Nothing } else { Heard::Text(text.trim().to_string()) });
        }
        Err(message) if state.testing => {
            state.testing = false;
            state.asking_models = false;
            state.heard = Some(Heard::Failed(message.clone()));
        }
        _ => {}
    }
}

/// One voice setting; the host loads again afterwards, so the status follows.
fn set(key: SettingKey, value: serde_json::Value) -> Action {
    Action::send(Request::SetSetting { key, value })
}

/// The clocks of the section: one that asks how voice stands again while the listener has not
/// followed what was last switched, one that asks for the models while one downloads or a load is
/// due after a download; none otherwise – a section at rest asks nothing. The app batches them into
/// the window's own subscription ([`crate::assistant::subscription`]).
pub fn subscription(state: &State) -> Subscription<Message> {
    let listener = match state.expected.is_some() && state.polls > 0 {
        true => iced::time::every(VOICE_POLL).map(|_| Message::Poll),
        false => Subscription::none(),
    };
    let models = match state.downloading || state.reload {
        true => iced::time::every(MODEL_POLL).map(|_| Message::ModelTick),
        false => Subscription::none(),
    };
    Subscription::batch([listener, models])
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

/// The models to show: the section's own answer while it is newer than the last load.
pub fn models_of<'a>(state: &'a State, data: &'a Data) -> &'a [VoiceModel] {
    state.models.as_deref().unwrap_or(&data.voice_models)
}

// ----- Views --------------------------------------------------------------------------------

/// The group of the tab on screen (issue #763): reading aloud, the conversation by voice with the
/// microphone where one can be named, or the models (issue #762). While the assistant is off, the tab
/// says so under its own name.
pub fn view<'a>(state: &'a State, data: &'a Data, tab: VoiceTab) -> Element<'a, Message> {
    if !super::is_on(data) {
        return super::off_group(tab.label());
    }
    match tab {
        VoiceTab::ReadAloud => read_aloud_group(state, data),
        VoiceTab::Conversation => voice_group(state, data),
        VoiceTab::Models => models_group(state, data),
    }
}

/// The open dropdown under its button, the chosen entry marked: the two speech recognitions, the
/// languages of Whisper, the providers or voices of reading aloud, or the system's default and every
/// microphone the system lists.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let key = state.menus.open().filter(|_| super::is_on(data))?;
    let items = match key {
        Choice::Recognizer => recognizer_items(data),
        Choice::Language => language_items(data),
        Choice::Provider | Choice::SpeechVoice => speech_items(*key, data),
        Choice::Microphone => {
            let microphones = data.voice.as_ref().map(|voice| &voice.microphones).filter(|list| !list.is_empty())?;
            let chosen = data.settings.voice.microphone.as_deref();
            let default = MenuItem::plain(tr("System default"), Message::Microphone(None)).current(chosen.is_none());
            std::iter::once(default)
                .chain(microphones.iter().map(|name| {
                    MenuItem::plain(name.as_str(), Message::Microphone(Some(name.clone())))
                        .current(chosen == Some(name.as_str()))
                }))
                .collect()
        }
    };
    let sections = vec![MenuSection::new(items)];
    Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus))
}

/// Fresh data came ([`super::received`]): with no microphone listed any more its dropdown closes, so
/// it neither takes the next Escape nor opens again by itself later. The other dropdowns stay open:
/// their entries do not depend on what was found. The section's own list of models gives way to the
/// loaded one.
pub fn loaded(state: &mut State, data: &Data) {
    let no_microphones = data.voice.as_ref().is_none_or(|voice| voice.microphones.is_empty());
    if !super::is_on(data) || (no_microphones && state.menus.is_open(&Choice::Microphone)) {
        state.menus.close();
    }
    // An answer still out is newer than this load: it keeps its place when it comes. Otherwise the
    // load's list is the one the next answer is compared with, so a model removed or finished right
    // after a load still loads the page again (review of #762).
    if !state.asking_models {
        state.models = Some(data.voice_models.clone());
    }
    state.downloading = models_of(state, data).iter().any(|model| model.state == VoiceModelState::Running);
}

/// Escape closes the open dropdown first.
pub fn escape(state: &State) -> Option<Message> {
    state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
}

/// The two speech recognitions, Whisper recommended.
fn recognizer_items(data: &Data) -> Vec<MenuItem<'_, Message>> {
    let chosen = data.settings.voice.recognizer;
    vec![
        MenuItem::plain(recognizer_name(Recognizer::Whisper), Message::Recognizer(Recognizer::Whisper))
            .detail(tr("Understands German well, you choose the language"))
            .tag(tr("Recommended"), false)
            .current(chosen == Recognizer::Whisper),
        MenuItem::plain(recognizer_name(Recognizer::Parakeet), Message::Recognizer(Recognizer::Parakeet))
            .detail(tr("Faster, detects the language itself"))
            .current(chosen == Recognizer::Parakeet),
    ]
}

/// "Automatic" and the languages of Whisper by their own names, sorted; the one Whisper listens for
/// marked.
fn language_items(data: &Data) -> Vec<MenuItem<'_, Message>> {
    let chosen = data.settings.voice.language_or_default();
    let automatic = MenuItem::plain(tr("Automatic"), Message::Language("auto".into()))
        .detail(tr("Whisper guesses; short sentences are often taken for another language"))
        .current(chosen == "auto");
    let mut languages: Vec<&(&str, &str)> = WHISPER_LANGUAGES.iter().collect();
    languages.sort_by_key(|(_, name)| name.to_lowercase());
    std::iter::once(automatic)
        .chain(languages.into_iter().map(|(code, name)| {
            MenuItem::plain(*name, Message::Language((*code).to_string())).current(chosen == *code)
        }))
        .collect()
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
        Choice::Recognizer | Choice::Language | Choice::Microphone => Vec::new(),
    }
}

/// "Read aloud" (issue #745): the switch of the chat windows, the provider with what it lacks, the
/// voice with "Preview", and who the voices are by.
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
        (None, Some(_)) => Some(tr("This computer could not be checked. Use Check again under Models.").to_string()),
        (speech, _) => provider_detail(speech.as_ref(), current),
    };
    let provider =
        setting_row(None, tr("Provider"), detail, state.menus.button(Choice::Provider, label, Message::Menus));
    let preview = match state.previewing {
        true => widgets::text_button(None, tr("Stop"), style::button::secondary, Some(Message::StopPreview)),
        false => widgets::text_button(
            None,
            tr("Preview"),
            style::button::secondary,
            (!state.changing_models).then_some(Message::Preview),
        ),
    };
    let chosen = chosen_voice(data, current);
    let recommended = voices_of(data, current).iter().any(|(id, recommended)| *recommended && *id == chosen);
    let picker = state.menus.button(Choice::SpeechVoice, marked(&voice_name(&chosen), recommended), Message::Menus);
    let control = row![picker, preview].spacing(layout::item_gap()).align_y(Alignment::Center);
    // "Voice" as the sound of a speaker: in German the page itself is "Sprache".
    // A preview that failed says so with a dot, not only in words (#560).
    let dot = state.preview_failed.as_ref().map(|_| widgets::status_dot(Status::Error));
    let voice = setting_row(dot, trc("speaker", "Voice"), state.preview_failed.clone(), control);
    let footer = match current {
        // The licences of the voices ask for their names (decision 2 of #762).
        SpeechProvider::Pocket => format!(
            "{} {}",
            tr(
                "Voices by Kyutai (Pocket TTS, CC BY 4.0), Alba MacKenna (CC BY 4.0), VCTK (CC BY 4.0), voice donation (CC0)."
            ),
            tr("Pocket TTS reads on this computer.")
        ),
        SpeechProvider::OpenAi => tr("With OpenAI the text of each answer goes to OpenAI.").to_string(),
    };
    widgets::group(tr("Read aloud"), vec![on, provider, voice], Some(footer))
}

/// "Voice": on or off with what the listener does, the wake word, "Speak answers", the speech
/// recognition with its language, the microphone and the test.
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
            (can_listen && !state.changing_models).then_some(Message::TestVoice),
        ),
    };
    let test = setting_row(None, tr("Test the microphone"), Some(heard_text(state)), test_button);
    let footer = tr(
        "Say “Hey Torro” and your question. Everything stays on this computer. Approvals are only given with a click, never by voice.",
    );
    let recognizer = marked(recognizer_name(settings.recognizer), settings.recognizer == Recognizer::Whisper);
    let recognizer = state.menus.button(Choice::Recognizer, recognizer, Message::Menus);
    let recognizer = setting_row(None, tr("Speech recognition"), None, recognizer);
    let mut rows = vec![on, wake, speak, recognizer, language_row(state, data)];
    rows.extend(microphone_row(state, data));
    rows.push(test);
    widgets::group(tr("Conversation"), rows, Some(footer.to_string()))
}

/// "Language" (decision 4 of #762): Whisper's as a dropdown, with the language of the suite unless
/// chosen; Parakeet takes none and the row says so.
fn language_row<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let voice = &data.settings.voice;
    if voice.recognizer == Recognizer::Parakeet {
        let says = typo::text(typo::body(), tr("Automatic")).style(theme::text_2);
        return setting_row(None, tr("Language"), Some(tr("Parakeet detects the language itself.").to_string()), says);
    }
    let button = state.menus.button(Choice::Language, language_name(voice.language_or_default()), Message::Menus);
    setting_row(None, tr("Language"), None, button)
}

/// "Microphone" with its dropdown (issue #547): the system's default or the chosen one. None when
/// the system lists no microphone.
fn microphone_row<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    let microphones = &data.voice.as_ref()?.microphones;
    if microphones.is_empty() {
        return None;
    }
    let label = data.settings.voice.microphone.clone().unwrap_or_else(|| tr("System default").to_string());
    let button = state.menus.button(Choice::Microphone, label, Message::Menus);
    Some(setting_row(None, tr("Microphone"), microphone_detail(data), button))
}

/// The line under "Microphone" when the chosen one does not record (review of #762): it is gone or
/// another program holds it, and the system's default records instead.
pub fn microphone_detail(data: &Data) -> Option<String> {
    let chosen = data.settings.voice.microphone.as_deref()?;
    let recorder = data.voice.as_ref()?.program(VoicePart::Recorder)?;
    let in_use = recorder.program.as_deref()?;
    (in_use != chosen).then(|| tr("This microphone cannot be used now, the system default records.").to_string())
}

/// "Models" (issue #762): every model with its size and button, the reason when a request was
/// refused, and a line for each sound device the system does not offer.
fn models_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let again = || widgets::text_button(None, tr("Check again"), style::button::secondary, Some(Message::Check));
    let mut rows: Vec<Element<'a, Message>> = match &data.voice {
        Some(status) if !status.installed => vec![setting_row(
            Some(widgets::status_dot(Status::Error)),
            tr("The assistant app is not installed"),
            Some(tr("Add the assistant under Apps in Torro Workplace.").to_string()),
            again(),
        )],
        _ => {
            let free = !(state.previewing || state.testing || state.changing_models);
            models_of(state, data).iter().map(|model| model_row(model, free)).collect()
        }
    };
    if let Some(message) = &state.models_failed {
        rows.push(setting_row(Some(widgets::status_dot(Status::Error)), message.clone(), None, again()));
    }
    for (label, detail) in data.voice.as_ref().map(missing_devices).unwrap_or_default() {
        rows.push(setting_row(Some(widgets::status_dot(Status::Error)), label, Some(detail.to_string()), again()));
    }
    if data.voice.is_none() {
        rows.push(setting_row(None, tr("This computer could not be checked"), None, again()));
    }
    let footer = tr("Downloads the models from Hugging Face. They stay on this computer.");
    widgets::group(tr("Models"), rows, Some(footer.to_string()))
}

/// One model: its name, what it does and its size, and by its state the progress or the reason it
/// failed, with the one button that fits; the button waits while `free` is false.
fn model_row(model: &VoiceModel, free: bool) -> Element<'_, Message> {
    let button = |label, message| widgets::text_button(None, label, style::button::secondary, free.then_some(message));
    let detail = Some(model_detail(model));
    let name = marked(model_name(model), model.recommended);
    match model.state {
        VoiceModelState::Ready => {
            let done = typo::text(typo::meta(), tr("Downloaded")).style(theme::text_2);
            let control = row![done, button(tr("Remove"), Message::Remove(model.id.clone()))]
                .spacing(space::m())
                .align_y(Alignment::Center);
            // No dot: "Downloaded" says it, and the rows keep one axis (design review of #547).
            setting_row(None, name, detail, control)
        }
        VoiceModelState::Running => {
            let percent = percent(model);
            let bar = widgets::progress_bar(percent as usize, 100);
            let label = typo::text(typo::meta().tabular(), format!("{percent} %")).style(theme::text_3);
            let control = row![bar, label, button(tr("Cancel"), Message::CancelDownload(model.id.clone()))]
                .spacing(space::m())
                .align_y(Alignment::Center);
            setting_row(None, name, detail, control)
        }
        VoiceModelState::Failed => {
            // The reason in words and with a mark, not only in colour (#560).
            let mark =
                icon::icon(Icon::CircleX, icon::SIZE).style(|t, _| svg::Style { color: Some(theme::palette(t).error) });
            let reason = model.error.clone().unwrap_or_else(|| tr("The download failed. Try again.").to_string());
            let detail = format!("{} · {reason}", model_detail(model));
            setting_row(
                Some(mark.into()),
                name,
                Some(detail),
                button(tr("Download"), Message::Download(model.id.clone())),
            )
        }
        VoiceModelState::Missing | VoiceModelState::Cancelled => {
            setting_row(None, name, detail, button(tr("Download"), Message::Download(model.id.clone())))
        }
    }
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The line under a model: what it does and its size, and how far a cancelled download got.
pub fn model_detail(model: &VoiceModel) -> String {
    let kind = match model.kind {
        VoiceModelKind::Recognition => tr("Speech recognition"),
        VoiceModelKind::Speech => tr("Read aloud"),
    };
    let size = format_bytes(model.bytes);
    match model.state {
        VoiceModelState::Cancelled if model.completed > 0 => trf(
            "{kind} · {size} · paused at {percent} %",
            &[("kind", &kind), ("size", &size), ("percent", &percent(model))],
        ),
        _ => format!("{kind} · {size}"),
    }
}

/// The name of a model for people: the product's name, the language of a voice model in words.
pub fn model_name(model: &VoiceModel) -> &str {
    match model.id.as_str() {
        torro_core::voice_models::POCKET_DE => tr("Pocket TTS, German"),
        _ => &model.name,
    }
}

/// How far a download got, in whole percent.
pub fn percent(model: &VoiceModel) -> u64 {
    match model.bytes {
        0 => 0,
        bytes => (model.completed.min(bytes) * 100) / bytes,
    }
}

/// The sound devices the system does not offer, as a label and how to get one: a row each instead
/// of a list of programs (issue #762).
pub fn missing_devices(status: &VoiceStatus) -> Vec<(&'static str, &'static str)> {
    let lacks = |part, missing| status.program(part).and_then(|program| program.missing) == Some(missing);
    let mut rows = Vec::new();
    if lacks(VoicePart::Recorder, VoiceMissing::Program) {
        rows.push((
            tr("No microphone found"),
            tr("Connect a microphone or allow Torro to use it in the system settings."),
        ));
    }
    if lacks(VoicePart::Speech, VoiceMissing::Player) {
        rows.push((tr("No sound output found"), tr("Connect speakers or headphones to hear the answers.")));
    }
    rows
}

/// The line under "Test the microphone": what was heard, or how to test.
pub fn heard_text(state: &State) -> String {
    if state.testing {
        return tr("Speak now, it stops at a pause").to_string();
    }
    match &state.heard {
        None => tr("Shows what the assistant understands").to_string(),
        Some(Heard::Text(text)) => trf("Heard: “{text}”", &[("text", text)]),
        Some(Heard::Nothing) => tr("Nothing understood. Speak a little louder or closer.").to_string(),
        Some(Heard::Failed(_)) => tr("The test failed. Check the models under Models.").to_string(),
    }
}

/// The provider that reads now: the core's word (the chosen one, or its choice), else the chosen one,
/// else Pocket TTS.
pub fn current_provider(data: &Data) -> SpeechProvider {
    data.speech
        .as_ref()
        .map(|speech| speech.current)
        .or(data.settings.voice.speech_provider)
        .unwrap_or(SpeechProvider::Pocket)
}

/// The name of `provider` in the dropdown.
pub fn provider_name(provider: SpeechProvider) -> &'static str {
    match provider {
        SpeechProvider::Pocket => tr("Local: Pocket TTS"),
        SpeechProvider::OpenAi => tr("Online: OpenAI"),
    }
}

/// `name` with "(recommended)" when it is.
pub fn marked(name: &str, recommended: bool) -> String {
    match recommended {
        true => trf("{name} (recommended)", &[("name", &name)]),
        false => name.to_string(),
    }
}

/// The name of a voice: the assistant's templates in words, the others by their name, capitalised.
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

/// The name of the language `code` on its button: "Automatic" or its own name.
pub fn language_name(code: &str) -> String {
    match code {
        "auto" => tr("Automatic").to_string(),
        code => torro_core::voice_models::whisper_language(code).unwrap_or(code).to_string(),
    }
}

/// The voices of `provider` with whether each is recommended; without a status those of the
/// settings' lists.
fn voices_of(data: &Data, provider: SpeechProvider) -> Vec<(String, bool)> {
    use torro_core::settings::{OPENAI_RECOMMENDED, OPENAI_VOICES, POCKET_RECOMMENDED, POCKET_VOICES};
    if let Some(option) = data.speech.as_ref().and_then(|speech| speech.option(provider)) {
        return option.voices.iter().map(|voice| (voice.id.clone(), voice.recommended)).collect();
    }
    match provider {
        SpeechProvider::Pocket => POCKET_VOICES.iter().map(|id| (id.to_string(), *id == POCKET_RECOMMENDED)).collect(),
        SpeechProvider::OpenAi => {
            OPENAI_VOICES.iter().map(|id| (id.to_string(), OPENAI_RECOMMENDED.contains(id))).collect()
        }
    }
}

/// The voice `provider` reads with: the chosen one, else the recommended one.
fn chosen_voice(data: &Data, provider: SpeechProvider) -> String {
    let voice = &data.settings.voice;
    match provider {
        SpeechProvider::Pocket => voice.pocket_voice_or_default().to_string(),
        SpeechProvider::OpenAi => voice.openai_voice_or_default().to_string(),
    }
}

/// The line under "Provider": what keeps the provider chosen from reading; none when it can.
pub fn provider_detail(speech: Option<&SpeechStatus>, current: SpeechProvider) -> Option<String> {
    let Some(speech) = speech else { return Some(tr("Checking this computer…").to_string()) };
    speech.option(current).and_then(|option| option.missing).map(|missing| missing_text(missing).to_string())
}

/// What keeps a provider from reading, for people.
pub fn missing_text(missing: SpeechMissing) -> &'static str {
    match missing {
        SpeechMissing::Model => tr("Download the model under Models first."),
        SpeechMissing::Player => tr("No sound output was found."),
        SpeechMissing::Account => tr("Connect an OpenAI account in Settings › AI."),
        SpeechMissing::App => tr("The assistant app is not installed."),
        // Not reported since #762; a core of an earlier version may still send them.
        SpeechMissing::Gpu | SpeechMissing::Program | SpeechMissing::Voice => {
            tr("Update Torro Workplace to read aloud on this computer.")
        }
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
        let detail =
            if status.can_listen() { tr("Off") } else { tr("Download a speech recognition model under Models first") };
        return (Status::Inactive, detail.to_string());
    }
    match (status.listener, &status.problem) {
        (VoiceState::Listening, _) => (Status::Ok, tr("Listening for “Hey Torro”").to_string()),
        (VoiceState::Recording, _) => (Status::Ok, tr("Recording …").to_string()),
        (VoiceState::Answering, _) => (Status::Ok, tr("Answering …").to_string()),
        (VoiceState::Off, Some(_)) => {
            (Status::Error, tr("The listener stopped. Check the models under Models.").to_string())
        }
        (VoiceState::Off, None) => (Status::Attention, tr("Starting …").to_string()),
    }
}

/// The name of a speech recognition, on its button and in its dropdown: names of models, not
/// translated.
pub fn recognizer_name(recognizer: Recognizer) -> &'static str {
    match recognizer {
        Recognizer::Parakeet => "Parakeet",
        Recognizer::Whisper => "Whisper",
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;
    use torro_ipc::VoiceProgram;

    fn status(listener: VoiceState, missing: Option<VoiceMissing>) -> VoiceStatus {
        let part = |part| VoiceProgram { part, program: Some("x".into()), missing: None };
        VoiceStatus {
            listener,
            installed: true,
            problem: None,
            programs: vec![
                part(VoicePart::WakeWord),
                part(VoicePart::Recorder),
                VoiceProgram { missing, ..part(VoicePart::SpeechToText) },
                part(VoicePart::Speech),
            ],
            microphones: vec!["USB microphone".into()],
            folder: Some("/home/anna/.local/share/torroassistant/voice".into()),
        }
    }

    fn model(id: &str, state: VoiceModelState, completed: u64) -> VoiceModel {
        VoiceModel {
            id: id.into(),
            name: "Whisper turbo".into(),
            kind: VoiceModelKind::Recognition,
            bytes: 1_000,
            recommended: true,
            state,
            completed,
            error: (state == VoiceModelState::Failed)
                .then(|| tr("No connection to the internet. Check the network and try again.").to_string()),
        }
    }

    /// Builds every tab of the page with `state` and `data`.
    fn views(state: &State, data: &Data) {
        for tab in VoiceTab::ALL {
            let _ = view(state, data, tab);
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
            sent(update(&mut state, Message::Microphone(Some("USB microphone".into())), &data)),
            (SettingKey::Microphone, serde_json::json!("USB microphone"))
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
        answered(&mut state, &Err("no microphone".into()));
        assert_eq!(heard_text(&state), tr("The test failed. Check the models under Models."));

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
        let starting = Data { voice: Some(status(VoiceState::Off, None)), voice_models: Vec::new(), ..data().await };
        let mut state = State::default();
        assert_eq!(subscription(&state).units(), 0, "a section at rest asks nothing");

        let _ = update(&mut state, Message::Enabled(true), &starting);
        assert_eq!(subscription(&state).units(), 1, "the clock runs");
        assert_eq!(voice_state(true, starting.voice.as_ref()).1, tr("Starting …"));
        // Still starting: ask once more.
        assert_eq!(update(&mut state, Message::Poll, &starting), Act::Send(Vec::new()));

        // The listener runs: the line says so and the clock stops.
        let listening = Data { voice: Some(status(VoiceState::Listening, None)), ..starting.clone() };
        assert!(matches!(update(&mut state, Message::Poll, &listening), Act::None));
        assert_eq!(subscription(&state).units(), 0, "nothing left to wait for");
        assert_eq!(voice_state(true, listening.voice.as_ref()).1, tr("Listening for “Hey Torro”"));

        // Switched off, it waits for the listener to be off.
        let _ = update(&mut state, Message::Enabled(false), &listening);
        assert_eq!(subscription(&state).units(), 1);
        assert!(matches!(update(&mut state, Message::Poll, &starting), Act::None), "off is off");
        assert_eq!(subscription(&state).units(), 0);
    }

    /// Found in the second review of #210: a load that failed stops the clock at once.
    #[tokio::test]
    async fn a_daemon_that_does_not_answer_stops_the_clock() {
        let starting = Data { voice: Some(status(VoiceState::Off, None)), voice_models: Vec::new(), ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Enabled(true), &starting);
        let offline = Data { error: Some("the core is not running".into()), voice: None, ..starting.clone() };
        assert!(matches!(update(&mut state, Message::Poll, &offline), Act::None), "no second try");
        assert_eq!(subscription(&state).units(), 0, "and no clock");
        assert_eq!(update(&mut state, Message::Check, &offline), Act::Send(Vec::new()));
    }

    /// It does not ask for ever: after [`VOICE_POLLS`] tries the row shows what last came.
    #[tokio::test]
    async fn the_asking_stops_after_a_few_tries() {
        let starting = Data { voice: Some(status(VoiceState::Off, None)), voice_models: Vec::new(), ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Enabled(true), &starting);
        for left in 0..VOICE_POLLS {
            assert_eq!(update(&mut state, Message::Poll, &starting), Act::Send(Vec::new()), "try {left}");
        }
        assert!(matches!(update(&mut state, Message::Poll, &starting), Act::None), "enough");
        assert_eq!(subscription(&state).units(), 0);
    }

    /// Without a speech recognition model voice cannot be switched on; off always works.
    #[tokio::test]
    async fn voice_needs_a_model_to_switch_on() {
        let missing = status(VoiceState::Off, Some(VoiceMissing::Model));
        assert!(!missing.can_listen());
        let data = Data { voice: Some(missing.clone()), ..data().await };
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::Enabled(true), &data), Act::None));
        assert!(matches!(update(&mut state, Message::Enabled(false), &data), Act::Send(_)));
        assert_eq!(voice_state(false, Some(&missing)).1, tr("Download a speech recognition model under Models first"));
    }

    #[test]
    fn voice_states_read_as_words() {
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
    }

    /// Issue #762: a missing microphone or sound output gets a line of its own, nothing to install.
    #[test]
    fn missing_devices_are_named() {
        assert!(missing_devices(&status(VoiceState::Off, None)).is_empty());
        let mut quiet = status(VoiceState::Off, None);
        for program in &mut quiet.programs {
            program.missing = match program.part {
                VoicePart::Recorder => Some(VoiceMissing::Program),
                VoicePart::Speech => Some(VoiceMissing::Player),
                _ => None,
            };
        }
        let labels: Vec<&str> = missing_devices(&quiet).into_iter().map(|(label, _)| label).collect();
        assert_eq!(labels, [tr("No microphone found"), tr("No sound output found")]);
    }

    /// Issues #749, #762: "Speech recognition" is a dropdown of Whisper (recommended) and Parakeet; a
    /// choice closes it and sends the setting. It stays open over a load, also without microphones.
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
        views(&state, &data);
        let chosen = update(&mut state, Message::Recognizer(Recognizer::Parakeet), &data);
        assert_eq!(sent(chosen), (SettingKey::SpeechRecognizer, serde_json::json!("parakeet")));
        assert_eq!(state.menus.open(), None, "the choice closes it");
        assert_eq!(recognizer_items(&data).len(), 2);
    }

    /// Issue #762, decision 4: Whisper's language is a dropdown with "Automatic" and every language,
    /// the suite's language unless chosen; with Parakeet the row says it detects the language.
    #[tokio::test]
    async fn the_language_is_a_dropdown_for_whisper_only() {
        let mut data = Data { voice: Some(status(VoiceState::Listening, None)), ..data().await };
        data.settings.voice.recognizer = Recognizer::Whisper;
        let mut state = State::default();
        assert_eq!(
            update(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::Language)), &data),
            Act::Focus(DROPDOWN_SEARCH),
            "a hundred languages: with a search"
        );
        assert!(overlay(&state, &data).is_some());
        assert_eq!(language_items(&data).len(), WHISPER_LANGUAGES.len() + 1);
        let chosen = update(&mut state, Message::Language("fr".into()), &data);
        assert_eq!(sent(chosen), (SettingKey::SpeechLanguage, serde_json::json!("fr")));
        assert_eq!(language_name("auto"), tr("Automatic"));
        assert_eq!(language_name("de"), "Deutsch");
        assert_eq!(
            language_name(data.settings.voice.language_or_default()),
            language_name(match torro_core::i18n::lang() {
                torro_core::i18n::Lang::De => "de",
                torro_core::i18n::Lang::En => "en",
            })
        );
        views(&state, &data);
        data.settings.voice.recognizer = Recognizer::Parakeet;
        views(&state, &data);
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
        views(&state, &data);
        let chosen = update(&mut state, Message::Microphone(Some("USB microphone".into())), &data);
        assert_eq!(sent(chosen), (SettingKey::Microphone, serde_json::json!("USB microphone")));
        assert_eq!((state.menus.open(), escape(&state)), (None, None), "the choice closes it");
        let none =
            Data { voice: Some(VoiceStatus { microphones: Vec::new(), ..status(VoiceState::Off, None) }), ..data };
        assert!(microphone_row(&state, &none).is_none() && overlay(&state, &none).is_none());
    }

    /// Issue #762: Download, Cancel and Remove ask the core and take the list it answers; while one
    /// downloads the list is asked for every second, and once it ended the page loads again.
    #[tokio::test]
    async fn models_download_cancel_and_remove() {
        let data = Data { voice_models: vec![model("whisper-turbo", VoiceModelState::Missing, 0)], ..data().await };
        let mut state = State::default();
        assert_eq!(subscription(&state).units(), 0, "nothing downloads");
        assert_eq!(
            update(&mut state, Message::Download("whisper-turbo".into()), &data),
            Act::Ask(Request::DownloadVoiceModel { model: "whisper-turbo".into() })
        );
        answered(
            &mut state,
            &Ok(Response::VoiceModels { models: vec![model("whisper-turbo", VoiceModelState::Running, 620)] }),
        );
        assert_eq!(models_of(&state, &data)[0].state, VoiceModelState::Running);
        assert_eq!(subscription(&state).units(), 1, "the clock runs while it downloads");
        assert_eq!(update(&mut state, Message::ModelTick, &data), Act::Ask(Request::ListVoiceModels));
        assert!(matches!(update(&mut state, Message::ModelTick, &data), Act::None), "one ask at a time");
        answered(
            &mut state,
            &Ok(Response::VoiceModels { models: vec![model("whisper-turbo", VoiceModelState::Running, 700)] }),
        );
        assert_eq!(
            update(&mut state, Message::CancelDownload("whisper-turbo".into()), &data),
            Act::Ask(Request::CancelVoiceDownload { model: "whisper-turbo".into() })
        );
        answered(
            &mut state,
            &Ok(Response::VoiceModels { models: vec![model("whisper-turbo", VoiceModelState::Cancelled, 700)] }),
        );
        assert!(model_detail(&models_of(&state, &data)[0]).contains("70 %"), "how far it got");
        // The download ended: one load, then the clock rests.
        assert_eq!(update(&mut state, Message::ModelTick, &data), Act::Send(Vec::new()));
        loaded(&mut state, &data);
        assert_eq!(subscription(&state).units(), 0);
        assert_eq!(models_of(&state, &data)[0].state, VoiceModelState::Missing, "the load's list counts again");

        assert_eq!(
            update(&mut state, Message::Remove("whisper-turbo".into()), &data),
            Act::Ask(Request::RemoveVoiceModel { model: "whisper-turbo".into() })
        );
        answered(&mut state, &Err("Another download is running.".into()));
        assert_eq!(state.models_failed.as_deref(), Some("Another download is running."));
        views(&state, &data);
    }

    /// Review of #762: a model removed right after a load loads the page again, so "Talk to the
    /// assistant" and the provider follow; a refusal while the list is asked goes to the preview.
    #[tokio::test]
    async fn a_removal_right_after_a_load_loads_again() {
        let data = Data { voice_models: vec![model("whisper-turbo", VoiceModelState::Ready, 1_000)], ..data().await };
        let mut state = State::default();
        loaded(&mut state, &data);
        let _ = update(&mut state, Message::Remove("whisper-turbo".into()), &data);
        answered(
            &mut state,
            &Ok(Response::VoiceModels { models: vec![model("whisper-turbo", VoiceModelState::Missing, 0)] }),
        );
        assert_eq!(subscription(&state).units(), 1, "the clock runs for the load");
        assert_eq!(update(&mut state, Message::ModelTick, &data), Act::Send(Vec::new()));

        let running = Data { voice_models: vec![model("whisper-turbo", VoiceModelState::Running, 10)], ..data };
        loaded(&mut state, &running);
        assert_eq!(update(&mut state, Message::ModelTick, &running), Act::Ask(Request::ListVoiceModels));
        let _ = update(&mut state, Message::Preview, &running);
        answered(&mut state, &Err("Download the voice first.".into()));
        assert_eq!(state.preview_failed.as_deref(), Some("Download the voice first."));
        assert!(!state.previewing && state.models_failed.is_none(), "the preview took the refusal");
        assert_eq!(update(&mut state, Message::ModelTick, &running), Act::Ask(Request::ListVoiceModels));
    }

    /// Review of #762: a change of the models and the preview or test never run at once, so a refusal
    /// always reaches the one that asked.
    #[tokio::test]
    async fn a_change_of_the_models_and_the_preview_wait_for_each_other() {
        let data = Data { voice_models: vec![model("parakeet", VoiceModelState::Missing, 0)], ..data().await };
        let mut state = State::default();
        let _ = update(&mut state, Message::Download("parakeet".into()), &data);
        assert!(matches!(update(&mut state, Message::Preview, &data), Act::None), "the preview waits");
        assert!(matches!(update(&mut state, Message::TestVoice, &data), Act::None), "the test waits");
        answered(&mut state, &Err("This model is downloading already.".into()));
        assert_eq!(state.models_failed.as_deref(), Some("This model is downloading already."));
        assert!(state.preview_failed.is_none() && state.heard.is_none());

        let _ = update(&mut state, Message::Preview, &data);
        assert!(matches!(update(&mut state, Message::Remove("parakeet".into()), &data), Act::None), "Remove waits");
        views(&state, &data);
    }

    /// Review of #762: a chosen microphone that does not record says so; the default records.
    #[tokio::test]
    async fn a_microphone_that_does_not_record_says_so() {
        let mut data = Data { voice: Some(status(VoiceState::Listening, None)), ..data().await };
        assert_eq!(microphone_detail(&data), None, "the default needs no line");
        data.settings.voice.microphone = Some("x".into());
        assert_eq!(microphone_detail(&data), None, "it records");
        data.settings.voice.microphone = Some("HD Pro Webcam C920".into());
        assert_eq!(
            microphone_detail(&data).as_deref(),
            Some(tr("This microphone cannot be used now, the system default records."))
        );
        views(&State::default(), &data);
    }

    /// Issue #762: every state of a model builds, with its button; nothing asks to install or copy.
    #[tokio::test]
    async fn every_model_state_builds_without_a_command_to_copy() {
        let mut data = data().await;
        data.voice_models = [
            VoiceModelState::Missing,
            VoiceModelState::Running,
            VoiceModelState::Ready,
            VoiceModelState::Failed,
            VoiceModelState::Cancelled,
        ]
        .into_iter()
        .map(|state| model("parakeet", state, 500))
        .collect();
        views(&State::default(), &data);
        assert_eq!(percent(&model("x", VoiceModelState::Running, 620)), 62);
        assert_eq!(percent(&VoiceModel { bytes: 0, ..model("x", VoiceModelState::Running, 5) }), 0);
        let detail = model_detail(&model("x", VoiceModelState::Missing, 0));
        assert!(detail.starts_with(tr("Speech recognition")), "{detail}");
        // The texts of the page never send anyone to a terminal (Erledigt 3 of #762): no text for
        // people names a command, a package or copying.
        let source = include_str!("voice.rs");
        let view_part = &source[..source.find("#[cfg(test)]").expect("tests")];
        let texts: Vec<&str> = view_part.split("tr(\"").skip(1).filter_map(|rest| rest.split('"').next()).collect();
        assert!(texts.len() > 40, "the texts were found: {}", texts.len());
        for text in texts {
            let lower = text.to_lowercase();
            for word in ["install ", "pip", "copy", "terminal", "command", "brew", "whisper-cli", "onnx-asr"] {
                assert!(!lower.contains(word), "{text}");
            }
        }
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
                views(&state, &Data { voice: voice.clone(), ..on.clone() });
                views(&state, &Data { voice, ..data.clone() });
            }
            views(&state, &turned_off(&data));
        }
    }

    /// Issue #745: the switch, provider and voice each send their setting; the recommended entry is
    /// marked; Preview asks to read and Stop stops it.
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
            sent(update(&mut state, Message::SpeechVoice(SpeechProvider::Pocket, "marius".into()), &data)),
            (SettingKey::PocketVoice, serde_json::json!("marius"))
        );
        assert_eq!(
            update(&mut state, Message::Preview, &data),
            Act::Ask(Request::Speak { text: preview_text().into() })
        );
        assert!(matches!(update(&mut state, Message::Preview, &data), Act::None), "one at a time");
        assert_eq!(update(&mut state, Message::StopPreview, &data), Act::Send(vec![Request::StopSpeaking]));
        answered(&mut state, &Ok(Response::Ok));
        assert!(!state.previewing);
        let _ = update(&mut state, Message::Preview, &data);
        answered(&mut state, &Err("No sound output was found.".into()));
        assert_eq!(state.preview_failed.as_deref(), Some("No sound output was found."));

        let failed = Data { speech: None, ..data.clone() };
        views(&state, &failed);

        assert_eq!(speech_items(Choice::Provider, &data).len(), 2);
        let pocket =
            Data { speech: Some(SpeechStatus { current: SpeechProvider::Pocket, ..speech.clone() }), ..data.clone() };
        assert_eq!(speech_items(Choice::SpeechVoice, &pocket).len(), 5);
        let openai =
            Data { speech: Some(SpeechStatus { current: SpeechProvider::OpenAi, ..speech.clone() }), ..data.clone() };
        assert_eq!(speech_items(Choice::SpeechVoice, &openai).len(), 13);
        assert_eq!(voice_name("warm_woman"), tr("Warm woman"));
        assert_eq!(voice_name("alba"), "Alba");
        for data in [&data, &pocket, &openai] {
            views(&state, data);
            for key in [Choice::Provider, Choice::SpeechVoice] {
                state.menus = Dropdown::default();
                let _ = update(&mut state, Message::Menus(DropdownEvent::Toggle(key)), data);
                assert!(overlay(&state, data).is_some(), "{key:?}");
            }
        }
    }

    /// Issue #762: the line under "Provider" names only what keeps it from reading.
    #[test]
    fn the_provider_line_says_what_is_missing() {
        let status = |missing| SpeechStatus {
            gpu: None,
            fast_enough: true,
            recommended: SpeechProvider::Pocket,
            current: SpeechProvider::Pocket,
            providers: vec![torro_ipc::SpeechOption {
                provider: SpeechProvider::Pocket,
                missing,
                voices: Vec::new(),
                model: None,
                setup: None,
            }],
        };
        assert_eq!(provider_detail(Some(&status(None)), SpeechProvider::Pocket), None);
        assert_eq!(
            provider_detail(Some(&status(Some(SpeechMissing::Model))), SpeechProvider::Pocket).as_deref(),
            Some(tr("Download the model under Models first."))
        );
        assert_eq!(provider_detail(None, SpeechProvider::Pocket).as_deref(), Some(tr("Checking this computer…")));
        assert_eq!(missing_text(SpeechMissing::Account), tr("Connect an OpenAI account in Settings › AI."));
    }
}
