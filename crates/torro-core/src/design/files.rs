//! The file rules of a design, a font and a colour scheme: paths stay inside the part's folder (also
//! through `..` and links), sizes are checked before anything is read, SVGs hold no references and no
//! scripts, logos are square, fonts are TrueType/OpenType of the named family, `preview.png` is a PNG
//! of at most 1200 × 800. Nothing here interprets more of a file than these checks need; of a font
//! only its family names, its x-height and the advance of its figures are read ([`FontInfo`]).

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::{DesignError, shown};
use crate::i18n::tr;

/// `design.toml`, `font.toml`, `color-scheme.toml`.
pub(super) const TOML_LIMIT: u64 = 64 * 1024;
const SVG_LIMIT: u64 = 256 * 1024;
const PNG_LIMIT: u64 = 2 * 1024 * 1024;
const FONT_LIMIT: u64 = 5 * 1024 * 1024;
const PACK_LIMIT: u64 = 20 * 1024 * 1024;
/// More entries than any folder needs; a folder with more is treated as too large, so walking it
/// stays cheap.
const PACK_ENTRIES: usize = 1000;
const PREVIEW_MAX: (u32, u32) = (1200, 800);

/// The kinds of files a TOML file may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Svg,
    Font,
    Png,
}

impl Kind {
    fn accepts(self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        match self {
            Kind::Svg => ext.as_deref() == Some("svg"),
            Kind::Font => matches!(ext.as_deref(), Some("ttf" | "otf")),
            Kind::Png => ext.as_deref() == Some("png"),
        }
    }

    fn expected(self) -> &'static str {
        match self {
            Kind::Svg => "SVG",
            Kind::Font => "TTF or OTF",
            Kind::Png => "PNG",
        }
    }
}

/// The folder of one design, font or colour scheme, canonical, so links and `..` can be compared
/// against it.
pub(super) struct Root {
    pub(super) path: PathBuf,
}

impl Root {
    /// `dir` resolved; `None` when it does not exist.
    pub(super) fn open(dir: &Path) -> Option<Root> {
        let path = fs::canonicalize(dir).ok()?;
        path.is_dir().then_some(Root { path })
    }

