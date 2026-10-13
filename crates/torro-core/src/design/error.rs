//! Why a design pack was refused: one variant per rule of the concept (`docs/design-pakete.md`,
//! "Regeln"), each with a readable English sentence through `tr()`, because the control centre
//! shows it. Keys in the messages are the TOML keys (`colors.dark.text_3`), which pack authors know.

use std::fmt;

use crate::i18n::{tr, trf};

use super::{FontRole, Mode, Weight};

/// One reason a design pack can't be used. [`super::load`] collects every reason it finds.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignError {
    /// The folder name is not a valid id (`a-z`, `0-9`, `-`, at most 32) or is the standard's.
    BadId { id: String },
    /// There is no folder for this id.
    NotFound { id: String },
    /// A file could not be read (`reason` is the system's or the parser's).
    Unreadable { file: String, reason: String },
    /// A file is over its limit (`design.toml` 64 KB, SVG 256 KB, PNG 2 MB, font 5 MB).
    TooLarge { file: String, limit: String },
    /// All files together are over 20 MB (or there are too many of them).
    PackTooLarge { limit: String },
    /// `design.toml` is not valid TOML or a value has the wrong type; `line` counts from 1.
    Syntax { line: Option<usize>, reason: String },
    /// A key the format does not know (a typo should not go unnoticed).
    UnknownKey { key: String },
    /// A required key is missing (`format`, `about.name`, `about.version`, `about.font_license`,
    /// `fonts.<role>.family`, `fonts.text.regular`).
    MissingKey { key: String },
    /// `format` is not 1.
    UnsupportedFormat { found: i64 },
    /// A colour is not `#RRGGBB` / `#RRGGBBAA` (or `calendar` does not hold five of them).
    BadColor { key: String, value: String },
    /// `text`, `text_2` or `text_3` below 4.5:1 on a surface.
    TextContrast { fg: String, on: String, ratio: f32 },
    /// A control colour (focus ring, status, merged, calendar) below 3:1 on `panel`.
    ControlContrast { fg: String, on: String, ratio: f32 },
    /// `line_selected` does not stand clearly above `line_strong` or reaches `focus_ring`, so a
    /// choice reads like a frame or like focus.
    ChosenEdge { mode: Mode },
    /// `on_accent` below 4.5:1 on `accent` (text on the primary button).
    AccentContrast { mode: Mode, ratio: f32 },
    /// A number outside its range (type sizes, scales, layout gaps, sizes, `tint`).
    OutOfRange { key: String, value: f32, min: f32, max: f32 },
    /// A line height below the size or above twice the size.
    LineHeight { key: String, line: f32, size: f32 },
    /// `space.scale` or `radius.scale` does not grow from small to large.
    NotAscending { key: String },
    /// A text level uses a weight its font has no file for.
    MissingWeight { key: String, font: FontRole, weight: Weight },
    /// A file named in `design.toml` has the wrong type (`expected`: "SVG", "TTF or OTF").
    WrongFileType { key: String, path: String, expected: &'static str },
    /// A file named in `design.toml` does not exist.
    MissingFile { key: String, path: String },
    /// A path leaves the pack's folder (`..`, an absolute path or a link pointing outside).
    PathOutside { key: String, path: String },
    /// An SVG refers to another file or an address.
    SvgReference { file: String, reference: String },
    /// An SVG contains a script, an event handler or embedded HTML.
    SvgScript { file: String },
    /// A logo is not square (ratio 1 ± 0.05); `None` when the SVG gives no size at all.
    NotSquare { file: String, ratio: Option<f32> },
    /// A font file is not TrueType/OpenType.
    FontUnreadable { file: String },
    /// A font file does not carry the family `design.toml` names.
    FontFamily { file: String, family: String, found: Vec<String> },
    /// Fonts without `fonts/LICENSE.txt`.
    FontLicense,
    /// `preview.png` is larger than 1200 × 800.
    PreviewSize { width: u32, height: u32 },
}

