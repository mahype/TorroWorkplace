//! A chat window (`torrochat --conversation=<id>`, `docs/chat.md`, "Chatfenster"): its state
//! ([`ChatState`]) and what its clicks and answers do ([`App::chat_update`]).
//!
//! - **Live:** every [`POLL`] the window asks `chat.get_messages` with `after` the newest message it
//!   has; that is a look into the core's memory, and it tells the core to hold the conversation live
//!   (long polling, `crates/torroworkplace/src/chat/live.rs`). "Show earlier" asks with `before`.
//! - **Read:** while the window has the keyboard and the newest message is in view, the window
//!   sends `chat.mark_read` – once when it opens, then for each message that comes. "Mark as unread"
//!   (`chat.mark_unread`) holds that back until the next message of someone else.
//! - **Writing:** Enter sends (`chat.send_message`), Shift+Enter starts a new line. "Reply" quotes a
//!   message above the field. `@` asks `chat.suggest_mentions` for the word after it; a pick writes
//!   `@Name`, and sending turns the picked names into the provider's `@"<id>"`
//!   ([`crate::timeline::outgoing`]). A message that could not go stays in the field (nothing is sent
//!   later, decision 8).

use std::time::Duration;

use iced::widget::operation;
use iced::widget::scrollable::Viewport;
use iced::widget::text_editor::{self, Content, Cursor, Position};
use iced::{Point, Subscription, Task};
use torro_chat::{ChatError, Mention, Message as ChatMessage, Outgoing, Quote};
use torro_core::i18n::tr;

use super::{App, Menu, Message, chat_failed};
use crate::launch::Open;
use crate::timeline;

/// How often a chat window asks for new messages (`docs/chat.md`, decision 9).
pub const POLL: Duration = Duration::from_secs(2);

/// The composer.
pub const EDITOR: &str = "chat-editor";

/// The history.
pub const HISTORY: &str = "chat-history";

/// The most suggestions under an `@`.
const SUGGESTIONS: usize = 6;

/// A chat window's state.
pub struct ChatState {
    /// The conversation (`<account>/<token>`).
    pub id: String,
    /// The messages held, the oldest first.
    pub messages: Vec<ChatMessage>,
    /// Older messages exist on the server.
    pub more: bool,
    /// The number of the message the person had read when the window opened: "New" goes after it.
    pub new_after: Option<u64>,
    /// The first page came (or failed).
    pub loaded: bool,
    /// The conversation does not exist (any more), or the person is no longer in it.
    pub gone: bool,
    /// Why the first page could not be read; `None` once it came.
    pub error: Option<String>,
    /// The server could not be reached: the history is what the core still held.
    pub offline: bool,
    /// An ask for new messages is out.
    pub polling: bool,
    pub loading_earlier: bool,
    pub editor: Content,
    /// The message the next one answers.
    pub reply: Option<Quote>,
    /// The mentions picked from the suggestions for the text in the field.
    pub picked: Vec<Mention>,
    pub suggestions: Vec<Mention>,
    /// The word after `@` the suggestions were asked for.
    pub asked: Option<String>,
    pub sending: bool,
    /// Why the last message could not go.
    pub send_error: Option<String>,
    /// The history shows its end.
    pub at_bottom: bool,
    /// "Mark as unread" was chosen: no read mark until someone writes.
    pub hold_read: bool,
    /// The newest message number marked read by this window.
    pub read_up_to: Option<u64>,
    /// The message under the pointer: it shows "Reply".
    pub hover: Option<String>,
    /// A demo state still to show once the first page is in.
    pub open: Option<Open>,
}

/// What happens in a chat window.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The conversations were listed again (name, kind, unread).
    Listed,
    /// The window's tick: load the first page if it is not there.
    Refresh,
    /// The window got the keyboard.
    Seen,
    Escape,
    Page(Result<torro_chat::MessagePage, ChatError>),
    Earlier,
    EarlierPage(Result<torro_chat::MessagePage, ChatError>),
    Poll,
    Polled(Result<torro_chat::MessagePage, ChatError>),
    Edit(text_editor::Action),
    Send,
    Sent(Result<String, ChatError>),
    Reply(String),
    CancelReply,
    Suggested(String, Result<Vec<Mention>, ChatError>),
    Pick(Mention),
    MessageMenu(String, Point),
    HeadMenu,
    MarkUnread,
    MarkedUnread(Result<(), ChatError>),
    MarkedRead(Result<(), ChatError>),
    Copy(String),
    Scrolled(Viewport),
    Hover(Option<String>),
}

