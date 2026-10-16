//! Design packs: what the suite looks like, as data (concept `docs/design-pakete.md`, task L.1).
//!
//! A [`Design`] holds every value the look is built from: fonts, the type scale, spacing, radii,
//! layout gaps, sizes, the colours of both modes and the logo. [`Design::standard`] is the look
//! the suite ships ("Torro", after Linear) – exactly the values `torro_gui::theme`, `torro_gui::typo`
//! and [`crate::theme`] had as constants; `torro_gui` reads them from here (task L.2).
//!
//! Further designs are packs: a folder `<data>/designs/<id>/` with a `design.toml` and optional
//! SVG, PNG and font files ([`dir`]). [`load`] reads one, fills everything it leaves out from the
//! standard and checks it against the rules of the concept; [`list`] does that for every pack.
//! A pack that breaks a rule is refused as a whole, with every reason ([`DesignError`]).
//!
//! Nothing here runs anything from a pack: only `design.toml`, SVG, PNG and TTF/OTF files are
//! read, each only after its size was checked, and paths never leave the pack's folder.
//!
//! No Iced in here: colours are [`Rgba`] with `f32` channels in 0–1, built the way Iced builds a
//! `Color` from 8-bit values, so `torro_gui` converts them 1:1 (`Color { r, g, b, a }`).

mod error;
mod files;
mod pack;
mod rules;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::AppId;
use crate::i18n::tr;
use crate::paths::AppPaths;
use crate::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_INACTIVE, STATUS_OK, TORRO_RED};

pub use error::DesignError;
pub use pack::{Listed, list, load};

/// The id of the standard design. It has no folder; [`load`] answers it without touching the disk,
/// and a folder of that name under [`dir`] is refused ([`DesignError::BadId`]).
pub const STANDARD_ID: &str = "torro";

/// The folder that holds the design packs: `designs/` in the core's data directory
/// (`TORRO_DATA_HOME` overrides the root). `None` when no home directory is known.
///
/// Callers pass it to [`list`] and [`load`]; tests pass a temporary folder instead.
pub fn dir() -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.data.join("designs"))
}

/// One look of the whole suite: the standard or a pack, already merged with the standard and
/// checked. Every field is set; there is nothing optional left to fill in.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    /// [`STANDARD_ID`] or the pack's folder name.
    pub id: String,
    /// Name, version and author, for the list in the control centre.
    pub about: About,
    /// The (up to) three font families.
    pub fonts: Fonts,
    /// The nine text levels (`torro_gui::typo`).
    pub typo: TypeScale,
    /// The spacing scale (`torro_gui::theme::space`).
    pub space: Space,
    /// The radius scale (`torro_gui::theme::radius`).
    pub radius: Radius,
    /// Margins, paddings and gaps by role (`torro_gui::layout`, task L.2).
    pub layout: Layout,
    /// Fixed sizes of controls, rows and regions (`torro_gui::theme::size`).
    pub size: Size,
    /// The colours of both modes.
    pub colors: Colors,
    /// Own logo and tray symbol, if the pack brings them.
    pub logo: Logo,
    /// `preview.png` in the pack (at most 1200 × 800), for the control centre; `None` shows a
    /// swatch built from the colours.
    pub preview: Option<PathBuf>,
}

/// What a design says about itself (`[about]`). Pack data: shown as written, never translated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct About {
    /// Required in a pack.
    pub name: String,
    /// Required in a pack.
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    /// SPDX id of the fonts' licence ("OFL-1.1"); required as soon as a pack brings fonts.
    pub font_license: Option<String>,
}

/// The three font roles a text level can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fonts {
    /// Running text, lists, buttons. Also the apps' default font.
    pub text: Font,
    /// Headings (`title`, `headline` in the standard). A pack without one uses `text`.
    pub heading: Font,
    /// Tabular figures for digits that change (clocks, axes, counters). A pack without one uses
    /// `text`, so its figures are proportional.
    pub numbers: Font,
}

impl Fonts {
    /// The font of `role`.
    pub fn get(&self, role: FontRole) -> &Font {
        match role {
            FontRole::Text => &self.text,
            FontRole::Heading => &self.heading,
            FontRole::Numbers => &self.numbers,
        }
    }
}

/// One font family and the weights it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    /// The family name as the files carry it; Iced selects the font by it.
    pub family: String,
    /// Weight 400. Always there for `text`; `heading` and `numbers` may bring only the weights their
    /// levels use.
    pub regular: Option<Face>,
    /// Weight 500.
    pub medium: Option<Face>,
    /// Weight 600.
    pub semibold: Option<Face>,
    /// Weight 700.
    pub bold: Option<Face>,
}

impl Font {
    /// The face for `weight`, or `None` when the family has no file for it.
    pub fn face(&self, weight: Weight) -> Option<&Face> {
        match weight {
            Weight::Regular => self.regular.as_ref(),
            Weight::Medium => self.medium.as_ref(),
            Weight::Semibold => self.semibold.as_ref(),
            Weight::Bold => self.bold.as_ref(),
        }
    }
}

/// Where the bytes of one weight come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Face {
    /// Embedded in `torro_gui` (Inter and "Inter Tabular"); `torro_gui` picks the file by family
    /// and weight. Only the standard design uses it.
    Builtin,
    /// A TTF/OTF file inside the pack, absolute, checked to carry [`Font::family`].
    File(PathBuf),
}

