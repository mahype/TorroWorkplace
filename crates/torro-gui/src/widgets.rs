//! Small building blocks shared by the apps.
//!
//! Task K.1 adds the colour dot, the empty state and the section title here. Toolbar buttons and
//! sidebar rows come with the views that need them.

use iced::widget::{center, column, container, mouse_area, opaque, space, stack, text};
use iced::{Alignment, Background, Color, Element, Length, Shadow, Vector, border};

use crate::theme;

/// `#RRGGBB` (or `#RRGGBBAA`, alpha ignored) as a colour; `None` for anything else.
/// A dialog over `base`: the background is dimmed (Iced has no backdrop blur, so dimming
/// does the job of focusing), the dialog gets a soft shadow, and a click next to it sends
/// `on_dismiss`. The dialog draws its own card (radius 12) and is centred at most
/// `max_width` wide; Escape is the dialog's job.
pub fn modal<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    dialog: impl Into<Element<'a, M>>,
    max_width: f32,
    on_dismiss: M,
) -> Element<'a, M> {
    let scrim = mouse_area(container(space::horizontal()).width(Length::Fill).height(Length::Fill).style(|_| {
        container::Style { background: Some(Background::Color(theme::SCRIM)), ..container::Style::default() }
    }))
    .on_press(on_dismiss);
    let card = container(dialog).max_width(max_width).style(|_| container::Style {
        border: border::rounded(12),
        shadow: Shadow { color: theme::SHADOW, offset: Vector::new(0.0, 8.0), blur_radius: 32.0 },
        ..container::Style::default()
    });
    stack![base.into(), opaque(scrim), center(opaque(card)).padding(16)].into()
}

pub fn color_from_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

/// A collection's colour, or a neutral blue when the server reports none.
pub fn collection_color(color: Option<&str>) -> Color {
    color.and_then(color_from_hex).unwrap_or(Color::from_rgb8(0x7F, 0xB2, 0xFF))
}

/// A small filled circle of `size` pixels in `color`, for marking a collection or a status.
pub fn color_dot<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    container(space().width(size).height(size))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(color)),
            border: border::rounded(size / 2.0),
            ..Default::default()
        })
        .into()
}

/// A centred placeholder for an empty list or view: a title and a muted hint below it.
pub fn empty_state<'a, M: 'a>(title: &'a str, hint: &'a str) -> Element<'a, M> {
    let content = column![text(title).size(18).color(theme::TEXT), text(hint).size(14).color(theme::MUTED)]
        .spacing(6)
        .align_x(Alignment::Center);
    container(content).padding(24).center(iced::Fill).into()
}

/// A small muted heading over a group of rows, shown in capitals (`Kalender` → `KALENDER`).
pub fn section_title<'a, M: 'a>(title: &str) -> Element<'a, M> {
    text(title.to_uppercase()).size(12).color(theme::MUTED).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(color_from_hex("#D50C0C"), Some(Color::from_rgb8(0xD5, 0x0C, 0x0C)));
        assert_eq!(color_from_hex("#0082c9FF"), Some(Color::from_rgb8(0x00, 0x82, 0xC9)));
        assert_eq!(color_from_hex("red"), None);
        assert_eq!(color_from_hex("#12345"), None);
    }

    #[test]
    fn building_blocks_build() {
        let _dot: Element<'_, ()> = color_dot(theme::attention(), 10.0);
        let _empty: Element<'_, ()> = empty_state("No events", "Nothing is planned for today.");
        let _title: Element<'_, ()> = section_title("Calendar");
    }
}
