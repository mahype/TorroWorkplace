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
//! mail window (task E11.1) the chips of attachments ([`file_chip`]); for saving them (task M12.5) the split
//! chip with a menu at its triangle ([`split_file_chip`]), the block that holds them ([`file_block`]) and the
//! "Save all" action ([`split_action`], framed [`split_action_framed`]); for the mail's actions at its
//! right edge (task E14.1):
//! the action rail ([`action_rail`] with [`RailGroup`]s of [`RailItem`]s, collapsed or expanded,
//! [`RailMode`]) and the layer of an open group ([`rail_flyout`]). For the compact Appearance page
//! (task L.18): a segment with a symbol and a word in each part ([`icon_segmented`]), a heading with
//! quiet actions at its right ([`group_heading`]) and a grid of tiles with a preview to pick from
//! ([`preview_tile`], [`preview_grid`]). For a long choice in a menu (issue #299, the fonts): a menu
//! with a search field at its top ([`search_menu`]).
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
    button, canvas, center, column, container, float, mouse_area, opaque, pin, responsive, row, rule, scrollable,
    stack, svg, tooltip,
};
use iced::{
    Alignment, Background, Color, Element, Fill, Length, Padding, Point, Rectangle, Size, Subscription, Theme, Vector,
};
use iced::{border, keyboard, mouse};

use torro_core::design::{ListLook, ListSelection};
use torro_core::i18n::tr;
use torro_core::shortcuts::{Held, System};
use torro_ipc::api::{Confirm, Level};

use crate::icon::{self, Icon, icon};
use crate::layout;
use crate::shell::{SidebarMode, is_sidebar_key};
use crate::style;
use crate::theme::{self, ChartColor, Palette, palette, radius, size, space};
use crate::themed;
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
/// Edge of an avatar in [`avatar_stack`]: the source tile's 28, so two initials in `CAPTION` fit
/// inside the ring.
fn avatar_edge() -> f32 {
    size::tile()
}
/// How far the next avatar of an [`avatar_stack`] is moved right: it overlaps the one before by
/// [`space::xs()`] (its ring and a little more), so neither covers the other's initials.
fn avatar_step() -> f32 {
    avatar_edge() - space::xs()
}
/// The ring around each avatar of an [`avatar_stack`], in the surface under it.
const AVATAR_RING: f32 = 2.0;
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
    /// An icon in the row's tone with a filled dot in a status colour on its top-right corner, e.g. a
    /// provider whose accounts need the person (issue #248). The dot is a shape, so the state does not
    /// rest on colour alone, and the icon stays the entry's own, so two such entries stay apart in the
    /// rail. The mark takes the icon's room; the dot reaches past its corner.
    Badged(Icon, Color),
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
        Mark::Badged(which, colour) => badged(toned(which, scale.icon(), tone), colour),
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

/// `content` (an icon) with the dot of [`Mark::Badged`] in `colour` centred on its top-right corner.
/// The dot keeps one size at every scale, the smallest that still reads as a shape; it adds no room,
/// so rows and chips do not move when a state comes or goes. Shared with the panel's path marks.
pub(crate) fn badged<'a, M: 'a>(content: Element<'a, M>, colour: Color) -> Element<'a, M> {
    Element::new(badge::CornerDot { content, colour, edge: Scale::Small.dot() })
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

/// An icon button that can be switched on, 28 × 28: ghost while `on` is false, the surface of a
/// choice (`style::button::icon_chosen`) while it is true – the filter icon on the mail search field
/// shows an active quick filter this way. Like [`icon_button`] it needs a tooltip; `None` disables it.
pub fn icon_toggle<'a, M: Clone + 'a>(
    which: Icon,
    tooltip: impl IntoFragment<'a>,
    on: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let style = if on { style::button::icon_chosen } else { style::button::icon };
    icon_only(which, tooltip, style, on_press)
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
    segmented_maybe(options.into_iter().map(|(label, message)| (label, Some(message))), active)
}

/// A [`segmented`] control in which a segment may be off: `(label, None)` is drawn at 40 % and takes
/// no click (a choice that does not apply now; say why in a tooltip around the control). Frame,
/// height and the active segment as in [`segmented`].
///
/// ```ignore
/// let scopes = [(tr("This event"), Some(Message::Scope(One))), (tr("All"), None)];
/// widgets::with_tooltip(widgets::segmented_maybe(scopes, 0), tr("Why All is off"))
/// ```
pub fn segmented_maybe<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (L, Option<M>)>,
    active: usize,
) -> Element<'a, M> {
    let inner = size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0 * LINE;
    let segments = options.into_iter().enumerate().map(|(i, (label, message))| {
        button(container(typo::text(typo::caption(), label).wrapping(Wrapping::None)).center_y(Fill))
            .height(inner)
            .padding(style::button_padding())
            .style(style::button::segment(i == active))
            .on_press_maybe(message)
            .into()
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
}

/// The height of a segment inside a [`segmented`] frame: the control's height less the frame's
/// inset and border.
fn segment_height() -> f32 {
    size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0 * LINE
}

/// A [`segmented`] control whose segments carry a symbol and a word (task L.18, the colour mode:
/// [`Icon::Monitor`] "System", [`Icon::Sun`] "Light", [`Icon::Moon`] "Dark"): one segment per
/// `(icon, label, message)`, the one at index `active` chosen. Frame, height (28) and states are
/// those of [`segmented`] – `bg`, the active segment `selected` with a `line_selected` edge –; in
/// each segment the icon (14) in the segment's text colour, 6 before the label in `CAPTION`.
///
/// Without `words` each segment is a square with the icon alone and `label` as its tooltip, for a
/// row where the words do not fit; decide with [`icon_segmented_width`] against the room the row
/// leaves, so a longer translation holds too. The caller keeps which one is active.
///
/// ```ignore
/// let modes = [(Icon::Monitor, tr("System"), Message::Mode(System)), (Icon::Sun, tr("Light"), …), (Icon::Moon, tr("Dark"), …)];
/// widgets::icon_segmented(modes, active, room >= widgets::icon_segmented_width(&[tr("System"), …], true))
/// ```
pub fn icon_segmented<'a, M: Clone + 'a, L: IntoFragment<'a>>(
    options: impl IntoIterator<Item = (Icon, L, M)>,
    active: usize,
    words: bool,
) -> Element<'a, M> {
    let inner = segment_height();
    let segments = options.into_iter().enumerate().map(|(i, (which, label, message))| {
        let style = style::button::segment(i == active);
        let mark: Element<'a, M> = icon(which, icon::SIZE_SM)
            .style(move |t, _| svg::Style { color: Some(style(t, button::Status::Active).text_color) })
            .into();
        if words {
            let content = row![mark, typo::text(typo::caption(), label).wrapping(Wrapping::None)]
                .spacing(ICON_GAP)
                .align_y(Alignment::Center);
            button(container(content).center_y(Fill))
                .height(inner)
                .padding(style::button_padding())
                .style(style)
                .on_press(message)
                .into()
        } else {
            let square = button(center(mark)).width(inner).height(inner).padding(0).style(style).on_press(message);
            with_tooltip(square, label)
        }
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
}

/// The estimated width of an [`icon_segmented`] control with `labels`: with `words` each segment
/// is its icon, the gap and the label in `CAPTION` with the button padding, without them a square;
/// plus the frame's inset and border. `typo::text_width` estimates generously, so the control fits.
pub fn icon_segmented_width(labels: &[&str], words: bool) -> f32 {
    let frame = 2.0 * (style::SEGMENT_INSET + LINE);
    if !words {
        return labels.len() as f32 * segment_height() + frame;
    }
    let padding = style::button_padding().left + style::button_padding().right;
    let segments: f32 =
        labels.iter().map(|label| typo::text_width(label, typo::caption()) + icon::SIZE_SM + ICON_GAP + padding).sum();
    segments + frame
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
// The chips of a mail's attachments (task E11.1)
// ---------------------------------------------------------------------------------------------

/// An attachment as a chip (task E11.1, the line of attachments at the mail reader's bottom):
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
    button(container(file_chip_content(name, size_text)).center_y(Fill))
        .height(size::control())
        .padding([0.0, space::m()])
        .style(style::button::secondary)
        .on_press_maybe(on_press)
        .into()
}

/// The paperclip, name and size of an attachment, as in a file chip.
fn file_chip_content<'a, M: 'a>(name: impl IntoFragment<'a>, size_text: impl IntoFragment<'a>) -> Element<'a, M> {
    let mut content = row![toned(Icon::Paperclip, icon::SIZE_SM, Tone::Text3), typo::text(typo::body(), name)]
        .spacing(ICON_GAP)
        .align_y(Alignment::Center);
    let size_text = size_text.into_fragment();
    if !size_text.is_empty() {
        content = content.push(typo::text(typo::meta(), size_text).style(theme::text_3).wrapping(Wrapping::None));
    }
    content.into()
}

/// The corners of one part of a split control: the outer ones on its side, radius [`radius::sm()`].
fn split_corners(left: bool) -> border::Radius {
    let r = radius::sm();
    if left {
        border::Radius { top_left: r, bottom_left: r, ..border::Radius::default() }
    } else {
        border::Radius { top_right: r, bottom_right: r, ..border::Radius::default() }
    }
}

/// A part of a split control: no surface at rest, `hover` on hover and `selected` when pressed,
/// the word in `text` – or in `text_2` when `quiet`, so an action reads as an action, not a file.
/// Off, the part is faded like every disabled control ([`style::DISABLED_ALPHA`]).
fn split_part(corners: border::Radius, quiet: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Copy {
    move |theme, status| {
        let p = palette(theme);
        let rest = if quiet { p.text_2 } else { p.text };
        let (background, text) = match status {
            button::Status::Active => (None, rest),
            button::Status::Hovered => (Some(p.hover), p.text),
            button::Status::Pressed => (Some(p.selected), p.text),
            button::Status::Disabled => (None, Color { a: style::DISABLED_ALPHA, ..rest }),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: text,
            border: border::Border { radius: corners, ..border::Border::default() },
            ..button::Style::default()
        }
    }
}

/// A rule between the two parts of a split control: `line` in an action, `line_strong` on a chip.
fn split_divider(strong: bool) -> impl Fn(&Theme) -> rule::Style + Copy {
    move |theme| {
        let p = palette(theme);
        rule::Style {
            color: if strong { p.line_strong } else { p.line },
            radius: border::Radius::default(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        }
    }
}

/// The triangle of a split control: a quiet part with a chevron down (14, `text_2`) that opens the
/// menu, `tip` as its tooltip, and the anchor of the menu when there is one.
fn split_menu<'a, M: Clone + 'a>(
    on_menu: Option<M>,
    anchor: Option<&Anchor>,
    corners: border::Radius,
    tip: impl IntoFragment<'a>,
) -> Element<'a, M> {
    let chevron =
        container(toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text2)).center_y(Fill).padding([0.0, space::s()]);
    let trigger = button(chevron).height(Fill).style(split_part(corners, true)).on_press_maybe(on_menu);
    with_tooltip(anchored_if(anchor, trigger), tip)
}

/// The frame of a framed split control ([`split_file_chip`], [`split_action_framed`]): `raised` with a
/// `line_strong` border, radius [`radius::sm()`].
fn split_frame(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.raised)),
        border: border::rounded(radius::sm()).color(p.line_strong).width(LINE),
        ..container::Style::default()
    }
}

/// A file attachment as a chip with a menu: [`file_chip`]'s part that opens the file (paperclip,
/// `name`, `size_text`), a hairline in `line_strong`, and at the right the triangle that opens the
/// menu of ways to open or save (task M12.5). The two parts have their own hover, the frame is
/// shared; the chip is [`size::control()`] high. `on_open` and `on_menu` show their part off when
/// `None`; `anchor` is where the menu floats (`anchored_if`). `open_tip` is the tooltip of the part
/// that opens ("Open: invoice.pdf"), none when empty – it sits on that part alone, so it never stands
/// over the triangle's own.
///
/// ```ignore
/// widgets::split_file_chip(&a.name, format_size(a.size), "Open: a.pdf", Some(Msg::Open(i)), Some(Msg::Menu(i)), Some(&anchor))
/// ```
pub fn split_file_chip<'a, M: Clone + 'a>(
    name: impl IntoFragment<'a>,
    size_text: impl IntoFragment<'a>,
    open_tip: impl IntoFragment<'a>,
    on_open: Option<M>,
    on_menu: Option<M>,
    anchor: Option<&Anchor>,
) -> Element<'a, M> {
    let open = button(container(file_chip_content(name, size_text)).center_y(Fill))
        .height(Fill)
        .padding([0.0, space::m()])
        .style(split_part(split_corners(true), false))
        .on_press_maybe(on_open);
    let open_tip = open_tip.into_fragment();
    let open: Element<'a, M> = if open_tip.is_empty() { open.into() } else { with_tooltip(open, open_tip) };
    let triangle = split_menu(on_menu, anchor, split_corners(false), tr("More ways to open or save"));
    let parts = row![open, rule::vertical(LINE).style(split_divider(true)), triangle].height(Fill);
    container(parts).height(size::control()).style(split_frame).into()
}

/// "Save all" and its menu as one ghost control (task M12.5): no surface and no frame, so it does
/// not look like an attachment; the action with `which` (Download) and `label` in `text_2`, a quiet
/// hairline and the triangle beside it as in [`split_file_chip`], with `menu_tip` as the triangle's
/// tooltip ("More ways to save all"). [`size::control_sm()`] high, shown from two attachments on. For
/// the head of a [`file_block`], whose band and frame hold it; where it stands alone, take
/// [`split_action_framed`].
///
/// ```ignore
/// widgets::split_action(Icon::Download, tr("Save all"), tr("More ways to save all"), Some(Msg::SaveAll), Some(Msg::SaveAllMenu), Some(&anchor))
/// ```
pub fn split_action<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    menu_tip: impl IntoFragment<'a>,
    on_press: Option<M>,
    on_menu: Option<M>,
    anchor: Option<&Anchor>,
) -> Element<'a, M> {
    split_action_parts(which, label, menu_tip, on_press, on_menu, anchor, false).height(size::control_sm()).into()
}

/// [`split_action`] in a frame like a secondary button (`raised`, `line_strong` border, radius
/// [`radius::sm()`], the hairline between the parts in `line_strong`), [`size::control_sm()`] high: for
/// "Save all ▾" where no block frames it, e.g. over the attachment rows of the panel (task M12.6), so
/// its hairline does not stand alone.
///
/// ```ignore
/// widgets::split_action_framed(Icon::Download, tr("Save all"), tr("More ways to save all"), Some(Msg::SaveAll), Some(Msg::SaveAllMenu), None)
/// ```
pub fn split_action_framed<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    menu_tip: impl IntoFragment<'a>,
    on_press: Option<M>,
    on_menu: Option<M>,
    anchor: Option<&Anchor>,
) -> Element<'a, M> {
    let parts = split_action_parts(which, label, menu_tip, on_press, on_menu, anchor, true).height(Fill);
    container(parts).height(size::control_sm()).style(split_frame).into()
}

/// The two parts of a split action and the hairline between them: the action (`which`, `label` in
/// `text_2`) and the triangle with `menu_tip`. The hairline is `line_strong` in a frame (`framed`),
/// like a chip's, and the quieter `line` on a band without one.
fn split_action_parts<'a, M: Clone + 'a>(
    which: Icon,
    label: impl IntoFragment<'a>,
    menu_tip: impl IntoFragment<'a>,
    on_press: Option<M>,
    on_menu: Option<M>,
    anchor: Option<&Anchor>,
    framed: bool,
) -> iced::widget::Row<'a, M> {
    let content =
        row![toned(which, icon::SIZE_SM, Tone::Text2), typo::text(typo::caption(), label).style(theme::text_2)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center);
    let open = button(container(content).center_y(Fill))
        .height(Fill)
        .padding([0.0, space::s()])
        .style(split_part(split_corners(true), true))
        .on_press_maybe(on_press);
    let triangle = split_menu(on_menu, anchor, split_corners(false), menu_tip);
    row![open, rule::vertical(LINE).style(split_divider(framed)), triangle]
}

/// The head of an attachment block (see [`file_block`]): a band with the paperclip, `title` in
/// `typo::body_strong()` and `meta` in `typo::meta()` in `text_3`, and `action` at its right end.
///
/// ```ignore
/// widgets::file_block_head("3 attachments", "1.2 MB", Some(widgets::split_action(…)))
/// ```
pub fn file_block_head<'a, M: 'a>(
    title: impl IntoFragment<'a>,
    meta: impl IntoFragment<'a>,
    action: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let mut head = row![
        toned(Icon::Paperclip, icon::SIZE, Tone::Text3),
        container(typo::text(typo::body_strong(), title).wrapping(Wrapping::None)).width(Fill).clip(true),
        typo::text(typo::meta().tabular(), meta).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(ICON_GAP)
    .align_y(Alignment::Center);
    if let Some(action) = action {
        head = head.push(action);
    }
    // `center_y` with the control's height, not `Fill`: `Fill` would replace the band's height and let
    // it take half a reader.
    container(head)
        .padding(Padding::new(0.0).left(space::m()).right(space::s()))
        .center_y(size::control())
        .width(Fill)
        .into()
}

/// The block around a set of attachments (task M12.5, mail reader): `head` (see
/// [`file_block_head`]) as a band with a [`LINE`] under it, then `content` with [`space::s()`]
/// above and below and [`space::m()`] at the sides – the head's own inset, so the chips start under
/// its paperclip – all in a `line_strong` frame with radius [`radius::sm()`].
///
/// ```ignore
/// widgets::file_block(widgets::file_block_head(title, meta, None), chips)
/// ```
pub fn file_block<'a, M: 'a>(head: impl Into<Element<'a, M>>, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    let body = column![
        head.into(),
        rule::horizontal(LINE).style(style::rule),
        container(content).padding([space::s(), space::m()]).width(Fill),
    ];
    container(body)
        .width(Fill)
        .style(|t| container::Style {
            border: border::rounded(radius::sm()).color(palette(t).line_strong).width(LINE),
            ..container::Style::default()
        })
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
    sidebar_row(mode, SidebarRow { mark: mark.into(), scale: Scale::Row, fold: None }, label, trail, active, on_press)
}

