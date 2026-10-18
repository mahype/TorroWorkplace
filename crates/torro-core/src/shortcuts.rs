//! The keyboard shortcuts of the mail window (tasks E10.1 and E10.1b in `docs/tasks/eingang.md`,
//! section E10): which [`Command`]s there are, how a key combination is written ([`Shortcut`]) and
//! the [`Preset`]s that bind them – "Torro" and those of Apple Mail, Thunderbird and Outlook. Each
//! system starts with the preset of the mail program people know there ([`Preset::standard`]).
//!
//! Without Iced, so the core can check the same table when a person binds keys of their own (task
//! E10.2). The window turns a key press into [`Held`] and a [`Key`] and asks
//! [`Preset::command`]; hints next to buttons come from [`Preset::hint`]. Keys no preset may take
//! (copy, paste, undo, zoom …) are [`reserved`].
//!
//! **Notation** (what E10.2 stores and agents send): parts joined by `+`, modifiers first, the key
//! last – `"Mod+Shift+A"`, `"Ctrl+Cmd+A"`, `"Shift+Delete"`. `Mod` is the command key of the
//! system: Ctrl on Windows and Linux, ⌘ *or* Ctrl on macOS (both count the same there, so a hand
//! used to either works). `Ctrl`, `Cmd` (⌘, the logo key elsewhere), `Alt` (⌥) and `Shift` mean
//! exactly that key. Names are matched without regard to case; [`Shortcut`]'s `Display` writes
//! the canonical form.
//!
//! The single keys of the inbox (`r`, `a`, `f`, `e`, `#`, `/`, `j`/`k` …) are not in this table:
//! they stay in every preset (`torroinbox::keys`). Undo (Ctrl + Z, ⌘ + Z) is the suite's and
//! cannot be bound (`torro_gui::widgets::is_undo_key`).

use std::fmt;
use std::str::FromStr;

/// The system whose conventions a shortcut follows: `Mod`, the names in hints and the standard
/// preset ([`Preset::standard`]) differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    /// macOS: `Mod` is ⌘ or Ctrl, hints in Apple's order with "Cmd" and "Option".
    Mac,
    /// Windows: `Mod` is Ctrl.
    Windows,
    /// Linux and every other system: `Mod` is Ctrl.
    Linux,
}

impl System {
    /// Every system, so tests and the settings of task E10.2 can go through all of them.
    pub const ALL: [System; 3] = [System::Mac, System::Windows, System::Linux];

    /// The system this program runs on; anything that is neither macOS nor Windows counts as Linux.
    pub fn current() -> System {
        if cfg!(target_os = "macos") {
            System::Mac
        } else if cfg!(target_os = "windows") {
            System::Windows
        } else {
            System::Linux
        }
    }
}

/// What a shortcut of the mail window does. The names ([`Command::name`]) are the `command` of
/// task E10.2's settings and never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// Opens the compose window for a new message (`new_mail`).
    NewMail,
    /// Answers the sender of the mail under the cursor (`reply`).
    Reply,
    /// Answers everyone on it (`reply_all`).
    ReplyAll,
    /// Passes it on (`forward`).
    Forward,
    /// Sends the message in the compose window (`send`); works while a field has the focus.
    Send,
    /// Moves the mail to the trash, with an undo (`delete`).
    Delete,
    /// Removes the mail for good after a question (`delete_permanently`): there is no undo.
    DeletePermanently,
    /// Moves the mail to the account's archive, with an undo (`archive`).
    Archive,
    /// Marks the mail read when it is unread, else unread (`toggle_read`).
    ToggleRead,
    /// Flags the mail, or takes the flag away (`flag`).
    Flag,
    /// Moves the mail to the account's junk folder, with an undo (`junk`).
    Junk,
    /// Opens the choice of folders to move the mail to (`move_to_folder`).
    MoveToFolder,
    /// Puts the keyboard into the search field (`search`); works while a field has the focus.
    Search,
    /// Fetches mail from the servers now (`fetch`, `accounts.sync`).
    Fetch,
}

impl Command {
    /// Every command, in the order of the table in `docs/tasks/eingang.md` (E10).
    pub const ALL: [Command; 14] = [
        Command::NewMail,
        Command::Reply,
        Command::ReplyAll,
        Command::Forward,
        Command::Send,
        Command::Delete,
        Command::DeletePermanently,
        Command::Archive,
        Command::ToggleRead,
        Command::Flag,
        Command::Junk,
        Command::MoveToFolder,
        Command::Search,
        Command::Fetch,
    ];

    /// The command's name in snake case (`"reply_all"`), as settings and the API spell it.
    pub fn name(self) -> &'static str {
        match self {
            Command::NewMail => "new_mail",
            Command::Reply => "reply",
            Command::ReplyAll => "reply_all",
            Command::Forward => "forward",
            Command::Send => "send",
            Command::Delete => "delete",
            Command::DeletePermanently => "delete_permanently",
            Command::Archive => "archive",
            Command::ToggleRead => "toggle_read",
            Command::Flag => "flag",
            Command::Junk => "junk",
            Command::MoveToFolder => "move_to_folder",
            Command::Search => "search",
            Command::Fetch => "fetch",
        }
    }

    /// The command named `name` ([`Command::name`]); `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|command| command.name() == name)
    }

    /// Whether the shortcut works also while a text field has the focus. Only searching and sending
    /// do: every other shortcut leaves the field its own keys (copy, paste, select all), so a
    /// shortcut never archives a mail while someone types.
    pub fn works_in_text_field(self) -> bool {
        matches!(self, Command::Search | Command::Send)
    }
}

/// A key of a shortcut, without its modifiers. Letters are kept lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A letter (`a`–`z`, lower case), a digit or another printable ASCII sign except `+`.
    Char(char),
    /// A function key, F1 to F12 (task E10.1b: Thunderbird fetches with F5, Outlook with F9). Nothing
    /// in the suite makes another number: parsing refuses `F0` and `F13`, and the window maps only
    /// F1–F12 (`torroinbox::keys::table_key`).
    F(u8),
    /// Insert (Einfg; Outlook flags with it). A Mac keyboard has none.
    Insert,
    Enter,
    /// Delete (Entf; the forward delete of a Mac keyboard).
    Delete,
    /// Backspace (the key a Mac keyboard calls "delete").
    Backspace,
    Tab,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
}

/// The named keys and how the notation spells them; the function keys are spelled `F1`–`F12`
/// apart from these.
const NAMED: [(Key, &str); 11] = [
    (Key::Enter, "Enter"),
    (Key::Delete, "Delete"),
    (Key::Backspace, "Backspace"),
    (Key::Tab, "Tab"),
    (Key::Escape, "Escape"),
    (Key::Space, "Space"),
    (Key::Up, "Up"),
    (Key::Down, "Down"),
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Insert, "Insert"),
];

impl Key {
    /// The key for a typed character: letters lower case; `None` for a sign a shortcut cannot name
    /// (`+`, spaces, anything beyond ASCII).
    pub fn of_char(c: char) -> Option<Key> {
        (c.is_ascii_graphic() && c != '+').then(|| Key::Char(c.to_ascii_lowercase()))
    }

