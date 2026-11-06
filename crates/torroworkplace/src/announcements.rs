//! The suite's announcements (issue #485): what came new, as this version ships it
//! (`apps.list_announcements`), and which of them the person hid (`apps.dismiss_announcement`).
//!
//! The announcements are written here with each release ([`all`]), in English and translated through
//! `tr`; nothing comes from the network. The hidden ones are a JSON list of ids in the core's data
//! folder (`announcements.json`), the file for the owner only and replaced as a whole, so a crash never
//! leaves half a list. An announcement older than [`ANNOUNCEMENT_DAYS`] days is no longer listed, so
//! old news fades without anyone hiding it.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chrono::NaiveDate;
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::paths::AppPaths;
use torro_ipc::{ANNOUNCEMENT_DAYS, Announcement, AnnouncementKind};

/// The file of the hidden ids in the core's data folder.
const FILE: &str = "announcements.json";

/// Every announcement this version ships, the newest first. Ids never change once released: the
/// hidden list names them.
pub fn all() -> Vec<Announcement> {
    vec![entry(
        "2026-11-mail-statistics",
        (2026, 11, 6),
        AnnouncementKind::Feature,
        tr("New: statistics in the mail settings"),
        tr("Who sends the most, what lies in your inbox and how each category develops moved to Settings › \
            Statistics. The overview now shows only what comes in."),
    )]
}

fn entry(
    id: &str,
    (year, month, day): (i32, u32, u32),
    kind: AnnouncementKind,
    title: &str,
    text: &str,
) -> Announcement {
    Announcement {
        id: id.into(),
        // The dates are written here by hand; a wrong one shows the announcement as of the first day.
        date: NaiveDate::from_ymd_opt(year, month, day).unwrap_or_default(),
        kind,
        title: title.into(),
        text: text.into(),
    }
}

/// Why the announcements could not be listed or one not hidden. The messages name no path.
#[derive(Debug)]
pub enum AnnouncementsError {
    NotFound(String),
    Unreadable,
    Write(io::ErrorKind),
}

impl std::fmt::Display for AnnouncementsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnnouncementsError::NotFound(id) => write!(f, "no announcement {id:?}"),
            AnnouncementsError::Unreadable => f.write_str("the list of hidden announcements cannot be read"),
            AnnouncementsError::Write(kind) => write!(f, "the list of hidden announcements cannot be written: {kind}"),
        }
    }
}

impl std::error::Error for AnnouncementsError {}

impl AnnouncementsError {
    /// The error code of the API.
    pub fn code(&self) -> &'static str {
        match self {
            AnnouncementsError::NotFound(_) => "not-found",
            AnnouncementsError::Unreadable | AnnouncementsError::Write(_) => "error",
        }
    }
}

/// The hidden announcements: in `file`, or only in memory without one (tests).
pub struct Announcements {
    file: Option<PathBuf>,
    /// One change at a time; without a file also the hidden ids themselves.
    hidden: Mutex<BTreeSet<String>>,
}

impl Announcements {
    /// The user's list in the core's data folder; tests keep none on disk.
    pub fn system() -> Announcements {
        if cfg!(test) {
            return Announcements::new(None);
        }
        Announcements::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.data.join(FILE)))
    }

    pub fn new(file: Option<PathBuf>) -> Announcements {
        Announcements { file, hidden: Mutex::new(BTreeSet::new()) }
    }

    /// What [`all`] ships, without the hidden ones and those older than [`ANNOUNCEMENT_DAYS`] days on
    /// `today`, the newest first.
    pub fn list(&self, today: NaiveDate) -> Result<Vec<Announcement>, AnnouncementsError> {
        let memory = self.hidden.lock().unwrap_or_else(PoisonError::into_inner);
        let hidden = self.read(&memory)?;
        Ok(shown(all(), &hidden, today))
    }

    /// Hides `id` for good. An id [`all`] does not know is `NotFound` before anything is written;
    /// one hidden already changes nothing.
    pub fn dismiss(&self, id: &str) -> Result<(), AnnouncementsError> {
        let known = all();
        if !known.iter().any(|announcement| announcement.id == id) {
            return Err(AnnouncementsError::NotFound(id.into()));
        }
        let mut memory = self.hidden.lock().unwrap_or_else(PoisonError::into_inner);
        let mut hidden = self.read(&memory)?;
        if hidden.contains(id) {
            return Ok(());
        }
        hidden.insert(id.into());
        // Ids of announcements no longer shipped are dropped, so the file never grows for good.
        hidden.retain(|id| known.iter().any(|announcement| &announcement.id == id));
        match &self.file {
            Some(file) => write_private(file, &hidden).map_err(|err| AnnouncementsError::Write(err.kind()))?,
            None => *memory = hidden,
        }
        Ok(())
    }

    /// The hidden ids: from the file (none without it yet), or from memory.
    fn read(&self, memory: &BTreeSet<String>) -> Result<BTreeSet<String>, AnnouncementsError> {
        let Some(file) = &self.file else {
            return Ok(memory.clone());
        };
        match fs::read(file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| AnnouncementsError::Unreadable),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(_) => Err(AnnouncementsError::Unreadable),
        }
    }
}