/// A group row that folds its children in and out (the category groups of the mail window, issue
/// #300): expanded like [`sidebar_item_in`], with a chevron ([`Icon::ChevronDown`] when `open`, else
/// [`Icon::ChevronRight`], 12 in `text_3`) right after the label, so the icons of all rows stay in one
/// column; in the rail exactly a [`sidebar_item_in`], without a chevron (the rail shows no children).
/// The whole row is the button; what a click does (show the group, fold it) is the app's to decide.
/// Put the open group's children after it in a [`sidebar_branch`].
///
/// ```ignore
/// let open = self.open_groups.contains(&group);
/// items.push(widgets::sidebar_fold_in(mode, Mark::Tinted(icon, colour), tr("Paper trail"), Trail::Count(7), active, open, Message::Fold(group)));
/// if open && mode == SidebarMode::Expanded {
///     items.push(widgets::sidebar_branch(colour, children));
/// }
/// ```
pub fn sidebar_fold_in<'a, M: Clone + 'a>(
    mode: SidebarMode,
    mark: impl Into<Mark>,
    label: impl IntoFragment<'a>,
    trail: Trail,
    active: bool,
    open: bool,
    on_press: M,
) -> Element<'a, M> {
    let row = SidebarRow { mark: mark.into(), scale: Scale::Row, fold: Some(open) };
    sidebar_row(mode, row, label, trail, active, on_press)
}

/// The children of an open group ([`sidebar_fold_in`]) as one branch: `rows` ([`sidebar_child`])
/// below one another with the sidebar's gap, [`icon::SIZE`] + [`space::s()`] further in than the
/// group, and a 1-px line in `line` (the group's colour) under the middle of the group's icon, over
/// the whole height of the branch – it binds the children to their group. Only in the expanded
/// sidebar; the rail shows no branches.
///
/// ```ignore
/// let children = categories.iter().map(|c| widgets::sidebar_child(Mark::Tinted(c.icon, colour), c.label, Trail::Count(c.unread), c.active, Message::Show(c.view))).collect();
/// items.push(widgets::sidebar_branch(colour, children));
/// ```
pub fn sidebar_branch<'a, M: 'a>(line: Color, rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    branch(rows, move |_| line)
}

/// A [`sidebar_branch`] whose line is `line_strong` from the theme (task DS.4): for an object open
/// under its section (an account under "Accounts"), which has no colour of its own. In the
/// settings' sidebar, whose colours a design may set apart (`palette(theme)` there).
pub fn sidebar_branch_quiet<'a, M: 'a>(rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    branch(rows, |t| palette(t).line_strong)
}

/// The rows of a branch with their line in the colour `line` picks from the theme.
fn branch<'a, M: 'a>(rows: Vec<Element<'a, M>>, line: impl Fn(&Theme) -> Color + 'a) -> Element<'a, M> {
    let children = container(column(rows).spacing(crate::shell::SIDEBAR_GAP))
        .width(Fill)
        .padding(Padding::new(0.0).left(branch_indent()));
    let stroke = container(blank()).width(LINE).height(Fill).style(move |t| container::Style {
        background: Some(Background::Color(line(t))),
        ..container::Style::default()
    });
    let guide =
        container(stroke).width(Fill).height(Fill).padding(Padding::new(0.0).left(branch_line_x() - LINE / 2.0));
    // The children are the stack's first layer and so give it its height; the line fills it.
    stack![children, guide].width(Fill).into()
}

/// A child row in a [`sidebar_branch`] (a category under its group, issue #300): drawn like an
/// expanded [`sidebar_item_in`], with the mark one step smaller ([`icon::SIZE_SM`]) so the group
/// above it keeps the rank.
///
/// ```ignore
/// widgets::sidebar_child(Mark::Tinted(Icon::TextQuote, colour), tr("Invoice received"), Trail::Count(2), shown, Message::Show(view))
/// ```
pub fn sidebar_child<'a, M: Clone + 'a>(
    mark: impl Into<Mark>,
    label: impl IntoFragment<'a>,
    trail: Trail,
    active: bool,
    on_press: M,
) -> Element<'a, M> {
    let row = SidebarRow { mark: mark.into(), scale: Scale::Small, fold: None };
    sidebar_row(SidebarMode::Expanded, row, label, trail, active, on_press)
}

/// How far the children of a group stand in ([`sidebar_branch`], nested [`MenuItem`]s): one icon
/// and the gap after it, so a child's mark starts where the group's label does.
fn branch_indent() -> f32 {
    icon::SIZE + space::s()
}

/// Where the line of a [`sidebar_branch`] runs, from the branch's left edge: the middle of the
/// group's icon in the row above.
fn branch_line_x() -> f32 {
    style::ITEM_PADDING.left + icon::SIZE / 2.0
}

/// What sets the sidebar rows apart: the mark and its scale, and for a folding group whether it is
/// open (`Some`, drawn with a chevron).
struct SidebarRow {
    mark: Mark,
    scale: Scale,
    fold: Option<bool>,
}

fn sidebar_row<'a, M: Clone + 'a>(
    mode: SidebarMode,
    parts: SidebarRow,
    label: impl IntoFragment<'a>,
    trail: Trail,
    active: bool,
    on_press: M,
) -> Element<'a, M> {
    let tone = if active { Tone::Text } else { Tone::Text2 };
    let mark = mark_view(parts.mark, parts.scale, tone);
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
            let name = match parts.fold {
                None => line_label(typo::body(), label),
                Some(open) => {
                    // The chevron follows the name instead of the row's end, so the label takes
                    // only its own width. A name wider than the row pushes the chevron out of the
                    // clip (Iced 0.14 has no ellipsis); the group names it is made for are short.
                    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
                    let name = row![
                        typo::text(typo::body(), label).wrapping(Wrapping::None),
                        toned(chevron, GROUP_CHEVRON, Tone::Text3)
                    ]
                    .spacing(space::xs())
                    .align_y(Alignment::Center);
                    container(name).width(Fill).clip(true).into()
                }
            };
            let content = row![mark, name].push(trail_view(trail)).spacing(space::s()).align_y(Alignment::Center);
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
        return sidebar_gap_in(mode);
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

/// The gap before a group of the sidebar that has no title (issue #337, the piles of the mail
/// window): expanded a step more than the space a [`sidebar_group_in`] leaves above its title
/// (`space::l()`, "group to group 16–24"), since no title marks where the group starts – so the rows
/// stand apart from those before without a word that would say nothing new; in the rail the same
/// line as a group's title there.
///
/// ```ignore
/// items.push(widgets::sidebar_gap_in(mode));
/// items.extend(piles.into_iter().map(row));
/// ```
pub fn sidebar_gap_in<'a, M: 'a>(mode: SidebarMode) -> Element<'a, M> {
    match mode {
        SidebarMode::Rail => {
            container(rule::horizontal(LINE).style(style::rule)).padding([space::s(), space::xs()]).into()
        }
        SidebarMode::Expanded => blank().height(space::l()).into(),
    }
}

// ---------------------------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------------------------

/// A list row (a task, an agenda entry): [`size::row()`] high, `cells` side by side with 10 between
/// them, the sides of [`layout::row_padding`] (the content is centred in the height). Give the title
/// cell `.width(Fill)` so the cells after it sit at the right. How the row is drawn follows the
/// design's [`layout::list`] (issue #291); the cells stay where they are in every design:
///
/// - `separators`: a line below the row (inside its height, as the CSS's `border-box`), or none.
/// - the `selected` row (the one in the detail) with `selection = "bar"` (Standard, Compact): the
///   neutral `selected` surface with a 2-px bar of the accent colour at its left ([`selection_bar`]);
///   with `selection = "fill"` (Console): the whole row in `accent`, its cells drawn in `on_accent`
///   ([`row_content`]) – the one place besides the primary button where the accent is a surface.
///
/// ```ignore
/// widgets::list_row(vec![check.into(), priority, typo::text(typo::body(), &task.title).width(Fill).into(), due], task.id == selected, Some(Message::Select(task.id)))
/// ```
pub fn list_row<'a, M: Clone + 'a>(cells: Vec<Element<'a, M>>, selected: bool, on_press: Option<M>) -> Element<'a, M> {
    list_row_in(layout::list(), cells, selected, on_press)
}

/// What [`list_row`] draws around its cells in a design with the list look `look`: worked out apart
/// from the widgets, so the tests can check every look without switching the process's design.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RowParts {
    /// The accent bar at the left ([`selection_bar`]).
    bar: bool,
    /// The cells in `on_accent` ([`row_content`]).
    on_accent: bool,
    /// The line below the row.
    line: bool,
}

impl RowParts {
    fn of(look: ListLook, selected: bool) -> RowParts {
        let (bar, on_accent) = match (selected, look.selection) {
            (false, _) => (false, false),
            (true, ListSelection::Bar) => (true, false),
            (true, ListSelection::Fill) => (false, true),
        };
        RowParts { bar, on_accent, line: look.separators }
    }
}

/// [`list_row`] in the list look `look`.
fn list_row_in<'a, M: Clone + 'a>(
    look: ListLook,
    cells: Vec<Element<'a, M>>,
    selected: bool,
    on_press: Option<M>,
) -> Element<'a, M> {
    let parts = RowParts::of(look, selected);
    let content = row(cells).spacing(ROW_GAP).align_y(Alignment::Center);
    let content = if parts.on_accent { themed::on_accent(content) } else { content.into() };
    let pad = layout::row_padding();
    // The bar takes its width from the padding, so the cells do not move when the row is chosen.
    let left = if parts.bar { pad.left - SELECTED_BAR } else { pad.left };
    let height = size::row() - if parts.line { LINE } else { 0.0 };
    let body = button(container(content).center_y(Fill))
        .width(Fill)
        .height(height)
        // Only the sides: the height is the row's, the content centred in it.
        .padding(Padding::new(0.0).left(left).right(pad.right))
        .style(style::button::list_row_in(look.selection, selected))
        .on_press_maybe(on_press);
    let body: Element<'a, M> = if parts.bar { row![selection_bar(height), body].into() } else { body.into() };
    if parts.line { column![body, rule::horizontal(LINE).style(style::rule)].into() } else { body }
}

/// The bar that marks the chosen row of a list in a design with `[list] selection = "bar"` (issue
/// #291): 2 wide, `height` high, in the accent colour. [`list_row`] puts it at the row's left edge and
/// takes its width from the row's left padding; a view that draws its rows itself does the same.
pub fn selection_bar<'a, M: 'a>(height: f32) -> Element<'a, M> {
    container(blank())
        .width(SELECTED_BAR)
        .height(height)
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).accent)),
            ..container::Style::default()
        })
        .into()
}

/// The cells of a list row as the design draws them (issue #291): on the `selected` row of a design
/// with `[list] selection = "fill"` in a theme where every text colour – `text`, `text_2`, `text_3`,
/// and icons toned with them – is `on_accent`, so they stay readable on the accent surface of
/// `style::button::list_row`; otherwise `content` unchanged. Colours of their own (a status icon, a
/// list's colour) stay. [`list_row`] does this itself; a view that draws its rows with
/// `style::button::list_row` wraps its content in it.
pub fn row_content<'a, M: 'a>(content: impl Into<Element<'a, M>>, selected: bool) -> Element<'a, M> {
    if RowParts::of(layout::list(), selected).on_accent { themed::on_accent(content) } else { content.into() }
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
}

impl<'a, M> RowAction<'a, M> {
    /// A button with `icon`, `label` and the key hint `key` that sends `on_press`; no anchor.
    pub fn new(icon: Icon, label: impl IntoFragment<'a>, key: &'static str, on_press: M) -> Self {
        Self { icon, label: label.into_fragment(), key, on_press, anchor: None, blocked: None }
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
    /// A second line under the label in `text_3` ([`MenuItem::detail`]), e.g. the folder a save goes to.
    pub detail: Option<iced::widget::text::Fragment<'a>>,
    /// Words between the label and the trail, at most two ([`MenuItem::tag`]).
    pub tags: Vec<MenuTag<'a>>,
    /// A child of the entry above it (a category under its group, issue #300): indented by an icon
    /// and its gap, with the mark one step smaller.
    pub nested: bool,
    /// Sent when the entry is clicked; the app closes the menu itself.
    pub on_press: M,
}

/// A word at the end of a [`MenuItem`] (task M12.5), e.g. "Click" or "Suggested"; with `sparkles`
/// the Torro sparkles in front of it, for a suggestion the model made.
#[derive(Debug, Clone)]
pub struct MenuTag<'a> {
    /// The word, through `tr` like every label.
    pub label: iced::widget::text::Fragment<'a>,
    /// Whether the sparkles of the AI stand in front of the word.
    pub sparkles: bool,
}

/// How many [`MenuTag`]s an entry shows: a third word would crowd the label out.
const MENU_TAGS: usize = 2;

impl<'a, M> MenuItem<'a, M> {
    /// An entry with `mark` and `label` that sends `on_press`; no trail, not current, not nested.
    pub fn new(mark: impl Into<Mark>, label: impl IntoFragment<'a>, on_press: M) -> Self {
        Self {
            mark: Some(mark.into()),
            label: label.into_fragment(),
            trail: Trail::None,
            current: false,
            detail: None,
            tags: Vec::new(),
            nested: false,
            on_press,
        }
    }

    /// The same entry with `text` as a second line in `typo::meta()` in `text_3`, cut with "…" when
    /// it is too long. The entry is twice as high as one without it.
    ///
    /// ```ignore
    /// MenuItem::new(Icon::Folder, tr("Save to Downloads"), Message::SaveTo(ToDownloads))
    ///     .detail("~/Downloads")
    /// ```
    pub fn detail(self, text: impl IntoFragment<'a>) -> Self {
        Self { detail: Some(text.into_fragment()), ..self }
    }

    /// The same entry with the word `label` after its label, before the trail; with `sparkles` the
    /// AI's sparkles in `accent` in front of it. At most two words are shown; a third is left out.
    ///
    /// ```ignore
    /// MenuItem::new(Icon::Folder, "Invoices", Message::Save(id)).tag("Suggested", true)
    /// ```
    pub fn tag(mut self, label: impl IntoFragment<'a>, sparkles: bool) -> Self {
        if self.tags.len() < MENU_TAGS {
            self.tags.push(MenuTag { label: label.into_fragment(), sparkles });
        }
        self
    }

    /// The same entry with `trail` at the end.
    pub fn trail(self, trail: Trail) -> Self {
        Self { trail, ..self }
    }

    /// The same entry marked as the current choice when `current` is true.
    pub fn current(self, current: bool) -> Self {
        Self { current, ..self }
    }

    /// The same entry as a child of the one above it: indented, its mark one step smaller.
    pub fn nested(self) -> Self {
        Self { nested: true, ..self }
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
    let height = menu_entry_height(item.detail.is_some());
    let label: Element<'a, M> = match item.detail {
        Some(detail) => column![menu_label(item.label), detail_line(detail)].width(Fill).into(),
        // Without tags nothing takes from the label's width: clip it as every menu did before
        // (task M12.5 must not move the entries of the suite's other menus).
        None if item.tags.is_empty() => line_label(typo::body(), item.label),
        None => menu_label(item.label),
    };
    let scale = if item.nested { Scale::Small } else { Scale::Row };
    let mut content = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(mark) = item.mark {
        content = content.push(mark_view(mark, scale, Tone::Text));
    }
    content = content.push(label);
    for tag in item.tags {
        content = content.push(tag_view(tag));
    }
    content = content.push(trail_view(item.trail));
    if item.current {
        content = content.push(container(blank()).width(ICON_GAP).height(ICON_GAP).style(|t| container::Style {
            background: Some(Background::Color(palette(t).text_2)),
            border: border::rounded(ICON_GAP / 2.0),
            ..container::Style::default()
        }));
    }
    let entry = button(container(content).center_y(Fill))
        .width(Fill)
        .height(height)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(item.on_press);
    if item.nested {
        // The whole entry moves in, hover surface included, as in the sidebar's branch.
        container(entry).padding(Padding::new(0.0).left(branch_indent())).into()
    } else {
        entry.into()
    }
}

/// How high a menu entry is: [`style::MENU_ITEM_HEIGHT`], and with a detail line that line's height
/// of `typo::meta()` more, so title and detail stand close together in the middle of the entry with
/// the same air above and below as a one-line entry has.
fn menu_entry_height(detail: bool) -> f32 {
    if detail { style::MENU_ITEM_HEIGHT + typo::line_height(typo::meta()) } else { style::MENU_ITEM_HEIGHT }
}

/// The label of a menu entry, cut with "…" to the width the tags and trail leave it (measured when
/// laid out) instead of clipped mid-letter: the same look as [`line_label`] otherwise.
fn menu_label<'a, M: 'a>(label: iced::widget::text::Fragment<'a>) -> Element<'a, M> {
    let label = label.to_string();
    measured_line(typo::body(), move |size| {
        let shown = typo::ellipsize(&label, typo::body(), size.width);
        typo::text(typo::body(), shown).wrapping(Wrapping::None).into()
    })
}

/// The second line of a menu entry: `typo::meta()` in `text_3`, cut with "…" to the width the
/// label column has, which the tags behind the label take from (measured when laid out).
fn detail_line<'a, M: 'a>(detail: iced::widget::text::Fragment<'a>) -> Element<'a, M> {
    let detail = detail.to_string();
    measured_line(typo::meta(), move |size| {
        let shown = typo::ellipsize(&detail, typo::meta(), size.width);
        typo::text(typo::meta(), shown).style(theme::text_3).wrapping(Wrapping::None).into()
    })
}

/// A line of text in `level` built by `view` from the width it gets, exactly one line of `level`
/// high: `responsive` alone takes all the height it is offered, and its text would then sit at the
/// top of the entry instead of in its middle.
fn measured_line<'a, M: 'a>(level: typo::Level, view: impl Fn(Size) -> Element<'a, M> + 'a) -> Element<'a, M> {
    container(responsive(view)).width(Fill).height(typo::line_height(level)).into()
}

/// A [`MenuTag`] behind the label of a menu entry: `typo::caption()` in `text_3`, with the sparkles
/// of the AI in `accent` in front of it when it has them.
fn tag_view<'a, M: 'a>(tag: MenuTag<'a>) -> Element<'a, M> {
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if tag.sparkles {
        let sparkles = icon(Icon::Sparkles, icon::SIZE_SM).style(|t, _| svg::Style { color: Some(palette(t).accent) });
        content = content.push(sparkles);
    }
    content.push(typo::text(typo::caption(), tag.label).style(theme::text_3).wrapping(Wrapping::None)).into()
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

/// A long choice in a floating menu (issue #299, the fonts of Appearance): a search field at the top
/// (flat, [`style::text_input_flat`], with the symbol `Search`, [`size::control()`] high), a line, and
/// under it the caller's `items` – section titles, lines, entries – scrolling past [`size::panel()`],
/// in the card of a menu `width` wide. Without items it says "No matches" in `text_3`.
///
/// The caller keeps `query` and filters `items` by it itself; `on_input` comes with every change of
/// the field, `on_submit` with Enter (the caller takes its first match). `id` names the field, so the
/// caller can focus it when it opens the menu (`iced::widget::operation::focus(id)`). Show it with
/// [`floating`]; Escape closes it like any menu ([`escape`]).
///
/// ```ignore
/// let items: Vec<Element<_>> = fonts.iter().filter(|f| matches(f, &self.query)).map(font_item).collect();
/// let menu = widgets::search_menu(FONT_SEARCH, &self.query, Message::Query, Message::First, items, size::props());
/// widgets::floating(&anchor, Placement::Below(Align::End), menu, Message::Close)
/// ```
pub fn search_menu<'a, M: Clone + 'a>(
    id: &'static str,
    query: &'a str,
    on_input: impl Fn(String) -> M + 'a,
    on_submit: M,
    items: Vec<Element<'a, M>>,
    width: f32,
) -> Element<'a, M> {
    let level = typo::body();
    let input = iced::widget::text_input(tr("Search"), query)
        .id(id)
        .size(level.size)
        .line_height(level.line_height)
        .font(level.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input_flat)
        .on_input(on_input)
        .on_submit(on_submit);
    let field =
        row![toned(Icon::Search, icon::SIZE_SM, Tone::Text3), input].spacing(ICON_GAP).align_y(Alignment::Center);
    let field =
        container(field).width(Fill).height(size::control()).center_y(size::control()).padding(style::ITEM_PADDING);
    let list = match search_results(items) {
        Ok(items) => column(items),
        Err(none) => {
            column![container(typo::text(typo::body(), none).style(theme::text_3)).padding(style::ITEM_PADDING)]
                .padding([space::s(), 0.0])
        }
    };
    let body = scrollable(list)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Length::Shrink)
        .width(Fill);
    let line = container(rule::horizontal(LINE).style(style::rule)).padding([space::xs(), 0.0]);
    let card = column![field, line, container(body).max_height(size::panel())];
    container(card).width(width).padding(style::MENU_INSET).style(style::container::menu).into()
}

/// The entries of a [`search_menu`], or the words it shows instead when the search found none.
fn search_results<M>(items: Vec<Element<'_, M>>) -> Result<Vec<Element<'_, M>>, &'static str> {
    if items.is_empty() { Err(tr("No matches")) } else { Ok(items) }
}

/// The action rail as a context menu: its groups form vertical sections with separators.
/// Uses the rail's item rows so disabled actions and key hints have the same appearance.
pub fn rail_context_menu<'a, M: Clone + 'a, N: Clone + 'a>(
    groups: Vec<RailGroup<'a, M>>,
    on_action: impl Fn(M) -> N,
    max_height: f32,
) -> Element<'a, N> {
    let mut list = column![];
    for (index, group) in groups.into_iter().enumerate() {
        if index > 0 {
            list = list.push(container(rule::horizontal(LINE).style(style::rule)).padding([space::xs(), 0.0]));
        }
        let title = typo::text(typo::caption(), group.label).style(theme::text_3).wrapping(Wrapping::None);
        list = list.push(container(title).padding(flyout_title_padding()));
        for item in group.items {
            let item = RailItem {
                icon: item.icon,
                label: item.label,
                key: item.key,
                on_press: item.on_press.map(&on_action),
                accent: item.accent,
                lead: item.lead,
                danger: item.danger,
                anchor: None,
            };
            list = list.push(rail_row(item, RailPlace::Card, false));
        }
    }
    let body = scrollable(list).height(Length::Shrink).width(Fill);
    container(container(body).max_height(max_height))
        .width(rail_flyout_width())
        .padding(style::MENU_INSET)
        .style(style::container::menu)
        .into()
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

// ---------------------------------------------------------------------------------------------
// Action rail of a mail (task E14.1)
// ---------------------------------------------------------------------------------------------

/// How the action rail of a mail ([`action_rail`]) is drawn. The person's choice for the wide
/// mail window, remembered per window in `ui.json` ([`crate::ui_state::actions_from`]); medium and
/// narrow windows always draw [`RailMode::Collapsed`] (task E14, decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RailMode {
    /// [`crate::shell::rail()`] wide: one icon per group; hovering an icon opens its group beside it
    /// ([`rail_flyout`]), a click does the group's first action that goes.
    #[default]
    Collapsed,
    /// [`size::rail_wide()`] wide (issue #291; 248 in the standard design): every group open with its
    /// title, every action with icon, word and key – a column of the layout that moves the mail
    /// inwards, nothing lies over it.
    Expanded,
}

