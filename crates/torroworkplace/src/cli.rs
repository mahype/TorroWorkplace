//! The command-line clients of the core: `ctl setups …`, `ctl clients …`, `status` and `notify`.
//!
//! Each command becomes one [`Request`], goes to the daemon over the local socket and prints
//! the [`Response`]: short lines for people, or the response as JSON with `--json` for bars
//! and scripts. [`parse`] only reads the arguments and is tested without a socket; [`run`]
//! talks to the core. Exit codes: 0 done, 1 failed or core not running, 3 a load found the
//! workspace not empty, 64 bad arguments.
//!
//! `ctl clients` connects agents and scripts before the control centre can (A.6): `add` prints
//! the new key once, `new-key` a replacement; neither can be shown again.
//!
//! `ctl approvals` lists what agents wait for and `ctl allow|deny <id>` decides, with the app key
//! like the control centre (A.3b): a way to decide on systems without notification buttons and
//! before the control centre has its approvals page (A.8).
//!
//! `notify` (E1.3) posts a notice to the inbox for scripts: `inbox.post_item` with kind `notice`.
//! Unlike `ctl` it sends the client key in `TORRO_TOKEN` when there is one, so the entry belongs
//! to that script's source; without, it sends the app key and the core files it under
//! `script:cli`. The core decides the source, never this command. A core that is not running is
//! an error (exit code 1): a script should learn that its notice went nowhere.

use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use torro_core::i18n::{tr, trf};
use torro_inbox::{AgentKind, Kind, Lane, Link};
use torro_ipc::api::{Rights, RightsTemplate};
use torro_ipc::{ApprovalInfo, ApprovalState, ClientInfo, IpcError, Key, Request, Response, socket_path};
use torro_setups::{LoadMode, TemplateSummary};

const USAGE: &str = "usage:
  torroworkplace status [--json]
  torroworkplace ctl setups list [--json]
  torroworkplace ctl setups save [<name>] [--json]
  torroworkplace ctl setups update <id> [--json]
  torroworkplace ctl setups delete <id> [--json]
  torroworkplace ctl setups load <id> [--mode empty|add|replace] [--json]
  torroworkplace ctl clients list [--json]
  torroworkplace ctl clients add <name> [--kind claude-code|codex|claude-desktop|cursor|script|hook]
                                        [--agent claude-code|codex|gemini]
                                        [--template read-only|read-and-write|full-access] [--json]
  torroworkplace ctl clients remove <id> [--json]
  torroworkplace ctl clients new-key <id> [--json]
  torroworkplace ctl approvals [--json]
  torroworkplace ctl allow <id> [--json]
  torroworkplace ctl deny <id> [--json]
  torroworkplace ctl quit [--json]
  torroworkplace notify --title <text> [--body <text>] [--url <url>] [--for-me] [--thread <key>] [--json]
  torroworkplace hook claude-code|codex|gemini [--key-file <file>]
                                                 (run by an agent's hooks; reads stdin, prints nothing)";

/// The kind of a client added without `--kind`.
const DEFAULT_KIND: &str = "script";

/// The options that take a value, as given on the command line.
#[derive(Debug, Default)]
struct Options<'a> {
    mode: Option<&'a str>,
    kind: Option<&'a str>,
    /// The agent a `hook` client reports for (task E1.2b binds every hook client to one).
    agent: Option<&'a str>,
    template: Option<&'a str>,
}

/// One parsed command line: what to ask the core and how to print the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub request: Request,
    /// `--json`: print the response as JSON instead of text.
    pub json: bool,
}

/// The arguments were not a valid command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(String);

impl UsageError {
    fn new(message: impl Into<String>) -> UsageError {
        UsageError(message.into())
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// Entry point from `main`: `args` are the arguments after the binary name, starting with
/// `status`, `ctl` or `notify`.
pub fn from_args(args: &[String]) -> ExitCode {
    if args.first().is_some_and(|head| head == "notify") {
        return notify_from_args(&args[1..]);
    }
    match parse(args) {
        Ok(command) => run(command),
        Err(err) => {
            eprintln!("{}: {err}\n\n{USAGE}", crate::APP.binary);
            ExitCode::from(64)
        }
    }
}

/// Turns the arguments (starting with `status` or `ctl`) into a [`Command`].
/// `--json` works everywhere; `--mode` only for `ctl setups load`, `--kind` and `--template`
/// only for `ctl clients add`.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let Some((head, rest)) = args.split_first() else {
        return Err(UsageError::new("missing command"));
    };

    let mut json = false;
    let mut options = Options::default();
    let mut words: Vec<&str> = Vec::new();
    let mut rest_iter = rest.iter();
    while let Some(arg) = rest_iter.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg.as_str(), None),
        };
        let slot = match flag {
            "--json" if inline.is_none() => {
                json = true;
                continue;
            }
            "--mode" => &mut options.mode,
            "--kind" => &mut options.kind,
            "--agent" => &mut options.agent,
            "--template" => &mut options.template,
            flag if flag.starts_with('-') => return Err(UsageError::new(format!("unknown option {arg}"))),
            word => {
                words.push(word);
                continue;
            }
        };
        let value = match inline {
            Some(value) => value,
            None => rest_iter.next().map(String::as_str).ok_or_else(|| missing_value(flag))?,
        };
        *slot = Some(value);
    }

    let request = match head.as_str() {
        "status" => {
            if !words.is_empty() {
                return Err(UsageError::new("status takes no arguments"));
            }
            Request::Status
        }
        "ctl" => ctl_request(&words, &options)?,
        other => return Err(UsageError::new(format!("unknown command {other:?}"))),
    };

    if options.mode.is_some() && !matches!(request, Request::LoadTemplate { .. }) {
        return Err(UsageError::new("--mode only applies to ctl setups load"));
    }
    let client_options = options.kind.is_some() || options.agent.is_some() || options.template.is_some();
    if client_options && !matches!(request, Request::CreateClient { .. }) {
        return Err(UsageError::new(tr("--kind, --agent and --template only apply to ctl clients add")));
    }
    Ok(Command { request, json })
}

