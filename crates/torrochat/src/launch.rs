//! The start parameters of `torrochat` ([`Launch::parse`]): which window to open – the contact list
//! or the chat window of one conversation ([`Window`]) – and the demo switches every app of the suite
//! has.
//!
//! - no window flag: the contact list, class `torro-chat` ([`torro_core::AppId::CHAT`]).
//! - `--conversation=<id>`: the chat window of the conversation `<account>/<token>`, class
//!   `torro-chat-conversation` ([`torro_core::app::CHAT_CONVERSATION_CLASS`]). A second start on the
//!   same conversation brings its window to the front (`torro_gui::instance::claim_named`).
//! - `--demo`: the sample of `torro_chat::demo` instead of the core; nothing is written.
//! - `--open <state>`: start in a state ([`Open`]), for the pictures of the visual check; needs
//!   `--demo`. `chat` is the chat window with Ben of the demo.
//! - `--appearance light|dark|system`: this process only, nothing is stored.
//! - `--design <id>`, `--font <id>`, `--color-scheme <id>`: draw with this design, font or colour
//!   scheme; need `--demo`.

use std::fmt;

use torro_gui::design;
use torro_gui::theme::Appearance;

/// The longest conversation id `--conversation` takes, in bytes.
const ID_MAX: usize = 1000;

/// Which window the process opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Window {
    /// The contact list: people by status, the group chats, the own status.
    #[default]
    Contacts,
    /// The chat window of one conversation (`<account>/<token>`).
    Conversation(String),
}

/// A state the window can start in (`--demo --open <state>`), so every state can be photographed
/// without clicking. The people and conversations named are the demo's (`torro_chat::demo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The contact list on "People" (also the start without `--open`).
    Contacts,
    /// The contact list on "Groups".
    Groups,
    /// The contact list with the menu of the own status open.
    Status,
    /// The contact list with the dialog "Set status message".
    StatusMessage,
    /// The contact list with the dialog "New chat", "be" typed.
    New,
    /// The contact list with "Offline" folded and the search "e" typed.
    Search,
    /// The chat window with Ben: mention, file, missed call, the divider "New".
    Chat,
    /// The chat window with Ben, answering his last message, "@" typed with the suggestions open.
    Reply,
    /// The chat window of the group "Team Alpha": system message, a quote, several authors.
    Group,
    /// The chat window with Ben, the menu of a message open.
    MessageMenu,
}

impl Open {
    /// Every state by the word `--open` takes.
    pub const ALL: [(&'static str, Open); 10] = [
        ("contacts", Open::Contacts),
        ("groups", Open::Groups),
        ("status", Open::Status),
        ("status-message", Open::StatusMessage),
        ("new", Open::New),
        ("search", Open::Search),
        ("chat", Open::Chat),
        ("reply", Open::Reply),
        ("group", Open::Group),
        ("message-menu", Open::MessageMenu),
    ];

    /// The demo conversation a state of the chat window shows; `None` for a state of the contact
    /// list.
    pub fn conversation(self) -> Option<&'static str> {
        match self {
            Open::Chat | Open::Reply | Open::MessageMenu => Some(torro_chat::demo::BEN),
            Open::Group => Some(torro_chat::demo::TEAM),
            Open::Contacts | Open::Groups | Open::Status | Open::StatusMessage | Open::New | Open::Search => None,
        }
    }
}

/// The start parameters (see the module's documentation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
    pub window: Window,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub design: Option<String>,
    pub font: Option<String>,
    pub color_scheme: Option<String>,
}

/// Why the start parameters were refused; `main` prints it and exits with 64, like the other apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// A flag without its value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// A flag that only makes sense on sample data.
    NeedsDemo(&'static str),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// The value of `--flag value` or `--flag=value`; `None` when the flag is not there.
fn flag_value<'a>(args: &'a [String], flag: &'static str) -> Result<Option<&'a str>, LaunchError> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            return words.next().map(|value| Some(value.as_str())).ok_or(LaunchError::MissingValue(flag));
        }
        if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

impl Launch {
    /// `--design`, `--font` and `--color-scheme` for `design::start_for`.
    pub fn forced(&self) -> design::Forced {
        design::Forced { design: self.design.clone(), font: self.font.clone(), color_scheme: self.color_scheme.clone() }
    }