impl RailMode {
    /// The other mode: what the button at the top of the rail and the key `]` switch to
    /// ([`crate::shell::is_actions_key`]).
    pub fn toggled(self) -> Self {
        match self {
            RailMode::Collapsed => RailMode::Expanded,
            RailMode::Expanded => RailMode::Collapsed,
        }
    }

    /// How wide the rail is in this mode: its column – [`crate::shell::rail()`] collapsed,
    /// [`size::rail_wide()`] expanded – and the line on its left. For apps that work out the room
    /// left for the mail.
    pub fn width(self) -> f32 {
        self.column() + LINE
    }

    /// The rail's column without its line. Collapsed it holds an icon of [`size::control_lg()`]
    /// centred in [`crate::shell::rail()`], as the collapsed sidebar does: the line stands outside,
    /// so the icons sit on whole pixels.
    fn column(self) -> f32 {
        match self {
            RailMode::Collapsed => crate::shell::rail(),
            RailMode::Expanded => size::rail_wide(),
        }
    }
}

/// One action in the action rail of a mail ([`action_rail`], [`rail_flyout`]): icon, word and the
/// key that does the same in the window, and the message it sends.
///
/// ```ignore
/// RailItem::new(Icon::Reply, tr("Reply"), "R", Some(Message::Mail(MailAction::Reply))).lead()
/// RailItem::new(Icon::Trash2, tr("Delete"), "#", Some(Message::Mail(MailAction::Delete))).danger()
/// RailItem::new(Icon::Archive, tr("Archive"), "", None) // there is no archive folder: off
/// ```
#[derive(Debug, Clone)]
pub struct RailItem<'a, M> {
    pub icon: Icon,
    /// The word of the row ("Reply", "Later…"), sentence case through `tr`.
    pub label: iced::widget::text::Fragment<'a>,
    /// The key that does the same ("R"), shown as [`kbd`] at the end of the row; `""` for none.
    /// A row that is off shows no key: the key does nothing then either.
    pub key: &'static str,
    /// Sent when the row is pressed; `None` shows it off (faded to [`style::DISABLED_ALPHA`], like a
    /// [`bar_button`] without a message) and it sends nothing. The app closes an open group
    /// ([`rail_flyout`]) on every message of an item: a button sends one message only.
    pub on_press: Option<M>,
    /// The icon in the accent colour (the AI's sparkles), set with [`RailItem::accent`] – an
    /// exception to "red only for the primary button", as the pills of task E11 were. Only the icon,
    /// never the word or the surface.
    pub accent: bool,
    /// The mail's main action (Reply, task E14.3 decision A), set with [`RailItem::lead`]: the icon
    /// in the accent and the word strong (`typo::body_strong()` in `text`), where the row stands
    /// with its word – expanded and in the open group.
    pub lead: bool,
    /// The action cannot be taken back at once (Delete), set with [`RailItem::danger`]: it is never
    /// what a click on the group's icon does ([`RailGroup::first_press`]).
    pub danger: bool,
    /// Registered by the row wherever it is drawn (expanded rail, open group), set with
    /// [`RailItem::at`]: a dialog the item opens hangs at its row. Chain it with the group's
    /// anchor (`row_anchor.or(&group_anchor)`, [`Anchor::or`]) so it hangs at the group's icon while the row is
    /// not on screen (collapsed rail, group closed).
    pub anchor: Option<Anchor>,
}

impl<'a, M> RailItem<'a, M> {
    /// An item with `icon`, `label` and the key hint `key` (`""` for none) that sends `on_press`
    /// (`None`: off); not in the accent, not the lead, not dangerous, no anchor.
    pub fn new(icon: Icon, label: impl IntoFragment<'a>, key: &'static str, on_press: Option<M>) -> Self {
        Self {
            icon,
            label: label.into_fragment(),
            key,
            on_press,
            accent: false,
            lead: false,
            danger: false,
            anchor: None,
        }
    }

    /// The same item with its icon in the accent colour ([`RailItem::accent`]).
    pub fn accent(self) -> Self {
        Self { accent: true, ..self }
    }

    /// The same item as the mail's main action ([`RailItem::lead`]).
    pub fn lead(self) -> Self {
        Self { lead: true, ..self }
    }

    /// The same item as one that cannot be taken back at once ([`RailItem::danger`]).
    pub fn danger(self) -> Self {
        Self { danger: true, ..self }
    }

    /// The same item registering `anchor` on its row ([`RailItem::anchor`]).
    pub fn at(self, anchor: &Anchor) -> Self {
        Self { anchor: Some(anchor.clone()), ..self }
    }

    /// What a press sends: `on_press`, `None` while the item is off.
    pub fn press(&self) -> Option<M>
    where
        M: Clone,
    {
        self.on_press.clone()
    }
}

/// A group of the action rail ([`action_rail`]): the icon that stands for it in the collapsed rail,
/// its title ("File") and its items, top to bottom.
///
/// ```ignore
/// let file = RailGroup::new(Icon::Check, tr("File"), vec![done, later, archive, delete]).at(&anchors.file);
/// let mark = RailGroup::new(Icon::Flag, tr("Mark"), vec![flag, read]).tinted(attention);
/// let more = RailGroup::new(Icon::Ellipsis, tr("More"), vec![copy, show_as, colours]).foot();
/// ```
#[derive(Debug, Clone)]
pub struct RailGroup<'a, M> {
    /// The icon of the group in the collapsed rail: one fixed icon per group (task E14.3, finding 8),
    /// whatever its first item is.
    pub icon: Icon,
    /// The group's title, in the expanded rail and at the top of its open group ([`rail_flyout`]).
    pub label: iced::widget::text::Fragment<'a>,
    /// The actions, top to bottom. Leave out what does not go for the mail; an item that is
    /// there but cannot be done now is off ([`RailItem::on_press`] `None`).
    pub items: Vec<RailItem<'a, M>>,
    /// The mail's main group (Reply, task E14.3 decision A), set with [`RailGroup::accent`]: its icon
    /// in the collapsed rail is a filled red surface like the primary button, the icon in `on_accent`.
    pub accent: bool,
    /// A state of the mail the icon shows (Mark of a flagged mail: the `attention` colour), set with
    /// [`RailGroup::tinted`]: the collapsed icon in that colour.
    pub tint: Option<Color>,
    /// The group stands apart at the bottom of the rail (Mute, More), set with
    /// [`RailGroup::foot`]; its open group lines up with its icon at the bottom, not at the top.
    /// Groups keep their order within the top and within the foot.
    pub foot: bool,
    /// Registered by the group's icon in the collapsed rail, set with [`RailGroup::at`]: pass the
    /// same anchor to [`rail_flyout`], and use it for the dialogs of the group's items.
    pub anchor: Option<Anchor>,
}

impl<'a, M> RailGroup<'a, M> {
    /// A group with `icon`, `label` and `items`; at the top, not in the accent, no tint, no anchor.
    pub fn new(icon: Icon, label: impl IntoFragment<'a>, items: Vec<RailItem<'a, M>>) -> Self {
        Self { icon, label: label.into_fragment(), items, accent: false, tint: None, foot: false, anchor: None }
    }

    /// The same group as the mail's main one ([`RailGroup::accent`]).
    pub fn accent(self) -> Self {
        Self { accent: true, ..self }
    }

    /// The same group with its collapsed icon in `colour` ([`RailGroup::tint`]).
    pub fn tinted(self, colour: Color) -> Self {
        Self { tint: Some(colour), ..self }
    }

    /// The same group at the bottom of the rail ([`RailGroup::foot`]).
    pub fn foot(self) -> Self {
        Self { foot: true, ..self }
    }

    /// The same group registering `anchor` on its icon ([`RailGroup::anchor`]).
    pub fn at(self, anchor: &Anchor) -> Self {
        Self { anchor: Some(anchor.clone()), ..self }
    }

    /// What a click on the group's icon sends: the message of its first item that goes – unless
    /// that one is [`RailItem::danger`] (Delete is never done by a click on "File"). `None` then, and
    /// when no item goes: the click opens the group's card instead ([`RailEvent::Open`]).
    pub fn first_press(&self) -> Option<M>
    where
        M: Clone,
    {
        self.items.iter().find(|item| item.on_press.is_some()).filter(|item| !item.danger).and_then(RailItem::press)
    }

    /// The index of the item [`RailGroup::first_press`] sends, which the open card shows lit.
    fn first_index(&self) -> Option<usize> {
        let index = self.items.iter().position(|item| item.on_press.is_some())?;
        (!self.items[index].danger).then_some(index)
    }
}

/// What the pointer and a click do to the groups of the collapsed action rail ([`action_rail`],
/// [`rail_flyout`]). The app keeps which group is open: it opens one only once the pointer rested a
/// moment on its icon (task E14.3, decision B), goes from an open group to the next at once, and
/// closes it as soon as the pointer left card and icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailEvent {
    /// The pointer came onto the icon of the group at this index of `groups`.
    Enter(usize),
    /// The pointer left that icon – also when the group's open card covers it.
    Leave(usize),
    /// A click on the icon of the group at this index that has no click of its own
    /// ([`RailGroup::first_press`] `None`): open its card now.
    Open(usize),
    /// The pointer left the open card together with its icon ([`rail_flyout`]'s `on_close`).
    Close,
}

/// How wide an open group of the collapsed rail is ([`rail_flyout`]): [`size::sidebar()`] for every
/// group, however long its words, so moving from one group to the next does not make it jump.
pub fn rail_flyout_width() -> f32 {
    size::sidebar()
}

/// The gap between the rows of the expanded rail, as in the sidebar (`torro.css`: `.sidebar{gap:2px}`,
/// `shell::SIDEBAR_GAP`), so both columns keep one rhythm.
const RAIL_GAP: f32 = 2.0;

/// The action rail at the right edge of a mail (task E14.1, `docs/prototypes/inbox/mail-aktionsleiste.html#d`):
/// a column over the mail's whole height on the `panel` surface, a `line` on its left, as wide as
/// [`RailMode::width`] ([`size::rail()`] collapsed, [`size::rail_wide()`] expanded, and the line).
///
/// - **Top, in both modes alike** (no jump on switching): the icon button `panel-right`
///   ([`size::control_lg()`], `style::button::icon`, tooltip "Expand actions" or "Collapse actions"
///   with the key `]`) [`space::s()`] in from the top and centred in the collapsed rail's width from
///   the right ([`rail_toggle_inset`]) – where the collapsed rail has its icons –, [`space::s()`]
///   below it a `line` from edge to edge. Without `on_toggle` neither (medium and
///   narrow windows, which always show the collapsed rail, task E14 decision 5).
/// - **Collapsed:** one icon per group ([`size::control_lg()`] square, [`space::xs()`] apart), the
///   groups marked [`RailGroup::foot`] at the bottom; the Reply group ([`RailGroup::accent`]) as a
///   filled red surface, a [`RailGroup::tint`] colours its icon. A click sends the group's
///   [`RailGroup::first_press`], else `on_event(RailEvent::Open(index))`; the pointer coming onto an
///   icon and leaving it sends [`RailEvent::Enter`] and [`RailEvent::Leave`] with the index into
///   `groups`. The group at `active` – open beside its icon ([`rail_flyout`], a layer over the
///   window whose `on_close` is `on_event(RailEvent::Close)`) or with a menu or dialog of its own
///   open – stands `selected`.
/// - **Expanded:** every group in the rhythm of the sidebar: rows of [`size::row_sidebar()`],
///   [`RAIL_GAP`] apart (`style::button::ghost`: icon, word, [`kbd`] at the right), titles as
///   [`sidebar_group_in`]; the first group and groups of a single item without a title (the
///   latter with the title's room above). `active` and `on_event` are not used.
///
/// The top groups scroll when the window is too low for them, the scrollbar in the rail's right
/// edge beside the keys; [`space::s()`] parts them from the foot, which stays.
///
/// ```ignore
/// let toggle = (class == SizeClass::Wide).then_some(Message::ToggleActions);
/// let rail = widgets::action_rail(groups.clone(), mode, self.rail_open, Message::Rail, toggle);
/// // the layer, over the window:
/// let layer = self.rail_open.and_then(|i| groups.get(i).cloned()).map(|group| {
///     let anchor = group.anchor.clone().unwrap_or_default();
///     widgets::rail_flyout(group, &anchor, Message::Rail(RailEvent::Close))
/// });
/// ```
pub fn action_rail<'a, M: Clone + 'a>(
    groups: Vec<RailGroup<'a, M>>,
    mode: RailMode,
    active: Option<usize>,
    on_event: fn(RailEvent) -> M,
    on_toggle: Option<M>,
) -> Element<'a, M> {
    action_rail_toggle(groups, mode, active, on_event, on_toggle.into())
}

/// The toggle at the top of the action rail ([`action_rail_toggle`], issue #291): none, off or working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailToggle<M> {
    /// No head – no toggle and no line below it: medium and narrow windows, which always show the
    /// collapsed rail (task E14 decision 5).
    Hidden,
    /// The head with the toggle off (as any button that is off: faded, no hover, no key in its
    /// tooltip), the tooltip saying why – `tr("Widen the window to expand actions")` where the rail
    /// cannot expand. The head stays, so the icons below it do not move.
    Off(&'static str),
    /// The head with the working toggle, which sends this message.
    On(M),
}

impl<M> From<Option<M>> for RailToggle<M> {
    /// The toggle of [`action_rail`]'s `on_toggle`: none without a message, else a working one.
    fn from(on_toggle: Option<M>) -> RailToggle<M> {
        on_toggle.map_or(RailToggle::Hidden, RailToggle::On)
    }
}

