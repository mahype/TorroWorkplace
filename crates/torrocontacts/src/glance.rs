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
use iced::widget::{button, column, container, row, stack, text_input};
use iced::{Alignment, Element, Fill, Padding};
use torro_contacts::{Birthday, ContactSummary};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::shell;
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

/// Below this height the mini tile drops its search field and shows only the favourites.
pub fn search_from() -> f32 {
    torro_gui::layout::STRIP_MAX_HEIGHT
}

/// How many rows of `row` height fit into `height` after `taken`; at least none.
pub fn rows_fitting(height: f32, taken: f32, row: f32) -> usize {
    if row <= 0.0 { 0 } else { ((height - taken).max(0.0) / row).floor() as usize }
}

/// The mini tile (see the module docs).
pub fn mini(app: &App) -> Element<'_, Message> {
    let room = shell::below_titlebar(app.size, app.zoom.factor());
    let tall = room.height >= search_from();
    let shown = favorites(&app.contacts, if tall { &app.search } else { "" });
    let colours = palette(&app.theme());
    let mut parts = column![].spacing(space::s());
    let mut taken = 2.0 * space::m();
    if tall {
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
        parts = parts.push(stack![input, glass]);
        parts = parts.push(typo::text(typo::caption(), tr("Favorites")).style(theme::text_3));
        taken += size::control() + typo::line_height(typo::caption()) + 2.0 * space::s();
    }
    if shown.is_empty() {
        let line =
            if app.search.trim().is_empty() || !tall { tr("No favorites yet.") } else { tr("No favorite matches.") };
        parts = parts.push(typo::text(typo::body(), line).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    } else {
        // Only rows that fit whole: a display does not scroll.
        let fit = rows_fitting(room.height, taken, size::row_sidebar() + space::xs()).max(1);
        let rows = shown.into_iter().take(fit).map(|contact| {
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
        parts = parts.push(column(rows).spacing(space::xs()));
    }
    container(parts).padding(space::m()).width(Fill).height(Fill).clip(true).style(style::container::panel_flush).into()
}

/// The strip (see the module docs): the next birthdays as cells side by side, each at most
/// [`cell_max`] wide – icon, the name, "turns 40 · Tue 18" under it.
pub fn strip(app: &App) -> Element<'_, Message> {
    let room = shell::below_titlebar(app.size, app.zoom.factor());
    let next = next_birthdays(&app.birthdays, cells_fitting(room.width));
    let content: Element<'_, Message> = if next.is_empty() {
        row![
            tinted(Icon::Cake, icon::SIZE, |p| p.text_3),
            typo::text(typo::body(), tr("No birthdays in the next year")).style(theme::text_3).wrapping(Wrapping::None),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    } else {
        let cells = next.into_iter().map(|birthday| {
            let cell = row![
                tinted(Icon::Cake, icon::SIZE, |p| p.text_3),
                column![
                    widgets::ellipsized(typo::body_strong(), birthday.contact.display_name.clone(), theme::text),
                    widgets::ellipsized(typo::meta(), birthday_words(birthday, app.today), theme::text_2),
                ]
                .width(Fill),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center);
            container(cell).width(Fill).max_width(cell_max()).into()
        });
        row(cells).spacing(space::l()).align_y(Alignment::Center).into()
    };
    container(content)
        .padding(Padding { top: space::s(), bottom: space::s(), left: space::m(), right: space::m() })
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .style(style::container::panel_flush)
        .into()
}

/// The widest a cell of the strip gets.
pub fn cell_max() -> f32 {
    size::props()
}

/// How many cells the strip shows at `width`: one per [`cell_max`], one to three.
pub fn cells_fitting(width: f32) -> usize {
    ((width / cell_max()).floor() as usize).clamp(1, 3)
}

/// The `n` next birthdays, the soonest first.
pub fn next_birthdays(birthdays: &[Birthday], n: usize) -> Vec<&Birthday> {
    let mut sorted: Vec<&Birthday> = birthdays.iter().collect();
    sorted.sort_by_key(|b| b.next);
    sorted.truncate(n);
    sorted
}

/// "turns 40 · Tue 18" under a name of the strip, "turns 40 today" for today; without a year of birth
/// only the day.
pub fn birthday_words(birthday: &Birthday, today: chrono::NaiveDate) -> String {
    let today_ = birthday.next == today;
    let day = torro_gui::datetime::format_short_date(birthday.next, today);
    match (birthday.turns, today_) {
        (Some(turns), true) => trf("turns {n} today", &[("n", &turns)]),
        (Some(turns), false) => trf("turns {n} · {day}", &[("n", &turns), ("day", &day)]),
        (None, true) => tr("Today").to_owned(),
        (None, false) => day,
    }
}

/// A tile's line after a failed load: the alert, the reason cut to fit; a click tries again.
pub fn failed<'a>(err: &str) -> Element<'a, Message> {
    let line = row![
        tinted(Icon::CircleAlert, icon::SIZE, |p| p.error),
        container(widgets::ellipsized(typo::body(), err.to_owned(), theme::text)).width(Fill),
        tinted(Icon::RefreshCw, icon::SIZE_SM, |p| p.text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let pressable = button(line).width(Fill).padding(space::s()).style(style::button::ghost).on_press(Message::Retry);
    container(widgets::with_tooltip(pressable, tr("Retry")))
        .padding(space::s())
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .style(style::container::panel_flush)
        .into()
}

/// A tile without an account with an address book: the book and the one sentence.
pub fn no_account<'a>() -> Element<'a, Message> {
    let line = row![
        tinted(Icon::BookUser, icon::SIZE, |p| p.text_3),
        typo::text(typo::body(), tr("Connect an account in Torro Workplace"))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    container(line)
        .padding(space::m())
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
    fn the_strip_names_the_next_birthdays() {
        let contacts = demo::sample(today()).1;
        let birthdays: Vec<Birthday> = contacts.iter().filter_map(|c| Birthday::upcoming(c, today(), 366)).collect();
        let next = next_birthdays(&birthdays, 3);
        assert_eq!(next.len(), 3);
        assert_eq!(next[0].next, today(), "the sample has one today");
        assert!(next.windows(2).all(|w| w[0].next <= w[1].next));
        let words = birthday_words(next[0], today());
        assert_eq!(words, trf("turns {n} today", &[("n", &next[0].turns.expect("a year"))]));
        assert!(next_birthdays(&[], 3).is_empty());
        assert_eq!((cells_fitting(200.0), cells_fitting(700.0), cells_fitting(4000.0)), (1, 2, 3));
    }

    #[test]
    fn only_whole_rows_fit() {
        assert_eq!(rows_fitting(160.0, 24.0, 36.0), 3);
        assert_eq!(rows_fitting(20.0, 24.0, 36.0), 0);
    }
}
