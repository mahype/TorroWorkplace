//! What of the suite is installed on this computer, in which version, and whether a package manager
//! looks after it (issue #411, decision 1).
//!
//! On macOS an app is its bundle (`Torro Calendar.app/Contents/MacOS/torrocalendar`) in one of the
//! folders the suite looks in ([`torro_gui::source::bundle_folders`]); its version is the one in the
//! bundle's `Info.plist` (`TorroVersion`, else `CFBundleShortVersionString`). The main application is
//! `Torro Workplace.app`. Elsewhere an app is its program ([`AppId::program_file`]) beside the service or on
//! `PATH`, with the version its `--version` prints (at most [`VERSION_WAIT`]); the main application is
//! installed as [`crate::apps::main_app_installed`] tells.
//!
//! Who installed a program is read from its path ([`manager_of`], Linux and Windows only): `/usr/bin`
//! with pacman is the AUR, a Homebrew cellar Homebrew, a Scoop folder Scoop. For an app not installed
//! the service's own path decides: the catalog installs as the service came.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use torro_core::AppId;
use torro_core::ai::BoxFuture;
use torro_core::version::Version;
use torro_ipc::ManagedBy;

use super::Platform;

/// How long a program may take to print its version.
pub const VERSION_WAIT: Duration = Duration::from_secs(2);

/// Reads the version a program prints with `--version`.
pub type VersionOf = Arc<dyn Fn(PathBuf) -> BoxFuture<'static, Option<String>> + Send + Sync>;

/// Moves a file or folder away for good (to the trash).
pub type Discard = Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

/// Where the catalog looks and installs, and the few things it asks the system. Every part is a value,
/// so tests build one in temporary folders for any [`Platform`].
#[derive(Clone)]
pub struct Env {
    pub platform: Platform,
    /// [`std::env::consts::ARCH`]: which archives of the release fit.
    pub arch: String,
    /// The service's program ([`std::env::current_exe`]).
    pub exe: Option<PathBuf>,
    /// The person's home folder; on macOS `~/Applications` lies in it.
    pub home: Option<PathBuf>,
    /// macOS: the folders bundles are looked for in ([`torro_gui::source::bundle_folders`]).
    pub bundle_folders: Vec<PathBuf>,
    /// Linux, Windows: the folders programs are looked for in, the service's own first
    /// ([`crate::apps::search_dirs`]).
    pub dirs: Vec<PathBuf>,
    /// Linux: `$XDG_DATA_HOME` (else `~/.local/share`), for `.desktop` entries and icons.
    pub data_home: Option<PathBuf>,
    /// Windows: the person's Start menu programs (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`).
    pub start_menu: Option<PathBuf>,
    /// Linux, Windows: whether the main application is installed ([`crate::apps::main_app_installed`]).
    pub main_app: Arc<dyn Fn() -> bool + Send + Sync>,
    pub version_of: VersionOf,
    /// The folders of an app's own data (`remove_data`): [`torro_core::paths::AppPaths::for_app`].
    pub app_dirs: Arc<dyn Fn(AppId) -> Vec<PathBuf> + Send + Sync>,
    /// Moves a file or folder away for good: to the system's trash (`trash`).
    pub discard: Discard,
}

