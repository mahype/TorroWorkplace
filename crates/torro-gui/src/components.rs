//! Which components of the suite are on (task W.2, `docs/einrichtung.md`, "Komponenten an und
//! aus"), for every window of this process: the app switcher leaves out the apps of components
//! turned off.
//!
//! The windows ask the core for the settings anyway, for the design pack (`design::Follow`, every
//! two seconds); that answer also brings `Settings::components_off`, which is kept here
//! process-wide, like the active design. Before the first answer, and with `--demo`, every
//! component is on.

use std::collections::BTreeSet;
use std::sync::{LazyLock, PoisonError, RwLock};

use torro_core::AppId;
use torro_core::settings::Component;

static OFF: LazyLock<RwLock<BTreeSet<Component>>> = LazyLock::new(|| RwLock::new(BTreeSet::new()));

/// Notes the components the settings turn off; the next frame follows.
pub fn set_off(off: &BTreeSet<Component>) {
    let mut held = OFF.write().unwrap_or_else(PoisonError::into_inner);
    if *held != *off {
        *held = off.clone();
    }
}

/// Whether `component` is on, as the settings said last.
pub fn is_on(component: Component) -> bool {
    !OFF.read().unwrap_or_else(PoisonError::into_inner).contains(&component)
}

/// The component an app of the suite belongs to; `None` for the control centre, which is always
/// there.
pub fn of(app: AppId) -> Option<Component> {
    match app.window_class {
        "torro-calendar" => Some(Component::Calendar),
        "torro-tasks" => Some(Component::Tasks),
        "torro-inbox-mail" => Some(Component::Mail),
        "torro-inbox" | "torro-inbox-notifications" => Some(Component::Inbox),
        "torro-assistant" => Some(Component::Assistant),
        _ => None,
    }
}

/// Whether the app switcher lists `app`: its component is on, or it has none.
pub fn app_on(app: AppId) -> bool {
    of(app).is_none_or(is_on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_but_the_control_centre_belongs_to_a_component() {
        for app in AppId::ALL {
            assert_eq!(of(*app).is_none(), *app == AppId::WORKPLACE, "{}", app.window_class);
        }
        assert_eq!(of(AppId::INBOX_MAIL), Some(Component::Mail), "the mail window is mail, not the hub");
    }

    #[test]
    fn the_settings_decide_which_apps_are_on() {
        // The app switcher's test reads the same state.
        let _lock = crate::installed::TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_off(&BTreeSet::from([Component::Calendar]));
        assert!(!app_on(AppId::CALENDAR) && app_on(AppId::TASKS) && app_on(AppId::WORKPLACE));
        set_off(&BTreeSet::new());
        assert!(app_on(AppId::CALENDAR));
    }
}
