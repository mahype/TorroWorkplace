//! How a chat window looks (`docs/chat.md`, "Chatfenster"; draft "Chatfenster mit Anna" in
//! `docs/prototypes/chat/`): the head with the face, the name and the status (the number of a group's
//! kind), the history and the composer.
//!
//! - **History:** a column of reading width ([`size::content_max`]), at its end while new messages
//!   come. Days get a divider ("Today", "Yesterday", "Sat, 3 Oct"), the messages after the last read
//!   one the divider "New". Messages of others stand left on `raised`, the person's own right on
//!   `selected`; a run of one author shows the name once (in groups) and the time once. Messages of
//!   the provider are small and grey in the middle, a missed call with its icon. A file is a line with
//!   its name and size (pictures and the rest come with stage 4). Mentions read stronger.
//! - **Composer:** Enter sends, Shift+Enter starts a new line; the quote of a reply above it with ✕;
//!   the suggestions of an `@` float above it; a message that could not go says why above it.
//! - The head's "…" holds "Mark as unread", the favourite (a pin, as the icon set has no star) and
//!   the notification level; a favourite shows its pin in the head, "Never" a crossed-out bell.
//! - **A message's menu** (right click, or the "…" beside it under the pointer): the quick
//!   reactions in a row at the top, "Reply", "Copy text", for the person's own "Edit" and
//!   "Delete…" (which asks in the menu), "Mark as unread".
//! - **Reactions** stand under the bubble as chips with their count; the person's own is chosen
//!   (`selected` with the stronger edge), a click gives or takes it back.
//! - **Stage 7 states:** the message being edited fills the composer with "Edit message" above it,
//!   Enter saves; a deletion on its way says "Deleting…"; what the core refused or holds for
//!   approval stands in a calm line above the composer ([`crate::app::chat::Note`]).

use std::collections::BTreeMap;

use iced::widget::text::{Rich, Span, Wrapping};
use iced::widget::text_editor::{Binding, KeyPress};
use iced::widget::{
    Space, button, column, container, mouse_area, rich_text, row, rule, scrollable, stack, text_editor as editor,
};
use iced::{Alignment, Background, Element, Fill, Padding, Rectangle, Size, Theme, border, keyboard};
use torro_chat::{Conversation, Mention, MessageKind, NotifyLevel, Person};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, TopRow};
use torro_gui::split;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, MenuItem, MenuSection, Placement};
use torro_gui::{style, typo};

use super::{Anchors, HAIRLINE, avatar, kind_words, tile_face, tinted};
use crate::app::chat::{Change, EDITOR, HISTORY, Msg, level_word};
use crate::app::files::{self, FilesMsg};
use crate::app::{App, ChatState, Menu, Message};
use crate::people::{self, Group};
use crate::timeline::{self, Item};

/// How much of the history's width a message may take at most: the other side stays free, so who
/// wrote it reads from where it stands.
const BUBBLE_SHARE: f32 = 0.78;

/// The joint between the tiles of a grid: two hairlines in every spacing step, so it never reads
/// as a crack.
const ALBUM_GAP: f32 = 2.0 * HAIRLINE;

/// The stroke left of a quote: as wide as the bar that marks a chosen row (two hairlines).
const QUOTE_STROKE: f32 = 2.0 * HAIRLINE;

/// The composer: two lines high at first, at most six, then it scrolls.
fn editor_heights() -> (f32, f32) {
    let line = typo::line_height(typo::body());
    let pad = style::INPUT_PADDING.top + style::INPUT_PADDING.bottom;
    (2.0 * line + pad, 6.0 * line + pad)
}

/// How wide a message may stand in the history.
#[derive(Debug, Clone, Copy)]
struct Room {
    /// A bubble at most: [`BUBBLE_SHARE`] of the history, leaving "Reply" and "…" their place.
    bubble: f32,
    /// A picture filling its bubble at most: within the bubble, [`size::list()`] and the room left
    /// beside "Reply" and "…".
    picture: f32,
}

/// "Reply" and "…" beside a bubble under the pointer, with the gap between them.
fn tools_width() -> f32 {
    size::control_sm() + space::xs() + size::control_sm()
}

/// The edge of a bubble: the others' stand off the panel with a line, the person's own on
/// `selected` without one, unless "Show in chat" marks it.
fn bubble_edge(own: bool, marked: bool) -> f32 {
    if own && !marked { 0.0 } else { HAIRLINE }
}

/// A bubble around `content`, at most `max` wide: the others' on `raised` with a line, the person's
/// own on `selected`; a marked one with the stronger line.
fn bubble<'a>(
    content: Element<'a, Message>,
    own: bool,
    marked: bool,
    padding: Padding,
    max: f32,
) -> Element<'a, Message> {
    container(content)
        .padding(padding)
        .max_width(max)
        .style(move |t: &Theme| {
            let p = palette(t);
            let edge = if marked { p.line_selected } else { p.line };
            container::Style {
                background: Some(Background::Color(if own { p.selected } else { p.raised })),
                border: border::rounded(theme::radius::md()).color(edge).width(bubble_edge(own, marked)),
                text_color: Some(p.text),
                ..container::Style::default()
            }
        })
        .into()
}

/// The text and link card under a flush picture or grid `width` wide, with the bubble's padding.
fn caption_block<'a>(parts: Vec<Element<'a, Message>>, width: f32) -> Element<'a, Message> {
    container(column(parts).spacing(space::xs())).padding([space::s(), space::m()]).width(width).into()
}

