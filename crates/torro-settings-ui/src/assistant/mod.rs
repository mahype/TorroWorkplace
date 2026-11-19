//! The settings of the assistant, in sections (task S.17 in `docs/tasks/steuerzentrale.md`,
//! concept `docs/settings.md`, "Einstellungen in den Apps"): what the gear of the assistant app
//! opens in its content area, and what the control centre's one long Assistant page becomes.
//!
//! Five sections since issue #763, what is used every day first; where a page holds more than one
//! look takes in, a row of tabs (`widgets::tabs`, the row of a mail account) splits it:
//!
//! ```text
//! General   Chats      whether the assistant is on (switched under Access), what a new chat
//!                      starts with: engine, model, effort
//!           Profile    who the assistant is, and what it keeps because it was asked to (two pages
//!                      below)
//!           History    how long conversations are kept
//! Voice     Read aloud, Conversation (talking by voice, speaking answers, the microphone),
//!           Programs (the programs on this computer)
//! Skills    guides for tasks that come again (issue #703), and below them where the assistant's own
//!           tools went: the control centre's MCP servers (task MA.7)
//! Bots      the bots, each with its rights in words, "Edit" in the assistant and "Access", which
//!           opens the bot's rights on a page below this section
//! Access    what the assistant may use, and its selections; the assistant on and off (task S.34)
//! ```
//!
//! A page below a section (the profile, the memory, a skill, a bot's rights) has no row of tabs:
//! the path above it leads back.
//!
//! **Access is the one section with rights** (task S.34, `docs/settings.md` › "Assistent und
//! Bots"): since issue #231 what the assistant and its bots may use is chosen in their own window,
//! with the rights editor of [`crate::rights`]. Only [`access`] sends `clients.*` beyond reading the
//! list – `clients.update_client`, and `clients.create_client` / `clients.remove_client` for the
//! kind `assistant` –, never `clients.new_key`; no other section shows rights. "API and MCP" ([`crate::outside`]) changes
//! only the tools and servers of the assistant and its bots. Tests watch over both
//! (`no_section_touches_clients_rights`, `access::tests`).
//!
//! While the assistant is off, every section but General's Chats and Access says in one sentence that
//! it stands ready once it is on ([`off_group`]), each tab with its own name.
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
pub mod skills;
pub mod tools;
pub mod voice;
/// Not a section: the search providers of the tool `web`, shown in the control centre's MCP servers next to
/// the other built-in tools (task MA.7 with WEB.6) and in the setup (task WEB.7).
pub mod web;

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, svg};
use iced::{Background, Element, Theme, border};

use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size};
use torro_gui::{typo, widgets};
use torro_ipc::api::ASSISTANT_CLIENT;
use torro_ipc::assistant::EngineKind;
use torro_ipc::{ClientInfo, Request, Response};

use crate::parts::setting_row;
use crate::{Data, Page, Section};

/// The sections, in the order of the list (issue #763: what is used every day first, the rights
/// last). [`Section::id`] is what `--settings <section>` takes.
pub const SECTIONS: [Section; 5] = [
    // General is first, so the gear lands on the status: what is on, and what a new chat starts
    // with.
    Section { id: "general", icon: Icon::SlidersHorizontal, label: "General" },
    // The suite has no microphone icon; talking is a conversation.
    Section { id: "voice", icon: Icon::MessageCircle, label: "Voice" },
    // What the assistant knows how to do (issue #703).
    Section { id: "skills", icon: Icon::Lightbulb, label: "Skills" },
    // The suite has no robot icon; the sparkles stand for AI (design skill, section 5).
    Section { id: bots::BOTS, icon: Icon::Sparkles, label: "Bots" },
    // Last of the assistant's own, which General points to for turning the assistant on (task
    // S.34); the icon of "Access" in a bot's menu.
    Section { id: access::ID, icon: Icon::UserCheck, label: "Access" },
];

/// The tabs of General (issue #763), in the order of the row; the index is what
/// [`Message::GeneralTab`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeneralTab {
    /// The status and what a new chat starts with ([`general`]).
    #[default]
    Chats,
    /// Who the assistant is and what it keeps ([`profile`]).
    Profile,
    /// How long conversations are kept ([`history`]).
    History,
}

