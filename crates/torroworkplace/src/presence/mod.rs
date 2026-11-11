//! Presence in the core (`docs/chat.md`, decision 10; `docs/praesenz.md`): who of the person's
//! Nextcloud is there, and the person's own status.
//!
//! - **An area of its own** with accounts of its own ([`Rights::reaches_presence_account`]): status
//!   exists without Talk, and whoever gives an agent the chats does not thereby tell it who sits at
//!   their computer.
//! - **Who is in the list:** the people the core can name on the account: those of the server's
//!   address book (handed in by the core, [`PresenceService::with_names`]), the other people of
//!   the direct chats, and those the person added themselves ([`Added`], kept in `presence.json`).
//!   The server's status list alone names nobody: it carries user ids without names.
//! - **Asking:** one call for everyone when the server answers it; when its list is empty (an admin
//!   restricted it) the people are asked one by one, at most [`SINGLE_PER_MINUTE`] a minute, and
//!   the rest are "unknown" and never count as online.
//! - **Only in memory**, and only while someone asks: the list is fetched again when it is older
//!   than [`FRESH_FOR`].
//! - **Heartbeat** ([`PresenceService::beat`]): off unless the person switched it on; the core
//!   calls it every five minutes then.
//!
//! Logs name an account's id and a code, never a name or a status message.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_chat::{ChatError, OwnStatus, Person, Status, person_id, split_person};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::paths::AppPaths;
use torro_ipc::chat::{MAX_PEOPLE, MAX_STATUS_MESSAGE, MIN_QUERY};
use torro_ipc::{Request, Response};

use crate::chat::ChatService;
use crate::chat::remote::{RemotePerson, RemoteStatus};
use crate::chat::service::error_response;
use crate::pim::Reach;

/// How old the list of people may be before it is fetched again.
pub const FRESH_FOR: Duration = Duration::from_secs(60);

/// The most people asked for their status one by one in a minute, when the server lists none.
pub const SINGLE_PER_MINUTE: usize = 50;

/// How often the heartbeat is sent while it is on.
pub const BEAT_EVERY: Duration = Duration::from_secs(5 * 60);

/// The file with the people a person added to their list, in the core's configuration directory.
const FILE: &str = "presence.json";

/// The people a person added to their list themselves, by account: user id and the name known
/// when they were added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Added {
    #[serde(default)]
    accounts: BTreeMap<String, Vec<AddedPerson>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AddedPerson {
    id: String,
    name: String,
}

/// What is held of one account's people.
#[derive(Debug, Default)]
struct Held {
    people: Vec<Person>,
    at: Option<Instant>,
    /// When people were last asked one by one, for the budget of [`SINGLE_PER_MINUTE`].
    singles: Vec<Instant>,
}

/// The people of an account's address book on the server, by user id; empty when the core has
/// none (the book is closed or not synced).
pub type Names = Arc<dyn Fn(&str) -> Vec<RemotePerson> + Send + Sync>;

/// See the module docs.
pub struct PresenceService {
    chat: Arc<ChatService>,
    names: Names,
    held: Mutex<BTreeMap<String, Held>>,
    added: Mutex<Added>,
    /// `None`: nothing is kept across restarts (tests, no home directory).
    file: Option<PathBuf>,
}

impl std::fmt::Debug for PresenceService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the people: names end up in logs otherwise.
        f.debug_struct("PresenceService").field("file", &self.file).finish_non_exhaustive()
    }
}

/// Whether `reach` sees the people of `account`.
fn reaches(reach: Reach<'_>, account: &str) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.reaches_presence_account(account),
    }
}

/// The server's name in an account id (`nextcloud:<login>@<host>`), for "Everyone on <server>
/// sees …"; the id itself when it has no `@`.
pub fn server_of(account: &str) -> &str {
    account.rsplit_once('@').map_or(account, |(_, host)| host)
}

impl PresenceService {
    /// The service of the core, over the chat service's connections.
    pub fn system(chat: Arc<ChatService>) -> PresenceService {
        let file = if cfg!(test) { None } else { AppPaths::for_app(AppId::WORKPLACE).map(|p| p.config.join(FILE)) };
        PresenceService::open(chat, file)
    }

    /// The service keeping its list in `dir` (tests).
    #[cfg(test)]
    pub fn in_dir(chat: Arc<ChatService>, dir: &Path) -> PresenceService {
        PresenceService::open(chat, Some(dir.join(FILE)))
    }

