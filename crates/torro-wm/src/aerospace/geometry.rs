//! Window and screen geometry, which AeroSpace's command line does not print.
//!
//! Task X.6 in `docs/tasks/plattformen.md`. Templates infer the layout from window rectangles
//! (`torro-setups`, `layout::infer_tree`), so without them every AeroSpace template would be a
//! flat list. macOS hands out window bounds without any permission through CoreGraphics
//! (`CGWindowListCopyWindowInfo`), and AeroSpace window ids are those window numbers. The
//! crate forbids `unsafe` and has no Objective-C bindings, so a short JavaScript for
//! Automation run by `osascript` reads them (about 0.15 s) and prints one JSON line,
//! [`SCRIPT`]. Screen frames come from `NSScreen.screens` in the same run.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::{Rect, SplitDirection, WmError};

/// Where macOS keeps `osascript`.
pub const OSASCRIPT: &str = "/usr/bin/osascript";

/// Prints `{"windows":[[id,x,y,w,h],…],"screens":[[x,y,w,h,scale],…]}`. Window bounds are in
/// CoreGraphics coordinates (origin top left of the main screen); screen frames are Cocoa
/// frames (origin bottom left of the main screen) and are converted by [`parse`]. Only
/// windows on layer 0 (normal app windows) are printed.
pub const SCRIPT: &str = r#"ObjC.import("CoreGraphics"); ObjC.import("AppKit");
var list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionAll, 0))) || [];
var windows = [];
list.forEach(function (w) {
  var b = w.kCGWindowBounds;
  if (w.kCGWindowLayer === 0 && b) windows.push([w.kCGWindowNumber, b.X, b.Y, b.Width, b.Height]);
});
var screens = [];
var all = $.NSScreen.screens;
for (var i = 0; i < all.count; i++) {
  var s = all.objectAtIndex(i); var f = s.frame;
  screens.push([f.origin.x, f.origin.y, f.size.width, f.size.height, s.backingScaleFactor]);
}
JSON.stringify({ windows: windows, screens: screens });"#;

/// Rectangles by window number, and the screens in `NSScreen.screens` order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    /// Desktop coordinates, logical pixels, origin top left of the main screen.
    pub windows: HashMap<u32, Rect>,
    pub screens: Vec<Screen>,
}

/// One screen in the same coordinates as the windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub rect: Rect,
    pub scale: f32,
}

#[derive(Deserialize)]
struct RawGeometry {
    #[serde(default)]
    windows: Vec<[f64; 5]>,
    #[serde(default)]
    screens: Vec<[f64; 5]>,
}

/// Runs [`SCRIPT`] with `osascript` and parses its answer.
pub async fn read(osascript: &Path) -> Result<Geometry, WmError> {
    let answer = super::cli::run(osascript, &["-l", "JavaScript", "-e", SCRIPT]).await?;
    parse(&answer)
}

/// Reads the script's answer. Screen frames are flipped to a top-left origin: the first screen
/// is the main screen, whose height is the reference.
pub fn parse(json: &str) -> Result<Geometry, WmError> {
    let raw: RawGeometry =
        serde_json::from_str(json).map_err(|err| WmError::Protocol(format!("window geometry: {err}")))?;
    let windows = raw
        .windows
        .iter()
        .filter_map(|&[id, x, y, width, height]| {
            let id = u32::try_from(id as i64).ok()?;
            Some((id, rect(x, y, width, height)))
        })
        .collect();
    let main_height = raw.screens.first().map_or(0.0, |screen| screen[3]);
    let screens = raw
        .screens
        .iter()
        .map(|&[x, y, width, height, scale]| Screen {
            rect: rect(x, main_height - (y + height), width, height),
            scale: scale as f32,
        })
        .collect();
    Ok(Geometry { windows, screens })
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect { x: x.round() as i32, y: y.round() as i32, width: width.round() as i32, height: height.round() as i32 }
}

/// The `resize` that gives `sibling` its share `ratio` of the pair it forms with `window`, as
/// `("width" | "height", pixels)` for `aerospace resize --window-id <sibling>`.
///
/// `ratio` is measured the way templates store it: where `window` starts within the pair's
/// extent. The gap between the two stays as it is. `None` when the two do not sit next to each
/// other in `direction` (sibling first) or the size would not be positive.
pub fn ratio_resize(direction: SplitDirection, ratio: f32, sibling: Rect, window: Rect) -> Option<(&'static str, i32)> {
    let (dimension, start, size, window_start, window_size) = match direction {
        SplitDirection::LeftRight => ("width", sibling.x, sibling.width, window.x, window.width),
        SplitDirection::TopBottom => ("height", sibling.y, sibling.height, window.y, window.height),
    };
    if window_start <= start {
        return None;
    }
    let total = window_start + window_size - start;
    let gap = (window_start - (start + size)).max(0);
    let target = (f64::from(ratio) * f64::from(total)).round() as i32 - gap;
    (target > 0 && target < total - gap).then_some((dimension, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOMETRY: &str = include_str!("../../tests/fixtures/aerospace-geometry.json");

    #[test]
    fn windows_by_number_and_screens_with_top_left_origin() {
        let geometry = parse(GEOMETRY).expect("fixture parses");
        assert_eq!(geometry.windows.get(&102), Some(&Rect { x: 872, y: 41, width: 848, height: 530 }));
        assert_eq!(geometry.windows.len(), 6);
        assert_eq!(
            geometry.screens,
            vec![
                Screen { rect: Rect { x: 0, y: 0, width: 1728, height: 1117 }, scale: 2.0 },
                // Cocoa y -323 with height 1440 below a 1117 high main screen: top edge at 0.
                Screen { rect: Rect { x: 1728, y: 0, width: 2560, height: 1440 }, scale: 1.0 },
            ]
        );
    }

    #[test]
    fn fractional_values_are_rounded_and_garbage_is_a_protocol_error() {
        let geometry = parse(r#"{"windows":[[7,10.4,20.6,100.5,50]],"screens":[]}"#).expect("parses");
        assert_eq!(geometry.windows.get(&7), Some(&Rect { x: 10, y: 21, width: 101, height: 50 }));
        assert!(matches!(parse("osascript: error"), Err(WmError::Protocol(_))));
    }

    #[test]
    fn resize_keeps_the_gap_and_sets_the_share() {
        let sibling = Rect { x: 8, y: 41, width: 856, height: 1068 };
        let window = Rect { x: 872, y: 41, width: 848, height: 1068 };
        // Pair extent 1712, gap 8: a third means the window starts at 571, sibling 563 wide.
        assert_eq!(ratio_resize(SplitDirection::LeftRight, 1.0 / 3.0, sibling, window), Some(("width", 563)));

        let top = Rect { x: 0, y: 0, width: 800, height: 400 };
        let bottom = Rect { x: 0, y: 410, width: 800, height: 390 };
        assert_eq!(ratio_resize(SplitDirection::TopBottom, 0.75, top, bottom), Some(("height", 590)));
    }

    #[test]
    fn no_resize_when_the_window_is_not_after_the_sibling() {
        let sibling = Rect { x: 872, y: 41, width: 848, height: 1068 };
        let window = Rect { x: 8, y: 41, width: 856, height: 1068 };
        assert_eq!(ratio_resize(SplitDirection::LeftRight, 0.5, sibling, window), None);
        assert_eq!(ratio_resize(SplitDirection::TopBottom, 0.5, sibling, sibling), None);
    }
}