/// The one picture or video of `message` that stands flush in its bubble, in its frame: no quote,
/// exactly one file showing its preview, and in its frame at least as wide as a sidebar – a smaller
/// picture keeps the bubble's padding around it.
fn flush_preview<'a>(
    chat: &ChatState,
    message: &'a torro_chat::Message,
    room: Room,
) -> Option<(&'a torro_chat::FileRef, f32, f32)> {
    let [file] = message.files.as_slice() else { return None };
    if message.reply_to.is_some() || message.kind == MessageKind::Deleted {
        return None;
    }
    let shown = chat.files.picture(&(message.id.clone(), Some(file.id.clone())));
    if !matches!(super::files::look(message, file, shown), super::files::Look::Picture | super::files::Look::Video) {
        return None;
    }
    let (width, height) = super::files::framed(file.width, file.height, room.picture, size::panel());
    (width >= size::sidebar()).then_some((file, width, height))
}

/// The reactions of `members` counted together, by emoji: how many gave it and the member the
/// person gave it to, if any.
fn reactions_of<'a>(members: &[&'a torro_chat::Message]) -> BTreeMap<&'a str, (u64, Option<&'a torro_chat::Message>)> {
    let mut all: BTreeMap<&'a str, (u64, Option<&'a torro_chat::Message>)> = BTreeMap::new();
    for &member in members {
        for (emoji, count) in &member.reactions {
            let entry = all.entry(emoji.as_str()).or_insert((0, None));
            entry.0 += count;
            if member.own_reactions.iter().any(|own| own == emoji) {
                entry.1 = Some(member);
            }
        }
    }
    all
}

