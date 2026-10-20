//! Reading a colour scheme folder `color-schemes/<id>/color-scheme.toml` (task L.19): the colours of
//! both modes, merged into the standard's, with the sidebar and chart keys following the scheme's own
//! colours where it leaves them out, and the logo. [`load_color_scheme`] and [`list_color_schemes`]
//! are the public entry points; [`build`] also reads the `[colors.*]` and `[logo]` of an old complete
//! pack for the bridge in `pack.rs`.
//!
//! The rules of a colour scheme: the contrasts (`rules::colors`) and the logo (SVG without references
//! and scripts, square). A tray symbol is not part of the format any more (issue #114).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::files::{Kind, Root};
use super::pack::{AboutFile, Listed, header, listing, open, parse};
use super::{About, ColorScheme, DesignError, Logo, Mode, Palette, Part, Rgba, SIDEBAR_FALLBACK, STANDARD_ID, rules};

/// Every colour scheme: the standard first, then each folder under `dir` (the colour schemes folder,
/// see [`super::color_scheme_dir`]) by name, valid or with its reasons. Hidden folders and names that
/// are not UTF-8 are skipped; a missing `dir` lists only the standard.
pub fn list_color_schemes(dir: &Path) -> Vec<Listed<ColorScheme>> {
    let standard = ColorScheme::standard();
    listing(dir, standard.about.clone(), standard, read)
}

/// The colour scheme `id` from the colour schemes folder `dir`, merged with the standard and checked.
/// [`STANDARD_ID`] gives [`ColorScheme::standard`] without touching the disk.
///
/// `Err` holds every reason the colour scheme can't be used (at least one).
pub fn load_color_scheme(dir: &Path, id: &str) -> Result<ColorScheme, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(ColorScheme::standard());
    }
    read(dir, id, &mut About::default())
}

/// `color-scheme.toml` as written; `deny_unknown_fields` backs up [`known`].
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemeFile {
    format: Option<i64>,
    #[serde(default)]
    about: AboutFile,
    #[serde(default)]
    colors: ColorsFile,
    #[serde(default)]
    logo: LogoFile,
}

/// `[colors]`. Colours stay loose values here, so each bad one is reported with its key.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ColorsFile {
    #[serde(default)]
    dark: BTreeMap<String, toml::Value>,
    #[serde(default)]
    light: BTreeMap<String, toml::Value>,
}

/// `[logo]`. `tray` is only read from an old complete pack; [`known`] refuses it in
/// `color-scheme.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LogoFile {
    mark: Option<String>,
    tray: Option<String>,
}

/// The keys of `[colors.dark]` and `[colors.light]`: the fields of [`Palette`].
pub(super) const PALETTE_KEYS: [&str; 34] = [
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

/// The keys of `color-scheme.toml`.
fn known(path: &str, key: &str) -> Option<bool> {
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" => Some(false),
            "about" | "colors" | "logo" => Some(true),
            _ => None,
        },
        "about" => value(["name", "version", "author", "description"].contains(&key)),
        "colors" => ["dark", "light"].contains(&key).then_some(true),
        "colors.dark" | "colors.light" => value(PALETTE_KEYS.contains(&key)),
        "logo" => value(key == "mark"),
        _ => None,
    }
}

fn read(dir: &Path, id: &str, about: &mut About) -> Result<ColorScheme, Vec<DesignError>> {
    let root = open(dir, id, Part::ColorScheme)?;
    let text = root.toml(Part::ColorScheme.file()).map_err(|err| vec![err])?;
    let file: SchemeFile = parse(Part::ColorScheme.file(), &text, known)?;
    *about = file.about.about(id);
    let mut problems = Vec::new();
    header(file.format, &file.about, &mut problems);
    let scheme = build(id, about.clone(), &root, &file.colors, file.logo, &mut problems);
    if problems.is_empty() { Ok(scheme) } else { Err(problems) }
}

/// A colour scheme from `colors` and `logo` as written in the folder `root`, checked; every broken
/// rule goes to `problems`.
pub(super) fn build(
    id: &str,
    about: About,
    root: &Root,
    colors: &ColorsFile,
    logo: LogoFile,
    problems: &mut Vec<DesignError>,
) -> ColorScheme {
    let mut scheme = ColorScheme { id: id.to_string(), about, ..ColorScheme::standard() };
    merge_palette(Mode::Dark, &mut scheme.colors.dark, &colors.dark, problems);
    merge_palette(Mode::Light, &mut scheme.colors.light, &colors.light, problems);
    scheme.logo = read_logo(root, logo, problems);
    problems.extend(rules::colors(&scheme.colors));
    scheme
}

fn read_logo(root: &Root, file: LogoFile, problems: &mut Vec<DesignError>) -> Logo {
    let mut logo = Logo::default();
    for (key, rel, slot) in [("logo.mark", file.mark, &mut logo.mark), ("logo.tray", file.tray, &mut logo.tray)] {
        let Some(rel) = rel else { continue };
        match root.named(key, &rel, Kind::Svg) {
            Ok(path) => {
                problems.extend(root.check_logo(&path));
                *slot = Some(path);
            }
            Err(found) => problems.extend(found),
        }
    }
    logo
}

