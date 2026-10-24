//! The fonts installed on the computer (issue #299): used by their family name, never copied and never
//! handed on. Only the id is stored (`system:<family>`, or [`SYSTEM_ID`] for the system's interface
//! font); a computer without that family draws in the standard, because [`SystemFonts::load`] answers
//! [`DesignError::NotFound`] then.
//!
//! [`SystemFonts`] is one scan of the installed fonts (fontdb, the library Iced's text stack loads
//! them with). Each family is checked on its upright face of weight 400 as a font folder would be:
//! families whose name is hidden, too long or holds invisible characters, that have no upright face,
//! or whose regular face lacks the letters of German and English text are left out altogether
//! (symbols, emoji, other scripts); the rest is listed, valid or with its reasons. A family whose
//! figures are not all equally wide gets the embedded Inter Tabular for its figures (decided in the
//! planning of #299: refusing them would refuse a third of a Mac's fonts).
//!
//! The files are only read (mapped by fontdb) to check them; nothing is written. A family name comes
//! from a file of someone else and becomes part of an id and of messages to agents, so it is held to
//! [`is_family_name`], and reasons name the family only, never a file: a path would tell the user's
//! name and home folder.

use std::collections::BTreeMap;

use fontdb::{Database, FaceInfo};

use super::files::font_info_at;
use super::font::{BUILTIN_WIDEST_FIGURE, size_adjust_for};
use super::{
    About, DIGIT_MAX_EM, DesignError, Face, Font, FontSet, Fonts, Listed, Part, SYSTEM_ID, SYSTEM_PREFIX, rules,
};

/// The longest family name taken in, in characters.
const NAME_MAX: usize = 64;

/// The letters a family's regular face must have to be listed: English and German text.
const LETTERS: [char; 5] = ['ä', 'ö', 'ü', 'ß', 'é'];

/// The weight of a regular face.
const REGULAR: u16 = 400;

/// One scan of the fonts installed on the computer.
pub struct SystemFonts {
    db: Database,
}

/// The faces of one family that count: upright ones.
struct Family<'a> {
    name: String,
    faces: Vec<&'a FaceInfo>,
}

/// What one family's regular face says about it.
struct Examined {
    /// Every letter of the alphabet and of [`LETTERS`].
    latin: bool,
    /// The ten figures are all equally wide.
    equal_figures: bool,
    /// A variable font with a `wght` axis.
    variable_weight: bool,
    x_height: Option<f32>,
    widest_figure: Option<f32>,
}

impl SystemFonts {
    /// Scans the fonts installed on the computer (about 100 ms on a Mac). Nothing is kept between
    /// scans, so a font installed meanwhile shows up in the next one.
    pub fn scan() -> SystemFonts {
        let mut db = Database::new();
        db.load_system_fonts();
        SystemFonts { db }
    }

    /// The fonts of `db` instead of the computer's: for tests, which may not depend on what is
    /// installed.
    pub fn from_database(db: Database) -> SystemFonts {
        SystemFonts { db }
    }

    /// [`SYSTEM_ID`] first ("System", the family it resolves to, or [`DesignError::NotFound`] when the
    /// computer has none of them), then every family that is not left out, by name, valid or with its
    /// reasons. Ids are `system:<family>`.
    pub fn list(&self) -> Vec<Listed<FontSet>> {
        let families = self.families();
        let examined: Vec<(String, Result<FontSet, Vec<DesignError>>)> = families
            .iter()
            .filter_map(|family| {
                self.font(family, &format!("{SYSTEM_PREFIX}{}", family.name)).map(|r| (family.name.clone(), r))
            })
            .collect();
        let system = self
            .system_family(&families)
            .and_then(|family| examined.iter().find(|(name, _)| *name == family.name))
            .map(|(_, result)| result.clone().map(as_system))
            .unwrap_or_else(|| Err(not_found(SYSTEM_ID)));
        let mut listed =
            vec![Listed { id: SYSTEM_ID.to_string(), about: system_about(), standard: false, result: system }];
        // "System" stands for the interface font already; its alias would be a second entry for it.
        let families = examined.into_iter().filter(|(name, _)| !system_aliases().contains(&name.as_str()));
        listed.extend(families.map(|(name, result)| Listed {
            id: format!("{SYSTEM_PREFIX}{name}"),
            about: About { name, ..About::default() },
            standard: false,
            result,
        }));
        listed
    }

