//! Where the bots live on disk (task B.1, "Festlegungen für alle B-Aufgaben" in
//! `docs/tasks/assistent.md`): one folder per bot, `<config>/assistant/bots/<id>/`, the folders 0700
//! and every file 0600, each file written whole and atomically:
//!
//! - `bot.json`: name, description, picture, engine, model, the id of the bot's client and the
//!   names of its skills in order ([`BotFile`]),
//! - `persona.md`: the personality, `instructions.md`: the instructions, as the person wrote them,
//! - `skills/<n>.md`: the text of the n-th skill (from 1), plain text.
//!
//! The texts are files of their own so a person can read and edit them like any text. `bot.json` is
//! written last: a bot whose save broke off reads with the texts written so far. A text file that
//! is missing reads as empty; a folder without a readable `bot.json` is no bot and is left alone.
//!
//! Errors carry the path only for the core's own log; what goes to a client never names it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torro_ipc::bots::{Bot, Skill};

/// The file with everything but the texts.
const BOT_FILE: &str = "bot.json";
/// The file with the personality.
const PERSONA_FILE: &str = "persona.md";
/// The file with the instructions.
const INSTRUCTIONS_FILE: &str = "instructions.md";
/// The folder with the skills' texts.
const SKILLS_FOLDER: &str = "skills";

/// What `bot.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BotFile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    avatar: String,
    engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    client: String,
    /// The skills' names in order; the n-th text is `skills/<n>.md`.
    #[serde(default)]
    skills: Vec<String>,
}

/// The form of a bot id the core makes: 8 hex digits in small letters. Anything else is never used
/// as a folder name.
pub fn is_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// A new bot id: 8 random hex digits none of `taken` has.
pub fn new_id(taken: &[String]) -> Result<String, String> {
    loop {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|err| format!("no random numbers for an id: {err}"))?;
        let id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        if !taken.contains(&id) {
            return Ok(id);
        }
    }
}

/// The folder of bot `id` in `dir`.
fn folder(dir: &Path, id: &str) -> PathBuf {
    dir.join(id)
}

/// The ids of every bot in `dir`: folders with a bot's id for a name. None when `dir` is not there.
pub fn ids(dir: &Path) -> io::Result<Vec<String>> {
    let listing = match fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut ids = Vec::new();
    for item in listing {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if is_id(&name) && item.file_type()?.is_dir() {
            ids.push(name);
        }
    }
    Ok(ids)
}

/// Every bot in `dir`, in no order; a folder that holds no bot is named on stderr and left out.
pub fn load_all(dir: &Path) -> io::Result<Vec<Bot>> {
    let mut bots = Vec::new();
    for id in ids(dir)? {
        match load(dir, &id) {
            Ok(Some(bot)) => bots.push(bot),
            Ok(None) => {}
            Err(err) => eprintln!("torroworkplace: cannot read the bot in {}: {err}", folder(dir, &id).display()),
        }
    }
    Ok(bots)
}

/// The bot `id` in `dir`; `None` when there is none (no folder, or no `bot.json`) or `id` is no
/// bot id.
pub fn load(dir: &Path, id: &str) -> io::Result<Option<Bot>> {
    if !is_id(id) {
        return Ok(None);
    }
    let folder = folder(dir, id);
    let Some(json) = read_if_there(&folder.join(BOT_FILE))? else { return Ok(None) };
    let file: BotFile = serde_json::from_slice(&json).map_err(io::Error::other)?;
    let text = |path: PathBuf| -> io::Result<String> {
        Ok(read_if_there(&path)?
            .map(|bytes| String::from_utf8_lossy(&bytes).trim_end().to_string())
            .unwrap_or_default())
    };
    let mut skills = Vec::new();
    for (index, name) in file.skills.into_iter().enumerate() {
        skills.push(Skill { name, text: text(skill_file(&folder, index))? });
    }
    Ok(Some(Bot {
        id: id.to_string(),
        name: file.name,
        description: file.description,
        avatar: file.avatar,
        engine: file.engine,
        model: file.model,
        personality: text(folder.join(PERSONA_FILE))?,
        instructions: text(folder.join(INSTRUCTIONS_FILE))?,
        skills,
        client: file.client,
    }))
}

