//! The design this process draws with (task L.2, concept `docs/design-pakete.md`).
//!
//! Every token of `torro_gui` – the colours of [`crate::theme::palette`], the levels of
//! [`crate::typo`], [`crate::theme::space`], [`crate::theme::radius`], [`crate::theme::size`], the
//! layout gaps of [`crate::layout`] and the fonts – is read from the **active design**, a
//! [`Design`] from `torro_core::design`. There is one per process: [`active`] reads it, [`set`]
//! replaces it. Until the setting exists (task L.3) it is always [`Design::standard`], which gives
//! exactly the values the suite had as constants (the test "same picture" below holds it to that).
//!
//! Views never cache a token across frames: a switch takes effect on the next `view`/`draw`.
//!
//! ```ignore
//! use torro_gui::design;
//! let pack = torro_core::design::load(&dir, "plex")?;
//! let fonts = design::font_files(&pack);   // load them with `iced::font::load` (task L.3)
//! design::set(pack);                       // the next frame draws with the new design
//! ```

use std::borrow::Cow;
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use iced::{Font, Theme};
use torro_core::design::{Face, FontRole, Mode, Weight};

pub use torro_core::design::Design;

use crate::theme::{self, Palette};

#[cfg(test)]
mod legacy;

/// The active design and what `torro_gui` derives from it once per [`set`], so the token functions
/// only copy values out.
pub(crate) struct Tokens {
    /// The design itself.
    pub design: Arc<Design>,
    /// The Iced colours of both modes, with the derived ones (`attention_text`, `error_text`).
    pub dark: Palette,
    pub light: Palette,
    /// The Iced themes built from [`Tokens::dark`] and [`Tokens::light`].
    pub dark_theme: Theme,
    pub light_theme: Theme,
}

impl Tokens {
    fn new(design: Design) -> Tokens {
        let dark = Palette::of(&design, Mode::Dark);
        let light = Palette::of(&design, Mode::Light);
        // The standard keeps the names the apps' tests know ("Torro Dark"); a pack is named after itself.
        let name = |mode: &str| -> Cow<'static, str> {
            if design.is_standard() {
                Cow::Owned(format!("Torro {mode}"))
            } else {
                Cow::Owned(format!("{} {mode}", design.about.name))
            }
        };
        let dark_theme = theme::iced_theme(name("Dark"), &dark);
        let light_theme = theme::iced_theme(name("Light"), &light);
        Tokens { design: Arc::new(design), dark, light, dark_theme, light_theme }
    }
}

static ACTIVE: LazyLock<RwLock<Arc<Tokens>>> = LazyLock::new(|| RwLock::new(Arc::new(Tokens::new(Design::standard()))));

/// Runs `f` on the active tokens. `f` must not call another token function: it runs under the
/// read lock, and a nested read could wait behind a writer.
pub(crate) fn with<R>(f: impl FnOnce(&Tokens) -> R) -> R {
    let tokens = ACTIVE.read().unwrap_or_else(PoisonError::into_inner);
    f(&tokens)
}

/// The design this process draws with. Cheap (an `Arc`); read it again after a switch instead of
/// keeping it.
pub fn active() -> Arc<Design> {
    with(|t| Arc::clone(&t.design))
}

/// Makes `design` the active one for the whole process: every token function returns its values
/// from the next call on. Takes a design that `torro_core::design::load` accepted (merged and
/// checked); nothing is checked here.
///
/// Fonts are not loaded here: at start [`crate::settings`] loads the active design's fonts; on a
/// switch the app loads [`font_files`] with `iced::font::load` before or with this call (task L.3).
/// Canvas caches keep what they drew; apps clear them after a switch.
pub fn set(design: Design) {
    let tokens = Arc::new(Tokens::new(design));
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = tokens;
}

/// The font files of `design` in the form `iced::Settings::fonts` and `iced::font::load` take:
/// each family once, every weight it has. Built-in faces are the Inter files embedded in
/// `torro_gui` ([`crate::fonts`]); a pack's files are read from its folder (a file that cannot be
/// read is left out, Iced then falls back to another font – the pack was checked when it was
/// loaded, so this only happens when the folder changed since).
pub fn font_files(design: &Design) -> Vec<Cow<'static, [u8]>> {
    let mut seen: Vec<&str> = Vec::new();
    let mut files = Vec::new();
    for role in [FontRole::Text, FontRole::Heading, FontRole::Numbers] {
        let font = design.fonts.get(role);
        if seen.contains(&font.family.as_str()) {
            continue;
        }
        seen.push(&font.family);
        for weight in WEIGHTS {
            match font.face(weight) {
                Some(Face::Builtin) => files.extend(builtin(&font.family, weight).map(Cow::Borrowed)),
                Some(Face::File(path)) => files.extend(std::fs::read(path).ok().map(Cow::Owned)),
                None => {}
            }
        }
    }
    files
}