/// Which of the [`Fonts`] a level uses (`font = "heading"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontRole {
    Text,
    Heading,
    Numbers,
}

impl fmt::Display for FontRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FontRole::Text => "text",
            FontRole::Heading => "heading",
            FontRole::Numbers => "numbers",
        })
    }
}

/// A font weight (`weight = "medium"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    /// 400.
    Regular,
    /// Weight 500.
    Medium,
    /// Weight 600.
    Semibold,
    /// Weight 700.
    Bold,
}

impl fmt::Display for Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Weight::Regular => "regular",
            Weight::Medium => "medium",
            Weight::Semibold => "semibold",
            Weight::Bold => "bold",
        })
    }
}

/// One text level: font role, weight, size and absolute line height (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Level {
    pub font: FontRole,
    pub weight: Weight,
    pub size: f32,
    pub line: f32,
}

/// The nine text levels, named as in `torro_gui::typo` (lowercase).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    /// Time axis, times in events, weekday letters. Uses `numbers` in the standard.
    pub micro: Level,
    /// Group titles, buttons, chips, badges.
    pub caption: Level,
    /// Metadata, counters, due dates.
    pub meta: Level,
    /// The default: lists, sidebar, inputs, running text; the apps' default text size.
    pub body: Level,
    /// Header and row titles, event titles.
    pub body_strong: Level,
    /// Reading text: the whole of a mail in the inbox's reader (task E8.2), a little larger and with
    /// more line height than [`TypeScale::body`], because it is read for longer than a list.
    pub read: Level,
    /// Card and dialog titles.
    pub title: Level,
    /// Detail titles, view titles without a header bar.
    pub headline: Level,
    /// The big number in a mini tile.
    pub display: Level,
    /// The title of a reminder over the whole screen (task ER1.5), read from across the room.
    pub hero: Level,
    /// The countdown of a reminder over the whole screen ("Starts in 4:32"); uses `numbers`.
    pub countdown: Level,
}

impl TypeScale {
    /// Every level with its key (`"body"`), smallest first.
    pub fn levels(&self) -> [(&'static str, Level); 11] {
        [
            ("micro", self.micro),
            ("caption", self.caption),
            ("meta", self.meta),
            ("body", self.body),
            ("body_strong", self.body_strong),
            ("read", self.read),
            ("title", self.title),
            ("headline", self.headline),
            ("display", self.display),
            ("hero", self.hero),
            ("countdown", self.countdown),
        ]
    }
}

impl TypeScale {
    /// The level named `key` (`"body"`), for reading packs.
    pub(crate) fn slot_mut(&mut self, key: &str) -> Option<&mut Level> {
        Some(match key {
            "micro" => &mut self.micro,
            "caption" => &mut self.caption,
            "meta" => &mut self.meta,
            "body" => &mut self.body,
            "body_strong" => &mut self.body_strong,
            "read" => &mut self.read,
            "title" => &mut self.title,
            "headline" => &mut self.headline,
            "display" => &mut self.display,
            "hero" => &mut self.hero,
            "countdown" => &mut self.countdown,
            _ => return None,
        })
    }
}

/// The spacing scale (`space = [xs, s, m, l, xl, xxl]`), ascending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Space {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
    pub xxl: f32,
}

/// The radius scale (`radius = [xs, sm, md, lg]`), ascending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radius {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

/// Padding as `[vertical, horizontal]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pad {
    pub vertical: f32,
    pub horizontal: f32,
}

/// Margins, paddings and gaps by role (`[layout]`), the same for every element of that role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The margin around the inset content panel beside the sidebar (top, right, bottom).
    /// Standard 8: `torro_gui::theme::size::INSET`, used in `torro_gui::shell`.
    pub inset: f32,
    /// Padding of content and pages. Standard 24 × 24: the control centre's pages in the wide and
    /// medium layout (`page_margin` in `torroworkplace/src/ui/mod.rs`).
    pub page_padding: Pad,
    /// Padding of panels, cards and dialogs. Standard 16 × 16: the floating card
    /// (`torro_gui::widgets::popover`, 16 above and below, 8 + 8 at the sides).
    pub panel_padding: Pad,
    /// Padding of list rows. Standard 0 × 16: `torro_gui::widgets::list_row` and `group_row` (the
    /// height comes from [`Size::row`], the content is centred in it).
    pub row_padding: Pad,
    /// Padding of buttons, chips and inputs. Standard 0 × 10: `torro_gui::style::BUTTON_PADDING`.
    pub control_padding: Pad,
    /// Between groups and sections. Standard 24: the groups of the control centre's General page
    /// (`torroworkplace/src/ui/general.rs`).
    pub section_gap: f32,
    /// Between elements inside a group. Standard 8: `torro_gui::widgets::group`.
    pub item_gap: f32,
}

