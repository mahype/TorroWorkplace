//! The contact in the side panel (`torro_gui::panel`): beside the list in a wide window, over it in
//! the others. Head with picture, name, position and company; quick buttons Mail (`mailto:`) and Call
//! (`tel:`), both through the system's opener; then a section per kind of field. The "…" of the head
//! holds Copy, Favourite, Move to another book and Delete.
//!
//! Links open only with the schemes [`SAFE_SCHEMES`] (`docs/kontakte.md`, "Sicherheit"): everything in
//! a contact is text of other people, so any other link stays plain text.
//!
//! Under each address stands a small map ([`AddressMap`], issue #355): the picture with its pin, a
//! click on it or ↗ opens the maps app; or, where there is no map, one quiet line saying why with
//! "Open in Maps" beside it. The core looks the address up; the window only asks.
//!
//! The marks of the contact stand above its fields: "Read-only address book" (and no "Edit"), "Saved on
//! this device, not yet on the server", and for a conflict "Changed elsewhere" with both versions
//! ([`crate::edit::conflict`]).

use std::process::Command;

use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, text_input};
use iced::{Alignment, Background, Element, Fill, Theme, border};
use torro_contacts::{AddressBook, Contact, ContactState, Labeled};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, palette, radius, size, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{datetime, style, typo};

use crate::app::Message;
use crate::edit::{self, kind_word};
use crate::list::{face, role_line, tinted};

/// The schemes a link of a contact may open with; any other stays text.
pub const SAFE_SCHEMES: [&str; 6] = ["https", "mailto", "tel", "sms", "xmpp", "matrix"];

/// The field of the panel's group input.
pub const GROUP_INPUT: &str = "contacts-group";

/// `link` when its scheme is one of [`SAFE_SCHEMES`] and it has something after the scheme; `None`
/// otherwise. A link with spaces, control characters or characters that turn or hide text (bidi
/// marks, zero-width signs) is refused, so what the panel shows is where the link goes; `mailto:` and
/// `sms:` take no query, so a contact's field cannot fill in a message (`?body=…&cc=…`).
pub fn safe_link(link: &str) -> Option<&str> {
    let link = link.trim();
    let (scheme, rest) = link.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    let rest = rest.trim_start_matches('/');
    let hidden = |c: char| {
        c.is_whitespace()
            || c.is_control()
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
    };
    let query = matches!(scheme.as_str(), "mailto" | "sms") && rest.contains(['?', '&']);
    // A number to call or text is a number: a `+` in front and digits, nothing a phone would dial as a
    // code (`tel:*21*123#`).
    let number = |rest: &str| {
        let digits = rest.strip_prefix('+').unwrap_or(rest);
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    };
    let not_a_number = matches!(scheme.as_str(), "tel" | "sms") && !number(rest);
    let safe = SAFE_SCHEMES.contains(&scheme.as_str()) && !rest.is_empty() && !link.chars().any(hidden);
    (safe && !query && !not_a_number).then_some(link)
}

/// The `tel:` address of a number: only `+` (in front) and digits stay, so a text cannot become a
/// command. `None` without three digits.
pub fn tel_link(number: &str) -> Option<String> {
    let plus = number.trim_start().starts_with('+');
    let digits: String = number.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= 3).then(|| format!("tel:{}{digits}", if plus { "+" } else { "" }))
}

/// The `mailto:` address of a mail address, `None` for one with spaces or without `@`.
pub fn mail_link(address: &str) -> Option<String> {
    let address = address.trim();
    (address.contains('@') && !address.chars().any(|c| c.is_whitespace() || c == '?' || c == '&'))
        .then(|| format!("mailto:{address}"))
}

/// Opens `uri` (`mailto:`, `tel:`, a safe link) with the system's program for it: `open` on macOS, the
/// Explorer on Windows, `xdg-open` elsewhere. Only links [`safe_link`] lets through. An error is a
/// sentence for people.
pub fn open_uri(uri: &str) -> Result<(), String> {
    let Some(uri) = safe_link(uri) else { return Err(tr("This link cannot be opened.").to_owned()) };
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    Command::new(program).arg(uri).spawn().map(|_| ()).map_err(|_| tr("The link could not be opened.").to_owned())
}

