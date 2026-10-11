//! Desktop notifications for tray commands: "Vorlage gespeichert", "Vorlage geladen" and
//! the failures. [`message`] decides what to say, [`show`] puts it on the desktop without
//! ever blocking the daemon.

use torro_ipc::Response;

/// The app name shown with each notification.
const APP_NAME: &str = "Torro Workplace";

/// The summary and body for a finished command, or `None` when there is nothing to tell.
///
/// `what` is the short description the daemon gives each request (`delete dev`, `save`, ...).
pub fn message(what: &str, response: &Response) -> Option<(String, String)> {
    match response {
        Response::Saved { template } => {
            Some(("Vorlage gespeichert".into(), format!("{} · {} Fenster", template.name, template.windows)))
        }
        Response::Loaded { restored, failed } => match failed.first() {
            None => Some(("Vorlage geladen".into(), format!("{restored} Fenster"))),
            Some((_, reason)) => Some((
                "Vorlage teilweise geladen".into(),
                format!("{restored} Fenster da, {} fehlgeschlagen\n{reason}", failed.len()),
            )),
        },
        Response::Error { code, message } => {
            let body = if code == "workspace-not-empty" {
                "Der Workspace ist nicht leer. Wähle „Hinzufügen“ oder „Ersetzen“.".to_string()
            } else {
                message.clone()
            };
            Some(("Fehlgeschlagen".into(), body))
        }
        Response::Ok => match what.strip_prefix("delete ") {
            Some(id) => Some(("Vorlage gelöscht".into(), id.to_string())),
            None => Some(("Erledigt".into(), what.to_string())),
        },
        Response::Pong { .. } | Response::Status(_) | Response::Templates { .. } => None,
    }
}

/// Shows the notification on a thread of its own: showing may block on some systems, and the
/// daemon must not wait for it. A failure is only printed to stderr.
pub fn show(summary: String, body: String) {
    std::thread::spawn(move || {
        if let Err(err) = send(&summary, &body) {
            eprintln!("torroworkplace: cannot show notification: {err}");
        }
    });
}

fn send(summary: &str, body: &str) -> notify_rust::error::Result<()> {
    notify_rust::Notification::new().appname(APP_NAME).summary(summary).body(body).show().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_setups::TemplateSummary;

    fn summary(name: &str, windows: usize) -> TemplateSummary {
        TemplateSummary { id: "dev".into(), name: name.into(), windows, updated: chrono::Utc::now() }
    }

    fn text(summary: &str, body: &str) -> Option<(String, String)> {
        Some((summary.to_string(), body.to_string()))
    }

    #[test]
    fn saved_names_template_and_window_count() {
        let response = Response::Saved { template: summary("Dev", 3) };
        assert_eq!(message("save", &response), text("Vorlage gespeichert", "Dev · 3 Fenster"));
    }

    #[test]
    fn loaded_without_failures_reports_windows() {
        let response = Response::Loaded { restored: 4, failed: vec![] };
        assert_eq!(message("load dev", &response), text("Vorlage geladen", "4 Fenster"));
    }

    #[test]
    fn loaded_with_failures_reports_counts_and_first_reason() {
        let response = Response::Loaded {
            restored: 2,
            failed: vec![("firefox".into(), "no window".into()), ("kitty".into(), "timeout".into())],
        };
        assert_eq!(
            message("load dev", &response),
            text("Vorlage teilweise geladen", "2 Fenster da, 2 fehlgeschlagen\nno window")
        );
    }

    #[test]
    fn error_shows_its_message() {
        let response = Response::Error { code: "not-found".into(), message: "no template dev".into() };
        assert_eq!(message("load dev", &response), text("Fehlgeschlagen", "no template dev"));
    }

    #[test]
    fn workspace_not_empty_has_a_clearer_body() {
        let response = Response::Error { code: "workspace-not-empty".into(), message: "3 windows".into() };
        assert_eq!(
            message("load dev", &response),
            text("Fehlgeschlagen", "Der Workspace ist nicht leer. Wähle „Hinzufügen“ oder „Ersetzen“.")
        );
    }

    #[test]
    fn deleted_template_names_its_id() {
        assert_eq!(message("delete dev", &Response::Ok), text("Vorlage gelöscht", "dev"));
    }

    #[test]
    fn other_ok_says_done_with_what() {
        assert_eq!(message("update dev", &Response::Ok), text("Erledigt", "update dev"));
    }

    #[test]
    fn queries_get_no_notification() {
        assert_eq!(message("ping", &Response::Pong { version: "0.0.1".into() }), None);
        assert_eq!(message("list", &Response::Templates { templates: vec![] }), None);
    }

    /// Shows one real notification. Run by hand: `cargo test -p torroworkplace -- --ignored`.
    #[test]
    #[ignore = "shows a real desktop notification"]
    fn shows_a_real_notification() {
        send("Vorlage gespeichert", "Test · 0 Fenster").expect("notification shown");
    }
}
