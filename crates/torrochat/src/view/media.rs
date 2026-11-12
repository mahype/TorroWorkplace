//! The shared media at the side of a chat window (stage 4 of `docs/chat.md`, "Medien-Übersicht";
//! draft "Seitenleiste im Chatfenster" in `docs/prototypes/chat/`): the head "Shared" with ✕, the
//! tabs Media, Files, Voice and Links, and under them
//!
//! - **Media:** a grid of three columns by month, the newest first; a picture opens large, a video
//!   in the system's player, the right button offers "Show in chat".
//! - **Files, Voice, Links:** a line each with kind, name, size, author and day, "Open" (or "Play",
//!   "Open link") and "Show in chat".

use chrono::{Datelike, Utc};
use iced::widget::{Space, column, container, row, rule, scrollable, stack};
use iced::{Alignment, Element, Fill, Length};
use torro_chat::{ChatError, SharedItem};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, MenuItem, TileFace, TileSize};
use torro_gui::{datetime, style, typo, units};

use super::HAIRLINE;
use super::files::{clickable, kind_icon, play_mark};
use crate::app::files::{FilesMsg, MediaPanel, MediaTab, PictureKey, Shown, wrap};
use crate::app::{App, ChatState, Message};

/// Pictures in a row of the grid.
const GRID_COLUMNS: usize = 3;

impl App {
    /// The shared media, `width` wide.
    pub(super) fn media_panel<'a>(
        &'a self,
        chat: &'a ChatState,
        panel: &'a MediaPanel,
        width: f32,
    ) -> Element<'a, Message> {
        let close = widgets::icon_button(Icon::X, tr("Close shared media (Esc)"), Some(wrap(FilesMsg::CloseMedia)));
        let head = row![typo::text(typo::body_strong(), tr("Shared")), Space::new().width(Fill), close]
            .align_y(Alignment::Center);
        let head =
            container(head).width(Fill).height(size::header()).padding([0.0, space::l()]).center_y(size::header());
        let labels = MediaTab::ALL.iter().map(|tab| (tab.label().to_owned(), None)).collect();
        let selected = MediaTab::ALL.iter().position(|tab| *tab == panel.tab).unwrap_or(0);
        let tabs = widgets::tabs(labels, selected, |at| wrap(FilesMsg::Tab(MediaTab::ALL[at])));
        let tabs = container(tabs).padding([0.0, space::l()]);
        let content =
            column![head, rule::horizontal(HAIRLINE).style(style::rule), tabs, self.media_body(chat, panel, width)];
        container(content).width(Fill).height(Fill).style(style::container::panel_flush).into()
    }

    /// The list of the tab, or why there is none.
    fn media_body<'a>(&'a self, chat: &'a ChatState, panel: &'a MediaPanel, width: f32) -> Element<'a, Message> {
        if panel.items.is_empty() {
            let state: Element<'a, Message> = match &panel.error {
                _ if panel.loading => widgets::loading(),
                Some(ChatError::Unsupported(_)) => widgets::empty_state(
                    tr("Shared items cannot be listed"),
                    tr("This server does not list what a conversation shared."),
                ),
                Some(err) => widgets::error_state(crate::app::chat_failed(err), wrap(FilesMsg::Tab(panel.tab))),
                None => {
                    let title = match panel.tab {
                        MediaTab::Media => tr("No pictures or videos yet"),
                        MediaTab::Files => tr("No files yet"),
                        MediaTab::Voice => tr("No voice messages yet"),
                        MediaTab::Links => tr("No links yet"),
                    };
                    widgets::empty_state(title, tr("What this conversation shares shows here."))
                }
            };
            return container(state).width(Fill).height(Fill).center(Fill).padding(space::l()).into();
        }
        let mut lines: Vec<Element<'a, Message>> = match panel.tab {
            MediaTab::Media => self.media_grid(chat, &panel.items, width),
            MediaTab::Files | MediaTab::Voice | MediaTab::Links => {
                panel.items.iter().enumerate().map(|(index, item)| shared_line(panel.tab, index, item)).collect()
            }
        };
        if panel.more {
            let label = if panel.loading { tr("Loading…") } else { tr("Show older") };
            let older = widgets::text_button(
                Some(Icon::ChevronDown),
                label,
                style::button::ghost,
                (!panel.loading).then_some(wrap(FilesMsg::MoreShared)),
            );
            lines.push(container(older).width(Fill).center_x(Fill).into());
        }
        scrollable(column(lines).spacing(space::s()).padding([space::m(), space::l()]).width(Fill))
            .height(Fill)
            .width(Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .into()
    }

    /// Pictures and videos in rows of [`GRID_COLUMNS`], under the month they came in.
    fn media_grid<'a>(&'a self, chat: &'a ChatState, items: &'a [SharedItem], width: f32) -> Vec<Element<'a, Message>> {
        let inner = width - 2.0 * space::l();
        let gap = space::xs();
        let edge = ((inner - gap * (GRID_COLUMNS - 1) as f32) / GRID_COLUMNS as f32).floor().max(size::tile());
        let this_year = Utc::now().year();
        let mut lines = Vec::new();
        let mut month: Option<(i32, u32)> = None;
        let mut tiles: Vec<Element<'a, Message>> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let local = datetime::to_local(item.at);
            let this = (local.year(), local.month());
            if month != Some(this) {
                if !tiles.is_empty() {
                    lines.push(grid_rows(std::mem::take(&mut tiles), gap));
                }
                let name = datetime::month_name(this.1);
                let title = if this.0 == this_year {
                    name.to_owned()
                } else {
                    trf("{month} {year}", &[("month", &name), ("year", &this.0)])
                };
                lines.push(typo::text(typo::caption(), title).style(theme::text_3).into());
                month = Some(this);
            }
            tiles.push(media_tile(chat, index, item, edge));
        }
        if !tiles.is_empty() {
            lines.push(grid_rows(tiles, gap));
        }
        lines
    }

    /// The entries of a shared item's menu: open it, "Show in chat", save a file to Downloads.
    pub(super) fn shared_items<'a>(&'a self, item: &'a SharedItem) -> Vec<MenuItem<'a, Message>> {
        let mut items = vec![MenuItem::new(Icon::ExternalLink, tr("Open"), open_message(item))];
        items.push(MenuItem::new(
            Icon::MessageCircle,
            tr("Show in chat"),
            wrap(FilesMsg::ShowInChat(item.message.clone())),
        ));
        if let Some(file) = &item.file {
            items.push(MenuItem::new(
                Icon::Download,
                tr("Save to Downloads"),
                wrap(FilesMsg::Save(item.message.clone(), file.clone())),
            ));
        }
        items
    }
}

