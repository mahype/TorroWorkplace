//! The cards in the chat (task WZ.8, `docs/assistent.md` "Datei-Karte", prototype
//! `docs/prototypes/assistant/cards.html`): what the core resolved for a card entry
//! (`assistant.get_cards`), drawn as a `widgets::card` – never anything a model wrote.
//!
//! - **File card** (on `torro_gui::content_card` since issue #784): the paperclip with the file's template and size
//!   in the head, its name, the path on one line (cut with "…"), and three buttons – "Open", "Save as…", "Send by
//!   mail" – with their words where the card has room, else as icons with the word as tooltip ("Open" keeps its word
//!   first).
//! - **No longer available:** the file went, changed or is out of reach; no buttons, and the tile
//!   only its quiet mark in `text_3` without its surface, so it never reads as a file to open.
//! - **Suggestion of a mail** (issue #605, a chat about a mail): only its short form – the kind's
//!   tile, what was read out (or the kind's word) and "Accept it in Mail" – and one button, "Open in
//!   Mail": accepting opens the filled dialog in the mail window, where the mail shows the whole card.
//! - **Short form:** one line for mini and strip tiles (`ContentCard::short`: icon, name, first line, "Open").
//! - **Content cards** (issue #784, `docs/inhaltskarten.md`): a link, a mail, an event, a task, a draft, a contact and
//!   a group, each in its module below (`cards/*.rs`) on the shared `torro_gui::content_card`; a kind the window does
//!   not know shows as "No longer available". Every button is the person's click with one request
//!   ([`App::card_actions`] lists them as the cards show them).

mod contact;
mod draft;
mod event;
mod group;
mod link;
mod mail;
mod todo;

use chrono::Utc;
use iced::widget::{container, row};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::{Action, ContentCard, Head};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo};
use torro_ipc::Request;
use torro_ipc::actions::Suggestion;
use torro_ipc::assistant::Card;
use torro_ipc::tools::FileInfo;

use super::tinted;
use crate::app::{App, Message};
use crate::chat;

/// A button of a content card as the person's click: sends `request` with the app key ([`Message::CardRequest`]);
/// when it fails the window says `failed`. Either way the cards are asked for again.
fn card_request(request: Request, failed: &'static str) -> Message {
    Message::CardRequest(Box::new(request), failed)
}

/// `card` with `actions`: the first its main button, the others after it (at most `content_card::MAX_MORE`).
fn with_actions(card: ContentCard<Message>, actions: Vec<Action<Message>>) -> ContentCard<Message> {
    let mut actions = actions.into_iter();
    let card = match actions.next() {
        Some(main) => card.main(main),
        None => card,
    };
    actions.fold(card, ContentCard::more)
}

/// The icon of a file of the assistant's tools: a paperclip, as an attachment in the mail window
/// (`widgets::file_chip`) – what the file becomes when it goes by mail.
pub(super) const FILE_ICON: Icon = Icon::Paperclip;

