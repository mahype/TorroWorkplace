//! The grouped list (draft D2, `docs/prototypes/kontakte/d2-gruppen.html`): the contacts under heads
//! that fold, by group, town or address book ([`GroupBy`]), each head with its count and "+" (not by
//! town), and a row per contact [`size::row_contact()`] high on two lines.
//!
//! What goes under which head is a plain function ([`heads`]), as are the columns a row keeps at a
//! width ([`Columns::at`]), so tests check them without a window.
//!
//! - **By group:** a head per group name over all books, sorted by name, "No group" last; a contact
//!   stands under each of its groups.
//! - **By town:** the towns with the most contacts first, then by name, "No city" last.
//! - **By address book:** in the order of the books.
//!
//! In each head the contacts sort by family name, then by the name shown. A row: the picture or the
//! initials, given name quiet and family name strong, the star of a favourite, the lock of a read-only
//! book and the marks of a change not yet on the server or in conflict; under the name position ·
//! company. Mail over number to the right of it, the town at the end. Below 1100 the town goes, below
//! 900 the number (the mail address stays, on one line, so a medium window is not half empty), below 600
//! the mail address too. Moving the pointer over a row shows Mail, Call and Copy – each only with a value.

use std::collections::{BTreeMap, HashMap};

use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, image, mouse_area, row};
use iced::{Alignment, Background, Element, Fill, Padding, Theme, border, mouse};
use torro_contacts::{AddressBook, Birthday, ContactState, ContactSummary};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, RowAction};
use torro_gui::{datetime, layout, style, typo};

use crate::app::Message;

/// How the list is grouped (the menu "Group by"); stored in `ui.json` as `group_by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum GroupBy {
    #[default]
    Group,
    City,
    Book,
}

impl GroupBy {
    pub const ALL: [GroupBy; 3] = [GroupBy::Group, GroupBy::City, GroupBy::Book];

    /// The word in `ui.json`.
    pub fn word(self) -> &'static str {
        match self {
            GroupBy::Group => "group",
            GroupBy::City => "city",
            GroupBy::Book => "book",
        }
    }

    /// The grouping a stored word names; `None` for a word this version does not know.
    pub fn from_word(word: &str) -> Option<GroupBy> {
        GroupBy::ALL.into_iter().find(|by| by.word() == word)
    }

    /// The name in the menu and the button.
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Group => tr("Group"),
            GroupBy::City => tr("City"),
            GroupBy::Book => tr("Address book"),
        }
    }

    /// The words in the header's button: "By group".
    pub fn by_label(self) -> &'static str {
        match self {
            GroupBy::Group => tr("By group"),
            GroupBy::City => tr("By city"),
            GroupBy::Book => tr("By address book"),
        }
    }

    /// The icon of its heads.
    pub fn icon(self) -> Icon {
        match self {
            GroupBy::Group => Icon::Tag,
            GroupBy::City => Icon::MapPin,
            GroupBy::Book => Icon::BookUser,
        }
    }

    /// The key of the folded heads of this grouping in `ui.json`.
    pub fn collapsed_key(self) -> String {
        format!("collapsed.{}", self.word())
    }
}

/// What a head stands for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A group, by its name.
    Group(String),
    /// The contacts in no group.
    NoGroup,
    /// A town.
    City(String),
    /// The contacts without a town.
    NoCity,
    /// An address book, by its id.
    Book(String),
    /// A month of the view "Birthdays" (year, month).
    Month(i32, u32),
}

impl Key {
    /// The word under which a folded head is stored: the name, empty for "No group" and "No city".
    pub fn word(&self) -> String {
        match self {
            Key::Group(name) | Key::City(name) | Key::Book(name) => format!("={name}"),
            Key::NoGroup | Key::NoCity => String::new(),
            Key::Month(year, month) => format!("{year}-{month:02}"),
        }
    }
}

