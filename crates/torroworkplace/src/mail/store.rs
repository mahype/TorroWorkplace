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
//! **What is kept** follows the account's cache policy: headers of known messages, selected
//! readable texts, and search words (issue #136). In `AllOffline`, full RFC-822 originals also
//! enter the independent `originals` table; deleting text cache or stale headers does not delete
//! them. An original whose message the sync saw leave its place (moved, deleted, folder renumbered
//! or removed) is marked gone and deleted [`GONE_ORIGINAL_RETENTION_DAYS`] later
//! ([`MailStore::prune_gone_originals`]). Attachments extracted for previews stay outside the
//! database; see `AttachmentCache`.
//!
//! **Invitations** (task E8.3b): what an invitation invites to is read from its `text/calendar` part
//! wherever a whole message is read (the sync's texts, `mail.get_message`), kept with the text
//! ([`Body::invite`]) in the message's row, and outlives the text; the answer given from the suite
//! is kept beside it ([`MailStore::set_invite_answer`]), so lists show both.
//!
//! **Full text** (issue #136). Two FTS5 indexes hold subject, sender, recipients, text and
//! attachment names of every message, all folded (`super::fold`: case, accents, `ß` = `ss`,
//! `ä` = `ae` = `a`): `mail_fts` (`unicode61`) for words and phrases and their BM25 rank, and
//! `mail_tri` (`trigram`) for words inside longer words (`rechnung` in `Stromrechnung`), its text
//! cut at [`TRIGRAM_TEXT_MAX`]. The headers go in when a message is stored, its text when it is
//! read ([`MailStore::set_body`], or [`MailStore::index_text`] for an older message whose text is
//! not kept to read); `messages.indexed` says which messages still lack theirs
//! ([`MailStore::unindexed`]), so the sync can fetch them in the background. Pruning a text to read
//! keeps its words in the index. A search ([`SearchQuery::query`], the language of `super::query`)
//! becomes SQL here ([`compile`]).
//!
//! **Server figures** (DS.1, #314): the sync keeps each message's `RFC822.SIZE`
//! ([`MailStore::set_size`], `NULL` for rows from before version 12 until a later sync fills them
//! in, [`MailStore::uids_without_size`]) and the account's storage quota from `GETQUOTAROOT`
//! ([`MailStore::set_quota`]). Both are what the server said at the last sync;
//! [`MailStore::account_stats`] hands them out with the messages per month and the kept originals.
//!
//! Every message is data from outside: the summaries and messages this store builds are always
//! `untrusted`, see `torro_mail::MessageSummary`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use sha2::{Digest, Sha256};
use torro_ipc::inbox::{CategoryJobPreview, CategoryJobScope};
use torro_ipc::mail::{MailAccountStats, MailFolderStats, MailMonthCount, MailQuota};
use torro_ipc::mail::{MailCacheEstimate, MailCacheJob, MailCacheMode, MailCachePolicy};
use torro_mail::{
    AttachmentInfo, Category, CategorySource, Flag, Folder, InvitationAnswer, Invite, MailAddress, MailCategory,
    Message, MessageSummary, MessageTraits, Order, Role,
};
use torro_mail_imap::mime::ReadMessage;

use super::fold::{Needle, fold, head};
use super::query::{Field, Query, Test};
use super::{MailDirs, MailStoreError, create_private, make_private, random_hex};

/// Message texts older than this many days are dropped by [`MailStore::prune_bodies`]; headers stay.
pub const BODY_RETENTION_DAYS: i64 = 90;

/// The newest layout this version writes (`PRAGMA user_version`).
const SCHEMA_VERSION: i64 = 12;

/// Originals of mail that is no longer on the server (`originals.gone_at`) are deleted this many
/// days after the sync noticed by [`MailStore::prune_gone_originals`]: long enough to still back up
/// a folder emptied by mistake, short enough that moved and deleted mail neither piles up nor comes
/// back as a duplicate when a local backup is restored.
pub const GONE_ORIGINAL_RETENTION_DAYS: i64 = 30;

/// The largest RFC-822 original the depot takes. SQLite's default `SQLITE_MAX_LENGTH` (10^9 bytes)
/// bounds a BLOB and the row around it; one MiB stays for the other columns. A larger mail stays
/// missing locally ([`MailStoreError::OriginalTooLarge`]); the server backup still reads it.
pub const ORIGINAL_MAX_BYTES: u64 = 1_000_000_000 - (1 << 20);

const SCHEMA_V9: &str = "CREATE TABLE IF NOT EXISTS cache_control (key TEXT PRIMARY KEY, value TEXT NOT NULL);";

/// Independent of `messages` and `bodies`: cache pruning, UID resets and header sync may not
/// delete the only locally held RFC-822 original. Removing the account removes this database.
const SCHEMA_V10: &str = "CREATE TABLE IF NOT EXISTS originals (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_path TEXT NOT NULL,
    uidvalidity INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    internal_date TEXT NOT NULL,
    flags_json TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    raw BLOB NOT NULL,
    UNIQUE(folder_path, uidvalidity, uid)
);";

/// Version 11 (#267): when the sync saw an original's message go from the server (`gone_at`, Unix
/// seconds; `NULL` while it is there), the hierarchy delimiter of each folder for a local backup's
/// folder table, and each message's `INTERNALDATE` (Unix seconds, `NULL` for rows from before) so a
/// local backup's date limit counts missing originals by the same date as retained ones. Each
/// column is only added where it is missing ([`migrate`]).
const SCHEMA_V11_GONE: &str = "ALTER TABLE originals ADD COLUMN gone_at INTEGER;";
const SCHEMA_V11_DELIMITER: &str = "ALTER TABLE folders ADD COLUMN delimiter TEXT;";
const SCHEMA_V11_INTERNAL_DATE: &str = "ALTER TABLE messages ADD COLUMN internal_date INTEGER;";
const SCHEMA_V11_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS originals_gone ON originals (gone_at) WHERE gone_at IS NOT NULL;";

/// Version 12 (DS.1, #314): each message's size on the server (`RFC822.SIZE` in bytes), for the
/// folder sizes of `mail.account_stats`. `NULL` for rows from before: the sync fills them in a
/// bounded number at a time ([`MailStore::uids_without_size`]). Only added where it is missing.
const SCHEMA_V12_SIZE: &str = "ALTER TABLE messages ADD COLUMN size INTEGER;";

/// Before version 11 nothing marked an original whose message went: every original without a live
/// header row at its place (folder path, the folder's current UIDVALIDITY, UID) is such one. An
/// original is only ever taken for a stored header, and the row only goes when the sync, a move or
/// a deletion saw the message leave that place.
const SCHEMA_V11_MARK_GONE: &str = "UPDATE originals SET gone_at = ?1 WHERE gone_at IS NULL AND NOT EXISTS (
    SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
    WHERE f.path = originals.folder_path AND f.uidvalidity = originals.uidvalidity AND m.uid = originals.uid)";

/// At most this many bytes of a message's text go into the word index: a log of megabytes is
/// found by its first half megabyte, and the index stays small.
pub const INDEX_TEXT_MAX: usize = 512 * 1024;

/// At most this many bytes of a message's text go into the trigram index, which is about three
/// times the size of its text: words inside longer words are found in the first 16 KiB of a
/// text (and in all of subject, sender, recipients and attachment names).
pub const TRIGRAM_TEXT_MAX: usize = 16 * 1024;

/// How much a field counts for the rank of a match (BM25 weights of subject, sender, recipients,
/// text, attachment names): a word in the subject says more than one deep in the text.
const RANK_WEIGHTS: &str = "8.0, 4.0, 2.0, 1.0, 2.0";

/// How many of the best matches of each index a ranking by relevance weighs by their words; the
/// others rank by recency alone. Keeps a search for a word in nearly every mail quick.
const RANK_CANDIDATES: u32 = 1000;

/// How much a match inside a word (trigram index) counts against a whole word's.
const INSIDE_WEIGHT: f64 = 0.5;

/// How recent mail is raised in a ranking by relevance: by up to half again, half of that at this
/// age in seconds (30 days), little after a year.
const RECENT_SECONDS: f64 = 30.0 * 24.0 * 3600.0;

/// Version 2 (task E8.7a): messages found by their `Message-ID`, which a task's link names
/// ([`MailStore::messages_by_rfc_id`]), without reading the whole table.
const SCHEMA_V2: &str = "CREATE INDEX IF NOT EXISTS messages_by_rfc_id ON messages (rfc_message_id);";

/// Version 3 (task E8.3b): what an invitation invites to (`torro_mail::Invite` as JSON, without the
/// answer) and the answer given from the suite (`accept`, `tentative`, `decline`).
const SCHEMA_V3: &str = "ALTER TABLE messages ADD COLUMN invite_json TEXT;
ALTER TABLE messages ADD COLUMN invite_answer TEXT;";

/// Version 4 (task E5.6): the header `X-GitHub-Reason` (`torro_mail::MessageTraits::github`), which
/// with `From`, `List-Id` and `Message-ID` tells a GitHub mail. `In-Reply-To` is not kept: the
/// `Message-ID` of every GitHub mail names its issue or pull request already. Rows from before
/// read as none.
const SCHEMA_V4: &str = "ALTER TABLE messages ADD COLUMN github_reason TEXT;";

/// Version 5 (issue #136): both indexes of folded text, the trigram index beside the word index,
/// and which messages have their text in them (`indexed`: 0 not yet, 1 indexed, 2 none to read,
/// so it is not fetched again). Rebuilt by [`migrate_v5`] from what the store holds.
const SCHEMA_V5: &str = "
DROP TRIGGER IF EXISTS messages_gone;
DROP TABLE IF EXISTS mail_fts;
DROP TABLE IF EXISTS mail_tri;
CREATE VIRTUAL TABLE mail_fts USING fts5(
    subject, sender, recipients, body, filenames,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE mail_tri USING fts5(
    subject, sender, recipients, body, filenames,
    tokenize = 'trigram'
);
CREATE TRIGGER messages_gone AFTER DELETE ON messages BEGIN
    DELETE FROM mail_fts WHERE rowid = old.seq;
    DELETE FROM mail_tri WHERE rowid = old.seq;
END;
CREATE INDEX IF NOT EXISTS messages_unindexed ON messages (indexed, date);";

/// Version 6 (task M8.1): the cleaned HTML part beside the text ([`Body::html`]). `NULL` in rows
/// from before: not read yet, so `mail.get_message` with `include_html` fetches the message once
/// more; an empty text: the message has no HTML part to show (or it was too large).
const SCHEMA_V6: &str = "ALTER TABLE bodies ADD COLUMN html TEXT;";

/// Version 7 (task E12.1/E12.2, `docs/kategorien.md`): what a mail is, where that came from and
/// how sure the model was (percent), plus the two answers the same model call brings along.
/// `needs_reply` and `urgency` are written but read by nothing yet; they are kept because asking
/// for them again later would mean running the model over the whole backlog a second time.
/// `NULL` everywhere in rows from before: nothing is said about that mail yet.
const SCHEMA_V7: &str = "ALTER TABLE messages ADD COLUMN category TEXT;
ALTER TABLE messages ADD COLUMN category_source TEXT;
ALTER TABLE messages ADD COLUMN category_confidence INTEGER;
ALTER TABLE messages ADD COLUMN needs_reply INTEGER;
ALTER TABLE messages ADD COLUMN urgency INTEGER;
CREATE INDEX IF NOT EXISTS messages_by_category ON messages (category, date DESC);";

/// Whether the full text may not be searchable: a limit clipped it, or an older indexed row has
/// already lost its source text and cannot be verified after this migration.
const SCHEMA_V8: &str = "ALTER TABLE messages ADD COLUMN index_partial_or_unverified INTEGER NOT NULL DEFAULT 0;
UPDATE messages SET index_partial_or_unverified = 1 WHERE indexed = 1;";

/// `indexed` of a message whose text is not in the index yet.
const NOT_INDEXED: i64 = 0;
/// `indexed` of a message whose text is in the index.
const INDEXED: i64 = 1;
/// `indexed` of a message with no text to index: gone from the server or unreadable.
const NO_TEXT: i64 = 2;

const FLAG_SEEN: i64 = 1;
const FLAG_FLAGGED: i64 = 2;
const FLAG_ANSWERED: i64 = 4;

/// Bound parameters for a selected job scope. Every folder ID is a value, never SQL syntax.
fn category_job_filter(scope: &CategoryJobScope) -> (String, Vec<Value>) {
    let mut conditions = vec!["1 = 1".to_string()];
    let mut values = Vec::new();
    if !scope.folders.is_empty() {
        conditions.push(format!("folder_id IN ({})", vec!["?"; scope.folders.len()].join(", ")));
        values.extend(scope.folders.iter().cloned().map(Value::Text));
    }
    if let Some(since) = scope.since {
        conditions.push("date >= ?".to_string());
        values.push(Value::Integer(since.timestamp()));
    }
    if let Some(before) = scope.before {
        conditions.push("date < ?".to_string());
        values.push(Value::Integer(before.timestamp()));
    }
    (conditions.join(" AND "), values)
}

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

/// A fully held local RFC-822 original ([`MailStore::local_originals_page`]); its bytes are read
/// apart ([`MailStore::original_chunk`]) to keep page reads small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOriginal {
    /// The depot's own sequence: rising in the order originals were first taken and kept when one
    /// is taken again, so `max(seq)` at a moment bounds what existed then
    /// ([`MailStore::original_snapshot`]).
    pub seq: i64,
    /// The folder as the server names it (modified UTF-7), where the message was when it was taken.
    pub folder_path: String,
    /// The folder's UIDVALIDITY the UID was valid under.
    pub uid_validity: u32,
    /// The UID in that folder.
    pub uid: u32,
    /// IMAP `INTERNALDATE`, with the server's offset: what a backup's date limit compares.
    pub internal_date: DateTime<FixedOffset>,
    /// Every flag and keyword as the server named them, refreshed by the sync
    /// ([`MailStore::set_original_flags`]).
    pub flags: Vec<String>,
    /// Length of the raw bytes.
    pub bytes: u64,
    /// SHA-256 of the raw bytes, lower-case hex.
    pub sha256: String,
    /// The sync saw the message leave this place (moved, deleted, folder renumbered or removed): a
    /// local backup skips it, and [`MailStore::prune_gone_originals`] deletes it after
    /// [`GONE_ORIGINAL_RETENTION_DAYS`].
    pub gone: bool,
}

/// A folder as a local backup describes it (MB.4): as the last sync of the folder list saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    /// The path as the server names it.
    pub path: String,
    /// The name to show, decoded from modified UTF-7.
    pub name: String,
    /// The hierarchy delimiter; `None` for a flat server, or a folder not listed since version 11.
    pub delimiter: Option<char>,
    /// The role, as the store names it (`inbox`, `sent`, …).
    pub role: Option<String>,
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
    /// The cleaned HTML part (`mime::read`, task M8.1); `None` when the message has none to show.
    /// Kept and pruned with the text; [`MailStore::html`] hands it out.
    pub html: Option<String>,
    /// All attachments, inline parts included; their names go into the search index.
    pub attachments: Vec<AttachmentInfo>,
    /// What the message invites to (`mail::read::invite_of`); `None` for a message without an
    /// invitation that can be answered. Kept in the message's row, so it outlives the text; an
    /// answer given before stays ([`MailStore::set_invite_answer`]).
    pub invite: Option<Invite>,
}

impl Body {
    /// The body of a message read whole: its text, cleaned HTML and attachments as `read` has them,
    /// and what it invites to (`invite`, read from its calendar part by the caller).
    pub fn of(read: ReadMessage, invite: Option<Invite>) -> Body {
        Body { text: read.text, from_html: read.from_html, html: read.html, attachments: read.attachments, invite }
    }
}

/// A search over the cache; every field but `order` and `relevance` narrows the result
/// (`mail.search_messages`). All `None` lists everything, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Only this folder (an id from [`MailStore::folders`]).
    pub folder: Option<String>,
    /// What the person typed, read by `super::query::parse` (issue #136): words, phrases, `OR`,
    /// `-`, operators. Empty: no condition. A word or phrase without letters or digits matches
    /// nothing.
    pub query: Query,
    /// `Some(true)`: not yet read; `Some(false)`: read.
    pub unread: Option<bool>,
    /// `Some(true)`: flagged; `Some(false)`: not flagged.
    pub flagged: Option<bool>,
    /// Sent at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Sent before this time.
    pub before: Option<DateTime<Utc>>,
    /// Newest or oldest first by date (task E8.15); the same date by the order the store took the
    /// messages in, turned along.
    pub order: Order,
    /// `Some(now)`: best match first instead (issue #136): the BM25 rank of the query's words over
    /// the folded fields (weighted by [`RANK_WEIGHTS`]), raised for mail recent at `now` (by up to
    /// half again); equal ranks newest first. `order` is ignored then. Without words in `query`
    /// every rank is the same, so it is by date.
    pub relevance: Option<DateTime<Utc>>,
    /// `Some`: only these messages (`m:…`), none when empty. For "Later" (task E9.1): the snoozed
    /// mail, or the mail back from it, which `mail.search_messages` asks for apart. Bound as one
    /// JSON list (task E18.1), so any number of ids goes.
    pub only: Option<Vec<String>>,
    /// Never these messages (`m:…`), for "Later" (task E9.1): left out before paging, so pages hold.
    /// Bound as one JSON list like `only`.
    pub except: Vec<String>,
    /// Hidden mail of the inbox folder (task E18.1, `mail.search_messages` with `hidden`): `Some`
    /// leaves it out or keeps only it ([`Hiding`]); `None` does not ask.
    pub hidden: Option<Hiding>,
    /// Only mail of one of these categories (task E12.2); empty: every category, and mail without
    /// one. A typed set, not a word in [`Query`]: a group of the sidebar is up to seven
    /// categories, and this way the condition is exact and survives paging.
    pub categories: Vec<Category>,
    /// Only mail in the inbox folder, whatever `folder` says (task E12.8): what the sidebar's
    /// categories list, and what [`MailStore::unread_by_category`] counts, so the number on an
    /// entry and the mail behind it agree. A reply of one's own in Sent and mail thrown away keep
    /// their category, and neither belongs under "Invoices". Without an inbox folder nothing
    /// matches.
    pub in_inbox: bool,
}

