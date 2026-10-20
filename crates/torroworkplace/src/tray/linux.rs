//! Tray icon for Linux, built on ksni (StatusNotifierItem over D-Bus; Omarchy's bar,
//! Waybar and KDE show it). It only draws the shared menu from `menu.rs`, built from a
//! [`Snapshot`] of the core; clicks run the entries' actions on the [`Daemon`]. The icon comes
//! from `icon.rs`: always the red grid, whatever design pack is chosen (issue #114).
//!
//! What waits for the person ([`Waiting`], task E3.3b) shows in two ksni-only places: the
//! tooltip says how many ("3 for you", [`menu::waiting_line`]), never a title, since bars may
//! read the description as markup; and the item status is `NeedsAttention`, which bars use to
//! highlight the icon, only while something urgent waits (an approval, an agent waiting); notices
//! and finished agents count but stay calm.
//!
//! A number drawn onto the icon is left out: the protocol's overlay icon is only another pixmap,
//! so the digits would have to be rasterised here, and many bars ignore it.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};

use super::icon;
use super::menu::{self, Item, Waiting};
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
    /// The pixmaps for the voice mark they were drawn for (`Snapshot::voice`). ksni asks for them
    /// on every update to see whether the icon changed; they are drawn again only when it did.
    icon: Mutex<Option<(icon::Mark, Vec<ksni::Icon>)>>,
}

/// Shows the icon. Clicks go to `daemon`; [`TrayHandle::update`] replaces the snapshot, and with
/// it the count of what waits.
pub async fn spawn(daemon: Arc<Daemon>, snapshot: Snapshot) -> Result<TrayHandle, TrayError> {
    Tray { snapshot, daemon, icon: Mutex::new(None) }.spawn().await.map_err(TrayError::Spawn)
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
        let lines =
            [menu::voice_line(self.snapshot.voice).to_string(), menu::waiting_line(Waiting::of(&self.snapshot))];
        let description = lines.into_iter().filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" · ");
        ksni::ToolTip { title: "Torro Workplace".into(), description, ..Default::default() }
    }

    /// The red grid (issue #114) with the mark of the voice listener (task C.9).
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let wanted = icon::Mark::of(self.snapshot.voice);
        let mut cached = self.icon.lock().unwrap_or_else(PoisonError::into_inner);
        match cached.as_ref() {
            Some((drawn_for, drawn)) if *drawn_for == wanted => drawn.clone(),
            _ => {
                let drawn = pixmaps(&icon::Look::standard().marked(wanted));
                *cached = Some((wanted, drawn.clone()));
                drawn
            }
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        build_menu(&self.snapshot)
    }
}

/// The icon in every size of [`icon::SIZES`].
fn pixmaps(look: &icon::Look) -> Vec<ksni::Icon> {
    icon::SIZES
        .iter()
        .map(|&size| ksni::Icon { width: size as i32, height: size as i32, data: look.argb32(size) })
        .collect()
}

/// `NeedsAttention` while something urgent waits (an approval, an agent waiting), else `Active`.
/// Notices and finished agents count in the number but do not make the icon stand out: the tray
/// stays calm for what can wait.
fn item_status(waiting: Waiting) -> ksni::Status {
    if waiting.urgent > 0 { ksni::Status::NeedsAttention } else { ksni::Status::Active }
}

/// The menu from docs/core.md for one snapshot ([`menu::build`]), as ksni items.
fn build_menu(snapshot: &Snapshot) -> Vec<MenuItem<Tray>> {
    menu::build(snapshot).into_iter().map(ksni_item).collect()
}

/// One item of the shared menu as ksni draws it. An entry without an action is greyed out.
fn ksni_item(item: Item) -> MenuItem<Tray> {
    match item {
        Item::Entry { label, action } => StandardItem {
            label: label_text(&label),
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
            let label = label_text(&label);
            SubMenu { label, submenu: items.into_iter().map(ksni_item).collect(), ..Default::default() }.into()
        }
        Item::Separator => MenuItem::Separator,
    }
}

