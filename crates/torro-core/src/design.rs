//! The look of the suite as data, in three parts (concept `docs/design-pakete.md`, tasks L.1 and L.19).
//!
//! - A [`Design`] is the form: the type scale (role, size, line height and weight of each level, sizes
//!   meant for Inter), spacing, radii, layout gaps and sizes. Folder `designs/<id>/design.toml` ([`dir`],
//!   [`load`], [`list`]), with an optional `preview.png`.
//! - A [`FontSet`] is the font: the families of the three roles text, heading and numbers, and the size
//!   adjustment that makes its lowercase look as large as Inter's. Folder `fonts/<id>/font.toml`
//!   ([`font_dir`], [`load_font`], [`list_fonts`]) with the font files and `LICENSE.txt`. The fonts
//!   installed on the computer can be used too, by name and never copied ([`SystemFonts`], issue #299):
//!   [`SYSTEM_ID`] follows the system's interface font, `system:<family>` names one family.
//! - A [`ColorScheme`] is the colour of both modes and the logo. Folder
//!   `color-schemes/<id>/color-scheme.toml` ([`color_scheme_dir`], [`load_color_scheme`],
//!   [`list_color_schemes`]) with an optional `logo.svg`.
//!
//! `torro` ([`STANDARD_ID`]) is all three, built in. Its design is the file `designs/torro/design.toml` of
//! the repository, embedded at build time; two more designs are built in the same way, `compact` and
//! `console` ([`BUILT_IN_IDS`], issue #291). The standard font and colour scheme are exactly the values
//! `torro_gui::theme`, `torro_gui::typo` and [`crate::theme`] had as constants. The parts meet only through fixed roles (the
//! three font roles and four weights, the colour roles of [`Palette`]), and every rule checks one part
//! only, so any valid design, font and colour scheme go together. [`Selection`] puts them together and
//! gives a text level as it is drawn ([`Selection::level`]: size adjustment, nearest weight).
//!
//! Each part fills what it leaves out from the standard and is checked against the rules of the concept;
//! one that breaks a rule is refused as a whole, with every reason ([`DesignError`]). Nothing here runs
//! anything from a folder: only the TOML file, SVG, PNG and TTF/OTF files are read, each only after its
//! size was checked, and paths never leave the part's folder.
//!
//! No Iced in here: colours are [`Rgba`] with `f32` channels in 0–1, built the way Iced builds a
//! `Color` from 8-bit values, so `torro_gui` converts them 1:1 (`Color { r, g, b, a }`).

mod error;
mod files;
mod font;
mod pack;
mod rules;
mod scheme;
mod system;
#[cfg(test)]
mod testdata;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::AppId;
use crate::paths::AppPaths;
use crate::settings::MailFormat;
use crate::theme::{Rgb, STATUS_ATTENTION, STATUS_ERROR, STATUS_INACTIVE, STATUS_OK, TORRO_RED};

pub use error::DesignError;
pub use font::{list_fonts, load_font};
pub use pack::{Listed, list, load};
pub use scheme::{list_color_schemes, load_color_scheme};
pub use system::{SystemFonts, is_family_name, list_system_fonts};

/// The id of the standard design. It has no folder; [`load`] answers it without touching the disk,
/// and a folder of that name under [`dir`] is refused ([`DesignError::BadId`]).
pub const STANDARD_ID: &str = "torro";

/// The font that follows the interface font of the operating system (issue #299), resolved anew each
/// time it is loaded ([`SystemFonts`]). Reserved like [`STANDARD_ID`]: no folder may carry it.
pub const SYSTEM_ID: &str = "system";

/// The start of the id of an installed font: `system:<family>` (`system:Avenir Next`). Folder ids are
/// `a-z0-9-` only ([`is_valid_id`]), so none can begin like this.
pub const SYSTEM_PREFIX: &str = "system:";

/// Whether `id` names a font of the computer: [`SYSTEM_ID`] or `system:<family>`.
pub fn is_system_id(id: &str) -> bool {
    id == SYSTEM_ID || id.starts_with(SYSTEM_PREFIX)
}

/// The designs built into the suite, in the order [`list`] gives them: the standard ("Standard"),
/// "Compact" and "Console" (issue #291). Each is `designs/<id>/design.toml` in the repository,
/// embedded at build time. [`load`] answers each without touching the disk, and a folder of one of
/// these names under [`dir`] is refused ([`DesignError::BadId`]). Fonts and colour schemes have only
/// [`STANDARD_ID`] built in.
pub const BUILT_IN_IDS: [&str; 3] = [STANDARD_ID, "compact", "console"];

/// The folder that holds the designs: `designs/` in the core's data directory (`TORRO_DATA_HOME`
/// overrides the root). `None` when no home directory is known.
///
/// Callers pass it to [`list`] and [`load`]; tests pass a temporary folder instead.
pub fn dir() -> Option<PathBuf> {
    data_folder(Part::Design)
}

/// The folder that holds the fonts: `fonts/` beside [`dir`]. `None` when no home directory is known.
///
/// Callers pass it to [`list_fonts`] and [`load_font`].
pub fn font_dir() -> Option<PathBuf> {
    data_folder(Part::Font)
}

/// The folder that holds the colour schemes: `color-schemes/` beside [`dir`]. `None` when no home
/// directory is known.
///
/// Callers pass it to [`list_color_schemes`] and [`load_color_scheme`].
pub fn color_scheme_dir() -> Option<PathBuf> {
    data_folder(Part::ColorScheme)
}

fn data_folder(part: Part) -> Option<PathBuf> {
    AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.data.join(part.folder()))
}

/// The three parts of the look, for messages that name the folder a file belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// `designs/<id>/design.toml`.
    Design,
    /// `fonts/<id>/font.toml`.
    Font,
    /// `color-schemes/<id>/color-scheme.toml`.
    ColorScheme,
}

