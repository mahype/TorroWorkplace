//! Tests of [`Content`] without a window: the fixtures draw, the width changes the layout, dark recolours
//! without touching pictures, links are hit, clicks and selections work. No network: web pictures are off.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::recolor::{MIN_TEXT_CONTRAST, contrast, lightness};

include!("../../tests/fixtures/long_mail.rs");

/// The inline pictures of the fixtures, as the app would hand them out over `mail.get_attachment`.
struct FixtureParts;

impl CidSource for FixtureParts {
    fn picture(&self, content_id: &str) -> Option<Vec<u8>> {
        if content_id.contains(['/', '\\']) || content_id.starts_with('.') {
            return None;
        }
        std::fs::read(fixtures().join("cid").join(format!("{content_id}.png"))).ok()
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(format!("{name}.html"))).expect("fixture")
}

fn content(html: &str) -> Content {
    Content::offline(html, Some(Arc::new(FixtureParts)))
}

/// Waits until every picture of `content` is answered (they come from files, so this is quick).
fn settle(content: &Content) {
    let started = Instant::now();
    while content.is_loading() && started.elapsed() < Duration::from_secs(10) {
        content.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!content.is_loading(), "pictures did not arrive");
}

fn color(pixel: [u8; 4]) -> peniko::Color {
    peniko::Color::from_rgb8(pixel[0], pixel[1], pixel[2])
}

fn background(scheme: Scheme) -> [u8; 4] {
    let c = surface(scheme);
    let byte = |v: f32| (v * 255.0).round() as u8;
    [byte(c.r), byte(c.g), byte(c.b), 255]
}

/// How many pixels differ from the surface: the mail drew something.
fn painted(snapshot: &Snapshot, scheme: Scheme) -> usize {
    let surface = background(scheme);
    snapshot.rgba.chunks(4).filter(|pixel| pixel.iter().zip(surface).any(|(a, b)| a.abs_diff(b) > 8)).count()
}

const FIXTURES: [&str; 5] = ["newsletter", "invoice", "outlook", "styled", "plain"];

#[test]
fn every_fixture_draws_at_both_widths_in_both_schemes() {
    for name in FIXTURES {
        let content = content(&fixture(name));
        settle(&content);
        for width in [360.0, 700.0] {
            for scheme in [Scheme::Light, Scheme::Dark] {
                let height = content.height(width, 1.0, scheme);
                assert!(height > 40.0, "{name} at {width}: {height}");
                let snapshot = content.snapshot(width, 1.0, scheme, 0.0, height);
                assert_eq!(snapshot.width, width as u32);
                assert_eq!(snapshot.height, height.ceil() as u32, "{name} at {width}");
                assert_eq!(snapshot.rgba.len(), (snapshot.width * snapshot.height * 4) as usize);
                let painted = painted(&snapshot, scheme);
                assert!(painted > 500, "{name} at {width} {scheme:?}: only {painted} pixels painted");
            }
        }
    }
}

#[test]
fn the_long_mail_draws_in_parts() {
    let html = long_mail();
    assert!(html.len() > 1024 * 1024);
    let content = content(&html);
    let height = content.height(700.0, 1.0, Scheme::Light);
    assert!(height > 50_000.0, "{height}");
    let top = content.snapshot(700.0, 1.0, Scheme::Light, 0.0, 800.0);
    let middle = content.snapshot(700.0, 1.0, Scheme::Light, (height / 2.0).floor(), 800.0);
    let end = content.snapshot(700.0, 1.0, Scheme::Light, height.ceil() - 300.0, 800.0);
    assert_eq!((top.height, middle.height), (800, 800));
    assert_eq!(end.height, 300, "cut at the end of the mail");
    assert!(painted(&middle, Scheme::Light) > 1000);
    assert_ne!(top.rgba, middle.rgba);
}

#[test]
fn the_width_changes_the_layout() {
    for name in ["newsletter", "invoice", "styled", "long"] {
        let content = content(&if name == "long" { long_mail() } else { fixture(name) });
        settle(&content);
        let narrow = content.height(360.0, 1.0, Scheme::Light);
        let wide = content.height(700.0, 1.0, Scheme::Light);
        assert!((narrow - wide).abs() > 20.0, "{name}: {narrow} at 360, {wide} at 700");
    }
    // Text reflows: narrower is taller.
    let invoice = content(&fixture("invoice"));
    assert!(invoice.height(360.0, 1.0, Scheme::Light) > invoice.height(700.0, 1.0, Scheme::Light));
}

