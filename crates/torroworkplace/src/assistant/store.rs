//! Where the assistant keeps its conversations (task C.1): one file `<data>/assistant/<id>.jsonl`
//! per conversation, the folder 0700 and every file 0600, as the concept says ("Wo die Logik
//! liegt"). Mail the assistant read can stand in a conversation, so it is kept like the mail cache.
//!
//! A file only grows: its first line starts the conversation ([`Line::Started`]), every entry is a
//! line of its own, and a new title or pin is a line too. Reading folds the lines in order
//! ([`Conversation::fold`]), so nothing is ever rewritten in place and a crash loses at most the
//! line being written. A line that does not read (cut off by a crash, from a newer core) is
//! skipped; a file without its first line is no conversation.
//!
//! Errors carry the path only for the core's own log; what goes to a client says nothing of it.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_ipc::assistant::{ConversationSummary, Entry, EntryContent};

/// The extension of a conversation's file.
const EXTENSION: &str = "jsonl";

/// At most this many characters of the first message become the title.
const TITLE_CHARS: usize = 60;

/// One line of a conversation's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "line", rename_all = "snake_case")]
pub enum Line {
    /// The first line: when the conversation started and its first title.
    Started { created: DateTime<Utc>, title: String },
    /// One entry, as the API shows it.
    Entry { entry: Entry },
    /// `assistant.rename_conversation`.
    Renamed { title: String },
    /// `assistant.pin_conversation`.
    Pinned { pinned: bool },
}

/// One conversation as the core holds it in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    /// 16 hex digits ([`is_id`]); also the file's name.
    pub id: String,
    pub title: String,
    pub created: DateTime<Utc>,
    pub pinned: bool,
    /// In order of their `seq`.
    pub entries: Vec<Entry>,
}

impl Conversation {
    /// A new conversation that starts with `text` at `now`: titled with the start of its first line.
    pub fn new(id: String, text: &str, now: DateTime<Utc>) -> Conversation {
        Conversation { id, title: title_of(text), created: now, pinned: false, entries: Vec::new() }
    }

    /// The first line of its file.
    pub fn started(&self) -> Line {
        Line::Started { created: self.created, title: self.title.clone() }
    }

    /// The conversation the lines of the file `id` tell, `None` without [`Line::Started`] first.
    fn fold(id: String, lines: impl IntoIterator<Item = Line>) -> Option<Conversation> {
        let mut lines = lines.into_iter();
        let Some(Line::Started { created, title }) = lines.next() else { return None };
        let mut conversation = Conversation { id, title, created, pinned: false, entries: Vec::new() };
        for line in lines {
            conversation.apply(line);
        }
        Some(conversation)
    }

    /// Takes in one line after the first. An entry whose number is not past the last one is
    /// dropped: numbers are never reused, whatever a damaged file says.
    pub fn apply(&mut self, line: Line) {
        match line {
            Line::Started { .. } => {}
            Line::Entry { entry } if entry.seq > self.last() => self.entries.push(entry),
            Line::Entry { .. } => {}
            Line::Renamed { title } => self.title = title,
            Line::Pinned { pinned } => self.pinned = pinned,
        }
    }

    /// The number of the newest entry, 0 without one.
    pub fn last(&self) -> u64 {
        self.entries.last().map_or(0, |entry| entry.seq)
    }

    /// The next entry: numbered after the newest, at `now`.
    pub fn next(&self, content: EntryContent, now: DateTime<Utc>) -> Entry {
        Entry { seq: self.last() + 1, time: now, content }
    }

    /// When the newest entry was added, else when it started: the retention time counts from here.
    pub fn updated(&self) -> DateTime<Utc> {
        self.entries.last().map_or(self.created, |entry| entry.time)
    }

    /// The engine and model of the newest message; empty without one.
    fn engine(&self) -> (String, Option<String>) {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.content {
                EntryContent::Message { engine, model, .. } => Some((engine.clone(), model.clone())),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The conversation as `assistant.list_conversations` lists it.
    pub fn summary(&self, running: bool) -> ConversationSummary {
        let (engine, model) = self.engine();
        ConversationSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            engine,
            model,
            created: self.created,
            updated: self.updated(),
            pinned: self.pinned,
            running,
            last: self.last(),
        }
    }

    /// The entries numbered after `after`, all of them for `None`.
    pub fn entries_after(&self, after: Option<u64>) -> Vec<Entry> {
        let after = after.unwrap_or(0);
        self.entries.iter().filter(|entry| entry.seq > after).cloned().collect()
    }
}

/// A title from the first message: its first line that is not blank, trimmed, cut after
/// [`TITLE_CHARS`] characters with "…".
pub fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    match line.char_indices().nth(TITLE_CHARS) {
        Some((end, _)) => format!("{}…", line[..end].trim_end()),
        None => line.to_string(),
    }
}

/// The form of a conversation id the core makes: 16 hex digits in small letters. Anything else
/// is never used as a file name.
pub fn is_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// A new conversation id: 16 random hex digits.
pub fn new_id() -> Result<String, String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|err| format!("no random numbers for an id: {err}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The file of conversation `id` in `dir`.
fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{EXTENSION}"))
}

