//! "What's new" under the mail window's overview (issue #485): the suite's announcements – a new
//! app, a new function, a new version – as the core lists them (`apps.list_announcements`), each with
//! ✕ to hide it for good (`apps.dismiss_announcement`).
//!
//! The announcements come with the suite, never from the network; the core leaves out the hidden ones
//! and those older than 90 days. The block shows at most [`SHOWN`] and is left out entirely while they
//! load, after an error and without any, so the overview never waits for news. Called "What's new",
//! not "News": that would read like the newsletters of the categories.

use std::future::Future;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill, Padding, Theme};
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::{datetime, typo, widgets};
use torro_inbox::demo::DemoSource;
use torro_ipc::inbox::InboxClient;
use torro_ipc::{Announcement, AnnouncementKind, Request, Response};

use crate::app::Message;

/// How many announcements the block shows at most.
pub const SHOWN: usize = 3;

/// Where the announcements come from: the core, the demo's examples, or none (tests).
pub trait NewsSource: Send + Sync {
    /// `apps.list_announcements`: what is new, the newest first, without the hidden ones.
    fn announcements(&self) -> impl Future<Output = Result<Vec<Announcement>, String>> + Send;

    /// `apps.dismiss_announcement`: hides the announcement `id` for good.
    fn dismiss_announcement(&self, id: String) -> impl Future<Output = Result<(), String>> + Send;
}

impl NewsSource for InboxClient {
    async fn announcements(&self) -> Result<Vec<Announcement>, String> {
        match torro_ipc::client::call(&self.socket, &Request::ListAnnouncements).await {
            Ok(Response::Announcements { announcements }) => Ok(announcements),
            Ok(Response::Error { message, .. }) => Err(message),
            Ok(_) => Err("unexpected answer".to_owned()),
            Err(err) => Err(err.to_string()),
        }
    }

