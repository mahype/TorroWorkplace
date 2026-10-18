//! The settings of the assistant, in sections (task S.17 in `docs/tasks/steuerzentrale.md`,
//! concept `docs/settings.md`, "Einstellungen in den Apps"): what the gear of the assistant app
//! opens in its content area, and what the control centre's one long Assistant page becomes.
//!
//! ```text
//! General   the assistant on or off (shown, switched in the control centre), what a new chat
//!           starts with: engine, model, effort
//! Profile   who the assistant is, and what it keeps because it was asked to (two pages below)
//! Bots      the bots, each with "Edit" in the assistant and "Access ↗" in the control centre
//! Voice     talking by voice, speaking answers, the microphone, the programs on this computer
//! History   how long conversations are kept
//! ```
//!
//! **What is not here** (task S.17): what the assistant may use ("What the assistant may use",
//! approval, the chosen collections), the data rule for mail, and the group "Engines". Those stay
//! in the control centre, where the keys and rights live (`docs/api.md`, "Was nie nach außen
//! geht"): switching the assistant on or off is `clients.create_client` and
//! `clients.remove_client`, which only the control centre sends, so the key of a new client never
//! reaches an app process. No section here sends any `clients.*` request (a test watches over
//! that); General shows the status and points at the control centre with
//! [`crate::Action::ControlCentre`].
//!
//! While the assistant is off, every section but General says in one sentence that it stands ready
//! once it is on ([`off_group`]).
//!
//! # What the app does (task S.18)
//!
//! ```ignore
//! let (host, load) = Host::new(backend, assistant::SECTIONS.to_vec(), assistant::wanted());
//! // view: the open section's page, as the host takes it.
//! let page = assistant::page(&self.sections, host.data(), host.current().id, class);
//! let frame = host.view(class, page, Message::Settings);
//! // update: what a section returned goes through the host; its app part is this app's work.
//! match host.act(assistant::update(&mut self.sections, message, host.data())) {
//!     Outcome::App(AppAction::EditBot(id)) => self.edit_bot(id),
//!     // The answer to an `Ask` ("Say something") belongs to the section that asked.
//!     Outcome::Answer(result) => assistant::answered(&mut self.sections, &result),
//!     other => self.settings_outcome(other),
//! }
//! ```
//!
//! Escape goes to [`escape`] first (it closes a dialog of the Profile section) and only then to the
//! host, which closes the settings.

pub mod bots;
pub mod general;
pub mod history;
pub mod profile;
pub mod voice;

use iced::widget::text::Wrapping;
use iced::widget::{Space, container, svg};
use iced::{Background, Element, Theme, border};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, palette, size};
use torro_gui::{typo, widgets};
use torro_ipc::api::ASSISTANT_CLIENT;
use torro_ipc::assistant::EngineKind;
use torro_ipc::{ClientInfo, Request, Response};

use crate::parts::setting_row;
use crate::{Data, Page, Section};

/// The sections, in the order of the list. [`Section::id`] is what `--settings <section>` takes.
pub const SECTIONS: [Section; 5] = [
    // General is first, so the gear lands on the status: what is on, and what a new chat starts
    // with.
    Section { id: "general", icon: Icon::SlidersHorizontal, label: "General" },
    Section { id: "profile", icon: Icon::User, label: "Profile" },
    // The suite has no robot icon; the sparkles stand for AI (design skill, section 5).
    Section { id: "bots", icon: Icon::Sparkles, label: "Bots" },
    // The suite has no microphone icon; talking is a conversation.
    Section { id: "voice", icon: Icon::MessageCircle, label: "Voice" },
    Section { id: "history", icon: Icon::History, label: "History" },
];

/// The section of the control centre that switches the assistant on and off and holds what it may
/// use (`torroworkplace ui --section assistant`). Task S.21 makes it the assistant under Agents &
/// API; the word stays the same.
pub const CONTROL_CENTRE: &str = "assistant";

/// Where an AI account is added (`torroworkplace ui --section accounts`): where General points
/// when there is no engine at all. Task S.25 brings a section `ai`; until then the accounts page
/// is the place (task S.17).
pub const AI_SECTION: &str = "accounts";

