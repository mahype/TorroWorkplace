//! Reading the folders of the three parts, and the designs folder itself.
//!
//! Shared by all three: [`Listed`], the listing of a folder ([`listing`]), opening one entry
//! ([`open`]), the three passes over a TOML file ([`parse`]: TOML at all, then every unknown key, then
//! the typed values with their positions) and `[about]` ([`AboutFile`]). Fonts are read in `font.rs`,
//! colour schemes in `scheme.rs`.
//!
//! A `design.toml` is merged into [`Design::standard`] and held to the design rules. It holds the form
//! only: fonts and colours have folders of their own, so `[fonts.*]`, `[colors.*]`, `[logo]` and
//! `[about] font_license` of a pack from before task L.19 are unknown keys like any other (task L.21,
//! no carry-over of old packs).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::files::Root;
use super::{
    About, Density, Design, DesignError, FontRole, Level, Pad, Part, Radius, STANDARD_ID, STANDARD_SIZE, Space, Weight,
    is_valid_id, rules,
};

/// One entry of [`list`], [`super::list_fonts`] or [`super::list_color_schemes`]: a part (`T` is
/// [`Design`], [`FontSet`] or [`ColorScheme`]) and whether it can be used.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed<T> {
    /// [`STANDARD_ID`] or the folder name.
    pub id: String,
    /// From the TOML file as far as it could be read; for a folder whose file can't be read the name
    /// is the id and the version empty.
    pub about: About,
    /// The standard (always first, always valid).
    pub standard: bool,
    /// The merged part, or every reason it can't be used (never empty).
    pub result: Result<T, Vec<DesignError>>,
}

impl<T> Listed<T> {
    /// Whether the part can be used.
    pub fn valid(&self) -> bool {
        self.result.is_ok()
    }

    /// Why it can't be used; empty when it can.
    pub fn problems(&self) -> &[DesignError] {
        self.result.as_ref().err().map_or(&[], Vec::as_slice)
    }
}

/// Every design: the standard first, then each folder under `dir` (the designs folder, see
/// [`super::dir`]) by name, valid or with its reasons. Hidden folders (`.git`) and folder names that
/// are not UTF-8 are left out; a missing `dir` lists only the standard.
pub fn list(dir: &Path) -> Vec<Listed<Design>> {
    let standard = Design::standard();
    listing(dir, standard.about.clone(), standard, read)
}

/// The design `id` from the designs folder `dir`, merged with the standard and checked.
/// [`STANDARD_ID`] gives [`Design::standard`] without touching the disk.
///
/// `Err` holds every reason the design can't be used (at least one); the caller then keeps the design
/// it has (the concept: a part is never applied in part).
pub fn load(dir: &Path, id: &str) -> Result<Design, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(Design::standard());
    }
    read(dir, id, &mut About::default())
}

/// What reads one entry of a folder: `(folder, id, about)`, where `about` is filled as far as the file
/// could be read.
pub(super) type Reader<T> = fn(&Path, &str, &mut About) -> Result<T, Vec<DesignError>>;

/// The standard (with `about`) first, then every visible folder under `dir` by name, read by `read`.
pub(super) fn listing<T>(dir: &Path, about: About, standard: T, read: Reader<T>) -> Vec<Listed<T>> {
    let mut listed = vec![Listed { id: STANDARD_ID.to_string(), about, standard: true, result: Ok(standard) }];
    let Ok(entries) = fs::read_dir(dir) else { return listed };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    ids.sort();
    listed.extend(ids.into_iter().map(|id| {
        let mut about = About { name: id.clone(), ..About::default() };
        let result = read(dir, &id, &mut about);
        Listed { id, about, standard: false, result }
    }));
    listed
}

/// The folder of `id` under `dir` for `part`, after the id and the total size were checked.
pub(super) fn open(dir: &Path, id: &str, part: Part) -> Result<Root, Vec<DesignError>> {
    if !is_valid_id(id) {
        return Err(vec![DesignError::BadId { id: id.to_string() }]);
    }
    let Some(root) = Root::open(&dir.join(id)) else {
        return Err(vec![DesignError::NotFound { part, id: id.to_string() }]);
    };
    match root.check_total() {
        Some(too_large) => Err(vec![too_large]),
        None => Ok(root),
    }
}