impl Part {
    /// The folder's name in the data directory.
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Part::Design => "designs",
            Part::Font => "fonts",
            Part::ColorScheme => "color-schemes",
        }
    }

    /// The file each entry of the folder must have.
    pub(crate) fn file(self) -> &'static str {
        match self {
            Part::Design => "design.toml",
            Part::Font => "font.toml",
            Part::ColorScheme => "color-scheme.toml",
        }
    }
}

/// The form of the suite: the standard or one from [`dir`], already merged with the standard and
/// checked. Every field is set; there is nothing optional left to fill in.
///
/// The sizes of [`Design::typo`] are meant for Inter; [`Selection::level`] adjusts them to the chosen
/// font.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    /// [`STANDARD_ID`] or the folder name.
    pub id: String,
    /// Name, version and author, for the list in the control centre; with the font, colour scheme and
    /// mail view the design suggests.
    pub about: About,
    /// The eleven text levels (`torro_gui::typo`), sizes for Inter.
    pub typo: TypeScale,
    /// The spacing scale (`torro_gui::theme::space`).
    pub space: Space,
    /// The radius scale (`torro_gui::theme::radius`).
    pub radius: Radius,
    /// Margins, paddings and gaps by role (`torro_gui::layout`, task L.2).
    pub layout: Layout,
    /// Fixed sizes of controls, rows and regions (`torro_gui::theme::size`).
    pub size: Size,
    /// How lists are drawn (`[list]`, issue #291); the inbox's [`Density`] follows from it.
    pub list: ListLook,
    /// `preview.png` in the folder (at most 1200 × 800), for the control centre; `None` shows a
    /// swatch.
    pub preview: Option<PathBuf>,
}

/// What a design, a font or a colour scheme says about itself (`[about]`). Folder data: shown as
/// written, never translated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct About {
    /// Required in a folder.
    pub name: String,
    /// Required in a folder.
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    /// Designs only (`[about] font`): the id of the font the design is meant for. A valid id or
    /// [`STANDARD_ID`]; whether that font exists is not checked here (each part is checked on its
    /// own). The core service sets it when the design is chosen, and only when the font loads valid
    /// (task L.33); after that the font is chosen freely. Always `None` for fonts and colour schemes.
    /// Colours never belong to a design: `[about] color_scheme` is an unknown key since task L.33.
    pub font: Option<String>,
    /// Designs only (`[about] mail_view`, issue #291): how the design suggests a mail with HTML shows in
    /// the reader – `text` for Console. A suggestion like [`About::font`]; the core service sets it
    /// when the design is chosen (task L.33). Always `None` for fonts and colour schemes.
    pub mail_view: Option<MailFormat>,
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
    /// Weight 400. Always there for `text`; `heading` and `numbers` may bring only some weights, and
    /// a missing one is drawn in the nearest ([`Font::nearest`]).
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

    /// The weight drawn when a level asks for `wanted` (task L.19, "Ersatz der Gewichte"): `wanted`
    /// itself when the family has a face for it, else the next in the concept's order – medium →
    /// semibold → regular, semibold → bold → medium, bold → semibold – continued until every weight was
    /// tried, so there is always an answer. Only a family without any face gives `wanted` back, and
    /// reading never builds one.
    pub fn nearest(&self, wanted: Weight) -> Weight {
        let order = match wanted {
            Weight::Regular => [Weight::Regular, Weight::Medium, Weight::Semibold, Weight::Bold],
            Weight::Medium => [Weight::Medium, Weight::Semibold, Weight::Regular, Weight::Bold],
            Weight::Semibold => [Weight::Semibold, Weight::Bold, Weight::Medium, Weight::Regular],
            Weight::Bold => [Weight::Bold, Weight::Semibold, Weight::Medium, Weight::Regular],
        };
        order.into_iter().find(|weight| self.face(*weight).is_some()).unwrap_or(wanted)
    }
}

/// Where the bytes of one weight come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Face {
    /// Embedded in `torro_gui` (Inter and "Inter Tabular"); `torro_gui` picks the file by family
    /// and weight. Only the standard font uses it.
    Builtin,
    /// A TTF/OTF file inside the font's folder, absolute, checked to carry [`Font::family`].
    File(PathBuf),
    /// Drawn by [`Font::family`] from the fonts installed on the computer, without bytes of its own
    /// (issue #299): Iced loads the system's fonts itself, so nothing is copied or handed on – a PDF,
    /// which embeds its fonts, sets such a font in the standard's Inter.
    System,
}

/// What kind of letters a font has (`kind` in `font.toml`, issue #299): the control centre groups the
/// fonts by it. On the wire in lowercase (`"sans"`, `"serif"`, `"mono"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontKind {
    /// Without serifs; the standard's Inter.
    Sans,
    /// With serifs.
    Serif,
    /// Every letter as wide as the next.
    Mono,
}

/// Which of the [`Fonts`] a level uses (`font = "heading"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontRole {
    #[default]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    /// 400.
    #[default]
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

/// One text level: font role, weight, size and absolute line height (logical pixels). In a [`Design`]
/// the size is meant for Inter; [`Selection::level`] gives the level as drawn in the chosen font.
///
/// `Default` (and the `Default` of the other parts of a [`Design`]) is all zeros: nothing to draw with,
/// only the ground the complete built-in standard is read onto. A design comes from [`load`] or
/// [`Design::standard`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    pub font: FontRole,
    pub weight: Weight,
    pub size: f32,
    pub line: f32,
}

/// The name of one text level of a [`TypeScale`], for [`Selection::level`] and [`TypeScale::get`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelName {
    Micro,
    Caption,
    Meta,
    Body,
    BodyStrong,
    Read,
    Title,
    Headline,
    Display,
    Hero,
    Countdown,
}

