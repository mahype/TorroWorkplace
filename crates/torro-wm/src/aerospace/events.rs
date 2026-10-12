//! Window and workspace events from `aerospace subscribe`.
//!
//! Task X.6 in `docs/tasks/plattformen.md`. AeroSpace 0.21 streams JSON lines while
//! `aerospace subscribe <event>…` runs, e.g. `{"_event":"focus-changed","windowId":432,
//! "workspace":"1"}`. It reports new windows (`window-detected`) and focus changes, but not
//! closed or moved windows. So every notice also triggers one `list-windows`, and comparing it
//! with the previous listing yields [`WmEvent::WindowClosed`] and [`WmEvent::WindowMoved`].
//! Closing or moving a window moves the focus, so the comparison runs right when it matters,
//! and nothing runs while the desktop is idle (no polling).

use std::path::PathBuf;
use std::process::Stdio;

use serde::Deserialize;

use super::json::{self, RawWindow};
use crate::{Events, WindowId, WmError, WmEvent, WorkspaceId};

/// Arguments for the subscription. No initial state: the first listing is taken before.
pub const SUBSCRIBE_ARGS: [&str; 5] =
    ["subscribe", "--no-send-initial", "window-detected", "focus-changed", "focused-workspace-changed"];

/// Capacity of the channel between the reader task and the core, as in the Hyprland backend.
const EVENT_QUEUE: usize = 256;

/// One line of `aerospace subscribe`, reduced to what we use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// `window-detected`: a new window, with its app's bundle id (or name) as class.
    Detected { window: WindowId, class: String, workspace: Option<WorkspaceId> },
    /// `focus-changed`: often the trace of a window that closed or moved.
    Focus,
    /// `focused-workspace-changed`.
    Workspace(WorkspaceId),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawNotice {
    #[serde(rename = "_event")]
    event: String,
    #[serde(default)]
    window_id: Option<u32>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    app_bundle_id: Option<String>,
    #[serde(default)]
    app_name: Option<String>,
}

/// One subscription line as a [`Notice`]; `None` for other events and lines we cannot read.
pub fn parse_notice(line: &str) -> Option<Notice> {
    let raw: RawNotice = serde_json::from_str(line).ok()?;
    let workspace = raw.workspace.filter(|name| !name.is_empty()).map(WorkspaceId);
    match raw.event.as_str() {
        "window-detected" => {
            let bundle = raw.app_bundle_id.filter(|id| !id.is_empty() && !id.starts_with("NULL-"));
            Some(Notice::Detected {
                window: WindowId(raw.window_id?.to_string()),
                class: bundle.or(raw.app_name).unwrap_or_default(),
                workspace,
            })
        }
        "focus-changed" => Some(Notice::Focus),
        "focused-workspace-changed" => workspace.map(Notice::Workspace),
        _ => None,
    }
}

/// What the comparison remembers of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub window: WindowId,
    pub class: String,
    pub workspace: WorkspaceId,
}

/// The normal windows of a listing as [`Known`] entries, in listing order.
pub fn known(raw: &[RawWindow]) -> Vec<Known> {
    raw.iter()
        .filter(|window| window.is_normal())
        .map(|window| Known { window: window.id(), class: window.class(), workspace: window.workspace() })
        .collect()
}

/// The events that turn `old` into `new`: opened and moved windows in the order of `new`,
/// then closed windows in the order of `old`.
pub fn diff(old: &[Known], new: &[Known]) -> Vec<WmEvent> {
    let mut events = Vec::new();
    for now in new {
        match old.iter().find(|before| before.window == now.window) {
            None => events.push(WmEvent::WindowOpened {
                window: now.window.clone(),
                class: now.class.clone(),
                workspace: now.workspace.clone(),
            }),
            Some(before) if before.workspace != now.workspace => {
                events.push(WmEvent::WindowMoved { window: now.window.clone(), workspace: now.workspace.clone() })
            }
            Some(_) => {}
        }
    }
    for before in old {
        if !new.iter().any(|now| now.window == before.window) {
            events.push(WmEvent::WindowClosed { window: before.window.clone() });
        }
    }
    events
}

