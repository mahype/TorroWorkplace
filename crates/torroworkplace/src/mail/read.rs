//! Reading mail (task M3.1): the handlers of `mail.list_folders`, `mail.search_messages`,
//! `mail.index_status` (issue #136), `mail.get_message`, `mail.get_thread` and
//! `mail.get_attachment`. The core sends these requests here ([`answer`]) from
//! `Core::handle_within`, with the caller's reach.
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
//! **HTML** (task M8.1): the store keeps the cleaned HTML part beside the text (`mime::read`
//! cleans it). `mail.get_message` hands it out only with `include_html`; when the store has the
//! text but no HTML for it (kept before the core knew HTML, or pruned), the message is fetched once
//! more the same way, without marking it read.
//!
//! **Search** ([`search_messages`], issue #136) runs on the stores alone, so it works offline and
//! the same everywhere: the query language of [`query`] (words, phrases, `-`, `OR`, operators),
//! folded as [`fold`] says, over subject, sender, recipients, text and attachment names of every
//! message in scope, every folder unless one is asked for. Texts of older messages are put into
//! the index in the background (`sync::backfill`); until that is done a search that needs texts
//! says it may be incomplete. Pages come by date (`order`) or best match first (`sort:
//! relevance`), the `cursor` (`c:<n>`, at most [`CURSOR_MAX`]) says how far. Each page says where
//! the words matched ([`MailHighlight`], found in the message's own text, not in the folded
//! index) and the first one how many match in all.
//!
//! **Later** (task E9.1, `snoozed`): the inbox says which messages wait in "Later" (filed into a
//! snoozed entry) and which came back and when ([`Inbox::mail_later`]). They are picked by id in
//! each store's query (`SearchQuery::only`, `except`), so they are left out or kept *before* the
//! page is cut and the offsets of the cursor hold. With `snoozed: false` a message back from Later
//! is fetched apart, whatever its date, and the merge puts it by the time it came back
//! (`MessageSummary::back`), so it stands on top of "All" as if it had just arrived; every search
//! fills `back`.
//!
//! **Attachments** go to the managed folder (`AttachmentCache`), at most
//! [`ATTACHMENT_MAX_BYTES`], under a cleaned file name, so a name from outside never picks the
//! path.
//!
//! **Invitations** (task E8.3b): wherever a whole message is read (here and in the sync), its
//! calendar part is read as well ([`invite_of`]) and what it invites to kept with its text, so the
//! summaries carry it (`MessageSummary::invite`). [`fetch_invitation`] reads the whole invitation
//! from the server for an answer (`mail.respond_to_invitation`): method, event, sequence,
//! organizer and attendees ([`Invitation`]); `torro_pim_caldav::parse_events` reads the event, a
//! small reader here the rest.
//!
//! **Log.** Nothing here writes a search term, folder name, file name or text anywhere; the
//! core's audit log names accounts and hashed ids only (`audit::ids`).
//!
//! **Read counts as opened** (task E8.8b, [`answer_counting`]): a mail the person reads in the mail
//! window (`mail.get_message` with the app key) whose mail is an open entry of the inbox counts that
//! entry as opened for "Insights", once per entry and day ([`Inbox::mail_read`]). An agent reading
//! mail is not the person opening it and counts nothing. No new request.

use chrono::{DateTime, Utc};
use torro_core::i18n::tr;
use torro_inbox::SourceRef;
use torro_ipc::Response;
use torro_ipc::api::{Access, Area};
use torro_ipc::mail::{IncompleteReason, MailHighlight, MailSort, MailSource};
use torro_ipc::{Request, mail::limit_refusal};
use torro_mail::{Invite, MailAddress, Message, MessageSummary, Order};
use torro_mail_imap::imap::Session;
use torro_mail_imap::mime::{self, MimeError, ReadMessage};

use super::MailStoreError;
use super::fold::{self, Needle};
use super::query::{self, Field, Query};
use super::service::{MailError, MailService, SharedStore};
#[cfg(doc)]
use super::store::BODY_RETENTION_DAYS;
use super::store::{Body, FolderRef, MessageRef, SearchQuery};
use crate::inbox::{Inbox, MailLater};
use crate::pim::Reach;

/// The largest attachment `mail.get_attachment` hands out: 50 MiB.
pub const ATTACHMENT_MAX_BYTES: usize = 50 * 1024 * 1024;

/// At most this many messages of one conversation (`mail.get_thread`), the newest: a thread made
/// up by a sender must not make the core fetch thousands of texts.
pub const THREAD_MAX: usize = 100;

/// A failed request, already as the answer to send.
type Answer<T> = Result<T, Box<Response>>;

/// The stores a caller sees: account id and store, in the order of `accounts.json`.
type Stores = Vec<(String, SharedStore)>;

/// [`answer`], and then – for `mail.get_message` with the app key ([`Reach::Suite`]) that found the
/// mail – tells `inbox` the person read it ([`Inbox::mail_read`], task E8.8b): the open entry the
/// mail is filed into counts as opened, once per entry and day. The answer is [`answer`]'s
/// unchanged; a client's reading counts nothing. Every window of the suite reads with the app key, so
/// the compose window fetching the mail it answers or forwards counts too: who answers has read it.
pub async fn answer_counting(mail: &MailService, inbox: &Inbox, reach: Reach<'_>, request: Request) -> Response {
    let read = match (&request, reach) {
        (Request::GetMail { id, .. }, Reach::Suite) => Some(id.clone()),
        _ => None,
    };
    let response = answer(mail, inbox, reach, request).await;
    if let (Some(id), Response::MailMessage { message }) = (read, &response) {
        inbox.mail_read(&message.summary.account, &id, Utc::now());
    }
    response
}