    /// The key as the notation writes it: `"A"`, `"Enter"`.
    fn name(self) -> String {
        match self {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::F(n) => format!("F{n}"),
            named => NAMED.iter().find(|(key, _)| *key == named).map_or_else(String::new, |(_, name)| (*name).into()),
        }
    }

    /// The key as a hint on `system` writes it: Backspace is "Delete" on a Mac keyboard, Delete
    /// there and elsewhere "Del"; Escape is "Esc", Insert "Ins".
    fn hint(self, system: System) -> String {
        match (self, system) {
            (Key::Backspace, System::Mac) => "Delete".into(),
            (Key::Delete, _) => "Del".into(),
            (Key::Escape, _) => "Esc".into(),
            (Key::Insert, _) => "Ins".into(),
            (key, _) => key.name(),
        }
    }

    /// The key named `name` in the notation; `None` for an unknown name.
    fn parse(name: &str) -> Option<Key> {
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Key::of_char(c);
        }
        let function = name.strip_prefix(['F', 'f']).filter(|n| !n.starts_with('0')).and_then(|n| n.parse::<u8>().ok());
        if let Some(n) = function {
            return (1..=12).contains(&n).then_some(Key::F(n));
        }
        NAMED.iter().find(|(_, spelled)| spelled.eq_ignore_ascii_case(name)).map(|(key, _)| *key)
    }
}

/// The modifiers held with a key press, as the system reports them. `cmd` is ⌘ on macOS and the
/// logo key (Windows, Super) elsewhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    pub ctrl: bool,
    pub cmd: bool,
    pub alt: bool,
    pub shift: bool,
}

/// One key combination: a [`Key`] and the modifiers to hold with it. Made by parsing the notation
/// (`"Mod+Shift+A".parse()`), so `Mod` never stands together with `Ctrl` or `Cmd` (which of the
/// two `Mod` takes would be unclear on a Mac).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// `Mod`: Ctrl, on macOS ⌘ or Ctrl.
    primary: bool,
    ctrl: bool,
    cmd: bool,
    alt: bool,
    shift: bool,
    key: Key,
}

impl Shortcut {
    /// The key of the combination.
    pub fn key(&self) -> Key {
        self.key
    }

    /// Whether pressing `key` while holding exactly `held` on `system` is this combination. `Mod`
    /// is Ctrl on Windows and Linux; on macOS either ⌘ or Ctrl, but not both (⌃⌘ is its own
    /// combination, "Ctrl+Cmd"). Every other modifier must be held as written – no more, no less.
    pub fn matches(&self, system: System, held: Held, key: Key) -> bool {
        if key != self.key || held.alt != self.alt || held.shift != self.shift {
            return false;
        }
        if !self.primary {
            return (held.ctrl, held.cmd) == (self.ctrl, self.cmd);
        }
        match system {
            System::Windows | System::Linux => held.ctrl && !held.cmd,
            System::Mac => held.ctrl != held.cmd,
        }
    }

    /// The combination as a key hint on `system` (`widgets::kbd`, tooltips): "Ctrl+Shift+A" on
    /// Windows and Linux; on macOS in Apple's order Ctrl, Option, Shift, Cmd with `Mod` as "Cmd"
    /// ("Shift+Cmd+R", "Ctrl+Cmd+A"), as the suite's undo hint names ⌘.
    pub fn hint(&self, system: System) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut push = |on: bool, name: &str| {
            if on {
                parts.push(name.to_owned());
            }
        };
        match system {
            System::Mac => {
                push(self.ctrl, "Ctrl");
                push(self.alt, "Option");
                push(self.shift, "Shift");
                push(self.cmd || self.primary, "Cmd");
            }
            System::Windows | System::Linux => {
                push(self.ctrl || self.primary, "Ctrl");
                push(self.cmd, "Super");
                push(self.alt, "Alt");
                push(self.shift, "Shift");
            }
        }
        parts.push(self.key.hint(system));
        parts.join("+")
    }
}

impl fmt::Display for Shortcut {
    /// The canonical notation: `Mod`, `Ctrl`, `Cmd`, `Alt`, `Shift` in this order, then the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in
            [(self.primary, "Mod"), (self.ctrl, "Ctrl"), (self.cmd, "Cmd"), (self.alt, "Alt"), (self.shift, "Shift")]
        {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key.name())
    }
}

/// Why a text is no [`Shortcut`]. The messages are English: they are meant for the API (task
/// E10.2), whose messages agents read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Nothing, or no key after the modifiers (`""`, `"Mod+"`, `"Shift"`).
    MissingKey,
    /// A part before the key that is no modifier.
    UnknownModifier(String),
    /// The last part names no key.
    UnknownKey(String),
    /// A modifier written twice.
    Repeated(String),
    /// `Mod` together with `Ctrl` or `Cmd`.
    ModWithCtrlOrCmd,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingKey => f.write_str("a shortcut needs a key after its modifiers, e.g. \"Mod+Shift+A\""),
            ParseError::UnknownModifier(name) => {
                write!(f, "\"{name}\" is no modifier; use Mod, Ctrl, Cmd, Alt or Shift")
            }
            ParseError::UnknownKey(name) => write!(f, "\"{name}\" is no key a shortcut can use"),
            ParseError::Repeated(name) => write!(f, "\"{name}\" is written twice"),
            ParseError::ModWithCtrlOrCmd => {
                f.write_str("Mod already stands for Ctrl (Cmd or Ctrl on macOS); name Ctrl and Cmd without it")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Shortcut {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Shortcut, ParseError> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, modifiers) = parts.split_last().ok_or(ParseError::MissingKey)?;
        let mut shortcut =
            Shortcut { primary: false, ctrl: false, cmd: false, alt: false, shift: false, key: Key::Space };
        for name in modifiers {
            let flag = match name.to_ascii_lowercase().as_str() {
                "mod" => &mut shortcut.primary,
                "ctrl" => &mut shortcut.ctrl,
                "cmd" => &mut shortcut.cmd,
                "alt" => &mut shortcut.alt,
                "shift" => &mut shortcut.shift,
                _ => return Err(ParseError::UnknownModifier((*name).to_owned())),
            };
            if *flag {
                return Err(ParseError::Repeated((*name).to_owned()));
            }
            *flag = true;
        }
        if shortcut.primary && (shortcut.ctrl || shortcut.cmd) {
            return Err(ParseError::ModWithCtrlOrCmd);
        }
        let is_modifier = ["mod", "ctrl", "cmd", "alt", "shift"].contains(&key.to_ascii_lowercase().as_str());
        if key.is_empty() || is_modifier {
            return Err(ParseError::MissingKey);
        }
        shortcut.key = Key::parse(key).ok_or_else(|| ParseError::UnknownKey((*key).to_owned()))?;
        Ok(shortcut)
    }
}

