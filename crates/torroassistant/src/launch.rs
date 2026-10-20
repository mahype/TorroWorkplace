//! The start parameters of `torroassistant` ([`Launch::parse`]): which window to open – the main
//! window or a single chat window ([`Window`]) – and the demo switches every app of the suite has.
//!
//! - no window flag: the main window, class `torro-assistant` ([`torro_core::AppId::ASSISTANT`]).
//! - `--chat <id>` / `--chat new`: one conversation in a window of its own, class
//!   `torro-assistant-chat` ([`crate::app::CHAT`]); `assistant.open_window` starts it this way, and a
//!   workspace template stores the command line, so the same conversation comes back.
//! - `--bot <id>`: the chat with a bot in a window of its own (task B.3): its newest conversation
//!   or a new one, class `torro-assistant-chat`, titled "{bot} · {title}". `assistant.open_window`
//!   starts it this way, and a workspace template brings the same bot back with its argument.
//! - `--demo`: sample conversations in memory instead of the core ([`crate::demo`]).
//! - `--settings <section>`: open the window with its settings in the content area, at this
//!   section (`general`, `access`, `profile`, `bots`, `voice`, `history`; tasks S.18, S.34). In a
//!   bot's window `--bot <id> --settings access` opens Access at that bot. A start parameter, so
//!   only a person starting the program opens the settings: no text of an answer can, and the
//!   links of an answer stay on `http(s)` ([`crate::app`]).
//! - `--open <state>`: start in a state ([`Open`]), for screenshots; needs `--demo`.
//! - `--appearance light|dark|system`: this process only, nothing is stored.
//! - `--design <id>`: draw with this design pack; needs `--demo`.
//! - `--size <W>x<H>`: open the window at this size in logical pixels; needs `--demo`. For the
//!   screenshots of every size class where no tiling manager resizes the window (macOS).

use std::fmt;

use torro_gui::design;
use torro_gui::theme::Appearance;
use torro_ipc::assistant::MAX_ID_BYTES;
use torro_settings_ui::assistant::SECTIONS;

/// Which window the process opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Window {
    /// The main window: the conversations in the sidebar, the chat beside them.
    #[default]
    Main,
    /// A single chat window without sidebar (`--chat`): the conversation with this id, or a new
    /// one (`None`, `--chat new`).
    Chat(Option<String>),
    /// A single chat window with the bot of this id (`--bot`): its newest conversation once the
    /// conversations are listed, else a new one.
    Bot(String),
}

/// A state the window can start in (`--demo --open <state>`), so every state can be photographed
/// without clicking. The conversations named are the demo's ([`crate::demo`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// A new chat: the welcome above the composer (also the start without `--open`).
    New,
    /// "Plan for tomorrow": messages, tool lines (the first unfolded) and an answer in Markdown.
    Tools,
    /// "Block the afternoon": a tool waits for approval; the card shows Allow and Deny.
    Approval,
    /// "Draft a reply to Anna": an answer the model is still writing, with Stop.
    Streaming,
    /// "Tasks from Anna's mails": a tool the assistant may not use, with the hint to change it.
    Forbidden,
    /// The model picker of the composer.
    Picker,
    /// The effort menu of the composer (issue #131).
    Effort,
    /// The permission mode menu under the composer (issue #132).
    Mode,
    /// The "…" menu of the open conversation (Rename, Pin, Open in new window, Delete).
    Menu,
    /// The dialog that renames the open conversation.
    Rename,
    /// The question before deleting the open conversation.
    Delete,
    /// The first start: no client "Assistant" yet, so the window asks to choose its rights.
    Setup,
    /// No AI account: nothing could answer.
    NoEngines,
    /// No conversations at all.
    Empty,
    /// The conversations could not be loaded.
    Error,
    /// Loading, kept for good.
    Loading,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher (in the drawer where there is no sidebar).
    Switcher,
    /// The chat with the demo's bot "Travel planner" in the main window: its conversation.
    Bot,
    /// A new chat with the bot "Travel planner": its welcome above the composer.
    BotWelcome,
    /// The menu of the bot "Travel planner" in the sidebar (New chat, Edit, Access, Delete).
    BotMenu,
    /// The editor of the bot "Travel planner": profile, personality, instructions, skills.
    BotEditor,
    /// The editor of a new bot, after a first "Save" without a name: the problem shows.
    BotNew,
    /// The question before deleting the bot "Travel planner".
    BotDelete,
    /// "Weekend in Porto": the assistant asked a bot; its tool line unfolded with "Open chat".
    BotAsked,
    /// A conversation of a bot that was deleted: it can be read, not continued.
    BotGone,
    /// "Train or plane to Vienna?": the assistant proposes a bot; the card "Ask Travel planner?".
    BotProposal,
}

