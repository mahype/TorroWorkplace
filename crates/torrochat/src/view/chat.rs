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

use iced::widget::text::{Rich, Span, Wrapping};
use iced::widget::text_editor::{Binding, KeyPress};
use iced::widget::{
    Space, button, column, container, mouse_area, rich_text, row, rule, scrollable, text_editor as editor,
};
use iced::{Alignment, Background, Element, Fill, Padding, Rectangle, Size, Theme, border, keyboard};
use torro_chat::{Conversation, FileRef, Mention, MessageKind, NotifyLevel, Person};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, Cover, TopRow};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, MenuItem, MenuSection, Placement, TileFace, TileSize};
use torro_gui::{style, typo, units};

use super::{Anchors, HAIRLINE, avatar, kind_words, tinted};
use crate::app::chat::{Change, EDITOR, HISTORY, Msg, level_word};
use crate::app::{App, ChatState, Menu, Message};
use crate::people::{self, Group};
use crate::timeline::{self, Item};

/// How much of the history's width a message may take at most: the other side stays free, so who
/// wrote it reads from where it stands.
const BUBBLE_SHARE: f32 = 0.78;

/// The composer: two lines high at first, at most six, then it scrolls.
fn editor_heights() -> (f32, f32) {
    let line = typo::line_height(typo::body());
    let pad = style::INPUT_PADDING.top + style::INPUT_PADDING.bottom;
    (2.0 * line + pad, 6.0 * line + pad)
}

impl App {
    /// A chat window: the flush frame of the narrow layout in every size, with the top row.
    pub(crate) fn chat_window<'a>(&'a self, chat: &'a ChatState, anchors: &Anchors) -> Element<'a, Message> {
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow).title(AppId::CHAT).cover(Cover::Nothing);
        let body = column![self.history(chat, anchors), self.composer(chat, anchors)].width(Fill).height(Fill);
        let body = container(body).width(Fill).height(Fill).center_x(Fill);
        shell::shell(SizeClass::Narrow, Space::new(), self.chat_head(chat, anchors), body, None)
            .overlay(self.chat_layer(chat, anchors))
            .top_row(top)
            .into()
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
            (None, Some(conversation)) if conversation.kind != torro_chat::ConversationKind::Direct => (
                widgets::source_tile(TileFace::Icon(Icon::Users), TileSize::Large, None),
                kind_words(conversation).to_owned(),
            ),
            _ => (avatar(&name, &chat.id, None, &colours), String::new()),
        };
        // Name over status: both fit the top row (22 + 18 of the standard's 40).
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
        let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::Chat(Msg::HeadMenu)));
        head.push(widgets::anchored(&anchors.head, more)).into()
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
            return container(widgets::error_state(error.as_str(), Message::Chat(Msg::Refresh)))
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
        let width = self.size.width.min(size::content_max());
        let bubble = width * BUBBLE_SHARE;
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
                    self.message_line(chat, message, first, last, group, bubble, anchors)
                }
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
    /// the bubble with the quote, the text and the files, the time below the last of a run. Under
    /// the pointer it offers "Reply" beside the bubble; right-clicked its menu.
    #[allow(clippy::too_many_arguments)]
    fn message_line<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a torro_chat::Message,
        first: bool,
        last: bool,
        group: bool,
        bubble_max: f32,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let own = message.author.own;
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if let Some(quote) = &message.reply_to {
            parts.push(quote_block(&quote.author, &quote.text));
        }
        if message.kind == MessageKind::Deleted {
            parts.push(typo::text(typo::body(), tr("This message was deleted.")).style(theme::text_3).into());
        } else if !message.text.trim().is_empty()
            && !(message.files.len() == 1 && message.text == message.files[0].name)
        {
            parts.push(message_text(&message.text, &message.mentions));
        }
        parts.extend(message.files.iter().map(file_line));
        let bubble = container(column(parts).spacing(space::xs()))
            .padding([space::s(), space::m()])
            .max_width(bubble_max)
            .style(move |t: &Theme| {
                let p = palette(t);
                container::Style {
                    background: Some(Background::Color(if own { p.selected } else { p.raised })),
                    border: border::rounded(theme::radius::md())
                        .color(if own { p.line_strong } else { p.line })
                        .width(HAIRLINE),
                    text_color: Some(p.text),
                    ..container::Style::default()
                }
            });
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
            Space::new().width(size::control_sm() + space::xs() + size::control_sm()).into()
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
        let change = chat.changing.get(&message.id).copied();
        if !message.reactions.is_empty() {
            let chips = message
                .reactions
                .iter()
                .map(|(emoji, count)| {
                    let given = message.own_reactions.iter().any(|own| own == emoji);
                    // A reaction on its way takes no second click until the core answered.
                    reaction_chip(&message.id, emoji, *count, given, change.is_none())
                })
                .collect();
            column = column.push(container(widgets::chip_row(chips)).max_width(bubble_max));
        }
        if change == Some(Change::Delete) {
            column = column.push(typo::text(typo::micro(), tr("Deleting…")).style(theme::text_3));
        } else if chat.editing.as_ref().is_some_and(|editing| editing.id == message.id) {
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
        let ready = !chat.editor.text().trim().is_empty() && !chat.sending && !read_only;
        let editing = chat.editing.is_some();
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
        let entry = row![field, send].spacing(space::s()).align_y(Alignment::End);
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
        parts.push(entry.into());
        let content =
            column(parts).spacing(space::s()).padding([space::s(), space::m()]).max_width(size::content_max());
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
                sections.extend(own);
                sections.push(MenuSection::new(last));
                let reactions = (open && timeline::is_reactable(message) && !chat.changing.contains_key(id))
                    .then(|| quick_reactions(message));
                let card = menu_card(reactions, sections);
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
            None if chat.loaded => tr("No messages yet").to_owned(),
            None => tr("Loading…").to_owned(),
        };
        let line = widgets::ellipsized(typo::meta(), line, theme::text_2);
        let pad = if class == SizeClass::Strip { [space::s(), space::m()] } else { [space::m(), space::m()] };
        container(column![name, line].spacing(space::xs())).width(Fill).height(Fill).padding(pad).center_y(Fill).into()
    }
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

