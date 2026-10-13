//! The Torro tray icon. It follows the chosen design pack (task L.3, [`Look`]): the pack's own
//! symbol (`logo.tray`, an SVG rastered with resvg for each size), or else the suite's symbol – a
//! 2×2 grid of four rounded squares – in the pack's accent colour (`theme::TORRO_RED` in the
//! standard), on a transparent background.
//!
//! The grid is drawn in code, so no image file is needed. Its geometry is in fractions of the icon
//! size, so one description serves every pixmap size, and each pixel is anti-aliased by
//! supersampling. Linux gets ARGB32 pixmaps ([`Look::argb32`], ksni), macOS and Windows an RGBA
//! image ([`Look::rgba`], tray-icon).

use std::path::Path;

use resvg::{tiny_skia, usvg};
use torro_core::design::{self, Design, Rgba, STANDARD_ID};

/// Pixmap sizes handed to the tray. The desktop picks the one that fits its panel.
#[cfg(any(target_os = "linux", test))]
pub const SIZES: [u32; 4] = [16, 22, 32, 48];

/// Distance from the icon edge to the grid, as a fraction of the icon size.
const MARGIN: f32 = 0.06;
/// Gap between two neighbouring tiles.
const GAP: f32 = 0.08;
/// Corner radius of one tile.
const RADIUS: f32 = 0.12;
/// Side of one tile: the grid spans the icon minus the margins on both sides.
const TILE: f32 = (1.0 - 2.0 * MARGIN - GAP) / 2.0;
/// Start of the second tile row and column.
const SECOND: f32 = MARGIN + TILE + GAP;
/// Top-left corners of the four tiles.
const TILES: [(f32, f32); 4] = [(MARGIN, MARGIN), (SECOND, MARGIN), (MARGIN, SECOND), (SECOND, SECOND)];
/// Samples per pixel side. 4×4 gives 16 coverage levels for the edges.
const SAMPLES: u32 = 4;

/// What the tray icon looks like under one design: the pack's symbol, or the grid in its accent.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The source of the pack's `logo.tray`, already checked when the pack was loaded (size, no
    /// outside references, no scripts); `None` draws the grid.
    svg: Option<Vec<u8>>,
    /// The colour of the grid: the design's dark accent. The tray has no light or dark of its
    /// own to follow (macOS tints the image anyway, it is a template there).
    color: [u8; 3],
}

impl Look {
    /// The icon of the standard design: the grid in Torro red.
    pub fn standard() -> Look {
        Look::of(&Design::standard())
    }

    /// The icon of `design`. A symbol file that cannot be read any more (the folder changed since
    /// the pack was checked) gives the grid.
    pub fn of(design: &Design) -> Look {
        let svg = design.logo.tray.as_deref().and_then(|path| std::fs::read(path).ok());
        Look { svg, color: rgb8(design.colors.dark.accent) }
    }

    /// The icon for the design the settings name (`Settings::design`, as `Snapshot::design`
    /// carries it), read from the user's designs folder. Empty, gone or broken: the standard's,
    /// like the apps draw then.
    pub fn for_design(id: &str) -> Look {
        match design::dir() {
            Some(dir) => Look::for_design_in(&dir, id),
            None => Look::standard(),
        }
    }

    /// [`Look::for_design`] with the packs in `dir`.
    pub fn for_design_in(dir: &Path, id: &str) -> Look {
        if id.is_empty() || id == STANDARD_ID {
            return Look::standard();
        }
        design::load(dir, id).map(|design| Look::of(&design)).unwrap_or_else(|_| Look::standard())
    }

    /// The ARGB32 pixmap of the icon, `size` × `size` pixels, as ksni expects it: four bytes per
    /// pixel (A, R, G, B) in network byte order, row by row from the top left.
    #[cfg(any(target_os = "linux", test))]
    pub fn argb32(&self, size: u32) -> Vec<u8> {
        self.pixels(size).into_iter().flat_map(|[r, g, b, a]| [a, r, g, b]).collect()
    }

