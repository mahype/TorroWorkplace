//! The assistant's profile and memory on disk (task C.10, `docs/assistent.md` "Persönlichkeit"):
//! three files in `<config>/assistant/`, the folder 0700 and every file 0600, each written whole and
//! atomically (a temporary file renamed over it), so a crash never leaves half a file.
//!
//! - `profile.json`: name, description and picture ([`ProfileFile`]). Missing fields take the
//!   defaults ([`DEFAULT_NAME`], the description the core passes in, [`LETTER_AVATAR`]).
//! - `persona.md`: the personality as the person wrote it, so it can be read and edited like any
//!   text file. Control characters other than line breaks and tabs are taken out
//!   ([`clean_personality`]): the text goes into every prompt.
//! - `memory.md`: the notes, one line each, readable for people ([`render_notes`]):
//!   `- [1a2b3c4d · 2026-10-14T09:30:00Z · local only] Anna works in sales.` A line the person added
//!   by hand (`- Anna works in sales.`) is read as a note too, with an id from its text, and gets its
//!   full form the next time the file is written ([`parse_notes`]).
//!
//! Errors carry the path only for the core's own log; what goes to a client never names it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_ipc::assistant::{
    AVATARS, DEFAULT_NAME, LETTER_AVATAR, MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS, MAX_NOTE_CHARS, MAX_PERSONALITY_CHARS,
    Note, Profile,
};

/// The file with name, description and picture.
const PROFILE_FILE: &str = "profile.json";
/// The file with the personality.
pub const PERSONA_FILE: &str = "persona.md";
/// The file with the notes.
pub const MEMORY_FILE: &str = "memory.md";

/// What marks a note kept from remote models in `memory.md`.
const LOCAL_ONLY: &str = "local only";

/// Between the parts of a note's head in `memory.md`.
const SEPARATOR: &str = " · ";

/// The head of `memory.md`: what the file is, for a person who opens it. English like the files of
/// the suite; the notes below it are in the person's words.
const MEMORY_HEAD: &str = "# Memory of the assistant\n\nNotes the assistant keeps because you asked it to. It reads \
                           them in every conversation. Change them in Torro Workplace › Assistant › Memory.\n";

/// `profile.json` as stored: what is not there takes its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ProfileFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar: Option<String>,
}

/// A change of the profile (`assistant.update_profile`): each field given replaces the stored one.
/// The limits were checked before ([`torro_ipc::assistant::profile_refusal`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub name: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub personality: Option<String>,
}

/// The profile with no files: the default name, `description`, the letter, no personality.
pub fn default_profile(description: &str) -> Profile {
    Profile {
        name: DEFAULT_NAME.to_string(),
        description: description.to_string(),
        avatar: LETTER_AVATAR.to_string(),
        personality: String::new(),
    }
}

/// The profile in `dir`, with `description` as the default description. Files that are not there
/// hold the defaults; a value a hand broke (a name of three lines, an unknown picture) is cleaned
/// or falls back to its default, so the profile always keeps its limits.
pub fn load_profile(dir: &Path, description: &str) -> io::Result<Profile> {
    let mut profile = default_profile(description);
    if let Some(bytes) = read_if_there(&dir.join(PROFILE_FILE))? {
        let stored: ProfileFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if let Some(name) = stored.name.map(|name| clean_line(&name, MAX_NAME_CHARS)).filter(|name| !name.is_empty()) {
            profile.name = name;
        }
        if let Some(description) = stored.description {
            profile.description = clean_line(&description, MAX_DESCRIPTION_CHARS);
        }
        if let Some(avatar) = stored.avatar.filter(|avatar| AVATARS.contains(&avatar.as_str())) {
            profile.avatar = avatar;
        }
    }
    if let Some(bytes) = read_if_there(&dir.join(PERSONA_FILE))? {
        profile.personality = clean_personality(&String::from_utf8_lossy(&bytes));
    }
    Ok(profile)
}

/// `profile` with `change` applied, every new value cleaned: names and descriptions one line and
/// trimmed, the personality without control characters ([`clean_personality`]).
pub fn apply(mut profile: Profile, change: Change) -> Profile {
    if let Some(name) = change.name.map(|name| clean_line(&name, MAX_NAME_CHARS)).filter(|name| !name.is_empty()) {
        profile.name = name;
    }
    if let Some(description) = change.description {
        profile.description = clean_line(&description, MAX_DESCRIPTION_CHARS);
    }
    if let Some(avatar) = change.avatar.filter(|avatar| AVATARS.contains(&avatar.as_str())) {
        profile.avatar = avatar;
    }
    if let Some(personality) = change.personality {
        profile.personality = clean_personality(&personality);
    }
    profile
}

/// Writes `profile` to `dir`: `profile.json` and `persona.md`, creating the folder (0700).
pub fn save_profile(dir: &Path, profile: &Profile) -> io::Result<()> {
    crate::clients::create_private_dir(dir)?;
    let file = ProfileFile {
        name: Some(profile.name.clone()),
        description: Some(profile.description.clone()),
        avatar: Some(profile.avatar.clone()),
    };
    let mut json = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
    json.push(b'\n');
    write(&dir.join(PROFILE_FILE), &json)?;
    let mut persona = profile.personality.clone();
    if !persona.is_empty() {
        persona.push('\n');
    }
    write(&dir.join(PERSONA_FILE), persona.as_bytes())
}