/// A set of bindings to start from. Each system starts with the preset of its usual mail program
/// ([`Preset::standard`]); task E10.2 adds Gmail and lets a person choose another preset – one of
/// another system too – and bind keys over it.
///
/// Every preset has keys on every system and binds every command: what its model has no key for
/// takes the key of "Torro" on that system. Rules for all presets (task E10.1b), so no key does two
/// things and nothing that is the system's or the suite's changes:
///
/// - No letter, digit, sign, Enter, Tab, Space or arrow without Ctrl or ⌘: those are the single keys
///   of the inbox (`a` reply all, `j` down, `m` mute, `s` snooze …). A model's single-letter
///   shortcut (Thunderbird's A, J, M, S) takes the key of "Torro" instead.
/// - No [`reserved`] key: ⌃⌘A of Apple Mail would be Ctrl + A ("select all") on Windows and Linux and
///   becomes Ctrl + Shift + A there, Outlook's Ctrl + Q would be ⌘Q ("quit") on a Mac and stays
///   Control + Q there.
/// - No Ctrl + Shift + U on Linux, where IBus takes it for typing Unicode while an input method is
///   active: toggling read is Ctrl + U there ("Torro" keeps its key of task E10.1).
/// - A Mac keyboard has no Insert, and its function keys set brightness and volume: on macOS those
///   commands take the key of "Torro", even where the model names a Mac key of its own:
///   Thunderbird's ⌘Y and Outlook's Control + M for getting mail are left out, so ⇧⌘N fetches in
///   every preset there.
/// - Search and send keep Ctrl + F and Ctrl + Enter (⌘ on macOS) beside the model's key, where the
///   preset does not bind them otherwise: the suite has used them since issue #136.
/// - Delete and delete for good are the same everywhere (task E8.18, E10.1); the question before
///   deleting for good belongs to the command, not to its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// The suite's own (`torro`): the usual shortcuts, else Apple Mail's, moving as in Outlook – the
    /// table "Vorlage „Torro“" in `docs/tasks/eingang.md`, the same on every system. Ctrl + A stays
    /// "select all", so archiving is Ctrl + Shift + A (⌃⌘A on macOS, as in Apple Mail).
    Torro,
    /// Apple Mail (`apple_mail`), the standard on macOS: [`APPLE_MAIL`].
    AppleMail,
    /// Thunderbird (`thunderbird`), the standard on Linux: [`THUNDERBIRD`].
    Thunderbird,
    /// Outlook (`outlook`), the standard on Windows: [`OUTLOOK`].
    Outlook,
}

/// The keys of one command in a preset on Windows, Linux and macOS, in the notation. The first keys
/// of a command are the ones its hint shows.
type Row = (Command, &'static [&'static str], &'static [&'static str], &'static [&'static str]);

/// The preset "Torro": each command with its keys on Windows, Linux and macOS (task E10.1).
const TORRO: [Row; 14] = [
    (Command::NewMail, &["Mod+N"], &["Mod+N"], &["Mod+N"]),
    (Command::Reply, &["Mod+R"], &["Mod+R"], &["Mod+R"]),
    (Command::ReplyAll, &["Mod+Shift+R"], &["Mod+Shift+R"], &["Mod+Shift+R"]),
    (Command::Forward, &["Mod+Shift+F"], &["Mod+Shift+F"], &["Mod+Shift+F"]),
    (Command::Send, &["Mod+Enter"], &["Mod+Enter"], &["Mod+Enter"]),
    (Command::Delete, &["Delete", "Backspace"], &["Delete", "Backspace"], &["Backspace", "Delete"]),
    (Command::DeletePermanently, &["Shift+Delete"], &["Shift+Delete"], &["Shift+Backspace", "Shift+Delete"]),
    (Command::Archive, &["Mod+Shift+A"], &["Mod+Shift+A"], &["Ctrl+Cmd+A"]),
    (Command::ToggleRead, &["Mod+Shift+U"], &["Mod+Shift+U"], &["Mod+Shift+U"]),
    (Command::Flag, &["Mod+Shift+L"], &["Mod+Shift+L"], &["Mod+Shift+L"]),
    (Command::Junk, &["Mod+Shift+J"], &["Mod+Shift+J"], &["Mod+Shift+J"]),
    (Command::MoveToFolder, &["Mod+Shift+V"], &["Mod+Shift+V"], &["Mod+Shift+V"]),
    (Command::Search, &["Mod+F"], &["Mod+F"], &["Mod+F"]),
    (Command::Fetch, &["Mod+Shift+N"], &["Mod+Shift+N"], &["Mod+Shift+N"]),
];

/// The preset "Apple Mail", on Windows, Linux and macOS.
///
/// Source: Apple, "Keyboard shortcuts in Mail on Mac",
/// <https://support.apple.com/guide/mail/keyboard-shortcuts-mlhlb94f262b/mac>, read 2026-10-16:
/// ⌘N new, ⌘R reply, ⇧⌘R reply all, ⇧⌘F forward, ⇧⌘D send, ⌃⌘A archive, ⇧⌘U read or unread, ⇧⌘J
/// junk, ⇧⌘N get new mail. Searching the mailbox is ⌥⌘F in Mail's menu (Edit › Find › Mailbox
/// Search), which the page leaves out; ⌘F searches too, as in "Torro". The page names no key for
/// flagging or moving to a chosen mailbox; those take the keys of "Torro" (⇧⌘L, ⇧⌘V). Sending keeps
/// ⌘Enter beside ⇧⌘D.
///
/// On Windows and Linux ⌘ is Ctrl. Archiving is Ctrl + Shift + A there (⌃⌘A would be Ctrl + A),
/// toggling read on Linux Ctrl + U (IBus takes Ctrl + Shift + U). ⌥⌘F stays the Mac's: Ctrl + Alt is
/// AltGr on Windows, which types characters; searching is Ctrl + F there.
const APPLE_MAIL: [Row; 14] = [
    (Command::NewMail, &["Mod+N"], &["Mod+N"], &["Mod+N"]),
    (Command::Reply, &["Mod+R"], &["Mod+R"], &["Mod+R"]),
    (Command::ReplyAll, &["Mod+Shift+R"], &["Mod+Shift+R"], &["Mod+Shift+R"]),
    (Command::Forward, &["Mod+Shift+F"], &["Mod+Shift+F"], &["Mod+Shift+F"]),
    (Command::Send, &["Mod+Shift+D", "Mod+Enter"], &["Mod+Shift+D", "Mod+Enter"], &["Mod+Shift+D", "Mod+Enter"]),
    (Command::Delete, &["Delete", "Backspace"], &["Delete", "Backspace"], &["Backspace", "Delete"]),
    (Command::DeletePermanently, &["Shift+Delete"], &["Shift+Delete"], &["Shift+Backspace", "Shift+Delete"]),
    (Command::Archive, &["Mod+Shift+A"], &["Mod+Shift+A"], &["Ctrl+Cmd+A"]),
    (Command::ToggleRead, &["Mod+Shift+U"], &["Mod+U"], &["Mod+Shift+U"]),
    (Command::Flag, &["Mod+Shift+L"], &["Mod+Shift+L"], &["Mod+Shift+L"]),
    (Command::Junk, &["Mod+Shift+J"], &["Mod+Shift+J"], &["Mod+Shift+J"]),
    (Command::MoveToFolder, &["Mod+Shift+V"], &["Mod+Shift+V"], &["Mod+Shift+V"]),
    (Command::Search, &["Mod+F"], &["Mod+F"], &["Mod+Alt+F", "Mod+F"]),
    (Command::Fetch, &["Mod+Shift+N"], &["Mod+Shift+N"], &["Mod+Shift+N"]),
];

