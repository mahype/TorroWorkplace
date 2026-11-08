//! Private workflow records and durable jobs. Unknown/corrupt databases are never repaired or overwritten.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::Error;

pub struct Store {
    path: Option<PathBuf>,
    connection: Mutex<Option<Connection>>,
}

impl Store {
    pub fn at(path: Option<PathBuf>) -> Self {
        Self { path, connection: Mutex::new(None) }
    }

    pub fn transaction<T>(&self, change: impl FnOnce(&Transaction<'_>) -> Result<T, Error>) -> Result<T, Error> {
        let mut held = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            *held = Some(self.open()?);
        }
        let connection = held.as_mut().ok_or(Error::Blocked)?;
        let transaction = connection.transaction().map_err(|_| Error::Blocked)?;
        let result = change(&transaction)?;
        transaction.commit().map_err(|_| Error::Blocked)?;
        Ok(result)
    }

    /// A missing journal reads as empty without creating a file or caching an in-memory replacement.
    pub fn view<T>(&self, query: impl FnOnce(&Transaction<'_>) -> Result<T, Error>) -> Result<T, Error> {
        let mut held = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(connection) = held.as_mut() {
            let transaction = connection.transaction().map_err(|_| Error::Blocked)?;
            return query(&transaction);
        }
        let mut connection = if let Some(path) = self.path.as_ref().filter(|path| path.exists()) {
            self.validate_existing()?;
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| Error::Blocked)?
        } else {
            let memory = Connection::open_in_memory().map_err(|_| Error::Blocked)?;
            initialize(&memory)?;
            memory
        };
        let transaction = connection.transaction().map_err(|_| Error::Blocked)?;
        query(&transaction)
    }

    fn validate_existing(&self) -> Result<(), Error> {
        let path = self.path.as_ref().ok_or(Error::Blocked)?;
        if !path.symlink_metadata().is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink()) {
            return Err(Error::Blocked);
        }
        let inspect =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| Error::Blocked)?;
        let version: i64 =
            inspect.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|_| Error::Blocked)?;
        let check: String =
            inspect.query_row("PRAGMA quick_check", [], |row| row.get(0)).map_err(|_| Error::Blocked)?;
        if version != 1 || check != "ok" {
            return Err(Error::Blocked);
        }
        inspect.prepare("SELECT kind,id,owner,account,body,state FROM records LIMIT 0").map_err(|_| Error::Blocked)?;
        Ok(())
    }

    fn open(&self) -> Result<Connection, Error> {
        let exists = self.path.as_ref().is_some_and(|path| path.exists());
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or(Error::Blocked)?;
            if path.symlink_metadata().is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
                || parent.symlink_metadata().is_ok_and(|meta| !meta.is_dir() || meta.file_type().is_symlink())
            {
                return Err(Error::Blocked);
            }
            if exists {
                self.validate_existing()?;
            } else {
                fs::create_dir_all(parent).map_err(|_| Error::Blocked)?;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options.open(path).map_err(|_| Error::Blocked)?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|_| Error::Blocked)?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| Error::Blocked)?;
            }
        }
        let connection = match &self.path {
            Some(path) => Connection::open(path),
            None => Connection::open_in_memory(),
        }
        .map_err(|_| Error::Blocked)?;
        connection.busy_timeout(std::time::Duration::from_secs(2)).map_err(|_| Error::Blocked)?;
        if !exists {
            initialize(&connection)?;
        }
        Ok(connection)
    }
}

fn initialize(connection: &Connection) -> Result<(), Error> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
        CREATE TABLE records(kind TEXT NOT NULL,id TEXT NOT NULL,owner TEXT NOT NULL,account TEXT NOT NULL,
            body TEXT NOT NULL,state TEXT NOT NULL DEFAULT '',PRIMARY KEY(kind,id));
        CREATE INDEX record_scope ON records(kind,owner,account);
        CREATE INDEX record_queue ON records(kind,state);
        PRAGMA user_version=1; COMMIT;",
        )
        .map_err(|_| Error::Blocked)
}

