//! App identity.
//!
//! The app id is the one name a window carries everywhere: the Wayland app id and X11
//! class on Linux (Hyprland window rules match on it), the terminal `--class` a TUI is
//! started with, the key in AeroSpace's `on-window-detected`, and what workspace-setups
//! stores to find the window again. It never changes once an app has shipped.
//!
//! One binary may have several entries (task E8.1): `torroinbox` opens three kinds of window –
//! mail, notifications and the hub – each with its own window class, name and start arguments,
//! so window rules, the app switcher and workspace templates tell them apart.

/// An app of the suite, or one kind of window of an app with several ([`AppId::INBOX_MAIL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppId {
    /// Window class / app id, e.g. `torro-tasks`. Unique in [`AppId::ALL`].
    pub window_class: &'static str,
    /// Binary name, e.g. `torrotasks`. Several entries may share one ([`AppId::find`]).
    pub binary: &'static str,
    /// Directory name below the per-OS config, data and state roots.
    pub dir: &'static str,
    /// Human name for titles and launchers.
    pub display_name: &'static str,
    /// The arguments that start the binary on this entry's window, after the program name: `["ui"]`
    /// for the control centre, `["--view", "mail"]` for the inbox's mail window, none for an app
    /// with one window. Whoever starts an entry runs `binary` with exactly these ([`AppId::command`]).
    pub args: &'static [&'static str],
}

impl AppId {
    /// The core: background service, tray icon, control centre. Its window is the control centre,
    /// `torroworkplace ui`; without arguments the binary is the service.
    pub const WORKPLACE: AppId = AppId {
        window_class: "torro-workplace",
        binary: "torroworkplace",
        dir: "torroworkplace",
        display_name: "Torro Workplace",
        args: &["ui"],
    };
    pub const TASKS: AppId = AppId {
        window_class: "torro-tasks",
        binary: "torrotasks",
        dir: "torrotasks",
        display_name: "Torro Tasks",
        args: &[],
    };
    pub const CALENDAR: AppId = AppId {
        window_class: "torro-calendar",
        binary: "torrocalendar",
        dir: "torrocalendar",
        display_name: "Torro Calendar",
        args: &[],
    };

    /// The inbox's hub (task E8.1): what to decide or hand on. `torroinbox` without `--view` starts
    /// it, so it comes first of the inbox's entries in [`AppId::ALL`] and the binary alone names it.
    pub const INBOX: AppId = AppId {
        window_class: "torro-inbox",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro Inbox",
        args: &[],
    };

    /// The inbox's mail window (task E8.1): `torroinbox --view mail`. Same binary and state directory
    /// as [`AppId::INBOX`].
    pub const INBOX_MAIL: AppId = AppId {
        window_class: "torro-inbox-mail",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro Inbox Mail",
        args: &["--view", "mail"],
    };

    /// The inbox's notifications window (task E8.1): `torroinbox --view notifications`, every source
    /// but mail. Same binary and state directory as [`AppId::INBOX`].
    pub const INBOX_NOTIFICATIONS: AppId = AppId {
        window_class: "torro-inbox-notifications",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro Inbox Notifications",
        args: &["--view", "notifications"],
    };

    /// Every app of the suite, in launcher order. Of the entries sharing a binary, the one the
    /// binary starts without arguments comes first: a lookup by binary finds it.
    pub const ALL: &'static [AppId] =
        &[AppId::WORKPLACE, AppId::INBOX, AppId::INBOX_MAIL, AppId::INBOX_NOTIFICATIONS, AppId::TASKS, AppId::CALENDAR];

    /// The entry of [`AppId::ALL`] that `name` names: a window class (`torro-inbox-mail`) names its
    /// entry; a binary (`torroinbox`) the first entry with that binary. `None` for anything else.
    ///
    /// This is how `apps.open_app` should read its `app` (binary or window class), so the three
    /// windows of the inbox can each be opened while every binary keeps working.
    pub fn find(name: &str) -> Option<AppId> {
        let by_class = AppId::ALL.iter().find(|app| app.window_class == name);
        by_class.or_else(|| AppId::ALL.iter().find(|app| app.binary == name)).copied()
    }

