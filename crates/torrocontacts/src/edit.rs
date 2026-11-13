//! Editing a contact in its side panel (task KO1.7): "Edit" (Ctrl+E, ⌘E) turns the fields into inputs,
//! each value with a chooser of its kind ("work", "home" …), "+ Add field" for more values. "Done"
//! sends only what changed ([`Draft::changes`]) with the version the contact had (`etag`); nothing
//! changed, nothing is sent. "Cancel" drops the draft.
//!
//! Mail addresses and numbers are checked while typing ([`mail_ok`], [`phone_ok`]): a quiet line under
//! a value that looks wrong, once the person has left it or tried to save; saving stays possible. A
//! date that is no date stays out of what is sent and says so.
//!
//! The same form writes a new contact ([`Draft::empty`]); `contacts.create_contact` takes it.
//!
//! A contact in conflict (changed here and on the server) shows both versions side by side
//! ([`differences`]), the values that differ marked, with "Keep mine", "Keep theirs" and "Keep both"
//! (`contacts.resolve_conflict`).

use std::collections::BTreeSet;

use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, text_input};
use iced::{Alignment, Element, Fill, Length, Padding};
use torro_contacts::{Contact, ContactFields, FieldKind, Labeled, Name, PartialDate, PostalAddress};
use torro_core::i18n::tr;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, space};
use torro_gui::widgets::{self, Anchor};
use torro_gui::{style, typo};

use crate::app::Message;
use crate::list::tinted;

/// A field of the form, for the check while typing and for which field the person left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    Given,
    Family,
    Org,
    Title,
    Birthday,
    Note,
    Email(usize),
    Phone(usize),
    Url(usize),
    Street(usize),
    Postal(usize),
    Town(usize),
    Country(usize),
}

/// A field that holds several values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum List {
    Email,
    Phone,
    Url,
    Address,
}

impl List {
    pub const ALL: [List; 4] = [List::Phone, List::Email, List::Address, List::Url];

    /// The name of the field ("Phone", "Email" …).
    pub fn label(self) -> &'static str {
        match self {
            List::Email => tr("Email"),
            List::Phone => tr("Phone"),
            List::Url => tr("Link"),
            List::Address => tr("Address"),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            List::Email => Icon::Mail,
            List::Phone => Icon::Phone,
            List::Url => Icon::Link,
            List::Address => Icon::MapPin,
        }
    }

    /// The kinds the chooser offers for a value of this field.
    pub fn kinds(self) -> Vec<FieldKind> {
        match self {
            List::Phone => vec![FieldKind::Mobile, FieldKind::Work, FieldKind::Home, FieldKind::Fax, FieldKind::Other],
            List::Email | List::Url | List::Address => vec![FieldKind::Work, FieldKind::Home, FieldKind::Other],
        }
    }

    /// The kind a new value of this field starts with.
    fn first_kind(self) -> FieldKind {
        match self {
            List::Phone => FieldKind::Mobile,
            List::Email | List::Url | List::Address => FieldKind::Work,
        }
    }
}

/// The word of a kind ("work", "mobile"), or the person's own label.
pub fn kind_word(kind: &FieldKind) -> String {
    match kind {
        FieldKind::Home => tr("home").to_owned(),
        FieldKind::Work => tr("work").to_owned(),
        FieldKind::Mobile => tr("mobile").to_owned(),
        FieldKind::Fax => tr("fax").to_owned(),
        FieldKind::Other => tr("other").to_owned(),
        FieldKind::Custom(text) => text.clone(),
    }
}

/// One value of a field with several (a mail address, a number, a link).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub value: String,
    pub kind: FieldKind,
    pub pref: bool,
}

impl Line {
    fn of(value: &Labeled<String>) -> Line {
        Line { value: value.value.clone(), kind: value.kind.clone(), pref: value.pref }
    }
}

/// One postal address being edited. The parts the form does not show (region, post office box …) stay
/// as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub street: String,
    pub postal_code: String,
    pub locality: String,
    pub country: String,
    pub kind: FieldKind,
    pub pref: bool,
    rest: PostalAddress,
}

impl Place {
    fn of(value: &Labeled<PostalAddress>) -> Place {
        Place {
            street: value.value.street.clone(),
            postal_code: value.value.postal_code.clone(),
            locality: value.value.locality.clone(),
            country: value.value.country.clone(),
            kind: value.kind.clone(),
            pref: value.pref,
            rest: value.value.clone(),
        }
    }