/// The error for an option given without its value.
fn missing_value(flag: &str) -> UsageError {
    match flag {
        "--mode" => UsageError::new("--mode needs a value: empty, add or replace"),
        "--kind" => UsageError::new(tr("--kind needs a value, e.g. claude-code or script")),
        "--agent" => UsageError::new(tr("--agent needs a value: claude-code, codex or gemini")),
        _ => UsageError::new(tr("--template needs a value: read-only, read-and-write or full-access")),
    }
}

fn ctl_request(words: &[&str], options: &Options) -> Result<Request, UsageError> {
    match words {
        ["setups", verb, args @ ..] => setups_request(verb, args, options.mode),
        ["setups"] => Err(UsageError::new("ctl setups needs a subcommand: list, save, update, delete or load")),
        ["clients", verb, args @ ..] => clients_request(verb, args, options),
        ["clients"] => Err(UsageError::new(tr("ctl clients needs a subcommand: list, add, remove or new-key"))),
        ["approvals"] => Ok(Request::ListApprovals),
        ["approvals", ..] => Err(UsageError::new(tr("ctl approvals takes no arguments"))),
        [verb @ ("allow" | "deny"), args @ ..] => decide_request(verb, args),
        ["quit"] => Ok(Request::Quit),
        ["quit", ..] => Err(UsageError::new("ctl quit takes no arguments")),
        [] => Err(UsageError::new("ctl needs a group, e.g. `ctl setups list`")),
        [other, ..] => Err(UsageError::new(trf(
            "unknown ctl group {group}; try setups, clients, approvals, allow, deny or quit",
            &[("group", &format!("{other:?}"))],
        ))),
    }
}

/// `ctl allow <id>` and `ctl deny <id>`: a person's decision on an open approval.
fn decide_request(verb: &str, args: &[&str]) -> Result<Request, UsageError> {
    match args {
        [id] => Ok(Request::DecideApproval { id: id.to_string(), allow: verb == "allow" }),
        [] => Err(UsageError::new(trf(
            "ctl {verb} needs the id of an approval; `ctl approvals` lists them",
            &[("verb", &verb)],
        ))),
        _ => Err(UsageError::new(trf("too many arguments for ctl {verb}", &[("verb", &verb)]))),
    }
}

/// `ctl clients …`: the agents and scripts that may use the API.
fn clients_request(verb: &str, args: &[&str], options: &Options) -> Result<Request, UsageError> {
    match (verb, args) {
        ("list", []) => Ok(Request::ListClients),
        ("add", [name]) => Ok(Request::CreateClient {
            name: name.to_string(),
            kind: options.kind.unwrap_or(DEFAULT_KIND).to_string(),
            template: parse_template(options.template)?,
            agent: parse_agent(options.agent)?,
        }),
        ("remove", [id]) => Ok(Request::RemoveClient { id: id.to_string() }),
        ("new-key", [id]) => Ok(Request::NewClientKey { id: id.to_string() }),
        ("add", []) => Err(UsageError::new(tr("ctl clients add needs a name"))),
        ("remove" | "new-key", []) => {
            Err(UsageError::new(trf("ctl clients {verb} needs a client id", &[("verb", &verb)])))
        }
        ("list" | "add" | "remove" | "new-key", _) => {
            Err(UsageError::new(trf("too many arguments for ctl clients {verb}", &[("verb", &verb)])))
        }
        _ => Err(UsageError::new(trf(
            "unknown subcommand {verb}; try list, add, remove or new-key",
            &[("verb", &format!("{verb:?}"))],
        ))),
    }
}

/// `--agent` for a hook client: the agent it reports for; left out, none (the core then refuses
/// a `hook` client, task E1.2b).
fn parse_agent(agent: Option<&str>) -> Result<Option<AgentKind>, UsageError> {
    agent
        .map(|id| {
            AgentKind::from_id(id)
                .ok_or_else(|| UsageError::new(tr("--agent needs a value: claude-code, codex or gemini")))
        })
        .transpose()
}

/// `--template`; without it the standard template, *Read and write*.
fn parse_template(template: Option<&str>) -> Result<RightsTemplate, UsageError> {
    match template {
        None => Ok(RightsTemplate::default()),
        Some("read-only") => Ok(RightsTemplate::ReadOnly),
        Some("read-and-write") => Ok(RightsTemplate::ReadAndWrite),
        Some("full-access") => Ok(RightsTemplate::FullAccess),
        Some(other) => Err(UsageError::new(trf(
            "unknown template {template}; use read-only, read-and-write or full-access",
            &[("template", &format!("{other:?}"))],
        ))),
    }
}

