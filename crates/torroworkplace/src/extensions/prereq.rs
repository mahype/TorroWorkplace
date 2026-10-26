//! Whether the programs an MCP server needs are on this computer (task MA.2): `npx`, `uvx`, `docker` or
//! another program, looked for on the service's `PATH` and then in the folders installers use.
//!
//! The service often starts without the `PATH` of a login shell (from the tray, launchd, systemd), so a
//! program a person installed with Homebrew, nvm or Volta is not on it; hence the usual folders. Every root
//! (the `PATH`, the home directory, the folders outside it) is a field of [`Roots`], so tests look in
//! temporary folders and never in the person's home. The folder a program was found in goes to the front of
//! the `PATH` of the server's process (task MA.3), so `npx` finds the `node` next to it.

use std::path::{Path, PathBuf};

use torro_ipc::extensions::Prerequisite;

/// Where programs are looked for. [`Roots::system`] is the service's own; tests state theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    /// The service's `PATH`, in order; looked in first.
    pub path: Vec<PathBuf>,
    /// The home directory, below which `.local/bin`, `bin`, `.cargo/bin`, `.volta/bin` and the newest
    /// `.nvm/versions/node/*/bin` are looked in; `None` without one.
    pub home: Option<PathBuf>,
    /// The usual folders outside the home directory, in order: `/opt/homebrew/bin`, `/usr/local/bin` and on
    /// macOS Docker Desktop's `/Applications/Docker.app/Contents/Resources/bin`; on Windows
    /// `%ProgramFiles%\nodejs`, `%APPDATA%\npm` and Docker Desktop's `%ProgramFiles%\Docker\Docker\resources\bin`.
    pub fixed: Vec<PathBuf>,
}

impl Roots {
    /// The service's view: its `PATH`, its home directory and the usual folders of this system.
    pub fn system() -> Roots {
        let path = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        Roots { path, home: dirs::home_dir(), fixed: fixed_dirs() }
    }

    /// The usual folders that exist, in the order they are looked in after the `PATH`: [`Roots::fixed`], then
    /// those below [`Roots::home`]. The `PATH` the core gives a server's process ends with them (task MA.3).
    pub fn usual(&self) -> Vec<PathBuf> {
        let below_home = self.home.iter().flat_map(|home| home_dirs(home));
        self.fixed.iter().cloned().chain(below_home).filter(|dir| dir.is_dir()).collect()
    }
}

/// A program that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The program to start, as found (a link is not resolved: nvm's `npx` is one).
    pub program: PathBuf,
    /// The folder it is in, which goes to the front of the server's `PATH`.
    pub dir: PathBuf,
}

/// The program a prerequisite stands for: `npx`, `uvx`, `docker` or the named one.
pub fn program_name(prerequisite: &Prerequisite) -> &str {
    match prerequisite {
        Prerequisite::Npx => "npx",
        Prerequisite::Uvx => "uvx",
        Prerequisite::Docker => "docker",
        Prerequisite::Program { name } => name,
    }
}

/// The first executable called `name` on [`Roots::path`], then in [`Roots::usual`]; on Windows `name.exe`
/// or `name.cmd`, as `client_setup` looks for commands. An absolute `name` is taken as it is when it is an
/// executable. `None` when nothing is found, and for a relative path (`./server`, `bin/server`): a server
/// starts only a program by name or by absolute path. Only absolute folders are looked in: a relative one on
/// the `PATH` (`.`, `bin`) would depend on the folder the service happens to run in.
pub fn find(name: &str, roots: &Roots) -> Option<Found> {
    let path = Path::new(name);
    if path.is_absolute() {
        return is_executable(path).then(|| found(path.to_path_buf()));
    }
    if name.is_empty() || path.components().count() != 1 {
        return None;
    }
    let names: Vec<String> =
        if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.to_string()] };
    roots
        .path
        .iter()
        .cloned()
        .chain(roots.usual())
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| is_executable(candidate))
        .map(found)
}

