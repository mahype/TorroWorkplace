//! The tray menu as plain data, the same on every system. [`build`] turns a [`Snapshot`] of the
//! core into [`Item`]s; the front ends only draw them: `linux.rs` with ksni, `desktop.rs` with
//! tray-icon on macOS and Windows. A click runs the entry's [`Action`] on the [`Daemon`], so
//! every entry does the same thing everywhere.

use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_pim::AccountState;
use torro_setups::{LoadMode, TemplateSummary};

use crate::core::Snapshot;
use crate::daemon::Daemon;

/// What a click on an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Sends the request to the core in the background ([`Daemon::run`]); `Request::Quit` ends
    /// the service.
    Run(Box<Request>),
    /// Opens or focuses one of the suite's apps ([`Daemon::open_app`]).
    OpenApp(AppId),
}

impl Action {
    /// Carries the action out. Returns at once; the work runs on the daemon's runtime.
    pub fn perform(&self, daemon: &Daemon) {
        match self {
            Action::Run(request) => daemon.run((**request).clone()),
            Action::OpenApp(app) => daemon.open_app(*app),
        }
    }
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A clickable line. Without an action it is greyed out: an information line, or a command
    /// that does not fit the moment ("Load here" on a workspace with windows).
    Entry { label: String, action: Option<Action> },
    /// A line that opens further items.
    Submenu { label: String, items: Vec<Item> },
    /// A thin line between groups.
    Separator,
}

/// The menu from docs/core.md for one snapshot.
///
/// Labels stay short: Omarchy's bar draws tray menus about 232 px wide and cuts longer
/// text off with an ellipsis.
pub fn build(snapshot: &Snapshot) -> Vec<Item> {
    let mut menu = Vec::new();
    if let Some(name) = &snapshot.unsupported {
        menu.push(info(trf("{name} not supported yet", &[("name", name)])));
        menu.push(Item::Separator);
    } else {
        let load = per_template(snapshot, |t| {
            let id = &t.id;
            vec![
                entry_if(
                    tr("Load here"),
                    snapshot.workspace_windows == 0,
                    Request::LoadTemplate { id: id.clone(), mode: LoadMode::Empty },
                ),
                entry(tr("Add"), Request::LoadTemplate { id: id.clone(), mode: LoadMode::Add }),
                entry(tr("Replace"), Request::LoadTemplate { id: id.clone(), mode: LoadMode::Replace }),
            ]
        });
        menu.push(submenu(tr("Load template"), load));
        menu.push(entry(tr("Save workspace"), Request::SaveTemplate { name: None }));
        let update = per_template_entry(snapshot, |id| Request::UpdateTemplate { id });
        menu.push(submenu(tr("Update"), update));
        let delete = per_template_entry(snapshot, |id| Request::DeleteTemplate { id });
        menu.push(submenu(tr("Delete"), delete));
        menu.push(Item::Separator);
    }
    menu.push(open(tr("Calendar"), AppId::CALENDAR));
    menu.push(open(tr("Tasks"), AppId::TASKS));
    menu.push(Item::Separator);
    menu.push(info(info_line(snapshot)));
    if let Some(line) = account_line(snapshot) {
        menu.push(info(line));
    }
    menu.push(Item::Separator);
    menu.push(entry(tr("Settings…"), Request::OpenApp { app: AppId::WORKPLACE.binary.to_string() }));
    menu.push(entry(tr("Quit"), Request::Quit));
    menu
}

/// `Workspace 3 · hyprland`, or what is going on instead.
fn info_line(snapshot: &Snapshot) -> String {
    if snapshot.busy {
        return tr("Busy …").into();
    }
    if let Some(problem) = &snapshot.problem {
        return problem.clone();
    }
    match &snapshot.workspace {
        Some(workspace) => {
            trf("Workspace {workspace} · {backend}", &[("workspace", workspace), ("backend", &snapshot.backend)])
        }
        None => snapshot.backend.clone(),
    }
}

/// The Nextcloud status: who is connected, a sign-in in progress, or a sync problem.
fn account_line(snapshot: &Snapshot) -> Option<String> {
    Some(match snapshot.account.as_ref()? {
        AccountState::NotConnected => tr("Not connected to Nextcloud").to_string(),
        AccountState::SigningIn { .. } => tr("Signing in to Nextcloud …").to_string(),
        AccountState::Connected { problem: Some(problem), .. } => trf("Nextcloud: {problem}", &[("problem", problem)]),
        AccountState::Connected { login_name, server, .. } => {
            let host = server.split("://").nth(1).unwrap_or(server);
            trf("Nextcloud: {account}", &[("account", &format!("{login_name}@{host}"))])
        }
    })
}

/// An entry that opens or focuses one of the suite's apps.
fn open(label: &str, app: AppId) -> Item {
    Item::Entry { label: label.into(), action: Some(Action::OpenApp(app)) }
}

/// One submenu per template, or a disabled hint when there are none.
fn per_template(snapshot: &Snapshot, items: impl Fn(&TemplateSummary) -> Vec<Item>) -> Vec<Item> {
    if snapshot.templates.is_empty() {
        return vec![info(tr("No templates yet").into())];
    }
    snapshot.templates.iter().map(|t| submenu(&t.name, items(t))).collect()
}

/// One entry per template that sends `request(id)`.
fn per_template_entry(snapshot: &Snapshot, request: impl Fn(String) -> Request) -> Vec<Item> {
    if snapshot.templates.is_empty() {
        return vec![info(tr("No templates yet").into())];
    }
    snapshot.templates.iter().map(|t| entry(&t.name, request(t.id.clone()))).collect()
}

fn submenu(label: &str, items: Vec<Item>) -> Item {
    Item::Submenu { label: label.into(), items }
}

