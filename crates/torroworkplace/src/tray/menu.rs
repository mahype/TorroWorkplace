//! The tray menu as plain data, the same on every system. [`build`] turns a [`Snapshot`] of the
//! core into [`Item`]s; the front ends only draw them: `linux.rs` with ksni, `desktop.rs` with
//! tray-icon on macOS and Windows. A click runs the entry's [`Action`] on the [`Daemon`], so
//! every entry does the same thing everywhere.
//!
//! What waits for the person ([`Waiting`], task E3.3b) is **one** number: the open inbox entries
//! "for me". Open approvals are such entries since E2.1, so the "Approvals (n)" of task A.8 went
//! into it; the approvals page stays in the control centre ("Settings…"). The number stands only
//! next to "Inbox" among the apps ("Inbox (3)"): single mails or notices never show in the menu,
//! a tray that lists them gets in the way. What a front end must escape in a label (DBusMenu's
//! `_`, muda's `&`) it does itself.
//!
//! The counts are part of the [`Snapshot`]; the daemon hands over a new one whenever something
//! is written to the inbox or an approval changes, so the tray never asks the socket itself and
//! nothing polls.
//!
//! While voice is on (task C.9, [`voice_section`]) the menu says what the listener does ("Listening
//! for “Hey Torro”", "Recording …", or "Not listening" when it stopped) and offers "Mute
//! microphone", which switches voice off – also when the listener crashed (task C.11), so the way
//! out never depends on it. Switching it on is in the control centre, where the microphone is
//! chosen and tested.
//!
//! The workspace templates (task WS.7, [`templates_section`]): a line naming the window manager,
//! the templates in the first level in the order of their keys, each named with its key ("Deep work ·
//! 1", tray menus know no hints on the right) and opening "Load here", "Add", "Replace" and "Edit…",
//! then "Save workspace…" and "Manage workspaces…". Updating and deleting a template is on the
//! workspaces page of the control centre.
//!
//! The reminders (task ER2.5, [`reminder_section`]): "Next: 10:00 Weekly sync" opens the calendar,
//! "Pause reminders" offers an hour or until tomorrow morning, worked out when clicked
//! ([`Pause::until`]); while paused the menu says until when and offers "Resume reminders". The
//! event's title is the one foreign text in the menu, so it shows as [`plain`] text.

use std::collections::BTreeSet;

use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::Component;
use torro_ipc::{Request, SettingKey, VoiceState};
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
    /// Pauses the reminders from the moment of the click (task ER2.5).
    Pause(Pause),
    /// "Save workspace…" (task WS.7): `workspaces.save_template` without a name at the click, then
    /// the dialog for the new template in its mode "Save" ([`Daemon::save_workspace`]).
    SaveWorkspace,
    /// "Manage workspaces…" (task WS.7): the control centre on its page Workspaces
    /// ([`Daemon::manage_workspaces`]).
    ManageWorkspaces,
}

/// How long "Pause reminders" pauses (task ER2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// An hour.
    Hour,
    /// Until the next 8 o'clock in the morning, local time.
    UntilTomorrow,
}

impl Pause {
    /// When the pause that starts at `now` ends; always less than a day ahead.
    pub fn until(self, now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
        match self {
            Pause::Hour => now + chrono::TimeDelta::hours(1),
            Pause::UntilTomorrow => {
                let local = now.with_timezone(&chrono::Local);
                let morning = |date: chrono::NaiveDate| {
                    date.and_hms_opt(8, 0, 0)
                        .and_then(|at| at.and_local_timezone(chrono::Local).earliest())
                        .map(|at| at.with_timezone(&chrono::Utc))
                };
                let today = morning(local.date_naive()).filter(|at| *at > now);
                let tomorrow = local.date_naive().succ_opt().and_then(morning);
                today.or(tomorrow).unwrap_or(now + chrono::TimeDelta::hours(12))
            }
        }
    }
}