    /// The file `rel` (as written under `key` in the TOML file), absolute, after checking that it
    /// stays inside the folder, exists and is a file with the extension of `kind`. The content is
    /// checked separately ([`Root::check_logo`], [`Root::font_info`]).
    pub(super) fn named(&self, key: &str, rel: &str, kind: Kind) -> Result<PathBuf, Vec<DesignError>> {
        let outside = || vec![DesignError::PathOutside { key: key.to_string(), path: rel.to_string() }];
        let relative = Path::new(rel);
        // `..`, a root or a drive letter never stay inside, even if they would land there again.
        if rel.is_empty() || !relative.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err(outside());
        }
        if !kind.accepts(relative) {
            return Err(vec![DesignError::WrongFileType {
                key: key.to_string(),
                path: rel.to_string(),
                expected: kind.expected(),
            }]);
        }
        let Ok(path) = fs::canonicalize(self.path.join(relative)) else {
            return Err(vec![DesignError::MissingFile { key: key.to_string(), path: rel.to_string() }]);
        };
        // A link inside the folder that points out of it.
        if !path.starts_with(&self.path) {
            return Err(outside());
        }
        if !path.is_file() {
            return Err(vec![DesignError::MissingFile { key: key.to_string(), path: rel.to_string() }]);
        }
        Ok(path)
    }

    /// `path` for messages, relative to the folder.
    pub(super) fn show(&self, path: &Path) -> String {
        shown(&self.path, path)
    }

    /// The TOML file `name` (`design.toml` …) as text, at most [`TOML_LIMIT`].
    pub(super) fn toml(&self, name: &str) -> Result<String, DesignError> {
        let path = self.path.join(name);
        if !path.is_file() {
            return Err(DesignError::MissingFile { key: name.into(), path: name.into() });
        }
        let bytes = self.read(&path, TOML_LIMIT)?;
        String::from_utf8(bytes).map_err(|err| DesignError::Unreadable { file: name.into(), reason: err.to_string() })
    }

    /// The bytes of `path`, refused before reading when the file is larger than `limit`, and
    /// read with that bound in case it grows meanwhile.
    fn read(&self, path: &Path, limit: u64) -> Result<Vec<u8>, DesignError> {
        let file = self.show(path);
        let unreadable = |err: std::io::Error| DesignError::Unreadable { file: file.clone(), reason: err.to_string() };
        let too_large = || DesignError::TooLarge { file: file.clone(), limit: human(limit) };
        let opened = fs::File::open(path).map_err(unreadable)?;
        if opened.metadata().map_err(unreadable)?.len() > limit {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        opened.take(limit + 1).read_to_end(&mut bytes).map_err(unreadable)?;
        if bytes.len() as u64 > limit {
            return Err(too_large());
        }
        Ok(bytes)
    }

    /// The total size of the folder, without following links; a problem when it is over 20 MB or
    /// holds more than [`PACK_ENTRIES`] entries.
    pub(super) fn check_total(&self) -> Option<DesignError> {
        let mut total = 0u64;
        let mut entries = 0usize;
        let mut pending = vec![self.path.clone()];
        while let Some(dir) = pending.pop() {
            let Ok(read) = fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                entries += 1;
                let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
                if meta.is_dir() {
                    pending.push(entry.path());
                } else if meta.is_file() {
                    total += meta.len();
                }
                if total > PACK_LIMIT || entries > PACK_ENTRIES {
                    return Some(DesignError::PackTooLarge { limit: human(PACK_LIMIT) });
                }
            }
        }
        None
    }

    /// The SVG rules for the logo file `path`: size, no references, no scripts, square.
    pub(super) fn check_logo(&self, path: &Path) -> Vec<DesignError> {
        let file = self.show(path);
        let text = match self.read(path, SVG_LIMIT) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(err) => return vec![DesignError::Unreadable { file, reason: err.to_string() }],
            },
            Err(err) => return vec![err],
        };
        svg_problems(&file, &text)
    }

    /// What the rules need of the font at `path`, after its size and format were checked.
    pub(super) fn font_info(&self, path: &Path) -> Result<FontInfo, DesignError> {
        let bytes = self.read(path, FONT_LIMIT)?;
        font_info(&bytes).ok_or_else(|| DesignError::FontUnreadable { file: self.show(path) })
    }

    /// `preview.png` if the pack has one: a PNG of at most 2 MB and 1200 × 800.
    pub(super) fn preview(&self) -> Result<Option<PathBuf>, Vec<DesignError>> {
        if fs::symlink_metadata(self.path.join("preview.png")).is_err() {
            return Ok(None);
        }
        let path = self.named("preview.png", "preview.png", Kind::Png)?;
        let bytes = self.read(&path, PNG_LIMIT).map_err(|err| vec![err])?;
        let Some((width, height)) = png_size(&bytes) else {
            let reason = tr("not a PNG image").to_string();
            return Err(vec![DesignError::Unreadable { file: "preview.png".into(), reason }]);
        };
        if width > PREVIEW_MAX.0 || height > PREVIEW_MAX.1 {
            return Err(vec![DesignError::PreviewSize { width, height }]);
        }
        Ok(Some(path))
    }
}

/// What the rules need of one font file.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FontInfo {
    /// Family, typographic family and WWS family names, sorted, without doubles.
    pub(super) families: Vec<String>,
    /// The x-height in em: OS/2 `sxHeight`, else the top of the "x"; `None` when the font has neither.
    pub(super) x_height: Option<f32>,
    /// The advance of its widest figure 0–9 in em; `None` without figures.
    pub(super) widest_figure: Option<f32>,
}

