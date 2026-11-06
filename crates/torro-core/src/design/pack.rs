//! Reading the folders of fonts and colour schemes, and the built-in designs.
//!
//! Shared by fonts and colour schemes: [`Listed`], the listing of a folder ([`listing`]), opening one
//! entry ([`open`]), the three passes over a TOML file ([`parse`]: TOML at all, then every unknown key,
//! then the typed values with their positions) and `[about]` ([`AboutFile`]). Fonts are read in
//! `font.rs`, colour schemes in `scheme.rs`.
//!
//! Designs are built in only (issue #484, [`BUILT_IN_IDS`]): the files `designs/<id>/design.toml` of
//! the repository, embedded at build time ([`BUILT_IN_FILES`]). No designs folder is read any more.
//! The standard must set every key and is read onto nothing ([`built_in_standard`]); Comfort and Dense
//! are merged into the standard and set only spacing. A `design.toml` holds the form only and is held
//! to the design rules: `[fonts.*]`, `[colors.*]`, `[logo]`, `[about] font_license`, `[about] font` and
//! `[about] mail_view` are unknown keys like any other.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::files::Root;
use super::{
    About, Design, DesignError, FontRole, Layout, Level, ListLook, ListSelection, ListStyle, Pad, Part, Radius,
    STANDARD_ID, Size, Space, TypeScale, Weight, is_valid_id, rules,
};
use crate::i18n::tr;

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

/// Every design, in the order of [`BUILT_IN_IDS`] (Standard, Comfort, Dense; only the standard has
/// `standard: true`). All are built in (issue #484), so each is valid; nothing is read from the disk.
///
/// [`BUILT_IN_IDS`]: super::BUILT_IN_IDS
pub fn list() -> Vec<Listed<Design>> {
    BUILT_IN_FILES
        .iter()
        .map(|(id, text)| {
            let mut about = About { name: id.to_string(), ..About::default() };
            let result = if *id == STANDARD_ID { Ok(Design::standard()) } else { built_in(id, text, &mut about) };
            if let Ok(design) = &result {
                about = design.about.clone();
            }
            Listed { id: id.to_string(), about, standard: *id == STANDARD_ID, result }
        })
        .collect()
}

/// The built-in design `id` ([`BUILT_IN_IDS`]); [`STANDARD_ID`] gives [`Design::standard`]. Another
/// id – also `compact` or `console`, which [`super::known_id`] maps first – is
/// [`DesignError::NotFound`].
///
/// `Err` holds every reason the design can't be used (at least one); the caller then keeps the design
/// it has (the concept: a part is never applied in part).
///
/// [`BUILT_IN_IDS`]: super::BUILT_IN_IDS
pub fn load(id: &str) -> Result<Design, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(Design::standard());
    }
    match BUILT_IN_FILES.iter().find(|(built_in, _)| *built_in == id) {
        Some((id, text)) => built_in(id, text, &mut About::default()),
        None => Err(vec![DesignError::NotFound { part: Part::Design, id: id.to_string() }]),
    }
}

// ---------------------------------------------------------------------------------------------
// Built-in designs
// ---------------------------------------------------------------------------------------------

