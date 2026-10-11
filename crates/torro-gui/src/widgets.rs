//! Building blocks of a Linear-style window: buttons, sidebar rows, list rows, group heads, menus,
//! floating cards, chips, property rows, settings groups and the empty, loading and error states.
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
    button, canvas, center, column, container, float, mouse_area, opaque, pin, row, rule, stack, svg, tooltip,
};
use iced::{Alignment, Background, Color, Element, Fill, Padding, Point, Rectangle, Size, Subscription, Theme, Vector};
use iced::{border, keyboard, mouse};

use torro_core::i18n::tr;

use crate::icon::{self, Icon, icon};
use crate::shell::{SidebarMode, is_sidebar_key};
use crate::style;
use crate::theme::{self, Palette, palette, radius, size, space};
use crate::typo;

/// A button style from [`crate::style::button`] (`style::button::primary`, `ghost` …).
pub type ButtonStyle = fn(&Theme, button::Status) -> button::Style;

/// Width of a menu card ([`menu`]): the sidebar width, as in the mock-ups.
pub const MENU_WIDTH: f32 = size::SIDEBAR;
/// Width of a floating card ([`popover`]).
pub const POPOVER_WIDTH: f32 = 320.0;
/// Gap between the anchor and a floating card below it.
const BELOW_GAP: f32 = space::XS;
/// Gap between the anchor and a floating card beside it, and the least margin to the window edge.
const BESIDE_GAP: f32 = space::S;
/// Gap between icon and text inside a button or chip (skill: 6 is allowed inside building blocks).
const ICON_GAP: f32 = 6.0;
/// Gap between the cells of a list row (`tasks.html`: `.row{gap:10px}`).
const ROW_GAP: f32 = 10.0;
/// The one-pixel lines of the design.
const LINE: f32 = size::HAIRLINE;
/// Height of a list pill ([`pill`]) and of its round ends.
const PILL_HEIGHT: f32 = 20.0;
/// Edge of an avatar in [`avatar_stack`] and how far the next one is moved right.
const AVATAR: f32 = 22.0;
const AVATAR_STEP: f32 = 20.0;
/// Edge of the chevron after a sidebar group title.
const GROUP_CHEVRON: f32 = 12.0;
/// Width of the bar that marks the selected list row.
const SELECTED_BAR: f32 = 2.0;

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
            Scale::Row => space::S,
            Scale::Small => ICON_GAP,
        }
    }
}

/// `which` in the colour `tone` picks from the current palette.
pub(crate) fn toned<'a, M: 'a>(which: Icon, edge: f32, tone: Tone) -> Element<'a, M> {
    icon(which, edge).style(move |t, _| svg::Style { color: Some(tone.of(palette(t))) }).into()
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
                    border: border::rounded(radius::XS).color(colour).width(style::CHECK_BORDER),
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
        Trail::Count(n) => Some(typo::text(typo::META.tabular(), n.to_string()).style(theme::text_3).into()),
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
fn button_icon<'a, M: 'a>(which: Icon, edge: f32, style: ButtonStyle) -> Element<'a, M> {
    icon(which, edge).style(move |t, _| svg::Style { color: Some(style(t, button::Status::Active).text_color) }).into()
}

/// A text button, 28 high ([`size::CONTROL_SM`]), label in `CAPTION`, optional icon (14) in front.
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
        content = content.push(button_icon(which, icon::SIZE_SM, style));
    }
    content = content.push(typo::text(typo::CAPTION, label).wrapping(Wrapping::None));
    button(container(content).center_y(Fill))
        .height(size::CONTROL_SM)
        .padding(style::BUTTON_PADDING)
        .style(style)
        .on_press_maybe(on_press)
        .into()
}

/// `content` with `label` as tooltip below it (a small menu-style card, after a short delay).
fn with_tooltip<'a, M: 'a>(content: impl Into<Element<'a, M>>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    tip(content, typo::text(typo::CAPTION, label).style(theme::text_2), tooltip::Position::Bottom)
}