/// The map under one address of the open contact (issue #355), in the order the plan's picture
/// names them.
#[derive(Debug, Clone, PartialEq)]
pub enum AddressMap {
    /// Asking the core, or the picture of a place found is on its way: "Finding address…".
    Finding,
    /// Found: the picture.
    Shown(Handle),
    /// The place search found no place: "Address not found on the map".
    NotFound,
    /// The lookup of contact addresses is off (maps are on): "Turn on address lookup to see a map".
    LookupOff,
    /// Maps are off: "Turn on maps to see a map".
    MapsOff,
    /// The service is busy or cannot be reached: "The map is not available right now."
    Unavailable,
}

/// What the panel shows besides the contact.
pub struct Look<'a> {
    pub picture: Option<&'a Handle>,
    pub colours: Palette,
    pub today: chrono::NaiveDate,
    /// What the group input holds, and the names it may suggest.
    pub group_text: &'a str,
    pub suggestions: Vec<String>,
    /// A write is out: the buttons that change the contact wait.
    pub busy: bool,
    /// Why the last change failed, as one line.
    pub error: Option<&'a str>,
    /// The map under each address, by its place in `addresses`; an address without one shows none.
    pub maps: &'a [AddressMap],
}

/// The panel's title: the picture, the name and under it position · company.
pub fn title<'a>(contact: &'a Contact, look: &Look<'a>) -> Element<'a, Message> {
    let name = contact.shown_name();
    let role = role_line(contact.title.as_deref(), contact.org.as_deref());
    let mut words = column![typo::text(typo::title(), name.clone()).wrapping(Wrapping::WordOrGlyph)];
    if !role.is_empty() {
        words = words.push(typo::text(typo::meta(), role).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    row![face(&name, look.picture, size::avatar_lg(), &look.colours), words.width(Fill)]
        .spacing(space::m())
        .align_y(Alignment::Center)
        .into()
}

/// A band at the top of the body: `icon` in `tone`, `text`, on `raised` with a frame, as the suite's
/// question cards.
fn band<'a>(which: Icon, tone: fn(&Palette) -> iced::Color, text: &'a str) -> Element<'a, Message> {
    container(
        row![tinted(which, icon::SIZE, tone), typo::text(typo::body(), text).wrapping(Wrapping::WordOrGlyph)]
            .spacing(space::s())
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(space::m())
    .style(|t: &Theme| {
        let p = palette(t);
        container::Style {
            background: Some(Background::Color(p.raised)),
            border: border::rounded(radius::sm()).color(p.line_strong).width(size::HAIRLINE),
            ..container::Style::default()
        }
    })
    .into()
}

/// The body of a contact being read (see the module docs).
pub fn body<'a>(contact: &'a Contact, look: Look<'a>) -> Element<'a, Message> {
    let mut parts = column![].spacing(space::l());
    if contact.readonly {
        parts = parts.push(torro_gui::panel::inset(band(Icon::Lock, |p| p.text_3, tr("Read-only address book"))));
    }
    match contact.state {
        ContactState::Synced => {}
        ContactState::Pending => {
            parts = parts.push(torro_gui::panel::inset(band(
                Icon::Clock,
                |p| p.text_3,
                tr("Saved on this device, not yet on the server"),
            )));
        }
        ContactState::Conflict => {
            let keep = row![
                widgets::text_button(
                    None,
                    tr("Keep mine"),
                    style::button::secondary,
                    (!look.busy).then_some(Message::Resolve(torro_contacts::Keep::Mine))
                ),
                widgets::text_button(
                    None,
                    tr("Keep theirs"),
                    style::button::secondary,
                    (!look.busy).then_some(Message::Resolve(torro_contacts::Keep::Theirs))
                ),
                widgets::text_button(
                    None,
                    tr("Keep both"),
                    style::button::secondary,
                    (!look.busy).then_some(Message::Resolve(torro_contacts::Keep::Both))
                ),
            ]
            .spacing(space::s())
            .wrap();
            parts = parts.push(torro_gui::panel::inset(
                column![
                    band(
                        Icon::TriangleAlert,
                        |p| p.attention,
                        tr("Changed elsewhere: this device and the server differ.")
                    ),
                    keep
                ]
                .spacing(space::s()),
            ));
            if let Some(server) = &contact.server {
                parts = parts.push(edit::conflict(edit::differences(contact, server)));
            }
        }
    }
    if let Some(error) = look.error {
        parts = parts.push(torro_gui::panel::inset(band(Icon::CircleAlert, |p| p.error, error)));
    }
    let mut quick = row![].spacing(space::s());
    if let Some(mail) = contact.emails.iter().find(|m| m.pref).or(contact.emails.first()) {
        quick = quick.push(widgets::text_button(
            Some(Icon::Mail),
            tr("Mail"),
            style::button::secondary,
            mail_link(&mail.value).map(Message::OpenUri),
        ));
    }
    if let Some(phone) = contact.phones.iter().find(|p| p.pref).or(contact.phones.first()) {
        quick = quick.push(widgets::text_button(
            Some(Icon::Phone),
            tr("Call"),
            style::button::secondary,
            tel_link(&phone.value).map(Message::OpenUri),
        ));
    }
    parts = parts.push(torro_gui::panel::inset(quick));
    let section = |title: &'static str, rows: Vec<Element<'a, Message>>| -> Option<Element<'a, Message>> {
        (!rows.is_empty()).then(|| widgets::prop_section(title, rows))
    };
    let values = |values: &'a [Labeled<String>], link: fn(&str) -> Option<String>| -> Vec<Element<'a, Message>> {
        values.iter().map(|value| field_line(kind_word(&value.kind), &value.value, link(&value.value))).collect()
    };
    let mut addresses: Vec<Element<'a, Message>> = Vec::new();
    for (index, address) in contact.addresses.iter().enumerate() {
        addresses.push(field_line(kind_word(&address.kind), &address.value.one_line(), None));
        if let Some(map) = look.maps.get(index) {
            addresses.push(torro_gui::panel::inset(address_map(map, index, look.colours.text)));
        }
    }
    let mut dates = Vec::new();
    if let Some(birthday) = contact.birthday {
        dates.push(field_line(tr("Birthday").to_owned(), &date_words(birthday, look.today), None));
    }
    if let Some(anniversary) = contact.anniversary {
        dates.push(field_line(tr("Anniversary").to_owned(), &date_words(anniversary, look.today), None));
    }
    let sections = [
        section(tr("Phone"), values(&contact.phones, tel_link)),
        section(tr("Email"), values(&contact.emails, mail_link)),
        section(tr("Address"), addresses),
        section(tr("Links"), values(&contact.urls, |url| safe_link(url).map(str::to_owned))),
        section(tr("Messengers"), values(&contact.impps, |handle| safe_link(handle).map(str::to_owned))),
        section(tr("Dates"), dates),
    ];
    for part in sections.into_iter().flatten() {
        parts = parts.push(part);
    }
    if let Some(note) = contact.note.as_deref().filter(|n| !n.trim().is_empty()) {
        parts = parts.push(widgets::prop_section(
            tr("Notes"),
            vec![torro_gui::panel::inset(typo::text(typo::body(), note).wrapping(Wrapping::WordOrGlyph))],
        ));
    }
    parts = parts.push(widgets::prop_section(tr("Groups"), vec![groups(contact, &look)]));
    parts.into()
}

