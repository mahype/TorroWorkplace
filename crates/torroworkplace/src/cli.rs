//! The command-line clients of the core: `ctl setups …`, `ctl clients …`, `status` and `notify`.
//!
//! Each command becomes one [`Request`], goes to the daemon over the local socket and prints
//! the [`Response`]: short lines for people, or the response as JSON with `--json` for bars
//! and scripts. [`parse`] only reads the arguments and is tested without a socket; [`run`]
//! talks to the core. Exit codes: 0 done, 1 failed or core not running, 3 a load found the
//! workspace not empty, 64 bad arguments.
//!
//! `ctl mail …` (issue #267, `docs/mail-backup.md`) backs up, verifies and restores one mail
//! account. Its exit codes come from [`mail_exit_code`]: 0 done (also a successful `cancel` and a
//! restore test without findings), 1 the core refused or is not running, 2 the result needs a
//! person's review (an incomplete backup or archive, uncertain or unconfirmed mail), 3 a backup
//! ended cancelled, 64 bad arguments.
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
use std::time::Duration;

use chrono::NaiveDate;
use torro_core::i18n::{tr, trf};
use torro_inbox::{AgentKind, Kind, Lane, Link};
use torro_ipc::api::{Rights, RightsTemplate};
use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope, BackupVerification, RestorePlan, RestoreResult};
use torro_ipc::{ApprovalInfo, ApprovalState, ClientInfo, IpcError, Key, Request, Response, socket_path};
use torro_setups::{LoadMode, TemplateSummary};

/// The help, with `{kinds}` for the kinds of client `ctl clients add --kind` takes ([`usage`]).
const USAGE: &str = "usage:
  torroworkplace status [--json]
  torroworkplace ctl setups list [--json]
  torroworkplace ctl setups save [<name>] [--json]
  torroworkplace ctl setups update <id> [--json]
  torroworkplace ctl setups delete <id> [--json]
  torroworkplace ctl setups load <id> [--mode empty|add|replace] [--json]
  torroworkplace ctl setups key <id> <0-9|none> [--json]
  torroworkplace ctl clients list [--json]
  torroworkplace ctl clients add <name> [--kind {kinds}]
                                        [--agent claude-code|codex|gemini]
                                        [--template read-only|read-and-write|full-access] [--json]
  torroworkplace ctl clients remove <id> [--json]
  torroworkplace ctl clients new-key <id> [--json]
  torroworkplace ctl approvals [--json]
  torroworkplace ctl allow <id> [--json]
  torroworkplace ctl deny <id> [--json]
  torroworkplace ctl quit [--json]
  torroworkplace ctl mail backup create --account <id> [--source server|local] [--since YYYY-MM-DD] [--file <name.zip>] [--wait] [--json]
  torroworkplace ctl mail backup status|cancel --account <id> --job <id> [--json]
  torroworkplace ctl mail backup verify --account <id> --file <archive.zip> [--json]
  torroworkplace ctl mail restore --account <id> --file <archive.zip> [--dry-run|--apply]
                                 [--allow-incomplete] [--retry-unconfirmed] [--json]
                                 (mail exit codes: 0 done, 1 failed, 2 needs review, 3 backup cancelled;
                                  --file: a plain name is in the managed folder, ./x.zip or sub/x.zip is
                                  relative to here; --wait has no time limit and Ctrl-C does not cancel the job)
  torroworkplace notify --title <text> [--body <text>] [--url <url>] [--for-me] [--thread <key>] [--json]
  torroworkplace hook claude-code|codex|gemini [--key-file <file>]
                                                 (run by an agent's hooks; reads stdin, prints nothing)";

/// The help as printed: the kinds of client are those the core takes ([`torro_ipc::api::CLIENT_KINDS`],
/// #231; any other is an `error`), so the help never names one it refuses or leaves one out.
fn usage() -> String {
    USAGE.replace("{kinds}", &torro_ipc::api::CLIENT_KINDS.join("|"))
}

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
    if let Some(tail) = mail_args(args) {
        return match parse_mail(&tail) {
            Ok(command) => run_mail(command),
            Err(err) => {
                eprintln!("{}: {err}\n\n{}", crate::APP.binary, usage());
                ExitCode::from(64)
            }
        };
    }
    match parse(args) {
        Ok(command) => run(command),
        Err(err) => {
            eprintln!("{}: {err}\n\n{}", crate::APP.binary, usage());
            ExitCode::from(64)
        }
    }
}

/// The `ctl mail` tail of `args` with `mail` taken out, also when options stand before it
/// (`ctl --json mail …`); `None` for every other command line.
fn mail_args(args: &[String]) -> Option<Vec<String>> {
    let (head, rest) = args.split_first()?;
    if head != "ctl" {
        return None;
    }
    let at = rest.iter().position(|arg| !arg.starts_with('-'))?;
    (rest[at] == "mail").then(|| rest[..at].iter().chain(&rest[at + 1..]).cloned().collect())
}