/// Takes the first listing, starts `aerospace subscribe` and forwards events into the returned
/// channel from a background task. The task ends, and the subscription process is killed, when
/// AeroSpace ends the stream or the receiver is dropped.
pub async fn subscribe(program: PathBuf) -> Result<Events, WmError> {
    let first = list(&program).await?;
    let mut child = tokio::process::Command::new(&program)
        .args(SUBSCRIBE_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| WmError::NotRunning(format!("start aerospace subscribe: {err}")))?;
    let stdout = child.stdout.take().ok_or_else(|| WmError::Io("aerospace subscribe has no output".into()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(EVENT_QUEUE);
    tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};

        // Owned by the task so that ending the task kills the process (`kill_on_drop`).
        let _child = child;
        let mut known = first;
        let mut lines = BufReader::new(stdout).lines();
        loop {
            let line = tokio::select! {
                _ = tx.closed() => break,
                line = lines.next_line() => line,
            };
            // AeroSpace quitting closes the stream; so does a read error.
            let Ok(Some(line)) = line else {
                break;
            };
            let Some(notice) = parse_notice(&line) else {
                continue;
            };
            for event in handle(&program, notice, &mut known).await {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        }
    });
    Ok(rx)
}

/// The events for one notice. A failed listing keeps the previous one; a detected window that
/// the listing does not show yet is still reported from the notice itself.
async fn handle(program: &std::path::Path, notice: Notice, known: &mut Vec<Known>) -> Vec<WmEvent> {
    let mut events = Vec::new();
    if let Notice::Workspace(workspace) = &notice {
        events.push(WmEvent::WorkspaceFocused { workspace: workspace.clone() });
    }
    if let Ok(now) = list(program).await {
        events.extend(diff(known, &now));
        *known = now;
    }
    if let Notice::Detected { window, class, workspace: Some(workspace) } = notice
        && !known.iter().any(|entry| entry.window == window)
    {
        events.push(WmEvent::WindowOpened {
            window: window.clone(),
            class: class.clone(),
            workspace: workspace.clone(),
        });
        known.push(Known { window, class, workspace });
    }
    events
}

async fn list(program: &std::path::Path) -> Result<Vec<Known>, WmError> {
    let answer =
        super::cli::run(program, &["list-windows", "--all", "--json", "--format", json::WINDOW_FORMAT]).await?;
    Ok(known(&json::parse_raw_windows(&answer)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(window: &str, class: &str, workspace: &str) -> Known {
        Known { window: WindowId(window.into()), class: class.into(), workspace: WorkspaceId(workspace.into()) }
    }

    #[test]
    fn parses_the_three_notices_we_use() {
        assert_eq!(
            parse_notice(
                r#"{"_event":"window-detected","windowId":512,"workspace":"2","appBundleId":"com.apple.Notes","appName":"Notes"}"#
            ),
            Some(Notice::Detected {
                window: WindowId("512".into()),
                class: "com.apple.Notes".into(),
                workspace: Some(WorkspaceId("2".into())),
            })
        );
        assert_eq!(parse_notice(r#"{"_event":"focus-changed","windowId":432,"workspace":"1"}"#), Some(Notice::Focus));
        assert_eq!(
            parse_notice(r#"{"_event":"focused-workspace-changed","prevWorkspace":"1","workspace":"3"}"#),
            Some(Notice::Workspace(WorkspaceId("3".into())))
        );
    }

    #[test]
    fn detected_window_without_bundle_id_uses_the_app_name() {
        let line = r#"{"_event":"window-detected","windowId":7,"appBundleId":"NULL-APP-BUNDLE-ID","appName":"Tool"}"#;
        assert_eq!(
            parse_notice(line),
            Some(Notice::Detected { window: WindowId("7".into()), class: "Tool".into(), workspace: None })
        );
    }

    #[test]
    fn ignores_other_events_and_garbage() {
        assert_eq!(parse_notice(r#"{"_event":"mode-changed","mode":"main"}"#), None);
        assert_eq!(parse_notice(r#"{"_event":"window-detected","workspace":"1"}"#), None);
        assert_eq!(parse_notice("not json"), None);
    }

    #[test]
    fn diff_reports_opened_moved_and_closed() {
        let old = [entry("1", "a", "1"), entry("2", "b", "1"), entry("3", "c", "2")];
        let new = [entry("1", "a", "1"), entry("3", "c", "4"), entry("9", "d", "1")];
        assert_eq!(
            diff(&old, &new),
            vec![
                WmEvent::WindowMoved { window: WindowId("3".into()), workspace: WorkspaceId("4".into()) },
                WmEvent::WindowOpened {
                    window: WindowId("9".into()),
                    class: "d".into(),
                    workspace: WorkspaceId("1".into()),
                },
                WmEvent::WindowClosed { window: WindowId("2".into()) },
            ]
        );
        assert!(diff(&new, &new).is_empty());
    }
}
