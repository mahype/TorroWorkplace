//! Which core this process is: the person's own, or a second one for development and tests
//! (issue #877).
//!
//! A second core runs with its own `TORRO_{CONFIG,DATA,STATE}_HOME`. Its files are then apart from
//! the real ones, but the system keychain and the notifications are shared by every process of the
//! user. So the second core:
//!
//! - files its secrets under a keychain service of its own ([`keychain_service`]). On macOS every
//!   account secret sits in one entry of the service (`account::vault`); with the real service a
//!   second core would read the real vault and write its own version back over it.
//! - marks its notifications as a test, or shows none ([`notifications`]), so "Account removed"
//!   from a test never looks like news from the real core.
//!
//! `TORRO_KEYCHAIN_SERVICE` names the service outright, also `torroworkplace` for a portable install
//! that should keep using the real secrets. `TORRO_NOTIFICATIONS` is `on`, `test` or `off`.

use std::env;
use std::sync::LazyLock;

use sha2::{Digest, Sha256};

/// The keychain service of the person's own core; every suite secret is filed under it.
pub const KEYCHAIN_SERVICE: &str = "torroworkplace";

/// Names the keychain service outright, before anything is derived from the folders.
pub const KEYCHAIN_SERVICE_VAR: &str = "TORRO_KEYCHAIN_SERVICE";

/// How notifications are shown: `on`, `test` (marked as a test) or `off`.
pub const NOTIFICATIONS_VAR: &str = "TORRO_NOTIFICATIONS";

/// The variables that move the core's folders away from the real ones ([`crate::paths`]).
const HOME_VARS: [&str; 3] = ["TORRO_CONFIG_HOME", "TORRO_DATA_HOME", "TORRO_STATE_HOME"];

/// How many hex digits of the folders' hash go into a derived service name: enough to keep two test
/// cores apart, short enough to read in Keychain Access.
const HASH_DIGITS: usize = 12;

/// How this process shows desktop notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notifications {
    /// As they are: the person's own core.
    On,
    /// With "[Test]" in front of the title, so they cannot pass for the real core's.
    Test,
    /// Not at all.
    Off,
}

/// The keychain service of this process, decided once from the environment
/// ([`keychain_service_from`]).
pub fn keychain_service() -> &'static str {
    static SERVICE: LazyLock<String> = LazyLock::new(|| keychain_service_from(env_value));
    &SERVICE
}

/// Whether this process is a second core: its secrets are not under [`KEYCHAIN_SERVICE`].
pub fn is_second_core() -> bool {
    keychain_service() != KEYCHAIN_SERVICE
}

/// How this process shows notifications, decided once from the environment
/// ([`notifications_from`]).
pub fn notifications() -> Notifications {
    static MODE: LazyLock<Notifications> =
        LazyLock::new(|| notifications_from(env_value(NOTIFICATIONS_VAR).as_deref(), is_second_core()));
    *MODE
}

/// The keychain service for the environment `var` describes: [`KEYCHAIN_SERVICE_VAR`] when set;
/// otherwise, when any of `TORRO_{CONFIG,DATA,STATE}_HOME` is set, `torroworkplace-` with a hash of
/// the three folders, the same one for every start with the same folders; otherwise
/// [`KEYCHAIN_SERVICE`]. Empty values count as unset.
pub fn keychain_service_from(var: impl Fn(&str) -> Option<String>) -> String {
    if let Some(service) = var(KEYCHAIN_SERVICE_VAR) {
        return service;
    }
    let homes = HOME_VARS.map(&var);
    if homes.iter().all(Option::is_none) {
        return KEYCHAIN_SERVICE.to_string();
    }
    let mut hash = Sha256::new();
    for home in &homes {
        // A separator that cannot be in a path, so ("a", "bc") and ("ab", "c") differ.
        hash.update(home.as_deref().unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    let digits: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{KEYCHAIN_SERVICE}-{}", &digits[..HASH_DIGITS])
}

/// The notifications for `value` of [`NOTIFICATIONS_VAR`]: `on`, `test` and `off` (any case) as
/// named; otherwise a second core marks them as a test and the person's own core shows them as
/// they are.
pub fn notifications_from(value: Option<&str>, second_core: bool) -> Notifications {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("on") => Notifications::On,
        Some("test") => Notifications::Test,
        Some("off") => Notifications::Off,
        _ if second_core => Notifications::Test,
        _ => Notifications::On,
    }
}

/// The value of the variable `name`, `None` when unset, empty or blank.
fn env_value(name: &str) -> Option<String> {
    env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn service(vars: &[(&str, &str)]) -> String {
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        keychain_service_from(|name| vars.get(name).cloned())
    }

    #[test]
    fn the_own_core_keeps_the_real_service() {
        assert_eq!(service(&[]), KEYCHAIN_SERVICE);
    }

    #[test]
    fn own_folders_give_a_service_of_their_own() {
        let test = service(&[("TORRO_CONFIG_HOME", "/tmp/a/config"), ("TORRO_DATA_HOME", "/tmp/a/data")]);
        assert_ne!(test, KEYCHAIN_SERVICE);
        assert!(test.starts_with("torroworkplace-"), "{test}");
        assert_eq!(test.len(), KEYCHAIN_SERVICE.len() + 1 + HASH_DIGITS);
        let again = service(&[("TORRO_DATA_HOME", "/tmp/a/data"), ("TORRO_CONFIG_HOME", "/tmp/a/config")]);
        assert_eq!(test, again, "the same folders, the same service on every start");
    }

    #[test]
    fn other_folders_give_another_service() {
        let one = service(&[("TORRO_STATE_HOME", "/tmp/one")]);
        let two = service(&[("TORRO_STATE_HOME", "/tmp/two")]);
        assert_ne!(one, two);
        let moved = service(&[("TORRO_DATA_HOME", "/tmp/one")]);
        assert_ne!(one, moved, "the same folder for another kind is another core");
    }

    #[test]
    fn the_named_service_wins() {
        let named = [("TORRO_KEYCHAIN_SERVICE", "torro-dev"), ("TORRO_CONFIG_HOME", "/tmp/a")];
        assert_eq!(service(&named), "torro-dev");
        let portable = [("TORRO_KEYCHAIN_SERVICE", KEYCHAIN_SERVICE), ("TORRO_CONFIG_HOME", "/tmp/a")];
        assert_eq!(service(&portable), KEYCHAIN_SERVICE, "a portable install may keep the real secrets");
    }

    #[test]
    fn a_second_core_marks_its_notifications() {
        assert_eq!(notifications_from(None, false), Notifications::On);
        assert_eq!(notifications_from(None, true), Notifications::Test);
        assert_eq!(notifications_from(Some("OFF"), true), Notifications::Off);
        assert_eq!(notifications_from(Some("on"), true), Notifications::On);
        assert_eq!(notifications_from(Some("test"), false), Notifications::Test);
        assert_eq!(notifications_from(Some("loud"), false), Notifications::On, "unknown values change nothing");
    }
}
