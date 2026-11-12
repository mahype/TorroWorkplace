//! How the windows of `torrochat` look (design skill `torro-design`, drafts in
//! `docs/prototypes/chat/`): the contact list here, a chat window in [`chat`].
//!
//! Nothing here changes state: every control sends a [`Message`] that [`App::update`] handles.
//!
//! The contact list is a narrow tile without sidebar: the flush frame of the narrow layout in every
//! size, with the top row (the app's name, "New chat" at the right). Under it the search and
//! "People | Groups", the list, and the own status at the bottom. In a wide tile the column stays
//! [`size::content_max`] wide, centred: a list to read is never stretched. Mini shows how many are
//! there and the unread chats in figures; the strip the faces of the people who are there.
//!
//! Presence marks (`docs/chat.md`, "Kontaktliste"): each status has a form of its own, so it never
//! rests on colour alone – a dot (online), the moon (away), half a circle (busy), a dot with a bar (do
//! not disturb), a ring (offline). Their word stands beside them or in the tooltip.

mod chat;

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, mouse_area, row, rule, scrollable, stack, text_input};
use iced::{Alignment, Background, Element, Fill, Padding, Rectangle, Size, Theme, border};
use torro_chat::{Conversation, OwnStatus, Person, Status};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, Cover, Surface, TitleButtons, TopRow};
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, TileFace, TileSize};
use torro_gui::{layout, style, typo};

use crate::app::{App, ClearAfter, Dialog, Menu, Message, NEW_QUERY, SEARCH, STATUS_TEXT, Tab, Target};
use crate::people::{self, Group};

/// The ring around a presence mark, in the surface under it, so the mark stands off the avatar.
const MARK_RING: f32 = 2.0;

/// A line between parts (torro-design, "Linien": one logical pixel).
pub(crate) const HAIRLINE: f32 = 1.0;

/// The faces the strip shows before "+n".
const STRIP_FACES: usize = 8;

/// The unread chats a glance lists where it has room (mini tall, strip).
const GLANCE_CHATS: usize = 3;

/// How a row of the contact list reads: a person offline dimmed, a favourite with its pin.
#[derive(Debug, Clone, Copy)]
struct Look {
    dim: bool,
    favorite: bool,
}

/// The anchors of the menus that float next to their triggers; new in every `view`.
#[derive(Default)]
pub(crate) struct Anchors {
    pub status: Anchor,
    pub row: Anchor,
    pub head: Anchor,
    pub message: Anchor,
    /// The "…" beside a message under the pointer.
    pub more: Anchor,
    pub composer: Anchor,
}

