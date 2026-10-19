//! The Torro theme for Iced: colour tokens in a light and a dark [`Palette`], the Iced
//! [`Theme`]s built from them, spacing, radius and size tokens, and the fonts.
//!
//! The values come from the **active design** ([`crate::design`], concept `docs/design-pakete.md`):
//! every token is a function that reads it, so a switch of design pack shows on the next frame.
//! The standard design gives the values of the design skill (`.claude/skills/torro-design`,
//! sections 1–4), after Linear; the reference mock-ups are `docs/prototypes/linear/`
//! (`torro.css`). Torro red is brand only (primary button, today, the now line, the app logo),
//! never selection and never status – a pack changes how `accent` looks, not what it is for.
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
//! typo::text(typo::meta(), tr("3 open")).style(theme::text_3);          // text in a token colour
//! container(body).padding(space::l()).style(|t| palette(t).panel.into()); // a plain surface
//! container(chip).style(|t| {
//!     let p = palette(t);
//!     container::background(p.raised).border(border::rounded(radius::xs()).color(p.line_strong).width(1))
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
//! `.padding()`, `.width()`/`.height()` and border radii; the gaps by role (inset, page and panel
//! padding …) are in [`crate::layout`]. They return `f32`, so they go straight into Iced
//! (`.spacing(space::s())`, `.height(size::row())`, `border::rounded(radius::sm())`). They are
//! functions, not constants: never put them into a `const` or `static`, compute derived values
//! in a function instead.
//!
//! # Fonts
//!
//! [`font`] and friends name the families of the active design (Inter in the standard, embedded by
//! [`crate::fonts`]). Apps rarely pick a font directly: the levels in [`crate::typo`] carry the
//! right one.
//!
//! Outside Iced (the tray icon, notifications) there is no `&Theme`; such code takes the dark
//! tokens from [`Palette::dark`].

use std::borrow::Cow;

use crate::design;
use crate::source::Source;
use iced::theme::Mode;
use iced::widget::text::Style as TextStyle;
use iced::{Color, Font, Shadow, Theme, Vector};
use torro_core::design::{Design, FontRole, Mode as DesignMode, Rgba, Weight};
use torro_core::theme::Rgb;
use torro_ipc::{Request, Response};

/// The text font at Regular (400) – Inter in the standard design: the default font of every app,
/// for body text, lists and inputs.
///
/// Set as the app default by [`crate::settings`]. Never use `Font::DEFAULT`: depending on the
/// system that is Liberation Sans or Arial, not the design's font.
pub fn font() -> Font {
    design::font(FontRole::Text, Weight::Regular)
}

/// The text font at Medium (500): emphasis – row titles, buttons, event titles, captions.
pub fn font_medium() -> Font {
    design::font(FontRole::Text, Weight::Medium)
}

/// The text font at SemiBold (600): titles – dialog, group and view titles, the big number in a
/// mini tile.
pub fn font_semibold() -> Font {
    design::font(FontRole::Text, Weight::Semibold)
}

/// The figures font at Regular (400) – "Inter Tabular" in the standard design, Inter with tabular
/// figures baked in – for digits that change (clocks, time axes, counters, dates in columns), so
/// nothing jumps and columns line up.
///
/// A separate family because Iced 0.14 cannot switch OpenType features such as `tnum`.
pub fn font_tabular() -> Font {
    design::tabular_font(iced::font::Weight::Normal)
}

/// The figures font at Medium (500), the weight of [`font_medium`] (or the nearest lighter one the
/// design's figures font has).
pub fn font_tabular_medium() -> Font {
    design::tabular_font(iced::font::Weight::Medium)
}

/// A `torro_core` colour (brand and status tokens) as an Iced colour.
pub const fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

/// The attention colour (waiting for the user, unchecked, deviating, overdue) as an Iced colour.
///
/// From the dark palette of the active design (the standard has the same in both modes); inside a
/// style closure prefer `palette(theme).attention`, which follows the mode.
pub fn attention() -> Color {
    design::with(|t| t.dark.attention)
}

