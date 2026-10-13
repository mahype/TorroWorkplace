//! The suite's apps as the core reports them (`Request::ListApps`): every app of
//! `torro_core::AppId::ALL` except the core itself, and whether it is installed.
//!
//! "Installed" means the binary was found next to the core's executable or on `PATH`, the same
//! places `Request::OpenApp` starts apps from, so a listed app that is installed can be opened.
//!
//! The inbox is three entries (task E8.1): its mail window, its notifications window and its hub,
//! one binary `torroinbox` with three window classes. All three are installed or none.
//!
//! `apps.open_app` with a `target` (task E8.7a) opens a mail or a task: [`Target`] reads what the
//! request names, the core looks it up, and [`target_command`] builds the command line from what it
//! found ([`Opened`]), never from the target's text.

use std::path::{Path, PathBuf};

use torro_core::AppId;
use torro_ipc::AppInfo;

/// The suite's apps with their install state, looked up now (a handful of file checks).
pub fn list() -> Vec<AppInfo> {
    list_in(&search_dirs())
}

/// [`list`] with the folders to look in; tests pass temporary ones.
fn list_in(dirs: &[PathBuf]) -> Vec<AppInfo> {
    AppId::ALL
        .iter()
        // The core is the service answering this; it is not one of the apps it opens.
        .filter(|app| **app != AppId::WORKPLACE)
        .map(|app| AppInfo {
            binary: app.binary.to_string(),
            name: app.display_name.to_string(),
            window_class: app.window_class.to_string(),
            installed: is_installed(app.binary, dirs),
        })
        .collect()
}

/// The folders an app binary is looked for in: the folder of this executable, then `PATH`.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// Whether a file named `binary` (plus `.exe` on Windows) exists in one of `dirs`.
fn is_installed(binary: &str, dirs: &[PathBuf]) -> bool {
    let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().any(|dir| dir.join(&file).is_file())
}

/// What `apps.open_app` is asked to show (`target`, task E8.7a), as the request names it. The core
/// looks it up before anything starts (`Core::open_target`): only what it finds goes on the
/// command line ([`Opened`]), never the target's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// `mail:<address>:<Message-ID>`: a task's link to a mail, as the account id and the
    /// Message-ID ([`torro_pim::model::Link::mail_ref`]).
    Mail { account: &'a str, message_id: &'a str },
    /// `task:<uid>`: a task by its iCalendar UID.
    Task { uid: &'a str },
}

/// The start of a task target.
const TASK_TARGET: &str = "task:";

impl<'a> Target<'a> {
    /// The target `text` names; `Err` with the forms there are for anything else, and for a uid
    /// that is empty or holds control characters.
    pub fn parse(text: &'a str) -> Result<Target<'a>, &'static str> {
        const FORMS: &str = "a target is mail:<address>:<Message-ID> or task:<uid>";
        if let Some(uid) = text.strip_prefix(TASK_TARGET) {
            return match uid.is_empty() || uid.chars().any(char::is_control) {
                true => Err(FORMS),
                false => Ok(Target::Task { uid }),
            };
        }
        // Only the prefix decides; the parts are borrowed from `text`, not from a temporary link.
        let link = torro_pim::model::Link::new(text);
        let (account, message_id) = link.mail_ref().ok_or(FORMS)?;
        let (account, message_id) = (account.len(), message_id.len());
        Ok(Target::Mail { account: &text[..account], message_id: &text[text.len() - message_id..] })
    }

    /// The entry of the suite this target opens in: the inbox's mail window, or the tasks app.
    pub fn app(self) -> AppId {
        match self {
            Target::Mail { .. } => AppId::INBOX_MAIL,
            Target::Task { .. } => AppId::TASKS,
        }
    }

    /// `app` may show it: any window of the inbox (they share a binary) for a mail, the tasks app
    /// for a task.
    pub fn opens_in(self, app: AppId) -> bool {
        app.binary == self.app().binary
    }
}

/// What a new window shows, as the core found it ([`target_command`]): the store's id of a mail
/// (`m:…`) or the uid of a cached task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    Mail { message: String },
    Task { uid: String },
}

impl Opened {
    /// The entry of the suite whose window shows it.
    pub fn app(&self) -> AppId {
        match self {
            Opened::Mail { .. } => AppId::INBOX_MAIL,
            Opened::Task { .. } => AppId::TASKS,
        }
    }
}

