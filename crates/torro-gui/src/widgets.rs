//! Building blocks of a Linear-style window: buttons (also busy), sidebar rows, list rows, group
//! heads, the action bar of a row ([`row_actions`]), the toast with "Undo" ([`toast`]), menus, floating cards, chips (also to choose), property rows (also with the name in
//! front), label chips, state pills, progress bars, settings groups and the empty, loading and
//! error states. For the comfortable density of the inbox (task E8.2): cards ([`card`],
//! [`card_head`]) and source tiles ([`source_tile`]) – which density a window draws in is the
//! design's to say (task E13.3, `crate::layout::density`), so there is no switch for it.
//! For figures and charts (task E8.8b, the hub's "Insights"): a figure tile
//! ([`stat_tile`]), a stacked column chart with its legend ([`column_chart`], [`chart_legend`]), a
//! neutral share bar ([`share_bar`]) and a table of figures ([`data_table`]). For rights (task U.13):
//! rows with a segmented control in a common column ([`segment_row`], [`segment_column`]). For the
//! mail window's large buttons (task E11.1): the reader's command bar ([`command_bar`] with
//! [`command_cell`], [`command_split`], [`command_more`]), the strip in the chosen mail's row
//! ([`row_strip`]), the Torro pills ([`pill_button`]), the sunken view switch ([`view_switch`]) and
//! the chips of attachments ([`file_chip`]).
//!
//! Each block puts a style from [`crate::style`], the sizes from [`crate::theme`] and a level from
//! [`crate::typo`] together, so apps write no numbers and no colours (design skill, sections 0 and
//! 5; reference `docs/prototypes/linear/`). The window frame itself (sidebar, header, properties
//! column) is [`crate::shell`].
//!
//! # Menus and floating cards
//!
//! Iced 0.14 (without its `advanced` feature) has no dropdown widget, so a menu is two pieces:
//!
//! 1. the **trigger** registers where it is on screen in an [`Anchor`] ([`menu_button`],
//!    [`select_button`], or any widget wrapped in [`anchored`]);
//! 2. the **layer** ([`floating`]) draws the card ([`menu`] or [`popover`]) next to that anchor,
//!    above the whole window, and sends `on_dismiss` on a click next to it. The app hands the layer
//!    to [`crate::shell::Shell::overlay`] and closes it on Escape with [`escape`].
//!
//! The app keeps which menu is open (`Option<MyMenu>`); a new [`Anchor`] is made in every `view`.
//!
//! ```ignore
//! use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Trail};
//!
//! // view
//! let nav = Anchor::new();
//! let title = widgets::menu_button(&nav, Some(Icon::Sun), tr("Today"), Message::ToggleMenu(Menu::Nav));
//! let layer = (self.menu == Some(Menu::Nav)).then(|| {
//!     let items = vec![
//!         MenuItem::new(Icon::Inbox, tr("Inbox"), Message::Show(View::Inbox)).trail(Trail::Count(2)),
//!         MenuItem::new(Icon::Sun, tr("Today"), Message::Show(View::Today)).current(true),
//!     ];
//!     widgets::floating(&nav, Placement::Below(Align::Start), widgets::menu(vec![MenuSection::new(items)]), Message::CloseMenu)
//! });
//! shell(class, sidebar, header_bar([], title, []), body, None).overlay(layer).into()
//!
//! // subscription: Escape closes it
//! if self.menu.is_some() { widgets::escape().map(|()| Message::CloseMenu) } else { Subscription::none() }
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{
    button, canvas, center, column, container, float, mouse_area, opaque, pin, responsive, row, rule, stack, svg,
    tooltip,
};
use iced::{
    Alignment, Background, Color, Element, Fill, Length, Padding, Point, Rectangle, Size, Subscription, Theme, Vector,
};
use iced::{border, keyboard, mouse};

use torro_core::i18n::tr;
use torro_core::shortcuts::{Held, System};
use torro_ipc::api::{Confirm, Level};

use crate::icon::{self, Icon, icon};
use crate::layout;
use crate::shell::{SidebarMode, is_sidebar_key};
use crate::style;
use crate::theme::{self, ChartColor, Palette, palette, radius, size, space};
use crate::typo;

/// A button style from [`crate::style::button`] (`style::button::primary`, `ghost` …).
pub type ButtonStyle = fn(&Theme, button::Status) -> button::Style;

/// Width of a menu card ([`menu`]): the sidebar width, as in the mock-ups.
pub fn menu_width() -> f32 {
    size::sidebar()
}
/// Width of a floating card ([`popover`]).
pub const POPOVER_WIDTH: f32 = 320.0;
/// Width of a small dialog over the window ([`modal`]) that asks one thing or holds a short form:
/// "New list" with its [`choice_cards`] and the form "New task list" after it. One width for
/// both steps, so the dialog does not jump when it goes from one to the other. Larger forms (a
/// GitHub filter list) take [`size::form_max()`].
pub const DIALOG_WIDTH: f32 = 560.0;
/// Gap between the anchor and a floating card below it.
fn below_gap() -> f32 {
    space::xs()
}
/// Gap between the anchor and a floating card beside it, and the least margin to the window edge.
fn beside_gap() -> f32 {
    space::s()
}
/// Gap between icon and text inside a button or chip (skill: 6 is allowed inside building blocks).
const ICON_GAP: f32 = 6.0;
/// Gap between the cells of a list row (`tasks.html`: `.row{gap:10px}`).
const ROW_GAP: f32 = 10.0;
/// The one-pixel lines of the design.
const LINE: f32 = size::HAIRLINE;
/// Height of a list pill ([`pill`]) and of its round ends.
const PILL_HEIGHT: f32 = 20.0;
/// Edge of the icon in an [`icon_pill`] (the mock-up's `.s12`).
fn pill_icon() -> f32 {
    space::m()
}
/// Edge of an avatar in [`avatar_stack`] and how far the next one is moved right.
const AVATAR: f32 = 22.0;
const AVATAR_STEP: f32 = 20.0;
/// Edge of the chevron after a sidebar group title.
const GROUP_CHEVRON: f32 = 12.0;
/// Width of the bar that marks the selected list row.
const SELECTED_BAR: f32 = 2.0;
/// Width of the label column of a [`prop_line`] ("Priority", "Due"): the value starts at the same
/// x in every row of the properties column.
const PROP_LABEL_WIDTH: f32 = 88.0;

/// How wide the label of a property row is ([`prop_line`]), for views that line up their own rows
/// with property rows (the facts of the reminder window, task ER2.1).
pub fn prop_label_width() -> f32 {
    PROP_LABEL_WIDTH
}
/// Top and bottom padding of a [`prop_line`]: with the 20-px line of `BODY` that makes the 32-px
/// [`size::control()`]; a value of several lines (wrapping label chips) makes the row taller.
const PROP_PADDING_Y: f32 = 6.0;
/// Height of the pill with a state word ([`state_pill`]) and of its round ends.
const STATE_PILL_HEIGHT: f32 = 22.0;
/// Size of the progress bar ([`progress_bar`]); its width is public so a head can tell whether the
/// bar fits beside its words.
pub const PROGRESS_WIDTH: f32 = 64.0;
const PROGRESS_HEIGHT: f32 = 4.0;

// ---------------------------------------------------------------------------------------------
// Marks and trailing details, shared by sidebar rows, menu items, chips and property rows.
// ---------------------------------------------------------------------------------------------

/// What stands in front of a label: an icon, or the colour of a list or calendar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    /// An icon in the row's tone (`text_2` in the sidebar, `text` in menus, `text_3` in properties).
    Icon(Icon),
    /// An icon in a colour of its own, e.g. the attention colour for "Overdue".
    Tinted(Icon, Color),
    /// A ring in a list colour (task lists).
    Ring(Color),
    /// A filled dot in a calendar colour.
    Dot(Color),
    /// A small square check box in a calendar colour, checked or not (calendar visibility in a menu).
    Check(Color, bool),
}

impl From<Icon> for Mark {
    fn from(icon: Icon) -> Self {
        Mark::Icon(icon)
    }
}

/// What stands at the end of a sidebar row or menu item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trail {
    /// Nothing.
    None,
    /// A counter (`META`, tabular, `text_3`).
    Count(usize),
    /// A key hint ([`kbd`]), e.g. `"W"`.
    Kbd(&'static str),
    /// A small icon in `text_3`, e.g. [`Icon::ArrowUpRight`] for "Open Calendar".
    Icon(Icon),
    /// A small status dot, e.g. green for a connected account.
    Dot(Color),
}

/// The text tone of a row; icons follow it, because Iced cannot pass the text colour to an SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Text,
    Text2,
    Text3,
}

impl Tone {
    fn of(self, p: &Palette) -> Color {
        match self {
            Tone::Text => p.text,
            Tone::Text2 => p.text_2,
            Tone::Text3 => p.text_3,
        }
    }
}

/// Mark sizes: `Row` in sidebar, menu and property rows, `Small` in chips and crumbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scale {
    Row,
    Small,
}

impl Scale {
    fn icon(self) -> f32 {
        match self {
            Scale::Row => icon::SIZE,
            Scale::Small => icon::SIZE_SM,
        }
    }

    /// Edge of a ring or check box (`torro.css`: `.ring` 14, inside chips 12).
    fn ring(self) -> f32 {
        self.icon() - 2.0
    }

    /// Edge of a dot (`torro.css`: `.dot` 8, inside pills and trails 6).
    fn dot(self) -> f32 {
        match self {
            Scale::Row => space::s(),
            Scale::Small => ICON_GAP,
        }
    }
}

/// `which` in the colour `tone` picks from the current palette.
pub(crate) fn toned<'a, M: 'a>(which: Icon, edge: f32, tone: Tone) -> Element<'a, M> {
    icon(which, edge).style(move |t, _| svg::Style { color: Some(tone.of(&palette(t))) }).into()
}

/// `which` in a fixed colour (a status or list colour).
fn tinted<'a, M: 'a>(which: Icon, edge: f32, colour: Color) -> Element<'a, M> {
    icon(which, edge).style(move |_, _| svg::Style { color: Some(colour) }).into()
}

fn mark_view<'a, M: 'a>(mark: Mark, scale: Scale, tone: Tone) -> Element<'a, M> {
    match mark {
        Mark::Icon(which) => toned(which, scale.icon(), tone),
        Mark::Tinted(which, colour) => tinted(which, scale.icon(), colour),
        Mark::Ring(colour) => {
            let edge = scale.ring();
            container(blank()).width(edge).height(edge).style(move |_| ring(colour, edge)).into()
        }
        Mark::Dot(colour) => color_dot(colour, scale.dot()),
        Mark::Check(colour, checked) => {
            let edge = scale.ring();
            let inner: Element<'a, M> = if checked { tinted(Icon::Check, edge - 4.0, colour) } else { blank().into() };
            container(inner)
                .center(edge)
                .style(move |_| container::Style {
                    border: border::rounded(radius::xs()).color(colour).width(style::CHECK_BORDER),
                    ..container::Style::default()
                })
                .into()
        }
    }
}

/// A transparent circle with a 2-px border in `colour`.
fn ring(colour: Color, edge: f32) -> container::Style {
    container::Style { border: border::rounded(edge / 2.0).color(colour).width(2), ..container::Style::default() }
}

fn trail_view<'a, M: 'a>(trail: Trail) -> Option<Element<'a, M>> {
    match trail {
        Trail::None => None,
        Trail::Count(n) => Some(typo::text(typo::meta().tabular(), n.to_string()).style(theme::text_3).into()),
        Trail::Kbd(key) => Some(kbd(key)),
        Trail::Icon(which) => Some(toned(which, icon::SIZE_SM, Tone::Text3)),
        Trail::Dot(colour) => Some(color_dot(colour, Scale::Small.dot())),
    }
}

/// A single-line label that takes the free width and is clipped instead of wrapping (Iced 0.14
/// has no ellipsis). Without a colour it takes the button's text colour, so it follows hover.
fn line_label<'a, M: 'a>(level: typo::Level, label: impl IntoFragment<'a>) -> Element<'a, M> {
    container(typo::text(level, label).wrapping(Wrapping::None)).width(Fill).clip(true).into()
}

// ---------------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------------

/// The colour an icon inside a button of `style` takes: the button's text colour at rest.
fn button_icon<'a, M: 'a>(which: Icon, edge: f32, style: ButtonStyle, on: bool) -> Element<'a, M> {
    icon(which, edge)
        .style(move |t, _| svg::Style { color: Some(style(t, button::Status::Active).text_color) })
        .opacity(icon_opacity(on))
        .into()
}

/// The opacity of the icon in a button that is `on` or off (issue #168): an icon's tint keeps no
/// alpha (Iced takes only its colour), so a button that is off fades its icon through the opacity
/// of the image instead – to [`style::DISABLED_ALPHA`], like its word and frame.
fn icon_opacity(on: bool) -> f32 {
    if on { 1.0 } else { style::DISABLED_ALPHA }
}

/// A text button, 28 high ([`size::control_sm()`]), label in `CAPTION`, optional icon (14) in front.
///
/// `style` is one of [`crate::style::button`]: `primary` (at most one per window), `secondary`,
/// `ghost` (the default in header bars) or `danger`. `None` for `on_press` shows it disabled.
///
/// ```ignore
/// widgets::text_button(Some(Icon::Plus), tr("New task"), style::button::primary, Some(Message::New))
/// ```
pub fn text_button<'a, M: Clone + 'a>(
    icon: Option<Icon>,
    label: impl IntoFragment<'a>,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if let Some(which) = icon {
        content = content.push(button_icon(which, icon::SIZE_SM, style, on_press.is_some()));
    }
    content = content.push(typo::text(typo::caption(), label).wrapping(Wrapping::None));
    button(container(content).center_y(Fill))
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style)
        .on_press_maybe(on_press)
        .into()
}

/// A large button of a reminder over the whole screen (task ER2.1): [`size::control_xl()`] high,
/// label in `typo::body_strong()`, optional icon (16) in front and the key that presses it as a
/// [`kbd`] behind ("Enter", "S", "Esc"). `style` as for [`text_button`]; `None` for `on_press`
/// shows it disabled.
pub fn big_button<'a, M: Clone + 'a>(
    icon: Option<Icon>,
    label: impl IntoFragment<'a>,
    key: Option<&'a str>,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(which) = icon {
        content = content.push(button_icon(which, icon::SIZE, style, on_press.is_some()));
    }
    content = content.push(typo::text(typo::body_strong(), label).wrapping(Wrapping::None));
    if let Some(key) = key {
        // In the button's own text colour: `text_3` would be too faint on the primary's red.
        let hint = typo::text(typo::caption(), key)
            .style(move |t| iced::widget::text::Style { color: Some(style(t, button::Status::Active).text_color) });
        content = content.push(container(hint).padding(style::KBD_PADDING).style(style::container::kbd));
    }
    button(container(content).center_y(Fill))
        .height(size::control_xl())
        .padding([0.0, space::l()])
        .style(style)
        .on_press_maybe(on_press)
        .into()
}

/// A [`text_button`] while the action it started runs ("Creating…", "Saving…"): the same size and
/// `style` (usually `style::button::primary`), the label `label` from the caller, led by the
/// [`Icon::RefreshCw`] arrows (14) as a quiet "working" sign. It has no message, so a second click
/// does nothing, yet it keeps the full look of `style` (`style::button::busy`) instead of the faded
/// disabled one, so it reads as working, not as unavailable. Nothing moves (the design skill: no
/// spinners); the label says what is happening.
///
/// Show it instead of the normal button while the request is out, and the normal one again when
/// the answer is there:
///
/// ```ignore
/// if self.saving {
///     widgets::busy_button(tr("Creating…"), style::button::primary)
/// } else {
///     widgets::text_button(None, tr("Create list"), style::button::primary, Some(Message::Save))
/// }
/// ```
pub fn busy_button<'a, M: Clone + 'a>(label: impl IntoFragment<'a>, style: ButtonStyle) -> Element<'a, M> {
    let content = row![
        button_icon(Icon::RefreshCw, icon::SIZE_SM, style, true),
        typo::text(typo::caption(), label).wrapping(Wrapping::None)
    ]
    .spacing(ICON_GAP)
    .align_y(Alignment::Center);
    button(container(content).center_y(Fill))
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::busy(style))
        .into()
}

/// `content` with `label` as tooltip below it (a small menu-style card, after a short delay).
/// Also for a control that is locked, to say why (the locked check of an issue, task G.5).
pub fn with_tooltip<'a, M: 'a>(content: impl Into<Element<'a, M>>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    tip(content, typo::text(typo::caption(), label).style(theme::text_2), tooltip::Position::Bottom)
}

/// `content` with `label` and its `trail` (count, key hint …) as tooltip at its right: the names of
/// the rows in the collapsed sidebar, which point into the content.
pub(crate) fn tooltip_right<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    trail: Trail,
) -> Element<'a, M> {
    let text = row![typo::text(typo::caption(), label).style(theme::text_2).wrapping(Wrapping::None)]
        .push(trail_view(trail))
        .spacing(space::s())
        .align_y(Alignment::Center);
    tip(content, text, tooltip::Position::Right)
}

/// The tooltip card (menu surface, after a short delay) with `text` at `position` of `content`.
fn tip<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    text: impl Into<Element<'a, M>>,
    position: tooltip::Position,
) -> Element<'a, M> {
    let card = container(text)
        .padding([style::KBD_PADDING.top + style::KBD_PADDING.bottom, space::s()])
        .style(style::container::menu);
    tooltip(content, card, position).gap(space::xs()).delay(Duration::from_millis(500)).into()
}

/// An icon-only button in `style`, 28 high; `square` makes it 28 × 28, otherwise it keeps the
/// button padding (for an icon-only primary button in the narrow layout).
fn icon_only<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    let b = button(center(button_icon(which, icon::SIZE, style, on_press.is_some())))
        .width(size::control_sm())
        .height(size::control_sm())
        .padding(0)
        .style(style)
        .on_press_maybe(on_press);
    with_tooltip(b, tooltip)
}

/// An icon button (‹ › refresh, filter …): 28 × 28, ghost, icon 16 in `text_2`, hover `hover`.
/// `tooltip` names the action (it is the only label, so it must be there). `None` disables it.
pub fn icon_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style::button::icon, on_press)
}

/// An icon button with a frame (`style::button::boxed`), 28 × 28: "New" next to the app switcher.
pub fn boxed_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style::button::boxed, on_press)
}

/// A text button reduced to its icon, for the narrow layout where the label does not fit (the
/// primary "+" in a narrow header): 28 × 28 with the icon at 16 in the style's text colour
/// (`on_accent` for `primary`), as `.lhead .btn.primary` in the tasks mock-up. The larger icon
/// keeps its thin strokes readable on the red; `tooltip` keeps the label.
pub fn compact_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style, on_press)
}

/// A segmented control (Day · Week · Month): one segment per `(label, message)`, the segment at
/// index `active` is chosen. Frame on `bg`, 28 high, labels `CAPTION`.
pub fn segmented<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (L, M)>,
    active: usize,
) -> Element<'a, M> {
    let inner = size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0 * LINE;
    let segments = options.into_iter().enumerate().map(|(i, (label, message))| {
        button(container(typo::text(typo::caption(), label).wrapping(Wrapping::None)).center_y(Fill))
            .height(inner)
            .padding(style::button_padding())
            .style(style::button::segment(i == active))
            .on_press(message)
            .into()
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
}

// ---------------------------------------------------------------------------------------------
// Bars of actions over a page (task E9.4): the reader's standard bar and its floating Torro bar
// ---------------------------------------------------------------------------------------------

/// A button of a bar over a page (task E9.4: the mail reader's standard bar at the top and the
/// floating bar at the bottom): one step taller than [`text_button`] – [`size::control()`], 32 –
/// so it reads as a button, not as a link.
///
/// With `words`: the icon (14) and `label` in `CAPTION` with the button padding, as wide as
/// [`text_button_width`]`(true, label)` says. Without: a square [`size::control()`] with the icon at
/// 16 and `label` as its tooltip, [`size::control()`] wide. `style` is one of
/// [`crate::style::button`]: `primary` for the window's one main action, `secondary` for the
/// others, `icon` for quiet icons at the end of a bar. `None` for `on_press` shows it off.
///
/// ```ignore
/// widgets::bar_button(Icon::Reply, tr("Reply"), words, style::button::primary, Some(Message::Reply))
/// ```
pub fn bar_button<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    words: bool,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    let on = on_press.is_some();
    bar_button_with(
        button_icon(which, if words { icon::SIZE_SM } else { icon::SIZE }, style, on),
        label,
        words,
        style,
        on_press,
    )
}

/// A [`bar_button`] whose icon is in the accent colour instead of the button's text colour: a
/// function of its own that the bar wants recognised at a glance – the AI buttons with
/// [`Icon::Sparkles`] (task E9.4). The word stays in the button's colour; off, the icon fades with it.
pub fn bar_button_accent<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    words: bool,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    let edge = if words { icon::SIZE_SM } else { icon::SIZE };
    let mark = icon(which, edge)
        .style(|t, _| svg::Style { color: Some(palette(t).accent) })
        .opacity(icon_opacity(on_press.is_some()));
    bar_button_with(mark.into(), label, words, style, on_press)
}

/// The [`bar_button`] around `mark`: with its word or square with a tooltip.
fn bar_button_with<'a, M: Clone + 'a>(
    mark: Element<'a, M>,
    label: impl IntoFragment<'a>,
    words: bool,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    if words {
        let content = row![mark, typo::text(typo::caption(), label).wrapping(Wrapping::None)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center);
        return button(container(content).center_y(Fill))
            .height(size::control())
            .padding(style::button_padding())
            .style(style)
            .on_press_maybe(on_press)
            .into();
    }
    let square = button(center(mark))
        .width(size::control())
        .height(size::control())
        .padding(0)
        .style(style)
        .on_press_maybe(on_press);
    with_tooltip(square, label)
}

/// A [`segmented`] control as tall as a [`bar_button`] ([`size::control()`]), for the switches in
/// the same bars ("HTML | Text", "Light | Dark"). As wide as [`segmented_width`] estimates.
pub fn bar_segmented<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (L, M)>,
    active: usize,
) -> Element<'a, M> {
    let inner = size::control() - 2.0 * style::SEGMENT_INSET - 2.0 * LINE;
    let segments = options.into_iter().enumerate().map(|(i, (label, message))| {
        button(container(typo::text(typo::caption(), label).wrapping(Wrapping::None)).center_y(Fill))
            .height(inner)
            .padding(style::button_padding())
            .style(style::button::segment(i == active))
            .on_press(message)
            .into()
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
}

/// The padding inside a [`floating_bar`], around its rows, and the gap between its rows and
/// controls.
pub fn floating_bar_gap() -> f32 {
    space::xs()
}

/// A bar that floats over a page (task E9.4, the mail reader's Torro bar): `content` – rows of
/// [`bar_button`]s and [`bar_segmented`] switches, [`floating_bar_gap`] apart – on the menu surface
/// with its frame and shadow and the radius of floating cards ([`style::container::popover`]),
/// [`floating_bar_gap`] inside. Place it with [`over_bar`]; it is as wide as its content.
pub fn floating_bar<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).padding(floating_bar_gap()).style(style::container::popover).into()
}

/// The height of a [`floating_bar`] of `rows` rows of [`size::control()`]: the rows, the gaps
/// between them, the padding and the frame.
pub fn floating_bar_height(rows: usize) -> f32 {
    let rows = rows.max(1) as f32;
    rows * size::control() + (rows - 1.0) * floating_bar_gap() + 2.0 * (floating_bar_gap() + LINE)
}

/// The room for the controls of a [`floating_bar`] over a page `width` wide: the page less the bar's
/// margin ([`space::m()`]) at both sides, its padding and its frame.
pub fn floating_bar_room(width: f32) -> f32 {
    (width - 2.0 * (space::m() + floating_bar_gap() + LINE)).max(0.0)
}

/// How much room a page under a [`floating_bar`] of `rows` rows keeps at its end (its bottom
/// padding), so its last line and what follows it scroll out from under the bar: the bar and its
/// margin below and above it ([`space::m()`]).
pub fn floating_bar_clearance(rows: usize) -> f32 {
    floating_bar_height(rows) + 2.0 * space::m()
}

/// `page` with `bar` ([`floating_bar`]) floating over it, fixed at the bottom, centred,
/// [`space::m()`] from the bottom and the sides. Give the scrolling page the bottom padding of
/// [`floating_bar_clearance`]. A click or the wheel beside the bar reaches the page below.
///
/// ```ignore
/// let rows = 2;
/// let page = scrollable(container(content).padding(Padding { bottom: widgets::floating_bar_clearance(rows), ..pad }));
/// widgets::over_bar(page, widgets::floating_bar(column![first_row, second_row].spacing(widgets::floating_bar_gap())))
/// ```
pub fn over_bar<'a, M: 'a>(page: impl Into<Element<'a, M>>, bar: impl Into<Element<'a, M>>) -> Element<'a, M> {
    let layer =
        container(bar).width(Fill).height(Fill).padding(space::m()).align_x(Alignment::Center).align_y(Alignment::End);
    stack![page.into(), layer].width(Fill).height(Fill).into()
}

