//! Reading a pack folder: `design.toml` in three passes (TOML at all, then every unknown key, then
//! the typed values with their positions), merged into [`Design::standard`], then the file and
//! value rules. [`list`] and [`load`] are the public entry points.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::files::{Kind, Root, has_font_license};
use super::{
    About, Design, DesignError, Face, Font, FontRole, Fonts, Level, Mode, Pad, Palette, Radius, Rgba, SIDEBAR_FALLBACK,
    STANDARD_ID, STANDARD_SIZE, Space, Weight, is_valid_id, rules,
};

/// One entry of [`list`]: a design and whether it can be used.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    /// [`STANDARD_ID`] or the folder name.
    pub id: String,
    /// From `design.toml` as far as it could be read; for a pack whose file can't be read the
    /// name is the id and the version empty.
    pub about: About,
    /// The standard design (always first, always valid).
    pub standard: bool,
    /// The merged design, or every reason it can't be used (never empty).
    pub result: Result<Design, Vec<DesignError>>,
}

impl Listed {
    /// Whether the design can be used.
    pub fn valid(&self) -> bool {
        self.result.is_ok()
    }

    /// Why it can't be used; empty when it can.
    pub fn problems(&self) -> &[DesignError] {
        self.result.as_ref().err().map_or(&[], Vec::as_slice)
    }
}

/// Every design: the standard first, then each folder under `dir` (the designs folder, see
/// [`super::dir`]) by name, valid or with its reasons. Hidden folders (`.git`) and folder names
/// that are not UTF-8 are skipped; a missing `dir` lists only the standard.
pub fn list(dir: &Path) -> Vec<Listed> {
    let standard = Design::standard();
    let mut listed = vec![Listed {
        id: STANDARD_ID.to_string(),
        about: standard.about.clone(),
        standard: true,
        result: Ok(standard),
    }];
    let Ok(entries) = fs::read_dir(dir) else { return listed };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    ids.sort();
    listed.extend(ids.iter().map(|id| inspect(dir, id)));
    listed
}

/// The design `id` from the designs folder `dir`, merged with the standard and checked.
/// [`STANDARD_ID`] gives [`Design::standard`] without touching the disk.
///
/// `Err` holds every reason the pack can't be used (at least one); the caller then keeps the
/// design it has (the concept: a pack is never applied in part).
pub fn load(dir: &Path, id: &str) -> Result<Design, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(Design::standard());
    }
    inspect(dir, id).result
}

fn inspect(dir: &Path, id: &str) -> Listed {
    let mut about = About { name: id.to_string(), ..About::default() };
    let result = read(dir, id, &mut about);
    Listed { id: id.to_string(), about, standard: false, result }
}

fn read(dir: &Path, id: &str, about: &mut About) -> Result<Design, Vec<DesignError>> {
    if !is_valid_id(id) {
        return Err(vec![DesignError::BadId { id: id.to_string() }]);
    }
    let Some(root) = Root::open(&dir.join(id)) else {
        return Err(vec![DesignError::NotFound { id: id.to_string() }]);
    };
    if let Some(too_large) = root.check_total() {
        return Err(vec![too_large]);
    }
    let text = root.design_toml().map_err(|err| vec![err])?;
    let file = parse(&text)?;
    *about = About {
        name: file.about.name.clone().unwrap_or_else(|| id.to_string()),
        version: file.about.version.clone().unwrap_or_default(),
        author: file.about.author.clone(),
        description: file.about.description.clone(),
        font_license: file.about.font_license.clone(),
    };
    let mut problems = Vec::new();
    let design = merge(id, &root, file, &mut problems);
    problems.extend(rules::check(&design));
    if problems.is_empty() { Ok(design) } else { Err(problems) }
}

// ---------------------------------------------------------------------------------------------
// design.toml
// ---------------------------------------------------------------------------------------------

