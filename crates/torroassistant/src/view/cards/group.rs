//! A group of items in the assistant's chat (issue #786, "Grenzen" in `docs/inhaltskarten.md`): one to three as full
//! cards, four to ten as short forms, and with `total` the line "{n} … match" with "Show all" (`apps.open_app` of the
//! area's app, without a target).
//!
//! Strand 2g of issue #784 adds the line of the total and "Show all"; the limits are already drawn.

use iced::Element;
use iced::widget::column;
use torro_gui::theme::size;
use torro_ipc::api::Area;
use torro_ipc::assistant::Card;

use crate::app::{App, Message};

/// At most this many items of a group are full cards; more are short forms.
pub(super) const FULL_UP_TO: usize = 3;

impl App {
    /// The group of `items` of `area` (`total` found in all, when more than shown), at most `room` wide.
    pub(super) fn group_card<'a>(
        &'a self,
        seq: u64,
        items: &'a [Card],
        area: Area,
        total: Option<u64>,
        room: f32,
    ) -> Element<'a, Message> {
        let _ = (area, total);
        let full = items.len() <= FULL_UP_TO;
        let parts = items.iter().filter_map(|item| match full {
            true => self.content_view(seq, item, room),
            false => self.content_line(item),
        });
        column(parts).spacing(size::card_gap()).into()
    }
}
