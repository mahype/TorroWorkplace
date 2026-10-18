//! The keyboard of the inbox (task E3.2 in `docs/tasks/eingang.md`): which key does what.
//!
//! Two steps, so the map can be tested without a window:
//!
//! 1. [`listen`] turns a key press into a [`Key`] – only plain keys (no Ctrl, Alt or logo key) that
//!    no widget has taken. A text field that has the focus takes the characters typed into it, so
//!    they never reach the list: the title field of "Make task" (task E3.5) takes letters, the
//!    arrows and `Enter` (which creates the task through its `on_submit`).
//! 2. [`command`] says what a [`Key`] does where the keys go ([`Focus`]): in the list `j`/`k` (and
//!    ↓/↑) move, `Enter` opens, `e` done, `u` undo (as Ctrl + Z, ⌘ + Z on macOS, does in the whole
//!    window while the toast offers it – `widgets::undo_key`, task E3.11, not a key of this map:
//!    `ctrl_guard` takes ⌘ + Z before any widget, only a subscription still hears it), `s` snooze, `l` lane, `m` hide from the inbox
//!    (task E3.10: its menu, which also mutes), `t` make a task; for mail (task E4.2) `r` reply, `a` reply all, `f` forward, `#` or Delete (⌫ on a Mac keyboard, issue #118) delete and `/` search
//!    the folder (in a folder `e` archives: the app decides), `v` switches the mail in the panel or
//!    reader between HTML and text (task M9.7; the app decides whether it has HTML); in a menu of choices the digits pick
//!    its entries; in the date picker the arrows change
//!    day and time and `Enter` snoozes; in the dialog of "Make task" `Enter` creates the task.
//!    Behind the navigation menu, the app switcher or the drawer no key reaches the list. Escape
//!    is not here: `widgets::escape` closes whatever is in front.
//!
//! The overview (task E3.9, [`Focus::Overview`]) takes the keys of the list, and `h`/`l` and ←/→
//! change the column. There `l` changes the column instead of moving the entry to the other lane:
//! the overview shows both lanes, and "Move to …" stays in the panel. In the notifications window
//! (task E8.4) the same keys go from app column to app column.
//!
//! `x` dismisses a notification (task E8.4, the card's ✕): done, counted apart. The app decides
//! where it works – never for mail and approvals.
//!
//! The bell's menu (task E8.6b, [`Focus::Mute`]; `m` in the notifications window) takes the digits
//! for its ways to mute, ←/→ for "For" (shorter, longer) and ↑/↓ for "What happens" (milder,
//! stronger), as its segments lie from left to right.
//!
//! The hub's "Act now" (task E8.5) is one list of sections, so it takes the keys of the list
//! ([`Focus::List`]): `j`/`k` walk Decide, Agents and To sort, `l` moves an entry to the other lane.
//! `s` on an approval opens no snooze menu – an approval waits for yes or no.
//!
//! `c` makes an event of the entry (task E8.5b, "Event" in To sort and the panel): its dialog takes
//! the keys as the one of "Make task" does ([`Focus::Task`]), `Enter` creates the event.
//!
//! **Shortcuts** (tasks E10.1 and E10.1b, section E10 of `docs/tasks/eingang.md`) come from the
//! table in `torro_core::shortcuts`, in the preset of the system (`Preset::standard`: Apple Mail on
//! macOS, Thunderbird on Linux, Outlook on Windows), on top of the single keys, which stay as they
//! are:
//!
//! - With Ctrl or ⌘ held, and for the function keys and Insert also without, [`shortcut`] reads
//!   the table – Ctrl + R reply, Ctrl + Shift + A archive, Ctrl + Shift + V move to a folder, Ctrl +
//!   K search and F5 fetch in Thunderbird … (on macOS ⌘ or Ctrl, archive ⌃⌘A) – and sends
//!   [`Message::Shortcut`]. F5 (F9 in Outlook) fetches with `accounts.sync`, which brings in every
//!   account, not only mail. It hears the key also when a widget took it: the window's
//!   `widgets::ctrl_guard` captures ⌘ + letter on macOS, while a text field does not capture Ctrl +
//!   Shift + letter. So the status says nothing about where the keyboard is, and the app asks Iced
//!   whether the search field has the focus before it acts (`App::shortcut`); only search and send
//!   work while it has ([`torro_core::shortcuts::Command::works_in_text_field`]). Ctrl + A is no
//!   shortcut: it stays "select all".
//! - Without Ctrl or ⌘, Delete and Backspace ask the table too ([`key_on`]): plain they delete (as
//!   since issue #118), Shift + Delete (on macOS also Shift + Backspace) deletes for good after a
//!   question ([`Key::DeleteForGood`], [`Focus::Question`]: `Enter` confirms, Escape cancels). A
//!   text field takes them first, as any single key.

