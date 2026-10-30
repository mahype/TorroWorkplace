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
//!         │ [T] App ▾ 🔍 ✎│ header 64                                             │
//!         │ rows …      │├──────────────────────────────────┬──────────────────┤
//!         │             ││ body                             │ properties 300   │
//!         └─────────────┘└──────────────────────────────────┴──────────────────┘
//! Medium  ┌rail┐│ panel (flush) ──────────────────────────────────────────────────┐
//!         │ T  ││ header 64                                                       │
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
//! The sidebar has three states: **expanded** (240, as above), **rail** (only the icons, [`rail()`] wide,
//! names in tooltips, group titles as lines) and **drawer** (the expanded sidebar over the content,
//! [`Shell::drawer`]). In the wide layout the user picks expanded or rail with the panel button –
//! on macOS in the band right of the traffic lights ([`TitleButtons`]), elsewhere at the top of the
//! sidebar ([`sidebar_head`]) – or the key `[` ([`is_sidebar_key`]); the app stores the
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
//! # Width of the expanded sidebar
//!
//! An app may let the person drag the expanded sidebar wider or narrower (issue #376, first the mail
//! window): [`Shell::sidebar_width`] lays a handle over its right edge that works like the divider of
//! [`crate::split`], and [`sidebar_limits`] bounds the width. The rail, the drawer and
//! [`Cover::Drawer`] keep [`size::sidebar()`].
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
use crate::widgets::{self, Anchor, MenuItem, MenuSection, Tone, Trail};

use crate::themed::in_sidebar;

/// The one-pixel lines of the design.
const LINE: f32 = 1.0;
/// Height of the app switcher row (`torro.css`: `.switcher{height:32px}`).
fn switcher_height() -> f32 {
    size::control()
}
/// Edge of the app logo in the switcher.
const LOGO: f32 = 20.0;
/// Gap between the rows of the sidebar (`torro.css`: `.sidebar{gap:2px}`).
pub(crate) const SIDEBAR_GAP: f32 = 2.0;
/// Width of the collapsed sidebar (rail) and of the collapsed action rail of the mail window: the
/// design's [`size::rail()`] (issue #291), 60 in the standard design – rows as wide as a large control
/// (40) with 10 at both sides, room for a 16 icon on its hover surface, and the icons sit on the same
/// axis as the logo. The rules keep it at least as wide as [`size::control_lg()`].
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
    /// [`rail()`] wide: only the icons, names and counters in tooltips, group titles as lines, the app
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
    let default = size::sidebar();
    split::Limits::new(default, window - layout::inset()).bounded(0.75 * default, 2.0 * default, rest)
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
}