/// Fixed sizes (`torro_gui::theme::size`, lowercase). `INSET` moved to [`Layout::inset`];
/// `HAIRLINE` stays a constant in `torro_gui` (always one logical pixel, not a design choice).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub control_sm: f32,
    pub control: f32,
    pub control_lg: f32,
    /// The buttons of a reminder over the whole screen (task ER1.5): Join, Snooze, Dismiss; and the
    /// strip of actions under the chosen mail in the mail window's list (task E11.1).
    pub control_xl: f32,
    pub row: f32,
    pub row_two: f32,
    /// A mail entry's row in the inbox (task E3.9): avatar, sender, subject and preview on three
    /// lines of 20 with 16 above and below (task E11.1).
    pub row_message: f32,
    /// Any other entry's row in the inbox (task E3.9): source icon, title and text on two lines.
    pub row_notice: f32,
    pub row_sidebar: f32,
    pub row_group: f32,
    pub header: f32,
    pub sidebar: f32,
    pub props: f32,
    pub panel: f32,
    pub list: f32,
    pub list_medium: f32,
    pub hour: f32,
    pub axis: f32,
    pub axis_narrow: f32,
    pub content_max: f32,
    pub form_max: f32,
    /// The gap between two cards and between the parts of one card (task E8.2,
    /// `torro_gui::widgets::card`): the comfortable density of the inbox.
    pub card_gap: f32,
    /// The avatar of a card (task E8.2): a sender's initials beside a mail card.
    pub avatar: f32,
    /// The large avatar (task E8.2): the sender at the head of the mail reader.
    pub avatar_lg: f32,
    /// The small source tile (task E8.2, `torro_gui::widgets::source_tile`): beside a word, in a
    /// compact row or a menu.
    pub tile_sm: f32,
    /// The source tile of a notice row and of a column head (task E8.2).
    pub tile: f32,
    /// The large source tile (task E8.2): an attachment, the head of a card that stands for an app.
    pub tile_lg: f32,
    /// The command bar at the top of the mail reader (task E11.1): Reply, Forward, Archive … as
    /// cells side by side over the reader's whole width.
    pub command: f32,
    /// A Torro pill in the mail reader (task E11.1: Task, Event, Mute…) and the sender's avatar
    /// beside them in the reader's head.
    pub pill: f32,
}

impl Size {
    /// Every size with its key (`"row"`), for checks and error messages.
    pub fn entries(&self) -> [(&'static str, f32); 29] {
        [
            ("control_sm", self.control_sm),
            ("control", self.control),
            ("control_lg", self.control_lg),
            ("control_xl", self.control_xl),
            ("row", self.row),
            ("row_two", self.row_two),
            ("row_message", self.row_message),
            ("row_notice", self.row_notice),
            ("row_sidebar", self.row_sidebar),
            ("row_group", self.row_group),
            ("header", self.header),
            ("sidebar", self.sidebar),
            ("props", self.props),
            ("panel", self.panel),
            ("list", self.list),
            ("list_medium", self.list_medium),
            ("hour", self.hour),
            ("axis", self.axis),
            ("axis_narrow", self.axis_narrow),
            ("content_max", self.content_max),
            ("form_max", self.form_max),
            ("card_gap", self.card_gap),
            ("avatar", self.avatar),
            ("avatar_lg", self.avatar_lg),
            ("tile_sm", self.tile_sm),
            ("tile", self.tile),
            ("tile_lg", self.tile_lg),
            ("command", self.command),
            ("pill", self.pill),
        ]
    }

    pub(crate) fn slot_mut(&mut self, key: &str) -> Option<&mut f32> {
        Some(match key {
            "control_sm" => &mut self.control_sm,
            "control" => &mut self.control,
            "control_lg" => &mut self.control_lg,
            "control_xl" => &mut self.control_xl,
            "row" => &mut self.row,
            "row_two" => &mut self.row_two,
            "row_message" => &mut self.row_message,
            "row_notice" => &mut self.row_notice,
            "row_sidebar" => &mut self.row_sidebar,
            "row_group" => &mut self.row_group,
            "header" => &mut self.header,
            "sidebar" => &mut self.sidebar,
            "props" => &mut self.props,
            "panel" => &mut self.panel,
            "list" => &mut self.list,
            "list_medium" => &mut self.list_medium,
            "hour" => &mut self.hour,
            "axis" => &mut self.axis,
            "axis_narrow" => &mut self.axis_narrow,
            "content_max" => &mut self.content_max,
            "form_max" => &mut self.form_max,
            "card_gap" => &mut self.card_gap,
            "avatar" => &mut self.avatar,
            "avatar_lg" => &mut self.avatar_lg,
            "tile_sm" => &mut self.tile_sm,
            "tile" => &mut self.tile,
            "tile_lg" => &mut self.tile_lg,
            "command" => &mut self.command,
            "pill" => &mut self.pill,
            _ => return None,
        })
    }
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        })
    }
}

/// The colours of both modes (`[colors.dark]`, `[colors.light]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub dark: Palette,
    pub light: Palette,
}

impl Colors {
    /// The palette of `mode`.
    pub fn get(&self, mode: Mode) -> &Palette {
        match mode {
            Mode::Dark => &self.dark,
            Mode::Light => &self.light,
        }
    }
}

