//! Reading mail (task M3.1): the handlers of `mail.list_folders`, `mail.search_messages`,
//! `mail.get_message`, `mail.get_thread` and `mail.get_attachment`. The core sends these requests
//! here ([`answer`]) from `Core::handle_within`, with the caller's reach.
//!
//! **Reach.** A client sees only the accounts its rights name (`Rights::reaches_mail_account`).
//! An account a request names is checked in the core before anything else (`mail_out_of_reach`);
//! folder and message ids (`f:…`, `m:…`) hold no account, so they are looked up here in the stores
//! of the reachable accounts only ([`MailService::stores`]), one after the other. An id no
//! reachable store knows is `not-found` for the whole call, before anything is read or fetched:
//! the same answer as for an id that does not exist. Reading never waits for an approval
//! (`Access::Read`), so this check is the one before the handler runs, also for an approved run
//! (`Core::run_approved` ends in `handle_within` as well).
//!
//! **Cache first.** The store holds the headers of every message and the texts of the last
//! [`BODY_RETENTION_DAYS`] days. A message without its text is fetched from the server with
//! `BODY.PEEK[]` (never marked read) and its text kept with `MailStore::set_body`.
//!
//! **Search** ([`search_messages`]): words match subject, sender, recipients, text and attachment
//! names in the store (FTS5). Words in older texts are only on the server: when a search with
//! words reaches back before the text cutoff, its older part is asked with `UID SEARCH TEXT …`
//! ([`Session::uid_search_text`]) in every folder in scope. Pages come newest first: first the
//! store's (`source: cache`, from the cutoff on), then the server's (`source: live`, before it);
//! a page never mixes the two. The `cursor` says which part and how far (`c:<n>`, `l:<n>`, at
//! most [`CURSOR_MAX`]). The server's part finds only messages the sync has stored, and leaves
//! out accounts whose server cannot be reached: it may be incomplete.
//!
//! **Attachments** go to the managed folder (`AttachmentCache`), at most
//! [`ATTACHMENT_MAX_BYTES`], under a cleaned file name, so a name from outside never picks the
//! path.
//!
//! **Log.** Nothing here writes a search term, folder name, file name or text anywhere; the
//! core's audit log names accounts and hashed ids only (`audit::ids`).

use chrono::{DateTime, Duration, NaiveDate, Utc};
use torro_core::i18n::tr;
use torro_ipc::Response;
use torro_ipc::mail::MailSource;
use torro_ipc::{Request, mail::limit_refusal};
use torro_mail::{Message, MessageSummary};
use torro_mail_imap::imap::Session;
use torro_mail_imap::mime::{self, MimeError};

use super::MailStoreError;
use super::service::{MailError, MailService, SharedStore};
use super::store::{BODY_RETENTION_DAYS, Body, FolderRef, MessageRef, SearchQuery};
use crate::pim::Reach;

/// The largest attachment `mail.get_attachment` hands out: 50 MiB.
pub const ATTACHMENT_MAX_BYTES: usize = 50 * 1024 * 1024;

/// At most this many messages of one conversation (`mail.get_thread`), the newest: a thread made
/// up by a sender must not make the core fetch thousands of texts.
pub const THREAD_MAX: usize = 100;

/// A failed request, already as the answer to send.
type Answer<T> = Result<T, Response>;

/// The stores a caller sees: account id and store, in the order of `accounts.json`.
type Stores = Vec<(String, SharedStore)>;

/// Answers a reading mail request for `reach` (`mail.list_folders`, `mail.search_messages`,
/// `mail.get_message`, `mail.get_thread`, `mail.get_attachment`). The limits are checked first
/// ([`limit_refusal`]), also for the app key. Any other request is `unsupported`.
pub async fn answer(mail: &MailService, reach: Reach<'_>, request: Request) -> Response {
    if let Some(refused) = limit_refusal(&request) {
        return refused;
    }
    let answered = match request {
        Request::MailFolders { account } => list_folders(mail, reach, account.as_deref()).await,
        Request::SearchMail { account, folder, query, unread, flagged, since, before, limit, cursor } => {
            let search = Search { account, folder, query, unread, flagged, since, before, limit, cursor };
            search_messages(mail, reach, search, Utc::now()).await
        }
        Request::GetMail { id, include_body } => get_message(mail, reach, &id, include_body).await,
        Request::GetMailThread { id } => get_thread(mail, reach, &id).await,
        Request::GetMailAttachment { id, attachment } => get_attachment(mail, reach, &id, &attachment).await,
        other => {
            let message = format!("{} is not a reading mail command", other.operation().name());
            Err(Response::Error { code: "unsupported".into(), message })
        }
    };
    answered.unwrap_or_else(|refused| refused)
}

// ----- mail.list_folders -----

/// The folders of every account `reach` sees, or of `account` alone.
async fn list_folders(mail: &MailService, reach: Reach<'_>, account: Option<&str>) -> Answer<Response> {
    let mut folders = Vec::new();
    for (_, store) in scoped(mail, reach, account).await? {
        folders.extend(store.run(|store| store.folders()).await.map_err(store_failed)?);
    }
    Ok(Response::MailFolders { folders })
}