impl Env {
    /// This computer, as the service runs on it.
    pub fn system() -> Env {
        let platform = Platform::CURRENT;
        let exe = std::env::current_exe().ok();
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = var(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let bundle_folders = match platform {
            Platform::MacOs => torro_gui::source::bundle_folders(exe.as_deref(), home.as_deref()),
            _ => Vec::new(),
        };
        let data_home = match platform {
            Platform::Linux => var("XDG_DATA_HOME").or_else(|| home.as_ref().map(|home| home.join(".local/share"))),
            _ => None,
        };
        let start_menu = match platform {
            Platform::Windows => var("APPDATA").map(|roaming| roaming.join("Microsoft\\Windows\\Start Menu\\Programs")),
            _ => None,
        };
        Env {
            platform,
            arch: std::env::consts::ARCH.into(),
            exe,
            home,
            bundle_folders,
            dirs: crate::apps::search_dirs(),
            data_home,
            start_menu,
            main_app: Arc::new(crate::apps::main_app_installed),
            version_of: Arc::new(run_version),
            app_dirs: Arc::new(own_dirs),
            discard: Arc::new(|path: &Path| trash::delete(path).map_err(std::io::Error::other)),
        }
    }

    /// Nowhere: finds nothing, removes nothing.
    pub fn empty(platform: Platform) -> Env {
        Env {
            platform,
            arch: std::env::consts::ARCH.into(),
            exe: None,
            home: None,
            bundle_folders: Vec::new(),
            dirs: Vec::new(),
            data_home: None,
            start_menu: None,
            main_app: Arc::new(|| false),
            version_of: Arc::new(|_| Box::pin(async { None })),
            app_dirs: Arc::new(|_| Vec::new()),
            discard: Arc::new(|_: &Path| Err(std::io::Error::other("nothing is removed here"))),
        }
    }
}

/// The folders of `app`'s own files ([`torro_core::paths::AppPaths::for_app`]), each once; none for the
/// main application, whose folder holds the accounts of the whole suite.
fn own_dirs(app: AppId) -> Vec<PathBuf> {
    if app == AppId::WORKPLACE {
        return Vec::new();
    }
    let Some(paths) = torro_core::paths::AppPaths::for_app(app) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in [paths.config, paths.data, paths.state] {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The package managers whose installs the catalog leaves alone (decision 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Aur,
    Homebrew,
    Scoop,
    Deb,
    Rpm,
    Pacman,
    Setup,
    /// An unreadable or inconsistent native marker still protects its files.
    Invalid,
}

impl Manager {
    /// As people know it.
    pub fn name(self) -> &'static str {
        match self {
            Manager::Aur => "AUR",
            Manager::Homebrew => "Homebrew",
            Manager::Scoop => "Scoop",
            Manager::Deb => "DEB",
            Manager::Rpm => "RPM",
            Manager::Pacman => "pacman",
            Manager::Setup => "Windows Setup",
            Manager::Invalid => "native installer",
        }
    }

    /// Its commands for `app`, from the suite's package names only (issue #411, step 7 of the plan):
    /// the AUR has `<program>-bin` per program and `torro-workplace` for the main application, the tap
    /// `mahype/torro` and the Scoop bucket one name per program.
    pub fn commands(self, app: AppId) -> ManagedBy {
        let program = app.program_file();
        let (install, update, uninstall) = match self {
            Manager::Aur => {
                let package =
                    if app == AppId::WORKPLACE { "torro-workplace".to_string() } else { format!("{program}-bin") };
                (format!("yay -S {package}"), "yay -Syu".to_string(), format!("sudo pacman -R {package}"))
            }
            Manager::Homebrew => (
                format!("brew install mahype/torro/{program}"),
                format!("brew upgrade mahype/torro/{program}"),
                format!("brew uninstall mahype/torro/{program}"),
            ),
            Manager::Scoop => (
                format!("scoop install {program}"),
                format!("scoop update {program}"),
                format!("scoop uninstall {program}"),
            ),
            Manager::Deb | Manager::Rpm | Manager::Pacman | Manager::Setup | Manager::Invalid => {
                let downloads = "https://github.com/mahype/TorroWorkplace/releases/latest".to_string();
                let package = if app == AppId::WORKPLACE { "torro-workplace".into() } else { format!("{program}-bin") };
                let uninstall = match self {
                    Manager::Deb => format!("sudo apt remove {package}"),
                    Manager::Rpm => format!("sudo dnf remove {package}"),
                    Manager::Pacman => format!("sudo pacman -R {package}"),
                    Manager::Setup => "control appwiz.cpl".into(),
                    _ => downloads.clone(),
                };
                (downloads.clone(), downloads, uninstall)
            }
        };
        ManagedBy { manager: self.name().into(), install, update, uninstall }
    }
}

/// The package manager that installed the program at `path` on `platform`, read from the path
/// (decision 1): on Linux a Homebrew cellar (`/Cellar/`, `linuxbrew`) is Homebrew, `/usr/bin/` on a
/// system with `/usr/bin/pacman` (`exists`) the AUR; on Windows a Scoop folder (`\scoop\apps\`, or its
/// shims on `PATH`) Scoop. On macOS the catalog always installs itself.
pub fn manager_of(path: &Path, platform: Platform, exists: impl Fn(&Path) -> bool) -> Option<Manager> {
    let text = path.to_string_lossy();
    match platform {
        Platform::MacOs => None,
        Platform::Linux if text.contains("/Cellar/") || text.contains("linuxbrew") => Some(Manager::Homebrew),
        Platform::Linux if text.starts_with("/usr/bin/") && exists(Path::new("/usr/bin/pacman")) => Some(Manager::Aur),
        Platform::Linux => None,
        Platform::Windows => {
            let text = text.to_lowercase().replace('/', "\\");
            (text.contains("\\scoop\\apps\\") || text.contains("\\scoop\\shims\\")).then_some(Manager::Scoop)
        }
    }
}

/// [`manager_of`] the path as found and as it really is (a link of Homebrew's `bin` into its cellar).
pub(super) fn manager_on_disk(path: &Path, platform: Platform) -> Option<Manager> {
    if platform != Platform::MacOs
        && let Some(manager) = native_marker(path, platform)
    {
        return Some(manager);
    }
    let real = std::fs::canonicalize(path).ok();
    if platform != Platform::MacOs
        && let Some(manager) = real.as_deref().and_then(|real| native_marker(real, platform))
    {
        return Some(manager);
    }
    let exists = |path: &Path| path.is_file();
    manager_of(path, platform, exists).or_else(|| real.and_then(|real| manager_of(&real, platform, exists)))
}

/// Native installers own one marker beside each program. Never fall back to self-management when
/// a marker is broken: that would turn a damaged install into permission to overwrite it.
fn native_marker(path: &Path, platform: Platform) -> Option<Manager> {
    let marker = path.with_file_name(format!("{}.torro-package.json", path.file_name()?.to_string_lossy()));
    match std::fs::symlink_metadata(&marker) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return Some(Manager::Invalid),
        Ok(meta) if !meta.is_file() || meta.len() > 4096 => return Some(Manager::Invalid),
        Ok(_) => {}
    }
    let read = || -> Option<Manager> {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(marker).ok()?).ok()?;
        if value.get("schema")?.as_u64()? != 1 {
            return None;
        }
        let manager = match (platform, value.get("manager")?.as_str()?) {
            (Platform::Linux, "deb") => Manager::Deb,
            (Platform::Linux, "rpm") => Manager::Rpm,
            (Platform::Linux, "pacman") => Manager::Pacman,
            (Platform::Windows, "setup") => Manager::Setup,
            _ => return None,
        };
        let program = path.file_stem()?.to_str()?;
        let package = if program == "torroworkplace" { "torroworkplace-bin".into() } else { format!("{program}-bin") };
        let expected_package = if manager == Manager::Setup { program } else { package.as_str() };
        if value.get("package")?.as_str()? != expected_package {
            return None;
        }
        let declared = Path::new(value.get("program")?.as_str()?);
        // Canonical parents also protect a not-yet-installed target in an installer-owned folder.
        let canonical = |p: &Path| {
            std::fs::canonicalize(p)
                .ok()
                .or_else(|| Some(std::fs::canonicalize(p.parent()?).ok()?.join(p.file_name()?)))
        };
        (canonical(declared)? == canonical(path)?).then_some(manager)
    };
    Some(read().unwrap_or(Manager::Invalid))
}

