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
//! conversation would otherwise keep a copy of it as long as the conversation lives. A result marked `untrusted`
//! leaves a note without content in its place ([`Line::Untrusted`], issue #724): the conversation keeps counting as
//! untrusted for the memory, and nothing of what was read stays on the disk.
//!
//! What a command of the conversation found (issue #786, [`Line::Seen`]) is kept as the ids of the items only,
//! never their content, beside its tool line: `assistant.show_items` shows only items found there, and the count of
//! a larger search comes from it, never from a model. It is not a result, so dropping results leaves it
//! ([`drop_results`]): a mail's result goes after a day, the ids the model may still show stay.
//!
//! Searching (issue #704, [`Conversation::find`]) reads what the core holds in memory, without an index of
//! its own: titles, messages and answers, never a result, a tool line, an approval, an error or a card.
//!
//! Errors carry the path only for the core's own log; what goes to a client says nothing of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use torro_core::ai::Effort;
use torro_ipc::assistant::{
    ConversationSummary, Entry, EntryContent, HitKind, ItemRef, MailRef, SNIPPET_CHARS, split_spoken,
};

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
    /// `assistant.archive_conversations` (issue #697), and a message sent in an archived one.
    Archived { archived: bool },
    /// What the model read of the tool call whose line is the entry `seq` (issue #701): kept for later turns
    /// ([`Conversation::results`]), never shown to a client.
    Result { seq: u64, text: String },
    /// The result of the tool call `seq` was marked `untrusted` and has been dropped (issue #724): only its number
    /// stays, so the conversation still counts as untrusted ([`Conversation::untrusted`]).
    Untrusted { seq: u64 },
    /// What the command of the tool line `seq` found (issue #786, [`Seen`]): the ids of its items, in its order, and
    /// how many it found in all. Ids only, so it stays when the result goes ([`drop_results`]).
    Seen { seq: u64, refs: Vec<ItemRef>, total: u64 },
}

/// What one command of a conversation found (issue #786, [`Line::Seen`]): written by the core after
/// `mail.search_messages`, `mail.get_message`, `calendar.list_events`, `tasks.list_todos`, `contacts.list_contacts`
/// and `contacts.get_contact` (`super::cards::group::seen`), from the core's own answer or, for engine B, from the
/// answer the agent hands back (made-up ids there do no harm: a card looks every item up with the rights of the
/// conversation's client).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The items as the command answered them, in its order; never empty.
    pub refs: Vec<ItemRef>,
    /// How many the command found in all: at least `refs.len()`, more for a search that counted beyond its page
    /// (`total_estimate` of `mail.search_messages`).
    pub total: u64,
}

/// One conversation as the core holds it in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    /// 16 hex digits ([`is_id`]); also the file's name.
    pub id: String,
    pub title: String,
    pub created: DateTime<Utc>,
    pub pinned: bool,
    /// In the archive (issue #697, [`Line::Archived`]): kept beyond the retention time.
    pub archived: bool,
    /// In order of their `seq`.
    pub entries: Vec<Entry>,
    /// The bot it is with (task B.1), fixed when it starts; `None` for the assistant.
    pub bot: Option<String>,
    /// The mail it is about (issue #605), fixed when it starts; `None` for every other conversation.
    pub about: Option<MailRef>,
    /// What the model read of each tool call, by the number of its tool line (issue #701, [`Line::Result`]).
    pub results: BTreeMap<u64, String>,
    /// Whether a result marked `untrusted` was dropped from it (issue #724, [`Line::Untrusted`]): the conversation
    /// has read what another's data said, though no result tells so any more.
    pub untrusted: bool,
    /// What each command found, by the number of its tool line (issue #786, [`Line::Seen`]).
    pub seen: BTreeMap<u64, Seen>,
}