impl GeneralTab {
    /// Every tab, in the order of the row.
    pub const ALL: [GeneralTab; 3] = [GeneralTab::Chats, GeneralTab::Profile, GeneralTab::History];

    /// Its word, translated.
    pub fn label(self) -> &'static str {
        match self {
            GeneralTab::Chats => tr("Chats"),
            GeneralTab::Profile => tr("Profile"),
            GeneralTab::History => tr("History"),
        }
    }
}

/// The tabs of Voice (issue #763): its three groups, each a page of its own, so the page fits the
/// window. The index is what [`Message::VoiceTab`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceTab {
    /// Reading answers aloud: provider, voice, preview.
    #[default]
    ReadAloud,
    /// Talking to the assistant: the wake word, speaking answers, the microphone and its test.
    Conversation,
    /// The models the assistant listens and speaks with (issue #762; "Programs" until then).
    Models,
}

impl VoiceTab {
    /// Every tab, in the order of the row.
    pub const ALL: [VoiceTab; 3] = [VoiceTab::ReadAloud, VoiceTab::Conversation, VoiceTab::Models];

    /// Its word, translated.
    pub fn label(self) -> &'static str {
        match self {
            VoiceTab::ReadAloud => tr("Read aloud"),
            VoiceTab::Conversation => tr("Conversation"),
            VoiceTab::Models => tr("Models"),
        }
    }
}

