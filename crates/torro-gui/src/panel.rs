//! The side panel for details (an event, a task, later more): one look everywhere, placed
//! **beside** the content when the window has room and **over** it otherwise (task U.3 in
//! `docs/tasks/design.md`). There is no centred dialog for details any more.
//!
//! ```text
//! Beside (wide)                              Over (medium)               Over (narrow)
//! ┌ content ────────────┬ panel 400 ───┐     ┌ content ┬ panel 400 ┐     ┌ panel ───────┐
//! │                     │ path …  ✕   │     │░░scrim░░│ path … ✕ │     │ path …    ✕ │
//! │                     │ Title        │     │░░░░░░░░░│ Title     │     │ Title        │
//! │                     │ body (scroll)│     │░░░░░░░░░│ body      │     │ body         │
//! │                     ├──────────────┤     │░░░░░░░░░├───────────┤     ├──────────────┤
//! │                     │ footer       │     │░░░░░░░░░│ footer    │     │ footer       │
//! └─────────────────────┴──────────────┘     └─────────┴───────────┘     └──────────────┘
//! ```
//!
//! - **Beside** ([`PanelMode::Beside`], the wide layout): a column [`size::PANEL`] wide at the
//!   right of the body, with a line on its left, under the window's header bar. It takes the place
//!   of the properties column: pass `props: None` to the shell while it is open.
//! - **Over** ([`PanelMode::Over`], medium and narrow): the same panel at the right edge over the
//!   whole window, full height, with the menu shadow to the left; the rest of the window is dimmed
//!   (scrim) and a click on it sends the panel's `on_close`. Narrow windows show it full width.
//!
//! The panel itself ([`panel`]): `panel` surface; a 44-high head ([`size::HEADER`], in line with
//! the window's header bar) with the path ([`path`]), optional actions and a close button; the
//! title ([`title`] or the app's own borderless title field); the body, which scrolls together
//! with the title; and an optional fixed footer for actions below a line.
//!
//! # Use
//!
//! The app keeps whether the panel is open (e.g. `Option<Detail>`); the panel is built in `view`
//! and placed with [`place`], which also stacks the open menu above it:
//!
//! ```ignore
//! use torro_gui::panel::{self, PanelMode};
//! use torro_gui::widgets::{self, Mark};
//!
//! fn view(&self) -> Element<'_, Message> {
//!     iced::widget::responsive(|size| {
//!         let class = layout::of(size);
//!         let anchor = Anchor::new();
//!         let detail = self.detail.as_ref().map(|d| {
//!             let body = column![
//!                 widgets::prop_section(tr("Properties"), vec![/* widgets::prop_row(…) */]),
//!                 panel::inset(notes_field),          // free text lines up with the title
//!             ]
//!             .spacing(space::L);
//!             panel::panel(
//!                 panel::path(Some(Mark::Ring(colour)), [list_name, filter_name]),
//!                 title_field,                        // or panel::title(&event.summary)
//!                 body,
//!                 Message::CloseDetail,
//!             )
//!             .actions(vec![more_button])             // optional, left of ✕
//!             .footer(vec![widgets::text_button(None, tr("Delete task"), style::button::danger, Some(Message::Delete))])
//!         });
//!         // The panel replaces the properties column while it is beside the content.
//!         let beside = detail.is_some() && panel::mode_for(class) == PanelMode::Beside;
//!         let props = (shell::has_props(class) && !beside).then(|| self.properties());
//!         let placed = panel::place(class, self.list(class), detail, self.open_menu(&anchor));
//!         shell(class, sidebar, header, placed.body, props)
//!             .drawer(self.drawer, Message::CloseMenu)
//!             .overlay(placed.overlay)
//!             .into()
//!     })
//!     .into()
//! }
//!
//! // subscription: Escape closes the open menu first, then the panel (in both modes).
//! if self.menu.is_some() || self.detail.is_some() { widgets::escape().map(|()| Message::Escape) } else { … }
//! ```
//!
//! The app's state does not depend on the mode: when the window is resized, the same panel moves
//! from beside to over and back, with its fields, scroll position and open menu kept.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{column, container, mouse_area, opaque, row, rule, scrollable, stack, svg};
use iced::{Alignment, Background, Color, Element, Fill, Length, Padding, Shadow, Vector, border};