/// The colour tokens of one mode: every field of `torro_gui::theme::Palette` that is not derived
/// (`attention_text` and `error_text` are, see [`Palette::attention_text`]). Field names and
/// meanings are the same as there; the TOML keys are the field names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Rgba,
    pub panel: Rgba,
    pub raised: Rgba,
    pub hover: Rgba,
    pub selected: Rgba,
    pub menu: Rgba,
    pub text: Rgba,
    pub text_2: Rgba,
    pub text_3: Rgba,
    pub line: Rgba,
    pub line_strong: Rgba,
    pub line_selected: Rgba,
    pub focus_ring: Rgba,
    pub accent: Rgba,
    pub on_accent: Rgba,
    pub ok: Rgba,
    pub attention: Rgba,
    pub error: Rgba,
    pub inactive: Rgba,
    pub merged: Rgba,
    /// Default calendar and list colours: work, personal, family, birthday, groceries.
    pub calendar: [Rgba; 5],
    /// Opacity of a calendar colour used as a surface, 0–1.
    pub tint: f32,
    /// The colours of chart series (task E8.8b, "Insights"), in their fixed order: blue, orange,
    /// green, magenta. Picked and checked as a set for colour blindness in this order (adjacent
    /// pairs, the dataviz validator), so a chart gives them to its series in this order and never
    /// cycles; a fifth series goes to [`Palette::chart_other`]. Never the accent or a status colour.
    pub chart: [Rgba; 4],
    /// The grey of the series "Other" in a chart: what is not one of the [`Palette::chart`] series.
    pub chart_other: Rgba,
    /// The colour of the shadow under menus, cards and dialogs; offset (0, 12) and blur 32 stay
    /// fixed in `torro_gui` (geometry, not colour).
    pub shadow: Rgba,
    pub scrim: Rgba,
    pub night: Rgba,
    /// The frame of the window: the sidebar, the rail, the drawer, the margin around the content
    /// panel and the title band over them. A pack that leaves it out gets its own `bg`.
    pub sidebar: Rgba,
    /// A sidebar row under the pointer (pack without it: its `hover`).
    pub sidebar_hover: Rgba,
    /// The chosen sidebar row (pack without it: its `selected`).
    pub sidebar_selected: Rgba,
    /// Text in the sidebar: the chosen row, titles (pack without it: its `text`).
    pub sidebar_text: Rgba,
    /// The rows of the sidebar (pack without it: its `text_2`).
    pub sidebar_text_2: Rgba,
    /// Group titles, counters, days outside the month (pack without it: its `text_3`).
    pub sidebar_text_3: Rgba,
    /// Lines in and at the sidebar (pack without it: its `line`).
    pub sidebar_line: Rgba,
}

/// The sidebar keys of a palette with the key each falls back to when a pack leaves it out.
pub(crate) const SIDEBAR_FALLBACK: [(&str, &str); 7] = [
    ("sidebar", "bg"),
    ("sidebar_hover", "hover"),
    ("sidebar_selected", "selected"),
    ("sidebar_text", "text"),
    ("sidebar_text_2", "text_2"),
    ("sidebar_text_3", "text_3"),
    ("sidebar_line", "line"),
];

impl Palette {
    /// Whether the sidebar has a surface of its own (`sidebar` is not `bg`), as Slack's aubergine
    /// beside light content. Only then do raised surfaces and frames in it change too, see
    /// [`Palette::in_sidebar`].
    pub fn own_sidebar(&self) -> bool {
        self.sidebar != self.bg
    }

    /// This palette as the sidebar draws with it: `bg`, `hover`, `selected`, the three text colours
    /// and `line` are the `sidebar…` ones. With [`Palette::own_sidebar`] also `panel` (the
    /// sidebar), `raised` (`sidebar_hover`) and `line_strong` (`sidebar_line`), because surfaces and
    /// frames made for the light content would not carry the sidebar's text. Everything else
    /// (accent, status, calendar colours, focus) stays; the rules hold it to the sidebar too.
    ///
    /// For a pack without sidebar keys (the standard among them) it is the palette itself.
    pub fn in_sidebar(&self) -> Palette {
        let own = self.own_sidebar();
        Palette {
            bg: self.sidebar,
            panel: if own { self.sidebar } else { self.panel },
            raised: if own { self.sidebar_hover } else { self.raised },
            hover: self.sidebar_hover,
            selected: self.sidebar_selected,
            text: self.sidebar_text,
            text_2: self.sidebar_text_2,
            text_3: self.sidebar_text_3,
            line: self.sidebar_line,
            line_strong: if own { self.sidebar_line } else { self.line_strong },
            ..*self
        }
    }