/// A mail command can make a second request after the first one: waiting on a backup or
/// applying the exact plan returned by a read-only restore test.
#[derive(Debug, PartialEq, Eq)]
struct MailCommand {
    request: Request,
    account: String,
    json: bool,
    /// `--wait`: ask for the job's status every two seconds until it has ended. There is no time
    /// limit, and Ctrl-C ends only this command, never the job (`ctl mail backup cancel` does).
    wait: bool,
    apply: bool,
    allow_incomplete: bool,
    /// `--retry-unconfirmed` (only with `--apply`): upload again the mail an earlier restore could
    /// not confirm.
    retry_unconfirmed: bool,
}

/// Parse only the `ctl mail` tail. Every option is validated before contacting the daemon.
fn parse_mail(args: &[String]) -> Result<MailCommand, UsageError> {
    let mut words = Vec::new();
    let (mut account, mut source, mut since, mut file, mut job) = (None, None, None, None, None);
    let (mut json, mut wait, mut apply, mut dry_run, mut allow_incomplete) = (false, false, false, false, false);
    let mut retry_unconfirmed = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (flag, inline) = arg
            .split_once('=')
            .filter(|(flag, _)| flag.starts_with("--"))
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        match flag {
            "--json" | "--wait" | "--apply" | "--dry-run" | "--allow-incomplete" | "--retry-unconfirmed" => {
                if inline.is_some() {
                    return Err(UsageError::new(tr("This option takes no value.")));
                }
                match flag {
                    "--json" => json = true,
                    "--wait" => wait = true,
                    "--apply" => apply = true,
                    "--dry-run" => dry_run = true,
                    "--retry-unconfirmed" => retry_unconfirmed = true,
                    _ => allow_incomplete = true,
                }
            }
            "--account" | "--source" | "--since" | "--file" | "--job" => {
                let value = inline
                    .or_else(|| iter.next().map(String::as_str))
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| UsageError::new(tr("An option needs a value.")))?;
                let slot = match flag {
                    "--account" => &mut account,
                    "--source" => &mut source,
                    "--since" => &mut since,
                    "--file" => &mut file,
                    _ => &mut job,
                };
                if slot.replace(value).is_some() {
                    return Err(UsageError::new(tr("An option was given twice.")));
                }
            }
            option if option.starts_with('-') => {
                return Err(UsageError::new(trf("unknown option {option}", &[("option", &arg)])));
            }
            word => words.push(word),
        }
    }
    let account = account.ok_or_else(|| UsageError::new(tr("Mail commands need --account.")))?.to_string();
    let file = file.map(resolve_file).transpose()?;
    let must_file = || file.clone().ok_or_else(|| UsageError::new(tr("This command needs --file.")));
    // Only a real restore takes the two confirmations; anything else with them is a mistake.
    let restore_flags = apply || allow_incomplete || retry_unconfirmed;
    let must_job = || job.ok_or_else(|| UsageError::new(tr("This command needs --job.")));
    let command = match words.as_slice() {
        ["backup", "create"] => {
            if job.is_some() || restore_flags || dry_run {
                return Err(UsageError::new(tr("Options do not match this mail command.")));
            }
            let source = match source.unwrap_or("server") {
                "server" => BackupScope::Server,
                "local" => BackupScope::Local,
                _ => return Err(UsageError::new(tr("Source must be server or local."))),
            };
            let since = since
                .map(|day| {
                    NaiveDate::parse_from_str(day, "%Y-%m-%d")
                        .map_err(|_| UsageError::new(tr("Enter a valid date as YYYY-MM-DD.")))
                })
                .transpose()?;
            Request::MailBackupCreate { account: account.clone(), source, since, file_name: file.clone() }
        }
        ["backup", "status"] | ["backup", "cancel"] => {
            if source.is_some() || since.is_some() || file.is_some() || wait || restore_flags || dry_run {
                return Err(UsageError::new(tr("Options do not match this mail command.")));
            }
            let job = must_job()?.to_string();
            if words[1] == "status" {
                Request::MailBackupStatus { account: account.clone(), job }
            } else {
                Request::MailBackupCancel { account: account.clone(), job }
            }
        }
        ["backup", "verify"] => {
            if source.is_some() || since.is_some() || job.is_some() || wait || restore_flags || dry_run {
                return Err(UsageError::new(tr("Options do not match this mail command.")));
            }
            Request::MailBackupVerify { account: account.clone(), file: must_file()? }
        }
        ["restore"] => {
            if source.is_some()
                || since.is_some()
                || job.is_some()
                || wait
                || (apply && dry_run)
                || ((allow_incomplete || retry_unconfirmed) && !apply)
            {
                return Err(UsageError::new(tr("Options do not match this mail command.")));
            }
            Request::MailRestorePlan { account: account.clone(), file: must_file()? }
        }
        _ => return Err(UsageError::new(tr("Use ctl mail backup create|status|cancel|verify or ctl mail restore."))),
    };
    Ok(MailCommand { request: command, account, json, wait, apply, allow_incomplete, retry_unconfirmed })
}