/// The preset "Thunderbird", on Windows, Linux and macOS.
///
/// Source: Mozilla, "Keyboard shortcuts – perform common Thunderbird tasks quickly",
/// <https://support.mozilla.org/kb/keyboard-shortcuts-thunderbird>, read 2026-10-16: Ctrl + N (or
/// Ctrl + M) new, Ctrl + R reply, Ctrl + Shift + R reply all, Ctrl + L forward, Ctrl + Enter send,
/// Ctrl + K search all messages, Ctrl + Shift + K quick filter, F5 get new messages (Shift + F5 for
/// all accounts), Del and Shift + Del delete; on macOS ⌘ for Ctrl and ⌘Y, ⇧⌘Y to get new messages.
///
/// Its single letters – A archive, M read or unread, J junk, S star – are the inbox's single keys
/// here, so these commands take the keys of "Torro" (Ctrl + Shift + A, U, J, L; toggling read on
/// Linux Ctrl + U for IBus). Moving to a chosen folder has no key in Thunderbird: "Torro"'s. Its
/// Ctrl + M for a new message is left out on macOS, where ⌘M minimizes (the page lists only ⌘N). Ctrl + F
/// finds text in Thunderbird's message; here it searches as everywhere in the suite.
///
/// Its F5 is no key on a Mac, and the page's ⌘Y, ⇧⌘Y are left out: fetching takes "Torro"'s ⇧⌘N
/// there, as in every preset, so one key gets new mail on every Mac.
const THUNDERBIRD: [Row; 14] = [
    (Command::NewMail, &["Mod+N", "Mod+M"], &["Mod+N", "Mod+M"], &["Mod+N"]),
    (Command::Reply, &["Mod+R"], &["Mod+R"], &["Mod+R"]),
    (Command::ReplyAll, &["Mod+Shift+R"], &["Mod+Shift+R"], &["Mod+Shift+R"]),
    (Command::Forward, &["Mod+L"], &["Mod+L"], &["Mod+L"]),
    (Command::Send, &["Mod+Enter"], &["Mod+Enter"], &["Mod+Enter"]),
    (Command::Delete, &["Delete", "Backspace"], &["Delete", "Backspace"], &["Backspace", "Delete"]),
    (Command::DeletePermanently, &["Shift+Delete"], &["Shift+Delete"], &["Shift+Backspace", "Shift+Delete"]),
    (Command::Archive, &["Mod+Shift+A"], &["Mod+Shift+A"], &["Ctrl+Cmd+A"]),
    (Command::ToggleRead, &["Mod+Shift+U"], &["Mod+U"], &["Mod+Shift+U"]),
    (Command::Flag, &["Mod+Shift+L"], &["Mod+Shift+L"], &["Mod+Shift+L"]),
    (Command::Junk, &["Mod+Shift+J"], &["Mod+Shift+J"], &["Mod+Shift+J"]),
    (Command::MoveToFolder, &["Mod+Shift+V"], &["Mod+Shift+V"], &["Mod+Shift+V"]),
    (
        Command::Search,
        &["Mod+K", "Mod+Shift+K", "Mod+F"],
        &["Mod+K", "Mod+Shift+K", "Mod+F"],
        &["Mod+K", "Mod+Shift+K", "Mod+F"],
    ),
    (Command::Fetch, &["F5", "Shift+F5"], &["F5", "Shift+F5"], &["Mod+Shift+N"]),
];

/// The preset "Outlook", on Windows, Linux and macOS.
///
/// Source: Microsoft, "Keyboard shortcuts for Outlook",
/// <https://support.microsoft.com/office/keyboard-shortcuts-for-outlook-3cdeb221-7ae5-4c1d-8c1d-9e63216c1efd>,
/// read 2026-10-16. New Outlook for Windows: Ctrl + N new, Ctrl + R reply, Ctrl + Shift + R reply
/// all, Ctrl + F forward, Ctrl + Enter send, Delete and Shift + Delete, Ctrl + Q read, Ctrl + U
/// unread, Insert flag. Classic Outlook for what new Outlook leaves out: Ctrl + E or F3 search,
/// Ctrl + Shift + V move to a folder, F9 or Ctrl + M check for new messages. Neither names a key
/// for archiving or junk: "Torro"'s on every system (Ctrl + Shift + A, J; ⌃⌘A and ⇧⌘J on macOS).
/// Ctrl + F forwards, so it does not search in this preset.
///
/// Read and unread are one command here: Ctrl + Q and Ctrl + U both toggle. On macOS Ctrl turns ⌘
/// (Outlook for Windows translated, not Outlook for Mac), except where ⌘ is the system's: Ctrl + Q
/// stays Control there (⌘Q quits). Insert, F9 and Ctrl + M have no Mac key (⌘M minimizes): flagging
/// takes "Torro"'s ⇧⌘L, fetching its ⇧⌘N; F3 is left out.
const OUTLOOK: [Row; 14] = [
    (Command::NewMail, &["Mod+N"], &["Mod+N"], &["Mod+N"]),
    (Command::Reply, &["Mod+R"], &["Mod+R"], &["Mod+R"]),
    (Command::ReplyAll, &["Mod+Shift+R"], &["Mod+Shift+R"], &["Mod+Shift+R"]),
    (Command::Forward, &["Mod+F"], &["Mod+F"], &["Mod+F"]),
    (Command::Send, &["Mod+Enter"], &["Mod+Enter"], &["Mod+Enter"]),
    (Command::Delete, &["Delete", "Backspace"], &["Delete", "Backspace"], &["Backspace", "Delete"]),
    (Command::DeletePermanently, &["Shift+Delete"], &["Shift+Delete"], &["Shift+Backspace", "Shift+Delete"]),
    (Command::Archive, &["Mod+Shift+A"], &["Mod+Shift+A"], &["Ctrl+Cmd+A"]),
    (Command::ToggleRead, &["Mod+U", "Mod+Q"], &["Mod+U", "Mod+Q"], &["Mod+U", "Ctrl+Q"]),
    (Command::Flag, &["Insert"], &["Insert"], &["Mod+Shift+L"]),
    (Command::Junk, &["Mod+Shift+J"], &["Mod+Shift+J"], &["Mod+Shift+J"]),
    (Command::MoveToFolder, &["Mod+Shift+V"], &["Mod+Shift+V"], &["Mod+Shift+V"]),
    (Command::Search, &["Mod+E", "F3"], &["Mod+E", "F3"], &["Mod+E"]),
    (Command::Fetch, &["F9", "Mod+M"], &["F9", "Mod+M"], &["Mod+Shift+N"]),
];

impl Preset {
    /// Every preset, in the order a choice lists them (task E10.2).
    pub const ALL: [Preset; 4] = [Preset::Torro, Preset::AppleMail, Preset::Thunderbird, Preset::Outlook];