pub fn put<T: Serialize>(
    tx: &Transaction<'_>,
    kind: &str,
    id: &str,
    owner: &str,
    account: &str,
    state: &str,
    value: &T,
) -> Result<(), Error> {
    let body = serde_json::to_string(value).map_err(|_| Error::Invalid)?;
    tx.execute("INSERT INTO records(kind,id,owner,account,body,state) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(kind,id) DO UPDATE SET owner=excluded.owner,account=excluded.account,body=excluded.body,state=excluded.state",
        params![kind,id,owner,account,body,state]).map_err(|_| Error::Blocked)?;
    Ok(())
}

/// Filter metadata before selecting any stored content. A foreign object looks absent.
pub fn get<T: DeserializeOwned>(
    tx: &Transaction<'_>,
    kind: &str,
    id: &str,
    sees: impl Fn(&str, &str) -> bool,
) -> Result<T, Error> {
    let scope: Option<(String, String)> = tx
        .query_row("SELECT owner,account FROM records WHERE kind=?1 AND id=?2", params![kind, id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .map_err(|_| Error::Blocked)?;
    let Some((owner, account)) = scope.filter(|(owner, account)| sees(owner, account)) else {
        return Err(Error::NotFound);
    };
    let body: String = tx
        .query_row(
            "SELECT body FROM records WHERE kind=?1 AND id=?2 AND owner=?3 AND account=?4",
            params![kind, id, owner, account],
            |row| row.get(0),
        )
        .map_err(|_| Error::Blocked)?;
    serde_json::from_str(&body).map_err(|_| Error::Blocked)
}

pub fn list<T: DeserializeOwned>(
    tx: &Transaction<'_>,
    kind: &str,
    sees: impl Fn(&str, &str) -> bool,
) -> Result<Vec<T>, Error> {
    let mut statement =
        tx.prepare("SELECT id,owner,account FROM records WHERE kind=?1 ORDER BY rowid").map_err(|_| Error::Blocked)?;
    let rows = statement
        .query_map([kind], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .map_err(|_| Error::Blocked)?;
    let mut result = Vec::new();
    for row in rows {
        let (id, owner, account) = row.map_err(|_| Error::Blocked)?;
        if sees(&owner, &account) {
            result.push(get(tx, kind, &id, |_, _| true)?);
        }
    }
    Ok(result)
}

pub fn remove(tx: &Transaction<'_>, kind: &str, id: &str) -> Result<(), Error> {
    tx.execute("DELETE FROM records WHERE kind=?1 AND id=?2", params![kind, id]).map_err(|_| Error::Blocked)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_content_is_filtered_and_unknown_databases_remain_unchanged() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("workflows").join("journal.sqlite");
        let store = Store::at(Some(path.clone()));
        assert!(!path.exists(), "construction does not create a database");
        let empty: Vec<String> = store.view(|tx| list(tx, "workflow", |_, _| true)).expect("missing reads empty");
        assert!(empty.is_empty());
        assert!(!path.exists(), "a read does not create a journal or its folder");
        store.transaction(|tx| put(tx, "workflow", "wf1", "client-a", "mail:a", "", &"private")).expect("write");
        let hidden: Vec<String> =
            store.transaction(|tx| list(tx, "workflow", |owner, _| owner == "client-b")).expect("filtered");
        assert!(hidden.is_empty());
        drop(store);
        let connection = Connection::open(&path).expect("test db");
        connection.pragma_update(None, "user_version", 99).expect("future version");
        drop(connection);
        let before = fs::read(&path).expect("before");
        let future = Store::at(Some(path.clone()));
        assert_eq!(future.view(|_| Ok(())).expect_err("read blocked"), Error::Blocked);
        assert_eq!(future.transaction(|_| Ok(())).expect_err("blocked"), Error::Blocked);
        assert_eq!(fs::read(path).expect("after"), before);
    }
}
