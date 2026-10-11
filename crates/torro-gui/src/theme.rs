//! The Torro theme for Iced: colour tokens in a light and a dark [`Palette`], the Iced
//! [`Theme`]s built from them, spacing, radius and size tokens, and the fonts.
//!
//! Values from the design skill (`.claude/skills/torro-design`, sections 1–4), after Linear; the
//! reference mock-ups are `docs/prototypes/linear/` (`torro.css`). Brand and status colours come
//! from `torro_core::theme`. Torro red is brand only (primary button, today, the now line, the
//! app logo), never selection and never status.
//!
//! # Colours: always through `&Theme`
//!
//! Light and dark switch at runtime, so views never hold a colour of their own. Every style
//! closure gets the current `&Theme`; [`palette`] turns it into the tokens of that mode:
//!
//! ```ignore
//! use torro_gui::theme::{self, palette, radius, space};
//! use torro_gui::typo;
//!
//! typo::text(typo::META, tr("3 open")).style(theme::text_3);          // text in a token colour
//! container(body).padding(space::L).style(|t| palette(t).panel.into()); // a plain surface
//! container(chip).style(|t| {
//!     let p = palette(t);
//!     container::background(p.raised).border(border::rounded(radius::XS).color(p.line_strong).width(1))
//! });
//! ```
//!
//! Canvas programs get the theme in `draw(…, theme: &Theme, …)` and call [`palette`] there.
//! Ready-made widget styles (buttons, inputs, panels) live in `torro_gui::style` (task F.3);
//! prefer them over hand-written closures.
//!
//! # Spacing and sizes
//!
//! [`space`], [`radius`] and [`size`] hold every number a view may use for `.spacing()`,
//! `.padding()`, `.width()`/`.height()` and border radii. They are `f32`, so they go straight
//! into Iced (`.spacing(space::S)`, `.height(size::ROW)`, `border::rounded(radius::SM)`).
//!
//! # Fonts
//!
//! [`FONT`] and friends name the Inter families that [`crate::fonts`] embeds. Apps rarely pick a
//! font directly: the levels in [`crate::typo`] carry the right one.
//!
//! The old dark-only constants ([`BACKGROUND`], [`SURFACE`], [`MUTED`] …) are deprecated aliases
//! to the dark values until the app tasks (C.*, T.*, W.*) have moved to [`palette`]; W.5 removes
//! them.

use std::sync::LazyLock;

use crate::source::Source;
use iced::font::Weight;
use iced::theme::Mode;
use iced::widget::text::Style as TextStyle;
use iced::{Color, Font, Shadow, Theme, Vector};
use torro_core::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_INACTIVE, STATUS_OK, TORRO_RED};
use torro_ipc::{Request, Response};

/// Inter Regular (400): the default font of every app, for body text, lists and inputs.
///
/// Set as the app default by [`crate::settings`]. Never use `Font::DEFAULT`: depending on the
/// system that is Liberation Sans or Arial, not Inter.
pub const FONT: Font = Font::with_name("Inter");
/// Inter Medium (500): emphasis – row titles, buttons, event titles, captions.
pub const FONT_MEDIUM: Font = Font { weight: Weight::Medium, ..FONT };
/// Inter SemiBold (600): titles – dialog, group and view titles, the big number in a mini tile.
pub const FONT_SEMIBOLD: Font = Font { weight: Weight::Semibold, ..FONT };
/// "Inter Tabular" Regular (400): Inter with tabular figures baked in, for digits that change
/// (clocks, time axes, counters, dates in columns), so nothing jumps and columns line up.
///
/// A separate family because Iced 0.14 cannot switch OpenType features such as `tnum`.
pub const FONT_TABULAR: Font = Font::with_name("Inter Tabular");
/// "Inter Tabular" Medium (500): tabular figures at the weight of [`FONT_MEDIUM`].
pub const FONT_TABULAR_MEDIUM: Font = Font { weight: Weight::Medium, ..FONT_TABULAR };

/// A `torro_core` colour (brand and status tokens) as an Iced colour.
pub const fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

/// The attention colour (waiting for the user, unchecked, deviating, overdue) as an Iced colour.
///
/// The same in both modes; inside a style closure `palette(theme).attention` is equivalent.
pub fn attention() -> Color {
    color(STATUS_ATTENTION)
}