impl Open {
    /// Every state by the word `--open` takes.
    pub const ALL: [(&'static str, Open); 27] = [
        ("new", Open::New),
        ("tools", Open::Tools),
        ("approval", Open::Approval),
        ("streaming", Open::Streaming),
        ("forbidden", Open::Forbidden),
        ("picker", Open::Picker),
        ("effort", Open::Effort),
        ("mode", Open::Mode),
        ("menu", Open::Menu),
        ("rename", Open::Rename),
        ("delete", Open::Delete),
        ("setup", Open::Setup),
        ("no-engines", Open::NoEngines),
        ("empty", Open::Empty),
        ("error", Open::Error),
        ("loading", Open::Loading),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("bot", Open::Bot),
        ("bot-welcome", Open::BotWelcome),
        ("bot-menu", Open::BotMenu),
        ("bot-editor", Open::BotEditor),
        ("bot-new", Open::BotNew),
        ("bot-delete", Open::BotDelete),
        ("bot-asked", Open::BotAsked),
        ("bot-gone", Open::BotGone),
        ("bot-proposal", Open::BotProposal),
    ];
}

/// The start parameters (see the module's documentation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
    pub window: Window,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    /// `--settings <section>`: the window opens with its settings in the content area, at the
    /// section with this [`torro_settings_ui::Section::id`].
    pub settings: Option<String>,
    pub design: Option<String>,
    /// `--size`: the window's size at start, width and height in logical pixels.
    pub size: Option<(u16, u16)>,
}

/// Why the start parameters were refused; `main` prints it and exits with 64, like the other apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// A flag without its value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// `--open` or `--design` without `--demo`: they only make sense on sample data.
    NeedsDemo(&'static str),
    /// Two flags that each choose the window.
    Conflict(&'static str, &'static str),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
            LaunchError::Conflict(one, other) => write!(f, "{one} and {other} cannot be used together"),
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
    /// Reads the start parameters from `args` (the words after the program name). Words it does
    /// not know are left alone.
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let window = window(args)?;
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
        let settings = match flag_value(args, "--settings")? {
            None => None,
            Some(word) if SECTIONS.iter().any(|section| section.id == word) => Some(word.to_owned()),
            Some(word) => {
                return Err(unknown("--settings", word, SECTIONS.iter().map(|section| section.id).collect()));
            }
        };
        let design = match flag_value(args, "--design")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--design")),
            Some(id) if design::is_choice(id) => Some(id.to_owned()),
            Some(id) => {
                let valid = design::choices();
                return Err(unknown("--design", id, valid.iter().map(String::as_str).collect()));
            }
        };
        let size = match flag_value(args, "--size")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--size")),
            Some(word) => Some(size_of(word).ok_or_else(|| unknown("--size", word, vec!["<width>x<height>"]))?),
        };
        Ok(Launch { demo, window, open, appearance, settings, design, size })
    }
}

/// `800x600` as width and height; at least the suite's smallest window (200 × 120).
fn size_of(word: &str) -> Option<(u16, u16)> {
    let (width, height) = word.split_once('x')?;
    let (width, height) = (width.parse::<u16>().ok()?, height.parse::<u16>().ok()?);
    (width >= 200 && height >= 120).then_some((width, height))
}

/// The window `--chat` or `--bot` asks for; the main window without either.
fn window(args: &[String]) -> Result<Window, LaunchError> {
    let chat = flag_value(args, "--chat")?;
    let bot = flag_value(args, "--bot")?;
    match (chat, bot) {
        (Some(_), Some(_)) => Err(LaunchError::Conflict("--chat", "--bot")),
        (None, Some(id)) if is_id(id) => Ok(Window::Bot(id.to_owned())),
        (None, Some(id)) => {
            Err(LaunchError::UnknownValue { flag: "--bot", value: id.to_owned(), valid: "a bot's id".to_owned() })
        }
        (None, None) => Ok(Window::Main),
        (Some("new"), None) => Ok(Window::Chat(None)),
        (Some(id), None) if is_id(id) => Ok(Window::Chat(Some(id.to_owned()))),
        (Some(id), None) => Err(LaunchError::UnknownValue {
            flag: "--chat",
            value: id.to_owned(),
            valid: "new, or a conversation's id".to_owned(),
        }),
    }
}

