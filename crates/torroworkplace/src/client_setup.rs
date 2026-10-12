//! Putting Torro Workplace's MCP server into the configuration of a coding agent or app, and taking
//! it out again (task A.7 in `docs/tasks/api-mcp.md`, concept in `docs/api.md`, "Steuerzentrale:
//! Abschnitt „Agents & API“"). The "Connect agent" wizard (`ui/agent_wizard.rs`) and "Disconnect…"
//! on an agent's page (`ui/agents.rs`) use it.
//!
//! - **Detect** ([`detect`]): which of the known clients ([`Target`]) are on this computer and how
//!   each is reached ([`Method`]): Claude Code and Codex through their own command (`claude mcp
//!   add`, `codex mcp add`) when it is on the `PATH`, else through their files (`~/.claude.json`,
//!   `~/.codex/config.toml`); Claude Desktop and Cursor through their JSON files.
//! - **Connect** ([`connect`]): writes the entry `torroworkplace` that runs `<this program> mcp`
//!   with the client's key in `TORRO_TOKEN`. Entries of other servers and every other setting stay
//!   as they are: JSON is merged, never replaced; TOML is edited as text, only our own table.
//!   A file is written atomically with mode 0600 (it holds a key now), and its previous content is
//!   kept next to it as `<file>.torroworkplace-backup` (0600 too). A file that cannot be read is an
//!   error and is never overwritten. Writing the same entry again changes nothing.
//! - **Disconnect** ([`disconnect`]): removes the entry again, but only for a client the wizard set
//!   up. Which one that was is noted in `client_setup.json` in the core's config directory (0600,
//!   never a key): one [`Record`] per target with the client's id and the way it was written. A
//!   target holds one `torroworkplace` entry, so connecting a second client there takes the record
//!   over; disconnecting the first one then leaves the entry of the second alone. Deriving the
//!   target from the client's kind alone would remove the newer client's entry.
//!
//! Why the control centre writes these files and not the core: the key leaves the core once, in
//! the answer to `clients.create_client`, and the files belong to the person's other programs, not
//! to the suite. The core never needs to know them.
//!
//! The key is never logged and never part of an error. The command line of `claude mcp add` and
//! `codex mcp add` carries it for the moment the command runs (that is how both CLIs take it); the
//! same user can read the file it ends up in anyway.
//!
//! Every path comes from an [`Env`], so tests point it at a temporary directory, and every command
//! runs through a [`Runner`], so tests never start the real `claude` or `codex`. Later setup steps
//! (the hooks of task E2.3) add a [`Target`]-like table and functions next to [`add`]/[`remove`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::paths::AppPaths;
use torro_ipc::IssuedKey;

use crate::clients::{create_private_dir, write_private};

/// The name the server is registered under in every client.
pub const SERVER_NAME: &str = "torroworkplace";
/// The variable that carries the client's key (`torroworkplace mcp` reads it).
pub const TOKEN_VARIABLE: &str = "TORRO_TOKEN";
/// The record of what the wizard wrote, in [`Env::config_dir`].
pub const RECORD_FILE: &str = "client_setup.json";
/// Appended to a client's file name for the copy of its previous content.
pub const BACKUP_SUFFIX: &str = ".torroworkplace-backup";
/// How long a client's command may take before it is given up (and killed).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Where everything lives, stated instead of read from the process, so tests can use temporary
/// directories. [`Env::current`] is the real one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    /// The home directory: `~/.claude.json`, `~/.codex`, `~/.cursor` are below it.
    pub home: PathBuf,
    /// Where desktop apps keep their settings: `~/.config` on Linux, `~/Library/Application
    /// Support` on macOS, `%APPDATA%` on Windows. Claude Desktop's folder `Claude` is below it.
    pub app_config: PathBuf,
    /// The directories commands are looked for in, in order (the `PATH`).
    pub path: Vec<PathBuf>,
    /// The program the clients start with the argument `mcp`: this `torroworkplace`.
    pub program: PathBuf,
    /// The core's config directory, where [`RECORD_FILE`] lives.
    pub config_dir: PathBuf,
}

