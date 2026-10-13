//! The example design packs the suite ships (tasks L.5, L.7, L.9): `designs/` in the repository,
//! built into this binary and written to the designs folder when the suite starts.
//!
//! They live here rather than in `torro-core`, because only the core service and the control
//! centre write the designs folder; in `torro-core` every app would carry the fonts (about 1 MB)
//! without ever using the copy.
//!
//! Each example is offered once (task L.10): the folder keeps the ids it was offered in a hidden
//! list, and a newer suite adds only the examples that are not on it. A pack the user deleted or
//! changed stays deleted or changed, a folder of their own with an example's name stays theirs, and
//! an update of the suite never overwrites anything in the folder.

use std::fs;
use std::io;
use std::path::Path;

/// One file of an example pack: its path inside the pack folder and its bytes.
type File = (&'static str, &'static [u8]);

/// An example pack: its id (the folder name) and its files.
struct Example {
    id: &'static str,
    files: &'static [File],
}

/// The hidden file in the designs folder that lists the examples the suite has offered, one id per
/// line. `torro_core::design::list` reads only folders, so it never shows up as a pack.
const OFFERED: &str = ".torro-examples";

/// The examples a designs folder from before the list (task L.10) was offered: only Plex existed.
const BEFORE_THE_LIST: &[&str] = &["plex"];