/// `content` with `label` and its `trail` (count, key hint …) as tooltip at its right: the names of
/// the rows in the collapsed sidebar, which point into the content.
pub(crate) fn tooltip_right<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    label: impl IntoFragment<'a>,
    trail: Trail,
) -> Element<'a, M> {
    let text = row![typo::text(typo::CAPTION, label).style(theme::text_2).wrapping(Wrapping::None)]
        .push(trail_view(trail))
        .spacing(space::S)
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
        .padding([style::KBD_PADDING.top + style::KBD_PADDING.bottom, space::S])
        .style(style::container::menu);
    tooltip(content, card, position).gap(space::XS).delay(Duration::from_millis(500)).into()
}

/// An icon-only button in `style`, 28 high; `square` makes it 28 × 28, otherwise it keeps the
/// button padding (for an icon-only primary button in the narrow layout).
fn icon_only<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    style: ButtonStyle,
    square: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let edge = if square { icon::SIZE } else { icon::SIZE_SM };
    let b = if square {
        button(center(button_icon(which, edge, style))).width(size::CONTROL_SM).padding(0)
    } else {
        button(container(button_icon(which, edge, style)).center_y(Fill)).padding(style::BUTTON_PADDING)
    };
    let b = b.height(size::CONTROL_SM).style(style).on_press_maybe(on_press);
    with_tooltip(b, tooltip)
}

/// An icon button (‹ › refresh, filter …): 28 × 28, ghost, icon 16 in `text_2`, hover `hover`.
/// `tooltip` names the action (it is the only label, so it must be there). `None` disables it.
pub fn icon_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style::button::icon, true, on_press)
}

/// An icon button with a frame (`style::button::boxed`), 28 × 28: "New" next to the app switcher.
pub fn boxed_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style::button::boxed, true, on_press)
}

/// A text button reduced to its icon, for the narrow layout where the label does not fit (the
/// primary "+" in a narrow header). Same height and padding as [`text_button`]; `tooltip` keeps
/// the label.
pub fn compact_button<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    style: ButtonStyle,
    on_press: Option<M>,
) -> Element<'a, M> {
    icon_only(which, tooltip, style, false, on_press)
}

/// A segmented control (Day · Week · Month): one segment per `(label, message)`, the segment at
/// index `active` is chosen. Frame on `bg`, 28 high, labels `CAPTION`.
pub fn segmented<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (L, M)>,
    active: usize,
) -> Element<'a, M> {
    let inner = size::CONTROL_SM - 2.0 * style::SEGMENT_INSET - 2.0 * LINE;
    let segments = options.into_iter().enumerate().map(|(i, (label, message))| {
        button(container(typo::text(typo::CAPTION, label).wrapping(Wrapping::None)).center_y(Fill))
            .height(inner)
            .padding(style::BUTTON_PADDING)
            .style(style::button::segment(i == active))
            .on_press(message)
            .into()
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
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
            .height(size::ROW_SIDEBAR)
            .padding(padding)
            .style(style::button::sidebar_item(active))
            .on_press(on_press)
    };
    match mode {
        SidebarMode::Expanded => {
            let content = row![mark, line_label(typo::BODY, label)]
                .push(trail_view(trail))
                .spacing(space::S)
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
        return container(rule::horizontal(LINE).style(style::rule)).padding([space::S, space::XS]).into();
    }
    let mut title =
        row![typo::text(typo::CAPTION, label).style(theme::text_3)].spacing(space::XS).align_y(Alignment::Center);
    if let Some((expanded, _)) = &toggle {
        let chevron = if *expanded { Icon::ChevronDown } else { Icon::ChevronRight };
        title = title.push(toned(chevron, GROUP_CHEVRON, Tone::Text3));
    }
    let bar = container(title).height(size::ROW_SIDEBAR).padding(style::ITEM_PADDING).center_y(size::ROW_SIDEBAR);
    let bar: Element<'a, M> = match toggle {
        Some((_, message)) => mouse_area(bar).on_press(message).interaction(mouse::Interaction::Pointer).into(),
        None => bar.into(),
    };
    container(bar).padding(Padding::new(0.0).top(space::M)).into()
}

