//! The mini chat about a mail (issue #605, "Chat zu einer Mail" in `docs/assistent.md`, mock-up
//! `docs/prototypes/mail-mini-chat/`): "Ask AI…" in the rail's group "Torro" opens a panel in which
//! the person asks the assistant about the open mail. It is a conversation of the assistant with the
//! mail as its `about`; the core reads the mail first, lets the answer only read and suggest, and a
//! suggestion it reads out (`mail.extract_action`) comes back as a card whose "Accept" opens the
//! filled dialog of issue #603 in this window.
//!
//! - **State** ([`Ask`]): the mail, the conversation as a `torro_gui::chat::Transcript`, its resolved
//!   cards, the field and what answers (the assistant's default, `chat::preferred_choice`).
//! - **Requests only** ([`Call`], [`call_for`]): opening asks `assistant.list_engines`,
//!   `settings.get_settings` and `assistant.list_conversations` and continues the newest conversation
//!   about the same mail ([`newest_about`]); "+" starts a new one. Before the chat shows it asks
//!   `assistant.check_about` with what would answer ([`check_about`]), so a hint stands at once instead of
//!   after the first message. "Send" is
//!   `assistant.send_message` (with `about` for a new conversation, [`send_request`]); while an answer
//!   runs the window asks `assistant.get_conversation` with `after` every [`FOLLOW`], and the cards
//!   come from `assistant.get_cards`. The window reads and writes nothing itself; with `--demo` a
//!   small stand-in answers ([`demo_call`]).
//! - **Answers belong to one chat** ([`Round`]): every answer of the core carries the round it was
//!   asked in – a number new for each panel and each "+" –, and an answer of another round (another
//!   mail's panel, the conversation before "+") is dropped, never merged into the chat open now.
//! - **Hints instead of a chat** ([`Hint`], [`hint_for`]): no AI set up, the assistant may not read
//!   the mail (`forbidden`, with "Open assistant"), or a model that cannot chat about a mail
//!   (`unsupported`) – on opening, or when "Send" is refused. No data rule keeps the mail from a model
//!   (issue #664).
//! - **Where it stands** (the app decides, `App::ask_layout`): wide beside the mail, medium and
//!   narrow over it with "‹ Mail" back ([`view`]).
//!
//! Everything in the conversation but the person's own words – answers, tool results, the cards'
//! text – comes from the mail and the model: data to show, never instructions.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Days, NaiveTime, Utc, Weekday};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, rule, scrollable, svg, text_input};
use iced::{Alignment, Element, Fill, Padding, Task, Theme};
use torro_core::ai::Effort;
use torro_core::i18n::{tr, trf};
use torro_gui::chat::{self, Choice, Transcript};
use torro_gui::icon::{self, Icon};
use torro_gui::markdown;
use torro_gui::source::Source;
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets;
use torro_gui::{datetime, style, typo};
use torro_ipc::actions::{ActionKind, Suggestion, SuggestionState};
use torro_ipc::assistant::{
    Card, CardEntry, CardRef, ConversationSummary, Engine, EngineKind, Entry, EntryContent, MailRef, Mode,
};
use torro_ipc::{EventSuggestion, Request, Response};
use torro_mail::MessageSummary;
use torro_pim::EventTime;

use crate::app::Message;
use crate::mail;

/// How often the panel asks for a running answer (`assistant.get_conversation` with `after`). Slower
/// than the assistant's window, which lets the answer appear letter by letter; here it comes in
/// pieces, and the panel rests between them.
pub const FOLLOW: Duration = Duration::from_millis(300);

/// The id of the field, so opening the panel puts the cursor there.
pub const FIELD: &str = "ask-ai";

/// The round an answer of the core belongs to: new for every panel ([`Ask::open`]) and every "+"
/// ([`Event::New`]), unique in the process, so an answer that comes late – after the panel moved to
/// another mail or started anew – is told apart from the answers of the chat open now and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Round(u64);

impl Round {
    /// A round no panel had before.
    fn next() -> Round {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Round(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// The questions an empty chat offers, in English; [`view`] translates them.
pub const STARTERS: [&str; 3] =
    ["What does the sender want from me?", "Is there a deadline?", "What was said about this before?"];

/// A request the core refused: its code (`forbidden`, `not-connected` …) and its words for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    /// A refusal of the window's own, for an answer it did not expect or a core it could not reach.
    fn error(message: impl Into<String>) -> Refusal {
        Refusal { code: "error".to_owned(), message: message.into() }
    }

    /// An answer that is not the one asked for.
    fn unexpected(response: &Response) -> Refusal {
        Refusal::error(format!("unexpected answer: {response:?}"))
    }
}

/// The answer to one request: its response, or the core's refusal with its code.
pub type Answer = Pin<Box<dyn Future<Output = Result<Response, Refusal>> + Send>>;

/// How the panel asks the core: one request, one answer ([`call_for`]). Shared, so a test can hand
/// in its own core.
pub type Call = Arc<dyn Fn(Request) -> Answer + Send + Sync>;

/// The [`Call`] of the suite behind the window: the core over its socket (`Response::Error` becomes a
/// [`Refusal`] with its code), or with `--demo` the stand-in [`demo_call`].
pub fn call_for(suite: &Source) -> Call {
    match suite {
        Source::Core(client) => {
            let socket = client.socket.clone();
            Arc::new(move |request: Request| -> Answer {
                let socket = socket.clone();
                Box::pin(async move {
                    match torro_ipc::client::call(&socket, &request).await {
                        Ok(Response::Error { code, message }) => Err(Refusal { code, message }),
                        Ok(response) => Ok(response),
                        Err(err) => Err(Refusal { code: "offline".to_owned(), message: err.to_string() }),
                    }
                })
            })
        }
        Source::Demo(_) => demo_call(),
    }
}

/// What a hint says instead of the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// No engine at all: no AI account is set up (or none has a model turned on).
    NoAi,
    /// `forbidden`: the assistant may not read the mail's account.
    NoRight,
    /// `unsupported`: the chosen engine cannot chat about a mail (a coding agent, a bot).
    Unsupported,
    /// Anything else the core said, in its words.
    Failed(String),
}

impl Hint {
    /// The sentence of the hint.
    pub fn text(&self) -> String {
        match self {
            Hint::NoAi => tr("Set up an AI in Settings › AI first.").to_owned(),
            Hint::NoRight => {
                tr("The assistant may not read this mail. Allow it in the assistant's settings under Access.")
                    .to_owned()
            }
            Hint::Unsupported => {
                tr("This AI can't chat about a mail. Choose an AI account in Settings › AI.").to_owned()
            }
            Hint::Failed(message) => message.clone(),
        }
    }

    /// The button under the hint and what it does; `None` where nothing in reach helps.
    pub fn action(&self) -> Option<(&'static str, Event)> {
        match self {
            Hint::NoAi | Hint::Unsupported => Some((tr("Open settings"), Event::OpenSettings)),
            Hint::NoRight => Some((tr("Open assistant"), Event::OpenAssistant)),
            Hint::Failed(_) => None,
        }
    }
}

/// The hint for a refusal of the core (issue #605): `forbidden` and `unsupported` have their own; any
/// other code shows the core's words.
pub fn hint_for(refusal: &Refusal) -> Hint {
    match refusal.code.as_str() {
        "forbidden" => Hint::NoRight,
        "unsupported" => Hint::Unsupported,
        _ => Hint::Failed(refusal.message.clone()),
    }
}

/// How far the panel is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Asking what answers and which conversation to continue.
    Loading,
    /// The chat.
    Ready,
    /// A hint in place of the chat; the field is off until the panel opens again.
    Hint(Hint),
}

/// What opening found ([`open`]): what answers, and the conversation continued with its cards.
#[derive(Debug, Clone)]
pub struct Opened {
    /// The engine and model with their effort; `None` without any engine.
    pub choice: Option<(Choice, Option<Effort>)>,
    /// The newest conversation about the mail, whole; `None` when there is none.
    pub conversation: Option<(ConversationSummary, Vec<Entry>)>,
    /// Its cards as the core resolves them now.
    pub cards: Vec<CardEntry>,
}