// ---------------------------------------------------------------------------------------------
// The mail window's large buttons (task E11.1): command bar, strip under a row, pills, view switch
// ---------------------------------------------------------------------------------------------
//
// Actions are cells or pills, the view is sunk: what is done to the mail stands raised
// (`style::button::cell` on `raised`, `style::button::pill`), how the mail is shown sits in a
// sunken track (`style::container::track`). Reference: `docs/prototypes/inbox/mail-buttons.html#d`.

/// The width a cell's word needs beside its icon: the room at both sides ([`space::s()`]), the icon
/// with words (14), the gap and the word in `level` – estimated like [`typo::ellipsize`], on the
/// wide side.
fn cell_need(label: &str, level: typo::Level) -> f32 {
    2.0 * space::s() + icon::SIZE_SM + ICON_GAP + typo::text_width(label, level)
}

/// The width of the menu part of a [`command_split`] (its chevron).
fn command_menu_width() -> f32 {
    size::control()
}

/// A bar's surface under its cells: `raised`, so the lines between the cells lie on the cells'
/// own surface.
fn raised_surface(theme: &Theme) -> container::Style {
    container::Style { background: Some(Background::Color(palette(theme).raised)), ..container::Style::default() }
}

/// Elements side by side with a vertical [`style::rule`] between each two, `height` high, on the
/// `raised` surface over the whole width.
fn cells_with_lines<'a, M: 'a>(cells: Vec<Element<'a, M>>, height: f32) -> Element<'a, M> {
    let mut bar = iced::widget::Row::new().height(height).width(Fill);
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            bar = bar.push(rule::vertical(LINE).style(style::rule));
        }
        bar = bar.push(cell);
    }
    container(bar).width(Fill).style(raised_surface).into()
}

/// The content of a cell: `mark` and, with `words`, `label` in `level` after it, centred in the
/// cell and clipped rather than pushed out of it.
fn cell_content<'a, M: 'a>(
    mark: Element<'a, M>,
    label: iced::widget::text::Fragment<'a>,
    level: typo::Level,
    words: bool,
) -> Element<'a, M> {
    let content: Element<'a, M> = if words {
        row![mark, typo::text(level, label).wrapping(Wrapping::None)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center)
            .into()
    } else {
        mark
    };
    center(content).clip(true).into()
}

/// The command bar at the top of the mail reader (task E11.1): `cells` side by side over the whole
/// width, [`size::command()`] high, a vertical `line` between each two and a `line` below the bar.
/// Fill it with [`command_split`] (the lead, Reply), [`command_cell`]s (Forward, Archive, Delete,
/// Later) and a [`command_more`] at the end: the cells share the width equally, "…" keeps its own.
/// Whether the cells show their words, [`command_words`] decides – for all of them alike.
///
/// To hang a menu at a cell (Later, "…"), wrap that cell in [`anchored`].
///
/// ```ignore
/// let labels = [tr("Reply"), tr("Forward"), tr("Archive"), tr("Delete")];
/// let words = widgets::command_words(&labels, width);
/// widgets::command_bar(vec![
///     widgets::command_split(Icon::Reply, labels[0], words, Some(Message::Reply), Some(Message::AskReplyMenu(id.clone()))),
///     widgets::command_cell(Icon::Forward, labels[1], words, false, Some(Message::Forward)),
///     widgets::command_cell(Icon::Archive, labels[2], words, false, Some(Message::Archive)),
///     widgets::command_cell(Icon::Trash2, labels[3], words, false, (!in_trash).then_some(Message::Delete)),
///     widgets::anchored(&anchors.more, widgets::command_more(Some(Message::AskMore(id)))),
/// ])
/// ```
pub fn command_bar<'a, M: 'a>(cells: Vec<Element<'a, M>>) -> Element<'a, M> {
    column![cells_with_lines(cells, size::command()), rule::horizontal(LINE).style(style::rule)].width(Fill).into()
}

/// One cell of a [`command_bar`]: a flat button (`style::button::cell`) as wide as its share and as
/// high as the bar, the icon (14) and `label` in `typo::body()` – with `strong` in
/// `typo::body_strong()` – centred in it. Without `words` the icon alone (16), with `label` as its
/// tooltip. `None` for `on_press` shows it off (40 %), as Delete in the trash.
pub fn command_cell<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    words: bool,
    strong: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let level = if strong { typo::body_strong() } else { typo::body() };
    cell_button(which, label.into_fragment(), level, words, on_press, Fill)
}

/// A cell of `width` with `which` and `label` as [`command_cell`] draws it; without `words` the
/// label is the tooltip.
fn cell_button<'a, M: Clone + 'a>(
    which: Icon,
    label: iced::widget::text::Fragment<'a>,
    level: typo::Level,
    words: bool,
    on_press: Option<M>,
    width: impl Into<Length>,
) -> Element<'a, M> {
    let edge = if words { icon::SIZE_SM } else { icon::SIZE };
    let mark = button_icon(which, edge, style::button::cell, on_press.is_some());
    let cell = button(cell_content(mark, label.clone(), level, words))
        .width(width)
        .height(Fill)
        .padding([0.0, space::s()])
        .style(style::button::cell)
        .on_press_maybe(on_press);
    if words { cell.into() } else { with_tooltip(cell, label) }
}

/// The lead cell of a [`command_bar`] with a menu of its own (the reader's Reply, whose menu holds
/// "Reply all"): like a [`command_cell`], always in `typo::body_strong()` – it is the bar's main
/// action, drawn strong instead of red –, and at its right, behind a short line, a narrow part with
/// [`Icon::ChevronDown`] (tooltip "More replies") that sends `on_menu`. Both parts are as high as
/// the bar; together they take one cell's share. To hang the menu at it, wrap it in [`anchored`].
pub fn command_split<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    words: bool,
    on_press: Option<M>,
    on_menu: Option<M>,
) -> Element<'a, M> {
    let main = cell_button(which, label.into_fragment(), typo::body_strong(), words, on_press, Fill);
    let line = container(rule::vertical(LINE).style(style::rule)).height(icon::SIZE);
    let chevron = button_icon(Icon::ChevronDown, icon::SIZE_SM, style::button::cell, on_menu.is_some());
    let menu = button(center(chevron))
        .width(command_menu_width())
        .height(Fill)
        .padding(0)
        .style(style::button::cell)
        .on_press_maybe(on_menu);
    let menu = with_tooltip(menu, tr("More replies"));
    row![main, line, menu].width(Fill).height(Fill).align_y(Alignment::Center).into()
}

/// The last cell of a [`command_bar`]: [`Icon::Ellipsis`] (16) in a cell as wide as the bar is high
/// ([`size::command()`]), tooltip "More". It opens the menu of what the bar leaves out (Flag, Mark
/// read, Done, Move to, Close); wrap it in [`anchored`] for that menu.
pub fn command_more<'a, M: Clone + 'a>(on_press: Option<M>) -> Element<'a, M> {
    let mark = button_icon(Icon::Ellipsis, icon::SIZE, style::button::cell, on_press.is_some());
    let more = button(center(mark))
        .width(size::command())
        .height(Fill)
        .padding(0)
        .style(style::button::cell)
        .on_press_maybe(on_press);
    with_tooltip(more, tr("More"))
}

/// Whether the cells of a [`command_bar`] `room` wide show their words: only when every cell holds
/// its icon and word in its equal share – all words or none. `labels` are the words of the bar's
/// equal cells from the left; the first is reckoned as the bar's [`command_split`] (strong, with
/// its chevron), and the bar as closed by a [`command_more`], as the mail reader builds it. A bar
/// without split or "…" gets its words a little later than it could, never cut.
///
/// ```ignore
/// let words = widgets::command_words(&[tr("Reply"), tr("Forward"), tr("Archive"), tr("Delete")], width);
/// ```
pub fn command_words(labels: &[&str], room: f32) -> bool {
    let Some((lead, rest)) = labels.split_first() else {
        return true;
    };
    let split = cell_need(lead, typo::body_strong()) + LINE + command_menu_width();
    let share = rest.iter().map(|label| cell_need(label, typo::body())).fold(split, f32::max);
    // The cells, a line after each (the last one before "…") and "…" itself.
    let cells = labels.len() as f32;
    cells * (share + LINE) + size::command() <= room
}

/// The strip of actions under the chosen row of the mail window's list (task E11.1): the `actions`
/// as flat cells (`style::button::cell`) side by side, equally wide over the row's whole width,
/// [`size::control_sm()`] high, a `line` above the strip and a vertical `line` between each two
/// cells. Each cell shows the icon and its word in `typo::body()` – or, without `words`, the icon
/// alone (16); [`row_strip_words`] chooses. An action marked [`RowAction::accent`] (Task) has its
/// icon in the accent colour. Every cell's tooltip names the action with its key ([`kbd`]), as in
/// [`row_actions_strip`]; an action that is off ([`RowAction::blocked`]) stands faded and says why.
/// The strip is [`row_strip_height`] high; empty without actions. It stands where the row's preview
/// line stands, so the chosen row is as high as any other (issue #227). The hub keeps
/// [`row_actions_strip`].
///
/// ```ignore
/// let words = widgets::row_strip_words(&actions, width);
/// column![row_lines, widgets::row_strip(actions, words)]
/// ```
pub fn row_strip<'a, M: Clone + 'a>(actions: Vec<RowAction<'a, M>>, words: bool) -> Element<'a, M> {
    if actions.is_empty() {
        return column![].into();
    }
    let cells = actions.into_iter().map(|action| strip_cell(action, words)).collect();
    column![rule::horizontal(LINE).style(style::rule), cells_with_lines(cells, size::control_sm())].width(Fill).into()
}

/// The height of a [`row_strip`]: its cells ([`size::control_sm()`]) and the line above them.
pub fn row_strip_height() -> f32 {
    size::control_sm() + LINE
}

/// Whether the cells of the [`row_strip`] of `actions` show their words over a row `width` wide:
/// only when every cell holds its icon and word in its equal share – all words or none.
pub fn row_strip_words<M>(actions: &[RowAction<'_, M>], width: f32) -> bool {
    let share = actions.iter().map(|action| cell_need(action.label.as_ref(), typo::body())).fold(0.0, f32::max);
    let lines = LINE * actions.len().saturating_sub(1) as f32;
    actions.len() as f32 * share + lines <= width
}

/// One cell of [`row_strip`].
fn strip_cell<'a, M: Clone + 'a>(action: RowAction<'a, M>, words: bool) -> Element<'a, M> {
    let press = action.press();
    let on = press.is_some();
    let tooltip = action_tip(&action);
    let accent = action.accent;
    let mark: Element<'a, M> = icon(action.icon, if words { icon::SIZE_SM } else { icon::SIZE })
        .style(move |t, _| {
            let p = palette(t);
            svg::Style { color: Some(if accent { p.accent } else { p.text }) }
        })
        .opacity(icon_opacity(on))
        .into();
    let cell = button(cell_content(mark, action.label, typo::body(), words))
        .width(Fill)
        .height(Fill)
        .padding([0.0, space::s()])
        .style(style::button::cell)
        .on_press_maybe(press);
    let cell = tip(cell, tooltip, tooltip::Position::Bottom);
    anchored_if(action.anchor.as_ref(), cell)
}

/// A Torro pill in the mail reader (task E11.1: Task, AI task, Event, AI event, Mute…):
/// [`size::pill()`] high, fully round with a frame in the accent (`style::button::pill`), the icon
/// (16) in the accent and `label` in `typo::body()` in `text`, [`space::l()`] in at the sides. The
/// red marks what Torro adds to a mail – an exception like the AI's sparkles; surface and word stay
/// neutral. `None` for `on_press` shows it off, the icon faded with frame and word; say why with
/// [`with_tooltip`] (an AI pill without an AI). Lay several out in a wrapping row, [`space::s()`]
/// apart.
///
/// ```ignore
/// widgets::pill_button(Icon::ListTodo, tr("Task"), Some(Message::Act(id.clone(), Command::Task)))
/// ```
pub fn pill_button<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mark = icon(which, icon::SIZE)
        .style(|t, _| svg::Style { color: Some(palette(t).accent) })
        .opacity(icon_opacity(on_press.is_some()));
    let content = row![mark, typo::text(typo::body(), label).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    button(container(content).center_y(Fill))
        .height(size::pill())
        .padding([0.0, space::l()])
        .style(style::button::pill)
        .on_press_maybe(on_press)
        .into()
}

/// A switch of how something is shown (task E11.1, the mail reader's foot: HTML | Text, Light |
/// Dark): a sunken track (`style::container::track`) [`size::control()`] high with one square
/// segment per `(icon, label, message)` – the icon (14) alone, `label` as its tooltip. The segment
/// at `active` is chosen (`style::button::segment(true)`: `selected` with a `line_selected` edge).
///
/// ```ignore
/// widgets::view_switch([(Icon::Code, tr("HTML"), Message::Html), (Icon::AlignLeft, tr("Text"), Message::Text)], 0)
/// ```
pub fn view_switch<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (Icon, L, M)>,
    active: usize,
) -> Element<'a, M> {
    let inner = size::control() - 2.0 * style::SEGMENT_INSET - 2.0 * LINE;
    let segments = options.into_iter().enumerate().map(|(i, (which, label, message))| {
        let style = style::button::segment(i == active);
        let mark: Element<'a, M> = icon(which, icon::SIZE_SM)
            .style(move |t, _| svg::Style { color: Some(style(t, button::Status::Active).text_color) })
            .into();
        let square = button(center(mark)).width(inner).height(inner).padding(0).style(style).on_press(message);
        with_tooltip(square, label)
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::track).into()
}

/// An attachment as a chip (task E11.1, the line of attachments above the mail reader's foot):
/// [`size::control()`] high, radius [`radius::sm()`], `raised` with a `line_strong` frame
/// (`style::button::secondary`, hover and pressed with the `line_selected` frame), the paperclip
/// (14) in `text_3`, `name` in `typo::body()` and `size_text` ("3 KB", may be empty) in
/// `typo::meta()`/`text_3`. The whole chip is the button that opens the attachment; `None` shows it
/// off. Lay several out with a wrapping row, [`space::s()`] apart.
///
/// ```ignore
/// widgets::file_chip(&attachment.name, format_size(attachment.size), Some(Message::OpenAttachment(i)))
/// ```
pub fn file_chip<'a, M: Clone + 'a>(
    name: impl IntoFragment<'a>,
    size_text: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![toned(Icon::Paperclip, icon::SIZE_SM, Tone::Text3), typo::text(typo::body(), name)]
        .spacing(ICON_GAP)
        .align_y(Alignment::Center);
    let size_text = size_text.into_fragment();
    if !size_text.is_empty() {
        content = content.push(typo::text(typo::meta(), size_text).style(theme::text_3).wrapping(Wrapping::None));
    }
    button(container(content).center_y(Fill))
        .height(size::control())
        .padding([0.0, space::m()])
        .style(style::button::secondary)
        .on_press_maybe(on_press)
        .into()
}

// ---------------------------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------------------------

/// A sidebar row (navigation, a list, a calendar, "Settings"): 28 high, `mark` (icon 16, ring or
/// dot), label `BODY` in `text_2`, `trail` at the end (count, key hint, ↗, status dot). The
/// `active` row is `selected` with `text` – neutral, never red.
///
/// ```ignore
/// widgets::sidebar_item(Icon::Sun, tr("Today"), Trail::Count(5), view == View::Today, Message::Show(View::Today))
/// widgets::sidebar_item(Mark::Ring(colour), list.name.as_str(), Trail::Count(list.open), false, Message::List(id))
/// ```
pub fn sidebar_item<'a, M: Clone + 'a>(
    mark: impl Into<Mark>,
    label: impl IntoFragment<'a>,
    trail: Trail,
    active: bool,
    on_press: M,
) -> Element<'a, M> {
    sidebar_item_in(SidebarMode::Expanded, mark, label, trail, active, on_press)
}

/// A sidebar row ([`sidebar_item`]) drawn for `mode`: expanded as there; in the rail
/// ([`SidebarMode::Rail`]) only `mark` centred on the 28-high row, with `label` and `trail` as
/// tooltip at its right. `active` and `on_press` work the same in both, so an app builds its rows
/// once with the mode as a parameter ([`crate::shell`], "Collapsed sidebar").
///
/// ```ignore
/// widgets::sidebar_item_in(mode, Icon::Sun, tr("Today"), Trail::Count(5), view == View::Today, Message::Show(View::Today))
/// ```
pub fn sidebar_item_in<'a, M: Clone + 'a>(
    mode: SidebarMode,
    mark: impl Into<Mark>,
    label: impl IntoFragment<'a>,
    trail: Trail,
    active: bool,
    on_press: M,
) -> Element<'a, M> {
    let tone = if active { Tone::Text } else { Tone::Text2 };
    let mark = mark_view(mark.into(), Scale::Row, tone);
    let row_button = |content: Element<'a, M>, padding: Padding| {
        button(content)
            .width(Fill)
            .height(size::row_sidebar())
            .padding(padding)
            .style(style::button::sidebar_item(active))
            .on_press(on_press)
    };
    match mode {
        SidebarMode::Expanded => {
            let content = row![mark, line_label(typo::body(), label)]
                .push(trail_view(trail))
                .spacing(space::s())
                .align_y(Alignment::Center);
            row_button(container(content).center_y(Fill).into(), style::ITEM_PADDING).into()
        }
        SidebarMode::Rail => {
            let content = container(mark).center_x(Fill).center_y(Fill).into();
            tooltip_right(row_button(content, Padding::ZERO), label, trail)
        }
    }
}

/// The title of a group in the sidebar ("Lists", "Calendars"): `CAPTION` in `text_3`, 28 high,
/// 12 above it. With `toggle = Some((expanded, message))` a chevron follows the title and a click
/// sends `message` (the app shows or hides the rows).
pub fn sidebar_group<'a, M: Clone + 'a>(label: impl IntoFragment<'a>, toggle: Option<(bool, M)>) -> Element<'a, M> {
    sidebar_group_in(SidebarMode::Expanded, label, toggle)
}

/// A sidebar group title ([`sidebar_group`]) drawn for `mode`: expanded as there; in the rail a
/// 1-px line in `line` with 8 above and below it instead of the title (the rail has no room for
/// words), and no chevron – the app decides whether the group's rows are shown, as before.
pub fn sidebar_group_in<'a, M: Clone + 'a>(
    mode: SidebarMode,
    label: impl IntoFragment<'a>,
    toggle: Option<(bool, M)>,
) -> Element<'a, M> {
    if mode == SidebarMode::Rail {
        return container(rule::horizontal(LINE).style(style::rule)).padding([space::s(), space::xs()]).into();
    }
    let mut title =
        row![typo::text(typo::caption(), label).style(theme::text_3)].spacing(space::xs()).align_y(Alignment::Center);
    if let Some((expanded, _)) = &toggle {
        let chevron = if *expanded { Icon::ChevronDown } else { Icon::ChevronRight };
        title = title.push(toned(chevron, GROUP_CHEVRON, Tone::Text3));
    }
    let bar = container(title).height(size::row_sidebar()).padding(style::ITEM_PADDING).center_y(size::row_sidebar());
    let bar: Element<'a, M> = match toggle {
        Some((_, message)) => mouse_area(bar).on_press(message).interaction(mouse::Interaction::Pointer).into(),
        None => bar.into(),
    };
    container(bar).padding(Padding::new(0.0).top(space::m())).into()
}

// ---------------------------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------------------------

/// A list row (a task, an agenda entry): 40 high, `cells` side by side with 10 between them,
/// 16 at the sides ([`layout::row_padding`]), a line below. Give the title cell `.width(Fill)` so the cells after it sit at
/// the right. The `selected` row (the one in the detail) is `selected` with a 2-px bar on the left.
///
/// ```ignore
/// widgets::list_row(vec![check.into(), priority, typo::text(typo::body(), &task.title).width(Fill).into(), due], task.id == selected, Some(Message::Select(task.id)))
/// ```
pub fn list_row<'a, M: Clone + 'a>(cells: Vec<Element<'a, M>>, selected: bool, on_press: Option<M>) -> Element<'a, M> {
    let content = row(cells).spacing(ROW_GAP).align_y(Alignment::Center);
    let pad = layout::row_padding();
    let left = if selected { pad.left - SELECTED_BAR } else { pad.left };
    let body = button(container(content).center_y(Fill))
        .width(Fill)
        // 40 including the line below, as the CSS's `border-box`.
        .height(size::row() - LINE)
        .padding(Padding { left, ..pad })
        .style(style::button::list_row(selected))
        .on_press_maybe(on_press);
    let line: Element<'a, M> = if selected {
        let bar = container(blank()).width(SELECTED_BAR).height(size::row() - LINE).style(|t| container::Style {
            background: Some(Background::Color(palette(t).text_3)),
            ..container::Style::default()
        });
        row![bar, body].into()
    } else {
        body.into()
    };
    column![line, rule::horizontal(LINE).style(style::rule)].into()
}

/// The head of a group in a list ("Overdue 2", "Today 3"): 32 high on `raised` with lines above
/// and below and 4 above that; `mark` (icon 14), label `CAPTION` in `text_2`, count tabular in
/// `text_3`, and with `on_add` a "+" at the right (tinted like a [`Mark::Tinted`] mark).
pub fn group_head<'a, M: Clone + 'a>(
    mark: impl Into<Mark>,
    label: impl IntoFragment<'a>,
    count: Option<usize>,
    on_add: Option<M>,
) -> Element<'a, M> {
    let mark = mark.into();
    let mut content =
        row![mark_view(mark, Scale::Small, Tone::Text2), typo::text(typo::caption(), label).style(theme::text_2)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    if let Some(n) = count {
        content = content.push(typo::text(typo::meta().tabular(), n.to_string()).style(theme::text_3));
    }
    content = content.push(blank::horizontal());
    if let Some(message) = on_add {
        let plus = match mark {
            Mark::Tinted(_, colour) => tinted(Icon::Plus, icon::SIZE_SM, colour),
            _ => toned(Icon::Plus, icon::SIZE_SM, Tone::Text3),
        };
        let add = button(center(plus))
            .width(size::control_sm())
            .height(size::control_sm())
            .padding(0)
            .style(style::button::icon)
            .on_press(message);
        content = content.push(add);
    }
    // The "+" button is wider than its icon; pull it in so the icon sits as far from the edge as
    // the content of a row (16).
    let pad = layout::row_padding();
    let right = pad.right - (size::control_sm() - icon::SIZE_SM) / 2.0;
    let head = container(content)
        .width(Fill)
        .center_y(size::row_group() - 2.0 * LINE)
        .padding(Padding::new(0.0).left(pad.left).right(right))
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).raised)),
            ..container::Style::default()
        });
    column![
        blank().height(space::xs()),
        rule::horizontal(LINE).style(style::rule),
        head,
        rule::horizontal(LINE).style(style::rule)
    ]
    .into()
}

// ---------------------------------------------------------------------------------------------
// Row actions
// ---------------------------------------------------------------------------------------------

/// One button of a row's action bar ([`row_actions`]): icon and word, the key that does the same in
/// the list (shown in the tooltip) and the message it sends.
///
/// ```ignore
/// widgets::RowAction::new(Icon::Check, tr("Done"), "E", Message::Act(id.clone(), Command::Done))
/// ```
#[derive(Debug, Clone)]
pub struct RowAction<'a, M> {
    pub icon: Icon,
    /// The word on the button ("Task", "Done"), sentence case through `tr`; the tooltip when the
    /// bar shows icons only.
    pub label: iced::widget::text::Fragment<'a>,
    /// The key that does the same in the list ("E"), shown as [`kbd`] in the tooltip.
    pub key: &'static str,
    /// Sent when the button is pressed. The app moves its cursor to the row and acts as the key
    /// would.
    pub on_press: M,
    /// Registered by the button when set ([`RowAction::at`]): the menu it opens hangs at the button
    /// itself (issue #122), not at the whole row. Only the row under the cursor sets it, so one
    /// anchor is never registered by two rows.
    pub anchor: Option<Anchor>,
    /// Why the button is off, if it is (issue #168): "Set up an AI first". Set with
    /// [`RowAction::blocked`]. An action that is off still stands in its bar, at the disabled
    /// opacity, takes no press and says in its tooltip why ([`RowAction::tip`]). Whoever draws an
    /// action asks [`RowAction::press`] for the message, never `on_press` directly.
    pub blocked: Option<iced::widget::text::Fragment<'a>>,
    /// The icon in the accent colour (task E11.1: Task in the mail window's strip, what Torro adds
    /// to a mail), set with [`RowAction::accent`]. Only [`row_strip`] draws it; the floating bar and
    /// [`row_actions_strip`] of the hub keep their icons in `text`.
    pub accent: bool,
}

