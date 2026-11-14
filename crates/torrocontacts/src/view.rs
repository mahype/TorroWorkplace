//! How the contacts window looks (design skill `torro-design`, draft D2 in `docs/prototypes/kontakte/`):
//! the sidebar, the header with "Group by", "…" (import, export) and "New contact", the search over the
//! grouped list, the contact in its panel, the menus and the dialog "Delete contact?". The view
//! "Duplicates" and its dialog are [`crate::duplicates`], the dialog "Import vCard" [`crate::transfer`].
//!
//! Nothing here changes state: every control sends a [`Message`] that [`App::update`] handles.
//!
//! - **Wide:** sidebar (or rail), list, the contact beside it.
//! - **Medium:** the rail, the town drops out of the rows, the contact lies over the list.
//! - **Narrow:** the navigation in a menu at the top left, rows with name and position only, the
//!   contact covers the window.
//! - **Mini and strip:** the glance ([`crate::glance`]).
//!
//! Before the first load the window shows "Loading", after a failed one the error with "Retry", and
//! without an account with an address book "Connect an account in Torro Workplace".

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, scrollable, stack, text_input};
use iced::{Alignment, Element, Fill, Padding, Rectangle, Size};
use torro_contacts::FieldKind;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::panel;
use torro_gui::shell::{self, Cover, SidebarMode, Surface, TitleButtons, TopRow};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{dialog, layout, style, typo};

use crate::app::{APP, App, Detail, Dialog, Menu, Message, SEARCH, View};
use crate::contact;
use crate::edit::{self, List};
use crate::list::{self, Columns, GroupBy, Key, RowLook, tinted};

/// The anchors of the menus that float next to their triggers; new in every `view`.
#[derive(Default)]
pub struct Anchors {
    pub group_by: Anchor,
    pub header: Anchor,
    pub import_book: Anchor,
    pub nav: Anchor,
    pub more: Anchor,
    pub row: Anchor,
    pub form: edit::Anchors,
}