impl<M> RailToggle<M> {
    /// Whether the rail draws its head (the toggle and the line below it): off or on, not hidden.
    pub fn has_head(&self) -> bool {
        !matches!(self, RailToggle::Hidden)
    }
}

/// [`action_rail`] with the toggle as a [`RailToggle`] (issue #291): [`RailToggle::Off`] draws the head
/// with the toggle off and its reason as the tooltip – where an expanded rail would squeeze the reader –,
/// so the groups stand where they stand with a working toggle, in every design.
pub fn action_rail_toggle<'a, M: Clone + 'a>(
    groups: Vec<RailGroup<'a, M>>,
    mode: RailMode,
    active: Option<usize>,
    on_event: fn(RailEvent) -> M,
    toggle: RailToggle<M>,
) -> Element<'a, M> {
    let (foot, top): (Vec<_>, Vec<_>) = groups.into_iter().enumerate().partition(|(_, group)| group.foot);
    let mut rail = column![].width(Fill).height(Fill);
    if toggle.has_head() {
        let (press, hint): (Option<M>, Element<'a, M>) = match toggle {
            RailToggle::On(message) => {
                let word = match mode {
                    RailMode::Collapsed => tr("Expand actions"),
                    RailMode::Expanded => tr("Collapse actions"),
                };
                let hint = row![typo::text(typo::caption(), word).style(theme::text_2), kbd("]")]
                    .spacing(space::s())
                    .align_y(Alignment::Center);
                (Some(message), hint.into())
            }
            RailToggle::Off(why) => (None, typo::text(typo::caption(), why).style(theme::text_2).into()),
            RailToggle::Hidden => (None, blank().into()),
        };
        let mark = button_icon(Icon::PanelRight, icon::SIZE, style::button::icon, press.is_some());
        let button = button(center(mark))
            .width(size::control_lg())
            .height(size::control_lg())
            .padding(0)
            .style(style::button::icon)
            .on_press_maybe(press);
        let button = container(tip(button, hint, tooltip::Position::Left)).width(Fill).align_right(Fill);
        let inset = Padding::new(space::s()).left(0.0).right(rail_toggle_inset());
        rail = rail.push(container(button).padding(inset)).push(rule::horizontal(LINE).style(style::rule));
    }
    let edge = Padding::new(0.0).horizontal(space::s());
    let (top, foot): (Element<'a, M>, Element<'a, M>) = match mode {
        RailMode::Collapsed => {
            let icons = |groups: Vec<(usize, RailGroup<'a, M>)>| {
                let faces = groups.into_iter().map(|(at, group)| {
                    let press = group.first_press().unwrap_or_else(|| on_event(RailEvent::Open(at)));
                    let face = mouse_area(rail_face(&group, active == Some(at), press))
                        .on_enter(on_event(RailEvent::Enter(at)))
                        .on_exit(on_event(RailEvent::Leave(at)));
                    anchored_if(group.anchor.as_ref(), face)
                });
                column(faces).spacing(space::xs()).width(Fill).align_x(Alignment::Center).into()
            };
            (icons(top), icons(foot))
        }
        RailMode::Expanded => {
            let first = top.first().map(|(at, _)| *at);
            let foot_first = foot.first().map(|(at, _)| *at);
            let sections = |groups: Vec<(usize, RailGroup<'a, M>)>, first: Option<usize>| {
                let rows = groups.into_iter().flat_map(|(at, group)| rail_section(group, Some(at) == first));
                column(rows).spacing(RAIL_GAP).width(Fill).into()
            };
            (sections(top, first), sections(foot, foot_first))
        }
    };
    let top = scrollable(container(top).width(Fill).padding(edge))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill);
    let body = column![top, blank().height(space::s()), container(foot).width(Fill).padding(edge)]
        .height(Fill)
        .padding(Padding::new(0.0).vertical(space::s()));
    let rail = rail.push(body);
    container(row![rule::vertical(LINE).style(style::rule), container(rail).width(mode.column()).height(Fill)])
        .width(mode.width())
        .height(Fill)
        .style(style::container::panel_flush)
        .into()
}

/// How far the toggle of the action rail stands from the rail's right edge: centred in the collapsed
/// rail's width, like the icons below it – [`space::s()`] when the rail is the button and 8 at either
/// side (as the suite had it), 10 in the standard design (60 and 40).
fn rail_toggle_inset() -> f32 {
    (crate::shell::rail() - size::control_lg()) / 2.0
}

/// The rows of a group of the expanded rail ([`action_rail`]): its title as in the sidebar
/// ([`sidebar_group_in`]) unless it is the `first` of its part or holds a single item – that one
/// gets the title's room above it alone –, then its rows.
fn rail_section<'a, M: Clone + 'a>(group: RailGroup<'a, M>, first: bool) -> Vec<Element<'a, M>> {
    let mut rows: Vec<Element<'a, M>> = Vec::new();
    if !first {
        if group.items.len() > 1 {
            rows.push(sidebar_group_in(SidebarMode::Expanded, group.label, None));
        } else {
            // The title's room above it without the title; the rows' gap is added around it.
            rows.push(blank().height(space::m() - RAIL_GAP).into());
        }
    }
    rows.extend(group.items.into_iter().map(|item| rail_row(item, RailPlace::Column, false)));
    rows
}

/// An open group of the collapsed rail (task E14.1): a layer over the window like [`floating`] –
/// pass it to [`crate::shell::Shell::overlay`] (or [`crate::panel::place`]'s `layer`), never into a
/// narrower part of the window, where it would be squeezed and miss its icon – with the group's
/// card left of its icon (the one that registered `anchor`, [`RailGroup::at`]), ending
/// [`space::xs()`] before the rail's line ([`flyout_gap`]): for a group at the top its first row
/// level with the icon ([`flyout_lift`]), for a [`RailGroup::foot`] group the bottom edges in line;
/// kept inside the window. The card is [`rail_flyout_width()`] wide whatever the group, on the menu
/// surface with its frame and shadow (`style::container::menu`), the group's title on top as in a
/// [`menu`] section and its items as menu entries (icon, word, [`kbd`]); the item a click on the
/// icon does ([`RailGroup::first_press`]) stands lit, as under the keyboard cursor.
///
/// The layer covers the group's icon with an invisible area, so a click there still does what the
/// icon does and the icon (drawn `selected` by the rail, `active`) shows through; when the pointer
/// leaves card and icon together, it sends `on_close`. A click on an item sends the item's message –
/// the app closes the layer then too. No scrim and no catcher: a click beside it reaches the window,
/// and the other icons of the rail stay under the pointer, so moving down the rail opens the next
/// group. In a window too low for the card at its icon the card moves in from the edge, and the
/// invisible area with it.
pub fn rail_flyout<'a, M: Clone + 'a>(group: RailGroup<'a, M>, anchor: &Anchor, on_close: M) -> Element<'a, M> {
    let foot = group.foot;
    let press = group.first_press();
    let lit = group.first_index();
    let title = typo::text(typo::caption(), group.label).style(theme::text_3).wrapping(Wrapping::None);
    let rows = group.items.into_iter().enumerate().map(|(i, item)| rail_row(item, RailPlace::Card, lit == Some(i)));
    let card = container(column![container(title).padding(flyout_title_padding())].extend(rows))
        .width(rail_flyout_width())
        .padding(style::MENU_INSET)
        .style(style::container::menu);
    let area = mouse_area(blank().width(size::control_lg()).height(size::control_lg()));
    let area = match press {
        Some(press) => area.on_press(press).interaction(mouse::Interaction::Pointer),
        None => area,
    };
    // Lifted for a top group: the icon's area lies as far below the card's top.
    let lift = if foot { 0.0 } else { flyout_lift() };
    let cover = column![blank().height(lift), opaque(area)];
    // The card is opaque, so the window below it takes no clicks; the room beside the icon is not,
    // so the rail's other icons keep their hover there.
    let layer =
        row![opaque(card), cover].spacing(flyout_gap()).align_y(if foot { Alignment::End } else { Alignment::Start });
    let anchor = anchor.clone();
    float(mouse_area(layer).on_exit(on_close))
        .translate(move |bounds, viewport| match anchor.get() {
            Some(target) => flyout_origin(target, bounds.size(), foot, viewport) - bounds.position(),
            None => centre(bounds.size(), viewport) - bounds.position(),
        })
        .into()
}

/// The padding of the title of an open group, as a [`menu`] section's.
fn flyout_title_padding() -> Padding {
    Padding::new(space::s()).bottom(space::xs())
}

/// How far an open top group's card reaches above its icon: its inset and title, less half the
/// difference between the icon and a row – so its first row stands level with the icon.
fn flyout_lift() -> f32 {
    let title = flyout_title_padding();
    let title = title.top + typo::line_height(typo::caption()) + title.bottom;
    style::MENU_INSET + title - (size::control_lg() - style::MENU_ITEM_HEIGHT) / 2.0
}

/// The gap between an open group's card and its icon: the icon stands [`space::s()`] right of the
/// rail's line (and the line), the card ends [`space::xs()`] left of it.
fn flyout_gap() -> f32 {
    space::s() + LINE + space::xs()
}

/// The top-left corner of a [`rail_flyout`] layer of `layer` size whose icon covers `target`: the
/// right edges in line, the tops – or for a `foot` group the bottoms –, then moved in to keep
/// [`beside_gap()`] from the top and bottom of `viewport`.
fn flyout_origin(target: Rectangle, layer: Size, foot: bool, viewport: Rectangle) -> Point {
    let y = if foot { target.y + target.height - layer.height } else { target.y - flyout_lift() };
    let (low, high) = (viewport.y + beside_gap(), viewport.y + viewport.height - layer.height - beside_gap());
    let y = if high < low { low } else { y.clamp(low, high) };
    Point::new(target.x + target.width - layer.width, y)
}

/// The icon of `group` in the collapsed rail: [`size::control_lg()`] square, icon 16 in `text_2` –
/// in `on_accent` on the filled red surface of the main group ([`RailGroup::accent`], decision A),
/// in the group's [`RailGroup::tint`] –, `selected` while its group is `open` (its card or one of
/// its menus); a click sends `press`.
fn rail_face<'a, M: Clone + 'a>(group: &RailGroup<'a, M>, open: bool, press: M) -> Element<'a, M> {
    let (accent, tint) = (group.accent, group.tint);
    let mark: Element<'a, M> = icon(group.icon, icon::SIZE)
        .style(move |t, _| {
            let colour = match tint {
                _ if accent => palette(t).on_accent,
                Some(tint) => tint,
                None => style::button::icon(t, button::Status::Active).text_color,
            };
            svg::Style { color: Some(colour) }
        })
        .into();
    button(center(mark))
        .width(size::control_lg())
        .height(size::control_lg())
        .padding(0)
        .style(move |t, status| match (accent, open) {
            (true, true) => style::button::primary(t, button::Status::Hovered),
            (true, false) => style::button::primary(t, status),
            (false, true) => style::button::sidebar_item(true)(t, status),
            (false, false) => style::button::icon(t, status),
        })
        .on_press(press)
        .into()
}

/// One row of the rail like a menu entry: the icon (16) in `tone` (the row's text colour at rest)
/// or the accent, the word in `typo::body()` – for the [`RailItem::lead`] the icon in the accent and
/// the word strong in `text` –, the key as [`kbd`] at the end while the item is on. Expanded it is a
/// row of the sidebar's height in `ghost` ([`RailPlace::Column`]), in an open group a menu entry
/// ([`RailPlace::Card`]), `lit` as under the keyboard cursor. Registers the item's anchor.
fn rail_row<'a, M: Clone + 'a>(item: RailItem<'a, M>, place: RailPlace, lit: bool) -> Element<'a, M> {
    let tone = match place {
        RailPlace::Column => Tone::Text2,
        RailPlace::Card => Tone::Text,
    };
    let on = item.on_press.is_some();
    let accent = item.accent || item.lead;
    let mark = icon(item.icon, icon::SIZE)
        .style(move |t, _| {
            let p = palette(t);
            svg::Style { color: Some(if accent { p.accent } else { tone.of(&p) }) }
        })
        .opacity(icon_opacity(on));
    let word: Element<'a, M> = if item.lead {
        let word = typo::text(typo::body_strong(), item.label).style(theme::text).wrapping(Wrapping::None);
        container(word).width(Fill).clip(true).into()
    } else {
        line_label(typo::body(), item.label)
    };
    let mut content = row![mark, word].spacing(space::s()).align_y(Alignment::Center);
    if on && !item.key.is_empty() {
        content = content.push(kbd(item.key));
    }
    let row = button(container(content).center_y(Fill)).width(Fill).padding(style::ITEM_PADDING);
    let row = match place {
        RailPlace::Column => row.height(size::row_sidebar()).style(style::button::ghost),
        RailPlace::Card => row.height(style::MENU_ITEM_HEIGHT).style(style::button::menu_item(lit)),
    };
    anchored_if(item.anchor.as_ref(), row.on_press_maybe(item.on_press))
}

/// Where a row of the rail stands ([`rail_row`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailPlace {
    /// In the expanded rail on the `panel` surface: as high as a sidebar row (the rail is a column
    /// as wide as the sidebar, and 17 actions with their titles must fit a window's height), `ghost`
    /// – `text_2`, on hover `hover` and `text`.
    Column,
    /// In an open group on the `menu` surface: a menu entry, [`style::MENU_ITEM_HEIGHT`] high,
    /// `menu_item` – `text`, on hover `selected`.
    Card,
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

/// A clickable card with a reserved mark slot inside its left padding. Mail cards use the slot
/// for the unread dot; an empty mark keeps read cards at precisely the same width and height.
/// The mark belongs to the card button, so its hover and click area is continuous.
pub fn card_with_inset_mark<'a, M: Clone + 'a>(
    parts: Vec<Element<'a, M>>,
    selected: bool,
    on_press: M,
    mark: Element<'a, M>,
) -> Element<'a, M> {
    let pad = layout::panel_padding();
    let head_height = size::avatar().max(typo::line_height(typo::body_strong()) + typo::line_height(typo::meta()));
    let slot = container(mark).width(card_mark_inset()).height(head_height).align_x(iced::Center).align_y(iced::Center);
    let content = row![slot, column(parts).spacing(size::card_gap()).width(Fill)].align_y(Alignment::Start);
    button(content)
        .width(Fill)
        .padding(Padding { left: 0.0, ..pad })
        .style(card_style(selected))
        .on_press(on_press)
        .into()
}

/// The left inset of a card with an unread dot: at least one dot and a small gap on both sides.
/// Mail cards reserve it in both read states, including when a design sets panel padding to zero.
pub fn card_mark_inset() -> f32 {
    card_mark_inset_for(layout::panel_padding().left)
}

fn card_mark_inset_for(left: f32) -> f32 {
    left.max(space::s() + 2.0 * space::xs())
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
    card_head_with_level(lead, title, meta, typo::body_strong(), dismiss)
}

/// A card head whose title may use a lighter level, for read mail.
pub fn card_head_with_level<'a, M: Clone + 'a>(
    lead: Option<Element<'a, M>>,
    title: impl IntoFragment<'a>,
    meta: impl IntoFragment<'a>,
    title_level: typo::Level,
    dismiss: Option<M>,
) -> Element<'a, M> {
    let mut words = column![one_line(title_level, title, theme::text)];
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
/// `docs/kategorien.md`): the category's monochrome icon and words, tinted by its group's colour.
///
/// `group` is the index of `torro_mail::CategoryGroup` (0 waiting for me, 3 unwanted, 4 unsorted)
/// and picks the colour out of [`theme::Palette::category`]; a larger index falls back to the last. The colour
/// belongs to the group; the icon and words belong to the category.
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
    let at = category_index(group);
    let mut content = row![].spacing(ICON_GAP).align_y(Alignment::Center);
    if suggested {
        content = content.push(sparkle(pill_icon()));
    }
    content = content.push(category_icon(icon, at)).push(typo::text(typo::caption(), label).wrapping(Wrapping::None));
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

/// The small mark of a mail's category for a row of the list (task E12.8): its icon in the group's
/// colour, `edge` wide. The words belong in its tooltip – a row's marks are icons, so
/// the whole pill ([`category_pill`]) stays for the reader and the card, where there is room.
pub fn category_mark<'a, M: 'a>(which: Icon, group: usize, edge: f32) -> Element<'a, M> {
    let at = category_index(group);
    icon(which, edge).style(move |t, _| svg::Style { color: Some(palette(t).category[at]) }).into()
}

/// The colour of group `group` in [`theme::Palette::category`]: a larger index than the palette
/// holds takes its last colour.
fn category_index(group: usize) -> usize {
    group.min(CATEGORY_COLOURS - 1)
}

/// How many group colours [`theme::Palette::category`] holds, one per `torro_mail::CategoryGroup`.
const CATEGORY_COLOURS: usize = 5;

/// The icon of a category in its group's colour ([`category_pill`]).
fn category_icon<'a, M: 'a>(which: Icon, group: usize) -> Element<'a, M> {
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
/// menu). [`size::header()`] high, 12 more with a detail; the sides of [`layout::row_padding`], the
/// content centred in the height.
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
        .padding(group_row_padding(layout::row_padding()))
        .into()
}

/// The padding of a [`group_row`] from the row padding `pad`: only its sides. The row has a fixed
/// height and centres its content, so padding above and below would only squeeze the control (18
/// above and below a row of 64 left a button of 36 only 28).
fn group_row_padding(pad: Padding) -> Padding {
    Padding::new(0.0).left(pad.left).right(pad.right)
}

/// A heading over a block of a settings page with quiet actions at its right (task L.18: "Design"
/// with "Reload" and "Open designs folder"): `title` in the look of a [`group`] title (`CAPTION` in
/// `text_3`), the `actions` right-aligned, [`space::xs()`] apart and centred on the title. Pass
/// quiet buttons – [`icon_button`] or a [`text_button`] in `style::button::ghost` –, never a
/// primary one: they belong to the block, not to the page. With actions the heading is as high as
/// they are (28), without it is the title alone. Put the block's content under it with
/// [`layout::item_gap()`], as [`group`] does.
pub fn group_heading<'a, M: 'a>(title: impl IntoFragment<'a>, actions: Vec<Element<'a, M>>) -> Element<'a, M> {
    let title = typo::text(typo::caption(), title).style(theme::text_3);
    if actions.is_empty() {
        return title.into();
    }
    let actions = row(actions).spacing(space::xs()).align_y(Alignment::Center);
    row![container(title).width(Fill).clip(true), actions].spacing(space::s()).align_y(Alignment::Center).into()
}

