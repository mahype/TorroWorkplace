//! Tray icon for Linux, built on ksni (StatusNotifierItem over D-Bus; Omarchy's bar,
//! Waybar and KDE show it). The menu is built from a [`Snapshot`] of the core and turns
//! clicks into the same [`Request`]s the socket takes. The icon comes from `icon.rs`.

use std::fmt;
use std::sync::Arc;

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
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

/// The tray: the last snapshot plus the way back into the daemon.
pub struct Tray {
    pub snapshot: Snapshot,
    daemon: Arc<Daemon>,
}

/// Shows the icon. Clicks go to `daemon`; [`TrayHandle::update`] replaces the snapshot.
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

/// The menu from docs/core.md for one snapshot.
///
/// Labels stay short: Omarchy's bar draws tray menus about 232 px wide and cuts longer
/// text off with an ellipsis.
fn build_menu(snapshot: &Snapshot) -> Vec<MenuItem<Tray>> {
    let mut menu = Vec::new();
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
    menu.push(info(info_line(snapshot)));
    menu.push(MenuItem::Separator);
    menu.push(
        StandardItem {
            label: tr("Quit").into(),
            activate: Box::new(|tray: &mut Tray| tray.daemon.quit()),
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
                format!(
                    "({})",
                    trf("Workspace {workspace} · {backend}", &[("workspace", &"3"), ("backend", &"hyprland")])
                ),
                "-".to_string(),
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
