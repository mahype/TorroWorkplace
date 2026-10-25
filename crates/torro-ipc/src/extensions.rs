//! MCP servers connected to Torro Workplace (issue #302, `docs/erweiterungen.md`) on the wire: what
//! the `extensions.*` requests and answers carry, and the rules for ids, tool names and variables
//! that the core, the control centre and the MCP server share.
//!
//! A server ("extension") is a program on the person's computer (stdio) or an address (Streamable
//! HTTP) whose tools the core offers to the assistant, its bots and MCP clients. The core is its MCP
//! client; the server never calls Torro. Rights: the area `extensions` manages the servers
//! ([`crate::api::Area::Extensions`]), and each server has a level of its own per client
//! ([`crate::api::Rights::extensions`]). Installing, switching on, allowing tools, updating, its
//! secrets and its output are a person's (point 10 of "Was nie nach außen geht" in `docs/api.md`).
//!
//! Everything a server says – the descriptions and schemas of its tools, its answers, its tool names –
//! is foreign text: data, never instructions. The logic is the core's
//! (`crates/torroworkplace/src/extensions`).

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_core::i18n::{tr, trf};

use crate::Secret;
use crate::api::Access;

/// At most this many characters in the id of a server ([`valid_id`]).
pub const MAX_ID_CHARS: usize = 24;

/// At most this many characters in the name of a tool towards agents ([`tool_name`]); MCP clients
/// accept no longer names.
pub const MAX_TOOL_NAME_CHARS: usize = 64;

/// What the name of every server tool starts with towards agents: `ext_<id>_<tool>`.
pub const TOOL_PREFIX: &str = "ext_";

/// What a server tool switched off for a client starts with in [`crate::api::Rights::denied`]:
/// `ext.<id>.<tool>` ([`denied_name`]).
pub const DENIED_PREFIX: &str = "ext.";

/// Whether `id` is a valid id of a server: 1 to [`MAX_ID_CHARS`] characters of `a-z`, `0-9` and `-`,
/// starting with a letter. It has no `_`, so [`parse_tool_name`] can split `ext_<id>_<tool>` at the
/// first `_` after the prefix. Unique among the servers: the core's check.
pub fn valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_CHARS
        && id.chars().next().is_some_and(|first| first.is_ascii_lowercase())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The name of the tool `tool` of the server `id` towards agents (MCP and the assistant):
/// `ext_<id>_<tool>`. `None` when `id` is not valid ([`valid_id`]), `tool` is empty or has a
/// character other than `A-Z`, `a-z`, `0-9`, `_` and `-`, or the whole is longer than
/// [`MAX_TOOL_NAME_CHARS`]: such a tool is not offered, and its server's status says so
/// ([`ToolState::Unsupported`]).
pub fn tool_name(id: &str, tool: &str) -> Option<String> {
    let fits = valid_id(id)
        && !tool.is_empty()
        && tool.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && TOOL_PREFIX.len() + id.len() + 1 + tool.len() <= MAX_TOOL_NAME_CHARS;
    fits.then(|| format!("{TOOL_PREFIX}{id}_{tool}"))
}

/// The server id and the tool of a name made by [`tool_name`], split at the first `_` after `ext_`;
/// `None` for any name [`tool_name`] would not make (another prefix, an invalid id, too long).
pub fn parse_tool_name(name: &str) -> Option<(&str, &str)> {
    let (id, tool) = name.strip_prefix(TOOL_PREFIX)?.split_once('_')?;
    tool_name(id, tool).is_some().then_some((id, tool))
}

/// The entry in [`crate::api::Rights::denied`] that switches the tool `tool` of the server `id` off
/// for one client: `ext.<id>.<tool>`. The core stores only entries [`parse_denied_name`] reads.
pub fn denied_name(id: &str, tool: &str) -> String {
    format!("{DENIED_PREFIX}{id}.{tool}")
}

/// The server id and the tool of an entry made by [`denied_name`]: the id valid ([`valid_id`]) and
/// the tool one [`tool_name`] would offer; `None` for anything else.
pub fn parse_denied_name(name: &str) -> Option<(&str, &str)> {
    let (id, tool) = name.strip_prefix(DENIED_PREFIX)?.split_once('.')?;
    tool_name(id, tool).is_some().then_some((id, tool))
}

