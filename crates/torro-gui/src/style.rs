//! Style functions for every Iced widget the apps use, light and dark, after Linear.
//!
//! Values from the design skill (`.claude/skills/torro-design`, sections 1, 2 and 5) and the
//! reference mock-ups in `docs/prototypes/linear/` (`torro.css`). Colours come only from
//! [`palette`] of the `&Theme` Iced passes in, so every style follows a live light/dark switch.
//! Apps never use Iced's built-in styles (`button::primary`, the default checkbox …); they pass
//! one of these functions to `.style(…)`:
//!
//! ```ignore
//! use iced::widget::{button, checkbox, container, scrollable, text_input};
//! use torro_gui::style;
//!
//! button(label).padding(style::BUTTON_PADDING).style(style::button::primary);
//! button(label).style(style::button::segment(view == View::Week));
//! button(row).style(style::button::sidebar_item(list == active));
//! text_input(tr("Search"), &query).style(style::text_input);
//! checkbox(visible).style(style::checkbox(calendar_colour));
//! container(body).style(style::container::panel);
//! scrollable(list).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable);
//! ```
//!
//! # States
//!
//! Every control follows the same rules (skill, section 5): normal; hover = `hover`; pressed or
//! selected = `selected`; focus = `focus_ring` (2 px); disabled = 40 % opacity of the normal look
//! and *no* hover surface. Selection is always neutral, never Torro red: red is only
//! [`button::primary`]. Iced 0.14 gives keyboard focus only to text inputs and text editors, so only
//! those have a focus state here; buttons, checkboxes and pick lists cannot be focused yet.
//!
//! Framed controls on `raised` ([`button::secondary`], [`button::boxed`], [`button::chip`],
//! [`button::choice_chip`] and [`pick_list`]) also draw their border in `line_selected` while hovered,
//! pressed or open: in dark mode `hover` is almost the colour of `raised` (#16171A on #141518), so the
//! surface alone would give no feedback. A *chosen* chip or segment is `selected` with the same
//! `line_selected` border; the fill (and a chip's check mark) tells it from a hovered one.
//!
//! Sizes (height, padding) are not part of a style in Iced; the constants below and the size tokens
//! in [`crate::theme::size`] carry them. The building blocks in `torro_gui::widgets` (F.5) put style
//! and size together; apps should prefer those.

use iced::widget;
use iced::widget::overlay::menu;
use iced::{Background, Border, Color, Padding, Shadow, Theme, border};

use crate::theme::{Palette, palette, radius};

/// Opacity of a disabled control (skill, section 5).
pub const DISABLED_ALPHA: f32 = 0.4;
/// Width of the focus ring on inputs, in logical pixels (skill, section 4).
pub const FOCUS_RING_WIDTH: f32 = 2.0;
/// Inner padding of a button (secondary, ghost, primary, danger) and of a segment: 10 left and
/// right, the height comes from `.height(size::CONTROL_SM)`.
pub const BUTTON_PADDING: Padding = Padding { top: 0.0, right: 10.0, bottom: 0.0, left: 10.0 };
/// Padding of a text input or editor: 6 top and bottom, 8 left and right; with the 20-px line of
/// [`crate::typo::BODY`] that is the 32-px [`crate::theme::size::CONTROL`].
pub const INPUT_PADDING: Padding = Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 };
/// Padding of a pick list: 6 top and bottom, 10 left and right; with the 16-px line of
/// [`crate::typo::CAPTION`] that is the 28-px [`crate::theme::size::CONTROL_SM`] of a button.
pub const PICK_LIST_PADDING: Padding = Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 };
/// Padding of the segmented control's frame ([`container::segmented`]) around its segments.
pub const SEGMENT_INSET: f32 = 2.0;
/// Height of a menu item (menus are 2 px roomier than sidebar rows, as in Linear).
pub const MENU_ITEM_HEIGHT: f32 = 30.0;
/// Inner padding of a menu item and of a sidebar row: 8 left and right.
pub const ITEM_PADDING: Padding = Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 };
/// Padding of a menu around its items ([`container::menu`]).
pub const MENU_INSET: f32 = 4.0;
/// Height of a chip (property chips under a title in the medium layout).
pub const CHIP_HEIGHT: f32 = 26.0;
/// Inner padding of a chip: 8 left and right.
pub const CHIP_PADDING: Padding = Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 8.0 };
/// Inner padding of a key hint ([`container::kbd`]): 2 top and bottom, 4 left and right.
pub const KBD_PADDING: Padding = Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 };
/// Size of a check box, a task circle, a radio button and the height of a toggler.
pub const CHECK_SIZE: f32 = 16.0;
/// Border width of an empty check box, task circle or radio button (`torro.css`: 1.5 px).
pub const CHECK_BORDER: f32 = 1.5;

/// A radius larger than any control, for fully round shapes when the style does not know the
/// size. The renderers clamp it to half the shorter side (Iced uses the same trick).
const ROUND: u32 = u32::MAX;

/// The colour faded to the disabled opacity.
fn faded(color: Color) -> Color {
    color.scale_alpha(DISABLED_ALPHA)
}

/// A 1-px border in `color` with `radius`.
fn outline(color: Color, radius: impl Into<border::Radius>) -> Border {
    border::rounded(radius).color(color).width(1)
}

/// The colour of selected text in inputs: the text colour at 20 %, neutral like all selection.
/// Text on it keeps ≥ 9:1 in both modes.
fn text_selection(p: &Palette) -> Color {
    p.text.scale_alpha(0.2)
}

/// `a` moved towards `b` by `t` (0 = `a`, 1 = `b`), in sRGB like the mock-ups' CSS.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Color { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) }
}