    /// The shortest name that [`AppId::find`] resolves to this entry: its binary when the binary
    /// alone names it, else its window class (`torro-inbox-mail`). What the app switcher sends with
    /// `apps.open_app`, so apps with one window are opened by binary exactly as before.
    pub fn open_name(self) -> &'static str {
        if AppId::find(self.binary) == Some(self) { self.binary } else { self.window_class }
    }

    /// The command line that starts this entry's window: `program` (the path of [`AppId::binary`]
    /// as the caller found it), then [`AppId::args`].
    pub fn command(self, program: impl Into<String>) -> Vec<String> {
        let mut command = vec![program.into()];
        command.extend(self.args.iter().map(|arg| (*arg).to_string()));
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inbox_has_three_entries_with_their_own_window_class() {
        for app in [AppId::INBOX, AppId::INBOX_MAIL, AppId::INBOX_NOTIFICATIONS] {
            assert!(AppId::ALL.contains(&app), "{} is listed", app.window_class);
            assert_eq!((app.binary, app.dir), ("torroinbox", "torroinbox"), "one program, one state");
        }
        assert_eq!(AppId::INBOX.window_class, "torro-inbox");
        assert_eq!(AppId::INBOX_MAIL.window_class, "torro-inbox-mail");
        assert_eq!(AppId::INBOX_NOTIFICATIONS.window_class, "torro-inbox-notifications");
        assert_eq!(AppId::INBOX_MAIL.args, ["--view", "mail"]);
        assert_eq!(AppId::INBOX_NOTIFICATIONS.args, ["--view", "notifications"]);
        assert!(AppId::INBOX.args.is_empty(), "without --view the inbox is the hub");
    }

    #[test]
    fn window_classes_are_unique() {
        for (i, app) in AppId::ALL.iter().enumerate() {
            assert!(
                AppId::ALL[i + 1..].iter().all(|other| other.window_class != app.window_class),
                "{} twice",
                app.window_class
            );
        }
    }

    #[test]
    fn a_binary_names_its_first_entry_and_a_class_its_own() {
        assert_eq!(AppId::find("torroinbox"), Some(AppId::INBOX));
        assert_eq!(AppId::find("torro-inbox-mail"), Some(AppId::INBOX_MAIL));
        assert_eq!(AppId::find("torro-inbox-notifications"), Some(AppId::INBOX_NOTIFICATIONS));
        assert_eq!(AppId::find("torrotasks"), Some(AppId::TASKS));
        assert_eq!(AppId::find("torro-tasks"), Some(AppId::TASKS));
        assert_eq!(AppId::find("torro-compose"), None);
        assert_eq!(AppId::find(""), None);
    }

    #[test]
    fn every_entry_is_found_again_by_its_open_name() {
        for &app in AppId::ALL {
            assert_eq!(AppId::find(app.open_name()), Some(app), "{}", app.window_class);
        }
        assert_eq!(AppId::CALENDAR.open_name(), "torrocalendar", "one window: the binary, as before");
        assert_eq!(AppId::INBOX.open_name(), "torroinbox");
        assert_eq!(AppId::INBOX_MAIL.open_name(), "torro-inbox-mail");
    }

    #[test]
    fn the_command_carries_the_start_arguments() {
        assert_eq!(AppId::INBOX_MAIL.command("/usr/bin/torroinbox"), ["/usr/bin/torroinbox", "--view", "mail"]);
        assert_eq!(AppId::INBOX.command("torroinbox"), ["torroinbox"]);
        assert_eq!(AppId::WORKPLACE.command("torroworkplace"), ["torroworkplace", "ui"]);
    }
}