/// [`FontInfo`] of the font `bytes`; `None` when they are no TrueType/OpenType font.
pub(super) fn font_info(bytes: &[u8]) -> Option<FontInfo> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let ids = [ttf_parser::name_id::FAMILY, ttf_parser::name_id::TYPOGRAPHIC_FAMILY, ttf_parser::name_id::WWS_FAMILY];
    let mut families: Vec<String> = face
        .names()
        .into_iter()
        .filter(|name| ids.contains(&name.name_id))
        .filter_map(|name| name.to_string())
        .collect();
    families.sort();
    families.dedup();
    let em = f32::from(face.units_per_em());
    let glyph = |c: char| face.glyph_index(c);
    let x_height = face
        .x_height()
        .filter(|height| *height > 0)
        .or_else(|| {
            glyph('x').and_then(|g| face.glyph_bounding_box(g)).map(|bounds| bounds.y_max).filter(|top| *top > 0)
        })
        .map(|height| f32::from(height) / em);
    let widest_figure = ('0'..='9')
        .filter_map(|c| glyph(c).and_then(|g| face.glyph_hor_advance(g)))
        .max()
        .map(|advance| f32::from(advance) / em);
    Some(FontInfo { families, x_height, widest_figure })
}

/// The licence file `rel` (`LICENSE.txt`, or `fonts/LICENSE.txt` in an old complete pack) exists
/// inside the folder.
pub(super) fn has_license(root: &Root, rel: &str) -> bool {
    fs::canonicalize(root.path.join(rel)).is_ok_and(|path| path.starts_with(&root.path) && path.is_file())
}

/// "64 KB", "20 MB".
fn human(bytes: u64) -> String {
    if bytes >= 1024 * 1024 { format!("{} MB", bytes / (1024 * 1024)) } else { format!("{} KB", bytes / 1024) }
}

/// Width and height from a PNG's header (signature, then the IHDR chunk).
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let number = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    Some((number(16)?, number(20)?))
}

/// Every SVG rule `text` breaks: it must parse (without a DTD, so no entities), refer to nothing
/// outside itself, run nothing and be square.
fn svg_problems(file: &str, text: &str) -> Vec<DesignError> {
    let options = roxmltree::ParsingOptions { allow_dtd: false, ..roxmltree::ParsingOptions::default() };
    let doc = match roxmltree::Document::parse_with_options(text, options) {
        Ok(doc) => doc,
        Err(err) => return vec![DesignError::Unreadable { file: file.to_string(), reason: err.to_string() }],
    };
    let mut problems = Vec::new();
    let mut scripted = false;
    for node in doc.descendants().filter(|n| n.is_element()) {
        let name = node.tag_name().name();
        let handler = node.attributes().any(|a| a.name().to_ascii_lowercase().starts_with("on"));
        if name.eq_ignore_ascii_case("script") || name == "foreignObject" || handler {
            scripted = true;
        }
        for attr in node.attributes() {
            if attr.name() == "href" && !is_local(attr.value()) {
                problems
                    .push(DesignError::SvgReference { file: file.to_string(), reference: attr.value().to_string() });
            }
            problems.extend(
                css_references(attr.value())
                    .map(|r| DesignError::SvgReference { file: file.to_string(), reference: r }),
            );
        }
        if name == "style" {
            let css: String = node.children().filter_map(|c| c.text()).collect();
            problems.extend(
                css_references(&css).map(|r| DesignError::SvgReference { file: file.to_string(), reference: r }),
            );
        }
    }
    if scripted {
        problems.push(DesignError::SvgScript { file: file.to_string() });
    }
    let ratio = svg_size(doc.root_element()).map(|(w, h)| w / h);
    if !ratio.is_some_and(|r| (r - 1.0).abs() <= 0.05) {
        problems.push(DesignError::NotSquare { file: file.to_string(), ratio });
    }
    problems
}

/// A reference that stays in the file: a fragment (`#id`) or inline data (`data:`).
fn is_local(reference: &str) -> bool {
    let reference = reference.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    reference.starts_with('#') || reference.to_ascii_lowercase().starts_with("data:")
}