impl LevelName {
    /// Every level, smallest first (the order of [`TypeScale::levels`]).
    pub const ALL: [LevelName; 11] = [
        LevelName::Micro,
        LevelName::Caption,
        LevelName::Meta,
        LevelName::Body,
        LevelName::BodyStrong,
        LevelName::Read,
        LevelName::Title,
        LevelName::Headline,
        LevelName::Display,
        LevelName::Hero,
        LevelName::Countdown,
    ];

    /// The key in `design.toml` (`[type.body_strong]`).
    pub fn key(self) -> &'static str {
        match self {
            LevelName::Micro => "micro",
            LevelName::Caption => "caption",
            LevelName::Meta => "meta",
            LevelName::Body => "body",
            LevelName::BodyStrong => "body_strong",
            LevelName::Read => "read",
            LevelName::Title => "title",
            LevelName::Headline => "headline",
            LevelName::Display => "display",
            LevelName::Hero => "hero",
            LevelName::Countdown => "countdown",
        }
    }
}

/// The eleven text levels, named as in `torro_gui::typo` (lowercase).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
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
    /// The level `name`, as the design sets it (sizes for Inter).
    pub fn get(&self, name: LevelName) -> Level {
        match name {
            LevelName::Micro => self.micro,
            LevelName::Caption => self.caption,
            LevelName::Meta => self.meta,
            LevelName::Body => self.body,
            LevelName::BodyStrong => self.body_strong,
            LevelName::Read => self.read,
            LevelName::Title => self.title,
            LevelName::Headline => self.headline,
            LevelName::Display => self.display,
            LevelName::Hero => self.hero,
            LevelName::Countdown => self.countdown,
        }
    }

    /// Every level with its key (`"body"`), smallest first.
    pub fn levels(&self) -> [(&'static str, Level); 11] {
        LevelName::ALL.map(|name| (name.key(), self.get(name)))
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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Space {
    pub xs: f32,
    pub s: f32,
    pub m: f32,
    pub l: f32,
    pub xl: f32,
    pub xxl: f32,
}

/// The radius scale (`radius = [xs, sm, md, lg]`), never falling from small to large: equal steps
/// are allowed (Console: all 0, Standard and Compact: all 4).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radius {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

/// Padding as `[vertical, horizontal]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pad {
    pub vertical: f32,
    pub horizontal: f32,
}

/// How close the inbox's entries stand, a value of the design and not a choice made per window:
/// `Compact` draws one row per entry, `Comfortable` draws cards with a preview, chips and buttons
/// (`torro_gui::widgets::card`, task E8.2). The whole suite shows the density of the design that is
/// active, so every window agrees (task E13.1, issue #226).
///
/// Since issue #291 it is no key of its own (`layout.density` is gone): it follows the list style,
/// [`ListLook::density`] – `cards` is `Comfortable`, `rows` and `line` are `Compact`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    /// One row per entry; what the standard design "Torro" shows.
    Compact,
    /// Cards with a preview and their buttons shown; only a pack switches it on.
    Comfortable,
}

/// Margins, paddings and gaps by role (`[layout]`), the same for every element of that role.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Layout {
    /// The margin around the inset content panel beside the sidebar (top, right, bottom), read as
    /// `torro_gui::layout::inset()` by `torro_gui::shell`. Standard 0 (issue #291): the content is
    /// flush, a hairline parts it from the sidebar; above 0 it is an inset panel with corners.
    pub inset: f32,
    /// Padding of content and pages. Standard 24 × 24: the control centre's pages in the wide and
    /// medium layout (`page_margin` in `torroworkplace/src/ui/mod.rs`).
    pub page_padding: Pad,
    /// Padding of panels, cards and dialogs. Standard 16 × 16: the floating card
    /// (`torro_gui::widgets::popover`, 16 above and below, 8 + 8 at the sides).
    pub panel_padding: Pad,
    /// Padding of list rows. Standard 18 × 24: `torro_gui::widgets::list_row` and `group_row` take
    /// the sides (the height comes from [`Size::row`], the content is centred in it); the inbox's rows
    /// of several lines also take above and below.
    pub row_padding: Pad,
    /// Padding of buttons, chips and inputs. Standard 0 × 12: `torro_gui::style::button_padding()`.
    pub control_padding: Pad,
    /// Between groups and sections. Standard 24: the groups of the control centre's General page
    /// (`torroworkplace/src/ui/general.rs`).
    pub section_gap: f32,
    /// Between elements inside a group. Standard 10: `torro_gui::widgets::group`.
    pub item_gap: f32,
    /// The padding of a mail in the reader, inside the reading column [`Size::reading_max`]
    /// (`[layout] reading_padding`, issue #291): Standard 52 × 64, Compact 32 × 40, Console 14 × 18,
    /// read as `torro_gui::layout::reading_padding()`.
    pub reading_padding: Pad,
}

/// How lists are drawn (`[list]`, issue #291): the same cells in the same places in every design, only
/// drawn another way – several lines or one, lines between rows or not, how the chosen row shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ListLook {
    /// Rows with several lines, one line, or cards ([`ListStyle`]).
    pub style: ListStyle,
    /// A hairline below each row.
    pub separators: bool,
    /// How the chosen row shows ([`ListSelection`]).
    pub selection: ListSelection,
    /// The edge of a sender's avatar in a mail row and in the reader's head, 20–48; `0` draws no avatar
    /// and gives its place to the text.
    pub avatar: f32,
    /// Lines of preview under the subject of a mail row, 1 or 2.
    pub preview_lines: u8,
}

impl ListLook {
    /// The inbox's [`Density`] of this list style: [`ListStyle::Cards`] is `Comfortable`, the others
    /// are `Compact`.
    pub fn density(&self) -> Density {
        match self.style {
            ListStyle::Cards => Density::Comfortable,
            ListStyle::Rows | ListStyle::Line => Density::Compact,
        }
    }
}

