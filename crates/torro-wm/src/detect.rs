//! Picking the backend at runtime.
//!
//! Task 1.9 in `docs/tasks/phase-1.md`.

use crate::Backend;
use crate::hyprland::Hyprland;

/// Name of the window manager we look for on this platform, for the tray menu.
#[cfg(target_os = "macos")]
const EXPECTED_WM: &str = "AeroSpace (macOS)";
#[cfg(target_os = "windows")]
const EXPECTED_WM: &str = "GlazeWM (Windows)";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const EXPECTED_WM: &str = "no supported window manager (Hyprland not found)";

/// The backend for the window manager that runs in this session. `env` looks up an
/// environment variable (pass `|k| std::env::var(k).ok()`), so tests can fake it.
pub fn detect(env: impl Fn(&str) -> Option<String>) -> Backend {
    // `&F` implements `Fn` as well, so the same closure can be handed to every probe.
    match Hyprland::from_env(&env) {
        Ok(hyprland) => Backend::Hyprland(hyprland),
        Err(_) => Backend::Unsupported { name: EXPECTED_WM.into() },
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A fake environment that only knows the given pairs; the process environment stays
    /// untouched.
    fn fake_env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| pairs.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_string())
    }

    fn assert_unsupported(backend: Backend) {
        match backend {
            Backend::Unsupported { name } => assert_eq!(name, EXPECTED_WM),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn detects_hyprland_when_both_variables_are_set() {
        let env = [("XDG_RUNTIME_DIR", "/run/user/1000"), ("HYPRLAND_INSTANCE_SIGNATURE", "abc_123")];
        match detect(fake_env(&env)) {
            Backend::Hyprland(hyprland) => {
                assert_eq!(hyprland.socket_dir, Path::new("/run/user/1000").join("hypr").join("abc_123"));
            }
            other => panic!("expected Hyprland, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_when_signature_is_missing() {
        let env = [("XDG_RUNTIME_DIR", "/run/user/1000")];
        assert_unsupported(detect(fake_env(&env)));
    }

    #[test]
    fn unsupported_when_runtime_dir_is_missing() {
        let env = [("HYPRLAND_INSTANCE_SIGNATURE", "abc_123")];
        assert_unsupported(detect(fake_env(&env)));
    }

    #[test]
    fn unsupported_when_variables_are_empty() {
        let env = [("XDG_RUNTIME_DIR", ""), ("HYPRLAND_INSTANCE_SIGNATURE", "")];
        assert_unsupported(detect(fake_env(&env)));
    }

    #[test]
    fn unsupported_when_no_variables_are_set() {
        let env: [(&str, &str); 0] = [];
        assert_unsupported(detect(fake_env(&env)));
    }
}