impl App {
    /// The window: a chat window or the contact list, with the zoom badge and the notice over it.
    pub fn view(&self) -> Element<'_, Message> {
        let class = self.class();
        if matches!(class, SizeClass::Mini | SizeClass::Strip) {
            let glance = if self.chat.is_some() { self.chat_glance(class) } else { self.glance(class) };
            return widgets::ctrl_guard(shell::titlebar(
                self.zoom.view(glance),
                self.zoom.factor(),
                Surface::of(class),
                Cover::Nothing,
                TitleButtons::none(),
                Message::DragWindow,
            ));
        }
        let anchors = Anchors::default();
        let window = match &self.chat {
            Some(chat) => self.chat_window(chat, &anchors),
            None => self.contacts(&anchors),
        };
        let window = match &self.dialog {
            Some(dialog) => widgets::modal(window, self.dialog_view(dialog), size::form_max(), Message::CloseDialog),
            None => window,
        };
        // Always two layers, so the window keeps its state when the notice comes and goes.
        let notice: Element<'_, Message> = match &self.notice {
            Some(notice) => widgets::toast(notice.text.as_str(), None),
            None => Space::new().into(),
        };
        widgets::ctrl_guard(self.zoom.view(stack![window, notice].width(Fill).height(Fill)))
    }

    /// The colours of the window's theme, for the avatars, whose hue is the person's.
    pub(crate) fn colours(&self) -> Palette {
        palette(&self.theme())
    }

    // -- The contact list -----------------------------------------------------------------------

    fn contacts<'a>(&'a self, anchors: &Anchors) -> Element<'a, Message> {
        let new = widgets::icon_button(Icon::Plus, tr("New chat"), Some(Message::NewChat));
        let header = row![Space::new().width(Fill), new].align_y(Alignment::Center);
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow).title(AppId::CHAT).cover(self.cover());
        let body = container(self.contacts_body(anchors)).width(Fill).height(Fill).center_x(Fill);
        shell::shell(SizeClass::Narrow, Space::new(), header, body, None)
            .overlay(self.contacts_layer(anchors))
            .top_row(top)
            .into()
    }

    /// What lies over the top of the window, which the top row repeats: a dialog.
    fn cover(&self) -> Cover {
        Cover::dialog(self.dialog.is_some())
    }

    /// Search and tabs, the list, the own status: a column at most `content_max` wide.
    fn contacts_body<'a>(&'a self, anchors: &Anchors) -> Element<'a, Message> {
        let body = typo::body();
        let placeholder = match self.tab {
            Tab::People => tr("Search people"),
            Tab::Groups => tr("Search groups"),
            Tab::Messages => tr("Search messages"),
        };
        // A quiet field as the mail window's, its magnifier inside before the text.
        let lead = style::INPUT_PADDING.left + icon::SIZE + space::s();
        let input = text_input(placeholder, &self.search)
            .id(SEARCH)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(Padding { left: lead, ..style::INPUT_PADDING })
            .width(Fill)
            .style(style::text_input_quiet)
            .on_input(Message::Search)
            .on_submit(Message::SearchSubmit);
        let glass = container(tinted(Icon::Search, icon::SIZE, |p| p.text_3))
            .height(Fill)
            .center_y(Fill)
            .padding(Padding::ZERO.left(style::INPUT_PADDING.left));
        let search = stack![input, glass];
        let unread = people::unread_groups(&self.conversations);
        let groups = if unread > 0 { trf("Groups ({count})", &[("count", &unread)]) } else { tr("Groups").to_owned() };
        let selected = match self.tab {
            Tab::People => 0,
            Tab::Groups => 1,
            Tab::Messages => 2,
        };
        let tabs = segmented_fill(
            [
                (tr("People").to_owned(), Message::Tab(Tab::People)),
                (groups, Message::Tab(Tab::Groups)),
                (tr("Messages").to_owned(), Message::Tab(Tab::Messages)),
            ],
            selected,
        );
        let tools = column![search, tabs].spacing(space::s()).padding(space::s());
        let list = scrollable(container(self.list(anchors)).padding([0.0, space::s()]))
            .height(Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(style::scrollbar()));
        let mut content = column![tools, list].width(Fill).max_width(size::content_max()).height(Fill);
        if let Some(own) = &self.own {
            content = content.push(widgets::anchored(&anchors.status, self.footer(own)));
        }
        content.into()
    }

    /// The list of the open tab, or why there is none.
    fn list<'a>(&'a self, anchors: &Anchors) -> Element<'a, Message> {
        if !self.people_loaded {
            return widgets::loading();
        }
        if self.people.is_empty() && self.conversations.is_empty() {
            if let Some(err) = &self.error {
                return no_people(
                    crate::app::chat_failed(err),
                    Some((tr("Open Torro Workplace"), Message::OpenWorkplace)),
                );
            }
            if !self.conversations_loaded {
                return widgets::loading();
            }
            return no_people(
                tr("Connect a Nextcloud account with Talk in Torro Workplace to chat.").to_owned(),
                Some((tr("Open Torro Workplace"), Message::OpenWorkplace)),
            );
        }
        if self.tab == Tab::Messages {
            return self.hit_list(anchors);
        }
        let rows: Vec<Element<'a, Message>> = match self.tab {
            Tab::People => self.people_rows(anchors),
            Tab::Groups | Tab::Messages => self.group_rows(anchors),
        };
        if rows.is_empty() {
            let (title, hint) = match (self.tab, self.search.trim().is_empty()) {
                (Tab::People, true) => (tr("No people yet"), tr("Your Nextcloud does not show other users.")),
                (Tab::Groups | Tab::Messages, true) => {
                    (tr("No group chats"), tr("Group chats you take part in show here."))
                }
                (_, false) => (tr("No matches"), tr("Try another name.")),
            };
            return container(widgets::empty_state(title, hint)).padding([space::xl(), 0.0]).into();
        }
        column(rows).spacing(space::xs()).padding(Padding::ZERO.bottom(space::s())).into()
    }

    fn people_rows<'a>(&'a self, anchors: &Anchors) -> Vec<Element<'a, Message>> {
        let searching = !self.search.trim().is_empty();
        let colours = self.colours();
        let mut rows = Vec::new();
        for (group, members) in people::groups(&self.people, &self.conversations, &self.search) {
            // A search shows every match; a folded group keeps its head.
            let folded = self.folded(group) && !searching;
            rows.push(group_head(group.title(), members.len(), !folded, Message::Fold(group)));
            if folded {
                continue;
            }
            for person in members {
                let chat = people::direct_chat(&self.conversations, person);
                let unread = chat.map_or(0, |c| c.unread);
                let lead = avatar(&person.name, &person.id, Some(Group::of(person).status()), &colours);
                let target = Target::Person(person.id.clone());
                let line = people::person_line(person);
                // The group "Favorites" says it already; a pin on each of its rows would repeat it.
                let look = Look { dim: Group::of(person) == Group::Offline, favorite: false };
                rows.push(self.row(target, lead, &person.name, line, unread, look, anchors));
            }
        }
        rows
    }

    fn group_rows<'a>(&'a self, anchors: &Anchors) -> Vec<Element<'a, Message>> {
        people::group_chats(&self.conversations, &self.search)
            .into_iter()
            .map(|conversation| {
                let lead = tile_face(Icon::Users);
                let target = Target::Conversation(conversation.id.clone());
                let line = people::chat_line(conversation);
                let look = Look { dim: false, favorite: conversation.favorite };
                self.row(target, lead, &conversation.name, line, conversation.unread, look, anchors)
            })
            .collect()
    }

    /// The tab "Messages": what to type, the search running, why it failed, or the hits – each with
    /// the conversation's face, "Author in Conversation" and the part of the text that matched.
    fn hit_list<'a>(&'a self, anchors: &Anchors) -> Element<'a, Message> {
        let found = &self.found;
        let quiet = |title: &'a str, hint: &'a str| -> Element<'a, Message> {
            container(widgets::empty_state(title, hint)).padding([space::xl(), 0.0]).into()
        };
        if found.query.is_empty() {
            return quiet(tr("Search messages"), tr("Type at least two letters to search all your chats."));
        }
        if let Some(error) = &found.error {
            return no_people(error.clone(), None);
        }
        if found.searching && found.hits.is_empty() {
            return container(widgets::loading()).padding([space::xl(), 0.0]).into();
        }
        if found.hits.is_empty() {
            return quiet(tr("No messages found"), tr("Try other words."));
        }
        let colours = self.colours();
        let rows = found.hits.iter().map(|hit| {
            let conversation = torro_chat::split_message(&hit.id)
                .and_then(|(id, _)| self.conversations.iter().find(|conversation| conversation.id == id));
            let lead = match conversation {
                Some(c) if c.kind == torro_chat::ConversationKind::Direct => {
                    avatar(&c.name, c.person.as_deref().unwrap_or(&c.id), None, &colours)
                }
                Some(_) => tile_face(Icon::Users),
                None => tile_face(Icon::MessageCircle),
            };
            let look = Look { dim: false, favorite: false };
            self.row(Target::Hit(hit.id.clone()), lead, &hit.title, hit.text.clone(), 0, look, anchors)
        });
        column(rows.collect::<Vec<_>>()).spacing(space::xs()).padding(Padding::ZERO.bottom(space::s())).into()
    }

    /// One row: the face, the name over its line, a favourite's pin and the unread count. No button,
    /// so the double and right clicks reach it; it takes the look of a sidebar row for its surface.
    #[allow(clippy::too_many_arguments)]
    fn row<'a>(
        &'a self,
        target: Target,
        lead: Element<'a, Message>,
        name: &'a str,
        line: String,
        unread: u64,
        look: Look,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let chosen = self.cursor.as_ref() == Some(&target);
        let hovered = self.hover.as_ref() == Some(&target);
        let name_style = if look.dim { theme::text_2 } else { theme::text };
        let words = column![
            widgets::ellipsized(typo::body_strong(), name, name_style),
            widgets::ellipsized(typo::meta(), line, theme::text_3),
        ]
        .width(Fill);
        let mut content = row![lead, words].spacing(space::m()).align_y(Alignment::Center);
        if look.favorite {
            content =
                content.push(widgets::with_tooltip(tinted(Icon::Pin, icon::SIZE_SM, |p| p.text_3), tr("Favorite")));
        }
        if unread > 0 {
            content = content.push(widgets::with_tooltip(count_badge(unread), unread_words(unread)));
        }
        let surface = container(content)
            .width(Fill)
            .height(size::row())
            .padding(style::ITEM_PADDING)
            .center_y(size::row())
            .style(move |t: &Theme| {
                let status = if hovered { button::Status::Hovered } else { button::Status::Active };
                let look = style::button::sidebar_item(chosen)(t, status);
                container::Style {
                    background: look.background,
                    text_color: Some(look.text_color),
                    border: look.border,
                    ..container::Style::default()
                }
            });
        let menu_target = target.clone();
        let area = widgets::context_area(surface, move |at| Message::RowMenu(menu_target.clone(), at));
        let row = mouse_area(area)
            .on_press(Message::Select(target.clone()))
            .on_double_click(Message::Open(target.clone()))
            .on_enter(Message::Hover(Some(target.clone())))
            .on_exit(Message::Hover(None))
            .interaction(iced::mouse::Interaction::Pointer);
        let menu_here = matches!(&self.menu, Some(Menu::Row { target: open, .. }) if *open == target);
        let row: Element<'a, Message> = if menu_here { widgets::anchored(&anchors.row, row) } else { row.into() };
        // The person whose direct chat is being created shows it at once.
        if matches!(&target, Target::Person(id) if self.creating.as_ref() == Some(id)) {
            return widgets::with_tooltip(row, tr("Opening the chat…"));
        }
        row
    }

    /// The own status at the bottom: face with mark, the status's word, the message (or what to do),
    /// and the chevron of its menu.
    fn footer<'a>(&'a self, own: &'a OwnStatus) -> Element<'a, Message> {
        let colours = self.colours();
        let face = avatar(&own.name, &own.account, Some(own.status), &colours);
        let line = match own.message.as_deref().filter(|message| !message.trim().is_empty()) {
            Some(message) => match own.icon.as_deref() {
                Some(icon) if !icon.is_empty() => format!("{icon} {message}"),
                _ => message.to_owned(),
            },
            None if !own.heartbeat => tr("You appear offline to others").to_owned(),
            None => tr("Set a status message").to_owned(),
        };
        let words = column![
            widgets::ellipsized(typo::body_strong(), people::status_word(own.status), theme::text),
            widgets::ellipsized(typo::meta(), line, theme::text_3),
        ]
        .width(Fill);
        let open = self.menu == Some(Menu::Status);
        let chevron = tinted(if open { Icon::ChevronDown } else { Icon::ChevronUp }, icon::SIZE_SM, |p| p.text_3);
        let content = row![face, words, chevron].spacing(space::m()).align_y(Alignment::Center);
        let button = button(content)
            .width(Fill)
            .height(size::row())
            .padding(style::ITEM_PADDING)
            .style(style::button::sidebar_item(open))
            .on_press(Message::ToggleStatusMenu);
        let line = rule::horizontal(HAIRLINE).style(style::rule);
        column![line, container(widgets::with_tooltip(button, tr("Set your status"))).padding(space::s())].into()
    }

    /// The open menu of the contact list, floating at its trigger.
    fn contacts_layer(&self, anchors: &Anchors) -> Option<Element<'_, Message>> {
        match self.menu.as_ref()? {
            Menu::Status => Some(widgets::floating(
                &anchors.status,
                Placement::Below(Align::Start),
                self.status_menu(),
                Message::CloseMenu,
            )),
            Menu::Row { target, at } => {
                let anchor = anchors.row.area(Rectangle::new(*at, Size::ZERO));
                Some(widgets::floating(&anchor, Placement::At, self.row_menu(target), Message::CloseMenu))
            }
            Menu::Head | Menu::Message { .. } => None,
        }
    }

    /// The own status's menu: the statuses, each with its mark, then the status message.
    fn status_menu(&self) -> Element<'_, Message> {
        let own = self.own.as_ref();
        let heartbeat = own.is_some_and(|own| own.heartbeat);
        let statuses = [Status::Online, Status::Away, Status::Busy, Status::Dnd, Status::Invisible]
            .into_iter()
            .map(|status| {
                let detail = match status {
                    Status::Online if !heartbeat => Some(tr("Needs “Show others when I am online”")),
                    Status::Invisible => Some(tr("Others see you offline")),
                    _ => None,
                };
                status_entry(status, own.is_some_and(|own| own.status == status), detail)
            })
            .collect();
        let mut message = vec![MenuItem::new(Icon::SquarePen, tr("Set status message…"), Message::StatusDialog)];
        if own.and_then(|own| own.message.as_deref()).is_some_and(|text| !text.trim().is_empty()) {
            message.push(MenuItem::new(Icon::X, tr("Clear status message"), Message::ClearStatusText));
        }
        chat::menu_card(statuses, vec![MenuSection::new(message)])
    }

    /// A row's context menu: open the chat, and take off the list someone added by hand.
    fn row_menu(&self, target: &Target) -> Element<'_, Message> {
        let mut items = vec![MenuItem::new(Icon::MessageCircle, tr("Open chat"), Message::Open(target.clone()))];
        if let Target::Person(id) = target
            && self.people.iter().any(|person| &person.id == id && person.added)
        {
            items.push(MenuItem::new(Icon::X, tr("Remove from list"), Message::Remove(id.clone())));
        }
        widgets::menu(vec![MenuSection::new(items)])
    }

    // -- Dialogs --------------------------------------------------------------------------------

    fn dialog_view<'a>(&'a self, dialog: &'a Dialog) -> Element<'a, Message> {
        let body = typo::body();
        let field = |placeholder: &'a str, value: &'a str, id: &'static str| {
            text_input(placeholder, value)
                .id(id)
                .size(body.size)
                .line_height(body.line_height)
                .font(body.font)
                .padding(style::INPUT_PADDING)
                .width(Fill)
                .style(style::text_input)
        };
        let cancel = widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseDialog));
        let (title, content, confirm): (&str, Element<'a, Message>, Option<Element<'a, Message>>) = match dialog {
            Dialog::NewChat { query, found, searching, error } => {
                let input = field(tr("Name of a person"), query, NEW_QUERY).on_input(Message::NewQuery);
                let result: Element<'a, Message> = if let Some(error) = error {
                    hint(error.as_str())
                } else if query.trim().chars().count() < torro_ipc::chat::MIN_QUERY {
                    hint(tr("Type at least two letters of a name."))
                } else if *searching && found.is_empty() {
                    hint(tr("Searching…"))
                } else if found.is_empty() {
                    hint(tr("No one found with this name."))
                } else {
                    let colours = self.colours();
                    column(found.iter().map(|person| found_row(person, &colours))).spacing(space::xs()).into()
                };
                (tr("New chat"), column![input, result].spacing(space::m()).into(), None)
            }
            Dialog::StatusMessage { text, clear, saving } => {
                let input = field(tr("What are you up to?"), text, STATUS_TEXT)
                    .on_input(Message::StatusText)
                    .on_submit(Message::SaveStatusText);
                let chips = ClearAfter::ALL.into_iter().map(|choice| {
                    let on = *clear == Some(choice);
                    widgets::choice_chip(choice.label(), None, on, Some(Message::StatusClear(Some(choice))))
                });
                let chips = row(chips).spacing(space::s()).wrap();
                let clear = column![typo::text(typo::caption(), tr("Clear after")).style(theme::text_3), chips]
                    .spacing(space::xs());
                // The dialog's one primary button (the window has none of its own).
                let save = if *saving {
                    widgets::busy_button(tr("Saving…"), style::button::primary)
                } else {
                    widgets::text_button(None, tr("Save"), style::button::primary, Some(Message::SaveStatusText))
                };
                (tr("Set status message"), column![input, clear].spacing(space::l()).into(), Some(save))
            }
        };
        let mut buttons = row![Space::new().width(Fill), cancel].spacing(space::s()).align_y(Alignment::Center);
        if let Some(confirm) = confirm {
            buttons = buttons.push(confirm);
        }
        let card = column![typo::text(typo::title(), title), content, buttons].spacing(space::l());
        container(card).width(Fill).padding(layout::panel_padding()).style(style::container::popover).into()
    }

    // -- Mini and strip -------------------------------------------------------------------------

    /// The glance of mini and strip tiles: how many are there, how many chats are unread; the strip
    /// adds the faces of those who are there.
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        if !self.people_loaded {
            return container(widgets::loading()).width(Fill).height(Fill).into();
        }
        // Only who is online: away, busy and "do not disturb" are there, but not "online".
        let online = people::online(&self.people);
        let unread = people::unread_conversations(&self.conversations);
        let figure = typo::text(typo::display().tabular(), online.to_string()).wrapping(Wrapping::None);
        let label = typo::text(typo::meta(), if online == 1 { tr("person online") } else { tr("people online") })
            .style(theme::text_2)
            .wrapping(Wrapping::None);
        let unread_line = if unread == 0 {
            tr("No unread chats").to_owned()
        } else if unread == 1 {
            tr("1 unread chat").to_owned()
        } else {
            trf("{count} unread chats", &[("count", &unread)])
        };
        let unread_line = widgets::ellipsized(typo::meta(), unread_line, theme::text_3);
        let counts =
            column![row![figure, label].spacing(space::s()).align_y(Alignment::End), unread_line].spacing(space::xs());
        // Mini tall and the strip have room for the unread chats themselves; plain mini does not.
        let room = class == SizeClass::Strip || self.size.height >= size::panel();
        let chats: Vec<Element<'_, Message>> = people::unread_chats(&self.conversations)
            .into_iter()
            .take(if room { GLANCE_CHATS } else { 0 })
            .map(|chat| {
                let name = widgets::ellipsized(typo::body(), chat.name.as_str(), theme::text);
                let line = row![container(name).width(Fill), count_badge(chat.unread)].align_y(Alignment::Center);
                container(line).height(size::row_sidebar()).center_y(size::row_sidebar()).into()
            })
            .collect();
        let chats = column(chats).spacing(space::xs());
        let content: Element<'_, Message> = if class == SizeClass::Strip {
            // The faces of those who are there, each with the mark of its status, as in the list.
            let colours = self.colours();
            let there: Vec<&Person> =
                self.people.iter().filter(|person| !person.unknown && person.status.is_there()).collect();
            let mut faces = row(there
                .iter()
                .take(STRIP_FACES)
                .map(|person| avatar(&person.name, &person.id, Some(Group::of(person).status()), &colours)))
            .spacing(space::xs())
            .align_y(Alignment::Center);
            let more = people::there(&self.people).saturating_sub(STRIP_FACES);
            if more > 0 {
                let label = trf("+{count}", &[("count", &more)]);
                faces = faces.push(typo::text(typo::meta().tabular(), label).style(theme::text_3));
            }
            row![counts, container(chats).width(size::props()), Space::new().width(Fill), faces]
                .spacing(space::xl())
                .align_y(Alignment::Center)
                .into()
        } else {
            column![counts, chats].spacing(space::l()).into()
        };
        let pad = if class == SizeClass::Strip { [space::s(), space::m()] } else { [space::m(), space::m()] };
        container(content).width(Fill).height(Fill).padding(pad).center_y(Fill).into()
    }
}

