//! Tray icon for macOS (menu bar) and Windows (notification area), built on tray-icon with
//! menus from muda. It draws the shared menu from `menu.rs`, like ksni does on Linux.
//!
//! Both systems want a native event loop on the main thread: macOS runs the status item on
//! NSApplication, Windows delivers the icon's messages to the thread that created it. So the
//! daemon hands its main thread to a [`Shell`], a winit event loop (winit already comes with
//! iced, works the same on both systems and lets us mark the process as a menu bar agent on
//! macOS), while tokio runs the service on its worker threads. The two sides only talk through
//! the loop's proxy:
//!
//! - the service sends [`Handle::start`] once (the daemon and the first snapshot; the icon is
//!   created then), [`Handle::show`] after every change, [`Handle::exit`] when it ends;
//! - muda reports clicks to a handler that forwards them into the loop, which maps the entry to
//!   its [`Action`] and runs it on the [`Daemon`].
//!
//! Every native object (icon, menu) is created and changed on the main thread only. The icon
//! follows the chosen design pack (task L.3): every snapshot names it, and the image is drawn
//! again when it changes.
//!
//! What waits in the inbox ([`Waiting`], task E3.3b) shows as the inbox section at the top of the
//! shared menu and as the count in the tooltip ("Torro Workplace · 3 for you"). Neither system
//! has a highlight like ksni's `NeedsAttention` for a status item, so urgency stays in the menu
//! and the notifications; a text next to the icon (macOS `set_title`) would take menu bar room
//! for every notice and is left out.

use std::fmt;
use std::sync::{Arc, Mutex};

use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use super::icon;
use super::menu::{self, Action, Item, Waiting};
use crate::core::Snapshot;
use crate::daemon::Daemon;

/// Side of the icon image, in pixels.
///
/// macOS shows the image 22 pt high and draws one pixel per point unless the image is taller;
/// 44 px is sharp on Retina. Windows scales the image into its slot (16 px at 100 %).
#[cfg(target_os = "macos")]
const ICON_SIDE: u32 = 44;
#[cfg(windows)]
const ICON_SIDE: u32 = 32;
/// Side of the symbol (the grid, or a design pack's own) in the middle of the image. On macOS the rest is a transparent border, so
/// the grid stands about 16 pt high like the system's own symbols; Windows uses the whole image.
#[cfg(target_os = "macos")]
const ICON_GRID: u32 = 36;
#[cfg(windows)]
const ICON_GRID: u32 = 32;

/// Errors of the tray icon.
#[derive(Debug)]
pub enum TrayError {
    /// The event loop could not be created or run.
    EventLoop(winit::error::EventLoopError),
    /// The icon image was refused.
    Icon(tray_icon::BadIcon),
    /// The system did not take the icon.
    Tray(tray_icon::Error),
    /// The menu could not be put together.
    Menu(tray_icon::menu::Error),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrayError::EventLoop(err) => write!(f, "cannot run the tray event loop: {err}"),
            TrayError::Icon(err) => write!(f, "cannot use the tray icon image: {err}"),
            TrayError::Tray(err) => write!(f, "cannot show the tray icon: {err}"),
            TrayError::Menu(err) => write!(f, "cannot build the tray menu: {err}"),
        }
    }
}

impl std::error::Error for TrayError {}

/// What reaches the event loop from other threads.
enum Event {
    /// The service is up: remember the daemon and show the icon with this menu.
    Start(Arc<Daemon>, Snapshot),
    /// The state changed: rebuild the menu.
    Show(Snapshot),
    /// An entry of the menu was clicked.
    Menu(MenuEvent),
    /// The service ended: leave the loop.
    Exit,
    /// Does nothing but wake the loop once more (see [`App::show`]).
    Wake,
}

/// The way from the service (any thread) into the event loop. Cheap to clone.
///
/// The proxy sits behind a mutex because winit's proxy is `Send` on every system but `Sync`
/// only on some, and the handle is shared between tokio tasks and muda's click handler.
#[derive(Clone)]
pub struct Handle(Arc<Mutex<EventLoopProxy<Event>>>);

impl Handle {
    /// Shows the icon with the menu for `snapshot`; clicks go to `daemon` from now on. Call once,
    /// when the service is ready.
    pub fn start(&self, daemon: Arc<Daemon>, snapshot: Snapshot) {
        self.send(Event::Start(daemon, snapshot));
    }

    /// Rebuilds the menu for `snapshot`.
    pub fn show(&self, snapshot: Snapshot) {
        self.send(Event::Show(snapshot));
    }

    /// Ends the event loop, so [`Shell::run`] returns. Call when the service has ended.
    pub fn exit(&self) {
        self.send(Event::Exit);
    }

    /// Hands `event` to the loop. A loop that already ended has nothing left to show, so a
    /// failure is ignored.
    fn send(&self, event: Event) {
        if let Ok(proxy) = self.0.lock() {
            let _ = proxy.send_event(event);
        }
    }
}

/// The event loop that owns the tray icon. Create it on the main thread, before the service
/// starts, and run it there.
pub struct Shell {
    event_loop: EventLoop<Event>,
    handle: Handle,
}