/// The weights a family can have, lightest first.
const WEIGHTS: [Weight; 4] = [Weight::Regular, Weight::Medium, Weight::Semibold, Weight::Bold];

/// The embedded file of a built-in face.
fn builtin(family: &str, weight: Weight) -> Option<&'static [u8]> {
    let [regular, medium, semibold, tabular, tabular_medium] = crate::fonts();
    match (family, weight) {
        ("Inter", Weight::Regular) => Some(regular),
        ("Inter", Weight::Medium) => Some(medium),
        ("Inter", Weight::Semibold) => Some(semibold),
        ("Inter Tabular", Weight::Regular) => Some(tabular),
        ("Inter Tabular", Weight::Medium) => Some(tabular_medium),
        _ => None,
    }
}

/// The Iced font of `role` at `weight` in the active design.
pub fn font(role: FontRole, weight: Weight) -> Font {
    let family = with(|t| family(&t.design.fonts.get(role).family));
    Font { weight: iced_weight(weight), ..Font::with_name(family) }
}

/// The figures font (`numbers`) of the active design at `weight`, or at the nearest lighter weight
/// it has: the standard ships "Inter Tabular" only in Regular and Medium, so a SemiBold level
/// becomes tabular Medium. Regular when it has nothing lighter.
pub fn tabular_font(weight: iced::font::Weight) -> Font {
    let (name, weight) = with(|t| {
        let numbers = &t.design.fonts.numbers;
        let wanted = WEIGHTS.iter().rposition(|w| rank(iced_weight(*w)) <= rank(weight)).unwrap_or(0);
        let found = WEIGHTS[..=wanted].iter().rev().find(|w| numbers.face(**w).is_some()).copied();
        (numbers.family.clone(), found.unwrap_or(Weight::Regular))
    });
    Font { weight: iced_weight(weight), ..Font::with_name(family(&name)) }
}

fn iced_weight(weight: Weight) -> iced::font::Weight {
    match weight {
        Weight::Regular => iced::font::Weight::Normal,
        Weight::Medium => iced::font::Weight::Medium,
        Weight::Semibold => iced::font::Weight::Semibold,
        Weight::Bold => iced::font::Weight::Bold,
    }
}

/// The CSS number of an Iced weight (400 = normal), to compare weights.
fn rank(weight: iced::font::Weight) -> u16 {
    use iced::font::Weight as W;
    match weight {
        W::Thin => 100,
        W::ExtraLight => 200,
        W::Light => 300,
        W::Normal => 400,
        W::Medium => 500,
        W::Semibold => 600,
        W::Bold => 700,
        W::ExtraBold => 800,
        W::Black => 900,
    }
}

