//! The window frame every app of the suite shares, after Linear (design skill, section 0):
//! sidebar on the window background, the content as an inset panel with a 44-high header bar,
//! and the properties column on the right – in the wide layout. Medium windows shrink the sidebar
//! to a rail of icons (its full form opens as a drawer) and drop the properties (they become
//! chips); narrow windows drop the sidebar too (its rows move into a menu at the top left, or into
//! a drawer), and the content runs flush to the window edges.
//!
//! ```text
//! Wide    ┌ sidebar 232 ┐┌ panel (inset 8, radius 8) ───────────────────────────┐
//!         │ [T] App ▾ 🔍 ✎││ header 44                                         │
//!         │ rows …      │├──────────────────────────────────┬─────────────────┤
//!         │             ││ body                             │ properties 300  │
//!         └─────────────┘└──────────────────────────────────┴─────────────────┘
//! Medium  ┌rail┐┌ panel (inset 8, radius 8) ──────────────────────────────────────┐
//!         │ T  ││ header 44                                                      │
//!         │ ☀  │├────────────────────────────────────────────────────────────────┤
//!         │ ○  ││ body                                                           │
//!         └────┘└────────────────────────────────────────────────────────────────┘
//! Narrow  ┌ header 44 (flush) ─────────────────────────────────────────────────┐
//!         │ body                                                               │
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
//!
//! # Collapsed sidebar (rail)
//!
//! The sidebar has three states: **expanded** (232, as above), **rail** (only the icons, 52 wide,
//! names in tooltips, group titles as lines) and **drawer** (the expanded sidebar over the content,
//! [`Shell::drawer`]). In the wide layout the user picks expanded or rail with the panel button at
//! the top of the sidebar ([`sidebar_head`]) or the key `[` ([`is_sidebar_key`]); the app stores the
//! choice with [`crate::ui_state`]. The medium layout always shows the rail, and its panel button
//! opens the expanded sidebar as a drawer. Narrow windows keep the drawer alone.
//!
//! The rows know how to draw themselves in both modes ([`widgets::sidebar_item_in`],
//! [`widgets::sidebar_group_in`]), so an app writes its sidebar once, takes the mode as a parameter
//! and builds it twice where both are needed:
//!
//! ```ignore
//! use torro_gui::shell::{self, SidebarMode};
//!
//! fn sidebar(&self, mode: SidebarMode, toggle: Message, switcher: &Anchor) -> Element<'_, Message> {
//!     let items = vec![widgets::sidebar_item_in(mode, Icon::Sun, tr("Today"), Trail::Count(5), true, Message::Today)];
//!     let head = shell::sidebar_head(mode, AppId::TASKS, switcher, Message::Switcher, toggle);
//!     shell::sidebar(head, items, vec![])
//! }
//!
//! // view: the panel button flips the stored choice in wide windows, else it opens or closes the drawer.
//! let toggle = if shell::has_sidebar(class) { Message::ToggleSidebar } else { Message::ToggleDrawer };
//! let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
//!     .then(|| self.sidebar(SidebarMode::Rail, toggle.clone(), &switcher));
//! shell(class, self.sidebar(SidebarMode::Expanded, toggle, &switcher), header, body, props)
//!     .rail(rail)
//!     .drawer(self.drawer, Message::CloseMenu)
//!     .into()
//! ```
//!
//! The drawer of the medium and narrow layouts dims the window behind it (scrim) and has a shadow.
//!
//! While something is dragged over the sidebar, the rows that may take it are wrapped in
//! [`drop_target`]: they report the pointer and show a ring while it is on them.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, scrollable, stack};
use iced::{Alignment, Background, Border, Color, Element, Fill, Padding, Shadow, Vector, keyboard};
use serde::{Deserialize, Serialize};

use torro_core::AppId;
use torro_core::i18n::tr;
use torro_pim::{PimAccount, account::account_of};

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
/// Width of the collapsed sidebar (rail): rows as wide as a large control (36), 8 at both sides –
/// room for a 16 icon on its hover surface, and the icons sit on the same axis as the logo.
pub const RAIL: f32 = size::CONTROL_LG + 2.0 * space::S;

