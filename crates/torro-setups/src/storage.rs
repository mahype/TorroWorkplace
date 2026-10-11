//! Templates on disk.
//!
//! Task 1.11 in `docs/tasks/phase-1.md`. Reference:
//! `apps/workspace-setups/workspace_setups/storage.py`.
//!
//! Layout below the core's data directory (`torro_core::paths`, app `torroworkplace`):
//!
//! ```text
//! templates/<id>.json             one template each
//! backups/<id>/<timestamp>.json   the previous version before each overwrite, at most 5
//! ```
//!
//! Writes are atomic: a temporary file in the same directory, then rename.

use std::path::PathBuf;

use crate::{SetupsError, Template};

/// Backups kept per template.
pub const MAX_BACKUPS: usize = 5;

/// The template store at a root directory.
#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    /// The store in the core's data directory (respects `TORRO_DATA_HOME`).
    pub fn open_default() -> Result<Store, SetupsError> {
        Err(SetupsError::NotImplemented("Store::open_default (task 1.11)"))
    }

    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    /// All templates, sorted by name. Unreadable files are skipped, not fatal.
    pub fn list(&self) -> Result<Vec<Template>, SetupsError> {
        Err(SetupsError::NotImplemented("Store::list (task 1.11)"))
    }

    pub fn load(&self, id: &str) -> Result<Template, SetupsError> {
        let _ = id;
        Err(SetupsError::NotImplemented("Store::load (task 1.11)"))
    }

    /// Writes `template` under its id; an existing file goes to the backups first.
    pub fn save(&self, template: &Template) -> Result<(), SetupsError> {
        let _ = template;
        Err(SetupsError::NotImplemented("Store::save (task 1.11)"))
    }

    /// Removes the template and its backups.
    pub fn delete(&self, id: &str) -> Result<(), SetupsError> {
        let _ = id;
        Err(SetupsError::NotImplemented("Store::delete (task 1.11)"))
    }

    /// A free id for `name`: [`slug`], with `-2`, `-3`, … appended when taken.
    pub fn unique_id(&self, name: &str) -> Result<String, SetupsError> {
        let _ = name;
        Err(SetupsError::NotImplemented("Store::unique_id (task 1.11)"))
    }
}

/// Lowercase ASCII letters and digits, everything else collapsed to single `-`, German
/// umlauts and ß transliterated (`Übersicht` → `uebersicht`), `template` when empty.
pub fn slug(name: &str) -> String {
    let _ = name;
    String::new()
}