impl Action {
    /// Carries the action out. Returns at once; the work runs on the daemon's runtime.
    pub fn perform(&self, daemon: &Daemon) {
        match self {
            Action::Run(request) => daemon.run((**request).clone()),
            Action::OpenApp(app) => daemon.open_app(*app),
            Action::Pause(pause) => {
                daemon.run(Request::PauseReminders { until: Some(pause.until(chrono::Utc::now())) });
            }
            Action::SaveWorkspace => daemon.save_workspace(),
            Action::ManageWorkspaces => daemon.manage_workspaces(),
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

/// What waits for the person, counted in the tray (task E3.3b): the open inbox entries "for me",
/// approvals included (task E2.1), so they are never counted twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Waiting {
    /// Open inbox entries "for me" ([`Snapshot::inbox_for_me`]).
    pub for_me: usize,
    /// How many of them are urgent ([`Snapshot::inbox_urgent`]).
    pub urgent: usize,
}

impl Waiting {
    /// What waits according to `snapshot`.
    pub fn of(snapshot: &Snapshot) -> Waiting {
        Waiting { for_me: snapshot.inbox_for_me, urgent: snapshot.inbox_urgent }
    }

    /// Everything that waits, as one number.
    pub fn total(self) -> usize {
        self.for_me
    }
}

/// "3 for you", or nothing when nothing waits: the tooltip line of every front end. Only the
/// count, never a title: bars may read a tooltip as markup, and titles are foreign text.
pub fn waiting_line(waiting: Waiting) -> String {
    match waiting.total() {
        0 => String::new(),
        n => trf("{n} for you", &[("n", &n)]),
    }
}

/// The menu from docs/core.md for one snapshot: the apps first, then the workspace templates,
/// then "Settings…" and "Quit". Which workspace is active and which Nextcloud is signed in are
/// left out: they say nothing the person needs here. Only work in progress or a problem with the
/// templates gets a line ([`status_line`]).
///
/// Labels stay short: Omarchy's bar draws tray menus about 232 px wide and cuts longer
/// text off with an ellipsis.
pub fn build(snapshot: &Snapshot) -> Vec<Item> {
    let mut menu = voice_section(snapshot.voice, snapshot.voice_enabled);
    menu.extend(apps_section(Waiting::of(snapshot), &snapshot.components_off));
    menu.push(Item::Separator);
    // Without the calendar (task W.2) no reminders come, so there is nothing to pause.
    if !snapshot.components_off.contains(&Component::Calendar) {
        menu.extend(reminder_section(snapshot));
    }
    // Without workspaces (task W.2) the templates are left out.
    if !snapshot.components_off.contains(&Component::Workspaces) {
        menu.extend(templates_section(snapshot));
    }
    if let Some(line) = status_line(snapshot) {
        menu.push(info(line));
    }
    menu.push(Item::Separator);
    menu.push(entry(tr("Settings…"), Request::OpenApp { app: AppId::WORKPLACE.binary.to_string(), target: None }));
    menu.push(entry(tr("Quit"), Request::Quit));
    menu
}

/// The workspace templates (task WS.7): "Workspaces · Hyprland", one submenu per template in the
/// order of their keys ([`crate::workspace_keys::in_key_order`], as on the workspaces page) named
/// with its key ([`template_label`]), or "No templates yet"; then a separator, "Save workspace…" and
/// "Manage workspaces…". Without a window manager only why ("AeroSpace (macOS) not supported yet")
/// and "Manage workspaces…", whose page says what to do: nothing could be loaded or saved.
fn templates_section(snapshot: &Snapshot) -> Vec<Item> {
    let manage = Item::Entry { label: tr("Manage workspaces…").into(), action: Some(Action::ManageWorkspaces) };
    if let Some(name) = &snapshot.unsupported {
        return vec![info(trf("{name} not supported yet", &[("name", name)])), manage];
    }
    let mut section = vec![info(trf("Workspaces · {manager}", &[("manager", &manager_name(&snapshot.backend))]))];
    if snapshot.templates.is_empty() {
        section.push(info(tr("No templates yet").into()));
    }
    for template in crate::workspace_keys::in_key_order(&snapshot.templates) {
        section.push(submenu(&template_label(template), template_items(snapshot, &template.id)));
    }
    section.push(Item::Separator);
    section.push(Item::Entry { label: tr("Save workspace…").into(), action: Some(Action::SaveWorkspace) });
    section.push(manage);
    section
}

/// A template's line: its name with its key behind ("Deep work · 1", "Writing · 0"), or the name
/// alone. The name is cut like a title ([`plain`]): an agent may have named it
/// (`workspaces.rename_template`).
fn template_label(template: &TemplateSummary) -> String {
    let name = plain(&template.name, TITLE_IN_MENU);
    match template.shortcut {
        Some(key) => trf("{name} · {key}", &[("name", &name), ("key", &key)]),
        None => name,
    }
}

/// The submenu of the template `id`: "Load here" (only on an empty workspace), "Add", "Replace", and
/// apart from them "Edit…", which opens the template's dialog in its mode "Edit" (`apps.open_app` on
/// the target `template:<id>`).
fn template_items(snapshot: &Snapshot, id: &str) -> Vec<Item> {
    let load = |mode| Request::LoadTemplate { id: id.to_string(), mode };
    let edit = Request::OpenApp { app: AppId::WORKPLACE.binary.to_string(), target: Some(format!("template:{id}")) };
    vec![
        entry_if(tr("Load here"), snapshot.workspace_windows == 0, load(LoadMode::Empty)),
        entry(tr("Add"), load(LoadMode::Add)),
        entry(tr("Replace"), load(LoadMode::Replace)),
        Item::Separator,
        entry(tr("Edit…"), edit),
    ]
}

/// The name of the window manager the core reports (`hyprland`) as people write it.
fn manager_name(backend: &str) -> String {
    match backend {
        "hyprland" => "Hyprland".into(),
        "aerospace" => "AeroSpace".into(),
        "glazewm" => "GlazeWM".into(),
        other => other.to_string(),
    }
}

/// What the voice listener does, for the tooltip and the menu; empty while voice is off.
pub fn voice_line(state: VoiceState) -> &'static str {
    match state {
        VoiceState::Off => "",
        VoiceState::Listening => tr("Listening for “Hey Torro”"),
        VoiceState::Recording => tr("Recording …"),
        VoiceState::Answering => tr("Answering …"),
    }
}

/// While voice is on (`enabled`, the setting) or the listener still runs: what it does – "Not
/// listening" when it does not run although voice is on – and "Mute microphone", which switches
/// voice off (`settings.set_setting voice_enabled false`), then a separator. Empty while voice is
/// off and the listener stopped.
fn voice_section(state: VoiceState, enabled: bool) -> Vec<Item> {
    if state == VoiceState::Off && !enabled {
        return Vec::new();
    }
    let line = match state {
        VoiceState::Off => tr("Not listening"),
        state => voice_line(state),
    };
    let mute = Request::SetSetting { key: SettingKey::VoiceEnabled, value: serde_json::Value::Bool(false) };
    vec![info(line.into()), entry(tr("Mute microphone"), mute), Item::Separator]
}

/// One entry per app of the suite whose component is on (task W.2); the control centre is
/// "Settings…". Only the Inbox carries a number, what waits for the person ("Inbox (3)").
fn apps_section(waiting: Waiting, off: &BTreeSet<Component>) -> Vec<Item> {
    let inbox = match waiting.total() {
        0 => tr("Notification Hub").to_string(),
        n => trf("Notification Hub ({n})", &[("n", &n)]),
    };
    vec![
        open(&inbox, AppId::INBOX),
        open(tr("Mail"), AppId::INBOX_MAIL),
        open(tr("App Feed"), AppId::INBOX_NOTIFICATIONS),
        open(tr("Calendar"), AppId::CALENDAR),
        open(tr("Tasks"), AppId::TASKS),
        open(tr("Assistant"), AppId::ASSISTANT),
    ]
    .into_iter()
    .filter(|item| match item {
        Item::Entry { action: Some(Action::OpenApp(app)), .. } => {
            torro_gui::components::of(*app).is_none_or(|component| !off.contains(&component))
        }
        _ => true,
    })
    .collect()
}

/// The reminders (task ER2.5): the next event, and the pause or the way to it, then a separator.
/// Empty while no reminder is coming and none are paused: then there is nothing to pause.
fn reminder_section(snapshot: &Snapshot) -> Vec<Item> {
    if snapshot.next_reminder.is_none() && snapshot.reminders_paused.is_none() {
        return Vec::new();
    }
    let clock =
        |at: chrono::DateTime<chrono::Utc>| torro_gui::datetime::format_time(torro_gui::datetime::to_local(at).time());
    let mut section = Vec::new();
    if let Some((start, title)) = &snapshot.next_reminder {
        let label = trf("Next: {time} {title}", &[("time", &clock(*start)), ("title", &plain(title, TITLE_IN_MENU))]);
        section.push(open(&label, AppId::CALENDAR));
    }
    match snapshot.reminders_paused {
        Some(until) => {
            section.push(info(trf("Reminders paused until {time}", &[("time", &clock(until))])));
            section.push(entry(tr("Resume reminders"), Request::PauseReminders { until: None }));
        }
        None => section.push(submenu(
            tr("Pause reminders"),
            vec![
                Item::Entry { label: tr("For 1 hour").into(), action: Some(Action::Pause(Pause::Hour)) },
                Item::Entry { label: tr("Until tomorrow").into(), action: Some(Action::Pause(Pause::UntilTomorrow)) },
            ],
        )),
    }
    section.push(Item::Separator);
    section
}

/// The most characters of an event's title in the menu: the bar draws it about 232 px wide.
const TITLE_IN_MENU: usize = 28;

/// `text` from outside the suite, cut to `max` characters, so that it shows exactly as written in
/// a menu label: control characters (line breaks, tabs) become spaces, and the invisible marks
/// that turn the direction of text are dropped, so a title cannot make the line read differently.
/// Mnemonic marks are left to the front ends, which know their own (`linux.rs`, `desktop.rs`).
fn plain(text: &str, max: usize) -> String {
    let flat: String =
        text.chars().filter(|c| !turns_direction(*c)).map(|c| if c.is_control() { ' ' } else { c }).collect();
    crate::inbox::cap(flat.trim(), max)
}

/// The Unicode marks that embed, override or isolate a direction of text, and the plain marks.
fn turns_direction(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// "Busy …" while a template is loaded or saved, or the problem the core ran into (the window
/// manager not reachable, templates not readable); nothing while all is well.
fn status_line(snapshot: &Snapshot) -> Option<String> {
    if snapshot.busy {
        return Some(tr("Busy …").into());
    }
    snapshot.problem.clone()
}

/// An entry that opens or focuses one of the suite's apps.
fn open(label: &str, app: AppId) -> Item {
    Item::Entry { label: label.into(), action: Some(Action::OpenApp(app)) }
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
    use crate::core::InboxLine;
    use torro_pim::AccountState;

    /// Where the template entries start: after the six apps and a separator.
    const TEMPLATES: usize = 7;

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

    /// A template `id` named `name` with the key `key`.
    fn summary(id: &str, name: &str, key: Option<u8>) -> TemplateSummary {
        TemplateSummary {
            id: id.into(),
            name: name.into(),
            windows: 3,
            updated: chrono::Utc::now(),
            shortcut: key,
            startup: None,
            workspace: "3".into(),
            monitor: None,
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            backend: "hyprland".into(),
            workspace: Some("3".into()),
            workspace_windows: 2,
            templates: vec![summary("dev", "Entwicklung", None)],
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

    /// The items of the top-level submenu labelled `label`.
    fn submenu_of<'a>(menu: &'a [Item], label: &str) -> &'a [Item] {
        menu.iter()
            .find_map(|item| match item {
                Item::Submenu { label: l, items } if l == label => Some(items.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no submenu {label:?} in {:?}", labels(menu)))
    }

    /// Every action of `menu`, submenus included.
    fn all_actions(menu: &[Item]) -> Vec<Action> {
        menu.iter()
            .flat_map(|item| match item {
                Item::Submenu { items, .. } => all_actions(items),
                Item::Entry { action: Some(action), .. } => vec![action.clone()],
                _ => Vec::new(),
            })
            .collect()
    }

    fn workspaces_line() -> String {
        format!("({})", trf("Workspaces · {manager}", &[("manager", &"Hyprland")]))
    }

    #[test]
    fn menu_follows_the_concept() {
        let sub = |label: &str| format!("{label} ▸");
        assert_eq!(
            labels(&build(&snapshot())),
            [
                tr("Notification Hub").to_string(),
                tr("Mail").to_string(),
                tr("App Feed").to_string(),
                tr("Calendar").to_string(),
                tr("Tasks").to_string(),
                tr("Assistant").to_string(),
                "-".to_string(),
                workspaces_line(),
                sub("Entwicklung"),
                "-".to_string(),
                tr("Save workspace…").to_string(),
                tr("Manage workspaces…").to_string(),
                "-".to_string(),
                tr("Settings…").to_string(),
                tr("Quit").to_string(),
            ]
        );
    }

    /// Task WS.7: the templates stand in the first level in the order of their keys (1 … 9, 0, then
    /// by name), each with its key behind its name.
    #[test]
    fn templates_stand_in_the_first_level_by_key_with_the_key_behind() {
        let templates = vec![
            summary("writing", "Writing", Some(0)),
            summary("dev", "Entwicklung", None),
            summary("mail", "Mail & chat", Some(2)),
            summary("alpha", "alpha", None),
            summary("deep", "Deep work", Some(1)),
        ];
        let menu = labels(&build(&Snapshot { templates, ..snapshot() }));
        let keyed = |name: &str, key: u8| format!("{} ▸", trf("{name} · {key}", &[("name", &name), ("key", &key)]));
        assert_eq!(
            menu[TEMPLATES..TEMPLATES + 7],
            [
                workspaces_line(),
                keyed("Deep work", 1),
                keyed("Mail & chat", 2),
                keyed("Writing", 0),
                "alpha ▸".to_string(),
                "Entwicklung ▸".to_string(),
                "-".to_string(),
            ]
        );
    }

    /// Task WS.7: a template's name shows as plain text, cut like a title: an agent may have named it.
    #[test]
    fn template_names_show_as_plain_text() {
        let templates = vec![summary("odd", "Two\nlines\u{202E}", Some(3))];
        let menu = labels(&build(&Snapshot { templates, ..snapshot() }));
        assert_eq!(menu[TEMPLATES + 1], format!("{} ▸", trf("{name} · {key}", &[("name", &"Two lines"), ("key", &3)])));
    }

    /// Task W.2: components turned off leave the menu.
    #[test]
    fn apps_and_templates_of_components_turned_off_are_left_out() {
        let snapshot =
            Snapshot { components_off: BTreeSet::from([Component::Mail, Component::Workspaces]), ..snapshot() };
        let menu = build(&snapshot);
        assert_eq!(action_of(&menu, tr("Mail")), None);
        assert!(action_of(&menu, tr("Calendar")).is_some());
        let shown = labels(&menu);
        for gone in
            [workspaces_line(), "Entwicklung ▸".into(), tr("Save workspace…").into(), tr("Manage workspaces…").into()]
        {
            assert!(!shown.contains(&gone), "{gone:?} in {shown:?}");
        }
    }

    #[test]
    fn entries_run_the_catalogue_commands() {
        let menu = build(&snapshot());
        assert_eq!(action_of(&menu, tr("Save workspace…")), Some(Action::SaveWorkspace));
        assert_eq!(action_of(&menu, tr("Manage workspaces…")), Some(Action::ManageWorkspaces));
        assert_eq!(action_of(&menu, tr("Notification Hub")), Some(Action::OpenApp(AppId::INBOX)));
        assert_eq!(action_of(&menu, tr("Mail")), Some(Action::OpenApp(AppId::INBOX_MAIL)));
        assert_eq!(action_of(&menu, tr("App Feed")), Some(Action::OpenApp(AppId::INBOX_NOTIFICATIONS)));
        assert_eq!(action_of(&menu, tr("Calendar")), Some(Action::OpenApp(AppId::CALENDAR)));
        assert_eq!(action_of(&menu, tr("Tasks")), Some(Action::OpenApp(AppId::TASKS)));
        assert_eq!(action_of(&menu, tr("Assistant")), Some(Action::OpenApp(AppId::ASSISTANT)));
        assert_eq!(
            action_of(&menu, tr("Settings…")),
            Some(Action::Run(Box::new(Request::OpenApp { app: AppId::WORKPLACE.binary.to_string(), target: None })))
        );
        assert_eq!(action_of(&menu, tr("Quit")), Some(Action::Run(Box::new(Request::Quit))));
    }

    /// Task WS.7: a template's submenu loads in the three modes and opens its dialog to edit it, the
    /// target `template:<id>` of `apps.open_app`; updating and deleting is on the workspaces page.
    #[test]
    fn a_template_loads_or_opens_its_dialog_and_nothing_updates_or_deletes() {
        let menu = build(&snapshot());
        let items = submenu_of(&menu, "Entwicklung");
        let load = |mode| Some(Action::Run(Box::new(Request::LoadTemplate { id: "dev".into(), mode })));
        assert_eq!(action_of(items, tr("Add")), load(LoadMode::Add));
        assert_eq!(action_of(items, tr("Replace")), load(LoadMode::Replace));
        let edit = Request::OpenApp { app: AppId::WORKPLACE.binary.to_string(), target: Some("template:dev".into()) };
        assert_eq!(action_of(items, tr("Edit…")), Some(Action::Run(Box::new(edit))));
        // The target is one `apps.open_app` takes, in the mode "Edit".
        assert_eq!(crate::apps::Target::parse("template:dev"), Ok(crate::apps::Target::Template { id: "dev" }));
        assert!(!all_actions(&menu).iter().any(|action| matches!(
            action,
            Action::Run(request) if matches!(**request, Request::UpdateTemplate { .. } | Request::DeleteTemplate { .. })
        )));
    }

    #[test]
    fn load_here_only_on_an_empty_workspace() {
        let template = |snapshot: &Snapshot| labels(submenu_of(&build(snapshot), "Entwicklung"));
        let (here, add, replace) = (tr("Load here"), tr("Add").to_string(), tr("Replace").to_string());
        let edit = tr("Edit…").to_string();
        assert_eq!(
            template(&snapshot()),
            [format!("({here})"), add.clone(), replace.clone(), "-".into(), edit.clone()]
        );
        let empty = Snapshot { workspace_windows: 0, ..snapshot() };
        assert_eq!(template(&empty), [here.to_string(), add, replace, "-".into(), edit]);
    }

    #[test]
    fn no_templates_shows_a_hint() {
        let menu = labels(&build(&Snapshot { templates: Vec::new(), ..snapshot() }));
        assert_eq!(
            menu[TEMPLATES..TEMPLATES + 5],
            [
                workspaces_line(),
                format!("({})", tr("No templates yet")),
                "-".to_string(),
                tr("Save workspace…").to_string(),
                tr("Manage workspaces…").to_string(),
            ]
        );
    }

    /// Without a window manager nothing loads or saves: the menu says why and leads to the page.
    #[test]
    fn unsupported_system_shows_why() {
        let snapshot = Snapshot { unsupported: Some("AeroSpace (macOS)".into()), ..Snapshot::default() };
        let menu = build(&snapshot);
        let shown = labels(&menu);
        assert_eq!(
            shown[TEMPLATES..TEMPLATES + 3],
            [
                format!("({})", trf("{name} not supported yet", &[("name", &"AeroSpace (macOS)")])),
                tr("Manage workspaces…").to_string(),
                "-".to_string(),
            ]
        );
        assert_eq!(action_of(&menu, tr("Manage workspaces…")), Some(Action::ManageWorkspaces));
        assert!(!shown.contains(&tr("Save workspace…").to_string()));
        assert_eq!(shown.last().map(String::as_str), Some(tr("Quit")));
    }

    #[test]
    fn only_work_in_progress_or_a_problem_gets_a_line() {
        // After "Manage workspaces…".
        const STATUS: usize = TEMPLATES + 5;
        let quiet = labels(&build(&Snapshot { account: Some(AccountState::NotConnected), ..snapshot() }));
        assert_eq!(quiet[STATUS], "-", "no workspace, no Nextcloud line");
        assert!(!quiet.contains(&format!("({})", tr("Not connected to Nextcloud"))));

        let busy = labels(&build(&Snapshot { busy: true, ..snapshot() }));
        assert_eq!(busy[STATUS], format!("({})", tr("Busy …")), "below the templates");
        let broken = labels(&build(&Snapshot { problem: Some("Hyprland not reachable".into()), ..snapshot() }));
        assert_eq!(broken[STATUS], "(Hyprland not reachable)");
    }

    /// Task E3.3b: one number for everything "for me", approvals included, next to "Inbox" only;
    /// single entries never show in the menu.
    #[test]
    fn the_inbox_carries_one_number_and_no_entries() {
        let none = Waiting::of(&snapshot());
        assert_eq!(none, Waiting::default());
        assert_eq!(waiting_line(none), String::new());

        // An approval and two notices; the snapshot still knows the approval apart, the tray not.
        let waiting = Snapshot {
            approvals_waiting: 1,
            inbox_for_me: 3,
            inbox_urgent: 1,
            inbox_newest: vec![InboxLine { id: "a".into(), title: "Backup failed".into(), source: "backup".into() }],
            ..snapshot()
        };
        let three = Waiting::of(&waiting);
        assert_eq!((three, three.total()), (Waiting { for_me: 3, urgent: 1 }, 3), "approvals are not counted twice");
        let menu = build(&waiting);
        let inbox = trf("Notification Hub ({n})", &[("n", &3)]);
        assert_eq!(action_of(&menu, &inbox), Some(Action::OpenApp(AppId::INBOX)));
        let quiet = labels(&build(&snapshot()));
        let counted: Vec<String> =
            quiet.iter().map(|l| if l == tr("Notification Hub") { inbox.clone() } else { l.clone() }).collect();
        assert_eq!(labels(&menu), counted, "only the Inbox label changes, nothing is added");
        assert!(!labels(&menu).iter().any(|l| l.contains("Backup failed")), "no single entry in the menu");
        assert!(!labels(&menu).contains(&trf("Approvals ({n})", &[("n", &1)])), "no second number for approvals");
        assert_eq!(waiting_line(three), trf("{n} for you", &[("n", &3)]));
    }

    /// Task C.9: while the listener runs the menu starts with what it does and "Mute microphone";
    /// voice off, there is nothing about it.
    #[test]
    fn the_menu_shows_listening_and_mutes_the_microphone() {
        assert_eq!(labels(&build(&snapshot()))[0], tr("Notification Hub"), "off: nothing");
        for state in [VoiceState::Listening, VoiceState::Recording, VoiceState::Answering] {
            let menu = build(&Snapshot { voice: state, ..snapshot() });
            assert_eq!(
                labels(&menu)[..3],
                [format!("({})", voice_line(state)), tr("Mute microphone").to_string(), "-".to_string()],
                "{state:?}"
            );
            assert_eq!(
                action_of(&menu, tr("Mute microphone")),
                Some(Action::Run(Box::new(Request::SetSetting {
                    key: SettingKey::VoiceEnabled,
                    value: serde_json::json!(false)
                })))
            );
        }
        assert_eq!(voice_line(VoiceState::Recording), tr("Recording …"));
        assert_eq!(voice_line(VoiceState::Off), "");
    }

    /// Task C.11: voice on but the listener gone (crashed, or it could not start) still offers
    /// "Mute microphone", which switches voice off; the line says it does not listen.
    #[test]
    fn mute_stays_when_the_listener_crashed() {
        let crashed = Snapshot { voice: VoiceState::Off, voice_enabled: true, ..snapshot() };
        let menu = build(&crashed);
        assert_eq!(
            labels(&menu)[..3],
            [format!("({})", tr("Not listening")), tr("Mute microphone").to_string(), "-".to_string()]
        );
        assert_eq!(
            action_of(&menu, tr("Mute microphone")),
            Some(Action::Run(Box::new(Request::SetSetting {
                key: SettingKey::VoiceEnabled,
                value: serde_json::json!(false)
            })))
        );
        let off = Snapshot { voice: VoiceState::Off, voice_enabled: false, ..snapshot() };
        assert!(!labels(&build(&off)).contains(&tr("Mute microphone").to_string()), "voice off: nothing");
    }

    #[test]
    fn foreign_titles_show_as_plain_text() {
        assert_eq!(plain("fix_build", TITLE_IN_MENU), "fix_build", "mnemonics are the front ends' job");
        assert_eq!(plain("two\nlines\there", TITLE_IN_MENU), "two lines here");
        assert_eq!(plain("\u{202E}gnp.exe", TITLE_IN_MENU), "gnp.exe", "no turned text");
        assert_eq!(plain("<b>bold</b> &amp;", TITLE_IN_MENU), "<b>bold</b> &amp;", "markup stays text");
        let long = "A very long title from an agent that goes on";
        let cut: String = long.chars().take(TITLE_IN_MENU - 1).collect();
        assert_eq!(plain(long, TITLE_IN_MENU), format!("{cut}…"));
    }

    /// Task ER2.5: the next reminder, the pause and the way back.
    #[test]
    fn reminders_show_the_next_event_and_pause_or_resume() {
        use chrono::TimeZone;
        let at = chrono::Utc.with_ymd_and_hms(2026, 10, 15, 8, 0, 0).single().expect("a time");
        let labels = |items: &[Item]| -> Vec<String> {
            items
                .iter()
                .filter_map(|item| match item {
                    Item::Entry { label, .. } | Item::Submenu { label, .. } => Some(label.clone()),
                    Item::Separator => None,
                })
                .collect()
        };
        assert!(reminder_section(&Snapshot::default()).is_empty(), "nothing coming, nothing to pause");
        let coming = Snapshot { next_reminder: Some((at, "Weekly\nsync".into())), ..Snapshot::default() };
        let section = reminder_section(&coming);
        let time = torro_gui::datetime::format_time(torro_gui::datetime::to_local(at).time());
        assert_eq!(
            labels(&section),
            [
                trf("Next: {time} {title}", &[("time", &time), ("title", &"Weekly sync")]),
                tr("Pause reminders").to_string()
            ]
        );
        let Item::Submenu { items, .. } = &section[1] else { panic!("the pause's submenu") };
        assert!(matches!(&items[0], Item::Entry { action: Some(Action::Pause(Pause::Hour)), .. }));
        let paused = Snapshot { reminders_paused: Some(at), ..Snapshot::default() };
        let section = reminder_section(&paused);
        assert_eq!(labels(&section)[1], tr("Resume reminders"));
        assert!(matches!(
            &section[1],
            Item::Entry { action: Some(Action::Run(request)), .. } if **request == Request::PauseReminders { until: None }
        ));
        let calendar_off = Snapshot { components_off: BTreeSet::from([Component::Calendar]), ..coming };
        assert!(
            !build(&calendar_off)
                .iter()
                .any(|item| matches!(item, Item::Submenu { label, .. } if label == tr("Pause reminders")))
        );
    }

    #[test]
    fn a_pause_ends_within_a_day() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 15, 14, 0, 0).single().expect("a time");
        assert_eq!(Pause::Hour.until(now), now + chrono::TimeDelta::hours(1));
        let morning = Pause::UntilTomorrow.until(now);
        assert!(morning > now && morning <= now + chrono::TimeDelta::hours(24), "{morning}");
    }
}