/// How a sidebar is drawn. The user's choice for the wide layout, stored per app in
/// [`crate::ui_state::UiState::sidebar`] (`"expanded"` / `"rail"` in `ui.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarMode {
    /// 232 wide: icons, names, counters, group titles (design skill, section 0).
    #[default]
    Expanded,
    /// [`RAIL`] wide: only the icons, names and counters in tooltips, group titles as lines, the app
    /// switcher as its logo. Apps leave out what has no icon (the small month, the version line).
    Rail,
}

impl SidebarMode {
    /// The other mode: what the panel button and `[` switch to in the wide layout.
    pub fn toggled(self) -> Self {
        match self {
            SidebarMode::Expanded => SidebarMode::Rail,
            SidebarMode::Rail => SidebarMode::Expanded,
        }
    }
}

/// The sidebar a window of `class` shows beside its content, given the user's `choice`: the
/// choice in the wide layout, always the rail in the medium one, none in the smaller ones (they
/// open the sidebar only as a drawer). Pass the rail to [`Shell::rail`] when this is
/// `Some(SidebarMode::Rail)`.
pub fn sidebar_mode(class: SizeClass, choice: SidebarMode) -> Option<SidebarMode> {
    match class {
        SizeClass::Wide => Some(choice),
        SizeClass::Medium => Some(SidebarMode::Rail),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => None,
    }
}

/// Whether a key press is the sidebar shortcut `[` (as in Linear): the app then flips its choice
/// (wide) or opens or closes the drawer (medium, narrow) – the same as the panel button. Ctrl and
/// Alt are allowed, because some layouts type `[` with AltGr (German: AltGr+8, which Windows
/// reports as Ctrl+Alt); the logo key is not. Check it before an app drops keys with modifiers,
/// and only for keys no widget captured (a text field types the bracket).
pub fn is_sidebar_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    !modifiers.logo() && matches!(key.as_ref(), keyboard::Key::Character("["))
}

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
    rail: Option<Element<'a, M>>,
    drawer: Option<M>,
    overlay: Option<Element<'a, M>>,
}

/// The window of an app in the layout for `class` ([`crate::layout::of`] of the window size):
///
/// - **Wide:** `sidebar` (232, window background, 8 inside), then the content as an inset panel
///   (8 margin top/right/bottom, radius 8, 1-px line): `header` in a 44-high bar with a line below,
///   then `body`, and `props` in a 300-wide column at the right (scrolls on its own).
/// - **Medium, narrow (and mini, strip):** `header` and `body` flush with the window; `sidebar`
///   only appears as a drawer ([`Shell::drawer`]) and `props` is not shown. With a rail
///   ([`Shell::rail`]; wide and medium) the content is inset beside the rail instead.
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
        rail: None,
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

    /// Shows `rail` – the sidebar built in [`SidebarMode::Rail`] – beside the content instead of the
    /// expanded `sidebar`: pass it when [`sidebar_mode`] says `Some(SidebarMode::Rail)` (wide with
    /// the rail chosen, medium always), `None` otherwise. The content is inset as in the wide layout
    /// and gets the width the rail leaves; nothing overlaps. In the medium layout the expanded
    /// `sidebar` still opens as drawer over it. Ignored in the narrow (and mini, strip) layouts.
    pub fn rail(mut self, rail: Option<Element<'a, M>>) -> Self {
        self.rail = rail;
        self
    }

    /// In the medium and narrow layouts, shows the sidebar as a drawer over the left edge while
    /// `open`; a click next to it sends `on_dismiss`. Open it from [`sidebar_toggle`] or the panel
    /// button of the rail ([`sidebar_head`]). No effect in the wide layout, where the sidebar (or the
    /// rail) is always there.
    pub fn drawer(mut self, open: bool, on_dismiss: M) -> Self {
        self.drawer = open.then_some(on_dismiss);
        self
    }
}