use torro_core::i18n::tr;

use crate::icon::{self, Icon, icon};
use crate::layout::SizeClass;
use crate::style;
use crate::theme::{self, palette, radius, size, space};
use crate::typo;
use crate::widgets::{self, Mark, Tone};

/// The one-pixel lines of the design.
const LINE: f32 = size::HAIRLINE;
/// Gap between the pieces of the path (skill: 6 is allowed inside building blocks).
const PATH_GAP: f32 = 6.0;
/// Edge of a ring or check box in the path, as in a chip (`torro.css`: `.crumb .ring` 12).
const PATH_RING: f32 = icon::SIZE_SM - 2.0;
/// Line width of a ring in the path.
const RING_LINE: f32 = 2.0;
/// Edge of a dot in the path (`torro.css`: dots in pills and crumbs 6).
const PATH_DOT: f32 = PATH_GAP;

/// Where the panel goes: beside the content or over it. Get it with [`mode_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    /// A column at the right of the body, the content gets narrower (wide layout).
    Beside,
    /// Over the window at the right edge, the content behind it dimmed (medium and narrow).
    Over,
}

/// The mode for the size class `class` ([`crate::layout::of`] of the window size): the wide
/// layout has room for the panel beside the content (at 1024 the content keeps about 380, the
/// width of the medium list), every other layout shows it over the content.
///
/// Mini and strip windows are displays without dialogs (skill, section 6) and should not open a
/// panel; if they do, it is [`PanelMode::Over`] at full width.
pub fn mode_for(class: SizeClass) -> PanelMode {
    match class {
        SizeClass::Wide => PanelMode::Beside,
        SizeClass::Medium | SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => PanelMode::Over,
    }
}

/// The width of the panel in `class`: [`size::PANEL`] beside the content and over it in the
/// medium layout, the full window in narrow (and mini, strip) windows, where 400 would leave no
/// useful strip of content next to it.
fn width_for(class: SizeClass) -> Length {
    match class {
        SizeClass::Wide | SizeClass::Medium => Length::Fixed(size::PANEL),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => Fill,
    }
}

/// A side panel being built by [`panel`]; add [`Panel::actions`] and [`Panel::footer`] if needed,
/// then hand it to [`place`].
pub struct Panel<'a, M> {
    path: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
    title: Element<'a, M>,
    body: Element<'a, M>,
    footer: Vec<Element<'a, M>>,
    on_close: M,
}

/// A side panel with the details of one thing:
///
/// - `path`: where the thing lives, left in the head row ([`path`]: "● Work › Today").
/// - `title`: the thing's name under the head, `HEADLINE`, wrapping: [`title`] for plain text, or
///   the app's own borderless `text_editor` in `typo::HEADLINE` when it can be edited. Give it no
///   padding: the panel lines it up (16 from the edge).
/// - `body`: everything else, scrolling together with the title, 16 below it. The body has 8 at
///   the sides so that [`widgets::prop_section`]s and [`widgets::prop_row`]s line their text up
///   with the title (their hover surface reaches out, as in the properties column); wrap free
///   content (notes, subtasks, a sentence) in [`inset`] to line it up as well. Stack groups with
///   `space::L` between them.
/// - `on_close`: sent by the ✕ button in the head ("Close") and, over the content, by a click on
///   the dimmed window beside the panel. Escape is the app's job ([`widgets::escape`]).
pub fn panel<'a, M: Clone + 'a>(
    path: impl Into<Element<'a, M>>,
    title: impl Into<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
    on_close: M,
) -> Panel<'a, M> {
    Panel {
        path: path.into(),
        actions: Vec::new(),
        title: title.into(),
        body: body.into(),
        footer: Vec::new(),
        on_close,
    }
}