/// A group's face, as large as a person's ([`face_edge`]): its icon on a neutral tile.
pub(crate) fn tile_face<'a>(which: Icon) -> Element<'a, Message> {
    let size = if face_edge() >= size::tile_lg() { TileSize::Large } else { TileSize::Medium };
    widgets::source_tile(TileFace::Icon(which), size, None)
}

/// The edge of a face: [`size::tile_lg()`] while a row leaves it [`space::m()`] of air, else
/// [`size::tile()`] – in Dense (rows 42) a face of 36 would touch the next row's.
pub(crate) fn face_edge() -> f32 {
    if size::row() >= size::tile_lg() + space::m() { size::tile_lg() } else { size::tile() }
}

/// The colour of a person's avatar: the sender colour of their id, the same everywhere.
pub(crate) fn hue(colours: &Palette, id: &str) -> iced::Color {
    colours.sender.get(widgets::sender_hue(id)).copied().unwrap_or(colours.text_3)
}

/// A face, [`face_edge`] across: the initials in `text` on the person's colour, tinted as the
/// faces of [`widgets::avatar_stack`] are (never the full colour as a surface), with the presence mark
/// of `status` over its lower right edge, where it covers no letter. The small face of Dense has
/// room for one letter only; a second would run under the mark.
pub(crate) fn avatar<'a>(name: &str, id: &str, status: Option<Status>, colours: &Palette) -> Element<'a, Message> {
    let edge = face_edge();
    let round = edge / 2.0;
    let colour = hue(colours, id);
    let mut letters = people::initials(name);
    if edge < size::tile_lg() {
        letters = letters.chars().take(1).collect();
    }
    let face = container(typo::text(typo::caption(), letters).wrapping(Wrapping::None)).center(edge).style(
        move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.tinted(colour))),
                text_color: Some(p.text),
                border: border::rounded(round),
                ..container::Style::default()
            }
        },
    );
    let Some(status) = status else { return face.into() };
    // The mark reaches past the face by its ring, as in the draft.
    let room = edge + MARK_RING;
    let mark = container(presence(status)).width(Fill).height(Fill).align_right(Fill).align_bottom(Fill);
    container(stack![face, mark]).width(room).height(room).into()
}

