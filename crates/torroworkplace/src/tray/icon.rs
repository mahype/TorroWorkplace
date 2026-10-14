//! The Torro tray icon: a 2×2 grid of four small filled squares with slightly rounded corners in
//! Torro red, on a transparent background ([`Look`]). It is the same under every design pack
//! (issue #114): an icon that changed with the design was hard to find again in the menu bar. On
//! macOS it is a template image, so the menu bar tints it for light and dark anyway.
//!
//! The grid is drawn in code, so no image file is needed. Its geometry is in fractions of the icon
//! size, so one description serves every pixmap size, and each pixel is anti-aliased by
//! supersampling. Linux gets ARGB32 pixmaps ([`Look::argb32`], ksni), macOS and Windows an RGBA
//! image ([`Look::rgba`], tray-icon).
//!
//! While the voice listener runs (task C.9) the icon carries a [`Mark`] in its lower right corner,
//! cut free from the symbol: a ring while it waits for the wake word, a solid dot while it records.
//! Shape, not colour, tells them apart, since macOS tints the whole image.

use torro_core::theme::TORRO_RED;
use torro_ipc::VoiceState;

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
/// Colour of the grid and the mark: Torro red, the suite's mark, whatever design is chosen.
const COLOR: [u8; 3] = [TORRO_RED.0, TORRO_RED.1, TORRO_RED.2];
/// Samples per pixel side. 4×4 gives 16 coverage levels for the edges.
const SAMPLES: u32 = 4;
/// Centre of the voice mark, as a fraction of the icon size (both axes).
const MARK_CENTRE: f32 = 0.78;
/// Radius of the voice mark.
const MARK_RADIUS: f32 = 0.2;
/// Inner radius of the ring while listening.
const MARK_HOLE: f32 = 0.11;
/// Radius of the space cut free around the mark, so it stands apart from the symbol.
const MARK_CLEARING: f32 = 0.29;

/// What the icon marks besides the symbol: the voice listener (task C.9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mark {
    /// Voice is off: the symbol alone.
    #[default]
    None,
    /// Waiting for the wake word, or answering: a ring.
    Listening,
    /// Recording what is said: a solid dot.
    Recording,
}

impl Mark {
    /// The mark for what the listener does.
    pub fn of(state: VoiceState) -> Mark {
        match state {
            VoiceState::Off => Mark::None,
            VoiceState::Listening | VoiceState::Answering => Mark::Listening,
            VoiceState::Recording => Mark::Recording,
        }
    }

    /// Whether a point (in fractions of the icon size) lies on the mark.
    fn covers(self, px: f32, py: f32) -> bool {
        let distance = ((px - MARK_CENTRE).powi(2) + (py - MARK_CENTRE).powi(2)).sqrt();
        match self {
            Mark::None => false,
            Mark::Listening => (MARK_HOLE..=MARK_RADIUS).contains(&distance),
            Mark::Recording => distance <= MARK_RADIUS,
        }
    }

    /// Whether a point lies in the space cut free around the mark.
    fn clears(self, px: f32, py: f32) -> bool {
        self != Mark::None && ((px - MARK_CENTRE).powi(2) + (py - MARK_CENTRE).powi(2)).sqrt() <= MARK_CLEARING
    }
}

/// What the tray icon looks like: always the grid of four rounded squares in Torro red (issue
/// #114), whatever design pack is chosen, so the icon stays easy to find in the menu bar. Only the
/// voice mark changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    /// The voice mark on top ([`Look::marked`]).
    mark: Mark,
}

impl Look {
    /// The icon without a mark: the grid in Torro red.
    pub fn standard() -> Look {
        Look::default()
    }

