//! The keyboard shortcuts of the mail window (task E10.1 in `docs/tasks/eingang.md`, section E10):
//! which [`Command`]s there are, how a key combination is written ([`Shortcut`]) and the preset
//! "Torro" ([`Preset::Torro`]) that binds them.
//!
//! Without Iced, so the core can check the same table when a person binds keys of their own (task
//! E10.2). The window turns a key press into [`Held`] and a [`Key`] and asks
//! [`Preset::command`]; hints next to buttons come from [`Preset::hint`].
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

/// The system whose conventions a shortcut follows: `Mod` and the names in hints differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    /// macOS: `Mod` is ⌘ or Ctrl, hints in Apple's order with "Cmd" and "Option".
    Mac,
    /// Windows and Linux: `Mod` is Ctrl.
    Other,
}

impl System {
    /// The system this program runs on.
    pub fn current() -> System {
        if cfg!(target_os = "macos") { System::Mac } else { System::Other }
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

/// The named keys and how the notation spells them.
const NAMED: [(Key, &str); 10] = [
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
            named => NAMED.iter().find(|(key, _)| *key == named).map_or_else(String::new, |(_, name)| (*name).into()),
        }
    }

    /// The key as a hint on `system` writes it: Backspace is "Delete" on a Mac keyboard, Delete
    /// there and elsewhere "Del"; Escape is "Esc".
    fn hint(self, system: System) -> String {
        match (self, system) {
            (Key::Backspace, System::Mac) => "Delete".into(),
            (Key::Delete, _) => "Del".into(),
            (Key::Escape, _) => "Esc".into(),
            (key, _) => key.name(),
        }
    }

