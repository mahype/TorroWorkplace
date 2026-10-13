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
//! While something is dragged over the sidebar, the rows that may take it are wrapped in
//! [`drop_target`]: they report the pointer and show a ring while it is on them.

use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{button, column, container, mouse_area, opaque, row, rule, scrollable, stack};
use iced::{Alignment, Background, Border, Color, Element, Fill, Padding, Shadow, Size, Vector, keyboard};
use serde::{Deserialize, Serialize};

use torro_core::AppId;
use torro_core::i18n::tr;
use torro_pim::{PimAccount, account::account_of};

use crate::design;
use crate::icon::{self, Icon};
use crate::layout::{self, SizeClass};
use crate::style;
use crate::theme::{self, palette, radius, size, space};
use crate::typo;
use crate::widgets::{self, Anchor, MenuItem, MenuSection, Tone, Trail};

/// The one-pixel lines of the design.
const LINE: f32 = 1.0;
/// Height of the app switcher row (`torro.css`: `.switcher{height:32px}`).
fn switcher_height() -> f32 {
    size::control()
}
/// Edge of the app logo in the switcher.
const LOGO: f32 = 20.0;
/// Gap between the rows of the sidebar (`torro.css`: `.sidebar{gap:2px}`).
const SIDEBAR_GAP: f32 = 2.0;
/// Width of the collapsed sidebar (rail): rows as wide as a large control (36), 8 at both sides –
/// room for a 16 icon on its hover surface, and the icons sit on the same axis as the logo.
pub fn rail() -> f32 {
    size::control_lg() + 2.0 * space::s()
}

/// How a sidebar is drawn. The user's choice for the wide layout, stored per app in
/// [`crate::ui_state::UiState::sidebar`] (`"expanded"` / `"rail"` in `ui.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarMode {
    /// 232 wide: icons, names, counters, group titles (design skill, section 0).
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
                Some(rail) => layers.push(inset_window(rail, self::rail(), header, body, props)),
                None => layers.push(inset_window(sidebar, size::sidebar(), header, body, props)),
            }
        } else if let Some(rail) = rail {
            layers.push(inset_window(rail, self::rail(), header, body, props));
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
/// of the close button from the left edge (U.11). The line lies halfway between the window's top edge
/// and the inset panel below the band (32 + 8), so the lights have as much room above as below, and
/// the close button is as far from the left edge as from the top – as in Claude. macOS itself would
/// put them at 16 / 16; [`crate::place_traffic_lights`] moves them.
#[cfg(target_os = "macos")]
pub(crate) const LIGHTS: f32 = 20.0;

/// The surface at the top of a window, right under the band of [`titlebar`]: the band takes its
/// colour, so the top of every window reads as one piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The window background `bg`: the sidebar or the rail (wide, medium), and the states that lie
    /// on the window itself (signing in, loading or failing before a layout exists).
    Window,
    /// The content panel `panel`: the flush narrow window and the glance of mini and strip.
    Panel,
}