#[test]
fn a_mail_wider_than_the_reader_is_zoomed_out_not_cut() {
    let content = content(&fixture("newsletter"));
    settle(&content);
    content.height(700.0, 1.0, Scheme::Light);
    assert_eq!(content.inner.borrow().zoom, 1.0, "600 px fit into 700");
    content.height(360.0, 1.0, Scheme::Light);
    let inner = content.inner.borrow();
    assert!(inner.zoom < 0.7, "zoom {}", inner.zoom);
    let shown = content_width(&inner.doc) * inner.zoom;
    assert!(shown <= 361.0, "{shown} px wide in a 360-px reader");
    drop(inner);
    // A link keeps working at the zoom: "full story with all setups" sits inside the mail.
    let links = (0..360).step_by(4).flat_map(|x| (0..600).step_by(4).map(move |y| (x, y)));
    let found = links.filter_map(|(x, y)| content.link_at(Point::new(x as f32, y as f32))).count();
    assert!(found > 0, "no link found at the zoom");
}

#[test]
fn the_media_query_follows_the_width() {
    let content = content(&fixture("styled"));
    settle(&content);
    content.height(360.0, 1.0, Scheme::Light);
    assert_eq!((shown(&content, "narrow-only"), shown(&content, "wide-only")), (true, false), "at 360");
    content.height(700.0, 1.0, Scheme::Light);
    assert_eq!((shown(&content, "narrow-only"), shown(&content, "wide-only")), (false, true), "at 700");
}

/// Whether the element with `class` takes up room in the layout (`display: none` takes none).
fn shown(content: &Content, class: &str) -> bool {
    let inner = content.inner.borrow();
    let doc = &inner.doc;
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else { continue };
        if node.attr(local_name!("class")).is_some_and(|classes| classes.split_whitespace().any(|c| c == class)) {
            return node.final_layout().size.height > 0.0;
        }
        stack.extend(node.children.iter().copied());
    }
    false
}

/// White box with black text, a flat picture above it: what dark mode must do to each.
const RECOLOR: &str = "<img src=\"cid:swatch@torro.example\" style=\"display:block;width:40px;height:40px\">\
     <div style=\"background:#ffffff;color:#000000;padding:20px;font-size:20px\">Dark text on white</div>";

#[test]
fn dark_turns_white_dark_and_black_light_and_leaves_pictures() {
    let content = content(RECOLOR);
    settle(&content);
    let light = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 120.0);
    let dark = content.snapshot(300.0, 1.0, Scheme::Dark, 0.0, 120.0);
    // The picture sits at 8..48 (body margin), the box below it from y = 48.
    let swatch = [51, 102, 204, 255];
    assert_eq!(light.pixel(28, 28), Some(swatch), "picture in light");
    assert_eq!(dark.pixel(28, 28), Some(swatch), "picture unchanged in dark");
    let paper = |snapshot: &Snapshot| color(snapshot.pixel(12, 52).expect("pixel"));
    assert!(lightness(paper(&light)) > 0.99, "white stays white in light");
    assert!(lightness(paper(&dark)) < 0.3, "white turns dark: {:?}", paper(&dark));
    let ink = |snapshot: &Snapshot| strongest(snapshot, 28, 100, 48..100);
    assert!(lightness(ink(&light)) < 0.2, "black stays black in light");
    let text = ink(&dark);
    assert!(lightness(text) > 0.8, "black turns light: {text:?}");
    assert!(contrast(text, paper(&dark)) >= MIN_TEXT_CONTRAST, "{:.2}", contrast(text, paper(&dark)));
}

/// A mail that colours its `body` and keeps the browser's margin of 8 around it.
const COLORED_BODY: &str = "<body style=\"background:#f0e0c0\"><div style=\"height:60px\"></div></body>";

#[test]
fn the_bodys_background_fills_the_whole_view_as_a_browsers_canvas() {
    let content = content(COLORED_BODY);
    for scheme in [Scheme::Light, Scheme::Dark] {
        let snapshot = content.snapshot(300.0, 1.0, scheme, 0.0, 76.0);
        let inside = snapshot.pixel(150, 38).expect("pixel");
        assert_ne!(inside, background(scheme), "the body is coloured in {scheme:?}");
        // The margin of 8 around the body, up to every edge of the view, has the body's colour.
        for (x, y) in [(0, 0), (299, 0), (3, 38), (296, 38), (0, 75), (299, 75)] {
            assert_eq!(snapshot.pixel(x, y), Some(inside), "({x}, {y}) in {scheme:?}");
        }
    }
}