/// What the panel reports. The app handles [`Event::Close`], [`Event::OpenAssistant`],
/// [`Event::OpenSettings`], [`Event::Link`] and [`Event::Accept`] itself; the rest goes to
/// [`Ask::update`].
#[derive(Debug, Clone)]
pub enum Event {
    /// The answer of [`open`], for the round it was asked in.
    Opened(Round, Result<Opened, Refusal>),
    /// The field changed.
    Input(String),
    /// "Send" or Enter in the field.
    Send,
    /// A question the empty chat offers, sent at once.
    Starter(String),
    /// The core's answer to "Send" (`assistant.send_message`), for the round it was sent in.
    Sent(Round, Result<(ConversationSummary, Vec<Entry>), Refusal>),
    /// Every [`FOLLOW`] while an answer runs.
    Poll,
    /// The core's answer to a poll (`assistant.get_conversation` with `after`), for its round.
    Polled(Round, Result<(ConversationSummary, Vec<Entry>), Refusal>),
    /// The cards of the conversation with this id (`assistant.get_cards`), for its round.
    Cards(Round, String, Result<Vec<CardEntry>, Refusal>),
    /// "+": a new conversation about the same mail.
    New,
    /// ✕ or "‹ Mail": the panel closes, the mail shows.
    Close,
    /// "Open assistant" under the hint that the assistant may not read the mail.
    OpenAssistant,
    /// "Open settings" under the hint that no AI is set up: the control centre's AI page.
    OpenSettings,
    /// A link in an answer: opened like a link of the mail (`App::open_link`).
    Link(String),
    /// "Accept" on a suggestion's card: account, message and kind of action.
    Accept { account: String, id: String, kind: String },
}

/// The mini chat about one mail.
pub struct Ask {
    /// The mail it is about.
    pub about: MailRef,
    pub phase: Phase,
    pub transcript: Transcript,
    /// The cards of the conversation by the number of their entry, as the core resolved them last.
    pub cards: HashMap<u64, Card>,
    /// What is typed in the field.
    pub input: String,
    /// What answers a new message, with its effort; `None` until [`open`] answered or without any.
    pub choice: Option<(Choice, Option<Effort>)>,
    /// A message is on its way: "Send" takes no second click.
    pub sending: bool,
    /// The last refusal of "Send" that is no hint, said over the field.
    pub failed: Option<String>,
    /// A poll is on its way: the next tick waits for it.
    following: bool,
    /// The round the answers of the chat open now belong to ([`Round`]).
    round: Round,
    /// The answers as Markdown, parsed once per text, by entry number.
    parsed: HashMap<u64, (String, Vec<markdown::Item>)>,
    call: Call,
}

impl Ask {
    /// The panel for the mail `about`, loading, and the task that opens it ([`open`]) and puts the
    /// cursor into the field.
    pub fn open(call: Call, about: MailRef) -> (Ask, Task<Event>) {
        let ask = Ask {
            about: about.clone(),
            phase: Phase::Loading,
            transcript: Transcript::default(),
            cards: HashMap::new(),
            input: String::new(),
            choice: None,
            sending: false,
            failed: None,
            following: false,
            round: Round::next(),
            parsed: HashMap::new(),
            call: call.clone(),
        };
        let round = ask.round;
        let opening = Task::perform(open(call, about), move |result| Event::Opened(round, result));
        (ask, Task::batch([opening, iced::widget::operation::focus(FIELD)]))
    }

    /// Whether the panel asks the core every [`FOLLOW`]: while a message is on its way or an answer
    /// runs.
    pub fn follows(&self) -> bool {
        self.sending || self.transcript.running()
    }

    /// The panel's part of an [`Event`]; the events the app handles change nothing here. An answer of
    /// another round ([`Round`]) is dropped.
    pub fn update(&mut self, event: Event) -> Task<Event> {
        let asked = match &event {
            Event::Opened(round, _) | Event::Sent(round, _) | Event::Polled(round, _) | Event::Cards(round, ..) => {
                Some(*round)
            }
            _ => None,
        };
        if asked.is_some_and(|round| round != self.round) {
            return Task::none();
        }
        match event {
            Event::Opened(_, Ok(opened)) => return self.opened(opened),
            Event::Opened(_, Err(refusal)) => self.phase = Phase::Hint(hint_for(&refusal)),
            Event::Input(text) => self.input = text,
            Event::Send => return self.send(),
            Event::Starter(text) => {
                self.input = text;
                return self.send();
            }
            Event::Sent(_, result) => {
                self.sending = false;
                match result {
                    Ok((summary, entries)) => {
                        self.input.clear();
                        self.failed = None;
                        return self.take(summary, entries);
                    }
                    Err(refusal) => match hint_for(&refusal) {
                        Hint::Failed(message) => self.failed = Some(message),
                        hint => self.phase = Phase::Hint(hint),
                    },
                }
            }
            Event::Poll => return self.poll(),
            Event::Polled(_, result) => {
                self.following = false;
                // A poll that failed is asked again on the next tick.
                if let Ok((summary, entries)) = result {
                    return self.take(summary, entries);
                }
            }
            Event::Cards(_, conversation, Ok(cards)) => {
                if self.transcript.summary.as_ref().is_some_and(|summary| summary.id == conversation) {
                    self.cards = cards.into_iter().map(|card| (card.seq, card.card)).collect();
                }
            }
            // The cards stay as they were; the next answer or tick asks again.
            Event::Cards(_, _, Err(_)) => {}
            // While the panel opens there is nothing to start anew; a new round would drop the answer
            // of the opening and leave it loading for good (review of #605).
            Event::New if self.phase == Phase::Loading => {}
            Event::New => {
                // Answers still on their way belong to the conversation before.
                self.round = Round::next();
                self.sending = false;
                self.following = false;
                self.transcript = Transcript::default();
                self.cards.clear();
                self.parsed.clear();
                self.failed = None;
                return iced::widget::operation::focus(FIELD);
            }
            Event::Close | Event::OpenAssistant | Event::OpenSettings | Event::Link(_) | Event::Accept { .. } => {}
        }
        Task::none()
    }

    /// The cards of the conversation again (`assistant.get_cards`), for the app's tick: a suggestion
    /// may have been accepted or dismissed meanwhile. Nothing without a card.
    pub fn refresh_cards(&self) -> Task<Event> {
        match &self.transcript.summary {
            Some(summary) if !self.transcript.cards().is_empty() => self.fetch_cards(summary.id.clone()),
            _ => Task::none(),
        }
    }

    fn opened(&mut self, opened: Opened) -> Task<Event> {
        let Opened { choice, conversation, cards } = opened;
        let Some(choice) = choice else {
            self.phase = Phase::Hint(Hint::NoAi);
            return Task::none();
        };
        self.choice = Some(choice);
        self.phase = Phase::Ready;
        if let Some((summary, entries)) = conversation {
            self.transcript = Transcript::new(summary, entries);
            self.cards = cards.into_iter().map(|card| (card.seq, card.card)).collect();
            self.parse();
        }
        Task::none()
    }

    /// "Send": `assistant.send_message` with what is typed – to the conversation open, or a new one
    /// about the mail. Nothing while one is on its way or an answer runs.
    fn send(&mut self) -> Task<Event> {
        if self.sending || self.transcript.running() || self.phase != Phase::Ready {
            return Task::none();
        }
        let conversation = self.transcript.summary.as_ref().map(|summary| summary.id.as_str());
        let choice = self.choice.as_ref();
        let Some(request) = send_request(conversation, choice, &self.input, &self.about) else {
            return Task::none();
        };
        self.sending = true;
        self.failed = None;
        let round = self.round;
        Task::perform(ask_conversation(self.call.clone(), request), move |result| Event::Sent(round, result))
    }

    /// Every [`FOLLOW`] while an answer runs: the entries after the newest one held.
    fn poll(&mut self) -> Task<Event> {
        let Some(id) = self.transcript.summary.as_ref().map(|summary| summary.id.clone()) else {
            return Task::none();
        };
        if self.following || !self.transcript.running() {
            return Task::none();
        }
        self.following = true;
        let request = Request::GetConversation { id, after: Some(self.transcript.last()) };
        let round = self.round;
        Task::perform(ask_conversation(self.call.clone(), request), move |result| Event::Polled(round, result))
    }

