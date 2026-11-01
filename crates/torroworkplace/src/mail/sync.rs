//! The sync of one mail account (task M2.6): lists the folders and their roles, then brings each
//! folder of the store in line with the server. [`MailService`](super::service::MailService) runs
//! it: [`sync_account`] on the command connection every five minutes and on `accounts.sync`,
//! [`sync_inbox`] on the IDLE connection whenever the server reports a change in the inbox.
//!
//! **One folder** ([`sync_folder`]):
//!
//! 1. `SELECT` it and tell the store its UIDVALIDITY; a new value drops what was cached (the UIDs
//!    mean other messages now) and the folder is synced from scratch.
//! 2. With CONDSTORE, an unchanged HIGHESTMODSEQ and the same number of messages, nothing changed:
//!    done. Otherwise `UID SEARCH ALL` and the flags (only those changed since the last
//!    HIGHESTMODSEQ with CONDSTORE, else all) are compared with the store: messages gone from the
//!    server (or marked `\Deleted`) are dropped, changed flags taken over.
//! 3. Headers of the new messages, **newest first, in batches** of [`HEADER_BATCH`]: a first sync
//!    of a large inbox shows the latest mail at once, and the command connection is handed back
//!    between batches, so commands run in between.
//! 4. Texts selected by the account's offline policy, newest first (the core
//!    decides, task M2.5); bigger than [`BODY_PREFETCH_MAX`], or older, they are fetched when
//!    someone opens them (task M3.1). Fetched with `BODY.PEEK[]`: syncing never marks mail read.
//! 5. Only a folder synced to the end remembers its HIGHESTMODSEQ, so an interrupted sync starts
//!    over where it matters next time. The same for a folder whose flags were not all taken over:
//!    flags of messages whose flags are being changed (`mail.set_flags`, the inbox marking mail
//!    read), or were changed while the sync ran, stay as the store has them, also when such a
//!    message comes back with the new ones in step 3, because the server may have reported them
//!    from before the change
//!    ([`FlagsUnderway`](super::service::FlagsUnderway), issue #318); the next sync compares them
//!    again.
//! 6. Sizes the store lacks (rows from before it kept `RFC822.SIZE`, DS.1) are asked for, at most
//!    [`SIZE_FILL_MAX`] per folder and run ([`fill_sizes`]), also for a folder that did not change.
//!
//! After the folders and the clean-up of texts and originals the account's storage quota is read once (`GETQUOTAROOT "INBOX"`, only from
//! servers with `QUOTA`) and kept for `mail.account_stats`; a server without one clears it.
//!
//! Every step that found something sends a [`MailChange`].
//!
//! **Older texts for the search** ([`backfill`], issue #136): the texts the sync does not fetch
//! ahead (outside the offline policy, or bigger than [`BODY_PREFETCH_MAX`]) are fetched
//! afterwards by the account's indexer task, newest first, a message at a time, and put into the
//! search index: older ones only into the index, recent ones kept to read as well.
//!
//! Everything read from the server is data from outside: it is parsed (`mime::read`), stored and
//! never acted on.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex as StdMutex, PoisonError};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use torro_ipc::mail::{MailCacheMode, MailFolderSyncState, MailQuota, MailSyncPhase, MailSyncTrigger};
use torro_mail::{Flag, Role};
use torro_mail_imap::folders;
use torro_mail_imap::imap::{BackupMessage, BodyChunks, FetchedHeader, ImapError, Search, Session};
use torro_mail_imap::mime::{self, ReadMessage};

use super::MailStoreError;
use super::progress::SyncProgress;
use super::service::{Command, FlagsUnderway, MailChange, MailError, Mailbox, SharedStore};
use super::store::{Body, MailStore, NewMessage, ORIGINAL_MAX_BYTES, UidValidity};

/// How many headers one `UID FETCH` asks for.
pub const HEADER_BATCH: usize = 100;

/// Texts of messages bigger than this (5 MiB, attachments included) are not fetched ahead.
pub const BODY_PREFETCH_MAX: u32 = 5 << 20;

/// Beyond the IMAP parser's full-literal ceiling, keep the original through bounded partial
/// reads. Tests use a smaller threshold to exercise the same path without a 100-MiB fixture.
#[cfg(not(test))]
pub(crate) const LARGE_ORIGINAL: u64 = 100 << 20;
#[cfg(test)]
pub(crate) const LARGE_ORIGINAL: u64 = 32 << 20;

/// How many texts one step of [`backfill`] fetches before the indexer looks again.
pub const BACKFILL_BATCH: u32 = 25;

/// At most this many missing message sizes are filled in per folder and sync ([`fill_sizes`]):
/// an old account gets its folder sizes over a few syncs instead of in one burst.
pub const SIZE_FILL_MAX: u32 = 2000;

/// How many sizes one `UID FETCH` of [`fill_sizes`] asks for.
pub const SIZE_FILL_BATCH: usize = 500;

/// A folder to sync: its id in the store, its path on the server, its name for people and its role.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    id: String,
    path: String,
    name: String,
    role: Option<Role>,
}

/// Where a folder sync gets its connection from.
enum Conn<'a> {
    /// The IDLE connection's own session, held for the whole sync.
    Own(&'a mut Session),
    /// The account's command connection, taken for each step and handed back after it.
    Commands(&'a Mailbox),
}

/// A connection for one step of a sync ([`Conn::lease`]).
enum Lease<'a> {
    Own(&'a mut Session),
    // Boxed: a session with its TLS state is large, a reference small.
    Command(Box<Command>),
}

impl Conn<'_> {
    async fn lease(&mut self) -> Result<Lease<'_>, MailError> {
        match self {
            Conn::Own(session) => Ok(Lease::Own(session)),
            Conn::Commands(mailbox) => Ok(Lease::Command(Box::new(mailbox.command().await?))),
        }
    }
}

impl Lease<'_> {
    fn session(&mut self) -> &mut Session {
        match self {
            Lease::Own(session) => session,
            Lease::Command(command) => command.session(),
        }
    }

    /// The step worked: the command connection goes back for the next one. Dropping a lease
    /// instead closes the command connection (see [`Command`]).
    fn done(self) {
        if let Lease::Command(command) = self {
            command.release();
        }
    }
}

/// Syncs the whole account on its command connection: lists the folders (creating a missing
/// drafts or sent folder, task M2.3), keeps the store's folder list in line, then syncs every
/// folder, the inbox first, and applies its account's body-retention policy; originals of mail
/// gone from the server for [`GONE_ORIGINAL_RETENTION_DAYS`](super::store::GONE_ORIGINAL_RETENTION_DAYS)
/// are deleted. A folder that vanished on the server meanwhile is skipped; any other failure ends
/// the sync with that error.
pub(crate) async fn sync_account(mailbox: &Mailbox, trigger: MailSyncTrigger) -> Result<(), MailError> {
    let progress = mailbox.progress();
    let queued = progress.queue();
    let _one = mailbox.one_sync().await;
    drop(queued);
    let run = progress.begin(trigger);
    let result = sync_all_folders(mailbox).await;
    run.end(result.as_ref().map(|_| ()));
    result
}

/// The run of [`sync_account`], reporting each step to the account's
/// [`SyncProgress`](super::progress::SyncProgress) (issue #422).
async fn sync_all_folders(mailbox: &Mailbox) -> Result<(), MailError> {
    let progress = mailbox.progress();
    let store = mailbox.store()?;
    let mut command = mailbox.command().await?;
    let list = folders::discover(command.session(), mailbox.folder_roles()).await?;
    command.release();
    // For the account's page (task M2.8): which roles are still open, without asking again.
    mailbox.note_folders(&list);
    // Only folders that hold messages; the others only hold folders.
    let listed: Vec<folders::RemoteFolder> = list.folders.into_iter().filter(|folder| folder.selectable).collect();
    let mut targets = store
        .run(move |store| {
            let mut targets = Vec::with_capacity(listed.len());
            for folder in listed {
                let id = store.upsert_folder(&folder.path, &folder.name, folder.role)?;
                // For a local backup's folder table (MB.4), as the server lists it.
                store.set_folder_delimiter(&folder.path, folder.delimiter)?;
                targets.push(Target { id, path: folder.path, name: folder.name, role: folder.role });
            }
            // An empty list would drop every folder; a server always has an inbox.
            if !targets.is_empty() {
                let paths: Vec<String> = targets.iter().map(|target| target.path.clone()).collect();
                store.retain_folders(&paths)?;
            }
            Ok(targets)
        })
        .await?;
    targets.sort_by_key(|target| target.role != Some(Role::Inbox));
    if let Some(inbox) = targets.iter().find(|target| target.role == Some(Role::Inbox)) {
        mailbox.set_inbox_path(&inbox.path);
    }
    let names: Vec<(String, String)> = targets.iter().map(|target| (target.id.clone(), target.name.clone())).collect();
    progress.listed(&names);
    let mut conn = Conn::Commands(mailbox);
    for (index, target) in targets.iter().enumerate() {
        let folder = progress.folder_begins(Some(index), &target.id);
        match sync_folder(&mut conn, mailbox, &store, target, Utc::now()).await {
            Ok(outcome) => folder.end(outcome),
            Err(MailError::Server(ImapError::NotFound(_))) => folder.gone(),
            Err(err) => {
                folder.end(MailFolderSyncState::Failed);
                return Err(err);
            }
        }
    }
    progress.finishing();
    store
        .run(|store| {
            store.prune_bodies(Utc::now())?;
            store.prune_gone_originals(Utc::now()).map(drop)
        })
        .await?;
    // Last, so a connection lost here costs no clean-up; a lost connection or a refused sign-in is
    // still the sync's error, so the account shows it and `refused` keeps the password from being
    // tried again.
    note_quota(mailbox, &store).await
}

/// Reads the account's storage quota (`GETQUOTAROOT "INBOX"`) and keeps it for
/// `mail.account_stats`; a server without `QUOTA`, or without a limit on storage, clears a kept
/// one. A server that refuses the command keeps the old value and the sync goes on; only a lost
/// connection or account is an error.
async fn note_quota(mailbox: &Mailbox, store: &SharedStore) -> Result<(), MailError> {
    let mut command = mailbox.command().await?;
    let quota = match command.session().quota_root("INBOX").await.map_err(MailError::from) {
        Ok(quota) => {
            command.release();
            quota
        }
        Err(err) if connection_lost(&err) => return Err(err),
        Err(_) => {
            // The account and a fixed reason only, never the server's words.
            eprintln!("torroworkplace: the storage quota of {} was not read: the server refused", mailbox.id());
            return Ok(());
        }
    };
    let quota = quota.map(|quota| MailQuota {
        used_bytes: quota.used_kib.saturating_mul(1024),
        limit_bytes: quota.limit_kib.saturating_mul(1024),
        checked: Utc::now(),
    });
    store.run(move |store| store.set_quota(quota.as_ref())).await?;
    Ok(())
}

/// Syncs the inbox on `session`, the IDLE connection, which keeps the inbox open afterwards. Does
/// nothing before [`sync_account`] listed the folders once.
pub(crate) async fn sync_inbox(session: &mut Session, mailbox: &Mailbox) -> Result<(), MailError> {
    let _one = mailbox.one_sync().await;
    let store = mailbox.store()?;
    let path = mailbox.inbox_path();
    let lookup = path.clone();
    let Some(folder) = store.run(move |store| store.folder_by_path(&lookup)).await? else {
        return Ok(());
    };
    // The name is the listing's, which `sync_account` reports; here none is needed.
    let target = Target { id: folder.id, path, name: String::new(), role: Some(Role::Inbox) };
    // Only the inbox's record: this is no run of all folders (issue #422).
    let progress = mailbox.progress();
    let folder = progress.folder_begins(None, &target.id);
    match sync_folder(&mut Conn::Own(session), mailbox, &store, &target, Utc::now()).await {
        Ok(outcome) => {
            folder.end(outcome);
            Ok(())
        }
        Err(err) => {
            folder.end(MailFolderSyncState::Failed);
            Err(err)
        }
    }
}

/// Puts the texts of up to `limit` messages that are not in the search index yet into it (issue
/// #136), newest first, and answers how many it handled; 0 when every message is indexed. Each
/// text is fetched with `BODY.PEEK[]` (never marked read) on the command connection, taken for
/// one message at a time, so commands and the sync run in between. A body is kept for reading
/// when the account policy includes it; its search text is indexed independently. One gone from the server, or one that
/// cannot be read, is marked as having no text and not asked for again; its headers stay
/// searchable. Notes at the mailbox whether it is indexing (`mail.index_status`): after a failure
/// (server offline, password refused) it is waiting.
pub(crate) async fn backfill(mailbox: &Mailbox, limit: u32, now: DateTime<Utc>) -> Result<usize, MailError> {
    let store = mailbox.store()?;
    let policy = store.run(|store| store.cache_policy()).await?;
    let todo = store.run(move |store| store.unindexed(limit)).await?;
    mailbox.note_indexing(!todo.is_empty());
    let ids: Vec<String> = todo.iter().map(|(place, _)| place.id.clone()).collect();
    let policy_to_check = policy.clone();
    let keep = store.run(move |store| store.should_cache_bodies(&ids, &policy_to_check, now)).await?;
    for ((place, _date), keep) in todo.iter().zip(keep) {
        let fetched = fetch_text(mailbox, place).await;
        if fetched.is_err() {
            mailbox.note_indexing(false);
        }
        let raw = fetched?;
        let id = place.id.clone();
        store
            .run(move |store| {
                let Some(raw) = raw else { return store.mark_unindexable(&id).map(drop) };
                let Ok(read) = mime::read(&raw) else { return store.mark_unindexable(&id).map(drop) };
                if keep {
                    let invite = super::read::invite_of(&raw, &read);
                    return store.set_body(&id, &Body::of(read, invite)).map(drop);
                }
                let names: Vec<&str> = read.attachments.iter().filter_map(|part| part.filename.as_deref()).collect();
                store.index_text(&id, &read.text, &names).map(drop)
            })
            .await?;
    }
    if !todo.is_empty() {
        store.run(move |store| store.note_index_run(now)).await?;
    }
    Ok(todo.len())
}

