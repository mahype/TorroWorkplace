//! Reading a font folder `fonts/<id>/font.toml` (task L.19): the families of the three roles with
//! their files, the licence and the size adjustment. [`load_font`] and [`list_fonts`] are the public
//! entry points.
//!
//! The rules of a font: every file inside the folder, of the right type and size, carrying the family
//! named; `LICENSE.txt` and `license` as soon as there are files; `regular` of the text font; the size
//! adjustment within its range; the widest figure of `numbers` within [`super::DIGIT_MAX_EM`]. A
//! missing weight is no rule: it is drawn in the nearest ([`super::Font::nearest`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::files::{FontInfo, Kind, Root, has_license};
use super::pack::{AboutFile, Listed, header, listing, open, parse};
use super::rules::{self, SIZE_ADJUST_AUTO};
use super::{About, DesignError, Face, Font, FontSet, Fonts, INTER_X_HEIGHT, Part, STANDARD_ID};

/// Every font: the standard first, then each folder under `dir` (the fonts folder, see
/// [`super::font_dir`]) by name, valid or with its reasons. Hidden folders and names that are not
/// UTF-8 are skipped; a missing `dir` lists only the standard.
pub fn list_fonts(dir: &Path) -> Vec<Listed<FontSet>> {
    let standard = FontSet::standard();
    listing(dir, standard.about.clone(), standard, read)
}

/// The font `id` from the fonts folder `dir`, merged with the standard and checked.
/// [`STANDARD_ID`] gives [`FontSet::standard`] without touching the disk.
///
/// `Err` holds every reason the font can't be used (at least one).
pub fn load_font(dir: &Path, id: &str) -> Result<FontSet, Vec<DesignError>> {
    if id == STANDARD_ID {
        return Ok(FontSet::standard());
    }
    read(dir, id, &mut About::default())
}

/// `font.toml` as written; `deny_unknown_fields` backs up [`known`].
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FontSetFile {
    format: Option<i64>,
    #[serde(default)]
    about: AboutFile,
    license: Option<String>,
    size_adjust: Option<f32>,
    fonts: Option<FontsFile>,
}

/// `[fonts]`: the three roles.
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

/// The keys of `font.toml`.
fn known(path: &str, key: &str) -> Option<bool> {
    let value = |ok: bool| ok.then_some(false);
    match path {
        "" => match key {
            "format" | "license" | "size_adjust" => Some(false),
            "about" | "fonts" => Some(true),
            _ => None,
        },
        "about" => value(["name", "version", "author", "description"].contains(&key)),
        "fonts" => ["text", "heading", "numbers"].contains(&key).then_some(true),
        "fonts.text" | "fonts.heading" | "fonts.numbers" => {
            value(["family", "regular", "medium", "semibold", "bold"].contains(&key))
        }
        _ => None,
    }
}

fn read(dir: &Path, id: &str, about: &mut About) -> Result<FontSet, Vec<DesignError>> {
    let root = open(dir, id, Part::Font)?;
    let text = root.toml(Part::Font.file()).map_err(|err| vec![err])?;
    let file: FontSetFile = parse(Part::Font.file(), &text, known)?;
    *about = file.about.about(id);
    let mut problems = Vec::new();
    header(file.format, &file.about, &mut problems);
    let set = build(id, about.clone(), &root, file, &mut problems);
    if problems.is_empty() { Ok(set) } else { Err(problems) }
}