    /// The icon as RGBA bytes (R, G, B, A per pixel, row by row from the top left) on a
    /// transparent canvas of `size + 2 * padding` pixels a side, the icon `size` pixels wide in the
    /// middle. The padding lets a system that fills its whole tray slot with the image (macOS, 22 pt
    /// high) still show the symbol at the size of its own symbols.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn rgba(&self, size: u32, padding: u32) -> Vec<u8> {
        let canvas = size + 2 * padding;
        let icon = self.pixels(size);
        let mut data = Vec::with_capacity((canvas * canvas * 4) as usize);
        for y in 0..canvas {
            for x in 0..canvas {
                let inside = (padding..padding + size).contains(&x) && (padding..padding + size).contains(&y);
                let pixel = match inside {
                    true => icon[((y - padding) * size + x - padding) as usize],
                    false => [0, 0, 0, 0],
                };
                data.extend_from_slice(&pixel);
            }
        }
        data
    }

    /// The icon as `size` × `size` straight (not premultiplied) RGBA pixels, row by row. An SVG
    /// that does not render falls back to the grid, so the tray never shows an empty slot.
    fn pixels(&self, size: u32) -> Vec<[u8; 4]> {
        if let Some(pixels) = self.svg.as_deref().and_then(|svg| svg_pixels(svg, size)) {
            return pixels;
        }
        let [r, g, b] = self.color;
        (0..size * size).map(|i| [r, g, b, coverage(i % size, i / size, size)]).collect()
    }
}

/// `color` as 8-bit RGB, rounded the way Iced turns its colours into bytes.
fn rgb8(color: Rgba) -> [u8; 3] {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(color.r), byte(color.g), byte(color.b)]
}

/// `svg` rastered to `size` × `size`, scaled to fit and centred; `None` when it cannot be parsed or
/// has no area. resvg draws premultiplied pixels; the trays want them straight.
fn svg_pixels(svg: &[u8], size: u32) -> Option<Vec<[u8; 4]>> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / width.max(height);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (dx, dy) = ((size as f32 - width * scale) / 2.0, (size as f32 - height * scale) / 2.0);
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(
        pixmap
            .pixels()
            .iter()
            .map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect(),
    )
}

/// Alpha of one pixel: the share of its samples that lie inside the grid.
fn coverage(x: u32, y: u32, size: u32) -> u8 {
    let size = size as f32;
    let mut inside = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size;
            let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size;
            if in_grid(px, py) {
                inside += 1;
            }
        }
    }
    (inside * 255 / (SAMPLES * SAMPLES)) as u8
}

/// Whether a point (in fractions of the icon size) lies on one of the four tiles.
fn in_grid(px: f32, py: f32) -> bool {
    TILES.iter().any(|&(x0, y0)| in_rounded_square(px, py, x0, y0))
}

/// Whether a point lies in the tile whose top-left corner is `(x0, y0)`, with rounded corners.
fn in_rounded_square(px: f32, py: f32, x0: f32, y0: f32) -> bool {
    let x1 = x0 + TILE;
    let y1 = y0 + TILE;
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    // The nearest point of the rectangle shrunk by the radius. Inside the tile means: within
    // the radius of that point. Outside the corner squares the distance is zero.
    let cx = px.clamp(x0 + RADIUS, x1 - RADIUS);
    let cy = py.clamp(y0 + RADIUS, y1 - RADIUS);
    let (dx, dy) = (px - cx, py - cy);
    dx * dx + dy * dy <= RADIUS * RADIUS
}

#[cfg(test)]
mod tests {
    use torro_core::theme::TORRO_RED;

    use super::*;

    /// The pixel at `(x, y)` as `[A, R, G, B]`.
    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    fn argb32(size: u32) -> Vec<u8> {
        Look::standard().argb32(size)
    }

