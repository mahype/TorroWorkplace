//! Desktop notifications for template commands: "Template saved", "Template loaded" and
//! the failures (translated with `torro_core::i18n`). [`message`] decides what to say, [`show`] puts it on the desktop without
//! ever blocking the daemon.

use torro_core::i18n::{tr, trf};
use torro_ipc::Response;

/// The app name shown with each notification.
const APP_NAME: &str = "Torro Workplace";

/// The summary and body for a finished command, or `None` when there is nothing to tell.
///
/// `what` is the short description the daemon gives each request (`delete dev`, `save`, ...).
pub fn message(what: &str, response: &Response) -> Option<(String, String)> {
    match response {
        Response::Saved { template } => Some((
            tr("Template saved").into(),
            trf("{name} · {n} windows", &[("name", &template.name), ("n", &template.windows)]),
        )),
        Response::Loaded { restored, failed } => match failed.first() {
            None => Some((tr("Template loaded").into(), trf("{n} windows", &[("n", restored)]))),
            Some((_, reason)) => Some((
                tr("Template partly loaded").into(),
                format!(
                    "{}\n{reason}",
                    trf(
                        "{restored} windows in place, {failed} failed",
                        &[("restored", restored), ("failed", &failed.len())]
                    )
                ),
            )),
        },
        Response::Error { code, message } => {
            let body = if code == "workspace-not-empty" {
                tr("The workspace is not empty. Choose “Add” or “Replace”.").to_string()
            } else {
                message.clone()
            };
            Some((tr("Failed").into(), body))
        }
        Response::Ok => match what.strip_prefix("delete ") {
            Some(id) => Some((tr("Template deleted").into(), id.to_string())),
            None => Some((tr("Done").into(), what.to_string())),
        },
        _ => None,
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
        assert_eq!(
            message("save", &response),
            text(tr("Template saved"), &trf("{name} · {n} windows", &[("name", &"Dev"), ("n", &3)]))
        );
    }

    #[test]
    fn loaded_without_failures_reports_windows() {
        let response = Response::Loaded { restored: 4, failed: vec![] };
        assert_eq!(message("load dev", &response), text(tr("Template loaded"), &trf("{n} windows", &[("n", &4)])));
    }

    #[test]
    fn loaded_with_failures_reports_counts_and_first_reason() {
        let response = Response::Loaded {
            restored: 2,
            failed: vec![("firefox".into(), "no window".into()), ("kitty".into(), "timeout".into())],
        };
        assert_eq!(
            message("load dev", &response),
            text(
                tr("Template partly loaded"),
                &format!(
                    "{}\nno window",
                    trf("{restored} windows in place, {failed} failed", &[("restored", &2), ("failed", &2)])
                )
            )
        );
    }

    #[test]
    fn error_shows_its_message() {
        let response = Response::Error { code: "not-found".into(), message: "no template dev".into() };
        assert_eq!(message("load dev", &response), text(tr("Failed"), "no template dev"));
    }

    #[test]
    fn workspace_not_empty_has_a_clearer_body() {
        let response = Response::Error { code: "workspace-not-empty".into(), message: "3 windows".into() };
        assert_eq!(
            message("load dev", &response),
            text(tr("Failed"), tr("The workspace is not empty. Choose “Add” or “Replace”."))
        );
    }

    #[test]
    fn deleted_template_names_its_id() {
        assert_eq!(message("delete dev", &Response::Ok), text(tr("Template deleted"), "dev"));
    }

    #[test]
    fn other_ok_says_done_with_what() {
        assert_eq!(message("update dev", &Response::Ok), text(tr("Done"), "update dev"));
    }

    #[test]
    fn queries_get_no_notification() {
        assert_eq!(message("ping", &Response::Pong { version: "0.0.1".into(), api: torro_ipc::API_VERSION }), None);
        assert_eq!(message("list", &Response::Templates { templates: vec![] }), None);
    }

    /// Shows one real notification. Run by hand: `cargo test -p torroworkplace -- --ignored`.
    #[test]
    #[ignore = "shows a real desktop notification"]
    fn shows_a_real_notification() {
        send(tr("Template saved"), "Test · 0 windows").expect("notification shown");
    }
}
