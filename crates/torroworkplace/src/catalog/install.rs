//! The work of an app job on this system (issue #411, step 5 of the plan): download, check, put in
//! place, add or remove the launcher entry. Each step answers a fixed code ([`Fail`]) and never what a
//! program it ran printed.
//!
//! - **macOS**: the signed bundle from `<program>-<arch>-apple-darwin.app.zip`, unpacked with `ditto -x
//!   -k` into a temporary folder, checked with `codesign --verify --deep --strict` and the suite's team
//!   ([`TEAM`]), then put beside "Torro Workplace.app" (where that is not writable: `~/Applications`);
//!   the version before goes to the trash.
//! - **Linux**: the program from `<program>-<target>.tar.xz` (`tar -xJf`) after its SHA-256, beside the
//!   service; its `.desktop` entry from `packaging/linux` (with `Exec` made absolute) and its icon below
//!   `$XDG_DATA_HOME`.
//! - **Windows**: the program from `<program>-<target>.zip` after its SHA-256, beside the service; a
//!   running program is renamed to `.old` first (the next start of the service deletes it,
//!   [`sweep_old`]); a Start menu shortcut in `Programs\Torro Workplace\` through PowerShell's
//!   `WScript.Shell`.
//!
//! Removing moves to the system's trash ([`super::Env::discard`]).

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use torro_core::AppId;

use super::Platform;
use super::installed::{self, Env};
use super::releases::Asset;

/// The team that signs the suite's bundles (Developer ID): a bundle signed by anyone else is refused.
pub const TEAM: &str = "2GA7DQ3P3Z";

/// The largest `.sha256` file that is read.
const MAX_SUM: usize = 4096;

/// Why a step failed, as [`torro_ipc::AppJobInfo::error`] names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// The release's server was not reached, or the download broke off.
    Offline,
    /// The archive's SHA-256 is not the release's.
    Checksum,
    /// The bundle is not signed by [`TEAM`], or its signature does not hold.
    Signature,
    /// The disk is full.
    NoSpace,
    /// The folder cannot be written.
    Permission,
    /// The person cancelled while it downloaded.
    Cancelled,
    /// Anything else.
    Error,
}

impl Fail {
    /// `offline`, `checksum`, `signature`, `no-space`, `permission`, `cancelled` or `error`.
    pub fn code(self) -> &'static str {
        match self {
            Fail::Offline => "offline",
            Fail::Checksum => "checksum",
            Fail::Signature => "signature",
            Fail::NoSpace => "no-space",
            Fail::Permission => "permission",
            Fail::Cancelled => "cancelled",
            Fail::Error => "error",
        }
    }
}

/// The [`Fail`] of a file error.
pub fn io_fail(err: &io::Error) -> Fail {
    match err.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => Fail::Permission,
        io::ErrorKind::StorageFull => Fail::NoSpace,
        _ => Fail::Error,
    }
}

/// The detail of the step Download: the bytes loaded as decimal digits (`25795000`); front ends
/// format the size in their own language (`AppJobStep::detail`).
pub fn byte_count(bytes: u64) -> String {
    bytes.to_string()
}

/// Waits until `cancel` says so; forever when nobody can say so any more.
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Loads `asset` into the file `to`, telling `progress` the bytes so far; stops when `cancel` turns
/// true. More or fewer bytes than the release names are an error.
pub async fn download(
    http: &reqwest::Client,
    asset: &Asset,
    to: &Path,
    cancel: &mut watch::Receiver<bool>,
    progress: impl Fn(u64),
) -> Result<(), Fail> {
    let mut response = tokio::select! {
        response = http.get(&asset.url).send() => response.map_err(|_| Fail::Offline)?,
        () = cancelled(cancel) => return Err(Fail::Cancelled),
    };
    if !response.status().is_success() {
        return Err(Fail::Error);
    }
    let mut file = tokio::fs::File::create(to).await.map_err(|err| io_fail(&err))?;
    let mut loaded = 0u64;
    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.map_err(|_| Fail::Offline)?,
            () = cancelled(cancel) => return Err(Fail::Cancelled),
        };
        let Some(chunk) = chunk else { break };
        loaded += chunk.len() as u64;
        if loaded > asset.size {
            return Err(Fail::Error);
        }
        file.write_all(&chunk).await.map_err(|err| io_fail(&err))?;
        progress(loaded);
    }
    file.flush().await.map_err(|err| io_fail(&err))?;
    if loaded != asset.size {
        return Err(Fail::Offline);
    }
    Ok(())
}