/// `design.toml` as written; everything optional, so missing keys can be named one by one.
/// `deny_unknown_fields` backs up [`unknown_keys`].
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    format: Option<i64>,
    #[serde(default)]
    about: AboutFile,
    fonts: Option<FontsFile>,
    #[serde(rename = "type", default)]
    typo: BTreeMap<String, LevelFile>,
    space: Option<SpaceFile>,
    radius: Option<RadiusFile>,
    layout: Option<LayoutFile>,
    #[serde(default)]
    size: BTreeMap<String, f32>,
    #[serde(default)]
    colors: ColorsFile,
    #[serde(default)]
    logo: LogoFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AboutFile {
    name: Option<String>,
    version: Option<String>,
    author: Option<String>,
    description: Option<String>,
    font_license: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FontsFile {
    text: Option<FontFile>,
    heading: Option<FontFile>,
    numbers: Option<FontFile>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FontFile {
    family: Option<String>,
    regular: Option<String>,
    medium: Option<String>,
    semibold: Option<String>,
    bold: Option<String>,
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
}

/// Colours stay loose values here, so each bad one is reported with its key.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorsFile {
    #[serde(default)]
    dark: BTreeMap<String, toml::Value>,
    #[serde(default)]
    light: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LogoFile {
    mark: Option<String>,
    tray: Option<String>,
}

/// The keys of `[colors.dark]` and `[colors.light]`: the fields of [`Palette`].
const PALETTE_KEYS: [&str; 34] = [
    "bg",
    "panel",
    "raised",
    "hover",
    "selected",
    "menu",
    "text",
    "text_2",
    "text_3",
    "line",
    "line_strong",
    "line_selected",
    "focus_ring",
    "accent",
    "on_accent",
    "ok",
    "attention",
    "error",
    "inactive",
    "merged",
    "calendar",
    "tint",
    "chart",
    "chart_other",
    "shadow",
    "scrim",
    "night",
    "sidebar",
    "sidebar_hover",
    "sidebar_selected",
    "sidebar_text",
    "sidebar_text_2",
    "sidebar_text_3",
    "sidebar_line",
];
const LEVEL_KEYS: [&str; 11] =
    ["micro", "caption", "meta", "body", "body_strong", "read", "title", "headline", "display", "hero", "countdown"];
const LAYOUT_KEYS: [&str; 7] =
    ["inset", "page_padding", "panel_padding", "row_padding", "control_padding", "section_gap", "item_gap"];

/// What `key` is in the table at `path` (`""`, `"colors.dark"` …): a value, a table to look
/// into, or unknown (`None`).
fn known(path: &str, key: &str) -> Option<bool> {
    const TABLE: Option<bool> = Some(true);
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" => Some(false),
            "about" | "fonts" | "type" | "space" | "radius" | "layout" | "size" | "colors" | "logo" => TABLE,
            _ => None,
        },
        "about" => value(["name", "version", "author", "description", "font_license"].contains(&key)),
        "fonts" => ["text", "heading", "numbers"].contains(&key).then_some(true),
        "fonts.text" | "fonts.heading" | "fonts.numbers" => {
            value(["family", "regular", "medium", "semibold", "bold"].contains(&key))
        }
        "type" => LEVEL_KEYS.contains(&key).then_some(true),
        p if p.strip_prefix("type.").is_some_and(|level| LEVEL_KEYS.contains(&level)) => {
            value(["font", "size", "line", "weight"].contains(&key))
        }
        "space" | "radius" => value(key == "scale"),
        "layout" => value(LAYOUT_KEYS.contains(&key)),
        "size" => value(STANDARD_SIZE.entries().iter().any(|(name, _)| *name == key)),
        "colors" => ["dark", "light"].contains(&key).then_some(true),
        "colors.dark" | "colors.light" => value(PALETTE_KEYS.contains(&key)),
        "logo" => value(key == "mark" || key == "tray"),
        _ => None,
    }
}

/// Every key in `table` (at `path`) the format does not know, with its full path.
fn unknown_keys(path: &str, table: &toml::Table, found: &mut Vec<DesignError>) {
    for (key, value) in table {
        let full = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
        match (known(path, key), value) {
            (None, _) => found.push(DesignError::UnknownKey { key: full }),
            (Some(true), toml::Value::Table(inner)) => unknown_keys(&full, inner, found),
            // A value where a table belongs (or the reverse) is a type error for the third pass.
            _ => {}
        }
    }
}

/// The three passes over `text`; the first that fails ends the reading.
fn parse(text: &str) -> Result<PackFile, Vec<DesignError>> {
    let table: toml::Table = toml::from_str(text).map_err(|err| vec![syntax(text, &err)])?;
    let mut unknown = Vec::new();
    unknown_keys("", &table, &mut unknown);
    if !unknown.is_empty() {
        return Err(unknown);
    }
    toml::from_str(text).map_err(|err| vec![syntax(text, &err)])
}

fn syntax(text: &str, err: &toml::de::Error) -> DesignError {
    let line = err.span().map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
    DesignError::Syntax { line, reason: err.message().trim().to_string() }
}

// ---------------------------------------------------------------------------------------------
// Merging into the standard
// ---------------------------------------------------------------------------------------------

