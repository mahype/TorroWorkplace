//! The built-in catalog of MCP servers (task MA.2): `assets/extensions/catalog.toml`, built in, read and
//! checked once.
//!
//! Each entry says what people see before they install it (a [`CatalogEntry`]) and how the core starts it:
//! a program by name with fixed arguments, never through a shell. The checks keep a mistake in the file
//! from reaching a person's computer: an id the rest of the core cannot take, a variable that would make
//! the program load other code ([`blocked_env`]), a tool that would need `admin`, a shell as the command, or
//! a program that would wait for an answer on stdin (stdin is the MCP connection). Unit tests prove the
//! built-in file passes.
//!
//! Entries that work on the person's files (issue #303, ArtCraft) say so in `[server.folder]`: the options
//! that bound the server to the folder a person chose ([`Bounds`]), or none when only Torro's check of the
//! paths keeps it there. Their tools may name the arguments that are paths read or written ([`ToolFiles`]),
//! and tools or commands Torro never lets anyone use ([`Entry::blocked`], [`Entry::blocked_commands`]).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;
use torro_ipc::api::Access;
use torro_ipc::extensions::{
    CatalogEntry, CatalogSecret, CatalogTool, FolderCheck, Prerequisite, blocked_env, tool_name, valid_env_name,
    valid_id,
};

use super::prereq::program_name;

/// The catalog as built in.
const CATALOG: &str = include_str!("../../assets/extensions/catalog.toml");

/// Programs that would read their arguments as a script: never a command of the catalog.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "csh", "tcsh", "cmd", "powershell", "pwsh"];

/// The programs that install a package by a version the arguments name: only for them the catalog fixes the
/// version. Any other program a person installs, and `version` says which one the entry was tested with.
const VERSIONED: &[&str] = &["npx", "uvx", "docker"];

/// Arguments an entry with a folder must never have: ArtCraft's `--connect` works the running app without
/// its bounds, PdfCraft's `--root` is an older bound beside the two Torro sets ([`Bounds`]).
const UNBOUNDED_ARGS: &[&str] = &["--connect", "--root"];

/// The only placeholder of [`Entry::look_in`]: Windows' folder for programs, where ArtCraft's installers put
/// `<app>-cli.exe` without adding it to the `PATH`.
const PROGRAM_FILES: &str = "{ProgramFiles}";

/// One server of the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What people see: `missing` is empty here; [`super::Extensions::catalog`] fills it for this computer.
    pub info: CatalogEntry,
    /// The program, by name: one of its prerequisites ([`program_name`]), looked for with
    /// [`super::prereq::find`].
    pub command: String,
    /// Its arguments, the fixed version among them.
    pub args: Vec<String>,
    /// Variables that are no secret, by name; the secrets are `info.secrets`.
    pub env: BTreeMap<String, String>,
    /// The options that bound the server to the folder a person chose; `None` for an entry without
    /// `[server.folder]` and for one whose server has no bound of its own ([`FolderCheck::TorroOnly`]).
    pub bounds: Option<Bounds>,
    /// Folders besides the usual ones its program is looked for in ([`Entry::look_in_dirs`]), as written:
    /// `{ProgramFiles}/<App>`.
    pub look_in: Vec<String>,
    /// Tools never offered and never allowed ([`torro_ipc::extensions::ToolState::Blocked`]).
    pub blocked: Vec<String>,
    /// Commands of the server's all-purpose tools Torro refuses by name; one ending in `.*` stands for every
    /// command starting with what comes before it (`ui.*`).
    pub blocked_commands: Vec<String>,
    /// The arguments of its tools that are paths read or written, by tool; only tools that have some.
    pub files: BTreeMap<String, ToolFiles>,
}

/// The two options that bound a server to folders (ArtCraft: `--automation-read-root` and
/// `--automation-write-root`), each followed by one folder when the core starts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bounds {
    /// The option for the folder it may read in.
    pub read_root: String,
    /// The option for the folder it may write in.
    pub write_root: String,
}

/// The paths among a tool's arguments, by the arguments' names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFiles {
    /// Arguments that name files or folders it reads (a text, or a list of texts): each must exist.
    pub reads: Vec<String>,
    /// The argument that names the file it writes: it must not exist yet.
    pub target: Option<String>,
    /// Without `target` the tool writes over the file it opened, so `target` must be given.
    pub target_required: bool,
}

