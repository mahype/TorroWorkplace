//! macOS window details that Iced and winit cannot set (task U.11): where the traffic lights sit.
//!
//! The apps draw their own title band (`torro_gui::shell::titlebar`) with buttons beside the
//! traffic lights. macOS puts the lights close to the top edge of a window without a title bar;
//! Claude, Slack and Safari give them more room. [`place_traffic_lights`] moves them, as Electron's
//! `trafficLightPosition` does: it sets the three buttons' frames and, because AppKit lays out the
//! title bar again after a resize, a change of focus or the end of full screen, sets them again after
//! each of these.
//!
//! This crate is the only place in the suite with `unsafe`: four calls into AppKit, each marked
//! with why it is sound. On other systems [`place_traffic_lights`] does nothing.

use raw_window_handle::HasWindowHandle;

/// Where the traffic lights go, in points of the window: the centre of the first (close) button,
/// measured from the left and the top edge. The other two follow at the system's own pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// Why the traffic lights could not be moved; the window keeps them where macOS put them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The window has no native handle (yet).
    NoHandle,
    /// The handle is not an AppKit view.
    NotAppKit,
    /// Called off the main thread, where AppKit must not be touched.
    NotMainThread,
    /// The view is not in a window.
    NoWindow,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::NoHandle => "the window has no native handle",
            Error::NotAppKit => "the window is not an AppKit window",
            Error::NotMainThread => "not on the main thread",
            Error::NoWindow => "the view is not in a window",
        })
    }
}

impl std::error::Error for Error {}

/// Moves the traffic lights of `window` to `at` now and after every new layout of its title bar
/// (resize, focus, end of full screen); in full screen they stay where macOS puts them. Call it once
/// per window on the main thread, as Iced's `window::run` does. Does nothing on other systems.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn place_traffic_lights<W: HasWindowHandle + ?Sized>(window: &W, at: Position) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    {
        mac::place_traffic_lights(window, at)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::Weak;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSView, NSWindow, NSWindowButton, NSWindowDidBecomeKeyNotification, NSWindowDidEndLiveResizeNotification,
        NSWindowDidExitFullScreenNotification, NSWindowDidResignKeyNotification, NSWindowDidResizeNotification,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName, NSPoint};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{Error, Position};

    pub(super) fn place_traffic_lights<W: HasWindowHandle + ?Sized>(window: &W, at: Position) -> Result<(), Error> {
        MainThreadMarker::new().ok_or(Error::NotMainThread)?;
        let handle = window.window_handle().map_err(|_| Error::NoHandle)?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return Err(Error::NotAppKit) };
        // SAFETY: an AppKit handle's `ns_view` points to the window's content view, an `NSView` that
        // lives as long as the borrowed handle; we are on the main thread (checked above), the only
        // thread AppKit views may be used on.
        #[allow(unsafe_code)]
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        let ns_window = view.window().ok_or(Error::NoWindow)?;
        place(&ns_window, at);

        // AppKit puts the buttons back where it wants them whenever it lays out the title bar.
        // The block holds the window weakly: it neither keeps a closed window alive nor acts on it.
        let weak = Weak::from_retained(&ns_window);
        let again = RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(ns_window) = weak.load() {
                place(&ns_window, at);
            }
        });
        // SAFETY: the notification names are AppKit's constant strings, set before any code runs.
        #[allow(unsafe_code)]
        let names: [&NSNotificationName; 5] = unsafe {
            [
                NSWindowDidResizeNotification,
                NSWindowDidEndLiveResizeNotification,
                NSWindowDidExitFullScreenNotification,
                NSWindowDidBecomeKeyNotification,
                NSWindowDidResignKeyNotification,
            ]
        };
        let object: &AnyObject = &ns_window;
        let center = NSNotificationCenter::defaultCenter();
        for name in names {
            // SAFETY: `object` is the window, the sender of these notifications. Without a queue the
            // block runs on the posting thread, which for window notifications is the main thread
            // the block's AppKit calls need.
            #[allow(unsafe_code)]
            let observer =
                unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), Some(object), None, &again) };
            // Registered for the life of the process: a window of the suite is its app.
            std::mem::forget(observer);
        }
        Ok(())
    }

    /// Sets the three buttons to `at`, at the pitch macOS gave them, unless the window is in full
    /// screen (the lights then sit in the title bar that slides in from the top).
    fn place(window: &NSWindow, at: Position) {
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let buttons = [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton]
            .map(|which| window.standardWindowButton(which));
        let [Some(close), Some(minimize), Some(zoom)] = buttons else { return };
        // SAFETY: the parent is returned retained and used at once, while `close` – which its parent
        // holds – is alive; all on the main thread (the caller's, see `place_traffic_lights`).
        #[allow(unsafe_code)]
        let Some(parent) = (unsafe { close.superview() }) else { return };
        let pitch = minimize.frame().origin.x - close.frame().origin.x;
        let height = window.frame().size.height;
        for (index, button) in [close, minimize, zoom].iter().enumerate() {
            let size = button.frame().size;
            // In window coordinates the origin is bottom left; the centre minus half the size is the
            // frame's origin in the parent, flipped or not.
            let centre = NSPoint::new(at.x + index as f64 * pitch, height - at.y);
            let centre = parent.convertPoint_fromView(centre, None);
            button.setFrameOrigin(NSPoint::new(centre.x - size.width / 2.0, centre.y - size.height / 2.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_is_missing() {
        assert_eq!(Error::NotAppKit.to_string(), "the window is not an AppKit window");
        assert_eq!(Error::NotMainThread.to_string(), "not on the main thread");
    }
}