/// How the entries of a list are drawn (`[list] style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListStyle {
    /// `"rows"`: a row per entry with its lines (a mail: avatar, sender, subject, preview). Standard
    /// and Compact.
    #[default]
    Rows,
    /// `"line"`: one line per entry in fixed columns, as tall as [`Size::row_message`]. Console.
    Line,
    /// `"cards"`: a card per entry with its buttons shown ([`Density::Comfortable`]).
    Cards,
}

/// How the chosen row of a list shows (`[list] selection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListSelection {
    /// `"bar"`: a bar at the left edge of the row. Standard and Compact.
    #[default]
    Bar,
    /// `"fill"`: the whole row in `accent`, its text and icons in `on_accent`. Console.
    Fill,
}

/// How much a design shows at once, for the list of designs (task L.33, `DesignInfo::density`):
/// decided by the height of a mail row ([`Design::density_level`]), so a design can't put itself
/// in the wrong class.
///
/// On the wire (`DesignInfo::density`) in snake case: `"airy"`, `"balanced"`, `"dense"`,
/// `"very_dense"`; [`DensityLevel::Balanced`] is the default, so a reply without it still reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DensityLevel {
    /// A mail row of 110 or more.
    Airy,
    /// 88 to 109 (Standard: 100).
    #[default]
    Balanced,
    /// 56 to 87 (Compact: 76).
    Dense,
    /// Below 56 (Console: 24, one line).
    VeryDense,
}

/// Fixed sizes (`torro_gui::theme::size`, lowercase). `INSET` moved to [`Layout::inset`];
/// `HAIRLINE` stays a constant in `torro_gui` (always one logical pixel, not a design choice).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
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
    /// lines with the row padding above and below (task E11.1). Standard 100 (three lines of 22 and
    /// 18 above and below), one line in Console (24).
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
    /// The collapsed sidebar and the collapsed action rail of the mail window (issue #291), 40–80 and
    /// at least [`Size::control_lg`], the rail's button. Standard 60 (`torro_gui::shell::rail()`).
    pub rail: f32,
    /// The expanded action rail of the mail window (issue #291), 180–320. Standard 248
    /// (`torro_gui::widgets::RailMode::Expanded`).
    pub rail_wide: f32,
    /// The widest the text of a mail stands in the reader (issue #291), 480–1200: the reading column.
    /// Standard 600, Compact 640, Console 1200.
    pub reading_max: f32,
}

impl Size {
    /// Every size with its key (`"row"`), for checks and error messages.
    pub fn entries(&self) -> [(&'static str, f32); 32] {
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
            ("rail", self.rail),
            ("rail_wide", self.rail_wide),
            ("reading_max", self.reading_max),
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
            "rail" => &mut self.rail,
            "rail_wide" => &mut self.rail_wide,
            "reading_max" => &mut self.reading_max,
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
    /// Unread mail in lists and cards: a blue dot, separate from account and category colours.
    /// At least 3:1 on every mail row and card surface.
    pub unread: Rgba,
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
    /// The colours of the five groups of mail categories (task E12.7, `docs/kategorien.md`), in
    /// the order of `torro_mail::CategoryGroup`: waiting for me, paper trail, info, unwanted,
    /// unsorted. A mark
    /// only – the dot and the border of a category pill – never a text colour, and checked as a
    /// set for colour blindness like [`Palette::chart`]. One colour per **group**, not per
    /// category: 23 colours could not stay distinguishable.
    pub category: [Rgba; 5],
    /// The colour of the shadow under menus, cards and dialogs; offset (0, 12) and blur 32 stay
    /// fixed in `torro_gui` (geometry, not colour).
    pub shadow: Rgba,
    pub scrim: Rgba,
    pub night: Rgba,
    /// The thin line of the working hours in the calendar (task K.33): a mark beside the events,
    /// never text. Muted, so it is not mistaken for the now line or an event's colour bar. A pack
    /// that leaves it out gets its own `line_selected`.
    pub working_hours: Rgba,
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

/// The working-hours key of a palette with the key it falls back to when a pack leaves it out: the
/// neutral grey of the pack's selection edges, so a pack made before task K.33 still draws it in
/// its own colours.
pub(crate) const WORKING_HOURS_FALLBACK: (&str, &str) = ("working_hours", "line_selected");

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
            "unread" => &mut self.unread,
            "chart_other" => &mut self.chart_other,
            "shadow" => &mut self.shadow,
            "scrim" => &mut self.scrim,
            "night" => &mut self.night,
            "working_hours" => &mut self.working_hours,
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

/// The logo of a colour scheme, absolute; `None` draws the suite's own in the accent colour.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Logo {
    /// The mark in the app switcher and the rail (SVG). `None`: the "T" on `accent`. There is no tray
    /// symbol: the tray always draws the four red squares (issue #114).
    pub mark: Option<PathBuf>,
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
    /// The standard design: `designs/torro/design.toml` of the repository, embedded at build time and
    /// read once per process. The file sets every key, so nothing here fills in a value; with
    /// [`FontSet::standard`] and [`ColorScheme::standard`] it is the whole look of the suite as it ships.
    ///
    /// # Panics
    ///
    /// When the embedded file can't be read or breaks a rule. That is a broken build, never a state of
    /// the disk: the test `the_built_in_designs_load_without_reasons` stops such a build, and there are
    /// no stand-in values in the code that would have to be kept in step with the file.
    pub fn standard() -> Design {
        STANDARD.clone()
    }

    /// Whether this is the standard design.
    pub fn is_standard(&self) -> bool {
        self.id == STANDARD_ID
    }

    /// How much this design shows at once, from the height of a mail row ([`Size::row_message`]): from
    /// 110 [`DensityLevel::Airy`], from 88 `Balanced`, from 56 `Dense`, below that `VeryDense`.
    pub fn density_level(&self) -> DensityLevel {
        match self.size.row_message {
            h if h >= 110.0 => DensityLevel::Airy,
            h if h >= 88.0 => DensityLevel::Balanced,
            h if h >= 56.0 => DensityLevel::Dense,
            _ => DensityLevel::VeryDense,
        }
    }
}

/// The standard design, read from the embedded file on first use ([`Design::standard`]).
static STANDARD: LazyLock<Design> = LazyLock::new(|| match pack::built_in_standard() {
    Ok(design) => design,
    Err(problems) => {
        let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
        panic!("designs/torro/design.toml, built into torro-core, can't be used – a broken build: {reasons:?}")
    }
});

/// The families of a font with their faces, its licence and its size adjustment: the standard or one
/// from [`font_dir`], merged with the standard and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSet {
    /// [`STANDARD_ID`] or the folder name.
    pub id: String,
    /// Name, version and author from `font.toml`. The control centre names a font by the family of
    /// its text font ([`Font::family`] of [`Fonts::text`]) and shows this name in the tooltip.
    pub about: About,
    /// SPDX id of the licence of the font files (`license = "OFL-1.1"`), required as soon as the font
    /// brings files; the standard's ("OFL-1.1", Inter) when it brings none.
    pub license: String,
    /// The families of the three roles. A role `font.toml` leaves out is the text font; without
    /// `[fonts.text]` the roles left out are the standard's.
    pub fonts: Fonts,
    /// The factor every size of a design is multiplied by in this font (as `font-size-adjust` in CSS),
    /// so its lowercase looks as large as Inter's: `size_adjust` from `font.toml` when set (allowed
    /// 0.85–1.25), else Inter's x-height divided by the x-height of the text font's regular face
    /// ([`INTER_X_HEIGHT`], OS/2 `sxHeight`, else the height of its "x"), held to 0.90–1.15. Exactly 1
    /// for Inter. [`FontSet::apply`] uses it.
    pub size_adjust: f32,
    /// `kind` from `font.toml`; `None` when it leaves it out and for a font of the computer, whose kind
    /// can't be told reliably. [`FontKind::Sans`] for the standard.
    pub kind: Option<FontKind>,
}

impl FontSet {
    /// The standard font "Torro": Inter for text and headings, Inter Tabular for figures, embedded in
    /// `torro_gui`, size adjustment 1.
    pub fn standard() -> FontSet {
        FontSet {
            id: STANDARD_ID.to_string(),
            about: About {
                name: "Torro".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: Some("Torro".to_string()),
                ..About::default()
            },
            license: "OFL-1.1".to_string(),
            fonts: inter(),
            size_adjust: 1.0,
            kind: Some(FontKind::Sans),
        }
    }