    fn open(chat: Arc<ChatService>, file: Option<PathBuf>) -> PresenceService {
        let added = file.as_deref().map(read_added).unwrap_or_default();
        PresenceService {
            chat,
            names: Arc::new(|_| Vec::new()),
            held: Mutex::new(BTreeMap::new()),
            added: Mutex::new(added),
            file,
        }
    }

    /// The service with `names` as the way to the people of an account's address book.
    pub fn with_names(mut self, names: Names) -> PresenceService {
        self.names = names;
        self
    }

    fn held(&self) -> MutexGuard<'_, BTreeMap<String, Held>> {
        self.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn added(&self) -> MutexGuard<'_, Added> {
        self.added.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// For a client: the account in `request` it may not reach, or an id that names none, answered
    /// `not-found` before anything else; `None` when everything is in reach and for requests of
    /// other areas.
    pub fn out_of_reach(&self, reach: Reach<'_>, request: &Request) -> Option<String> {
        let Reach::Client(rights) = reach else { return None };
        let (account, person) = match request {
            Request::PresenceListPeople { account } => (account.as_deref()?, None),
            Request::PresenceFindPeople { account, .. }
            | Request::PresenceGetOwnStatus { account }
            | Request::PresenceSetOwnStatus { account, .. } => (account.as_str(), None),
            Request::PresenceAddPerson { account, person } | Request::PresenceRemovePerson { account, person } => {
                (account.as_str(), Some(person.as_str()))
            }
            _ => return None,
        };
        let foreign = person.is_some_and(|person| split_person(person).is_none_or(|(of, _)| of != account));
        (foreign || !rights.reaches_presence_account(account)).then(|| account.to_string())
    }

    /// Answers a `presence.*` request for `reach` (its rights and reach checked before).
    /// `client` is the client's id for the budget of what it sends, `heartbeat` whether "Show
    /// others when I am online" is on.
    pub async fn handle(&self, request: Request, reach: Reach<'_>, client: Option<&str>, heartbeat: bool) -> Response {
        if request.operation().command.is_outbound()
            && let Some(client) = client
            && !self.chat.throttle().admit(client, Instant::now())
        {
            return error_response(&ChatError::Busy);
        }
        let answer = match request {
            Request::PresenceListPeople { account } => {
                self.list(reach, account.as_deref()).await.map(|people| Response::PresencePeople { people })
            }
            Request::PresenceFindPeople { account, query, limit } => self.find(&account, &query, limit).await,
            Request::PresenceAddPerson { account, person } => self.add(&account, &person).await,
            Request::PresenceRemovePerson { account, person } => self.remove(&account, &person),
            Request::PresenceGetOwnStatus { account } => self
                .own(&account, heartbeat)
                .await
                .map(|status| Response::PresenceOwnStatus { status: Box::new(status) }),
            Request::PresenceSetOwnStatus { account, status, message, icon, clear_at } => {
                self.set_own(&account, status, message.as_deref(), icon.as_deref(), clear_at, heartbeat).await
            }
            other => Err(ChatError::Other(format!("{} is no presence command", other.operation().name()))),
        };
        answer.unwrap_or_else(|err| error_response(&err))
    }

    async fn list(&self, reach: Reach<'_>, account: Option<&str>) -> Result<Vec<Person>, ChatError> {
        let accounts: Vec<String> = match account {
            Some(account) => vec![account.to_string()],
            None => self.chat.accounts().await.into_iter().filter(|id| reaches(reach, id)).collect(),
        };
        let mut people = Vec::new();
        let mut failed = None;
        for id in &accounts {
            match self.people_of(id).await {
                Ok(found) => people.extend(found),
                Err(err) => failed = Some(err),
            }
        }
        if let Some(err) = failed
            && people.is_empty()
        {
            return Err(err);
        }
        Ok(people)
    }