/// The map under the address number `index` ([`AddressMap`]): the picture with its pin in `pin` and
/// ↗ "Open in Maps" over it, a click on it opens the maps app too; else a quiet line why there is
/// none, with "Open in Maps" beside it (the maps app finds the address as text). The lines of the
/// switches lead to Settings › General of the control centre.
pub fn address_map(map: &AddressMap, index: usize, pin: iced::Color) -> Element<'_, Message> {
    let open = Message::OpenInMaps(index);
    let words = match map {
        AddressMap::Shown(picture) => {
            let credit = Message::OpenUri(torro_gui::map::OSM_COPYRIGHT.to_owned());
            let map = torro_gui::map::map_view(picture, pin, open.clone(), credit);
            return iced::widget::mouse_area(map).on_press(open).interaction(iced::mouse::Interaction::Pointer).into();
        }
        AddressMap::Finding => return quiet(tr("Finding address…")),
        AddressMap::NotFound => quiet(tr("Address not found on the map")),
        AddressMap::Unavailable => quiet(tr("The map is not available right now.")),
        AddressMap::LookupOff => widgets::text_button_trailing(
            tr("Turn on address lookup to see a map"),
            Icon::ChevronRight,
            style::button::ghost,
            Some(Message::OpenMapSettings),
        ),
        AddressMap::MapsOff => widgets::text_button_trailing(
            tr("Turn on maps to see a map"),
            Icon::ChevronRight,
            style::button::ghost,
            Some(Message::OpenMapSettings),
        ),
    };
    let open = widgets::text_button(Some(Icon::ArrowUpRight), tr("Open in Maps"), style::button::ghost, Some(open));
    row![words, Space::new().width(Fill), open].spacing(space::s()).align_y(Alignment::Center).wrap().into()
}

