//! The window frame every app of the suite shares, after Linear (design skill, section 0):
//! sidebar on the window background, the content as an inset panel with a 44-high header bar,
//! and the properties column on the right – in the wide layout. Medium and narrow windows drop
//! the sidebar (its rows move into a menu at the top left, or into a drawer) and the properties
//! (they become chips), and the content runs flush to the window edges.
//!
//! ```text
//! Wide    ┌ sidebar 232 ┐┌ panel (inset 8, radius 8) ───────────────────────────┐
//!         │ [T] App ▾ 🔍 ✎││ header 44                                         │
//!         │ rows …      │├──────────────────────────────────┬─────────────────┤
//!         │             ││ body                             │ properties 300  │
//!         └─────────────┘└──────────────────────────────────┴─────────────────┘
//! Medium, ┌ header 44 (flush) ─────────────────────────────────────────────────┐
//! Narrow  │ body                                                               │
//!         └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! A whole window, as the tasks app builds it:
//!
//! ```ignore
//! use torro_gui::shell::{self, Switcher, header_bar, header_title, shell};
//! use torro_gui::widgets::{self, Anchor, Trail};
//! use torro_gui::layout::{self, SizeClass};
//!
//! fn view(&self) -> Element<'_, Message> {
//!     iced::widget::responsive(|size| {
//!         let class = layout::of(size);
//!         let (switcher, nav) = (Anchor::new(), Anchor::new());
//!         let sidebar = shell::sidebar(
//!             shell::app_switcher(AppId::TASKS, &switcher, Message::Switcher),
//!             vec![widgets::sidebar_item(Icon::Sun, tr("Today"), Trail::Count(5), true, Message::Today)],
//!             vec![widgets::sidebar_item(Icon::Settings, tr("Settings"), Trail::None, false, Message::Settings)],
//!         );
//!         let header = if shell::has_sidebar(class) {
//!             header_bar([], header_title(Some(Icon::Sun), tr("Today"), Some(tr("5 open").into())), [new_task])
//!         } else {
//!             let menu = widgets::menu_button(&nav, Some(Icon::Sun), tr("Today"), Message::ToggleMenu(Menu::Nav));
//!             header_bar([shell::sidebar_toggle(Message::ToggleDrawer)], menu, [new_task])
//!         };
//!         let props = shell::has_props(class).then(|| self.properties());
//!         shell(class, sidebar, header, self.body(class), props)
//!             .drawer(self.drawer, Message::CloseMenu)
//!             .overlay(self.open_menu(&switcher, &nav))
//!             .into()
//!     })
//!     .into()
//! }
//! ```
//!
//! Mini and strip windows have no chrome (skill, section 6): apps show their glance view there
//! instead of a shell. A shell built for them anyway looks like the narrow one.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, scrollable, stack};
use iced::{Alignment, Background, Element, Fill, Padding};

use torro_core::AppId;
use torro_core::i18n::tr;

use crate::icon::{self, Icon};
use crate::layout::SizeClass;
use crate::style;
use crate::theme::{self, palette, radius, size, space};
use crate::typo;
use crate::widgets::{self, Anchor, MenuItem, MenuSection, Tone, Trail};

/// The one-pixel lines of the design.
const LINE: f32 = 1.0;
/// Height of the app switcher row (`torro.css`: `.switcher{height:32px}`).
const SWITCHER_HEIGHT: f32 = size::CONTROL;
/// Edge of the red app logo in the switcher.
const LOGO: f32 = 20.0;
/// Gap between the rows of the sidebar (`torro.css`: `.sidebar{gap:2px}`).
const SIDEBAR_GAP: f32 = 2.0;

/// Whether `class` shows the sidebar beside the content (only the wide layout). Otherwise the
/// header carries [`sidebar_toggle`] and a navigation menu ([`widgets::menu_button`]).
pub fn has_sidebar(class: SizeClass) -> bool {
    class == SizeClass::Wide
}

/// Whether `class` shows the properties column (only the wide layout). Otherwise the detail shows
/// the properties as chips under its title ([`widgets::chip_row`]).
pub fn has_props(class: SizeClass) -> bool {
    class == SizeClass::Wide
}

/// The window being built by [`shell`]; finish it with `.into()`.
pub struct Shell<'a, M> {
    class: SizeClass,
    sidebar: Element<'a, M>,
    header: Element<'a, M>,
    body: Element<'a, M>,
    props: Option<Element<'a, M>>,
    drawer: Option<M>,
    overlay: Option<Element<'a, M>>,
}

