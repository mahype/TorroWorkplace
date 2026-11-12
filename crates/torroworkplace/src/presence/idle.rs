//! Whether the person is at the computer, for the heartbeat of the presence (`docs/chat.md`,
//! decision 10): `online` while they are, `away` after the system's idle time, nothing while the
//! screen is locked. Read from what the system already tells, without a library of its own:
//!
//! - Linux: systemd-logind's session (`loginctl show-session auto`), whose `LockedHint` lockers
//!   such as hyprlock set through `loginctl lock-session`, and `IdleHint`, which idle daemons set.
//! - macOS: the idle time of the input devices (`ioreg -c IOHIDSystem`, `HIDIdleTime`); a locked
//!   screen is not told, so after five minutes without input it counts as away.
//! - Elsewhere, and whenever the answer cannot be read: [`Activity::Active`]. The plan says so: a
//!   heartbeat the person switched on goes out rather than not at all.
//!
//! While the computer sleeps the core does not run, so nothing goes out then either.

use std::time::Duration;

/// What the heartbeat says about the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// At the computer: `online`.
    Active,
    /// Not touched it for a while: `away`.
    Idle,
    /// The screen is locked: nothing is sent.
    Locked,
}

/// How long without input counts as away where the system tells only the idle time (macOS).
#[cfg(target_os = "macos")]
pub const AWAY_AFTER: Duration = Duration::from_secs(5 * 60);

/// How long the system's answer may take; slower counts as not told.
const ASK_FOR: Duration = Duration::from_secs(5);

/// What the system tells about the person now ([`Activity::Active`] when it tells nothing).
pub async fn activity() -> Activity {
    match tokio::time::timeout(ASK_FOR, ask()).await {
        Ok(Some(activity)) => activity,
        _ => Activity::Active,
    }
}

#[cfg(target_os = "linux")]
async fn ask() -> Option<Activity> {
    let output = tokio::process::Command::new("loginctl")
        .args(["show-session", "auto", "-p", "LockedHint", "-p", "IdleHint"])
        .output()
        .await
        .ok()?;
    output.status.success().then(|| logind(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(target_os = "macos")]
async fn ask() -> Option<Activity> {
    let output = tokio::process::Command::new("ioreg").args(["-c", "IOHIDSystem", "-d", "4"]).output().await.ok()?;
    let idle = hid_idle(&String::from_utf8_lossy(&output.stdout))?;
    Some(if idle >= AWAY_AFTER { Activity::Idle } else { Activity::Active })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn ask() -> Option<Activity> {
    None
}

/// The activity in the lines `loginctl show-session -p LockedHint -p IdleHint` prints
/// (`LockedHint=yes`, `IdleHint=no`).
#[cfg_attr(not(any(target_os = "linux", test)), expect(dead_code, reason = "read on Linux only"))]
fn logind(text: &str) -> Activity {
    let yes = |key: &str| text.lines().any(|line| line.trim() == format!("{key}=yes"));
    if yes("LockedHint") {
        Activity::Locked
    } else if yes("IdleHint") {
        Activity::Idle
    } else {
        Activity::Active
    }
}

/// The idle time in the output of `ioreg -c IOHIDSystem`: the line `"HIDIdleTime" = <nanoseconds>`.
#[cfg_attr(not(any(target_os = "macos", test)), expect(dead_code, reason = "read on macOS only"))]
fn hid_idle(text: &str) -> Option<Duration> {
    let line = text.lines().find(|line| line.contains("\"HIDIdleTime\""))?;
    let nanos: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(Duration::from_nanos(nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logind_tells_locked_before_idle() {
        assert_eq!(logind("LockedHint=yes\nIdleHint=yes\n"), Activity::Locked);
        assert_eq!(logind("LockedHint=no\nIdleHint=yes\n"), Activity::Idle);
        assert_eq!(logind("LockedHint=no\nIdleHint=no\n"), Activity::Active);
        assert_eq!(logind(""), Activity::Active, "nothing told");
    }

    #[test]
    fn the_idle_time_of_the_input_devices_is_read_in_nanoseconds() {
        let text = "    | |   \"HIDIdleTime\" = 301000000000\n";
        assert_eq!(hid_idle(text), Some(Duration::from_secs(301)));
        assert_eq!(hid_idle("nothing"), None);
    }
}
