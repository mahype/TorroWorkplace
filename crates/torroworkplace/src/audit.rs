//! The audit log (`docs/api.md`, "Protokoll", task A.5): `audit.jsonl` in the core's state
//! directory, mode 0600, one JSON line ([`ActivityEntry`]) per request of a client key and per run
//! of a command a person allowed. It says who did what to which ids and how it ended, so a person
//! can follow what an agent did (`clients.activity`).
//!
//! **No contents, ever.** A line holds the time, the client id, the command's catalogue name, the
//! ids the command named ([`ids`]) and the result word ([`outcome`]). Never titles, names, search
//! terms, bodies, thread keys, approval summaries, requests or answers: those hold what the
//! person wrote (an answer of `calendar.list_events` is the calendar; an approval's summary and
//! its stored answer carry titles; inbox rule 7 keeps titles, bodies and thread keys out of every
//! log). Requests of the app key are not logged: that would be every move in the control centre.
//!
//! The file is rotated at 5 MB ([`ROTATE_AT`]): it becomes `audit.jsonl.1`, replacing an older
//! one, so the log never takes more than about twice that.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use torro_core::AppId;
use torro_core::paths::AppPaths;
use torro_ipc::mail::MoveTarget;
use torro_ipc::{ActivityEntry, Request, Response};

/// The log's name in the state directory.
pub const FILE: &str = "audit.jsonl";

/// The log is rotated before a line would make it larger than this.
pub const ROTATE_AT: u64 = 5 * 1024 * 1024;

/// At most this many entries answer `clients.activity`.
pub const ACTIVITY_MAX: usize = 50;

/// An id longer than this is cut: ids come from the client, and a line must stay a line.
const ID_MAX: usize = 100;

/// Why the log could not be read. Says no path: the answer goes to a client; the path is only
/// printed to the core's own log.
#[derive(Debug)]
pub enum AuditError {
    Io(io::Error),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::Io(err) => write!(f, "cannot read the audit log: {err}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl AuditError {
    /// The answer to the app key: code `error`.
    pub fn response(&self) -> Response {
        Response::Error { code: "error".into(), message: self.to_string() }
    }
}

/// The audit log of the core.
pub struct Audit {
    /// `audit.jsonl`; `None` without a home directory and in tests that pass none: then nothing
    /// is written and the activity is empty.
    file: Option<PathBuf>,
    /// [`ROTATE_AT`]; tests pass a small one.
    rotate_at: u64,
    /// One write (and rotation) at a time, so lines never interleave and none is lost to a
    /// rename in between.
    writing: Mutex<()>,
}

impl Audit {
    /// The user's log: `audit.jsonl` in the core's state directory.
    ///
    /// In tests there is none, so a test that forgets to pass its own
    /// ([`crate::core::Core::with_audit`]) never writes into the user's state directory.
    pub fn system() -> Audit {
        if cfg!(test) {
            return Audit::new(None, ROTATE_AT);
        }
        Audit::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join(FILE)), ROTATE_AT)
    }

    /// A log in `file`, rotated before it would grow past `rotate_at` bytes.
    pub fn new(file: Option<PathBuf>, rotate_at: u64) -> Audit {
        Audit { file, rotate_at, writing: Mutex::new(()) }
    }

    /// Appends `entry` as one line. Never fails the request it describes: a log that cannot be
    /// written is reported on stderr, and the request's answer goes out all the same.
    pub fn record(&self, entry: &ActivityEntry) {
        let Some(file) = &self.file else { return };
        let mut line = match serde_json::to_vec(entry) {
            Ok(line) => line,
            Err(err) => return eprintln!("torroworkplace: cannot write the audit log: {err}"),
        };
        line.push(b'\n');
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(err) = append(file, &line, self.rotate_at) {
            eprintln!("torroworkplace: cannot write the audit log {}: {err}", file.display());
        }
    }

    /// The latest entries of `client`, newest first, at most [`ACTIVITY_MAX`]: from the log and,
    /// when that holds too few, from the rotated one. A log that is not there yet is empty; a
    /// line that does not parse is skipped.
    pub fn recent(&self, client: &str) -> Result<Vec<ActivityEntry>, AuditError> {
        let Some(file) = &self.file else { return Ok(Vec::new()) };
        let mut found = Vec::new();
        for path in [file.clone(), rotated(file)] {
            let mut older = read_client(&path, client).map_err(|err| {
                eprintln!("torroworkplace: cannot read the audit log {}: {err}", path.display());
                AuditError::Io(err)
            })?;
            older.reverse();
            found.extend(older);
            if found.len() >= ACTIVITY_MAX {
                break;
            }
        }
        found.truncate(ACTIVITY_MAX);
        Ok(found)
    }
}

