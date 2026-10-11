//! The Torro tray icon: a 2×2 grid of four rounded squares in `theme::TORRO_RED`, on a
//! transparent background. It is drawn in code, so no image file is needed. The geometry is
//! in fractions of the icon size, so one description serves every pixmap size, and each
//! pixel is anti-aliased by supersampling.

use torro_core::theme::TORRO_RED;

/// Pixmap sizes handed to the tray. The desktop picks the one that fits its panel.
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

/// Returns the ARGB32 pixmap of the icon, `size` × `size` pixels, as ksni expects it: four
/// bytes per pixel (A, R, G, B) in network byte order, row by row from the top left.
pub fn argb32(size: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let alpha = coverage(x, y, size);
            data.extend_from_slice(&[alpha, TORRO_RED.0, TORRO_RED.1, TORRO_RED.2]);
        }
    }
    data
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
}