impl<'a, M: Clone + 'a> Panel<'a, M> {
    /// Buttons in the head row, left of ✕, 8 apart: icon buttons ([`widgets::icon_button`]), e.g.
    /// a "…" menu ([`widgets::anchored`] for its layer). Few: the head is for the path.
    pub fn actions(mut self, actions: Vec<Element<'a, M>>) -> Self {
        self.actions = actions;
        self
    }

    /// A footer fixed at the bottom, below a line, that does not scroll: the panel's actions in a
    /// row, 8 apart, 12 above and below, 16 at the sides ("Delete event" as `danger`, "Join" …).
    /// Push `iced::widget::space::horizontal()` between them to move the rest to the right. Empty
    /// (the default): no footer and no line.
    pub fn footer(mut self, footer: Vec<Element<'a, M>>) -> Self {
        self.footer = footer;
        self
    }

    /// The panel as a column for `class`: the head, the scrolling title and body, the footer, on
    /// the `panel` surface; with the left line unless it fills the window (narrow).
    fn view(self, class: SizeClass) -> Element<'a, M> {
        let Panel { path, actions, title, body, footer, on_close } = self;
        let close = widgets::icon_button(Icon::X, tr("Close"), Some(on_close));
        // The head pads like the header bar (16 left, 12 right), so ✕ lines up with its buttons.
        let head = container(
            row![container(path).width(Fill).clip(true)]
                .extend(actions)
                .push(close)
                .spacing(space::S)
                .align_y(Alignment::Center),
        )
        .width(Fill)
        .center_y(size::HEADER)
        .padding(Padding::new(0.0).left(space::L).right(space::M));
        let content =
            column![container(title).width(Fill).padding(Padding::new(0.0).left(space::S).right(space::S)), body]
                .spacing(space::L)
                .padding(Padding { top: space::XS, right: space::S, bottom: space::XL, left: space::S });
        let scroll = scrollable(content)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
        let mut inner = column![head, scroll];
        if !footer.is_empty() {
            let actions = row(footer).spacing(space::S).align_y(Alignment::Center);
            inner = inner
                .push(rule::horizontal(LINE).style(style::rule))
                .push(container(actions).width(Fill).padding([space::M, space::L]));
        }
        let inner = container(inner).width(Fill).height(Fill);
        // The left edge is a rule on the panel's own surface, as in the drawer: Iced leaves a
        // translucent border see-through, and the content below would shine through it over.
        if width_for(class) == Fill {
            inner.into()
        } else {
            row![rule::vertical(LINE).style(style::rule), inner].into()
        }
    }
}

/// The path of the thing in the panel's head: an optional `mark` (the list's ring, the calendar's
/// dot) and `parts` ("Work", "Today") in `META` `text_3`, with a small chevron between the parts,
/// on one line (clipped when too long).
pub fn path<'a, M: 'a, S: IntoFragment<'a>>(mark: Option<Mark>, parts: impl IntoIterator<Item = S>) -> Element<'a, M> {
    let mut line = row![].spacing(PATH_GAP).align_y(Alignment::Center);
    if let Some(mark) = mark {
        line = line.push(path_mark(mark));
    }
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            line = line.push(widgets::toned(Icon::ChevronRight, icon::SIZE_SM, Tone::Text3));
        }
        line = line.push(typo::text(typo::META, part).style(theme::text_3).wrapping(Wrapping::None));
    }
    line.into()
}

/// A mark at the size of a chip's (icons 14, rings 12, dots 6), icons in `text_3`.
fn path_mark<'a, M: 'a>(mark: Mark) -> Element<'a, M> {
    let tinted = |which: Icon, colour: Color| -> Element<'a, M> {
        icon(which, icon::SIZE_SM).style(move |_, _| svg::Style { color: Some(colour) }).into()
    };
    let framed = |colour: Color, corner: f32, line: f32, inner: Element<'a, M>| -> Element<'a, M> {
        container(inner)
            .center(PATH_RING)
            .style(move |_| container::Style {
                border: border::rounded(corner).color(colour).width(line),
                ..container::Style::default()
            })
            .into()
    };
    match mark {
        Mark::Icon(which) => widgets::toned(which, icon::SIZE_SM, Tone::Text3),
        Mark::Tinted(which, colour) => tinted(which, colour),
        Mark::Ring(colour) => framed(colour, PATH_RING / 2.0, RING_LINE, blank().into()),
        Mark::Dot(colour) => widgets::color_dot(colour, PATH_DOT),
        Mark::Check(colour, checked) => {
            let inner = if checked { tinted(Icon::Check, colour) } else { blank().into() };
            framed(colour, radius::XS, style::CHECK_BORDER, inner)
        }
    }
}

