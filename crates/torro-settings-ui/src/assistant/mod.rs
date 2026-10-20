//! The settings of the assistant, in sections (task S.17 in `docs/tasks/steuerzentrale.md`,
//! concept `docs/settings.md`, "Einstellungen in den Apps"): what the gear of the assistant app
//! opens in its content area, and what the control centre's one long Assistant page becomes.
//!
//! ```text
//! General   whether the assistant is on (switched under Access), what a new chat starts with:
//!           engine, model, effort
//! Access    what the assistant and each bot may use; the assistant on and off (task S.34)
//! Profile   who the assistant is, and what it keeps because it was asked to (two pages below)
//! Bots      the bots, each with "Edit" in the assistant and "Access" here
//! Voice     talking by voice, speaking answers, the microphone, the programs on this computer
//! History   how long conversations are kept
//! ```
//!
//! **Access is the one section with rights** (task S.34, `docs/settings.md` › "Assistent und
//! Bots"): since issue #231 what the assistant and its bots may use is chosen in their own window,
//! with the rights editor of [`crate::rights`]. Only [`access`] sends `clients.*` beyond reading the
//! list – `clients.update_client`, and `clients.create_client` / `clients.remove_client` for the
//! kind `assistant` –, never `clients.new_key`; no other section shows rights or the data rule for
//! mail, which lives under the control centre's Accounts. Tests watch over both
//! (`no_section_touches_clients_rights_or_the_data_rule`, `access::tests`).
//!
//! While the assistant is off, every section but General and Access says in one sentence that it
//! stands ready once it is on ([`off_group`]).
//!
//! # What the app does (task S.18)
//!
//! ```ignore
//! let (host, load) = Host::new(backend, assistant::SECTIONS.to_vec(), assistant::wanted());
//! // view: the open section's page, as the host takes it.
//! let page = assistant::page(&self.sections, host.data(), host.current().id, class);
//! let frame = host.view(class, page, Message::Settings);
//! // update: the host's own messages go past the sections first (Access follows its sends).
//! assistant::received(&mut self.sections, &message);
//! let outcome = host.update(message);
//! // What a section returned goes through the host; its app part is this app's work.
//! match host.act(assistant::update(&mut self.sections, message, host.data())) {
//!     Outcome::App(AppAction::EditBot(id)) => self.edit_bot(id),
//!     // The answer to an `Ask` ("Say something") belongs to the section that asked.
//!     Outcome::Answer(result) => assistant::answered(&mut self.sections, &result),
//!     other => self.settings_outcome(other),
//! }
//! ```
//!
//! Escape goes to [`escape`] first (it closes a dialog of Profile or Access) and only then to the
//! host, which closes the settings – but every way out of the settings passes [`closing`] first, so
//! a profile that is typed and not saved is never dropped without "Discard changes?", and
//! [`save_now`] then, so a change of rights made a moment ago goes out before the sections go. The
//! window's own subscription batches [`subscription`], the clocks the open sections need (Voice
//! asks how the listener stands after it was switched, Access saves its draft).

pub mod access;
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
pub const SECTIONS: [Section; 6] = [
    // General is first, so the gear lands on the status: what is on, and what a new chat starts
    // with.
    Section { id: "general", icon: Icon::SlidersHorizontal, label: "General" },
    // Next to General, which points here to turn the assistant on (task S.34); the icon of
    // "Access…" in a bot's menu.
    Section { id: access::ID, icon: Icon::UserCheck, label: "Access" },
    Section { id: "profile", icon: Icon::User, label: "Profile" },
    // The suite has no robot icon; the sparkles stand for AI (design skill, section 5).
    Section { id: "bots", icon: Icon::Sparkles, label: "Bots" },
    // The suite has no microphone icon; talking is a conversation.
    Section { id: "voice", icon: Icon::MessageCircle, label: "Voice" },
    Section { id: "history", icon: Icon::History, label: "History" },
];

/// Where an AI account is added and its models are turned on and off
/// (`torroworkplace ui --section accounts`): where General points when there is no engine at all
/// and for "Manage models…" (task S.26). There is no section `ai` (issue #231).
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
/// `clients.list_clients` tells whether the assistant is on and holds the rights Access edits
/// (the assistant's and the bots' clients). `accounts.list_accounts` tells, while no AI account is
/// an engine, whether there are AI accounts with no model on (task S.26), and names the mail
/// accounts Access offers; the calendars, task lists and inbox sources are the other things it
/// offers to choose from (task S.34, [`crate::rights::Known`]). The models of an AI account follow
/// in a second round, because only the loaded settings name the account ([`models_request`],
/// [`Data::follow_up`]).
pub fn wanted() -> Vec<Request> {
    vec![
        Request::Settings,
        Request::ListClients,
        Request::GetProfile,
        Request::ListMemories,
        Request::ListBots,
        Request::ListEngines,
        Request::VoiceStatus,
        Request::Accounts,
        Request::CalendarCollections,
        Request::TaskCollections,
        Request::ListSources,
    ]
}

