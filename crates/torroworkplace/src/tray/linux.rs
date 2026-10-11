//! Tray icon for Linux, built on ksni. This is the first attempt (task 1.1): a fixed menu
//! whose clicks only name the entry on stderr. "Beenden" ends the daemon. Task 1.19 wires
//! the menu to the real state. The icon is the Torro symbol from `icon.rs` (task 1.2).

use std::fmt;

use ksni::TrayMethods; // provides `spawn`
use ksni::menu::{MenuItem, StandardItem, SubMenu};
use tokio::sync::mpsc;

use super::icon;

/// Errors of the tray icon.
#[derive(Debug)]
pub enum TrayError {
    /// The async runtime for the tray could not be built.
    Runtime(std::io::Error),
    /// The tray could not be registered on D-Bus or shown by the desktop.
    Spawn(ksni::Error),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrayError::Runtime(err) => write!(f, "cannot start the async runtime: {err}"),
            TrayError::Spawn(err) => write!(f, "cannot show the tray icon: {err}"),
        }
    }
}

impl std::error::Error for TrayError {}

/// The tray state. It holds no data yet, only the channel that ends the daemon.
struct Tray {
    /// Receives one message when the user picks "Beenden".
    quit: mpsc::UnboundedSender<()>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "TorroWorkplace".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        icon::SIZES
            .iter()
            .map(|&size| ksni::Icon { width: size as i32, height: size as i32, data: icon::argb32(size) })
            .collect()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let quit = self.quit.clone();
        vec![
            SubMenu {
                label: "Vorlage laden".into(),
                submenu: vec![
                    SubMenu {
                        label: "Beispiel".into(),
                        submenu: vec![entry("Hier laden"), entry("Hinzufügen"), entry("Ersetzen")],
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Beenden".into(),
                activate: Box::new(move |_| {
                    report("Beenden");
                    // The receiver lives in `run`; if it is gone, the daemon is already ending.
                    let _ = quit.send(());
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// A menu entry that only reports its name when clicked.
fn entry(label: &'static str) -> MenuItem<Tray> {
    StandardItem { label: label.into(), activate: Box::new(|_| report(label)), ..Default::default() }.into()
}

fn report(name: &str) {
    eprintln!("{name}");
}

/// Shows the tray icon and blocks until the user picks "Beenden".
pub fn run() -> Result<(), TrayError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(TrayError::Runtime)?;
    runtime.block_on(async {
        let (quit, mut quit_rx) = mpsc::unbounded_channel();
        // The handle keeps the tray service running. Dropping it removes the icon.
        let _handle = Tray { quit }.spawn().await.map_err(TrayError::Spawn)?;
        quit_rx.recv().await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _; // brings `menu` into scope

    #[test]
    fn menu_offers_the_example_submenu_and_quit() {
        let (quit, _rx) = mpsc::unbounded_channel();
        let menu = Tray { quit }.menu();
        let top: Vec<&str> = menu
            .iter()
            .map(|item| match item {
                MenuItem::SubMenu(sub) => sub.label.as_str(),
                MenuItem::Standard(item) => item.label.as_str(),
                _ => "-",
            })
            .collect();
        assert_eq!(top, ["Vorlage laden", "-", "Beenden"]);
    }

    #[test]
    fn icon_comes_as_pixmaps_without_icon_name() {
        let (quit, _rx) = mpsc::unbounded_channel();
        let tray = Tray { quit };
        assert_eq!(tray.icon_name(), "");
        let sizes: Vec<(i32, i32)> = tray.icon_pixmap().iter().map(|icon| (icon.width, icon.height)).collect();
        let expected: Vec<(i32, i32)> = icon::SIZES.iter().map(|&s| (s as i32, s as i32)).collect();
        assert_eq!(sizes, expected);
    }
}