    /// An answer of the core merged into the transcript; its cards asked for where one is not
    /// resolved yet, or once the answer is over.
    fn take(&mut self, summary: ConversationSummary, entries: Vec<Entry>) -> Task<Event> {
        let was_running = self.transcript.running();
        if !self.transcript.merge(summary, entries) {
            return Task::none();
        }
        self.parse();
        let unresolved = self.transcript.cards().iter().any(|seq| !self.cards.contains_key(seq));
        let ended = was_running && !self.transcript.running();
        match &self.transcript.summary {
            Some(summary) if unresolved || ended => self.fetch_cards(summary.id.clone()),
            _ => Task::none(),
        }
    }

    fn fetch_cards(&self, conversation: String) -> Task<Event> {
        let (call, round) = (self.call.clone(), self.round);
        let asked = conversation.clone();
        Task::perform(
            async move {
                match call(Request::GetCards { conversation: asked }).await? {
                    Response::Cards { cards } => Ok(cards),
                    other => Err(Refusal::unexpected(&other)),
                }
            },
            move |result| Event::Cards(round, conversation.clone(), result),
        )
    }

    /// The answers parsed as Markdown, each once per text.
    fn parse(&mut self) {
        let mut seen = Vec::new();
        for entry in &self.transcript.entries {
            if let EntryContent::Answer { text, .. } = &entry.content {
                seen.push(entry.seq);
                if self.parsed.get(&entry.seq).is_none_or(|(held, _)| held != text) {
                    self.parsed.insert(entry.seq, (text.clone(), markdown::parse(text)));
                }
            }
        }
        self.parsed.retain(|seq, _| seen.contains(seq));
    }
}

/// The request of "Send" (issue #605): `assistant.send_message` with the text as typed (trailing
/// blank lines dropped) and what answers; a new conversation names the mail as `about`, a continued
/// one keeps its own. `None` for a blank text or without an engine.
pub fn send_request(
    conversation: Option<&str>,
    choice: Option<&(Choice, Option<Effort>)>,
    text: &str,
    about: &MailRef,
) -> Option<Request> {
    let text = text.trim_end();
    let (choice, effort) = choice?;
    if text.trim().is_empty() {
        return None;
    }
    Some(Request::SendMessage {
        conversation: conversation.map(str::to_owned),
        engine: choice.engine.clone(),
        model: choice.model.clone(),
        effort: *effort,
        text: text.to_owned(),
        without: Vec::new(),
        mode: Mode::Default,
        bot: None,
        about: conversation.is_none().then(|| about.clone()),
    })
}

/// The newest conversation about the mail `about` (issue #605: "Ask AI…" continues it), by its last
/// change; `None` when there is none.
pub fn newest_about<'a>(conversations: &'a [ConversationSummary], about: &MailRef) -> Option<&'a ConversationSummary> {
    conversations
        .iter()
        .filter(|conversation| conversation.about.as_ref() == Some(about))
        .max_by_key(|conversation| conversation.updated)
}

/// What answers in the chat about a mail and which conversation it continues (issue #605): the
/// engines (`assistant.list_engines`), the default of the control centre (`settings.get_settings`,
/// left out when the core does not tell) and the conversations (`assistant.list_conversations`); the
/// newest conversation about the mail is loaded whole with its cards and answers with its own engine
/// while that is still offered, a new one with `chat::preferred_choice`. What answers is checked last
/// ([`check_about`]): a refusal of the chat is the refusal of opening, so its hint shows at once.
pub async fn open(call: Call, about: MailRef) -> Result<Opened, Refusal> {
    let engines = match call(Request::ListEngines).await? {
        Response::Engines { engines } => chat::ordered(engines),
        other => return Err(Refusal::unexpected(&other)),
    };
    let default = match call(Request::Settings).await {
        Ok(Response::Settings { settings }) => settings.ai_slots.chat,
        _ => None,
    };
    let conversations = match call(Request::ListConversations).await? {
        Response::Conversations { conversations } => conversations,
        other => return Err(Refusal::unexpected(&other)),
    };
    let preferred = chat::preferred_choice(default.as_ref(), &conversations, &engines, |_| None);
    let preferred = ai_choice(preferred, &engines);
    let Some(newest) = newest_about(&conversations, &about).cloned() else {
        check_about(&call, preferred.as_ref(), &about).await?;
        return Ok(Opened { choice: preferred, conversation: None, cards: Vec::new() });
    };
    let request = Request::GetConversation { id: newest.id.clone(), after: None };
    let (summary, entries) = ask_conversation(call.clone(), request).await?;
    let cards = match call(Request::GetCards { conversation: summary.id.clone() }).await {
        Ok(Response::Cards { cards }) => cards,
        _ => Vec::new(),
    };
    let own = engines
        .iter()
        .find(|engine| engine.id == summary.engine)
        .filter(|engine| chats_about_mail(engine))
        .map(|_| (Choice { engine: summary.engine.clone(), model: summary.model.clone() }, summary.effort));
    let choice = own.or(preferred);
    check_about(&call, choice.as_ref(), &about).await?;
    Ok(Opened { choice, conversation: Some((summary, entries)), cards })
}

/// Whether the core would take a chat about `about` with `choice` (issue #605): `assistant.check_about`,
/// which stores nothing and asks no model, so the panel shows its hint on opening. `Err` only for the refusals
/// of the chat itself – `forbidden`, `not-connected`, `unsupported` and `not-found` (a mail the core does not
/// see) –; anything else, above all an older core that does not know the command yet (`error`, "malformed
/// request"), lets the panel open as before, and "Send" tells. Nothing to check without a choice.
pub async fn check_about(
    call: &Call,
    choice: Option<&(Choice, Option<Effort>)>,
    about: &MailRef,
) -> Result<(), Refusal> {
    let Some((choice, _)) = choice else { return Ok(()) };
    let request =
        Request::CheckAbout { engine: choice.engine.clone(), model: choice.model.clone(), about: about.clone() };
    match call(request).await {
        Err(refusal) if ["forbidden", "not-connected", "unsupported", "not-found"].contains(&refusal.code.as_str()) => {
            Err(refusal)
        }
        _ => Ok(()),
    }
}

/// Whether `engine` may chat about a mail (review of #605): an AI account – no coding agent, no test
/// engine, which the core refuses with `unsupported`. Wherever its model computes (issue #664).
pub fn chats_about_mail(engine: &Engine) -> bool {
    engine.kind == EngineKind::Ai
}

/// What answers the chat about a mail (review of #605): the assistant's own choice (`preferred`)
/// while it is an AI account ([`chats_about_mail`]), else the first AI account of `engines` (in the
/// order of `chat::ordered`, this computer first), letting it pick its model. `None` without any AI
/// account (the hint "Set up an AI…").
pub fn ai_choice(preferred: Option<(Choice, Option<Effort>)>, engines: &[Engine]) -> Option<(Choice, Option<Effort>)> {
    let fits = |choice: &(Choice, Option<Effort>)| {
        engines.iter().find(|engine| engine.id == choice.0.engine).is_some_and(chats_about_mail)
    };
    if let Some(preferred) = preferred.filter(fits) {
        return Some(preferred);
    }
    let ordered = chat::ordered(engines.to_vec());
    ordered
        .iter()
        .find(|engine| chats_about_mail(engine))
        .map(|engine| (Choice { engine: engine.id.clone(), model: None }, None))
}

/// A request answered with a conversation (`assistant.send_message`, `assistant.get_conversation`).
async fn ask_conversation(call: Call, request: Request) -> Result<(ConversationSummary, Vec<Entry>), Refusal> {
    match call(request).await? {
        Response::Conversation { conversation, entries } => Ok((conversation, entries)),
        other => Err(Refusal::unexpected(&other)),
    }
}

// -- Drawing ------------------------------------------------------------------------------------------