/// A Setup owns its common directory too: the catalog must not sweep `.old` or put missing apps
/// beside its service. Linux ownership is per package/program, not an entire system bin directory.
pub(super) fn setup_directory(path: &Path, platform: Platform) -> bool {
    platform == Platform::Windows
        && path.parent().is_some_and(|dir| {
            manager_on_disk(&dir.join("torroworkplace.exe"), platform)
                .is_some_and(|manager| matches!(manager, Manager::Setup | Manager::Invalid))
        })
}

/// One installed app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// macOS: the bundle (`/Applications/Torro Calendar.app`); else the program.
    pub path: PathBuf,
    /// The version it has; `None` when it cannot be read.
    pub version: Option<String>,
    pub managed: Option<Manager>,
}

/// What [`scan`] found.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub found: Vec<(AppId, Found)>,
    /// The package manager the service came with ([`manager_of`] its program).
    pub origin: Option<Manager>,
}

impl Scan {
    pub fn found(&self, app: AppId) -> Option<&Found> {
        self.found.iter().find(|(known, _)| *known == app).map(|(_, found)| found)
    }

    /// Who looks after `app`: its package manager when installed, else the service's (decision 1).
    pub fn manager(&self, app: AppId) -> Option<Manager> {
        match self.found(app) {
            Some(found) => found.managed,
            None => self.origin,
        }
    }
}

