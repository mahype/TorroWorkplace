//! The examples the suite ships (tasks L.5, L.7–L.9, L.12–L.16, L.20): `designs/`, `fonts/` and
//! `color-schemes/` in the repository, built into this binary and written to the same three folders in
//! the data folder (`torro_core::design::{dir, font_dir, color_scheme_dir}`).
//!
//! They live here rather than in `torro-core`, because only the core service and the control
//! centre write these folders; in `torro-core` every app would carry the fonts (about 1 MB) without
//! ever using the copy.
//!
//! Each example is offered once (task L.10): every folder keeps the ids it was offered in a hidden
//! list, and a newer suite adds only the examples that are not on it. An example the user deleted or
//! changed stays deleted or changed, a folder of their own with an example's name stays theirs, and
//! an update of the suite never overwrites anything. Each of the three folders keeps its own list
//! (task L.20).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use torro_core::design::Part;

/// One file of an example: its path inside the example's folder and its bytes.
type File = (&'static str, &'static [u8]);

/// An example: its id (the folder name) and its files.
struct Example {
    id: &'static str,
    files: &'static [File],
}

/// The hidden file in each of the three folders that lists the examples the suite has offered there,
/// one id per line. The listings of `torro_core::design` read only folders, so it never shows up.
const OFFERED: &str = ".torro-examples";

/// The examples a designs folder from before the list (task L.10) was offered: only Plex existed.
const BEFORE_THE_LIST: &[&str] = &["plex"];