/// The colour tokens of one mode (light or dark) of the active design. Get it with [`palette`] from
/// the `&Theme` a style closure receives; [`Palette::dark`] and [`Palette::light`] are for tests,
/// galleries and code without a theme. A copy: take it fresh in every style call or `draw`.
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
    /// The border of a chosen chip and of the active segment (translucent): clearly stronger than
    /// [`Palette::line_strong`], so a choice reads at a glance, and weaker than
    /// [`Palette::focus_ring`], so "chosen" never looks like "focused". Neutral, never red.
    pub line_selected: Color,
    /// Keyboard focus: a 2-px ring with 1 px gap (translucent, never red).
    pub focus_ring: Color,
    /// Torro red: only the primary button, today, the now line and the app logo.
    pub accent: Color,
    /// Text and icons on [`Palette::accent`] (white). Red itself is never used as small text.
    pub on_accent: Color,
    /// Status "fine, connected, checked" – in a dot or icon, always next to a word.
    pub ok: Color,
    /// Status "waiting, deviating, overdue" – in a dot or icon, always next to a word. Too light
    /// for text and small icons on light surfaces; those use [`Palette::attention_text`].
    pub attention: Color,
    /// Attention colour for text and small icons (an overdue date, the "Overdue" group icon):
    /// ≥ 4.5:1 on every surface. Dark mode: [`Palette::attention`] itself; light mode: the
    /// attention colour moved 50 % towards `text`.
    pub attention_text: Color,
    /// Status "broken, refused" – in a dot or icon, always next to a word. Too light for text on
    /// light surfaces; text uses [`Palette::error_text`].
    pub error: Color,
    /// Error colour for text (the `danger` button, error messages): ≥ 4.5:1 on every surface. Dark
    /// mode: [`Palette::error`] itself; light mode: the error colour moved 30 % towards `text`.
    pub error_text: Color,
    /// Status "not set up, inactive".
    pub inactive: Color,
    /// Status "merged / closed as done" on GitHub (a merged pull request, a closed issue): violet,
    /// the same hue as the fifth list colour. A graphic (icon, dot) next to a word, never text;
    /// ≥ 3:1 on every surface.
    pub merged: Color,
    /// Default colours for calendars and task lists, in this order: work (blue), personal
    /// (green), family (orange), birthday (pink), groceries (violet). Use them as bar, dot, ring
    /// or check mark; as a surface only through [`Palette::tinted`].
    pub calendar: [Color; 5],
    /// Opacity of a calendar colour used as a surface (events): 18 % dark, 14 % light.
    pub tint: f32,
    /// The colours of chart series (task E8.8b), in their fixed order: blue, orange, green,
    /// magenta. Checked as a set for colour blindness in this order, so take them through
    /// [`ChartColor`] and never cycle; a fifth series is [`ChartColor::Other`]. Marks only (bars,
    /// swatches), never text: values and labels stay in the text colours.
    pub chart: [Color; 4],
    /// The grey of the series "Other" ([`ChartColor::Other`]).
    pub chart_other: Color,
    /// The colours of the four groups of mail categories (task E12.7), in the order of
    /// `torro_mail::CategoryGroup`: waiting for me, paper trail, info, junk. The dot and the
    /// border of a category pill ([`crate::widgets::category_pill`]), never text.
    pub category: [Color; 4],
    /// The shadow under menus, floating cards and dialogs.
    pub shadow: Shadow,
    /// Dims the window behind a modal dialog.
    pub scrim: Color,
    /// The night in the calendar's timeline (22:00–06:00): the band of a folded night, and – at
    /// 60 % over [`Palette::panel`] – the ground of an unfolded one. A cool, quiet blue-grey;
    /// `text_3` on it stays readable (≥ 4.5:1).
    pub night: Color,
    /// The frame of the window: sidebar, rail, drawer, the margin around the content panel and the
    /// title band over them. [`Palette::bg`] unless the design gives the sidebar a colour of its own.
    pub sidebar: Color,
    /// A sidebar row under the pointer.
    pub sidebar_hover: Color,
    /// The chosen sidebar row.
    pub sidebar_selected: Color,
    /// Text in the sidebar: the chosen row, titles.
    pub sidebar_text: Color,
    /// The rows of the sidebar.
    pub sidebar_text_2: Color,
    /// Group titles, counters, days outside the month in the sidebar.
    pub sidebar_text_3: Color,
    /// Lines in and at the sidebar.
    pub sidebar_line: Color,
}