/// Answers a reading mail request for `reach` (`mail.list_folders`, `mail.search_messages`,
/// `mail.index_status`, `mail.get_message`, `mail.get_thread`, `mail.get_attachment`). The limits are checked first
/// ([`limit_refusal`]), also for the app key. Any other request is `unsupported`. `inbox` says
/// which mail waits in "Later" for a search (task E9.1); nothing here changes it.
pub async fn answer(mail: &MailService, inbox: &Inbox, reach: Reach<'_>, request: Request) -> Response {
    if let Some(refused) = limit_refusal(&request) {
        return refused;
    }
    let answered = match request {
        Request::MailFolders { account } => list_folders(mail, reach, account.as_deref()).await,
        Request::SearchMail {
            account,
            folder,
            query,
            unread,
            flagged,
            since,
            before,
            limit,
            cursor,
            order,
            sort,
            snoozed,
            categories,
        } => {
            let (order, sort) = (order.unwrap_or_default(), sort.unwrap_or_default());
            let search = Search {
                account,
                folder,
                query,
                unread,
                flagged,
                since,
                before,
                limit,
                cursor,
                order,
                sort,
                snoozed,
                categories,
            };
            // Boxed: the core's request match awaits this deep in the stack already.
            Box::pin(search_messages(mail, inbox, reach, search, Utc::now())).await
        }
        Request::MailIndexStatus { account } => index_status(mail, reach, account.as_deref()).await,
        Request::GetMail { id, include_body, include_html } => {
            get_message(mail, reach, &id, include_body, include_html).await
        }
        Request::GetMailThread { id } => get_thread(mail, reach, &id).await,
        Request::GetMailAttachment { id, attachment } => get_attachment(mail, reach, &id, &attachment).await,
        other => {
            let message = format!("{} is not a reading mail command", other.operation().name());
            Err(Box::new(Response::Error { code: "unsupported".into(), message }))
        }
    };
    answered.unwrap_or_else(|refused| *refused)
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
        (true, Err(err)) => Err(err.response().into()),
        _ => Err(MailError::NotFound.response().into()),
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
    /// The query language of [`query`]; blank counts as none.
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
    /// Newest or oldest first (task E8.15).
    pub order: Order,
    /// Only mail of one of these categories (task E12.4); empty: every message.
    pub categories: Vec<torro_mail::Category>,
    /// By date or by relevance (issue #136).
    pub sort: MailSort,
    /// "Later" (task E9.1): `false` without the snoozed mail and with mail back from it on top,
    /// `true` the snoozed mail only, `None` all of it by date.
    pub snoozed: Option<bool>,
}

/// What a search makes of "Later" (task E9.1): the inbox's word on the stores in scope and the
/// search's `snoozed`.
struct LaterView {
    later: MailLater,
    snoozed: Option<bool>,
}

impl LaterView {
    /// The queries for the store of `account`: the page is merged from all of them. One query
    /// except with `snoozed: false`, which fetches the mail back from Later apart (none when
    /// there is none), so it is found whatever its date.
    fn queries(&self, account: &str, query: &SearchQuery) -> Vec<SearchQuery> {
        match self.snoozed {
            None => vec![query.clone()],
            Some(true) => vec![SearchQuery { only: Some(self.later.snoozed_of(account)), ..query.clone() }],
            Some(false) => {
                let back = self.later.back_of(account);
                let mut except = self.later.snoozed_of(account);
                except.extend(back.iter().cloned());
                let mut queries = vec![SearchQuery { except, ..query.clone() }];
                if !back.is_empty() {
                    queries.push(SearchQuery { only: Some(back), ..query.clone() });
                }
                queries
            }
        }
    }

    /// The query that counts the matches in the store of `account`.
    fn counting(&self, account: &str, query: &SearchQuery) -> SearchQuery {
        match self.snoozed {
            None => query.clone(),
            Some(true) => SearchQuery { only: Some(self.later.snoozed_of(account)), ..query.clone() },
            Some(false) => SearchQuery { except: self.later.snoozed_of(account), ..query.clone() },
        }
    }

    /// Where `summary` sorts by date: when it came back from Later with `snoozed: false`, else its
    /// date.
    fn sorts_at(&self, summary: &MessageSummary) -> DateTime<Utc> {
        match (self.snoozed, summary.back) {
            (Some(false), Some(back)) => back,
            _ => summary.date,
        }
    }
}

/// How deep a search pages: a page starts at most at the 10 000th match. Paging deeper would
/// make every page read that many rows; narrow the search instead.
pub const CURSOR_MAX: u32 = 10_000;

/// Where a page starts: `c:<n>` is the n-th match. No text: the first page. `None` for a text
/// this search does not hand out (also the `l:` of cores before issue #136), or one deeper than
/// [`CURSOR_MAX`].
fn cursor_offset(text: Option<&str>) -> Option<u32> {
    let Some(text) = text else { return Some(0) };
    let offset: u32 = text.strip_prefix("c:")?.parse().ok()?;
    (offset <= CURSOR_MAX).then_some(offset)
}

/// One page of a search as of `now`, from the stores of every account in scope (or the one
/// holding `folder`), merged in `search.order` by date, or best match first for `sort:
/// relevance` (issue #136). Everything is answered from the core's own index, so it works
/// offline; while an account in scope still indexes older texts, a search that needs texts says
/// so (`complete: false`, `incomplete_reason: indexing`). The first page counts every match
/// (`total_estimate`); with words, each message that matched in its subject or kept text has its
/// highlights.
///
/// `not-found` for a `folder` no reachable store knows (or one outside `account`), `error` for a
/// cursor this search did not hand out and for a query that cannot be read ([`query::parse`]).
pub async fn search_messages(
    mail: &MailService,
    inbox: &Inbox,
    reach: Reach<'_>,
    search: Search,
    now: DateTime<Utc>,
) -> Answer<Response> {
    let mut stores = scoped(mail, reach, search.account.as_deref()).await?;
    let mut folder = None;
    if let Some(id) = &search.folder {
        let (account, store, found) = locate_folder(&stores, id).await?.ok_or_else(no_folder)?;
        stores = vec![(account, store)];
        folder = Some(found.id);
    }
    let offset = cursor_offset(search.cursor.as_deref()).ok_or_else(bad_cursor)?;
    let parsed = query::parse(search.query.as_deref().unwrap_or("")).map_err(|err| failed(&err.message()))?;
    // Without words every match ranks the same: then it is by date, in `order`.
    let relevance = (search.sort == MailSort::Relevance && parsed.has_words()).then_some(now);
    let query = SearchQuery {
        folder,
        query: parsed,
        unread: search.unread,
        flagged: search.flagged,
        since: search.since,
        before: search.before,
        order: search.order,
        relevance,
        only: None,
        except: Vec::new(),
        categories: search.categories.clone(),
    };
    let later = match inbox.mail_later(&later_accounts(&stores, reach), now) {
        Ok(later) => later,
        // Without `snoozed` the page needs only `back`: it comes without, as before task E9.1.
        Err(err) if search.snoozed.is_none() => {
            eprintln!("torroworkplace: mail: cannot read what waits in later: {err}");
            MailLater::default()
        }
        Err(err) => {
            eprintln!("torroworkplace: mail: cannot read what waits in later: {err}");
            return Err(failed("cannot read which mail waits for later"));
        }
    };
    let view = LaterView { later, snoozed: search.snoozed };
    let limit = search.limit as usize;
    let mut page = ranked_page(&stores, &query, &view, offset, limit + 1).await?;
    let more = page.len() > limit;
    page.truncate(limit);
    let deeper = offset.saturating_add(search.limit);
    let cursor = (more && deeper <= CURSOR_MAX).then(|| format!("c:{deeper}"));
    let complete = !query.query.needs_texts() || all_indexed(&stores).await?;
    let total_estimate = match search.cursor {
        None => Some(count(&stores, &query, &view).await?),
        Some(_) => None,
    };
    let highlights = if query.query.has_words() { highlights(&stores, &page, &query.query).await? } else { Vec::new() };
    Ok(Response::MailMessages {
        messages: page,
        cursor,
        source: MailSource::Cache,
        complete,
        incomplete_reason: (!complete).then_some(IncompleteReason::Indexing),
        highlights,
        total_estimate,
    })
}

