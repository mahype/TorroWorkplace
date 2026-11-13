//! App identity.
//!
//! The app id is the one name a window carries everywhere: the Wayland app id and X11
//! class on Linux (Hyprland window rules match on it), the terminal `--class` a TUI is
//! started with, the key in AeroSpace's `on-window-detected`, and what workspace-setups
//! stores to find the window again. It never changes once an app has shipped.
//!
//! One binary may have several entries (task E8.1): `torroinbox` opens three kinds of window –
//! mail, notifications and the hub – each with its own window class, name and start arguments,
//! so window rules, the tray menu and workspace templates tell them apart.

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

/// The window class of a chat window of `torrochat` (issue #643, `torrochat --conversation=<id>`): one
/// conversation, floating (decision 16 in `docs/chat.md`). No app of its own, so not in
/// [`AppId::ALL`]; a workspace template stores the window's command line with its conversation.
pub const CHAT_CONVERSATION_CLASS: &str = "torro-chat-conversation";

/// The window class of the small window of `torrochat` that rings for an incoming call (stage 6 of
/// `docs/chat.md`, decision 24). No app of its own, so not in [`AppId::ALL`]; it floats, centred.
pub const CHAT_RING_CLASS: &str = "torro-chat-ring";

/// The window class of the window of `torrochat` that shows a picture large (stage 4 of
/// `docs/chat.md`, decision 24). No app of its own, so not in [`AppId::ALL`]; it floats.
pub const CHAT_VIEWER_CLASS: &str = "torro-chat-viewer";

/// A kind of window of the suite that floats over the tiled windows instead of being tiled (issue #667,
/// decision 16 in `docs/chat.md`): `binary` started with `flag` opens it, and it carries `class`.
///
/// Window managers that cannot match on a window's class tell these windows apart from their app's
/// other windows only by the start command: AeroSpace and GlazeWM name every window of `torrochat`,
/// the contact list too, by its bundle id or process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingWindow {
    /// The binary that opens the window, as in [`AppId::binary`].
    pub binary: &'static str,
    /// The flag of the start command that opens this window, e.g. `--conversation` (given as
    /// `--conversation=<id>` or `--conversation <id>`).
    pub flag: &'static str,
    /// The window class (app id) the window carries.
    pub class: &'static str,
}

/// Every window of the suite that floats. A new floating window (a call or device window of the chat)
/// is entered here and floats under every window manager without anything else to change: Hyprland
/// gets a rule per class, AeroSpace and GlazeWM float the window when it appears.
pub const FLOATING_WINDOWS: [FloatingWindow; 4] = [
    FloatingWindow { binary: "torrochat", flag: "--conversation", class: CHAT_CONVERSATION_CLASS },
    FloatingWindow { binary: "torrochat", flag: "--ring", class: CHAT_RING_CLASS },
    FloatingWindow { binary: "torrochat", flag: "--viewer", class: CHAT_VIEWER_CLASS },
    FloatingWindow { binary: "torroworkplace", flag: "--template", class: WORKPLACE_DIALOG_CLASS },
];

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

    /// The chat's contact list (issue #643, `docs/chat.md`): who is there, the group chats, the own
    /// status. The same binary opens a window per conversation (`torrochat --conversation=<id>`) with
    /// a class of its own, [`CHAT_CONVERSATION_CLASS`]; such a window is no app of its own, as the
    /// assistant's chat window is not.
    pub const CHAT: AppId = AppId {
        window_class: "torro-chat",
        binary: "torrochat",
        dir: "torrochat",
        display_name: "Torro Chat",
        args: &[],
    };

    /// The contacts app (issue #352, `docs/kontakte.md`): contacts grouped in a list, the contact in a
    /// side panel, editing, birthdays.
    pub const CONTACTS: AppId = AppId {
        window_class: "torro-contacts",
        binary: "torrocontacts",
        dir: "torrocontacts",
        display_name: "Torro Contacts",
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
        AppId::CHAT,
        AppId::CONTACTS,
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
    /// alone names it, else its window class (`torro-inbox-mail`). What the tray menu and the jumps to an app
    /// send with `apps.open_app`, so apps with one window are opened by binary exactly as before.
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
        for class in [WORKPLACE_DIALOG_CLASS, CHAT_CONVERSATION_CLASS, CHAT_RING_CLASS, CHAT_VIEWER_CLASS] {
            assert!(AppId::ALL.iter().all(|app| app.window_class != class), "{class} is no app");
        }
        for (i, app) in AppId::ALL.iter().enumerate() {
            assert!(
                AppId::ALL[i + 1..].iter().all(|other| other.window_class != app.window_class),
                "{} twice",
                app.window_class
            );
        }
    }

    /// Issue #643: the contact list is the chat's entry; its other windows have classes of their own.
    #[test]
    fn the_chat_is_one_entry_with_window_classes_of_its_own() {
        assert_eq!(AppId::find("torrochat"), Some(AppId::CHAT));
        assert_eq!(AppId::find("torro-chat"), Some(AppId::CHAT));
        assert_eq!(AppId::CHAT.open_name(), "torrochat");
        assert_eq!(AppId::find(CHAT_CONVERSATION_CLASS), None, "a chat window is no app of its own");
        for class in [CHAT_CONVERSATION_CLASS, CHAT_RING_CLASS, CHAT_VIEWER_CLASS] {
            assert!(class.starts_with("torro-chat-"), "{class}");
        }
    }

    /// Issue #667: a floating window is opened by an app's binary and carries no app's window class.
    #[test]
    fn floating_windows_belong_to_an_app_and_have_classes_of_their_own() {
        for window in FLOATING_WINDOWS {
            assert!(AppId::ALL.iter().any(|app| app.binary == window.binary), "{}", window.binary);
            assert_eq!(AppId::find(window.class), None, "{} is no app", window.class);
            assert!(window.flag.starts_with("--"), "{}", window.flag);
        }
        for (i, window) in FLOATING_WINDOWS.iter().enumerate() {
            assert!(FLOATING_WINDOWS[i + 1..].iter().all(|other| other.class != window.class), "{}", window.class);
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
                "torroassistant",
                "torrochat",
                "torrocontacts"
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
}