    /// Reads the start parameters from `args` (the words after the program name). Words it does
    /// not know are left alone (`--shots` and its kin belong to `torro_gui::shots`).
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--open")),
            Some(word) => match Open::ALL.iter().find(|(name, _)| *name == word) {
                Some(&(_, open)) => Some(open),
                None => return Err(unknown("--open", word, Open::ALL.iter().map(|(name, _)| *name).collect())),
            },
        };
        let window = match flag_value(args, "--conversation")? {
            Some(id) if is_conversation(id) => Window::Conversation(id.to_owned()),
            Some(id) => return Err(unknown("--conversation", id, vec!["<account>/<token>"])),
            None => match open.and_then(Open::conversation) {
                Some(id) => Window::Conversation(id.to_owned()),
                None => Window::Contacts,
            },
        };
        let appearance = match flag_value(args, "--appearance")? {
            None => None,
            Some("light") => Some(Appearance::Light),
            Some("dark") => Some(Appearance::Dark),
            Some("system") => Some(Appearance::System),
            Some(other) => return Err(unknown("--appearance", other, vec!["light", "dark", "system"])),
        };
        let part = |flag: &'static str, part: design::Part| -> Result<Option<String>, LaunchError> {
            match flag_value(args, flag)? {
                None => Ok(None),
                Some(_) if !demo => Err(LaunchError::NeedsDemo(flag)),
                Some(id) if design::is_choice(part, id) => Ok(Some(id.to_owned())),
                Some(id) => {
                    let valid = design::choices(part);
                    Err(unknown(flag, id, valid.iter().map(String::as_str).collect()))
                }
            }
        };
        let design = part("--design", design::Part::Design)?;
        let font = part("--font", design::Part::Font)?;
        let color_scheme = part("--color-scheme", design::Part::ColorScheme)?;
        Ok(Launch { demo, window, open, appearance, design, font, color_scheme })
    }
}

/// A word that can be a conversation's id: account and token around the last `/`
/// (`torro_chat::split_conversation`), not a flag, within [`ID_MAX`] bytes, without spaces or control
/// characters (a mistyped command must not become a conversation). The core checks the rest.
fn is_conversation(word: &str) -> bool {
    !word.starts_with('-')
        && word.len() <= ID_MAX
        && !word.chars().any(|c| c.is_whitespace() || c.is_control())
        && torro_chat::split_conversation(word).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn window_of(words: &[&str]) -> Result<Window, LaunchError> {
        Launch::parse(&args(words)).map(|launch| launch.window)
    }

    #[test]
    fn the_contact_list_is_the_plain_start() {
        assert_eq!(window_of(&[]), Ok(Window::Contacts));
        assert_eq!(window_of(&["--demo", "--open", "groups"]), Ok(Window::Contacts));
    }

    #[test]
    fn conversation_takes_a_conversation_id() {
        let id = "nextcloud:anna@cloud.example.org/ben4chat";
        assert_eq!(window_of(&[&format!("--conversation={id}")]), Ok(Window::Conversation(id.into())));
        assert_eq!(window_of(&["--conversation", id]), Ok(Window::Conversation(id.into())));
        assert_eq!(window_of(&["--conversation"]), Err(LaunchError::MissingValue("--conversation")));
        let long = format!("a/{}", "b".repeat(ID_MAX));
        for bad in ["", "token", "a/", "/b", "--demo", "a/b c", "a/b\u{7}", long.as_str()] {
            assert!(matches!(window_of(&["--conversation", bad]), Err(LaunchError::UnknownValue { .. })), "{bad:?}");
        }
    }

    #[test]
    fn the_states_of_the_chat_window_open_a_demo_conversation() {
        assert_eq!(window_of(&["--demo", "--open", "chat"]), Ok(Window::Conversation(torro_chat::demo::BEN.into())));
        assert_eq!(window_of(&["--demo", "--open", "group"]), Ok(Window::Conversation(torro_chat::demo::TEAM.into())));
        for (word, open) in Open::ALL {
            let parsed = Launch::parse(&args(&["--demo", "--open", word])).expect("every state parses");
            assert_eq!(parsed.open, Some(open), "{word}");
        }
    }

    #[test]
    fn demo_switches_need_the_demo_and_known_values() {
        assert_eq!(Launch::parse(&args(&["--open", "chat"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert!(matches!(
            Launch::parse(&args(&["--demo", "--open", "ring"])),
            Err(LaunchError::UnknownValue { flag: "--open", .. })
        ));
        let launch = Launch::parse(&args(&["--demo", "--appearance", "dark", "--design", "dense"])).expect("valid");
        assert_eq!((launch.appearance, launch.design.as_deref()), (Some(Appearance::Dark), Some("dense")));
        assert!(Launch::parse(&args(&["--appearance", "dim"])).is_err());
    }
}