/// Whether a server may not be given the variable `name` (`docs/tasks/mcp-anbindungen.md`,
/// "Umgebung des Kindprozesses"), in any case: `PATH` and `PATHEXT` (the core sets them); what makes
/// a program load other code (`LD_*`, `DYLD_*`, `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`,
/// `PYTHONWARNINGS`, `PYTHONUSERBASE`, `NODE_OPTIONS`, `NODE_PATH`, `PERL5LIB`, `PERLLIB`, `PERL5OPT`,
/// `RUBYOPT`, `RUBYLIB`, `GEM_HOME`, `GEM_PATH`, `CLASSPATH`, `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`,
/// `JDK_JAVA_OPTIONS`, `BASH_ENV`, `BASH_FUNC_*`, `ENV`, `IFS`, `SHELLOPTS`, `PS4`, `ZDOTDIR`, `GIT_SSH`,
/// `GIT_SSH_COMMAND`, `GIT_EXEC_PATH`, `GIT_PROXY_COMMAND`, `GIT_ASKPASS`, `SSH_ASKPASS`, `GIT_EXTERNAL_DIFF`,
/// `GIT_TEMPLATE_DIR`, `GIT_CONFIG*`, `PYTHONPLATLIBDIR`, on Windows `ComSpec`, `SystemRoot` and `WINDIR`);
/// where packages and settings come from (`npm_config_*`, `UV_*`, `PIP_*`, and the folders the core sets or
/// that hold settings and caches: `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `XDG_CONFIG_HOME`,
/// `XDG_CACHE_HOME`, `XDG_DATA_HOME`); what sends a server's secrets elsewhere (a Docker daemon or its
/// settings: `DOCKER_HOST`, `DOCKER_CONTEXT`, `DOCKER_CONFIG`; certificates to trust or none:
/// `NODE_EXTRA_CA_CERTS`, `NODE_TLS_REJECT_UNAUTHORIZED`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `REQUESTS_CA_BUNDLE`,
/// `CURL_CA_BUNDLE`, `GIT_SSL_NO_VERIFY`, `GIT_SSL_CAINFO`, `PYTHONHTTPSVERIFY`); and Torro's own (`TORRO_*`). Installing a server with one is
/// refused with `error`.
pub fn blocked_env(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "PATH",
        "PATHEXT",
        "PYTHONPATH",
        "PYTHONHOME",
        "PYTHONSTARTUP",
        "PYTHONWARNINGS",
        "PYTHONUSERBASE",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PERL5LIB",
        "PERLLIB",
        "PERL5OPT",
        "RUBYOPT",
        "RUBYLIB",
        "GEM_HOME",
        "GEM_PATH",
        "CLASSPATH",
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "BASH_ENV",
        "ENV",
        "IFS",
        "SHELLOPTS",
        "PS4",
        "ZDOTDIR",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_EXEC_PATH",
        "GIT_PROXY_COMMAND",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_EXTERNAL_DIFF",
        "GIT_TEMPLATE_DIR",
        "GIT_SSL_NO_VERIFY",
        "GIT_SSL_CAINFO",
        "PYTHONHTTPSVERIFY",
        "PYTHONPLATLIBDIR",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "COMSPEC",
        "SYSTEMROOT",
        "WINDIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
        "DOCKER_CONFIG",
        "NODE_EXTRA_CA_CERTS",
        "NODE_TLS_REJECT_UNAUTHORIZED",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "REQUESTS_CA_BUNDLE",
        "CURL_CA_BUNDLE",
    ];
    const PREFIXES: &[&str] = &["LD_", "DYLD_", "BASH_FUNC_", "GIT_CONFIG", "NPM_CONFIG_", "UV_", "PIP_", "TORRO_"];
    let upper = name.to_ascii_uppercase();
    NAMES.contains(&upper.as_str()) || PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
}

/// Whether `name` can name an environment variable at all: not empty, no `=`, no control character
/// and no white space. Whether the server may get it is [`blocked_env`].
pub fn valid_env_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c == '=' || c.is_control() || c.is_whitespace())
}

/// Whether `name` can name an HTTP header: printable ASCII without `:` and white space.
pub fn valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_graphic() && c != ':')
}

