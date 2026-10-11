//! Icons for the apps: Lucide line icons as SVG, bundled in `assets/icons/` (ISC licence, see
//! `assets/icons/LICENSE.txt`).
//!
//! Only the icons listed in [`Icon`] are bundled. To use another one, copy its SVG from the
//! Lucide package into `assets/icons/`, then add a variant and an arm in [`Icon::svg`]. The
//! bundled files are Lucide 0.469.0 with `stroke-width` set to 1.75, the line weight of the
//! design (`torro-design`, section 2); that is the only change made to them.
//!
//! Icons are tinted in the text colour of the current mode ([`crate::theme::palette`]). Restyle
//! one with `.style(…)` for a quieter colour, e.g. `text_3` for a secondary icon. Use [`SIZE`]
//! for icons on their own and [`SIZE_SM`] inside buttons.

use iced::Color;
use iced::widget::{Svg, svg};

use crate::theme::palette;

/// Edge length of a stand-alone icon, in logical pixels.
pub const SIZE: f32 = 16.0;
/// Edge length of an icon inside a button, in logical pixels.
pub const SIZE_SM: f32 = 14.0;

/// The bundled Lucide icons. Names follow the current Lucide names (`ellipsis`, `panel-left`,
/// `square-check`, `chart-gantt`); older aliases from the prototypes map to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Plus,
    Minus,
    RefreshCw,
    Check,
    X,
    Ellipsis,
    Calendar,
    CalendarClock,
    CalendarDays,
    CalendarPlus,
    CalendarRange,
    ListTodo,
    Inbox,
    Sun,
    Moon,
    Settings,
    User,
    Users,
    LayoutGrid,
    AppWindow,
    Search,
    Flag,
    CircleAlert,
    CircleCheck,
    CirclePlus,
    PanelLeft,
    ArrowUpRight,
    Bell,
    SquareCheck,
    Square,
    SquarePen,
    Clock,
    ChartGantt,
    Radar,
    List,
    ListFilter,
    MapPin,
    Repeat,
    SlidersHorizontal,
    Video,
    Cloud,
    Layers,
    Sparkles,
}

impl Icon {
    /// Every bundled icon, for tests and the gallery.
    pub const ALL: &[Icon] = &[
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::Plus,
        Icon::Minus,
        Icon::RefreshCw,
        Icon::Check,
        Icon::X,
        Icon::Ellipsis,
        Icon::Calendar,
        Icon::CalendarClock,
        Icon::CalendarDays,
        Icon::CalendarPlus,
        Icon::CalendarRange,
        Icon::ListTodo,
        Icon::Inbox,
        Icon::Sun,
        Icon::Moon,
        Icon::Settings,
        Icon::User,
        Icon::Users,
        Icon::LayoutGrid,
        Icon::AppWindow,
        Icon::Search,
        Icon::Flag,
        Icon::CircleAlert,
        Icon::CircleCheck,
        Icon::CirclePlus,
        Icon::PanelLeft,
        Icon::ArrowUpRight,
        Icon::Bell,
        Icon::SquareCheck,
        Icon::Square,
        Icon::SquarePen,
        Icon::Clock,
        Icon::ChartGantt,
        Icon::Radar,
        Icon::List,
        Icon::ListFilter,
        Icon::MapPin,
        Icon::Repeat,
        Icon::SlidersHorizontal,
        Icon::Video,
        Icon::Cloud,
        Icon::Layers,
        Icon::Sparkles,
    ];

    /// The icon as an SVG handle with its stroke in `color` (opaque), for drawing it on a canvas,
    /// where the colour filter of the [`icon`] widget does not reach:
    /// `frame.draw_svg(bounds, &Icon::Moon.tinted(palette(theme).text_3))`. Take the colour from the
    /// current [`palette`] at draw time, so it follows light and dark.
    pub fn tinted(self, color: Color) -> svg::Handle {
        let [r, g, b, _] = color.into_rgba8();
        let svg = String::from_utf8_lossy(self.svg()).replace("currentColor", &format!("#{r:02X}{g:02X}{b:02X}"));
        svg::Handle::from_memory(svg.into_bytes())
    }