impl<'a, M> RowAction<'a, M> {
    /// A button with `icon`, `label` and the key hint `key` that sends `on_press`; no anchor.
    pub fn new(icon: Icon, label: impl IntoFragment<'a>, key: &'static str, on_press: M) -> Self {
        Self { icon, label: label.into_fragment(), key, on_press, anchor: None, blocked: None, accent: false }
    }

    /// The same button with its icon in the accent colour ([`RowAction::accent`]).
    ///
    /// ```ignore
    /// RowAction::new(Icon::ListTodo, tr("Task"), "T", Message::Act(id.clone(), Command::Task)).accent()
    /// ```
    pub fn accent(self) -> Self {
        Self { accent: true, ..self }
    }

    /// The same button, off for `reason` ([`RowAction::blocked`]): "AI task" without an AI.
    ///
    /// ```ignore
    /// let ai = RowAction::new(Icon::Sparkles, tr("AI task"), "", Message::SuggestTask(id.clone()));
    /// if !ready { ai.blocked(tr("Set up an AI first")) } else { ai }
    /// ```
    pub fn blocked(self, reason: impl IntoFragment<'a>) -> Self {
        Self { blocked: Some(reason.into_fragment()), ..self }
    }

    /// What a press sends: `on_press`, `None` while the button is off ([`RowAction::blocked`]).
    pub fn press(&self) -> Option<M>
    where
        M: Clone,
    {
        self.blocked.is_none().then(|| self.on_press.clone())
    }

    /// The words of the button's tooltip: its label, and while it is off why – "AI task · Set up an
    /// AI first".
    pub fn tip(&self) -> String {
        match &self.blocked {
            Some(reason) => format!("{} · {}", self.label, reason),
            None => self.label.to_string(),
        }
    }

    /// The same button registering `anchor`, for the menu it opens.
    pub fn at(self, anchor: &Anchor) -> Self {
        Self { anchor: Some(anchor.clone()), ..self }
    }
}

/// The frame of the action bar around its buttons, and the gap between them: 2, as inside the
/// segmented control (the skill allows 2 inside building blocks).
const BAR_INSET: f32 = style::SEGMENT_INSET;

/// The width of one button of the bar with its word: the button padding, the icon (14), the gap and
/// the word in `CAPTION` (estimated like [`typo::ellipsize`]).
fn row_action_width(label: &str) -> f32 {
    text_button_width(true, label)
}

/// The width a [`text_button`] with `label` takes, with its icon (`icon`) or without: the button
/// padding, the icon (14) and its gap, the word in `CAPTION` – estimated like [`typo::ellipsize`],
/// on the wide side. For choosing between words and icons before laying out.
pub fn text_button_width(icon: bool, label: &str) -> f32 {
    let pad = style::button_padding();
    let mark = if icon { icon::SIZE_SM + ICON_GAP } else { 0.0 };
    pad.left + pad.right + mark + typo::text_width(label, typo::caption())
}

/// How a row of buttons on a card shows its words (task E8.12, finding 4): all or none – a bar never
/// mixes words and icons, except that its lead (the action the entry is about: "Open", "Go to
/// window", "Reply") may keep its word while the others are icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarWords {
    /// Every button with its icon and word.
    All,
    /// The first with its word, the rest as icons with their word as tooltip.
    Lead,
    /// Icons alone, each with its word as tooltip.
    None,
}

impl BarWords {
    /// Whether the button at `index` (from the left) shows its word.
    pub fn worded(self, index: usize) -> bool {
        match self {
            BarWords::All => true,
            BarWords::Lead => index == 0,
            BarWords::None => false,
        }
    }
}

/// The width a bar of buttons with `labels` (icon and word each, from the left) takes as `words`
/// says, followed by `icons` more icon-only buttons (a bell, "…"), [`space::s()`] between all of
/// them: each worded button [`text_button_width`], each icon [`size::control_sm()`].
pub fn bar_width(labels: &[&str], words: BarWords, icons: usize) -> f32 {
    let buttons: f32 = labels
        .iter()
        .enumerate()
        .map(|(i, label)| if words.worded(i) { text_button_width(true, label) } else { size::control_sm() })
        .sum();
    let count = labels.len() + icons;
    buttons + icons as f32 * size::control_sm() + space::s() * count.saturating_sub(1) as f32
}

/// How the bar of `labels` (plus `icons` icon-only buttons, see [`bar_width`]) shows its words in
/// `room`: [`BarWords::All`] when they fit; else [`BarWords::Lead`] when the first is the bar's
/// `lead` action and fits with its word; else [`BarWords::None`].
///
/// ```ignore
/// let words = widgets::bar_words(&["Open", "Task", "Later"], true, 1, inner);
/// for (i, button) in buttons.iter().enumerate() { /* text_button if words.worded(i), else icon_button */ }
/// ```
pub fn bar_words(labels: &[&str], lead: bool, icons: usize, room: f32) -> BarWords {
    if bar_width(labels, BarWords::All, icons) <= room {
        BarWords::All
    } else if lead && labels.len() > 1 && bar_width(labels, BarWords::Lead, icons) <= room {
        BarWords::Lead
    } else {
        BarWords::None
    }
}

/// The parts of a meta line ("Mail · Jonas Weber · anna@work.example.org · Tue, 13 Oct") that fit
/// `room` in `level`, joined by " · " (task E8.12, finding 1): all of them when they fit, else
/// without the parts at the indices of `drop`, left out one after the other in that order (the
/// account first, then the sender) until the line fits. What still does not fit is for the caller
/// to cut with "…" ([`typo::ellipsize`]). Empty parts are skipped.
pub fn meta_fit(parts: &[String], drop: &[usize], level: typo::Level, room: f32) -> String {
    let join = |left_out: &[usize]| {
        let kept: Vec<&str> = parts
            .iter()
            .enumerate()
            .filter(|(i, part)| !part.is_empty() && !left_out.contains(i))
            .map(|(_, part)| part.as_str())
            .collect();
        kept.join(" · ")
    };
    let mut line = join(&[]);
    for n in 1..=drop.len() {
        if typo::text_width(&line, level) <= room {
            break;
        }
        line = join(&drop[..n]);
    }
    line
}

/// The narrowest the words of a [`card_line`] get before its action gives up its word: half the
/// properties column, room for a short title.
fn card_line_words_min() -> f32 {
    size::props() / 2.0
}

/// The one line of a small card that points somewhere else (task E8.12, findings 1–3: "From" in a
/// task, the task in a mail): `lead` (an avatar or a mark, `lead_width` wide), the `title` in
/// `BODY_STRONG` over the `meta` line in `META`/`text_3`, and the `action` at the right. Laid out
/// at the width it gets:
///
/// - the action keeps its word ([`text_button`], `secondary`) while the words keep at least half
///   the properties column, else it is its icon with the word as tooltip – it never pushes the
///   title away;
/// - the meta line leaves out the parts at `drop` in that order where it does not fit
///   ([`meta_fit`]), and title and meta are cut with "…", showing the whole as tooltip – never cut
///   silently.
///
/// Put it into a [`card`] (`widgets::card(vec![card_line(…)], false, None)`).
pub fn card_line<'a, M: Clone + 'a>(
    lead: impl Fn() -> Element<'a, M> + 'a,
    lead_width: f32,
    title: String,
    meta: Vec<String>,
    drop: Vec<usize>,
    action: RowAction<'a, M>,
) -> Element<'a, M> {
    responsive(move |room| {
        let gap = space::m();
        let (spoken, words_room) = card_line_split(room.width, lead_width, action.label.as_ref());
        let button = if spoken {
            let button = text_button(Some(action.icon), action.label.clone(), style::button::secondary, action.press());
            if action.blocked.is_some() { with_tooltip(button, action.tip()) } else { button }
        } else {
            icon_button(action.icon, action.tip(), action.press())
        };
        let whole = meta.iter().filter(|part| !part.is_empty()).cloned().collect::<Vec<_>>().join(" · ");
        let fitted = meta_fit(&meta, &drop, typo::meta(), words_room);
        let words = column![
            cut_line(typo::body_strong(), &title, title.clone(), theme::text, words_room),
            cut_line(typo::meta(), &fitted, whole, theme::text_3, words_room),
        ]
        .width(Fill);
        row![lead(), words, button].spacing(gap).align_y(Alignment::Center).into()
    })
    .height(Length::Shrink)
    .into()
}

/// How a [`card_line`] `width` wide with a lead `lead_width` wide splits its room: whether its
/// action keeps its word `label` (the words keep at least [`card_line_words_min`] beside it), and
/// the room left for title and meta – beside the button with its word, or beside its icon.
pub fn card_line_split(width: f32, lead_width: f32, label: &str) -> (bool, f32) {
    let words = width - lead_width - 2.0 * space::m();
    let worded = text_button_width(true, label);
    let spoken = words - worded >= card_line_words_min();
    (spoken, (words - if spoken { worded } else { size::control_sm() }).max(0.0))
}

/// `text` in `level` on one line cut to `room` with "…"; when it is cut or is not all of `whole`,
/// `whole` shows as tooltip.
fn cut_line<'a, M: 'a>(
    level: typo::Level,
    text: &str,
    whole: String,
    colour: fn(&Theme) -> iced::widget::text::Style,
    room: f32,
) -> Element<'a, M> {
    let shown = typo::ellipsize(text, level, room);
    let cut = shown != whole;
    let line = container(typo::text(level, shown).style(colour).wrapping(Wrapping::None)).clip(true);
    if cut { with_tooltip(line, whole) } else { line.into() }
}

/// The width the action bar of `actions` takes with `words` (icon and word on every button) or
/// without (28 per icon), frame included. Apps use [`row_actions_words`] to choose.
pub fn row_actions_width<M>(actions: &[RowAction<'_, M>], words: bool) -> f32 {
    let buttons: f32 = actions
        .iter()
        .map(|action| if words { row_action_width(action.label.as_ref()) } else { size::control_sm() })
        .sum();
    let gaps = BAR_INSET * actions.len().saturating_sub(1) as f32;
    buttons + gaps + 2.0 * (BAR_INSET + LINE)
}

/// Whether the bar of `actions` shows its words over a row `width` wide: only while it takes at most
/// half the row, so the start of the row – who and what – stays readable beside it. Narrower rows
/// (a narrow window, a column of the overview) get the icons alone, each with its word as tooltip.
pub fn row_actions_words<M>(actions: &[RowAction<'_, M>], width: f32) -> bool {
    row_actions_width(actions, true) <= width / 2.0
}

/// A row with its action bar (task E3.9: "Task", "Done", "Later", "Hide" on an inbox entry): the
/// bar floats at the row's right end, [`layout::row_padding`] in from the edge and centred in the
/// row's height, over the end of the row (its time). Surface `menu`, frame `line_strong`, radius
/// [`radius::sm()`], buttons [`size::control_sm()`] high with icon and word – or, without `words`,
/// 28 × 28 with the icon alone ([`row_actions_words`] chooses). Every button's tooltip names it
/// with its key ([`kbd`]).
///
/// The bar shows while the pointer is over the row, and always while `shown` – the row under the
/// keyboard cursor, so the keys and the mouse find the same actions. Without actions the row stays
/// as it is.
///
/// ```ignore
/// let actions = vec![RowAction::new(Icon::Check, tr("Done"), "E", Message::Act(id.clone(), Command::Done))];
/// let words = widgets::row_actions_words(&actions, width);
/// widgets::row_actions(widgets::list_row(cells, selected, Some(on_press)), actions, selected, words)
/// ```
pub fn row_actions<'a, M: Clone + 'a>(
    row: impl Into<Element<'a, M>>,
    actions: Vec<RowAction<'a, M>>,
    shown: bool,
    words: bool,
) -> Element<'a, M> {
    with_actions(row.into(), actions, shown, words, Place::Middle)
}

/// [`row_actions`] for a row of several lines (task E3.11: the inbox's mail and notice rows): the
/// bar is centred on the line whose middle lies `line_middle` below the row's top – the first line,
/// where the time stands –, so it covers neither the preview nor the account further down. Shown
/// and sized as in [`row_actions`]; [`row_actions_height`] is the bar's height.
///
/// ```ignore
/// // A row 87 high whose three lines of 20 are centred: the first line's middle is 13.5 + 10 down.
/// let middle = (87.0 - 3.0 * 20.0) / 2.0 + 20.0 / 2.0;
/// widgets::row_actions_at(row, actions, selected, words, middle)
/// ```
pub fn row_actions_at<'a, M: Clone + 'a>(
    row: impl Into<Element<'a, M>>,
    actions: Vec<RowAction<'a, M>>,
    shown: bool,
    words: bool,
    line_middle: f32,
) -> Element<'a, M> {
    with_actions(row.into(), actions, shown, words, Place::Line(line_middle))
}

/// The height of the action bar of [`row_actions`]: a button ([`size::control_sm()`]) in its frame.
pub fn row_actions_height() -> f32 {
    size::control_sm() + 2.0 * (BAR_INSET + LINE)
}

/// A card with the action bar of [`row_actions`], for a card whose last line is a row of buttons
/// [`size::control_sm()`] high, `inset` in from the card's bottom and right edge (its padding and
/// frame): the bar sits at the right end of that row, centred on it, beside the card's own buttons
/// instead of over its text. Shown and sized as in [`row_actions`].
pub fn card_actions<'a, M: Clone + 'a>(
    card: impl Into<Element<'a, M>>,
    actions: Vec<RowAction<'a, M>>,
    shown: bool,
    words: bool,
    inset: f32,
) -> Element<'a, M> {
    with_actions(card.into(), actions, shown, words, Place::Card(inset))
}

/// The action bar of a row as a strip of its own below the row's lines (the inbox's list): the
/// buttons of `actions` side by side, [`space::s()`] apart, each `secondary` ([`size::control_sm()`]
/// high) so they stand out on the row's `selected` surface, with icon and word – or, without
/// `words`, 28 × 28 with the icon alone ([`row_actions_strip_words`] chooses). Every button's
/// tooltip names it with its key ([`kbd`]). Unlike [`row_actions`] nothing floats over the row: the
/// row grows by [`row_actions_strip_height`] while it shows the strip. Empty without actions.
///
/// ```ignore
/// let words = widgets::row_actions_strip_words(&actions, room);
/// column![lines, widgets::row_actions_strip(actions, words)].spacing(space::s())
/// ```
pub fn row_actions_strip<'a, M: Clone + 'a>(actions: Vec<RowAction<'a, M>>, words: bool) -> Element<'a, M> {
    let buttons = actions.into_iter().map(|action| row_action_button(action, words, style::button::secondary));
    iced::widget::Row::with_children(buttons).spacing(space::s()).align_y(Alignment::Center).into()
}

/// The height a [`row_actions_strip`] adds to its row: a button ([`size::control_sm()`]) and the gap
/// [`space::s()`] above it.
pub fn row_actions_strip_height() -> f32 {
    size::control_sm() + space::s()
}

/// Whether the [`row_actions_strip`] of `actions` shows its words in `room`: when all of them fit,
/// else icons alone with their word as tooltip – never a mix.
pub fn row_actions_strip_words<M>(actions: &[RowAction<'_, M>], room: f32) -> bool {
    let buttons: f32 = actions.iter().map(|action| row_action_width(action.label.as_ref())).sum();
    buttons + space::s() * actions.len().saturating_sub(1) as f32 <= room
}

/// Where the bar of [`with_actions`] sits at the right of what it lies over.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// Centred in the height ([`row_actions`]).
    Middle,
    /// Centred on a line whose middle lies this far below the top ([`row_actions_at`]).
    Line(f32),
    /// At the bottom, centred on a button row this far in from the edges ([`card_actions`]).
    Card(f32),
}

/// `base` with the bar of `actions` over it, at `place`.
fn with_actions<'a, M: Clone + 'a>(
    base: Element<'a, M>,
    actions: Vec<RowAction<'a, M>>,
    shown: bool,
    words: bool,
    place: Place,
) -> Element<'a, M> {
    if actions.is_empty() {
        return base;
    }
    let buttons = actions.into_iter().map(|action| row_action_button(action, words, style::button::menu_item(false)));
    let bar = container(iced::widget::Row::with_children(buttons).spacing(BAR_INSET).align_y(Alignment::Center))
        .padding(BAR_INSET)
        .style(|t| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.menu)),
                border: border::rounded(radius::sm()).color(p.line_strong).width(LINE),
                shadow: p.shadow,
                ..container::Style::default()
            }
        });
    let layer = container(bar).width(Fill).height(Fill).align_right(Fill);
    let right = layout::row_padding().right;
    let layer = match place {
        Place::Middle => layer.center_y(Fill).padding(Padding::new(0.0).right(right)),
        Place::Line(middle) => {
            let top = (middle - row_actions_height() / 2.0).max(0.0);
            layer.align_top(Fill).padding(Padding::new(0.0).right(right).top(top))
        }
        // The bar is its frame taller than a button: half of that hangs below the button row.
        Place::Card(inset) => {
            layer.align_bottom(Fill).padding(Padding::new(0.0).right(inset).bottom(inset - BAR_INSET - LINE))
        }
    };
    if shown { stack![base, layer].into() } else { iced::widget::hover(base, layer) }
}

/// The tooltip of an action: its words ([`RowAction::tip`]) and its key as [`kbd`], if it has one.
fn action_tip<'a, M: 'a>(action: &RowAction<'a, M>) -> Element<'a, M> {
    let mut tooltip = row![typo::text(typo::caption(), action.tip()).style(theme::text_2)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if !action.key.is_empty() {
        tooltip = tooltip.push(kbd(action.key));
    }
    tooltip.into()
}

/// One button of [`row_actions`] in `style`: on the `menu` surface `menu_item`, so hover and press
/// are `selected` (as in menus, where `hover` would vanish); in [`row_actions_strip`] `secondary`.
/// The tooltip names it with its key.
///
/// An action that is off ([`RowAction::blocked`]) stands there faded – word, icon and all – takes no
/// press and says why in its tooltip. An action without a key (`""`) shows no key hint.
fn row_action_button<'a, M: Clone + 'a>(
    action: RowAction<'a, M>,
    words: bool,
    style: impl Fn(&Theme, button::Status) -> button::Style + Copy + 'a,
) -> Element<'a, M> {
    let press = action.press();
    let on = press.is_some();
    let tooltip = action_tip(&action);
    // `text`, the colour of a menu item's label; faded with the button when it is off.
    let mark = |edge| -> Element<'a, M> {
        icon(action.icon, edge)
            .style(|t, _| svg::Style { color: Some(Tone::Text.of(&palette(t))) })
            .opacity(icon_opacity(on))
            .into()
    };
    let button = if words {
        let content = row![mark(icon::SIZE_SM), typo::text(typo::caption(), action.label).wrapping(Wrapping::None)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center);
        button(container(content).center_y(Fill)).height(size::control_sm()).padding(style::button_padding())
    } else {
        button(center(mark(icon::SIZE))).width(size::control_sm()).height(size::control_sm()).padding(0)
    };
    let button = tip(button.style(style).on_press_maybe(press), tooltip, tooltip::Position::Bottom);
    anchored_if(action.anchor.as_ref(), button)
}

// ---------------------------------------------------------------------------------------------
// Menus and floating cards
// ---------------------------------------------------------------------------------------------

/// Where a trigger is on screen, shared between the trigger ([`anchored`], [`menu_button`]) and
/// the layer that floats next to it ([`floating`]).
///
/// Make a new one in every `view` (it is a cheap shared cell) and pass the same one to both. The
/// trigger writes its bounds while Iced lays out and draws the window; the layer reads them in
/// the same frame. `Clone` shares the cell.
#[derive(Debug, Clone, Default)]
pub struct Anchor {
    bounds: Rc<Cell<Option<Rectangle>>>,
    area: Option<Rectangle>,
    /// Where to float instead while this anchor's trigger was not laid out ([`Anchor::or`]).
    fallback: Option<Rc<Anchor>>,
}

impl Anchor {
    /// A new, still empty anchor.
    pub fn new() -> Self {
        Self::default()
    }

    /// The same anchor narrowed to `area`, given relative to the anchored widget's top-left corner.
    /// For triggers that are not widgets of their own: an event drawn on a canvas anchors the
    /// canvas with [`anchored`] and floats its card next to `anchor.area(event_rect)`.
    pub fn area(&self, area: Rectangle) -> Anchor {
        Anchor { bounds: Rc::clone(&self.bounds), area: Some(area), fallback: self.fallback.clone() }
    }

    /// This anchor, or `fallback` while this one's trigger is not on screen in the frame: a menu
    /// opened by a key hangs at the button that does the same, and at the row when that button is
    /// missing (issue #122). Chains: `a.or(&b).or(&c)` tries `a`, then `b`, then `c`.
    pub fn or(&self, fallback: &Anchor) -> Anchor {
        let fallback = match &self.fallback {
            Some(next) => next.or(fallback),
            None => fallback.clone(),
        };
        Anchor { fallback: Some(Rc::new(fallback)), ..self.clone() }
    }

    fn set(&self, bounds: Rectangle) {
        self.bounds.set(Some(bounds));
    }

    /// The anchored rectangle in window coordinates, once the trigger has been laid out; else the
    /// fallback's ([`Anchor::or`]).
    fn get(&self) -> Option<Rectangle> {
        let own = self.bounds.get().map(|bounds| match self.area {
            Some(area) => Rectangle { x: bounds.x + area.x, y: bounds.y + area.y, ..area },
            None => bounds,
        });
        own.or_else(|| self.fallback.as_ref().and_then(|fallback| fallback.get()))
    }
}

/// `content` as the trigger of a floating layer: it is laid out and drawn as usual and records its
/// on-screen bounds in `anchor`.
///
/// Uses Iced's `float` widget, whose translation hook sees the widget's bounds in window
/// coordinates (scrolling included); the hook only records them and never moves the content.
pub fn anchored<'a, M: 'a>(anchor: &Anchor, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    let anchor = anchor.clone();
    float(content)
        .translate(move |bounds, _viewport| {
            anchor.set(bounds);
            Vector::ZERO
        })
        .into()
}

/// `content`, registering `anchor` when there is one ([`anchored`]); as it is without.
pub fn anchored_if<'a, M: 'a>(anchor: Option<&Anchor>, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    match anchor {
        Some(anchor) => anchored(anchor, content),
        None => content.into(),
    }
}

/// Which edge of the anchor a card below it lines up with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Left edges line up (menus opened from the left of a header, the app switcher).
    Start,
    /// Right edges line up (menus opened from the right of a header: "Week ▾", "…").
    End,
}

/// Where a floating card goes relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// 4 below the anchor (menus); 4 above it when the window has no room below but room above
    /// (task E8.12: a menu never covers its trigger where it can help it, [`below_or_above`]).
    Below(Align),
    /// 8 right of the anchor, top edges in line; left of it when there is no room (event cards).
    Beside,
}

/// The top-left corner of a card of `card` size placed at `target` inside `viewport`; the card
/// keeps 8 from the viewport edges when it fits.
fn place(target: Rectangle, card: Size, viewport: Rectangle, placement: Placement) -> Point {
    let (x, y) = match placement {
        Placement::Below(Align::Start) => (target.x, below_or_above(target, card, viewport)),
        Placement::Below(Align::End) => (target.x + target.width - card.width, below_or_above(target, card, viewport)),
        Placement::Beside => {
            let right = target.x + target.width + beside_gap();
            let x = if right + card.width <= viewport.x + viewport.width - beside_gap() {
                right
            } else {
                target.x - beside_gap() - card.width
            };
            (x, target.y)
        }
    };
    let clamp = |value: f32, start: f32, length: f32, extent: f32| {
        let (low, high) = (start + beside_gap(), start + length - extent - beside_gap());
        if high < low { low } else { value.clamp(low, high) }
    };
    Point::new(clamp(x, viewport.x, viewport.width, card.width), clamp(y, viewport.y, viewport.height, card.height))
}

/// The top-left corner of a card of `card` size in the middle of `viewport`: where a card whose
/// anchor never showed goes (issue #122), rather than the window's top-left corner.
fn centre(card: Size, viewport: Rectangle) -> Point {
    Point::new(viewport.x + (viewport.width - card.width) / 2.0, viewport.y + (viewport.height - card.height) / 2.0)
}

