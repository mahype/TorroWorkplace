//! Which apps of the suite are installed, and whether the main application is (issue #408,
//! `docs/einzelne-apps.md`), for every window of this process: the app switcher and every "Open in …"
//! leave out apps that are not there, and an app without the main application shows the suite's
//! settings itself.
//!
//! The windows ask the core for the settings anyway (`design::Follow`); it also asks `apps.list_apps`
//! at start, then with every poll until the core first answered and from then on every minute, and the answer is kept here process-wide, like the
//! components that are off (`crate::components`). Before the first answer, and with `--demo`, every
//! app and the main application count as installed, so nothing disappears while the core is starting.
//! The main application is read only from the core's answer: an app the core starts through its bundle
//! (`open`) gets no environment passed on. Only with `--demo` does `TORRO_MAIN_APP=0` take it away
//! directly ([`start_demo`]), for looking at the app without it.

use std::collections::BTreeSet;
use std::sync::{LazyLock, PoisonError, RwLock};

use torro_core::AppId;
use torro_ipc::AppInfo;

/// What the core said last; `None` before its first answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Known {
    /// The window classes of the installed apps.
    classes: BTreeSet<String>,
    main_app: bool,
}

static KNOWN: LazyLock<RwLock<Option<Known>>> = LazyLock::new(|| RwLock::new(None));

/// Notes the core's answer to `apps.list_apps`; the next frame follows.
pub fn set(apps: &[AppInfo], main_app: bool) {
    let classes = apps.iter().filter(|app| app.installed).map(|app| app.window_class.clone()).collect();
    let known = Some(Known { classes, main_app });
    let mut held = KNOWN.write().unwrap_or_else(PoisonError::into_inner);
    if *held != known {
        *held = known;
    }
}

/// With `--demo`: every app is installed, and the main application too unless `TORRO_MAIN_APP` is `0`.
/// The windows ask no core then, so this stays.
pub fn start_demo() {
    let known = demo(std::env::var("TORRO_MAIN_APP").ok().as_deref());
    *KNOWN.write().unwrap_or_else(PoisonError::into_inner) = Some(known);
}

/// What `--demo` knows with `TORRO_MAIN_APP` set to `value`: every app, and the main application
/// unless `value` is `0`.
fn demo(value: Option<&str>) -> Known {
    let classes = AppId::ALL.iter().map(|app| app.window_class.to_string()).collect();
    Known { classes, main_app: value.is_none_or(|value| value.trim() != "0") }
}

/// Whether `app` is installed, as the core said last. The main application ([`AppId::WORKPLACE`],
/// which the core does not list) is [`main_app`].
pub fn is_installed(app: AppId) -> bool {
    if app == AppId::WORKPLACE {
        return main_app();
    }
    KNOWN
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_none_or(|known| known.classes.contains(app.window_class))
}

/// Whether the main application (the control centre with its tray icon) is installed, as the core
/// said last.
pub fn main_app() -> bool {
    KNOWN.read().unwrap_or_else(PoisonError::into_inner).as_ref().is_none_or(|known| known.main_app)
}

/// Whether the core has answered once (or the demo set what is installed).
pub fn answered() -> bool {
    KNOWN.read().unwrap_or_else(PoisonError::into_inner).is_some()
}

/// Asks the core on `socket` for the installed apps and notes the answer; an error keeps what was
/// known.
pub async fn fetch(socket: &std::path::Path) {
    if let Ok(torro_ipc::Response::Apps { apps, main_app }) =
        torro_ipc::client::call(socket, &torro_ipc::Request::ListApps).await
    {
        set(&apps, main_app);
    }
}

/// Forgets the core's answer, as before its first one: for tests.
#[cfg(test)]
pub(crate) fn reset() {
    *KNOWN.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Serialises this crate's tests that change what the windows list (this state and
/// `crate::components`).
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn info(app: AppId, installed: bool) -> AppInfo {
        AppInfo {
            binary: app.binary.to_string(),
            name: app.display_name.to_string(),
            window_class: app.window_class.to_string(),
            installed,
        }
    }

    #[test]
    fn the_core_decides_what_is_installed() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        reset();
        // Before the first answer everything is there.
        assert!(!answered());
        assert!(AppId::ALL.iter().all(|app| is_installed(*app)) && main_app());
        set(&[info(AppId::CALENDAR, true), info(AppId::TASKS, false)], false);
        assert!(answered());
        assert!(is_installed(AppId::CALENDAR));
        assert!(!is_installed(AppId::TASKS), "listed, but not installed");
        assert!(!is_installed(AppId::INBOX_MAIL), "not listed");
        assert!(!main_app() && !is_installed(AppId::WORKPLACE));
        set(&[], true);
        assert!(main_app() && is_installed(AppId::WORKPLACE));
        reset();
    }

    #[test]
    fn the_demo_has_every_app_and_only_zero_takes_the_main_app_away() {
        for value in [None, Some("1"), Some(""), Some("0")] {
            let known = demo(value);
            assert!(AppId::ALL.iter().all(|app| known.classes.contains(app.window_class)), "{value:?}");
            assert_eq!(known.main_app, value != Some("0"), "{value:?}");
        }
    }
}
