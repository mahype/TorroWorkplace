//! Shared pieces for the Iced apps of the suite (calendar, tasks, later the control
//! centre), so they look and behave like one family when tiled side by side.
//!
//! - [`source::Source`]: where data comes from (the core, or demo data with `--demo`).
//! - [`theme`]: light and dark colour tokens (`theme::palette(&Theme)`), the Iced themes,
//!   spacing, radius and size tokens (`theme::{space, radius, size}`), and the fonts.
//! - [`typo`]: the type scale (`typo::text(typo::BODY, …)`).
//! - [`fonts`] / [`settings`]: the bundled Inter files and the Iced settings that load them.
//! - [`connect`]: the "connect your Nextcloud" screen every app shows without an account.
//! - [`datetime`]: German date and time parsing and formatting, week arithmetic.
//! - [`dialog`]: the frame of a dialog: fixed head and foot, a body that scrolls in low windows.
//! - [`style`]: the style functions for every Iced widget, light and dark.
//! - [`icon`]: the bundled Lucide icons.
//! - [`layout`]: the size classes of tiled windows (`layout::of(size)`).
//! - [`markdown`]: Markdown to read (GitHub texts) in the Torro look.
//! - [`shell`]: the Linear window frame (sidebar, inset panel, header bar, properties column)
//!   and the app switcher; the sidebar expanded, collapsed to a rail of icons, or as a drawer.
//! - [`ui_state`]: what a window remembers between starts (`ui.json`: the sidebar choice).
//! - [`widgets`]: the building blocks inside it (buttons, sidebar and list rows, menus, floating
//!   cards, chips, property rows, settings groups, empty/loading/error states).
//! - [`zoom`]: zoom like a browser (Ctrl + Plus / Minus / 0) for the whole suite, with the short
//!   badge that shows the new level.

pub mod connect;
pub mod datetime;
pub mod dialog;
pub mod icon;
pub mod layout;
pub mod markdown;
pub mod panel;
pub mod shell;
pub mod source;
pub mod style;
pub mod theme;
pub mod typo;
pub mod ui_state;
pub mod widgets;
pub mod zoom;

use std::borrow::Cow;

