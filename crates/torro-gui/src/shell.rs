//! The window frame every app of the suite shares, after Linear (design skill, section 0):
//! sidebar on the window background, the content beside it with a header bar ([`size::header()`],
//! 64 in the standard design), and the properties column on the right – in the wide layout. Medium
//! windows shrink the sidebar to a rail of icons (its full form opens as a drawer) and drop the
//! properties (they become chips); narrow windows drop the sidebar too (its rows move into a menu at
//! the top left, or into a drawer), and the content runs flush to the window edges.
//!
//! Beside the sidebar or rail the content is **flush** when the design's [`layout::inset()`] is 0
//! (every built-in design since issue #291): the `panel` surface runs to the window's edges, parted
//! from the sidebar by a line. A design with an inset draws it as an inset panel instead (that margin
//! on top, right and bottom, radius `md`, a line around it), as the suite did before.
//!
//! ```text
//! Wide    ┌ sidebar 240 ┐│ panel (flush) ────────────────────────────────────────┐
//!         │      🔍 ✎   │ header 64                                             │
//!         │ rows …      │├──────────────────────────────────┬──────────────────┤
//!         │             ││ body                             │ properties 300   │
//!         └─────────────┘└──────────────────────────────────┴──────────────────┘
//! Medium  ┌rail┐│ panel (flush) ──────────────────────────────────────────────────┐
//!         │ 🔍 ││ header 64                                                       │
//!         │ ☀  │├─────────────────────────────────────────────────────────────────┤
//!         │ ○  ││ body                                                            │
//!         └────┘└─────────────────────────────────────────────────────────────────┘
//! Narrow  ┌ header 64 (flush) ─────────────────────────────────────────────────┐
//!         │ body                                                               │
//!         └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! A whole window, as the tasks app builds it:
//!
//! ```ignore
//! use torro_gui::shell::{self, SidebarMode, header_bar, header_title, shell};
//! use torro_gui::widgets::{self, Anchor, Trail};
//! use torro_gui::layout::{self, SizeClass};
//!
//! fn view(&self) -> Element<'_, Message> {
//!     iced::widget::responsive(|size| {
//!         let class = layout::of(size);
//!         let nav = Anchor::new();
//!         let sidebar = shell::sidebar(
//!             shell::sidebar_tools(SidebarMode::Expanded, true, Message::Switcher),
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
//!             .overlay(self.open_menu(&nav))
//!             .into()
//!     })
//!     .into()
//! }
//! ```
//!
//! # Top row (issue #459)
//!
//! The main windows of the suite draw no header bar and no band of [`titlebar`] above it any more, but
//! one top row ([`TopRow`], [`Shell::top_row`]) on every system: beside the traffic lights the panel
//! button, then the app's header, at the right the settings ([`SETTINGS_ICON`]). The sidebar begins
//! with its tools ([`sidebar_tools`]). Windows without a shell (signing in, a glance, a compose window)
//! keep [`titlebar`].
//!
//! There is no app switcher in the window any more (issue #530): the person changes apps from the tray
//! menu, the dock or task bar and the control centre's Apps; jumps with a reason ("Open in …") stay.
//!
//! Mini and strip windows have no chrome (skill, section 6): apps show their glance view there
//! instead of a shell. A shell built for them anyway looks like the narrow one.
//!
//! # Collapsed sidebar (rail)
//!
//! The sidebar has three states: **expanded** (240, as above), **rail** (only the icons, [`rail()`] wide,
//! names in tooltips, group titles as lines) and **drawer** (the expanded sidebar over the content,
//! [`Shell::drawer`]). In the wide layout the user picks expanded or rail with the panel button of the
//! top row ([`TopRow::sidebar`]) or the key `[` ([`is_sidebar_key`]); the app stores the
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
//! fn sidebar(&self, mode: SidebarMode) -> Element<'_, Message> {
//!     let items = vec![widgets::sidebar_item_in(mode, Icon::Sun, tr("Today"), Trail::Count(5), true, Message::Today)];
//!     shell::sidebar(shell::sidebar_tools(mode, true, Message::Switcher), items, vec![])
//! }
//!
//! // view: the panel button of the top row flips the stored choice in wide windows, else it opens or
//! // closes the drawer.
//! let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
//!     .then(|| self.sidebar(SidebarMode::Rail));
//! shell(class, self.sidebar(SidebarMode::Expanded), header, body, props)
//!     .rail(rail)
//!     .drawer(self.drawer, Message::CloseMenu)
//!     .into()
//! ```
//!
//! The drawer of the medium and narrow layouts dims the window behind it (scrim) and has a shadow.
//!
//! # Width of the expanded sidebar
//!
//! An app may let the person drag the expanded sidebar wider or narrower (issue #376, first the mail
//! window): [`Shell::sidebar_width`] lays a handle over its right edge that works like the divider of
//! [`crate::split`], and [`sidebar_limits`] bounds the width. The rail, the drawer and
//! [`Cover::Drawer`] keep [`size::sidebar()`].
//!
//! The properties column is dragged the same way (issue #604): [`Shell::props_width`] puts the
//! divider of [`split::split_end`] between the body and it, bounded by [`props_limits`].
//!
//! While something is dragged over the sidebar, the rows that may take it are wrapped in
//! [`drop_target`]: they report the pointer and show a ring while it is on them.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, scrollable, stack};
use iced::{Alignment, Background, Border, Color, Element, Fill, Padding, Shadow, Size, Vector, keyboard, mouse};
use serde::{Deserialize, Serialize};

use torro_core::AppId;
use torro_core::i18n::tr;
use torro_pim::{PimAccount, account::account_of};

use crate::icon::{self, Icon};
use crate::layout::{self, SizeClass};
use crate::scheme;
use crate::split::{self, SplitEvent};
use crate::style;
use crate::theme::{self, palette, radius, size, space};
use crate::typo;
use crate::widgets::{self, Tone};

use crate::themed::in_sidebar;

/// The one-pixel lines of the design.
const LINE: f32 = 1.0;
/// Height of the sidebar's first row with search and "New" (`torro.css`: `.switcher{height:32px}`).
fn tools_height() -> f32 {
    size::control()
}
/// Edge of the app's mark ([`app_mark`]).
const LOGO: f32 = 20.0;
/// Gap between the rows of the sidebar (`torro.css`: `.sidebar{gap:2px}`).
pub(crate) const SIDEBAR_GAP: f32 = 2.0;
/// Width of the collapsed sidebar (rail) and of the collapsed action rail of the mail window: the
/// design's [`size::rail()`] (issue #291), 60 in the standard design – rows as wide as a large control
/// (40) with 10 at both sides, room for a 16 icon on its hover surface, and the icons sit on the same
/// axis. The rules keep it at least as wide as [`size::control_lg()`].
pub fn rail() -> f32 {
    size::rail()
}

/// How a sidebar is drawn. The user's choice for the wide layout, stored per app in
/// [`crate::ui_state::UiState::sidebar`] (`"expanded"` / `"rail"` in `ui.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarMode {
    /// [`size::sidebar()`] wide: icons, names, counters, group titles (design skill, section 0).
    #[default]
    Expanded,
    /// [`rail()`] wide: only the icons, names and counters in tooltips, group titles as lines. Apps
    /// leave out what has no icon (the small month, the version line).
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

/// The width of the content beside the sidebar for a window `width` wide in `class`, given the user's
/// `choice` ([`sidebar_mode`]): the window less the sidebar or the rail and the inset margin, the whole
/// window where no sidebar stands beside the content. Views that switch their own columns compare this
/// width, not the window's, so a column does not change while the sidebar takes or gives back room.
pub fn content_width(class: SizeClass, choice: SidebarMode, width: f32) -> f32 {
    let side = match sidebar_mode(class, choice) {
        Some(SidebarMode::Expanded) => size::sidebar(),
        Some(SidebarMode::Rail) => rail(),
        None => return width,
    };
    (width - side - layout::inset()).max(0.0)
}

/// What the handle of the expanded sidebar ([`Shell::sidebar_width`], issue #376) moves in, for a
/// window `window` wide whose content must keep `rest` beside the sidebar: [`size::sidebar()`] by
/// default, at least three quarters and at most twice that, and never so wide that the content gets
/// less than `rest`.
pub fn sidebar_limits(window: f32, rest: f32) -> split::Limits {
    split::limits_around(size::sidebar(), window - layout::inset(), rest)
}

/// Whether a key press is the sidebar shortcut `[` (as in Linear): the app then flips its choice
/// (wide) or opens or closes the drawer (medium, narrow) – the same as the panel button. Ctrl and
/// Alt are allowed, because some layouts type `[` with AltGr (German: AltGr+8, which Windows
/// reports as Ctrl+Alt); the logo key is not. Check it before an app drops keys with modifiers,
/// and only for keys no widget captured (a text field types the bracket).
pub fn is_sidebar_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    !modifiers.logo() && matches!(key.as_ref(), keyboard::Key::Character("["))
}

/// Whether a key press is the shortcut `]` of the mail's action rail (task E14.1,
/// [`widgets::action_rail`]): the app then expands or collapses the rail – the same as the button
/// `panel-right` at its top –, mirroring `[` for the sidebar ([`is_sidebar_key`]). Ctrl and Alt are
/// allowed, because some layouts type `]` with AltGr or Option (German: AltGr+9, which Windows
/// reports as Ctrl+Alt; macOS: Option+6); the logo key is not. Check it, like the sidebar key,
/// before an app drops keys with modifiers, and only for keys no widget captured.
pub fn is_actions_key(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> bool {
    !modifiers.logo() && matches!(key.as_ref(), keyboard::Key::Character("]"))
}

/// What the divider before the properties column ([`Shell::props_width`], issue #604) moves in, for
/// content `total` wide (body, divider and column): [`size::props()`] by default, three quarters to
/// twice that ([`split::limits_around`]), and the body keeps at least [`split::min_pane()`].
pub fn props_limits(total: f32) -> split::Limits {
    split::limits_around(size::props(), total, split::min_pane())
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
    side: Option<SideWidth<M>>,
    props_side: Option<PropsWidth<M>>,
    top: Option<TopRow<M>>,
}

/// The width of the properties column the person drags ([`Shell::props_width`]).
struct PropsWidth<M> {
    width: f32,
    total: f32,
    dragging: bool,
    on: fn(SplitEvent) -> M,
}

/// The width of the expanded sidebar the person drags ([`Shell::sidebar_width`]).
struct SideWidth<M> {
    width: f32,
    dragging: bool,
    on: fn(SplitEvent) -> M,
}

/// The window of an app in the layout for `class` ([`crate::layout::of`] of the window size):
///
/// - **Wide:** `sidebar` ([`size::sidebar()`], window background, 8 inside), then the content – flush
///   with a line beside the sidebar when [`layout::inset()`] is 0, else as an inset panel (that
///   margin top/right/bottom, radius `md`, 1-px line): `header` in a [`size::header()`] high bar with
///   a line below, then `body`, and `props` in a [`size::props()`] wide column at the right (scrolls
///   on its own).
/// - **Medium, narrow (and mini, strip):** `header` and `body` flush with the window; `sidebar`
///   only appears as a drawer ([`Shell::drawer`]) and `props` is not shown. With a rail
///   ([`Shell::rail`]; wide and medium) the content stands beside the rail as beside the sidebar.
///
/// The header bar adds the padding (16 left, 12 right; 8 and 8 in narrow), so `header` is just the
/// row from [`header_bar`]. `body` gets the free space and scrolls itself. Build `props` from
/// [`widgets::prop_section`]s stacked with `space::l()`.
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
        side: None,
        props_side: None,
        top: None,
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
    /// button of the top row ([`TopRow::sidebar`]). No effect in the wide layout, where the sidebar (or the
    /// rail) is always there.
    pub fn drawer(mut self, open: bool, on_dismiss: M) -> Self {
        self.drawer = open.then_some(on_dismiss);
        self
    }

    /// Draws the expanded sidebar beside the content `width` wide instead of [`size::sidebar()`], with
    /// a handle over its right edge that the person drags (issue #376): pressed it sends
    /// [`SplitEvent::Grab`], double-clicked [`SplitEvent::Reset`], and while `dragging` every move over
    /// the window sends [`SplitEvent::Drag`] with the width that puts the handle under the pointer, and
    /// the line beside the sidebar shows in `line_selected`. The app keeps a [`split::Split`] and
    /// clamps it to [`sidebar_limits`], as for [`split::split`]. Only the wide layout without the rail
    /// uses the width; the rail, the drawer and the band's [`Cover::Drawer`] keep their widths. The
    /// window has the same widgets with and without a drag, and beside the rail as well, so neither a
    /// drag nor `[` loses the content's state (scroll positions, focus). [`content_width`] still counts
    /// [`size::sidebar()`]: an app that drags the width measures its content with that width itself.
    pub fn sidebar_width(mut self, width: f32, dragging: bool, on: fn(SplitEvent) -> M) -> Self {
        self.side = Some(SideWidth { width, dragging, on });
        self
    }

    /// Draws the properties column `width` wide instead of [`size::props()`], with the divider of
    /// [`split::split_end`] between the body and it (issue #604): `total` is the width of body, divider
    /// and column together – the content beside the sidebar –, as in the [`props_limits`] the app
    /// clamps its [`split::Split`] to. Only where the properties show (the wide layout); without it the
    /// column is fixed as before.
    pub fn props_width(mut self, width: f32, total: f32, dragging: bool, on: fn(SplitEvent) -> M) -> Self {
        self.props_side = Some(PropsWidth { width, total, dragging, on });
        self
    }

    /// Draws the window's top row (issue #459, [`TopRow`]) instead of the header bar and the band of
    /// [`titlebar`]: `header` moves up into it, beside the panel button, with the settings at its right
    /// end. The app then shows the window without [`titlebar`] around it.
    pub fn top_row(mut self, top: TopRow<M>) -> Self {
        self.top = Some(top);
        self
    }
}