impl Shell {
    /// Prepares the loop. On macOS the process becomes a menu bar agent: no Dock icon, no app
    /// menu, and starting it does not take the focus from the app in front. Fails when not called
    /// on the main thread or when called twice.
    pub fn new() -> Result<Shell, TrayError> {
        let mut builder = EventLoop::<Event>::with_user_event();
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder
                .with_activation_policy(ActivationPolicy::Accessory)
                .with_default_menu(false)
                .with_activate_ignoring_other_apps(false);
        }
        let event_loop = builder.build().map_err(TrayError::EventLoop)?;
        let handle = Handle(Arc::new(Mutex::new(event_loop.create_proxy())));
        let clicks = handle.clone();
        MenuEvent::set_event_handler(Some(move |event| clicks.send(Event::Menu(event))));
        // A click on the icon itself only opens the menu. Without a handler tray-icon would keep
        // every such event (also hovering) in a channel nobody reads, for as long as the service
        // runs.
        TrayIconEvent::set_event_handler(Some(|_| {}));
        Ok(Shell { event_loop, handle })
    }

    /// The way in for the service.
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Runs the loop on this (the main) thread until [`Handle::exit`].
    pub fn run(self) -> Result<(), TrayError> {
        self.event_loop.set_control_flow(ControlFlow::Wait);
        let mut app =
            App { handle: self.handle, daemon: None, tray: None, design: None, actions: Vec::new(), generation: 0 };
        self.event_loop.run_app(&mut app).map_err(TrayError::EventLoop)
    }
}

/// The state on the main thread: the icon and what its current entries do.
struct App {
    handle: Handle,
    daemon: Option<Arc<Daemon>>,
    tray: Option<TrayIcon>,
    /// The design pack the icon shows (`Snapshot::design`); `None` until the icon exists.
    design: Option<String>,
    /// The actions of the current menu; an entry's id holds its index here.
    actions: Vec<Action>,
    /// Counts the menus built so far. Ids carry it, so a click that arrives from a menu already
    /// replaced does nothing instead of running another entry.
    generation: u64,
}

impl App {
    /// Shows the menu for `snapshot`, creating the icon the first time.
    fn show(&mut self, snapshot: &Snapshot) {
        self.generation += 1;
        let (native, actions) = match native_menu(&menu::build(snapshot), self.generation) {
            Ok(built) => built,
            Err(err) => {
                eprintln!("torroworkplace: {err}");
                return;
            }
        };
        self.actions = actions;
        let tooltip = tooltip(Waiting::of(snapshot));
        match &self.tray {
            Some(tray) => {
                tray.set_menu(Some(Box::new(native)));
                if let Err(err) = tray.set_tooltip(Some(&tooltip)) {
                    eprintln!("torroworkplace: cannot update the tray tooltip: {err}");
                }
                self.follow_design(&snapshot.design);
            }
            None => match new_tray(native, &tooltip, &icon::Look::for_design(&snapshot.design)) {
                Ok(tray) => {
                    self.tray = Some(tray);
                    self.design = Some(snapshot.design.clone());
                    // macOS draws a new status item only on the next pass of the run loop; a
                    // second event makes sure that pass comes now and not with the next click.
                    self.handle.send(Event::Wake);
                }
                // The service keeps working without the icon; the next change tries again.
                Err(err) => eprintln!("torroworkplace: {err}; running without a tray icon for now"),
            },
        }
    }

    /// Draws the icon again when the snapshot names another design pack than the one it shows.
    /// A failure keeps the old image and is tried again with the next snapshot.
    fn follow_design(&mut self, design: &str) {
        let Some(tray) = &self.tray else { return };
        if self.design.as_deref() == Some(design) {
            return;
        }
        let image = match Icon::from_rgba(icon_image(&icon::Look::for_design(design)), ICON_SIDE, ICON_SIDE) {
            Ok(image) => image,
            Err(err) => return eprintln!("torroworkplace: {}", TrayError::Icon(err)),
        };
        // macOS keeps it a template, so the menu bar tints it for light and dark.
        #[cfg(target_os = "macos")]
        let set = tray.set_icon_templated(Some(image));
        #[cfg(windows)]
        let set = tray.set_icon(Some(image));
        match set {
            Ok(()) => self.design = Some(design.to_string()),
            Err(err) => eprintln!("torroworkplace: cannot change the tray icon: {err}"),
        }
    }

    /// Runs the action behind the clicked entry.
    fn click(&self, id: &MenuId) {
        let Some(index) = action_index(&id.0, self.generation) else { return };
        if let (Some(daemon), Some(action)) = (&self.daemon, self.actions.get(index)) {
            action.perform(daemon);
        }
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _window: WindowId, _event: WindowEvent) {}

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Start(daemon, snapshot) => {
                self.daemon = Some(daemon);
                self.show(&snapshot);
            }
            Event::Show(snapshot) => self.show(&snapshot),
            Event::Menu(event) => self.click(&event.id),
            Event::Exit => event_loop.exit(),
            Event::Wake => {}
        }
    }
}

