//! The mail store of one account (task M2.5): folders, message headers, texts, a full-text index
//! and the author of each draft, in one SQLite file `<data>/mail/<account hash>.sqlite` (WAL,
//! mode 0600, folder 0700).
//!
//! **Ids.** Clients see opaque ids: `f:<random>` for a folder, `m:<random>` for a message. The
//! store hands them out and is the only place that resolves them ([`MailStore::resolve_folder`],
//! [`MailStore::resolve_message`]) to (account, folder path, UIDVALIDITY, UID); nothing splits an
//! id. A folder keeps its id when the server renumbers it, a message keeps its id when it is moved
//! ([`MailStore::relocate_message`]). The file is per account, so an id from a client is looked up
//! in the stores of the accounts that client may reach, one after the other (a primary-key lookup
//! each); an id that no reachable store knows is `not-found`.
//!
//! **UIDVALIDITY.** A message row is only valid under the UIDVALIDITY of its folder. Writers pass
//! the value they worked with ([`MailStore::upsert_message`]); a changed value is announced with
//! [`MailStore::note_uidvalidity`], which drops every message of the folder (rows, texts and index
//! entries) and keeps the folder and its id.
//!
//! **What is kept** is decided here, not by the person: headers of everything, texts for
//! [`BODY_RETENTION_DAYS`] (see [`MailStore::prune_bodies`]). Attachments are not in the database;
//! see `AttachmentCache`.
//!
//! **Full text.** `mail_fts` (FTS5, `unicode61 remove_diacritics 2`) holds subject, sender,
//! recipients, text and attachment names, so `Muller` finds `Müller` and the other way round.
//! The words of a search are all required and each matches as a prefix.
//!
//! Every message is data from outside: the summaries and messages this store builds are always
//! `untrusted`, see `torro_mail::MessageSummary`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use sha2::{Digest, Sha256};
use torro_mail::{AttachmentInfo, Flag, Folder, MailAddress, Message, MessageSummary, MessageTraits, Role};

use super::{MailDirs, MailStoreError, create_private, make_private, random_hex};

/// Message texts older than this many days are dropped by [`MailStore::prune_bodies`]; headers stay.
pub const BODY_RETENTION_DAYS: i64 = 90;

/// The newest layout this version writes (`PRAGMA user_version`).
const SCHEMA_VERSION: i64 = 1;

const FLAG_SEEN: i64 = 1;
const FLAG_FLAGGED: i64 = 2;
const FLAG_ANSWERED: i64 = 4;

const SCHEMA_V1: &str = "
CREATE TABLE folders (
    id             TEXT PRIMARY KEY NOT NULL,
    path           TEXT NOT NULL UNIQUE,
    name           TEXT NOT NULL,
    role           TEXT,
    uidvalidity    INTEGER,
    highest_modseq INTEGER
);
CREATE TABLE messages (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    id               TEXT NOT NULL UNIQUE,
    folder_id        TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid              INTEGER NOT NULL,
    rfc_message_id   TEXT,
    thread_id        TEXT NOT NULL,
    from_json        TEXT,
    to_json          TEXT NOT NULL,
    cc_json          TEXT NOT NULL,
    reply_to_json    TEXT NOT NULL,
    subject          TEXT NOT NULL,
    sender           TEXT NOT NULL,
    recipients       TEXT NOT NULL,
    date             INTEGER NOT NULL,
    snippet          TEXT NOT NULL,
    flags            INTEGER NOT NULL,
    has_attachments  INTEGER NOT NULL,
    list_id          TEXT,
    list_unsubscribe INTEGER NOT NULL,
    precedence       TEXT,
    auto_submitted   TEXT,
    invitation       INTEGER NOT NULL,
    UNIQUE (folder_id, uid)
);
CREATE INDEX messages_by_date ON messages (folder_id, date DESC);
CREATE INDEX messages_by_thread ON messages (thread_id);
CREATE TABLE bodies (
    seq              INTEGER PRIMARY KEY NOT NULL REFERENCES messages(seq) ON DELETE CASCADE,
    text             TEXT NOT NULL,
    from_html        INTEGER NOT NULL,
    attachments_json TEXT NOT NULL,
    filenames        TEXT NOT NULL
);
CREATE VIRTUAL TABLE mail_fts USING fts5(
    subject, sender, recipients, body, filenames,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER messages_gone AFTER DELETE ON messages BEGIN
    DELETE FROM mail_fts WHERE rowid = old.seq;
END;
CREATE TABLE drafts (
    id         TEXT PRIMARY KEY NOT NULL,
    author     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
";

/// Who wrote a draft (security rule 2 of task M2.1): a client key replaces and sends only its own
/// drafts, so the core remembers the author of each one. A draft nobody registered (written in
/// another mail program) belongs to the person: [`MailStore::draft_author`] gives `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code, reason = "used from drafts, task M3.2")]
pub enum DraftAuthor {
    /// The person, through the app key (Inbox app, control centre).
    App,
    /// A client key; the string is the client's id (`clients.json`), never the key.
    Client(String),
}

#[allow(dead_code, reason = "used from drafts, task M3.2")]
impl DraftAuthor {
    fn to_column(&self) -> String {
        match self {
            DraftAuthor::App => "app".to_string(),
            DraftAuthor::Client(id) => format!("client:{id}"),
        }
    }

    fn from_column(text: &str) -> Option<DraftAuthor> {
        match text.strip_prefix("client:") {
            Some(id) => Some(DraftAuthor::Client(id.to_string())),
            None => (text == "app").then_some(DraftAuthor::App),
        }
    }
}

/// What a folder id resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRef {
    /// The folder's id (`f:…`).
    pub id: String,
    /// The account (`mail:<address>`).
    pub account: String,
    /// The path as the server names it; what `SELECT` takes.
    pub path: String,
    /// The folder's UIDVALIDITY, `None` before the first sync.
    pub uidvalidity: Option<u32>,
    /// HIGHESTMODSEQ at the last sync (CONDSTORE), `None` when unknown or not offered.
    pub highest_modseq: Option<u64>,
}

/// What a message id resolves to: where the message is on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRef {
    /// The message's id (`m:…`).
    pub id: String,
    /// The account (`mail:<address>`).
    pub account: String,
    /// The id of the folder it is in.
    pub folder: String,
    /// The folder's path as the server names it.
    pub folder_path: String,
    /// The UIDVALIDITY the UID is valid under.
    pub uidvalidity: u32,
    /// The UID in that folder.
    pub uid: u32,
    /// The `Message-ID:` header, for `In-Reply-To` and `References` when answering.
    pub rfc_message_id: Option<String>,
}

/// What [`MailStore::note_uidvalidity`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UidValidity {
    /// Same as before: nothing to do.
    Unchanged,
    /// The folder had none yet (first sync): nothing was dropped.
    First,
    /// The server renumbered the folder: all its messages were dropped, sync it from scratch.
    Reset,
}

/// The headers of a message as the sync reads them, to be stored by [`MailStore::upsert_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    /// Use this id for a new row: a draft's id from [`MailStore::new_draft_id`]. `None`: a new
    /// random one. Ignored when the folder already has a row for `uid`.
    pub id: Option<String>,
    /// The UID in its folder.
    pub uid: u32,
    /// `Message-ID:`.
    pub rfc_message_id: Option<String>,
    /// The conversation key the sync worked out; the same value groups messages into a thread.
    pub thread_id: String,
    pub from: Option<MailAddress>,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub reply_to: Vec<MailAddress>,
    pub subject: String,
    /// `Date:` or, if that is missing or broken, when the server received it.
    pub date: DateTime<Utc>,
    pub snippet: String,
    pub flags: Vec<Flag>,
    pub has_attachments: bool,
    pub traits: MessageTraits,
    pub invitation: bool,
}

/// The text of a message and its attachments, to be stored by [`MailStore::set_body`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// Plain text, or the HTML part turned into text.
    pub text: String,
    /// `text` was made from HTML.
    pub from_html: bool,
    /// All attachments, inline parts included; their names go into the search index.
    pub attachments: Vec<AttachmentInfo>,
}

/// A search over the cache; every field narrows the result (`mail.search_messages`). All `None`
/// lists everything, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Only this folder (an id from [`MailStore::folders`]).
    pub folder: Option<String>,
    /// Words that must all appear in subject, sender, recipients, text or attachment names; each
    /// matches as a prefix, accents do not matter. Whitespace only counts as no text.
    pub text: Option<String>,
    /// `Some(true)`: not yet read; `Some(false)`: read.
    pub unread: Option<bool>,
    /// `Some(true)`: flagged; `Some(false)`: not flagged.
    pub flagged: Option<bool>,
    /// Sent at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Sent before this time.
    pub before: Option<DateTime<Utc>>,
}

