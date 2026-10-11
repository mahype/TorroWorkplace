//! Picking the backend at runtime.
//!
//! Task 1.9 in `docs/tasks/phase-1.md`.

use crate::Backend;

/// The backend for the window manager that runs in this session. `env` looks up an
/// environment variable (pass `|k| std::env::var(k).ok()`), so tests can fake it.
pub fn detect(env: impl Fn(&str) -> Option<String>) -> Backend {
    let _ = env;
    Backend::Unsupported { name: "detection not implemented yet".into() }
}
