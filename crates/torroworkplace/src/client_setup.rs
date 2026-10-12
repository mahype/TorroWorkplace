//! Putting Torro Workplace's MCP server into the configuration of a coding agent or app, and taking
//! it out again (task A.7 in `docs/tasks/api-mcp.md`, concept in `docs/api.md`, "Steuerzentrale:
//! Abschnitt „Agents & API“"), and the hooks with which coding agents report to the inbox (task
//! E2.3 in `docs/tasks/eingang.md`). The "Connect agent" wizard (`ui/agent_wizard.rs`) and
//! "Disconnect…" on an agent's page (`ui/agents.rs`) use it.
//!
//! - **Detect** ([`detect`]): which of the known clients ([`Target`]) are on this computer and how
//!   each is reached ([`Method`]): Claude Code and Codex through their own command (`claude mcp
//!   add`, `codex mcp add`) when it is on the `PATH`, else through their files (`~/.claude.json`,
//!   `~/.codex/config.toml`); Gemini CLI, Claude Desktop and Cursor through their JSON files.
//! - **Connect** ([`connect`]): writes the entry `torroworkplace` that runs `<this program> mcp`
//!   with the client's key in `TORRO_TOKEN`. Entries of other servers and every other setting stay
//!   as they are: JSON is merged, never replaced; TOML is edited as text, only our own table.
//!   A file is written atomically with mode 0600 (it holds a key now), and its previous content is
//!   kept next to it as `<file>.torroworkplace-backup` (0600 too). A file that cannot be read is an
//!   error and is never overwritten. Writing the same entry again changes nothing.
//! - **Hooks** ([`connect_hooks`]): for Claude Code, Codex and Gemini CLI ([`Target::agent`]) the
//!   events of the table in task E2.2 run `torroworkplace hook <agent> --key-file <file>`
//!   ([`hook_command`]) with the key of a separate client of the kind `hook`. The key itself is in
//!   a file of its own (task E2.3b, [`hook_key_file`]): `<config>/hooks/<hook client id>.key` in
//!   the core's config directory, the directory 0700, the file 0600, written before the hooks and
//!   deleted when they are taken out. So the key is neither in the agent's settings nor, while a
//!   hook runs, in a command line others can see with `ps`. The hooks go into
//!   `~/.claude/settings.json`, `~/.codex/hooks.json` and `~/.gemini/settings.json`
//!   ([`hook_file`]), with the same care as the MCP entry: merged, atomic, 0600, backup. Ours are
//!   the handlers whose command runs `torroworkplace … hook <agent>`, with or without
//!   `--key-file` ([`is_our_hook`]); every other hook stays untouched, and writing again first
//!   takes ours out, so a second run leaves one set. That also replaces entries of an older
//!   version, which carried the key as `TORRO_TOKEN='…'` in the command.
//!   Claude Code's and Codex's `PermissionRequest` hook runs with `--answer` and a longer timeout
//!   ([`ANSWER_TIMEOUT_SECS`]): it puts the request to the person and hands the agent their
//!   decision (task E2.5). Connecting again updates an older set up the same way.
//! - **Disconnect** ([`disconnect`], [`unhook`]): removes the entry again, but only for a client
//!   the wizard set up. Which one that was is noted in `client_setup.json` in the core's config
//!   directory (0600, never a key): one [`Record`] per target with the client's id and the way it
//!   was written, one [`HookRecord`] per agent with the hook client, the agent client it was set
//!   up with, the settings file and the key file. A target holds one `torroworkplace` entry, so connecting a second
//!   client there takes the record over; disconnecting the first one then leaves the entry of the
//!   second alone. Deriving the target from the client's kind alone would remove the newer
//!   client's entry. Disconnecting an agent client also takes out its hooks and names their hook
//!   client, which the agents page then removes from the core.
//!
//! Why the control centre writes these files and not the core: the key leaves the core once, in
//! the answer to `clients.create_client`, and the files belong to the person's other programs, not
//! to the suite. The core never needs to know them.
//!
//! The key is never logged and never part of an error. The command line of `claude mcp add` and
//! `codex mcp add` carries it for the moment the command runs (that is how both CLIs take it); the
//! same user can read the file it ends up in anyway. A hook's command carries only the path of
//! its key file; the hook reads the key from there and refuses a file others could read (task
//! E2.3b, `hook.rs`). A hook key may only report to the inbox anyway (security rule 4 in
//! `docs/tasks/eingang.md`).
//!
//! Every path comes from an [`Env`], so tests point it at a temporary directory, and every command
//! runs through a [`Runner`], so tests never start the real `claude` or `codex`.

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
use torro_inbox::AgentKind;
use torro_ipc::IssuedKey;

use crate::clients::{create_private_dir, write_private};
use crate::hook::KEY_FILE_ARG;

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
    /// Gemini CLI: its MCP servers and its hooks share `~/.gemini/settings.json`.
    Gemini,
    ClaudeDesktop,
    Cursor,
}

impl Target {
    /// In the order the wizard lists them.
    pub const ALL: [Target; 5] =
        [Target::ClaudeCode, Target::Codex, Target::Gemini, Target::ClaudeDesktop, Target::Cursor];