/// The top of a card of `card` size that opens under `target` (task E8.12): [`below_gap()`] under
/// it while the card fits above the viewport's bottom margin, else as far above it when it fits
/// there; with room on neither side, on the side with more room – the clamp in [`place`] then keeps
/// it on screen.
fn below_or_above(target: Rectangle, card: Size, viewport: Rectangle) -> f32 {
    let below = target.y + target.height + below_gap();
    let above = target.y - below_gap() - card.height;
    let room_below = viewport.y + viewport.height - beside_gap() - below;
    let room_above = target.y - below_gap() - (viewport.y + beside_gap());
    if card.height <= room_below || (card.height > room_above && room_below >= room_above) { below } else { above }
}

/// The layer for an open menu or floating card: `card` ([`menu`], [`popover`]) next to `anchor`
/// as `placement` says, above everything; a click anywhere else sends `on_dismiss` and reaches
/// nothing below (like Linear). Pass it to [`crate::shell::Shell::overlay`]; close it on Escape
/// with [`escape`].
///
/// The card stays inside the window: it moves in from the edges and flips to the other side of
/// the anchor for [`Placement::Beside`]. While the anchor (and its fallbacks, [`Anchor::or`]) was
/// not laid out, the card stands in the middle of the window.
pub fn floating<'a, M: Clone + 'a>(
    anchor: &Anchor,
    placement: Placement,
    card: impl Into<Element<'a, M>>,
    on_dismiss: M,
) -> Element<'a, M> {
    let anchor = anchor.clone();
    let catcher = opaque(mouse_area(blank().width(Fill).height(Fill)).on_press(on_dismiss));
    // Laid out at the window's top-left corner and moved to its place as a float overlay, which
    // also draws it above the rest of the window and gives it the pointer first.
    let card = float(card).translate(move |bounds, viewport| match anchor.get() {
        Some(target) => place(target, bounds.size(), viewport, placement) - bounds.position(),
        None => centre(bounds.size(), viewport) - bounds.position(),
    });
    stack![catcher, card].into()
}

/// One entry of a [`menu`]: 30 high, `mark`, label `BODY` in `text`, `trail` at the end. The
/// `current` choice (the view shown, the app running) gets a small dot at the end.
#[derive(Debug, Clone)]
pub struct MenuItem<'a, M> {
    /// Icon, ring, dot or check box in front; `None` for text only.
    pub mark: Option<Mark>,
    /// The label (sentence case, through `tr`).
    pub label: iced::widget::text::Fragment<'a>,
    /// Count, key hint or ↗ at the end.
    pub trail: Trail,
    /// Marks the current choice.
    pub current: bool,
    /// Sent when the entry is clicked; the app closes the menu itself.
    pub on_press: M,
}

impl<'a, M> MenuItem<'a, M> {
    /// An entry with `mark` and `label` that sends `on_press`; no trail, not current.
    pub fn new(mark: impl Into<Mark>, label: impl IntoFragment<'a>, on_press: M) -> Self {
        Self { mark: Some(mark.into()), label: label.into_fragment(), trail: Trail::None, current: false, on_press }
    }

    /// The same entry with `trail` at the end.
    pub fn trail(self, trail: Trail) -> Self {
        Self { trail, ..self }
    }

    /// The same entry marked as the current choice when `current` is true.
    pub fn current(self, current: bool) -> Self {
        Self { current, ..self }
    }
}

/// A part of a [`menu`]: an optional `CAPTION` title and its entries. Sections are separated by
/// a line.
#[derive(Debug, Clone)]
pub struct MenuSection<'a, M> {
    /// "View", "Calendars", "Lists"; `None` for none.
    pub title: Option<iced::widget::text::Fragment<'a>>,
    /// The entries, top to bottom.
    pub items: Vec<MenuItem<'a, M>>,
}

impl<'a, M> MenuSection<'a, M> {
    /// A section without a title.
    pub fn new(items: Vec<MenuItem<'a, M>>) -> Self {
        Self { title: None, items }
    }

    /// A section with `title` above its entries.
    pub fn titled(title: impl IntoFragment<'a>, items: Vec<MenuItem<'a, M>>) -> Self {
        Self { title: Some(title.into_fragment()), items }
    }
}

/// One entry of a [`menu`] on its own, for a menu card that holds more than entries – the bell's
/// menu of the inbox (task E8.6b) has segments between its entries and ends in "All rules ↗". Draws
/// exactly what [`menu`] draws for the item: 30 high, mark, label, trail, the dot of `current`.
/// Put it in a card of `style::container::menu` with [`style::MENU_INSET`] inside.
///
/// ```ignore
/// let rules = MenuItem::new(Icon::Settings, tr("All rules"), Message::OpenSettings)
///     .trail(Trail::Icon(Icon::ArrowUpRight));
/// list = list.push(widgets::menu_item(rules));
/// ```
pub fn menu_item<'a, M: Clone + 'a>(item: MenuItem<'a, M>) -> Element<'a, M> {
    menu_entry(item)
}

fn menu_entry<'a, M: Clone + 'a>(item: MenuItem<'a, M>) -> Element<'a, M> {
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(mark) = item.mark {
        content = content.push(mark_view(mark, Scale::Row, Tone::Text));
    }
    content = content.push(line_label(typo::body(), item.label)).push(trail_view(item.trail));
    if item.current {
        content = content.push(container(blank()).width(ICON_GAP).height(ICON_GAP).style(|t| container::Style {
            background: Some(Background::Color(palette(t).text_2)),
            border: border::rounded(ICON_GAP / 2.0),
            ..container::Style::default()
        }));
    }
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(item.on_press)
        .into()
}

/// A menu card: `menu` surface, `line_strong` border, radius 8, shadow, 4 inside, [`menu_width()`]
/// wide; `sections` top to bottom with a line between them. Show it with [`floating`].
pub fn menu<'a, M: Clone + 'a>(sections: Vec<MenuSection<'a, M>>) -> Element<'a, M> {
    let mut list = column![];
    for (i, section) in sections.into_iter().enumerate() {
        if i > 0 {
            list = list.push(container(rule::horizontal(LINE).style(style::rule)).padding([space::xs(), 0.0]));
        }
        if let Some(title) = section.title {
            let label = typo::text(typo::caption(), title).style(theme::text_3);
            list = list.push(container(label).padding(Padding::new(space::s()).bottom(space::xs())));
        }
        list = list.extend(section.items.into_iter().map(menu_entry));
    }
    container(list).width(menu_width()).padding(style::MENU_INSET).style(style::container::menu).into()
}

/// A header button that opens a menu, with a chevron: the navigation menu of the medium and
/// narrow layouts ("☀ Today ▾"). Ghost, label `BODY` in `text`, icon 14. Registers `anchor`.
pub fn menu_button<'a, M: Clone + 'a>(
    anchor: &Anchor,
    icon: Option<Icon>,
    label: impl IntoFragment<'a>,
    on_press: M,
) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if let Some(which) = icon {
        content = content.push(toned(which, icon::SIZE_SM, Tone::Text));
    }
    content = content.push(typo::text(typo::body(), label).style(theme::text).wrapping(Wrapping::None)).push(toned(
        Icon::ChevronDown,
        icon::SIZE_SM,
        Tone::Text3,
    ));
    let b = button(container(content).center_y(Fill))
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::ghost)
        .on_press(on_press);
    anchored(anchor, b)
}

/// A framed button that opens a menu of choices, with a chevron: "Week ▾". Secondary, label
/// `CAPTION`. Registers `anchor`; its menu usually opens with [`Align::End`].
pub fn select_button<'a, M: Clone + 'a>(anchor: &Anchor, label: impl IntoFragment<'a>, on_press: M) -> Element<'a, M> {
    let content = row![
        typo::text(typo::caption(), label).wrapping(Wrapping::None),
        toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text3)
    ]
    .spacing(ICON_GAP)
    .align_y(Alignment::Center);
    let b = button(container(content).center_y(Fill))
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::secondary)
        .on_press(on_press);
    anchored(anchor, b)
}

/// Escape pressed (and not taken by a text input): subscribe while a menu, card or drawer is open
/// and close it: `widgets::escape().map(|()| Message::CloseMenu)`.
pub fn escape() -> Subscription<()> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. } => Some(()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------------
// Ctrl shortcuts and text fields
// ---------------------------------------------------------------------------------------------

/// Whether a key press is a Ctrl shortcut (⌘ on macOS) that Iced's text fields would still type
/// as a character: `text` is the character the system made of the key, and a field inserts it
/// even though Ctrl is held (Ctrl + Plus, Ctrl + `-`, Ctrl + `0` leave "a+--" in a field while the
/// window zooms).
///
/// Not a typing shortcut: Alt held on Windows and Linux (Windows reports AltGr as Ctrl + Alt, and
/// AltGr + Q types `@`). On macOS every ⌥⌘ press is one, on purpose and not only the ones the
/// shortcut table uses: ⌥ changes the character of any key (⌥⌘F would type "ƒ"), and ⌥⌘ is a
/// shortcut modifier there, not a way to type (Apple Mail searches with ⌥⌘F, task E10.1b);
/// keys with no printable character (Ctrl + letters send control characters, arrows and Backspace
/// are named keys); Ctrl + C, X, V and A, which a field handles itself as copy, cut, paste and
/// select all (also on non-Latin layouts, hence `to_latin`); and the sidebar key, which an app
/// handles when no field takes it.
pub fn is_ctrl_typing(
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> bool {
    if !modifiers.command() || (modifiers.alt() && !cfg!(target_os = "macos")) {
        return false;
    }
    if text.and_then(|text| text.chars().next()).is_none_or(|c| c.is_control()) {
        return false;
    }
    !matches!(key.to_latin(physical_key), Some('c' | 'x' | 'v' | 'a')) && !is_sidebar_key(key, modifiers)
}

/// Sits on top of a window and swallows the key presses [`is_ctrl_typing`] finds, before any
/// widget below sees them. Draws nothing and never touches the mouse.
struct CtrlGuard;

impl<M> canvas::Program<M> for CtrlGuard {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, text, .. }) = event
        else {
            return None;
        };
        is_ctrl_typing(key, *physical_key, *modifiers, text.as_deref()).then(canvas::Action::capture)
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &iced::Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        Vec::new()
    }
}

/// `content` (the whole window) with a guard on top so that Ctrl + character never types into a
/// text field ([`is_ctrl_typing`]); `text_input` and `text_editor` alike, in every place of the
/// window, now and later. Copy, cut, paste, select all, arrows and Backspace work as before.
///
/// Iced offers no hook on the field itself (that needs its `advanced` feature and a wrapper
/// around every field), but an event that a layer on top captures never reaches the layers below.
/// Subscriptions still get it, also captured (the zoom keys rely on that). Each app wraps its
/// whole view: `widgets::ctrl_guard(self.zoom.view(self.content()))`. The layer count never
/// changes, so the window keeps its state (focus, scroll positions).
pub fn ctrl_guard<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    stack![content.into(), canvas(CtrlGuard).width(Fill).height(Fill)].width(Fill).height(Fill).into()
}

// ---------------------------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------------------------

/// How long a [`toast`] stays: five seconds, then the app takes it away. Long enough to read it and
/// to press "Undo", short enough not to linger over the list.
pub const TOAST_FOR: Duration = Duration::from_secs(5);

/// The one button of a [`toast`]: its word, the key that does the same (shown as [`kbd`]) and the
/// message it sends.
#[derive(Debug, Clone)]
pub struct ToastAction<'a, M> {
    /// The word on the button ("Undo"), sentence case through `tr`.
    pub label: iced::widget::text::Fragment<'a>,
    /// The key that does the same, shown after the button ([`undo_hint`] for "Undo"); `None`
    /// shows none.
    pub key: Option<&'static str>,
    /// Sent when the button is pressed.
    pub on_press: M,
}

impl<'a, M> ToastAction<'a, M> {
    /// A button with `label` and the key hint `key` that sends `on_press`.
    pub fn new(label: impl IntoFragment<'a>, key: Option<&'static str>, on_press: M) -> Self {
        Self { label: label.into_fragment(), key, on_press }
    }

    /// "Undo" with the suite's undo key ([`undo_hint`]), sending `on_press`.
    pub fn undo(on_press: M) -> Self {
        Self::new(tr("Undo"), Some(undo_hint()), on_press)
    }
}

/// A toast (task E3.11): what just happened, quietly at the bottom centre of the window, with at
/// most one button – "Undo" after a change that can be taken back ([`ToastAction::undo`]).
///
/// Surface `menu`, frame `line_strong`, radius [`radius::md()`] and the menu's shadow
/// (`style::container::menu`); the text in `BODY` and `text`, [`size::control_lg()`] high with one
/// line, taller when a long text wraps, at most [`size::content_max()`] wide, [`space::xl()`] above
/// the window's bottom edge. It does not move or fade: it appears and goes.
///
/// The toast is a layer as large as the window: stack it over the window's content, always (an
/// empty [`blank`] when there is nothing to say), so the window keeps its state as it comes and
/// goes. The app keeps when it appeared, takes it away after [`TOAST_FOR`] and on Escape, and
/// listens to [`undo_key`] while it can be undone.
///
/// ```ignore
/// let toast = match &self.toast {
///     Some(toast) => widgets::toast(toast.text.as_str(), toast.undo.is_some().then(|| ToastAction::undo(Message::Undo))),
///     None => Space::new().into(),
/// };
/// stack![self.window(), toast]
/// ```
pub fn toast<'a, M: Clone + 'a>(text: impl IntoFragment<'a>, action: Option<ToastAction<'a, M>>) -> Element<'a, M> {
    let words = typo::text(typo::body(), text).style(theme::text).wrapping(Wrapping::WordOrGlyph);
    let mut content = row![words].spacing(space::s()).align_y(Alignment::Center);
    // Room at the right as at the left without a button; with one, its own padding makes up for it.
    let mut right = space::m();
    if let Some(action) = action {
        content = content.push(text_button(None, action.label, style::button::ghost, Some(action.on_press)));
        if let Some(key) = action.key {
            content = content.push(kbd(key));
        }
        right = space::s();
    }
    // One line of 28 (the button) and 4 above and below is `control_lg`, the height of a toast.
    let inset = (size::control_lg() - size::control_sm()) / 2.0;
    let card = container(content)
        .max_width(size::content_max())
        .padding(Padding { top: inset, bottom: inset, left: space::m(), right })
        .style(style::container::menu);
    container(card).width(Fill).height(Fill).center_x(Fill).align_bottom(Fill).padding(space::xl()).into()
}

/// The suite's undo key as a key hint: "Cmd Z" on macOS, "Ctrl Z" elsewhere ([`is_undo_key`]).
pub fn undo_hint() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd Z" } else { "Ctrl Z" }
}

/// Whether a key press is the suite's undo key: Ctrl + Z, on macOS ⌘ + Z or Ctrl + Z (task E10.1:
/// both count the same there, as `Mod` does in `torro_core::shortcuts`), without Shift or Alt
/// (Ctrl + Shift + Z is redo elsewhere), on any layout: the key that types Z (a German keyboard's Z
/// sits where the US one has Y), on a non-Latin layout the key in Z's place (`to_latin`).
pub fn is_undo_key(key: &keyboard::Key, physical_key: keyboard::key::Physical, modifiers: keyboard::Modifiers) -> bool {
    is_undo_key_on(System::current(), key, physical_key, modifiers)
}

/// [`is_undo_key`] as `system` has it, so both systems are tested everywhere.
fn is_undo_key_on(
    system: System,
    key: &keyboard::Key,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> bool {
    let held = held(modifiers);
    is_mod(system, held)
        && !held.shift
        && !held.alt
        && key.to_latin(physical_key).map(|c| c.to_ascii_lowercase()) == Some('z')
}

/// The modifiers of a key press as `torro_core::shortcuts` reads them (task E10.1): the logo key is
/// `cmd`, ⌘ on macOS.
pub fn held(modifiers: keyboard::Modifiers) -> Held {
    Held { ctrl: modifiers.control(), cmd: modifiers.logo(), alt: modifiers.alt(), shift: modifiers.shift() }
}

/// Whether the suite's command key `Mod` is held on `system`: Ctrl on Windows and Linux, ⌘ or Ctrl
/// – one of them – on macOS (`torro_core::shortcuts::Shortcut::matches`).
fn is_mod(system: System, held: Held) -> bool {
    match system {
        System::Windows | System::Linux => held.ctrl && !held.cmd,
        System::Mac => held.ctrl != held.cmd,
    }
}

/// The undo key pressed ([`is_undo_key`]): subscribe while a toast can be undone and take it back,
/// `widgets::undo_key().map(|()| Message::Undo)`. It listens to every key press, also those a text
/// field or [`ctrl_guard`] took – Iced's fields have no undo of their own, and the guard captures
/// ⌘ + Z on macOS.
pub fn undo_key() -> Subscription<()> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }
            if is_undo_key(&key, physical_key, modifiers) =>
        {
            Some(())
        }
        _ => None,
    })
}

/// A floating card (an event in the medium calendar): 320 wide, `menu` surface, radius 10,
/// 16 inside ([`layout::panel_padding`]), shadow. `head` (calendar or list with "…" and ✕), `title` in `TITLE`, `rows`
/// ([`prop_row`]s) and, below a line, `actions` (buttons). Show it with [`floating`].
pub fn popover<'a, M: 'a>(
    head: impl Into<Element<'a, M>>,
    title: impl IntoFragment<'a>,
    rows: Vec<Element<'a, M>>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    // The card has the panel padding above and below; at the sides the rows bring their own 8
    // ([`style::ITEM_PADDING`]) so their hover surface reaches out, and everything else gets the same
    // 8 inside the card's own, so the icons line up with the title.
    let rows_inset = style::ITEM_PADDING.left;
    let pad = layout::panel_padding();
    let inset = Padding::new(0.0).left(rows_inset).right(rows_inset);
    let mut card = column![
        container(head).padding(inset.bottom(space::xs())),
        container(typo::text(typo::title(), title)).padding(inset.bottom(space::m())),
        column(rows)
    ];
    if !actions.is_empty() {
        card = card.push(
            column![
                rule::horizontal(LINE).style(style::rule),
                row(actions).spacing(space::s()).align_y(Alignment::Center)
            ]
            .spacing(space::m())
            .padding(inset.top(space::m())),
        );
    }
    let outer = Padding { left: (pad.left - rows_inset).max(0.0), right: (pad.right - rows_inset).max(0.0), ..pad };
    container(card).width(POPOVER_WIDTH).padding(outer).style(style::container::popover).into()
}

// ---------------------------------------------------------------------------------------------
// Cards and source tiles (task E8.2, the comfortable density of the inbox)
// ---------------------------------------------------------------------------------------------

/// A card: one entry in the comfortable density of the inbox (task E8.2, `docs/eingang.md`
/// "Dichte"; Canary Mail for mail, the columns of the notifications window for the rest). Surface
/// `raised`, frame `line_strong`, radius [`radius::lg()`], padding [`layout::panel_padding()`] (16),
/// its `parts` stacked with [`size::card_gap()`] (12) between them, as wide as its column. Lay
/// several out with the same gap: `column(cards).spacing(size::card_gap())`.
///
/// `parts` from top to bottom, typically [`card_head`] (or a row with an avatar), the text, a
/// [`chip_row`] of [`state_pill`]s (attachment, "Answer by Fri") and **the card's buttons, always
/// shown** – a row of [`text_button`]s and [`icon_button`]s with [`space::s()`] between them, not the
/// hover bar of [`row_actions`]. Nothing in the card is red but a primary button, and that only
/// where the window has no other.
///
/// `selected` (the card under the keyboard cursor, the one in the reader) gives it the `selected`
/// surface with the `line_selected` edge, as a chosen chip. With `on_press` the whole card is a
/// button (it opens the entry): hover and press draw the `line_selected` edge, press also the
/// `selected` surface. Buttons inside take their own clicks first, so a click on "Done" does not
/// open the card as well. Text in the card is `text` unless a part sets its own colour.
///
/// ```ignore
/// let head = widgets::card_head(Some(widgets::source_tile(TileFace::letter("Slack"), TileSize::Medium, None)),
///     "#dev-torro · Thread", "3 new · 10:12", Some(Message::Dismiss(id.clone())));
/// let text = typo::text(typo::body(), preview).style(theme::text_2);
/// let chips = widgets::chip_row(vec![widgets::state_pill(Icon::Paperclip, "Angebot_v2.pdf")]);
/// let buttons = row![
///     widgets::text_button(Some(Icon::Reply), tr("Reply"), style::button::secondary, Some(Message::Reply(id.clone()))),
///     widgets::text_button(Some(Icon::Clock), tr("Later"), style::button::ghost, Some(Message::Later(id.clone()))),
/// ]
/// .spacing(space::s());
/// widgets::card(vec![head, text.into(), chips, buttons.into()], selected, Some(Message::Select(id)))
/// ```
pub fn card<'a, M: Clone + 'a>(parts: Vec<Element<'a, M>>, selected: bool, on_press: Option<M>) -> Element<'a, M> {
    let content = column(parts).spacing(size::card_gap()).width(Fill);
    match on_press {
        Some(message) => button(content)
            .width(Fill)
            .padding(layout::panel_padding())
            .style(card_style(selected))
            .on_press(message)
            .into(),
        None => container(content)
            .width(Fill)
            .padding(layout::panel_padding())
            .style(move |t| {
                let style = card_style(selected)(t, button::Status::Active);
                container::Style {
                    background: style.background,
                    text_color: Some(style.text_color),
                    border: style.border,
                    ..container::Style::default()
                }
            })
            .into(),
    }
}

/// The look of a [`card`]: `raised` with a `line_strong` frame at rest, `line_selected` on hover,
/// `selected` with `line_selected` when pressed or `selected` – the states of a framed button
/// (`style::button::secondary`), with the card's radius.
fn card_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Copy {
    move |t, status| {
        let p = palette(t);
        let (surface, edge) = match status {
            _ if selected => (p.selected, p.line_selected),
            button::Status::Pressed => (p.selected, p.line_selected),
            button::Status::Hovered => (p.raised, p.line_selected),
            button::Status::Active | button::Status::Disabled => (p.raised, p.line_strong),
        };
        button::Style {
            background: Some(Background::Color(surface)),
            text_color: p.text,
            border: border::rounded(radius::lg()).color(edge).width(LINE),
            ..button::Style::default()
        }
    }
}

/// The head of a [`card`] (task E8.2): `lead` (a [`source_tile`], an avatar, an icon) at the left,
/// then the `title` in `BODY_STRONG` and `text` with `meta` below it in `META` and `text_3` ("3 new
/// · 10:12", "Direct message · 09:58"); an empty `meta` leaves its line out. Both stay on one line
/// each and are cut with "…" to the room they get, the whole text as tooltip ([`typo::ellipsize`]).
///
/// `dismiss` puts the ✕ ([`icon_button`], tooltip "Dismiss") at the top right: "gone, without doing
/// anything" (`docs/eingang.md`, "Knöpfe je Eintrag"). Leave it out where an entry must not vanish
/// by accident – mail in the mail window, approvals.
///
/// ```ignore
/// widgets::card_head(None, "Jonas Brandt", "Direct message · 09:58", Some(Message::Dismiss(id)))
/// ```
pub fn card_head<'a, M: Clone + 'a>(
    lead: Option<Element<'a, M>>,
    title: impl IntoFragment<'a>,
    meta: impl IntoFragment<'a>,
    dismiss: Option<M>,
) -> Element<'a, M> {
    let mut words = column![one_line(typo::body_strong(), title, theme::text)];
    let meta = meta.into_fragment();
    if !meta.is_empty() {
        words = words.push(one_line(typo::meta(), meta, theme::text_3));
    }
    let mut head = row![].spacing(space::m()).align_y(Alignment::Center);
    if let Some(lead) = lead {
        head = head.push(lead);
    }
    head = head.push(container(words).width(Fill));
    if let Some(message) = dismiss {
        head = head.push(icon_button(Icon::X, tr("Dismiss"), Some(message)));
    }
    head.into()
}

/// What a [`source_tile`] shows: the app's icon, or – for a source without one – a letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileFace {
    /// An icon of [`crate::icon`] (GitHub, the calendar, the suite's grid …).
    Icon(Icon),
    /// One letter, upper case; build it with [`TileFace::letter`].
    Letter(char),
}

impl TileFace {
    /// The first letter or digit of `name`, upper case ("slack" → "S", "#dev" → "D"); "?" when
    /// `name` has none.
    pub fn letter(name: &str) -> TileFace {
        let first = name.chars().find(|c| c.is_alphanumeric()).and_then(|c| c.to_uppercase().next());
        TileFace::Letter(first.unwrap_or('?'))
    }
}