/// The stores of the accounts `reach` sees; only `account`'s when it is given (`not-found` when
/// it is no mail account `reach` sees, the account's own error when its store is not open).
async fn scoped(mail: &MailService, reach: Reach<'_>, account: Option<&str>) -> Answer<Stores> {
    let mut stores = mail.stores(reach).await;
    let Some(account) = account else { return Ok(stores) };
    stores.retain(|(id, _)| id == account);
    if !stores.is_empty() {
        return Ok(stores);
    }
    let reached = match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.reaches_mail_account(account),
    };
    // A reachable account that is listed but has no store says why; any other is none.
    match (reached, mail.store(account).await) {
        (true, Err(err)) => Err(err.response()),
        _ => Err(MailError::NotFound.response()),
    }
}

// ----- mail.search_messages -----

/// What `mail.search_messages` asks for: the fields of `Request::SearchMail`, documented there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// Only this account (`mail:<address>`).
    pub account: Option<String>,
    /// Only this folder (`f:…`).
    pub folder: Option<String>,
    /// Words that must all match; blank counts as none.
    pub query: Option<String>,
    /// Only unread (`true`) or read (`false`) messages.
    pub unread: Option<bool>,
    /// Only flagged (`true`) or unflagged (`false`) messages.
    pub flagged: Option<bool>,
    /// Sent at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Sent before this instant.
    pub before: Option<DateTime<Utc>>,
    /// Page size, 1 to 100 (checked by `limit_refusal`).
    pub limit: u32,
    /// Where the page starts, as the previous page handed it out.
    pub cursor: Option<String>,
}

/// How deep a search pages into each of its parts: a page starts at most at the 10 000th match.
/// Paging deeper would make every page read that many rows; narrow the search instead.
pub const CURSOR_MAX: u32 = 10_000;

/// Where a page starts: the n-th match of the store's part or of the server's part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Cache(u32),
    Live(u32),
}

impl Cursor {
    /// No text: the first page. `None` for a text this search does not hand out, or one deeper
    /// than [`CURSOR_MAX`].
    fn parse(text: Option<&str>) -> Option<Cursor> {
        let Some(text) = text else { return Some(Cursor::Cache(0)) };
        let cursor = match text.split_once(':')? {
            ("c", n) => n.parse().ok().map(Cursor::Cache),
            ("l", n) => n.parse().ok().map(Cursor::Live),
            _ => None,
        }?;
        match cursor {
            Cursor::Cache(n) | Cursor::Live(n) if n > CURSOR_MAX => None,
            cursor => Some(cursor),
        }
    }

    fn text(self) -> String {
        match self {
            Cursor::Cache(n) => format!("c:{n}"),
            Cursor::Live(n) => format!("l:{n}"),
        }
    }
}

/// The time range whose texts only the server has: from `since` (or the beginning) to `until`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Older {
    since: Option<DateTime<Utc>>,
    until: DateTime<Utc>,
}

/// One page of a search as of `now`, newest first: the store's matches from the text cutoff
/// (`now` minus [`BODY_RETENTION_DAYS`]) on, then, for a search with words that reaches back
/// before it, the server's matches before the cutoff ([`live_matches`]). Without words the store
/// answers alone: it holds the headers of every message.
///
/// `not-found` for a `folder` no reachable store knows (or one outside `account`), `error` for a
/// cursor this search did not hand out.
pub async fn search_messages(
    mail: &MailService,
    reach: Reach<'_>,
    search: Search,
    now: DateTime<Utc>,
) -> Answer<Response> {
    let mut stores = scoped(mail, reach, search.account.as_deref()).await?;
    let mut folder = None;
    if let Some(id) = &search.folder {
        let (account, store, found) = locate_folder(&stores, id).await?.ok_or_else(no_folder)?;
        stores = vec![(account, store)];
        folder = Some(found);
    }
    let cursor = Cursor::parse(search.cursor.as_deref()).ok_or_else(bad_cursor)?;
    let words = search.query.as_deref().map(str::trim).filter(|words| !words.is_empty()).map(str::to_string);
    let cutoff = now - Duration::days(BODY_RETENTION_DAYS);
    let older = words.as_ref().and_then(|_| {
        let until = search.before.map_or(cutoff, |before| before.min(cutoff));
        search.since.is_none_or(|since| since < until).then_some(Older { since: search.since, until })
    });
    // With a part on the server, the store answers only from the cutoff on, so the parts meet
    // without overlap; a range that ends before the cutoff leaves the store nothing.
    let cache_since = match older {
        Some(_) => Some(search.since.map_or(cutoff, |since| since.max(cutoff))),
        None => search.since,
    };
    let cache_empty = older.is_some() && search.before.is_some_and(|before| before <= cutoff);
    let query = SearchQuery {
        folder: folder.as_ref().map(|folder| folder.id.clone()),
        text: words.clone(),
        unread: search.unread,
        flagged: search.flagged,
        since: cache_since,
        before: search.before,
    };
    let limit = search.limit as usize;

    if let Cursor::Cache(offset) = cursor {
        let page = if cache_empty { Vec::new() } else { cache_page(&stores, &query, offset, limit + 1).await? };
        let more = page.len() > limit;
        let messages: Vec<MessageSummary> = page.into_iter().take(limit).collect();
        let deeper = offset.saturating_add(search.limit);
        let next = match (more && deeper <= CURSOR_MAX, older) {
            (true, _) => Some(Cursor::Cache(deeper)),
            (false, Some(_)) if !messages.is_empty() => Some(Cursor::Live(0)),
            (false, _) => None,
        };
        // An exhausted store part goes on with the server's part at once, not with an empty page.
        if !messages.is_empty() || older.is_none() {
            let cursor = next.map(Cursor::text);
            return Ok(Response::MailMessages { messages, cursor, source: MailSource::Cache });
        }
    }
    let (Some(words), Some(older)) = (words, older) else {
        return Ok(Response::MailMessages { messages: Vec::new(), cursor: None, source: MailSource::Live });
    };
    let offset = match cursor {
        Cursor::Live(offset) => offset as usize,
        Cursor::Cache(_) => 0,
    };
    let filter = Filter { older, unread: search.unread, flagged: search.flagged };
    let found = live_matches(mail, &stores, folder.as_ref(), &words, &filter).await?;
    let more = found.len() > offset.saturating_add(limit);
    let messages: Vec<MessageSummary> = found.into_iter().skip(offset).take(limit).collect();
    let deeper = u32::try_from(offset + limit).unwrap_or(u32::MAX);
    let cursor = (more && deeper <= CURSOR_MAX).then(|| Cursor::Live(deeper).text());
    Ok(Response::MailMessages { messages, cursor, source: MailSource::Live })
}