/// How the core reaches a server: a program it starts and talks to on stdin/stdout, or an address
/// it talks Streamable HTTP to. The rules for the program's environment (empty, a few basics, the
/// `PATH` the core sets, then `env`) and for the address (`https://`, `http://` only for
/// `localhost`, `127.0.0.1`, `::1`, no redirects) are the core's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum ServerSpec {
    /// A program started without a shell: `command` with `args`, and the variables `env` besides
    /// the ones the core sets. No name in `env` may be blocked ([`blocked_env`]).
    Stdio {
        /// The program: a name the core looks for in the usual folders (`npx`, `uvx`, `docker`) or
        /// an absolute path.
        command: String,
        /// Its arguments, one per entry; no shell reads them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// The variables the server gets, by name.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, Value>,
    },
    /// An address that speaks Streamable HTTP, with headers for signing in.
    Http {
        /// `https://…`, or `http://` for `localhost`, `127.0.0.1` and `::1` only.
        url: String,
        /// The headers of every request, by name (`Authorization`).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, Value>,
    },
}

impl ServerSpec {
    /// The variables of a program or the headers of an address, by name.
    pub fn values(&self) -> &BTreeMap<String, Value> {
        match self {
            ServerSpec::Stdio { env, .. } => env,
            ServerSpec::Http { headers, .. } => headers,
        }
    }
}

/// The value of one variable or header of a server.
///
/// When installing or updating, a person gives each one as plain text ([`Value::Plain`]) or as a
/// secret ([`Value::Secret`]), which the core keeps in the keychain (`ext:<id>:<NAME>`). The core
/// never sends a secret back: on disk and in every answer a secret is [`Value::Stored`]. In an
/// update, [`Value::Stored`] keeps the secret that is stored under that name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    /// A value that is no secret; stored and shown as it is.
    Plain(String),
    /// A secret on its way to the keychain. Only in requests: it serializes as its text, so an
    /// answer must never hold it.
    Secret(Secret),
    /// A secret the core keeps; its value never leaves the core. `secret`: whether the keychain
    /// holds a value for it now ("Stored") or not ("Missing").
    Stored { secret: bool },
}

/// Where a server came from: an entry of the core's built-in catalog in a fixed version, or a
/// person's own (`extensions.install_extension` with `custom`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// The catalog entry `entry` ([`CatalogEntry::id`]) in the version `version` it was installed or
    /// last updated in. A newer [`CatalogEntry::version`] means "Update" can take it.
    Catalog { entry: String, version: String },
    /// Command or address given by a person.
    Custom,
}

/// How a server stands. Clients must accept states they do not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Not running: switched off, not needed yet, or ended after 10 minutes without a call. It
    /// starts on the next call.
    Stopped,
    /// Starting and reading its tools.
    Starting,
    /// Running; its allowed tools can be called.
    Running,
    /// It reported tools a person has not allowed yet (after installing, or new or changed ones
    /// later): those are not offered until a person allows them (`extensions.confirm_tools`).
    Review,
    /// It ended or failed three times within 5 minutes and is not started again until a person
    /// switches it on again. Only the kind of failure: the server's own words are its output
    /// (`extensions.read_logs`), which only people read.
    Crashed { reason: CrashReason },
    /// What it needs to start is missing on this computer.
    Missing { what: Prerequisite },
}

/// Why a server failed ([`Status::Crashed`]): a fixed kind, never text of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashReason {
    /// The program ended.
    Exited,
    /// It did not answer `initialize` in time (30 s, 180 s when installing or updating).
    StartTimeout,
    /// The program could not be started.
    SpawnFailed,
    /// It broke the MCP protocol.
    Protocol,
}

/// What a person has decided about one tool of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// A person allowed it as the server describes it now: it is offered to whom may use it.
    Allowed,
    /// New, or changed since a person allowed it (its name, description or input schema): not
    /// offered until a person allows it again.
    Review,
    /// Never offered: its name makes no valid tool name ([`tool_name`]).
    Unsupported,
}

/// One tool of a server as the core knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionTool {
    /// The server's id ([`ExtensionInfo::id`]).
    pub extension: String,
    /// The server's name as people see it ([`ExtensionInfo::name`]): "From <name> (external,
    /// untrusted)" in the descriptions agents read.
    pub extension_name: String,
    /// The tool's name as the server calls it; the server's text.
    pub name: String,
    /// The name towards agents, `ext_<id>_<tool>` ([`tool_name`]); `None` exactly for
    /// [`ToolState::Unsupported`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// What the server says the tool does: foreign text, data, never instructions.
    pub description: String,
    /// The JSON Schema of its arguments as the server gives it: foreign text too.
    pub input_schema: serde_json::Value,
    /// The access a call needs: from the catalog entry, `delete` when it names none or for a
    /// person's own server. Never `admin`; the server's own hints do not count.
    pub access: Access,
    pub state: ToolState,
}