/// `--file` as the core takes it. A plain name stays a name in the managed backup folder; a path
/// with a separator or a leading `.` (`./a.zip`, `sub/a.zip`) is relative to this shell's
/// directory, which the core does not know, so it is made absolute here.
fn resolve_file(file: &str) -> Result<String, UsageError> {
    let path = Path::new(file);
    let relative_path = file.starts_with('.') || file.contains('/') || file.contains(std::path::MAIN_SEPARATOR);
    if path.is_absolute() || !relative_path {
        return Ok(file.to_string());
    }
    std::path::absolute(path)
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .ok_or_else(|| UsageError::new(tr("This path cannot be used.")))
}

fn run_mail(command: MailCommand) -> ExitCode {
    let Some(socket) = socket_path() else {
        eprintln!("{}", tr("No core socket is available."));
        return ExitCode::FAILURE;
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(mail_session(&socket, &command)) {
        Ok(response) => {
            print_mail(&response, &command);
            ExitCode::from(mail_exit_code(&command, &response))
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// The requests of one mail command, up to its last answer: the command's own request, then with
/// `--wait` the job's status until it has ended, and with `--apply` the restore of exactly the plan
/// the test returned, if nothing in it needs a person first ([`plan_blockers`]).
async fn mail_session(socket: &Path, command: &MailCommand) -> Result<Response, IpcError> {
    let mut response = torro_ipc::client::call(socket, &command.request).await?;
    if command.wait {
        while let Response::MailBackupJob { job } = &response {
            if job.phase.is_final() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            let status = Request::MailBackupStatus { account: command.account.clone(), job: job.id.clone() };
            response = torro_ipc::client::call(socket, &status).await?;
        }
    }
    if command.apply
        && let Response::MailRestorePlanned { plan } = &response
        && plan_blockers(plan, command.allow_incomplete, command.retry_unconfirmed).is_empty()
    {
        let apply = Request::MailRestoreApply {
            account: command.account.clone(),
            plan: plan.id.clone(),
            allow_incomplete: command.allow_incomplete,
            retry_unconfirmed: command.retry_unconfirmed,
        };
        response = torro_ipc::client::call(socket, &apply).await?;
    }
    Ok(response)
}

/// Prints the last answer of a mail command: as JSON with `--json`, else as lines for people
/// ([`render`]); the core's error message and why a plan needs review go to stderr.
fn print_mail(response: &Response, command: &MailCommand) {
    match response {
        Response::Error { message, .. } => {
            if command.json {
                println!("{}", output(response, true));
            }
            eprintln!("{message}");
        }
        _ => println!("{}", output(response, command.json)),
    }
    if let Response::MailRestorePlanned { plan } = response {
        for reason in plan_blockers(plan, command.allow_incomplete, command.retry_unconfirmed) {
            eprintln!("{reason}");
        }
    }
}

/// A response as `ctl` prints it on stdout: pretty JSON with `--json`, else [`render`].
fn output(response: &Response, json: bool) -> String {
    match json {
        true => serde_json::to_string_pretty(response).unwrap_or_else(|err| err.to_string()),
        false => render(response),
    }
}

/// Why `plan` must not be applied as it stands, one sentence per reason; empty when it may. An
/// incomplete archive needs `--allow-incomplete`, unconfirmed uploads `--retry-unconfirmed`, and
/// uncertain mail always a person's look at the target account first.
fn plan_blockers(plan: &RestorePlan, allow_incomplete: bool, retry_unconfirmed: bool) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if plan.uncertain > 0 {
        reasons.push(tr("Some mail is uncertain; check the target account before restoring."));
    }
    if !plan.complete_archive && !allow_incomplete {
        reasons.push(tr("The archive is incomplete; add --allow-incomplete to restore it anyway."));
    }
    if plan.unconfirmed > 0 && !retry_unconfirmed {
        reasons.push(tr(
            "An earlier restore left uploads that could not be confirmed: they may not have arrived, or arrived changed. Check the target folders, then add --retry-unconfirmed to upload them again.",
        ));
    }
    reasons
}

/// The exit code of a mail command from its last answer, as [`USAGE`] and `docs/mail-backup.md`
/// list them:
///
/// - 0: done – a job that runs or is complete, a successful `cancel` (also while the job is still
///   `cancelling`), a complete archive, a restore test without findings, a restore without
///   uncertain mail.
/// - 1: the core refused or failed (`Response::Error`, or an answer that does not fit).
/// - 2: the result needs a person's review – an incomplete backup or archive, a restore test with
///   uncertain or unconfirmed mail or an incomplete archive that the flags did not accept, a restore
///   that left mail uncertain.
/// - 3: the backup ended cancelled (`create --wait`, `status`).
///
/// Connection failures (1) and bad arguments (64) never reach this function.
fn mail_exit_code(command: &MailCommand, response: &Response) -> u8 {
    let cancel = matches!(command.request, Request::MailBackupCancel { .. });
    match response {
        Response::MailBackupJob { .. } if cancel => 0,
        Response::MailBackupJob { job } => match job.phase {
            BackupPhase::Cancelled => 3,
            BackupPhase::Incomplete => 2,
            _ => 0,
        },
        Response::MailBackupVerified { verification } => u8::from(!verification.complete) * 2,
        Response::MailRestorePlanned { plan } => {
            let blocked = !plan_blockers(plan, command.allow_incomplete, command.retry_unconfirmed).is_empty();
            u8::from(blocked) * 2
        }
        Response::MailRestored { restore } => u8::from(restore.uncertain > 0) * 2,
        _ => 1,
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
        ["setups"] => Err(UsageError::new("ctl setups needs a subcommand: list, save, update, delete, load or key")),
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
            rights: None,
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
        // Issue #243: only the key here; startup and screens are for the control centre, the API
        // and MCP.
        ("key", [id, key]) => Ok(Request::SetTemplateShortcut { id: id.to_string(), key: parse_key(key)? }),
        ("key", [_]) => Err(UsageError::new("ctl setups key needs a key: 0-9 or none")),
        ("update" | "delete" | "load" | "key", []) => {
            Err(UsageError::new(format!("ctl setups {verb} needs a template id")))
        }
        ("list" | "save" | "update" | "delete" | "load" | "key", _) => {
            Err(UsageError::new(format!("too many arguments for ctl setups {verb}")))
        }
        _ => Err(UsageError::new(format!("unknown subcommand {verb:?}; try list, save, update, delete, load or key"))),
    }
}

/// A digit key `0` … `9`, or `none` to take the key away.
fn parse_key(key: &str) -> Result<Option<u8>, UsageError> {
    match key.as_bytes() {
        [digit @ b'0'..=b'9'] => Ok(Some(digit - b'0')),
        _ if key == "none" => Ok(None),
        _ => Err(UsageError::new(format!("unknown key {key:?}; use a digit 0-9 or none"))),
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
            eprintln!("{}: {err}\n\n{}", crate::APP.binary, usage());
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
                    "Torro Workplace isn't running, so nothing reached the Notification Hub. Start it with `torroworkplace daemon`."
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
        Response::Posted { id: Some(id) } => trf("Posted to the Notification Hub ({id}).", &[("id", id)]),
        Response::Posted { id: None } => tr("Not posted: this source is muted in the Notification Hub.").to_string(),
        Response::Ok => "ok".to_string(),
        Response::Error { message, .. } => message.clone(),
        Response::MailBackupJob { job } => job_text(job),
        Response::MailBackupVerified { verification } => verification_text(verification),
        Response::MailRestorePlanned { plan } => plan_text(plan),
        Response::MailRestored { restore } => restore_text(restore),
        // Calendar and task answers are for the apps; show them as JSON.
        other => serde_json::to_string_pretty(other).unwrap_or_else(|err| err.to_string()),
    }
}

/// A backup job for people: its id and phase, the counts so far, the archive once there is one,
/// and each warning on its own line.
fn job_text(job: &BackupJob) -> String {
    let mut lines = vec![
        trf("Backup {id}: {phase}", &[("id", &job.id), ("phase", &phase_word(job.phase))]),
        counts_line(job.messages, job.folders, job.bytes),
    ];
    lines.extend(job.archive.iter().map(|archive| trf("Archive: {name}", &[("name", archive)])));
    lines.extend(warning_lines(&job.warnings));
    lines.join("\n")
}

/// An offline verification for people: complete or not, the counts and the warnings.
fn verification_text(verification: &BackupVerification) -> String {
    let verdict = if verification.complete { tr("Archive complete") } else { tr("Archive incomplete") };
    let mut lines =
        vec![verdict.to_string(), counts_line(verification.messages, verification.folders, verification.bytes)];
    lines.extend(warning_lines(&verification.warnings));
    lines.join("\n")
}

/// A restore test for people: what it found for which archive, what still needs a confirmation,
/// one line per folder, the warnings, and the plan's id.
fn plan_text(plan: &RestorePlan) -> String {
    let mut lines = vec![
        trf("Restore test of {name}: nothing was changed", &[("name", &plan.archive)]),
        trf(
            "To add: {add} · Already present: {present} · Uncertain: {uncertain} · New folders: {folders} · Size: {size}",
            &[
                ("add", &plan.messages_to_add),
                ("present", &plan.already_present),
                ("uncertain", &plan.uncertain),
                ("folders", &plan.folders_to_create),
                ("size", &size_text(plan.bytes_to_add)),
            ],
        ),
    ];
    if !plan.complete_archive {
        lines.push(tr("The archive is incomplete.").to_string());
    }
    if plan.unconfirmed > 0 {
        lines.push(trf("Unconfirmed uploads of an earlier restore: {n}", &[("n", &plan.unconfirmed)]));
    }
    for folder in &plan.folders {
        let into = match folder.create {
            true => trf("Into {target}, new folder", &[("target", &folder.target_path)]),
            false => trf("Into {target}, existing folder", &[("target", &folder.target_path)]),
        };
        let counts = trf(
            "To add: {add} · Already present: {present} · Uncertain: {uncertain}",
            &[("add", &folder.messages_to_add), ("present", &folder.already_present), ("uncertain", &folder.uncertain)],
        );
        lines.push(format!("  {}: {into}; {counts}", folder.archive_path));
        if folder.aggregate {
            lines.push(format!(
                "    {}",
                trf(
                    "Collection folder: only mail that is in no other folder is added here. Copies of mail in other folders: {covered}, not uploaded.",
                    &[("covered", &folder.covered)],
                )
            ));
        }
    }
    lines.extend(warning_lines(&plan.warnings));
    lines.push(trf("Plan: {id}", &[("id", &plan.id)]));
    lines.join("\n")
}

/// A finished restore for people: its counts and warnings.
fn restore_text(restore: &RestoreResult) -> String {
    let mut lines = vec![
        tr("Restore finished").to_string(),
        trf(
            "Added: {added} · Skipped: {skipped} · Uncertain: {uncertain}",
            &[("added", &restore.added), ("skipped", &restore.skipped), ("uncertain", &restore.uncertain)],
        ),
    ];
    lines.extend(warning_lines(&restore.warnings));
    lines.join("\n")
}

/// "Messages: 2 · Folders: 1 · Size: 0.0 MiB", the same sentence as on the mail account's page.
fn counts_line(messages: u64, folders: u64, bytes: u64) -> String {
    trf(
        "Messages: {messages} · Folders: {folders} · Size: {size}",
        &[("messages", &messages), ("folders", &folders), ("size", &size_text(bytes))],
    )
}

fn warning_lines(warnings: &[String]) -> impl Iterator<Item = String> + '_ {
    warnings.iter().map(|warning| trf("Warning: {text}", &[("text", warning)]))
}

/// A size in MiB with one decimal, as the mail account's page shows it.
fn size_text(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// The phase of a backup job, in the words of the mail account's page.
fn phase_word(phase: BackupPhase) -> &'static str {
    match phase {
        BackupPhase::Queued => tr("Queued"),
        BackupPhase::Counting => tr("Counting"),
        BackupPhase::Reading => tr("Reading mail"),
        BackupPhase::Verifying => tr("Verifying"),
        BackupPhase::Complete => tr("Complete"),
        BackupPhase::Incomplete => tr("Incomplete"),
        BackupPhase::Cancelling => tr("Cancelling…"),
        BackupPhase::Cancelled => tr("Cancelled"),
    }
}

/// `id  name  (n windows)`, with the id padded to `id_width` so the names line up.
fn template_line(template: &TemplateSummary, id_width: usize) -> String {
    let key = template.shortcut.map(|key| format!(", key {key}")).unwrap_or_default();
    format!(
        "{:<id_width$}  {}  ({} {}{key})",
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

    fn mail(args: &[&str]) -> Result<MailCommand, UsageError> {
        parse_mail(&args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn mail_backup_cli_parses_automation_options() {
        let command = mail(&[
            "backup",
            "create",
            "--account",
            "mail:a@example.org",
            "--source=local",
            "--since",
            "2026-10-20",
            "--file",
            "a.zip",
            "--wait",
            "--json",
        ])
        .expect("a valid command line");
        assert_eq!(
            command.request,
            Request::MailBackupCreate {
                account: "mail:a@example.org".into(),
                source: BackupScope::Local,
                since: NaiveDate::from_ymd_opt(2026, 10, 20),
                file_name: Some("a.zip".into()),
            }
        );
        assert!(command.wait && command.json);
        assert_eq!(
            mail(&["backup", "status", "--account", "a", "--job", "j"]).expect("a valid command line").request,
            Request::MailBackupStatus { account: "a".into(), job: "j".into() }
        );
        let selected = std::env::temp_dir().join("anna.zip").to_string_lossy().into_owned();
        assert_eq!(
            mail(&["backup", "create", "--account", "a", "--file", &selected]).expect("a valid command line").request,
            Request::MailBackupCreate {
                account: "a".into(),
                source: BackupScope::Server,
                since: None,
                file_name: Some(selected)
            }
        );
    }

    #[test]
    fn mail_restore_cli_defaults_to_test_run_and_requires_explicit_apply() {
        let args = ["restore", "--account", "a", "--file", "a.zip"];
        let plan = mail(&args).expect("a valid command line");
        assert_eq!(plan.request, Request::MailRestorePlan { account: "a".into(), file: "a.zip".into() });
        assert!(!plan.apply);
        assert!(
            mail(&["restore", "--account", "a", "--file", "a.zip", "--apply"]).expect("a valid command line").apply
        );
        assert!(mail(&["restore", "--account", "a", "--file", "a.zip", "--allow-incomplete"]).is_err());
        assert!(mail(&["restore", "--account", "a", "--file", "a.zip", "--apply", "--dry-run"]).is_err());
    }

    #[test]
    fn mail_cli_rejects_missing_account_and_bad_day() {
        assert!(mail(&["backup", "create"]).is_err());
        assert!(mail(&["backup", "create", "--account", "a", "--since", "2026-02-30"]).is_err());
        assert!(mail(&["backup", "create", "--account", "a", "--wait", "--job", "j"]).is_err());
    }

    fn backup_job(phase: BackupPhase) -> BackupJob {
        BackupJob {
            id: "job-1".into(),
            account: "a".into(),
            source: BackupScope::Server,
            phase,
            folders: 2,
            messages: 3,
            bytes: 4,
            archive: Some("a.zip".into()),
            warnings: vec!["Folder Spam was not readable".into()],
        }
    }

    fn restore_plan(complete_archive: bool, uncertain: u64, unconfirmed: u64) -> RestorePlan {
        RestorePlan {
            id: "plan-1".into(),
            account: "a".into(),
            archive: "a.zip".into(),
            complete_archive,
            folders_to_create: 1,
            messages_to_add: 5,
            already_present: 6,
            uncertain,
            unconfirmed,
            bytes_to_add: 7,
            folders: vec![torro_ipc::mail::RestoreFolderPlan {
                archive_path: "[Gmail]/All Mail".into(),
                target_path: "[Gmail]/All Mail".into(),
                create: false,
                messages_to_add: 0,
                already_present: 0,
                uncertain: 0,
                bytes_to_add: 0,
                aggregate: true,
                covered: 9,
            }],
            warnings: Vec::new(),
        }
    }

    fn verified(complete: bool) -> Response {
        Response::MailBackupVerified {
            verification: BackupVerification {
                source: BackupScope::Server,
                since: None,
                started_at: DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch"),
                finished_at: None,
                complete,
                folders: 2,
                messages: 3,
                bytes: 4,
                warnings: Vec::new(),
            },
        }
    }

    fn restored(uncertain: u64) -> Response {
        Response::MailRestored { restore: RestoreResult { added: 1, skipped: 2, uncertain, warnings: Vec::new() } }
    }

    /// The exit codes of `ctl mail`: 0 done, 1 failed, 2 needs review, 3 cancelled.
    #[test]
    fn mail_results_map_to_exit_codes() {
        let create = mail(&["backup", "create", "--account", "a", "--wait"]).expect("a valid command line");
        let status = mail(&["backup", "status", "--account", "a", "--job", "j"]).expect("a valid command line");
        let cancel = mail(&["backup", "cancel", "--account", "a", "--job", "j"]).expect("a valid command line");
        let job = |phase| Response::MailBackupJob { job: backup_job(phase) };
        assert_eq!(mail_exit_code(&create, &job(BackupPhase::Complete)), 0);
        assert_eq!(mail_exit_code(&create, &job(BackupPhase::Incomplete)), 2);
        assert_eq!(mail_exit_code(&create, &job(BackupPhase::Cancelled)), 3);
        assert_eq!(mail_exit_code(&status, &job(BackupPhase::Reading)), 0);
        assert_eq!(mail_exit_code(&status, &job(BackupPhase::Cancelled)), 3);
        assert_eq!(mail_exit_code(&cancel, &job(BackupPhase::Cancelling)), 0, "a successful cancel is done");
        assert_eq!(mail_exit_code(&cancel, &job(BackupPhase::Cancelled)), 0);
        let refused = Response::Error { code: "not-found".into(), message: "gone".into() };
        assert_eq!(mail_exit_code(&cancel, &refused), 1);
        assert_eq!(mail_exit_code(&status, &Response::Ok), 1, "an answer that does not fit");

        let verify = mail(&["backup", "verify", "--account", "a", "--file", "a.zip"]).expect("a valid command line");
        assert_eq!(mail_exit_code(&verify, &verified(true)), 0);
        assert_eq!(mail_exit_code(&verify, &verified(false)), 2);

        let test = mail(&["restore", "--account", "a", "--file", "a.zip"]).expect("a valid command line");
        let planned = |complete, uncertain, unconfirmed| Response::MailRestorePlanned {
            plan: restore_plan(complete, uncertain, unconfirmed),
        };
        assert_eq!(mail_exit_code(&test, &planned(true, 0, 0)), 0, "a restore test without findings");
        assert_eq!(mail_exit_code(&test, &planned(false, 0, 0)), 2);
        assert_eq!(mail_exit_code(&test, &planned(true, 1, 0)), 2);
        assert_eq!(mail_exit_code(&test, &planned(true, 0, 1)), 2);
        let apply = mail(&["restore", "--account", "a", "--file", "a.zip", "--apply", "--allow-incomplete"])
            .expect("a valid command line");
        assert_eq!(mail_exit_code(&apply, &planned(false, 0, 1)), 2, "unconfirmed mail needs its own flag");
        assert_eq!(mail_exit_code(&apply, &restored(0)), 0);
        assert_eq!(mail_exit_code(&apply, &restored(1)), 2);
    }

    #[test]
    fn a_plan_names_what_it_still_needs() {
        assert!(plan_blockers(&restore_plan(true, 0, 0), false, false).is_empty());
        assert_eq!(plan_blockers(&restore_plan(true, 2, 0), true, true).len(), 1, "uncertain mail blocks always");
        assert_eq!(plan_blockers(&restore_plan(false, 0, 3), false, false).len(), 2);
        assert!(plan_blockers(&restore_plan(false, 0, 3), true, true).is_empty());
    }

    #[test]
    fn retry_unconfirmed_needs_apply() {
        let args = ["restore", "--account", "a", "--file", "a.zip", "--apply", "--retry-unconfirmed"];
        assert!(mail(&args).expect("a valid command line").retry_unconfirmed);
        assert!(mail(&["restore", "--account", "a", "--file", "a.zip", "--retry-unconfirmed"]).is_err());
        assert!(mail(&["restore", "--account", "a", "--file", "a.zip", "--dry-run", "--retry-unconfirmed"]).is_err());
        assert!(mail(&["backup", "create", "--account", "a", "--retry-unconfirmed"]).is_err());
        assert!(mail(&["restore", "--account", "a", "--file", "a.zip", "--apply", "--retry-unconfirmed=yes"]).is_err());
    }

    #[test]
    fn json_may_stand_before_mail() {
        let words = |args: &[&str]| args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
        let tail = mail_args(&words(&["ctl", "--json", "mail", "backup", "status", "--account", "a", "--job", "j"]))
            .expect("a mail command");
        let command = parse_mail(&tail).expect("a valid command line");
        assert!(command.json);
        assert_eq!(command.request, Request::MailBackupStatus { account: "a".into(), job: "j".into() });
        assert!(mail_args(&words(&["ctl", "mail", "restore"])).is_some());
        assert!(mail_args(&words(&["ctl", "--json", "setups", "list"])).is_none());
        assert!(mail_args(&words(&["status"])).is_none());
    }

    #[test]
    fn relative_archive_paths_are_made_absolute() {
        let file = |file: &str| match mail(&["backup", "verify", "--account", "a", "--file", file])
            .expect("a valid command line")
            .request
        {
            Request::MailBackupVerify { file, .. } => file,
            other => panic!("verify asks for a verification, not {other:?}"),
        };
        assert_eq!(file("anna.zip"), "anna.zip", "a plain name stays a name in the managed folder");
        for relative in ["./anna.zip", "sub/anna.zip"] {
            let absolute = file(relative);
            assert!(Path::new(&absolute).is_absolute() && absolute.ends_with("anna.zip"), "{relative}: {absolute}");
            let here = std::env::current_dir().expect("a current directory");
            assert!(Path::new(&absolute).starts_with(&here), "{absolute} lies under {}", here.display());
        }
        let selected = std::env::temp_dir().join("anna.zip").to_string_lossy().into_owned();
        assert_eq!(file(&selected), selected);
    }

    #[test]
    fn mail_answers_print_as_text_or_json() {
        let job = Response::MailBackupJob { job: backup_job(BackupPhase::Reading) };
        let text = output(&job, false);
        assert!(
            text.contains(&trf("Backup {id}: {phase}", &[("id", &"job-1"), ("phase", &tr("Reading mail"))])),
            "{text}"
        );
        assert!(text.contains(&trf("Archive: {name}", &[("name", &"a.zip")])), "{text}");
        assert!(text.contains(&trf("Warning: {text}", &[("text", &"Folder Spam was not readable")])), "{text}");
        let json: serde_json::Value = serde_json::from_str(&output(&job, true)).expect("JSON");
        assert_eq!(json["result"], "mail-backup-job", "{json}");
        assert_eq!(json["job"]["phase"], "reading");

        assert!(output(&verified(false), false).starts_with(tr("Archive incomplete")));
        let plan = output(&Response::MailRestorePlanned { plan: restore_plan(false, 0, 2) }, false);
        assert!(plan.contains(tr("The archive is incomplete.")), "{plan}");
        assert!(plan.contains(&trf("Unconfirmed uploads of an earlier restore: {n}", &[("n", &2)])), "{plan}");
        assert!(plan.contains("[Gmail]/All Mail") && plan.contains(&trf("Plan: {id}", &[("id", &"plan-1")])), "{plan}");
        assert!(output(&restored(0), false).starts_with(tr("Restore finished")));
        assert!(!output(&restored(0), false).starts_with('{'), "text, not JSON");
    }

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
            updated: DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch"),
            shortcut: None,
            startup: None,
            workspace: "1".into(),
            monitor: None,
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
        let command = parse_strs(&["ctl", "setups", "list", "--json"]).expect("a valid command line");
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

    /// Issue #243: `ctl setups key <id> <0-9|none>`.
    #[test]
    fn setups_key_takes_a_digit_or_none() {
        let key = |key| Request::SetTemplateShortcut { id: "dev".into(), key };
        assert_eq!(request(&["ctl", "setups", "key", "dev", "1"]), key(Some(1)));
        assert_eq!(request(&["ctl", "setups", "key", "dev", "0"]), key(Some(0)));
        assert_eq!(request(&["ctl", "setups", "key", "dev", "none"]), key(None));
        for bad in
            [&["ctl", "setups", "key", "dev"][..], &["ctl", "setups", "key", "dev", "10"], &["ctl", "setups", "key"]]
        {
            assert!(parse_strs(bad).is_err(), "{bad:?}");
        }
        assert!(parse_strs(&["ctl", "setups", "key", "dev", "F1"]).is_err());
        let held = TemplateSummary { shortcut: Some(3), ..summary("dev", "Dev", 2) };
        assert_eq!(render(&Response::Saved { template: held }), "saved dev  Dev  (2 windows, key 3)");
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
        let command = parse_strs(&["ctl", "--json", "setups", "delete", "dev"]).expect("a valid command line");
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

    /// #231: the help names every kind the core takes for `--kind`, and no other.
    #[test]
    fn the_help_names_the_kinds_the_core_takes() {
        let help = usage();
        let line = help.lines().find(|line| line.contains("clients add")).expect("the line of clients add");
        let named = line.split("--kind ").nth(1).and_then(|rest| rest.split(']').next()).expect("its kinds");
        assert_eq!(named.split('|').collect::<Vec<_>>(), torro_ipc::api::CLIENT_KINDS);
        assert!(!help.contains("{kinds}"));
    }

    #[test]
    fn clients_add_with_name_kind_and_template() {
        let add = |kind: &str, template| Request::CreateClient {
            name: "Claude Code".into(),
            kind: kind.into(),
            template,
            agent: None,
            rights: None,
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
            created: DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch"),
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
            wm_version: None,
            shortcuts: None,
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
        let at = DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch");
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
        let until =
            DateTime::<Utc>::from_timestamp(0, 0).expect("the epoch").with_timezone(&chrono::Local).format("%H:%M");
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
                    shortcut: None,
                    startup: None,
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
            torro_ipc::server::serve_with_key(&path, move |key, door, request| {
                let core = Arc::clone(&core);
                async move { core.handle_from(key.as_ref(), door, request).await.unwrap_or_else(|r| r.response()) }
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
            trf("Posted to the Notification Hub ({id}).", &[("id", &"i1")])
        );
        assert_eq!(
            render(&Response::Posted { id: None }),
            tr("Not posted: this source is muted in the Notification Hub.")
        );
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
            torro_ipc::server::serve_with_key(&path, move |key, door, request| {
                let core = Arc::clone(&core);
                async move { core.handle_from(key.as_ref(), door, request).await.unwrap_or_else(|r| r.response()) }
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

        let all = Request::ListItems {
            lane: None,
            source: None,
            state: None,
            since: None,
            limit: None,
            order: None,
            dismissed: None,
            categories: Vec::new(),
        };
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