/// A plain title for the panel: `HEADLINE` in `text`, wrapping. Editable titles are the app's own
/// borderless `text_editor` in `typo::HEADLINE` instead.
pub fn title<'a, M: 'a>(text: impl IntoFragment<'a>) -> Element<'a, M> {
    typo::text(typo::HEADLINE, text).style(theme::text).wrapping(Wrapping::WordOrGlyph).width(Fill).into()
}

/// Free content in the panel's body (notes, subtasks, a sentence) moved in by 8, so its text
/// starts in line with the title and the text of the property rows (16 from the panel's edge).
pub fn inset<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).width(Fill).padding(Padding::new(0.0).left(space::S).right(space::S)).into()
}

/// The pieces for the shell, from [`place`].
pub struct Placed<'a, M> {
    /// The window's body: the content, with the panel beside it in [`PanelMode::Beside`]. Pass it
    /// as `body` to [`crate::shell::shell`].
    pub body: Element<'a, M>,
    /// What lies over the window: the panel in [`PanelMode::Over`] and the open menu or floating
    /// card above it. Pass it to [`crate::shell::Shell::overlay`].
    pub overlay: Option<Element<'a, M>>,
}

/// Puts `panel` (`None` while closed) next to or over `content` as [`mode_for`] says for `class`,
/// and the open menu or floating card `layer` ([`widgets::floating`], `None` when nothing is open)
/// above everything, so a menu opened inside the panel shows on top of it.
///
/// - Beside: `body` is `content` (taking the free width) and the panel ([`size::PANEL`] with its
///   line) in a row; `overlay` is just `layer`.
/// - Over: `body` is `content` alone; `overlay` is the scrim over the whole window (a click on it
///   sends the panel's `on_close` and reaches nothing below), the panel at the right edge, full
///   height, with the menu shadow to the left ([`size::PANEL`] in medium, full width in narrow),
///   and `layer` on top.
///
/// Without a panel `body` is `content` and `overlay` is `layer`, so apps can always call it.
pub fn place<'a, M: Clone + 'a>(
    class: SizeClass,
    content: impl Into<Element<'a, M>>,
    panel: Option<Panel<'a, M>>,
    layer: Option<Element<'a, M>>,
) -> Placed<'a, M> {
    let content = content.into();
    let Some(panel) = panel else {
        return Placed { body: content, overlay: layer };
    };
    match mode_for(class) {
        PanelMode::Beside => {
            let side =
                container(panel.view(class)).width(size::PANEL).height(Fill).style(style::container::panel_flush);
            let body = row![container(content).width(Fill).height(Fill), side].into();
            Placed { body, overlay: layer }
        }
        PanelMode::Over => {
            let mut layers = vec![over(panel, class)];
            layers.extend(layer);
            Placed { body: content, overlay: Some(stack(layers).into()) }
        }
    }
}