    /// The icon with `mark` in its corner.
    pub fn marked(self, mark: Mark) -> Look {
        Look { mark }
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

    /// The icon as `size` × `size` straight (not premultiplied) RGBA pixels, row by row.
    fn pixels(&self, size: u32) -> Vec<[u8; 4]> {
        let [r, g, b] = COLOR;
        let symbol: Vec<[u8; 4]> = (0..size * size).map(|i| [r, g, b, coverage(i % size, i / size, size)]).collect();
        if self.mark == Mark::None {
            return symbol;
        }
        symbol
            .into_iter()
            .enumerate()
            .map(|(i, pixel)| self.with_mark(pixel, i as u32 % size, i as u32 / size, size))
            .collect()
    }

    /// `pixel` at `(x, y)` with the space around the mark cut free and the mark drawn over it in
    /// the symbol's colour (straight alpha, "over").
    fn with_mark(&self, pixel: [u8; 4], x: u32, y: u32, size: u32) -> [u8; 4] {
        let mark = self.mark;
        let cleared = 1.0 - share(x, y, size, |px, py| mark.clears(px, py) && !mark.covers(px, py));
        let dot = share(x, y, size, |px, py| mark.covers(px, py));
        let below = f32::from(pixel[3]) / 255.0 * cleared;
        let alpha = dot + below * (1.0 - dot);
        if alpha <= 0.0 {
            return [0, 0, 0, 0];
        }
        let blend = |top: u8, under: u8| {
            ((f32::from(top) * dot + f32::from(under) * below * (1.0 - dot)) / alpha).round().clamp(0.0, 255.0) as u8
        };
        let [r, g, b] = COLOR;
        [blend(r, pixel[0]), blend(g, pixel[1]), blend(b, pixel[2]), (alpha * 255.0).round() as u8]
    }
}

/// The share (0 to 1) of the samples of pixel `(x, y)` for which `inside` holds, the point given in
/// fractions of the icon size.
fn share(x: u32, y: u32, size: u32, inside: impl Fn(f32, f32) -> bool) -> f32 {
    let size = size as f32;
    let mut count = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size;
            let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size;
            if inside(px, py) {
                count += 1;
            }
        }
    }
    count as f32 / (SAMPLES * SAMPLES) as f32
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
    use super::*;

    /// The pixel at `(x, y)` as `[A, R, G, B]`.
    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    fn argb32(size: u32) -> Vec<u8> {
        Look::standard().argb32(size)
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

    /// Issue #114: the icon has no input but the voice mark, so every design pack gets the same
    /// pixels – the grid in Torro red, not the pack's accent or symbol.
    #[test]
    fn the_icon_is_the_red_grid_whatever_the_design() {
        assert_eq!(Look::standard(), Look::default());
        for size in SIZES {
            let data = argb32(size);
            let (near, far) = (size / 4, size - 1 - size / 4);
            for (x, y) in [(near, near), (far, near), (near, far), (far, far)] {
                assert_eq!(pixel(&data, size, x, y), [255, TORRO_RED.0, TORRO_RED.1, TORRO_RED.2], "tile, size {size}");
            }
            assert!(pixel(&data, size, size / 2, near)[0] < 128, "gap between the columns, size {size}");
            assert!(pixel(&data, size, near, size / 2)[0] < 128, "gap between the rows, size {size}");
        }
    }

    /// Task C.9: a ring while listening, a dot while recording, cut free from the grid; off, the
    /// icon is as before.
    #[test]
    fn the_voice_mark_shows_listening_and_recording() {
        assert_eq!(Look::standard().marked(Mark::of(VoiceState::Off)).argb32(32), argb32(32));
        let size = 32;
        let centre = (MARK_CENTRE * size as f32) as u32;
        let ring = (MARK_CENTRE * size as f32 + (MARK_HOLE + MARK_RADIUS) / 2.0 * size as f32) as u32;
        let clearing = ((MARK_CENTRE - (MARK_RADIUS + MARK_CLEARING) / 2.0) * size as f32) as u32;
        let alpha = |look: &Look, x, y| pixel(&look.argb32(size), size, x, y)[0];
        let listening = Look::standard().marked(Mark::of(VoiceState::Listening));
        assert_eq!(alpha(&listening, centre, centre), 0, "the ring's hole");
        assert_eq!(alpha(&listening, ring, centre), 255, "the ring");
        assert_eq!(alpha(&listening, clearing, centre), 0, "cut free from the grid");
        assert_eq!(Mark::of(VoiceState::Answering), Mark::Listening);
        let recording = Look::standard().marked(Mark::of(VoiceState::Recording));
        assert_eq!(pixel(&recording.argb32(size), size, centre, centre), [255, TORRO_RED.0, TORRO_RED.1, TORRO_RED.2]);
        assert_eq!(alpha(&recording, size / 4, size / 4), 255, "the rest of the grid stays");
    }
}