    fn blank(kind: FieldKind) -> Place {
        Place {
            street: String::new(),
            postal_code: String::new(),
            locality: String::new(),
            country: String::new(),
            kind,
            pref: false,
            rest: PostalAddress::default(),
        }
    }

    fn address(&self) -> PostalAddress {
        PostalAddress {
            street: self.street.trim().to_owned(),
            postal_code: self.postal_code.trim().to_owned(),
            locality: self.locality.trim().to_owned(),
            country: self.country.trim().to_owned(),
            ..self.rest.clone()
        }
    }
}

/// What a change of the form says.
#[derive(Debug, Clone)]
pub enum Msg {
    Text(Field, String),
    Kind(List, usize, FieldKind),
    Add(List),
    Remove(List, usize),
}

/// A contact being edited, or a new one.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// What editing started from: the changes are measured against it, so a value the server changed
    /// meanwhile is not overwritten unless the person changed it too. Empty for a new contact.
    pub base: Contact,
    /// The version to send with the change; after a conflict on saving, the server's newer one.
    pub etag: Option<String>,
    pub given: String,
    pub family: String,
    pub org: String,
    pub title: String,
    pub birthday: String,
    pub note: String,
    pub emails: Vec<Line>,
    pub phones: Vec<Line>,
    pub urls: Vec<Line>,
    pub addresses: Vec<Place>,
    /// "Done" was pressed once: every check shows.
    pub tried: bool,
    /// The field typed in last, and the ones left since: their checks show.
    active: Option<Field>,
    left: BTreeSet<Field>,
    /// The save is out: "Done" takes no second press.
    pub saving: bool,
    /// Why the last save failed, as one line for people.
    pub error: Option<String>,
}

impl Draft {
    /// The form for `contact` as it is.
    pub fn of(contact: &Contact) -> Draft {
        Draft {
            base: contact.clone(),
            etag: contact.etag.clone(),
            given: contact.name.given.clone(),
            family: contact.name.family.clone(),
            org: contact.org.clone().unwrap_or_default(),
            title: contact.title.clone().unwrap_or_default(),
            birthday: contact.birthday.map(|b| b.to_string()).unwrap_or_default(),
            note: contact.note.clone().unwrap_or_default(),
            emails: contact.emails.iter().map(Line::of).collect(),
            phones: contact.phones.iter().map(Line::of).collect(),
            urls: contact.urls.iter().map(Line::of).collect(),
            addresses: contact.addresses.iter().map(Place::of).collect(),
            tried: false,
            active: None,
            left: BTreeSet::new(),
            saving: false,
            error: None,
        }
    }

    /// An empty form for a new contact: a number and a mail address to fill in.
    pub fn empty() -> Draft {
        let mut draft = Draft::of(&Contact::default());
        draft.phones.push(Line { value: String::new(), kind: List::Phone.first_kind(), pref: false });
        draft.emails.push(Line { value: String::new(), kind: List::Email.first_kind(), pref: false });
        draft
    }