// ---------------------------------------------------------------------------------------------
// Detail pages (tasks DS.3, DS.4): the third level of the settings, one object on its own page
// ---------------------------------------------------------------------------------------------

/// A button described as data, so a block can lay it out again when its width changes (the head of
/// a detail page puts its buttons beside or under the name, a question card beside or under its
/// sentence). [`PageAction::new`] makes a `secondary` one, as the design skill wants for the one
/// visible action of a page.
#[derive(Debug, Clone)]
pub struct PageAction<M> {
    icon: Option<Icon>,
    label: String,
    tip: Option<String>,
    style: ButtonStyle,
    on_press: Option<M>,
}

impl<M: Clone> PageAction<M> {
    /// A `secondary` button with `label`; `None` for `on_press` shows it off.
    pub fn new(label: impl Into<String>, on_press: Option<M>) -> Self {
        Self { icon: None, label: label.into(), tip: None, style: style::button::secondary, on_press }
    }

    /// An icon (14) in front of the label.
    pub fn icon(mut self, which: Icon) -> Self {
        self.icon = Some(which);
        self
    }

    /// A tooltip that says more than the label ("Syncs all accounts").
    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// Another style of [`crate::style::button`] (`ghost` for an action that only lets go).
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// The width the button takes ([`text_button_width`]).
    pub fn width(&self) -> f32 {
        text_button_width(self.icon.is_some(), &self.label)
    }

    /// The button with only its icon (the label and the tip as tooltip), for a head too narrow for
    /// the word; without an icon it keeps its word.
    fn view_compact<'a>(self) -> Element<'a, M>
    where
        M: 'a,
    {
        let Some(which) = self.icon else { return self.view() };
        let tip = match &self.tip {
            Some(tip) => format!("{} · {tip}", self.label),
            None => self.label.clone(),
        };
        icon_only(which, tip, style::button::boxed, self.on_press)
    }

    fn view<'a>(self) -> Element<'a, M>
    where
        M: 'a,
    {
        let button = text_button(self.icon, self.label, self.style, self.on_press);
        match self.tip {
            Some(tip) => with_tooltip(button, tip),
            None => button,
        }
    }
}

/// What the head of a detail page offers at its right: at most one visible action and the "…"
/// button, whose menu the caller opens at the [`Anchor`] with [`floating`] (the rest of the
/// object's actions, design skill: one visible action, the rest in "…").
#[derive(Debug, Clone)]
pub struct HeadActions<M> {
    /// The one visible action ("Sync now").
    pub action: Option<PageAction<M>>,
    /// The "…" button: where its menu hangs and what a click on it sends.
    pub more: Option<(Anchor, M)>,
}

impl<M> Default for HeadActions<M> {
    fn default() -> Self {
        Self { action: None, more: None }
    }
}

impl<M: Clone> HeadActions<M> {
    /// The buttons; `compact` shows the action by its icon only (a head in a mini tile).
    fn view<'a>(self, compact: bool) -> Vec<Element<'a, M>>
    where
        M: 'a,
    {
        let mut out = Vec::new();
        if let Some(action) = self.action {
            out.push(if compact { action.view_compact() } else { action.view() });
        }
        if let Some((anchor, message)) = self.more {
            out.push(anchored(&anchor, icon_button(Icon::Ellipsis, tr("More"), Some(message))));
        }
        out
    }
}

/// The head of a detail page (task DS.3, block 1 of `docs/tasks/detailseiten.md`): a large
/// [`source_tile`] with `lead`, the object's `title` in `TITLE`, under it one `meta` line ("Anna ·
/// IMAP mail.example.org:993") in `META` and `text_3` – both on one line, cut with "…" and then
/// whole as tooltip –, at the right a `status` pill (only where the state says something: "Synced
/// today 15:39", a sync problem) and the [`HeadActions`]. It replaces a group "Status" and groups
/// that only held "Open X".
///
/// Narrower than [`size::form_max()`] the pill and the buttons move under the meta line, so the
/// name keeps its room.
///
/// ```ignore
/// let more = Anchor::new();
/// let actions = HeadActions { action: Some(PageAction::new(tr("Sync now"), Some(Message::Sync))), more: Some((more.clone(), Message::More)) };
/// widgets::detail_head(TileFace::letter(&address), address, meta, Some((Status::Ok, synced)), actions)
/// ```
pub fn detail_head<'a, M: Clone + 'a>(
    lead: TileFace,
    title: impl Into<String>,
    meta: impl Into<String>,
    status: Option<(Status, String)>,
    actions: HeadActions<M>,
) -> Element<'a, M> {
    let (title, meta) = (title.into(), meta.into());
    responsive(move |room| {
        let tile = source_tile(lead, TileSize::Large, None);
        let mut words = column![one_line(typo::title(), title.clone(), theme::text)];
        if !meta.is_empty() {
            words = words.push(one_line(typo::meta(), meta.clone(), theme::text_3));
        }
        let mut side: Vec<Element<'a, M>> = Vec::new();
        if let Some((state, label)) = &status {
            side.push(status_pill(*state, label.clone()));
        }
        // So narrow that the word would push "…" onto a line of its own: the action as an icon.
        side.extend(actions.clone().view(room.width < size::panel()));
        if head_is_wide(room.width) {
            let mut head = row![tile, words.width(Fill)].spacing(space::m()).align_y(Alignment::Center);
            if !side.is_empty() {
                head = head.push(row(side).spacing(space::s()).align_y(Alignment::Center));
            }
            head.into()
        } else {
            if !side.is_empty() {
                words = words.push(row(side).spacing(space::s()).align_y(Alignment::Center).wrap());
                words = words.spacing(space::xs());
            }
            row![tile, words.width(Fill)].spacing(space::m()).align_y(Alignment::Start).into()
        }
    })
    .height(Length::Shrink)
    .into()
}

/// Whether the head of a detail page has the room for its pill and buttons beside the name.
fn head_is_wide(width: f32) -> bool {
    width >= size::form_max()
}

/// A state in words with its dot in the colour of `status` (the pill of [`detail_head`]): as
/// [`state_pill`], the colour only in the dot.
fn status_pill<'a, M: 'a>(status: Status, label: String) -> Element<'a, M> {
    let content = row![status_dot(status), typo::text(typo::caption(), label).wrapping(Wrapping::None)]
        .spacing(ICON_GAP)
        .align_y(Alignment::Center);
    container(content)
        .center_y(STATE_PILL_HEIGHT)
        .padding(Padding::new(0.0).left(space::s()).right(space::s()))
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

/// The strip under the chosen tab.
const TAB_MARK: f32 = 2.0;

/// One tab as the pick list shows it when the tabs do not fit: its label and count, and its index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabChoice {
    /// Where the tab stands in the row.
    pub index: usize,
    label: String,
}

impl std::fmt::Display for TabChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The tabs as the choices of the pick list ("Folders 12").
pub fn tab_choices(labels: &[(String, Option<u64>)]) -> Vec<TabChoice> {
    labels
        .iter()
        .enumerate()
        .map(|(index, (label, count))| {
            let label = match count {
                Some(n) => format!("{label} {}", crate::units::format_count(*n)),
                None => label.clone(),
            };
            TabChoice { index, label }
        })
        .collect()
}

/// The width a row of [`tabs`] needs: each tab's padding, its word in `BODY_STRONG` (the chosen
/// look, so choosing one never makes the row too wide) and its count in `META`, and the gaps.
pub fn tabs_width(labels: &[(String, Option<u64>)]) -> f32 {
    let tabs: f32 = labels
        .iter()
        .map(|(label, count)| {
            let count = count.map_or(0.0, |n| {
                space::xs() + typo::text_width(&crate::units::format_count(n), typo::meta().tabular())
            });
            2.0 * space::s() + typo::text_width(label, typo::body_strong()) + count
        })
        .sum();
    tabs + space::xs() * labels.len().saturating_sub(1) as f32
}

/// Tabs of a detail page (task DS.3, block 2): the parts of one object ("Overview · Writing ·
/// Folders · Backup"), only for a page with more than three groups, "Overview" first. Each tab is
/// its `label` and an optional count at its right (`META`, `text_3`, tabular); the chosen one at
/// `selected` is `TEXT` in `BODY_STRONG` with a 2-px strip in `TEXT` under it, the others `TEXT_2`.
/// A `line` runs under the whole row. Never red, and no [`segmented`]: a segmented control changes
/// the view of the same data, tabs change the part of an object.
///
/// When the row does not fit ([`tabs_width`]), the tabs become a pick list – never a row that
/// scrolls sideways. `on_select` gets the index of the tab.
///
/// ```ignore
/// let labels = vec![(tr("Overview").to_string(), None), (tr("Folders").to_string(), Some(12))];
/// widgets::tabs(labels, state.tab, Message::Tab)
/// ```
pub fn tabs<'a, M: Clone + 'a>(
    labels: Vec<(String, Option<u64>)>,
    selected: usize,
    on_select: fn(usize) -> M,
) -> Element<'a, M> {
    responsive(move |room| {
        if room.width >= tabs_width(&labels) {
            let line = rule::horizontal(LINE).style(style::rule);
            let row = row(labels
                .iter()
                .enumerate()
                .map(|(i, (label, count))| tab(label.clone(), *count, i == selected, on_select(i))))
            .spacing(space::xs());
            column![row, line].into()
        } else {
            let choices = tab_choices(&labels);
            let chosen = choices.get(selected).cloned();
            let pick = iced::widget::pick_list(choices, chosen, move |choice: TabChoice| on_select(choice.index))
                .padding(style::pick_list_padding())
                .text_size(typo::body().size)
                .text_line_height(typo::body().line_height)
                .font(typo::body().font)
                .style(style::pick_list)
                .menu_style(style::pick_list_menu);
            // A pick list needs no line under it; it would read as a row of tabs without tabs.
            pick.into()
        }
    })
    .height(Length::Shrink)
    .into()
}

/// One of the [`tabs`].
fn tab<'a, M: Clone + 'a>(label: String, count: Option<u64>, on: bool, message: M) -> Element<'a, M> {
    let level = if on { typo::body_strong() } else { typo::body() };
    let mut words =
        row![typo::text(level, label).wrapping(Wrapping::None)].spacing(space::xs()).align_y(Alignment::Center);
    if let Some(n) = count {
        let count = typo::text(typo::meta().tabular(), crate::units::format_count(n)).style(theme::text_3);
        words = words.push(count.wrapping(Wrapping::None));
    }
    // The strip is a second layer of a stack: the first layer (the words) sets the tab's size, so
    // a strip as wide as the tab does not stretch the tab to the row.
    let strip = container(blank().height(TAB_MARK)).width(Fill).style(move |t: &Theme| container::Style {
        background: on.then(|| Background::Color(palette(t).text)),
        ..container::Style::default()
    });
    let face = stack![
        container(words).center_y(size::control_lg()).padding([0.0, space::s()]),
        container(strip).height(Fill).align_bottom(Fill),
    ];
    button(face)
        .padding(0)
        .style(move |t: &Theme, status| {
            let p = palette(t);
            let lit = on || matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style { text_color: if lit { p.text } else { p.text_2 }, ..button::Style::default() }
        })
        .on_press(message)
        .into()
}

/// An open question on a detail page (task DS.3, block 6), right under the tabs and only while
/// something waits for the person: open folder roles, tools waiting to be allowed, a missing
/// secret, a sync problem. On `raised` with a `line_strong` frame and radius `sm`, the padding of
/// a panel; `mark` in the attention colour, `title` in `BODY_STRONG`, the `detail` sentence in
/// `text_2`, the `actions` at the right – or under the sentence, wrapping, where the card is too
/// narrow for both. Several questions stand one under the other, [`size::card_gap()`] apart.
///
/// ```ignore
/// widgets::question_card(Icon::Folder, tr("Which folder holds sent mail?"), detail, vec![PageAction::new(tr("Use Sent"), Some(Message::Pick(…)))])
/// ```
pub fn question_card<'a, M: Clone + 'a>(
    mark: Icon,
    title: impl Into<String>,
    detail: impl Into<String>,
    actions: Vec<PageAction<M>>,
) -> Element<'a, M> {
    let (title, detail) = (title.into(), detail.into());
    let card = responsive(move |room| {
        // The mark stands on the title line, wide or narrow, so it never jumps with the layout.
        let glyph: Element<'a, M> =
            icon(mark, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).attention) }).into();
        let mark: Element<'a, M> = container(glyph).center_y(typo::line_height(typo::body_strong())).into();
        let mut words = column![typo::text(typo::body_strong(), title.clone())].spacing(space::xs());
        if !detail.is_empty() {
            words = words.push(typo::text(typo::body(), detail.clone()).style(theme::text_2));
        }
        let buttons = row(actions.iter().cloned().map(PageAction::view)).spacing(space::s()).align_y(Alignment::Center);
        if actions.is_empty() {
            row![mark, words.width(Fill)].spacing(space::m()).into()
        } else if question_is_wide(room.width, &actions) {
            row![mark, words.width(Fill), buttons].spacing(space::m()).into()
        } else {
            row![mark, words.push(buttons.wrap()).width(Fill)].spacing(space::m()).into()
        }
    })
    .height(Length::Shrink);
    container(card)
        .width(Fill)
        .padding(layout::panel_padding())
        .style(|t| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.raised)),
                text_color: Some(p.text),
                border: border::rounded(radius::sm()).color(p.line_strong).width(LINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// Whether a question card of `width` keeps its buttons beside the sentence: the sentence keeps at
/// least the width of the properties column.
fn question_is_wide<M: Clone>(width: f32, actions: &[PageAction<M>]) -> bool {
    let buttons: f32 =
        actions.iter().map(PageAction::width).sum::<f32>() + space::s() * actions.len().saturating_sub(1) as f32;
    width - buttons >= size::props()
}

/// A figure for [`stat_grid`]: what [`stat_tile`] takes, as data, so the grid can lay its tiles out
/// again when its width changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatFigure {
    /// What is counted ("Mail").
    pub label: String,
    /// The figure ("4,796").
    pub value: String,
    /// What it means or is measured against ("59 unread", "of 10 GB · 24%").
    pub note: String,
}

impl StatFigure {
    /// A figure with its `label`, `value` and `note`.
    pub fn new(label: impl Into<String>, value: impl Into<String>, note: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into(), note: note.into() }
    }
}

/// The narrowest a tile of [`stat_grid`] gets before the grid takes fewer per row: room for a figure
/// in `DISPLAY` ("2.36 GB") and the tile's padding, so four fit the reading width of a page.
fn stat_tile_min() -> f32 {
    size::props() / 2.0
}

/// How many tiles of [`stat_grid`] stand in a row `width` wide: all of them while each keeps
/// [`stat_tile_min`], else two, else one (the mini tile).
pub fn stat_columns(count: usize, width: f32) -> usize {
    let fits = |n: usize| n as f32 * stat_tile_min() + space::m() * n.saturating_sub(1) as f32 <= width;
    if count == 0 {
        1
    } else if fits(count) {
        count
    } else if fits(2) {
        2.min(count)
    } else {
        1
    }
}

/// The figures at the top of a detail page (task DS.4, block 3): two to four [`stat_tile`]s with
/// what they are measured against ("59 unread", "of 10 GB"). In one row where they fit, else two
/// per row, in the mini tile one; the tiles of a row share its width ([`stat_columns`]).
///
/// ```ignore
/// widgets::stat_grid(vec![StatFigure::new(tr("Mail"), format_count(stats.messages), unread), …])
/// ```
pub fn stat_grid<'a, M: 'a>(figures: Vec<StatFigure>) -> Element<'a, M> {
    responsive(move |room| {
        let per_row = stat_columns(figures.len(), room.width);
        let rows = figures.chunks(per_row).map(|chunk| {
            let mut line = row(chunk.iter().cloned().map(|f| stat_tile(f.label, f.value, f.note)));
            // An unfinished last row keeps the width of the tiles above it.
            for _ in chunk.len()..per_row {
                line = line.push(blank().width(Fill));
            }
            line.spacing(space::m()).into()
        });
        column(rows).spacing(space::m()).into()
    })
    .height(Length::Shrink)
    .into()
}

/// One part of a [`share_strip`]: what it is, how much, that amount in words ("1.2 GB") and its
/// colour. The colour goes by what the part is, never by its rank.
#[derive(Debug, Clone, PartialEq)]
pub struct SharePart {
    /// What the part is ("Search index", "INBOX").
    pub label: String,
    /// Its amount, in the unit all parts share.
    pub value: u64,
    /// The amount as people read it ("1.2 GB", "4,796").
    pub shown: String,
    /// Its colour; a fifth part and more are drawn as [`ChartColor::Other`].
    pub color: ChartColor,
}

impl SharePart {
    /// A part with its `label`, `value`, `shown` amount and `color`.
    pub fn new(label: impl Into<String>, value: u64, shown: impl Into<String>, color: ChartColor) -> Self {
        Self { label: label.into(), value, shown: shown.into(), color }
    }
}

/// The 2-px gap of `panel` between the parts of a [`share_strip`].
const SHARE_GAP: f32 = 2.0;
/// The narrowest a part of a [`share_strip`] is drawn, so a small one stays visible.
const SHARE_MIN: f32 = 2.0;

/// The colour a part of a [`share_strip`] at `index` is drawn in: its own for the first four,
/// [`ChartColor::Other`] for the rest (four series and "Other", as in the charts).
pub fn share_color(index: usize, color: ChartColor) -> ChartColor {
    if index < ChartColor::SLOTS { color } else { ChartColor::Other }
}

/// The widths of the segments of a [`share_strip`] `width` wide: one per value that is not 0, the
/// rest last; with the gaps between them they fill the width exactly. A whole of 0 gives one
/// segment, the empty track.
pub fn share_widths(values: &[u64], rest: u64, width: f32) -> Vec<f32> {
    let mut amounts: Vec<u64> = values.iter().copied().filter(|v| *v > 0).collect();
    if rest > 0 {
        amounts.push(rest);
    }
    let whole: u64 = amounts.iter().sum();
    if whole == 0 {
        return vec![width.max(0.0)];
    }
    let room = (width - SHARE_GAP * amounts.len().saturating_sub(1) as f32).max(0.0);
    let mut widths: Vec<f32> =
        amounts.iter().map(|v| (room * (*v as f32 / whole as f32)).max(SHARE_MIN.min(room))).collect();
    // The last segment takes what rounding and the minimum left, so the strip ends at the edge.
    let before: f32 = widths.iter().rev().skip(1).sum();
    if let Some(last) = widths.last_mut() {
        *last = (room - before).max(0.0);
    }
    widths
}

