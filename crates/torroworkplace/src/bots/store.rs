//! Where the bots live on disk (task B.1, "Festlegungen für alle B-Aufgaben" in
//! `docs/tasks/assistent.md`): one folder per bot, `<config>/assistant/bots/<id>/`, the folders 0700
//! and every file 0600, each file written whole and atomically:
//!
//! - `bot.json`: name, description, picture, engine, model and the id of the bot's client
//!   ([`BotFile`]),
//! - `persona.md`: the personality, `instructions.md`: the instructions, as the person wrote them,
//! - `skills/<8 hex>/SKILL.md`: one skill each, as the assistant keeps its own (issue #703,
//!   [`crate::assistant::skills`]).
//!
//! Before issue #703 `bot.json` named the skills in order and `skills/<n>.md` held the text of the
//! n-th (from 1). Such a bot still reads, its skills without a description; its next save writes the
//! new form and removes the old files.
//!
//! The texts are files of their own so a person can read and edit them like any text. `bot.json` is
//! written after them (only the skill files of the old form go after it): a bot whose save broke off
//! reads with the texts written so far. A text file that is missing reads as empty; a folder without a
//! readable `bot.json` is no bot and is left alone.
//!
//! Errors carry the path only for the core's own log; what goes to a client never names it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use torro_ipc::bots::{Bot, Skill};

use crate::assistant::skills;

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
    /// Before issue #703: the skills' names in order, the n-th text in `skills/<n>.md`. Written
    /// empty (and so left out) since.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    // The old form wins while `bot.json` names skills: [`save`] removes its files only after it wrote
    // `bot.json` without them, so a save that broke off reads as before, never half of the new form.
    let mut skills = Vec::new();
    for (index, name) in file.skills.into_iter().enumerate() {
        skills.push(Skill { name, description: String::new(), text: text(old_skill_file(&folder, index))? });
    }
    if skills.is_empty() {
        skills = skills::load_all(&folder.join(SKILLS_FOLDER))?.into_iter().map(|stored| stored.skill).collect();
    }
    unique_names(&mut skills);
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