/// The accounts of `stores` whose "Later" `reach` may learn (task E9.1): every one for the suite; for
/// a client only those whose mail entries it sees in the inbox – read access to `inbox` and the
/// account as one of its sources ([`torro_ipc::api::Rights::reaches_source`]), as
/// `inbox.list_items` shows them. For the others the search knows nothing of "Later": nothing is
/// left out, nothing is `back`.
fn later_accounts(stores: &[(String, SharedStore)], reach: Reach<'_>) -> Vec<String> {
    let sees = |account: &str| match reach {
        Reach::Suite => true,
        Reach::Client(rights) => {
            rights.area(Area::Inbox).allows(Access::Read)
                && account.parse::<SourceRef>().is_ok_and(|source| rights.reaches_source(&source))
        }
    };
    stores.iter().map(|(account, _)| account.clone()).filter(|account| sees(account)).collect()
}

/// The matches `offset` to `offset + count` over all `stores`: by date in `query.order` (with
/// `snoozed: false` mail back from Later by the time it came back, [`LaterView::sorts_at`]), or
/// by rank for a search by relevance. Each store gives its first `offset + count` (and all its
/// mail back from Later that matches), so pages of several accounts interleave. Every message
/// carries `back` when it came back from Later.
async fn ranked_page(
    stores: &[(String, SharedStore)],
    query: &SearchQuery,
    view: &LaterView,
    offset: u32,
    count: usize,
) -> Answer<Vec<MessageSummary>> {
    let wanted = offset.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    let mut all: Vec<(MessageSummary, f64)> = Vec::new();
    for (account, store) in stores {
        for query in view.queries(account, query) {
            // Mail picked by id is all fetched: there is little of it, and its place is not its date.
            let wanted = query.only.as_ref().map_or(wanted, |ids| u32::try_from(ids.len()).unwrap_or(u32::MAX));
            all.extend(store.run(move |store| store.search_ranked(&query, wanted, 0)).await.map_err(store_failed)?);
        }
    }
    for (summary, _) in &mut all {
        summary.back = view.later.back_at(&summary.account, &summary.id);
    }
    match query.relevance {
        // Stable: the same rank keeps each store's own order (newest first among equals).
        Some(_) => all.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.date.cmp(&a.0.date))),
        None => all.sort_by(|a, b| query.order.apply(view.sorts_at(&b.0).cmp(&view.sorts_at(&a.0)))),
    }
    Ok(all.into_iter().skip(offset as usize).take(count).map(|(summary, _)| summary).collect())
}