impl Env {
    /// This process's view: its home, `PATH` and executable, and the core's config directory.
    /// `None` without a home directory.
    pub fn current() -> Option<Env> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)?;
        let path = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(AppId::WORKPLACE.binary));
        Some(Env {
            app_config: app_config(&home),
            path,
            program,
            config_dir: AppPaths::for_app(AppId::WORKPLACE)?.config,
            home,
        })
    }

    /// The first executable called `name` (on Windows `name.exe` or `name.cmd`) in `extra`, then
    /// on the `PATH`.
    fn find_command(&self, name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
        let names: Vec<String> =
            if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd")] } else { vec![name.to_string()] };
        let on_path = self.path.iter().flat_map(|dir| names.iter().map(move |name| dir.join(name)));
        extra.iter().cloned().chain(on_path).find(|candidate| is_executable(candidate))
    }

    /// `path` for people: below the home directory as `~/…`.
    pub fn shown(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

#[cfg(target_os = "macos")]
fn app_config(home: &Path) -> PathBuf {
    home.join("Library/Application Support")
}

#[cfg(windows)]
fn app_config(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Roaming"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn app_config(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else { return false };
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

/// A client the wizard can set up by itself. "Script" is none: its key is shown to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    ClaudeCode,
    Codex,
    ClaudeDesktop,
    Cursor,
}

impl Target {
    /// In the order the wizard lists them.
    pub const ALL: [Target; 4] = [Target::ClaudeCode, Target::Codex, Target::ClaudeDesktop, Target::Cursor];

    /// The client kind for `clients.create_client` (`ClientInfo::kind`).
    pub fn kind(self) -> &'static str {
        match self {
            Target::ClaudeCode => "claude-code",
            Target::Codex => "codex",
            Target::ClaudeDesktop => "claude-desktop",
            Target::Cursor => "cursor",
        }
    }

    /// The product's name, also the new client's name. Names are not translated.
    pub fn label(self) -> &'static str {
        match self {
            Target::ClaudeCode => "Claude Code",
            Target::Codex => "Codex",
            Target::ClaudeDesktop => "Claude Desktop",
            Target::Cursor => "Cursor",
        }
    }

    /// How this client is reached on this computer, `None` when it is not found: its command if
    /// it has one on the `PATH` (Claude Code also in `~/.claude/local` and `~/.local/bin`), else
    /// its file when its folder is there (`~/.claude.json` counts for Claude Code).
    pub fn locate(self, env: &Env) -> Option<Method> {
        let home = &env.home;
        let folder = |dir: PathBuf, file: &str| dir.is_dir().then(|| dir.join(file));
        match self {
            Target::ClaudeCode => {
                let extra = [home.join(".claude/local/claude"), home.join(".local/bin/claude")];
                if let Some(executable) = env.find_command("claude", &extra) {
                    return Some(Method::Cli { tool: Tool::Claude, executable });
                }
                let file = home.join(".claude.json");
                (file.is_file() || home.join(".claude").is_dir()).then_some(Method::Json { file, stdio_type: true })
            }
            Target::Codex => match env.find_command("codex", &[]) {
                Some(executable) => Some(Method::Cli { tool: Tool::Codex, executable }),
                None => folder(home.join(".codex"), "config.toml").map(|file| Method::Toml { file }),
            },
            Target::ClaudeDesktop => folder(env.app_config.join("Claude"), "claude_desktop_config.json")
                .map(|file| Method::Json { file, stdio_type: false }),
            Target::Cursor => {
                folder(home.join(".cursor"), "mcp.json").map(|file| Method::Json { file, stdio_type: false })
            }
        }
    }
}

/// A client's own command that registers MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    /// The command's name, as people type it.
    pub fn command(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
        }
    }

    /// `claude mcp add --scope user torroworkplace -e TORRO_TOKEN=… -- <program> mcp`: user scope,
    /// because the default (the current project) would be nowhere useful; the name before `-e`,
    /// because `-e` takes several values. `codex mcp add torroworkplace --env TORRO_TOKEN=… --
    /// <program> mcp`.
    fn add_args(self, server: &Server<'_>) -> Vec<String> {
        let env = format!("{TOKEN_VARIABLE}={}", server.key);
        let mut args: Vec<String> = match self {
            Tool::Claude => ["mcp", "add", "--scope", "user", SERVER_NAME, "-e"].map(String::from).to_vec(),
            Tool::Codex => ["mcp", "add", SERVER_NAME, "--env"].map(String::from).to_vec(),
        };
        args.push(env);
        args.extend(["--".to_string(), server.program.display().to_string(), "mcp".to_string()]);
        args
    }

    /// `claude mcp remove torroworkplace --scope user`, `codex mcp remove torroworkplace`.
    fn remove_args(self) -> Vec<String> {
        let args: &[&str] = match self {
            Tool::Claude => &["mcp", "remove", SERVER_NAME, "--scope", "user"],
            Tool::Codex => &["mcp", "remove", SERVER_NAME],
        };
        args.iter().map(|arg| arg.to_string()).collect()
    }
}

/// How the entry gets into a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "way")]
pub enum Method {
    /// The client's own command does it; its file is never touched by hand.
    Cli { tool: Tool, executable: PathBuf },
    /// A JSON file with the servers under a top-level `mcpServers`. `stdio_type`: the entry says
    /// `"type": "stdio"`, as Claude Code's own file does.
    Json { file: PathBuf, stdio_type: bool },
    /// Codex's `config.toml`: the table `[mcp_servers.torroworkplace]`.
    Toml { file: PathBuf },
}

/// What the entry runs: `program mcp` with `key` in [`TOKEN_VARIABLE`].
#[derive(Clone, Copy)]
pub struct Server<'a> {
    pub program: &'a Path,
    pub key: &'a str,
}

/// A client found on this computer, as [`detect`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub target: Target,
    pub method: Method,
    /// Where the entry goes, for people: `~/.cursor/mcp.json`, or the command `claude`.
    pub place: String,
}

/// The known clients on this computer, in [`Target::ALL`] order. Reads nothing but the presence
/// of folders, files and commands.
pub fn detect(env: &Env) -> Vec<Found> {
    Target::ALL
        .into_iter()
        .filter_map(|target| {
            let method = target.locate(env)?;
            let place = match &method {
                Method::Cli { tool, .. } => tool.command().to_string(),
                Method::Json { file, .. } | Method::Toml { file } => env.shown(file),
            };
            Some(Found { target, method, place })
        })
        .collect()
}

