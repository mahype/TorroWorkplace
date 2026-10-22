//! Test data for the design tests: copies of the fixture folders under `tests/fixtures/{designs,fonts,
//! color-schemes}/` in a temporary data folder laid out like the real one, with the font files the
//! fixtures name copied in from where the suite keeps them (Inter in `torro-gui`, the sample fonts in
//! the repository), so no font file is stored twice.

use std::fs;
use std::path::{Path, PathBuf};

use super::{
    About, Design, FontRole, Layout, Level, ListLook, ListSelection, ListStyle, Pad, Part, Radius, STANDARD_ID, Size,
    Space, TypeScale, Weight,
};
use crate::i18n::tr;

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

/// The standard design as the code held it before issue #291 (`Design::standard()` at `bb6e993`), with
/// the keys issue #291 added at the values that keep the picture: the rail as wide as
/// `torro_gui::shell::rail()` (52) and the expanded rail as the sidebar (232), no reading width (the
/// rule's most, 1200) and the reader's page padding, rows with a line below, a bar at the chosen row
/// and an avatar as wide as a row's marks (`size.control`). Never edit these values to make a test
/// pass: `designs/torro/design.toml` must give exactly this until task L.34.
pub(super) fn standard_of_before() -> Design {
    let level = |font, weight, size, line| Level { font, weight, size, line };
    Design {
        id: STANDARD_ID.to_string(),
        about: About {
            // The only difference to before: the standard is called "Standard" since issue #291.
            name: "Standard".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: Some("Torro".to_string()),
            description: Some(tr("Inter, quiet surfaces, Torro red").to_string()),
            ..About::default()
        },
        typo: TypeScale {
            micro: level(FontRole::Numbers, Weight::Regular, 11.0, 14.0),
            caption: level(FontRole::Text, Weight::Medium, 12.0, 16.0),
            meta: level(FontRole::Text, Weight::Regular, 12.0, 16.0),
            body: level(FontRole::Text, Weight::Regular, 13.0, 20.0),
            body_strong: level(FontRole::Text, Weight::Medium, 13.0, 20.0),
            read: level(FontRole::Text, Weight::Regular, 14.0, 22.0),
            title: level(FontRole::Heading, Weight::Semibold, 16.0, 22.0),
            headline: level(FontRole::Heading, Weight::Semibold, 22.0, 28.0),
            display: level(FontRole::Text, Weight::Semibold, 28.0, 32.0),
            hero: level(FontRole::Heading, Weight::Semibold, 48.0, 56.0),
            countdown: level(FontRole::Numbers, Weight::Medium, 28.0, 32.0),
        },
        space: Space { xs: 4.0, s: 8.0, m: 12.0, l: 16.0, xl: 24.0, xxl: 32.0 },
        radius: Radius { xs: 4.0, sm: 6.0, md: 8.0, lg: 10.0 },
        layout: Layout {
            inset: 8.0,
            page_padding: Pad { vertical: 24.0, horizontal: 24.0 },
            panel_padding: Pad { vertical: 16.0, horizontal: 16.0 },
            row_padding: Pad { vertical: 0.0, horizontal: 16.0 },
            control_padding: Pad { vertical: 0.0, horizontal: 10.0 },
            section_gap: 24.0,
            item_gap: 8.0,
            reading_padding: Pad { vertical: 24.0, horizontal: 24.0 },
        },
        size: Size {
            control_sm: 28.0,
            control: 32.0,
            control_lg: 36.0,
            control_xl: 44.0,
            row: 40.0,
            row_two: 52.0,
            row_message: 92.0,
            row_notice: 64.0,
            row_sidebar: 28.0,
            row_group: 32.0,
            header: 44.0,
            sidebar: 232.0,
            props: 300.0,
            panel: 400.0,
            list: 460.0,
            list_medium: 380.0,
            hour: 48.0,
            axis: 52.0,
            axis_narrow: 44.0,
            content_max: 720.0,
            form_max: 640.0,
            card_gap: 12.0,
            avatar: 36.0,
            avatar_lg: 40.0,
            tile_sm: 20.0,
            tile: 28.0,
            tile_lg: 36.0,
            command: 46.0,
            pill: 42.0,
            rail: 52.0,
            rail_wide: 232.0,
            reading_max: 1200.0,
        },
        list: ListLook {
            style: ListStyle::Rows,
            separators: true,
            selection: ListSelection::Bar,
            avatar: 32.0,
            preview_lines: 1,
        },
        preview: None,
    }
}