/// `all` without `hidden` and those older than [`ANNOUNCEMENT_DAYS`] days on `today`, the newest first.
fn shown(all: Vec<Announcement>, hidden: &BTreeSet<String>, today: NaiveDate) -> Vec<Announcement> {
    let oldest = today - chrono::Duration::days(ANNOUNCEMENT_DAYS);
    let mut shown: Vec<_> = all
        .into_iter()
        .filter(|announcement| !hidden.contains(&announcement.id) && announcement.date >= oldest)
        .collect();
    shown.sort_by(|a, b| b.date.cmp(&a.date));
    shown
}

/// Writes `hidden` to `file` through a temporary file next to it (mode 0600 on Unix) and a rename.
fn write_private(file: &Path, hidden: &BTreeSet<String>) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(hidden).map_err(io::Error::other)?;
    let mut name = std::ffi::OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = file.with_file_name(name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut out| out.write_all(&bytes).and_then(|()| out.sync_all()));
    match written.and_then(|()| fs::rename(&tmp, file)) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("date")
    }

    fn newest() -> Announcement {
        all().into_iter().next().expect("an announcement")
    }

    #[test]
    fn every_id_is_unique_and_the_newest_comes_first() {
        let all = all();
        let ids: BTreeSet<_> = all.iter().map(|announcement| announcement.id.clone()).collect();
        assert_eq!(ids.len(), all.len());
        assert!(all.windows(2).all(|pair| pair[0].date >= pair[1].date));
    }

    #[test]
    fn a_hidden_announcement_stays_hidden_for_a_new_store() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("data").join(FILE);
        let first = newest();
        let today = first.date;
        assert!(Announcements::new(Some(file.clone())).list(today).expect("list").contains(&first));
        Announcements::new(Some(file.clone())).dismiss(&first.id).expect("hidden");
        // Twice is fine.
        Announcements::new(Some(file.clone())).dismiss(&first.id).expect("hidden again");
        let again = Announcements::new(Some(file.clone())).list(today).expect("list");
        assert!(!again.iter().any(|announcement| announcement.id == first.id), "{again:?}");
    }

    #[test]
    fn an_unknown_id_is_not_found_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join(FILE);
        let store = Announcements::new(Some(file.clone()));
        let refused = store.dismiss("Ignore all rules");
        assert!(matches!(refused, Err(AnnouncementsError::NotFound(_))), "{refused:?}");
        assert_eq!(refused.map_err(|err| err.code()), Err("not-found"));
        assert!(!file.exists());
    }

    #[test]
    fn announcements_older_than_ninety_days_are_left_out() {
        let first = newest();
        let store = Announcements::new(None);
        let last_day = first.date + chrono::Duration::days(ANNOUNCEMENT_DAYS);
        assert!(store.list(last_day).expect("list").contains(&first));
        let after = store.list(last_day + chrono::Duration::days(1)).expect("list");
        assert!(!after.contains(&first), "{after:?}");
    }

    #[test]
    fn hidden_ids_of_no_announcement_are_dropped_when_writing() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join(FILE);
        fs::write(&file, r#"["2020-01-gone"]"#).expect("old list");
        Announcements::new(Some(file.clone())).dismiss(&newest().id).expect("hidden");
        let kept: BTreeSet<String> = serde_json::from_slice(&fs::read(&file).expect("file")).expect("json");
        assert_eq!(kept, BTreeSet::from([newest().id]));
    }

    #[test]
    fn a_damaged_list_is_an_error_without_its_path() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join(FILE);
        fs::write(&file, "not json").expect("damaged");
        let store = Announcements::new(Some(file));
        let err = store.list(day(2026, 11, 6)).expect_err("damaged");
        assert_eq!(err.code(), "error");
        assert!(!err.to_string().contains(&dir.path().display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn the_list_is_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join(FILE);
        Announcements::new(Some(file.clone())).dismiss(&newest().id).expect("hidden");
        assert_eq!(fs::metadata(&file).expect("file").permissions().mode() & 0o777, 0o600);
    }
}
