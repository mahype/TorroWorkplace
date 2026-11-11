//! The panel of a Talk entry in the Notification Hub (issue #643, `docs/chat.md`, "Notification Hub
//! (Stufe 1)"): the conversation behind the entry, and an answer to it, without leaving the hub.
//!
//! **With a conversation** (`Item::conversation`, which the core sets for a caller that may read chat
//! with the entry's account): under the entry's path and title the last [`PAGE`] messages, the
//! unread ones behind the divider "New"; "Show earlier" loads the page before them. Messages of the
//! provider itself (a call started) stand small and grey, the person's own on a raised surface with
//! "You" for a name. A message is answered with "Reply" on it (the bar over its first line, as on
//! the inbox's rows): it shows as a quote over the field and goes along as `reply_to`. Enter sends,
//! Shift + Enter breaks the line; once sent the window marks the entry done (`inbox.set_item_done`,
//! which also moves the read marker in Talk). While the panel is open the window asks every [`POLL`]
//! for what came after the newest message shown.
//!
//! **Without one** (no right to chat, or the account has no chat): the entry's last line and "Open
//! in Talk", nothing else.
//!
//! "Open in Talk" opens the entry's own link in the browser, in both cases. "Open chat" (the chat
//! window, `apps.open_app` with `chat:<id>`) waits for stage 3 of the concept: the core does not
//! take that target yet.
//!
//! Everything goes through requests: `chat.get_messages` and `chat.send_message` ([`Chat`]), the
//! entry's own commands through the window. A failure shows as one quiet line in the panel with what
//! the person can do ([`problem_text`]); nothing is sent later, the text stays in the field.
//!
//! All text of a conversation is foreign and shown as plain text.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use iced::advanced::widget::{Id, Operation, operation};
use iced::keyboard::{self, key::Named};
use iced::widget::scrollable::RelativeOffset;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, row, rule, text_editor};
use iced::{Alignment, Element, Fill, Rectangle, Theme, Vector};
use torro_chat::demo::DemoSource;
use torro_chat::{ChatError, ChatSource, MessageKind, MessagePage, Outgoing, Quote};
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::panel::{self, Panel};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, RowAction};
use torro_gui::{datetime, style, typo};
use torro_inbox::{Item, SourceKind};
use torro_ipc::chat::ChatClient;

use crate::app::Message;
use crate::{detail, list, mail};

/// How many messages the panel shows first and loads with each "Show earlier".
pub const PAGE: u32 = 20;

/// How often the open panel asks for new messages.
pub const POLL: Duration = Duration::from_secs(2);

/// Polls skipped after the core answered `busy`: it asked for fewer calls (ten seconds at [`POLL`]).
const BUSY_PAUSE: u8 = 5;

/// The history's id: [`scroll_to_end`] finds the scrollable around it.
const HISTORY: &str = "talk-history";

/// The reply field's id, to focus it when "Reply" quotes a message.
pub const FIELD: &str = "talk-reply";

/// Lines the reply field shows at least and grows to at most.
const FIELD_LINES: f32 = 2.0;
const FIELD_MAX_LINES: f32 = 6.0;

/// The way to the commands of the area `chat`: the core, or the demo's conversations.
#[derive(Debug, Clone)]
pub enum Chat {
    /// The core at its socket. An entry names its conversation itself (`Item::conversation`), and
    /// only when the window may read chat with the entry's account.
    Core(ChatClient),
    /// `--demo`: the conversations of [`DemoSource::for_hub`]. A Talk entry of the inbox's demo has
    /// no conversation set (there is no core to set it), so it gets the one of its thread, as the
    /// core would name it: `<chat account>/<thread key>`.
    Demo(Arc<DemoSource>),
    /// A demo whose every call fails with this error (`--open talk-offline`), for the error line.
    Failing(ChatError),
    /// A demo without the right to chat (`--open talk-no-chat`): no entry has a conversation.
    Without,
}

impl Chat {
    /// The conversation the panel of `item` shows (`<account>/<token>`), `None` when it shows only
    /// the entry's last line: no Talk entry, or no right to chat ([`Chat`]).
    pub fn conversation_of(&self, item: &Item) -> Option<String> {
        if item.source.kind != SourceKind::Talk {
            return None;
        }
        match self {
            Chat::Core(_) => item.conversation.clone(),
            Chat::Demo(_) | Chat::Failing(_) => item
                .conversation
                .clone()
                .or_else(|| Some(torro_chat::conversation_id(torro_chat::demo::ACCOUNT, &item.thread_key))),
            Chat::Without => None,
        }
    }

    /// Runs `ask` for `conversation` and hands back the answer as the [`Event`] the panel takes.
    pub async fn run(self, conversation: String, ask: Ask) -> Event {
        let answer = match ask {
            Ask::Latest => Answer::Latest(self.page(conversation.clone(), None, None).await),
            Ask::Earlier(before) => Answer::Earlier(self.page(conversation.clone(), Some(before), None).await),
            Ask::Newer(after) => Answer::Newer(self.page(conversation.clone(), None, after).await),
            Ask::Send(outgoing) => Answer::Sent(self.send(outgoing).await),
        };
        Event::Answer(conversation, answer)
    }