/// Where an AI account is added and its models are turned on and off: the section AI of the app's
/// own settings since issue #410 ([`crate::host::AI`], `--settings ai`), where General points when
/// there is no engine at all and for "Manage models…" (task S.26). The control centre has the same
/// page as `--section ai`.
pub const AI_SECTION: &str = "ai";

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
/// (the assistant's and the bots' clients).
/// `accounts.list_accounts` tells, while no AI account is an engine, whether there are AI accounts
/// with no model on (task S.26), and names the mail accounts Access offers; the calendars, task
/// lists, inbox sources, the tools (`tools.list_tools`) and the installed MCP servers
/// (`extensions.list_extensions`, task MA.10, each with a level of its own under "MCP servers") are the
/// other things it offers to choose from (task S.34, [`crate::rights::Known`]). The models of an AI account follow in a
/// second round, because only the loaded settings name the account ([`models_request`],
/// [`Data::follow_up`]).
pub fn wanted() -> Vec<Request> {
    vec![
        Request::Settings,
        Request::ListClients,
        Request::GetProfile,
        Request::ListMemories,
        Request::ListSkills,
        Request::ListBots,
        Request::ListTools,
        Request::ListEngines,
        Request::VoiceStatus,
        Request::VoiceTimings,
        Request::SpeechStatus,
        Request::ListVoiceModels,
        Request::Accounts,
        Request::CalendarCollections,
        Request::TaskCollections,
        Request::ListAddressBooks { account: None },
        Request::ListSources,
        Request::ListExtensions,
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
    let default = data.settings.ai_slots.chat.as_ref()?;
    let engine = data.engines.iter().find(|engine| engine.id == default.engine)?;
    match engine.kind {
        EngineKind::Ai => Some(Request::AiModels { account: engine.id.clone(), all: true }),
        EngineKind::Agent | EngineKind::Test => None,
    }
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// The tab of General on screen (issue #763).
    pub general_tab: GeneralTab,
    /// The tab of Voice on screen (issue #763).
    pub voice_tab: VoiceTab,
    /// General: which of its dropdowns ("Account", "Model") is open (issue #547).
    pub general: general::State,
    /// Profile: the typed profile, a note being written, the questions over it.
    pub profile: profile::State,
    /// Skills: the skill being typed, the questions over the list (issue #703).
    pub skills: skills::State,
    /// Voice: what "Say something" is doing and what it heard, and its dropdown of the microphone.
    /// Bots and History read everything from [`Data`] and keep nothing.
    pub voice: voice::State,
    /// Access: whose rights are open, the first start's choice, the editor's draft. Tools keeps
    /// nothing since task MA.7: it only leads to the control centre.
    pub access: access::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    /// A tab of General, by its place in [`GeneralTab::ALL`]: shows that part, sends nothing.
    GeneralTab(usize),
    /// A tab of Voice, by its place in [`VoiceTab::ALL`]: shows that part, sends nothing.
    VoiceTab(usize),
    General(general::Message),
    /// A control of Access, the assistant's rights ([`access::update_assistant`]).
    Access(access::Message),
    /// A control of a bot's rights, on its page below Bots (issue #763).
    BotAccess(access::Message),
    Profile(profile::Message),
    Skills(skills::Message),
    Bots(bots::Message),
    Tools(tools::Message),
    Voice(voice::Message),
    History(history::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        // A tab only changes what is shown; a dropdown of the part it leaves would hang over the
        // other one, so it closes.
        Message::GeneralTab(index) => {
            state.general_tab = GeneralTab::ALL.get(index).copied().unwrap_or_default();
            state.general.menus.close();
            Action::None
        }
        Message::VoiceTab(index) => {
            state.voice_tab = VoiceTab::ALL.get(index).copied().unwrap_or_default();
            state.voice.menus.close();
            Action::None
        }
        // The status row leads to Access: the assistant's page, not a bot's left open (issue #763).
        Message::General(general::Message::Access) => {
            access::leave_bot(&mut state.access);
            general::update(&mut state.general, general::Message::Access, data)
        }
        Message::General(message) => general::update(&mut state.general, message, data),
        Message::Access(message) => access::update_assistant(&mut state.access, message, data),
        Message::BotAccess(message) => access::update(&mut state.access, message, data),
        Message::Profile(message) => profile::update(&mut state.profile, message, data),
        Message::Skills(message) => skills::update(&mut state.skills, message, data),
        // "Access" at a bot opens its rights on a page below Bots (issue #763).
        Message::Bots(bots::Message::Access(id)) => {
            show_access(state, Some(id));
            Action::Open(bots::BOTS)
        }
        Message::Bots(message) => bots::update(message, data),
        Message::Tools(message) => tools::update(message),
        // "Models" at a model that downloads under an action (issue #823): its tab, with the progress
        // and "Cancel".
        Message::Voice(voice::Message::ShowModels) => {
            state.voice_tab = VoiceTab::Models;
            state.voice.menus.close();
            Action::None
        }
        Message::Voice(message) => voice::update(&mut state.voice, message, data),
        Message::History(message) => history::update(message, data),
    }
}

/// The page of the section with this id, as the host takes it ([`crate::Host::view`]): its body
/// and, where it has one, the page below it and the buttons of the header bar. General and Voice
/// stand under their row of tabs, a page below a section without one (issue #763).
///
/// An id no section has shows General, the section the gear lands on; the host only ever asks for
/// one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str, class: SizeClass) -> Page<'a, Message> {
    match section {
        access::ID => wrap(access::page(&state.access, data, class), Message::Access),
        "skills" => skills_page(state, data, class),
        // While the assistant is off, Bots is its one sentence, a bot's page too.
        bots::BOTS => match access::bot_page(&state.access, data).filter(|_| is_on(data)) {
            Some(page) => wrap(page, Message::BotAccess),
            None => {
                let rights = |bot: &torro_ipc::bots::BotSummary| access::bot_rights(&state.access, data, bot);
                wrap(Page::new(bots::view(data, class, rights)), Message::Bots)
            }
        },
        "voice" => {
            let body = voice::view(&state.voice, data, state.voice_tab, class).map(Message::Voice);
            under_tabs(
                Page::new(body),
                &VoiceTab::ALL.map(VoiceTab::label),
                state.voice_tab as usize,
                Message::VoiceTab,
            )
        }
        _ => {
            let inner = match state.general_tab {
                GeneralTab::Chats => wrap(Page::new(general::view(&state.general, data)), Message::General),
                GeneralTab::Profile => wrap(profile::page(&state.profile, data, class), Message::Profile),
                GeneralTab::History => wrap(Page::new(history::view(data)), Message::History),
            };
            let labels = GeneralTab::ALL.map(GeneralTab::label);
            under_tabs(inner, &labels, state.general_tab as usize, Message::GeneralTab)
        }
    }
}

