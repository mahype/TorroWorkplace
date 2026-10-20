//! Reading the folders of the three parts, and the designs folder itself.
//!
//! Shared by all three: [`Listed`], the listing of a folder ([`listing`]), opening one entry
//! ([`open`]), the three passes over a TOML file ([`parse`]: TOML at all, then every unknown key, then
//! the typed values with their positions) and `[about]` ([`AboutFile`]). Fonts are read in `font.rs`,
//! colour schemes in `scheme.rs`.
//!
//! A `design.toml` is merged into [`Design::standard`] and held to the design rules. The old sections
//! of a complete pack (`[fonts.*]`, `[colors.*]`, `[logo]`, `[about] font_license`) are no error there:
//! they are read as loose values and skipped, so they never break a design (task L.19). A file that has
//! them but no form at all is an old pure font or colour pack: [`load`] says where it belongs
//! ([`DesignError::WrongFolder`]) and [`list`] leaves it out.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::files::Root;
use super::font::{self, FontsFile};
use super::scheme::{self, ColorsFile, LogoFile};
use super::{
    About, ColorScheme, Density, Design, DesignError, FontRole, FontSet, Level, Pad, Part, Radius, STANDARD_ID,
    STANDARD_SIZE, Space, Weight, is_valid_id, rules,
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
/// [`super::dir`]) by name, valid or with its reasons. Hidden folders (`.git`), folder names that are
/// not UTF-8 and old pure font or colour packs ([`DesignError::WrongFolder`]) are left out; a missing
/// `dir` lists only the standard.
pub fn list(dir: &Path) -> Vec<Listed<Design>> {
    let standard = Design::standard();
    let mut listed = listing(dir, standard.about.clone(), standard, read);
    listed.retain(|entry| !entry.problems().iter().any(|p| matches!(p, DesignError::WrongFolder { .. })));
    listed
}

/// The design `id` from the designs folder `dir`, merged with the standard and checked.
/// [`STANDARD_ID`] gives [`Design::standard`] without touching the disk.
///
/// `Err` holds every reason the design can't be used (at least one); the caller then keeps the design
/// it has (the concept: a part is never applied in part). An old pure font or colour pack answers with
/// [`DesignError::WrongFolder`] alone.
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
    /// Old complete packs in `designs/` only: loose, so a wrong type never breaks the design.
    pub(super) font_license: Option<toml::Value>,
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
    /// The old sections of a complete pack, loose: skipped by the design, read by the bridge.
    fonts: Option<toml::Value>,
    colors: Option<toml::Value>,
    logo: Option<toml::Value>,
}

