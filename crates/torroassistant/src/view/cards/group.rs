//! A group of items in the assistant's chat (issue #786, "Grenzen" in `docs/inhaltskarten.md`): one to three as full
//! cards, four to ten as short forms (each with its "Open"), and with `total` the line "{n} … match" with "Show all"
//! (`apps.open_app` of the area's app, without a target: the app as it starts, not a search the window made up).

use iced::widget::{Column, column, container, row};
use iced::{Alignment, Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::content_card::Action;
use torro_gui::icon::Icon;
use torro_gui::theme::{self, size, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::Request;
use torro_ipc::api::Area;
use torro_ipc::assistant::Card;

use super::card_request;
use crate::app::{App, Message};

/// At most this many items of a group are full cards; more are short forms.
pub(super) const FULL_UP_TO: usize = 3;

/// The app "Show all" opens for `area`; `None` for an area without one (no button then).
fn app_of(area: Area) -> Option<&'static str> {
    match area {
        Area::Mail => Some(AppId::INBOX_MAIL.window_class),
        Area::Calendar => Some(AppId::CALENDAR.binary),
        Area::Tasks => Some(AppId::TASKS.binary),
        Area::Contacts => Some(AppId::CONTACTS.binary),
        _ => None,
    }
}

/// The line of the total: "{count} mails match" and so on by `area`.
fn total_words(area: Area, total: u64) -> String {
    let count = [("count", &total as &dyn std::fmt::Display)];
    match area {
        Area::Mail => trf("{count} mails match", &count),
        Area::Calendar => trf("{count} events match", &count),
        Area::Tasks => trf("{count} tasks match", &count),
        Area::Contacts => trf("{count} contacts match", &count),
        _ => trf("{count} items match", &count),
    }
}

/// "Show all" of a group of `area` with `total`: the area's app without a target; `None` without a total (the
/// group shows every item already) or without an app for the area.
pub(super) fn show_all(area: Area, total: Option<u64>) -> Option<Action<Message>> {
    total?;
    let request = Request::OpenApp { app: app_of(area)?.to_owned(), target: None };
    Some(Action::new(Icon::ArrowUpRight, tr("Show all"), card_request(request, tr("Couldn't open the app."))))
}

impl App {
    /// How a group of `count` items of `area` shows, `total` found in all: whether as full cards (one to three, else
    /// short forms) and the line of the total ("12 mails match"), only when more were found than it holds.
    pub(crate) fn group_look(area: Area, count: usize, total: Option<u64>) -> (bool, Option<String>) {
        let line = total.filter(|total| *total > count as u64).map(|total| total_words(area, total));
        (count <= FULL_UP_TO, line)
    }

    /// The group of `items` of `area` (`total` found in all, when more than shown), at most `room` wide: the full
    /// cards one under the other, or the short forms together on one card; below them the line of the total with
    /// "Show all".
    pub(super) fn group_card<'a>(
        &'a self,
        seq: u64,
        items: &'a [Card],
        area: Area,
        total: Option<u64>,
        room: f32,
    ) -> Element<'a, Message> {
        let (full, line) = Self::group_look(area, items.len(), total);
        let width = room;
        let mut parts: Column<'a, Message> = if full {
            column(items.iter().filter_map(|item| self.content_view(seq, item, room))).spacing(size::card_gap())
        } else {
            let lines = items.iter().filter_map(|item| self.content_line(item)).collect();
            column![container(widgets::card(lines, false, None)).width(width)]
        };
        if let Some(line) = line {
            let words = container(widgets::ellipsized(typo::meta(), line, theme::text_2)).width(Fill);
            let mut total_row = row![words].spacing(space::s()).align_y(Alignment::Center);
            if let Some(all) = show_all(area, total) {
                total_row =
                    total_row.push(widgets::text_button(Some(all.icon), all.label, style::button::ghost, all.on_press));
            }
            parts = parts.push(container(total_row).width(width));
        }
        parts.spacing(size::card_gap()).into()
    }
}