impl<'a, M: Clone + 'a> From<Shell<'a, M>> for Element<'a, M> {
    fn from(shell: Shell<'a, M>) -> Self {
        let Shell { class, sidebar, header, body, props, rail, drawer: on_dismiss, overlay, side, props_side, top } =
            shell;
        let props = Props { column: props, side: props_side };
        // With the top row the header lies in it, and the content starts with the body.
        let (header, top) = match top {
            Some(top) => (None, Some((top, header))),
            None => (Some(header_box(class, header)), None),
        };
        let mut layers = Vec::new();
        let rail = rail.filter(|_| matches!(class, SizeClass::Wide | SizeClass::Medium));
        // The top row's part over the sidebar: as wide as the expanded sidebar beside the content.
        let lead = (class == SizeClass::Wide && rail.is_none())
            .then(|| side.as_ref().map_or(size::sidebar(), |side| side.width));
        if class == SizeClass::Wide {
            match rail {
                Some(rail) => {
                    layers.push(resizable(inset_window(rail, self::rail(), header, body, props, false), side, false));
                }
                None => {
                    let width = side.as_ref().map_or(size::sidebar(), |side| side.width);
                    let dragging = side.as_ref().is_some_and(|side| side.dragging);
                    layers.push(resizable(inset_window(sidebar, width, header, body, props, dragging), side, true));
                }
            }
        } else if let Some(rail) = rail {
            layers.push(resizable(inset_window(rail, self::rail(), header, body, props, false), side, false));
            if let Some(on_dismiss) = on_dismiss {
                layers.push(drawer(sidebar, on_dismiss));
            }
        } else {
            let window = container(column(header).push(container(body).width(Fill).height(Fill)))
                .width(Fill)
                .height(Fill)
                .style(style::container::panel_flush);
            layers.push(window.into());
            if let Some(on_dismiss) = on_dismiss {
                layers.push(drawer(sidebar, on_dismiss));
            }
        }
        layers.extend(overlay);
        let window = if layers.len() == 1 { layers.remove(0) } else { stack(layers).into() };
        match top {
            Some((top, header)) => with_top_row(window, top, header, class, lead),
            None => window,
        }
    }
}

/// The height of the band above the content on macOS, in window points: the traffic lights sit in it.
#[cfg(target_os = "macos")]
const TITLEBAR: f32 = 32.0;

/// Where the traffic lights and the band's buttons sit, in points: their centre line, and the centre
/// of the close button from the left edge (U.11). Fixed, whatever the design: the line lies halfway
/// between the window's top edge and 8 below the band (32 + 8, where the inset panel began before issue
/// #291), and the close button is as far from the left edge as from the top – as in Claude. A design
/// that draws the content flush ([`layout::inset()`] 0) does not move them: the lights are placed
/// once at start ([`crate::place_traffic_lights`]), before any design is known, and stay in the band.
/// macOS itself would put them at 16 / 16.
#[cfg(target_os = "macos")]
pub(crate) const LIGHTS: f32 = 20.0;

/// The surface at the top of a window, right under the band of [`titlebar`]: the band takes its
/// colour, so the top of every window reads as one piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The window background `bg`: the states that lie on the window itself (signing in, loading
    /// or failing before a layout exists).
    Window,
    /// The frame `sidebar` (the window background unless the design gives the sidebar a colour of
    /// its own): where the sidebar or the rail runs up to the top (wide, medium).
    Sidebar,
    /// The content panel `panel`: the flush narrow window and the glance of mini and strip.
    Panel,
}

impl Surface {
    /// The surface at the top of the window [`shell`] lays out in `class`, or of the glance in mini
    /// and strip: the sidebar's frame where a sidebar or rail runs up to the top, else the panel.
    pub fn of(class: SizeClass) -> Surface {
        match sidebar_mode(class, SidebarMode::Expanded) {
            Some(_) => Surface::Sidebar,
            None => Surface::Panel,
        }
    }
}

/// What an open overlay lays over the top of the window, so the band of [`titlebar`] repeats it:
/// the scrim where the window is dimmed, the surface of a drawer or side panel where one runs up to
/// the top. Without it the band stays light over a dimmed window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cover {
    /// Nothing open: the band is the [`Surface`] alone.
    #[default]
    Nothing,
    /// A dialog ([`widgets::modal`]): the whole width dimmed.
    Dialog,
    /// The drawer ([`Shell::drawer`]): the sidebar's surface on its width at the left, dimmed beside it.
    Drawer,
    /// A side panel over the content ([`crate::panel::place`] in the medium layout): dimmed, and the
    /// panel's surface on its width at the right.
    Panel,
    /// An overlay that fills the window (the side panel in the narrow layout): the band is its
    /// surface, and nothing below it shows.
    Filled(Surface),
}

impl Cover {
    /// [`Cover::Dialog`] while a dialog is `open`.
    pub fn dialog(open: bool) -> Cover {
        if open { Cover::Dialog } else { Cover::Nothing }
    }

    /// [`Cover::Drawer`] while the drawer is `open` in `class`. [`Shell`] shows it only in the medium
    /// and narrow layouts; wide has the sidebar, and mini and strip show a glance without the shell.
    pub fn drawer(class: SizeClass, open: bool) -> Cover {
        let shown = open && matches!(class, SizeClass::Medium | SizeClass::Narrow);
        if shown { Cover::Drawer } else { Cover::Nothing }
    }

    /// The cover of a side panel `open` in `class`: [`Cover::Panel`] over a dimmed medium window,
    /// [`Cover::Filled`] where it fills the window (narrow, also over the drawer), nothing in wide,
    /// where it stands beside the content.
    pub fn panel(class: SizeClass, open: bool) -> Cover {
        match class {
            _ if !open => Cover::Nothing,
            SizeClass::Medium => Cover::Panel,
            SizeClass::Narrow => Cover::Filled(Surface::Panel),
            SizeClass::Wide | SizeClass::Mini | SizeClass::Strip => Cover::Nothing,
        }
    }

    /// This cover, or `below` when nothing is open here: apps name their overlays from the top down
    /// (`Cover::dialog(…).or(Cover::panel(…)).or(Cover::drawer(…))`), as they lie in the window.
    pub fn or(self, below: Cover) -> Cover {
        if self == Cover::Nothing { below } else { self }
    }
}

/// Whether the window has the band of [`titlebar`], and with it [`TitleButtons`] (only macOS). Where
/// it has, the panel button lives in the band, so apps leave it out of the narrow header
/// ([`sidebar_toggle`]); elsewhere it stays where it was.
pub fn has_band() -> bool {
    cfg!(target_os = "macos")
}

/// The window-wide buttons in the band of [`titlebar`] (U.10, as Claude and Slack have them): the
/// panel button right of the traffic lights. Only macOS draws it ([`has_band`]); a window without it
/// (a compose window, a glance) passes [`TitleButtons::none`]. The settings button lives only in the
/// top row of a window with settings ([`TopRow::settings`], issue #530): the windows that keep the band
/// (signing in, loading, a glance, a compose window, the template dialog) have no settings of their own.
///
/// ```ignore
/// let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
/// let buttons = TitleButtons::none().sidebar(label, Message::ToggleSidebar);
/// shell::titlebar(content, zoom, surface, cover, buttons, Message::DragWindow)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TitleButtons<M> {
    sidebar: Option<(&'static str, M)>,
}

impl<M> TitleButtons<M> {
    /// No buttons: the band holds the traffic lights alone.
    pub fn none() -> Self {
        TitleButtons { sidebar: None }
    }

    /// The panel button, which sends `on_press` – the same as `[` ([`is_sidebar_key`]): the app flips
    /// its choice in the wide layout and opens or closes the drawer elsewhere. `label` is its tooltip,
    /// from [`sidebar_button_label`].
    pub fn sidebar(mut self, label: &'static str, on_press: M) -> Self {
        self.sidebar = Some((label, on_press));
        self
    }
}

/// The tooltip of the panel button in a window of `class`, with the user's `choice` for the wide
/// layout and the drawer `open` or not: what a press does.
pub fn sidebar_button_label(class: SizeClass, choice: SidebarMode, open: bool) -> &'static str {
    match sidebar_mode(class, choice) {
        Some(SidebarMode::Expanded) => tr("Collapse sidebar"),
        Some(SidebarMode::Rail) if class == SizeClass::Wide => tr("Expand sidebar"),
        _ if open => tr("Hide sidebar"),
        _ => tr("Show sidebar"),
    }
}

/// Puts the band of the window's own title bar above `content` on macOS, where the window has none
/// (see [`crate::window`]): the traffic lights sit in it, so nothing of the content lies under them.
/// The band takes the colour of `surface`, the top of `content`, with what `cover` lays over it,
/// and is pressed to drag the window (also while the window is dimmed); `on_drag` goes to the app's
/// `update`, which returns [`crate::drag_window`]. `buttons` lie in the band: the panel button right
/// of the traffic lights. Over a dialog or a side panel it does not show (the window below is dimmed),
/// over the drawer it does, and closes it.
/// `zoom` is the factor of the app's `scale_factor`, so the band and its buttons stay the same size
/// in points at any zoom, as the traffic lights do. Other platforms keep their own title bar and get
/// `content` back unchanged.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn titlebar<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    zoom: f32,
    surface: Surface,
    cover: Cover,
    buttons: TitleButtons<M>,
    on_drag: M,
) -> Element<'a, M> {
    #[cfg(target_os = "macos")]
    {
        // The band is in layout units, which `scale_factor` multiplies; dividing keeps it in points.
        let on_sidebar = matches!((surface, cover), (Surface::Sidebar, Cover::Nothing) | (_, Cover::Drawer));
        let buttons = band_buttons(buttons, cover, zoom);
        let buttons = if on_sidebar { in_sidebar(buttons) } else { buttons };
        let band = stack![band(surface, cover), buttons];
        let band = container(band).width(Fill).height(TITLEBAR / zoom);
        let below = column![blank().height(TITLEBAR / zoom), container(content).width(Fill).height(Fill)];
        // The band lies over the content, not merely above it: the shadow of a side panel or the
        // drawer blurs up past their top edge, and drawn first the band showed it as a grey strip.
        // The buttons capture their presses, so only the free band drags the window.
        stack![below, mouse_area(band).on_press(on_drag)].into()
    }
    #[cfg(not(target_os = "macos"))]
    {
        content.into()
    }
}