/// Why a client could not be set up or cleaned up. Never holds a key.
#[derive(Debug)]
pub enum SetupError {
    /// The file is there but is not what it should be (not JSON, not an object). Left as it is.
    Unreadable(PathBuf),
    Io {
        path: PathBuf,
        err: io::Error,
    },
    /// The client's command could not be started.
    NotStarted(Tool),
    /// The client's command ended with an error.
    Failed(Tool),
    /// The client's command did not end within [`COMMAND_TIMEOUT`].
    TimedOut(Tool),
    /// No home directory, so nothing is known.
    NoHome,
}

impl SetupError {
    fn io(path: &Path) -> impl FnOnce(io::Error) -> SetupError + '_ {
        move |err| SetupError::Io { path: path.to_path_buf(), err }
    }

    /// The error for people, translated.
    pub fn message(&self) -> String {
        match self {
            SetupError::Unreadable(path) => {
                trf("{file} could not be read, so it was left as it is.", &[("file", &path.display())])
            }
            SetupError::Io { path, err } => {
                trf("{file} could not be written: {error}", &[("file", &path.display()), ("error", err)])
            }
            SetupError::NotStarted(tool) => {
                trf("The {command} command could not be started.", &[("command", &tool.command())])
            }
            SetupError::Failed(tool) => {
                trf("The {command} command reported an error.", &[("command", &tool.command())])
            }
            SetupError::TimedOut(tool) => trf("The {command} command did not finish.", &[("command", &tool.command())]),
            SetupError::NoHome => tr("No home directory").to_string(),
        }
    }
}

/// How a command run went, as a [`Runner`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    NotStarted,
    Failed,
    TimedOut,
}

impl RunError {
    fn of(self, tool: Tool) -> SetupError {
        match self {
            RunError::NotStarted => SetupError::NotStarted(tool),
            RunError::Failed => SetupError::Failed(tool),
            RunError::TimedOut => SetupError::TimedOut(tool),
        }
    }
}

/// Runs a client's command. Implementations must not print or log `args`: they hold the key.
pub trait Runner {
    /// Runs `program` with `args`, without input and without showing its output, and reports
    /// whether it ended successfully.
    fn run(&self, program: &Path, args: &[String]) -> Result<(), RunError>;
}

/// The real [`Runner`]: starts the program, waits at most [`COMMAND_TIMEOUT`], kills it then.
pub struct System;

impl Runner for System {
    fn run(&self, program: &Path, args: &[String]) -> Result<(), RunError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| RunError::NotStarted)?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return if status.success() { Ok(()) } else { Err(RunError::Failed) },
                Ok(None) if started.elapsed() < COMMAND_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::TimedOut);
                }
                Err(_) => return Err(RunError::Failed),
            }
        }
    }
}

// ----- Writing and removing one entry ---------------------------------------------------------

/// Puts the entry for `server` into the client reached by `method`, replacing an older entry of
/// ours. A command first removes an old entry (both CLIs refuse a name that exists; a failing
/// removal is fine, no entry is what it aims for), then adds. A file is merged as the module doc
/// says; writing what is there already writes nothing.
pub fn add(method: &Method, server: &Server<'_>, runner: &dyn Runner) -> Result<(), SetupError> {
    match method {
        Method::Cli { tool, executable } => {
            let _ = runner.run(executable, &tool.remove_args());
            runner.run(executable, &tool.add_args(server)).map_err(|err| err.of(*tool))
        }
        Method::Json { file, stdio_type } => {
            let before = read_text(file)?;
            let mut root = parse_json(file, before.as_deref())?;
            let servers = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
            if !servers.is_object() {
                return Err(SetupError::Unreadable(file.clone()));
            }
            servers[SERVER_NAME] = json_entry(server, *stdio_type);
            replace(file, before.as_deref(), &json_text(&root))
        }
        Method::Toml { file } => {
            let before = read_text(file)?;
            let rest = without_our_table(before.as_deref().unwrap_or_default());
            let separator = if rest.is_empty() { "" } else { "\n" };
            replace(file, before.as_deref(), &format!("{rest}{separator}{}", toml_table(server)))
        }
    }
}

/// Takes our entry out of the client reached by `method`; everything else stays. No entry (or no
/// file) is fine and writes nothing.
pub fn remove(method: &Method, runner: &dyn Runner) -> Result<(), SetupError> {
    match method {
        Method::Cli { tool, executable } => runner.run(executable, &tool.remove_args()).map_err(|err| err.of(*tool)),
        Method::Json { file, .. } => {
            let Some(before) = read_text(file)? else { return Ok(()) };
            let mut root = parse_json(file, Some(&before))?;
            let removed = root.get_mut("mcpServers").and_then(Value::as_object_mut).and_then(|s| s.remove(SERVER_NAME));
            match removed {
                Some(_) => replace(file, Some(&before), &json_text(&root)),
                None => Ok(()),
            }
        }
        Method::Toml { file } => {
            let Some(before) = read_text(file)? else { return Ok(()) };
            replace(file, Some(&before), &without_our_table(&before))
        }
    }
}

/// The JSON entry: `{"command": program, "args": ["mcp"], "env": {"TORRO_TOKEN": key}}`, with
/// `"type": "stdio"` for Claude Code.
fn json_entry(server: &Server<'_>, stdio_type: bool) -> Value {
    let mut entry = json!({
        "command": server.program.display().to_string(),
        "args": ["mcp"],
        "env": { TOKEN_VARIABLE: server.key },
    });
    if stdio_type {
        entry["type"] = json!("stdio");
    }
    entry
}