    /// Applies a change of the form.
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Text(field, value) => {
                if let Some(before) = self.active.filter(|before| *before != field) {
                    self.left.insert(before);
                }
                self.active = Some(field);
                if let Some(slot) = self.slot(field) {
                    *slot = value;
                }
            }
            Msg::Kind(list, index, kind) => match list {
                List::Address => {
                    if let Some(place) = self.addresses.get_mut(index) {
                        place.kind = kind;
                    }
                }
                _ => {
                    if let Some(line) = self.lines_mut(list).get_mut(index) {
                        line.kind = kind;
                    }
                }
            },
            Msg::Add(List::Address) => self.addresses.push(Place::blank(List::Address.first_kind())),
            Msg::Add(list) => {
                let kind = list.first_kind();
                self.lines_mut(list).push(Line { value: String::new(), kind, pref: false });
            }
            Msg::Remove(List::Address, index) => {
                if index < self.addresses.len() {
                    self.addresses.remove(index);
                }
            }
            Msg::Remove(list, index) => {
                let lines = self.lines_mut(list);
                if index < lines.len() {
                    lines.remove(index);
                }
            }
        }
    }

    fn lines_mut(&mut self, list: List) -> &mut Vec<Line> {
        match list {
            List::Email => &mut self.emails,
            List::Phone => &mut self.phones,
            List::Url | List::Address => &mut self.urls,
        }
    }

    fn slot(&mut self, field: Field) -> Option<&mut String> {
        Some(match field {
            Field::Given => &mut self.given,
            Field::Family => &mut self.family,
            Field::Org => &mut self.org,
            Field::Title => &mut self.title,
            Field::Birthday => &mut self.birthday,
            Field::Note => &mut self.note,
            Field::Email(i) => &mut self.emails.get_mut(i)?.value,
            Field::Phone(i) => &mut self.phones.get_mut(i)?.value,
            Field::Url(i) => &mut self.urls.get_mut(i)?.value,
            Field::Street(i) => &mut self.addresses.get_mut(i)?.street,
            Field::Postal(i) => &mut self.addresses.get_mut(i)?.postal_code,
            Field::Town(i) => &mut self.addresses.get_mut(i)?.locality,
            Field::Country(i) => &mut self.addresses.get_mut(i)?.country,
        })
    }

    /// What "Done" sends: only the fields that differ from [`Draft::base`]. Empty values of a list are
    /// left out; a birthday that is no date is left out too ([`Draft::hint`] says so).
    pub fn changes(&self) -> ContactFields {
        let base = &self.base;
        let mut fields = ContactFields::default();
        let name =
            Name { given: self.given.trim().to_owned(), family: self.family.trim().to_owned(), ..base.name.clone() };
        if name != base.name {
            // The name shown follows the name, unless the person gave it one of its own.
            let own = !base.display_name.trim().is_empty() && base.display_name.trim() != base.name.full();
            if !own {
                fields.display_name = Some(shown_name(&name, self.org.trim()));
            }
            fields.name = Some(name);
        } else if base.display_name.trim().is_empty() && !self.org.trim().is_empty() && base.name.is_empty() {
            fields.display_name = Some(self.org.trim().to_owned());
        }
        let text = |now: &str, before: &Option<String>| {
            let now = now.trim();
            (now != before.as_deref().unwrap_or("").trim()).then(|| now.to_owned())
        };
        fields.org = text(&self.org, &base.org);
        fields.title = text(&self.title, &base.title);
        fields.note = (self.note.trim() != base.note.as_deref().unwrap_or("").trim()).then(|| self.note.clone());
        let lines = |lines: &[Line], before: &[Labeled<String>]| {
            let now: Vec<Labeled<String>> = lines
                .iter()
                .filter(|line| !line.value.trim().is_empty())
                .map(|line| Labeled { value: line.value.trim().to_owned(), kind: line.kind.clone(), pref: line.pref })
                .collect();
            (now != before).then_some(now)
        };
        fields.emails = lines(&self.emails, &base.emails);
        fields.phones = lines(&self.phones, &base.phones);
        fields.urls = lines(&self.urls, &base.urls);
        let places: Vec<Labeled<PostalAddress>> = self
            .addresses
            .iter()
            .map(|place| Labeled { value: place.address(), kind: place.kind.clone(), pref: place.pref })
            .filter(|place| !place.value.is_empty())
            .collect();
        fields.addresses = (places != base.addresses).then_some(places);
        let birthday = self.birthday.trim();
        let before = base.birthday.map(|b| b.to_string()).unwrap_or_default();
        if birthday != before && (birthday.is_empty() || PartialDate::parse(birthday).is_some()) {
            fields.birthday = Some(birthday.to_owned());
        }
        fields
    }

    /// Whether the form holds nothing a new contact could be made of.
    pub fn is_blank(&self) -> bool {
        let changes = self.changes();
        changes.name.is_none() && changes.org.is_none() && changes.emails.is_none() && changes.phones.is_none()
    }

    /// The quiet line under `field`, while its value looks wrong and the person left it or tried to
    /// save: never before the first input.
    pub fn hint(&self, field: Field) -> Option<&'static str> {
        if !self.tried && !self.left.contains(&field) {
            return None;
        }
        let value = match field {
            Field::Email(i) => self.emails.get(i).map(|l| l.value.as_str()),
            Field::Phone(i) => self.phones.get(i).map(|l| l.value.as_str()),
            Field::Birthday => Some(self.birthday.as_str()),
            _ => None,
        }?;
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        match field {
            Field::Email(_) if !mail_ok(value) => Some(tr("This does not look like a mail address.")),
            Field::Phone(_) if !phone_ok(value) => Some(tr("This does not look like a phone number.")),
            Field::Birthday if PartialDate::parse(value).is_none() => {
                Some(tr("Not a date: write 1990-04-15, or --04-15 without the year. It is not saved."))
            }
            _ => None,
        }
    }
}