/// The three sizes of a [`source_tile`] (task E8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileSize {
    /// [`size::tile_sm()`] (20), radius [`radius::xs()`]: beside a word, in a compact row or a menu.
    Small,
    /// [`size::tile()`] (28), radius [`radius::sm()`]: a notice row, a column head ("Slack 5").
    Medium,
    /// [`size::tile_lg()`] (36), radius [`radius::md()`]: an attachment, the head of an app's card.
    Large,
}

impl TileSize {
    /// The edge of the tile.
    pub fn edge(self) -> f32 {
        match self {
            TileSize::Small => size::tile_sm(),
            TileSize::Medium => size::tile(),
            TileSize::Large => size::tile_lg(),
        }
    }

    /// The corner radius: a quarter of the edge or so, from the radius scale.
    fn radius(self) -> f32 {
        match self {
            TileSize::Small => radius::xs(),
            TileSize::Medium => radius::sm(),
            TileSize::Large => radius::md(),
        }
    }

    /// The edge of the icon on the tile: 12 on the small one (as in an [`icon_pill`]), 16 else.
    fn icon(self) -> f32 {
        match self {
            TileSize::Small => pill_icon(),
            TileSize::Medium | TileSize::Large => icon::SIZE,
        }
    }

    /// The level of the letter on the tile.
    fn level(self) -> typo::Level {
        match self {
            TileSize::Small | TileSize::Medium => typo::caption(),
            TileSize::Large => typo::body_strong(),
        }
    }
}

/// A source tile (task E8.2): which app an entry comes from, as a small square with its icon or,
/// for a source without one, its letter ([`TileFace`]) in `text`. Three sizes ([`TileSize`]).
///
/// Without `tint` the tile is neutral: `selected` with a `line_strong` frame. With a colour (the
/// source's or account's colour the person chose, `collection_color(…)`) it is that colour as a
/// tinted surface ([`Palette::tinted`] over `panel`, as an event block) without a frame – never the
/// full colour: colours belong to the person and are no large saturated surfaces (design skill,
/// section 1). The tile is no button; the row or card around it takes the click.
///
/// ```ignore
/// widgets::source_tile(TileFace::Icon(Icon::Github), TileSize::Medium, None)
/// widgets::source_tile(TileFace::letter(&source.name), TileSize::Small, Some(widgets::collection_color(source.color.as_deref())))
/// ```
pub fn source_tile<'a, M: 'a>(face: TileFace, size: TileSize, tint: Option<Color>) -> Element<'a, M> {
    let glyph: Element<'a, M> = match face {
        TileFace::Icon(which) => toned(which, size.icon(), Tone::Text),
        TileFace::Letter(letter) => {
            typo::text(size.level(), letter.to_string()).style(theme::text).wrapping(Wrapping::None).into()
        }
    };
    let edge = size.edge();
    let corner = size.radius();
    container(glyph)
        .center(edge)
        .style(move |t: &Theme| {
            let p = palette(t);
            let (surface, frame) = match tint {
                Some(colour) => (theme::over(p.tinted(colour), p.panel), Color::TRANSPARENT),
                None => (p.selected, p.line_strong),
            };
            container::Style {
                background: Some(Background::Color(surface)),
                border: border::rounded(corner).color(frame).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// `text` in `level` and the colour `colour` picks, on one line cut with "…" to the room it gets
/// ([`typo::ellipsize`]); a cut text shows the whole of it as tooltip.
/// `text` on one line in `level` and `colour` (`theme::text`, `theme::text_2` …), cut with "…" to
/// the room it gets and then shown whole as tooltip ([`typo::ellipsize`]) – never cut silently. For
/// a name in a row whose width the layout decides (a suggestion of the hub's Insights, task E8.8b).
pub fn ellipsized<'a, M: 'a>(
    level: typo::Level,
    text: impl IntoFragment<'a>,
    colour: fn(&Theme) -> iced::widget::text::Style,
) -> Element<'a, M> {
    one_line(level, text, colour)
}

fn one_line<'a, M: 'a>(
    level: typo::Level,
    text: impl IntoFragment<'a>,
    colour: fn(&Theme) -> iced::widget::text::Style,
) -> Element<'a, M> {
    let full = text.into_fragment().into_owned();
    responsive(move |room| {
        let shown = typo::ellipsize(&full, level, room.width);
        let cut = shown != full;
        let line = container(typo::text(level, shown).style(colour).wrapping(Wrapping::None)).clip(true);
        if cut { with_tooltip(line, full.clone()) } else { line.into() }
    })
    .height(Length::Shrink)
    .into()
}

// ---------------------------------------------------------------------------------------------
// Chips, pills, key hints, properties
// ---------------------------------------------------------------------------------------------

/// A property chip under a title in the medium layout ("○ Open", "📅 Today, 16:00"): 26 high,
/// `raised` with a `line_strong` frame, `mark` 14 in `text_3`, label `META`. `on_press` opens the
/// property's editor; such a chip is a button with hover and pressed states
/// (`style::button::chip`), without one it is a plain label. An empty label gives an icon-only
/// chip ("…"). For a chip that is *chosen* or not (filters, labels), use [`choice_chip`].
pub fn chip<'a, M: Clone + 'a>(
    mark: Option<Mark>,
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    chip_with(mark.map(|mark| mark_view(mark, Scale::Small, Tone::Text3)), label, on_press)
}

/// A [`chip`] led by a glyph of the app's own instead of a [`Mark`], e.g. the priority bars of the
/// task list, so a chip reads like the row it belongs to. `lead` should be at most 14 high.
///
/// ```ignore
/// widgets::chip_led(list::prio_glyph(prio), prio.label(), Some(Message::Open(Editor::Priority)))
/// ```
pub fn chip_led<'a, M: Clone + 'a>(
    lead: impl Into<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    chip_with(Some(lead.into()), label, on_press)
}

fn chip_with<'a, M: Clone + 'a>(
    lead: Option<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if let Some(lead) = lead {
        content = content.push(lead);
    }
    let label = label.into_fragment();
    if !label.is_empty() {
        content = content.push(typo::text(typo::meta(), label).wrapping(Wrapping::None));
    }
    match on_press {
        // A button, not a mouse area around a container: only a button shows hover and press,
        // and without them a click on a chip seemed to do nothing (task G.10).
        Some(message) => button(container(content).center_y(Fill))
            .height(style::CHIP_HEIGHT)
            .padding(style::CHIP_PADDING)
            .style(style::button::chip)
            .on_press(message)
            .into(),
        None => container(content)
            .center_y(style::CHIP_HEIGHT)
            .padding(style::CHIP_PADDING)
            .style(style::container::chip)
            .into(),
    }
}

/// Chips side by side with 6 between them, wrapping onto more lines when they do not fit.
///
/// Also the chips of a [`card`] (task E8.2): [`state_pill`]s for an attachment and a due date the
/// mail asks for, the colour only in the icon:
///
/// ```ignore
/// widgets::chip_row(vec![
///     widgets::state_pill(Icon::Paperclip, "Angebot_v2.pdf"),
///     widgets::state_pill(Mark::Tinted(Icon::Clock, theme::attention()), tr("Answer by Fri")),
/// ])
/// ```
pub fn chip_row<'a, M: 'a>(chips: Vec<Element<'a, M>>) -> Element<'a, M> {
    row(chips).spacing(ICON_GAP).wrap().vertical_spacing(ICON_GAP).into()
}

/// A chip that is chosen or not: a filter template ("Bugs"), an assignee ("Me"), a milestone, one
/// of several labels. 28 high ([`size::control_sm()`]), radius 6, `mark` in front, `label` in
/// `CAPTION`; look and states from `style::button::choice_chip(on)`.
///
/// - Not chosen: `raised`, `line_strong` frame, `text_2`; hover `hover` + `text`, pressed
///   `selected`, both with the stronger `line_selected` frame.
/// - Chosen (`on`): `selected`, `line_selected` frame, `text`, and a check mark (14) before the
///   label. An icon mark ([`Mark::Icon`]) gives its place to the check, so the chip keeps its width
///   and the row does not reflow; any other mark (a colour dot, a ring) stays after the check,
///   because its colour carries meaning.
/// - `None` for `on_press`: disabled, 40 %. Neutral in every state, never red.
///
/// Marks: an icon at 14, a [`Mark::Dot`] at 8 (a label's colour, use [`label_choice`] for GitHub
/// labels), rings and check boxes at 12. Lay several out with [`chip_row`]. Single or multiple
/// choice is the caller's business: it decides `on` and what `on_press` does.
///
/// ```ignore
/// let chips = Template::ALL.iter().map(|&t| {
///     widgets::choice_chip(t.label(), Some(Mark::Icon(t.icon())), self.template == Some(t), Some(Message::Template(t)))
/// });
/// widgets::chip_row(chips.collect())
/// ```
pub fn choice_chip<'a, M: Clone + 'a>(
    label: impl IntoFragment<'a>,
    mark: Option<Mark>,
    on: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let tone = if on { Tone::Text } else { Tone::Text2 };
    let mark = match mark {
        // The check takes the place of a decorative icon: same width, so the row does not reflow.
        Some(Mark::Icon(_)) if on => None,
        Some(Mark::Dot(colour)) => Some(color_dot(colour, Scale::Row.dot())),
        Some(other) => Some(mark_view(other, Scale::Small, tone)),
        None => None,
    };
    choice_chip_with(mark, label, on, on_press)
}

/// A [`choice_chip`] for a GitHub label: the label's colour as an 8-px dot, then the name. `hex`
/// is `#RRGGBB` or GitHub's `d73a4a` without `#`; an unreadable one gives a neutral dot in `text_3`.
/// The colour is only the dot, the name carries the meaning (as in [`label_chip`]); chosen adds the
/// check mark in front of the dot. For "with label" and "without label" pass the same chip twice
/// with different messages; the caller keeps which labels are chosen.
///
/// ```ignore
/// widgets::chip_row(repo.labels.iter().map(|l| {
///     widgets::label_choice(&l.color, &l.name, self.labels.contains(&l.name), Some(Message::Label(l.name.clone())))
/// }).collect())
/// ```
pub fn label_choice<'a, M: Clone + 'a>(
    hex: &str,
    name: impl IntoFragment<'a>,
    on: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    choice_chip_with(Some(label_dot(hex, Scale::Row.dot())), name, on, on_press)
}

/// The chip of [`choice_chip`] with `lead` (already drawn) after the check mark of a chosen chip.
fn choice_chip_with<'a, M: Clone + 'a>(
    lead: Option<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    on: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if on {
        content = content.push(toned(Icon::Check, icon::SIZE_SM, Tone::Text));
    }
    if let Some(lead) = lead {
        content = content.push(lead);
    }
    content = content.push(typo::text(typo::caption(), label).wrapping(Wrapping::None));
    button(container(content).center_y(Fill))
        .height(size::control_sm())
        .padding(style::CHIP_PADDING)
        .style(style::button::choice_chip(on))
        .on_press_maybe(on_press)
        .into()
}

/// One large choice of [`choice_cards`]: what to make ("Task list" / "GitHub filter list" in "New
/// list").
#[derive(Debug, Clone)]
pub struct ChoiceCard<'a, M> {
    /// The symbol (16) on a [`size::control()`] square of `selected` at the top left.
    pub icon: Icon,
    /// The name of the choice, `BODY_STRONG`.
    pub title: &'a str,
    /// One or two sentences under the title, `META` in `text_2`, wrapping. For a choice that is
    /// off: why, and what to do ("Connect a GitHub account in Settings first.").
    pub text: &'a str,
    /// The key that picks it ("1"), shown at the top right as [`kbd`]. The caller listens for it.
    pub key: &'a str,
    /// `None`: the choice is off.
    pub on_press: Option<M>,
}

/// Large choices in a dialog, side by side and all as high as the highest – or, `stacked`, one
/// under the other where they do not fit side by side. [`space::m()`] between them.
///
/// Each card is a button as wide as its share, `raised` with a `line_strong` frame, radius
/// [`radius::md()`], [`space::l()`] inside: on top the icon on its square and the key hint, below the
/// title and the text. States as a [`crate::style::button::secondary`] button: hover `hover` and
/// pressed `selected`, both with a `line_selected` frame. A card that is off (no `on_press`) shows
/// icon, title and frame at 40 % and no key hint – the key does nothing then –, while its `text`
/// stays at full strength, so the reason can be read.
///
/// ```ignore
/// let cards = vec![
///     ChoiceCard { icon: Icon::ListChecks, title: tr("Task list"), text: tr("Your own tasks …"), key: "1", on_press: Some(Message::Pick(Kind::TaskList)) },
///     ChoiceCard { icon: Icon::Github, title: tr("GitHub filter list"), text: tr("Connect a GitHub account in Settings first."), key: "2", on_press: None },
/// ];
/// widgets::choice_cards(cards, narrow)
/// ```
pub fn choice_cards<'a, M: Clone + 'a>(cards: Vec<ChoiceCard<'a, M>>, stacked: bool) -> Element<'a, M> {
    if stacked {
        return column(cards.into_iter().map(|card| choice_card(card, Length::Shrink))).spacing(space::m()).into();
    }
    // Equal heights: an invisible copy of the cards at their own heights sizes the stack, and the
    // real cards fill it. (A row cannot give its children the height of the highest one when its
    // own height is open, as inside a scrolling dialog.)
    let ghosts = row(cards.iter().map(choice_ghost)).spacing(space::m());
    let real = row(cards.into_iter().map(|card| choice_card(card, Fill))).spacing(space::m());
    stack![ghosts, real].into()
}

/// One card of [`choice_cards`], `height` high.
fn choice_card<'a, M: Clone + 'a>(card: ChoiceCard<'a, M>, height: Length) -> Element<'a, M> {
    let on = card.on_press.is_some();
    // Disabled parts at the disabled opacity, drawn by hand: Iced fades only the button's surface.
    let fade = move |colour: Color| if on { colour } else { Color { a: colour.a * style::DISABLED_ALPHA, ..colour } };
    let symbol: Element<'a, M> =
        icon(card.icon, icon::SIZE).style(move |t, _| svg::Style { color: Some(fade(palette(t).text)) }).into();
    let tile: Element<'a, M> = container(symbol)
        .center(size::control())
        .style(move |t| container::Style {
            background: Some(Background::Color(fade(palette(t).selected))),
            border: border::rounded(radius::md()),
            ..container::Style::default()
        })
        .into();
    let mut top = row![tile, blank::horizontal()].align_y(Alignment::Center);
    if on {
        top = top.push(kbd(card.key));
    }
    let title = typo::text(typo::body_strong(), card.title)
        .style(move |t: &Theme| iced::widget::text::Style { color: Some(fade(palette(t).text)) });
    let content = column![top, title, typo::text(typo::meta(), card.text).style(theme::text_2)].spacing(space::m());
    button(content)
        .width(Fill)
        .height(height)
        .padding(space::l())
        .style(|t, status| {
            let base = style::button::secondary(t, status);
            button::Style { border: base.border.rounded(radius::md()), ..base }
        })
        .on_press_maybe(card.on_press)
        .into()
}

/// The invisible copy of a card that gives [`choice_cards`] its height: the same parts and spaces,
/// drawn in no colour and taking no input.
fn choice_ghost<'a, M: 'a>(card: &ChoiceCard<'a, M>) -> Element<'a, M> {
    let unseen = |_: &Theme| iced::widget::text::Style { color: Some(Color::TRANSPARENT) };
    let content = column![
        blank().width(size::control()).height(size::control()),
        typo::text(typo::body_strong(), card.title).style(unseen),
        typo::text(typo::meta(), card.text).style(unseen),
    ]
    .spacing(space::m());
    container(content).width(Fill).padding(space::l()).into()
}

/// A list label in a row ("● Work"): 20 high, round, `line_strong` frame, dot 6 in `colour`,
/// label `META` in `text_2`. Without a colour it is a plain tag ("review").
pub fn pill<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    pill_at(colour, label, 1.0)
}

/// A [`pill`] led by a 12-px icon in `text_3` instead of the dot: where a row comes from when
/// that is not a list, e.g. the GitHub mark before a repository ("TorroWorkplace").
///
/// ```ignore
/// widgets::icon_pill(Icon::Github, "TorroWorkplace")
/// ```
pub fn icon_pill<'a, M: 'a>(which: Icon, label: impl IntoFragment<'a>) -> Element<'a, M> {
    pill_led(Some(toned(which, pill_icon(), Tone::Text3)), label, 1.0)
}

/// [`pill`] for a disabled row ("Coming soon" next to a service that is not available yet): the
/// whole pill at the disabled opacity of 40 %, like the row's text.
pub fn pill_disabled<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    pill_at(colour, label, style::DISABLED_ALPHA)
}

/// The pill with its colours at `alpha` of their strength.
fn pill_at<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>, alpha: f32) -> Element<'a, M> {
    pill_led(colour.map(|colour| color_dot(colour.scale_alpha(alpha), Scale::Small.dot())), label, alpha)
}

/// The pill with `lead` (dot or icon) before the label, its colours at `alpha` of their strength.
fn pill_led<'a, M: 'a>(lead: Option<Element<'a, M>>, label: impl IntoFragment<'a>, alpha: f32) -> Element<'a, M> {
    let mut content = row![].spacing(space::xs()).align_y(Alignment::Center);
    if let Some(lead) = lead {
        content = content.push(lead);
    }
    content = content.push(typo::text(typo::meta(), label).wrapping(Wrapping::None));
    container(content)
        .center_y(PILL_HEIGHT)
        .padding(Padding::new(0.0).left(ICON_GAP).right(ICON_GAP))
        .style(move |t| {
            let p = palette(t);
            container::Style {
                text_color: Some(p.text_2.scale_alpha(alpha)),
                border: border::rounded(PILL_HEIGHT / 2.0).color(p.line_strong.scale_alpha(alpha)).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// A key hint ("N", "W", "Ctrl K"): `CAPTION` in `text_3` in a small `line_strong` frame.
pub fn kbd<'a, M: 'a>(key: impl IntoFragment<'a>) -> Element<'a, M> {
    container(typo::text(typo::caption(), key)).padding(style::KBD_PADDING).style(style::container::kbd).into()
}

/// A row in the properties column or a floating card: 32 high, `mark` 16 in `text_3`, then
/// `value` (text in `BODY`, pills, avatars …). With `on_press` it is a button with a hover surface
/// that opens the editor. Its 8 inner margin is meant to stick out of the column's text edge, as
/// in Linear; [`prop_section`] lines its title up with it.
pub fn prop_row<'a, M: Clone + 'a>(
    mark: Option<Mark>,
    value: impl Into<Element<'a, M>>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(mark) = mark {
        content = content.push(mark_view(mark, Scale::Row, Tone::Text3));
    }
    content = content.push(value);
    let content = container(content).center_y(Fill);
    match on_press {
        Some(message) => button(content)
            .width(Fill)
            .height(size::control())
            .padding(style::ITEM_PADDING)
            .style(style::button::sidebar_item(false))
            .on_press(message)
            .into(),
        None => container(content).width(Fill).height(size::control()).padding(style::ITEM_PADDING).into(),
    }
}

/// A group of [`prop_row`]s under a `CAPTION` title in `text_3` ("Properties", "List"). Stack the
/// sections with `space::l()` between them.
pub fn prop_section<'a, M: 'a>(title: impl IntoFragment<'a>, rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    let title = container(typo::text(typo::caption(), title).style(theme::text_3))
        .padding(Padding { bottom: space::xs(), ..style::ITEM_PADDING });
    column![title].extend(rows).into()
}

/// A property with its name in front, for the properties column of a detail ("Priority  ▂▄▆ High"):
/// the `label` in `META` and `text_3`, 88 wide so the values line up, then `mark` (16, `text_3`)
/// and the `value`. At least 32 high, taller when the value wraps. With `on_press` the whole row
/// is a button with a hover surface that opens the editor; its 8 inner margin sticks out of the
/// column's text edge, like [`prop_row`].
///
/// Fill `value` with [`prop_value`], [`prop_placeholder`] (nothing set yet: "Add due date") or any
/// element (a [`label_chip_row`], a date in `BODY.tabular()`).
///
/// ```ignore
/// widgets::prop_line(tr("Due"), Some(Icon::Calendar.into()), widgets::prop_value(&due), Some(Message::EditDue))
/// widgets::prop_line(tr("Reminder"), Some(Icon::Bell.into()), widgets::prop_placeholder(tr("Add reminder")), Some(Message::EditReminder))
/// widgets::prop_line(tr("Labels"), None, widgets::label_chip_row(chips), Some(Message::EditLabels))
/// ```
pub fn prop_line<'a, M: Clone + 'a>(
    label: impl IntoFragment<'a>,
    mark: Option<Mark>,
    value: impl Into<Element<'a, M>>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let name = container(typo::text(typo::meta(), label).style(theme::text_3).wrapping(Wrapping::None))
        .width(PROP_LABEL_WIDTH)
        .clip(true);
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(mark) = mark {
        content = content.push(mark_view(mark, Scale::Row, Tone::Text3));
    }
    content = content.push(container(value.into()).width(Fill));
    let line = row![name, content].spacing(space::s()).align_y(Alignment::Center);
    let padding = Padding { top: PROP_PADDING_Y, bottom: PROP_PADDING_Y, ..style::ITEM_PADDING };
    match on_press {
        Some(message) => {
            button(line).width(Fill).padding(padding).style(style::button::sidebar_item(false)).on_press(message).into()
        }
        None => container(line).width(Fill).padding(padding).into(),
    }
}

/// The value of a [`prop_line`] that is set: one line of `BODY` in `text`, clipped at the edge.
pub fn prop_value<'a, M: 'a>(value: impl IntoFragment<'a>) -> Element<'a, M> {
    prop_text(value, theme::text)
}

/// The value of a [`prop_line`] that is not set yet ("Add due date"): one line of `BODY` in
/// `text_3`. The row stays a button, so a click sets it.
pub fn prop_placeholder<'a, M: 'a>(placeholder: impl IntoFragment<'a>) -> Element<'a, M> {
    prop_text(placeholder, theme::text_3)
}

/// One line of `BODY` in the colour `colour` picks, cut with "…" to the width it gets
/// ([`typo::ellipsize`]; Iced 0.14 cannot do that itself); a cut value shows the whole of it as a
/// tooltip. The colour is set explicitly because the text sits in a button, which would otherwise
/// hand it its own (`text_2`).
fn prop_text<'a, M: 'a>(
    text: impl IntoFragment<'a>,
    colour: fn(&Theme) -> iced::widget::text::Style,
) -> Element<'a, M> {
    let full = text.into_fragment().into_owned();
    responsive(move |room| {
        let shown = typo::ellipsize(&full, typo::body(), room.width);
        let cut = shown != full;
        let line = container(typo::text(typo::body(), shown).style(colour).wrapping(Wrapping::None)).clip(true);
        if cut { with_tooltip(line, full.clone()) } else { line.into() }
    })
    .height(Length::Shrink)
    .into()
}

/// A label of a GitHub issue or pull request ("● bug"): 20 high, `line_strong` frame with radius 4,
/// the name in `CAPTION` and `text_2`, led by a 6-px dot in the label's own colour. `hex` is the
/// colour as the server sends it, `#RRGGBB` (GitHub's `d73a4a` without the `#` works as well); an
/// unreadable one gives a neutral dot. The colour is only ever the dot: the name carries the
/// meaning. Not a button; clicks belong to the row around it.
///
/// A row shows at most two and then "+n": `typo::text(typo::caption().tabular(), format!("+{n}"))`
/// in `theme::text_3`.
///
/// ```ignore
/// widgets::label_chip_row(issue.labels.iter().map(|l| widgets::label_chip(&l.color, &l.name)).collect())
/// ```
pub fn label_chip<'a, M: 'a>(hex: &str, name: impl IntoFragment<'a>) -> Element<'a, M> {
    let dot = label_dot(hex, Scale::Small.dot());
    let content = row![dot, typo::text(typo::caption(), name).wrapping(Wrapping::None)]
        .spacing(ICON_GAP)
        .align_y(Alignment::Center);
    container(content)
        .center_y(PILL_HEIGHT)
        .padding(Padding::new(0.0).left(ICON_GAP).right(ICON_GAP))
        .style(|t| {
            let p = palette(t);
            container::Style {
                text_color: Some(p.text_2),
                border: border::rounded(radius::xs()).color(p.line_strong).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// What a mail is, as a pill in the list, the reader and the panel (task E12.7,
/// `docs/kategorien.md`): the group's icon and colour, the category's words.
///
/// `group` is the index of `torro_mail::CategoryGroup` (0 waiting for me, 3 junk) and picks the
/// colour out of [`theme::Palette::category`]; a larger index falls back to the last. Colour and
/// icon are per **group**, the words per category: 23 colours could not stay apart, and four can.
///
/// `suggested` marks a category a model chose (`docs/ki.md`, principle 5): the sparkles the suite
/// puts on every AI result go in front. A category from a header or from the person has none.
///
/// Not a button: the click belongs to the row around it. Changing the category goes through the
/// row's menu.
pub fn category_pill<'a, M: 'a>(
    icon: Icon,
    group: usize,
    label: impl IntoFragment<'a>,
    suggested: bool,
) -> Element<'a, M> {
    let at = group.min(3);
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if suggested {
        content = content.push(sparkle(pill_icon()));
    }
    content = content.push(group_icon(icon, at)).push(typo::text(typo::caption(), label).wrapping(Wrapping::None));
    container(content)
        .center_y(PILL_HEIGHT)
        .padding(Padding::new(0.0).left(ICON_GAP).right(ICON_GAP))
        .style(move |t| {
            let p = palette(t);
            container::Style {
                text_color: Some(p.text_2),
                border: border::rounded(radius::xs()).color(p.category[at]).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// The small mark of a mail's category for a row of the list (task E12.8): the group's icon in
/// the group's colour, `edge` wide. The words belong in its tooltip – a row's marks are icons, so
/// the whole pill ([`category_pill`]) stays for the reader and the card, where there is room.
pub fn category_mark<'a, M: 'a>(which: Icon, group: usize, edge: f32) -> Element<'a, M> {
    let at = group.min(3);
    icon(which, edge).style(move |t, _| svg::Style { color: Some(palette(t).category[at]) }).into()
}

/// The icon of a category group in its own colour ([`category_pill`]).
fn group_icon<'a, M: 'a>(which: Icon, group: usize) -> Element<'a, M> {
    icon(which, pill_icon()).style(move |t, _| svg::Style { color: Some(palette(t).category[group]) }).into()
}

/// The sparkles the suite marks every AI result with, in the accent colour.
fn sparkle<'a, M: 'a>(edge: f32) -> Element<'a, M> {
    icon(Icon::Sparkles, edge).style(move |t, _| svg::Style { color: Some(palette(t).accent) }).into()
}

/// [`label_chip`]s side by side with 4 between them, wrapping onto more lines when they do not
/// fit (the value of a "Labels" [`prop_line`]).
pub fn label_chip_row<'a, M: 'a>(chips: Vec<Element<'a, M>>) -> Element<'a, M> {
    row(chips).spacing(space::xs()).wrap().vertical_spacing(space::xs()).into()
}

/// A dot of `edge` in the colour of a label (`hex` with or without `#`); an unreadable colour gives
/// a neutral dot in `text_3` of the current mode.
fn label_dot<'a, M: 'a>(hex: &str, edge: f32) -> Element<'a, M> {
    match label_colour(hex) {
        Some(colour) => color_dot(colour, edge),
        None => container(blank().width(edge).height(edge))
            .style(move |t| container::Style {
                background: Some(Background::Color(palette(t).text_3)),
                border: border::rounded(edge / 2.0),
                ..container::Style::default()
            })
            .into(),
    }
}

/// The colour of a label from its `hex`, with or without `#`.
fn label_colour(hex: &str) -> Option<Color> {
    color_from_hex(hex).or_else(|| color_from_hex(&format!("#{}", hex.trim())))
}

/// The state of an item in words ("Merged", "Draft", "Open", "Completed"): 22 high, round,
/// `line_strong` frame, `mark` 14 in front, the `label` in `CAPTION` and `text`. The colour sits
/// only in the mark, the word says the state, so the pill reads without colour.
///
/// Pass an icon tinted with the palette of the current theme, as [`group_head`] does:
///
/// ```ignore
/// let p = palette(&self.theme());
/// widgets::state_pill(Mark::Tinted(Icon::GitMerge, p.merged), tr("Merged"))
/// widgets::state_pill(Mark::Tinted(Icon::CircleDot, p.ok), tr("Open"))
/// widgets::state_pill(Icon::GitPullRequestDraft, tr("Draft")) // neutral, in text_3
/// ```
pub fn state_pill<'a, M: 'a>(mark: impl Into<Mark>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    let content = row![
        mark_view(mark.into(), Scale::Small, Tone::Text3),
        typo::text(typo::caption(), label).wrapping(Wrapping::None)
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    container(content)
        .center_y(STATE_PILL_HEIGHT)
        .padding(Padding::new(0.0).left(ICON_GAP).right(space::s()))
        .style(|t| {
            let p = palette(t);
            container::Style {
                text_color: Some(p.text),
                border: border::rounded(STATE_PILL_HEIGHT / 2.0).color(p.line_strong).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// The share of `done` in `total` for [`progress_bar`]: 0 for an empty list, never above 1.
fn progress_fraction(done: usize, total: usize) -> f32 {
    if total == 0 { 0.0 } else { (done as f32 / total as f32).min(1.0) }
}

/// A progress bar of `done` out of `total` ("2/4" sub-tasks): 64 × 4, round, a `line_strong` track
/// with the `ok` colour filled from the left. Put the count next to it in `META` and `text_3`
/// (the bar alone is no information); an empty list (`total` 0) shows an empty track.
///
/// ```ignore
/// row![widgets::progress_bar(done, total), typo::text(typo::meta().tabular(), format!("{done}/{total}")).style(theme::text_3)].spacing(space::s())
/// ```
pub fn progress_bar<'a, M: 'a>(done: usize, total: usize) -> Element<'a, M> {
    let fill =
        container(blank().width(PROGRESS_WIDTH * progress_fraction(done, total)).height(PROGRESS_HEIGHT)).style(|t| {
            container::Style {
                background: Some(Background::Color(palette(t).ok)),
                border: border::rounded(PROGRESS_HEIGHT / 2.0),
                ..container::Style::default()
            }
        });
    container(fill)
        .width(PROGRESS_WIDTH)
        .height(PROGRESS_HEIGHT)
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).line_strong)),
            border: border::rounded(PROGRESS_HEIGHT / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A small muted heading over a group of rows: `CAPTION` in `text_3`, in sentence case as written
/// (Linear uses no capitals for titles).
pub fn section_title<'a, M: 'a>(title: &str) -> Element<'a, M> {
    typo::text(typo::caption(), title.to_owned()).style(theme::text_3).into()
}

// ---------------------------------------------------------------------------------------------
// Settings pages
// ---------------------------------------------------------------------------------------------

/// A group on a settings page: `title` ([`section_title`] look), the `rows` ([`group_row`]) in a
/// framed `raised` card with lines between them, and an explaining `footer` in `text_2` below.
pub fn group<'a, M: 'a>(
    title: impl IntoFragment<'a>,
    rows: Vec<Element<'a, M>>,
    footer: Option<String>,
) -> Element<'a, M> {
    let mut card = column![];
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            card = card.push(rule::horizontal(LINE).style(style::rule));
        }
        card = card.push(r);
    }
    let mut block = column![
        typo::text(typo::caption(), title).style(theme::text_3),
        container(card).width(Fill).style(style::container::group)
    ]
    .spacing(layout::item_gap());
    if let Some(footer) = footer {
        block = block.push(typo::text(typo::meta(), footer).style(theme::text_2));
    }
    block.into()
}

/// A row of a settings [`group`]: `label` in `BODY`, an optional `detail` below it in `META`
/// `text_2`, and one `control` at the right (toggler, pick list, button; the rest goes into a "…"
/// menu). 44 high, 56 with a detail; 16 at the sides ([`layout::row_padding`]).
pub fn group_row<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let height = if detail.is_some() { size::header() + space::m() } else { size::header() };
    let mut text = column![typo::text(typo::body(), label)];
    if let Some(detail) = detail {
        text = text.push(typo::text(typo::meta(), detail).style(theme::text_2));
    }
    container(row![text.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center))
        .center_y(height)
        .padding(layout::row_padding())
        .into()
}

// ---------------------------------------------------------------------------------------------
// Rows with a segmented control (rights)
// ---------------------------------------------------------------------------------------------
//
// The rights of a client or a bot (task U.13): the control centre's API and MCP and the bot
// editor of the assistant show them alike – a row per area with a [`segmented`] control of the
// levels, all controls ending in one column at the right ([`segment_column`]), or, where a name
// would not fit beside that column, each control on a line of its own under its name.

/// The estimated width of a [`segmented`] control with `labels`: each label in `CAPTION` with the
/// button padding, plus the frame's inset and border. `typo::text_width` estimates generously, so
/// the control fits.
pub fn segmented_width(labels: &[&str]) -> f32 {
    let padding = style::button_padding().left + style::button_padding().right;
    let segments: f32 = labels.iter().map(|label| typo::text_width(label, typo::caption()) + padding).sum();
    segments + 2.0 * (style::SEGMENT_INSET + size::HAIRLINE)
}

/// Where the rows put their segmented controls in a group `width` wide: `Some(slot)`, a column `slot`
/// wide at the right of every row, while the longest name (`BODY`) and line (`META`) of `texts`
/// fit beside it; `None` when they would not (a narrow window), then each control stands under its
/// name ([`segment_row`]). Decided by what fits rather than by the size class, so longer
/// translations hold too.
pub fn segment_column<'t>(
    slot: f32,
    texts: impl IntoIterator<Item = (&'t str, Option<&'t str>)>,
    width: f32,
) -> Option<f32> {
    let text = texts
        .into_iter()
        .flat_map(|(label, detail)| {
            let detail = detail.map_or(0.0, |detail| typo::text_width(detail, typo::meta()));
            [typo::text_width(label, typo::body()), detail]
        })
        .fold(0.0, f32::max);
    // A row of a group: 16 at both sides and 16 between the text and the control.
    (width >= 3.0 * space::l() + text + slot).then_some(slot)
}

/// A row of a settings [`group`] with a segmented `control`. With a `column` ([`segment_column`])
/// the row is 44 high (56 with a `detail`), 16 at the sides, name and line on one line each, the
/// control at the right of the column – right-aligned, so controls of every width end on the axis
/// of the switches, chevrons and buttons of the other rows. Without one the name, the line below it
/// (wrapping) and the control on a line of its own under both; 12 above and below, 16 at the sides.
/// The sides stay 16 in every design, the width [`segment_column`] reckons with.
pub fn segment_row<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
    column: Option<f32>,
) -> Element<'a, M> {
    segment_row_noted(label, None, detail, control, column)
}

/// A [`segment_row`] with a quiet `note` right after the name, in `META` and `text_3` (the rights
/// editor's "· 2 off"): it says something about the row, but is not part of its name. The caller
/// writes the note as it is shown, separator included.
pub fn segment_row_noted<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    note: Option<String>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
    column: Option<f32>,
) -> Element<'a, M> {
    let name = |wrapping: Wrapping| {
        let name = row![typo::text(typo::body(), label).wrapping(wrapping)]
            .push(note.map(|note| typo::text(typo::meta(), note).style(theme::text_3).wrapping(Wrapping::None)))
            .spacing(space::xs())
            .align_y(Alignment::Center);
        container(name).width(Fill).clip(true)
    };
    match column {
        Some(slot) => {
            let height = if detail.is_some() { size::header() + space::m() } else { size::header() };
            let mut text = column![name(Wrapping::None)];
            if let Some(detail) = detail {
                text = text.push(single_line(typo::text(typo::meta(), detail).style(theme::text_2)));
            }
            let control = container(control).width(slot).align_x(Alignment::End);
            let row = row![text.width(Fill), control].spacing(space::l());
            container(row.align_y(Alignment::Center)).width(Fill).center_y(height).padding([0.0, space::l()]).into()
        }
        None => {
            let mut lines = column![name(Wrapping::WordOrGlyph)];
            if let Some(detail) = detail {
                lines =
                    lines.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
            }
            container(column![lines, control.into()].spacing(space::s()))
                .width(Fill)
                .padding([space::m(), space::l()])
                .into()
        }
    }
}

/// `text` on one line, as wide as there is room, clipped rather than wrapped: a name or line of a
/// [`segment_row`] beside the column, where [`segment_column`] made sure it fits – so an estimate
/// a little short never makes the row taller.
fn single_line<'a, M: 'a>(text: iced::widget::Text<'a>) -> Element<'a, M> {
    container(text.wrapping(Wrapping::None)).width(Fill).clip(true).into()
}

/// The word of a rights level on its segment.
pub fn level_label(level: Level) -> &'static str {
    match level {
        Level::NoAccess => tr("None"),
        Level::Read => tr("Read"),
        Level::Write => tr("Write"),
        Level::Full => tr("Full"),
    }
}

/// The options of "Ask first", in the order the control shows them.
pub fn confirm_options() -> [(&'static str, Confirm); 3] {
    [
        (tr("Never"), Confirm::Never),
        (tr("On delete"), Confirm::OnDelete),
        (tr("On every change"), Confirm::OnEveryChange),
    ]
}

// ---------------------------------------------------------------------------------------------
// Page width and view states
// ---------------------------------------------------------------------------------------------

/// `body` at most `max_width` wide ([`size::content_max()`] for lists and pages,
/// [`size::form_max()`] for forms), centred in the free width – large windows give the extra room
/// to the margins, never to gaps between elements.
pub fn content<'a, M: 'a>(max_width: f32, body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(container(body).max_width(max_width).width(Fill)).center_x(Fill).into()
}

/// The loading state: a calm "Loading …" in `text_3`, centred; no spinner, nothing moves.
pub fn loading<'a, M: 'a>() -> Element<'a, M> {
    center(typo::text(typo::meta(), tr("Loading …")).style(theme::text_3)).into()
}

/// The error state: an alert icon, `what` happened (`BODY`, centred, says what to do) and a
/// "Retry" button that sends `retry`.
pub fn error_state<'a, M: Clone + 'a>(what: impl IntoFragment<'a>, retry: M) -> Element<'a, M> {
    let alert = icon(Icon::CircleAlert, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).error_text) });
    let content = column![
        alert,
        typo::text(typo::body(), what).style(theme::text_2).align_x(Alignment::Center),
        text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::secondary, Some(retry)),
    ]
    .spacing(space::m())
    .align_x(Alignment::Center)
    .max_width(size::form_max() / 2.0);
    center(content).padding(space::xl()).into()
}