/// The file's text; `None` when there is no file.
fn read_text(file: &Path) -> Result<Option<String>, SetupError> {
    match fs::read(file) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| SetupError::Unreadable(file.to_path_buf())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(SetupError::io(file)(err)),
    }
}

/// The JSON object in `text`. No file and an empty one (Cursor ships one) are a fresh start;
/// anything but an object is [`SetupError::Unreadable`].
fn parse_json(file: &Path, text: Option<&str>) -> Result<Map<String, Value>, SetupError> {
    match text.map(str::trim) {
        None | Some("") => Ok(Map::new()),
        Some(text) => match serde_json::from_str(text) {
            Ok(Value::Object(root)) => Ok(root),
            _ => Err(SetupError::Unreadable(file.to_path_buf())),
        },
    }
}

fn json_text(root: &Map<String, Value>) -> String {
    // Serialising a map of values cannot fail; an empty object would only hide a bug.
    serde_json::to_string_pretty(root).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

/// Writes `after` over `file` unless it is what is there already: the previous content first to
/// the backup next to it, then the new one atomically, both 0600. A link (a dotfiles repository)
/// is followed, so the link stays a link. A missing folder is created.
fn replace(file: &Path, before: Option<&str>, after: &str) -> Result<(), SetupError> {
    if before == Some(after) {
        return Ok(());
    }
    let target = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if let Some(parent) = target.parent().filter(|parent| !parent.as_os_str().is_empty() && !parent.exists()) {
        fs::create_dir_all(parent).map_err(SetupError::io(parent))?;
    }
    if let Some(before) = before.filter(|before| !before.is_empty()) {
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(BACKUP_SUFFIX);
        let backup = target.with_file_name(name);
        write_private(&backup, before.as_bytes()).map_err(|_| SetupError::Io {
            path: backup.clone(),
            err: io::Error::other("the backup could not be written"),
        })?;
    }
    write_private(&target, after.as_bytes())
        .map_err(|_| SetupError::Io { path: target.clone(), err: io::Error::other("the file could not be replaced") })
}

// ----- Codex's TOML, edited as text -------------------------------------------------------------
//
// The workspace has no TOML editor, and a parse-and-print round trip would lose the person's
// comments anyway. Our entry is one table, `[mcp_servers.torroworkplace]` (the CLI may add
// `[mcp_servers.torroworkplace.env]`), so it is found by its header and cut out up to the next
// table that is not ours. Limits: a header-like line inside a multi-line string is taken for a
// header, and a config that defines `mcp_servers` as an inline table cannot take our table.

/// Our table for `server`.
fn toml_table(server: &Server<'_>) -> String {
    format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [\"mcp\"]\nenv = {{ {TOKEN_VARIABLE} = {} }}\n",
        toml_string(&server.program.display().to_string()),
        toml_string(server.key),
    )
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The key path of a table header (`[a.b]`, `[[a.b]]`, `[ "a" . b ]  # note`), quotes taken off;
/// `None` for any other line, also for a line of an array like `[1, 2],`.
fn table_path(line: &str) -> Option<Vec<String>> {
    let line = line.trim();
    let (inner, rest) = if let Some(inner) = line.strip_prefix("[[") {
        inner.split_once("]]")?
    } else {
        line.strip_prefix('[')?.split_once(']')?
    };
    let rest = rest.trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return None;
    }
    let key_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '"' | '\'' | ' ' | '.');
    if inner.trim().is_empty() || !inner.chars().all(key_char) {
        return None;
    }
    Some(inner.split('.').map(|part| part.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).collect())
}

fn is_ours(path: &[String]) -> bool {
    path.len() >= 2 && path[0] == "mcp_servers" && path[1] == SERVER_NAME
}

fn blank(line: &&str) -> bool {
    line.trim().is_empty()
}

/// `text` without our table(s). Comments right above the next table belong to that one and stay;
/// the blank line that set ours apart goes with it. Ends with one newline, or is empty.
fn without_our_table(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        match table_path(line) {
            Some(path) if is_ours(&path) => {
                if !skipping {
                    while out.last().is_some_and(blank) {
                        out.pop();
                    }
                }
                skipping = true;
                skipped.clear();
            }
            Some(_) if skipping => {
                skipping = false;
                let tail =
                    skipped.iter().rposition(|l| !blank(l) && !l.trim_start().starts_with('#')).map_or(0, |i| i + 1);
                if !out.is_empty() {
                    out.push("");
                }
                out.extend(skipped[tail..].iter().skip_while(|l| blank(l)));
                out.push(line);
            }
            _ if skipping => skipped.push(line),
            _ => out.push(line),
        }
    }
    while out.last().is_some_and(blank) {
        out.pop();
    }
    if out.is_empty() { String::new() } else { out.join("\n") + "\n" }
}

// ----- The record of what the wizard wrote ------------------------------------------------------

/// One client the wizard set up: which client got which target's entry, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The client's id (`ClientInfo::id`).
    pub client: String,
    pub target: Target,
    pub method: Method,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Records {
    #[serde(default)]
    written: Vec<Record>,
}

fn record_file(env: &Env) -> PathBuf {
    env.config_dir.join(RECORD_FILE)
}

