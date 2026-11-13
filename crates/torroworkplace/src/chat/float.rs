//! Floating the chat's windows under AeroSpace and GlazeWM (issue #667, decision 16 in `docs/chat.md`).
//!
//! Hyprland floats a window by a rule on its class before it opens. AeroSpace and GlazeWM have no such
//! rule at run time, and they name every window of `torrochat` alike – the contact list too – by its
//! bundle id or process. So the core watches new windows: a window of the suite whose start command
//! carries the flag of a floating window ([`torro_core::app::FLOATING_WINDOWS`]) is floated as it
//! appears. It stands tiled for a moment before; only a rule in the person's configuration would avoid
//! that, and the core leaves that configuration alone.

use std::time::Duration;

use torro_setups::capture::{floating_window_class, suite_app};
use torro_wm::{Events, Window, WindowId, WindowManager, WmError, WmEvent};

/// How long [`float_opened`] waits before it looks once more for a window the window manager announced
/// but does not list yet (AeroSpace can report a window before it lays it out).
const NOT_LISTED_YET: Duration = Duration::from_millis(300);

/// Floats the window `window` that just opened with the class `class` if it is one of the floating
/// windows of the suite; `Ok(true)` when it was floated.
///
/// A window outside the suite costs no question to the window manager and no look at its process.
/// A window the window manager does not list is looked for once more after [`NOT_LISTED_YET`]; one that
/// is gone then or already floats is left alone. `command_of` finds the window's start command (the
/// core passes `torro_setups::launch::command_for`).
pub async fn float_opened(
    wm: &impl WindowManager,
    window: &WindowId,
    class: &str,
    command_of: impl Fn(&Window) -> Vec<String>,
) -> Result<bool, WmError> {
    if suite_app(class).is_none() {
        return Ok(false);
    }
    let mut opened = listed(wm, window).await?;
    if opened.is_none() {
        tokio::time::sleep(NOT_LISTED_YET).await;
        opened = listed(wm, window).await?;
    }
    let Some(opened) = opened else {
        return Ok(false);
    };
    if opened.floating || floating_window_class(class, &command_of(&opened)).is_none() {
        return Ok(false);
    }
    wm.float_window(window).await?;
    Ok(true)
}

/// The window `window` as the window manager lists it now, `None` when it does not.
async fn listed(wm: &impl WindowManager, window: &WindowId) -> Result<Option<Window>, WmError> {
    Ok(wm.windows(None).await?.into_iter().find(|w| &w.id == window))
}

/// Floats every floating window of the suite that is open and tiled now; the number floated. For the
/// start of watching: windows that opened while nobody listened, and windows a restarted window
/// manager tiled again. A window that cannot be floated is logged and the next one looked at.
pub async fn float_open_windows(
    wm: &impl WindowManager,
    command_of: impl Fn(&Window) -> Vec<String>,
) -> Result<usize, WmError> {
    let mut floated = 0;
    for window in wm.windows(None).await? {
        if window.floating
            || suite_app(&window.class).is_none()
            || floating_window_class(&window.class, &command_of(&window)).is_none()
        {
            continue;
        }
        match wm.float_window(&window.id).await {
            Ok(()) => floated += 1,
            Err(err) => eprintln!("torroworkplace: a chat window will not float: {err}"),
        }
    }
    Ok(floated)
}