#[test]
fn the_roots_background_wins_over_the_bodys() {
    let content = content(
        "<html style=\"background:#203040\"><body style=\"background:#f0e0c0\">\
         <div style=\"height:60px\"></div></body></html>",
    );
    let snapshot = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 76.0);
    assert_eq!(snapshot.pixel(2, 2), Some([0x20, 0x30, 0x40, 255]), "the canvas takes the root's colour");
    assert_eq!(snapshot.pixel(150, 38), Some([0xf0, 0xe0, 0xc0, 255]), "the body keeps its own");
}

/// A newsletter as the core's cleaning leaves it: `body` and its colour are gone, the same colour is
/// on a wrapper around the whole mail.
const WRAPPED: &str = "<style>p { margin: 0 }</style>\n<div style=\"background-color:#f0e0c0\">\
     <div style=\"height:60px\"></div></div>\n";

#[test]
fn a_wrapper_around_the_whole_mail_colours_the_view_as_the_lost_body_would() {
    let content = content(WRAPPED);
    for scheme in [Scheme::Light, Scheme::Dark] {
        let snapshot = content.snapshot(300.0, 1.0, scheme, 0.0, 76.0);
        let inside = snapshot.pixel(150, 38).expect("pixel");
        assert_ne!(inside, background(scheme), "the wrapper is coloured in {scheme:?}");
        for (x, y) in [(0, 0), (299, 0), (3, 38), (296, 38), (0, 75), (299, 75)] {
            assert_eq!(snapshot.pixel(x, y), Some(inside), "({x}, {y}) in {scheme:?}");
        }
    }
}

#[test]
fn a_narrow_box_or_text_beside_it_is_no_wrapper() {
    let narrow = content("<div style=\"background:#f0e0c0;width:200px;height:60px\"></div>");
    let beside = content("Hello <div style=\"background:#f0e0c0;height:60px\"></div>");
    for content in [narrow, beside] {
        let snapshot = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 76.0);
        assert_eq!(snapshot.pixel(2, 2), Some(background(Scheme::Light)), "the margin keeps the surface");
        assert_eq!(snapshot.pixel(296, 70), Some(background(Scheme::Light)), "the margin keeps the surface");
    }
}

/// Where the first black pixel is in row `y`, from the left.
fn first_black(snapshot: &Snapshot, y: u32) -> Option<u32> {
    (0..snapshot.width).find(|&x| snapshot.pixel(x, y) == Some([0, 0, 0, 255]))
}

/// A black box 50 wide at the start of a mail, then text: no background of its own.
const PLAIN: &str = "<div style=\"background:#000;width:50px;height:20px\"></div><p>Text</p>";

#[test]
fn a_mail_without_a_background_gets_the_asked_margin() {
    let content = content(PLAIN);
    let browser = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 40.0);
    assert_eq!(first_black(&browser, 12), Some(8), "the browser's margin by default");
    content.set_margin(24.0);
    let asked = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 40.0);
    assert_eq!(first_black(&asked, 12), Some(24), "the margin the view asks for, at the sides");
    assert_eq!(first_black(&asked, 4), None, "the browser's 8 above");
    content.set_margin(BROWSER_MARGIN);
    let back = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 40.0);
    assert_eq!(first_black(&back, 12), Some(8), "and back");
}

#[test]
fn a_mail_with_a_background_or_its_own_margin_keeps_it() {
    let wrapped = content(
        "<div style=\"background:#f0e0c0\"><div style=\"background:#000;width:50px;height:20px\"></div>\
         <p>Text</p></div>",
    );
    let own = content(&format!("<style>body {{ margin: 0 }}</style>{PLAIN}"));
    for (content, left) in [(wrapped, 8), (own, 0)] {
        content.set_margin(24.0);
        let snapshot = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 40.0);
        assert_eq!(first_black(&snapshot, 12), Some(left));
    }
}