/// A band over the content of a message that warns about it (task E12.9, `docs/kategorien.md`):
/// the shield in the error colour, `what` in `BODY_STRONG` and `why` under it in `text_2`, on a
/// surface tinted with the error colour and a border of it.
///
/// A warning, never a wall: what it stands over stays readable, nothing is hidden or moved. The
/// only thing the band changes is that the links below it ask first.
pub fn warning_band<'a, M: 'a>(what: impl IntoFragment<'a>, why: impl IntoFragment<'a>) -> Element<'a, M> {
    let shield = icon(Icon::ShieldAlert, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).error_text) });
    let words = column![
        typo::text(typo::body_strong(), what),
        typo::text(typo::meta(), why).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::xs())
    .width(Fill);
    let content = row![shield, words].spacing(space::m()).align_y(Alignment::Start);
    container(content)
        .width(Fill)
        .padding(space::m())
        .style(|t| {
            let p = palette(t);
            container::Style {
                background: Some(p.error.scale_alpha(0.12).into()),
                text_color: Some(p.error_text),
                border: border::rounded(radius::sm()).color(p.error).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// The empty state of a list or view: what appears here (`title`, `BODY_STRONG`) and what to do
/// (`hint`, `text_2`), centred.
pub fn empty_state<'a, M: 'a>(title: &'a str, hint: &'a str) -> Element<'a, M> {
    let content = column![
        typo::text(typo::body_strong(), title),
        typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::xs())
    .align_x(Alignment::Center)
    .max_width(size::form_max() / 2.0);
    center(content).padding(space::xl()).into()
}

// ---------------------------------------------------------------------------------------------
// Dots, avatars, dialogs
// ---------------------------------------------------------------------------------------------

/// A status for [`status_dot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Fine, connected, checked (green).
    Ok,
    /// Waiting, deviating, overdue (orange).
    Attention,
    /// Broken, refused (red-orange, not Torro red).
    Error,
    /// Not set up, inactive (grey).
    Inactive,
}

impl Status {
    fn colour(self, p: &Palette) -> Color {
        match self {
            Status::Ok => p.ok,
            Status::Attention => p.attention,
            Status::Error => p.error,
            Status::Inactive => p.inactive,
        }
    }
}

/// An 8-px dot in the colour of `status`. Colour is never the only sign: put the word next to it.
pub fn status_dot<'a, M: 'a>(status: Status) -> Element<'a, M> {
    let edge = Scale::Row.dot();
    container(blank().width(edge).height(edge))
        .style(move |t| container::Style {
            background: Some(Background::Color(status.colour(&palette(t)))),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// Overlapping round avatars (event guests): `people` as `(initials, colour)`, then "+`more`"
/// when `more > 0`. Each is 22 across with a `panel` ring, initials in `MICRO`.
pub fn avatar_stack<'a, M: 'a>(people: &[(&'a str, Color)], more: usize) -> Element<'a, M> {
    let avatar = |label: String, colour: Option<Color>| -> Element<'a, M> {
        container(typo::text(typo::micro(), label))
            .center(AVATAR)
            .style(move |t| {
                let p = palette(t);
                container::Style {
                    text_color: Some(p.panel),
                    background: Some(Background::Color(colour.unwrap_or(p.text_3))),
                    border: border::rounded(AVATAR / 2.0).color(p.panel).width(2),
                    ..container::Style::default()
                }
            })
            .into()
    };
    let mut faces: Vec<Element<'a, M>> =
        people.iter().map(|(initials, colour)| avatar((*initials).to_owned(), Some(*colour))).collect();
    if more > 0 {
        faces.push(avatar(format!("+{more}"), None));
    }
    let width = AVATAR + AVATAR_STEP * faces.len().saturating_sub(1) as f32;
    let layers = faces.into_iter().enumerate().map(|(i, face)| pin(face).x(AVATAR_STEP * i as f32).into());
    stack(std::iter::once(blank().width(width).height(AVATAR).into()).chain(layers)).into()
}

/// `#RRGGBB` (or `#RRGGBBAA`, alpha ignored) as a colour; `None` for anything else.
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
    container(blank().width(size).height(size))
        .style(move |_theme| container::Style {
            background: Some(Background::Color(color)),
            border: border::rounded(size / 2.0),
            ..Default::default()
        })
        .into()
}

/// A dialog over `base`: the background is dimmed with `scrim` (Iced has no backdrop blur, so
/// dimming does the job of focusing), the dialog gets the menu shadow and radius 10, and a click
/// next to it sends `on_dismiss`. The dialog draws its own surface (`style::container::popover`)
/// and is centred at most `max_width` wide; Escape is the app's job ([`escape`]).
pub fn modal<'a, M: Clone + 'a>(
    base: impl Into<Element<'a, M>>,
    dialog: impl Into<Element<'a, M>>,
    max_width: f32,
    on_dismiss: M,
) -> Element<'a, M> {
    let scrim = mouse_area(container(blank::horizontal()).width(Fill).height(Fill).style(|t| container::Style {
        background: Some(Background::Color(palette(t).scrim)),
        ..container::Style::default()
    }))
    .on_press(on_dismiss);
    let card = container(dialog).max_width(max_width).style(|t| container::Style {
        border: border::rounded(radius::lg()),
        shadow: palette(t).shadow,
        ..container::Style::default()
    });
    stack![base.into(), opaque(scrim), center(opaque(card)).padding(space::l())].into()
}

// ---------------------------------------------------------------------------------------------
// Charts and figures (task E8.8b, the hub's "Insights")
// ---------------------------------------------------------------------------------------------

/// The gap between stacked segments and the least gap between columns: the surface shows between
/// fills, so neighbours never touch (dataviz: 2-px spacer).
const MARK_GAP: f32 = 2.0;
/// Edge of the colour swatch in a legend and a chart's tooltip.
const SWATCH: f32 = 8.0;
/// Width of a chart's tooltip card, so its numbers stand in one column.
const CHART_TIP_WIDTH: f32 = 176.0;
/// Size of a [`share_bar`]: as wide as a [`progress_bar`].
const SHARE_HEIGHT: f32 = 4.0;

/// The height of the plot of a [`column_chart`]: five list rows (200 in the standard design).
fn chart_height() -> f32 {
    size::row() * 5.0
}

/// The widest a column's bar gets in a [`column_chart`] with few columns, so a week does not draw
/// slabs (44 in the standard design).
fn bar_max() -> f32 {
    size::control_lg() + space::s()
}

/// One series of a [`column_chart`]: its `name` (legend, tooltip) and the colour it wears. Give
/// colours by what the series is, never by its rank ([`ChartColor`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartSeries {
    pub name: String,
    pub color: ChartColor,
}

/// One column of a [`column_chart`]: a day, a week, an hour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartColumn {
    /// Under the column on the axis, short ("Mon", "13", "7 Oct", "9"). The chart leaves out as many
    /// labels as it must so that none touch, always keeping the last column's.
    pub label: String,
    /// The head of the column's tooltip, whole ("Monday, 13 Oct", "09:00–10:00").
    pub title: String,
    /// One value per series of the chart, in the order of its series; drawn as a stack from the
    /// bottom up. Missing values count as 0.
    pub values: Vec<u32>,
}

impl ChartColumn {
    /// All values together: the height of the stack.
    pub fn total(&self) -> u32 {
        self.values.iter().fold(0u32, |sum, value| sum.saturating_add(*value))
    }
}

/// The values on the y-axis of a chart whose largest column is `max`: from 0 in steps of 1, 2 or 5
/// times a power of ten, at most four steps, the last one at or above `max`. `[0, 1]` for an empty
/// chart, so the axis still has a top.
pub fn axis_ticks(max: u32) -> Vec<u32> {
    if max == 0 {
        return vec![0, 1];
    }
    let mut power: u32 = 1;
    loop {
        for factor in [1, 2, 5] {
            let step = factor * power;
            if max.div_ceil(step) <= 4 {
                return (0..=max.div_ceil(step)).map(|i| i * step).collect();
            }
        }
        power = power.saturating_mul(10);
    }
}

/// Which column labels a chart of `count` columns, `cell` wide each, shows when the widest label is
/// `label` wide: every one that fits with a gap, counted back from the last, so the newest column
/// is always named.
pub fn shown_labels(count: usize, cell: f32, label: f32) -> Vec<bool> {
    let need = label + space::s();
    let every = if cell <= 0.0 { count.max(1) } else { ((need / cell).ceil() as usize).max(1) };
    (0..count).map(|i| (count - 1 - i).is_multiple_of(every)).collect()
}

/// A column chart (task E8.8b; the dataviz method): one column per entry of `columns`, each a stack
/// of its `series` values from the bottom up, in their colours ([`ChartColor`]), with a 2-px gap of
/// surface between segments and the top corners of a stack rounded ([`radius::xs`]). One series
/// makes a plain column chart.
///
/// - **Axes:** gridlines in `line` at [`axis_ticks`] with their values at the left (`MICRO`,
///   tabular, `text_3`), the baseline in `line_strong`; the column labels below, as many as fit
///   ([`shown_labels`]).
/// - **Hover:** the whole height of a column is its hit area; it shows a tooltip at once with the
///   column's `title`, its total and each series with its swatch and value (the top of the stack
///   first). Text stays in the text colours, only the swatches wear the series' colour.
/// - **Size:** as wide as it is given, [`chart_height`] plus the axis labels high; bars at most
///   [`bar_max`] wide. Put a [`chart_legend`] over it for two series or more, and offer the same
///   numbers as a [`data_table`] (identity must never hang on colour alone).
///
/// ```ignore
/// let series = vec![ChartSeries { name: tr("Mail").into(), color: ChartColor::Series(0) }];
/// let columns = days.iter().map(|day| ChartColumn { label: day.short.clone(), title: day.long.clone(), values: vec![day.mail] });
/// column![widgets::chart_legend(&series), widgets::column_chart(series, columns.collect())]
/// ```
pub fn column_chart<'a, M: 'a>(series: Vec<ChartSeries>, columns: Vec<ChartColumn>) -> Element<'a, M> {
    let axis = typo::micro().tabular();
    let ticks = axis_ticks(columns.iter().map(ChartColumn::total).max().unwrap_or(0));
    let top = ticks.last().copied().unwrap_or(1).max(1);
    let half = typo::line_height(axis) / 2.0;
    let labels_width = ticks.iter().map(|tick| typo::text_width(&tick.to_string(), axis)).fold(0.0, f32::max);
    let height = chart_height() + 2.0 * half + space::xs() + typo::line_height(axis);
    responsive(move |room| {
        let plot_width = (room.width - labels_width - space::s()).max(0.0);
        let count = columns.len().max(1);
        let gap = column_gap(count, plot_width);
        let cell = (plot_width - gap * (count - 1) as f32) / count as f32;
        let y_axis = y_labels(&ticks, top, labels_width, half);
        let plot = stack![gridlines(&ticks), bars(&series, &columns, top, gap)].width(Fill).height(chart_height());
        let chart = row![y_axis, container(plot).padding([half, 0.0]).width(Fill)].spacing(space::s());
        let longest = columns.iter().map(|c| typo::text_width(&c.label, axis)).fold(0.0, f32::max);
        let shown = shown_labels(columns.len(), cell, longest);
        let x_axis = x_labels(&columns, &shown, cell, gap, plot_width);
        let x_axis = row![blank().width(labels_width), x_axis].spacing(space::s());
        column![chart, x_axis].spacing(space::xs()).into()
    })
    .height(height)
    .into()
}

