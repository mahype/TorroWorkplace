//! Test data for the design tests: copies of the fixture folders under `tests/fixtures/{designs,fonts,
//! color-schemes}/` in a temporary data folder laid out like the real one, with the font files the
//! fixtures name copied in from where the suite keeps them (Inter in `torro-gui`, the sample fonts in
//! the repository), so no font file is stored twice.

use std::fs;
use std::path::{Path, PathBuf};

use super::Part;

/// `tests/fixtures/` of this crate.
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
/// The root of the repository.
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// A temporary data folder with `designs/`, `fonts/` and `color-schemes/`.
pub(super) struct Data {
    tmp: tempfile::TempDir,
}

impl Data {
    /// An empty data folder; the part folders do not exist yet.
    pub(super) fn new() -> Data {
        Data { tmp: tempfile::tempdir().expect("temp dir") }
    }

    /// A data folder with copies of the fixtures `names` of `part`.
    pub(super) fn with(part: Part, names: &[&str]) -> Data {
        let data = Data::new();
        data.add(part, names);
        data
    }

    /// A data folder with every fixture of every part.
    pub(super) fn all() -> Data {
        let data = Data::new();
        for part in [Part::Design, Part::Font, Part::ColorScheme] {
            let names = fixture_names(part);
            data.add(part, &names.iter().map(String::as_str).collect::<Vec<_>>());
        }
        data
    }

    /// Copies the fixtures `names` of `part` in, with the font files they name.
    pub(super) fn add(&self, part: Part, names: &[&str]) {
        for name in names {
            let target = self.dir(part).join(name);
            copy_dir(&Path::new(FIXTURES).join(part.folder()).join(name), &target);
            supply_fonts(&target, part.file());
        }
    }

    /// The folder of `part` (it may not exist).
    pub(super) fn dir(&self, part: Part) -> PathBuf {
        self.tmp.path().join(part.folder())
    }

    /// The folder of one entry.
    pub(super) fn entry(&self, part: Part, id: &str) -> PathBuf {
        self.dir(part).join(id)
    }

    /// Writes `toml` as the file of `part` for the entry `id`.
    pub(super) fn write(&self, part: Part, id: &str, toml: &str) {
        fs::create_dir_all(self.entry(part, id)).expect("entry folder");
        fs::write(self.entry(part, id).join(part.file()), toml).expect("toml");
    }
}

/// The names of the fixtures of `part`, sorted.
pub(super) fn fixture_names(part: Part) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(Path::new(FIXTURES).join(part.folder()))
        .expect("fixtures")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The font file `name` (`Inter-Regular.ttf`, `Lato-Bold.ttf` …) where the suite keeps it: Inter in
/// `torro-gui`, the samples in the repository's `fonts/<id>/`.
pub(super) fn font_file(name: &str) -> PathBuf {
    let mut places = vec![Path::new(REPO).join("crates/torro-gui/assets/fonts")];
    if let Ok(entries) = fs::read_dir(Path::new(REPO).join("fonts")) {
        places.extend(entries.flatten().map(|entry| entry.path()));
    }
    places
        .into_iter()
        .map(|place| place.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no font file {name} in the repository"))
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create");
    for entry in fs::read_dir(from).expect("fixture").flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

/// Copies in every TTF/OTF file the TOML `file` in `folder` names, under the path it names.
fn supply_fonts(folder: &Path, file: &str) {
    let Ok(text) = fs::read_to_string(folder.join(file)) else { return };
    let Ok(table) = toml::from_str::<toml::Table>(&text) else { return };
    let mut named = Vec::new();
    font_paths(&toml::Value::Table(table), &mut named);
    for rel in named {
        let target = folder.join(&rel);
        let Some(name) = target.file_name().map(|name| name.to_string_lossy().into_owned()) else { continue };
        if target.exists() || rel.contains("..") {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("font folder");
        }
        fs::copy(font_file(&name), &target).expect("copy font");
    }
}

fn font_paths(value: &toml::Value, found: &mut Vec<String>) {
    match value {
        toml::Value::String(text) if text.ends_with(".ttf") || text.ends_with(".otf") => found.push(text.clone()),
        toml::Value::Table(table) => table.values().for_each(|inner| font_paths(inner, found)),
        _ => {}
    }
}