/// The colour tokens of one mode (light or dark). Get it with [`palette`] from the `&Theme` a
/// style closure receives; [`Palette::dark`] and [`Palette::light`] are for tests and galleries.
///
/// Field names are the skill's token names in snake case (`TEXT_2` → `text_2`). Text colours
/// meet WCAG AA (≥ 4.5:1) on every surface (`bg`, `panel`, `raised`, `hover`, `selected`,
/// `menu`); the tests in this module hold them to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window background and sidebar.
    pub bg: Color,
    /// The content area (the inset panel next to the sidebar), lists, grids.
    pub panel: Color,
    /// Secondary buttons, chips, group heads in lists.
    pub raised: Color,
    /// Hover on rows, buttons and menu items.
    pub hover: Color,
    /// Selection, pressed, the active segment and sidebar item. Neutral, never red.
    pub selected: Color,
    /// Menus, floating cards and dialogs (always with [`Palette::shadow`]).
    pub menu: Color,
    /// Text. Also the theme's default text colour, so plain text needs no style.
    pub text: Color,
    /// Secondary text: descriptions, inactive sidebar items, ghost buttons.
    pub text_2: Color,
    /// Tertiary text: metadata, counters, group titles, placeholders.
    pub text_3: Color,
    /// Separators, grid lines, the border of the content panel (translucent, 1 logical px).
    pub line: Color,
    /// Borders of buttons, inputs, chips and menus (translucent).
    pub line_strong: Color,
    /// Keyboard focus: a 2-px ring with 1 px gap (translucent, never red).
    pub focus_ring: Color,
    /// Torro red: only the primary button, today, the now line and the app logo.
    pub accent: Color,
    /// Text and icons on [`Palette::accent`] (white). Red itself is never used as small text.
    pub on_accent: Color,
    /// Status "fine, connected, checked" – in a dot or icon, always next to a word.
    pub ok: Color,
    /// Status "waiting, deviating, overdue" – in a dot or icon, always next to a word.
    pub attention: Color,
    /// Status "broken, refused" and the text of the `danger` button.
    pub error: Color,
    /// Status "not set up, inactive".
    pub inactive: Color,
    /// Default colours for calendars and task lists, in this order: work (blue), personal
    /// (green), family (orange), birthday (pink), groceries (violet). Use them as bar, dot, ring
    /// or check mark; as a surface only through [`Palette::tinted`].
    pub calendar: [Color; 5],
    /// Opacity of a calendar colour used as a surface (events): 18 % dark, 14 % light.
    pub tint: f32,
    /// The shadow under menus, floating cards and dialogs.
    pub shadow: Shadow,
    /// Dims the window behind a modal dialog.
    pub scrim: Color,
}

impl Palette {
    /// The dark tokens ("Torro Dark").
    pub fn dark() -> &'static Palette {
        &DARK
    }

    /// The light tokens ("Torro Light").
    pub fn light() -> &'static Palette {
        &LIGHT
    }

    /// `color` as an event surface: the same hue at [`Palette::tint`] opacity, to be drawn over
    /// `panel`. Text on it stays [`Palette::text`] (title) and [`Palette::text_2`] (time).
    pub fn tinted(&self, color: Color) -> Color {
        Color { a: self.tint, ..color }
    }
}

const fn hex(rgb: u32) -> Color {
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Translucent white (dark mode) or black (light mode): lines and the focus ring sit on several
/// surfaces, so they are an overlay instead of one fixed grey per surface.
const fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha)
}

const fn black(alpha: f32) -> Color {
    Color::from_rgba(0.0, 0.0, 0.0, alpha)
}

/// `0 12px 32px` as in `torro.css`; only the opacity differs between the modes.
const fn shadow(alpha: f32) -> Shadow {
    Shadow { color: black(alpha), offset: Vector::new(0.0, 12.0), blur_radius: 32.0 }
}

const DARK: Palette = Palette {
    bg: hex(0x08090A),
    panel: hex(0x0F1011),
    raised: hex(0x141518),
    hover: hex(0x16171A),
    selected: hex(0x1C1E22),
    menu: hex(0x17181B),
    text: hex(0xEDEEF0),
    text_2: hex(0xB4B8BF),
    text_3: hex(0x8A8F98),
    line: white(0.06),
    line_strong: white(0.10),
    focus_ring: white(0.35),
    accent: color(TORRO_RED),
    on_accent: Color::WHITE,
    ok: color(STATUS_OK),
    attention: color(STATUS_ATTENTION),
    error: color(STATUS_ERROR),
    inactive: color(STATUS_INACTIVE),
    calendar: [hex(0x5B8CFF), hex(0x3FB984), hex(0xE8A33D), hex(0xD96CA8), hex(0xA78BFA)],
    tint: 0.18,
    shadow: shadow(0.5),
    scrim: black(0.62),
};

