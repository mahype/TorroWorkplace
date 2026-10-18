//! The [`Backend`] of `--demo`: sample data from memory, never a socket.
//!
//! Every app can be started with `--demo` for a look at its settings (design skill, section 9).
//! A window started that way must not change anything real – `torro_ipc::client` reads the app
//! key, so a demo on the socket would really switch rights and settings. The apps therefore hand
//! the [`crate::Host`] this backend instead of their own, and nothing in this library knows the
//! socket at all.
//!
//! Like the control centre's `--demo`, the demo keeps nothing: a change is answered as done and
//! the next load shows the sample data again. It does remember what it was asked to send, which
//! is what the tests of the sections check ([`Demo::sent`]).

use std::sync::Mutex;

use torro_core::settings::Settings;
use torro_core::theme::Appearance;
use torro_ipc::{Request, Response};

use crate::{Answer, Backend};

/// Sample data for the sections, answered from memory.
#[derive(Debug, Default)]
pub struct Demo {
    settings: Settings,
    sent: Mutex<Vec<Request>>,
}

impl Demo {
    /// A demo with the sample settings: dark, so the first look shows the suite's own look.
    pub fn new() -> Demo {
        let settings = Settings { appearance: Appearance::Dark, ..Settings::default() };
        Demo { settings, sent: Mutex::new(Vec::new()) }
    }

    /// The sample settings it answers `settings.settings` with.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Every request it was asked to send, in order: what a section's test looks at.
    pub fn sent(&self) -> Vec<Request> {
        match self.sent.lock() {
            Ok(sent) => sent.clone(),
            // A poisoned lock means a test panicked while holding it; there is nothing to report.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Notes `request` as sent.
    fn note(&self, request: Request) {
        match self.sent.lock() {
            Ok(mut sent) => sent.push(request),
            Err(poisoned) => poisoned.into_inner().push(request),
        }
    }
}

impl Backend for Demo {
    /// Answers what the sections read with sample data and everything else as done, after noting
    /// it ([`Demo::sent`]). A request the demo has no sample for is answered as done too: a
    /// section shows its empty state then instead of an error nobody can act on.
    fn call(&self, request: Request) -> Answer {
        let answer = match &request {
            Request::Settings => Response::Settings { settings: self.settings.clone() },
            _ => {
                self.note(request);
                Response::Ok
            }
        };
        Box::pin(async move { Ok(answer) })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Reading answers with the sample data, without any daemon.
    #[tokio::test]
    async fn settings_come_from_memory() {
        let demo = Demo::new();
        let answer = demo.call(Request::Settings).await.expect("the demo answers");
        assert_eq!(answer, Response::Settings { settings: demo.settings().clone() });
        assert!(demo.sent().is_empty(), "reading is not a change");
    }

    /// A change is answered as done and remembered, but nothing is kept.
    #[tokio::test]
    async fn a_change_is_remembered_but_not_kept() {
        let demo = Arc::new(Demo::new());
        let request = Request::SetSetting { key: torro_ipc::SettingKey::Appearance, value: serde_json::json!("light") };
        assert_eq!(demo.call(request.clone()).await, Ok(Response::Ok));
        assert_eq!(demo.sent(), vec![request]);
        assert_eq!(demo.settings().appearance, Appearance::Dark, "the demo stores nothing");
    }
}