impl ChatState {
    pub fn new(id: String, open: Option<Open>) -> ChatState {
        ChatState {
            id,
            messages: Vec::new(),
            more: false,
            new_after: None,
            loaded: false,
            gone: false,
            error: None,
            offline: false,
            polling: false,
            loading_earlier: false,
            editor: Content::new(),
            reply: None,
            picked: Vec::new(),
            suggestions: Vec::new(),
            asked: None,
            sending: false,
            send_error: None,
            at_bottom: true,
            hold_read: false,
            read_up_to: None,
            hover: None,
            open,
        }
    }

    /// A chat window listens for Escape always: it cancels suggestions or a reply, and closes an
    /// empty window.
    pub fn escapes(&self) -> bool {
        true
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        if self.loaded && !self.gone { iced::time::every(POLL).map(|_| Msg::Poll) } else { Subscription::none() }
    }

    /// The newest message number held.
    pub fn newest(&self) -> Option<u64> {
        self.messages.last().map(ChatMessage::number)
    }

    /// The message with `id`.
    pub fn message(&self, id: &str) -> Option<&ChatMessage> {
        self.messages.iter().find(|message| message.id == id)
    }

    /// Adds `incoming` (any order, maybe known already) and keeps the history ordered by number.
    /// Whether a message of someone else came.
    fn merge(&mut self, incoming: Vec<ChatMessage>) -> bool {
        let mut others = false;
        for message in incoming {
            match self.messages.iter_mut().find(|known| known.id == message.id) {
                Some(known) => *known = message,
                None => {
                    others |= !message.author.own;
                    self.messages.push(message);
                }
            }
        }
        self.messages.sort_by_key(ChatMessage::number);
        others
    }

    /// The text before the cursor in the composer.
    fn before_cursor(&self) -> String {
        let Position { line, column } = self.editor.cursor().position;
        let mut before = String::new();
        for (index, text) in self.editor.lines().enumerate() {
            if index < line {
                before.push_str(&text.text);
                before.push('\n');
            } else {
                let end =
                    (0..=column.min(text.text.len())).rev().find(|at| text.text.is_char_boundary(*at)).unwrap_or(0);
                before.push_str(&text.text[..end]);
                break;
            }
        }
        before
    }

    /// The text after the cursor in the composer.
    fn after_cursor(&self) -> String {
        let text = self.editor.text();
        text.get(self.before_cursor().len()..).unwrap_or_default().to_owned()
    }

    /// Replaces the composer's text by `before` + `after`, the cursor between them.
    fn set_text(&mut self, before: &str, after: &str) {
        self.editor = Content::with_text(&format!("{before}{after}"));
        let line = before.matches('\n').count();
        let column = before.rsplit('\n').next().map_or(0, str::len);
        self.editor.move_to(Cursor { position: Position { line, column }, selection: None });
    }
}