/// A quiet line under an address: `meta` in `text_3`, as high as a button so the states do not jump.
fn quiet<'a>(words: &'static str) -> Element<'a, Message> {
    container(typo::text(typo::meta(), words).style(theme::text_3).wrapping(Wrapping::WordOrGlyph))
        .height(size::control_sm())
        .align_y(Alignment::Center)
        .into()
}

/// A value of a field: its kind as the label, the value – wrapping, so a long address or date reads
/// whole –; with `link` the value opens it.
fn field_line<'a>(kind: String, value: &str, link: Option<String>) -> Element<'a, Message> {
    let shown: Element<'a, Message> =
        typo::text(typo::body(), value.to_owned()).wrapping(Wrapping::Word).width(Fill).into();
    match link {
        Some(link) => widgets::prop_line(kind, None, shown, Some(Message::OpenUri(link))),
        None => widgets::prop_line(kind, None, shown, None),
    }
}

/// "15 April 1990 · turns 37 on Wed, 15 Apr" or, without a year, "15 April".
pub fn date_words(date: torro_contacts::PartialDate, today: chrono::NaiveDate) -> String {
    let day = edit::date_text(date);
    let Some(next) = date.next_on_or_after(today) else { return day };
    use chrono::Datelike;
    match date.year {
        Some(year) if next.year() > year && next == today => {
            trf("{date} · turns {n} today", &[("date", &day), ("n", &(next.year() - year))])
        }
        Some(year) if next.year() > year => trf(
            "{date} · turns {n} on {day}",
            &[("date", &day), ("n", &(next.year() - year)), ("day", &datetime::format_short_date(next, today))],
        ),
        _ => day,
    }
}

/// The groups as chips, each with ✕ when the book may be written, then a field to add one with the
/// book's other groups as suggestions. Changes go to the server at once.
fn groups<'a>(contact: &'a Contact, look: &Look<'a>) -> Element<'a, Message> {
    let writable = !contact.readonly && contact.state != ContactState::Conflict;
    let mut chips: Vec<Element<'a, Message>> = contact
        .groups
        .iter()
        .map(|group| {
            // A group of a read-only book has no ✕ at all, not one that is off.
            if !writable {
                return widgets::chip(Some(Mark::Icon(Icon::Tag)), group.as_str(), None);
            }
            let lead: Element<'a, Message> = tinted(Icon::Tag, icon::SIZE_SM, |p| p.text_3);
            let remove = (!look.busy).then(|| Message::RemoveGroup(group.clone()));
            widgets::chip_removable(lead, group.as_str(), None, tr("Remove from group"), remove)
        })
        .collect();
    if chips.is_empty() && !writable {
        chips.push(typo::text(typo::body(), tr("No group")).style(theme::text_3).into());
    }
    let mut part = column![widgets::chip_row(chips)].spacing(space::s());
    if writable {
        let body = typo::body();
        let input = text_input(tr("Add to group"), look.group_text)
            .id(GROUP_INPUT)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::GroupText)
            .on_submit(Message::AddGroup(look.group_text.trim().to_owned()));
        part = part.push(input);
        if !look.suggestions.is_empty() {
            let suggestions = look
                .suggestions
                .iter()
                .map(|name| {
                    widgets::chip(
                        Some(Mark::Icon(Icon::Plus)),
                        name.clone(),
                        (!look.busy).then(|| Message::AddGroup(name.clone())),
                    )
                })
                .collect();
            part = part.push(widgets::chip_row(suggestions));
        }
    }
    torro_gui::panel::inset(part)
}