// ---------------------------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------------------------

/// A list row (a task, an agenda entry): 40 high, `cells` side by side with 10 between them,
/// 16 at the sides, a line below. Give the title cell `.width(Fill)` so the cells after it sit at
/// the right. The `selected` row (the one in the detail) is `selected` with a 2-px bar on the left.
///
/// ```ignore
/// widgets::list_row(vec![check.into(), priority, typo::text(typo::BODY, &task.title).width(Fill).into(), due], task.id == selected, Some(Message::Select(task.id)))
/// ```
pub fn list_row<'a, M: Clone + 'a>(cells: Vec<Element<'a, M>>, selected: bool, on_press: Option<M>) -> Element<'a, M> {
    let content = row(cells).spacing(ROW_GAP).align_y(Alignment::Center);
    let left = if selected { space::L - SELECTED_BAR } else { space::L };
    let body = button(container(content).center_y(Fill))
        .width(Fill)
        // 40 including the line below, as the CSS's `border-box`.
        .height(size::ROW - LINE)
        .padding(Padding::new(0.0).left(left).right(space::L))
        .style(style::button::list_row(selected))
        .on_press_maybe(on_press);
    let line: Element<'a, M> = if selected {
        let bar = container(blank()).width(SELECTED_BAR).height(size::ROW - LINE).style(|t| container::Style {
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
        row![mark_view(mark, Scale::Small, Tone::Text2), typo::text(typo::CAPTION, label).style(theme::text_2)]
            .spacing(space::S)
            .align_y(Alignment::Center);
    if let Some(n) = count {
        content = content.push(typo::text(typo::META.tabular(), n.to_string()).style(theme::text_3));
    }
    content = content.push(blank::horizontal());
    if let Some(message) = on_add {
        let plus = match mark {
            Mark::Tinted(_, colour) => tinted(Icon::Plus, icon::SIZE_SM, colour),
            _ => toned(Icon::Plus, icon::SIZE_SM, Tone::Text3),
        };
        let add = button(center(plus))
            .width(size::CONTROL_SM)
            .height(size::CONTROL_SM)
            .padding(0)
            .style(style::button::icon)
            .on_press(message);
        content = content.push(add);
    }
    // The "+" button is wider than its icon; pull it in so the icon sits 16 from the edge.
    let right = space::L - (size::CONTROL_SM - icon::SIZE_SM) / 2.0;
    let head = container(content)
        .width(Fill)
        .center_y(size::ROW_GROUP - 2.0 * LINE)
        .padding(Padding::new(0.0).left(space::L).right(right))
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).raised)),
            ..container::Style::default()
        });
    column![
        blank().height(space::XS),
        rule::horizontal(LINE).style(style::rule),
        head,
        rule::horizontal(LINE).style(style::rule)
    ]
    .into()
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
        Anchor { bounds: Rc::clone(&self.bounds), area: Some(area) }
    }

    fn set(&self, bounds: Rectangle) {
        self.bounds.set(Some(bounds));
    }

    /// The anchored rectangle in window coordinates, once the trigger has been laid out.
    fn get(&self) -> Option<Rectangle> {
        let bounds = self.bounds.get()?;
        Some(match self.area {
            Some(area) => Rectangle { x: bounds.x + area.x, y: bounds.y + area.y, ..area },
            None => bounds,
        })
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
    /// 4 below the anchor (menus).
    Below(Align),
    /// 8 right of the anchor, top edges in line; left of it when there is no room (event cards).
    Beside,
}

