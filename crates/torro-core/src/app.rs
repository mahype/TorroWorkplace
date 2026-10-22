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

/// The window class of the core's small dialog window (issue #243, `torroworkplace ui --template=<id>`):
/// a template's name, key and startup, after "Save workspace…" in the tray or from the control
/// centre. No app of its own, so not in [`AppId::ALL`]; the window manager floats it, and like the
/// control centre ([`AppId::WORKPLACE`]) it never goes into a workspace template.
pub const WORKPLACE_DIALOG_CLASS: &str = "torro-workplace-dialog";

/// The bundle id of the core's app on macOS (`packaging/macos/Info.plist.in` builds
/// `io.github.mahype.<app id>` from [`AppId::WORKPLACE`]'s window class), also its AppUserModelID on
/// Windows. AeroSpace names a window by it, so the control centre and the dialog, which both run
/// from this bundle, carry it there instead of their window class. Never changes once released.
pub const WORKPLACE_BUNDLE_ID: &str = "io.github.mahype.torro-workplace";

/// What every bundle id of the suite starts with: `packaging/macos/Info.plist.in` names an entry's
/// bundle `io.github.mahype.<window class>` ([`WORKPLACE_BUNDLE_ID`] is the core's).
const BUNDLE_ID_PREFIX: &str = "io.github.mahype.";

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

    /// The inbox's hub (task E8.1), shown as "Notification Hub" (issue #127): what to decide or hand
    /// on. `torroinbox` without `--view` starts it, so it comes first of the inbox's entries in
    /// [`AppId::ALL`] and the binary alone names it.
    pub const INBOX: AppId = AppId {
        window_class: "torro-inbox",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro Notification Hub",
        args: &[],
    };

    /// The inbox's mail window (task E8.1), shown as "Mail": `torroinbox --view mail`. Same binary and
    /// state directory as [`AppId::INBOX`]. Its long name says "Workplace" so nobody takes it for the
    /// separate product TorroMail (decision 20 in `docs/eingang.md`).
    pub const INBOX_MAIL: AppId = AppId {
        window_class: "torro-inbox-mail",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro Workplace Mail",
        args: &["--view", "mail"],
    };

    /// The inbox's notifications window (task E8.1), shown as "App Feed" (issue #127: a name unlike
    /// "Notification Hub"): `torroinbox --view notifications`, every source but mail. Same binary and
    /// state directory as [`AppId::INBOX`].
    pub const INBOX_NOTIFICATIONS: AppId = AppId {
        window_class: "torro-inbox-notifications",
        binary: "torroinbox",
        dir: "torroinbox",
        display_name: "Torro App Feed",
        args: &["--view", "notifications"],
    };

    /// The assistant's main window (task C.4, `docs/assistent.md`): its conversations and the chat.
    /// The same binary opens single chat windows (`torroassistant --chat <id>|new`) with a class of
    /// their own, `torro-assistant-chat`; such a window is no app of its own, so it is not listed
    /// here, as the inbox's compose window is not. A workspace template stores the window's whole
    /// command line, so a chat window comes back with its conversation.
    pub const ASSISTANT: AppId = AppId {
        window_class: "torro-assistant",
        binary: "torroassistant",
        dir: "torroassistant",
        display_name: "Torro Assistant",
        args: &[],
    };

    /// Every app of the suite, in launcher order. Of the entries sharing a binary, the one the
    /// binary starts without arguments comes first: a lookup by binary finds it.
    pub const ALL: &'static [AppId] = &[
        AppId::WORKPLACE,
        AppId::INBOX,
        AppId::INBOX_MAIL,
        AppId::INBOX_NOTIFICATIONS,
        AppId::TASKS,
        AppId::CALENDAR,
        AppId::ASSISTANT,
    ];

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

    /// The name of the program file that opens this entry's window on its own (issue #279, decision 2
    /// in `docs/lebenszyklus.md`): [`AppId::binary`], but for Mail and App Feed their window class
    /// (`torro-inbox-mail`, `torro-inbox-notifications`), each a program of its own so the taskbar and
    /// the Dock show it with its own icon. Without `.exe`. [`AppId::binary`] stays what `apps.list_apps`
    /// answers (`torroinbox`), so nothing changes for clients.
    pub fn program_file(self) -> &'static str {
        if self == AppId::INBOX_MAIL || self == AppId::INBOX_NOTIFICATIONS { self.window_class } else { self.binary }
    }

    /// Whether a window the window manager lists with `class` is this entry's (issue #279). Each
    /// window manager names a window differently: Hyprland by its window class (`torro-calendar`),
    /// AeroSpace by its app's bundle id (`io.github.mahype.torro-calendar`, else the program's name),
    /// GlazeWM by the process name (`torrocalendar`, maybe with `.exe`). So all three count: the window
    /// class, `io.github.mahype.` followed by it, and [`AppId::program_file`] with or without `.exe`
    /// (in any case, as Windows does not tell file names apart by case).
    ///
    /// No two entries of [`AppId::ALL`] own the same class: `torroinbox` is the hub's only, Mail and
    /// App Feed have programs of their own. A bundle id of anyone else never matches.
    pub fn owns_class(self, class: &str) -> bool {
        if class == self.window_class || class.strip_prefix(BUNDLE_ID_PREFIX) == Some(self.window_class) {
            return true;
        }
        let program = class.len().checked_sub(".exe".len()).and_then(|end| {
            let (name, suffix) = (class.get(..end)?, class.get(end..)?);
            suffix.eq_ignore_ascii_case(".exe").then_some(name)
        });
        program.unwrap_or(class).eq_ignore_ascii_case(self.program_file())
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
        assert!(AppId::ALL.iter().all(|app| app.window_class != WORKPLACE_DIALOG_CLASS), "the dialog is no app");
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

    /// Issue #279: Mail and App Feed get programs of their own, named like their window class; every
    /// other entry's program is its binary, and the binary itself stays as it was.
    #[test]
    fn the_program_file_is_the_binary_but_for_mail_and_app_feed() {
        let programs: Vec<_> = AppId::ALL.iter().map(|app| app.program_file()).collect();
        assert_eq!(
            programs,
            [
                "torroworkplace",
                "torroinbox",
                "torro-inbox-mail",
                "torro-inbox-notifications",
                "torrotasks",
                "torrocalendar",
                "torroassistant"
            ]
        );
        assert_eq!(AppId::INBOX_MAIL.binary, "torroinbox", "apps.list_apps answers as before");
        for (i, app) in AppId::ALL.iter().enumerate() {
            assert!(
                AppId::ALL[i + 1..].iter().all(|other| other.program_file() != app.program_file()),
                "{} twice",
                app.program_file()
            );
        }
    }

    /// Issue #279: every name a window manager gives an entry's window finds that entry and no other.
    #[test]
    fn an_entry_owns_its_class_bundle_id_and_program() {
        for &app in AppId::ALL {
            let bundle = format!("io.github.mahype.{}", app.window_class);
            let exe = format!("{}.exe", app.program_file());
            let shouted = format!("{}.EXE", app.program_file().to_uppercase());
            let names = [app.window_class, bundle.as_str(), app.program_file(), exe.as_str(), shouted.as_str()];
            for &other in AppId::ALL {
                for name in names {
                    assert_eq!(other.owns_class(name), other == app, "{} owns {name}?", other.window_class);
                }
            }
        }
        assert!(AppId::WORKPLACE.owns_class(WORKPLACE_BUNDLE_ID));
        assert!(AppId::INBOX.owns_class("torroinbox"));
        assert!(!AppId::INBOX_MAIL.owns_class("torroinbox"), "the binary names the hub only");
        assert!(!AppId::INBOX_NOTIFICATIONS.owns_class("torroinbox"));
    }

    #[test]
    fn foreign_classes_belong_to_no_entry() {
        let foreign = [
            "",
            ".exe",
            "io.github.mahype.",
            "io.github.other.torro-calendar",
            "com.example.torro-calendar",
            "io.github.mahype.torro-calendar.helper",
            "io.github.mahype.torrocalendar",
            "torrocalendar.app",
            "torrocalendar.exe.exe",
            WORKPLACE_DIALOG_CLASS,
            "torro-assistant-chat",
            "com.apple.finder",
        ];
        for class in foreign {
            assert!(AppId::ALL.iter().all(|app| !app.owns_class(class)), "{class:?} is nobody's");
        }
    }
}