/// Skills: the list, and under it where the tools went ([`tools::view`], issue #763); the editor of a
/// skill alone.
fn skills_page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Page<'a, Message> {
    let page = wrap(skills::page(&state.skills, data, class), Message::Skills);
    // While the assistant is off the section is its one sentence, as every other.
    if page.sub_page.is_some() || !is_on(data) {
        return page;
    }
    let body = column![page.body, tools::view().map(Message::Tools)].spacing(layout::section_gap());
    Page { body: body.into(), ..page }
}

/// A section's page under its row of tabs, `chosen` the tab on screen; a page below the section
/// (its [`Page::sub_page`]) is handed on as it is, since the path above it leads back. The page's
/// buttons and save bar stay its own.
fn under_tabs<'a>(
    page: Page<'a, Message>,
    labels: &[&'static str],
    chosen: usize,
    on_select: fn(usize) -> Message,
) -> Page<'a, Message> {
    if page.sub_page.is_some() {
        return page;
    }
    let labels = labels.iter().map(|label| (label.to_string(), None)).collect();
    let body = column![widgets::tabs(labels, chosen, on_select), page.body].spacing(layout::section_gap());
    Page { body: body.into(), ..page }
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

/// Opens the rights of the assistant (`None`) or of the bot with this id ([`access::show`]); the app
/// shows the section itself: `Host::open(access::ID)` for the assistant, `Host::open(bots::BOTS)` for
/// a bot, whose rights are a page below Bots since issue #763. For "Change access" under a refused
/// tool, the bot editor's "What the bot may use" and `--settings access [--bot <id>]`.
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
/// (`assistant::received(&mut sections, &message); host.update(message)`): a send that came back,
/// a load, and Access chosen in the list. The host gives a section neither the answers of its requests nor the fresh data;
/// Access needs both to settle the editor's draft and to see its client appear ([`access::sent`],
/// [`access::loaded`]). Every other message is left alone.
pub fn received(state: &mut State, message: &crate::host::Message) {
    match message {
        // Access chosen in the list shows the assistant's page (issue #763).
        crate::host::Message::Select(access::ID) => access::leave_bot(&mut state.access),
        crate::host::Message::Sent { changed, result } => {
            access::sent(&mut state.access, *changed, result);
            if let Ok(data) = &**result {
                dropdowns_loaded(state, data);
            }
        }
        crate::host::Message::Loaded(data) => {
            access::loaded(&mut state.access, data);
            dropdowns_loaded(state, data);
        }
        _ => {}
    }
}

/// Fresh data reached General and Voice (issue #547): a dropdown that can no longer show closes.
fn dropdowns_loaded(state: &mut State, data: &Data) {
    general::loaded(&mut state.general, data);
    voice::loaded(&mut state.voice, data);
}

/// The answer to a section's [`crate::Action::Ask`], as the host handed it to the app
/// ([`crate::Outcome::Answer`]): the section that asked makes what it can of it, every other one
/// is left alone. Two sections ask – Voice, for what "Say something" heard, and Skills, for the
/// text of the skill "Edit" opens (issue #703).
pub fn answered(state: &mut State, result: &Result<Response, String>) {
    voice::answered(&mut state.voice, result);
    skills::answered(&mut state.skills, result);
}

/// The dialog of the open section over the whole window, while one is open: an open dropdown of
/// General or Voice first (issue #547), then a dialog of Profile or Access. A dropdown closes with a
/// click next to it, so the one open belongs to the section on screen.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    general::overlay(&state.general, data)
        .map(|menu| menu.map(Message::General))
        .or_else(|| voice::overlay(&state.voice, data).map(|menu| menu.map(Message::Voice)))
        .or_else(|| profile::overlay(&state.profile, data).map(|dialog| dialog.map(Message::Profile)))
        .or_else(|| skills::overlay(&state.skills, data).map(|dialog| dialog.map(Message::Skills)))
        .or_else(|| access::overlay(&state.access, data).map(|dialog| dialog.map(Message::Access)))
}

