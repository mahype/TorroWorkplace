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
//!
//! The sender ("Torro Workplace", with the app icon) is not a plain text on macOS and Windows:
//! the system takes it from an identity of the app, so `platform` sets that up before the
//! first notification. Where that is not possible, the notification still appears and carries
//! the name in its subtitle instead.

use torro_core::i18n::{tr, trf};
use torro_ipc::{ApprovalInfo, Response};

/// The app name shown with each notification (also the inbox popups, `inbox_notify`).
pub(crate) const APP_NAME: &str = "Torro Workplace";

/// The identity of the app for the notification system: the bundle id on macOS (the one
/// `packaging/macos/Info.plist.in` builds from the app id `torro-workplace`, so the two must
/// stay equal) and the AppUserModelID on Windows. Reverse-DNS, no spaces, at most 129 characters
/// (the AppUserModelID limit). Never change it once released: the system keys the user's
/// notification settings by it.
#[cfg_attr(target_os = "linux", allow(dead_code))]
const APP_ID: &str = "io.github.mahype.torro-workplace";

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
/// daemon must not wait for it. A failure (no notification service, a thread that cannot
/// start) is only printed to stderr and never ends the daemon.
pub fn show(summary: String, body: String) {
    let spawned = std::thread::Builder::new().name("notify".into()).spawn(move || {
        if let Err(err) = send(&summary, &body) {
            eprintln!("torroworkplace: cannot show notification: {err}");
        }
    });
    if let Err(err) = spawned {
        eprintln!("torroworkplace: cannot show notification: {err}");
    }
}

/// The title and body of the question for `approval`: who asks, and what would happen. Both hold
/// text the client chose (its name, the titles in the summary): [`ask`] shows them as plain text.
///
/// A question of an agent (task E2.5) says "asks for permission": allowing it runs nothing, it
/// only answers the agent.
pub fn question(approval: &ApprovalInfo) -> (String, String) {
    let client = [("client", &approval.client_name as &dyn std::fmt::Display)];
    let title = match crate::approvals::is_question(approval) {
        true => trf("{client} asks for permission", &client),
        false => trf("{client} asks for approval", &client),
    };
    (title, approval.summary.clone())
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
/// Linux and the BSDs show the buttons. macOS and Windows show the question without them (notify-rust
/// has no buttons there) under the app's own sender, so the person decides in the control centre
/// there.
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
pub(crate) fn plain(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn send(summary: &str, body: &str) -> notify_rust::error::Result<()> {
    let mut notification = notify_rust::Notification::new();
    notification.appname(APP_NAME).summary(summary).body(body);
    platform::name_sender(&mut notification);
    notification.show().map(|_| ())
}

/// The subtitle that stands in for the sender when the system cannot show our own name.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn fallback_subtitle(named: bool) -> Option<&'static str> {
    (!named).then_some(APP_NAME)
}

/// macOS: the sender is the application whose bundle id `mac-notification-sys` reports; it
/// ignores `appname`. Without `set_application` it asks AppleScript for an application called
/// "use_default" and ends up as Finder, so we always set our own id first (once per process).
///
/// `set_application` only succeeds when Launch Services knows an app with that id. That is
/// the case when the daemon runs from `Torro Workplace.app` (it registers itself on launch)
/// or the app is installed. A plain `torroworkplace` binary from `cargo install` or the AUR
/// package, with no app installed, fails; the library then falls back to Terminal's bundle id.
/// Terminal is the honest guess for a program started from a shell, and the notification
/// carries "Torro Workplace" in its subtitle so it is still clear who is talking.
///
/// Notifications use `NSUserNotificationCenter`, which does not ask for permission itself: if
/// the user switched notifications off for the app in System Settings, nothing is shown and
/// nothing fails, and the daemon never notices.
#[cfg(target_os = "macos")]
mod platform {
    use std::sync::OnceLock;

    use super::{APP_ID, fallback_subtitle};

    /// Whether the notification system will show our own name. Decided once; `OnceLock` also
    /// holds back a second notification until the first one has set the application, which
    /// the library allows only once.
    fn named() -> bool {
        static NAMED: OnceLock<bool> = OnceLock::new();
        *NAMED.get_or_init(|| match notify_rust::set_application(APP_ID) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("torroworkplace: notifications cannot use {APP_ID} as sender ({err}); falling back");
                false
            }
        })
    }

    pub fn name_sender(notification: &mut notify_rust::Notification) {
        if let Some(subtitle) = fallback_subtitle(named()) {
            notification.subtitle(subtitle);
        }
    }
}