/// Floats every floating window of the suite that opens while `events` runs ([`float_opened`]). A
/// failure is only logged and the next window is looked at; ends with the event stream (the window
/// manager stopped), so the caller subscribes again.
pub async fn follow(wm: &impl WindowManager, mut events: Events, command_of: impl Fn(&Window) -> Vec<String>) {
    while let Some(event) = events.recv().await {
        let WmEvent::WindowOpened { window, class, .. } = event else { continue };
        if let Err(err) = float_opened(wm, &window, &class, &command_of).await {
            eprintln!("torroworkplace: a chat window will not float: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use torro_wm::Rect;
    use torro_wm::fake::{Call, FakeWm, window};

    use super::*;

    const CHAT_BUNDLE: &str = "io.github.mahype.torro-chat";

    fn chat_command(window: &Window) -> Vec<String> {
        let _ = window;
        ["torrochat", "--conversation=ben"].map(String::from).to_vec()
    }

    fn contact_list_command(window: &Window) -> Vec<String> {
        let _ = window;
        vec!["torrochat".to_string()]
    }

    #[tokio::test]
    async fn a_chat_window_is_floated() {
        let wm = FakeWm::new("1");
        let opened = wm.open_window(CHAT_BUNDLE, Some(42));
        assert_eq!(float_opened(&wm, &opened, CHAT_BUNDLE, chat_command).await, Ok(true));
        assert_eq!(wm.calls(), vec![Call::FloatWindow(opened)]);
        assert!(wm.all_windows()[0].floating);
    }

    #[tokio::test]
    async fn the_contact_list_stays_tiled() {
        let wm = FakeWm::new("1");
        let opened = wm.open_window("torrochat.exe", Some(42));
        assert_eq!(float_opened(&wm, &opened, "torrochat.exe", contact_list_command).await, Ok(false));
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn another_program_is_not_looked_at() {
        let wm = FakeWm::new("1");
        let opened = wm.open_window("com.mitchellh.ghostty", Some(42));
        let asked = Cell::new(false);
        let command_of = |window: &Window| {
            asked.set(true);
            chat_command(window)
        };
        assert_eq!(float_opened(&wm, &opened, "com.mitchellh.ghostty", command_of).await, Ok(false));
        assert!(!asked.get(), "no look at a foreign process");
        assert!(wm.calls().is_empty());
    }

    #[tokio::test]
    async fn a_closed_or_floating_window_is_left_alone() {
        let wm = FakeWm::new("1");
        let gone = WindowId("0xgone".into());
        assert_eq!(float_opened(&wm, &gone, CHAT_BUNDLE, chat_command).await, Ok(false));
        let mut floating = window("0xa", CHAT_BUNDLE, "1", Rect::default());
        floating.floating = true;
        wm.add_window(floating);
        assert_eq!(float_opened(&wm, &WindowId("0xa".into()), CHAT_BUNDLE, chat_command).await, Ok(false));
        assert!(wm.calls().is_empty());
    }

    /// After a restart of the window manager the chat windows are tiles again: watching starts by
    /// floating them, not the contact list, not a window that floats already, not another program.
    #[tokio::test]
    async fn open_chat_windows_are_floated_when_watching_starts() {
        let wm = FakeWm::new("1");
        let tiled_chat = window("0xa", CHAT_BUNDLE, "1", Rect::default());
        let mut list = window("0xb", CHAT_BUNDLE, "1", Rect::default());
        list.pid = Some(1);
        let mut floating_chat = window("0xc", CHAT_BUNDLE, "1", Rect::default());
        floating_chat.floating = true;
        wm.add_window(tiled_chat);
        wm.add_window(list);
        wm.add_window(floating_chat);
        wm.add_window(window("0xd", "com.mitchellh.ghostty", "1", Rect::default()));
        let command_of = |window: &Window| {
            if window.pid == Some(1) { contact_list_command(window) } else { chat_command(window) }
        };
        assert_eq!(float_open_windows(&wm, command_of).await, Ok(1));
        assert_eq!(wm.calls(), vec![Call::FloatWindow(WindowId("0xa".into()))]);
    }

    #[tokio::test]
    async fn follow_floats_a_window_as_it_opens() {
        let wm = FakeWm::new("1");
        let mut events = wm.events().await.expect("events");
        let list = wm.open_window(CHAT_BUNDLE, Some(1));
        let chat = wm.open_window(CHAT_BUNDLE, Some(2));
        // The fake keeps its subscribers, so its events go into a stream that ends, as a stopped
        // window manager's does, and `follow` returns.
        let (tx, ended) = tokio::sync::mpsc::channel(8);
        while let Ok(event) = events.try_recv() {
            tx.send(event).await.expect("forward");
        }
        drop(tx);
        let command_of = |window: &Window| {
            if window.pid == Some(2) { chat_command(window) } else { contact_list_command(window) }
        };
        follow(&wm, ended, command_of).await;
        assert_eq!(wm.calls(), vec![Call::FloatWindow(chat.clone())]);
        let floating: Vec<_> = wm.all_windows().into_iter().filter(|w| w.floating).map(|w| w.id).collect();
        assert_eq!(floating, [chat], "the contact list {list} stays tiled");
    }
}