/// The store of one account. `Send`, not `Sync`: share it as `Arc<Mutex<MailStore>>` and call it
/// from async code through `tokio::task::spawn_blocking`. Methods take `&self`; each write is one
/// transaction, so a crash leaves no half-written message.
#[derive(Debug)]
pub struct MailStore {
    account: String,
    path: PathBuf,
    db: Connection,
}

impl MailStore {
    /// Opens the store of `account` (`mail:<address>`) in `dirs.data`, creating the directory
    /// (0700), the file (0600) and the tables if needed. Fails with [`MailStoreError::NoFullText`]
    /// when SQLite has no FTS5, so a missing search shows at once, not at the first query.
    pub fn open(dirs: &MailDirs, account: &str) -> Result<MailStore, MailStoreError> {
        crate::clients::create_private_dir(&dirs.data)?;
        let path = file_for(dirs, account);
        // Create the file with its mode before SQLite does; SQLite gives `-wal` and `-shm` the
        // mode of the main file.
        create_private(&path)?;
        let db = Connection::open(&path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let _mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        db.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrate(&db)?;
        let store = MailStore { account: account.to_string(), path, db };
        // Tighten the side files anyway, in case a looser version made them.
        store.restrict_files()?;
        Ok(store)
    }

    /// Deletes the store of `account`: its file and the `-wal`/`-shm` files. Close the store first
    /// (drop it). Nothing there is nothing to do. Called when the account is removed.
    #[allow(dead_code, reason = "used from accounts.remove_account, task M2.7")]
    pub fn remove(dirs: &MailDirs, account: &str) -> Result<(), MailStoreError> {
        remove_files(&file_for(dirs, account))
    }

    /// Deletes the stores in `dirs.data` that belong to none of `accounts` (`mail:<address>`),
    /// each with `-wal` and `-shm`; returns how many stores went. A store whose account was
    /// removed while Windows still kept its file open stays behind; this clean-up, run at start and
    /// every hour, deletes it later. Other files in the folder are left alone, and so is a store
    /// changed less than `min_age` ago: one connected just now may not be in `accounts` yet.
    pub fn remove_unknown(
        dirs: &MailDirs,
        accounts: &[String],
        min_age: std::time::Duration,
    ) -> Result<usize, MailStoreError> {
        let known: Vec<PathBuf> = accounts.iter().map(|account| file_for(dirs, account)).collect();
        let entries = match std::fs::read_dir(&dirs.data) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(err.into()),
        };
        let mut removed = 0;
        for entry in entries {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
            // The store itself, or a `-wal`/`-shm` left without it.
            let Some(stem) =
                ["", "-wal", "-shm"].iter().find_map(|suffix| name.strip_suffix(&format!(".sqlite{suffix}")))
            else {
                continue;
            };
            let store = dirs.data.join(format!("{stem}.sqlite"));
            if known.contains(&store) || !path.is_file() || changed_within(&path, min_age) {
                continue;
            }
            if remove_files(&store).is_ok() && name.ends_with(".sqlite") {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// The account this store belongs to (`mail:<address>`).
    #[allow(dead_code, reason = "used from the mail commands, task M3.1")]
    pub fn account(&self) -> &str {
        &self.account
    }

    fn restrict_files(&self) -> Result<(), MailStoreError> {
        for file in [self.path.clone(), side_file(&self.path, "-wal"), side_file(&self.path, "-shm")] {
            if file.exists() {
                make_private(&file)?;
            }
        }
        Ok(())
    }

    // ----- folders -----

    /// Adds the folder at `path` or updates its display name and role; returns its id. The id,
    /// UIDVALIDITY and modseq of a known folder stay.
    pub fn upsert_folder(&self, path: &str, name: &str, role: Option<Role>) -> Result<String, MailStoreError> {
        let known: Option<String> =
            self.db.query_row("SELECT id FROM folders WHERE path = ?1", [path], |row| row.get(0)).optional()?;
        let role = role.map(role_name);
        if let Some(id) = known {
            self.db.execute("UPDATE folders SET name = ?2, role = ?3 WHERE id = ?1", params![id, name, role])?;
            return Ok(id);
        }
        let id = self.fresh_id("f:", "folders")?;
        self.db.execute(
            "INSERT INTO folders (id, path, name, role) VALUES (?1, ?2, ?3, ?4)",
            params![id, path, name, role],
        )?;
        Ok(id)
    }

    /// Drops every folder whose path is not in `paths` (deleted on the server) with its messages;
    /// returns how many folders went. An empty list drops all, which a sync must not send by
    /// mistake: pass the whole `LIST` result.
    pub fn retain_folders(&self, paths: &[String]) -> Result<usize, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let known: Vec<(String, String)> = {
            let mut statement = tx.prepare("SELECT id, path FROM folders")?;
            statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?
        };
        let mut removed = 0;
        for (id, path) in known {
            if !paths.contains(&path) {
                tx.execute("DELETE FROM folders WHERE id = ?1", [&id])?;
                removed += 1;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// All folders, ordered by role (inbox first) and then path. `unread` and `total` are counted
    /// from the stored headers.
    pub fn folders(&self) -> Result<Vec<Folder>, MailStoreError> {
        let mut statement = self.db.prepare(&format!("{FOLDER_SELECT} ORDER BY {FOLDER_ORDER}"))?;
        let rows = statement.query_map([], |row| folder_from_row(&self.account, row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One folder by id.
    #[allow(dead_code, reason = "used from mail.list_folders, task M3.1")]
    pub fn folder(&self, id: &str) -> Result<Option<Folder>, MailStoreError> {
        let sql = format!("{FOLDER_SELECT} WHERE f.id = ?1");
        Ok(self.db.query_row(&sql, [id], |row| folder_from_row(&self.account, row)).optional()?)
    }

    /// The folder with this role, if the account has one.
    #[allow(dead_code, reason = "used from mail.move_messages, task M3.2")]
    pub fn folder_by_role(&self, role: Role) -> Result<Option<Folder>, MailStoreError> {
        let sql = format!("{FOLDER_SELECT} WHERE f.role = ?1 ORDER BY f.path LIMIT 1");
        Ok(self.db.query_row(&sql, [role_name(role)], |row| folder_from_row(&self.account, row)).optional()?)
    }

    /// Resolves a folder id (`None`: this account does not have it).
    pub fn resolve_folder(&self, id: &str) -> Result<Option<FolderRef>, MailStoreError> {
        self.folder_ref("WHERE id = ?1", id)
    }

    /// Finds a folder by the path the server names it (for the sync, which works with paths).
    pub fn folder_by_path(&self, path: &str) -> Result<Option<FolderRef>, MailStoreError> {
        self.folder_ref("WHERE path = ?1", path)
    }

    fn folder_ref(&self, filter: &str, value: &str) -> Result<Option<FolderRef>, MailStoreError> {
        let sql = format!("SELECT id, path, uidvalidity, highest_modseq FROM folders {filter}");
        let row = self.db.query_row(&sql, [value], |row| {
            Ok(FolderRef {
                id: row.get(0)?,
                account: self.account.clone(),
                path: row.get(1)?,
                uidvalidity: row.get(2)?,
                highest_modseq: row.get::<_, Option<i64>>(3)?.and_then(|value| u64::try_from(value).ok()),
            })
        });
        Ok(row.optional()?)
    }

    /// Tells the store which UIDVALIDITY the server reports for the folder. A different value than
    /// stored drops all messages of the folder ([`UidValidity::Reset`]) and its modseq; the
    /// folder and its id stay. Call this when the folder is selected, before reading or writing
    /// messages of it.
    pub fn note_uidvalidity(&self, folder: &str, uidvalidity: u32) -> Result<UidValidity, MailStoreError> {
        let current = self.resolve_folder(folder)?.ok_or(MailStoreError::UnknownFolder)?;
        match current.uidvalidity {
            Some(seen) if seen == uidvalidity => Ok(UidValidity::Unchanged),
            None => {
                self.db.execute("UPDATE folders SET uidvalidity = ?2 WHERE id = ?1", params![folder, uidvalidity])?;
                Ok(UidValidity::First)
            }
            Some(_) => {
                let tx = self.db.unchecked_transaction()?;
                tx.execute("DELETE FROM messages WHERE folder_id = ?1", [folder])?;
                tx.execute(
                    "UPDATE folders SET uidvalidity = ?2, highest_modseq = NULL WHERE id = ?1",
                    params![folder, uidvalidity],
                )?;
                tx.commit()?;
                Ok(UidValidity::Reset)
            }
        }
    }

    /// Remembers HIGHESTMODSEQ of the last complete sync of the folder (`None`: forget it).
    pub fn set_highest_modseq(&self, folder: &str, modseq: Option<u64>) -> Result<(), MailStoreError> {
        let modseq = modseq.and_then(|value| i64::try_from(value).ok());
        let changed =
            self.db.execute("UPDATE folders SET highest_modseq = ?2 WHERE id = ?1", params![folder, modseq])?;
        if changed == 0 { Err(MailStoreError::UnknownFolder) } else { Ok(()) }
    }

    // ----- messages: writing -----

    /// Stores the headers of a message of `folder`, which the caller read under `uidvalidity`.
    /// Updates the row of the same UID (id, text and thread stay unless the headers say
    /// otherwise), else adds one. Returns the message's id. [`MailStoreError::StaleUidValidity`] if
    /// the folder's UIDVALIDITY is not `uidvalidity` (call [`note_uidvalidity`](Self::note_uidvalidity)
    /// first): the UID would mean another message.
    pub fn upsert_message(&self, folder: &str, uidvalidity: u32, new: &NewMessage) -> Result<String, MailStoreError> {
        self.check_uidvalidity(folder, uidvalidity)?;
        let tx = self.db.unchecked_transaction()?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT seq, id FROM messages WHERE folder_id = ?1 AND uid = ?2",
                params![folder, new.uid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let values = header_values(new)?;
        let (seq, id) = match existing {
            Some((seq, id)) => {
                tx.execute(UPDATE_HEADERS, params_from_iter(std::iter::once(Value::Integer(seq)).chain(values)))?;
                (seq, id)
            }
            None => {
                let id = match &new.id {
                    Some(id) => id.clone(),
                    None => self.fresh_id("m:", "messages")?,
                };
                let all =
                    [Value::Text(id.clone()), Value::Text(folder.to_string()), Value::Integer(i64::from(new.uid))]
                        .into_iter()
                        .chain(values);
                tx.execute(INSERT_HEADERS, params_from_iter(all))?;
                (tx.last_insert_rowid(), id)
            }
        };
        refresh_index(&tx, seq)?;
        tx.commit()?;
        Ok(id)
    }

    /// Sets the flags of a message to exactly `flags`. `false`: no such message.
    #[allow(dead_code, reason = "used from mail.set_flags, task M3.2")]
    pub fn set_flags(&self, id: &str, flags: &[Flag]) -> Result<bool, MailStoreError> {
        Ok(self.db.execute("UPDATE messages SET flags = ?2 WHERE id = ?1", params![id, flag_bits(flags)])? > 0)
    }

    /// Sets the flags of the message with this UID (a sync that sees a change on the server).
    /// `false`: no such message.
    pub fn set_flags_by_uid(&self, folder: &str, uid: u32, flags: &[Flag]) -> Result<bool, MailStoreError> {
        let sql = "UPDATE messages SET flags = ?3 WHERE folder_id = ?1 AND uid = ?2";
        Ok(self.db.execute(sql, params![folder, uid, flag_bits(flags)])? > 0)
    }

    /// Stores (or replaces) the text of a message and puts it into the search index. `false`: no
    /// such message.
    pub fn set_body(&self, id: &str, body: &Body) -> Result<bool, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let seq: Option<i64> =
            tx.query_row("SELECT seq FROM messages WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        let Some(seq) = seq else { return Ok(false) };
        let names: Vec<&str> = body.attachments.iter().filter_map(|a| a.filename.as_deref()).collect();
        tx.execute(
            "INSERT INTO bodies (seq, text, from_html, attachments_json, filenames) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (seq) DO UPDATE SET text = ?2, from_html = ?3, attachments_json = ?4, filenames = ?5",
            params![seq, body.text, body.from_html, serde_json::to_string(&body.attachments)?, names.join(" ")],
        )?;
        refresh_index(&tx, seq)?;
        tx.commit()?;
        Ok(true)
    }

    /// Drops the texts of messages sent more than [`BODY_RETENTION_DAYS`] before `now`; their
    /// headers stay and `Message::text` is `None` again until someone fetches it. Returns how many
    /// texts went. Run it after a sync and now and then.
    pub fn prune_bodies(&self, now: DateTime<Utc>) -> Result<usize, MailStoreError> {
        let limit = (now - Duration::days(BODY_RETENTION_DAYS)).timestamp();
        let tx = self.db.unchecked_transaction()?;
        let old: Vec<i64> = {
            let mut statement =
                tx.prepare("SELECT b.seq FROM bodies b JOIN messages m ON m.seq = b.seq WHERE m.date < ?1")?;
            statement.query_map([limit], |row| row.get(0))?.collect::<Result<_, _>>()?
        };
        for seq in &old {
            tx.execute("DELETE FROM bodies WHERE seq = ?1", [seq])?;
            refresh_index(&tx, *seq)?;
        }
        tx.commit()?;
        Ok(old.len())
    }

    /// Moves a message to another folder under its UID there; the id stays (a client may hold on
    /// to it). `uidvalidity` is the target folder's, as for [`upsert_message`](Self::upsert_message).
    /// `false`: no such message.
    #[allow(dead_code, reason = "used from mail.move_messages and drafts, task M3.2")]
    pub fn relocate_message(&self, id: &str, folder: &str, uidvalidity: u32, uid: u32) -> Result<bool, MailStoreError> {
        self.check_uidvalidity(folder, uidvalidity)?;
        // A row already at the target UID would be a leftover of an older sync; the moved
        // message replaces it.
        let tx = self.db.unchecked_transaction()?;
        tx.execute("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2 AND id <> ?3", params![folder, uid, id])?;
        let changed =
            tx.execute("UPDATE messages SET folder_id = ?2, uid = ?3 WHERE id = ?1", params![id, folder, uid])?;
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Removes one message with its text and index entry (`false`: no such message). A draft's
    /// author stays until [`forget_draft`](Self::forget_draft).
    #[allow(dead_code, reason = "used from mail.delete_messages, task M3.2")]
    pub fn delete_message(&self, id: &str) -> Result<bool, MailStoreError> {
        Ok(self.db.execute("DELETE FROM messages WHERE id = ?1", [id])? > 0)
    }

    /// Removes the messages with these UIDs from the folder (they are gone on the server);
    /// returns how many were there.
    pub fn delete_uids(&self, folder: &str, uids: &[u32]) -> Result<usize, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let mut removed = 0;
        for uid in uids {
            removed += tx.execute("DELETE FROM messages WHERE folder_id = ?1 AND uid = ?2", params![folder, uid])?;
        }
        tx.commit()?;
        Ok(removed)
    }

    // ----- messages: reading -----

    /// Resolves a message id to where it is on the server (`None`: this account does not have it,
    /// or the sync dropped it).
    pub fn resolve_message(&self, id: &str) -> Result<Option<MessageRef>, MailStoreError> {
        let sql = "SELECT m.id, m.folder_id, f.path, f.uidvalidity, m.uid, m.rfc_message_id
                   FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.id = ?1";
        let row = self.db.query_row(sql, [id], |row| {
            Ok((
                MessageRef {
                    id: row.get(0)?,
                    account: self.account.clone(),
                    folder: row.get(1)?,
                    folder_path: row.get(2)?,
                    uidvalidity: 0,
                    uid: row.get(4)?,
                    rfc_message_id: row.get(5)?,
                },
                row.get::<_, Option<u32>>(3)?,
            ))
        });
        // A row only exists under a UIDVALIDITY (`upsert_message` checks it), so it is set.
        Ok(row.optional()?.and_then(|(message, uidvalidity)| Some(MessageRef { uidvalidity: uidvalidity?, ..message })))
    }

    /// The id of the message with this UID, if the store has it.
    pub fn message_id_for_uid(&self, folder: &str, uid: u32) -> Result<Option<String>, MailStoreError> {
        let sql = "SELECT id FROM messages WHERE folder_id = ?1 AND uid = ?2";
        Ok(self.db.query_row(sql, params![folder, uid], |row| row.get(0)).optional()?)
    }

    /// The UIDs the store holds for the folder with their flags, ascending: what a sync compares
    /// with the server.
    pub fn folder_uids(&self, folder: &str) -> Result<Vec<(u32, Vec<Flag>)>, MailStoreError> {
        let mut statement = self.db.prepare("SELECT uid, flags FROM messages WHERE folder_id = ?1 ORDER BY uid")?;
        let rows = statement.query_map([folder], |row| Ok((row.get(0)?, flags_from_bits(row.get(1)?))))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// A message as a list shows it.
    pub fn summary(&self, id: &str) -> Result<Option<MessageSummary>, MailStoreError> {
        let sql = format!("SELECT {SUMMARY_COLUMNS} FROM messages m WHERE m.id = ?1");
        Ok(self.db.query_row(&sql, [id], |row| summary_from_row(&self.account, row)).optional()?)
    }

    /// A whole message. `text` is `None` while the text is not stored (never fetched, or older
    /// than [`BODY_RETENTION_DAYS`]): the caller fetches it from the server with `BODY.PEEK` and
    /// stores it with [`set_body`](Self::set_body).
    pub fn message(&self, id: &str) -> Result<Option<Message>, MailStoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS}, m.cc_json, m.reply_to_json, m.list_id, m.list_unsubscribe, m.precedence,
                    m.auto_submitted, m.invitation, b.text, b.from_html, b.attachments_json
             FROM messages m LEFT JOIN bodies b ON b.seq = m.seq WHERE m.id = ?1"
        );
        let row = self.db.query_row(&sql, [id], |row| {
            let summary = summary_from_row(&self.account, row)?;
            let attachments_json: Option<String> = row.get(19)?;
            Ok(Message {
                summary,
                cc: json_column(row, 10)?,
                reply_to: json_column(row, 11)?,
                text: row.get(17)?,
                from_html: row.get::<_, Option<bool>>(18)?.unwrap_or(false),
                attachments: match attachments_json {
                    Some(json) => parse_json(&json, 19)?,
                    None => Vec::new(),
                },
                traits: MessageTraits {
                    list_id: row.get(12)?,
                    list_unsubscribe: row.get(13)?,
                    precedence: row.get(14)?,
                    auto_submitted: row.get(15)?,
                },
                invitation: row.get(16)?,
            })
        });
        Ok(row.optional()?)
    }

    /// The messages of a folder, newest first; `limit` and `offset` page through them.
    #[allow(dead_code, reason = "used from mail.search_messages, task M3.1")]
    pub fn list_messages(&self, folder: &str, limit: u32, offset: u32) -> Result<Vec<MessageSummary>, MailStoreError> {
        let query = SearchQuery { folder: Some(folder.to_string()), ..SearchQuery::default() };
        self.search(&query, limit, offset)
    }

    /// Searches the cache, newest first; `limit` and `offset` page through the result. A search
    /// whose words hold nothing searchable (only punctuation) finds nothing.
    pub fn search(&self, query: &SearchQuery, limit: u32, offset: u32) -> Result<Vec<MessageSummary>, MailStoreError> {
        let mut from = "messages m".to_string();
        let mut conditions: Vec<&str> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if let Some(text) = query.text.as_deref().filter(|text| !text.trim().is_empty()) {
            let Some(expression) = match_expression(text) else { return Ok(Vec::new()) };
            from.push_str(" JOIN mail_fts ON mail_fts.rowid = m.seq");
            conditions.push("mail_fts MATCH ?");
            values.push(Value::Text(expression));
        }
        if let Some(folder) = &query.folder {
            conditions.push("m.folder_id = ?");
            values.push(Value::Text(folder.clone()));
        }
        if let Some(unread) = query.unread {
            conditions.push(if unread { "m.flags & 1 = 0" } else { "m.flags & 1 = 1" });
        }
        if let Some(flagged) = query.flagged {
            conditions.push(if flagged { "m.flags & 2 = 2" } else { "m.flags & 2 = 0" });
        }
        if let Some(since) = query.since {
            conditions.push("m.date >= ?");
            values.push(Value::Integer(since.timestamp()));
        }
        if let Some(before) = query.before {
            conditions.push("m.date < ?");
            values.push(Value::Integer(before.timestamp()));
        }
        let filter = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
        let sql =
            format!("SELECT {SUMMARY_COLUMNS} FROM {from} {filter} ORDER BY m.date DESC, m.seq DESC LIMIT ? OFFSET ?");
        values.push(Value::Integer(i64::from(limit)));
        values.push(Value::Integer(i64::from(offset)));
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| summary_from_row(&self.account, row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// All messages of a conversation (any folder), oldest first. The thread of a message is
    /// `summary(id)?.thread_id`.
    #[allow(dead_code, reason = "used from mail.get_thread, task M3.1")]
    pub fn thread(&self, thread_id: &str) -> Result<Vec<MessageSummary>, MailStoreError> {
        let sql = format!("SELECT {SUMMARY_COLUMNS} FROM messages m WHERE m.thread_id = ?1 ORDER BY m.date, m.seq");
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([thread_id], |row| summary_from_row(&self.account, row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The newest `limit` messages of a conversation (any folder), oldest first: a thread a sender
    /// made up to be huge stays bounded (`mail.get_thread`, task M3.1).
    pub fn thread_newest(&self, thread_id: &str, limit: u32) -> Result<Vec<MessageSummary>, MailStoreError> {
        let sql = format!(
            "SELECT * FROM (SELECT {SUMMARY_COLUMNS}, m.seq AS seq_order FROM messages m WHERE m.thread_id = ?1
             ORDER BY m.date DESC, m.seq DESC LIMIT ?2) ORDER BY date, seq_order"
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params![thread_id, limit], |row| summary_from_row(&self.account, row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ----- drafts -----

    /// Reserves the id of a new draft and remembers its `author`. The id is a message id: build
    /// the message in the drafts folder with [`NewMessage::id`] set to it. To replace a draft,
    /// [`relocate_message`](Self::relocate_message) it to its new UID so the id stays.
    #[allow(dead_code, reason = "used from drafts, task M3.2")]
    pub fn new_draft_id(&self, author: &DraftAuthor) -> Result<String, MailStoreError> {
        let id = self.fresh_id("m:", "messages")?;
        self.db.execute(
            "INSERT INTO drafts (id, author, created_at) VALUES (?1, ?2, ?3)",
            params![id, author.to_column(), Utc::now().timestamp()],
        )?;
        Ok(id)
    }

    /// Who wrote the draft. `None`: nobody registered it, so it is the person's (written in
    /// another program) and a client key may not touch it (`not-found`).
    #[allow(dead_code, reason = "used from drafts, task M3.2")]
    pub fn draft_author(&self, id: &str) -> Result<Option<DraftAuthor>, MailStoreError> {
        let author: Option<String> =
            self.db.query_row("SELECT author FROM drafts WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        Ok(author.as_deref().and_then(DraftAuthor::from_column))
    }

    /// Forgets the author of a draft that was sent or deleted. `false`: it was not registered.
    #[allow(dead_code, reason = "used from drafts, task M3.2")]
    pub fn forget_draft(&self, id: &str) -> Result<bool, MailStoreError> {
        Ok(self.db.execute("DELETE FROM drafts WHERE id = ?1", [id])? > 0)
    }

    // ----- internals -----

    fn check_uidvalidity(&self, folder: &str, uidvalidity: u32) -> Result<(), MailStoreError> {
        match self.resolve_folder(folder)? {
            None => Err(MailStoreError::UnknownFolder),
            Some(found) if found.uidvalidity == Some(uidvalidity) => Ok(()),
            Some(_) => Err(MailStoreError::StaleUidValidity),
        }
    }

    /// A random id with `prefix` that no row of `table` has.
    fn fresh_id(&self, prefix: &str, table: &str) -> Result<String, MailStoreError> {
        loop {
            let id = format!("{prefix}{}", random_hex(12)?);
            let taken: bool = self.db.query_row(
                &format!(
                    "SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1) OR EXISTS (SELECT 1 FROM drafts WHERE id = ?1)"
                ),
                [&id],
                |row| row.get(0),
            )?;
            if !taken {
                return Ok(id);
            }
        }
    }
}

/// The file of an account: a hash of its id, so no address shows in a file name.
fn file_for(dirs: &MailDirs, account: &str) -> PathBuf {
    let hash: String = Sha256::digest(account.as_bytes()).iter().take(16).map(|byte| format!("{byte:02x}")).collect();
    dirs.data.join(format!("{hash}.sqlite"))
}

/// `path` was changed less than `age` ago (or its age cannot be told, which counts as recent).
fn changed_within(path: &Path, age: std::time::Duration) -> bool {
    let modified = std::fs::metadata(path).and_then(|meta| meta.modified());
    modified.map_or(true, |at| at.elapsed().map_or(true, |elapsed| elapsed < age))
}

/// Deletes the store `path` with its `-wal` and `-shm`; a missing file is no error. On Windows a
/// file someone still has open (or a virus scanner looks into just after it was closed) cannot be
/// deleted: tried again for up to two seconds before giving up.
fn remove_files(path: &Path) -> Result<(), MailStoreError> {
    for file in [path.to_path_buf(), side_file(path, "-wal"), side_file(path, "-shm")] {
        let mut tries = 0;
        loop {
            match std::fs::remove_file(&file) {
                Ok(()) => break,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if cfg!(windows) && tries < 20 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
    Ok(())
}

fn side_file(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn migrate(db: &Connection) -> Result<(), MailStoreError> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(MailStoreError::NewerSchema(version));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    if let Err(err) = tx.execute_batch(SCHEMA_V1) {
        // `no such module: fts5` from the virtual table.
        return Err(if err.to_string().contains("fts5") { MailStoreError::NoFullText } else { err.into() });
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}

// ----- SQL pieces -----

const FOLDER_SELECT: &str = "SELECT f.id, f.name, f.path, f.role,
        (SELECT count(*) FROM messages m WHERE m.folder_id = f.id AND m.flags & 1 = 0),
        (SELECT count(*) FROM messages m WHERE m.folder_id = f.id)
    FROM folders f";

/// Inbox, drafts, sent, archive, junk, trash, then folders without a role; by path inside each.
const FOLDER_ORDER: &str =
    "CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2 WHEN 'archive' THEN 3
        WHEN 'junk' THEN 4 WHEN 'trash' THEN 5 ELSE 6 END, f.path";

const SUMMARY_COLUMNS: &str =
    "m.id, m.folder_id, m.thread_id, m.from_json, m.to_json, m.subject, m.date, m.snippet, m.flags, m.has_attachments";

const INSERT_HEADERS: &str = "INSERT INTO messages (id, folder_id, uid, rfc_message_id, thread_id, from_json, to_json,
    cc_json, reply_to_json, subject, sender, recipients, date, snippet, flags, has_attachments, list_id,
    list_unsubscribe, precedence, auto_submitted, invitation)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

const UPDATE_HEADERS: &str = "UPDATE messages SET rfc_message_id = ?2, thread_id = ?3, from_json = ?4, to_json = ?5,
    cc_json = ?6, reply_to_json = ?7, subject = ?8, sender = ?9, recipients = ?10, date = ?11, snippet = ?12,
    flags = ?13, has_attachments = ?14, list_id = ?15, list_unsubscribe = ?16, precedence = ?17,
    auto_submitted = ?18, invitation = ?19 WHERE seq = ?1";

/// The header values in the order the two statements above take them after their leading columns.
fn header_values(new: &NewMessage) -> Result<Vec<Value>, MailStoreError> {
    let text = |value: &str| Value::Text(value.to_string());
    let optional = |value: &Option<String>| value.as_deref().map_or(Value::Null, text);
    let sender = new.from.as_ref().map(address_words).unwrap_or_default();
    let recipients: Vec<String> = new.to.iter().chain(&new.cc).map(address_words).collect();
    Ok(vec![
        optional(&new.rfc_message_id),
        text(&new.thread_id),
        match &new.from {
            Some(from) => Value::Text(serde_json::to_string(from)?),
            None => Value::Null,
        },
        Value::Text(serde_json::to_string(&new.to)?),
        Value::Text(serde_json::to_string(&new.cc)?),
        Value::Text(serde_json::to_string(&new.reply_to)?),
        text(&new.subject),
        Value::Text(sender),
        Value::Text(recipients.join(" ")),
        Value::Integer(new.date.timestamp()),
        text(&new.snippet),
        Value::Integer(flag_bits(&new.flags)),
        Value::Integer(i64::from(new.has_attachments)),
        optional(&new.traits.list_id),
        Value::Integer(i64::from(new.traits.list_unsubscribe)),
        optional(&new.traits.precedence),
        optional(&new.traits.auto_submitted),
        Value::Integer(i64::from(new.invitation)),
    ])
}

/// Name and address as words for the index.
fn address_words(address: &MailAddress) -> String {
    match &address.name {
        Some(name) => format!("{name} {}", address.address),
        None => address.address.clone(),
    }
}

/// Rewrites the index entry of one message from its row and its text (if any).
fn refresh_index(db: &Connection, seq: i64) -> Result<(), MailStoreError> {
    db.execute("DELETE FROM mail_fts WHERE rowid = ?1", [seq])?;
    db.execute(
        "INSERT INTO mail_fts (rowid, subject, sender, recipients, body, filenames)
         SELECT m.seq, m.subject, m.sender, m.recipients, coalesce(b.text, ''), coalesce(b.filenames, '')
         FROM messages m LEFT JOIN bodies b ON b.seq = m.seq WHERE m.seq = ?1",
        [seq],
    )?;
    Ok(())
}

/// Every word as a quoted prefix term, all required. Words without a letter or digit are left
/// out (the tokenizer would make an empty phrase of them); `None` when nothing is left.
fn match_expression(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() { None } else { Some(terms.join(" AND ")) }
}

// ----- rows -----

fn folder_from_row(account: &str, row: &Row<'_>) -> rusqlite::Result<Folder> {
    let role: Option<String> = row.get(3)?;
    Ok(Folder {
        id: row.get(0)?,
        account: account.to_string(),
        name: row.get(1)?,
        path: row.get(2)?,
        role: role.as_deref().and_then(role_from_name),
        unread: row.get(4)?,
        total: row.get(5)?,
    })
}

fn summary_from_row(account: &str, row: &Row<'_>) -> rusqlite::Result<MessageSummary> {
    let from: Option<String> = row.get(3)?;
    let seconds: i64 = row.get(6)?;
    Ok(MessageSummary {
        id: row.get(0)?,
        account: account.to_string(),
        folder: row.get(1)?,
        thread_id: row.get(2)?,
        from: from.map(|json| parse_json(&json, 3)).transpose()?,
        to: json_column(row, 4)?,
        subject: row.get(5)?,
        date: DateTime::from_timestamp(seconds, 0).ok_or_else(|| conversion_failure(6, "date out of range"))?,
        snippet: row.get(7)?,
        flags: flags_from_bits(row.get(8)?),
        has_attachments: row.get(9)?,
        untrusted: true,
    })
}

fn json_column<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let json: String = row.get(index)?;
    parse_json(&json, index)
}

fn parse_json<T: serde::de::DeserializeOwned>(json: &str, index: usize) -> rusqlite::Result<T> {
    serde_json::from_str(json).map_err(|err| conversion_failure(index, &err.to_string()))
}

fn conversion_failure(index: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, what.to_string().into())
}

fn flag_bits(flags: &[Flag]) -> i64 {
    flags.iter().fold(0, |bits, flag| {
        bits | match flag {
            Flag::Seen => FLAG_SEEN,
            Flag::Flagged => FLAG_FLAGGED,
            Flag::Answered => FLAG_ANSWERED,
        }
    })
}

fn flags_from_bits(bits: i64) -> Vec<Flag> {
    [(FLAG_SEEN, Flag::Seen), (FLAG_FLAGGED, Flag::Flagged), (FLAG_ANSWERED, Flag::Answered)]
        .into_iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, flag)| flag)
        .collect()
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Inbox => "inbox",
        Role::Drafts => "drafts",
        Role::Sent => "sent",
        Role::Archive => "archive",
        Role::Junk => "junk",
        Role::Trash => "trash",
    }
}

fn role_from_name(name: &str) -> Option<Role> {
    [Role::Inbox, Role::Drafts, Role::Sent, Role::Archive, Role::Junk, Role::Trash]
        .into_iter()
        .find(|role| role_name(*role) == name)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const ACCOUNT: &str = "mail:anna@example.org";

    fn open() -> (tempfile::TempDir, MailDirs, MailStore) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dirs = MailDirs::new(&tmp.path().join("data"), &tmp.path().join("cache"));
        let store = MailStore::open(&dirs, ACCOUNT).expect("store");
        (tmp, dirs, store)
    }

    /// A folder with its UIDVALIDITY set.
    fn folder(store: &MailStore, path: &str, role: Option<Role>) -> String {
        let id = store.upsert_folder(path, path, role).expect("folder");
        store.note_uidvalidity(&id, 7).expect("uidvalidity");
        id
    }

    fn address(name: Option<&str>, address: &str) -> MailAddress {
        MailAddress { name: name.map(str::to_string), address: address.to_string() }
    }

    fn day(n: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, n, 9, 0, 0).single().expect("date")
    }

    fn message(uid: u32, subject: &str) -> NewMessage {
        NewMessage {
            id: None,
            uid,
            rfc_message_id: Some(format!("<{uid}@example.org>")),
            thread_id: format!("t{uid}"),
            from: Some(address(Some("Bernd Beispiel"), "bernd@example.com")),
            to: vec![address(None, "anna@example.org")],
            cc: vec![],
            reply_to: vec![],
            subject: subject.to_string(),
            date: day(uid),
            snippet: "snippet".to_string(),
            flags: vec![],
            has_attachments: false,
            traits: MessageTraits::default(),
            invitation: false,
        }
    }

    fn add(store: &MailStore, folder: &str, new: &NewMessage) -> String {
        store.upsert_message(folder, 7, new).expect("message")
    }

    fn search_ids(store: &MailStore, text: &str) -> Vec<String> {
        let query = SearchQuery { text: Some(text.to_string()), ..SearchQuery::default() };
        store.search(&query, 50, 0).expect("search").into_iter().map(|summary| summary.id).collect()
    }

    #[test]
    fn the_store_is_send_and_fts5_is_there() {
        fn assert_send<T: Send>() {}
        assert_send::<MailStore>();
        let (_tmp, _dirs, store) = open();
        let count: i64 = store.db.query_row("SELECT count(*) FROM mail_fts", [], |row| row.get(0)).expect("fts5");
        assert_eq!(count, 0);
    }

    #[test]
    fn reopening_keeps_the_data_and_a_newer_file_is_refused() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let id = add(&store, &inbox, &message(1, "Hello"));
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("reopened");
        assert!(store.summary(&id).expect("summary").is_some());
        store.db.execute_batch("PRAGMA user_version = 99").expect("newer");
        drop(store);
        assert!(matches!(MailStore::open(&dirs, ACCOUNT), Err(MailStoreError::NewerSchema(99))));
    }

    #[test]
    fn a_message_goes_in_and_comes_out_with_everything() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let mut new = message(3, "Quarterly report");
        new.cc = vec![address(Some("Carla"), "carla@example.net")];
        new.reply_to = vec![address(None, "reports@example.com")];
        new.flags = vec![Flag::Seen, Flag::Answered];
        new.has_attachments = true;
        new.invitation = true;
        new.traits = MessageTraits {
            list_id: Some("<news.example.com>".into()),
            list_unsubscribe: true,
            precedence: Some("bulk".into()),
            auto_submitted: Some("auto-generated".into()),
        };
        let id = add(&store, &inbox, &new);
        assert!(id.starts_with("m:") && id.len() > 10, "{id}");

        let summary = store.summary(&id).expect("read").expect("there");
        assert_eq!((summary.account.as_str(), summary.folder.as_str()), (ACCOUNT, inbox.as_str()));
        assert_eq!(
            (summary.subject.as_str(), summary.thread_id.as_str(), summary.date),
            ("Quarterly report", "t3", day(3))
        );
        assert_eq!(summary.from, new.from);
        assert_eq!(summary.to, new.to);
        assert_eq!(summary.flags, [Flag::Seen, Flag::Answered]);
        assert!(summary.has_attachments && summary.untrusted);

        let whole = store.message(&id).expect("read").expect("there");
        assert_eq!((whole.cc, whole.reply_to), (new.cc, new.reply_to));
        assert_eq!(whole.traits, new.traits);
        assert!(whole.invitation);
        assert_eq!((whole.text, whole.from_html), (None, false), "no text stored yet");
        assert!(store.message("m:missing").expect("read").is_none());
    }

    #[test]
    fn the_same_uid_updates_the_row_and_keeps_its_id_and_text() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let id = add(&store, &inbox, &message(1, "First"));
        store.set_body(&id, &Body { text: "the text".into(), from_html: true, attachments: vec![] }).expect("body");
        let mut changed = message(1, "Second");
        changed.flags = vec![Flag::Flagged];
        assert_eq!(add(&store, &inbox, &changed), id);
        let whole = store.message(&id).expect("read").expect("there");
        assert_eq!(
            (whole.summary.subject.as_str(), whole.text.as_deref(), whole.from_html),
            ("Second", Some("the text"), true)
        );
        assert_eq!(store.folder(&inbox).expect("folder").expect("there").total, 1);
        assert!(search_ids(&store, "First").is_empty(), "the index follows the headers");
        assert_eq!(search_ids(&store, "Second"), vec![id.clone()]);

        assert!(store.set_flags(&id, &[Flag::Seen]).expect("flags"));
        assert_eq!(store.summary(&id).expect("read").expect("there").flags, [Flag::Seen]);
        assert!(store.set_flags_by_uid(&inbox, 1, &[]).expect("flags"));
        assert!(store.summary(&id).expect("read").expect("there").flags.is_empty());
        assert!(!store.set_flags("m:nope", &[]).expect("flags"));
        assert!(!store.set_flags_by_uid(&inbox, 99, &[]).expect("flags"));
    }

    #[test]
    fn full_text_search_handles_umlauts_prefixes_and_all_columns() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let mut one = message(1, "Grüße von Müller");
        one.from = Some(address(Some("Jürgen Käfer"), "jk@example.com"));
        one.to = vec![address(Some("Zoë Ångström"), "zoe@example.org")];
        let one = add(&store, &inbox, &one);
        let two = add(&store, &inbox, &message(2, "Invoice for October"));
        store
            .set_body(
                &two,
                &Body {
                    text: "Bitte überweisen Sie die Rechnung".into(),
                    from_html: false,
                    attachments: vec![AttachmentInfo {
                        part: "2".into(),
                        filename: Some("Übersicht Q3.pdf".into()),
                        content_type: "application/pdf".into(),
                        size: 10,
                        inline: false,
                    }],
                },
            )
            .expect("body");

        for term in ["Müller", "muller", "MÜLLER", "Mül", "grüße muller"] {
            assert_eq!(search_ids(&store, term), vec![one.clone()], "{term}");
        }
        assert_eq!(search_ids(&store, "kafer"), vec![one.clone()], "sender name");
        assert_eq!(search_ids(&store, "jk@example"), vec![one.clone()], "sender address");
        assert_eq!(search_ids(&store, "angstrom"), vec![one.clone()], "recipient name");
        assert_eq!(search_ids(&store, "uberweisen"), vec![two.clone()], "text");
        assert_eq!(search_ids(&store, "ubersicht"), vec![two.clone()], "attachment name");
        assert_eq!(search_ids(&store, "invoice october"), vec![two.clone()]);
        assert!(search_ids(&store, "invoice müller").is_empty(), "all words are needed");
        assert!(search_ids(&store, "nothing").is_empty());
        for odd in ["\"", "-", "AND", "a\"b", "*", "NEAR(", "  "] {
            let found = search_ids(&store, odd);
            assert!(found.len() <= 2, "{odd:?} must not fail");
        }
        assert!(search_ids(&store, "\" -").is_empty(), "no searchable word finds nothing");
    }

    #[test]
    fn filters_order_and_paging() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let mut ids = Vec::new();
        for uid in 1..=5 {
            let mut new = message(uid, &format!("Message {uid}"));
            new.flags = if uid % 2 == 0 { vec![Flag::Seen] } else { vec![Flag::Flagged] };
            ids.push(add(&store, &inbox, &new));
        }
        let other = add(&store, &archive, &message(9, "Elsewhere"));

        let list = |query: &SearchQuery, limit, offset| -> Vec<String> {
            store.search(query, limit, offset).expect("search").into_iter().map(|s| s.subject).collect()
        };
        let in_inbox = SearchQuery { folder: Some(inbox.clone()), ..SearchQuery::default() };
        assert_eq!(list(&in_inbox, 10, 0), ["Message 5", "Message 4", "Message 3", "Message 2", "Message 1"]);
        assert_eq!(list(&in_inbox, 2, 0), ["Message 5", "Message 4"]);
        assert_eq!(list(&in_inbox, 2, 2), ["Message 3", "Message 2"]);
        assert_eq!(list(&in_inbox, 2, 4), ["Message 1"]);
        assert_eq!(store.list_messages(&archive, 10, 0).expect("list")[0].id, other);
        assert_eq!(list(&SearchQuery::default(), 1, 0), ["Elsewhere"], "newest of all folders first");

        let unread = SearchQuery { unread: Some(true), folder: Some(inbox.clone()), ..SearchQuery::default() };
        assert_eq!(list(&unread, 10, 0), ["Message 5", "Message 3", "Message 1"]);
        let read = SearchQuery { unread: Some(false), folder: Some(inbox.clone()), ..SearchQuery::default() };
        assert_eq!(list(&read, 10, 0), ["Message 4", "Message 2"]);
        let flagged = SearchQuery { flagged: Some(true), ..SearchQuery::default() };
        assert_eq!(list(&flagged, 10, 0), ["Message 5", "Message 3", "Message 1"]);
        let window = SearchQuery { since: Some(day(2)), before: Some(day(4)), ..SearchQuery::default() };
        assert_eq!(list(&window, 10, 0), ["Message 3", "Message 2"], "since is inclusive, before is not");
        let all = SearchQuery {
            text: Some("message".into()),
            unread: Some(true),
            since: Some(day(2)),
            ..SearchQuery::default()
        };
        assert_eq!(list(&all, 10, 0), ["Message 5", "Message 3"]);
        let unknown = SearchQuery { folder: Some("f:nope".into()), ..SearchQuery::default() };
        assert!(list(&unknown, 10, 0).is_empty());
    }

    #[test]
    fn folders_count_unread_and_order_by_role() {
        let (_tmp, _dirs, store) = open();
        let trash = folder(&store, "Trash", Some(Role::Trash));
        let custom = folder(&store, "Projects", None);
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let mut read = message(1, "read");
        read.flags = vec![Flag::Seen];
        add(&store, &inbox, &read);
        add(&store, &inbox, &message(2, "unread"));
        add(&store, &inbox, &message(3, "unread too"));

        let folders = store.folders().expect("folders");
        let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["INBOX", "Trash", "Projects"]);
        assert_eq!((folders[0].unread, folders[0].total, folders[0].role), (2, 3, Some(Role::Inbox)));
        assert_eq!(folders[0].account, ACCOUNT);
        assert_eq!(store.folder_by_role(Role::Trash).expect("by role").map(|f| f.id), Some(trash));
        assert!(store.folder_by_role(Role::Sent).expect("by role").is_none());

        // The same path again updates name and role, keeps the id.
        let again = store.upsert_folder("Projects", "Projekte", Some(Role::Archive)).expect("upsert");
        assert_eq!(again, custom);
        let found = store.folder(&custom).expect("folder").expect("there");
        assert_eq!((found.name.as_str(), found.role), ("Projekte", Some(Role::Archive)));

        // Folders the server dropped go with their messages.
        add(&store, &custom, &message(1, "doomed"));
        assert_eq!(store.retain_folders(&["INBOX".into(), "Trash".into()]).expect("retain"), 1);
        assert!(search_ids(&store, "doomed").is_empty(), "their index entries go too");
        assert!(store.folder(&custom).expect("folder").is_none());
        assert_eq!(store.retain_folders(&["INBOX".into(), "Trash".into()]).expect("retain"), 0);
    }

    #[test]
    fn a_changed_uidvalidity_drops_the_folders_messages_and_keeps_its_id() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", None);
        let gone = add(&store, &inbox, &message(1, "Grüße"));
        store
            .set_body(&gone, &Body { text: "secret words".into(), from_html: false, attachments: vec![] })
            .expect("body");
        let kept = add(&store, &archive, &message(1, "Grüße"));
        store.set_highest_modseq(&inbox, Some(42)).expect("modseq");
        assert_eq!(store.resolve_folder(&inbox).expect("folder").expect("there").highest_modseq, Some(42));

        assert_eq!(store.note_uidvalidity(&inbox, 7).expect("same"), UidValidity::Unchanged);
        assert_eq!(store.note_uidvalidity(&inbox, 8).expect("new"), UidValidity::Reset);

        assert!(store.summary(&gone).expect("read").is_none(), "the message is gone");
        assert!(store.resolve_message(&gone).expect("resolve").is_none());
        assert!(store.message(&gone).expect("read").is_none());
        assert_eq!(search_ids(&store, "secret"), Vec::<String>::new(), "the index is cleaned too");
        assert_eq!(search_ids(&store, "grüße"), [kept], "other folders are untouched");
        let folder = store.resolve_folder(&inbox).expect("folder").expect("same id");
        assert_eq!((folder.uidvalidity, folder.highest_modseq), (Some(8), None));
        let count: i64 = store.db.query_row("SELECT count(*) FROM mail_fts", [], |row| row.get(0)).expect("count");
        assert_eq!(count, 1);
        let bodies: i64 = store.db.query_row("SELECT count(*) FROM bodies", [], |row| row.get(0)).expect("count");
        assert_eq!(bodies, 0);

        // Work done under the old value is refused.
        assert!(matches!(store.upsert_message(&inbox, 7, &message(1, "late")), Err(MailStoreError::StaleUidValidity)));
        assert!(store.upsert_message(&inbox, 8, &message(1, "fresh")).is_ok());
        assert!(matches!(store.upsert_message("f:nope", 8, &message(1, "x")), Err(MailStoreError::UnknownFolder)));
        assert!(matches!(store.note_uidvalidity("f:nope", 1), Err(MailStoreError::UnknownFolder)));
    }

    #[test]
    fn a_folder_without_uidvalidity_takes_no_messages() {
        let (_tmp, _dirs, store) = open();
        let inbox = store.upsert_folder("INBOX", "INBOX", None).expect("folder");
        assert!(matches!(store.upsert_message(&inbox, 7, &message(1, "x")), Err(MailStoreError::StaleUidValidity)));
        assert_eq!(store.note_uidvalidity(&inbox, 7).expect("first"), UidValidity::First);
        assert!(store.upsert_message(&inbox, 7, &message(1, "x")).is_ok());
    }

    #[test]
    fn ids_resolve_to_account_path_uidvalidity_and_uid_and_survive_a_move() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX.Eingang", Some(Role::Inbox));
        let archive = folder(&store, "INBOX/Archive/2026", Some(Role::Archive));
        store.note_uidvalidity(&archive, 7).expect("same");
        let id = add(&store, &inbox, &message(30, "Move me"));

        let at = store.resolve_message(&id).expect("resolve").expect("there");
        assert_eq!(
            at,
            MessageRef {
                id: id.clone(),
                account: ACCOUNT.into(),
                folder: inbox.clone(),
                folder_path: "INBOX.Eingang".into(),
                uidvalidity: 7,
                uid: 30,
                rfc_message_id: Some("<30@example.org>".into()),
            }
        );
        let folder = store.resolve_folder(&inbox).expect("folder").expect("there");
        assert_eq!(
            (folder.path.as_str(), folder.account.as_str(), folder.uidvalidity),
            ("INBOX.Eingang", ACCOUNT, Some(7))
        );
        assert_eq!(store.folder_by_path("INBOX.Eingang").expect("by path").map(|f| f.id), Some(inbox.clone()));
        assert!(store.folder_by_path("nope").expect("by path").is_none());
        assert_eq!(store.message_id_for_uid(&inbox, 30).expect("by uid"), Some(id.clone()));
        assert!(store.resolve_folder("f:nope").expect("folder").is_none());
        assert!(store.resolve_folder(&id).expect("a message id is no folder id").is_none());
        assert!(store.resolve_message(&inbox).expect("a folder id is no message id").is_none());

        assert!(store.relocate_message(&id, &archive, 7, 3).expect("move"));
        let moved = store.resolve_message(&id).expect("resolve").expect("same id");
        assert_eq!(
            (moved.folder.as_str(), moved.folder_path.as_str(), moved.uid),
            (archive.as_str(), "INBOX/Archive/2026", 3)
        );
        assert!(store.message_id_for_uid(&inbox, 30).expect("by uid").is_none());
        assert_eq!(search_ids(&store, "move"), vec![id.clone()]);
        assert!(matches!(store.relocate_message(&id, &archive, 99, 4), Err(MailStoreError::StaleUidValidity)));
        assert!(!store.relocate_message("m:nope", &archive, 7, 4).expect("move"));
    }

    #[test]
    fn deleting_removes_rows_texts_and_index_entries() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let one = add(&store, &inbox, &message(1, "alpha"));
        let two = add(&store, &inbox, &message(2, "beta"));
        let three = add(&store, &inbox, &message(3, "gamma"));
        store.set_body(&one, &Body { text: "alpha text".into(), from_html: false, attachments: vec![] }).expect("body");
        let uids = store.folder_uids(&inbox).expect("uids");
        assert_eq!(uids.iter().map(|(uid, _)| *uid).collect::<Vec<_>>(), [1, 2, 3]);

        assert!(store.delete_message(&one).expect("delete"));
        assert!(!store.delete_message(&one).expect("delete again"));
        assert_eq!(store.delete_uids(&inbox, &[2, 99]).expect("delete uids"), 1);
        assert!(search_ids(&store, "alpha").is_empty() && search_ids(&store, "beta").is_empty());
        assert_eq!(search_ids(&store, "gamma"), [three]);
        let _ = two;
        let leftovers: i64 = store.db.query_row("SELECT count(*) FROM bodies", [], |row| row.get(0)).expect("count");
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn old_texts_are_pruned_but_headers_stay() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let now = Utc.with_ymd_and_hms(2026, 10, 12, 12, 0, 0).single().expect("now");
        let mut old = message(1, "Old mail");
        old.date = now - Duration::days(91);
        let mut recent = message(2, "Recent mail");
        recent.date = now - Duration::days(89);
        let (old, recent) = (add(&store, &inbox, &old), add(&store, &inbox, &recent));
        for id in [&old, &recent] {
            store.set_body(id, &Body { text: "zebra".into(), from_html: false, attachments: vec![] }).expect("body");
        }
        assert_eq!(search_ids(&store, "zebra").len(), 2);

        assert_eq!(store.prune_bodies(now).expect("prune"), 1);
        assert_eq!(store.message(&old).expect("read").expect("header stays").text, None);
        assert_eq!(store.message(&recent).expect("read").expect("there").text.as_deref(), Some("zebra"));
        assert_eq!(search_ids(&store, "zebra"), vec![recent.clone()], "the index lets go of the text");
        assert_eq!(search_ids(&store, "old mail"), vec![old.clone()], "but still knows the subject");
        assert_eq!(store.prune_bodies(now).expect("prune again"), 0);
        // Fetched again on demand, the text is back.
        store.set_body(&old, &Body { text: "zebra".into(), from_html: false, attachments: vec![] }).expect("body");
        assert_eq!(search_ids(&store, "zebra").len(), 2);
    }

    #[test]
    fn attachments_of_a_text_are_listed_with_the_message() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let id = add(&store, &inbox, &message(1, "With file"));
        let attachments = vec![AttachmentInfo {
            part: "2".into(),
            filename: Some("a.pdf".into()),
            content_type: "application/pdf".into(),
            size: 5,
            inline: false,
        }];
        assert!(
            store
                .set_body(&id, &Body { text: "t".into(), from_html: false, attachments: attachments.clone() })
                .expect("body")
        );
        assert_eq!(store.message(&id).expect("read").expect("there").attachments, attachments);
        assert!(
            !store.set_body("m:nope", &Body { text: "t".into(), from_html: false, attachments: vec![] }).expect("body")
        );
    }

    #[test]
    fn threads_group_across_folders_oldest_first() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let sent = folder(&store, "Sent", Some(Role::Sent));
        let mut first = message(1, "Plan");
        first.thread_id = "plan".into();
        let mut reply = message(5, "Re: Plan");
        reply.thread_id = "plan".into();
        let first = add(&store, &inbox, &first);
        let reply = add(&store, &sent, &reply);
        add(&store, &inbox, &message(3, "Unrelated"));
        let thread = store.thread("plan").expect("thread");
        assert_eq!(thread.iter().map(|m| m.id.clone()).collect::<Vec<_>>(), [first, reply]);
        assert!(store.thread("none").expect("thread").is_empty());
    }