/// The line about a request of `client` (`None`: an unknown key) that got `answer`, for `ids`
/// taken before the request ran ([`ids`]).
pub fn entry(
    client: Option<String>,
    command: &str,
    ids: Vec<String>,
    answer: &Response,
    time: DateTime<Utc>,
) -> ActivityEntry {
    let (result, approval) = outcome(answer);
    ActivityEntry { time, client, command: command.to_string(), ids, result, approval }
}

/// The result word of `answer` and the approval it names: `pending` with the approval it waits
/// for, the error code of an error, otherwise `ok`. Only the code of an error, never its message,
/// which can quote what the client sent.
pub fn outcome(answer: &Response) -> (String, Option<String>) {
    match answer {
        Response::Pending { approval } => ("pending".into(), Some(approval.id.clone())),
        Response::Error { code, .. } => (code.clone(), None),
        _ => ("ok".into(), None),
    }
}

/// The ids `request` names that may go into the log, each as `kind:value`
/// ([`ActivityEntry::ids`]).
///
/// What counts as safe:
///
/// - **As sent, if they look the part:** ids that carry no content by themselves. Inbox entries,
///   rules, approvals and clients are random hex digits the suite made ([`is_random_id`]);
///   accounts are `<kind>:<name>` ([`is_account_id`]). All of them come from the client, which can
///   put any text there, so one that does not have its form is hashed instead (task E1.2c). Also
///   a source, a setting's key (never its value), and an app only when it is one of the suite's.
/// - **Partly hashed:** a collection id keeps its account and hashes the rest
///   (`collection:<account>/<hash>`, [`collection`]): the rest is the calendar's URL on the
///   server, with server, user and the calendar's name in it.
/// - **Hashed** ([`hashed`]): what can be made from a name. CalDAV hrefs are often a UUID, but
///   other programs name them after the title (`Dinner-with-Anna.ics`); issue URLs name the
///   repository; template ids are made from the template's name (`dev-morning`).
/// - **Never:** titles, names, search terms, bodies, thread keys, server addresses (a URL can
///   hold a user name), tokens, settings' values, time ranges of a query.
///
/// A hashed id that could also have been sent as it is says so (`inbox:hash:<hash>`), so it is
/// never taken for an id of that form. The match has no catch-all on purpose: a new request must
/// get its rule here.
pub fn ids(request: &Request) -> Vec<String> {
    let id = |kind: &str, value: &str| format!("{kind}:{}", cut(value));
    // An id from the client as it is when it has its form, else hashed.
    let checked = |kind: &str, value: &str, form: fn(&str) -> bool| match form(value) {
        true => id(kind, value),
        false => format!("{kind}:hash:{}", hashed(value)),
    };
    let random = |kind: &str, value: &str| checked(kind, value, is_random_id);
    let account = |value: &str| checked("account", value, is_account_id);
    let item = |href: &Option<String>| href.as_deref().map(|href| format!("item:{}", hashed(href)));
    match request {
        Request::Ping
        | Request::Status
        | Request::Quit
        | Request::ListTemplates
        | Request::SaveTemplate { .. }
        | Request::ConnectNextcloud { .. }
        | Request::ConnectGithub
        | Request::SetGithubToken { .. }
        // Never the address searched or signed in with (task M7.1): it names a person.
        | Request::DiscoverServices { .. }
        | Request::ConnectCalDav { .. }
        | Request::Sync
        | Request::CalendarCollections
        | Request::TaskCollections
        | Request::Events { .. }
        | Request::Todos
        | Request::Settings
        | Request::UpdateSettings { .. }
        | Request::Accounts
        | Request::ListApps
        | Request::ListDesigns
        | Request::ListClients
        | Request::CreateClient { .. }
        | Request::ListApprovals
        | Request::ListItems { .. }
        | Request::ListSources
        | Request::PostItem { .. }
        | Request::ResolveItems { .. }
        | Request::ListRules => Vec::new(),
        Request::UpdateTemplate { id: template }
        | Request::DeleteTemplate { id: template }
        | Request::LoadTemplate { id: template, .. }
        | Request::RenameTemplate { id: template, .. } => vec![format!("template:{}", hashed(template))],
        Request::SaveEvent { event } | Request::DeleteEvent { event } => {
            [Some(collection(&event.collection)), item(&event.href)].into_iter().flatten().collect()
        }
        Request::SaveTodo { todo } | Request::DeleteTodo { todo } => {
            [Some(collection(&todo.collection)), item(&todo.href)].into_iter().flatten().collect()
        }
        Request::MoveTodo { href, before, .. } => {
            [item(&Some(href.clone())), item(before)].into_iter().flatten().collect()
        }
        // The account only, never the name of the new list (task G.7).
        Request::CreateTaskList { account: name, .. }
        | Request::CreateFilterList { account: name, .. }
        | Request::PreviewFilter { account: name, .. } => vec![account(name)],
        // The account, and the repository hashed: its name may be a private one (task G.9).
        Request::ListGithubRepos { account: name } => vec![account(name)],
        Request::DescribeGithubRepo { account: name, repo } => {
            vec![account(name), format!("repo:{}", hashed(&repo.trim().to_ascii_lowercase()))]
        }
        Request::UpdateFilterList { collection: list, .. }
        | Request::DeleteFilterList { collection: list }
        | Request::SetCollectionShown { collection: list, .. } => vec![collection(list)],
        Request::UnpinIssue { collection: list, url } | Request::AddLink { collection: list, url } => {
            vec![collection(list), format!("item:{}", hashed(url))]
        }
        Request::SetSetting { key, .. } => {
            let key = serde_json::to_value(key).ok().and_then(|key| key.as_str().map(str::to_string));
            key.map(|key| vec![id("setting", &key)]).unwrap_or_default()
        }
        Request::RemoveAccount { id: name } => vec![account(name)],
        Request::OpenApp { app } => match AppId::ALL.iter().any(|known| known.binary == app) {
            true => vec![id("app", app)],
            false => Vec::new(),
        },
        Request::UpdateClient { id: client, .. }
        | Request::NewClientKey { id: client }
        | Request::RemoveClient { id: client }
        | Request::ClientActivity { id: client } => vec![random("client", client)],
        Request::ApprovalStatus { id: approval } | Request::DecideApproval { id: approval, .. } => {
            vec![random("approval", approval)]
        }
        // A question names no id: its tool, project and command are what the agent would run, and
        // the approval it opens is in the line's `approval` already (task E2.5).
        Request::AskPermission { .. } => Vec::new(),
        Request::GetItem { id: entry }
        | Request::OpenItem { id: entry }
        | Request::SetItemDone { id: entry, .. }
        | Request::SnoozeItem { id: entry, .. }
        | Request::SetItemLane { id: entry, .. }
        | Request::HideSender { id: entry, .. } => vec![random("inbox", entry)],
        Request::CreateTaskFromItem { id: entry, collection: list, .. } => {
            vec![random("inbox", entry), collection(list)]
        }
        Request::MuteSource { source, .. } => vec![id("source", &source.to_string())],
        Request::RemoveRule { id: rule } => vec![random("rule", rule)],
        // Mail (security rule 3 of task M2.1): never the address signed in with, nor a search, a
        // subject, a recipient, a file or a folder name. The account as for collections; message,
        // draft and folder ids only hashed, so lines can be matched but not with a mailbox.
        Request::ConnectMail { .. } => Vec::new(),
        // TorroMail's accounts (task M7.4): no address, and no id of another app's keychain.
        Request::TorroMailAccounts | Request::ImportTorroMailAccount { .. } => Vec::new(),
        Request::UpdateMailAccount { id: name, .. } => vec![account(name)],
        mail @ (Request::MailFolders { .. }
        | Request::SearchMail { .. }
        | Request::GetMail { .. }
        | Request::GetMailThread { .. }
        | Request::GetMailAttachment { .. }
        | Request::SetMailFlags { .. }
        | Request::MoveMail { .. }
        | Request::DeleteMail { .. }
        | Request::SaveDraft { .. }
        | Request::SendDraft { .. }) => {
            let folder = match mail {
                Request::SearchMail { folder: Some(folder), .. }
                | Request::MoveMail { to: MoveTarget::Folder(folder), .. } => Some(folder),
                _ => None,
            };
            mail.mail_account()
                .map(account)
                .into_iter()
                .chain(folder.map(|folder| format!("folder:{}", hashed(folder))))
                .chain(mail.message_ids().into_iter().map(|message| format!("message:{}", hashed(message))))
                .collect()
        }
    }
}