const LIGHT: Palette = Palette {
    bg: hex(0xF4F5F6),
    panel: hex(0xFFFFFF),
    raised: hex(0xF8F9FA),
    hover: hex(0xF0F1F3),
    selected: hex(0xE9EAED),
    menu: hex(0xFFFFFF),
    text: hex(0x18191B),
    text_2: hex(0x4F535A),
    text_3: hex(0x62666D),
    line: black(0.07),
    line_strong: black(0.11),
    focus_ring: black(0.5),
    accent: color(TORRO_RED),
    on_accent: Color::WHITE,
    ok: color(STATUS_OK),
    attention: color(STATUS_ATTENTION),
    error: color(STATUS_ERROR),
    inactive: color(STATUS_INACTIVE),
    calendar: [hex(0x3B6EF0), hex(0x1F9D68), hex(0xB06F12), hex(0xC2508F), hex(0x7C5CE6)],
    tint: 0.14,
    shadow: shadow(0.12),
    scrim: black(0.3),
};

/// The tokens of the mode `theme` is in: [`Palette::light`] for a light theme, otherwise
/// [`Palette::dark`]. Works for any Iced theme (it asks Iced whether the background is dark), so a
/// stray built-in theme still gets Torro colours.
///
/// The one way views reach colours: call it inside a style closure or a canvas `draw` with the
/// `&Theme` Iced passes in, never store the result across frames (the mode can change any time).
pub fn palette(theme: &Theme) -> &'static Palette {
    if theme.extended_palette().is_dark { &DARK } else { &LIGHT }
}

/// Text style in [`Palette::text`]: `typo::text(…).style(theme::text)`. Plain text already has
/// this colour; use it where a parent (a button) would set another.
pub fn text(theme: &Theme) -> TextStyle {
    TextStyle { color: Some(palette(theme).text) }
}

/// Text style in [`Palette::text_2`] (secondary): `typo::text(…).style(theme::text_2)`.
pub fn text_2(theme: &Theme) -> TextStyle {
    TextStyle { color: Some(palette(theme).text_2) }
}

/// Text style in [`Palette::text_3`] (tertiary: metadata, counters, group titles):
/// `typo::text(…).style(theme::text_3)`.
pub fn text_3(theme: &Theme) -> TextStyle {
    TextStyle { color: Some(palette(theme).text_3) }
}

pub use torro_core::theme::Appearance;

/// The appearance stored in the suite settings, asked from the core (`Request::Settings`).
///
/// `Err` when the core does not answer; the app then keeps the appearance it has, so a core that
/// is starting up or restarting never flips the colours. `--demo` has no core and is always
/// [`Appearance::System`].
pub async fn stored_appearance(source: &Source) -> Result<Appearance, String> {
    let Source::Core(client) = source else {
        return Ok(Appearance::System);
    };
    match torro_ipc::client::call(&client.socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => Ok(settings.appearance),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// The Iced theme for `appearance`. `system` is the desktop's preference as Iced reports it
/// (`iced::system::theme()` once at start, `iced::system::theme_changes()` afterwards); it only
/// matters for [`Appearance::System`], and `Mode::None` counts as dark.
///
/// Use in the app builder: `.theme(|app: &App| theme::for_appearance(app.appearance, app.system_mode))`.
/// Cheap to call on every frame: the two themes are built once and cloned (an `Arc`).
pub fn for_appearance(appearance: Appearance, system: Mode) -> Theme {
    match (appearance, system) {
        (Appearance::Light, _) | (Appearance::System, Mode::Light) => light(),
        (Appearance::Dark, _) | (Appearance::System, Mode::Dark | Mode::None) => theme(),
    }
}

/// The Iced palette (what built-in widgets and the window background use) from our tokens.
fn iced_theme(name: &'static str, p: &Palette) -> Theme {
    Theme::custom(
        name,
        iced::theme::Palette {
            background: p.bg,
            text: p.text,
            primary: p.accent,
            success: p.ok,
            warning: p.attention,
            danger: p.error,
        },
    )
}

static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| iced_theme("Torro Dark", &DARK));
static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| iced_theme("Torro Light", &LIGHT));