/// The pitch of the traffic lights on macOS 26 (measured): their centres lie 23 apart.
#[cfg(target_os = "macos")]
const LIGHTS_PITCH: f32 = 23.0;
/// Where the panel button starts, in points from the left edge: its icon's centre lies a pitch and a
/// half after the last light's, so it reads as a group of its own beside them, as in Claude.
#[cfg(target_os = "macos")]
const BAND_LEADING: f32 = LIGHTS + 3.5 * LIGHTS_PITCH - BAND_BUTTON / 2.0;
/// Edge of a button in the band, in points.
#[cfg(target_os = "macos")]
const BAND_BUTTON: f32 = 24.0;

/// The row of [`TitleButtons`] over the band, as far as `cover` leaves them; all sizes in points,
/// divided by `zoom` into layout units.
#[cfg(target_os = "macos")]
fn band_buttons<'a, M: Clone + 'a>(buttons: TitleButtons<M>, cover: Cover, zoom: f32) -> Element<'a, M> {
    let TitleButtons { sidebar } = buttons;
    let sidebar = match cover {
        Cover::Nothing | Cover::Drawer => sidebar,
        Cover::Dialog | Cover::Panel | Cover::Filled(_) => None,
    };
    let mut buttons = row![blank().width(BAND_LEADING / zoom)];
    if let Some((label, on_press)) = sidebar {
        let mark = widgets::toned(Icon::PanelLeft, icon::SIZE / zoom, Tone::Text2);
        let edge = BAND_BUTTON / zoom;
        let pressable = button(container(mark).center(Fill)).width(edge).height(edge).padding(0);
        buttons = buttons.push(widgets::with_tooltip(pressable.style(style::button::icon).on_press(on_press), label));
    }
    // Centred on the line of the traffic lights, not in the band: the line sits lower (see `LIGHTS`).
    let row = buttons.push(blank::horizontal()).height(BAND_BUTTON / zoom).align_y(Alignment::Center);
    container(row).padding(Padding::new(0.0).top((LIGHTS - BAND_BUTTON / 2.0) / zoom)).into()
}

/// The band of [`titlebar`]: `surface`, and over it the parts of `cover`, with the widths the drawer
/// and the side panel have below it (both in layout units, as the band is).
#[cfg(target_os = "macos")]
fn band<'a, M: 'a>(surface: Surface, cover: Cover) -> Element<'a, M> {
    let style_of = |surface| -> fn(&iced::Theme) -> container::Style {
        match surface {
            Surface::Window => style::container::window,
            Surface::Sidebar => style::container::sidebar,
            Surface::Panel => style::container::panel_flush,
        }
    };
    let base = style_of(surface);
    let plain = |style: fn(&iced::Theme) -> container::Style| container(blank()).width(Fill).height(Fill).style(style);
    let dimmed = move || container(plain(scrim)).width(Fill).height(Fill).style(base);
    match cover {
        Cover::Nothing => plain(base).into(),
        Cover::Filled(top) => plain(style_of(top)).into(),
        Cover::Dialog => dimmed().into(),
        Cover::Drawer => {
            // As the drawer draws it: its line on its own surface (see `drawer`).
            let side = container(blank()).width(size::sidebar() - LINE).height(Fill);
            let drawer = container(row![side, rule::vertical(LINE).style(style::rule)])
                .height(Fill)
                .style(style::container::sidebar);
            row![in_sidebar(drawer), dimmed()].into()
        }
        Cover::Panel => {
            let panel = container(blank()).width(size::panel()).height(Fill).style(style::container::panel_flush);
            row![dimmed(), panel].into()
        }
    }
}

/// The scrim that dims the window behind a dialog, the drawer or a side panel over the content.
fn scrim(theme: &iced::Theme) -> container::Style {
    container::Style { background: Some(Background::Color(palette(theme).scrim)), ..container::Style::default() }
}

/// The room `content` gets under the band of [`titlebar`], for a window of `size` in layout units
/// at the zoom factor `zoom`: views that count what fits (the glances, a dialog sized to the window)
/// measure with it, so nothing is cut off at the bottom. Other platforms have no band and get `size`
/// back unchanged.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn below_titlebar(size: Size, zoom: f32) -> Size {
    #[cfg(target_os = "macos")]
    {
        // As in `titlebar`: the band is TITLEBAR points, which is TITLEBAR / zoom in layout units.
        Size::new(size.width, (size.height - TITLEBAR / zoom).max(0.0))
    }
    #[cfg(not(target_os = "macos"))]
    {
        size
    }
}

/// The height of the top row ([`TopRow`], issue #459), in points: twice the height of the traffic
/// lights' centre line ([`LIGHTS`] on macOS), so the lights sit in its middle. The same on every
/// system, so the windows of the suite look alike.
const TOP_ROW: f32 = 40.0;

/// Where the top row's own pieces start on macOS, in points from the left edge: the traffic lights
/// end a light's radius after the third centre ([`LIGHTS`] + 2 × [`LIGHTS_PITCH`]), and half a pitch
/// more keeps the panel button apart from them.
#[cfg(target_os = "macos")]
const LIGHTS_ROOM: f32 = LIGHTS + 2.5 * LIGHTS_PITCH;

/// The icon of the settings everywhere in the suite (issue #459): sliders, not the gear – they lead
/// to how the apps look and behave. Menus and rows that open the settings use it too.
pub const SETTINGS_ICON: Icon = Icon::SlidersHorizontal;

/// The window's top row (issue #459), drawn by [`Shell::top_row`] in place of the header bar and of
/// the band of [`titlebar`], on every system. One row of [`TOP_ROW`] points, as in the mail window's
/// draft (`docs/prototypes/mail-layout/`):
///
/// ```text
/// Wide    ● ● ●  [▯]                    │ the app's header …                      [☰]
///         └──────── sidebar ───────────┘└──────────── content ───────────────────────┘
/// Rail    ● ● ●  [▯]  the app's header …                                           [☰]
/// Narrow  ● ● ●  [▯]  the app's header …                                           [☰]
/// ```
///
/// - Over the expanded sidebar (wide): room for the traffic lights (macOS only) and right after them
///   the panel button, on the sidebar's surface, as wide as the sidebar; then the line beside it.
/// - With the rail (wide by choice, medium) and in narrow windows the same lights' room and panel
///   button stand before the header; narrow, the button opens the drawer. There is no app switcher
///   (issue #530): the panel button stands at the same place in every form.
/// - Then the app's header ([`header_bar`]), padded as in the header bar, with a line below, and the
///   settings ([`SETTINGS_ICON`]) at the right end, marked while they are open.
/// - Mail opts into [`TopRow::sidebar_surface`] for one sidebar-coloured head and
///   [`TopRow::sidebar_at_edge`] for the expanded sidebar's button at its right edge (issue #564),
///   and into [`TopRow::keep_buttons`] for the panel button and the settings over an open mail (#545).
///
/// The free row drags the window. What an overlay lays over the top of the window ([`Cover`]) dims
/// or covers the row as it did the band. The sizes of the lights' room are points (divided by
/// `zoom`), the rest scales with the window.
///
/// ```ignore
/// let top = TopRow::new(self.zoom.factor(), Message::DragWindow)
///     .sidebar(shell::sidebar_button_label(class, self.sidebar, self.drawer), Message::ToggleSidebar)
///     .settings(Message::ToggleSettings)
///     .settings_open(self.settings.is_some())
///     .cover(self.cover());
/// shell::shell(class, sidebar, header, body, None).rail(rail).top_row(top).into()
/// ```
#[derive(Debug, Clone)]
pub struct TopRow<M> {
    sidebar: Option<(&'static str, M)>,
    sidebar_surface: bool,
    sidebar_at_edge: bool,
    keep_buttons: bool,
    settings: Option<M>,
    settings_open: bool,
    zoom: f32,
    cover: Cover,
    panel: Option<f32>,
    on_drag: M,
}

impl<M> TopRow<M> {
    /// The top row of a window: a press on the free row sends `on_drag` (the app returns
    /// [`crate::drag_window`]). `zoom` is the factor of the app's `scale_factor`.
    pub fn new(zoom: f32, on_drag: M) -> Self {
        TopRow {
            sidebar: None,
            sidebar_surface: false,
            sidebar_at_edge: false,
            keep_buttons: false,
            settings: None,
            settings_open: false,
            zoom,
            cover: Cover::Nothing,
            panel: None,
            on_drag,
        }
    }

    /// How wide the side panel over the content is ([`Cover::Panel`]) where it is not
    /// [`size::panel()`]: a panel that grew by a side column or a floating rail (issue #456).
    pub fn panel_width(mut self, width: f32) -> Self {
        self.panel = Some(width);
        self
    }

    /// The panel button, which sends `on_press` – the same as `[` ([`is_sidebar_key`]); `label` is its
    /// tooltip, from [`sidebar_button_label`].
    pub fn sidebar(mut self, label: &'static str, on_press: M) -> Self {
        self.sidebar = Some((label, on_press));
        self
    }

    /// Draws the entire normal head in the sidebar's palette (mail, issue #564), including its
    /// text and controls. Menus and tooltips keep the window's theme. Off for existing callers.
    pub fn sidebar_surface(mut self, enabled: bool) -> Self {
        self.sidebar_surface = enabled;
        self
    }

    /// Places the panel button at the expanded sidebar's right edge (mail, issue #564). The compact
    /// lead and the drawer keep their usual position beside the lights. Off for existing callers.
    pub fn sidebar_at_edge(mut self, enabled: bool) -> Self {
        self.sidebar_at_edge = enabled;
        self
    }

    /// Keeps the panel button and the settings in reach over a side panel ([`Cover::Panel`],
    /// [`Cover::Filled`]), at the places they have without it (mail, issue #545): the app closes the
    /// panel when either is pressed. Off for existing callers, whose panel covers the whole row.
    pub fn keep_buttons(mut self, enabled: bool) -> Self {
        self.keep_buttons = enabled;
        self
    }

    /// The normal head's surface; opaque overlays keep their own surface instead.
    fn head_surface(&self) -> Surface {
        if self.sidebar_surface { Surface::Sidebar } else { Surface::Panel }
    }

    /// The settings button ("Settings", [`SETTINGS_ICON`]), which sends `on_press`. Every window with
    /// settings shows it, the same at the same place (issue #530); windows without settings leave it out.
    pub fn settings(mut self, on_press: M) -> Self {
        self.settings = Some(on_press);
        self
    }

    /// Marks the settings button while the settings fill the content area, and the button leads back
    /// out of them (task S.16, `docs/settings.md`): `selected` under the icon and the icon in `text`,
    /// like a chosen sidebar row. Neutral, never red.
    pub fn settings_open(mut self, open: bool) -> Self {
        self.settings_open = open;
        self
    }