    /// The opaque surfaces text sits on, with their keys.
    pub fn surfaces(&self) -> [(&'static str, Rgba); 6] {
        [
            ("bg", self.bg),
            ("panel", self.panel),
            ("raised", self.raised),
            ("hover", self.hover),
            ("selected", self.selected),
            ("menu", self.menu),
        ]
    }

    /// Attention colour for text and small icons: dark mode the status colour itself, light mode
    /// moved 50 % towards `text` (as `torro_gui::theme::Palette::attention_text` today).
    pub fn attention_text(&self, mode: Mode) -> Rgba {
        match mode {
            Mode::Dark => self.attention,
            Mode::Light => Rgba::mix(self.attention, self.text, 0.5),
        }
    }

    /// Error colour for text: dark mode the status colour itself, light mode moved 30 % towards
    /// `text` (as `torro_gui::theme::Palette::error_text` today).
    pub fn error_text(&self, mode: Mode) -> Rgba {
        match mode {
            Mode::Dark => self.error,
            Mode::Light => Rgba::mix(self.error, self.text, 0.3),
        }
    }

    /// The colour field named `key` (not `calendar`, `chart`, `tint`), for reading packs.
    pub(crate) fn slot_mut(&mut self, key: &str) -> Option<&mut Rgba> {
        Some(match key {
            "bg" => &mut self.bg,
            "panel" => &mut self.panel,
            "raised" => &mut self.raised,
            "hover" => &mut self.hover,
            "selected" => &mut self.selected,
            "menu" => &mut self.menu,
            "text" => &mut self.text,
            "text_2" => &mut self.text_2,
            "text_3" => &mut self.text_3,
            "line" => &mut self.line,
            "line_strong" => &mut self.line_strong,
            "line_selected" => &mut self.line_selected,
            "focus_ring" => &mut self.focus_ring,
            "accent" => &mut self.accent,
            "on_accent" => &mut self.on_accent,
            "ok" => &mut self.ok,
            "attention" => &mut self.attention,
            "error" => &mut self.error,
            "inactive" => &mut self.inactive,
            "merged" => &mut self.merged,
            "chart_other" => &mut self.chart_other,
            "shadow" => &mut self.shadow,
            "scrim" => &mut self.scrim,
            "night" => &mut self.night,
            "sidebar" => &mut self.sidebar,
            "sidebar_hover" => &mut self.sidebar_hover,
            "sidebar_selected" => &mut self.sidebar_selected,
            "sidebar_text" => &mut self.sidebar_text,
            "sidebar_text_2" => &mut self.sidebar_text_2,
            "sidebar_text_3" => &mut self.sidebar_text_3,
            "sidebar_line" => &mut self.sidebar_line,
            _ => return None,
        })
    }
}

/// Own logo files of a pack, absolute; `None` draws the suite's own in the accent colour.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Logo {
    /// The mark in the app switcher and the rail (SVG). `None`: the "T" on `accent`.
    pub mark: Option<PathBuf>,
    /// A tray symbol (SVG), still checked so older packs stay valid, but no longer shown: the tray
    /// always draws the four red squares, whatever the design (issue #114).
    pub tray: Option<PathBuf>,
}