/// `name` as the `&'static str` Iced wants in `Font::with_name`. Each family is leaked once and
/// reused afterwards, so memory grows only with the families a process ever draws with.
pub(crate) fn family(name: &str) -> &'static str {
    static FAMILIES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut families = FAMILIES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = families.iter().find(|known| **known == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    families.push(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{radius, size, space};
    use crate::{layout, shell, style, typo};
    use iced::widget::text::LineHeight;

    /// The test "same picture" (L.2): with the standard design, every token is what the constants
    /// were before design packs ([`legacy`]).
    #[test]
    fn the_standard_design_gives_the_old_palettes() {
        assert_eq!(Palette::dark(), legacy::DARK, "dark palette, derived colours included");
        assert_eq!(Palette::light(), legacy::LIGHT, "light palette, derived colours included");
        assert_eq!(theme::theme().palette(), legacy::iced_palette(&legacy::DARK));
        assert_eq!(theme::light().palette(), legacy::iced_palette(&legacy::LIGHT));
        assert_eq!(theme::attention(), legacy::DARK.attention);
    }

    #[test]
    fn the_standard_design_gives_the_old_type_scale() {
        let levels = typo::all();
        assert_eq!(levels.len(), legacy::LEVELS.len());
        for (level, (name, size, line, font)) in levels.into_iter().zip(legacy::LEVELS) {
            assert_eq!(level.size.0, size, "size of {name}");
            assert_eq!(level.line_height, LineHeight::Absolute(iced::Pixels(line)), "line height of {name}");
            assert_eq!(level.font, font, "font of {name}");
        }
        // Tabular cuts: Regular stays Regular, everything heavier is the tabular Medium.
        assert_eq!(typo::meta().tabular().font, legacy::FONT_TABULAR);
        assert_eq!(typo::micro().tabular().font, legacy::FONT_TABULAR);
        assert_eq!(typo::body_strong().tabular().font, legacy::FONT_TABULAR_MEDIUM);
        assert_eq!(typo::display().tabular().font, legacy::FONT_TABULAR_MEDIUM);
        assert_eq!(
            [
                theme::font(),
                theme::font_medium(),
                theme::font_semibold(),
                theme::font_tabular(),
                theme::font_tabular_medium()
            ],
            [
                legacy::FONT,
                legacy::FONT_MEDIUM,
                legacy::FONT_SEMIBOLD,
                legacy::FONT_TABULAR,
                legacy::FONT_TABULAR_MEDIUM
            ]
        );
    }

    #[test]
    fn the_standard_design_gives_the_old_spacing_radii_and_sizes() {
        assert_eq!([space::xs(), space::s(), space::m(), space::l(), space::xl(), space::xxl()], legacy::SPACE);
        assert_eq!([radius::xs(), radius::sm(), radius::md(), radius::lg()], legacy::RADIUS);
        let now = [
            size::control_sm(),
            size::control(),
            size::control_lg(),
            size::row(),
            size::row_two(),
            size::row_sidebar(),
            size::row_group(),
            size::header(),
            size::sidebar(),
            size::props(),
            size::panel(),
            size::list(),
            size::list_medium(),
            size::hour(),
            size::axis(),
            size::axis_narrow(),
            layout::inset(),
            size::content_max(),
            size::form_max(),
            size::HAIRLINE,
            shell::rail(),
        ];
        for (value, (name, old)) in now.into_iter().zip(legacy::SIZES) {
            assert_eq!(value, old, "{name}");
        }
        assert_eq!(style::button_padding(), legacy::BUTTON_PADDING);
        assert_eq!(style::pick_list_padding(), legacy::PICK_LIST_PADDING);
    }

    #[test]
    fn the_standard_design_loads_the_old_fonts() {
        let settings = crate::settings();
        let embedded: Vec<&[u8]> = crate::fonts().to_vec();
        let loaded: Vec<&[u8]> = settings.fonts.iter().map(|f| f.as_ref()).collect();
        assert_eq!(loaded, embedded, "the five Inter files, in the old order");
        assert_eq!(settings.default_font, legacy::FONT);
        assert_eq!(settings.default_text_size, iced::Pixels(13.0));
    }

    #[test]
    fn tokens_follow_the_design_they_are_built_from() {
        let mut design = Design::standard();
        design.colors.dark.accent = torro_core::design::Rgba::hex(0x0EA5A4);
        design.colors.light.text = torro_core::design::Rgba::hex(0x101010);
        let tokens = Tokens::new(design);
        assert_eq!(tokens.dark.accent.into_rgba8(), [0x0E, 0xA5, 0xA4, 0xFF]);
        assert_eq!(tokens.dark_theme.palette().primary, tokens.dark.accent, "the Iced palette follows");
        // Derived colours are computed from the design, not taken from the old constants.
        assert_ne!(tokens.light.attention_text, legacy::LIGHT.attention_text);
        assert_eq!(tokens.dark_theme.to_string(), "Torro Dark");
    }

    #[test]
    fn set_replaces_the_active_design() {
        // Same values as the standard under another id: tests running beside this one keep reading
        // the standard tokens.
        let mut twin = Design::standard();
        twin.id = "twin".to_string();
        set(twin);
        assert_eq!(active().id, "twin");
        assert_eq!(space::m(), 12.0);
        set(Design::standard());
        assert!(active().is_standard());
    }

    #[test]
    fn families_are_leaked_once() {
        let a = family("Some Family");
        let b = family(&String::from("Some Family"));
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn tabular_falls_back_to_a_lighter_weight() {
        use iced::font::Weight as W;
        assert_eq!(tabular_font(W::Semibold).weight, W::Medium);
        assert_eq!(tabular_font(W::Bold).weight, W::Medium);
        assert_eq!(tabular_font(W::Normal).weight, W::Normal);
        assert_eq!(tabular_font(W::Thin).weight, W::Normal);
    }
}
