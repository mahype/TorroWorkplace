//! Zoom like a browser (task Z.1): Ctrl + Plus makes every window of the suite larger, Ctrl +
//! Minus smaller, Ctrl + 0 goes back to 100 %. The value is one suite-wide setting
//! (`torro_core::settings::Zoom`, `Settings::zoom`), so tiled windows stay alike; every app follows
//! a change made elsewhere, like the appearance.
//!
//! The whole window scales with Iced's `scale_factor`: text, spacing and layout grow together, and
//! the sizes Iced reports (`window::resize_events`, `responsive`) are logical pixels *after* the
//! zoom. So the size classes switch the way a browser's breakpoints do: at 150 % a window 1440
//! wide is 960 logical pixels, the medium layout.
//!
//! An app keeps one [`Zoom`] and wires it up in five places:
//!
//! ```ignore
//! use torro_gui::zoom::{self, Zoom};
//!
//! // new: no socket with `--demo`, so the zoom stays in the process and nothing is saved.
//! let zoom = Zoom::for_source(&source); // or Zoom::new(socket)
//! let task = Task::batch([zoom.fetch().map(Message::Zoom), …]);
//! // main.rs: `.scale_factor(App::scale_factor)` with `fn scale_factor(&self) -> f32 { self.zoom.factor() }`.
//! // subscription: always, also in glance views and with a text field focused.
//! all.push(self.zoom.subscription().map(Message::Zoom));
//! // update: a size kept from the window events is in the old logical pixels; rescale it at
//! // once and ask the window for the exact one.
//! Message::Zoom(message) => {
//!     let before = self.zoom.factor();
//!     let task = self.zoom.update(message).map(Message::Zoom);
//!     if self.zoom.factor() == before {
//!         return task;
//!     }
//!     self.size = zoom::rescale(self.size, before, self.zoom.factor());
//!     let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
//!     return Task::batch([task, size]);
//! }
//! // Tick: `self.zoom.fetch().map(Message::Zoom)`, so a change in another window arrives.
//! // view: the short "110%" after a change, over everything else; and the guard that keeps Ctrl + Plus
//! // from typing "+" into a focused text field (Z.2).
//! widgets::ctrl_guard(self.zoom.view(content))
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{Space, container, stack};
use iced::{Border, Element, Fill, Size, Subscription, Task, Theme, keyboard};
use torro_core::i18n::trf;
use torro_core::settings;
use torro_ipc::{Request, Response, SettingKey};

use crate::source::Source;
use crate::theme::{size, space};
use crate::{style, typo};

/// How long the new value shows after a change, as in a browser.
pub const BADGE: Duration = Duration::from_millis(1500);
/// How often the badge checks whether its time is up. Only runs while it shows.
const BADGE_CLOCK: Duration = Duration::from_millis(250);

/// What a key or a button asks of the zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// One step larger (Ctrl + Plus, Ctrl + `=`, Ctrl + keypad Plus).
    In,
    /// One step smaller (Ctrl + Minus).
    Out,
    /// Back to 100 % (Ctrl + 0).
    Reset,
}