/// The store's matches `offset` to `offset + count` over all `stores`, newest first. Each store
/// gives its first `offset + count`, so pages of several accounts interleave by date.
async fn cache_page(
    stores: &[(String, SharedStore)],
    query: &SearchQuery,
    offset: u32,
    count: usize,
) -> Answer<Vec<MessageSummary>> {
    let wanted = offset.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    let mut all = Vec::new();
    for (_, store) in stores {
        let query = query.clone();
        all.extend(store.run(move |store| store.search(&query, wanted, 0)).await.map_err(store_failed)?);
    }
    // Stable: the same date keeps the account order and each store's own order.
    all.sort_by_key(|summary| std::cmp::Reverse(summary.date));
    Ok(all.into_iter().skip(offset as usize).take(count).collect())
}

/// What a message found on the server must also hold to be a match.
#[derive(Debug, Clone, Copy)]
struct Filter {
    older: Older,
    unread: Option<bool>,
    flagged: Option<bool>,
}

impl Filter {
    fn holds(&self, summary: &MessageSummary) -> bool {
        let seen = summary.flags.contains(&torro_mail::Flag::Seen);
        let flagged = summary.flags.contains(&torro_mail::Flag::Flagged);
        self.older.since.is_none_or(|since| summary.date >= since)
            && summary.date < self.older.until
            && self.unread.is_none_or(|unread| unread != seen)
            && self.flagged.is_none_or(|wanted| wanted == flagged)
    }
}

/// Every match of the server's part, newest first: `UID SEARCH TEXT` in each folder in scope
/// (`folder`, else every folder the store knows), the UIDs found mapped to the store's messages
/// and checked against `filter` with what the store knows (date, flags). Messages the sync has not
/// stored yet are left out: they have no id yet, a later search finds them.
///
/// An account whose server cannot be reached is left out, so the server's part may be incomplete
/// (the answer does not say so; the catalogue's description of `mail.search_messages` does). When
/// no account could be asked, the first error is the answer.
async fn live_matches(
    mail: &MailService,
    stores: &[(String, SharedStore)],
    folder: Option<&FolderRef>,
    words: &str,
    filter: &Filter,
) -> Answer<Vec<MessageSummary>> {
    // A day more on each side: the server compares its own receiving day, the store the `Date:`
    // header; the filter afterwards is exact.
    let since_day = filter.older.since.map(|since| since.date_naive() - Duration::days(1));
    let until_day = filter.older.until.date_naive() + Duration::days(2);
    let mut found = Vec::new();
    let mut failure = None;
    let mut asked = false;
    for (account, store) in stores {
        let folders = match folder {
            Some(folder) => vec![folder.clone()],
            None => store.run(known_folders).await.map_err(store_failed)?,
        };
        if folders.is_empty() {
            continue;
        }
        let uids = match search_folders(mail, account, &folders, words, since_day, until_day).await {
            Ok(uids) => uids,
            Err(err) => {
                failure.get_or_insert(err);
                continue;
            }
        };
        asked = true;
        let filter = *filter;
        let matches = store
            .run(move |store| {
                let mut matches = Vec::new();
                for (folder, uids) in &uids {
                    for uid in uids {
                        let Some(id) = store.message_id_for_uid(folder, *uid)? else { continue };
                        matches.extend(store.summary(&id)?.filter(|summary| filter.holds(summary)));
                    }
                }
                Ok(matches)
            })
            .await
            .map_err(store_failed)?;
        found.extend(matches);
    }
    if let (false, Some(err)) = (asked, failure) {
        return Err(err.response());
    }
    found.sort_by_key(|summary| std::cmp::Reverse(summary.date));
    Ok(found)
}