/// `part` of `whole` as a percentage people read: "24%", "<1%" for a small part that is not 0,
/// "0%" for none.
pub fn share_percent(part: u64, whole: u64) -> String {
    if whole == 0 || part == 0 {
        return torro_core::i18n::trf("{n}%", &[("n", &0)]);
    }
    let percent = part as f64 * 100.0 / whole as f64;
    if percent < 1.0 {
        torro_core::i18n::trf("{n}%", &[("n", &"<1")])
    } else {
        torro_core::i18n::trf("{n}%", &[("n", &(percent.round() as u64))])
    }
}

/// Parts that make up a whole, as one bar with a legend (task DS.4, block 4): storage, the states
/// of a search index, rights per area. The bar is [`space::s()`] high, each part in its
/// [`ChartColor`] with 2 px of the surface between them, `rest` ("Free") as a track in
/// `line_strong`; under it the legend – swatch, word, `shown`, share in percent – wrapping, so every
/// value reads without its colour too. At most four series, a fifth part and more draw as
/// [`ChartColor::Other`]; build the "Other" part yourself where several are left over. A whole of 0
/// shows only the track. A `rest` with an empty word draws its track without a legend entry, for
/// a rest whose number stands elsewhere on the page already. [`share_bar`] stays for a single
/// neutral share.
///
/// ```ignore
/// let parts = vec![SharePart::new(tr("Search index"), index, format_bytes(index), ChartColor::Series(0)), …];
/// widgets::share_strip(parts, Some((tr("Free").to_string(), free)))
/// ```
pub fn share_strip<'a, M: 'a>(parts: Vec<SharePart>, rest: Option<(String, u64)>) -> Element<'a, M> {
    let parts: Vec<SharePart> = parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| SharePart { color: share_color(i, part.color), ..part })
        .collect();
    let rest_value = rest.as_ref().map_or(0, |(_, value)| *value);
    let whole: u64 = parts.iter().map(|p| p.value).sum::<u64>() + rest_value;
    let colours: Vec<Option<ChartColor>> = parts.iter().filter(|p| p.value > 0).map(|p| Some(p.color)).collect();
    let values: Vec<u64> = parts.iter().map(|p| p.value).collect();
    let bar = responsive(move |room| {
        let widths = share_widths(&values, rest_value, room.width);
        let last = widths.len().saturating_sub(1);
        // A whole of 0 or the rest: the track.
        let segments = widths.iter().enumerate().map(|(i, width)| {
            let colour = colours.get(i).copied().flatten();
            let corners = border::Radius {
                top_left: if i == 0 { radius::xs() } else { 0.0 },
                bottom_left: if i == 0 { radius::xs() } else { 0.0 },
                top_right: if i == last { radius::xs() } else { 0.0 },
                bottom_right: if i == last { radius::xs() } else { 0.0 },
            };
            container(blank().width(*width).height(space::s()))
                .style(move |t: &Theme| {
                    let p = palette(t);
                    let fill = colour.map_or(p.line_strong, |c| p.chart_color(c));
                    container::Style {
                        background: Some(Background::Color(fill)),
                        border: border::Border { radius: corners, ..border::Border::default() },
                        ..container::Style::default()
                    }
                })
                .into()
        });
        row(segments).spacing(SHARE_GAP).into()
    })
    .height(Length::Shrink);
    let mut entries: Vec<Element<'a, M>> =
        parts.iter().map(|part| share_entry(Some(part.color), &part.label, &part.shown, part.value, whole)).collect();
    if let Some((label, value)) = rest.as_ref().filter(|(label, _)| !label.is_empty()) {
        entries.push(share_entry(None, label, "", *value, whole));
    }
    let legend = row(entries).spacing(space::l()).wrap().vertical_spacing(space::xs());
    column![bar, legend].spacing(space::s()).width(Fill).into()
}

/// One entry of the legend of a [`share_strip`]: swatch (or the track's colour for the rest), word
/// in `text_2`, the amount in `text` and the share in `text_3`.
fn share_entry<'a, M: 'a>(
    colour: Option<ChartColor>,
    label: &str,
    shown: &str,
    value: u64,
    whole: u64,
) -> Element<'a, M> {
    let mark: Element<'a, M> = match colour {
        Some(colour) => swatch(colour),
        // The track's colour is faint on the panel; its edge keeps the swatch visible.
        None => container(blank().width(SWATCH).height(SWATCH))
            .style(|t| container::Style {
                background: Some(Background::Color(palette(t).line_strong)),
                border: border::rounded(MARK_GAP).color(palette(t).line_selected).width(LINE),
                ..container::Style::default()
            })
            .into(),
    };
    let mut entry =
        row![mark, typo::text(typo::meta(), label.to_string()).style(theme::text_2).wrapping(Wrapping::None)]
            .spacing(ICON_GAP)
            .align_y(Alignment::Center);
    if !shown.is_empty() {
        entry = entry.push(typo::text(typo::meta().tabular(), shown.to_string()).wrapping(Wrapping::None));
    }
    entry
        .push(
            typo::text(typo::meta().tabular(), share_percent(value, whole))
                .style(theme::text_3)
                .wrapping(Wrapping::None),
        )
        .into()
}

/// A setting with what it is for (task DS.4, block 7): at the left `title` in `BODY_STRONG` and the
/// `why` sentence in `META` and `text_2`, at the right the `control` (a field, a picker, an editor),
/// the columns 1 : 1.6, aligned at the top. Narrower than [`size::form_max()`] the control moves
/// under the sentence. `control` builds the control each time the pair is laid out anew, so it is a
/// closure. Put several pairs into [`setting_pairs`].
///
/// ```ignore
/// widgets::setting_pair(tr("Sender name"), tr("Shown as the sender of mail from this account."), move || field("Anna", &draft.name).on_input(Message::Name).into())
/// ```
pub fn setting_pair<'a, M: 'a>(
    title: impl Into<String>,
    why: impl Into<String>,
    control: impl Fn() -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    let (title, why) = (title.into(), why.into());
    responsive(move |room| {
        let mut words = column![typo::text(typo::body_strong(), title.clone())].spacing(space::xs());
        if !why.is_empty() {
            words = words.push(typo::text(typo::meta(), why.clone()).style(theme::text_2));
        }
        if pair_is_wide(room.width) {
            row![
                container(words).width(Length::FillPortion(SETTING_WORDS)),
                container(control()).width(Length::FillPortion(SETTING_CONTROL)),
            ]
            .spacing(space::xl())
            .align_y(Alignment::Start)
            .into()
        } else {
            column![words, control()].spacing(space::s()).into()
        }
    })
    .height(Length::Shrink)
    .into()
}

/// The portions of the two columns of a [`setting_pair`]: 1 : 1.6.
const SETTING_WORDS: u16 = 10;
const SETTING_CONTROL: u16 = 16;

/// Whether a [`setting_pair`] `width` wide keeps its two columns.
fn pair_is_wide(width: f32) -> bool {
    width >= size::form_max()
}

/// Several [`setting_pair`]s, a `line` between them and [`layout::section_gap()`] around it.
pub fn setting_pairs<'a, M: 'a>(pairs: Vec<Element<'a, M>>) -> Element<'a, M> {
    let mut list = column![].spacing(layout::section_gap());
    for (i, pair) in pairs.into_iter().enumerate() {
        if i > 0 {
            list = list.push(rule::horizontal(LINE).style(style::rule));
        }
        list = list.push(pair);
    }
    list.width(Fill).into()
}

/// The bar that saves a page that does not save itself (task DS.4, block 8), shown only while
/// something is changed: a [`floating_bar`] with `summary` ("Signature changed") in `text_2`,
/// "Discard" (`ghost`) and "Save" (`primary`; while it saves, `busy` shows "Saving…"). `None` for
/// `discard` or `save` shows that button off. Lay it over the page with [`over_bar`] and give the
/// page [`floating_bar_clearance`]`(1)` at its end.
pub fn save_bar<'a, M: Clone + 'a>(
    summary: impl IntoFragment<'a>,
    discard: Option<M>,
    save: Option<M>,
    busy: bool,
) -> Element<'a, M> {
    let save = if busy {
        busy_button(tr("Saving…"), style::button::primary)
    } else {
        text_button(None, tr("Save"), style::button::primary, save)
    };
    let content = row![
        container(typo::text(typo::body(), summary).style(theme::text_2).wrapping(Wrapping::None))
            .padding([0.0, space::s()]),
        text_button(None, tr("Discard"), style::button::ghost, discard),
        save,
    ]
    .spacing(floating_bar_gap())
    .align_y(Alignment::Center);
    floating_bar(content)
}

/// How a [`preview_tile`] stands among the others of its grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewState {
    /// Can be chosen, is not.
    Plain,
    /// The current choice: `line_selected` frame and a check mark at the top right.
    Chosen,
    /// Can't be chosen (a design pack that breaks a rule): greyed out, [`Icon::CircleAlert`] in
    /// place of the preview. `open` while the page shows its reasons (under the grid), then it looks
    /// pressed.
    Unusable { open: bool },
}

/// The width of a [`preview_tile`]: three [`size::control()`]s (96 in the standard), so a page of
/// [`size::content_max()`] takes seven in a row and a narrow window still four.
pub fn preview_tile_width() -> f32 {
    3.0 * size::control()
}

/// The size of the preview inside a [`preview_tile`]: the tile's width less its inner margin
/// ([`space::xs()`]) and frame, [`size::control_sm()`] high (86 × 28 in the standard) – wide and
/// flat, so a row of tiles stays low. Draw the preview at this size; a picture is cut to it
/// (`ContentFit::Cover`).
pub fn preview_size() -> Size {
    Size::new(preview_tile_width() - 2.0 * (space::xs() + LINE), size::control_sm())
}

/// A tile of a grid to pick from (task L.18: the design packs under Appearance; later the fonts and
/// colour schemes): `preview` on top at [`preview_size`] (`None` leaves the place empty), `name`
/// under it on one line in `CAPTION`, cut with "…" to the tile's width. The whole tile is the
/// button (`style::button::preview_tile`), [`preview_tile_width`] wide, [`space::xs()`] inside, the
/// name [`space::xs()`] under the preview.
///
/// - [`PreviewState::Chosen`]: `selected` with a `line_selected` frame and a round check mark at
///   the top right of the preview (`style::container::tile_mark`), so the choice does not rest on
///   the frame alone.
/// - [`PreviewState::Unusable`]: greyed out – no surface, a quiet frame, the name in `text_3` – and
///   [`Icon::CircleAlert`] in `text_3` on an empty frame in place of `preview`, so it is told by a
///   symbol, not by colour alone. It stays pressable: a click is for showing its reasons.
/// - Hover lays `hover` under it and strengthens the frame; `None` for `on_press` fades it (40 %).
///   Buttons have no keyboard focus in Iced 0.14 (`crate::style`), so there is no focus ring yet.
///
/// `tip` is the tooltip ("1.2 · Anna Berg"); a name that had to be cut is put in front of it
/// ("Commitr Dark Extended · 1.2 · Anna Berg"), so it is never cut silently. Lay tiles out with
/// [`preview_grid`].
///
/// ```ignore
/// let state = if pack.id == chosen { PreviewState::Chosen } else { PreviewState::Plain };
/// widgets::preview_tile(Some(sample(pack)), &pack.name, Some(byline), state, Some(Message::Design(pack.id.clone())))
/// ```
pub fn preview_tile<'a, M: Clone + 'a>(
    preview: Option<Element<'a, M>>,
    name: &str,
    tip: Option<String>,
    state: PreviewState,
    on_press: Option<M>,
) -> Element<'a, M> {
    preview_tile_noted(preview, name, None, tip, state, on_press)
}

/// [`preview_tile`] with `note` (task L.23: "Suggested" on the font or colour scheme the chosen
/// design suggests): one word on a line of its own under the name, in `CAPTION` and `text_3`, cut
/// with "…" to the tile's width like the name. It sits under the name rather than on the preview,
/// so it never covers the sample, which is only [`size::control_sm()`] high; the tile grows by one
/// `CAPTION` line and [`space::xs()`] (20 in the standard), and [`preview_grid`] aligns the tiles of
/// a row at the top. `None` is exactly [`preview_tile`].
///
/// The note is a mark, not the explanation: say in `tip` what it means ("Suggested for Commitr ·
/// 1.0 · IBM"), because the word alone does not name the design.
///
/// ```ignore
/// widgets::preview_tile_noted(Some(sample), "IBM Plex Sans", Some(tr("Suggested")), Some(tip), PreviewState::Plain, Some(Message::Font(id)))
/// ```
pub fn preview_tile_noted<'a, M: Clone + 'a>(
    preview: Option<Element<'a, M>>,
    name: &str,
    note: Option<&str>,
    tip: Option<String>,
    state: PreviewState,
    on_press: Option<M>,
) -> Element<'a, M> {
    let area = preview_size();
    let (on, usable) = match state {
        PreviewState::Plain => (false, true),
        PreviewState::Chosen => (true, true),
        PreviewState::Unusable { open } => (open, false),
    };
    let picture: Element<'a, M> = if usable {
        container(preview.unwrap_or_else(|| blank().into())).width(area.width).height(area.height).clip(true).into()
    } else {
        container(toned(Icon::CircleAlert, icon::SIZE, Tone::Text3))
            .center_x(area.width)
            .center_y(area.height)
            .style(|t: &Theme| container::Style {
                border: border::rounded(radius::xs()).color(palette(t).line).width(LINE),
                ..container::Style::default()
            })
            .into()
    };
    let picture = match state {
        PreviewState::Chosen => {
            stack![picture, container(tile_check()).align_right(area.width).padding(space::xs())].into()
        }
        _ => picture,
    };
    let shown = typo::ellipsize(name, typo::caption(), area.width);
    let cut = shown != name;
    // No colour of its own: the name takes the tile's text colour, so it follows hover and state.
    let label = container(typo::text(typo::caption(), shown).wrapping(Wrapping::None)).width(area.width).clip(true);
    let mut parts = column![picture, label].spacing(space::xs());
    if let Some(note) = note {
        let note = typo::ellipsize(note, typo::caption(), area.width);
        let note = typo::text(typo::caption(), note).wrapping(Wrapping::None).style(theme::text_3);
        parts = parts.push(container(note).width(area.width).clip(true));
    }
    let tile = button(parts)
        .width(preview_tile_width())
        .padding(space::xs())
        .style(style::button::preview_tile(on, usable))
        .on_press_maybe(on_press);
    match preview_tip(name, cut, tip) {
        Some(tip) => with_tooltip(tile, tip),
        None => tile.into(),
    }
}

/// The tooltip of a [`preview_tile`]: `tip`, with the whole `name` in front when it was `cut`.
fn preview_tip(name: &str, cut: bool, tip: Option<String>) -> Option<String> {
    match (cut, tip) {
        (true, Some(tip)) => Some(format!("{name} · {tip}")),
        (true, None) => Some(name.to_owned()),
        (false, tip) => tip,
    }
}

/// The check mark of a chosen [`preview_tile`]: the check (12) in `panel` on a round badge in
/// `text` ([`icon::SIZE`] across).
fn tile_check<'a, M: 'a>() -> Element<'a, M> {
    let check: Element<'a, M> =
        icon(Icon::Check, space::m()).style(|t, _| svg::Style { color: Some(palette(t).panel) }).into();
    container(check).center(icon::SIZE).style(style::container::tile_mark).into()
}

/// [`preview_tile`]s in rows that take as many as fit the width and wrap the rest, 6 apart both
/// ways (the gap of [`chip_row`]). Nothing stretches: a wide page shows more tiles in a row, not
/// wider ones.
pub fn preview_grid<'a, M: 'a>(tiles: Vec<Element<'a, M>>) -> Element<'a, M> {
    row(tiles).spacing(ICON_GAP).wrap().vertical_spacing(ICON_GAP).into()
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
/// when `more > 0`. Each is [`size::tile()`] across with a ring in `panel`; the person's colour only
/// tints the surface (`Palette::tinted` over `panel`, never the full colour as a surface) and the
/// initials stand in `CAPTION` and `text` on it, readable at 4.5:1 in light and dark. "+n" is neutral
/// (`selected`, `text_2`). A colour with alpha below 1 fades the avatar to that alpha (ended
/// events). The labels are copied, so they may be built on the spot ([`String`] or `&str`).
///
/// ```ignore
/// widgets::avatar_stack(&[("AW".to_string(), colour)], 2)
/// ```
pub fn avatar_stack<'a, M: 'a, S: AsRef<str>>(people: &[(S, Color)], more: usize) -> Element<'a, M> {
    let edge = avatar_edge();
    let avatar = |label: String, colour: Option<Color>| -> Element<'a, M> {
        container(typo::text(typo::caption(), label).wrapping(Wrapping::None))
            .center(edge)
            .style(move |t| {
                let p = palette(t);
                let (surface, ink) = avatar_colours(&p, colour);
                container::Style {
                    text_color: Some(ink),
                    background: Some(Background::Color(surface)),
                    border: border::rounded(edge / 2.0).color(p.panel).width(AVATAR_RING),
                    ..container::Style::default()
                }
            })
            .into()
    };
    let mut faces: Vec<Element<'a, M>> =
        people.iter().map(|(initials, colour)| avatar(initials.as_ref().to_owned(), Some(*colour))).collect();
    if more > 0 {
        faces.push(avatar(format!("+{more}"), None));
    }
    let width = avatar_stack_width(faces.len());
    let step = avatar_step();
    let layers = faces.into_iter().enumerate().map(|(i, face)| pin(face).x(step * i as f32).into());
    stack(std::iter::once(blank().width(width).height(edge).into()).chain(layers)).into()
}

/// The surface and the ink of an avatar of an [`avatar_stack`]: a person's `colour` tinted over
/// `panel` with `text` on it (both faded by the colour's alpha), "+n" (`None`) `selected` with
/// `text_2`.
fn avatar_colours(p: &Palette, colour: Option<Color>) -> (Color, Color) {
    match colour {
        Some(colour) => {
            let fade = colour.a;
            (theme::over(Color { a: p.tint * fade, ..colour }, p.panel), p.text.scale_alpha(fade))
        }
        None => (p.selected, p.text_2),
    }
}