    /// The client kind for `clients.create_client` (`ClientInfo::kind`).
    pub fn kind(self) -> &'static str {
        match self {
            Target::ClaudeCode => "claude-code",
            Target::Codex => "codex",
            Target::Gemini => "gemini",
            Target::ClaudeDesktop => "claude-desktop",
            Target::Cursor => "cursor",
        }
    }

    /// The product's name, also the new client's name. Names are not translated.
    pub fn label(self) -> &'static str {
        match self {
            Target::ClaudeCode => "Claude Code",
            Target::Codex => "Codex",
            Target::Gemini => "Gemini CLI",
            Target::ClaudeDesktop => "Claude Desktop",
            Target::Cursor => "Cursor",
        }
    }

    /// The coding agent whose hooks can report to the inbox (task E2.3): Claude Code, Codex and
    /// Gemini CLI. `None` for apps without such hooks. Its id is also [`Target::kind`].
    pub fn agent(self) -> Option<AgentKind> {
        match self {
            Target::ClaudeCode => Some(AgentKind::ClaudeCode),
            Target::Codex => Some(AgentKind::Codex),
            Target::Gemini => Some(AgentKind::Gemini),
            Target::ClaudeDesktop | Target::Cursor => None,
        }
    }

    /// How this client is reached on this computer, `None` when it is not found: its command if
    /// it has one on the `PATH` (Claude Code also in `~/.claude/local` and `~/.local/bin`), else
    /// its file when its folder is there (`~/.claude.json` counts for Claude Code). Gemini CLI is
    /// always set up through its file, found by its folder `~/.gemini` or its command `gemini`.
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
            Target::Gemini => {
                let dir = home.join(".gemini");
                (dir.is_dir() || env.find_command("gemini", &[]).is_some())
                    .then(|| Method::Json { file: dir.join("settings.json"), stdio_type: false })
            }
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
///
/// A link that leads nowhere is [`SetupError::Unreadable`] and left as it is (task E1.2c): writing
/// would put a plain file in its place, and the person's dotfiles would silently lose it.
fn replace(file: &Path, before: Option<&str>, after: &str) -> Result<(), SetupError> {
    if before == Some(after) {
        return Ok(());
    }
    let target = match fs::canonicalize(file) {
        Ok(target) => target,
        Err(_) if fs::symlink_metadata(file).is_ok_and(|meta| meta.file_type().is_symlink()) => {
            return Err(SetupError::Unreadable(file.to_path_buf()));
        }
        Err(_) => file.to_path_buf(),
    };
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

// ----- Hooks that report to the inbox (task E2.3) -----------------------------------------------
//
// All three agents keep hooks in the same shape: `hooks` → event name → a list of groups, each
// `{"matcher": …, "hooks": [{"type": "command", "command": …, "timeout": …}]}`. Ours are groups
// without a matcher (every notification, every tool: `torroworkplace hook` sorts them out itself,
// task E2.2a), one handler each.

/// How long one run of our hook may take, in seconds. The hook gives up after 500 ms (task E2.2);
/// the rest is room for starting the process.
const HOOK_TIMEOUT_SECS: u64 = 5;
/// Codex gives `SessionEnd` hooks 1 s by default and allows at most 3.
const CODEX_SESSION_END_SECS: u64 = 3;
/// How long the answering hook for `PermissionRequest` may take (task E2.5): it waits for the
/// person up to [`crate::hook::ANSWER_WAIT`] (34 s), plus room for starting and its last calls.
/// Both agents take this (Claude Code's default is 600 s; Codex names no limit but for
/// `SessionEnd` and `Interrupt`). Should the agent still cut it off, it asks in its terminal.
pub const ANSWER_TIMEOUT_SECS: u64 = 40;

/// Whether the hook of `agent` answers `event` with the person's decision (`--answer`, task E2.5):
/// `PermissionRequest` of Claude Code and Codex. Gemini CLI has no hook that takes an answer.
fn answers(agent: AgentKind, event: &str) -> bool {
    event == "PermissionRequest" && matches!(agent, AgentKind::ClaudeCode | AgentKind::Codex)
}

/// The file the hooks of `agent` go into, below the home directory: the user settings of Claude
/// Code and Gemini CLI, Codex's own hooks file.
pub fn hook_file(env: &Env, agent: AgentKind) -> PathBuf {
    env.home.join(hook_file_in_home(agent))
}

fn hook_file_in_home(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::ClaudeCode => ".claude/settings.json",
        AgentKind::Codex => ".codex/hooks.json",
        AgentKind::Gemini => ".gemini/settings.json",
    }
}

/// The events the hook of `agent` listens to, in the order they are written: the ones the table
/// in task E2.2 gives a meaning, and Claude Code's `PermissionRequest`, which only the answering
/// hook takes (task E2.5).
fn hook_events(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::ClaudeCode => {
            &["Notification", "Stop", "UserPromptSubmit", "PostToolUse", "SessionEnd", "PermissionRequest"]
        }
        AgentKind::Codex => &["PermissionRequest", "Stop", "UserPromptSubmit", "PostToolUse", "SessionEnd"],
        AgentKind::Gemini => &["Notification", "AfterAgent", "BeforeAgent", "SessionEnd"],
    }
}

/// The directory below [`Env::config_dir`] that holds the hook keys (task E2.3b), 0700.
pub const HOOK_KEY_DIR: &str = "hooks";

/// Where the key of the hook client `client` lives: `<config>/hooks/<client>.key`, one file per
/// hook client, so replacing a hook client never overwrites the key another set of hooks still
/// reads. Client ids are short hex strings; anything but ASCII letters, digits, `-` and `_` is
/// replaced by `_` all the same, so an id can never lead out of the directory.
pub fn hook_key_file(env: &Env, client: &str) -> PathBuf {
    let name: String =
        client.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    env.config_dir.join(HOOK_KEY_DIR).join(format!("{name}.key"))
}

/// Writes `key` into `file` ([`hook_key_file`]): the directory 0700, the file 0600, atomically,
/// with a trailing newline (the hook trims it).
fn write_hook_key(file: &Path, key: &str) -> Result<(), SetupError> {
    if let Some(dir) = file.parent() {
        create_private_dir(dir).map_err(SetupError::io(dir))?;
    }
    write_private(file, format!("{key}\n").as_bytes())
        .map_err(|_| SetupError::Io { path: file.to_path_buf(), err: io::Error::other("the key could not be written") })
}

/// Deletes a hook's key file; one that is gone already is fine.
fn delete_hook_key(file: &Path) -> Result<(), SetupError> {
    match fs::remove_file(file) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(SetupError::io(file)(err)),
        _ => Ok(()),
    }
}

/// The command a hook runs: `'<program>' hook <agent> --key-file '<key file>'`. All three agents
/// hand a hook's command to a shell; on Windows that is PowerShell (Claude Code is told so with
/// `"shell": "powershell"`, see [`hook_handler`]), so there it reads
/// `& '<program>' hook <agent> --key-file '<key file>'`. Both quote with single quotes, so
/// nothing in the paths is expanded. The key itself is never part of the command.
pub fn hook_command(program: &Path, key_file: &Path, agent: AgentKind) -> String {
    if cfg!(windows) { powershell_command(program, key_file, agent) } else { posix_command(program, key_file, agent) }
}