// ---------------------------------------------------------------------------------------------
// TOML files
// ---------------------------------------------------------------------------------------------

/// `[about]` as written in any of the three files; which keys a file may use, its `known` function
/// says ([`parse`]).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AboutFile {
    pub(super) name: Option<String>,
    pub(super) version: Option<String>,
    pub(super) author: Option<String>,
    pub(super) description: Option<String>,
    /// Designs only: the suggested font.
    pub(super) font: Option<String>,
    /// Designs only: the suggested colour scheme.
    pub(super) color_scheme: Option<String>,
}

impl AboutFile {
    /// What the list shows: the name (else `id`) and the version as far as they are there.
    pub(super) fn about(&self, id: &str) -> About {
        About {
            name: self.name.clone().unwrap_or_else(|| id.to_string()),
            version: self.version.clone().unwrap_or_default(),
            author: self.author.clone(),
            description: self.description.clone(),
            font: self.font.clone(),
            color_scheme: self.color_scheme.clone(),
        }
    }
}

/// The checks every file shares: `format = 1`, `about.name` and `about.version`.
pub(super) fn header(format: Option<i64>, about: &AboutFile, problems: &mut Vec<DesignError>) {
    match format {
        None => problems.push(DesignError::MissingKey { key: "format".into() }),
        Some(1) => {}
        Some(found) => problems.push(DesignError::UnsupportedFormat { found }),
    }
    for (key, value) in [("about.name", &about.name), ("about.version", &about.version)] {
        if value.is_none() {
            problems.push(DesignError::MissingKey { key: key.into() });
        }
    }
}

/// What `key` is in the table at `path` (`""`, `"colors.dark"` …) of one kind of file: a value, a
/// table to look into, or unknown (`None`).
pub(super) type Known = fn(path: &str, key: &str) -> Option<bool>;

/// Every key in `table` (at `path`) `known` does not know, with its full path.
fn unknown_keys(known: Known, path: &str, table: &toml::Table, found: &mut Vec<DesignError>) {
    for (key, value) in table {
        let full = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
        match (known(path, key), value) {
            (None, _) => found.push(DesignError::UnknownKey { key: full }),
            (Some(true), toml::Value::Table(inner)) => unknown_keys(known, &full, inner, found),
            // A value where a table belongs (or the reverse) is a type error for the third pass.
            _ => {}
        }
    }
}

/// The three passes over `text`, the TOML file `file`; the first that fails ends the reading.
pub(super) fn parse<F: DeserializeOwned>(file: &str, text: &str, known: Known) -> Result<F, Vec<DesignError>> {
    let table: toml::Table = toml::from_str(text).map_err(|err| vec![syntax(file, text, &err)])?;
    let mut unknown = Vec::new();
    unknown_keys(known, "", &table, &mut unknown);
    if !unknown.is_empty() {
        return Err(unknown);
    }
    toml::from_str(text).map_err(|err| vec![syntax(file, text, &err)])
}

fn syntax(file: &str, text: &str, err: &toml::de::Error) -> DesignError {
    let line = err.span().map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
    DesignError::Syntax { file: file.to_string(), line, reason: err.message().trim().to_string() }
}

// ---------------------------------------------------------------------------------------------
// design.toml
// ---------------------------------------------------------------------------------------------