/// Whether the keychain holds a secret the server needs ("Stored", "Missing"). Never its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    /// The variable or header it goes to (`GITHUB_PERSONAL_ACCESS_TOKEN`).
    pub name: String,
    pub stored: bool,
}

/// One installed server (`extensions.list_extensions`, `extensions.get_extension`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionInfo {
    /// Its id ([`valid_id`]): what the other `extensions.*` requests take.
    pub id: String,
    /// What people call it ("Time", or the name a person gave).
    pub name: String,
    pub source: Source,
    /// How the core reaches it; every secret as [`Value::Stored`], so the form "Update" can be
    /// filled in without one.
    pub server: ServerSpec,
    /// A person switched it on (after allowing its tools); off, none of its tools is offered.
    pub enabled: bool,
    pub status: Status,
    /// Every tool it reported when it last ran, with its state.
    pub tools: Vec<ExtensionTool>,
    /// The secrets it needs and whether each is stored.
    pub secrets: Vec<SecretInfo>,
    /// What the core noticed for people, in English: tools it does not offer and why. A tool name in
    /// it is the server's text.
    pub notes: Vec<String>,
    /// The fingerprint of the tool list as the server last reported it, which
    /// `extensions.confirm_tools` takes to allow exactly the list a person saw; `None` before the
    /// core ever read its tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

/// What a server needs on the computer to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prerequisite {
    /// Node.js with `npx`.
    Npx,
    /// uv with `uvx`.
    Uvx,
    /// Docker.
    Docker,
    /// Another program, by the name the core looks for.
    Program { name: String },
}

impl Prerequisite {
    /// What is missing, for people, in the suite's language ("Needs Node.js (npx)").
    pub fn needs(&self) -> String {
        match self {
            Prerequisite::Npx => tr("Needs Node.js (npx)").into(),
            Prerequisite::Uvx => tr("Needs uv (uvx)").into(),
            Prerequisite::Docker => tr("Needs Docker").into(),
            Prerequisite::Program { name } => trf("Needs {program}", &[("program", name)]),
        }
    }
}

/// A secret a catalog entry needs, with where to get it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSecret {
    /// The variable or header it goes to.
    pub name: String,
    /// What it is, in English ("A GitHub token with read access to your repositories").
    pub hint: String,
    /// Where a person gets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A tool of a catalog entry and the access a call of it needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogTool {
    pub name: String,
    /// `read`, `write` or `delete`, never `admin`.
    pub access: Access,
}

/// One entry of the core's built-in catalog (`extensions.list_catalog`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Its id; installed, the server has the same id.
    pub id: String,
    pub name: String,
    /// Who makes the server.
    pub publisher: String,
    /// Its licence (`MIT`).
    pub license: String,
    /// Where it is described.
    pub url: String,
    /// What it does, in English.
    pub description: String,
    /// The fixed version the catalog installs.
    pub version: String,
    /// The publisher calls it experimental.
    pub experimental: bool,
    /// What it needs on the computer.
    pub requires: Vec<Prerequisite>,
    /// The secrets it needs.
    pub secrets: Vec<CatalogSecret>,
    /// Its tools with their access.
    pub tools: Vec<CatalogTool>,
    /// What of `requires` is missing on this computer now ([`Prerequisite::needs`]); installing
    /// waits until nothing is.
    pub missing: Vec<Prerequisite>,
}

/// What a person installs (`extensions.install_extension`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallFrom {
    /// The catalog entry `entry` ([`CatalogEntry::id`]), with the secrets it needs by name.
    Catalog {
        entry: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        secrets: BTreeMap<String, Secret>,
    },
    /// A person's own server.
    Custom {
        /// Its id ([`valid_id`]), unique.
        id: String,
        /// What people call it.
        name: String,
        server: ServerSpec,
    },
}