/// Whether there are AI accounts but none of them is an engine (task S.26): no model is turned on
/// at any of them, so the core lists none (`assistant.list_engines`).
pub fn no_model_on(data: &Data) -> bool {
    let ai_engine = data.engines.iter().any(|engine| engine.kind == EngineKind::Ai);
    !ai_engine && data.accounts.iter().any(|account| account.kind == "ai")
}

/// `ai.list_models` for the AI account the assistant's default engine names, so General can offer
/// its models; `None` for an agent's engine (its models come with it), for the test engine and
/// while no default is set. Closed models too: General offers the models turned on (task S.26),
/// and a closed model may be one of them.
pub fn models_request(data: &Data) -> Option<Request> {
    let default = data.settings.assistant_default.as_ref()?;
    let engine = data.engines.iter().find(|engine| engine.id == default.engine)?;
    match engine.kind {
        EngineKind::Ai => Some(Request::AiModels { account: engine.id.clone(), all: true }),
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
    /// Access: whose rights are open, the first start's choice, the editor's draft.
    pub access: access::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    General(general::Message),
    Access(access::Message),
    Profile(profile::Message),
    Bots(bots::Message),
    Voice(voice::Message),
    History(history::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::General(message) => general::update(message, data),
        Message::Access(message) => access::update(&mut state.access, message, data),
        Message::Profile(message) => profile::update(&mut state.profile, message, data),
        // "Access" at a bot opens this section at that very bot.
        Message::Bots(bots::Message::Access(id)) => {
            show_access(state, Some(id));
            Action::Open(access::ID)
        }
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
        access::ID => wrap(access::page(&state.access, data, class), Message::Access),
        "profile" => wrap(profile::page(&state.profile, data, class), Message::Profile),
        "bots" => wrap(Page::new(bots::view(data, class)), Message::Bots),
        "voice" => wrap(Page::new(voice::view(&state.voice, data)), Message::Voice),
        "history" => wrap(Page::new(history::view(data)), Message::History),
        _ => wrap(Page::new(general::view(data)), Message::General),
    }
}

/// What the open sections need a clock for, as the app batches it into the window's own
/// subscription (`Subscription::batch([…, settings::subscription(&pane.sections).map(…)])`); none
/// while nothing waits. Voice asks how the listener stands again after it was switched
/// ([`voice::subscription`]), Access saves a changed right [`crate::rights::SAVE_DELAY`] later
/// ([`access::subscription`]).
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    iced::Subscription::batch([
        voice::subscription(&state.voice).map(Message::Voice),
        access::subscription(&state.access).map(Message::Access),
    ])
}

/// Opens Access at the assistant (`None`) or at the bot with this id ([`access::show`]); the app
/// shows the section itself (`Host::open(access::ID)`). For "Change access" under a refused tool,
/// the bot editor's "What the bot may use" and `--settings access [--bot <id>]`.
pub fn show_access(state: &mut State, bot: Option<String>) {
    access::show(&mut state.access, bot);
}

/// What the sections still have to send before the settings close: a change of rights made a
/// moment ago and not yet saved ([`access::save_now`]). The window hands it to the host as any
/// other action on every way out of the settings, so it is not lost with them;
/// [`crate::Action::None`] while nothing waits.
pub fn save_now(state: &mut State) -> Action {
    access::save_now(&mut state.access)
}

/// One of the host's own messages, as the app hands it on **before** the host takes it
/// (`assistant::received(&mut sections, &message); host.update(message)`): a send that came back
/// and a load. The host gives a section neither the answers of its requests nor the fresh data;
/// Access needs both to settle the editor's draft and to see its client appear ([`access::sent`],
/// [`access::loaded`]). Every other message is left alone.
pub fn received(state: &mut State, message: &crate::host::Message) {
    match message {
        crate::host::Message::Sent { changed, result } => access::sent(&mut state.access, *changed, result),
        crate::host::Message::Loaded(data) => access::loaded(&mut state.access, data),
        _ => {}
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
    profile::overlay(&state.profile, data)
        .map(|dialog| dialog.map(Message::Profile))
        .or_else(|| access::overlay(&state.access, data).map(|dialog| dialog.map(Message::Access)))
}

/// What Escape means in these sections: close the question, stop typing a note. `None` leaves it
/// to the host, which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    profile::escape(&state.profile).map(Message::Profile).or_else(|| access::escape(&state.access).map(Message::Access))
}