/// Button styles (`button(…).style(style::button::ghost)`).
///
/// All buttons share the state rules of the module: hover lays `hover` under the content, pressed
/// `selected`, disabled fades the normal look to 40 % without a hover surface. Height
/// [`crate::theme::size::CONTROL_SM`], padding [`BUTTON_PADDING`] (icon buttons: 28 × 28, no
/// padding), label in [`crate::typo::CAPTION`].
pub mod button {
    use iced::widget::button::{Status, Style};
    use iced::{Background, Color, Theme};

    use super::{faded, mix, outline};
    use crate::theme::{Palette, palette, radius};

    /// The look of a button at rest; hover, pressed and disabled are derived from it.
    #[derive(Clone, Copy)]
    struct Rest {
        background: Color,
        text: Color,
        border: Color,
        radius: f32,
    }

    impl Rest {
        fn style(self) -> Style {
            Style {
                background: Some(Background::Color(self.background)),
                text_color: self.text,
                border: outline(self.border, self.radius),
                ..Style::default()
            }
        }
    }

    /// A neutral button: `rest` normally, `hover` with `active_text` on hover, `selected` with
    /// `active_text` when pressed, `rest` faded when disabled.
    fn neutral(p: &Palette, status: Status, rest: Rest, active_text: Color) -> Style {
        match status {
            Status::Active => rest.style(),
            Status::Hovered => Rest { background: p.hover, text: active_text, ..rest }.style(),
            Status::Pressed => Rest { background: p.selected, text: active_text, ..rest }.style(),
            Status::Disabled => disabled(rest),
        }
    }

    /// A button that is the current choice (active segment, active sidebar row, selected list
    /// row): `selected` in every state – hovering a selected row must not make it look less
    /// selected – and faded when disabled.
    fn chosen(status: Status, rest: Rest) -> Style {
        match status {
            Status::Disabled => disabled(rest),
            _ => rest.style(),
        }
    }

    fn disabled(rest: Rest) -> Style {
        Rest { background: faded(rest.background), text: faded(rest.text), border: faded(rest.border), ..rest }.style()
    }

    /// A [`neutral`] button with a frame on `raised`: hovered and pressed also draw the border in
    /// `line_selected`, because `hover` on `raised` is almost invisible in dark mode (module docs).
    fn framed(p: &Palette, status: Status, rest: Rest, active_text: Color) -> Style {
        let style = neutral(p, status, rest, active_text);
        match status {
            Status::Hovered | Status::Pressed => Style { border: style.border.color(p.line_selected), ..style },
            Status::Active | Status::Disabled => style,
        }
    }

    /// The one primary action of a window ("New event", "Save"): Torro red with white text.
    ///
    /// At most one per window – red is the brand, never selection or status. Hover and pressed
    /// darken the red (10 % and 20 % towards black), which keeps white text above 5.4:1.
    pub fn primary(theme: &Theme, status: Status) -> Style {
        let p = palette(theme);
        let rest = Rest { background: p.accent, text: p.on_accent, border: Color::TRANSPARENT, radius: radius::SM };
        match status {
            Status::Active => rest.style(),
            Status::Hovered => Rest { background: mix(p.accent, Color::BLACK, 0.1), ..rest }.style(),
            Status::Pressed => Rest { background: mix(p.accent, Color::BLACK, 0.2), ..rest }.style(),
            Status::Disabled => disabled(rest),
        }
    }

    /// A secondary action with a frame ("Today", "Filter"): `raised` with a `line_strong` border
    /// and `text`; hovered and pressed the border turns `line_selected` (module docs).
    pub fn secondary(theme: &Theme, status: Status) -> Style {
        let p = palette(theme);
        let rest = Rest { background: p.raised, text: p.text, border: p.line_strong, radius: radius::SM };
        framed(p, status, rest, p.text)
    }

    /// The default button in header bars: no surface, `text_2`; on hover `hover` and `text`.
    pub fn ghost(theme: &Theme, status: Status) -> Style {
        let p = palette(theme);
        let rest =
            Rest { background: Color::TRANSPARENT, text: p.text_2, border: Color::TRANSPARENT, radius: radius::SM };
        neutral(p, status, rest, p.text)
    }

    /// An icon-only button (‹ › refresh …): a [`ghost`] button, 28 × 28
    /// ([`crate::theme::size::CONTROL_SM`] square, no padding), icon 14–16 in the text colour.
    /// Always give it a tooltip.
    pub fn icon(theme: &Theme, status: Status) -> Style {
        ghost(theme, status)
    }

    /// An icon button with a frame, for "New" next to the app switcher in the sidebar: looks like
    /// [`secondary`] at 28 × 28.
    pub fn boxed(theme: &Theme, status: Status) -> Style {
        secondary(theme, status)
    }

    /// A destructive action ("Delete event"): a [`ghost`] button with the text in
    /// [`Palette::error_text`] (≥ 4.5:1 on every surface, both modes), in every state.
    pub fn danger(theme: &Theme, status: Status) -> Style {
        let p = palette(theme);
        let rest =
            Rest { background: Color::TRANSPARENT, text: p.error_text, border: Color::TRANSPARENT, radius: radius::SM };
        neutral(p, status, rest, p.error_text)
    }