impl App {
    /// What happens in a chat window; nothing in the contact list.
    pub(crate) fn chat_update(&mut self, message: Msg) -> Task<Message> {
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let backend = self.backend.clone();
        let id = chat.id.clone();
        let wrap =
            |msg: fn(Result<torro_chat::MessagePage, ChatError>) -> Msg| move |result| Message::Chat(msg(result));
        match message {
            Msg::Listed => {}
            Msg::Refresh => {
                if !chat.loaded && !chat.polling {
                    chat.polling = true;
                    return Task::perform(backend.messages(id, None, None), wrap(Msg::Page));
                }
            }
            Msg::Seen => return self.mark_read_if_seen(),
            Msg::Escape => {
                if !chat.suggestions.is_empty() {
                    chat.suggestions.clear();
                    chat.asked = None;
                } else if chat.reply.is_some() {
                    chat.reply = None;
                } else if chat.editor.text().trim().is_empty() && self.menu.is_none() {
                    // An empty window closes; one with text keeps it.
                    return iced::window::latest().and_then(iced::window::close);
                }
            }
            Msg::Page(result) => {
                chat.polling = false;
                chat.loaded = true;
                match result {
                    Ok(page) => {
                        chat.new_after = page.last_read.as_deref().and_then(torro_chat::split_message).map(|(_, n)| n);
                        chat.more = page.more;
                        chat.offline = page.offline;
                        chat.error = None;
                        chat.merge(page.messages);
                        let demo = self.apply_open();
                        return Task::batch([self.mark_read_if_seen(), demo, operation::snap_to_end(HISTORY)]);
                    }
                    Err(ChatError::NotFound) => chat.gone = true,
                    Err(err) => chat.error = Some(chat_failed(&err)),
                }
            }
            Msg::Earlier => {
                let Some(first) = chat.messages.first().map(|message| message.id.clone()) else { return Task::none() };
                if chat.loading_earlier {
                    return Task::none();
                }
                chat.loading_earlier = true;
                return Task::perform(backend.messages(id, Some(first), None), wrap(Msg::EarlierPage));
            }
            Msg::EarlierPage(result) => {
                chat.loading_earlier = false;
                match result {
                    Ok(page) => {
                        chat.more = page.more;
                        chat.merge(page.messages);
                    }
                    Err(err) => {
                        let text = chat_failed(&err);
                        self.notify(text);
                    }
                }
            }
            Msg::Poll => {
                if chat.polling || !chat.loaded || chat.gone {
                    return Task::none();
                }
                chat.polling = true;
                let after = chat.messages.last().map(|message| message.id.clone());
                return Task::perform(backend.messages(id, None, after), wrap(Msg::Polled));
            }
            Msg::Polled(result) => {
                chat.polling = false;
                match result {
                    Ok(page) => {
                        chat.offline = page.offline;
                        if chat.merge(page.messages) {
                            chat.hold_read = false;
                        }
                        return self.mark_read_if_seen();
                    }
                    Err(ChatError::NotFound) => chat.gone = true,
                    Err(ChatError::Offline) => chat.offline = true,
                    // Another try at the next poll.
                    Err(_) => {}
                }
            }
            Msg::Edit(action) => {
                let edits = action.is_edit();
                chat.editor.perform(action);
                if edits {
                    chat.send_error = None;
                }
                return self.suggest();
            }
            Msg::Send => {
                let text = timeline::outgoing(&chat.editor.text(), &chat.picked);
                if text.is_empty() || chat.sending || chat.gone {
                    return Task::none();
                }
                chat.sending = true;
                chat.send_error = None;
                let outgoing = Outgoing {
                    conversation: id,
                    text,
                    reply_to: chat.reply.as_ref().map(|quote| quote.id.clone()),
                    silent: false,
                };
                return Task::perform(backend.send(outgoing), |result| Message::Chat(Msg::Sent(result)));
            }
            Msg::Sent(result) => {
                chat.sending = false;
                match result {
                    Ok(_) => {
                        chat.editor = Content::new();
                        chat.reply = None;
                        chat.picked.clear();
                        chat.suggestions.clear();
                        chat.asked = None;
                        chat.at_bottom = true;
                        return Task::batch([
                            self.chat_update(Msg::Poll),
                            operation::snap_to_end(HISTORY),
                            operation::focus(EDITOR),
                        ]);
                    }
                    // The text stays in the field; nothing is sent later (decision 8).
                    Err(err) => chat.send_error = Some(chat_failed(&err)),
                }
            }
            Msg::Reply(message) => {
                self.menu = None;
                if let Some(quoted) = chat.message(&message) {
                    chat.reply = Some(Quote {
                        id: quoted.id.clone(),
                        author: quoted.author.name.clone(),
                        text: quoted.text.clone(),
                    });
                }
                return operation::focus(EDITOR);
            }
            Msg::CancelReply => chat.reply = None,
            Msg::Suggested(query, result) => {
                if chat.asked.as_deref() != Some(query.as_str()) {
                    return Task::none();
                }
                chat.suggestions = result.unwrap_or_default().into_iter().take(SUGGESTIONS).collect();
            }
            Msg::Pick(mention) => {
                let before = timeline::with_mention(&chat.before_cursor(), &mention);
                let after = chat.after_cursor();
                chat.set_text(&before, &after);
                if !chat.picked.contains(&mention) {
                    chat.picked.push(mention);
                }
                chat.suggestions.clear();
                chat.asked = None;
                return operation::focus(EDITOR);
            }
            Msg::MessageMenu(message, at) => self.menu = Some(Menu::Message { id: message, at }),
            Msg::HeadMenu => {
                self.menu = if self.menu == Some(Menu::Head) { None } else { Some(Menu::Head) };
            }
            Msg::MarkUnread => {
                self.menu = None;
                chat.hold_read = true;
                return Task::perform(backend.mark_unread(id), |result| Message::Chat(Msg::MarkedUnread(result)));
            }
            Msg::MarkedUnread(result) => match result {
                Ok(()) => {
                    chat.read_up_to = None;
                    self.notify(tr("Marked as unread").to_owned());
                }
                Err(err) => {
                    chat.hold_read = false;
                    let text = chat_failed(&err);
                    self.notify(text);
                }
            },
            Msg::MarkedRead(Ok(())) => {}
            // A read mark that failed is tried again with the next message or look; nothing to tell.
            Msg::MarkedRead(Err(_)) => chat.read_up_to = None,
            Msg::Copy(text) => {
                self.menu = None;
                self.notify(tr("Copied").to_owned());
                return iced::clipboard::write(text);
            }
            Msg::Scrolled(viewport) => {
                let bottom = viewport.relative_offset().y >= 0.99
                    || viewport.content_bounds().height <= viewport.bounds().height;
                let reached = bottom && !chat.at_bottom;
                chat.at_bottom = bottom;
                if reached {
                    return self.mark_read_if_seen();
                }
            }
            Msg::Hover(message) => chat.hover = message,
        }
        Task::none()
    }