/// The records; none without a file. An unreadable file is an error and stays as it is.
fn load_records(env: &Env) -> Result<Records, SetupError> {
    let file = record_file(env);
    match read_text(&file)? {
        None => Ok(Records::default()),
        Some(text) => serde_json::from_str(&text).map_err(|_| SetupError::Unreadable(file)),
    }
}

fn save_records(env: &Env, records: &Records) -> Result<(), SetupError> {
    let file = record_file(env);
    create_private_dir(&env.config_dir).map_err(SetupError::io(&env.config_dir))?;
    let text = serde_json::to_string_pretty(records).unwrap_or_default() + "\n";
    write_private(&file, text.as_bytes())
        .map_err(|_| SetupError::Io { path: file.clone(), err: io::Error::other("the record could not be written") })
}

/// The records of every client the wizard set up (for the tests; the wizard only adds and removes).
#[cfg(test)]
pub fn records(env: &Env) -> Result<Vec<Record>, SetupError> {
    load_records(env).map(|records| records.written)
}

/// Sets up `target` through `method` for the client `client` with its `key`: notes it first (so
/// a half-written entry can still be removed), then writes the entry ([`add`]). The note takes
/// over an older one of the same target. If writing fails, the note goes again.
pub fn connect(
    env: &Env,
    target: Target,
    method: &Method,
    client: &str,
    key: &IssuedKey,
    runner: &dyn Runner,
) -> Result<(), SetupError> {
    let mut records = load_records(env)?;
    let previous = records.written.clone();
    records.written.retain(|record| record.target != target);
    records.written.push(Record { client: client.to_string(), target, method: method.clone() });
    save_records(env, &records)?;
    let server = Server { program: &env.program, key: key.expose() };
    add(method, &server, runner).inspect_err(|_| {
        let _ = save_records(env, &Records { written: previous });
    })
}

/// Removes the entry the wizard wrote for the client `client` ([`remove`]) and its note. Returns
/// the target it was in, `None` when the wizard did not set that client up (nothing to do).
pub fn disconnect(env: &Env, client: &str, runner: &dyn Runner) -> Result<Option<Target>, SetupError> {
    let mut records = load_records(env)?;
    let Some(index) = records.written.iter().position(|record| record.client == client) else { return Ok(None) };
    let record = records.written.remove(index);
    remove(&record.method, runner)?;
    save_records(env, &records)?;
    Ok(Some(record.target))
}

// ----- Jobs of the control centre ---------------------------------------------------------------

/// What the control centre asks for (`ui::Action::Setup`); it runs on a blocking thread
/// ([`perform`]), in `--demo` not at all ([`demo`]).
#[derive(Debug, Clone)]
pub enum Job {
    /// Which clients are on this computer: [`Outcome::Detected`].
    Detect,
    /// [`connect`]: [`Outcome::Connected`].
    Connect { target: Target, method: Method, client: String, key: IssuedKey },
    /// [`disconnect`]: [`Outcome::Disconnected`].
    Disconnect { client: String },
}

/// How a [`Job`] went; errors as [`SetupError::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Detected(Vec<Found>),
    Connected(Result<(), String>),
    /// The target whose entry was removed; `None` when the client was not set up by the wizard.
    Disconnected(Result<Option<Target>, String>),
}

/// Runs `job` in `env` (`None`: no home directory) with `runner`.
pub fn run(job: Job, env: Option<&Env>, runner: &dyn Runner) -> Outcome {
    let no_home = || SetupError::NoHome.message();
    match job {
        Job::Detect => Outcome::Detected(env.map(detect).unwrap_or_default()),
        Job::Connect { target, method, client, key } => Outcome::Connected(match env {
            Some(env) => connect(env, target, &method, &client, &key, runner).map_err(|err| err.message()),
            None => Err(no_home()),
        }),
        Job::Disconnect { client } => Outcome::Disconnected(match env {
            Some(env) => disconnect(env, &client, runner).map_err(|err| err.message()),
            None => Err(no_home()),
        }),
    }
}

/// Runs `job` on a blocking thread with the real [`Env`] and [`System`].
pub async fn perform(job: Job) -> Outcome {
    let failed = |job: &Job| match job {
        Job::Detect => Outcome::Detected(Vec::new()),
        Job::Connect { .. } => Outcome::Connected(Err(tr("Something went wrong. Try again.").to_string())),
        Job::Disconnect { .. } => Outcome::Disconnected(Err(tr("Something went wrong. Try again.").to_string())),
    };
    let fallback = failed(&job);
    tokio::task::spawn_blocking(move || run(job, Env::current().as_ref(), &System)).await.unwrap_or(fallback)
}