fn merge(id: &str, root: &Root, file: PackFile, problems: &mut Vec<DesignError>) -> Design {
    let mut design = Design::standard();
    design.id = id.to_string();

    match file.format {
        None => problems.push(DesignError::MissingKey { key: "format".into() }),
        Some(1) => {}
        Some(found) => problems.push(DesignError::UnsupportedFormat { found }),
    }
    for (key, value) in [("about.name", &file.about.name), ("about.version", &file.about.version)] {
        if value.is_none() {
            problems.push(DesignError::MissingKey { key: key.into() });
        }
    }
    let a = file.about;
    design.about = About {
        name: a.name.unwrap_or_default(),
        version: a.version.unwrap_or_default(),
        author: a.author,
        description: a.description,
        font_license: a.font_license,
    };

    if let Some(fonts) = file.fonts {
        merge_fonts(root, fonts, &mut design, problems);
    }
    for (name, level) in &file.typo {
        if let Some(slot) = design.typo.slot_mut(name) {
            merge_level(slot, level);
        }
    }
    // The levels of a reminder over the whole screen (task ER1.5) follow a pack that does not name
    // them, so packs from before stay valid: the title takes its headlines' font and weight, the
    // countdown Medium where its figures font has it, else the weight of its time axis.
    if !file.typo.contains_key("hero") {
        design.typo.hero.font = design.typo.headline.font;
        design.typo.hero.weight = design.typo.headline.weight;
    }
    if !file.typo.contains_key("countdown") && design.fonts.numbers.face(Weight::Medium).is_none() {
        design.typo.countdown.weight = design.typo.micro.weight;
    }
    if let Some(SpaceFile { scale: [xs, s, m, l, xl, xxl] }) = file.space {
        design.space = Space { xs, s, m, l, xl, xxl };
    }
    if let Some(RadiusFile { scale: [xs, sm, md, lg] }) = file.radius {
        design.radius = Radius { xs, sm, md, lg };
    }
    if let Some(layout) = file.layout {
        merge_layout(&mut design, layout);
    }
    for (key, value) in file.size {
        if let Some(slot) = design.size.slot_mut(&key) {
            *slot = value;
        }
    }
    for (mode, values) in [(Mode::Dark, &file.colors.dark), (Mode::Light, &file.colors.light)] {
        let palette = match mode {
            Mode::Dark => &mut design.colors.dark,
            Mode::Light => &mut design.colors.light,
        };
        merge_palette(mode, palette, values, problems);
    }
    for (key, rel, slot) in
        [("logo.mark", file.logo.mark, &mut design.logo.mark), ("logo.tray", file.logo.tray, &mut design.logo.tray)]
    {
        let Some(rel) = rel else { continue };
        match root.named(key, &rel, Kind::Svg) {
            Ok(path) => {
                problems.extend(root.check_logo(&path));
                *slot = Some(path);
            }
            Err(found) => problems.extend(found),
        }
    }
    match root.preview() {
        Ok(preview) => design.preview = preview,
        Err(found) => problems.extend(found),
    }
    design
}

/// The pack's fonts. With `[fonts.text]`, a missing `heading` or `numbers` is the pack's text
/// font; without it, they stay the standard's (so a pack may change only the headings).
fn merge_fonts(root: &Root, file: FontsFile, design: &mut Design, problems: &mut Vec<DesignError>) {
    let mut read = |role: &str, font: Option<FontFile>| font.map(|f| read_font(root, role, f, problems));
    let text = read("text", file.text);
    let heading = read("heading", file.heading);
    let numbers = read("numbers", file.numbers);
    let standard = design.fonts.clone();
    let base = text.clone();
    design.fonts = Fonts {
        text: text.unwrap_or(standard.text),
        heading: heading.or_else(|| base.clone()).unwrap_or(standard.heading),
        numbers: numbers.or(base).unwrap_or(standard.numbers),
    };
    let has_files = [&design.fonts.text, &design.fonts.heading, &design.fonts.numbers].iter().any(|font| {
        [&font.regular, &font.medium, &font.semibold, &font.bold].iter().any(|face| matches!(face, Some(Face::File(_))))
    });
    if has_files {
        if design.about.font_license.is_none() {
            problems.push(DesignError::MissingKey { key: "about.font_license".into() });
        }
        if !has_font_license(root) {
            problems.push(DesignError::FontLicense);
        }
    }
}

/// One `[fonts.<role>]`: family, the files that exist, each checked to be a font of that family.
fn read_font(root: &Root, role: &str, file: FontFile, problems: &mut Vec<DesignError>) -> Font {
    let family = file.family.unwrap_or_else(|| {
        problems.push(DesignError::MissingKey { key: format!("fonts.{role}.family") });
        String::new()
    });
    let any_file = [&file.regular, &file.medium, &file.semibold, &file.bold].iter().any(|f| f.is_some());
    // Text needs its regular cut (running text); the others need at least one file.
    if file.regular.is_none() && (role == "text" || !any_file) {
        problems.push(DesignError::MissingKey { key: format!("fonts.{role}.regular") });
    }
    let mut face = |weight: &str, rel: Option<String>| -> Option<Face> {
        let key = format!("fonts.{role}.{weight}");
        let path = match root.named(&key, &rel?, Kind::Font) {
            Ok(path) => path,
            Err(found) => {
                problems.extend(found);
                return None;
            }
        };
        match root.font_families(&path) {
            Ok(found) if found.iter().any(|name| name == &family) || family.is_empty() => {}
            Ok(found) => {
                problems.push(DesignError::FontFamily { file: root.show(&path), family: family.clone(), found })
            }
            Err(err) => problems.push(err),
        }
        Some(Face::File(path))
    };
    Font {
        regular: face("regular", file.regular),
        medium: face("medium", file.medium),
        semibold: face("semibold", file.semibold),
        bold: face("bold", file.bold),
        family,
    }
}