/// The SHA-256 the release names in `asset` (a `.sha256` file).
pub async fn fetch_sum(http: &reqwest::Client, asset: &Asset) -> Result<String, Fail> {
    let mut response = http.get(&asset.url).send().await.map_err(|_| Fail::Offline)?;
    if !response.status().is_success() {
        return Err(Fail::Error);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| Fail::Offline)? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_SUM {
            return Err(Fail::Checksum);
        }
    }
    parse_sum(&String::from_utf8_lossy(&body)).ok_or(Fail::Checksum)
}

/// The hash in a `.sha256` file as `shasum -a 256` writes it (`<64 hex digits>  <name>`), in small letters.
pub fn parse_sum(text: &str) -> Option<String> {
    let word = text.split_whitespace().next()?;
    (word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit())).then(|| word.to_ascii_lowercase())
}

/// The SHA-256 of `file` in small hex letters.
pub fn sha256_of(file: &Path) -> io::Result<String> {
    let mut reader = std::fs::File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `file` has the SHA-256 `expected`, else [`Fail::Checksum`].
pub fn check_sum(file: &Path, expected: &str) -> Result<(), Fail> {
    let actual = sha256_of(file).map_err(|err| io_fail(&err))?;
    if actual == expected.to_ascii_lowercase() { Ok(()) } else { Err(Fail::Checksum) }
}

/// A system program by its absolute path, so none earlier on `PATH` stands in for it; its output is
/// never read unless the caller asks for it.
fn quiet(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

/// macOS: unpacks the `.app.zip` `archive` with `ditto -x -k` into `into` and answers the bundle
/// `bundle` in it, which must hold its program `program`.
pub fn unpack_bundle(archive: &Path, into: &Path, bundle: &str, program: &str) -> Result<PathBuf, Fail> {
    std::fs::create_dir_all(into).map_err(|err| io_fail(&err))?;
    let status = quiet("/usr/bin/ditto").arg("-x").arg("-k").arg(archive).arg(into).status();
    if !status.is_ok_and(|status| status.success()) {
        return Err(Fail::Error);
    }
    let unpacked = into.join(bundle);
    let program = unpacked.join("Contents/MacOS").join(program);
    let plain = std::fs::symlink_metadata(&program).is_ok_and(|meta| meta.is_file());
    if plain { Ok(unpacked) } else { Err(Fail::Error) }
}

/// macOS: the bundle's signature holds (`codesign --verify --deep --strict`) and is the suite's team's
/// ([`TEAM`]); else [`Fail::Signature`].
pub fn check_signature(bundle: &Path) -> Result<(), Fail> {
    let codesign = "/usr/bin/codesign";
    let verified = quiet(codesign).args(["--verify", "--deep", "--strict"]).arg(bundle).status();
    match verified {
        Ok(status) if status.success() => {}
        Ok(_) => return Err(Fail::Signature),
        Err(_) => return Err(Fail::Error),
    }
    // What `codesign -dv` says is read here for the team only and goes nowhere else.
    let shown = quiet(codesign).args(["-dv", "--verbose=2"]).arg(bundle).stderr(Stdio::piped()).output();
    let shown = shown.map_err(|_| Fail::Error)?;
    let said = String::from_utf8_lossy(&shown.stderr);
    let team = said.lines().any(|line| line.trim() == format!("TeamIdentifier={TEAM}"));
    if shown.status.success() && team { Ok(()) } else { Err(Fail::Signature) }
}

/// Whether a file can be made in `folder`.
fn writable(folder: &Path) -> bool {
    tempfile::tempfile_in(folder).is_ok()
}

/// macOS: where a new bundle goes: beside "Torro Workplace.app", else beside the bundle the service runs
/// in, when that folder is writable; else `~/Applications` (made when missing).
pub fn bundle_folder(env: &Env) -> Option<PathBuf> {
    let main = installed::bundle_path(env, "Torro Workplace", "torroworkplace");
    let main = main.as_deref().and_then(Path::parent).map(Path::to_path_buf);
    let own = env.exe.as_deref().and_then(|exe| {
        let outer = exe.ancestors().filter(|folder| folder.extension().is_some_and(|ext| ext == "app")).last()?;
        outer.parent().map(Path::to_path_buf)
    });
    if let Some(folder) = [main, own].into_iter().flatten().find(|folder| writable(folder)) {
        return Some(folder);
    }
    let applications = env.home.as_ref()?.join("Applications");
    std::fs::create_dir_all(&applications).ok()?;
    Some(applications)
}

/// macOS: puts the bundle `new` at `target`; what lay there (`old`, and anything else at `target`) goes
/// to the trash first. Moved when on the same disk, else copied with `ditto`, which keeps the signature.
pub fn place_bundle(
    new: &Path,
    target: &Path,
    old: Option<&Path>,
    discard: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), Fail> {
    for gone in old.into_iter().chain([target]) {
        if std::fs::symlink_metadata(gone).is_ok() {
            discard(gone).map_err(|err| io_fail(&err))?;
        }
    }
    if std::fs::rename(new, target).is_ok() {
        return Ok(());
    }
    let copied = quiet("/usr/bin/ditto").arg(new).arg(target).status();
    if copied.is_ok_and(|status| status.success()) { Ok(()) } else { Err(Fail::Error) }
}

/// The plain file `program` in `folder` or in a folder directly below it (dist's archives hold one
/// folder named like the archive).
fn find_program(folder: &Path, program: &str) -> Option<PathBuf> {
    let plain = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
    let top = folder.join(program);
    if plain(&top) {
        return Some(top);
    }
    let entries = std::fs::read_dir(folder).ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path().join(program))
        .find(|path| plain(path))
}

/// Linux: unpacks the `.tar.xz` `archive` into `into` with `tar -xJf` and answers its program `program`.
pub fn unpack_tar(archive: &Path, into: &Path, program: &str) -> Result<PathBuf, Fail> {
    std::fs::create_dir_all(into).map_err(|err| io_fail(&err))?;
    let tar = if Path::new("/usr/bin/tar").is_file() { "/usr/bin/tar" } else { "/bin/tar" };
    let status = quiet(tar).arg("-xJf").arg(archive).arg("-C").arg(into).status();
    if !status.is_ok_and(|status| status.success()) {
        return Err(Fail::Error);
    }
    find_program(into, program).ok_or(Fail::Error)
}

/// Windows: takes the program `program` (`torrocalendar.exe`) out of the `.zip` `archive` into `into`,
/// only that file and only from the top or one folder down; nothing else of the archive is written.
pub fn unpack_zip(archive: &Path, into: &Path, program: &str) -> Result<PathBuf, Fail> {
    let file = std::fs::File::open(archive).map_err(|err| io_fail(&err))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| Fail::Error)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| Fail::Error)?;
        let Some(name) = entry.enclosed_name() else { continue };
        let fits =
            entry.is_file() && name.file_name().is_some_and(|file| file == program) && name.components().count() <= 2;
        if !fits {
            continue;
        }
        std::fs::create_dir_all(into).map_err(|err| io_fail(&err))?;
        let out = into.join(program);
        let mut written = std::fs::File::create(&out).map_err(|err| io_fail(&err))?;
        io::copy(&mut entry, &mut written).map_err(|err| io_fail(&err))?;
        return Ok(out);
    }
    Err(Fail::Error)
}