/// The presence mark of `status` ([`presence_mark`]) with its word as the tooltip.
pub(crate) fn presence<'a>(status: Status) -> Element<'a, Message> {
    widgets::with_tooltip(presence_mark(status), people::status_word(status))
}

/// The presence mark of `status`, [`icon::SIZE_SM`] across with a ring in the panel's colour: a dot
/// (online), the moon (away), half a circle (busy), a dot with a bar (do not disturb), a ring
/// (offline, invisible). The same form in the list, the head, the footer and the status menu.
pub(crate) fn presence_mark<'a>(status: Status) -> Element<'a, Message> {
    let edge = icon::SIZE_SM;
    let round = edge / 2.0;
    let inner = edge - MARK_RING;
    let surface = move |fill: Option<fn(&Palette) -> iced::Color>, ring: fn(&Palette) -> iced::Color| {
        move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(fill.map_or(p.panel, |fill| fill(&p)))),
                border: border::rounded(round).color(ring(&p)).width(MARK_RING),
                ..container::Style::default()
            }
        }
    };
    let content: Element<'a, Message> = match status {
        Status::Online => Space::new().into(),
        Status::Away => tinted(Icon::Moon, inner, |p| p.attention),
        Status::Busy => tinted(Icon::Contrast, inner, |p| p.error),
        Status::Dnd => {
            let bar_width = inner / 2.0;
            container(Space::new().width(bar_width).height(MARK_RING))
                .style(|t: &Theme| container::Style {
                    background: Some(Background::Color(palette(t).panel)),
                    ..container::Style::default()
                })
                .into()
        }
        Status::Invisible | Status::Offline => Space::new().into(),
    };
    let style = match status {
        Status::Online => surface(Some(|p| p.ok), |p| p.panel),
        Status::Dnd => surface(Some(|p| p.error), |p| p.panel),
        Status::Away | Status::Busy => surface(None, |p| p.panel),
        Status::Invisible | Status::Offline => surface(None, |p| p.text_3),
    };
    container(content).center(edge).style(style).into()
}