fn entry(label: &str, request: Request) -> Item {
    entry_if(label, true, request)
}

/// An entry that sends `request`, greyed out unless `enabled`.
fn entry_if(label: &str, enabled: bool, request: Request) -> Item {
    Item::Entry { label: label.into(), action: enabled.then(|| Action::Run(Box::new(request))) }
}

/// A greyed-out line that only informs.
fn info(label: String) -> Item {
    Item::Entry { label, action: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu as text: `▸` marks a submenu, parentheses a greyed-out entry, `-` a separator.
    fn labels(menu: &[Item]) -> Vec<String> {
        menu.iter()
            .map(|item| match item {
                Item::Submenu { label, .. } => format!("{label} ▸"),
                Item::Entry { label, action: Some(_) } => label.clone(),
                Item::Entry { label, action: None } => format!("({label})"),
                Item::Separator => "-".into(),
            })
            .collect()
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            backend: "hyprland".into(),
            workspace: Some("3".into()),
            workspace_windows: 2,
            templates: vec![TemplateSummary {
                id: "dev".into(),
                name: "Entwicklung".into(),
                windows: 3,
                updated: chrono::Utc::now(),
            }],
            ..Snapshot::default()
        }
    }

    /// The action behind the top-level entry labelled `label`.
    fn action_of(menu: &[Item], label: &str) -> Option<Action> {
        menu.iter().find_map(|item| match item {
            Item::Entry { label: l, action } if l == label => action.clone(),
            _ => None,
        })
    }

    #[test]
    fn menu_follows_the_concept() {
        let sub = |label: &str| format!("{label} ▸");
        assert_eq!(
            labels(&build(&snapshot())),
            [
                sub(tr("Load template")),
                tr("Save workspace").to_string(),
                sub(tr("Update")),
                sub(tr("Delete")),
                "-".to_string(),
                tr("Calendar").to_string(),
                tr("Tasks").to_string(),
                "-".to_string(),
                format!(
                    "({})",
                    trf("Workspace {workspace} · {backend}", &[("workspace", &"3"), ("backend", &"hyprland")])
                ),
                "-".to_string(),
                tr("Settings…").to_string(),
                tr("Quit").to_string(),
            ]
        );
    }

    #[test]
    fn entries_run_the_catalogue_commands() {
        let menu = build(&snapshot());
        assert_eq!(
            action_of(&menu, tr("Save workspace")),
            Some(Action::Run(Box::new(Request::SaveTemplate { name: None })))
        );
        assert_eq!(action_of(&menu, tr("Calendar")), Some(Action::OpenApp(AppId::CALENDAR)));
        assert_eq!(action_of(&menu, tr("Tasks")), Some(Action::OpenApp(AppId::TASKS)));
        assert_eq!(
            action_of(&menu, tr("Settings…")),
            Some(Action::Run(Box::new(Request::OpenApp { app: AppId::WORKPLACE.binary.to_string() })))
        );
        assert_eq!(action_of(&menu, tr("Quit")), Some(Action::Run(Box::new(Request::Quit))));
    }

    #[test]
    fn template_submenus_name_the_template() {
        let menu = build(&snapshot());
        let Item::Submenu { items: update, .. } = &menu[2] else { panic!("update submenu") };
        assert_eq!(
            update,
            &[Item::Entry {
                label: "Entwicklung".into(),
                action: Some(Action::Run(Box::new(Request::UpdateTemplate { id: "dev".into() })))
            }]
        );
        let Item::Submenu { items: delete, .. } = &menu[3] else { panic!("delete submenu") };
        assert_eq!(labels(delete), ["Entwicklung"]);
    }

    #[test]
    fn load_here_only_on_an_empty_workspace() {
        let load_items = |snapshot: &Snapshot| {
            let menu = build(snapshot);
            let Item::Submenu { items: load, .. } = &menu[0] else { panic!("load submenu") };
            let Item::Submenu { items: template, .. } = &load[0] else { panic!("template submenu") };
            labels(template)
        };
        let (here, add, replace) = (tr("Load here"), tr("Add").to_string(), tr("Replace").to_string());
        assert_eq!(load_items(&snapshot()), [format!("({here})"), add.clone(), replace.clone()]);
        let empty = Snapshot { workspace_windows: 0, ..snapshot() };
        assert_eq!(load_items(&empty), [here.to_string(), add, replace]);
    }

    #[test]
    fn no_templates_shows_a_hint() {
        let menu = build(&Snapshot { templates: Vec::new(), ..snapshot() });
        let hint = format!("({})", tr("No templates yet"));
        for index in [0, 2, 3] {
            let Item::Submenu { items, .. } = &menu[index] else { panic!("submenu {index}") };
            assert_eq!(labels(items), std::slice::from_ref(&hint));
        }
    }

    #[test]
    fn unsupported_system_shows_why() {
        let snapshot = Snapshot { unsupported: Some("AeroSpace (macOS)".into()), ..Snapshot::default() };
        let menu = labels(&build(&snapshot));
        assert_eq!(menu[0], format!("({})", trf("{name} not supported yet", &[("name", &"AeroSpace (macOS)")])));
        assert_eq!(menu[1], "-");
        assert_eq!(menu.last().map(String::as_str), Some(tr("Quit")));
    }

    #[test]
    fn status_lines_show_busy_and_the_account() {
        let busy = Snapshot { busy: true, account: Some(AccountState::NotConnected), ..snapshot() };
        let menu = labels(&build(&busy));
        assert!(menu.contains(&format!("({})", tr("Busy …"))));
        assert!(menu.contains(&format!("({})", tr("Not connected to Nextcloud"))));
    }
}
