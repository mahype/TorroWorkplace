//! The keyboard of the inbox (task E3.2 in `docs/tasks/eingang.md`): which key does what.
//!
//! Two steps, so the map can be tested without a window:
//!
//! 1. [`listen`] turns a key press into a [`Key`] – only plain keys (no Ctrl, Alt or logo key) that
//!    no widget has taken. A text field that has the focus takes the characters typed into it, so
//!    they never reach the list. (The inbox has no text field yet; one that does not take every
//!    key, like a search field with its arrows, has to be asked for its focus first, as torrotasks
//!    does with `operation::is_focused`.)
//! 2. [`command`] says what a [`Key`] does where the keys go ([`Focus`]): in the list `j`/`k` (and
//!    ↓/↑) move, `Enter` opens, `e` done, `u` undo, `s` snooze, `l` lane, `m` mute; in a menu of
//!    choices the digits pick its entries; in the date picker the arrows change day and time and
//!    `Enter` snoozes. Behind the navigation menu, the app switcher or the drawer no key
//!    reaches the list. Escape is not here: `widgets::escape` closes whatever is in front.

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
    /// ←.
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
    /// `m`.
    M,
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
    /// A menu of choices for an entry is open (snooze, lane); its entries carry digits.
    Choices,
    /// The date and time picker of "Pick…" is open.
    Picker,
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
    /// Open the entry under the cursor: its web link in the browser, else the panel.
    Open,
    /// Mark the entry done, or open it again when it is done.
    Done,
    /// Undo the last change while its hint shows.
    Undo,
    /// Open the snooze menu.
    Snooze,
    /// Move the entry to the other lane (asking whether always for its source).
    Lane,
    /// Mute the entry's source, or unmute it; at once, with an undo.
    Mute,
    /// Pick the n-th entry (from 1) of the open menu of choices.
    Choose(usize),
    /// Picker: one day earlier.
    EarlierDay,
    /// Picker: one day later.
    LaterDay,
    /// Picker: the previous time of the day.
    EarlierTime,
    /// Picker: the next time of the day.
    LaterTime,
    /// Picker: snooze to the picked time.
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
        Name::Character("e") => Key::E,
        Name::Character("u") => Key::U,
        Name::Character("s") => Key::S,
        Name::Character("l") => Key::L,
        Name::Character("m") => Key::M,
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
        (Focus::List | Focus::Elsewhere, Key::Sidebar) => Some(Command::Sidebar),
        (Focus::List, Key::Down) => Some(Command::Next),
        (Focus::List, Key::Up) => Some(Command::Previous),
        (Focus::List, Key::Enter) => Some(Command::Open),
        (Focus::List, Key::E) => Some(Command::Done),
        (Focus::List, Key::U) => Some(Command::Undo),
        (Focus::List, Key::S) => Some(Command::Snooze),
        (Focus::List, Key::L) => Some(Command::Lane),
        (Focus::List, Key::M) => Some(Command::Mute),
        (Focus::Choices, Key::Digit(n)) => Some(Command::Choose(usize::from(n))),
        (Focus::Picker, Key::Left) => Some(Command::EarlierDay),
        (Focus::Picker, Key::Right) => Some(Command::LaterDay),
        (Focus::Picker, Key::Up) => Some(Command::EarlierTime),
        (Focus::Picker, Key::Down) => Some(Command::LaterTime),
        (Focus::Picker, Key::Enter) => Some(Command::Confirm),
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
            (char("m"), Command::Mute),
            (char("["), Command::Sidebar),
        ] {
            let key = heard(key, none, Status::Ignored).expect("a key of the inbox");
            assert_eq!(command(Focus::List, key), Some(wanted), "{key:?}");
        }
    }

    #[test]
    fn keys_a_widget_took_or_with_modifiers_do_nothing() {
        // A text field that has the focus captures what is typed into it.
        assert_eq!(heard(char("e"), Modifiers::empty(), Status::Captured), None);
        for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            assert_eq!(heard(char("e"), modifiers, Status::Ignored), None, "{modifiers:?}");
        }
        assert_eq!(heard(char("x"), Modifiers::empty(), Status::Ignored), None, "not a key of the inbox");
        assert_eq!(heard(char("0"), Modifiers::empty(), Status::Ignored), None, "choices count from 1");
        assert_eq!(heard(char("E"), Modifiers::SHIFT, Status::Ignored), None, "Shift + e is not e");
    }

    #[test]
    fn menus_take_digits_and_nothing_else() {
        assert_eq!(heard(char("2"), Modifiers::empty(), Status::Ignored), Some(Key::Digit(2)));
        assert_eq!(command(Focus::Choices, Key::Digit(2)), Some(Command::Choose(2)));
        assert_eq!(command(Focus::List, Key::Digit(2)), None, "digits only pick menu entries");
        for key in [Key::Down, Key::E, Key::S, Key::M, Key::Enter, Key::Sidebar] {
            assert_eq!(command(Focus::Choices, key), None, "{key:?} stays out of the open menu");
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
    fn other_menus_block_the_list() {
        for key in [Key::Down, Key::Up, Key::Enter, Key::E, Key::U, Key::S, Key::L, Key::M, Key::Digit(1)] {
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