/// Windows: a toast needs the AppUserModelID of an app the shell knows. Normally a Start menu
/// shortcut carries it, but an unpackaged program can register one without a shortcut: the key
/// `HKCU\Software\Classes\AppUserModelId\<id>` with `DisplayName` and `IconUri` is enough (the
/// way `tauri-winrt-notification` documents in its `unpackaged_app` example), and it needs no
/// administrator rights. It is written the first time a notification is sent and left alone
/// while it is up to date.
///
/// The icon: `IconUri` wants a file, and the release is a single `.exe`. So the 256 px PNG
/// inside `packaging/windows/torro-workplace.ico` is compiled in and written to the state
/// directory when it is missing or different.
///
/// If the registration fails (read-only profile), the toast goes out under PowerShell's id,
/// which Windows always shows; it is then named by its subtitle.
#[cfg(windows)]
mod platform {
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    use torro_core::AppId;
    use torro_core::paths::AppPaths;
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::{APP_ID, fallback_subtitle, icon_png, registry_values, toast_app_id};

    /// The icon of the executable, which `build.rs` embeds as well.
    const ICON: &[u8] = include_bytes!("../../../packaging/windows/torro-workplace.ico");

    /// File name of the PNG in the state directory.
    const ICON_FILE: &str = "notification-icon.png";

    pub fn name_sender(notification: &mut notify_rust::Notification) {
        let registered = match register() {
            Ok(()) => true,
            Err(err) => {
                eprintln!("torroworkplace: cannot register {APP_ID} for notifications ({err}); using PowerShell's id");
                false
            }
        };
        notification.app_id(toast_app_id(registered));
        if let Some(subtitle) = fallback_subtitle(registered) {
            notification.subtitle(subtitle);
        }
    }