    /// One segment of a segmented control (Day · Week · Month), inside
    /// [`super::container::segmented`]. Label `CAPTION` in `text_3`, on hover `hover` and `text`;
    /// the `active` segment is `selected` with a `line_selected` edge and `text` – the edge makes
    /// the choice readable also in light mode, where `selected` is close to the frame's `bg`.
    /// Radius [`radius::XS`], padding [`super::BUTTON_PADDING`], height
    /// `CONTROL_SM − 2 × SEGMENT_INSET − 2` (the frame's border).
    pub fn segment(active: bool) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, status| {
            let p = palette(theme);
            if active {
                let rest = Rest { background: p.selected, text: p.text, border: p.line_selected, radius: radius::XS };
                chosen(status, rest)
            } else {
                let rest = Rest {
                    background: Color::TRANSPARENT,
                    text: p.text_3,
                    border: Color::TRANSPARENT,
                    radius: radius::XS,
                };
                neutral(p, status, rest, p.text)
            }
        }
    }

    /// A property chip that opens an editor ("Today, 16:00" under a title,
    /// `widgets::chip` with a message): the look of [`super::container::chip`] – `raised`,
    /// `line_strong` border, radius [`radius::XS`], `text` – with the states of a [`secondary`]
    /// button: hover `hover`, pressed `selected`, both with a `line_selected` border.
    pub fn chip(theme: &Theme, status: Status) -> Style {
        let p = palette(theme);
        let rest = Rest { background: p.raised, text: p.text, border: p.line_strong, radius: radius::XS };
        framed(p, status, rest, p.text)
    }

    /// A chip that can be chosen (a filter template, a label, a person, a milestone –
    /// `widgets::choice_chip`), [`crate::theme::size::CONTROL_SM`] high, radius [`radius::SM`],
    /// label `CAPTION`.
    ///
    /// Not chosen: `raised`, `line_strong` border, `text_2`; hover `hover` and `text`, pressed
    /// `selected` and `text`, both with a `line_selected` border. Chosen (`on`): `selected`,
    /// `line_selected` border and `text` in every state – hovering must not make a choice look
    /// undone; the widget adds a check mark, so the choice does not rest on the fill alone.
    /// Disabled: the normal look at 40 %. Neutral, never red.
    pub fn choice_chip(on: bool) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, status| {
            let p = palette(theme);
            if on {
                let rest = Rest { background: p.selected, text: p.text, border: p.line_selected, radius: radius::SM };
                chosen(status, rest)
            } else {
                let rest = Rest { background: p.raised, text: p.text_2, border: p.line_strong, radius: radius::SM };
                framed(p, status, rest, p.text)
            }
        }
    }

    /// `style` while the action it started is running ("Creating…"): the button has no message
    /// (it takes no second press), yet it keeps the full look of `style` at rest instead of the
    /// faded disabled look, so it reads as working, not as unavailable. No hover either.
    ///
    /// ```ignore
    /// button(content).style(style::button::busy(style::button::primary)) // no .on_press
    /// ```
    pub fn busy(style: fn(&Theme, Status) -> Style) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, _| style(theme, Status::Active)
    }

    /// A row in the sidebar (navigation, lists, calendars), 28 high
    /// ([`crate::theme::size::ROW_SIDEBAR`]), padding [`super::ITEM_PADDING`]: `text_2`, on hover
    /// `hover` and `text`; the `active` row is `selected` with `text`.
    pub fn sidebar_item(active: bool) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, status| {
            let p = palette(theme);
            if active {
                chosen(
                    status,
                    Rest { background: p.selected, text: p.text, border: Color::TRANSPARENT, radius: radius::SM },
                )
            } else {
                let rest = Rest {
                    background: Color::TRANSPARENT,
                    text: p.text_2,
                    border: Color::TRANSPARENT,
                    radius: radius::SM,
                };
                neutral(p, status, rest, p.text)
            }
        }
    }

    /// A row in a list (tasks, agenda), 40 high ([`crate::theme::size::ROW`]), full width without
    /// radius; the line between rows is a [`super::rule`]. Text `text`; hover `hover`; the
    /// `selected` row (the one shown in the detail) is `selected`.
    pub fn list_row(selected: bool) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, status| {
            let p = palette(theme);
            let rest = Rest { background: Color::TRANSPARENT, text: p.text, border: Color::TRANSPARENT, radius: 0.0 };
            if selected {
                chosen(status, Rest { background: p.selected, ..rest })
            } else {
                neutral(p, status, rest, p.text)
            }
        }
    }

    /// An item in a menu or dropdown, [`super::MENU_ITEM_HEIGHT`] high, padding
    /// [`super::ITEM_PADDING`], text `text`. `highlighted` is the item under the keyboard cursor
    /// (arrow keys): it looks like a hovered item. The current choice in a menu is not a surface
    /// but a small dot at the end of the item (content, not style).
    ///
    /// Menus sit on `menu`, which in dark mode is lighter than `hover`; a `hover` surface would
    /// vanish there, so hover, highlight and pressed are `selected` on menus.
    pub fn menu_item(highlighted: bool) -> impl Fn(&Theme, Status) -> Style + Copy {
        move |theme, status| {
            let p = palette(theme);
            let rest =
                Rest { background: Color::TRANSPARENT, text: p.text, border: Color::TRANSPARENT, radius: radius::SM };
            let lit = Rest { background: p.selected, ..rest };
            match status {
                Status::Disabled => disabled(if highlighted { lit } else { rest }),
                Status::Active if !highlighted => rest.style(),
                _ => lit.style(),
            }
        }
    }
}

/// Container styles (`container(…).style(style::container::panel)`): the surfaces of the window
/// (skill, section 0) and the small framed pieces (chip, key hint).
pub mod container {
    use iced::widget::container::Style;
    use iced::{Background, Color, Theme};

    use super::outline;
    use crate::theme::{palette, radius};

    fn surface(background: Color) -> Style {
        Style { background: Some(Background::Color(background)), ..Style::default() }
    }

    /// The inset content area next to the sidebar (wide layout): `panel`, 1-px `line` border,
    /// radius [`radius::MD`], with [`crate::theme::size::INSET`] margin top, right and bottom
    /// (the margin is the parent's padding).
    pub fn panel(theme: &Theme) -> Style {
        let p = palette(theme);
        Style { border: outline(p.line, radius::MD), ..surface(p.panel) }
    }

    /// The content area without a sidebar (medium, narrow): `panel`, flush with the window, no
    /// border, no radius.
    pub fn panel_flush(theme: &Theme) -> Style {
        surface(palette(theme).panel)
    }

    /// The sidebar: the window background `bg`, no lines.
    pub fn sidebar(theme: &Theme) -> Style {
        surface(palette(theme).bg)
    }