impl Surface {
    /// The surface at the top of the window [`shell`] lays out in `class`, or of the glance in mini
    /// and strip: the window background where a sidebar or rail runs up to the top, else the panel.
    pub fn of(class: SizeClass) -> Surface {
        match sidebar_mode(class, SidebarMode::Expanded) {
            Some(_) => Surface::Window,
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
/// shell::titlebar(content, zoom, surface, cover, buttons, Message::DragWindow)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TitleButtons<M> {
    sidebar: Option<(&'static str, M)>,
    settings: Option<M>,
}

impl<M> TitleButtons<M> {
    /// No buttons: the band holds the traffic lights alone.
    pub fn none() -> Self {
        TitleButtons { sidebar: None, settings: None }
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
        let band = stack![band(surface, cover), band_buttons(buttons, cover, zoom)];
        let band = container(band).width(Fill).height(TITLEBAR / zoom);
        // The buttons capture their presses, so only the free band drags the window.
        column![mouse_area(band).on_press(on_drag), container(content).width(Fill).height(Fill)].into()
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
    let TitleButtons { sidebar, settings } = buttons;
    let (sidebar, settings) = match cover {
        Cover::Nothing => (sidebar, settings),
        Cover::Drawer => (sidebar, None),
        Cover::Dialog | Cover::Panel | Cover::Filled(_) => (None, None),
    };
    let button_of = |which: Icon, label: &'static str, on_press: M| -> Element<'a, M> {
        let mark = widgets::toned(which, icon::SIZE / zoom, Tone::Text2);
        let edge = BAND_BUTTON / zoom;
        let pressable = button(container(mark).center(Fill)).width(edge).height(edge).padding(0);
        widgets::with_tooltip(pressable.style(style::button::icon).on_press(on_press), label)
    };
    let mut buttons = row![blank().width(BAND_LEADING / zoom)];
    if let Some((label, on_press)) = sidebar {
        buttons = buttons.push(button_of(Icon::PanelLeft, label, on_press));
    }
    buttons = buttons.push(blank::horizontal());
    if let Some(on_press) = settings {
        buttons = buttons.push(button_of(Icon::Settings, tr("Settings"), on_press));
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
            Surface::Window => style::container::sidebar,
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
            row![drawer, dimmed()].into()
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

/// The header bar: 44 high, the padding of the size class, a line below.
fn header_box<'a, M: 'a>(class: SizeClass, header: Element<'a, M>) -> Element<'a, M> {
    let (left, right) = match class {
        SizeClass::Wide | SizeClass::Medium => (space::l(), space::m()),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => (space::s(), space::s()),
    };
    let bar = container(header).width(Fill).center_y(size::header()).padding(Padding::new(0.0).left(left).right(right));
    column![bar, rule::horizontal(LINE).style(style::rule)].into()
}

/// The sidebar (`width` 232) or the rail ([`rail()`]) on the window background, and the content as
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
    // The 1-px padding keeps the content inside the panel's border, as `border-box` in the CSS.
    let panel = container(column![header, main.height(Fill)])
        .width(Fill)
        .height(Fill)
        .padding(LINE)
        .style(style::container::panel);
    let side = container(sidebar).width(width).height(Fill).padding(space::s());
    container(row![side, container(panel).padding(Padding { left: 0.0, ..Padding::new(layout::inset()) })])
        .width(Fill)
        .height(Fill)
        .style(style::container::sidebar)
        .into()
}

/// The sidebar as a drawer over the left edge: the rest of the window is dimmed with the scrim (as
/// behind a dialog) and the drawer casts the menu shadow to the right; a click on the scrim
/// dismisses.
fn drawer<'a, M: Clone + 'a>(sidebar: Element<'a, M>, on_dismiss: M) -> Element<'a, M> {
    let catcher = opaque(mouse_area(container(blank().width(Fill).height(Fill)).style(scrim)).on_press(on_dismiss));
    // The edge is a rule on the drawer's own surface: Iced leaves a translucent border see-through,
    // and the content below would shine through it.
    let side = container(sidebar).width(size::sidebar() - LINE).height(Fill).padding(space::s());
    let panel = container(row![side, rule::vertical(LINE).style(style::rule)]).height(Fill).style(|t| {
        let p = palette(t);
        // The menu shadow falls sideways here: the drawer's edge is vertical.
        let shadow = Shadow { offset: Vector::new(space::m(), 0.0), ..p.shadow };
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
    let top = column![switcher.into(), blank().height(space::s())].extend(items).spacing(SIDEBAR_GAP);
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

/// The top row of the sidebar, like Linear's workspace menu: the logo, the name of `current`
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
///
/// Where the window has the band of [`titlebar`] ([`has_band`], macOS), the panel button lies there
/// ([`TitleButtons::sidebar`]) and the head leaves it out.
pub fn sidebar_head<'a, M: Clone + 'a>(
    mode: SidebarMode,
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: M,
) -> Element<'a, M> {
    match mode {
        SidebarMode::Expanded => switcher_row(current, anchor, on, (!has_band()).then_some(on_toggle)),
        SidebarMode::Rail => {
            // The logo on a full-width row, as the other rows of the rail.
            let logo = button(center_x(logo()))
                .width(Fill)
                .height(switcher_height())
                .padding(0)
                .style(style::button::sidebar_item(false))
                .on_press(on(Switcher::Toggle));
            let mut head = column![
                widgets::anchored(anchor, widgets::tooltip_right(logo, app_name(current), Trail::None)),
                widgets::icon_button(Icon::Search, tr("Search"), Some(on(Switcher::Search))),
                widgets::boxed_button(Icon::SquarePen, tr("New"), Some(on(Switcher::New))),
            ];
            if !has_band() {
                head = head.push(widgets::icon_button(Icon::PanelLeft, tr("Expand sidebar"), Some(on_toggle)));
            }
            head.spacing(SIDEBAR_GAP).align_x(Alignment::Center).into()
        }
    }
}

/// `content` centred across the full width, at its own height.
fn center_x<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content).center_x(Fill).center_y(Fill).into()
}

/// The app logo, 20 × 20: the design's own mark (`logo.mark`, an SVG from the pack) if it has one,
/// otherwise the suite's: a white "T" on the accent colour, radius 4.
fn logo<'a, M: 'a>() -> Element<'a, M> {
    if let Some(mark) = design::active().logo.mark.clone() {
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

/// The switcher row of [`app_switcher`], with the panel button at its end when `on_toggle` is given.
fn switcher_row<'a, M: Clone + 'a>(
    current: AppId,
    anchor: &Anchor,
    on: fn(Switcher) -> M,
    on_toggle: Option<M>,
) -> Element<'a, M> {
    let name = row![
        logo(),
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
    fn titlebar_wraps_any_content() {
        for surface in [Surface::Window, Surface::Panel] {
            let filled = Cover::Filled(Surface::Panel);
            for cover in [Cover::Nothing, Cover::Dialog, Cover::Drawer, Cover::Panel, filled] {
                let buttons = || TitleButtons::none().sidebar("Collapse sidebar", Msg::Close).settings(Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, TitleButtons::none(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.0, surface, cover, buttons(), Msg::Close);
                let _: Element<'_, Msg> = titlebar(blank(), 1.5, surface, cover, buttons(), Msg::Close);
            }
        }
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

    #[cfg(target_os = "macos")]
    #[test]
    fn the_lights_have_as_much_room_above_as_below() {
        // Halfway between the top edge and the inset panel under the band, and as far from the left.
        assert_eq!(LIGHTS, (TITLEBAR + layout::inset()) / 2.0);
        // The band's buttons sit on that line and stay inside the band.
        const { assert!(LIGHTS + BAND_BUTTON / 2.0 <= TITLEBAR) };
        assert_eq!(BAND_TRAILING + BAND_BUTTON / 2.0, LIGHTS, "the settings mirror the close button");
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
        assert_eq!(Surface::of(SizeClass::Wide), Surface::Window);
        assert_eq!(Surface::of(SizeClass::Medium), Surface::Window);
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