fn setups_request(verb: &str, args: &[&str], mode: Option<&str>) -> Result<Request, UsageError> {
    match (verb, args) {
        ("list", []) => Ok(Request::ListTemplates),
        ("save", []) => Ok(Request::SaveTemplate { name: None }),
        ("save", [name]) => Ok(Request::SaveTemplate { name: Some(name.to_string()) }),
        ("update", [id]) => Ok(Request::UpdateTemplate { id: id.to_string() }),
        ("delete", [id]) => Ok(Request::DeleteTemplate { id: id.to_string() }),
        ("load", [id]) => Ok(Request::LoadTemplate { id: id.to_string(), mode: parse_mode(mode)? }),
        ("update" | "delete" | "load", []) => Err(UsageError::new(format!("ctl setups {verb} needs a template id"))),
        ("list" | "save" | "update" | "delete" | "load", _) => {
            Err(UsageError::new(format!("too many arguments for ctl setups {verb}")))
        }
        _ => Err(UsageError::new(format!("unknown subcommand {verb:?}; try list, save, update, delete or load"))),
    }
}

fn parse_mode(mode: Option<&str>) -> Result<LoadMode, UsageError> {
    match mode {
        None => Ok(LoadMode::default()),
        Some("empty") => Ok(LoadMode::Empty),
        Some("add") => Ok(LoadMode::Add),
        Some("replace") => Ok(LoadMode::Replace),
        Some(other) => Err(UsageError::new(format!("unknown mode {other:?}; use empty, add or replace"))),
    }
}

/// Sends the command to the core and prints the answer.
pub fn run(command: Command) -> ExitCode {
    let Some(path) = socket_path() else {
        eprintln!("{}: no state directory, so no core socket to look for", crate::APP.binary);
        return ExitCode::FAILURE;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{}: {err}", crate::APP.binary);
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(torro_ipc::client::call(&path, &command.request)) {
        Ok(response) => show(&response, command.json),
        // `NotRunning` already says how to start the core.
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Prints a response. Errors go to stderr with the exit code from [`error_exit_status`].
fn show(response: &Response, json: bool) -> ExitCode {
    if json {
        match serde_json::to_string_pretty(response) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("{err}"),
        }
    }
    match response {
        Response::Error { code, message } => {
            eprintln!("{message}");
            ExitCode::from(error_exit_status(code))
        }
        _ => {
            if !json {
                println!("{}", render(response));
            }
            ExitCode::SUCCESS
        }
    }
}

/// `torroworkplace notify …` with `args` after `notify`: parses ([`parse_notify`]), takes the key
/// from [`crate::mcp::TOKEN_VAR`] ([`token_from`]) and posts ([`run_notify`]). Exit codes: 0
/// posted (or dropped because the source is muted), 1 refused or core not running, 64 bad
/// arguments.
pub fn notify_from_args(args: &[String]) -> ExitCode {
    let command = match parse_notify(args) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("{}: {err}\n\n{USAGE}", crate::APP.binary);
            return ExitCode::from(64);
        }
    };
    let Some(socket) = socket_path() else {
        eprintln!("{}: no state directory, so no core socket to look for", crate::APP.binary);
        return ExitCode::FAILURE;
    };
    let token = token_from(std::env::var(crate::mcp::TOKEN_VAR).ok());
    run_notify(&command, &socket, token.as_ref())
}

/// The arguments after `notify` as an `inbox.post_item` with kind `notice`: `--title` (required,
/// not empty), `--body`, `--url` (a link to open), `--thread` (the `thread_key` that merges
/// reports of one matter), `--for-me` (lane `for_me`; else the core's default, `info`) and
/// `--json`. Values come as the next argument or after `=`.
pub fn parse_notify(args: &[String]) -> Result<Command, UsageError> {
    let (mut title, mut body, mut url, mut thread) = (None, None, None, None);
    let (mut for_me, mut json) = (false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg.as_str(), None),
        };
        let slot = match flag {
            "--for-me" | "--json" if inline.is_some() => {
                return Err(UsageError::new(trf("{flag} takes no value", &[("flag", &flag)])));
            }
            "--for-me" => {
                for_me = true;
                continue;
            }
            "--json" => {
                json = true;
                continue;
            }
            "--title" => &mut title,
            "--body" => &mut body,
            "--url" => &mut url,
            "--thread" => &mut thread,
            flag if flag.starts_with('-') => {
                return Err(UsageError::new(trf("unknown option {option}", &[("option", &arg)])));
            }
            _ => {
                return Err(UsageError::new(trf(
                    "notify takes no words, only options; put the text in --title \"{word}\"",
                    &[("word", &arg)],
                )));
            }
        };
        let value = match inline {
            Some(value) => value,
            None => rest
                .next()
                .map(String::as_str)
                .ok_or_else(|| UsageError::new(trf("{flag} needs a value", &[("flag", &flag)])))?,
        };
        *slot = Some(value.to_string());
    }
    let title = title
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| UsageError::new(tr("notify needs a title: --title \"Backup finished\"")))?;
    let request = Request::PostItem {
        kind: Kind::Notice,
        title,
        body,
        link: url.map(|url| Link::Url { url }),
        lane: for_me.then_some(Lane::ForMe),
        thread_key: thread,
        agent: None,
    };
    Ok(Command { request, json })
}

/// The client key in `TORRO_TOKEN` (`value` is the variable's content), `None` when it is
/// missing or blank: then [`run_notify`] sends the app key.
pub fn token_from(value: Option<String>) -> Option<Key> {
    value.as_deref().map(str::trim).filter(|key| !key.is_empty()).map(Key::new)
}