/// Linux, Windows: puts the program `new` at `target`, through a file beside it so the program is never
/// half there. On Windows a program at `target` is renamed aside first ([`move_aside`]): a running one
/// cannot be replaced, only renamed. On Linux the new file replaces the old one, which a running program
/// keeps open.
pub fn place_program(new: &Path, target: &Path, platform: Platform) -> Result<(), Fail> {
    let dir = target.parent().ok_or(Fail::Error)?;
    let name = target.file_name().ok_or(Fail::Error)?.to_string_lossy().into_owned();
    std::fs::create_dir_all(dir).map_err(|err| io_fail(&err))?;
    let staged = dir.join(format!(".{name}.new"));
    std::fs::copy(new, &staged).map_err(|err| io_fail(&err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|err| io_fail(&err))?;
    }
    if platform == Platform::Windows && std::fs::symlink_metadata(target).is_ok() {
        move_aside(target).inspect_err(|_| {
            let _ = std::fs::remove_file(&staged);
        })?;
    }
    std::fs::rename(&staged, target).map_err(|err| {
        let _ = std::fs::remove_file(&staged);
        io_fail(&err)
    })
}

/// Windows: renames `program` to `<program>.old` (an older `.old` is deleted, or, while it still runs,
/// the next free `<program>.<n>.old` is taken). [`sweep_old`] deletes them at the next start.
fn move_aside(program: &Path) -> Result<(), Fail> {
    let name = program.file_name().ok_or(Fail::Error)?.to_string_lossy().into_owned();
    let dir = program.parent().ok_or(Fail::Error)?;
    let first = dir.join(format!("{name}.old"));
    let _ = std::fs::remove_file(&first);
    let free = std::iter::once(first)
        .chain((1..10).map(|n| dir.join(format!("{name}.{n}.old"))))
        .find(|path| std::fs::symlink_metadata(path).is_err())
        .ok_or(Fail::Error)?;
    std::fs::rename(program, free).map_err(|err| io_fail(&err))
}