    /// What an overlay lays over the top of the window ([`Cover`]), which the row repeats.
    pub fn cover(mut self, cover: Cover) -> Self {
        self.cover = cover;
        self
    }
}

/// How far the header in the top row starts right of the content's left edge, in layout units, for a
/// window of `class` with the user's `choice` at the zoom factor `zoom`: 0 beside the expanded sidebar,
/// where the row's lead is as wide as the sidebar; else what the lights' room and the panel button take
/// beyond the rail (or the whole of it in narrow windows). Apps that split their header
/// by the columns below (the list's head over the list) take it off the first part.
pub fn head_inset(class: SizeClass, choice: SidebarMode, zoom: f32) -> f32 {
    match sidebar_mode(class, choice) {
        Some(SidebarMode::Expanded) => 0.0,
        Some(SidebarMode::Rail) => (compact_lead_width(zoom) - rail() - LINE).max(0.0),
        None => compact_lead_width(zoom),
    }
}

/// The room the lights take at the left of the top row, in layout units: [`LIGHTS_ROOM`] points on
/// macOS, the gap of the row's edge elsewhere.
fn lights_room(zoom: f32) -> f32 {
    #[cfg(target_os = "macos")]
    {
        LIGHTS_ROOM / zoom
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = zoom;
        space::s()
    }
}

/// The width of the top row's lead where it does not stand over the expanded sidebar: the lights'
/// room and the panel button, [`SIDEBAR_GAP`] apart, and the gap before the header.
fn compact_lead_width(zoom: f32) -> f32 {
    // The pieces of `compact_lead` and the gap of its row between each two of them.
    lights_room(zoom) + SIDEBAR_GAP + size::control_sm() + SIDEBAR_GAP + space::s()
}

/// The window `window` under its top row ([`Shell::top_row`]): `header` in the row, the row as wide as
/// the window and [`TOP_ROW`] points high. `lead` is the width of the expanded sidebar beside the
/// content, `None` with the rail or without a sidebar.
fn with_top_row<'a, M: Clone + 'a>(
    window: Element<'a, M>,
    top: TopRow<M>,
    header: Element<'a, M>,
    class: SizeClass,
    lead: Option<f32>,
) -> Element<'a, M> {
    let height = TOP_ROW / top.zoom;
    let on_drag = top.on_drag.clone();
    let below = column![blank().height(height), container(window).width(Fill).height(Fill)];
    let band = container(top_row_band(top, header, class, lead)).width(Fill).height(height);
    // As the band of `titlebar`: over the window, so the shadow of a drawer or a side panel, which
    // blurs up past their top edge, does not show as a strip. The buttons capture their presses, so
    // only the free row drags the window.
    stack![below, mouse_area(band).on_press(on_drag)].into()
}

/// Whether the top row lays its panel button and settings over `cover` (issue #545): only where the
/// caller `keep`s them ([`TopRow::keep_buttons`]) and a side panel covers the row. A dialog dims the
/// whole window and the drawer has its own lead, so both stay as they were.
fn keeps_buttons(cover: Cover, keep: bool) -> bool {
    keep && matches!(cover, Cover::Panel | Cover::Filled(_))
}

/// The pieces of the top row as `cover` leaves them (see [`TopRow`]), and over a side panel the
/// buttons the caller keeps ([`keeps_buttons`]).
fn top_row_band<'a, M: Clone + 'a>(
    top: TopRow<M>,
    header: Element<'a, M>,
    class: SizeClass,
    lead: Option<f32>,
) -> Element<'a, M> {
    if !keeps_buttons(top.cover, top.keep_buttons) {
        return covered_row(top, header, class, lead);
    }
    let buttons = kept_buttons(&top);
    stack![covered_row(top, header, class, lead), buttons].into()
}

/// The panel button at the left and the settings at the right, where the row without a cover has
/// them; the room between takes no press, so the window still drags there. The lead lies on the
/// dimmed head, in the sidebar's palette where the head has it; the settings lie on the panel.
fn kept_buttons<'a, M: Clone + 'a>(top: &TopRow<M>) -> Element<'a, M> {
    let lead = container(compact_lead(top)).height(Fill).center_y(Fill);
    let lead = if top.sidebar_surface && top.cover == Cover::Panel { in_sidebar(lead) } else { lead.into() };
    let mut pieces = row![lead, blank::horizontal()];
    if let Some(on_press) = top.settings.clone() {
        pieces = pieces.push(settings_button(on_press, top.settings_open)).push(blank().width(settings_gap(top.zoom)));
    }
    // Over the line the uncovered row draws below its buttons, so they stay at the same height.
    column![pieces.height(Fill).align_y(Alignment::Center), blank().height(LINE)].into()
}

/// The pieces of the top row as `cover` leaves them, without the kept buttons.
fn covered_row<'a, M: Clone + 'a>(
    top: TopRow<M>,
    header: Element<'a, M>,
    class: SizeClass,
    lead: Option<f32>,
) -> Element<'a, M> {
    let cover = top.cover;
    let on_drag = top.on_drag.clone();
    // A dimmed part keeps the window dragging, and nothing under it takes a press.
    let dimmer = move || -> Element<'a, M> {
        opaque(mouse_area(container(blank()).width(Fill).height(Fill).style(scrim)).on_press(on_drag.clone()))
    };
    match cover {
        Cover::Filled(surface) => container(blank()).width(Fill).height(Fill).style(surface_style(surface)).into(),
        Cover::Drawer => {
            // The drawer's own top: the panel button over its width, the rest dimmed.
            let width = size::sidebar();
            let side = expanded_lead(&top, width);
            row![side, stack![head_part(&top, header, class), dimmer()]].into()
        }
        Cover::Dialog => stack![plain_row(&top, header, class, lead), dimmer()].into(),
        Cover::Panel => {
            let width = top.panel.unwrap_or_else(size::panel);
            let panel = container(blank()).width(width).height(Fill).style(style::container::panel_flush);
            stack![plain_row(&top, header, class, lead), row![dimmer(), opaque(panel)]].into()
        }
        Cover::Nothing => plain_row(&top, header, class, lead),
    }
}

/// The top row with nothing over it: the lead over the sidebar (`lead`) or the compact one, then the
/// header and the settings.
fn plain_row<'a, M: Clone + 'a>(
    top: &TopRow<M>,
    header: Element<'a, M>,
    class: SizeClass,
    lead: Option<f32>,
) -> Element<'a, M> {
    match lead {
        Some(width) => row![expanded_lead(top, width), head_part(top, header, class)].into(),
        None => {
            let lead = compact_lead(top);
            // The compact lead lies on the panel, as the header does, and the line runs under both.
            let row = row![container(lead).height(Fill).center_y(Fill), head_row(top, header_padded(header, class))];
            let part = column![row.height(Fill), rule::horizontal(LINE).style(style::rule)];
            head_background(top, part.into())
        }
    }
}

/// The lead over the expanded sidebar, `width` wide. Mail may put the button at its right edge;
/// the drawer always keeps the compact lead beside the lights.
fn expanded_lead<'a, M: Clone + 'a>(top: &TopRow<M>, width: f32) -> Element<'a, M> {
    let at_edge = top.sidebar_at_edge && top.cover != Cover::Drawer;
    let lead = if at_edge {
        let mut pieces = row![blank().width(lights_room(top.zoom)), blank::horizontal()];
        if let Some((label, on_press)) = top.sidebar.clone() {
            pieces = pieces.push(widgets::icon_button(Icon::PanelLeft, label, Some(on_press)));
        }
        pieces.push(blank().width(space::s())).align_y(Alignment::Center).into()
    } else {
        compact_lead(top)
    };
    // The body's expanded sidebar is `width` plus its separator. The drawer instead includes
    // its line in `size::sidebar()`. Keep existing callers' geometry, opt mail into the exact axis.
    let side = container(lead)
        .width((if at_edge { width } else { width - LINE }).max(0.0))
        .height(Fill)
        .center_y(Fill)
        .style(style::container::sidebar);
    row![in_sidebar(side), rule::vertical(LINE).style(style::rule)].into()
}

/// The lead's pieces: the lights' room and the panel button, then the gap before the header. The same
/// over the expanded sidebar by default, beside the rail and in narrow windows.
fn compact_lead<'a, M: Clone + 'a>(top: &TopRow<M>) -> Element<'a, M> {
    let mut pieces = row![blank().width(lights_room(top.zoom))];
    if let Some((label, on_press)) = top.sidebar.clone() {
        pieces = pieces.push(widgets::icon_button(Icon::PanelLeft, label, Some(on_press)));
    }
    pieces.push(blank().width(space::s())).spacing(SIDEBAR_GAP).align_y(Alignment::Center).into()
}