/// What stands between the window and closing these settings: the question a section must ask
/// before its unsaved work is dropped ([`profile::closing`], "Discard changes?"). `None` lets the
/// window close them.
///
/// The window asks this on every way out – the gear, Escape, a click in the sidebar – and sends
/// the message back as any other; after "Discard" the section closes the settings itself
/// ([`crate::Action::Close`]), so the question and the closing stay in one place.
pub fn closing(state: &State, data: &Data) -> Option<Message> {
    profile::closing(&state.profile, data).map(Message::Profile)
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

/// Whether the assistant is on: it has its client. Switched under Access ([`access`]).
pub fn is_on(data: &Data) -> bool {
    client(data).is_some()
}

/// What a section shows while the assistant is off: its group with one row, which says in one
/// sentence that the section stands ready once the assistant is on (task S.17).
pub fn off_group<'a, M: 'a>(title: &'a str) -> Element<'a, M> {
    let detail = tr("These settings are ready once you turn it on under Access.");
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

    /// A coding agent as an engine, with its fixed models (issue #131). The demo offers none since
    /// task C.19, issue #219 — the product does not — but `EngineKind::Agent` stays in the
    /// protocol, so the pages must still be able to show one: the tests bring their own.
    pub(super) fn agent_engine() -> torro_ipc::assistant::Engine {
        let model = |id: &str, name: &str, reasoning| torro_core::ai::AiModel {
            id: id.into(),
            name: name.into(),
            tools: Some(true),
            open_weights: None,
            context: None,
            reasoning,
            remote: None,
            decision: None,
            enabled: false,
        };
        torro_ipc::assistant::Engine {
            id: "claude-code".into(),
            kind: EngineKind::Agent,
            name: "Claude Code".into(),
            problem: None,
            models: vec![
                model("opus", "Opus", Some(true)),
                model("sonnet", "Sonnet", Some(true)),
                model("haiku", "Haiku", Some(false)),
            ],
        }
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

    /// Task S.17, narrowed by task S.34: no section but Access switches the assistant on or off,
    /// changes what a client may use or shows the data rule, and none ever hands out a key. Every
    /// `clients.*` request but the reading `ListClients` is named here, and the rights editor
    /// (`rights::`, which holds `UpdateClient` and `api::Rights`); Access is watched by its own
    /// test (`access::tests::access_sends_only_what_it_may`), which allows it what the boundary
    /// does and never a new key.
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
            ["rights", "::"].concat(),
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
            // The one section with rights, under a test of its own.
            if path.file_stem().is_some_and(|stem| stem == access::ID) {
                continue;
            }
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
        assert!(found.is_empty(), "a section reaches into the rights: {found:?}");
    }

    /// What one load asks for, and the second round for an AI account's models.
    #[tokio::test]
    async fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(wanted().len(), 11, "one request per thing the sections read");
        for offered in [Request::CalendarCollections, Request::TaskCollections, Request::ListSources] {
            assert!(wanted().contains(&offered), "what Access offers to choose from (task S.34)");
        }
        assert!(wanted().contains(&Request::ListClients), "to tell whether the assistant is on");
        assert!(wanted().contains(&Request::Accounts), "to tell no model on from no AI account (task S.26)");
        let data = data().await;
        assert!(is_on(&data) && client(&data).is_some());
        assert!(!data.default_models.is_empty(), "the second round brought the AI account's models");
        assert!(models_request(&Data::default()).is_none(), "no default, no second round");
        let ai = data.engines.iter().find(|engine| engine.kind == EngineKind::Ai).expect("the demo has one");
        assert_eq!(
            models_request(&data),
            Some(Request::AiModels { account: ai.id.clone(), all: true }),
            "the account is asked for its models, closed ones too (task S.26)"
        );
        // An agent brings its models with it; nothing to ask.
        let agent = agent_engine();
        let mut on_agent = data.clone();
        on_agent.engines.push(agent.clone());
        on_agent.settings.assistant_default =
            Some(AssistantDefault { engine: agent.id.clone(), model: None, effort: None });
        assert!(models_request(&on_agent).is_none());
    }

    /// While the assistant is off, every section but General and Access says so in one sentence.
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

    /// Task S.34: "Access" at a bot opens the section at that bot; the host's sends and loads reach
    /// Access, every other message passes by.
    #[tokio::test]
    async fn access_opens_at_a_bot_and_follows_the_host() {
        let data = data().await;
        let bot = data.bots.first().expect("the demo has a bot").id.clone();
        let mut state = State::default();
        let action = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        assert_eq!(action, Action::Open(access::ID));
        assert_eq!(access::open_bot(&state.access, &data).map(|open| open.id.clone()), Some(bot));
        let below = page(&state, &data, access::ID, SizeClass::Wide).sub_page.is_some();
        assert!(below, "the bot's name follows Access in the path");
        show_access(&mut state, None);
        assert!(access::open_bot(&state.access, &data).is_none());

        // A change of rights waits for the clock; leaving the settings sends it now.
        let level = access::Message::Rights(crate::rights::Message::Level(
            torro_ipc::api::Area::Mail,
            torro_ipc::api::Level::Read,
        ));
        let _ = update(&mut state, Message::Access(level), &data);
        let Action::Send(sent) = save_now(&mut state) else { panic!("a draft waits") };
        assert!(matches!(&sent[..], [Request::UpdateClient { .. }]), "{sent:?}");
        assert_eq!(save_now(&mut state), Action::None, "once");
        received(&mut state, &host::Message::Sent { changed: true, result: Box::new(Ok(data.clone())) });
        received(&mut state, &host::Message::Menu);
        assert!(overlay(&state, &data).is_none() && escape(&state).is_none());
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
        assert_eq!(ids, ["general", "access", "profile", "bots", "voice", "history"]);
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