/// The name shown for `name`, or the company when there is no name.
pub fn shown_name(name: &Name, org: &str) -> String {
    let full = name.full();
    if full.is_empty() { org.to_owned() } else { full }
}

/// Whether `value` looks like a mail address: one `@` with something before it and a domain with a dot
/// after it, no spaces.
pub fn mail_ok(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else { return false };
    !local.is_empty()
        && !domain.contains('@')
        && !value.chars().any(char::is_whitespace)
        && domain.split('.').count() >= 2
        && domain.split('.').all(|part| !part.is_empty())
}

/// Whether `value` looks like a phone number: digits with at most a leading `+` and the usual signs
/// between them (spaces, `-`, `/`, `.`, brackets), at least three digits.
pub fn phone_ok(value: &str) -> bool {
    let body = value.strip_prefix('+').unwrap_or(value);
    let digits = body.chars().filter(char::is_ascii_digit).count();
    digits >= 3 && body.chars().all(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '/' | '.' | '(' | ')'))
}

/// One line of the conflict view: the field, this device's value, the server's, and whether they differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub label: &'static str,
    pub mine: String,
    pub theirs: String,
    pub differs: bool,
}

/// The fields of `mine` and `theirs` side by side, for the conflict view; fields empty in both are left
/// out.
pub fn differences(mine: &Contact, theirs: &Contact) -> Vec<Difference> {
    fn joined(values: &[Labeled<String>]) -> String {
        values.iter().map(|v| v.value.as_str()).collect::<Vec<_>>().join("\n")
    }
    let places = |c: &Contact| c.addresses.iter().map(|a| a.value.one_line()).collect::<Vec<_>>().join("\n");
    let rows = [
        (tr("Name"), mine.shown_name(), theirs.shown_name()),
        (tr("Company"), mine.org.clone().unwrap_or_default(), theirs.org.clone().unwrap_or_default()),
        (tr("Position"), mine.title.clone().unwrap_or_default(), theirs.title.clone().unwrap_or_default()),
        (tr("Phone"), joined(&mine.phones), joined(&theirs.phones)),
        (tr("Email"), joined(&mine.emails), joined(&theirs.emails)),
        (tr("Address"), places(mine), places(theirs)),
        (tr("Link"), joined(&mine.urls), joined(&theirs.urls)),
        (
            tr("Birthday"),
            mine.birthday.map(|b| b.to_string()).unwrap_or_default(),
            theirs.birthday.map(|b| b.to_string()).unwrap_or_default(),
        ),
        (tr("Notes"), mine.note.clone().unwrap_or_default(), theirs.note.clone().unwrap_or_default()),
        (tr("Groups"), mine.groups.join(", "), theirs.groups.join(", ")),
    ];
    rows.into_iter()
        .filter(|(_, mine, theirs)| !mine.is_empty() || !theirs.is_empty())
        .map(|(label, mine, theirs)| Difference { label, differs: mine != theirs, mine, theirs })
        .collect()
}

// -- The form ------------------------------------------------------------------------------------

/// The anchors of the menus the form opens (the kind of a value, "+ Add field"); new in every `view`.
#[derive(Default)]
pub struct Anchors {
    pub kind: Anchor,
    pub add: Anchor,
}

/// The menu open in the form, if one is: the kind chooser of a value, or "+ Add field".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Kind(List, usize),
    Add,
}