/// The header part of the row: the header padded as in the header bar, the settings, the line below.
fn head_part<'a, M: Clone + 'a>(top: &TopRow<M>, header: Element<'a, M>, class: SizeClass) -> Element<'a, M> {
    let row = head_row(top, header_padded(header, class));
    let part = column![row.height(Fill), rule::horizontal(LINE).style(style::rule)];
    head_background(top, part.into())
}

/// Replace the inner panel too: merely colouring an outer row leaves a strip beside the search.
fn head_background<'a, M: 'a>(top: &TopRow<M>, part: Element<'a, M>) -> Element<'a, M> {
    let area = container(part).width(Fill).height(Fill).style(surface_style(top.head_surface()));
    if top.sidebar_surface { in_sidebar(area) } else { area.into() }
}

/// `header` with the padding of the header bar in `class`, centred in the row.
fn header_padded<'a, M: 'a>(header: Element<'a, M>, class: SizeClass) -> Element<'a, M> {
    let (left, right) = match class {
        SizeClass::Wide | SizeClass::Medium => (space::l(), space::m()),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => (space::s(), space::s()),
    };
    container(header).width(Fill).height(Fill).center_y(Fill).padding(Padding::new(0.0).left(left).right(right)).into()
}

/// `header`, then the settings button and the gap to the window's edge ([`settings_gap`]).
fn head_row<'a, M: Clone + 'a>(top: &TopRow<M>, header: Element<'a, M>) -> iced::widget::Row<'a, M> {
    let mut row = row![header];
    if let Some(on_press) = top.settings.clone() {
        row = row.push(settings_button(on_press, top.settings_open)).push(blank().width(settings_gap(top.zoom)));
    }
    row.align_y(Alignment::Center)
}

/// The edge of the settings button in the top row (issue #474): one [`space::s()`] smaller than the
/// other icon buttons ([`size::control_sm()`]; 36 − 8 = 28 in the standard design), so it keeps room
/// to the window's corner, whose rounding on macOS would cut a button that stands closer, and to the
/// line below. It follows the design's tokens, as the other buttons do. Decided after the visual check
/// of #472 and its review.
fn settings_edge() -> f32 {
    (size::control_sm() - space::s()).max(0.0)
}

/// The gap right of the settings button, in layout units, at the zoom factor `zoom` (issue #474): as
/// wide as the room above and below the button ([`settings_edge`]), which sits in the middle of the
/// row over the line, so it stands the same distance from the top, the right edge and the line (5.5
/// at zoom 1). The gap shrinks to nothing rather than below.
fn settings_gap(zoom: f32) -> f32 {
    ((TOP_ROW / zoom - LINE - settings_edge()) / 2.0).max(0.0)
}

/// The settings button of the top row: [`SETTINGS_ICON`], "Settings", marked while `open` like a
/// chosen sidebar row (neutral, never red).
fn settings_button<'a, M: Clone + 'a>(on_press: M, open: bool) -> Element<'a, M> {
    // Closed it looks like an icon button (`text_2`, `hover` on hover), only [`settings_edge`] square.
    let tone = if open { Tone::Text } else { Tone::Text2 };
    let pressable = button(container(widgets::toned(SETTINGS_ICON, icon::SIZE, tone)).center(Fill))
        .width(settings_edge())
        .height(settings_edge())
        .padding(0)
        .on_press(on_press);
    let pressable = match open {
        true => pressable.style(style::button::sidebar_item(true)),
        false => pressable.style(style::button::icon),
    };
    widgets::with_tooltip(pressable, tr("Settings"))
}

/// The container style of `surface`.
fn surface_style(surface: Surface) -> fn(&iced::Theme) -> container::Style {
    match surface {
        Surface::Window => style::container::window,
        Surface::Sidebar => style::container::sidebar,
        Surface::Panel => style::container::panel_flush,
    }
}

/// The first row of a sidebar under the top row (issue #459): search (only while `search`, task E11.2:
/// a window whose search field always stands over its list – the inbox's mail window – leaves the
/// magnifier out) and "New". Expanded they stand at the row's end, in the rail below each other. `on`
/// wraps the [`Switcher`] messages.
pub fn sidebar_tools<'a, M: Clone + 'a>(mode: SidebarMode, search: bool, on: fn(Switcher) -> M) -> Element<'a, M> {
    let tools = head_tools(search).into_iter().map(|tool| tool.button(on));
    match mode {
        SidebarMode::Expanded => row![blank::horizontal()]
            .extend(tools)
            .spacing(SIDEBAR_GAP)
            .height(tools_height())
            .align_y(Alignment::Center)
            .into(),
        SidebarMode::Rail => column(tools).spacing(SIDEBAR_GAP).width(Fill).align_x(Alignment::Center).into(),
    }
}

/// The room the window's content gets under the top row ([`Shell::top_row`]), for a window of `size`
/// in layout units at the zoom factor `zoom`: dialogs sized to the window measure with it.
pub fn below_top_row(size: Size, zoom: f32) -> Size {
    Size::new(size.width, (size.height - TOP_ROW / zoom).max(0.0))
}

/// The header bar: [`size::header()`] high (64 in the standard), the padding of the size class, a line below.
fn header_box<'a, M: 'a>(class: SizeClass, header: Element<'a, M>) -> Element<'a, M> {
    let (left, right) = match class {
        SizeClass::Wide | SizeClass::Medium => (space::l(), space::m()),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => (space::s(), space::s()),
    };
    let bar = container(header).width(Fill).center_y(size::header()).padding(Padding::new(0.0).left(left).right(right));
    column![bar, rule::horizontal(LINE).style(style::rule)].into()
}

/// The sidebar (`width`, [`size::sidebar()`] unless the person dragged it) or the rail ([`rail()`]) on
/// the window background, and the content beside it: flush with a line between them when the design's
/// [`layout::inset()`] is 0, else as an inset panel with that margin. While the sidebar's edge is
/// `dragged` ([`Shell::sidebar_width`]) the line shows in `line_selected`, as the divider of a split.
fn inset_window<'a, M: Clone + 'a>(
    sidebar: Element<'a, M>,
    width: f32,
    header: Option<Element<'a, M>>,
    body: Element<'a, M>,
    props: Props<'a, M>,
    dragged: bool,
) -> Element<'a, M> {
    let body = container(body).width(Fill).height(Fill);
    let main: Element<'a, M> = match props {
        // The body in the widgets of the divider without a column (a mouse area, a stack, a row), so it
        // keeps its state while the column comes and goes (the hub's "Coming back").
        Props { column: None, side: Some(_) } => {
            mouse_area(stack![row![container(body).width(Fill).height(Fill)]]).into()
        }
        Props { column: Some(props), side } => {
            let column = scrollable(container(props).width(Fill).padding(Padding {
                top: space::l(),
                right: space::s(),
                bottom: space::xl(),
                left: space::s(),
            }))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill);
            match side {
                Some(PropsWidth { width, total, dragging, on }) => {
                    split::split_end(body, column, width, total, dragging, on)
                }
                None => {
                    row![body, rule::vertical(LINE).style(style::rule), container(column).width(size::props() - LINE)]
                        .height(Fill)
                        .into()
                }
            }
        }
        Props { column: None, side: None } => row![body].height(Fill).into(),
    };
    let content = column(header).push(main);
    // Only the top and bottom edge: the sidebar sets its side edges itself ([`sidebar`]).
    let side = in_sidebar(container(sidebar).width(width).height(Fill).padding(Padding::new(0.0).vertical(space::s())));
    let inset = layout::inset();
    let window = if is_flush(inset) {
        // Flush (issue #291): the panel runs to the window's edges, a line parts it from the sidebar.
        let panel = container(content).width(Fill).height(Fill).style(style::container::panel_flush);
        let line = rule::vertical(LINE).style(move |t| {
            let mut look = style::rule(t);
            if dragged {
                look.color = palette(t).line_selected;
            }
            look
        });
        row![side, line, panel]
    } else {
        // The 1-px padding keeps the content inside the panel's border, as `border-box` in the CSS.
        let panel = container(content).width(Fill).height(Fill).padding(LINE).style(style::container::panel);
        row![side, container(panel).padding(Padding { left: 0.0, ..Padding::new(inset) })]
    };
    container(window).width(Fill).height(Fill).style(style::container::sidebar).into()
}

/// The properties column for [`inset_window`] (`None` while the view has none) and, when the app
/// drags it, its width.
struct Props<'a, M> {
    column: Option<Element<'a, M>>,
    side: Option<PropsWidth<M>>,
}

/// How far the handle of the expanded sidebar reaches over the sidebar ([`resizable`]): the margin
/// of [`style::scrollbar()`], so the sidebar's scroller, which floats in its right edge, stays free
/// to grab and to scroll over; the rest of the handle lies over the content's edge.
const GRIP_INSIDE: f32 = 2.0;

/// `window` with the handle of the expanded sidebar over it (issue #376), when the app gave a `side`
/// width ([`Shell::sidebar_width`]); `handle` is false beside the rail, which has none. The handle is
/// [`split::handle_width()`] wide, [`GRIP_INSIDE`] of it over the sidebar, and only it takes the
/// pointer: the rest of its layer is empty, so the window below gets every other press. Window and
/// layer always lie in one mouse area, with or without the handle and the drag, so the window keeps
/// its state.
fn resizable<'a, M: Clone + 'a>(window: Element<'a, M>, side: Option<SideWidth<M>>, handle: bool) -> Element<'a, M> {
    let Some(SideWidth { width, dragging, on }) = side else {
        return window;
    };
    let layer: Element<'a, M> = if handle {
        let grip = mouse_area(container(blank()).width(split::handle_width()).height(Fill))
            .on_press(on(SplitEvent::Grab))
            .on_double_click(on(SplitEvent::Reset))
            .interaction(mouse::Interaction::ResizingHorizontally);
        row![blank().width((width - GRIP_INSIDE).max(0.0)), grip].height(Fill).into()
    } else {
        blank().into()
    };
    let area = mouse_area(stack![window, layer]);
    if handle && dragging {
        // The window starts at the left edge, so the pointer's x is the sidebar's width once the
        // handle's middle, which lies right of the edge, is under the pointer.
        let middle = split::handle_width() / 2.0 - GRIP_INSIDE;
        area.on_move(move |point| on(SplitEvent::Drag(point.x - middle)))
            .interaction(mouse::Interaction::ResizingHorizontally)
            .into()
    } else {
        area.into()
    }
}

/// Whether a design with the inset `inset` ([`layout::inset()`]) draws the content beside the sidebar
/// flush, with a line between them, instead of as an inset panel: an inset of 0 (issue #291).
fn is_flush(inset: f32) -> bool {
    inset <= 0.0
}

/// The sidebar as a drawer over the left edge: the rest of the window is dimmed with the scrim (as
/// behind a dialog) and the drawer casts the menu shadow to the right; a click on the scrim
/// dismisses.
fn drawer<'a, M: Clone + 'a>(sidebar: Element<'a, M>, on_dismiss: M) -> Element<'a, M> {
    let catcher = opaque(mouse_area(container(blank().width(Fill).height(Fill)).style(scrim)).on_press(on_dismiss));
    // The edge is a rule on the drawer's own surface: Iced leaves a translucent border see-through,
    // and the content below would shine through it.
    let side =
        container(sidebar).width(size::sidebar() - LINE).height(Fill).padding(Padding::new(0.0).vertical(space::s()));
    let panel = container(row![side, rule::vertical(LINE).style(style::rule)]).height(Fill).style(|t| {
        let p = palette(t);
        // The menu shadow falls sideways here: the drawer's edge is vertical.
        let shadow = Shadow { offset: Vector::new(space::m(), 0.0), ..p.shadow };
        container::Style { background: Some(Background::Color(p.sidebar)), shadow, ..container::Style::default() }
    });
    stack![catcher, opaque(in_sidebar(panel))].into()
}

/// The content of the sidebar: the `switcher` row (since issue #530 the tools, [`sidebar_tools`]) with 8
/// below it, the `items` (navigation, [`widgets::sidebar_group`]s with their rows) from the top, and the
/// `footer` rows ("Open Calendar ↗", "Settings") at the bottom. Rows are 2 apart. The same for both
/// [`SidebarMode`]s: build the pieces in the mode you want.
///
/// Only the `items` scroll (task U.12): a design pack with taller rows or a low window has no room
/// for all of them, and the first row and the footer must stay in reach. The scrollbar floats in the
/// sidebar's right edge, beside the rows, so it never covers their counters.
pub fn sidebar<'a, M: 'a>(
    switcher: impl Into<Element<'a, M>>,
    items: Vec<Element<'a, M>>,
    footer: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    sidebar_parts(switcher.into(), items, footer)
}

/// [`sidebar`] for any renderer, so the tests can lay it out with Iced's null renderer.
fn sidebar_parts<'a, M: 'a, R: iced::advanced::text::Renderer + 'a>(
    switcher: Element<'a, M, iced::Theme, R>,
    items: Vec<Element<'a, M, iced::Theme, R>>,
    footer: Vec<Element<'a, M, iced::Theme, R>>,
) -> Element<'a, M, iced::Theme, R> {
    // The sidebar sets its side edges itself, the shell around it only the top and bottom: the
    // scrollbar then lies in the right edge instead of over the rows.
    let edge = Padding::new(0.0).horizontal(space::s());
    let middle = scrollable(container(column(items).spacing(SIDEBAR_GAP)).width(Fill).padding(edge))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(Fill)
        .height(Fill);
    column![
        container(switcher).width(Fill).padding(edge),
        // 8 below the first row and the row gap at both sides, as when the rows followed it directly.
        blank().height(space::s() + 2.0 * SIDEBAR_GAP),
        middle,
        // Scrolled rows end here, not flush against the footer.
        blank().height(space::s()),
        container(column(footer).spacing(SIDEBAR_GAP)).width(Fill).padding(edge),
    ]
    .height(Fill)
    .into()
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
            radius: radius::sm().into(),
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
        .spacing(space::s())
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
    content = content.push(typo::text(typo::body_strong(), title).wrapping(Wrapping::None));
    if let Some(note) = note {
        content = content.push(typo::text(typo::body(), note).style(theme::text_3).wrapping(Wrapping::None));
    }
    content.into()
}

/// The header button that opens the sidebar drawer in the medium and narrow layouts of a window without
/// the top row. Only where the window has no band ([`has_band`]); with one, the band's panel button does
/// it.
pub fn sidebar_toggle<'a, M: Clone + 'a>(on_press: M) -> Element<'a, M> {
    widgets::icon_button(Icon::PanelLeft, tr("Show sidebar"), Some(on_press))
}