/// The pixel in rows `rows`, columns `left..right`, that differs most from the pixel at `(left - 16, row)`
/// (the box's own surface): the core of a glyph.
fn strongest(snapshot: &Snapshot, left: u32, right: u32, rows: std::ops::Range<u32>) -> peniko::Color {
    let surface = color(snapshot.pixel(12, rows.start + 4).expect("pixel"));
    let mut best = surface;
    for y in rows {
        for x in left..right {
            let Some(pixel) = snapshot.pixel(x, y) else { continue };
            let pixel = color(pixel);
            if contrast(pixel, surface) > contrast(best, surface) {
                best = pixel;
            }
        }
    }
    best
}

#[test]
fn pictures_arriving_grow_the_mail() {
    let content = content("<p>Above</p><img src=\"cid:hero@torro.example\"><p>Below</p>");
    let before = content.height(700.0, 1.0, Scheme::Light);
    settle(&content);
    let after = content.height(700.0, 1.0, Scheme::Light);
    assert!(after >= before + 200.0, "{before} before the picture, {after} after");
}

#[test]
fn a_refused_style_sheet_does_not_blank_the_mail() {
    let content = content("<style>@import url(\"https://fonts.example.invalid/a.css\");</style><p>Still here</p>");
    settle(&content);
    let snapshot = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 60.0);
    assert!(painted(&snapshot, Scheme::Light) > 20);
}

#[test]
fn an_empty_css_import_does_not_crash_the_mail() {
    let tricky = r#"<!-- A description of the <style> block and its @import rule. -->
        <style type="text/css">@import url(""); @import url("https://fonts.example/a;b.css");
        /* @import url("kept"); */ p::before { content: "@import url('kept');"; }
        p { color: red; }</style><p>Still here</p>"#;
    let cleaned = without_css_imports(tricky);
    assert!(!cleaned.contains("@import url(\"\");"));
    assert!(cleaned.contains("/* @import url(\"kept\"); */"));
    assert!(cleaned.contains("<style type=\"text/css\">"));
    assert!(cleaned.contains("p { color: red; }"));

    assert!(!without_css_imports("<style>@import url(\"\");").contains("@import"));
    assert!(!without_css_imports("<style>p { font-family: 'Foo; }\n@import url(\"\");</style>").contains("@import"));
    for html in [
        "<style>@import url(\"\"); p { color: red; }</style><p>Still here</p>",
        "<style>@IMPORT \"mail.css\"; p { color: red; }</style><p>Still here</p>",
    ] {
        let content = content(html);
        let snapshot = content.snapshot(300.0, 1.0, Scheme::Light, 0.0, 60.0);
        assert!(painted(&snapshot, Scheme::Light) > 20);
    }
}

const LINKS: &str = "<a href=\"https://example.com/x\" style=\"display:block;width:200px;height:30px\">Example</a>\
     <a href=\"javascript:alert(1)\" style=\"display:block;width:200px;height:30px\">Script</a>\
     <a href=\"mailto:sam@example.com\" style=\"display:block;width:200px;height:30px\">Mail</a>\
     <p style=\"margin:0;height:30px\">No link</p>";

#[test]
fn links_are_hit_at_their_place() {
    let content = content(LINKS);
    content.height(400.0, 1.0, Scheme::Light);
    // Body margin 8: the links fill 8..38, 38..68 and 68..98, the paragraph 98..128.
    assert_eq!(content.link_at(Point::new(20.0, 20.0)).as_deref(), Some("https://example.com/x"));
    assert_eq!(content.link_at(Point::new(20.0, 50.0)), None, "javascript: is never reported");
    assert_eq!(content.link_at(Point::new(20.0, 80.0)).as_deref(), Some("mailto:sam@example.com"));
    assert_eq!(content.link_at(Point::new(20.0, 110.0)), None);
    assert_eq!(content.link_at(Point::new(300.0, 20.0)), None, "beside the link");
}

#[test]
fn a_click_reports_the_link_a_drag_does_not() {
    let content = content(LINKS);
    content.height(400.0, 1.0, Scheme::Light);
    let link = Point::new(20.0, 20.0);
    content.press(link);
    assert_eq!(content.release(link).as_deref(), Some("https://example.com/x"));

    content.press(link);
    content.drag(Point::new(150.0, 20.0));
    assert_eq!(content.release(Point::new(150.0, 20.0)), None, "dragging selects, it does not click");

    content.press(link);
    assert_eq!(content.release(Point::new(20.0, 110.0)), None, "released off the link");
}