/// Windows: deletes what updates renamed aside in `dir` ([`move_aside`]): `<program>.exe.old` and
/// `<program>.exe.<n>.old` of the suite's programs, nothing else. One still running stays for the next time.
pub fn sweep_old(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = AppId::ALL
            .iter()
            .map(|app| app.program_file())
            .chain([super::REMINDER.program])
            .any(|program| name.strip_prefix(&format!("{program}.exe.")).is_some_and(|rest| rest.ends_with("old")));
        if ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The `.desktop` entries and icons of `packaging/linux`, by window class.
const LAUNCHERS: &[(&str, &str, &str)] = &[
    (
        "torro-workplace",
        include_str!("../../../../packaging/linux/torro-workplace.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-workplace.svg"),
    ),
    (
        "torro-inbox",
        include_str!("../../../../packaging/linux/torro-inbox.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-inbox.svg"),
    ),
    (
        "torro-inbox-mail",
        include_str!("../../../../packaging/linux/torro-inbox-mail.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-inbox-mail.svg"),
    ),
    (
        "torro-inbox-notifications",
        include_str!("../../../../packaging/linux/torro-inbox-notifications.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-inbox-notifications.svg"),
    ),
    (
        "torro-tasks",
        include_str!("../../../../packaging/linux/torro-tasks.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-tasks.svg"),
    ),
    (
        "torro-calendar",
        include_str!("../../../../packaging/linux/torro-calendar.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-calendar.svg"),
    ),
    (
        "torro-assistant",
        include_str!("../../../../packaging/linux/torro-assistant.desktop"),
        include_str!("../../../../packaging/linux/icons/torro-assistant.svg"),
    ),
];

/// `program` as one argument of a `.desktop` file's `Exec` key, encoded for the file: quoted when it
/// holds a reserved character, `%` doubled, and the string's backslashes escaped (Desktop Entry
/// Specification, "The Exec key").
fn exec_argument(program: &Path) -> String {
    let text = program.to_string_lossy().replace('%', "%%");
    let reserved = |c: char| " \t\n\"'\\><~|&;$*?#()`".contains(c);
    let argument = match text.chars().any(reserved) {
        false => text,
        true => {
            let mut quoted = String::from("\"");
            for c in text.chars() {
                if "\"`$\\".contains(c) {
                    quoted.push('\\');
                }
                quoted.push(c);
            }
            quoted.push('"');
            quoted
        }
    };
    argument.replace('\\', "\\\\")
}

/// `desktop` with its `Exec` starting `program` by its absolute path, the arguments kept.
fn with_exec(desktop: &str, program: &Path) -> String {
    let lines = desktop.lines().map(|line| match line.strip_prefix("Exec=") {
        Some(command) => {
            let rest = command.split_once(' ').map(|(_, rest)| format!(" {rest}")).unwrap_or_default();
            format!("Exec={}{rest}", exec_argument(program))
        }
        None => line.to_string(),
    });
    let mut text = lines.collect::<Vec<_>>().join("\n");
    text.push('\n');
    text
}

/// The `.desktop` entry and icon of `app` below `data_home`.
fn launcher_paths(app: AppId, data_home: &Path) -> (PathBuf, PathBuf) {
    let class = app.window_class;
    (
        data_home.join("applications").join(format!("{class}.desktop")),
        data_home.join("icons/hicolor/scalable/apps").join(format!("{class}.svg")),
    )
}

/// Linux: writes `app`'s `.desktop` entry (starting `program`) and icon below `data_home`; answers the entry.
pub fn write_desktop(app: AppId, program: &Path, data_home: Option<&Path>) -> Result<PathBuf, Fail> {
    let data_home = data_home.ok_or(Fail::Error)?;
    let (_, desktop, icon) = LAUNCHERS.iter().find(|(class, _, _)| *class == app.window_class).ok_or(Fail::Error)?;
    let (entry, icon_path) = launcher_paths(app, data_home);
    for (path, text) in [(&entry, with_exec(desktop, program)), (&icon_path, (*icon).to_string())] {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| io_fail(&err))?;
        }
        std::fs::write(path, text).map_err(|err| io_fail(&err))?;
    }
    Ok(entry)
}

