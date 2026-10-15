//! Shared pieces for the Iced apps of the suite (calendar, tasks, later the control
//! centre), so they look and behave like one family when tiled side by side.
//!
//! - [`source::Source`]: where data comes from (the core, or demo data with `--demo`).
//! - [`design`]: the active design of the process (`design::active()`, `design::set(…)`); every
//!   token below is read from it.
//! - [`theme`]: light and dark colour tokens (`theme::palette(&Theme)`), the Iced themes,
//!   spacing, radius and size tokens (`theme::{space, radius, size}`), and the fonts.
//! - [`typo`]: the type scale (`typo::text(typo::body(), …)`).
//! - [`fonts`] / [`settings`]: the bundled Inter files and the Iced settings that load the fonts
//!   of the active design.
//! - [`connect`]: the "connect your Nextcloud" screen every app shows without an account.
//! - [`datetime`]: German date and time parsing and formatting, week arithmetic.
//! - [`dialog`]: the frame of a dialog: fixed head and foot, a body that scrolls in low windows.
//! - [`split`]: the divider between two panes side by side that the pointer drags (list and reader).
//! - [`style`]: the style functions for every Iced widget, light and dark.
//! - [`icon`]: the bundled Lucide icons.
//! - [`layout`]: the size classes of tiled windows (`layout::of(size)`) and the layout gaps by role
//!   (`layout::inset()`, `layout::page_padding()` …).
//! - [`links`]: web and mail addresses in plain text (`links::pieces`, `links::openable`,
//!   `links::meeting`), to make them clickable.
//! - [`markdown`]: Markdown to read (GitHub texts) in the Torro look.
//! - [`selectable`]: text to read that can be selected and copied but not changed, with the
//!   "Links" of its addresses below it.
//! - [`shell`]: the Linear window frame (sidebar, inset panel, header bar, properties column)
//!   and the app switcher; the sidebar expanded, collapsed to a rail of icons, or as a drawer.
//! - [`ui_state`]: what a window remembers between starts (`ui.json`: the sidebar choice).
//! - [`widgets`]: the building blocks inside it (buttons, sidebar and list rows, menus, floating
//!   cards, chips, property rows, settings groups, empty/loading/error states).
//! - [`zoom`]: zoom like a browser (Ctrl + Plus / Minus / 0) for the whole suite, with the short
//!   badge that shows the new level.

pub mod connect;
pub mod datetime;
pub mod design;
pub mod dialog;
pub mod icon;
pub mod layout;
pub mod links;
pub mod markdown;
pub mod panel;
pub mod selectable;
pub mod shell;
pub mod source;
pub mod split;
pub mod style;
pub mod theme;
pub mod typo;
pub mod ui_state;
pub mod widgets;
pub mod zoom;