/// What only the assistant app can do with these sections ([`crate::Action::App`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    /// Open the editor of the bot with this id: "Edit" in the Bots section. Who a bot is – its
    /// name, personality, instructions, skills and engine – is written in the assistant, not here.
    EditBot(String),
}

/// What a section of the assistant returns.
pub type Action = crate::Action<AppAction>;

/// What one load of these sections asks for, in this order; the host sends it again after every
/// change ([`crate::Host::new`]).
///
/// `clients.list_clients` is read only, to tell whether the assistant is on; nothing here writes a
/// client. The models of an AI account follow in a second round, because only the loaded settings
/// name the account ([`models_request`], [`Data::follow_up`]).
pub fn wanted() -> Vec<Request> {
    vec![
        Request::Settings,
        Request::ListClients,
        Request::GetProfile,
        Request::ListMemories,
        Request::ListBots,
        Request::ListEngines,
        Request::VoiceStatus,
    ]
}

/// `ai.list_models` for the AI account the assistant's default engine names, so General can offer
/// its models; `None` for an agent's engine (its models come with it), for the test engine and
/// while no default is set.
pub fn models_request(data: &Data) -> Option<Request> {
    let default = data.settings.assistant_default.as_ref()?;
    let engine = data.engines.iter().find(|engine| engine.id == default.engine)?;
    match engine.kind {
        EngineKind::Ai => Some(Request::AiModels { account: engine.id.clone(), all: false }),
        EngineKind::Agent | EngineKind::Test => None,
    }
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// Profile: the typed profile, a note being written, the questions over it.
    pub profile: profile::State,
    /// Voice: what "Say something" is doing and what it heard. General, Bots and History read
    /// everything from [`Data`] and keep nothing.
    pub voice: voice::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    General(general::Message),
    Profile(profile::Message),
    Bots(bots::Message),
    Voice(voice::Message),
    History(history::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::General(message) => general::update(message, data),
        Message::Profile(message) => profile::update(&mut state.profile, message, data),
        Message::Bots(message) => bots::update(message, data),
        Message::Voice(message) => voice::update(&mut state.voice, message, data),
        Message::History(message) => history::update(message, data),
    }
}

/// The page of the section with this id, as the host takes it ([`crate::Host::view`]): its body
/// and, where it has one, the page below it and the buttons of the header bar.
///
/// An id no section has shows General, the section the gear lands on; the host only ever asks for
/// one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str, class: SizeClass) -> Page<'a, Message> {
    match section {
        "profile" => wrap(profile::page(&state.profile, data, class), Message::Profile),
        "bots" => wrap(Page::new(bots::view(data, class)), Message::Bots),
        "voice" => wrap(Page::new(voice::view(&state.voice, data)), Message::Voice),
        "history" => wrap(Page::new(history::view(data)), Message::History),
        _ => wrap(Page::new(general::view(data)), Message::General),
    }
}

/// The answer to a section's [`crate::Action::Ask`], as the host handed it to the app
/// ([`crate::Outcome::Answer`]): the section that asked makes what it can of it, every other one
/// is left alone. Today one section asks – Voice, for what "Say something" heard.
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    voice::answered(&mut state.voice, result);
}

/// The dialog of the open section over the whole window, while one is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    profile::overlay(&state.profile, data).map(|dialog| dialog.map(Message::Profile))
}

/// What Escape means in these sections: close the question, stop typing a note. `None` leaves it
/// to the host, which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    profile::escape(&state.profile).map(Message::Profile)
}