    /// Whether this is the standard font.
    pub fn is_standard(&self) -> bool {
        self.id == STANDARD_ID
    }

    /// `level` of a design (size meant for Inter) as drawn in this font (task L.19):
    ///
    /// - **size** = size × [`FontSet::size_adjust`], rounded to 0.5, never below [`MIN_TEXT_SIZE`];
    /// - **line height** = the design's, but at least [`LINE_FLOOR`] × the new size – never more than
    ///   the design's own ratio of line to size, so a tight display level (Torro: 28/32) stays tight
    ///   and Inter gives exactly the design's values;
    /// - **weight** = the nearest the role's family has ([`Font::nearest`]);
    /// - the role stays.
    pub fn apply(&self, level: Level) -> Level {
        let size = ((level.size * self.size_adjust * 2.0).round() / 2.0).max(MIN_TEXT_SIZE);
        let ratio = if level.size > 0.0 { (level.line / level.size).min(LINE_FLOOR) } else { LINE_FLOOR };
        Level {
            font: level.font,
            weight: self.fonts.get(level.font).nearest(level.weight),
            size,
            line: level.line.max(size * ratio),
        }
    }
}

/// The colours of both modes and the logo: the standard or one from [`color_scheme_dir`], merged
/// with the standard and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorScheme {
    /// [`STANDARD_ID`] or the folder name.
    pub id: String,
    /// Name, version, author and description from `color-scheme.toml`.
    pub about: About,
    /// The colours of both modes. For a scheme that is [`ColorScheme::dark_only`], `light` is a copy of
    /// `dark`.
    pub colors: Colors,
    /// Its own logo, if it brings one.
    pub logo: Logo,
    /// `dark_only = true` in `color-scheme.toml` (issue #291): the scheme has dark colours only, so
    /// every app draws it dark in either mode. Such a file may not have `[colors.light]`.
    pub dark_only: bool,
}

impl ColorScheme {
    /// The standard colour scheme "Torro": quiet surfaces, Torro red, the suite's own logo.
    pub fn standard() -> ColorScheme {
        ColorScheme {
            id: STANDARD_ID.to_string(),
            about: About {
                name: "Torro".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: Some("Torro".to_string()),
                ..About::default()
            },
            colors: STANDARD_COLORS,
            logo: Logo::default(),
            dark_only: false,
        }
    }

    /// Whether this is the standard colour scheme.
    pub fn is_standard(&self) -> bool {
        self.id == STANDARD_ID
    }
}

/// The look as chosen: a design, a font and a colour scheme, each valid on its own, so every
/// combination is valid too (concept: "Die Schnittstellen zwischen den Teilen").
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub design: Design,
    pub font: FontSet,
    pub scheme: ColorScheme,
}

impl Selection {
    /// Torro, Torro, Torro: exactly the look of the suite before design packs.
    pub fn standard() -> Selection {
        Selection { design: Design::standard(), font: FontSet::standard(), scheme: ColorScheme::standard() }
    }

    /// The text level `name` as drawn: the design's level with the font's size adjustment and nearest
    /// weight ([`FontSet::apply`]). The family is `self.font.fonts.get(level.font)`.
    pub fn level(&self, name: LevelName) -> Level {
        self.font.apply(self.design.typo.get(name))
    }
}

/// The smallest text the suite draws (`typo::micro()`); a size adjustment never goes below it.
pub const MIN_TEXT_SIZE: f32 = 11.0;

