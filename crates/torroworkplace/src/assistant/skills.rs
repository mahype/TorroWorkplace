//! Where skills live on disk (issue #703, "Skills" in `docs/assistent.md`): one folder per skill,
//! `<dir>/<8 hex>/SKILL.md`, for the assistant in `<config>/assistant/skills/` and for a bot in
//! `<config>/assistant/bots/<id>/skills/`. Folders 0700, files 0600, each file written whole and
//! atomically.
//!
//! A `SKILL.md` starts with a head between two lines `---` holding `name:` and `description:`, one
//! line each; the text follows as the person wrote it:
//!
//! ```text
//! ---
//! name: Offers
//! description: How I write offers
//! ---
//! 1. Greeting …
//! ```
//!
//! A folder of its own per skill leaves room for files beside the text later, as other assistants
//! keep them. The folder's name is no part of the skill: the name in its head is what requests
//! take. A head without a name is no skill and is left alone (named on stderr).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use torro_ipc::assistant::Skill;
use torro_ipc::{Request, Response};

/// The file of a skill in its folder.
const SKILL_FILE: &str = "SKILL.md";

/// The line around the head.
const FENCE: &str = "---";

/// A skill as kept on disk: its folder's name (8 hex digits) and the skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: String,
    pub skill: Skill,
}

/// The assistant's skills in its config folder `<config>/assistant`.
pub fn folder(assistant_config: &Path) -> PathBuf {
    assistant_config.join("skills")
}