/// A text input of the form, as high as a control, in the suite's framed style.
fn input<'a>(placeholder: &'a str, value: &'a str, field: Field) -> Element<'a, Message> {
    let body = typo::body();
    text_input(placeholder, value)
        .size(body.size)
        .line_height(body.line_height)
        .font(body.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(move |text| Message::Draft(Msg::Text(field, text)))
        .into()
}

/// A line of the form: the label in `meta`/`text_3`, as wide as the labels of the properties, then the
/// value; with `hint` a quiet line under the value.
fn line<'a>(
    label: Element<'a, Message>,
    value: Element<'a, Message>,
    hint: Option<&'static str>,
) -> Element<'a, Message> {
    let mut value = column![value].spacing(space::xs()).width(Fill);
    if let Some(hint) = hint {
        value = value.push(
            row![
                tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.attention),
                typo::text(typo::meta(), hint).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center),
        );
    }
    row![container(label).width(widgets::prop_label_width()), value]
        .spacing(space::s())
        .align_y(Alignment::Start)
        .into()
}

fn label<'a>(text: &'a str) -> Element<'a, Message> {
    container(typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::None))
        .center_y(torro_gui::theme::size::control())
        .into()
}

/// The kind chooser of a value: its word with a chevron, quiet; a click opens the menu of kinds.
fn kind_button<'a>(
    list: List,
    index: usize,
    kind: &FieldKind,
    anchors: &Anchors,
    open: Option<Open>,
) -> Element<'a, Message> {
    let content = row![
        typo::text(typo::meta(), kind_word(kind)).style(theme::text_2).wrapping(Wrapping::None),
        tinted(Icon::ChevronDown, icon::SIZE_SM, |p| p.text_3),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    let pressable =
        button(content).padding(Padding::ZERO).style(style::button::bare).on_press(Message::ToggleKind(list, index));
    let pressable = container(pressable).center_y(torro_gui::theme::size::control());
    if open == Some(Open::Kind(list, index)) { widgets::anchored(&anchors.kind, pressable) } else { pressable.into() }
}

fn remove_button<'a>(list: List, index: usize) -> Element<'a, Message> {
    widgets::icon_button(Icon::X, tr("Remove"), Some(Message::Draft(Msg::Remove(list, index))))
}

/// The form of `draft` in the panel's body.
pub fn form<'a>(draft: &'a Draft, anchors: &Anchors, open: Option<Open>) -> Element<'a, Message> {
    let mut rows = column![].spacing(space::s());
    rows = rows.push(line(label(tr("First name")), input(tr("First name"), &draft.given, Field::Given), None));
    rows = rows.push(line(label(tr("Last name")), input(tr("Last name"), &draft.family, Field::Family), None));
    rows = rows.push(line(label(tr("Company")), input(tr("Company"), &draft.org, Field::Org), None));
    rows = rows.push(line(label(tr("Position")), input(tr("Position"), &draft.title, Field::Title), None));
    let mut sections = column![rows].spacing(space::l());
    for list in List::ALL {
        let mut group = column![].spacing(space::s());
        match list {
            List::Address => {
                for (index, place) in draft.addresses.iter().enumerate() {
                    let parts = column![
                        input(tr("Street"), &place.street, Field::Street(index)),
                        row![
                            container(input(tr("Postal code"), &place.postal_code, Field::Postal(index)))
                                .width(Length::FillPortion(2)),
                            container(input(tr("Town"), &place.locality, Field::Town(index)))
                                .width(Length::FillPortion(3)),
                        ]
                        .spacing(space::s()),
                        row![input(tr("Country"), &place.country, Field::Country(index)), remove_button(list, index)]
                            .spacing(space::xs())
                            .align_y(Alignment::Center),
                    ]
                    .spacing(space::xs());
                    group = group.push(line(kind_button(list, index, &place.kind, anchors, open), parts.into(), None));
                }
            }
            _ => {
                let (lines, field): (&[Line], fn(usize) -> Field) = match list {
                    List::Email => (&draft.emails, Field::Email),
                    List::Phone => (&draft.phones, Field::Phone),
                    _ => (&draft.urls, Field::Url),
                };
                for (index, value) in lines.iter().enumerate() {
                    let field = field(index);
                    let value_row = row![input(list.label(), &value.value, field), remove_button(list, index)]
                        .spacing(space::xs())
                        .align_y(Alignment::Center);
                    group = group.push(line(
                        kind_button(list, index, &value.kind, anchors, open),
                        value_row.into(),
                        draft.hint(field),
                    ));
                }
            }
        }
        // A field without values takes no room ("+ Add field" brings it).
        let filled = match list {
            List::Email => !draft.emails.is_empty(),
            List::Phone => !draft.phones.is_empty(),
            List::Url => !draft.urls.is_empty(),
            List::Address => !draft.addresses.is_empty(),
        };
        if filled {
            sections = sections.push(group);
        }
    }
    let more = column![
        line(label(tr("Birthday")), input("1990-04-15", &draft.birthday, Field::Birthday), draft.hint(Field::Birthday)),
        line(label(tr("Notes")), input(tr("Notes"), &draft.note, Field::Note), None),
    ]
    .spacing(space::s());
    let add =
        widgets::text_button(Some(Icon::Plus), tr("Add field"), style::button::ghost, Some(Message::ToggleAddField));
    let add = if open == Some(Open::Add) { widgets::anchored(&anchors.add, add) } else { add };
    sections = sections.push(more).push(row![Space::new().width(widgets::prop_label_width()), add].spacing(space::s()));
    if let Some(error) = &draft.error {
        sections = sections.push(
            row![
                tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error),
                typo::text(typo::meta(), error.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center),
        );
    }
    torro_gui::panel::inset(sections)
}