/// What Escape means in these sections: close an open dropdown, the question, stop typing a note.
/// `None` leaves it to the host, which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    general::escape(&state.general)
        .map(Message::General)
        .or_else(|| voice::escape(&state.voice).map(Message::Voice))
        .or_else(|| profile::escape(&state.profile).map(Message::Profile))
        .or_else(|| skills::escape(&state.skills).map(Message::Skills))
        .or_else(|| access::escape(&state.access).map(Message::Access))
}

/// What stands between the window and closing these settings: the question a section must ask
/// before its unsaved work is dropped ([`profile::closing`], "Discard changes?"). `None` lets the
/// window close them.
///
/// The window asks this on every way out – the gear, Escape, a click in the sidebar – and sends
/// the message back as any other; after "Discard" the section closes the settings itself
/// ([`crate::Action::Close`]), so the question and the closing stay in one place.
pub fn closing(state: &State, data: &Data) -> Option<Message> {
    profile::closing(&state.profile, data)
        .map(Message::Profile)
        .or_else(|| skills::closing(&state.skills).map(Message::Skills))
}

/// Whether `message` leaves the page open below a section for the section's own ("‹ Back" from a
/// bot, the profile or the memory): what a row of the host's section menu sends of these pages, after
/// which the app closes the menu (`Host::close_menu`, issue #473). Ticks and typing never do.
pub fn leaves_page(message: &Message) -> bool {
    matches!(
        message,
        Message::Access(access::Message::Back)
            | Message::BotAccess(access::Message::Back)
            | Message::Profile(profile::Message::Back)
    ) || matches!(message, Message::Skills(message) if skills::leaves_page(message))
}