    /// `chat.get_messages` with [`PAGE`] as limit.
    async fn page(
        self,
        conversation: String,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<MessagePage, ChatError> {
        match self {
            Chat::Core(client) => client.messages(conversation, before, after, Some(PAGE)).await,
            Chat::Demo(demo) => demo.messages(conversation, before, after, Some(PAGE)).await,
            Chat::Failing(error) => Err(error),
            Chat::Without => Err(ChatError::NotFound),
        }
    }

    /// `chat.send_message`: the id of the sent message.
    async fn send(self, outgoing: Outgoing) -> Result<String, ChatError> {
        match self {
            Chat::Core(client) => client.send(outgoing).await,
            Chat::Demo(demo) => demo.send(outgoing).await,
            Chat::Failing(error) => Err(error),
            Chat::Without => Err(ChatError::NotFound),
        }
    }
}

/// A request the panel needs answered; the window runs it with [`Chat::run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// The newest [`PAGE`] messages (on opening, and to try again after the first load failed).
    Latest,
    /// The page before the message with this id ("Show earlier").
    Earlier(String),
    /// What came after the message with this id; `None` when none is shown yet.
    Newer(Option<String>),
    /// The answer the person wrote.
    Send(Outgoing),
}

/// The answer to an [`Ask`], in the same order.
#[derive(Debug, Clone)]
pub enum Answer {
    Latest(Result<MessagePage, ChatError>),
    Earlier(Result<MessagePage, ChatError>),
    Newer(Result<MessagePage, ChatError>),
    /// The id of the sent message.
    Sent(Result<String, ChatError>),
}

/// What happens in the panel: the person's doing, the timer, and answers.
#[derive(Debug, Clone)]
pub enum Event {
    /// "Show earlier".
    ShowEarlier,
    /// The timer of [`POLL`].
    Poll,
    /// Typing in the reply field.
    Edit(text_editor::Action),
    /// Enter in the field, or the send button.
    Send,
    /// "Reply" on the message with this id: quote it over the field.
    Reply(String),
    /// ✕ on the quote: answer without it.
    DropReply,
    /// "Open in Talk".
    OpenInTalk,
    /// The answer for the conversation named, from [`Chat::run`]; an answer for another conversation
    /// than the one shown now is dropped.
    Answer(String, Answer),
}

/// What the window does for the panel after an [`Event`] ([`Talk::update`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Run this request for the panel's conversation ([`Chat::run`]).
    Ask(Ask),
    /// The answer went out: mark the entry done, if it is open.
    Done,
    /// Open the entry's link in the browser ("Open in Talk").
    Open,
    /// Put the cursor into the reply field ([`FIELD`]).
    Focus,
    /// Show the newest message: the first page is in, or new ones came ([`scroll_to_end`]).
    ScrollEnd,
}

/// Which call failed: it decides the first words of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    Loading,
    Sending,
}

/// The state of the panel of one Talk entry with a conversation. Made new when another entry opens;
/// the window keeps it while the entry is in the panel.
#[derive(Debug)]
pub struct Talk {
    /// The entry's id.
    pub entry: String,
    /// `<account>/<token>`.
    pub conversation: String,
    /// The messages shown, the oldest first.
    messages: Vec<torro_chat::Message>,
    /// The first page is in.
    loaded: bool,
    /// Number of the message read last when the panel opened: the divider "New" stands after it. It
    /// stays while the panel is open, so the divider does not jump with every poll; sending moves it
    /// to the sent message.
    last_read: Option<u64>,
    /// Older messages than the first shown exist.
    more: bool,
    /// The core could not reach the server and answered what it held.
    offline: bool,
    /// The last call that failed, until one of its kind goes through.
    problem: Option<(Doing, ChatError)>,
    reply: text_editor::Content,
    /// The message the answer refers to.
    quote: Option<Quote>,
    sending: bool,
    /// "Show earlier" is on its way.
    earlier: bool,
    /// A poll is on its way: no second one meanwhile.
    polling: bool,
    /// Polls left to skip after `busy`.
    pause: u8,
    /// An answer went out and the window was told to mark the entry done ([`Effect::Done`]): once is
    /// enough, the entry it holds may not know it yet.
    marked: bool,
}

impl Talk {
    /// The panel of the entry `entry` on `conversation`, before anything is loaded. Ask for
    /// [`Ask::Latest`] next.
    pub fn new(entry: String, conversation: String) -> Talk {
        Talk {
            entry,
            conversation,
            messages: Vec::new(),
            loaded: false,
            last_read: None,
            more: false,
            offline: false,
            problem: None,
            reply: text_editor::Content::new(),
            quote: None,
            sending: false,
            earlier: false,
            polling: false,
            pause: 0,
            marked: false,
        }
    }

    /// The message quoted over the field, if one is.
    pub fn quote(&self) -> Option<&Quote> {
        self.quote.as_ref()
    }

    /// The id of the newest message shown.
    pub fn newest(&self) -> Option<String> {
        self.messages.last().map(|message| message.id.clone())
    }

    /// Where the divider "New" goes: the index of the first message after the one read last, when
    /// one is shown. `None` without a known read marker or with nothing unread.
    pub fn divider(&self) -> Option<usize> {
        let read = self.last_read?;
        self.messages.iter().position(|message| message.number() > read)
    }