/// Linux: removes `app`'s `.desktop` entry and icon below `data_home`; answers the entry when it was there.
pub fn remove_desktop(app: AppId, data_home: Option<&Path>) -> Result<Option<PathBuf>, Fail> {
    let Some(data_home) = data_home else { return Ok(None) };
    let (entry, icon) = launcher_paths(app, data_home);
    let was = entry.is_file().then(|| entry.clone());
    for path in [entry, icon] {
        match std::fs::remove_file(&path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(io_fail(&err)),
            _ => {}
        }
    }
    Ok(was)
}

/// Windows: `app`'s shortcut in the Start menu, `Programs\Torro Workplace\<name>.lnk`.
fn shortcut_path(app: AppId, start_menu: &Path) -> PathBuf {
    start_menu.join("Torro Workplace").join(format!("{}.lnk", app.display_name))
}

/// Windows: the shortcut of `app` to `program` in the Start menu (no arguments: every program opens
/// its window without, the main application its control centre); answers its path.
pub fn make_shortcut(app: AppId, program: &Path, start_menu: Option<&Path>) -> Result<PathBuf, Fail> {
    let start_menu = start_menu.ok_or(Fail::Error)?;
    let link = shortcut_path(app, start_menu);
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir).map_err(|err| io_fail(&err))?;
    }
    create_shortcut(&link, program)?;
    Ok(link)
}