/// Posts `command` to the core at `socket`, with `token` when there is one, else with the app key
/// next to the socket ([`torro_ipc::client::call`]), and prints the answer like `ctl`. A core
/// that is not running is a message and [`ExitCode::FAILURE`]; nothing is started.
fn run_notify(command: &Command, socket: &Path, token: Option<&Key>) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{}: {err}", crate::APP.binary);
            return ExitCode::FAILURE;
        }
    };
    let answer = runtime.block_on(async {
        match token {
            Some(key) => torro_ipc::client::call_with_key(socket, Some(key), &command.request).await,
            None => torro_ipc::client::call(socket, &command.request).await,
        }
    });
    match answer {
        Ok(response) => show(&response, command.json),
        Err(IpcError::NotRunning) => {
            eprintln!(
                "{}",
                tr(
                    "Torro Workplace isn't running, so nothing reached the inbox. Start it with `torroworkplace daemon`."
                )
            );
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Exit code for a failed request: 3 when a load found the workspace not empty, otherwise 1.
pub fn error_exit_status(code: &str) -> u8 {
    match code {
        "workspace-not-empty" => 3,
        _ => 1,
    }
}

/// Human-readable text for a successful response.
pub fn render(response: &Response) -> String {
    match response {
        Response::Pong { version, .. } => format!("torroworkplace {version}"),
        Response::Status(status) => format!(
            "torroworkplace {}\nbackend:   {}\nworkspace: {}\ntemplates: {}\nbusy:      {}",
            status.version,
            status.backend,
            status.workspace.as_deref().unwrap_or("-"),
            status.templates,
            if status.busy { "yes" } else { "no" },
        ),
        Response::Templates { templates } => {
            if templates.is_empty() {
                return "no templates".to_string();
            }
            let width = templates.iter().map(|t| t.id.chars().count()).max().unwrap_or(0);
            templates.iter().map(|t| template_line(t, width)).collect::<Vec<_>>().join("\n")
        }
        Response::Saved { template } => format!("saved {}", template_line(template, 0)),
        Response::Loaded { restored, failed } => {
            let mut text = format!("restored {restored} {}", plural(*restored, "window", "windows"));
            for (app, reason) in failed {
                text.push_str(&format!("\nfailed {app}: {reason}"));
            }
            text
        }
        Response::Clients { clients } => {
            if clients.is_empty() {
                return tr("No agents or scripts are connected.").to_string();
            }
            clients.iter().map(client_line).collect::<Vec<_>>().join("\n")
        }
        Response::ClientKey { client, key } => format!(
            "{}\n{}",
            trf("Key for {name} ({id}). It is shown only this once:", &[("name", &client.name), ("id", &client.id)]),
            key.expose(),
        ),
        Response::Approvals { approvals } => {
            if approvals.is_empty() {
                return tr("No commands wait for approval.").to_string();
            }
            approvals.iter().map(approval_line).collect::<Vec<_>>().join("\n")
        }
        Response::Approval { approval } => approval_line(approval),
        Response::Posted { id: Some(id) } => trf("Posted to the inbox ({id}).", &[("id", id)]),
        Response::Posted { id: None } => tr("Not posted: this source is muted in the inbox.").to_string(),
        Response::Ok => "ok".to_string(),
        Response::Error { message, .. } => message.clone(),
        // Calendar and task answers are for the apps; show them as JSON.
        other => serde_json::to_string_pretty(other).unwrap_or_else(|err| err.to_string()),
    }
}

/// `id  name  (n windows)`, with the id padded to `id_width` so the names line up.
fn template_line(template: &TemplateSummary, id_width: usize) -> String {
    format!(
        "{:<id_width$}  {}  ({} {})",
        template.id,
        template.name,
        template.windows,
        plural(template.windows, "window", "windows"),
    )
}

/// `id  name  kind  rights  last use`; ids all have the same length, so the columns line up.
fn client_line(client: &ClientInfo) -> String {
    let last_used = match client.last_used {
        Some(time) => {
            trf("last used {time}", &[("time", &time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"))])
        }
        None => tr("never used").to_string(),
    };
    format!("{}  {}  {}  {}  {last_used}", client.id, client.name, client.kind, rights_word(&client.rights))
}

/// `id  state  client: summary`; the id is what `ctl allow` and `ctl deny` take. The summary is
/// the core's ([`crate::approvals::summary`]), already one line without control characters.
fn approval_line(approval: &ApprovalInfo) -> String {
    let local = |time: chrono::DateTime<chrono::Utc>| time.with_timezone(&chrono::Local).format("%H:%M").to_string();
    let state = match &approval.state {
        ApprovalState::Pending => trf("waiting until {time}", &[("time", &local(approval.expires))]),
        ApprovalState::Running => tr("running").to_string(),
        ApprovalState::Done { response } => match response.as_ref() {
            Response::Error { message, .. } => {
                // A terminal would act on escape sequences; the message may quote a client's ids.
                let message: String = message.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
                trf("ran and failed: {message}", &[("message", &message)])
            }
            _ => tr("done").to_string(),
        },
        ApprovalState::Denied => tr("denied").to_string(),
        ApprovalState::Expired => tr("expired").to_string(),
    };
    format!("{}  {state}  {}: {}", approval.id, approval.client_name, approval.summary)
}

/// The template the rights match, or that they were changed afterwards.
fn rights_word(rights: &Rights) -> &'static str {
    match rights.template() {
        Some(RightsTemplate::ReadOnly) => tr("Read only"),
        Some(RightsTemplate::ReadAndWrite) => tr("Read and write"),
        Some(RightsTemplate::FullAccess) => tr("Full access"),
        None => tr("Custom rights"),
    }
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use torro_ipc::Status;

    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, UsageError> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&args)
    }

    fn request(args: &[&str]) -> Request {
        parse_strs(args).map(|c| c.request).unwrap_or_else(|err| panic!("{args:?}: {err}"))
    }

    fn summary(id: &str, name: &str, windows: usize) -> TemplateSummary {
        TemplateSummary {
            id: id.into(),
            name: name.into(),
            windows,
            updated: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
        }
    }

    #[test]
    fn status_with_and_without_json() {
        assert_eq!(parse_strs(&["status"]), Ok(Command { request: Request::Status, json: false }));
        assert_eq!(parse_strs(&["status", "--json"]), Ok(Command { request: Request::Status, json: true }));
    }

    #[test]
    fn status_takes_no_arguments() {
        assert!(parse_strs(&["status", "extra"]).is_err());
        assert!(parse_strs(&["status", "--mode", "add"]).is_err());
    }

    #[test]
    fn setups_list() {
        assert_eq!(request(&["ctl", "setups", "list"]), Request::ListTemplates);
        let command = parse_strs(&["ctl", "setups", "list", "--json"]).unwrap();
        assert!(command.json);
    }

    #[test]
    fn setups_save_with_and_without_name() {
        assert_eq!(request(&["ctl", "setups", "save"]), Request::SaveTemplate { name: None });
        assert_eq!(request(&["ctl", "setups", "save", "dev"]), Request::SaveTemplate { name: Some("dev".into()) });
    }

    #[test]
    fn setups_update_and_delete_need_an_id() {
        assert_eq!(request(&["ctl", "setups", "update", "dev"]), Request::UpdateTemplate { id: "dev".into() });
        assert_eq!(request(&["ctl", "setups", "delete", "dev"]), Request::DeleteTemplate { id: "dev".into() });
        assert!(parse_strs(&["ctl", "setups", "update"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "delete"]).is_err());
    }

    #[test]
    fn setups_load_modes() {
        let load = |mode| Request::LoadTemplate { id: "dev".into(), mode };
        assert_eq!(request(&["ctl", "setups", "load", "dev"]), load(LoadMode::Empty));
        assert_eq!(request(&["ctl", "setups", "load", "dev", "--mode", "add"]), load(LoadMode::Add));
        assert_eq!(request(&["ctl", "setups", "load", "dev", "--mode=replace"]), load(LoadMode::Replace));
        assert_eq!(request(&["ctl", "setups", "load", "dev", "--mode", "empty"]), load(LoadMode::Empty));
    }

    #[test]
    fn setups_load_needs_an_id_and_a_valid_mode() {
        assert!(parse_strs(&["ctl", "setups", "load"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "load", "dev", "--mode", "merge"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "load", "dev", "--mode"]).is_err());
    }

    #[test]
    fn mode_only_applies_to_load() {
        assert!(parse_strs(&["ctl", "setups", "save", "--mode", "add"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "list", "--mode=add"]).is_err());
    }

    #[test]
    fn json_flag_may_come_first() {
        let command = parse_strs(&["ctl", "--json", "setups", "delete", "dev"]).unwrap();
        assert!(command.json);
        assert_eq!(command.request, Request::DeleteTemplate { id: "dev".into() });
    }

    #[test]
    fn unknown_words_are_usage_errors() {
        assert!(parse_strs(&[]).is_err());
        assert!(parse_strs(&["fly"]).is_err());
        assert!(parse_strs(&["ctl"]).is_err());
        assert!(parse_strs(&["ctl", "frobnicate"]).is_err());
        assert!(parse_strs(&["ctl", "setups"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "frobnicate"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "list", "extra"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "save", "a", "b"]).is_err());
        assert!(parse_strs(&["ctl", "setups", "list", "--verbose"]).is_err());
    }

    #[test]
    fn ctl_quit_asks_the_core_to_stop() {
        assert_eq!(request(&["ctl", "quit"]), Request::Quit);
        assert!(parse_strs(&["ctl", "quit", "now"]).is_err());
    }

    #[test]
    fn clients_list_remove_and_new_key() {
        assert_eq!(request(&["ctl", "clients", "list"]), Request::ListClients);
        assert_eq!(request(&["ctl", "clients", "remove", "a1b2c3d4"]), Request::RemoveClient { id: "a1b2c3d4".into() });
        assert_eq!(
            request(&["ctl", "clients", "new-key", "a1b2c3d4"]),
            Request::NewClientKey { id: "a1b2c3d4".into() }
        );
        assert!(parse_strs(&["ctl", "clients", "remove"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "new-key"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "list", "extra"]).is_err());
        assert!(parse_strs(&["ctl", "clients"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "frobnicate"]).is_err());
    }

    #[test]
    fn clients_add_with_name_kind_and_template() {
        let add = |kind: &str, template| Request::CreateClient {
            name: "Claude Code".into(),
            kind: kind.into(),
            template,
            agent: None,
        };
        assert_eq!(request(&["ctl", "clients", "add", "Claude Code"]), add("script", RightsTemplate::ReadAndWrite));
        assert_eq!(
            request(&["ctl", "clients", "add", "Claude Code", "--kind", "claude-code", "--template=read-only"]),
            add("claude-code", RightsTemplate::ReadOnly)
        );
        assert_eq!(
            request(&["ctl", "clients", "add", "--template", "full-access", "Claude Code", "--kind=codex"]),
            add("codex", RightsTemplate::FullAccess)
        );
        assert!(parse_strs(&["ctl", "clients", "add"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "add", "A", "B"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "add", "A", "--template", "everything"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "add", "A", "--kind"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "list", "--kind", "codex"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "list", "--agent", "codex"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "add", "H", "--kind", "hook", "--agent", "nobody"]).is_err());
        let Request::CreateClient { agent, kind, .. } =
            request(&["ctl", "clients", "add", "Claude Code hooks", "--kind", "hook", "--agent", "claude-code"])
        else {
            panic!("ctl clients add asks for a new client");
        };
        assert_eq!((kind.as_str(), agent), ("hook", Some(AgentKind::ClaudeCode)));
        assert!(parse_strs(&["ctl", "setups", "list", "--template", "read-only"]).is_err());
        assert!(parse_strs(&["ctl", "clients", "add", "A", "--mode", "add"]).is_err());
    }

    fn client_info(rights: Rights, last_used: Option<DateTime<Utc>>) -> ClientInfo {
        ClientInfo {
            id: "a1b2c3d4".into(),
            name: "Claude Code".into(),
            kind: "claude-code".into(),
            created: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
            last_used,
            rights,
            agent: None,
        }
    }

    #[test]
    fn clients_are_listed_with_their_rights() {
        assert_eq!(render(&Response::Clients { clients: vec![] }), tr("No agents or scripts are connected."));
        let mut custom = RightsTemplate::ReadOnly.rights();
        custom.calendars = torro_ipc::api::Selection::Only(vec![]);
        let response = Response::Clients {
            clients: vec![client_info(RightsTemplate::ReadOnly.rights(), None), client_info(custom, None)],
        };
        let expected = format!(
            "a1b2c3d4  Claude Code  claude-code  {}  {}\na1b2c3d4  Claude Code  claude-code  {}  {}",
            tr("Read only"),
            tr("never used"),
            tr("Custom rights"),
            tr("never used")
        );
        assert_eq!(render(&response), expected);
    }

    #[test]
    fn a_new_key_is_printed_once_with_a_warning() {
        let response = Response::ClientKey {
            client: client_info(RightsTemplate::ReadAndWrite.rights(), None),
            key: torro_ipc::IssuedKey::new(torro_ipc::Key::new("torro_a1b2c3d4_00")),
        };
        let expected = format!(
            "{}\ntorro_a1b2c3d4_00",
            trf("Key for {name} ({id}). It is shown only this once:", &[("name", &"Claude Code"), ("id", &"a1b2c3d4")])
        );
        assert_eq!(render(&response), expected);
    }

    #[test]
    fn templates_are_listed_one_per_line() {
        let response = Response::Templates {
            templates: vec![summary("dev", "Development", 3), summary("review-setup", "Review", 1)],
        };
        assert_eq!(render(&response), "dev           Development  (3 windows)\nreview-setup  Review  (1 window)");
    }

    #[test]
    fn empty_template_list_says_so() {
        assert_eq!(render(&Response::Templates { templates: vec![] }), "no templates");
    }

    #[test]
    fn status_and_loaded_render() {
        let status = Response::Status(Status {
            version: "0.1.0".into(),
            backend: "hyprland".into(),
            workspace: None,
            templates: 2,
            busy: false,
        });
        assert!(render(&status).contains("workspace: -"));
        let loaded = Response::Loaded { restored: 2, failed: vec![("kitty".into(), "gone".into())] };
        assert_eq!(render(&loaded), "restored 2 windows\nfailed kitty: gone");
    }

    #[test]
    fn approvals_are_listed_allowed_and_denied() {
        assert_eq!(request(&["ctl", "approvals"]), Request::ListApprovals);
        assert_eq!(request(&["ctl", "allow", "9c1f"]), Request::DecideApproval { id: "9c1f".into(), allow: true });
        assert_eq!(request(&["ctl", "deny", "9c1f"]), Request::DecideApproval { id: "9c1f".into(), allow: false });
        assert!(parse_strs(&["ctl", "approvals", "list"]).is_err());
        assert!(parse_strs(&["ctl", "allow"]).is_err());
        assert!(parse_strs(&["ctl", "deny", "a", "b"]).is_err());
        assert!(parse_strs(&["ctl", "allow", "a", "--mode", "add"]).is_err());
        assert!(parse_strs(&["ctl", "approvals", "--json"]).expect("json").json);
    }

    fn approval(state: ApprovalState) -> ApprovalInfo {
        let at = DateTime::<Utc>::from_timestamp(0, 0).unwrap();
        ApprovalInfo {
            id: "9c1f0a7e3b2d4c58".into(),
            client: "a1b2c3d4".into(),
            client_name: "Claude Code".into(),
            command: "workspaces.delete_template".into(),
            summary: "Delete template \"Dev\"".into(),
            details: Vec::new(),
            created: at,
            expires: at,
            decided: None,
            state,
        }
    }

    #[test]
    fn approvals_render_one_per_line_with_their_state() {
        assert_eq!(render(&Response::Approvals { approvals: vec![] }), tr("No commands wait for approval."));
        assert_eq!(
            render(&Response::Approval { approval: approval(ApprovalState::Denied) }),
            format!("9c1f0a7e3b2d4c58  {}  Claude Code: Delete template \"Dev\"", tr("denied"))
        );
        let failed = Response::Error { code: "not-found".into(), message: "no template\u{1b}[2J \"dev\"".into() };
        let line =
            render(&Response::Approval { approval: approval(ApprovalState::Done { response: Box::new(failed) }) });
        assert!(line.contains(&trf("ran and failed: {message}", &[("message", &"no template [2J \"dev\"")])), "{line}");
        let pending = render(&Response::Approvals { approvals: vec![approval(ApprovalState::Pending)] });
        let until = DateTime::<Utc>::from_timestamp(0, 0).unwrap().with_timezone(&chrono::Local).format("%H:%M");
        assert!(pending.contains(&trf("waiting until {time}", &[("time", &until)])), "{pending}");
    }

    /// `ctl approvals`, `ctl allow` and `ctl deny` against a core in a temporary directory: the
    /// app key is read next to its socket, as `run` reads it next to the user's.
    #[cfg(unix)]
    #[tokio::test]
    async fn ctl_decides_approvals_with_the_app_key() {
        use std::sync::Arc;

        use torro_setups::storage::Store;

        let dir = tempfile::tempdir().expect("temp dir");
        let clients = crate::clients::tests::open_in(dir.path());
        let (_, agent) = clients.add("Agent", "script", RightsTemplate::ReadAndWrite.rights()).await.expect("added");
        let settings = crate::settings_service::SettingsStore::at(dir.path().join("config").join("settings.json"));
        let store = Store::at(dir.path().to_path_buf());
        for id in ["dev", "review"] {
            let now = Utc::now();
            store
                .save(&torro_setups::Template {
                    schema_version: torro_setups::SCHEMA_VERSION,
                    id: id.into(),
                    name: format!("Template {id}"),
                    wm: "hyprland".into(),
                    created: now,
                    updated: now,
                    source: torro_setups::Source { workspace: "1".into(), monitor: None, monitor_rect: None },
                    tree: None,
                    windows: vec![],
                })
                .expect("saved");
        }
        let core = Arc::new(
            crate::core::Core::new(
                torro_wm::Backend::Unsupported { name: "test".into() },
                store,
                Arc::new(crate::pim::PimService::with_settings(settings)),
                clients,
            )
            .with_approvals(crate::approvals::tests::quiet_in(dir.path())),
        );
        // Next to `state/app.key`, where `torro_ipc::client::call` looks for the app key.
        let socket = dir.path().join("state").join("core.sock");
        let path = socket.clone();
        tokio::spawn(async move {
            torro_ipc::server::serve_with_key(&path, move |key, request| {
                let core = Arc::clone(&core);
                async move { core.handle_from(key.as_ref(), request).await.unwrap_or_else(|r| r.response()) }
            })
            .await
        });
        let ask = |args: &[&str]| {
            let (socket, args) = (socket.clone(), args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
            async move {
                let command = parse(&args).expect("parses");
                for _ in 0..100 {
                    if let Ok(response) = torro_ipc::client::call(&socket, &command.request).await {
                        return response;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                panic!("the test core does not answer");
            }
        };
        assert_eq!(render(&ask(&["ctl", "approvals"]).await), tr("No commands wait for approval."));

        // The agent wants two templates deleted; both wait for a person.
        let mut ids = Vec::new();
        for id in ["dev", "review"] {
            let delete = Request::DeleteTemplate { id: id.into() };
            match torro_ipc::client::call_with_key(&socket, Some(&agent), &delete).await.expect("answered") {
                Response::Pending { approval } => ids.push(approval.id),
                other => panic!("expected pending, got {other:?}"),
            }
        }
        let listed = render(&ask(&["ctl", "approvals"]).await);
        assert_eq!(listed.lines().count(), 2, "{listed}");
        assert!(listed.contains("Template dev") && listed.contains(&ids[0]), "{listed}");

        let allow = ["ctl", "allow", ids[0].as_str()];
        let allowed = ask(&allow).await;
        assert!(
            matches!(&allowed, Response::Approval { approval } if matches!(approval.state, ApprovalState::Done { .. })),
            "{allowed:?}"
        );
        assert!(render(&allowed).contains(tr("done")));
        let denied = ask(&["ctl", "deny", ids[1].as_str()]).await;
        assert!(render(&denied).contains(tr("denied")), "{denied:?}");
        assert!(Store::at(dir.path().to_path_buf()).load("dev").is_err(), "allowed, so deleted");
        assert!(Store::at(dir.path().to_path_buf()).load("review").is_ok(), "denied, so kept");
        // Decided once: a second decision is refused.
        assert!(matches!(ask(&allow).await, Response::Error { ref code, .. } if code == "conflict"));
    }

    fn notify(args: &[&str]) -> Result<Command, UsageError> {
        parse_notify(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    fn notice(title: &str) -> Request {
        Request::PostItem {
            kind: Kind::Notice,
            title: title.into(),
            body: None,
            link: None,
            lane: None,
            thread_key: None,
            agent: None,
        }
    }

    #[test]
    fn notify_posts_a_notice() {
        assert_eq!(
            notify(&["--title", "Backup finished"]),
            Ok(Command { request: notice("Backup finished"), json: false })
        );
        let full = notify(&[
            "--title=Build failed",
            "--body",
            "3 tests failing",
            "--url",
            "https://ci.example/1",
            "--for-me",
            "--thread=build:main",
            "--json",
        ]);
        let wanted = Request::PostItem {
            kind: Kind::Notice,
            title: "Build failed".into(),
            body: Some("3 tests failing".into()),
            link: Some(Link::Url { url: "https://ci.example/1".into() }),
            lane: Some(Lane::ForMe),
            thread_key: Some("build:main".into()),
            agent: None,
        };
        assert_eq!(full, Ok(Command { request: wanted, json: true }));
        // Through the entry point's dispatch too: `from_args` hands `notify …` over.
        assert!(parse_strs(&["notify", "--title", "x"]).is_err(), "notify is not a ctl command");
    }

    #[test]
    fn notify_refuses_what_it_cannot_post() {
        for args in [
            &[][..],
            &["--body", "no title"],
            &["--title", "  "],
            &["--title"],
            &["--title", "x", "--thread"],
            &["--title", "x", "extra"],
            &["--title", "x", "--kind", "approval"],
            &["--title", "x", "--for-me=yes"],
        ] {
            assert!(notify(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn the_token_is_trimmed_and_blank_means_the_app_key() {
        assert!(token_from(None).is_none());
        assert!(token_from(Some("  \n".into())).is_none());
        assert_eq!(token_from(Some(" torro_c1_x\n".into())).as_ref().map(Key::expose), Some("torro_c1_x"));
    }

    #[test]
    fn posted_answers_render() {
        assert_eq!(
            render(&Response::Posted { id: Some("i1".into()) }),
            trf("Posted to the inbox ({id}).", &[("id", &"i1")])
        );
        assert_eq!(render(&Response::Posted { id: None }), tr("Not posted: this source is muted in the inbox."));
    }

    #[test]
    fn notify_without_a_core_fails_with_exit_code_1() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("state").join("core.sock");
        let command = notify(&["--title", "Backup finished"]).expect("parses");
        assert_eq!(run_notify(&command, &socket, None), ExitCode::FAILURE);
        assert_eq!(run_notify(&command, &socket, Some(&Key::new("torro_c1_x"))), ExitCode::FAILURE);
        assert_eq!(ExitCode::FAILURE, ExitCode::from(1));
    }

    /// `notify` against a core in a temporary directory: with the app key next to its socket the
    /// entry is filed under `script:cli`, with a script's key under that script.
    #[cfg(unix)]
    #[tokio::test]
    async fn notify_posts_under_the_cli_or_the_scripts_source() {
        use std::sync::Arc;

        use torro_inbox::SourceRef;
        use torro_setups::storage::Store;

        let dir = tempfile::tempdir().expect("temp dir");
        let clients = crate::clients::tests::open_in(dir.path());
        let (_, backup) = clients.add("Backup", "script", RightsTemplate::ReadAndWrite.rights()).await.expect("added");
        let settings = crate::settings_service::SettingsStore::at(dir.path().join("config").join("settings.json"));
        let core = Arc::new(
            crate::core::Core::new(
                torro_wm::Backend::Unsupported { name: "test".into() },
                Store::at(dir.path().to_path_buf()),
                Arc::new(crate::pim::PimService::with_settings(settings)),
                clients,
            )
            .with_approvals(crate::approvals::tests::quiet_in(dir.path()))
            .with_inbox(crate::inbox::tests::inbox_in(dir.path())),
        );
        let socket = dir.path().join("state").join("core.sock");
        let path = socket.clone();
        tokio::spawn(async move {
            torro_ipc::server::serve_with_key(&path, move |key, request| {
                let core = Arc::clone(&core);
                async move { core.handle_from(key.as_ref(), request).await.unwrap_or_else(|r| r.response()) }
            })
            .await
        });
        for _ in 0..100 {
            if torro_ipc::client::call(&socket, &Request::Ping).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let post = |args: &'static [&'static str], key: Option<Key>| {
            let socket = socket.clone();
            tokio::task::spawn_blocking(move || {
                let command = notify(args).expect("parses");
                run_notify(&command, &socket, key.as_ref())
            })
        };
        assert_eq!(post(&["--title", "From the shell", "--for-me"], None).await.expect("ran"), ExitCode::SUCCESS);
        assert_eq!(post(&["--title", "Backup finished"], Some(backup)).await.expect("ran"), ExitCode::SUCCESS);
        let unknown = post(&["--title", "x"], Some(Key::new("torro_00000000_nope"))).await.expect("ran");
        assert_eq!(unknown, ExitCode::FAILURE, "an unknown key is refused");

        let all = Request::ListItems { lane: None, source: None, state: None, since: None, limit: None };
        let Ok(Response::Items { items }) = torro_ipc::client::call(&socket, &all).await else {
            panic!("the app key lists the inbox");
        };
        let mut filed: Vec<(String, String, Lane)> =
            items.iter().map(|item| (item.title.clone(), item.source.key(), item.lane)).collect();
        filed.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            filed,
            [
                ("Backup finished".into(), SourceRef::script("Backup").key(), Lane::Info),
                ("From the shell".into(), SourceRef::script("cli").key(), Lane::ForMe),
            ]
        );
        assert!(items.iter().all(|item| item.kind == Kind::Notice && item.untrusted));
    }

    #[test]
    fn error_codes_map_to_exit_status() {
        assert_eq!(error_exit_status("workspace-not-empty"), 3);
        assert_eq!(error_exit_status("not-found"), 1);
        assert_eq!(error_exit_status("busy"), 1);
        assert_eq!(error_exit_status("error"), 1);
    }
}
