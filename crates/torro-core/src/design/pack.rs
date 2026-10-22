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
//! no carry-over of old packs). `layout.density` is unknown since issue #291: `[list] style = "cards"`
//! took its place.
//!
//! The built-in designs (issue #291, [`BUILT_IN_IDS`]) are read here too, from the files
//! `designs/<id>/design.toml` of the repository embedded at build time ([`BUILT_IN_FILES`]): the
//! standard must set every key and is read onto nothing ([`built_in_standard`]), Compact and Console
//! are merged into the standard like a folder's design.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::files::Root;
use super::{
    About, BUILT_IN_IDS, Design, DesignError, FontRole, Layout, Level, ListLook, ListSelection, ListStyle, Pad, Part,
    Radius, STANDARD_ID, Size, Space, TypeScale, Weight, is_valid_id, rules,
};
use crate::i18n::tr;
use crate::settings::MailFormat;

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

/// Every design: the built-in ones first in the order of [`BUILT_IN_IDS`] (Standard, Compact,
/// Console; only the standard has `standard: true`), then each folder under `dir` (the designs
/// folder, see [`super::dir`]) by name, valid or with its reasons. Hidden folders (`.git`) and folder
/// names that are not UTF-8 are left out; a missing `dir` lists only the built-in designs. A folder
/// with a built-in id is listed with [`DesignError::BadId`].
pub fn list(dir: &Path) -> Vec<Listed<Design>> {
    let standard = Design::standard();
    let mut listed = listing(dir, standard.about.clone(), standard, read);
    let others = BUILT_IN_FILES.iter().filter(|(id, _)| *id != STANDARD_ID).map(|(id, text)| {
        let mut about = About { name: id.to_string(), ..About::default() };
        let result = built_in(id, text, &mut about);
        Listed { id: id.to_string(), about, standard: false, result }
    });
    listed.splice(1..1, others);
    listed
}

/// The design `id` from the designs folder `dir`, merged with the standard and checked. A built-in id
/// ([`BUILT_IN_IDS`]) gives the built-in design without touching the disk; [`STANDARD_ID`] gives
/// [`Design::standard`].
///
/// `Err` holds every reason the design can't be used (at least one); the caller then keeps the design
/// it has (the concept: a part is never applied in part).
pub fn load(dir: &Path, id: &str) -> Result<Design, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(Design::standard());
    }
    if let Some((id, text)) = BUILT_IN_FILES.iter().find(|(built_in, _)| *built_in == id) {
        return built_in(id, text, &mut About::default());
    }
    read(dir, id, &mut About::default())
}

// ---------------------------------------------------------------------------------------------
// Built-in designs
// ---------------------------------------------------------------------------------------------