impl App {
    /// The card of the entry `seq` as the core resolved it last; `None` while it is not resolved,
    /// so it takes no room (and no gap) until it can show something.
    pub(super) fn card(&self, seq: u64) -> Option<Element<'_, Message>> {
        let card = self.cards.get(&seq)?;
        Some(match card {
            Card::File(file) => file_content(file).view(self.card_room()),
            Card::Suggestion { account, id, suggestion } => suggestion_card(account, id, suggestion),
            Card::Unavailable | Card::Unknown => gone_card(self.card_room()),
            content => self.content_view(seq, content, self.card_room())?,
        })
    }

    /// How wide a content card is (issue #784): the history's reading column within the window, so a card grows with
    /// the answer above it (its picture stays at most `size::card_picture_max()` high).
    fn card_room(&self) -> f32 {
        (self.size.width - 2.0 * space::l()).clamp(0.0, size::content_max())
    }

    /// A content card (issue #784) of the entry `seq`, at most `room` wide, from its module; `None` for a link that
    /// stays a link, and for the kinds [`App::card`] draws itself (a file, a suggestion, gone).
    fn content_view<'a>(&'a self, seq: u64, card: &'a Card, room: f32) -> Option<Element<'a, Message>> {
        let now = Utc::now();
        Some(match card {
            Card::Link { url, card, ask } => {
                let picture =
                    self.current.clone().and_then(|conversation| self.card_pictures.get(&(conversation, seq)));
                return self.link_card(seq, url, card.as_deref(), ask.as_ref(), picture, room);
            }
            Card::Mail { message, .. } => mail::view(message, now, room),
            Card::Event { event, calendar, color, overlaps, more_overlaps } => {
                event::view(event, calendar, color.as_deref(), overlaps, *more_overlaps, now, room)
            }
            Card::Todo { todo, list, color } => todo::view(todo, list, color.as_deref(), now, room),
            Card::Draft { draft, approval } => draft::view(draft, approval.as_deref(), room),
            Card::Contact { contact } => contact::view(contact, room),
            Card::Group { items, area, total } => self.group_card(seq, items, *area, *total, room),
            Card::Unavailable | Card::Unknown => gone_card(room),
            Card::File(_) | Card::Suggestion { .. } => return None,
        })
    }

    /// A content card (issue #784) in one line, from its module; `None` as [`App::content_view`]. A group in one line
    /// is its first item's.
    fn content_line<'a>(&'a self, card: &'a Card) -> Option<Element<'a, Message>> {
        let now = Utc::now();
        Some(match card {
            Card::Link { url, card, .. } => return self.link_line(url, card.as_deref()),
            Card::Mail { message, .. } => mail::line(message, now),
            Card::Event { event, calendar, .. } => event::line(event, calendar, now),
            Card::Todo { todo, list, .. } => todo::line(todo, list, now),
            Card::Draft { draft, approval } => draft::line(draft, approval.as_deref()),
            Card::Contact { contact } => contact::line(contact),
            Card::Group { items, .. } => return self.content_line(items.first()?),
            Card::Unavailable | Card::Unknown => gone_line(),
            Card::File(_) | Card::Suggestion { .. } => return None,
        })
    }

    /// The buttons of the content card of the entry `seq` as it shows them now, each with what its click sends
    /// (`None`: switched off), the main one first; a group's are its items' in order, then "Show all". Empty for a
    /// card not resolved yet, a suggestion and a card that is gone. The views take their buttons from the same
    /// functions, so this is what a click on the card does; only the tests ask.
    #[cfg(test)]
    pub(crate) fn card_actions(&self, seq: u64) -> Vec<Action<Message>> {
        self.cards.get(&seq).map(|card| self.actions_of(seq, card)).unwrap_or_default()
    }

    /// The buttons of `card`, the content card of the entry `seq` or an item of its group.
    #[cfg(test)]
    fn actions_of(&self, seq: u64, card: &Card) -> Vec<Action<Message>> {
        match card {
            Card::Link { url, card, ask } => self.link_actions(seq, url, card.as_deref(), ask.as_ref()),
            Card::Mail { message, .. } => mail::actions(message),
            Card::Event { event, .. } => event::actions(event),
            Card::Todo { todo, .. } => todo::actions(todo),
            Card::Draft { draft, approval } => draft::actions(draft, approval.as_deref()),
            Card::Contact { contact } => contact::actions(contact),
            Card::Group { items, area, total } => {
                let mut actions: Vec<Action<Message>> =
                    items.iter().flat_map(|item| self.actions_of(seq, item)).collect();
                actions.extend(group::show_all(*area, *total));
                actions
            }
            Card::File(file) => file_actions(file),
            Card::Suggestion { .. } | Card::Unavailable | Card::Unknown => Vec::new(),
        }
    }

    /// The card of the entry `seq` as one line, for mini and strip tiles; `None` while it is not
    /// resolved.
    pub(super) fn card_line(&self, seq: u64) -> Option<Element<'_, Message>> {
        let (tile, name): (_, Element<'_, Message>) = match self.cards.get(&seq)? {
            Card::File(file) => return Some(file_content(file).short()),
            Card::Suggestion { suggestion, .. } => (
                suggestion_tile(suggestion, TileSize::Small),
                widgets::ellipsized(typo::body(), suggestion_title(suggestion), theme::text),
            ),
            Card::Unavailable | Card::Unknown => return Some(gone_line()),
            content => return self.content_line(content),
        };
        Some(row![tile, container(name).width(Fill)].spacing(space::s()).align_y(Alignment::Center).into())
    }

    /// Whether the entry right after `seq` is a card the core resolved to a file: the tool line
    /// above it then leaves the file's name to the card.
    pub(crate) fn file_card_after(&self, seq: u64) -> bool {
        self.transcript.card_after(seq).is_some_and(|card| matches!(self.cards.get(&card), Some(Card::File(_))))
    }
}

/// The tile of a file that is gone: the same room, but only the paperclip in `text_3`, without
/// the tile's surface – quiet, so it does not look like a file that can be opened.
fn gone_tile<'a>(size: TileSize) -> Element<'a, Message> {
    let edge = size.edge();
    let mark = if size == TileSize::Small { icon::SIZE_SM } else { icon::SIZE };
    container(tinted(FILE_ICON, mark, |p| p.text_3)).center(edge).into()
}

