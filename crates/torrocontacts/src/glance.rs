//! Mini and strip: the contacts as a glance at a small tile (task KO1.8; design skill, section 6; the
//! pattern is `torroinbox/src/glance.rs`).
//!
//! - **Mini** ([`mini`]): a search field, under it the favourites – those matching the search, once
//!   something is typed – as rows: the face, the name cut to fit, Mail and Call at the right (each only
//!   with a value). Without favourites a calm line says so.
//! - **Strip** ([`strip`]): the next birthday on one line, "Lena Fischer turns 40 · Tue 18", or "No
//!   birthdays in the next year".
//!
//! Both are displays, not small apps: no header, sidebar, menu or dialog. What they show is chosen by
//! plain functions with tests ([`favorites`], [`next_birthday`]).

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, scrollable, stack, text_input};
use iced::{Alignment, Element, Fill, Padding};
use torro_contacts::{Birthday, ContactSummary};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::{style, typo, widgets};

use crate::app::{App, Message, SEARCH};
use crate::contact;
use crate::list::{self, face, tinted};

/// The favourites the mini tile lists: matching `search` (name, mail, number) when something is typed,
/// by name.
pub fn favorites<'a>(contacts: &'a [ContactSummary], search: &str) -> Vec<&'a ContactSummary> {
    let typed = search.trim().to_lowercase();
    let mut shown: Vec<&ContactSummary> = contacts
        .iter()
        .filter(|c| c.favorite)
        .filter(|c| {
            typed.is_empty()
                || [Some(&c.display_name), c.email.as_ref(), c.phone.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|text| text.to_lowercase().contains(&typed))
        })
        .collect();
    shown.sort_by_key(|c| list::sort_key(c));
    shown
}

/// The birthday the strip names: the soonest.
pub fn next_birthday(birthdays: &[Birthday]) -> Option<&Birthday> {
    birthdays.iter().min_by_key(|b| b.next)
}

/// The strip's sentence for `birthday`: "Lena Fischer turns 40 · Tue 18", without a year of birth
/// "Lena Fischer · Tue 18"; "today" for today.
pub fn birthday_line(birthday: &Birthday, today: chrono::NaiveDate) -> String {
    let day = if birthday.next == today {
        tr("today").to_owned()
    } else {
        torro_gui::datetime::format_short_date(birthday.next, today)
    };
    let name = birthday.contact.display_name.as_str();
    match birthday.turns {
        Some(turns) => trf("{name} turns {n} · {day}", &[("name", &name), ("n", &turns), ("day", &day)]),
        None => trf("{name} · {day}", &[("name", &name), ("day", &day)]),
    }
}

/// The mini tile (see the module docs).
pub fn mini(app: &App) -> Element<'_, Message> {
    let body = typo::body();
    let lead = style::INPUT_PADDING.left + icon::SIZE + space::s();
    let input = text_input(tr("Search favorites"), &app.search)
        .id(SEARCH)
        .size(body.size)
        .line_height(body.line_height)
        .font(body.font)
        .padding(Padding { left: lead, ..style::INPUT_PADDING })
        .width(Fill)
        .style(style::text_input_quiet)
        .on_input(Message::Search);
    let glass = container(tinted(Icon::Search, icon::SIZE, |p| p.text_3))
        .height(Fill)
        .center_y(Fill)
        .padding(Padding::ZERO.left(style::INPUT_PADDING.left));
    let search = stack![input, glass];
    let shown = favorites(&app.contacts, &app.search);
    let colours = palette(&app.theme());
    let content: Element<'_, Message> = if shown.is_empty() {
        let line = if app.search.trim().is_empty() { tr("No favorites yet.") } else { tr("No favorite matches.") };
        container(typo::text(typo::body(), line).style(theme::text_3).wrapping(Wrapping::WordOrGlyph))
            .padding(space::s())
            .into()
    } else {
        let rows = shown.into_iter().map(|contact| {
            let picture = app.photos.get(&contact.id).and_then(|(_, p)| p.as_ref());
            let mut line = row![
                face(&contact.display_name, picture, size::tile(), &colours),
                container(widgets::ellipsized(typo::body(), contact.display_name.clone(), theme::text)).width(Fill),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center);
            if let Some(link) = contact.email.as_deref().and_then(contact::mail_link) {
                line = line.push(widgets::icon_button(Icon::Mail, tr("Mail"), Some(Message::OpenUri(link))));
            }
            if let Some(link) = contact.phone.as_deref().and_then(contact::tel_link) {
                line = line.push(widgets::icon_button(Icon::Phone, tr("Call"), Some(Message::OpenUri(link))));
            }
            container(line).center_y(size::row_sidebar()).into()
        });
        scrollable(column(rows).spacing(space::xs()))
            .height(Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .into()
    };
    container(column![search, content].spacing(space::s()))
        .padding(space::m())
        .width(Fill)
        .height(Fill)
        .style(style::container::panel_flush)
        .into()
}

/// The strip (see the module docs).
pub fn strip(app: &App) -> Element<'_, Message> {
    let line: Element<'_, Message> = match next_birthday(&app.birthdays) {
        Some(birthday) => row![
            tinted(Icon::Cake, icon::SIZE, |p| p.text_3),
            widgets::ellipsized(typo::body_strong(), birthday_line(birthday, app.today), theme::text),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into(),
        None => row![
            tinted(Icon::Cake, icon::SIZE, |p| p.text_3),
            typo::text(typo::body(), tr("No birthdays in the next year")).style(theme::text_3).wrapping(Wrapping::None),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into(),
    };
    container(row![line, Space::new().width(Fill)])
        .padding(Padding { top: space::s(), bottom: space::s(), left: space::m(), right: space::m() })
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .style(style::container::panel_flush)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_contacts::demo;

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 28).expect("date")
    }

    #[test]
    fn the_mini_tile_lists_the_matching_favorites_by_name() {
        let summaries: Vec<ContactSummary> = demo::sample(today()).1.iter().map(|c| c.summary()).collect();
        let all = favorites(&summaries, "");
        assert_eq!(all.len(), 5);
        assert!(all.windows(2).all(|w| list::sort_key(w[0]) <= list::sort_key(w[1])));
        let found = favorites(&summaries, "berg");
        assert_eq!(found.iter().map(|c| c.display_name.as_str()).collect::<Vec<_>>(), ["Anna Berg"]);
        assert!(favorites(&summaries, "nobody").is_empty());
    }

    #[test]
    fn the_strip_names_the_next_birthday() {
        let contacts = demo::sample(today()).1;
        let birthdays: Vec<Birthday> = contacts.iter().filter_map(|c| Birthday::upcoming(c, today(), 366)).collect();
        let next = next_birthday(&birthdays).expect("one");
        assert_eq!(next.next, today(), "the sample has one today");
        let line = birthday_line(next, today());
        assert!(line.contains(&next.contact.display_name) && line.contains(tr("today")), "{line}");
        assert!(next_birthday(&[]).is_none());
    }
}