    /// The key named `name` in the notation; `None` for an unknown name.
    fn parse(name: &str) -> Option<Key> {
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Key::of_char(c);
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
            System::Other => held.ctrl && !held.cmd,
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
            System::Other => {
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

/// A set of bindings to start from. Task E10.2 adds Apple Mail, Outlook, Thunderbird and Gmail and
/// lets a person bind keys over the chosen one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// The standard (`torro`): the usual shortcuts, else Apple Mail's, moving as in Outlook – the
    /// table "Vorlage „Torro“" in `docs/tasks/eingang.md`. Ctrl + A stays "select all", so archiving
    /// is Ctrl + Shift + A (⌃⌘A on macOS, as in Apple Mail).
    #[default]
    Torro,
}

/// The preset "Torro": each command with its keys on Windows and Linux, then on macOS, in the
/// notation. The first keys of a command are the ones its hint shows.
const TORRO: [(Command, &[&str], &[&str]); 14] = [
    (Command::NewMail, &["Mod+N"], &["Mod+N"]),
    (Command::Reply, &["Mod+R"], &["Mod+R"]),
    (Command::ReplyAll, &["Mod+Shift+R"], &["Mod+Shift+R"]),
    (Command::Forward, &["Mod+Shift+F"], &["Mod+Shift+F"]),
    (Command::Send, &["Mod+Enter"], &["Mod+Enter"]),
    (Command::Delete, &["Delete", "Backspace"], &["Backspace", "Delete"]),
    (Command::DeletePermanently, &["Shift+Delete"], &["Shift+Backspace", "Shift+Delete"]),
    (Command::Archive, &["Mod+Shift+A"], &["Ctrl+Cmd+A"]),
    (Command::ToggleRead, &["Mod+Shift+U"], &["Mod+Shift+U"]),
    (Command::Flag, &["Mod+Shift+L"], &["Mod+Shift+L"]),
    (Command::Junk, &["Mod+Shift+J"], &["Mod+Shift+J"]),
    (Command::MoveToFolder, &["Mod+Shift+V"], &["Mod+Shift+V"]),
    (Command::Search, &["Mod+F"], &["Mod+F"]),
    (Command::Fetch, &["Mod+Shift+N"], &["Mod+Shift+N"]),
];

impl Preset {
    /// The preset's name in settings (`"torro"`).
    pub fn name(self) -> &'static str {
        match self {
            Preset::Torro => "torro",
        }
    }

    /// The keys of `command` in this preset on `system`, the one hints show first; empty when the
    /// preset gives the command no keys.
    pub fn keys(self, command: Command, system: System) -> Vec<Shortcut> {
        let table = match self {
            Preset::Torro => &TORRO,
        };
        let written = table.iter().find(|(c, _, _)| *c == command).map_or(&[][..], |(_, other, mac)| match system {
            System::Mac => *mac,
            System::Other => *other,
        });
        // The table is tested to parse whole; nothing is dropped here.
        written.iter().filter_map(|text| text.parse().ok()).collect()
    }

    /// The command that `key` pressed with exactly `held` runs on `system`, if any.
    pub fn command(self, system: System, held: Held, key: Key) -> Option<Command> {
        Command::ALL
            .into_iter()
            .find(|command| self.keys(*command, system).iter().any(|shortcut| shortcut.matches(system, held, key)))
    }

    /// The hint of `command` on `system` ([`Shortcut::hint`] of its first keys): "Ctrl+Enter",
    /// "Cmd+Enter". `None` when it has no keys.
    pub fn hint(self, command: Command, system: System) -> Option<String> {
        self.keys(command, system).first().map(|shortcut| shortcut.hint(system))
    }
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
            assert_eq!(torro.command(System::Other, held(CTRL), key), Some(command), "Ctrl: {command:?}");
            assert_eq!(torro.command(System::Other, held(CMD), key), None, "the logo key is no Ctrl: {command:?}");
            assert_eq!(torro.command(System::Mac, held(CMD), key), Some(command), "⌘: {command:?}");
            assert_eq!(torro.command(System::Mac, held(CTRL), key), Some(command), "Ctrl on a Mac: {command:?}");
            let both = Held { ctrl: true, cmd: true, ..held(NONE) };
            assert_eq!(torro.command(System::Mac, both, key), None, "⌃⌘ is its own combination: {command:?}");
            assert_eq!(torro.command(System::Other, held(NONE), key), None, "a plain key: {command:?}");
        }
    }

    #[test]
    fn archive_is_ctrl_shift_a_and_on_a_mac_ctrl_cmd_a() {
        let torro = Preset::Torro;
        let a = letter('a');
        assert_eq!(torro.command(System::Other, with_shift(CTRL), a), Some(Command::Archive));
        let ctrl_cmd = Held { ctrl: true, cmd: true, ..NONE };
        assert_eq!(torro.command(System::Mac, ctrl_cmd, a), Some(Command::Archive));
        assert_eq!(torro.command(System::Mac, with_shift(CMD), a), None, "not the Linux one on a Mac");
    }

    #[test]
    fn ctrl_a_stays_select_all() {
        let torro = Preset::Torro;
        for system in [System::Other, System::Mac] {
            for held in [CTRL, CMD] {
                assert_eq!(torro.command(system, held, letter('a')), None, "{system:?} {held:?}");
            }
        }
    }

    #[test]
    fn delete_moves_to_the_trash_and_shift_delete_removes_for_good() {
        let torro = Preset::Torro;
        for system in [System::Other, System::Mac] {
            assert_eq!(torro.command(system, NONE, Key::Delete), Some(Command::Delete), "{system:?}");
            assert_eq!(torro.command(system, NONE, Key::Backspace), Some(Command::Delete), "{system:?}");
            assert_eq!(torro.command(system, SHIFT, Key::Delete), Some(Command::DeletePermanently), "{system:?}");
        }
        assert_eq!(torro.command(System::Mac, SHIFT, Key::Backspace), Some(Command::DeletePermanently));
        assert_eq!(torro.command(System::Other, SHIFT, Key::Backspace), None, "only Shift + Delete there");
        assert_eq!(torro.command(System::Other, CTRL, Key::Delete), None);
    }

    #[test]
    fn every_command_has_keys_on_both_systems_and_no_two_share_one() {
        for system in [System::Other, System::Mac] {
            let mut all: Vec<String> = Vec::new();
            for command in Command::ALL {
                let keys = Preset::Torro.keys(command, system);
                assert!(!keys.is_empty(), "{command:?} on {system:?}");
                all.extend(keys.iter().map(ToString::to_string));
            }
            let mut unique = all.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), all.len(), "{system:?}: {all:?}");
        }
        // The table parses whole: as many keys as it writes.
        for (_, other, mac) in TORRO {
            for text in other.iter().chain(mac) {
                assert!(text.parse::<Shortcut>().is_ok(), "{text}");
            }
        }
    }

    #[test]
    fn hints_name_the_keys_as_each_system_does() {
        let hint = |command, system| Preset::Torro.hint(command, system).expect("a hint");
        assert_eq!(hint(Command::Send, System::Other), "Ctrl+Enter");
        assert_eq!(hint(Command::Send, System::Mac), "Cmd+Enter");
        assert_eq!(hint(Command::ReplyAll, System::Other), "Ctrl+Shift+R");
        assert_eq!(hint(Command::ReplyAll, System::Mac), "Shift+Cmd+R");
        assert_eq!(hint(Command::Archive, System::Other), "Ctrl+Shift+A");
        assert_eq!(hint(Command::Archive, System::Mac), "Ctrl+Cmd+A");
        assert_eq!(hint(Command::Delete, System::Other), "Del");
        assert_eq!(hint(Command::Delete, System::Mac), "Delete", "a Mac keyboard's ⌫");
        assert_eq!(hint(Command::DeletePermanently, System::Other), "Shift+Del");
        assert_eq!(hint(Command::DeletePermanently, System::Mac), "Shift+Delete");
        let alt: Shortcut = "Alt+Shift+Escape".parse().expect("valid");
        assert_eq!(
            (alt.hint(System::Other), alt.hint(System::Mac)),
            ("Alt+Shift+Esc".into(), "Option+Shift+Esc".into())
        );
    }

    #[test]
    fn the_notation_parses_and_writes_back() {
        for text in ["Mod+Shift+A", "Ctrl+Cmd+A", "Shift+Delete", "Mod+Enter", "Alt+F", "Mod+/", "Backspace", "7"] {
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
        assert_eq!(Preset::Torro.name(), "torro");
        assert_eq!(Preset::default(), Preset::Torro);
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