/// The top-left corner of a card of `card` size placed at `target` inside `viewport`; the card
/// keeps 8 from the viewport edges when it fits.
fn place(target: Rectangle, card: Size, viewport: Rectangle, placement: Placement) -> Point {
    let (x, y) = match placement {
        Placement::Below(Align::Start) => (target.x, target.y + target.height + BELOW_GAP),
        Placement::Below(Align::End) => (target.x + target.width - card.width, target.y + target.height + BELOW_GAP),
        Placement::Beside => {
            let right = target.x + target.width + BESIDE_GAP;
            let x = if right + card.width <= viewport.x + viewport.width - BESIDE_GAP {
                right
            } else {
                target.x - BESIDE_GAP - card.width
            };
            (x, target.y)
        }
    };
    let clamp = |value: f32, start: f32, length: f32, extent: f32| {
        let (low, high) = (start + BESIDE_GAP, start + length - extent - BESIDE_GAP);
        if high < low { low } else { value.clamp(low, high) }
    };
    Point::new(clamp(x, viewport.x, viewport.width, card.width), clamp(y, viewport.y, viewport.height, card.height))
}

/// The layer for an open menu or floating card: `card` ([`menu`], [`popover`]) next to `anchor`
/// as `placement` says, above everything; a click anywhere else sends `on_dismiss` and reaches
/// nothing below (like Linear). Pass it to [`crate::shell::Shell::overlay`]; close it on Escape
/// with [`escape`].
///
/// The card stays inside the window: it moves in from the edges and flips to the other side of
/// the anchor for [`Placement::Beside`].
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
        None => Vector::ZERO,
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