/// A level's line height is at least this times its size after the size adjustment, as far as the
/// design's own ratio goes ([`FontSet::apply`]).
pub const LINE_FLOOR: f32 = 1.2;

/// Inter's x-height in em: `sxHeight` 1118 of 2048 units, the same in every cut of Inter 4 (measured
/// 2026-10-20). The sizes of every design are meant for Inter; a font's size adjustment is this divided
/// by its own x-height.
pub const INTER_X_HEIGHT: f32 = 1118.0 / 2048.0;

/// The widest a figure may be in the `numbers` font, in em of the design's size, after the size
/// adjustment (and never less than the plain width, because sizes below [`MIN_TEXT_SIZE`] are raised
/// to it): what the time axis of a design is made to hold (`size.axis`, `size.axis_narrow`).
///
/// Measured 2026-10-20 on the figures fonts of the samples (widest figure of any cut × adjustment):
/// Inter Tabular 0.648 × 1 = 0.648, IBM Plex Sans 0.600 × 1.058 = 0.635, Commitr Figures (Mona Sans)
/// 0.590 × 1.038 = 0.612, Lato 0.580 × 1.078 = 0.625, Figtree Tabular 0.626 × 1.092 = 0.684, Roboto
/// Flex 0.565 × 1.063 = 0.600, Selawik 0.575 × 1.092 = 0.628. The widest, Figtree Tabular, plus 5 %:
/// 0.718, rounded up to 0.72.
pub const DIGIT_MAX_EM: f32 = 0.72;

/// Inter for text and headings, Inter Tabular (400 and 500) for figures, all embedded.
fn inter() -> Fonts {
    let inter = |family: &str, semibold: bool| Font {
        family: family.to_string(),
        regular: Some(Face::Builtin),
        medium: Some(Face::Builtin),
        semibold: semibold.then_some(Face::Builtin),
        bold: None,
    };
    Fonts { text: inter("Inter", true), heading: inter("Inter", true), numbers: inter("Inter Tabular", false) }
}

const STANDARD_COLORS: Colors = Colors { dark: STANDARD_DARK, light: STANDARD_LIGHT };

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
    unread: Rgba::hex(0x66A3FF),
    calendar: [Rgba::hex(0x5B8CFF), Rgba::hex(0x3FB984), Rgba::hex(0xE8A33D), Rgba::hex(0xD96CA8), Rgba::hex(0xA78BFA)],
    tint: 0.18,
    // Checked with the dataviz validator against `panel` (task E8.8b): lightness band, chroma, CVD
    // ΔE ≥ 8 for every adjacent pair, ≥ 3:1. "Other" is ΔE ≥ 8 from each of the four.
    chart: [Rgba::hex(0x3B78D8), Rgba::hex(0xC9701E), Rgba::hex(0x2E9E6B), Rgba::hex(0xB5509A)],
    chart_other: Rgba::hex(0x5C6068),
    // Waiting for me (blue), paper trail (amber), info (teal), unwanted (grey-violet), unsorted
    // (rosé, issue #338: a neutral grey would read like the teal for red-green colour blindness).
    category: [Rgba::hex(0x4C8DF0), Rgba::hex(0xCF8A2C), Rgba::hex(0x2BA39A), Rgba::hex(0xA99BD0), Rgba::hex(0xD9739A)],
    shadow: Rgba::black(0.5),
    scrim: Rgba::black(0.62),
    night: Rgba::hex(0x121626),
    working_hours: Rgba::white(0.28),
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
    unread: Rgba::hex(0x2563D8),
    calendar: [Rgba::hex(0x3B6EF0), Rgba::hex(0x1F9D68), Rgba::hex(0xB06F12), Rgba::hex(0xC2508F), Rgba::hex(0x7C5CE6)],
    tint: 0.14,
    // The same hues stepped for the light panel, checked the same way (task E8.8b).
    chart: [Rgba::hex(0x2F6BC9), Rgba::hex(0xB8621A), Rgba::hex(0x2A9466), Rgba::hex(0xA8448C)],
    category: [Rgba::hex(0x3D77CE), Rgba::hex(0xB1741F), Rgba::hex(0x238A82), Rgba::hex(0x756990), Rgba::hex(0xB34A6E)],
    chart_other: Rgba::hex(0x50545B),
    shadow: Rgba::black(0.12),
    scrim: Rgba::black(0.3),
    night: Rgba::hex(0xE3E7F1),
    working_hours: Rgba::black(0.32),
    sidebar: Rgba::hex(0xF4F5F6),
    sidebar_hover: Rgba::hex(0xF0F1F3),
    sidebar_selected: Rgba::hex(0xE9EAED),
    sidebar_text: Rgba::hex(0x18191B),
    sidebar_text_2: Rgba::hex(0x4F535A),
    sidebar_text_3: Rgba::hex(0x62666D),
    sidebar_line: Rgba::black(0.07),
};