/// One head of the list with its contacts, already sorted.
#[derive(Debug, Clone, PartialEq)]
pub struct Head<'a> {
    pub key: Key,
    /// The name the head shows.
    pub label: String,
    pub members: Vec<&'a ContactSummary>,
}

/// The order of contacts in a head: family name, then the name shown, without case. A contact without
/// a family name (a company) sorts by the name shown.
pub fn sort_key(contact: &ContactSummary) -> (String, String) {
    let shown = contact.display_name.to_lowercase();
    let family = contact.family.trim().to_lowercase();
    (if family.is_empty() { shown.clone() } else { family }, shown)
}

/// The heads of `contacts` grouped `by`, in the order the module says; `books` give the order and names
/// of the books.
pub fn heads<'a>(contacts: &[&'a ContactSummary], by: GroupBy, books: &[AddressBook]) -> Vec<Head<'a>> {
    let mut heads: Vec<Head<'a>> = match by {
        GroupBy::Group => {
            let mut named: BTreeMap<String, (String, Vec<&'a ContactSummary>)> = BTreeMap::new();
            let mut none = Vec::new();
            for contact in contacts {
                // One head per group without case: "Work" and "work" of one contact count once.
                let mut names: Vec<&str> = Vec::new();
                for group in contact.groups.iter().map(|g| g.trim()).filter(|g| !g.is_empty()) {
                    if !names.iter().any(|known| known.to_lowercase() == group.to_lowercase()) {
                        names.push(group);
                    }
                }
                if names.is_empty() {
                    none.push(*contact);
                }
                for name in names {
                    named.entry(name.to_lowercase()).or_insert_with(|| (name.to_owned(), Vec::new())).1.push(*contact);
                }
            }
            let mut heads: Vec<Head<'a>> = named
                .into_values()
                .map(|(name, members)| Head { key: Key::Group(name.clone()), label: name, members })
                .collect();
            if !none.is_empty() {
                heads.push(Head { key: Key::NoGroup, label: tr("No group").to_owned(), members: none });
            }
            heads
        }
        GroupBy::City => {
            let mut towns: HashMap<String, (String, Vec<&'a ContactSummary>)> = HashMap::new();
            let mut none = Vec::new();
            for contact in contacts {
                match contact.locality.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    Some(town) => towns
                        .entry(town.to_lowercase())
                        .or_insert_with(|| (town.to_owned(), Vec::new()))
                        .1
                        .push(*contact),
                    None => none.push(*contact),
                }
            }
            let mut heads: Vec<Head<'a>> = towns
                .into_values()
                .map(|(town, members)| Head { key: Key::City(town.clone()), label: town, members })
                .collect();
            heads.sort_by(|a, b| {
                b.members.len().cmp(&a.members.len()).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            });
            if !none.is_empty() {
                heads.push(Head { key: Key::NoCity, label: tr("No city").to_owned(), members: none });
            }
            heads
        }
        GroupBy::Book => books
            .iter()
            .map(|book| Head {
                key: Key::Book(book.id.clone()),
                label: book.display_name.clone(),
                members: contacts.iter().filter(|c| c.book == book.id).copied().collect(),
            })
            .filter(|head| !head.members.is_empty())
            .collect(),
    };
    for head in &mut heads {
        head.members.sort_by_key(|contact| sort_key(contact));
    }
    heads
}

/// The birthdays of the coming year under a head per month, the soonest first.
pub fn birthday_heads<'a>(birthdays: &[&'a Birthday]) -> Vec<(Key, String, Vec<&'a Birthday>)> {
    let mut months: Vec<(Key, String, Vec<&'a Birthday>)> = Vec::new();
    let mut sorted: Vec<&'a Birthday> = birthdays.to_vec();
    sorted.sort_by(|a, b| a.next.cmp(&b.next).then_with(|| sort_key(&a.contact).cmp(&sort_key(&b.contact))));
    for birthday in sorted {
        use chrono::Datelike;
        let key = Key::Month(birthday.next.year(), birthday.next.month());
        match months.last_mut() {
            Some((last, _, members)) if *last == key => members.push(birthday),
            _ => {
                let label = trf(
                    "{month} {year}",
                    &[("month", &datetime::month_name(birthday.next.month())), ("year", &birthday.next.year())],
                );
                months.push((key, label, vec![birthday]));
            }
        }
    }
    months
}

/// What a row of the list shows beside the name, by the width of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// Mail over number.
    pub reach: bool,
    /// Below [`REACH_FROM`]: the mail address alone, on one line beside the name.
    pub mail: bool,
    /// The town at the end.
    pub city: bool,
}