/// The panel over the window: scrim with the close catcher, the panel at the right edge.
fn over<'a, M: Clone + 'a>(panel: Panel<'a, M>, class: SizeClass) -> Element<'a, M> {
    let on_close = panel.on_close.clone();
    let scrim = container(blank().width(Fill).height(Fill)).style(|t| container::Style {
        background: Some(Background::Color(palette(t).scrim)),
        ..container::Style::default()
    });
    let catcher = opaque(mouse_area(scrim).on_press(on_close));
    let sheet = container(panel.view(class)).width(width_for(class)).height(Fill).style(|t| {
        let p = palette(t);
        // The menu shadow falls sideways, to the left: the panel's edge is vertical.
        let shadow = Shadow { offset: Vector::new(-space::M, 0.0), ..p.shadow };
        container::Style { background: Some(Background::Color(p.panel)), shadow, ..container::Style::default() }
    });
    // The aligning container has no interaction of its own, so a click beside the sheet reaches
    // the catcher below.
    stack![catcher, container(opaque(sheet)).width(Fill).height(Fill).align_right(Fill)].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::column;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Close,
        Go,
    }

    const CLASSES: [SizeClass; 5] =
        [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide];

    fn sample<'a>() -> Panel<'a, Msg> {
        panel(
            path(Some(Mark::Ring(Color::WHITE)), ["Work", "Today"]),
            title("Review API concept"),
            column![
                widgets::prop_section("Properties", vec![widgets::prop_row(None, title("x"), Some(Msg::Go))]),
                inset(typo::text(typo::BODY, "Notes")),
            ],
            Msg::Close,
        )
        .actions(vec![widgets::icon_button(Icon::Ellipsis, "More", Some(Msg::Go))])
        .footer(vec![widgets::text_button(None, "Delete task", style::button::danger, Some(Msg::Go))])
    }

    #[test]
    fn only_the_wide_layout_has_room_beside() {
        assert_eq!(mode_for(SizeClass::Wide), PanelMode::Beside);
        for class in [SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            assert_eq!(mode_for(class), PanelMode::Over, "{class:?}");
        }
    }

    #[test]
    fn the_panel_is_400_wide_except_in_narrow_windows() {
        assert_eq!(size::PANEL, 400.0);
        assert_eq!(width_for(SizeClass::Wide), Length::Fixed(size::PANEL));
        assert_eq!(width_for(SizeClass::Medium), Length::Fixed(size::PANEL));
        for class in [SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            assert_eq!(width_for(class), Fill, "{class:?}");
        }
    }

    #[test]
    fn without_a_panel_the_content_and_layer_pass_through() {
        for class in CLASSES {
            let placed: Placed<'_, Msg> = place(class, blank(), None, None);
            assert!(placed.overlay.is_none(), "{class:?}");
            let placed: Placed<'_, Msg> = place(class, blank(), None, Some(blank().into()));
            assert!(placed.overlay.is_some(), "{class:?}");
        }
    }

    #[test]
    fn beside_needs_no_overlay_and_over_always_has_one() {
        let placed = place(SizeClass::Wide, blank(), Some(sample()), None);
        assert!(placed.overlay.is_none(), "beside the content nothing lies over the window");
        let placed = place(SizeClass::Wide, blank(), Some(sample()), Some(blank().into()));
        assert!(placed.overlay.is_some(), "an open menu still floats above");
        for class in [SizeClass::Medium, SizeClass::Narrow] {
            let placed = place(class, blank(), Some(sample()), None);
            assert!(placed.overlay.is_some(), "{class:?}: the panel lies over the window");
        }
    }

    #[test]
    fn panels_build_in_every_class_inside_a_shell() {
        use crate::shell;
        use crate::widgets::Anchor;
        for class in CLASSES {
            let anchor = Anchor::new();
            let layer =
                Some(widgets::floating(&anchor, widgets::Placement::Below(widgets::Align::Start), blank(), Msg::Go));
            let placed = place(class, blank(), Some(sample()), layer);
            let header = shell::header_bar([], shell::header_title(None, "Today", None), []);
            let _: Element<'_, Msg> =
                shell::shell(class, blank(), header, placed.body, None).overlay(placed.overlay).into();
        }
    }

    #[test]
    fn every_mark_fits_in_a_path() {
        let marks = [
            Mark::Icon(Icon::Calendar),
            Mark::Tinted(Icon::CircleAlert, Color::WHITE),
            Mark::Ring(Color::WHITE),
            Mark::Dot(Color::WHITE),
            Mark::Check(Color::WHITE, true),
            Mark::Check(Color::WHITE, false),
        ];
        for mark in marks {
            let _: Element<'_, Msg> = path(Some(mark), ["Work"]);
        }
        let _: Element<'_, Msg> = path(None, Vec::<String>::new());
    }
}