    /// The font `id`: [`SYSTEM_ID`] resolved to the system's interface font, or `system:<family>`.
    /// [`DesignError::NotFound`] for a family that is not installed, is left out by the rules above,
    /// or an id of neither form.
    pub fn load(&self, id: &str) -> Result<FontSet, Vec<DesignError>> {
        let families = self.families();
        if id == SYSTEM_ID {
            return self
                .system_family(&families)
                .and_then(|family| self.font(family, SYSTEM_ID))
                .map(|result| result.map(as_system))
                .unwrap_or_else(|| Err(not_found(id)));
        }
        let name = id.strip_prefix(SYSTEM_PREFIX).filter(|name| is_family_name(name));
        name.and_then(|name| families.iter().find(|family| family.name == name))
            .and_then(|family| self.font(family, id))
            .unwrap_or_else(|| Err(not_found(id)))
    }

    /// The families with a name [`is_family_name`] takes and at least one upright face, by name
    /// (ignoring case).
    fn families(&self) -> Vec<Family<'_>> {
        let mut by_name: BTreeMap<String, Vec<&FaceInfo>> = BTreeMap::new();
        for face in self.db.faces().filter(|face| face.style == fontdb::Style::Normal) {
            if let Some((name, _)) = face.families.first()
                && is_family_name(name)
            {
                by_name.entry(name.clone()).or_default().push(face);
            }
        }
        let mut families: Vec<Family<'_>> = by_name.into_iter().map(|(name, faces)| Family { name, faces }).collect();
        families.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
        families
    }

    /// The first family of [`system_candidates`] that is installed and listed.
    fn system_family<'a>(&self, families: &'a [Family<'a>]) -> Option<&'a Family<'a>> {
        system_candidates().iter().find_map(|wanted| {
            families.iter().find(|family| family.name == *wanted && self.examine(family).is_none_or(|e| e.latin))
        })
    }

    /// `family` as the font `id`, checked; `None` when it is left out (its regular face lacks
    /// [`LETTERS`]).
    fn font(&self, family: &Family<'_>, id: &str) -> Option<Result<FontSet, Vec<DesignError>>> {
        let Some(examined) = self.examine(family) else {
            return Some(Err(vec![DesignError::SystemUnreadable { family: family.name.clone() }]));
        };
        if !examined.latin {
            return None;
        }
        let weights: Vec<u16> = family.faces.iter().map(|face| face.weight.0).collect();
        if !weights.contains(&REGULAR) {
            return Some(Err(vec![DesignError::SystemNoRegular { family: family.name.clone() }]));
        }
        let [regular, medium, semibold, bold] = present_weights(&weights, examined.variable_weight);
        let face = |present: bool| present.then_some(Face::System);
        let text = Font {
            family: family.name.clone(),
            regular: face(regular),
            medium: face(medium),
            semibold: face(semibold),
            bold: face(bold),
        };
        let standard = FontSet::standard();
        // Proportional figures would shift a clock as it ticks: such a family takes Inter Tabular for its
        // figures, measured with the embedded file's width.
        let (numbers, widest) = if examined.equal_figures {
            (text.clone(), examined.widest_figure)
        } else {
            (standard.fonts.numbers.clone(), Some(BUILTIN_WIDEST_FIGURE))
        };
        let factor = figures_adjust(examined.x_height.map_or(1.0, size_adjust_for), examined.equal_figures);
        let problems = rules::font(None, factor, &numbers.family, widest);
        if !problems.is_empty() {
            return Some(Err(problems));
        }
        Some(Ok(FontSet {
            id: id.to_string(),
            about: About { name: family.name.clone(), ..About::default() },
            license: String::new(),
            fonts: Fonts { heading: text.clone(), text, numbers },
            size_adjust: factor,
            kind: None,
        }))
    }

    /// What the regular face of `family` says (the upright face nearest to weight 400); `None` when
    /// its file can't be read or parsed.
    fn examine(&self, family: &Family<'_>) -> Option<Examined> {
        let face = family.faces.iter().min_by_key(|face| face.weight.0.abs_diff(REGULAR))?;
        self.db.with_face_data(face.id, examine).flatten()
    }
}