/// Builds the `(path, bytes)` pair of `<folder>/<id>/<path>` in the repository.
macro_rules! example_file {
    ($folder:literal, $id:literal, $path:literal) => {
        ($path, include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", $folder, "/", $id, "/", $path)))
    };
}

// The lists below hold every example with every file it has; the test
// `the_embedded_files_are_the_whole_example` fails when a file is added to the repository but not here.

/// The example designs with every file they have (`designs/` in the repository).
const DESIGNS: &[Example] = &[
    Example {
        id: "plex",
        files: &[example_file!("designs", "plex", "design.toml"), example_file!("designs", "plex", "preview.png")],
    },
    Example {
        id: "commitr",
        files: &[
            example_file!("designs", "commitr", "design.toml"),
            example_file!("designs", "commitr", "preview.png"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[example_file!("designs", "ribbon", "design.toml"), example_file!("designs", "ribbon", "preview.png")],
    },
    Example {
        id: "channelr",
        files: &[
            example_file!("designs", "channelr", "design.toml"),
            example_file!("designs", "channelr", "preview.png"),
        ],
    },
    Example {
        id: "guildr",
        files: &[example_file!("designs", "guildr", "design.toml"), example_file!("designs", "guildr", "preview.png")],
    },
    Example {
        id: "quattro",
        files: &[
            example_file!("designs", "quattro", "design.toml"),
            example_file!("designs", "quattro", "preview.png"),
        ],
    },
];

/// The example fonts with every file they have (`fonts/` in the repository).
const FONTS: &[Example] = &[
    Example {
        id: "plex",
        files: &[
            example_file!("fonts", "plex", "font.toml"),
            example_file!("fonts", "plex", "IBMPlexSans-Bold.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-Medium.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-Regular.ttf"),
            example_file!("fonts", "plex", "IBMPlexSans-SemiBold.ttf"),
            example_file!("fonts", "plex", "LICENSE.txt"),
        ],
    },
    Example {
        id: "commitr",
        files: &[
            example_file!("fonts", "commitr", "font.toml"),
            example_file!("fonts", "commitr", "CommitrFigures-Medium.ttf"),
            example_file!("fonts", "commitr", "CommitrFigures-Regular.ttf"),
            example_file!("fonts", "commitr", "LICENSE.txt"),
            example_file!("fonts", "commitr", "MonaSans-Bold.ttf"),
            example_file!("fonts", "commitr", "MonaSans-Medium.ttf"),
            example_file!("fonts", "commitr", "MonaSans-Regular.ttf"),
            example_file!("fonts", "commitr", "MonaSans-SemiBold.ttf"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[
            example_file!("fonts", "ribbon", "font.toml"),
            example_file!("fonts", "ribbon", "LICENSE.txt"),
            example_file!("fonts", "ribbon", "Selawik-Bold.ttf"),
            example_file!("fonts", "ribbon", "Selawik-Regular.ttf"),
            example_file!("fonts", "ribbon", "Selawik-SemiBold.ttf"),
        ],
    },
    Example {
        id: "channelr",
        files: &[
            example_file!("fonts", "channelr", "font.toml"),
            example_file!("fonts", "channelr", "LICENSE.txt"),
            example_file!("fonts", "channelr", "Lato-Bold.ttf"),
            example_file!("fonts", "channelr", "Lato-Regular.ttf"),
        ],
    },
    Example {
        id: "guildr",
        files: &[
            example_file!("fonts", "guildr", "font.toml"),
            example_file!("fonts", "guildr", "Figtree-Bold.ttf"),
            example_file!("fonts", "guildr", "Figtree-Medium.ttf"),
            example_file!("fonts", "guildr", "Figtree-Regular.ttf"),
            example_file!("fonts", "guildr", "Figtree-SemiBold.ttf"),
            example_file!("fonts", "guildr", "FigtreeTabular-Medium.ttf"),
            example_file!("fonts", "guildr", "FigtreeTabular-Regular.ttf"),
            example_file!("fonts", "guildr", "LICENSE.txt"),
        ],
    },
    Example {
        id: "quattro",
        files: &[
            example_file!("fonts", "quattro", "font.toml"),
            example_file!("fonts", "quattro", "LICENSE.txt"),
            example_file!("fonts", "quattro", "RobotoFlex-Bold.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-Medium.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-Regular.ttf"),
            example_file!("fonts", "quattro", "RobotoFlex-SemiBold.ttf"),
        ],
    },
];

/// The example colour schemes with every file they have (`color-schemes/` in the repository).
const COLOR_SCHEMES: &[Example] = &[
    Example {
        id: "plex",
        files: &[
            example_file!("color-schemes", "plex", "color-scheme.toml"),
            example_file!("color-schemes", "plex", "logo.svg"),
        ],
    },
    Example {
        id: "commitr",
        files: &[
            example_file!("color-schemes", "commitr", "color-scheme.toml"),
            example_file!("color-schemes", "commitr", "logo.svg"),
        ],
    },
    Example {
        id: "ribbon",
        files: &[
            example_file!("color-schemes", "ribbon", "color-scheme.toml"),
            example_file!("color-schemes", "ribbon", "logo.svg"),
        ],
    },
    Example {
        id: "channelr",
        files: &[
            example_file!("color-schemes", "channelr", "color-scheme.toml"),
            example_file!("color-schemes", "channelr", "logo.svg"),
        ],
    },
    Example {
        id: "guildr",
        files: &[
            example_file!("color-schemes", "guildr", "color-scheme.toml"),
            example_file!("color-schemes", "guildr", "logo.svg"),
        ],
    },
    Example {
        id: "quattro",
        files: &[
            example_file!("color-schemes", "quattro", "color-scheme.toml"),
            example_file!("color-schemes", "quattro", "logo.svg"),
        ],
    },
    Example {
        id: "nord",
        files: &[
            example_file!("color-schemes", "nord", "color-scheme.toml"),
            example_file!("color-schemes", "nord", "logo.svg"),
        ],
    },
    Example {
        id: "solarized",
        files: &[
            example_file!("color-schemes", "solarized", "color-scheme.toml"),
            example_file!("color-schemes", "solarized", "logo.svg"),
        ],
    },
    Example {
        id: "catppuccin",
        files: &[
            example_file!("color-schemes", "catppuccin", "color-scheme.toml"),
            example_file!("color-schemes", "catppuccin", "logo.svg"),
        ],
    },
    Example {
        id: "gruvbox",
        files: &[
            example_file!("color-schemes", "gruvbox", "color-scheme.toml"),
            example_file!("color-schemes", "gruvbox", "logo.svg"),
        ],
    },
    Example {
        id: "dracula",
        files: &[
            example_file!("color-schemes", "dracula", "color-scheme.toml"),
            example_file!("color-schemes", "dracula", "logo.svg"),
        ],
    },
    Example {
        id: "everforest",
        files: &[
            example_file!("color-schemes", "everforest", "color-scheme.toml"),
            example_file!("color-schemes", "everforest", "logo.svg"),
        ],
    },
];

/// The examples of `part`.
fn examples(part: Part) -> &'static [Example] {
    match part {
        Part::Design => DESIGNS,
        Part::Font => FONTS,
        Part::ColorScheme => COLOR_SCHEMES,
    }
}

/// The three folders of the look in one data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// `designs/`.
    pub designs: PathBuf,
    /// `fonts/`.
    pub fonts: PathBuf,
    /// `color-schemes/`.
    pub color_schemes: PathBuf,
}

impl Folders {
    /// The user's folders (`torro_core::design::{dir, font_dir, color_scheme_dir}`); `None` without a
    /// home directory.
    pub fn user() -> Option<Folders> {
        Some(Folders {
            designs: torro_core::design::dir()?,
            fonts: torro_core::design::font_dir()?,
            color_schemes: torro_core::design::color_scheme_dir()?,
        })
    }

    /// The folder of `part`.
    pub fn get(&self, part: Part) -> &Path {
        match part {
            Part::Design => &self.designs,
            Part::Font => &self.fonts,
            Part::ColorScheme => &self.color_schemes,
        }
    }
}

/// The three parts, in the order they are brought up to date.
const PARTS: [Part; 3] = [Part::Design, Part::Font, Part::ColorScheme];

/// Brings all three `folders` up to the examples of this suite ([`install_new`] for each), each on
/// its own: an error in one does not keep the others from their examples. Returns every error with
/// its part.
pub fn install_all(folders: &Folders) -> Vec<(Part, io::Error)> {
    PARTS.into_iter().filter_map(|part| install_new(folders.get(part), part).err().map(|err| (part, err))).collect()
}

/// Brings the folder `dir` of `part` up to this suite's examples of that part: a missing folder is
/// created with all of them, an existing one gets each example it was not offered yet, unless a
/// folder of that name is already there. Returns the ids it wrote. Each folder keeps its own list.
///
/// Every example is written to a hidden folder and moved into place in one step, the list too, so a
/// start that is cut short leaves nothing half done; an example another process put there in the
/// meantime counts as written.
pub fn install_new(dir: &Path, part: Part) -> io::Result<Vec<&'static str>> {
    if fs::symlink_metadata(dir).is_err() {
        return create(dir, part);
    }
    let (mut offered, known) = offered_so_far(dir, part)?;
    let mut added = Vec::new();
    let mut changed = !known;
    for example in examples(part) {
        if offered.iter().any(|id| id == example.id) {
            continue;
        }
        let target = dir.join(example.id);
        if fs::symlink_metadata(&target).is_err() {
            add_example(dir, &target, example)?;
            added.push(example.id);
        }
        offered.push(example.id.to_string());
        changed = true;
    }
    if changed {
        write_list(dir, &offered)?;
    }
    Ok(added)
}

/// What the existing folder `dir` (of `part`) was offered so far, and whether it has a list. Without
/// a list: for designs the examples of before the list and every example whose folder is there (an
/// older suite or the user put it there); for fonts and colour schemes nothing.
fn offered_so_far(dir: &Path, part: Part) -> io::Result<(Vec<String>, bool)> {
    match fs::read_to_string(dir.join(OFFERED)) {
        Ok(text) => Ok((text.lines().map(str::trim).filter(|id| !id.is_empty()).map(String::from).collect(), true)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Ok((if part == Part::Design { before_the_list(dir) } else { Vec::new() }, false))
        }
        Err(err) => Err(err),
    }
}

/// What a designs folder without the list was offered: the examples of before the list, and every
/// example whose folder is there.
fn before_the_list(dir: &Path) -> Vec<String> {
    let present = DESIGNS.iter().map(|example| example.id).filter(|id| dir.join(id).exists());
    let present = present.filter(|id| !BEFORE_THE_LIST.contains(id));
    BEFORE_THE_LIST.iter().copied().chain(present).map(String::from).collect()
}

/// Creates the missing folder `dir` with every example of `part` and the list, through a hidden
/// folder beside it moved into place in one step: a second process starting at the same time finds
/// either no folder or the whole one.
fn create(dir: &Path, part: Part) -> io::Result<Vec<&'static str>> {
    let parent = dir.parent().ok_or_else(|| io::Error::other("the folder has no parent"))?;
    fs::create_dir_all(parent)?;
    let name = dir.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let staging = parent.join(format!(".{name}-{}", std::process::id()));
    let ids: Vec<&'static str> = examples(part).iter().map(|example| example.id).collect();
    let offered: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let written = examples(part)
        .iter()
        .try_for_each(|example| write_example(&staging.join(example.id), example))
        .and_then(|()| fs::create_dir_all(&staging))
        .and_then(|()| fs::write(staging.join(OFFERED), list_text(&offered)))
        .and_then(|()| fs::rename(&staging, dir));
    if let Err(err) = written {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the folder there in the meantime: it has the examples.
        return if dir.is_dir() { Ok(Vec::new()) } else { Err(err) };
    }
    Ok(ids)
}

/// Writes the files of `example` into the folder `to`.
fn write_example(to: &Path, example: &Example) -> io::Result<()> {
    for (path, bytes) in example.files {
        let target = to.join(path);
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(target, bytes)?;
    }
    Ok(())
}

/// Writes `example` as the folder `target` in `dir`, through a hidden folder beside it.
fn add_example(dir: &Path, target: &Path, example: &Example) -> io::Result<()> {
    let staging = dir.join(format!(".example-{}-{}", example.id, std::process::id()));
    if let Err(err) = write_example(&staging, example).and_then(|()| fs::rename(&staging, target)) {
        let _ = fs::remove_dir_all(&staging);
        // Another process put the example there in the meantime.
        if !target.is_dir() {
            return Err(err);
        }
    }
    Ok(())
}

/// Replaces the list of `dir` in one step.
fn write_list(dir: &Path, ids: &[String]) -> io::Result<()> {
    let staging = dir.join(format!("{OFFERED}.{}", std::process::id()));
    fs::write(&staging, list_text(ids))?;
    fs::rename(&staging, dir.join(OFFERED))
}

/// The list file's text: one id per line.
fn list_text(ids: &[String]) -> String {
    ids.iter().map(|id| format!("{id}\n")).collect()
}

/// [`install_all`] for the user's folders, where errors are only reported: the suite works without
/// the examples. Called before the control centre opens a folder; it never carries old packs over,
/// so only the core service ever writes `designs/.torro-migrated`.
pub fn install_examples() {
    let Some(folders) = Folders::user() else { return };
    report(&folders, install_all(&folders));
}

/// Prints each error with the folder it happened in.
fn report(folders: &Folders, errors: Vec<(Part, io::Error)>) {
    for (part, err) in errors {
        eprintln!("torroworkplace: cannot write the examples to {}: {err}", folders.get(part).display());
    }
}

#[cfg(test)]
mod tests {
    use torro_core::design::{
        self, ColorScheme, Design, DesignError, FontSet, LevelName, MIN_TEXT_SIZE, Rgba, STANDARD_ID, Selection, Weight,
    };

    use super::*;

    /// `<folder>/` of `part` in the repository.
    fn repo(part: Part) -> PathBuf {
        let folder = match part {
            Part::Design => "designs",
            Part::Font => "fonts",
            Part::ColorScheme => "color-schemes",
        };
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(folder)
    }

    /// The three folders under `data`.
    fn folders(data: &Path) -> Folders {
        Folders { designs: data.join("designs"), fonts: data.join("fonts"), color_schemes: data.join("color-schemes") }
    }

    /// The design, font and colour scheme `id` of the repository, each from its own folder.
    fn shipped(id: &str) -> Selection {
        let fail = |part: &str, problems: Vec<DesignError>| -> ! {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            panic!("{part}/{id} must be valid: {reasons:#?}")
        };
        Selection {
            design: design::load(&repo(Part::Design), id).unwrap_or_else(|p| fail("designs", p)),
            font: design::load_font(&repo(Part::Font), id).unwrap_or_else(|p| fail("fonts", p)),
            scheme: design::load_color_scheme(&repo(Part::ColorScheme), id)
                .unwrap_or_else(|p| fail("color-schemes", p)),
        }
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

    /// The ids in the list of `dir`.
    fn offered(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join(OFFERED)).expect("list").lines().map(String::from).collect()
    }

    fn ids(part: Part) -> Vec<String> {
        examples(part).iter().map(|example| example.id.to_string()).collect()
    }

    /// Every entry of `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).expect("read").flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// Every file in the three folders, for "nothing changed".
    fn everything(all: &Folders) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        for part in PARTS {
            let mut names = Vec::new();
            files_in(all.get(part), &format!("{part:?}/"), &mut names);
            let dir = all.get(part).to_path_buf();
            found.extend(names.into_iter().map(|name| {
                let rel = name.split_once('/').map_or(name.as_str(), |(_, rel)| rel).to_string();
                let bytes = fs::read(dir.join(&rel)).expect("read");
                (name, bytes)
            }));
        }
        found.sort();
        found
    }

    /// The six examples that are a design, a font and a colour scheme.
    const COMPLETE: [&str; 6] = ["channelr", "commitr", "guildr", "plex", "quattro", "ribbon"];
    /// The six examples that are only a colour scheme (task L.20).
    const SCHEMES_ONLY: [&str; 6] = ["catppuccin", "dracula", "everforest", "gruvbox", "nord", "solarized"];

    #[test]
    fn the_embedded_files_are_the_whole_example() {
        for part in PARTS {
            let mut folders = entries(&repo(part));
            folders.retain(|name| repo(part).join(name).is_dir());
            let mut expected = ids(part);
            expected.sort();
            assert_eq!(folders, expected, "every folder in {} is an example", repo(part).display());
            for example in examples(part) {
                let mut on_disk = Vec::new();
                files_in(&repo(part).join(example.id), "", &mut on_disk);
                on_disk.sort();
                let mut embedded: Vec<String> = example.files.iter().map(|(path, _)| path.to_string()).collect();
                embedded.sort();
                assert_eq!(embedded, on_disk, "{part:?} {}", example.id);
                assert!(!on_disk.iter().any(|file| file.ends_with("tray.svg")), "no tray symbol any more (#114)");
            }
        }
    }

    /// Task L.20: Nord and the other five are only colour schemes; the six others are all three parts,
    /// and every one of them is valid.
    #[test]
    fn every_example_loads_without_reasons_in_its_folders() {
        let listed = |part: Part| -> Vec<String> {
            let found: Vec<(String, Vec<DesignError>)> = match part {
                Part::Design => {
                    design::list(&repo(part)).into_iter().map(|l| (l.id.clone(), l.problems().to_vec())).collect()
                }
                Part::Font => {
                    design::list_fonts(&repo(part)).into_iter().map(|l| (l.id.clone(), l.problems().to_vec())).collect()
                }
                Part::ColorScheme => design::list_color_schemes(&repo(part))
                    .into_iter()
                    .map(|l| (l.id.clone(), l.problems().to_vec()))
                    .collect(),
            };
            for (id, problems) in &found {
                assert!(problems.is_empty(), "{part:?} {id}: {problems:?}");
            }
            found.into_iter().map(|(id, _)| id).filter(|id| id != STANDARD_ID).collect()
        };
        assert_eq!(listed(Part::Design), COMPLETE);
        assert_eq!(listed(Part::Font), COMPLETE);
        let mut schemes: Vec<&str> = COMPLETE.iter().chain(&SCHEMES_ONLY).copied().collect();
        schemes.sort_unstable();
        assert_eq!(listed(Part::ColorScheme), schemes);
        for id in COMPLETE {
            let design = design::load(&repo(Part::Design), id).expect("valid");
            assert_eq!((design.about.font.as_deref(), design.about.color_scheme.as_deref()), (Some(id), Some(id)));
            assert!(design.preview.is_some(), "{id} keeps its preview");
        }
        for id in COMPLETE.iter().chain(&SCHEMES_ONLY) {
            assert!(design::load_color_scheme(&repo(Part::ColorScheme), id).expect("valid").logo.mark.is_some());
        }
    }

    /// The text levels of an example as they were drawn before task L.20: the standard's, with what
    /// the old complete pack set. `(level, size, line)`.
    fn before(id: &str) -> Vec<(LevelName, f32, f32)> {
        let own: &[(LevelName, f32, f32)] = match id {
            "plex" => &[
                (LevelName::Meta, 12.0, 18.0),
                (LevelName::Body, 14.0, 22.0),
                (LevelName::BodyStrong, 14.0, 22.0),
                (LevelName::Read, 15.0, 24.0),
                (LevelName::Title, 17.0, 24.0),
                (LevelName::Headline, 24.0, 30.0),
                (LevelName::Display, 30.0, 36.0),
            ],
            "ribbon" => &[],
            _ => &[(LevelName::Body, 14.0, 20.0), (LevelName::BodyStrong, 14.0, 20.0)],
        };
        let standard = Design::standard().typo;
        LevelName::ALL
            .iter()
            .map(|name| {
                let level = standard.get(*name);
                own.iter().find(|(own, ..)| own == name).copied().unwrap_or((*name, level.size, level.line))
            })
            .collect()
    }

    /// Task L.20: the sizes of the designs are meant for Inter now; in their own font they are drawn as
    /// large as before, at most 0.5 px off. The one exception is the smallest level: a design may not
    /// go below 11 ([`MIN_TEXT_SIZE`]), so in a font drawn more than 6.8 % larger than Inter (Lato,
    /// Figtree, Selawik) `micro` is 12 instead of 11.
    #[test]
    fn the_designs_look_as_before_in_their_own_font() {
        for id in COMPLETE {
            let look = shipped(id);
            for (name, size, line) in before(id) {
                let drawn = look.level(name);
                let smallest = name == LevelName::Micro && look.design.typo.micro.size == MIN_TEXT_SIZE;
                let off =
                    if smallest { (MIN_TEXT_SIZE * look.font.size_adjust * 2.0).round() / 2.0 - size } else { 0.5 };
                assert!((drawn.size - size).abs() <= off.max(0.5), "{id} {name:?}: {} for {size}", drawn.size);
                assert!((drawn.line - line).abs() <= 0.5, "{id} {name:?}: line {} for {line}", drawn.line);
            }
            // Inter draws them smaller now: the sizes are Inter's.
            let inter = Selection { font: FontSet::standard(), ..look.clone() };
            assert!(inter.level(LevelName::Body).size <= look.level(LevelName::Body).size, "{id}");
        }
    }

    /// The size adjustments the samples were measured with in task L.19.
    #[test]
    fn the_fonts_adjust_as_measured() {
        for (id, factor) in [
            ("plex", 1.058),
            ("commitr", 1.038),
            ("channelr", 1.078),
            ("guildr", 1.092),
            ("quattro", 1.063),
            ("ribbon", 1.092),
        ] {
            let font = design::load_font(&repo(Part::Font), id).expect("valid");
            assert!((font.size_adjust - factor).abs() < 0.001, "{id}: {}", font.size_adjust);
        }
    }

    #[test]
    fn the_plex_example_is_its_three_parts() {
        let look = shipped("plex");
        assert_eq!((look.design.about.name.as_str(), look.font.about.name.as_str()), ("Plex", "Plex"));
        assert_eq!(look.font.fonts.text.family, "IBM Plex Sans");
        assert_eq!(look.font.fonts.numbers, look.font.fonts.text, "Plex's figures are tabular already");
        let body = look.level(LevelName::Body);
        assert_eq!((body.size, body.line, look.design.size.row), (14.0, 22.0, 44.0));
        assert_eq!(look.scheme.colors.dark.accent, Rgba::hex(0x3AA6A0));
        assert!(look.scheme.logo.mark.is_some());
    }

    #[test]
    fn the_commitr_example_is_its_three_parts() {
        let look = shipped("commitr");
        assert_eq!(look.font.fonts.text.family, "Mona Sans");
        assert_eq!(look.font.fonts.numbers.family, "Commitr Figures", "Mona Sans's figures, made tabular");
        let light = look.scheme.colors.light;
        assert!(light.own_sidebar() && !look.scheme.colors.dark.own_sidebar(), "a dark sidebar in light mode only");
        assert_eq!(light.accent, Rgba::hex(0x1F883D));
    }

    #[test]
    fn the_ribbon_example_is_its_three_parts() {
        let look = shipped("ribbon");
        assert_eq!(look.font.fonts.text.family, "Selawik");
        assert!(look.font.fonts.text.medium.is_none(), "Selawik has no Medium");
        let weight = |name| look.level(name).weight;
        assert_eq!((weight(LevelName::Caption), weight(LevelName::BodyStrong)), (Weight::Semibold, Weight::Semibold));
        assert!(!look.scheme.colors.light.own_sidebar() && !look.scheme.colors.dark.own_sidebar());
    }

    #[test]
    fn the_further_fonts_are_their_families() {
        // id, text font, figures font
        for (id, text, numbers) in [
            ("channelr", "Lato", "Lato"),
            ("guildr", "Figtree", "Figtree Tabular"),
            ("quattro", "Roboto Flex", "Roboto Flex"),
        ] {
            let font = shipped(id).font;
            assert_eq!((font.fonts.text.family.as_str(), font.fonts.numbers.family.as_str()), (text, numbers));
        }
        assert_eq!(shipped("channelr").level(LevelName::Title).weight, Weight::Bold, "Lato has no SemiBold");
    }

    #[test]
    fn dracula_is_a_colour_scheme_only() {
        let scheme = design::load_color_scheme(&repo(Part::ColorScheme), "dracula").expect("valid");
        let rgb = Rgba::hex;
        assert_eq!((scheme.colors.dark.panel, scheme.colors.light.panel), (rgb(0x282A36), rgb(0xFFFBEB)));
        assert_eq!((scheme.colors.dark.accent, scheme.colors.dark.focus_ring), (rgb(0xBD93F9), rgb(0xFF79C6)));
        assert!(
            matches!(design::load(&repo(Part::Design), "dracula"), Err(p) if matches!(p[..], [DesignError::NotFound { .. }]))
        );
        assert_ne!(scheme, ColorScheme::standard());
    }

    #[test]
    fn a_new_data_folder_gets_every_example_and_the_lists() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(&home.path().join("data"));
        let errors = install_all(&all);
        assert!(errors.is_empty(), "{errors:?}");
        for part in PARTS {
            let mut expected = ids(part);
            expected.push(OFFERED.to_string());
            expected.sort();
            assert_eq!(entries(all.get(part)), expected, "{part:?}");
            assert_eq!(offered(all.get(part)), ids(part));
        }
        assert!(design::list(&all.designs).iter().all(|entry| entry.valid()), "the copies are valid, the list is none");
        assert!(design::list_fonts(&all.fonts).iter().all(|entry| entry.valid()));
        assert!(design::list_color_schemes(&all.color_schemes).iter().all(|entry| entry.valid()));
        let before = everything(&all);
        install_all(&all);
        assert_eq!(everything(&all), before, "nothing twice");
        let left = entries(&home.path().join("data"));
        assert_eq!(left, ["color-schemes", "designs", "fonts"], "no staging folder stays behind");
    }

    #[test]
    fn an_old_designs_folder_with_plex_gets_the_others_and_keeps_plex() {
        let home = tempfile::tempdir().expect("temp dir");
        let dir = home.path().join("designs");
        fs::create_dir_all(dir.join("plex")).expect("plex");
        fs::write(dir.join("plex").join("design.toml"), "format = 1\n").expect("changed plex");
        let added = install_new(&dir, Part::Design).expect("install");
        assert_eq!(added, ids(Part::Design).iter().filter(|id| *id != "plex").map(String::as_str).collect::<Vec<_>>());
        assert_eq!(fs::read_to_string(dir.join("plex").join("design.toml")).expect("read"), "format = 1\n");
        let mut expected = ids(Part::Design);
        expected.push(OFFERED.to_string());
        expected.sort();
        assert_eq!(entries(&dir), expected, "no hidden work folder stays behind");
    }

    #[test]
    fn a_deleted_example_stays_deleted() {
        let home = tempfile::tempdir().expect("temp dir");
        // An old designs folder whose Plex the user deleted: Plex does not come back, the others do.
        let old = home.path().join("old");
        fs::create_dir(&old).expect("empty designs folder");
        assert!(!install_new(&old, Part::Design).expect("install").contains(&"plex"));
        assert!(!old.join("plex").exists());

        // Deleted after it was offered: it stays deleted, in every folder.
        let all = folders(&home.path().join("data"));
        install_all(&all);
        for part in PARTS {
            let gone = examples(part)[examples(part).len() - 1].id;
            fs::remove_dir_all(all.get(part).join(gone)).expect("delete");
        }
        let before = everything(&all);
        install_all(&all);
        assert_eq!(everything(&all), before);
    }

    /// Task L.20: each folder keeps its own list; a new fonts or colour schemes folder gets all of
    /// its examples, whatever the designs folder was offered.
    #[test]
    fn each_folder_keeps_its_own_list() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(&home.path().join("data"));
        fs::create_dir_all(&all.designs).expect("designs");
        fs::write(all.designs.join(OFFERED), list_text(&ids(Part::Design))).expect("every design offered");
        let errors = install_all(&all);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(entries(&all.designs), [OFFERED], "deleted designs stay deleted");
        for part in [Part::Font, Part::ColorScheme] {
            assert_eq!(offered(all.get(part)), ids(part));
            assert!(ids(part).iter().all(|id| all.get(part).join(id).is_dir()), "{part:?}");
        }
        // A colour schemes folder the user made without a list gets every example it has no folder of.
        let own = folders(&home.path().join("own"));
        fs::create_dir_all(own.color_schemes.join("nord")).expect("own nord");
        install_new(&own.color_schemes, Part::ColorScheme).expect("install");
        assert_eq!(entries(&own.color_schemes.join("nord")), Vec::<String>::new(), "theirs stays theirs");
        assert!(own.color_schemes.join("dracula").is_dir());
    }

    #[test]
    fn a_folder_of_the_users_own_with_an_examples_name_stays_theirs() {
        let home = tempfile::tempdir().expect("temp dir");
        for part in PARTS {
            let dir = home.path().join(format!("{part:?}"));
            let own = examples(part)[examples(part).len() - 1].id;
            fs::create_dir_all(dir.join(own)).expect("own");
            fs::write(dir.join(OFFERED), "plex\n").expect("an older list");
            fs::write(dir.join(own).join("notes.txt"), "mine").expect("own file");
            assert!(!install_new(&dir, part).expect("install").contains(&own));
            assert_eq!(entries(&dir.join(own)), ["notes.txt"]);
            assert!(offered(&dir).iter().any(|id| id == own), "offered, so it is not tried again");
        }
    }

    #[test]
    fn a_changed_example_keeps_the_change() {
        let home = tempfile::tempdir().expect("temp dir");
        let all = folders(home.path());
        install_all(&all);
        let toml = all.fonts.join("plex").join("font.toml");
        fs::write(&toml, "format = 1\n").expect("change");
        fs::remove_file(all.fonts.join(OFFERED)).expect("even without its list");
        install_all(&all);
        assert_eq!(fs::read_to_string(&toml).expect("read"), "format = 1\n");
    }
}