/// Whether `id` may name a pack folder: `a-z`, `0-9`, `-`, 1–32 characters, not [`STANDARD_ID`] and
/// not [`SYSTEM_ID`] (the fonts of the computer, issue #299).
pub fn is_valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id != STANDARD_ID
        && id != SYSTEM_ID
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
        let c = ColorScheme::standard().colors;
        let f = FontSet::standard();
        assert_eq!(d.id, STANDARD_ID);
        assert!(d.is_standard() && f.is_standard() && ColorScheme::standard().is_standard());
        assert_eq!(c.dark.bg, Rgba::hex(0x08090A));
        assert_eq!(c.light.text_3, Rgba::hex(0x62666D));
        assert_eq!(c.dark.accent, c.light.accent);
        assert_eq!((c.dark.line.a, c.dark.focus_ring.a), (0.06, 0.35));
        assert_eq!((c.light.line_selected.a, c.light.focus_ring.a), (0.32, 0.5));
        assert_eq!((c.dark.tint, c.light.tint), (0.18, 0.14));
        // Issue #291 (task L.34): the new standard's measures.
        assert_eq!(d.typo.body, Level { font: FontRole::Text, weight: Weight::Regular, size: 14.0, line: 22.0 });
        assert_eq!(d.typo.micro.font, FontRole::Numbers);
        assert_eq!(f.fonts.text.family, "Inter");
        assert_eq!(f.fonts.numbers.family, "Inter Tabular");
        assert_eq!(f.fonts.numbers.face(Weight::Semibold), None, "Inter Tabular has 400 and 500 only");
        assert_eq!((f.size_adjust, f.license.as_str()), (1.0, "OFL-1.1"));
        assert_eq!([d.space.xs, d.space.xxl, d.radius.lg, d.layout.inset], [4.0, 32.0, 4.0, 0.0]);
        assert_eq!([d.size.row, d.size.sidebar, d.size.content_max], [48.0, 240.0, 720.0]);
        assert_eq!([d.size.row_message, d.size.row_notice], [100.0, 80.0], "the inbox's rows (tasks E3.9, E11.1)");
        // The comfortable density of the inbox (task E8.2).
        assert_eq!(d.typo.read, Level { font: FontRole::Text, weight: Weight::Regular, size: 16.0, line: 26.0 });
        // The standard shows rows, not cards (task E13.1): the density is the design's list style, and
        // the standard's rows are compact. Only a pack switches the cards on.
        assert_eq!(d.list.density(), Density::Compact);
        // A reminder over the whole screen (task ER1.5).
        assert_eq!(d.typo.hero, Level { font: FontRole::Heading, weight: Weight::Semibold, size: 48.0, line: 56.0 });
        assert_eq!(d.typo.countdown, Level { font: FontRole::Numbers, weight: Weight::Medium, size: 28.0, line: 32.0 });
        assert_eq!(d.size.control_xl, 48.0);
        assert_eq!([d.size.card_gap, d.size.avatar, d.size.avatar_lg], [12.0, 40.0, 48.0]);
        assert_eq!([d.size.tile_sm, d.size.tile, d.size.tile_lg], [20.0, 28.0, 36.0]);
        // The mail window's large buttons (task E11.1).
        assert_eq!([d.size.command, d.size.pill], [46.0, 40.0]);
        assert_eq!(ColorScheme::standard().logo, Logo::default());
        // The chart colours as the dataviz validator checked them (task E8.8b).
        assert_eq!(c.dark.chart.map(hex), [0x3B78D8, 0xC9701E, 0x2E9E6B, 0xB5509A]);
        assert_eq!(c.light.chart.map(hex), [0x2F6BC9, 0xB8621A, 0x2A9466, 0xA8448C]);
        assert_eq!((hex(c.dark.chart_other), hex(c.light.chart_other)), (0x5C6068, 0x50545B));
    }

    /// Issue #291: the inbox's density follows the list style – only cards are comfortable.
    #[test]
    fn cards_are_the_comfortable_density() {
        let look = |style| ListLook { style, ..Design::standard().list };
        assert_eq!(look(ListStyle::Cards).density(), Density::Comfortable);
        assert_eq!(look(ListStyle::Rows).density(), Density::Compact);
        assert_eq!(look(ListStyle::Line).density(), Density::Compact);
    }

    /// Issue #291: the density level comes from the height of a mail row, at the bounds 110, 88 and 56.
    #[test]
    fn the_density_level_follows_the_mail_row() {
        let level = |row_message: f32| {
            let mut d = Design::standard();
            d.size.row_message = row_message;
            d.density_level()
        };
        assert_eq!([level(120.0), level(110.0)], [DensityLevel::Airy; 2]);
        assert_eq!([level(109.0), level(88.0)], [DensityLevel::Balanced; 2]);
        assert_eq!([level(87.0), level(56.0)], [DensityLevel::Dense; 2]);
        assert_eq!([level(55.0), level(24.0)], [DensityLevel::VeryDense; 2]);
    }

    /// Task L.33: the wire form of the density level in `DesignInfo::density`, `balanced` by default.
    #[test]
    fn the_density_level_is_written_in_snake_case() {
        let words = [DensityLevel::Airy, DensityLevel::Balanced, DensityLevel::Dense, DensityLevel::VeryDense]
            .map(|level| serde_json::to_value(level).expect("a word"));
        assert_eq!(words, ["airy", "balanced", "dense", "very_dense"]);
        let read: DensityLevel = serde_json::from_value(serde_json::json!("very_dense")).expect("a level");
        assert_eq!((read, DensityLevel::default()), (DensityLevel::VeryDense, DensityLevel::Balanced));
    }

    /// `colour` as `0xRRGGBB`.
    fn hex(colour: Rgba) -> u32 {
        let byte = |channel: f32| (channel * 255.0).round() as u32;
        byte(colour.r) << 16 | byte(colour.g) << 8 | byte(colour.b)
    }

    /// Task E8.8b: chart colours are never the accent or a status colour, in either mode.
    #[test]
    fn chart_colours_are_neither_accent_nor_status() {
        let c = ColorScheme::standard().colors;
        for p in [c.dark, c.light] {
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
        let c = ColorScheme::standard().colors;
        assert_eq!(c.dark.attention_text(Mode::Dark), c.dark.attention);
        assert_eq!(c.light.error_text(Mode::Light), Rgba::mix(Rgba::from_rgb(STATUS_ERROR), Rgba::hex(0x18191B), 0.3));
    }

    #[test]
    fn ids_are_short_lowercase_names() {
        for good in ["plex", "my-design-2", "a"] {
            assert!(is_valid_id(good), "{good}");
        }
        for bad in ["", "Plex", "my_design", "../x", "torro", "system", "x".repeat(33).as_str(), "plex.d"] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    /// Task L.19: Torro, Torro, Torro draws every level exactly as the design sets it.
    #[test]
    fn the_standard_selection_draws_the_designs_levels_unchanged() {
        let selection = Selection::standard();
        for name in LevelName::ALL {
            assert_eq!(selection.level(name), selection.design.typo.get(name), "{}", name.key());
        }
        let keys: Vec<&str> = Design::standard().typo.levels().iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, LevelName::ALL.map(LevelName::key), "one name per level, in the same order");
    }

    /// Task L.19: the size adjustment rounds to 0.5, never goes below 11 and keeps at least 1.2 × the
    /// size as line height, up to the design's own ratio.
    #[test]
    fn the_size_adjustment_rounds_and_keeps_room() {
        let font = |size_adjust: f32| FontSet { size_adjust, ..FontSet::standard() };
        let level = |size: f32, line: f32| Level { font: FontRole::Text, weight: Weight::Regular, size, line };
        let plex = font(1.058);
        assert_eq!(plex.apply(level(13.0, 20.0)), level(14.0, 20.0), "13.75 → 14, the line still has room");
        assert_eq!(plex.apply(level(12.0, 14.0)).size, 12.5, "12.7 → 12.5");
        let tight = plex.apply(level(28.0, 32.0));
        assert_eq!(tight.size, 29.5);
        assert!((tight.line - 29.5 * 32.0 / 28.0).abs() < 1e-4, "as tight as the design meant: {}", tight.line);
        let roomy = font(1.15).apply(level(20.0, 22.0));
        assert_eq!((roomy.size, roomy.line), (23.0, 23.0 * 1.1), "the design's 1.1, not 1.2");
        let open = font(1.15).apply(level(16.0, 17.0));
        assert_eq!(open.size, 18.5);
        assert!((open.line - 18.5 * 17.0 / 16.0).abs() < 1e-4);
        assert_eq!(font(0.85).apply(level(11.0, 14.0)), level(11.0, 14.0), "never below 11");
        let bodies = font(1.25).apply(level(13.0, 14.0));
        assert_eq!(bodies.size, 16.5, "16.25 → 16.5");
        assert!((bodies.line - 16.5 * 14.0 / 13.0).abs() < 1e-4, "{}", bodies.line);
    }

    /// Task L.19: a missing weight is drawn in the nearest, in the concept's order.
    #[test]
    fn missing_weights_are_replaced_in_the_concepts_order() {
        let file = || Some(Face::File(PathBuf::from("/x.ttf")));
        let font = |regular: bool, medium: bool, semibold: bool, bold: bool| Font {
            family: "Sample".into(),
            regular: regular.then(file).flatten(),
            medium: medium.then(file).flatten(),
            semibold: semibold.then(file).flatten(),
            bold: bold.then(file).flatten(),
        };
        let lato = font(true, false, false, true);
        assert_eq!(lato.nearest(Weight::Medium), Weight::Regular, "medium → (semibold) → regular");
        assert_eq!(lato.nearest(Weight::Semibold), Weight::Bold, "semibold → bold");
        let selawik = font(true, false, true, true);
        assert_eq!(selawik.nearest(Weight::Medium), Weight::Semibold, "medium → semibold");
        let no_bold = font(true, true, false, false);
        assert_eq!(no_bold.nearest(Weight::Bold), Weight::Medium, "bold → (semibold) → medium");
        assert_eq!(no_bold.nearest(Weight::Semibold), Weight::Medium, "semibold → (bold) → medium");
        let only_semibold = font(false, false, true, false);
        for wanted in [Weight::Regular, Weight::Medium, Weight::Bold] {
            assert_eq!(only_semibold.nearest(wanted), Weight::Semibold, "{wanted}");
        }
        assert_eq!(font(false, false, false, false).nearest(Weight::Bold), Weight::Bold, "nothing to choose");
        let standard = FontSet::standard();
        let countdown = standard.apply(Design::standard().typo.countdown);
        assert_eq!(countdown.weight, Weight::Medium, "Inter Tabular has Medium");
    }

    /// Task L.19: every design × every font × every colour scheme of the test data gives text levels
    /// that can be drawn – at least 11, with a line at least as high as the text. No combination is
    /// checked on its own; this holds because each part is checked on its own.
    #[test]
    fn every_valid_design_font_and_colour_scheme_go_together() {
        let data = testdata::Data::all();
        // The old complete pack `full` is no design any more (task L.21): a roomy one in its place.
        let roomy = "format = 1\n[about]\nname = \"Roomy\"\nversion = \"1.0.0\"\n\
                     [type.body]\nsize = 14\nline = 22\n[type.title]\nsize = 17\nline = 24\n[size]\nrow = 44\n";
        data.write(Part::Design, "roomy", roomy);
        fn valid<T>(listed: Vec<Listed<T>>) -> Vec<T> {
            listed.into_iter().filter_map(|entry| entry.result.ok()).collect()
        }
        let designs: Vec<Design> = valid(list(&data.dir(Part::Design)));
        let fonts: Vec<FontSet> = valid(list_fonts(&data.dir(Part::Font)));
        let schemes: Vec<ColorScheme> = valid(list_color_schemes(&data.dir(Part::ColorScheme)));
        assert!(designs.len() >= 3 && fonts.len() >= 5 && schemes.len() >= 4, "the test data has choices");
        for design in &designs {
            for font in &fonts {
                for scheme in &schemes {
                    let selection = Selection { design: design.clone(), font: font.clone(), scheme: scheme.clone() };
                    for name in LevelName::ALL {
                        let level = selection.level(name);
                        let which = format!("{} × {} × {}: {}", design.id, font.id, scheme.id, name.key());
                        assert!(level.size >= MIN_TEXT_SIZE, "{which}: {level:?}");
                        assert!(level.line >= level.size, "{which}: {level:?}");
                        assert!(selection.font.fonts.get(level.font).face(level.weight).is_some(), "{which}: a face");
                    }
                }
            }
        }
    }
}