/// Every store in scope holds the texts of all its messages in its index.
async fn all_indexed(stores: &[(String, SharedStore)]) -> Answer<bool> {
    for (_, store) in stores {
        if !store.run(|store| store.fully_indexed()).await.map_err(store_failed)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// How many messages match `query` in all `stores`, as `view` leaves or keeps the mail in Later.
async fn count(stores: &[(String, SharedStore)], query: &SearchQuery, view: &LaterView) -> Answer<u64> {
    let mut total = 0u64;
    for (account, store) in stores {
        let query = view.counting(account, query);
        total += store.run(move |store| store.count(&query)).await.map_err(store_failed)?;
    }
    Ok(total)
}

/// Where the words of `query` matched in each message of `page`: in its subject and in its kept
/// text ([`fold::find`]). Messages with neither are left out; so is the text of a message that is
/// no longer kept to read (its words are still in the index).
async fn highlights(
    stores: &[(String, SharedStore)],
    page: &[MessageSummary],
    query: &Query,
) -> Answer<Vec<MailHighlight>> {
    let needles = |field| -> Vec<Needle> {
        query
            .marked(field)
            .into_iter()
            .map(|(text, phrase)| if phrase { Needle::phrase(text) } else { Needle::word(text) })
            .collect()
    };
    let (in_subject, in_text) = (needles(Field::Subject), needles(Field::Any));
    let mut found = Vec::new();
    for (account, store) in stores {
        let ids: Vec<String> =
            page.iter().filter(|message| &message.account == account).map(|message| message.id.clone()).collect();
        if ids.is_empty() {
            continue;
        }
        let (in_subject, in_text) = (in_subject.clone(), in_text.clone());
        let marked = store
            .run(move |store| {
                let mut marked = Vec::new();
                for id in ids {
                    let Some(message) = store.message(&id)? else { continue };
                    let subject = &message.summary.subject;
                    let subject = fold::pieces(subject, &fold::find(subject, &in_subject));
                    let text = fold::head(message.text.as_deref().unwrap_or(""), fold::HIGHLIGHT_TEXT_MAX);
                    let snippet = fold::passage(text, &fold::find(text, &in_text));
                    if !subject.is_empty() || !snippet.is_empty() {
                        marked.push(MailHighlight { id, subject, snippet });
                    }
                }
                Ok(marked)
            })
            .await
            .map_err(store_failed)?;
        found.extend(marked);
    }
    // In the page's order.
    found.sort_by_key(|highlight| page.iter().position(|message| message.id == highlight.id));
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

// ----- mail.index_status -----

/// `mail.index_status` (issue #136): how far the index of each account `reach` sees is, or of
/// `account` alone (`not-found` when it is none `reach` sees).
async fn index_status(mail: &MailService, reach: Reach<'_>, account: Option<&str>) -> Answer<Response> {
    scoped(mail, reach, account).await?;
    let mut accounts = mail.index_progress(reach).await.map_err(|err| Box::new(err.response()))?;
    if let Some(account) = account {
        accounts.retain(|progress| progress.account == account);
    }
    Ok(Response::MailIndexStatus { accounts })
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
/// to list; so is its HTML when it is asked for (`include_html`) and the store has none stored.
/// Never marked read.
async fn get_message(
    mail: &MailService,
    reach: Reach<'_>,
    id: &str,
    include_body: bool,
    include_html: bool,
) -> Answer<Response> {
    let stores = mail.stores(reach).await;
    let located = locate(&stores, id).await?;
    let mut message = stored_message(&located).await?;
    // `None`: not stored, so it must be fetched; `Some(None)`: the message has none.
    let stored_html = if include_html { stored_html(&located).await? } else { Some(None) };
    let needs_text = message.text.is_none() && (include_body || message.summary.has_attachments);
    let html = match stored_html {
        Some(html) if !needs_text => html,
        _ => {
            message = fetch_body(mail, &located).await?;
            message.html.take()
        }
    };
    if !include_body {
        message.text = None;
    }
    message.html = if include_html { html } else { None };
    Ok(Response::MailMessage { message })
}

/// The cleaned HTML of the message as the store has it ([`MailStore::html`](super::store::MailStore::html)).
async fn stored_html(located: &Located) -> Answer<Option<Option<String>>> {
    let id = located.place.id.clone();
    located.store.run(move |store| store.html(&id)).await.map_err(store_failed)
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
        let invite = invite_of(&raw, &read);
        let body = Body::of(read, invite);
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

/// Fetches the whole message from the server (`BODY.PEEK[]`, so it stays unread), keeps its text,
/// cleaned HTML and attachment list in the store and returns the message as the store has it then,
/// with its `html` set (the caller decides whether it hands it out).
async fn fetch_body(mail: &MailService, located: &Located) -> Answer<Message> {
    let raw = fetch_raw(mail, located).await?;
    let read = mime::read(&raw).map_err(|_| unreadable())?;
    let id = located.place.id.clone();
    let invite = invite_of(&raw, &read);
    let body = Body::of(read, invite);
    let html = body.html.clone();
    let message = located
        .store
        .run(move |store| {
            store.set_body(&id, &body)?;
            store.message(&id)
        })
        .await
        .map_err(store_failed)?
        .ok_or_else(no_message)?;
    Ok(Message { html, ..message })
}

/// The raw message from the account's command connection. `not-found` when it is gone from the
/// server or the folder was renumbered since the store took it.
async fn fetch_raw(mail: &MailService, located: &Located) -> Answer<Vec<u8>> {
    let mut command = mail.command(&located.account).await.map_err(|err| Box::new(err.response()))?;
    let raw = peek(command.session(), &located.place).await.map_err(|err| Box::new(err.response()))?;
    command.release();
    raw.ok_or_else(no_message)
}

/// `BODY.PEEK[]` of the message at `place`, opening its folder first unless it is open. `None`
/// when it is gone, or the folder was renumbered since the store took it.
pub(crate) async fn peek(session: &mut Session, place: &MessageRef) -> Result<Option<Vec<u8>>, MailError> {
    let uid_validity = match session.selected() {
        Some(selected) if selected.path == place.folder_path => selected.uid_validity,
        _ => session.select(&place.folder_path).await?.uid_validity,
    };
    if uid_validity != place.uidvalidity {
        return Ok(None);
    }
    Ok(session.fetch_message(place.uid).await?)
}

// ----- invitations (task E8.3b) -----

/// The largest calendar part read for an invitation: an invitation is a few kilobytes, and
/// nothing from outside makes the core parse megabytes.
const CALENDAR_MAX_BYTES: u64 = 256 * 1024;

/// An invitation as the calendar part of a message says it (iTIP, RFC 5546), read for
/// `mail.respond_to_invitation`. Everything in it came from outside and is data, never
/// instructions; the core never takes an address or a time from anywhere else than here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invitation {
    /// The iTIP method, upper case. Only `REQUEST` asks for an answer; `CANCEL`, `REPLY`, `PUBLISH`
    /// … do not.
    pub method: String,
    /// The first event as `torro_pim_caldav::parse_events` reads it, without a calendar yet
    /// (`collection` empty) and without `href` or `etag`.
    pub event: torro_pim::Event,
    /// `SEQUENCE`, 0 when missing: the answer names the version of the event it answers.
    pub sequence: u32,
    /// `ORGANIZER`: who gets the answer.
    pub organizer: Option<MailAddress>,
    /// Every `ATTENDEE` with a `mailto:` address, in order.
    pub attendees: Vec<MailAddress>,
}

impl Invitation {
    /// Reads the first event of the iCalendar object `ics` and what an answer needs: the method of
    /// the object, `SEQUENCE`, `ORGANIZER` and `ATTENDEE` of that event (nested components such as
    /// `VALARM` left out). `None` without an event with a readable start (or a cancelled one).
    pub(crate) fn parse(ics: &str) -> Option<Invitation> {
        let event = torro_pim_caldav::parse_events(ics, "").into_iter().next()?;
        let mut invitation =
            Invitation { method: String::new(), event, sequence: 0, organizer: None, attendees: Vec::new() };
        let (mut open, mut in_first, mut first_done) = (Vec::<String>::new(), false, false);
        for line in unfold(ics) {
            let Some((name, params, value)) = content_line(&line) else { continue };
            match name.to_ascii_uppercase().as_str() {
                "BEGIN" => {
                    open.push(value.to_ascii_uppercase());
                    in_first |= !first_done && open.len() == 2 && value.eq_ignore_ascii_case("VEVENT");
                }
                "END" => {
                    if in_first && open.len() == 2 {
                        (in_first, first_done) = (false, true);
                    }
                    open.pop();
                }
                "METHOD" if open.len() == 1 => invitation.method = value.trim().to_ascii_uppercase(),
                "SEQUENCE" if in_first && open.len() == 2 => invitation.sequence = value.trim().parse().unwrap_or(0),
                "ORGANIZER" if in_first && open.len() == 2 => invitation.organizer = mailto(&params, &value),
                "ATTENDEE" if in_first && open.len() == 2 => invitation.attendees.extend(mailto(&params, &value)),
                _ => {}
            }
        }
        Some(invitation)
    }

    /// It can be answered: a `REQUEST` with a UID and an organizer, for one event. Invitations to
    /// a series (`RRULE`, `RDATE`, `RECURRENCE-ID`) cannot yet: the calendar service writes single
    /// events only, and an answer would lose the rule.
    pub(crate) fn answerable(&self) -> bool {
        self.method == "REQUEST"
            && !self.event.uid.trim().is_empty()
            && self.organizer.is_some()
            && !self.event.recurring
    }

    /// What it invites to, as lists show it ([`Invite`]); `None` when it cannot be answered.
    pub(crate) fn invite(&self) -> Option<Invite> {
        if !self.answerable() {
            return None;
        }
        Some(Invite {
            title: self.event.summary.trim().to_string(),
            start: instant(self.event.start),
            end: self.event.end.map(instant),
            all_day: self.event.all_day(),
            answered: None,
        })
    }

    /// The event is over at `now`: its end (its start without one) has passed.
    pub(crate) fn ended(&self, now: DateTime<Utc>) -> bool {
        instant(self.event.end.unwrap_or(self.event.start)) <= now
    }
}

/// The instant `time` stands for: an all-day date as midnight UTC.
fn instant(time: torro_pim::EventTime) -> DateTime<Utc> {
    match time {
        torro_pim::EventTime::Date(day) => day.and_time(chrono::NaiveTime::MIN).and_utc(),
        torro_pim::EventTime::DateTime(at) => at,
    }
}

/// The invitation in the message `raw` (read as `read`): the first calendar part (`text/calendar`
/// before `application/ics`) that holds an event. `None` for a message without one.
pub(crate) fn invitation_in(raw: &[u8], read: &ReadMessage) -> Option<Invitation> {
    if !read.invitation {
        return None;
    }
    let mut parts: Vec<_> = read
        .attachments
        .iter()
        .filter(|part| matches!(part.content_type.as_str(), "text/calendar" | "application/ics"))
        .filter(|part| part.size <= CALENDAR_MAX_BYTES)
        .collect();
    parts.sort_by_key(|part| part.content_type != "text/calendar");
    parts.into_iter().find_map(|part| {
        let found = mime::attachment(raw, &part.part).ok()?;
        Invitation::parse(&String::from_utf8_lossy(&found.bytes))
    })
}

/// What the message `raw` invites to (the summary's `invite`, kept with its text); `None` without
/// an invitation that can be answered ([`Invitation::answerable`]).
pub(crate) fn invite_of(raw: &[u8], read: &ReadMessage) -> Option<Invite> {
    invitation_in(raw, read).and_then(|invitation| invitation.invite())
}

/// A message with its invitation, fetched from the server for an answer ([`fetch_invitation`]).
#[derive(Debug, Clone)]
pub(crate) struct Invited {
    /// Where the message is.
    pub place: MessageRef,
    /// The message as read from its bytes.
    pub read: ReadMessage,
    /// Its invitation; `None` when it carries none.
    pub invitation: Option<Invitation>,
}

/// The message `id` of the mail account `account`, fetched from the server (`BODY.PEEK[]`, so it
/// stays unread) and its invitation read ([`invitation_in`]). What the invitation invites to is
/// kept in the store as well, so lists show it from now on. `not-found` for a message the
/// account's store does not hold (or one gone from the server); the account's own errors when it
/// cannot be reached. The caller checked that it may read the account.
pub(crate) async fn fetch_invitation(mail: &MailService, account: &str, id: &str) -> Answer<Invited> {
    let store = mail.store(account).await.map_err(|err| Box::new(err.response()))?;
    let located = locate(&[(account.to_string(), store)], id).await?;
    let raw = fetch_raw(mail, &located).await?;
    let read = mime::read(&raw).map_err(|_| unreadable())?;
    let invitation = invitation_in(&raw, &read);
    let body = Body::of(read.clone(), invitation.as_ref().and_then(Invitation::invite));
    let kept = id.to_string();
    located.store.run(move |store| store.set_body(&kept, &body)).await.map_err(store_failed)?;
    Ok(Invited { place: located.place, read, invitation })
}

/// The parameters of a content line: names upper case, values without quotes.
type Params = Vec<(String, String)>;

/// Joins folded lines of an iCalendar object (a line break followed by a space or tab continues
/// the line); bare LF line ends count like CRLF.
fn unfold(ics: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match (raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ if raw.trim().is_empty() => {}
            _ => lines.push(raw.to_string()),
        }
    }
    lines
}

/// One content line split into its name, its parameters (names upper case, quotes removed) and
/// its value; the value starts at the first colon outside a quoted parameter value.
fn content_line(line: &str) -> Option<(String, Params, String)> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(at, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(at),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_string();
    let params = parts
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_uppercase(), value.trim().trim_matches('"').to_string()))
        .collect();
    Some((name, params, value.to_string()))
}

/// The address of an `ORGANIZER` or `ATTENDEE` value (`mailto:anna@example.org`, any case) with
/// its `CN` as the name; `None` for anything but one plain address.
fn mailto(params: &[(String, String)], value: &str) -> Option<MailAddress> {
    let value = value.trim();
    let address = value.get(..7).filter(|scheme| scheme.eq_ignore_ascii_case("mailto:")).map(|_| &value[7..])?;
    let address = address.trim();
    let plain = address.split('@').count() == 2 && !address.starts_with('@') && !address.ends_with('@');
    if !plain || address.chars().any(|c| c.is_whitespace() || c.is_control() || "<>,;\"".contains(c)) {
        return None;
    }
    let name = params.iter().find(|(key, _)| key == "CN").map(|(_, name)| name.trim().to_string());
    Some(MailAddress { name: name.filter(|name| !name.is_empty()), address: address.to_string() })
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
    let cache = mail.attachment_cache().ok_or_else(no_attachment_folder)?;
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

fn store_failed(err: MailStoreError) -> Box<Response> {
    MailError::Store(err).response().into()
}

fn not_found(message: &str) -> Box<Response> {
    Box::new(Response::Error { code: "not-found".into(), message: message.into() })
}

fn failed(message: &str) -> Box<Response> {
    Box::new(Response::Error { code: "error".into(), message: message.into() })
}

fn no_message() -> Box<Response> {
    not_found(tr("No such message."))
}

fn no_folder() -> Box<Response> {
    not_found(tr("No such folder."))
}

fn no_attachment() -> Box<Response> {
    not_found(tr("The message has no such attachment."))
}

fn too_big() -> Box<Response> {
    failed(tr("The attachment is larger than 50 MiB."))
}

fn unreadable() -> Box<Response> {
    failed(tr("The message could not be read."))
}

fn bad_cursor() -> Box<Response> {
    failed(tr("This cursor is not valid. Search again without it."))
}

fn no_attachment_folder() -> Box<Response> {
    failed(tr("Attachments cannot be kept on this device."))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use chrono::{Duration, Utc};
    use torro_ipc::StatsRange;
    use torro_ipc::api::{RightsTemplate, Selection};
    use torro_mail::{Flag, Folder, Role};

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::{self, FakeImap};

    const ANNA: &str = "mail:anna@example.org";

    /// [`super::answer`] with an inbox of its own that holds nothing for later (task E9.1): what
    /// every test but those of "Later" reads with.
    async fn answer(service: &MailService, reach: Reach<'_>, request: Request) -> Response {
        let dir = tempfile::tempdir().expect("temp dir");
        let inbox = crate::inbox::tests::inbox_in(dir.path());
        super::answer(service, &inbox, reach, request).await
    }

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
            order: None,
            sort: None,
            snoozed: None,
            categories: Vec::new(),
        }
    }

    /// A first page of ten for `query` in `folder` from `since` on.
    fn find(query: Option<&str>, folder: Option<String>, since: Option<DateTime<Utc>>) -> Request {
        let query = query.map(str::to_string);
        let (unread, flagged, before, cursor) = (None, None, None, None);
        Request::SearchMail {
            account: None,
            folder,
            query,
            unread,
            flagged,
            since,
            before,
            limit: 10,
            cursor,
            order: None,
            sort: None,
            snoozed: None,
            categories: Vec::new(),
        }
    }

    /// [`search`] oldest first (task E8.15).
    fn oldest(query: Option<&str>, limit: u32, cursor: Option<String>) -> Request {
        let mut request = search(query, limit, cursor);
        if let Request::SearchMail { order, .. } = &mut request {
            *order = Some(Order::OldestFirst);
        }
        request
    }

    /// [`search`] best match first (issue #136).
    fn relevant(query: &str, limit: u32) -> Request {
        let mut request = search(Some(query), limit, None);
        if let Request::SearchMail { sort, .. } = &mut request {
            *sort = Some(MailSort::Relevance);
        }
        request
    }

    fn page(response: Response) -> (Vec<String>, Option<String>, MailSource) {
        let Response::MailMessages { messages, cursor, source, .. } = response else {
            panic!("a page, got {response:?}")
        };
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
        Request::GetMail { id: id.into(), include_body, include_html: false }
    }

    /// `mail.get_message` with text and HTML, as the mail window asks (task M8.1).
    fn get_html(id: &str) -> Request {
        Request::GetMail { id: id.into(), include_body: true, include_html: true }
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
        // Task E8.15: oldest first, with its own cursor.
        let (subjects, cursor, _) = page(answer(&service, Reach::Suite, oldest(None, 2, None)).await);
        assert_eq!(subjects, ["Kept", "Question"]);
        let (rest, last, _) = page(answer(&service, Reach::Suite, oldest(None, 2, cursor)).await);
        assert_eq!((rest, last), (vec!["Answer".to_string()], None));
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
        // Its text cannot be fetched any more; expunging it instead would race with the sync, which
        // could drop it from the cache before the thread is read.
        server.withhold_text("INBOX", gone);

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
        assert_eq!(cursor_offset(None), Some(0));
        assert_eq!(cursor_offset(Some("c:100")), Some(100));
        assert_eq!(cursor_offset(Some("c:10000")), Some(CURSOR_MAX));
        for bad in ["c:10001", "l:0", "l:5", "c:-1", "x:1", "c", "c:abc", ""] {
            assert_eq!(cursor_offset(Some(bad)), None, "{bad}");
        }
    }

    /// Task E8.8b: the person reading a mail of an open entry counts the entry as opened; an agent
    /// reading it, or a mail of no entry, counts nothing, and the answer stays the same.
    #[tokio::test]
    async fn the_person_reading_a_mail_counts_its_entry_as_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with("Offer", 0, "By Friday"), &[]);
        server.add("INBOX", &mail_with("Other", 0, "Not filed"), &[]);
        let service = synced(dir.path(), &server).await;
        let (offer, other) = (id_of(&service, "Offer").await, id_of(&service, "Other").await);
        let inbox = crate::inbox::tests::inbox_in(dir.path());
        let mail = crate::inbox::NewMail {
            account: ANNA.into(),
            message: offer.clone(),
            thread: "t:offer".into(),
            subject: "Offer".into(),
            snippet: "By Friday".into(),
            from: None,
            bulk: None,
            list: None,
            github: None,
            date: Utc::now(),
            invite: false,
            auto_submitted: None,
            junk: false,
        };
        inbox.file_mail(&mail, torro_mail::Notify::All, Utc::now()).expect("filed").expect("an entry");
        let opened = || match inbox.handle(Request::GetStats { range: StatsRange::Week, until: None }, None, Utc::now())
        {
            Response::Stats(stats) => stats.totals.opened,
            other => panic!("statistics, got {other:?}"),
        };

        let rights = RightsTemplate::FullAccess.rights();
        let agent = message(answer_counting(&service, &inbox, Reach::Client(&rights), get(&offer, false)).await);
        assert_eq!((agent.summary.subject.as_str(), opened()), ("Offer", 0), "an agent reading is no opening");
        let read = message(answer_counting(&service, &inbox, Reach::Suite, get(&offer, true)).await);
        assert_eq!(read.summary.subject, "Offer");
        answer_counting(&service, &inbox, Reach::Suite, get(&offer, false)).await;
        answer_counting(&service, &inbox, Reach::Suite, get(&other, false)).await;
        assert_eq!(opened(), 1, "once a day, and only the mail of an entry");
        let missing = answer_counting(&service, &inbox, Reach::Suite, get("m:nope", false)).await;
        assert_eq!(code(&missing), "not-found");
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
            get_html(&id),
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

    /// A recent mail with a text and an HTML part; the HTML names an inline picture by its
    /// `Content-ID` and carries a script (task M8.1).
    fn mail_with_html() -> String {
        let date = Utc::now().to_rfc2822();
        format!(
            "From: News <news@example.com>\r\nTo: anna@example.org\r\nSubject: Weekly\r\nDate: {date}\r\n\
             Message-ID: <weekly@example.org>\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\n\
             Content-Type: multipart/alternative; boundary=\"a\"\r\n\r\n--a\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\nThe week in short.\r\n--a\r\n\
             Content-Type: text/html; charset=utf-8\r\n\r\n<p onclick=\"x()\">The <b>week</b></p>\
             <img src=\"cid:logo@example.com\"><script>x()</script>\r\n--a--\r\n--r\r\n\
             Content-Type: image/png\r\nContent-ID: <logo@example.com>\r\nContent-Disposition: inline\r\n\r\nPNG\r\n--r--\r\n"
        )
    }

    /// Task M8.1: the HTML comes only with `include_html`, cleaned, from the store; a text kept
    /// without HTML (from before) is fetched once more without marking the mail read.
    #[tokio::test]
    async fn html_comes_only_when_asked_and_is_fetched_once_when_the_store_lacks_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with_html(), &[]);
        let service = synced(dir.path(), &server).await;
        let id = id_of(&service, "Weekly").await;
        let peeks = |lines: Vec<String>| lines.iter().filter(|line| line.contains("BODY.PEEK[]")).count();
        let synced_peeks = peeks(server.lines());

        let plain = message(answer(&service, Reach::Suite, get(&id, true)).await);
        assert_eq!((plain.text.as_deref(), plain.html), (Some("The week in short."), None), "not asked for");
        let shown = message(answer(&service, Reach::Suite, get_html(&id)).await);
        let html = shown.html.expect("the HTML");
        assert!(html.contains("<b>week</b>") && html.contains("src=\"cid:logo@example.com\""), "{html}");
        assert!(!html.contains("script") && !html.contains("onclick"), "{html}");
        assert_eq!(shown.text.as_deref(), Some("The week in short."), "the text stays the text part");
        let logo = shown.attachments.iter().find(|attachment| attachment.inline).expect("the inline picture");
        assert_eq!(logo.content_id.as_deref(), Some("logo@example.com"));
        assert_eq!(peeks(server.lines()), synced_peeks, "kept by the sync, nothing fetched");
        let without_text = Request::GetMail { id: id.clone(), include_body: false, include_html: true };
        let only_html = message(answer(&service, Reach::Suite, without_text).await);
        assert_eq!((only_html.text, only_html.html.is_some()), (None, true));

        let stores = service.stores(Reach::Suite).await;
        let lookup = id.clone();
        stores[0].1.run(move |store| store.forget_html(&lookup)).await.expect("forgotten");
        let again = message(answer(&service, Reach::Suite, get(&id, true)).await);
        assert_eq!((again.html, peeks(server.lines())), (None, synced_peeks), "the text alone needs no fetch");
        let fetched = message(answer(&service, Reach::Suite, get_html(&id)).await);
        assert!(fetched.html.is_some_and(|html| html.contains("<b>week</b>")));
        assert!(!fetched.summary.flags.contains(&Flag::Seen));
        assert_eq!(peeks(server.lines()), synced_peeks + 1, "fetched once more");
        let lines = server.lines();
        assert!(!lines.iter().any(|line| line.contains(" BODY[") || line.contains("UID STORE")), "{lines:?}");
        message(answer(&service, Reach::Suite, get_html(&id)).await);
        assert_eq!(peeks(server.lines()), synced_peeks + 1, "kept now");
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
        let managed = service.attachment_cache().expect("a folder").dir().to_path_buf();
        assert!(path.starts_with(&managed), "{path:?}");
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("evil.txt"));
        assert_eq!(std::fs::read(&path).expect("written"), b"hello attachment");
        assert_eq!((size, content_type.as_str()), (16, "text/plain"));

        let missing = Request::GetMailAttachment { id, attachment: "9".into() };
        assert_eq!(code(&answer(&service, Reach::Suite, missing).await), "not-found");
    }

    /// Issue #136: old texts are put into the index in the background and found locally, offline
    /// too; until then a search that needs texts says it may be incomplete, and the server is never
    /// asked to search.
    #[tokio::test]
    async fn old_texts_are_indexed_in_the_background_and_found_without_the_server() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with("Recent zebra", 1, "nothing here"), &[]);
        server.add("INBOX", &mail_with("Old one", 200, "a zebra and Müller"), &[]);
        server.add("Archive", &mail_with("Older one", 300, "no stripes"), &[]);
        let service = synced(dir.path(), &server).await;
        let status = || async {
            match answer(&service, Reach::Suite, Request::MailIndexStatus { account: None }).await {
                Response::MailIndexStatus { accounts } => accounts,
                other => panic!("a status, got {other:?}"),
            }
        };
        let before = status().await;
        assert_eq!(
            before.iter().map(|p| (p.account.as_str(), p.messages, p.indexed)).collect::<Vec<_>>(),
            [(ANNA, 3, 1)],
            "only the recent text came with the sync"
        );
        assert_eq!(before[0].state, torro_ipc::mail::IndexState::Waiting, "no indexer runs in tests");

        let incomplete = answer(&service, Reach::Suite, search(Some("zebra"), 10, None)).await;
        let Response::MailMessages { messages, complete, incomplete_reason, total_estimate, .. } = incomplete else {
            panic!("a page")
        };
        assert_eq!(messages.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["Recent zebra"]);
        assert_eq!((complete, incomplete_reason, total_estimate), (false, Some(IncompleteReason::Indexing), Some(1)));
        let headers = answer(&service, Reach::Suite, search(Some("subject:older"), 10, None)).await;
        let Response::MailMessages { messages, complete, .. } = headers else { panic!("a page") };
        assert!(complete && messages.len() == 1, "headers of every message are searchable at once");

        let peeks = |lines: Vec<String>| lines.iter().filter(|line| line.contains("BODY.PEEK[]")).count();
        let fetched_before = peeks(server.lines());
        assert_eq!(service.backfill_now(ANNA, 25).await.expect("indexed"), 2);
        assert_eq!(peeks(server.lines()), fetched_before + 2);
        assert_eq!(service.backfill_now(ANNA, 25).await.expect("nothing left"), 0);
        let after = status().await;
        assert_eq!((after[0].indexed, after[0].state), (3, torro_ipc::mail::IndexState::Complete));
        let old = id_of(&service, "Old one").await;
        assert_eq!(message(answer(&service, Reach::Suite, get(&old, false)).await).text, None, "indexed, not kept");

        // Offline: the server is gone, the search is local.
        drop(server);
        let (found, last, source) = page(answer(&service, Reach::Suite, search(Some("zebra"), 1, None)).await);
        assert_eq!((found, source), (vec!["Recent zebra".to_string()], MailSource::Cache));
        let (rest, end, _) = page(answer(&service, Reach::Suite, search(Some("zebra"), 1, last)).await);
        assert_eq!((rest, end), (vec!["Old one".to_string()], None));
        let (found, ..) = page(answer(&service, Reach::Suite, oldest(Some("zebra"), 10, None)).await);
        assert_eq!(found, ["Old one", "Recent zebra"]);
        for spelling in ["müller", "mueller", "MULLER"] {
            let (found, ..) = page(answer(&service, Reach::Suite, search(Some(spelling), 10, None)).await);
            assert_eq!(found, ["Old one"], "{spelling}");
        }
        let (found, ..) = page(answer(&service, Reach::Suite, search(Some("zebra -in:inbox"), 10, None)).await);
        assert!(found.is_empty(), "both are in the inbox");
        let (found, ..) = page(answer(&service, Reach::Suite, search(Some("stripes in:archive"), 10, None)).await);
        assert_eq!(found, ["Older one"]);
        let complete = answer(&service, Reach::Suite, search(Some("zebra"), 10, None)).await;
        assert!(matches!(complete, Response::MailMessages { complete: true, incomplete_reason: None, .. }));
    }

    /// Issue #136: highlights come from the message's own text, the first page counts, relevance
    /// ranks the subject first, and a query that cannot be read is an error.
    #[tokio::test]
    async fn pages_carry_highlights_counts_and_ranks() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &mail_with("Ihre Stromrechnung", 5, "Guten Tag, anbei die Rechnung für Oktober."), &[]);
        server.add("INBOX", &mail_with("Lunch", 1, "Bring the invoice for the rechnung please"), &[]);
        server.add("INBOX", &mail_with("Weekly", 0, "Nothing to see"), &[]);
        // BM25 weighs a word by how rare it is: a word in most mails says nothing.
        for day in 10..16 {
            server.add("Archive", &mail_with(&format!("Note {day}"), day, "Plain words only"), &[]);
        }
        let service = synced(dir.path(), &server).await;

        let response = answer(&service, Reach::Suite, search(Some("rechnung"), 1, None)).await;
        let Response::MailMessages { messages, highlights, total_estimate, cursor, .. } = response else {
            panic!("a page")
        };
        assert_eq!((messages[0].subject.as_str(), total_estimate), ("Lunch", Some(2)), "newest first");
        let snippet: Vec<(&str, bool)> = highlights[0].snippet.iter().map(|s| (s.text.as_str(), s.hit)).collect();
        assert_eq!(snippet, [("Bring the invoice for the ", false), ("rechnung", true), (" please", false)]);
        assert!(highlights[0].subject.is_empty(), "no match in the subject");
        let next = answer(&service, Reach::Suite, search(Some("rechnung"), 1, cursor)).await;
        let Response::MailMessages { highlights, total_estimate, .. } = next else { panic!("a page") };
        assert_eq!(total_estimate, None, "counted on the first page only");
        let subject: Vec<(&str, bool)> = highlights[0].subject.iter().map(|s| (s.text.as_str(), s.hit)).collect();
        assert_eq!(subject, [("Ihre Strom", false), ("rechnung", true)]);

        let (ranked, ..) = page(answer(&service, Reach::Suite, relevant("rechnung", 10)).await);
        assert_eq!(ranked, ["Ihre Stromrechnung", "Lunch"], "the subject and the text before the text alone");
        let none = answer(&service, Reach::Suite, search(Some("is:unread"), 10, None)).await;
        assert!(matches!(none, Response::MailMessages { ref highlights, .. } if highlights.is_empty()));
        let bad = answer(&service, Reach::Suite, search(Some("before:soon"), 10, None)).await;
        assert_eq!(code(&bad), "error");
        let unknown =
            answer(&service, Reach::Suite, Request::MailIndexStatus { account: Some("mail:x@y.z".into()) }).await;
        assert_eq!(code(&unknown), "not-found");
    }

    // ----- invitations (task E8.3b) -----

    /// The invitation of `tests/fixtures/mail/invitation.eml` (Jonas invites Anna to "Planning, Q4"),
    /// sent now, with the Message-ID `<mid>@example.com`, the UID `planning-<mid>@example.com` and the
    /// event from `start` to `end`; CRLF line ends, as servers hand out mail.
    pub(crate) fn invitation_mail(mid: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mail/invitation.eml");
        let text = std::fs::read_to_string(path).expect("the fixture");
        let ics = |at: DateTime<Utc>| at.format("%Y%m%dT%H%M%SZ").to_string();
        text.replace("{DATE}", &Utc::now().to_rfc2822())
            .replace("{MID}", mid)
            .replace("{START}", &ics(start))
            .replace("{END}", &ics(end))
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
    }

    fn in_a_day() -> (DateTime<Utc>, DateTime<Utc>) {
        let start = (Utc::now() + Duration::days(1)).with_nanosecond(0).expect("whole seconds");
        (start, start + Duration::hours(1))
    }

    use chrono::Timelike;

    #[test]
    fn an_invitation_is_read_from_its_calendar_part() {
        let (start, end) = in_a_day();
        let raw = invitation_mail("p1", start, end).into_bytes();
        let read = mime::read(&raw).expect("read");
        assert!(read.invitation);
        let invitation = invitation_in(&raw, &read).expect("an invitation");
        assert_eq!(invitation.method, "REQUEST");
        assert_eq!(invitation.event.uid, "planning-p1@example.com");
        assert_eq!(invitation.event.summary, "Planning, Q4");
        assert_eq!(invitation.event.start, torro_pim::EventTime::DateTime(start));
        assert_eq!(invitation.event.location.as_deref(), Some("Room 3"));
        assert_eq!(invitation.event.description.as_deref(), Some("Agenda follows."), "not the alarm's text");
        assert_eq!(invitation.sequence, 2);
        let jonas = MailAddress { name: Some("Jonas Weber".into()), address: "jonas@example.com".into() };
        assert_eq!(invitation.organizer, Some(jonas));
        let attendees: Vec<&str> = invitation.attendees.iter().map(|who| who.address.as_str()).collect();
        assert_eq!(attendees, ["Anna@Example.org", "carla@example.net"], "the folded line is joined");
        assert!(invitation.answerable() && !invitation.ended(Utc::now()) && invitation.ended(end));
        let invite = invite_of(&raw, &read).expect("an invite");
        assert_eq!(
            invite,
            Invite { title: "Planning, Q4".into(), start, end: Some(end), all_day: false, answered: None }
        );
        assert!(!read.text.contains("BEGIN:VCALENDAR"), "the text is the plain part");
    }

    #[test]
    fn only_requests_for_one_event_can_be_answered() {
        let (start, end) = in_a_day();
        let raw = invitation_mail("p2", start, end);
        let with = |from: &str, to: &str| {
            let raw = raw.replace(from, to).into_bytes();
            let read = mime::read(&raw).expect("read");
            (invitation_in(&raw, &read), invite_of(&raw, &read))
        };
        let (cancel, invite) = with("METHOD:REQUEST", "METHOD:CANCEL");
        assert_eq!(cancel.map(|invitation| invitation.method), Some("CANCEL".into()));
        assert_eq!(invite, None, "a cancellation is no invitation to answer");
        let (series, invite) = with("SEQUENCE:2", "SEQUENCE:2\r\nRRULE:FREQ=WEEKLY");
        assert!(series.is_some_and(|invitation| invitation.event.recurring) && invite.is_none());
        let (nobody, invite) = with("ORGANIZER;CN=\"Jonas Weber\":mailto:jonas@example.com", "X-NONE:1");
        assert!(nobody.is_some_and(|invitation| invitation.organizer.is_none()) && invite.is_none());
        let (bad, _) = with("mailto:jonas@example.com", "mailto:jonas@example.com>, eve@evil.example");
        assert_eq!(bad.and_then(|invitation| invitation.organizer), None, "one plain address or none");
        let plain = mail_with("Hello", 0, "No calendar here.").into_bytes();
        let read = mime::read(&plain).expect("read");
        assert_eq!((invitation_in(&plain, &read), invite_of(&plain, &read)), (None, None));
    }

    /// The sync reads what an invitation invites to, and lists and the message carry it.
    #[tokio::test]
    async fn synced_invitations_show_in_their_summaries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let (start, end) = in_a_day();
        server.add("INBOX", &invitation_mail("p3", start, end), &[]);
        server.add("INBOX", &mail_with("Hello", 0, "Just text."), &[]);
        let service = synced(dir.path(), &server).await;
        let listed = match answer(&service, Reach::Suite, search(None, 10, None)).await {
            Response::MailMessages { messages, .. } => messages,
            other => panic!("messages, got {other:?}"),
        };
        let invite = |subject: &str| listed.iter().find(|m| m.subject == subject).and_then(|m| m.invite.clone());
        assert_eq!(
            invite("Invitation: Planning").map(|invite| (invite.title, invite.start)),
            Some(("Planning, Q4".into(), start))
        );
        assert_eq!(invite("Hello"), None);
        let id = id_of(&service, "Invitation: Planning").await;
        let shown = message(answer(&service, Reach::Suite, get(&id, true)).await);
        assert!(shown.invitation && shown.summary.invite.is_some());
        let fetched = fetch_invitation(&service, ANNA, &id).await.expect("fetched");
        assert_eq!(fetched.invitation.map(|invitation| invitation.sequence), Some(2));
        assert_eq!(fetched.place.id, id);
        assert!(
            matches!(fetch_invitation(&service, ANNA, "m:nope").await, Err(answer) if code(&answer) == "not-found")
        );
    }
}