impl DesignFile {
    /// Whether the file sets any of the form, so it is a design and not an old pure font or colour
    /// pack.
    fn has_form(&self) -> bool {
        !self.typo.is_empty()
            || self.space.is_some()
            || self.radius.is_some()
            || self.layout.is_some()
            || !self.size.is_empty()
    }
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

/// The keys of `design.toml`. The old sections are values (not looked into), so nothing in them is
/// ever an unknown key.
fn known(path: &str, key: &str) -> Option<bool> {
    const TABLE: Option<bool> = Some(true);
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" | "fonts" | "colors" | "logo" => Some(false),
            "about" | "type" | "space" | "radius" | "layout" | "size" => TABLE,
            _ => None,
        },
        "about" => {
            value(["name", "version", "author", "description", "font", "color_scheme", "font_license"].contains(&key))
        }
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
    if !file.has_form() {
        let mut wrong = Vec::new();
        if file.fonts.is_some() {
            wrong.push(DesignError::WrongFolder { part: Part::Font, id: id.to_string() });
        }
        if file.colors.is_some() || file.logo.is_some() {
            wrong.push(DesignError::WrongFolder { part: Part::ColorScheme, id: id.to_string() });
        }
        if !wrong.is_empty() {
            return Err(wrong);
        }
    }
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
    if !problems.is_empty() {
        return Err(problems);
    }
    bridge(dir, id, &root, &file, &mut design);
    Ok(design)
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

// ---------------------------------------------------------------------------------------------
// Bridge until L.21/L.22, removed there
// ---------------------------------------------------------------------------------------------

/// **Bridge until L.21/L.22, removed there:** fills [`Design::fonts`], [`Design::colors`] and
/// [`Design::logo`] of a valid design, so the apps keep drawing one id as before L.19. Each from the
/// first source that passes its part's rules, else the standard's (no reason is reported: the design
/// itself is valid):
///
/// 1. the old sections of a complete pack in `file` (fonts under its `fonts/`, the licence in
///    `fonts/LICENSE.txt`, a `[logo] tray` still allowed), read as `fonts/<id>/` and
///    `color-schemes/<id>/` would be;
/// 2. `fonts/<id>/` and `color-schemes/<id>/` beside `dir`, but only when `dir` is a folder named
///    `designs` (the data folder, the repository's), so a test's folder never looks beside itself.
fn bridge(dir: &Path, id: &str, root: &Root, file: &DesignFile, design: &mut Design) {
    let beside = |part: Part| {
        let parent = dir.parent().filter(|_| dir.file_name().is_some_and(|name| name == Part::Design.folder()))?;
        Some(parent.join(part.folder()))
    };
    let old_font = file.fonts.as_ref().and_then(|fonts| old_font_set(id, root, file, fonts));
    let font = old_font.or_else(|| beside(Part::Font).and_then(|fonts| font::load_font(&fonts, id).ok()));
    if let Some(font) = font {
        design.fonts = font.fonts;
    }
    let old_scheme = (file.colors.is_some() || file.logo.is_some()).then(|| old_color_scheme(id, root, file));
    let scheme = old_scheme
        .flatten()
        .or_else(|| beside(Part::ColorScheme).and_then(|schemes| scheme::load_color_scheme(&schemes, id).ok()));
    if let Some(scheme) = scheme {
        design.colors = scheme.colors;
        design.logo = scheme.logo;
    }
}

/// The font of an old complete pack, if its `[fonts.*]` pass the font rules.
fn old_font_set(id: &str, root: &Root, file: &DesignFile, fonts: &toml::Value) -> Option<FontSet> {
    let fonts: FontsFile = fonts.clone().try_into().ok()?;
    let license = file.about.font_license.as_ref().and_then(|value| value.as_str().map(str::to_string));
    let mut problems = Vec::new();
    let source = font::Source { license, size_adjust: None, fonts: Some(fonts), license_file: "fonts/LICENSE.txt" };
    let set = font::build(id, About::default(), root, source, &mut problems);
    problems.is_empty().then_some(set)
}

/// The colours and logo of an old complete pack, if they pass the colour rules.
fn old_color_scheme(id: &str, root: &Root, file: &DesignFile) -> Option<ColorScheme> {
    let colors: ColorsFile = match &file.colors {
        Some(colors) => colors.clone().try_into().ok()?,
        None => ColorsFile::default(),
    };
    let logo: LogoFile = match &file.logo {
        Some(logo) => logo.clone().try_into().ok()?,
        None => LogoFile::default(),
    };
    let mut problems = Vec::new();
    let scheme = scheme::build(id, About::default(), root, &colors, logo, &mut problems);
    problems.is_empty().then_some(scheme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::testdata::{Data, fixture_names};
    use crate::design::{FontSet, Logo, Rgba};

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
        let standard = Design::standard();
        assert_eq!((design.fonts, design.colors, design.logo), (standard.fonts, standard.colors, Logo::default()));
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

    /// Task L.19: an old complete pack loads as a design without a reason; its old sections are not
    /// part of the design, only the bridge until L.21/L.22 reads them.
    #[test]
    fn an_old_complete_pack_loads_as_a_design_without_errors() {
        let data = Data::with(Part::Design, &["full"]);
        let design = load(&data.dir(Part::Design), "full").expect("an old pack is still a design");
        assert_eq!((design.about.name.as_str(), design.about.version.as_str()), ("Full", "2.1.0"));
        assert_eq!(design.typo.body.size, 14.0);
        assert_eq!((design.size.row, design.radius.lg), (44.0, 9.0));

        // The bridge: the old fonts, colours and logo, as before L.19.
        let root = fs::canonicalize(data.entry(Part::Design, "full")).expect("canonical");
        assert_eq!(design.fonts.text.regular, Some(crate::design::Face::File(root.join("fonts/Inter-Regular.ttf"))));
        assert_eq!(design.fonts.numbers.family, "Inter Tabular");
        assert_eq!(design.colors.dark.accent, Rgba::hex(0x0E7C7B));
        assert_eq!(design.colors.light.accent, Rgba::hex(0x0B6B6A));
        assert_eq!(design.logo, Logo { mark: Some(root.join("logo.svg")), tray: Some(root.join("tray.svg")) });
    }

    /// Task L.19: old sections that break their rules or the format never break the design; the
    /// bridge then keeps the standard's fonts and colours.
    #[test]
    fn broken_old_sections_are_skipped() {
        let data = Data::new();
        let old = "[size]\nrow = 44\n[colors.dark]\ntext_3 = \"#3A3A3A\"\naccnt = 3\n\
                   [fonts.text]\nfamily = \"Gone\"\nregular = \"fonts/Gone.ttf\"\n[logo]\nmark = \"../x.svg\"\n";
        written(&data, "old", old);
        let toml = fs::read_to_string(data.entry(Part::Design, "old").join("design.toml")).expect("read");
        data.write(Part::Design, "old", &toml.replace("[about]\n", "[about]\nfont_license = 7\n"));
        let design = load(&data.dir(Part::Design), "old").expect("valid");
        let standard = Design::standard();
        assert_eq!(design.size.row, 44.0);
        assert_eq!((design.fonts, design.colors, design.logo), (standard.fonts, standard.colors, standard.logo));
    }

    /// Task L.19: an old pure colour pack is no design any more: `list` leaves it out, `load` says
    /// where it belongs. The same for an old pure font pack.
    #[test]
    fn an_old_pure_colour_or_font_pack_belongs_elsewhere() {
        let data = Data::with(Part::Design, &["accent-only", "font-only", "form"]);
        let dir = data.dir(Part::Design);
        let colours = problems(&dir, "accent-only");
        assert_eq!(colours, [DesignError::WrongFolder { part: Part::ColorScheme, id: "accent-only".into() }]);
        assert!(colours[0].to_string().contains("color-schemes/"), "{}", colours[0]);
        let fonts = problems(&dir, "font-only");
        assert_eq!(fonts, [DesignError::WrongFolder { part: Part::Font, id: "font-only".into() }]);
        assert!(fonts[0].to_string().contains("fonts/"), "{}", fonts[0]);
        let ids: Vec<String> = list(&dir).into_iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [STANDARD_ID, "form"]);

        written(&data, "both", "[fonts.text]\nfamily = \"Inter\"\n[logo]\nmark = \"logo.svg\"\n");
        assert_eq!(problems(&dir, "both").len(), 2, "one for each folder it belongs in");
    }

    /// The bridge until L.21/L.22: a design without old sections draws the font and colour scheme of
    /// its id from the folders beside the designs folder, so the apps look as before after task L.20.
    #[test]
    fn the_bridge_takes_the_parts_of_the_same_id_beside_the_designs_folder() {
        let data = Data::with(Part::Font, &["plex"]);
        data.add(Part::ColorScheme, &["accent-only"]);
        fs::rename(data.entry(Part::Font, "plex"), data.entry(Part::Font, "accent-only")).expect("rename");
        written(&data, "accent-only", "[size]\nrow = 44\n");
        let design = load(&data.dir(Part::Design), "accent-only").expect("valid");
        assert_eq!(design.fonts.text.family, "IBM Plex Sans");
        assert_eq!(design.colors.dark.accent, Rgba::hex(0x0E7C7B));

        // A folder that is not called `designs` never looks beside itself.
        let elsewhere = data.entry(Part::Design, "accent-only");
        let copy = data.dir(Part::Font).parent().expect("data").join("other");
        fs::create_dir_all(&copy).expect("other");
        fs::copy(elsewhere.join("design.toml"), {
            fs::create_dir_all(copy.join("accent-only")).expect("entry");
            copy.join("accent-only/design.toml")
        })
        .expect("copy");
        let design = load(&copy, "accent-only").expect("valid");
        assert_eq!(design.fonts, FontSet::standard().fonts);
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
        let data = Data::with(Part::Design, &["form", "short-row", "unknown-key", "accent-only"]);
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
        assert_eq!(keys, ["about.fonts", "colour", "size.rowx", "type.bodyy"], "old sections are not looked into");
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
        assert!(parse_only("[colors.dark]\naccent = 3\n[fonts]\ntext = 1\n").is_ok(), "old sections are loose");
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
    /// would look unlike its own `preview.png`. Task L.19: the old pure colour packs among them are
    /// left out of the list until task L.20 moves them.
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

    /// The fixture designs as the table test in `design.rs` sees them: every one listed except the two
    /// old pure packs.
    #[test]
    fn every_fixture_design_is_listed_but_the_old_pure_packs() {
        let data = Data::all();
        let listed: Vec<String> = list(&data.dir(Part::Design)).into_iter().skip(1).map(|entry| entry.id).collect();
        let mut expected = fixture_names(Part::Design);
        expected.retain(|name| name != "accent-only" && name != "font-only");
        assert_eq!(listed, expected);
    }
}