/// A menu card as [`widgets::menu`] draws it, with `top` (the quick reactions) above the entries.
fn menu_card<'a>(top: Option<Element<'a, Message>>, sections: Vec<MenuSection<'a, Message>>) -> Element<'a, Message> {
    let line = || -> Element<'a, Message> {
        container(rule::horizontal(HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]).into()
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(top) = top {
        rows.push(top);
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

/// The quote of the message another one answers: the author and the text in one line, on a quiet
/// surface inside the bubble (a line's tint, so it reads on `raised` and on `selected`).
fn quote_block<'a>(author: &'a str, text: &'a str) -> Element<'a, Message> {
    let text = if text.is_empty() { tr("This message was deleted.") } else { text };
    let words = column![
        typo::text(typo::caption(), author).style(theme::text_2),
        widgets::ellipsized(typo::meta(), text, theme::text_3),
    ];
    container(words)
        .width(Fill)
        .padding([space::xs(), space::s()])
        .style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.line_strong)),
                border: border::rounded(theme::radius::xs()),
                ..container::Style::default()
            }
        })
        .into()
}

/// A file of a message as one line (stage 3: every kind; pictures and players come with stage 4):
/// its icon, name and size.
fn file_line<'a>(file: &'a FileRef) -> Element<'a, Message> {
    let size = units::format_bytes(file.size);
    row![
        tinted(Icon::FileText, icon::SIZE, |p| p.text_2),
        widgets::ellipsized(typo::body(), file.name.as_str(), theme::text),
        typo::text(typo::meta().tabular(), size).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
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