impl fmt::Display for DesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ratio = |r: &f32| format!("{r:.2}");
        let text = match self {
            DesignError::BadId { id } => trf(
                "“{id}” can't be a design folder. Use up to 32 characters a–z, 0–9 and -; “torro” is the standard design.",
                &[("id", id)],
            ),
            DesignError::NotFound { id } => trf("There is no design “{id}”.", &[("id", id)]),
            DesignError::Unreadable { file, reason } => {
                trf("{file} can't be read: {reason}", &[("file", file), ("reason", reason)])
            }
            DesignError::TooLarge { file, limit } => {
                trf("{file} is larger than {limit}.", &[("file", file), ("limit", limit)])
            }
            DesignError::PackTooLarge { limit } => {
                trf("The design is larger than {limit} in total.", &[("limit", limit)])
            }
            DesignError::Syntax { line: Some(line), reason } => {
                trf("design.toml can't be read (line {line}): {reason}", &[("line", line), ("reason", reason)])
            }
            DesignError::Syntax { line: None, reason } => {
                trf("design.toml can't be read: {reason}", &[("reason", reason)])
            }
            DesignError::UnknownKey { key } => trf("Unknown setting “{key}”. Check the spelling.", &[("key", key)]),
            DesignError::MissingKey { key } => trf("“{key}” is missing.", &[("key", key)]),
            DesignError::UnsupportedFormat { found } => {
                trf("Format {found} isn't supported. This version reads format 1.", &[("found", found)])
            }
            DesignError::BadColor { key, value } => {
                trf("“{key}” isn't a color: {value}. Write #RRGGBB or #RRGGBBAA.", &[("key", key), ("value", value)])
            }
            DesignError::TextContrast { fg, on, ratio: r } => trf(
                "{fg} on {on} has a contrast of {ratio}:1; text needs at least 4.5:1.",
                &[("fg", fg), ("on", on), ("ratio", &ratio(r))],
            ),
            DesignError::ControlContrast { fg, on, ratio: r } => trf(
                "{fg} on {on} has a contrast of {ratio}:1; controls need at least 3:1.",
                &[("fg", fg), ("on", on), ("ratio", &ratio(r))],
            ),
            DesignError::ChosenEdge { mode } => trf(
                "colors.{mode}.line_selected must be clearly stronger than line_strong and weaker than focus_ring.",
                &[("mode", mode)],
            ),
            DesignError::AccentContrast { mode, ratio: r } => trf(
                "colors.{mode}.on_accent on accent has a contrast of {ratio}:1; text on the primary button needs at least 4.5:1.",
                &[("mode", mode), ("ratio", &ratio(r))],
            ),
            DesignError::OutOfRange { key, value, min, max } => trf(
                "“{key}” is {value}; allowed are {min} to {max}.",
                &[("key", key), ("value", value), ("min", min), ("max", max)],
            ),
            DesignError::LineHeight { key, line, size } => trf(
                "The line height of “{key}” is {line}; it must be between {size} and twice that.",
                &[("key", key), ("line", line), ("size", size)],
            ),
            DesignError::NotAscending { key } => {
                trf("The values of “{key}” must grow from small to large.", &[("key", key)])
            }
            DesignError::MissingWeight { key, font, weight } => trf(
                "“{key}” uses {weight} of the {font} font, but there is no file for it.",
                &[("key", key), ("weight", weight), ("font", font)],
            ),
            DesignError::WrongFileType { key, path, expected } => trf(
                "{path} in “{key}” has the wrong type. Use {expected}.",
                &[("path", path), ("key", key), ("expected", expected)],
            ),
            DesignError::MissingFile { key, path } => {
                trf("{path} from “{key}” doesn't exist.", &[("path", path), ("key", key)])
            }
            DesignError::PathOutside { key, path } => {
                trf("{path} from “{key}” lies outside the design folder.", &[("path", path), ("key", key)])
            }
            DesignError::SvgReference { file, reference } => trf(
                "{file} refers to {reference}. SVG files may only link to #… and data:.",
                &[("file", file), ("reference", reference)],
            ),
            DesignError::SvgScript { file } => {
                trf("{file} contains a script. SVG files may not run anything.", &[("file", file)])
            }
            DesignError::NotSquare { file, ratio: Some(r) } => trf(
                "{file} isn't square (ratio {ratio}). Logos need a ratio of 1.",
                &[("file", file), ("ratio", &ratio(r))],
            ),
            DesignError::NotSquare { file, ratio: None } => {
                trf("{file} has no size. Give the SVG a viewBox.", &[("file", file)])
            }
            DesignError::FontUnreadable { file } => trf("{file} isn't a TrueType or OpenType font.", &[("file", file)]),
            DesignError::FontFamily { file, family, found } => trf(
                "{file} isn't the family “{family}”; it says {found}.",
                &[("file", file), ("family", family), ("found", &found.join(", "))],
            ),
            DesignError::FontLicense => tr("Fonts need their license in fonts/LICENSE.txt.").to_string(),
            DesignError::PreviewSize { width, height } => trf(
                "preview.png is {width} × {height}; at most 1200 × 800 is allowed.",
                &[("width", width), ("height", height)],
            ),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for DesignError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_the_key_and_the_numbers() {
        let low = DesignError::TextContrast {
            fg: "colors.dark.text_3".into(),
            on: "colors.dark.raised".into(),
            ratio: 2.345,
        };
        let shown = low.to_string();
        assert!(shown.contains("colors.dark.text_3") && shown.contains("2.35"), "{shown}");
        let range = DesignError::OutOfRange { key: "size.row".into(), value: 20.0, min: 32.0, max: 64.0 };
        assert!(range.to_string().contains("size.row"));
        assert_eq!(DesignError::FontLicense.to_string(), tr("Fonts need their license in fonts/LICENSE.txt."));
    }
}