impl Conversation {
    /// A new conversation that starts with `text` at `now`: titled with the start of its first line.
    pub fn new(id: String, text: &str, now: DateTime<Utc>) -> Conversation {
        Conversation {
            id,
            title: title_of(text),
            created: now,
            pinned: false,
            archived: false,
            entries: Vec::new(),
            bot: None,
            about: None,
            results: BTreeMap::new(),
            untrusted: false,
            seen: BTreeMap::new(),
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
            archived: false,
            entries: Vec::new(),
            bot,
            about,
            results: BTreeMap::new(),
            untrusted: false,
            seen: BTreeMap::new(),
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
            Line::Archived { archived } => self.archived = archived,
            // Only for a tool line it has, so a damaged file adds no result to an answer or a message.
            Line::Result { seq, text } if self.is_tool(seq) => {
                self.results.insert(seq, text);
            }
            Line::Result { .. } => {}
            Line::Untrusted { seq } if self.is_tool(seq) => self.untrusted = true,
            Line::Untrusted { .. } => {}
            // As a result: only for a tool line it has.
            Line::Seen { seq, refs, total } if self.is_tool(seq) && !refs.is_empty() => {
                self.seen.insert(seq, Seen { refs, total });
            }
            Line::Seen { .. } => {}
        }
    }

    /// The numbers of the results whose tool line was added before `oldest` (issue #714): those
    /// [`drop_results`] removes.
    pub fn expired_results(&self, oldest: DateTime<Utc>) -> BTreeSet<u64> {
        self.results
            .keys()
            .copied()
            .filter(|seq| {
                self.entries.binary_search_by_key(seq, |entry| entry.seq).is_ok_and(|at| self.entries[at].time < oldest)
            })
            .collect()
    }