    /// A designs folder with the packs `teal` (only an accent: the grid in teal), `round` (its own
    /// tray symbol: a blue circle) and `pale` (refused).
    fn designs() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let packs = [
            (
                "teal",
                "format = 1\n[about]\nname = \"Teal\"\nversion = \"1.0.0\"\n[colors.dark]\naccent = \"#0E7C7B\"\n",
            ),
            ("round", "format = 1\n[about]\nname = \"Round\"\nversion = \"1.0.0\"\n[logo]\ntray = \"tray.svg\"\n"),
            ("pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n"),
        ];
        for (id, toml) in packs {
            std::fs::create_dir_all(dir.path().join(id)).expect("pack folder");
            std::fs::write(dir.path().join(id).join("design.toml"), toml).expect("design.toml");
        }
        let circle = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="5" fill="#2050F0"/></svg>"##;
        std::fs::write(dir.path().join("round").join("tray.svg"), circle).expect("tray.svg");
        dir
    }

    #[test]
    fn rgba_has_the_padding_transparent_and_the_grid_in_the_middle() {
        let (size, padding) = (36, 4);
        let canvas = size + 2 * padding;
        let data = Look::standard().rgba(size, padding);
        assert_eq!(data.len(), (canvas * canvas * 4) as usize);
        let alpha = |x: u32, y: u32| data[((y * canvas + x) * 4 + 3) as usize];
        assert_eq!(alpha(padding - 1, padding + size / 4), 0, "padding");
        assert_eq!(alpha(canvas / 2, canvas / 2), 0, "gap between the tiles");
        let p = padding + size / 4;
        let i = ((p * canvas + p) * 4) as usize;
        assert_eq!(&data[i..i + 4], &[TORRO_RED.0, TORRO_RED.1, TORRO_RED.2, 255], "middle of a tile");
    }

    #[test]
    fn rgba_without_padding_matches_the_pixmap() {
        let size = 22;
        let (argb, rgba) = (argb32(size), Look::standard().rgba(size, 0));
        for (a, r) in argb.chunks(4).zip(rgba.chunks(4)) {
            assert_eq!([a[1], a[2], a[3], a[0]], [r[0], r[1], r[2], r[3]]);
        }
    }

    #[test]
    fn every_size_has_four_bytes_per_pixel() {
        for size in SIZES {
            assert_eq!(argb32(size).len(), (size * size * 4) as usize, "size {size}");
        }
    }

    #[test]
    fn corner_pixel_is_transparent() {
        for size in SIZES {
            let data = argb32(size);
            assert_eq!(pixel(&data, size, 0, 0)[0], 0, "size {size}");
        }
    }

    #[test]
    fn gap_between_the_tiles_is_transparent() {
        for size in SIZES {
            let data = argb32(size);
            let middle = size / 2;
            assert_eq!(pixel(&data, size, middle, middle)[0], 0, "size {size}");
        }
    }

    #[test]
    fn middle_of_top_left_tile_is_opaque_torro_red() {
        for size in SIZES {
            let data = argb32(size);
            let p = size / 4;
            assert_eq!(pixel(&data, size, p, p), [255, TORRO_RED.0, TORRO_RED.1, TORRO_RED.2], "size {size}");
        }
    }

    #[test]
    fn a_pack_without_a_symbol_gets_the_grid_in_its_accent() {
        let dir = designs();
        let look = Look::for_design_in(dir.path(), "teal");
        for size in SIZES {
            let data = look.argb32(size);
            let p = size / 4;
            assert_eq!(pixel(&data, size, p, p), [255, 0x0E, 0x7C, 0x7B], "size {size}");
            assert_eq!(pixel(&data, size, size / 2, size / 2)[0], 0, "the grid's gap, size {size}");
        }
    }

    #[test]
    fn a_pack_symbol_is_rastered_for_every_size() {
        let dir = designs();
        let look = Look::for_design_in(dir.path(), "round");
        assert_ne!(look, Look::standard());
        for size in SIZES {
            let data = look.argb32(size);
            assert_eq!(data.len(), (size * size * 4) as usize, "size {size}");
            let [alpha, r, g, b] = pixel(&data, size, size / 2, size / 2);
            assert_eq!(alpha, 255, "the middle of the circle is drawn (where the grid has its gap), size {size}");
            assert_eq!([r, g, b], [0x20, 0x50, 0xF0], "in the symbol's own colour, size {size}");
            assert_eq!(pixel(&data, size, 0, 0)[0], 0, "a circle leaves the corner empty, size {size}");
        }
        let padded = look.rgba(36, 4);
        assert_eq!(padded[(4 * 44 + 4) * 4 + 3], 0, "the corner of the symbol's square stays clear");
    }

    #[test]
    fn a_broken_missing_or_unset_design_gets_the_standard_icon() {
        let dir = designs();
        for id in ["pale", "gone", "", "torro"] {
            assert_eq!(Look::for_design_in(dir.path(), id), Look::standard(), "{id:?}");
        }
    }

    #[test]
    fn an_svg_that_does_not_render_falls_back_to_the_grid() {
        let broken = Look { svg: Some(b"<svg".to_vec()), color: [TORRO_RED.0, TORRO_RED.1, TORRO_RED.2] };
        assert_eq!(broken.argb32(22), argb32(22));
    }
}