/// Builds the `(path, bytes)` pair of `designs/<pack>/<path>` in the repository.
macro_rules! pack_file {
    ($pack:literal, $path:literal) => {
        ($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/", $pack, "/", $path)))
    };
}

/// Every example pack with every file it has; the test `the_embedded_files_are_the_whole_pack`
/// fails when a file is added to `designs/` but not here.
const EXAMPLES: &[Example] = &[
    Example {
        id: "plex",
        files: &[
            pack_file!("plex", "design.toml"),
            pack_file!("plex", "logo.svg"),
            pack_file!("plex", "tray.svg"),
            pack_file!("plex", "preview.png"),
            pack_file!("plex", "fonts/LICENSE.txt"),
            pack_file!("plex", "fonts/IBMPlexSans-Regular.ttf"),
            pack_file!("plex", "fonts/IBMPlexSans-Medium.ttf"),
            pack_file!("plex", "fonts/IBMPlexSans-SemiBold.ttf"),
            pack_file!("plex", "fonts/IBMPlexSans-Bold.ttf"),
        ],
    },
    Example {
        id: "commitr",
        files: &[
            pack_file!("commitr", "design.toml"),
            pack_file!("commitr", "logo.svg"),
            pack_file!("commitr", "tray.svg"),
            pack_file!("commitr", "preview.png"),
            pack_file!("commitr", "fonts/LICENSE.txt"),
            pack_file!("commitr", "fonts/MonaSans-Regular.ttf"),
            pack_file!("commitr", "fonts/MonaSans-Medium.ttf"),
            pack_file!("commitr", "fonts/MonaSans-SemiBold.ttf"),
            pack_file!("commitr", "fonts/MonaSans-Bold.ttf"),
            pack_file!("commitr", "fonts/CommitrFigures-Regular.ttf"),
            pack_file!("commitr", "fonts/CommitrFigures-Medium.ttf"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[
            pack_file!("ribbon", "design.toml"),
            pack_file!("ribbon", "logo.svg"),
            pack_file!("ribbon", "tray.svg"),
            pack_file!("ribbon", "preview.png"),
            pack_file!("ribbon", "fonts/LICENSE.txt"),
            pack_file!("ribbon", "fonts/Selawik-Regular.ttf"),
            pack_file!("ribbon", "fonts/Selawik-SemiBold.ttf"),
            pack_file!("ribbon", "fonts/Selawik-Bold.ttf"),
        ],
    },
    Example {
        id: "channelr",
        files: &[
            pack_file!("channelr", "design.toml"),
            pack_file!("channelr", "logo.svg"),
            pack_file!("channelr", "tray.svg"),
            pack_file!("channelr", "preview.png"),
            pack_file!("channelr", "fonts/LICENSE.txt"),
            pack_file!("channelr", "fonts/Lato-Bold.ttf"),
            pack_file!("channelr", "fonts/Lato-Regular.ttf"),
        ],
    },
    Example {
        id: "guildr",
        files: &[
            pack_file!("guildr", "design.toml"),
            pack_file!("guildr", "logo.svg"),
            pack_file!("guildr", "tray.svg"),
            pack_file!("guildr", "preview.png"),
            pack_file!("guildr", "fonts/Figtree-Bold.ttf"),
            pack_file!("guildr", "fonts/Figtree-Medium.ttf"),
            pack_file!("guildr", "fonts/Figtree-Regular.ttf"),
            pack_file!("guildr", "fonts/Figtree-SemiBold.ttf"),
            pack_file!("guildr", "fonts/FigtreeTabular-Medium.ttf"),
            pack_file!("guildr", "fonts/FigtreeTabular-Regular.ttf"),
            pack_file!("guildr", "fonts/LICENSE.txt"),
        ],
    },
];

/// Creates the designs folder `dir` with the example packs in it, if `dir` does not exist yet.
/// Returns whether it did; an existing folder is never touched, even when it is empty.
///
/// The packs are written to a hidden folder next to `dir` first and moved into place in one step,
/// so a start that is cut short leaves no half pack behind, and a second process starting at the
/// same time finds either no folder or the whole one.
pub fn install_if_missing(dir: &Path) -> io::Result<bool> {
    if fs::symlink_metadata(dir).is_ok() {
        return Ok(false);
    }
    let parent = dir.parent().ok_or_else(|| io::Error::other("the designs folder has no parent"))?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".designs-{}", std::process::id()));
    if let Err(err) = write_examples(&staging).and_then(|()| fs::rename(&staging, dir)) {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the folder there in the meantime: it has the packs.
        return if dir.is_dir() { Ok(false) } else { Err(err) };
    }
    Ok(true)
}

/// Writes every example pack into the new folder `to`, with the list that says they were offered.
fn write_examples(to: &Path) -> io::Result<()> {
    for example in EXAMPLES {
        write_pack(&to.join(example.id), example)?;
    }
    let ids: Vec<String> = EXAMPLES.iter().map(|example| example.id.to_string()).collect();
    fs::write(to.join(OFFERED), list_text(&ids))
}

/// Writes the files of `example` into the folder `to`.
fn write_pack(to: &Path, example: &Example) -> io::Result<()> {
    for (path, bytes) in example.files {
        let target = to.join(path);
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(target, bytes)?;
    }
    Ok(())
}

/// Brings the designs folder `dir` up to the examples of this suite: a missing folder is created
/// with all of them ([`install_if_missing`]); an existing one gets each example it was not offered
/// yet, unless a folder of that name is already there. Returns the ids it wrote.
///
/// Every pack is written to a hidden folder in `dir` and moved into place in one step, the list
/// too, so a start that is cut short leaves nothing half done; a pack another process put there in
/// the meantime counts as written.
pub fn install_new(dir: &Path) -> io::Result<Vec<&'static str>> {
    if install_if_missing(dir)? {
        return Ok(EXAMPLES.iter().map(|example| example.id).collect());
    }
    let (mut offered, known) = match fs::read_to_string(dir.join(OFFERED)) {
        Ok(text) => (text.lines().map(str::trim).filter(|id| !id.is_empty()).map(String::from).collect(), true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => (offered_before_the_list(dir), false),
        Err(err) => return Err(err),
    };
    let mut added = Vec::new();
    let mut changed = !known;
    for example in EXAMPLES {
        if offered.iter().any(|id| id == example.id) {
            continue;
        }
        let target = dir.join(example.id);
        if fs::symlink_metadata(&target).is_err() {
            add_pack(dir, &target, example)?;
            added.push(example.id);
        }
        offered.push(example.id.to_string());
        changed = true;
    }
    if changed {
        let staging = dir.join(format!("{OFFERED}.{}", std::process::id()));
        fs::write(&staging, list_text(&offered))?;
        fs::rename(&staging, dir.join(OFFERED))?;
    }
    Ok(added)
}

/// What a folder without the list was offered: the examples of before the list, and every example
/// whose folder is there (an older suite or the user put it there).
fn offered_before_the_list(dir: &Path) -> Vec<String> {
    let present = EXAMPLES.iter().map(|example| example.id).filter(|id| dir.join(id).exists());
    let present = present.filter(|id| !BEFORE_THE_LIST.contains(id));
    BEFORE_THE_LIST.iter().copied().chain(present).map(String::from).collect()
}

/// Writes `example` as the pack folder `target` in `dir`, through a hidden folder beside it.
fn add_pack(dir: &Path, target: &Path, example: &Example) -> io::Result<()> {
    let staging = dir.join(format!(".example-{}-{}", example.id, std::process::id()));
    if let Err(err) = write_pack(&staging, example).and_then(|()| fs::rename(&staging, target)) {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the pack there in the meantime.
        if !target.is_dir() {
            return Err(err);
        }
    }
    Ok(())
}

/// The list file's text: one id per line.
fn list_text(ids: &[String]) -> String {
    ids.iter().map(|id| format!("{id}\n")).collect()
}

/// [`install_new`] for the user's designs folder (`torro_core::design::dir`), where errors are
/// only reported: the suite works without the examples. Called when the core starts and before the
/// control centre opens the folder.
pub fn install_examples() {
    let Some(dir) = torro_core::design::dir() else { return };
    if let Err(err) = install_new(&dir) {
        eprintln!("torroworkplace: cannot write the example designs to {}: {err}", dir.display());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// `designs/` in the repository.
    fn repo_designs() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../designs")
    }

    /// Every file under `dir`, relative, with `/` between folders.
    fn files_in(dir: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("read folder").flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                files_in(&entry.path(), &format!("{name}/"), found);
            } else {
                found.push(name);
            }
        }
    }

    #[test]
    fn the_plex_pack_in_the_repository_passes_every_rule() {
        let design = torro_core::design::load(&repo_designs(), "plex").unwrap_or_else(|problems| {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("designs/plex must be valid: {reasons:#?}")
        });
        assert_eq!(design.about.name, "Plex");
        assert_eq!(design.fonts.text.family, "IBM Plex Sans");
        assert_eq!(design.fonts.numbers, design.fonts.text, "Plex's figures are tabular already");
        assert_eq!((design.typo.body.size, design.typo.body.line, design.size.row), (14.0, 22.0, 44.0));
        assert!(design.logo.mark.is_some() && design.logo.tray.is_some() && design.preview.is_some());
        let listed = torro_core::design::list(&repo_designs());
        assert!(listed.iter().all(|pack| pack.valid()), "every pack in designs/ is valid");
    }

    #[test]
    fn the_commitr_pack_in_the_repository_passes_every_rule() {
        let design = torro_core::design::load(&repo_designs(), "commitr").unwrap_or_else(|problems| {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("designs/commitr must be valid: {reasons:#?}")
        });
        assert_eq!(design.about.name, "Commitr");
        assert_eq!(design.fonts.text.family, "Mona Sans");
        assert_eq!(design.fonts.numbers.family, "Commitr Figures", "Mona Sans's figures, made tabular");
        let light = design.colors.light;
        assert!(light.own_sidebar() && !design.colors.dark.own_sidebar(), "a dark sidebar in light mode only");
        assert_eq!(light.accent, torro_core::design::Rgba::hex(0x1F883D));
        assert!(design.logo.mark.is_some() && design.logo.tray.is_some() && design.preview.is_some());
    }

    #[test]
    fn the_ribbon_pack_in_the_repository_passes_every_rule() {
        use torro_core::design::Weight;
        let design = torro_core::design::load(&repo_designs(), "ribbon").unwrap_or_else(|problems| {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("designs/ribbon must be valid: {reasons:#?}")
        });
        assert_eq!(design.about.name, "Ribbon");
        assert_eq!(design.fonts.text.family, "Selawik");
        assert_eq!(design.fonts.numbers, design.fonts.text, "Selawik's figures are tabular already");
        assert!(design.fonts.text.medium.is_none(), "Selawik has no Medium");
        assert_eq!((design.typo.caption.weight, design.typo.body_strong.weight), (Weight::Semibold, Weight::Semibold));
        assert!(!design.colors.light.own_sidebar() && !design.colors.dark.own_sidebar());
        assert!(design.logo.mark.is_some() && design.logo.tray.is_some() && design.preview.is_some());
    }

    #[test]
    fn the_further_packs_pass_every_rule() {
        // id, name, text font, figures font
        let packs: &[(&str, &str, &str, &str)] = &[
            ("channelr", "Channelr", "Lato", "Lato"),
            ("guildr", "Guildr", "Figtree", "Figtree Tabular"),
            // further packs
        ];
        for (id, name, text, numbers) in packs {
            let design = torro_core::design::load(&repo_designs(), id).unwrap_or_else(|problems| {
                let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
                panic!("designs/{id} must be valid: {reasons:#?}")
            });
            assert_eq!(design.about.name, *name);
            assert_eq!((design.fonts.text.family.as_str(), design.fonts.numbers.family.as_str()), (*text, *numbers));
            assert!(design.logo.mark.is_some() && design.logo.tray.is_some() && design.preview.is_some(), "{id}");
        }
    }

    #[test]
    fn the_embedded_files_are_the_whole_pack() {
        let designs = repo_designs();
        let mut folders: Vec<String> = fs::read_dir(&designs)
            .expect("designs/")
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        folders.sort();
        let mut ids: Vec<&str> = EXAMPLES.iter().map(|example| example.id).collect();
        ids.sort_unstable();
        assert_eq!(folders, ids, "every folder in designs/ is an example");
        for example in EXAMPLES {
            let mut on_disk = Vec::new();
            files_in(&designs.join(example.id), "", &mut on_disk);
            on_disk.sort();
            let mut embedded: Vec<String> = example.files.iter().map(|(path, _)| path.to_string()).collect();
            embedded.sort();
            assert_eq!(embedded, on_disk, "{}", example.id);
        }
    }

    #[test]
    fn a_missing_folder_gets_the_examples_and_they_load() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("data").join("designs");
        assert!(install_if_missing(&dir).expect("install"));
        for id in ["plex", "commitr", "ribbon", "channelr", "guildr"] {
            assert!(torro_core::design::load(&dir, id).is_ok(), "the copy of {id} is valid too");
        }
        let left: Vec<_> = fs::read_dir(home.path().join("data")).expect("data").flatten().collect();
        assert_eq!(left.len(), 1, "no staging folder stays behind");
    }

    /// The ids in the list of `dir`.
    fn offered(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join(OFFERED)).expect("list").lines().map(String::from).collect()
    }

    fn all_ids() -> Vec<String> {
        EXAMPLES.iter().map(|example| example.id.to_string()).collect()
    }

    /// Every entry of `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).expect("read").flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_folder_gets_every_example_and_the_list() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        assert_eq!(install_new(&dir).expect("install").len(), EXAMPLES.len());
        assert_eq!(offered(&dir), all_ids());
        assert!(install_new(&dir).expect("again").is_empty(), "nothing twice");
        assert!(torro_core::design::list(&dir).iter().all(|pack| pack.valid()), "the list is no pack");
    }

    #[test]
    fn an_old_folder_with_plex_gets_the_others_and_keeps_plex() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        fs::create_dir_all(dir.join("plex")).expect("plex");
        fs::write(dir.join("plex").join("design.toml"), "format = 1\n").expect("changed plex");
        let added = install_new(&dir).expect("install");
        assert_eq!(added, all_ids().iter().filter(|id| *id != "plex").map(String::as_str).collect::<Vec<_>>());
        assert_eq!(fs::read_to_string(dir.join("plex").join("design.toml")).expect("read"), "format = 1\n");
        assert_eq!(offered(&dir).len(), EXAMPLES.len());
        let mut expected = all_ids();
        expected.push(OFFERED.to_string());
        expected.sort();
        assert_eq!(entries(&dir), expected, "no hidden work folder stays behind");
    }

    #[test]
    fn a_deleted_example_stays_deleted() {
        let home = tempfile::tempdir().expect("temp dir");
        // An old folder whose Plex the user deleted: Plex does not come back, the others do.
        let old = home.path().join("old");
        fs::create_dir(&old).expect("empty designs folder");
        assert!(!install_new(&old).expect("install").contains(&"plex"));
        assert!(!old.join("plex").exists());

        // Deleted after it was offered: it stays deleted.
        let dir = home.path().join("designs");
        install_new(&dir).expect("install");
        let gone = EXAMPLES[EXAMPLES.len() - 1].id;
        fs::remove_dir_all(dir.join(gone)).expect("delete");
        assert!(install_new(&dir).expect("again").is_empty());
        assert!(!dir.join(gone).exists());
    }

    #[test]
    fn a_folder_of_the_users_own_with_an_examples_name_stays_theirs() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        let own = EXAMPLES[EXAMPLES.len() - 1].id;
        fs::create_dir_all(dir.join(own)).expect("own");
        fs::write(dir.join(OFFERED), "plex\n").expect("an older list");
        fs::write(dir.join(own).join("notes.txt"), "mine").expect("own file");
        assert!(!install_new(&dir).expect("install").contains(&own));
        assert_eq!(entries(&dir.join(own)), ["notes.txt"]);
        assert!(offered(&dir).iter().any(|id| id == own), "offered, so it is not tried again");
    }

    #[test]
    fn an_existing_folder_is_never_touched() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        fs::create_dir(&dir).expect("empty designs folder");
        assert!(!install_if_missing(&dir).expect("install"));
        assert_eq!(fs::read_dir(&dir).expect("read").count(), 0, "a deleted example stays deleted");

        // An installed pack the user changed keeps the change.
        let other = home.path().join("other");
        assert!(install_if_missing(&other).expect("install"));
        let toml = other.join("plex").join("design.toml");
        fs::write(&toml, "format = 1\n").expect("change");
        assert!(!install_if_missing(&other).expect("install"));
        assert_eq!(fs::read_to_string(&toml).expect("read"), "format = 1\n");
    }
}