    /// The number of the newest tool line of the command `name` (its catalogue name), `None` without one.
    pub fn newest_tool(&self, name: &str) -> Option<u64> {
        self.entries.iter().rev().find_map(|entry| match &entry.content {
            EntryContent::Tool { name: of, .. } if of == name => Some(entry.seq),
            _ => None,
        })
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
            archived: self.archived,
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

impl super::Assistant {
    /// Keeps `seen`, what the command `name` (its catalogue name) found, with its tool line in conversation `id`
    /// (issue #786, [`Line::Seen`]): the newest tool line of `name` there ([`Conversation::newest_tool`]), which
    /// the run `run` added just before. Engine A calls it after its tool line (`super::model`), engine B after the
    /// tool line of the agent's call (`super::engines`). `false` when the run was stopped or replaced meanwhile,
    /// the conversation has no such tool line, or the line could not be stored: nothing counts as found then.
    pub(super) async fn see(&self, id: &str, run: u64, name: &str, seen: Seen) -> bool {
        let mut state = self.state().await;
        if state.running.get(id).is_none_or(|running| running.id != run) {
            return false;
        }
        let Some(seq) = state.conversations.get(id).and_then(|conversation| conversation.newest_tool(name)) else {
            return false;
        };
        let line = Line::Seen { seq, refs: seen.refs, total: seen.total };
        if self.write(id, vec![line.clone()]).await.is_err() {
            return false;
        }
        if let Some(conversation) = state.conversations.get_mut(id) {
            conversation.apply(line);
        }
        true
    }
}

/// A part of a conversation that holds every word of a search ([`Conversation::find`], issue #704).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The entry's number; `None` for the title.
    pub seq: Option<u64>,
    pub kind: HitKind,
    /// The words with text around them ([`snippet`]).
    pub snippet: String,
    /// The text may come from outside the suite ([`torro_ipc::assistant::ConversationHit::untrusted`]).
    pub untrusted: bool,
    /// The commands that ran before it in the conversation and did not fail, by their names as the tool lines
    /// keep them (a server's tool as `ext_…`): what its text may tell of.
    pub after: BTreeSet<String>,
}

impl Conversation {
    /// The parts that hold every one of `words` (in small letters, none empty), in order: the messages
    /// and answers, then the title (`seq` `None`), which a search shows only when none of the others is
    /// left to show (it is mostly the start of the first message). Never a result, a tool line, an approval, an error or a card: what a tool answered can
    /// tell of an area the searcher may not read, and the other entries hold the core's words.
    ///
    /// A part is untrusted when the conversation is about a mail or with a bot, when a client sent the
    /// message, or after a command that may have read data from outside
    /// ([`super::tools::may_have_read_untrusted`]); the title when the conversation is so or its first
    /// message came from a client.
    pub fn find(&self, words: &[String]) -> Vec<Found> {
        if words.is_empty() {
            return Vec::new();
        }
        let foreign = self.about.is_some() || self.bot.is_some();
        let mut outside = false;
        let mut after = BTreeSet::new();
        let mut found = Vec::new();
        for entry in &self.entries {
            let (kind, text, sent): (_, std::borrow::Cow<'_, str>, _) = match &entry.content {
                EntryContent::Message { text, from, .. } => (HitKind::Message, text.into(), from.is_some()),
                // Searched as clients see it (issue #774): its spoken and shown part, without the mark.
                EntryContent::Answer { text, .. } => match split_spoken(text, false) {
                    (Some(spoken), shown) => (HitKind::Answer, format!("{spoken}\n\n{shown}").into(), false),
                    (None, _) => (HitKind::Answer, text.into(), false),
                },
                EntryContent::Tool { name, error: None, .. } => {
                    outside |= super::tools::may_have_read_untrusted(name);
                    after.insert(name.clone());
                    continue;
                }
                _ => continue,
            };
            if let Some(snippet) = snippet(&text, words) {
                let untrusted = foreign || sent || outside;
                found.push(Found { seq: Some(entry.seq), kind, snippet, untrusted, after: after.clone() });
            }
        }
        if let Some(snippet) = snippet(&self.title, words) {
            let first_sent = self.entries.iter().find_map(|entry| match &entry.content {
                EntryContent::Message { from, .. } => Some(from.is_some()),
                _ => None,
            });
            let untrusted = foreign || first_sent.unwrap_or(false);
            found.push(Found { seq: None, kind: HitKind::Title, snippet, untrusted, after: BTreeSet::new() });
        }
        found
    }
}

/// The words of a search (issue #704): `query` split at white space, in small letters letter by letter, as
/// [`snippet`] lowers the text (`str::to_lowercase` would end a word in `ς` where the text has `σ`).
pub fn words_of(query: &str) -> Vec<String> {
    query.split_whitespace().map(|word| word.chars().flat_map(char::to_lowercase).collect()).collect()
}

/// When `text` holds every one of `words` (in small letters): the text around the first of them, white
/// space folded to single spaces, at most [`SNIPPET_CHARS`] characters with "…" where it was cut, cut at
/// characters only. `None` when a word is missing.
pub fn snippet(text: &str, words: &[String]) -> Option<String> {
    let chars: Vec<char> = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    // The text in small letters, and for each of its characters the character of `chars` it comes from:
    // a letter may become several in small letters.
    let mut lowered = String::new();
    let mut origin = Vec::new();
    for (at, char) in chars.iter().enumerate() {
        for small in char.to_lowercase() {
            origin.push((lowered.len(), at));
            lowered.push(small);
        }
    }
    if !words.iter().all(|word| lowered.contains(word.as_str())) {
        return None;
    }
    let byte = words.first().and_then(|word| lowered.find(word.as_str())).unwrap_or(0);
    let at = origin.iter().find(|(start, _)| *start >= byte).map_or(0, |(_, at)| *at);
    if chars.len() <= SNIPPET_CHARS {
        return Some(chars.into_iter().collect());
    }
    // A little before the word, so it reads in its sentence; the window kept within the text.
    let mut start = at.saturating_sub(SNIPPET_CHARS / 4);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    start = end.saturating_sub(SNIPPET_CHARS);
    let (cut_start, cut_end) = (start > 0, end < chars.len());
    let from = start + usize::from(cut_start);
    let to = end - usize::from(cut_end);
    let middle: String = chars[from..to].iter().collect();
    let middle = middle.trim();
    Some(format!("{}{middle}{}", if cut_start { "…" } else { "" }, if cut_end { "…" } else { "" }))
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
/// every other line stays as it was, also one that does not read. A result marked `untrusted` ([`marked_untrusted`])
/// is replaced by a [`Line::Untrusted`] with its number, so only the mark is kept (issue #724). The file is written anew as
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
        match serde_json::from_slice::<Line>(line.trim_ascii_end()) {
            Ok(Line::Result { seq, text }) if seqs.contains(&seq) => {
                if marked_untrusted(&text) {
                    // A line of its own, so the next one does not stick to it.
                    serde_json::to_writer(&mut kept, &Line::Untrusted { seq })?;
                    kept.push(b'\n');
                }
            }
            _ => kept.extend_from_slice(line),
        }
    }
    let temp = temp_file(dir, id);
    let written = write_new(&temp, &kept).and_then(|()| fs::rename(&temp, &path));
    if written.is_err() {
        // What is left of it is of no use; the old file still holds everything.
        let _ = fs::remove_file(&temp);
        return written;
    }
    // The rename on the disk too; should that fail, a crash brings back the old file whole, and the next round
    // drops its results again.
    #[cfg(unix)]
    if let Err(err) = fs::File::open(dir).and_then(|folder| folder.sync_all()) {
        eprintln!("torroworkplace: cannot sync the folder of the conversation {id}: {err}");
    }
    Ok(())
}