/// The font `file` in the folder `root`, checked; every broken rule goes to `problems`.
fn build(id: &str, about: About, root: &Root, file: FontSetFile, problems: &mut Vec<DesignError>) -> FontSet {
    let FontSetFile { license, size_adjust, fonts, .. } = file;
    let standard = FontSet::standard();
    let mut infos = BTreeMap::new();
    let fonts = merge_fonts(root, fonts.unwrap_or_default(), &standard.fonts, &mut infos, problems);
    let has_files = [&fonts.text, &fonts.heading, &fonts.numbers].iter().any(|font| files(font).next().is_some());
    if has_files {
        if license.is_none() {
            problems.push(DesignError::MissingKey { key: "license".into() });
        }
        if !has_license(root, "LICENSE.txt") {
            problems.push(DesignError::FontLicense);
        }
    }
    let factor = size_adjust.unwrap_or_else(|| auto_adjust(&fonts.text, &infos));
    // Figures that stay the embedded Inter Tabular have no file here to measure, but a `size_adjust`
    // still widens them: their known width keeps the rule in force.
    let widest = match fonts.numbers.regular {
        Some(Face::Builtin) => Some(BUILTIN_WIDEST_FIGURE),
        _ => files(&fonts.numbers).filter_map(|path| infos.get(path)?.widest_figure).reduce(f32::max),
    };
    problems.extend(rules::font(size_adjust, factor, &fonts.numbers.family, widest));
    FontSet { id: id.to_string(), about, license: license.unwrap_or(standard.license), fonts, size_adjust: factor }
}

/// The widest figure of the embedded Inter Tabular, in em (1328 of 2048 units), for the figures rule of
/// a font that keeps the embedded figures.
const BUILTIN_WIDEST_FIGURE: f32 = 1328.0 / 2048.0;

/// The size adjustment from the x-height of `text`'s regular face (task L.19): Inter's divided by its
/// own, held to [`SIZE_ADJUST_AUTO`]. 1 for the embedded Inter and for a face without an x-height.
fn auto_adjust(text: &Font, infos: &BTreeMap<PathBuf, FontInfo>) -> f32 {
    match &text.regular {
        Some(Face::File(path)) => infos.get(path).and_then(|info| info.x_height).map_or(1.0, size_adjust_for),
        _ => 1.0,
    }
}

/// The size adjustment for a text font with this x-height (em).
pub(super) fn size_adjust_for(x_height: f32) -> f32 {
    (INTER_X_HEIGHT / x_height).clamp(SIZE_ADJUST_AUTO.0, SIZE_ADJUST_AUTO.1)
}

/// The files of `font`'s faces.
fn files(font: &Font) -> impl Iterator<Item = &PathBuf> {
    [&font.regular, &font.medium, &font.semibold, &font.bold].into_iter().filter_map(|face| match face {
        Some(Face::File(path)) => Some(path),
        _ => None,
    })
}

/// The three roles. With `[fonts.text]`, a missing `heading` or `numbers` is the text font; without
/// it, they stay the standard's (so a font may change only the headings).
fn merge_fonts(
    root: &Root,
    file: FontsFile,
    standard: &Fonts,
    infos: &mut BTreeMap<PathBuf, FontInfo>,
    problems: &mut Vec<DesignError>,
) -> Fonts {
    let mut read = |role: &str, font: Option<FontFile>| font.map(|f| read_font(root, role, f, infos, problems));
    let text = read("text", file.text);
    let heading = read("heading", file.heading);
    let numbers = read("numbers", file.numbers);
    let base = text.clone();
    Fonts {
        text: text.unwrap_or_else(|| standard.text.clone()),
        heading: heading.or_else(|| base.clone()).unwrap_or_else(|| standard.heading.clone()),
        numbers: numbers.or(base).unwrap_or_else(|| standard.numbers.clone()),
    }
}