impl Step {
    /// The zoom after this step from `zoom`; at the ends it stays.
    pub fn apply(self, zoom: settings::Zoom) -> settings::Zoom {
        match self {
            Step::In => zoom.larger(),
            Step::Out => zoom.smaller(),
            Step::Reset => settings::Zoom::DEFAULT,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A zoom key or a button of the control centre.
    Step(Step),
    /// The stored zoom, asked by [`Zoom::fetch`] when the window had made this many changes
    /// ([`Zoom::changes`]); an answer from before a later change is stale and dropped.
    Stored(u64, Result<settings::Zoom, String>),
    /// The core answered a save.
    Saved(Result<(), String>),
    /// The badge's clock.
    Clock(Instant),
}

/// The zoom of one window: the level it draws at, whether the badge shows, and the save to the
/// core in flight.
#[derive(Debug, Clone)]
pub struct Zoom {
    level: settings::Zoom,
    /// The core's socket; `None` with `--demo`, where the zoom only lives in the process.
    socket: Option<PathBuf>,
    /// Since when the badge shows the new level, if it does.
    badge: Option<Instant>,
    /// Counts the changes made in this window, so a fetch from before one is not applied.
    changes: u64,
    /// A save is on its way to the core.
    saving: bool,
    /// The level changed while a save was on its way; it is sent once that one is answered, so
    /// the saves reach the core in order and the last one wins.
    unsaved: bool,
}

impl Zoom {
    /// A window at 100 % until [`Zoom::fetch`] brings the stored level. `socket` is the core's
    /// (`torro_ipc::socket_path()`, `PimClient::socket`); `None` keeps every change in this
    /// process (`--demo`).
    pub fn new(socket: Option<PathBuf>) -> Zoom {
        Zoom { level: settings::Zoom::DEFAULT, socket, badge: None, changes: 0, saving: false, unsaved: false }
    }

    /// [`Zoom::new`] for an app on `source`: the core's socket, or none for demo data.
    pub fn for_source(source: &Source) -> Zoom {
        match source {
            Source::Core(client) => Zoom::new(Some(client.socket.clone())),
            Source::Demo(_) => Zoom::new(None),
        }
    }

    /// The level the window draws at.
    pub fn level(&self) -> settings::Zoom {
        self.level
    }

    /// The factor for the app builder's `.scale_factor(…)`.
    pub fn factor(&self) -> f32 {
        self.level.factor()
    }

    /// Whether the badge with the new level shows (for tests and for apps that want to know).
    pub fn badge_shows(&self) -> bool {
        self.badge.is_some()
    }

    /// Asks the core for the stored level (`Request::Settings`). Call at start and on every tick,
    /// so a change made in another window arrives. Without a socket (`--demo`) it does nothing.
    pub fn fetch(&self) -> Task<Message> {
        let Some(socket) = self.socket.clone() else { return Task::none() };
        let changes = self.changes;
        Task::perform(
            async move {
                match torro_ipc::client::call(&socket, &Request::Settings).await {
                    Ok(Response::Settings { settings }) => Ok(settings.zoom),
                    Ok(other) => Err(format!("unexpected answer: {other:?}")),
                    Err(err) => Err(err.to_string()),
                }
            },
            move |stored| Message::Stored(changes, stored),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Step(step) => {
                // The badge shows even at the ends, so the key visibly did something.
                self.badge = Some(Instant::now());
                let level = step.apply(self.level);
                if level == self.level {
                    return Task::none();
                }
                self.level = level;
                self.changes += 1;
                return self.save();
            }
            // While this window's own change is on its way, the stored level is the old one.
            Message::Stored(changes, Ok(level)) if changes == self.changes && !self.saving && !self.unsaved => {
                self.level = level;
            }
            // Stale, or the core is not answering: keep the level we have.
            Message::Stored(..) => {}
            Message::Saved(_) => {
                self.saving = false;
                if self.unsaved {
                    self.unsaved = false;
                    return self.save();
                }
            }
            Message::Clock(now) => {
                if self.badge.is_some_and(|since| now.saturating_duration_since(since) >= BADGE) {
                    self.badge = None;
                }
            }
        }
        Task::none()
    }

    /// Sends the level to the core, or remembers to once the save in flight is answered. A failed
    /// save keeps the level in this window; the next fetch brings back the stored one.
    fn save(&mut self) -> Task<Message> {
        let Some(socket) = self.socket.clone() else { return Task::none() };
        if self.saving {
            self.unsaved = true;
            return Task::none();
        }
        self.saving = true;
        let request = Request::SetSetting { key: SettingKey::Zoom, value: serde_json::json!(self.level.factor()) };
        Task::perform(
            async move {
                match torro_ipc::client::call(&socket, &request).await {
                    Ok(Response::Ok) => Ok(()),
                    Ok(Response::Error { message, .. }) => Err(message),
                    Ok(other) => Err(format!("unexpected answer: {other:?}")),
                    Err(err) => Err(err.to_string()),
                }
            },
            Message::Saved,
        )
    }

    /// The zoom keys, in every state of the window, also while a text field has the focus (the
    /// keys mean nothing there); and the badge's clock while it shows.
    pub fn subscription(&self) -> Subscription<Message> {
        let keys = iced::event::listen_with(zoom_key);
        if self.badge.is_none() {
            return keys;
        }
        Subscription::batch([keys, iced::time::every(BADGE_CLOCK).map(Message::Clock)])
    }

    /// `content` with the badge over it: the new level ("110%") in a small pill at the bottom
    /// centre, for [`BADGE`] after a change. Always the same two layers, so the window keeps its
    /// state (scroll positions, focus) when the badge comes and goes.
    pub fn view<'a, M: 'a>(&self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        let badge: Element<'a, M> = match self.badge {
            Some(_) => badge(self.level),
            None => Space::new().into(),
        };
        let layer = container(badge).width(Fill).height(Fill).center_x(Fill).align_bottom(Fill).padding(space::XL);
        stack![content.into(), layer].width(Fill).height(Fill).into()
    }
}

/// `zoom` as people read it: "110%" (German "110 %").
pub fn label(zoom: settings::Zoom) -> String {
    trf("{percent}%", &[("percent", &zoom.percent())])
}

/// The pill with the level: `menu` surface with its border and shadow, fully rounded, the value in
/// tabular `CAPTION`.
fn badge<'a, M: 'a>(zoom: settings::Zoom) -> Element<'a, M> {
    container(typo::text(typo::CAPTION.tabular(), label(zoom)))
        .center_y(size::CONTROL_SM)
        .padding([0.0, space::M])
        .style(|theme: &Theme| container::Style {
            border: Border { radius: (size::CONTROL_SM / 2.0).into(), ..style::container::menu(theme).border },
            ..style::container::menu(theme)
        })
        .into()
}