fn menu_entry<'a, M: Clone + 'a>(item: MenuItem<'a, M>) -> Element<'a, M> {
    let mut content = row![].spacing(space::S).align_y(Alignment::Center);
    if let Some(mark) = item.mark {
        content = content.push(mark_view(mark, Scale::Row, Tone::Text));
    }
    content = content.push(line_label(typo::BODY, item.label)).push(trail_view(item.trail));
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

/// A menu card: `menu` surface, `line_strong` border, radius 8, shadow, 4 inside, [`MENU_WIDTH`]
/// wide; `sections` top to bottom with a line between them. Show it with [`floating`].
pub fn menu<'a, M: Clone + 'a>(sections: Vec<MenuSection<'a, M>>) -> Element<'a, M> {
    let mut list = column![];
    for (i, section) in sections.into_iter().enumerate() {
        if i > 0 {
            list = list.push(container(rule::horizontal(LINE).style(style::rule)).padding([space::XS, 0.0]));
        }
        if let Some(title) = section.title {
            let label = typo::text(typo::CAPTION, title).style(theme::text_3);
            list = list.push(container(label).padding(Padding::new(space::S).bottom(space::XS)));
        }
        list = list.extend(section.items.into_iter().map(menu_entry));
    }
    container(list).width(MENU_WIDTH).padding(style::MENU_INSET).style(style::container::menu).into()
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
    content = content.push(typo::text(typo::BODY, label).style(theme::text).wrapping(Wrapping::None)).push(toned(
        Icon::ChevronDown,
        icon::SIZE_SM,
        Tone::Text3,
    ));
    let b = button(container(content).center_y(Fill))
        .height(size::CONTROL_SM)
        .padding(style::BUTTON_PADDING)
        .style(style::button::ghost)
        .on_press(on_press);
    anchored(anchor, b)
}

/// A framed button that opens a menu of choices, with a chevron: "Week ▾". Secondary, label
/// `CAPTION`. Registers `anchor`; its menu usually opens with [`Align::End`].
pub fn select_button<'a, M: Clone + 'a>(anchor: &Anchor, label: impl IntoFragment<'a>, on_press: M) -> Element<'a, M> {
    let content = row![
        typo::text(typo::CAPTION, label).wrapping(Wrapping::None),
        toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text3)
    ]
    .spacing(ICON_GAP)
    .align_y(Alignment::Center);
    let b = button(container(content).center_y(Fill))
        .height(size::CONTROL_SM)
        .padding(style::BUTTON_PADDING)
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
/// Not a typing shortcut: Alt held (Windows reports AltGr as Ctrl + Alt, and AltGr + Q types `@`);
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
    if !modifiers.command() || modifiers.alt() {
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

/// A floating card (an event in the medium calendar): 320 wide, `menu` surface, radius 10,
/// 16 inside, shadow. `head` (calendar or list with "…" and ✕), `title` in `TITLE`, `rows`
/// ([`prop_row`]s) and, below a line, `actions` (buttons). Show it with [`floating`].
pub fn popover<'a, M: 'a>(
    head: impl Into<Element<'a, M>>,
    title: impl IntoFragment<'a>,
    rows: Vec<Element<'a, M>>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    // The card has 8 at the sides and everything but the rows another 8: property rows bring
    // their own 8 so their hover surface reaches out, and their icons line up with the title.
    let inset = Padding::new(0.0).left(space::S).right(space::S);
    let mut card = column![
        container(head).padding(inset.bottom(space::XS)),
        container(typo::text(typo::TITLE, title)).padding(inset.bottom(space::M)),
        column(rows)
    ];
    if !actions.is_empty() {
        card = card.push(
            column![
                rule::horizontal(LINE).style(style::rule),
                row(actions).spacing(space::S).align_y(Alignment::Center)
            ]
            .spacing(space::M)
            .padding(inset.top(space::M)),
        );
    }
    container(card).width(POPOVER_WIDTH).padding([space::L, space::S]).style(style::container::popover).into()
}

// ---------------------------------------------------------------------------------------------
// Chips, pills, key hints, properties
// ---------------------------------------------------------------------------------------------

/// A property chip under a title in the medium layout ("○ Open", "📅 Today, 16:00"): 26 high,
/// `raised` with a `line_strong` frame, `mark` 14 in `text_3`, label `META`. `on_press` opens the
/// property's editor. An empty label gives an icon-only chip ("…").
pub fn chip<'a, M: Clone + 'a>(
    mark: Option<Mark>,
    label: impl IntoFragment<'a>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if let Some(mark) = mark {
        content = content.push(mark_view(mark, Scale::Small, Tone::Text3));
    }
    let label = label.into_fragment();
    if !label.is_empty() {
        content = content.push(typo::text(typo::META, label).wrapping(Wrapping::None));
    }
    let chip =
        container(content).center_y(style::CHIP_HEIGHT).padding(style::CHIP_PADDING).style(style::container::chip);
    match on_press {
        Some(message) => mouse_area(chip).on_press(message).interaction(mouse::Interaction::Pointer).into(),
        None => chip.into(),
    }
}

/// Chips side by side with 6 between them, wrapping onto more lines when they do not fit.
pub fn chip_row<'a, M: 'a>(chips: Vec<Element<'a, M>>) -> Element<'a, M> {
    row(chips).spacing(ICON_GAP).wrap().vertical_spacing(ICON_GAP).into()
}

/// A list label in a row ("● Work"): 20 high, round, `line_strong` frame, dot 6 in `colour`,
/// label `META` in `text_2`. Without a colour it is a plain tag ("review").
pub fn pill<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    pill_at(colour, label, 1.0)
}

/// [`pill`] for a disabled row ("Coming soon" next to a service that is not available yet): the
/// whole pill at the disabled opacity of 40 %, like the row's text.
pub fn pill_disabled<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>) -> Element<'a, M> {
    pill_at(colour, label, style::DISABLED_ALPHA)
}

/// The pill with its colours at `alpha` of their strength.
fn pill_at<'a, M: 'a>(colour: Option<Color>, label: impl IntoFragment<'a>, alpha: f32) -> Element<'a, M> {
    let mut content = row![].spacing(space::XS).align_y(Alignment::Center);
    if let Some(colour) = colour {
        content = content.push(color_dot(colour.scale_alpha(alpha), Scale::Small.dot()));
    }
    content = content.push(typo::text(typo::META, label).wrapping(Wrapping::None));
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
    container(typo::text(typo::CAPTION, key)).padding(style::KBD_PADDING).style(style::container::kbd).into()
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
    let mut content = row![].spacing(space::S).align_y(Alignment::Center);
    if let Some(mark) = mark {
        content = content.push(mark_view(mark, Scale::Row, Tone::Text3));
    }
    content = content.push(value);
    let content = container(content).center_y(Fill);
    match on_press {
        Some(message) => button(content)
            .width(Fill)
            .height(size::CONTROL)
            .padding(style::ITEM_PADDING)
            .style(style::button::sidebar_item(false))
            .on_press(message)
            .into(),
        None => container(content).width(Fill).height(size::CONTROL).padding(style::ITEM_PADDING).into(),
    }
}

/// A group of [`prop_row`]s under a `CAPTION` title in `text_3` ("Properties", "List"). Stack the
/// sections with `space::L` between them.
pub fn prop_section<'a, M: 'a>(title: impl IntoFragment<'a>, rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    let title = container(typo::text(typo::CAPTION, title).style(theme::text_3))
        .padding(Padding { bottom: space::XS, ..style::ITEM_PADDING });
    column![title].extend(rows).into()
}

/// A small muted heading over a group of rows: `CAPTION` in `text_3`, in sentence case as written
/// (Linear uses no capitals for titles).
pub fn section_title<'a, M: 'a>(title: &str) -> Element<'a, M> {
    typo::text(typo::CAPTION, title.to_owned()).style(theme::text_3).into()
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
        typo::text(typo::CAPTION, title).style(theme::text_3),
        container(card).width(Fill).style(style::container::group)
    ]
    .spacing(space::S);
    if let Some(footer) = footer {
        block = block.push(typo::text(typo::META, footer).style(theme::text_2));
    }
    block.into()
}

/// A row of a settings [`group`]: `label` in `BODY`, an optional `detail` below it in `META`
/// `text_2`, and one `control` at the right (toggler, pick list, button; the rest goes into a "…"
/// menu). 44 high, 56 with a detail.
pub fn group_row<'a, M: 'a>(
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let height = if detail.is_some() { size::HEADER + space::M } else { size::HEADER };
    let mut text = column![typo::text(typo::BODY, label)];
    if let Some(detail) = detail {
        text = text.push(typo::text(typo::META, detail).style(theme::text_2));
    }
    container(row![text.width(Fill), control.into()].spacing(space::L).align_y(Alignment::Center))
        .center_y(height)
        .padding(Padding::new(0.0).left(space::L).right(space::L))
        .into()
}

// ---------------------------------------------------------------------------------------------
// Page width and view states
// ---------------------------------------------------------------------------------------------

/// `body` at most `max_width` wide ([`size::CONTENT_MAX`] for lists and pages,
/// [`size::FORM_MAX`] for forms), centred in the free width – large windows give the extra room
/// to the margins, never to gaps between elements.
pub fn content<'a, M: 'a>(max_width: f32, body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(container(body).max_width(max_width).width(Fill)).center_x(Fill).into()
}

/// The loading state: a calm "Loading …" in `text_3`, centred; no spinner, nothing moves.
pub fn loading<'a, M: 'a>() -> Element<'a, M> {
    center(typo::text(typo::META, tr("Loading …")).style(theme::text_3)).into()
}

