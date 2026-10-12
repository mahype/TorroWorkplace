//! Tray icon for Linux, built on ksni (StatusNotifierItem over D-Bus; Omarchy's bar,
//! Waybar and KDE show it). It only draws the shared menu from `menu.rs`, built from a
//! [`Snapshot`] of the core; clicks run the entries' actions on the [`Daemon`]. The icon comes
//! from `icon.rs`.

use std::fmt;
use std::sync::Arc;

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};

use super::icon;
use super::menu::{self, Item};
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

/// The menu from docs/core.md for one snapshot ([`menu::build`]), as ksni items.
fn build_menu(snapshot: &Snapshot) -> Vec<MenuItem<Tray>> {
    menu::build(snapshot).into_iter().map(ksni_item).collect()
}

/// One item of the shared menu as ksni draws it. An entry without an action is greyed out.
fn ksni_item(item: Item) -> MenuItem<Tray> {
    match item {
        Item::Entry { label, action } => StandardItem {
            label,
            enabled: action.is_some(),
            activate: Box::new(move |tray: &mut Tray| {
                if let Some(action) = &action {
                    action.perform(&tray.daemon);
                }
            }),
            ..Default::default()
        }
        .into(),
        Item::Submenu { label, items } => {
            SubMenu { label, submenu: items.into_iter().map(ksni_item).collect(), ..Default::default() }.into()
        }
        Item::Separator => MenuItem::Separator,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::i18n::{tr, trf};
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

    /// The shared model as the same text as [`labels`], recursively, to compare with ksni.
    fn model_tree(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .flat_map(|item| match item {
                Item::Submenu { label, items } => {
                    let mut lines = vec![format!("{label} ▸")];
                    lines.extend(model_tree(items).into_iter().map(|line| format!("  {line}")));
                    lines
                }
                Item::Entry { label, action: Some(_) } => vec![label.clone()],
                Item::Entry { label, action: None } => vec![format!("({label})")],
                Item::Separator => vec!["-".into()],
            })
            .collect()
    }

    fn ksni_tree(items: &[MenuItem<Tray>]) -> Vec<String> {
        items
            .iter()
            .flat_map(|item| match item {
                MenuItem::SubMenu(sub) => {
                    let mut lines = vec![format!("{} ▸", sub.label)];
                    lines.extend(ksni_tree(&sub.submenu).into_iter().map(|line| format!("  {line}")));
                    lines
                }
                other => labels(std::slice::from_ref(other)),
            })
            .collect()
    }

    #[test]
    fn ksni_draws_the_shared_menu_entry_for_entry() {
        for snapshot in [snapshot(), Snapshot { workspace_windows: 0, ..snapshot() }, Snapshot::default()] {
            assert_eq!(ksni_tree(&build_menu(&snapshot)), model_tree(&menu::build(&snapshot)));
        }
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