/// Whether a key press is a zoom key: Ctrl (⌘ on macOS) with Plus, `=` (Plus without Shift on US
/// keyboards) or keypad Plus; with Minus; or with 0. Alt must not be held: Windows reports AltGr
/// as Ctrl + Alt, and AltGr + 0 types `}` on German keyboards.
///
/// Iced reports the key without its modifiers, so Shift does not change it (Ctrl + Shift + `=`
/// is still `=`).
pub fn step_for(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Step> {
    if !modifiers.command() || modifiers.alt() {
        return None;
    }
    match key.as_ref() {
        keyboard::Key::Character("+" | "=") => Some(Step::In),
        keyboard::Key::Character("-") => Some(Step::Out),
        keyboard::Key::Character("0") => Some(Step::Reset),
        _ => None,
    }
}

/// The zoom keys as messages. Captured events count too: a focused text field captures every key
/// that types a character, Ctrl + Plus included.
fn zoom_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    step_for(&key, modifiers).map(Message::Step)
}

/// `size` in logical pixels at the factor `from`, measured again at the factor `to`. A window
/// keeps its physical size when the zoom changes, so its logical size shrinks as the zoom grows.
/// For apps that keep the size from `window::resize_events`, which do not fire on a zoom change.
///
/// Floating point drifts a little over many steps (1024 may come back as 1023.9999, one pixel
/// short of the wide layout), so apps also ask the window for its size after a change
/// (`iced::window::latest().and_then(iced::window::size)`); this keeps the layout right until the
/// answer is there, and in tests.
pub fn rescale(size: Size, from: f32, to: f32) -> Size {
    if to <= 0.0 || from == to {
        return size;
    }
    size * (from / to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Key, Modifiers};

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn ctrl_with_plus_minus_and_zero_are_the_zoom_keys() {
        // The platform's command key: Ctrl, on macOS Cmd (what `Modifiers::command` asks for).
        let ctrl = Modifiers::COMMAND;
        assert_eq!(step_for(&ch("+"), ctrl), Some(Step::In));
        assert_eq!(step_for(&ch("="), ctrl), Some(Step::In));
        assert_eq!(step_for(&ch("="), ctrl | Modifiers::SHIFT), Some(Step::In), "Ctrl + Shift + = on US keyboards");
        assert_eq!(step_for(&ch("-"), ctrl), Some(Step::Out));
        assert_eq!(step_for(&ch("0"), ctrl), Some(Step::Reset));
    }

    #[test]
    fn other_keys_are_not_zoom_keys() {
        // Without Ctrl the keys type text.
        for c in ["+", "-", "0", "="] {
            assert_eq!(step_for(&ch(c), Modifiers::empty()), None, "{c}");
            assert_eq!(step_for(&ch(c), Modifiers::SHIFT), None, "{c}");
        }
        // AltGr on Windows is Ctrl + Alt.
        assert_eq!(step_for(&ch("0"), Modifiers::CTRL | Modifiers::ALT), None);
        assert_eq!(step_for(&ch("1"), Modifiers::CTRL), None);
        assert_eq!(step_for(&Key::Named(keyboard::key::Named::Enter), Modifiers::CTRL), None);
    }

    #[test]
    fn steps_walk_the_browser_scale() {
        let mut zoom = Zoom::new(None);
        let _ = zoom.update(Message::Step(Step::In));
        assert_eq!(zoom.level().percent(), 110);
        for _ in 0..20 {
            let _ = zoom.update(Message::Step(Step::In));
        }
        assert_eq!(zoom.level(), settings::Zoom::MAX, "stops at 200 %");
        let _ = zoom.update(Message::Step(Step::Out));
        assert_eq!(zoom.level().percent(), 175);
        let _ = zoom.update(Message::Step(Step::Reset));
        assert_eq!(zoom.level(), settings::Zoom::DEFAULT);
        assert_eq!(zoom.factor(), 1.0);
    }

    #[test]
    fn the_badge_shows_after_a_step_and_goes_after_its_time() {
        let mut zoom = Zoom::new(None);
        assert!(!zoom.badge_shows());
        let _ = zoom.update(Message::Step(Step::Out));
        assert!(zoom.badge_shows());
        let shown = zoom.badge.expect("shows");
        let _ = zoom.update(Message::Clock(shown + BADGE / 2));
        assert!(zoom.badge_shows(), "still within its time");
        let _ = zoom.update(Message::Clock(shown + BADGE));
        assert!(!zoom.badge_shows());
        // At the end of the scale a key still shows the value.
        let mut max = Zoom { level: settings::Zoom::MAX, ..Zoom::new(None) };
        let _ = max.update(Message::Step(Step::In));
        assert!(max.badge_shows());
    }

    #[test]
    fn a_stored_level_is_taken_unless_this_window_changed_it_since() {
        let mut zoom = Zoom::new(None);
        let _ = zoom.update(Message::Stored(0, Ok(settings::Zoom::MAX)));
        assert_eq!(zoom.level(), settings::Zoom::MAX, "a change from another window");
        assert!(!zoom.badge_shows(), "no badge for a change made elsewhere");
        let _ = zoom.update(Message::Stored(0, Err("core is not running".into())));
        assert_eq!(zoom.level(), settings::Zoom::MAX, "an error keeps the level");

        let _ = zoom.update(Message::Step(Step::Reset));
        let _ = zoom.update(Message::Stored(0, Ok(settings::Zoom::MIN)));
        assert_eq!(zoom.level(), settings::Zoom::DEFAULT, "asked before the change: stale");
    }

    #[test]
    fn saves_go_out_one_at_a_time_and_the_last_level_wins() {
        let mut zoom = Zoom::new(Some(PathBuf::from("/nonexistent/torro.sock")));
        let first = zoom.update(Message::Step(Step::In));
        assert_eq!(first.units(), 1, "the first change is saved at once");
        assert!(zoom.saving);
        let second = zoom.update(Message::Step(Step::In));
        assert_eq!(second.units(), 0, "the second waits for the first answer");
        let stale = zoom.changes;
        let _ = zoom.update(Message::Stored(stale, Ok(settings::Zoom::MIN)));
        assert_eq!(zoom.level().percent(), 125, "the stored level is old while saving");
        let again = zoom.update(Message::Saved(Ok(())));
        assert_eq!(again.units(), 1, "then the latest level goes out");
        let done = zoom.update(Message::Saved(Err("core is not running".into())));
        assert_eq!(done.units(), 0);
        assert!(!zoom.saving && !zoom.unsaved);
        assert_eq!(zoom.level().percent(), 125, "a failed save keeps the level here");
    }

    #[test]
    fn the_demo_saves_nothing() {
        let mut zoom = Zoom::new(None);
        assert_eq!(zoom.update(Message::Step(Step::In)).units(), 0);
        assert_eq!(zoom.fetch().units(), 0);
        assert_eq!(zoom.level().percent(), 110, "but the window zooms");
    }

    #[test]
    fn a_window_is_smaller_in_logical_pixels_when_zoomed_in() {
        let wide = Size::new(1440.0, 900.0);
        let zoomed = rescale(wide, 1.0, 1.5);
        assert_eq!(zoomed, Size::new(960.0, 600.0));
        assert_eq!(rescale(zoomed, 1.5, 1.0), wide);
        assert_eq!(rescale(wide, 1.0, 1.0), wide);
        assert_eq!(rescale(wide, 1.0, 0.0), wide, "a broken factor changes nothing");
        // Step by step, as the keys do: close enough until the window reports its size.
        let near = |a: Size, b: Size| (a.width - b.width).abs() < 0.01 && (a.height - b.height).abs() < 0.01;
        let stepped = [1.0, 1.1, 1.25, 1.5].windows(2).fold(wide, |size, pair| rescale(size, pair[0], pair[1]));
        assert!(near(stepped, Size::new(960.0, 600.0)), "{stepped:?}");
    }

    #[test]
    fn the_label_is_the_percentage() {
        assert_eq!(label(settings::Zoom::nearest(1.1)), trf("{percent}%", &[("percent", &110)]));
        let zoom = Zoom { badge: Some(Instant::now()), ..Zoom::new(None) };
        let _view: Element<'_, ()> = zoom.view(Space::new());
        let _hidden: Element<'_, ()> = Zoom::new(None).view(Space::new());
    }
}