fn posix_command(program: &Path, key_file: &Path, agent: AgentKind) -> String {
    let quote = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
    format!("{} hook {} {KEY_FILE_ARG} {}", quote(program), agent.id(), quote(key_file))
}

fn powershell_command(program: &Path, key_file: &Path, agent: AgentKind) -> String {
    let quote = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "''"));
    format!("& {} hook {} {KEY_FILE_ARG} {}", quote(program), agent.id(), quote(key_file))
}

/// Whether `command` is one of ours for `agent`: it names a `torroworkplace` program and runs
/// `hook <agent>` either at its end (an older wizard's entry with `TORRO_TOKEN='…'` in front, or
/// one set up by hand as task E2.2 describes, which would otherwise report everything twice) or
/// followed by `--key-file <file>` and nothing else (the current form, [`hook_command`]).
pub fn is_our_hook(command: &str, agent: AgentKind) -> bool {
    if !command.to_lowercase().contains(SERVER_NAME) {
        return false;
    }
    let call = format!(" hook {}", agent.id());
    command.match_indices(&call).any(|(at, _)| {
        let rest = command[at + call.len()..].trim();
        rest.is_empty()
            || rest.strip_prefix(KEY_FILE_ARG).is_some_and(|file| file.starts_with(' ') && !file.trim().is_empty())
    })
}

/// One handler of ours for `event`. Gemini CLI counts milliseconds and names its hooks (the name
/// is what `/hooks disable` takes); the others count seconds. The answering hook ([`answers`])
/// runs `command` with `--answer` and gets [`ANSWER_TIMEOUT_SECS`]; [`is_our_hook`] still knows
/// it, so connecting again replaces it like the others.
fn hook_handler(agent: AgentKind, event: &str, command: &str) -> Value {
    if answers(agent, event) {
        let mut handler = json!({
            "type": "command",
            "command": format!("{command} {}", crate::hook::ANSWER_ARG),
            "timeout": ANSWER_TIMEOUT_SECS,
        });
        if agent == AgentKind::ClaudeCode && cfg!(windows) {
            handler["shell"] = json!("powershell");
        }
        return handler;
    }
    let mut handler = json!({ "type": "command", "command": command });
    match agent {
        AgentKind::ClaudeCode => {
            handler["timeout"] = json!(HOOK_TIMEOUT_SECS);
            if cfg!(windows) {
                handler["shell"] = json!("powershell");
            }
        }
        AgentKind::Codex => {
            let secs = if event == "SessionEnd" { CODEX_SESSION_END_SECS } else { HOOK_TIMEOUT_SECS };
            handler["timeout"] = json!(secs);
        }
        AgentKind::Gemini => {
            handler["name"] = json!(SERVER_NAME);
            handler["timeout"] = json!(HOOK_TIMEOUT_SECS * 1000);
        }
    }
    handler
}

/// Puts our hooks with `command` ([`hook_command`]) for `agent` into `file`: takes out older ones
/// of ours first, then adds one group per event at the end of its list. Foreign hooks and every
/// other setting stay. A file whose `hooks` (or one of its events) is not what it should be is
/// [`SetupError::Unreadable`] and stays as it is; writing what is there already writes nothing.
pub fn add_hooks(file: &Path, agent: AgentKind, command: &str) -> Result<(), SetupError> {
    let before = read_text(file)?;
    let mut root = parse_json(file, before.as_deref())?;
    strip_our_hooks(&mut root, agent);
    let unreadable = || SetupError::Unreadable(file.to_path_buf());
    let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks.as_object_mut().ok_or_else(unreadable)?;
    for event in hook_events(agent) {
        let groups = hooks.entry(*event).or_insert_with(|| Value::Array(Vec::new()));
        let groups = groups.as_array_mut().ok_or_else(unreadable)?;
        groups.push(json!({ "hooks": [hook_handler(agent, event, command)] }));
    }
    replace(file, before.as_deref(), &json_text(&root))
}

/// Takes our hooks for `agent` out of `file`; everything else stays. None there (or no file) is
/// fine and writes nothing.
pub fn remove_hooks(file: &Path, agent: AgentKind) -> Result<(), SetupError> {
    let Some(before) = read_text(file)? else { return Ok(()) };
    let mut root = parse_json(file, Some(&before))?;
    match strip_our_hooks(&mut root, agent) {
        true => replace(file, Some(&before), &json_text(&root)),
        false => Ok(()),
    }
}

/// Removes our handlers from `root["hooks"]`, and a group, an event list or `hooks` itself only
/// when taking ours out emptied it (an empty list someone left there stays). Shapes it does not
/// know are left alone. Returns whether anything was removed.
fn strip_our_hooks(root: &mut Map<String, Value>, agent: AgentKind) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else { return false };
    let mut changed = false;
    hooks.retain(|_, groups| {
        let Some(groups) = groups.as_array_mut() else { return true };
        let mut touched = false;
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
            let count = handlers.len();
            handlers.retain(|handler| {
                !handler.get("command").and_then(Value::as_str).is_some_and(|command| is_our_hook(command, agent))
            });
            if handlers.len() == count {
                return true;
            }
            touched = true;
            !handlers.is_empty()
        });
        changed |= touched;
        !(touched && groups.is_empty())
    });
    if changed && hooks.is_empty() {
        root.remove("hooks");
    }
    changed
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

/// The hooks the wizard wrote for one agent (task E2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRecord {
    /// The hook client's id (`ClientInfo::id`, kind `hook`).
    pub client: String,
    /// The agent client it was set up with; disconnecting that one takes the hooks out and names
    /// this hook client for removal ([`unhook`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub agent: AgentKind,
    /// Where the hooks are ([`hook_file`] when they were written).
    pub file: PathBuf,
    /// The file with the hook client's key ([`hook_key_file`], task E2.3b), deleted with the
    /// hooks. `None` in records of an older version, whose hooks carried the key in the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Records {
    #[serde(default)]
    written: Vec<Record>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hooks: Vec<HookRecord>,
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

