//! The start parameters of the control centre and the sample data of `--demo`.
//!
//! `torroworkplace ui --demo --open wizard --appearance light` shows a state without a service
//! and without clicking, so `torro-design-review` can photograph every state from a script. In
//! demo mode the window never talks to a socket and never stores anything: the pages get
//! [`demo_data`], and every request a page wants to send is dropped (see `Ui::perform`).

use std::fmt;

use chrono::{Duration, Local, Utc};
use torro_core::AppId;
use torro_core::settings::Settings;
use torro_ipc::{AccountInfo, AppInfo, Status};
use torro_pim::{AccountState, Collection, CollectionKind};
use torro_setups::TemplateSummary;

use super::{Appearance, Data, Section};

/// A state the control centre can start in (`--demo --open <state>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The add-account dialog.
    Wizard,
    /// The detail page of the first demo account.
    AccountDetail,
    /// The section menu ("Accounts ▾"); only the medium and narrow header have it.
    Menu,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The "Remove this account?" confirmation.
    Remove,
    /// The "Delete this template?" confirmation.
    DeleteTemplate,
    /// The first template's row in rename mode.
    RenameTemplate,
}

impl Open {
    pub const ALL: [(&'static str, Open); 8] = [
        ("wizard", Open::Wizard),
        ("account-detail", Open::AccountDetail),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("remove", Open::Remove),
        ("delete-template", Open::DeleteTemplate),
        ("rename-template", Open::RenameTemplate),
    ];
}

/// The start parameters of `torroworkplace ui`:
///
/// - `--section overview|accounts|workspaces|apps|general`: the page to start on. The states of
///   `--open` that belong to a page choose that page themselves.
/// - `--demo`: sample accounts, templates, settings and apps in memory instead of the service.
///   Nothing is sent and nothing is stored.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub section: Section,
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
}

impl Default for Launch {
    fn default() -> Self {
        Launch { section: Section::Overview, demo: false, open: None, appearance: None }
    }
}

/// Why the start parameters were refused; `main` prints it and exits with 64, as `ctl` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// A flag without its value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the list of valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// `--open` without `--demo`: the states need sample data.
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
    /// Reads the start parameters from `args` (the words after `ui`).
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let section = match flag_value(args, "--section")? {
            None => Section::Overview,
            Some(word) => Section::from_arg(word)
                .ok_or_else(|| unknown("--section", word, Section::ALL.iter().map(|s| s.arg()).collect()))?,
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
        Ok(Launch { section, demo, open, appearance })
    }
}