    /// Takes `event` and says what the window must do for it. Typing, quoting and the answers change
    /// only this state; every request goes out as an [`Effect::Ask`].
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::ShowEarlier => match self.messages.first() {
                Some(first) if self.more && !self.earlier => {
                    self.earlier = true;
                    vec![Effect::Ask(Ask::Earlier(first.id.clone()))]
                }
                _ => Vec::new(),
            },
            Event::Poll => self.poll(),
            Event::Edit(action) => {
                if !self.sending {
                    self.reply.perform(action);
                }
                Vec::new()
            }
            Event::Send => self.send(),
            Event::Reply(id) => {
                let Some(message) = self.messages.iter().find(|message| message.id == id) else { return Vec::new() };
                self.quote = Some(Quote {
                    id: message.id.clone(),
                    author: message.author.name.clone(),
                    text: message.text.clone(),
                });
                vec![Effect::Focus]
            }
            Event::DropReply => {
                self.quote = None;
                vec![Effect::Focus]
            }
            Event::OpenInTalk => vec![Effect::Open],
            Event::Answer(conversation, _) if conversation != self.conversation => Vec::new(),
            Event::Answer(_, answer) => self.take(answer),
        }
    }

    /// A tick of the timer: what came after the newest message, or the first page again while it
    /// failed. Nothing while a call is out, after `busy` for a while, or once the conversation is gone
    /// or the sign-in refused – asking again would not help there.
    fn poll(&mut self) -> Vec<Effect> {
        if self.pause > 0 {
            self.pause -= 1;
            return Vec::new();
        }
        let hopeless = matches!(
            self.problem,
            Some((_, ChatError::NotFound | ChatError::Auth | ChatError::Forbidden(_) | ChatError::NotConnected))
        );
        if self.polling || self.sending || hopeless {
            return Vec::new();
        }
        self.polling = true;
        let ask =
            if self.loaded { Ask::Newer(self.messages.last().map(|message| message.id.clone())) } else { Ask::Latest };
        vec![Effect::Ask(ask)]
    }

    fn send(&mut self) -> Vec<Effect> {
        let text = self.reply.text().trim().to_string();
        if self.sending || text.is_empty() {
            return Vec::new();
        }
        self.sending = true;
        let outgoing = Outgoing {
            conversation: self.conversation.clone(),
            text,
            reply_to: self.quote.as_ref().map(|quote| quote.id.clone()),
            silent: false,
        };
        vec![Effect::Ask(Ask::Send(outgoing))]
    }

    fn take(&mut self, answer: Answer) -> Vec<Effect> {
        match answer {
            Answer::Latest(result) => {
                self.polling = false;
                match result {
                    Ok(page) => {
                        self.last_read = page.last_read.as_deref().and_then(number);
                        self.more = page.more;
                        self.offline = page.offline;
                        self.messages = page.messages;
                        self.loaded = true;
                        self.clear(Doing::Loading);
                        return vec![Effect::ScrollEnd];
                    }
                    Err(error) => self.fail(Doing::Loading, error),
                }
            }
            Answer::Earlier(result) => {
                self.earlier = false;
                match result {
                    Ok(page) => {
                        let first = self.messages.first().map_or(u64::MAX, torro_chat::Message::number);
                        let mut older: Vec<_> =
                            page.messages.into_iter().filter(|message| message.number() < first).collect();
                        older.append(&mut self.messages);
                        self.messages = older;
                        self.more = page.more;
                        self.clear(Doing::Loading);
                    }
                    Err(error) => self.fail(Doing::Loading, error),
                }
            }
            Answer::Newer(result) => {
                self.polling = false;
                match result {
                    Ok(page) => {
                        let (last, before) =
                            (self.messages.last().map_or(0, torro_chat::Message::number), self.messages.len());
                        self.messages.extend(page.messages.into_iter().filter(|message| message.number() > last));
                        self.offline = page.offline;
                        self.clear(Doing::Loading);
                        if self.messages.len() > before {
                            return vec![Effect::ScrollEnd];
                        }
                    }
                    Err(error) => self.fail(Doing::Loading, error),
                }
            }
            Answer::Sent(result) => {
                self.sending = false;
                match result {
                    Ok(id) => {
                        self.reply = text_editor::Content::new();
                        self.quote = None;
                        // Everything up to one's own answer is read.
                        self.last_read = number(&id).or(self.last_read);
                        self.clear(Doing::Sending);
                        // The sent message comes back with the next page of what is newer.
                        let mut effects = Vec::new();
                        if !std::mem::replace(&mut self.marked, true) {
                            effects.push(Effect::Done);
                        }
                        if !self.polling {
                            self.polling = true;
                            effects.push(Effect::Ask(Ask::Newer(self.messages.last().map(|m| m.id.clone()))));
                        }
                        return effects;
                    }
                    Err(error) => self.fail(Doing::Sending, error),
                }
            }
        }
        Vec::new()
    }

    fn fail(&mut self, doing: Doing, error: ChatError) {
        if error == ChatError::Busy {
            self.pause = BUSY_PAUSE;
        }
        self.problem = Some((doing, error));
    }

    /// A call of `doing` went through: its failure is over (a failed send stays until a send works).
    fn clear(&mut self, doing: Doing) {
        if self.problem.as_ref().is_some_and(|(failed, _)| *failed == doing) {
            self.problem = None;
        }
    }
}