    /// `chat.mark_read` when the person sees the newest message: the window has the keyboard, the
    /// history shows its end, "Mark as unread" holds nothing back, and it was not marked yet.
    fn mark_read_if_seen(&mut self) -> Task<Message> {
        let focused = self.focused;
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let newest = chat.newest();
        if !focused || !chat.at_bottom || chat.hold_read || newest.is_none() || chat.read_up_to >= newest {
            return Task::none();
        }
        chat.read_up_to = newest;
        Task::perform(self.backend.clone().mark_read(chat.id.clone()), |result| Message::Chat(Msg::MarkedRead(result)))
    }

    /// Asks for suggestions when the cursor stands in a word after `@`; drops them otherwise.
    fn suggest(&mut self) -> Task<Message> {
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let before = chat.before_cursor();
        let Some(query) = timeline::mention_query(&before).map(str::to_owned) else {
            chat.suggestions.clear();
            chat.asked = None;
            return Task::none();
        };
        if chat.asked.as_deref() == Some(query.as_str()) {
            return Task::none();
        }
        chat.asked = Some(query.clone());
        let id = chat.id.clone();
        Task::perform(self.backend.clone().suggest_mentions(id, query.clone()), move |result| {
            Message::Chat(Msg::Suggested(query.clone(), result))
        })
    }

    /// The demo states of a chat window that need its messages (`--open reply`, `message-menu`).
    fn apply_open(&mut self) -> Task<Message> {
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let theirs = chat.messages.iter().rev().find(|message| !message.author.own && !timeline::is_system(message));
        match chat.open.take() {
            Some(Open::Reply) => {
                if let Some(quoted) = theirs {
                    chat.reply = Some(Quote {
                        id: quoted.id.clone(),
                        author: quoted.author.name.clone(),
                        text: quoted.text.clone(),
                    });
                }
                chat.set_text("Sure, I look at it now. @", "");
                self.suggest()
            }
            Some(Open::MessageMenu) => {
                if let Some(message) = theirs {
                    self.menu = Some(Menu::Message { id: message.id.clone(), at: Point::new(48.0, 12.0) });
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