impl<'a, M: Clone + 'a> From<Shell<'a, M>> for Element<'a, M> {
    fn from(shell: Shell<'a, M>) -> Self {
        let Shell { class, sidebar, header, body, props, rail, drawer: on_dismiss, overlay, side } = shell;
        let header = header_box(class, header);
        let mut layers = Vec::new();
        let rail = rail.filter(|_| matches!(class, SizeClass::Wide | SizeClass::Medium));
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
/// it has, the panel button and the settings live in the band, so apps leave them out of the sidebar
/// ([`sidebar_head`] does that itself) and the narrow header ([`sidebar_toggle`]); elsewhere they stay
/// where they were.
pub fn has_band() -> bool {
    cfg!(target_os = "macos")
}

/// The window-wide buttons in the band of [`titlebar`] (U.10, as Claude and Slack have them): the
/// panel button right of the traffic lights and the settings in the top right corner. Only macOS
/// draws them ([`has_band`]); a window without them (a compose window, a glance) passes
/// [`TitleButtons::none`].
///
/// ```ignore
/// let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
/// let buttons = TitleButtons::none().sidebar(label, Message::ToggleSidebar).settings(Message::OpenSettings);
/// // While the settings fill the content area, the gear is marked (task S.16).
/// let buttons = buttons.settings_open(self.settings.is_some());
/// shell::titlebar(content, zoom, surface, cover, buttons, Message::DragWindow)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TitleButtons<M> {
    sidebar: Option<(&'static str, M)>,
    settings: Option<M>,
    settings_open: bool,
}

impl<M> TitleButtons<M> {
    /// No buttons: the band holds the traffic lights alone.
    pub fn none() -> Self {
        TitleButtons { sidebar: None, settings: None, settings_open: false }
    }

    /// The panel button, which sends `on_press` – the same as `[` ([`is_sidebar_key`]): the app flips
    /// its choice in the wide layout and opens or closes the drawer elsewhere. `label` is its tooltip,
    /// from [`sidebar_button_label`].
    pub fn sidebar(mut self, label: &'static str, on_press: M) -> Self {
        self.sidebar = Some((label, on_press));
        self
    }

    /// The settings button ("Settings"), which sends `on_press`.
    pub fn settings(mut self, on_press: M) -> Self {
        self.settings = Some(on_press);
        self
    }

    /// Marks the settings button while the settings are `open` – they fill the app's content area,
    /// and the gear leads back out of them (task S.16, `docs/settings.md`): `selected` under the
    /// icon and the icon in `text`, like a chosen sidebar row. Neutral, never red.
    pub fn settings_open(mut self, open: bool) -> Self {
        self.settings_open = open;
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
/// of the traffic lights, the settings in the top right corner. Over a dialog or a side panel neither
/// shows (the window below is dimmed), over the drawer only the panel button, which closes it.
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
/// The gap between the settings button and the right edge, in points: its icon is centred as far from
/// the edge as the close button is from the left one.
#[cfg(target_os = "macos")]
const BAND_TRAILING: f32 = LIGHTS - BAND_BUTTON / 2.0;
/// Edge of a button in the band, in points.
#[cfg(target_os = "macos")]
const BAND_BUTTON: f32 = 24.0;

/// The row of [`TitleButtons`] over the band, as far as `cover` leaves them; all sizes in points,
/// divided by `zoom` into layout units.
#[cfg(target_os = "macos")]
fn band_buttons<'a, M: Clone + 'a>(buttons: TitleButtons<M>, cover: Cover, zoom: f32) -> Element<'a, M> {
    let TitleButtons { sidebar, settings, settings_open } = buttons;
    let (sidebar, settings) = match cover {
        Cover::Nothing => (sidebar, settings),
        Cover::Drawer => (sidebar, None),
        Cover::Dialog | Cover::Panel | Cover::Filled(_) => (None, None),
    };
    let button_of = |which: Icon, label: &'static str, on_press: M, marked: bool| -> Element<'a, M> {
        let tone = if marked { Tone::Text } else { Tone::Text2 };
        let mark = widgets::toned(which, icon::SIZE / zoom, tone);
        let edge = BAND_BUTTON / zoom;
        let pressable = button(container(mark).center(Fill)).width(edge).height(edge).padding(0);
        // One closure for both, because the two styles are different types.
        let how = move |theme: &iced::Theme, status: iced::widget::button::Status| match marked {
            true => style::button::sidebar_item(true)(theme, status),
            false => style::button::icon(theme, status),
        };
        widgets::with_tooltip(pressable.style(how).on_press(on_press), label)
    };
    let mut buttons = row![blank().width(BAND_LEADING / zoom)];
    if let Some((label, on_press)) = sidebar {
        buttons = buttons.push(button_of(Icon::PanelLeft, label, on_press, false));
    }
    buttons = buttons.push(blank::horizontal());
    if let Some(on_press) = settings {
        buttons = buttons.push(button_of(Icon::Settings, tr("Settings"), on_press, settings_open));
    }
    // Centred on the line of the traffic lights, not in the band: the line sits lower (see `LIGHTS`).
    let row = buttons.push(blank().width(BAND_TRAILING / zoom)).height(BAND_BUTTON / zoom).align_y(Alignment::Center);
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
fn inset_window<'a, M: 'a>(
    sidebar: Element<'a, M>,
    width: f32,
    header: Element<'a, M>,
    body: Element<'a, M>,
    props: Option<Element<'a, M>>,
    dragged: bool,
) -> Element<'a, M> {
    let mut main = row![container(body).width(Fill).height(Fill)];
    if let Some(props) = props {
        let column = scrollable(container(props).width(Fill).padding(Padding {
            top: space::l(),
            right: space::s(),
            bottom: space::xl(),
            left: space::s(),
        }))
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .width(size::props() - LINE)
        .height(Fill);
        main = main.push(rule::vertical(LINE).style(style::rule)).push(column);
    }
    let content = column![header, main.height(Fill)];
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

/// The content of the sidebar: the `switcher` row ([`app_switcher`], or [`sidebar_head`] with the
/// panel button) with 8 below it, the `items` (navigation, [`widgets::sidebar_group`]s with their
/// rows) from the top, and the `footer` rows ("Open Calendar ↗", "Settings") at the bottom. Rows
/// are 2 apart. The same for both [`SidebarMode`]s: build the pieces in the mode you want.
///
/// Only the `items` scroll (task U.12): a design pack with taller rows or a low window has no room
/// for all of them, and the switcher and the footer must stay in reach. The scrollbar floats in the
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
        // 8 below the switcher and the row gap at both sides, as when the rows followed it directly.
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

/// The header button that opens the sidebar drawer in the medium and narrow layouts. Only where the
/// window has no band ([`has_band`]); with one, the band's panel button does it.
pub fn sidebar_toggle<'a, M: Clone + 'a>(on_press: M) -> Element<'a, M> {
    widgets::icon_button(Icon::PanelLeft, tr("Show sidebar"), Some(on_press))
}

/// What the app switcher asks the app to do; the app wraps it in its own message
/// (`Message::Switcher(Switcher)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switcher {
    /// Open or close the switcher menu ([`switcher_menu`]).
    Toggle,
    /// Start or raise another app of the suite: send `Request::OpenApp { app }` to the core, then
    /// close the menu. Carries [`AppId::open_name`]: the binary for an app with one window (as
    /// [`AppId::binary`]), the window class for a further window of a shared binary
    /// (`torro-inbox-mail`, task E8.1).
    OpenApp(&'static str),
    /// The search button next to the switcher.
    Search,
    /// The "New" button next to the switcher (new task, new event …).
    New,
}

/// The short name of an app in the switcher ("Tasks", "Calendar", "Workplace"); the inbox's three
/// windows by their kind ("Notification Hub", "Mail", "App Feed"), since they share one binary. The
/// three names share no word (issue #127), so nobody takes one window for another.
fn app_name(app: AppId) -> &'static str {
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

/// The icon of an app in the switcher menu and on its mark ([`app_mark`]). Each of the inbox's
/// windows has its own: the hub the tray, mail the envelope, the app feed the bell.
fn app_icon(app: AppId) -> Icon {
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

/// The mark of `app`, 20 × 20, where the switcher shows the logo (issue #127): for a window that
/// shares its program with others ([`app_hue`]: the inbox's hub, mail window and app feed) its icon
/// ([`app_icon`], 14) in its own hue on a tile tinted with it ([`theme::Palette::tinted`], radius
/// [`radius::xs()`]), so the windows differ at a glance; for every other app the suite's logo.
///
/// The switcher and the rail show it by themselves. A window without sidebar or rail (narrow) puts
/// it first in its header ([`header_bar`]'s `leading`), so the window still says which one it is.
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

/// The apps [`switcher_menu`] lists for a window of `current`: the apps of components turned off (task
/// W.2) and apps not installed (issue #408) are left out; the window's own app stays.
fn switcher_apps(current: AppId) -> Vec<AppId> {
    AppId::ALL
        .iter()
        .copied()
        .filter(|&app| (crate::components::app_on(app) && crate::installed::is_installed(app)) || app == current)
        .collect()
}

/// What a click on `app` in the menu of a window of `current` sends: [`Switcher::Toggle`] (closes)
/// for the window's own entry, else [`Switcher::OpenApp`] with its [`AppId::open_name`]. Entries
/// are told apart by window class, so the inbox's mail window can open its hub and the other way.
fn switcher_message(app: AppId, current: AppId) -> Switcher {
    if app.window_class == current.window_class { Switcher::Toggle } else { Switcher::OpenApp(app.open_name()) }
}

/// The top row of the sidebar, like Linear's workspace menu: the logo, the name of `current`
/// with a chevron (registers `anchor` and sends [`Switcher::Toggle`]), then search and a framed
/// "New" button. `on` wraps the switcher's messages in the app's (`Message::Switcher`).
pub fn app_switcher<'a, M: Clone + 'a>(current: AppId, anchor: &Anchor, on: fn(Switcher) -> M) -> Element<'a, M> {
    switcher_row(current, anchor, on, None, true)
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
///
/// Where the window has the band of [`titlebar`] ([`has_band`], macOS), the panel button lies there
/// ([`TitleButtons::sidebar`]) and the head leaves it out.
///
/// The same as [`sidebar_head_in`] with the search button.
pub fn sidebar_head<'a, M: Clone + 'a>(
    mode: SidebarMode,
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: M,
) -> Element<'a, M> {
    sidebar_head_in(mode, current, anchor, on, on_toggle, true)
}

/// [`sidebar_head`] with the search button ([`Icon::Search`], "Search", [`Switcher::Search`]) only
/// while `search` (task E11.2): a window whose search field always stands over its list – the
/// inbox's mail window – leaves the magnifier out in both forms, expanded and rail, since a second
/// way to the same field only takes room; its keys (`/`, ⌘F / Ctrl + F) still focus the field.
/// Everything else is as in [`sidebar_head`]; which tools the head carries says [`head_tools`].
///
/// ```ignore
/// shell::sidebar_head_in(mode, AppId::INBOX_MAIL, switcher, Message::Switcher, Message::ToggleSidebar, false)
/// ```
pub fn sidebar_head_in<'a, M: Clone + 'a>(
    mode: SidebarMode,
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: M,
    search: bool,
) -> Element<'a, M> {
    match mode {
        SidebarMode::Expanded => switcher_row(current, anchor, on, (!has_band()).then_some(on_toggle), search),
        SidebarMode::Rail => {
            // The logo on a full-width row, as the other rows of the rail.
            let logo = button(center_x(app_mark(current)))
                .width(Fill)
                .height(switcher_height())
                .padding(0)
                .style(style::button::sidebar_item(false))
                .on_press(on(Switcher::Toggle));
            let mut head =
                column![widgets::anchored(anchor, widgets::tooltip_right(logo, app_name(current), Trail::None))];
            head = head.extend(head_tools(search).into_iter().map(|tool| tool.button(on)));
            if !has_band() {
                head = head.push(widgets::icon_button(Icon::PanelLeft, tr("Expand sidebar"), Some(on_toggle)));
            }
            head.spacing(SIDEBAR_GAP).align_x(Alignment::Center).into()
        }
    }
}

/// A tool beside the app switcher in the head of the sidebar ([`sidebar_head_in`]).
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

/// The tools of the sidebar's head after the switcher, in their order: Search (only while
/// `search`, [`sidebar_head_in`]), then New.
pub fn head_tools(search: bool) -> Vec<HeadTool> {
    [search.then_some(HeadTool::Search), Some(HeadTool::New)].into_iter().flatten().collect()
}

/// `content` centred across the full width, at its own height.
fn center_x<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).center_x(Fill).center_y(Fill).into()
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

/// The switcher row of [`app_switcher`], with the panel button at its end when `on_toggle` is given
/// and the search button only while `search` ([`head_tools`]).
fn switcher_row<'a, M: Clone + 'a>(
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: Option<M>,
    search: bool,
) -> Element<'a, M> {
    let name = row![
        app_mark(current),
        typo::text(typo::body_strong(), app_name(current)).style(theme::text).wrapping(Wrapping::None),
        widgets::toned(Icon::ChevronDown, icon::SIZE_SM, Tone::Text3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    // 6 inside, as `.switcher{padding:0 6px}`, so the logo lines up with the row icons below.
    let trigger = button(container(name).center_y(Fill).clip(true))
        .width(Fill)
        .height(switcher_height())
        .padding(Padding::new(0.0).left(6.0).right(6.0))
        .style(style::button::sidebar_item(false))
        .on_press(on(Switcher::Toggle));
    let mut top = row![widgets::anchored(anchor, trigger)];
    top = top.extend(head_tools(search).into_iter().map(|tool| tool.button(on)));
    if let Some(on_toggle) = on_toggle {
        top = top.push(widgets::icon_button(Icon::PanelLeft, tr("Collapse sidebar"), Some(on_toggle)));
    }
    top.spacing(SIDEBAR_GAP).align_y(Alignment::Center).into()
}

/// The menu of the app switcher: every app of the suite ([`AppId::ALL`]) whose component is on
/// ([`crate::components`], task W.2) and that is installed ([`crate::installed`], issue #408), `current`
/// marked and always there. A click
/// on another app sends [`Switcher::OpenApp`], on the current one [`Switcher::Toggle`] (closes).
/// Show it with `widgets::floating(&anchor, Placement::Below(Align::Start), switcher_menu(..), close)`.
pub fn switcher_menu<'a, M: Clone + 'a>(current: AppId, on: fn(Switcher) -> M) -> Element<'a, M> {
    let items = switcher_apps(current)
        .into_iter()
        .map(|app| {
            let message = switcher_message(app, current);
            let is_current = message == Switcher::Toggle;
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

    /// Task E11.2: the head carries Search and New – without `search` (the mail window, whose field
    /// stands over its list) New alone, in both forms; `sidebar_head` keeps its magnifier.
    #[test]
    fn the_head_leaves_search_out_where_the_field_stands_over_the_list() {
        assert_eq!(head_tools(true), [HeadTool::Search, HeadTool::New]);
        assert_eq!(head_tools(false), [HeadTool::New]);
        let anchor = Anchor::new();
        for mode in [SidebarMode::Expanded, SidebarMode::Rail] {
            for search in [true, false] {
                let _: Element<'_, Msg> =
                    sidebar_head_in(mode, AppId::TASKS, &anchor, Msg::Switcher, Msg::Close, search);
            }
            let _: Element<'_, Msg> = sidebar_head(mode, AppId::TASKS, &anchor, Msg::Switcher, Msg::Close);
        }
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
    fn titlebar_wraps_any_content() {
        for surface in [Surface::Window, Surface::Sidebar, Surface::Panel] {
            let filled = Cover::Filled(Surface::Panel);
            for cover in [Cover::Nothing, Cover::Dialog, Cover::Drawer, Cover::Panel, filled] {
                let buttons = || TitleButtons::none().sidebar("Collapse sidebar", Msg::Close).settings(Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, TitleButtons::none(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, buttons(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.5, surface, cover, buttons(), Msg::Close);
            }
        }
    }

    /// The gear is marked while the settings are open, and nothing else about the buttons changes
    /// (task S.16).
    #[test]
    fn the_gear_is_marked_while_the_settings_are_open() {
        let buttons = TitleButtons::none().sidebar("Collapse sidebar", Msg::Close).settings(Msg::Close);
        assert!(!buttons.settings_open, "closed by default, as before S.16");
        let open = buttons.clone().settings_open(true);
        assert!(open.settings_open);
        assert_eq!((open.sidebar, open.settings), (buttons.sidebar, buttons.settings), "nothing else changed");
        // A window without a gear keeps none.
        assert_eq!(TitleButtons::<Msg>::none().settings_open(true).settings, None);
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
        assert_eq!(BAND_TRAILING + BAND_BUTTON / 2.0, LIGHTS, "the settings mirror the close button");
    }

    /// Issue #291: an inset of 0 draws the content flush beside the sidebar, any other an inset panel;
    /// both build, with and without the properties column.
    #[test]
    fn an_inset_of_zero_draws_the_content_flush() {
        assert!(is_flush(0.0) && !is_flush(8.0));
        assert_eq!(layout::inset(), 0.0, "the standard is flush");
        let part = || -> Element<'static, Msg> { blank().into() };
        let _ = inset_window(part(), size::sidebar(), part(), part(), Some(part()), false);
        let _ = inset_window(part(), rail(), part(), part(), None, true);
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
            1024.0 - layout::inset() - split::handle_width() - rest,
            "the content keeps its rest"
        );
        for id in torro_core::design::BUILT_IN_IDS {
            let d = torro_core::design::load(std::path::Path::new(""), id).expect("a built-in design");
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
    fn switcher_messages_name_the_binary_to_open() {
        let wrap: fn(Switcher) -> Msg = Msg::Switcher;
        assert_eq!(wrap(Switcher::OpenApp(AppId::CALENDAR.binary)), Msg::Switcher(Switcher::OpenApp("torrocalendar")));
        assert_eq!(switcher_message(AppId::CALENDAR, AppId::TASKS), Switcher::OpenApp("torrocalendar"));
        assert_eq!(switcher_message(AppId::TASKS, AppId::TASKS), Switcher::Toggle);
    }

    #[test]
    fn the_switcher_lists_only_installed_apps() {
        let _lock = crate::installed::TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::installed::reset();
        assert_eq!(switcher_apps(AppId::TASKS), AppId::ALL, "before the core answers, all");
        let calendar = torro_ipc::AppInfo {
            binary: AppId::CALENDAR.binary.into(),
            name: AppId::CALENDAR.display_name.into(),
            window_class: AppId::CALENDAR.window_class.into(),
            installed: true,
        };
        crate::installed::set(&[calendar], false);
        // Neither the main application nor the other apps are there; the own app stays.
        assert_eq!(switcher_apps(AppId::TASKS), [AppId::TASKS, AppId::CALENDAR]);
        crate::installed::reset();
    }

    /// The inbox's three windows share a binary (task E8.1): each is an entry of its own, only the
    /// window's own is current, the others open by the name that names them.
    #[test]
    fn the_inbox_windows_are_three_entries_of_the_switcher() {
        let current = AppId::INBOX_MAIL;
        assert_eq!(switcher_message(AppId::INBOX_MAIL, current), Switcher::Toggle);
        assert_eq!(switcher_message(AppId::INBOX, current), Switcher::OpenApp("torroinbox"));
        assert_eq!(
            switcher_message(AppId::INBOX_NOTIFICATIONS, current),
            Switcher::OpenApp("torro-inbox-notifications")
        );
        let toggles = AppId::ALL.iter().filter(|&&app| switcher_message(app, current) == Switcher::Toggle).count();
        assert_eq!(toggles, 1, "one current entry");
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
