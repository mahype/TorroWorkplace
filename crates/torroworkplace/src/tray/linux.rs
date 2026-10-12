//! Tray icon for Linux, built on ksni (StatusNotifierItem over D-Bus; Omarchy's bar,
//! Waybar and KDE show it). The menu is built from a [`Snapshot`] of the core and turns
//! clicks into the same [`Request`]s the socket takes. The icon comes from `icon.rs`.
//!
//! What waits for the person ([`Waiting`], task E3.3b) is **one** number: the open inbox entries
//! "for me". Open approvals are such entries since E2.1, so the "Approvals (n)" of task A.8 went
//! into it; the approvals page stays in the control centre ("Settings…"). While something waits,
//! the menu starts with "Inbox (n)", the five newest entries and "Open Inbox", all opening the
//! Inbox app (it cannot be told yet which entry to show: that needs a channel into the running
//! app). The tooltip says only how many ("3 for you"), never a title: bars may read the
//! description as markup, and titles of foreign sources are `untrusted`. The item status is
//! `NeedsAttention`, which bars use to highlight the icon, only while something urgent waits (an
//! approval, an agent waiting); notices and finished agents count but stay calm.
//!
//! A number drawn onto the icon is left out: the protocol's overlay icon is only another pixmap,
//! so the digits would have to be rasterised here, and many bars ignore it. The counts are part of
//! the [`Snapshot`]; the daemon hands over a new one whenever something is written to the inbox
//! or an approval changes, so the tray never asks the socket itself and nothing polls.

use std::fmt;
use std::sync::Arc;

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_pim::AccountState;
use torro_setups::LoadMode;

use super::icon;
use crate::core::{InboxLine, Snapshot};
use crate::daemon::Daemon;

/// Errors of the tray icon.
#[derive(Debug)]
pub enum TrayError {
    /// The tray could not be registered on D-Bus or shown by the desktop.
    Spawn(ksni::Error),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrayError::Spawn(err) => write!(f, "cannot show the tray icon: {err}"),
        }
    }
}

impl std::error::Error for TrayError {}

/// Keeps the icon alive; updating it shows a new snapshot.
pub type TrayHandle = ksni::Handle<Tray>;

/// The tray: the last snapshot and the way back into the daemon.
pub struct Tray {
    pub snapshot: Snapshot,
    daemon: Arc<Daemon>,
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

/// Shows the icon. Clicks go to `daemon`; [`TrayHandle::update`] replaces the snapshot, and with
/// it the count of what waits.
pub async fn spawn(daemon: Arc<Daemon>, snapshot: Snapshot) -> Result<TrayHandle, TrayError> {
    Tray { snapshot, daemon }.spawn().await.map_err(TrayError::Spawn)
}

impl ksni::Tray for Tray {
    /// A left click opens the menu, like every other icon in the bar (sets `ItemIsMenu`).
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "Torro Workplace".into()
    }

    /// Bars highlight an item that needs attention: only while something urgent waits.
    fn status(&self) -> ksni::Status {
        item_status(Waiting::of(&self.snapshot))
    }

    /// The name and, when something waits, how much. Never a summary: the description may be
    /// read as markup by the bar, and summaries hold text a client sent.
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Torro Workplace".into(),
            description: waiting_line(Waiting::of(&self.snapshot)),
            ..Default::default()
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        icon::SIZES
            .iter()
            .map(|&size| ksni::Icon { width: size as i32, height: size as i32, data: icon::argb32(size) })
            .collect()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        build_menu(&self.snapshot)
    }
}

/// `NeedsAttention` while something urgent waits (an approval, an agent waiting), else `Active`.
/// Notices and finished agents count in the number but do not make the icon stand out: the tray
/// stays calm for what can wait.
fn item_status(waiting: Waiting) -> ksni::Status {
    if waiting.urgent > 0 { ksni::Status::NeedsAttention } else { ksni::Status::Active }
}

/// "3 for you", or nothing when nothing waits. Only the count: the bar may read the description
/// as markup.
fn waiting_line(waiting: Waiting) -> String {
    match waiting.total() {
        0 => String::new(),
        n => trf("{n} for you", &[("n", &n)]),
    }
}