/// One status in the menu of the own status: the mark of [`presence_mark`] in a cell of
/// [`icon::SIZE`], so every label starts on one axis, the word and an optional second line; the
/// current one is chosen (`menu_item(true)`) with a check at its end, not a dot that reads as a mark.
fn status_entry<'a>(status: Status, current: bool, detail: Option<&'a str>) -> Element<'a, Message> {
    let mark = container(presence_mark(status)).center(icon::SIZE);
    let mut words = column![typo::text(typo::body(), people::status_word(status)).wrapping(Wrapping::None)];
    let mut height = style::MENU_ITEM_HEIGHT;
    if let Some(detail) = detail {
        words = words.push(typo::text(typo::meta(), detail).style(theme::text_3).wrapping(Wrapping::None));
        height += typo::line_height(typo::meta());
    }
    let mut content =
        row![mark, container(words).width(Fill).clip(true)].spacing(space::s()).align_y(Alignment::Center);
    if current {
        content = content.push(tinted(Icon::Check, icon::SIZE, |p| p.text_2));
    }
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(height)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(current))
        .on_press(Message::SetStatus(status))
        .into()
}

/// The head of a group of people: its title and the chevron in `caption`/`text_3` as a sidebar
/// group has them, the number of people as a cell of its own at the end, in `meta`, tabular, on
/// the axis of the rows' unread counts. A click folds or unfolds the group.
fn group_head<'a>(title: &'a str, count: usize, open: bool, on_press: Message) -> Element<'a, Message> {
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    let content = row![
        typo::text(typo::caption(), title).style(theme::text_3).wrapping(Wrapping::None),
        tinted(chevron, icon::SIZE_SM, |p| p.text_3),
        Space::new().width(Fill),
        typo::text(typo::meta().tabular(), count.to_string()).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    let bar = container(content)
        .width(Fill)
        .height(size::row_sidebar())
        .padding(style::ITEM_PADDING)
        .center_y(size::row_sidebar());
    let bar = mouse_area(bar).on_press(on_press).interaction(iced::mouse::Interaction::Pointer);
    container(bar).padding(Padding::ZERO.top(space::m())).into()
}