/// Every skill in `dir`, by name (ignoring case); none when `dir` is not there. A folder whose
/// `SKILL.md` is missing is left out, one whose head has no name is named on stderr and left out.
pub fn load_all(dir: &Path) -> io::Result<Vec<Stored>> {
    let mut skills = Vec::new();
    for id in crate::bots::store::ids(dir)? {
        let path = dir.join(&id).join(SKILL_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        match parse(&String::from_utf8_lossy(&bytes)) {
            Some(skill) => skills.push(Stored { id, skill }),
            None => eprintln!("torroworkplace: {} has no name in its head; left out", path.display()),
        }
    }
    skills.sort_by(|a, b| a.skill.name.to_lowercase().cmp(&b.skill.name.to_lowercase()).then(a.id.cmp(&b.id)));
    Ok(skills)
}

/// Writes `stored` to its folder in `dir` (0700, the file 0600).
pub fn save(dir: &Path, stored: &Stored) -> io::Result<()> {
    if !crate::bots::store::is_id(&stored.id) {
        return Err(io::Error::other(format!("{:?} is no skill id", stored.id)));
    }
    let folder = dir.join(&stored.id);
    crate::clients::create_private_dir(&folder)?;
    crate::bots::store::write(&folder.join(SKILL_FILE), format(&stored.skill).as_bytes())
}

/// Removes the folder of skill `id` from `dir` with everything in it; one that is not there is fine.
pub fn remove(dir: &Path, id: &str) -> io::Result<()> {
    if !crate::bots::store::is_id(id) {
        return Ok(());
    }
    match fs::remove_dir_all(dir.join(id)) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// Writes `skills` as every skill in `dir` (a bot's, saved whole): a skill whose name is there keeps
/// its folder, a new one gets one, and the folders of skills no longer among them go. Nothing else in
/// `dir` is touched.
pub fn replace_all(dir: &Path, skills: &[Skill]) -> io::Result<()> {
    let before = load_all(dir)?;
    let mut taken: Vec<String> = crate::bots::store::ids(dir)?;
    let mut kept = Vec::new();
    for skill in skills {
        let id = match before.iter().find(|old| old.skill.is_named(&skill.name)) {
            Some(old) => old.id.clone(),
            None => {
                let id = crate::bots::store::new_id(&taken).map_err(io::Error::other)?;
                taken.push(id.clone());
                id
            }
        };
        save(dir, &Stored { id: id.clone(), skill: skill.clone() })?;
        kept.push(id);
    }
    for old in before {
        if !kept.contains(&old.id) {
            remove(dir, &old.id)?;
        }
    }
    Ok(())
}

/// The answer of `assistant.list_skills` (by name, ignoring case) or `assistant.get_skill`
/// (`not-found` for an unknown name) from `skills`: the assistant's or a bot's. `None` for every
/// other request.
pub fn read_answer(skills: &[Skill], request: &Request) -> Option<Response> {
    match request {
        Request::ListSkills => {
            let mut listed: Vec<_> = skills.iter().map(Skill::summary).collect();
            listed.sort_by_key(|skill| skill.name.to_lowercase());
            Some(Response::Skills { skills: listed })
        }
        Request::GetSkill { name } => Some(match skills.iter().find(|skill| skill.is_named(name)) {
            Some(skill) => Response::Skill { skill: skill.clone() },
            None => Response::Error { code: "not-found".into(), message: format!("no skill {:?}", name.trim()) },
        }),
        _ => None,
    }
}

/// A skill from the contents of its `SKILL.md`; `None` without a head or without a name in it. Keys
/// it does not know are skipped, so a file written by hand with more of them still reads.
pub fn parse(contents: &str) -> Option<Skill> {
    let mut lines = contents.split_inclusive('\n');
    if lines.next()?.trim_end() != FENCE {
        return None;
    }
    let (mut name, mut description) = (None, String::new());
    let mut head = 0;
    let mut closed = false;
    for line in lines.by_ref() {
        head += line.len();
        let line = line.trim_end();
        if line == FENCE {
            closed = true;
            break;
        }
        match line.split_once(':') {
            Some(("name", value)) => name = Some(value.trim().to_string()),
            Some(("description", value)) => description = value.trim().to_string(),
            _ => {}
        }
    }
    let name = name.filter(|name| closed && !name.is_empty())?;
    let start = contents.split_inclusive('\n').next().map_or(0, str::len) + head;
    let text = contents.get(start..).unwrap_or_default().trim_end().to_string();
    Some(Skill { name, description, text })
}

/// The contents of the `SKILL.md` of `skill`: the head, then the text with a line break at its end.
pub fn format(skill: &Skill) -> String {
    let mut contents =
        format!("{FENCE}\nname: {}\ndescription: {}\n{FENCE}\n", one_line(&skill.name), one_line(&skill.description));
    if !skill.text.is_empty() {
        contents.push_str(&skill.text);
        contents.push('\n');
    }
    contents
}

/// `text` on one line: line breaks become spaces, so a head always reads back as written.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, description: &str, text: &str) -> Skill {
        Skill { name: name.into(), description: description.into(), text: text.into() }
    }

    /// The head is read back as written, also a text that holds the fence itself and colons.
    #[test]
    fn a_skill_reads_back_as_written() {
        for written in [
            skill("Offers", "How I write offers: short", "1. Greeting\n---\nname: not a head\n\n2. Price"),
            skill("Invoices", "", "Sort by month."),
            skill("Empty", "No text", ""),
        ] {
            let contents = format(&written);
            assert_eq!(parse(&contents), Some(written.clone()), "{contents}");
        }
        assert_eq!(
            format(&skill("Offers", "How I write offers", "Be brief.")),
            "---\nname: Offers\ndescription: How I write offers\n---\nBe brief.\n"
        );
        // Written by hand: Windows line ends and unknown keys still read.
        let by_hand = "---\r\nname: Offers\r\nlicense: mine\r\n---\r\nBe brief.\r\n";
        assert_eq!(parse(by_hand), Some(skill("Offers", "", "Be brief.")));
        for broken in
            ["", "name: Offers\n", "---\nname: Offers\n", "---\ndescription: x\n---\ntext", "---\nname: \n---\n"]
        {
            assert_eq!(parse(broken), None, "{broken:?}");
        }
    }

    #[test]
    fn skills_are_kept_privately_one_folder_each() {
        let dir = tempfile::tempdir().expect("temp dir");
        let skills = dir.path().join("skills");
        assert_eq!(load_all(&skills).expect("no folder"), Vec::new());
        let offers = Stored { id: "1a2b3c4d".into(), skill: skill("offers", "How I write offers", "Be brief.") };
        let invoices = Stored { id: "5e6f7a8b".into(), skill: skill("Invoices", "", "By month.") };
        save(&skills, &offers).expect("saved");
        save(&skills, &invoices).expect("saved");
        assert_eq!(load_all(&skills).expect("read"), vec![invoices.clone(), offers.clone()], "by name");

        // A folder without a name in its head, or of another form, is no skill.
        fs::create_dir_all(skills.join("0a0b0c0d")).expect("folder");
        fs::write(skills.join("0a0b0c0d/SKILL.md"), "no head").expect("file");
        fs::create_dir_all(skills.join("notes")).expect("other folder");
        assert_eq!(load_all(&skills).expect("read").len(), 2);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode(&skills), 0o700);
            assert_eq!(mode(&skills.join("1a2b3c4d")), 0o700);
            assert_eq!(mode(&skills.join("1a2b3c4d/SKILL.md")), 0o600);
        }

        remove(&skills, "1a2b3c4d").expect("removed");
        remove(&skills, "1a2b3c4d").expect("again is fine");
        assert_eq!(load_all(&skills).expect("read"), vec![invoices.clone()]);

        // Saved whole: a kept name keeps its folder, a new one gets one, a dropped one goes, the rest stays.
        replace_all(&skills, &[skill("INVOICES", "", "By year."), skill("Travel", "", "Trains.")]).expect("replaced");
        let now = load_all(&skills).expect("read");
        assert_eq!(now.len(), 2);
        assert_eq!((now[0].id.as_str(), now[0].skill.text.as_str()), ("5e6f7a8b", "By year."));
        assert_eq!(now[1].skill.name, "Travel");
        replace_all(&skills, &[]).expect("none");
        assert_eq!(load_all(&skills).expect("read"), Vec::new());
        assert!(skills.join("0a0b0c0d").exists() && skills.join("notes").exists(), "nothing else goes");
    }
}