/// What opening a shared item does: a picture large, a link in the browser, any other file with the
/// system's program.
fn open_message(item: &SharedItem) -> Message {
    match &item.file {
        Some(file) if item.content_type.starts_with("image/") && item.preview => {
            wrap(FilesMsg::View(item.message.clone(), file.clone()))
        }
        Some(file) => wrap(FilesMsg::Open(item.message.clone(), file.clone())),
        None => wrap(FilesMsg::OpenLink(item.name.clone())),
    }
}

/// Tiles in rows of [`GRID_COLUMNS`].
fn grid_rows(tiles: Vec<Element<'_, Message>>, gap: f32) -> Element<'_, Message> {
    let mut rows = Vec::new();
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let line: Vec<Element<'_, Message>> = tiles.by_ref().take(GRID_COLUMNS).collect();
        rows.push(row(line).spacing(gap).into());
    }
    column(rows).spacing(gap).into()
}

/// A picture or video of the grid, `edge` square: its preview (a calm placeholder until it came), a
/// video with its play mark; a click opens it, the right button its menu.
fn media_tile<'a>(chat: &'a ChatState, index: usize, item: &'a SharedItem, edge: f32) -> Element<'a, Message> {
    let key: PictureKey = (item.message.clone(), item.file.clone());
    let video = item.content_type.starts_with("video/");
    let which = if video { Icon::Video } else { Icon::Image };
    let picture: Element<'a, Message> = match chat.files.picture(&key) {
        Some(Shown::Ready(handle)) => iced::widget::image(handle.clone())
            .width(edge)
            .height(edge)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(theme::radius::sm())
            .into(),
        _ => container(widgets::source_tile(TileFace::Icon(which), TileSize::Medium, None))
            .width(edge)
            .height(edge)
            .center(Fill)
            .style(style::container::chip)
            .into(),
    };
    let tile: Element<'a, Message> = if video {
        stack![picture, container(play_mark()).center(Fill)].width(edge).height(edge).into()
    } else {
        picture
    };
    let tile = clickable(tile, open_message(item), item.name.clone());
    widgets::context_area(tile, move |at| wrap(FilesMsg::ItemMenu(index, at)))
}

/// A file, voice message or link of the lists: kind, name over size, author and day, then "Open"
/// and "Show in chat".
fn shared_line(tab: MediaTab, index: usize, item: &SharedItem) -> Element<'_, Message> {
    let which = match tab {
        MediaTab::Links => Icon::Link,
        MediaTab::Voice => Icon::Play,
        MediaTab::Media | MediaTab::Files => kind_icon(&item.content_type),
    };
    let when = datetime::format_moment(item.at, Utc::now());
    let mut meta = Vec::new();
    if item.size > 0 {
        meta.push(units::format_bytes(item.size));
    }
    meta.push(item.author.clone());
    meta.push(when);
    let name = match tab {
        MediaTab::Links => item.name.trim_start_matches("https://").trim_start_matches("http://").to_owned(),
        _ => item.name.clone(),
    };
    let words = column![
        widgets::ellipsized(typo::body(), name, theme::text),
        widgets::ellipsized(typo::meta(), meta.join(" · "), theme::text_3),
    ];
    let (open_icon, open_tip) = match tab {
        MediaTab::Links => (Icon::ExternalLink, tr("Open link")),
        MediaTab::Voice => (Icon::Play, tr("Play in your audio player")),
        MediaTab::Media | MediaTab::Files => (Icon::ExternalLink, tr("Open")),
    };
    let line = row![
        widgets::source_tile(TileFace::Icon(which), TileSize::Medium, None),
        container(words).width(Length::Fill).clip(true),
        widgets::icon_button(open_icon, open_tip, Some(open_message(item))),
        widgets::icon_button(
            Icon::MessageCircle,
            tr("Show in chat"),
            Some(wrap(FilesMsg::ShowInChat(item.message.clone())))
        ),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    widgets::context_area(line, move |at| wrap(FilesMsg::ItemMenu(index, at)))
}