/// The tabs of the contact list over the column's whole width: the frame and segments of
/// [`widgets::segmented`] (its styles), each segment an equal share with its label in the middle.
fn segmented_fill<'a>(options: impl IntoIterator<Item = (String, Message)>, active: usize) -> Element<'a, Message> {
    let inner = size::control_sm() - (style::SEGMENT_INSET + HAIRLINE) * 2.0;
    let segments = options.into_iter().enumerate().map(|(at, (label, message))| {
        let label = typo::text(typo::caption(), label).wrapping(Wrapping::None);
        button(container(label).center(Fill))
            .width(Fill)
            .height(inner)
            .padding(style::button_padding())
            .style(style::button::segment(at == active))
            .on_press(message)
            .into()
    });
    container(row(segments.collect::<Vec<_>>()).width(Fill))
        .width(Fill)
        .padding(style::SEGMENT_INSET)
        .style(style::container::segmented)
        .into()
}

/// The unread count of a row: a small round badge, the figure tabular.
fn count_badge<'a>(count: u64) -> Element<'a, Message> {
    let figure = if count > 99 { "99+".to_owned() } else { count.to_string() };
    let edge = size::tile_sm();
    let round = edge / 2.0;
    // 6 at the sides: the gap the design allows inside small pieces (torro-design, "Abstände").
    let side = space::xs() + space::xs() / 2.0;
    container(typo::text(typo::caption().tabular(), figure).wrapping(Wrapping::None))
        .height(edge)
        .padding([0.0, side])
        .center_y(edge)
        .style(move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.selected)),
                text_color: Some(p.text),
                border: border::rounded(round),
                ..container::Style::default()
            }
        })
        .into()
}

