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

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use torro_core::AppId;
use torro_core::paths::AppPaths;

use crate::{SCHEMA_VERSION, SetupsError, Template};

/// Backups kept per template.
pub const MAX_BACKUPS: usize = 5;

/// Format of backup file names; fixed width, so names sort by age.
const BACKUP_STAMP: &str = "%Y%m%dT%H%M%S%.3fZ";

/// The template store at a root directory.
#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    /// The store in the core's data directory (respects `TORRO_DATA_HOME`).
    pub fn open_default() -> Result<Store, SetupsError> {
        let paths = AppPaths::for_app(AppId::WORKPLACE).ok_or(SetupsError::NoHome)?;
        Ok(Store::at(paths.data))
    }

    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    /// All templates, sorted by name. Unreadable files are skipped, not fatal.
    pub fn list(&self) -> Result<Vec<Template>, SetupsError> {
        let mut templates: Vec<Template> = json_files(&self.templates_dir())?
            .iter()
            .filter_map(|path| {
                let id = path.file_stem()?.to_str()?;
                if !is_valid_id(id) {
                    return None;
                }
                self.read_template(id).ok()
            })
            .collect();
        templates.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        Ok(templates)
    }

    pub fn load(&self, id: &str) -> Result<Template, SetupsError> {
        if !is_valid_id(id) {
            return Err(SetupsError::NotFound(id.to_string()));
        }
        let path = self.template_path(id);
        if !path.is_file() {
            return Err(SetupsError::NotFound(id.to_string()));
        }
        self.read_template(id)
    }

    /// Writes `template` under its id; an existing file goes to the backups first.
    pub fn save(&self, template: &Template) -> Result<(), SetupsError> {
        if !is_valid_id(&template.id) {
            return Err(SetupsError::Format(format!("invalid template id {:?}", template.id)));
        }
        let bytes = serde_json::to_vec_pretty(template).map_err(|err| SetupsError::Format(err.to_string()))?;
        let path = self.template_path(&template.id);
        fs::create_dir_all(self.templates_dir()).map_err(io_error)?;
        if path.is_file() {
            self.backup(&template.id, &path)?;
        }
        write_atomic(&path, &bytes).map_err(io_error)
    }

    /// Removes the template and its backups.
    pub fn delete(&self, id: &str) -> Result<(), SetupsError> {
        if !is_valid_id(id) {
            return Err(SetupsError::NotFound(id.to_string()));
        }
        fs::remove_file(self.template_path(id)).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => SetupsError::NotFound(id.to_string()),
            _ => io_error(err),
        })?;
        match fs::remove_dir_all(self.backups_dir(id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(io_error(err)),
        }
    }

    /// A free id for `name`: [`slug`], with `-2`, `-3`, … appended when taken.
    pub fn unique_id(&self, name: &str) -> Result<String, SetupsError> {
        let base = slug(name);
        if !self.template_path(&base).exists() {
            return Ok(base);
        }
        let mut counter = 2;
        loop {
            let candidate = format!("{base}-{counter}");
            if !self.template_path(&candidate).exists() {
                return Ok(candidate);
            }
            counter += 1;
        }
    }

    fn templates_dir(&self) -> PathBuf {
        self.root.join("templates")
    }

    fn backups_dir(&self, id: &str) -> PathBuf {
        self.root.join("backups").join(id)
    }

    fn template_path(&self, id: &str) -> PathBuf {
        self.templates_dir().join(format!("{id}.json"))
    }

    /// Reads and checks one template file; the id must match the file name.
    fn read_template(&self, id: &str) -> Result<Template, SetupsError> {
        let text = fs::read_to_string(self.template_path(id)).map_err(io_error)?;
        let template: Template = serde_json::from_str(&text).map_err(|err| SetupsError::Format(err.to_string()))?;
        if template.schema_version != SCHEMA_VERSION {
            return Err(SetupsError::Format(format!("unsupported schema version {}", template.schema_version)));
        }
        if template.id != id {
            return Err(SetupsError::Format(format!("id {:?} does not match file name {id:?}", template.id)));
        }
        Ok(template)
    }

    /// Copies the current file into `backups/<id>/` and keeps only the newest [`MAX_BACKUPS`].
    fn backup(&self, id: &str, current: &Path) -> Result<(), SetupsError> {
        let folder = self.backups_dir(id);
        fs::create_dir_all(&folder).map_err(io_error)?;
        let target = free_backup_path(&folder, Utc::now())?;
        fs::copy(current, &target).map_err(io_error)?;
        let mut backups = json_files(&folder)?;
        let excess = backups.len().saturating_sub(MAX_BACKUPS);
        for old in backups.drain(..excess) {
            fs::remove_file(old).map_err(io_error)?;
        }
        Ok(())
    }
}