    async fn dismiss_announcement(&self, id: String) -> Result<(), String> {
        match torro_ipc::client::call(&self.socket, &Request::DismissAnnouncement { id }).await {
            Ok(Response::Ok) => Ok(()),
            Ok(Response::Error { message, .. }) => Err(message),
            Ok(_) => Err("unexpected answer".to_owned()),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// Two examples, so the demo shows the block; hiding one keeps it hidden while the window runs (the
/// window drops it and asks for the list only once).
impl NewsSource for DemoSource {
    async fn announcements(&self) -> Result<Vec<Announcement>, String> {
        Ok(demo())
    }

    async fn dismiss_announcement(&self, id: String) -> Result<(), String> {
        match demo().iter().any(|announcement| announcement.id == id) {
            true => Ok(()),
            false => Err(format!("no announcement {id:?}")),
        }
    }
}

#[cfg(test)]
impl NewsSource for crate::empty::EmptySource {
    async fn announcements(&self) -> Result<Vec<Announcement>, String> {
        Ok(Vec::new())
    }

    async fn dismiss_announcement(&self, id: String) -> Result<(), String> {
        Err(format!("no announcement {id:?}"))
    }
}

/// The demo's announcements: the new statistics and a new app.
pub fn demo() -> Vec<Announcement> {
    let day = |month, day| chrono::NaiveDate::from_ymd_opt(2026, month, day).unwrap_or_default();
    vec![
        Announcement {
            id: "2026-11-mail-statistics".into(),
            date: day(11, 6),
            kind: AnnouncementKind::Feature,
            title: tr("New: statistics in the mail settings").into(),
            text: tr("Who sends the most, what lies in your inbox and how each category develops moved to Settings › \
                      Statistics. The overview now shows only what comes in.")
            .into(),
        },
        Announcement {
            id: "demo-new-app".into(),
            date: day(10, 20),
            kind: AnnouncementKind::App,
            title: tr("New app: Torro Contacts").into(),
            text: tr("Your address books from Nextcloud, with birthdays and the people you write to most.").into(),
        },
    ]
}

/// The window's announcements: `None` until the list came (or while it is on its way), then the list
/// or why it could not be read.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub items: Option<Result<Vec<Announcement>, String>>,
    /// The list was asked for; it is asked once per window.
    pub asked: bool,
}

impl State {
    /// Takes the announcement `id` out of the list (✕), and gives it back for the case that hiding
    /// it fails.
    pub fn take(&mut self, id: &str) -> Option<Announcement> {
        let Some(Ok(items)) = self.items.as_mut() else { return None };
        let at = items.iter().position(|announcement| announcement.id == id)?;
        Some(items.remove(at))
    }

    /// Puts `announcement` back where it stood by date, after hiding it failed.
    pub fn put_back(&mut self, announcement: Announcement) {
        let Some(Ok(items)) = self.items.as_mut() else { return };
        if items.iter().any(|held| held.id == announcement.id) {
            return;
        }
        let at = items.iter().position(|held| held.date < announcement.date).unwrap_or(items.len());
        items.insert(at, announcement);
    }

    /// What the block shows: at most [`SHOWN`], nothing while loading, after an error or without any.
    pub fn shown(&self) -> &[Announcement] {
        match &self.items {
            Some(Ok(items)) => &items[..items.len().min(SHOWN)],
            _ => &[],
        }
    }
}

/// "What's new": a card with a line per announcement ([`State::shown`]); `None` when there is none to
/// show.
pub fn block(state: &State) -> Option<Element<'_, Message>> {
    let shown = state.shown();
    if shown.is_empty() {
        return None;
    }
    let lines = column(shown.iter().map(line)).spacing(space::m());
    Some(widgets::chart_card(tr("What's new"), "", lines))
}

/// One announcement: its symbol, the title strong with the text under it (wrapping), the day and ✕.
fn line(announcement: &Announcement) -> Element<'_, Message> {
    let symbol = match announcement.kind {
        AnnouncementKind::App => Icon::AppWindow,
        AnnouncementKind::Feature | AnnouncementKind::Update => Icon::Megaphone,
    };
    let mark = icon::icon(symbol, icon::SIZE_SM)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let words = column![
        typo::text(typo::body_strong(), announcement.title.as_str()).wrapping(Wrapping::WordOrGlyph),
        typo::text(typo::body(), announcement.text.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::xs());
    let date = typo::text(typo::meta().tabular(), datetime::format_day_month(announcement.date))
        .style(theme::text_3)
        .wrapping(Wrapping::None);
    let dismiss = widgets::icon_button(Icon::X, tr("Dismiss"), Some(Message::DismissNews(announcement.id.clone())));
    // Symbol, date and ✕ centred on the height of the button, the title's first line with them.
    let edge = size::control_sm();
    let lead = (edge - typo::line_height(typo::body_strong())).max(0.0) / 2.0;
    row![
        container(mark).center_y(edge),
        container(words).width(Fill).padding(Padding::ZERO.top(lead)),
        container(date).center_y(edge),
        dismiss,
    ]
    .spacing(space::m())
    .align_y(Alignment::Start)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(items: Vec<Announcement>) -> State {
        State { items: Some(Ok(items)), asked: true }
    }

    #[test]
    fn the_block_shows_at_most_three_and_nothing_while_loading_failed_or_empty() {
        let mut many = demo();
        for n in 0..3 {
            many.push(Announcement { id: format!("extra-{n}"), ..demo().remove(1) });
        }
        assert_eq!(state(many).shown().len(), SHOWN);
        assert!(block(&State::default()).is_none(), "loading");
        assert!(block(&State { items: Some(Err("down".into())), asked: true }).is_none(), "an error");
        assert!(block(&state(Vec::new())).is_none(), "none");
        assert!(block(&state(demo())).is_some());
    }

    #[test]
    fn a_dismissed_announcement_goes_and_comes_back_where_it_stood() {
        let mut news = state(demo());
        let first = demo().remove(0);
        let taken = news.take(&first.id).expect("taken");
        assert_eq!(news.shown().len(), 1);
        assert!(news.take("unknown").is_none());
        news.put_back(taken);
        assert_eq!(news.shown().first().map(|held| held.id.as_str()), Some(first.id.as_str()), "the newest first");
        news.put_back(first);
        assert_eq!(news.shown().len(), 2, "never twice");
    }
}