/// What the tools at the top of the sidebar ([`sidebar_tools`]) ask the app to do; the app wraps it in
/// its own message (`Message::Switcher(Switcher)`). The name stays from the app switcher, which had
/// these tools beside it until issue #530.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switcher {
    /// The search button.
    Search,
    /// The "New" button (new task, new event …).
    New,
}

/// The short name of an app ("Tasks", "Calendar", "Workplace"); the inbox's three
/// windows by their kind ("Notification Hub", "Mail", "App Feed"), since they share one binary. The
/// three names share no word (issue #127), so nobody takes one window for another. Translated; the
/// accounts' "Used by …" names the apps with it too (issue #410).
pub fn app_name(app: AppId) -> &'static str {
    match app.window_class {
        "torro-inbox" => tr("Notification Hub"),
        "torro-inbox-mail" => tr("Mail"),
        "torro-inbox-notifications" => tr("App Feed"),
        "torro-tasks" => tr("Tasks"),
        "torro-calendar" => tr("Calendar"),
        "torro-assistant" => tr("Assistant"),
        _ => tr("Workplace"),
    }
}

/// The icon of an app, also on its mark ([`app_mark`]). Each of the inbox's
/// windows has its own: the hub the tray, mail the envelope, the app feed the bell. Public for the
/// control centre's app catalog (issue #411), which shows each app with the icon it has here.
pub fn app_icon(app: AppId) -> Icon {
    match app.window_class {
        "torro-inbox" => Icon::Inbox,
        "torro-inbox-mail" => Icon::Mail,
        "torro-inbox-notifications" => Icon::Bell,
        "torro-tasks" => Icon::ListTodo,
        "torro-calendar" => Icon::Calendar,
        "torro-assistant" => Icon::Sparkles,
        _ => Icon::LayoutGrid,
    }
}

/// The hue of a window that shares its program with others (issue #127): a slot of
/// [`theme::Palette::calendar`] (the hub orange, mail blue, the app feed pink), `None` for an app
/// with one window, which the suite's logo names well enough. Only ever as a tinted tile with the
/// icon in the full hue ([`app_mark`]), never as a large surface or as text.
fn app_hue(app: AppId) -> Option<usize> {
    match app.window_class {
        "torro-inbox" => Some(2),
        "torro-inbox-mail" => Some(0),
        "torro-inbox-notifications" => Some(3),
        _ => None,
    }
}

/// The mark of `app`, 20 × 20 (issue #127): for a window that
/// shares its program with others ([`app_hue`]: the inbox's hub, mail window and app feed) its icon
/// ([`app_icon`], 14) in its own hue on a tile tinted with it ([`theme::Palette::tinted`], radius
/// [`radius::xs()`]), so the windows differ at a glance; for every other app the suite's logo.
///
/// A window without sidebar or rail (narrow) puts it first in its header ([`header_bar`]'s
/// `leading`), so the window still says which one it is; the setup shows it as the suite's logo.
pub fn app_mark<'a, M: 'a>(app: AppId) -> Element<'a, M> {
    let Some(hue) = app_hue(app) else { return logo() };
    let glyph: Element<'a, M> = icon::icon(app_icon(app), icon::SIZE_SM)
        .style(move |t: &iced::Theme, _| iced::widget::svg::Style { color: palette(t).calendar.get(hue).copied() })
        .into();
    container(glyph)
        .center(LOGO)
        .style(move |t: &iced::Theme| {
            let p = palette(t);
            container::Style {
                background: p.calendar.get(hue).map(|&colour| Background::Color(p.tinted(colour))),
                border: iced::border::rounded(radius::xs()),
                ..container::Style::default()
            }
        })
        .into()
}

/// A tool in the first row of the sidebar ([`sidebar_tools`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadTool {
    /// The magnifier, "Search" ([`Switcher::Search`]).
    Search,
    /// The framed "New" ([`Switcher::New`]).
    New,
}

impl HeadTool {
    /// The tool's button, sending its [`Switcher`] message wrapped by `on`.
    fn button<'a, M: Clone + 'a>(self, on: fn(Switcher) -> M) -> Element<'a, M> {
        match self {
            HeadTool::Search => widgets::icon_button(Icon::Search, tr("Search"), Some(on(Switcher::Search))),
            HeadTool::New => widgets::boxed_button(Icon::SquarePen, tr("New"), Some(on(Switcher::New))),
        }
    }
}

/// The tools of the sidebar's first row, in their order: Search (only while `search`,
/// [`sidebar_tools`]), then New.
pub fn head_tools(search: bool) -> Vec<HeadTool> {
    [search.then_some(HeadTool::Search), Some(HeadTool::New)].into_iter().flatten().collect()
}