/// `design.toml` as written; everything optional, so missing keys can be named one by one.
/// `deny_unknown_fields` backs up [`known`].
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignFile {
    format: Option<i64>,
    #[serde(default)]
    about: AboutFile,
    #[serde(rename = "type", default)]
    typo: BTreeMap<String, LevelFile>,
    space: Option<SpaceFile>,
    radius: Option<RadiusFile>,
    layout: Option<LayoutFile>,
    #[serde(default)]
    size: BTreeMap<String, f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelFile {
    font: Option<FontRole>,
    size: Option<f32>,
    line: Option<f32>,
    weight: Option<Weight>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceFile {
    scale: [f32; 6],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RadiusFile {
    scale: [f32; 4],
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFile {
    inset: Option<f32>,
    page_padding: Option<[f32; 2]>,
    panel_padding: Option<[f32; 2]>,
    row_padding: Option<[f32; 2]>,
    control_padding: Option<[f32; 2]>,
    section_gap: Option<f32>,
    item_gap: Option<f32>,
    /// A word, so a design that writes anything but `compact` or `comfortable` fails in the third
    /// pass with its line, as `type.*.font` and `weight` do.
    density: Option<Density>,
}

const LEVEL_KEYS: [&str; 11] =
    ["micro", "caption", "meta", "body", "body_strong", "read", "title", "headline", "display", "hero", "countdown"];
const LAYOUT_KEYS: [&str; 8] =
    ["inset", "page_padding", "panel_padding", "row_padding", "control_padding", "section_gap", "item_gap", "density"];

/// The keys of `design.toml`: the form only, so the fonts and colours of an old complete pack are
/// unknown keys.
fn known(path: &str, key: &str) -> Option<bool> {
    const TABLE: Option<bool> = Some(true);
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" => Some(false),
            "about" | "type" | "space" | "radius" | "layout" | "size" => TABLE,
            _ => None,
        },
        "about" => value(["name", "version", "author", "description", "font", "color_scheme"].contains(&key)),
        "type" => LEVEL_KEYS.contains(&key).then_some(true),
        p if p.strip_prefix("type.").is_some_and(|level| LEVEL_KEYS.contains(&level)) => {
            value(["font", "size", "line", "weight"].contains(&key))
        }
        "space" | "radius" => value(key == "scale"),
        "layout" => value(LAYOUT_KEYS.contains(&key)),
        "size" => value(STANDARD_SIZE.entries().iter().any(|(name, _)| *name == key)),
        _ => None,
    }
}

fn read(dir: &Path, id: &str, about: &mut About) -> Result<Design, Vec<DesignError>> {
    let root = open(dir, id, Part::Design)?;
    let text = root.toml(Part::Design.file()).map_err(|err| vec![err])?;
    let file: DesignFile = parse(Part::Design.file(), &text, known)?;
    *about = file.about.about(id);
    let mut problems = Vec::new();
    header(file.format, &file.about, &mut problems);
    for (key, suggested) in [("about.font", &file.about.font), ("about.color_scheme", &file.about.color_scheme)] {
        if let Some(suggested) = suggested
            && !(is_valid_id(suggested) || suggested == STANDARD_ID)
        {
            problems.push(DesignError::BadSuggestion { key: key.into(), id: suggested.clone() });
        }
    }
    let mut design = merge(id, &file);
    design.about = about.clone();
    match root.preview() {
        Ok(preview) => design.preview = preview,
        Err(found) => problems.extend(found),
    }
    problems.extend(rules::design(&design));
    if problems.is_empty() { Ok(design) } else { Err(problems) }
}

/// The form of `file` over the standard's.
fn merge(id: &str, file: &DesignFile) -> Design {
    let mut design = Design::standard();
    design.id = id.to_string();
    for (name, level) in &file.typo {
        if let Some(slot) = design.typo.slot_mut(name) {
            merge_level(slot, level);
        }
    }
    // The title of a reminder over the whole screen (task ER1.5) follows a design that does not name
    // it, so designs from before stay as they were: it takes its headlines' font and weight.
    if !file.typo.contains_key("hero") {
        design.typo.hero.font = design.typo.headline.font;
        design.typo.hero.weight = design.typo.headline.weight;
    }
    if let Some(SpaceFile { scale: [xs, s, m, l, xl, xxl] }) = file.space {
        design.space = Space { xs, s, m, l, xl, xxl };
    }
    if let Some(RadiusFile { scale: [xs, sm, md, lg] }) = file.radius {
        design.radius = Radius { xs, sm, md, lg };
    }
    if let Some(layout) = &file.layout {
        merge_layout(&mut design, layout);
    }
    for (key, value) in &file.size {
        if let Some(slot) = design.size.slot_mut(key) {
            *slot = *value;
        }
    }
    design
}

fn merge_level(slot: &mut Level, file: &LevelFile) {
    slot.font = file.font.unwrap_or(slot.font);
    slot.weight = file.weight.unwrap_or(slot.weight);
    slot.size = file.size.unwrap_or(slot.size);
    slot.line = file.line.unwrap_or(slot.line);
}

fn merge_layout(design: &mut Design, file: &LayoutFile) {
    let pad =
        |value: Option<[f32; 2]>, old: Pad| value.map_or(old, |[vertical, horizontal]| Pad { vertical, horizontal });
    let l = &mut design.layout;
    l.inset = file.inset.unwrap_or(l.inset);
    l.page_padding = pad(file.page_padding, l.page_padding);
    l.panel_padding = pad(file.panel_padding, l.panel_padding);
    l.row_padding = pad(file.row_padding, l.row_padding);
    l.control_padding = pad(file.control_padding, l.control_padding);
    l.section_gap = file.section_gap.unwrap_or(l.section_gap);
    l.item_gap = file.item_gap.unwrap_or(l.item_gap);
    l.density = file.density.unwrap_or(l.density);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::testdata::{Data, fixture_names};

    fn problems(dir: &Path, id: &str) -> Vec<DesignError> {
        match load(dir, id) {
            Ok(_) => panic!("{id} must be refused"),
            Err(problems) => problems,
        }
    }

    /// A design `id` with `body` after `[about]`, in a temporary designs folder.
    fn written(data: &Data, id: &str, body: &str) {
        let toml = format!("format = 1\n[about]\nname = \"{id}\"\nversion = \"1.0.0\"\n{body}");
        data.write(Part::Design, id, &toml);
    }

    #[test]
    fn the_standard_is_loaded_without_a_folder_and_listed_first() {
        let data = Data::new();
        assert_eq!(load(&data.dir(Part::Design), STANDARD_ID), Ok(Design::standard()));
        let listed = list(&data.dir(Part::Design));
        assert_eq!(listed.len(), 1, "a missing folder lists only Torro");
        assert!(listed[0].standard && listed[0].valid() && listed[0].problems().is_empty());
        assert_eq!(listed[0].about.name, "Torro");
    }

    /// Task L.19: a `design.toml` with nothing but `[about]` is a design that looks like Torro.
    #[test]
    fn only_about_is_the_standards_form() {
        let data = Data::new();
        written(&data, "plain", "");
        let design = load(&data.dir(Part::Design), "plain").expect("valid");
        let mut expected = Design::standard();
        expected.id = "plain".into();
        expected.about = About { name: "plain".into(), version: "1.0.0".into(), ..About::default() };
        assert_eq!(design, expected);
    }

    #[test]
    fn a_design_with_every_key_loads() {
        let data = Data::with(Part::Design, &["form"]);
        let design = load(&data.dir(Part::Design), "form").expect("valid");
        assert_eq!((design.about.version.as_str(), design.about.author.as_deref()), ("2.1.0", Some("Torro tests")));
        assert_eq!((design.about.font.as_deref(), design.about.color_scheme.as_deref()), (Some("plex"), Some("torro")));
        assert_eq!(
            design.typo.title,
            Level { font: FontRole::Heading, weight: Weight::Semibold, size: 16.0, line: 22.0 }
        );
        assert_eq!(design.typo.hero.weight, Weight::Bold, "any weight: the font replaces what it lacks");
        assert_eq!(design.radius, Radius { xs: 3.0, sm: 5.0, md: 7.0, lg: 9.0 });
        assert_eq!(design.layout.page_padding, Pad { vertical: 24.0, horizontal: 32.0 });
        assert_eq!((design.layout.inset, design.layout.item_gap), (10.0, 10.0));
        assert_eq!(design.layout.density, Density::Comfortable);
        assert_eq!((design.size.row, design.size.sidebar, design.size.axis), (44.0, 248.0, 56.0));
    }

    /// Task E8.2: the reading level and the sizes of the comfortable density are keys of the format.
    #[test]
    fn the_comfortable_tokens_are_read() {
        let data = Data::new();
        let body = "[type.read]\nsize = 15\nline = 24\n\
                    [size]\ncard_gap = 16\navatar = 40\navatar_lg = 44\ntile_sm = 22\ntile = 30\ntile_lg = 40\n";
        written(&data, "roomy", body);
        let design = load(&data.dir(Part::Design), "roomy").expect("valid");
        assert_eq!(design.typo.read, Level { font: FontRole::Text, weight: Weight::Regular, size: 15.0, line: 24.0 });
        let s = design.size;
        assert_eq!(
            [s.card_gap, s.avatar, s.avatar_lg, s.tile_sm, s.tile, s.tile_lg],
            [16.0, 40.0, 44.0, 22.0, 30.0, 40.0]
        );
    }

    /// Task E13.1: `layout.density` is a key of the format. A design that leaves it out keeps the
    /// standard's `compact`, and a word the format does not know is refused with its line – the
    /// density has no range rule of its own (`rules.rs`).
    #[test]
    fn the_density_is_read() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        written(&data, "cards", "[layout]\ndensity = \"comfortable\"\n");
        assert_eq!(load(&dir, "cards").expect("valid").layout.density, Density::Comfortable);

        written(&data, "rows", "[layout]\nitem_gap = 10\n");
        let rows = load(&dir, "rows").expect("valid");
        assert_eq!(rows.layout.density, Density::Compact, "left out: the standard's density stands");
        assert_eq!(rows.layout.item_gap, 10.0, "the rest of the table is still read");

        written(&data, "roomy", "[layout]\ndensity = \"roomy\"\n");
        let refused = problems(&dir, "roomy");
        assert!(matches!(&refused[..], [DesignError::Syntax { line: Some(6), .. }]), "{refused:?}");

        written(&data, "misspelt", "[layout]\ndense = \"compact\"\n");
        assert_eq!(problems(&dir, "misspelt"), [DesignError::UnknownKey { key: "layout.dense".into() }]);
    }

    /// Task L.21: no carry-over of old packs. The fonts, colours and logo of a pack from before task
    /// L.19 are unknown keys in `design.toml` like any other, so such a pack can't be used and is
    /// listed with its reasons – a complete one as well as one that held only colours or fonts.
    #[test]
    fn the_sections_of_an_old_pack_are_unknown_keys() {
        let data = Data::with(Part::Design, &["full", "accent-only", "font-only"]);
        let dir = data.dir(Part::Design);
        let unknown = |keys: &[&str]| -> Vec<DesignError> {
            keys.iter().map(|key| DesignError::UnknownKey { key: (*key).into() }).collect()
        };
        assert_eq!(problems(&dir, "full"), unknown(&["about.font_license", "colors", "fonts", "logo"]));
        assert_eq!(problems(&dir, "accent-only"), unknown(&["colors"]));
        assert_eq!(problems(&dir, "font-only"), unknown(&["about.font_license", "fonts"]));

        written(&data, "dark", "[size]\nrow = 44\n[colors.dark]\naccent = \"#0E7C7B\"\n");
        let dark = problems(&dir, "dark");
        assert_eq!(dark, unknown(&["colors"]), "a design with colours is unusable, whatever else it holds");
        assert_eq!(dark[0].to_string(), DesignError::UnknownKey { key: "colors".into() }.to_string());

        let listed: Vec<(String, bool)> =
            list(&dir).into_iter().map(|entry| (entry.id.clone(), entry.valid())).collect();
        let expected =
            [(STANDARD_ID, true), ("accent-only", false), ("dark", false), ("font-only", false), ("full", false)];
        assert_eq!(listed, expected.map(|(id, valid)| (id.to_string(), valid)), "listed, with their reasons");
    }

    #[test]
    fn each_design_rule_has_a_broken_example() {
        let data = Data::with(
            Part::Design,
            &["tiny-type", "short-row", "unknown-key", "format-2", "narrow-axis", "bad-suggestion"],
        );
        let dir = data.dir(Part::Design);
        assert_eq!(
            problems(&dir, "tiny-type"),
            [DesignError::OutOfRange { key: "type.caption.size".into(), value: 9.0, min: 11.0, max: 40.0 }]
        );
        assert_eq!(
            problems(&dir, "short-row"),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 32.0, max: 64.0 }]
        );
        assert_eq!(problems(&dir, "unknown-key"), [DesignError::UnknownKey { key: "size.rowx".into() }]);
        assert_eq!(problems(&dir, "format-2"), [DesignError::UnsupportedFormat { found: 2 }]);
        assert_eq!(
            problems(&dir, "narrow-axis"),
            [DesignError::OutOfRange { key: "size.axis_narrow".into(), value: 40.0, min: 44.0, max: 120.0 }]
        );
        assert_eq!(
            problems(&dir, "bad-suggestion"),
            [DesignError::BadSuggestion { key: "about.font".into(), id: "IBM Plex".into() }],
            "a suggestion is only checked to be an id, not to exist"
        );
    }

    #[test]
    fn required_keys_are_named() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        data.write(Part::Design, "bare", "[about]\nauthor = \"Nobody\"\n");
        let keys: Vec<String> = problems(&dir, "bare")
            .into_iter()
            .map(|p| match p {
                DesignError::MissingKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["format", "about.name", "about.version"]);
        fs::create_dir_all(dir.join("empty")).expect("dir");
        assert_eq!(
            problems(&dir, "empty"),
            [DesignError::MissingFile { key: "design.toml".into(), path: "design.toml".into() }]
        );
        assert_eq!(problems(&dir, "gone"), [DesignError::NotFound { part: Part::Design, id: "gone".into() }]);
        assert_eq!(problems(&dir, "../etc"), [DesignError::BadId { id: "../etc".into() }]);
    }

    #[test]
    fn files_are_size_checked_before_reading() {
        let data = Data::with(Part::Design, &["short-row"]);
        let toml = data.entry(Part::Design, "short-row").join("design.toml");
        let mut text = fs::read_to_string(&toml).expect("read");
        text.push_str(&format!("# {}\n", "x".repeat(70 * 1024)));
        fs::write(toml, &text).expect("write");
        assert_eq!(
            problems(&data.dir(Part::Design), "short-row"),
            [DesignError::TooLarge { file: "design.toml".into(), limit: "64 KB".into() }]
        );
    }

    #[test]
    fn the_preview_is_a_small_png() {
        let data = Data::with(Part::Design, &["form"]);
        let dir = data.dir(Part::Design);
        let png = |w: u32, h: u32| {
            let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
            bytes.extend(w.to_be_bytes());
            bytes.extend(h.to_be_bytes());
            bytes
        };
        let preview = data.entry(Part::Design, "form").join("preview.png");
        fs::write(&preview, png(1200, 800)).expect("write");
        let design = load(&dir, "form").expect("valid");
        assert_eq!(design.preview, Some(fs::canonicalize(&preview).expect("canonical")));
        fs::write(&preview, png(1600, 900)).expect("write");
        assert_eq!(problems(&dir, "form"), [DesignError::PreviewSize { width: 1600, height: 900 }]);
        fs::write(&preview, b"GIF89a").expect("write");
        assert!(matches!(problems(&dir, "form")[..], [DesignError::Unreadable { .. }]));
    }

    #[test]
    fn list_shows_valid_and_broken_designs_with_reasons() {
        let data = Data::with(Part::Design, &["form", "short-row", "unknown-key"]);
        let dir = data.dir(Part::Design);
        fs::create_dir(dir.join("Bad_Name")).expect("dir");
        fs::create_dir(dir.join(".hidden")).expect("dir");
        fs::create_dir(dir.join(STANDARD_ID)).expect("dir");
        fs::write(dir.join("notes.txt"), "not a design").expect("write");
        let listed = list(&dir);
        let ids: Vec<&str> = listed.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, [STANDARD_ID, "Bad_Name", "form", "short-row", STANDARD_ID, "unknown-key"]);
        let valid: Vec<bool> = listed.iter().map(Listed::valid).collect();
        assert_eq!(valid, [true, false, true, false, false, false]);
        assert_eq!(listed[2].about.name, "Form");
        assert_eq!(listed[3].about.name, "Short row", "a broken design still shows its name");
        assert_eq!(
            listed[3].problems(),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 32.0, max: 64.0 }]
        );
        assert_eq!(listed[4].problems(), [DesignError::BadId { id: STANDARD_ID.into() }]);
        assert_eq!(listed[5].about.name, "unknown-key", "unreadable: the id stands in for the name");
        assert!(listed.iter().skip(1).all(|l| !l.standard));
    }

    fn parse_only(text: &str) -> Result<DesignFile, Vec<DesignError>> {
        parse("design.toml", text, known)
    }

    #[test]
    fn unknown_keys_are_named_with_their_path() {
        let text = "format = 1\ncolour = 1\n[size]\nrowx = 44\n[about]\nfonts = \"x\"\n[type.bodyy]\nsize = 3\n\
                    [colors.dark]\naccnt = \"#000000\"\n";
        let Err(problems) = parse_only(text) else { panic!("unknown keys must fail") };
        let keys: Vec<String> = problems
            .into_iter()
            .map(|p| match p {
                DesignError::UnknownKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            keys,
            ["about.fonts", "colors", "colour", "size.rowx", "type.bodyy"],
            "an unknown table is one key, not looked into"
        );
    }

    #[test]
    fn type_errors_carry_the_file_and_the_line() {
        let Err(problems) = parse_only("format = 1\n\n[type.body]\nsize = \"big\"\n") else { panic!("must fail") };
        assert!(
            matches!(&problems[..], [DesignError::Syntax { file, line: Some(4), .. }] if file == "design.toml"),
            "{problems:?}"
        );
        let Err(problems) = parse_only("format = = 1") else { panic!("must fail") };
        assert!(matches!(&problems[..], [DesignError::Syntax { line: Some(1), .. }]), "{problems:?}");
        let Err(problems) = parse_only("[space]\nscale = [1, 2]\n") else { panic!("must fail") };
        assert!(matches!(&problems[..], [DesignError::Syntax { .. }]), "{problems:?}");
        let Err(problems) = parse_only("[colors.dark]\naccent = 3\n[fonts]\ntext = 1\n") else { panic!("must fail") };
        assert_eq!(
            problems,
            [DesignError::UnknownKey { key: "colors".into() }, DesignError::UnknownKey { key: "fonts".into() }],
            "old sections are unknown keys before their values are read"
        );
    }

    #[test]
    fn integers_and_floats_both_work_for_numbers() {
        let file = parse_only("[type.body]\nsize = 14\nline = 21.5\n[size]\nrow = 44\n").expect("valid");
        assert_eq!(file.typo["body"].size, Some(14.0));
        assert_eq!(file.typo["body"].line, Some(21.5));
        assert_eq!(file.size["row"], 44.0);
    }

    /// Task E8.8b: the designs in the repository's `designs/` stay valid. Task E13.1: none of them
    /// sets `layout.density`, so every shipped design shows rows – a design that switched to cards
    /// would look unlike its own `preview.png`. Since task L.20 they hold the form only.
    #[test]
    fn the_shipped_designs_stay_valid() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../designs");
        let listed = list(&shipped);
        assert!(listed.len() > 1, "the designs are there");
        for design in listed {
            let Ok(found) = &design.result else { panic!("{}: {:?}", design.id, design.result.err()) };
            assert_eq!(found.layout.density, Density::Compact, "{} ships cards", design.id);
        }
    }

    /// The fixture designs as the table test in `design.rs` sees them: every one listed, the old
    /// packs among them too (task L.21).
    #[test]
    fn every_fixture_design_is_listed() {
        let data = Data::all();
        let listed: Vec<String> = list(&data.dir(Part::Design)).into_iter().skip(1).map(|entry| entry.id).collect();
        assert_eq!(listed, fixture_names(Part::Design));
    }
}