/// Every font of the computer, from one fresh [`SystemFonts::scan`] ([`SystemFonts::list`]).
pub fn list_system_fonts() -> Vec<Listed<FontSet>> {
    SystemFonts::scan().list()
}

/// Whether `name`, a family name read from a font file, may become part of an id (`system:<name>`):
/// not empty, at most 64 characters, not hidden (`.SF NS`, the system's private faces) and without
/// control or format characters, which could change what a person reads in a list or an approval.
pub fn is_family_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_MAX
        && !name.starts_with('.')
        && !name.chars().any(|c| c.is_control() || is_format(c))
}

/// Which of regular, medium, semibold and bold a family has, from the weights of its upright faces:
/// each where a face has exactly that weight (a missing one is drawn in the nearest,
/// [`Font::nearest`]). A variable regular face with a `wght` axis also counts as medium and semibold,
/// but not as bold: tried in the planning of #299, Iced draws SF at 500 and 600 from the axis and
/// falls back to another font at 700.
pub(super) fn present_weights(weights: &[u16], variable_weight: bool) -> [bool; 4] {
    let has = |weight: u16| weights.contains(&weight);
    [has(REGULAR), has(500) || variable_weight, has(600) || variable_weight, has(700)]
}

/// [`Examined`] of face `index` of the font `data`; `None` when it doesn't parse.
fn examine(data: &[u8], index: u32) -> Option<Examined> {
    let face = ttf_parser::Face::parse(data, index).ok()?;
    let info = font_info_at(data, index)?;
    let latin = latin(|c| face.glyph_index(c).is_some());
    let advances: Vec<Option<u16>> =
        ('0'..='9').map(|c| face.glyph_index(c).and_then(|glyph| face.glyph_hor_advance(glyph))).collect();
    let equal_figures = advances.iter().all(|advance| advance.is_some() && *advance == advances[0]);
    let wght = ttf_parser::Tag::from_bytes(b"wght");
    let variable_weight = face.is_variable() && face.variation_axes().into_iter().any(|axis| axis.tag == wght);
    Some(Examined { latin, equal_figures, variable_weight, x_height: info.x_height, widest_figure: info.widest_figure })
}

/// Whether a face with these glyphs sets English and German text: A–Z, a–z and [`LETTERS`].
fn latin(has_glyph: impl Fn(char) -> bool) -> bool {
    ('A'..='Z').chain('a'..='z').chain(LETTERS).all(has_glyph)
}

/// The size adjustment of a family: `factor` from its x-height, but for one that draws its figures in
/// Inter Tabular (`equal_figures` false) at most what keeps those within [`DIGIT_MAX_EM`] – 0.72 / 0.648,
/// cut to 1.110. Decided in the planning of #299: such a family gets Inter Tabular instead of being
/// refused, and a small x-height (Georgia, Didot) would otherwise widen the borrowed figures past the
/// time axis. Its text is then drawn a little smaller than Inter's x-height asks for.
fn figures_adjust(factor: f32, equal_figures: bool) -> f32 {
    let most = (DIGIT_MAX_EM / BUILTIN_WIDEST_FIGURE * 1000.0).floor() / 1000.0;
    if equal_figures { factor } else { factor.min(most) }
}

/// Names under which the interface font is installed a second time, left out of the families because
/// "System" stands for it: macOS lists SF as the family "System Font".
fn system_aliases() -> &'static [&'static str] {
    if cfg!(target_os = "macos") { &["System Font"] } else { &[] }
}