/// The number at the end of a message id.
fn number(id: &str) -> Option<u64> {
    torro_chat::split_message(id).map(|(_, number)| number)
}

/// The first words of a failure line: what did not work.
pub fn problem_what(doing: Doing) -> &'static str {
    match doing {
        Doing::Loading => tr("Couldn't load the conversation."),
        Doing::Sending => tr("Couldn't send the message."),
    }
}

/// Why, in words for people, and what to do: offline, sign-in, gone, busy; anything else generic.
pub fn problem_text(error: &ChatError) -> &'static str {
    match error {
        ChatError::Offline => tr("Talk can't be reached right now. Trying again."),
        ChatError::Auth => tr("Talk refused the sign-in. Check the account in the control centre."),
        ChatError::NotFound => tr("This conversation is gone, or you are no longer part of it."),
        ChatError::Busy => tr("Talk is busy. Trying again in a few seconds."),
        ChatError::NotConnected => tr("This account has no Talk."),
        ChatError::Unsupported(_) => tr("Nobody can write in this conversation right now."),
        ChatError::Forbidden(_) | ChatError::Pending => tr("Chat is not allowed for this window."),
        ChatError::Other(_) => tr("Something went wrong. Open it in Talk to go on."),
    }
}

// ---------------------------------------------------------------------------------------------
// Scrolling to the newest message
// ---------------------------------------------------------------------------------------------

/// Scrolls the panel to its end, where the newest message and the divider stand. The panel's
/// scrollable belongs to `torro_gui::panel` and has no id, so this finds it in two passes: the first
/// ([`FindHistory`]) notes which scrollable, counted in the order the tree is walked, is the innermost
/// one around the history ([`HISTORY`]); the second ([`SnapNth`]) snaps that one to its end. Without
/// the history on screen nothing moves.
pub fn scroll_to_end<T: Send + 'static>() -> iced::Task<T> {
    iced::advanced::widget::operate(FindHistory::default())
}

/// The first pass of [`scroll_to_end`].
#[derive(Debug, Default)]
struct FindHistory {
    /// Scrollables seen so far.
    seen: usize,
    /// What the last node was (`Some(Some(n))`: the scrollable `n`), for the `traverse` that follows
    /// it; a widget that only groups others (a column) traverses without a node of its own.
    pending: Option<Option<usize>>,
    /// The scrollables around the node being walked, innermost last (`None` for other nodes).
    around: Vec<Option<usize>>,
    /// The scrollable around the history, once found.
    found: Option<usize>,
}

impl<T> Operation<T> for FindHistory {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        self.around.push(self.pending.take().flatten());
        operate(self);
        self.around.pop();
    }

    fn container(&mut self, id: Option<&Id>, _bounds: Rectangle) {
        self.pending = Some(None);
        if self.found.is_none() && id == Some(&Id::new(HISTORY)) {
            self.found = self.around.iter().rev().find_map(|scrollable| *scrollable);
        }
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        _translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.pending = Some(Some(self.seen));
        self.seen += 1;
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, _state: &mut dyn operation::Focusable) {
        self.pending = Some(None);
    }

    fn text_input(&mut self, _id: Option<&Id>, _bounds: Rectangle, _state: &mut dyn operation::TextInput) {
        self.pending = Some(None);
    }

    fn text(&mut self, _id: Option<&Id>, _bounds: Rectangle, _text: &str) {
        self.pending = Some(None);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, _state: &mut dyn std::any::Any) {
        self.pending = Some(None);
    }

    fn finish(&self) -> operation::Outcome<T> {
        match self.found {
            Some(target) => operation::Outcome::Chain(Box::new(SnapNth { target, seen: 0 })),
            None => operation::Outcome::None,
        }
    }
}

/// The second pass of [`scroll_to_end`]: snaps the scrollable `target` to its end.
#[derive(Debug)]
struct SnapNth {
    target: usize,
    seen: usize,
}

impl<T> Operation<T> for SnapNth {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        _translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if self.seen == self.target {
            state.snap_to(RelativeOffset { x: None, y: Some(1.0) });
        }
        self.seen += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// The panel
// ---------------------------------------------------------------------------------------------

/// The panel of the Talk entry `item`, reached through `view_name`; `now` for the times; `buttons`
/// are the entry's own footer buttons (`detail::buttons`: Done, Later, …), under the reply field.
/// With `talk` the conversation and the field, without it the entry's last line ([`bare_body`]).
pub fn panel<'a>(
    item: &'a Item,
    talk: Option<&'a Talk>,
    view_name: String,
    now: DateTime<Utc>,
    buttons: Vec<Element<'a, Message>>,
) -> Panel<'a, Message> {
    let path = detail::path(item, view_name);
    let buttons: Element<'a, Message> = row(buttons).spacing(space::s()).align_y(Alignment::Center).width(Fill).into();
    let Some(talk) = talk else {
        return panel::panel(path, detail::heading(item), bare_body(item), Message::CloseDetail).footer(vec![buttons]);
    };
    let open = widgets::icon_button(Icon::ExternalLink, tr("Open in Talk"), open_message(item));
    let foot = column![reply_block(talk), buttons].spacing(space::m()).width(Fill);
    panel::panel(path, detail::heading(item), panel::inset(history(talk, now)), Message::CloseDetail)
        .actions(vec![open])
        .footer(vec![foot.into()])
}