/// The whole message at `place` from the account's command connection; `None` when it is gone, or
/// larger than [`LARGE_ORIGINAL`] (then only its original is kept, in `AllOffline`). Fails only when
/// the connection does: a mail whose original cannot be kept is read anyway ([`fetch_for_reading`]).
async fn fetch_text(mailbox: &Mailbox, place: &super::store::MessageRef) -> Result<Option<Vec<u8>>, MailError> {
    let mut command = mailbox.command().await?;
    let store = mailbox.store()?;
    let retain = store.run(|store| store.cache_policy()).await?.mode == MailCacheMode::AllOffline;
    if !super::read::valid_selection(command.session(), place).await? {
        return Ok(None);
    }
    let fetched = fetch_for_reading(command.session(), &store, place, retain).await?;
    command.release();
    Ok(match fetched {
        Fetched::Whole(raw) => Some(raw),
        // A message this large is not parsed into memory by the normal MIME cache.
        Fetched::TooLarge { .. } | Fetched::Gone => None,
    })
}

/// What [`fetch_for_reading`] brought.
#[derive(Debug)]
pub(crate) enum Fetched {
    /// The whole message, as the server holds it.
    Whole(Vec<u8>),
    /// Larger than [`LARGE_ORIGINAL`]: not read into memory. `retained`: its original is in the
    /// local depot (it was, or it was taken now in bounded pieces).
    TooLarge { retained: bool },
    /// Gone from the server.
    Gone,
}

/// The whole message at `place` for reading, on `session`, which has the folder open under
/// `place.uidvalidity` ([`super::read::valid_selection`]); never marked read. With `retain` (the
/// account keeps all texts offline) its RFC-822 original also goes into the local depot, with
/// `INTERNALDATE` and flags, unless the depot holds one for this place already: then nothing more
/// is asked or written, and a large one is not fetched at all.
///
/// Keeping the original never stops the reading: a server that names no `INTERNALDATE` or flags, a
/// message that changes or vanishes while it is taken, one longer than its `RFC822.SIZE` or larger
/// than the depot takes ([`ORIGINAL_MAX_BYTES`]) is logged without contents and left missing, which
/// a local backup then reports. Errors only for the connection (offline, refused password) and the
/// store's lookup; a server error while fetching the text itself is the caller's as before.
pub(crate) async fn fetch_for_reading(
    session: &mut Session,
    store: &SharedStore,
    place: &super::store::MessageRef,
    retain: bool,
) -> Result<Fetched, MailError> {
    if !retain {
        return whole(session, place).await;
    }
    let (path, validity, uid) = (place.folder_path.clone(), place.uidvalidity, place.uid);
    match store.run(move |db| db.original_size(&path, validity, uid)).await? {
        Some(bytes) if bytes > LARGE_ORIGINAL => return Ok(Fetched::TooLarge { retained: true }),
        Some(_) => return whole(session, place).await,
        None => {}
    }
    let metadata = match session.fetch_backup_metadata(&[place.uid]).await.map_err(MailError::from) {
        Ok(found) => found.into_iter().find(|item| item.uid == place.uid),
        Err(err) if connection_lost(&err) => return Err(err),
        Err(err) => {
            not_retained(place, &err);
            return whole(session, place).await;
        }
    };
    // The server named no such UID: the plain fetch tells whether it is gone.
    let Some(metadata) = metadata else { return whole(session, place).await };
    if metadata.bytes > LARGE_ORIGINAL {
        let retained = match retain_large_original(session, store, place, &metadata).await {
            Ok(()) => true,
            Err(err) if connection_lost(&err) => return Err(err),
            Err(err) => {
                not_retained(place, &err);
                false
            }
        };
        return Ok(Fetched::TooLarge { retained });
    }
    let Fetched::Whole(raw) = whole(session, place).await? else { return Ok(Fetched::Gone) };
    let path = place.folder_path.clone();
    let validity = place.uidvalidity;
    let (raw, kept) = store
        .run(move |db| {
            let kept = db.save_original(&path, validity, metadata.uid, metadata.internal_date, &metadata.flags, &raw);
            Ok((raw, kept))
        })
        .await?;
    if let Err(err) = kept {
        not_retained(place, &MailError::Store(err));
    }
    Ok(Fetched::Whole(raw))
}

/// `BODY.PEEK[]` of `place` in the open folder.
async fn whole(session: &mut Session, place: &super::store::MessageRef) -> Result<Fetched, MailError> {
    Ok(match session.fetch_message(place.uid).await? {
        Some(raw) => Fetched::Whole(raw),
        None => Fetched::Gone,
    })
}

/// The failure is about the connection or the account, not about this one message: reading stops
/// and is tried again later. Anything else only leaves the original missing.
fn connection_lost(err: &MailError) -> bool {
    matches!(
        err,
        MailError::NotFound
            | MailError::NoPassword
            | MailError::Auth
            | MailError::Offline(_)
            | MailError::NotPublic
            | MailError::Server(ImapError::Offline(_) | ImapError::Auth(_))
    )
}

/// Logs that the original of the message at `place` was not kept: the account and a fixed reason,
/// never a server text, a folder name or contents.
fn not_retained(place: &super::store::MessageRef, err: &MailError) {
    let reason = match err {
        MailError::Store(super::MailStoreError::OriginalTooLarge) => "too large for the local store",
        MailError::Store(_) => "the local store failed",
        MailError::Server(ImapError::NotFound(_)) => "it vanished while it was read",
        _ => "the server answered unexpectedly",
    };
    eprintln!("torroworkplace: an original of {} was not kept: {reason}", place.account);
}

/// Fetch a large mail in bounded pieces, then atomically place it in the local original depot.
/// Fails without keeping anything for a mail larger than the depot takes, one that vanishes or
/// changes while it is read, and one with bytes past its `RFC822.SIZE`: a cut original must never
/// pass for the whole one. The caller leaves such an original missing ([`fetch_for_reading`]).
pub(crate) async fn retain_large_original(
    session: &mut Session,
    store: &SharedStore,
    place: &super::store::MessageRef,
    metadata: &BackupMessage,
) -> Result<(), MailError> {
    use std::io::Write;

    // Checked before anything is fetched: the store would refuse it at the end.
    if metadata.bytes > ORIGINAL_MAX_BYTES {
        return Err(MailError::Store(super::MailStoreError::OriginalTooLarge));
    }
    let mut file = tempfile::tempfile().map_err(super::MailStoreError::Io)?;
    // Checks that nothing follows RFC822.SIZE, so a cut original never passes for the whole one.
    let mut body = BodyChunks::new(place.uid, metadata.bytes);
    while let Some(chunk) = body.next(session).await.map_err(ImapError::from)? {
        file.write_all(&chunk).map_err(super::MailStoreError::Io)?;
    }
    let after = session.fetch_backup_metadata(&[place.uid]).await?.into_iter().find(|item| item.uid == place.uid);
    if after.as_ref() != Some(metadata) {
        return Err(MailError::Server(ImapError::Protocol("original metadata changed during capture".into())));
    }
    let path = place.folder_path.clone();
    let validity = place.uidvalidity;
    let item = metadata.clone();
    store
        .run(move |db| db.save_original_file(&path, validity, item.uid, item.internal_date, &item.flags, &mut file))
        .await?;
    Ok(())
}

/// Fetch one complete reading body for an offline maintenance job, without setting Seen.
pub(crate) async fn cache_body(mailbox: &Mailbox, place: &super::store::MessageRef) -> Result<bool, MailError> {
    let Some(raw) = fetch_text(mailbox, place).await? else { return Ok(false) };
    if mailbox.store()?.run(|store| store.cache_policy()).await?.prefetch_attachments {
        let _ = prefetch_attachments(mailbox, &place.id, &raw).await;
    }
    let Ok(read) = mime::read(&raw) else { return Ok(false) };
    let invite = super::read::invite_of(&raw, &read);
    let body = Body::of(read, invite);
    let id = place.id.clone();
    Ok(mailbox.store()?.run(move |store| store.set_body(&id, &body)).await?)
}

pub(crate) async fn prefetch_message_attachments(
    mailbox: &Mailbox,
    place: &super::store::MessageRef,
) -> Result<(), MailError> {
    if let Some(raw) = fetch_text(mailbox, place).await? {
        prefetch_attachments(mailbox, &place.id, &raw).await?;
    }
    Ok(())
}

async fn prefetch_attachments(mailbox: &Mailbox, message: &str, raw: &[u8]) -> Result<usize, MailError> {
    let Some(cache) = mailbox.attachment_cache() else { return Ok(0) };
    let Ok(read) = mime::read(raw) else { return Ok(0) };
    let parts: Vec<_> =
        read.attachments.into_iter().filter(|part| part.size <= super::read::ATTACHMENT_MAX_BYTES as u64).collect();
    if parts.is_empty() {
        return Ok(0);
    }
    let account = mailbox.id().to_string();
    let message = message.to_string();
    let raw = raw.to_vec();
    tokio::task::spawn_blocking(move || {
        let mut saved = 0;
        for part in parts {
            if let Ok(content) = mime::attachment(&raw, &part.part)
                && content.bytes.len() <= super::read::ATTACHMENT_MAX_BYTES
            {
                let name = content.filename.unwrap_or_default();
                cache.store_for_part(&account, &message, &part.part, &name, &content.bytes)?;
                saved += 1;
            }
        }
        Ok::<usize, super::MailStoreError>(saved)
    })
    .await
    .map_err(|err| MailError::Store(super::MailStoreError::Io(std::io::Error::other(err.to_string()))))?
    .map_err(Into::into)
}

/// What the store knew of a folder before this sync.
struct Known {
    validity: UidValidity,
    /// HIGHESTMODSEQ of the last complete sync under the same UIDVALIDITY.
    modseq: Option<u64>,
    /// UIDs and flags.
    messages: BTreeMap<u32, Vec<Flag>>,
    /// Ids dropped because the server renumbered the folder.
    dropped: Vec<String>,
}