/// The families that stand for the system's interface font, the first installed one wins: SF on
/// macOS, Segoe UI on Windows, on Linux what fontconfig answers for `sans-serif`.
fn system_candidates() -> Vec<String> {
    #[cfg(target_os = "macos")]
    let candidates = vec!["System Font".to_string()];
    #[cfg(target_os = "windows")]
    let candidates = ["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"].map(String::from).to_vec();
    #[cfg(target_os = "linux")]
    let candidates = fontconfig_sans().into_iter().collect();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let candidates = Vec::new();
    candidates
}

/// The family fontconfig chooses for `sans-serif` (`fc-match`), the desktop's interface font.
#[cfg(target_os = "linux")]
fn fontconfig_sans() -> Option<String> {
    let output = std::process::Command::new("fc-match").args(["-f", "%{family[0]}", "sans-serif"]).output().ok()?;
    let family = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && is_family_name(&family)).then_some(family)
}

/// `font` as the entry "System": its id and name, the family it resolved to stays in its fonts.
fn as_system(font: FontSet) -> FontSet {
    FontSet { id: SYSTEM_ID.to_string(), about: system_about(), ..font }
}

fn system_about() -> About {
    About { name: "System".to_string(), ..About::default() }
}

fn not_found(id: &str) -> Vec<DesignError> {
    vec![DesignError::NotFound { part: Part::Font, id: id.to_string() }]
}

