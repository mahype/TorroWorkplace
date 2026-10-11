//! The suite's apps as the core reports them (`Request::ListApps`): every app of
//! `torro_core::AppId::ALL` except the core itself, and whether it is installed.
//!
//! "Installed" means the binary was found next to the core's executable or on `PATH`, the same
//! places `Request::OpenApp` starts apps from, so a listed app that is installed can be opened.

use std::path::{Path, PathBuf};

use torro_core::AppId;
use torro_ipc::AppInfo;

/// The suite's apps with their install state, looked up now (a handful of file checks).
pub fn list() -> Vec<AppInfo> {
    list_in(&search_dirs())
}

/// [`list`] with the folders to look in; tests pass temporary ones.
fn list_in(dirs: &[PathBuf]) -> Vec<AppInfo> {
    AppId::ALL
        .iter()
        // The core is the service answering this; it is not one of the apps it opens.
        .filter(|app| **app != AppId::WORKPLACE)
        .map(|app| AppInfo {
            binary: app.binary.to_string(),
            name: app.display_name.to_string(),
            window_class: app.window_class.to_string(),
            installed: is_installed(app.binary, dirs),
        })
        .collect()
}

/// The folders an app binary is looked for in: the folder of this executable, then `PATH`.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        dirs.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// Whether a file named `binary` (plus `.exe` on Windows) exists in one of `dirs`.
fn is_installed(binary: &str, dirs: &[PathBuf]) -> bool {
    let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().any(|dir| dir.join(&file).is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn binary(name: &str) -> String {
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    }

    #[test]
    fn detects_installed_apps_in_the_given_dirs() {
        let next_to_exe = tempfile::tempdir().expect("temp dir");
        let on_path = tempfile::tempdir().expect("temp dir");
        let empty = tempfile::tempdir().expect("temp dir");
        fs::write(on_path.path().join(binary("torrocalendar")), b"").expect("write binary");
        // A directory with the right name is not an installed binary.
        fs::create_dir(next_to_exe.path().join(binary("torrotasks"))).expect("create dir");

        let dirs = vec![next_to_exe.path().to_path_buf(), empty.path().to_path_buf(), on_path.path().to_path_buf()];
        assert!(is_installed("torrocalendar", &dirs));
        assert!(!is_installed("torrotasks", &dirs));
        assert!(!is_installed("torrocalendar", &[empty.path().to_path_buf()]));
        assert!(!is_installed("torrocalendar", &[]));
    }

    #[test]
    fn lists_every_app_but_the_core_with_its_state() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(binary("torrotasks")), b"").expect("write binary");
        let apps = list_in(&[dir.path().to_path_buf()]);
        assert_eq!(apps.len(), AppId::ALL.len() - 1);
        assert!(apps.iter().all(|app| app.binary != AppId::WORKPLACE.binary));
        let tasks = apps.iter().find(|app| app.binary == "torrotasks").expect("tasks listed");
        assert_eq!(
            tasks,
            &AppInfo {
                binary: "torrotasks".into(),
                name: AppId::TASKS.display_name.into(),
                window_class: AppId::TASKS.window_class.into(),
                installed: true,
            }
        );
        let calendar = apps.iter().find(|app| app.binary == "torrocalendar").expect("calendar listed");
        assert!(!calendar.installed);
    }
}
