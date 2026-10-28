//! The contacts on this device: one SQLite file `<data>/contacts/contacts.sqlite` (0600, its
//! folder 0700) for every account, so contacts work offline and searches never wait for a server.
//!
//! - **Books** with their server address, rights, whether they are hidden and the sync token.
//! - **Cards:** the vCard as this device has it, the *base* (the server's version this device last
//!   saw, for the three-way merge of `torro_contacts::merge`), `href`, `etag` and the state
//!   (`synced`, `pending`, `conflict` with the server's version beside it). A few columns are
//!   derived from the vCard for sorting and filtering; the vCard stays the truth.
//! - **The queue** of changes that did not reach the server yet (written or deleted offline), one
//!   entry per contact; it survives a restart.
//! - **Search:** an FTS5 table with the trigram tokenizer over names, company, mail addresses,
//!   numbers as written and as digits only, places and the note: any three characters find what
//!   holds them, and "0171 234" finds "+49 171 2345678" through the digits.
//! - **Favourites** and the **contacts used last** (at most [`RECENT_MAX`]), on this device only.
//!
//! `secure_delete` is on, so removed contacts do not linger in free pages; removing an account
//! truncates the write-ahead log as well ([`Store::forget_account`]). Errors never carry a name or
//! a field: they may end up in logs.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};
use torro_contacts::{Contact, ContactState, vcard};

use crate::mail::{create_private, make_private};

/// The newest layout this version writes (`PRAGMA user_version`).
const SCHEMA_VERSION: i64 = 1;

/// The most contacts "Recent" keeps per account.
pub const RECENT_MAX: usize = 50;

const SCHEMA_V1: &str = "
CREATE TABLE books (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    href TEXT NOT NULL,
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    writable INTEGER NOT NULL,
    hidden INTEGER NOT NULL,
    token TEXT,
    group_cards INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX books_account ON books(account);
CREATE TABLE cards (
    id TEXT PRIMARY KEY,
    book TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    href TEXT,
    etag TEXT,
    vcard TEXT NOT NULL,
    base TEXT,
    state TEXT NOT NULL,
    server TEXT,
    server_etag TEXT,
    family TEXT NOT NULL,
    given TEXT NOT NULL,
    shown TEXT NOT NULL,
    groups TEXT NOT NULL,
    birthday TEXT
);
CREATE INDEX cards_book ON cards(book);
CREATE INDEX cards_href ON cards(book, href);
CREATE TABLE queue (
    card TEXT PRIMARY KEY,
    book TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    op TEXT NOT NULL,
    uid TEXT NOT NULL,
    href TEXT,
    etag TEXT,
    seq INTEGER NOT NULL
);
CREATE TABLE favorites (card TEXT PRIMARY KEY, account TEXT NOT NULL);
CREATE TABLE recent (card TEXT PRIMARY KEY, account TEXT NOT NULL, used INTEGER NOT NULL);
CREATE VIRTUAL TABLE search USING fts5(card UNINDEXED, text, digits, tokenize = 'trigram');
";

/// Why the store failed. Never a name, an address or a number: the text may reach a log.
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Sql(rusqlite::Error),
    /// SQLite without FTS5 or its trigram tokenizer.
    NoFullText,
    /// A file of a newer version.
    NewerSchema(i64),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(err) => write!(f, "contacts store: {err}"),
            StoreError::Sql(err) => write!(f, "contacts store: {err}"),
            StoreError::NoFullText => f.write_str("contacts store: SQLite has no FTS5 trigram search"),
            StoreError::NewerSchema(version) => {
                write!(f, "contacts store: written by a newer version (layout {version})")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(err: io::Error) -> Self {
        StoreError::Io(err)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        StoreError::Sql(err)
    }
}

/// An address book as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRow {
    /// `<account>/<name>`.
    pub id: String,
    pub account: String,
    /// The book's address on the server.
    pub href: String,
    pub name: String,
    pub display_name: String,
    pub writable: bool,
    pub hidden: bool,
    pub token: Option<String>,
    /// The book keeps groups as cards of their own (iCloud, `X-ADDRESSBOOKSERVER-KIND:group`),
    /// which this stage reads but does not write: changing groups there is refused.
    pub group_cards: bool,
}