/// The files of the built-in designs, in the order of [`BUILT_IN_IDS`]: `(id, design.toml)`.
///
/// [`BUILT_IN_IDS`]: super::BUILT_IN_IDS
pub(super) const BUILT_IN_FILES: [(&str, &str); 3] = [
    (STANDARD_ID, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/torro/design.toml"))),
    ("comfort", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/comfort/design.toml"))),
    ("dense", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/dense/design.toml"))),
];

/// The names and descriptions of the built-in designs, English as their files say them: each is handed
/// to `tr()`, so the translation in `i18n/de.rs` finds it ([`built_in_about`]); since issue #484 the
/// names are words of the suite ("Dense" is "Dicht"), not names of packs. A text that is not here is
/// shown as written.
const BUILT_IN_TEXTS: [&str; 6] = [
    "Standard",
    "Comfort",
    "Dense",
    "The suite's standard spacing",
    "Lots of room around text and elements",
    "Tightly packed, more on the screen",
];

/// The standard design from its embedded file ([`Design::standard`] keeps it). The file must set every
/// key ([`missing`]); it is read onto a design of zeros, so a key it leaves out can't hide behind a
/// value in the code, and then held to the rules like any design.
pub(super) fn built_in_standard() -> Result<Design, Vec<DesignError>> {
    let (id, text) = BUILT_IN_FILES[0];
    let mut about = About::default();
    let (file, mut problems) = from_text(id, text, &mut about)?;
    problems.extend(missing(&file));
    if !problems.is_empty() {
        return Err(problems);
    }
    let mut design = merge(blank(id), &file);
    design.about = built_in_about(about);
    finish(design, problems)
}

/// The built-in design `id` (not the standard) from its embedded `text`, merged with the standard and
/// checked; `about` as far as the file could be read.
fn built_in(id: &str, text: &str, about: &mut About) -> Result<Design, Vec<DesignError>> {
    let (file, problems) = from_text(id, text, about)?;
    *about = built_in_about(about.clone());
    let mut design = merge(Design::standard(), &file);
    design.id = id.to_string();
    design.about = about.clone();
    finish(design, problems)
}

/// What a built-in design says about itself: its file's `[about]` with the suite's version (it ships
/// with the suite), and its name and description in the language of the process ([`BUILT_IN_TEXTS`]).
fn built_in_about(about: About) -> About {
    let translated = |english: String| {
        BUILT_IN_TEXTS.iter().find(|known| **known == english).map_or(english.clone(), |known| tr(known).to_string())
    };
    About {
        name: translated(about.name),
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: about.description.map(translated),
        ..about
    }
}

/// A design with every value zero: the ground the complete standard is read onto.
fn blank(id: &str) -> Design {
    Design {
        id: id.to_string(),
        about: About::default(),
        typo: TypeScale::default(),
        space: Space::default(),
        radius: Radius::default(),
        layout: Layout::default(),
        size: Size::default(),
        list: ListLook::default(),
    }
}

/// Every key of the format `file` leaves out, as [`DesignError::MissingKey`] (`type.body.weight`,
/// `size.rail`, `list` …): what the standard's file must hold.
fn missing(file: &DesignFile) -> Vec<DesignError> {
    let mut keys: Vec<String> = Vec::new();
    let mut each = |table: &str, fields: &[(&str, bool)]| {
        keys.extend(fields.iter().filter(|(_, set)| !set).map(|(key, _)| format!("{table}.{key}")));
    };
    for level in LEVEL_KEYS {
        match file.typo.get(level) {
            Some(l) => each(
                &format!("type.{level}"),
                &[
                    ("font", l.font.is_some()),
                    ("size", l.size.is_some()),
                    ("line", l.line.is_some()),
                    ("weight", l.weight.is_some()),
                ],
            ),
            None => each("type", &[(level, false)]),
        }
    }
    each("space", &[("scale", file.space.is_some())]);
    each("radius", &[("scale", file.radius.is_some())]);
    let layout = file.layout.as_ref();
    let set = |field: fn(&LayoutFile) -> bool| layout.is_some_and(field);
    each(
        "layout",
        &[
            ("inset", set(|l| l.inset.is_some())),
            ("page_padding", set(|l| l.page_padding.is_some())),
            ("panel_padding", set(|l| l.panel_padding.is_some())),
            ("row_padding", set(|l| l.row_padding.is_some())),
            ("control_padding", set(|l| l.control_padding.is_some())),
            ("section_gap", set(|l| l.section_gap.is_some())),
            ("item_gap", set(|l| l.item_gap.is_some())),
            ("reading_padding", set(|l| l.reading_padding.is_some())),
        ],
    );
    let sizes: Vec<(&str, bool)> =
        Size::default().entries().iter().map(|(key, _)| (*key, file.size.contains_key(*key))).collect();
    each("size", &sizes);
    let list = file.list.as_ref();
    let set = |field: fn(&ListFile) -> bool| list.is_some_and(field);
    each(
        "list",
        &[
            ("style", set(|l| l.style.is_some())),
            ("separators", set(|l| l.separators.is_some())),
            ("selection", set(|l| l.selection.is_some())),
            ("avatar", set(|l| l.avatar.is_some())),
            ("preview_lines", set(|l| l.preview_lines.is_some())),
        ],
    );
    keys.into_iter().map(|key| DesignError::MissingKey { key }).collect()
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
}

impl AboutFile {
    /// What the list shows: the name (else `id`) and the version as far as they are there.
    pub(super) fn about(&self, id: &str) -> About {
        About {
            name: self.name.clone().unwrap_or_else(|| id.to_string()),
            version: self.version.clone().unwrap_or_default(),
            author: self.author.clone(),
            description: self.description.clone(),
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
    list: Option<ListFile>,
}

/// `[list]`: words for `style` and `selection`, so a word the format does not know fails in the third
/// pass with its line, as `type.*.font` does.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListFile {
    style: Option<ListStyle>,
    separators: Option<bool>,
    selection: Option<ListSelection>,
    avatar: Option<f32>,
    preview_lines: Option<u8>,
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
    reading_padding: Option<[f32; 2]>,
}

const LEVEL_KEYS: [&str; 11] =
    ["micro", "caption", "meta", "body", "body_strong", "read", "title", "headline", "display", "hero", "countdown"];
const LAYOUT_KEYS: [&str; 8] = [
    "inset",
    "page_padding",
    "panel_padding",
    "row_padding",
    "control_padding",
    "section_gap",
    "item_gap",
    "reading_padding",
];
const LIST_KEYS: [&str; 5] = ["style", "separators", "selection", "avatar", "preview_lines"];

/// The keys of `design.toml`: the form only, so the fonts and colours of an old complete pack are
/// unknown keys.
fn known(path: &str, key: &str) -> Option<bool> {
    const TABLE: Option<bool> = Some(true);
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" => Some(false),
            "about" | "type" | "space" | "radius" | "layout" | "size" | "list" => TABLE,
            _ => None,
        },
        // `font` and `mail_view` are unknown since issue #484: a design suggests nothing any more.
        "about" => value(["name", "version", "author", "description"].contains(&key)),
        "type" => LEVEL_KEYS.contains(&key).then_some(true),
        p if p.strip_prefix("type.").is_some_and(|level| LEVEL_KEYS.contains(&level)) => {
            value(["font", "size", "line", "weight"].contains(&key))
        }
        "space" | "radius" => value(key == "scale"),
        "layout" => value(LAYOUT_KEYS.contains(&key)),
        "list" => value(LIST_KEYS.contains(&key)),
        // The keys, not the values: the standard is itself read with this function.
        "size" => value(Size::default().entries().iter().any(|(name, _)| *name == key)),
        _ => None,
    }
}

/// The three passes over the `design.toml` `text` of `id`, then `about` (as far as it was read) and the
/// header; the file and what it breaks so far. `Err` when it can't be read at all.
fn from_text(id: &str, text: &str, about: &mut About) -> Result<(DesignFile, Vec<DesignError>), Vec<DesignError>> {
    let file: DesignFile = parse(Part::Design.file(), text, known)?;
    *about = file.about.about(id);
    let mut problems = Vec::new();
    header(file.format, &file.about, &mut problems);
    Ok((file, problems))
}

/// `design` held to the rules, with the `problems` found before.
fn finish(design: Design, mut problems: Vec<DesignError>) -> Result<Design, Vec<DesignError>> {
    problems.extend(rules::design(&design));
    if problems.is_empty() { Ok(design) } else { Err(problems) }
}

/// The form of `file` over `base`'s (the standard's, or zeros for the standard itself).
fn merge(mut design: Design, file: &DesignFile) -> Design {
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
    if let Some(list) = &file.list {
        merge_list(&mut design.list, list);
    }
    design
}

fn merge_list(look: &mut ListLook, file: &ListFile) {
    look.style = file.style.unwrap_or(look.style);
    look.separators = file.separators.unwrap_or(look.separators);
    look.selection = file.selection.unwrap_or(look.selection);
    look.avatar = file.avatar.unwrap_or(look.avatar);
    look.preview_lines = file.preview_lines.unwrap_or(look.preview_lines);
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
    l.reading_padding = pad(file.reading_padding, l.reading_padding);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::testdata::standard_of_the_table;
    use crate::design::{BUILT_IN_IDS, Density, DensityLevel};

    /// The design `id` read from `body` after `format` and `[about]`, as a built-in file is read: merged
    /// into the standard and held to the rules. The rules and the format are tested this way since
    /// issue #484, when the designs folder went away.
    fn from_toml(id: &str, body: &str) -> Result<Design, Vec<DesignError>> {
        let toml = format!("format = 1\n[about]\nname = \"{id}\"\nversion = \"1.0.0\"\n{body}");
        built_in(id, &toml, &mut About::default())
    }

    fn problems(id: &str, body: &str) -> Vec<DesignError> {
        match from_toml(id, body) {
            Ok(_) => panic!("{id} must be refused"),
            Err(problems) => problems,
        }
    }

    /// Issue #484: the three levels, in their order, each valid, only the standard the standard; their
    /// names are words of the suite and translated.
    #[test]
    fn the_three_levels_are_listed_in_order() {
        assert_eq!(load(STANDARD_ID), Ok(Design::standard()));
        let listed = list();
        let ids: Vec<&str> = listed.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, BUILT_IN_IDS);
        assert!(listed.iter().all(|entry| entry.valid() && entry.problems().is_empty()));
        let standard: Vec<bool> = listed.iter().map(|entry| entry.standard).collect();
        assert_eq!(standard, [true, false, false], "only the standard is the standard");
        let names: Vec<&str> = listed.iter().map(|entry| entry.about.name.as_str()).collect();
        assert_eq!(names, [tr("Standard"), tr("Comfort"), tr("Dense")]);
    }

    /// Issue #484: an id that is no built-in design is not there – also the old `compact` and
    /// `console` (`known_id` maps them before anything is loaded).
    #[test]
    fn other_designs_are_not_found() {
        for id in ["compact", "console", "plex", "../etc"] {
            assert_eq!(load(id), Err(vec![DesignError::NotFound { part: Part::Design, id: id.into() }]), "{id}");
        }
    }

    /// Issue #291: the built-in designs load from their embedded files without a reason, in the order
    /// of [`BUILT_IN_IDS`], without touching the disk; the standard's file sets every key.
    #[test]
    fn the_built_in_designs_load_without_reasons() {
        let ids: Vec<&str> = BUILT_IN_FILES.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, BUILT_IN_IDS);
        for (id, text) in BUILT_IN_FILES {
            let loaded = load(id);
            let Ok(design) = &loaded else { panic!("{id}: {:?}", loaded.err()) };
            assert_eq!(design.id, id);
            assert_eq!(design.about.version, env!("CARGO_PKG_VERSION"), "{id}: ships with the suite");
            // Names and descriptions are translatable texts (`BUILT_IN_TEXTS`).
            let about = parse_only(text).expect("readable").about;
            let english = [about.name.expect("a name"), about.description.expect("a description")];
            assert!(english.iter().all(|text| BUILT_IN_TEXTS.contains(&text.as_str())), "{english:?}");
        }
        let file = parse_only(BUILT_IN_FILES[0].1).expect("readable");
        assert_eq!(missing(&file), [], "the standard sets every key");
        assert_eq!(built_in_standard(), Ok(Design::standard()));
    }

    /// Issue #291, task L.34: the standard gives exactly the values of the table, read from its file;
    /// a mail row of 100 is "balanced".
    #[test]
    fn the_standard_has_the_values_of_the_table() {
        assert_eq!(Design::standard(), standard_of_the_table());
        assert_eq!(Design::standard().density_level(), DensityLevel::Balanced);
    }

    /// A key the standard's file leaves out is named, never filled in from the code.
    #[test]
    fn the_standard_must_set_every_key() {
        let whole = BUILT_IN_FILES[0].1;
        // Without the whole `[list]` table, the weight of one level and one size.
        let cut = whole.find("[list]").map_or(whole, |at| &whole[..at]);
        let text = cut.replacen("weight = \"medium\"\n", "", 1).replace("rail_wide = 248", "");
        let file: DesignFile = toml::from_str(&text).expect("toml");
        let keys: Vec<String> = missing(&file)
            .into_iter()
            .map(|p| match p {
                DesignError::MissingKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(keys.contains(&"type.caption.weight".to_string()), "{keys:?}");
        assert!(keys.contains(&"size.rail_wide".to_string()), "{keys:?}");
        assert!(keys.contains(&"list.style".to_string()) && keys.contains(&"list.avatar".to_string()), "{keys:?}");
        assert_eq!(keys.len(), 2 + 5, "nothing else: {keys:?}");
    }

    /// The sizes a spacing level may set (issue #484): the heights of rows and controls and the gaps
    /// of cards; widths of regions, text and the avatars stay the standard's.
    const SPACING_SIZES: [&str; 17] = [
        "control_sm",
        "control",
        "control_lg",
        "control_xl",
        "row",
        "row_two",
        "row_message",
        "row_notice",
        "row_sidebar",
        "row_group",
        "header",
        "hour",
        "card_gap",
        "command",
        "pill",
        "rail",
        "rail_wide",
    ];

    /// Issue #484: Comfort and Dense set only `[about]`, `[space]`, `[layout]` (without the inset) and
    /// the spacing sizes; text, radii, widths and how lists are drawn are the standard's.
    #[test]
    fn comfort_and_dense_set_only_spacing() {
        let standard = Design::standard();
        for (id, text) in &BUILT_IN_FILES[1..] {
            let file = parse_only(text).expect("readable");
            assert!(file.typo.is_empty() && file.radius.is_none() && file.list.is_none(), "{id}: spacing only");
            assert!(file.layout.as_ref().is_some_and(|l| l.inset.is_none()), "{id}: content stays flush");
            let foreign: Vec<&String> = file.size.keys().filter(|key| !SPACING_SIZES.contains(&key.as_str())).collect();
            assert!(foreign.is_empty(), "{id}: {foreign:?}");
            let design = load(id).expect("built in");
            assert_eq!((design.typo, design.radius, &design.list), (standard.typo, standard.radius, &standard.list));
            let widths = |s: &Size| [s.sidebar, s.props, s.panel, s.list, s.list_medium, s.content_max, s.reading_max];
            assert_eq!(widths(&design.size), widths(&standard.size), "{id}");
            assert_eq!(design.layout.inset, 0.0);
        }
    }

    /// Issue #484: the values of the plan for Comfort and Dense.
    #[test]
    fn comfort_and_dense_have_the_values_of_the_plan() {
        let comfort = load("comfort").expect("comfort");
        assert_eq!(comfort.space, Space { xs: 4.0, s: 10.0, m: 16.0, l: 20.0, xl: 32.0, xxl: 40.0 });
        let l = comfort.layout;
        assert_eq!(
            [l.page_padding, l.panel_padding, l.row_padding, l.control_padding, l.reading_padding],
            [pad(32.0, 32.0), pad(20.0, 20.0), pad(24.0, 32.0), pad(0.0, 16.0), pad(64.0, 80.0)]
        );
        assert_eq!((l.section_gap, l.item_gap), (32.0, 14.0));
        let s = comfort.size;
        assert_eq!([s.control_sm, s.control, s.control_lg, s.control_xl], [40.0, 42.0, 44.0, 52.0]);
        assert_eq!([s.row, s.row_two, s.row_message, s.row_notice], [56.0, 64.0, 116.0, 92.0]);
        assert_eq!([s.row_sidebar, s.row_group, s.header, s.hour, s.card_gap], [36.0, 44.0, 64.0, 56.0, 16.0]);
        assert_eq!([s.command, s.pill, s.rail, s.rail_wide], [50.0, 44.0, 64.0, 264.0]);
        assert_eq!(comfort.density_level(), DensityLevel::Airy);

        let dense = load("dense").expect("dense");
        assert_eq!(dense.space, Space { xs: 2.0, s: 6.0, m: 8.0, l: 12.0, xl: 16.0, xxl: 24.0 });
        let l = dense.layout;
        assert_eq!(
            [l.page_padding, l.panel_padding, l.row_padding, l.control_padding, l.reading_padding],
            [pad(16.0, 16.0), pad(12.0, 12.0), pad(6.0, 16.0), pad(0.0, 8.0), pad(32.0, 40.0)]
        );
        assert_eq!((l.section_gap, l.item_gap), (16.0, 6.0));
        let s = dense.size;
        assert_eq!([s.control_sm, s.control, s.control_lg, s.control_xl], [30.0, 32.0, 36.0, 44.0]);
        assert_eq!([s.row, s.row_two, s.row_message, s.row_notice], [40.0, 48.0, 78.0, 56.0]);
        assert_eq!([s.row_sidebar, s.row_group, s.header, s.hour, s.card_gap], [28.0, 32.0, 48.0, 40.0, 8.0]);
        assert_eq!([s.command, s.pill, s.rail, s.rail_wide], [40.0, 36.0, 52.0, 232.0]);
        assert_eq!(dense.density_level(), DensityLevel::Dense);
        assert_eq!(dense.list.density(), Density::Compact, "rows, never the old single line");
    }

    fn pad(vertical: f32, horizontal: f32) -> Pad {
        Pad { vertical, horizontal }
    }

    /// Task L.19: a `design.toml` with nothing but `[about]` is a design that looks like Torro.
    #[test]
    fn only_about_is_the_standards_form() {
        let design = from_toml("plain", "").expect("valid");
        let mut expected = Design::standard();
        expected.id = "plain".into();
        expected.about = About { name: "plain".into(), version: env!("CARGO_PKG_VERSION").into(), ..About::default() };
        assert_eq!(design, expected);
    }

    /// Every key of the format is read and merged.
    #[test]
    fn a_design_with_every_key_loads() {
        let body = "[type.title]\nfont = \"heading\"\nsize = 16\nline = 22\nweight = \"semibold\"\n\
                    [type.hero]\nsize = 44\nline = 52\nweight = \"bold\"\n\
                    [radius]\nscale = [3, 5, 7, 9]\n\
                    [layout]\ninset = 10\npage_padding = [24, 32]\nitem_gap = 10\nreading_padding = [40, 48]\n\
                    [size]\nrow = 44\nsidebar = 248\naxis = 56\nrail = 56\nrail_wide = 240\nreading_max = 680\n\
                    [list]\nstyle = \"cards\"\nseparators = false\nselection = \"fill\"\navatar = 36\npreview_lines = 2\n";
        let design = from_toml("form", body).expect("valid");
        assert_eq!(
            design.typo.title,
            Level { font: FontRole::Heading, weight: Weight::Semibold, size: 16.0, line: 22.0 }
        );
        assert_eq!(design.typo.hero.weight, Weight::Bold, "any weight: the font replaces what it lacks");
        assert_eq!(design.radius, Radius { xs: 3.0, sm: 5.0, md: 7.0, lg: 9.0 });
        assert_eq!(design.layout.page_padding, Pad { vertical: 24.0, horizontal: 32.0 });
        assert_eq!((design.layout.inset, design.layout.item_gap), (10.0, 10.0));
        assert_eq!(design.layout.reading_padding, Pad { vertical: 40.0, horizontal: 48.0 });
        assert_eq!((design.size.row, design.size.sidebar, design.size.axis), (44.0, 248.0, 56.0));
        assert_eq!((design.size.rail, design.size.rail_wide, design.size.reading_max), (56.0, 240.0, 680.0));
        let look = ListLook {
            style: ListStyle::Cards,
            separators: false,
            selection: ListSelection::Fill,
            avatar: 36.0,
            preview_lines: 2,
        };
        assert_eq!(design.list, look);
        assert_eq!(design.list.density(), Density::Comfortable);
    }

    /// Task E8.2: the reading level and the sizes of the comfortable density are keys of the format.
    #[test]
    fn the_comfortable_tokens_are_read() {
        let body = "[type.read]\nsize = 15\nline = 24\n\
                    [size]\ncard_gap = 16\navatar = 40\navatar_lg = 44\ntile_sm = 22\ntile = 30\ntile_lg = 40\n";
        let design = from_toml("roomy", body).expect("valid");
        assert_eq!(design.typo.read, Level { font: FontRole::Text, weight: Weight::Regular, size: 15.0, line: 24.0 });
        let s = design.size;
        assert_eq!(
            [s.card_gap, s.avatar, s.avatar_lg, s.tile_sm, s.tile, s.tile_lg],
            [16.0, 40.0, 44.0, 22.0, 30.0, 40.0]
        );
    }

    /// Issue #291: the density follows `[list] style` – `cards` gives cards, a design that leaves the
    /// style out keeps the standard's rows – and `layout.density` is an unknown key. A word the format
    /// does not know is refused with its line.
    #[test]
    fn the_list_style_gives_the_density_and_layout_density_is_gone() {
        let cards = from_toml("cards", "[list]\nstyle = \"cards\"\n").expect("valid");
        assert_eq!(cards.list.density(), Density::Comfortable);

        let rows = from_toml("rows", "[layout]\nitem_gap = 10\n").expect("valid");
        assert_eq!(rows.list, Design::standard().list, "left out: the standard's list stands");
        assert_eq!(rows.list.density(), Density::Compact);
        assert_eq!(rows.layout.item_gap, 10.0, "the rest of the table is still read");

        let old = problems("old", "[layout]\ndensity = \"comfortable\"\n");
        assert_eq!(old, [DesignError::UnknownKey { key: "layout.density".into() }]);

        for body in ["[list]\nstyle = \"grid\"\n", "[list]\nselection = \"outline\"\n"] {
            let refused = problems("word", body);
            assert!(matches!(&refused[..], [DesignError::Syntax { line: Some(_), .. }]), "{body}: {refused:?}");
        }
        let misspelt = problems("misspelt", "[list]\nstyles = \"rows\"\n");
        assert_eq!(misspelt, [DesignError::UnknownKey { key: "list.styles".into() }]);
    }

    /// Issue #484 (task L.33 for the colours): a design suggests nothing – neither a colour scheme, nor
    /// a font, nor a mail view; each is an unknown key.
    #[test]
    fn a_design_suggests_nothing() {
        for key in ["color_scheme = \"nord\"", "font = \"plex\"", "mail_view = \"text\""] {
            let toml = format!("format = 1\n[about]\nname = \"x\"\nversion = \"1\"\n{key}\n");
            let Err(problems) = built_in("x", &toml, &mut About::default()) else { panic!("{key} must be refused") };
            let name = key.split(' ').next().unwrap_or_default();
            assert_eq!(problems, [DesignError::UnknownKey { key: format!("about.{name}") }]);
        }
    }

    /// Issue #291: the numbers of `[list]` are held to their rules when read from a file.
    #[test]
    fn the_list_numbers_are_checked() {
        assert_eq!(
            problems("odd", "[list]\navatar = 12\npreview_lines = 3\n"),
            [
                DesignError::OffOrRange { key: "list.avatar".into(), value: 12.0, min: 20.0, max: 48.0 },
                DesignError::OutOfRange { key: "list.preview_lines".into(), value: 3.0, min: 1.0, max: 2.0 },
            ]
        );
        let off = from_toml("off", "[list]\navatar = 0\n").expect("valid");
        assert_eq!(off.list.avatar, 0.0, "0 switches the avatar off");
    }

    /// Task L.21: no carry-over of old packs. The fonts, colours and logo of a pack from before task
    /// L.19 are unknown keys in `design.toml` like any other.
    #[test]
    fn the_sections_of_an_old_pack_are_unknown_keys() {
        let unknown = |keys: &[&str]| -> Vec<DesignError> {
            keys.iter().map(|key| DesignError::UnknownKey { key: (*key).into() }).collect()
        };
        let toml = "format = 1\n[about]\nname = \"Full\"\nversion = \"1\"\nfont_license = \"OFL\"\n\
                    [fonts.text]\nfamily = \"Inter\"\n[colors.dark]\naccent = \"#0E7C7B\"\n[logo]\nmark = \"logo.svg\"\n";
        let Err(full) = built_in("full", toml, &mut About::default()) else { panic!("must be refused") };
        assert_eq!(full, unknown(&["about.font_license", "colors", "fonts", "logo"]));
        let dark = problems("dark", "[size]\nrow = 44\n[colors.dark]\naccent = \"#0E7C7B\"\n");
        assert_eq!(dark, unknown(&["colors"]), "a design with colours is unusable, whatever else it holds");
    }

    #[test]
    fn each_design_rule_has_a_broken_example() {
        assert_eq!(
            problems("tiny-type", "[type.caption]\nsize = 9\n"),
            [DesignError::OutOfRange { key: "type.caption.size".into(), value: 9.0, min: 11.0, max: 40.0 }]
        );
        assert_eq!(
            problems("short-row", "[size]\nrow = 20\n"),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 24.0, max: 64.0 }]
        );
        assert_eq!(
            problems("unknown-key", "[size]\nrowx = 44\n"),
            [DesignError::UnknownKey { key: "size.rowx".into() }]
        );
        let Err(format_2) =
            built_in("format-2", "format = 2\n[about]\nname = \"F\"\nversion = \"1\"\n", &mut About::default())
        else {
            panic!("format 2 must be refused")
        };
        assert_eq!(format_2, [DesignError::UnsupportedFormat { found: 2 }]);
        assert_eq!(
            problems("narrow-axis", "[size]\naxis_narrow = 40\n"),
            [DesignError::OutOfRange { key: "size.axis_narrow".into(), value: 40.0, min: 44.0, max: 120.0 }]
        );
    }

    #[test]
    fn required_keys_are_named() {
        let Err(problems) = built_in("bare", "[about]\nauthor = \"Nobody\"\n", &mut About::default()) else {
            panic!("must be refused")
        };
        let keys: Vec<String> = problems
            .into_iter()
            .map(|p| match p {
                DesignError::MissingKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["format", "about.name", "about.version"]);
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
}
