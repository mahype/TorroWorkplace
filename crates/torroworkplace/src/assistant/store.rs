//! Where the assistant keeps its conversations (task C.1): one file `<data>/assistant/<id>.jsonl`
//! per conversation, the folder 0700 and every file 0600, as the concept says ("Wo die Logik
//! liegt"). Mail the assistant read can stand in a conversation, so it is kept like the mail cache.
//!
//! A file only grows: its first line starts the conversation ([`Line::Started`]), every entry is a
//! line of its own, and a new title or pin is a line too. Reading folds the lines in order
//! ([`Conversation::fold`]), so nothing is ever rewritten in place and a crash loses at most the
//! line being written. A line that does not read (cut off by a crash, from a newer core) is
//! skipped; a file without its first line is no conversation. The one exception is dropping old
//! results ([`drop_results`]): the file is written anew beside the old one and renamed over it, so
//! a crash leaves either file whole.
//!
//! What a tool answered the model ([`Line::Result`], issue #701) is kept beside its tool line, so a later turn
//! sends the model its calls with their results again. It never leaves the core: `assistant.get_conversation`
//! shows the entries only, as a result can hold what another area's data said. It is kept for
//! [`RESULT_HOURS`] after its call only (issue #714): a mail read then may be deleted since, and the
//! conversation would otherwise keep a copy of it as long as the conversation lives.
//!
//! Errors carry the path only for the core's own log; what goes to a client says nothing of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_core::ai::Effort;
use torro_ipc::assistant::{ConversationSummary, Entry, EntryContent, MailRef};

/// The extension of a conversation's file.
const EXTENSION: &str = "jsonl";

/// How long a tool's result stays in a conversation's file after its call (issue #714), pinned
/// conversations too; then [`drop_results`] removes it. Fixed, no setting: a day covers going on
/// with a conversation the same day, and a mail deleted since does not linger for long.
pub const RESULT_HOURS: i64 = 24;

/// At most this many characters of the first message become the title.
const TITLE_CHARS: usize = 60;

/// One line of a conversation's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "line", rename_all = "snake_case")]
pub enum Line {
    /// The first line: when the conversation started, its first title, the bot it is with
    /// (task B.1; left out for the assistant, so files from before bots read the same) and the mail
    /// it is about (issue #605; left out for every other conversation, so older files read as none).
    Started {
        created: DateTime<Utc>,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bot: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        about: Option<MailRef>,
    },
    /// One entry, as the API shows it.
    Entry { entry: Entry },
    /// `assistant.rename_conversation`.
    Renamed { title: String },
    /// `assistant.pin_conversation`.
    Pinned { pinned: bool },
    /// What the model read of the tool call whose line is the entry `seq` (issue #701): kept for later turns
    /// ([`Conversation::results`]), never shown to a client.
    Result { seq: u64, text: String },
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
    /// The bot it is with (task B.1), fixed when it starts; `None` for the assistant.
    pub bot: Option<String>,
    /// The mail it is about (issue #605), fixed when it starts; `None` for every other conversation.
    pub about: Option<MailRef>,
    /// What the model read of each tool call, by the number of its tool line (issue #701, [`Line::Result`]).
    pub results: BTreeMap<u64, String>,
}

impl Conversation {
    /// A new conversation that starts with `text` at `now`: titled with the start of its first line.
    pub fn new(id: String, text: &str, now: DateTime<Utc>) -> Conversation {
        Conversation {
            id,
            title: title_of(text),
            created: now,
            pinned: false,
            entries: Vec::new(),
            bot: None,
            about: None,
            results: BTreeMap::new(),
        }
    }

    /// The first line of its file.
    pub fn started(&self) -> Line {
        Line::Started {
            created: self.created,
            title: self.title.clone(),
            bot: self.bot.clone(),
            about: self.about.clone(),
        }
    }