/// Every conversation in `dir`, in no order. A folder that is not there holds none; a file that
/// is no conversation (wrong name, no first line, unreadable) is left out and named on stderr.
pub fn load_all(dir: &Path) -> io::Result<Vec<Conversation>> {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut conversations = Vec::new();
    for item in listing {
        let path = item?.path();
        let id = match (path.file_stem().and_then(|s| s.to_str()), path.extension().and_then(|e| e.to_str())) {
            (Some(id), Some(EXTENSION)) if is_id(id) => id.to_string(),
            _ => continue,
        };
        match read(&path, id) {
            Ok(Some(conversation)) => conversations.push(conversation),
            Ok(None) => eprintln!("torroworkplace: {} is no conversation; left alone", path.display()),
            Err(err) => eprintln!("torroworkplace: cannot read {}: {err}", path.display()),
        }
    }
    Ok(conversations)
}

/// The conversation in the file `path`, `None` when it does not start with [`Line::Started`].
fn read(path: &Path, id: String) -> io::Result<Option<Conversation>> {
    let reader = io::BufReader::new(fs::File::open(path)?);
    let mut lines = Vec::new();
    for text in reader.lines() {
        match serde_json::from_str::<Line>(&text?) {
            Ok(line) => lines.push(line),
            // Cut off by a crash, or written by a newer core: the rest still reads.
            Err(err) => eprintln!("torroworkplace: skipped a line of {}: {err}", path.display()),
        }
    }
    Ok(Conversation::fold(id, lines))
}

/// Appends `lines` to the file of conversation `id` in `dir`, creating the folder (0700) and the
/// file (0600) when they are not there yet.
pub fn append(dir: &Path, id: &str, lines: &[Line]) -> io::Result<()> {
    crate::clients::create_private_dir(dir)?;
    let mut bytes = Vec::new();
    for line in lines {
        serde_json::to_writer(&mut bytes, line).map_err(io::Error::other)?;
        bytes.push(b'\n');
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(file(dir, id))?;
    // One write per call, so the lines of one change land together.
    file.write_all(&bytes)?;
    file.flush()
}

/// Removes the file of conversation `id` from `dir`; one that is not there is fine.
pub fn remove(dir: &Path, id: &str) -> io::Result<()> {
    match fs::remove_file(file(dir, id)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 9, minute, 0).single().expect("valid time")
    }

    fn answer(text: &str) -> EntryContent {
        EntryContent::Answer { text: text.into(), engine: "test".into(), model: None }
    }

    #[test]
    fn titles_are_the_start_of_the_first_line() {
        assert_eq!(title_of("\n  What is on tomorrow?  \nAnd after?"), "What is on tomorrow?");
        let long = "a".repeat(80);
        assert_eq!(title_of(&long), format!("{}…", "a".repeat(TITLE_CHARS)));
        assert_eq!(title_of(&"ä".repeat(TITLE_CHARS)), "ä".repeat(TITLE_CHARS), "characters, not bytes");
    }

    #[test]
    fn ids_are_sixteen_small_hex_digits() {
        let id = new_id().expect("random");
        assert!(is_id(&id), "{id}");
        assert_ne!(new_id().expect("random"), id);
        for bad in ["", "0123456789ABCDEF", "../../etc/passwd", "0123456789abcde", "0123456789abcdefa"] {
            assert!(!is_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_conversation_is_written_line_by_line_privately_and_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Hello", at(0));
        let first = conversation.next(answer("Hi"), at(1));
        conversation.apply(Line::Entry { entry: first.clone() });
        append(&folder, &conversation.id, &[conversation.started(), Line::Entry { entry: first }]).expect("written");
        append(&folder, &conversation.id, &[Line::Renamed { title: "Greeting".into() }, Line::Pinned { pinned: true }])
            .expect("written");
        conversation.apply(Line::Renamed { title: "Greeting".into() });
        conversation.apply(Line::Pinned { pinned: true });

        // A line cut off by a crash and files that are no conversation are left out.
        let path = file(&folder, &conversation.id);
        let mut cut = OpenOptions::new().append(true).open(&path).expect("open");
        cut.write_all(b"{\"line\":\"entry\",\"entr").expect("cut line");
        fs::write(folder.join("notes.txt"), "not mine").expect("other file");
        fs::write(file(&folder, "fedcba9876543210"), "{\"line\":\"pinned\",\"pinned\":true}\n").expect("no start");

        assert_eq!(load_all(&folder).expect("read"), vec![conversation.clone()]);
        assert_eq!(conversation.summary(false).last, 1);
        assert_eq!(conversation.updated(), at(1));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(&path), 0o600);
        }

        remove(&folder, &conversation.id).expect("removed");
        remove(&folder, &conversation.id).expect("removing again is fine");
        assert_eq!(load_all(&folder).expect("read"), Vec::new());
        assert_eq!(load_all(&dir.path().join("missing")).expect("no folder"), Vec::new());
    }

    #[test]
    fn numbers_are_never_reused_and_after_selects_the_newer() {
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Hello", at(0));
        for minute in 1..=3 {
            let entry = conversation.next(answer("…"), at(minute));
            conversation.apply(Line::Entry { entry });
        }
        let stale = Entry { seq: 2, time: at(4), content: EntryContent::Stopped };
        conversation.apply(Line::Entry { entry: stale });
        assert_eq!(conversation.last(), 3);
        let seqs = |after| conversation.entries_after(after).iter().map(|entry| entry.seq).collect::<Vec<_>>();
        assert_eq!(seqs(None), vec![1, 2, 3]);
        assert_eq!(seqs(Some(1)), vec![2, 3]);
        assert_eq!(seqs(Some(3)), Vec::<u64>::new());
    }
}