/// "Open in Talk" for `item`, when it has a page to open ([`detail::openable`]).
fn open_message(item: &Item) -> Option<Message> {
    matches!(detail::openable(item), Some(torro_inbox::Link::Url { .. })).then_some(Message::Talk(Event::OpenInTalk))
}

/// Without a conversation: the entry's last line, plain, the note on foreign text and "Open in Talk".
fn bare_body<'a>(item: &'a Item) -> Element<'a, Message> {
    let line = detail::message_text(item);
    let mut block = Column::new().spacing(space::s());
    if !line.trim().is_empty() {
        block = block.push(typo::text(typo::body(), line).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
        block = block.push(quiet(detail::foreign_note()));
    }
    let open = widgets::text_button(
        Some(Icon::ExternalLink),
        tr("Open in Talk"),
        style::button::secondary,
        open_message(item),
    );
    panel::inset(column![block, row![open]].spacing(space::l()))
}

/// The conversation: "Show earlier", the messages in blocks per author with the divider "New", and a
/// failure line under them.
fn history<'a>(talk: &'a Talk, now: DateTime<Utc>) -> Element<'a, Message> {
    let mut lines = Column::new().spacing(space::m()).width(Fill);
    if talk.more {
        let earlier = if talk.earlier {
            widgets::busy_button(tr("Loading…"), style::button::ghost)
        } else {
            widgets::text_button(
                None,
                tr("Show earlier"),
                style::button::ghost,
                Some(Message::Talk(Event::ShowEarlier)),
            )
        };
        lines = lines.push(container(earlier).center_x(Fill));
    }
    if !talk.loaded {
        if talk.problem.is_none() {
            lines = lines.push(quiet(tr("Loading the conversation…")));
        }
    } else if talk.messages.is_empty() {
        lines = lines.push(quiet(tr("No messages yet.")));
    }
    let divider = talk.divider();
    let mut start = 0;
    while start < talk.messages.len() {
        if divider == Some(start) {
            lines = lines.push(new_divider());
        }
        let end = block_end(&talk.messages, start, divider);
        lines = lines.push(block(&talk.messages[start..end], talk.quote.as_ref(), now));
        start = end;
    }
    if talk.offline {
        lines = lines.push(quiet(tr("Talk can't be reached. These are the messages saved last.")));
    }
    if let Some((Doing::Loading, error)) = &talk.problem {
        lines = lines.push(failure(Doing::Loading, error));
    }
    container(lines).id(HISTORY).width(Fill).into()
}

/// Where the block starting at `start` ends: messages of one author follow on without a name of their
/// own, until a message of the provider, another author, ten minutes between two or the divider.
fn block_end(messages: &[torro_chat::Message], start: usize, divider: Option<usize>) -> usize {
    let Some(first) = messages.get(start) else { return start };
    if first.kind != MessageKind::Message && first.kind != MessageKind::Deleted {
        return start + 1;
    }
    let mut end = start + 1;
    while let Some(next) = messages.get(end) {
        let previous = &messages[end - 1];
        let same = next.author.name == first.author.name && next.author.own == first.author.own;
        let close = next.at - previous.at <= chrono::TimeDelta::minutes(10);
        let person = matches!(next.kind, MessageKind::Message | MessageKind::Deleted);
        if !same || !close || !person || divider == Some(end) {
            break;
        }
        end += 1;
    }
    end
}

/// One block: a message of the provider small and grey; else the author ("You" for the person's
/// own) and time over the messages, the person's own on a raised surface.
fn block<'a>(messages: &'a [torro_chat::Message], quoted: Option<&Quote>, now: DateTime<Utc>) -> Element<'a, Message> {
    let Some(first) = messages.first() else { return Column::new().into() };
    if !matches!(first.kind, MessageKind::Message | MessageKind::Deleted) {
        let line = format!("{} · {}", list::one_line(&first.text), stamp(first.at, now));
        return quiet(line);
    }
    let name = if first.author.own { tr("You").to_owned() } else { list::one_line(&first.author.name) };
    let head = row![
        container(widgets::ellipsized(typo::body_strong(), name, theme::text)).width(Fill),
        typo::text(typo::meta(), stamp(first.at, now)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let mut parts = Column::new().spacing(space::xs()).push(head);
    for message in messages {
        let chosen = quoted.is_some_and(|quote| quote.id == message.id);
        parts = parts.push(message_line(message, chosen));
    }
    if first.author.own {
        container(parts).padding(space::s()).width(Fill).style(style::container::group).into()
    } else {
        parts.into()
    }
}

/// One message under its block's head: its quote, its text (or that it was deleted), its files; with
/// "Reply" over its first line while the pointer is on it, and always while it is quoted.
fn message_line<'a>(message: &'a torro_chat::Message, chosen: bool) -> Element<'a, Message> {
    let mut parts = Column::new().spacing(space::xs());
    if let Some(quote) = &message.reply_to {
        parts = parts.push(quote_view(quote));
    }
    if message.kind == MessageKind::Deleted {
        parts = parts.push(typo::text(typo::body(), tr("Message deleted")).style(theme::text_3));
    } else if !message.text.trim().is_empty() {
        parts = parts.push(
            typo::text(typo::body(), list::plain(&message.text)).style(theme::text).wrapping(Wrapping::WordOrGlyph),
        );
    }
    for file in &message.files {
        let mark = icon::icon(Icon::Paperclip, icon::SIZE_SM).style(icon_text_3);
        let name = widgets::ellipsized(typo::meta(), list::one_line(&file.name), theme::text_2);
        parts = parts.push(row![mark, name].spacing(space::xs()).align_y(Alignment::Center));
    }
    if message.kind == MessageKind::Deleted {
        return parts.into();
    }
    let reply = RowAction::new(Icon::Reply, tr("Reply"), "", Message::Talk(Event::Reply(message.id.clone())));
    let body = typo::body();
    let middle = body.line_height.to_absolute(body.size).0 / 2.0;
    widgets::row_actions_at(container(parts).width(Fill), vec![reply], chosen, false, middle)
}