/// `label` as DBusMenu shows it as written: it takes an underscore as the mark of an access key,
/// so each one is doubled. Template names may hold any (`fix_build`).
fn label_text(label: &str) -> String {
    label.replace('_', "__")
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

    /// Where the template entries start: after the six apps and a separator.
    const TEMPLATES: usize = 7;

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
                shortcut: None,
                startup: None,
                workspace: "3".into(),
                monitor: None,
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
                tr("Notification Hub").to_string(),
                tr("Mail").to_string(),
                tr("App Feed").to_string(),
                tr("Calendar").to_string(),
                tr("Tasks").to_string(),
                tr("Assistant").to_string(),
                "-".to_string(),
                format!("({})", trf("Workspaces · {manager}", &[("manager", &"Hyprland")])),
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

    #[test]
    fn a_template_offers_load_and_replace_windows_on_any_workspace() {
        let load_items = |snapshot: &Snapshot| {
            let menu = build_menu(snapshot);
            // The templates follow the line that names the window manager (task WS.7).
            let MenuItem::SubMenu(template) = &menu[TEMPLATES + 1] else { panic!("template submenu") };
            labels(&template.submenu)
        };
        let expected = [tr("Load").to_string(), tr("Replace windows").to_string(), "-".into(), tr("Edit…").to_string()];
        assert_eq!(load_items(&snapshot()), expected);
        let empty = Snapshot { workspace_windows: 0, ..snapshot() };
        assert_eq!(load_items(&empty), expected);
    }

    #[test]
    fn unsupported_system_shows_why() {
        let snapshot = Snapshot { unsupported: Some("AeroSpace (macOS)".into()), ..Snapshot::default() };
        let menu = labels(&build_menu(&snapshot));
        assert_eq!(menu[TEMPLATES], format!("({})", trf("{name} not found", &[("name", &"AeroSpace (macOS)")])));
        assert_eq!(menu.last().map(String::as_str), Some(tr("Quit")));
    }

    /// The shared model as the same text as [`labels`], recursively, to compare with ksni.
    fn model_tree(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .flat_map(|item| match item {
                Item::Submenu { label, items } => {
                    let mut lines = vec![format!("{} ▸", label_text(label))];
                    lines.extend(model_tree(items).into_iter().map(|line| format!("  {line}")));
                    lines
                }
                Item::Entry { label, action: Some(_) } => vec![label_text(label)],
                Item::Entry { label, action: None } => vec![format!("({})", label_text(label))],
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
        let waiting = Snapshot { inbox_for_me: 1, ..snapshot() };
        for snapshot in [snapshot(), Snapshot { workspace_windows: 0, ..snapshot() }, Snapshot::default(), waiting] {
            assert_eq!(ksni_tree(&build_menu(&snapshot)), model_tree(&menu::build(&snapshot)));
        }
    }

    #[test]
    fn only_something_urgent_makes_the_icon_stand_out() {
        assert_eq!(item_status(Waiting::default()), ksni::Status::Active);
        assert_eq!(item_status(Waiting { for_me: 4, urgent: 0 }), ksni::Status::Active, "notices count but stay calm");
        assert_eq!(item_status(Waiting { for_me: 4, urgent: 1 }), ksni::Status::NeedsAttention);
    }

    /// The count next to "Inbox" reaches the bar, and underscores show as themselves instead of
    /// as access keys.
    #[test]
    fn the_inbox_count_reaches_the_ksni_menu_and_underscores_show() {
        let waiting = Snapshot { inbox_for_me: 2, inbox_urgent: 1, ..snapshot() };
        assert!(labels(&build_menu(&waiting)).contains(&trf("Notification Hub ({n})", &[("n", &2)])));
        assert_eq!(label_text("fix_build"), "fix__build");
        assert_eq!(label_text("Mail_Chat"), "Mail__Chat");
    }

    #[test]
    fn left_click_opens_the_menu() {
        const { assert!(<Tray as ksni::Tray>::MENU_ON_ACTIVATE) };
    }

    #[test]
    fn icon_comes_as_pixmaps() {
        let sizes: Vec<usize> = pixmaps(&icon::Look::standard()).iter().map(|icon| icon.data.len()).collect();
        assert_eq!(sizes, icon::SIZES.iter().map(|&s| (s * s * 4) as usize).collect::<Vec<_>>());
    }
}