/// The tooltip of the icon: the name and, when something waits, how much
/// ([`menu::waiting_line`]); never a title.
fn tooltip(waiting: Waiting) -> String {
    match menu::waiting_line(waiting) {
        line if line.is_empty() => APP_NAME.to_string(),
        line => format!("{APP_NAME} · {line}"),
    }
}

/// The name of the icon, as the notifications name the sender.
const APP_NAME: &str = crate::notify::APP_NAME;

/// Creates the icon with `menu`, `tooltip` and the image of `look`. A left click opens the menu on
/// both systems; on macOS the image is a template, so the menu bar tints it for light and dark.
fn new_tray(menu: Menu, tooltip: &str, look: &icon::Look) -> Result<TrayIcon, TrayError> {
    let image = Icon::from_rgba(icon_image(look), ICON_SIDE, ICON_SIDE).map_err(TrayError::Icon)?;
    let builder = TrayIconBuilder::new().with_tooltip(tooltip).with_menu(Box::new(menu)).with_menu_on_left_click(true);
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_templated(image);
    #[cfg(windows)]
    let builder = builder.with_icon(image);
    builder.build().map_err(TrayError::Tray)
}

/// The icon of `look` as RGBA, [`ICON_SIDE`] pixels a side with the symbol centred in it.
fn icon_image(look: &icon::Look) -> Vec<u8> {
    look.rgba(ICON_GRID, (ICON_SIDE - ICON_GRID) / 2)
}

/// The native menu for `items`, and the actions its entries stand for, in id order.
fn native_menu(items: &[Item], generation: u64) -> Result<(Menu, Vec<Action>), TrayError> {
    let menu = Menu::new();
    let mut actions = Vec::new();
    for item in native_items(items, generation, &mut actions)? {
        menu.append(item.as_ref()).map_err(TrayError::Menu)?;
    }
    Ok((menu, actions))
}

fn native_items(
    items: &[Item],
    generation: u64,
    actions: &mut Vec<Action>,
) -> Result<Vec<Box<dyn IsMenuItem>>, TrayError> {
    items.iter().map(|item| native_item(item, generation, actions)).collect()
}

/// One native item. An entry with an action gets the id `<generation>:<index in actions>`.
fn native_item(item: &Item, generation: u64, actions: &mut Vec<Action>) -> Result<Box<dyn IsMenuItem>, TrayError> {
    Ok(match item {
        Item::Entry { label, action: Some(action) } => {
            let id = entry_id(generation, actions.len());
            actions.push(action.clone());
            Box::new(MenuItem::with_id(id, escape(label), true, None))
        }
        Item::Entry { label, action: None } => Box::new(MenuItem::new(escape(label), false, None)),
        Item::Submenu { label, items } => {
            let submenu = Submenu::new(escape(label), true);
            for child in native_items(items, generation, actions)? {
                submenu.append(child.as_ref()).map_err(TrayError::Menu)?;
            }
            Box::new(submenu)
        }
        Item::Separator => Box::new(PredefinedMenuItem::separator()),
    })
}

/// The id of the entry whose action sits at `index`.
fn entry_id(generation: u64, index: usize) -> String {
    format!("{generation}:{index}")
}

/// The index in the actions of the menu `generation` for the entry `id`; `None` for an entry of
/// an older menu or an id that is not ours.
fn action_index(id: &str, generation: u64) -> Option<usize> {
    let (built, index) = id.split_once(':')?;
    if built.parse::<u64>().ok()? != generation {
        return None;
    }
    index.parse().ok()
}

/// muda reads `&` as the mark of a keyboard shortcut and drops it; `&&` shows one `&`, so a
/// template called "Mail & Chat" keeps its name.
fn escape(label: &str) -> String {
    label.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_ids_find_their_action() {
        assert_eq!(action_index(&entry_id(3, 7), 3), Some(7));
        assert_eq!(action_index(&entry_id(3, 0), 3), Some(0));
    }

    #[test]
    fn clicks_from_an_older_menu_do_nothing() {
        assert_eq!(action_index(&entry_id(2, 7), 3), None);
    }

    #[test]
    fn foreign_ids_do_nothing() {
        for id in ["", "7", "x:1", "3:", "3:x", "3:-1"] {
            assert_eq!(action_index(id, 3), None, "{id:?}");
        }
    }

    #[test]
    fn ampersands_survive_muda() {
        assert_eq!(escape("Mail & Chat"), "Mail && Chat");
        assert_eq!(escape("Settings…"), "Settings…");
    }

    #[test]
    fn the_tooltip_counts_what_waits() {
        assert_eq!(tooltip(Waiting::default()), "Torro Workplace");
        let waiting = Waiting { for_me: 3, urgent: 0 };
        assert_eq!(tooltip(waiting), format!("Torro Workplace · {}", menu::waiting_line(waiting)));
    }

    #[test]
    fn icon_image_is_square_rgba() {
        assert_eq!(icon_image(&icon::Look::standard()).len(), (ICON_SIDE * ICON_SIDE * 4) as usize);
    }
}