/// The two versions of a contact in conflict side by side, the values that differ marked with an
/// icon and in `text` (the same ones quiet in `text_3`).
pub fn conflict<'a>(differences: Vec<Difference>) -> Element<'a, Message> {
    let head = row![
        Space::new().width(widgets::prop_label_width()),
        container(typo::text(typo::caption(), tr("On this device")).style(theme::text_3)).width(Fill),
        container(typo::text(typo::caption(), tr("On the server")).style(theme::text_3)).width(Fill),
    ]
    .spacing(space::s());
    let mut table = column![head].spacing(space::s());
    for difference in differences {
        let value = |text: String, differs: bool| -> Element<'a, Message> {
            let style = if differs { theme::text } else { theme::text_3 };
            let text = if text.is_empty() { "—".to_owned() } else { text };
            container(typo::text(typo::body(), text).style(style).wrapping(Wrapping::WordOrGlyph)).width(Fill).into()
        };
        let mut name = row![typo::text(typo::meta(), difference.label).style(theme::text_3)]
            .spacing(space::xs())
            .align_y(Alignment::Center);
        if difference.differs {
            name = name.push(widgets::with_tooltip(
                tinted(Icon::TriangleAlert, icon::SIZE_SM, |p| p.attention),
                tr("Differs"),
            ));
        }
        table = table.push(
            row![
                container(name).width(widgets::prop_label_width()),
                value(difference.mine, difference.differs),
                value(difference.theirs, difference.differs),
            ]
            .spacing(space::s()),
        );
    }
    torro_gui::panel::inset(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anna() -> Contact {
        Contact {
            id: "b/1".into(),
            book: "b".into(),
            uid: "1".into(),
            name: Name { given: "Anna".into(), family: "Berg".into(), ..Name::default() },
            display_name: "Anna Berg".into(),
            org: Some("Acme".into()),
            emails: vec![Labeled { value: "anna@acme.example".into(), kind: FieldKind::Work, pref: true }],
            phones: vec![Labeled::new("+49 30 1234".into(), FieldKind::Mobile)],
            addresses: vec![Labeled::new(
                PostalAddress {
                    street: "Main 1".into(),
                    locality: "Berlin".into(),
                    region: "BE".into(),
                    ..PostalAddress::default()
                },
                FieldKind::Home,
            )],
            birthday: PartialDate::parse("1990-04-15"),
            etag: Some("\"1\"".into()),
            ..Contact::default()
        }
    }

    #[test]
    fn nothing_changed_sends_nothing() {
        let draft = Draft::of(&anna());
        assert!(draft.changes().is_empty(), "{:?}", draft.changes());
    }

    #[test]
    fn only_what_changed_is_sent() {
        let mut draft = Draft::of(&anna());
        draft.update(Msg::Text(Field::Org, "Acme Ltd".into()));
        draft.update(Msg::Text(Field::Town(0), "Potsdam".into()));
        let changes = draft.changes();
        assert_eq!(changes.names(), ["org", "addresses"]);
        assert_eq!(changes.org.as_deref(), Some("Acme Ltd"));
        let addresses = changes.addresses.expect("the addresses");
        assert_eq!(addresses[0].value.locality, "Potsdam");
        assert_eq!(addresses[0].value.region, "BE", "parts the form does not show stay");
    }

    #[test]
    fn a_new_name_renames_the_shown_name_unless_it_is_its_own() {
        let mut draft = Draft::of(&anna());
        draft.update(Msg::Text(Field::Family, "Klein".into()));
        let changes = draft.changes();
        assert_eq!(changes.display_name.as_deref(), Some("Anna Klein"));
        assert_eq!(changes.name.map(|n| n.family), Some("Klein".into()));
        let mut own = anna();
        own.display_name = "Anni".into();
        let mut draft = Draft::of(&own);
        draft.update(Msg::Text(Field::Family, "Klein".into()));
        assert_eq!(draft.changes().display_name, None, "a name of its own stays");
    }

    #[test]
    fn lists_drop_empty_values_and_replace_as_a_whole() {
        let mut draft = Draft::of(&anna());
        draft.update(Msg::Add(List::Email));
        assert!(draft.changes().emails.is_none(), "an empty new value changes nothing");
        draft.update(Msg::Text(Field::Email(1), "a@b.example".into()));
        draft.update(Msg::Kind(List::Email, 1, FieldKind::Home));
        let emails = draft.changes().emails.expect("emails");
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[1].kind, FieldKind::Home);
        draft.update(Msg::Remove(List::Phone, 0));
        assert_eq!(draft.changes().phones, Some(Vec::new()), "the last number removed empties the field");
    }

    #[test]
    fn a_birthday_that_is_no_date_stays_out() {
        let mut draft = Draft::of(&anna());
        draft.update(Msg::Text(Field::Birthday, "soon".into()));
        assert_eq!(draft.changes().birthday, None);
        draft.update(Msg::Text(Field::Birthday, "--05-01".into()));
        assert_eq!(draft.changes().birthday.as_deref(), Some("--05-01"));
        draft.update(Msg::Text(Field::Birthday, String::new()));
        assert_eq!(draft.changes().birthday.as_deref(), Some(""), "emptied, it is removed");
    }

    #[test]
    fn checks_show_after_leaving_the_field_or_trying_to_save() {
        let mut draft = Draft::of(&anna());
        draft.update(Msg::Text(Field::Email(0), "anna@".into()));
        assert_eq!(draft.hint(Field::Email(0)), None, "not while typing the first time");
        draft.update(Msg::Text(Field::Phone(0), "call me".into()));
        assert!(draft.hint(Field::Email(0)).is_some(), "left, it shows");
        assert_eq!(draft.hint(Field::Phone(0)), None);
        draft.tried = true;
        assert!(draft.hint(Field::Phone(0)).is_some());
        assert!(!draft.changes().is_empty(), "saving stays possible");
    }

    #[test]
    fn mail_and_phone_checks() {
        for good in ["a@b.de", "anna.berg+x@mail.example.org"] {
            assert!(mail_ok(good), "{good}");
        }
        for bad in ["a@b", "@b.de", "a b@c.de", "a@@b.de", "a@b..de"] {
            assert!(!mail_ok(bad), "{bad}");
        }
        for good in ["+49 30 1234-56", "(030) 123 45", "0171/2345678"] {
            assert!(phone_ok(good), "{good}");
        }
        for bad in ["12", "+49 call", "1+2345"] {
            assert!(!phone_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_new_contact_needs_something() {
        let mut draft = Draft::empty();
        assert!(draft.is_blank());
        draft.update(Msg::Text(Field::Given, "Max".into()));
        assert!(!draft.is_blank());
        let changes = draft.changes();
        assert_eq!(changes.display_name.as_deref(), Some("Max"));
        assert_eq!(changes.emails, None, "the empty starting values send nothing");
    }

    #[test]
    fn the_conflict_marks_what_differs() {
        let mine = anna();
        let mut theirs = anna();
        theirs.phones[0].value = "+49 30 9999".into();
        let rows = differences(&mine, &theirs);
        let phone = rows.iter().find(|d| d.label == tr("Phone")).expect("phone");
        assert!(phone.differs);
        assert!(rows.iter().filter(|d| d.label != tr("Phone")).all(|d| !d.differs));
        assert!(rows.iter().all(|d| d.label != tr("Notes")), "empty in both, left out");
    }
}