/// Writes `bot` to its folder in `dir` (0700, its files 0600): the texts first, `bot.json` last.
/// The folders of skills it no longer has go, and so do the skill files of the form before issue
/// #703 (`skills/<n>.md`); nothing else in `skills/` is touched.
pub fn save(dir: &Path, bot: &Bot) -> io::Result<()> {
    if !is_id(&bot.id) {
        return Err(io::Error::other(format!("{:?} is no bot id", bot.id)));
    }
    let folder = folder(dir, &bot.id);
    let skills = folder.join(SKILLS_FOLDER);
    crate::clients::create_private_dir(&skills)?;
    write(&folder.join(PERSONA_FILE), &with_end(&bot.personality))?;
    write(&folder.join(INSTRUCTIONS_FILE), &with_end(&bot.instructions))?;
    skills::replace_all(&skills, &bot.skills)?;
    let file = BotFile {
        name: bot.name.clone(),
        description: bot.description.clone(),
        avatar: bot.avatar.clone(),
        engine: bot.engine.clone(),
        model: bot.model.clone(),
        client: bot.client.clone(),
        skills: Vec::new(),
    };
    let mut json = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
    json.push(b'\n');
    write(&folder.join(BOT_FILE), &json)?;
    // The old files go last, once nothing names them any more.
    for item in fs::read_dir(&skills)? {
        let item = item?;
        if is_old_skill_file(&item.file_name().to_string_lossy()) && item.file_type()?.is_file() {
            fs::remove_file(item.path())?;
        }
    }
    Ok(())
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

/// The file of the skill at `index` (from 0) in a bot's `folder` before issue #703:
/// `skills/<index + 1>.md`.
fn old_skill_file(folder: &Path, index: usize) -> PathBuf {
    folder.join(SKILLS_FOLDER).join(format!("{}.md", index + 1))
}

/// Gives every skill a name of its own (review of #703): a bot from before issue #703 may hold two
/// skills of the same name, which `assistant.get_skill` could not tell apart and `bots.save_bot`
/// refuses. The later one becomes "<name> 2" (then 3 …), cut so it stays within
/// [`torro_ipc::bots::MAX_SKILL_NAME_CHARS`]; the person sees it in the bot editor and can rename it.
fn unique_names(skills: &mut [Skill]) {
    for at in 1..skills.len() {
        let (before, rest) = skills.split_at_mut(at);
        let skill = &mut rest[0];
        let taken = |name: &str| before.iter().any(|other| other.is_named(name));
        if !taken(&skill.name) {
            continue;
        }
        let base = skill.name.trim().to_string();
        for n in 2.. {
            let suffix = format!(" {n}");
            let room = torro_ipc::bots::MAX_SKILL_NAME_CHARS.saturating_sub(suffix.chars().count());
            let name = format!("{}{suffix}", base.chars().take(room).collect::<String>().trim_end());
            if !taken(&name) {
                skill.name = name;
                break;
            }
        }
    }
}

/// Whether `name` is that of a skill file before issue #703: `<n>.md` with `n` digits.
fn is_old_skill_file(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()))
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
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
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
            skills: (1..=skills)
                .map(|n| Skill {
                    name: format!("Skill {n}"),
                    description: format!("When {n}"),
                    text: format!("Text {n}"),
                })
                .collect(),
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
        let on_disk = skills::load_all(&folder.join("skills")).expect("skills");
        assert_eq!(on_disk.iter().map(|kept| kept.skill.clone()).collect::<Vec<_>>(), stored.skills);
        assert!(!fs::read_to_string(folder.join("bot.json")).expect("json").contains("skills"));

        // Fewer skills: the folders of the others go.
        stored.skills.truncate(1);
        stored.personality.clear();
        save(&bots, &stored).expect("saved again");
        assert_eq!(skills::load_all(&folder.join("skills")).expect("skills").len(), 1);
        assert_eq!(load(&bots, "1a2b3c4d").expect("read"), Some(stored.clone()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(&bots), 0o700);
            assert_eq!(mode(&folder), 0o700);
            assert_eq!(mode(&folder.join("skills")), 0o700);
            let skill = &skills::load_all(&folder.join("skills")).expect("skills")[0].id;
            for file in ["bot.json", "persona.md", "instructions.md", &format!("skills/{skill}/SKILL.md")] {
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

    /// Issue #703: a bot of the form before reads with its skills in order and no description; its
    /// next save writes the new form, removes the old files and nothing else.
    #[test]
    fn a_bot_of_the_old_form_keeps_its_skills() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bots = dir.path().join("bots");
        let folder = bots.join("1a2b3c4d");
        fs::create_dir_all(folder.join("skills")).expect("folder");
        fs::write(
            folder.join("bot.json"),
            r#"{"name": "Travel planner", "engine": "test", "client": "9f8e7d6c", "skills": ["Packing", "Trains"]}"#,
        )
        .expect("bot.json");
        fs::write(folder.join("skills/1.md"), "Pack light.\n").expect("skill 1");
        fs::write(folder.join("skills/2.md"), "Trains first.\n").expect("skill 2");
        fs::write(folder.join("skills/notes.txt"), "mine").expect("another file");
        let old = load(&bots, "1a2b3c4d").expect("read").expect("a bot");
        let named = |name: &str, text: &str| Skill { name: name.into(), description: String::new(), text: text.into() };
        assert_eq!(old.skills, vec![named("Packing", "Pack light."), named("Trains", "Trains first.")]);

        save(&bots, &old).expect("saved");
        assert!(!folder.join("skills/1.md").exists() && !folder.join("skills/2.md").exists());
        assert!(folder.join("skills/notes.txt").exists(), "nothing else goes");
        assert!(!fs::read_to_string(folder.join("bot.json")).expect("json").contains("skills"));
        let mut read = load(&bots, "1a2b3c4d").expect("read").expect("a bot");
        read.skills.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(read, old);
    }

    /// Review of #703: two skills of the same name in an old bot read with names of their own, so the
    /// bot can be saved again; the first keeps its name.
    #[test]
    fn an_old_bot_with_two_skills_of_one_name_reads_them_apart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bots = dir.path().join("bots");
        let folder = bots.join("1a2b3c4d");
        fs::create_dir_all(folder.join("skills")).expect("folder");
        fs::write(
            folder.join("bot.json"),
            r#"{"name": "Bot", "engine": "test", "client": "9f8e7d6c", "skills": ["Packing", "packing", "Packing 2"]}"#,
        )
        .expect("bot.json");
        let read = load(&bots, "1a2b3c4d").expect("read").expect("a bot");
        let names: Vec<&str> = read.skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, ["Packing", "packing 2", "Packing 2 2"]);
        let long = "a".repeat(torro_ipc::bots::MAX_SKILL_NAME_CHARS);
        let mut two = vec![
            Skill { name: long.clone(), description: String::new(), text: String::new() },
            Skill { name: long, description: String::new(), text: String::new() },
        ];
        unique_names(&mut two);
        assert_eq!(two[1].name.chars().count(), torro_ipc::bots::MAX_SKILL_NAME_CHARS);
        assert!(two[1].name.ends_with(" 2"));
    }

    /// Review of #703: a save of an old bot that broke off after part of the new form keeps reading the
    /// old form whole, as its files go only after `bot.json`.
    #[test]
    fn a_broken_off_save_of_an_old_bot_reads_as_before() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bots = dir.path().join("bots");
        let folder = bots.join("1a2b3c4d");
        fs::create_dir_all(folder.join("skills")).expect("folder");
        fs::write(
            folder.join("bot.json"),
            r#"{"name": "Travel planner", "engine": "test", "client": "9f8e7d6c", "skills": ["Packing", "Trains"]}"#,
        )
        .expect("bot.json");
        fs::write(folder.join("skills/1.md"), "Pack light.\n").expect("skill 1");
        fs::write(folder.join("skills/2.md"), "Trains first.\n").expect("skill 2");
        let written = Skill { name: "Packing".into(), description: String::new(), text: "Pack light.".into() };
        skills::replace_all(&folder.join("skills"), std::slice::from_ref(&written)).expect("half of the new form");
        let read = load(&bots, "1a2b3c4d").expect("read").expect("a bot");
        let names: Vec<&str> = read.skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names, ["Packing", "Trains"]);
        assert_eq!(read.skills[1].text, "Trains first.");
    }
}
