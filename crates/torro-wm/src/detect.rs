//! Picking the backend at runtime.
//!
//! Task 1.9 in `docs/tasks/phase-1.md`; AeroSpace on macOS: task X.6 in `docs/tasks/plattformen.md`.

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
///
/// On macOS this runs `aerospace list-workspaces --focused` once (at most two seconds, usually
/// a few milliseconds) and blocks the calling thread meanwhile.
pub fn detect(env: impl Fn(&str) -> Option<String>) -> Backend {
    // `&F` implements `Fn` as well, so the same closure can be handed to every probe.
    if let Ok(hyprland) = Hyprland::from_env(&env) {
        return Backend::Hyprland(hyprland);
    }
    #[cfg(target_os = "macos")]
    if let Some(aerospace) = crate::aerospace::AeroSpace::from_env(&env).ok().filter(|wm| wm.answers()) {
        return Backend::AeroSpace(aerospace);
    }
    Backend::Unsupported { name: EXPECTED_WM.into() }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A fake environment that only knows the given pairs; the process environment stays
    /// untouched. Unless the pairs name one, the AeroSpace program points nowhere, so a real
    /// AeroSpace on the test machine is never found.
    fn fake_env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            let found = pairs.iter().find(|(name, _)| *name == key).map(|(_, value)| (*value).to_string());
            found.or_else(|| (key == AEROSPACE_BIN).then(|| "/nonexistent/aerospace".into()))
        }
    }

    const AEROSPACE_BIN: &str = crate::aerospace::cli::PROGRAM_VARIABLE;

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

    /// A fake `aerospace` in a temporary directory that prints `answer` and exits with `code`.
    /// It is run once before it is returned: macOS scans a new executable on its first start,
    /// which can take longer than the probe's timeout.
    #[cfg(target_os = "macos")]
    fn fake_aerospace(name: &str, answer: &str, code: i32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("torro-wm-detect-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let program = dir.join("aerospace");
        std::fs::write(&program, format!("#!/bin/sh\necho '{answer}'\nexit {code}\n")).expect("write fake");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("make fake executable");
        let _ = std::process::Command::new(&program).output();
        program
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detects_aerospace_when_it_answers() {
        let program = fake_aerospace("answers", "1", 0);
        let path = program.to_string_lossy().into_owned();
        let backend = detect(fake_env(&[(AEROSPACE_BIN, path.as_str())]));
        let _ = std::fs::remove_dir_all(program.parent().expect("temp dir"));
        match backend {
            Backend::AeroSpace(aerospace) => assert_eq!(aerospace.program, program),
            other => panic!("expected AeroSpace, got {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unsupported_when_aerospace_server_does_not_answer() {
        let program = fake_aerospace("down", "Can't connect to AeroSpace server. Is AeroSpace.app running?", 1);
        let path = program.to_string_lossy().into_owned();
        let backend = detect(fake_env(&[(AEROSPACE_BIN, path.as_str())]));
        let _ = std::fs::remove_dir_all(program.parent().expect("temp dir"));
        assert_unsupported(backend);
    }
}
