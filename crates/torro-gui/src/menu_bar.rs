//! The menu bar of an app on macOS with "Edit › Copy" (issue #894): ⌘C copies what the app says is
//! selected, through the menu.
//!
//! Why a menu: macOS hands a key with ⌘ to the menu bar before the window sees it. The menu winit
//! puts up has no "Copy", and a ⌘C that does not reach the window never reaches the text either, so
//! a marked text could not be copied. The item is enabled only while the app has a selection
//! ([`set_copy_enabled`]); a disabled item does not take its key, so ⌘C then goes to the window as
//! before (a search field copies its own selection). A new menu bar replaces winit's, so the
//! app menu brings back its "Hide", "Hide others", "Show all" and "Quit" ("About" and "Services" are
//! left out).
//!
//! There is no "Select all": ⌘A selects the mail list. On Linux and Windows the keys reach the
//! window, and every function here does nothing.
//!
//! ```ignore
//! // Start: torro_gui::menu_bar::install(),
//! // Subscription: menu_bar::copy_pressed().map(|()| Message::CopySelection),
//! // Update, when the selection came or went: menu_bar::set_copy_enabled(has_selection),
//! ```

/// The id of "Copy" in the menu events.
#[cfg(target_os = "macos")]
const COPY: &str = "torro.copy";

#[cfg(target_os = "macos")]
thread_local! {
    /// The menu bar and its "Copy", kept alive on the main thread: muda's items are not `Send`, and
    /// the menu would go once dropped.
    static INSTALLED: std::cell::RefCell<Option<(muda::Menu, muda::MenuItem)>> = const { std::cell::RefCell::new(None) };
}

/// The task that puts up the menu bar on macOS, "Copy" disabled; nothing elsewhere. If muda fails,
/// the error goes to stderr and the window keeps winit's menu.
pub fn install<T: Send + 'static>() -> iced::Task<T> {
    #[cfg(target_os = "macos")]
    {
        // The menu bar belongs to the main thread, where `window::run` runs.
        iced::window::latest()
            .and_then(|id| {
                iced::window::run(id, |_| match build() {
                    Ok(installed) => INSTALLED.with(|cell| *cell.borrow_mut() = Some(installed)),
                    Err(err) => eprintln!("cannot put up the menu bar: {err}"),
                })
            })
            .discard()
    }
    #[cfg(not(target_os = "macos"))]
    {
        iced::Task::none()
    }
}

/// The menu bar: the app menu (Hide, Hide others, Show all, Quit) and "Edit" with "Copy", disabled
/// (⌘C: `META` is Command on macOS).
#[cfg(target_os = "macos")]
fn build() -> Result<(muda::Menu, muda::MenuItem), muda::Error> {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    use torro_core::i18n::tr;

    let app = Submenu::with_items(
        "Torro",
        true,
        &[
            &PredefinedMenuItem::hide(Some(tr("Hide"))),
            &PredefinedMenuItem::hide_others(Some(tr("Hide others"))),
            &PredefinedMenuItem::show_all(Some(tr("Show all"))),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(Some(tr("Quit"))),
        ],
    )?;
    let copy = MenuItem::with_id(COPY, tr("Copy"), false, Some(Accelerator::new(Modifiers::META, Code::KeyC)));
    let edit = Submenu::with_items(tr("Edit"), true, &[&copy])?;
    let menu = Menu::with_items(&[&app, &edit])?;
    // Instead of muda's channel, which only a blocked thread could wait on.
    muda::MenuEvent::set_event_handler(Some(|event: muda::MenuEvent| {
        if event.id == COPY {
            COPIED.notify_one();
        }
    }));
    menu.init_for_nsapp();
    Ok((menu, copy))
}

/// The task that enables or disables "Copy" (on the main thread); nothing without a menu bar.
pub fn set_copy_enabled<T: Send + 'static>(enabled: bool) -> iced::Task<T> {
    #[cfg(target_os = "macos")]
    {
        iced::window::latest()
            .and_then(move |id| {
                iced::window::run(id, move |_| {
                    INSTALLED.with(|cell| {
                        if let Some((_, copy)) = cell.borrow().as_ref() {
                            copy.set_enabled(enabled);
                        }
                    });
                })
            })
            .discard()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        iced::Task::none()
    }
}

/// One `()` each time "Copy" is chosen (in the menu or with ⌘C) on macOS; nothing elsewhere.
pub fn copy_pressed() -> iced::Subscription<()> {
    #[cfg(target_os = "macos")]
    {
        iced::Subscription::run(presses)
    }
    #[cfg(not(target_os = "macos"))]
    {
        iced::Subscription::none()
    }
}

/// Where muda's handler ([`build`]) hands each "Copy" to [`presses`]. A permit waits there until the
/// stream takes it; nothing blocks a thread, so a dropped subscription leaves nothing behind and the
/// runtime ends when the window closes.
#[cfg(target_os = "macos")]
static COPIED: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// The stream of [`copy_pressed`]: one item per wake-up of [`COPIED`].
#[cfg(target_os = "macos")]
fn presses() -> impl iced::futures::Stream<Item = ()> {
    iced::futures::stream::unfold((), |()| async {
        COPIED.notified().await;
        Some(((), ()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_macos_nothing_happens() {
        // On macOS the tasks need a window and the main thread; the tests run on neither.
        if cfg!(not(target_os = "macos")) {
            assert_eq!(install::<()>().units(), 0);
            assert_eq!(set_copy_enabled::<()>(true).units(), 0);
        }
    }
}