/// An sRGB colour with alpha, every channel 0–1 (the layout of Iced's `Color`).
///
/// Written in packs as `#RRGGBB` or `#RRGGBBAA` ([`Rgba::parse`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    /// An opaque colour from 8-bit channels (as Iced's `Color::from_rgb8`).
    pub const fn rgb8(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
    }

    /// An opaque colour from `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Rgba {
        Rgba::rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }

    /// A `torro_core::theme` colour.
    pub const fn from_rgb(rgb: Rgb) -> Rgba {
        Rgba::rgb8(rgb.0, rgb.1, rgb.2)
    }

    /// Translucent white (lines and focus in dark mode).
    pub const fn white(alpha: f32) -> Rgba {
        Rgba { r: 1.0, g: 1.0, b: 1.0, a: alpha }
    }

    /// Translucent black (lines and focus in light mode, shadows, scrims).
    pub const fn black(alpha: f32) -> Rgba {
        Rgba { r: 0.0, g: 0.0, b: 0.0, a: alpha }
    }

    /// `#RRGGBB` or `#RRGGBBAA` (either case); `None` for anything else.
    pub fn parse(text: &str) -> Option<Rgba> {
        let digits = text.strip_prefix('#')?;
        if !(digits.len() == 6 || digits.len() == 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        let colour = Rgba::rgb8(byte(0)?, byte(2)?, byte(4)?);
        let a = if digits.len() == 8 { f32::from(byte(6)?) / 255.0 } else { 1.0 };
        Some(Rgba { a, ..colour })
    }

    /// `a` moved towards `b` by `t` in sRGB; opaque (as `torro_gui::theme::mix`).
    pub const fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
        Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1.0 }
    }

    /// `self` drawn over the opaque `under`, as one opaque colour (as `torro_gui::theme::over`).
    pub const fn over(self, under: Rgba) -> Rgba {
        Rgba::mix(under, self, self.a)
    }

    /// WCAG relative luminance (alpha ignored), the formula of Iced's `Color::relative_luminance`.
    pub fn luminance(self) -> f32 {
        let linear = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio of `self` drawn on `surface` (1–21). A translucent `self` is laid over
    /// the surface first, as it is drawn; the surface counts as opaque.
    pub fn contrast(self, surface: Rgba) -> f32 {
        let (a, b) = (self.over(surface).luminance(), surface.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

impl Design {
    /// The standard design "Torro": exactly the values the suite had as constants before design
    /// packs (`torro_gui::theme`, `torro_gui::typo`, [`crate::theme`]), with the embedded Inter
    /// and the suite's own logo and tray symbol.
    pub fn standard() -> Design {
        let inter = |family: &str, semibold: bool| Font {
            family: family.to_string(),
            regular: Some(Face::Builtin),
            medium: Some(Face::Builtin),
            semibold: semibold.then_some(Face::Builtin),
            bold: None,
        };
        let level = |font, weight, size, line| Level { font, weight, size, line };
        Design {
            id: STANDARD_ID.to_string(),
            about: About {
                name: "Torro".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: Some("Torro".to_string()),
                description: Some(tr("Inter, quiet surfaces, Torro red").to_string()),
                font_license: Some("OFL-1.1".to_string()),
            },
            fonts: Fonts {
                text: inter("Inter", true),
                heading: inter("Inter", true),
                numbers: inter("Inter Tabular", false),
            },
            typo: TypeScale {
                micro: level(FontRole::Numbers, Weight::Regular, 11.0, 14.0),
                caption: level(FontRole::Text, Weight::Medium, 12.0, 16.0),
                meta: level(FontRole::Text, Weight::Regular, 12.0, 16.0),
                body: level(FontRole::Text, Weight::Regular, 13.0, 20.0),
                body_strong: level(FontRole::Text, Weight::Medium, 13.0, 20.0),
                read: level(FontRole::Text, Weight::Regular, 14.0, 22.0),
                title: level(FontRole::Heading, Weight::Semibold, 16.0, 22.0),
                headline: level(FontRole::Heading, Weight::Semibold, 22.0, 28.0),
                display: level(FontRole::Text, Weight::Semibold, 28.0, 32.0),
                hero: level(FontRole::Heading, Weight::Semibold, 48.0, 56.0),
                countdown: level(FontRole::Numbers, Weight::Medium, 28.0, 32.0),
            },
            space: Space { xs: 4.0, s: 8.0, m: 12.0, l: 16.0, xl: 24.0, xxl: 32.0 },
            radius: Radius { xs: 4.0, sm: 6.0, md: 8.0, lg: 10.0 },
            layout: Layout {
                inset: 8.0,
                page_padding: Pad { vertical: 24.0, horizontal: 24.0 },
                panel_padding: Pad { vertical: 16.0, horizontal: 16.0 },
                row_padding: Pad { vertical: 0.0, horizontal: 16.0 },
                control_padding: Pad { vertical: 0.0, horizontal: 10.0 },
                section_gap: 24.0,
                item_gap: 8.0,
            },
            size: STANDARD_SIZE,
            colors: Colors { dark: STANDARD_DARK, light: STANDARD_LIGHT },
            logo: Logo::default(),
            preview: None,
        }
    }

    /// Whether this is the standard design.
    pub fn is_standard(&self) -> bool {
        self.id == STANDARD_ID
    }
}

/// The sizes of the standard design (also the list of valid `[size]` keys).
pub(crate) const STANDARD_SIZE: Size = Size {
    control_sm: 28.0,
    control: 32.0,
    control_lg: 36.0,
    control_xl: 44.0,
    row: 40.0,
    row_two: 52.0,
    row_message: 92.0,
    row_notice: 64.0,
    row_sidebar: 28.0,
    row_group: 32.0,
    header: 44.0,
    sidebar: 232.0,
    props: 300.0,
    panel: 400.0,
    list: 460.0,
    list_medium: 380.0,
    hour: 48.0,
    axis: 52.0,
    axis_narrow: 44.0,
    content_max: 720.0,
    form_max: 640.0,
    card_gap: 12.0,
    avatar: 36.0,
    avatar_lg: 40.0,
    tile_sm: 20.0,
    tile: 28.0,
    tile_lg: 36.0,
    command: 46.0,
    pill: 42.0,
};

const STANDARD_DARK: Palette = Palette {
    bg: Rgba::hex(0x08090A),
    panel: Rgba::hex(0x0F1011),
    raised: Rgba::hex(0x141518),
    hover: Rgba::hex(0x16171A),
    selected: Rgba::hex(0x1C1E22),
    menu: Rgba::hex(0x17181B),
    text: Rgba::hex(0xEDEEF0),
    text_2: Rgba::hex(0xB4B8BF),
    text_3: Rgba::hex(0x8A8F98),
    line: Rgba::white(0.06),
    line_strong: Rgba::white(0.10),
    line_selected: Rgba::white(0.28),
    focus_ring: Rgba::white(0.35),
    accent: Rgba::from_rgb(TORRO_RED),
    on_accent: Rgba::hex(0xFFFFFF),
    ok: Rgba::from_rgb(STATUS_OK),
    attention: Rgba::from_rgb(STATUS_ATTENTION),
    error: Rgba::from_rgb(STATUS_ERROR),
    inactive: Rgba::from_rgb(STATUS_INACTIVE),
    merged: Rgba::hex(0xA78BFA),
    calendar: [Rgba::hex(0x5B8CFF), Rgba::hex(0x3FB984), Rgba::hex(0xE8A33D), Rgba::hex(0xD96CA8), Rgba::hex(0xA78BFA)],
    tint: 0.18,
    // Checked with the dataviz validator against `panel` (task E8.8b): lightness band, chroma, CVD
    // ΔE ≥ 8 for every adjacent pair, ≥ 3:1. "Other" is ΔE ≥ 8 from each of the four.
    chart: [Rgba::hex(0x3B78D8), Rgba::hex(0xC9701E), Rgba::hex(0x2E9E6B), Rgba::hex(0xB5509A)],
    chart_other: Rgba::hex(0x5C6068),
    shadow: Rgba::black(0.5),
    scrim: Rgba::black(0.62),
    night: Rgba::hex(0x121626),
    sidebar: Rgba::hex(0x08090A),
    sidebar_hover: Rgba::hex(0x16171A),
    sidebar_selected: Rgba::hex(0x1C1E22),
    sidebar_text: Rgba::hex(0xEDEEF0),
    sidebar_text_2: Rgba::hex(0xB4B8BF),
    sidebar_text_3: Rgba::hex(0x8A8F98),
    sidebar_line: Rgba::white(0.06),
};

const STANDARD_LIGHT: Palette = Palette {
    bg: Rgba::hex(0xF4F5F6),
    panel: Rgba::hex(0xFFFFFF),
    raised: Rgba::hex(0xF8F9FA),
    hover: Rgba::hex(0xF0F1F3),
    selected: Rgba::hex(0xE9EAED),
    menu: Rgba::hex(0xFFFFFF),
    text: Rgba::hex(0x18191B),
    text_2: Rgba::hex(0x4F535A),
    text_3: Rgba::hex(0x62666D),
    line: Rgba::black(0.07),
    line_strong: Rgba::black(0.11),
    line_selected: Rgba::black(0.32),
    focus_ring: Rgba::black(0.5),
    accent: Rgba::from_rgb(TORRO_RED),
    on_accent: Rgba::hex(0xFFFFFF),
    ok: Rgba::from_rgb(STATUS_OK),
    attention: Rgba::from_rgb(STATUS_ATTENTION),
    error: Rgba::from_rgb(STATUS_ERROR),
    inactive: Rgba::from_rgb(STATUS_INACTIVE),
    merged: Rgba::hex(0x7C5CE6),
    calendar: [Rgba::hex(0x3B6EF0), Rgba::hex(0x1F9D68), Rgba::hex(0xB06F12), Rgba::hex(0xC2508F), Rgba::hex(0x7C5CE6)],
    tint: 0.14,
    // The same hues stepped for the light panel, checked the same way (task E8.8b).
    chart: [Rgba::hex(0x2F6BC9), Rgba::hex(0xB8621A), Rgba::hex(0x2A9466), Rgba::hex(0xA8448C)],
    chart_other: Rgba::hex(0x50545B),
    shadow: Rgba::black(0.12),
    scrim: Rgba::black(0.3),
    night: Rgba::hex(0xE3E7F1),
    sidebar: Rgba::hex(0xF4F5F6),
    sidebar_hover: Rgba::hex(0xF0F1F3),
    sidebar_selected: Rgba::hex(0xE9EAED),
    sidebar_text: Rgba::hex(0x18191B),
    sidebar_text_2: Rgba::hex(0x4F535A),
    sidebar_text_3: Rgba::hex(0x62666D),
    sidebar_line: Rgba::black(0.07),
};

/// Whether `id` may name a pack folder: `a-z`, `0-9`, `-`, 1–32 characters, not [`STANDARD_ID`].
pub fn is_valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id != STANDARD_ID
}

/// `path` relative to `root` for messages ("fonts/Inter.ttf"); the file name when it is outside.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .or_else(|| path.file_name().map(Path::new))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_in_both_spellings() {
        assert_eq!(Rgba::parse("#D50C0C"), Some(Rgba::rgb8(0xD5, 0x0C, 0x0C)));
        assert_eq!(Rgba::parse("#d50c0c"), Some(Rgba::rgb8(0xD5, 0x0C, 0x0C)));
        let translucent = Rgba::parse("#FFFFFF80").expect("8 digits");
        assert_eq!(translucent.a, 128.0 / 255.0);
        for bad in ["D50C0C", "#D50C0", "#D50C0C0", "#GG0000", "#D50C0C80FF", "", "#ÄÄÄÄÄÄ"] {
            assert_eq!(Rgba::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn contrast_follows_wcag() {
        let (black, white) = (Rgba::hex(0x000000), Rgba::hex(0xFFFFFF));
        assert!((black.contrast(white) - 21.0).abs() < 0.01);
        assert!((white.contrast(white) - 1.0).abs() < 0.001);
        // White on Torro red: the skill measures 5.4.
        assert!((white.contrast(Rgba::from_rgb(TORRO_RED)) - 5.4).abs() < 0.05);
        // A translucent foreground is laid over the surface first.
        assert_eq!(Rgba::white(0.0).contrast(black), 1.0);
    }

    #[test]
    fn mix_and_over_match_torro_gui() {
        let (black, white) = (Rgba::hex(0x000000), Rgba::hex(0xFFFFFF));
        assert_eq!(Rgba::mix(black, white, 0.25), Rgba { r: 0.25, g: 0.25, b: 0.25, a: 1.0 });
        assert_eq!(Rgba::white(0.25).over(black), Rgba { r: 0.25, g: 0.25, b: 0.25, a: 1.0 });
    }

    #[test]
    fn standard_keeps_the_values_of_the_design_skill() {
        let d = Design::standard();
        assert_eq!(d.id, STANDARD_ID);
        assert!(d.is_standard());
        assert_eq!(d.colors.dark.bg, Rgba::hex(0x08090A));
        assert_eq!(d.colors.light.text_3, Rgba::hex(0x62666D));
        assert_eq!(d.colors.dark.accent, d.colors.light.accent);
        assert_eq!((d.colors.dark.line.a, d.colors.dark.focus_ring.a), (0.06, 0.35));
        assert_eq!((d.colors.light.line_selected.a, d.colors.light.focus_ring.a), (0.32, 0.5));
        assert_eq!((d.colors.dark.tint, d.colors.light.tint), (0.18, 0.14));
        assert_eq!(d.typo.body, Level { font: FontRole::Text, weight: Weight::Regular, size: 13.0, line: 20.0 });
        assert_eq!(d.typo.micro.font, FontRole::Numbers);
        assert_eq!(d.fonts.text.family, "Inter");
        assert_eq!(d.fonts.numbers.family, "Inter Tabular");
        assert_eq!(d.fonts.numbers.face(Weight::Semibold), None, "Inter Tabular has 400 and 500 only");
        assert_eq!([d.space.xs, d.space.xxl, d.radius.lg, d.layout.inset], [4.0, 32.0, 10.0, 8.0]);
        assert_eq!([d.size.row, d.size.sidebar, d.size.content_max], [40.0, 232.0, 720.0]);
        assert_eq!([d.size.row_message, d.size.row_notice], [92.0, 64.0], "the inbox's rows (tasks E3.9, E11.1)");
        // The comfortable density of the inbox (task E8.2).
        assert_eq!(d.typo.read, Level { font: FontRole::Text, weight: Weight::Regular, size: 14.0, line: 22.0 });
        // A reminder over the whole screen (task ER1.5).
        assert_eq!(d.typo.hero, Level { font: FontRole::Heading, weight: Weight::Semibold, size: 48.0, line: 56.0 });
        assert_eq!(d.typo.countdown, Level { font: FontRole::Numbers, weight: Weight::Medium, size: 28.0, line: 32.0 });
        assert_eq!(d.size.control_xl, 44.0);
        assert_eq!([d.size.card_gap, d.size.avatar, d.size.avatar_lg], [12.0, 36.0, 40.0]);
        assert_eq!([d.size.tile_sm, d.size.tile, d.size.tile_lg], [20.0, 28.0, 36.0]);
        // The mail window's large buttons (task E11.1).
        assert_eq!([d.size.command, d.size.pill], [46.0, 42.0]);
        assert_eq!(d.logo, Logo::default());
        // The chart colours as the dataviz validator checked them (task E8.8b).
        assert_eq!(d.colors.dark.chart.map(hex), [0x3B78D8, 0xC9701E, 0x2E9E6B, 0xB5509A]);
        assert_eq!(d.colors.light.chart.map(hex), [0x2F6BC9, 0xB8621A, 0x2A9466, 0xA8448C]);
        assert_eq!((hex(d.colors.dark.chart_other), hex(d.colors.light.chart_other)), (0x5C6068, 0x50545B));
    }

    /// `colour` as `0xRRGGBB`.
    fn hex(colour: Rgba) -> u32 {
        let byte = |channel: f32| (channel * 255.0).round() as u32;
        byte(colour.r) << 16 | byte(colour.g) << 8 | byte(colour.b)
    }

    /// Task E8.8b: chart colours are never the accent or a status colour, in either mode.
    #[test]
    fn chart_colours_are_neither_accent_nor_status() {
        let d = Design::standard();
        for p in [d.colors.dark, d.colors.light] {
            let reserved = [p.accent, p.ok, p.attention, p.error, p.inactive, p.merged];
            for colour in p.chart.iter().chain([&p.chart_other]) {
                assert!(!reserved.contains(colour), "{colour:?}");
            }
        }
    }

    /// Task E11.1: a pack written before the mail window's large buttons has no `command` and no
    /// `pill`; it takes them from the standard, while a pack that names them keeps its own.
    #[test]
    fn a_pack_without_the_mail_sizes_takes_the_standards() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let pack = |id: &str, size: &str| {
            std::fs::create_dir_all(tmp.path().join(id)).expect("pack folder");
            let toml = format!("format = 1\n[about]\nname = \"{id}\"\nversion = \"1.0.0\"\n[size]\n{size}");
            std::fs::write(tmp.path().join(id).join("design.toml"), toml).expect("design.toml");
        };
        pack("older", "row = 44\n");
        pack("roomier", "command = 52\npill = 40\n");
        let older = load(tmp.path(), "older").expect("valid");
        let standard = Design::standard().size;
        assert_eq!((older.size.command, older.size.pill), (standard.command, standard.pill));
        assert_eq!(older.size.row, 44.0, "its own sizes stay");
        let roomier = load(tmp.path(), "roomier").expect("valid");
        assert_eq!((roomier.size.command, roomier.size.pill), (52.0, 40.0));
    }

    #[test]
    fn derived_text_colours_follow_torro_gui() {
        let d = Design::standard();
        assert_eq!(d.colors.dark.attention_text(Mode::Dark), d.colors.dark.attention);
        assert_eq!(
            d.colors.light.error_text(Mode::Light),
            Rgba::mix(Rgba::from_rgb(STATUS_ERROR), Rgba::hex(0x18191B), 0.3)
        );
    }

    #[test]
    fn ids_are_short_lowercase_names() {
        for good in ["plex", "my-design-2", "a"] {
            assert!(is_valid_id(good), "{good}");
        }
        for bad in ["", "Plex", "my_design", "../x", "torro", "x".repeat(33).as_str(), "plex.d"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }
}