    /// The header bar in the content area, [`crate::theme::size::HEADER`] high: `panel`, so it
    /// covers content scrolled under it. The line below it is a [`super::rule`]: Iced borders
    /// cannot be drawn on one side only.
    pub fn header(theme: &Theme) -> Style {
        surface(palette(theme).panel)
    }

    /// A group of rows on a settings page: `raised`, 1-px `line` border, radius [`radius::MD`].
    /// Rows inside are separated by [`super::rule`].
    pub fn group(theme: &Theme) -> Style {
        let p = palette(theme);
        Style { border: outline(p.line, radius::MD), ..surface(p.raised) }
    }

    /// A menu (dropdown, context menu): `menu`, 1-px `line_strong` border, radius [`radius::MD`],
    /// the menu shadow, text `text`. Padding [`super::MENU_INSET`].
    pub fn menu(theme: &Theme) -> Style {
        let p = palette(theme);
        Style {
            text_color: Some(p.text),
            border: outline(p.line_strong, radius::MD),
            shadow: p.shadow,
            ..surface(p.menu)
        }
    }

    /// A floating card or dialog: like [`menu`] with radius [`radius::LG`]. Padding
    /// [`crate::theme::space::L`], width 320.
    pub fn popover(theme: &Theme) -> Style {
        Style { border: outline(palette(theme).line_strong, radius::LG), ..menu(theme) }
    }

    /// A property chip ("Today", "Work"): `raised`, 1-px `line_strong` border, radius
    /// [`radius::XS`], text `text`. Height [`super::CHIP_HEIGHT`], padding [`super::CHIP_PADDING`],
    /// label `CAPTION`, icon 14 in `text_3`.
    pub fn chip(theme: &Theme) -> Style {
        let p = palette(theme);
        Style { text_color: Some(p.text), border: outline(p.line_strong, radius::XS), ..surface(p.raised) }
    }

    /// A key hint ("⌘K", "N"): no surface, 1-px `line_strong` border, radius [`radius::XS`], text
    /// `text_3` in `CAPTION`. Padding [`super::KBD_PADDING`].
    pub fn kbd(theme: &Theme) -> Style {
        let p = palette(theme);
        Style { text_color: Some(p.text_3), border: outline(p.line_strong, radius::XS), ..Style::default() }
    }

    /// The frame of a segmented control: `bg`, 1-px `line` border, radius [`radius::SM`], padding
    /// [`super::SEGMENT_INSET`]; the segments inside use [`super::button::segment`].
    pub fn segmented(theme: &Theme) -> Style {
        let p = palette(theme);
        Style { border: outline(p.line, radius::SM), ..surface(p.bg) }
    }
}

/// A text input (`text_input(…).style(style::text_input)`): no surface, 1-px `line_strong` border,
/// radius [`radius::SM`]; value `text`, placeholder and icon `text_3`. Hover `hover`; focused a
/// 2-px `focus_ring` border; disabled 40 %.
///
/// Text [`crate::typo::BODY`] with padding [`INPUT_PADDING`] gives the height
/// [`crate::theme::size::CONTROL`] (Iced text inputs have no height of their own).
pub fn text_input(theme: &Theme, status: widget::text_input::Status) -> widget::text_input::Style {
    let p = palette(theme);
    let rest = widget::text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: outline(p.line_strong, radius::SM),
        icon: p.text_3,
        placeholder: p.text_3,
        value: p.text,
        selection: text_selection(p),
    };
    match status {
        widget::text_input::Status::Active => rest,
        widget::text_input::Status::Hovered => {
            widget::text_input::Style { background: Background::Color(p.hover), ..rest }
        }
        widget::text_input::Status::Focused { .. } => {
            widget::text_input::Style { border: rest.border.color(p.focus_ring).width(FOCUS_RING_WIDTH), ..rest }
        }
        widget::text_input::Status::Disabled => widget::text_input::Style {
            border: rest.border.color(faded(p.line_strong)),
            icon: faded(rest.icon),
            placeholder: faded(rest.placeholder),
            value: faded(rest.value),
            ..rest
        },
    }
}

/// A multi-line text editor (notes, descriptions): the same look and states as [`text_input`].
pub fn text_editor(theme: &Theme, status: widget::text_editor::Status) -> widget::text_editor::Style {
    let status = match status {
        widget::text_editor::Status::Active => widget::text_input::Status::Active,
        widget::text_editor::Status::Hovered => widget::text_input::Status::Hovered,
        widget::text_editor::Status::Focused { is_hovered } => widget::text_input::Status::Focused { is_hovered },
        widget::text_editor::Status::Disabled => widget::text_input::Status::Disabled,
    };
    let input = text_input(theme, status);
    widget::text_editor::Style {
        background: input.background,
        border: input.border,
        placeholder: input.placeholder,
        value: input.value,
        selection: input.selection,
    }
}

/// A pick list (`pick_list(…).style(style::pick_list).menu_style(style::pick_list_menu)`): looks
/// like [`button::secondary`] – `raised`, `line_strong` border, radius [`radius::SM`], text
/// `text`, arrow and placeholder `text_3`. Hover `hover`, open `selected`; both with a
/// `line_selected` border like the other framed controls (module docs).
///
/// Text [`crate::typo::CAPTION`] (size, font *and* `text_line_height`) with padding
/// [`PICK_LIST_PADDING`] gives the height [`crate::theme::size::CONTROL_SM`]. Iced 0.14 pick lists
/// have no disabled state.
pub fn pick_list(theme: &Theme, status: widget::pick_list::Status) -> widget::pick_list::Style {
    let p = palette(theme);
    let (background, border) = match status {
        widget::pick_list::Status::Active => (p.raised, p.line_strong),
        widget::pick_list::Status::Hovered => (p.hover, p.line_selected),
        widget::pick_list::Status::Opened { .. } => (p.selected, p.line_selected),
    };
    widget::pick_list::Style {
        text_color: p.text,
        placeholder_color: p.text_3,
        handle_color: p.text_3,
        background: Background::Color(background),
        border: outline(border, radius::SM),
    }
}