/// Lowercase ASCII letters and digits, everything else collapsed to single `-`, German
/// umlauts and ß transliterated (`Übersicht` → `uebersicht`), `template` when empty.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.to_lowercase().chars() {
        match ch {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ => {
                if !out.ends_with('-') {
                    out.push('-');
                }
            }
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "template".to_string() } else { trimmed.to_string() }
}

/// Ids are slugs: `[a-z0-9]+(-[a-z0-9]+)*`. This also rules out path separators and `..`,
/// so an id can never point outside the templates directory.
fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// `*.json` files directly in `dir`, sorted; an absent directory is empty, not an error.
fn json_files(dir: &Path) -> Result<Vec<PathBuf>, SetupsError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io_error(err)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(io_error)?.path();
        if path.extension().is_some_and(|ext| ext == "json") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// A backup name for `now` that sorts after every existing backup. Pruning frees the names of
/// the oldest backups, so a name based on the clock alone could sort below the newest one and
/// be pruned right away; starting after the newest existing stamp rules that out. Saves within
/// the same millisecond step forward one millisecond at a time.
fn free_backup_path(folder: &Path, now: DateTime<Utc>) -> Result<PathBuf, SetupsError> {
    let newest = json_files(folder)?.iter().filter_map(|path| backup_time(path)).max();
    let mut stamp = match newest {
        Some(newest) if newest >= now => newest + TimeDelta::milliseconds(1),
        _ => now,
    };
    loop {
        let candidate = folder.join(format!("{}.json", stamp.format(BACKUP_STAMP)));
        if !candidate.exists() {
            return Ok(candidate);
        }
        stamp += TimeDelta::milliseconds(1);
    }
}

/// The time a backup file was taken, from its name; `None` for files we did not write.
fn backup_time(path: &Path) -> Option<DateTime<Utc>> {
    let stem = path.file_stem()?.to_str()?;
    NaiveDateTime::parse_from_str(stem, BACKUP_STAMP).ok().map(|time| time.and_utc())
}