/// The command line that starts a window of `program` (the path of [`Opened::app`]'s binary)
/// showing `opened`: `torroinbox --view mail --message=m:…` or `torrotasks --task=<uid>`. One
/// argument each, with `=`, so a value that starts with `-` stays a value; started without a
/// shell.
pub fn target_command(program: String, opened: &Opened) -> Vec<String> {
    let mut command = opened.app().command(program);
    command.push(match opened {
        Opened::Mail { message } => format!("--message={message}"),
        Opened::Task { uid } => format!("--task={uid}"),
    });
    command
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn binary(name: &str) -> String {
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    }

    #[test]
    fn detects_installed_apps_in_the_given_dirs() {
        let next_to_exe = tempfile::tempdir().expect("temp dir");
        let on_path = tempfile::tempdir().expect("temp dir");
        let empty = tempfile::tempdir().expect("temp dir");
        fs::write(on_path.path().join(binary("torrocalendar")), b"").expect("write binary");
        // A directory with the right name is not an installed binary.
        fs::create_dir(next_to_exe.path().join(binary("torrotasks"))).expect("create dir");

        let dirs = vec![next_to_exe.path().to_path_buf(), empty.path().to_path_buf(), on_path.path().to_path_buf()];
        assert!(is_installed("torrocalendar", &dirs));
        assert!(!is_installed("torrotasks", &dirs));
        assert!(!is_installed("torrocalendar", &[empty.path().to_path_buf()]));
        assert!(!is_installed("torrocalendar", &[]));
    }

    #[test]
    fn lists_every_app_but_the_core_with_its_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torrotasks")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        assert_eq!(apps.len(), AppId::ALL.len() - 1);
        assert!(apps.iter().all(|app| app.binary != AppId::WORKPLACE.binary));
        let tasks = apps.iter().find(|app| app.binary == "torrotasks").expect("tasks listed");
        assert_eq!(
            tasks,
            &AppInfo {
                binary: "torrotasks".into(),
                name: AppId::TASKS.display_name.into(),
                window_class: AppId::TASKS.window_class.into(),
                installed: true,
            }
        );
        let calendar = apps.iter().find(|app| app.binary == "torrocalendar").expect("calendar listed");
        assert!(!calendar.installed);
    }

    /// Task E8.7a: targets are read in their two forms, open in their app and become one argument.
    #[test]
    fn targets_name_their_app_and_become_one_argument() {
        let mail = Target::parse("mail:anna@mailbox.org:<a:1@example.org>").expect("mail");
        assert_eq!(mail, Target::Mail { account: "mail:anna@mailbox.org", message_id: "<a:1@example.org>" });
        assert!(mail.opens_in(AppId::INBOX_MAIL) && mail.opens_in(AppId::INBOX) && !mail.opens_in(AppId::TASKS));
        let task = Target::parse("task:7d1e-5b2a").expect("task");
        assert_eq!(task, Target::Task { uid: "7d1e-5b2a" });
        assert!(task.opens_in(AppId::TASKS) && !task.opens_in(AppId::INBOX_MAIL));
        for bad in ["", "task:", "task:a\nb", "inbox:0a1b", "mail:anna@mailbox.org", "https://example.org"] {
            assert!(Target::parse(bad).is_err(), "{bad:?}");
        }
        let opened = Opened::Mail { message: "m:0a1b".into() };
        assert_eq!(
            target_command("/bin/torroinbox".into(), &opened),
            ["/bin/torroinbox", "--view", "mail", "--message=m:0a1b"]
        );
        let opened = Opened::Task { uid: "--version".into() };
        assert_eq!(target_command("torrotasks".into(), &opened), ["torrotasks", "--task=--version"]);
    }

    #[test]
    fn the_inbox_is_listed_as_three_windows() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torroinbox")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        let inbox: Vec<(&str, &str, bool)> = apps
            .iter()
            .filter(|app| app.binary == "torroinbox")
            .map(|app| (app.window_class.as_str(), app.name.as_str(), app.installed))
            .collect();
        assert_eq!(
            inbox,
            [
                ("torro-inbox", AppId::INBOX.display_name, true),
                ("torro-inbox-mail", AppId::INBOX_MAIL.display_name, true),
                ("torro-inbox-notifications", AppId::INBOX_NOTIFICATIONS.display_name, true),
            ]
        );
    }
}