impl Palette {
    /// The dark tokens of the active design ("Torro Dark" in the standard).
    pub fn dark() -> Palette {
        design::with(|t| t.dark)
    }

    /// The light tokens of the active design ("Torro Light" in the standard).
    pub fn light() -> Palette {
        design::with(|t| t.light)
    }

    /// The tokens of `mode` in `design`, with the derived colours (`attention_text`, `error_text`)
    /// computed from it and the shadow geometry of the suite (`0 12px 32px`, `torro.css`).
    pub fn of(design: &Design, mode: DesignMode) -> Palette {
        Palette::from_tokens(design.colors.get(mode), mode)
    }

    /// The tokens of `mode` in `design` as the sidebar draws with them
    /// (`torro_core::design::Palette::in_sidebar`): `bg`, `hover`, `selected`, the text colours and
    /// `line` are the sidebar's. Views never ask for it: [`crate::shell`] draws the sidebar with a
    /// theme of its own, and [`palette`] hands this out for it.
    ///
    /// `attention_text` and `error_text` are derived for the sidebar's own brightness, not the
    /// mode's: a dark sidebar in light mode takes the status colours as dark mode does.
    pub fn of_sidebar(design: &Design, mode: DesignMode) -> Palette {
        let c = design.colors.get(mode).in_sidebar();
        let dark = c.bg.luminance() < c.text.luminance();
        Palette::from_tokens(&c, if dark { DesignMode::Dark } else { DesignMode::Light })
    }

    /// The design tokens `c` as Iced colours, the derived ones computed for `mode`.
    fn from_tokens(c: &torro_core::design::Palette, mode: DesignMode) -> Palette {
        Palette {
            bg: rgba(c.bg),
            panel: rgba(c.panel),
            raised: rgba(c.raised),
            hover: rgba(c.hover),
            selected: rgba(c.selected),
            menu: rgba(c.menu),
            text: rgba(c.text),
            text_2: rgba(c.text_2),
            text_3: rgba(c.text_3),
            line: rgba(c.line),
            line_strong: rgba(c.line_strong),
            line_selected: rgba(c.line_selected),
            focus_ring: rgba(c.focus_ring),
            accent: rgba(c.accent),
            on_accent: rgba(c.on_accent),
            ok: rgba(c.ok),
            attention: rgba(c.attention),
            attention_text: rgba(c.attention_text(mode)),
            error: rgba(c.error),
            error_text: rgba(c.error_text(mode)),
            inactive: rgba(c.inactive),
            merged: rgba(c.merged),
            calendar: c.calendar.map(rgba),
            tint: c.tint,
            chart: c.chart.map(rgba),
            category: c.category.map(rgba),
            chart_other: rgba(c.chart_other),
            shadow: Shadow { color: rgba(c.shadow), offset: Vector::new(0.0, 12.0), blur_radius: 32.0 },
            scrim: rgba(c.scrim),
            night: rgba(c.night),
            sidebar: rgba(c.sidebar),
            sidebar_hover: rgba(c.sidebar_hover),
            sidebar_selected: rgba(c.sidebar_selected),
            sidebar_text: rgba(c.sidebar_text),
            sidebar_text_2: rgba(c.sidebar_text_2),
            sidebar_text_3: rgba(c.sidebar_text_3),
            sidebar_line: rgba(c.sidebar_line),
        }
    }

    /// The colour of a chart series ([`ChartColor`]): one of [`Palette::chart`], or
    /// [`Palette::chart_other`] for "Other" and for a slot past the fourth.
    pub fn chart_color(&self, color: ChartColor) -> Color {
        match color {
            ChartColor::Series(slot) => self.chart.get(slot).copied().unwrap_or(self.chart_other),
            ChartColor::Other => self.chart_other,
        }
    }