/// A Unicode format character (general category Cf, Unicode 16): invisible, yet able to change what a
/// person reads (bidi controls, zero-width characters, tags). The same ranges as the approvals'
/// check in the core service; Rust's standard library has no category lookup.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use fontdb::{Language, Stretch, Style};

    use super::*;
    use crate::design::testdata::font_file;
    use crate::design::{STANDARD_ID, Weight, is_valid_id, load_font};

    /// The face of the file `name` under the family `family` at `weight`, as fontdb would list it.
    fn face(name: &str, family: &str, weight: u16) -> FaceInfo {
        let data = std::fs::read(font_file(name)).expect("a test font");
        FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(Arc::new(data)),
            index: 0,
            families: vec![(family.to_string(), Language::English_UnitedStates)],
            post_script_name: family.replace(' ', ""),
            style: Style::Normal,
            weight: fontdb::Weight(weight),
            stretch: Stretch::Normal,
            monospaced: false,
        }
    }

    fn fonts(faces: Vec<FaceInfo>) -> SystemFonts {
        let mut db = Database::new();
        for face in faces {
            db.push_face_info(face);
        }
        SystemFonts::from_database(db)
    }

    /// The test fonts as a computer could have them: Plex (equal figures, four weights), Mona Sans
    /// (proportional figures), Lato (regular and bold only), Inter Tabular as a family of its own.
    fn computer() -> SystemFonts {
        let mut db = Database::new();
        for file in [
            "IBMPlexSans-Regular.ttf",
            "IBMPlexSans-Medium.ttf",
            "IBMPlexSans-SemiBold.ttf",
            "IBMPlexSans-Bold.ttf",
            "MonaSans-Regular.ttf",
            "Lato-Regular.ttf",
            "Lato-Bold.ttf",
        ] {
            db.load_font_data(std::fs::read(font_file(file)).expect("a test font"));
        }
        SystemFonts::from_database(db)
    }

    fn loaded(fonts: &SystemFonts, id: &str) -> FontSet {
        fonts.load(id).unwrap_or_else(|problems| panic!("{id}: {problems:?}"))
    }

    #[test]
    fn families_are_listed_after_system_by_name() {
        let listed = computer().list();
        let ids: Vec<&str> = listed.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, [SYSTEM_ID, "system:IBM Plex Sans", "system:Lato", "system:Mona Sans"]);
        assert!(listed.iter().skip(1).all(|entry| entry.valid() && !entry.standard), "{listed:?}");
        assert_eq!(listed[1].about.name, "IBM Plex Sans");
        let plex = listed[1].result.as_ref().expect("valid");
        assert_eq!((plex.license.as_str(), plex.kind), ("", None), "used, never handed on; no kind");
    }

    #[test]
    fn weights_are_the_faces_there_and_the_nearest_stands_in() {
        let fonts = computer();
        let plex = loaded(&fonts, "system:IBM Plex Sans");
        assert_eq!(plex.fonts.text.family, "IBM Plex Sans");
        for weight in [Weight::Regular, Weight::Medium, Weight::Semibold, Weight::Bold] {
            assert_eq!(plex.fonts.text.face(weight), Some(&Face::System), "{weight}");
        }
        let lato = loaded(&fonts, "system:Lato");
        assert_eq!((lato.fonts.text.medium.as_ref(), lato.fonts.text.semibold.as_ref()), (None, None));
        assert_eq!(lato.fonts.text.nearest(Weight::Medium), Weight::Regular);
        assert_eq!(lato.fonts.text.nearest(Weight::Semibold), Weight::Bold);
        assert_eq!(lato.fonts.heading, lato.fonts.text, "headings in the family too");
    }

    /// Task SR.1: the repository has no variable test font, so the rule for one is tested on its own.
    #[test]
    fn a_variable_regular_face_has_medium_and_semibold_but_no_bold() {
        assert_eq!(present_weights(&[400], true), [true, true, true, false]);
        assert_eq!(present_weights(&[400], false), [true, false, false, false]);
        assert_eq!(present_weights(&[400, 700], false), [true, false, false, true]);
        assert_eq!(present_weights(&[300, 500, 600], false), [false, true, true, false]);
    }

    /// Decided in the planning of #299, and kept by the review: a family with proportional figures and a
    /// small x-height is drawn a little smaller so the borrowed Inter Tabular still fits the time axis,
    /// instead of being refused. Its own equal figures follow its own size adjustment.
    #[test]
    fn borrowed_figures_hold_the_size_adjustment_down() {
        let most = figures_adjust(1.15, false);
        assert!((most - 1.110).abs() < 1e-6, "{most}");
        assert_eq!(rules::font(None, most, "Inter Tabular", Some(BUILTIN_WIDEST_FIGURE)), Vec::new());
        assert!(!rules::font(None, 1.15, "Inter Tabular", Some(BUILTIN_WIDEST_FIGURE)).is_empty(), "uncapped");
        assert_eq!(figures_adjust(1.15, true), 1.15, "its own figures");
        assert_eq!(figures_adjust(1.05, false), 1.05, "below the cap nothing changes");
    }

    /// macOS installs SF a second time as "System Font": "System" stands for it, so it is no family
    /// entry of its own, while "System" itself resolves to it.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_interface_font_is_listed_once() {
        let fonts = fonts(vec![face("Inter-Regular.ttf", "System Font", 400), face("Lato-Regular.ttf", "Lato", 400)]);
        let listed = fonts.list();
        let ids: Vec<&str> = listed.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, [SYSTEM_ID, "system:Lato"]);
        let system = listed[0].result.as_ref().map(|font| font.fonts.text.family.as_str());
        assert_eq!(system, Ok("System Font"), "listed as usable, not only loadable");
        assert_eq!(fonts.load(SYSTEM_ID).map(|font| font.fonts.text.family), Ok("System Font".to_string()));
    }

    /// Decided in the planning of #299: proportional figures make a family take Inter Tabular for its
    /// figures; equal ones stay its own.
    #[test]
    fn proportional_figures_are_drawn_in_inter_tabular() {
        let fonts = computer();
        let mona = loaded(&fonts, "system:Mona Sans");
        assert_eq!(mona.fonts.numbers, FontSet::standard().fonts.numbers);
        assert_eq!(mona.fonts.numbers.regular, Some(Face::Builtin));
        let plex = loaded(&fonts, "system:IBM Plex Sans");
        assert_eq!(plex.fonts.numbers, plex.fonts.text);
        assert!((plex.size_adjust - 1.058).abs() < 0.001, "after the x-height: {}", plex.size_adjust);
    }

    #[test]
    fn a_family_not_installed_is_not_found() {
        let fonts = computer();
        for id in ["system:Fehlt", "system:", "system:lato", "plex", STANDARD_ID] {
            assert_eq!(fonts.load(id), Err(not_found(id)), "{id}");
        }
        // The test computer has none of the interface fonts.
        assert_eq!(fonts.load(SYSTEM_ID), Err(not_found(SYSTEM_ID)));
        assert_eq!(fonts.list()[0].problems(), not_found(SYSTEM_ID));
        assert_eq!(fonts.list()[0].about.name, "System");
    }

    #[test]
    fn system_is_reserved_and_routed_by_load_font() {
        assert!(!is_valid_id(SYSTEM_ID));
        let gone = load_font(&PathBuf::from("/no/such/folder"), "system:Torro Test Font That Is Never Installed");
        assert!(
            matches!(&gone, Err(problems) if matches!(problems[..], [DesignError::NotFound { part: Part::Font, .. }]))
        );
    }

    #[test]
    fn a_family_without_a_regular_face_is_listed_with_its_reason() {
        let fonts = fonts(vec![face("IBMPlexSans-Medium.ttf", "Plex Medium Only", 500)]);
        let reason = DesignError::SystemNoRegular { family: "Plex Medium Only".into() };
        assert_eq!(fonts.load("system:Plex Medium Only"), Err(vec![reason.clone()]));
        assert_eq!(fonts.list()[1].problems(), [reason]);
    }

    #[test]
    fn hidden_long_and_invisible_names_are_left_out() {
        let long = "L".repeat(NAME_MAX + 1);
        let names = [".SF NS", long.as_str(), "Evil\u{202E}Name", "Tab\tName", "Zero\u{200B}Width", ""];
        let mut faces: Vec<FaceInfo> = names.iter().map(|name| face("Lato-Regular.ttf", name, 400)).collect();
        faces.push(face("Lato-Regular.ttf", &"K".repeat(NAME_MAX), 400));
        let fonts = fonts(faces);
        let ids: Vec<String> = fonts.list().into_iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [SYSTEM_ID.to_string(), format!("{SYSTEM_PREFIX}{}", "K".repeat(NAME_MAX))]);
        for name in names {
            let id = format!("{SYSTEM_PREFIX}{name}");
            assert_eq!(fonts.load(&id).err(), Some(not_found(&id)), "{name:?}");
        }
    }

    /// An italic face alone is no entry, and neither is a face without the letters of German text (the
    /// repository has no font without them, so the letter check is tested on its own too).
    #[test]
    fn families_without_an_upright_latin_face_are_left_out() {
        let italic = FaceInfo { style: Style::Italic, ..face("Lato-Regular.ttf", "Only Italic", 400) };
        let fonts = fonts(vec![italic, face("Inter-Regular.ttf", "Latin", 400)]);
        let ids: Vec<String> = fonts.list().into_iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [SYSTEM_ID, "system:Latin"]);
        assert!(latin(|_| true));
        assert!(!latin(|c| c.is_ascii_alphabetic()), "English letters only: no ä, ß, é");
        assert!(!latin(|c| c != 'Q'));
        assert!(examine(b"\0\x01\0\0", 0).is_none(), "not a font: unreadable");
    }

    /// The reasons of a font of the computer name its family, never a file: the path would tell an agent
    /// the user's name.
    #[test]
    fn no_reason_names_a_path() {
        let secret = PathBuf::from("/Users/secret-name/Library/Fonts/Gone.ttf");
        let gone = FaceInfo { source: fontdb::Source::File(secret), ..face("Lato-Regular.ttf", "Gone", 400) };
        let fonts = fonts(vec![gone, face("IBMPlexSans-Medium.ttf", "No Regular", 500)]);
        let listed = fonts.list();
        assert_eq!(fonts.load("system:Gone"), Err(vec![DesignError::SystemUnreadable { family: "Gone".into() }]));
        let reasons: Vec<String> = listed.iter().flat_map(|entry| entry.problems()).map(ToString::to_string).collect();
        assert!(reasons.len() >= 2, "{reasons:?}");
        for reason in reasons {
            assert!(!reason.contains('/') && !reason.contains("secret"), "{reason}");
        }
    }
}