use iced::keyboard::{self, Key as Name, key::Named};
use torro_core::shortcuts::{self, Preset, System};
use torro_gui::{shell, widgets};

use crate::app::Message;

/// A key the inbox knows, before it is decided what it does ([`command`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// `j` or ↓.
    Down,
    /// `k` or ↑.
    Up,
    /// ← (in the overview also `h`).
    Left,
    /// →.
    Right,
    /// Enter.
    Enter,
    /// `e`.
    E,
    /// `u`.
    U,
    /// `s`.
    S,
    /// `l`.
    L,
    /// `h`.
    H,
    /// `m`.
    M,
    /// `x`.
    X,
    /// `t`.
    T,
    /// `c`.
    C,
    /// `r`.
    R,
    /// `a`.
    A,
    /// `f`.
    F,
    /// `v`.
    V,
    /// The hash sign (Shift + 3 on many keyboards: Shift does not stop it).
    Hash,
    /// Delete or Backspace (the key Mac keyboards call "delete", issue #118). A text field that has
    /// the focus takes it before the list hears it.
    Delete,
    /// Shift + Delete, on macOS also Shift + Backspace (task E10.1): delete for good, after a
    /// question. A text field takes it first.
    DeleteForGood,
    /// `/`.
    Slash,
    /// Tab: the first suggestion under the search field (issue #136).
    Tab,
    /// `1` … `9`.
    Digit(u8),
    /// `[` ([`shell::is_sidebar_key`]).
    Sidebar,
}

/// Where the keys go: what is in front of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Nothing in front: the list (and the panel showing its entry) takes the triage keys.
    List,
    /// Nothing in front of the overview (task E3.9): the keys of the list, and the column keys.
    Overview,
    /// A menu of choices for an entry is open (snooze, lane, hide); its entries carry digits.
    Choices,
    /// The bell's menu (task E8.6b): its ways to mute carry digits, the arrows change "For" and
    /// "What happens".
    Mute,
    /// The date and time picker of "Pick…" is open.
    Picker,
    /// The dialog of "Make task" (task E3.5) or of "Make event" (task E8.5b) is open; its title
    /// field takes what is typed.
    Task,
    /// The navigation menu, the app switcher or the drawer is open; only `[` works.
    Elsewhere,
    /// A question waits for its answer (task E10.1: "Delete permanently?"): `Enter` confirms, Escape
    /// (the app's) cancels, nothing of the list.
    Question,
}

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Move the cursor to the next row.
    Next,
    /// Move the cursor to the previous row.
    Previous,
    /// The overview: the cursor to the column at the left (`h`, ←).
    ColumnLeft,
    /// The overview: the cursor to the column at the right (`l`, →).
    ColumnRight,
    /// Open the entry under the cursor: its page, app or agent's window (`inbox.open_item`), else
    /// the panel.
    Open,
    /// Mark the entry done, or open it again when it is done.
    Done,
    /// Dismiss the notification (task E8.4, the ✕): done without doing anything, counted apart.
    Dismiss,
    /// Undo the last change while its hint shows.
    Undo,
    /// Open the snooze menu.
    Snooze,
    /// Move the entry to the other lane (asking whether always for its source).
    Lane,
    /// Open the menu of `m`: the bell's menu (task E8.6b) for mail and in the notifications window
    /// (issue #123), else "Hide from Notification Hub" (task E3.10): the entry, its source.
    Hide,
    /// Open the dialog that makes a task of the entry (task E3.5).
    MakeTask,
    /// Open the dialog that makes an event of the entry (task E8.5b).
    MakeEvent,
    /// Reply to the mail under the cursor (task E4.2).
    Reply,
    /// Reply to everyone on the mail under the cursor.
    ReplyAll,
    /// Forward the mail under the cursor.
    Forward,
    /// Move the mail under the cursor to the trash.
    Delete,
    /// Ask whether to remove the mail under the cursor for good (task E10.1), then do it.
    DeleteForGood,
    /// Move the mail under the cursor to its account's archive – in every view, also at an entry
    /// (task E10.1; `e` archives only in the views of mail, at an entry it is Done).
    Archive,
    /// Mark the mail under the cursor read when it is unread, else unread (task E10.1).
    ToggleRead,
    /// Flag the mail under the cursor, or take its flag away (task E10.1).
    Flag,
    /// Move the mail under the cursor to its account's junk folder (task E10.1).
    Junk,
    /// Open the choice of folders to move the mail under the cursor to (task E10.1).
    MoveToFolder,
    /// Open the compose window for a new message (task E10.1, as "New" does).
    NewMail,
    /// Fetch mail from the servers now (task E10.1, `accounts.sync`), then list anew.
    Fetch,
    /// Put the keyboard into the search field over the list (issue #136).
    Search,
    /// Switch the mail in the panel or reader between HTML and text (task M9.7, the switch
    /// "HTML | Text"); nothing for a mail without HTML.
    SwitchView,
    /// Take the first suggestion under the search field (issue #136).
    Complete,
    /// Pick the n-th entry (from 1) of the open menu of choices.
    Choose(usize),
    /// The bell's menu: "For" one step longer (→, `true`) or shorter (←).
    MuteFor { longer: bool },
    /// The bell's menu: "What happens" one step stronger (↓, `true`: towards "Hide") or milder (↑).
    MuteHow { stronger: bool },
    /// Picker: one day earlier.
    EarlierDay,
    /// Picker: one day later.
    LaterDay,
    /// Picker: the previous time of the day.
    EarlierTime,
    /// Picker: the next time of the day.
    LaterTime,
    /// Picker: snooze to the picked time; dialog of "Make task" or "Make event": create it; the
    /// question "Delete permanently?": delete.
    Confirm,
    /// Collapse or expand the sidebar, open or close the drawer.
    Sidebar,
}