/// One section's page with its messages wrapped in [`Message`].
fn wrap<'a, M: 'a>(page: Page<'a, M>, into: fn(M) -> Message) -> Page<'a, Message> {
    Page {
        body: page.body.map(into),
        sub_page: page.sub_page.map(|(name, back)| (name, into(back))),
        actions: page.actions.into_iter().map(|action| action.map(into)).collect(),
    }
}

/// The assistant's client as the daemon lists it (one client of the kind `assistant`, decision 5
/// in `docs/assistent.md`); `None` while the assistant is off.
pub fn client(data: &Data) -> Option<&ClientInfo> {
    data.clients.iter().find(|client| client.kind == ASSISTANT_CLIENT)
}

/// Whether the assistant is on: it has its client. Switched in the control centre only.
pub fn is_on(data: &Data) -> bool {
    client(data).is_some()
}

/// What a section shows while the assistant is off: its group with one row, which says in one
/// sentence that the section stands ready once the assistant is on (task S.17).
pub fn off_group<'a, M: 'a>(title: &'a str) -> Element<'a, M> {
    let detail = tr("These settings are ready once you turn it on under General.");
    let row = setting_row(
        Some(widgets::status_dot(widgets::Status::Inactive)),
        tr("The assistant is off"),
        Some(detail.to_string()),
        Space::new(),
    );
    widgets::group(title, vec![row], None)
}

/// The assistant's or a bot's picture, round, `edge` across: its symbol, else the first letter of
/// `name` (`?` without one), in `text` on `selected` – neutral, the red is the app's.
pub fn avatar<'a, M: 'a>(name: &str, avatar: &str, edge: f32) -> Element<'a, M> {
    // A symbol keeps a ring of room in a small circle.
    let symbol_edge = if edge < size::control_sm() { icon::SIZE_SM } else { icon::SIZE };
    let face: Element<'a, M> = match avatar_icon(avatar) {
        Some(symbol) => {
            icon::icon(symbol, symbol_edge).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
        }
        None => {
            let letter = name.trim().chars().next().map_or_else(|| "?".to_string(), |c| c.to_uppercase().to_string());
            let style = if edge >= size::control_lg() { typo::title() } else { typo::caption() };
            typo::text(style, letter).style(theme::text).wrapping(Wrapping::None).into()
        }
    };
    container(face)
        .center(edge)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).selected)),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// The symbol of a picture (`torro_ipc::assistant::AVATARS`); `None` for the letter and for a