/// Windows: `powershell -NoProfile` with `WScript.Shell` writes the shortcut; the paths go in through
/// the environment, never into the script's text.
#[cfg(windows)]
fn create_shortcut(link: &Path, program: &Path) -> Result<(), Fail> {
    use std::os::windows::process::CommandExt;
    // From `winbase.h`: no console window flashes up.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SCRIPT: &str = "$shell = New-Object -ComObject WScript.Shell; \
                          $link = $shell.CreateShortcut($env:TORRO_LINK); \
                          $link.TargetPath = $env:TORRO_TARGET; \
                          $link.WorkingDirectory = $env:TORRO_FOLDER; \
                          $link.Save()";
    let powershell = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let folder = program.parent().unwrap_or(program);
    let status = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("TORRO_LINK", link)
        .env("TORRO_TARGET", program)
        .env("TORRO_FOLDER", folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if status.is_ok_and(|status| status.success()) && link.is_file() { Ok(()) } else { Err(Fail::Error) }
}

/// Elsewhere only tests run the Windows rules, without PowerShell: the shortcut is not written.
#[cfg(not(windows))]
fn create_shortcut(link: &Path, program: &Path) -> Result<(), Fail> {
    let _ = (link, program);
    Ok(())
}

/// Windows: removes `app`'s shortcut; answers it when it was there.
pub fn remove_shortcut(app: AppId, start_menu: Option<&Path>) -> Result<Option<PathBuf>, Fail> {
    let Some(start_menu) = start_menu else { return Ok(None) };
    let link = shortcut_path(app, start_menu);
    match std::fs::remove_file(&link) {
        Ok(()) => Ok(Some(link)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(io_fail(&err)),
    }
}

/// Moves the bundles or programs `paths` and the data folders `data` away with `discard` (to the
/// trash). On Windows a program the trash will not take (it runs) is renamed aside ([`move_aside`]).
pub fn remove(
    paths: &[PathBuf],
    data: &[PathBuf],
    platform: Platform,
    discard: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), Fail> {
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(path) else { continue };
        if let Err(err) = discard(path) {
            match platform == Platform::Windows && meta.is_file() {
                true => move_aside(path)?,
                false => return Err(io_fail(&err)),
            }
        }
    }
    for dir in data {
        if std::fs::symlink_metadata(dir).is_ok() {
            discard(dir).map_err(|err| io_fail(&err))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A `.zip` like dist's for Windows (no top folder) with `program` (contents `body`), beside a file
    /// that tries to leave the archive and a program of the same name too deep down.
    pub(crate) fn zip_with(path: &Path, program: &str, body: &[u8]) {
        let file = std::fs::File::create(path).expect("zip");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("../escape.txt", options).expect("entry");
        zip.write_all(b"never written").expect("write");
        zip.start_file(format!("a/b/{program}"), options).expect("entry");
        zip.write_all(b"too deep").expect("write");
        zip.start_file("README.md", options).expect("entry");
        zip.write_all(b"readme").expect("write");
        zip.start_file(program, options).expect("entry");
        zip.write_all(body).expect("write");
        zip.finish().expect("finish");
    }

    /// A `.tar.xz` like dist's for Linux: `<program>-<target>/<program>` with `body` (Unix: `tar`).
    #[cfg(unix)]
    pub(crate) fn tar_with(path: &Path, program: &str, body: &[u8]) {
        let source = tempfile::tempdir().expect("source");
        let top = format!("{program}-x86_64-unknown-linux-gnu");
        std::fs::create_dir_all(source.path().join(&top)).expect("folder");
        std::fs::write(source.path().join(&top).join(program), body).expect("program");
        let status = Command::new("tar").arg("-cJf").arg(path).arg("-C").arg(source.path()).arg(&top).status();
        assert!(status.is_ok_and(|status| status.success()), "tar -cJf");
    }

    #[test]
    fn the_checksum_file_is_read_as_shasum_writes_it() {
        let sum = "A".repeat(64);
        assert_eq!(parse_sum(&format!("{sum}  torrocalendar.tar.xz\n")), Some("a".repeat(64)));
        assert_eq!(parse_sum("abc  short"), None);
        assert_eq!(parse_sum(""), None);
        assert_eq!(parse_sum(&format!("{}  x", "g".repeat(64))), None);
    }

    #[test]
    fn a_file_is_checked_against_its_sha256() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("archive");
        std::fs::write(&file, b"abc").expect("write");
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(check_sum(&file, abc), Ok(()));
        assert_eq!(check_sum(&file, &abc.to_uppercase()), Ok(()));
        assert_eq!(check_sum(&file, &"0".repeat(64)), Err(Fail::Checksum));
    }

    #[test]
    fn only_the_program_comes_out_of_a_zip() {
        let dir = tempfile::tempdir().expect("dir");
        let archive = dir.path().join("torrocalendar.zip");
        zip_with(&archive, "torrocalendar.exe", b"MZ program");
        let into = dir.path().join("out/unpacked");
        let program = unpack_zip(&archive, &into, "torrocalendar.exe").expect("unpacked");
        assert_eq!(std::fs::read(&program).expect("read"), b"MZ program");
        assert!(!dir.path().join("out/escape.txt").exists() && !into.join("README.md").exists());
        assert_eq!(unpack_zip(&archive, &into, "torrotasks.exe"), Err(Fail::Error), "not in the archive");
    }

    #[cfg(unix)]
    #[test]
    fn the_program_comes_out_of_a_tar_xz() {
        let dir = tempfile::tempdir().expect("dir");
        let archive = dir.path().join("torrotasks.tar.xz");
        tar_with(&archive, "torrotasks", b"ELF program");
        let program = unpack_tar(&archive, &dir.path().join("unpacked"), "torrotasks").expect("unpacked");
        assert_eq!(std::fs::read(program).expect("read"), b"ELF program");
        assert_eq!(unpack_tar(&archive, &dir.path().join("again"), "torrocalendar"), Err(Fail::Error));
    }

    #[test]
    fn a_program_on_windows_is_renamed_aside_and_swept_at_the_next_start() {
        let dir = tempfile::tempdir().expect("dir");
        let target = dir.path().join("torrocalendar.exe");
        std::fs::write(&target, "old").expect("old");
        let new = dir.path().join("new.exe");
        std::fs::write(&new, "new").expect("new");
        place_program(&new, &target, Platform::Windows).expect("placed");
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "new");
        let aside = dir.path().join("torrocalendar.exe.old");
        assert_eq!(std::fs::read_to_string(&aside).expect("aside"), "old");
        // Again while the first `.old` is still there (here it can be deleted).
        place_program(&new, &target, Platform::Windows).expect("again");
        std::fs::write(dir.path().join("torrocalendar.exe.1.old"), "older").expect("older");
        std::fs::write(dir.path().join("notes.old"), "not ours").expect("other");
        sweep_old(dir.path());
        assert!(!aside.exists() && !dir.path().join("torrocalendar.exe.1.old").exists());
        assert!(dir.path().join("notes.old").exists(), "only the suite's programs");
        assert!(target.exists());
    }

    #[test]
    fn a_program_on_linux_replaces_the_old_one_in_place() {
        let dir = tempfile::tempdir().expect("dir");
        let target = dir.path().join("bin/torrotasks");
        let new = dir.path().join("new");
        std::fs::write(&new, "new").expect("new");
        place_program(&new, &target, Platform::Linux).expect("placed");
        std::fs::write(&new, "newer").expect("newer");
        place_program(&new, &target, Platform::Linux).expect("replaced");
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "newer");
        let left: Vec<_> = std::fs::read_dir(dir.path().join("bin")).expect("dir").filter_map(Result::ok).collect();
        assert_eq!(left.len(), 1, "no file left beside it");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn the_desktop_entry_starts_the_program_by_its_path() {
        let dir = tempfile::tempdir().expect("dir");
        let program = dir.path().join("bin/torroworkplace");
        let entry = write_desktop(AppId::WORKPLACE, &program, Some(dir.path())).expect("entry");
        assert_eq!(entry, dir.path().join("applications/torro-workplace.desktop"));
        let text = std::fs::read_to_string(&entry).expect("read");
        assert!(text.contains(&format!("Exec={} ui\n", program.display())), "{text}");
        assert!(text.contains("StartupWMClass=torro-workplace"));
        assert!(dir.path().join("icons/hicolor/scalable/apps/torro-workplace.svg").is_file());
        assert_eq!(remove_desktop(AppId::WORKPLACE, Some(dir.path())), Ok(Some(entry.clone())));
        assert!(!entry.exists());
        assert_eq!(remove_desktop(AppId::WORKPLACE, Some(dir.path())), Ok(None));
    }

    #[test]
    fn every_app_has_a_launcher_entry() {
        for app in AppId::ALL {
            assert!(LAUNCHERS.iter().any(|(class, _, _)| *class == app.window_class), "{}", app.window_class);
        }
    }

    #[test]
    fn a_path_with_spaces_is_quoted_in_exec() {
        assert_eq!(exec_argument(Path::new("/opt/torro/torrocalendar")), "/opt/torro/torrocalendar");
        assert_eq!(
            exec_argument(Path::new("/home/anna/My Apps/torrocalendar")),
            "\"/home/anna/My Apps/torrocalendar\""
        );
        assert_eq!(exec_argument(Path::new("/a b/$x")), "\"/a b/\\\\$x\"", "escaped for Exec, then for the string");
        assert_eq!(exec_argument(Path::new("/50%/x")), "/50%%/x");
    }

    #[test]
    fn the_windows_shortcut_lies_in_the_suites_folder() {
        let dir = tempfile::tempdir().expect("dir");
        let link = make_shortcut(AppId::INBOX_MAIL, Path::new(r"C:\Torro\torro-inbox-mail.exe"), Some(dir.path()));
        assert_eq!(link, Ok(dir.path().join("Torro Workplace/Torro Workplace Mail.lnk")));
        assert_eq!(remove_shortcut(AppId::INBOX_MAIL, Some(dir.path())), Ok(None), "no PowerShell here");
    }

    #[test]
    fn a_running_program_the_trash_refuses_is_renamed_aside_on_windows() {
        let dir = tempfile::tempdir().expect("dir");
        let program = dir.path().join("torrotasks.exe");
        std::fs::write(&program, "running").expect("program");
        let refuse = |_: &Path| Err(io::Error::other("in use"));
        remove(std::slice::from_ref(&program), &[], Platform::Windows, &refuse).expect("aside");
        assert!(!program.exists() && dir.path().join("torrotasks.exe.old").exists());
        std::fs::write(&program, "again").expect("program");
        assert_eq!(remove(std::slice::from_ref(&program), &[], Platform::Linux, &refuse), Err(Fail::Error));
    }

    #[test]
    fn sizes_are_handed_on_as_bytes() {
        assert_eq!(byte_count(24_600_000), "24600000");
        assert_eq!(byte_count(0), "0");
    }

    /// macOS: an unsigned bundle is refused before anything is put in place.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_unsigned_bundle_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let source = dir.path().join("source");
        let bundle = source.join("Torro Tasks.app/Contents/MacOS");
        std::fs::create_dir_all(&bundle).expect("bundle");
        std::fs::write(bundle.join("torrotasks"), "#!/bin/sh\n").expect("program");
        let archive = dir.path().join("torrotasks-aarch64-apple-darwin.app.zip");
        let zipped = quiet("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(source.join("Torro Tasks.app"))
            .arg(&archive)
            .status();
        assert!(zipped.is_ok_and(|status| status.success()));
        let unpacked =
            unpack_bundle(&archive, &dir.path().join("unpacked"), "Torro Tasks.app", "torrotasks").expect("unpacked");
        assert_eq!(check_signature(&unpacked), Err(Fail::Signature));
        assert_eq!(
            unpack_bundle(&archive, &dir.path().join("other"), "Torro Calendar.app", "torrocalendar"),
            Err(Fail::Error)
        );
    }
}