    /// `color` as an event surface: the same hue at [`Palette::tint`] opacity, to be drawn over
    /// `panel`. Text on it stays [`Palette::text`] (title) and [`Palette::text_2`] (time).
    pub fn tinted(&self, color: Color) -> Color {
        Color { a: self.tint, ..color }
    }
}

/// Which colour a chart series wears (task E8.8b): a slot of [`Palette::chart`] in their fixed
/// order, or the grey of "Other". Views keep this, never a colour, and turn it into one with
/// [`Palette::chart_color`] inside a style closure, so light and dark mode and design packs follow.
///
/// Give slots to series by what they are, not by their rank: the same series keeps its colour when
/// a filter or another span changes how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartColor {
    /// Slot 0 to 3 of [`Palette::chart`]; a higher slot draws as [`ChartColor::Other`].
    Series(usize),
    /// What is not one of the series: the grey [`Palette::chart_other`].
    Other,
}

impl ChartColor {
    /// How many series slots there are before "Other".
    pub const SLOTS: usize = 4;
}

/// `a` moved towards `b` by `t` (0 = `a`, 1 = `b`) in sRGB, like the mock-ups' CSS `color-mix`.
/// The result is opaque; the alpha of `a` and `b` is ignored.
///
/// Read it also as "`b` laid over `a` at opacity `t`": that is how views fade or shade a colour
/// where a translucent one would add up or show what is underneath (canvas fills, overlapping
/// event blocks). Examples: a past event's colour `mix(p.panel, colour, 0.55)`, a weekend column
/// `mix(p.panel, p.bg, 0.5)`.
pub const fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}

/// The translucent `top` composited over the opaque `under`, as the renderer would draw it, as one
/// opaque colour: `over(p.tinted(colour), p.panel)` is an event surface on the panel. Use it where
/// the result is the base of a further [`mix`] or must not let lines underneath show through.
pub const fn over(top: Color, under: Color) -> Color {
    mix(under, top, top.a)
}

/// A design colour as an Iced colour: the channels are laid out the same, so nothing is rounded.
pub const fn rgba(c: Rgba) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

/// The tokens of the active design in the mode `theme` is in: [`Palette::light`] for a light
/// theme, otherwise [`Palette::dark`]. Works for any Iced theme (it asks Iced whether the background
/// is dark), so a stray built-in theme still gets Torro colours.
///
/// The one way views reach colours: call it inside a style closure or a canvas `draw` with the
/// `&Theme` Iced passes in, never store the result across frames (the mode and the design can
/// change any time). Returns a copy; pass `&p` where a function wants `&Palette`.
///
/// Inside the sidebar the theme is the sidebar's ([`sidebar_theme`]) and this returns
/// [`Palette::of_sidebar`]: the same code draws a row in the sidebar's colours there.
pub fn palette(theme: &Theme) -> Palette {
    design::with(|t| match t.sidebar_mode(theme) {
        Some(DesignMode::Dark) => t.sidebar_dark,
        Some(DesignMode::Light) => t.sidebar_light,
        None if theme.extended_palette().is_dark => t.dark,
        None => t.light,
    })
}