/// The notes in `dir`, oldest first; none without `memory.md`.
pub fn load_notes(dir: &Path) -> io::Result<Vec<Note>> {
    match read_if_there(&dir.join(MEMORY_FILE))? {
        Some(bytes) => Ok(parse_notes(&String::from_utf8_lossy(&bytes), Utc::now())),
        None => Ok(Vec::new()),
    }
}

/// Writes `notes` to `dir` as `memory.md` ([`render_notes`]), creating the folder (0700).
pub fn save_notes(dir: &Path, notes: &[Note]) -> io::Result<()> {
    crate::clients::create_private_dir(dir)?;
    write(&dir.join(MEMORY_FILE), render_notes(notes).as_bytes())
}

/// `memory.md` for `notes`: the head, then one line per note.
pub fn render_notes(notes: &[Note]) -> String {
    let mut text = format!("{MEMORY_HEAD}\n");
    for note in notes {
        let mut head = vec![note.id.clone(), note.created.to_rfc3339_opts(SecondsFormat::Secs, true)];
        if note.local_only {
            head.push(LOCAL_ONLY.to_string());
        }
        text.push_str(&format!("- [{}] {}\n", head.join(SEPARATOR), clean_line(&note.text, MAX_NOTE_CHARS)));
    }
    text
}

/// The notes of a `memory.md`, in its order: every line that starts with `- `. A line in the full
/// form keeps its id, time and mark; any other (written by hand) gets an id from its text and
/// `now` as its time. Two lines with the same id keep the first; empty notes are left out.
pub fn parse_notes(text: &str, now: DateTime<Utc>) -> Vec<Note> {
    let mut notes: Vec<Note> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("- ") else { continue };
        let note = full_note(rest).unwrap_or_else(|| Note {
            id: id_of(rest),
            text: clean_line(rest, MAX_NOTE_CHARS),
            created: now,
            local_only: false,
        });
        if !note.text.is_empty() && !notes.iter().any(|known| known.id == note.id) {
            notes.push(note);
        }
    }
    notes
}

/// A note in the full form `[id · time · local only] text`; `None` for anything else.
fn full_note(rest: &str) -> Option<Note> {
    let (head, text) = rest.strip_prefix('[')?.split_once("] ")?;
    let mut parts = head.split(SEPARATOR);
    let id = parts.next().filter(|id| is_note_id(id))?.to_string();
    let created = DateTime::parse_from_rfc3339(parts.next()?).ok()?.with_timezone(&Utc);
    let local_only = parts.any(|part| part.trim() == LOCAL_ONLY);
    Some(Note { id, text: clean_line(text, MAX_NOTE_CHARS), created, local_only })
}

/// An id for a note written by hand: the first 8 hex digits of its text's SHA-256, so it stays the
/// same until the file is written again.
fn id_of(text: &str) -> String {
    let digest = Sha256::digest(text.trim().as_bytes());
    digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect()
}

/// The form of a note's id: 8 hex digits in small letters.
pub fn is_note_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// A new note id: 8 random hex digits that none of `notes` has.
pub fn new_note_id(notes: &[Note]) -> Result<String, String> {
    for _ in 0..16 {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|err| format!("no random numbers for an id: {err}"))?;
        let id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        if !notes.iter().any(|note| note.id == id) {
            return Ok(id);
        }
    }
    Err("no free id for a note".into())
}