/// The width of an [`avatar_stack`] with `faces` avatars ("+n" counts as one), for a row that must
/// leave it room; 0 without any.
pub fn avatar_stack_width(faces: usize) -> f32 {
    match faces {
        0 => 0.0,
        n => avatar_edge() + avatar_step() * (n - 1) as f32,
    }
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
    let labels_width = ticks
        .iter()
        .map(|tick| typo::text_width(&crate::units::format_count(u64::from(*tick)), axis))
        .fold(0.0, f32::max);
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
        let label =
            typo::text(axis, crate::units::format_count(u64::from(tick))).style(theme::text_3).wrapping(Wrapping::None);
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
        typo::text(typo::body_strong().tabular(), crate::units::format_count(u64::from(column.total()))),
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

/// The widget behind [`badged`]: Iced has no layer that may reach past its parent's bounds, so the
/// dot is drawn as a quad over the corner after the content, outside the layout. Only for content
/// that takes no input (an icon): events and operations are not passed on.
mod badge {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::renderer::{self, Quad, Renderer as _};
    use iced::advanced::widget::{Tree, tree};
    use iced::advanced::{Widget, mouse};
    use iced::{Background, Color, Element, Length, Rectangle, Renderer, Size, Theme, border};

    /// `content` with a dot `edge` wide in `colour` on its top-right corner.
    pub(super) struct CornerDot<'a, M> {
        pub(super) content: Element<'a, M>,
        pub(super) colour: Color,
        pub(super) edge: f32,
    }

    /// Where the dot stands for content in `bounds`: its centre on the top-right corner.
    pub(super) fn dot_bounds(bounds: Rectangle, edge: f32) -> Rectangle {
        let half = edge / 2.0;
        Rectangle { x: bounds.x + bounds.width - half, y: bounds.y - half, width: edge, height: edge }
    }

    /// The clip of the dot's layer: the dot and one pixel around it (the smoothed edge reaches past
    /// the quad), within the `viewport`; `None` when none of it is in view.
    pub(super) fn clip_bounds(dot: Rectangle, viewport: &Rectangle) -> Option<Rectangle> {
        dot.expand(1.0).intersection(viewport)
    }

    impl<M> Widget<M, Theme, Renderer> for CornerDot<'_, M> {
        fn tag(&self) -> tree::Tag {
            self.content.as_widget().tag()
        }

        fn state(&self) -> tree::State {
            self.content.as_widget().state()
        }

        fn children(&self) -> Vec<Tree> {
            self.content.as_widget().children()
        }

        fn diff(&self, tree: &mut Tree) {
            self.content.as_widget().diff(tree);
        }

        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
            let child = self.content.as_widget_mut().layout(tree, renderer, limits);
            layout::Node::with_children(child.size(), vec![child])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(child) = layout.children().next() {
                self.content.as_widget().draw(tree, renderer, theme, style, child, cursor, viewport);
            }
            let bounds = dot_bounds(layout.bounds(), self.edge);
            // A layer of its own: wgpu draws a layer's quads before its images, so in the icon's layer
            // the icon would cover the dot. A layer does not inherit the clip of the one around it, so
            // it is clipped to the viewport (a scrolled page hides the dot with its entry), one pixel
            // wider than the dot for its smoothed edge.
            let Some(clip) = clip_bounds(bounds, viewport) else { return };
            let quad = Quad { bounds, border: border::rounded(self.edge / 2.0), ..Quad::default() };
            renderer.with_layer(clip, |renderer| renderer.fill_quad(quad, Background::Color(self.colour)));
        }
    }
}

#[cfg(test)]
mod tests {
    /// DS.4: the segments and their gaps fill the strip exactly, the rest comes last, a small part
    /// stays visible.
    #[test]
    fn shares_add_up_to_the_width() {
        let filled = |widths: &[f32]| widths.iter().sum::<f32>() + SHARE_GAP * widths.len().saturating_sub(1) as f32;
        let widths = share_widths(&[600, 300, 1], 99, 500.0);
        assert_eq!(widths.len(), 4);
        assert!((filled(&widths) - 500.0).abs() < 0.01, "{widths:?}");
        assert!(widths[2] >= SHARE_MIN);
        assert!(widths[0] > widths[1] && widths[1] > widths[3]);
        // Parts of 0 take no segment.
        assert_eq!(share_widths(&[5, 0, 5], 0, 300.0).len(), 2);
    }

    /// DS.4: a whole of 0 draws only the track.
    #[test]
    fn an_empty_whole_draws_only_the_track() {
        assert_eq!(share_widths(&[0, 0], 0, 320.0), vec![320.0]);
        assert_eq!(share_widths(&[], 0, 320.0), vec![320.0]);
        let _: Element<'_, ()> = share_strip(vec![SharePart::new("Index", 0, "0 B", ChartColor::Series(0))], None);
    }

    /// DS.4: four series, then "Other".
    #[test]
    fn a_fifth_part_is_drawn_as_other() {
        assert_eq!(share_color(3, ChartColor::Series(3)), ChartColor::Series(3));
        assert_eq!(share_color(4, ChartColor::Series(0)), ChartColor::Other);
        assert_eq!(share_color(0, ChartColor::Other), ChartColor::Other);
        let parts = (0..5).map(|i| SharePart::new(format!("Part {i}"), 10, "10", ChartColor::Series(i))).collect();
        let _: Element<'_, ()> = share_strip(parts, Some(("Free".into(), 50)));
    }

    /// DS.4: shares are rounded, a small one says "<1%", none says "0%".
    #[test]
    fn shares_are_rounded_percentages() {
        let percent = |n: &dyn std::fmt::Display| torro_core::i18n::trf("{n}%", &[("n", n)]);
        assert_eq!(share_percent(236, 1000), percent(&24));
        assert_eq!(share_percent(1, 1000), percent(&"<1"));
        assert_eq!(share_percent(0, 1000), percent(&0));
        assert_eq!(share_percent(5, 0), percent(&0));
        assert_eq!(share_percent(1000, 1000), percent(&100));
    }

    /// DS.4: figures in one row while they fit, then two per row, then one.
    #[test]
    fn figures_take_fewer_columns_when_narrow() {
        let row_of = |n: usize| n as f32 * stat_tile_min() + space::m() * (n - 1) as f32;
        assert_eq!(stat_columns(4, row_of(4)), 4);
        assert_eq!(stat_columns(4, row_of(4) - 1.0), 2);
        assert_eq!(stat_columns(4, row_of(2) - 1.0), 1);
        assert_eq!(stat_columns(3, row_of(3)), 3);
        assert_eq!(stat_columns(0, 100.0), 1);
        let figures = vec![StatFigure::new("Mail", "4,796", "59 unread"); 3];
        let _: Element<'_, ()> = stat_grid(figures);
    }

    /// DS.4: a setting pair keeps two columns from the form width on; the save bar builds busy and
    /// idle; the quiet branch builds.
    #[test]
    fn setting_pairs_save_bar_and_quiet_branch_build() {
        assert!(pair_is_wide(size::form_max()));
        assert!(!pair_is_wide(size::form_max() - 1.0));
        let pair: Element<'_, ()> = setting_pair("Sender name", "Shown as the sender.", || blank().into());
        let _: Element<'_, ()> = setting_pairs(vec![pair, setting_pair("Signature", "", || blank().into())]);
        let _: Element<'_, ()> = save_bar("Signature changed", Some(()), Some(()), false);
        let _: Element<'_, ()> = save_bar("Signature changed", None, None, true);
        let _: Element<'_, ()> = sidebar_branch_quiet(vec![blank().into()]);
    }

    /// DS.3: a longer word and a count make the row of tabs wider.
    #[test]
    fn tabs_grow_with_word_and_count() {
        let one = |label: &str, count| vec![(label.to_string(), count)];
        assert!(tabs_width(&one("Overview", None)) > tabs_width(&one("Mail", None)));
        assert!(tabs_width(&one("Mail", Some(12))) > tabs_width(&one("Mail", None)));
        assert!(tabs_width(&one("Mail", Some(1200))) > tabs_width(&one("Mail", Some(12))));
        let two = vec![("Mail".to_string(), None), ("Mail".to_string(), None)];
        assert!(tabs_width(&two) > 2.0 * tabs_width(&one("Mail", None)));
        assert_eq!(tabs_width(&[]), 0.0);
    }