    /// The preset's name in settings (`"torro"`, `"apple_mail"`, `"thunderbird"`, `"outlook"`); the
    /// values of task E10.2's `mail_shortcut_preset`, which never change.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Torro => "torro",
            Preset::AppleMail => "apple_mail",
            Preset::Thunderbird => "thunderbird",
            Preset::Outlook => "outlook",
        }
    }

    /// The preset named `name` ([`Preset::name`]); `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|preset| preset.name() == name)
    }

    /// The preset `system` starts with (task E10.1b): Apple Mail on macOS, Thunderbird on Linux,
    /// Outlook on Windows – the keys of the mail program people know there.
    pub fn standard(system: System) -> Preset {
        match system {
            System::Mac => Preset::AppleMail,
            System::Linux => Preset::Thunderbird,
            System::Windows => Preset::Outlook,
        }
    }

    fn table(self) -> &'static [Row; 14] {
        match self {
            Preset::Torro => &TORRO,
            Preset::AppleMail => &APPLE_MAIL,
            Preset::Thunderbird => &THUNDERBIRD,
            Preset::Outlook => &OUTLOOK,
        }
    }

    /// The keys of `command` in this preset on `system`, the one hints show first; empty when the
    /// preset gives the command no keys.
    pub fn keys(self, command: Command, system: System) -> Vec<Shortcut> {
        let written =
            self.table().iter().find(|(c, ..)| *c == command).map_or(
                &[][..],
                |(_, windows, linux, mac)| match system {
                    System::Windows => *windows,
                    System::Linux => *linux,
                    System::Mac => *mac,
                },
            );
        // The tables are tested to parse whole; nothing is dropped here.
        written.iter().filter_map(|text| text.parse().ok()).collect()
    }

    /// The command that `key` pressed with exactly `held` runs on `system`, if any.
    pub fn command(self, system: System, held: Held, key: Key) -> Option<Command> {
        Command::ALL
            .into_iter()
            .find(|command| self.keys(*command, system).iter().any(|shortcut| shortcut.matches(system, held, key)))
    }

    /// The hint of `command` on `system` ([`Shortcut::hint`] of its first keys): "Ctrl+Enter",
    /// "Shift+Cmd+D". `None` when it has no keys.
    pub fn hint(self, command: Command, system: System) -> Option<String> {
        self.keys(command, system).first().map(|shortcut| shortcut.hint(system))
    }
}

impl Default for Preset {
    /// The standard preset of the system this program runs on ([`Preset::standard`]).
    fn default() -> Preset {
        Preset::standard(System::current())
    }
}