/// The groups `typed` may mean: the names of `known` (the groups of the contact's book) not on the
/// contact yet, that contain what is typed (all of them while nothing is typed), at most five.
pub fn suggestions(known: &[String], on: &[String], typed: &str) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    known
        .iter()
        .filter(|name| !on.iter().any(|g| crate::app::same_group(g, name)))
        .filter(|name| typed.is_empty() || name.to_lowercase().contains(&typed))
        .take(5)
        .cloned()
        .collect()
}

/// The path of the panel's head: the book the contact lives in.
pub fn path<'a>(book: Option<&AddressBook>) -> Element<'a, Message> {
    let name = book.map(|b| b.display_name.clone()).unwrap_or_default();
    torro_gui::panel::path(Some(Mark::Icon(Icon::BookUser)), [name])
}

/// The quiet button "Choose picture…" / "Remove picture" row of the form.
pub fn picture_buttons<'a>(has: bool, busy: bool) -> Element<'a, Message> {
    let mut buttons =
        row![edit::quiet_button(Icon::Image, tr("Choose picture…"), (!busy).then_some(Message::ChoosePhoto))]
            .spacing(space::l());
    if has {
        buttons =
            buttons.push(edit::quiet_button(Icon::X, tr("Remove picture"), (!busy).then_some(Message::RemovePhoto)));
    }
    let hint = typo::text(typo::meta(), tr("Or drop a picture on the window.")).style(theme::text_3);
    torro_gui::panel::inset(column![buttons, hint].spacing(space::xs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_schemes_open() {
        for good in ["https://example.org", "mailto:a@b.de", "tel:+4930", "sms:+49", "xmpp:a@b.org", "matrix:u/a:b.org"]
        {
            assert_eq!(safe_link(good), Some(good), "{good}");
        }
        for bad in [
            "http://example.org",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https:",
            "https://a b",
            "ftp://x",
            "mailto:x@y.org?body=hi&cc=z@y.org",
            "sms:+49?body=hi",
            "tel:*21*123#",
            "tel:+49 30 1234",
            "sms:12a",
            "https://example.org/\u{202E}gpj.exe",
            "https://exa\u{200B}mple.org",
            "https://example.org/\u{0}",
        ] {
            assert_eq!(safe_link(bad), None, "{bad}");
        }
    }

    #[test]
    fn numbers_and_addresses_become_links() {
        assert_eq!(tel_link("+49 (30) 123-45").as_deref(), Some("tel:+493012345"));
        assert_eq!(tel_link("0171 / 234"), Some("tel:0171234".into()));
        assert_eq!(tel_link("12"), None);
        assert_eq!(tel_link("+49; rm -rf /"), None, "too few digits");
        assert_eq!(mail_link(" anna@example.org ").as_deref(), Some("mailto:anna@example.org"));
        assert_eq!(mail_link("anna@example.org?subject=x"), None);
        assert_eq!(mail_link("no address"), None);
    }

    #[test]
    fn group_suggestions_leave_out_what_the_contact_has() {
        let known = vec!["Family".to_owned(), "Friends".to_owned(), "Work".to_owned()];
        assert_eq!(suggestions(&known, &["Work".to_owned()], ""), ["Family", "Friends"]);
        assert_eq!(suggestions(&known, &[], "fr"), ["Friends"]);
        assert!(suggestions(&known, &[], "zz").is_empty());
    }

    #[test]
    fn dates_say_how_old_the_person_turns() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).expect("date");
        let with = torro_contacts::PartialDate::parse("1990-04-15").expect("date");
        let words = date_words(with, today);
        assert!(words.contains("1990") && words.contains("36"), "{words}");
        let without = torro_contacts::PartialDate::parse("--04-15").expect("date");
        assert!(!date_words(without, today).contains("turns"));
    }
}