/// The hook records (for the tests, like [`records`]).
#[cfg(test)]
pub fn hook_records(env: &Env) -> Result<Vec<HookRecord>, SetupError> {
    load_records(env).map(|records| records.hooks)
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
    let previous = records.clone();
    records.written.retain(|record| record.target != target);
    records.written.push(Record { client: client.to_string(), target, method: method.clone() });
    save_records(env, &records)?;
    let server = Server { program: &env.program, key: key.expose() };
    add(method, &server, runner).inspect_err(|_| {
        let _ = save_records(env, &previous);
    })
}

/// Sets up the hooks of `agent` for the hook client `client` with its `key` (tasks E2.3, E2.3b):
/// notes them first, with `owner`, the agent client they were set up with, then writes the key
/// into its own file ([`hook_key_file`]) and the hooks that name that file into [`hook_file`]
/// ([`add_hooks`]). The note takes over an older one of the same agent (the file holds one set
/// of ours), and that older note's key file is deleted once the new hooks are in. If writing
/// fails, the note goes again and so does a key file written just now.
pub fn connect_hooks(
    env: &Env,
    agent: AgentKind,
    owner: Option<&str>,
    client: &str,
    key: &IssuedKey,
) -> Result<(), SetupError> {
    let mut records = load_records(env)?;
    let previous = records.clone();
    let file = hook_file(env, agent);
    let key_file = hook_key_file(env, client);
    let (replaced, kept): (Vec<HookRecord>, Vec<HookRecord>) =
        records.hooks.into_iter().partition(|record| record.agent == agent);
    records.hooks = kept;
    records.hooks.push(HookRecord {
        client: client.to_string(),
        owner: owner.map(str::to_string),
        agent,
        file: file.clone(),
        key_file: Some(key_file.clone()),
    });
    save_records(env, &records)?;
    let existed = key_file.exists();
    let written = write_hook_key(&key_file, key.expose())
        .and_then(|()| add_hooks(&file, agent, &hook_command(&env.program, &key_file, agent)));
    if let Err(err) = written {
        if !existed {
            let _ = delete_hook_key(&key_file);
        }
        let _ = save_records(env, &previous);
        return Err(err);
    }
    // The older hooks are replaced in the file, so nothing reads their keys any more.
    for old in replaced.iter().filter_map(|record| record.key_file.as_ref()).filter(|old| **old != key_file) {
        let _ = delete_hook_key(old);
    }
    Ok(())
}

/// Takes out the hooks noted for `client`, whether it is the hook client itself or the agent
/// client they were set up with ([`remove_hooks`]), deletes their key files and their notes.
/// Returns the hook clients that were set up with `client` as their agent: they report for an
/// agent that is gone, so the caller removes them from the core too. `client` itself is never in
/// it; nothing noted is an empty list. A note stays when its hooks could not be taken out.
pub fn unhook(env: &Env, client: &str) -> Result<Vec<String>, SetupError> {
    let mut records = load_records(env)?;
    let mine = |record: &HookRecord| record.client == client || record.owner.as_deref() == Some(client);
    let (gone, kept): (Vec<HookRecord>, Vec<HookRecord>) = records.hooks.into_iter().partition(mine);
    records.hooks = kept;
    if gone.is_empty() {
        return Ok(Vec::new());
    }
    for record in &gone {
        remove_hooks(&record.file, record.agent)?;
        if let Some(key_file) = &record.key_file {
            delete_hook_key(key_file)?;
        }
    }
    save_records(env, &records)?;
    Ok(gone.into_iter().map(|record| record.client).filter(|hook| hook != client).collect())
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
    /// [`connect_hooks`]: [`Outcome::Hooked`]. `owner` is the agent client the hooks belong to.
    Hook { agent: AgentKind, owner: Option<String>, client: String, key: IssuedKey },
    /// [`disconnect`] and [`unhook`]: [`Outcome::Disconnected`].
    Disconnect { client: String },
}

/// What [`Job::Disconnect`] took out of the configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Released {
    /// The target whose MCP entry was removed; `None` when the wizard did not write one for the
    /// client.
    pub target: Option<Target>,
    /// The hook clients whose hooks went with the client ([`unhook`]); the agents page removes
    /// them from the core.
    pub hook_clients: Vec<String>,
}

/// How a [`Job`] went; errors as [`SetupError::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Detected(Vec<Found>),
    Connected(Result<(), String>),
    /// The hooks are written; `Ok` holds where, for people (`~/.claude/settings.json`).
    Hooked(Result<String, String>),
    /// What was removed; nothing when the client was not set up by the wizard.
    Disconnected(Result<Released, String>),
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
        Job::Hook { agent, owner, client, key } => Outcome::Hooked(match env {
            Some(env) => connect_hooks(env, agent, owner.as_deref(), &client, &key)
                .map(|()| env.shown(&hook_file(env, agent)))
                .map_err(|err| err.message()),
            None => Err(no_home()),
        }),
        Job::Disconnect { client } => Outcome::Disconnected(match env {
            Some(env) => release(env, &client, runner).map_err(|err| err.message()),
            None => Err(no_home()),
        }),
    }
}

/// [`disconnect`] and [`unhook`] for `client`. Both are tried, so a failing MCP entry does not
/// leave the hooks behind; the first error is the answer.
fn release(env: &Env, client: &str, runner: &dyn Runner) -> Result<Released, SetupError> {
    let target = disconnect(env, client, runner);
    let hook_clients = unhook(env, client);
    Ok(Released { target: target?, hook_clients: hook_clients? })
}

/// Runs `job` on a blocking thread with the real [`Env`] and [`System`].
pub async fn perform(job: Job) -> Outcome {
    let failed = |job: &Job| match job {
        Job::Detect => Outcome::Detected(Vec::new()),
        Job::Connect { .. } => Outcome::Connected(Err(tr("Something went wrong. Try again.").to_string())),
        Job::Hook { .. } => Outcome::Hooked(Err(tr("Something went wrong. Try again.").to_string())),
        Job::Disconnect { .. } => Outcome::Disconnected(Err(tr("Something went wrong. Try again.").to_string())),
    };
    let fallback = failed(&job);
    tokio::task::spawn_blocking(move || run(job, Env::current().as_ref(), &System)).await.unwrap_or(fallback)
}