pub(super) fn merge_palette(
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
    // A sidebar key the scheme leaves out follows the scheme's own colour, not the standard's: a
    // scheme that sets only `bg` gets a sidebar in its `bg`, and looks as before sidebar keys existed.
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

/// The order in which the chart takes the calendar colours of a scheme (task E8.8b): blue, orange,
/// green, pink – the hues of [`Palette::chart`], so neighbours in a stack stay apart.
const CHART_FROM_CALENDAR: [usize; 4] = [0, 2, 1, 3];

/// The chart keys a scheme leaves out (task E8.8b) follow the scheme where the standard's would not
/// carry: the standard's chart colours were checked against the standard's `panel` only.
///
/// - `chart`: a scheme with its own `calendar` takes those colours in the chart's order
///   ([`CHART_FROM_CALENDAR`]); they hold 3:1 on its `panel` already. Without one the standard's
///   chart colours stay.
/// - `chart_other`: a scheme with its own `panel` takes its `text_3`, which holds 4.5:1 there; with
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
    use std::fs;

    use super::*;
    use crate::design::testdata::Data;

    fn problems(dir: &Path, id: &str) -> Vec<DesignError> {
        match load_color_scheme(dir, id) {
            Ok(_) => panic!("{id} must be refused"),
            Err(problems) => problems,
        }
    }

    #[test]
    fn the_standard_is_loaded_without_a_folder_and_listed_first() {
        let data = Data::new();
        let dir = data.dir(Part::ColorScheme);
        assert_eq!(load_color_scheme(&dir, STANDARD_ID), Ok(ColorScheme::standard()));
        let listed = list_color_schemes(&dir);
        assert_eq!(listed.len(), 1, "a missing folder lists only Torro");
        assert!(listed[0].standard && listed[0].valid() && listed[0].about.name == "Torro");
    }

    #[test]
    fn only_an_accent_takes_everything_else_from_the_standard() {
        let data = Data::with(Part::ColorScheme, &["accent-only"]);
        let scheme = load_color_scheme(&data.dir(Part::ColorScheme), "accent-only").expect("valid");
        let mut expected = ColorScheme::standard();
        expected.id = "accent-only".into();
        expected.about = About { name: "Teal".into(), version: "1.0.0".into(), ..About::default() };
        expected.colors.dark.accent = Rgba::hex(0x0E7C7B);
        assert_eq!(scheme, expected);
    }

    #[test]
    fn a_colour_scheme_with_every_key_loads() {
        let data = Data::with(Part::ColorScheme, &["full"]);
        let scheme = load_color_scheme(&data.dir(Part::ColorScheme), "full").expect("valid");
        let root = fs::canonicalize(data.entry(Part::ColorScheme, "full")).expect("canonical");
        assert_eq!((scheme.about.version.as_str(), scheme.about.author.as_deref()), ("2.1.0", Some("Torro tests")));
        assert_eq!(scheme.colors.dark.shadow, Rgba { a: 128.0 / 255.0, ..Rgba::hex(0) });
        assert_eq!(scheme.colors.light.accent, Rgba::hex(0x0B6B6A));
        assert_eq!(scheme.colors.light.bg, ColorScheme::standard().colors.light.bg);
        assert_eq!(scheme.logo, Logo { mark: Some(root.join("logo.svg")), tray: None });
    }

    #[test]
    fn sidebar_keys_fall_back_to_the_schemes_own_colours() {
        let data = Data::new();
        let toml = "format = 1\n[about]\nname = \"Own bg\"\nversion = \"1\"\n\
                    [colors.light]\nbg = \"#ECEEF0\"\ntext_2 = \"#474B52\"\n";
        data.write(Part::ColorScheme, "own-bg", toml);
        let scheme = load_color_scheme(&data.dir(Part::ColorScheme), "own-bg").expect("valid");
        let light = scheme.colors.light;
        assert_eq!((light.sidebar, light.sidebar_text_2), (Rgba::hex(0xECEEF0), Rgba::hex(0x474B52)));
        assert_eq!(light.sidebar_hover, light.hover, "not the standard's sidebar, the scheme's own");
        assert!(!light.own_sidebar());
        assert_eq!(light.in_sidebar(), light, "without own sidebar keys the sidebar draws as before");
        assert_eq!(scheme.colors.dark, ColorScheme::standard().colors.dark);
    }

    #[test]
    fn a_dark_sidebar_beside_light_content_loads() {
        let data = Data::with(Part::ColorScheme, &["sidebar-dark"]);
        let scheme = load_color_scheme(&data.dir(Part::ColorScheme), "sidebar-dark").expect("valid");
        let light = scheme.colors.light;
        assert!(light.own_sidebar());
        assert_eq!(light.sidebar_line, Rgba { a: 31.0 / 255.0, ..Rgba::hex(0xFFFFFF) });
        let side = light.in_sidebar();
        assert_eq!((side.bg, side.panel, side.text), (Rgba::hex(0x24292F), Rgba::hex(0x24292F), Rgba::hex(0xF6F8FA)));
        assert_eq!((side.raised, side.line_strong), (light.sidebar_hover, light.sidebar_line));
        assert_eq!(side.accent, light.accent, "accent and status stay");
    }

    #[test]
    fn a_dark_sidebar_with_the_contents_text_is_refused() {
        let data = Data::with(Part::ColorScheme, &["sidebar-faint"]);
        let found = problems(&data.dir(Part::ColorScheme), "sidebar-faint");
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
    fn each_colour_rule_has_a_broken_example() {
        let names = ["low-contrast", "bad-on-accent", "path-outside", "svg-href", "tray"];
        let data = Data::with(Part::ColorScheme, &names);
        let dir = data.dir(Part::ColorScheme);
        assert!(problems(&dir, "low-contrast").iter().all(
            |p| matches!(p, DesignError::TextContrast { fg, ratio, .. } if fg == "colors.dark.text_3" && *ratio < 4.5)
        ));
        assert!(matches!(problems(&dir, "bad-on-accent")[..], [DesignError::AccentContrast { mode: Mode::Light, .. }]));
        assert_eq!(
            problems(&dir, "path-outside"),
            [DesignError::PathOutside { key: "logo.mark".into(), path: "../x.svg".into() }]
        );
        assert_eq!(
            problems(&dir, "svg-href"),
            [DesignError::SvgReference { file: "logo.svg".into(), reference: "https://example.org/logo.png".into() }]
        );
        assert_eq!(
            problems(&dir, "tray"),
            [DesignError::UnknownKey { key: "logo.tray".into() }],
            "the tray symbol is fixed (issue #114)"
        );
        assert_eq!(problems(&dir, "gone"), [DesignError::NotFound { part: Part::ColorScheme, id: "gone".into() }]);
        data.write(Part::ColorScheme, "empty", "");
        let keys: Vec<String> = problems(&dir, "empty")
            .into_iter()
            .map(|p| match p {
                DesignError::MissingKey { key } => key,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["format", "about.name", "about.version"]);
        data.write(Part::ColorScheme, "form", "format = 1\n[about]\nname = \"x\"\nversion = \"1\"\n[size]\nrow = 44\n");
        assert_eq!(problems(&dir, "form"), [DesignError::UnknownKey { key: "size".into() }], "no form in a scheme");
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_refused() {
        let data = Data::with(Part::ColorScheme, &["full"]);
        let dir = data.dir(Part::ColorScheme);
        let scheme = data.entry(Part::ColorScheme, "full");
        let outside = dir.join("elsewhere.svg");
        fs::copy(scheme.join("logo.svg"), &outside).expect("copy");
        fs::rename(scheme.join("logo.svg"), scheme.join("inside.svg")).expect("move");
        std::os::unix::fs::symlink(&outside, scheme.join("logo.svg")).expect("link");
        assert_eq!(
            problems(&dir, "full"),
            [DesignError::PathOutside { key: "logo.mark".into(), path: "logo.svg".into() }]
        );
        // A link that stays inside is fine.
        fs::remove_file(scheme.join("logo.svg")).expect("remove");
        std::os::unix::fs::symlink(scheme.join("inside.svg"), scheme.join("logo.svg")).expect("link");
        assert!(load_color_scheme(&dir, "full").is_ok());
    }

    #[test]
    fn list_shows_valid_and_broken_colour_schemes_with_reasons() {
        let data = Data::with(Part::ColorScheme, &["accent-only", "low-contrast"]);
        let listed = list_color_schemes(&data.dir(Part::ColorScheme));
        let ids: Vec<(&str, bool)> = listed.iter().map(|l| (l.id.as_str(), l.valid())).collect();
        assert_eq!(ids, [(STANDARD_ID, true), ("accent-only", true), ("low-contrast", false)]);
        assert_eq!(listed[2].about.name, "Low contrast", "a broken scheme still shows its name");
        assert!(!listed[2].problems().is_empty());
    }

    #[test]
    fn every_palette_key_has_a_field() {
        let mut p = ColorScheme::standard().colors.dark;
        for key in PALETTE_KEYS {
            assert!(p.slot_mut(key).is_some() || ["calendar", "chart", "tint"].contains(&key), "{key}");
        }
    }

    /// Task E8.8b: chart keys are read like the calendar's; left out, they follow the scheme's own
    /// calendar and `text_3` where it has them, else the standard's.
    #[test]
    fn chart_colours_are_read_and_follow_the_scheme_when_left_out() {
        let standard = ColorScheme::standard().colors.dark;
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
        let mut palette = ColorScheme::standard().colors.light;
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

    /// Task L.19: the repository's colour schemes, once task L.20 has made them, are valid.
    #[test]
    fn the_shipped_colour_schemes_stay_valid() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../color-schemes");
        for scheme in list_color_schemes(&shipped) {
            assert!(scheme.valid(), "{}: {:?}", scheme.id, scheme.problems());
        }
    }
}