/// The keys no preset and no binding of a person's own may take on `system` (tasks E10.1b, E10.2):
/// they are the window's or the system's. Escape closes what is in front; `[` toggles the sidebar
/// with any modifiers but ⌘ (`torro_gui::shell::is_sidebar_key`); `Mod+Z` undoes
/// (`torro_gui::widgets::is_undo_key`); `Mod` with C, X, V and A copy, cut, paste and select all in a
/// text field, on Windows and Linux also Ctrl + Insert (copy) and Shift + Insert (paste); `Mod` with
/// `=`, `-` and `0` zoom, also with Shift and with Ctrl and ⌘ together (`torro_gui::zoom::step_for`
/// zooms whenever ⌘ – Ctrl elsewhere – is held without Alt). On macOS also ⌘Q quit, ⌘W
/// close, ⌘H hide, ⌘M minimize and ⌘, settings – Control with those letters stays free there.
///
/// Shift + Delete, which cuts in a text field on Windows, is not here: it deletes for good in every
/// preset since task E10.1, and shortcuts do not act while a field has the keyboard.
pub fn reserved(system: System) -> Vec<Shortcut> {
    const EVERYWHERE: [&str; 13] = [
        "Escape",
        "[",
        "Mod+Z",
        "Mod+C",
        "Mod+X",
        "Mod+V",
        "Mod+A",
        "Mod+=",
        "Mod+Shift+=",
        "Mod+-",
        "Mod+Shift+-",
        "Mod+0",
        "Mod+Shift+0",
    ];
    const ZOOM: [&str; 6] =
        ["Ctrl+Cmd+=", "Ctrl+Cmd+Shift+=", "Ctrl+Cmd+-", "Ctrl+Cmd+Shift+-", "Ctrl+Cmd+0", "Ctrl+Cmd+Shift+0"];
    const SIDEBAR: [&str; 7] =
        ["Shift+[", "Ctrl+[", "Alt+[", "Ctrl+Shift+[", "Ctrl+Alt+[", "Alt+Shift+[", "Ctrl+Alt+Shift+["];
    const MAC: [&str; 5] = ["Cmd+Q", "Cmd+W", "Cmd+H", "Cmd+M", "Cmd+,"];
    const OTHER: [&str; 2] = ["Ctrl+Insert", "Shift+Insert"];
    let own: &[&str] = if system == System::Mac { &MAC } else { &OTHER };
    // Tested to parse whole, as the presets are.
    EVERYWHERE.iter().chain(&ZOOM).chain(&SIDEBAR).chain(own).filter_map(|text| text.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Held = Held { ctrl: false, cmd: false, alt: false, shift: false };
    const CTRL: Held = Held { ctrl: true, ..NONE };
    const CMD: Held = Held { cmd: true, ..NONE };
    const SHIFT: Held = Held { shift: true, ..NONE };

    fn with_shift(held: Held) -> Held {
        Held { shift: true, ..held }
    }

    fn letter(c: char) -> Key {
        Key::Char(c)
    }

    /// The table of E10 key by key: what the command keys of each system run. On macOS ⌘ and Ctrl
    /// count the same for every `Mod` shortcut.
    #[test]
    fn every_shortcut_of_the_torro_preset_on_both_systems() {
        let mod_keys = [
            (Command::NewMail, false, letter('n')),
            (Command::Reply, false, letter('r')),
            (Command::ReplyAll, true, letter('r')),
            (Command::Forward, true, letter('f')),
            (Command::Send, false, Key::Enter),
            (Command::ToggleRead, true, letter('u')),
            (Command::Flag, true, letter('l')),
            (Command::Junk, true, letter('j')),
            (Command::MoveToFolder, true, letter('v')),
            (Command::Search, false, letter('f')),
            (Command::Fetch, true, letter('n')),
        ];
        let torro = Preset::Torro;
        for (command, shift, key) in mod_keys {
            let held = |held: Held| if shift { with_shift(held) } else { held };
            for other in [System::Windows, System::Linux] {
                assert_eq!(torro.command(other, held(CTRL), key), Some(command), "Ctrl: {command:?} {other:?}");
                assert_eq!(torro.command(other, held(CMD), key), None, "the logo key is no Ctrl: {command:?}");
                assert_eq!(torro.command(other, held(NONE), key), None, "a plain key: {command:?}");
            }
            assert_eq!(torro.command(System::Mac, held(CMD), key), Some(command), "⌘: {command:?}");
            assert_eq!(torro.command(System::Mac, held(CTRL), key), Some(command), "Ctrl on a Mac: {command:?}");
            let both = Held { ctrl: true, cmd: true, ..held(NONE) };
            assert_eq!(torro.command(System::Mac, both, key), None, "⌃⌘ is its own combination: {command:?}");
        }
    }

    #[test]
    fn archive_is_ctrl_shift_a_and_on_a_mac_ctrl_cmd_a() {
        let torro = Preset::Torro;
        let a = letter('a');
        assert_eq!(torro.command(System::Windows, with_shift(CTRL), a), Some(Command::Archive));
        assert_eq!(torro.command(System::Linux, with_shift(CTRL), a), Some(Command::Archive));
        let ctrl_cmd = Held { ctrl: true, cmd: true, ..NONE };
        assert_eq!(torro.command(System::Mac, ctrl_cmd, a), Some(Command::Archive));
        assert_eq!(torro.command(System::Mac, with_shift(CMD), a), None, "not the Linux one on a Mac");
    }

    #[test]
    fn ctrl_a_stays_select_all() {
        for preset in Preset::ALL {
            for system in System::ALL {
                for held in [CTRL, CMD] {
                    assert_eq!(preset.command(system, held, letter('a')), None, "{preset:?} {system:?} {held:?}");
                }
            }
        }
    }

    #[test]
    fn delete_moves_to_the_trash_and_shift_delete_removes_for_good() {
        // The same in every preset (task E10.1b): deleting for good always asks first, in the app.
        for preset in Preset::ALL {
            for system in System::ALL {
                let on = |held, key| preset.command(system, held, key);
                assert_eq!(on(NONE, Key::Delete), Some(Command::Delete), "{preset:?} {system:?}");
                assert_eq!(on(NONE, Key::Backspace), Some(Command::Delete), "{preset:?} {system:?}");
                assert_eq!(on(SHIFT, Key::Delete), Some(Command::DeletePermanently), "{preset:?} {system:?}");
                assert_eq!(on(CTRL, Key::Delete), None, "{preset:?} {system:?}");
                let shift_backspace = (system == System::Mac).then_some(Command::DeletePermanently);
                assert_eq!(on(SHIFT, Key::Backspace), shift_backspace, "⇧⌫ only on a Mac: {preset:?} {system:?}");
            }
        }
    }

    /// Every key press there is for `keys`: each of them with each of the 16 ways to hold the
    /// modifiers.
    fn presses(keys: impl IntoIterator<Item = Key>) -> Vec<(Held, Key)> {
        let mut presses = Vec::new();
        for key in keys {
            for bits in 0..16u8 {
                let held = Held { ctrl: bits & 1 != 0, cmd: bits & 2 != 0, alt: bits & 4 != 0, shift: bits & 8 != 0 };
                presses.push((held, key));
            }
        }
        presses
    }

    /// The keys a preset or [`reserved`] uses on `system`.
    fn used_keys(system: System) -> Vec<Key> {
        let mut keys: Vec<Key> = reserved(system).iter().map(Shortcut::key).collect();
        for preset in Preset::ALL {
            for command in Command::ALL {
                keys.extend(preset.keys(command, system).iter().map(Shortcut::key));
            }
        }
        keys.sort_by_key(|key| format!("{key:?}"));
        keys.dedup();
        keys
    }

    /// Task E10.1b: every preset gives every command keys on every system, and no press runs two
    /// commands, a reserved key or a single key of the inbox.
    #[test]
    fn every_preset_binds_every_command_once_on_every_system() {
        for preset in Preset::ALL {
            for system in System::ALL {
                for command in Command::ALL {
                    assert!(!preset.keys(command, system).is_empty(), "{preset:?} {command:?} on {system:?}");
                }
                let reserved = reserved(system);
                for (held, key) in presses(used_keys(system)) {
                    let commands: Vec<Command> = Command::ALL
                        .into_iter()
                        .filter(|c| preset.keys(*c, system).iter().any(|s| s.matches(system, held, key)))
                        .collect();
                    assert!(commands.len() <= 1, "{preset:?} {system:?} {held:?} {key:?}: {commands:?}");
                    let taken = reserved.iter().any(|s| s.matches(system, held, key));
                    assert!(!taken || commands.is_empty(), "{preset:?} {system:?} {held:?} {key:?} is reserved");
                }
            }
        }
    }

    /// Without Ctrl, ⌘ or Alt only Delete, Backspace, the function keys and Insert: letters, digits,
    /// signs, Enter, Tab, Space and the arrows are the inbox's single keys (`j` down, `a` reply all,
    /// `m` mute, `s` snooze …) – Thunderbird's A, J, M and S become keys with Ctrl.
    #[test]
    fn no_preset_takes_a_single_key() {
        for preset in Preset::ALL {
            for system in System::ALL {
                for command in Command::ALL {
                    for shortcut in preset.keys(command, system) {
                        let plain = !(shortcut.primary || shortcut.ctrl || shortcut.cmd || shortcut.alt);
                        let free = matches!(shortcut.key, Key::Delete | Key::Backspace | Key::F(_) | Key::Insert);
                        assert!(!plain || free, "{preset:?} {system:?} {command:?}: {shortcut}");
                    }
                }
            }
        }
        let thunderbird = Preset::Thunderbird;
        for key in ['a', 'j', 'm', 's'] {
            assert_eq!(thunderbird.command(System::Linux, NONE, letter(key)), None, "{key}");
        }
    }

    /// IBus takes Ctrl + Shift + U on Linux while an input method is active: only "Torro" keeps it
    /// (task E10.1), the presets of task E10.1b toggle read with Ctrl + U there.
    #[test]
    fn no_new_preset_uses_ctrl_shift_u_on_linux() {
        let u = letter('u');
        for preset in [Preset::AppleMail, Preset::Thunderbird, Preset::Outlook] {
            assert_eq!(preset.command(System::Linux, with_shift(CTRL), u), None, "{preset:?}");
            assert_eq!(preset.command(System::Linux, CTRL, u), Some(Command::ToggleRead), "{preset:?}");
        }
    }

    #[test]
    fn the_tables_and_the_reserved_keys_parse_whole() {
        for table in [&TORRO, &APPLE_MAIL, &THUNDERBIRD, &OUTLOOK] {
            for (command, windows, linux, mac) in table {
                for text in windows.iter().chain(*linux).chain(*mac) {
                    assert!(text.parse::<Shortcut>().is_ok(), "{command:?}: {text}");
                }
            }
            let commands: Vec<Command> = table.iter().map(|(command, ..)| *command).collect();
            assert_eq!(commands, Command::ALL, "every command once, in order");
        }
        assert_eq!(reserved(System::Linux).len(), 28);
        assert_eq!(reserved(System::Windows).len(), 28);
        assert_eq!(reserved(System::Mac).len(), 31);
    }

    #[test]
    fn reserved_keys_are_the_windows_and_the_systems() {
        let is_reserved = |system, held, key| reserved(system).iter().any(|s| s.matches(system, held, key));
        for system in System::ALL {
            assert!(is_reserved(system, NONE, Key::Escape), "{system:?}");
            assert!(is_reserved(system, NONE, letter('[')), "{system:?}");
            for c in ['z', 'c', 'x', 'v', 'a', '=', '-', '0'] {
                let command = if system == System::Mac { CMD } else { CTRL };
                assert!(is_reserved(system, command, letter(c)), "{system:?} {c}");
            }
            assert!(is_reserved(system, with_shift(CTRL), letter('=')), "zoom in with Shift: {system:?}");
            assert!(is_reserved(system, Held { ctrl: true, cmd: true, ..NONE }, letter('0')), "⌃⌘0: {system:?}");
        }
        for system in System::ALL {
            for held in [CTRL, SHIFT, Held { alt: true, ..NONE }, with_shift(CTRL)] {
                assert!(is_reserved(system, held, letter('[')), "the sidebar: {system:?} {held:?}");
            }
        }
        for other in [System::Windows, System::Linux] {
            assert!(is_reserved(other, CTRL, Key::Insert) && is_reserved(other, SHIFT, Key::Insert), "{other:?}");
            assert!(!is_reserved(other, NONE, Key::Insert), "plain Insert flags in Outlook: {other:?}");
        }
        assert!(is_reserved(System::Mac, CMD, letter('q')), "⌘Q quits");
        assert!(!is_reserved(System::Mac, CTRL, letter('q')), "Control + Q stays free");
        assert!(!is_reserved(System::Linux, CTRL, letter('q')));
    }

    /// The table of each preset against its maker's help (sources in the doc comments of the
    /// tables): the first keys of every command, as hints show them, on each system.
    #[test]
    fn every_preset_matches_its_model() {
        use Command as C;
        let first = |preset: Preset, system| -> Vec<String> {
            Command::ALL
                .into_iter()
                .map(|c| preset.keys(c, system).first().map(ToString::to_string).unwrap_or_default())
                .collect()
        };
        let order = [
            C::NewMail,
            C::Reply,
            C::ReplyAll,
            C::Forward,
            C::Send,
            C::Delete,
            C::DeletePermanently,
            C::Archive,
            C::ToggleRead,
            C::Flag,
            C::Junk,
            C::MoveToFolder,
            C::Search,
            C::Fetch,
        ];
        assert_eq!(order, Command::ALL);
        let apple_mac = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+Shift+F",
            "Mod+Shift+D",
            "Backspace",
            "Shift+Backspace",
            "Ctrl+Cmd+A",
            "Mod+Shift+U",
            "Mod+Shift+L",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+Alt+F",
            "Mod+Shift+N",
        ];
        assert_eq!(first(Preset::AppleMail, System::Mac), apple_mac);
        let apple_linux = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+Shift+F",
            "Mod+Shift+D",
            "Delete",
            "Shift+Delete",
            "Mod+Shift+A",
            "Mod+U",
            "Mod+Shift+L",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+F",
            "Mod+Shift+N",
        ];
        assert_eq!(first(Preset::AppleMail, System::Linux), apple_linux);
        let mut apple_windows = apple_linux;
        apple_windows[8] = "Mod+Shift+U";
        assert_eq!(first(Preset::AppleMail, System::Windows), apple_windows);

        let thunderbird_linux = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+L",
            "Mod+Enter",
            "Delete",
            "Shift+Delete",
            "Mod+Shift+A",
            "Mod+U",
            "Mod+Shift+L",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+K",
            "F5",
        ];
        assert_eq!(first(Preset::Thunderbird, System::Linux), thunderbird_linux);
        let mut thunderbird_windows = thunderbird_linux;
        thunderbird_windows[8] = "Mod+Shift+U";
        assert_eq!(first(Preset::Thunderbird, System::Windows), thunderbird_windows);
        let thunderbird_mac = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+L",
            "Mod+Enter",
            "Backspace",
            "Shift+Backspace",
            "Ctrl+Cmd+A",
            "Mod+Shift+U",
            "Mod+Shift+L",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+K",
            "Mod+Shift+N",
        ];
        assert_eq!(first(Preset::Thunderbird, System::Mac), thunderbird_mac);

        let outlook_windows = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+F",
            "Mod+Enter",
            "Delete",
            "Shift+Delete",
            "Mod+Shift+A",
            "Mod+U",
            "Insert",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+E",
            "F9",
        ];
        assert_eq!(first(Preset::Outlook, System::Windows), outlook_windows);
        assert_eq!(first(Preset::Outlook, System::Linux), outlook_windows);
        let outlook_mac = [
            "Mod+N",
            "Mod+R",
            "Mod+Shift+R",
            "Mod+F",
            "Mod+Enter",
            "Backspace",
            "Shift+Backspace",
            "Ctrl+Cmd+A",
            "Mod+U",
            "Mod+Shift+L",
            "Mod+Shift+J",
            "Mod+Shift+V",
            "Mod+E",
            "Mod+Shift+N",
        ];
        assert_eq!(first(Preset::Outlook, System::Mac), outlook_mac);

        for preset in Preset::ALL {
            let fetch = preset.keys(C::Fetch, System::Mac);
            assert_eq!(
                fetch.iter().map(ToString::to_string).collect::<Vec<_>>(),
                ["Mod+Shift+N"],
                "a Mac has no F keys to spare, so every preset fetches with ⇧⌘N ({preset:?})"
            );
        }
    }

    /// The further keys of each model: Ctrl + M for a new message in Thunderbird, Ctrl + Q (Control +
    /// Q on a Mac) for read in Outlook, F3 and Ctrl + M there, Shift + F5 and the quick filter in
    /// Thunderbird, and Ctrl + F and Ctrl + Enter where the preset leaves them free. The keys a Mac
    /// keeps for itself bind nothing: ⌘Q, ⌘M, and Thunderbird's ⇧⌘Y, which it does not use here.
    #[test]
    fn the_further_keys_of_the_models() {
        let run = |preset: Preset, system, text: &str| {
            let shortcut: Shortcut = text.parse().expect(text);
            let held = Held {
                ctrl: shortcut.ctrl || (shortcut.primary && system != System::Mac),
                cmd: shortcut.cmd || (shortcut.primary && system == System::Mac),
                alt: shortcut.alt,
                shift: shortcut.shift,
            };
            preset.command(system, held, shortcut.key)
        };
        for system in [System::Windows, System::Linux] {
            assert_eq!(run(Preset::Thunderbird, system, "Ctrl+M"), Some(Command::NewMail));
            assert_eq!(run(Preset::Thunderbird, system, "Shift+F5"), Some(Command::Fetch));
            assert_eq!(run(Preset::Outlook, system, "Ctrl+Q"), Some(Command::ToggleRead));
            assert_eq!(run(Preset::Outlook, system, "F3"), Some(Command::Search));
            assert_eq!(run(Preset::Outlook, system, "Ctrl+M"), Some(Command::Fetch));
        }
        assert_eq!(run(Preset::Thunderbird, System::Mac, "Cmd+Shift+Y"), None, "⇧⌘Y is left out");
        assert_eq!(run(Preset::Outlook, System::Mac, "Ctrl+Q"), Some(Command::ToggleRead));
        assert_eq!(run(Preset::Outlook, System::Mac, "Cmd+Q"), None, "⌘Q quits");
        assert_eq!(run(Preset::Outlook, System::Mac, "Cmd+M"), None, "⌘M minimizes");
        assert_eq!(run(Preset::Outlook, System::Mac, "Ctrl+M"), None, "Control + M is Windows' only");
        for preset in Preset::ALL {
            for system in System::ALL {
                assert_eq!(run(preset, system, "Mod+Enter"), Some(Command::Send), "{preset:?} {system:?}");
                let ctrl_f = if preset == Preset::Outlook { Command::Forward } else { Command::Search };
                assert_eq!(run(preset, system, "Mod+F"), Some(ctrl_f), "{preset:?} {system:?}");
            }
        }
        for system in System::ALL {
            assert_eq!(run(Preset::Thunderbird, system, "Mod+Shift+K"), Some(Command::Search), "quick filter");
        }
    }

    #[test]
    fn each_system_starts_with_its_mail_program() {
        assert_eq!(Preset::standard(System::Mac), Preset::AppleMail);
        assert_eq!(Preset::standard(System::Linux), Preset::Thunderbird);
        assert_eq!(Preset::standard(System::Windows), Preset::Outlook);
        assert_eq!(Preset::default(), Preset::standard(System::current()));
        let current = System::current();
        if cfg!(target_os = "macos") {
            assert_eq!(current, System::Mac);
        } else if cfg!(target_os = "windows") {
            assert_eq!(current, System::Windows);
        } else {
            assert_eq!(current, System::Linux);
        }
    }

    #[test]
    fn hints_name_the_keys_as_each_system_does() {
        let windows = |text: &str| text.parse::<Shortcut>().expect(text).hint(System::Windows);
        assert_eq!(windows("Shift+F5"), "Shift+F5");
        assert_eq!(windows("Insert"), "Ins");
        assert_eq!(windows("Mod+Shift+A"), "Ctrl+Shift+A");
        assert_eq!(Preset::AppleMail.hint(Command::Send, System::Mac).as_deref(), Some("Shift+Cmd+D"));
        let hint = |command, system| Preset::Torro.hint(command, system).expect("a hint");
        assert_eq!(hint(Command::Send, System::Linux), "Ctrl+Enter");
        assert_eq!(hint(Command::Send, System::Mac), "Cmd+Enter");
        assert_eq!(hint(Command::ReplyAll, System::Linux), "Ctrl+Shift+R");
        assert_eq!(hint(Command::ReplyAll, System::Mac), "Shift+Cmd+R");
        assert_eq!(hint(Command::Archive, System::Linux), "Ctrl+Shift+A");
        assert_eq!(hint(Command::Archive, System::Mac), "Ctrl+Cmd+A");
        assert_eq!(hint(Command::Delete, System::Linux), "Del");
        assert_eq!(hint(Command::Delete, System::Mac), "Delete", "a Mac keyboard's ⌫");
        assert_eq!(hint(Command::DeletePermanently, System::Linux), "Shift+Del");
        assert_eq!(hint(Command::DeletePermanently, System::Mac), "Shift+Delete");
        let alt: Shortcut = "Alt+Shift+Escape".parse().expect("valid");
        assert_eq!(
            (alt.hint(System::Linux), alt.hint(System::Mac)),
            ("Alt+Shift+Esc".into(), "Option+Shift+Esc".into())
        );
    }

    #[test]
    fn the_notation_parses_and_writes_back() {
        for text in [
            "Mod+Shift+A",
            "Ctrl+Cmd+A",
            "Shift+Delete",
            "Mod+Enter",
            "Alt+F",
            "Mod+/",
            "Backspace",
            "7",
            "F5",
            "Shift+F12",
            "Insert",
            "F",
        ] {
            let shortcut: Shortcut = text.parse().expect(text);
            assert_eq!(shortcut.to_string(), text);
        }
        let loose: Shortcut = " shift + mod + a ".parse().expect("any case, any order, spaces");
        assert_eq!(loose.to_string(), "Mod+Shift+A");
        assert_eq!(loose.key(), letter('a'));
        assert_eq!("cmd+ctrl+enter".parse::<Shortcut>().map(|s| s.to_string()), Ok("Ctrl+Cmd+Enter".into()));
    }

    #[test]
    fn the_notation_refuses_what_it_cannot_mean() {
        let refused = |text: &str| text.parse::<Shortcut>().expect_err(text);
        assert_eq!(refused(""), ParseError::MissingKey);
        assert_eq!(refused("Mod+"), ParseError::MissingKey);
        assert_eq!(refused("Shift"), ParseError::MissingKey);
        assert_eq!(refused("Hyper+A"), ParseError::UnknownModifier("Hyper".into()));
        assert_eq!(refused("Mod+Home"), ParseError::UnknownKey("Home".into()));
        assert_eq!(refused("Mod+ä"), ParseError::UnknownKey("ä".into()));
        assert_eq!(refused("F0"), ParseError::UnknownKey("F0".into()));
        assert_eq!(refused("F13"), ParseError::UnknownKey("F13".into()));
        assert_eq!(refused("F05"), ParseError::UnknownKey("F05".into()));
        assert_eq!(refused("Fetch"), ParseError::UnknownKey("Fetch".into()));
        assert_eq!("f5".parse::<Shortcut>().map(|s| s.to_string()), Ok("F5".into()), "any case");
        assert_eq!(refused("Shift+Shift+A"), ParseError::Repeated("Shift".into()));
        assert_eq!(refused("Mod+Ctrl+A"), ParseError::ModWithCtrlOrCmd);
        assert_eq!(refused("Mod+Cmd+A"), ParseError::ModWithCtrlOrCmd);
        assert!(!refused("Mod+Ctrl+A").to_string().is_empty());
    }

    #[test]
    fn commands_have_stable_names() {
        for command in Command::ALL {
            assert_eq!(Command::from_name(command.name()), Some(command));
        }
        assert_eq!(Command::from_name("delete_permanently"), Some(Command::DeletePermanently));
        assert_eq!(Command::from_name("undo"), None, "undo is the suite's, not a mail shortcut");
        // The values of task E10.2's `mail_shortcut_preset`: an API, so spelled out here.
        let names: Vec<&str> = Preset::ALL.into_iter().map(Preset::name).collect();
        assert_eq!(names, ["torro", "apple_mail", "thunderbird", "outlook"]);
        for preset in Preset::ALL {
            assert_eq!(Preset::from_name(preset.name()), Some(preset));
        }
        assert_eq!(Preset::from_name("gmail"), None, "Gmail comes with task E10.2");
    }

    #[test]
    fn only_search_and_send_work_in_a_text_field() {
        let typing: Vec<Command> = Command::ALL.into_iter().filter(|c| c.works_in_text_field()).collect();
        assert_eq!(typing, [Command::Send, Command::Search]);
    }

    #[test]
    fn keys_of_typed_characters() {
        assert_eq!(Key::of_char('R'), Some(letter('r')));
        assert_eq!(Key::of_char('#'), Some(Key::Char('#')));
        assert_eq!(Key::of_char('+'), None);
        assert_eq!(Key::of_char(' '), None);
        assert_eq!(Key::of_char('ß'), None);
    }
}