/// `text` as one line of at most `max` characters: control characters and line breaks become
/// spaces, runs of white space one space, trimmed.
pub fn clean_line(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// The personality as it is kept and put into the prompt: line breaks as `\n`, no control
/// characters but line breaks and tabs (an escape sequence or a NUL has no business in a prompt),
/// no white space at the ends, at most [`MAX_PERSONALITY_CHARS`] characters.
pub fn clean_personality(text: &str) -> String {
    clean_text(text, MAX_PERSONALITY_CHARS)
}

/// A text of several lines as it is kept and put into a prompt (the personality, a bot's
/// instructions and skills, task B.1): line breaks as `\n`, no control characters but line breaks
/// and tabs, no white space at the ends, at most `max` characters.
pub fn clean_text(text: &str, max: usize) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let kept: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    kept.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

/// The bytes of `path`, `None` when it is not there.
fn read_if_there(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Writes `bytes` to `path` atomically, 0600 on Unix.
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::clients::write_private(path, bytes).map_err(|err| io::Error::other(err.to_string()))
}

/// The folder of the profile and the memory in a config folder: `<config>/assistant`.
pub fn folder(config: &Path) -> PathBuf {
    config.join("assistant")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 9, minute, 0).single().expect("valid time")
    }

    fn note(id: &str, text: &str, local_only: bool) -> Note {
        Note { id: id.into(), text: text.into(), created: at(30), local_only }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn a_profile_is_stored_privately_and_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = folder(dir.path());
        assert_eq!(load_profile(&folder, "Yours").expect("no files"), default_profile("Yours"));

        let change = Change {
            name: Some("  Jarvis \n".into()),
            description: Some("Your helper".into()),
            avatar: Some("sparkles".into()),
            personality: Some("Calm and brief.\r\nAsk before big changes.\u{1b}[31m\u{0}".into()),
        };
        let profile = apply(load_profile(&folder, "Yours").expect("defaults"), change);
        assert_eq!(profile.name, "Jarvis");
        assert_eq!(profile.personality, "Calm and brief.\nAsk before big changes.[31m", "no control characters");
        save_profile(&folder, &profile).expect("saved");
        assert_eq!(load_profile(&folder, "Yours").expect("read"), profile);
        let persona = fs::read_to_string(folder.join(PERSONA_FILE)).expect("persona.md");
        assert_eq!(persona, "Calm and brief.\nAsk before big changes.[31m\n", "readable as text");

        #[cfg(unix)]
        {
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(&folder.join(PROFILE_FILE)), 0o600);
            assert_eq!(mode(&folder.join(PERSONA_FILE)), 0o600);
        }

        // An unknown avatar or a blank name leaves the stored one.
        let unchanged =
            apply(profile.clone(), Change { name: Some(" ".into()), avatar: Some("bot".into()), ..Change::default() });
        assert_eq!(unchanged, profile);
    }

    #[test]
    fn a_profile_broken_by_hand_keeps_its_limits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = folder(dir.path());
        crate::clients::create_private_dir(&folder).expect("folder");
        let long = "x".repeat(MAX_NAME_CHARS + 10);
        let json = serde_json::json!({"name": format!("{long}\nsecond"), "avatar": "dragon"});
        fs::write(folder.join(PROFILE_FILE), json.to_string()).expect("written by hand");
        fs::write(folder.join(PERSONA_FILE), "a".repeat(MAX_PERSONALITY_CHARS + 50)).expect("written by hand");
        let profile = load_profile(&folder, "Yours").expect("read");
        assert_eq!(profile.name.chars().count(), MAX_NAME_CHARS);
        assert!(!profile.name.contains('\n'));
        assert_eq!(profile.avatar, LETTER_AVATAR);
        assert_eq!(profile.personality.chars().count(), MAX_PERSONALITY_CHARS);
        assert_eq!(profile.description, "Yours", "the default where nothing is stored");
    }

    #[test]
    fn notes_are_a_readable_list_with_ids() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = folder(dir.path());
        assert_eq!(load_notes(&folder).expect("no file"), Vec::new());
        let notes = vec![note("1a2b3c4d", "Anna works in sales.", false), note("5e6f7a8b", "Prefers mornings.", true)];
        save_notes(&folder, &notes).expect("saved");
        let text = fs::read_to_string(folder.join(MEMORY_FILE)).expect("memory.md");
        assert!(text.starts_with("# Memory of the assistant"), "{text}");
        assert!(text.contains("- [1a2b3c4d · 2026-10-14T09:30:00Z] Anna works in sales.\n"), "{text}");
        assert!(text.contains("- [5e6f7a8b · 2026-10-14T09:30:00Z · local only] Prefers mornings.\n"), "{text}");
        assert_eq!(load_notes(&folder).expect("read"), notes);
        #[cfg(unix)]
        assert_eq!(mode(&folder.join(MEMORY_FILE)), 0o600);
    }

    #[test]
    fn lines_written_by_hand_become_notes() {
        let text = "# Memory\n\nSome words.\n- [1a2b3c4d · 2026-10-14T09:30:00Z] Anna works in sales.\n\
                    - Bob likes tea.\n- [1a2b3c4d · 2026-10-14T09:31:00Z] A second line with the same id.\n-   \n";
        let notes = parse_notes(text, at(45));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert_eq!(notes[0], note("1a2b3c4d", "Anna works in sales.", false));
        assert_eq!(notes[1].text, "Bob likes tea.");
        assert!(is_note_id(&notes[1].id), "{}", notes[1].id);
        assert_eq!(notes[1].created, at(45));
        assert_eq!(parse_notes(text, at(50))[1].id, notes[1].id, "the same id every time");
    }

    #[test]
    fn note_ids_are_eight_small_hex_digits_and_new() {
        let id = new_note_id(&[]).expect("random");
        assert!(is_note_id(&id), "{id}");
        for bad in ["", "1A2B3C4D", "1a2b3c4", "1a2b3c4d5", "../x/../"] {
            assert!(!is_note_id(bad), "{bad}");
        }
    }

    #[test]
    fn lines_are_cleaned_and_cut() {
        assert_eq!(clean_line(" Anna\n works\tin\u{7}sales ", 100), "Anna works in sales");
        assert_eq!(clean_line(&"é".repeat(600), MAX_NOTE_CHARS).chars().count(), MAX_NOTE_CHARS);
    }
}