/// The menu a pick list opens: like [`container::menu`] (`menu`, `line_strong`, radius
/// [`radius::MD`], shadow); the item under the pointer is `selected` with `text`, as in
/// [`button::menu_item`].
pub fn pick_list_menu(theme: &Theme) -> menu::Style {
    let p = palette(theme);
    menu::Style {
        background: Background::Color(p.menu),
        border: outline(p.line_strong, radius::MD),
        text_color: p.text,
        selected_text_color: p.text,
        selected_background: Background::Color(p.selected),
        shadow: p.shadow,
    }
}

/// The colour of the check mark on a filled check box or task circle: `panel` (white in light
/// mode, near black in dark mode), ≥ 3:1 on every calendar colour of the mode.
fn check_mark(p: &Palette) -> Color {
    p.panel
}

/// Shared states of the two check styles; only the empty border and the shape differ.
fn check(
    p: &Palette,
    status: widget::checkbox::Status,
    color: Color,
    empty_border: Color,
    hover_border: Color,
    radius: border::Radius,
) -> widget::checkbox::Style {
    let (is_checked, hovered, disabled) = match status {
        widget::checkbox::Status::Active { is_checked } => (is_checked, false, false),
        widget::checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        widget::checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let (background, border_color) = match (is_checked, hovered) {
        (true, _) => (color, color),
        (false, true) => (p.hover, hover_border),
        (false, false) => (Color::TRANSPARENT, empty_border),
    };
    let style = widget::checkbox::Style {
        background: Background::Color(background),
        icon_color: check_mark(p),
        border: border::rounded(radius).color(border_color).width(CHECK_BORDER),
        text_color: Some(p.text),
    };
    if !disabled {
        return style;
    }
    widget::checkbox::Style {
        background: Background::Color(faded(background)),
        icon_color: faded(style.icon_color),
        border: style.border.color(faded(border_color)),
        text_color: style.text_color.map(faded),
    }
}

/// A square check box in a calendar's colour (calendar visibility, "all day"):
/// `checkbox(on).style(style::checkbox(colour)).size(CHECK_SIZE)`.
///
/// Empty: a [`CHECK_BORDER`] frame in `color`, hover lays `hover` under it; checked: filled with
/// `color`, check mark in `panel`; radius [`radius::XS`]; disabled 40 %. A filled box has no hover
/// surface: the row it sits in shows the hover.
pub fn checkbox(color: Color) -> impl Fn(&Theme, widget::checkbox::Status) -> widget::checkbox::Style + Copy {
    move |theme, status| check(palette(theme), status, color, color, color, radius::XS.into())
}

/// A round task check in a list's colour: `checkbox(done).style(style::task_check(colour))`.
///
/// Empty: a [`CHECK_BORDER`] ring in `text_3`; hover: `hover` inside a ring in `color` (shows
/// which list it belongs to); done: filled with `color`, check mark in `panel`; disabled 40 %.
pub fn task_check(color: Color) -> impl Fn(&Theme, widget::checkbox::Status) -> widget::checkbox::Style + Copy {
    move |theme, status| {
        let p = palette(theme);
        check(p, status, color, p.text_3, color, ROUND.into())
    }
}

/// A switch for settings (`toggler(on).style(style::toggler).size(CHECK_SIZE)`). Neutral, never
/// red: on is a `text` track with a `panel` knob; off a `raised` track with `line_strong` border
/// and a `text_3` knob. Hover: off `hover` track and `text_2` knob, on `text_2` track. Disabled
/// 40 %.
pub fn toggler(theme: &Theme, status: widget::toggler::Status) -> widget::toggler::Style {
    let p = palette(theme);
    let (on, hovered, disabled) = match status {
        widget::toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        widget::toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        widget::toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let (track, knob, track_border) = match (on, hovered) {
        (true, false) => (p.text, p.panel, Color::TRANSPARENT),
        (true, true) => (p.text_2, p.panel, Color::TRANSPARENT),
        (false, false) => (p.raised, p.text_3, p.line_strong),
        (false, true) => (p.hover, p.text_2, p.line_strong),
    };
    let fade = |c: Color| if disabled { faded(c) } else { c };
    widget::toggler::Style {
        background: Background::Color(fade(track)),
        background_border_width: 1.0,
        background_border_color: fade(track_border),
        foreground: Background::Color(fade(knob)),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(fade(p.text)),
        border_radius: None,
        // 2 px between knob and track at the 16-px height.
        padding_ratio: 0.125,
    }
}

/// A radio button (`radio(…).style(style::radio).size(CHECK_SIZE)`): a [`CHECK_BORDER`] ring in
/// `text_3`, chosen: ring and dot in `text`; hover lays `hover` inside. Iced 0.14 radios have no
/// disabled state.
pub fn radio(theme: &Theme, status: widget::radio::Status) -> widget::radio::Style {
    let p = palette(theme);
    let (selected, hovered) = match status {
        widget::radio::Status::Active { is_selected } => (is_selected, false),
        widget::radio::Status::Hovered { is_selected } => (is_selected, true),
    };
    widget::radio::Style {
        background: Background::Color(if hovered { p.hover } else { Color::TRANSPARENT }),
        dot_color: p.text,
        border_width: CHECK_BORDER,
        border_color: if selected { p.text } else { p.text_3 },
        text_color: Some(p.text),
    }
}

/// The scrollbar geometry for [`scrollable`]: 6 px wide, 2 px from the edge, as narrow as Linear's.
/// Use as `.direction(scrollable::Direction::Vertical(style::scrollbar()))`.
pub fn scrollbar() -> widget::scrollable::Scrollbar {
    widget::scrollable::Scrollbar::new().width(6).scroller_width(6).margin(2)
}

/// A scrollable (`scrollable(…).style(style::scrollable)`, with [`scrollbar`]): no rail, a round
/// scroller in `line_strong` that turns `focus_ring` while the pointer is on it or drags it. In the
/// light theme the scroller shows only while the pointer is over the scrollable (that is, while
/// one scrolls): a grey bar at rest is noise on a white page; the dark theme keeps it visible.
pub fn scrollable(theme: &Theme, status: widget::scrollable::Status) -> widget::scrollable::Style {
    let p = palette(theme);
    let resting = matches!(status, widget::scrollable::Status::Active { .. }) && !theme.extended_palette().is_dark;
    let (on_vertical, on_horizontal) = match status {
        widget::scrollable::Status::Active { .. } => (false, false),
        widget::scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered, is_horizontal_scrollbar_hovered, ..
        } => (is_vertical_scrollbar_hovered, is_horizontal_scrollbar_hovered),
        widget::scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged, is_horizontal_scrollbar_dragged, ..
        } => (is_vertical_scrollbar_dragged, is_horizontal_scrollbar_dragged),
    };
    let rail = |engaged: bool| widget::scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: widget::scrollable::Scroller {
            background: Background::Color(match (resting, engaged) {
                (true, _) => Color::TRANSPARENT,
                (false, true) => p.focus_ring,
                (false, false) => p.line_strong,
            }),
            border: border::rounded(ROUND),
        },
    };
    widget::scrollable::Style {
        container: widget::container::Style::default(),
        vertical_rail: rail(on_vertical),
        horizontal_rail: rail(on_horizontal),
        gap: None,
        auto_scroll: widget::scrollable::AutoScroll {
            background: Background::Color(p.menu),
            border: outline(p.line_strong, ROUND),
            shadow: Shadow::default(),
            icon: p.text_2,
        },
    }
}

