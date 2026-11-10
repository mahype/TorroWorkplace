//! The cards in the chat (task WZ.8, `docs/assistent.md` "Datei-Karte", prototype
//! `docs/prototypes/assistant/cards.html`): what the core resolved for a card entry
//! (`assistant.get_cards`), drawn as a `widgets::card` – never anything a model wrote.
//!
//! - **File card:** a tile, the file's name, its template and size, the path on one line (cut with
//!   "…", whole in the tooltip), and three buttons – "Open", "Save as…", "Send by mail" – with their
//!   words where the card has room, else as icons with the word as tooltip ("Open" keeps its word
//!   first, `widgets::bar_words`).
//! - **No longer available:** the file went, changed or is out of reach; no buttons, and the tile
//!   only its quiet mark in `text_3` without its surface, so it never reads as a file to open.
//! - **Suggestion of a mail** (issue #605, a chat about a mail): only its short form – the kind's
//!   tile, what was read out (or the kind's word) and "Accept it in Mail"; no buttons, as accepting
//!   opens the filled dialog in the mail window, where the chat about the mail has the whole card.
//! - **Short form:** one line for mini and strip tiles, the tile small, the name and the size.

use iced::widget::text::Wrapping;
use iced::widget::{Row, container, responsive, row};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo};
use torro_ipc::actions::Suggestion;
use torro_ipc::assistant::Card;
use torro_ipc::tools::FileInfo;

use super::tinted;
use crate::app::{App, Message};
use crate::chat;

/// The icon of a file of the assistant's tools: a paperclip, as an attachment in the mail window
/// (`widgets::file_chip`) – what the file becomes when it goes by mail.
pub(super) const FILE_ICON: Icon = Icon::Paperclip;

impl App {
    /// The card of the entry `seq` as the core resolved it last; `None` while it is not resolved,
    /// so it takes no room (and no gap) until it can show something.
    pub(super) fn card(&self, seq: u64) -> Option<Element<'_, Message>> {
        Some(match self.cards.get(&seq)? {
            Card::File(file) => file_card(file),
            Card::Suggestion { suggestion, .. } => suggestion_card(suggestion),
            Card::Unavailable => gone_card(),
        })
    }

    /// The card of the entry `seq` as one line, for mini and strip tiles; `None` while it is not
    /// resolved.
    pub(super) fn card_line(&self, seq: u64) -> Option<Element<'_, Message>> {
        let (tile, name, size): (_, Element<'_, Message>, _) = match self.cards.get(&seq)? {
            Card::File(file) => (
                file_tile(TileSize::Small),
                widgets::ellipsized(typo::body(), file.name.as_str(), theme::text),
                Some(chat::size_text(file.size)),
            ),
            Card::Suggestion { suggestion, .. } => (
                suggestion_tile(suggestion, TileSize::Small),
                widgets::ellipsized(typo::body(), suggestion_title(suggestion), theme::text),
                None,
            ),
            Card::Unavailable => (
                gone_tile(TileSize::Small),
                widgets::ellipsized(typo::body(), tr("No longer available"), theme::text_3),
                None,
            ),
        };
        let mut line = row![tile, container(name).width(Fill)].spacing(space::s()).align_y(Alignment::Center);
        if let Some(size) = size {
            line = line.push(typo::text(typo::meta().tabular(), size).style(theme::text_3).wrapping(Wrapping::None));
        }
        Some(line.into())
    }

    /// Whether the entry right after `seq` is a card the core resolved to a file: the tool line
    /// above it then leaves the file's name to the card.
    pub(crate) fn file_card_after(&self, seq: u64) -> bool {
        self.transcript.card_after(seq).is_some_and(|card| matches!(self.cards.get(&card), Some(Card::File(_))))
    }
}

/// The tile of a file: neutral, the paperclip in `text`.
fn file_tile<'a>(size: TileSize) -> Element<'a, Message> {
    widgets::source_tile(TileFace::Icon(FILE_ICON), size, None)
}

/// The tile of a file that is gone: the same room, but only the paperclip in `text_3`, without
/// the tile's surface – quiet, so it does not look like a file that can be opened.
fn gone_tile<'a>(size: TileSize) -> Element<'a, Message> {
    let edge = size.edge();
    let mark = if size == TileSize::Small { icon::SIZE_SM } else { icon::SIZE };
    container(tinted(FILE_ICON, mark, |p| p.text_3)).center(edge).into()
}

/// A file of the register as the core keeps it: the tile, the name with template and size, the
/// path, and the buttons.
fn file_card(file: &FileInfo) -> Element<'_, Message> {
    let head = widgets::card_head(Some(file_tile(TileSize::Large)), file.name.as_str(), chat::file_meta(file), None);
    let path = row![
        tinted(Icon::Folder, icon::SIZE_SM, |p| p.text_3),
        container(widgets::ellipsized(typo::meta(), file.path.as_str(), theme::text_2)).width(Fill)
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::card(vec![head, path.into(), file_buttons(file)], false, None)
}

/// The buttons of a file card, each a person's click: "Open" (`tools.open_file`), "Save as…" (the
/// system's save dialog with the file's name, then `tools.save_file_as`) and "Send by mail" (a new
/// mail with the file attached, only while the Notification Hub is installed: issue #408,
/// [`chat::can_mail_files`]). With their words where they fit the card, else icons with the word
/// as tooltip; "Open", the lead, keeps its word first.
fn file_buttons(file: &FileInfo) -> Element<'_, Message> {
    let (id, name) = (file.id.clone(), file.name.clone());
    let mail = chat::can_mail_files(torro_gui::installed::is_installed);
    let line = responsive(move |bounds| {
        let mut buttons = vec![
            (Icon::ArrowUpRight, tr("Open"), Message::OpenFile(id.clone())),
            (Icon::Folder, tr("Save as…"), Message::SaveFileAs { id: id.clone(), name: name.clone() }),
        ];
        if mail {
            buttons.push((Icon::Send, tr("Send by mail"), Message::MailFile(id.clone())));
        }
        let labels: Vec<&str> = buttons.iter().map(|(_, label, _)| *label).collect();
        let words = widgets::bar_words(&labels, true, 0, bounds.width);
        let buttons = buttons.into_iter().enumerate().map(|(i, (mark, label, message))| match words.worded(i) {
            true => widgets::text_button(Some(mark), label, style::button::secondary, Some(message)),
            false => widgets::icon_button(mark, label, Some(message)),
        });
        Row::with_children(buttons).spacing(space::s()).align_y(Alignment::Center).into()
    });
    container(line).width(Fill).height(size::control_sm()).into()
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
/// with where to accept it – the mail window, whose chat about the mail shows the whole card.
fn suggestion_card<'a>(suggestion: &Suggestion) -> Element<'a, Message> {
    let meta = trf("{kind} · Accept it in Mail", &[("kind", &suggestion_kind(suggestion))]);
    let head = widgets::card_head(
        Some(suggestion_tile(suggestion, TileSize::Large)),
        suggestion_title(suggestion),
        meta,
        None,
    );
    widgets::card(vec![head], false, None)
}

/// A card whose item is gone or out of reach: said so, without buttons; why is not told (the core
/// does not say it either).
fn gone_card<'a>() -> Element<'a, Message> {
    let tile = gone_tile(TileSize::Large);
    let head =
        widgets::card_head(Some(tile), tr("No longer available"), tr("Moved, changed, deleted or out of reach."), None);
    widgets::card(vec![head], false, None)
}