/// The error state: an alert icon, `what` happened (`BODY`, centred, says what to do) and a
/// "Retry" button that sends `retry`.
pub fn error_state<'a, M: Clone + 'a>(what: impl IntoFragment<'a>, retry: M) -> Element<'a, M> {
    let alert = icon(Icon::CircleAlert, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).error_text) });
    let content = column![
        alert,
        typo::text(typo::BODY, what).style(theme::text_2).align_x(Alignment::Center),
        text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::secondary, Some(retry)),
    ]
    .spacing(space::M)
    .align_x(Alignment::Center)
    .max_width(size::FORM_MAX / 2.0);
    center(content).padding(space::XL).into()
}

/// The empty state of a list or view: what appears here (`title`, `BODY_STRONG`) and what to do
/// (`hint`, `text_2`), centred.
pub fn empty_state<'a, M: 'a>(title: &'a str, hint: &'a str) -> Element<'a, M> {
    let content = column![
        typo::text(typo::BODY_STRONG, title),
        typo::text(typo::BODY, hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::XS)
    .align_x(Alignment::Center)
    .max_width(size::FORM_MAX / 2.0);
    center(content).padding(space::XL).into()
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
            background: Some(Background::Color(status.colour(palette(t)))),
            border: border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// Overlapping round avatars (event guests): `people` as `(initials, colour)`, then "+`more`"
/// when `more > 0`. Each is 22 across with a `panel` ring, initials in `MICRO`.
pub fn avatar_stack<'a, M: 'a>(people: &[(&'a str, Color)], more: usize) -> Element<'a, M> {
    let avatar = |label: String, colour: Option<Color>| -> Element<'a, M> {
        container(typo::text(typo::MICRO, label))
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
        border: border::rounded(radius::LG),
        shadow: palette(t).shadow,
        ..container::Style::default()
    });
    stack![base.into(), opaque(scrim), center(opaque(card)).padding(space::L)].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Go,
        Close,
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle { x, y, width, height }
    }

    const WINDOW: Rectangle = Rectangle { x: 0.0, y: 0.0, width: 900.0, height: 700.0 };
    const MENU: Size = Size { width: MENU_WIDTH, height: 300.0 };

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
    fn a_menu_below_starts_under_the_left_edge_of_its_trigger() {
        let at = place(rect(60.0, 8.0, 90.0, 28.0), MENU, WINDOW, Placement::Below(Align::Start));
        assert_eq!(at, Point::new(60.0, 8.0 + 28.0 + BELOW_GAP));
    }

    #[test]
    fn an_end_aligned_menu_ends_under_the_right_edge_of_its_trigger() {
        let at = place(rect(700.0, 8.0, 76.0, 28.0), MENU, WINDOW, Placement::Below(Align::End));
        assert_eq!(at.x + MENU.width, 776.0);
    }

    #[test]
    fn a_card_beside_flips_left_when_there_is_no_room_on_the_right() {
        let card = Size::new(POPOVER_WIDTH, 240.0);
        let right = place(rect(100.0, 200.0, 120.0, 60.0), card, WINDOW, Placement::Beside);
        assert_eq!(right, Point::new(100.0 + 120.0 + BESIDE_GAP, 200.0));
        let left = place(rect(600.0, 200.0, 120.0, 60.0), card, WINDOW, Placement::Beside);
        assert_eq!(left, Point::new(600.0 - BESIDE_GAP - POPOVER_WIDTH, 200.0));
    }

    #[test]
    fn cards_stay_inside_the_window() {
        // A trigger at the right edge with a start-aligned menu, and one near the bottom.
        let at = place(rect(850.0, 680.0, 40.0, 16.0), MENU, WINDOW, Placement::Below(Align::Start));
        assert_eq!(at, Point::new(900.0 - MENU.width - BESIDE_GAP, 700.0 - MENU.height - BESIDE_GAP));
        // A card larger than the window keeps its top-left corner on screen.
        let huge = place(rect(10.0, 10.0, 10.0, 10.0), Size::new(2000.0, 2000.0), WINDOW, Placement::Beside);
        assert_eq!(huge, Point::new(BESIDE_GAP, BESIDE_GAP));
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
        assert_eq!(Status::Ok.colour(p), p.ok);
        assert_eq!(Status::Error.colour(p), p.error);
        assert_ne!(Status::Error.colour(p), p.accent, "errors are not Torro red");
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
            list_row(vec![typo::text(typo::BODY, "Review").into()], true, Some(Msg::Go)),
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
            pill(Some(p.calendar[1]), "Personal"),
            prop_row(Some(Mark::Icon(Icon::Bell)), typo::text(typo::BODY, "15:45"), Some(Msg::Go)),
            prop_section("Properties", vec![]),
            group("General", vec![group_row("Start at login", Some("Recommended".into()), blank())], None),
            content(size::CONTENT_MAX, blank()),
            loading(),
            error_state("The server did not answer.", Msg::Go),
            empty_state("No tasks", "Add one with N."),
            status_dot(Status::Ok),
            avatar_stack(&[("AW", p.calendar[0])], 3),
            section_title("Calendars"),
            modal(blank(), blank(), size::FORM_MAX, Msg::Close),
        ];
        assert!(!blocks.is_empty());
        let _: Subscription<()> = escape();
    }
}