/// A separator or grid line (`rule::horizontal(1).style(style::rule)`): 1 logical px in `line`,
/// snapped to the pixel grid so it stays crisp on HiDPI.
pub fn rule(theme: &Theme) -> widget::rule::Style {
    widget::rule::Style {
        color: palette(theme).line,
        radius: border::Radius::default(),
        fill_mode: widget::rule::FillMode::Full,
        snap: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{self, Palette};
    use iced::widget::button::{Status, Style};

    fn scroller_alpha(theme: &Theme, status: widget::scrollable::Status) -> f32 {
        match scrollable(theme, status).vertical_rail.scroller.background {
            Background::Color(color) => color.a,
            _ => 1.0,
        }
    }

    #[test]
    fn the_light_scrollbar_shows_only_while_the_pointer_is_over_the_scrollable() {
        let resting = widget::scrollable::Status::Active {
            is_horizontal_scrollbar_disabled: false,
            is_vertical_scrollbar_disabled: false,
        };
        let over = widget::scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered: false,
            is_vertical_scrollbar_hovered: false,
            is_horizontal_scrollbar_disabled: false,
            is_vertical_scrollbar_disabled: false,
        };
        assert_eq!(scroller_alpha(&theme::light(), resting), 0.0);
        assert!(scroller_alpha(&theme::light(), over) > 0.0);
        assert!(scroller_alpha(&theme::theme(), resting) > 0.0, "the dark theme keeps it visible");
    }

    type ButtonStyle = Box<dyn Fn(&Theme, Status) -> Style>;
    type CheckStyle = Box<dyn Fn(&Theme, widget::checkbox::Status) -> widget::checkbox::Style>;

    fn checks(colour: Color) -> [CheckStyle; 2] {
        [Box::new(checkbox(colour)), Box::new(task_check(colour))]
    }

    const BUTTON_STATES: [Status; 4] = [Status::Active, Status::Hovered, Status::Pressed, Status::Disabled];

    fn modes() -> [(&'static str, Theme, &'static Palette); 2] {
        [("dark", theme::theme(), Palette::dark()), ("light", theme::light(), Palette::light())]
    }

    /// Every button style with its name, including both variants of the parametrised ones.
    fn buttons() -> Vec<(&'static str, ButtonStyle)> {
        vec![
            ("primary", Box::new(button::primary)),
            ("secondary", Box::new(button::secondary)),
            ("ghost", Box::new(button::ghost)),
            ("icon", Box::new(button::icon)),
            ("boxed", Box::new(button::boxed)),
            ("danger", Box::new(button::danger)),
            ("segment", Box::new(button::segment(false))),
            ("segment(active)", Box::new(button::segment(true))),
            ("sidebar_item", Box::new(button::sidebar_item(false))),
            ("sidebar_item(active)", Box::new(button::sidebar_item(true))),
            ("list_row", Box::new(button::list_row(false))),
            ("list_row(selected)", Box::new(button::list_row(true))),
            ("menu_item", Box::new(button::menu_item(false))),
            ("menu_item(highlighted)", Box::new(button::menu_item(true))),
            ("chip", Box::new(button::chip)),
            ("choice_chip", Box::new(button::choice_chip(false))),
            ("choice_chip(on)", Box::new(button::choice_chip(true))),
        ]
    }

    fn bg(style: &Style) -> Color {
        match style.background {
            Some(Background::Color(c)) => c,
            _ => Color::TRANSPARENT,
        }
    }

    /// `top` composited over the opaque `under`.
    fn over(top: Color, under: Color) -> Color {
        let mix = |a: f32, b: f32| a * top.a + b * (1.0 - top.a);
        Color::from_rgb(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b))
    }

    fn same(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-4 && (a.g - b.g).abs() < 1e-4 && (a.b - b.b).abs() < 1e-4 && (a.a - b.a).abs() < 1e-4
    }

    #[test]
    fn selection_is_never_red() {
        for (mode, theme, p) in modes() {
            for (name, style) in buttons().into_iter().filter(|(name, _)| *name != "primary") {
                for status in BUTTON_STATES {
                    let s = style(&theme, status);
                    assert!(
                        !same(bg(&s), p.accent) && !same(s.text_color, p.accent),
                        "{mode}: {name} {status:?} is red"
                    );
                }
            }
            assert!(same(bg(&button::segment(true)(&theme, Status::Hovered)), p.selected), "{mode}: active segment");
            assert!(same(bg(&button::sidebar_item(true)(&theme, Status::Active)), p.selected));
            assert!(
                same(bg(&button::list_row(true)(&theme, Status::Hovered)), p.selected),
                "{mode}: hover hides selection"
            );
            let open = pick_list(&theme, widget::pick_list::Status::Opened { is_hovered: false });
            assert_eq!(open.background, Background::Color(p.selected));
            assert_eq!(pick_list_menu(&theme).selected_background, Background::Color(p.selected));
            for on in [true, false] {
                let t = toggler(&theme, widget::toggler::Status::Active { is_toggled: on });
                assert_ne!(t.background, Background::Color(p.accent), "{mode}: toggler is red");
            }
            let r = radio(&theme, widget::radio::Status::Active { is_selected: true });
            assert!(!same(r.dot_color, p.accent) && !same(r.border_color, p.accent));
        }
    }

    #[test]
    fn primary_is_the_only_red_button_and_stays_readable() {
        for (mode, theme, p) in modes() {
            assert!(same(bg(&button::primary(&theme, Status::Active)), p.accent));
            for status in [Status::Active, Status::Hovered, Status::Pressed] {
                let s = button::primary(&theme, status);
                let ratio = s.text_color.relative_contrast(bg(&s));
                assert!(ratio >= 5.35, "{mode}: white on primary {status:?} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn hover_and_press_use_the_neutral_tokens() {
        for (mode, theme, p) in modes() {
            for (name, style) in buttons() {
                if name == "primary" || name.contains('(') || name == "menu_item" {
                    continue;
                }
                assert!(same(bg(&style(&theme, Status::Hovered)), p.hover), "{mode}: {name} hover");
                assert!(same(bg(&style(&theme, Status::Pressed)), p.selected), "{mode}: {name} pressed");
            }
            for status in [Status::Hovered, Status::Pressed] {
                assert!(
                    same(bg(&button::menu_item(false)(&theme, status)), p.selected),
                    "{mode}: menu item {status:?}"
                );
            }
            assert!(same(bg(&button::menu_item(true)(&theme, Status::Active)), p.selected), "{mode}: highlighted item");
            // On a menu the highlight must stand out; `hover` would not in dark mode.
            assert!(p.selected.relative_contrast(p.menu) > p.hover.relative_contrast(p.menu));
            assert_eq!(text_input(&theme, widget::text_input::Status::Hovered).background, Background::Color(p.hover));
        }
    }

    #[test]
    fn framed_controls_strengthen_their_border_on_hover_and_press() {
        for (mode, theme, p) in modes() {
            let framed: [(&str, ButtonStyle); 4] = [
                ("secondary", Box::new(button::secondary)),
                ("boxed", Box::new(button::boxed)),
                ("chip", Box::new(button::chip)),
                ("choice_chip", Box::new(button::choice_chip(false))),
            ];
            for (name, style) in framed {
                assert_eq!(style(&theme, Status::Active).border.color, p.line_strong, "{mode}: {name} at rest");
                for status in [Status::Hovered, Status::Pressed] {
                    assert_eq!(style(&theme, status).border.color, p.line_selected, "{mode}: {name} {status:?}");
                }
            }
            let hovered = pick_list(&theme, widget::pick_list::Status::Hovered);
            assert_eq!(hovered.border.color, p.line_selected, "{mode}: pick list hover");
            assert_eq!(pick_list(&theme, widget::pick_list::Status::Active).border.color, p.line_strong);
        }
    }

    #[test]
    fn a_choice_reads_as_chosen_in_every_state() {
        for (mode, theme, p) in modes() {
            let chosen: [(&str, ButtonStyle); 2] = [
                ("choice_chip(on)", Box::new(button::choice_chip(true))),
                ("segment(active)", Box::new(button::segment(true))),
            ];
            for (name, style) in chosen {
                for status in [Status::Active, Status::Hovered, Status::Pressed] {
                    let s = style(&theme, status);
                    assert!(same(bg(&s), p.selected), "{mode}: {name} {status:?} fill");
                    assert_eq!(s.border.color, p.line_selected, "{mode}: {name} {status:?} edge");
                    assert_eq!(s.text_color, p.text, "{mode}: {name} {status:?} text");
                }
            }
            // Not chosen: quieter text at rest, full text under the pointer.
            let off = button::choice_chip(false);
            assert_eq!(off(&theme, Status::Active).text_color, p.text_2, "{mode}");
            assert_eq!(off(&theme, Status::Hovered).text_color, p.text, "{mode}");
            assert_eq!(button::choice_chip(true)(&theme, Status::Active).border.radius, radius::SM.into());
        }
    }

    #[test]
    fn a_busy_button_keeps_its_look_and_is_not_faded() {
        for (mode, theme, p) in modes() {
            let busy = button::busy(button::primary);
            for status in BUTTON_STATES {
                let s = busy(&theme, status);
                assert!(same(bg(&s), p.accent), "{mode}: busy primary {status:?} is not plain red");
                assert_eq!(s.text_color, p.on_accent, "{mode}: busy text {status:?}");
            }
            let secondary = button::busy(button::secondary)(&theme, Status::Disabled);
            assert!(same(bg(&secondary), p.raised), "{mode}: busy secondary");
        }
    }

    #[test]
    fn disabled_is_the_normal_look_faded_without_hover() {
        for (mode, theme, _) in modes() {
            for (name, style) in buttons() {
                let (rest, off) = (style(&theme, Status::Active), style(&theme, Status::Disabled));
                assert!(same(bg(&off), faded(bg(&rest))), "{mode}: {name} disabled background");
                assert!(same(off.text_color, faded(rest.text_color)), "{mode}: {name} disabled text");
                assert!(same(off.border.color, faded(rest.border.color)), "{mode}: {name} disabled border");
            }
            let input = text_input(&theme, widget::text_input::Status::Disabled);
            assert_eq!(input.background, Background::Color(Color::TRANSPARENT), "{mode}: disabled input has hover");
            assert!((input.value.a - DISABLED_ALPHA).abs() < 1e-4);
            for checked in [true, false] {
                let off = widget::checkbox::Status::Disabled { is_checked: checked };
                let rest = widget::checkbox::Status::Active { is_checked: checked };
                for style in checks(Palette::dark().calendar[1]) {
                    let (off, rest) = (style(&theme, off), style(&theme, rest));
                    let (Background::Color(off_bg), Background::Color(rest_bg)) = (off.background, rest.background)
                    else {
                        panic!("check without colour background");
                    };
                    assert!(same(off_bg, faded(rest_bg)), "{mode}: disabled check ({checked}) has hover");
                }
                let t = toggler(&theme, widget::toggler::Status::Disabled { is_toggled: checked });
                let r = toggler(&theme, widget::toggler::Status::Active { is_toggled: checked });
                let (Background::Color(t_bg), Background::Color(r_bg)) = (t.background, r.background) else {
                    panic!("toggler without colour background");
                };
                assert!(same(t_bg, faded(r_bg)), "{mode}: disabled toggler ({checked})");
            }
        }
    }

    #[test]
    fn styles_follow_the_mode() {
        for (mode, theme, p) in modes() {
            assert!(same(bg(&button::secondary(&theme, Status::Active)), p.raised), "{mode}");
            assert_eq!(button::ghost(&theme, Status::Active).text_color, p.text_2, "{mode}");
            assert_eq!(container::panel(&theme).background, Some(Background::Color(p.panel)), "{mode}");
            assert_eq!(container::sidebar(&theme).background, Some(Background::Color(p.bg)), "{mode}");
            assert_eq!(text_input(&theme, widget::text_input::Status::Active).placeholder, p.text_3, "{mode}");
            assert_eq!(rule(&theme).color, p.line, "{mode}");
        }
        assert_ne!(container::menu(&theme::theme()).background, container::menu(&theme::light()).background);
    }

    #[test]
    fn focus_is_a_two_pixel_ring_never_red() {
        for (mode, theme, p) in modes() {
            let focused = text_input(&theme, widget::text_input::Status::Focused { is_hovered: true });
            assert_eq!((focused.border.color, focused.border.width), (p.focus_ring, FOCUS_RING_WIDTH), "{mode}");
            let editor = text_editor(&theme, widget::text_editor::Status::Focused { is_hovered: false });
            assert_eq!(editor.border.color, p.focus_ring, "{mode}");
            // A 1-px ring would pass 3:1 too, but only just: the ring must read against the panel.
            let ring = over(p.focus_ring, p.panel);
            assert!(
                ring.relative_contrast(p.panel) >= 3.0,
                "{mode}: focus ring is {:.2}:1",
                ring.relative_contrast(p.panel)
            );
        }
    }

    #[test]
    fn danger_text_is_readable() {
        for (mode, theme, p) in modes() {
            for status in [Status::Active, Status::Hovered, Status::Pressed] {
                let s = button::danger(&theme, status);
                for (surface, under) in [("panel", p.panel), ("raised", p.raised), ("menu", p.menu)] {
                    let ratio = s.text_color.relative_contrast(over(bg(&s), under));
                    assert!(ratio >= 4.5, "{mode}: danger {status:?} on {surface} is {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn check_marks_are_visible_on_every_calendar_colour() {
        for (mode, theme, p) in modes() {
            for colour in p.calendar {
                for style in checks(colour) {
                    let s = style(&theme, widget::checkbox::Status::Active { is_checked: true });
                    assert_eq!(s.background, Background::Color(colour));
                    let ratio = s.icon_color.relative_contrast(colour);
                    assert!(ratio >= 3.0, "{mode}: check mark on {colour:?} is {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn calendar_check_is_square_and_task_check_round() {
        let theme = theme::theme();
        let status = widget::checkbox::Status::Active { is_checked: false };
        let square = checkbox(Palette::dark().calendar[0])(&theme, status);
        let round = task_check(Palette::dark().calendar[0])(&theme, status);
        assert_eq!(square.border.radius, radius::XS.into());
        assert!(round.border.radius.top_left >= CHECK_SIZE / 2.0);
        assert_eq!(round.border.color, Palette::dark().text_3, "an open task is a grey ring");
        assert_eq!(square.border.color, Palette::dark().calendar[0], "a hidden calendar keeps its colour");
    }

    #[test]
    fn surfaces_have_the_radii_and_shadows_of_the_skill() {
        let theme = theme::light();
        let p = Palette::light();
        assert_eq!(container::panel(&theme).border.radius, radius::MD.into());
        assert_eq!(container::panel(&theme).border.color, p.line);
        assert_eq!(container::panel_flush(&theme).border.width, 0.0);
        assert_eq!(container::menu(&theme).border.radius, radius::MD.into());
        assert_eq!(container::popover(&theme).border.radius, radius::LG.into());
        assert_eq!(container::menu(&theme).shadow, p.shadow);
        assert_eq!(container::popover(&theme).shadow, p.shadow);
        assert_eq!(container::chip(&theme).border.radius, radius::XS.into());
        assert_eq!(container::kbd(&theme).text_color, Some(p.text_3));
        assert_eq!(pick_list_menu(&theme).shadow, p.shadow);
        assert_eq!(button::secondary(&theme, Status::Active).border.radius, radius::SM.into());
        assert_eq!(button::segment(true)(&theme, Status::Active).border.radius, radius::XS.into());
    }
}