/// What of `requires` is not found ([`find`] of [`program_name`]), in its order: what
/// [`torro_ipc::extensions::CatalogEntry::missing`] says.
pub fn missing(requires: &[Prerequisite], roots: &Roots) -> Vec<Prerequisite> {
    requires.iter().filter(|needed| find(program_name(needed), roots).is_none()).cloned().collect()
}

fn found(program: PathBuf) -> Found {
    let dir = program.parent().map(Path::to_path_buf).unwrap_or_default();
    Found { program, dir }
}

/// The usual folders below the home directory, in order, whether they exist or not.
fn home_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        [".local/bin", "bin", ".cargo/bin", ".volta/bin"].iter().map(|dir| home.join(dir)).collect();
    dirs.extend(newest_nvm(home));
    dirs
}

/// `~/.nvm/versions/node/<newest>/bin`: nvm keeps one folder per Node.js version (`v22.11.0`) and puts only
/// the default one on the `PATH` of a login shell; the newest is the best guess without it. Folders whose
/// name is no version are left out.
fn newest_nvm(home: &Path) -> Option<PathBuf> {
    let versions = std::fs::read_dir(home.join(".nvm/versions/node")).ok()?;
    versions
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((version_of(&name)?, entry.path()))
        })
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, dir)| dir.join("bin"))
}

/// `v22.11.0` → `[22, 11, 0]`, compared number by number (so `v10` is newer than `v9`).
fn version_of(name: &str) -> Option<Vec<u64>> {
    name.strip_prefix('v')?.split('.').map(|part| part.parse().ok()).collect()
}

#[cfg(not(windows))]
fn fixed_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")];
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"));
    }
    dirs
}