/// Below this width a row drops the town (draft D2).
pub const CITY_FROM: f32 = 1100.0;
/// Below this width a row drops the number and keeps the mail address on one line.
pub const REACH_FROM: f32 = 900.0;
/// Below this width a row shows name and position only.
pub const MAIL_FROM: f32 = 600.0;

impl Columns {
    /// The columns of a list `width` wide.
    pub fn at(width: f32) -> Columns {
        Columns { reach: width >= REACH_FROM, mail: (MAIL_FROM..REACH_FROM).contains(&width), city: width >= CITY_FROM }
    }
}

/// What a row shows at its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The town (or nothing) as wide as [`end_width`].
    Town,
    /// "turns 40" and the day, each in a column of its own, the day at the right ("Birthdays").
    Birthday { age: Option<String>, day: String },
}

/// The initials of a name for its picture: the first letters of the first and the last word, in
/// capitals ("AB" of "Anna Berg"); "?" for a name without letters.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|word| word.chars().any(char::is_alphanumeric)).collect();
    let first = |word: &&str| word.chars().find(|c| c.is_alphanumeric());
    let letters: String = match words.as_slice() {
        [] => return "?".to_owned(),
        [one] => first(one).into_iter().collect(),
        [one, .., last] => first(one).into_iter().chain(first(last)).collect(),
    };
    letters.to_uppercase()
}

/// The colour of a contact's initials: the sender colour of the name, the same everywhere.
pub fn hue(colours: &Palette, name: &str) -> iced::Color {
    colours.sender.get(widgets::sender_hue(name)).copied().unwrap_or(colours.text_3)
}

/// A contact's face, `edge` across: the picture when there is one, else the initials in `text` on the
/// colour of the name, tinted as the faces of the suite are (never the full colour as a surface).
pub fn face<'a>(name: &str, picture: Option<&Handle>, edge: f32, colours: &Palette) -> Element<'a, Message> {
    let round = edge / 2.0;
    if let Some(picture) = picture {
        return container(image(picture.clone()).width(edge).height(edge).border_radius(round)).into();
    }
    let colour = hue(colours, name);
    let mut letters = initials(name);
    if edge < size::tile_lg() {
        letters = letters.chars().take(1).collect();
    }
    let level = if edge >= size::avatar_lg() { typo::title() } else { typo::caption() };
    container(typo::text(level, letters).wrapping(Wrapping::None))
        .center(edge)
        .style(move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.tinted(colour))),
                text_color: Some(p.text),
                border: border::rounded(round),
                ..container::Style::default()
            }
        })
        .into()
}