impl App {
    /// A chat window: the flush frame of the narrow layout in every size. The top row names the
    /// conversation (as the draft has it, the window's title); under it the head of the person or
    /// group as a row of its own, [`size::header`] high, on the axis of the history.
    pub(crate) fn chat_window<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Element<'a, Message> {
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow).cover(self.cover());
        let title = widgets::ellipsized(typo::body_strong(), self.chat_name(chat), theme::text);
        // Off macOS the app's mark stands where the lights would, so the corner is never empty.
        let mut header = row![].spacing(space::s()).width(Fill).align_y(Alignment::Center);
        if !cfg!(target_os = "macos") {
            header = header.push(shell::app_mark(torro_core::AppId::CHAT));
        }
        let header = header.push(title);
        let head = container(self.chat_head(chat, anchors))
            .width(Fill)
            .max_width(size::content_max())
            .height(size::header())
            .padding([0.0, space::l()])
            .center_y(size::header());
        let head = column![container(head).width(Fill).center_x(Fill), rule::horizontal(HAIRLINE).style(style::rule)];
        let body = column![head, self.history(chat, anchors), self.composer(chat, anchors)].width(Fill).height(Fill);
        let body = container(body).width(Fill).height(Fill).center_x(Fill);
        // The shared media stand at the side where both fit, else they take the window (stage 4).
        let body: Element<'a, Message> = match &chat.files.media {
            Some(panel) if files::media_beside(self.size.width) => {
                let total = self.size.width;
                let width = chat.files.split.width(files::media_limits(total));
                let side = widgets::anchored(&anchors.shared, self.media_panel(chat, panel, width));
                split::split_end(body, side, width, total, chat.files.split.dragging, |event| {
                    files::wrap(FilesMsg::Split(event))
                })
            }
            Some(panel) => widgets::anchored(&anchors.shared, self.media_panel(chat, panel, self.size.width)),
            None => body.into(),
        };
        let body = stack![body, self.drop_layer(chat)].width(Fill).height(Fill);
        shell::shell(SizeClass::Narrow, Space::new(), header, body, None)
            .overlay(self.chat_layer(chat, anchors))
            .top_row(top)
            .into()
    }

    /// How wide the history stands: the window, less the shared media where they stand beside it.
    fn history_width(&self, chat: &ChatState) -> f32 {
        let total = self.size.width;
        match &chat.files.media {
            Some(_) if files::media_beside(total) => {
                total - chat.files.split.width(files::media_limits(total)) - split::divider_width()
            }
            _ => total,
        }
    }

    /// The conversation as the list has it.
    fn conversation(&self, chat: &ChatState) -> Option<&Conversation> {
        self.conversations.iter().find(|conversation| conversation.id == chat.id)
    }

    /// The other person of a direct chat, with their status.
    fn other(&self, chat: &ChatState) -> Option<&Person> {
        let person = self.conversation(chat)?.person.as_deref()?;
        self.people.iter().find(|known| known.id == person)
    }

    /// The head: face, name, and the status with its message (a group's kind), "Offline" while the
    /// server cannot be reached, and "…".
    fn chat_head<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Element<'a, Message> {
        let colours = self.colours();
        let name = self.chat_name(chat);
        let conversation = self.conversation(chat);
        let other = self.other(chat);
        let (face, line): (Element<'a, Message>, String) = match (other, conversation) {
            (Some(person), _) => (
                avatar(&person.name, &person.id, Some(Group::of(person).status()), &colours),
                match person.message.as_deref().filter(|message| !message.trim().is_empty()) {
                    Some(message) if !person.unknown => {
                        format!("{} · {message}", people::status_word(Group::of(person).status()))
                    }
                    _ => people::person_line(person),
                },
            ),
            (None, Some(conversation)) if conversation.kind != torro_chat::ConversationKind::Direct => {
                (tile_face(Icon::Users), kind_words(conversation).to_owned())
            }
            _ => (avatar(&name, &chat.id, None, &colours), String::new()),
        };
        let mut words = column![widgets::ellipsized(typo::body_strong(), name, theme::text)];
        if !line.is_empty() {
            words = words.push(widgets::ellipsized(typo::meta(), line, theme::text_3));
        }
        let mut head =
            row![face, container(words).width(Fill).clip(true)].spacing(space::s()).align_y(Alignment::Center);
        if conversation.is_some_and(|c| c.favorite) {
            head = head.push(widgets::with_tooltip(tinted(Icon::Pin, icon::SIZE_SM, |p| p.text_3), tr("Favorite")));
        }
        if conversation.is_some_and(|c| c.notify == NotifyLevel::Never) {
            head = head.push(widgets::with_tooltip(
                tinted(Icon::BellOff, icon::SIZE_SM, |p| p.text_3),
                tr("Notifications are off"),
            ));
        }
        if chat.offline {
            let mark = widgets::Mark::Tinted(Icon::CircleAlert, colours.attention);
            head = head.push(widgets::with_tooltip(
                widgets::state_pill(mark, tr("Offline")),
                tr("The server cannot be reached. This is what was kept."),
            ));
        }
        let open = chat.files.media.is_some();
        let tip = if open { tr("Hide shared media") } else { tr("Shared media") };
        let media =
            widgets::icon_toggle(Icon::LayoutGrid, tip, open, Some(Message::Chat(Msg::Files(FilesMsg::ToggleMedia))));
        let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::Chat(Msg::HeadMenu)));
        head.push(media).push(widgets::anchored(&anchors.head, more)).into()
    }

    /// The history, or why there is none.
    fn history<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Element<'a, Message> {
        if chat.gone {
            return container(widgets::empty_state(
                tr("This conversation is no longer available"),
                tr("It was deleted, or you are no longer part of it."),
            ))
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .into();
        }
        if let Some(error) = &chat.error {
            return container(widgets::error_state(error.as_str(), Message::Chat(Msg::Retry)))
                .width(Fill)
                .height(Fill)
                .center(Fill)
                .into();
        }
        if !chat.loaded {
            return container(widgets::loading()).width(Fill).height(Fill).into();
        }
        if chat.messages.is_empty() {
            return container(widgets::empty_state(tr("No messages yet"), tr("Write the first one below.")))
                .width(Fill)
                .height(Fill)
                .center(Fill)
                .into();
        }
        let width = self.history_width(chat).min(size::content_max());
        // A bubble leaves "Reply" and "…" their place beside it (the history's padding and the
        // buttons with their gaps); a picture fills it inside its edge.
        let beside = width - 2.0 * space::l() - tools_width() - 2.0 * space::xs();
        let bubble = (width * BUBBLE_SHARE).min(beside);
        let picture = (bubble - 2.0 * HAIRLINE).min(size::list()).floor();
        let room = Room { bubble, picture };
        let group = self.conversation(chat).is_some_and(|c| c.kind != torro_chat::ConversationKind::Direct);
        let today = timeline::day_of(chrono::Utc::now());
        let mut lines: Vec<Element<'a, Message>> = Vec::new();
        if chat.more {
            let label = if chat.loading_earlier { tr("Loading…") } else { tr("Show earlier") };
            let earlier = widgets::text_button(
                Some(Icon::ChevronUp),
                label,
                style::button::ghost,
                (!chat.loading_earlier).then_some(Message::Chat(Msg::Earlier)),
            );
            lines.push(container(earlier).width(Fill).center_x(Fill).into());
        }
        for item in timeline::items(&chat.messages, chat.new_after) {
            lines.push(match item {
                Item::Day(day) => {
                    centred(typo::text(typo::caption(), timeline::day_label(day, today)).style(theme::text_3))
                }
                Item::New => new_divider(),
                Item::System(message) => system_line(message),
                Item::Message { message, first, last } => {
                    self.message_line(chat, message, first, last, group, room, anchors)
                }
                Item::Album { tiles, first, last } => self.album_line(chat, &tiles, first, last, group, room, anchors),
            });
        }
        let content = column(lines).spacing(space::xs()).padding([space::m(), space::l()]).width(Fill);
        let content = container(content).width(Fill).max_width(size::content_max());
        scrollable(container(content).width(Fill).center_x(Fill))
            .id(HISTORY)
            .anchor_bottom()
            .height(Fill)
            .width(Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .on_scroll(|viewport| Message::Chat(Msg::Scrolled(viewport)))
            .into()
    }

    /// One message: the author's name above the first of a run (in groups, not the person's own),
    /// the bubble with the quote, the files and the text, the time below the last of a run. A single
    /// picture or video stands flush in its bubble, the caption under it (issue #674, decision 1);
    /// everything else inside the bubble's padding.
    #[allow(clippy::too_many_arguments)]
    fn message_line<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a torro_chat::Message,
        first: bool,
        last: bool,
        group: bool,
        room: Room,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let own = message.author.own;
        // "Show in chat" marks the message it went to.
        let marked = chat.files.highlight.as_deref() == Some(message.id.as_str());
        if let Some((file, width, height)) = flush_preview(chat, message, room) {
            let below = self.caption_parts(chat, message, width - 2.0 * space::m());
            let edge = bubble_edge(own, marked);
            let round = theme::radius::md() - edge;
            let corners = border::Radius {
                top_left: round,
                top_right: round,
                bottom_right: if below.is_empty() { round } else { 0.0 },
                bottom_left: if below.is_empty() { round } else { 0.0 },
            };
            let mut content = column![self.preview_part(chat, message, file, width, height, corners)];
            if !below.is_empty() {
                content = content.push(caption_block(below, width));
            }
            let bubble = bubble(content.into(), own, marked, Padding::new(edge), room.bubble);
            return self.message_frame(chat, &[message], bubble, first, last, group, room, anchors);
        }
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if let Some(quote) = &message.reply_to {
            parts.push(quote_block(&quote.author, &quote.text));
        }
        // Inside the bubble: what is left of its width after its padding.
        let inside = room.bubble - 2.0 * space::m();
        if message.kind == MessageKind::Deleted {
            parts.push(typo::text(typo::body(), tr("This message was deleted.")).style(theme::text_3).into());
        } else {
            // Files first, the text under them as their caption (stage 4), as Talk shows it.
            parts.extend(message.files.iter().map(|file| self.file_part(chat, message, file, inside)));
            parts.extend(self.caption_parts(chat, message, inside));
        }
        let content = column(parts).spacing(space::xs()).into();
        let bubble = bubble(content, own, marked, Padding::from([space::s(), space::m()]), room.bubble);
        self.message_frame(chat, &[message], bubble, first, last, group, room, anchors)
    }

    /// Pictures sent at once as one grid in one flush bubble (issue #674, decision 2): the tiles of
    /// [`super::files::album_tiles`], the caption of the last message under them. A click on a tile shows its
    /// picture large; the right button opens the menu of its own message. Reply, "…", reactions and
    /// the time belong to the last message.
    #[allow(clippy::too_many_arguments)]
    fn album_line<'a>(
        &'a self,
        chat: &'a ChatState,
        tiles: &[(&'a torro_chat::Message, &'a torro_chat::FileRef)],
        first: bool,
        last: bool,
        group: bool,
        room: Room,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let mut members: Vec<&'a torro_chat::Message> = Vec::new();
        for (message, _) in tiles {
            if members.last().is_none_or(|before| before.id != message.id) {
                members.push(message);
            }
        }
        let Some(&newest) = members.last() else { return Space::new().into() };
        let own = newest.author.own;
        let marked = members.iter().any(|member| chat.files.highlight.as_deref() == Some(member.id.as_str()));
        let width = room.picture;
        let below = self.caption_parts(chat, newest, width - 2.0 * space::m());
        let (rects, high) = super::files::album_tiles(tiles.len(), width, ALBUM_GAP, size::panel());
        let edge = bubble_edge(own, marked);
        let more = tiles.len().checked_sub(rects.len()).filter(|more| *more > 0);
        let mut layers: Vec<Element<'a, Message>> = Vec::new();
        for (at, (rect, (message, file))) in rects.iter().zip(tiles).enumerate() {
            let mut corners = super::files::tile_corners(*rect, width, high, !below.is_empty());
            for corner in
                [&mut corners.top_left, &mut corners.top_right, &mut corners.bottom_right, &mut corners.bottom_left]
            {
                *corner = (*corner - edge).max(0.0);
            }
            let plus = if at + 1 == rects.len() { more } else { None };
            let mut tile = self.album_tile(chat, message, file, *rect, corners, plus);
            if message.id != newest.id {
                let id = message.id.clone();
                tile = widgets::context_area(tile, move |at| Message::Chat(Msg::MessageMenu(id.clone(), at)));
                let menu_here = matches!(&self.menu, Some(Menu::Message { id, at: Some(_), .. }) if *id == message.id);
                if menu_here {
                    tile = widgets::anchored(&anchors.message, tile);
                }
            }
            layers.push(container(tile).padding(Padding::ZERO.top(rect.y).left(rect.x)).into());
        }
        let grid = iced::widget::Stack::with_children(layers).width(width).height(high);
        let mut content = column![grid];
        if !below.is_empty() {
            content = content.push(caption_block(below, width));
        }
        let bubble = bubble(content.into(), own, marked, Padding::new(edge), room.bubble);
        self.message_frame(chat, &members, bubble, first, last, group, room, anchors)
    }

    /// What stands under a message's files: its text (unless it only names its one file, or it is a
    /// voice message) and its link card, at most `room` wide.
    fn caption_parts<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a torro_chat::Message,
        room: f32,
    ) -> Vec<Element<'a, Message>> {
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if message.kind == MessageKind::Deleted {
            return parts;
        }
        let caption_is_name = message.files.len() == 1 && message.text == message.files[0].name;
        if !message.text.trim().is_empty() && !caption_is_name && message.kind != MessageKind::Voice {
            parts.push(message_text(&message.text, &message.mentions));
        }
        parts.extend(self.link_part(chat, message, room));
        parts
    }

    /// Around a bubble of `members` (one message, or the messages of a grid, the oldest first): the
    /// author's name above the first of a run (in groups, not the person's own), "Reply" and "…"
    /// beside the bubble under the pointer, the reactions, and the time below the last of a run.
    /// Right-clicked it opens the menu of the last member. Reactions of a grid count together; a
    /// click goes to the member with the person's own reaction, else to the last.
    #[allow(clippy::too_many_arguments)]
    fn message_frame<'a>(
        &'a self,
        chat: &'a ChatState,
        members: &[&'a torro_chat::Message],
        bubble: Element<'a, Message>,
        first: bool,
        last: bool,
        group: bool,
        room: Room,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let Some(&message) = members.last() else { return Space::new().into() };
        let own = message.author.own;
        let hovered = chat.hover.as_deref() == Some(message.id.as_str());
        // The "…" stays while its menu is open, so the menu keeps its place.
        let more_here = matches!(&self.menu, Some(Menu::Message { id, at: None, .. }) if *id == message.id);
        let tools: Element<'a, Message> = if (hovered || more_here) && !chat.gone {
            let reply: Element<'a, Message> = if timeline::is_reactable(message) {
                widgets::icon_button(Icon::Reply, tr("Reply"), Some(Message::Chat(Msg::Reply(message.id.clone()))))
            } else {
                Space::new().width(size::control_sm()).into()
            };
            let more = widgets::icon_button(
                Icon::Ellipsis,
                tr("More"),
                Some(Message::Chat(Msg::MoreMenu(message.id.clone()))),
            );
            let more = if more_here { widgets::anchored(&anchors.more, more) } else { more };
            // The "…" stands on the outer side, away from the bubble, in both directions.
            let tools = if own { row![more, reply] } else { row![reply, more] };
            tools.spacing(space::xs()).align_y(Alignment::Center).into()
        } else {
            // As wide as the two buttons, so the bubble does not move when they come.
            Space::new().width(tools_width()).into()
        };
        let line: Element<'a, Message> = if own {
            row![Space::new().width(Fill), tools, bubble].spacing(space::xs()).align_y(Alignment::Center).into()
        } else {
            row![bubble, tools, Space::new().width(Fill)].spacing(space::xs()).align_y(Alignment::Center).into()
        };
        let align = if own { Alignment::End } else { Alignment::Start };
        let mut column = column![].spacing(space::xs()).width(Fill).align_x(align);
        if first && group && !own {
            column = column.push(typo::text(typo::caption(), message.author.name.as_str()).style(theme::text_3));
        }
        column = column.push(line);
        let reactions = reactions_of(members);
        if !reactions.is_empty() {
            let chips = reactions
                .into_iter()
                .map(|(emoji, (count, given))| {
                    let target = given.unwrap_or(message);
                    // A reaction on its way takes no second click until the core answered.
                    let free = !chat.changing.contains_key(&target.id);
                    reaction_chip(&target.id, emoji, count, given.is_some(), free)
                })
                .collect();
            column = column.push(container(widgets::chip_row(chips)).max_width(room.bubble));
        }
        let deleting = members.iter().any(|member| chat.changing.get(&member.id) == Some(&Change::Delete));
        let editing = chat.editing.as_ref().is_some_and(|editing| members.iter().any(|member| member.id == editing.id));
        if deleting {
            column = column.push(typo::text(typo::micro(), tr("Deleting…")).style(theme::text_3));
        } else if editing {
            column = column.push(typo::text(typo::micro(), tr("Editing below")).style(theme::text_3));
        } else if last {
            let mut time = timeline::time_label(message.at);
            if message.edited {
                time = trf("{time} · edited", &[("time", &time)]);
            }
            column = column.push(typo::text(typo::micro().tabular(), time).style(theme::text_3));
        }
        let id = message.id.clone();
        let area = widgets::context_area(column, move |at| Message::Chat(Msg::MessageMenu(id.clone(), at)));
        let area = mouse_area(area)
            .on_enter(Message::Chat(Msg::Hover(Some(message.id.clone()))))
            .on_exit(Message::Chat(Msg::Hover(None)));
        let menu_here = matches!(&self.menu, Some(Menu::Message { id, at: Some(_), .. }) if *id == message.id);
        // A new run stands a little off the one before it.
        let gap = if first { space::xs() } else { 0.0 };
        let area = container(area).padding(iced::Padding::ZERO.top(gap));
        if menu_here { widgets::anchored(&anchors.message, area) } else { area.into() }
    }

    /// The composer at the bottom: why the last message did not go, the quote of a reply, the field
    /// and "Send".
    fn composer<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Element<'a, Message> {
        if chat.gone {
            return Space::new().into();
        }
        let body = typo::body();
        let (least, most) = editor_heights();
        let first = chat.suggestions.first().cloned();
        let read_only = self.conversation(chat).is_some_and(|c| c.read_only);
        let placeholder = if read_only {
            tr("Nobody can write here now").to_owned()
        } else {
            trf("Message {name}", &[("name", &self.chat_name(chat))])
        };
        let mut field = editor(&chat.editor)
            .id(EDITOR)
            .placeholder(placeholder)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(style::INPUT_PADDING)
            .min_height(least)
            .max_height(most)
            .wrapping(Wrapping::WordOrGlyph)
            .style(style::text_editor)
            .key_binding(move |press| composer_binding(press, first.clone()));
        if !read_only {
            field = field.on_action(|action| Message::Chat(Msg::Edit(action)));
        }
        let editing = chat.editing.is_some();
        let files = &chat.files;
        let something = !chat.editor.text().trim().is_empty() || (!editing && !files.ready().is_empty());
        let ready = something && !chat.sending && !read_only && !files.preparing() && files.upload.is_none();
        let (send_icon, send_tip, busy_tip) = if editing {
            (Icon::Check, tr("Save (Enter)"), tr("Saving…"))
        } else {
            (Icon::Send, tr("Send (Enter)"), tr("Sending…"))
        };
        let send: Element<'a, Message> = if chat.sending {
            widgets::with_tooltip(
                container(tinted(send_icon, icon::SIZE, |p| p.text_3)).center(size::control_sm()),
                busy_tip,
            )
        } else {
            widgets::icon_button(send_icon, send_tip, ready.then_some(Message::Chat(Msg::Send)))
        };
        let can_attach = !read_only && !editing && files.upload.is_none();
        let attach = widgets::icon_button(
            Icon::Paperclip,
            tr("Attach file"),
            can_attach.then_some(Message::Chat(Msg::Files(FilesMsg::Attach))),
        );
        let entry = row![attach, field, send].spacing(space::s()).align_y(Alignment::Center);
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if let Some(error) = &chat.send_error {
            let mark = tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text);
            let words = typo::text(typo::meta(), error.as_str())
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) });
            parts.push(
                row![mark, words.wrapping(Wrapping::WordOrGlyph)].spacing(space::s()).align_y(Alignment::Center).into(),
            );
        }
        if let Some(note) = &chat.note {
            // Calm: a change the server does not take is no failure of the app; the icon says which.
            let mark = if note.waiting {
                tinted(Icon::Clock, icon::SIZE_SM, |p| p.attention)
            } else {
                tinted(Icon::Info, icon::SIZE_SM, |p| p.text_2)
            };
            let words = typo::text(typo::meta(), note.text.as_str())
                .style(theme::text_2)
                .wrapping(Wrapping::WordOrGlyph)
                .width(Fill);
            let dismiss = widgets::icon_button(Icon::X, tr("Dismiss"), Some(Message::Chat(Msg::DismissNote)));
            parts.push(row![mark, words, dismiss].spacing(space::s()).align_y(Alignment::Center).into());
        }
        if let Some(editing) = &chat.editing {
            let cancel =
                widgets::icon_button(Icon::X, tr("Cancel editing (Esc)"), Some(Message::Chat(Msg::CancelEdit)));
            let words = column![
                typo::text(typo::caption(), tr("Edit message")).style(theme::text_2),
                widgets::ellipsized(typo::meta(), editing.original.as_str(), theme::text_3),
            ]
            .width(Fill);
            parts.push(
                row![tinted(Icon::SquarePen, icon::SIZE_SM, |p| p.text_3), words, cancel]
                    .spacing(space::s())
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        if let Some(quote) = &chat.reply {
            let cancel = widgets::icon_button(Icon::X, tr("Cancel reply (Esc)"), Some(Message::Chat(Msg::CancelReply)));
            let label = trf("Reply to {author}", &[("author", &quote.author)]);
            let words = column![
                typo::text(typo::caption(), label).style(theme::text_2),
                widgets::ellipsized(typo::meta(), quote.text.as_str(), theme::text_3),
            ]
            .width(Fill);
            parts.push(
                row![tinted(Icon::Reply, icon::SIZE_SM, |p| p.text_3), words, cancel]
                    .spacing(space::s())
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        // The files stand right above the field, where the text that becomes their caption is typed.
        parts.extend(self.composer_files(chat));
        parts.push(entry.into());
        let content = column(parts).spacing(space::s()).padding(composer_padding()).max_width(size::content_max());
        // The suggestions of an `@` float above the whole composer, so they never hide the quote.
        let content = widgets::anchored(&anchors.composer, content);
        column![rule::horizontal(HAIRLINE).style(style::rule), container(content).width(Fill).center_x(Fill)].into()
    }

    /// The open menu of a chat window: the head's "…", a message's context menu, or the suggestions
    /// of an `@` above the composer.
    fn chat_layer<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Option<Element<'a, Message>> {
        match &self.menu {
            Some(Menu::Head) => {
                let menu = widgets::menu_wide(self.head_sections(chat), size::props());
                return Some(widgets::floating(&anchors.head, Placement::Below(Align::End), menu, Message::CloseMenu));
            }
            Some(Menu::Message { id, at, confirm }) => {
                let message = chat.message(id)?;
                let read_only = self.conversation(chat).is_some_and(|c| c.read_only);
                let open = !chat.gone && !read_only;
                let mut first = Vec::new();
                if open && timeline::is_reactable(message) {
                    first.push(MenuItem::new(Icon::Reply, tr("Reply"), Message::Chat(Msg::Reply(id.clone()))));
                }
                if !message.text.is_empty() && message.kind != MessageKind::Deleted {
                    first.push(MenuItem::new(
                        Icon::Copy,
                        tr("Copy text"),
                        Message::Chat(Msg::Copy(message.text.clone())),
                    ));
                }
                let own = if !(open && timeline::is_changeable(message)) {
                    None
                } else if *confirm {
                    let items = vec![
                        MenuItem::new(Icon::Trash2, tr("Delete for everyone"), Message::Chat(Msg::Delete(id.clone()))),
                        MenuItem::new(Icon::X, tr("Cancel"), Message::CloseMenu),
                    ];
                    Some(MenuSection::titled(tr("Delete this message?"), items))
                } else {
                    Some(MenuSection::new(vec![
                        MenuItem::new(Icon::SquarePen, tr("Edit"), Message::Chat(Msg::StartEdit(id.clone()))),
                        MenuItem::new(Icon::Trash2, tr("Delete…"), Message::Chat(Msg::AskDelete(id.clone()))),
                    ]))
                };
                let last = vec![MenuItem::new(Icon::MailOpen, tr("Mark as unread"), Message::Chat(Msg::MarkUnread))];
                let mut sections: Vec<MenuSection<'a, Message>> =
                    [first].into_iter().filter(|items| !items.is_empty()).map(MenuSection::new).collect();
                sections.extend(file_items(message));
                sections.extend(own);
                sections.push(MenuSection::new(last));
                let reactions = (open && timeline::is_reactable(message) && !chat.changing.contains_key(id))
                    .then(|| quick_reactions(message));
                let card = menu_card(reactions.into_iter().collect(), sections);
                return Some(match at {
                    Some(at) => {
                        let anchor = anchors.message.area(Rectangle::new(*at, Size::ZERO));
                        widgets::floating(&anchor, Placement::At, card, Message::CloseMenu)
                    }
                    None => {
                        let side = if message.author.own { Align::End } else { Align::Start };
                        widgets::floating(&anchors.more, Placement::Below(side), card, Message::CloseMenu)
                    }
                });
            }
            Some(Menu::Shared { index, at }) => {
                let item = chat.files.media.as_ref()?.items.get(*index)?;
                let menu = widgets::menu(vec![MenuSection::new(self.shared_items(item))]);
                let anchor = anchors.shared.area(Rectangle::new(*at, Size::ZERO));
                return Some(widgets::floating(&anchor, Placement::At, menu, Message::CloseMenu));
            }
            _ => {}
        }
        if chat.suggestions.is_empty() {
            return None;
        }
        let items = chat
            .suggestions
            .iter()
            .enumerate()
            .map(|(at, mention)| {
                let item = MenuItem::new(Icon::User, mention.name.as_str(), Message::Chat(Msg::Pick(mention.clone())));
                // The first is what Enter picks.
                if at == 0 { item.trail(widgets::Trail::Kbd("Enter")) } else { item }
            })
            .collect();
        let menu = widgets::menu(vec![MenuSection::titled(tr("Mention"), items)]);
        Some(widgets::floating(&anchors.composer, Placement::Below(Align::Start), menu, Message::Chat(Msg::Escape)))
    }

    /// The head's "…": "Mark as unread" and the favourite, then the notification levels with the
    /// current one marked. Without the conversation in the list only the first.
    fn head_sections<'a>(&'a self, chat: &'a ChatState) -> Vec<MenuSection<'a, Message>> {
        let mut first = vec![MenuItem::new(Icon::MailOpen, tr("Mark as unread"), Message::Chat(Msg::MarkUnread))];
        let Some(conversation) = self.conversation(chat) else { return vec![MenuSection::new(first)] };
        let (mark, label) = if conversation.favorite {
            (Icon::PinOff, tr("Remove from favorites"))
        } else {
            (Icon::Pin, tr("Add to favorites"))
        };
        first.push(MenuItem::new(mark, label, Message::Chat(Msg::ToggleFavorite)));
        let levels = [NotifyLevel::Default, NotifyLevel::Always, NotifyLevel::Mention, NotifyLevel::Never]
            .into_iter()
            .map(|level| {
                let item = MenuItem::plain(level_word(level), Message::Chat(Msg::SetLevel(level)))
                    .current(conversation.notify == level);
                match level {
                    NotifyLevel::Default => item.detail(tr("Direct chats always, groups on mentions")),
                    NotifyLevel::Always | NotifyLevel::Mention | NotifyLevel::Never => item,
                }
            })
            .collect();
        vec![MenuSection::new(first), MenuSection::titled(tr("Notifications"), levels)]
    }

    /// The glance of a chat window in a mini or strip tile: the name and the newest message.
    pub(crate) fn chat_glance(&self, class: SizeClass) -> Element<'_, Message> {
        let Some(chat) = &self.chat else { return Space::new().into() };
        let name = widgets::ellipsized(typo::body_strong(), self.chat_name(chat), theme::text);
        let newest = chat.messages.iter().rev().find(|message| !timeline::is_system(message));
        let line = match newest {
            Some(message) => format!("{}: {}", people::first_name(&message.author.name), message.text),
            // A first page that failed is not loading: the glance says why.
            None => match &chat.error {
                Some(error) => error.clone(),
                None if chat.loaded => tr("No messages yet").to_owned(),
                None => tr("Loading…").to_owned(),
            },
        };
        let line = widgets::ellipsized(typo::meta(), line, theme::text_2);
        let pad = if class == SizeClass::Strip { [space::s(), space::m()] } else { [space::m(), space::m()] };
        container(column![name, line].spacing(space::xs())).width(Fill).height(Fill).padding(pad).center_y(Fill).into()
    }
}