/// The folders of a store that a sync has opened (they have a UIDVALIDITY, so their UIDs mean
/// something).
fn known_folders(store: &super::store::MailStore) -> Result<Vec<FolderRef>, MailStoreError> {
    let mut known = Vec::new();
    for folder in store.folders()? {
        known.extend(store.resolve_folder(&folder.id)?.filter(|found| found.uidvalidity.is_some()));
    }
    Ok(known)
}

/// `UID SEARCH TEXT` in each of `folders` of `account` on its command connection; per folder its
/// id and the UIDs found. A folder gone from the server or renumbered since the last sync finds
/// nothing. The connection is released only when every command worked.
async fn search_folders(
    mail: &MailService,
    account: &str,
    folders: &[FolderRef],
    words: &str,
    since: Option<NaiveDate>,
    until: NaiveDate,
) -> Result<Vec<(String, Vec<u32>)>, MailError> {
    let mut command = mail.command(account).await?;
    let mut found = Vec::new();
    for folder in folders {
        let selected = match command.session().select(&folder.path).await {
            Ok(selected) => selected,
            Err(torro_mail_imap::imap::ImapError::NotFound(_)) => continue,
            Err(err) => return Err(err.into()),
        };
        if Some(selected.uid_validity) != folder.uidvalidity {
            continue;
        }
        let uids = command.session().uid_search_text(words, since, Some(until)).await?;
        found.push((folder.id.clone(), uids));
    }
    command.release();
    Ok(found)
}

/// The store that knows the folder `id`, with the folder.
async fn locate_folder(stores: &[(String, SharedStore)], id: &str) -> Answer<Option<(String, SharedStore, FolderRef)>> {
    for (account, store) in stores {
        let lookup = id.to_string();
        if let Some(found) = store.run(move |store| store.resolve_folder(&lookup)).await.map_err(store_failed)? {
            return Ok(Some((account.clone(), store.clone(), found)));
        }
    }
    Ok(None)
}

// ----- mail.get_message, mail.get_thread -----

/// A message found in a reachable store: its account, the store and where it is on the server.
struct Located {
    account: String,
    store: SharedStore,
    place: MessageRef,
}

/// The store among `stores` that knows the message `id`; `not-found` when none does.
async fn locate(stores: &[(String, SharedStore)], id: &str) -> Answer<Located> {
    for (account, store) in stores {
        let lookup = id.to_string();
        if let Some(place) = store.run(move |store| store.resolve_message(&lookup)).await.map_err(store_failed)? {
            return Ok(Located { account: account.clone(), store: store.clone(), place });
        }
    }
    Err(no_message())
}

/// `mail.get_message`: the message from the store. Its text (and so its attachments) is fetched
/// from the server when the store lacks it and it is asked for, or the message has attachments
/// to list; never marked read.
async fn get_message(mail: &MailService, reach: Reach<'_>, id: &str, include_body: bool) -> Answer<Response> {
    let stores = mail.stores(reach).await;
    let located = locate(&stores, id).await?;
    let mut message = stored_message(&located).await?;
    if message.text.is_none() && (include_body || message.summary.has_attachments) {
        message = fetch_body(mail, &located).await?;
    }
    if !include_body {
        message.text = None;
    }
    Ok(Response::MailMessage { message })
}

/// `mail.get_thread`: the conversation of the message `id` in every account `reach` sees, oldest
/// first, each message with its text: the store's, else fetched over one command connection per
/// account. At most the newest [`THREAD_MAX`] messages. A text that cannot be fetched (server
/// offline, message gone) leaves that message without `text`; the rest is answered anyway.
async fn get_thread(mail: &MailService, reach: Reach<'_>, id: &str) -> Answer<Response> {
    let stores = mail.stores(reach).await;
    let located = locate(&stores, id).await?;
    let lookup = id.to_string();
    let thread = located
        .store
        .run(move |store| Ok(store.summary(&lookup)?.map(|summary| summary.thread_id)))
        .await
        .map_err(store_failed)?
        .ok_or_else(no_message)?;
    let mut members: Vec<(usize, Message)> = Vec::new();
    for (at, (_, store)) in stores.iter().enumerate() {
        let thread = thread.clone();
        let found = store
            .run(move |store| {
                let mut messages = Vec::new();
                for summary in store.thread_newest(&thread, THREAD_MAX as u32)? {
                    messages.extend(store.message(&summary.id)?);
                }
                Ok(messages)
            })
            .await
            .map_err(store_failed)?;
        members.extend(found.into_iter().map(|message| (at, message)));
    }
    members.sort_by_key(|(_, message)| message.summary.date);
    let skip = members.len().saturating_sub(THREAD_MAX);
    let mut members: Vec<(usize, Message)> = members.into_iter().skip(skip).collect();
    for (at, (account, store)) in stores.iter().enumerate() {
        let missing: Vec<usize> =
            (0..members.len()).filter(|&index| members[index].0 == at && members[index].1.text.is_none()).collect();
        if !missing.is_empty() {
            fill_texts(mail, account, store, &mut members, &missing).await;
        }
    }
    Ok(Response::MailThread { messages: members.into_iter().map(|(_, message)| message).collect() })
}