    #[test]
    fn a_bounded_thread_keeps_its_newest_messages_oldest_first() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let ids: Vec<String> = [1, 5, 9]
            .into_iter()
            .map(|day| {
                let mut new = message(day, &format!("Day {day}"));
                new.thread_id = "long".into();
                add(&store, &inbox, &new)
            })
            .collect();
        let newest = store.thread_newest("long", 2).expect("thread");
        assert_eq!(newest.iter().map(|m| m.id.clone()).collect::<Vec<_>>(), [ids[1].clone(), ids[2].clone()]);
        assert_eq!(store.thread_newest("long", 100).expect("thread").len(), 3);
    }

    #[test]
    fn drafts_remember_their_author_and_unknown_ones_belong_to_the_person() {
        let (_tmp, _dirs, store) = open();
        let drafts = folder(&store, "Drafts", Some(Role::Drafts));
        let mine = store.new_draft_id(&DraftAuthor::App).expect("draft");
        let theirs = store.new_draft_id(&DraftAuthor::Client("client-1".into())).expect("draft");
        assert_ne!(mine, theirs);
        assert!(mine.starts_with("m:"));
        assert_eq!(store.draft_author(&mine).expect("author"), Some(DraftAuthor::App));
        assert_eq!(store.draft_author(&theirs).expect("author"), Some(DraftAuthor::Client("client-1".into())));
        assert_eq!(store.draft_author("m:written-elsewhere").expect("author"), None);

        // The draft is appended on the server and shows up as a message under the reserved id.
        let mut draft = message(4, "Draft");
        draft.id = Some(theirs.clone());
        assert_eq!(add(&store, &drafts, &draft), theirs);
        assert_eq!(store.resolve_message(&theirs).expect("resolve").map(|m| m.uid), Some(4));
        // Replacing it moves it to the new UID; the id and the author stay.
        assert!(store.relocate_message(&theirs, &drafts, 7, 9).expect("replace"));
        assert_eq!(store.resolve_message(&theirs).expect("resolve").map(|m| m.uid), Some(9));
        assert_eq!(store.draft_author(&theirs).expect("author"), Some(DraftAuthor::Client("client-1".into())));
        // Deleting the message does not lose the author by itself (a sync may see it vanish first).
        store.delete_message(&theirs).expect("delete");
        assert!(store.draft_author(&theirs).expect("author").is_some());
        assert!(store.forget_draft(&theirs).expect("forget"));
        assert!(!store.forget_draft(&theirs).expect("forget again"));
        assert_eq!(store.draft_author(&theirs).expect("author"), None);
    }

    #[test]
    fn accounts_have_separate_files_and_removing_one_deletes_its_files() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        add(&store, &inbox, &message(1, "mine"));
        let other = MailStore::open(&dirs, "mail:bob@example.org").expect("second store");
        assert!(other.folders().expect("folders").is_empty(), "no mixing between accounts");
        let files = || -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&dirs.data)
                .expect("dir")
                .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert!(files().iter().all(|name| !name.contains("anna") && !name.contains("example")), "{:?}", files());
        assert_eq!(files().iter().filter(|name| name.ends_with(".sqlite")).count(), 2);

        drop(store);
        MailStore::remove(&dirs, ACCOUNT).expect("remove");
        let left = files();
        assert_eq!(left.len(), 3, "only the other account's db, -wal and -shm are left: {left:?}");
        assert!(other.folders().is_ok());
        MailStore::remove(&dirs, ACCOUNT).expect("removing twice is fine");
        drop(other);
        MailStore::remove(&dirs, "mail:bob@example.org").expect("remove");
        assert!(files().is_empty(), "{:?}", files());
        MailStore::remove(&dirs, "mail:never@example.org").expect("nothing there is nothing to do");
        // A new store for the removed account starts empty.
        let fresh = MailStore::open(&dirs, ACCOUNT).expect("again");
        assert!(fresh.folders().expect("folders").is_empty());
    }

    #[test]
    fn stores_of_unknown_accounts_are_cleaned_up_and_nothing_else() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dirs = MailDirs::new(tmp.path(), tmp.path());
        drop(MailStore::open(&dirs, ACCOUNT).expect("anna"));
        drop(MailStore::open(&dirs, "mail:bob@example.org").expect("bob"));
        let gone = file_for(&dirs, "mail:bob@example.org");
        std::fs::write(side_file(&gone, "-wal"), b"x").expect("a wal left behind");
        std::fs::write(dirs.data.join("notes.txt"), b"x").expect("another file");
        std::fs::create_dir_all(dirs.data.join("send-journal")).expect("a folder");

        assert_eq!(
            MailStore::remove_unknown(&dirs, &[ACCOUNT.to_string()], std::time::Duration::ZERO).expect("cleaned"),
            1
        );
        assert!(file_for(&dirs, ACCOUNT).exists(), "a known account keeps its store");
        assert!(!gone.exists() && !side_file(&gone, "-wal").exists(), "the unknown one goes with its wal");
        assert!(dirs.data.join("notes.txt").exists() && dirs.data.join("send-journal").exists());
        assert_eq!(
            MailStore::remove_unknown(&dirs, &[ACCOUNT.to_string()], std::time::Duration::ZERO).expect("again"),
            0
        );
        let none = MailDirs::new(&tmp.path().join("missing"), tmp.path());
        assert_eq!(
            MailStore::remove_unknown(&none, &[], std::time::Duration::ZERO).expect("no folder, nothing to do"),
            0
        );
        // A store changed just now stays: its account may be connecting at this moment.
        drop(MailStore::open(&dirs, "mail:carla@example.org").expect("carla"));
        let fresh = file_for(&dirs, "mail:carla@example.org");
        assert_eq!(MailStore::remove_unknown(&dirs, &[], std::time::Duration::from_secs(3600)).expect("young"), 0);
        assert!(fresh.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_is_private_and_so_are_all_files_including_wal_and_shm() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("temp dir");
        let dirs = MailDirs::new(&tmp.path().join("data"), &tmp.path().join("cache"));
        // A folder and a file made too open by an older version are tightened.
        std::fs::create_dir_all(&dirs.data).expect("dir");
        std::fs::set_permissions(&dirs.data, std::fs::Permissions::from_mode(0o755)).expect("open dir");
        let loose = file_for(&dirs, ACCOUNT);
        std::fs::write(&loose, b"").expect("file");
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).expect("open file");

        let store = MailStore::open(&dirs, ACCOUNT).expect("store");
        let inbox = folder(&store, "INBOX", None);
        add(&store, &inbox, &message(1, "write something so the log files exist"));

        let mode = |path: &Path| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&dirs.data), 0o700);
        let mut seen = Vec::new();
        for entry in std::fs::read_dir(&dirs.data).expect("dir") {
            let path = entry.expect("entry").path();
            assert_eq!(mode(&path), 0o600, "{path:?}");
            seen.push(path.to_string_lossy().into_owned());
        }
        assert!(seen.iter().any(|name| name.ends_with(".sqlite-wal")), "WAL is on: {seen:?}");
        assert!(seen.iter().any(|name| name.ends_with(".sqlite-shm")), "{seen:?}");
    }
}