/// "Torro Dark": the dark Iced theme ([`Palette::dark`]).
pub fn theme() -> Theme {
    DARK_THEME.clone()
}

/// "Torro Light": the light Iced theme ([`Palette::light`]).
pub fn light() -> Theme {
    LIGHT_THEME.clone()
}

/// The theme for [`Appearance::System`] without knowing the desktop's preference: dark.
///
/// Prefer [`for_appearance`] with the mode from `iced::system::theme()`.
pub fn for_system() -> Theme {
    for_appearance(Appearance::System, Mode::None)
}

/// Spacing between and inside elements (`.spacing()`, `.padding()`): 4 · 8 · 12 · 16 · 24 · 32.
///
/// Label → field 4 (`XS`), within a group 8 (`S`), group → group 16–24 (`L`/`XL`).
/// Only building blocks in `torro_gui` may use 2 and 6 (segment inset, icon–text gap in chips).
pub mod space {
    /// 4: label → field, icon → text in tight rows.
    pub const XS: f32 = 4.0;
    /// 8: within a group, between buttons in a bar.
    pub const S: f32 = 8.0;
    /// 12: padding of small tiles (mini, narrow), sidebar group gap.
    pub const M: f32 = 12.0;
    /// 16: group → group, padding of the properties column and of medium tiles.
    pub const L: f32 = 16.0;
    /// 24: larger group gaps, content padding in the wide layout.
    pub const XL: f32 = 24.0;
    /// 32: section gaps on large pages.
    pub const XXL: f32 = 32.0;
}

/// Corner radii (`border::rounded(radius::SM)`). Fully round (dots, rings, avatars, today):
/// half the element's size.
pub mod radius {
    /// 4: chips, event blocks, inner segment, key hints.
    pub const XS: f32 = 4.0;
    /// 6: buttons, rows, inputs, menu items.
    pub const SM: f32 = 6.0;
    /// 8: the content panel, menus.
    pub const MD: f32 = 8.0;
    /// 10: floating cards and dialogs.
    pub const LG: f32 = 10.0;
}

/// Fixed sizes of controls, rows and window regions (`.width()`, `.height()`, `.max_width()`).
pub mod size {
    /// 28: buttons, icon buttons, segments, menu buttons in the header.
    pub const CONTROL_SM: f32 = 28.0;
    /// 32: inputs, property rows.
    pub const CONTROL: f32 = 32.0;
    /// 36: quick entry, the primary button in a dialog.
    pub const CONTROL_LG: f32 = 36.0;
    /// 40: a list row (tasks).
    pub const ROW: f32 = 40.0;
    /// 28: a sidebar row (menu rows are 30, set by the menu building block).
    pub const ROW_SIDEBAR: f32 = 28.0;
    /// 32: a group head in a list ("Overdue 2").
    pub const ROW_GROUP: f32 = 32.0;
    /// 44: the header bar of the content panel.
    pub const HEADER: f32 = 44.0;
    /// 232: the sidebar.
    pub const SIDEBAR: f32 = 232.0;
    /// 300: the properties column on the right.
    pub const PROPS: f32 = 300.0;
    /// 460: the list next to the detail in the wide layout.
    pub const LIST: f32 = 460.0;
    /// 380: the list next to the detail in the medium layout.
    pub const LIST_MEDIUM: f32 = 380.0;
    /// 48: one hour in the calendar's day and week grid.
    pub const HOUR: f32 = 48.0;
    /// 52: the time axis of the calendar grid.
    pub const AXIS: f32 = 52.0;
    /// 44: the time axis in the narrow layout.
    pub const AXIS_NARROW: f32 = 44.0;
    /// 8: the margin around the inset content panel (top, right, bottom).
    pub const INSET: f32 = 8.0;
    /// 720: reading width of lists and pages without a detail beside them.
    pub const CONTENT_MAX: f32 = 720.0;
    /// 640: reading width of forms.
    pub const FORM_MAX: f32 = 640.0;
}