/// The longest title of an entry in the menu, in characters, before it ends in "…"; with the
/// source after it, that is about what Omarchy's bar shows of a line.
const TITLE_IN_MENU: usize = 28;

/// The longest source name of an entry in the menu, in characters.
const SOURCE_IN_MENU: usize = 16;

/// The menu from docs/core.md for one snapshot, with the inbox on top while something waits
/// ([`inbox_section`]).
///
/// Labels stay short: Omarchy's bar draws tray menus about 232 px wide and cuts longer
/// text off with an ellipsis.
fn build_menu(snapshot: &Snapshot) -> Vec<MenuItem<Tray>> {
    let mut menu = inbox_section(snapshot);
    if let Some(name) = &snapshot.unsupported {
        menu.push(info(trf("{name} not supported yet", &[("name", name)])));
        menu.push(MenuItem::Separator);
    } else {
        let load = per_template(snapshot, |t| {
            let id = &t.id;
            vec![
                action_if(
                    tr("Load here"),
                    snapshot.workspace_windows == 0,
                    Request::LoadTemplate { id: id.clone(), mode: LoadMode::Empty },
                ),
                action(tr("Add"), Request::LoadTemplate { id: id.clone(), mode: LoadMode::Add }),
                action(tr("Replace"), Request::LoadTemplate { id: id.clone(), mode: LoadMode::Replace }),
            ]
        });
        menu.push(submenu(tr("Load template"), load));
        menu.push(action(tr("Save workspace"), Request::SaveTemplate { name: None }));
        let update = per_template_action(snapshot, |id| Request::UpdateTemplate { id });
        menu.push(submenu(tr("Update"), update));
        let delete = per_template_action(snapshot, |id| Request::DeleteTemplate { id });
        menu.push(submenu(tr("Delete"), delete));
        menu.push(MenuItem::Separator);
    }
    menu.push(open("Inbox", AppId::INBOX));
    menu.push(open("Calendar", AppId::CALENDAR));
    menu.push(open("Tasks", AppId::TASKS));
    menu.push(MenuItem::Separator);
    menu.push(info(info_line(snapshot)));
    if let Some(line) = account_line(snapshot) {
        menu.push(info(line));
    }
    menu.push(MenuItem::Separator);
    menu.push(
        StandardItem {
            label: tr("Settings…").into(),
            activate: Box::new(|tray: &mut Tray| {
                tray.daemon.run(Request::OpenApp { app: AppId::WORKPLACE.binary.to_string() })
            }),
            ..Default::default()
        }
        .into(),
    );
    menu.push(
        StandardItem {
            label: tr("Quit").into(),
            activate: Box::new(|tray: &mut Tray| tray.daemon.run(Request::Quit)),
            ..Default::default()
        }
        .into(),
    );
    menu
}

/// While something waits: "Inbox (n)", the newest entries ("title · source") and "Open Inbox",
/// then a separator. Empty when nothing waits; the Inbox app is still among the apps below.
/// Every entry opens the Inbox app; jumping to one entry needs a channel into the app.
fn inbox_section(snapshot: &Snapshot) -> Vec<MenuItem<Tray>> {
    let waiting = Waiting::of(snapshot);
    if waiting.total() == 0 {
        return Vec::new();
    }
    let mut section = vec![open_inbox(trf("Inbox ({n})", &[("n", &waiting.total())]))];
    section.extend(snapshot.inbox_newest.iter().map(|line| open_inbox(entry_label(line))));
    section.push(open_inbox(trf("Open {app}", &[("app", &tr("Inbox"))])));
    section.push(MenuItem::Separator);
    section
}

/// An entry labelled `label` that opens or focuses the Inbox app.
fn open_inbox(label: String) -> MenuItem<Tray> {
    StandardItem {
        label,
        activate: Box::new(|tray: &mut Tray| tray.daemon.open_app(AppId::INBOX)),
        ..Default::default()
    }
    .into()
}

/// "Title · Source" for one entry, both as [`plain`] text.
fn entry_label(line: &InboxLine) -> String {
    format!("{} · {}", plain(&line.title, TITLE_IN_MENU), plain(&line.source, SOURCE_IN_MENU))
}