/// A contact as the store keeps it.
#[derive(Clone, PartialEq, Eq)]
pub struct CardRow {
    /// `<book>/<uid>`.
    pub id: String,
    pub book: String,
    pub uid: String,
    /// `None` until the server has it.
    pub href: Option<String>,
    pub etag: Option<String>,
    /// The card as this device has it.
    pub vcard: String,
    /// The server's version this device last saw; `None` for one made here.
    pub base: Option<String>,
    pub state: ContactState,
    /// With [`ContactState::Conflict`]: the server's version (`None`: the server deleted it).
    pub server: Option<String>,
    pub server_etag: Option<String>,
}

impl fmt::Debug for CardRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CardRow").field("book", &self.book).field("state", &self.state).finish_non_exhaustive()
    }
}

/// What a queued change does on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Write the card as the store has it.
    Put,
    /// Delete the card at `href`.
    Delete,
}

/// A change waiting for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub card: String,
    pub book: String,
    pub op: Op,
    pub uid: String,
    /// For a delete, where the card was.
    pub href: Option<String>,
    pub etag: Option<String>,
}

/// Which contacts a query wants; `books` is what the caller reaches, filtered before anything else.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub books: Vec<String>,
    pub group: Option<String>,
    pub favorites: bool,
    pub recent: bool,
    pub text: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

fn state_name(state: ContactState) -> &'static str {
    match state {
        ContactState::Synced => "synced",
        ContactState::Pending => "pending",
        ContactState::Conflict => "conflict",
    }
}

fn state_of(name: &str) -> ContactState {
    match name {
        "pending" => ContactState::Pending,
        "conflict" => ContactState::Conflict,
        _ => ContactState::Synced,
    }
}