/// What [`view`] needs of the window besides the panel.
pub struct Look<'a> {
    /// The mail's head as the window holds it: its subject and sender over the chat, and the
    /// fallback title of a suggestion's card.
    pub summary: Option<&'a MessageSummary>,
    /// The panel lies over the mail (medium, narrow, or a wide reader too narrow for both): "‹ Mail"
    /// in its head leads back.
    pub covering: bool,
    /// The kinds of action, for the cards' icons and words.
    pub kinds: Option<&'a [ActionKind]>,
    /// The suggestions dismissed but still waiting out their undo, as `(message, kind)`: their cards
    /// are already gone over the mail, so here too.
    pub dismissing: Vec<(&'a str, &'a str)>,
    pub now: DateTime<Utc>,
    /// The colours of the window's theme, for the answers' Markdown.
    pub palette: Palette,
}

/// The panel (mock-up `docs/prototypes/mail-mini-chat/`): a head of [`size::header()`] with "‹ Mail"
/// where it covers the mail, the sparkles, "Ask AI", "+" and ✕; under a line what it is about (the
/// mail's subject, its sender and time); then the chat – or, before the first message, what to ask
/// with three questions to start from, or a hint in place of it –, and the field with "Send" at the
/// bottom. The chat scrolls and keeps its end in view.
pub fn view<'a>(ask: &'a Ask, look: Look<'a>) -> Element<'a, Message> {
    let colours = look.palette;
    let body: Element<'a, Message> = match &ask.phase {
        Phase::Loading => widgets::loading(),
        Phase::Hint(hint) => hint_card(hint),
        Phase::Ready if ask.transcript.entries.is_empty() && !ask.sending => welcome(),
        Phase::Ready => {
            let log = entries(ask, &look, colours);
            scrollable(container(log).width(Fill).padding(layout_padding()))
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(style::scrollable)
                .anchor_bottom()
                .width(Fill)
                .height(Fill)
                .into()
        }
    };
    let content = column![
        head(look.covering, ask.phase != Phase::Loading),
        rule::horizontal(size::HAIRLINE).style(style::rule),
        about_row(look.summary, look.now),
        rule::horizontal(size::HAIRLINE).style(style::rule),
        container(body).width(Fill).height(Fill),
        composer(ask),
    ];
    container(content).width(Fill).height(Fill).style(style::container::panel_flush).into()
}

/// The room around the chat: the panel's inset.
fn layout_padding() -> Padding {
    Padding::new(space::l())
}

/// The sparkles in the accent: the AI's mark, as on the rail (`RailItem::accent`).
fn sparkles<'a>(edge: f32) -> Element<'a, Message> {
    icon::icon(Icon::Sparkles, edge).style(|t: &Theme, _| svg::Style { color: Some(palette(t).accent) }).into()
}