/// The key of a key press, for `iced::event::listen_with`: `None` for keys a widget took, keys
/// held with Ctrl, Alt or the logo key, and keys the inbox does not use.
pub fn listen(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    key_of(&key, modifiers).map(Message::Key)
}

/// A shortcut with Ctrl or ⌘, or a function key or Insert ([`shortcut_on`] on this system), for
/// `iced::event::listen_with`: it hears the key also when a text field or the window's
/// `widgets::ctrl_guard` took it – the guard does for ⌘ + letter on macOS, so the field does not
/// type the letter. The app decides whether it
/// acts while a field has the focus (`App::shortcut`).
pub fn shortcut(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    shortcut_on(System::current(), &key, physical_key, modifiers).map(Message::Shortcut)
}

/// The command of the system's preset (`Preset::standard`) that `key` held with `modifiers` runs on
/// `system`, for presses with Ctrl or ⌘ (the logo key), and for the function keys and Insert, which
/// no single key and no text field uses: the others are single keys ([`key_on`]), which a text
/// field takes first. On any layout: the key in a letter's place on a non-Latin one (`to_latin`).
pub fn shortcut_on(
    system: System,
    key: &Name,
    physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<shortcuts::Command> {
    let key = table_key(key, physical_key)?;
    let free = matches!(key, shortcuts::Key::F(_) | shortcuts::Key::Insert);
    if !modifiers.control() && !modifiers.logo() && !free {
        return None;
    }
    Preset::standard(system).command(system, widgets::held(modifiers), key)
}

/// The key of the shortcut table for a key press: the named keys it knows, else the character
/// (letters lower case); `None` for keys a shortcut cannot name.
pub fn table_key(key: &Name, physical_key: keyboard::key::Physical) -> Option<shortcuts::Key> {
    Some(match key.as_ref() {
        Name::Named(Named::Enter) => shortcuts::Key::Enter,
        Name::Named(Named::Delete) => shortcuts::Key::Delete,
        Name::Named(Named::Backspace) => shortcuts::Key::Backspace,
        Name::Named(Named::Tab) => shortcuts::Key::Tab,
        Name::Named(Named::Escape) => shortcuts::Key::Escape,
        Name::Named(Named::Space) => shortcuts::Key::Space,
        Name::Named(Named::ArrowUp) => shortcuts::Key::Up,
        Name::Named(Named::ArrowDown) => shortcuts::Key::Down,
        Name::Named(Named::ArrowLeft) => shortcuts::Key::Left,
        Name::Named(Named::ArrowRight) => shortcuts::Key::Right,
        Name::Named(Named::Insert) => shortcuts::Key::Insert,
        Name::Named(Named::F1) => shortcuts::Key::F(1),
        Name::Named(Named::F2) => shortcuts::Key::F(2),
        Name::Named(Named::F3) => shortcuts::Key::F(3),
        Name::Named(Named::F4) => shortcuts::Key::F(4),
        Name::Named(Named::F5) => shortcuts::Key::F(5),
        Name::Named(Named::F6) => shortcuts::Key::F(6),
        Name::Named(Named::F7) => shortcuts::Key::F(7),
        Name::Named(Named::F8) => shortcuts::Key::F(8),
        Name::Named(Named::F9) => shortcuts::Key::F(9),
        Name::Named(Named::F10) => shortcuts::Key::F(10),
        Name::Named(Named::F11) => shortcuts::Key::F(11),
        Name::Named(Named::F12) => shortcuts::Key::F(12),
        Name::Character(_) => shortcuts::Key::of_char(key.to_latin(physical_key)?)?,
        _ => return None,
    })
}

/// What a shortcut of the table does in the inbox window: the [`Command`] its single key (or button)
/// does there. `None` for sending, which belongs to the compose window.
pub fn of_shortcut(command: shortcuts::Command) -> Option<Command> {
    use shortcuts::Command as Shortcut;
    Some(match command {
        Shortcut::NewMail => Command::NewMail,
        Shortcut::Reply => Command::Reply,
        Shortcut::ReplyAll => Command::ReplyAll,
        Shortcut::Forward => Command::Forward,
        Shortcut::Delete => Command::Delete,
        Shortcut::DeletePermanently => Command::DeleteForGood,
        Shortcut::Archive => Command::Archive,
        Shortcut::ToggleRead => Command::ToggleRead,
        Shortcut::Flag => Command::Flag,
        Shortcut::Junk => Command::Junk,
        Shortcut::MoveToFolder => Command::MoveToFolder,
        Shortcut::Search => Command::Search,
        Shortcut::Fetch => Command::Fetch,
        Shortcut::Send => return None,
    })
}

/// The [`Key`] for `key` held with `modifiers` ([`key_on`] on this system).
pub fn key_of(key: &Name, modifiers: keyboard::Modifiers) -> Option<Key> {
    key_on(key, modifiers, System::current())
}

/// The [`Key`] for `key` held with `modifiers` on `system`. Delete and Backspace are what the
/// shortcut table of the system's preset says for them there (tasks E10.1, E10.1b): plain
/// [`Key::Delete`], with Shift [`Key::DeleteForGood`] where the table has it, else nothing.
pub fn key_on(key: &Name, modifiers: keyboard::Modifiers, system: System) -> Option<Key> {
    if shell::is_sidebar_key(key, modifiers) {
        return Some(Key::Sidebar);
    }
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    Some(match key.as_ref() {
        Name::Named(Named::ArrowDown) | Name::Character("j") => Key::Down,
        Name::Named(Named::ArrowUp) | Name::Character("k") => Key::Up,
        Name::Named(Named::ArrowLeft) => Key::Left,
        Name::Named(Named::ArrowRight) => Key::Right,
        Name::Named(Named::Enter) => Key::Enter,
        Name::Named(Named::Tab) if !modifiers.shift() => Key::Tab,
        Name::Character("e") => Key::E,
        Name::Character("u") => Key::U,
        Name::Character("s") => Key::S,
        Name::Character("l") => Key::L,
        Name::Character("h") => Key::H,
        Name::Character("m") => Key::M,
        Name::Character("x") => Key::X,
        Name::Character("t") => Key::T,
        Name::Character("c") => Key::C,
        Name::Character("r") => Key::R,
        Name::Character("a") => Key::A,
        Name::Character("f") => Key::F,
        Name::Character("v") => Key::V,
        Name::Character("#") => Key::Hash,
        Name::Named(named @ (Named::Delete | Named::Backspace)) => {
            let pressed = if named == Named::Delete { shortcuts::Key::Delete } else { shortcuts::Key::Backspace };
            match Preset::standard(system).command(system, widgets::held(modifiers), pressed) {
                Some(shortcuts::Command::Delete) => Key::Delete,
                Some(shortcuts::Command::DeletePermanently) => Key::DeleteForGood,
                _ => return None,
            }
        }
        Name::Character("/") => Key::Slash,
        Name::Character(digit) => match digit.parse::<u8>() {
            Ok(n @ 1..=9) => Key::Digit(n),
            _ => return None,
        },
        _ => return None,
    })
}

/// What `key` does while the keys go to `focus`; `None` when it does nothing there.
pub fn command(focus: Focus, key: Key) -> Option<Command> {
    match (focus, key) {
        (Focus::Overview, Key::Left | Key::H) => Some(Command::ColumnLeft),
        (Focus::Overview, Key::Right | Key::L) => Some(Command::ColumnRight),
        (Focus::Overview, key) => command(Focus::List, key),
        (Focus::List | Focus::Elsewhere, Key::Sidebar) => Some(Command::Sidebar),
        (Focus::List, Key::Down) => Some(Command::Next),
        (Focus::List, Key::Up) => Some(Command::Previous),
        (Focus::List, Key::Enter) => Some(Command::Open),
        (Focus::List, Key::E) => Some(Command::Done),
        (Focus::List, Key::U) => Some(Command::Undo),
        (Focus::List, Key::S) => Some(Command::Snooze),
        (Focus::List, Key::L) => Some(Command::Lane),
        (Focus::List, Key::M) => Some(Command::Hide),
        (Focus::List, Key::X) => Some(Command::Dismiss),
        (Focus::List, Key::T) => Some(Command::MakeTask),
        (Focus::List, Key::C) => Some(Command::MakeEvent),
        (Focus::List, Key::R) => Some(Command::Reply),
        (Focus::List, Key::A) => Some(Command::ReplyAll),
        (Focus::List, Key::F) => Some(Command::Forward),
        (Focus::List, Key::Hash | Key::Delete) => Some(Command::Delete),
        (Focus::List, Key::DeleteForGood) => Some(Command::DeleteForGood),
        (Focus::List, Key::Slash) => Some(Command::Search),
        (Focus::List, Key::Tab) => Some(Command::Complete),
        (Focus::List, Key::V) => Some(Command::SwitchView),
        (Focus::Choices | Focus::Mute, Key::Digit(n)) => Some(Command::Choose(usize::from(n))),
        (Focus::Mute, Key::Left) => Some(Command::MuteFor { longer: false }),
        (Focus::Mute, Key::Right) => Some(Command::MuteFor { longer: true }),
        (Focus::Mute, Key::Up) => Some(Command::MuteHow { stronger: false }),
        (Focus::Mute, Key::Down) => Some(Command::MuteHow { stronger: true }),
        (Focus::Picker, Key::Left) => Some(Command::EarlierDay),
        (Focus::Picker, Key::Right) => Some(Command::LaterDay),
        (Focus::Picker, Key::Up) => Some(Command::EarlierTime),
        (Focus::Picker, Key::Down) => Some(Command::LaterTime),
        (Focus::Picker | Focus::Task | Focus::Question, Key::Enter) => Some(Command::Confirm),
        _ => None,
    }
}

/// The key hint of the n-th entry (from 1) of a menu of choices: `"1"` … `"9"`.
pub fn digit(n: usize) -> &'static str {
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    DIGITS.get(n.wrapping_sub(1)).copied().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use iced::event::Status;
    use iced::keyboard::{Location, Modifiers, key::NativeCode, key::Physical};

    use super::*;

    fn press(key: Name, modifiers: Modifiers) -> iced::Event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    fn char(c: &str) -> Name {
        Name::Character(c.into())
    }

    fn heard(key: Name, modifiers: Modifiers, status: Status) -> Option<Key> {
        match listen(press(key, modifiers), status, iced::window::Id::unique()) {
            Some(Message::Key(key)) => Some(key),
            _ => None,
        }
    }

    #[test]
    fn every_key_of_the_list_has_its_command() {
        let none = Modifiers::empty();
        for (key, wanted) in [
            (char("j"), Command::Next),
            (Name::Named(Named::ArrowDown), Command::Next),
            (char("k"), Command::Previous),
            (Name::Named(Named::ArrowUp), Command::Previous),
            (Name::Named(Named::Enter), Command::Open),
            (char("e"), Command::Done),
            (char("u"), Command::Undo),
            (char("s"), Command::Snooze),
            (char("l"), Command::Lane),
            (char("m"), Command::Hide),
            (char("x"), Command::Dismiss),
            (char("t"), Command::MakeTask),
            (char("c"), Command::MakeEvent),
            (char("r"), Command::Reply),
            (char("a"), Command::ReplyAll),
            (char("f"), Command::Forward),
            (char("#"), Command::Delete),
            (Name::Named(Named::Delete), Command::Delete),
            (Name::Named(Named::Backspace), Command::Delete),
            (char("/"), Command::Search),
            (char("v"), Command::SwitchView),
            (char("["), Command::Sidebar),
        ] {
            let key = heard(key, none, Status::Ignored).expect("a key of the inbox");
            assert_eq!(command(Focus::List, key), Some(wanted), "{key:?}");
        }
    }

    #[test]
    fn the_overview_changes_columns_and_keeps_the_keys_of_the_list() {
        let none = Modifiers::empty();
        for (key, wanted) in [
            (char("h"), Command::ColumnLeft),
            (Name::Named(Named::ArrowLeft), Command::ColumnLeft),
            (char("l"), Command::ColumnRight),
            (Name::Named(Named::ArrowRight), Command::ColumnRight),
            (char("j"), Command::Next),
            (char("k"), Command::Previous),
            (Name::Named(Named::Enter), Command::Open),
            (char("e"), Command::Done),
            (char("s"), Command::Snooze),
            (char("m"), Command::Hide),
            (char("x"), Command::Dismiss),
            (char("t"), Command::MakeTask),
            (char("["), Command::Sidebar),
        ] {
            let key = heard(key, none, Status::Ignored).expect("a key of the inbox");
            assert_eq!(command(Focus::Overview, key), Some(wanted), "{key:?}");
        }
        assert_eq!(command(Focus::List, Key::L), Some(Command::Lane), "l keeps the lane in the other views");
        assert_eq!(command(Focus::List, Key::H), None);
    }

    #[test]
    fn keys_a_widget_took_or_with_modifiers_do_nothing() {
        // A text field that has the focus captures what is typed into it.
        assert_eq!(heard(char("e"), Modifiers::empty(), Status::Captured), None);
        for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            assert_eq!(heard(char("e"), modifiers, Status::Ignored), None, "{modifiers:?}");
        }
        assert_eq!(heard(char("q"), Modifiers::empty(), Status::Ignored), None, "not a key of the inbox");
        assert_eq!(heard(char("0"), Modifiers::empty(), Status::Ignored), None, "choices count from 1");
        assert_eq!(heard(char("E"), Modifiers::SHIFT, Status::Ignored), None, "Shift + e is not e");
    }

    #[test]
    fn menus_take_digits_and_nothing_else() {
        assert_eq!(heard(char("2"), Modifiers::empty(), Status::Ignored), Some(Key::Digit(2)));
        assert_eq!(command(Focus::Choices, Key::Digit(2)), Some(Command::Choose(2)));
        assert_eq!(command(Focus::List, Key::Digit(2)), None, "digits only pick menu entries");
        for key in [Key::Down, Key::E, Key::S, Key::M, Key::X, Key::Enter, Key::Sidebar] {
            assert_eq!(command(Focus::Choices, key), None, "{key:?} stays out of the open menu");
        }
    }

    #[test]
    fn the_bells_menu_takes_digits_and_arrows() {
        assert_eq!(command(Focus::Mute, Key::Digit(3)), Some(Command::Choose(3)));
        assert_eq!(command(Focus::Mute, Key::Left), Some(Command::MuteFor { longer: false }));
        assert_eq!(command(Focus::Mute, Key::Right), Some(Command::MuteFor { longer: true }));
        assert_eq!(command(Focus::Mute, Key::Up), Some(Command::MuteHow { stronger: false }));
        assert_eq!(command(Focus::Mute, Key::Down), Some(Command::MuteHow { stronger: true }));
        for key in [Key::E, Key::S, Key::M, Key::X, Key::Enter, Key::Sidebar] {
            assert_eq!(command(Focus::Mute, key), None, "{key:?} stays out of the open menu");
        }
    }

    #[test]
    fn the_picker_takes_arrows_and_enter() {
        assert_eq!(command(Focus::Picker, Key::Left), Some(Command::EarlierDay));
        assert_eq!(command(Focus::Picker, Key::Right), Some(Command::LaterDay));
        assert_eq!(command(Focus::Picker, Key::Up), Some(Command::EarlierTime));
        assert_eq!(command(Focus::Picker, Key::Down), Some(Command::LaterTime));
        assert_eq!(command(Focus::Picker, Key::Enter), Some(Command::Confirm));
        assert_eq!(command(Focus::Picker, Key::E), None);
    }

    #[test]
    fn the_task_dialog_takes_enter_and_nothing_of_the_list() {
        assert_eq!(command(Focus::Task, Key::Enter), Some(Command::Confirm));
        for key in [Key::Down, Key::E, Key::S, Key::T, Key::C, Key::U, Key::Digit(1), Key::Sidebar] {
            assert_eq!(command(Focus::Task, key), None, "{key:?} stays out of the dialog");
        }
    }

    #[test]
    fn other_menus_block_the_list() {
        for key in
            [Key::Down, Key::Enter, Key::E, Key::U, Key::S, Key::T, Key::R, Key::A, Key::Hash, Key::Slash, Key::V]
        {
            assert_eq!(command(Focus::Elsewhere, key), None, "{key:?} behind a menu or the drawer");
        }
        assert_eq!(command(Focus::Elsewhere, Key::Sidebar), Some(Command::Sidebar), "[ still closes the drawer");
    }

    /// The shortcuts of each system's preset (tasks E10.1, E10.1b): Apple Mail on macOS (⌘ and Ctrl
    /// alike), Thunderbird on Linux, Outlook on Windows (Ctrl); each runs what its single key or
    /// button runs in the window.
    #[test]
    fn every_shortcut_runs_its_command_on_every_system() {
        use Modifiers as M;
        use shortcuts::Command as Shortcut;
        let enter = || Name::Named(Named::Enter);
        let mac = [
            (M::empty(), char("n"), Shortcut::NewMail),
            (M::empty(), char("r"), Shortcut::Reply),
            (M::SHIFT, char("r"), Shortcut::ReplyAll),
            (M::SHIFT, char("f"), Shortcut::Forward),
            (M::SHIFT, char("d"), Shortcut::Send),
            (M::empty(), enter(), Shortcut::Send),
            (M::SHIFT, char("u"), Shortcut::ToggleRead),
            (M::SHIFT, char("l"), Shortcut::Flag),
            (M::SHIFT, char("j"), Shortcut::Junk),
            (M::SHIFT, char("v"), Shortcut::MoveToFolder),
            (M::ALT, char("f"), Shortcut::Search),
            (M::empty(), char("f"), Shortcut::Search),
            (M::SHIFT, char("n"), Shortcut::Fetch),
        ];
        let linux = [
            (M::empty(), char("n"), Shortcut::NewMail),
            (M::empty(), char("m"), Shortcut::NewMail),
            (M::empty(), char("r"), Shortcut::Reply),
            (M::SHIFT, char("r"), Shortcut::ReplyAll),
            (M::empty(), char("l"), Shortcut::Forward),
            (M::empty(), enter(), Shortcut::Send),
            (M::SHIFT, char("a"), Shortcut::Archive),
            (M::empty(), char("u"), Shortcut::ToggleRead),
            (M::SHIFT, char("l"), Shortcut::Flag),
            (M::SHIFT, char("j"), Shortcut::Junk),
            (M::SHIFT, char("v"), Shortcut::MoveToFolder),
            (M::empty(), char("k"), Shortcut::Search),
            (M::SHIFT, char("k"), Shortcut::Search),
            (M::empty(), char("f"), Shortcut::Search),
        ];
        let windows = [
            (M::empty(), char("n"), Shortcut::NewMail),
            (M::empty(), char("r"), Shortcut::Reply),
            (M::SHIFT, char("r"), Shortcut::ReplyAll),
            (M::empty(), char("f"), Shortcut::Forward),
            (M::empty(), enter(), Shortcut::Send),
            (M::SHIFT, char("a"), Shortcut::Archive),
            (M::empty(), char("u"), Shortcut::ToggleRead),
            (M::empty(), char("q"), Shortcut::ToggleRead),
            (M::SHIFT, char("j"), Shortcut::Junk),
            (M::SHIFT, char("v"), Shortcut::MoveToFolder),
            (M::empty(), char("e"), Shortcut::Search),
            (M::empty(), char("m"), Shortcut::Fetch),
        ];
        let physical = Physical::Unidentified(NativeCode::Unidentified);
        let on = |system, key: &Name, modifiers| shortcut_on(system, key, physical, modifiers);
        for (extra, key, shortcut) in mac {
            assert_eq!(on(System::Mac, &key, M::LOGO | extra), Some(shortcut), "⌘: {shortcut:?} {key:?}");
            assert_eq!(on(System::Mac, &key, M::CTRL | extra), Some(shortcut), "Ctrl on a Mac: {shortcut:?} {key:?}");
            assert_eq!(on(System::Mac, &key, extra), None, "a plain key stays a single key: {shortcut:?}");
        }
        for (system, table) in [(System::Linux, &linux[..]), (System::Windows, &windows[..])] {
            for (extra, key, shortcut) in table {
                assert_eq!(on(system, key, M::CTRL | *extra), Some(*shortcut), "Ctrl: {system:?} {shortcut:?} {key:?}");
                assert_eq!(on(system, key, M::LOGO | *extra), None, "the logo key is no Ctrl: {system:?} {key:?}");
                assert_eq!(on(system, key, *extra), None, "a plain key stays a single key: {system:?} {key:?}");
            }
        }
        // The function keys and Insert need no Ctrl: F5 in Thunderbird, F9, F3 and Insert in Outlook.
        let f = |n| Name::Named(n);
        assert_eq!(on(System::Linux, &f(Named::F5), M::empty()), Some(Shortcut::Fetch));
        assert_eq!(on(System::Linux, &f(Named::F5), M::SHIFT), Some(Shortcut::Fetch), "all accounts");
        assert_eq!(on(System::Windows, &f(Named::F9), M::empty()), Some(Shortcut::Fetch));
        assert_eq!(on(System::Windows, &f(Named::F3), M::empty()), Some(Shortcut::Search));
        assert_eq!(on(System::Windows, &f(Named::Insert), M::empty()), Some(Shortcut::Flag));
        assert_eq!(on(System::Linux, &f(Named::Insert), M::empty()), None, "Thunderbird has no Insert");
        assert_eq!(on(System::Mac, &f(Named::F5), M::empty()), None, "Apple Mail has no F5");
        assert_eq!(on(System::Linux, &f(Named::F6), M::empty()), None, "a free function key does nothing");
        // Archive on a Mac is ⌃⌘A, as in Apple Mail.
        assert_eq!(on(System::Mac, &char("a"), M::CTRL | M::LOGO), Some(Shortcut::Archive), "⌃⌘A");
        for (shortcut, wanted) in [
            (Shortcut::NewMail, Some(Command::NewMail)),
            (Shortcut::Reply, Some(Command::Reply)),
            (Shortcut::ReplyAll, Some(Command::ReplyAll)),
            (Shortcut::Forward, Some(Command::Forward)),
            (Shortcut::Send, None),
            (Shortcut::ToggleRead, Some(Command::ToggleRead)),
            (Shortcut::Flag, Some(Command::Flag)),
            (Shortcut::Junk, Some(Command::Junk)),
            (Shortcut::MoveToFolder, Some(Command::MoveToFolder)),
            (Shortcut::Search, Some(Command::Search)),
            (Shortcut::Fetch, Some(Command::Fetch)),
            (Shortcut::Archive, Some(Command::Archive)),
            (Shortcut::Delete, Some(Command::Delete)),
            (Shortcut::DeletePermanently, Some(Command::DeleteForGood)),
        ] {
            assert_eq!(of_shortcut(shortcut), wanted, "{shortcut:?}");
        }
        // Shift turns the letter upper case, and a non-Latin layout types another one: the key counts.
        for system in [System::Linux, System::Windows] {
            assert_eq!(on(system, &char("R"), M::CTRL | M::SHIFT), Some(Shortcut::ReplyAll), "{system:?}");
            let cyrillic = shortcut_on(system, &char("к"), Physical::Code(keyboard::key::Code::KeyR), M::CTRL);
            assert_eq!(cyrillic, Some(Shortcut::Reply), "{system:?}");
        }
    }

    /// Thunderbird's single letters (A archive, J junk, M read, S star) stay the inbox's single keys
    /// on Linux (task E10.1b): `a` answers all, `j` moves down, `m` mutes, `s` snoozes.
    #[test]
    fn the_single_keys_stay_with_thunderbird() {
        let physical = Physical::Unidentified(NativeCode::Unidentified);
        for (letter, key) in [("a", Key::A), ("j", Key::Down), ("m", Key::M), ("s", Key::S)] {
            assert_eq!(key_on(&char(letter), Modifiers::empty(), System::Linux), Some(key), "{letter}");
            assert_eq!(shortcut_on(System::Linux, &char(letter), physical, Modifiers::empty()), None, "{letter}");
        }
    }

    #[test]
    fn ctrl_a_selects_all_and_archives_nothing() {
        let physical = Physical::Unidentified(NativeCode::Unidentified);
        for system in System::ALL {
            for modifiers in [Modifiers::CTRL, Modifiers::LOGO] {
                assert_eq!(shortcut_on(system, &char("a"), physical, modifiers), None, "{system:?} {modifiers:?}");
            }
        }
        assert_eq!(shortcut_on(System::Mac, &char("a"), physical, Modifiers::LOGO | Modifiers::SHIFT), None);
    }

    /// The shortcut is heard also when a widget took the key (the guard captures ⌘ + letter on
    /// macOS); whether it acts while a field has the focus is the app's decision.
    #[test]
    fn shortcuts_are_heard_also_when_a_widget_took_them() {
        let ctrl = if cfg!(target_os = "macos") { Modifiers::LOGO } else { Modifiers::CTRL };
        let reply = shortcut(press(char("r"), ctrl), Status::Captured, iced::window::Id::unique());
        assert!(matches!(reply, Some(Message::Shortcut(shortcuts::Command::Reply))), "{reply:?}");
        let plain = shortcut(press(char("r"), Modifiers::empty()), Status::Ignored, iced::window::Id::unique());
        assert!(plain.is_none(), "a plain r is the single key, not a shortcut");
        // A captured single key stays with its field.
        assert_eq!(heard(char("r"), Modifiers::empty(), Status::Captured), None);
    }

    #[test]
    fn shift_delete_deletes_for_good_and_delete_as_before() {
        let (delete, backspace) = (Name::Named(Named::Delete), Name::Named(Named::Backspace));
        for system in System::ALL {
            assert_eq!(key_on(&delete, Modifiers::empty(), system), Some(Key::Delete), "{system:?}");
            assert_eq!(key_on(&backspace, Modifiers::empty(), system), Some(Key::Delete), "{system:?}");
            assert_eq!(key_on(&delete, Modifiers::SHIFT, system), Some(Key::DeleteForGood), "{system:?}");
            assert_eq!(key_on(&delete, Modifiers::CTRL, system), None, "{system:?}");
        }
        assert_eq!(key_on(&backspace, Modifiers::SHIFT, System::Mac), Some(Key::DeleteForGood), "⇧⌫ on a Mac");
        for other in [System::Windows, System::Linux] {
            assert_eq!(key_on(&backspace, Modifiers::SHIFT, other), None, "only Shift + Delete elsewhere: {other:?}");
        }
        assert_eq!(command(Focus::List, Key::DeleteForGood), Some(Command::DeleteForGood));
        assert_eq!(command(Focus::Overview, Key::DeleteForGood), Some(Command::DeleteForGood));
        for focus in [Focus::Choices, Focus::Task, Focus::Elsewhere, Focus::Question] {
            assert_eq!(command(focus, Key::DeleteForGood), None, "{focus:?}");
        }
    }

    #[test]
    fn the_question_takes_enter_and_nothing_of_the_list() {
        assert_eq!(command(Focus::Question, Key::Enter), Some(Command::Confirm));
        for key in [Key::Down, Key::E, Key::Delete, Key::Hash, Key::R, Key::Digit(1), Key::Sidebar] {
            assert_eq!(command(Focus::Question, key), None, "{key:?} waits for the answer");
        }
    }

    #[test]
    fn digits_are_the_hints_of_menu_entries() {
        assert_eq!((digit(1), digit(4), digit(9)), ("1", "4", "9"));
        assert_eq!((digit(0), digit(10)), ("", ""));
    }
}