/// Which mail of the account's inbox folder is hidden (task E18.1), as the inbox says it for
/// `mail.search_messages` with `hidden` (`crate::inbox::MailHidden`); the store only applies it.
///
/// A message is hidden when it lies in the folder with the role inbox, is not among `never`, and
/// its sender's address is among `senders`, its domain among `domains`, or its id among `ids`.
/// Mail in any other folder is never hidden, so a message moved to the archive (it keeps its id)
/// is found there. Without an inbox folder nothing is hidden. The id lists are bound as one JSON
/// value each (`json_each`), so tens of thousands of ids go.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hiding {
    /// `true`: only the hidden mail; `false`: everything but it.
    pub hidden: bool,
    /// Hidden senders' addresses, trimmed and in small letters (`rules::sender_key`); compared with
    /// the address of the message's `From:` the same way.
    pub senders: Vec<String>,
    /// Hidden domains, in small letters without a final dot (`rules::domain_of`): exactly what
    /// follows the last `@` of the sender's address, so `shop.example` does not cover
    /// `mail.shop.example`.
    pub domains: Vec<String>,
    /// Messages (`m:…`) hidden by their id, whoever sent them.
    pub ids: Vec<String>,
    /// Messages (`m:…`) never hidden, whatever the rest says: those of open or snoozed entries.
    pub never: Vec<String>,
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

    // ----- the original depot (#267, MB.4) -----

    /// Keeps the exact bytes of the message at (`folder_path`, `uid_validity`, `uid`) apart from the
    /// disposable text and header cache, with its `INTERNALDATE` and flags. No MIME decoding or
    /// line-ending rewrite happens. Taken again, the row keeps its `seq` and counts as on the server
    /// again (`gone_at` cleared). [`MailStoreError::OriginalTooLarge`] above
    /// [`ORIGINAL_MAX_BYTES`]; for large mail use [`save_original_file`](Self::save_original_file).
    pub fn save_original(
        &self,
        folder_path: &str,
        uid_validity: u32,
        uid: u32,
        internal_date: DateTime<FixedOffset>,
        flags: &[String],
        raw: &[u8],
    ) -> Result<(), MailStoreError> {
        let bytes = original_length(raw.len() as u64)?;
        let sha256 = hex(&Sha256::digest(raw));
        self.db.execute(
            SAVE_ORIGINAL,
            params![
                folder_path,
                uid_validity,
                uid,
                internal_date.to_rfc3339(),
                serde_json::to_string(flags)?,
                bytes,
                sha256,
                raw
            ],
        )?;
        Ok(())
    }

    /// [`save_original`](Self::save_original) for a large original, without holding the whole
    /// message in memory: `raw` is a complete temporary copy, hashed and then streamed into a BLOB.
    /// A failed BLOB write rolls back the metadata and bytes together.
    pub fn save_original_file(
        &self,
        folder_path: &str,
        uid_validity: u32,
        uid: u32,
        internal_date: DateTime<FixedOffset>,
        flags: &[String],
        raw: &mut std::fs::File,
    ) -> Result<(), MailStoreError> {
        use std::io::{Read, Seek, SeekFrom, Write};

        let bytes = raw.metadata()?.len();
        let size = i32::try_from(original_length(bytes)?).map_err(|_| MailStoreError::OriginalTooLarge)?;
        raw.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let count = raw.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            hash.update(&chunk[..count]);
        }
        let sha256 = hex(&hash.finalize());
        raw.seek(SeekFrom::Start(0))?;
        let transaction = self.db.unchecked_transaction()?;
        transaction.execute(
            SAVE_ORIGINAL,
            params![
                folder_path,
                uid_validity,
                uid,
                internal_date.to_rfc3339(),
                serde_json::to_string(flags)?,
                i64::from(size),
                sha256,
                rusqlite::blob::ZeroBlob(size)
            ],
        )?;
        let rowid: i64 = transaction.query_row(
            "SELECT seq FROM originals WHERE folder_path = ?1 AND uidvalidity = ?2 AND uid = ?3",
            params![folder_path, uid_validity, uid],
            |row| row.get(0),
        )?;
        {
            let mut blob = transaction.blob_open(rusqlite::MAIN_DB, "originals", "raw", rowid, false)?;
            if std::io::copy(raw, &mut blob)? != bytes {
                return Err(MailStoreError::Corrupt("original length changed during write".into()));
            }
            blob.flush()?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// The length of the original at this place, `None` when the depot holds none: the readers ask
    /// first, so a mail opened again is neither fetched whole for its original nor written again.
    pub fn original_size(&self, folder_path: &str, uid_validity: u32, uid: u32) -> Result<Option<u64>, MailStoreError> {
        let bytes: Option<i64> = self
            .db
            .query_row(
                "SELECT bytes FROM originals WHERE folder_path = ?1 AND uidvalidity = ?2 AND uid = ?3",
                params![folder_path, uid_validity, uid],
                |row| row.get(0),
            )
            .optional()?;
        Ok(bytes.map(|bytes| bytes.max(0) as u64))
    }

    /// Refresh retained RFC-822 metadata even when the body was not fetched again.
    pub fn set_original_flags(
        &self,
        folder_path: &str,
        uid_validity: u32,
        uid: u32,
        flags: &[String],
    ) -> Result<(), MailStoreError> {
        self.db.execute(
            "UPDATE originals SET flags_json = ?4 WHERE folder_path = ?1 AND uidvalidity = ?2 AND uid = ?3",
            params![folder_path, uid_validity, uid, serde_json::to_string(flags)?],
        )?;
        Ok(())
    }

    /// The highest `seq` of the depot now (0 when it is empty). A local backup takes only originals
    /// up to it: one taken later has a higher `seq`, one taken again keeps its own.
    pub fn original_snapshot(&self) -> Result<i64, MailStoreError> {
        Ok(self.db.query_row("SELECT coalesce(max(seq), 0) FROM originals", [], |row| row.get(0))?)
    }

    /// Up to `limit` originals with `seq` above `after`, by `seq`: a bounded metadata page, gone ones
    /// included ([`LocalOriginal::gone`]).
    pub fn local_originals_page(&self, after: i64, limit: u32) -> Result<Vec<LocalOriginal>, MailStoreError> {
        let mut statement = self.db.prepare(
            "SELECT seq, folder_path, uidvalidity, uid, internal_date, flags_json, bytes, sha256, gone_at IS NOT NULL
             FROM originals WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after, limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, bool>(8)?,
            ))
        })?;
        let mut originals = Vec::new();
        for row in rows {
            let (seq, folder_path, uid_validity, uid, date, flags, bytes, sha256, gone) = row?;
            let internal_date = DateTime::parse_from_rfc3339(&date)
                .map_err(|_| MailStoreError::Corrupt("invalid original INTERNALDATE".into()))?;
            let flags = serde_json::from_str(&flags)?;
            let bytes =
                u64::try_from(bytes).map_err(|_| MailStoreError::Corrupt("invalid original byte count".into()))?;
            originals.push(LocalOriginal {
                seq,
                folder_path,
                uid_validity,
                uid,
                internal_date,
                flags,
                bytes,
                sha256,
                gone,
            });
        }
        Ok(originals)
    }

    #[cfg(test)]
    pub fn original_bytes(&self, seq: i64) -> Result<Option<Vec<u8>>, MailStoreError> {
        Ok(self.db.query_row("SELECT raw FROM originals WHERE seq = ?1", [seq], |row| row.get(0)).optional()?)
    }

    /// At most `count` (1 to 64 KiB) bytes of original `seq` from `offset` on, fewer at its end and
    /// none past it; `None` when the depot no longer holds it. Read through SQLite's incremental
    /// BLOB I/O, so only the pages asked for are loaded: an SQL `substr` would load the whole BLOB
    /// for every piece, under the store's lock.
    pub fn original_chunk(&self, seq: i64, offset: u64, count: usize) -> Result<Option<Vec<u8>>, MailStoreError> {
        if count == 0 || count > 64 * 1024 {
            return Err(MailStoreError::Corrupt("invalid original chunk size".into()));
        }
        let start = usize::try_from(offset).map_err(|_| MailStoreError::Corrupt("invalid original offset".into()))?;
        let held: bool =
            self.db.query_row("SELECT EXISTS (SELECT 1 FROM originals WHERE seq = ?1)", [seq], |row| row.get(0))?;
        if !held {
            return Ok(None);
        }
        let blob = self.db.blob_open(rusqlite::MAIN_DB, "originals", "raw", seq, true)?;
        let mut chunk = vec![0u8; count];
        let read = blob.read_at(&mut chunk, start)?;
        chunk.truncate(read);
        Ok(Some(chunk))
    }

    /// How many stored headers have no original at their place (folder path, the folder's current
    /// UIDVALIDITY, UID): what a local backup reports as missing, and what a cache job in
    /// `AllOffline` fetches ([`cached_body_missing`](Self::cached_body_missing)). Every header counts,
    /// also one whose text the server could not give (`indexed = 2`): the gap is real either way.
    /// With `since`, only messages received on or after that day: by `INTERNALDATE` where the sync
    /// stored it (UTC), else by the header's date. Retained originals compare their own
    /// `INTERNALDATE`; a missing one has no other date to go by.
    pub fn known_missing_originals(&self, since: Option<NaiveDate>) -> Result<u64, MailStoreError> {
        let cutoff = since.and_then(|day| day.and_hms_opt(0, 0, 0)).map(|at| at.and_utc().timestamp());
        let count: i64 = self.db.query_row(
            "SELECT count(*) FROM messages m JOIN folders f ON f.id = m.folder_id
             LEFT JOIN originals o ON o.folder_path = f.path AND o.uidvalidity = f.uidvalidity AND o.uid = m.uid
             WHERE o.seq IS NULL AND (?1 IS NULL OR coalesce(m.internal_date, m.date) >= ?1)",
            [cutoff],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    /// Deletes the originals the sync saw go more than [`GONE_ORIGINAL_RETENTION_DAYS`] before
    /// `now`; returns how many went. Run with the regular clean-up after a sync. Originals of mail
    /// still on the server stay, whatever the cache policy.
    pub fn prune_gone_originals(&self, now: DateTime<Utc>) -> Result<usize, MailStoreError> {
        let limit = (now - Duration::days(GONE_ORIGINAL_RETENTION_DAYS)).timestamp();
        Ok(self.db.execute("DELETE FROM originals WHERE gone_at IS NOT NULL AND gone_at < ?1", [limit])?)
    }

    /// Every folder with its path, name, delimiter and role, for a local backup's folder table.
    pub fn folder_infos(&self) -> Result<Vec<FolderInfo>, MailStoreError> {
        let mut statement = self.db.prepare("SELECT path, name, delimiter, role FROM folders ORDER BY path")?;
        let rows = statement.query_map([], |row| {
            Ok(FolderInfo {
                path: row.get(0)?,
                name: row.get(1)?,
                delimiter: row.get::<_, Option<String>>(2)?.and_then(|text| text.chars().next()),
                role: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Remembers the hierarchy delimiter the server lists for the folder at `path` (`None`: a flat
    /// server). Nothing happens for a folder the store does not know.
    pub fn set_folder_delimiter(&self, path: &str, delimiter: Option<char>) -> Result<(), MailStoreError> {
        let delimiter = delimiter.map(String::from);
        self.db.execute("UPDATE folders SET delimiter = ?2 WHERE path = ?1", params![path, delimiter])?;
        Ok(())
    }

    /// Remembers the `INTERNALDATE` the server reports for message `uid` of `folder` (what a local
    /// backup's date limit uses for a header without original). `false`: no such message.
    pub fn set_internal_date(&self, folder: &str, uid: u32, at: DateTime<Utc>) -> Result<bool, MailStoreError> {
        let sql = "UPDATE messages SET internal_date = ?3 WHERE folder_id = ?1 AND uid = ?2";
        Ok(self.db.execute(sql, params![folder, uid, at.timestamp()])? > 0)
    }

    /// Remembers the size the server reports for message `uid` of `folder` (`RFC822.SIZE`, bytes),
    /// which the folder sizes of [`account_stats`](Self::account_stats) add up. `false`: no such
    /// message.
    pub fn set_size(&self, folder: &str, uid: u32, bytes: u64) -> Result<bool, MailStoreError> {
        let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
        let sql = "UPDATE messages SET size = ?3 WHERE folder_id = ?1 AND uid = ?2";
        Ok(self.db.execute(sql, params![folder, uid, bytes])? > 0)
    }

    /// Up to `limit` UIDs of messages of `folder` whose size is not known yet (rows from before
    /// version 12), highest UID first, for the sync to ask the server for.
    pub fn uids_without_size(&self, folder: &str, limit: u32) -> Result<Vec<u32>, MailStoreError> {
        let mut statement = self
            .db
            .prepare("SELECT uid FROM messages WHERE folder_id = ?1 AND size IS NULL ORDER BY uid DESC LIMIT ?2")?;
        let rows = statement.query_map(params![folder, limit], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Forgets every message's size, as in a store from before version 12.
    #[cfg(test)]
    pub(crate) fn forget_sizes(&self) -> Result<(), MailStoreError> {
        self.db.execute("UPDATE messages SET size = NULL", [])?;
        Ok(())
    }

    /// Keeps the storage quota the sync read from the server; `None` removes a kept one (the
    /// server no longer reports one). [`account_stats`](Self::account_stats) hands it out.
    pub fn set_quota(&self, quota: Option<&MailQuota>) -> Result<(), MailStoreError> {
        match quota {
            Some(quota) => self.db.execute(
                "INSERT INTO cache_control (key, value) VALUES ('quota', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [serde_json::to_string(quota)?],
            )?,
            None => self.db.execute("DELETE FROM cache_control WHERE key = 'quota'", [])?,
        };
        Ok(())
    }

    /// The kept storage quota ([`set_quota`](Self::set_quota)); `None` without one, or when the
    /// kept value cannot be read (a later sync writes it again).
    fn quota(&self) -> Result<Option<MailQuota>, MailStoreError> {
        let json: Option<String> = self
            .db
            .query_row("SELECT value FROM cache_control WHERE key = 'quota'", [], |row| row.get(0))
            .optional()?;
        Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
    }

    /// Stored messages per month of their date (UTC), oldest first, with how many have their text
    /// kept to read.
    fn month_counts(&self) -> Result<Vec<MailMonthCount>, MailStoreError> {
        // A date outside SQLite's years 0000 to 9999 has no month (NULL): such a mail from outside
        // is left out rather than failing the whole answer.
        let mut statement = self.db.prepare(
            "SELECT CAST(strftime('%Y', m.date, 'unixepoch') AS INTEGER) AS year,
                    CAST(strftime('%m', m.date, 'unixepoch') AS INTEGER) AS month,
                    count(*), count(b.seq)
             FROM messages m LEFT JOIN bodies b ON b.seq = m.seq
             GROUP BY year, month HAVING year IS NOT NULL AND month IS NOT NULL ORDER BY year, month",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(MailMonthCount {
                year: row.get(0)?,
                month: row.get(1)?,
                messages: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                cached_bodies: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The sum of the sizes of the messages of `folder` (an id); `None` while any of them lacks
    /// its size, because a partial sum would pass for the folder's size.
    fn server_bytes(&self, folder: &str) -> Result<Option<u64>, MailStoreError> {
        let (complete, bytes): (bool, i64) = self.db.query_row(
            "SELECT count(*) = count(size), coalesce(sum(size), 0) FROM messages WHERE folder_id = ?1",
            [folder],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(complete.then(|| u64::try_from(bytes).unwrap_or(0)))
    }

    /// How many originals at `path` (every path when `None`) are kept and still on the server, and
    /// their bytes.
    fn kept_originals(&self, path: Option<&str>) -> Result<(u64, u64), MailStoreError> {
        let counts = |row: &Row<'_>| Ok((row.get(0)?, row.get(1)?));
        let sql = "SELECT count(*), coalesce(sum(bytes), 0) FROM originals WHERE gone_at IS NULL";
        // Per folder by the depot's unique index on the path, which an `OR` would leave unused.
        let (count, bytes): (i64, i64) = match path {
            Some(path) => self.db.query_row(&format!("{sql} AND folder_path = ?1"), [path], counts)?,
            None => self.db.query_row(sql, [], counts)?,
        };
        Ok((u64::try_from(count).unwrap_or(0), u64::try_from(bytes).unwrap_or(0)))
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
    /// returns how many folders went. Their originals stay, marked gone ([`LocalOriginal::gone`]).
    /// An empty list drops all, which a sync must not send by mistake: pass the whole `LIST`
    /// result.
    pub fn retain_folders(&self, paths: &[String]) -> Result<usize, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let known: Vec<(String, String)> = {
            let mut statement = tx.prepare("SELECT id, path FROM folders")?;
            statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?
        };
        let mut removed = 0;
        for (id, path) in known {
            if !paths.contains(&path) {
                // Every original of the folder, under any UIDVALIDITY: the place is gone.
                tx.execute(
                    "UPDATE originals SET gone_at = ?2 WHERE folder_path = ?1 AND gone_at IS NULL",
                    params![path, Utc::now().timestamp()],
                )?;
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

    /// Local inventory only: a successful account sync does not prove that every server folder
    /// has been fetched. Dates therefore describe the oldest and newest *known* messages.
    pub fn account_stats(&self) -> Result<MailAccountStats, MailStoreError> {
        let mut folders = Vec::new();
        for folder in self.folders()? {
            let sql = "SELECT count(*), coalesce(sum((m.flags & 1) <> 0), 0), min(m.date), max(m.date),
                coalesce(sum(EXISTS(SELECT 1 FROM bodies b WHERE b.seq = m.seq)), 0),
                coalesce(sum(m.indexed = 1 AND m.index_partial_or_unverified = 0), 0),
                coalesce(sum(m.indexed = 1 AND m.index_partial_or_unverified <> 0), 0),
                coalesce(sum(m.indexed = 2), 0), coalesce(sum(m.indexed = 0), 0)
                FROM messages m WHERE m.folder_id = ?1";
            let (_total, read, oldest, newest, bodies, indexed_full, indexed_partial, unavailable, awaiting): (
                i64,
                i64,
                Option<i64>,
                Option<i64>,
                i64,
                i64,
                i64,
                i64,
                i64,
            ) = self.db.query_row(sql, [&folder.id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            })?;
            let to_date = |seconds: Option<i64>| seconds.and_then(|value| DateTime::from_timestamp(value, 0));
            let server_bytes = self.server_bytes(&folder.id)?;
            let (originals, original_bytes) = self.kept_originals(Some(&folder.path))?;
            folders.push(MailFolderStats {
                folder,
                read: u64::try_from(read).unwrap_or(0),
                oldest_known: to_date(oldest),
                newest_known: to_date(newest),
                cached_bodies: u64::try_from(bodies).unwrap_or(0),
                indexed_full: u64::try_from(indexed_full).unwrap_or(0),
                indexed_partial_or_unverified: u64::try_from(indexed_partial).unwrap_or(0),
                text_unavailable: u64::try_from(unavailable).unwrap_or(0),
                awaiting_index: u64::try_from(awaiting).unwrap_or(0),
                server_bytes,
                originals,
                original_bytes,
            });
        }
        let messages = folders.iter().map(|stats| u64::from(stats.folder.total)).sum();
        let read = folders.iter().map(|stats| stats.read).sum();
        let oldest_known = folders.iter().filter_map(|stats| stats.oldest_known).min();
        let newest_known = folders.iter().filter_map(|stats| stats.newest_known).max();
        let wal = self.path.with_extension("sqlite-wal");
        let shm = self.path.with_extension("sqlite-shm");
        let database_bytes = [&self.path, &wal, &shm]
            .into_iter()
            .filter_map(|path| std::fs::metadata(path).ok().map(|meta| meta.len()))
            .sum();
        let (oldest_body, body_cache_bytes): (Option<i64>, i64) = self.db.query_row(
            "SELECT min(m.date), coalesce(sum(length(b.text) + coalesce(length(b.html), 0) + length(b.attachments_json) + length(b.filenames)), 0) FROM bodies b JOIN messages m ON m.seq = b.seq",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let search_index_bytes = self
            .db
            .query_row(
                "SELECT coalesce(sum(pgsize), 0) FROM dbstat WHERE name LIKE 'mail_fts%' OR name LIKE 'mail_tri%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|bytes| bytes.max(0) as u64);
        let last_index_run: Option<i64> = self
            .db
            .query_row("SELECT value FROM cache_control WHERE key = 'last_index_run'", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
            .and_then(|value| value.parse().ok());
        let (originals, original_bytes) = self.kept_originals(None)?;
        Ok(MailAccountStats {
            account: self.account.clone(),
            messages,
            read,
            unread: messages.saturating_sub(read),
            oldest_known,
            newest_known,
            cached_bodies: folders.iter().map(|stats| stats.cached_bodies).sum(),
            oldest_cached_body: oldest_body.and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
            body_cache_bytes: body_cache_bytes.max(0) as u64,
            search_index_bytes,
            last_index_run: last_index_run.and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
            indexed_full: folders.iter().map(|stats| stats.indexed_full).sum(),
            indexed_partial_or_unverified: folders.iter().map(|stats| stats.indexed_partial_or_unverified).sum(),
            text_unavailable: folders.iter().map(|stats| stats.text_unavailable).sum(),
            awaiting_index: folders.iter().map(|stats| stats.awaiting_index).sum(),
            database_bytes,
            cached_attachments: 0,
            attachment_cache_bytes: None,
            last_sync: None,
            sync_problem: None,
            quota: self.quota()?,
            months: self.month_counts()?,
            originals,
            original_bytes,
            folders,
        })
    }

    /// Id, name, path and role of every folder, without counting their messages: what `in:` of a
    /// search looks for ([`Folder::unread`] and [`Folder::total`] are 0).
    fn folder_names(&self) -> Result<Vec<Folder>, MailStoreError> {
        let mut statement = self.db.prepare("SELECT f.id, f.name, f.path, f.role, 0, 0 FROM folders f")?;
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
    /// stored drops all messages of the folder ([`UidValidity::Reset`]) and its modseq, and marks
    /// the originals taken under the old value gone; the folder and its id stay. Call this when the
    /// folder is selected, before reading or writing messages of it.
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
                // The old UIDs name nothing any more: every original taken under another
                // UIDVALIDITY of this folder is gone (the sync takes the messages anew).
                tx.execute(
                    "UPDATE originals SET gone_at = ?3 WHERE folder_path = ?1 AND uidvalidity <> ?2 AND gone_at IS NULL",
                    params![current.path, uidvalidity, Utc::now().timestamp()],
                )?;
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
        refresh_index(&tx, seq, None)?;
        // The server lists the message at this place: an original kept for it is not gone.
        if let Some((path, _)) = original_place(&tx, folder)? {
            set_original_gone(&tx, &path, uidvalidity, new.uid, None)?;
        }
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

    /// Stores (or replaces) the text of a message, its cleaned HTML, and puts the text into the
    /// search index (the message counts as indexed from now on). `false`: no such message.
    pub fn set_body(&self, id: &str, body: &Body) -> Result<bool, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let seq: Option<i64> =
            tx.query_row("SELECT seq FROM messages WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        let Some(seq) = seq else { return Ok(false) };
        let names: Vec<&str> = body.attachments.iter().filter_map(|a| a.filename.as_deref()).collect();
        // An empty text, not `NULL`, for a message without HTML: it was read and has none
        // (`SCHEMA_V6`).
        let html = body.html.as_deref().unwrap_or_default();
        tx.execute(
            "INSERT INTO bodies (seq, text, from_html, attachments_json, filenames, html)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (seq) DO UPDATE SET text = ?2, from_html = ?3, attachments_json = ?4, filenames = ?5,
                 html = ?6",
            params![seq, body.text, body.from_html, serde_json::to_string(&body.attachments)?, names.join(" "), html],
        )?;
        // The answer has its own column: reading the text again keeps it.
        let invite = body.invite.as_ref().map(|invite| Invite { answered: None, ..invite.clone() });
        let invite = invite.map(|invite| serde_json::to_string(&invite)).transpose()?;
        tx.execute(
            "UPDATE messages SET invite_json = ?2, indexed = ?3, index_partial_or_unverified = ?4 WHERE seq = ?1",
            params![seq, invite, INDEXED, index_is_partial(&body.text)],
        )?;
        refresh_index(&tx, seq, Some(IndexText { body: &body.text, filenames: &names.join(" ") }))?;
        tx.commit()?;
        Ok(true)
    }

    /// Puts the text `text` and the attachment names `filenames` of the message `id` into the
    /// search index without keeping the text to read (issue #136): the background fetch of older
    /// texts, whose text would only be pruned again ([`prune_bodies`](Self::prune_bodies)). The
    /// message counts as indexed. `false`: no such message.
    pub fn index_text(&self, id: &str, text: &str, filenames: &[&str]) -> Result<bool, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let seq: Option<i64> =
            tx.query_row("SELECT seq FROM messages WHERE id = ?1", [id], |row| row.get(0)).optional()?;
        let Some(seq) = seq else { return Ok(false) };
        tx.execute(
            "UPDATE messages SET indexed = ?2, index_partial_or_unverified = ?3 WHERE seq = ?1",
            params![seq, INDEXED, index_is_partial(text)],
        )?;
        refresh_index(&tx, seq, Some(IndexText { body: text, filenames: &filenames.join(" ") }))?;
        tx.commit()?;
        Ok(true)
    }

    /// Notes that the message `id` has no text to index (gone from the server, or unreadable), so
    /// [`unindexed`](Self::unindexed) does not hand it out again; its headers stay searchable.
    /// `false`: no such message.
    pub fn mark_unindexable(&self, id: &str) -> Result<bool, MailStoreError> {
        Ok(self.db.execute("UPDATE messages SET indexed = ?2 WHERE id = ?1", params![id, NO_TEXT])? > 0)
    }

    /// Up to `limit` messages whose text is not in the index yet, newest first, with their dates:
    /// what the background fetch takes next (issue #136). Only messages of folders a sync has
    /// opened (they have a UIDVALIDITY).
    pub fn unindexed(&self, limit: u32) -> Result<Vec<(MessageRef, DateTime<Utc>)>, MailStoreError> {
        let sql = "SELECT m.id, m.folder_id, f.path, f.uidvalidity, m.uid, m.rfc_message_id, m.date
                   FROM messages m JOIN folders f ON f.id = m.folder_id
                   WHERE m.indexed = ?1 AND f.uidvalidity IS NOT NULL ORDER BY m.date DESC, m.seq DESC LIMIT ?2";
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map(params![NOT_INDEXED, limit], |row| {
            let seconds: i64 = row.get(6)?;
            Ok((
                MessageRef {
                    id: row.get(0)?,
                    account: self.account.clone(),
                    folder: row.get(1)?,
                    folder_path: row.get(2)?,
                    uidvalidity: row.get(3)?,
                    uid: row.get(4)?,
                    rfc_message_id: row.get(5)?,
                },
                DateTime::from_timestamp(seconds, 0).ok_or_else(|| conversion_failure(6, "date out of range"))?,
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many messages the store holds, and how many of them are done for the index (their text
    /// is in it, or they have none): `mail.index_status`.
    #[cfg(test)]
    pub fn index_counts(&self) -> Result<(u64, u64), MailStoreError> {
        let sql = "SELECT count(*), coalesce(sum(indexed <> 0), 0) FROM messages";
        let (all, done): (i64, i64) = self.db.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok((u64::try_from(all).unwrap_or(0), u64::try_from(done).unwrap_or(0)))
    }

    /// Separate indexed text from text the server cannot provide (`indexed = 2`).
    pub fn index_breakdown(&self) -> Result<(u64, u64, u64), MailStoreError> {
        let sql = "SELECT count(*), coalesce(sum(indexed = 1), 0), coalesce(sum(indexed = 2), 0) FROM messages";
        let (all, indexed, unavailable): (i64, i64, i64) =
            self.db.query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok((
            u64::try_from(all).unwrap_or(0),
            u64::try_from(indexed).unwrap_or(0),
            u64::try_from(unavailable).unwrap_or(0),
        ))
    }

    /// Every message has searchable text. A message known to have no available text keeps a
    /// search answer incomplete even when the backfill has no more work to do.
    pub fn fully_indexed(&self) -> Result<bool, MailStoreError> {
        let sql = "SELECT NOT EXISTS (SELECT 1 FROM messages WHERE indexed <> ?1)";
        Ok(self.db.query_row(sql, [INDEXED], |row| row.get(0))?)
    }

    /// Keeps what the mail `id` is (task E12.2); `None` takes the category away again. `false`
    /// when the store has no such message. The confidence is only kept for a model's category.
    pub fn set_category(&self, id: &str, category: Option<MailCategory>) -> Result<bool, MailStoreError> {
        let key = category.map(|held| held.category.key());
        let source = category.map(|held| match held.source {
            CategorySource::Rule => "rule",
            CategorySource::Model => "model",
            CategorySource::Person => "person",
        });
        let confidence = category.and_then(|held| held.confidence).map(i64::from);
        let sql = "UPDATE messages SET category = ?2, category_source = ?3, category_confidence = ?4 WHERE id = ?1";
        Ok(self.db.execute(sql, params![id, key, source, confidence])? > 0)
    }

    /// Commit a job's decision only if the category still matches the value read before the
    /// model call. A person's or rule's concurrent change therefore wins, including when the
    /// model took a long time to answer.
    pub fn set_category_if_unchanged(
        &self,
        id: &str,
        expected: Option<MailCategory>,
        category: MailCategory,
    ) -> Result<bool, MailStoreError> {
        let expected_key = expected.map(|held| held.category.key());
        let expected_source = expected.map(|held| match held.source {
            CategorySource::Rule => "rule",
            CategorySource::Model => "model",
            CategorySource::Person => "person",
        });
        let source = match category.source {
            CategorySource::Rule => "rule",
            CategorySource::Model => "model",
            CategorySource::Person => "person",
        };
        let changed = self.db.execute(
            "UPDATE messages SET category = ?2, category_source = ?3, category_confidence = ?4
             WHERE id = ?1 AND category IS ?5 AND category_source IS ?6
             AND (category_source IS NULL OR category_source = 'model')",
            params![
                id,
                category.category.key(),
                source,
                category.confidence.map(i64::from),
                expected_key,
                expected_source
            ],
        )?;
        Ok(changed > 0)
    }

    /// Counts and freezes a user-selected categorization scope. Empty folders mean all folders;
    /// account filtering is performed by the service before this store is asked.
    pub fn category_job_preview(&self, scope: &CategoryJobScope) -> Result<(CategoryJobPreview, i64), MailStoreError> {
        let (where_sql, values) = category_job_filter(scope);
        let sql = format!(
            "SELECT count(*), coalesce(sum(category IS NULL), 0),
             coalesce(sum(category_source = 'model'), 0),
             coalesce(sum(category IS NOT NULL AND (category_source IS NULL OR category_source <> 'model')), 0)
             FROM messages WHERE {where_sql}"
        );
        let (total, unclassified, automatic, protected): (i64, i64, i64, i64) =
            self.db.query_row(&sql, params_from_iter(values), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
        let max_sequence: i64 =
            self.db.query_row("SELECT coalesce(max(seq), 0) FROM messages", [], |row| row.get(0))?;
        let preview = CategoryJobPreview {
            total: total.max(0) as u64,
            unclassified: unclassified.max(0) as u64,
            automatic: automatic.max(0) as u64,
            protected: protected.max(0) as u64,
            eligible: (unclassified + if scope.again { automatic } else { 0 }).max(0) as u64,
            request_scope: None,
        };
        Ok((preview, max_sequence))
    }

    /// The next small batch after `cursor`, bounded by the store's sequence when the job began.
    pub fn category_job_batch(
        &self,
        scope: &CategoryJobScope,
        cursor: i64,
        max_sequence: i64,
        limit: u32,
    ) -> Result<Vec<(i64, String)>, MailStoreError> {
        let (filter, mut values) = category_job_filter(scope);
        let predicate =
            if scope.again { "(category IS NULL OR category_source = 'model')" } else { "category IS NULL" };
        values.push(Value::Integer(cursor));
        values.push(Value::Integer(max_sequence));
        values.push(Value::Integer(i64::from(limit)));
        let sql = format!(
            "SELECT seq, id FROM messages WHERE {filter} AND {predicate} AND seq > ? AND seq <= ? ORDER BY seq LIMIT ?"
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// All assignments in this account, regardless of folder, read state or source.
    pub fn category_clear_preview(&self) -> Result<(u64, i64), MailStoreError> {
        let (count, maximum): (i64, i64) = self.db.query_row(
            "SELECT count(*), coalesce(max(seq), 0) FROM messages WHERE category IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((count.max(0) as u64, maximum))
    }

    /// Remove up to `limit` category assignments and the two answers from the same model call,
    /// in one transaction. The cursor is the last row changed; a restart can repeat this safely.
    /// The search index, bodies, folders, flags, drafts and server mail are untouched.
    pub fn clear_category_batch(&self, cursor: i64, maximum: i64, limit: u32) -> Result<(i64, u64), MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let sequences: Vec<i64> = {
            let mut statement = tx.prepare(
                "SELECT seq FROM messages WHERE category IS NOT NULL AND seq > ?1 AND seq <= ?2 ORDER BY seq LIMIT ?3",
            )?;
            let rows = statement.query_map(params![cursor, maximum, i64::from(limit)], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut changed = 0u64;
        for seq in &sequences {
            changed += tx.execute(
                "UPDATE messages SET category = NULL, category_source = NULL, category_confidence = NULL,
                 needs_reply = NULL, urgency = NULL WHERE seq = ?1 AND category IS NOT NULL",
                [seq],
            )? as u64;
        }
        tx.commit()?;
        Ok((sequences.last().copied().unwrap_or(maximum), changed))
    }

    /// Clear model answers left on messages whose category was already removed separately.
    /// This is bounded and repeatable after a restart.
    pub fn clear_model_answers_batch(&self, limit: u32) -> Result<u64, MailStoreError> {
        let changed = self.db.execute(
            "UPDATE messages SET needs_reply = NULL, urgency = NULL WHERE seq IN (
             SELECT seq FROM messages WHERE needs_reply IS NOT NULL OR urgency IS NOT NULL LIMIT ?1)",
            [i64::from(limit)],
        )?;
        Ok(changed as u64)
    }

    /// How much mail of any folder is of `category` (task E15.2): what removing the category
    /// would move.
    pub fn count_category(&self, category: Category) -> Result<u64, MailStoreError> {
        let count: i64 =
            self.db
                .query_row("SELECT count(*) FROM messages WHERE category = ?1", [category.key()], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Files every mail of `category` as `replacement` (task E15.2), keeping where its category
    /// came from and how sure a model was: the person's choice stays the person's. Returns how
    /// many changed.
    pub fn replace_category(&self, category: Category, replacement: Category) -> Result<usize, MailStoreError> {
        let sql = "UPDATE messages SET category = ?2 WHERE category = ?1";
        Ok(self.db.execute(sql, params![category.key(), replacement.key()])?)
    }

    /// Keeps the two answers the same model call brings along besides the category (task E12.6):
    /// whether a reply is waited for and how pressing the mail is (0 routine, 1 soon, 2 urgent).
    /// Nothing reads them yet; they are kept so a later step need not run the model over the
    /// whole backlog again. `false` when the store has no such message.
    pub fn set_model_answers(
        &self,
        id: &str,
        needs_reply: Option<bool>,
        urgency: Option<u8>,
    ) -> Result<bool, MailStoreError> {
        let sql = "UPDATE messages SET needs_reply = ?2, urgency = ?3 WHERE id = ?1";
        Ok(self.db.execute(sql, params![id, needs_reply, urgency.map(i64::from)])? > 0)
    }

    /// The mail of the inbox folder that has no category yet, newest first, at most `limit` and
    /// not older than `since`: what the backlog run asks for (task E12.6). Mail that already has
    /// one is never asked about again, so a second run asks nothing twice.
    pub fn uncategorised(&self, since: DateTime<Utc>, limit: u32) -> Result<Vec<String>, MailStoreError> {
        let Some(inbox) = self.folder_by_role(Role::Inbox)? else { return Ok(Vec::new()) };
        let sql = "SELECT id FROM messages WHERE folder_id = ?1 AND category IS NULL AND date >= ?2
            ORDER BY date DESC, seq DESC LIMIT ?3";
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map(params![inbox.id, since.timestamp(), limit], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The mail of the inbox folder in the window the backlog run uses, newest first, with a
    /// category or without (task E15.4): what sorting again goes through.
    pub fn in_window(&self, since: DateTime<Utc>, limit: u32) -> Result<Vec<String>, MailStoreError> {
        let Some(inbox) = self.folder_by_role(Role::Inbox)? else { return Ok(Vec::new()) };
        let sql = "SELECT id FROM messages WHERE folder_id = ?1 AND date >= ?2 ORDER BY date DESC, seq DESC LIMIT ?3";
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map(params![inbox.id, since.timestamp(), limit], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Which of `ids` already have a category (task E12.6), in no particular order: what the
    /// feed leaves alone when mail comes in again – after a gap in the change stream, or on the
    /// way back from "Later". Sorting it again would write a rule's answer over the person's.
    pub fn categorised(&self, ids: &[String]) -> Result<Vec<String>, MailStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let places = vec!["?"; ids.len()].join(", ");
        let sql = format!("SELECT id FROM messages WHERE category IS NOT NULL AND id IN ({places})");
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(ids.iter()), |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How much mail each category holds and how much of it is unread (task E12.4), only
    /// categories that occur: `(category, unread, all)`.
    ///
    /// **The inbox folder only**, as the sidebar's categories list it ([`SearchQuery::in_inbox`]):
    /// a reply of one's own in Sent, and mail thrown away or marked as spam, carry their category
    /// on, and counting them would offer a category whose view is empty. Without an inbox folder
    /// nothing is counted. Mail without a category is not counted at all: the sidebar offers only
    /// what is there.
    pub fn unread_by_category(&self) -> Result<Vec<(Category, u32, u32)>, MailStoreError> {
        let Some(inbox) = self.folder_by_role(Role::Inbox)? else { return Ok(Vec::new()) };
        let sql = "SELECT category, sum(flags & 1 = 0), count(*) FROM messages
            WHERE folder_id = ?1 AND category IS NOT NULL GROUP BY category";
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map([&inbox.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })?;
        let mut counts = Vec::new();
        let held = |count: i64| count.clamp(0, i64::from(u32::MAX)) as u32;
        for row in rows {
            let (key, unread, all) = row?;
            // A key this version does not know (a store written by a newer one) is left out.
            if let Some(category) = Category::from_key(&key) {
                counts.push((category, held(unread), held(all)));
            }
        }
        Ok(counts)
    }

    /// How much mail of the inbox folder the backlog run looks at and how much of it has a
    /// category already (task E12.4): `(messages, done)`, counted over the last `days` days and
    /// at most `limit` messages, the window the run itself uses.
    pub fn category_counts(&self, since: DateTime<Utc>, limit: u32) -> Result<(u64, u64), MailStoreError> {
        let Some(inbox) = self.folder_by_role(Role::Inbox)? else { return Ok((0, 0)) };
        let sql = "SELECT count(*), coalesce(sum(category IS NOT NULL), 0) FROM (
                SELECT category FROM messages WHERE folder_id = ?1 AND date >= ?2
                ORDER BY date DESC, seq DESC LIMIT ?3)";
        let counted = self.db.query_row(sql, params![inbox.id, since.timestamp(), limit], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        Ok((counted.0.max(0) as u64, counted.1.max(0) as u64))
    }

    /// Keeps `answer` as the answer given to the invitation of the message `id`
    /// (`mail.respond_to_invitation`, task E8.3b); `None` forgets it. `false`: no such message.
    pub fn set_invite_answer(&self, id: &str, answer: Option<InvitationAnswer>) -> Result<bool, MailStoreError> {
        let answer = answer.map(InvitationAnswer::as_str);
        Ok(self.db.execute("UPDATE messages SET invite_answer = ?2 WHERE id = ?1", params![id, answer])? > 0)
    }

    /// Drops the texts to read of messages sent more than [`BODY_RETENTION_DAYS`] before `now`;
    /// their headers stay, and so do their words in the search index (issue #136).
    /// `Message::text` is `None` again until someone fetches it. Returns how many texts went. Run
    /// it after a sync and now and then.
    pub fn prune_bodies(&self, now: DateTime<Utc>) -> Result<usize, MailStoreError> {
        self.prune_bodies_for(self.cache_policy()?.mode, now, usize::MAX)
    }

    /// Drop reading texts in bounded steps according to the account policy. The FTS index stays.
    pub fn prune_bodies_for(
        &self,
        mode: MailCacheMode,
        now: DateTime<Utc>,
        batch: usize,
    ) -> Result<usize, MailStoreError> {
        if mode == MailCacheMode::AllOffline {
            return Ok(0);
        }
        let limit = (now - Duration::days(BODY_RETENTION_DAYS)).timestamp();
        let tx = self.db.unchecked_transaction()?;
        let old: Vec<i64> = {
            let mut statement =
                tx.prepare("SELECT b.seq FROM bodies b JOIN messages m ON m.seq = b.seq JOIN folders f ON f.id = m.folder_id LEFT JOIN drafts d ON d.id = m.id WHERE (?1 = 1 OR m.date < ?2) AND f.role IS NOT 'drafts' AND d.id IS NULL LIMIT ?3")?;
            statement
                .query_map(
                    params![mode == MailCacheMode::HeadersOnly, limit, i64::try_from(batch).unwrap_or(i64::MAX)],
                    |row| row.get(0),
                )?
                .collect::<Result<_, _>>()?
        };
        for seq in &old {
            tx.execute("DELETE FROM bodies WHERE seq = ?1", [seq])?;
        }
        tx.commit()?;
        Ok(old.len())
    }

    pub fn cache_policy(&self) -> Result<MailCachePolicy, MailStoreError> {
        let raw: Option<String> = self
            .db
            .query_row("SELECT value FROM cache_control WHERE key = 'policy'", [], |row| row.get(0))
            .optional()?;
        match raw {
            Some(raw) => Ok(serde_json::from_str(&raw)?),
            None => Ok(MailCachePolicy {
                account: self.account.clone(),
                mode: MailCacheMode::Recent90Days,
                prefetch_attachments: false,
                attachment_retention_days: 1,
            }),
        }
    }

    pub fn note_index_run(&self, now: DateTime<Utc>) -> Result<(), MailStoreError> {
        self.db.execute("INSERT INTO cache_control (key, value) VALUES ('last_index_run', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [now.timestamp().to_string()])?;
        Ok(())
    }

    pub fn set_cache_policy(&self, policy: &MailCachePolicy) -> Result<(), MailStoreError> {
        self.db.execute("INSERT INTO cache_control (key, value) VALUES ('policy', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [serde_json::to_string(policy)?])?;
        Ok(())
    }

    pub fn cache_job(&self) -> Result<Option<MailCacheJob>, MailStoreError> {
        let raw: Option<String> =
            self.db.query_row("SELECT value FROM cache_control WHERE key = 'job'", [], |row| row.get(0)).optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into)).transpose()
    }

    pub fn set_cache_job(&self, job: &MailCacheJob) -> Result<(), MailStoreError> {
        self.db.execute("INSERT INTO cache_control (key, value) VALUES ('job', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [serde_json::to_string(job)?])?;
        Ok(())
    }

    /// Do not let a finishing batch overwrite a pause or a replacement job.
    pub fn change_cache_job(&self, expected: &MailCacheJob, changed: &MailCacheJob) -> Result<bool, MailStoreError> {
        Ok(self.db.execute(
            "UPDATE cache_control SET value = ?2 WHERE key = 'job' AND value = ?1",
            params![serde_json::to_string(expected)?, serde_json::to_string(changed)?],
        )? > 0)
    }

    /// Estimate using the average locally kept text, since IMAP header sizes are not stored. To
    /// fetch: texts the mode keeps that are not stored (none the server could not give, `indexed =
    /// 2`), and in `AllOffline` every header without an original, as
    /// [`known_missing_originals`](Self::known_missing_originals) counts them.
    pub fn cache_estimate(&self, mode: MailCacheMode) -> Result<MailCacheEstimate, MailStoreError> {
        let now = Utc::now();
        let cutoff = (now - Duration::days(BODY_RETENTION_DAYS)).timestamp();
        let (current, bytes): (i64, i64) = self.db.query_row("SELECT count(*), coalesce(sum(length(text) + coalesce(length(html), 0) + length(attachments_json) + length(filenames)), 0) FROM bodies", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let (missing, remove): (i64, i64) = self.db.query_row("SELECT coalesce(sum(((b.seq IS NULL AND m.indexed <> 2) OR (?1 = 2 AND o.seq IS NULL)) AND (?1 = 2 OR (?1 = 1 AND m.date >= ?2))), 0), coalesce(sum(b.seq IS NOT NULL AND (?1 = 0 OR (?1 = 1 AND m.date < ?2)) AND f.role IS NOT 'drafts' AND d.id IS NULL), 0) FROM messages m JOIN folders f ON f.id = m.folder_id LEFT JOIN bodies b ON b.seq = m.seq LEFT JOIN originals o ON o.folder_path = f.path AND o.uidvalidity = f.uidvalidity AND o.uid = m.uid LEFT JOIN drafts d ON d.id = m.id", params![mode as i64, cutoff], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let average = if current > 0 { bytes.max(0) as u64 / current as u64 } else { 16 * 1024 };
        Ok(MailCacheEstimate {
            account: self.account.clone(),
            current_bodies: current.max(0) as u64,
            bodies_to_fetch: missing.max(0) as u64,
            bodies_to_remove: remove.max(0) as u64,
            estimated_download_bytes: missing.max(0) as u64 * average,
            estimated_reclaim_bytes: remove.max(0) as u64 * average,
        })
    }

    /// Up to `limit` messages below `before` (by `seq`, newest first) that the mode wants fetched:
    /// what [`cache_estimate`](Self::cache_estimate) counts as to fetch.
    pub fn cached_body_missing(
        &self,
        before: Option<i64>,
        mode: MailCacheMode,
        limit: u32,
    ) -> Result<Vec<(i64, MessageRef)>, MailStoreError> {
        let cutoff = (Utc::now() - Duration::days(BODY_RETENTION_DAYS)).timestamp();
        let mut statement = self.db.prepare("SELECT m.seq, m.id, m.folder_id, f.path, f.uidvalidity, m.uid, m.rfc_message_id FROM messages m JOIN folders f ON f.id = m.folder_id LEFT JOIN bodies b ON b.seq = m.seq LEFT JOIN originals o ON o.folder_path = f.path AND o.uidvalidity = f.uidvalidity AND o.uid = m.uid WHERE ((b.seq IS NULL AND m.indexed <> 2) OR (?2 = 2 AND o.seq IS NULL)) AND f.uidvalidity IS NOT NULL AND (?1 IS NULL OR m.seq < ?1) AND (?2 = 2 OR (?2 = 1 AND m.date >= ?3)) ORDER BY m.seq DESC LIMIT ?4")?;
        let rows = statement.query_map(params![before, mode as i64, cutoff, limit], |row| {
            Ok((
                row.get(0)?,
                MessageRef {
                    id: row.get(1)?,
                    account: self.account.clone(),
                    folder: row.get(2)?,
                    folder_path: row.get(3)?,
                    uidvalidity: row.get(4)?,
                    uid: row.get(5)?,
                    rfc_message_id: row.get(6)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn attachment_messages(
        &self,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<(i64, MessageRef)>, MailStoreError> {
        let mut statement = self.db.prepare("SELECT m.seq, m.id, m.folder_id, f.path, f.uidvalidity, m.uid, m.rfc_message_id FROM messages m JOIN folders f ON f.id = m.folder_id WHERE m.has_attachments <> 0 AND f.uidvalidity IS NOT NULL AND (?1 IS NULL OR m.seq < ?1) ORDER BY m.seq DESC LIMIT ?2")?;
        let rows = statement.query_map(params![before, limit], |row| {
            Ok((
                row.get(0)?,
                MessageRef {
                    id: row.get(1)?,
                    account: self.account.clone(),
                    folder: row.get(2)?,
                    folder_path: row.get(3)?,
                    uidvalidity: row.get(4)?,
                    uid: row.get(5)?,
                    rfc_message_id: row.get(6)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn attachment_message_count(&self) -> Result<u64, MailStoreError> {
        let count: i64 =
            self.db.query_row("SELECT count(*) FROM messages WHERE has_attachments <> 0", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    pub fn reset_index(&self) -> Result<u64, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let count = tx.execute(
            "UPDATE messages SET indexed = 0, index_partial_or_unverified = 0 WHERE indexed = 1 OR indexed = 2",
            [],
        )?;
        tx.commit()?;
        Ok(count as u64)
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
        // The message left its old place; at the new one it is on the server.
        let old: Option<(String, u32)> = tx
            .query_row("SELECT folder_id, uid FROM messages WHERE id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((old_folder, old_uid)) = old
            && (old_folder != folder || old_uid != uid)
        {
            original_gone(&tx, &old_folder, old_uid)?;
        }
        let changed =
            tx.execute("UPDATE messages SET folder_id = ?2, uid = ?3 WHERE id = ?1", params![id, folder, uid])?;
        if let Some((path, _)) = original_place(&tx, folder)? {
            set_original_gone(&tx, &path, uidvalidity, uid, None)?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Removes one message with its text and index entry (`false`: no such message); its original
    /// is marked gone (a caller that stores the same place again revives it,
    /// [`upsert_message`](Self::upsert_message)). A draft's author stays until
    /// [`forget_draft`](Self::forget_draft).
    #[allow(dead_code, reason = "used from mail.delete_messages, task M3.2")]
    pub fn delete_message(&self, id: &str) -> Result<bool, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let place: Option<(String, u32)> = tx
            .query_row("SELECT folder_id, uid FROM messages WHERE id = ?1", [id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((folder, uid)) = &place {
            original_gone(&tx, folder, *uid)?;
        }
        let removed = tx.execute("DELETE FROM messages WHERE id = ?1", [id])? > 0;
        tx.commit()?;
        Ok(removed)
    }

    /// Removes the messages with these UIDs from the folder (they are gone on the server);
    /// returns how many were there. Their originals are marked gone.
    pub fn delete_uids(&self, folder: &str, uids: &[u32]) -> Result<usize, MailStoreError> {
        let tx = self.db.unchecked_transaction()?;
        let place = original_place(&tx, folder)?;
        let now = Utc::now().timestamp();
        let mut removed = 0;
        for uid in uids {
            if let Some((path, Some(uidvalidity))) = &place {
                set_original_gone(&tx, path, *uidvalidity, *uid, Some(now))?;
            }
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
    /// stores it with [`set_body`](Self::set_body). `html` is always `None`: [`html`](Self::html)
    /// hands it out, only to who asks for it.
    pub fn message(&self, id: &str) -> Result<Option<Message>, MailStoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS}, m.cc_json, m.reply_to_json, m.list_id, m.list_unsubscribe, m.precedence,
                    m.auto_submitted, m.invitation, b.text, b.from_html, b.attachments_json, m.github_reason
             FROM messages m LEFT JOIN bodies b ON b.seq = m.seq WHERE m.id = ?1"
        );
        let row = self.db.query_row(&sql, [id], |row| {
            let summary = summary_from_row(&self.account, row)?;
            // The columns after `SUMMARY_COLUMNS`, which holds [`SUMMARY_WIDTH`] of them.
            let after = |n: usize| SUMMARY_WIDTH + n;
            let attachments_json: Option<String> = row.get(after(9))?;
            Ok(Message {
                summary,
                cc: json_column(row, after(0))?,
                reply_to: json_column(row, after(1))?,
                text: row.get(after(7))?,
                from_html: row.get::<_, Option<bool>>(after(8))?.unwrap_or(false),
                html: None,
                attachments: match attachments_json {
                    Some(json) => parse_json(&json, after(9))?,
                    None => Vec::new(),
                },
                traits: MessageTraits {
                    list_id: row.get(after(2))?,
                    list_unsubscribe: row.get(after(3))?,
                    precedence: row.get(after(4))?,
                    auto_submitted: row.get(after(5))?,
                    github: row.get(after(10))?,
                },
                invitation: row.get(after(6))?,
            })
        });
        Ok(row.optional()?)
    }

    /// The cleaned HTML of the message `id` (task M8.1): `None` while it is not stored (no such
    /// message, its text not fetched or pruned, or kept before the store knew HTML), so the caller
    /// fetches the message; `Some(None)` when the message has no HTML to show; else the HTML.
    pub fn html(&self, id: &str) -> Result<Option<Option<String>>, MailStoreError> {
        let sql = "SELECT b.html FROM bodies b JOIN messages m ON m.seq = b.seq WHERE m.id = ?1";
        let html: Option<Option<String>> = self.db.query_row(sql, [id], |row| row.get(0)).optional()?;
        Ok(html.flatten().map(|html| (!html.is_empty()).then_some(html)))
    }

    /// Forgets the HTML kept with the text of the message `id`, as a store from before version 6
    /// has it: for the tests of fetching it again.
    #[cfg(test)]
    pub(crate) fn forget_html(&self, id: &str) -> Result<bool, MailStoreError> {
        let sql = "UPDATE bodies SET html = NULL WHERE seq = (SELECT seq FROM messages WHERE id = ?1)";
        Ok(self.db.execute(sql, [id])? > 0)
    }

    /// The messages of a folder, newest first; `limit` and `offset` page through them.
    #[allow(dead_code, reason = "used from mail.search_messages, task M3.1")]
    pub fn list_messages(&self, folder: &str, limit: u32, offset: u32) -> Result<Vec<MessageSummary>, MailStoreError> {
        let query = SearchQuery { folder: Some(folder.to_string()), ..SearchQuery::default() };
        self.search(&query, limit, offset)
    }

    /// Searches the cache in `query.order` (or by relevance, [`SearchQuery::relevance`]); `limit`
    /// and `offset` page through the result.
    pub fn search(&self, query: &SearchQuery, limit: u32, offset: u32) -> Result<Vec<MessageSummary>, MailStoreError> {
        Ok(self.search_ranked(query, limit, offset)?.into_iter().map(|(summary, _)| summary).collect())
    }

    /// [`search`](Self::search), each message with its rank: higher is better, 0 for every message
    /// of a search by date. Ranks of several stores can be merged (`mail.search_messages` over
    /// all accounts).
    pub fn search_ranked(
        &self,
        query: &SearchQuery,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<(MessageSummary, f64)>, MailStoreError> {
        let (filter, mut values) = self.filter(query)?;
        let ranked = query.relevance.filter(|_| query.query.has_words());
        let (with, join, rank) = match ranked {
            Some(now) => {
                // BM25 is negative, better more so: a word's rank from the word index, a part of a
                // word's (`rechnung` in `Stromrechnung`) from the trigram index, counting less.
                // Each index is asked once (`MATERIALIZED`); joined row by row, SQLite would run
                // the full-text query again for every message. Only the best [`RANK_CANDIDATES`]
                // of each index count: a word in nearly every mail ranks nothing anyway, and the
                // rest follows by recency.
                let (mut ctes, mut join) = (Vec::new(), String::new());
                let mut score = vec!["1.0".to_string()];
                let mut asked = Vec::new();
                for (table, alias, weight, inside) in
                    [("mail_fts", "r", 1.0, false), ("mail_tri", "t", INSIDE_WEIGHT, true)]
                {
                    let Some(matching) = rank_expression(&query.query, inside) else { continue };
                    if inside && self.words_saturate(&query.query)? {
                        continue;
                    }
                    ctes.push(format!(
                        "{alias} AS MATERIALIZED (SELECT rowid AS hit, bm25({table}, {RANK_WEIGHTS}) AS bm \
                         FROM {table} WHERE {table} MATCH ? ORDER BY bm LIMIT {RANK_CANDIDATES})"
                    ));
                    join.push_str(&format!(" LEFT JOIN {alias} ON {alias}.hit = m.seq"));
                    asked.push(Value::Text(matching));
                    score.push(format!("{weight:.2} * coalesce(-{alias}.bm, 0.0)"));
                }
                values.splice(0..0, asked);
                let rank = format!(
                    "({}) * (1.0 + 0.5 / (1.0 + max({} - m.date, 0) / {RECENT_SECONDS:.1}))",
                    score.join(" + "),
                    now.timestamp()
                );
                // Only excluded words: nothing to rank by but recency.
                let with = if ctes.is_empty() { String::new() } else { format!("WITH {} ", ctes.join(", ")) };
                (with, join, rank)
            }
            None => (String::new(), String::new(), "0.0".to_string()),
        };
        let direction = match query.order {
            Order::NewestFirst => "DESC",
            Order::OldestFirst => "ASC",
        };
        let order = match ranked {
            Some(_) => "rank DESC, m.date DESC, m.seq DESC".to_string(),
            None => format!("m.date {direction}, m.seq {direction}"),
        };
        let sql = format!(
            "{with}SELECT {SUMMARY_COLUMNS}, {rank} AS rank FROM messages m{join} {filter} ORDER BY {order} LIMIT ? OFFSET ?"
        );
        values.push(Value::Integer(i64::from(limit)));
        values.push(Value::Integer(i64::from(offset)));
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| {
            Ok((summary_from_row(&self.account, row)?, row.get::<_, f64>(SUMMARY_WIDTH)?))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The words of `query` match at least [`RANK_CANDIDATES`] messages whole: then they decide
    /// the ranking alone, and the costly rank of parts of words (trigram index) is left out.
    fn words_saturate(&self, query: &Query) -> Result<bool, MailStoreError> {
        let Some(matching) = rank_expression(query, false) else { return Ok(false) };
        let sql =
            format!("SELECT count(*) FROM (SELECT 1 FROM mail_fts WHERE mail_fts MATCH ?1 LIMIT {RANK_CANDIDATES})");
        let found: i64 = self.db.query_row(&sql, [matching], |row| row.get(0))?;
        Ok(found >= i64::from(RANK_CANDIDATES))
    }

    /// How many messages match `query` in all (`total_estimate` of `mail.search_messages`).
    pub fn count(&self, query: &SearchQuery) -> Result<u64, MailStoreError> {
        let (filter, values) = self.filter(query)?;
        let sql = format!("SELECT count(*) FROM messages m {filter}");
        let count: i64 = self.db.query_row(&sql, params_from_iter(values), |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// The `WHERE` clause of `query` over `messages m`, with its values.
    fn filter(&self, query: &SearchQuery) -> Result<(String, Vec<Value>), MailStoreError> {
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        if !query.query.is_empty() {
            let folders = if query.query.terms().any(|term| matches!(term.test, Test::In(_))) {
                self.folder_names()?
            } else {
                Vec::new()
            };
            let (sql, more) = compile(&query.query, &folders);
            conditions.push(sql);
            values.extend(more);
        }
        if let Some(folder) = &query.folder {
            conditions.push("m.folder_id = ?".into());
            values.push(Value::Text(folder.clone()));
        }
        if let Some(unread) = query.unread {
            conditions.push(if unread { "m.flags & 1 = 0" } else { "m.flags & 1 = 1" }.into());
        }
        if let Some(flagged) = query.flagged {
            conditions.push(if flagged { "m.flags & 2 = 2" } else { "m.flags & 2 = 0" }.into());
        }
        if let Some(since) = query.since {
            conditions.push("m.date >= ?".into());
            values.push(Value::Integer(since.timestamp()));
        }
        if let Some(before) = query.before {
            conditions.push("m.date < ?".into());
            values.push(Value::Integer(before.timestamp()));
        }
        // One JSON list per set of ids (task E18.1): SQLite takes at most 32 766 values per query,
        // and "Later" or "Hidden" may name more messages than that.
        if let Some(only) = &query.only {
            if only.is_empty() {
                conditions.push("0".into());
            } else {
                conditions.push(format!("m.id IN {IDS_OF_JSON}"));
                values.push(json_list(only)?);
            }
        }
        if !query.except.is_empty() {
            conditions.push(format!("m.id NOT IN {IDS_OF_JSON}"));
            values.push(json_list(&query.except)?);
        }
        if let Some(hiding) = &query.hidden {
            let (held, more) = self.hidden_condition(hiding)?;
            conditions.push(if hiding.hidden { held } else { format!("NOT {held}") });
            values.extend(more);
        }
        if !query.categories.is_empty() {
            conditions.push(format!("m.category IN ({})", vec!["?"; query.categories.len()].join(", ")));
            values.extend(query.categories.iter().map(|category| Value::Text(category.key().to_string())));
        }
        if query.in_inbox {
            match self.folder_by_role(Role::Inbox)? {
                Some(inbox) => {
                    conditions.push("m.folder_id = ?".into());
                    values.push(Value::Text(inbox.id));
                }
                None => conditions.push("0".into()),
            }
        }
        let filter = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
        Ok((filter, values))
    }

    /// The condition "this message is hidden" of `hiding` over `messages m` ([`Hiding`]), with its
    /// values; never `NULL`, so `NOT` of it is the rest. `0` without an inbox folder or without
    /// anything hidden.
    fn hidden_condition(&self, hiding: &Hiding) -> Result<(String, Vec<Value>), MailStoreError> {
        let Some(inbox) = self.folder_by_role(Role::Inbox)? else { return Ok(("0".into(), Vec::new())) };
        let mut by: Vec<String> = Vec::new();
        let mut values = vec![Value::Text(inbox.id)];
        if !hiding.senders.is_empty() {
            by.push(format!("coalesce(lower(trim(json_extract(m.from_json, '$.address'))) IN {IDS_OF_JSON}, 0)"));
            values.push(json_list(&hiding.senders)?);
        }
        if !hiding.domains.is_empty() {
            // What follows the last `@`: a domain holds no `@`, so `@<domain>` at the end is it.
            by.push(
                "EXISTS (SELECT 1 FROM json_each(?) d WHERE \
                 substr(rtrim(lower(trim(json_extract(m.from_json, '$.address'))), '.'), -length(d.value) - 1) \
                 = '@' || d.value)"
                    .into(),
            );
            values.push(json_list(&hiding.domains)?);
        }
        if !hiding.ids.is_empty() {
            by.push(format!("m.id IN {IDS_OF_JSON}"));
            values.push(json_list(&hiding.ids)?);
        }
        if by.is_empty() {
            return Ok(("0".into(), Vec::new()));
        }
        let mut held = format!("(m.folder_id = ? AND ({})", by.join(" OR "));
        if !hiding.never.is_empty() {
            held.push_str(&format!(" AND m.id NOT IN {IDS_OF_JSON}"));
            values.push(json_list(&hiding.never)?);
        }
        held.push(')');
        Ok((held, values))
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

    /// The messages whose `Message-ID` header is `rfc_message_id` (as the sync stores it: without
    /// angle brackets), newest first: the same mail can be in several folders (task E8.7a). A task's
    /// link names a mail this way, so it is found again after it moved to another folder.
    pub fn messages_by_rfc_id(&self, rfc_message_id: &str) -> Result<Vec<MessageSummary>, MailStoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM messages m WHERE m.rfc_message_id = ?1 ORDER BY m.date DESC, m.seq DESC"
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map([rfc_message_id], |row| summary_from_row(&self.account, row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many mails of the conversation `thread_id` are dated after `after` and not sent from
    /// `own` (the account's address, in any case): the replies that came since (task E8.7a). The
    /// same mail in several folders counts once (by `Message-ID`; one without counts per copy).
    pub fn replies_after(&self, thread_id: &str, after: DateTime<Utc>, own: &str) -> Result<u32, MailStoreError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS}, m.rfc_message_id FROM messages m WHERE m.thread_id = ?1 AND m.date > ?2"
        );
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(params![thread_id, after.timestamp()], |row| {
            Ok((summary_from_row(&self.account, row)?, row.get::<_, Option<String>>(SUMMARY_WIDTH)?))
        })?;
        let mut seen = std::collections::BTreeSet::new();
        let mut count = 0u32;
        for row in rows {
            let (summary, rfc_message_id) = row?;
            let mine = summary.from.as_ref().is_some_and(|from| from.address.eq_ignore_ascii_case(own));
            let again = rfc_message_id.is_some_and(|rfc| !seen.insert(rfc));
            if !mine && !again {
                count += 1;
            }
        }
        Ok(count)
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

/// Inserts an original, or replaces the one at the same place; a place taken again is on the
/// server again (`gone_at` cleared) and keeps its `seq`.
const SAVE_ORIGINAL: &str =
    "INSERT INTO originals (folder_path, uidvalidity, uid, internal_date, flags_json, bytes, sha256, raw)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
     ON CONFLICT(folder_path, uidvalidity, uid) DO UPDATE SET
       internal_date = excluded.internal_date, flags_json = excluded.flags_json,
       bytes = excluded.bytes, sha256 = excluded.sha256, raw = excluded.raw, gone_at = NULL";

/// `bytes` as the column holds it; [`MailStoreError::OriginalTooLarge`] above [`ORIGINAL_MAX_BYTES`].
fn original_length(bytes: u64) -> Result<i64, MailStoreError> {
    if bytes > ORIGINAL_MAX_BYTES {
        return Err(MailStoreError::OriginalTooLarge);
    }
    i64::try_from(bytes).map_err(|_| MailStoreError::OriginalTooLarge)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Path and UIDVALIDITY of the folder `folder` (an id): where its messages' originals are kept.
fn original_place(db: &Connection, folder: &str) -> rusqlite::Result<Option<(String, Option<u32>)>> {
    db.query_row("SELECT path, uidvalidity FROM folders WHERE id = ?1", [folder], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()
}

/// Notes that the message `uid` left `path` under `uidvalidity` (`gone`: since when, Unix
/// seconds), or that it is there again (`None`). A UID is never given again under the same
/// UIDVALIDITY, so a place once gone only comes back when the server lists the same message again.
fn set_original_gone(
    db: &Connection,
    path: &str,
    uidvalidity: u32,
    uid: u32,
    gone: Option<i64>,
) -> rusqlite::Result<()> {
    let sql = match gone {
        Some(_) => {
            "UPDATE originals SET gone_at = ?4 WHERE folder_path = ?1 AND uidvalidity = ?2 AND uid = ?3 AND gone_at IS NULL"
        }
        None => {
            "UPDATE originals SET gone_at = ?4 WHERE folder_path = ?1 AND uidvalidity = ?2 AND uid = ?3 AND gone_at IS NOT NULL"
        }
    };
    db.execute(sql, params![path, uidvalidity, uid, gone])?;
    Ok(())
}

/// [`set_original_gone`] for the message `uid` of folder `folder` (an id), now.
fn original_gone(db: &Connection, folder: &str, uid: u32) -> rusqlite::Result<()> {
    if let Some((path, Some(uidvalidity))) = original_place(db, folder)? {
        set_original_gone(db, &path, uidvalidity, uid, Some(Utc::now().timestamp()))?;
    }
    Ok(())
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
    if version < 1
        && let Err(err) = tx.execute_batch(SCHEMA_V1)
    {
        // `no such module: fts5` from the virtual table.
        return Err(if err.to_string().contains("fts5") { MailStoreError::NoFullText } else { err.into() });
    }
    tx.execute_batch(SCHEMA_V2)?;
    // `ADD COLUMN` cannot be run twice, so only a store from before has it run.
    if version < 3 {
        tx.execute_batch(SCHEMA_V3)?;
    }
    if version < 4 {
        tx.execute_batch(SCHEMA_V4)?;
    }
    if version < 5 {
        migrate_v5(&tx)?;
    }
    if version < 6 && !has_column(&tx, "bodies", "html")? {
        tx.execute_batch(SCHEMA_V6)?;
    }
    if version < 7 && !has_column(&tx, "messages", "category")? {
        tx.execute_batch(SCHEMA_V7)?;
    }
    if version < 8 && !has_column(&tx, "messages", "index_partial_or_unverified")? {
        tx.execute_batch(SCHEMA_V8)?;
        let known: Vec<(i64, bool)> = {
            let mut statement =
                tx.prepare("SELECT m.seq, b.text FROM messages m JOIN bodies b ON b.seq = m.seq WHERE m.indexed = 1")?;
            statement
                .query_map([], |row| {
                    let text: String = row.get(1)?;
                    Ok((row.get(0)?, index_is_partial(&text)))
                })?
                .collect::<Result<_, _>>()?
        };
        for (seq, partial) in known {
            tx.execute("UPDATE messages SET index_partial_or_unverified = ?2 WHERE seq = ?1", params![seq, partial])?;
        }
    }
    if version < 9 {
        tx.execute_batch(SCHEMA_V9)?;
    }
    if version < 10 {
        tx.execute_batch(SCHEMA_V10)?;
    }
    if version < 11 {
        for (table, column, sql) in [
            ("originals", "gone_at", SCHEMA_V11_GONE),
            ("folders", "delimiter", SCHEMA_V11_DELIMITER),
            ("messages", "internal_date", SCHEMA_V11_INTERNAL_DATE),
        ] {
            if !has_column(&tx, table, column)? {
                tx.execute_batch(sql)?;
            }
        }
        tx.execute_batch(SCHEMA_V11_INDEX)?;
        tx.execute(SCHEMA_V11_MARK_GONE, [Utc::now().timestamp()])?;
    }
    if version < 12 && !has_column(&tx, "messages", "size")? {
        tx.execute_batch(SCHEMA_V12_SIZE)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}

/// Version 5 (issue #136): the indexes of folded text, rebuilt from the headers and the texts the
/// store holds. Texts pruned before are not there: their messages count as not indexed, so the
/// sync fetches them again in the background. Runs once per store, on the first open after the
/// update; a large store takes a few seconds.
fn migrate_v5(db: &Connection) -> Result<(), MailStoreError> {
    if !has_column(db, "messages", "indexed")? {
        db.execute_batch("ALTER TABLE messages ADD COLUMN indexed INTEGER NOT NULL DEFAULT 0;")?;
    }
    db.execute_batch(SCHEMA_V5)?;
    db.execute("UPDATE messages SET indexed = ?1 WHERE seq IN (SELECT seq FROM bodies)", [INDEXED])?;
    let rows: Vec<(i64, Option<String>, Option<String>)> = {
        let mut statement =
            db.prepare("SELECT m.seq, b.text, b.filenames FROM messages m LEFT JOIN bodies b ON b.seq = m.seq")?;
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect::<Result<_, _>>()?
    };
    for (seq, text, filenames) in rows {
        let body = text.as_deref().map(|body| IndexText { body, filenames: filenames.as_deref().unwrap_or("") });
        refresh_index(db, seq, body)?;
    }
    Ok(())
}

/// The table `table` has the column `column` (a store that a later version wrote and an earlier
/// one marked as its own may have it already).
fn has_column(db: &Connection, table: &str, column: &str) -> Result<bool, MailStoreError> {
    let mut statement = db.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")?;
    Ok(statement.exists([table, column])?)
}

// ----- SQL pieces -----

/// The values of one JSON list bound as one value ([`json_list`]), for `IN` and `NOT IN` (task
/// E18.1).
const IDS_OF_JSON: &str = "(SELECT value FROM json_each(?))";

/// `items` as one JSON list, the value [`IDS_OF_JSON`] reads.
fn json_list(items: &[String]) -> Result<Value, MailStoreError> {
    Ok(Value::Text(serde_json::to_string(items)?))
}

const FOLDER_SELECT: &str = "SELECT f.id, f.name, f.path, f.role,
        (SELECT count(*) FROM messages m WHERE m.folder_id = f.id AND m.flags & 1 = 0),
        (SELECT count(*) FROM messages m WHERE m.folder_id = f.id)
    FROM folders f";

/// Inbox, drafts, sent, archive, junk, trash, then folders without a role; by path inside each.
const FOLDER_ORDER: &str =
    "CASE f.role WHEN 'inbox' THEN 0 WHEN 'drafts' THEN 1 WHEN 'sent' THEN 2 WHEN 'archive' THEN 3
        WHEN 'junk' THEN 4 WHEN 'trash' THEN 5 ELSE 6 END, f.path";

const SUMMARY_COLUMNS: &str = "m.id, m.folder_id, m.thread_id, m.from_json, m.to_json, m.subject, m.date, m.snippet,
    m.flags, m.has_attachments, m.invite_json, m.invite_answer, m.category, m.category_source, m.category_confidence";

/// How many columns [`SUMMARY_COLUMNS`] selects: where the columns a query adds after it begin.
/// A query that appends its own must count from here, never from a number written out – task
/// E12.2 added three columns in the middle and every hard-coded index moved.
const SUMMARY_WIDTH: usize = 15;

const INSERT_HEADERS: &str = "INSERT INTO messages (id, folder_id, uid, rfc_message_id, thread_id, from_json, to_json,
    cc_json, reply_to_json, subject, sender, recipients, date, snippet, flags, has_attachments, list_id,
    list_unsubscribe, precedence, auto_submitted, invitation, github_reason)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

const UPDATE_HEADERS: &str = "UPDATE messages SET rfc_message_id = ?2, thread_id = ?3, from_json = ?4, to_json = ?5,
    cc_json = ?6, reply_to_json = ?7, subject = ?8, sender = ?9, recipients = ?10, date = ?11, snippet = ?12,
    flags = ?13, has_attachments = ?14, list_id = ?15, list_unsubscribe = ?16, precedence = ?17,
    auto_submitted = ?18, invitation = ?19, github_reason = ?20 WHERE seq = ?1";

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
        optional(&new.traits.github),
    ])
}

/// Name and address as words for the index.
fn address_words(address: &MailAddress) -> String {
    match &address.name {
        Some(name) => format!("{name} {}", address.address),
        None => address.address.clone(),
    }
}

/// A message's text and attachment names as they go into the index, before folding.
struct IndexText<'a> {
    body: &'a str,
    /// The names, separated by spaces.
    filenames: &'a str,
}

/// Rewrites the index entries of one message: its headers from its row, folded, and with `text`
/// its text and attachment names (folded, cut at [`INDEX_TEXT_MAX`] and [`TRIGRAM_TEXT_MAX`]).
/// Without `text` the text already in the index stays: new headers (a sync, a flag) never lose
/// words of a text that is no longer kept to read.
fn index_is_partial(text: &str) -> bool {
    text.len() > INDEX_TEXT_MAX || fold(head(text, INDEX_TEXT_MAX)).len() > TRIGRAM_TEXT_MAX
}

fn refresh_index(db: &Connection, seq: i64, text: Option<IndexText<'_>>) -> Result<(), MailStoreError> {
    let headers: Option<(String, String, String)> = db
        .query_row("SELECT subject, sender, recipients FROM messages WHERE seq = ?1", [seq], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .optional()?;
    let Some((subject, sender, recipients)) = headers else { return Ok(()) };
    let (body, filenames) = match text {
        Some(text) => (fold(head(text.body, INDEX_TEXT_MAX)), fold(text.filenames)),
        None => db
            .query_row("SELECT body, filenames FROM mail_fts WHERE rowid = ?1", [seq], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?
            .unwrap_or_default(),
    };
    let (subject, sender, recipients) = (fold(&subject), fold(&sender), fold(&recipients));
    for (table, body) in [("mail_fts", body.as_str()), ("mail_tri", head(&body, TRIGRAM_TEXT_MAX))] {
        db.execute(&format!("DELETE FROM {table} WHERE rowid = ?1"), [seq])?;
        db.execute(
            &format!("INSERT INTO {table} (rowid, subject, sender, recipients, body, filenames) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"),
            params![seq, subject, sender, recipients, body, filenames],
        )?;
    }
    Ok(())
}

/// The column of the indexes a field searches; `None` for all of them.
fn column(field: Field) -> Option<&'static str> {
    match field {
        Field::Any => None,
        Field::From => Some("sender"),
        Field::To => Some("recipients"),
        Field::Subject => Some("subject"),
        Field::Filename => Some("filenames"),
    }
}

/// The FTS5 expression of `needle` in `column` (all columns without one): its words quoted as one
/// phrase, the last one a prefix for a typed word. `trigram`: as one string for the trigram index,
/// which finds it anywhere.
fn fts_expression(needle: &Needle, column: Option<&str>, trigram: bool) -> String {
    let quoted = format!("\"{}\"", needle.tokens.join(" ").replace('"', "\"\""));
    let star = if needle.prefix && !trigram { "*" } else { "" };
    match column {
        Some(column) => format!("{column} : {quoted}{star}"),
        None => format!("{quoted}{star}"),
    }
}

/// The needle of a text test, as the indexes see it.
fn needle_of(text: &str, phrase: bool) -> Needle {
    if phrase { Needle::phrase(text) } else { Needle::word(text) }
}

/// `query` as a condition over `messages m`, with its values in order. `folders` are the store's,
/// for `in:` (only needed when the query has one). A word or phrase without letters or digits
/// matches nothing; `in:` a folder the store does not have matches nothing either.
fn compile(query: &Query, folders: &[Folder]) -> (String, Vec<Value>) {
    let mut values = Vec::new();
    let clauses: Vec<String> = query
        .clauses
        .iter()
        .map(|clause| {
            let any: Vec<String> = clause
                .any
                .iter()
                .map(|term| {
                    let test = test_sql(&term.test, folders, &mut values);
                    if term.negated { format!("NOT ({test})") } else { test }
                })
                .collect();
            format!("({})", any.join(" OR "))
        })
        .collect();
    (clauses.join(" AND "), values)
}

/// One test of a query as SQL; its values are appended to `values`.
fn test_sql(test: &Test, folders: &[Folder], values: &mut Vec<Value>) -> String {
    match test {
        Test::Text { field, text, phrase } => {
            let needle = needle_of(text, *phrase);
            if needle.tokens.is_empty() {
                return "0".into();
            }
            let column = column(*field);
            values.push(Value::Text(fts_expression(&needle, column, false)));
            let words = "m.seq IN (SELECT rowid FROM mail_fts WHERE mail_fts MATCH ?)";
            if !needle.infix() {
                return words.into();
            }
            values.push(Value::Text(fts_expression(&needle, column, true)));
            format!("{words} OR m.seq IN (SELECT rowid FROM mail_tri WHERE mail_tri MATCH ?)")
        }
        Test::HasAttachment => "m.has_attachments = 1".into(),
        Test::Unread(true) => "m.flags & 1 = 0".into(),
        Test::Unread(false) => "m.flags & 1 = 1".into(),
        Test::Flagged(true) => "m.flags & 2 = 2".into(),
        Test::Flagged(false) => "m.flags & 2 = 0".into(),
        Test::In(name) => {
            let wanted = name.to_lowercase();
            let role = match wanted.as_str() {
                "spam" => Some(Role::Junk),
                "bin" => Some(Role::Trash),
                other => role_from_name(other),
            };
            let ids: Vec<&str> = folders
                .iter()
                .filter(|folder| {
                    (role.is_some() && folder.role == role)
                        || folder.name.to_lowercase() == wanted
                        || folder.path.to_lowercase() == wanted
                })
                .map(|folder| folder.id.as_str())
                .collect();
            if ids.is_empty() {
                return "0".into();
            }
            values.extend(ids.iter().map(|id| Value::Text((*id).to_string())));
            format!("m.folder_id IN ({})", vec!["?"; ids.len()].join(", "))
        }
        Test::Before(at) => {
            values.push(Value::Integer(at.timestamp()));
            "m.date < ?".into()
        }
        Test::After(at) => {
            values.push(Value::Integer(at.timestamp()));
            "m.date >= ?".into()
        }
    }
}

/// The FTS5 expression a search by relevance ranks with: every word and phrase the query looks
/// for, in its field, joined by `OR`; with `inside` the words that also match inside longer
/// words, for the trigram index. `None` without any.
fn rank_expression(query: &Query, inside: bool) -> Option<String> {
    let parts: Vec<String> = query
        .terms()
        .filter(|term| !term.negated)
        .filter_map(|term| match &term.test {
            Test::Text { field, text, phrase } => {
                let needle = needle_of(text, *phrase);
                let wanted = if inside { needle.infix() } else { !needle.tokens.is_empty() };
                wanted.then(|| fts_expression(&needle, column(*field), inside))
            }
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
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
        invite: invite_from_row(row)?,
        back: None,
        category: category_from_row(row)?,
        untrusted: true,
    })
}

/// The category of a summary row (columns 12 to 14, task E12.2). A value this version cannot read
/// counts as none: a list must not fail on one message, just as with an invitation.
fn category_from_row(row: &Row<'_>) -> rusqlite::Result<Option<MailCategory>> {
    let key: Option<String> = row.get(12)?;
    let source: Option<String> = row.get(13)?;
    let confidence: Option<i64> = row.get(14)?;
    let Some(category) = key.as_deref().and_then(Category::from_key) else { return Ok(None) };
    let source = match source.as_deref() {
        Some("rule") => CategorySource::Rule,
        Some("model") => CategorySource::Model,
        Some("person") => CategorySource::Person,
        _ => return Ok(None),
    };
    let confidence = match source {
        CategorySource::Model => confidence.map(|value| value.clamp(0, 100) as u8),
        CategorySource::Rule | CategorySource::Person => None,
    };
    Ok(Some(MailCategory { category, source, confidence }))
}

/// The invitation of a summary row (columns 10 and 11). A value this version cannot read counts as
/// none: a list must not fail on one message.
fn invite_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Invite>> {
    let json: Option<String> = row.get(10)?;
    let answer: Option<String> = row.get(11)?;
    let invite = json.and_then(|json| serde_json::from_str::<Invite>(&json).ok());
    Ok(invite.map(|invite| Invite { answered: answer.as_deref().and_then(InvitationAnswer::from_name), ..invite }))
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
    use crate::mail::query::parse;

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
        let query = SearchQuery { query: parse(text).expect("a query"), ..SearchQuery::default() };
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

    /// Task E8.7a: a mail is found by its Message-ID in whatever folder it is, and the replies after
    /// it count once each, the account's own not at all.
    #[test]
    fn a_mail_is_found_by_its_message_id_and_later_replies_count() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let sent = folder(&store, "Sent", Some(Role::Sent));
        let offer = NewMessage { thread_id: "offer".into(), ..message(3, "Offer") };
        let moved = add(&store, &archive, &offer);
        assert!(store.messages_by_rfc_id("<3@example.org>").expect("by id").iter().any(|m| m.id == moved));
        assert!(store.messages_by_rfc_id("<4@example.org>").expect("by id").is_empty());
        // A reply from Bernd, the same reply in a second folder, and the person's own answer.
        let reply = NewMessage { thread_id: "offer".into(), ..message(5, "Re: Offer") };
        add(&store, &inbox, &reply);
        add(&store, &archive, &NewMessage { uid: 50, ..reply.clone() });
        let own = NewMessage {
            thread_id: "offer".into(),
            from: Some(address(None, "Anna@Example.org")),
            ..message(6, "Re: Re: Offer")
        };
        add(&store, &sent, &own);
        assert_eq!(store.replies_after("offer", day(3), "anna@example.org").expect("replies"), 1);
        assert_eq!(store.replies_after("offer", day(5), "anna@example.org").expect("replies"), 0);
        assert_eq!(store.messages_by_rfc_id("<5@example.org>").expect("copies").len(), 2, "both copies");
    }

    /// Task E8.3b: the invitation read with the text is in every summary, outlives the text and
    /// keeps the answer given when the text is read again; a store of version 2 gets the columns.
    #[test]
    fn invitations_and_their_answers_are_kept_beside_the_text() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let id = add(&store, &inbox, &message(4, "Invitation: Planning"));
        assert_eq!(store.summary(&id).expect("summary").expect("there").invite, None);
        let invite =
            Invite { title: "Planning".into(), start: day(15), end: Some(day(16)), all_day: false, answered: None };
        let body = Body {
            text: "Join us".into(),
            from_html: false,
            html: None,
            attachments: vec![],
            invite: Some(invite.clone()),
        };
        assert!(store.set_body(&id, &body).expect("body"));
        assert!(store.set_invite_answer(&id, Some(InvitationAnswer::Accept)).expect("answer"));
        let summary = store.summary(&id).expect("summary").expect("there");
        assert_eq!(summary.invite, Some(Invite { answered: Some(InvitationAnswer::Accept), ..invite.clone() }));
        store.set_body(&id, &body).expect("read again");
        let listed = store.list_messages(&inbox, 10, 0).expect("listed");
        assert_eq!(listed[0].invite.as_ref().and_then(|invite| invite.answered), Some(InvitationAnswer::Accept));
        assert_eq!(store.prune_bodies(day(15) + Duration::days(BODY_RETENTION_DAYS)).expect("pruned"), 1);
        assert_eq!(
            store.message(&id).expect("message").expect("there").summary.invite.map(|i| i.title),
            Some("Planning".into())
        );
        assert!(!store.set_invite_answer("m:nope", None).expect("none"));

        // A store as version 2 left it: the columns are added, the messages stay.
        store.db.execute_batch("ALTER TABLE messages DROP COLUMN github_reason;").expect("drop");
        store.db.execute_batch("ALTER TABLE messages DROP COLUMN invite_json;").expect("drop");
        store.db.execute_batch("ALTER TABLE messages DROP COLUMN invite_answer; PRAGMA user_version = 2").expect("v2");
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        assert_eq!(store.summary(&id).expect("summary").expect("kept").invite, None);
    }

    #[test]
    fn a_store_of_version_3_learns_the_github_reason_and_keeps_its_messages() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let mut new = message(1, "Add the inbox");
        new.traits.github = Some("review_requested".into());
        let id = add(&store, &inbox, &new);
        store.db.execute_batch("ALTER TABLE messages DROP COLUMN github_reason; PRAGMA user_version = 3").expect("v3");
        drop(store);

        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        let version: i64 = store.db.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        let kept = store.message(&id).expect("read").expect("kept");
        assert_eq!((kept.summary.subject.as_str(), kept.traits.github), ("Add the inbox", None), "old rows: none");
        let again = add(&store, &inbox, &new);
        assert_eq!(again, id, "the same UID");
        let read = store.message(&id).expect("read").expect("there");
        assert_eq!(read.traits.github.as_deref(), Some("review_requested"), "written from now on");
    }

    /// Issue #136: a store of version 4 gets both indexes of folded text, rebuilt from what it
    /// holds; messages without a kept text wait for the background fetch, and one without any
    /// text is not handed out again.
    #[test]
    fn a_store_of_version_4_gets_folded_indexes_and_knows_what_to_fetch() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let with_text = add(&store, &inbox, &message(1, "Grüße"));
        let body =
            Body { text: "Ihre Stromrechnung".into(), from_html: false, html: None, attachments: vec![], invite: None };
        store.set_body(&with_text, &body).expect("body");
        let without = add(&store, &inbox, &message(2, "Kein Text"));
        store
            .db
            .execute_batch(
                "DROP INDEX messages_unindexed; ALTER TABLE messages DROP COLUMN indexed; DROP TABLE mail_tri;
                 PRAGMA user_version = 4",
            )
            .expect("v4");
        drop(store);

        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        assert_eq!(store.index_counts().expect("counts"), (2, 1));
        assert!(!store.fully_indexed().expect("asked"));
        assert_eq!(
            search_ids(&store, "rechnung"),
            std::slice::from_ref(&with_text),
            "inside a word, from the trigram index"
        );
        assert_eq!(search_ids(&store, "gruesse"), [with_text]);
        let next: Vec<String> = store.unindexed(10).expect("next").into_iter().map(|(at, _)| at.id).collect();
        assert_eq!(next, std::slice::from_ref(&without));
        assert!(store.mark_unindexable(&without).expect("marked"));
        assert!(store.unindexed(10).expect("next").is_empty() && !store.fully_indexed().expect("asked"));
        assert_eq!(search_ids(&store, "kein"), [without], "its headers are searchable");
    }

    /// Task M8.1: the cleaned HTML is kept beside the text and handed out on its own; a message
    /// without HTML says so, and a text kept before the store knew HTML (version 5) is not stored
    /// HTML, so it is fetched once more. Pruning the text drops the HTML too.
    #[test]
    fn html_is_kept_beside_the_text_and_rows_from_before_have_none_stored() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", None);
        let html = add(&store, &inbox, &message(1, "Newsletter"));
        let plain = add(&store, &inbox, &message(2, "Plain"));
        let body = |html: Option<&str>| Body {
            text: "Hello".into(),
            from_html: false,
            html: html.map(str::to_owned),
            attachments: vec![],
            invite: None,
        };
        assert_eq!(store.html(&html).expect("asked"), None, "no text read yet");
        store.set_body(&html, &body(Some("<p>Hello</p>"))).expect("body");
        store.set_body(&plain, &body(None)).expect("body");
        assert_eq!(store.html(&html).expect("asked"), Some(Some("<p>Hello</p>".into())));
        assert_eq!(store.html(&plain).expect("asked"), Some(None), "read, and it has none");
        assert_eq!(store.html("m:nope").expect("asked"), None);
        assert_eq!(store.message(&html).expect("read").expect("there").html, None, "only through html()");

        store.db.execute_batch("ALTER TABLE bodies DROP COLUMN html; PRAGMA user_version = 5").expect("v5");
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        assert_eq!(store.html(&html).expect("asked"), None, "kept before: fetch it again");
        assert_eq!(store.message(&html).expect("read").expect("there").text.as_deref(), Some("Hello"), "text stays");
        store.set_body(&html, &body(Some("<p>Hello</p>"))).expect("read again");
        assert_eq!(store.html(&html).expect("asked"), Some(Some("<p>Hello</p>".into())));
        assert_eq!(store.prune_bodies(day(3) + Duration::days(BODY_RETENTION_DAYS)).expect("pruned"), 2);
        assert_eq!(store.html(&html).expect("asked"), None, "pruned with the text");
    }

    /// Task E12.2: what a mail is is kept with it, filters the search and survives the migration
    /// of a store written before schema 7.
    #[test]
    fn the_category_of_a_mail_is_kept_filtered_and_migrated() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let bill = add(&store, &inbox, &message(1, "Your invoice"));
        let parcel = add(&store, &inbox, &message(2, "Your parcel is on its way"));
        let quiet = add(&store, &inbox, &message(3, "Nothing in particular"));
        assert_eq!(store.summary(&bill).expect("summary").expect("there").category, None, "nothing said yet");

        store.set_category(&bill, Some(MailCategory::by_model(Category::Invoice, 0.93))).expect("kept");
        store.set_category(&parcel, Some(MailCategory::by_rule(Category::Shipping))).expect("kept");
        let held = store.summary(&bill).expect("summary").expect("there").category.expect("a category");
        assert_eq!((held.category, held.source, held.confidence), (Category::Invoice, CategorySource::Model, Some(93)));
        let held = store.summary(&parcel).expect("summary").expect("there").category.expect("a category");
        assert_eq!((held.category, held.source, held.confidence), (Category::Shipping, CategorySource::Rule, None));
        assert!(!store.set_category("m:nope", None).expect("asked"), "no such message");

        // The typed filter picks a set of categories and leaves mail without one out.
        let of = |categories: Vec<Category>| {
            let query = SearchQuery { categories, ..SearchQuery::default() };
            store.search(&query, 50, 0).expect("searched").into_iter().map(|m| m.id).collect::<Vec<_>>()
        };
        assert_eq!(of(vec![Category::Invoice]), std::slice::from_ref(&bill));
        assert_eq!(of(vec![Category::Invoice, Category::Shipping]).len(), 2, "one of the set");
        assert_eq!(of(vec![Category::Spam]), Vec::<String>::new());
        assert_eq!(of(Vec::new()).len(), 3, "no condition: every mail, with or without one");
        assert!(of(Vec::new()).contains(&quiet), "mail without a category is only left out by a filter");

        // Taking it away again leaves the mail without one.
        store.set_category(&bill, None).expect("taken away");
        assert_eq!(store.summary(&bill).expect("summary").expect("there").category, None);

        // A store written before schema 7 gets the columns, and its mail survives.
        store.set_category(&bill, Some(MailCategory::by_person(Category::Document))).expect("kept");
        store
            .db
            .execute_batch(
                "DROP INDEX IF EXISTS messages_by_category;
                 ALTER TABLE messages DROP COLUMN category;
                 ALTER TABLE messages DROP COLUMN category_source;
                 ALTER TABLE messages DROP COLUMN category_confidence;
                 ALTER TABLE messages DROP COLUMN needs_reply;
                 ALTER TABLE messages DROP COLUMN urgency;
                 PRAGMA user_version = 6",
            )
            .expect("v6");
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        let version: i64 = store.db.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(store.summary(&parcel).expect("summary").expect("there").subject, "Your parcel is on its way");
        assert_eq!(store.summary(&bill).expect("summary").expect("there").category, None, "said anew after the move");
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
            github: Some("review_requested".into()),
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
        store
            .set_body(
                &id,
                &Body { text: "the text".into(), from_html: true, html: None, attachments: vec![], invite: None },
            )
            .expect("body");
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
                    html: None,
                    attachments: vec![AttachmentInfo {
                        part: "2".into(),
                        filename: Some("Übersicht Q3.pdf".into()),
                        content_type: "application/pdf".into(),
                        size: 10,
                        inline: false,
                        content_id: None,
                    }],
                    invite: None,
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
        // Task E8.15: oldest first, paged the same way.
        let oldest = SearchQuery { order: Order::OldestFirst, ..in_inbox.clone() };
        assert_eq!(list(&oldest, 2, 0), ["Message 1", "Message 2"]);
        assert_eq!(list(&oldest, 2, 2), ["Message 3", "Message 4"]);
        assert_eq!(list(&oldest, 2, 4), ["Message 5"]);

        let unread = SearchQuery { unread: Some(true), folder: Some(inbox.clone()), ..SearchQuery::default() };
        assert_eq!(list(&unread, 10, 0), ["Message 5", "Message 3", "Message 1"]);
        let read = SearchQuery { unread: Some(false), folder: Some(inbox.clone()), ..SearchQuery::default() };
        assert_eq!(list(&read, 10, 0), ["Message 4", "Message 2"]);
        let flagged = SearchQuery { flagged: Some(true), ..SearchQuery::default() };
        assert_eq!(list(&flagged, 10, 0), ["Message 5", "Message 3", "Message 1"]);
        let window = SearchQuery { since: Some(day(2)), before: Some(day(4)), ..SearchQuery::default() };
        assert_eq!(list(&window, 10, 0), ["Message 3", "Message 2"], "since is inclusive, before is not");
        let all = SearchQuery {
            query: parse("message").expect("a query"),
            unread: Some(true),
            since: Some(day(2)),
            ..SearchQuery::default()
        };
        assert_eq!(list(&all, 10, 0), ["Message 5", "Message 3"]);
        let unknown = SearchQuery { folder: Some("f:nope".into()), ..SearchQuery::default() };
        assert!(list(&unknown, 10, 0).is_empty());
        // Task E9.1: "Later" picks messages by id, before paging.
        let later = SearchQuery { except: vec![ids[4].clone(), ids[2].clone()], ..in_inbox.clone() };
        assert_eq!(list(&later, 2, 0), ["Message 4", "Message 2"]);
        assert_eq!(list(&later, 2, 2), ["Message 1"]);
        assert_eq!(store.count(&later).expect("count"), 3);
        let only = SearchQuery { only: Some(vec![ids[2].clone(), other.clone()]), ..in_inbox.clone() };
        assert_eq!(list(&only, 10, 0), ["Message 3"], "and still in the folder asked for");
        let none = SearchQuery { only: Some(Vec::new()), ..SearchQuery::default() };
        assert!(list(&none, 10, 0).is_empty());
        assert_eq!(store.count(&none).expect("count"), 0);
    }

    /// Task E18.1: hidden mail of the inbox folder by sender, domain or id, never one of `never`
    /// and never mail in another folder; left out or kept before paging, so pages and counts hold.
    #[test]
    fn hidden_mail_is_picked_in_the_inbox_folder_only() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let from = |uid: u32, sender: &str| NewMessage {
            from: Some(address(None, sender)),
            ..message(uid, &format!("Message {uid}"))
        };
        let news = add(&store, &inbox, &from(1, " News@Shop.Example "));
        add(&store, &inbox, &from(2, "deals@shop.example."));
        add(&store, &inbox, &from(3, "info@mail.shop.example"));
        let anna = add(&store, &inbox, &from(4, "anna@example.org"));
        add(&store, &inbox, &from(5, "bob@example.org"));
        let open_one = add(&store, &inbox, &from(6, "news@shop.example"));
        add(&store, &archive, &from(7, "news@shop.example"));
        let nobody = add(&store, &inbox, &NewMessage { from: None, ..message(8, "Message 8") });

        let subjects = |query: &SearchQuery, limit, offset| -> Vec<String> {
            store.search(query, limit, offset).expect("search").into_iter().map(|s| s.subject).collect()
        };
        let hiding = |hidden: bool| Hiding {
            hidden,
            senders: vec!["news@shop.example".into()],
            domains: vec!["shop.example".into()],
            ids: vec![anna.clone(), nobody.clone()],
            never: vec![open_one.clone()],
        };
        let hidden = SearchQuery { hidden: Some(hiding(true)), ..SearchQuery::default() };
        assert_eq!(subjects(&hidden, 10, 0), ["Message 8", "Message 4", "Message 2", "Message 1"]);
        assert_eq!(store.count(&hidden).expect("count"), 4);
        let shown = SearchQuery { hidden: Some(hiding(false)), ..SearchQuery::default() };
        assert_eq!(subjects(&shown, 10, 0), ["Message 7", "Message 6", "Message 5", "Message 3"]);
        assert_eq!(subjects(&shown, 2, 2), ["Message 5", "Message 3"], "paged after the filter");
        assert_eq!(store.count(&shown).expect("count"), 4);
        let in_archive = SearchQuery { folder: Some(archive.clone()), ..shown.clone() };
        assert_eq!(subjects(&in_archive, 10, 0), ["Message 7"], "another folder hides nothing");

        // Moved to the archive, it keeps its id and is hidden no more.
        assert!(store.relocate_message(&news, &archive, 7, 70).expect("moved"));
        assert!(!subjects(&hidden, 10, 0).contains(&"Message 1".to_string()));
        assert_eq!(subjects(&in_archive, 10, 0), ["Message 7", "Message 1"]);

        // Nothing named: nothing hidden.
        let none = SearchQuery { hidden: Some(Hiding { hidden: true, ..Hiding::default() }), ..SearchQuery::default() };
        assert!(subjects(&none, 10, 0).is_empty());
        let all = SearchQuery { hidden: Some(Hiding::default()), ..SearchQuery::default() };
        assert_eq!(store.count(&all).expect("count"), 8);
    }

    /// Task E18.1: without an inbox folder nothing is hidden.
    #[test]
    fn without_an_inbox_folder_nothing_is_hidden() {
        let (_tmp, _dirs, store) = open();
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let id = add(&store, &archive, &message(1, "Message 1"));
        let hiding = |hidden| Hiding { hidden, ids: vec![id.clone()], ..Hiding::default() };
        let hidden = SearchQuery { hidden: Some(hiding(true)), ..SearchQuery::default() };
        assert_eq!(store.count(&hidden).expect("count"), 0);
        let shown = SearchQuery { hidden: Some(hiding(false)), ..SearchQuery::default() };
        assert_eq!(store.count(&shown).expect("count"), 1);
    }

    /// Task E18.1: 50 000 ids in `except`, `only` and the hidden ones go, as one value each.
    #[test]
    fn fifty_thousand_ids_go_in_one_search() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let kept = add(&store, &inbox, &message(1, "Message 1"));
        let left_out = add(&store, &inbox, &message(2, "Message 2"));
        let mut many: Vec<String> = (0..50_000).map(|n| format!("m:none{n}")).collect();
        many.push(left_out.clone());
        let subjects = |query: &SearchQuery| -> Vec<String> {
            store.search(query, 10, 0).expect("search").into_iter().map(|s| s.subject).collect()
        };
        let except = SearchQuery { except: many.clone(), ..SearchQuery::default() };
        assert_eq!(subjects(&except), ["Message 1"]);
        assert_eq!(store.count(&except).expect("count"), 1);
        let only = SearchQuery { only: Some(many.clone()), ..SearchQuery::default() };
        assert_eq!(subjects(&only), ["Message 2"]);
        let hiding = Hiding { hidden: false, ids: many.clone(), never: vec![kept], ..Hiding::default() };
        let shown = SearchQuery { hidden: Some(hiding), ..SearchQuery::default() };
        assert_eq!(subjects(&shown), ["Message 1"]);
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
            .set_body(
                &gone,
                &Body { text: "secret words".into(), from_html: false, html: None, attachments: vec![], invite: None },
            )
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
        store
            .set_body(
                &one,
                &Body { text: "alpha text".into(), from_html: false, html: None, attachments: vec![], invite: None },
            )
            .expect("body");
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
            store
                .set_body(
                    id,
                    &Body { text: "zebra".into(), from_html: false, html: None, attachments: vec![], invite: None },
                )
                .expect("body");
        }
        assert_eq!(search_ids(&store, "zebra").len(), 2);

        assert_eq!(store.prune_bodies(now).expect("prune"), 1);
        assert_eq!(store.message(&old).expect("read").expect("header stays").text, None);
        assert_eq!(store.message(&recent).expect("read").expect("there").text.as_deref(), Some("zebra"));
        // Issue #136: the index keeps the words of a pruned text, also when the headers change.
        assert_eq!(search_ids(&store, "zebra").len(), 2, "the index keeps the text");
        let mut renamed = message(1, "Old mail, renamed");
        renamed.date = now - Duration::days(91);
        add(&store, &inbox, &renamed);
        assert_eq!(search_ids(&store, "zebra").len(), 2, "new headers keep the indexed text");
        assert_eq!(search_ids(&store, "renamed"), vec![old.clone()]);
        assert_eq!(store.prune_bodies(now).expect("prune again"), 0);
        assert_eq!(store.index_counts().expect("counts"), (2, 2));
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
            content_id: Some("a@example.org".into()),
        }];
        assert!(
            store
                .set_body(
                    &id,
                    &Body {
                        text: "t".into(),
                        from_html: false,
                        html: None,
                        attachments: attachments.clone(),
                        invite: None
                    }
                )
                .expect("body")
        );
        assert_eq!(store.message(&id).expect("read").expect("there").attachments, attachments);
        assert!(
            !store
                .set_body(
                    "m:nope",
                    &Body { text: "t".into(), from_html: false, html: None, attachments: vec![], invite: None }
                )
                .expect("body")
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

    // ----- the original depot (#267, MB.4) -----

    fn now_offset() -> DateTime<FixedOffset> {
        Utc::now().fixed_offset()
    }

    /// The original at `(path, 7, uid)` of the depot, if any.
    fn original_at(store: &MailStore, path: &str, uid: u32) -> Option<LocalOriginal> {
        let all = store.local_originals_page(0, 1000).expect("originals");
        all.into_iter().find(|original| original.folder_path == path && original.uid == uid)
    }

    #[test]
    fn original_chunks_of_a_large_original_are_read_piece_by_piece_to_the_end() {
        use std::io::{Seek, SeekFrom, Write};

        let (_tmp, _dirs, store) = open();
        // Above the threshold where the sync stops reading a mail whole, and not a multiple of
        // the piece size.
        let size = (crate::mail::sync::LARGE_ORIGINAL + 1000) as usize;
        let raw: Vec<u8> = (0..size).map(|at| (at % 251) as u8).collect();
        let mut spool = tempfile::tempfile().expect("spool");
        spool.write_all(&raw).expect("written");
        spool.seek(SeekFrom::Start(0)).expect("rewound");
        store.save_original_file("INBOX", 7, 1, now_offset(), &[], &mut spool).expect("saved");
        let seq = original_at(&store, "INBOX", 1).expect("listed").seq;
        let piece = 64 * 1024;
        let read = |offset: usize, count: usize| store.original_chunk(seq, offset as u64, count).expect("read");

        assert_eq!(read(0, piece).as_deref(), Some(&raw[..piece]));
        let middle = size / 2 + 13;
        assert_eq!(read(middle, piece).as_deref(), Some(&raw[middle..middle + piece]));
        let last = size - 1000;
        assert_eq!(read(last, piece).as_deref(), Some(&raw[last..]), "the last piece is shorter");
        assert_eq!(read(size, piece), Some(Vec::new()), "nothing at the end");
        assert_eq!(read(size + piece, 1), Some(Vec::new()), "nothing past it");
        assert_eq!(store.original_chunk(seq + 1, 0, 1).expect("asked"), None, "no such original");
        assert!(store.original_chunk(seq, 0, piece + 1).is_err(), "pieces stay bounded");
    }

    #[test]
    fn an_original_larger_than_the_depot_takes_is_refused_as_such() {
        let (_tmp, _dirs, store) = open();
        assert!(matches!(original_length(ORIGINAL_MAX_BYTES + 1), Err(MailStoreError::OriginalTooLarge)));
        assert_eq!(original_length(ORIGINAL_MAX_BYTES).expect("fits"), ORIGINAL_MAX_BYTES as i64);
        store.save_original("INBOX", 7, 1, now_offset(), &[], b"small").expect("small is kept");
        assert_eq!(store.original_size("INBOX", 7, 1).expect("asked"), Some(5));
        assert_eq!(store.original_size("INBOX", 7, 2).expect("asked"), None);
    }

    #[test]
    fn originals_are_marked_gone_wherever_their_message_leaves_and_back_when_it_returns() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let old = folder(&store, "Old", None);
        for uid in 1..=4 {
            add(&store, &inbox, &message(uid, "kept"));
            store.save_original("INBOX", 7, uid, now_offset(), &[], b"raw").expect("saved");
        }
        add(&store, &old, &message(1, "old"));
        store.save_original("Old", 7, 1, now_offset(), &[], b"raw").expect("saved");
        let gone = |path: &str, uid: u32| original_at(&store, path, uid).expect("held").gone;

        store.delete_uids(&inbox, &[1]).expect("expunged");
        assert!(gone("INBOX", 1), "expunged on the server");
        let id = store.message_id_for_uid(&inbox, 2).expect("asked").expect("there");
        store.delete_message(&id).expect("deleted");
        assert!(gone("INBOX", 2), "deleted from the suite");
        let id = store.message_id_for_uid(&inbox, 3).expect("asked").expect("there");
        store.relocate_message(&id, &archive, 7, 9).expect("moved");
        assert!(gone("INBOX", 3), "moved away");
        store.retain_folders(&["INBOX".into(), "Archive".into()]).expect("folder list");
        assert!(gone("Old", 1), "folder removed");
        assert!(!gone("INBOX", 4), "still there");

        // The server lists UID 1 again (it was only marked deleted): its original counts again.
        add(&store, &inbox, &message(1, "back"));
        assert!(!gone("INBOX", 1));

        store.note_uidvalidity(&inbox, 8).expect("renumbered");
        assert!(gone("INBOX", 4), "an old UIDVALIDITY names nothing");
    }

    #[test]
    fn gone_originals_are_deleted_after_the_retention_period_only() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        for uid in 1..=2 {
            add(&store, &inbox, &message(uid, "mail"));
            store.save_original("INBOX", 7, uid, now_offset(), &[], b"raw").expect("saved");
        }
        store.delete_uids(&inbox, &[1]).expect("gone");
        let now = Utc::now();
        assert_eq!(store.prune_gone_originals(now).expect("pruned"), 0, "not yet");
        let later = now + Duration::days(GONE_ORIGINAL_RETENTION_DAYS) + Duration::minutes(1);
        assert_eq!(store.prune_gone_originals(later).expect("pruned"), 1);
        assert!(original_at(&store, "INBOX", 1).is_none());
        assert!(original_at(&store, "INBOX", 2).is_some(), "mail on the server stays");
    }

    #[test]
    fn missing_originals_count_every_header_and_go_by_internaldate_where_known() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        // Sent on the 2nd, received on the 5th; the other has no INTERNALDATE stored (older row).
        add(&store, &inbox, &message(2, "late"));
        store.set_internal_date(&inbox, 2, day(5)).expect("internal date");
        add(&store, &inbox, &message(3, "header date"));
        store.mark_unindexable(&store.message_id_for_uid(&inbox, 3).expect("asked").expect("there")).expect("marked");
        let since = |n: u32| Some(day(n).date_naive());
        assert_eq!(store.known_missing_originals(None).expect("counted"), 2, "no text to read is still a gap");
        assert_eq!(store.known_missing_originals(since(4)).expect("counted"), 1, "received on the 5th");
        assert_eq!(store.known_missing_originals(since(6)).expect("counted"), 0);
        let estimate = store.cache_estimate(MailCacheMode::AllOffline).expect("estimate");
        assert_eq!(estimate.bodies_to_fetch, 2, "the cache job sees the same gaps");
        let work = store.cached_body_missing(None, MailCacheMode::AllOffline, 10).expect("work");
        assert_eq!(work.len(), 2);
        store.save_original("INBOX", 7, 2, now_offset(), &[], b"raw").expect("saved");
        assert_eq!(store.known_missing_originals(None).expect("counted"), 1);
    }

    #[test]
    fn folder_infos_name_delimiter_and_role_for_a_local_backup() {
        let (_tmp, _dirs, store) = open();
        store.upsert_folder("INBOX.Entw&APw-rfe", "Entwürfe", Some(Role::Drafts)).expect("folder");
        store.set_folder_delimiter("INBOX.Entw&APw-rfe", Some('.')).expect("delimiter");
        store.upsert_folder("INBOX", "INBOX", Some(Role::Inbox)).expect("folder");
        let infos = store.folder_infos().expect("infos");
        let drafts = infos.iter().find(|info| info.path == "INBOX.Entw&APw-rfe").expect("listed");
        assert_eq!(
            (drafts.name.as_str(), drafts.delimiter, drafts.role.as_deref()),
            ("Entwürfe", Some('.'), Some("drafts"))
        );
        let inbox = infos.iter().find(|info| info.path == "INBOX").expect("listed");
        assert_eq!(inbox.delimiter, None, "not listed with one yet");
    }

    // ----- server figures (DS.1, #314) -----

    /// A store of version 11 gets `messages.size`, keeps its rows, and their sizes are not known.
    #[test]
    fn version_12_adds_the_size_and_keeps_the_rows() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let id = add(&store, &inbox, &message(1, "before"));
        store
            .db
            .execute_batch("ALTER TABLE messages DROP COLUMN size; PRAGMA user_version = 11;")
            .expect("a store of version 11");
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        let version: i64 = store.db.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(store.summary(&id).expect("read").map(|summary| summary.subject).as_deref(), Some("before"));
        assert_eq!(store.uids_without_size(&inbox, 10).expect("asked"), [1], "size NULL");
        assert_eq!(store.account_stats().expect("stats").folders[0].server_bytes, None);
        assert!(store.set_size(&inbox, 1, 4096).expect("set"));
        assert!(!store.set_size(&inbox, 2, 1).expect("asked"), "no such message");
        assert!(store.uids_without_size(&inbox, 10).expect("asked").is_empty());
    }

    #[test]
    fn folder_sizes_count_only_once_every_size_is_known() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let empty = folder(&store, "Empty", None);
        for uid in 1..=3 {
            add(&store, &inbox, &message(uid, "mail"));
        }
        store.set_size(&inbox, 1, 1000).expect("set");
        store.set_size(&inbox, 3, 3000).expect("set");
        assert_eq!(store.uids_without_size(&inbox, 10).expect("asked"), [2]);
        assert_eq!(store.uids_without_size(&inbox, 0).expect("asked"), Vec::<u32>::new(), "bounded");
        let sizes = |store: &MailStore| -> Vec<(String, Option<u64>)> {
            let stats = store.account_stats().expect("stats");
            stats.folders.into_iter().map(|folder| (folder.folder.path, folder.server_bytes)).collect()
        };
        assert_eq!(sizes(&store), [("INBOX".to_string(), None), ("Empty".to_string(), Some(0))]);
        store.set_size(&inbox, 2, 2000).expect("set");
        assert_eq!(sizes(&store)[0], ("INBOX".to_string(), Some(6000)));
        assert_eq!(store.uids_without_size(&empty, 10).expect("asked"), Vec::<u32>::new());
    }

    #[test]
    fn months_are_counted_by_date_in_utc_with_their_offline_texts() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        let dated = |uid: u32, at: DateTime<Utc>| NewMessage { date: at, ..message(uid, "mail") };
        let at = |y: i32, m: u32, d: u32, h: u32| Utc.with_ymd_and_hms(y, m, d, h, 0, 0).single().expect("date");
        // 23:00 UTC on the last of October is November in Berlin: counted in October.
        let late = add(&store, &inbox, &dated(1, at(2025, 10, 31, 23)));
        add(&store, &inbox, &dated(2, at(2025, 12, 1, 0)));
        add(&store, &archive, &dated(1, at(2025, 12, 24, 12)));
        add(&store, &archive, &dated(2, at(2020, 1, 5, 8)));
        let kept = Body { text: "kept".into(), from_html: false, html: None, attachments: vec![], invite: None };
        store.set_body(&late, &kept).expect("text kept");
        let months: Vec<(i32, u32, u64, u64)> = store
            .account_stats()
            .expect("stats")
            .months
            .into_iter()
            .map(|month| (month.year, month.month, month.messages, month.cached_bodies))
            .collect();
        assert_eq!(
            months,
            [(2020, 1, 1, 0), (2025, 10, 1, 1), (2025, 12, 2, 0)],
            "only months with mail, oldest first"
        );
        // A date past the year 9999 has no month: left out, and the answer still comes.
        store.db.execute("UPDATE messages SET date = 400000000000 WHERE id = ?1", [&late]).expect("odd date");
        assert_eq!(store.account_stats().expect("stats").months.len(), 2);
    }

    #[test]
    fn originals_are_counted_per_folder_without_the_gone_ones() {
        let (_tmp, _dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        let archive = folder(&store, "Archive", Some(Role::Archive));
        for uid in 1..=3 {
            add(&store, &inbox, &message(uid, "mail"));
            store.save_original("INBOX", 7, uid, now_offset(), &[], b"12345").expect("saved");
        }
        add(&store, &archive, &message(1, "filed"));
        store.save_original("Archive", 7, 1, now_offset(), &[], b"123").expect("saved");
        // Of a folder the store does not list (renamed on the server): counts for the account.
        store.save_original("Old", 7, 1, now_offset(), &[], b"1").expect("saved");
        store.delete_uids(&inbox, &[3]).expect("expunged");
        let stats = store.account_stats().expect("stats");
        let of = |path: &str| {
            let folder = stats.folders.iter().find(|folder| folder.folder.path == path).expect("listed");
            (folder.originals, folder.original_bytes)
        };
        assert_eq!(of("INBOX"), (2, 10), "the expunged one is gone");
        assert_eq!(of("Archive"), (1, 3));
        assert_eq!((stats.originals, stats.original_bytes), (4, 14));
    }

    #[test]
    fn the_quota_is_kept_until_the_server_names_none() {
        let (_tmp, _dirs, store) = open();
        assert_eq!(store.account_stats().expect("stats").quota, None);
        let quota = MailQuota { used_bytes: 2475 * 1024, limit_bytes: 10_485_760 * 1024, checked: day(3) };
        store.set_quota(Some(&quota)).expect("kept");
        assert_eq!(store.account_stats().expect("stats").quota, Some(quota));
        store.set_quota(None).expect("removed");
        assert_eq!(store.account_stats().expect("stats").quota, None);
        store.set_quota(None).expect("removing none is fine");
    }

    /// A store of version 10 gets the columns of version 11, and its originals without a live
    /// header are taken as gone; opening it again changes nothing.
    #[test]
    fn version_11_marks_originals_of_mail_that_already_went_and_runs_once() {
        let (_tmp, dirs, store) = open();
        let inbox = folder(&store, "INBOX", Some(Role::Inbox));
        add(&store, &inbox, &message(1, "live"));
        store.save_original("INBOX", 7, 1, now_offset(), &[], b"live").expect("saved");
        store.save_original("INBOX", 7, 2, now_offset(), &[], b"moved before").expect("saved");
        store.save_original("INBOX", 6, 1, now_offset(), &[], b"older UIDVALIDITY").expect("saved");
        store
            .db
            .execute_batch(
                "DROP INDEX originals_gone;
                 ALTER TABLE originals DROP COLUMN gone_at;
                 ALTER TABLE folders DROP COLUMN delimiter;
                 ALTER TABLE messages DROP COLUMN internal_date;
                 PRAGMA user_version = 10;",
            )
            .expect("a store of version 10");
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("migrated");
        let gone: Vec<(u32, u32, bool)> = store
            .local_originals_page(0, 10)
            .expect("listed")
            .into_iter()
            .map(|original| (original.uid_validity, original.uid, original.gone))
            .collect();
        assert_eq!(gone, vec![(7, 1, false), (7, 2, true), (6, 1, true)]);
        drop(store);
        let store = MailStore::open(&dirs, ACCOUNT).expect("opened again");
        assert_eq!(store.local_originals_page(0, 10).expect("listed").len(), 3);
    }
}

#[cfg(test)]
#[path = "store_corpus.rs"]
mod corpus;