/// Brings one folder of the store in line with the server; see the module documentation. Answers how
/// it ended: up to date, synced, or interrupted because the server renumbered it meanwhile; each step
/// is reported to the account's progress (issue #422).
async fn sync_folder(
    conn: &mut Conn<'_>,
    mailbox: &Mailbox,
    store: &SharedStore,
    target: &Target,
    now: DateTime<Utc>,
) -> Result<MailFolderSyncState, MailError> {
    let progress = mailbox.progress();
    let mut lease = conn.lease().await?;
    let selected = lease.session().select(&target.path).await?;
    progress.selected(selected.exists);
    let condstore = lease.session().capabilities().has("CONDSTORE");
    let uidvalidity = selected.uid_validity;
    // Before the flags are asked for: a flag change that ends from now on is held below.
    mailbox.underway().lock().unwrap_or_else(PoisonError::into_inner).sync_started(&target.id);
    let known = know(store, &target.id, uidvalidity).await?;
    let unchanged = known.validity == UidValidity::Unchanged
        && condstore
        && selected.highest_modseq.is_some()
        && selected.highest_modseq == known.modseq
        && selected.exists as usize == known.messages.len();
    if unchanged {
        lease.done();
        return Ok(match fill_sizes(conn, progress, store, target, uidvalidity).await? {
            true => MailFolderSyncState::UpToDate,
            false => MailFolderSyncState::Interrupted,
        });
    }
    let on_server: BTreeSet<u32> = lease.session().uid_search(&Search::All).await?.into_iter().collect();
    let states = match known.messages.is_empty() {
        true => Vec::new(),
        false => lease.session().fetch_flags(known.modseq.filter(|_| condstore)).await?,
    };
    lease.done();

    // What went, and which flags changed.
    let deleted: BTreeSet<u32> = states.iter().filter(|state| state.deleted).map(|state| state.uid).collect();
    let gone: Vec<u32> =
        known.messages.keys().filter(|uid| !on_server.contains(uid) || deleted.contains(uid)).copied().collect();
    let changed: Vec<(u32, Vec<Flag>)> = states
        .iter()
        .filter(|state| !state.deleted && on_server.contains(&state.uid))
        .filter(|state| known.messages.get(&state.uid).is_some_and(|flags| !same_flags(flags, &state.flags)))
        .map(|state| (state.uid, state.flags.clone()))
        .collect();
    let folder = target.id.clone();
    let path = target.path.clone();
    let original_flags: Vec<(u32, Vec<String>)> = states
        .iter()
        .filter(|state| !state.deleted && on_server.contains(&state.uid))
        .map(|state| (state.uid, state.raw_flags.clone()))
        .collect();
    let underway = mailbox.underway();
    let (mut removed, flags, mut held) = store
        .run(move |store| {
            let mut removed = Vec::new();
            for uid in &gone {
                removed.extend(store.message_id_for_uid(&folder, *uid)?);
            }
            store.delete_uids(&folder, &gone)?;
            // Checked here, holding the store: a `mail.set_flags` notes its change before it writes
            // the store, so it either is held now or writes after this.
            let (held, changed): (Vec<_>, Vec<_>) = {
                let underway = underway.lock().unwrap_or_else(PoisonError::into_inner);
                changed.into_iter().partition(|(uid, _)| underway.held(&folder, *uid))
            };
            let mut flagged = Vec::new();
            for (uid, flags) in &changed {
                if store.set_flags_by_uid(&folder, *uid, flags)? {
                    flagged.extend(store.message_id_for_uid(&folder, *uid)?);
                }
            }
            for (uid, flags) in &original_flags {
                store.set_original_flags(&path, uidvalidity, *uid, flags)?;
            }
            Ok((removed, flagged, !held.is_empty()))
        })
        .await?;
    removed.splice(0..0, known.dropped);
    let initial = known.validity != UidValidity::Unchanged;
    let change = |new: Vec<String>, flags: Vec<String>, removed: Vec<String>| MailChange {
        account: mailbox.id().to_string(),
        folder: target.id.clone(),
        role: target.role,
        new,
        flags,
        removed,
        initial,
    };
    mailbox.announce(change(Vec::new(), flags, removed));

    // New messages: headers, newest first, batch by batch.
    let new: Vec<u32> = on_server.iter().rev().filter(|uid| !known.messages.contains_key(uid)).copied().collect();
    let policy = store.run(|store| store.cache_policy()).await?;
    let mode = policy.mode;
    let mut ahead: Vec<(String, FetchedHeader)> = Vec::new();
    progress.phase(MailSyncPhase::Headers, new.len());
    for batch in new.chunks(HEADER_BATCH) {
        let mut lease = conn.lease().await?;
        if !reselect(lease.session(), &target.path, uidvalidity).await? {
            // Renumbered meanwhile: the next sync starts the folder over.
            lease.done();
            return Ok(MailFolderSyncState::Interrupted);
        }
        let mut headers = lease.session().fetch_headers(batch).await?;
        lease.done();
        headers.retain(|header| !header.deleted);
        headers.sort_by_key(|header| std::cmp::Reverse(header.uid));
        let folder = target.id.clone();
        let underway = mailbox.underway();
        let (stored, kept) = store
            .run(move |store| {
                let mut stored = Vec::with_capacity(headers.len());
                let mut kept = false;
                for header in headers {
                    let read = mime::read(&header.header).unwrap_or_else(|_| unreadable());
                    let mut message = new_message(&folder, uidvalidity, &header, &read, now);
                    // A message moved here since `know` is in the store already.
                    if let Some(flags) = flags_on_their_way(store, &underway, &folder, header.uid)? {
                        message.flags = flags;
                        kept = true;
                    }
                    let id = store.upsert_message(&folder, uidvalidity, &message)?;
                    if let Some(at) = header.internal_date {
                        store.set_internal_date(&folder, header.uid, at)?;
                    }
                    store.set_size(&folder, header.uid, u64::from(header.size))?;
                    stored.push((id, message.date, header));
                }
                Ok((stored, kept))
            })
            .await?;
        held |= kept;
        let ids: Vec<String> = stored.iter().map(|(id, _, _)| id.clone()).collect();
        let policy_to_check = policy.clone();
        let keep = store.run(move |store| store.should_cache_bodies(&ids, &policy_to_check, now)).await?;
        let ids = stored.iter().map(|(id, _, _)| id.clone()).collect();
        for ((id, _date, header), keep) in stored.into_iter().zip(keep) {
            if header.size > BODY_PREFETCH_MAX {
                continue;
            }
            if keep {
                ahead.push((id, header));
            }
        }
        mailbox.announce(change(ids, Vec::new(), Vec::new()));
        progress.advance(batch.len());
    }

    // Texts of the recent ones, newest first.
    progress.phase(MailSyncPhase::Texts, ahead.len());
    for (id, header) in ahead {
        let mut lease = conn.lease().await?;
        if !reselect(lease.session(), &target.path, uidvalidity).await? {
            lease.done();
            return Ok(MailFolderSyncState::Interrupted);
        }
        let place = super::store::MessageRef {
            id: id.clone(),
            account: mailbox.id().to_string(),
            folder: target.id.clone(),
            folder_path: target.path.clone(),
            uidvalidity,
            uid: header.uid,
            rfc_message_id: None,
        };
        // In `AllOffline` the original is kept as well; failing to keep it never ends the sync.
        let fetched = fetch_for_reading(lease.session(), store, &place, mode == MailCacheMode::AllOffline).await?;
        lease.done();
        progress.advance(1);
        let Fetched::Whole(raw) = fetched else { continue };
        if policy.prefetch_attachments {
            let _ = prefetch_attachments(mailbox, &id, &raw).await;
        }
        let Ok(read) = mime::read(&raw) else { continue };
        // What an invitation invites to, from its calendar part (task E8.3b).
        let invite = super::read::invite_of(&raw, &read);
        let folder = target.id.clone();
        store
            .run(move |store| {
                // The flags as the store has them now: a command may have changed them meanwhile.
                let flags = store.summary(&id)?.map_or_else(|| header.flags.clone(), |summary| summary.flags);
                let mut message = new_message(&folder, uidvalidity, &header, &read, now);
                message.flags = flags;
                store.upsert_message(&folder, uidvalidity, &message)?;
                store.set_body(&id, &Body::of(read, invite))?;
                Ok(())
            })
            .await?;
    }

    // Held flags are compared again next time, so the HIGHESTMODSEQ that hid them is not kept.
    if !held {
        let (folder, modseq) = (target.id.clone(), selected.highest_modseq);
        store.run(move |store| store.set_highest_modseq(&folder, modseq)).await?;
    }
    Ok(match fill_sizes(conn, progress, store, target, uidvalidity).await? {
        true => MailFolderSyncState::Synced,
        false => MailFolderSyncState::Interrupted,
    })
}

/// The flags the store has for `uid` of `folder` while a change of them is on its way to the server
/// ([`FlagsUnderway::held`]), to keep over the ones the server reported; `None` when nothing is on
/// its way or the store has no such message. Call it holding the store, as the flags above do.
fn flags_on_their_way(
    store: &MailStore,
    underway: &StdMutex<FlagsUnderway>,
    folder: &str,
    uid: u32,
) -> Result<Option<Vec<Flag>>, MailStoreError> {
    if !underway.lock().unwrap_or_else(PoisonError::into_inner).held(folder, uid) {
        return Ok(None);
    }
    let Some(id) = store.message_id_for_uid(folder, uid)? else { return Ok(None) };
    Ok(store.summary(&id)?.map(|summary| summary.flags))
}

/// Asks the server for the sizes (`RFC822.SIZE`) of up to [`SIZE_FILL_MAX`] stored messages of the
/// folder that lack one, highest UID first, [`SIZE_FILL_BATCH`] per `UID FETCH`. Sizes are an
/// extra: a server that answers unexpectedly leaves them missing for the next sync and ends
/// nothing; only a lost connection or account is an error.
/// A message the answer names no size for, with or without a `FETCH` of its own, is kept with 0.
/// `false` when the server renumbered the folder meanwhile.
async fn fill_sizes(
    conn: &mut Conn<'_>,
    progress: &SyncProgress,
    store: &SharedStore,
    target: &Target,
    uidvalidity: u32,
) -> Result<bool, MailError> {
    let folder = target.id.clone();
    let missing = store.run(move |store| store.uids_without_size(&folder, SIZE_FILL_MAX)).await?;
    progress.phase(MailSyncPhase::Sizes, missing.len());
    for batch in missing.chunks(SIZE_FILL_BATCH) {
        let mut lease = conn.lease().await?;
        if !reselect(lease.session(), &target.path, uidvalidity).await? {
            lease.done();
            return Ok(false);
        }
        let sizes = match lease.session().fetch_sizes(batch).await.map_err(MailError::from) {
            Ok(sizes) => sizes,
            Err(err) if connection_lost(&err) => return Err(err),
            // Dropping the lease closes a connection that answered something unexpected.
            Err(_) => return Ok(true),
        };
        lease.done();
        // A message the answer names no size for (answered without one, or not at all) would never
        // get one: it counts as 0, so its folder still gets its size and no sync asks for it again.
        let named: BTreeSet<u32> = sizes.iter().map(|(uid, _)| *uid).collect();
        let sizeless: Vec<u32> = batch.iter().copied().filter(|uid| !named.contains(uid)).collect();
        let folder = target.id.clone();
        store
            .run(move |store| {
                for (uid, bytes) in &sizes {
                    store.set_size(&folder, *uid, *bytes)?;
                }
                for uid in &sizeless {
                    store.set_size(&folder, *uid, 0)?;
                }
                Ok(())
            })
            .await?;
        progress.advance(batch.len());
    }
    Ok(true)
}

/// Reads what the store knows of `folder` and tells it the UIDVALIDITY the server reports. After
/// a renumbering the ids of the dropped messages are kept, so the change can name them.
async fn know(store: &SharedStore, folder: &str, uidvalidity: u32) -> Result<Known, MailError> {
    let folder = folder.to_string();
    let known = store
        .run(move |store| {
            let before = store.resolve_folder(&folder)?.ok_or(super::MailStoreError::UnknownFolder)?;
            let renumbered = before.uidvalidity.is_some_and(|seen| seen != uidvalidity);
            let mut dropped = Vec::new();
            if renumbered {
                for (uid, _) in store.folder_uids(&folder)? {
                    dropped.extend(store.message_id_for_uid(&folder, uid)?);
                }
            }
            let validity = store.note_uidvalidity(&folder, uidvalidity)?;
            let modseq = before.highest_modseq.filter(|_| validity == UidValidity::Unchanged);
            let messages = store.folder_uids(&folder)?.into_iter().collect();
            Ok(Known { validity, modseq, messages, dropped })
        })
        .await?;
    Ok(known)
}

/// Opens `path` again when the session has another folder open (a command ran in between), or
/// this one only read-only. `false` when the folder's UIDVALIDITY is no longer `uidvalidity`.
async fn reselect(session: &mut Session, path: &str, uidvalidity: u32) -> Result<bool, MailError> {
    if let Some(selected) = session.selected()
        && selected.path == path
        && !selected.read_only
    {
        return Ok(selected.uid_validity == uidvalidity);
    }
    Ok(session.select(path).await?.uid_validity == uidvalidity)
}

fn same_flags(a: &[Flag], b: &[Flag]) -> bool {
    let set = |flags: &[Flag]| flags.iter().copied().collect::<BTreeSet<Flag>>();
    set(a) == set(b)
}

/// The headers of a message as the store keeps them. `read` may come from the header block only
/// (then the text and its snippet are empty) or from the whole message.
fn new_message(
    folder: &str,
    uidvalidity: u32,
    header: &FetchedHeader,
    read: &ReadMessage,
    now: DateTime<Utc>,
) -> NewMessage {
    NewMessage {
        id: None,
        uid: header.uid,
        rfc_message_id: read.message_id.clone(),
        thread_id: thread_id(read).unwrap_or_else(|| format!("t:{folder}:{uidvalidity}:{}", header.uid)),
        from: read.from.clone(),
        to: read.to.clone(),
        cc: read.cc.clone(),
        reply_to: read.reply_to.clone(),
        subject: read.subject.clone(),
        date: read.date.or(header.internal_date).unwrap_or(now),
        snippet: read.snippet(),
        flags: header.flags.clone(),
        has_attachments: header.has_attachments || read.has_attachments(),
        traits: read.traits.clone(),
        invitation: read.invitation,
    }
}

/// The conversation of a message: the first message it refers to (`References:`, else
/// `In-Reply-To:`), else itself, as a short hash. `None` without any `Message-ID`. Drafts the
/// core saves itself (task M3.2, [`super::write`]) are threaded the same way.
pub(super) fn thread_id(read: &ReadMessage) -> Option<String> {
    let root = read.references.first().or(read.in_reply_to.as_ref()).or(read.message_id.as_ref())?;
    let hash = torro_mail_backup::hex_digest(&Sha256::digest(root.as_bytes())[..12]);
    Some(format!("t:{hash}"))
}

/// What is kept of a header block `mime::read` cannot read: nothing but the server's markers.
fn unreadable() -> ReadMessage {
    ReadMessage {
        message_id: None,
        in_reply_to: None,
        references: Vec::new(),
        from: None,
        to: Vec::new(),
        cc: Vec::new(),
        reply_to: Vec::new(),
        subject: String::new(),
        date: None,
        text: String::new(),
        from_html: false,
        html: None,
        html_source: None,
        attachments: Vec::new(),
        traits: torro_mail::MessageTraits::default(),
        invitation: false,
    }
}