impl<'a, M: Clone + 'a> From<Shell<'a, M>> for Element<'a, M> {
    fn from(shell: Shell<'a, M>) -> Self {
        let Shell { class, sidebar, header, body, props, rail, drawer: on_dismiss, overlay } = shell;
        let header = header_box(class, header);
        let mut layers = Vec::new();
        let rail = rail.filter(|_| matches!(class, SizeClass::Wide | SizeClass::Medium));
        if class == SizeClass::Wide {
            match rail {
                Some(rail) => layers.push(inset_window(rail, RAIL, header, body, props)),
                None => layers.push(inset_window(sidebar, size::SIDEBAR, header, body, props)),
            }
        } else if let Some(rail) = rail {
            layers.push(inset_window(rail, RAIL, header, body, props));
            if let Some(on_dismiss) = on_dismiss {
                layers.push(drawer(sidebar, on_dismiss));
            }
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

/// The sidebar (`width` 232) or the rail ([`RAIL`]) on the window background, and the content as
/// an inset panel beside it.
fn inset_window<'a, M: 'a>(
    sidebar: Element<'a, M>,
    width: f32,
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
    let side = container(sidebar).width(width).height(Fill).padding(space::S);
    container(row![side, container(panel).padding(Padding { left: 0.0, ..Padding::new(size::INSET) })])
        .width(Fill)
        .height(Fill)
        .style(style::container::sidebar)
        .into()
}

/// The sidebar as a drawer over the left edge: the rest of the window is dimmed with the scrim (as
/// behind a dialog) and the drawer casts the menu shadow to the right; a click on the scrim
/// dismisses.
fn drawer<'a, M: Clone + 'a>(sidebar: Element<'a, M>, on_dismiss: M) -> Element<'a, M> {
    let scrim = container(blank().width(Fill).height(Fill)).style(|t| container::Style {
        background: Some(Background::Color(palette(t).scrim)),
        ..container::Style::default()
    });
    let catcher = opaque(mouse_area(scrim).on_press(on_dismiss));
    // The edge is a rule on the drawer's own surface: Iced leaves a translucent border see-through,
    // and the content below would shine through it.
    let side = container(sidebar).width(size::SIDEBAR - LINE).height(Fill).padding(space::S);
    let panel = container(row![side, rule::vertical(LINE).style(style::rule)]).height(Fill).style(|t| {
        let p = palette(t);
        // The menu shadow falls sideways here: the drawer's edge is vertical.
        let shadow = Shadow { offset: Vector::new(space::M, 0.0), ..p.shadow };
        container::Style { background: Some(Background::Color(p.bg)), shadow, ..container::Style::default() }
    });
    stack![catcher, opaque(panel)].into()
}

/// The content of the sidebar: the `switcher` row ([`app_switcher`], or [`sidebar_head`] with the
/// panel button) with 8 below it, the `items` (navigation, [`widgets::sidebar_group`]s with their
/// rows) from the top, and the `footer` rows ("Open Calendar ↗", "Settings") at the bottom. Rows
/// are 2 apart. The same for both [`SidebarMode`]s: build the pieces in the mode you want.
pub fn sidebar<'a, M: 'a>(
    switcher: impl Into<Element<'a, M>>,
    items: Vec<Element<'a, M>>,
    footer: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let top = column![switcher.into(), blank().height(space::S)].extend(items).spacing(SIDEBAR_GAP);
    let bottom = column(footer).spacing(SIDEBAR_GAP);
    column![top, blank::vertical(), bottom].height(Fill).into()
}

/// A sidebar row ([`widgets::sidebar_item_in`], either mode) that takes what is being dragged –
/// the tasks app drags a task onto a list (task G.6c). The app wraps only the rows that may take
/// the dragged item, and only while something is dragged:
///
/// - `on_enter` / `on_exit` report the pointer coming onto the row and leaving it. The app keeps
///   which row it is over and acts on the drop where the mouse button is released; a press that
///   began elsewhere never presses the row's button.
/// - `over`: the pointer is on the row. A 2-px ring in `focus_ring` with the row's radius lies on
///   top of its hover surface, so the target stands apart from the chosen row (`selected`, no
///   ring) and from rows that take nothing (hover only). The ring layer is there in both states,
///   so the row keeps its widget state when the pointer comes and goes.
///
/// ```ignore
/// let row = widgets::sidebar_item_in(mode, mark, name, trail, active, Message::Show(id.clone()));
/// shell::drop_target(row, drag.over.as_ref() == Some(&id), Message::DragOver(id.clone()), Message::DragLeave(id))
/// ```
pub fn drop_target<'a, M: Clone + 'a>(row: Element<'a, M>, over: bool, on_enter: M, on_exit: M) -> Element<'a, M> {
    let ring = container(blank().width(Fill).height(Fill)).style(move |t| container::Style {
        border: Border {
            color: if over { palette(t).focus_ring } else { Color::TRANSPARENT },
            width: style::FOCUS_RING_WIDTH,
            radius: radius::SM.into(),
        },
        ..container::Style::default()
    });
    mouse_area(stack![row, ring].width(Fill)).on_enter(on_enter).on_exit(on_exit).into()
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
        "torroinbox" => tr("Inbox"),
        "torrotasks" => tr("Tasks"),
        "torrocalendar" => tr("Calendar"),
        _ => tr("Workplace"),
    }
}

fn app_icon(app: AppId) -> Icon {
    match app.binary {
        "torroinbox" => Icon::Inbox,
        "torrotasks" => Icon::ListTodo,
        "torrocalendar" => Icon::Calendar,
        _ => Icon::LayoutGrid,
    }
}

/// The top row of the sidebar, like Linear's workspace menu: the red logo, the name of `current`
/// with a chevron (registers `anchor` and sends [`Switcher::Toggle`]), then search and a framed
/// "New" button. `on` wraps the switcher's messages in the app's (`Message::Switcher`).
pub fn app_switcher<'a, M: Clone + 'a>(current: AppId, anchor: &Anchor, on: fn(Switcher) -> M) -> Element<'a, M> {
    switcher_row(current, anchor, on, None)
}

/// The top of the sidebar with the panel button that collapses and expands it:
///
/// - [`SidebarMode::Expanded`]: the [`app_switcher`] row, the panel button (`panel-left`,
///   "Collapse sidebar") at its end.
/// - [`SidebarMode::Rail`]: the logo alone (opens the switcher menu, registers `anchor`; the app's
///   name is its tooltip), search, "New" and the panel button ("Expand sidebar") below each other.
///
/// `on_toggle` is what the panel button sends: in the wide layout the app flips its stored choice
/// ([`SidebarMode::toggled`], saved with [`crate::ui_state`]); in the medium and narrow layouts it
/// opens the drawer from the rail and closes it from the drawer. [`has_sidebar`] tells the cases
/// apart in `view`, and `[` ([`is_sidebar_key`]) does the same.
pub fn sidebar_head<'a, M: Clone + 'a>(
    mode: SidebarMode,
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: M,
) -> Element<'a, M> {
    match mode {
        SidebarMode::Expanded => switcher_row(current, anchor, on, Some(on_toggle)),
        SidebarMode::Rail => {
            // The logo on a full-width row, as the other rows of the rail.
            let logo = button(center_x(logo()))
                .width(Fill)
                .height(SWITCHER_HEIGHT)
                .padding(0)
                .style(style::button::sidebar_item(false))
                .on_press(on(Switcher::Toggle));
            column![
                widgets::anchored(anchor, widgets::tooltip_right(logo, app_name(current), Trail::None)),
                widgets::icon_button(Icon::Search, tr("Search"), Some(on(Switcher::Search))),
                widgets::boxed_button(Icon::SquarePen, tr("New"), Some(on(Switcher::New))),
                widgets::icon_button(Icon::PanelLeft, tr("Expand sidebar"), Some(on_toggle)),
            ]
            .spacing(SIDEBAR_GAP)
            .align_x(Alignment::Center)
            .into()
        }
    }
}

/// `content` centred across the full width, at its own height.
fn center_x<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).center_x(Fill).center_y(Fill).into()
}