/// picture this window does not know, which shows the letter too.
pub fn avatar_icon(id: &str) -> Option<Icon> {
    match id {
        "sparkles" => Some(Icon::Sparkles),
        "sun" => Some(Icon::Sun),
        "moon" => Some(Icon::Moon),
        "cloud" => Some(Icon::Cloud),
        "radar" => Some(Icon::Radar),
        "user" => Some(Icon::User),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use torro_core::i18n::trf;
    use torro_core::settings::AssistantDefault;
    use torro_ipc::assistant::{AVATARS, LETTER_AVATAR};

    use super::*;
    use crate::demo::Demo;
    use crate::{Backend, Form, Host, host};

    /// Everything the demo answers: the assistant on, with a profile, notes, a bot, engines and a
    /// voice status.
    pub(super) async fn data() -> Data {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        crate::load(backend, wanted()).await
    }

    /// The same data with the assistant off.
    pub(super) fn turned_off(data: &Data) -> Data {
        Data { clients: Vec::new(), ..data.clone() }
    }

    /// Every section builds its page in every form of the host, on and off, so no layout is left
    /// untried (design skill, section 6).
    #[tokio::test]
    async fn every_section_builds_in_every_size_and_both_states() {
        let on = data().await;
        let off = turned_off(&on);
        let mut state = State::default();
        for data in [&on, &off, &Data::default()] {
            for section in SECTIONS {
                for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip]
                {
                    let page = page(&state, data, section.id, class);
                    let _ = (page.body, page.sub_page, page.actions);
                }
            }
        }
        // The pages below Profile, with their header buttons.
        for message in [profile::Message::OpenProfile, profile::Message::OpenMemory] {
            let _ = update(&mut state, Message::Profile(message), &on);
            let page = page(&state, &on, "profile", SizeClass::Wide);
            assert!(page.sub_page.is_some(), "its name follows the section in the path");
        }
        // An id no section has is General, not an empty page.
        let unknown = page(&state, &on, "engines", SizeClass::Wide);
        let _ = unknown.body;
    }

    /// Task S.17: no section switches the assistant on or off, hands out a key, or shows what it
    /// may use or the data rule – that stays in the control centre, with the keys and the rights.
    /// Every `clients.*` request but the reading `ListClients` is named here.
    #[test]
    fn no_section_touches_clients_rights_or_the_data_rule() {
        // Spelled in pieces: this test's own words are not what it looks for.
        let forbidden = [
            ["Create", "Client"].concat(),
            ["Remove", "Client"].concat(),
            ["Update", "Client"].concat(),
            ["NewClient", "Key"].concat(),
            ["Client", "Activity"].concat(),
            ["Mail", "ToCloud"].concat(),
            ["mail_to", "_cloud"].concat(),
            ["api::", "Rights"].concat(),
        ];
        let mut found = Vec::new();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assistant");
        let Ok(entries) = std::fs::read_dir(&dir) else { panic!("no sections in {}", dir.display()) };
        let mut files = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&path).expect("a section reads");
            for line in text.lines().take_while(|line| line.trim() != "#[cfg(test)]") {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for word in &forbidden {
                    if line.contains(word.as_str()) {
                        found.push(format!("{}: {word}", path.display()));
                    }
                }
            }
        }
        assert_eq!(files, SECTIONS.len() + 1, "one file per section and this one");
        assert!(found.is_empty(), "a section reaches into the control centre's own: {found:?}");
    }

    /// What one load asks for, and the second round for an AI account's models.
    #[tokio::test]
    async fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(wanted().len(), 7, "one request per thing the sections read");
        assert!(wanted().contains(&Request::ListClients), "to tell whether the assistant is on");
        let data = data().await;
        assert!(is_on(&data) && client(&data).is_some());
        assert!(!data.default_models.is_empty(), "the second round brought the AI account's models");
        assert!(models_request(&Data::default()).is_none(), "no default, no second round");
        let ai = data.engines.iter().find(|engine| engine.kind == EngineKind::Ai).expect("the demo has one");
        assert_eq!(
            models_request(&data),
            Some(Request::AiModels { account: ai.id.clone(), all: false }),
            "the account is asked for its models"
        );
        // An agent brings its models with it; nothing to ask.
        let agent = data.engines.iter().find(|engine| engine.kind == EngineKind::Agent).expect("the demo has one");
        let mut on_agent = data.clone();
        on_agent.settings.assistant_default =
            Some(AssistantDefault { engine: agent.id.clone(), model: None, effort: None });
        assert!(models_request(&on_agent).is_none());
    }

    /// While the assistant is off, every section but General says so in one sentence.
    #[tokio::test]
    async fn the_other_sections_wait_while_the_assistant_is_off() {
        let off = turned_off(&data().await);
        assert!(!is_on(&off));
        let state = State::default();
        for section in SECTIONS {
            let page = page(&state, &off, section.id, SizeClass::Wide);
            let _ = page.body;
        }
        let _: Element<'_, Message> = off_group(tr("Profile"));
    }

    /// The answer to an `Ask` reaches the section that asked and leaves the others alone.
    #[tokio::test]
    async fn an_answer_reaches_the_section_that_asked() {
        let data = data().await;
        let mut state = State::default();
        assert_eq!(
            update(&mut state, Message::Voice(voice::Message::TestVoice), &data),
            Action::Ask(Request::TestVoice)
        );
        answered(&mut state, &Ok(torro_ipc::Response::Transcript { text: "What is on today?".into() }));
        assert_eq!(voice::heard_text(&state.voice), trf("Heard: “{text}”", &[("text", &"What is on today?")]));
    }

    /// Every picture has a symbol or shows the letter of the name.
    #[test]
    fn every_picture_is_a_symbol_or_the_letter() {
        for id in AVATARS {
            assert_eq!(avatar_icon(id).is_none(), *id == LETTER_AVATAR, "{id}");
        }
        assert!(avatar_icon("dragon").is_none());
        let _: Element<'_, Message> = avatar("torro", "dragon", size::control_sm());
        let _: Element<'_, Message> = avatar("", LETTER_AVATAR, size::control_lg());
    }

    /// The sections' ids, icons and names, as the list and `--settings <section>` use them.
    #[tokio::test]
    async fn the_sections_are_named_and_reachable_by_their_words() {
        let ids: Vec<&str> = SECTIONS.iter().map(|section| section.id).collect();
        assert_eq!(ids, ["general", "profile", "bots", "voice", "history"]);
        assert_eq!(SECTIONS[0].name(), tr("General"));
        let (mut host, _) = Host::new(Arc::new(Demo::new()), SECTIONS.to_vec(), wanted());
        for id in ids {
            assert!(host.open(id), "{id}");
        }
        assert_eq!(crate::form(SizeClass::Wide), Form::List);
        let data = data().await;
        host.update::<AppAction>(host::Message::Loaded(Box::new(data)));
        assert!(host.loaded());
    }
}