/// `text` from outside the suite, cut to `max` characters, so that it shows exactly as written in
/// a menu label: control characters (line breaks, tabs) become spaces, and the invisible marks
/// that turn the direction of text are dropped, so a title cannot make the line read differently.
/// Labels are no markup, but DBusMenu takes an underscore as the mark of an access key, so each
/// one is doubled to show as itself.
fn plain(text: &str, max: usize) -> String {
    let flat: String =
        text.chars().filter(|c| !turns_direction(*c)).map(|c| if c.is_control() { ' ' } else { c }).collect();
    crate::inbox::cap(flat.trim(), max).replace('_', "__")
}

/// The Unicode marks that embed, override or isolate a direction of text, and the plain marks.
fn turns_direction(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
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
fn open(label: &'static str, app: AppId) -> MenuItem<Tray> {
    StandardItem {
        label: tr(label).into(),
        activate: Box::new(move |tray: &mut Tray| tray.daemon.open_app(app)),
        ..Default::default()
    }
    .into()
}

/// One submenu per template, or a disabled hint when there are none.
fn per_template(
    snapshot: &Snapshot,
    items: impl Fn(&torro_setups::TemplateSummary) -> Vec<MenuItem<Tray>>,
) -> Vec<MenuItem<Tray>> {
    if snapshot.templates.is_empty() {
        return vec![info(tr("No templates yet").into())];
    }
    snapshot.templates.iter().map(|t| submenu(&t.name, items(t))).collect()
}

/// One entry per template that sends `request(id)`.
fn per_template_action(snapshot: &Snapshot, request: impl Fn(String) -> Request) -> Vec<MenuItem<Tray>> {
    if snapshot.templates.is_empty() {
        return vec![info(tr("No templates yet").into())];
    }
    snapshot.templates.iter().map(|t| action(&t.name, request(t.id.clone()))).collect()
}

fn submenu(label: &str, submenu: Vec<MenuItem<Tray>>) -> MenuItem<Tray> {
    SubMenu { label: label.into(), submenu, ..Default::default() }.into()
}

fn action(label: &str, request: Request) -> MenuItem<Tray> {
    action_if(label, true, request)
}

fn action_if(label: &str, enabled: bool, request: Request) -> MenuItem<Tray> {
    StandardItem {
        label: label.into(),
        enabled,
        activate: Box::new(move |tray: &mut Tray| tray.daemon.run(request.clone())),
        ..Default::default()
    }
    .into()
}

/// A greyed-out line that only informs.
fn info(label: String) -> MenuItem<Tray> {
    StandardItem { label, enabled: false, ..Default::default() }.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_setups::TemplateSummary;

    fn labels(menu: &[MenuItem<Tray>]) -> Vec<String> {
        menu.iter()
            .map(|item| match item {
                MenuItem::SubMenu(sub) => format!("{} ▸", sub.label),
                MenuItem::Standard(item) if item.enabled => item.label.clone(),
                MenuItem::Standard(item) => format!("({})", item.label),
                _ => "-".into(),
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

    #[test]
    fn menu_follows_the_concept() {
        let sub = |label: &str| format!("{label} ▸");
        assert_eq!(
            labels(&build_menu(&snapshot())),
            [
                sub(tr("Load template")),
                tr("Save workspace").to_string(),
                sub(tr("Update")),
                sub(tr("Delete")),
                "-".to_string(),
                tr("Inbox").to_string(),
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
    fn load_here_only_on_an_empty_workspace() {
        let load_items = |snapshot: &Snapshot| {
            let menu = build_menu(snapshot);
            let MenuItem::SubMenu(load) = &menu[0] else { panic!("load submenu") };
            let MenuItem::SubMenu(template) = &load.submenu[0] else { panic!("template submenu") };
            labels(&template.submenu)
        };
        let (here, add, replace) = (tr("Load here"), tr("Add").to_string(), tr("Replace").to_string());
        assert_eq!(load_items(&snapshot()), [format!("({here})"), add.clone(), replace.clone()]);
        let empty = Snapshot { workspace_windows: 0, ..snapshot() };
        assert_eq!(load_items(&empty), [here.to_string(), add, replace]);
    }

    #[test]
    fn unsupported_system_shows_why() {
        let snapshot = Snapshot { unsupported: Some("AeroSpace (macOS)".into()), ..Snapshot::default() };
        let menu = labels(&build_menu(&snapshot));
        assert_eq!(menu[0], format!("({})", trf("{name} not supported yet", &[("name", &"AeroSpace (macOS)")])));
        assert_eq!(menu.last().map(String::as_str), Some(tr("Quit")));
    }

    fn line(title: &str, source: &str) -> InboxLine {
        InboxLine { id: format!("id-{title}"), title: title.into(), source: source.into() }
    }

    /// Task E3.3b: one number for everything "for me", approvals included.
    #[test]
    fn the_inbox_leads_the_menu_with_one_number() {
        let none = Waiting::of(&snapshot());
        assert_eq!(none, Waiting::default());
        assert_eq!((item_status(none), waiting_line(none)), (ksni::Status::Active, String::new()));
        assert_eq!(labels(&build_menu(&snapshot()))[0], format!("{} ▸", tr("Load template")));

        // An approval and two notices; the snapshot still knows the approval apart, the tray not.
        let waiting = Snapshot {
            approvals_waiting: 1,
            inbox_for_me: 3,
            inbox_urgent: 1,
            inbox_newest: vec![line("Delete event", "Suite"), line("Backup failed", "backup")],
            ..snapshot()
        };
        let three = Waiting::of(&waiting);
        assert_eq!((three, three.total()), (Waiting { for_me: 3, urgent: 1 }, 3), "approvals are not counted twice");
        let menu = build_menu(&waiting);
        assert_eq!(
            labels(&menu)[..5],
            [
                trf("Inbox ({n})", &[("n", &3)]),
                "Delete event · Suite".to_string(),
                "Backup failed · backup".to_string(),
                trf("Open {app}", &[("app", &tr("Inbox"))]),
                "-".to_string(),
            ]
        );
        assert!(
            menu[..4].iter().all(|item| matches!(item, MenuItem::Standard(entry) if entry.enabled)),
            "every inbox line can be clicked"
        );
        assert_eq!(labels(&menu)[5], format!("{} ▸", tr("Load template")), "the rest of the menu follows unchanged");
        assert!(!labels(&menu).contains(&trf("Approvals ({n})", &[("n", &1)])), "no second number for approvals");
        assert_eq!(item_status(three), ksni::Status::NeedsAttention);
        assert_eq!(waiting_line(three), trf("{n} for you", &[("n", &3)]));
    }

    #[test]
    fn only_something_urgent_makes_the_icon_stand_out() {
        let calm = Waiting { for_me: 4, urgent: 0 };
        assert_eq!(item_status(calm), ksni::Status::Active, "notices count but stay calm");
        assert_eq!(waiting_line(calm), trf("{n} for you", &[("n", &4)]));
        assert_eq!(item_status(Waiting { for_me: 4, urgent: 1 }), ksni::Status::NeedsAttention);
    }

    /// Titles come from agents and scripts: shown as written, never as a mnemonic, a line break
    /// or turned text, and short enough for the bar.
    #[test]
    fn foreign_titles_show_as_plain_text() {
        let label = |title: &str, source: &str| entry_label(&line(title, source));
        assert_eq!(label("fix_build", "my_script"), "fix__build · my__script", "underscores show as themselves");
        assert_eq!(label("two\nlines\there", "x"), "two lines here · x");
        assert_eq!(label("\u{202E}gnp.exe", "x"), "gnp.exe · x", "no turned text");
        assert_eq!(label("<b>bold</b> &amp;", "x"), "<b>bold</b> &amp; · x", "markup stays text");
        let long = "A very long title from an agent that goes on";
        let cut: String = long.chars().take(TITLE_IN_MENU - 1).collect();
        assert_eq!(label(long, "x"), format!("{cut}… · x"));
    }

    #[test]
    fn left_click_opens_the_menu() {
        const { assert!(<Tray as ksni::Tray>::MENU_ON_ACTIVATE) };
    }

    #[test]
    fn icon_comes_as_pixmaps() {
        let sizes: Vec<usize> = icon::SIZES.iter().map(|&s| icon::argb32(s).len()).collect();
        assert_eq!(sizes, icon::SIZES.iter().map(|&s| (s * s * 4) as usize).collect::<Vec<_>>());
    }
}