#[test]
fn dragging_selects_text() {
    let content = content("<p style=\"font-size:20px\">Select these words please</p>");
    content.height(600.0, 1.0, Scheme::Light);
    assert_eq!(content.selected_text(), None);
    content.press(Point::new(9.0, 40.0));
    content.drag(Point::new(300.0, 40.0));
    content.drag(Point::new(590.0, 40.0));
    assert_eq!(content.release(Point::new(590.0, 40.0)), None);
    let selected = content.selected_text().unwrap_or_default();
    assert!(selected.contains("these words"), "{selected:?}");
}

#[test]
fn selecting_repaints() {
    let content = content("<p style=\"font-size:20px\">Select these words please</p>");
    let before = content.frame(600.0, 1.0, Scheme::Light, 0.0, 100.0).expect("frame");
    let again = content.frame(600.0, 1.0, Scheme::Light, 0.0, 100.0).expect("frame");
    assert_eq!(before.tiles[0].handle.id(), again.tiles[0].handle.id(), "nothing changed: the cached picture");
    content.press(Point::new(9.0, 40.0));
    content.drag(Point::new(400.0, 40.0));
    content.release(Point::new(400.0, 40.0));
    let after = content.frame(600.0, 1.0, Scheme::Light, 0.0, 100.0).expect("frame");
    assert_ne!(before.tiles[0].handle.id(), after.tiles[0].handle.id());
}

#[test]
fn a_frame_covers_the_visible_band_and_some_more() {
    let content = content(&long_mail());
    let frame = content.frame(600.0, 2.0, Scheme::Light, 5000.0, 700.0).expect("frame");
    assert!(frame.top <= 5000.0 && frame.top + frame.height >= 5700.0, "{} + {}", frame.top, frame.height);
    assert!(frame.height <= 1400.0 + 1.0);
    assert_eq!(frame.width, 600.0);
    let near = content.frame(600.0, 2.0, Scheme::Light, 5100.0, 700.0).expect("frame");
    assert_eq!(frame.tiles[0].handle.id(), near.tiles[0].handle.id(), "a small scroll step reuses the picture");
    assert!(content.frame(600.0, 2.0, Scheme::Light, -500.0, 100.0).is_none(), "above the mail");
}

#[test]
fn every_scrolled_tile_is_small_enough_to_draw_in_the_current_frame() {
    let content = content(&long_mail());
    for scheme in [Scheme::Light, Scheme::Dark] {
        let frame = content.frame(700.0, 2.0, scheme, 25_000.0, 700.0).expect("frame");
        assert!(frame.tiles.len() > 1, "the high-resolution band must be tiled");
        let mut next = frame.top;
        for tile in &frame.tiles {
            let image::Handle::Rgba { width, height, pixels, .. } = &tile.handle else {
                panic!("a tile must contain RGBA pixels");
            };
            assert_eq!(pixels.len(), (*width * *height * 4) as usize);
            assert!(pixels.len() <= MAX_TILE_BYTES, "a larger tile would be uploaded asynchronously");
            assert!((tile.top - next).abs() < 0.01, "gap between tiles at {next}");
            next = tile.top + tile.height;
        }
        assert!((next - frame.top - frame.height).abs() < 0.01, "tiles must cover the whole painted band");
    }
}

#[test]
fn the_scale_sets_device_pixels_not_the_layout() {
    let content = content(&fixture("invoice"));
    let one = content.height(500.0, 1.0, Scheme::Light);
    let two = content.height(500.0, 2.0, Scheme::Light);
    assert!((one - two).abs() < 2.0, "{one} at 1x, {two} at 2x");
    let snapshot = content.snapshot(500.0, 2.0, Scheme::Light, 0.0, 100.0);
    assert_eq!((snapshot.width, snapshot.height), (1000, 200));
}

#[test]
fn a_mail_tells_whether_it_has_a_background_of_its_own() {
    assert!(!content(PLAIN).has_own_background(), "a black box is not the mail's background");
    assert!(content(&format!("<body style=\"background:#fc0\">{PLAIN}</body>")).has_own_background());
    assert!(content(WRAPPED).has_own_background(), "a wrapper's colour counts");
    let laid_out = content(WRAPPED);
    laid_out.snapshot(300.0, 1.0, Scheme::Light, 0.0, 40.0);
    assert!(laid_out.has_own_background(), "also after the first layout");
}