fn merge_level(slot: &mut Level, file: &LevelFile) {
    slot.font = file.font.unwrap_or(slot.font);
    slot.weight = file.weight.unwrap_or(slot.weight);
    slot.size = file.size.unwrap_or(slot.size);
    slot.line = file.line.unwrap_or(slot.line);
}

fn merge_layout(design: &mut Design, file: LayoutFile) {
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
}

fn merge_palette(
    mode: Mode,
    palette: &mut Palette,
    values: &BTreeMap<String, toml::Value>,
    problems: &mut Vec<DesignError>,
) {
    let shown = |value: &toml::Value| value.as_str().unwrap_or(value.type_str()).to_string();
    for (name, value) in values {
        let key = format!("colors.{mode}.{name}");
        match name.as_str() {
            "tint" => match value.as_float().or_else(|| value.as_integer().map(|i| i as f64)) {
                Some(tint) => palette.tint = tint as f32,
                None => problems.push(DesignError::OutOfRange { key, value: f32::NAN, min: 0.0, max: 1.0 }),
            },
            "calendar" => match colour_list::<5>(value) {
                Some(list) => palette.calendar = list,
                None => problems.push(DesignError::BadColor { key, value: shown(value) }),
            },
            "chart" => match colour_list::<4>(value) {
                Some(list) => palette.chart = list,
                None => problems.push(DesignError::BadColor { key, value: shown(value) }),
            },
            _ => match (palette.slot_mut(name), value.as_str().and_then(Rgba::parse)) {
                (Some(slot), Some(colour)) => *slot = colour,
                _ => problems.push(DesignError::BadColor { key, value: shown(value) }),
            },
        }
    }
    // A sidebar key the pack leaves out follows the pack's own colour, not the standard's: a pack
    // that sets only `bg` gets a sidebar in its `bg`, and looks as before sidebar keys existed.
    for (key, base) in SIDEBAR_FALLBACK {
        if values.contains_key(key) {
            continue;
        }
        if let Some(colour) = palette.slot_mut(base).map(|slot| *slot)
            && let Some(slot) = palette.slot_mut(key)
        {
            *slot = colour;
        }
    }
    chart_fallback(palette, values);
}

/// `value` as a list of exactly `N` colours (`calendar`, `chart`); `None` for anything else.
fn colour_list<const N: usize>(value: &toml::Value) -> Option<[Rgba; N]> {
    let list = value.as_array()?.iter().map(|v| v.as_str().and_then(Rgba::parse)).collect::<Option<Vec<_>>>()?;
    <[Rgba; N]>::try_from(list).ok()
}

/// The order in which the chart takes the calendar colours of a pack (task E8.8b): blue, orange,
/// green, pink – the hues of [`Palette::chart`], so neighbours in a stack stay apart.
const CHART_FROM_CALENDAR: [usize; 4] = [0, 2, 1, 3];