/// A quote in a message or over the field: a line at the left, the author in `text_2`, the start of
/// the text in `text_3`, both on one line.
fn quote_view<'a>(quote: &Quote) -> Element<'a, Message> {
    let text =
        if quote.text.trim().is_empty() { tr("Message deleted").to_owned() } else { list::one_line(&quote.text) };
    let words = column![
        widgets::ellipsized(typo::meta(), list::one_line(&quote.author), theme::text_2),
        widgets::ellipsized(typo::meta(), text, theme::text_3),
    ];
    row![rule::vertical(size::HAIRLINE).style(style::rule), words]
        .spacing(space::s())
        .height(iced::Length::Shrink)
        .into()
}

/// "New" between two lines.
fn new_divider<'a>() -> Element<'a, Message> {
    let line = || rule::horizontal(size::HAIRLINE).style(style::rule);
    row![line(), typo::text(typo::caption(), tr("New")).style(theme::text_3).wrapping(Wrapping::None), line()]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// The quote, the field with its send button, and why the last send failed.
fn reply_block<'a>(talk: &'a Talk) -> Element<'a, Message> {
    let mut block = Column::new().spacing(space::s()).width(Fill);
    if let Some(quote) = &talk.quote {
        let mark = icon::icon(Icon::Reply, icon::SIZE_SM).style(icon_text_3);
        let drop = widgets::icon_button(Icon::X, tr("Cancel reply"), Some(Message::Talk(Event::DropReply)));
        block = block.push(
            row![mark, container(quote_view(quote)).width(Fill), drop].spacing(space::s()).align_y(Alignment::Center),
        );
    }
    let mut field = text_editor(&talk.reply)
        .id(FIELD)
        .placeholder(tr("Write a reply…"))
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .min_height(mail::field_height(FIELD_LINES))
        .max_height(mail::field_height(FIELD_MAX_LINES))
        .wrapping(Wrapping::WordOrGlyph)
        .style(style::text_editor)
        .key_binding(binding);
    if !talk.sending {
        field = field.on_action(|action| Message::Talk(Event::Edit(action)));
    }
    let send: Element<'a, Message> = if talk.sending {
        widgets::busy_button(tr("Sending…"), style::button::ghost)
    } else {
        let ready = !talk.reply.text().trim().is_empty();
        widgets::icon_button(Icon::Send, tr("Send"), ready.then_some(Message::Talk(Event::Send)))
    };
    block = block.push(row![field, send].spacing(space::s()).align_y(Alignment::End));
    if let Some((Doing::Sending, error)) = &talk.problem {
        block = block.push(failure(Doing::Sending, error));
    }
    block.into()
}

/// The field's keys: Enter sends, Shift + Enter breaks the line; Escape leaves the field (captured,
/// so the window's Escape does not also close the panel with the text typed – a second one does).
fn binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(press);
    }
    match press.key.as_ref() {
        keyboard::Key::Named(Named::Enter) if !press.modifiers.shift() => {
            Some(text_editor::Binding::Custom(Message::Talk(Event::Send)))
        }
        keyboard::Key::Named(Named::Enter) => Some(text_editor::Binding::Enter),
        keyboard::Key::Named(Named::Escape) => {
            Some(text_editor::Binding::Sequence(vec![text_editor::Binding::Unfocus]))
        }
        _ => text_editor::Binding::from_key_press(press),
    }
}

/// A failure as one quiet line: the alert in the error colour, what did not work in `text_2`, why and
/// what to do in `text_3`.
fn failure<'a>(doing: Doing, error: &ChatError) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE_SM)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let what = typo::text(typo::meta(), problem_what(doing)).style(theme::text_2);
    let why = typo::text(typo::meta(), problem_text(error)).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
    row![alert, column![what, why].spacing(space::xs()).width(Fill)]
        .spacing(space::s())
        .align_y(Alignment::Start)
        .into()
}

/// A quiet sentence (`META`, `text_3`).
fn quiet<'a>(text: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
}

fn icon_text_3(t: &Theme, _: iced::widget::svg::Status) -> iced::widget::svg::Style {
    iced::widget::svg::Style { color: Some(palette(t).text_3) }
}