/// An icon in a colour of the palette.
fn tinted<'a>(which: Icon, edge: f32, colour: fn(&Palette) -> iced::Color) -> Element<'a, Message> {
    icon::icon(which, edge).style(move |t: &Theme, _| svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// The head: "‹ Mail" while the panel covers the mail, the sparkles and "Ask AI", then "+" ("New
/// chat", off while the panel opens) and ✕ ("Close"); padded like a panel's head, so ✕ lines up
/// with the window's buttons. The tip of the chevron of "‹ Mail" stands on the line of the content below – the mail's mark and the
/// chat's text: the ghost button's own padding and the room Lucide leaves inside the icon before the
/// tip ([`chevron_inset`]) are taken off the head's (design review of #605: it stood that far to the
/// right of the line).
fn head<'a>(covering: bool, can_start_anew: bool) -> Element<'a, Message> {
    let left = match covering {
        true => (space::l() - style::button_padding().left - chevron_inset()).max(0.0),
        false => space::l(),
    };
    let mut line = row![].spacing(space::s()).align_y(Alignment::Center);
    if covering {
        line = line.push(widgets::text_button(
            Some(Icon::ChevronLeft),
            tr("Mail"),
            style::button::ghost,
            Some(Message::Ask(Event::Close)),
        ));
    }
    line = line
        .push(sparkles(icon::SIZE))
        .push(typo::text(typo::body_strong(), tr("Ask AI")).wrapping(Wrapping::None))
        .push(Space::new().width(Fill))
        .push(widgets::icon_button(Icon::Plus, tr("New chat"), can_start_anew.then_some(Message::Ask(Event::New))))
        .push(widgets::icon_button(Icon::X, tr("Close"), Some(Message::Ask(Event::Close))));
    container(line).width(Fill).center_y(size::header()).padding(Padding::new(0.0).left(left).right(space::m())).into()
}

/// How far inside its icon the chevron of a text button ([`widgets::text_button`], icon
/// [`icon::SIZE_SM`]) has its tip: `chevron-left.svg` draws it at 9 of 24, less half its stroke of
/// 1.75. Taken off the head's inset with the button's padding (never below 0), it puts the tip where
/// the mail's mark below begins – that icon leaves a little room before its edge too – in all three
/// spacing levels.
fn chevron_inset() -> f32 {
    icon::SIZE_SM * (9.0 - 1.75 / 2.0) / 24.0
}

/// What the chat is about: the mail's mark, "About: {subject}" and its sender and time, each on one
/// line with "…".
fn about_row<'a>(summary: Option<&MessageSummary>, now: DateTime<Utc>) -> Element<'a, Message> {
    let (title, meta) = match summary {
        Some(summary) => (
            trf("About: {subject}", &[("subject", &mail::subject(summary))]),
            format!("{} · {}", mail::sender_name(summary.from.as_ref()), datetime::format_moment(summary.date, now)),
        ),
        None => (tr("About this mail").to_owned(), String::new()),
    };
    let mut words = column![widgets::ellipsized(typo::body_strong(), title, theme::text)];
    if !meta.is_empty() {
        words = words.push(widgets::ellipsized(typo::meta().tabular(), meta, theme::text_3));
    }
    let line = row![tinted(Icon::Mail, icon::SIZE, |p| p.text_3), container(words).width(Fill)]
        .spacing(space::m())
        .align_y(Alignment::Center);
    container(line).width(Fill).padding([space::m(), space::l()]).into()
}

/// Before the first message: what the chat is for, and three questions to start from, one under the
/// other, all as wide as the column and their words at the left (mock-up).
fn welcome<'a>() -> Element<'a, Message> {
    let starters = STARTERS.iter().map(|&question| {
        let words = tr(question);
        let label = typo::text(typo::caption(), words).wrapping(Wrapping::None);
        button(container(label).center_y(Fill))
            .width(Fill)
            .height(size::control_sm())
            .padding(style::button_padding())
            .style(style::button::secondary)
            .on_press(Message::Ask(Event::Starter(words.to_owned())))
            .into()
    });
    let content = column![
        tinted(Icon::Sparkles, icon::SIZE, |p| p.text_3),
        typo::text(typo::body(), tr("Ask about this mail. Torro reads it first; it only reads and suggests."))
            .style(theme::text_2)
            .align_x(Alignment::Center)
            .wrapping(Wrapping::WordOrGlyph),
        column(starters).spacing(space::s()).width(Fill),
    ]
    .spacing(space::m())
    .align_x(Alignment::Center)
    .max_width(size::props());
    container(content).center(Fill).padding(space::l()).into()
}

/// A hint in place of the chat: the info mark in the attention colour, the sentence and its button.
fn hint_card<'a>(hint: &Hint) -> Element<'a, Message> {
    let line = row![
        tinted(Icon::Info, icon::SIZE, |p| p.attention),
        typo::text(typo::body(), hint.text()).style(theme::text).width(Fill).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::m())
    .align_y(Alignment::Start);
    let mut parts: Vec<Element<'a, Message>> = vec![line.into()];
    if let Some((label, event)) = hint.action() {
        let button = widgets::text_button(None, label, style::button::secondary, Some(Message::Ask(event)));
        parts.push(container(button).width(Fill).align_right(Fill).into());
    }
    let card = widgets::card(parts, false, None);
    container(card).width(Fill).padding(layout_padding()).into()
}

/// The entries in order: the person's messages right in a bubble, tool lines close together, answers
/// as Markdown, the cards; "Working …" while an answer runs and nothing shows it yet.
fn entries<'a>(ask: &'a Ask, look: &Look<'a>, colours: Palette) -> Element<'a, Message> {
    let mut list = column![].width(Fill);
    let mut previous_tool: Option<bool> = None;
    for entry in &ask.transcript.entries {
        let Some(shown) = entry_view(ask, entry, look, colours) else { continue };
        let tool = matches!(entry.content, EntryContent::Tool { .. });
        if let Some(was_tool) = previous_tool {
            let gap = if was_tool && tool { space::xs() } else { space::m() };
            list = list.push(Space::new().height(gap));
        }
        previous_tool = Some(tool);
        list = list.push(shown);
    }
    let writing = ask.transcript.growing().is_some();
    if (ask.transcript.running() || ask.sending) && !writing {
        list = list.push(Space::new().height(space::m()));
        list = list.push(typo::text(typo::meta(), tr("Working …")).style(theme::text_3));
    }
    list.into()
}

/// One entry; `None` for what the panel does not show (an approval: the chat about a mail changes
/// nothing; a card not resolved yet or dismissed with its undo still running).
fn entry_view<'a>(ask: &'a Ask, entry: &'a Entry, look: &Look<'a>, colours: Palette) -> Option<Element<'a, Message>> {
    Some(match &entry.content {
        EntryContent::Message { text, .. } => {
            let bubble = container(typo::text(typo::body(), text.as_str()).wrapping(Wrapping::WordOrGlyph))
                .padding([space::s(), space::m()])
                .style(style::container::group);
            container(bubble).width(Fill).align_right(Fill).padding(Padding::new(0.0).left(space::xl())).into()
        }
        EntryContent::Answer { text, .. } => match ask.parsed.get(&entry.seq) {
            Some((_, items)) => markdown::view(items, colours).map(|uri| Message::Ask(Event::Link(uri.to_string()))),
            None => typo::text(typo::body(), text.as_str()).wrapping(Wrapping::WordOrGlyph).into(),
        },
        EntryContent::Tool { name, summary, error, .. } => tool_line(name, summary, error.as_deref()),
        EntryContent::Error { message, .. } => row![
            tinted(Icon::CircleAlert, icon::SIZE, |p| p.error_text),
            typo::text(typo::body(), message.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into(),
        EntryContent::Stopped => row![
            tinted(Icon::Square, icon::SIZE_SM, |p| p.text_3),
            typo::text(typo::meta(), tr("Stopped")).style(theme::text_3)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into(),
        EntryContent::Card { .. } => return card_view(ask.cards.get(&entry.seq)?, &ask.about, look),
        EntryContent::Approval { .. } => return None,
    })
}

/// A tool line: a check in `ok` (or the alert in `error_text` for a refused one) and "Read mail ·
/// …" in `meta`, as the assistant's window words it (`chat::tool_line`).
fn tool_line<'a>(name: &str, summary: &str, error: Option<&str>) -> Element<'a, Message> {
    let mark = match error {
        Some(_) => tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text),
        None => tinted(Icon::Check, icon::SIZE_SM, |p| p.ok),
    };
    row![mark, typo::text(typo::meta(), chat::tool_line(name, summary, error)).style(theme::text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// Whether a suggestion's card belongs to the mail the chat is about (review of #605): only those
/// show. Its "Accept" and "Dismiss" act on the open mail, so a card of another mail – which a model
/// could read out with a wider right – must never stand here as if it were this mail's.
pub fn of_this_mail(card: &Card, about: &MailRef) -> bool {
    matches!(card, Card::Suggestion { account, id, .. } if *account == about.account && *id == about.id)
}

/// A card as the core resolved it: a suggestion of the chat's own mail ([`of_this_mail`]) as the card
/// over the mail shows it (`mail::suggestion_card`), its "Accept" the panel's own (the suggestion is
/// fetched again first); a suggestion of another mail, or one dismissed with its undo still running,
/// is left out; anything gone a quiet line.
fn card_view<'a>(card: &'a Card, about: &MailRef, look: &Look<'a>) -> Option<Element<'a, Message>> {
    match card {
        Card::Suggestion { suggestion, .. } => {
            let summary = look.summary.filter(|summary| summary.account == about.account && summary.id == about.id)?;
            if !of_this_mail(card, about) {
                return None;
            }
            let dismissed =
                look.dismissing.iter().any(|(id, kind)| *id == about.id && *kind == suggestion.kind.as_str());
            if dismissed {
                return None;
            }
            let card = mail::suggestion_card(suggestion, summary, look.kinds, look.now, false);
            Some(card.map(|message| match message {
                Message::AcceptSuggestion(account, id, kind) => Message::Ask(Event::Accept { account, id, kind }),
                other => other,
            }))
        }
        Card::File(file) => Some(quiet_line(Icon::Paperclip, file.name.clone())),
        Card::Unavailable => Some(quiet_line(Icon::Sparkles, tr("No longer available").to_owned())),
    }
}

/// A card that offers nothing to do here: its mark and words in `text_3`, on one line.
fn quiet_line<'a>(mark: Icon, words: String) -> Element<'a, Message> {
    row![tinted(mark, icon::SIZE_SM, |p| p.text_3), widgets::ellipsized(typo::meta(), words, theme::text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// The field at the bottom, "Ask about this mail…", and "Send" (the paper plane, as in the assistant's
/// window) beside it – framed, on while there is
/// something to send, framed and off otherwise; off with the field under a hint. The core's words of
/// a message that did not go stand over it.
fn composer<'a>(ask: &'a Ask) -> Element<'a, Message> {
    let open = ask.phase == Phase::Ready;
    let body = typo::body();
    let mut field = text_input(tr("Ask about this mail…"), &ask.input)
        .id(FIELD)
        .size(body.size)
        .line_height(body.line_height)
        .font(body.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input);
    if open && !ask.sending {
        field = field.on_input(|text| Message::Ask(Event::Input(text))).on_submit(Message::Ask(Event::Send));
    }
    // Framed and neutral, never red: the window's one primary button is "New email" (#476).
    let ready = open && !ask.follows() && !ask.input.trim().is_empty();
    let send = widgets::boxed_button(Icon::Send, tr("Send"), ready.then_some(Message::Ask(Event::Send)));
    let mut parts = column![].spacing(space::s());
    if let Some(failed) = &ask.failed {
        parts = parts.push(
            row![
                tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text),
                typo::text(typo::meta(), failed.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
        );
    }
    parts = parts.push(row![field, send].spacing(space::s()).align_y(Alignment::Center));
    // The same left edge as the chat above it.
    container(parts)
        .width(Fill)
        .padding(Padding { top: 0.0, right: space::l(), bottom: space::m(), left: space::l() })
        .into()
}

// -- Demo ---------------------------------------------------------------------------------------------

/// The scenes of the demo (`--open ask`, `ask-card`, `ask-hint`, `ask-empty`, `ask-working`,
/// `ask-failed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// Nothing asked yet: what to ask, and the three questions to start from.
    Empty,
    /// A question sent, the mail read, the answer still being worked on ("Working …").
    Working,
    /// An answer, then a message that did not go: the core's words over the field, the text kept.
    Failed,
    /// A question, the mail read and an answer.
    Answer,
    /// Asked for an event: the mail read, the event read out and its card.
    Card,
    /// The assistant may not read the mail: the hint with "Open assistant".
    Hint,
}

/// The engine of the demo: a local model, so nothing reads as leaving the computer.
fn demo_engine() -> Engine {
    Engine {
        id: "ai:ollama@localhost:11434".to_owned(),
        kind: EngineKind::Ai,
        name: "Ollama".to_owned(),
        problem: None,
        models: Vec::new(),
    }
}

/// A conversation head of the demo about `about`, `last` entries long.
fn demo_summary(about: &MailRef, title: &str, last: u64, now: DateTime<Utc>) -> ConversationSummary {
    ConversationSummary {
        id: "c-demo-mail".to_owned(),
        title: title.to_owned(),
        engine: demo_engine().id,
        model: Some("qwen3:8b".to_owned()),
        effort: None,
        created: now,
        updated: now,
        pinned: false,
        running: false,
        last,
        bot: None,
        bot_deleted: false,
        about: Some(about.clone()),
    }
}

fn demo_entry(seq: u64, content: EntryContent, now: DateTime<Utc>) -> Entry {
    Entry { seq, time: now, content }
}

fn demo_message(text: &str) -> EntryContent {
    EntryContent::Message {
        text: text.to_owned(),
        engine: demo_engine().id,
        model: Some("qwen3:8b".to_owned()),
        effort: None,
        without: Vec::new(),
        mode: Mode::Default,
        from: None,
    }
}

fn demo_tool(name: &str, summary: &str) -> EntryContent {
    EntryContent::Tool {
        name: name.to_owned(),
        input: serde_json::json!({}),
        summary: summary.to_owned(),
        error: None,
        lack: None,
    }
}

fn demo_answer(text: &str) -> EntryContent {
    EntryContent::Answer { text: text.to_owned(), engine: demo_engine().id, model: Some("qwen3:8b".to_owned()) }
}

/// The demo's event read out of the mail `summary`: its subject on the Thursday after it, 14:00 to
/// 15:00 in "Office". Made-up model text like the demo's mail, so it stays untranslated.
fn demo_event(summary: &MessageSummary) -> Suggestion {
    let day = datetime::to_local(summary.date).date();
    let ahead = (Weekday::Thu.num_days_from_monday() + 7 - day.weekday().num_days_from_monday()) % 7;
    let thursday = day + Days::new(if ahead == 0 { 7 } else { u64::from(ahead) });
    let at = |hour: u32| {
        NaiveTime::from_hms_opt(hour, 0, 0).map(|time| EventTime::DateTime(datetime::to_utc(thursday.and_time(time))))
    };
    Suggestion {
        kind: "event".to_owned(),
        state: SuggestionState::Ready,
        task: None,
        event: Some(EventSuggestion {
            title: mail::subject(summary),
            start: at(14),
            end: at(15),
            location: Some("Office".to_owned()),
            attendees: Vec::new(),
            engine: demo_engine().id,
            model: "qwen3:8b".to_owned(),
        }),
        reason: Some("The mail proposes the meeting".to_owned()),
    }
}

impl Ask {
    /// The panel of a demo scene about the mail `summary`, at once and without a core (the shots
    /// draw it as it stands). Its words are the demo's, like its mail: untranslated.
    pub fn staged(suite: &Source, summary: &MessageSummary, scene: Scene, now: DateTime<Utc>) -> Ask {
        let about = MailRef { account: summary.account.clone(), id: summary.id.clone() };
        let (mut ask, _) = Ask::open(call_for(suite), about.clone());
        ask.choice = Some((Choice { engine: demo_engine().id, model: Some("qwen3:8b".to_owned()) }, None));
        ask.phase = Phase::Ready;
        let entries = match scene {
            Scene::Hint => {
                ask.phase = Phase::Hint(Hint::NoRight);
                return ask;
            }
            Scene::Empty => return ask,
            Scene::Working => vec![demo_message("Is there a deadline?"), demo_tool("mail.get_message", "")],
            Scene::Failed => {
                ask.input = "And what about the figures?".to_owned();
                ask.failed = Some("The AI's server did not answer. Try again in a moment.".to_owned());
                vec![
                    demo_message("Is there a deadline?"),
                    demo_tool("mail.get_message", ""),
                    demo_answer("Yes: Anna needs your feedback on the draft by Friday."),
                ]
            }
            Scene::Answer => vec![
                demo_message("What does the sender want from me?"),
                demo_tool("mail.get_message", ""),
                demo_answer(
                    "Two things: bring the figures of the last quarter to the meeting, and give feedback on the draft by Friday.",
                ),
                demo_message("Was that discussed before?"),
                demo_tool("mail.search_messages", "3 results"),
                demo_answer(
                    "Yes – the draft came up in the review two weeks ago; the figures were asked for then too.",
                ),
            ],
            Scene::Card => vec![
                demo_message("There is a meeting on Thursday, make an event of it please."),
                demo_tool("mail.get_message", ""),
                demo_tool("mail.extract_action", ""),
                demo_answer("Here it is. Check it and accept it to open the event editor."),
                EntryContent::Card {
                    card: CardRef::Suggestion {
                        account: about.account.clone(),
                        id: about.id.clone(),
                        action: "event".to_owned(),
                    },
                },
            ],
        };
        let last = entries.len() as u64;
        let entries: Vec<Entry> =
            entries.into_iter().enumerate().map(|(at, content)| demo_entry(at as u64 + 1, content, now)).collect();
        if scene == Scene::Card {
            let card = Card::Suggestion {
                account: about.account.clone(),
                id: about.id.clone(),
                suggestion: Box::new(demo_event(summary)),
            };
            ask.cards.insert(last, card);
        }
        let mut head = demo_summary(&about, &mail::subject(summary), last, now);
        // Staged still: the demo's stand-in is never asked for the answer that runs.
        head.running = scene == Scene::Working;
        ask.transcript = Transcript::new(head, entries);
        ask.parse();
        ask
    }
}

/// The stand-in core of `--demo`: one local engine, no conversation about the mail yet, and a short
/// made-up answer to every message (the mail read first, as the core does), kept for the panel's life
/// so a second message follows the first. Nothing leaves the window.
pub fn demo_call() -> Call {
    let held: Arc<Mutex<Vec<Entry>>> = Arc::new(Mutex::new(Vec::new()));
    Arc::new(move |request: Request| -> Answer {
        let held = held.clone();
        Box::pin(async move { demo_answer_to(&held, request) })
    })
}

/// What the demo answers to `request`, with the conversation `held` so far.
fn demo_answer_to(held: &Mutex<Vec<Entry>>, request: Request) -> Result<Response, Refusal> {
    let now = Utc::now();
    let refused = || Refusal { code: "not-found".to_owned(), message: "not in the demo".to_owned() };
    match request {
        Request::ListEngines => Ok(Response::Engines { engines: vec![demo_engine()] }),
        Request::ListConversations => Ok(Response::Conversations { conversations: Vec::new() }),
        Request::GetCards { .. } => Ok(Response::Cards { cards: Vec::new() }),
        Request::CheckAbout { .. } => Ok(Response::Ok),
        Request::SendMessage { conversation, text, about, .. } => {
            let mut entries = held.lock().map_err(|_| refused())?;
            // A message without a conversation starts a new one ("+"): the one before is left.
            if conversation.is_none() {
                entries.clear();
            }
            let first = entries.is_empty();
            let mut add = |content| {
                let seq = entries.len() as u64 + 1;
                entries.push(demo_entry(seq, content, now));
            };
            add(demo_message(&text));
            if first {
                add(demo_tool("mail.get_message", ""));
            }
            add(demo_answer("This is the demo: with an AI set up, the answer about this mail would stand here."));
            let about = about.unwrap_or_else(|| MailRef { account: String::new(), id: String::new() });
            let summary = demo_summary(&about, "Demo", entries.len() as u64, now);
            Ok(Response::Conversation { conversation: summary, entries: entries.clone() })
        }
        _ => Err(refused()),
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 11, 10, 9, minute, 0).single().expect("a time")
    }

    fn mail_ref(id: &str) -> MailRef {
        MailRef { account: "mail:anna@example.org".to_owned(), id: id.to_owned() }
    }

    fn conversation(id: &str, about: Option<MailRef>, updated: u32) -> ConversationSummary {
        ConversationSummary {
            id: id.to_owned(),
            title: id.to_owned(),
            engine: demo_engine().id,
            model: Some("qwen3:8b".to_owned()),
            effort: None,
            created: at(0),
            updated: at(updated),
            pinned: false,
            running: false,
            last: 1,
            bot: None,
            bot_deleted: false,
            about,
        }
    }

    /// Design review of #605: over the mail the tip of "‹ Mail" stands on the line of the content below
    /// (`space::l()` and the room the mail's mark has inside its icon), not a button's padding to the
    /// right of it.
    #[test]
    fn the_chevron_of_the_way_back_stands_on_the_content_line() {
        let left = (space::l() - style::button_padding().left - chevron_inset()).max(0.0);
        let tip = left + style::button_padding().left + chevron_inset();
        let mark = space::l() + icon::SIZE * (2.0 - 1.75 / 2.0) / 24.0;
        assert!((tip - mark).abs() < 1.0, "tip {tip}, mark {mark}");
    }

    #[test]
    fn a_hint_follows_the_code_of_the_refusal() {
        let refusal = |code: &str| Refusal { code: code.to_owned(), message: "The core's words".to_owned() };
        assert_eq!(hint_for(&refusal("forbidden")), Hint::NoRight);
        assert_eq!(hint_for(&refusal("not-connected")), Hint::Failed("The core's words".to_owned()));
        assert_eq!(hint_for(&refusal("unsupported")), Hint::Unsupported);
        assert_eq!(hint_for(&refusal("offline")), Hint::Failed("The core's words".to_owned()));
        assert_eq!(
            Hint::NoRight.text(),
            tr("The assistant may not read this mail. Allow it in the assistant's settings under Access.")
        );
        assert!(matches!(Hint::NoRight.action(), Some((label, Event::OpenAssistant)) if label == tr("Open assistant")));
        assert!(matches!(Hint::NoAi.action(), Some((_, Event::OpenSettings))));
        assert_eq!(Hint::NoAi.text(), tr("Set up an AI in Settings › AI first."));
    }

    #[test]
    fn the_newest_conversation_about_the_same_mail_is_continued() {
        let list = vec![
            conversation("other-mail", Some(mail_ref("m:2")), 9),
            conversation("older", Some(mail_ref("m:1")), 3),
            conversation("plain", None, 8),
            conversation("newer", Some(mail_ref("m:1")), 5),
        ];
        assert_eq!(newest_about(&list, &mail_ref("m:1")).map(|c| c.id.as_str()), Some("newer"));
        assert_eq!(newest_about(&list, &mail_ref("m:3")), None);
    }

    /// A core that knows two conversations about the mail: opening loads the newer one whole, with
    /// its cards, and answers with its engine.
    #[test]
    fn opening_loads_the_newest_conversation_about_the_mail() {
        let asked: Arc<Mutex<Vec<Request>>> = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let call: Call = Arc::new(move |request: Request| -> Answer {
            if let Ok(mut log) = log.lock() {
                log.push(request.clone());
            }
            let answer = match request {
                Request::ListEngines => Ok(Response::Engines { engines: vec![demo_engine()] }),
                Request::ListConversations => Ok(Response::Conversations {
                    conversations: vec![
                        conversation("older", Some(mail_ref("m:1")), 3),
                        conversation("newer", Some(mail_ref("m:1")), 5),
                    ],
                }),
                Request::GetConversation { id, .. } => Ok(Response::Conversation {
                    conversation: conversation(&id, Some(mail_ref("m:1")), 5),
                    entries: vec![demo_entry(1, demo_message("Hi"), at(5))],
                }),
                Request::GetCards { .. } => Ok(Response::Cards { cards: Vec::new() }),
                _ => Err(Refusal::error("old core")),
            };
            Box::pin(async move { answer })
        });
        let opened = block_on(open(call, mail_ref("m:1"))).expect("opened");
        let (summary, entries) = opened.conversation.expect("a conversation");
        assert_eq!((summary.id.as_str(), entries.len()), ("newer", 1));
        assert_eq!(opened.choice.map(|(choice, _)| choice.engine), Some(demo_engine().id));
        let asked = asked.lock().expect("log").clone();
        assert!(asked.contains(&Request::GetConversation { id: "newer".to_owned(), after: None }), "{asked:?}");
        assert!(asked.contains(&Request::GetCards { conversation: "newer".to_owned() }));
        // This core does not know `assistant.check_about` yet: the panel opens all the same.
        assert!(asked.iter().any(|request| matches!(request, Request::CheckAbout { .. })), "{asked:?}");
    }

    /// A core with one local engine and no conversation that answers `assistant.check_about` with `check`; the
    /// requests it was asked are logged.
    fn checking_core(check: Result<Response, Refusal>) -> (Call, Arc<Mutex<Vec<Request>>>) {
        let asked: Arc<Mutex<Vec<Request>>> = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let call: Call = Arc::new(move |request: Request| -> Answer {
            if let Ok(mut log) = log.lock() {
                log.push(request.clone());
            }
            let answer = match request {
                Request::ListEngines => Ok(Response::Engines { engines: vec![demo_engine()] }),
                Request::ListConversations => Ok(Response::Conversations { conversations: Vec::new() }),
                Request::CheckAbout { .. } => check.clone(),
                _ => Err(Refusal::error("not asked")),
            };
            Box::pin(async move { answer })
        });
        (call, asked)
    }

    /// Issue #605: opening asks `assistant.check_about` with what would answer; a refusal shows its hint at once,
    /// before anything is typed, and a chat the core takes is ready.
    #[test]
    fn the_hint_stands_as_soon_as_the_panel_opens() {
        let forbidden = Refusal { code: "forbidden".to_owned(), message: "no".to_owned() };
        let (call, asked) = checking_core(Err(forbidden));
        let (mut ask, _) = Ask::open(call.clone(), mail_ref("m:1"));
        let opened = block_on(open(call, mail_ref("m:1")));
        let round = ask.round;
        let _ = ask.update(Event::Opened(round, opened));
        assert_eq!(ask.phase, Phase::Hint(Hint::NoRight));
        let check = Request::CheckAbout { engine: demo_engine().id, model: None, about: mail_ref("m:1") };
        assert!(asked.lock().expect("log").contains(&check), "with the engine that would answer");

        let (call, _) = checking_core(Ok(Response::Ok));
        let (mut ask, _) = Ask::open(call.clone(), mail_ref("m:1"));
        let opened = block_on(open(call, mail_ref("m:1")));
        let round = ask.round;
        let _ = ask.update(Event::Opened(round, opened));
        assert_eq!(ask.phase, Phase::Ready);

        let unsupported = Refusal { code: "unsupported".to_owned(), message: "no chat".to_owned() };
        let (call, _) = checking_core(Err(unsupported));
        let refused = block_on(open(call, mail_ref("m:1"))).expect_err("refused");
        assert_eq!(hint_for(&refused), Hint::Unsupported);
        // An older core without the command: nothing in the way.
        let old = Refusal::error("malformed request: unknown method or arguments that do not fit it");
        let (call, _) = checking_core(Err(old));
        assert!(block_on(open(call, mail_ref("m:1"))).is_ok());
    }

    #[test]
    fn a_new_conversation_names_the_mail_and_a_continued_one_does_not() {
        let choice = (Choice { engine: demo_engine().id, model: None }, Some(Effort::Low));
        let about = mail_ref("m:1");
        let new = send_request(None, Some(&choice), "Is there a deadline?\n", &about);
        assert!(matches!(
            new,
            Some(Request::SendMessage { conversation: None, about: Some(ref named), ref text, effort: Some(Effort::Low), .. })
                if *named == about && text == "Is there a deadline?"
        ));
        let continued = send_request(Some("c1"), Some(&choice), "And then?", &about);
        assert!(matches!(continued, Some(Request::SendMessage { about: None, .. })));
        assert_eq!(send_request(None, Some(&choice), "  ", &about), None, "blank");
        assert_eq!(send_request(None, None, "Hi", &about), None, "no engine");
    }

    /// Without an engine the panel shows the hint, not a chat; a refused send its hint.
    #[test]
    fn no_engine_and_a_refused_send_show_their_hints() {
        let (mut ask, _) = Ask::open(demo_call(), mail_ref("m:1"));
        let round = ask.round;
        let _ = ask.update(Event::Opened(round, Ok(Opened { choice: None, conversation: None, cards: Vec::new() })));
        assert_eq!(ask.phase, Phase::Hint(Hint::NoAi));

        let (mut ask, _) = Ask::open(demo_call(), mail_ref("m:1"));
        let choice = (Choice { engine: demo_engine().id, model: None }, None);
        let round = ask.round;
        let opened = Opened { choice: Some(choice), conversation: None, cards: Vec::new() };
        let _ = ask.update(Event::Opened(round, Ok(opened)));
        assert_eq!(ask.phase, Phase::Ready);
        let _ = ask.update(Event::Input("Is there a deadline?".to_owned()));
        let _ = ask.update(Event::Send);
        assert!(ask.sending && ask.follows());
        let refused = Refusal { code: "forbidden".to_owned(), message: "no".to_owned() };
        let _ = ask.update(Event::Sent(round, Err(refused)));
        assert_eq!(ask.phase, Phase::Hint(Hint::NoRight));
        assert_eq!(ask.input, "Is there a deadline?", "the words stay");
    }

    /// Review of #605: an answer comes back to the round it was asked in – a late answer after "+" or
    /// to another mail's panel is dropped, never merged into the chat open now.
    #[test]
    fn a_late_answer_of_another_round_is_dropped() {
        let ready = |ask: &mut Ask| {
            let choice = (Choice { engine: demo_engine().id, model: None }, None);
            let opened = Opened { choice: Some(choice), conversation: None, cards: Vec::new() };
            let round = ask.round;
            let _ = ask.update(Event::Opened(round, Ok(opened)));
        };
        let answer = |id: &str| {
            let summary = ConversationSummary { last: 1, ..conversation(id, Some(mail_ref("m:1")), 1) };
            Ok((summary, vec![demo_entry(1, demo_message("Hi"), at(1))]))
        };
        // Sent in the panel of mail A, then the panel of mail B opened: A's answer stays out of B.
        let (mut a, _) = Ask::open(demo_call(), mail_ref("m:1"));
        ready(&mut a);
        let _ = a.update(Event::Input("Hi".to_owned()));
        let _ = a.update(Event::Send);
        let sent_in_a = a.round;
        let (mut b, _) = Ask::open(demo_call(), mail_ref("m:2"));
        ready(&mut b);
        assert_ne!(b.round, sent_in_a);
        let _ = b.update(Event::Sent(sent_in_a, answer("c-a")));
        assert!(b.transcript.summary.is_none() && b.transcript.entries.is_empty(), "nothing of A in B");
        let _ = b.update(Event::Opened(sent_in_a, Ok(Opened { choice: None, conversation: None, cards: Vec::new() })));
        assert_eq!(b.phase, Phase::Ready, "a late opening of A changes nothing");
        // "+" while the message is on its way: its answer no longer fills the new chat.
        let _ = a.update(Event::New);
        assert!(!a.sending && !a.follows(), "the new chat takes a message at once");
        let _ = a.update(Event::Sent(sent_in_a, answer("c-a")));
        assert!(a.transcript.entries.is_empty());
        let _ = a.update(Event::Polled(sent_in_a, answer("c-a")));
        assert!(a.transcript.entries.is_empty());
        // The answer of the round open now is taken.
        let round = a.round;
        let _ = a.update(Event::Sent(round, answer("c-new")));
        assert_eq!(a.transcript.summary.as_ref().map(|summary| summary.id.as_str()), Some("c-new"));
    }

    /// Review of #605: "+" while the panel opens changes nothing, so the opening's answer still comes.
    #[test]
    fn a_new_chat_waits_until_the_panel_is_open() {
        let (mut ask, _) = Ask::open(demo_call(), mail_ref("m:1"));
        let round = ask.round;
        let _ = ask.update(Event::New);
        assert_eq!((ask.round, &ask.phase), (round, &Phase::Loading), "nothing to start anew yet");
        let choice = (Choice { engine: demo_engine().id, model: None }, None);
        let opened = Opened { choice: Some(choice), conversation: None, cards: Vec::new() };
        let _ = ask.update(Event::Opened(round, Ok(opened)));
        assert_eq!(ask.phase, Phase::Ready);
    }

    /// Review of #605: the chat about a mail answers with an AI account – the assistant's own choice
    /// where it is one, else the first account, this computer first; issue #664: wherever it computes.
    #[test]
    fn the_chat_answers_with_an_ai_account() {
        let engine =
            |id: &str, kind| Engine { id: id.to_owned(), kind, name: id.to_owned(), problem: None, models: Vec::new() };
        let local = engine("ai:ollama@localhost:11434", EngineKind::Ai);
        let cloud = engine("ai:openrouter@openrouter.ai", EngineKind::Ai);
        let agent = engine("claude-code", EngineKind::Agent);
        let pick =
            |id: &str, model: Option<&str>| (Choice { engine: id.to_owned(), model: model.map(str::to_owned) }, None);
        let engines = vec![agent.clone(), cloud.clone(), local.clone()];
        let chosen = |preferred| ai_choice(preferred, &engines).map(|(c, _)| c);
        // A coding agent never: the first AI account, this computer first.
        assert_eq!(chosen(Some(pick("claude-code", None))), Some(pick(&local.id, None).0));
        // The assistant's own choice stays with its model, here or elsewhere, an Ollama cloud model too.
        assert_eq!(chosen(Some(pick(&local.id, Some("qwen3:8b")))), Some(pick(&local.id, Some("qwen3:8b")).0));
        let ollama_cloud = pick(&local.id, Some("gpt-oss:120b-cloud"));
        assert_eq!(chosen(Some(ollama_cloud.clone())), Some(ollama_cloud.0));
        assert_eq!(chosen(Some(pick(&cloud.id, Some("qwen/qwen3")))), Some(pick(&cloud.id, Some("qwen/qwen3")).0));
        // Only an account elsewhere: it answers.
        assert_eq!(ai_choice(None, &[cloud.clone(), agent.clone()]).map(|(c, _)| c), Some(pick(&cloud.id, None).0));
        // No AI account at all: "Set up an AI…".
        assert_eq!(ai_choice(Some(pick("claude-code", None)), &[agent]), None);
    }

    /// Review of #605: in the demo, a message after "+" starts its own conversation.
    #[test]
    fn the_demo_starts_anew_after_new_chat() {
        let call = demo_call();
        let about = mail_ref("m:1");
        let choice = (Choice { engine: demo_engine().id, model: None }, None);
        let first = send_request(None, Some(&choice), "Hi", &about).expect("a request");
        let (_, entries) = block_on(ask_conversation(call.clone(), first)).expect("an answer");
        assert_eq!(entries.len(), 3);
        let again = send_request(None, Some(&choice), "Anew", &about).expect("a request");
        let (_, entries) = block_on(ask_conversation(call, again)).expect("an answer");
        assert!(matches!(&entries[0].content, EntryContent::Message { text, .. } if text == "Anew"), "{entries:?}");
        assert_eq!(entries.len(), 3, "the mail read again, nothing of the one before");
    }

    /// Review of #605: a suggestion's card shows only in the chat about its own mail.
    #[test]
    fn only_cards_of_the_chats_mail_show() {
        let about = mail_ref("m:1");
        let card = |account: &str, id: &str| Card::Suggestion {
            account: account.to_owned(),
            id: id.to_owned(),
            suggestion: Box::new(Suggestion {
                kind: "event".to_owned(),
                state: SuggestionState::Ready,
                task: None,
                event: None,
                reason: None,
            }),
        };
        assert!(of_this_mail(&card("mail:anna@example.org", "m:1"), &about));
        assert!(!of_this_mail(&card("mail:anna@example.org", "m:2"), &about), "another mail");
        assert!(!of_this_mail(&card("mail:kim@example.org", "m:1"), &about), "another account");
        assert!(!of_this_mail(&Card::Unavailable, &about));
    }

    /// The demo answers a message with the mail read first, and a second message follows the first.
    #[test]
    fn the_demo_answers_and_keeps_the_conversation() {
        let (mut ask, _) = Ask::open(demo_call(), mail_ref("m:1"));
        let opened = block_on(open(ask.call.clone(), mail_ref("m:1")));
        let round = ask.round;
        let _ = ask.update(Event::Opened(round, opened));
        assert_eq!(ask.phase, Phase::Ready);
        let request = send_request(None, ask.choice.as_ref(), "Hi", &ask.about).expect("a request");
        let sent = block_on(ask_conversation(ask.call.clone(), request));
        let _ = ask.update(Event::Sent(round, sent));
        assert_eq!(ask.transcript.entries.len(), 3, "message, mail read, answer");
        assert!(ask.input.is_empty());
        let id = ask.transcript.summary.as_ref().map(|summary| summary.id.clone());
        let again = send_request(id.as_deref(), ask.choice.as_ref(), "And?", &ask.about).expect("a request");
        let sent = block_on(ask_conversation(ask.call.clone(), again));
        let _ = ask.update(Event::Sent(round, sent));
        assert_eq!(ask.transcript.entries.len(), 5);
        assert!(ask.transcript.last_answer_text().is_some());
    }
}