/// The files of the built-in designs, in the order of [`BUILT_IN_IDS`]: `(id, design.toml)`.
pub(super) const BUILT_IN_FILES: [(&str, &str); 3] = [
    (STANDARD_ID, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/torro/design.toml"))),
    ("compact", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/compact/design.toml"))),
    ("console", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../designs/console/design.toml"))),
];

/// The descriptions of the built-in designs, English as their files say them: each is handed to
/// `tr()`, so the translation in `i18n/de.rs` finds it ([`built_in_about`]). A description that is not
/// here is shown as written. Names are not translated: they are names.
const BUILT_IN_DESCRIPTIONS: [&str; 4] = [
    "Inter, quiet surfaces, Torro red",
    "Calm and open: flush content, hairlines, a reading column with wide margins",
    "Standard, built tighter",
    "Very dense and square, one line per mail",
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
/// with the suite) and its description in the language of the process ([`BUILT_IN_DESCRIPTIONS`]).
fn built_in_about(about: About) -> About {
    let description = about.description.map(|english| {
        BUILT_IN_DESCRIPTIONS
            .iter()
            .find(|known| **known == english)
            .map_or(english.clone(), |known| tr(known).to_string())
    });
    About { version: env!("CARGO_PKG_VERSION").to_string(), description, ..about }
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
        preview: None,
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
    /// Designs only: the suggested font. (`color_scheme` is unknown since task L.33: colours never
    /// belong to a design.)
    pub(super) font: Option<String>,
    /// Designs only: the suggested way to show mail (`"html"`, `"text"`); another word fails in the
    /// third pass with its line.
    pub(super) mail_view: Option<MailFormat>,
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
            mail_view: self.mail_view,
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
        "about" => value(["name", "version", "author", "description", "font", "mail_view"].contains(&key)),
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

fn read(dir: &Path, id: &str, about: &mut About) -> Result<Design, Vec<DesignError>> {
    // `torro` is no valid id at all; the other built-in ids are valid names, but taken.
    if BUILT_IN_IDS.contains(&id) {
        return Err(vec![DesignError::BadId { id: id.to_string() }]);
    }
    let root = open(dir, id, Part::Design)?;
    let text = root.toml(Part::Design.file()).map_err(|err| vec![err])?;
    let (file, mut problems) = from_text(id, &text, about)?;
    let mut design = merge(Design::standard(), &file);
    design.id = id.to_string();
    design.about = about.clone();
    match root.preview() {
        Ok(preview) => design.preview = preview,
        Err(found) => problems.extend(found),
    }
    finish(design, problems)
}

/// The three passes over the `design.toml` `text` of `id`, then `about` (as far as it was read), the
/// header and the suggestions; the file and what it breaks so far. `Err` when it can't be read at all.
fn from_text(id: &str, text: &str, about: &mut About) -> Result<(DesignFile, Vec<DesignError>), Vec<DesignError>> {
    let file: DesignFile = parse(Part::Design.file(), text, known)?;
    *about = file.about.about(id);
    let mut problems = Vec::new();
    header(file.format, &file.about, &mut problems);
    if let Some(suggested) = &file.about.font
        && !(is_valid_id(suggested) || suggested == STANDARD_ID)
    {
        problems.push(DesignError::BadSuggestion { key: "about.font".into(), id: suggested.clone() });
    }
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
    use crate::design::testdata::{Data, fixture_names, standard_of_before};
    use crate::design::{Density, DensityLevel};

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
        let ids: Vec<&str> = listed.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, BUILT_IN_IDS, "a missing folder lists only the built-in designs");
        assert!(listed.iter().all(|entry| entry.valid() && entry.problems().is_empty()));
        let standard: Vec<bool> = listed.iter().map(|entry| entry.standard).collect();
        assert_eq!(standard, [true, false, false], "only the standard is the standard");
        let names: Vec<&str> = listed.iter().map(|entry| entry.about.name.as_str()).collect();
        assert_eq!(names, ["Standard", "Compact", "Console"]);
    }

    /// Issue #291: the three built-in designs load from their embedded files without a reason, in the
    /// order of [`BUILT_IN_IDS`], without touching the disk; the standard's file sets every key.
    #[test]
    fn the_built_in_designs_load_without_reasons() {
        let ids: Vec<&str> = BUILT_IN_FILES.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, BUILT_IN_IDS);
        let nowhere = Path::new("/nonexistent/torro-designs");
        for (id, text) in BUILT_IN_FILES {
            let loaded = load(nowhere, id);
            let Ok(design) = &loaded else { panic!("{id}: {:?}", loaded.err()) };
            assert_eq!(design.id, id);
            assert_eq!(design.about.version, env!("CARGO_PKG_VERSION"), "{id}: ships with the suite");
            let file = parse_only(text).expect("readable");
            assert_eq!(missing(&file), [], "{id} sets every key, so it does not move with the standard");
        }
        assert_eq!(built_in_standard(), Ok(Design::standard()));
        // The descriptions are translatable texts (`BUILT_IN_DESCRIPTIONS`).
        for (_, text) in BUILT_IN_FILES {
            let english = parse_only(text).expect("readable").about.description.expect("a description");
            assert!(BUILT_IN_DESCRIPTIONS.contains(&english.as_str()), "{english}");
        }
    }

    /// Issue #291, task L.31: until task L.34 the standard gives exactly the form of before, read from
    /// its file.
    #[test]
    fn the_standard_has_the_values_of_before() {
        assert_eq!(Design::standard(), standard_of_before());
        assert_eq!(Design::standard().density_level(), DensityLevel::Balanced);
    }

    /// A key the standard's file leaves out is named, never filled in from the code.
    #[test]
    fn the_standard_must_set_every_key() {
        let whole = BUILT_IN_FILES[0].1;
        // Without the whole `[list]` table, the weight of one level and one size.
        let cut = whole.find("[list]").map_or(whole, |at| &whole[..at]);
        let text = cut.replacen("weight = \"medium\"\n", "", 1).replace("rail_wide = 232", "");
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

    /// Issue #291: Compact and Console have the values of the table in `docs/tasks/design-pakete.md`.
    #[test]
    fn compact_and_console_have_the_values_of_the_table() {
        let nowhere = Path::new("/nonexistent/torro-designs");
        let level = |l: Level| (l.size, l.line);
        let compact = load(nowhere, "compact").expect("compact");
        let t = compact.typo;
        assert_eq!([level(t.body), level(t.body_strong)], [(13.0, 20.0); 2]);
        assert_eq!([level(t.meta), level(t.caption), level(t.read)], [(12.0, 16.0), (11.0, 16.0), (14.0, 22.0)]);
        assert_eq!([level(t.title), level(t.headline)], [(17.0, 24.0), (22.0, 28.0)]);
        let s = compact.size;
        assert_eq!(
            [s.row_message, s.row_sidebar, s.header, s.control_sm, s.control_lg],
            [76.0, 28.0, 48.0, 30.0, 36.0]
        );
        assert_eq!([s.sidebar, s.list, s.rail, s.rail_wide, s.reading_max], [216.0, 400.0, 52.0, 232.0, 640.0]);
        let l = compact.layout;
        assert_eq!(
            (l.inset, l.reading_padding, l.row_padding, l.item_gap),
            (0.0, pad(32.0, 40.0), pad(10.0, 16.0), 8.0)
        );
        assert_eq!(compact.radius, Radius { xs: 4.0, sm: 4.0, md: 4.0, lg: 4.0 });
        let look = ListLook {
            style: ListStyle::Rows,
            separators: true,
            selection: ListSelection::Bar,
            avatar: 28.0,
            preview_lines: 1,
        };
        assert_eq!(compact.list, look);
        assert_eq!((compact.about.font.as_deref(), compact.about.mail_view), (None, None));
        assert_eq!(compact.density_level(), DensityLevel::Dense);

        let console = load(nowhere, "console").expect("console");
        let t = console.typo;
        assert_eq!(
            [level(t.body), level(t.body_strong), level(t.read), level(t.title), level(t.headline)],
            [(12.0, 18.0); 5]
        );
        assert_eq!([level(t.meta), level(t.caption)], [(11.0, 16.0); 2]);
        assert_eq!([t.body_strong.weight, t.title.weight, t.headline.weight], [Weight::Bold; 3]);
        let s = console.size;
        assert_eq!(
            [s.row_message, s.row_notice, s.row_sidebar, s.header, s.control_sm, s.control_lg],
            [24.0, 24.0, 24.0, 32.0, 24.0, 28.0]
        );
        assert_eq!([s.sidebar, s.list, s.rail, s.rail_wide, s.reading_max], [196.0, 600.0, 40.0, 200.0, 1200.0]);
        let l = console.layout;
        assert_eq!((l.inset, l.reading_padding, l.row_padding, l.item_gap), (0.0, pad(14.0, 18.0), pad(0.0, 8.0), 4.0));
        assert_eq!(console.radius, Radius { xs: 0.0, sm: 0.0, md: 0.0, lg: 0.0 });
        let look = ListLook {
            style: ListStyle::Line,
            separators: false,
            selection: ListSelection::Fill,
            avatar: 0.0,
            preview_lines: 1,
        };
        assert_eq!(console.list, look);
        assert_eq!(
            (console.about.font.as_deref(), console.about.mail_view),
            (Some("jetbrains-mono"), Some(MailFormat::Text))
        );
        assert_eq!(console.density_level(), DensityLevel::VeryDense);
    }

    fn pad(vertical: f32, horizontal: f32) -> Pad {
        Pad { vertical, horizontal }
    }

    /// Issue #291: a folder with the id of a built-in design is refused like `torro`, and loading that
    /// id never reads the folder.
    #[test]
    fn a_folder_named_like_a_built_in_design_is_refused() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        for id in ["console", "compact"] {
            written(&data, id, "[size]\nrow = 44\n");
        }
        let listed = list(&dir);
        let entries: Vec<(&str, bool, Vec<DesignError>)> =
            listed.iter().map(|l| (l.id.as_str(), l.valid(), l.problems().to_vec())).collect();
        let bad = |id: &str| vec![DesignError::BadId { id: id.into() }];
        assert_eq!(
            entries,
            [
                (STANDARD_ID, true, vec![]),
                ("compact", true, vec![]),
                ("console", true, vec![]),
                ("compact", false, bad("compact")),
                ("console", false, bad("console")),
            ]
        );
        assert_eq!(load(&dir, "console").expect("built in").size.row, 24.0, "the built-in one, not the folder");
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
        assert_eq!(design.about.font.as_deref(), Some("plex"));
        assert_eq!(design.about.mail_view, Some(MailFormat::Text));
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

    /// Issue #291: the density follows `[list] style` – `cards` gives cards, a design that leaves the
    /// style out keeps the standard's rows – and `layout.density` is an unknown key. A word the format
    /// does not know is refused with its line, for `style`, `selection` and `about.mail_view` alike.
    #[test]
    fn the_list_style_gives_the_density_and_layout_density_is_gone() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        written(&data, "cards", "[list]\nstyle = \"cards\"\n");
        assert_eq!(load(&dir, "cards").expect("valid").list.density(), Density::Comfortable);

        written(&data, "rows", "[layout]\nitem_gap = 10\n");
        let rows = load(&dir, "rows").expect("valid");
        assert_eq!(rows.list, Design::standard().list, "left out: the standard's list stands");
        assert_eq!(rows.list.density(), Density::Compact);
        assert_eq!(rows.layout.item_gap, 10.0, "the rest of the table is still read");

        written(&data, "old", "[layout]\ndensity = \"comfortable\"\n");
        assert_eq!(problems(&dir, "old"), [DesignError::UnknownKey { key: "layout.density".into() }]);

        written(&data, "grid", "[list]\nstyle = \"grid\"\n");
        written(&data, "outline", "[list]\nselection = \"outline\"\n");
        let markdown = "format = 1\n[about]\nname = \"x\"\nversion = \"1\"\nmail_view = \"markdown\"\n";
        data.write(Part::Design, "markdown", markdown);
        for id in ["grid", "outline", "markdown"] {
            let refused = problems(&dir, id);
            assert!(matches!(&refused[..], [DesignError::Syntax { line: Some(_), .. }]), "{id}: {refused:?}");
        }

        written(&data, "misspelt", "[list]\nstyles = \"rows\"\n");
        assert_eq!(problems(&dir, "misspelt"), [DesignError::UnknownKey { key: "list.styles".into() }]);
    }

    /// Task L.33: colours never belong to a design, so `[about] color_scheme` is an unknown key; the
    /// font and the mail view are still suggested.
    #[test]
    fn a_design_suggests_no_colour_scheme() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        let text = "format = 1\n[about]\nname = \"x\"\nversion = \"1\"\ncolor_scheme = \"nord\"\n";
        data.write(Part::Design, "tinted", text);
        assert_eq!(problems(&dir, "tinted"), [DesignError::UnknownKey { key: "about.color_scheme".into() }]);
        let text = "format = 1\n[about]\nname = \"x\"\nversion = \"1\"\nfont = \"plain\"\nmail_view = \"text\"\n";
        data.write(Part::Design, "plain", text);
        let about = load(&dir, "plain").expect("valid").about;
        assert_eq!((about.font.as_deref(), about.mail_view), (Some("plain"), Some(MailFormat::Text)));
    }

    /// Issue #291: the numbers of `[list]` are held to their rules when read from a file.
    #[test]
    fn the_list_numbers_are_checked() {
        let data = Data::new();
        let dir = data.dir(Part::Design);
        written(&data, "odd", "[list]\navatar = 12\npreview_lines = 3\n");
        assert_eq!(
            problems(&dir, "odd"),
            [
                DesignError::OffOrRange { key: "list.avatar".into(), value: 12.0, min: 20.0, max: 48.0 },
                DesignError::OutOfRange { key: "list.preview_lines".into(), value: 3.0, min: 1.0, max: 2.0 },
            ]
        );
        written(&data, "off", "[list]\navatar = 0\n");
        assert_eq!(load(&dir, "off").expect("valid").list.avatar, 0.0, "0 switches the avatar off");
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
        let expected = [
            (STANDARD_ID, true),
            ("compact", true),
            ("console", true),
            ("accent-only", false),
            ("dark", false),
            ("font-only", false),
            ("full", false),
        ];
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
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 24.0, max: 64.0 }]
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
        assert_eq!(
            ids,
            [STANDARD_ID, "compact", "console", "Bad_Name", "form", "short-row", STANDARD_ID, "unknown-key"],
            "the built-in designs before the folders"
        );
        let valid: Vec<bool> = listed.iter().map(Listed::valid).collect();
        assert_eq!(valid, [true, true, true, false, true, false, false, false]);
        assert_eq!(listed[4].about.name, "Form");
        assert_eq!(listed[5].about.name, "Short row", "a broken design still shows its name");
        assert_eq!(
            listed[5].problems(),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 24.0, max: 64.0 }]
        );
        assert_eq!(listed[6].problems(), [DesignError::BadId { id: STANDARD_ID.into() }]);
        assert_eq!(listed[7].about.name, "unknown-key", "unreadable: the id stands in for the name");
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

    /// The fixture designs as the table test in `design.rs` sees them: every one listed, the old
    /// packs among them too (task L.21).
    #[test]
    fn every_fixture_design_is_listed() {
        let data = Data::all();
        let listed: Vec<String> =
            list(&data.dir(Part::Design)).into_iter().skip(BUILT_IN_IDS.len()).map(|entry| entry.id).collect();
        assert_eq!(listed, fixture_names(Part::Design));
    }
}