/// The bundled font files: Inter 4.1 (SIL OFL 1.1, `assets/fonts/OFL.txt`), from
/// <https://github.com/rsms/inter/releases/tag/v4.1> (`Inter-4.1.zip`, SHA-256
/// `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`, `extras/ttf/`).
///
/// Regular, Medium and SemiBold are the static cuts unchanged ([`theme::font`],
/// [`theme::font_medium`], [`theme::font_semibold`] in the standard design). The two "Inter
/// Tabular" files ([`theme::font_tabular`], [`theme::font_tabular_medium`]) have the `tnum` feature baked in,
/// because Iced 0.14 cannot switch OpenType features. They were made with
/// `opentype-feature-freezer` 1.32.2 (fontTools 4.66.1):
///
/// ```sh
/// pyftfeatfreeze -f tnum -S -U Tabular Inter-Regular.ttf InterTabular-Regular.ttf
/// pyftfeatfreeze -f tnum -S -U Tabular Inter-Medium.ttf InterTabular-Medium.ttf
/// ```
///
/// Apps do not load these themselves; [`settings`] does (through [`design::font_files`], which
/// picks them for the standard design's built-in faces).
pub fn fonts() -> [&'static [u8]; 5] {
    [
        include_bytes!("../assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        include_bytes!("../assets/fonts/InterTabular-Regular.ttf"),
        include_bytes!("../assets/fonts/InterTabular-Medium.ttf"),
    ]
}

/// Iced settings for every app: the fonts of the active design loaded ([`design::font_files`]; the
/// embedded Inter for the standard), its text font as default font ([`theme::font`]) and its
/// [`typo::body`] size (13 in the standard) as default text size – Iced's own default is 16 in the
/// system sans-serif.
///
/// Use as `iced::application(…).settings(torro_gui::settings())`. It replaces all settings, so it
/// goes before any builder call that changes settings (`.font`, `.default_font`, `.antialiasing`).
/// Set the design first ([`design::start`]) if the app starts with another one than the standard.
pub fn settings() -> iced::Settings {
    let active = design::active();
    // A later switch back to this design need not load its fonts again.
    design::mark_fonts_loaded(&active.id);
    iced::Settings {
        fonts: design::font_files(&active),
        default_font: theme::font(),
        default_text_size: typo::body().size,
        ..iced::Settings::default()
    }
}

/// Window settings for an app: its fixed app id (Wayland app id / X11 class, which
/// Hyprland rules and workspace templates match on) and a sensible default size.
///
/// On macOS the window has no title bar of its own: the title is hidden, the bar is transparent and
/// the content runs up to the top edge, so the bar takes the colour of the sidebar. The traffic
/// lights then sit over the content, and [`shell::titlebar`] keeps them clear and drags the window.
// Only Linux reads the app id here (Wayland app id); macOS and Windows identify windows otherwise.
#[cfg_attr(not(target_os = "linux"), allow(unused_mut, unused_variables))]
pub fn window(app: torro_core::AppId) -> iced::window::Settings {
    let mut settings = iced::window::Settings {
        size: iced::Size::new(1100.0, 760.0),
        min_size: Some(iced::Size::new(200.0, 120.0)),
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = app.window_class.to_string();
    }
    #[cfg(target_os = "macos")]
    {
        settings.platform_specific = iced::window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        };
    }
    settings
}

/// The task that drags the window with the mouse, as the native title bar does. Apps return it from
/// their `update` for the message [`shell::titlebar`] sends when its band is pressed.
pub fn drag_window<T: Send + 'static>() -> iced::Task<T> {
    iced::window::latest().and_then(iced::window::drag)
}

/// The task that moves the window's traffic lights onto the line of the title band's buttons, with
/// room around them (macOS, U.11; see `shell::LIGHTS`). Apps run it once at start, beside their first
/// tasks; it keeps the lights there after resizes itself. Other systems have no traffic lights and
/// get a task that does nothing.
pub fn place_traffic_lights<T: Send + 'static>() -> iced::Task<T> {
    #[cfg(target_os = "macos")]
    {
        let at = torro_appkit::Position { x: f64::from(shell::LIGHTS), y: f64::from(shell::LIGHTS) };
        iced::window::latest()
            .and_then(move |id| {
                iced::window::run(id, move |window| {
                    if let Err(err) = torro_appkit::place_traffic_lights(window, at) {
                        eprintln!("cannot move the traffic lights: {err}");
                    }
                })
            })
            .discard()
    }
    #[cfg(not(target_os = "macos"))]
    {
        iced::Task::none()
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
        let expected: [Font; 5] = [
            theme::font(),
            theme::font_medium(),
            theme::font_semibold(),
            theme::font_tabular(),
            theme::font_tabular_medium(),
        ];
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

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_windows_have_no_title_bar_of_their_own() {
        let platform = window(torro_core::AppId::CALENDAR).platform_specific;
        assert!(platform.title_hidden);
        assert!(platform.titlebar_transparent);
        assert!(platform.fullsize_content_view);
    }

    #[test]
    fn settings_load_inter_at_13() {
        let settings = settings();
        assert_eq!(settings.fonts.len(), fonts().len());
        assert_eq!(settings.default_font, theme::font());
        assert_eq!(settings.default_text_size, iced::Pixels(13.0));
        assert!(settings.antialiasing);
    }
}