/// The versions read from programs, by path, size and time of change: running six programs for every
/// look at the catalog is not needed while none changed.
#[derive(Default)]
pub struct Versions(Mutex<HashMap<PathBuf, (Stamp, String)>>);

/// When a program last changed and its size.
type Stamp = (Option<SystemTime>, u64);

impl Versions {
    async fn of(&self, path: PathBuf, version_of: &VersionOf) -> Option<String> {
        let stamp: Option<Stamp> = std::fs::metadata(&path).ok().map(|meta| (meta.modified().ok(), meta.len()));
        if let Some(stamp) = stamp {
            let known = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&path).cloned();
            if let Some((was, version)) = known
                && was == stamp
            {
                return Some(version);
            }
        }
        let version = version_of(path.clone()).await?;
        if let Some(stamp) = stamp {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(path, (stamp, version.clone()));
        }
        Some(version)
    }
}

/// The bundle `<title>.app` with its program `program` in the first of `env`'s bundle folders that has it.
pub fn bundle_path(env: &Env, title: &str, program: &str) -> Option<PathBuf> {
    env.bundle_folders
        .iter()
        .map(|folder| folder.join(format!("{title}.app")))
        .find(|bundle| bundle.join("Contents/MacOS").join(program).is_file())
}

/// The program `program` in the first of `env`'s folders that has it.
pub fn program_path(env: &Env, program: &str) -> Option<PathBuf> {
    let file = env.platform.program_file(program);
    env.dirs.iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
}

/// Where `app` is installed under `env`; `None` when it is not.
fn locate(env: &Env, app: AppId) -> Option<PathBuf> {
    match env.platform {
        Platform::MacOs => bundle_path(env, app.display_name, app.program_file()),
        Platform::Linux | Platform::Windows if app == AppId::WORKPLACE => {
            (env.main_app)().then(|| program_path(env, app.program_file()).or_else(|| env.exe.clone())).flatten()
        }
        Platform::Linux | Platform::Windows => program_path(env, app.program_file()),
    }
}

/// Every app of the suite that is installed under `env`, with its version and package manager.
pub async fn scan(env: &Env, versions: &Versions) -> Scan {
    let located: Vec<(AppId, PathBuf)> =
        AppId::ALL.iter().filter_map(|app| locate(env, *app).map(|path| (*app, path))).collect();
    let read = located.iter().map(|(_, path)| async {
        match env.platform {
            Platform::MacOs => {
                std::fs::read_to_string(path.join("Contents/Info.plist")).ok().and_then(|text| plist_version(&text))
            }
            Platform::Linux | Platform::Windows => versions.of(path.clone(), &env.version_of).await,
        }
    });
    let read = futures_util::future::join_all(read).await;
    let found = located
        .into_iter()
        .zip(read)
        .map(|((app, path), version)| {
            let managed = manager_on_disk(&path, env.platform);
            (app, Found { path, version, managed })
        })
        .collect();
    let origin = env.exe.as_deref().and_then(|exe| manager_on_disk(exe, env.platform));
    Scan { found, origin }
}

/// Runs `program --version` for at most [`VERSION_WAIT`] and reads the version it prints.
fn run_version(program: PathBuf) -> BoxFuture<'static, Option<String>> {
    Box::pin(async move {
        let mut command = tokio::process::Command::new(&program);
        command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        {
            // From `winbase.h`: no console window flashes up for the program.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = tokio::time::timeout(VERSION_WAIT, command.output()).await.ok()?.ok()?;
        version_from_output(&String::from_utf8_lossy(&output.stdout))
    })
}

/// The version in what a program of the suite prints with `--version` (`torrocalendar 0.1.0`): the last
/// word of the first line, when it is a version.
pub fn version_from_output(text: &str) -> Option<String> {
    let word = text.lines().next()?.split_whitespace().last()?;
    word.parse::<Version>().ok().map(|version| version.to_string())
}