/// The window of an app in the layout for `class` ([`crate::layout::of`] of the window size):
///
/// - **Wide:** `sidebar` (232, window background, 8 inside), then the content as an inset panel
///   (8 margin top/right/bottom, radius 8, 1-px line): `header` in a 44-high bar with a line below,
///   then `body`, and `props` in a 300-wide column at the right (scrolls on its own).
/// - **Medium, narrow (and mini, strip):** `header` and `body` flush with the window; `sidebar`
///   only appears as a drawer ([`Shell::drawer`]) and `props` is not shown.
///
/// The header bar adds the padding (16 left, 12 right; 8 and 8 in narrow), so `header` is just the
/// row from [`header_bar`]. `body` gets the free space and scrolls itself. Build `props` from
/// [`widgets::prop_section`]s stacked with `space::L`.
pub fn shell<'a, M: 'a>(
    class: SizeClass,
    sidebar: impl Into<Element<'a, M>>,
    header: impl Into<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
    props: Option<Element<'a, M>>,
) -> Shell<'a, M> {
    Shell {
        class,
        sidebar: sidebar.into(),
        header: header.into(),
        body: body.into(),
        props,
        drawer: None,
        overlay: None,
    }
}

impl<'a, M: Clone + 'a> Shell<'a, M> {
    /// Shows `layer` above the whole window: the open menu or floating card from
    /// [`widgets::floating`]. `None` when nothing is open.
    pub fn overlay(mut self, layer: Option<Element<'a, M>>) -> Self {
        self.overlay = layer;
        self
    }

    /// In the medium and narrow layouts, shows the sidebar as a drawer over the left edge while
    /// `open`; a click next to it sends `on_dismiss`. Open it from [`sidebar_toggle`]. No effect in
    /// the wide layout, where the sidebar is always there.
    pub fn drawer(mut self, open: bool, on_dismiss: M) -> Self {
        self.drawer = open.then_some(on_dismiss);
        self
    }
}

impl<'a, M: Clone + 'a> From<Shell<'a, M>> for Element<'a, M> {
    fn from(shell: Shell<'a, M>) -> Self {
        let Shell { class, sidebar, header, body, props, drawer: on_dismiss, overlay } = shell;
        let header = header_box(class, header);
        let mut layers = Vec::new();
        if class == SizeClass::Wide {
            layers.push(wide_window(sidebar, header, body, props));
        } else {
            let window = container(column![header, container(body).width(Fill).height(Fill)])
                .width(Fill)
                .height(Fill)
                .style(style::container::panel_flush);
            layers.push(window.into());
            if let Some(on_dismiss) = on_dismiss {
                layers.push(drawer(sidebar, on_dismiss));
            }
        }
        layers.extend(overlay);
        if layers.len() == 1 { layers.remove(0) } else { stack(layers).into() }
    }
}