/// `which` at `edge` in the colour `colour` picks from the palette.
pub fn tinted<'a>(which: Icon, edge: f32, colour: fn(&Palette) -> iced::Color) -> Element<'a, Message> {
    icon::icon(which, edge).style(move |t, _| iced::widget::svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// The marks after a name: favourite, read-only, saved here only, in conflict – each with its word as
/// tooltip, so none rests on colour.
pub fn marks<'a>(contact: &ContactSummary) -> Vec<Element<'a, Message>> {
    let mut marks = Vec::new();
    let edge = icon::SIZE_SM;
    if contact.favorite {
        marks.push(widgets::with_tooltip(tinted(Icon::Star, edge, |p| p.text_3), tr("Favorite")));
    }
    if contact.readonly {
        marks.push(widgets::with_tooltip(tinted(Icon::Lock, edge, |p| p.text_3), tr("Read-only address book")));
    }
    match contact.state {
        ContactState::Synced => {}
        ContactState::Pending => marks.push(widgets::with_tooltip(
            tinted(Icon::Clock, edge, |p| p.text_3),
            tr("Saved on this device, not yet on the server"),
        )),
        ContactState::Conflict => marks
            .push(widgets::with_tooltip(tinted(Icon::TriangleAlert, edge, |p| p.attention), tr("Changed elsewhere"))),
    }
    marks
}

/// "Photographer · Lux" under the name; empty without both.
pub fn role_line(title: Option<&str>, org: Option<&str>) -> String {
    [title, org].into_iter().flatten().map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// What a row needs besides its contact.
pub struct RowLook<'a> {
    pub columns: Columns,
    pub selected: bool,
    /// The pointer or the cursor is on it: the action bar shows.
    pub hovered: bool,
    pub picture: Option<&'a Handle>,
    pub colours: Palette,
    /// What stands at the end.
    pub end: End,
    /// The width of the list, for the words of the action bar.
    pub width: f32,
    /// Registered by the row whose menu is open, so the menu hangs at the pointer in it.
    pub anchor: Option<widgets::Anchor>,
}

/// One row of the list (see the module docs).
pub fn contact_row<'a>(contact: &'a ContactSummary, look: RowLook<'a>) -> Element<'a, Message> {
    let edge = size::tile_lg();
    let shown = contact.display_name.as_str();
    let given = contact.given.trim();
    let family = contact.family.trim();
    // A word's gap between given and family name, whatever the spacing level.
    let gap = typo::text_width(" ", typo::body());
    let mut name = row![].align_y(Alignment::Center);
    if !family.is_empty() && shown.ends_with(family) && shown.len() > family.len() {
        let lead = shown[..shown.len() - family.len()].trim_end();
        name = name
            .push(typo::text(typo::body(), lead.to_owned()).style(theme::text_2).wrapping(Wrapping::None))
            .push(Space::new().width(gap))
            .push(typo::text(typo::body_strong(), family.to_owned()).wrapping(Wrapping::None));
    } else if !given.is_empty() && shown.starts_with(given) && shown.len() > given.len() {
        name = name
            .push(typo::text(typo::body(), given.to_owned()).style(theme::text_2).wrapping(Wrapping::None))
            .push(Space::new().width(gap))
            .push(typo::text(typo::body_strong(), shown[given.len()..].trim().to_owned()).wrapping(Wrapping::None));
    } else {
        name = name.push(typo::text(typo::body_strong(), shown.to_owned()).wrapping(Wrapping::None));
    }
    let marks = marks(contact);
    if !marks.is_empty() {
        name = name.push(Space::new().width(space::xs()));
        name = name.push(row(marks).spacing(space::xs()).align_y(Alignment::Center));
    }
    let role = role_line(contact.title.as_deref(), contact.org.as_deref());
    let mut who = column![container(name).clip(true)];
    if !role.is_empty() {
        who = who.push(widgets::ellipsized(typo::meta(), role, theme::text_3));
    }
    // The name takes the room, up to a reading width; what follows stands right after it.
    let who = container(who).width(Fill).max_width(size::content_max());
    let mut cells = row![face(shown, look.picture, edge, &look.colours), who]
        .spacing(space::m())
        .align_y(Alignment::Center)
        .height(Fill);
    if look.columns.mail {
        let mail = contact.email.clone().unwrap_or_default();
        cells = cells.push(container(widgets::ellipsized(typo::meta(), mail, theme::text_2)).width(reach_width()));
    }
    if look.columns.reach {
        let mail = contact.email.clone().unwrap_or_default();
        let phone = contact.phone.clone().unwrap_or_default();
        cells = cells.push(
            column![
                widgets::ellipsized(typo::meta(), mail, theme::text_2),
                widgets::ellipsized(typo::meta().tabular(), phone, theme::text_3),
            ]
            .width(reach_width()),
        );
    }
    match &look.end {
        End::Town if look.columns.city => {
            let town = contact.locality.clone().unwrap_or_default();
            cells = cells.push(container(widgets::ellipsized(typo::meta(), town, theme::text_3)).width(end_width()));
        }
        End::Town => {}
        End::Birthday { age, day } => {
            cells = cells.push(
                container(widgets::ellipsized(typo::meta(), age.clone().unwrap_or_default(), theme::text_3))
                    .width(end_width()),
            );
            cells = cells.push(
                container(
                    typo::text(typo::meta().tabular(), day.clone()).style(theme::text_2).wrapping(Wrapping::None),
                )
                .width(end_width())
                .align_right(end_width()),
            );
        }
    }
    cells = cells.push(Space::new().width(Fill));
    let id = contact.id.clone();
    let line = widgets::list_row_sized(cells, size::row_contact(), 0.0, look.selected, Some(Message::Open(id.clone())));
    let actions = row_actions(contact);
    let words = widgets::row_actions_words(&actions, look.width);
    let with_bar = widgets::row_actions(line, actions, look.hovered, words);
    let menu_id = id.clone();
    let area = widgets::context_area(with_bar, move |at| Message::RowMenu(menu_id.clone(), at));
    let area = match &look.anchor {
        Some(anchor) => widgets::anchored(anchor, area),
        None => area,
    };
    mouse_area(area).on_enter(Message::Hover(Some(id))).on_exit(Message::Hover(None)).into()
}

/// The width of mail over number: the width of the properties column (300 in the standard design).
pub fn reach_width() -> f32 {
    size::props()
}

/// The width of the town at the end of a row: half a sidebar (120 in the standard design).
pub fn end_width() -> f32 {
    size::sidebar() / 2.0
}

/// The bar a row shows under the pointer: Mail, Call and Copy, each only with a value.
pub fn row_actions<'a>(contact: &ContactSummary) -> Vec<RowAction<'a, Message>> {
    let mut actions = Vec::new();
    if contact.email.as_deref().is_some_and(|mail| !mail.trim().is_empty()) {
        actions.push(RowAction::new(Icon::Mail, tr("Mail"), "M", Message::QuickMail(contact.id.clone())));
    }
    if contact.phone.as_deref().is_some_and(|phone| !phone.trim().is_empty()) {
        actions.push(RowAction::new(Icon::Phone, tr("Call"), "C", Message::QuickCall(contact.id.clone())));
    }
    if contact.email.is_some() || contact.phone.is_some() {
        actions.push(RowAction::new(Icon::Copy, tr("Copy"), "", Message::QuickCopy(contact.id.clone())));
    }
    actions
}

/// A head of the list, [`size::row_group()`] high on `raised`, flush with the rows: the chevron in the
/// rows' left padding, the head's icon, its name and count; with `on_add` the "+" at the right. A
/// click anywhere folds or unfolds it.
pub fn head<'a>(
    mark: Icon,
    label: String,
    count: usize,
    folded: bool,
    fold: Message,
    on_add: Option<Message>,
) -> Element<'a, Message> {
    let chevron = if folded { Icon::ChevronRight } else { Icon::ChevronDown };
    let side = layout::row_padding();
    let chevron_edge = (typo::line_height(typo::caption()) - space::xs()).min(side.left);
    let mut line = row![
        container(tinted(chevron, chevron_edge, |p| p.text_3)).center_x(side.left),
        container(tinted(mark, icon::SIZE_SM, |p| p.text_3)).center_x(icon::SIZE),
        Space::new().width(space::m()),
        typo::text(typo::caption(), label).style(theme::text_2).wrapping(Wrapping::None),
        Space::new().width(space::s()),
        typo::text(typo::meta().tabular(), count.to_string()).style(theme::text_3),
        Space::new().width(Fill),
    ]
    .align_y(Alignment::Center);
    let edge = size::control_sm().min(size::row_group());
    if let Some(on_add) = on_add {
        let add = button(container(tinted(Icon::Plus, icon::SIZE_SM, |p| p.text_3)).center(Fill))
            .width(edge)
            .height(edge)
            .padding(0)
            .style(style::button::icon)
            .on_press(on_add);
        line = line.push(widgets::with_tooltip(add, tr("New contact here")));
    }
    let right = (side.right - (edge - icon::SIZE_SM) / 2.0).max(0.0);
    let head = container(line).width(Fill).center_y(size::row_group()).padding(Padding::new(0.0).right(right)).style(
        |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).raised)),
            ..container::Style::default()
        },
    );
    mouse_area(head).on_press(fold).interaction(mouse::Interaction::Pointer).into()
}