/// One `[fonts.<role>]`: family, the files that exist, each checked to be a font of that family; what
/// was read of each file goes to `infos`.
fn read_font(
    root: &Root,
    role: &str,
    file: FontFile,
    infos: &mut BTreeMap<PathBuf, FontInfo>,
    problems: &mut Vec<DesignError>,
) -> Font {
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
        if !infos.contains_key(&path) {
            match root.font_info(&path) {
                Ok(info) => {
                    infos.insert(path.clone(), info);
                }
                Err(err) => problems.push(err),
            }
        }
        if let Some(info) = infos.get(&path)
            && !family.is_empty()
            && !info.families.iter().any(|name| name == &family)
        {
            let found = info.families.clone();
            problems.push(DesignError::FontFamily { file: root.show(&path), family: family.clone(), found });
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::design::testdata::{Data, font_file};
    use crate::design::{DIGIT_MAX_EM, LevelName, Selection, Weight, files};

    fn problems(dir: &Path, id: &str) -> Vec<DesignError> {
        match load_font(dir, id) {
            Ok(_) => panic!("{id} must be refused"),
            Err(problems) => problems,
        }
    }

    #[test]
    fn the_standard_is_loaded_without_a_folder_and_listed_first() {
        let data = Data::new();
        assert_eq!(load_font(&data.dir(Part::Font), STANDARD_ID), Ok(FontSet::standard()));
        let listed = list_fonts(&data.dir(Part::Font));
        assert_eq!(listed.len(), 1, "a missing folder lists only Torro");
        assert!(listed[0].standard && listed[0].valid());
        assert_eq!(listed[0].result.as_ref().map(|font| font.fonts.text.family.as_str()), Ok("Inter"));
    }

    /// A font that keeps the embedded figures and only enlarges them (`size_adjust` alone) is held to
    /// the figures bound too: 0.648 × 1.2 is wider than the time axis holds.
    #[test]
    fn embedded_figures_are_held_to_the_bound_too() {
        let data = Data::new();
        let head = "format = 1\n\n[about]\nname = \"Loose\"\nversion = \"1.0.0\"\n";
        data.write(Part::Font, "loose", &format!("size_adjust = 1.2\n{head}"));
        assert!(
            matches!(&problems(&data.dir(Part::Font), "loose")[..],
                [DesignError::DigitsTooWide { family, .. }] if family == "Inter Tabular"),
            "{:?}",
            problems(&data.dir(Part::Font), "loose")
        );
        data.write(Part::Font, "loose", &format!("size_adjust = 1.1\n{head}"));
        assert!(load_font(&data.dir(Part::Font), "loose").is_ok(), "0.648 × 1.1 = 0.713 still fits");
    }

    /// A `font.toml` that is a link to a file outside the font's folder is refused, never read.
    #[cfg(unix)]
    #[test]
    fn a_font_toml_linked_from_outside_is_not_read() {
        let data = Data::new();
        let outside = data.dir(Part::Font).with_file_name("elsewhere.toml");
        fs::create_dir_all(data.entry(Part::Font, "linked")).expect("entry folder");
        fs::write(&outside, "format = 1\nsecret_key = 1\n").expect("outside");
        std::os::unix::fs::symlink(&outside, data.entry(Part::Font, "linked").join("font.toml")).expect("link");
        assert_eq!(
            problems(&data.dir(Part::Font), "linked"),
            [DesignError::PathOutside { key: "font.toml".into(), path: "font.toml".into() }]
        );
    }

    /// Task L.19: Inter is the reference, so Inter from a folder has a size adjustment of exactly 1.
    #[test]
    fn a_font_with_every_key_loads_and_inter_adjusts_by_exactly_one() {
        let data = Data::with(Part::Font, &["full"]);
        let font = load_font(&data.dir(Part::Font), "full").expect("valid");
        let root = fs::canonicalize(data.entry(Part::Font, "full")).expect("canonical");
        assert_eq!((font.about.name.as_str(), font.license.as_str()), ("Full", "OFL-1.1"));
        assert_eq!(font.fonts.text.regular, Some(Face::File(root.join("Inter-Regular.ttf"))));
        assert_eq!(font.fonts.heading.regular, None);
        assert_eq!(font.fonts.numbers.family, "Inter Tabular");
        assert_eq!(font.size_adjust, 1.0);

        // Without `size_adjust` it is computed from the x-height, and still exactly 1.
        let toml = data.entry(Part::Font, "full").join("font.toml");
        let text = fs::read_to_string(&toml).expect("read").replace("size_adjust = 1.0\n", "");
        fs::write(&toml, text).expect("write");
        assert_eq!(load_font(&data.dir(Part::Font), "full").expect("valid").size_adjust, 1.0);
        assert_eq!(FontSet::standard().size_adjust, 1.0);
    }

    /// Task L.19: IBM Plex Sans is drawn a little larger, after its x-height (516 of 1000 units).
    #[test]
    fn plex_adjusts_by_its_x_height_and_a_set_value_wins() {
        let data = Data::with(Part::Font, &["plex", "plex-set"]);
        let plex = load_font(&data.dir(Part::Font), "plex").expect("valid");
        assert!((plex.size_adjust - INTER_X_HEIGHT / 0.516).abs() < 1e-6, "{}", plex.size_adjust);
        assert!((plex.size_adjust - 1.058).abs() < 0.001);
        assert_eq!(plex.fonts.numbers, plex.fonts.text, "no figures font: the text font's");
        let selection = Selection { font: plex, ..Selection::standard() };
        assert_eq!((selection.level(LevelName::Body).size, selection.level(LevelName::Body).line), (14.0, 20.0));
        assert_eq!(selection.level(LevelName::Micro).size, 11.5);

        let set = load_font(&data.dir(Part::Font), "plex-set").expect("valid");
        assert_eq!(set.size_adjust, 1.2, "`size_adjust` goes before the x-height");
    }

    /// Task L.19: the size adjustment from the x-height is held to 0.90–1.15; set by hand it may be
    /// 0.85–1.25.
    #[test]
    fn the_size_adjustment_keeps_its_bounds() {
        assert_eq!(size_adjust_for(INTER_X_HEIGHT), 1.0);
        assert_eq!(size_adjust_for(0.40), 1.15, "a tiny x-height");
        assert_eq!(size_adjust_for(0.70), 0.90, "a huge x-height");
        let data = Data::with(Part::Font, &["bad-adjust"]);
        assert_eq!(
            problems(&data.dir(Part::Font), "bad-adjust"),
            [DesignError::OutOfRange { key: "size_adjust".into(), value: 0.8, min: 0.85, max: 1.25 }]
        );
    }

    /// Task L.19: Lato has no Medium and no SemiBold; it stays valid and draws the nearest weights.
    #[test]
    fn lato_without_medium_draws_the_nearest_weights() {
        let data = Data::with(Part::Font, &["lato"]);
        let lato = load_font(&data.dir(Part::Font), "lato").expect("a missing weight is no reason");
        assert!(lato.fonts.text.medium.is_none() && lato.fonts.text.semibold.is_none());
        let selection = Selection { font: lato, ..Selection::standard() };
        let weight = |name| selection.level(name).weight;
        assert_eq!(weight(LevelName::Caption), Weight::Regular, "medium → semibold → regular");
        assert_eq!(weight(LevelName::BodyStrong), Weight::Regular);
        assert_eq!(weight(LevelName::Title), Weight::Bold, "semibold → bold");
        assert_eq!(weight(LevelName::Countdown), Weight::Regular, "the figures font is Lato too");
        assert_eq!(weight(LevelName::Body), Weight::Regular);
    }

    #[test]
    fn each_font_rule_has_a_broken_example() {
        let data = Data::with(Part::Font, &["font-no-license", "wrong-family", "wide-figures"]);
        let dir = data.dir(Part::Font);
        assert_eq!(problems(&dir, "font-no-license"), [DesignError::FontLicense]);
        let wrong = problems(&dir, "wrong-family");
        assert_eq!(wrong.len(), 3, "one per file: {wrong:?}");
        assert!(wrong.iter().all(|p| matches!(p, DesignError::FontFamily { family, found, .. }
            if family == "IBM Plex Sans" && found.contains(&"Inter".to_string()))));
        let wide = problems(&dir, "wide-figures");
        assert!(
            matches!(&wide[..], [DesignError::DigitsTooWide { family, width, max }]
                if family == "Inter Tabular" && (*width - 0.778).abs() < 0.001 && *max == DIGIT_MAX_EM),
            "{wide:?}"
        );

        let toml = |body: &str| format!("format = 1\n[about]\nname = \"x\"\nversion = \"1\"\n{body}");
        data.write(Part::Font, "no-key", &toml("[fonts.text]\nfamily = \"Inter\"\nregular = \"Inter-Regular.ttf\"\n"));
        fs::copy(font_file("Inter-Regular.ttf"), data.entry(Part::Font, "no-key").join("Inter-Regular.ttf"))
            .expect("copy");
        fs::write(data.entry(Part::Font, "no-key").join("LICENSE.txt"), "OFL").expect("licence");
        assert_eq!(problems(&dir, "no-key"), [DesignError::MissingKey { key: "license".into() }]);
        data.write(Part::Font, "no-regular", &toml("[fonts.text]\nfamily = \"Inter\"\n"));
        assert_eq!(problems(&dir, "no-regular"), [DesignError::MissingKey { key: "fonts.text.regular".into() }]);
        data.write(Part::Font, "typo", &format!("licence = \"OFL-1.1\"\n{}", toml("[fonts.text]\nfamly = \"x\"\n")));
        let keys = [
            DesignError::UnknownKey { key: "fonts.text.famly".into() },
            DesignError::UnknownKey { key: "licence".into() },
        ];
        assert_eq!(problems(&dir, "typo"), keys);
        data.write(Part::Font, "outside", &toml("[fonts.text]\nfamily = \"Inter\"\nregular = \"../x.ttf\"\n"));
        assert_eq!(
            problems(&dir, "outside"),
            [DesignError::PathOutside { key: "fonts.text.regular".into(), path: "../x.ttf".into() }]
        );
        assert_eq!(problems(&dir, "gone"), [DesignError::NotFound { part: Part::Font, id: "gone".into() }]);
    }

    #[test]
    fn list_shows_valid_and_broken_fonts_with_reasons() {
        let data = Data::with(Part::Font, &["lato", "font-no-license"]);
        let listed = list_fonts(&data.dir(Part::Font));
        let ids: Vec<(&str, bool)> = listed.iter().map(|l| (l.id.as_str(), l.valid())).collect();
        assert_eq!(ids, [(STANDARD_ID, true), ("font-no-license", false), ("lato", true)]);
        assert_eq!(listed[1].about.name, "No license");
        assert_eq!(listed[1].problems(), [DesignError::FontLicense]);
    }

    /// Task L.19: the bound for the figures, [`DIGIT_MAX_EM`], is the widest figure of the samples'
    /// figures fonts after their size adjustment, plus 5 %. The fonts are the repository's.
    #[test]
    fn the_figures_bound_is_the_widest_sample_plus_five_percent() {
        let info = |file: &str| files::font_info(&fs::read(font_file(file)).expect("read")).expect("a font");
        // The text font's regular face (for the adjustment) and the faces of the figures font.
        let samples: [(&str, &[&str]); 7] = [
            ("Inter-Regular.ttf", &["InterTabular-Regular.ttf", "InterTabular-Medium.ttf"]),
            (
                "IBMPlexSans-Regular.ttf",
                &[
                    "IBMPlexSans-Regular.ttf",
                    "IBMPlexSans-Medium.ttf",
                    "IBMPlexSans-SemiBold.ttf",
                    "IBMPlexSans-Bold.ttf",
                ],
            ),
            ("MonaSans-Regular.ttf", &["CommitrFigures-Regular.ttf", "CommitrFigures-Medium.ttf"]),
            ("Lato-Regular.ttf", &["Lato-Regular.ttf", "Lato-Bold.ttf"]),
            ("Figtree-Regular.ttf", &["FigtreeTabular-Regular.ttf", "FigtreeTabular-Medium.ttf"]),
            (
                "RobotoFlex-Regular.ttf",
                &["RobotoFlex-Regular.ttf", "RobotoFlex-Medium.ttf", "RobotoFlex-SemiBold.ttf", "RobotoFlex-Bold.ttf"],
            ),
            ("Selawik-Regular.ttf", &["Selawik-Regular.ttf", "Selawik-SemiBold.ttf", "Selawik-Bold.ttf"]),
        ];
        let mut widest: f32 = 0.0;
        for (text, figures) in samples {
            let factor = info(text).x_height.map_or(1.0, size_adjust_for);
            for face in figures {
                let width = info(face).widest_figure.expect("figures") * factor.max(1.0);
                assert!(width <= DIGIT_MAX_EM, "{face}: {width}");
                widest = widest.max(width);
            }
        }
        let bound = widest * 1.05;
        assert!(bound <= DIGIT_MAX_EM && DIGIT_MAX_EM - bound < 0.01, "widest {widest}, bound {bound}");
        assert!((widest - 0.6835).abs() < 0.001, "Figtree Tabular Medium is the widest: {widest}");
    }
}