/// Around the composer: the history's side margin, so field and bubbles share an axis, and a step
/// more below than above, so the field keeps air over the window's edge also in Dense.
fn composer_padding() -> Padding {
    Padding { top: space::s(), right: space::l(), bottom: space::m(), left: space::l() }
}

/// The composer's keys: `Enter` sends – or, while suggestions show, picks the first, as `Tab` does;
/// `Shift + Enter` starts a new line; the rest as usual.
pub(crate) fn composer_binding(press: KeyPress, first: Option<Mention>) -> Option<Binding<Message>> {
    use keyboard::key::Named;
    let named = |name: Named| matches!(press.key.as_ref(), keyboard::Key::Named(key) if key == name);
    if let Some(mention) = first
        && ((named(Named::Enter) && !press.modifiers.shift()) || named(Named::Tab))
    {
        return Some(Binding::Custom(Message::Chat(Msg::Pick(mention))));
    }
    if named(Named::Enter) && !press.modifiers.shift() {
        return Some(Binding::Custom(Message::Chat(Msg::Send)));
    }
    // A paste may bring a picture or files (stage 4): the window reads the clipboard itself and
    // pastes text as before. Shift keeps the plain paste of the field.
    let modifiers = press.modifiers;
    if modifiers.command()
        && !modifiers.alt()
        && !modifiers.shift()
        && matches!(press.key.as_ref(), keyboard::Key::Character("v" | "V"))
    {
        return Some(Binding::Custom(Message::Chat(Msg::Files(FilesMsg::Paste))));
    }
    Binding::from_key_press(press)
}