/// The version of a bundle's `Info.plist` (XML), read as the apps read their service's copies
/// ([`torro_gui::source::plist_version`]: the suite's whole version, pre-release included).
pub fn plist_version(text: &str) -> Option<String> {
    torro_gui::source::plist_version(text).map(|version| version.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An `Info.plist` as `packaging/macos/make-app.sh` writes it, with `version`: Apple's keys without the
    /// pre-release, the whole one in `TorroVersion`.
    pub(crate) fn info_plist(version: &str) -> String {
        let short = version.split('-').next().unwrap_or(version);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>io.github.mahype.torro-calendar</string>
  <key>CFBundleShortVersionString</key>
  <string>{short}</string>
  <key>CFBundleVersion</key>
  <string>{short}</string>
  <key>TorroVersion</key>
  <string>{version}</string>
</dict>
</plist>
"#
        )
    }

    #[test]
    fn the_version_comes_from_the_bundles_info_plist() {
        assert_eq!(plist_version(&info_plist("0.1.0")).as_deref(), Some("0.1.0"));
        assert_eq!(plist_version(&info_plist("not a version")), None);
        assert_eq!(plist_version("<plist><dict></dict></plist>"), None);
        assert_eq!(plist_version("no xml"), None);
    }

    #[test]
    fn the_version_comes_from_what_the_program_prints() {
        assert_eq!(version_from_output("torrocalendar 0.1.0\n").as_deref(), Some("0.1.0"));
        assert_eq!(version_from_output("torroworkplace 0.2.0-beta.1").as_deref(), Some("0.2.0-beta.1"));
        assert_eq!(version_from_output("usage: torrocalendar [options]"), None);
        assert_eq!(version_from_output(""), None);
    }

    #[test]
    fn the_path_tells_which_package_manager_installed_a_program() {
        let pacman = |path: &Path| path == Path::new("/usr/bin/pacman");
        let none = |_: &Path| false;
        let linux = Platform::Linux;
        assert_eq!(manager_of(Path::new("/usr/bin/torrocalendar"), linux, pacman), Some(Manager::Aur));
        assert_eq!(manager_of(Path::new("/usr/bin/torrocalendar"), linux, none), None, "no pacman, no AUR");
        let cellar = Path::new("/home/linuxbrew/.linuxbrew/Cellar/torrocalendar/0.1.0/bin/torrocalendar");
        assert_eq!(manager_of(cellar, linux, none), Some(Manager::Homebrew));
        assert_eq!(
            manager_of(Path::new("/home/linuxbrew/.linuxbrew/bin/torrocalendar"), linux, none),
            Some(Manager::Homebrew)
        );
        assert_eq!(manager_of(Path::new("/home/anna/.local/bin/torrocalendar"), linux, pacman), None);
        let windows = Platform::Windows;
        let scoop = Path::new(r"C:\Users\Anna\scoop\apps\torroworkplace\current\torrocalendar.exe");
        assert_eq!(manager_of(scoop, windows, none), Some(Manager::Scoop));
        assert_eq!(
            manager_of(Path::new(r"C:\Users\Anna\scoop\shims\torrocalendar.exe"), windows, none),
            Some(Manager::Scoop)
        );
        assert_eq!(manager_of(Path::new(r"C:\Program Files\Torro\torrocalendar.exe"), windows, none), None);
        assert_eq!(
            manager_of(Path::new("/usr/bin/torrocalendar"), Platform::MacOs, pacman),
            None,
            "macOS installs itself"
        );
    }

    #[test]
    fn the_commands_name_the_suites_packages() {
        let aur = Manager::Aur.commands(AppId::INBOX);
        assert_eq!(
            (aur.manager.as_str(), aur.install.as_str(), aur.update.as_str(), aur.uninstall.as_str()),
            ("AUR", "yay -S torroinbox-bin", "yay -Syu", "sudo pacman -R torroinbox-bin")
        );
        assert_eq!(Manager::Aur.commands(AppId::WORKPLACE).install, "yay -S torro-workplace");
        assert_eq!(Manager::Aur.commands(AppId::INBOX_MAIL).install, "yay -S torro-inbox-mail-bin");
        let brew = Manager::Homebrew.commands(AppId::CALENDAR);
        assert_eq!(brew.install, "brew install mahype/torro/torrocalendar");
        assert_eq!(brew.update, "brew upgrade mahype/torro/torrocalendar");
        assert_eq!(brew.uninstall, "brew uninstall mahype/torro/torrocalendar");
        let scoop = Manager::Scoop.commands(AppId::TASKS);
        assert_eq!(
            (scoop.install.as_str(), scoop.update.as_str()),
            ("scoop install torrotasks", "scoop update torrotasks")
        );
        assert_eq!(scoop.uninstall, "scoop uninstall torrotasks");
    }

    pub(crate) fn put_marker(path: &Path, manager: &str) {
        let program = path.file_stem().expect("program").to_str().expect("utf8");
        let package = if manager == "setup" { program.to_string() } else { format!("{program}-bin") };
        let marker =
            path.with_file_name(format!("{}.torro-package.json", path.file_name().expect("name").to_string_lossy()));
        std::fs::write(
            marker,
            serde_json::json!({
                "schema": 1, "manager": manager, "package": package, "program": path
            })
            .to_string(),
        )
        .expect("marker");
    }

    #[test]
    fn native_markers_validate_paths_and_fail_closed() {
        let dir = tempfile::tempdir().expect("dir");
        for (platform, name, manager, expected) in [
            (Platform::Linux, "torrocalendar", "deb", Manager::Deb),
            (Platform::Linux, "torrocalendar", "rpm", Manager::Rpm),
            (Platform::Linux, "torrocalendar", "pacman", Manager::Pacman),
            (Platform::Windows, "torrocalendar.exe", "setup", Manager::Setup),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, "program").expect("program");
            put_marker(&path, manager);
            assert_eq!(manager_on_disk(&path, platform), Some(expected));
            let marker = path.with_file_name(format!("{name}.torro-package.json"));
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&marker).expect("read")).expect("json");
            value["program"] = serde_json::json!(dir.path().join("other"));
            std::fs::write(&marker, value.to_string()).expect("mismatch");
            assert_eq!(manager_on_disk(&path, platform), Some(Manager::Invalid));
            std::fs::write(&marker, "broken").expect("broken");
            assert_eq!(manager_on_disk(&path, platform), Some(Manager::Invalid));
            std::fs::remove_file(marker).expect("remove");
            assert_eq!(manager_on_disk(&path, platform), None);
        }
        assert_eq!(Manager::Deb.commands(AppId::CALENDAR).uninstall, "sudo apt remove torrocalendar-bin");
        assert_eq!(Manager::Rpm.commands(AppId::WORKPLACE).uninstall, "sudo dnf remove torro-workplace");
        assert_eq!(Manager::Pacman.commands(AppId::TASKS).uninstall, "sudo pacman -R torrotasks-bin");
        assert_eq!(Manager::Setup.commands(AppId::TASKS).uninstall, "control appwiz.cpl");
    }

    #[cfg(unix)]
    #[test]
    fn native_markers_also_protect_canonical_programs_behind_path_links() {
        let dir = tempfile::tempdir().expect("dir");
        let real = dir.path().join("torrocalendar");
        std::fs::write(&real, "program").expect("program");
        put_marker(&real, "deb");
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).expect("bin");
        let link = bin.join("torrocalendar");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        assert_eq!(manager_on_disk(&link, Platform::Linux), Some(Manager::Deb));
        std::fs::write(bin.join("torrocalendar.torro-package.json"), "broken").expect("broken");
        assert_eq!(manager_on_disk(&link, Platform::Linux), Some(Manager::Invalid));
    }

    /// An [`Env`] for `platform` in `dir`: the service in `dir/bin`, bundles in `dir/Applications`,
    /// launcher entries in `dir/share` and `dir/Programs`, data in `dir/data/<app>`; versions as
    /// `versions` names them by program, the main application installed when `main_app`. Removing
    /// deletes.
    pub(crate) fn env_in(dir: &Path, platform: Platform, versions: &[(&str, &str)], main_app: bool) -> Env {
        let bin = dir.join("bin");
        let data = dir.join("data");
        let versions: HashMap<String, String> =
            versions.iter().map(|(program, version)| ((*program).to_string(), (*version).to_string())).collect();
        let versions = Arc::new(versions);
        Env {
            platform,
            arch: "x86_64".into(),
            exe: Some(bin.join(platform.program_file("torroworkplace"))),
            home: Some(dir.join("home")),
            bundle_folders: vec![dir.join("Applications")],
            dirs: vec![bin],
            data_home: Some(dir.join("share")),
            start_menu: Some(dir.join("Programs")),
            main_app: Arc::new(move || main_app),
            version_of: Arc::new(move |path: PathBuf| {
                let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
                let version = versions.get(&name).cloned();
                Box::pin(async move { version })
            }),
            app_dirs: Arc::new(move |app: AppId| vec![data.join(app.dir)]),
            discard: Arc::new(|path: &Path| match path.is_dir() {
                true => std::fs::remove_dir_all(path),
                false => std::fs::remove_file(path),
            }),
        }
    }

    /// Puts the program `program` into the service's folder of `env`.
    pub(crate) fn put_program(env: &Env, program: &str) -> PathBuf {
        let dir = &env.dirs[0];
        std::fs::create_dir_all(dir).expect("bin");
        let path = dir.join(env.platform.program_file(program));
        std::fs::write(&path, format!("#!/bin/sh\necho {program}\n")).expect("program");
        path
    }

    /// Puts the bundle `<title>.app` with `program` and an `Info.plist` of `version` into `env`'s first
    /// bundle folder.
    pub(crate) fn put_bundle(env: &Env, title: &str, program: &str, version: &str) -> PathBuf {
        let bundle = env.bundle_folders[0].join(format!("{title}.app"));
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("bundle");
        std::fs::write(bundle.join("Contents/MacOS").join(program), "binary").expect("program");
        std::fs::write(bundle.join("Contents/Info.plist"), info_plist(version)).expect("plist");
        bundle
    }

    #[tokio::test]
    async fn a_scan_finds_programs_with_their_versions() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Linux, &[("torrocalendar", "0.1.0"), ("torroworkplace", "0.1.0")], true);
        put_program(&env, "torroworkplace");
        let calendar = put_program(&env, "torrocalendar");
        put_program(&env, "torrotasks");
        let scan = scan(&env, &Versions::default()).await;
        let found = scan.found(AppId::CALENDAR).expect("calendar");
        assert_eq!((found.path.clone(), found.version.as_deref(), found.managed), (calendar, Some("0.1.0"), None));
        assert_eq!(scan.found(AppId::TASKS).expect("tasks").version, None, "a version that cannot be read");
        assert!(scan.found(AppId::ASSISTANT).is_none());
        assert!(scan.found(AppId::WORKPLACE).is_some(), "the main application");
        assert_eq!(scan.origin, None);
    }

    #[tokio::test]
    async fn without_the_main_application_its_program_alone_is_not_it() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::Windows, &[], false);
        put_program(&env, "torroworkplace");
        assert!(scan(&env, &Versions::default()).await.found(AppId::WORKPLACE).is_none());
    }

    #[tokio::test]
    async fn on_macos_a_scan_finds_bundles_with_their_info_plist() {
        let dir = tempfile::tempdir().expect("dir");
        let env = env_in(dir.path(), Platform::MacOs, &[], false);
        let mail = put_bundle(&env, "Torro Workplace Mail", "torro-inbox-mail", "0.1.0");
        put_bundle(&env, "Torro Workplace", "torroworkplace", "0.2.0");
        // A bundle without its program is no app.
        std::fs::create_dir_all(env.bundle_folders[0].join("Torro Tasks.app/Contents")).expect("empty");
        let scan = scan(&env, &Versions::default()).await;
        assert_eq!(
            scan.found(AppId::INBOX_MAIL).map(|found| (&found.path, found.version.as_deref())),
            Some((&mail, Some("0.1.0")))
        );
        assert_eq!(scan.found(AppId::WORKPLACE).and_then(|found| found.version.as_deref()), Some("0.2.0"));
        assert!(scan.found(AppId::TASKS).is_none());
        assert!(scan.found(AppId::INBOX).is_none(), "the hub's bundle is its own");
    }

    #[tokio::test]
    async fn versions_are_read_again_only_when_the_program_changed() {
        let dir = tempfile::tempdir().expect("dir");
        let program = dir.path().join("torrocalendar");
        std::fs::write(&program, "one").expect("program");
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let version_of: VersionOf = Arc::new(move |_| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async { Some("0.1.0".to_string()) })
        });
        let versions = Versions::default();
        assert_eq!(versions.of(program.clone(), &version_of).await.as_deref(), Some("0.1.0"));
        assert_eq!(versions.of(program.clone(), &version_of).await.as_deref(), Some("0.1.0"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        std::fs::write(&program, "a longer program").expect("changed");
        versions.of(program, &version_of).await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// A real program prints its version (Unix: the fake program of the fixtures).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_program_is_asked_for_its_version() {
        let dir = tempfile::tempdir().expect("dir");
        let program = dir.path().join("torrotasks");
        crate::apps::tests::fake_at(&program, "echo \"torrotasks 0.3.1\"");
        assert_eq!(run_version(program).await.as_deref(), Some("0.3.1"));
        assert_eq!(run_version(dir.path().join("missing")).await, None);
    }
}
