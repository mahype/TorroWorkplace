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
    if let Ok(hyprland) = Hyprland::from_env(&env) {
        return Backend::Hyprland(hyprland);
    }
    // GlazeWM: its IPC port on 127.0.0.1 accepts connections (task X.7).
    #[cfg(target_os = "windows")]
    if let Some(glazewm) = crate::glazewm::GlazeWm::probe(&env) {
        return Backend::GlazeWm(glazewm);
    }
    Backend::Unsupported { name: EXPECTED_WM.into() }
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

    #[cfg(target_os = "windows")]
    #[test]
    fn detects_glazewm_when_its_port_accepts_connections() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("local address").port().to_string();
        let env = [(crate::glazewm::PORT_VARIABLE, port.as_str())];
        match detect(fake_env(&env)) {
            Backend::GlazeWm(glazewm) => assert_eq!(glazewm.port.to_string(), port),
            other => panic!("expected GlazeWM, got {other:?}"),
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unsupported_when_glazewm_port_is_closed() {
        let port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
            listener.local_addr().expect("local address").port().to_string()
        };
        let env = [(crate::glazewm::PORT_VARIABLE, port.as_str())];
        assert_unsupported(detect(fake_env(&env)));
    }
}