/// The buttons of a file card, each a person's click: "Open" (`tools.open_file`), "Save as…" (the
/// system's save dialog with the file's name, then `tools.save_file_as`) and "Send by mail" (a new
/// mail with the file attached, only while the Notification Hub is installed: issue #408,
/// [`chat::can_mail_files`]). With their words where they fit the card, else icons with the word
/// as tooltip; "Open", the main one, keeps its word first (`content_card`).
fn file_actions(file: &FileInfo) -> Vec<Action<Message>> {
    let (id, name) = (file.id.clone(), file.name.clone());
    let mut actions = vec![
        Action::new(Icon::ArrowUpRight, tr("Open"), Message::OpenFile(id.clone())),
        Action::new(Icon::Folder, tr("Save as…"), Message::SaveFileAs { id: id.clone(), name }),
    ];
    if chat::can_mail_files(torro_gui::installed::is_installed) {
        actions.push(Action::new(Icon::Send, tr("Send by mail"), Message::MailFile(id)));
    }
    actions
}

/// A file of the register as the core keeps it (on `content_card`, issue #784): the paperclip with its template
/// and size in the head, the name, the path on one line (cut with "…"), and the buttons.
fn file_content(file: &FileInfo) -> ContentCard<Message> {
    let card = ContentCard::new(Head::new(FILE_ICON, chat::file_meta(file)), file.name.as_str())
        .line(Icon::Folder, file.path.as_str());
    with_actions(card, file_actions(file))
}

/// The tile of a suggestion: the icon of its kind (an event's calendar, a task's list), neutral; the
/// sparkles for a kind the window does not know.
fn suggestion_tile<'a>(suggestion: &Suggestion, size: TileSize) -> Element<'a, Message> {
    let face = match suggestion.kind.as_str() {
        "event" => Icon::CalendarPlus,
        "task" => Icon::ListTodo,
        _ => Icon::Sparkles,
    };
    widgets::source_tile(TileFace::Icon(face), size, None)
}

/// The word for the kind of a suggestion ("Event", "Task"); another kind's id as it came.
fn suggestion_kind(suggestion: &Suggestion) -> String {
    match suggestion.kind.as_str() {
        "event" => tr("Event").to_owned(),
        "task" => tr("Task").to_owned(),
        other => other.to_owned(),
    }
}

/// What a suggestion is about: the title read out of the mail, else the word for its kind. The
/// mail's text: shown, never followed.
fn suggestion_title(suggestion: &Suggestion) -> String {
    let read = match (&suggestion.task, &suggestion.event) {
        (Some(task), _) => task.title.as_str(),
        (None, Some(event)) => event.title.as_str(),
        (None, None) => "",
    };
    match read.split_whitespace().collect::<Vec<_>>().join(" ") {
        title if title.is_empty() => suggestion_kind(suggestion),
        title => title,
    }
}

/// A suggestion of a mail in the short form (issue #605): its tile, what it is about, and its kind
/// with where to accept it – the mail window, whose chat about the mail shows the whole card –, and
/// "Open in Mail", which opens the mail window on the mail `id` of `account` (`apps.open_app`).
fn suggestion_card<'a>(account: &str, id: &str, suggestion: &Suggestion) -> Element<'a, Message> {
    let meta = trf("{kind} · Accept it in Mail", &[("kind", &suggestion_kind(suggestion))]);
    let head = widgets::card_head(
        Some(suggestion_tile(suggestion, TileSize::Large)),
        suggestion_title(suggestion),
        meta,
        None,
    );
    let open = Message::OpenMail { account: account.to_owned(), id: id.to_owned() };
    let button =
        widgets::text_button(Some(Icon::ArrowUpRight), tr("Open in Mail"), style::button::secondary, Some(open));
    widgets::card(vec![head, button], false, None)
}

/// A card whose item is gone or out of reach, in one line: the quiet mark and "No longer available".
fn gone_line<'a>() -> Element<'a, Message> {
    let words = widgets::ellipsized(typo::body(), tr("No longer available"), theme::text_3);
    row![gone_tile(TileSize::Small), container(words).width(Fill)].spacing(space::s()).align_y(Alignment::Center).into()
}

/// A card whose item is gone or out of reach: said so, without buttons; why is not told (the core
/// does not say it either). As wide as the content cards beside it, at most `room`.
fn gone_card<'a>(room: f32) -> Element<'a, Message> {
    let tile = gone_tile(TileSize::Large);
    let head =
        widgets::card_head(Some(tile), tr("No longer available"), tr("Moved, changed, deleted or out of reach."), None);
    container(widgets::card(vec![head], false, None)).width(room.min(size::list())).into()
}