fn side_file(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Only the digits of `text`.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The digits a search for a number looks for: the digits of `query` without leading zeros (a
/// number typed with its trunk prefix, "0171", finds the stored "+49 171"); `None` below three.
pub fn query_digits(query: &str) -> Option<String> {
    let digits = digits(query);
    let digits = digits.trim_start_matches('0');
    (digits.len() >= 3).then(|| digits.to_string())
}

/// `%text%` for `LIKE … ESCAPE '\'`.
fn like(text: &str) -> String {
    let mut pattern = String::from("%");
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// The text the search reads of a contact, and its numbers as digits.
fn search_text(contact: &Contact) -> (String, String) {
    let mut text = vec![contact.shown_name(), contact.name.full()];
    text.extend(contact.nickname.clone());
    text.extend(contact.org.clone());
    text.extend(contact.title.clone());
    text.extend(contact.emails.iter().map(|e| e.value.clone()));
    text.extend(contact.phones.iter().map(|p| p.value.clone()));
    text.extend(contact.addresses.iter().map(|a| a.value.one_line()));
    text.extend(contact.note.clone());
    let numbers = contact.phones.iter().map(|p| digits(&p.value)).collect::<Vec<_>>().join(" ");
    (text.join("\n"), numbers)
}

fn book_row(row: &Row<'_>) -> rusqlite::Result<BookRow> {
    Ok(BookRow {
        id: row.get(0)?,
        account: row.get(1)?,
        href: row.get(2)?,
        name: row.get(3)?,
        display_name: row.get(4)?,
        writable: row.get(5)?,
        hidden: row.get(6)?,
        token: row.get(7)?,
        group_cards: row.get(8)?,
    })
}

const BOOK_COLUMNS: &str = "id, account, href, name, display_name, writable, hidden, token, group_cards";
const CARD_COLUMNS: &str = "c.id, c.book, c.uid, c.href, c.etag, c.vcard, c.base, c.state, c.server, c.server_etag";

fn card_row(row: &Row<'_>) -> rusqlite::Result<CardRow> {
    Ok(CardRow {
        id: row.get(0)?,
        book: row.get(1)?,
        uid: row.get(2)?,
        href: row.get(3)?,
        etag: row.get(4)?,
        vcard: row.get(5)?,
        base: row.get(6)?,
        state: state_of(&row.get::<_, String>(7)?),
        server: row.get(8)?,
        server_etag: row.get(9)?,
    })
}

/// See the module docs.
#[derive(Debug)]
pub struct Store {
    db: Connection,
    path: PathBuf,
}

impl Store {
    /// Opens `<dir>/contacts.sqlite`, making the folder (0700), the file (0600) and the tables.
    pub fn open(dir: &Path) -> Result<Store, StoreError> {
        crate::clients::create_private_dir(dir)?;
        let path = dir.join("contacts.sqlite");
        // The file gets its mode before SQLite opens it; `-wal` and `-shm` take the mode of it.
        create_private(&path)?;
        let db = Connection::open(&path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let _mode: String = db.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        db.execute_batch("PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;")?;
        migrate(&db)?;
        let store = Store { db, path };
        store.restrict_files()?;
        Ok(store)
    }

    fn restrict_files(&self) -> Result<(), StoreError> {
        for file in [self.path.clone(), side_file(&self.path, "-wal"), side_file(&self.path, "-shm")] {
            if file.exists() {
                make_private(&file)?;
            }
        }
        Ok(())
    }

    // ----- books -----

    /// The books of `account`, or of every account.
    pub fn books(&self, account: Option<&str>) -> Result<Vec<BookRow>, StoreError> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {BOOK_COLUMNS} FROM books WHERE ?1 IS NULL OR account = ?1 ORDER BY account, display_name, id"
        ))?;
        let rows = statement.query_map([account], book_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn book(&self, id: &str) -> Result<Option<BookRow>, StoreError> {
        let row =
            self.db.query_row(&format!("SELECT {BOOK_COLUMNS} FROM books WHERE id = ?1"), [id], book_row).optional()?;
        Ok(row)
    }

    /// Adds a book or updates what the server says of it; its token and contacts stay.
    pub fn put_book(&self, book: &BookRow) -> Result<(), StoreError> {
        self.db.execute(
            "INSERT INTO books (id, account, href, name, display_name, writable, hidden, token)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
             ON CONFLICT(id) DO UPDATE SET href = ?3, name = ?4, display_name = ?5, writable = ?6, hidden = ?7",
            params![book.id, book.account, book.href, book.name, book.display_name, book.writable, book.hidden],
        )?;
        Ok(())
    }

    pub fn set_group_cards(&self, book: &str, group_cards: bool) -> Result<(), StoreError> {
        self.db.execute("UPDATE books SET group_cards = ?2 WHERE id = ?1", params![book, group_cards])?;
        Ok(())
    }

    pub fn set_token(&self, book: &str, token: Option<&str>) -> Result<(), StoreError> {
        self.db.execute("UPDATE books SET token = ?2 WHERE id = ?1", params![book, token])?;
        Ok(())
    }

    /// Removes a book with its contacts, queue, search entries, favourites and recents.
    pub fn remove_book(&self, id: &str) -> Result<(), StoreError> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute("DELETE FROM search WHERE card IN (SELECT id FROM cards WHERE book = ?1)", [id])?;
        tx.execute("DELETE FROM favorites WHERE card IN (SELECT id FROM cards WHERE book = ?1)", [id])?;
        tx.execute("DELETE FROM recent WHERE card IN (SELECT id FROM cards WHERE book = ?1)", [id])?;
        tx.execute("DELETE FROM queue WHERE book = ?1", [id])?;
        tx.execute("DELETE FROM cards WHERE book = ?1", [id])?;
        tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Everything of `account`: books, contacts, queue, search, favourites, recents; then the
    /// write-ahead log is truncated, so nothing of it stays in `-wal` either.
    pub fn forget_account(&self, account: &str) -> Result<(), StoreError> {
        for book in self.books(Some(account))? {
            self.remove_book(&book.id)?;
        }
        self.db.execute("DELETE FROM favorites WHERE account = ?1", [account])?;
        self.db.execute("DELETE FROM recent WHERE account = ?1", [account])?;
        self.db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// How many contacts the book holds.
    pub fn count(&self, book: &str) -> Result<u32, StoreError> {
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM cards WHERE book = ?1", [book], |row| row.get(0))?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    // ----- cards -----

    pub fn card(&self, id: &str) -> Result<Option<CardRow>, StoreError> {
        let row = self
            .db
            .query_row(&format!("SELECT {CARD_COLUMNS} FROM cards c WHERE c.id = ?1"), [id], card_row)
            .optional()?;
        Ok(row)
    }

    pub fn card_at(&self, book: &str, href: &str) -> Result<Option<CardRow>, StoreError> {
        let row = self
            .db
            .query_row(
                &format!("SELECT {CARD_COLUMNS} FROM cards c WHERE c.book = ?1 AND c.href = ?2"),
                [book, href],
                card_row,
            )
            .optional()?;
        Ok(row)
    }

    /// Every contact of a book.
    pub fn cards(&self, book: &str) -> Result<Vec<CardRow>, StoreError> {
        let mut statement =
            self.db.prepare(&format!("SELECT {CARD_COLUMNS} FROM cards c WHERE c.book = ?1 ORDER BY c.id"))?;
        let rows = statement.query_map([book], card_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Writes a contact with its search entry and the columns derived from the vCard.
    pub fn put_card(&self, card: &CardRow) -> Result<(), StoreError> {
        let contact = vcard::read(&card.vcard).unwrap_or_default();
        let (text, numbers) = search_text(&contact);
        let groups = serde_json_groups(&contact.groups);
        let birthday = contact.birthday.map(|b| b.to_string());
        let tx = self.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO cards (id, book, uid, href, etag, vcard, base, state, server, server_etag, family, given, shown,
                                groups, birthday)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET book = ?2, uid = ?3, href = ?4, etag = ?5, vcard = ?6, base = ?7, state = ?8,
                 server = ?9, server_etag = ?10, family = ?11, given = ?12, shown = ?13, groups = ?14, birthday = ?15",
            params![
                card.id,
                card.book,
                card.uid,
                card.href,
                card.etag,
                card.vcard,
                card.base,
                state_name(card.state),
                card.server,
                card.server_etag,
                contact.name.family.to_lowercase(),
                contact.name.given.to_lowercase(),
                contact.shown_name().to_lowercase(),
                groups,
                birthday,
            ],
        )?;
        tx.execute("DELETE FROM search WHERE card = ?1", [&card.id])?;
        tx.execute("INSERT INTO search (card, text, digits) VALUES (?1, ?2, ?3)", params![card.id, text, numbers])?;
        tx.commit()?;
        Ok(())
    }

    /// Removes a contact and its search entry; its favourite and recent marks go too.
    pub fn remove_card(&self, id: &str) -> Result<(), StoreError> {
        let tx = self.db.unchecked_transaction()?;
        for table in ["search", "favorites", "recent", "queue"] {
            tx.execute(&format!("DELETE FROM {table} WHERE card = ?1"), [id])?;
        }
        tx.execute("DELETE FROM cards WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    // ----- queue -----

    /// Queues a change of `card`, replacing one queued before.
    pub fn enqueue(&self, queued: &Queued) -> Result<(), StoreError> {
        let op = match queued.op {
            Op::Put => "put",
            Op::Delete => "delete",
        };
        self.db.execute(
            "INSERT INTO queue (card, book, op, uid, href, etag, seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(seq), 0) + 1 FROM queue))
             ON CONFLICT(card) DO UPDATE SET book = ?2, op = ?3, uid = ?4, href = ?5, etag = ?6",
            params![queued.card, queued.book, op, queued.uid, queued.href, queued.etag],
        )?;
        Ok(())
    }

    pub fn dequeue(&self, card: &str) -> Result<(), StoreError> {
        self.db.execute("DELETE FROM queue WHERE card = ?1", [card])?;
        Ok(())
    }

    /// The changes waiting for `book`, oldest first.
    pub fn queued(&self, book: &str) -> Result<Vec<Queued>, StoreError> {
        let mut statement =
            self.db.prepare("SELECT card, book, op, uid, href, etag FROM queue WHERE book = ?1 ORDER BY seq")?;
        let rows = statement
            .query_map([book], |row| {
                Ok(Queued {
                    card: row.get(0)?,
                    book: row.get(1)?,
                    op: if row.get::<_, String>(2)? == "delete" { Op::Delete } else { Op::Put },
                    uid: row.get(3)?,
                    href: row.get(4)?,
                    etag: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ----- favourites and recents -----

    pub fn set_favorite(&self, card: &str, account: &str, favorite: bool) -> Result<(), StoreError> {
        if favorite {
            self.db.execute("INSERT OR IGNORE INTO favorites (card, account) VALUES (?1, ?2)", [card, account])?;
        } else {
            self.db.execute("DELETE FROM favorites WHERE card = ?1", [card])?;
        }
        Ok(())
    }

    pub fn is_favorite(&self, card: &str) -> Result<bool, StoreError> {
        Ok(self.db.query_row("SELECT 1 FROM favorites WHERE card = ?1", [card], |_| Ok(())).optional()?.is_some())
    }

    /// Notes `card` as used at `now` (milliseconds); keeps the last [`RECENT_MAX`] of its account.
    pub fn mark_used(&self, card: &str, account: &str, now: i64) -> Result<(), StoreError> {
        self.db.execute(
            "INSERT INTO recent (card, account, used) VALUES (?1, ?2, ?3)
             ON CONFLICT(card) DO UPDATE SET used = ?3",
            params![card, account, now],
        )?;
        self.db.execute(
            "DELETE FROM recent WHERE account = ?1 AND card NOT IN
                 (SELECT card FROM recent WHERE account = ?1 ORDER BY used DESC LIMIT ?2)",
            params![account, RECENT_MAX as i64],
        )?;
        Ok(())
    }

    // ----- reading -----

    /// The contacts `query` asks for, filtered to its books before the search, the group and the
    /// paging; and whether there are more.
    pub fn query(&self, query: &Query) -> Result<(Vec<CardRow>, bool), StoreError> {
        let books = serde_json::to_string(&query.books).unwrap_or_else(|_| "[]".into());
        let mut sql = format!(
            "SELECT {CARD_COLUMNS} FROM cards c
             LEFT JOIN recent r ON r.card = c.id
             WHERE c.book IN (SELECT value FROM json_each(?1))"
        );
        let mut values: Vec<rusqlite::types::Value> = vec![books.into()];
        if let Some(group) = &query.group {
            values.push(group.clone().into());
            sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM json_each(c.groups) WHERE value = ?{})", values.len()));
        }
        if query.favorites {
            sql.push_str(" AND c.id IN (SELECT card FROM favorites)");
        }
        if query.recent {
            sql.push_str(" AND r.card IS NOT NULL");
        }
        if let Some(text) = query.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let mut words = Vec::new();
            for word in text.split_whitespace() {
                values.push(like(word).into());
                words.push(format!("s.text LIKE ?{} ESCAPE '\\'", values.len()));
            }
            let mut matched = format!("({})", words.join(" AND "));
            if let Some(number) = query_digits(text) {
                values.push(like(&number).into());
                matched = format!("({matched} OR s.digits LIKE ?{} ESCAPE '\\')", values.len());
            }
            sql.push_str(&format!(" AND c.id IN (SELECT s.card FROM search s WHERE {matched})"));
        }
        sql.push_str(if query.recent {
            " ORDER BY r.used DESC, c.id"
        } else {
            " ORDER BY c.family = '', c.family, c.given, c.shown, c.id"
        });
        values.push((query.limit as i64 + 1).into());
        sql.push_str(&format!(" LIMIT ?{}", values.len()));
        values.push((query.offset as i64).into());
        sql.push_str(&format!(" OFFSET ?{}", values.len()));
        let mut statement = self.db.prepare(&sql)?;
        let mut rows =
            statement.query_map(rusqlite::params_from_iter(values), card_row)?.collect::<Result<Vec<_>, _>>()?;
        let more = rows.len() > query.limit;
        rows.truncate(query.limit);
        Ok((rows, more))
    }

    /// The groups of `books` with how many contacts each holds: `(book, group, count)`.
    pub fn groups(&self, books: &[String]) -> Result<Vec<(String, String, u32)>, StoreError> {
        let books = serde_json::to_string(books).unwrap_or_else(|_| "[]".into());
        let mut statement = self.db.prepare(
            "SELECT c.book, g.value, COUNT(*) FROM cards c, json_each(c.groups) g
             WHERE c.book IN (SELECT value FROM json_each(?1))
             GROUP BY c.book, g.value ORDER BY c.book, g.value",
        )?;
        let rows = statement
            .query_map([books], |row| {
                Ok((row.get(0)?, row.get(1)?, u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0)))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The contacts of `books` with a birthday.
    pub fn with_birthday(&self, books: &[String]) -> Result<Vec<CardRow>, StoreError> {
        let books = serde_json::to_string(books).unwrap_or_else(|_| "[]".into());
        let mut statement = self.db.prepare(&format!(
            "SELECT {CARD_COLUMNS} FROM cards c
             WHERE c.birthday IS NOT NULL AND c.book IN (SELECT value FROM json_each(?1))"
        ))?;
        let rows = statement.query_map([books], card_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn serde_json_groups(groups: &[String]) -> String {
    serde_json::to_string(groups).unwrap_or_else(|_| "[]".into())
}

fn migrate(db: &Connection) -> Result<(), StoreError> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema(version));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    if let Err(err) = tx.execute_batch(SCHEMA_V1) {
        let text = err.to_string();
        return Err(if text.contains("fts5") || text.contains("trigram") {
            StoreError::NoFullText
        } else {
            err.into()
        });
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn book(account: &str, name: &str) -> BookRow {
        BookRow {
            id: format!("{account}/{name}"),
            account: account.into(),
            href: format!("https://cloud.example.org/remote.php/dav/addressbooks/users/anna/{name}/"),
            name: name.into(),
            display_name: name.into(),
            writable: true,
            hidden: false,
            token: None,
            group_cards: false,
        }
    }

    fn card(book: &str, uid: &str, vcard: &str) -> CardRow {
        CardRow {
            id: format!("{book}/{uid}"),
            book: book.into(),
            uid: uid.into(),
            href: Some(format!("{uid}.vcf")),
            etag: Some("\"1\"".into()),
            vcard: vcard.into(),
            base: Some(vcard.into()),
            state: ContactState::Synced,
            server: None,
            server_etag: None,
        }
    }

    fn vcard(name: &str, tel: &str, extra: &str) -> String {
        format!("BEGIN:VCARD\r\nVERSION:3.0\r\nUID:x\r\nFN:{name}\r\nN:{name};;;;\r\nTEL:{tel}\r\n{extra}END:VCARD\r\n")
    }

    #[test]
    fn the_file_is_private_and_searches_find_numbers_by_digits() {
        let dir = tempfile::tempdir().expect("dir");
        let store = Store::open(&dir.path().join("contacts")).expect("store");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode(&dir.path().join("contacts")), 0o700);
            assert_eq!(mode(&dir.path().join("contacts/contacts.sqlite")), 0o600);
        }
        let work = book("nextcloud:anna@cloud.example.org", "contacts");
        let other = book("nextcloud:anna@cloud.example.org", "private");
        store.put_book(&work).expect("book");
        store.put_book(&other).expect("book");
        store
            .put_card(&card(&work.id, "a", &vcard("Berg", "+49 171 2345678", "CATEGORIES:Friends\r\n")))
            .expect("card");
        store.put_card(&card(&work.id, "b", &vcard("Keller", "+41 79 1234567", ""))).expect("card");
        store.put_card(&card(&other.id, "c", &vcard("Secret", "+49 171 2345679", ""))).expect("card");
        let find = |text: &str, books: &[&BookRow]| {
            let query = Query {
                books: books.iter().map(|b| b.id.clone()).collect(),
                text: Some(text.into()),
                limit: 10,
                ..Query::default()
            };
            store.query(&query).expect("query").0.into_iter().map(|c| c.uid).collect::<Vec<_>>()
        };
        assert_eq!(find("0171 234", &[&work]), ["a"], "digits without the trunk zero");
        assert_eq!(find("0171 234", &[&work, &other]), ["a", "c"]);
        assert_eq!(find("kell", &[&work]), ["b"]);
        assert_eq!(find("secret", &[&work]), Vec::<String>::new(), "a book out of reach is filtered first");
        assert_eq!(find("50%_", &[&work]), Vec::<String>::new(), "wildcards are text");
        let groups = store.groups(&[work.id.clone(), other.id.clone()]).expect("groups");
        assert_eq!(groups, [(work.id.clone(), "Friends".to_string(), 1)]);
        let page = Query { books: vec![work.id.clone()], limit: 1, ..Query::default() };
        let (first, more) = store.query(&page).expect("page");
        assert_eq!((first[0].uid.as_str(), more), ("a", true));
        let (second, more) = store.query(&Query { offset: 1, ..page }).expect("page");
        assert_eq!((second[0].uid.as_str(), more), ("b", false));
    }

    #[test]
    fn the_queue_survives_a_restart_and_forgetting_an_account_empties_everything() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("contacts");
        let account = "nextcloud:anna@cloud.example.org";
        {
            let store = Store::open(&path).expect("store");
            let work = book(account, "contacts");
            store.put_book(&work).expect("book");
            let row = card(&work.id, "a", &vcard("Berg", "1", ""));
            store.put_card(&row).expect("card");
            store
                .enqueue(&Queued {
                    card: row.id.clone(),
                    book: work.id.clone(),
                    op: Op::Put,
                    uid: "a".into(),
                    href: None,
                    etag: None,
                })
                .expect("queued");
            store.set_favorite(&row.id, account, true).expect("favourite");
            store.mark_used(&row.id, account, 1).expect("used");
        }
        let store = Store::open(&path).expect("again");
        let work = format!("{account}/contacts");
        assert_eq!(store.queued(&work).expect("queue").len(), 1, "the queue survives");
        store.forget_account(account).expect("forgotten");
        assert!(store.books(None).expect("books").is_empty());
        assert!(store.card(&format!("{work}/a")).expect("card").is_none());
        assert!(store.queued(&work).expect("queue").is_empty());
        assert!(!store.is_favorite(&format!("{work}/a")).expect("favourite"));
        let left: i64 = store.db.query_row("SELECT COUNT(*) FROM search", [], |r| r.get(0)).expect("count");
        assert_eq!(left, 0);
    }

    #[test]
    fn recent_keeps_the_last_fifty() {
        let dir = tempfile::tempdir().expect("dir");
        let store = Store::open(dir.path()).expect("store");
        let account = "nextcloud:anna@cloud.example.org";
        let work = book(account, "contacts");
        store.put_book(&work).expect("book");
        for n in 0..60 {
            let row = card(&work.id, &format!("u{n:02}"), &vcard("X", "1", ""));
            store.put_card(&row).expect("card");
            store.mark_used(&row.id, account, n).expect("used");
        }
        let query = Query { books: vec![work.id.clone()], recent: true, limit: 100, ..Query::default() };
        let (recent, _) = store.query(&query).expect("recent");
        assert_eq!(recent.len(), RECENT_MAX);
        assert_eq!(recent[0].uid, "u59", "the latest first");
    }
}
