//! Tray icon for Linux, built on ksni (StatusNotifierItem over D-Bus; Omarchy's bar,
//! Waybar and KDE show it). The menu is built from a [`Snapshot`] of the core and turns
//! clicks into the same [`Request`]s the socket takes. The icon comes from `icon.rs`.
//!
//! What waits for the person ([`Waiting`], task A.8: the open approvals) shows as an entry
//! "Approvals (2)" at the top of the menu, which opens the control centre, as the tooltip, and as
//! the item status `NeedsAttention`, which bars use to highlight the icon. A number drawn onto the
//! icon is left out: the protocol's overlay icon is only another pixmap, so the digits would have to
//! be rasterised here, and many bars ignore it. The count comes from `approvals.list_approvals`
//! every few seconds ([`watch_waiting`]), because the snapshot the daemon hands over carries no
//! approvals and a client's request that opens one does not refresh the menu.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_ipc::{ApprovalInfo, Request, Response, client};
use torro_pim::AccountState;
use torro_setups::LoadMode;

use super::icon;
use crate::core::Snapshot;
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

/// The tray: the last snapshot, what waits for the person, and the way back into the daemon.
pub struct Tray {
    pub snapshot: Snapshot,
    waiting: Waiting,
    daemon: Arc<Daemon>,
}

/// What waits for a decision of the person, counted in the tray. Today the open approvals; the
/// inbox (task E3.3b) adds its own count here, and [`Waiting::total`] and the menu entry merge
/// them into one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Waiting {
    /// Approvals still open (`ApprovalState::Pending`).
    pub approvals: usize,
}

impl Waiting {
    /// Everything that waits, as one number.
    pub fn total(self) -> usize {
        self.approvals
    }
}

/// How often the tray asks the core what waits. The desktop notification announces a new approval
/// at once; the count is the reminder, so a few seconds late is fine.
const WAITING_EVERY: Duration = Duration::from_secs(5);

/// Shows the icon. Clicks go to `daemon`; [`TrayHandle::update`] replaces the snapshot. The count
/// of what waits follows by itself ([`watch_waiting`]) until the tray is closed.
pub async fn spawn(daemon: Arc<Daemon>, snapshot: Snapshot) -> Result<TrayHandle, TrayError> {
    let handle = Tray { snapshot, waiting: Waiting::default(), daemon }.spawn().await.map_err(TrayError::Spawn)?;
    if let Some(socket) = torro_ipc::socket_path() {
        tokio::spawn(watch_waiting(handle.clone(), socket));
    }
    Ok(handle)
}

/// Asks the core at `socket` every [`WAITING_EVERY`] what waits and shows it when it changed. It
/// goes through the socket with the app key like the control centre, so the core stays the only
/// one who reads `approvals.json`. A core that does not answer keeps the last count; the loop ends
/// with the tray.
async fn watch_waiting(handle: TrayHandle, socket: std::path::PathBuf) {
    let mut shown = Waiting::default();
    let mut every = tokio::time::interval(WAITING_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        if handle.is_closed() {
            return;
        }
        let Some(waiting) = fetch_waiting(&socket).await else { continue };
        if waiting == shown {
            continue;
        }
        if handle.update(|tray| tray.waiting = waiting).await.is_none() {
            return;
        }
        shown = waiting;
    }
}

/// What waits, from the core's answer to `approvals.list_approvals`; `None` when it does not answer.
async fn fetch_waiting(socket: &Path) -> Option<Waiting> {
    match client::call(socket, &Request::ListApprovals).await {
        Ok(Response::Approvals { approvals }) => Some(Waiting { approvals: open_approvals(&approvals) }),
        _ => None,
    }
}