/// The app logo, 20 × 20: the active colour scheme's own mark (`logo.mark`, an SVG from its folder)
/// if it has one, otherwise the suite's: a white "T" on the accent colour, radius 4.
fn logo<'a, M: 'a>() -> Element<'a, M> {
    if let Some(mark) = scheme::active().logo.mark.clone() {
        return iced::widget::svg(iced::widget::svg::Handle::from_path(mark)).width(LOGO).height(LOGO).into();
    }
    container(
        typo::text(typo::caption(), "T").style(|t| iced::widget::text::Style { color: Some(palette(t).on_accent) }),
    )
    .center(LOGO)
    .style(|t| container::Style {
        background: Some(Background::Color(palette(t).accent)),
        border: iced::border::rounded(radius::xs()),
        ..container::Style::default()
    })
    .into()
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
    use crate::widgets::{Anchor, MenuItem, MenuSection, Trail};
    use torro_pim::AccountState;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Switcher(Switcher),
        Close,
    }

    /// Issue #564: mail opts in; every existing caller keeps the split surfaces and fixed button.
    #[test]
    fn the_mail_top_row_options_are_independent_and_off_by_default() {
        let top = TopRow::new(1.0, Msg::Close);
        assert!(!top.sidebar_surface && !top.sidebar_at_edge);
        let surface = top.clone().sidebar_surface(true);
        assert!(surface.sidebar_surface && !surface.sidebar_at_edge);
        let edge = top.sidebar_at_edge(true);
        assert!(!edge.sidebar_surface && edge.sidebar_at_edge);
        assert!(!surface.sidebar_surface(false).sidebar_surface);
        assert!(!edge.sidebar_at_edge(false).sidebar_at_edge);
    }

    /// Issue #564: the moved button follows the real sidebar width, not its default or the zoom.
    #[test]
    fn the_expanded_button_moves_only_when_requested_and_never_in_the_drawer() {
        #[derive(Debug, Clone, PartialEq)]
        enum Press {
            Sidebar,
            Drag,
        }
        for zoom in [1.0, 1.5] {
            for width in [size::sidebar() * 0.75, size::sidebar(), size::sidebar() * 2.0] {
                for edge in [false, true] {
                    for cover in
                        [Cover::Nothing, Cover::Drawer, Cover::Dialog, Cover::Panel, Cover::Filled(Surface::Panel)]
                    {
                        let top = TopRow::new(zoom, Press::Drag)
                            .sidebar(tr("Collapse sidebar"), Press::Sidebar)
                            .sidebar_at_edge(edge)
                            .sidebar_surface(true)
                            .cover(cover);
                        let moved = edge && cover != Cover::Drawer;
                        let x = if moved {
                            width - space::s() - size::control_sm() / 2.0
                        } else {
                            lights_room(zoom) + SIDEBAR_GAP + size::control_sm() / 2.0
                        };
                        let mut ui = iced_test::Simulator::with_size(
                            iced::Settings::default(),
                            Size::new(1000.0, TOP_ROW / zoom),
                            top_row_band(top, blank().into(), SizeClass::Wide, Some(width)),
                        );
                        ui.point_at(iced::Point::new(x, TOP_ROW / zoom / 2.0));
                        ui.simulate(iced_test::simulator::click());
                        let pressed = ui.into_messages().filter(|m| *m == Press::Sidebar).count();
                        assert_eq!(
                            pressed,
                            usize::from(matches!(cover, Cover::Nothing | Cover::Drawer)),
                            "{zoom}, {width}, {edge}, {cover:?}"
                        );
                    }
                }
            }
        }
        let top = TopRow::new(1.0, Press::Drag).sidebar_at_edge(true);
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(1000.0, TOP_ROW),
            top_row_band(top, blank().into(), SizeClass::Wide, Some(size::sidebar())),
        );
        ui.point_at(iced::Point::new(size::sidebar() - space::s() - size::control_sm() / 2.0, TOP_ROW / 2.0));
        ui.simulate(iced_test::simulator::click());
        assert!(!ui.into_messages().any(|m| m == Press::Sidebar), "no message means no button");
    }

    /// Issue #545: only a caller that keeps its buttons gets them over a side panel, at the places
    /// they have without it; a dialog and the drawer stay as they were, and the free row drags.
    #[test]
    fn kept_buttons_stay_in_reach_over_a_side_panel_only() {
        #[derive(Debug, Clone, PartialEq)]
        enum Press {
            Sidebar,
            Settings,
            Drag,
        }
        let filled = Cover::Filled(Surface::Panel);
        for cover in [Cover::Nothing, Cover::Dialog, Cover::Drawer, Cover::Panel, filled] {
            for keep in [false, true] {
                let kept = keeps_buttons(cover, keep);
                assert_eq!(kept, keep && matches!(cover, Cover::Panel | Cover::Filled(_)), "{cover:?}, {keep}");
                for (class, zoom) in [(SizeClass::Medium, 1.0), (SizeClass::Narrow, 1.0), (SizeClass::Medium, 1.5)] {
                    let width = 800.0;
                    let height = TOP_ROW / zoom;
                    let press = |x: f32| {
                        let top = TopRow::new(zoom, Press::Drag)
                            .sidebar(tr("Show sidebar"), Press::Sidebar)
                            .settings(Press::Settings)
                            .sidebar_surface(true)
                            .keep_buttons(keep)
                            .cover(cover);
                        let band = top_row_band(top, blank().into(), class, None);
                        let band: Element<'_, Press> = mouse_area(band).on_press(Press::Drag).into();
                        let mut ui =
                            iced_test::Simulator::with_size(iced::Settings::default(), Size::new(width, height), band);
                        ui.point_at(iced::Point::new(x, height / 2.0));
                        ui.simulate(iced_test::simulator::click());
                        ui.into_messages().collect::<Vec<_>>()
                    };
                    let sidebar = lights_room(zoom) + SIDEBAR_GAP + size::control_sm() / 2.0;
                    let settings = width - settings_gap(zoom) - settings_edge() / 2.0;
                    let open = matches!(cover, Cover::Nothing | Cover::Drawer);
                    let reach = |messages: Vec<Press>, wanted: Press| messages.contains(&wanted);
                    assert_eq!(reach(press(sidebar), Press::Sidebar), open || kept, "{cover:?}, {keep}, {class:?}");
                    let settings_open = open && cover != Cover::Drawer || kept;
                    assert_eq!(reach(press(settings), Press::Settings), settings_open, "{cover:?}, {keep}, {class:?}");
                    if kept {
                        // Left of the medium panel, which takes presses of its own as before.
                        assert_eq!(press(width * 0.3), vec![Press::Drag], "the free row drags: {cover:?}");
                    }
                }
            }
        }
    }

    /// Issue #564: drawing the header must supply the whole sidebar palette, not just its background.
    #[test]
    fn the_mail_head_draws_in_the_sidebar_palette_in_both_modes() {
        use std::sync::{Arc, Mutex};
        // Palette readers in the other tests do not all take SWITCHING. Keep a real scheme switch
        // in a child process so this rendering test cannot change their colours mid-assertion.
        const CHILD: &str = "TORRO_TEST_MAIL_HEAD_PALETTE";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", "shell::tests::the_mail_head_draws_in_the_sidebar_palette_in_both_modes"])
                .env(CHILD, "1")
                .output()
                .expect("run palette test in isolation");
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let original = scheme::active();
        for scheme in [
            torro_core::design::ColorScheme::standard(),
            torro_core::design::load_color_scheme(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../color-schemes"),
                "channelr",
            )
            .expect("built-in scheme"),
        ] {
            scheme::set(scheme);
            for window in [theme::theme(), theme::light()] {
                for enabled in [false, true] {
                    for lead in [None, Some(size::sidebar()), Some(320.0)] {
                        let seen = Arc::new(Mutex::new(None));
                        let captured = seen.clone();
                        let header = container(blank()).width(Fill).height(Fill).style(move |t| {
                            *captured.lock().expect("probe") = Some(palette(t));
                            container::Style::default()
                        });
                        let top = TopRow::new(1.0, Msg::Close).sidebar_surface(enabled);
                        let surface = top.head_surface();
                        let expected = if enabled { theme::sidebar_theme(&window) } else { window.clone() };
                        assert_eq!(
                            surface_style(surface)(&expected).background,
                            Some(Background::Color(if enabled {
                                palette(&expected).sidebar
                            } else {
                                palette(&expected).panel
                            }))
                        );
                        let mut ui = iced_test::Simulator::with_size(
                            iced::Settings::default(),
                            Size::new(800.0, TOP_ROW),
                            top_row_band(top, header.into(), SizeClass::Medium, lead),
                        );
                        let _ = ui.snapshot(&window).expect("draw the actual header");
                        assert_eq!(*seen.lock().expect("probe"), Some(palette(&expected)));
                    }
                }
            }
        }
        scheme::set((*original).clone());
    }

    #[test]
    fn the_settings_button_stands_as_far_from_the_right_edge_as_from_the_top_and_the_line() {
        // At zoom 1 the row is TOP_ROW high: the button in its middle over the line leaves the same
        // room above and below, and the gap to the right edge matches it.
        let gap = settings_gap(1.0);
        assert!(gap > 0.0);
        assert!((gap + settings_edge() + gap + LINE - TOP_ROW).abs() < 0.001, "{gap}");
        // One small step smaller than the other icon buttons, from the design's tokens.
        assert!((settings_edge() - (size::control_sm() - space::s())).abs() < 0.001);
        assert!(gap >= 5.0, "room to the rounded corner: {gap}");
        // A zoom that leaves no room above and below leaves none to the right either.
        assert_eq!(settings_gap(10.0), 0.0);
    }

    #[test]
    fn only_the_wide_layout_has_sidebar_and_properties() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium] {
            assert!(!has_sidebar(class) && !has_props(class), "{class:?}");
        }
        assert!(has_sidebar(SizeClass::Wide) && has_props(SizeClass::Wide));
    }

    /// A sidebar part for Iced's null renderer `()`, which lays out without a window.
    type Part = Element<'static, (), iced::Theme, ()>;

    /// A row of the sidebar at the height of the active design.
    fn sidebar_row() -> Part {
        blank().width(Fill).height(size::row_sidebar()).into()
    }

    /// Lays out a sidebar with `items` rows and two footer rows in `height`: the bounds of the
    /// switcher, the scrolling middle and the footer.
    fn lay_out_sidebar(items: usize, height: f32) -> [iced::Rectangle; 3] {
        use iced::advanced::{layout, widget::Tree};
        let switcher: Part = blank().width(Fill).height(size::control()).into();
        let mut sidebar =
            sidebar_parts(switcher, (0..items).map(|_| sidebar_row()).collect(), vec![sidebar_row(), sidebar_row()]);
        let mut tree = Tree::new(&sidebar);
        let limits = layout::Limits::new(Size::ZERO, Size::new(size::sidebar(), height));
        let node = sidebar.as_widget_mut().layout(&mut tree, &(), &limits);
        let parts: Vec<_> = node.children().iter().map(layout::Node::bounds).collect();
        [parts[0], parts[2], parts[4]]
    }

    #[test]
    fn a_long_sidebar_scrolls_in_the_middle_and_keeps_its_footer_in_the_window() {
        let height = 400.0;
        let [switcher, middle, footer] = lay_out_sidebar(60, height);
        assert_eq!(switcher.y, 0.0, "the switcher stays at the top");
        assert_eq!(footer.y + footer.height, height, "the footer stays at the bottom, inside the window");
        assert!(middle.y >= switcher.y + switcher.height && middle.y + middle.height <= footer.y);
        assert!(middle.height < 60.0 * size::row_sidebar(), "the middle takes only the rest and scrolls");
    }

    #[test]
    fn a_short_sidebar_keeps_its_footer_at_the_bottom() {
        let height = 900.0;
        let [switcher, middle, footer] = lay_out_sidebar(3, height);
        assert_eq!(footer.y + footer.height, height);
        assert_eq!(middle.y, switcher.height + space::s() + 2.0 * SIDEBAR_GAP, "rows start below the switcher");
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

    /// Issue #248: the content is the window less the sidebar the class shows and the inset margin;
    /// without a sidebar it is the whole window.
    #[test]
    fn the_content_width_leaves_out_the_sidebar_beside_it() {
        let width = 1400.0;
        let expanded = content_width(SizeClass::Wide, SidebarMode::Expanded, width);
        assert_eq!(expanded, width - size::sidebar() - layout::inset());
        assert_eq!(content_width(SizeClass::Wide, SidebarMode::Rail, width), width - rail() - layout::inset());
        for choice in [SidebarMode::Expanded, SidebarMode::Rail] {
            assert_eq!(content_width(SizeClass::Medium, choice, 800.0), 800.0 - rail() - layout::inset());
            assert_eq!(content_width(SizeClass::Narrow, choice, 480.0), 480.0, "narrow: the whole window");
        }
        assert_eq!(content_width(SizeClass::Wide, SidebarMode::Expanded, 10.0), 0.0, "never below zero");
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

    /// Task E14.1: `]` flips the mail's action rail, also with AltGr (Ctrl+Alt) or Option, never
    /// with the logo key; `[` stays the sidebar's.
    #[test]
    fn the_closing_bracket_is_the_actions_key() {
        use keyboard::{Key, Modifiers};
        let bracket = Key::Character("]".into());
        assert!(is_actions_key(&bracket, Modifiers::empty()));
        assert!(is_actions_key(&bracket, Modifiers::CTRL | Modifiers::ALT), "AltGr+9 on Windows");
        assert!(is_actions_key(&bracket, Modifiers::ALT), "Option+6 on macOS");
        assert!(!is_actions_key(&bracket, Modifiers::LOGO));
        assert!(!is_actions_key(&Key::Character("[".into()), Modifiers::empty()));
        assert!(!is_sidebar_key(&bracket, Modifiers::empty()));
    }

    /// Task E11.2: the first row carries Search and New – without `search` (the mail window, whose
    /// field stands over its list) New alone.
    #[test]
    fn the_tools_leave_search_out_where_the_field_stands_over_the_list() {
        assert_eq!(head_tools(true), [HeadTool::Search, HeadTool::New]);
        assert_eq!(head_tools(false), [HeadTool::New]);
    }

    #[test]
    fn rails_and_heads_build_in_every_class() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let side = |mode| {
                let items = vec![
                    widgets::sidebar_item_in(mode, Icon::Sun, "Today", Trail::Count(5), true, Msg::Close),
                    widgets::sidebar_group_in(mode, "Lists", Some((true, Msg::Close))),
                ];
                sidebar(sidebar_tools(mode, true, Msg::Switcher), items, vec![])
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
    fn titlebar_wraps_any_content() {
        for surface in [Surface::Window, Surface::Sidebar, Surface::Panel] {
            let filled = Cover::Filled(Surface::Panel);
            for cover in [Cover::Nothing, Cover::Dialog, Cover::Drawer, Cover::Panel, filled] {
                let buttons = || TitleButtons::none().sidebar("Collapse sidebar", Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, TitleButtons::none(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, buttons(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.5, surface, cover, buttons(), Msg::Close);
            }
        }
    }

    /// The settings button is marked while the settings are open, and nothing else about the row
    /// changes (task S.16); the band of `titlebar` has no settings button at all (issue #530).
    #[test]
    fn the_settings_button_is_marked_while_the_settings_are_open() {
        let top = TopRow::new(1.0, Msg::Close).sidebar("Collapse sidebar", Msg::Close).settings(Msg::Close);
        assert!(!top.settings_open, "closed by default, as before S.16");
        let open = top.clone().settings_open(true);
        assert!(open.settings_open);
        assert_eq!((open.sidebar, open.settings), (top.sidebar, top.settings), "nothing else changed");
        // A window without settings keeps no button.
        assert_eq!(TopRow::<Msg>::new(1.0, Msg::Close).settings_open(true).settings, None);
    }

    #[test]
    fn the_panel_button_says_what_a_press_does() {
        use SizeClass::{Medium, Narrow, Wide};
        assert_eq!(sidebar_button_label(Wide, SidebarMode::Expanded, false), tr("Collapse sidebar"));
        assert_eq!(sidebar_button_label(Wide, SidebarMode::Rail, false), tr("Expand sidebar"));
        // The drawer only opens where the sidebar is not beside the content.
        assert_eq!(sidebar_button_label(Wide, SidebarMode::Rail, true), tr("Expand sidebar"));
        for class in [Medium, Narrow] {
            for choice in [SidebarMode::Expanded, SidebarMode::Rail] {
                assert_eq!(sidebar_button_label(class, choice, false), tr("Show sidebar"), "{class:?}");
                assert_eq!(sidebar_button_label(class, choice, true), tr("Hide sidebar"), "{class:?}");
            }
        }
    }

    /// Issue #291: the lights stay where U.11 put them – halfway between the top edge and 8 below the
    /// band, as far from the left – whatever inset the design has; the built-in designs draw the
    /// content flush (inset 0), and the lights do not move with it.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_lights_have_as_much_room_above_as_below() {
        const BELOW_BAND: f32 = 8.0;
        assert_eq!(LIGHTS, (TITLEBAR + BELOW_BAND) / 2.0);
        assert_eq!(LIGHTS, 20.0, "fixed, not a token of the design");
        assert!(is_flush(layout::inset()), "the standard is flush, and the lights stay at 20");
        // The band's buttons sit on that line and stay inside the band.
        const { assert!(LIGHTS + BAND_BUTTON / 2.0 <= TITLEBAR) };
    }

    /// Issue #291: an inset of 0 draws the content flush beside the sidebar, any other an inset panel;
    /// both build, with and without the properties column.
    #[test]
    fn an_inset_of_zero_draws_the_content_flush() {
        assert!(is_flush(0.0) && !is_flush(8.0));
        assert_eq!(layout::inset(), 0.0, "the standard is flush");
        let part = || -> Element<'static, Msg> { blank().into() };
        let props = |column: Option<Element<'static, Msg>>| Props { column, side: None };
        let _ = inset_window(part(), size::sidebar(), Some(part()), part(), props(Some(part())), false);
        let _ = inset_window(part(), rail(), None, part(), props(None), true);
    }

    /// Issue #604: the properties column moves between three quarters and twice its default, the
    /// body keeps a column's least; the shell builds with the dragged column, dragging or not.
    #[test]
    fn the_properties_column_stays_inside_its_limits_and_builds() {
        let limits = props_limits(1200.0);
        assert_eq!(limits.default, size::props());
        assert_eq!(limits.clamp(10.0), 0.75 * size::props());
        assert_eq!(limits.clamp(5000.0), 2.0 * size::props());
        let tight = props_limits(800.0);
        assert_eq!(tight.clamp(5000.0), 800.0 - split::divider_width() - split::min_pane(), "the body keeps its least");
        for dragging in [false, true] {
            let props = Some(Element::from(blank()));
            let shell = shell(SizeClass::Wide, blank(), blank(), blank(), props);
            let _: Element<'_, Msg> = shell.props_width(320.0, 1200.0, dragging, |_| Msg::Close).into();
        }
    }

    /// Issue #376: the expanded sidebar moves between three quarters and twice its default, and leaves
    /// the content its rest; in every built-in design the narrowest is still wider than the rail.
    #[test]
    fn the_sidebar_width_stays_inside_its_limits() {
        let rest = 700.0;
        let limits = sidebar_limits(1600.0, rest);
        assert_eq!(limits.default, size::sidebar());
        assert_eq!(limits.clamp(10.0), 0.75 * size::sidebar(), "never narrower than three quarters");
        assert_eq!(limits.clamp(5000.0), 2.0 * size::sidebar(), "never wider than twice the default");
        let tight = sidebar_limits(1024.0, rest);
        assert_eq!(
            tight.clamp(5000.0),
            1024.0 - layout::inset() - split::divider_width() - rest,
            "the content keeps its rest"
        );
        for id in torro_core::design::BUILT_IN_IDS {
            let d = torro_core::design::load(id).expect("a built-in design");
            assert!(0.75 * d.size.sidebar > d.size.rail, "{id}: the narrowest sidebar is no rail");
        }
    }

    /// Issue #376: the window builds with the sidebar's handle in every layout, dragged or not, and
    /// without it as before.
    #[test]
    fn the_shell_builds_with_the_sidebars_width() {
        let part = || -> Element<'static, Msg> { blank().into() };
        let on = |_| Msg::Close;
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            for rail in [false, true] {
                for dragging in [false, true] {
                    let shell = shell(class, part(), part(), part(), None).rail(rail.then(part));
                    let _: Element<'_, Msg> = shell.sidebar_width(300.0, dragging, on).into();
                }
                let _: Element<'_, Msg> = shell(class, part(), part(), part(), None).rail(rail.then(part)).into();
            }
        }
    }

    /// Issue #291: the rail is the design's `size.rail`, at least as wide as its button.
    /// Issue #459: the top row builds in every class, with and without the expanded sidebar, under
    /// every cover and zoom, with and without its buttons.
    #[test]
    fn the_top_row_builds_in_every_class_and_under_every_cover() {
        let part = || -> Element<'static, Msg> { blank().into() };
        let filled = Cover::Filled(Surface::Panel);
        for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            for cover in [Cover::Nothing, Cover::Dialog, Cover::Drawer, Cover::Panel, filled] {
                for zoom in [1.0, 1.5] {
                    let bare = TopRow::new(zoom, Msg::Close).cover(cover);
                    let full = bare.clone().sidebar("Collapse sidebar", Msg::Close).settings(Msg::Close);
                    for (top, rail) in [(bare, false), (full.clone(), true), (full.settings_open(true), false)] {
                        let shell = shell(class, part(), part(), part(), None).rail(rail.then(part));
                        let _: Element<'_, Msg> = shell.sidebar_width(280.0, false, |_| Msg::Close).top_row(top).into();
                    }
                }
            }
        }
    }

    /// Issue #459: the row is twice the lights' line high, so they sit in its middle, and the settings
    /// have sliders everywhere.
    #[test]
    fn the_top_row_centres_the_lights_and_shows_sliders() {
        #[cfg(target_os = "macos")]
        assert_eq!(TOP_ROW, 2.0 * LIGHTS);
        assert_eq!(TOP_ROW, 40.0);
        assert_eq!(SETTINGS_ICON, Icon::SlidersHorizontal);
        let top = TopRow::new(1.0, Msg::Close);
        assert!(!top.settings_open && top.settings.is_none() && top.sidebar.is_none(), "nothing by default");
        assert!(top.settings(Msg::Close).settings_open(true).settings_open);
    }

    /// Issue #459: the header starts at the content's edge beside the expanded sidebar, and after the
    /// lights and the panel button beside the rail and in narrow windows. Issue #530: without the app
    /// switcher the lead is the same in both, so the panel button stands at the same place.
    #[test]
    fn the_header_starts_after_the_lead_of_the_top_row() {
        assert_eq!(head_inset(SizeClass::Wide, SidebarMode::Expanded, 1.0), 0.0);
        let rail = head_inset(SizeClass::Wide, SidebarMode::Rail, 1.0);
        assert_eq!(head_inset(SizeClass::Medium, SidebarMode::Expanded, 1.0), rail, "medium always has the rail");
        assert_eq!(rail, (compact_lead_width(1.0) - self::rail() - LINE).max(0.0));
        assert_eq!(head_inset(SizeClass::Narrow, SidebarMode::Expanded, 1.0), compact_lead_width(1.0));
    }

    #[test]
    fn the_sidebar_tools_build_in_both_modes() {
        for mode in [SidebarMode::Expanded, SidebarMode::Rail] {
            for search in [true, false] {
                let _: Element<'_, Msg> = sidebar_tools(mode, search, Msg::Switcher);
            }
        }
    }

    #[test]
    fn the_rail_is_the_designs() {
        assert_eq!(rail(), size::rail());
        assert_eq!(rail(), 60.0, "the standard's");
        assert!(rail() >= size::control_lg());
    }

    #[test]
    fn only_macos_has_the_band() {
        assert_eq!(has_band(), cfg!(target_os = "macos"));
    }

    #[test]
    fn the_band_is_covered_where_an_overlay_reaches_the_top() {
        use SizeClass::{Medium, Mini, Narrow, Strip, Wide};
        assert_eq!(Cover::dialog(true), Cover::Dialog);
        assert_eq!(Cover::dialog(false), Cover::Nothing);
        // The drawer only exists beside a shell without the sidebar.
        for (class, cover) in [(Wide, Cover::Nothing), (Medium, Cover::Drawer), (Narrow, Cover::Drawer)] {
            assert_eq!(Cover::drawer(class, true), cover, "{class:?}");
        }
        assert_eq!(Cover::drawer(Mini, true), Cover::Nothing, "a glance has no drawer");
        assert_eq!(Cover::drawer(Medium, false), Cover::Nothing);
        // The panel dims only in medium: beside in wide, the whole window in narrow.
        let filled = Cover::Filled(Surface::Panel);
        for (class, cover) in [(Wide, Cover::Nothing), (Medium, Cover::Panel), (Narrow, filled)] {
            assert_eq!(Cover::panel(class, true), cover, "{class:?}");
        }
        assert_eq!(Cover::panel(Strip, true), Cover::Nothing);
        assert_eq!(Cover::panel(Medium, false), Cover::Nothing);
        // The top one wins.
        assert_eq!(Cover::dialog(true).or(Cover::Drawer), Cover::Dialog);
        assert_eq!(Cover::dialog(false).or(Cover::panel(Medium, true)).or(Cover::Drawer), Cover::Panel);
        assert_eq!(Cover::panel(Narrow, true).or(Cover::Drawer), filled, "the narrow panel hides the drawer");
        assert_eq!(Cover::Nothing.or(Cover::Nothing), Cover::Nothing);
    }

    #[test]
    fn content_gets_the_height_below_the_band() {
        let window = Size::new(240.0, 160.0);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(below_titlebar(window, 1.0), Size::new(240.0, 160.0 - TITLEBAR));
            // At 200 % the band is half as many layout units, as `titlebar` draws it.
            assert_eq!(below_titlebar(window, 2.0), Size::new(240.0, 160.0 - TITLEBAR / 2.0));
            assert_eq!(below_titlebar(Size::new(240.0, 10.0), 1.0).height, 0.0, "never negative");
        }
        #[cfg(not(target_os = "macos"))]
        assert_eq!(below_titlebar(window, 1.5), window);
    }

    #[test]
    fn the_band_takes_the_surface_at_the_top_of_each_class() {
        // Sidebar or rail run up to the top on the window background; elsewhere the panel does.
        assert_eq!(Surface::of(SizeClass::Wide), Surface::Sidebar);
        assert_eq!(Surface::of(SizeClass::Medium), Surface::Sidebar);
        assert_eq!(Surface::of(SizeClass::Narrow), Surface::Panel);
        assert_eq!(Surface::of(SizeClass::Mini), Surface::Panel);
        assert_eq!(Surface::of(SizeClass::Strip), Surface::Panel);
    }

    #[test]
    fn every_suite_app_has_a_short_name_and_icon() {
        assert_eq!(app_name(AppId::TASKS), tr("Tasks"));
        assert_eq!(app_name(AppId::CALENDAR), tr("Calendar"));
        assert_eq!(app_name(AppId::WORKPLACE), tr("Workplace"));
        assert_ne!(app_icon(AppId::TASKS), app_icon(AppId::CALENDAR));
        assert_eq!(app_name(AppId::INBOX), tr("Notification Hub"));
        assert_eq!(app_name(AppId::INBOX_MAIL), tr("Mail"));
        assert_eq!(app_name(AppId::INBOX_NOTIFICATIONS), tr("App Feed"));
        let [hub, mail, notices] = [AppId::INBOX, AppId::INBOX_MAIL, AppId::INBOX_NOTIFICATIONS].map(app_icon);
        assert!(hub != mail && mail != notices && hub != notices, "each window kind its own icon");
    }

    /// Issue #127: the inbox's three windows are told apart at a glance. Their names share no word
    /// (in either language), mail has the envelope, and each has a hue of its own; apps with one
    /// window keep the suite's logo.
    #[test]
    fn the_inbox_windows_have_names_icons_and_hues_of_their_own() {
        let windows = [AppId::INBOX, AppId::INBOX_MAIL, AppId::INBOX_NOTIFICATIONS];
        let words: Vec<Vec<String>> = windows
            .iter()
            .map(|&app| app_name(app).split([' ', '-']).map(|word| word.to_lowercase().replace('s', "")).collect())
            .collect();
        for (i, these) in words.iter().enumerate() {
            for those in &words[i + 1..] {
                assert!(these.iter().all(|word| !those.contains(word)), "{these:?} and {those:?} share a word");
            }
        }
        assert_eq!(app_icon(AppId::INBOX_MAIL), Icon::Mail, "mail is mail, not the hub's tray");
        let hues: Vec<Option<usize>> = windows.iter().map(|&app| app_hue(app)).collect();
        assert!(hues.iter().all(Option::is_some), "{hues:?}");
        assert!(hues[0] != hues[1] && hues[1] != hues[2] && hues[0] != hues[2], "{hues:?}");
        for app in [AppId::TASKS, AppId::CALENDAR, AppId::WORKPLACE, AppId::ASSISTANT] {
            assert_eq!(app_hue(app), None, "{}: the suite's logo", app.window_class);
        }
        let _: Element<'_, ()> = app_mark(AppId::INBOX_MAIL);
    }

    #[test]
    fn shells_build_in_every_class() {
        for class in [SizeClass::Mini, SizeClass::Strip, SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
            let anchor = Anchor::new();
            let side = sidebar(sidebar_tools(SidebarMode::Expanded, true, Msg::Switcher), vec![], vec![]);
            let header = header_bar([sidebar_toggle(Msg::Close)], header_title(Some(Icon::Sun), "Today", None), []);
            let props: Option<Element<'_, Msg>> = has_props(class).then(|| blank().into());
            let layer = Some(widgets::floating(
                &anchor,
                widgets::Placement::Below(widgets::Align::Start),
                widgets::menu(vec![MenuSection::titled("Lists", vec![MenuItem::new(Icon::Sun, "Today", Msg::Close)])]),
                Msg::Close,
            ));
            let _: Element<'_, Msg> =
                shell(class, side, header, blank(), props).drawer(true, Msg::Close).overlay(layer).into();
        }
    }
}