impl App {
    /// The window, with the zoom badge and the notice over it.
    pub fn view(&self) -> Element<'_, Message> {
        let class = self.class();
        if matches!(class, SizeClass::Mini | SizeClass::Strip) {
            let glance = match (self.loaded, &self.error) {
                (false, _) => widgets::loading(),
                (true, Some(err)) => crate::glance::failed(err),
                (true, None) if self.books.is_empty() => crate::glance::no_account(),
                (true, None) => match class {
                    SizeClass::Strip => crate::glance::strip(self),
                    _ => crate::glance::mini(self),
                },
            };
            return self.banded(glance, Surface::of(class));
        }
        if !self.loaded {
            return self.banded(widgets::loading(), Surface::Window);
        }
        if let Some(err) = &self.error {
            return self.banded(
                widgets::error_state(trf("Contacts could not be loaded: {error}", &[("error", err)]), Message::Retry),
                Surface::Window,
            );
        }
        if self.books.is_empty() {
            return self.banded(no_account(), Surface::Window);
        }
        let anchors = Anchors::default();
        let window = self.window(class, &anchors);
        let window = match (self.dialog, &self.import) {
            (Some(Dialog::Delete), _) => {
                widgets::modal(window, self.delete_dialog(), size::form_max(), Message::CloseDialog)
            }
            (Some(Dialog::Import), Some(import)) => {
                let books = self.writable_books();
                let dialog = crate::transfer::dialog(import, &books, &anchors.import_book);
                widgets::modal(window, dialog, size::form_max(), Message::CloseDialog)
            }
            (Some(Dialog::Merge), _) => {
                widgets::modal(window, self.merge_dialog(), size::form_max(), Message::CloseDialog)
            }
            (Some(Dialog::Import), None) | (None, _) => window,
        };
        // The books of the import open over its dialog, not under it.
        let window = match (&self.menu, self.dialog) {
            (Some(Menu::ImportBook), Some(Dialog::Import)) => match self.import_book_menu(&anchors) {
                Some(menu) => stack![window, menu].width(Fill).height(Fill).into(),
                None => window,
            },
            _ => window,
        };
        widgets::ctrl_guard(self.zoom.view(self.with_notice(window)))
    }

    /// `content` under the band of the window's own title bar (macOS), for the views without a top row.
    fn banded<'a>(&'a self, content: Element<'a, Message>, surface: Surface) -> Element<'a, Message> {
        let content = self.zoom.view(self.with_notice(content));
        widgets::ctrl_guard(shell::titlebar(
            content,
            self.zoom.factor(),
            surface,
            Cover::Nothing,
            TitleButtons::none(),
            Message::DragWindow,
        ))
    }

    /// `window` with the notice at the bottom while it shows; always two layers, so the window keeps its
    /// state when the notice comes and goes.
    fn with_notice<'a>(&'a self, window: Element<'a, Message>) -> Element<'a, Message> {
        let notice: Element<'a, Message> = match &self.notice {
            Some(notice) => widgets::toast(notice.text.as_str(), None),
            None => Space::new().into(),
        };
        stack![window, notice].width(Fill).height(Fill).into()
    }

    /// What lies over the top of the window, which the top row repeats.
    fn cover(&self, class: SizeClass) -> Cover {
        Cover::dialog(self.dialog.is_some())
            .or(Cover::panel(class, self.detail.is_some()))
            .or(Cover::drawer(class, self.drawer))
    }

    fn window<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Element<'a, Message> {
        let sidebar = self.sidebar_view(SidebarMode::Expanded);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar_view(SidebarMode::Rail));
        let header = self.header(class, anchors);
        let detail = self.panel(class, anchors);
        let placed = panel::place(class, self.list_view(anchors), detail, self.menu_layer(anchors));
        let side = self.side.width(shell::sidebar_limits(self.content_size().width, torro_gui::split::min_pane()));
        let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow)
            .title(APP)
            .sidebar(label, Message::ToggleSidebar)
            .sidebar_at_edge(true)
            .cover(self.cover(class));
        shell::shell(class, sidebar, header, placed.body, None)
            .sidebar_width(side, self.side.dragging, Message::SidebarWidth)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .top_row(top)
            .into()
    }

    // -- Sidebar --------------------------------------------------------------------------------

    /// The name of a view.
    pub fn view_name(&self, view: &View) -> String {
        match view {
            View::All => tr("All contacts").to_owned(),
            View::Favorites => tr("Favorites").to_owned(),
            View::Recent => tr("Recent").to_owned(),
            View::Birthdays => tr("Birthdays").to_owned(),
            View::Duplicates => tr("Duplicates").to_owned(),
            View::Book(id) => self.book(id).map(|b| b.display_name.clone()).unwrap_or_default(),
            View::Group(_, name) => name.clone(),
        }
    }

    fn view_icon(&self, view: &View) -> Icon {
        match view {
            View::All => Icon::Users,
            View::Favorites => Icon::Star,
            View::Recent => Icon::History,
            View::Birthdays => Icon::Cake,
            View::Duplicates => Icon::Copy,
            View::Book(id) => match self.book(id) {
                Some(book) if !book.writable => Icon::Lock,
                _ => Icon::BookUser,
            },
            View::Group(..) => Icon::Tag,
        }
    }

    /// The sidebar drawn for `mode`: the views, then the books (by account from two accounts on) with
    /// their groups.
    fn sidebar_view(&self, mode: SidebarMode) -> Element<'_, Message> {
        let item = |view: View, trail: Trail| {
            let active = self.view == view;
            widgets::sidebar_item_in(
                mode,
                self.view_icon(&view),
                self.view_name(&view),
                trail,
                active,
                Message::Show(view),
            )
        };
        let favorites = self.contacts.iter().filter(|c| c.favorite).count();
        let soon = self.soon();
        let mut items = vec![
            item(View::All, Trail::Count(self.contacts.len())),
            item(View::Favorites, if favorites > 0 { Trail::Count(favorites) } else { Trail::None }),
            item(View::Recent, Trail::None),
            item(View::Birthdays, if soon > 0 { Trail::Count(soon) } else { Trail::None }),
            item(
                View::Duplicates,
                if self.duplicates.is_empty() { Trail::None } else { Trail::Count(self.duplicates.len()) },
            ),
        ];
        let mut accounts: Vec<&str> = Vec::new();
        for book in &self.books {
            if !accounts.contains(&book.account.as_str()) {
                accounts.push(&book.account);
            }
        }
        let line = palette(&self.theme()).line;
        for account in &accounts {
            let title = if accounts.len() >= 2 { account_name(account) } else { tr("Address books").to_owned() };
            items.push(widgets::sidebar_group_in(mode, title, None));
            for book in self.books.iter().filter(|b| b.account == *account) {
                let count = self.contacts.iter().filter(|c| c.book == book.id).count();
                items.push(item(View::Book(book.id.clone()), Trail::Count(count)));
                let groups = self.groups_of(&book.id);
                if mode == SidebarMode::Expanded && !groups.is_empty() {
                    let children = groups
                        .into_iter()
                        .map(|name| {
                            let count = self
                                .contacts
                                .iter()
                                .filter(|c| {
                                    c.book == book.id && c.groups.iter().any(|g| crate::app::same_group(g, &name))
                                })
                                .count();
                            let view = View::Group(book.id.clone(), name.clone());
                            let active = self.view == view;
                            widgets::sidebar_child(Icon::Tag, name, Trail::Count(count), active, Message::Show(view))
                        })
                        .collect();
                    items.push(widgets::sidebar_branch(line, children));
                }
            }
        }
        shell::sidebar(shell::sidebar_tools(mode, false, Message::Switcher), items, vec![])
    }

    // -- Header ---------------------------------------------------------------------------------

    fn header<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Element<'a, Message> {
        let narrow = !matches!(class, SizeClass::Wide | SizeClass::Medium);
        let count = match &self.view {
            View::Birthdays => self.shown_birthdays().len(),
            View::Duplicates => self.duplicates.len(),
            _ => self.shown().len(),
        };
        let lead: Element<'a, Message> = if narrow {
            widgets::anchored(
                &anchors.nav,
                widgets::menu_button(
                    &anchors.nav,
                    Some(self.view_icon(&self.view)),
                    self.view_name(&self.view),
                    Message::ToggleMenu(Menu::Nav),
                ),
            )
        } else {
            shell::header_title(Some(self.view_icon(&self.view)), self.view_name(&self.view), Some(count.to_string()))
        };
        let mut trailing: Vec<Element<'a, Message>> = Vec::new();
        if !matches!(self.view, View::Recent | View::Birthdays | View::Duplicates) && !self.contacts.is_empty() {
            let toggle = Message::ToggleMenu(Menu::GroupBy);
            // Narrow, the grouping moves into the navigation menu, so the navigation and "+" keep their room.
            if !narrow {
                let button = widgets::select_button(&anchors.group_by, self.group_by.by_label(), toggle);
                trailing.push(widgets::anchored(&anchors.group_by, button));
            }
        }
        // Import and export (issue #725); narrow, they move into the navigation menu like "Group by".
        if !narrow {
            trailing.push(widgets::anchored(
                &anchors.header,
                widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(Menu::Header))),
            ));
        }
        // Off while a contact is edited: its input is never dropped for a new one.
        let new = self.new_book().filter(|_| !self.editing()).map(|_| Message::NewContact(None, None));
        trailing.push(if narrow {
            widgets::compact_button(Icon::Plus, tr("New contact"), style::button::primary, new)
        } else {
            widgets::text_button(Some(Icon::Plus), tr("New contact"), style::button::primary, new)
        });
        shell::header_bar([], lead, trailing)
    }

    // -- List -----------------------------------------------------------------------------------

    fn search_bar(&self) -> Element<'_, Message> {
        let body = typo::body();
        let lead = style::INPUT_PADDING.left + icon::SIZE + space::s();
        let trail = style::INPUT_PADDING.right + size::control_sm();
        let input = text_input(tr("Search name, company, phone, city…"), &self.search)
            .id(SEARCH)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(Padding { left: lead, right: trail, ..style::INPUT_PADDING })
            .width(Fill)
            .style(style::text_input_quiet)
            .on_input(Message::Search)
            .on_submit(Message::Key(crate::app::Press::Enter));
        let glass = container(tinted(Icon::Search, icon::SIZE, |p| p.text_3))
            .height(Fill)
            .center_y(Fill)
            .padding(Padding::ZERO.left(style::INPUT_PADDING.left));
        // At the right in the field: "Clear search" while something is typed, else the key that gets here.
        let end: Element<'_, Message> = if self.search.is_empty() {
            widgets::kbd("/")
        } else {
            widgets::icon_button(Icon::X, tr("Clear search"), Some(Message::Search(String::new())))
        };
        let end = container(end)
            .height(Fill)
            .width(Fill)
            .center_y(Fill)
            .align_right(Fill)
            .padding(Padding::ZERO.right(style::INPUT_PADDING.right));
        let field = stack![input, glass, end];
        let side = layout::row_padding();
        container(field)
            .padding(Padding { top: space::s(), bottom: space::s(), left: side.left - space::s(), right: side.right })
            .width(Fill)
            .into()
    }

    fn list_view<'a>(&'a self, anchors: &Anchors) -> Element<'a, Message> {
        // The duplicates are cards to decide on, not a list to search.
        if self.view == View::Duplicates {
            return self.duplicates_view();
        }
        let body: Element<'a, Message> = if self.contacts.is_empty() {
            let new = self.new_book().map(|_| {
                widgets::text_button(
                    Some(Icon::Plus),
                    tr("New contact"),
                    style::button::secondary,
                    (!self.editing()).then_some(Message::NewContact(None, None)),
                )
            });
            calm(Some(Icon::Users), tr("No contacts yet."), tr("Contacts of your address books show up here."), new)
        } else {
            let rows = self.rows_view(anchors);
            if rows.is_empty() {
                let (title, hint) = match (&self.view, self.search.trim().is_empty()) {
                    (_, false) => (tr("No contact matches."), tr("Try another name, number or town.")),
                    (View::Recent, true) => {
                        (tr("No contacts used yet."), tr("Contacts you open, mail or call show up here."))
                    }
                    (View::Favorites, true) => (tr("No favorites yet."), tr("Mark a contact as favorite in its menu.")),
                    (View::Birthdays, true) => {
                        (tr("No birthdays in the next year."), tr("Add a birthday to a contact to see it here."))
                    }
                    _ => (tr("No contacts here."), tr("Add one with New contact.")),
                };
                calm(None, title, hint, None)
            } else {
                scrollable(column(rows))
                    .height(Fill)
                    .style(style::scrollable)
                    .direction(scrollable::Direction::Vertical(style::scrollbar()))
                    .into()
            }
        };
        column![self.search_bar(), container(body).width(Fill).height(Fill).center_x(Fill)].height(Fill).into()
    }

    /// The heads and rows of the view.
    fn rows_view<'a>(&'a self, anchors: &Anchors) -> Vec<Element<'a, Message>> {
        let width = self.list_width();
        let columns = Columns::at(width);
        let colours = palette(&self.theme());
        let row_menu = match &self.menu {
            Some(Menu::Row(id, _)) => Some(id.as_str()),
            _ => None,
        };
        let open = self.detail.as_ref().and_then(|d| d.id.as_deref());
        let look = |contact: &torro_contacts::ContactSummary, end: list::End| RowLook {
            columns,
            selected: open == Some(contact.id.as_str())
                || (open.is_none() && self.cursor.as_deref() == Some(contact.id.as_str())),
            hovered: self.hover.as_deref() == Some(contact.id.as_str())
                || self.cursor.as_deref() == Some(contact.id.as_str()),
            picture: self.photos.get(&contact.id).and_then(|(_, picture)| picture.as_ref()),
            colours,
            end,
            width,
            anchor: (row_menu == Some(contact.id.as_str())).then(|| anchors.row.clone()),
        };
        let mut rows = Vec::new();
        match &self.view {
            View::Recent => {
                for contact in self.shown() {
                    rows.push(list::contact_row(contact, look(contact, list::End::Town)));
                }
            }
            View::Birthdays => {
                let birthdays = self.shown_birthdays();
                for (key, label, members) in list::birthday_heads(&birthdays) {
                    let folded = self.folded(&key);
                    rows.push(list::head(Icon::Cake, label, members.len(), folded, Message::Fold(key), None));
                    if folded {
                        continue;
                    }
                    for birthday in members {
                        let Some(contact) = self.summary(&birthday.contact.id) else { continue };
                        let end = list::birthday_end(birthday, self.today);
                        rows.push(list::contact_row(contact, look(contact, end)));
                    }
                }
            }
            _ => {
                let shown = self.shown();
                let by = self.group_by;
                for head in list::heads(&shown, by, &self.books) {
                    let folded = self.folded(&head.key);
                    let on_add = self.add_here(&head.key);
                    rows.push(list::head(
                        by.icon(),
                        head.label.clone(),
                        head.members.len(),
                        folded,
                        Message::Fold(head.key.clone()),
                        on_add,
                    ));
                    if folded {
                        continue;
                    }
                    for contact in head.members {
                        rows.push(list::contact_row(contact, look(contact, list::End::Town)));
                    }
                }
            }
        }
        rows
    }

    /// The "+" of a head: a new contact in that group or book; none by town, and none where no book may
    /// be written.
    fn add_here(&self, key: &Key) -> Option<Message> {
        if self.editing() {
            return None;
        }
        match key {
            Key::Group(name) => {
                self.book_of_group(name).map(|book| Message::NewContact(Some(book), Some(name.clone())))
            }
            Key::NoGroup => self.new_book().map(|_| Message::NewContact(None, None)),
            Key::Book(id) => {
                self.book(id).filter(|b| b.writable).map(|b| Message::NewContact(Some(b.id.clone()), None))
            }
            Key::City(_) | Key::NoCity | Key::Month(..) => None,
        }
    }

    // -- Panel ----------------------------------------------------------------------------------

    fn panel<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Option<panel::Panel<'a, Message>> {
        let detail = self.detail.as_ref()?;
        let colours = palette(&self.theme());
        let book_id = match (&detail.contact, &detail.new_in) {
            (Some(contact), _) => Some(contact.book.as_str()),
            (None, Some((book, _))) => Some(book.as_str()),
            (None, None) => None,
        };
        let book = book_id.and_then(|id| self.book(id));
        let path = contact::path(book);
        let picture = match (&detail.contact, &self.large) {
            (Some(contact), Some((id, _, picture))) if *id == contact.id => Some(picture),
            (Some(contact), _) => self.photos.get(&contact.id).and_then(|(_, p)| p.as_ref()),
            _ => None,
        };
        let built = match (&detail.draft, &detail.contact) {
            (Some(draft), contact) => {
                let title: Element<'a, Message> = match contact {
                    Some(contact) => contact::title(contact, &self.look(detail, picture, colours)),
                    None => panel::title(tr("New contact")),
                };
                let open = match &self.menu {
                    Some(Menu::Kind(list, index)) => Some(edit::Open::Kind(*list, *index)),
                    Some(Menu::AddField) => Some(edit::Open::Add),
                    _ => None,
                };
                let mut body = column![].spacing(space::l());
                if let Some(contact) = contact {
                    body = body.push(contact::picture_buttons(contact.photo.is_some(), detail.busy));
                }
                body = body.push(edit::form(draft, &self.note, &anchors.form, open));
                let done: Element<'a, Message> = if draft.saving {
                    widgets::busy_button(tr("Saving…"), style::button::secondary)
                } else {
                    widgets::text_button(Some(Icon::Check), tr("Done"), style::button::secondary, Some(Message::Done))
                };
                panel::panel(path, title, body, Message::Close).footer(vec![
                    widgets::text_button(
                        None,
                        tr("Cancel"),
                        style::button::ghost,
                        (!draft.saving).then_some(Message::Cancel),
                    ),
                    Space::new().width(Fill).into(),
                    done,
                ])
            }
            (None, Some(contact)) => {
                let look = self.look(detail, picture, colours);
                let title = contact::title(contact, &look);
                let body = contact::body(contact, look);
                let mut actions = Vec::new();
                let writable = !contact.readonly && contact.state != torro_contacts::ContactState::Conflict;
                if writable {
                    actions.push(widgets::with_tooltip(
                        widgets::icon_button(Icon::FilePenLine, tr("Edit"), Some(Message::Edit)),
                        tr("Edit (Ctrl+E)"),
                    ));
                }
                actions.push(widgets::anchored(
                    &anchors.more,
                    widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(Menu::More))),
                ));
                panel::panel(path, title, body, Message::Close).actions(actions)
            }
            (None, None) => {
                let body: Element<'a, Message> = match &detail.error {
                    Some(err) => widgets::error_state(err.as_str(), Message::Refetch),
                    None => widgets::loading(),
                };
                panel::panel(path, Space::new(), body, Message::Close)
            }
        };
        let total = self.list_room();
        let width = self.panel_split.width(panel::beside_limits(total));
        let _ = class;
        Some(built.resizable(width, total, self.panel_split.dragging, Message::PanelWidth))
    }

    fn look<'a>(
        &'a self,
        detail: &'a Detail,
        picture: Option<&'a iced::widget::image::Handle>,
        colours: theme::Palette,
    ) -> contact::Look<'a> {
        // The book's groups as `contacts.list_groups` names them, else those of the loaded contacts.
        let (known, on) = match &detail.contact {
            Some(contact) => {
                let known = match &detail.known {
                    Some((book, names)) if *book == contact.book => names.clone(),
                    _ => self.groups_of(&contact.book),
                };
                (known, contact.groups.clone())
            }
            None => (Vec::new(), Vec::new()),
        };
        contact::Look {
            picture,
            colours,
            today: self.today,
            group_text: &detail.group_text,
            suggestions: contact::suggestions(&known, &on, &detail.group_text),
            busy: detail.busy,
            error: detail.error.as_deref(),
        }
    }

    // -- Menus and the dialog -------------------------------------------------------------------

    fn menu_layer<'a>(&'a self, anchors: &Anchors) -> Option<Element<'a, Message>> {
        self.menu_layer_of(self.menu.as_ref()?, anchors)
    }

    fn menu_layer_of<'a>(&'a self, menu: &Menu, anchors: &Anchors) -> Option<Element<'a, Message>> {
        let (anchor, placement, sections) = match menu {
            // Narrow, "Group by" lives in the navigation menu.
            Menu::GroupBy if !matches!(self.class(), SizeClass::Wide | SizeClass::Medium) => {
                return self.menu_layer_of(&Menu::Nav, anchors);
            }
            Menu::GroupBy => {
                let items = GroupBy::ALL
                    .into_iter()
                    .map(|by| {
                        MenuItem::new(Mark::Icon(by.icon()), by.label(), Message::GroupBy(by))
                            .current(by == self.group_by)
                    })
                    .collect();
                (
                    anchors.group_by.clone(),
                    Placement::Below(Align::End),
                    vec![MenuSection::titled(tr("Group by"), items)],
                )
            }
            Menu::Nav => {
                let mut views = vec![View::All, View::Favorites, View::Recent, View::Birthdays, View::Duplicates];
                // The books with their groups, as the sidebar has them.
                let mut nested = vec![false; views.len()];
                for book in &self.books {
                    views.push(View::Book(book.id.clone()));
                    nested.push(false);
                    for name in self.groups_of(&book.id) {
                        views.push(View::Group(book.id.clone(), name));
                        nested.push(true);
                    }
                }
                let items = views
                    .into_iter()
                    .zip(nested)
                    .map(|(view, nested)| {
                        let current = self.view == view;
                        let item = MenuItem::new(
                            Mark::Icon(self.view_icon(&view)),
                            self.view_name(&view),
                            Message::Show(view),
                        )
                        .current(current);
                        if nested { item.nested() } else { item }
                    })
                    .collect();
                let mut sections = vec![MenuSection::new(items)];
                if !matches!(self.view, View::Recent | View::Birthdays | View::Duplicates) && !self.contacts.is_empty()
                {
                    let by = GroupBy::ALL
                        .into_iter()
                        .map(|by| {
                            MenuItem::new(Mark::Icon(by.icon()), by.label(), Message::GroupBy(by))
                                .current(by == self.group_by)
                        })
                        .collect();
                    sections.push(MenuSection::titled(tr("Group by"), by));
                }
                sections.push(MenuSection::new(self.transfer_items()));
                (anchors.nav.clone(), Placement::Below(Align::Start), sections)
            }
            Menu::More => (anchors.more.clone(), Placement::Below(Align::End), self.more_menu()),
            // Narrow, import and export live in the navigation menu.
            Menu::Header if !matches!(self.class(), SizeClass::Wide | SizeClass::Medium) => {
                return self.menu_layer_of(&Menu::Nav, anchors);
            }
            Menu::Header => {
                (anchors.header.clone(), Placement::Below(Align::End), vec![MenuSection::new(self.transfer_items())])
            }
            // Drawn over the dialog ([`App::import_book_menu`]).
            Menu::ImportBook => return None,
            Menu::Move => {
                let here = self.open_contact().map(|c| c.book.clone()).unwrap_or_default();
                let items = self
                    .books
                    .iter()
                    .filter(|b| b.writable && b.id != here)
                    .map(|b| {
                        MenuItem::new(Mark::Icon(Icon::BookUser), b.display_name.clone(), Message::MoveTo(b.id.clone()))
                    })
                    .collect::<Vec<_>>();
                // The menu never stands empty: without a book to go to, it says so.
                let items = if items.is_empty() {
                    vec![MenuItem::plain(tr("No other writable book"), Message::CloseMenu)]
                } else {
                    items
                };
                (anchors.more.clone(), Placement::Below(Align::End), vec![MenuSection::titled(tr("Move to"), items)])
            }
            Menu::Kind(list, index) => {
                let items = list
                    .kinds()
                    .into_iter()
                    .map(|kind: FieldKind| {
                        MenuItem::plain(edit::kind_word(&kind), Message::Draft(edit::Msg::Kind(*list, *index, kind)))
                    })
                    .collect();
                (anchors.form.kind.clone(), Placement::Below(Align::Start), vec![MenuSection::new(items)])
            }
            Menu::AddField => {
                let items = List::ALL
                    .into_iter()
                    .map(|list| {
                        MenuItem::new(Mark::Icon(list.icon()), list.label(), Message::Draft(edit::Msg::Add(list)))
                    })
                    .collect();
                (anchors.form.add.clone(), Placement::Below(Align::Start), vec![MenuSection::new(items)])
            }
            Menu::Row(id, at) => {
                let contact = self.summary(id)?;
                let mut items = vec![MenuItem::new(Mark::Icon(Icon::User), tr("Open"), Message::Open(id.clone()))];
                // Only what can open: an address with "@", a number with digits.
                if contact.email.as_deref().and_then(contact::mail_link).is_some() {
                    items.push(MenuItem::new(Mark::Icon(Icon::Mail), tr("Mail"), Message::QuickMail(id.clone())));
                }
                if contact.phone.as_deref().and_then(contact::tel_link).is_some() {
                    items.push(MenuItem::new(Mark::Icon(Icon::Phone), tr("Call"), Message::QuickCall(id.clone())));
                }
                items.push(MenuItem::new(Mark::Icon(Icon::Copy), tr("Copy"), Message::QuickCopy(id.clone())));
                let anchor = anchors.row.area(Rectangle::new(*at, Size::ZERO));
                (anchor, Placement::At, vec![MenuSection::new(items)])
            }
        };
        Some(widgets::floating(&anchor, placement, widgets::menu(sections), Message::CloseMenu))
    }

    /// The "…" of the contact: copy its parts, favourite, move, delete.
    fn more_menu<'a>(&'a self) -> Vec<MenuSection<'a, Message>> {
        let Some(contact) = self.open_contact() else { return Vec::new() };
        let mut copy =
            vec![MenuItem::new(Mark::Icon(Icon::Copy), tr("Copy name"), Message::CopyText(contact.shown_name()))];
        if let Some(mail) = contact.emails.first() {
            copy.push(MenuItem::new(
                Mark::Icon(Icon::Mail),
                tr("Copy mail address"),
                Message::CopyText(mail.value.clone()),
            ));
        }
        if let Some(phone) = contact.phones.first() {
            copy.push(MenuItem::new(
                Mark::Icon(Icon::Phone),
                tr("Copy number"),
                Message::CopyText(phone.value.clone()),
            ));
        }
        let favorite = if contact.favorite { tr("Remove from favorites") } else { tr("Add to favorites") };
        let mut change = vec![MenuItem::new(Mark::Icon(Icon::Star), favorite, Message::ToggleFavorite)];
        let elsewhere = self.books.iter().any(|b| b.writable && b.id != contact.book);
        if !contact.readonly && elsewhere {
            change.push(MenuItem::new(Mark::Icon(Icon::BookUser), tr("Move to…"), Message::ToggleMenu(Menu::Move)));
        }
        let export = vec![MenuItem::new(Mark::Icon(Icon::Download), tr("Export vCard…"), Message::ExportContact)];
        let mut sections = vec![MenuSection::new(copy), MenuSection::new(export), MenuSection::new(change)];
        if !contact.readonly {
            sections.push(MenuSection::new(vec![MenuItem::new(
                Mark::Icon(Icon::Trash2),
                tr("Delete…"),
                Message::AskDelete,
            )]));
        }
        sections
    }

    /// "Import vCard…" and, when the view shows someone, "Export…" (issue #725).
    fn transfer_items<'a>(&self) -> Vec<MenuItem<'a, Message>> {
        let mut items = vec![MenuItem::new(Mark::Icon(Icon::Upload), tr("Import vCard…"), Message::ImportVcard)];
        if self.can_export() {
            items.push(MenuItem::new(Mark::Icon(Icon::Download), tr("Export…"), Message::Export));
        }
        items
    }

    /// The writable books for the dialog "Import vCard", the chosen one marked.
    fn import_book_menu<'a>(&'a self, anchors: &Anchors) -> Option<Element<'a, Message>> {
        let chosen = self.import.as_ref()?.book.as_str();
        let items = self
            .writable_books()
            .into_iter()
            .map(|b| MenuItem::plain(b.display_name.clone(), Message::ImportBook(b.id.clone())).current(b.id == chosen))
            .collect();
        let menu = widgets::menu(vec![MenuSection::new(items)]);
        Some(widgets::floating(&anchors.import_book, Placement::Below(Align::Start), menu, Message::CloseMenu))
    }

    fn delete_dialog(&self) -> Element<'_, Message> {
        let name = self.open_contact().map(|c| c.shown_name()).unwrap_or_default();
        let head = typo::text(typo::title(), trf("Delete {name}?", &[("name", &name)])).wrapping(Wrapping::WordOrGlyph);
        let body = typo::text(
            typo::body(),
            tr("The contact is deleted from its address book on the server. This cannot be undone."),
        )
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph);
        let foot = row![
            Space::new().width(Fill),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseDialog)),
            widgets::text_button(
                Some(Icon::Trash2),
                tr("Delete contact"),
                style::button::danger,
                Some(Message::Delete)
            ),
        ]
        .spacing(space::s());
        container(dialog::frame(head, body, foot, space::l()))
            .width(Fill)
            .padding(layout::panel_padding())
            .style(style::container::popover)
            .into()
    }
}