    /// Writes the icon and the registry key, once per process. A failure is tried again with
    /// the next notification; a repeat after success is skipped.
    fn register() -> io::Result<()> {
        static DONE: AtomicBool = AtomicBool::new(false);
        if DONE.load(Ordering::Relaxed) {
            return Ok(());
        }
        let icon = write_icon();
        register_in(&format!(r"Software\Classes\AppUserModelId\{APP_ID}"), &registry_values(icon.as_deref()))?;
        DONE.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// The PNG in the state directory, or `None` when there is none (the toast then has no
    /// icon but still the right name).
    fn write_icon() -> Option<PathBuf> {
        let png = icon_png(ICON)?;
        let dir = AppPaths::for_app(AppId::WORKPLACE)?.state;
        let path = dir.join(ICON_FILE);
        if fs::read(&path).is_ok_and(|current| current == png) {
            return Some(path);
        }
        fs::create_dir_all(&dir).ok()?;
        fs::write(&path, png).ok()?;
        Some(path)
    }

    /// Makes `values` the values of `subkey` under `HKEY_CURRENT_USER`, creating the key.
    /// Values that are already right are not written again. Tests use a key of their own.
    fn register_in(subkey: &str, values: &[(&str, String)]) -> io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(subkey)?;
        for (name, value) in values {
            if key.get_value::<String, _>(name).ok().as_ref() != Some(value) {
                key.set_value(name, value)?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use std::path::Path;

        use super::*;

        /// A key of its own for one test, removed again when the test ends, even by a failure.
        struct TestKey(String);

        impl TestKey {
            fn new(name: &str) -> TestKey {
                TestKey(format!(r"Software\TorroWorkplaceTest\notify-{name}-{}", std::process::id()))
            }
        }

        impl Drop for TestKey {
            fn drop(&mut self) {
                let user = RegKey::predef(HKEY_CURRENT_USER);
                let _ = user.delete_subkey_all(&self.0);
                // Only succeeds when no other test key is left in it; the parent stays otherwise.
                let _ = user.delete_subkey(r"Software\TorroWorkplaceTest");
            }
        }

        fn read(key: &TestKey, name: &str) -> io::Result<String> {
            RegKey::predef(HKEY_CURRENT_USER).open_subkey(&key.0)?.get_value(name)
        }

        #[test]
        fn registration_writes_and_updates_the_values() {
            let key = TestKey::new("values");
            let first = registry_values(Some(Path::new(r"C:\one\icon.png")));
            register_in(&key.0, &first).expect("register");
            assert_eq!(read(&key, "DisplayName").expect("name"), "Torro Workplace");
            assert_eq!(read(&key, "IconUri").expect("icon"), r"C:\one\icon.png");

            // Registering again is fine, and a moved icon replaces the old path.
            register_in(&key.0, &first).expect("register twice");
            register_in(&key.0, &registry_values(Some(Path::new(r"C:\two\icon.png")))).expect("update");
            assert_eq!(read(&key, "IconUri").expect("icon"), r"C:\two\icon.png");
        }
    }
}

/// Linux: the notification service names the sender from `appname`; nothing to prepare.
#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub fn name_sender(_notification: &mut notify_rust::Notification) {}
}

/// The AppUserModelID for a toast. `registered`: our own key is in the registry. PowerShell's
/// id (the constant of `Toast::POWERSHELL_APP_ID` in `tauri-winrt-notification`) is the only
/// one every Windows 10/11 shows without registration.
#[cfg_attr(not(windows), allow(dead_code))]
fn toast_app_id(registered: bool) -> &'static str {
    if registered { APP_ID } else { r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe" }
}

/// The values of the `AppUserModelId` registry key: the name shown as sender and, when there
/// is an icon file, its path. `IconBackgroundColor` `0` means transparent, so the PNG's own
/// transparency shows.
#[cfg_attr(not(windows), allow(dead_code))]
fn registry_values(icon: Option<&std::path::Path>) -> Vec<(&'static str, String)> {
    let mut values = vec![("DisplayName", APP_NAME.to_string())];
    if let Some(icon) = icon {
        values.push(("IconUri", icon.display().to_string()));
        values.push(("IconBackgroundColor", "0".to_string()));
    }
    values
}

/// The largest PNG image inside an `.ico` file, or `None` if there is none. Modern `.ico`
/// files (ours too) store the big sizes as PNG; the toast icon must be a plain image file, and
/// PNG is the format Windows documents for it.
///
/// Layout: a 6-byte header (`0, 0`, type `1`, count) and then 16 bytes per image: width and
/// height (`0` means 256), 10 bytes of colour info, the data size (u32) and the data offset
/// (u32), all little endian.
#[cfg_attr(not(windows), allow(dead_code))]
fn icon_png(ico: &[u8]) -> Option<&[u8]> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let count = usize::from(u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?));
    let mut best: Option<(u32, &[u8])> = None;
    for index in 0..count {
        let start = 6 + index * 16;
        let entry = ico.get(start..start + 16)?;
        let width = if entry[0] == 0 { 256 } else { u32::from(entry[0]) };
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let Some(data) = ico.get(offset..offset.checked_add(size)?) else { continue };
        if data.starts_with(PNG_SIGNATURE) && best.is_none_or(|(w, _)| width > w) {
            best = Some((width, data));
        }
    }
    best.map(|(_, data)| data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_setups::TemplateSummary;

    fn summary(name: &str, windows: usize) -> TemplateSummary {
        TemplateSummary {
            id: "dev".into(),
            name: name.into(),
            windows,
            updated: chrono::Utc::now(),
            shortcut: None,
            startup: None,
            workspace: "1".into(),
            monitor: None,
        }
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
            details: Vec::new(),
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

    #[test]
    fn app_id_is_what_the_macos_bundle_gets() {
        // `make-app.sh` builds `io.github.mahype.@APPID@` with the app id `torro-workplace`.
        assert_eq!(APP_ID, "io.github.mahype.torro-workplace");
        // The AppUserModelID limit and allowed characters.
        assert!(APP_ID.len() <= 129 && !APP_ID.contains(char::is_whitespace));
    }

    #[test]
    fn subtitle_names_the_app_only_when_the_system_cannot() {
        assert_eq!(fallback_subtitle(true), None);
        assert_eq!(fallback_subtitle(false), Some("Torro Workplace"));
    }

    #[test]
    fn toast_uses_our_id_only_when_it_is_registered() {
        assert_eq!(toast_app_id(true), APP_ID);
        assert!(toast_app_id(false).ends_with(r"WindowsPowerShell\v1.0\powershell.exe"));
    }

    #[test]
    fn registry_values_name_the_app_and_point_at_the_icon() {
        assert_eq!(registry_values(None), vec![("DisplayName", "Torro Workplace".to_string())]);
        let with_icon = registry_values(Some(std::path::Path::new("icon.png")));
        assert_eq!(
            with_icon,
            vec![
                ("DisplayName", "Torro Workplace".to_string()),
                ("IconUri", "icon.png".to_string()),
                ("IconBackgroundColor", "0".to_string()),
            ]
        );
    }

    /// An `.ico` with the given `(width byte, data)` images.
    fn ico(images: &[(u8, &[u8])]) -> Vec<u8> {
        let mut file = vec![0, 0, 1, 0];
        file.extend_from_slice(&(images.len() as u16).to_le_bytes());
        let mut offset = 6 + images.len() * 16;
        for (width, data) in images {
            file.extend_from_slice(&[*width, *width, 0, 0, 1, 0, 32, 0]);
            file.extend_from_slice(&(data.len() as u32).to_le_bytes());
            file.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += data.len();
        }
        for (_, data) in images {
            file.extend_from_slice(data);
        }
        file
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nbig";
    const SMALL_PNG: &[u8] = b"\x89PNG\r\n\x1a\nsmall";

    #[test]
    fn icon_png_picks_the_largest_png() {
        // Width byte 0 means 256; a bitmap (no PNG signature) is never chosen.
        let file = ico(&[(16, SMALL_PNG), (0, PNG), (48, b"BM bitmap data")]);
        assert_eq!(icon_png(&file), Some(PNG));
    }

    #[test]
    fn icon_png_survives_broken_files() {
        assert_eq!(icon_png(&[]), None);
        assert_eq!(icon_png(&ico(&[(32, b"BM bitmap data")])), None);
        // The header claims more images than there are bytes.
        let mut cut = ico(&[(32, PNG)]);
        cut[4] = 9;
        assert_eq!(icon_png(&cut), None);
        // The data points beyond the end of the file.
        let mut beyond = ico(&[(32, PNG)]);
        beyond[6 + 8..6 + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(icon_png(&beyond), None);
    }

    #[test]
    fn the_shipped_icon_has_a_png_for_the_toast() {
        let shipped = include_bytes!("../../../packaging/windows/torro-workplace.ico");
        let png = icon_png(shipped).expect("a PNG image in torro-workplace.ico");
        assert!(png.starts_with(b"\x89PNG"));
        assert!(png.len() > 100);
    }

    /// Shows one real notification. Run by hand: `cargo test -p torroworkplace -- --ignored`.
    #[test]
    #[ignore = "shows a real desktop notification"]
    fn shows_a_real_notification() {
        send(tr("Template saved"), "Test · 0 windows").expect("notification shown");
        // macOS hands the notification to the system on a thread of its own when the handle
        // is dropped; leave it time to arrive before the test process ends.
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    #[test]
    fn a_question_of_an_agent_asks_for_permission() {
        let at = chrono::Utc::now();
        let approval = ApprovalInfo {
            id: "9c1f0a7e3b2d4c58".into(),
            client: "h1".into(),
            client_name: "Claude Code hooks".into(),
            command: crate::approvals::QUESTION_COMMAND.into(),
            summary: "Claude Code wants to use Bash: ls".into(),
            details: Vec::new(),
            created: at,
            expires: at,
            decided: None,
            state: torro_ipc::ApprovalState::Pending,
        };
        let (title, body) = question(&approval);
        assert_eq!(title, trf("{client} asks for permission", &[("client", &"Claude Code hooks")]));
        assert_eq!(body, approval.summary);
    }
}