/// The chart keys a pack leaves out (task E8.8b) follow the pack where the standard's would not
/// carry: the standard's chart colours were checked against the standard's `panel` only.
///
/// - `chart`: a pack with its own `calendar` takes those colours in the chart's order
///   ([`CHART_FROM_CALENDAR`]); they hold 3:1 on its `panel` already. Without one the standard's
///   chart colours stay.
/// - `chart_other`: a pack with its own `panel` takes its `text_3`, which holds 4.5:1 there; with
///   the standard's `panel` the standard's grey stays.
fn chart_fallback(palette: &mut Palette, values: &BTreeMap<String, toml::Value>) {
    if !values.contains_key("chart") && values.contains_key("calendar") {
        palette.chart = CHART_FROM_CALENDAR.map(|index| palette.calendar[index]);
    }
    if !values.contains_key("chart_other") && values.contains_key("panel") {
        palette.chart_other = palette.text_3;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::design::Logo;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/designs");
    /// Inter as `torro-gui` ships it; fixtures with fonts get a copy instead of their own bytes.
    const INTER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../torro-gui/assets/fonts");
    const INTER_FILES: [&str; 5] = [
        "Inter-Regular.ttf",
        "Inter-Medium.ttf",
        "Inter-SemiBold.ttf",
        "InterTabular-Regular.ttf",
        "InterTabular-Medium.ttf",
    ];

    /// A designs folder in a temporary directory with copies of the fixture packs `names`.
    fn designs(names: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("temp dir");
        for name in names {
            copy_dir(&Path::new(FIXTURES).join(name), &tmp.path().join(name));
        }
        tmp
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

    fn with_inter(pack: &Path) {
        let fonts = pack.join("fonts");
        fs::create_dir_all(&fonts).expect("fonts");
        for file in INTER_FILES {
            fs::copy(Path::new(INTER).join(file), fonts.join(file)).expect("copy Inter");
        }
    }

    fn problems(dir: &Path, id: &str) -> Vec<DesignError> {
        match load(dir, id) {
            Ok(_) => panic!("{id} must be refused"),
            Err(problems) => problems,
        }
    }

    fn write(path: PathBuf, text: &str) {
        fs::write(path, text).expect("write");
    }

    #[test]
    fn the_standard_is_loaded_without_a_folder_and_listed_first() {
        let tmp = tempfile::tempdir().expect("temp dir");
        assert_eq!(load(tmp.path(), STANDARD_ID), Ok(Design::standard()));
        let listed = list(&tmp.path().join("missing"));
        assert_eq!(listed.len(), 1);
        assert!(listed[0].standard && listed[0].valid() && listed[0].problems().is_empty());
        assert_eq!(listed[0].about.name, "Torro");
    }

    #[test]
    fn only_an_accent_takes_everything_else_from_the_standard() {
        let tmp = designs(&["accent-only"]);
        let design = load(tmp.path(), "accent-only").expect("valid");
        assert_eq!(design.colors.dark.accent, Rgba::hex(0x0E7C7B));
        let mut expected = Design::standard();
        expected.id = "accent-only".into();
        expected.about = About { name: "Teal".into(), version: "1.0.0".into(), ..About::default() };
        expected.colors.dark.accent = Rgba::hex(0x0E7C7B);
        assert_eq!(design, expected);
    }

    /// Task E8.2: the reading level and the sizes of the comfortable density are keys of the format.
    #[test]
    fn the_comfortable_tokens_are_read_from_a_pack() {
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(tmp.path().join("roomy")).expect("pack folder");
        let toml = "format = 1\n[about]\nname = \"Roomy\"\nversion = \"1.0.0\"\n\
                    [type.read]\nsize = 15\nline = 24\n\
                    [size]\ncard_gap = 16\navatar = 40\navatar_lg = 44\ntile_sm = 22\ntile = 30\ntile_lg = 40\n";
        write(tmp.path().join("roomy").join("design.toml"), toml);
        let design = load(tmp.path(), "roomy").expect("valid");
        assert_eq!(design.typo.read, Level { font: FontRole::Text, weight: Weight::Regular, size: 15.0, line: 24.0 });
        let s = design.size;
        assert_eq!(
            [s.card_gap, s.avatar, s.avatar_lg, s.tile_sm, s.tile, s.tile_lg],
            [16.0, 40.0, 44.0, 22.0, 30.0, 40.0]
        );
    }

    #[test]
    fn a_pack_with_every_key_loads() {
        let tmp = designs(&["full"]);
        let pack = tmp.path().join("full");
        with_inter(&pack);
        let design = load(tmp.path(), "full").expect("valid");
        let root = fs::canonicalize(&pack).expect("canonical");
        assert_eq!(design.about.version, "2.1.0");
        assert_eq!(design.fonts.text.regular, Some(Face::File(root.join("fonts/Inter-Regular.ttf"))));
        assert_eq!(design.fonts.heading.regular, None);
        assert_eq!(design.fonts.numbers.family, "Inter Tabular");
        assert_eq!(
            design.typo.title,
            Level { font: FontRole::Heading, weight: Weight::Semibold, size: 17.0, line: 24.0 }
        );
        assert_eq!(design.typo.meta.weight, Weight::Regular, "a missing weight stays the standard's");
        assert_eq!(design.radius, Radius { xs: 3.0, sm: 5.0, md: 7.0, lg: 9.0 });
        assert_eq!(design.layout.page_padding, Pad { vertical: 24.0, horizontal: 32.0 });
        assert_eq!((design.layout.inset, design.layout.item_gap), (10.0, 10.0));
        assert_eq!((design.size.row, design.size.sidebar), (44.0, 248.0));
        assert_eq!(design.colors.dark.shadow, Rgba { a: 128.0 / 255.0, ..Rgba::hex(0) });
        assert_eq!(design.colors.light.accent, Rgba::hex(0x0B6B6A));
        assert_eq!(design.colors.light.bg, Design::standard().colors.light.bg);
        assert_eq!(design.logo, Logo { mark: Some(root.join("logo.svg")), tray: Some(root.join("tray.svg")) });
    }

    #[test]
    fn sidebar_keys_fall_back_to_the_packs_own_colours() {
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::create_dir(tmp.path().join("own-bg")).expect("dir");
        write(
            tmp.path().join("own-bg/design.toml"),
            "format = 1\n[about]\nname = \"Own bg\"\nversion = \"1\"\n[colors.light]\nbg = \"#ECEEF0\"\ntext_2 = \"#474B52\"\n",
        );
        let design = load(tmp.path(), "own-bg").expect("valid");
        let light = design.colors.light;
        assert_eq!((light.sidebar, light.sidebar_text_2), (Rgba::hex(0xECEEF0), Rgba::hex(0x474B52)));
        assert_eq!(light.sidebar_hover, light.hover, "not the standard's sidebar, the pack's own");
        assert!(!light.own_sidebar());
        assert_eq!(light.in_sidebar(), light, "without own sidebar keys the sidebar draws as before");
        assert_eq!(design.colors.dark, Design::standard().colors.dark);
    }

    #[test]
    fn a_dark_sidebar_beside_light_content_loads() {
        let tmp = designs(&["sidebar-dark"]);
        let design = load(tmp.path(), "sidebar-dark").expect("valid");
        let light = design.colors.light;
        assert!(light.own_sidebar());
        assert_eq!(light.sidebar_line, Rgba { a: 31.0 / 255.0, ..Rgba::hex(0xFFFFFF) });
        let side = light.in_sidebar();
        assert_eq!((side.bg, side.panel, side.text), (Rgba::hex(0x24292F), Rgba::hex(0x24292F), Rgba::hex(0xF6F8FA)));
        assert_eq!((side.raised, side.line_strong), (light.sidebar_hover, light.sidebar_line));
        assert_eq!(side.accent, light.accent, "accent and status stay");
    }

    #[test]
    fn a_dark_sidebar_with_the_contents_text_is_refused() {
        let tmp = designs(&["sidebar-faint"]);
        let found = problems(tmp.path(), "sidebar-faint");
        let named = |fg: &str, on: &str| {
            found.iter().any(|p| {
                matches!(p, DesignError::TextContrast { fg: f, on: o, .. }
                if f == &format!("colors.light.{fg}") && o == &format!("colors.light.{on}"))
            })
        };
        assert!(named("sidebar_text", "sidebar"), "sidebar_text fell back to the dark text: {found:?}");
        assert!(found.iter().any(|p| matches!(p, DesignError::ControlContrast { fg, on, .. }
            if fg == "colors.light.accent" && on == "colors.light.sidebar")));
        assert!(found.iter().all(|p| !matches!(p, DesignError::TextContrast { fg, .. } if fg == "colors.light.text")));
    }

    #[test]
    fn a_text_font_stands_in_for_heading_and_numbers() {
        let tmp = designs(&["wrong-family"]);
        let pack = tmp.path().join("wrong-family");
        with_inter(&pack);
        let toml = fs::read_to_string(pack.join("design.toml")).expect("read").replace("IBM Plex Sans", "Inter");
        write(pack.join("design.toml"), &toml);
        let design = load(tmp.path(), "wrong-family").expect("valid");
        assert_eq!(design.fonts.heading, design.fonts.text);
        assert_eq!(design.fonts.numbers, design.fonts.text, "no tabular family: figures are proportional");
    }

    #[test]
    fn each_rule_has_a_broken_example() {
        let names = [
            "low-contrast",
            "bad-on-accent",
            "tiny-type",
            "short-row",
            "path-outside",
            "svg-href",
            "font-no-license",
            "wrong-family",
            "unknown-key",
            "format-2",
        ];
        let tmp = designs(&names);
        with_inter(&tmp.path().join("font-no-license"));
        with_inter(&tmp.path().join("wrong-family"));
        let dir = tmp.path();

        assert!(problems(dir, "low-contrast").iter().all(
            |p| matches!(p, DesignError::TextContrast { fg, ratio, .. } if fg == "colors.dark.text_3" && *ratio < 4.5)
        ));
        assert!(matches!(problems(dir, "bad-on-accent")[..], [DesignError::AccentContrast { mode: Mode::Light, .. }]));
        assert_eq!(
            problems(dir, "tiny-type"),
            [DesignError::OutOfRange { key: "type.caption.size".into(), value: 9.0, min: 11.0, max: 40.0 }]
        );
        assert_eq!(
            problems(dir, "short-row"),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 32.0, max: 64.0 }]
        );
        assert_eq!(
            problems(dir, "path-outside"),
            [DesignError::PathOutside { key: "logo.mark".into(), path: "../x.svg".into() }]
        );
        assert_eq!(
            problems(dir, "svg-href"),
            [DesignError::SvgReference { file: "logo.svg".into(), reference: "https://example.org/logo.png".into() }]
        );
        assert_eq!(problems(dir, "font-no-license"), [DesignError::FontLicense]);
        let wrong = problems(dir, "wrong-family");
        assert_eq!(wrong.len(), 3, "one per file: {wrong:?}");
        assert!(wrong.iter().all(|p| matches!(p, DesignError::FontFamily { family, found, .. }
            if family == "IBM Plex Sans" && found.contains(&"Inter".to_string()))));
        assert_eq!(problems(dir, "unknown-key"), [DesignError::UnknownKey { key: "size.rowx".into() }]);
        assert_eq!(problems(dir, "format-2"), [DesignError::UnsupportedFormat { found: 2 }]);
    }

    #[test]
    fn required_keys_are_named() {
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::create_dir(tmp.path().join("bare")).expect("dir");
        write(tmp.path().join("bare/design.toml"), "[about]\nauthor = \"Nobody\"\n");
        let keys: Vec<String> = problems(tmp.path(), "bare")
            .into_iter()
            .map(|p| match p {
                DesignError::MissingKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["format", "about.name", "about.version"]);
        fs::create_dir(tmp.path().join("empty")).expect("dir");
        assert_eq!(
            problems(tmp.path(), "empty"),
            [DesignError::MissingFile { key: "design.toml".into(), path: "design.toml".into() }]
        );
        assert_eq!(problems(tmp.path(), "gone"), [DesignError::NotFound { id: "gone".into() }]);
        assert_eq!(problems(tmp.path(), "../etc"), [DesignError::BadId { id: "../etc".into() }]);
    }

    #[test]
    fn every_used_weight_needs_a_file() {
        let tmp = designs(&["wrong-family"]);
        let pack = tmp.path().join("wrong-family");
        with_inter(&pack);
        let toml = fs::read_to_string(pack.join("design.toml"))
            .expect("read")
            .replace("IBM Plex Sans", "Inter")
            .replace("medium = \"fonts/Inter-Medium.ttf\"\n", "");
        write(pack.join("design.toml"), &toml);
        let keys: Vec<String> = problems(tmp.path(), "wrong-family")
            .into_iter()
            .map(|p| match p {
                DesignError::MissingWeight { key, weight: Weight::Medium, .. } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["type.caption", "type.body_strong"]);
    }

    #[test]
    fn files_are_size_checked_before_reading() {
        let tmp = designs(&["accent-only"]);
        let toml = tmp.path().join("accent-only/design.toml");
        let mut text = fs::read_to_string(&toml).expect("read");
        text.push_str(&format!("# {}\n", "x".repeat(70 * 1024)));
        write(toml, &text);
        assert_eq!(
            problems(tmp.path(), "accent-only"),
            [DesignError::TooLarge { file: "design.toml".into(), limit: "64 KB".into() }]
        );
    }

    #[test]
    fn the_preview_is_a_small_png() {
        let tmp = designs(&["accent-only"]);
        let png = |w: u32, h: u32| {
            let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
            bytes.extend(w.to_be_bytes());
            bytes.extend(h.to_be_bytes());
            bytes
        };
        let preview = tmp.path().join("accent-only/preview.png");
        fs::write(&preview, png(1200, 800)).expect("write");
        let design = load(tmp.path(), "accent-only").expect("valid");
        assert_eq!(design.preview, Some(fs::canonicalize(&preview).expect("canonical")));
        fs::write(&preview, png(1600, 900)).expect("write");
        assert_eq!(problems(tmp.path(), "accent-only"), [DesignError::PreviewSize { width: 1600, height: 900 }]);
        fs::write(&preview, b"GIF89a").expect("write");
        assert!(matches!(problems(tmp.path(), "accent-only")[..], [DesignError::Unreadable { .. }]));
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_refused() {
        let tmp = designs(&["full"]);
        let pack = tmp.path().join("full");
        with_inter(&pack);
        let outside = tmp.path().join("elsewhere.svg");
        fs::copy(pack.join("logo.svg"), &outside).expect("copy");
        fs::remove_file(pack.join("logo.svg")).expect("remove");
        std::os::unix::fs::symlink(&outside, pack.join("logo.svg")).expect("link");
        assert_eq!(
            problems(tmp.path(), "full"),
            [DesignError::PathOutside { key: "logo.mark".into(), path: "logo.svg".into() }]
        );
        // A link that stays inside is fine.
        fs::remove_file(pack.join("logo.svg")).expect("remove");
        std::os::unix::fs::symlink(pack.join("tray.svg"), pack.join("logo.svg")).expect("link");
        assert!(load(tmp.path(), "full").is_ok());
    }

    #[test]
    fn list_shows_valid_and_broken_packs_with_reasons() {
        let tmp = designs(&["accent-only", "short-row", "unknown-key"]);
        fs::create_dir(tmp.path().join("Bad_Name")).expect("dir");
        fs::create_dir(tmp.path().join(".hidden")).expect("dir");
        fs::create_dir(tmp.path().join(STANDARD_ID)).expect("dir");
        write(tmp.path().join("notes.txt"), "not a pack");
        let listed = list(tmp.path());
        let ids: Vec<&str> = listed.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, [STANDARD_ID, "Bad_Name", "accent-only", "short-row", STANDARD_ID, "unknown-key"]);
        let valid: Vec<bool> = listed.iter().map(Listed::valid).collect();
        assert_eq!(valid, [true, false, true, false, false, false]);
        assert_eq!(listed[2].about.name, "Teal");
        assert_eq!(listed[3].about.name, "Short row", "a broken pack still shows its name");
        assert_eq!(
            listed[3].problems(),
            [DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 32.0, max: 64.0 }]
        );
        assert_eq!(listed[4].problems(), [DesignError::BadId { id: STANDARD_ID.into() }]);
        assert_eq!(listed[5].about.name, "unknown-key", "unreadable: the id stands in for the name");
        assert!(listed.iter().skip(1).all(|l| !l.standard));
    }

    fn parse_only(text: &str) -> Result<PackFile, Vec<DesignError>> {
        parse(text)
    }

    #[test]
    fn every_palette_key_has_a_field() {
        let mut p = Design::standard().colors.dark;
        for key in PALETTE_KEYS {
            assert!(p.slot_mut(key).is_some() || ["calendar", "chart", "tint"].contains(&key), "{key}");
        }
    }

    #[test]
    fn unknown_keys_are_named_with_their_path() {
        let text =
            "format = 1\ncolour = 1\n[size]\nrowx = 44\n[colors.dark]\naccnt = \"#000000\"\n[type.bodyy]\nsize = 3\n";
        let Err(problems) = parse_only(text) else { panic!("unknown keys must fail") };
        let keys: Vec<String> = problems
            .into_iter()
            .map(|p| match p {
                DesignError::UnknownKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["colors.dark.accnt", "colour", "size.rowx", "type.bodyy"]);
    }

    #[test]
    fn type_errors_carry_the_line() {
        let Err(problems) = parse_only("format = 1\n\n[type.body]\nsize = \"big\"\n") else { panic!("must fail") };
        assert!(matches!(&problems[..], [DesignError::Syntax { line: Some(4), .. }]), "{problems:?}");
        let Err(problems) = parse_only("format = = 1") else { panic!("must fail") };
        assert!(matches!(&problems[..], [DesignError::Syntax { line: Some(1), .. }]), "{problems:?}");
        let Err(problems) = parse_only("[space]\nscale = [1, 2]\n") else { panic!("must fail") };
        assert!(matches!(&problems[..], [DesignError::Syntax { .. }]), "{problems:?}");
    }

    #[test]
    fn integers_and_floats_both_work_for_numbers() {
        let file = parse_only("[type.body]\nsize = 14\nline = 21.5\n[size]\nrow = 44\n").expect("valid");
        assert_eq!(file.typo["body"].size, Some(14.0));
        assert_eq!(file.typo["body"].line, Some(21.5));
        assert_eq!(file.size["row"], 44.0);
    }

    /// Task E8.8b: the packs in the repository's `designs/` stay valid with the chart rules – the
    /// chart keys they leave out follow their own colours.
    #[test]
    fn the_shipped_packs_stay_valid() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../designs");
        let listed = list(&shipped);
        assert!(listed.len() > 1, "the packs are there");
        for pack in listed {
            assert!(pack.result.is_ok(), "{}: {:?}", pack.id, pack.result.err());
        }
    }

    /// Task E8.8b: chart keys are read like the calendar's; left out, they follow the pack's own
    /// calendar and `text_3` where it has them, else the standard's.
    #[test]
    fn chart_colours_are_read_and_follow_the_pack_when_left_out() {
        let standard = Design::standard().colors.dark;
        let merged = |text: &str| {
            let mut palette = standard;
            let values: BTreeMap<String, toml::Value> = toml::from_str(text).expect("toml");
            let mut problems = Vec::new();
            merge_palette(Mode::Dark, &mut palette, &values, &mut problems);
            (palette, problems)
        };
        let own = merged("chart = [\"#111111\", \"#222222\", \"#333333\", \"#444444\"]\nchart_other = \"#555555\"");
        assert_eq!(own.0.chart, [0x111111, 0x222222, 0x333333, 0x444444].map(Rgba::hex));
        assert_eq!((own.0.chart_other, own.1.len()), (Rgba::hex(0x555555), 0));

        let accent_only = merged("accent = \"#0EA5A4\"").0;
        assert_eq!((accent_only.chart, accent_only.chart_other), (standard.chart, standard.chart_other));
        let calendar = "calendar = [\"#0000AA\", \"#00AA00\", \"#AA5500\", \"#AA00AA\", \"#5500AA\"]";
        let own_calendar = merged(&format!("{calendar}\npanel = \"#202020\"\ntext_3 = \"#A0A0A0\"")).0;
        assert_eq!(own_calendar.chart, [0x0000AA, 0xAA5500, 0x00AA00, 0xAA00AA].map(Rgba::hex), "blue, orange, green");
        assert_eq!(own_calendar.chart_other, Rgba::hex(0xA0A0A0), "its own grey on its own panel");

        let (_, problems) = merged("chart = [\"#111111\"]\nchart_other = \"grey\"");
        let keys: Vec<&str> = problems
            .iter()
            .map(|p| match p {
                DesignError::BadColor { key, .. } => key.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["colors.dark.chart", "colors.dark.chart_other"]);
    }

    #[test]
    fn bad_colours_are_named() {
        let mut palette = Design::standard().colors.light;
        let values: BTreeMap<String, toml::Value> =
            toml::from_str("accent = \"red\"\nbg = 3\ncalendar = [\"#000000\"]\ntint = \"x\"\nok = \"#00FF0080\"")
                .expect("toml");
        let mut problems = Vec::new();
        merge_palette(Mode::Light, &mut palette, &values, &mut problems);
        let keys: Vec<&str> = problems
            .iter()
            .map(|p| match p {
                DesignError::BadColor { key, .. } | DesignError::OutOfRange { key, .. } => key.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["colors.light.accent", "colors.light.bg", "colors.light.calendar", "colors.light.tint"]);
        assert_eq!(palette.ok.a, 128.0 / 255.0, "the good one is applied");
    }
}