/// The demo's answer to `job`: Claude Code and Codex found with their commands, Cursor with its
/// file, Claude Desktop not; connecting and disconnecting succeed without touching anything.
pub fn demo(job: &Job) -> Outcome {
    match job {
        Job::Detect => Outcome::Detected(vec![
            Found {
                target: Target::ClaudeCode,
                method: Method::Cli { tool: Tool::Claude, executable: PathBuf::from("/usr/local/bin/claude") },
                place: "claude".into(),
            },
            Found {
                target: Target::Codex,
                method: Method::Cli { tool: Tool::Codex, executable: PathBuf::from("/usr/local/bin/codex") },
                place: "codex".into(),
            },
            Found {
                target: Target::Cursor,
                method: Method::Json { file: PathBuf::from("/home/you/.cursor/mcp.json"), stdio_type: false },
                place: "~/.cursor/mcp.json".into(),
            },
        ]),
        Job::Connect { .. } => Outcome::Connected(Ok(())),
        Job::Disconnect { .. } => Outcome::Disconnected(Ok(None)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use tempfile::TempDir;
    use torro_ipc::Key;

    use super::*;

    const KEY: &str = "torro_c1_0123456789abcdef";

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/client_setup").join(name);
        fs::read_to_string(&path).expect("fixture exists")
    }

    /// An environment entirely inside a temporary directory.
    fn env(dir: &TempDir) -> Env {
        let root = dir.path();
        Env {
            home: root.join("home"),
            app_config: root.join("home/.config"),
            path: vec![root.join("bin")],
            program: PathBuf::from("/opt/torro/torroworkplace"),
            config_dir: root.join("config/torroworkplace"),
        }
    }

    fn key() -> IssuedKey {
        IssuedKey::new(Key::new(KEY))
    }

    fn server(env: &Env) -> Server<'_> {
        Server { program: &env.program, key: KEY }
    }

    /// Records every command instead of running it; fails the ones whose second word is in `fail`.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
        fail: Vec<&'static str>,
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[String]) -> Result<(), RunError> {
            self.calls.borrow_mut().push((program.to_path_buf(), args.to_vec()));
            match args.get(1) {
                Some(word) if self.fail.contains(&word.as_str()) => Err(RunError::Failed),
                _ => Ok(()),
            }
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("a parent")).expect("dir");
        fs::write(path, text).expect("written");
    }

    #[cfg(unix)]
    fn executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        write(path, "#!/bin/sh\n");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("exists").permissions().mode() & 0o777
    }

    fn json(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).expect("file")).expect("json")
    }

    #[test]
    fn nothing_is_found_in_an_empty_home() {
        let dir = TempDir::new().expect("tmp");
        assert!(detect(&env(&dir)).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn commands_win_over_files_and_folders_reveal_the_rest() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        fs::create_dir_all(env.home.join(".codex")).expect("dir");
        fs::create_dir_all(env.app_config.join("Claude")).expect("dir");
        fs::create_dir_all(env.home.join(".cursor")).expect("dir");
        write(&env.home.join(".claude.json"), "{}");
        let found = detect(&env);
        let targets: Vec<Target> = found.iter().map(|found| found.target).collect();
        assert_eq!(targets, Target::ALL, "every target, in the wizard's order");
        assert_eq!(found[0].method, Method::Json { file: env.home.join(".claude.json"), stdio_type: true });
        assert_eq!(found[1].method, Method::Toml { file: env.home.join(".codex/config.toml") });
        assert_eq!(found[3].place, "~/.cursor/mcp.json");

        executable(&dir.path().join("bin/claude"));
        executable(&dir.path().join("bin/codex"));
        write(&dir.path().join("bin/not-executable"), "");
        let found = detect(&env);
        assert_eq!(found[0].method, Method::Cli { tool: Tool::Claude, executable: dir.path().join("bin/claude") });
        assert_eq!(found[0].place, "claude");
        assert!(matches!(found[1].method, Method::Cli { tool: Tool::Codex, .. }));
    }

    #[test]
    fn json_keeps_foreign_entries_and_settings_and_is_written_once() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".config/Claude/claude_desktop_config.json");
        let original = fixture("claude_desktop_config.json");
        write(&file, &original);
        let method = Method::Json { file: file.clone(), stdio_type: false };
        add(&method, &server(&env), &Fake::default()).expect("added");

        let written = json(&file);
        let ours = &written["mcpServers"][SERVER_NAME];
        assert_eq!(ours["command"], "/opt/torro/torroworkplace");
        assert_eq!(ours["args"], json!(["mcp"]));
        assert_eq!(ours["env"][TOKEN_VARIABLE], KEY);
        assert!(ours.get("type").is_none(), "only Claude Code's file says stdio");
        let before: Value = serde_json::from_str(&original).expect("fixture json");
        assert_eq!(written["mcpServers"]["filesystem"], before["mcpServers"]["filesystem"], "a foreign server stays");
        assert_eq!(written["globalShortcut"], before["globalShortcut"], "other settings stay");

        let backup = file.with_file_name(format!("claude_desktop_config.json{BACKUP_SUFFIX}"));
        assert_eq!(fs::read_to_string(&backup).expect("backup"), original);
        #[cfg(unix)]
        assert_eq!((mode(&file), mode(&backup)), (0o600, 0o600));

        // Again with the same key: nothing changes, not even the backup.
        let text = fs::read_to_string(&file).expect("file");
        fs::write(&backup, "marker").expect("marker");
        add(&method, &server(&env), &Fake::default()).expect("again");
        assert_eq!(fs::read_to_string(&file).expect("file"), text);
        assert_eq!(fs::read_to_string(&backup).expect("backup"), "marker", "no second write");
        assert_eq!(fs::read_dir(file.parent().expect("dir")).expect("dir").count(), 2, "no temporary file left");

        remove(&method, &Fake::default()).expect("removed");
        let cleaned = json(&file);
        assert_eq!(cleaned, before, "only our entry went");
        remove(&method, &Fake::default()).expect("removing twice is fine");
    }

    #[test]
    fn claude_codes_own_file_gets_a_typed_entry() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".claude.json");
        write(&file, &fixture("claude.json"));
        let method = Method::Json { file: file.clone(), stdio_type: true };
        add(&method, &server(&env), &Fake::default()).expect("added");
        let written = json(&file);
        assert_eq!(written["mcpServers"][SERVER_NAME]["type"], "stdio");
        assert_eq!(written["mcpServers"]["github"]["type"], "http", "a foreign server stays");
        assert_eq!(written["projects"]["/home/anna/torro"]["allowedTools"], json!(["Bash"]), "projects stay");
    }

    #[test]
    fn a_missing_or_empty_file_is_a_fresh_start_and_a_broken_one_stays() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".cursor/mcp.json");
        let method = Method::Json { file: file.clone(), stdio_type: false };
        remove(&method, &Fake::default()).expect("nothing to remove");
        assert!(!file.exists(), "removing writes nothing");
        add(&method, &server(&env), &Fake::default()).expect("created with its folder");
        assert_eq!(json(&file)["mcpServers"][SERVER_NAME]["env"][TOKEN_VARIABLE], KEY);
        assert!(!file.with_file_name(format!("mcp.json{BACKUP_SUFFIX}")).exists(), "nothing to back up");

        write(&file, "");
        add(&method, &server(&env), &Fake::default()).expect("an empty file is fine");

        let broken = fixture("broken.json");
        write(&file, &broken);
        assert!(matches!(add(&method, &server(&env), &Fake::default()), Err(SetupError::Unreadable(_))));
        assert!(matches!(remove(&method, &Fake::default()), Err(SetupError::Unreadable(_))));
        assert_eq!(fs::read_to_string(&file).expect("file"), broken, "never overwritten");

        write(&file, "{\"mcpServers\": []}");
        assert!(matches!(add(&method, &server(&env), &Fake::default()), Err(SetupError::Unreadable(_))));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_config_stays_a_link() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let real = dir.path().join("dotfiles/mcp.json");
        write(&real, "{}");
        let link = env.home.join(".cursor/mcp.json");
        fs::create_dir_all(link.parent().expect("dir")).expect("dir");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        add(&Method::Json { file: link.clone(), stdio_type: false }, &server(&env), &Fake::default()).expect("added");
        assert!(fs::symlink_metadata(&link).expect("link").file_type().is_symlink());
        assert_eq!(json(&real)["mcpServers"][SERVER_NAME]["command"], "/opt/torro/torroworkplace");
    }

    #[test]
    fn toml_keeps_comments_and_foreign_tables_and_removes_cleanly() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".codex/config.toml");
        let original = fixture("codex_config.toml");
        write(&file, &original);
        let method = Method::Toml { file: file.clone() };
        add(&method, &server(&env), &Fake::default()).expect("added");
        let written = fs::read_to_string(&file).expect("file");
        assert!(written.starts_with(&original), "everything that was there stays as it was");
        assert!(written.ends_with(&format!(
            "\n[mcp_servers.torroworkplace]\ncommand = \"/opt/torro/torroworkplace\"\nargs = [\"mcp\"]\nenv = {{ TORRO_TOKEN = \"{KEY}\" }}\n"
        )));
        #[cfg(unix)]
        assert_eq!(mode(&file), 0o600);
        add(&method, &server(&env), &Fake::default()).expect("again");
        assert_eq!(fs::read_to_string(&file).expect("file"), written, "one table, written once");

        remove(&method, &Fake::default()).expect("removed");
        assert_eq!(fs::read_to_string(&file).expect("file"), original, "clean removal");
    }

    #[test]
    fn toml_takes_out_a_table_the_cli_wrote_in_the_middle() {
        let text = "model = \"o4\"\n\n[mcp_servers.torroworkplace]\ncommand = \"old\"\n\n[mcp_servers.torroworkplace.env]\nTORRO_TOKEN = \"old\"\n\n# docs server\n[mcp_servers.docs]\ncommand = \"docs\"\n";
        assert_eq!(
            without_our_table(text),
            "model = \"o4\"\n\n# docs server\n[mcp_servers.docs]\ncommand = \"docs\"\n"
        );
        let similar = "[mcp_servers.torroworkplace-old]\ncommand = \"x\"\n";
        assert_eq!(without_our_table(similar), similar, "another server whose name starts the same stays");
        assert_eq!(without_our_table("[mcp_servers.\"torroworkplace\"]\ncommand = \"x\"\n"), "");
        assert_eq!(table_path("x = [\n"), None);
        assert_eq!(table_path("  [1, 2],"), None);
        assert_eq!(table_path("[[profiles]] # many"), Some(vec!["profiles".to_string()]));
    }

    #[test]
    fn toml_strings_are_escaped() {
        assert_eq!(toml_string("C:\\Program Files\\\"x\""), "\"C:\\\\Program Files\\\\\\\"x\\\"\"");
        assert_eq!(toml_string("a\nb"), "\"a\\u000Ab\"");
    }

    #[test]
    fn commands_get_the_key_in_the_environment_and_remove_first() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let claude = PathBuf::from("/usr/bin/claude");
        let fake = Fake::default();
        add(&Method::Cli { tool: Tool::Claude, executable: claude.clone() }, &server(&env), &fake).expect("added");
        let calls = fake.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], (claude.clone(), Tool::Claude.remove_args()));
        assert_eq!(calls[0].1, ["mcp", "remove", "torroworkplace", "--scope", "user"]);
        let expected =
            format!("mcp add --scope user torroworkplace -e TORRO_TOKEN={KEY} -- /opt/torro/torroworkplace mcp");
        assert_eq!(calls[1].1.join(" "), expected);
        drop(calls);

        let codex = Fake { fail: vec!["remove"], ..Fake::default() };
        let method = Method::Cli { tool: Tool::Codex, executable: PathBuf::from("/usr/bin/codex") };
        add(&method, &server(&env), &codex).expect("a failing removal of nothing is fine");
        let expected = format!("mcp add torroworkplace --env TORRO_TOKEN={KEY} -- /opt/torro/torroworkplace mcp");
        assert_eq!(codex.calls.borrow()[1].1.join(" "), expected);
        assert_eq!(codex.calls.borrow()[0].1.join(" "), "mcp remove torroworkplace");

        let failing = Fake { fail: vec!["add"], ..Fake::default() };
        let err = add(&method, &server(&env), &failing).expect_err("add fails");
        assert!(matches!(err, SetupError::Failed(Tool::Codex)));
        assert!(!err.message().contains(KEY), "the key is in no error");
    }

    #[test]
    fn connect_notes_the_client_and_disconnect_removes_only_its_entry() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".cursor/mcp.json");
        write(&file, &fixture("cursor_mcp.json"));
        let method = Method::Json { file: file.clone(), stdio_type: false };
        let fake = Fake::default();
        connect(&env, Target::Cursor, &method, "c1", &key(), &fake).expect("connected");
        let noted = records(&env).expect("records");
        assert_eq!(noted, vec![Record { client: "c1".into(), target: Target::Cursor, method: method.clone() }]);
        let record_text = fs::read_to_string(env.config_dir.join(RECORD_FILE)).expect("record");
        assert!(!record_text.contains(KEY), "the record holds no key");
        #[cfg(unix)]
        assert_eq!(mode(&env.config_dir.join(RECORD_FILE)), 0o600);

        // A second client in the same target takes the entry over.
        connect(&env, Target::Cursor, &method, "c2", &key(), &fake).expect("connected again");
        assert_eq!(records(&env).expect("records").len(), 1);
        assert_eq!(disconnect(&env, "c1", &fake).expect("nothing to do"), None);
        assert!(json(&file)["mcpServers"].get(SERVER_NAME).is_some(), "c2's entry stays");
        assert_eq!(disconnect(&env, "c2", &fake).expect("removed"), Some(Target::Cursor));
        assert!(json(&file)["mcpServers"].get(SERVER_NAME).is_none());
        assert!(json(&file)["mcpServers"].get("linear").is_some(), "the foreign server stays");
        assert!(records(&env).expect("records").is_empty());
        assert_eq!(disconnect(&env, "c2", &fake).expect("twice"), None);
    }

    #[test]
    fn a_failed_connect_leaves_no_note_and_a_cli_disconnect_runs_remove() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let method = Method::Cli { tool: Tool::Codex, executable: PathBuf::from("/usr/bin/codex") };
        let failing = Fake { fail: vec!["add"], ..Fake::default() };
        assert!(connect(&env, Target::Codex, &method, "c1", &key(), &failing).is_err());
        assert!(records(&env).expect("records").is_empty());

        let fake = Fake::default();
        connect(&env, Target::Codex, &method, "c1", &key(), &fake).expect("connected");
        fake.calls.borrow_mut().clear();
        assert_eq!(disconnect(&env, "c1", &fake).expect("removed"), Some(Target::Codex));
        assert_eq!(fake.calls.borrow()[0].1.join(" "), "mcp remove torroworkplace");
    }

    #[test]
    fn a_damaged_record_is_left_alone() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        write(&env.config_dir.join(RECORD_FILE), "not json");
        assert!(matches!(disconnect(&env, "c1", &Fake::default()), Err(SetupError::Unreadable(_))));
        assert_eq!(fs::read_to_string(env.config_dir.join(RECORD_FILE)).expect("file"), "not json");
    }

    #[test]
    fn jobs_run_and_the_demo_touches_nothing() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        assert_eq!(run(Job::Detect, Some(&env), &Fake::default()), Outcome::Detected(Vec::new()));
        assert_eq!(
            run(Job::Disconnect { client: "x".into() }, Some(&env), &Fake::default()),
            Outcome::Disconnected(Ok(None))
        );
        let connect = Job::Connect {
            target: Target::Codex,
            method: Method::Toml { file: env.home.join(".codex/config.toml") },
            client: "c1".into(),
            key: key(),
        };
        assert!(!format!("{connect:?}").contains(KEY), "a job's Debug hides the key");
        assert!(matches!(run(connect.clone(), None, &Fake::default()), Outcome::Connected(Err(_))));
        assert_eq!(run(connect.clone(), Some(&env), &Fake::default()), Outcome::Connected(Ok(())));
        assert!(fs::read_to_string(env.home.join(".codex/config.toml")).expect("written").contains(KEY));

        assert!(matches!(demo(&Job::Detect), Outcome::Detected(found) if found.len() == 3));
        assert_eq!(demo(&connect), Outcome::Connected(Ok(())));
    }
}
