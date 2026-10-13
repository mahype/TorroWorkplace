//! The example design packs the suite ships (tasks L.5, L.8): `designs/` in the repository, built into
//! this binary and written to the designs folder the first time the suite starts.
//!
//! They live here rather than in `torro-core`, because only the core service and the control
//! centre write the designs folder; in `torro-core` every app would carry the fonts (about 1 MB)
//! without ever using the copy. They are written only when the designs folder does not exist yet:
//! a pack the user deleted or changed stays deleted or changed, and an update of the suite never
//! overwrites anything in the folder.

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
        id: "dracula",
        files: &[
            pack_file!("dracula", "design.toml"),
            pack_file!("dracula", "logo.svg"),
            pack_file!("dracula", "tray.svg"),
            pack_file!("dracula", "preview.png"),
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

/// Writes every example pack into the new folder `to`.
fn write_examples(to: &Path) -> io::Result<()> {
    for example in EXAMPLES {
        for (path, bytes) in example.files {
            let target = to.join(example.id).join(path);
            if let Some(folder) = target.parent() {
                fs::create_dir_all(folder)?;
            }
            fs::write(target, bytes)?;
        }
    }
    Ok(())
}

/// [`install_if_missing`] for the user's designs folder (`torro_core::design::dir`), where errors
/// are only reported: the suite works without the examples. Called when the core starts and
/// before the control centre opens the folder.
pub fn install_examples() {
    let Some(dir) = torro_core::design::dir() else { return };
    if let Err(err) = install_if_missing(&dir) {
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
    fn the_dracula_pack_in_the_repository_passes_every_rule() {
        let design = torro_core::design::load(&repo_designs(), "dracula").unwrap_or_else(|problems| {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("designs/dracula must be valid: {reasons:#?}")
        });
        assert_eq!(design.about.name, "Dracula");
        let standard = torro_core::design::Design::standard();
        assert_eq!(design.fonts, standard.fonts, "Dracula keeps Inter");
        assert_eq!((design.colors.dark.panel, design.colors.light.panel), (rgb(0x282A36), rgb(0xFFFBEB)));
        assert_eq!((design.colors.dark.accent, design.colors.dark.focus_ring), (rgb(0xBD93F9), rgb(0xFF79C6)));
        assert!(design.logo.mark.is_some() && design.logo.tray.is_some() && design.preview.is_some());
    }

    fn rgb(hex: u32) -> torro_core::design::Rgba {
        torro_core::design::Rgba::hex(hex)
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
        for id in ["plex", "dracula"] {
            assert!(torro_core::design::load(&dir, id).is_ok(), "the copy of {id} is valid too");
        }
        let left: Vec<_> = fs::read_dir(home.path().join("data")).expect("data").flatten().collect();
        assert_eq!(left.len(), 1, "no staging folder stays behind");
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