    /// DS.3: every tab reports its own index, also from the pick list of a narrow row.
    #[test]
    fn tabs_report_their_index() {
        #[derive(Debug, Clone, PartialEq)]
        enum Msg {
            Tab(usize),
        }
        let labels = vec![("Overview".to_string(), None), ("Folders".to_string(), Some(4796))];
        let choices = tab_choices(&labels);
        assert_eq!(choices.iter().map(|c| c.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(choices[1].to_string(), format!("Folders {}", crate::units::format_count(4796)));
        let on_select: fn(usize) -> Msg = Msg::Tab;
        assert_eq!(on_select(choices[1].index), Msg::Tab(1));
        let _: Element<'_, Msg> = tabs(labels, 1, Msg::Tab);
    }

    /// DS.3: the head builds without pill and buttons, and with them; narrow it stacks them.
    #[test]
    fn a_detail_head_builds_with_and_without_its_side() {
        #[derive(Debug, Clone)]
        enum Msg {
            Sync,
            More,
        }
        let _: Element<'_, Msg> =
            detail_head(TileFace::letter("anna"), "anna@example.org", "", None, HeadActions::default());
        let actions = HeadActions {
            action: Some(PageAction::new("Sync now", Some(Msg::Sync)).tip("Syncs all accounts")),
            more: Some((Anchor::new(), Msg::More)),
        };
        let _: Element<'_, Msg> = detail_head(
            TileFace::letter("anna"),
            "anna@example.org",
            "Anna",
            Some((Status::Ok, "Synced".into())),
            actions,
        );
        assert!(head_is_wide(size::form_max()));
        assert!(!head_is_wide(size::form_max() - 1.0));
    }

    /// DS.3: a question card keeps its buttons beside the sentence only while the sentence keeps
    /// its room.
    #[test]
    fn a_question_card_moves_its_buttons_under_the_sentence_when_narrow() {
        let actions = vec![PageAction::new("Use Sent", Some(())), PageAction::new("Use Sent Items", Some(()))];
        let buttons = actions[0].width() + actions[1].width() + space::s();
        assert!(question_is_wide(buttons + size::props(), &actions));
        assert!(!question_is_wide(buttons + size::props() - 1.0, &actions));
        let _: Element<'_, ()> = question_card(Icon::Folder, "Which folder holds sent mail?", "", actions);
        let _: Element<'_, ()> = question_card(Icon::Folder, "Sync problem", "Login failed.", Vec::new());
    }

    /// Issue #299: a search that finds nothing says so; one that finds something shows its entries.
    #[test]
    fn a_search_menu_says_when_nothing_matches() {
        #[derive(Debug, Clone)]
        enum Msg {
            Query(String),
            First,
        }
        assert_eq!(search_results::<Msg>(Vec::new()).err(), Some(tr("No matches")));
        let found: Vec<Element<'_, Msg>> = vec![typo::text(typo::body(), "Inter").into(), blank().into()];
        assert_eq!(search_results(found).map(|items| items.len()).ok(), Some(2));
        let _: Element<'_, Msg> = search_menu("search", "", Msg::Query, Msg::First, Vec::new(), size::props());
        let one = vec![typo::text(typo::body(), "Inter").into()];
        let _: Element<'_, Msg> = search_menu("search", "in", Msg::Query, Msg::First, one, size::props());
        // What the field hands the caller, typed: the whole text.
        let typed = Msg::Query("mo".into());
        assert!(matches!(&typed, Msg::Query(text) if text == "mo"), "{typed:?}");
    }

    /// Task E12.7, issue #338: the five group colours stand out on the panel in both modes, stay apart from
    /// one another, and there is one per group – no more.
    #[test]
    fn the_category_colours_are_visible_and_distinct() {
        let standard = torro_core::design::ColorScheme::standard();
        for (mode, p) in [("light", &standard.colors.light), ("dark", &standard.colors.dark)] {
            assert_eq!(p.category.len(), CATEGORY_COLOURS, "one colour per group, not per category");
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

    /// The groups of a mail's rail as task E14.3 builds them, with an item off in "File".
    fn rail_groups() -> Vec<RailGroup<'static, Msg>> {
        vec![
            RailGroup::new(Icon::Reply, "Reply", vec![RailItem::new(Icon::Reply, "Reply", "R", Some(Msg::Go)).lead()])
                .accent(),
            RailGroup::new(
                Icon::Check,
                "File",
                vec![
                    RailItem::new(Icon::Check, "Done", "E", Some(Msg::Go)),
                    RailItem::new(Icon::Clock, "Later…", "S", Some(Msg::Close)),
                    RailItem::new(Icon::Archive, "Archive", "", None),
                    RailItem::new(Icon::Trash2, "Delete", "#", Some(Msg::Go)).danger(),
                ],
            ),
            RailGroup::new(Icon::Flag, "Mark", vec![RailItem::new(Icon::Flag, "Unflag", "", Some(Msg::Go))])
                .tinted(Color::WHITE),
            RailGroup::new(Icon::BellOff, "Mute", vec![RailItem::new(Icon::BellOff, "Mute…", "M", Some(Msg::Go))])
                .foot(),
            RailGroup::new(
                Icon::Ellipsis,
                "More",
                vec![RailItem::new(Icon::Copy, "Copy text and everything else that is long", "", Some(Msg::Go))],
            )
            .foot(),
        ]
    }

    /// Issue #248: a badged mark keeps its own icon and adds a dot centred on the icon's top-right
    /// corner, one size at every scale and without room of its own; it builds in rows, chips and both
    /// sidebar modes.
    #[test]
    fn a_badged_mark_puts_a_dot_on_the_corner() {
        let dot = badge::dot_bounds(rect(10.0, 20.0, 16.0, 16.0), Scale::Small.dot());
        assert_eq!(dot, rect(23.0, 17.0, 6.0, 6.0), "centre on the corner (26, 20)");
        let window = rect(0.0, 0.0, 800.0, 600.0);
        assert_eq!(badge::clip_bounds(dot, &window), Some(rect(22.0, 16.0, 8.0, 8.0)), "room for the edge");
        let page = rect(0.0, 20.0, 800.0, 580.0);
        assert_eq!(badge::clip_bounds(dot, &page), Some(rect(22.0, 20.0, 8.0, 4.0)), "cut where the page ends");
        assert_eq!(badge::clip_bounds(dot, &rect(0.0, 100.0, 800.0, 500.0)), None, "scrolled out of view");
        let colour = Color::WHITE;
        let mark = Mark::Badged(Icon::Cloud, colour);
        assert_ne!(mark, Mark::Badged(Icon::Github, colour), "two badged entries keep their icons apart");
        let _: Element<'_, Msg> = mark_view(mark, Scale::Row, Tone::Text2);
        let _: Element<'_, Msg> = mark_view(mark, Scale::Small, Tone::Text);
        for mode in [SidebarMode::Expanded, SidebarMode::Rail] {
            let _ = sidebar_item_in(mode, mark, "Nextcloud", Trail::Count(2), false, Msg::Go);
            let _ = sidebar_item_in(mode, mark, "Nextcloud", Trail::Count(2), true, Msg::Go);
        }
        let _ = choice_chip("Nextcloud", Some(mark), true, Some(Msg::Go));
        let _ = choice_chip("Nextcloud", Some(mark), false, Some(Msg::Go));
    }

    /// Issue #291 (task L.34): a list row follows the list look – a bar of the accent colour at the
    /// chosen row (Standard, Compact) or the whole row in the accent with its cells in `on_accent`
    /// (Console), a line below or none – and builds in every look, chosen or not.
    #[test]
    fn a_list_row_follows_the_list_look() {
        let standard = layout::list();
        assert_eq!(standard.selection, ListSelection::Bar, "the standard marks the chosen row with a bar");
        let fill = ListLook { selection: ListSelection::Fill, separators: false, ..standard };
        assert_eq!(RowParts::of(standard, true), RowParts { bar: true, on_accent: false, line: true });
        assert_eq!(RowParts::of(standard, false), RowParts { bar: false, on_accent: false, line: true });
        assert_eq!(RowParts::of(fill, true), RowParts { bar: false, on_accent: true, line: false });
        assert_eq!(RowParts::of(fill, false), RowParts { bar: false, on_accent: false, line: false });
        for look in [standard, fill] {
            for selected in [true, false] {
                let cells = || vec![typo::text(typo::body(), "Review API concept").width(Fill).into()];
                let _: Element<'_, Msg> = list_row_in(look, cells(), selected, Some(Msg::Go));
                let _: Element<'_, Msg> = list_row_in(look, cells(), selected, None);
            }
        }
        let _: Element<'_, Msg> = row_content(typo::text(typo::body(), "x"), true);
        let _: Element<'_, Msg> = selection_bar(size::row());
    }

    /// Task E14.1: the rail's mode goes back and forth, starts collapsed and is as wide as the
    /// collapsed sidebar or the expanded one.
    #[test]
    fn the_rail_mode_goes_back_and_forth() {
        assert_eq!(RailMode::default(), RailMode::Collapsed);
        assert_eq!(RailMode::Collapsed.toggled(), RailMode::Expanded);
        assert_eq!(RailMode::Expanded.toggled(), RailMode::Collapsed);
        assert_eq!(RailMode::Collapsed.toggled().toggled(), RailMode::Collapsed);
        assert_eq!(RailMode::Collapsed.width(), crate::shell::rail() + LINE);
        assert_eq!(RailMode::Expanded.width(), size::rail_wide() + LINE);
        // Issue #291: the rails are the design's, the toggle centred over the collapsed icons.
        assert_eq!((crate::shell::rail(), size::rail_wide()), (60.0, 248.0), "the standard's");
        assert_eq!(rail_toggle_inset(), 10.0, "(60 - 40) / 2");
        assert_eq!(size::control_lg(), 40.0, "the rail's button");
    }

    /// Task E14.1: a group's icon sends its first item that goes – past an item that is off – and
    /// nothing when none goes; an item without a message sends nothing.
    #[test]
    fn the_group_icon_sends_the_first_item_that_goes() {
        let off = RailItem::<Msg>::new(Icon::Archive, "Archive", "", None);
        assert_eq!(off.press(), None, "an item without on_press sends nothing");
        assert_eq!(RailItem::new(Icon::Clock, "Later…", "S", Some(Msg::Close)).press(), Some(Msg::Close));
        let group = RailGroup::new(
            Icon::Archive,
            "File",
            vec![
                off.clone(),
                RailItem::new(Icon::Clock, "Later…", "S", Some(Msg::Close)),
                RailItem::new(Icon::Check, "Done", "E", Some(Msg::Go)),
            ],
        );
        assert_eq!(group.first_press(), Some(Msg::Close), "the first that goes, not the first");
        let dead = RailGroup::new(Icon::Archive, "File", vec![off.clone(), off.clone()]);
        assert_eq!(dead.first_press(), None, "no item goes: the icon opens the card");
        assert_eq!(RailGroup::<Msg>::new(Icon::Archive, "File", Vec::new()).first_press(), None);
        assert_eq!((group.first_index(), dead.first_index()), (Some(1), None), "the card lights what a click does");
    }

    /// Task E14.3 (finding 8): a destructive item is never what a click on the group's icon does –
    /// a group whose first item that goes is Delete opens its card instead; behind a safe first item
    /// it stays in the card.
    #[test]
    fn a_click_on_a_group_never_deletes() {
        let delete = RailItem::new(Icon::Trash2, "Delete", "#", Some(Msg::Close)).danger();
        let off = RailItem::<Msg>::new(Icon::Archive, "Archive", "", None);
        let only = RailGroup::new(Icon::Check, "File", vec![off, delete.clone()]);
        assert_eq!((only.first_press(), only.first_index()), (None, None), "Delete first: the card opens");
        let archive = RailItem::new(Icon::Archive, "Archive", "E", Some(Msg::Go));
        let safe = RailGroup::new(Icon::Check, "File", vec![archive, delete]);
        assert_eq!(safe.first_press(), Some(Msg::Go), "Archive first: the click archives");
        assert!(
            !RailItem::<Msg>::new(Icon::Check, "Done", "E", None).danger
                && RailItem::<Msg>::new(Icon::Check, "Done", "E", None).lead().lead
        );
    }

    /// Task E14.1: every group opens equally wide – the width depends on no group – and its layer
    /// lines up with its icon: the top for the groups at the top, the bottom for the foot.
    #[test]
    fn open_groups_are_equally_wide_and_line_up_with_their_icon() {
        assert_eq!(rail_flyout_width(), size::sidebar());
        let anchor = Anchor::new();
        for group in rail_groups() {
            let _: Element<'_, Msg> = rail_flyout(group.at(&anchor), &anchor, Msg::Close);
        }
        let window = rect(0.0, 0.0, 1600.0, 1000.0);
        let icon = rect(1500.0, 120.0, size::control_lg(), size::control_lg());
        let layer = Size::new(rail_flyout_width() + flyout_gap() + size::control_lg(), 160.0);
        let top = flyout_origin(icon, layer, false, window);
        assert_eq!((top.x + layer.width, top.y), (icon.x + icon.width, icon.y - flyout_lift()), "right edges in line");
        // Task E14.3 (finding 5): the first row stands level with the icon, not the title: the
        // card's top lies its inset and title above the row, the row centred on the icon.
        let title = flyout_title_padding().top + typo::line_height(typo::caption()) + flyout_title_padding().bottom;
        let row_top = top.y + style::MENU_INSET + title;
        assert_eq!(row_top + style::MENU_ITEM_HEIGHT / 2.0, icon.y + icon.height / 2.0);
        let foot = rect(1500.0, 900.0, size::control_lg(), size::control_lg());
        let low = flyout_origin(foot, layer, true, window);
        assert_eq!((low.x + layer.width, low.y + layer.height), (foot.x + foot.width, foot.y + foot.height));
        // Kept inside the window: a low window moves the card in from its bottom and top.
        let short = rect(0.0, 0.0, 1600.0, 200.0);
        let moved = flyout_origin(icon, layer, false, short);
        assert_eq!(moved.y + layer.height, short.height - beside_gap(), "in from the bottom");
        let tall = Size::new(layer.width, 400.0);
        assert_eq!(flyout_origin(icon, tall, true, short).y, beside_gap(), "too tall: from the top");
        // Task E14.3 (finding 6): the card ends `space::xs()` before the rail's line.
        assert_eq!(flyout_gap(), space::s() + LINE + space::xs());
    }

    /// Issue #291 (review of the three designs): a settings row of a fixed height takes only the sides
    /// of the row padding, so its control – a button, a pick list – keeps its height in every design
    /// (with 18 above and below a row of 64 squeezed a button of 36 to 28).
    #[test]
    fn a_group_row_holds_its_control_in_every_design() {
        for id in torro_core::design::BUILT_IN_IDS {
            let d = torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design");
            let pad = d.layout.row_padding;
            let full = Padding { top: pad.vertical, right: pad.horizontal, bottom: pad.vertical, left: pad.horizontal };
            let sides = group_row_padding(full);
            assert_eq!((sides.left, sides.right), (pad.horizontal, pad.horizontal), "{id}: the sides stay");
            let control = d.size.control_sm.max(d.size.control);
            for height in [d.size.header, d.size.header + d.space.m] {
                let inner = height - sides.top - sides.bottom;
                assert!(inner >= control, "{id}: {inner} inside a row of {height} holds a control of {control}");
            }
        }
    }

    /// Issue #291 (review of the three designs): where the rail cannot expand the head stays with the
    /// toggle off – drawn the same way as a working one –, so the icons below it do not move; only
    /// medium and narrow windows draw none.
    #[test]
    fn the_rails_head_stays_with_its_toggle_off() {
        assert!(RailToggle::On(Msg::Go).has_head() && RailToggle::<Msg>::Off("why").has_head());
        assert!(!RailToggle::<Msg>::Hidden.has_head());
        assert_eq!(RailToggle::from(Some(Msg::Go)), RailToggle::On(Msg::Go), "action_rail's on_toggle");
        assert_eq!(RailToggle::<Msg>::from(None), RailToggle::Hidden);
        for id in ["torro", "compact", "console"] {
            let d = torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design");
            assert!(d.size.control_lg <= d.size.rail, "{id}: the toggle fits the collapsed rail, on or off");
        }
        for mode in [RailMode::Collapsed, RailMode::Expanded] {
            for toggle in [RailToggle::On(Msg::Go), RailToggle::Off("why"), RailToggle::Hidden] {
                let _: Element<'_, Msg> = action_rail_toggle(rail_groups(), mode, None, |_| Msg::Close, toggle);
            }
        }
    }

    /// Task E14.1: the rail builds collapsed and expanded, with and without its button and an open
    /// group, also without groups.
    #[test]
    fn the_action_rail_builds_in_both_modes() {
        let anchor = Anchor::new();
        for mode in [RailMode::Collapsed, RailMode::Expanded] {
            for toggle in [Some(Msg::Go), None] {
                for open in [None, Some(1), Some(9)] {
                    let groups = rail_groups().into_iter().map(|g| g.at(&anchor)).collect();
                    let _: Element<'_, Msg> = action_rail(groups, mode, open, |_| Msg::Close, toggle.clone());
                }
            }
            let _: Element<'_, Msg> = action_rail(Vec::new(), mode, None, |_| Msg::Close, None);
        }
        // Expanded, a group of a single item has no title, the others theirs (finding 10).
        let groups = rail_groups();
        assert_eq!(rail_section(groups[3].clone(), false).len(), 2, "Mute: the room above and its row");
        assert_eq!(rail_section(groups[1].clone(), false).len(), 5, "File: its title and four rows");
        assert_eq!(rail_section(groups[1].clone(), true).len(), 4, "the first group without a title");
        let item = RailItem::new(Icon::ListTodo, "Task", "T", Some(Msg::Go)).at(&anchor).accent();
        assert!(item.anchor.is_some() && item.accent);
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

    /// Issue #300 (task E17.1): a nested entry is the builder's to set; a new entry is not nested.
    #[test]
    fn menu_items_are_nested_only_when_asked() {
        assert!(!MenuItem::new(Icon::Inbox, "Waiting for me", Msg::Go).nested);
        let child = MenuItem::new(Icon::Inbox, "Reply needed", Msg::Go).trail(Trail::Count(2)).nested();
        assert!(child.nested);
        assert_eq!(child.trail, Trail::Count(2), "nesting keeps the rest of the entry");
        let _: Element<'_, Msg> = menu(vec![MenuSection::new(vec![child])]);
    }

    /// Issue #300 (task E17.1): the children of a group sit one icon and its gap further in, and the
    /// line that binds them to the group runs under the middle of the group's icon, left of them.
    #[test]
    fn a_branch_is_indented_by_an_icon_with_its_line_under_the_group_icon() {
        assert_eq!(branch_indent(), icon::SIZE + space::s());
        assert_eq!(branch_indent(), 24.0, "the standard design's 16 + 8");
        assert_eq!(branch_line_x(), style::ITEM_PADDING.left + icon::SIZE / 2.0);
        assert_eq!(branch_line_x(), 16.0, "the middle of the group's icon");
        assert!(branch_line_x() < branch_indent(), "the line stays left of the indented rows");
    }

    /// Issue #300 (task E17.1): the group row, its branch and its children build in both sidebar modes.
    #[test]
    fn the_folding_group_and_its_branch_build_in_both_modes() {
        let colour = Color::from_rgb(0.2, 0.5, 0.9);
        for mode in [SidebarMode::Expanded, SidebarMode::Rail] {
            for (active, open) in [(false, false), (true, false), (false, true), (true, true)] {
                let _ = sidebar_fold_in(
                    mode,
                    Mark::Tinted(Icon::Inbox, colour),
                    "Waiting for me",
                    Trail::Count(4),
                    active,
                    open,
                    Msg::Go,
                );
            }
            let children = vec![
                sidebar_child(Mark::Tinted(Icon::Inbox, colour), "Reply needed", Trail::Count(2), true, Msg::Go),
                sidebar_child(Mark::Tinted(Icon::Inbox, colour), "Waiting", Trail::None, false, Msg::Close),
            ];
            let _ = sidebar_branch(colour, children);
            let _ = sidebar_branch::<Msg>(colour, Vec::new());
        }
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
            sidebar_gap_in(SidebarMode::Expanded),
            sidebar_gap_in(SidebarMode::Rail),
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

    /// Review of #278: the initials of an avatar read at 4.5:1 on its tinted surface in light and
    /// dark, for every calendar colour, and the stack leaves room for every face.
    #[test]
    fn avatar_initials_are_readable_on_every_calendar_colour() {
        for p in [Palette::dark(), Palette::light()] {
            for colour in p.calendar.into_iter().map(Some).chain([None]) {
                let (surface, ink) = avatar_colours(&p, colour);
                let ratio = ink.relative_contrast(surface);
                assert!(ratio >= 4.5, "{colour:?}: {ratio:.2}:1");
            }
        }
        assert_eq!(avatar_stack_width(0), 0.0);
        assert_eq!(avatar_stack_width(1), size::tile());
        assert!(avatar_stack_width(4) > 3.0 * size::tile(), "they overlap only by their rings");
        let owned = [("AW".to_string(), Palette::dark().calendar[0])];
        let _: Element<'_, Msg> = avatar_stack(&owned, 2);
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

    /// Task E11.1: the chips of attachments build on and off.
    #[test]
    fn the_attachment_chips_build_on_and_off() {
        let _: Element<'_, Msg> = file_chip("budget-q4.csv", "3 KB", Some(Msg::Go));
        let _: Element<'_, Msg> = file_chip("travel-policy.pdf", "", None);
    }

    /// Task M12.5: the split chip, the save-all action and the block build on, off and with or
    /// without an anchor for their menu.
    #[test]
    fn the_split_chips_and_the_block_build_on_and_off() {
        let anchor = Anchor::new();
        let _: Element<'_, Msg> = split_file_chip(
            "budget-q4.csv",
            "3 KB",
            "Open: budget-q4.csv",
            Some(Msg::Go),
            Some(Msg::Close),
            Some(&anchor),
        );
        let _: Element<'_, Msg> = split_file_chip("travel-policy.pdf", "", "", None, None, None);
        let tip = "More ways to save all";
        let _: Element<'_, Msg> = split_action(Icon::Download, "Save all", tip, Some(Msg::Go), Some(Msg::Close), None);
        let _: Element<'_, Msg> = split_action(Icon::Save, "Save all", tip, None, None, Some(&anchor));
        let _: Element<'_, Msg> =
            split_action_framed(Icon::Download, "Save all", tip, Some(Msg::Go), Some(Msg::Close), Some(&anchor));
        let _: Element<'_, Msg> = split_action_framed(Icon::Download, "Save all", "", None, None, None);
        let head = file_block_head(
            "2 attachments",
            "3 KB",
            Some(split_action(Icon::Download, "Save all", tip, None, None, None)),
        );
        let chips: Element<'_, Msg> = row![file_chip("a.pdf", "", None)].into();
        let _: Element<'_, Msg> = file_block(head, chips);
    }

    /// Task M12.5: a menu entry takes a detail line and at most two tags; a third is left out.
    #[test]
    fn menu_entries_take_a_detail_and_two_tags() {
        let item = MenuItem::new(Icon::Folder, "Invoices", Msg::Go)
            .detail("~/Documents/Invoices/2026/10")
            .tag("Click", false)
            .tag("Suggested", true)
            .tag("Third", false);
        assert_eq!(item.tags.len(), 2);
        assert!(item.tags[1].sparkles && !item.tags[0].sparkles);
        assert!(item.detail.is_some());
        assert!(MenuItem::new(Icon::Folder, "Plain", Msg::Go).detail.is_none());
        let _: Element<'_, Msg> = menu_item(item);
        let _: Element<'_, Msg> = menu_item(MenuItem::new(Icon::Folder, "Tagged", Msg::Go).tag("Click", false));
    }

    /// Design review of #290: an entry without a detail keeps the height every menu had; one with a
    /// detail grows by one line of `meta`, not by a whole entry, so its two lines stay together.
    #[test]
    fn a_detail_adds_one_meta_line_to_a_menu_entry() {
        assert_eq!(menu_entry_height(false), style::MENU_ITEM_HEIGHT);
        assert_eq!(menu_entry_height(true), style::MENU_ITEM_HEIGHT + typo::line_height(typo::meta()));
        // Title and detail fit in it with air above and below.
        let lines = typo::line_height(typo::body()) + typo::line_height(typo::meta());
        assert!(menu_entry_height(true) > lines);
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

    #[test]
    fn mail_card_marker_has_room_with_zero_panel_padding() {
        let minimum = space::s() + 2.0 * space::xs();
        assert_eq!(card_mark_inset_for(0.0), minimum);
        assert_eq!(card_mark_inset_for(minimum / 2.0), minimum);
        assert_eq!(card_mark_inset_for(minimum + space::s()), minimum + space::s());
    }

    /// Task L.18: the colour mode's segment is as wide as its icons and words, or squares without
    /// the words – and with them it still leaves room for a short label in a narrow window.
    #[test]
    fn an_icon_segment_is_wider_with_its_words() {
        let labels = [tr("System"), tr("Light"), tr("Dark")];
        let words = icon_segmented_width(&labels, true);
        let icons = icon_segmented_width(&labels, false);
        assert!(words > segmented_width(&labels), "the icons take room too");
        assert_eq!(icons, 3.0 * segment_height() + 2.0 * (style::SEGMENT_INSET + size::HAIRLINE));
        assert!(icons < words);
        assert!(words < size::content_max() / 2.0, "leaves room for a label");
        for words in [true, false] {
            let modes =
                [(Icon::Monitor, "System", Msg::Go), (Icon::Sun, "Light", Msg::Go), (Icon::Moon, "Dark", Msg::Go)];
            let _: Element<'_, Msg> = icon_segmented(modes, 1, words);
        }
    }

    /// Task L.18: tiles are made from the tokens – six fit a page of `content_max` in the standard of
    /// issue #291 (seven before, with buttons of 28), three a narrow window –, every state builds, and
    /// a name cut to the tile is never cut silently.
    #[test]
    fn preview_tiles_take_their_size_from_the_tokens_and_build_in_every_state() {
        assert_eq!(preview_tile_width(), 3.0 * size::control());
        assert_eq!(preview_size(), Size::new(preview_tile_width() - 2.0 * (space::xs() + LINE), size::control_sm()));
        assert_eq!((preview_tile_width(), preview_size().height), (114.0, 36.0), "the standard's tile");
        let fit = |room: f32| ((room + ICON_GAP) / (preview_tile_width() + ICON_GAP)).floor();
        assert_eq!(fit(size::content_max()), 6.0);
        assert!(fit(size::form_max() - size::sidebar()) >= 3.0);
        let states = [PreviewState::Plain, PreviewState::Chosen, PreviewState::Unusable { open: false }];
        let tiles: Vec<Element<'_, Msg>> = states
            .into_iter()
            .chain([PreviewState::Unusable { open: true }])
            .map(|state| {
                preview_tile(None, "Commitr Dark Extended Edition", Some("1.2 · Anna".into()), state, Some(Msg::Go))
            })
            .chain([preview_tile(Some(blank().into()), "Torro", None, PreviewState::Plain, None)])
            // Task L.23: a note under the name, in every state, also one that must be cut.
            .chain(
                states.into_iter().map(|state| {
                    preview_tile_noted(None, "IBM Plex Sans", Some("Suggested"), None, state, Some(Msg::Go))
                }),
            )
            .chain([preview_tile_noted(None, "Plex", Some("A note far too long for one tile"), None, states[0], None)])
            .collect();
        let _: Element<'_, Msg> = preview_grid(tiles);
        assert_eq!(preview_tip("Torro", false, None), None);
        assert_eq!(preview_tip("Torro", false, Some("1.0 · Torro".into())).as_deref(), Some("1.0 · Torro"));
        assert_eq!(preview_tip("Long name", true, Some("1.0".into())).as_deref(), Some("Long name · 1.0"));
        assert_eq!(preview_tip("Long name", true, None).as_deref(), Some("Long name"));
        let quiet = vec![icon_button(Icon::RefreshCw, "Reload", Some(Msg::Go))];
        let _: Element<'_, Msg> = group_heading("Design", quiet);
        let _: Element<'_, Msg> = group_heading("Design", Vec::new());
    }
}