/// The red app logo (20 × 20, radius 4, a white "T").
fn logo<'a, M: 'a>() -> Element<'a, M> {
    container(typo::text(typo::CAPTION, "T").style(|t| iced::widget::text::Style { color: Some(palette(t).on_accent) }))
        .center(LOGO)
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).accent)),
            border: iced::border::rounded(radius::XS),
            ..container::Style::default()
        })
        .into()
}

/// The switcher row of [`app_switcher`], with the panel button at its end when `on_toggle` is given.
fn switcher_row<'a, M: Clone + 'a>(
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: Option<M>,
) -> Element<'a, M> {
    let name = row![
        logo(),
        typo::text(typo::BODY_STRONG, app_name(current)).style(theme::text).wrapping(Wrapping::None),
        widgets::toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text3),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    // 6 inside, as `.switcher{padding:0 6px}`, so the logo lines up with the row icons below.
    let trigger = button(container(name).center_y(Fill).clip(true))
        .width(Fill)
        .height(SWITCHER_HEIGHT)
        .padding(Padding::new(0.0).left(6.0).right(6.0))
        .style(style::button::sidebar_item(false))
        .on_press(on(Switcher::Toggle));
    let mut top = row![
        widgets::anchored(anchor, trigger),
        widgets::icon_button(Icon::Search, tr("Search"), Some(on(Switcher::Search))),
        widgets::boxed_button(Icon::SquarePen, tr("New"), Some(on(Switcher::New))),
    ];
    if let Some(on_toggle) = on_toggle {
        top = top.push(widgets::icon_button(Icon::PanelLeft, tr("Collapse sidebar"), Some(on_toggle)));
    }
    top.spacing(SIDEBAR_GAP).align_y(Alignment::Center).into()
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