/// The gap between columns: wider where there is room (a week), down to the 2-px spacer for many
/// columns in a narrow chart.
fn column_gap(count: usize, width: f32) -> f32 {
    if width / count as f32 >= bar_max() { space::s() } else { MARK_GAP }
}

/// The values of the y-axis, right-aligned, each centred on its gridline.
fn y_labels<'a, M: 'a>(ticks: &[u32], top: u32, width: f32, half: f32) -> Element<'a, M> {
    let axis = typo::micro().tabular();
    let layers = ticks.iter().map(|&tick| {
        let y = chart_height() * (1.0 - tick as f32 / top as f32);
        let label = typo::text(axis, tick.to_string()).style(theme::text_3).wrapping(Wrapping::None);
        pin(container(label).width(width).align_right(width)).y(y).into()
    });
    let base = blank().width(width).height(chart_height() + 2.0 * half);
    stack(std::iter::once(base.into()).chain(layers)).into()
}

/// The gridlines at `ticks`, the lowest (the baseline) stronger.
fn gridlines<'a, M: 'a>(ticks: &[u32]) -> Element<'a, M> {
    let steps = ticks.len().saturating_sub(1).max(1);
    let between = (chart_height() - LINE * (steps + 1) as f32) / steps as f32;
    let mut lines = column![];
    for i in 0..=steps {
        if i > 0 {
            lines = lines.push(blank().height(between));
        }
        let strong = i == steps;
        lines = lines.push(container(blank().width(Fill).height(LINE)).style(move |t| {
            let p = palette(t);
            container::background(if strong { p.line_strong } else { p.line })
        }));
    }
    lines.width(Fill).into()
}

/// The columns side by side, each with its tooltip over its whole height.
fn bars<'a, M: 'a>(series: &[ChartSeries], columns: &[ChartColumn], top: u32, gap: f32) -> Element<'a, M> {
    let cells = columns.iter().map(|column| {
        let stack = bar_stack(series, column, top);
        let cell = container(stack).center_x(Fill).align_bottom(chart_height());
        let tip = chart_tip(series, column);
        tooltip(cell, tip, tooltip::Position::Top).gap(space::xs()).into()
    });
    row(cells).spacing(gap).width(Fill).height(chart_height()).into()
}

/// One column's stack: its non-zero values from the bottom up, each as high as its share of `top`,
/// the gaps taken from the upper segments so the stack keeps its height.
fn bar_stack<'a, M: 'a>(series: &[ChartSeries], column: &ChartColumn, top: u32) -> Element<'a, M> {
    let filled: Vec<(ChartColor, u32)> = series
        .iter()
        .zip(column.values.iter().copied().chain(std::iter::repeat(0)))
        .filter(|(_, value)| *value > 0)
        .map(|(series, value)| (series.color, value))
        .collect();
    let last = filled.len().saturating_sub(1);
    let mut stack = column![].spacing(MARK_GAP).width(Fill).max_width(bar_max());
    // Top first: a column lays its children out from the top.
    for (i, (colour, value)) in filled.iter().copied().enumerate().rev() {
        let full = chart_height() * value as f32 / top as f32;
        let height = if i == 0 { full } else { full - MARK_GAP }.max(LINE);
        let rounded = i == last;
        stack = stack.push(container(blank().width(Fill).height(height)).style(move |t| {
            let radius = if rounded { radius::xs() } else { 0.0 };
            container::Style {
                background: Some(Background::Color(palette(t).chart_color(colour))),
                border: border::rounded(border::top(radius)),
                ..container::Style::default()
            }
        }));
    }
    stack.into()
}

/// The tooltip of a column: its title and total, then each series with swatch and value, the top
/// of the stack first (one series: the title and its value only).
fn chart_tip<'a, M: 'a>(series: &[ChartSeries], column: &ChartColumn) -> Element<'a, M> {
    let head = row![
        typo::text(typo::body_strong(), column.title.clone()).width(Fill),
        typo::text(typo::body_strong().tabular(), column.total().to_string()),
    ]
    .spacing(space::s());
    let mut lines = column![head].spacing(space::xs());
    if series.len() > 1 {
        for (i, series) in series.iter().enumerate().rev() {
            let value = column.values.get(i).copied().unwrap_or(0);
            lines = lines.push(
                row![
                    swatch(series.color),
                    typo::text(typo::meta(), series.name.clone()).style(theme::text_2).width(Fill),
                    typo::text(typo::meta().tabular(), value.to_string()),
                ]
                .spacing(ICON_GAP)
                .align_y(Alignment::Center),
            );
        }
    }
    container(lines).width(CHART_TIP_WIDTH).padding([space::s(), space::m()]).style(style::container::menu).into()
}

/// The labels under the columns: `shown` ones centred under their column, `cell` wide with `gap`
/// between, in a strip `width` wide.
fn x_labels<'a, M: 'a>(columns: &[ChartColumn], shown: &[bool], cell: f32, gap: f32, width: f32) -> Element<'a, M> {
    let axis = typo::micro().tabular();
    let layers = columns.iter().zip(shown).enumerate().filter(|(_, (_, shown))| **shown).map(|(i, (column, _))| {
        let label_width = typo::text_width(&column.label, axis) + space::xs();
        let centre = i as f32 * (cell + gap) + cell / 2.0;
        let x = (centre - label_width / 2.0).clamp(0.0, (width - label_width).max(0.0));
        let label = typo::text(axis, column.label.clone()).style(theme::text_3).wrapping(Wrapping::None);
        pin(container(label).width(label_width).center_x(label_width)).x(x).into()
    });
    let base = blank().width(Fill).height(typo::line_height(axis));
    stack(std::iter::once(base.into()).chain(layers)).width(Fill).into()
}

/// The square of a series' colour in a legend or tooltip.
fn swatch<'a, M: 'a>(colour: ChartColor) -> Element<'a, M> {
    container(blank().width(SWATCH).height(SWATCH))
        .style(move |t| container::Style {
            background: Some(Background::Color(palette(t).chart_color(colour))),
            border: border::rounded(MARK_GAP),
            ..container::Style::default()
        })
        .into()
}

/// The legend of a chart: each series' swatch and name (`META`, `text_2`), in the series' order,
/// wrapping onto more lines where it must. For two series or more; one series needs none (the
/// chart's title names it).
pub fn chart_legend<'a, M: 'a>(series: &[ChartSeries]) -> Element<'a, M> {
    let entries = series.iter().map(|series| {
        row![swatch(series.color), typo::text(typo::meta(), series.name.clone()).style(theme::text_2)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center)
            .into()
    });
    row(entries).spacing(space::m()).wrap().vertical_spacing(space::xs()).into()
}

/// The frame of a chart or a table of figures (task E8.8b): `title` (`BODY_STRONG`) with a quiet
/// `note` at its right (`META`, `text_3`: "average, Mon–Fri", "by day"), the `body` below. On
/// `panel` – the surface the chart colours were checked on – with a `line_strong` frame, the card
/// radius and a panel's padding, as wide as it is given.
pub fn chart_card<'a, M: 'a>(
    title: impl IntoFragment<'a>,
    note: impl IntoFragment<'a>,
    body: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let head = row![
        typo::text(typo::body_strong(), title).width(Fill).wrapping(Wrapping::None),
        typo::text(typo::meta(), note).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    container(column![head, body.into()].spacing(space::l()))
        .width(Fill)
        .padding(layout::panel_padding())
        .style(|t| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.panel)),
                text_color: Some(p.text),
                border: border::rounded(radius::lg()).color(p.line_strong).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// A figure on its own (task E8.8b, "Insights"): `label` (`META`, `text_3`) over the `value` in
/// `DISPLAY` with tabular figures, and a `note` (`META`, `text_2`) that says what it means ("12 a
/// day"). On `raised` with a `line_strong` frame and the card radius, as wide as it is given.
///
/// ```ignore
/// row![widgets::stat_tile(tr("Notifications"), "372", tr("53 a day"))].spacing(space::m())
/// ```
pub fn stat_tile<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    value: impl IntoFragment<'a>,
    note: impl IntoFragment<'a>,
) -> Element<'a, M> {
    let content = column![
        typo::text(typo::meta(), label).style(theme::text_3).wrapping(Wrapping::None),
        typo::text(typo::display().tabular(), value).wrapping(Wrapping::None),
        typo::text(typo::meta(), note).style(theme::text_2),
    ]
    .spacing(space::xs());
    container(content)
        .width(Fill)
        .padding([space::m(), space::l()])
        .style(|t| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.raised)),
                text_color: Some(p.text),
                border: border::rounded(radius::lg()).color(p.line_strong).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// A share as a quiet bar: `part` of `whole` filled in `text_3` on a `line_strong` track, as wide