/// Writes `bot` to its folder in `dir` (0700, its files 0600): the texts first, `bot.json` last,
/// and the files of skills it no longer has removed.
pub fn save(dir: &Path, bot: &Bot) -> io::Result<()> {
    if !is_id(&bot.id) {
        return Err(io::Error::other(format!("{:?} is no bot id", bot.id)));
    }
    let folder = folder(dir, &bot.id);
    let skills = folder.join(SKILLS_FOLDER);
    crate::clients::create_private_dir(&skills)?;
    write(&folder.join(PERSONA_FILE), &with_end(&bot.personality))?;
    write(&folder.join(INSTRUCTIONS_FILE), &with_end(&bot.instructions))?;
    for (index, skill) in bot.skills.iter().enumerate() {
        write(&skill_file(&folder, index), &with_end(&skill.text))?;
    }
    for item in fs::read_dir(&skills)? {
        let path = item?.path();
        let kept = (0..bot.skills.len()).any(|index| skill_file(&folder, index) == path);
        if !kept {
            fs::remove_file(&path)?;
        }
    }
    let file = BotFile {
        name: bot.name.clone(),
        description: bot.description.clone(),
        avatar: bot.avatar.clone(),
        engine: bot.engine.clone(),
        model: bot.model.clone(),
        client: bot.client.clone(),
        skills: bot.skills.iter().map(|skill| skill.name.clone()).collect(),
    };
    let mut json = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
    json.push(b'\n');
    write(&folder.join(BOT_FILE), &json)
}

/// Removes the folder of bot `id` from `dir` with everything in it; one that is not there is fine.
pub fn remove(dir: &Path, id: &str) -> io::Result<()> {
    if !is_id(id) {
        return Ok(());
    }
    match fs::remove_dir_all(folder(dir, id)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// The file of the skill at `index` (from 0) in a bot's `folder`: `skills/<index + 1>.md`.
fn skill_file(folder: &Path, index: usize) -> PathBuf {
    folder.join(SKILLS_FOLDER).join(format!("{}.md", index + 1))
}

/// `text` as a file holds it: with a line break at the end unless it is empty.
fn with_end(text: &str) -> Vec<u8> {
    match text.is_empty() {
        true => Vec::new(),
        false => format!("{text}\n").into_bytes(),
    }
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn bot(id: &str, skills: usize) -> Bot {
        Bot {
            id: id.into(),
            name: "Travel planner".into(),
            description: "Plans trips by train.".into(),
            avatar: "sun".into(),
            engine: "ai:ollama@localhost:11434".into(),
            model: Some("qwen3:8b".into()),
            personality: "Calm and brief.\nNever in a hurry.".into(),
            instructions: "Suggest trains before flights.".into(),
            skills: (1..=skills).map(|n| Skill { name: format!("Skill {n}"), text: format!("Text {n}") }).collect(),
            client: "9f8e7d6c".into(),
        }
    }

    #[test]
    fn ids_are_eight_small_hex_digits() {
        let id = new_id(&[]).expect("random");
        assert!(is_id(&id), "{id}");
        for bad in ["", "1A2B3C4D", "../../etc", "1a2b3c4", "1a2b3c4d5", "assistant"] {
            assert!(!is_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_bot_is_written_privately_in_its_files_and_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bots = dir.path().join("bots");
        let mut stored = bot("1a2b3c4d", 3);
        save(&bots, &stored).expect("saved");
        assert_eq!(load(&bots, "1a2b3c4d").expect("read"), Some(stored.clone()));
        let folder = bots.join("1a2b3c4d");
        assert_eq!(
            fs::read_to_string(folder.join("instructions.md")).expect("text"),
            "Suggest trains before flights.\n"
        );
        assert_eq!(fs::read_to_string(folder.join("skills/2.md")).expect("text"), "Text 2\n");

        // Fewer skills: the files of the others go.
        stored.skills.truncate(1);
        stored.personality.clear();
        save(&bots, &stored).expect("saved again");
        assert!(!folder.join("skills/2.md").exists() && !folder.join("skills/3.md").exists());
        assert_eq!(load(&bots, "1a2b3c4d").expect("read"), Some(stored.clone()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(&bots), 0o700);
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(&folder.join("skills")), 0o700);
            for file in ["bot.json", "persona.md", "instructions.md", "skills/1.md"] {
                assert_eq!(mode(&folder.join(file)), 0o600, "{file}");
            }
        }

        // Other folders and files are no bots; a bot's folder without bot.json neither.
        fs::create_dir_all(bots.join("notes")).expect("other folder");
        fs::create_dir_all(bots.join("5e6f7a8b")).expect("empty bot folder");
        fs::write(bots.join("0a0b0c0d"), "a file").expect("a file");
        assert_eq!(load_all(&bots).expect("read"), vec![stored]);
        assert_eq!(load(&bots, "../1a2b3c4d").expect("no path"), None);

        remove(&bots, "1a2b3c4d").expect("removed");
        assert!(!folder.exists());
        remove(&bots, "1a2b3c4d").expect("removing again is fine");
        assert_eq!(load(&bots, "1a2b3c4d").expect("read"), None);
        assert_eq!(load_all(&dir.path().join("missing")).expect("no folder"), Vec::new());
    }
}