/// "2 unread messages", the badge's tooltip.
fn unread_words(count: u64) -> String {
    if count == 1 { tr("1 unread message").to_owned() } else { trf("{count} unread messages", &[("count", &count)]) }
}

/// One person found in the dialog "New chat": a click starts the direct chat.
fn found_row<'a>(person: &Person, colours: &Palette) -> Element<'a, Message> {
    // With the mark of their status, as in the list: the server's search tells it.
    let face = avatar(&person.name, &person.id, Some(Group::of(person).status()), colours);
    let content = row![face, widgets::ellipsized(typo::body(), person.name.clone(), theme::text)]
        .spacing(space::m())
        .align_y(Alignment::Center);
    button(content)
        .width(Fill)
        .height(size::row_sidebar() + space::s())
        .padding(style::ITEM_PADDING)
        .style(style::button::sidebar_item(false))
        .on_press(Message::StartWith(person.clone()))
        .into()
}

/// A quiet line in a dialog: what to do, or what happens.
fn hint<'a>(text: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
}

/// The list without anyone in it: why, and the way to change it.
fn no_people<'a>(why: String, action: Option<(&'a str, Message)>) -> Element<'a, Message> {
    let mut content = column![typo::text(typo::body(), why).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)]
        .spacing(space::m())
        .align_x(Alignment::Center);
    if let Some((label, message)) = action {
        content = content.push(widgets::text_button(None, label, style::button::secondary, Some(message)));
    }
    container(content).width(Fill).padding([space::xl(), space::l()]).center_x(Fill).into()
}

/// An icon in a colour of the palette.
pub(crate) fn tinted<'a>(which: Icon, edge: f32, colour: fn(&Palette) -> iced::Color) -> Element<'a, Message> {
    icon::icon(which, edge).style(move |t, _| iced::widget::svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// The name of a conversation's kind for a group chat's head.
pub(crate) fn kind_words(conversation: &Conversation) -> &'static str {
    match conversation.kind {
        torro_chat::ConversationKind::Group => tr("Group chat"),
        torro_chat::ConversationKind::Public => tr("Public conversation"),
        torro_chat::ConversationKind::NoteToSelf => tr("Note to self"),
        torro_chat::ConversationKind::Direct | torro_chat::ConversationKind::Other => tr("Conversation"),
    }
}