    /// The people of `account` with their status: from memory while fresh, else asked again. When
    /// the server cannot be reached, what is held still shows.
    async fn people_of(&self, account: &str) -> Result<Vec<Person>, ChatError> {
        let fresh = self.held().get(account).is_some_and(|held| held.at.is_some_and(|at| at.elapsed() < FRESH_FOR));
        if !fresh {
            match self.fetch(account).await {
                Ok(people) => {
                    let mut held = self.held();
                    let entry = held.entry(account.to_string()).or_default();
                    entry.people = people;
                    entry.at = Some(Instant::now());
                }
                Err(ChatError::Offline) if self.held().get(account).is_some_and(|held| held.at.is_some()) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(self.held().get(account).map(|held| held.people.clone()).unwrap_or_default())
    }

    /// Asks the server for the status of everyone the core can name on `account`.
    async fn fetch(&self, account: &str) -> Result<Vec<Person>, ChatError> {
        let (remote, caps, me) = self.chat.known(account).await?;
        // Who there is, by user id: the address book first (its names are the fullest), then the
        // people of direct chats, then those added by hand. Each user once, never the person.
        let mut known: BTreeMap<String, (String, bool)> = BTreeMap::new();
        let added = self.added().accounts.get(account).cloned().unwrap_or_default();
        let mut take = |person: RemotePerson, added: bool| {
            if person.id != me && !person.id.is_empty() {
                let entry = known.entry(person.id).or_insert((person.name, added));
                entry.1 |= added;
            }
        };
        for person in (self.names)(account) {
            take(person, false);
        }
        for person in self.chat.direct_people(account).await {
            take(person, false);
        }
        for person in added {
            take(RemotePerson { id: person.id, name: person.name }, true);
        }
        let mut statuses: BTreeMap<String, RemoteStatus> = BTreeMap::new();
        let mut asked: Vec<String> = Vec::new();
        if caps.status {
            let listed = self.chat.asked(account, remote.statuses().await).await?;
            if listed.is_empty() {
                // The server lists nobody (an admin restricted it): ask one by one, within budget.
                let room = self.single_budget(account, known.len());
                for user in known.keys().take(room) {
                    if let Some(status) = self.chat.asked(account, remote.status_of(user).await).await? {
                        statuses.insert(user.clone(), status);
                    }
                    asked.push(user.clone());
                }
            } else {
                asked.extend(known.keys().cloned());
                statuses.extend(listed.into_iter().map(|status| (status.user.clone(), status)));
            }
        }
        let mut people: Vec<Person> = known
            .into_iter()
            .map(|(user, (name, added))| {
                let status = statuses.remove(&user);
                Person {
                    id: person_id(account, &user),
                    account: account.to_string(),
                    name,
                    // Others never see "invisible": it shows as offline.
                    status: match status.as_ref().map(|status| status.status) {
                        Some(Status::Invisible) | None => Status::Offline,
                        Some(status) => status,
                    },
                    // Asked and nothing told is offline; not asked at all is unknown.
                    unknown: !asked.contains(&user),
                    message: status.as_ref().and_then(|status| status.message.clone()),
                    icon: status.as_ref().and_then(|status| status.icon.clone()),
                    clear_at: status.as_ref().and_then(|status| status.clear_at),
                    added,
                    untrusted: true,
                }
            })
            .collect();
        people.sort_by_key(|person| person.name.to_lowercase());
        Ok(people)
    }

    /// How many of `wanted` people may be asked one by one now, and notes them as asked.
    fn single_budget(&self, account: &str, wanted: usize) -> usize {
        let now = Instant::now();
        let mut held = self.held();
        let entry = held.entry(account.to_string()).or_default();
        entry.singles.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
        let room = SINGLE_PER_MINUTE.saturating_sub(entry.singles.len()).min(wanted);
        entry.singles.extend(std::iter::repeat_n(now, room));
        room
    }

    async fn find(&self, account: &str, query: &str, limit: Option<u32>) -> Result<Response, ChatError> {
        let query = query.trim();
        if query.chars().count() < MIN_QUERY {
            return Err(ChatError::Other(tr("A search needs at least two characters.").to_string()));
        }
        let (remote, _, me) = self.chat.known(account).await?;
        let limit = limit.unwrap_or(MAX_PEOPLE).clamp(1, MAX_PEOPLE);
        let found = self.chat.asked(account, remote.find_users(query, limit).await).await?;
        let people = found
            .into_iter()
            .filter(|person| person.id != me)
            .map(|person| crate::chat::person(account, person))
            .collect();
        Ok(Response::PresencePeople { people })
    }

    async fn add(&self, account: &str, person: &str) -> Result<Response, ChatError> {
        let user = match split_person(person) {
            Some((of, user)) if of == account => user.to_string(),
            _ => return Err(ChatError::NotFound),
        };
        let (remote, ..) = self.chat.known(account).await?;
        // The name comes from the server, never from the caller: only someone the server finds
        // under exactly this id is added.
        let found = self.chat.asked(account, remote.find_users(&user, MAX_PEOPLE).await).await?;
        let name = found.into_iter().find(|found| found.id == user).ok_or(ChatError::NotFound)?.name;
        {
            let mut added = self.added();
            let list = added.accounts.entry(account.to_string()).or_default();
            if !list.iter().any(|held| held.id == user) {
                list.push(AddedPerson { id: user, name });
            }
            self.save(&added)?;
        }
        self.forget_held(account);
        Ok(Response::Ok)
    }

    fn remove(&self, account: &str, person: &str) -> Result<Response, ChatError> {
        let user = match split_person(person) {
            Some((of, user)) if of == account => user,
            _ => return Err(ChatError::NotFound),
        };
        {
            let mut added = self.added();
            if let Some(list) = added.accounts.get_mut(account) {
                list.retain(|held| held.id != user);
            }
            added.accounts.retain(|_, list| !list.is_empty());
            self.save(&added)?;
        }
        self.forget_held(account);
        Ok(Response::Ok)
    }

    /// Drops the list held of `account`, so the next look asks again.
    fn forget_held(&self, account: &str) {
        if let Some(held) = self.held().get_mut(account) {
            held.at = None;
        }
    }

    fn save(&self, added: &Added) -> Result<(), ChatError> {
        let Some(file) = &self.file else { return Ok(()) };
        write_added(file, added).map_err(|err| {
            eprintln!("torroworkplace: presence: cannot keep the list of people: {err}");
            ChatError::Other(tr("The list of people could not be saved.").to_string())
        })
    }

    async fn own(&self, account: &str, heartbeat: bool) -> Result<OwnStatus, ChatError> {
        let (remote, caps, _) = self.chat.known(account).await?;
        if !caps.status {
            return Err(no_status());
        }
        let status = self.chat.asked(account, remote.own_status().await).await?;
        Ok(OwnStatus {
            account: account.to_string(),
            name: self.chat.own_name(account).unwrap_or_default(),
            status: status.status,
            message: status.message,
            icon: status.icon,
            clear_at: status.clear_at,
            heartbeat,
        })
    }

    async fn set_own(
        &self,
        account: &str,
        status: Option<Status>,
        message: Option<&str>,
        icon: Option<&str>,
        clear_at: Option<DateTime<Utc>>,
        heartbeat: bool,
    ) -> Result<Response, ChatError> {
        match status {
            // The server drops "online" after minutes unless the computer keeps telling it.
            Some(Status::Online) if !heartbeat => {
                return Err(unsupported("Turn on “Show others when I am online” first."));
            }
            Some(Status::Offline) => return Err(unsupported("Use “invisible” to appear offline.")),
            _ => {}
        }
        if message.is_some_and(|message| message.chars().count() > MAX_STATUS_MESSAGE) {
            return Err(ChatError::Other(tr("A status message has at most 80 characters.").to_string()));
        }
        if message.is_some_and(|message| message.chars().any(char::is_control)) {
            return Err(ChatError::Other(tr("A status message is one line.").to_string()));
        }
        let (remote, caps, _) = self.chat.known(account).await?;
        if !caps.status {
            return Err(no_status());
        }
        if let Some(status) = status {
            self.chat.asked(account, remote.set_status(status).await).await?;
        }
        if let Some(message) = message {
            let message = message.trim();
            let set = match message.is_empty() {
                true => remote.set_status_message(None, None, None).await,
                false => remote.set_status_message(Some(message), icon, clear_at).await,
            };
            self.chat.asked(account, set).await?;
        }
        Ok(Response::Ok)
    }

    /// One heartbeat: tells every account's server that the person is at the computer. Called by
    /// the core every [`BEAT_EVERY`] while "Show others when I am online" is on; never otherwise.
    /// An account whose server tells no status or cannot be reached is skipped.
    pub async fn beat(&self, status: Status) {
        for account in self.chat.accounts().await {
            let Ok((remote, caps, _)) = self.chat.known(&account).await else { continue };
            if caps.status
                && let Err(err) = self.chat.asked(&account, remote.heartbeat(status).await).await
            {
                eprintln!("torroworkplace: presence: heartbeat of {account} failed: {}", code(&err));
            }
        }
    }

    /// Drops everything of `account`: the list held and the people added by hand (account removed
    /// or wiped).
    pub fn forget_account(&self, account: &str) {
        self.held().remove(account);
        let mut added = self.added();
        if added.accounts.remove(account).is_some() {
            // Nowhere to report a failed write to; the entry is harmless without its account.
            let _ = self.save(&added);
        }
    }
}

/// A word for `err` that names no person and no text, for the log.
fn code(err: &ChatError) -> &'static str {
    match err {
        ChatError::Auth => "auth",
        ChatError::Offline => "offline",
        ChatError::NotConnected => "not-connected",
        ChatError::NotFound => "not-found",
        ChatError::Unsupported(_) => "unsupported",
        ChatError::Forbidden(_) => "forbidden",
        ChatError::Busy => "busy",
        ChatError::Pending | ChatError::Other(_) => "error",
    }
}

fn unsupported(text: &'static str) -> ChatError {
    ChatError::Unsupported(tr(text).to_string())
}

fn no_status() -> ChatError {
    unsupported("Your Nextcloud does not share status.")
}

/// The list in `file`; empty when it is missing or unreadable (the list is a convenience, never a
/// reason not to start).
fn read_added(file: &Path) -> Added {
    std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Writes the list to `file`, for the person alone (0600): it names people.
fn write_added(file: &Path, added: &Added) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(added).map_err(std::io::Error::other)?;
    std::fs::write(file, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use torro_ipc::api::{Rights, RightsTemplate, Selection};

    use super::*;
    use crate::chat::fake::Server;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";

    fn service(dir: &Path) -> (PresenceService, Arc<Server>) {
        let server = Arc::new(Server::default());
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        (PresenceService::in_dir(chat, dir), server)
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "",
        }
    }

    async fn people(presence: &PresenceService) -> Vec<Person> {
        let request = Request::PresenceListPeople { account: Some(ACCOUNT.into()) };
        match presence.handle(request, Reach::Suite, None, false).await {
            Response::PresencePeople { people } => people,
            other => panic!("no people: {other:?}"),
        }
    }

    fn set(status: Option<Status>, message: Option<&str>) -> Request {
        Request::PresenceSetOwnStatus {
            account: ACCOUNT.into(),
            status,
            message: message.map(str::to_string),
            icon: None,
            clear_at: None,
        }
    }

    #[tokio::test]
    async fn the_list_names_who_the_core_knows_with_their_status() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, server) = service(dir.path());
        // The address book names Cem and the person themselves; the direct chat names Ben.
        let presence = presence.with_names(Arc::new(|_| {
            vec![
                RemotePerson { id: "cem".into(), name: "Cem Demir".into() },
                RemotePerson { id: crate::chat::fake::ME.into(), name: "Anna Berg".into() },
            ]
        }));
        let listed = people(&presence).await;
        let shown: Vec<(&str, Status, bool)> =
            listed.iter().map(|person| (person.name.as_str(), person.status, person.unknown)).collect();
        assert_eq!(shown, [("Ben Koch", Status::Online, false), ("Cem Demir", Status::Away, false)]);
        assert_eq!(listed[0].id, "nextcloud:anna@cloud.example.org/ben");
        assert_eq!(listed[0].message.as_deref(), Some("In the office"));
        assert!(listed.iter().all(|person| person.untrusted && !person.added));
        people(&presence).await;
        assert_eq!(server.count("statuses"), 1, "a fresh list is read from memory");
    }

    #[tokio::test]
    async fn a_server_that_lists_nobody_is_asked_one_by_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, server) = service(dir.path());
        let ben = server.state().statuses.remove(0);
        server.state().statuses.clear();
        let listed = people(&presence).await;
        assert_eq!(listed.len(), 1, "Ben, from the direct chat");
        assert_eq!((listed[0].status, listed[0].unknown), (Status::Offline, false), "asked: he has no status");
        assert_eq!(server.count("status_of ben"), 1);
        // "Invisible" never shows to others, whatever the server sends.
        let mut hidden = ben;
        hidden.status = Status::Invisible;
        server.state().statuses.push(hidden);
        presence.forget_held(ACCOUNT);
        assert_eq!(people(&presence).await[0].status, Status::Offline);
    }

    #[tokio::test]
    async fn people_are_added_by_the_servers_name_and_kept_across_restarts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, server) = service(dir.path());
        let add = |user: &str| Request::PresenceAddPerson { account: ACCOUNT.into(), person: person_id(ACCOUNT, user) };
        assert_eq!(presence.handle(add("dana"), Reach::Suite, None, false).await, Response::Ok);
        assert_eq!(code(&presence.handle(add("nobody"), Reach::Suite, None, false).await), "not-found");
        let dana = people(&presence).await.into_iter().find(|person| person.name == "Dana Erb").expect("Dana");
        assert!(dana.added, "she can be removed again");
        // A new service on the same directory still has her.
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let again = PresenceService::in_dir(chat, dir.path());
        assert!(people(&again).await.iter().any(|person| person.name == "Dana Erb"));
        let remove = Request::PresenceRemovePerson { account: ACCOUNT.into(), person: dana.id.clone() };
        assert_eq!(again.handle(remove, Reach::Suite, None, false).await, Response::Ok);
        assert!(!people(&again).await.iter().any(|person| person.name == "Dana Erb"));
        again.forget_account(ACCOUNT);
        assert_eq!(read_added(&dir.path().join(FILE)), Added::default());
    }

    #[tokio::test]
    async fn the_own_status_is_set_part_by_part() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, server) = service(dir.path());
        let app = |request| presence.handle(request, Reach::Suite, None, false);
        assert_eq!(code(&app(set(Some(Status::Online), None)).await), "unsupported", "online needs the heartbeat");
        assert_eq!(code(&app(set(Some(Status::Offline), None)).await), "unsupported", "offline is invisible");
        assert_eq!(app(set(Some(Status::Dnd), None)).await, Response::Ok);
        assert_eq!(app(set(None, Some("In a meeting"))).await, Response::Ok);
        // One look at the server: its state sits behind one lock.
        let own = server.state().own.clone();
        assert_eq!((own.status, own.message.as_deref()), (Status::Dnd, Some("In a meeting")));
        let Response::PresenceOwnStatus { status } =
            app(Request::PresenceGetOwnStatus { account: ACCOUNT.into() }).await
        else {
            panic!("the own status")
        };
        assert_eq!((status.status, status.name.as_str(), status.heartbeat), (Status::Dnd, "Anna Berg", false));
        assert_eq!(app(set(None, Some(""))).await, Response::Ok);
        assert_eq!(server.state().own.message, None, "an empty message clears it");
        assert_eq!(code(&app(set(None, Some(&"x".repeat(81)))).await), "error");
        assert_eq!(code(&app(set(None, Some("two\nlines"))).await), "error");
        // With the heartbeat on, "online" holds.
        assert_eq!(presence.handle(set(Some(Status::Online), None), Reach::Suite, None, true).await, Response::Ok);
        server.state().caps.status = false;
        let chat = Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))]));
        let without = PresenceService::in_dir(chat, dir.path());
        let answer = without.handle(set(Some(Status::Dnd), None), Reach::Suite, None, false).await;
        assert_eq!(code(&answer), "unsupported", "a server without user status");
    }

    #[tokio::test]
    async fn a_client_reaches_only_its_presence_accounts_and_is_throttled() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, _) = service(dir.path());
        let mut rights = RightsTemplate::FullAccess.rights();
        rights.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        let own = Request::PresenceGetOwnStatus { account: ACCOUNT.into() };
        assert!(presence.out_of_reach(Reach::Client(&rights), &own).is_some(), "the chats do not open presence");
        rights.presence_accounts = Selection::Only(vec![ACCOUNT.into()]);
        assert_eq!(presence.out_of_reach(Reach::Client(&rights), &own), None);
        let foreign = Request::PresenceAddPerson {
            account: ACCOUNT.into(),
            person: person_id("nextcloud:anna@other.example.org", "ben"),
        };
        assert!(presence.out_of_reach(Reach::Client(&rights), &foreign).is_some());
        let none = Rights::none();
        let all = Request::PresenceListPeople { account: None };
        let listed = presence.handle(all, Reach::Client(&none), Some("agent"), false).await;
        assert_eq!(listed, Response::PresencePeople { people: Vec::new() }, "no account in reach: nobody");
        for _ in 0..crate::chat::throttle::PER_MINUTE {
            let answer = presence.handle(set(Some(Status::Away), None), Reach::Client(&rights), Some("agent"), false);
            assert_eq!(answer.await, Response::Ok);
        }
        let sixth = presence.handle(set(Some(Status::Away), None), Reach::Client(&rights), Some("agent"), false);
        assert_eq!(code(&sixth.await), "busy");
    }

    #[tokio::test]
    async fn the_heartbeat_reaches_every_account_that_tells_status() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (presence, server) = service(dir.path());
        presence.beat(Status::Online).await;
        assert_eq!(server.state().beats, [Status::Online]);
        assert_eq!(server_of(ACCOUNT), "cloud.example.org");
    }
}