/// The theme the sidebar draws its content with, for a window drawn in `theme`: an Iced theme of
/// [`Palette::of_sidebar`] in the window's mode, so [`palette`] gives the sidebar's colours inside
/// it. A sidebar theme stays itself. Used by [`crate::shell`]; views do not need it.
pub fn sidebar_theme(theme: &Theme) -> Theme {
    design::with(|t| match t.sidebar_mode(theme) {
        Some(_) => theme.clone(),
        None if theme.extended_palette().is_dark => t.sidebar_dark_theme.clone(),
        None => t.sidebar_light_theme.clone(),
    })
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
/// Cheap to call on every frame: the two themes are built once per design and cloned (an `Arc`).
pub fn for_appearance(appearance: Appearance, system: Mode) -> Theme {
    match (appearance, system) {
        (Appearance::Light, _) | (Appearance::System, Mode::Light) => light(),
        (Appearance::Dark, _) | (Appearance::System, Mode::Dark | Mode::None) => theme(),
    }
}

/// The Iced palette (what built-in widgets and the window background use) from our tokens.
pub(crate) fn iced_theme(name: Cow<'static, str>, p: &Palette) -> Theme {
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

/// "Torro Dark": the dark Iced theme of the active design ([`Palette::dark`]).
pub fn theme() -> Theme {
    design::with(|t| t.dark_theme.clone())
}

/// "Torro Light": the light Iced theme of the active design ([`Palette::light`]).
pub fn light() -> Theme {
    design::with(|t| t.light_theme.clone())
}

/// The theme for [`Appearance::System`] without knowing the desktop's preference: dark.
///
/// Prefer [`for_appearance`] with the mode from `iced::system::theme()`.
pub fn for_system() -> Theme {
    for_appearance(Appearance::System, Mode::None)
}

/// Spacing between and inside elements (`.spacing()`, `.padding()`): 4 · 8 · 12 · 16 · 24 · 32 in
/// the standard design.
///
/// Label → field 4 (`xs`), within a group 8 (`s`), group → group 16–24 (`l`/`xl`).
/// Only building blocks in `torro_gui` may use 2 and 6 (segment inset, icon–text gap in chips).
pub mod space {
    use crate::design;

    /// 4: label → field, icon → text in tight rows.
    pub fn xs() -> f32 {
        design::with(|t| t.design.space.xs)
    }
    /// 8: within a group, between buttons in a bar.
    pub fn s() -> f32 {
        design::with(|t| t.design.space.s)
    }
    /// 12: padding of small tiles (mini, narrow), sidebar group gap.
    pub fn m() -> f32 {
        design::with(|t| t.design.space.m)
    }
    /// 16: group → group, padding of the properties column and of medium tiles.
    pub fn l() -> f32 {
        design::with(|t| t.design.space.l)
    }
    /// 24: larger group gaps, content padding in the wide layout.
    pub fn xl() -> f32 {
        design::with(|t| t.design.space.xl)
    }
    /// 32: section gaps on large pages.
    pub fn xxl() -> f32 {
        design::with(|t| t.design.space.xxl)
    }
}

/// Corner radii (`border::rounded(radius::sm())`). Fully round (dots, rings, avatars, today):
/// half the element's size.
pub mod radius {
    use crate::design;

    /// 4: chips, event blocks, inner segment, key hints.
    pub fn xs() -> f32 {
        design::with(|t| t.design.radius.xs)
    }
    /// 6: buttons, rows, inputs, menu items.
    pub fn sm() -> f32 {
        design::with(|t| t.design.radius.sm)
    }
    /// 8: the content panel, menus.
    pub fn md() -> f32 {
        design::with(|t| t.design.radius.md)
    }
    /// 10: floating cards and dialogs.
    pub fn lg() -> f32 {
        design::with(|t| t.design.radius.lg)
    }
}

/// Fixed sizes of controls, rows and window regions (`.width()`, `.height()`, `.max_width()`).
/// The margin around the inset content panel is [`crate::layout::inset`].
pub mod size {
    use crate::design;

    /// 28: buttons, icon buttons, segments, menu buttons in the header.
    pub fn control_sm() -> f32 {
        design::with(|t| t.design.size.control_sm)
    }
    /// 32: inputs, property rows.
    pub fn control() -> f32 {
        design::with(|t| t.design.size.control)
    }
    /// 36: quick entry, the primary button in a dialog.
    pub fn control_lg() -> f32 {
        design::with(|t| t.design.size.control_lg)
    }
    /// 44: the buttons of a reminder over the whole screen (task ER1.5), with text in
    /// `typo::body_strong()`; nowhere else. (The strip of the chosen mail in the mail window's list,
    /// `widgets::row_strip`, was this high until issue #227; it is `control_sm()` now.)
    pub fn control_xl() -> f32 {
        design::with(|t| t.design.size.control_xl)
    }
    /// 40: a list row (tasks).
    pub fn row() -> f32 {
        design::with(|t| t.design.size.row)
    }
    /// 52: a list row on two lines (title over a `meta` line, as the inbox shows in narrow
    /// windows). `body` (20) and `meta` (16) need 36; in [`row`] that leaves 2 above and below,
    /// which reads cramped – 52 gives them the 8 of a one-line row.
    pub fn row_two() -> f32 {
        design::with(|t| t.design.size.row_two)
    }
    /// 92: a mail entry's row in the inbox (task E3.9) – a 32 avatar beside three lines of 20
    /// (sender with domain and time, subject, preview), with 16 above and below (task E11.1, the
    /// row padding of the list), so a list of mail reads at a glance instead of as one dense block.
    pub fn row_message() -> f32 {
        design::with(|t| t.design.size.row_message)
    }
    /// 64: any other entry's row in the inbox (task E3.9) – the source's 28 icon beside two lines of
    /// 20 (title and time, then the text or "project · branch"), with 12 above and below.
    pub fn row_notice() -> f32 {
        design::with(|t| t.design.size.row_notice)
    }
    /// 28: a sidebar row (menu rows are 30, set by the menu building block).
    pub fn row_sidebar() -> f32 {
        design::with(|t| t.design.size.row_sidebar)
    }
    /// 32: a group head in a list ("Overdue 2").
    pub fn row_group() -> f32 {
        design::with(|t| t.design.size.row_group)
    }
    /// 44: the header bar of the content panel.
    pub fn header() -> f32 {
        design::with(|t| t.design.size.header)
    }
    /// 232: the sidebar.
    pub fn sidebar() -> f32 {
        design::with(|t| t.design.size.sidebar)
    }
    /// 300: the properties column on the right.
    pub fn props() -> f32 {
        design::with(|t| t.design.size.props)
    }
    /// 400: the side panel with the details of an event or a task ([`crate::panel`]), beside the
    /// content in the wide layout and over it in the medium one.
    pub fn panel() -> f32 {
        design::with(|t| t.design.size.panel)
    }
    /// 460: the list next to the detail in the wide layout.
    pub fn list() -> f32 {
        design::with(|t| t.design.size.list)
    }
    /// 380: the list next to the detail in the medium layout.
    pub fn list_medium() -> f32 {
        design::with(|t| t.design.size.list_medium)
    }
    /// 48: one hour in the calendar's day and week grid.
    pub fn hour() -> f32 {
        design::with(|t| t.design.size.hour)
    }
    /// 52: the time axis of the calendar grid.
    pub fn axis() -> f32 {
        design::with(|t| t.design.size.axis)
    }
    /// 44: the time axis in the narrow layout.
    pub fn axis_narrow() -> f32 {
        design::with(|t| t.design.size.axis_narrow)
    }
    /// 720: reading width of lists and pages without a detail beside them.
    pub fn content_max() -> f32 {
        design::with(|t| t.design.size.content_max)
    }
    /// 640: reading width of forms.
    pub fn form_max() -> f32 {
        design::with(|t| t.design.size.form_max)
    }
    /// 12: the gap between two cards and between the parts of one card (task E8.2,
    /// `widgets::card`) – the comfortable density of the inbox, where entries get room instead of
    /// lines between them.
    pub fn card_gap() -> f32 {
        design::with(|t| t.design.size.card_gap)
    }
    /// 36: the avatar of a card (task E8.2): a sender's initials, round, beside a mail card's
    /// lines. A compact mail row keeps its 32 ([`control`]).
    pub fn avatar() -> f32 {
        design::with(|t| t.design.size.avatar)
    }
    /// 40: the large avatar (task E8.2): the sender at the head of the mail reader.
    pub fn avatar_lg() -> f32 {
        design::with(|t| t.design.size.avatar_lg)
    }
    /// 20: the small source tile (task E8.2, `widgets::source_tile`): beside a word, in a compact
    /// row or a menu.
    pub fn tile_sm() -> f32 {
        design::with(|t| t.design.size.tile_sm)
    }
    /// 28: the source tile of a notice row and of a column head ("Slack 5") (task E8.2).
    pub fn tile() -> f32 {
        design::with(|t| t.design.size.tile)
    }
    /// 36: the large source tile (task E8.2): an attachment in the reader, the head of a card that
    /// stands for an app.
    pub fn tile_lg() -> f32 {
        design::with(|t| t.design.size.tile_lg)
    }
    /// 46: the command bar at the top of the mail reader (task E11.1, `widgets::command_bar`):
    /// Reply, Forward, Archive … as cells over the reader's whole width, a step above an input so
    /// the actions read as buttons, not as a header.
    pub fn command() -> f32 {
        design::with(|t| t.design.size.command)
    }
    /// 42: a Torro pill in the mail reader (task E11.1, `widgets::pill_button`: Task, Event, Mute…)
    /// and the sender's avatar in the reader's head, which stands at the pills' height.
    pub fn pill() -> f32 {
        design::with(|t| t.design.size.pill)
    }
    /// 1: a line – separators, grid lines, borders – and a gap as wide as one (keeping labels
    /// above the columns of a grid). Always one logical pixel, also on HiDPI screens; not part of a
    /// design pack, so it stays a constant.
    pub const HAIRLINE: f32 = 1.0;
}

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
        assert_text_contrast("dark", &Palette::dark(), [14.4, 8.4, 5.1]);
    }

    #[test]
    fn light_text_is_readable_on_every_surface() {
        assert_text_contrast("light", &Palette::light(), [14.6, 6.4, 4.8]);
    }

    #[test]
    fn error_text_is_readable_on_every_surface() {
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            let (ratio, surface) = worst(&p, p.error_text);
            assert!(ratio >= 4.5, "{mode}: error_text on {surface} is {ratio:.2}:1");
        }
        assert_eq!(Palette::dark().error_text, Palette::dark().error, "dark mode keeps the status colour");
    }

    #[test]
    fn attention_text_is_readable_on_every_surface() {
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            let (ratio, surface) = worst(&p, p.attention_text);
            assert!(ratio >= 4.5, "{mode}: attention_text on {surface} is {ratio:.2}:1");
        }
        assert_eq!(Palette::dark().attention_text, Palette::dark().attention, "dark mode keeps the status colour");
    }

    #[test]
    fn merged_stands_out_on_every_surface() {
        // A graphic next to a word (WCAG: 3:1), like the list colours.
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            let (ratio, surface) = worst(&p, p.merged);
            assert!(ratio >= 3.0, "{mode}: merged on {surface} is {ratio:.2}:1");
        }
        assert_eq!(Palette::dark().merged.into_rgba8(), [0xA7, 0x8B, 0xFA, 0xFF]);
        assert_eq!(Palette::light().merged.into_rgba8(), [0x7C, 0x5C, 0xE6, 0xFF]);
    }

    #[test]
    fn quiet_text_is_readable_on_the_night() {
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            let ratio = p.text_3.relative_contrast(p.night);
            assert!(ratio >= 4.5, "{mode}: text_3 on night is {ratio:.2}:1");
        }
        assert_eq!(Palette::dark().night.into_rgba8(), [0x12, 0x16, 0x26, 0xFF]);
        assert_eq!(Palette::light().night.into_rgba8(), [0xE3, 0xE7, 0xF1, 0xFF]);
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
    fn chart_colours_stand_out_on_the_panel_and_are_never_status_or_accent() {
        // Bars and swatches are graphics (WCAG: 3:1 on the panel they sit on), task E8.8b.
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            let reserved = [p.accent, p.ok, p.attention, p.error, p.inactive];
            for colour in p.chart.into_iter().chain([p.chart_other]) {
                let ratio = colour.relative_contrast(p.panel);
                assert!(ratio >= 3.0, "{mode}: {colour:?} on panel is {ratio:.2}:1");
                assert!(!reserved.contains(&colour), "{mode}: {colour:?} is reserved");
            }
        }
        let p = Palette::dark();
        assert_eq!(p.chart_color(ChartColor::Series(0)).into_rgba8(), [0x3B, 0x78, 0xD8, 0xFF]);
        assert_eq!(p.chart_color(ChartColor::Series(ChartColor::SLOTS)), p.chart_other, "past the slots");
        assert_eq!(Palette::light().chart_color(ChartColor::Other).into_rgba8(), [0x50, 0x54, 0x5B, 0xFF]);
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
        assert_eq!((dark.line_selected.a, light.line_selected.a), (0.28, 0.32));
    }

    #[test]
    fn a_chosen_edge_is_stronger_than_a_frame_and_weaker_than_focus() {
        // The edge of a chosen chip sits on `selected`, the chip on `panel` or `menu` (dialogs).
        // "Stronger" is measured as the distance from 1:1 (no difference at all): at least three
        // times that of a plain frame.
        for (mode, p) in [("dark", Palette::dark()), ("light", Palette::light())] {
            for (name, surface) in [("selected", p.selected), ("panel", p.panel), ("menu", p.menu)] {
                let ratio = |edge: Color| over(edge, surface).relative_contrast(surface);
                let (frame, chosen, focus) = (ratio(p.line_strong), ratio(p.line_selected), ratio(p.focus_ring));
                assert!(
                    chosen - 1.0 >= 3.0 * (frame - 1.0),
                    "{mode}: chosen edge on {name} {chosen:.2} vs frame {frame:.2}"
                );
                assert!(chosen < focus, "{mode}: chosen edge on {name} reads like focus");
            }
        }
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
    fn the_standard_sidebar_draws_as_before() {
        for (window, p) in [(theme(), Palette::dark()), (light(), Palette::light())] {
            let sidebar = sidebar_theme(&window);
            assert_ne!(sidebar, window, "a theme of its own");
            assert_eq!(palette(&sidebar), p, "with the standard's colours");
            assert_eq!(sidebar_theme(&sidebar), sidebar, "a sidebar theme stays itself");
            assert_eq!((p.sidebar, p.sidebar_text_2, p.sidebar_line), (p.bg, p.text_2, p.line));
        }
        assert_eq!(palette(&sidebar_theme(&Theme::Light)), Palette::light(), "built-in themes too");
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
            [space::xs(), space::s(), space::m(), space::l(), space::xl(), space::xxl()],
            [4.0, 8.0, 12.0, 16.0, 24.0, 32.0]
        );
        assert_eq!([radius::xs(), radius::sm(), radius::md(), radius::lg()], [4.0, 6.0, 8.0, 10.0]);
        assert_eq!([size::control_sm(), size::control(), size::control_lg(), size::row()], [28.0, 32.0, 36.0, 40.0]);
        assert_eq!([size::header(), size::sidebar(), size::props()], [44.0, 232.0, 300.0]);
        assert_eq!(
            [size::list(), size::list_medium(), size::content_max(), size::form_max()],
            [460.0, 380.0, 720.0, 640.0]
        );
        assert_eq!([size::hour(), size::axis(), size::axis_narrow()], [48.0, 52.0, 44.0]);
        assert_eq!([size::row_sidebar(), size::row_group(), size::HAIRLINE], [28.0, 32.0, 1.0]);
        // The mail window's large buttons (task E11.1).
        assert_eq!([size::command(), size::pill(), size::control_xl(), size::row_message()], [46.0, 42.0, 44.0, 92.0]);
    }

    #[test]
    fn mix_and_over_blend_in_srgb() {
        let (black, white) = (Color::BLACK, Color::WHITE);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 0.25), Color::from_rgb(0.25, 0.25, 0.25));
        assert_eq!(mix(black.scale_alpha(0.5), white, 0.5).a, 1.0, "the result is opaque");
        assert_eq!(over(white.scale_alpha(0.25), black), Color::from_rgb(0.25, 0.25, 0.25));
        assert_eq!(over(white, black), white, "an opaque top covers the surface");
    }
}