/// A collection id (`<account id>/<the provider's id>`) for the log: the account as it is and the
/// rest hashed, `collection:nextcloud:anna@cloud.example.org/<hash>`. The rest is the calendar's
/// URL with server, user and the calendar's name in it. An id whose front is no account id (a bare
/// URL from before several accounts, or any text a client sent) is hashed whole:
/// `collection:hash:<hash>`. The account ends at the first `/`; for the rare account id with a `/`
/// in it the rest of it is hashed with the URL, which still names the collection the same way
/// every time.
fn collection(value: &str) -> String {
    match value.split_once('/') {
        Some((account, rest)) if is_account_id(account) => format!("collection:{account}/{}", hashed(rest)),
        _ => format!("collection:hash:{}", hashed(value)),
    }
}

/// The form of the ids the suite makes at random: inbox entries, rules and approvals (16 hex
/// digits), clients (8). Up to 36 hex digits and dashes, so a UUID passes too.
fn is_random_id(value: &str) -> bool {
    (1..=36).contains(&value.len()) && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The form of an account id: `<kind>:<name>`, the kind in small letters (`nextcloud`, `github`),
/// the whole without spaces or other characters a text would bring, at most [`ID_MAX`] long.
fn is_account_id(value: &str) -> bool {
    let Some((kind, name)) = value.split_once(':') else { return false };
    let printable = value.chars().all(|c| c.is_ascii_graphic());
    let kind_ok = !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase());
    kind_ok && !name.is_empty() && printable && value.len() <= ID_MAX
}