/// Fetches the texts of `members[missing]`, all of the account `account`, over its command
/// connection (`BODY.PEEK[]`) and keeps them in the store. One that cannot be read or is gone
/// stays without text; a failing connection ends the fetching (it is dropped, not released).
async fn fill_texts(
    mail: &MailService,
    account: &str,
    store: &SharedStore,
    members: &mut [(usize, Message)],
    missing: &[usize],
) {
    let Ok(mut command) = mail.command(account).await else { return };
    for &index in missing {
        let id = members[index].1.summary.id.clone();
        let lookup = id.clone();
        let Ok(Some(place)) = store.run(move |store| store.resolve_message(&lookup)).await else { continue };
        let raw = match peek(command.session(), &place).await {
            Ok(Some(raw)) => raw,
            Ok(None) => continue,
            Err(_) => return,
        };
        let Ok(read) = mime::read(&raw) else { continue };
        let body = Body { text: read.text, from_html: read.from_html, attachments: read.attachments };
        let kept = store.run(move |store| {
            store.set_body(&id, &body)?;
            store.message(&id)
        });
        if let Ok(Some(message)) = kept.await {
            members[index].1 = message;
        }
    }
    command.release();
}

/// The message as the store has it; `not-found` when the sync dropped it meanwhile.
async fn stored_message(located: &Located) -> Answer<Message> {
    let id = located.place.id.clone();
    located.store.run(move |store| store.message(&id)).await.map_err(store_failed)?.ok_or_else(no_message)
}

/// Fetches the whole message from the server (`BODY.PEEK[]`, so it stays unread), keeps its text
/// and attachment list in the store and returns the message as the store has it then.
async fn fetch_body(mail: &MailService, located: &Located) -> Answer<Message> {
    let raw = fetch_raw(mail, located).await?;
    let read = mime::read(&raw).map_err(|_| unreadable())?;
    let id = located.place.id.clone();
    let body = Body { text: read.text, from_html: read.from_html, attachments: read.attachments };
    located
        .store
        .run(move |store| {
            store.set_body(&id, &body)?;
            store.message(&id)
        })
        .await
        .map_err(store_failed)?
        .ok_or_else(no_message)
}

/// The raw message from the account's command connection. `not-found` when it is gone from the
/// server or the folder was renumbered since the store took it.
async fn fetch_raw(mail: &MailService, located: &Located) -> Answer<Vec<u8>> {
    let mut command = mail.command(&located.account).await.map_err(|err| err.response())?;
    let raw = peek(command.session(), &located.place).await.map_err(|err| err.response())?;
    command.release();
    raw.ok_or_else(no_message)
}

/// `BODY.PEEK[]` of the message at `place`, opening its folder first unless it is open.
async fn peek(session: &mut Session, place: &MessageRef) -> Result<Option<Vec<u8>>, MailError> {
    let uid_validity = match session.selected() {
        Some(selected) if selected.path == place.folder_path => selected.uid_validity,
        _ => session.select(&place.folder_path).await?.uid_validity,
    };
    if uid_validity != place.uidvalidity {
        return Ok(None);
    }
    Ok(session.fetch_message(place.uid).await?)
}

// ----- mail.get_attachment -----

/// `mail.get_attachment`: fetches the message, takes the attachment `part` out of it and stores
/// it in the managed folder under its cleaned name. `not-found` for a part that is no attachment,
/// `error` for one over [`ATTACHMENT_MAX_BYTES`] (refused before fetching when the store knows its
/// size).
async fn get_attachment(mail: &MailService, reach: Reach<'_>, id: &str, part: &str) -> Answer<Response> {
    let stores = mail.stores(reach).await;
    let located = locate(&stores, id).await?;
    let message = stored_message(&located).await?;
    if message.text.is_some() {
        match message.attachments.iter().find(|attachment| attachment.part == part) {
            None => return Err(no_attachment()),
            Some(known) if known.size > ATTACHMENT_MAX_BYTES as u64 => return Err(too_big()),
            Some(_) => {}
        }
    }
    let cache = mail.attachments().ok_or_else(no_attachment_folder)?;
    let raw = fetch_raw(mail, &located).await?;
    let attachment = mime::attachment(&raw, part).map_err(|err| match err {
        MimeError::NoSuchPart => no_attachment(),
        _ => unreadable(),
    })?;
    if attachment.bytes.len() > ATTACHMENT_MAX_BYTES {
        return Err(too_big());
    }
    let name = attachment.filename.clone().unwrap_or_default();
    let bytes = attachment.bytes;
    let size = bytes.len() as u64;
    let path = tokio::task::spawn_blocking(move || cache.store(&name, &bytes))
        .await
        .map_err(|_| no_attachment_folder())?
        .map_err(store_failed)?;
    Ok(Response::MailAttachment {
        path: path.to_string_lossy().into_owned(),
        size,
        content_type: attachment.content_type,
    })
}