/// Window background (dark).
#[deprecated(note = "use `theme::palette(theme).bg` in a style closure")]
pub const BACKGROUND: Color = DARK.bg;
/// Cards, sidebars (dark).
#[deprecated(note = "use `theme::palette(theme).panel` in a style closure")]
pub const SURFACE: Color = DARK.panel;
/// Selected rows, hovered items, the active view tab (dark).
#[deprecated(note = "use `palette(theme).selected` (or `.hover`/`.raised`) in a style closure")]
pub const SURFACE_2: Color = DARK.selected;
/// Grid lines, separators (dark).
#[deprecated(note = "use `theme::palette(theme).line` in a style closure")]
pub const LINE: Color = DARK.line;
/// Text (dark).
#[deprecated(note = "use `.style(theme::text)` or `theme::palette(theme).text`")]
pub const TEXT: Color = DARK.text;
/// Secondary text (dark).
#[deprecated(note = "use `.style(theme::text_2)` or `theme::palette(theme).text_2`")]
pub const MUTED: Color = DARK.text_2;
/// Tertiary text: dates outside the month, placeholders (dark).
#[deprecated(note = "use `.style(theme::text_3)` or `theme::palette(theme).text_3`")]
pub const FAINT: Color = DARK.text_3;
/// The "now" line in the day and week views: Torro red, like Linear's and the skill's.
#[deprecated(note = "use `theme::palette(theme).accent`")]
pub const NOW: Color = DARK.accent;
/// Dims everything behind a dialog (dark).
#[deprecated(note = "use `theme::palette(theme).scrim`")]
pub const SCRIM: Color = DARK.scrim;
/// The soft shadow under dialogs (dark).
#[deprecated(note = "use `theme::palette(theme).shadow`")]
pub const SHADOW: Color = DARK.shadow.color;

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(p: &Palette) -> [(&'static str, Color); 6] {
        [
            ("bg", p.bg),
            ("panel", p.panel),
            ("raised", p.raised),
            ("hover", p.hover),
            ("selected", p.selected),
            ("menu", p.menu),
        ]
    }

    /// `top` composited over the opaque `under`, as the renderer does it.
    fn over(top: Color, under: Color) -> Color {
        let mix = |a: f32, b: f32| a * top.a + b * (1.0 - top.a);
        Color::from_rgb(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b))
    }

    /// Worst contrast of `fg` over every surface of `p`.
    fn worst(p: &Palette, fg: Color) -> (f32, &'static str) {
        surfaces(p)
            .into_iter()
            .map(|(name, surface)| (fg.relative_contrast(surface), name))
            .fold((f32::INFINITY, ""), |worst, next| if next.0 < worst.0 { next } else { worst })
    }

    /// Minimums measured in the skill (section 4), rounded to one decimal there; 0.05 below
    /// keeps the rounding, a darker surface or a lighter text still fails.
    fn assert_text_contrast(mode: &str, p: &Palette, floors: [f32; 3]) {
        for ((name, fg), floor) in
            [("text", p.text), ("text_2", p.text_2), ("text_3", p.text_3)].into_iter().zip(floors)
        {
            let (ratio, surface) = worst(p, fg);
            assert!(ratio >= 4.5, "{mode}: {name} on {surface} is {ratio:.2}:1, below WCAG AA");
            assert!(ratio >= floor - 0.05, "{mode}: {name} on {surface} is {ratio:.2}:1, skill says {floor}");
        }
    }

    #[test]
    fn dark_text_is_readable_on_every_surface() {
        assert_text_contrast("dark", Palette::dark(), [14.4, 8.4, 5.1]);
    }

    #[test]
    fn light_text_is_readable_on_every_surface() {
        assert_text_contrast("light", Palette::light(), [14.6, 6.4, 4.8]);
    }

    #[test]
    fn white_on_torro_red_is_readable() {
        for p in [Palette::dark(), Palette::light()] {
            let ratio = p.on_accent.relative_contrast(p.accent);
            assert!(ratio >= 5.35, "white on red is {ratio:.2}:1, skill says 5.4");
        }
    }

    #[test]
    fn event_text_is_readable_on_tinted_calendar_colours() {
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            for colour in p.calendar {
                let surface = over(p.tinted(colour), p.panel);
                for (name, fg) in [("text", p.text), ("text_2", p.text_2)] {
                    let ratio = fg.relative_contrast(surface);
                    assert!(ratio >= 4.5, "{mode}: {name} on tinted {colour:?} is {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn calendar_colours_stand_out_on_the_panel() {
        // Bars, dots and rings are graphics: WCAG asks 3:1 against the surface they sit on.
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            for colour in p.calendar {
                let ratio = colour.relative_contrast(p.panel);
                assert!(ratio >= 3.0, "{mode}: {colour:?} on panel is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn values_match_the_design_skill() {
        let (dark, light) = (Palette::dark(), Palette::light());
        assert_eq!(dark.bg.into_rgba8(), [0x08, 0x09, 0x0A, 0xFF]);
        assert_eq!(light.selected.into_rgba8(), [0xE9, 0xEA, 0xED, 0xFF]);
        assert_eq!(light.text_3.into_rgba8(), [0x62, 0x66, 0x6D, 0xFF]);
        assert_eq!(dark.accent, light.accent, "Torro red is the same in both modes");
        assert_eq!(dark.accent.into_rgba8(), [0xD5, 0x0C, 0x0C, 0xFF]);
        assert_eq!((dark.tint, light.tint), (0.18, 0.14));
        assert_eq!((dark.line.a, dark.line_strong.a, dark.focus_ring.a), (0.06, 0.10, 0.35));
        assert_eq!((light.line.a, light.line_strong.a, light.focus_ring.a), (0.07, 0.11, 0.5));
    }

    #[test]
    fn palette_follows_the_theme_mode() {
        assert_eq!(palette(&theme()), Palette::dark());
        assert_eq!(palette(&light()), Palette::light());
        assert_eq!(palette(&Theme::Light), Palette::light());
        assert_eq!(palette(&Theme::Dark), Palette::dark());
        assert_eq!(theme().to_string(), "Torro Dark");
        assert_eq!(light().to_string(), "Torro Light");
    }

    #[test]
    fn text_styles_use_the_mode_of_the_theme() {
        assert_eq!(text_2(&theme()).color, Some(Palette::dark().text_2));
        assert_eq!(text_3(&light()).color, Some(Palette::light().text_3));
        assert_eq!(text(&light()).color, Some(Palette::light().text));
    }

    #[test]
    fn appearance_resolves_to_a_theme() {
        assert_eq!(for_appearance(Appearance::Light, Mode::Dark), light());
        assert_eq!(for_appearance(Appearance::Dark, Mode::Light), theme());
        assert_eq!(for_appearance(Appearance::System, Mode::Light), light());
        assert_eq!(for_appearance(Appearance::System, Mode::Dark), theme());
        assert_eq!(for_appearance(Appearance::System, Mode::None), theme(), "no preference is dark");
        assert_eq!(Appearance::default(), Appearance::System);
    }

    #[test]
    fn demo_has_no_core_and_follows_the_system() {
        let source = Source::from_args(["--demo".to_string()]);
        let stored = iced::futures::executor::block_on(stored_appearance(&source));
        assert_eq!(stored, Ok(Appearance::System));
    }

    #[test]
    fn tinted_keeps_the_hue() {
        let p = Palette::light();
        let tinted = p.tinted(p.calendar[0]);
        assert_eq!((tinted.r, tinted.g, tinted.b), (p.calendar[0].r, p.calendar[0].g, p.calendar[0].b));
        assert_eq!(tinted.a, 0.14);
    }

    #[test]
    fn tokens_are_on_the_scale() {
        assert_eq!(
            [space::XS, space::S, space::M, space::L, space::XL, space::XXL],
            [4.0, 8.0, 12.0, 16.0, 24.0, 32.0]
        );
        assert_eq!([radius::XS, radius::SM, radius::MD, radius::LG], [4.0, 6.0, 8.0, 10.0]);
        assert_eq!([size::CONTROL_SM, size::CONTROL, size::CONTROL_LG, size::ROW], [28.0, 32.0, 36.0, 40.0]);
        assert_eq!([size::HEADER, size::SIDEBAR, size::PROPS, size::INSET], [44.0, 232.0, 300.0, 8.0]);
        assert_eq!([size::LIST, size::LIST_MEDIUM, size::CONTENT_MAX, size::FORM_MAX], [460.0, 380.0, 720.0, 640.0]);
        assert_eq!([size::HOUR, size::AXIS, size::AXIS_NARROW], [48.0, 52.0, 44.0]);
        assert_eq!([size::ROW_SIDEBAR, size::ROW_GROUP], [28.0, 32.0]);
    }

    #[test]
    #[allow(deprecated)]
    fn old_constants_alias_the_dark_tokens() {
        let p = Palette::dark();
        assert_eq!([BACKGROUND, SURFACE, SURFACE_2, LINE], [p.bg, p.panel, p.selected, p.line]);
        assert_eq!([TEXT, MUTED, FAINT], [p.text, p.text_2, p.text_3]);
        assert_eq!([NOW, SCRIM, SHADOW], [p.accent, p.scrim, p.shadow.color]);
    }
}