/// Whether a result as the model read it is marked `untrusted` at its top (`tools::for_model`).
pub fn marked_untrusted(result: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(result)
        .is_ok_and(|value| value.get("untrusted").and_then(serde_json::Value::as_bool) == Some(true))
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

/// Removes the file of conversation `id` from `dir`, and what a crash in [`drop_results`] left beside it (a copy
/// of the conversation, review of #714); one that is not there is fine.
pub fn remove(dir: &Path, id: &str) -> io::Result<()> {
    for path in [file(dir, id), temp_file(dir, id)] {
        match fs::remove_file(path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    }
    Ok(())
}

/// Where [`drop_results`] writes the file of conversation `id` anew before renaming it.
fn temp_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{EXTENSION}.tmp"))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 9, minute, 0).single().expect("valid time")
    }

    /// Issue #704: a snippet holds the first word with text around it, cut at characters with "…", within
    /// [`SNIPPET_CHARS`]; small letters that grow (`İ`) shift nothing.
    #[test]
    fn a_snippet_shows_the_word_in_its_sentence() {
        let words = words_of("Flight");
        assert_eq!(snippet("no match here", &words), None);
        assert_eq!(snippet("Our  flight\nleaves", &words).as_deref(), Some("Our flight leaves"));
        let long = format!("{} the flight leaves at nine {}", "ä".repeat(200), "ö".repeat(200));
        let cut = snippet(&long, &words).expect("found");
        assert!(cut.starts_with('…') && cut.ends_with('…'), "{cut}");
        assert!(cut.contains("the flight leaves at nine"), "{cut}");
        assert!(cut.chars().count() <= SNIPPET_CHARS, "{}", cut.chars().count());
        let start = format!("flight {}", "x".repeat(300));
        let first = snippet(&start, &words).expect("found");
        assert!(first.starts_with("flight") && first.ends_with('…'), "{first}");
        let grown = format!("{} İİİ flight", "y".repeat(200));
        assert!(snippet(&grown, &words).expect("found").contains("flight"), "a letter of two in small letters");
        assert_eq!(words_of("  Lisbon\tFLIGHT "), ["lisbon", "flight"]);
        assert!(snippet("ΟΔΟΣ", &words_of("ΟΔΟΣ")).is_some(), "the same small letters for words and text");
    }

    fn answer(text: &str) -> EntryContent {
        EntryContent::Answer { text: text.into(), engine: "test".into(), model: None, spoken: None }
    }

    /// Issue #774: a hit in an answer in two parts never shows the mark between them.
    #[test]
    fn a_hit_in_an_answer_in_two_parts_leaves_the_mark_out() {
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Week", at(0));
        let entry = conversation.next(answer("Nine events.\n---shown---\nMonday standup"), at(1));
        conversation.apply(Line::Entry { entry });
        let found = conversation.find(&words_of("events standup"));
        assert_eq!(found.len(), 1);
        assert!(!found[0].snippet.contains("---shown---"), "{}", found[0].snippet);
        assert!(conversation.find(&words_of("shown")).is_empty(), "the mark is no word of the answer");
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
        let changes = [
            Line::Renamed { title: "Greeting".into() },
            Line::Pinned { pinned: true },
            Line::Archived { archived: true },
        ];
        append(&folder, &conversation.id, &changes).expect("written");
        for line in changes {
            conversation.apply(line);
        }
        assert!(conversation.summary(false).archived);

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

        // A copy a crash left while writing the file anew goes with it (review of #714).
        let temp = temp_file(&folder, &conversation.id);
        fs::write(&temp, "{}").expect("a copy left behind");
        remove(&folder, &conversation.id).expect("removed");
        assert!(!temp.exists(), "the copy goes too");
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
        assert!(!temp_file(&folder, &conversation.id).exists(), "nothing left beside it");
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

    /// Issue #724: an old result marked `untrusted` leaves a note without content, an unmarked one nothing; the
    /// conversation counts as untrusted after reading, and a note for a line that is no tool line changes nothing.
    #[test]
    fn a_dropped_untrusted_result_leaves_only_its_mark() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Hello", at(0));
        let mut lines = vec![conversation.started()];
        for (minute, text) in
            [(1, "{\"untrusted\":true,\"data\":\"the marked text\"}"), (2, "{\"data\":\"the plain text\"}")]
        {
            let tool = EntryContent::Tool {
                name: "tasks.get".into(),
                input: serde_json::json!({}),
                summary: "1 task".into(),
                error: None,
                lack: None,
            };
            let entry = conversation.next(tool, at(minute));
            let result = Line::Result { seq: entry.seq, text: text.into() };
            conversation.apply(Line::Entry { entry: entry.clone() });
            conversation.apply(result.clone());
            lines.extend([Line::Entry { entry }, result]);
        }
        assert!(!conversation.untrusted);
        append(&folder, &conversation.id, &lines).expect("written");
        drop_results(&folder, &conversation.id, &conversation.expired_results(at(10))).expect("dropped");

        let text = fs::read_to_string(file(&folder, &conversation.id)).expect("the file");
        assert!(!text.contains("the marked text") && !text.contains("the plain text"), "{text}");
        let read = load_all(&folder).expect("read");
        assert!(read[0].untrusted);
        assert!(read[0].results.is_empty());

        // A note for a message, not a tool line, is of no use.
        let message = conversation.next(answer("Hi"), at(11));
        let mut plain = Conversation::new("fedcba9876543210".into(), "Hello", at(0));
        plain.apply(Line::Entry { entry: message.clone() });
        plain.apply(Line::Untrusted { seq: message.seq });
        assert!(!plain.untrusted);
    }

    /// Issue #786: what a search found stays when its untrusted result is dropped: only ids, kept with its tool
    /// line; one for an entry that is no tool line, or with nothing found, is of no use.
    #[test]
    fn what_a_search_found_outlasts_its_dropped_result() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("assistant");
        let mut conversation = Conversation::new("0123456789abcdef".into(), "Mails from Anna", at(0));
        let tool = EntryContent::Tool {
            name: "mail.search_messages".into(),
            input: serde_json::json!({"query": "from:anna"}),
            summary: "2 mails".into(),
            error: None,
            lack: None,
        };
        let entry = conversation.next(tool, at(1));
        let refs = vec![
            ItemRef::Mail { account: "mail:anna@example.org".into(), id: "m:1".into() },
            ItemRef::Mail { account: "mail:anna@example.org".into(), id: "m:2".into() },
        ];
        let lines = vec![
            conversation.started(),
            Line::Entry { entry: entry.clone() },
            Line::Result { seq: entry.seq, text: "{\"untrusted\":true,\"data\":\"Offer Q4 from Anna\"}".into() },
            Line::Seen { seq: entry.seq, refs: refs.clone(), total: 14 },
        ];
        for line in lines.iter().skip(1).cloned() {
            conversation.apply(line);
        }
        assert_eq!(conversation.seen[&entry.seq], Seen { refs: refs.clone(), total: 14 });
        append(&folder, &conversation.id, &lines).expect("written");
        drop_results(&folder, &conversation.id, &conversation.expired_results(at(10))).expect("dropped");

        let text = fs::read_to_string(file(&folder, &conversation.id)).expect("the file");
        assert!(!text.contains("Offer Q4"), "{text}");
        let read = load_all(&folder).expect("read");
        assert!(read[0].results.is_empty() && read[0].untrusted, "the result went, its mark stayed");
        assert_eq!(read[0].seen, BTreeMap::from([(entry.seq, Seen { refs: refs.clone(), total: 14 })]));

        let mut plain = Conversation::new("fedcba9876543210".into(), "Hello", at(0));
        let message = plain.next(answer("Hi"), at(1));
        plain.apply(Line::Entry { entry: message.clone() });
        plain.apply(Line::Seen { seq: message.seq, refs, total: 2 });
        plain.apply(Line::Seen { seq: 9, refs: Vec::new(), total: 0 });
        assert!(plain.seen.is_empty(), "only beside a tool line it has");
    }

    #[test]
    fn marked_results_are_told_by_their_top() {
        assert!(marked_untrusted(&serde_json::json!({"untrusted": true, "data": {}}).to_string()));
        assert!(!marked_untrusted("{\"result\":\"todo\"}") && !marked_untrusted("not json"));
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
