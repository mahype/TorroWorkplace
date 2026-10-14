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
//!    the folder (in a folder `e` archives: the app decides); in a menu of choices the digits pick
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

use iced::keyboard::{self, Key as Name, key::Named};
use torro_gui::shell;

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
    /// The hash sign (Shift + 3 on many keyboards: Shift does not stop it).
    Hash,
    /// Delete or Backspace (the key Mac keyboards call "delete", issue #118). A text field that has
    /// the focus takes it before the list hears it.
    Delete,
    /// `/`.
    Slash,
    /// ⌘F on macOS, Ctrl + F elsewhere: the search field, as `/` (issue #136).
    Find,
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
    /// (issue #123), else "Hide from inbox" (task E3.10): the entry, its source.
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
    /// Put the keyboard into the search field over the list (issue #136).
    Search,
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
    /// Picker: snooze to the picked time; dialog of "Make task" or "Make event": create it.
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

/// ⌘F on macOS, Ctrl + F elsewhere, on any layout (the key in F's place, `to_latin`): the search
/// field, as in every mail app (issue #136). Subscribe with `iced::event::listen_with(find_key)`: it
/// hears the key also when a text field or the window's `widgets::ctrl_guard` took it – the guard
/// does on macOS, so the field does not type an "f".
pub fn find_key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
        return None;
    };
    let find = modifiers.command() && !modifiers.shift() && !modifiers.alt() && key.to_latin(physical_key) == Some('f');
    find.then_some(Message::Key(Key::Find))
}

/// The [`Key`] for `key` held with `modifiers`.
pub fn key_of(key: &Name, modifiers: keyboard::Modifiers) -> Option<Key> {
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
        Name::Character("#") => Key::Hash,
        Name::Named(Named::Delete | Named::Backspace) => Key::Delete,
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
        (Focus::List, Key::Slash | Key::Find) => Some(Command::Search),
        (Focus::List, Key::Tab) => Some(Command::Complete),
        (Focus::Choices | Focus::Mute, Key::Digit(n)) => Some(Command::Choose(usize::from(n))),
        (Focus::Mute, Key::Left) => Some(Command::MuteFor { longer: false }),
        (Focus::Mute, Key::Right) => Some(Command::MuteFor { longer: true }),
        (Focus::Mute, Key::Up) => Some(Command::MuteHow { stronger: false }),
        (Focus::Mute, Key::Down) => Some(Command::MuteHow { stronger: true }),
        (Focus::Picker, Key::Left) => Some(Command::EarlierDay),
        (Focus::Picker, Key::Right) => Some(Command::LaterDay),
        (Focus::Picker, Key::Up) => Some(Command::EarlierTime),
        (Focus::Picker, Key::Down) => Some(Command::LaterTime),
        (Focus::Picker | Focus::Task, Key::Enter) => Some(Command::Confirm),
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
        for key in [Key::Down, Key::Enter, Key::E, Key::U, Key::S, Key::T, Key::R, Key::A, Key::Hash, Key::Slash] {
            assert_eq!(command(Focus::Elsewhere, key), None, "{key:?} behind a menu or the drawer");
        }
        assert_eq!(command(Focus::Elsewhere, Key::Sidebar), Some(Command::Sidebar), "[ still closes the drawer");
    }

    #[test]
    fn digits_are_the_hints_of_menu_entries() {
        assert_eq!((digit(1), digit(4), digit(9)), ("1", "4", "9"));
        assert_eq!((digit(0), digit(10)), ("", ""));
    }
}