/// The sample content of `--demo`: a healthy Nextcloud account with a shared read-only calendar,
/// a second one that cannot reach its server (so the problem rows show), three templates, the
/// suite's apps, and settings with one list hidden. Times are relative to now.
pub fn demo_data() -> Data {
    let now = Utc::now();
    let (mut sample, _, _) = torro_pim::demo::sample(Local::now().date_naive());
    // A calendar shared by someone else: shown, but read-only.
    if let Some(shared) = sample.iter_mut().find(|collection| collection.name == "Family") {
        shared.writable = false;
    }
    let hidden_collections: Vec<String> = sample
        .iter()
        .filter(|collection| collection.name == "Groceries")
        .map(|collection| collection.id.clone())
        .collect();
    let team = |id: &str, name: &str, kind, color: &str| Collection {
        id: id.into(),
        name: name.into(),
        kind,
        color: Some(color.into()),
        writable: true,
    };
    let accounts = vec![
        AccountInfo {
            id: "nextcloud:anna@cloud.example.org".into(),
            kind: "nextcloud".into(),
            label: "anna@cloud.example.org".into(),
            state: AccountState::Connected {
                server: "https://cloud.example.org".into(),
                login_name: "anna".into(),
                last_sync: Some(now - Duration::minutes(4)),
                problem: None,
            },
            collections: sample,
        },
        AccountInfo {
            id: "nextcloud:anna@work.example.net".into(),
            kind: "nextcloud".into(),
            label: "anna@work.example.net".into(),
            state: AccountState::Connected {
                server: "https://work.example.net".into(),
                login_name: "anna".into(),
                last_sync: Some(now - Duration::days(2)),
                problem: Some("the server did not answer".into()),
            },
            collections: vec![
                team("demo:/work/calendars/team/", "Team", CollectionKind::Events, "#5B8CFF"),
                team("demo:/work/calendars/ops/", "Ops", CollectionKind::Todos, "#E8A33D"),
            ],
        },
    ];
    let template = |id: &str, name: &str, windows, age: Duration| TemplateSummary {
        id: id.into(),
        name: name.into(),
        windows,
        updated: now - age,
    };
    let templates = vec![
        template("development", "Development", 6, Duration::days(2)),
        template("writing", "Writing", 3, Duration::days(9)),
        template("review-call", "Review call", 4, Duration::hours(3)),
    ];
    let apps = AppId::ALL
        .iter()
        .filter(|app| **app != AppId::WORKPLACE)
        .map(|app| AppInfo {
            binary: app.binary.into(),
            name: app.display_name.into(),
            window_class: app.window_class.into(),
            installed: true,
        })
        .collect();
    Data {
        status: Some(Status {
            version: env!("CARGO_PKG_VERSION").into(),
            backend: "hyprland".into(),
            workspace: Some("3".into()),
            templates: templates.len(),
            busy: false,
        }),
        accounts,
        templates,
        settings: Settings { start_at_login: true, hidden_collections, ..Settings::default() },
        apps: Some(apps),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn every_state_of_open_is_parsed_in_both_spellings() {
        for (name, open) in Open::ALL {
            let expected = Launch { demo: true, open: Some(open), ..Launch::default() };
            assert_eq!(Launch::parse(&args(&["--demo", "--open", name])), Ok(expected.clone()), "{name}");
            assert_eq!(Launch::parse(&args(&["--demo", &format!("--open={name}")])), Ok(expected), "{name}");
        }
        assert_eq!(Launch::parse(&[]), Ok(Launch::default()));
    }

    #[test]
    fn section_and_appearance_are_parsed() {
        for section in Section::ALL {
            let launch = Launch::parse(&args(&["--section", section.arg()])).expect("a known section");
            assert_eq!(launch.section, section);
        }
        let launch = Launch::parse(&args(&["--demo", "--section=apps", "--appearance", "light"])).expect("valid");
        assert_eq!((launch.section, launch.appearance), (Section::Apps, Some(Appearance::Light)));
        for (word, appearance) in
            [("light", Appearance::Light), ("dark", Appearance::Dark), ("system", Appearance::System)]
        {
            let launch = Launch::parse(&args(&["--appearance", word])).expect("works without --demo");
            assert_eq!(launch.appearance, Some(appearance));
        }
    }

    #[test]
    fn bad_parameters_name_the_valid_values() {
        let err = Launch::parse(&args(&["--demo", "--open", "popup"])).expect_err("unknown state");
        let text = err.to_string();
        assert!(text.contains("\"popup\"") && text.contains("--open"), "{text}");
        for (name, _) in Open::ALL {
            assert!(text.contains(name), "{name} is listed in: {text}");
        }
        let err = Launch::parse(&args(&["--section", "mail"])).expect_err("unknown section");
        assert!(err.to_string().ends_with("valid values: overview, accounts, workspaces, apps, general"), "{err}");
        let err = Launch::parse(&args(&["--appearance", "sepia"])).expect_err("unknown appearance");
        assert!(err.to_string().ends_with("valid values: light, dark, system"), "{err}");
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        assert_eq!(Launch::parse(&args(&["--open", "wizard"])), Err(LaunchError::NeedsDemo("--open")));
    }

    #[test]
    fn the_demo_has_something_to_show_on_every_page() {
        let data = demo_data();
        let status = data.status.as_ref().expect("a status");
        assert_eq!(status.templates, data.templates.len());
        assert!(data.error.is_none());

        let listed: Vec<&AccountInfo> = super::super::accounts::listed(&data.accounts).collect();
        assert_eq!(listed.len(), data.accounts.len(), "no sign-in in progress");
        assert!(
            matches!(listed[0].state, AccountState::Connected { problem: None, .. }),
            "the detail page opens the first account, and it is a healthy one"
        );
        assert!(
            listed.iter().any(|account| matches!(account.state, AccountState::Connected { problem: Some(_), .. })),
            "one account shows a problem"
        );
        assert!(listed[0].collections.iter().any(|collection| !collection.writable), "a read-only calendar");
        assert!(listed[0].collections.iter().any(|collection| collection.kind.has_todos()), "a task list");

        let ids: Vec<&str> =
            listed.iter().flat_map(|account| &account.collections).map(|collection| collection.id.as_str()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<HashSet<_>>().len(), "collection ids are unique");
        assert!(data.settings.hidden_collections.iter().all(|id| ids.contains(&id.as_str())), "a hidden list exists");
        assert!(!data.settings.hidden_collections.is_empty(), "one list is hidden");

        assert!(data.templates.len() >= 2);
        let apps = data.apps.as_ref().expect("apps");
        assert!(apps.iter().all(|app| app.binary != AppId::WORKPLACE.binary), "the core is not one of its apps");
        assert!(apps.len() >= 2);
    }
}