/// Writes `bytes` to a new temporary file next to `path`, then renames it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn io_error(err: io::Error) -> SetupsError {
    SetupsError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_wm::Rect;

    use crate::{LayoutTree, Source, Strategy, TemplateWindow};

    /// A template with one window, named `name`, stored under `id`.
    fn sample(id: &str, name: &str) -> Template {
        let now = Utc::now();
        Template {
            schema_version: SCHEMA_VERSION,
            id: id.to_string(),
            name: name.to_string(),
            wm: "hyprland".to_string(),
            created: now,
            updated: now,
            source: Source { workspace: "3".to_string(), monitor: None, monitor_rect: None },
            tree: Some(LayoutTree::Window { id: "window-1".to_string() }),
            windows: vec![TemplateWindow {
                id: "window-1".to_string(),
                class: "foot".to_string(),
                title: "shell".to_string(),
                command: vec!["foot".to_string()],
                strategy: Strategy::New,
                floating: false,
                rect: Rect { x: 0, y: 0, width: 800, height: 600 },
            }],
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().to_path_buf());
        (dir, store)
    }

    fn backup_count(store: &Store, id: &str) -> usize {
        json_files(&store.backups_dir(id)).unwrap().len()
    }

    #[test]
    fn save_list_load_round_trip() {
        let (_dir, store) = store();
        let template = sample("entwicklung", "Entwicklung");
        store.save(&template).unwrap();

        assert_eq!(store.list().unwrap(), vec![template.clone()]);
        assert_eq!(store.load("entwicklung").unwrap(), template);
        assert_eq!(backup_count(&store, "entwicklung"), 0);
    }

    #[test]
    fn list_is_sorted_by_name_and_skips_broken_files() {
        let (_dir, store) = store();
        store.save(&sample("mail", "Mail")).unwrap();
        store.save(&sample("arbeit", "arbeit")).unwrap();
        store.save(&sample("anna", "Anna")).unwrap();
        fs::write(store.templates_dir().join("broken.json"), "{ not json").unwrap();
        fs::write(store.templates_dir().join("notes.txt"), "ignored").unwrap();

        let names: Vec<String> = store.list().unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Anna", "arbeit", "Mail"]);
    }

    #[test]
    fn list_of_missing_directory_is_empty() {
        let (_dir, store) = store();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn overwrite_creates_backup() {
        let (_dir, store) = store();
        let first = sample("setup", "Setup");
        store.save(&first).unwrap();
        let mut second = sample("setup", "Setup neu");
        second.windows.clear();
        second.tree = None;
        store.save(&second).unwrap();

        assert_eq!(store.load("setup").unwrap(), second);
        let backups = json_files(&store.backups_dir("setup")).unwrap();
        assert_eq!(backups.len(), 1);
        let backed_up: Template = serde_json::from_str(&fs::read_to_string(&backups[0]).unwrap()).unwrap();
        assert_eq!(backed_up, first);
    }

    #[test]
    fn backups_are_capped_at_five() {
        let (_dir, store) = store();
        store.save(&sample("setup", "Setup 0")).unwrap();
        for version in 1..=8 {
            store.save(&sample("setup", &format!("Setup {version}"))).unwrap();
        }

        assert_eq!(backup_count(&store, "setup"), MAX_BACKUPS);
        let newest = json_files(&store.backups_dir("setup")).unwrap().pop().unwrap();
        let newest: Template = serde_json::from_str(&fs::read_to_string(newest).unwrap()).unwrap();
        assert_eq!(newest.name, "Setup 7");
    }

    #[test]
    fn delete_removes_template_and_backups() {
        let (_dir, store) = store();
        store.save(&sample("setup", "Setup")).unwrap();
        store.save(&sample("setup", "Setup 2")).unwrap();
        assert_eq!(backup_count(&store, "setup"), 1);

        store.delete("setup").unwrap();

        assert!(!store.template_path("setup").exists());
        assert!(!store.backups_dir("setup").exists());
        assert_eq!(store.load("setup"), Err(SetupsError::NotFound("setup".into())));
        assert_eq!(store.delete("setup"), Err(SetupsError::NotFound("setup".into())));
    }

    #[test]
    fn unique_id_appends_counter() {
        let (_dir, store) = store();
        assert_eq!(store.unique_id("Workspace").unwrap(), "workspace");
        store.save(&sample("workspace", "Workspace")).unwrap();
        assert_eq!(store.unique_id("Workspace").unwrap(), "workspace-2");
        store.save(&sample("workspace-2", "Workspace")).unwrap();
        assert_eq!(store.unique_id("Workspace").unwrap(), "workspace-3");
    }

    #[test]
    fn backup_names_round_trip_their_time() {
        let time = DateTime::parse_from_rfc3339("2026-10-11T14:20:00.123Z").unwrap().with_timezone(&Utc);
        let name = format!("{}.json", time.format(BACKUP_STAMP));
        assert_eq!(name, "20261011T142000.123Z.json");
        assert_eq!(backup_time(Path::new(&name)), Some(time));
        assert_eq!(backup_time(Path::new("notes.json")), None);
    }

    #[test]
    fn slug_examples() {
        assert_eq!(slug("Entwicklung"), "entwicklung");
        assert_eq!(slug("Übersicht & Mail"), "uebersicht-mail");
        assert_eq!(slug("  "), "template");
        assert_eq!(slug("Straße Ölfeld"), "strasse-oelfeld");
        assert_eq!(slug("--Workspace 3, 11.10.--"), "workspace-3-11-10");
    }

    #[test]
    fn traversal_ids_are_not_found() {
        let (_dir, store) = store();
        for id in ["../secret", "a/b", "a\\b", "..", ".", "", "Bad", "x-"] {
            assert_eq!(store.load(id), Err(SetupsError::NotFound(id.to_string())), "load {id:?}");
            assert_eq!(store.delete(id), Err(SetupsError::NotFound(id.to_string())), "delete {id:?}");
        }
    }
}