/// A reaction under a message: the emoji and how many gave it, a chip as high as every chip. The
/// person's own is chosen (`selected` with the stronger edge) and its tooltip says a click takes it
/// back; `clickable` is false while a reaction to the message is on its way.
fn reaction_chip<'a>(id: &str, emoji: &'a str, count: u64, given: bool, clickable: bool) -> Element<'a, Message> {
    let content = row![
        typo::text(typo::body(), emoji).wrapping(Wrapping::None),
        typo::text(typo::meta().tabular(), count.to_string()).wrapping(Wrapping::None),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    let press = clickable.then(|| Message::Chat(Msg::React(id.to_owned(), emoji.to_owned())));
    let chip = button(container(content).center_y(Fill))
        .height(style::CHIP_HEIGHT)
        .padding(style::CHIP_PADDING)
        .style(style::button::choice_chip(given))
        .on_press_maybe(press);
    let tip =
        if given { tr("Take back your reaction").to_owned() } else { trf("React with {emoji}", &[("emoji", &emoji)]) };
    widgets::with_tooltip(chip, tip)
}

/// The quick reactions at the top of a message's menu, side by side over its whole width; one the
/// person gave already is chosen, and a click takes it back.
fn quick_reactions<'a>(message: &torro_chat::Message) -> Element<'a, Message> {
    let buttons = timeline::QUICK_REACTIONS.into_iter().map(|emoji| {
        let given = message.own_reactions.iter().any(|own| own == emoji);
        let face = typo::text(typo::title(), emoji).wrapping(Wrapping::None);
        let pick = button(container(face).center(Fill))
            .width(Fill)
            .height(size::control_sm())
            .padding(Padding::ZERO)
            .style(if given { style::button::icon_chosen } else { style::button::icon })
            .on_press(Message::Chat(Msg::React(message.id.clone(), emoji.to_owned())));
        let tip = if given {
            tr("Take back your reaction").to_owned()
        } else {
            trf("React with {emoji}", &[("emoji", &emoji)])
        };
        widgets::with_tooltip(pick, tip)
    });
    row(buttons.collect::<Vec<_>>()).width(Fill).into()
}