// ----- answers -----

fn store_failed(err: MailStoreError) -> Response {
    MailError::Store(err).response()
}

fn not_found(message: &str) -> Response {
    Response::Error { code: "not-found".into(), message: message.into() }
}

fn failed(message: &str) -> Response {
    Response::Error { code: "error".into(), message: message.into() }
}

fn no_message() -> Response {
    not_found(tr("No such message."))
}

fn no_folder() -> Response {
    not_found(tr("No such folder."))
}

fn no_attachment() -> Response {
    not_found(tr("The message has no such attachment."))
}

fn too_big() -> Response {
    failed(tr("The attachment is larger than 50 MiB."))
}

fn unreadable() -> Response {
    failed(tr("The message could not be read."))
}

fn bad_cursor() -> Response {
    failed(tr("This cursor is not valid. Search again without it."))
}

fn no_attachment_folder() -> Response {
    failed(tr("Attachments cannot be kept on this device."))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;
    use torro_ipc::api::{RightsTemplate, Selection};
    use torro_mail::{Flag, Folder, Role};

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::{self, FakeImap};

    const ANNA: &str = "mail:anna@example.org";

    /// The service with Anna's account on `server`, synced once.
    async fn synced(dir: &Path, server: &FakeImap) -> MailService {
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir, &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir, &secrets);
        service.reload().await;
        service.sync_all(Reach::Suite).await.expect("synced");
        service
    }

    /// A mail `days_ago` days old with `subject` and the text `body`.
    fn mail_with(subject: &str, days_ago: i64, body: &str) -> String {
        let date = (Utc::now() - Duration::days(days_ago)).to_rfc2822();
        let id = subject.replace(' ', "-");
        format!(
            "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: {subject}\r\nDate: {date}\r\n\
             Message-ID: <{id}@example.org>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
        )
    }

    /// A recent mail with one attachment whose name tries to leave the folder.
    fn mail_with_attachment() -> String {
        let date = Utc::now().to_rfc2822();
        format!(
            "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: Report\r\nDate: {date}\r\n\
             Message-ID: <report@example.org>\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nSee attached.\r\n--b1\r\n\
             Content-Type: text/plain; name=\"evil.txt\"\r\n\
             Content-Disposition: attachment; filename=\"../../evil.txt\"\r\n\r\nhello attachment\r\n--b1--\r\n"
        )
    }

    fn search(query: Option<&str>, limit: u32, cursor: Option<String>) -> Request {
        Request::SearchMail {
            account: None,
            folder: None,
            query: query.map(str::to_string),
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor,
        }
    }

    /// A first page of ten for `query` in `folder` from `since` on.
    fn find(query: Option<&str>, folder: Option<String>, since: Option<DateTime<Utc>>) -> Request {
        let query = query.map(str::to_string);
        let (unread, flagged, before, cursor) = (None, None, None, None);
        Request::SearchMail { account: None, folder, query, unread, flagged, since, before, limit: 10, cursor }
    }

    fn page(response: Response) -> (Vec<String>, Option<String>, MailSource) {
        let Response::MailMessages { messages, cursor, source } = response else { panic!("a page, got {response:?}") };
        (messages.into_iter().map(|message| message.subject).collect(), cursor, source)
    }

    fn folders(response: Response) -> Vec<Folder> {
        let Response::MailFolders { folders } = response else { panic!("folders, got {response:?}") };
        folders
    }

    fn message(response: Response) -> Message {
        let Response::MailMessage { message } = response else { panic!("a message, got {response:?}") };
        message
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    fn get(id: &str, include_body: bool) -> Request {
        Request::GetMail { id: id.into(), include_body }
    }

    async fn id_of(service: &MailService, subject: &str) -> String {
        let Response::MailMessages { messages, .. } = answer(service, Reach::Suite, search(None, 100, None)).await
        else {
            panic!("a page")
        };
        messages.into_iter().find(|message| message.subject == subject).expect("synced").id
    }

    #[tokio::test]
    async fn folders_messages_and_threads_come_from_the_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Question", 2, None), &["\\Seen"]);
        server.add("INBOX", &fake::mail("Answer", 1, Some("Question@example.org")), &[]);
        server.add("Archive", &fake::mail("Kept", 3, None), &[]);
        let service = synced(dir.path(), &server).await;

        let listed = folders(answer(&service, Reach::Suite, Request::MailFolders { account: None }).await);
        let inbox = listed.iter().find(|folder| folder.role == Some(Role::Inbox)).expect("an inbox");
        assert_eq!((listed.len(), inbox.total, inbox.unread), (4, 2, 1));
        let one = Request::MailFolders { account: Some(ANNA.into()) };
        assert_eq!(folders(answer(&service, Reach::Suite, one).await).len(), 4);
        let other = Request::MailFolders { account: Some("mail:bob@example.org".into()) };
        assert_eq!(code(&answer(&service, Reach::Suite, other).await), "not-found");

        let (subjects, cursor, source) = page(answer(&service, Reach::Suite, search(None, 2, None)).await);
        assert_eq!((subjects, source), (vec!["Answer".to_string(), "Question".into()], MailSource::Cache));
        let (rest, last, _) = page(answer(&service, Reach::Suite, search(None, 2, cursor)).await);
        assert_eq!((rest, last), (vec!["Kept".to_string()], None));
        let archive = listed.iter().find(|folder| folder.role == Some(Role::Archive)).map(|folder| folder.id.clone());
        let in_archive = find(None, archive, None);
        assert_eq!(page(answer(&service, Reach::Suite, in_archive).await).0, ["Kept"]);
        let bad = search(None, 10, Some("x:1".into()));
        assert_eq!(code(&answer(&service, Reach::Suite, bad).await), "error");

        let answer_id = id_of(&service, "Answer").await;
        let read = message(answer(&service, Reach::Suite, get(&answer_id, true)).await);
        assert_eq!(read.text.as_deref(), Some("Text of Answer\n"));
        assert!(read.summary.untrusted);
        let short = message(answer(&service, Reach::Suite, get(&answer_id, false)).await);
        assert_eq!(short.text, None, "no text unless asked for");
        let thread = answer(&service, Reach::Suite, Request::GetMailThread { id: answer_id }).await;
        let Response::MailThread { messages } = thread else { panic!("a thread, got {thread:?}") };
        let thread: Vec<(&str, Option<&str>)> =
            messages.iter().map(|m| (m.summary.subject.as_str(), m.text.as_deref())).collect();
        assert_eq!(thread, [("Question", Some("Text of Question\n")), ("Answer", Some("Text of Answer\n"))]);
    }

    #[tokio::test]
    async fn a_thread_fetches_missing_texts_and_keeps_going_without_one_it_cannot_get() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let gone = server.add("INBOX", &fake::mail("Start", 300, None), &[]);
        server.add("INBOX", &fake::mail("Old reply", 200, Some("Start@example.org")), &[]);
        server.add("INBOX", &fake::mail("New reply", 1, Some("Start@example.org")), &[]);
        let service = synced(dir.path(), &server).await;
        server.expunge("INBOX", gone);

        let id = id_of(&service, "New reply").await;
        let thread = answer(&service, Reach::Suite, Request::GetMailThread { id }).await;
        let Response::MailThread { messages } = thread else { panic!("a thread, got {thread:?}") };
        let texts: Vec<(&str, Option<&str>)> =
            messages.iter().map(|m| (m.summary.subject.as_str(), m.text.as_deref())).collect();
        assert_eq!(
            texts,
            [("Start", None), ("Old reply", Some("Text of Old reply\n")), ("New reply", Some("Text of New reply\n"))],
            "the old texts are fetched; the one gone from the server stays without"
        );
        let peeks: Vec<String> = server.lines().into_iter().filter(|line| line.contains("BODY.PEEK[]")).collect();
        assert!(peeks.iter().any(|line| line.contains("FETCH 2 ")), "{peeks:?}");
    }

    #[test]
    fn cursors_are_only_the_ones_a_search_hands_out() {
        assert_eq!(Cursor::parse(None), Some(Cursor::Cache(0)));
        assert_eq!(Cursor::parse(Some("c:100")), Some(Cursor::Cache(100)));
        assert_eq!(Cursor::parse(Some("l:10000")), Some(Cursor::Live(CURSOR_MAX)));
        for bad in ["c:10001", "l:-1", "x:1", "c", "c:abc", ""] {
            assert_eq!(Cursor::parse(Some(bad)), None, "{bad}");
        }
        assert_eq!(Cursor::parse(Some(&Cursor::Live(7).text())), Some(Cursor::Live(7)));
    }

    #[tokio::test]
    async fn an_account_a_client_may_not_see_is_not_found_and_nothing_is_fetched() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with_attachment(), &[]);
        let service = synced(dir.path(), &server).await;
        let id = id_of(&service, "Report").await;
        let all = folders(answer(&service, Reach::Suite, Request::MailFolders { account: None }).await);

        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.mail_accounts = Selection::Only(vec!["mail:bob@example.org".into()]);
        let reach = Reach::Client(&rights);
        let before = server.lines().len();
        assert!(folders(answer(&service, reach, Request::MailFolders { account: None }).await).is_empty());
        let in_folder = find(None, Some(all[0].id.clone()), None);
        for request in [
            get(&id, true),
            Request::GetMailThread { id: id.clone() },
            Request::GetMailAttachment { id: id.clone(), attachment: "2".into() },
            in_folder,
        ] {
            assert_eq!(code(&answer(&service, reach, request).await), "not-found");
        }
        assert!(page(answer(&service, reach, search(Some("Report"), 10, None)).await).0.is_empty());
        let asked: Vec<String> =
            server.lines()[before..].iter().filter(|line| line.contains("UID ")).cloned().collect();
        assert!(asked.is_empty(), "nothing was asked of the server: {asked:?}");

        rights.mail_accounts = Selection::Only(vec![ANNA.into()]);
        let reach = Reach::Client(&rights);
        assert_eq!(message(answer(&service, reach, get(&id, false)).await).summary.subject, "Report");
    }

    #[tokio::test]
    async fn a_text_the_store_lacks_is_fetched_without_marking_the_message_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with("Old news", 200, "From long ago"), &[]);
        let service = synced(dir.path(), &server).await;
        let id = id_of(&service, "Old news").await;
        let stored = message(answer(&service, Reach::Suite, get(&id, false)).await);
        assert_eq!(stored.text, None, "older than 90 days: not kept ahead");

        let read = message(answer(&service, Reach::Suite, get(&id, true)).await);
        assert_eq!(read.text.as_deref(), Some("From long ago\n"));
        assert!(!read.summary.flags.contains(&Flag::Seen));
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains("UID FETCH") && line.contains("BODY.PEEK[]")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains(" BODY[") || line.contains("UID STORE")), "{lines:?}");

        let peeks = |lines: Vec<String>| lines.iter().filter(|line| line.contains("BODY.PEEK[]")).count();
        let before = peeks(server.lines());
        message(answer(&service, Reach::Suite, get(&id, true)).await);
        assert_eq!(peeks(server.lines()), before, "the text is kept now");
    }

    #[tokio::test]
    async fn an_attachment_lands_in_the_managed_folder_under_a_clean_name() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with_attachment(), &[]);
        let service = synced(dir.path(), &server).await;
        let id = id_of(&service, "Report").await;
        let listed = message(answer(&service, Reach::Suite, get(&id, false)).await);
        let attachment = listed.attachments.iter().find(|a| !a.inline).expect("an attachment");
        assert_eq!(attachment.filename.as_deref(), Some("../../evil.txt"), "data, as the message says");

        let request = Request::GetMailAttachment { id: id.clone(), attachment: attachment.part.clone() };
        let fetched = answer(&service, Reach::Suite, request).await;
        let Response::MailAttachment { path, size, content_type } = fetched else { panic!("a file, got {fetched:?}") };
        let path = std::path::PathBuf::from(path);
        let managed = service.attachments().expect("a folder").dir().to_path_buf();
        assert!(path.starts_with(&managed), "{path:?}");
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("evil.txt"));
        assert_eq!(std::fs::read(&path).expect("written"), b"hello attachment");
        assert_eq!((size, content_type.as_str()), (16, "text/plain"));

        let missing = Request::GetMailAttachment { id, attachment: "9".into() };
        assert_eq!(code(&answer(&service, Reach::Suite, missing).await), "not-found");
    }

    #[tokio::test]
    async fn words_in_old_texts_are_searched_on_the_server_after_the_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with("Recent zebra", 1, "nothing here"), &[]);
        server.add("INBOX", &mail_with("Old one", 200, "a zebra and Müller"), &[]);
        server.add("Archive", &mail_with("Older one", 300, "no stripes"), &[]);
        let service = synced(dir.path(), &server).await;

        let (first, cursor, source) = page(answer(&service, Reach::Suite, search(Some("zebra"), 1, None)).await);
        assert_eq!((first, source), (vec!["Recent zebra".to_string()], MailSource::Cache));
        assert_eq!(cursor.as_deref(), Some("l:0"), "the store is done, the server comes next");
        let (second, last, source) = page(answer(&service, Reach::Suite, search(Some("zebra"), 1, cursor)).await);
        assert_eq!((second, last, source), (vec!["Old one".to_string()], None, MailSource::Live));

        // Only the server knows: the page goes there at once, for words that are not ASCII too.
        let (found, last, source) = page(answer(&service, Reach::Suite, search(Some("müller"), 10, None)).await);
        assert_eq!((found, last, source), (vec!["Old one".to_string()], None, MailSource::Live));
        let lines = server.lines();
        assert!(lines.iter().any(|line| line.contains("UID SEARCH CHARSET UTF-8 TEXT")), "{lines:?}");

        // Without words, or within the last 90 days, the store answers alone.
        let lines_before = server.lines().len();
        let (all, ..) = page(answer(&service, Reach::Suite, search(None, 10, None)).await);
        assert_eq!(all, ["Recent zebra", "Old one", "Older one"]);
        let recent = find(Some("zebra"), None, Some(Utc::now() - Duration::days(30)));
        assert_eq!(
            page(answer(&service, Reach::Suite, recent).await),
            (vec!["Recent zebra".into()], None, MailSource::Cache)
        );
        assert!(!server.lines()[lines_before..].iter().any(|line| line.contains("SEARCH")), "the store was enough");
    }
}