/// The end of a row of "Birthdays": "turns 40" (none without a year of birth) and the day, "18 Nov" or
/// "Today"; the month's head says which month, so the day needs no weekday.
pub fn birthday_end(birthday: &Birthday, today: chrono::NaiveDate) -> End {
    let day = if birthday.next == today { tr("Today").to_owned() } else { datetime::format_day_month(birthday.next) };
    End::Birthday { age: birthday.turns.map(|turns| trf("turns {n}", &[("n", &turns)])), day }
}

/// The width the list takes of `content` beside an open contact: everything left of the panel.
pub fn list_width(content: f32, panel: Option<f32>) -> f32 {
    (content - panel.unwrap_or(0.0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_contacts::demo;

    fn person(id: &str, given: &str, family: &str, town: Option<&str>, groups: &[&str], book: &str) -> ContactSummary {
        ContactSummary {
            id: id.into(),
            book: book.into(),
            display_name: format!("{given} {family}").trim().to_owned(),
            family: family.into(),
            given: given.into(),
            org: None,
            title: None,
            email: None,
            phone: None,
            locality: town.map(String::from),
            photo: None,
            favorite: false,
            readonly: false,
            state: ContactState::Synced,
            groups: groups.iter().map(|g| (*g).to_owned()).collect(),
            untrusted: true,
        }
    }

    fn book(id: &str, name: &str) -> AddressBook {
        AddressBook {
            id: id.into(),
            account: "a".into(),
            name: name.into(),
            display_name: name.into(),
            count: 0,
            writable: true,
            hidden_by_default: false,
            untrusted: true,
        }
    }

    fn names(head: &Head<'_>) -> Vec<String> {
        head.members.iter().map(|c| c.display_name.clone()).collect()
    }

    #[test]
    fn by_group_every_group_once_by_name_and_no_group_last() {
        let people = [
            person("1", "Tom", "Meier", None, &["Work", "Family"], "b"),
            person("2", "Anna", "Berg", None, &["work"], "c"),
            person("3", "Zoe", "Abel", None, &[], "b"),
            person("4", "Acme", "", None, &["Clients"], "b"),
        ];
        let refs: Vec<&ContactSummary> = people.iter().collect();
        let heads = heads(&refs, GroupBy::Group, &[]);
        let labels: Vec<&str> = heads.iter().map(|h| h.label.as_str()).collect();
        assert_eq!(labels, ["Clients", "Family", "Work", tr("No group")]);
        assert_eq!(names(&heads[2]), ["Anna Berg", "Tom Meier"], "one group over books, by family name");
        assert_eq!(names(&heads[1]), ["Tom Meier"], "a contact under each of its groups");
        assert_eq!(heads[3].key, Key::NoGroup);
    }

    #[test]
    fn by_city_the_largest_first_and_no_city_last() {
        let people = [
            person("1", "A", "One", Some("Kiel"), &[], "b"),
            person("2", "B", "Two", Some("Berlin"), &[], "b"),
            person("3", "C", "Three", Some("berlin"), &[], "b"),
            person("4", "D", "Four", None, &[], "b"),
            person("5", "E", "Five", Some("Bonn"), &[], "b"),
        ];
        let refs: Vec<&ContactSummary> = people.iter().collect();
        let heads = heads(&refs, GroupBy::City, &[]);
        let labels: Vec<&str> = heads.iter().map(|h| h.label.as_str()).collect();
        assert_eq!(labels, ["Berlin", "Bonn", "Kiel", tr("No city")]);
        assert_eq!(heads[0].members.len(), 2);
    }

    #[test]
    fn by_book_in_the_order_of_the_books() {
        let people = [person("1", "A", "One", None, &[], "y"), person("2", "B", "Two", None, &[], "x")];
        let refs: Vec<&ContactSummary> = people.iter().collect();
        let heads = heads(&refs, GroupBy::Book, &[book("y", "Team"), book("z", "Empty"), book("x", "Contacts")]);
        let labels: Vec<&str> = heads.iter().map(|h| h.label.as_str()).collect();
        assert_eq!(labels, ["Team", "Contacts"], "an empty book has no head");
    }

    #[test]
    fn the_demo_groups_as_the_draft() {
        let (books, contacts) = demo::sample(chrono::NaiveDate::from_ymd_opt(2026, 10, 28).expect("date"));
        let summaries: Vec<ContactSummary> = contacts.iter().map(|c| c.summary()).collect();
        let refs: Vec<&ContactSummary> = summaries.iter().collect();
        let by_group = heads(&refs, GroupBy::Group, &books);
        assert_eq!(by_group.len(), 3);
        assert_eq!(by_group.iter().map(|h| h.members.len()).sum::<usize>(), 41);
        let by_book = heads(&refs, GroupBy::Book, &books);
        assert_eq!(by_book.len(), 2);
    }

    #[test]
    fn columns_drop_the_town_then_mail_and_number() {
        assert_eq!(Columns::at(1400.0), Columns { reach: true, mail: false, city: true });
        assert_eq!(Columns::at(1000.0), Columns { reach: true, mail: false, city: false });
        assert_eq!(Columns::at(740.0), Columns { reach: false, mail: true, city: false });
        assert_eq!(Columns::at(500.0), Columns { reach: false, mail: false, city: false });
    }

    #[test]
    fn folded_heads_are_stored_by_name() {
        assert_eq!(Key::Group("Work".into()).word(), "=Work");
        assert_eq!(Key::NoGroup.word(), "");
        assert_ne!(Key::Group(String::new()).word(), Key::NoGroup.word());
    }

    #[test]
    fn grouping_words_round_trip() {
        for by in GroupBy::ALL {
            assert_eq!(GroupBy::from_word(by.word()), Some(by));
        }
        assert_eq!(GroupBy::from_word("company"), None);
    }

    #[test]
    fn initials_and_role() {
        assert_eq!(initials("Anna Maria Berg"), "AB");
        assert_eq!(initials("acme"), "A");
        assert_eq!(initials("  "), "?");
        assert_eq!(role_line(Some("Photographer"), Some("Lux")), "Photographer · Lux");
        assert_eq!(role_line(None, Some(" ")), "");
    }

    #[test]
    fn birthdays_by_month_the_soonest_first() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 28).expect("date");
        let demo = demo::sample(today).1;
        let all: Vec<Birthday> = demo.iter().filter_map(|c| Birthday::upcoming(c, today, 366)).collect();
        let refs: Vec<&Birthday> = all.iter().collect();
        let months = birthday_heads(&refs);
        assert!(months.len() >= 2);
        assert_eq!(months[0].0, Key::Month(2026, 10));
        let flat: Vec<chrono::NaiveDate> = months.iter().flat_map(|m| m.2.iter().map(|b| b.next)).collect();
        assert!(flat.windows(2).all(|w| w[0] <= w[1]));
        let first = months[0].2[0];
        let End::Birthday { day, .. } = birthday_end(first, today) else { panic!("a birthday's end") };
        assert_eq!(day == tr("Today"), first.next == today);
    }
}
