//! Desktop notifications for template commands: "Template saved", "Template loaded" and
//! the failures (translated with `torro_core::i18n`). [`message`] decides what to say, [`show`] puts it on the desktop without
//! ever blocking the daemon.
//!
//! And the question to the person when an agent's command waits for approval ([`ask`]): the
//! summary with "Allow" and "Deny" (task A.3), closed again once the approval is decided or
//! expires (task A.3b).
//!
//! On Linux the answer comes back as the D-Bus signal `ActionInvoked` for the notification's id.
//! The signal is not checked for its sender: any program of the same user on the session bus
//! could send it, as it could read the app key next to the socket. The threat model of approvals
//! is an agent talking to the API or MCP, not code that already runs as the user (`docs/api.md`,
//! "Freigaben").

use torro_core::i18n::{tr, trf};
use torro_ipc::{ApprovalInfo, Response};

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

/// The title and body of the question for `approval`: who asks, and what would happen. Both hold
/// text the client chose (its name, the titles in the summary): [`ask`] shows them as plain text.
pub fn question(approval: &ApprovalInfo) -> (String, String) {
    (trf("{client} asks for approval", &[("client", &approval.client_name)]), approval.summary.clone())
}

/// Asks the person about `approval` and calls `decide` with their answer: `true` for "Allow",
/// `false` for "Deny". Closing the notification, or clicking it anywhere else, decides nothing;
/// the approval then waits in the control centre until it expires.
///
/// The question goes away by itself when `settled` fires (the approval was decided elsewhere:
/// control centre, `ctl allow|deny`) or its sender is dropped, and when the approval expires,
/// whichever comes first; nothing waits for it longer than that. It runs as a task on the core's
/// runtime and never blocks the caller; without a runtime nothing is shown.
///
/// Linux and the BSDs show the buttons. macOS and Windows show the question without them (their
/// notifications need an app bundle or identity for buttons), so the person decides in the control
/// centre there.
pub fn ask(
    approval: &ApprovalInfo,
    settled: tokio::sync::oneshot::Receiver<()>,
    decide: impl FnOnce(bool) + Send + 'static,
) {
    let (summary, body) = question(approval);
    let lasts = (approval.expires - chrono::Utc::now()).to_std().unwrap_or_default();
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        eprintln!("torroworkplace: cannot show notification: no runtime to wait for the answer on");
        return;
    };
    runtime.spawn(async move {
        if let Err(err) = send_question(summary, body, lasts, settled, decide).await {
            eprintln!("torroworkplace: cannot show notification: {err}");
        }
    });
}

/// Shows the question and waits for the first of: an answer, `settled`, or `lasts` running out.
/// In the last two cases it closes the notification, so no question about a decided or expired
/// approval stays on the desktop (some servers keep notifications with actions until clicked).
#[cfg(all(unix, not(target_os = "macos")))]
async fn send_question(
    summary: String,
    body: String,
    lasts: std::time::Duration,
    settled: tokio::sync::oneshot::Receiver<()>,
    decide: impl FnOnce(bool),
) -> notify_rust::error::Result<()> {
    let millis = u32::try_from(lasts.as_millis()).unwrap_or(u32::MAX);
    let handle = notify_rust::Notification::new()
        .appname(APP_NAME)
        // The summary is plain text by the specification; only the body may carry markup.
        .summary(&summary)
        .body(&plain(&body))
        .action("allow", tr("Allow"))
        .action("deny", tr("Deny"))
        .timeout(notify_rust::Timeout::Milliseconds(millis))
        .show_async()
        .await?;
    let answered = handle.wait_for_action_async(|response| {
        // A click on the notification itself ("default") and closing it decide nothing.
        if let notify_rust::NotificationResponse::Action(action) = response {
            match action.as_str() {
                "allow" => decide(true),
                "deny" => decide(false),
                _ => {}
            }
        }
    });
    tokio::select! {
        () = answered => {}
        _ = settled => handle.close_async().await,
        () = tokio::time::sleep(lasts) => handle.close_async().await,
    }
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
async fn send_question(
    summary: String,
    body: String,
    _lasts: std::time::Duration,
    _settled: tokio::sync::oneshot::Receiver<()>,
    _decide: impl FnOnce(bool),
) -> notify_rust::error::Result<()> {
    // Showing may block on these systems; the core's runtime must not wait for it.
    tokio::task::spawn_blocking(move || send(&summary, &body))
        .await
        .unwrap_or_else(|err| Err(notify_rust::error::Error::from(err.to_string().as_str())))
}

/// `text` for a notification server that may read markup in the body (`<b>`, `<a href>`): `&`,
/// `<` and `>` as entities, so a client's text shows as written and never becomes a link.
#[cfg(all(unix, not(target_os = "macos")))]
fn plain(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
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

    #[test]
    fn the_question_names_the_client_and_what_would_happen() {
        let at = chrono::Utc::now();
        let approval = ApprovalInfo {
            id: "9c1f0a7e3b2d4c58".into(),
            client: "c1".into(),
            client_name: "Claude Code".into(),
            command: "workspaces.delete_template".into(),
            summary: "Delete template \"dev\"".into(),
            created: at,
            expires: at,
            decided: None,
            state: torro_ipc::ApprovalState::Pending,
        };
        assert_eq!(
            question(&approval),
            (trf("{client} asks for approval", &[("client", &"Claude Code")]), "Delete template \"dev\"".to_string())
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn client_text_never_becomes_markup() {
        assert_eq!(plain("<a href=\"x\">Tom & Jerry</a>"), "&lt;a href=\"x\"&gt;Tom &amp; Jerry&lt;/a&gt;");
    }

    /// Shows one real notification. Run by hand: `cargo test -p torroworkplace -- --ignored`.
    #[test]
    #[ignore = "shows a real desktop notification"]
    fn shows_a_real_notification() {
        send(tr("Template saved"), "Test · 0 windows").expect("notification shown");
    }
}