/// The part of a sidebar list (calendars, task lists) that belongs to one account.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountGroup<'a, T> {
    /// The owner; `None` for collections no known account owns (a stale cache), shown last
    /// without a title.
    pub account: Option<&'a PimAccount>,
    pub items: Vec<T>,
}

/// Splits `items` (calendars, lists) by the account their collection id names (`id`, see
/// [`torro_pim::account`]), for a sidebar that groups per account (task M1.3). The groups follow
/// the order of `accounts`, the items keep their order, accounts without items get no group.
///
/// `Err(items)` – the items given back – when fewer than two accounts own any of the items: one
/// account looks like it always did, a flat list under the app's own title, so the caller needs no
/// second code path, only `Err(items) => flat`. Only owners count, so an account without
/// collections of the kind the app shows – a GitHub account in the calendar, or one without
/// filter lists in the tasks app – brings no account titles (task G.3).
pub fn account_groups<'a, T>(
    accounts: &'a [PimAccount],
    items: Vec<T>,
    id: impl Fn(&T) -> &str,
) -> Result<Vec<AccountGroup<'a, T>>, Vec<T>> {
    let owners: Vec<Option<usize>> = items
        .iter()
        .map(|item| {
            account_of(id(item), accounts, |a| a.id.as_str())
                .and_then(|owner| accounts.iter().position(|a| std::ptr::eq(a, owner)))
        })
        .collect();
    let mut owning: Vec<usize> = owners.iter().flatten().copied().collect();
    owning.sort_unstable();
    owning.dedup();
    if owning.len() < 2 {
        return Err(items);
    }
    let mut groups: Vec<AccountGroup<'a, T>> =
        accounts.iter().map(|account| AccountGroup { account: Some(account), items: Vec::new() }).collect();
    let mut strays = Vec::new();
    for (item, owner) in items.into_iter().zip(owners) {
        match owner {
            Some(index) => groups[index].items.push(item),
            None => strays.push(item),
        }
    }
    groups.retain(|group| !group.items.is_empty());
    if !strays.is_empty() {
        groups.push(AccountGroup { account: None, items: strays });
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_pim::AccountState;

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
    fn the_rail_shows_where_the_user_chose_it_and_in_medium() {
        for choice in [SidebarMode::Expanded, SidebarMode::Rail] {
            assert_eq!(sidebar_mode(SizeClass::Wide, choice), Some(choice), "wide follows the choice");
            assert_eq!(sidebar_mode(SizeClass::Medium, choice), Some(SidebarMode::Rail), "medium has the rail");
            for class in [SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
                assert_eq!(sidebar_mode(class, choice), None, "{class:?} has only the drawer");
            }
        }
    }

    fn account(id: &str) -> PimAccount {
        let state =
            AccountState::Connected { server: "s".into(), login_name: "l".into(), last_sync: None, problem: None };
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state, scopes: None }
    }

    #[test]
    fn one_account_or_none_stays_a_flat_list() {
        let items = vec!["a/x".to_string()];
        assert_eq!(account_groups(&[], items.clone(), |i| i), Err(items.clone()));
        assert_eq!(account_groups(&[account("a")], items.clone(), |i| i), Err(items));
    }

    #[test]
    fn two_accounts_split_the_items_in_account_order() {
        let accounts = [account("a"), account("b"), account("c")];
        let items: Vec<String> = ["b/1", "a/1", "b/2", "a/2", "gone/3"].map(String::from).to_vec();
        let groups = account_groups(&accounts, items, |i| i).expect("grouped");
        let shape: Vec<(Option<&str>, Vec<&str>)> = groups
            .iter()
            .map(|g| (g.account.map(|a| a.id.as_str()), g.items.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            shape,
            vec![(Some("a"), vec!["a/1", "a/2"]), (Some("b"), vec!["b/1", "b/2"]), (None, vec!["gone/3"]),],
            "account c has no items and no group; strays come last"
        );
    }

    #[test]
    fn an_account_without_items_brings_no_titles() {
        // A Nextcloud account and a GitHub account without filter lists (or, in the calendar, a
        // GitHub account at all): one owner, so the flat list as before.
        let accounts = [account("a"), account("github:mahype")];
        let items: Vec<String> = ["a/1", "a/2", "gone/3"].map(String::from).to_vec();
        assert_eq!(account_groups(&accounts, items.clone(), |i| i), Err(items));
        let with_list: Vec<String> = ["a/1", "github:mahype/bugs"].map(String::from).to_vec();
        let groups = account_groups(&accounts, with_list, |i| i).expect("two owners");
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn toggling_switches_between_expanded_and_rail() {
        assert_eq!(SidebarMode::default(), SidebarMode::Expanded);
        assert_eq!(SidebarMode::Expanded.toggled(), SidebarMode::Rail);
        assert_eq!(SidebarMode::Rail.toggled(), SidebarMode::Expanded);
    }

    #[test]
    fn the_bracket_is_the_sidebar_key() {
        use keyboard::{Key, Modifiers};
        let bracket = Key::Character("[".into());
        assert!(is_sidebar_key(&bracket, Modifiers::empty()));
        assert!(is_sidebar_key(&bracket, Modifiers::CTRL | Modifiers::ALT), "AltGr on Windows");
        assert!(!is_sidebar_key(&bracket, Modifiers::LOGO));
        assert!(!is_sidebar_key(&Key::Character("]".into()), Modifiers::empty()));
    }

    #[test]
    fn rails_and_heads_build_in_every_class() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let anchor = Anchor::new();
            let side = |mode| {
                let items = vec![
                    widgets::sidebar_item_in(mode, Icon::Sun, "Today", Trail::Count(5), true, Msg::Close),
                    widgets::sidebar_group_in(mode, "Lists", Some((true, Msg::Close))),
                ];
                sidebar(sidebar_head(mode, AppId::TASKS, &anchor, Msg::Switcher, Msg::Close), items, vec![])
            };
            let rail =
                (sidebar_mode(class, SidebarMode::Rail) == Some(SidebarMode::Rail)).then(|| side(SidebarMode::Rail));
            let header = header_bar([], header_title(None, "Today", None), []);
            let _: Element<'_, Msg> = shell(class, side(SidebarMode::Expanded), header, blank(), None)
                .rail(rail)
                .drawer(true, Msg::Close)
                .into();
        }
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