/// One section's page with its messages wrapped in [`Message`].
fn wrap<'a, M: 'a>(page: Page<'a, M>, into: fn(M) -> Message) -> Page<'a, Message> {
    page.map(into)
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
            max_output: None,
            parameters: None,
            quantization: None,
            size: None,
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

    /// Every section builds its page with every tab in every form of the host, on and off, so no
    /// layout is left untried (design skill, section 6).
    #[tokio::test]
    async fn every_section_builds_in_every_size_and_both_states() {
        let on = data().await;
        let off = turned_off(&on);
        let mut state = State::default();
        for data in [&on, &off, &Data::default()] {
            for tab in 0..GeneralTab::ALL.len() {
                let _ = update(&mut state, Message::GeneralTab(tab), data);
                let _ = update(&mut state, Message::VoiceTab(tab), data);
                for section in SECTIONS {
                    for class in
                        [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip]
                    {
                        let page = page(&state, data, section.id, class);
                        assert!(page.sub_page.is_none(), "{}: the section's own page", section.id);
                        let _ = (page.body, page.actions);
                    }
                }
            }
        }
        // The pages below General › Profile, with their header buttons and without the row of tabs.
        let _ = update(&mut state, Message::GeneralTab(1), &on);
        for message in [profile::Message::OpenProfile, profile::Message::OpenMemory] {
            let _ = update(&mut state, Message::Profile(message), &on);
            let page = page(&state, &on, "general", SizeClass::Wide);
            assert!(page.sub_page.is_some(), "its name follows the section in the path");
        }
        // A skill's editor stands alone too, without the tools under it.
        let _ = update(&mut state, Message::Skills(skills::Message::Add), &on);
        assert!(page(&state, &on, "skills", SizeClass::Wide).sub_page.is_some());
        // An id no section has is General, not an empty page.
        let unknown = page(&state, &on, "engines", SizeClass::Wide);
        let _ = unknown.body;
    }

    /// Task S.17, narrowed by task S.34: no section but Access switches the assistant on or off,
    /// changes what a client may use, and none ever hands out a key. Every
    /// `clients.*` request but the reading `ListClients` is named here, and the rights editor
    /// (`rights::`, which holds `UpdateClient` and `api::Rights`); Access is watched by its own
    /// test (`access::tests::access_sends_only_what_it_may`), which allows it what the boundary
    /// does and never a new key.
    #[test]
    fn no_section_touches_clients_rights() {
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
        // Five sections, this file, and the parts that are tabs or groups of a section since issue
        // #763 (profile, history, tools) or no section at all (web).
        assert_eq!(files, SECTIONS.len() + 5, "one file per section and part, this one and the web view");
        assert!(found.is_empty(), "a section reaches into the rights: {found:?}");
    }

    /// What one load asks for, and the second round for an AI account's models.
    #[tokio::test]
    async fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(wanted().len(), 18, "one request per thing the sections read");
        assert!(wanted().contains(&Request::VoiceTimings), "Voice shows the response times (issue #828)");
        for offered in [
            Request::CalendarCollections,
            Request::TaskCollections,
            Request::ListAddressBooks { account: None },
            Request::ListSources,
            Request::ListTools,
            Request::ListExtensions,
        ] {
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
        on_agent.settings.ai_slots.chat =
            Some(AssistantDefault { engine: agent.id.clone(), model: None, effort: None });
        assert!(models_request(&on_agent).is_none());
    }

    /// While the assistant is off, every section but General and Access says so in one sentence,
    /// each tab under its own name.
    #[tokio::test]
    async fn the_other_sections_wait_while_the_assistant_is_off() {
        let off = turned_off(&data().await);
        assert!(!is_on(&off));
        let mut state = State::default();
        for tab in 0..VoiceTab::ALL.len() {
            let _ = update(&mut state, Message::VoiceTab(tab), &off);
            let _ = update(&mut state, Message::GeneralTab(tab), &off);
            for section in SECTIONS {
                let page = page(&state, &off, section.id, SizeClass::Wide);
                let _ = page.body;
            }
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

    /// Task S.34, issue #763: "Access" at a bot opens its rights on a page below Bots; Access chosen
    /// in the list shows the assistant's again; the host's sends and loads reach Access, every other
    /// message passes by.
    #[tokio::test]
    async fn access_opens_at_a_bot_and_follows_the_host() {
        let data = data().await;
        let bot = data.bots.first().expect("the demo has a bot").id.clone();
        let mut state = State::default();
        let action = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        assert_eq!(action, Action::Open(bots::BOTS));
        assert_eq!(access::open_bot(&state.access, &data).map(|open| open.id.clone()), Some(bot.clone()));
        let below = page(&state, &data, bots::BOTS, SizeClass::Wide).sub_page;
        let name = data.bots.first().map(|summary| summary.name.clone());
        assert_eq!(below.map(|(name, _)| name), name, "the bot's name follows Bots in the path");
        assert!(page(&state, &data, access::ID, SizeClass::Wide).sub_page.is_none(), "Access is the assistant's");
        // A change on the bot's page goes to the bot; one on Access – reached by any way while the
        // bot's page is still open – to the assistant, and the bot's waiting draft goes first.
        let level = |area| crate::rights::Message::Level(area, torro_ipc::api::Level::Read);
        let bot_client = data.bots.first().map(|summary| summary.client.clone()).expect("its client");
        let assistant = client(&data).map(|info| info.id.clone()).expect("on");
        let _ =
            update(&mut state, Message::BotAccess(access::Message::Rights(level(torro_ipc::api::Area::Tasks))), &data);
        let Action::Send(first) = update(&mut state, Message::Access(access::Message::Tab(0)), &data) else {
            panic!("the bot's draft is saved")
        };
        assert!(matches!(&first[..], [Request::UpdateClient { id, .. }] if *id == bot_client), "{first:?}");
        assert!(access::open_bot(&state.access, &data).is_none(), "Access shows the assistant's rights");
        assert!(page(&state, &data, bots::BOTS, SizeClass::Wide).sub_page.is_none(), "Bots shows its list");
        let _ = update(&mut state, Message::Access(access::Message::Rights(level(torro_ipc::api::Area::Mail))), &data);
        let Action::Send(then) = save_now(&mut state) else { panic!("the assistant's draft waits") };
        assert!(matches!(&then[..], [Request::UpdateClient { id, .. }] if *id == assistant), "{then:?}");
        let _ = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        assert!(page(&state, &turned_off(&data), bots::BOTS, SizeClass::Wide).sub_page.is_none(), "off: no bot page");
        received(&mut state, &host::Message::Select(access::ID));
        assert!(access::open_bot(&state.access, &data).is_none(), "Access chosen in the list");
        let _ = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        assert_eq!(update(&mut state, Message::General(general::Message::Access), &data), Action::Open(access::ID));
        assert!(access::open_bot(&state.access, &data).is_none(), "Access from General's status row");
        let _ = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        show_access(&mut state, None);
        assert!(access::open_bot(&state.access, &data).is_none());
        // Tools only leads to the control centre (task MA.7), whichever bot Access showed last.
        let _ = update(&mut state, Message::Bots(bots::Message::Access(bot.clone())), &data);
        let tools = update(&mut state, Message::Tools(tools::Message::Open), &data);
        assert_eq!(tools, Action::ControlCentre(tools::CONTROL_CENTRE_SECTION.to_string()));

        // A change of rights waits for the clock; leaving the settings sends it now.
        let level = access::Message::Rights(crate::rights::Message::Level(
            torro_ipc::api::Area::Calendar,
            torro_ipc::api::Level::Write,
        ));
        let _ = update(&mut state, Message::Access(level), &data);
        let Action::Send(sent) = save_now(&mut state) else { panic!("a draft waits") };
        assert!(matches!(&sent[..], [Request::UpdateClient { .. }]), "{sent:?}");
        assert_eq!(save_now(&mut state), Action::None, "once");
        received(&mut state, &host::Message::Sent { changed: true, result: Box::new(Ok(data.clone())) });
        received(&mut state, &host::Message::Menu);
        assert!(overlay(&state, &data).is_none() && escape(&state).is_none());
    }

    /// Issue #763: the tabs of General and Voice show a part and send nothing; an index past the row
    /// is the first tab, and a tab closes a dropdown that stood open.
    #[tokio::test]
    async fn the_tabs_show_a_part_close_a_dropdown_and_send_nothing() {
        let data = data().await;
        let mut state = State::default();
        assert_eq!((state.general_tab, state.voice_tab), (GeneralTab::Chats, VoiceTab::ReadAloud));
        for (index, tab) in GeneralTab::ALL.into_iter().enumerate() {
            assert_eq!(update(&mut state, Message::GeneralTab(index), &data), Action::None);
            assert_eq!(state.general_tab, tab);
        }
        for (index, tab) in VoiceTab::ALL.into_iter().enumerate() {
            assert_eq!(update(&mut state, Message::VoiceTab(index), &data), Action::None);
            assert_eq!(state.voice_tab, tab);
        }
        let _ = update(&mut state, Message::GeneralTab(9), &data);
        let _ = update(&mut state, Message::VoiceTab(9), &data);
        assert_eq!((state.general_tab, state.voice_tab), (GeneralTab::Chats, VoiceTab::ReadAloud));

        let toggle = |choice| voice::Message::Menus(torro_gui::widgets::DropdownEvent::Toggle(choice));
        let _ = update(&mut state, Message::Voice(toggle(voice::Choice::Provider)), &data);
        assert!(escape(&state).is_some(), "the dropdown is open");
        let _ = update(&mut state, Message::VoiceTab(1), &data);
        assert!(escape(&state).is_none(), "the tab closed it");
        // Issue #823: "Models" at a model that downloads under an action shows its tab, sends nothing.
        let _ = update(&mut state, Message::Voice(toggle(voice::Choice::Recognizer)), &data);
        assert_eq!(update(&mut state, Message::Voice(voice::Message::ShowModels), &data), Action::None);
        assert_eq!(state.voice_tab, VoiceTab::Models);
        assert!(escape(&state).is_none(), "and closes a dropdown");
        let toggle = |choice| general::Message::Menus(torro_gui::widgets::DropdownEvent::Toggle(choice));
        let _ = update(&mut state, Message::General(toggle(general::Choice::Engine)), &data);
        assert!(escape(&state).is_some(), "the dropdown is open");
        let _ = update(&mut state, Message::GeneralTab(2), &data);
        assert!(escape(&state).is_none(), "the tab closed it");
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
        assert_eq!(ids, ["general", "voice", "skills", "bots", "access"]);
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