/// A menu card as [`widgets::menu`] draws it, with the rows of `top` (the quick reactions, the statuses) above
/// the entries and a line between them.
pub(super) fn menu_card<'a>(
    top: Vec<Element<'a, Message>>,
    sections: Vec<MenuSection<'a, Message>>,
) -> Element<'a, Message> {
    let line = || -> Element<'a, Message> {
        container(rule::horizontal(HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]).into()
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if !top.is_empty() {
        rows.extend(top);
        rows.push(line());
    }
    for (at, section) in sections.into_iter().enumerate() {
        if at > 0 {
            rows.push(line());
        }
        if let Some(title) = section.title {
            let label = typo::text(typo::caption(), title).style(theme::text_3);
            rows.push(container(label).padding(Padding::new(space::s()).bottom(space::xs())).into());
        }
        rows.extend(section.items.into_iter().map(widgets::menu_item));
    }
    container(column(rows)).width(widgets::menu_width()).padding(style::MENU_INSET).style(style::container::menu).into()
}

/// The file entries of a message's menu (stage 4), for its first file – Talk shares one file per
/// message: show it large or open it, save it to Downloads or elsewhere, open it in Nextcloud. A file
/// shared without download keeps only the last.
fn file_items<'a>(message: &'a torro_chat::Message) -> Option<MenuSection<'a, Message>> {
    let file = message.files.first().filter(|_| message.kind != MessageKind::Deleted)?;
    let (id, file_id) = (message.id.clone(), file.id.clone());
    let wrap = |msg: FilesMsg| Message::Chat(Msg::Files(msg));
    let mut items = Vec::new();
    if super::files::look(message, file, None) == super::files::Look::Picture {
        items.push(MenuItem::new(Icon::Maximize, tr("Show large"), wrap(FilesMsg::View(id.clone(), file_id.clone()))));
    }
    if !file.hide_download {
        items.push(MenuItem::new(Icon::ExternalLink, tr("Open"), wrap(FilesMsg::Open(id.clone(), file_id.clone()))));
        items.push(MenuItem::new(
            Icon::Download,
            tr("Save to Downloads"),
            wrap(FilesMsg::Save(id.clone(), file_id.clone())),
        ));
        items.push(MenuItem::new(Icon::Save, tr("Save as…"), wrap(FilesMsg::SaveAs(id, file_id.clone()))));
    }
    items.push(MenuItem::new(Icon::Cloud, tr("Open in Nextcloud"), wrap(FilesMsg::Nextcloud(file_id))));
    Some(MenuSection::new(items))
}

