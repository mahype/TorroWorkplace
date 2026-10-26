//! The built-in catalog of MCP servers (task MA.2): `assets/extensions/catalog.toml`, built in, read and
//! checked once.
//!
//! Each entry says what people see before they install it (a [`CatalogEntry`]) and how the core starts it:
//! a program by name with fixed arguments, never through a shell. The checks keep a mistake in the file
//! from reaching a person's computer: an id the rest of the core cannot take, a variable that would make
//! the program load other code ([`blocked_env`]), a tool that would need `admin`, a shell as the command, or
//! a program that would wait for an answer on stdin (stdin is the MCP connection). Unit tests prove the
//! built-in file passes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;
use torro_ipc::api::Access;
use torro_ipc::extensions::{
    CatalogEntry, CatalogSecret, CatalogTool, Prerequisite, blocked_env, tool_name, valid_env_name, valid_id,
};

use super::prereq::program_name;

/// The catalog as built in.
const CATALOG: &str = include_str!("../../assets/extensions/catalog.toml");

/// Programs that would read their arguments as a script: never a command of the catalog.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "csh", "tcsh", "cmd", "powershell", "pwsh"];

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
    let tools = raw.tools.into_iter().map(|tool| CatalogTool { name: tool.name, access: tool.access }).collect();
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
            folder: None,
        },
        command: raw.command,
        args: raw.args,
        env: raw.env,
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
    check_tools(info)
}

/// A program by name, no shell, one of the prerequisites, the version in the arguments, and nothing that
/// waits on stdin.
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
    if !entry.args.iter().any(|arg| arg.contains(entry.info.version.as_str())) {
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
        assert_eq!(ids, ["time", "sequential-thinking", "github"]);
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
    }

    #[test]
    fn a_good_entry_reads() {
        let entries = parse(GOOD).expect("valid");
        let time = &entries[0];
        assert_eq!(time.info.requires, [Prerequisite::Uvx]);
        assert_eq!(time.info.tools, [CatalogTool { name: "get_current_time".into(), access: Access::Read }]);
        assert!(!time.info.experimental);
        assert_eq!(parse("").expect("empty catalog"), Vec::new());
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
