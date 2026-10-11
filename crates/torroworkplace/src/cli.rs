//! The command-line clients of the core: `ctl setups …` and `status`.
//!
//! Each command becomes one [`Request`], goes to the daemon over the local socket and prints
//! the [`Response`]: short lines for people, or the response as JSON with `--json` for bars
//! and scripts. [`parse`] only reads the arguments and is tested without a socket; [`run`]
//! talks to the core. Exit codes: 0 done, 1 failed or core not running, 3 a load found the
//! workspace not empty, 64 bad arguments.

use std::fmt;
use std::process::ExitCode;

use torro_ipc::{Request, Response, socket_path};
use torro_setups::{LoadMode, TemplateSummary};

const USAGE: &str = "usage:
  torroworkplace status [--json]
  torroworkplace ctl setups list [--json]
  torroworkplace ctl setups save [<name>] [--json]
  torroworkplace ctl setups update <id> [--json]
  torroworkplace ctl setups delete <id> [--json]
  torroworkplace ctl setups load <id> [--mode empty|add|replace] [--json]";

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
/// `status` or `ctl`.
pub fn from_args(args: &[String]) -> ExitCode {
    match parse(args) {
        Ok(command) => run(command),
        Err(err) => {
            eprintln!("{}: {err}\n\n{USAGE}", crate::APP.binary);
            ExitCode::from(64)
        }
    }
}

/// Turns the arguments (starting with `status` or `ctl`) into a [`Command`].
/// `--json` works everywhere; `--mode` only for `ctl setups load`.
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let Some((head, rest)) = args.split_first() else {
        return Err(UsageError::new("missing command"));
    };

    let mut json = false;
    let mut mode: Option<&str> = None;
    let mut words: Vec<&str> = Vec::new();
    let mut rest_iter = rest.iter();
    while let Some(arg) = rest_iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--mode" => {
                let value =
                    rest_iter.next().ok_or_else(|| UsageError::new("--mode needs a value: empty, add or replace"))?;
                mode = Some(value.as_str());
            }
            flag if flag.starts_with("--mode=") => mode = flag.strip_prefix("--mode="),
            flag if flag.starts_with('-') => return Err(UsageError::new(format!("unknown option {flag}"))),
            word => words.push(word),
        }
    }

    let request = match head.as_str() {
        "status" => {
            if !words.is_empty() {
                return Err(UsageError::new("status takes no arguments"));
            }
            Request::Status
        }
        "ctl" => ctl_request(&words, mode)?,
        other => return Err(UsageError::new(format!("unknown command {other:?}"))),
    };

    if mode.is_some() && !matches!(request, Request::LoadTemplate { .. }) {
        return Err(UsageError::new("--mode only applies to ctl setups load"));
    }
    Ok(Command { request, json })
}

fn ctl_request(words: &[&str], mode: Option<&str>) -> Result<Request, UsageError> {
    match words {
        ["setups", verb, args @ ..] => setups_request(verb, args, mode),
        ["setups"] => Err(UsageError::new("ctl setups needs a subcommand: list, save, update, delete or load")),
        [] => Err(UsageError::new("ctl needs a group, e.g. `ctl setups list`")),
        [other, ..] => Err(UsageError::new(format!("unknown ctl group {other:?}; try setups"))),
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
        Response::Pong { version } => format!("torroworkplace {version}"),
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
        Response::Ok => "ok".to_string(),
        Response::Error { message, .. } => message.clone(),
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
    fn error_codes_map_to_exit_status() {
        assert_eq!(error_exit_status("workspace-not-empty"), 3);
        assert_eq!(error_exit_status("not-found"), 1);
        assert_eq!(error_exit_status("busy"), 1);
        assert_eq!(error_exit_status("error"), 1);
    }
}
