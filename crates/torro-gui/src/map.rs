//! The small map of a place (task TE.8, issue #355): the picture the core draws
//! (`calendar.map_preview`) with a pin in the middle, ↗ "Open in Maps" and the attribution the
//! OpenStreetMap licence asks for, in a frame. The calendar's editor shows it under an event's
//! location, the contacts app under an address. Also the map `--demo` shows instead of asking the
//! tile service ([`demo_map`]).
//!
//! Nothing here talks to a map service: the apps ask the core for the picture and hand it in.

use iced::widget::{Space, button, column, container, image, row, stack, svg};
use iced::{Background, Border, Color, ContentFit, Element, Length, Theme};
use torro_core::i18n::tr;

use crate::icon::{self, Icon, icon};
use crate::theme::{palette, radius, size, space};
use crate::{style, typo, widgets};

/// The size of the picture `calendar.map_preview` is asked for, in logical pixels (layout A of the
/// event editor). It is only the request: the map is laid out as wide as its column and
/// [`size::map_preview()`] high, and the picture covers that ([`ContentFit::Cover`]), so a design
/// pack's other row height changes the frame, not what is fetched.
pub const MAP_WIDTH: u16 = 336;
pub const MAP_HEIGHT: u16 = 104;

/// Where the attribution of the map leads (ODbL).
pub const OSM_COPYRIGHT: &str = "https://www.openstreetmap.org/copyright";

/// The edge of the pin on the map: an icon and a half, so it stands out of the streets.
pub fn pin_size() -> f32 {
    icon::SIZE + icon::SIZE / 2.0
}

/// The map, [`size::map_preview()`] high: the picture with a pin in the middle in `pin`, ↗ "Open in
/// Maps" (`open`) in the top right corner and "© OpenStreetMap" (`credit`, which opens
/// [`OSM_COPYRIGHT`]; a button as high as ↗) in the bottom right one, in a frame with radius `md`.
/// With `on_map` a click on the map itself sends it too (the contacts app opens the maps app), and the
/// frame turns `line_selected` under the pointer; without, only the buttons take clicks (the event
/// editor).
pub fn map_view<'a, M: Clone + 'a>(
    picture: &'a image::Handle,
    pin: Color,
    open: M,
    credit: M,
    on_map: Option<M>,
) -> Element<'a, M> {
    let height = size::map_preview();
    let picture =
        image(picture).width(Length::Fill).height(height).content_fit(ContentFit::Cover).border_radius(radius::md());
    let marker = icon(Icon::MapPin, pin_size()).style(move |_, _| svg::Style { color: Some(pin) });
    // The tip of the pin, not its middle, stands on the point.
    let marker = container(column![marker, Space::new().height(pin_size())]).center(Length::Fill);
    let corner = widgets::boxed_button(Icon::ArrowUpRight, tr("Open in Maps"), Some(open));
    let credit = widgets::text_button(None, tr("© OpenStreetMap"), style::button::secondary, Some(credit));
    let over = column![
        row![Space::new().width(Length::Fill), corner],
        Space::new().height(Length::Fill),
        row![Space::new().width(Length::Fill), credit],
    ]
    .padding(space::s());
    let inside = container(stack![picture, marker, over].width(Length::Fill).height(height)).clip(true);
    match on_map {
        Some(on_map) => button(inside).padding(0).width(Length::Fill).style(frame_button).on_press(on_map).into(),
        None => inside.width(Length::Fill).style(frame).into(),
    }
}

/// The place of the map while there is none yet (issue #355, "Finding address…"): the frame as high
/// as the map with `words` in the middle in `meta`/`text_3` and ↗ "Open in Maps" (`open`) in its
/// corner, so nothing below moves when the map comes.
pub fn map_placeholder<'a, M: Clone + 'a>(words: &'a str, open: M) -> Element<'a, M> {
    let height = size::map_preview();
    let words = container(typo::text(typo::meta(), words).style(crate::theme::text_3)).center(Length::Fill);
    let corner = widgets::boxed_button(Icon::ArrowUpRight, tr("Open in Maps"), Some(open));
    let over = column![row![Space::new().width(Length::Fill), corner]].padding(space::s());
    container(stack![words, over].width(Length::Fill).height(height)).width(Length::Fill).style(frame).into()
}

/// The frame of the map as a button: [`frame`], its edge `line_selected` under the pointer, like a
/// card that opens.
fn frame_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut border = frame(theme).border;
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        border.color = palette(theme).line_selected;
    }
    button::Style {
        background: Some(Background::Color(palette(theme).raised)),
        border,
        text_color: palette(theme).text,
        ..button::Style::default()
    }
}

/// The frame of the map: `line_strong`, radius `md`.
pub fn frame(theme: &Theme) -> container::Style {
    container::Style {
        border: Border { color: palette(theme).line_strong, width: size::HAIRLINE, radius: radius::md().into() },
        background: Some(Background::Color(palette(theme).raised)),
        ..container::Style::default()
    }
}

/// The map `--demo` shows instead of asking the tile service: a calm sketch of streets and water in
/// the colours of `theme`, [`MAP_WIDTH`] × [`MAP_HEIGHT`] at twice the density, like the core's.
pub fn demo_map(theme: &Theme) -> image::Handle {
    let p = palette(theme);
    let (width, height) = (u32::from(MAP_WIDTH) * 2, u32::from(MAP_HEIGHT) * 2);
    let (land, road, water) = (p.hover.into_rgba8(), p.raised.into_rgba8(), p.bg.into_rgba8());
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let street = (height * 2 / 5..height / 2).contains(&y)
                || (width * 3 / 10..width * 3 / 10 + height / 16).contains(&x)
                || (width * 7 / 10..width * 7 / 10 + height / 16).contains(&x);
            let colour = if y > height * 4 / 5 {
                water
            } else if street {
                road
            } else {
                land
            };
            pixels.extend_from_slice(&colour);
        }
    }
    image::Handle::from_rgba(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review of #278: the map is laid out by its token, not by the size it is fetched in, and both
    /// of its buttons ("Open in Maps", "© OpenStreetMap") are proper click targets that fit it.
    #[test]
    fn the_map_holds_two_buttons_of_full_height() {
        assert!(size::control_sm() >= 24.0, "a click target of at least 24");
        assert!(size::map_preview() >= 2.0 * size::control_sm() + 2.0 * space::s());
        assert!(pin_size() > icon::SIZE);
        let picture = image::Handle::from_rgba(1, 1, vec![0; 4]);
        let _: Element<'_, u8> = map_view(&picture, Color::WHITE, 1, 2, Some(1));
        let _: Element<'_, u8> = map_view(&picture, Color::WHITE, 1, 2, None);
        let _: Element<'_, u8> = map_placeholder("Finding", 1);
        let _ = demo_map(&Theme::Dark);
    }
}
