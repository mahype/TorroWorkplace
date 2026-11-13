//! The start parameters of `torrocontacts` ([`Launch::parse`]): the demo switches every app of the
//! suite has.
//!
//! - `--demo`: the sample of `torro_contacts::demo` instead of the core; changes live until the window
//!   closes.
//! - `--open <state>`: start in a state ([`Open`]), for the pictures of the visual check; needs
//!   `--demo` (without it the start ends with exit code 64).
//! - `--appearance light|dark|system`: this process only, nothing is stored.
//! - `--design <id>`, `--font <id>`, `--color-scheme <id>`: draw with this design, font or colour
//!   scheme; need `--demo`.

use std::fmt;

use torro_gui::design;
use torro_gui::theme::Appearance;

/// A state the window can start in (`--demo --open <state>`), so every state can be photographed
/// without clicking. The contacts named are the demo's (`torro_contacts::demo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// "All contacts" grouped by group (also the start without `--open`).
    List,
    /// Grouped by town.
    ByCity,
    /// Grouped by address book.
    ByBook,
    /// The menu "Group by" open.
    GroupMenu,
    /// "an" typed into the search.
    Search,
    /// Anna Berg open in the side panel.
    Contact,
    /// Anna Berg being edited.
    Edit,
    /// A new contact being written.
    New,
    /// Pierre Martin open, changed here and on the server.
    Conflict,
    /// Lena Fischer open: her address book is read-only.
    Readonly,
    /// Lukas Hoffmann open: changed on this device, not yet on the server.
    Pending,
    /// Anna Berg open, asked whether to delete her.
    Delete,
    /// Anna Berg open, the menu "Move to" open.
    Move,
    /// The view "Birthdays".
    Birthdays,
    /// The search of the mini tile with "an" typed.
    MiniSearch,
    /// No contacts in the address books (set after loading).
    Empty,
    /// No account with an address book (set after loading).
    NoAccount,
    /// The first load failed (set after loading).
    Error,
}

impl Open {
    /// Every state by the word `--open` takes.
    pub const ALL: [(&'static str, Open); 18] = [
        ("list", Open::List),
        ("by-city", Open::ByCity),
        ("by-book", Open::ByBook),
        ("group-menu", Open::GroupMenu),
        ("search", Open::Search),
        ("contact", Open::Contact),
        ("edit", Open::Edit),
        ("new", Open::New),
        ("conflict", Open::Conflict),
        ("readonly", Open::Readonly),
        ("pending", Open::Pending),
        ("delete", Open::Delete),
        ("move", Open::Move),
        ("birthdays", Open::Birthdays),
        ("mini-search", Open::MiniSearch),
        ("empty", Open::Empty),
        ("no-account", Open::NoAccount),
        ("error", Open::Error),
    ];

    /// The uid of the demo contact this state opens in the side panel, if it opens one.
    pub fn contact(self) -> Option<&'static str> {
        match self {
            Open::Contact | Open::Edit | Open::Delete | Open::Move => Some("demo-01"),
            Open::Conflict => Some("demo-04"),
            Open::Readonly => Some("demo-05"),
            Open::Pending => Some("demo-08"),
            _ => None,
        }
    }
}

/// The start parameters (see the module's documentation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
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
        Ok(Launch { demo, open, appearance, design, font, color_scheme })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_string()).collect()
    }

    #[test]
    fn open_needs_the_demo_and_a_known_state() {
        assert_eq!(Launch::parse(&args(&["--open", "list"])), Err(LaunchError::NeedsDemo("--open")));
        assert!(matches!(
            Launch::parse(&args(&["--demo", "--open", "map"])),
            Err(LaunchError::UnknownValue { flag: "--open", .. })
        ));
        for (word, open) in Open::ALL {
            let launch = Launch::parse(&args(&["--demo", "--open", word])).expect("a known state");
            assert_eq!(launch.open, Some(open), "{word}");
            let joined = Launch::parse(&args(&["--demo", &format!("--open={word}")])).expect("with =");
            assert_eq!(joined.open, Some(open));
        }
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
    }

    #[test]
    fn the_other_switches() {
        let launch = Launch::parse(&args(&["--demo", "--appearance", "dark", "--design", "dense"])).expect("valid");
        assert_eq!(launch.appearance, Some(Appearance::Dark));
        assert_eq!(launch.design.as_deref(), Some("dense"));
        assert_eq!(Launch::parse(&args(&["--design", "dense"])), Err(LaunchError::NeedsDemo("--design")));
        assert!(Launch::parse(&args(&["--appearance", "blue"])).is_err());
        assert_eq!(Launch::parse(&args(&[])).expect("plain"), Launch::default());
    }
}