/// The targets of `url(…)` and `@import` in CSS (attributes and `<style>`) that are not local.
fn css_references(css: &str) -> impl Iterator<Item = String> + '_ {
    let lower = css.to_ascii_lowercase();
    let imports = lower.contains("@import").then(|| "@import".to_string());
    let urls: Vec<String> = lower
        .match_indices("url(")
        .filter_map(|(at, _)| {
            let rest = &css[at + 4..];
            let target = &rest[..rest.find(')').unwrap_or(rest.len())];
            (!is_local(target)).then(|| target.trim().to_string())
        })
        .collect();
    imports.into_iter().chain(urls)
}

/// Width and height of the root `svg`: its `viewBox`, else `width` and `height` in plain numbers or
/// `px`.
fn svg_size(svg: roxmltree::Node<'_, '_>) -> Option<(f32, f32)> {
    let positive = |v: f32| (v.is_finite() && v > 0.0).then_some(v);
    if let Some(view) = svg.attribute("viewBox") {
        let numbers: Vec<f32> = view
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [_, _, w, h] = numbers[..] {
            return Some((positive(w)?, positive(h)?));
        }
    }
    let length =
        |name| svg.attribute(name).and_then(|v: &str| v.trim().trim_end_matches("px").parse().ok()).and_then(positive);
    Some((length("width")?, length("height")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">"#;

    #[test]
    fn a_plain_square_svg_passes() {
        let svg = format!(
            r##"{SQUARE}<defs><linearGradient id="g"/></defs><rect fill="url(#g)" width="24" height="24"/></svg>"##
        );
        assert_eq!(svg_problems("logo.svg", &svg), Vec::new());
        let sized = r#"<svg xmlns="http://www.w3.org/2000/svg" width="32px" height="31"></svg>"#;
        assert_eq!(svg_problems("logo.svg", sized), Vec::new());
    }

    #[test]
    fn references_outside_the_file_are_refused() {
        for body in [
            r#"<image href="https://example.org/x.png"/>"#,
            r#"<use xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="other.svg#a"/>"#,
            r#"<rect style="fill: url('https://example.org/p')"/>"#,
            r#"<style>@import "theme.css";</style>"#,
        ] {
            let problems = svg_problems("logo.svg", &format!("{SQUARE}{body}</svg>"));
            assert!(matches!(problems[..], [DesignError::SvgReference { .. }]), "{body}: {problems:?}");
        }
        let inline = format!(r#"{SQUARE}<image href="data:image/png;base64,AAAA"/><use href="  #a"/></svg>"#);
        assert_eq!(svg_problems("logo.svg", &inline), Vec::new());
    }

    #[test]
    fn scripts_are_refused() {
        for body in [
            "<script>alert(1)</script>",
            r#"<rect onclick="x()"/>"#,
            r#"<foreignObject><div xmlns="http://www.w3.org/1999/xhtml"/></foreignObject>"#,
        ] {
            let problems = svg_problems("logo.svg", &format!("{SQUARE}{body}</svg>"));
            assert_eq!(problems, [DesignError::SvgScript { file: "logo.svg".into() }], "{body}");
        }
    }

    #[test]
    fn logos_must_be_square_and_have_a_size() {
        let wide = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 24"></svg>"#;
        assert!(matches!(svg_problems("logo.svg", wide)[..], [DesignError::NotSquare { ratio: Some(2.0), .. }]));
        let unsized_svg = r#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert!(matches!(svg_problems("logo.svg", unsized_svg)[..], [DesignError::NotSquare { ratio: None, .. }]));
    }

    #[test]
    fn entities_and_broken_xml_are_unreadable() {
        let dtd =
            r#"<!DOCTYPE svg [<!ENTITY a "aaaa">]><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">&a;</svg>"#;
        assert!(matches!(svg_problems("logo.svg", dtd)[..], [DesignError::Unreadable { .. }]));
        assert!(matches!(svg_problems("logo.svg", "<svg")[..], [DesignError::Unreadable { .. }]));
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        header.extend(1200u32.to_be_bytes());
        header.extend(801u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((1200, 801)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