impl Entry {
    /// [`Entry::look_in`] with `{ProgramFiles}` replaced by the variable `ProgramFiles` as `var` reads it; a
    /// folder whose variable is not set (every system but Windows) is left out.
    pub fn look_in_dirs(&self, var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
        let base = var("ProgramFiles").map(PathBuf::from);
        self.look_in
            .iter()
            .filter_map(|dir| {
                let rest = dir.strip_prefix(PROGRAM_FILES)?;
                let base = base.clone()?;
                Some(rest.split('/').filter(|part| !part.is_empty()).fold(base, |dir, part| dir.join(part)))
            })
            .collect()
    }
}

/// The folders of every built-in entry's [`Entry::look_in`] for this system, once each, in the catalog's
/// order: where [`super::prereq::Roots::system`] looks for programs after the usual folders. None for a broken
/// built-in catalog.
pub fn look_in(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in builtin().unwrap_or_default().iter().flat_map(|entry| entry.look_in_dirs(&var)) {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Why a catalog was not read. `Display` says it in English, for the log and the `message` of `error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// No valid TOML, or fields that do not fit: the parser's message.
    Toml(String),
    /// The entry `entry` (its id as written) breaks a rule; `reason` says which.
    Invalid { entry: String, reason: &'static str },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Toml(message) => write!(f, "the catalog of MCP servers is not valid: {message}"),
            CatalogError::Invalid { entry, reason } => write!(f, "catalog entry {entry:?}: {reason}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// The built-in catalog, read and checked on first use. `Err` only when the built-in file is broken, which
/// the tests rule out.
pub fn builtin() -> Result<&'static [Entry], CatalogError> {
    static BUILTIN: OnceLock<Result<Vec<Entry>, CatalogError>> = OnceLock::new();
    BUILTIN.get_or_init(|| parse(CATALOG)).as_deref().map_err(Clone::clone)
}

/// The entries of a catalog in the format of `catalog.toml`, in its order, each checked ([`check`]). Fails on
/// the first entry that breaks a rule; unknown fields count as broken.
pub fn parse(text: &str) -> Result<Vec<Entry>, CatalogError> {
    let raw: RawCatalog = toml::from_str(text).map_err(|err| CatalogError::Toml(err.message().to_string()))?;
    let mut ids = BTreeSet::new();
    let mut entries = Vec::with_capacity(raw.server.len());
    for server in raw.server {
        let entry = entry_of(server)?;
        if !ids.insert(entry.info.id.clone()) {
            return Err(invalid(&entry.info.id, "the id is used twice"));
        }
        entries.push(entry);
    }
    Ok(entries)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    #[serde(default)]
    server: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    id: String,
    name: String,
    publisher: String,
    license: String,
    url: String,
    description: String,
    version: String,
    #[serde(default)]
    experimental: bool,
    /// `npx`, `uvx`, `docker` or the name of another program.
    requires: Vec<String>,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    secrets: Vec<RawSecret>,
    #[serde(default)]
    tools: Vec<RawTool>,
    #[serde(default)]
    look_in: Vec<String>,
    #[serde(default)]
    blocked: Vec<String>,
    #[serde(default)]
    blocked_commands: Vec<String>,
    folder: Option<RawFolder>,
}

/// `[server.folder]`: both options, or neither for a server without a bound of its own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFolder {
    read_root: Option<String>,
    write_root: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSecret {
    name: String,
    hint: String,
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTool {
    name: String,
    access: Access,
    #[serde(default)]
    reads: Vec<String>,
    target: Option<String>,
    #[serde(default)]
    target_required: bool,
}

fn invalid(entry: &str, reason: &'static str) -> CatalogError {
    CatalogError::Invalid { entry: entry.to_string(), reason }
}

/// One entry read and checked.
fn entry_of(raw: RawEntry) -> Result<Entry, CatalogError> {
    let requires = raw.requires.iter().map(|name| prerequisite_of(name)).collect();
    let secrets = raw
        .secrets
        .into_iter()
        .map(|secret| CatalogSecret { name: secret.name, hint: secret.hint, url: secret.url })
        .collect();
    let files = raw
        .tools
        .iter()
        .filter(|tool| !tool.reads.is_empty() || tool.target.is_some() || tool.target_required)
        .map(|tool| {
            let files = ToolFiles {
                reads: tool.reads.clone(),
                target: tool.target.clone(),
                target_required: tool.target_required,
            };
            (tool.name.clone(), files)
        })
        .collect();
    let tools = raw.tools.into_iter().map(|tool| CatalogTool { name: tool.name, access: tool.access }).collect();
    let (folder, bounds) = match raw.folder {
        None => (None, Ok(None)),
        Some(RawFolder { read_root: None, write_root: None }) => (Some(FolderCheck::TorroOnly), Ok(None)),
        Some(RawFolder { read_root: Some(read_root), write_root: Some(write_root) }) => {
            (Some(FolderCheck::Both), Ok(Some(Bounds { read_root, write_root })))
        }
        Some(_) => (Some(FolderCheck::Both), Err("a folder needs both read_root and write_root, or neither")),
    };
    let bounds = bounds.map_err(|reason| invalid(&raw.id, reason))?;
    let entry = Entry {
        info: CatalogEntry {
            id: raw.id,
            name: raw.name,
            publisher: raw.publisher,
            license: raw.license,
            url: raw.url,
            description: raw.description,
            version: raw.version,
            experimental: raw.experimental,
            requires,
            secrets,
            tools,
            missing: Vec::new(),
            folder,
        },
        command: raw.command,
        args: raw.args,
        env: raw.env,
        bounds,
        look_in: raw.look_in,
        blocked: raw.blocked,
        blocked_commands: raw.blocked_commands,
        files,
    };
    check(&entry).map_err(|reason| invalid(&entry.info.id, reason))?;
    Ok(entry)
}

fn prerequisite_of(name: &str) -> Prerequisite {
    match name {
        "npx" => Prerequisite::Npx,
        "uvx" => Prerequisite::Uvx,
        "docker" => Prerequisite::Docker,
        other => Prerequisite::Program { name: other.to_string() },
    }
}

/// The rules of `catalog.toml` for one entry; the reason of the first one it breaks.
fn check(entry: &Entry) -> Result<(), &'static str> {
    let info = &entry.info;
    if !valid_id(&info.id) {
        return Err("the id is not valid: a-z, 0-9 and -, up to 24 characters, starting with a letter");
    }
    let texts = [&info.name, &info.publisher, &info.license, &info.description, &info.version];
    if texts.iter().any(|text| text.trim().is_empty()) {
        return Err("name, publisher, license, description and version must not be empty");
    }
    if !info.url.starts_with("https://")
        || info.secrets.iter().any(|s| s.url.as_ref().is_some_and(|u| !u.starts_with("https://")))
    {
        return Err("a link is not https");
    }
    check_command(entry)?;
    check_variables(entry)?;
    check_tools(info)?;
    check_files(entry)
}

/// A program by name, no shell, one of the prerequisites, the version in the arguments of a program that
/// installs by it ([`VERSIONED`]), and nothing that waits on stdin.
fn check_command(entry: &Entry) -> Result<(), &'static str> {
    let command = entry.command.as_str();
    if command.is_empty() || !command.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
        return Err("the command must be a program name without a path");
    }
    if SHELLS.contains(&command.to_ascii_lowercase().trim_end_matches(".exe")) {
        return Err("the command must not be a shell");
    }
    let prerequisites = &entry.info.requires;
    if prerequisites.iter().any(|needed| program_name(needed).trim().is_empty()) {
        return Err("a prerequisite has no name");
    }
    if !prerequisites.iter().any(|needed| program_name(needed) == command) {
        return Err("the command is not among its prerequisites");
    }
    if VERSIONED.contains(&command) && !entry.args.iter().any(|arg| arg.contains(entry.info.version.as_str())) {
        return Err("no argument names the fixed version");
    }
    match command {
        "npx" if !matches!(entry.args.first().map(String::as_str), Some("-y" | "--yes")) => {
            Err("npx must start with -y, or it asks before installing")
        }
        "docker" if !docker_runs_quietly(&entry.args) => Err("docker must run with run -i --rm"),
        _ => Ok(()),
    }
}

/// `docker run -i --rm …`: `-i` keeps stdin open for MCP, `--rm` removes the container when it ends.
fn docker_runs_quietly(args: &[String]) -> bool {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    args.first().is_some_and(|first| first == "run") && (has("-i") || has("--interactive")) && has("--rm")
}

/// Valid names, none blocked, none twice, and for Docker each passed into the container with `-e NAME`
/// (without it the container never sees the variable).
fn check_variables(entry: &Entry) -> Result<(), &'static str> {
    let secrets = entry.info.secrets.iter().map(|secret| &secret.name);
    let names: Vec<&String> = entry.env.keys().chain(secrets).collect();
    if names.iter().any(|name| !valid_env_name(name)) {
        return Err("a variable has no valid name");
    }
    if names.iter().any(|name| blocked_env(name)) {
        return Err("a variable is blocked");
    }
    if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
        return Err("a variable is named twice");
    }
    if entry.command == "docker" {
        let passed: BTreeSet<&str> =
            entry.args.windows(2).filter(|pair| pair[0] == "-e").map(|pair| pair[1].as_str()).collect();
        if names.iter().any(|name| !passed.contains(name.as_str())) {
            return Err("a variable does not reach the container: pass it as -e NAME");
        }
    }
    Ok(())
}

/// Each tool a name agents can be offered, once, and never `admin`.
fn check_tools(info: &CatalogEntry) -> Result<(), &'static str> {
    if info.tools.iter().any(|tool| tool_name(&info.id, &tool.name).is_none()) {
        return Err("a tool has a name that cannot be offered");
    }
    if info.tools.iter().map(|tool| &tool.name).collect::<BTreeSet<_>>().len() != info.tools.len() {
        return Err("a tool is listed twice");
    }
    if info.tools.iter().any(|tool| tool.access == Access::Admin) {
        return Err("a tool needs read, write or delete, never admin");
    }
    Ok(())
}

/// The rules of issue #303 for entries that work on files: no argument that lifts the bound, files and a
/// required target only on tools of such entries and only where they make sense, no tool both listed and
/// blocked, blocked names a tool could have, commands with `.*` only at their end, and `look_in` only below
/// `{ProgramFiles}`.
fn check_files(entry: &Entry) -> Result<(), &'static str> {
    if let Some(bounds) = &entry.bounds
        && ![&bounds.read_root, &bounds.write_root].iter().all(|option| is_option(option))
    {
        return Err("read_root and write_root must be options such as --read-root");
    }
    if entry.info.folder.is_some() && entry.args.iter().any(|arg| UNBOUNDED_ARGS.contains(&arg.as_str())) {
        return Err("an entry with a folder must not have the arguments --connect or --root");
    }
    if entry.info.folder.is_none() && !entry.files.is_empty() {
        return Err("reads and target need a folder");
    }
    if entry.files.values().any(|files| files.target_required && files.target.is_none()) {
        return Err("target_required needs a target");
    }
    let names: Vec<&String> = entry.files.values().flat_map(|files| files.reads.iter().chain(&files.target)).collect();
    if names.iter().any(|name| name.trim().is_empty()) {
        return Err("reads and target must name arguments");
    }
    if entry.blocked.iter().any(|name| tool_name(&entry.info.id, name).is_none()) {
        return Err("a blocked tool has a name no tool can have");
    }
    if entry.blocked.iter().any(|name| entry.info.tools.iter().any(|tool| &tool.name == name)) {
        return Err("a tool is both listed and blocked");
    }
    let command_ok = |command: &String| {
        let stem = command.strip_suffix(".*").unwrap_or(command);
        !stem.is_empty() && !stem.contains('*') && !stem.chars().any(char::is_whitespace)
    };
    if !entry.blocked_commands.iter().all(command_ok) {
        return Err("a blocked command must be a name, with .* only at its end");
    }
    let below_program_files = |dir: &String| {
        dir.strip_prefix(PROGRAM_FILES).and_then(|rest| rest.strip_prefix('/')).is_some_and(|rest| {
            !rest.is_empty() && rest.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        }) && dir.matches('{').count() == 1
    };
    if !entry.look_in.iter().all(below_program_files) {
        return Err("look_in must name folders below {ProgramFiles}/");
    }
    Ok(())
}

/// `--name`: an option, not a value.
fn is_option(text: &str) -> bool {
    text.strip_prefix("--")
        .is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid entry; the tests below break one rule each.
    const GOOD: &str = r#"
        [[server]]
        id = "time"
        name = "Time"
        publisher = "Model Context Protocol"
        license = "MIT"
        url = "https://example.org/time"
        description = "Tells the time."
        version = "1.2.3"
        requires = ["uvx"]
        command = "uvx"
        args = ["mcp-server-time@1.2.3"]
        tools = [{ name = "get_current_time", access = "read" }]
    "#;

    fn reason(text: &str) -> &'static str {
        match parse(text) {
            Err(CatalogError::Invalid { reason, .. }) => reason,
            other => panic!("expected a broken entry, got {other:?}"),
        }
    }

    #[test]
    fn the_built_in_catalog_is_valid() {
        let entries = builtin().expect("valid");
        let ids: Vec<&str> = entries.iter().map(|entry| entry.info.id.as_str()).collect();
        assert_eq!(ids, ["time", "sequential-thinking", "github", "wordcraft", "deckcraft", "gridcraft", "pdfcraft"]);
        let github = &entries[2];
        assert_eq!(github.info.requires, [Prerequisite::Docker]);
        assert_eq!(
            github.info.secrets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["GITHUB_PERSONAL_ACCESS_TOKEN"]
        );
        assert!(github.args.iter().any(|arg| arg == "ghcr.io/github/github-mcp-server:2.0.3"));
        let thinking = &entries[1];
        assert_eq!((thinking.command.as_str(), thinking.args[0].as_str()), ("npx", "-y"));
        for entry in entries {
            assert!(entry.info.missing.is_empty() && !entry.info.tools.is_empty(), "{}", entry.info.id);
        }
        for entry in &entries[..3] {
            assert_eq!(entry.info.folder, None, "{}", entry.info.id);
            assert!(entry.blocked.is_empty() && entry.files.is_empty() && entry.look_in.is_empty());
        }
    }

    /// Issue #303: the four ArtCraft apps, each started as `<app>-cli mcp` without `--compact`, `--connect` or
    /// `--root`, bounded by both options but GridCraft, with their blocked tools and the saves that need a path.
    #[test]
    fn the_artcraft_entries_are_as_planned() {
        let entries = builtin().expect("valid");
        let entry = |id: &str| entries.iter().find(|entry| entry.info.id == id).expect("in the catalog");
        let bounds =
            Bounds { read_root: "--automation-read-root".into(), write_root: "--automation-write-root".into() };
        let apps = [
            ("wordcraft", "WordCraft", FolderCheck::Both, "save_document"),
            ("deckcraft", "DeckCraft", FolderCheck::Both, "save_presentation"),
            ("gridcraft", "GridCraft", FolderCheck::TorroOnly, "save_workbook"),
            ("pdfcraft", "PdfCraft", FolderCheck::Both, "doc_save"),
        ];
        for (id, name, folder, save) in apps {
            let app = entry(id);
            assert_eq!(app.command, format!("{id}-cli"), "{id}");
            assert_eq!(app.args, ["mcp"], "{id}");
            assert_eq!(app.info.requires, [Prerequisite::Program { name: format!("{id}-cli") }]);
            assert_eq!(app.info.folder, Some(folder), "{id}");
            assert_eq!(app.bounds, (folder == FolderCheck::Both).then(|| bounds.clone()), "{id}");
            assert_eq!(app.look_in, [format!("{{ProgramFiles}}/{name}")]);
            assert_eq!(app.info.url, format!("https://github.com/storytold/{id}/releases"));
            assert!(app.info.experimental && app.info.license == "MIT OR Apache-2.0", "{id}");
            let saving = &app.files[save];
            assert_eq!((saving.target.as_deref(), saving.target_required), (Some("path"), true), "{id}");
            assert!(!app.blocked.is_empty(), "{id}");
            assert!(app.info.tools.iter().all(|tool| !app.blocked.contains(&tool.name)), "{id}");
        }
        let blocked = |id: &str| entry(id).blocked.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(blocked("wordcraft"), ["screenshot", "click", "key", "ui_inspect"]);
        assert_eq!(blocked("deckcraft"), ["screenshot", "key", "pointer", "call_app"]);
        assert_eq!(blocked("gridcraft"), ["screenshot", "execute_command"]);
        assert_eq!(
            blocked("pdfcraft"),
            ["sign_trust", "sign_id_create", "js_run", "sign_document", "sign_keychain_ids", "sign_windows_ids"]
        );
        assert_eq!(entry("wordcraft").blocked_commands, ["ui.*"]);
        assert_eq!(entry("deckcraft").blocked_commands, ["ui.*"]);
        assert_eq!(entry("pdfcraft").blocked_commands, ["tools.js_console", "sign.digital"]);
        // Every file GridCraft writes waits for a person: it has no bound of its own.
        let access = |id: &str, tool: &str| entry(id).info.tools.iter().find(|t| t.name == tool).map(|t| t.access);
        assert_eq!(access("gridcraft", "save_workbook"), Some(Access::Delete));
        for (id, tool) in [("wordcraft", "execute"), ("deckcraft", "run_command"), ("deckcraft", "export")] {
            assert_eq!(access(id, tool), Some(Access::Delete), "{id} {tool}");
        }
        // PdfCraft without --compact: 181 tools = 50 read + 16 delete + 109 write + 6 blocked.
        let pdf = entry("pdfcraft");
        let count = |wanted: Access| pdf.info.tools.iter().filter(|tool| tool.access == wanted).count();
        assert_eq!((count(Access::Read), count(Access::Delete), count(Access::Write)), (50, 16, 109));
        assert_eq!(pdf.info.tools.len() + pdf.blocked.len(), 181);
        for tool in ["doc_batch", "tool_call", "command_run", "doc_print", "doc_split", "page_extract"] {
            assert_eq!(access("pdfcraft", tool), Some(Access::Delete), "{tool}");
        }
        for tool in ["comment_add", "bookmark_add", "form_fill", "page_delete", "redact_apply", "doc_save"] {
            assert_eq!(access("pdfcraft", tool), Some(Access::Write), "{tool}");
        }
        let optional = &pdf.files["doc_combine"];
        assert_eq!((optional.target.as_deref(), optional.target_required), (Some("out"), false));
        assert_eq!(pdf.files["doc_combine"].reads, ["paths"]);
        assert_eq!(pdf.files["measure_export"].target.as_deref(), Some("out"));
    }

    /// Issue #303: `look_in` with `{ProgramFiles}` finds a program there, here in a temporary folder standing in
    /// for it; without the variable the folder is left out.
    #[test]
    fn a_program_is_found_in_look_in() {
        let dir = tempfile::tempdir().expect("temp dir");
        let entries = parse(&with_look_in("{ProgramFiles}/Word Craft/bin")).expect("valid");
        let program_files = dir.path().to_path_buf();
        let dirs = entries[0].look_in_dirs(|name| (name == "ProgramFiles").then(|| program_files.clone().into()));
        let folder = dir.path().join("Word Craft").join("bin");
        assert_eq!(dirs, std::slice::from_ref(&folder));
        std::fs::create_dir_all(&folder).expect("folder");
        let program = folder.join(if cfg!(windows) { "wordcraft-cli.exe" } else { "wordcraft-cli" });
        std::fs::write(&program, "#!/bin/sh\n").expect("program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        let roots = super::super::prereq::Roots { path: Vec::new(), home: None, fixed: dirs };
        assert_eq!(super::super::prereq::find("wordcraft-cli", &roots).map(|found| found.program), Some(program));
        assert_eq!(super::super::prereq::missing(&entries[0].info.requires, &roots), Vec::new());
        assert_eq!(entries[0].look_in_dirs(|_| None), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_good_entry_reads() {
        let entries = parse(GOOD).expect("valid");
        let time = &entries[0];
        assert_eq!(time.info.requires, [Prerequisite::Uvx]);
        assert_eq!(time.info.tools, [CatalogTool { name: "get_current_time".into(), access: Access::Read }]);
        assert!(!time.info.experimental);
        // Without the fields of issue #303 an entry reads as before.
        assert_eq!(time.info.folder, None);
        assert!(time.bounds.is_none() && time.files.is_empty() && time.look_in.is_empty());
        assert!(time.blocked.is_empty() && time.blocked_commands.is_empty());
        assert_eq!(parse("").expect("empty catalog"), Vec::new());
    }

    /// An entry of a program a person installs, with a folder (issue #303); the tests below break one rule each.
    const FILES: &str = r#"
        [[server]]
        id = "wordcraft"
        name = "WordCraft"
        publisher = "Storytold"
        license = "MIT"
        url = "https://example.org/wordcraft"
        description = "Edits documents."
        version = "0.11.0"
        requires = ["wordcraft-cli"]
        command = "wordcraft-cli"
        args = ["mcp"]
        blocked = ["screenshot"]
        blocked_commands = ["ui.*", "file.print"]
        tools = [
            { name = "open_document", access = "read", reads = ["path"] },
            { name = "save_document", access = "write", target = "path", target_required = true },
        ]
        [server.folder]
        read_root = "--read-root"
        write_root = "--write-root"
    "#;

    /// [`FILES`] with `look_in = [dir]`.
    fn with_look_in(dir: &str) -> String {
        FILES.replace("blocked = [", &format!("look_in = [\"{dir}\"]\n        blocked = ["))
    }

    #[test]
    fn an_entry_with_a_folder_reads() {
        let entries = parse(FILES).expect("valid");
        let word = &entries[0];
        assert_eq!(word.info.folder, Some(FolderCheck::Both));
        assert_eq!(word.bounds, Some(Bounds { read_root: "--read-root".into(), write_root: "--write-root".into() }));
        assert_eq!(word.files["open_document"], ToolFiles { reads: vec!["path".into()], ..Default::default() });
        let save = ToolFiles { reads: Vec::new(), target: Some("path".into()), target_required: true };
        assert_eq!(word.files["save_document"], save);
        assert_eq!(word.blocked, ["screenshot"]);
        // No version in the arguments: the person installs the program.
        assert_eq!(word.args, ["mcp"]);
        let own = FILES.replace("read_root = \"--read-root\"\n        write_root = \"--write-root\"\n", "");
        let gridcraft = parse(&own).expect("valid");
        assert_eq!((gridcraft[0].info.folder, gridcraft[0].bounds.clone()), (Some(FolderCheck::TorroOnly), None));
    }

    #[test]
    fn broken_entries_with_a_folder_are_refused() {
        let with = |from: &str, to: &str| {
            assert!(FILES.contains(from), "{from}");
            FILES.replace(from, to)
        };
        for arg in ["--connect", "--root"] {
            assert_eq!(
                reason(&with("args = [\"mcp\"]", &format!("args = [\"mcp\", \"{arg}\"]"))),
                "an entry with a folder must not have the arguments --connect or --root"
            );
        }
        let no_folder =
            with("[server.folder]\n        read_root = \"--read-root\"\n        write_root = \"--write-root\"\n", "");
        assert_eq!(reason(&no_folder), "reads and target need a folder");
        assert_eq!(
            reason(&with("write_root = \"--write-root\"\n", "")),
            "a folder needs both read_root and write_root, or neither"
        );
        assert_eq!(
            reason(&with("\"--read-root\"", "\"/home\"")),
            "read_root and write_root must be options such as --read-root"
        );
        assert_eq!(
            reason(&with("target = \"path\", target_required = true", "target_required = true")),
            "target_required needs a target"
        );
        assert_eq!(reason(&with("reads = [\"path\"]", "reads = [\" \"]")), "reads and target must name arguments");
        assert_eq!(
            reason(&with("blocked = [\"screenshot\"]", "blocked = [\"open_document\"]")),
            "a tool is both listed and blocked"
        );
        assert_eq!(
            reason(&with("blocked = [\"screenshot\"]", "blocked = [\"take a screenshot\"]")),
            "a blocked tool has a name no tool can have"
        );
        for command in ["\"ui.*.x\"", "\"*\"", "\"\"", "\"ui click\""] {
            assert_eq!(
                reason(&with("\"ui.*\"", command)),
                "a blocked command must be a name, with .* only at its end",
                "{command}"
            );
        }
        for dir in ["C:/WordCraft", "{ProgramFiles}", "{ProgramFiles}/../x", "{ProgramFiles}/{APPDATA}/x"] {
            assert_eq!(reason(&with_look_in(dir)), "look_in must name folders below {ProgramFiles}/", "{dir}");
        }
        // A program that installs by version still needs it in its arguments.
        let npx = GOOD
            .replace("requires = [\"uvx\"]", "requires = [\"npx\"]")
            .replace("command = \"uvx\"", "command = \"npx\"")
            .replace("args = [\"mcp-server-time@1.2.3\"]", "args = [\"-y\", \"server\"]");
        assert_eq!(reason(&npx), "no argument names the fixed version");
    }

    #[test]
    fn broken_entries_are_refused() {
        let with = |from: &str, to: &str| {
            assert!(GOOD.contains(from), "{from}");
            GOOD.replace(from, to)
        };
        assert!(matches!(parse("[[server]]\nid = 1"), Err(CatalogError::Toml(_))));
        assert!(matches!(parse(&with("version = ", "colour = \"red\"\nversion = ")), Err(CatalogError::Toml(_))));
        assert!(matches!(parse(&with("\"read\"", "\"sometimes\"")), Err(CatalogError::Toml(_))));
        assert!(reason(&with("id = \"time\"", "id = \"Time_1\"")).starts_with("the id is not valid"));
        assert_eq!(reason(&format!("{GOOD}{GOOD}")), "the id is used twice");
        assert!(reason(&with("license = \"MIT\"", "license = \" \"")).ends_with("must not be empty"));
        assert_eq!(reason(&with("https://example.org/time", "http://example.org/time")), "a link is not https");
        assert_eq!(
            reason(&with("command = \"uvx\"", "command = \"/bin/uvx\"")),
            "the command must be a program name without a path"
        );
        assert_eq!(
            reason(&with(
                "requires = [\"uvx\"]\n        command = \"uvx\"",
                "requires = [\"sh\"]\n        command = \"sh\""
            )),
            "the command must not be a shell"
        );
        assert_eq!(
            reason(&with("requires = [\"uvx\"]", "requires = [\"npx\"]")),
            "the command is not among its prerequisites"
        );
        assert_eq!(reason(&with("@1.2.3", "")), "no argument names the fixed version");
        assert_eq!(
            reason(&with("name = \"get_current_time\"", "name = \"get current time\"")),
            "a tool has a name that cannot be offered"
        );
        assert_eq!(
            reason(&with("tools = [", "tools = [{ name = \"get_current_time\", access = \"write\" }, ")),
            "a tool is listed twice"
        );
        assert_eq!(reason(&with("\"read\"", "\"admin\"")), "a tool needs read, write or delete, never admin");
    }

    #[test]
    fn programs_that_ask_on_stdin_are_refused() {
        let npx = GOOD
            .replace("requires = [\"uvx\"]", "requires = [\"npx\"]")
            .replace("command = \"uvx\"", "command = \"npx\"");
        assert_eq!(reason(&npx), "npx must start with -y, or it asks before installing");
        parse(&npx.replace("args = [", "args = [\"-y\", ")).expect("with -y");
        let docker = GOOD
            .replace("requires = [\"uvx\"]", "requires = [\"docker\"]")
            .replace("command = \"uvx\"", "command = \"docker\"")
            .replace("args = [\"mcp-server-time@1.2.3\"]", "args = [\"run\", \"--rm\", \"image:1.2.3\"]");
        assert_eq!(reason(&docker), "docker must run with run -i --rm");
        parse(&docker.replace("\"--rm\"", "\"-i\", \"--rm\"")).expect("with -i");
    }

    #[test]
    fn variables_are_checked() {
        let with_env = |env: &str| GOOD.replace("tools = [", &format!("env = {{ {env} }}\n        tools = ["));
        parse(&with_env("TZ = \"Europe/Berlin\"")).expect("a plain variable");
        assert_eq!(reason(&with_env("LD_PRELOAD = \"/tmp/x.so\"")), "a variable is blocked");
        assert_eq!(reason(&with_env("torro_token = \"x\"")), "a variable is blocked");
        assert_eq!(reason(&with_env("\"A B\" = \"x\"")), "a variable has no valid name");
        let secret = "\n        [[server.secrets]]\n        name = \"TZ\"\n        hint = \"x\"\n";
        assert_eq!(reason(&format!("{}{secret}", with_env("TZ = \"UTC\""))), "a variable is named twice");
        let docker = |args: &str| {
            format!(
                "{}{}",
                GOOD.replace("requires = [\"uvx\"]", "requires = [\"docker\"]")
                    .replace("command = \"uvx\"", "command = \"docker\"")
                    .replace("args = [\"mcp-server-time@1.2.3\"]", &format!("args = [{args}]")),
                secret.replace("\"TZ\"", "\"TOKEN\"")
            )
        };
        assert_eq!(
            reason(&docker("\"run\", \"-i\", \"--rm\", \"image:1.2.3\"")),
            "a variable does not reach the container: pass it as -e NAME"
        );
        parse(&docker("\"run\", \"-i\", \"--rm\", \"-e\", \"TOKEN\", \"image:1.2.3\"")).expect("passed with -e");
        let insecure = format!(
            "{GOOD}\n        [[server.secrets]]\n        name = \"T\"\n        hint = \"x\"\n        url = \"http://x.org\"\n"
        );
        assert_eq!(reason(&insecure), "a link is not https");
    }
}