/// One part of a tool's answer, as MCP has it: text or a picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Text {
        text: String,
    },
    /// A picture, Base64 in `data`.
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
}

/// The answer of a server tool (`extensions.call_tool`): foreign text, data, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<Content>,
    /// The server says the call failed; `content` says why.
    pub is_error: bool,
    /// Always `true`: what a server answers comes from outside the suite.
    pub untrusted: bool,
}

/// Why a pasted `mcpServers` block was not read ([`parse_mcp_servers`]). `Display` says it in
/// English; the control centre says it in the suite's language and marks the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No JSON: the parser's message (where, never the text).
    Json(String),
    /// JSON, but no server in it.
    NoServers,
    /// The server `server` is not described as a program or an address; `reason` says what is wrong.
    Invalid { server: String, reason: &'static str },
    /// The server `server` would get the blocked variable `variable` ([`blocked_env`]).
    Blocked { server: String, variable: String },
    /// The server `server` speaks a transport Torro does not (`sse`).
    Transport { server: String, transport: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Json(message) => write!(f, "not valid JSON: {message}"),
            ParseError::NoServers => f.write_str("no server in the configuration"),
            ParseError::Invalid { server, reason } => write!(f, "server {server:?}: {reason}"),
            ParseError::Blocked { server, variable } => {
                write!(f, "server {server:?}: the variable {variable} cannot be set")
            }
            ParseError::Transport { server, transport } => {
                write!(f, "server {server:?}: the transport {transport:?} is not supported, only Streamable HTTP")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// The servers of a configuration in the format of Claude Desktop, as a person pastes it:
/// `{"mcpServers": {"<name>": {...}}}`, the inner object alone (`{"<name>": {...}}`) or either
/// without its outer braces (`"mcpServers": {...}`). A server is a program (`command`, `args`,
/// `env`, `type` left out or `stdio`) or an address (`url` with `headers`, `type` left out or
/// `http`, `streamable-http`, `streamableHttp`); every value is plain text ([`Value::Plain`]), the
/// form marks secrets. Other fields (`disabled`, `alwaysAllow`) are ignored. In the order of their
/// names.
///
/// Fails on the first server that is no program and no address, has a value that is no text, an
/// invalid variable or header name, a blocked variable ([`ParseError::Blocked`]) or an `sse`
/// address ([`ParseError::Transport`]). The address itself is checked when installing.
pub fn parse_mcp_servers(text: &str) -> Result<Vec<(String, ServerSpec)>, ParseError> {
    let parsed = serde_json::from_str::<serde_json::Value>(text)
        .or_else(|first| serde_json::from_str(&format!("{{{text}}}")).map_err(|_| first))
        .map_err(|err| ParseError::Json(err.to_string()))?;
    let Some(object) = parsed.as_object() else {
        return Err(ParseError::NoServers);
    };
    let servers = match object.get("mcpServers") {
        Some(serde_json::Value::Object(servers)) => servers,
        Some(_) => return Err(ParseError::NoServers),
        None => object,
    };
    if servers.is_empty() {
        return Err(ParseError::NoServers);
    }
    servers.iter().map(|(name, server)| Ok((name.clone(), server_of(name, server)?))).collect()
}

/// One server of [`parse_mcp_servers`].
fn server_of(name: &str, server: &serde_json::Value) -> Result<ServerSpec, ParseError> {
    let invalid = |reason| ParseError::Invalid { server: name.to_string(), reason };
    if name.trim().is_empty() {
        return Err(invalid("a server needs a name"));
    }
    let server = server.as_object().ok_or_else(|| invalid("is not an object"))?;
    let kind = match server.get("type") {
        None => None,
        Some(serde_json::Value::String(kind)) => Some(kind.as_str()),
        Some(_) => return Err(invalid("`type` is not text")),
    };
    let text = |field: &str| match server.get(field) {
        None => Ok(None),
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(invalid("a command or url is empty or not text")),
    };
    match (text("command")?, text("url")?, kind) {
        (Some(_), Some(_), _) => Err(invalid("has both a command and a url")),
        (Some(command), None, None | Some("stdio")) => {
            let args = match server.get("args") {
                None => Vec::new(),
                Some(serde_json::Value::Array(args)) => args
                    .iter()
                    .map(|arg| arg.as_str().map(str::to_string).ok_or_else(|| invalid("an argument is not text")))
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(invalid("`args` is not a list")),
            };
            let env = values_of(name, server.get("env"), true)?;
            Ok(ServerSpec::Stdio { command, args, env })
        }
        (None, Some(url), None | Some("http" | "streamable-http" | "streamableHttp" | "streamable_http")) => {
            Ok(ServerSpec::Http { url, headers: values_of(name, server.get("headers"), false)? })
        }
        (_, _, Some(transport @ "sse")) => {
            Err(ParseError::Transport { server: name.to_string(), transport: transport.to_string() })
        }
        (Some(_), None, Some(_)) | (None, Some(_), Some(_)) => Err(invalid("`type` does not fit its command or url")),
        (None, None, _) => Err(invalid("has neither a command nor a url")),
    }
}

/// The variables (`env: true`) or headers of one server, every value plain text.
fn values_of(
    server: &str,
    values: Option<&serde_json::Value>,
    env: bool,
) -> Result<BTreeMap<String, Value>, ParseError> {
    let invalid = |reason| ParseError::Invalid { server: server.to_string(), reason };
    let Some(values) = values else { return Ok(BTreeMap::new()) };
    let values = values.as_object().ok_or_else(|| invalid("`env` or `headers` is not an object"))?;
    let mut read = BTreeMap::new();
    for (name, value) in values {
        if env && !valid_env_name(name) {
            return Err(invalid("a variable has no valid name"));
        }
        if env && blocked_env(name) {
            return Err(ParseError::Blocked { server: server.to_string(), variable: name.clone() });
        }
        if !env && !valid_header_name(name) {
            return Err(invalid("a header has no valid name"));
        }
        let value = value.as_str().ok_or_else(|| invalid("a value of `env` or `headers` is not text"))?;
        read.insert(name.clone(), Value::Plain(value.to_string()));
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(value: &str) -> Value {
        Value::Plain(value.into())
    }

    #[test]
    fn ids_are_short_lowercase_and_start_with_a_letter() {
        for good in ["time", "github", "a", "my-server-2", &"a".repeat(MAX_ID_CHARS)] {
            assert!(valid_id(good), "{good}");
        }
        for bad in ["", "Time", "2fa", "-x", "my_server", "my server", "zeit-ä", &"a".repeat(MAX_ID_CHARS + 1)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn tool_names_go_there_and_back() {
        let name = tool_name("time", "get_current_time").expect("fits");
        assert_eq!(name, "ext_time_get_current_time");
        assert_eq!(parse_tool_name(&name), Some(("time", "get_current_time")));
        let dashed = tool_name("my-server", "list-issues").expect("fits");
        assert_eq!(parse_tool_name(&dashed), Some(("my-server", "list-issues")));
        // Exactly 64 characters fit, one more does not.
        let longest = "t".repeat(MAX_TOOL_NAME_CHARS - "ext_time_".len());
        assert_eq!(tool_name("time", &longest).map(|name| name.len()), Some(MAX_TOOL_NAME_CHARS));
        assert_eq!(tool_name("time", &format!("{longest}x")), None, "too long");
        for bad in ["", "get time", "get.time", "zeit_ä", "a/b"] {
            assert_eq!(tool_name("time", bad), None, "{bad:?}");
        }
        assert_eq!(tool_name("Time", "now"), None, "the id must be valid");
        for bad in ["time_now", "ext_time", "ext__now", "ext_Time_now", "calendar_list_events", "ext_time_get time"] {
            assert_eq!(parse_tool_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn denied_names_go_there_and_back() {
        let name = denied_name("github", "create_issue");
        assert_eq!(name, "ext.github.create_issue");
        assert_eq!(parse_denied_name(&name), Some(("github", "create_issue")));
        for bad in ["ext.github", "ext.Git.create", "ext..x", "ext.github.", "github.create_issue", "ext.github.a b"] {
            assert_eq!(parse_denied_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn blocked_variables_are_refused_in_any_case() {
        for blocked in [
            "PATH",
            "path",
            "PathExt",
            "LD_PRELOAD",
            "ld_preload",
            "DYLD_FOO",
            "TORRO_TOKEN",
            "NODE_OPTIONS",
            "IFS",
            "ENV",
            "DOCKER_HOST",
            "npm_config_registry",
            "PYTHONWARNINGS",
            "JAVA_TOOL_OPTIONS",
            "ComSpec",
            "HOME",
            "DOCKER_CONFIG",
            "UV_INDEX_URL",
            "PIP_INDEX_URL",
            "GIT_CONFIG_COUNT",
            "BASH_FUNC_ls%%",
            "SSL_CERT_FILE",
            "GIT_PROXY_COMMAND",
            "XDG_CACHE_HOME",
        ] {
            assert!(blocked_env(blocked), "{blocked}");
        }
        for open in
            ["GITHUB_PERSONAL_ACCESS_TOKEN", "HOME_DIR", "ENVIRONMENT", "MY_PATH", "LDAP_URL", "HTTPS_PROXY", "LANG"]
        {
            assert!(!blocked_env(open), "{open}");
        }
        assert!(valid_env_name("API_KEY") && !valid_env_name("") && !valid_env_name("A=B") && !valid_env_name("A B"));
        assert!(valid_header_name("Authorization") && !valid_header_name("X Y") && !valid_header_name("A:"));
    }

    /// The examples of Claude Desktop's documentation, with and without the outer object.
    #[test]
    fn a_claude_desktop_configuration_reads() {
        let stdio = r#"{"mcpServers": {"filesystem": {"command": "npx", "args": ["-y",
            "@modelcontextprotocol/server-filesystem", "/Users/anna/Desktop"], "env": {"DEBUG": "1"}}}}"#;
        let read = parse_mcp_servers(stdio).expect("reads");
        assert_eq!(
            read,
            vec![(
                "filesystem".to_string(),
                ServerSpec::Stdio {
                    command: "npx".into(),
                    args: vec![
                        "-y".into(),
                        "@modelcontextprotocol/server-filesystem".into(),
                        "/Users/anna/Desktop".into()
                    ],
                    env: BTreeMap::from([("DEBUG".to_string(), plain("1"))]),
                }
            )]
        );
        let url = r#"{"github": {"type": "http", "url": "https://api.githubcopilot.com/mcp/",
            "headers": {"Authorization": "Bearer ghp_example"}}, "time": {"command": "uvx", "args": ["mcp-server-time"]}}"#;
        let read = parse_mcp_servers(url).expect("reads without the outer object");
        assert_eq!(read.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["github", "time"]);
        assert_eq!(
            read[0].1,
            ServerSpec::Http {
                url: "https://api.githubcopilot.com/mcp/".into(),
                headers: BTreeMap::from([("Authorization".to_string(), plain("Bearer ghp_example"))]),
            }
        );
        let braceless = r#""mcpServers": {"time": {"command": "uvx", "args": ["mcp-server-time"], "disabled": false}}"#;
        let read = parse_mcp_servers(braceless).expect("reads without braces");
        assert!(matches!(&read[0].1, ServerSpec::Stdio { command, env, .. } if command == "uvx" && env.is_empty()));
        let bare_url = parse_mcp_servers(r#"{"local": {"url": "http://127.0.0.1:8000/mcp"}}"#).expect("reads");
        assert!(matches!(&bare_url[0].1, ServerSpec::Http { headers, .. } if headers.is_empty()));
    }

    #[test]
    fn a_broken_or_dangerous_configuration_is_refused() {
        assert!(matches!(
            parse_mcp_servers(r#"{"mcpServers": {"time": {"command": "uvx",}}"#),
            Err(ParseError::Json(_))
        ));
        assert_eq!(parse_mcp_servers("{}"), Err(ParseError::NoServers));
        assert_eq!(parse_mcp_servers(r#"{"mcpServers": {}}"#), Err(ParseError::NoServers));
        assert_eq!(parse_mcp_servers("[1, 2]"), Err(ParseError::NoServers));
        let preload = r#"{"mcpServers": {"evil": {"command": "node", "env": {"ld_preload": "/tmp/x.so"}}}}"#;
        assert_eq!(
            parse_mcp_servers(preload),
            Err(ParseError::Blocked { server: "evil".into(), variable: "ld_preload".into() })
        );
        let torro = r#"{"x": {"command": "node", "env": {"TORRO_TOKEN": "t"}}}"#;
        assert!(matches!(parse_mcp_servers(torro), Err(ParseError::Blocked { .. })));
        let sse = r#"{"old": {"type": "sse", "url": "https://example.org/sse"}}"#;
        assert_eq!(
            parse_mcp_servers(sse),
            Err(ParseError::Transport { server: "old".into(), transport: "sse".into() })
        );
        for invalid in [
            r#"{"x": {"command": "npx", "url": "https://example.org"}}"#,
            r#"{"x": {"args": ["a"]}}"#,
            r#"{"x": {"command": ""}}"#,
            r#"{"x": {"command": "npx", "args": "-y"}}"#,
            r#"{"x": {"command": "npx", "args": [1]}}"#,
            r#"{"x": {"command": "npx", "env": {"N": 1}}}"#,
            r#"{"x": {"command": "npx", "env": {"A B": "1"}}}"#,
            r#"{"x": {"command": "npx", "type": "http"}}"#,
            r#"{"x": {"url": "https://example.org", "headers": {"X Y": "1"}}}"#,
            r#"{"x": "npx"}"#,
            r#"{"mcpServers": {"": {"command": "npx"}}}"#,
        ] {
            assert!(matches!(parse_mcp_servers(invalid), Err(ParseError::Invalid { .. })), "{invalid}");
        }
        assert!(ParseError::NoServers.to_string().contains("no server"));
    }

    /// A secret goes to the core as text and comes back only as `stored`; the shapes on the wire.
    #[test]
    fn values_and_specs_on_the_wire() {
        let spec = ServerSpec::Stdio {
            command: "docker".into(),
            args: vec!["run".into(), "-i".into(), "--rm".into()],
            env: BTreeMap::from([
                ("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), Value::Secret(Secret::new("ghp_1"))),
                ("GITHUB_HOST".to_string(), plain("github.com")),
            ]),
        };
        let json = serde_json::to_value(&spec).expect("json");
        assert_eq!(json["transport"], "stdio");
        assert_eq!(json["env"]["GITHUB_PERSONAL_ACCESS_TOKEN"], serde_json::json!({"secret": "ghp_1"}));
        assert_eq!(json["env"]["GITHUB_HOST"], serde_json::json!({"plain": "github.com"}));
        assert_eq!(serde_json::from_value::<ServerSpec>(json).expect("back"), spec);
        assert!(!format!("{spec:?}").contains("ghp_1"), "no secret in Debug");
        let stored = serde_json::to_value(Value::Stored { secret: true }).expect("json");
        assert_eq!(stored, serde_json::json!({"stored": {"secret": true}}));
        let http: ServerSpec =
            serde_json::from_value(serde_json::json!({"transport": "http", "url": "https://x.org/mcp"}))
                .expect("reads");
        assert_eq!(http.values(), &BTreeMap::new());

        let crashed = serde_json::to_value(Status::Crashed { reason: CrashReason::StartTimeout }).expect("json");
        assert_eq!(crashed, serde_json::json!({"state": "crashed", "reason": "start_timeout"}));
        let missing = serde_json::to_value(Status::Missing { what: Prerequisite::Npx }).expect("json");
        assert_eq!(missing, serde_json::json!({"state": "missing", "what": {"kind": "npx"}}));
        let result = ToolResult {
            content: vec![
                Content::Text { text: "10:30".into() },
                Content::Image { data: "iVBO".into(), mime_type: "image/png".into() },
            ],
            is_error: false,
            untrusted: true,
        };
        let json = serde_json::to_value(&result).expect("json");
        assert_eq!(json["content"][1], serde_json::json!({"type": "image", "data": "iVBO", "mimeType": "image/png"}));
        assert_eq!(
            (json["isError"].clone(), json["untrusted"].clone()),
            (serde_json::json!(false), serde_json::json!(true))
        );
        let from = InstallFrom::Catalog { entry: "time".into(), secrets: BTreeMap::new() };
        assert_eq!(serde_json::to_value(&from).expect("json"), serde_json::json!({"kind": "catalog", "entry": "time"}));
    }

    #[test]
    fn what_is_missing_is_said_in_words() {
        assert_eq!(Prerequisite::Npx.needs(), tr("Needs Node.js (npx)"));
        assert_eq!(Prerequisite::Uvx.needs(), tr("Needs uv (uvx)"));
        assert_eq!(Prerequisite::Docker.needs(), tr("Needs Docker"));
        let git = Prerequisite::Program { name: "git".into() };
        assert_eq!(git.needs(), trf("Needs {program}", &[("program", &"git")]));
    }
}