    /// The conversation the lines of the file `id` tell, `None` without [`Line::Started`] first.
    fn fold(id: String, lines: impl IntoIterator<Item = Line>) -> Option<Conversation> {
        let mut lines = lines.into_iter();
        let Some(Line::Started { created, title, bot, about }) = lines.next() else { return None };
        let mut conversation = Conversation {
            id,
            title,
            created,
            pinned: false,
            entries: Vec::new(),
            bot,
            about,
            results: BTreeMap::new(),
        };
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
            // Only for a tool line it has, so a damaged file adds no result to an answer or a message.
            Line::Result { seq, text } if self.is_tool(seq) => {
                self.results.insert(seq, text);
            }
            Line::Result { .. } => {}
        }
    }

    /// The numbers of the results whose tool line was added before `oldest` (issue #714): those
    /// [`drop_results`] removes.
    pub fn expired_results(&self, oldest: DateTime<Utc>) -> BTreeSet<u64> {
        self.results
            .keys()
            .copied()
            .filter(|seq| {
                self.entries
                    .binary_search_by_key(seq, |entry| entry.seq)
                    .is_ok_and(|at| self.entries[at].time < oldest)
            })
            .collect()
    }

    /// Whether the entry `seq` is a tool line.
    fn is_tool(&self, seq: u64) -> bool {
        self.entries
            .binary_search_by_key(&seq, |entry| entry.seq)
            .is_ok_and(|at| matches!(self.entries[at].content, EntryContent::Tool { .. }))
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

    /// The engine, model and effort of the newest message; empty without one.
    fn engine(&self) -> (String, Option<String>, Option<Effort>) {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.content {
                EntryContent::Message { engine, model, effort, .. } => Some((engine.clone(), model.clone(), *effort)),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The conversation as `assistant.list_conversations` lists it; `bot_deleted` is the core's to
    /// set ([`ConversationSummary::bot_deleted`]), it knows which bots there are.
    pub fn summary(&self, running: bool) -> ConversationSummary {
        let (engine, model, effort) = self.engine();
        ConversationSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            engine,
            model,
            effort,
            created: self.created,
            updated: self.updated(),
            pinned: self.pinned,
            running,
            last: self.last(),
            bot: self.bot.clone(),
            bot_deleted: false,
            about: self.about.clone(),
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

/// Removes the results numbered `seqs` from the file of conversation `id` in `dir` (issue #714):
/// every other line stays as it was, also one that does not read. The file is written anew as
/// `<id>.jsonl.tmp` (0600) and renamed over the old one, so a crash leaves either whole. Nothing to
/// drop, or no file, writes nothing.
pub fn drop_results(dir: &Path, id: &str, seqs: &BTreeSet<u64>) -> io::Result<()> {
    if seqs.is_empty() {
        return Ok(());
    }
    let path = file(dir, id);
    let old = match fs::read(&path) {
        Ok(old) => old,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let mut kept = Vec::with_capacity(old.len());
    // By bytes and with their line ends, so a line that does not read (or is not text) stays word for word.
    for line in old.split_inclusive(|byte| *byte == b'\n') {
        let dropped = matches!(
            serde_json::from_slice::<Line>(line.trim_ascii_end()),
            Ok(Line::Result { seq, .. }) if seqs.contains(&seq)
        );
        if !dropped {
            kept.extend_from_slice(line);
        }
    }
    let temp = dir.join(format!("{id}.{EXTENSION}.tmp"));
    let written = write_new(&temp, &kept).and_then(|()| fs::rename(&temp, &path));
    if written.is_err() {
        // What is left of it is of no use; the old file still holds everything.
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Writes `bytes` to a new file `path` (0600), on the disk before it answers.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
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

    /// Task B.1: a conversation with a bot keeps the bot in its first line; one from before bots
    /// reads as a conversation with the assistant.
    #[test]
    fn a_conversation_keeps_its_bot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut with_bot = Conversation::new("0123456789abcdef".into(), "Plan Lisbon", at(0));
        with_bot.bot = Some("1a2b3c4d".into());
        append(dir.path(), &with_bot.id, &[with_bot.started()]).expect("written");
        fs::write(
            file(dir.path(), "fedcba9876543210"),
            "{\"line\":\"started\",\"created\":\"2026-10-13T09:00:00Z\",\"title\":\"Old\"}\n",
        )
        .expect("an older file");
        let mut read = load_all(dir.path()).expect("read");
        read.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(read[0], with_bot);
        assert_eq!(read[0].summary(false).bot.as_deref(), Some("1a2b3c4d"));
        assert_eq!((read[1].title.as_str(), read[1].bot.as_ref()), ("Old", None));
        let line = fs::read_to_string(file(dir.path(), &with_bot.id)).expect("the file");
        assert!(line.contains("\"bot\":\"1a2b3c4d\""), "{line}");
    }

    /// Issue #605: a conversation about a mail keeps the mail in its first line and shows it; one from
    /// before reads as about none.
    #[test]
    fn a_conversation_keeps_the_mail_it_is_about() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut about = Conversation::new("0123456789abcdef".into(), "Is there a date?", at(0));
        about.about = Some(MailRef { account: "mail:anna@example.org".into(), id: "m:4".into() });
        append(dir.path(), &about.id, &[about.started()]).expect("written");
        fs::write(
            file(dir.path(), "fedcba9876543210"),
            "{\"line\":\"started\",\"created\":\"2026-10-13T09:00:00Z\",\"title\":\"Old\"}\n",
        )
        .expect("an older file");
        let mut read = load_all(dir.path()).expect("read");
        read.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(read[0], about);
        assert_eq!(read[0].summary(false).about, about.about);
        assert_eq!((read[1].about.as_ref(), read[1].summary(false).about), (None, None));
        let line = fs::read_to_string(file(dir.path(), &about.id)).expect("the file");
        assert!(line.contains("\"about\":{\"account\":\"mail:anna@example.org\",\"id\":\"m:4\"}"), "{line}");
        assert!(!fs::read_to_string(file(dir.path(), "fedcba9876543210")).expect("old").contains("about"));
    }

    /// Issue #701: a tool line's result is kept and read back; one for an entry that is no tool line, or for a
    /// number the conversation does not have, is dropped.
    #[test]
    fn results_belong_to_tool_lines_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Hello", at(0));
        let tool = EntryContent::Tool {
            name: "mail.search_messages".into(),
            input: serde_json::json!({}),
            summary: "2 results".into(),
            error: None,
            lack: None,
        };
        let mut lines = vec![conversation.started()];
        for content in [tool, answer("Two mails.")] {
            let entry = conversation.next(content, at(1));
            conversation.apply(Line::Entry { entry: entry.clone() });
            lines.push(Line::Entry { entry });
        }
        for (seq, text) in [(1, "{\"messages\":[]}"), (2, "an answer"), (9, "nothing")] {
            let line = Line::Result { seq, text: text.into() };
            conversation.apply(line.clone());
            lines.push(line);
        }
        assert_eq!(conversation.results, BTreeMap::from([(1, "{\"messages\":[]}".to_string())]));
        append(dir.path(), &conversation.id, &lines).expect("written");
        let read = load_all(dir.path()).expect("read");
        assert_eq!(read, [conversation]);
    }

    /// Issue #714: an old result goes from the file, and only it; the entries, the newer result and a line that does
    /// not read stay, the file stays private and nothing is left beside it.
    #[test]
    fn old_results_are_dropped_from_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Hello", at(0));
        let tool = |summary: &str| EntryContent::Tool {
            name: "mail.get_message".into(),
            input: serde_json::json!({}),
            summary: summary.into(),
            error: None,
            lack: None,
        };
        let mut lines = vec![conversation.started()];
        for (minute, text) in [(1, "the old mail"), (30, "the new mail")] {
            let entry = conversation.next(tool("1 mail"), at(minute));
            let result = Line::Result { seq: entry.seq, text: text.into() };
            conversation.apply(Line::Entry { entry: entry.clone() });
            conversation.apply(result.clone());
            lines.extend([Line::Entry { entry }, result]);
        }
        append(&folder, &conversation.id, &lines[..3]).expect("written");
        let path = file(&folder, &conversation.id);
        let mut damaged = OpenOptions::new().append(true).open(&path).expect("open");
        damaged.write_all(b"{\"line\":\"from a newer core\"}\n").expect("a line that does not read");
        append(&folder, &conversation.id, &lines[3..]).expect("written");

        let expired = conversation.expired_results(at(10));
        assert_eq!(expired, BTreeSet::from([1]));
        assert_eq!(conversation.expired_results(at(1)), BTreeSet::new(), "older than, not as old as");
        drop_results(&folder, &conversation.id, &expired).expect("dropped");

        let read = load_all(&folder).expect("read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].entries, conversation.entries, "every entry stays");
        assert_eq!(read[0].results, BTreeMap::from([(2, "the new mail".to_string())]));
        let text = fs::read_to_string(&path).expect("the file");
        assert!(!text.contains("the old mail"), "{text}");
        assert!(text.contains("{\"line\":\"from a newer core\"}\n"), "{text}");
        assert!(!folder.join("0123456789abcdef.jsonl.tmp").exists(), "nothing left beside it");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).expect("metadata").permissions().mode() & 0o777, 0o600);
        }

        // Nothing to drop, or no file, writes nothing.
        drop_results(&folder, &conversation.id, &BTreeSet::new()).expect("nothing");
        assert_eq!(fs::read_to_string(&path).expect("the file"), text);
        drop_results(&folder, "fedcba9876543210", &expired).expect("no file");
        assert!(!file(&folder, "fedcba9876543210").exists());
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