/// The header bar: 44 high, the padding of the size class, a line below.
fn header_box<'a, M: 'a>(class: SizeClass, header: Element<'a, M>) -> Element<'a, M> {
    let (left, right) = match class {
        SizeClass::Wide | SizeClass::Medium => (space::L, space::M),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => (space::S, space::S),
    };
    let bar = container(header).width(Fill).center_y(size::HEADER).padding(Padding::new(0.0).left(left).right(right));
    column![bar, rule::horizontal(LINE).style(style::rule)].into()
}

fn wide_window<'a, M: 'a>(
    sidebar: Element<'a, M>,
    header: Element<'a, M>,
    body: Element<'a, M>,
    props: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let mut main = row![container(body).width(Fill).height(Fill)];
    if let Some(props) = props {
        let column = scrollable(container(props).width(Fill).padding(Padding {
            top: space::L,
            right: space::S,
            bottom: space::XL,
            left: space::S,
        }))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(size::PROPS - LINE)
        .height(Fill);
        main = main.push(rule::vertical(LINE).style(style::rule)).push(column);
    }
    // The 1-px padding keeps the content inside the panel's border, as `border-box` in the CSS.
    let panel = container(column![header, main.height(Fill)])
        .width(Fill)
        .height(Fill)
        .padding(LINE)
        .style(style::container::panel);
    let side = container(sidebar).width(size::SIDEBAR).height(Fill).padding(space::S);
    container(row![side, container(panel).padding(Padding { left: 0.0, ..Padding::new(size::INSET) })])
        .width(Fill)
        .height(Fill)
        .style(style::container::sidebar)
        .into()
}

/// The sidebar as a drawer over the left edge, with the menu shadow; a click beside it dismisses.
fn drawer<'a, M: Clone + 'a>(sidebar: Element<'a, M>, on_dismiss: M) -> Element<'a, M> {
    let catcher = opaque(mouse_area(blank().width(Fill).height(Fill)).on_press(on_dismiss));
    // The edge is a rule on the drawer's own surface: Iced leaves a translucent border see-through,
    // and the content below would shine through it.
    let side = container(sidebar).width(size::SIDEBAR - LINE).height(Fill).padding(space::S);
    let panel = container(row![side, rule::vertical(LINE).style(style::rule)]).height(Fill).style(|t| {
        let p = palette(t);
        container::Style { background: Some(Background::Color(p.bg)), shadow: p.shadow, ..container::Style::default() }
    });
    stack![catcher, opaque(panel)].into()
}

/// The content of the sidebar: the `switcher` row ([`app_switcher`]) with 12 below it, the `items`
/// (navigation, [`widgets::sidebar_group`]s with their rows) from the top, and the `footer` rows
/// ("Open Calendar ↗", "Settings") at the bottom. Rows are 2 apart.
pub fn sidebar<'a, M: 'a>(
    switcher: impl Into<Element<'a, M>>,
    items: Vec<Element<'a, M>>,
    footer: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let top = column![switcher.into(), blank().height(space::S)].extend(items).spacing(SIDEBAR_GAP);
    let bottom = column(footer).spacing(SIDEBAR_GAP);
    column![top, blank::vertical(), bottom].height(Fill).into()
}

/// The row inside the header bar: `leading` (sidebar toggle, back button), the `title`
/// ([`header_title`], or a [`widgets::menu_button`] in medium and narrow), free space, then
/// `trailing` (view, filter, exactly one primary button). 8 between all pieces, centred.
pub fn header_bar<'a, M: 'a>(
    leading: impl IntoIterator<Item = Element<'a, M>>,
    title: impl Into<Element<'a, M>>,
    trailing: impl IntoIterator<Item = Element<'a, M>>,
) -> Element<'a, M> {
    row(leading)
        .push(title)
        .push(blank::horizontal())
        .extend(trailing)
        .spacing(space::S)
        .align_y(Alignment::Center)
        .into()
}

/// The title of a header bar: an optional icon (16, `text_3`), `title` in `BODY_STRONG` and a quiet
/// `note` in `text_3` ("5 open", "Week 41"), 6 apart.
pub fn header_title<'a, M: 'a>(
    icon_: Option<Icon>,
    title: impl IntoFragment<'a>,
    note: Option<String>,
) -> Element<'a, M> {
    let mut content = row![].spacing(6.0).align_y(Alignment::Center);
    if let Some(which) = icon_ {
        content = content.push(widgets::toned(which, icon::SIZE, Tone::Text3));
    }
    content = content.push(typo::text(typo::BODY_STRONG, title).wrapping(Wrapping::None));
    if let Some(note) = note {
        content = content.push(typo::text(typo::BODY, note).style(theme::text_3).wrapping(Wrapping::None));
    }
    content.into()
}

/// The header button that opens the sidebar drawer in the medium and narrow layouts.
pub fn sidebar_toggle<'a, M: Clone + 'a>(on_press: M) -> Element<'a, M> {
    widgets::icon_button(Icon::PanelLeft, tr("Show sidebar"), Some(on_press))
}

/// What the app switcher asks the app to do; the app wraps it in its own message
/// (`Message::Switcher(Switcher)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switcher {
    /// Open or close the switcher menu ([`switcher_menu`]).
    Toggle,
    /// Start or raise another app of the suite: send `Request::OpenApp { app: binary }` to the
    /// core, then close the menu. Carries [`AppId::binary`].
    OpenApp(&'static str),
    /// The search button next to the switcher.
    Search,
    /// The "New" button next to the switcher (new task, new event …).
    New,
}

/// The short name of an app in the switcher ("Tasks", "Calendar", "Workplace").
fn app_name(app: AppId) -> &'static str {
    match app.binary {
        "torrotasks" => tr("Tasks"),
        "torrocalendar" => tr("Calendar"),
        _ => tr("Workplace"),
    }
}

fn app_icon(app: AppId) -> Icon {
    match app.binary {
        "torrotasks" => Icon::ListTodo,
        "torrocalendar" => Icon::Calendar,
        _ => Icon::LayoutGrid,
    }
}