/// A word that can be a conversation's id: not empty, not a flag, within the core's limit and
/// without spaces or control characters (a mistyped command must not become a conversation).
fn is_id(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && word.len() <= MAX_ID_BYTES
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
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
    fn chat_takes_an_id_or_new() {
        assert_eq!(window_of(&[]), Ok(Window::Main));
        assert_eq!(window_of(&["--chat", "new"]), Ok(Window::Chat(None)));
        assert_eq!(window_of(&["--chat=new"]), Ok(Window::Chat(None)));
        assert_eq!(window_of(&["--chat", "9c1f0a7e3b2d4c58"]), Ok(Window::Chat(Some("9c1f0a7e3b2d4c58".into()))));
        assert_eq!(window_of(&["--chat=9c1f0a7e3b2d4c58"]), Ok(Window::Chat(Some("9c1f0a7e3b2d4c58".into()))));
        assert_eq!(window_of(&["--demo", "--chat", "new"]), Ok(Window::Chat(None)), "demo or not");
    }

    #[test]
    fn chat_refuses_what_is_no_id() {
        assert_eq!(window_of(&["--chat"]), Err(LaunchError::MissingValue("--chat")));
        for bad in ["--demo", "", "two words", "a\u{7}"] {
            assert!(matches!(window_of(&["--chat", bad]), Err(LaunchError::UnknownValue { .. })), "{bad:?}");
        }
        let long = "a".repeat(MAX_ID_BYTES + 1);
        assert!(matches!(window_of(&["--chat", &long]), Err(LaunchError::UnknownValue { .. })));
    }

    /// Task B.3: `--bot <id>` opens the bot's chat window; a word that is no id is refused.
    #[test]
    fn bot_takes_an_id() {
        assert_eq!(window_of(&["--bot", "1a2b3c4d"]), Ok(Window::Bot("1a2b3c4d".into())));
        assert_eq!(window_of(&["--bot=1a2b3c4d", "--demo"]), Ok(Window::Bot("1a2b3c4d".into())));
        assert_eq!(window_of(&["--bot"]), Err(LaunchError::MissingValue("--bot")));
        for bad in ["--demo", "", "two words", "a\u{7}"] {
            assert!(
                matches!(window_of(&["--bot", bad]), Err(LaunchError::UnknownValue { flag: "--bot", .. })),
                "{bad:?}"
            );
        }
        assert_eq!(window_of(&["--chat", "new", "--bot", "travel"]), Err(LaunchError::Conflict("--chat", "--bot")));
    }

    /// Task S.18: `--settings <section>` opens the settings at a section of the assistant, with
    /// and without `--demo`; a word no section has is refused with the words that work.
    #[test]
    fn settings_takes_a_section_of_the_assistant() {
        for section in SECTIONS {
            let launch = Launch::parse(&args(&["--settings", section.id])).expect("every section");
            assert_eq!(launch.settings.as_deref(), Some(section.id));
        }
        let demo = Launch::parse(&args(&["--demo", "--settings=voice"])).expect("valid");
        assert_eq!(demo.settings.as_deref(), Some("voice"));
        // Task S.34: Access, also in a bot's window, where it opens at the bot.
        let bot = Launch::parse(&args(&["--demo", "--settings", "access", "--bot", "b0000001"])).expect("valid");
        assert_eq!((bot.settings.as_deref(), bot.window), (Some("access"), Window::Bot("b0000001".into())));
        assert_eq!(Launch::parse(&args(&[])).expect("valid").settings, None);
        assert_eq!(Launch::parse(&args(&["--settings"])), Err(LaunchError::MissingValue("--settings")));
        for bad in ["appearance", "rights", "", "Voice"] {
            let refused = Launch::parse(&args(&["--settings", bad]));
            assert!(matches!(refused, Err(LaunchError::UnknownValue { flag: "--settings", .. })), "{bad:?}");
        }
    }

    #[test]
    fn demo_switches_need_the_demo_and_known_values() {
        assert_eq!(Launch::parse(&args(&["--open", "tools"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert_eq!(Launch::parse(&args(&["--size", "800x600"])), Err(LaunchError::NeedsDemo("--size")));
        let sized = Launch::parse(&args(&["--demo", "--size", "800x600"])).expect("a size");
        assert_eq!(sized.size, Some((800, 600)));
        for bad in ["800", "80x60", "axb", "800x600x2"] {
            assert!(Launch::parse(&args(&["--demo", "--size", bad])).is_err(), "{bad}");
        }
        assert!(matches!(
            Launch::parse(&args(&["--demo", "--open", "nothing"])),
            Err(LaunchError::UnknownValue { flag: "--open", .. })
        ));
        let launch = Launch::parse(&args(&["--demo", "--open", "approval", "--appearance", "dark"])).expect("valid");
        assert_eq!((launch.open, launch.appearance), (Some(Open::Approval), Some(Appearance::Dark)));
        for (word, open) in Open::ALL {
            let parsed = Launch::parse(&args(&["--demo", "--open", word])).expect("every state parses");
            assert_eq!(parsed.open, Some(open), "{word}");
        }
    }
}