/// A scripted IMAP server for the tests of the mail sync, after `torro-mail-imap`'s
/// `tests/imap_support`, but answering from a small world of folders and messages instead of a
/// fixed script, so a sync can run against it as against a real server: sign-in (`LOGIN`), `LIST`
/// with SPECIAL-USE, `SELECT` with CONDSTORE, `UID SEARCH`, `UID FETCH` (headers, flags with
/// `CHANGEDSINCE`, whole messages), `IDLE` with push, `NOOP`, `LOGOUT`, and for the mail commands
/// (task M3.2) `STATUS`, `CREATE`, `APPEND`, `UID STORE`, `UID COPY` and `EXPUNGE` (no MOVE, no UIDPLUS: the
/// client takes its fallbacks). Tests change the world
/// ([`FakeImap::add`], [`FakeImap::set_flags`], …) and cut connections; idling clients hear of
/// every change.
///
/// TLS with the test certificate of `torro-mail-imap/tests/fixtures/imap-tls/` (for `127.0.0.1`);
/// the client trusts only its CA ([`fake::trust`]). Loopback only, no network.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, FixedOffset, Utc};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::watch;
    use tokio_rustls::TlsAcceptor;
    use torro_mail::{Endpoint, Security};
    use torro_mail_imap::imap::Trust;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../torro-mail-imap/tests/fixtures/imap-tls/");
    const CAPABILITIES: &str = "IMAP4rev1 IDLE CONDSTORE SPECIAL-USE NAMESPACE";

    /// The client side's trust: only the test CA.
    pub(crate) fn trust() -> Trust {
        Trust::Only(vec![CertificateDer::from(fixture("ca.der"))])
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}{name}")).expect("test certificate of torro-mail-imap")
    }

    /// A message `days_ago` days old with `subject`, `Message-ID` `<subject>@example.org` (spaces
    /// as dashes) and, if given, `In-Reply-To`.
    pub(crate) fn mail(subject: &str, days_ago: i64, in_reply_to: Option<&str>) -> String {
        let date = (Utc::now() - Duration::days(days_ago)).to_rfc2822();
        let id = subject.replace(' ', "-");
        let reply = in_reply_to.map(|to| format!("In-Reply-To: <{to}>\r\n")).unwrap_or_default();
        format!(
            "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: {subject}\r\nDate: {date}\r\n\
             Message-ID: <{id}@example.org>\r\n{reply}Content-Type: text/plain; charset=utf-8\r\n\r\nText of {subject}\r\n"
        )
    }

    struct Message {
        uid: u32,
        flags: BTreeSet<String>,
        modseq: u64,
        raw: Vec<u8>,
        /// `INTERNALDATE`, in the zone it was received in.
        received: DateTime<FixedOffset>,
        /// `BODY.PEEK[]` answers nothing for it, as for a message gone from the server; listing and
        /// flags still show it, so the sync keeps it in the cache.
        withheld: bool,
        /// Gmail's `X-GM-MSGID`, answered when asked for.
        gm_msgid: Option<u64>,
        /// `RFC822.SIZE` as the server claims it, when that is not the true length.
        reported_size: Option<usize>,
        /// A fetch of the size alone answers without `RFC822.SIZE` (a server that never names it).
        sizeless: bool,
    }

    impl Message {
        fn new(uid: u32, flags: BTreeSet<String>, modseq: u64, raw: Vec<u8>, received: DateTime<FixedOffset>) -> Self {
            Message {
                uid,
                flags,
                modseq,
                raw,
                received,
                withheld: false,
                gm_msgid: None,
                reported_size: None,
                sizeless: false,
            }
        }
    }

    /// What the fake does to its world when a trigger ([`FakeImap::on_examine`]) fires.
    #[derive(Debug, Clone)]
    pub(crate) enum Change {
        /// A new message with this raw text arrives in the folder.
        Add(String),
        /// A new message with this raw text arrives in each of these folders, as Gmail files new
        /// mail in the inbox and in "All Mail" at once.
        AddTo(Vec<String>, String),
        /// The message with this UID is expunged.
        Expunge(u32),
        /// The folder gets a new UIDVALIDITY ([`FakeImap::renumber`]).
        Renumber,
        /// From this `EXAMINE` on the folder cannot be opened (`NO`), while `LIST` still shows it
        /// selectable.
        Refuse,
    }

    struct Folder {
        path: String,
        attributes: String,
        uidvalidity: u32,
        next_uid: u32,
        modseq: u64,
        messages: Vec<Message>,
        /// `PERMANENTFLAGS` answered on `SELECT`/`EXAMINE`, as the list's text; none: not sent.
        /// Without `\*` an `APPEND` with another keyword is refused, as strict servers do.
        permanent: Option<String>,
        /// `SELECT` and `EXAMINE` answer `NO` ([`Change::Refuse`]).
        refused: bool,
    }

    /// What the next `APPEND` does instead of storing the message as sent ([`FakeImap::quirk_next_append`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum AppendQuirk {
        /// Reads the message, stores nothing and answers `NO`.
        Refuse,
        /// Reads the message, stores nothing and loses the connection before answering.
        Lose,
        /// Stores the message with a header line added in front, as servers that rewrite mail do.
        Alter,
    }

    struct World {
        password: String,
        folders: Vec<Folder>,
        modseq: u64,
        /// Every line the clients sent, the password blanked.
        lines: Vec<String>,
        logins: usize,
        /// How often each folder was opened with `EXAMINE`.
        examines: HashMap<String, usize>,
        /// Changes waiting for the n-th `EXAMINE` of a folder.
        on_examine: Vec<(String, usize, Change)>,
    }

    impl World {
        fn folder(&mut self, path: &str) -> &mut Folder {
            let at = self.folders.iter().position(|folder| folder.path == path).expect("a folder of the fake");
            &mut self.folders[at]
        }

        fn bump(&mut self) -> u64 {
            self.modseq += 1;
            self.modseq
        }
    }

    /// A running fake server; clones share it.
    #[derive(Clone)]
    pub(crate) struct FakeImap {
        pub(crate) endpoint: Endpoint,
        world: Arc<Mutex<World>>,
        /// Counts changes of the world; idling clients wait on it.
        changes: watch::Sender<u64>,
        /// Counts cuts; every open connection ends on one.
        cuts: watch::Sender<u64>,
        backup_change: Arc<Mutex<Option<(String, String)>>>,
        selectable_change: Arc<Mutex<Option<String>>>,
        drop_after_append: Arc<Mutex<bool>>,
        /// What the next `APPEND` does instead of storing as sent.
        /// What an `APPEND` does instead of storing as sent, and how many plain ones come first.
        append_quirk: Arc<Mutex<Option<(usize, AppendQuirk)>>>,
        /// Answers `APPEND` with `[APPENDUID …]`, as servers with UIDPLUS.
        uidplus: Arc<AtomicBool>,
        /// A folder that shows every appended message once more, as Gmail's "All Mail".
        mirror: Arc<Mutex<Option<String>>>,
        /// Announces `X-GM-EXT-1` and answers `X-GM-MSGID`, as Gmail.
        gmail: Arc<AtomicBool>,
        /// While `true`, partial body fetches (`BODY.PEEK[]<…>`) wait for an answer.
        chunk_gate: watch::Sender<bool>,
        /// Used and allowed KiB of the storage quota; while set, `QUOTA` is announced.
        quota: Arc<Mutex<Option<(u64, u64)>>>,
    }

    impl FakeImap {
        /// A server on `127.0.0.1` that takes `password` for any login, with the folders INBOX,
        /// Drafts, Sent and Archive (marked with their SPECIAL-USE attributes), all empty.
        pub(crate) async fn start(password: &str) -> FakeImap {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
            let port = listener.local_addr().expect("address").port();
            let folder = |path: &str, attributes: &str, uidvalidity: u32| Folder {
                path: path.into(),
                attributes: attributes.into(),
                uidvalidity,
                next_uid: 1,
                modseq: 1,
                messages: Vec::new(),
                permanent: None,
                refused: false,
            };
            let world = World {
                password: password.into(),
                folders: vec![
                    folder("INBOX", "\\HasNoChildren", 11),
                    folder("Drafts", "\\HasNoChildren \\Drafts", 12),
                    folder("Sent", "\\HasNoChildren \\Sent", 13),
                    folder("Archive", "\\HasNoChildren \\Archive", 14),
                ],
                modseq: 1,
                lines: Vec::new(),
                logins: 0,
                examines: HashMap::new(),
                on_examine: Vec::new(),
            };
            let server = FakeImap {
                endpoint: Endpoint { host: "127.0.0.1".into(), port, security: Security::Tls },
                world: Arc::new(Mutex::new(world)),
                changes: watch::Sender::new(0),
                cuts: watch::Sender::new(0),
                backup_change: Arc::new(Mutex::new(None)),
                selectable_change: Arc::new(Mutex::new(None)),
                drop_after_append: Arc::new(Mutex::new(false)),
                append_quirk: Arc::new(Mutex::new(None)),
                uidplus: Arc::new(AtomicBool::new(false)),
                mirror: Arc::new(Mutex::new(None)),
                gmail: Arc::new(AtomicBool::new(false)),
                chunk_gate: watch::Sender::new(false),
                quota: Arc::new(Mutex::new(None)),
            };
            let accepting = server.clone();
            tokio::spawn(async move {
                let acceptor = acceptor();
                while let Ok((tcp, _)) = listener.accept().await {
                    let (acceptor, server) = (acceptor.clone(), accepting.clone());
                    tokio::spawn(async move {
                        if let Ok(tls) = acceptor.accept(tcp).await {
                            server.serve(tls).await;
                        }
                    });
                }
            });
            server
        }

        fn world(&self) -> std::sync::MutexGuard<'_, World> {
            self.world.lock().expect("the fake's world")
        }

        fn changed(&self) {
            self.changes.send_modify(|count| *count += 1);
        }

        /// Delivers `raw` into `folder` with `flags`; returns its UID.
        pub(crate) fn add(&self, folder: &str, raw: &str, flags: &[&str]) -> u32 {
            self.add_received(folder, raw.as_bytes(), flags, Utc::now().fixed_offset())
        }

        /// Delivers the bytes `raw` into `folder` with `flags`, received at `received` (its
        /// `INTERNALDATE`, zone included); returns its UID.
        pub(crate) fn add_received(
            &self,
            folder: &str,
            raw: &[u8],
            flags: &[&str],
            received: DateTime<FixedOffset>,
        ) -> u32 {
            let uid = {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                let uid = folder.next_uid;
                folder.next_uid += 1;
                folder.modseq = modseq;
                let flags = flags.iter().map(|flag| flag.to_string()).collect();
                folder.messages.push(Message::new(uid, flags, modseq, raw.to_vec(), received));
                uid
            };
            self.changed();
            uid
        }

        /// As Gmail from now on: announces `X-GM-EXT-1` and answers `X-GM-MSGID`.
        pub(crate) fn act_as_gmail(&self) {
            self.gmail.store(true, Ordering::Release);
        }

        /// Gives message `uid` of `folder` the Gmail message id `msgid` (the same in every label).
        pub(crate) fn set_gm_msgid(&self, folder: &str, uid: u32, msgid: u64) {
            let mut world = self.world();
            let message =
                world.folder(folder).messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
            message.gm_msgid = Some(msgid);
        }

        /// From now on the server claims `RFC822.SIZE` `size` for message `uid` of `folder`, whatever
        /// its true length; the body itself stays whole.
        pub(crate) fn misreport_size(&self, folder: &str, uid: u32, size: usize) {
            let mut world = self.world();
            let message =
                world.folder(folder).messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
            message.reported_size = Some(size);
        }

        /// From now on a fetch of the size alone answers for message `uid` of `folder` without
        /// `RFC822.SIZE`.
        pub(crate) fn withhold_size(&self, folder: &str, uid: u32) {
            let mut world = self.world();
            let message =
                world.folder(folder).messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
            message.sizeless = true;
        }

        /// Applies `change` to `folder` just before the `nth` `EXAMINE` of it (counted from 1) is
        /// answered.
        pub(crate) fn on_examine(&self, folder: &str, nth: usize, change: Change) {
            self.world().on_examine.push((folder.into(), nth, change));
        }

        /// From now on `folder` cannot be opened (`NO`), while `LIST` still shows it selectable.
        pub(crate) fn refuse(&self, folder: &str) {
            self.world().folder(folder).refused = true;
        }

        /// While `held`, every partial body fetch waits; `false` lets them go on.
        pub(crate) fn hold_backup_chunks(&self, held: bool) {
            self.chunk_gate.send_replace(held);
        }

        /// From the next sign-in on, announces `QUOTA` and reports `used` of `limit` KiB of storage;
        /// `None` takes the capability away again (sessions signed in before still ask and hear of
        /// no limit).
        pub(crate) fn set_quota(&self, quota: Option<(u64, u64)>) {
            *self.quota.lock().expect("quota") = quota;
        }

        /// The capabilities announced after the sign-in.
        fn capabilities(&self) -> String {
            let capabilities = world_capabilities(self.gmail.load(Ordering::Acquire));
            if self.quota.lock().expect("quota").is_some() { format!("{capabilities} QUOTA") } else { capabilities }
        }

        /// `GETQUOTAROOT`: the inbox's root `""` with the storage quota, or no root without one.
        fn quota_root(&self, tag: &str) -> String {
            match *self.quota.lock().expect("quota") {
                Some((used, limit)) => format!(
                    "* QUOTAROOT \"INBOX\" \"\"\r\n* QUOTA \"\" (STORAGE {used} {limit})\r\n{tag} OK Getquotaroot completed"
                ),
                None => format!("* QUOTAROOT \"INBOX\"\r\n{tag} OK Getquotaroot completed"),
            }
        }

        /// Counts an `EXAMINE` of `path` and applies the changes waiting for it.
        fn examined(&self, path: &str) {
            let due: Vec<Change> = {
                let mut world = self.world();
                let count = world.examines.entry(path.to_string()).or_default();
                *count += 1;
                let count = *count;
                let (due, waiting) =
                    world.on_examine.drain(..).partition(|(folder, nth, _)| folder == path && *nth == count);
                world.on_examine = waiting;
                due.into_iter().map(|(_, _, change)| change).collect()
            };
            for change in due {
                match change {
                    Change::Add(raw) => {
                        self.add(path, &raw, &[]);
                    }
                    Change::AddTo(folders, raw) => {
                        for folder in folders {
                            self.add(&folder, &raw, &[]);
                        }
                    }
                    Change::Expunge(uid) => self.expunge(path, uid),
                    Change::Renumber => self.renumber(path),
                    Change::Refuse => self.world().folder(path).refused = true,
                }
            }
        }

        /// Injects an incoming message when the next backup body chunk is served.
        pub(crate) fn add_on_backup_chunk(&self, folder: &str, raw: &str) {
            *self.backup_change.lock().expect("backup change") = Some((folder.into(), raw.into()));
        }

        pub(crate) fn make_selectable_on_backup_chunk(&self, folder: &str) {
            self.world().folder(folder).attributes = "\\Noselect".into();
            *self.selectable_change.lock().expect("selectable change") = Some(folder.into());
        }

        /// Store the next APPEND literal, then lose the connection before acknowledging it.
        pub(crate) fn drop_after_next_append(&self) {
            *self.drop_after_append.lock().expect("append cut") = true;
        }

        /// The next `APPEND` does `quirk` instead of storing the message as sent.
        pub(crate) fn quirk_next_append(&self, quirk: AppendQuirk) {
            self.quirk_append_after(0, quirk);
        }

        /// The `APPEND` after the next `plain` ones does `quirk` instead of storing as sent.
        pub(crate) fn quirk_append_after(&self, plain: usize, quirk: AppendQuirk) {
            *self.append_quirk.lock().expect("append quirk") = Some((plain, quirk));
        }

        /// From now on `APPEND` answers where the message landed (`APPENDUID`).
        pub(crate) fn act_with_uidplus(&self) {
            self.uidplus.store(true, Ordering::Release);
        }

        /// From now on every message appended to another folder shows up in `folder` too, as in
        /// Gmail's "All Mail".
        pub(crate) fn mirror_appends_into(&self, folder: &str) {
            *self.mirror.lock().expect("mirror") = Some(folder.into());
        }

        /// `folder` answers `PERMANENTFLAGS (flags)` from now on (`\\*` for any new keyword).
        pub(crate) fn set_permanent_flags(&self, folder: &str, flags: &str) {
            self.world().folder(folder).permanent = Some(flags.into());
        }

        /// Sets the flags of message `uid` in `folder`, as another mail program would.
        pub(crate) fn set_flags(&self, folder: &str, uid: u32, flags: &[&str]) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.modseq = modseq;
                let message = folder.messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
                message.flags = flags.iter().map(|flag| flag.to_string()).collect();
                message.modseq = modseq;
            }
            self.changed();
        }

        /// The flags of message `uid` in `folder` as the server holds them, sorted.
        pub(crate) fn flags(&self, folder: &str, uid: u32) -> Vec<String> {
            let mut world = self.world();
            let message = world.folder(folder).messages.iter().find(|m| m.uid == uid).expect("a message of the fake");
            message.flags.iter().cloned().collect()
        }

        /// Removes message `uid` from `folder` for good.
        pub(crate) fn expunge(&self, folder: &str, uid: u32) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.modseq = modseq;
                folder.messages.retain(|message| message.uid != uid);
            }
            self.changed();
        }

        /// From now on fetching the text of message `uid` in `folder` gives nothing, as for a message
        /// gone from the server, while the sync still sees it: no race with the sync dropping it.
        pub(crate) fn withhold_text(&self, folder: &str, uid: u32) {
            let mut world = self.world();
            let message =
                world.folder(folder).messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
            message.withheld = true;
        }

        /// Gives `folder` a new UIDVALIDITY and its messages new UIDs, as a server that rebuilt it.
        pub(crate) fn renumber(&self, folder: &str) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.uidvalidity += 100;
                folder.modseq = modseq;
                for message in &mut folder.messages {
                    message.uid += 1000;
                }
                folder.next_uid += 1000;
            }
            self.changed();
        }

        /// Adds an empty folder at `path` with the `LIST` attributes `attributes` (`\Trash`).
        pub(crate) fn add_folder(&self, path: &str, attributes: &str, uidvalidity: u32) {
            self.world().folders.push(Folder {
                path: path.into(),
                attributes: attributes.into(),
                uidvalidity,
                next_uid: 1,
                modseq: 1,
                messages: Vec::new(),
                permanent: None,
                refused: false,
            });
            self.changed();
        }

        /// The messages of `folder` as the server holds them: UID, flags and the raw message.
        pub(crate) fn messages(&self, folder: &str) -> Vec<(u32, Vec<String>, String)> {
            let mut world = self.world();
            world
                .folder(folder)
                .messages
                .iter()
                .map(|m| (m.uid, m.flags.iter().cloned().collect(), String::from_utf8_lossy(&m.raw).into_owned()))
                .collect()
        }

        /// Ends every open connection, as a network that went away.
        pub(crate) fn drop_connections(&self) {
            self.cuts.send_modify(|count| *count += 1);
        }

        /// Every line clients sent so far, the password blanked.
        pub(crate) fn lines(&self) -> Vec<String> {
            self.world().lines.clone()
        }

        /// How many sign-ins were tried.
        pub(crate) fn logins(&self) -> usize {
            self.world().logins
        }

        async fn serve<S>(&self, stream: S)
        where
            S: tokio::io::AsyncRead + AsyncWrite + Unpin,
        {
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let mut changes = self.changes.subscribe();
            let mut cuts = self.cuts.subscribe();
            cuts.borrow_and_update();
            let mut selected: Option<String> = None;
            // The changes this connection has seen with its last SELECT; newer ones are pushed at
            // the start of an IDLE, as a real server reports what happened since.
            let mut seen = 0;
            if write(&mut writer, "* OK [CAPABILITY IMAP4rev1] Fake IMAP ready").await.is_err() {
                return;
            }
            let mut line = String::new();
            loop {
                line.clear();
                let read = tokio::select! {
                    read = reader.read_line(&mut line) => read,
                    _ = cuts.changed() => return,
                };
                if !matches!(read, Ok(n) if n > 0) {
                    return;
                }
                // APPEND reads its message literal itself; search words come as literals too.
                let appends = line.split(' ').nth(1).is_some_and(|word| word.eq_ignore_ascii_case("APPEND"));
                let text = if appends {
                    line.trim_end_matches(['\r', '\n']).to_string()
                } else {
                    let Some(text) = with_literals(&mut reader, &mut writer, &line).await else { return };
                    text
                };
                let (tag, rest) = text.split_once(' ').unwrap_or((text.as_str(), ""));
                let (tag, rest) = (tag.to_string(), rest.to_string());
                let command = rest.split(' ').next().unwrap_or_default().to_ascii_uppercase();
                self.world().lines.push(if command == "LOGIN" { format!("{tag} LOGIN …") } else { text.clone() });
                let answer: Vec<u8> = match command.as_str() {
                    "LOGIN" => self.login(&tag, &rest).into(),
                    "CAPABILITY" => format!("* CAPABILITY {}\r\n{tag} OK done", self.capabilities()).into(),
                    "NOOP" => format!("{tag} OK done").into(),
                    "LOGOUT" => {
                        let _ = write(&mut writer, &format!("* BYE bye\r\n{tag} OK done")).await;
                        return;
                    }
                    "NAMESPACE" => format!("* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n{tag} OK done").into(),
                    "GETQUOTAROOT" => self.quota_root(&tag).into(),
                    "LIST" => self.list(&tag).into(),
                    "SELECT" | "EXAMINE" => {
                        let path = quoted(&rest[command.len() + 1..]);
                        if command == "EXAMINE" && self.unselectable(&tag, &path).is_none() {
                            self.examined(&path);
                        }
                        if let Some(refusal) = self.unselectable(&tag, &path) {
                            selected = None;
                            refusal.into()
                        } else {
                            seen = *changes.borrow_and_update();
                            let answer = self.select(&tag, &path, command == "EXAMINE");
                            selected = Some(path);
                            answer.into()
                        }
                    }
                    "SUBSCRIBE" => format!("{tag} OK subscribed").into(),
                    "UID" => {
                        if rest.contains("BODY.PEEK[]<") {
                            let mut gate = self.chunk_gate.subscribe();
                            while *gate.borrow_and_update() {
                                if gate.changed().await.is_err() {
                                    break;
                                }
                            }
                        }
                        let answer = self.uid(&tag, &rest["UID ".len()..], selected.as_deref());
                        if rest.contains("BODY.PEEK[]<")
                            && let Some((folder, raw)) = self.backup_change.lock().expect("backup change").take()
                        {
                            self.add(&folder, &raw, &[]);
                        }
                        if rest.contains("BODY.PEEK[]<")
                            && let Some(folder) = self.selectable_change.lock().expect("selectable change").take()
                        {
                            self.world().folder(&folder).attributes = "\\HasNoChildren".into();
                            self.changed();
                        }
                        answer
                    }
                    "STATUS" => self.status(&tag, &quoted(&rest["STATUS ".len()..])).into(),
                    "CREATE" => self.create(&tag, &rest).into(),
                    "EXPUNGE" => self.expunge_deleted(&tag, selected.as_deref()).into(),
                    "APPEND" => match self.append(&mut reader, &mut writer, &tag, &rest).await {
                        Some(answer) => answer.into(),
                        None => return,
                    },
                    "IDLE" => {
                        if write(&mut writer, "+ idling").await.is_err() {
                            return;
                        }
                        if *changes.borrow_and_update() != seen && self.push(&mut writer, selected.as_deref()).await {
                            return;
                        }
                        loop {
                            line.clear();
                            tokio::select! {
                                read = reader.read_line(&mut line) => {
                                    if !matches!(read, Ok(n) if n > 0) {
                                        return;
                                    }
                                    self.world().lines.push(line.trim_end().to_string());
                                    break;
                                }
                                _ = changes.changed() => {
                                    if self.push(&mut writer, selected.as_deref()).await {
                                        return;
                                    }
                                }
                                _ = cuts.changed() => return,
                            }
                        }
                        format!("{tag} OK Idle completed").into()
                    }
                    _ => format!("{tag} BAD unknown command").into(),
                };
                if write_bytes(&mut writer, &answer).await.is_err() {
                    return;
                }
            }
        }

        /// Tells an idling client the size of its folder; `true` when the connection broke.
        async fn push<W: AsyncWrite + Unpin>(&self, writer: &mut W, selected: Option<&str>) -> bool {
            let exists = selected.map_or(0, |path| self.world().folder(path).messages.len());
            write(writer, &format!("* {exists} EXISTS")).await.is_err()
        }

        fn login(&self, tag: &str, rest: &str) -> String {
            let mut world = self.world();
            world.logins += 1;
            let arguments = &rest["LOGIN ".len()..];
            let user = quoted(arguments);
            let after = &arguments[user.len() + 2..].trim_start();
            if !user.is_empty() && quoted(after) == world.password {
                format!("{tag} OK [CAPABILITY {}] Logged in", self.capabilities())
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.")
            }
        }

        fn list(&self, tag: &str) -> String {
            let world = self.world();
            let mut lines: Vec<String> = world
                .folders
                .iter()
                .map(|folder| format!("* LIST ({}) \"/\" \"{}\"", folder.attributes, folder.path))
                .collect();
            lines.push(format!("{tag} OK List completed"));
            lines.join("\r\n")
        }

        /// The `NO` for opening `path` when it does not exist or is `\\Noselect`.
        fn unselectable(&self, tag: &str, path: &str) -> Option<String> {
            let world = self.world();
            match world.folders.iter().find(|folder| folder.path == path) {
                None => Some(format!("{tag} NO [NONEXISTENT] no such folder")),
                Some(folder) if folder.refused || folder.attributes.contains("\\Noselect") => {
                    Some(format!("{tag} NO [CANNOT] folder cannot be selected"))
                }
                Some(_) => None,
            }
        }

        fn select(&self, tag: &str, path: &str, read_only: bool) -> String {
            let mut world = self.world();
            let folder = world.folder(path);
            let mode = if read_only { "READ-ONLY" } else { "READ-WRITE" };
            let permanent = folder
                .permanent
                .as_ref()
                .map(|flags| format!("* OK [PERMANENTFLAGS ({flags})] Kept\r\n"))
                .unwrap_or_default();
            format!(
                "* {} EXISTS\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] Next\r\n\
                 * OK [HIGHESTMODSEQ {}] Highest\r\n{permanent}{tag} OK [{mode}] Select completed",
                folder.messages.len(),
                folder.uidvalidity,
                folder.next_uid,
                folder.modseq
            )
        }

        /// `STATUS`: counts, UIDNEXT, UIDVALIDITY and HIGHESTMODSEQ of `path`.
        fn status(&self, tag: &str, path: &str) -> String {
            let world = self.world();
            let Some(folder) = world.folders.iter().find(|folder| folder.path == path) else {
                return format!("{tag} NO [NONEXISTENT] no such folder");
            };
            let unseen = folder.messages.iter().filter(|m| !m.flags.contains("\\Seen")).count();
            format!(
                "* STATUS \"{path}\" (MESSAGES {} UNSEEN {unseen} UIDNEXT {} UIDVALIDITY {} HIGHESTMODSEQ {})\r\n\
                 {tag} OK Status completed",
                folder.messages.len(),
                folder.next_uid,
                folder.uidvalidity,
                folder.modseq
            )
        }

        fn create(&self, tag: &str, rest: &str) -> String {
            let path = quoted(&rest["CREATE ".len()..]);
            if path.is_empty() {
                return format!("{tag} BAD empty folder name");
            }
            let mut world = self.world();
            if world.folders.iter().any(|folder| folder.path == path) {
                return format!("{tag} NO [ALREADYEXISTS] folder exists");
            }
            let uidvalidity = world.folders.iter().map(|folder| folder.uidvalidity).max().unwrap_or(10) + 1;
            world.folders.push(Folder {
                path,
                attributes: "\\HasNoChildren".into(),
                uidvalidity,
                next_uid: 1,
                modseq: 1,
                messages: Vec::new(),
                permanent: None,
                refused: false,
            });
            drop(world);
            self.changed();
            format!("{tag} OK folder created")
        }

        /// `APPEND "path" (flags) {n}`: asks for the literal, reads it and files it. `None` when
        /// the connection broke.
        async fn append<R, W>(&self, reader: &mut R, writer: &mut W, tag: &str, rest: &str) -> Option<String>
        where
            R: tokio::io::AsyncBufRead + Unpin,
            W: AsyncWrite + Unpin,
        {
            use tokio::io::AsyncReadExt;
            let arguments = &rest["APPEND ".len()..];
            let path = quoted(arguments);
            let size: usize = arguments.rsplit_once('{')?.1.trim_end_matches('}').parse().ok()?;
            let flags: BTreeSet<String> = arguments
                .split_once('(')
                .and_then(|(_, after)| after.split_once(')'))
                .map(|(list, _)| list.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default();
            let received = arguments
                .rsplit_once("\" {")
                .and_then(|(prefix, _)| prefix.rsplit_once('"'))
                .and_then(|(_, date)| DateTime::parse_from_str(date, "%d-%b-%Y %H:%M:%S %z").ok())
                .unwrap_or_else(|| Utc::now().fixed_offset());
            write(writer, "+ go ahead").await.ok()?;
            let mut raw = vec![0u8; size];
            reader.read_exact(&mut raw).await.ok()?;
            let mut end = String::new();
            reader.read_line(&mut end).await.ok()?;
            let quirk = {
                let mut waiting = self.append_quirk.lock().expect("append quirk");
                match waiting.as_mut() {
                    Some((0, _)) => waiting.take().map(|(_, quirk)| quirk),
                    Some((plain, _)) => {
                        *plain -= 1;
                        None
                    }
                    None => None,
                }
            };
            match quirk {
                Some(AppendQuirk::Refuse) => return Some(format!("{tag} NO [OVERQUOTA] mailbox is full")),
                Some(AppendQuirk::Lose) => return None,
                Some(AppendQuirk::Alter) => raw = [b"X-Rewritten: yes\r\n".as_slice(), &raw].concat(),
                None => {}
            }
            let mirror = self.mirror.lock().expect("mirror").clone().filter(|mirror| *mirror != path);
            let landed = {
                let mut world = self.world();
                let modseq = world.bump();
                let Some(folder) = world.folders.iter_mut().find(|folder| folder.path == path) else {
                    return Some(format!("{tag} NO [TRYCREATE] no such folder"));
                };
                let strict = folder.permanent.as_ref().filter(|kept| !kept.contains("\\*"));
                if let Some(kept) = strict
                    && flags.iter().any(|flag| !flag.starts_with('\\') && !kept.split_whitespace().any(|k| k == flag))
                {
                    return Some(format!("{tag} NO [CANNOT] keyword not allowed here"));
                }
                let uid = folder.next_uid;
                folder.next_uid += 1;
                folder.modseq = modseq;
                folder.messages.push(Message::new(uid, flags.clone(), modseq, raw.clone(), received));
                let landed = (folder.uidvalidity, uid);
                if let Some(mirror) = mirror
                    && let Some(all) = world.folders.iter_mut().find(|folder| folder.path == mirror)
                {
                    let uid = all.next_uid;
                    all.next_uid += 1;
                    all.modseq = modseq;
                    all.messages.push(Message::new(uid, flags, modseq, raw, received));
                }
                landed
            };
            self.changed();
            if std::mem::take(&mut *self.drop_after_append.lock().expect("append cut")) {
                return None;
            }
            if self.uidplus.load(Ordering::Acquire) {
                return Some(format!("{tag} OK [APPENDUID {} {}] APPEND completed", landed.0, landed.1));
            }
            Some(format!("{tag} OK APPEND completed"))
        }

        /// `EXPUNGE`: removes the messages marked `\Deleted` from the open folder.
        fn expunge_deleted(&self, tag: &str, selected: Option<&str>) -> String {
            let Some(path) = selected else { return format!("{tag} BAD no folder selected") };
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(path);
                folder.modseq = modseq;
                folder.messages.retain(|message| !message.flags.contains("\\Deleted"));
            }
            self.changed();
            format!("{tag} OK Expunge completed")
        }

        /// `UID STORE set ±FLAGS.SILENT (…)` in the open folder.
        fn store_flags(&self, tag: &str, path: &str, arguments: &str) -> String {
            let mut parts = arguments.splitn(3, ' ');
            let (Some(set), Some(change), Some(list)) = (parts.next(), parts.next(), parts.next()) else {
                return format!("{tag} BAD broken STORE");
            };
            let flags: Vec<String> =
                list.trim_matches(|c| c == '(' || c == ')').split_whitespace().map(str::to_string).collect();
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(path);
                folder.modseq = modseq;
                for message in folder.messages.iter_mut().filter(|message| in_set(set, message.uid)) {
                    for flag in &flags {
                        if change.starts_with('+') {
                            message.flags.insert(flag.clone());
                        } else {
                            message.flags.remove(flag);
                        }
                    }
                    message.modseq = modseq;
                }
            }
            self.changed();
            format!("{tag} OK Store completed")
        }

        /// `UID COPY set "target"` from the open folder: copies under new UIDs, in UID order.
        fn copy(&self, tag: &str, path: &str, arguments: &str) -> String {
            let (set, target) = arguments.split_once(' ').unwrap_or((arguments, ""));
            let target = quoted(target);
            {
                let mut world = self.world();
                if !world.folders.iter().any(|folder| folder.path == target) {
                    return format!("{tag} NO [TRYCREATE] no such folder");
                }
                let modseq = world.bump();
                let copies: Vec<(BTreeSet<String>, Vec<u8>, DateTime<FixedOffset>)> = world
                    .folder(path)
                    .messages
                    .iter()
                    .filter(|message| in_set(set, message.uid))
                    .map(|message| (message.flags.clone(), message.raw.clone(), message.received))
                    .collect();
                let folder = world.folder(&target);
                folder.modseq = modseq;
                for (flags, raw, received) in copies {
                    let uid = folder.next_uid;
                    folder.next_uid += 1;
                    folder.messages.push(Message::new(uid, flags, modseq, raw, received));
                }
            }
            self.changed();
            format!("{tag} OK Copy completed")
        }

        fn uid(&self, tag: &str, rest: &str, selected: Option<&str>) -> Vec<u8> {
            let Some(path) = selected else { return format!("{tag} BAD no folder selected").into() };
            if let Some(arguments) = rest.strip_prefix("STORE ") {
                return self.store_flags(tag, path, arguments).into();
            }
            if let Some(arguments) = rest.strip_prefix("COPY ") {
                return self.copy(tag, path, arguments).into();
            }
            let mut world = self.world();
            let folder = world.folder(path);
            let mut lines = Vec::new();
            if let Some(criteria) = rest.strip_prefix("SEARCH ") {
                let uids: Vec<String> = folder
                    .messages
                    .iter()
                    .filter(|message| matches_search(criteria, message))
                    .map(|message| message.uid.to_string())
                    .collect();
                lines.push(format!("* SEARCH {}", uids.join(" ")).trim_end().as_bytes().to_vec());
            } else if let Some(fetch) = rest.strip_prefix("FETCH ") {
                let (set, items) = fetch.split_once(' ').unwrap_or((fetch, ""));
                let since = items
                    .split_once("CHANGEDSINCE ")
                    .and_then(|(_, after)| after.trim_end_matches(')').parse::<u64>().ok());
                for (at, message) in folder.messages.iter().enumerate() {
                    if !in_set(set, message.uid)
                        || since.is_some_and(|since| message.modseq <= since)
                        || (message.withheld && items.contains("BODY.PEEK[]"))
                    {
                        continue;
                    }
                    lines.push(fetched(at + 1, message, items));
                }
            } else {
                return format!("{tag} BAD unknown UID command").into();
            }
            lines.push(format!("{tag} OK done").into());
            lines.join(b"\r\n".as_slice())
        }
    }

    /// One `FETCH` response for `message` (sequence number `seq`) with what `items` asks for, as
    /// bytes: a body goes out exactly as stored, also when a piece ends inside a UTF-8 character.
    fn fetched(seq: usize, message: &Message, items: &str) -> Vec<u8> {
        let flags = message.flags.iter().cloned().collect::<Vec<_>>().join(" ");
        let size = message.raw.len();
        let claimed = message.reported_size.unwrap_or(size);
        let date = message.received.format("%d-%b-%Y %H:%M:%S %z");
        let literal = |head: String, body: &[u8]| {
            let mut out = format!("{head} {{{}}}\r\n", body.len()).into_bytes();
            out.extend_from_slice(body);
            out.push(b')');
            out
        };
        if items.contains("BODY.PEEK[HEADER]") {
            let end = message.raw.windows(4).position(|window| window == b"\r\n\r\n").map_or(size, |at| at + 4);
            let head = format!(
                "* {seq} FETCH (UID {} FLAGS ({flags}) INTERNALDATE \"{date}\" RFC822.SIZE {claimed} BODYSTRUCTURE \
                 (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"8bit\" {size} 1 NIL NIL NIL) MODSEQ ({}) \
                 BODY[HEADER]",
                message.uid, message.modseq
            );
            literal(head, &message.raw[..end])
        } else if let Some(part) = items.split("BODY.PEEK[]<").nth(1) {
            let (offset, count) =
                part.trim_end_matches(")").trim_end_matches('>').split_once('.').expect("partial range");
            let offset: usize = offset.parse().expect("partial offset");
            let count: usize = count.parse().expect("partial count");
            let end = offset.saturating_add(count).min(size);
            let piece = message.raw.get(offset..end).unwrap_or_default();
            literal(format!("* {seq} FETCH (UID {} BODY[]<{offset}>", message.uid), piece)
        } else if items.contains("BODY.PEEK[]") {
            literal(format!("* {seq} FETCH (UID {} BODY[]", message.uid), &message.raw)
        } else if items.contains("INTERNALDATE") && items.contains("RFC822.SIZE") {
            let gmail = match message.gm_msgid {
                Some(msgid) if items.contains("X-GM-MSGID") => format!(" X-GM-MSGID {msgid}"),
                _ => String::new(),
            };
            format!(
                "* {seq} FETCH (UID {} FLAGS ({flags}) INTERNALDATE \"{date}\" RFC822.SIZE {claimed}{gmail})",
                message.uid
            )
            .into_bytes()
        } else if items.contains("RFC822.SIZE") && message.sizeless {
            format!("* {seq} FETCH (UID {})", message.uid).into_bytes()
        } else if items.contains("RFC822.SIZE") {
            format!("* {seq} FETCH (UID {} RFC822.SIZE {claimed})", message.uid).into_bytes()
        } else {
            format!("* {seq} FETCH (UID {} FLAGS ({flags}) MODSEQ ({}))", message.uid, message.modseq).into_bytes()
        }
    }

    /// `uid` is in the sequence set `set` (`1:3,5,7:*`).
    fn in_set(set: &str, uid: u32) -> bool {
        set.split(',').any(|part| match part.split_once(':') {
            Some((from, "*")) => from.parse::<u32>().is_ok_and(|from| uid >= from),
            Some((from, to)) => {
                matches!((from.parse::<u32>(), to.parse::<u32>()), (Ok(from), Ok(to)) if (from..=to).contains(&uid))
            }
            None => part.parse::<u32>().is_ok_and(|one| one == uid),
        })
    }

    /// The first quoted string of `text`, without quotes and escapes; the text up to the first
    /// space when it is not quoted.
    fn quoted(text: &str) -> String {
        let Some(inner) = text.strip_prefix('"') else {
            return text.split(' ').next().unwrap_or_default().to_string();
        };
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.extend(chars.next()),
                '"' => break,
                c => out.push(c),
            }
        }
        out
    }

    // ----- search criteria and literals (task M3.1) -----

    /// The command line `line` without its line end, with every literal (`{n}` at the end of a
    /// line, as a client sends a word that is not plain ASCII) asked for with `+`, read and put in
    /// as a quoted string. `None` when the connection broke.
    async fn with_literals<R, W>(reader: &mut R, writer: &mut W, line: &str) -> Option<String>
    where
        R: tokio::io::AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        use tokio::io::AsyncReadExt;
        let mut text = line.trim_end_matches(['\r', '\n']).to_string();
        while let Some(open) = text.strip_suffix('}').and_then(|inner| inner.rfind('{')) {
            let size: usize = text[open + 1..text.len() - 1].parse().ok()?;
            text.truncate(open);
            write(writer, "+ go on").await.ok()?;
            let mut literal = vec![0u8; size];
            reader.read_exact(&mut literal).await.ok()?;
            let literal = String::from_utf8_lossy(&literal).replace('\\', "\\\\").replace('"', "\\\"");
            let mut rest = String::new();
            reader.read_line(&mut rest).await.ok()?;
            text = format!("{text}\"{literal}\"{}", rest.trim_end_matches(['\r', '\n']));
        }
        Some(text)
    }

    /// `message` matches the `UID SEARCH` criteria: `ALL`, `UID <set>`, `CHARSET <name>` (taken
    /// as UTF-8), `TEXT <string>` (anywhere in the raw message, any case), `SINCE`/`BEFORE <day>`.
    /// The days compare with the `Date:` header, since the fake receives every message now.
    fn matches_search(criteria: &str, message: &Message) -> bool {
        let raw = String::from_utf8_lossy(&message.raw);
        let date = date_of(&raw).unwrap_or(message.received.to_utc()).date_naive();
        let lower = raw.to_lowercase();
        let day =
            |text: Option<String>| text.and_then(|text| chrono::NaiveDate::parse_from_str(&text, "%d-%b-%Y").ok());
        let mut words = search_words(criteria).into_iter();
        while let Some(word) = words.next() {
            let holds = match word.to_ascii_uppercase().as_str() {
                "ALL" => true,
                "CHARSET" => words.next().is_some(),
                "UID" => words.next().is_some_and(|set| in_set(&set, message.uid)),
                "TEXT" => words.next().is_some_and(|text| lower.contains(&text.to_lowercase())),
                "SINCE" => day(words.next()).is_some_and(|since| date >= since),
                "BEFORE" => day(words.next()).is_some_and(|before| date < before),
                _ => false,
            };
            if !holds {
                return false;
            }
        }
        true
    }

    /// The criteria split into atoms and quoted strings (without quotes and escapes).
    fn search_words(criteria: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut chars = criteria.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                ' ' => {}
                '"' => {
                    let mut word = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => word.extend(chars.next()),
                            '"' => break,
                            c => word.push(c),
                        }
                    }
                    words.push(word);
                }
                c => {
                    let mut word = c.to_string();
                    while let Some(c) = chars.next_if(|c| *c != ' ') {
                        word.push(c);
                    }
                    words.push(word);
                }
            }
        }
        words
    }

    /// The `Date:` header of a raw message.
    fn date_of(raw: &str) -> Option<DateTime<Utc>> {
        let date = raw.lines().take_while(|line| !line.is_empty()).find_map(|line| line.strip_prefix("Date:"))?;
        DateTime::parse_from_rfc2822(date.trim()).ok().map(|date| date.with_timezone(&Utc))
    }

    async fn write<W: AsyncWrite + Unpin>(writer: &mut W, text: &str) -> std::io::Result<()> {
        write_bytes(writer, text.as_bytes()).await
    }

    /// `bytes` and a line end, as one answer.
    async fn write_bytes<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
        writer.write_all(bytes).await?;
        writer.write_all(b"\r\n").await?;
        writer.flush().await
    }

    /// The capabilities after the sign-in; `gmail` adds `X-GM-EXT-1`.
    fn world_capabilities(gmail: bool) -> String {
        if gmail { format!("{CAPABILITIES} X-GM-EXT-1") } else { CAPABILITIES.to_string() }
    }

    fn acceptor() -> TlsAcceptor {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture("server.key.der")));
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(fixture("server.der"))], key)
            .expect("server certificate");
        TlsAcceptor::from(Arc::new(config))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use torro_mail::MessageSummary;

    use super::fake::{self, FakeImap};
    use super::*;
    use crate::mail::service::tests::{PASSWORD, mail_account, mailbox_in};
    use crate::mail::store::SearchQuery;

    async fn mailbox(dir: &Path, server: &FakeImap) -> Mailbox {
        mailbox_in(dir, mail_account("anna@example.org", &server.endpoint), PASSWORD).await
    }

    async fn all(mailbox: &Mailbox) -> Vec<MessageSummary> {
        let store = mailbox.store().expect("store");
        store.run(|store| store.search(&SearchQuery::default(), 1000, 0)).await.expect("searched")
    }

    async fn text_of(mailbox: &Mailbox, id: &str) -> Option<String> {
        let (store, id) = (mailbox.store().expect("store"), id.to_string());
        store.run(move |store| Ok(store.message(&id)?.and_then(|message| message.text))).await.expect("read")
    }

    fn fetches(server: &FakeImap, what: &str) -> Vec<String> {
        server.lines().into_iter().filter(|line| line.contains("UID FETCH") && line.contains(what)).collect()
    }

    #[tokio::test]
    async fn the_first_sync_takes_the_newest_headers_first_in_batches_and_the_recent_texts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for n in 1..=130 {
            // UID 1 is the oldest: 129 days, UID 130 today. Mail 40 is unambiguously
            // outside the 90-day boundary even when the test and sync share one clock second.
            let days_ago = if n == 40 { 91 } else { 130 - n };
            server.add("INBOX", &fake::mail(&format!("Mail {n}"), days_ago, None), &["\\Seen"]);
        }
        server.add("Archive", &fake::mail("Kept", 3, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let mut changes = mailbox_events(&mailbox);

        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");

        let headers = fetches(&server, "BODY.PEEK[HEADER]");
        assert!(headers[0].contains(" 31:130 "), "the newest hundred first: {headers:?}");
        assert!(headers[1].contains(" 1:30 "), "then the rest: {headers:?}");
        let first = changes.try_recv().expect("the first batch is announced");
        assert!(first.initial && first.new.len() == HEADER_BATCH && first.role == Some(Role::Inbox), "{first:?}");
        let messages = all(&mailbox).await;
        assert_eq!(messages.len(), 131);
        let newest = messages.iter().find(|m| m.subject == "Mail 130").expect("synced");
        assert_eq!((newest.flags.as_slice(), newest.snippet.as_str()), ([Flag::Seen].as_slice(), "Text of Mail 130"));
        assert_eq!(text_of(&mailbox, &newest.id).await.as_deref(), Some("Text of Mail 130\n"));
        let old = messages.iter().find(|m| m.subject == "Mail 1").expect("synced");
        assert_eq!(text_of(&mailbox, &old.id).await, None, "older than 90 days: only the headers");
        let bodies = fetches(&server, "BODY.PEEK[]");
        // Mail 41 to 130 (Mail 40 is older than 90 days) and the archive's one.
        assert_eq!(bodies.len(), 90 + 1, "texts of the last 90 days only");
        let stores: Vec<String> = server.lines().into_iter().filter(|line| line.contains(" UID STORE ")).collect();
        assert!(stores.is_empty(), "syncing never marks mail read: {stores:?}");
    }

    #[tokio::test]
    async fn all_offline_sync_retains_exact_original_and_keyword_for_local_backup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = fake::mail("Original", 0, None);
        server.add("INBOX", &raw, &["\\Seen", "$Work"]);
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");

        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let originals = store.run(|db| db.local_originals_page(0, 10)).await.expect("listed");
        assert_eq!(originals.len(), 1);
        assert_eq!(originals[0].flags, vec!["$Work", "\\Seen"]);
        let seq = originals[0].seq;
        assert_eq!(store.run(move |db| db.original_bytes(seq)).await.expect("read"), Some(raw.into_bytes()));
        server.set_flags("INBOX", 1, &["\\Answered", "$Updated"]);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("flags synced");
        let updated = store.run(|db| db.local_originals_page(0, 10)).await.expect("listed again");
        assert_eq!(updated[0].flags, vec!["$Updated", "\\Answered"]);
    }

    #[tokio::test]
    async fn all_offline_backfill_retains_a_message_above_the_chunk_threshold() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let raw = format!("From: a@example.org\r\nSubject: Large\r\n\r\n{}", "x".repeat((33 << 20) + 1));
        server.add("INBOX", &raw, &["$Large"]);
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("headers synced");
        backfill(&mailbox, 1, Utc::now()).await.expect("chunked original capture");
        let originals = store.run(|db| db.local_originals_page(0, 10)).await.expect("listed");
        assert_eq!(originals.len(), 1);
        assert_eq!(originals[0].bytes, raw.len() as u64);
        assert_eq!(originals[0].flags, vec!["$Large"]);
        let seq = originals[0].seq;
        let actual = store.run(move |db| db.original_bytes(seq)).await.expect("original").expect("present");
        assert_eq!(actual, raw.as_bytes());
        assert!(server.lines().iter().any(|line| line.contains("BODY.PEEK[]<")), "partial FETCH used");
        let past_the_end = format!("BODY.PEEK[]<{}.1>", raw.len());
        assert!(server.lines().iter().any(|line| line.contains(&past_the_end)), "nothing follows RFC822.SIZE");

        // Asked again (a cache job), the depot answers: nothing is fetched or written anew.
        let id = all(&mailbox).await[0].id.clone();
        let place = store.run(move |db| db.resolve_message(&id)).await.expect("resolved").expect("there");
        let asked = server.lines().len();
        assert!(!cache_body(&mailbox, &place).await.expect("asked"), "too large to read");
        let again: Vec<String> =
            server.lines()[asked..].iter().filter(|line| line.contains("UID FETCH")).cloned().collect();
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(store.run(|db| db.local_originals_page(0, 10)).await.expect("listed")[0].seq, seq);
    }

    #[tokio::test]
    async fn an_original_that_cannot_be_kept_leaves_a_gap_and_reading_goes_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let large = format!("From: a@example.org\r\nSubject: Large\r\n\r\n{}", "x".repeat((33 << 20) + 1));
        let uid = server.add("INBOX", &large, &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("headers synced");
        // Its body vanishes while it is taken: the original cannot be kept.
        server.withhold_text("INBOX", uid);
        // Not fetched ahead either (above `BODY_PREFETCH_MAX`): the backfill comes to it after the gap.
        let next = format!("{}{}\r\n", fake::mail("After it", 400, None), "z".repeat(6 << 20));
        server.add("INBOX", &next, &[]);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("the next one synced");

        assert_eq!(backfill(&mailbox, 10, Utc::now()).await.expect("not stopped by the gap"), 2);
        let originals = store.run(|db| db.local_originals_page(0, 10)).await.expect("listed");
        assert_eq!(originals.iter().map(|original| original.uid).collect::<Vec<_>>(), vec![uid + 1]);
        assert_eq!(store.run(|db| db.known_missing_originals(None)).await.expect("counted"), 1, "the gap shows");
        assert_eq!(backfill(&mailbox, 10, Utc::now()).await.expect("nothing left"), 0, "not asked again");
    }

    #[tokio::test]
    async fn reading_again_does_not_take_a_kept_original_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Kept", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced with its original");
        let metadata = || fetches(&server, "(UID FLAGS INTERNALDATE RFC822.SIZE)").len();
        assert_eq!(metadata(), 1);
        let id = all(&mailbox).await[0].id.clone();
        let place = store.run(move |db| db.resolve_message(&id)).await.expect("resolved").expect("there");
        assert!(cache_body(&mailbox, &place).await.expect("read again"));
        assert_eq!(metadata(), 1, "the depot holds it already");
        assert_eq!(store.run(|db| db.local_originals_page(0, 10)).await.expect("listed").len(), 1);
    }

    #[tokio::test]
    async fn the_sync_marks_originals_gone_when_their_mail_is_renumbered_or_expunged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Only", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let places = || async {
            let listed = store.run(|db| db.local_originals_page(0, 10)).await.expect("listed");
            listed.into_iter().map(|original| (original.uid_validity, original.uid, original.gone)).collect::<Vec<_>>()
        };
        assert_eq!(places().await, vec![(11, 1, false)]);

        server.renumber("INBOX");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced after renumbering");
        assert_eq!(places().await, vec![(11, 1, true), (111, 1001, false)], "the mail is kept once, anew");
        server.expunge("INBOX", 1001);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced after the expunge");
        assert_eq!(places().await, vec![(11, 1, true), (111, 1001, true)]);

        // What a local backup names the folder by, as the server lists it.
        let infos = store.run(|db| db.folder_infos()).await.expect("folders");
        let inbox = infos.iter().find(|info| info.path == "INBOX").expect("the inbox");
        assert_eq!((inbox.delimiter, inbox.role.as_deref()), (Some('/'), Some("inbox")));
    }

    #[tokio::test]
    async fn switching_to_all_offline_refetches_an_existing_body_without_an_original() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Previously cached", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("recent text synced");
        let store = mailbox.store().expect("store");
        assert!(store.run(|db| db.local_originals_page(0, 10)).await.expect("listed").is_empty());
        store
            .run(|db| {
                let mut policy = db.cache_policy()?;
                policy.mode = MailCacheMode::AllOffline;
                db.set_cache_policy(&policy)
            })
            .await
            .expect("all offline");
        let todo = store
            .run(|db| db.cached_body_missing(None, MailCacheMode::AllOffline, 10))
            .await
            .expect("missing originals");
        assert_eq!(todo.len(), 1);
        assert!(cache_body(&mailbox, &todo[0].1).await.expect("fetched"));
        assert_eq!(store.run(|db| db.local_originals_page(0, 10)).await.expect("listed").len(), 1);
    }

    #[tokio::test]
    async fn a_second_sync_takes_over_removals_and_flags_and_skips_unchanged_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let first = server.add("INBOX", &fake::mail("First", 2, None), &[]);
        let second = server.add("INBOX", &fake::mail("Second", 1, Some("First@example.org")), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let before = all(&mailbox).await;
        assert_eq!(before[0].thread_id, before[1].thread_id, "a reply is in the thread of what it answers");
        let lines = server.lines().len();

        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced again");
        let again: Vec<String> = server.lines()[lines..].to_vec();
        assert!(!again.iter().any(|line| line.contains("UID ")), "nothing changed, nothing fetched: {again:?}");

        let mut changes = mailbox_events(&mailbox);
        server.expunge("INBOX", first);
        server.set_flags("INBOX", second, &["\\Flagged"]);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced after the changes");
        let change = changes.try_recv().expect("announced");
        let first_id = before.iter().find(|m| m.subject == "First").map(|m| m.id.clone());
        let second_id = before.iter().find(|m| m.subject == "Second").map(|m| m.id.clone());
        assert_eq!((change.removed, change.flags), (Vec::from_iter(first_id), Vec::from_iter(second_id)));
        let after = all(&mailbox).await;
        assert_eq!((after.len(), after[0].flags.as_slice()), (1, [Flag::Flagged].as_slice()));
        assert!(
            server.lines().iter().any(|line| line.contains("(CHANGEDSINCE ")),
            "only changed flags are asked for with CONDSTORE"
        );
    }

    /// Issue #318: while a flag change is on its way to the server, a sync that reads the server's
    /// older flags leaves the store's alone and keeps no HIGHESTMODSEQ; once the change is done, the
    /// next sync compares again and keeps it.
    #[tokio::test]
    async fn a_sync_leaves_flags_on_their_way_to_the_server_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &fake::mail("Read me", 1, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let store = mailbox.store().expect("store");
        let inbox = store.run(|db| db.folder_by_path("INBOX")).await.expect("read").expect("the inbox");
        let modseq = |store: SharedStore, folder: String| async move {
            store.run(move |db| db.resolve_folder(&folder)).await.expect("read").expect("there").highest_modseq
        };
        let synced = modseq(store.clone(), inbox.id.clone()).await;
        assert!(synced.is_some(), "the fake offers CONDSTORE");
        let id = all(&mailbox).await[0].id.clone();

        // `mail.set_flags` holds the message, marks it read in the store, and has not reached the
        // server yet; meanwhile the server's HIGHESTMODSEQ grows without the flag.
        let held = mailbox.hold_flags(&inbox.id, &[uid]);
        let read = id.clone();
        store.run(move |db| db.set_flags(&read, &[Flag::Seen])).await.expect("marked read");
        server.set_flags("INBOX", uid, &[]);
        let mut changes = mailbox_events(&mailbox);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced while the change is on its way");
        assert_eq!(all(&mailbox).await[0].flags, [Flag::Seen], "the change stays");
        assert_eq!(modseq(store.clone(), inbox.id.clone()).await, synced, "the folder is compared again next time");
        while let Ok(change) = changes.try_recv() {
            assert!(change.flags.is_empty(), "no flag change announced: {change:?}");
        }

        // The server has it now, the change is done: the next sync takes the server's flags over.
        server.set_flags("INBOX", uid, &["\\Seen"]);
        drop(held);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced after the change");
        assert_eq!(all(&mailbox).await[0].flags, [Flag::Seen]);
        let kept = modseq(store.clone(), inbox.id.clone()).await;
        assert!(kept > synced, "kept again: {kept:?} after {synced:?}");
    }

    /// Issue #318: the header phase of a sync keeps the store's flags of a message it finds stored
    /// already (moved into the folder after the sync read it) while a flag change of it is on its
    /// way. This checks that decision ([`flags_on_their_way`]); no sync runs here, because the
    /// message would have to arrive in the store between two steps of one.
    #[tokio::test]
    async fn new_headers_keep_the_flags_on_their_way_to_the_server() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &fake::mail("Moved here", 1, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let store = mailbox.store().expect("store");
        let inbox = store.run(|db| db.folder_by_path("INBOX")).await.expect("read").expect("the inbox").id;
        let id = all(&mailbox).await[0].id.clone();
        let underway = mailbox.underway();
        let kept = |uid: u32| {
            let (store, underway, inbox) = (store.clone(), Arc::clone(&underway), inbox.clone());
            async move { store.run(move |db| flags_on_their_way(db, &underway, &inbox, uid)).await.expect("read") }
        };
        assert_eq!(kept(uid).await, None, "nothing on its way: the server's flags");

        let held = mailbox.hold_flags(&inbox, &[uid, uid + 1]);
        store.run(move |db| db.set_flags(&id, &[Flag::Flagged])).await.expect("flagged");
        assert_eq!(kept(uid).await, Some(vec![Flag::Flagged]), "on its way: the store's");
        assert_eq!(kept(uid + 1).await, None, "a message the store does not have yet");
        drop(held);
        assert_eq!(kept(uid).await, Some(vec![Flag::Flagged]), "done while the sync ran");
        underway.lock().unwrap_or_else(PoisonError::into_inner).sync_started(&inbox);
        assert_eq!(kept(uid).await, None, "the next sync takes the server's");
    }

    /// DS.1 (#314): the sync keeps every message's size; sizes a store of before lacks are filled
    /// in by the next sync, also of a folder that did not change, and then nothing is asked again.
    #[tokio::test]
    async fn sizes_are_kept_and_missing_ones_filled_in() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let (mut bytes, mut last) = (0, (0, 0));
        for n in 1..=3 {
            let raw = fake::mail(&format!("Sized {n}"), 200, None);
            bytes += raw.len() as u64;
            last = (server.add("INBOX", &raw, &[]), raw.len() as u64);
        }
        let mailbox = mailbox(dir.path(), &server).await;
        let store = mailbox.store().expect("store");
        let inbox_bytes = |store: &SharedStore| {
            let store = store.clone();
            async move {
                let stats = store.run(|store| store.account_stats()).await.expect("stats");
                stats.folders.into_iter().find(|folder| folder.folder.path == "INBOX").and_then(|f| f.server_bytes)
            }
        };

        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        assert_eq!(inbox_bytes(&store).await, Some(bytes), "from the headers' RFC822.SIZE");
        assert!(fetches(&server, "RFC822.SIZE)").is_empty(), "nothing to fill in");

        store.run(|store| store.forget_sizes()).await.expect("a store of before");
        assert_eq!(inbox_bytes(&store).await, None);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced again");
        assert_eq!(fetches(&server, "(UID RFC822.SIZE)").len(), 1, "one batch for the inbox");
        assert_eq!(inbox_bytes(&store).await, Some(bytes), "filled in without a change in the folder");

        let lines = server.lines().len();
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced a third time");
        assert!(!server.lines()[lines..].iter().any(|line| line.contains("UID ")), "nothing asked again");

        // A server that names no size for a message: it counts as 0 and is not asked for again.
        server.withhold_size("INBOX", last.0);
        store.run(|store| store.forget_sizes()).await.expect("a store of before");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced without one size");
        assert_eq!(inbox_bytes(&store).await, Some(bytes - last.1), "the sizeless message counts as 0");
        let lines = server.lines().len();
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced once more");
        assert!(
            !server.lines()[lines..].iter().any(|line| line.contains("UID ")),
            "the sizeless one is not asked again"
        );
    }

    /// DS.1 (#314): the storage quota is kept from a server with `QUOTA` and cleared by one
    /// without, which is not asked.
    #[tokio::test]
    async fn the_quota_is_kept_and_cleared_without_quota() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.set_quota(Some((2475, 10_485_760)));
        let quota_of = |mailbox: &Mailbox| {
            let store = mailbox.store().expect("store");
            async move { store.run(|store| store.account_stats()).await.expect("stats").quota }
        };
        let first = mailbox(dir.path(), &server).await;
        sync_account(&first, MailSyncTrigger::Manual).await.expect("synced");
        let quota = quota_of(&first).await.expect("kept");
        assert_eq!((quota.used_bytes, quota.limit_bytes), (2475 * 1024, 10_737_418_240));
        assert_eq!(server.lines().iter().filter(|line| line.contains("GETQUOTAROOT \"INBOX\"")).count(), 1);
        drop(first);

        // A new sign-in, which no longer hears of QUOTA.
        server.set_quota(None);
        let second = mailbox(dir.path(), &server).await;
        let lines = server.lines().len();
        sync_account(&second, MailSyncTrigger::Manual).await.expect("synced without QUOTA");
        assert!(!server.lines()[lines..].iter().any(|line| line.contains("GETQUOTAROOT")), "not asked");
        assert_eq!(quota_of(&second).await, None, "the old quota is gone");
    }

    #[tokio::test]
    async fn a_renumbered_folder_is_synced_from_scratch() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Only", 1, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let before = all(&mailbox).await;

        let mut changes = mailbox_events(&mailbox);
        server.renumber("INBOX");
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced after renumbering");
        let removed = changes.try_recv().expect("the old ids are gone");
        assert_eq!((removed.removed, removed.initial), (vec![before[0].id.clone()], true));
        let added = changes.try_recv().expect("and come back under new ones");
        assert!(added.initial && added.new.len() == 1);
        let after = all(&mailbox).await;
        assert_eq!((after.len(), after[0].subject.as_str()), (1, "Only"));
        assert_ne!(after[0].id, before[0].id);
    }

    #[tokio::test]
    async fn the_inbox_sync_of_the_idle_connection_waits_for_the_folder_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Waiting", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let mut session = mailbox.connect().await.expect("connected");
        sync_inbox(&mut session, &mailbox).await.expect("nothing to do yet");
        assert!(all(&mailbox).await.is_empty());
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        server.add("INBOX", &fake::mail("Next", 0, None), &[]);
        sync_inbox(&mut session, &mailbox).await.expect("the inbox synced");
        assert_eq!(all(&mailbox).await.len(), 2);
        assert_eq!(session.selected().map(|selected| selected.path.as_str()), Some("INBOX"), "ready to IDLE");
    }

    #[test]
    fn threads_follow_the_first_reference_and_messages_without_ids_stand_alone() {
        let mut read = unreadable();
        assert_eq!(thread_id(&read), None);
        read.message_id = Some("b@x".into());
        let alone = thread_id(&read);
        read.in_reply_to = Some("a@x".into());
        let reply = thread_id(&read);
        read.references = vec!["root@x".into(), "a@x".into()];
        let deeper = thread_id(&read);
        assert_ne!(alone, reply);
        assert_ne!(reply, deeper);
        let mut root = unreadable();
        root.message_id = Some("root@x".into());
        assert_eq!(thread_id(&root), deeper);
    }

    fn mailbox_events(mailbox: &Mailbox) -> tokio::sync::broadcast::Receiver<MailChange> {
        mailbox.subscribe()
    }

    /// The record of the folder named `name` in the account's sync state (issue #422).
    fn record<'a>(status: &'a torro_ipc::mail::MailSyncStatus, name: &str) -> &'a torro_ipc::mail::MailFolderSync {
        status.folders.iter().find(|folder| folder.name == name).expect("a record of the folder")
    }

    /// Issue #422: a run reports its folders in the order it syncs them, the inbox first, with the
    /// messages on the server, how it ended and how long it took; a second run without changes
    /// finds the folders up to date.
    #[tokio::test]
    async fn a_run_reports_every_folder_and_then_finds_them_up_to_date() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for n in 1..=3 {
            server.add("INBOX", &fake::mail(&format!("Mail {n}"), 0, None), &[]);
        }
        server.add("Archive", &fake::mail("Kept", 3, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;

        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        // The inbox first: three new headers, their three texts (recent), no size missing.
        let phases = mailbox.progress().phases();
        assert_eq!(
            phases[..3],
            [(MailSyncPhase::Headers, 3), (MailSyncPhase::Texts, 3), (MailSyncPhase::Sizes, 0)],
            "{phases:?}"
        );
        let status = mailbox.progress().status(mailbox.id());
        assert_eq!(status.running, None);
        assert!(!status.queued);
        let last = status.last_run.clone().expect("a last run");
        assert!(last.ok && last.problem.is_none(), "{last:?}");
        assert_eq!(last.trigger, MailSyncTrigger::Manual);
        assert_eq!(last.folders_done, last.folder_count);
        assert_eq!(status.folders[0].name, "INBOX", "the inbox first");
        let inbox = record(&status, "INBOX");
        assert_eq!((inbox.on_server, inbox.state), (Some(3), MailFolderSyncState::Synced));
        assert!(inbox.last_synced.is_some() && inbox.duration_ms.is_some());
        assert_eq!(record(&status, "Archive").on_server, Some(1));

        sync_account(&mailbox, MailSyncTrigger::Scheduled).await.expect("synced again");
        let status = mailbox.progress().status(mailbox.id());
        assert_eq!(record(&status, "INBOX").state, MailFolderSyncState::UpToDate, "nothing changed");
        assert_eq!(status.last_run.expect("a last run").trigger, MailSyncTrigger::Scheduled);
    }

    /// Issue #422: a folder that fails ends the run there; the folders after it are not reached and
    /// the run keeps why it ended.
    #[tokio::test]
    async fn a_failed_folder_ends_the_run_and_leaves_the_rest_not_reached() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("First", 0, None), &[]);
        server.add("Archive", &fake::mail("Kept", 3, None), &[]);
        server.add_folder("Zarchive", "\\HasNoChildren", 77);
        server.add("Zarchive", &fake::mail("Later", 3, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect("synced");
        let order: Vec<String> =
            mailbox.progress().status(mailbox.id()).folders.iter().map(|folder| folder.name.clone()).collect();

        // A folder within the run is refused: every folder after it waits for the next run.
        let refused = "Archive".to_string();
        let at = order.iter().position(|name| *name == refused).expect("the archive is listed");
        assert!(at > 0 && at + 1 < order.len(), "folders before and after it: {order:?}");
        server.refuse(&refused);
        sync_account(&mailbox, MailSyncTrigger::Manual).await.expect_err("a refused folder ends the run");
        let status = mailbox.progress().status(mailbox.id());
        let last = status.last_run.clone().expect("a last run");
        assert!(!last.ok && last.problem.is_some(), "{last:?}");
        assert_eq!(last.folders_done, u32::try_from(at).expect("few folders"), "the folders before it");
        assert_eq!(last.failed_at, u32::try_from(at + 1).ok(), "the refused folder, counted from 1");
        assert_eq!(record(&status, "INBOX").state, MailFolderSyncState::UpToDate);
        assert_eq!(record(&status, &refused).state, MailFolderSyncState::Failed);
        for name in &order[at + 1..] {
            assert_eq!(record(&status, name).state, MailFolderSyncState::NotReached, "{name}");
            assert!(record(&status, name).last_synced.is_some(), "{name} keeps when the run before synced it");
        }
    }

    /// Issue #422: the inbox sync of the IDLE connection is no run: it only updates the inbox's
    /// record.
    #[tokio::test]
    async fn the_inbox_sync_of_the_idle_connection_updates_only_the_inbox_record() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Waiting", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox, MailSyncTrigger::Start).await.expect("synced");
        let before = mailbox.progress().status(mailbox.id());
        server.add("INBOX", &fake::mail("Next", 0, None), &[]);
        let mut session = mailbox.connect().await.expect("connected");
        sync_inbox(&mut session, &mailbox).await.expect("the inbox synced");
        let after = mailbox.progress().status(mailbox.id());
        assert_eq!(after.running, None, "no run");
        assert_eq!(after.last_run, before.last_run, "the last run stays");
        let inbox = record(&after, "INBOX");
        assert_eq!((inbox.on_server, inbox.state), (Some(2), MailFolderSyncState::Synced));
    }
}