/// An account as the sidebar names it: the part after the provider (`nextcloud:anna@cloud.example.org`
/// becomes `anna@cloud.example.org`).
pub fn account_name(account: &str) -> String {
    account.split_once(':').map_or(account, |(_, rest)| rest).to_owned()
}

/// Without an account with an address book: what to do, and the way to the control centre.
fn no_account<'a>() -> Element<'a, Message> {
    calm(
        Some(Icon::BookUser),
        tr("Connect an account in Torro Workplace"),
        tr("Your contacts come from the address books of your accounts, such as Nextcloud."),
        Some(widgets::text_button(
            Some(Icon::ArrowUpRight),
            tr("Open Torro Workplace"),
            style::button::secondary,
            Some(Message::OpenWorkplace),
        )),
    )
}

/// A calm state in the middle of its room: an icon, what is (or is not) here, what to do, and a button
/// for it – as `widgets::empty_state`, with the button right under the words.
pub(crate) fn calm<'a>(
    mark: Option<Icon>,
    title: &'a str,
    hint: &'a str,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![].spacing(space::m()).align_x(Alignment::Center).max_width(size::form_max() / 2.0);
    if let Some(mark) = mark {
        content = content.push(tinted(mark, size::tile_lg(), |p| p.text_3));
    }
    content = content.push(
        column![
            typo::text(typo::body_strong(), title).align_x(Alignment::Center),
            typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center),
        ]
        .spacing(space::xs())
        .align_x(Alignment::Center),
    );
    if let Some(action) = action {
        content = content.push(action);
    }
    container(content).width(Fill).height(Fill).center(Fill).padding(space::xl()).into()
}