/// The approvals still waiting for a person.
fn open_approvals(approvals: &[ApprovalInfo]) -> usize {
    approvals.iter().filter(|approval| approval.state.is_pending()).count()
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

    /// Bars highlight an item that needs attention; nothing waiting is the normal, active state.
    fn status(&self) -> ksni::Status {
        item_status(self.waiting)
    }

    /// The name and, when something waits, how much. Never a summary: the description may be
    /// read as markup by the bar, and summaries hold text a client sent.
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Torro Workplace".into(), description: waiting_line(self.waiting), ..Default::default() }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        icon::SIZES
            .iter()
            .map(|&size| ksni::Icon { width: size as i32, height: size as i32, data: icon::argb32(size) })
            .collect()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        build_menu(&self.snapshot, self.waiting)
    }
}

/// `NeedsAttention` while something waits, else `Active`.
fn item_status(waiting: Waiting) -> ksni::Status {
    if waiting.total() > 0 { ksni::Status::NeedsAttention } else { ksni::Status::Active }
}

/// "2 requests wait for your approval", or nothing when none waits.
fn waiting_line(waiting: Waiting) -> String {
    match waiting.total() {
        0 => String::new(),
        1 => tr("1 request waits for your approval").into(),
        n => trf("{n} requests wait for your approval", &[("n", &n)]),
    }
}

/// The menu from docs/core.md for one snapshot, with "Approvals (n)" on top while something
/// waits.
///
/// Labels stay short: Omarchy's bar draws tray menus about 232 px wide and cuts longer
/// text off with an ellipsis.
fn build_menu(snapshot: &Snapshot, waiting: Waiting) -> Vec<MenuItem<Tray>> {
    let mut menu = Vec::new();
    if waiting.total() > 0 {
        menu.push(
            StandardItem {
                label: trf("Approvals ({n})", &[("n", &waiting.total())]),
                // The control centre comes to the front, or starts; its sidebar counts the open
                // approvals too.
                activate: Box::new(|tray: &mut Tray| tray.daemon.open_app(AppId::WORKPLACE)),
                ..Default::default()
            }
            .into(),
        );
        menu.push(MenuItem::Separator);
    }
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
            labels(&build_menu(&snapshot(), Waiting::default())),
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
            let menu = build_menu(snapshot, Waiting::default());
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
        let menu = labels(&build_menu(&snapshot, Waiting::default()));
        assert_eq!(menu[0], format!("({})", trf("{name} not supported yet", &[("name", &"AeroSpace (macOS)")])));
        assert_eq!(menu.last().map(String::as_str), Some(tr("Quit")));
    }

    #[test]
    fn waiting_approvals_lead_the_menu_and_mark_the_item() {
        let none = Waiting::default();
        assert_eq!((item_status(none), waiting_line(none)), (ksni::Status::Active, String::new()));
        assert_eq!(labels(&build_menu(&snapshot(), none))[0], format!("{} ▸", tr("Load template")));

        let two = Waiting { approvals: 2 };
        let menu = labels(&build_menu(&snapshot(), two));
        assert_eq!(menu[..2], [trf("Approvals ({n})", &[("n", &2)]), "-".to_string()]);
        assert_eq!(menu[2], format!("{} ▸", tr("Load template")), "the rest of the menu follows unchanged");
        assert_eq!(item_status(two), ksni::Status::NeedsAttention);
        assert_eq!(waiting_line(two), trf("{n} requests wait for your approval", &[("n", &2)]));
        assert_eq!(waiting_line(Waiting { approvals: 1 }), tr("1 request waits for your approval"));
    }

    #[test]
    fn only_open_approvals_wait() {
        use torro_ipc::ApprovalState;
        let approval = |state: ApprovalState| ApprovalInfo {
            id: "9c1f0a7e3b2d4c58".into(),
            client: "claude".into(),
            client_name: "Claude Code".into(),
            command: "calendar.delete_event".into(),
            summary: "Delete event".into(),
            created: chrono::Utc::now(),
            expires: chrono::Utc::now(),
            decided: None,
            state,
        };
        let approvals =
            [approval(ApprovalState::Pending), approval(ApprovalState::Denied), approval(ApprovalState::Pending)];
        assert_eq!(open_approvals(&approvals), 2);
        assert_eq!(open_approvals(&[approval(ApprovalState::Expired)]), 0);
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