/// The demo's answer to `job`: Claude Code and Codex found with their commands, Cursor with its
/// file, Gemini CLI and Claude Desktop not; connecting, setting up hooks and disconnecting succeed
/// without touching anything.
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
        Job::Hook { agent, .. } => Outcome::Hooked(Ok(format!("~/{}", hook_file_in_home(*agent)))),
        Job::Disconnect { .. } => Outcome::Disconnected(Ok(Released::default())),
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
        fs::create_dir_all(env.home.join(".gemini")).expect("dir");
        fs::create_dir_all(env.app_config.join("Claude")).expect("dir");
        fs::create_dir_all(env.home.join(".cursor")).expect("dir");
        write(&env.home.join(".claude.json"), "{}");
        let found = detect(&env);
        let targets: Vec<Target> = found.iter().map(|found| found.target).collect();
        assert_eq!(targets, Target::ALL, "every target, in the wizard's order");
        assert_eq!(found[0].method, Method::Json { file: env.home.join(".claude.json"), stdio_type: true });
        assert_eq!(found[1].method, Method::Toml { file: env.home.join(".codex/config.toml") });
        assert_eq!(found[2].method, Method::Json { file: env.home.join(".gemini/settings.json"), stdio_type: false });
        assert_eq!(found[4].place, "~/.cursor/mcp.json");

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

    /// Task E1.2c: a link that leads nowhere is refused and stays a link; no plain file takes its
    /// place.
    #[cfg(unix)]
    #[test]
    fn a_broken_link_is_refused_not_replaced() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let gone = dir.path().join("dotfiles/missing.json");
        let link = env.home.join(".cursor/mcp.json");
        fs::create_dir_all(link.parent().expect("dir")).expect("dir");
        std::os::unix::fs::symlink(&gone, &link).expect("link");
        let method = Method::Json { file: link.clone(), stdio_type: false };
        assert!(matches!(add(&method, &server(&env), &Fake::default()), Err(SetupError::Unreadable(_))));
        assert!(fs::symlink_metadata(&link).expect("link").file_type().is_symlink(), "still a link");
        assert!(!gone.exists(), "nothing written where it points");
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
            Outcome::Disconnected(Ok(Released::default()))
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

    // ----- Hooks (task E2.3) -----

    const OTHER_KEY: &str = "torro_h2_fedcba9876543210";

    /// Our hook command for the hook client `client`, naming its key file.
    fn command(env: &Env, client: &str, agent: AgentKind) -> String {
        hook_command(&env.program, &hook_key_file(env, client), agent)
    }

    /// The handlers in `file` that are ours for `agent`, per event, as (event, handler).
    fn ours(file: &Path, agent: AgentKind) -> Vec<(String, Value)> {
        let root = json(file);
        let Some(hooks) = root.get("hooks").and_then(Value::as_object) else { return Vec::new() };
        let mut found = Vec::new();
        for (event, groups) in hooks {
            for group in groups.as_array().into_iter().flatten() {
                for handler in group["hooks"].as_array().into_iter().flatten() {
                    if handler["command"].as_str().is_some_and(|command| is_our_hook(command, agent)) {
                        assert!(group.get("matcher").is_none(), "our groups match everything");
                        found.push((event.clone(), handler.clone()));
                    }
                }
            }
        }
        found
    }

    #[test]
    fn hooks_keep_foreign_ones_are_written_once_and_removed_cleanly() {
        let cases = [
            (AgentKind::ClaudeCode, "claude_settings.json", ".claude/settings.json"),
            (AgentKind::Codex, "codex_hooks.json", ".codex/hooks.json"),
            (AgentKind::Gemini, "gemini_settings.json", ".gemini/settings.json"),
        ];
        for (agent, name, place) in cases {
            let dir = TempDir::new().expect("tmp");
            let env = env(&dir);
            let file = hook_file(&env, agent);
            assert_eq!(file, env.home.join(place));
            let original = fixture(name);
            let before: Value = serde_json::from_str(&original).expect("fixture json");
            write(&file, &original);

            add_hooks(&file, agent, &command(&env, "h1", agent)).expect("added");
            let found = ours(&file, agent);
            let events: Vec<&str> = found.iter().map(|(event, _)| event.as_str()).collect();
            let mut expected = hook_events(agent).to_vec();
            expected.sort_unstable();
            let mut sorted = events.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, expected, "{agent:?}: one handler per event of the table");
            for (event, handler) in &found {
                assert_eq!(handler["type"], "command");
                // The permission request is answered (task E2.5); every other event only reported.
                let plain = command(&env, "h1", agent);
                let expected_command = match (agent, event.as_str()) {
                    (AgentKind::ClaudeCode | AgentKind::Codex, "PermissionRequest") => format!("{plain} --answer"),
                    _ => plain,
                };
                assert_eq!(handler["command"], expected_command.as_str(), "{agent:?} {event}");
                let timeout = match (agent, event.as_str()) {
                    (AgentKind::Gemini, _) => 5000,
                    (AgentKind::Codex, "SessionEnd") => 3,
                    (_, "PermissionRequest") => 40,
                    _ => 5,
                };
                assert_eq!(handler["timeout"], timeout, "{agent:?} {event}");
            }
            let written = json(&file);
            for (event, groups) in before["hooks"].as_object().expect("fixture hooks") {
                let kept =
                    &written["hooks"][event].as_array().expect("event")[..groups.as_array().expect("list").len()];
                assert_eq!(
                    kept,
                    groups.as_array().expect("list").as_slice(),
                    "{agent:?}: foreign {event} hooks stay first"
                );
            }
            for (key, value) in before.as_object().expect("object").iter().filter(|(key, _)| *key != "hooks") {
                assert_eq!(&written[key], value, "{agent:?}: the setting {key} stays");
            }
            let backup = file.with_file_name(format!("{}{BACKUP_SUFFIX}", file.file_name().expect("name").display()));
            assert_eq!(fs::read_to_string(&backup).expect("backup"), original);
            #[cfg(unix)]
            assert_eq!((mode(&file), mode(&backup)), (0o600, 0o600));

            // Twice is once: the same text, not even a new backup.
            let text = fs::read_to_string(&file).expect("file");
            fs::write(&backup, "marker").expect("marker");
            add_hooks(&file, agent, &command(&env, "h1", agent)).expect("again");
            assert_eq!(fs::read_to_string(&file).expect("file"), text, "{agent:?}: written once");
            assert_eq!(fs::read_to_string(&backup).expect("backup"), "marker");

            // A new hook client replaces ours, still one per event.
            add_hooks(&file, agent, &command(&env, "h2", agent)).expect("a new hook client");
            let found = ours(&file, agent);
            assert_eq!(found.len(), hook_events(agent).len());
            assert!(found.iter().all(|(_, handler)| handler["command"].as_str().is_some_and(|c| c.contains("h2.key"))));

            remove_hooks(&file, agent).expect("removed");
            assert_eq!(json(&file), before, "{agent:?}: only ours went");
            let text = fs::read_to_string(&file).expect("file");
            remove_hooks(&file, agent).expect("twice");
            assert_eq!(fs::read_to_string(&file).expect("file"), text, "removing nothing writes nothing");
        }
    }

    #[test]
    fn another_agents_hooks_and_a_hand_made_one_are_told_apart() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        // Gemini CLI's settings might hold Claude Code's hook of ours by mistake; it stays.
        let file = hook_file(&env, AgentKind::Gemini);
        add_hooks(&file, AgentKind::ClaudeCode, &command(&env, "h1", AgentKind::ClaudeCode)).expect("claude's");
        add_hooks(&file, AgentKind::Gemini, &command(&env, "h1", AgentKind::Gemini)).expect("gemini's");
        remove_hooks(&file, AgentKind::Gemini).expect("removed");
        assert!(ours(&file, AgentKind::Gemini).is_empty());
        let claude_events = hook_events(AgentKind::ClaudeCode).len();
        assert_eq!(ours(&file, AgentKind::ClaudeCode).len(), claude_events, "the other agent's hooks stay");

        // Entries of an older version (key in the command) and hand-made ones are ours too, so
        // connecting again replaces them.
        assert!(is_our_hook("TORRO_TOKEN=torro_x torroworkplace hook claude-code", AgentKind::ClaudeCode));
        assert!(is_our_hook("$env:TORRO_TOKEN='k'; & 'C:\\Torro\\TorroWorkplace.exe' hook codex ", AgentKind::Codex));
        assert!(is_our_hook("'/opt/torroworkplace' hook gemini --key-file '/c/hooks/h1.key'", AgentKind::Gemini));
        assert!(!is_our_hook("TORRO_TOKEN=k torroworkplace hook claude-code", AgentKind::Codex));
        assert!(!is_our_hook("torroworkplace hook codex --key-file /k", AgentKind::Gemini), "another agent");
        assert!(!is_our_hook("~/.claude/hooks/hook claude-code", AgentKind::ClaudeCode), "not our program");
        assert!(!is_our_hook("torroworkplace hook claude-code --debug", AgentKind::ClaudeCode));
        assert!(!is_our_hook("torroworkplace hook claude-code --key-file", AgentKind::ClaudeCode), "no file");
        assert!(!is_our_hook("torroworkplace hook codexx --key-file /k", AgentKind::Codex));
    }

    #[test]
    fn hook_commands_quote_the_paths_and_hold_no_key() {
        let program = Path::new("/home/an'na/my tools/torroworkplace");
        let key_file = Path::new("/home/an'na/.config/torroworkplace/hooks/h1.key");
        assert_eq!(
            posix_command(program, key_file, AgentKind::ClaudeCode),
            "'/home/an'\\''na/my tools/torroworkplace' hook claude-code --key-file \
             '/home/an'\\''na/.config/torroworkplace/hooks/h1.key'"
        );
        let windows = Path::new("C:\\Users\\O'Brien\\torroworkplace.exe");
        let windows_key = Path::new("C:\\Users\\O'Brien\\AppData\\Roaming\\torroworkplace\\hooks\\h1.key");
        assert_eq!(
            powershell_command(windows, windows_key, AgentKind::Gemini),
            "& 'C:\\Users\\O''Brien\\torroworkplace.exe' hook gemini --key-file \
             'C:\\Users\\O''Brien\\AppData\\Roaming\\torroworkplace\\hooks\\h1.key'"
        );
        for agent in AgentKind::ALL {
            let posix = posix_command(program, key_file, agent);
            assert!(is_our_hook(&posix, agent));
            assert!(is_our_hook(&powershell_command(windows, windows_key, agent), agent));
            assert!(!posix.contains(TOKEN_VARIABLE), "no key, no variable");
        }
        // The hook reads its command line back: program, `hook`, agent, `--key-file`, file.
        let args = ["claude-code", KEY_FILE_ARG, "/c/hooks/h1.key"].map(String::from);
        assert_eq!(
            crate::hook::parse_args(&args).and_then(|args| args.key_file),
            Some(PathBuf::from("/c/hooks/h1.key"))
        );
    }

    #[test]
    fn a_key_file_name_stays_in_its_directory() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let hooks = env.config_dir.join(HOOK_KEY_DIR);
        assert_eq!(hook_key_file(&env, "3fa9c2d1"), hooks.join("3fa9c2d1.key"));
        assert_eq!(hook_key_file(&env, "../../x"), hooks.join("______x.key"));
        assert_eq!(hook_key_file(&env, "a/b\\c"), hooks.join("a_b_c.key"));
    }

    #[test]
    fn a_missing_hooks_file_is_created_and_an_odd_one_stays() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = hook_file(&env, AgentKind::Codex);
        remove_hooks(&file, AgentKind::Codex).expect("nothing to remove");
        assert!(!file.exists(), "removing writes nothing");
        add_hooks(&file, AgentKind::Codex, &command(&env, "h1", AgentKind::Codex)).expect("created with its folder");
        assert_eq!(ours(&file, AgentKind::Codex).len(), 5);
        remove_hooks(&file, AgentKind::Codex).expect("removed");
        assert_eq!(json(&file), json!({}), "the `hooks` we created went with ours");

        for odd in ["{\"hooks\": []}", "{\"hooks\": {\"Stop\": {}}}", "[1]"] {
            write(&file, odd);
            let added = add_hooks(&file, AgentKind::Codex, &command(&env, "h1", AgentKind::Codex));
            assert!(matches!(added, Err(SetupError::Unreadable(_))), "{odd}");
            assert_eq!(fs::read_to_string(&file).expect("file"), odd, "never overwritten");
        }
        // An empty list someone left there stays.
        write(&file, "{\"hooks\": {\"PreToolUse\": []}}");
        add_hooks(&file, AgentKind::Codex, &command(&env, "h1", AgentKind::Codex)).expect("added");
        remove_hooks(&file, AgentKind::Codex).expect("removed");
        assert_eq!(json(&file), json!({"hooks": {"PreToolUse": []}}));
    }

    #[test]
    fn disconnecting_the_agent_takes_its_hooks_and_names_the_hook_client() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = env.home.join(".gemini/settings.json");
        let original = fixture("gemini_settings.json");
        write(&file, &original);
        let method = Method::Json { file: file.clone(), stdio_type: false };
        let fake = Fake::default();
        connect(&env, Target::Gemini, &method, "a1", &key(), &fake).expect("mcp entry");
        let hook_key = IssuedKey::new(Key::new(OTHER_KEY));
        connect_hooks(&env, AgentKind::Gemini, Some("a1"), "h1", &hook_key).expect("hooks");
        assert!(json(&file)["mcpServers"].get(SERVER_NAME).is_some(), "both share the file");
        assert_eq!(ours(&file, AgentKind::Gemini).len(), 4);
        let key_file = env.config_dir.join("hooks/h1.key");
        let noted = hook_records(&env).expect("records");
        assert_eq!(
            noted,
            vec![HookRecord {
                client: "h1".into(),
                owner: Some("a1".into()),
                agent: AgentKind::Gemini,
                file: file.clone(),
                key_file: Some(key_file.clone()),
            }]
        );
        let record_text = fs::read_to_string(env.config_dir.join(RECORD_FILE)).expect("record");
        assert!(!record_text.contains(KEY) && !record_text.contains(OTHER_KEY), "the record holds no key");

        // The hook key is in its own private file; the settings only name it (task E2.3b).
        assert_eq!(fs::read_to_string(&key_file).expect("key file"), format!("{OTHER_KEY}\n"));
        #[cfg(unix)]
        assert_eq!((mode(&key_file), mode(key_file.parent().expect("dir"))), (0o600, 0o700));
        let settings = fs::read_to_string(&file).expect("settings");
        assert!(!settings.contains(OTHER_KEY), "the hook key is not in the agent's settings");
        for (_, handler) in ours(&file, AgentKind::Gemini) {
            assert_eq!(handler["command"], command(&env, "h1", AgentKind::Gemini).as_str());
        }
        assert_eq!(crate::hook::read_key_file(&key_file).expect("the hook accepts it").expose(), OTHER_KEY);

        let released = run(Job::Disconnect { client: "a1".into() }, Some(&env), &fake);
        let expected = Released { target: Some(Target::Gemini), hook_clients: vec!["h1".into()] };
        assert_eq!(released, Outcome::Disconnected(Ok(expected)));
        let before: Value = serde_json::from_str(&original).expect("fixture json");
        assert_eq!(json(&file), before, "MCP entry and hooks gone, everything else as it was");
        assert!(!key_file.exists(), "disconnecting deletes the key file");
        assert!(records(&env).expect("records").is_empty() && hook_records(&env).expect("hooks").is_empty());
        // The agents page then removes h1 from the core, which disconnects it: nothing left.
        assert_eq!(
            run(Job::Disconnect { client: "h1".into() }, Some(&env), &fake),
            Outcome::Disconnected(Ok(Released::default()))
        );
    }

    #[test]
    fn disconnecting_the_hook_client_takes_only_its_hooks() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let fake = Fake::default();
        let claude = Method::Json { file: env.home.join(".claude.json"), stdio_type: true };
        connect(&env, Target::ClaudeCode, &claude, "a1", &key(), &fake).expect("mcp entry");
        connect_hooks(&env, AgentKind::ClaudeCode, Some("a1"), "h1", &key()).expect("hooks");
        // Setting up again takes the note over: the older hook client is not named any more.
        let (h1_key, h2_key) = (hook_key_file(&env, "h1"), hook_key_file(&env, "h2"));
        assert!(h1_key.exists());
        connect_hooks(&env, AgentKind::ClaudeCode, Some("a1"), "h2", &key()).expect("again");
        assert_eq!(hook_records(&env).expect("hooks").len(), 1);
        let settings = hook_file(&env, AgentKind::ClaudeCode);
        let claude_events = hook_events(AgentKind::ClaudeCode).len();
        assert_eq!(ours(&settings, AgentKind::ClaudeCode).len(), claude_events, "one set");
        assert!(!h1_key.exists() && h2_key.exists(), "the replaced hooks' key file goes");

        assert_eq!(unhook(&env, "h1").expect("not noted"), Vec::<String>::new());
        assert_eq!(ours(&settings, AgentKind::ClaudeCode).len(), claude_events, "h2's hooks stay");
        let released = run(Job::Disconnect { client: "h2".into() }, Some(&env), &fake);
        assert_eq!(released, Outcome::Disconnected(Ok(Released::default())), "a hook client names no other");
        assert!(ours(&settings, AgentKind::ClaudeCode).is_empty());
        assert!(!h2_key.exists(), "its key file went with the hooks");
        assert!(json(&env.home.join(".claude.json"))["mcpServers"].get(SERVER_NAME).is_some(), "the agent stays");
    }

    /// Hooks an older version wrote, with the key in the command and a record without a key file:
    /// connecting again replaces them, and disconnecting an old record still works.
    #[test]
    fn hooks_with_the_key_in_the_command_are_replaced() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = hook_file(&env, AgentKind::ClaudeCode);
        write(&file, &fixture("claude_settings_old_hooks.json"));
        let old_record = format!(
            "{{\"written\": [], \"hooks\": [{{\"client\": \"h0\", \"owner\": \"a1\", \"agent\": \"claude-code\", \"file\": {}}}]}}",
            json!(file)
        );
        write(&env.config_dir.join(RECORD_FILE), &old_record);
        assert_eq!(hook_records(&env).expect("an old record reads")[0].key_file, None);

        connect_hooks(&env, AgentKind::ClaudeCode, Some("a1"), "h1", &key()).expect("connected again");
        let text = fs::read_to_string(&file).expect("settings");
        assert!(!text.contains("torro_old_0000aaaa") && !text.contains(TOKEN_VARIABLE), "the old entries are gone");
        assert!(!text.contains(KEY), "no key in the settings");
        let found = ours(&file, AgentKind::ClaudeCode);
        assert_eq!(found.len(), hook_events(AgentKind::ClaudeCode).len(), "one set, in the new form");
        let plain = command(&env, "h1", AgentKind::ClaudeCode);
        for (event, handler) in &found {
            let expected = if event == "PermissionRequest" { format!("{plain} --answer") } else { plain.clone() };
            assert_eq!(handler["command"], expected.as_str(), "{event}");
        }
        let written = json(&file);
        assert_eq!(written["hooks"]["Notification"][0]["matcher"], "idle_prompt", "the foreign hook stays");
        assert_eq!(written["model"], "opus");
        assert_eq!(hook_records(&env).expect("records").len(), 1, "the new note took over");

        // An old record (no key file) is still taken out cleanly.
        write(&file, &fixture("claude_settings_old_hooks.json"));
        write(&env.config_dir.join(RECORD_FILE), &old_record);
        assert_eq!(unhook(&env, "a1").expect("unhooked"), vec!["h0".to_string()]);
        assert!(!fs::read_to_string(&file).expect("settings").contains("torro_old_0000aaaa"));
    }

    #[test]
    fn a_failed_hook_setup_leaves_no_note_and_the_job_says_where() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        let file = hook_file(&env, AgentKind::Codex);
        write(&file, "{\"hooks\": []}");
        assert!(connect_hooks(&env, AgentKind::Codex, None, "h1", &key()).is_err());
        assert!(hook_records(&env).expect("hooks").is_empty());
        assert!(!hook_key_file(&env, "h1").exists(), "no key file is left behind");

        fs::remove_file(&file).expect("removed");
        let job = Job::Hook { agent: AgentKind::Codex, owner: Some("a1".into()), client: "h1".into(), key: key() };
        assert!(!format!("{job:?}").contains(KEY), "a job's Debug hides the key");
        assert_eq!(run(job.clone(), Some(&env), &Fake::default()), Outcome::Hooked(Ok("~/.codex/hooks.json".into())));
        assert!(matches!(run(job.clone(), None, &Fake::default()), Outcome::Hooked(Err(_))));
        assert_eq!(demo(&job), Outcome::Hooked(Ok("~/.codex/hooks.json".into())));
    }

    #[cfg(unix)]
    #[test]
    fn gemini_is_found_by_its_command_alone() {
        let dir = TempDir::new().expect("tmp");
        let env = env(&dir);
        executable(&dir.path().join("bin/gemini"));
        let found = detect(&env);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].method, Method::Json { file: env.home.join(".gemini/settings.json"), stdio_type: false });
        assert_eq!(Target::Gemini.agent(), Some(AgentKind::Gemini));
        assert_eq!(Target::Gemini.kind(), AgentKind::Gemini.id(), "the client kind is the agent's id");
        assert_eq!(Target::Cursor.agent(), None);
    }

    /// Task E2.5: a set up from before (Codex's `PermissionRequest` only observed, Claude Code
    /// without one) becomes the answering one when the agent is connected again, and connecting
    /// once more changes nothing.
    #[test]
    fn connecting_again_makes_the_permission_hook_answer_and_stays_put() {
        for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
            let dir = TempDir::new().expect("tmp");
            let env = env(&dir);
            let file = hook_file(&env, agent);
            let plain = command(&env, "h1", agent);
            // What the wizard wrote before E2.5: every event with the plain command and 5 s.
            let events: Vec<&str> = hook_events(agent)
                .iter()
                .copied()
                .filter(|e| agent == AgentKind::Codex || *e != "PermissionRequest")
                .collect();
            let old: Map<String, Value> = events
                .iter()
                .map(|event| {
                    (
                        (*event).to_string(),
                        json!([{ "hooks": [{ "type": "command", "command": plain, "timeout": 5 }] }]),
                    )
                })
                .collect();
            write(&file, &json_text(&Map::from_iter([("hooks".to_string(), Value::Object(old))])));

            add_hooks(&file, agent, &plain).expect("connected again");
            let found = ours(&file, agent);
            assert_eq!(found.len(), hook_events(agent).len(), "{agent:?}: one set");
            let asking: Vec<&Value> =
                found.iter().filter(|(event, _)| event == "PermissionRequest").map(|(_, handler)| handler).collect();
            let [handler] = asking.as_slice() else { panic!("{agent:?}: one permission hook: {found:?}") };
            assert_eq!(handler["command"], format!("{plain} {}", crate::hook::ANSWER_ARG).as_str());
            assert_eq!(handler["timeout"], ANSWER_TIMEOUT_SECS);
            assert!(is_our_hook(handler["command"].as_str().expect("command"), agent), "still ours");
            assert!(ANSWER_TIMEOUT_SECS > crate::hook::ANSWER_WAIT.as_secs() + 1, "the hook is never cut off waiting");

            // The answering command parses back as the hook reads it.
            let words: Vec<String> =
                vec![agent.id().into(), KEY_FILE_ARG.into(), "/c/hooks/h1.key".into(), crate::hook::ANSWER_ARG.into()];
            assert!(crate::hook::parse_args(&words).is_some_and(|args| args.answer));

            let text = fs::read_to_string(&file).expect("settings");
            add_hooks(&file, agent, &plain).expect("once more");
            assert_eq!(fs::read_to_string(&file).expect("settings"), text, "{agent:?}: nothing changes");
            remove_hooks(&file, agent).expect("removed");
            assert!(ours(&file, agent).is_empty(), "{agent:?}: the answering hook goes too");
        }
    }
}