    /// The SVG file bytes, compiled into the binary.
    fn svg(self) -> &'static [u8] {
        match self {
            Icon::ChevronLeft => include_bytes!("../assets/icons/chevron-left.svg"),
            Icon::ChevronRight => include_bytes!("../assets/icons/chevron-right.svg"),
            Icon::ChevronDown => include_bytes!("../assets/icons/chevron-down.svg"),
            Icon::Plus => include_bytes!("../assets/icons/plus.svg"),
            Icon::Minus => include_bytes!("../assets/icons/minus.svg"),
            Icon::RefreshCw => include_bytes!("../assets/icons/refresh-cw.svg"),
            Icon::Check => include_bytes!("../assets/icons/check.svg"),
            Icon::X => include_bytes!("../assets/icons/x.svg"),
            Icon::Ellipsis => include_bytes!("../assets/icons/ellipsis.svg"),
            Icon::Calendar => include_bytes!("../assets/icons/calendar.svg"),
            Icon::CalendarClock => include_bytes!("../assets/icons/calendar-clock.svg"),
            Icon::CalendarDays => include_bytes!("../assets/icons/calendar-days.svg"),
            Icon::CalendarPlus => include_bytes!("../assets/icons/calendar-plus.svg"),
            Icon::CalendarRange => include_bytes!("../assets/icons/calendar-range.svg"),
            Icon::ListTodo => include_bytes!("../assets/icons/list-todo.svg"),
            Icon::Inbox => include_bytes!("../assets/icons/inbox.svg"),
            Icon::Sun => include_bytes!("../assets/icons/sun.svg"),
            Icon::Moon => include_bytes!("../assets/icons/moon.svg"),
            Icon::Settings => include_bytes!("../assets/icons/settings.svg"),
            Icon::User => include_bytes!("../assets/icons/user.svg"),
            Icon::Users => include_bytes!("../assets/icons/users.svg"),
            Icon::LayoutGrid => include_bytes!("../assets/icons/layout-grid.svg"),
            Icon::AppWindow => include_bytes!("../assets/icons/app-window.svg"),
            Icon::Search => include_bytes!("../assets/icons/search.svg"),
            Icon::Flag => include_bytes!("../assets/icons/flag.svg"),
            Icon::CircleAlert => include_bytes!("../assets/icons/circle-alert.svg"),
            Icon::CircleCheck => include_bytes!("../assets/icons/circle-check.svg"),
            Icon::CirclePlus => include_bytes!("../assets/icons/circle-plus.svg"),
            Icon::PanelLeft => include_bytes!("../assets/icons/panel-left.svg"),
            Icon::ArrowUpRight => include_bytes!("../assets/icons/arrow-up-right.svg"),
            Icon::Bell => include_bytes!("../assets/icons/bell.svg"),
            Icon::SquareCheck => include_bytes!("../assets/icons/square-check.svg"),
            Icon::Square => include_bytes!("../assets/icons/square.svg"),
            Icon::SquarePen => include_bytes!("../assets/icons/square-pen.svg"),
            Icon::Clock => include_bytes!("../assets/icons/clock.svg"),
            Icon::ChartGantt => include_bytes!("../assets/icons/chart-gantt.svg"),
            Icon::Radar => include_bytes!("../assets/icons/radar.svg"),
            Icon::List => include_bytes!("../assets/icons/list.svg"),
            Icon::ListFilter => include_bytes!("../assets/icons/list-filter.svg"),
            Icon::MapPin => include_bytes!("../assets/icons/map-pin.svg"),
            Icon::Repeat => include_bytes!("../assets/icons/repeat.svg"),
            Icon::SlidersHorizontal => include_bytes!("../assets/icons/sliders-horizontal.svg"),
            Icon::Video => include_bytes!("../assets/icons/video.svg"),
            Icon::Cloud => include_bytes!("../assets/icons/cloud.svg"),
            Icon::Layers => include_bytes!("../assets/icons/layers.svg"),
            Icon::Sparkles => include_bytes!("../assets/icons/sparkles.svg"),
        }
    }
}

/// `which` as a square of `size` logical pixels, in the text colour of the current mode.
///
/// Iced's SVG colour filter keeps the icon's alpha and replaces its colour, so the stroke
/// follows the theme. The result is a widget: turn it into an element with `.into()`, or
/// restyle it with `.style(…)` before that.
pub fn icon(which: Icon, size: f32) -> Svg<'static> {
    svg(svg::Handle::from_memory(which.svg()))
        .width(size)
        .height(size)
        .style(|theme, _status| svg::Style { color: Some(palette(theme).text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_lucide_svg_with_the_design_stroke() {
        for &which in Icon::ALL {
            let svg = std::str::from_utf8(which.svg()).expect("icon files are UTF-8");
            assert!(svg.contains("<svg"), "{which:?}: no <svg element");
            assert!(svg.trim_end().ends_with("</svg>"), "{which:?}: <svg> is not closed");
            assert!(svg.contains(r#"viewBox="0 0 24 24""#), "{which:?}: unexpected viewBox");
            assert!(svg.contains(r#"stroke-width="1.75""#), "{which:?}: stroke width is not 1.75");
            assert!(!svg.contains("<script"), "{which:?}: scripts are not allowed in icons");
        }
    }

    #[test]
    fn every_icon_builds_at_both_sizes() {
        for &which in Icon::ALL {
            let _ = icon(which, SIZE);
            let _ = icon(which, SIZE_SM);
        }
        assert_eq!((SIZE, SIZE_SM), (16.0, 14.0));
    }

    #[test]
    fn a_tinted_icon_strokes_in_the_given_colour() {
        let svg = std::str::from_utf8(Icon::Moon.svg()).expect("UTF-8");
        assert!(svg.contains("currentColor"), "the files leave the colour to the user");
        let tinted = format!("{:?}", Icon::Moon.tinted(Color::from_rgb8(0x8A, 0x8F, 0x98)));
        assert_ne!(tinted, format!("{:?}", Icon::Moon.tinted(Color::BLACK)), "another colour, another image");
    }
}