/// The top row of the sidebar, like Linear's workspace menu: the red logo, the name of `current`
/// with a chevron (registers `anchor` and sends [`Switcher::Toggle`]), then search and a framed
/// "New" button. `on` wraps the switcher's messages in the app's (`Message::Switcher`).
pub fn app_switcher<'a, M: Clone + 'a>(current: AppId, anchor: &Anchor, on: fn(Switcher) -> M) -> Element<'a, M> {
    let logo = container(
        typo::text(typo::CAPTION, "T").style(|t| iced::widget::text::Style { color: Some(palette(t).on_accent) }),
    )
    .center(LOGO)
    .style(|t| container::Style {
        background: Some(Background::Color(palette(t).accent)),
        border: iced::border::rounded(radius::XS),
        ..container::Style::default()
    });
    let name = row![
        logo,
        typo::text(typo::BODY_STRONG, app_name(current)).style(theme::text).wrapping(Wrapping::None),
        widgets::toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text3),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    // 6 inside, as `.switcher{padding:0 6px}`, so the logo lines up with the row icons below.
    let trigger = button(container(name).center_y(Fill))
        .width(Fill)
        .height(SWITCHER_HEIGHT)
        .padding(Padding::new(0.0).left(6.0).right(6.0))
        .style(style::button::sidebar_item(false))
        .on_press(on(Switcher::Toggle));
    row![
        widgets::anchored(anchor, trigger),
        widgets::icon_button(Icon::Search, tr("Search"), Some(on(Switcher::Search))),
        widgets::boxed_button(Icon::SquarePen, tr("New"), Some(on(Switcher::New))),
    ]
    .spacing(SIDEBAR_GAP)
    .align_y(Alignment::Center)
    .into()
}

/// The menu of the app switcher: every app of the suite ([`AppId::ALL`]), `current` marked. A click
/// on another app sends [`Switcher::OpenApp`], on the current one [`Switcher::Toggle`] (closes).
/// Show it with `widgets::floating(&anchor, Placement::Below(Align::Start), switcher_menu(..), close)`.
pub fn switcher_menu<'a, M: Clone + 'a>(current: AppId, on: fn(Switcher) -> M) -> Element<'a, M> {
    let items = AppId::ALL
        .iter()
        .map(|&app| {
            let is_current = app.binary == current.binary;
            let message = if is_current { Switcher::Toggle } else { Switcher::OpenApp(app.binary) };
            let trail = if is_current { Trail::None } else { Trail::Icon(Icon::ArrowUpRight) };
            MenuItem::new(app_icon(app), app_name(app), on(message)).trail(trail).current(is_current)
        })
        .collect();
    widgets::menu(vec![MenuSection::titled(tr("Apps"), items)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Switcher(Switcher),
        Close,
    }

    #[test]
    fn only_the_wide_layout_has_sidebar_and_properties() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium] {
            assert!(!has_sidebar(class) && !has_props(class), "{class:?}");
        }
        assert!(has_sidebar(SizeClass::Wide) && has_props(SizeClass::Wide));
    }

    #[test]
    fn every_suite_app_has_a_short_name_and_icon() {
        assert_eq!(app_name(AppId::TASKS), tr("Tasks"));
        assert_eq!(app_name(AppId::CALENDAR), tr("Calendar"));
        assert_eq!(app_name(AppId::WORKPLACE), tr("Workplace"));
        assert_ne!(app_icon(AppId::TASKS), app_icon(AppId::CALENDAR));
    }

    #[test]
    fn switcher_messages_name_the_binary_to_open() {
        let wrap: fn(Switcher) -> Msg = Msg::Switcher;
        assert_eq!(wrap(Switcher::OpenApp(AppId::CALENDAR.binary)), Msg::Switcher(Switcher::OpenApp("torrocalendar")));
    }

    #[test]
    fn shells_build_in_every_class() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let anchor = Anchor::new();
            let side = sidebar(app_switcher(AppId::TASKS, &anchor, Msg::Switcher), vec![], vec![]);
            let header = header_bar([sidebar_toggle(Msg::Close)], header_title(Some(Icon::Sun), "Today", None), []);
            let props: Option<Element<'_, Msg>> = has_props(class).then(|| blank().into());
            let layer = Some(widgets::floating(
                &anchor,
                widgets::Placement::Below(widgets::Align::Start),
                switcher_menu(AppId::TASKS, Msg::Switcher),
                Msg::Close,
            ));
            let _: Element<'_, Msg> =
                shell(class, side, header, blank(), props).drawer(true, Msg::Close).overlay(layer).into();
        }
    }
}