/// The first 16 hex digits of the SHA-256 of `value`: the same thing gets the same hash, so lines
/// about it can be matched, but the name it was made from does not show. It hides the name from
/// whoever reads the file, not from someone who guesses it and hashes the guess.
pub fn hashed(value: &str) -> String {
    Sha256::digest(value.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

/// `value` cut to [`ID_MAX`] characters.
fn cut(value: &str) -> &str {
    value.char_indices().nth(ID_MAX).map_or(value, |(end, _)| &value[..end])
}

/// Where the log goes when it is rotated: `audit.jsonl.1`.
fn rotated(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    file.with_file_name(name)
}

/// Appends `line` to `file` (created with mode 0600, its directory for the owner only), first
/// moving a log that would grow past `rotate_at` to [`rotated`].
fn append(file: &Path, line: &[u8], rotate_at: u64) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir)?;
    }
    let size = fs::metadata(file).map(|meta| meta.len()).unwrap_or(0);
    if size > 0 && size + line.len() as u64 > rotate_at {
        fs::rename(file, rotated(file))?;
    }
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(file)?;
    restrict(file)?;
    out.write_all(line)
}

/// A log made readable for others (by hand, by an older version) is made private again.
#[cfg(unix)]
fn restrict(file: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(file)?.permissions().mode();
    if mode & 0o077 != 0 {
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Windows keeps files in the profile private to the user already.
#[cfg(not(unix))]
fn restrict(file: &Path) -> io::Result<()> {
    let _ = file;
    Ok(())
}

/// The entries of `client` in `path`, oldest first; none when the file is not there.
fn read_client(path: &Path, client: &str) -> io::Result<Vec<ActivityEntry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut entries = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let Ok(entry) = serde_json::from_str::<ActivityEntry>(&line?) else { continue };
        if entry.client.as_deref() == Some(client) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A log in `dir/state/audit.jsonl`, rotated at the real size.
    pub(crate) fn audit_in(dir: &Path) -> Audit {
        Audit::new(Some(file_in(dir)), ROTATE_AT)
    }

    pub(crate) fn file_in(dir: &Path) -> PathBuf {
        dir.join("state").join(FILE)
    }

    fn line(client: &str, command: &str, n: i64) -> ActivityEntry {
        ActivityEntry {
            time: DateTime::from_timestamp(1_800_000_000 + n, 0).expect("time"),
            client: Some(client.into()),
            command: command.into(),
            ids: vec![format!("inbox:{n}")],
            result: "ok".into(),
            approval: None,
        }
    }

    #[test]
    fn the_log_is_private_and_one_line_per_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        audit.record(&line("a1", "inbox.get_item", 1));
        audit.record(&line("a1", "inbox.get_item", 2));
        let text = fs::read_to_string(file_in(dir.path())).expect("written");
        assert_eq!(text.lines().count(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(file_in(dir.path())).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_activity_is_the_clients_own_newest_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        audit.record(&line("a1", "system.ping", 1));
        audit.record(&line("b2", "system.status", 2));
        audit.record(&line("a1", "inbox.get_item", 3));
        let recent = audit.recent("a1").expect("read");
        assert_eq!(recent, vec![line("a1", "inbox.get_item", 3), line("a1", "system.ping", 1)]);
        assert_eq!(audit_in(&dir.path().join("none")).recent("a1").expect("no log yet"), vec![]);
    }

    #[test]
    fn the_log_is_rotated_before_it_grows_past_the_limit_and_both_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let one = serde_json::to_vec(&line("a1", "system.ping", 1)).expect("json").len() as u64 + 1;
        let audit = Audit::new(Some(file_in(dir.path())), one * 2);
        for n in 1..=3 {
            audit.record(&line("a1", "system.ping", n));
        }
        let current = fs::read_to_string(file_in(dir.path())).expect("log");
        let old = fs::read_to_string(rotated(&file_in(dir.path()))).expect("rotated");
        assert_eq!((old.lines().count(), current.lines().count()), (2, 1));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(rotated(&file_in(dir.path()))).expect("exists").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let commands: Vec<_> = audit.recent("a1").expect("read").into_iter().map(|e| e.time.timestamp()).collect();
        assert_eq!(commands, vec![1_800_000_003, 1_800_000_002, 1_800_000_001]);
    }

    #[test]
    fn at_most_fifty_entries_answer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let audit = audit_in(dir.path());
        for n in 0..60 {
            audit.record(&line("a1", "system.ping", n));
        }
        let recent = audit.recent("a1").expect("read");
        assert_eq!(recent.len(), ACTIVITY_MAX);
        assert_eq!(recent[0], line("a1", "system.ping", 59));
    }

    #[test]
    fn without_a_file_nothing_is_written() {
        let audit = Audit::system();
        audit.record(&line("a1", "system.ping", 1));
        assert_eq!(audit.recent("a1").expect("empty"), vec![]);
    }

    #[test]
    fn ids_never_carry_a_name_a_title_or_a_search() {
        let event = torro_pim::Event {
            uid: "u".into(),
            collection: "nextcloud:anna@cloud.example.org/cal/work/".into(),
            href: Some("https://cloud.example.org/cal/work/Dinner-with-Anna.ics".into()),
            etag: None,
            summary: "Dinner with Anna".into(),
            start: torro_pim::EventTime::DateTime(Utc::now()),
            end: None,
            location: Some("Secret place".into()),
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
        };
        let ids = ids(&Request::DeleteEvent { event });
        assert_eq!(ids[0], format!("collection:nextcloud:anna@cloud.example.org/{}", hashed("cal/work/")));
        assert!(ids[1].starts_with("item:") && ids[1].len() == "item:".len() + 16, "{ids:?}");
        assert!(!ids.join(" ").contains("Dinner") && !ids.join(" ").contains("work"), "{ids:?}");

        let template = super::ids(&Request::RenameTemplate { id: "secret-plans".into(), name: "Secret plans".into() });
        assert!(!template.join(" ").contains("secret"), "{template:?}");
        let preview = Request::PreviewFilter { account: "github:anna".into(), queries: vec!["is:open salary".into()] };
        assert_eq!(super::ids(&preview), vec!["account:github:anna".to_string()]);
        // The account, and a repository's name only hashed (task G.9), the same in any case.
        let repos = Request::ListGithubRepos { account: "github:anna".into() };
        assert_eq!(super::ids(&repos), vec!["account:github:anna".to_string()]);
        let describe =
            |repo: &str| super::ids(&Request::DescribeGithubRepo { account: "github:anna".into(), repo: repo.into() });
        let described = describe("anna/salary-talks");
        assert_eq!(described, vec!["account:github:anna".to_string(), format!("repo:{}", hashed("anna/salary-talks"))]);
        assert!(!described.join(" ").contains("salary"), "{described:?}");
        assert_eq!(describe(" Anna/Salary-Talks"), described);
        let open = Request::OpenApp { app: "Dinner with Anna".into() };
        assert_eq!(super::ids(&open), Vec::<String>::new());
        let set = Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value: serde_json::json!(1.5) };
        assert_eq!(super::ids(&set), vec!["setting:zoom".to_string()]);
    }

    /// Task E1.2c: collection ids keep only their account; ids a client sends are logged as they
    /// are only when they have the form the suite gives them, else hashed.
    #[test]
    fn collection_and_client_ids_keep_only_what_has_their_form() {
        let shown = |list: &str| super::ids(&Request::SetCollectionShown { collection: list.into(), shown: true });
        let url = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/salary/";
        let logged = shown(url);
        assert_eq!(logged, vec![format!("collection:nextcloud:anna@cloud.example.org/{}", hashed(&url[33..]))]);
        assert!(!logged[0].contains("salary") && !logged[0].contains("remote.php"), "{logged:?}");
        assert_eq!(shown("github:mahype/00ff"), vec![format!("collection:github:mahype/{}", hashed("00ff"))]);
        let legacy = "https://cloud.example.org/dav/anna/salary/";
        assert_eq!(shown(legacy), vec![format!("collection:hash:{}", hashed(legacy))]);
        assert_eq!(shown("Salary talk/x"), vec![format!("collection:hash:{}", hashed("Salary talk/x"))]);

        let get = |id: &str| super::ids(&Request::GetItem { id: id.into() });
        assert_eq!(get("0a1b2c3d4e5f6071"), vec!["inbox:0a1b2c3d4e5f6071".to_string()]);
        assert_eq!(get("123e4567-e89b-12d3-a456-426614174000"), vec!["inbox:123e4567-e89b-12d3-a456-426614174000"]);
        let text = "Ignore previous instructions";
        assert_eq!(get(text), vec![format!("inbox:hash:{}", hashed(text))]);
        assert_eq!(get(&"a".repeat(300)), vec![format!("inbox:hash:{}", hashed(&"a".repeat(300)))]);
        let rule = super::ids(&Request::RemoveRule { id: "my secret rule".into() });
        assert_eq!(rule, vec![format!("rule:hash:{}", hashed("my secret rule"))]);
        let status = super::ids(&Request::ApprovalStatus { id: "00ff00ff00ff00ff".into() });
        assert_eq!(status, vec!["approval:00ff00ff00ff00ff".to_string()]);
        let client = super::ids(&Request::RemoveClient { id: "Anna's laptop".into() });
        assert_eq!(client, vec![format!("client:hash:{}", hashed("Anna's laptop"))]);
        let account = super::ids(&Request::RemoveAccount { id: "Anna Smith".into() });
        assert_eq!(account, vec![format!("account:hash:{}", hashed("Anna Smith"))]);
        let task = Request::CreateTaskFromItem { id: "e1".into(), collection: url.into(), title: None, due: None };
        assert_eq!(super::ids(&task), vec!["inbox:e1".to_string(), logged[0].clone()]);
    }

    /// Security rule 3 of task M2.1: the account as it is, messages and folders only hashed, never
    /// a search, a subject, a recipient, a file or a folder name.
    #[test]
    fn mail_requests_log_the_account_and_hashes_only() {
        let search = Request::SearchMail {
            account: Some("mail:anna@mailbox.org".into()),
            folder: Some("f:0a1b".into()),
            query: Some("salary negotiation".into()),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit: 20,
            cursor: None,
        };
        let logged = super::ids(&search);
        assert_eq!(logged, vec!["account:mail:anna@mailbox.org".to_string(), format!("folder:{}", hashed("f:0a1b"))]);
        let draft = torro_mail::Draft {
            id: None,
            account: "mail:anna@mailbox.org".into(),
            to: vec![torro_mail::MailAddress { name: Some("Bob".into()), address: "bob@example.org".into() }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Secret plans".into(),
            body: "…".into(),
            in_reply_to: Some("m:7".into()),
            attachments: vec![torro_mail::DraftAttachment::File { path: "/home/anna/contract.pdf".into() }],
        };
        let logged = super::ids(&Request::SaveDraft { draft });
        assert_eq!(logged, vec!["account:mail:anna@mailbox.org".to_string(), format!("message:{}", hashed("m:7"))]);
        let line = logged.join(" ");
        for secret in ["bob", "Secret", "contract", "salary"] {
            assert!(!line.contains(secret), "{line}");
        }
        let to = torro_ipc::mail::MoveTarget::Folder("f:9".into());
        let moved = super::ids(&Request::MoveMail { ids: vec!["m:1".into(), "m:2".into()], to });
        let expected = [format!("folder:{}", hashed("f:9")), format!("message:{}", hashed("m:1"))];
        assert_eq!(moved[..2], expected, "{moved:?}");
        assert_eq!(moved.len(), 3);
        let update = Request::UpdateMailAccount {
            id: "mail:anna@mailbox.org".into(),
            sender_name: Some("Anna Example".into()),
            signature: None,
            notify: None,
            services: None,
            folder_roles: None,
        };
        assert_eq!(super::ids(&update), vec!["account:mail:anna@mailbox.org".to_string()]);
    }
}