/// The bundled font files: Inter 4.1 (SIL OFL 1.1, `assets/fonts/OFL.txt`), from
/// <https://github.com/rsms/inter/releases/tag/v4.1> (`Inter-4.1.zip`, SHA-256
/// `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`, `extras/ttf/`).
///
/// Regular, Medium and SemiBold are the static cuts unchanged ([`theme::FONT`],
/// [`theme::FONT_MEDIUM`], [`theme::FONT_SEMIBOLD`]). The two "Inter Tabular" files
/// ([`theme::FONT_TABULAR`], [`theme::FONT_TABULAR_MEDIUM`]) have the `tnum` feature baked in,
/// because Iced 0.14 cannot switch OpenType features. They were made with
/// `opentype-feature-freezer` 1.32.2 (fontTools 4.66.1):
///
/// ```sh
/// pyftfeatfreeze -f tnum -S -U Tabular Inter-Regular.ttf InterTabular-Regular.ttf
/// pyftfeatfreeze -f tnum -S -U Tabular Inter-Medium.ttf InterTabular-Medium.ttf
/// ```
///
/// Apps do not load these themselves; [`settings`] does.
pub fn fonts() -> [&'static [u8]; 5] {
    [
        include_bytes!("../assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        include_bytes!("../assets/fonts/InterTabular-Regular.ttf"),
        include_bytes!("../assets/fonts/InterTabular-Medium.ttf"),
    ]
}

/// Iced settings for every app: [`fonts`] loaded, [`theme::FONT`] (Inter) as default font and
/// [`typo::BODY`] (13) as default text size – Iced's own default is 16 in the system sans-serif.
///
/// Use as `iced::application(…).settings(torro_gui::settings())`. It replaces all settings, so it
/// goes before any builder call that changes settings (`.font`, `.default_font`, `.antialiasing`).
pub fn settings() -> iced::Settings {
    iced::Settings {
        fonts: fonts().into_iter().map(Cow::Borrowed).collect(),
        default_font: theme::FONT,
        default_text_size: typo::BODY.size,
        ..iced::Settings::default()
    }
}

/// Window settings for an app: its fixed app id (Wayland app id / X11 class, which
/// Hyprland rules and workspace templates match on) and a sensible default size.
// Only Linux reads the app id here (Wayland app id); macOS and Windows identify windows otherwise.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn window(app: torro_core::AppId) -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(1100.0, 760.0),
        min_size: Some(iced::Size::new(200.0, 120.0)),
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: app.window_class.to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Font;
    use iced::font::{Family, Weight};

    /// Big-endian `u16` at `at`, or 0 past the end (a broken file then fails the assertions).
    fn u16_at(data: &[u8], at: usize) -> usize {
        data.get(at..at + 2).map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])))
    }

    fn u32_at(data: &[u8], at: usize) -> usize {
        data.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    /// Start of the table `tag` in a TrueType file.
    fn table(data: &[u8], tag: &[u8; 4]) -> Option<usize> {
        (0..u16_at(data, 4))
            .map(|i| 12 + 16 * i)
            .find(|&rec| data.get(rec..rec + 4) == Some(tag))
            .map(|rec| u32_at(data, rec + 8))
    }

    /// The family name the font database (fontdb, used by Iced) sees: the typographic family
    /// (name id 16) if present, else the family (name id 1), from the Windows Unicode records.
    fn family(data: &[u8]) -> Option<String> {
        let name = table(data, b"name")?;
        let (count, strings) = (u16_at(data, name + 2), name + u16_at(data, name + 4));
        let find = |id: usize| {
            (0..count)
                .map(|i| name + 6 + 12 * i)
                .find(|&rec| u16_at(data, rec) == 3 && u16_at(data, rec + 6) == id)
                .and_then(|rec| {
                    let start = strings + u16_at(data, rec + 10);
                    let units: Vec<u16> = data
                        .get(start..start + u16_at(data, rec + 8))?
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&pair| u16::from_be_bytes(pair))
                        .collect();
                    String::from_utf16(&units).ok()
                })
        };
        find(16).or_else(|| find(1))
    }

    fn weight_class(data: &[u8]) -> Option<usize> {
        table(data, b"OS/2").map(|os2| u16_at(data, os2 + 4))
    }

    #[test]
    fn every_font_is_a_truetype_file() {
        for font in fonts() {
            assert!(font.len() > 100_000, "font file too small: {} bytes", font.len());
            assert_eq!(&font[..4], &[0, 1, 0, 0], "not a TrueType file");
        }
    }

    #[test]
    fn theme_fonts_match_the_bundled_files() {
        let expected: [Font; 5] =
            [theme::FONT, theme::FONT_MEDIUM, theme::FONT_SEMIBOLD, theme::FONT_TABULAR, theme::FONT_TABULAR_MEDIUM];
        for (data, font) in fonts().into_iter().zip(expected) {
            let Family::Name(name) = font.family else { panic!("theme font without a family name") };
            assert_eq!(family(data).as_deref(), Some(name));
            let weight = match font.weight {
                Weight::Normal => 400,
                Weight::Medium => 500,
                Weight::Semibold => 600,
                other => panic!("unexpected weight {other:?}"),
            };
            assert_eq!(weight_class(data), Some(weight), "{name}");
        }
    }

    #[test]
    fn settings_load_inter_at_13() {
        let settings = settings();
        assert_eq!(settings.fonts.len(), fonts().len());
        assert_eq!(settings.default_font, theme::FONT);
        assert_eq!(settings.default_text_size, iced::Pixels(13.0));
        assert!(settings.antialiasing);
    }
}