#[cfg(windows)]
fn fixed_dirs() -> Vec<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    [
        program_files.as_ref().map(|dir| dir.join("nodejs")),
        app_data.map(|dir| dir.join("npm")),
        program_files.map(|dir| dir.join("Docker").join("Docker").join("resources").join("bin")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An executable `name` (on Windows `name.exe`) in `dir`, which is made.
    fn program(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("dir");
        let file = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
        std::fs::write(&file, "#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        file
    }

    #[test]
    fn a_program_is_found_in_a_folder_standing_in_for_homebrew() {
        let dir = tempfile::tempdir().expect("temp dir");
        let brew = dir.path().join("homebrew/bin");
        let uvx = program(&brew, "uvx");
        let roots = Roots { path: vec![dir.path().join("empty")], home: None, fixed: vec![brew.clone()] };
        assert_eq!(find("uvx", &roots), Some(Found { program: uvx, dir: brew }));
        assert_eq!(missing(&[Prerequisite::Uvx, Prerequisite::Docker], &roots), vec![Prerequisite::Docker]);
        assert_eq!(roots.usual(), vec![dir.path().join("homebrew/bin")], "only folders that exist");
    }

    #[test]
    fn the_path_comes_before_the_usual_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let on_path = program(&dir.path().join("path"), "docker");
        program(&dir.path().join("fixed"), "docker");
        let roots = Roots { path: vec![dir.path().join("path")], home: None, fixed: vec![dir.path().join("fixed")] };
        assert_eq!(find("docker", &roots).map(|found| found.program), Some(on_path));
    }

    #[test]
    fn the_newest_nvm_node_is_found_below_home() {
        let home = tempfile::tempdir().expect("temp dir");
        let versions = home.path().join(".nvm/versions/node");
        program(&versions.join("v9.11.2/bin"), "npx");
        let newest = program(&versions.join("v22.11.0/bin"), "npx");
        program(&versions.join("v18.20.4/bin"), "npx");
        std::fs::create_dir_all(versions.join("system")).expect("not a version");
        let roots = Roots { path: Vec::new(), home: Some(home.path().to_path_buf()), fixed: Vec::new() };
        let found = find("npx", &roots).expect("found");
        assert_eq!(found.program, newest);
        assert_eq!(found.dir, versions.join("v22.11.0/bin"), "the folder for the server's PATH");
        assert_eq!(missing(&[Prerequisite::Npx], &roots), Vec::new());
    }

    #[test]
    fn the_folders_below_home_are_looked_in() {
        let home = tempfile::tempdir().expect("temp dir");
        let volta = program(&home.path().join(".volta/bin"), "npx");
        let cargo = program(&home.path().join(".cargo/bin"), "mcp-tool");
        let roots = Roots { path: Vec::new(), home: Some(home.path().to_path_buf()), fixed: Vec::new() };
        assert_eq!(find("npx", &roots).map(|found| found.program), Some(volta));
        let tool = Prerequisite::Program { name: "mcp-tool".into() };
        assert_eq!(find(program_name(&tool), &roots).map(|found| found.program), Some(cargo));
    }

    #[test]
    fn a_missing_program_is_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("npx.txt"), "").expect("write");
        let roots =
            Roots { path: vec![dir.path().to_path_buf()], home: Some(dir.path().to_path_buf()), fixed: Vec::new() };
        let all =
            [Prerequisite::Npx, Prerequisite::Uvx, Prerequisite::Docker, Prerequisite::Program { name: "git".into() }];
        assert_eq!(missing(&all, &roots), all.to_vec());
        assert_eq!(find("npx", &Roots::default()), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_executable_does_not_count() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let file = program(dir.path(), "uvx");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let roots = Roots { path: vec![dir.path().to_path_buf()], home: None, fixed: Vec::new() };
        assert_eq!(find("uvx", &roots), None);
    }

    #[test]
    fn absolute_paths_count_and_relative_ones_do_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = program(dir.path(), "server");
        let roots = Roots { path: vec![dir.path().to_path_buf()], home: None, fixed: Vec::new() };
        assert_eq!(find(&server.to_string_lossy(), &roots).map(|found| found.dir), Some(dir.path().to_path_buf()));
        assert_eq!(find(&dir.path().join("nothing").to_string_lossy(), &roots), None);
        let relative = if cfg!(windows) { ".\\server.exe" } else { "./server" };
        assert_eq!(find(relative, &roots), None);
        assert_eq!(find("", &roots), None);
    }

    /// H9 of the API review: a relative folder on the `PATH` (here one that leads to the program from the
    /// folder the tests run in) is not looked in, the same folder named absolutely is; nor does it reach the
    /// `PATH` of a server's process.
    #[cfg(unix)]
    #[test]
    fn relative_folders_on_the_path_are_not_looked_in() {
        let dir = tempfile::tempdir().expect("temp dir");
        let uvx = program(dir.path(), "uvx");
        let here = std::env::current_dir().expect("current dir");
        let up: PathBuf = here.components().skip(1).map(|_| Path::new("..")).collect();
        let relative = up.join(dir.path().strip_prefix("/").expect("absolute"));
        assert!(relative.is_relative() && relative.join("uvx").is_file(), "{relative:?} leads to the program");
        let roots = Roots { path: vec![relative.clone(), PathBuf::from(".")], home: None, fixed: Vec::new() };
        assert_eq!(find("uvx", &roots), None);
        let absolute = Roots { path: vec![dir.path().to_path_buf()], ..roots.clone() };
        assert_eq!(find("uvx", &absolute).map(|found| found.program), Some(uvx.clone()));
        let dirs = crate::extensions::env::path_dirs(&found(uvx), &roots);
        assert_eq!(dirs, vec![dir.path().to_path_buf()], "only the program's own folder");
    }

    #[test]
    fn versions_compare_number_by_number() {
        assert!(version_of("v10.0.0") > version_of("v9.11.2"));
        assert_eq!(version_of("v22.11.0"), Some(vec![22, 11, 0]));
        assert_eq!(version_of("system"), None);
        assert_eq!(version_of("v22.x"), None);
    }
}