/// "14:30" today, else "Mon, 12 Oct · 14:30", in local time.
fn stamp(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (local, today) = (at.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    let time = datetime::format_time(local.time());
    if local.date_naive() == today {
        time
    } else {
        format!("{} · {time}", datetime::format_short_date(local.date_naive(), today))
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;
    use torro_chat::demo::LENA;
    use torro_chat::message_id;

    use super::*;

    fn demo() -> Chat {
        Chat::Demo(Arc::new(DemoSource::for_hub(DateTime::from_timestamp(1_800_000_000, 0).expect("a time"))))
    }

    /// Runs every request of `effects` against `chat` and hands the answers back, as the window does;
    /// what is left besides the requests.
    fn run(talk: &mut Talk, chat: &Chat, effects: Vec<Effect>) -> Vec<Effect> {
        let mut rest = Vec::new();
        for effect in effects {
            match effect {
                Effect::Ask(ask) => {
                    let answer = block_on(chat.clone().run(talk.conversation.clone(), ask));
                    let more = talk.update(answer);
                    rest.extend(run(talk, chat, more));
                }
                other => rest.push(other),
            }
        }
        rest
    }

    fn opened(chat: &Chat) -> Talk {
        let mut talk = Talk::new("demo-17".into(), LENA.into());
        let _ = run(&mut talk, chat, vec![Effect::Ask(Ask::Latest)]);
        talk
    }

    fn type_in(talk: &mut Talk, text: &str) {
        let paste = text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text.to_string())));
        assert!(talk.update(Event::Edit(paste)).is_empty());
    }

    #[test]
    fn the_first_page_holds_twenty_with_new_before_the_unread() {
        let talk = opened(&demo());
        assert!(talk.loaded);
        assert_eq!(talk.messages.len(), PAGE as usize);
        assert!(talk.more, "older ones wait behind \"Show earlier\"");
        // The demo's last two of 24 are unread; the page holds 5 to 24.
        let divider = talk.divider().expect("a divider");
        assert_eq!(talk.messages[divider].number(), 23);
        assert_eq!(talk.messages.len() - divider, 2);
    }

    #[test]
    fn show_earlier_puts_the_older_ones_in_front() {
        let chat = demo();
        let mut talk = opened(&chat);
        let effects = talk.update(Event::ShowEarlier);
        assert_eq!(effects, vec![Effect::Ask(Ask::Earlier(message_id(LENA, 5)))]);
        assert!(talk.update(Event::ShowEarlier).is_empty(), "one at a time");
        let _ = run(&mut talk, &chat, effects);
        let numbers: Vec<u64> = talk.messages.iter().map(torro_chat::Message::number).collect();
        assert_eq!(numbers, (1..=24).collect::<Vec<_>>());
        assert!(!talk.more);
        assert!(talk.update(Event::ShowEarlier).is_empty(), "nothing more to show");
    }

    #[test]
    fn sending_quotes_marks_done_once_and_brings_the_answer_in() {
        let chat = demo();
        let mut talk = opened(&chat);
        assert_eq!(talk.update(Event::Reply(message_id(LENA, 24))), vec![Effect::Focus]);
        assert_eq!(talk.quote.as_ref().map(|quote| quote.author.as_str()), Some("Lena Hoffmann"));
        assert!(talk.update(Event::Send).is_empty(), "an empty field sends nothing");
        type_in(&mut talk, "  Yes, at two.  ");
        let effects = talk.update(Event::Send);
        let Some(Effect::Ask(Ask::Send(outgoing))) = effects.first() else { panic!("a send: {effects:?}") };
        assert_eq!(outgoing.text, "Yes, at two.");
        assert_eq!(outgoing.reply_to, Some(message_id(LENA, 24)));
        assert!(talk.update(Event::Send).is_empty(), "not twice while on its way");
        assert_eq!(run(&mut talk, &chat, effects), vec![Effect::Done, Effect::ScrollEnd], "and the answer shows");
        let sent = talk.messages.last().expect("the answer");
        assert!(sent.author.own && sent.text == "Yes, at two.");
        assert_eq!(sent.reply_to.as_ref().map(|quote| quote.id.clone()), Some(message_id(LENA, 24)));
        assert!(talk.quote.is_none() && talk.reply.text().trim().is_empty(), "field and quote are cleared");
        assert_eq!(talk.divider(), None, "everything up to one's own answer is read");
        type_in(&mut talk, "And bring the drafts.");
        let effects = talk.update(Event::Send);
        assert!(!run(&mut talk, &chat, effects).contains(&Effect::Done), "the entry is marked done once");
    }

    #[test]
    fn polling_appends_what_came_after() {
        let chat = demo();
        let mut talk = opened(&chat);
        if let Chat::Demo(demo) = &chat {
            let outgoing =
                Outgoing { conversation: LENA.into(), text: "From the browser".into(), ..Outgoing::default() };
            block_on(demo.send(outgoing)).expect("sent elsewhere");
        }
        let effects = talk.update(Event::Poll);
        assert_eq!(effects, vec![Effect::Ask(Ask::Newer(Some(message_id(LENA, 24))))]);
        assert!(talk.update(Event::Poll).is_empty(), "no second poll while one is out");
        let _ = run(&mut talk, &chat, effects);
        assert_eq!(talk.messages.last().map(|message| message.text.as_str()), Some("From the browser"));
        assert_eq!(talk.messages.len(), PAGE as usize + 1);
    }

    #[test]
    fn a_failure_is_a_line_and_the_text_stays() {
        let chat = Chat::Failing(ChatError::Offline);
        let mut talk = opened(&chat);
        assert!(!talk.loaded);
        assert_eq!(talk.problem, Some((Doing::Loading, ChatError::Offline)));
        assert_eq!(problem_what(Doing::Loading), tr("Couldn't load the conversation."));
        assert_eq!(problem_text(&ChatError::Offline), tr("Talk can't be reached right now. Trying again."));
        // Offline is worth asking again: the next poll asks for the first page.
        assert_eq!(talk.update(Event::Poll), vec![Effect::Ask(Ask::Latest)]);
        talk.polling = false;
        type_in(&mut talk, "Hello");
        let effects = talk.update(Event::Send);
        assert!(run(&mut talk, &chat, effects).is_empty());
        assert_eq!(talk.problem, Some((Doing::Sending, ChatError::Offline)));
        assert_eq!(problem_what(Doing::Sending), tr("Couldn't send the message."));
        assert_eq!(talk.reply.text().trim(), "Hello", "nothing is sent later; the text stays");
    }

    #[test]
    fn a_gone_conversation_stops_polling_and_busy_pauses_it() {
        let mut gone = opened(&Chat::Failing(ChatError::NotFound));
        assert!(gone.update(Event::Poll).is_empty());
        let mut busy = opened(&Chat::Failing(ChatError::Busy));
        for _ in 0..BUSY_PAUSE {
            assert!(busy.update(Event::Poll).is_empty());
        }
        assert_eq!(busy.update(Event::Poll), vec![Effect::Ask(Ask::Latest)]);
    }

    #[test]
    fn an_answer_for_another_conversation_is_dropped() {
        let mut talk = Talk::new("demo-17".into(), LENA.into());
        let page = MessagePage { messages: Vec::new(), last_read: None, more: false, offline: false };
        let _ = talk.update(Event::Answer("other/abc".into(), Answer::Latest(Ok(page))));
        assert!(!talk.loaded);
    }

    /// A scrollable's state that notes where it was snapped to.
    #[derive(Default)]
    struct Snapped(Option<RelativeOffset<Option<f32>>>);

    impl operation::Scrollable for Snapped {
        fn snap_to(&mut self, offset: RelativeOffset<Option<f32>>) {
            self.0 = Some(offset);
        }
        fn scroll_to(&mut self, _: iced::widget::scrollable::AbsoluteOffset<Option<f32>>) {}
        fn scroll_by(&mut self, _: iced::widget::scrollable::AbsoluteOffset, _: Rectangle, _: Rectangle) {}
    }

    /// Walks a window as Iced does: a list in a scrollable, beside it the panel's scrollable with a
    /// column holding a text and the history.
    fn walk(operation: &mut dyn Operation<()>, states: &mut [Snapped; 2]) {
        let [list, panel] = states;
        let nothing = Rectangle::default();
        operation.scrollable(None, nothing, nothing, Vector::ZERO, list);
        operation.traverse(&mut |op| op.text(None, nothing, "a row"));
        operation.container(None, nothing);
        operation.traverse(&mut |op| {
            op.scrollable(None, nothing, nothing, Vector::ZERO, panel);
            op.traverse(&mut |op| {
                op.traverse(&mut |op| {
                    op.text(None, nothing, "Lena Hoffmann");
                    op.container(Some(&Id::new(HISTORY)), nothing);
                    op.traverse(&mut |_| {});
                });
            });
        });
    }

    #[test]
    fn scrolling_to_the_end_moves_the_panel_only() {
        let mut find = FindHistory::default();
        let mut states = [Snapped::default(), Snapped::default()];
        walk(&mut find, &mut states);
        assert_eq!(find.found, Some(1), "the second scrollable holds the history");
        let operation::Outcome::Chain(mut snap) = Operation::<()>::finish(&find) else { panic!("a second pass") };
        walk(snap.as_mut(), &mut states);
        assert_eq!(states[0].0, None, "the list stays where it is");
        assert_eq!(states[1].0, Some(RelativeOffset { x: None, y: Some(1.0) }));
    }

    #[test]
    fn only_talk_entries_with_a_right_to_chat_have_a_conversation() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).expect("a time");
        let items = torro_inbox::demo::sample(now);
        let talk = items.iter().find(|item| item.source.kind == SourceKind::Talk).expect("a Talk entry");
        let other = items.iter().find(|item| item.source.kind != SourceKind::Talk).expect("another entry");
        assert_eq!(demo().conversation_of(talk), Some(LENA.to_string()), "the demo names it like the core");
        assert_eq!(demo().conversation_of(other), None);
        assert_eq!(Chat::Without.conversation_of(talk), None);
        let core = Chat::Core(ChatClient::default());
        assert_eq!(core.conversation_of(talk), None, "the core sets it, or there is no right to chat");
        let mut named = talk.clone();
        named.conversation = Some(LENA.into());
        assert_eq!(core.conversation_of(&named), Some(LENA.to_string()));
    }
}