/// The text of a message with its mentions in the stronger weight.
fn message_text<'a>(text: &'a str, mentions: &'a [Mention]) -> Element<'a, Message> {
    let body = typo::body();
    let strong = typo::body_strong();
    let spans: Vec<Span<'a, (), iced::Font>> = timeline::pieces(text, mentions)
        .into_iter()
        .map(|(piece, mention)| {
            let span = Span::new(piece);
            if mention { span.font(strong.font) } else { span.font(body.font) }
        })
        .collect();
    let rich: Rich<'a, (), Message> =
        rich_text(spans).size(body.size).line_height(body.line_height).wrapping(Wrapping::WordOrGlyph);
    rich.into()
}

/// The quote of the message another one answers: a stroke at the left and beside it the author
/// over the text in one line, as the Notification Hub's panel quotes (no surface of its own).
fn quote_block<'a>(author: &'a str, text: &'a str) -> Element<'a, Message> {
    let text = if text.is_empty() { tr("This message was deleted.") } else { text };
    let words = column![
        typo::text(typo::caption(), author).style(theme::text_2),
        widgets::ellipsized(typo::meta(), text, theme::text_3),
    ]
    .width(Fill);
    // As high as its two lines: a filling height would collapse in a row that shrinks to them.
    let high = typo::line_height(typo::caption()) + typo::line_height(typo::meta());
    let stroke = container(Space::new().width(QUOTE_STROKE).height(high)).style(|t: &Theme| container::Style {
        background: Some(Background::Color(palette(t).line_selected)),
        ..container::Style::default()
    });
    row![stroke, words].spacing(space::s()).into()
}

/// A message of the provider: small and grey in the middle, a missed call with its icon, the time
/// after it.
fn system_line<'a>(message: &'a torro_chat::Message) -> Element<'a, Message> {
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if message.kind == MessageKind::MissedCall {
        content = content.push(tinted(Icon::Video, icon::SIZE_SM, |p| p.text_3));
    }
    let words = format!("{} · {}", message.text, timeline::time_label(message.at));
    content = content.push(typo::text(typo::caption(), words).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    centred(content)
}

/// The divider "New": a line on either side of the word.
fn new_divider<'a>() -> Element<'a, Message> {
    let line = || {
        rule::horizontal(HAIRLINE).style(|t: &Theme| rule::Style { color: palette(t).line_strong, ..style::rule(t) })
    };
    row![line(), typo::text(typo::caption(), tr("New")).style(theme::text_2), line()]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .padding([space::s(), 0.0])
        .into()
}

/// `content` in the middle of the history.
fn centred<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Fill).center_x(Fill).padding([space::s(), 0.0]).into()
}