/// as a [`progress_bar`]. Neutral – it is a measure, not a status; put the number next to it
/// ("3 %").
pub fn share_bar<'a, M: 'a>(part: u32, whole: u32) -> Element<'a, M> {
    let fraction = if whole == 0 { 0.0 } else { (part as f32 / whole as f32).clamp(0.0, 1.0) };
    let fill = container(blank().width(PROGRESS_WIDTH * fraction).height(SHARE_HEIGHT)).style(|t| container::Style {
        background: Some(Background::Color(palette(t).text_3)),
        border: border::rounded(SHARE_HEIGHT / 2.0),
        ..container::Style::default()
    });
    container(fill)
        .width(PROGRESS_WIDTH)
        .height(SHARE_HEIGHT)
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).line_strong)),
            border: border::rounded(SHARE_HEIGHT / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A table of figures (task E8.8b, the "Table view" of a chart): `head` as captions (`CAPTION`,
/// `text_3`), then one row per entry of `rows`, and – with `total` – a last row in `BODY_STRONG`.
/// The first column is the name (`BODY`, wider), the others are numbers, right-aligned with
/// tabular figures. Rows are [`size::control`] high with a `line` between them; every row should
/// have as many cells as `head`.
pub fn data_table<'a, M: 'a>(head: Vec<String>, rows: Vec<Vec<String>>, total: Option<Vec<String>>) -> Element<'a, M> {
    let line = |cells: Vec<String>, level: typo::Level, colour: fn(&Theme) -> iced::widget::text::Style| {
        let cells = cells.into_iter().enumerate().map(move |(i, cell)| {
            if i == 0 {
                container(typo::text(level, cell).style(colour).wrapping(Wrapping::None))
                    .width(Length::FillPortion(2))
                    .clip(true)
                    .into()
            } else {
                let number = typo::text(level.tabular(), cell).style(colour).wrapping(Wrapping::None);
                container(number).width(Fill).align_right(Fill).into()
            }
        });
        container(row(cells).spacing(space::s()).align_y(Alignment::Center))
            .center_y(size::control())
            .padding([0.0, space::s()])
    };
    let mut table = column![line(head, typo::caption(), theme::text_3)];
    let mut lines: Vec<(Vec<String>, typo::Level)> = rows.into_iter().map(|row| (row, typo::body())).collect();
    if let Some(total) = total {
        lines.push((total, typo::body_strong()));
    }
    for (cells, level) in lines {
        table = table.push(rule::horizontal(LINE).style(style::rule));
        table = table.push(line(cells, level, theme::text));
    }
    table.width(Fill).into()
}

#[cfg(test)]
mod tests {
    /// Task E12.7: the four group colours stand out on the panel in both modes, stay apart from
    /// one another, and there is one per group – no more.
    #[test]
    fn the_category_colours_are_visible_and_distinct() {
        let standard = torro_core::design::Design::standard();
        for (mode, p) in [("light", &standard.colors.light), ("dark", &standard.colors.dark)] {
            assert_eq!(p.category.len(), 4, "one colour per group, not per category");
            for (at, colour) in p.category.iter().enumerate() {
                let ratio = colour.contrast(p.panel);
                assert!(ratio >= 3.0, "{mode} category[{at}] is {ratio:.2}:1 on the panel");
            }
            for (at, colour) in p.category.iter().enumerate() {
                for (other, second) in p.category.iter().enumerate().skip(at + 1) {
                    assert_ne!(colour, second, "{mode}: category[{at}] reads like category[{other}]");
                }
            }
        }
    }
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Go,
        Close,
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle { x, y, width, height }
    }

    /// Task E9.4: a floating bar is its rows of 32-high controls with the gaps, padding and frame; the
    /// page under it keeps room for it and its margins; its room is the page less margins and frame.
    #[test]
    fn a_floating_bar_is_as_high_as_its_rows() {
        let frame = 2.0 * (floating_bar_gap() + LINE);
        assert_eq!(floating_bar_height(1), size::control() + frame);
        assert_eq!(floating_bar_height(2), 2.0 * size::control() + floating_bar_gap() + frame);
        assert_eq!(floating_bar_height(0), floating_bar_height(1), "a bar has a row at least");
        assert_eq!(floating_bar_clearance(2), floating_bar_height(2) + 2.0 * space::m());
        assert_eq!(floating_bar_room(800.0), 800.0 - 2.0 * space::m() - frame);
        assert_eq!(floating_bar_room(10.0), 0.0);
        assert!(size::control() > size::control_sm(), "a bar's buttons are a step taller");
        let words: Element<'_, Msg> = bar_button(Icon::Reply, "Reply", true, style::button::primary, Some(Msg::Go));
        let icon: Element<'_, Msg> =
            bar_button_accent(Icon::Sparkles, "AI task", false, style::button::secondary, None);
        let switch: Element<'_, Msg> = bar_segmented([("HTML", Msg::Go), ("Text", Msg::Close)], 0);
        let bar: Element<'_, Msg> = floating_bar(row![words, icon, switch]);
        let _: Element<'_, Msg> = over_bar(blank(), bar);
    }

    /// Task U.13: a segmented control is as wide as its words with their padding and the frame.
    #[test]
    fn a_segmented_control_grows_with_its_words() {
        let one = segmented_width(&[tr("Read")]);
        let two = segmented_width(&[tr("Read"), tr("Write")]);
        assert!(one > typo::text_width(tr("Read"), typo::caption()));
        assert!(two > one);
        let levels = [Level::NoAccess, Level::Read, Level::Write, Level::Full].map(level_label);
        let confirm = confirm_options().map(|(label, _)| label);
        assert!(segmented_width(&levels) < size::content_max() / 2.0, "leaves room for a name");
        assert!(segmented_width(&confirm) < size::content_max() / 2.0, "leaves room for a name");
    }

    /// Task U.13: the controls stand in a column while the longest name and line fit beside it, and
    /// on lines of their own once they would not – a long line alone decides it too.
    #[test]
    fn the_column_holds_while_the_texts_fit_beside_it() {
        let slot = segmented_width(&[tr("None"), tr("Read"), tr("Write"), tr("Full")]);
        let texts = [(tr("Calendar"), Some(tr("Events and calendars"))), (tr("Ask first"), None)];
        let text = typo::text_width(tr("Events and calendars"), typo::meta())
            .max(typo::text_width(tr("Calendar"), typo::body()))
            .max(typo::text_width(tr("Ask first"), typo::body()));
        let room = 3.0 * space::l() + text + slot;
        assert_eq!(segment_column(slot, texts, room), Some(slot));
        assert_eq!(segment_column(slot, texts, room - 1.0), None);
        assert_eq!(segment_column(slot, texts, size::content_max()), Some(slot));
        let long = "A line much longer than any name, as a translation might make it, so that it cannot fit";
        assert_eq!(segment_column(slot, [(tr("Calendar"), Some(long))], size::form_max()), None);
    }

    /// Task E8.8b: the y-axis ends at or above the largest column in at most four round steps.
    #[test]
    fn axis_ticks_are_round_and_cover_the_largest_column() {
        assert_eq!(axis_ticks(0), [0, 1], "an empty chart still has a top");
        assert_eq!(axis_ticks(3), [0, 1, 2, 3]);
        assert_eq!(axis_ticks(7), [0, 2, 4, 6, 8]);
        assert_eq!(axis_ticks(74), [0, 20, 40, 60, 80]);
        assert_eq!(axis_ticks(90), [0, 50, 100]);
        assert_eq!(axis_ticks(1200), [0, 500, 1000, 1500]);
        for max in [1, 9, 11, 49, 101, 999, 12_345] {
            let ticks = axis_ticks(max);
            assert!(ticks.len() <= 5 && ticks.last().is_some_and(|&top| top >= max), "{max}: {ticks:?}");
        }
    }

    /// Task E8.8b: labels that would touch are left out, counted back from the newest column.
    #[test]
    fn column_labels_thin_out_from_the_end() {
        assert_eq!(shown_labels(3, 60.0, 20.0), [true, true, true], "room for all");
        assert_eq!(shown_labels(5, 10.0, 12.0), [true, false, true, false, true], "every second");
        let month = shown_labels(30, 14.0, 12.0);
        assert!(month[29] && !month[28], "the last one always, its neighbour not");
        assert_eq!(month.iter().filter(|&&shown| shown).count(), 15);
        let column = ChartColumn { label: "Mon".into(), title: "Monday".into(), values: vec![3, 0, 4] };
        assert_eq!(column.total(), 7);
    }

    const WINDOW: Rectangle = Rectangle { x: 0.0, y: 0.0, width: 900.0, height: 700.0 };
    fn menu_size() -> Size {
        Size { width: menu_width(), height: 300.0 }
    }

    fn press(key: &str, code: keyboard::key::Code, modifiers: keyboard::Modifiers, text: Option<&str>) -> iced::Event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character(key.into()),
            modified_key: keyboard::Key::Character(key.into()),
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        })
    }

    /// Whether the guard captures `event`, asked the way the canvas widget does.
    fn captured(event: &iced::Event) -> bool {
        let action = canvas::Program::<Msg>::update(&CtrlGuard, &mut (), event, WINDOW, mouse::Cursor::Unavailable);
        action.is_some_and(|action| action.into_inner().2 == iced::event::Status::Captured)
    }

    #[test]
    fn ctrl_with_a_character_is_not_typed() {
        use keyboard::key::Code;
        // The platform's command key: Ctrl, on macOS Cmd (what `Modifiers::command` asks for).
        let ctrl = keyboard::Modifiers::COMMAND;
        // The zoom keys that left "a+--" in a field.
        assert!(captured(&press("+", Code::Equal, ctrl | keyboard::Modifiers::SHIFT, Some("+"))));
        assert!(captured(&press("=", Code::Equal, ctrl, Some("="))));
        assert!(captured(&press("-", Code::Minus, ctrl, Some("-"))));
        assert!(captured(&press("0", Code::Digit0, ctrl, Some("0"))));
    }

    #[test]
    fn typing_copy_paste_and_editing_keys_stay() {
        use keyboard::key::Code;
        let ctrl = keyboard::Modifiers::CTRL;
        let none = keyboard::Modifiers::empty();
        // Plain and shifted typing.
        assert!(!captured(&press("+", Code::Equal, keyboard::Modifiers::SHIFT, Some("+"))));
        assert!(!captured(&press("a", Code::KeyA, none, Some("a"))));
        // AltGr: Windows reports Ctrl + Alt, and the key types `@` or `€`.
        assert!(!captured(&press("q", Code::KeyQ, ctrl | keyboard::Modifiers::ALT, Some("@"))));
        assert!(!captured(&press("e", Code::KeyE, ctrl | keyboard::Modifiers::ALT, Some("€"))));
        // ⌥⌘F on a Mac (Apple Mail's search, task E10.1b): ⌥ makes "ƒ", which the field must not type.
        let alt_cmd = keyboard::Modifiers::LOGO | keyboard::Modifiers::ALT;
        assert_eq!(captured(&press("f", Code::KeyF, alt_cmd, Some("ƒ"))), cfg!(target_os = "macos"));
        // Copy, cut, paste and select all, also on a Cyrillic layout (the key is `с`, the code `KeyC`).
        for (key, code, text) in [("c", Code::KeyC, "\u{3}"), ("x", Code::KeyX, "\u{18}"), ("v", Code::KeyV, "v")] {
            assert!(!captured(&press(key, code, ctrl, Some(text))), "{key}");
            assert!(!captured(&press(key, code, keyboard::Modifiers::COMMAND, Some(key))), "Cmd + {key}");
        }
        assert!(!captured(&press("a", Code::KeyA, keyboard::Modifiers::COMMAND, Some("a"))), "Cmd + a on macOS");
        assert!(!captured(&press("с", Code::KeyC, keyboard::Modifiers::COMMAND, Some("с"))), "Cyrillic с");
        // Ctrl + letter sends a control character; the app shortcuts and the field get it as before.
        assert!(!captured(&press("n", Code::KeyN, ctrl, Some("\u{e}"))));
        assert!(!captured(&press("n", Code::KeyN, ctrl, None)));
        // Named keys (arrows, Backspace with Ctrl for the word before) are not characters.
        let arrow = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            modified_key: keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            physical_key: keyboard::key::Physical::Code(Code::ArrowLeft),
            location: keyboard::Location::Standard,
            modifiers: ctrl,
            text: None,
            repeat: false,
        });
        assert!(!captured(&arrow));
        // The sidebar key stays with the app.
        assert!(!captured(&press("[", Code::BracketLeft, ctrl, Some("["))));
        // Releasing a key and the mouse are none of its business.
        let release = iced::Event::Keyboard(keyboard::Event::ModifiersChanged(ctrl));
        assert!(!captured(&release));
        assert!(!captured(&iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::ORIGIN })));
    }

    #[test]
    fn parses_hex_colours() {
        assert_eq!(color_from_hex("#D50C0C"), Some(Color::from_rgb8(0xD5, 0x0C, 0x0C)));
        assert_eq!(color_from_hex("#0082c9FF"), Some(Color::from_rgb8(0x00, 0x82, 0xC9)));
        assert_eq!(color_from_hex("red"), None);
        assert_eq!(color_from_hex("#12345"), None);
    }

    #[test]
    fn label_colours_read_with_or_without_the_hash() {
        let red = Some(Color::from_rgb8(0xD7, 0x3A, 0x4A));
        assert_eq!(label_colour("#D73A4A"), red);
        assert_eq!(label_colour("d73a4a"), red, "GitHub sends labels without the #");
        assert_eq!(label_colour(" d73a4a "), red);
        assert_eq!(label_colour(""), None);
        assert_eq!(label_colour("red"), None);
    }

    #[test]
    fn progress_is_the_share_done_and_never_leaves_the_track() {
        assert_eq!(progress_fraction(0, 0), 0.0, "no sub-tasks: an empty track, not NaN");
        assert_eq!(progress_fraction(0, 3), 0.0);
        assert_eq!(progress_fraction(2, 4), 0.5);
        assert_eq!(progress_fraction(4, 4), 1.0);
        assert_eq!(progress_fraction(5, 4), 1.0, "more done than total stays inside 64 px");
        assert_eq!((PROGRESS_WIDTH, PROGRESS_HEIGHT), (64.0, 4.0));
    }

    #[test]
    fn a_menu_below_starts_under_the_left_edge_of_its_trigger() {
        let at = place(rect(60.0, 8.0, 90.0, 28.0), menu_size(), WINDOW, Placement::Below(Align::Start));
        assert_eq!(at, Point::new(60.0, 8.0 + 28.0 + below_gap()));
    }

    #[test]
    fn an_end_aligned_menu_ends_under_the_right_edge_of_its_trigger() {
        let at = place(rect(700.0, 8.0, 76.0, 28.0), menu_size(), WINDOW, Placement::Below(Align::End));
        assert_eq!(at.x + menu_size().width, 776.0);
    }

    #[test]
    fn a_card_beside_flips_left_when_there_is_no_room_on_the_right() {
        let card = Size::new(POPOVER_WIDTH, 240.0);
        let right = place(rect(100.0, 200.0, 120.0, 60.0), card, WINDOW, Placement::Beside);
        assert_eq!(right, Point::new(100.0 + 120.0 + beside_gap(), 200.0));
        let left = place(rect(600.0, 200.0, 120.0, 60.0), card, WINDOW, Placement::Beside);
        assert_eq!(left, Point::new(600.0 - beside_gap() - POPOVER_WIDTH, 200.0));
    }

    #[test]
    fn cards_stay_inside_the_window() {
        // A trigger at the right edge near the bottom with a start-aligned menu: moved in from the
        // edge, and above the trigger, which has no room below (task E8.12).
        let at = place(rect(850.0, 680.0, 40.0, 16.0), menu_size(), WINDOW, Placement::Below(Align::Start));
        assert_eq!(at, Point::new(900.0 - menu_size().width - beside_gap(), 680.0 - below_gap() - menu_size().height));
        // A card larger than the window keeps its top-left corner on screen.
        let huge = place(rect(10.0, 10.0, 10.0, 10.0), Size::new(2000.0, 2000.0), WINDOW, Placement::Beside);
        assert_eq!(huge, Point::new(beside_gap(), beside_gap()));
    }

    /// Task E8.12 (finding 4): a bar of buttons shows all words or none – only its lead may keep its
    /// word alone –, and the estimate takes the gap the buttons really have.
    #[test]
    fn a_bar_shows_all_its_words_or_none_but_its_lead() {
        let labels = ["Open", "Task", "Later"];
        let all = bar_width(&labels, BarWords::All, 1);
        let expected: f32 = labels.iter().map(|label| text_button_width(true, label)).sum::<f32>()
            + size::control_sm()
            + 3.0 * space::s();
        assert_eq!(all, expected);
        assert_eq!(text_button_width(true, "Open") - text_button_width(false, "Open"), icon::SIZE_SM + ICON_GAP);
        assert_eq!(bar_words(&labels, true, 1, all), BarWords::All, "exactly the room");
        let lead = bar_width(&labels, BarWords::Lead, 1);
        assert_eq!(bar_words(&labels, true, 1, all - 1.0), BarWords::Lead);
        assert_eq!(bar_words(&labels, false, 1, all - 1.0), BarWords::None, "no lead: no word at all");
        assert_eq!(bar_words(&labels, true, 1, lead - 1.0), BarWords::None);
        let words: Vec<bool> = (0..3).map(|i| BarWords::Lead.worded(i)).collect();
        assert_eq!(words, [true, false, false]);
        assert!((0..3).all(|i| BarWords::All.worded(i) && !BarWords::None.worded(i)));
    }

    /// Task E8.12 (finding 1): a meta line leaves out the account, then the sender, before it is cut.
    #[test]
    fn a_meta_line_leaves_out_parts_before_it_is_cut() {
        let parts: Vec<String> =
            ["Mail", "Jonas Weber", "anna@work.example.org", "Tue, 13 Oct"].map(String::from).to_vec();
        let level = typo::meta();
        let whole = "Mail · Jonas Weber · anna@work.example.org · Tue, 13 Oct";
        assert_eq!(meta_fit(&parts, &[2, 1], level, 1000.0), whole);
        let without_account = "Mail · Jonas Weber · Tue, 13 Oct";
        let room = typo::text_width(without_account, level);
        assert_eq!(meta_fit(&parts, &[2, 1], level, room), without_account);
        assert_eq!(meta_fit(&parts, &[2, 1], level, room - 1.0), "Mail · Tue, 13 Oct");
        assert_eq!(meta_fit(&parts, &[2, 1], level, 10.0), "Mail · Tue, 13 Oct", "the rest is cut by the line");
        let gaps = vec!["Mail".to_string(), String::new(), "Tue".to_string()];
        assert_eq!(meta_fit(&gaps, &[], level, 1000.0), "Mail · Tue", "no empty part");
    }

    /// Task E8.12 (finding 3): a card's action gives up its word before it pushes the title away.
    #[test]
    fn a_card_line_keeps_room_for_its_title() {
        let lead = size::avatar();
        let worded = text_button_width(true, "Open mail");
        let enough = lead + 2.0 * space::m() + worded + card_line_words_min() + 0.5;
        let (spoken, room) = card_line_split(enough, lead, "Open mail");
        assert!(spoken && (room - card_line_words_min() - 0.5).abs() < 0.01, "{room}");
        let (spoken, room) = card_line_split(enough - 1.0, lead, "Open mail");
        assert!(!spoken, "a medium window: the icon alone");
        assert!(room > card_line_words_min(), "the title gets the button's room: {room}");
        assert_eq!(card_line_split(10.0, lead, "Open mail"), (false, 0.0), "never below nothing");
    }

    /// Task E8.12 (finding 6): a menu opens above its trigger when only there is room, never over
    /// it; with room on neither side it takes the side with more and stays on screen.
    #[test]
    fn a_menu_without_room_below_opens_above_its_trigger() {
        let menu = Size::new(240.0, 260.0);
        let trigger = rect(600.0, 560.0, 28.0, 28.0);
        let at = place(trigger, menu, WINDOW, Placement::Below(Align::End));
        assert_eq!(at, Point::new(628.0 - 240.0, 560.0 - below_gap() - 260.0), "above, right edges in line");
        assert!(at.y + menu.height <= trigger.y, "the trigger stays free");
        let roomy = place(rect(600.0, 100.0, 28.0, 28.0), menu, WINDOW, Placement::Below(Align::End));
        assert_eq!(roomy.y, 128.0 + below_gap(), "below while it fits there");
        // A window 400 high: neither side has room for 300, the larger side wins, kept on screen.
        let small = Rectangle { height: 400.0, ..WINDOW };
        let tall = Size::new(240.0, 300.0);
        let low = place(rect(600.0, 300.0, 28.0, 28.0), tall, small, Placement::Below(Align::End));
        assert_eq!(low.y, beside_gap(), "more room above: from the top margin");
        let high = place(rect(600.0, 60.0, 28.0, 28.0), tall, small, Placement::Below(Align::End));
        assert_eq!(high.y, 400.0 - 300.0 - beside_gap(), "more room below: up from the bottom margin");
    }

    #[test]
    fn an_anchor_reports_what_its_trigger_recorded() {
        let anchor = Anchor::new();
        assert_eq!(anchor.get(), None, "nothing before the trigger is laid out");
        let shared = anchor.clone();
        shared.set(rect(40.0, 50.0, 300.0, 400.0));
        assert_eq!(anchor.get(), Some(rect(40.0, 50.0, 300.0, 400.0)), "clones share the cell");
        let event = anchor.area(rect(10.0, 96.0, 120.0, 48.0));
        assert_eq!(event.get(), Some(rect(50.0, 146.0, 120.0, 48.0)), "an area is relative to the trigger");
    }

    #[test]
    fn an_anchor_falls_back_while_its_trigger_is_missing() {
        let (button, row, window) = (Anchor::new(), Anchor::new(), Anchor::new());
        let at = button.or(&row).or(&window);
        assert_eq!(at.get(), None, "none laid out");
        window.set(rect(0.0, 0.0, 800.0, 600.0));
        assert_eq!(at.get(), Some(rect(0.0, 0.0, 800.0, 600.0)), "the last in the chain");
        row.set(rect(0.0, 100.0, 800.0, 88.0));
        assert_eq!(at.get(), Some(rect(0.0, 100.0, 800.0, 88.0)), "the row before the window");
        button.set(rect(700.0, 110.0, 28.0, 28.0));
        assert_eq!(at.get(), Some(rect(700.0, 110.0, 28.0, 28.0)), "the button itself first");
    }

    #[test]
    fn a_card_without_its_anchor_stands_in_the_middle() {
        let at = centre(Size::new(240.0, 200.0), rect(0.0, 0.0, 800.0, 600.0));
        assert_eq!(at, Point::new(280.0, 200.0));
    }

    #[test]
    fn a_row_action_registers_its_anchor_only_when_asked() {
        let action = RowAction::new(Icon::Clock, "Later", "S", Msg::Go);
        assert!(action.anchor.is_none());
        let anchor = Anchor::new();
        let action = action.at(&anchor);
        action.anchor.as_ref().expect("set").set(rect(1.0, 2.0, 3.0, 4.0));
        assert_eq!(anchor.get(), Some(rect(1.0, 2.0, 3.0, 4.0)), "shares the cell");
    }

    /// Issue #168: an action that is off takes no press, says why in its tooltip and still draws in
    /// the bar, with words and as icon, with and without a key.
    #[test]
    fn a_blocked_row_action_takes_no_press_and_says_why() {
        let on = RowAction::new(Icon::Sparkles, "AI task", "", Msg::Go);
        assert_eq!((on.press(), on.tip()), (Some(Msg::Go), "AI task".to_string()));
        let off = on.clone().blocked("Set up an AI first");
        assert_eq!(off.press(), None);
        assert_eq!(off.tip(), "AI task · Set up an AI first");
        assert_eq!(off.on_press, Msg::Go, "the message stays for when it is on again");
        for words in [true, false] {
            let bar = vec![off.clone(), RowAction::new(Icon::Clock, "Later", "S", Msg::Go)];
            let _: Element<'_, Msg> = row_actions(blank(), bar, true, words);
        }
        let _: Element<'_, Msg> = card_line(|| blank().into(), 28.0, "Title".into(), vec![], vec![], off);
    }

    /// Issue #168: a button that is off fades its icon with its word and frame. The icon's tint
    /// drops alpha, so the image's opacity does it.
    #[test]
    fn icons_of_buttons_that_are_off_fade() {
        assert_eq!(icon_opacity(true), 1.0);
        assert_eq!(icon_opacity(false), style::DISABLED_ALPHA);
        let _: Element<'_, Msg> = icon_button(Icon::Sparkles, "AI task", None);
        let _: Element<'_, Msg> = text_button(Some(Icon::Sparkles), "AI task", style::button::secondary, None);
    }

    #[test]
    fn menu_items_default_to_no_trail_and_not_current() {
        let item = MenuItem::new(Icon::Sun, "Today", Msg::Go);
        assert_eq!((item.trail, item.current), (Trail::None, false));
        let item = item.trail(Trail::Count(5)).current(true);
        assert_eq!((item.trail, item.current, item.on_press.clone()), (Trail::Count(5), true, Msg::Go));
        assert_eq!(MenuSection::titled("Lists", vec![item]).title.as_deref(), Some("Lists"));
    }

    #[test]
    fn marks_scale_with_their_place() {
        assert_eq!((Scale::Row.icon(), Scale::Row.ring(), Scale::Row.dot()), (16.0, 14.0, 8.0));
        assert_eq!((Scale::Small.icon(), Scale::Small.ring(), Scale::Small.dot()), (14.0, 12.0, 6.0));
        assert_eq!(Mark::from(Icon::Inbox), Mark::Icon(Icon::Inbox));
    }

    #[test]
    fn status_colours_come_from_the_palette() {
        let p = Palette::light();
        assert_eq!(Status::Ok.colour(&p), p.ok);
        assert_eq!(Status::Error.colour(&p), p.error);
        assert_ne!(Status::Error.colour(&p), p.accent, "errors are not Torro red");
    }

    #[test]
    fn building_blocks_build() {
        let p = Palette::dark();
        let anchor = Anchor::new();
        let blocks: Vec<Element<'_, Msg>> = vec![
            text_button(Some(Icon::Plus), "New task", style::button::primary, Some(Msg::Go)),
            icon_button(Icon::Search, "Search", None),
            boxed_button(Icon::SquarePen, "New", Some(Msg::Go)),
            compact_button(Icon::Plus, "New task", style::button::primary, Some(Msg::Go)),
            segmented([("Day", Msg::Go), ("Week", Msg::Go)], 1),
            sidebar_item(Icon::Sun, "Today", Trail::Count(5), true, Msg::Go),
            sidebar_item(Mark::Ring(p.calendar[0]), "Work", Trail::Kbd("W"), false, Msg::Go),
            sidebar_group("Lists", Some((true, Msg::Go))),
            list_row(vec![typo::text(typo::body(), "Review").into()], true, Some(Msg::Go)),
            group_head(Mark::Tinted(Icon::CircleAlert, p.attention), "Overdue", Some(2), Some(Msg::Go)),
            menu(vec![MenuSection::titled("View", vec![MenuItem::new(Icon::Calendar, "Week", Msg::Go)])]),
            menu_button(&anchor, Some(Icon::Sun), "Today", Msg::Go),
            select_button(&anchor, "Week", Msg::Go),
            floating(&anchor, Placement::Below(Align::End), blank(), Msg::Close),
            popover(
                blank(),
                "Sprint planning",
                vec![],
                vec![text_button(None, "Open", style::button::secondary, None)],
            ),
            chip_row(vec![chip(Some(Mark::Icon(Icon::Calendar)), "Today, 16:00", Some(Msg::Go))]),
            chip_row(vec![
                choice_chip("Bugs", Some(Mark::Icon(Icon::Bug)), true, Some(Msg::Go)),
                choice_chip("Me", None, false, Some(Msg::Go)),
                choice_chip("Work", Some(Mark::Dot(p.calendar[0])), true, None),
                label_choice("d73a4a", "bug", true, Some(Msg::Go)),
                label_choice("not a colour", "ui", false, None),
            ]),
            busy_button("Creating…", style::button::primary),
            pill(Some(p.calendar[1]), "Personal"),
            prop_row(Some(Mark::Icon(Icon::Bell)), typo::text(typo::body(), "15:45"), Some(Msg::Go)),
            prop_section("Properties", vec![]),
            prop_line("Due", Some(Mark::Icon(Icon::Calendar)), prop_value("Today, 16:00"), Some(Msg::Go)),
            prop_line("Reminder", Some(Mark::Icon(Icon::Bell)), prop_placeholder("Add reminder"), None),
            prop_line(
                "Labels",
                None,
                label_chip_row(vec![label_chip("#D73A4A", "bug"), label_chip("nope", "ui")]),
                None,
            ),
            state_pill(Mark::Tinted(Icon::GitMerge, p.merged), "Merged"),
            state_pill(Icon::GitPullRequestDraft, "Draft"),
            progress_bar(2, 4),
            kbd("N"),
            group("General", vec![group_row("Start at login", Some("Recommended".into()), blank())], None),
            content(size::content_max(), blank()),
            loading(),
            error_state("The server did not answer.", Msg::Go),
            empty_state("No tasks", "Add one with N."),
            status_dot(Status::Ok),
            avatar_stack(&[("AW", p.calendar[0])], 3),
            section_title("Calendars"),
            modal(blank(), blank(), size::form_max(), Msg::Close),
        ];
        assert!(!blocks.is_empty());
        let _: Subscription<()> = escape();
    }

    fn actions() -> Vec<RowAction<'static, Msg>> {
        vec![
            RowAction::new(Icon::ListTodo, "Task", "T", Msg::Go),
            RowAction::new(Icon::Check, "Done", "E", Msg::Go),
            RowAction::new(Icon::Clock, "Later", "S", Msg::Go),
            RowAction::new(Icon::Ellipsis, "Hide", "M", Msg::Close),
        ]
    }

    #[test]
    fn the_action_bar_measures_its_buttons() {
        let icons = row_actions_width(&actions(), false);
        // Four 28-px buttons, three gaps of 2 and the frame of 2 + 1 on each side.
        assert_eq!(icons, 4.0 * size::control_sm() + 3.0 * BAR_INSET + 2.0 * (BAR_INSET + LINE));
        let words = row_actions_width(&actions(), true);
        assert!(words > icons, "words take more room: {words} vs {icons}");
        let one = row_actions_width(&actions()[..1], true);
        assert_eq!(one, row_action_width("Task") + 2.0 * (BAR_INSET + LINE), "no gap for a single button");
    }

    #[test]
    fn words_show_only_while_the_bar_takes_at_most_half_the_row() {
        let words = row_actions_width(&actions(), true);
        assert!(row_actions_words(&actions(), 2.0 * words));
        assert!(!row_actions_words(&actions(), 2.0 * words - 1.0), "a narrow row gets icons");
    }

    #[test]
    fn the_action_bar_builds_shown_on_hover_and_without_actions() {
        for (shown, words) in [(true, true), (false, true), (true, false), (false, false)] {
            let _: Element<'_, Msg> = row_actions(list_row(vec![], shown, Some(Msg::Go)), actions(), shown, words);
        }
        let _: Element<'_, Msg> = row_actions(list_row(vec![], false, None), Vec::new(), true, true);
        let _: Element<'_, Msg> = card_actions(list_row(vec![], false, None), actions(), true, false, space::m());
        let _: Element<'_, Msg> = row_actions_at(list_row(vec![], true, None), actions(), true, true, space::xl());
        // A line nearer the top than half the bar keeps the bar inside the row.
        let _: Element<'_, Msg> = row_actions_at(list_row(vec![], false, None), actions(), false, false, 0.0);
        assert_eq!(row_actions_height(), size::control_sm() + 2.0 * (BAR_INSET + LINE));
    }

    #[test]
    fn a_toast_builds_with_and_without_its_button() {
        let _: Element<'_, Msg> = toast("Marked as done", Some(ToastAction::undo(Msg::Go)));
        let _: Element<'_, Msg> = toast("Couldn't open the app.", None);
        let undo = ToastAction::undo(Msg::Close);
        assert_eq!((undo.label.as_ref(), undo.key, undo.on_press), (tr("Undo"), Some(undo_hint()), Msg::Close));
        assert_eq!(TOAST_FOR, Duration::from_secs(5));
    }

    /// Task E10.1: on macOS Ctrl + Z undoes as ⌘ + Z does; elsewhere only Ctrl + Z.
    #[test]
    fn the_undo_key_is_cmd_or_ctrl_z_on_a_mac() {
        use keyboard::key::Code;
        let z = |system, modifiers| {
            is_undo_key_on(
                system,
                &keyboard::Key::Character("z".into()),
                keyboard::key::Physical::Code(Code::KeyZ),
                modifiers,
            )
        };
        let (ctrl, logo) = (keyboard::Modifiers::CTRL, keyboard::Modifiers::LOGO);
        assert!(z(System::Mac, logo) && z(System::Mac, ctrl), "⌘ Z and Ctrl Z");
        assert!(!z(System::Mac, ctrl | logo), "⌃⌘Z is another combination");
        for other in [System::Windows, System::Linux] {
            assert!(z(other, ctrl) && !z(other, logo), "Ctrl Z, not the logo key: {other:?}");
        }
        assert!(!z(System::Mac, ctrl | keyboard::Modifiers::SHIFT), "redo");
        let held = held(ctrl | keyboard::Modifiers::SHIFT);
        assert_eq!(held, Held { ctrl: true, cmd: false, alt: false, shift: true });
    }

    #[test]
    fn the_undo_key_is_ctrl_z_on_any_layout() {
        use keyboard::key::Code;
        let z = |key: &str, code: Code, modifiers: keyboard::Modifiers| {
            is_undo_key(&keyboard::Key::Character(key.into()), keyboard::key::Physical::Code(code), modifiers)
        };
        let command = keyboard::Modifiers::COMMAND;
        assert!(z("z", Code::KeyZ, command));
        assert!(z("z", Code::KeyY, command), "a German keyboard has its Z where the US one has Y");
        assert!(!z("y", Code::KeyZ, command), "and its Y is no Z");
        assert!(z("я", Code::KeyZ, command), "Cyrillic: the key in Z's place");
        assert!(!z("z", Code::KeyZ, keyboard::Modifiers::empty()), "plain z types");
        assert!(!z("z", Code::KeyZ, command | keyboard::Modifiers::SHIFT), "redo");
        assert!(!z("z", Code::KeyZ, command | keyboard::Modifiers::ALT), "AltGr");
        assert!(!z("x", Code::KeyX, command));
    }

    /// Task E11.1: the command bar shows its words only when every cell holds its icon and word in
    /// its equal share – the split's chevron and "…" included –, and not a pixel earlier.
    #[test]
    fn the_command_bar_shows_words_only_when_every_cell_holds_its_word() {
        let labels = ["Reply", "Forward", "Archive", "Delete", "Later"];
        let split = cell_need("Reply", typo::body_strong()) + LINE + command_menu_width();
        let widest = ["Forward", "Archive", "Delete", "Later"].map(|l| cell_need(l, typo::body())).into_iter();
        let share = widest.fold(split, f32::max);
        let exact = 5.0 * (share + LINE) + size::command();
        assert!(command_words(&labels, exact), "exactly the room");
        assert!(!command_words(&labels, exact - 1.0), "a pixel too little");
        // A long lead decides for all: its share is everyone's share.
        let long = ["Reply to everyone in this thread", "Forward"];
        let lead = cell_need(long[0], typo::body_strong()) + LINE + command_menu_width();
        let exact = 2.0 * (lead + LINE) + size::command();
        assert!(command_words(&long, exact) && !command_words(&long, exact - 1.0));
        assert!(command_words(&[], 0.0), "an empty bar has nothing to cut");
    }

    /// Task E11.1: the strip under a row shows its words only when every cell holds its word in its
    /// equal share, and is a cell high plus its line.
    #[test]
    fn the_row_strip_shows_words_only_when_every_cell_holds_its_word() {
        let actions = actions();
        let share = actions.iter().map(|a| cell_need(a.label.as_ref(), typo::body())).fold(0.0, f32::max);
        let exact = 4.0 * share + 3.0 * LINE;
        assert!(row_strip_words(&actions, exact), "exactly the room");
        assert!(!row_strip_words(&actions, exact - 1.0), "a pixel too little");
        assert!(row_strip_words::<Msg>(&[], 0.0));
        assert_eq!(row_strip_height(), size::control_sm() + LINE);
    }

    /// Task E11.1: an action marked accent keeps everything else; only the strip draws it red.
    #[test]
    fn an_accent_action_is_the_same_action_with_a_red_icon() {
        let plain = RowAction::new(Icon::ListTodo, "Task", "T", Msg::Go);
        assert!(!plain.accent);
        let red = plain.clone().accent();
        assert!(red.accent);
        assert_eq!((red.key, red.press(), red.tip()), (plain.key, plain.press(), plain.tip()));
        let off = RowAction::new(Icon::Sparkles, "AI task", "", Msg::Go).accent().blocked("Set up an AI first");
        assert!(off.accent && off.press().is_none(), "blocked keeps the accent");
    }

    /// Task E11.1: every block of the mail window builds with and without words, on and off.
    #[test]
    fn the_mail_blocks_build_in_every_variant() {
        for words in [true, false] {
            let cells: Vec<Element<'_, Msg>> = vec![
                command_split(Icon::Reply, "Reply", words, Some(Msg::Go), Some(Msg::Close)),
                command_cell(Icon::Forward, "Forward", words, false, Some(Msg::Go)),
                command_cell(Icon::Trash2, "Delete", words, false, None),
                command_cell(Icon::Clock, "Later", words, true, Some(Msg::Go)),
                command_more(Some(Msg::Close)),
            ];
            let _: Element<'_, Msg> = command_bar(cells);
            let mut strip = actions();
            strip[0] = strip[0].clone().accent();
            strip[1] = strip[1].clone().blocked("Not now");
            let _: Element<'_, Msg> = row_strip(strip, words);
        }
        let _: Element<'_, Msg> = row_strip(Vec::new(), true);
        let _: Element<'_, Msg> = command_split(Icon::Reply, "Reply", true, None, None);
        let _: Element<'_, Msg> = command_more(None);
        let _: Element<'_, Msg> = pill_button(Icon::ListTodo, "Task", Some(Msg::Go));
        let _: Element<'_, Msg> = pill_button(Icon::Sparkles, "AI task", None);
        let views = [(Icon::Code, "HTML", Msg::Go), (Icon::AlignLeft, "Text", Msg::Close)];
        let _: Element<'_, Msg> = view_switch(views, 1);
        let _: Element<'_, Msg> = file_chip("budget-q4.csv", "3 KB", Some(Msg::Go));
        let _: Element<'_, Msg> = file_chip("travel-policy.pdf", "", None);
        assert_eq!([size::command(), size::pill()], [46.0, 42.0], "the standard's bar and pills");
    }

    /// Task E8.2: the blocks of the comfortable density build in every variant, and the tiles take
    /// their sizes and letters from the tokens.
    #[test]
    fn cards_and_tiles_build() {
        let tile = source_tile(TileFace::letter("slack"), TileSize::Medium, None);
        let head = card_head(Some(tile), "#dev", "10:12", Some(Msg::Close));
        let chips = chip_row(vec![state_pill(Icon::Paperclip, "Angebot_v2.pdf")]);
        let _: Element<'_, Msg> = card(vec![head, chips], true, Some(Msg::Go));
        let _: Element<'_, Msg> = card(vec![card_head(None, "Jonas Brandt", "", None)], false, None);
        let _: Element<'_, Msg> = source_tile(TileFace::Icon(Icon::Github), TileSize::Large, Some(Color::WHITE));
        assert_eq!(TileFace::letter("slack"), TileFace::Letter('S'));
        assert_eq!(TileFace::letter("#dev-torro"), TileFace::Letter('D'), "the first letter, not the #");
        assert_eq!(TileFace::letter("#!"), TileFace::Letter('?'));
        let edges = [TileSize::Small, TileSize::Medium, TileSize::Large].map(TileSize::edge);
        assert_eq!(edges, [size::tile_sm(), size::tile(), size::tile_lg()]);
        assert_eq!(edges, [20.0, 28.0, 36.0], "the standard's tiles");
    }
}
