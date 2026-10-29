//! One parsed mail as a Blitz document: layout for a width, painting a part of it into a picture, pointer
//! input for selecting text and clicking links.
//!
//! The document is laid out at the width it gets and as tall as it is, so the widget can be as tall as the
//! mail and the reader's own `scrollable` scrolls it. Only the visible part is painted (plus half a screen
//! above and below, so that small scroll steps reuse the picture); Blitz skips everything outside.

use std::cell::RefCell;
use std::fmt;
use std::sync::{Arc, OnceLock};

use anyrender::{ImageRenderer as _, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, Document as _, DocumentConfig, FontContext, RestyleHint, StyleThreading, local_name};
use blitz_html::HtmlDocument;
use blitz_paint::paint_scene;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::net::Url;
use blitz_traits::shell::{ColorScheme, Viewport};
use cursor_icon::CursorIcon;
use iced::widget::image;
use iced::{Point, mouse};
use peniko::Fill;
use peniko::kurbo::{Affine, Rect};

use crate::Scheme;
use crate::fetch::{CidSource, Fetcher, Limits};
use crate::recolor::{DarkScene, Recolor};

/// Mail clients' defaults on top of the browser's: a sans-serif face when the mail names none, and long
/// words and plain-text lines that wrap instead of running out of the reader.
const MAIL_DEFAULTS: &str = "html { font-family: sans-serif; overflow-wrap: break-word; } \
                             pre { white-space: pre-wrap; overflow-wrap: anywhere; }";

/// The margin browsers give `body` (logical pixels), kept unless the app sets another for a mail
/// without a background of its own ([`crate::HtmlView::margin`]).
pub(crate) const BROWSER_MARGIN: f32 = 8.0;

/// The viewport height Blitz lays out with, in CSS pixels. The widget is as tall as the mail, so there is no
/// real viewport height; this only matters for `vh` units and height media queries, rare in mail.
const VIEWPORT_HEIGHT: f32 = 800.0;

/// The largest picture painted at once, in device pixels per side (what GPUs take as one texture).
const MAX_PICTURE: u32 = 8192;

/// Iced 0.14 uploads smaller raster images in the current frame; larger ones are queued on a
/// worker and leave the mail's background visible until the next redraw. Stay below its 2 MiB
/// threshold so every newly scrolled band is complete on the first frame.
const MAX_TILE_BYTES: usize = 2 * 1024 * 1024 - 1;

/// A pointer press further than this from where it started (logical pixels) selects instead of clicking.
const CLICK_SLOP: f32 = 4.0;

/// The smallest zoom for a mail wider than the reader (a 600-px newsletter in a 360-px reader is drawn at
/// 0.6): below half size text is no longer readable, and the rest is cut off on the right instead.
const MIN_ZOOM: f32 = 0.5;

/// Imported CSS never reaches the mail renderer (the fetcher accepts pictures only). Remove the
/// rules before Blitz parses them: its stylesheet loader unwraps an unresolved import URL.
fn without_css_imports(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;
    let mut scan = 0;
    while let Some(offset) = html[scan..].find('<') {
        let start = scan + offset;
        if html[start..].starts_with("<!--") {
            scan = html[start + 4..].find("-->").map_or(html.len(), |offset| start + 4 + offset + 3);
            continue;
        }
        if html.as_bytes()[start..].get(..6).is_none_or(|part| !part.eq_ignore_ascii_case(b"<style")) {
            scan = start + 1;
            continue;
        }
        let after_name = start + "<style".len();
        if html.as_bytes().get(after_name).is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-') {
            scan = after_name;
            continue;
        }
        let Some(open_end) = html[after_name..].find('>').map(|offset| after_name + offset + 1) else { break };
        // A cut-off mail has no closing tag: the parser reads the rest of the document as the style.
        let close =
            find_ascii_case_insensitive(&html[open_end..], b"</style").map_or(html.len(), |offset| open_end + offset);
        result.push_str(&html[cursor..open_end]);
        result.push_str(&without_import_rules(&html[open_end..close]));
        cursor = close;
        scan = close;
    }
    result.push_str(&html[cursor..]);
    result
}

fn find_ascii_case_insensitive(text: &str, needle: &[u8]) -> Option<usize> {
    text.as_bytes().windows(needle.len()).position(|part| part.eq_ignore_ascii_case(needle))
}

/// Remove `@import` through its semicolon, respecting strings, comments and `url(…)` arguments.
fn without_import_rules(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut result = String::with_capacity(css.len());
    let mut cursor = 0;
    let mut index = 0;
    let mut quote = None;
    let mut comment = false;
    while index < bytes.len() {
        if comment {
            if bytes[index..].starts_with(b"*/") {
                comment = false;
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(mark) = quote {
            match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                // An unescaped line break ends a string in CSS, so a stray quote cannot hide the rest.
                byte if byte == mark || byte == b'\n' => {
                    quote = None;
                    index += 1;
                }
                _ => index += 1,
            }
        } else if bytes[index..].starts_with(b"/*") {
            comment = true;
            index += 2;
        } else if matches!(bytes[index], b'\'' | b'"') {
            quote = Some(bytes[index]);
            index += 1;
        } else if bytes[index..].get(..7).is_some_and(|part| part.eq_ignore_ascii_case(b"@import"))
            && !bytes.get(index + 7).is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            result.push_str(&css[cursor..index]);
            index = import_end(bytes, index + 7);
            cursor = index;
        } else {
            index += 1;
        }
    }
    result.push_str(&css[cursor..]);
    result
}

fn import_end(bytes: &[u8], mut index: usize) -> usize {
    let mut quote = None;
    let mut comment = false;
    let mut parens: usize = 0;
    while index < bytes.len() {
        if comment {
            if bytes[index..].starts_with(b"*/") {
                comment = false;
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(mark) = quote {
            match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                // An unescaped line break ends a string in CSS, so a stray quote cannot hide the rest.
                byte if byte == mark || byte == b'\n' => {
                    quote = None;
                    index += 1;
                }
                _ => index += 1,
            }
        } else if bytes[index..].starts_with(b"/*") {
            comment = true;
            index += 2;
        } else {
            match bytes[index] {
                b'\'' | b'"' => quote = Some(bytes[index]),
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                b';' if parens == 0 => return index + 1,
                _ => {}
            }
            index += 1;
        }
    }
    index
}

/// One mail, parsed and ready to draw with [`crate::HtmlView`].
///
/// Keep it in the app state while the mail is shown; build a new one for another mail. Its pictures start
/// loading when it is built and stop when it is dropped. Not `Send`: it lives with the window.
pub struct Content {
    inner: RefCell<Inner>,
}

/// A painted part of the mail, in device pixels: RGBA, 8 bits per channel, straight alpha, every pixel opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// `width * height * 4` bytes, row by row from the top.
    pub rgba: Vec<u8>,
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Snapshot({}x{})", self.width, self.height)
    }
}

impl Snapshot {
    /// The pixel at `(x, y)` in device pixels as `[r, g, b, a]`, `None` outside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = self.rgba.get(at..at + 4)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

/// What the document is laid out for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    /// Width in device pixels.
    width: u32,
    scale: f32,
    scheme: Scheme,
}

impl Fit {
    fn new(width: f32, scale: f32, scheme: Scheme) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let width = (width.max(1.0) * scale).round().clamp(1.0, MAX_PICTURE as f32) as u32;
        Self { width, scale, scheme }
    }

    /// The viewport at `zoom`: CSS pixels are `zoom` logical pixels, so the mail is `width / zoom` CSS
    /// pixels wide.
    fn viewport(self, zoom: f32) -> Viewport {
        let height = (VIEWPORT_HEIGHT * self.scale).round() as u32;
        let scheme = match self.scheme {
            Scheme::Light => ColorScheme::Light,
            Scheme::Dark => ColorScheme::Dark,
        };
        let mut viewport = Viewport::new(self.width, height, self.scale, scheme);
        viewport.set_zoom(zoom);
        viewport
    }

    /// The width in logical pixels.
    fn logical_width(self) -> f32 {
        self.width as f32 / self.scale
    }
}

/// The colours a picture is painted with; part of what decides whether a cached picture still fits.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Colors {
    background: peniko::Color,
    dark: Option<Recolor>,
}

impl Colors {
    /// From the active design: the light or dark content surface, and in dark the dark text colour.
    fn of(scheme: Scheme) -> Self {
        let peniko = |c: iced::Color| peniko::Color::new([c.r, c.g, c.b, c.a]);
        match scheme {
            Scheme::Light => Self { background: peniko(surface(Scheme::Light)), dark: None },
            Scheme::Dark => {
                let palette = torro_gui::theme::Palette::dark();
                let background = peniko(palette.panel);
                Self { background, dark: Some(Recolor::new(background, peniko(palette.text))) }
            }
        }
    }
}

/// The surface the mail is drawn on in `scheme`: the design's content surface (`PANEL`) of that mode.
pub(crate) fn surface(scheme: Scheme) -> iced::Color {
    match scheme {
        Scheme::Light => torro_gui::theme::Palette::light().panel,
        Scheme::Dark => torro_gui::theme::Palette::dark().panel,
    }
}

/// One image tile, positioned in logical pixels relative to the mail's top.
#[derive(Debug, Clone)]
pub(crate) struct FrameTile {
    pub(crate) handle: image::Handle,
    pub(crate) top: f32,
    pub(crate) height: f32,
}

/// The picture the widget shows: a band of the mail, in logical pixels relative to the mail's top.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    pub(crate) tiles: Vec<FrameTile>,
    pub(crate) top: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    key: FrameKey,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FrameKey {
    fit: Fit,
    generation: u64,
    colors: Colors,
}

/// A press of the primary button, until it is released.
#[derive(Debug, Clone)]
struct Press {
    at: Point,
    link: Option<String>,
    moved: bool,
}

struct Inner {
    doc: BaseDocument,
    fetcher: Fetcher,
    /// Answers of the fetcher the document has taken in.
    taken: u64,
    fit: Option<Fit>,
    /// Logical pixels per CSS pixel: below 1 when the mail is wider than `fit` (zoomed out to fit).
    zoom: f32,
    /// Height of the mail in logical pixels at `fit`.
    height: f32,
    painter: Option<VelloCpuImageRenderer>,
    frame: Option<Frame>,
    /// Grows whenever what a picture would show changes (layout, selection, pictures arriving).
    generation: u64,
    press: Option<Press>,
    /// The margin for a mail without a background of its own, as the view asks ([`Content::set_margin`]).
    margin: f32,
    /// Whether the mail has no background of its own ([`own_background`]), found at the first layout.
    plain: Option<bool>,
    /// The user agent style sheet with the margin in place of the browser's, while one is added.
    margin_css: Option<String>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.fetcher.close();
    }
}

impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Content").finish_non_exhaustive()
    }
}

impl Content {
    /// Parses `html` (cleaned by the core) and starts loading its pictures from the web and `data:` URLs.
    /// `cid:` pictures stay empty; use [`Content::with_cid`] when the mail has inline parts.
    pub fn new(html: &str) -> Self {
        Self::build(html, Fetcher::new(true, None, Limits::default()))
    }

    /// Like [`Content::new`], with `cid:` pictures from `source` (asked on a worker thread).
    pub fn with_cid(html: &str, source: Arc<dyn CidSource>) -> Self {
        Self::build(html, Fetcher::new(true, Some(source), Limits::default()))
    }

    /// Like [`Content::with_cid`] (or [`Content::new`] with `None`), but web pictures stay empty: nothing goes
    /// to the network. For tests, previews and examples; the reader uses [`Content::new`] or
    /// [`Content::with_cid`], because external pictures are always loaded (decision 8 in `docs/mail.md`).
    pub fn offline(html: &str, source: Option<Arc<dyn CidSource>>) -> Self {
        Self::build(html, Fetcher::new(false, source, Limits::default()))
    }

    pub(crate) fn build(html: &str, fetcher: Fetcher) -> Self {
        // Blitz panics when an @import has no resolvable URL (for example `url("")`). Imported
        // stylesheets are not used here anyway: the fetcher only hands pictures back to Blitz.
        let html = without_css_imports(html);
        let config = DocumentConfig {
            viewport: Some(Fit::new(1.0, 1.0, Scheme::Light).viewport(1.0)),
            net_provider: Some(Arc::new(fetcher.clone())),
            font_ctx: Some(fonts()),
            // Several mails may resolve on several threads (tests): Stylo's shared pool would panic.
            style_threading: StyleThreading::Sequential,
            ..Default::default()
        };
        let mut doc = HtmlDocument::from_html(&html, config).into_inner();
        doc.add_user_agent_stylesheet(MAIL_DEFAULTS);
        let inner = Inner {
            doc,
            fetcher,
            taken: 0,
            fit: None,
            zoom: 1.0,
            height: 0.0,
            painter: None,
            frame: None,
            generation: 0,
            press: None,
            margin: BROWSER_MARGIN,
            plain: None,
            margin_css: None,
        };
        Self { inner: RefCell::new(inner) }
    }

    /// The height of the mail in logical pixels when it is `width` logical pixels wide, laid out at `scale`
    /// device pixels per logical pixel in `scheme` (a mail may style itself for dark). Lays out again only
    /// when one of them changed.
    pub fn height(&self, width: f32, scale: f32, scheme: Scheme) -> f32 {
        let mut inner = self.inner.borrow_mut();
        inner.fit(Fit::new(width, scale, scheme));
        inner.height
    }

    /// Paints the part from `top` to `top + height` (logical pixels, cut to the mail) at `width` logical
    /// pixels, as the widget would show it. For tests, examples and anything else that wants a picture
    /// without a window. Takes in the pictures that arrived so far first.
    pub fn snapshot(&self, width: f32, scale: f32, scheme: Scheme, top: f32, height: f32) -> Snapshot {
        let mut inner = self.inner.borrow_mut();
        inner.poll();
        let fit = Fit::new(width, scale, scheme);
        inner.fit(fit);
        let (top, bottom) = inner.band(fit, top, top + height);
        let rgba = inner.paint(fit, top, bottom, Colors::of(scheme));
        Snapshot { width: fit.width, height: bottom - top, rgba }
    }

    /// The margin for a mail without a background of its own ([`crate::HtmlView::margin`]); lays the mail
    /// out again at the next use when it changed.
    pub(crate) fn set_margin(&self, margin: f32) {
        let Ok(mut inner) = self.inner.try_borrow_mut() else { return };
        if inner.margin != margin {
            inner.margin = margin;
            inner.fit = None;
        }
    }

    /// Whether the mail paints a background of its own (on `html`, on `body` or on a wrapper around the
    /// whole mail): the app lets such a mail fill its place from edge to edge, while a mail without one
    /// stands in a reading column. Styles the mail first when that has not happened yet.
    pub fn has_own_background(&self) -> bool {
        let Ok(mut inner) = self.inner.try_borrow_mut() else { return false };
        if let Some(plain) = inner.plain {
            return !plain;
        }
        // Before the first layout the styles are not there yet: look at them now, but leave the answer
        // for the layout to find out for itself.
        inner.doc.resolve(0.0);
        own_background(&inner.doc)
    }

    /// Whether pictures are still on their way (or arrived and are not drawn yet).
    pub fn is_loading(&self) -> bool {
        let inner = self.inner.borrow();
        inner.fetcher.pending() > 0 || inner.fetcher.answered() != inner.taken
    }

    /// The text the person selected, if any.
    pub fn selected_text(&self) -> Option<String> {
        self.inner.borrow().doc.get_selected_text().filter(|text| !text.trim().is_empty())
    }

    /// The target of the link at `point` (logical pixels from the mail's top left, at the last layout), only
    /// `http`, `https` and `mailto`; `None` elsewhere and for other targets.
    pub fn link_at(&self, point: Point) -> Option<String> {
        self.inner.borrow().link_at(point)
    }

    /// Takes in pictures that arrived. `true` when something changed (the height may have, too).
    pub(crate) fn poll(&self) -> bool {
        self.inner.borrow_mut().poll()
    }

    /// The picture for the band `top..top + height` of the mail (logical pixels), from the cache when it
    /// still fits. `None` when nothing of the mail is in the band.
    pub(crate) fn frame(&self, width: f32, scale: f32, scheme: Scheme, top: f32, height: f32) -> Option<Frame> {
        let mut inner = self.inner.try_borrow_mut().ok()?;
        let fit = Fit::new(width, scale, scheme);
        inner.fit(fit);
        let key = FrameKey { fit, generation: inner.generation, colors: Colors::of(scheme) };
        let bottom = (top + height).min(inner.height);
        let top = top.max(0.0);
        if bottom <= top {
            return None;
        }
        if let Some(frame) = &inner.frame
            && frame.key == key
            && frame.top <= top
            && frame.top + frame.height >= bottom
        {
            return Some(frame.clone());
        }
        // Half a screen more above and below: small scroll steps reuse the picture.
        let extra = height / 2.0;
        let (from, to) = inner.band(fit, top - extra, bottom + extra);
        let rgba = inner.paint(fit, from, to, key.colors);
        let bytes_per_row = fit.width as usize * 4;
        let rows_per_tile = (MAX_TILE_BYTES / bytes_per_row).max(1);
        let tiles = rgba
            .chunks(rows_per_tile * bytes_per_row)
            .enumerate()
            .map(|(index, pixels)| {
                let rows = (pixels.len() / bytes_per_row) as u32;
                FrameTile {
                    handle: image::Handle::from_rgba(fit.width, rows, pixels.to_vec()),
                    top: (from as usize + index * rows_per_tile) as f32 / fit.scale,
                    height: rows as f32 / fit.scale,
                }
            })
            .collect();
        let frame = Frame {
            tiles,
            top: from as f32 / fit.scale,
            width: fit.width as f32 / fit.scale,
            height: (to - from) as f32 / fit.scale,
            key,
        };
        inner.frame = Some(frame.clone());
        Some(frame)
    }

    /// The primary button went down at `point` (logical pixels from the mail's top left).
    pub(crate) fn press(&self, point: Point) {
        let mut inner = self.inner.borrow_mut();
        let link = inner.link_at(point);
        inner.press = Some(Press { at: point, link, moved: false });
        inner.pointer(UiEvent::PointerDown, point, MouseEventButtons::Primary);
    }

    /// The pointer moved to `point` with the button down: extends the selection.
    pub(crate) fn drag(&self, point: Point) {
        let mut inner = self.inner.borrow_mut();
        if let Some(press) = &mut inner.press
            && press.at.distance(point) > CLICK_SLOP
        {
            press.moved = true;
        }
        inner.pointer(UiEvent::PointerMove, point, MouseEventButtons::Primary);
    }

    /// The pointer moved to `point` without a button: only the cursor follows.
    pub(crate) fn hover(&self, point: Point) {
        let mut inner = self.inner.borrow_mut();
        let event = inner.event(point, MouseEventButtons::None);
        inner.doc.handle_ui_event(UiEvent::PointerMove(event));
    }

    /// The primary button came up at `point`. The link clicked, when it went down and up on the same link
    /// without dragging in between.
    pub(crate) fn release(&self, point: Point) -> Option<String> {
        let mut inner = self.inner.borrow_mut();
        let press = inner.press.take();
        inner.pointer(UiEvent::PointerUp, point, MouseEventButtons::None);
        let link = press.filter(|press| !press.moved)?.link?;
        (inner.link_at(point).as_deref() == Some(link.as_str())).then_some(link)
    }

    /// The cursor Blitz asks for at the last pointer position: a hand over links, a text cursor over text.
    pub(crate) fn cursor(&self) -> mouse::Interaction {
        match self.inner.borrow().doc.get_cursor() {
            Some(CursorIcon::Pointer) => mouse::Interaction::Pointer,
            Some(CursorIcon::Text) => mouse::Interaction::Text,
            _ => mouse::Interaction::Idle,
        }
    }
}

impl Inner {
    /// Lays the mail out for `fit`. A mail wider than that (a fixed 600-px newsletter in a narrow reader)
    /// is zoomed out until it fits, as phones show such mail, instead of being cut off: the reader never
    /// scrolls sideways.
    fn fit(&mut self, fit: Fit) {
        if self.fit == Some(fit) {
            return;
        }
        self.zoom = 1.0;
        self.doc.set_viewport(fit.viewport(1.0));
        self.doc.resolve(0.0);
        if self.apply_margin() {
            self.doc.resolve(0.0);
        }
        let available = fit.logical_width();
        let needed = content_width(&self.doc);
        if needed > available + 1.0 {
            self.zoom = (available / needed).max(MIN_ZOOM);
            self.doc.set_viewport(fit.viewport(self.zoom));
            self.doc.resolve(0.0);
        }
        self.fit = Some(fit);
        self.measure();
    }

    /// Puts the asked margin at the sides of `body` of a mail without a background of its own (above
    /// and below it keeps the browser's, so its text does not touch the edge of a light paper in a dark
    /// window), as a user agent style sheet, so a margin the mail sets itself still wins. `true` when the styles changed and the mail
    /// must be resolved again. Whether the mail has a background is looked at once, after its first
    /// styling: it does not change with the width.
    fn apply_margin(&mut self) -> bool {
        let plain = match self.plain {
            Some(plain) => plain,
            None => {
                let plain = !own_background(&self.doc);
                self.plain = Some(plain);
                plain
            }
        };
        let css = (plain && self.margin != BROWSER_MARGIN)
            .then(|| format!("body {{ margin: {BROWSER_MARGIN}px {}px; }}", self.margin));
        if css == self.margin_css {
            return false;
        }
        if let Some(old) = self.margin_css.take() {
            self.doc.remove_user_agent_stylesheet(&old);
        }
        if let Some(css) = &css {
            self.doc.add_user_agent_stylesheet(css);
        }
        self.margin_css = css;
        // A changed style sheet restyles nothing by itself: the whole document is styled again.
        if let Some(root) = self.doc.try_root_element().map(|root| root.id)
            && let Some(node) = self.doc.get_node_mut(root)
        {
            node.set_restyle_hint(RestyleHint::restyle_subtree());
            node.mark_ancestors_dirty();
        }
        true
    }

    /// The height of the laid-out mail in logical pixels: the root element or what overflows it, whichever
    /// is taller.
    fn measure(&mut self) {
        let height = self.doc.try_root_element().map_or(0.0, |root| {
            let layout = root.final_layout();
            layout.size.height.max(layout.scrollable_overflow_rect.bottom)
        });
        self.height = (height * self.zoom).max(0.0);
        self.generation += 1;
    }

    /// A point in logical pixels from the mail's top left as CSS pixels of the document.
    fn css(&self, point: Point) -> Point {
        Point::new(point.x / self.zoom, point.y / self.zoom)
    }

    fn poll(&mut self) -> bool {
        let answered = self.fetcher.answered();
        if answered == self.taken {
            return false;
        }
        // Read the count first: an answer landing during `resolve` is then taken in the next time.
        self.taken = answered;
        self.doc.resolve(0.0);
        // A picture may widen the mail: fit it again.
        match self.fit.take() {
            Some(fit) => self.fit(fit),
            None => self.generation += 1,
        }
        true
    }

    /// The device-pixel rows `from..to` for the logical band `top..bottom`, cut to the mail and to the
    /// largest picture.
    fn band(&self, fit: Fit, top: f32, bottom: f32) -> (u32, u32) {
        let end = (self.height * fit.scale).ceil().max(1.0) as u32;
        let from = ((top.max(0.0) * fit.scale).floor() as u32).min(end.saturating_sub(1));
        let to = ((bottom * fit.scale).ceil() as u32).clamp(from + 1, end.max(from + 1));
        (from, to.min(from + MAX_PICTURE))
    }

    /// Paints device rows `from..to` at `fit` on the surface of `colors`.
    fn paint(&mut self, fit: Fit, from: u32, to: u32, colors: Colors) -> Vec<u8> {
        let (width, height) = (fit.width, to - from);
        let painter = self.painter.get_or_insert_with(|| VelloCpuImageRenderer::new(width, height));
        painter.resize(width, height);
        // Blitz paints what its viewport scroll shows; the widget scrolls outside, so it is only set here.
        let scale = f64::from(fit.scale) * f64::from(self.zoom);
        self.doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: f64::from(from) / scale });
        let canvas = wrapper_background(&self.doc);
        let doc = &mut self.doc;
        let mut rgba = Vec::new();
        painter.render_to_vec(
            |scene| {
                let all = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
                scene.fill(Fill::NonZero, Affine::IDENTITY, colors.background, None, &all);
                match colors.dark {
                    None => {
                        if let Some(canvas) = canvas {
                            scene.fill(Fill::NonZero, Affine::IDENTITY, canvas, None, &all);
                        }
                        paint_scene(scene, doc, scale, width, height, 0, 0);
                    }
                    Some(recolor) => {
                        let mut dark = DarkScene::new(scene, recolor, scale);
                        // Through the dark scene: the canvas turns dark like every other surface.
                        if let Some(canvas) = canvas {
                            dark.fill(Fill::NonZero, Affine::IDENTITY, canvas, None, &all);
                        }
                        paint_scene(&mut dark, doc, scale, width, height, 0, 0);
                    }
                }
            },
            &mut rgba,
        );
        self.doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
        rgba
    }

    fn event(&self, point: Point, buttons: MouseEventButtons) -> BlitzPointerEvent {
        let point = self.css(point);
        // The viewport never scrolls outside painting: page and client coordinates are the same.
        let coords = PointerCoords {
            page_x: point.x,
            page_y: point.y,
            screen_x: point.x,
            screen_y: point.y,
            client_x: point.x,
            client_y: point.y,
        };
        BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: MouseEventButton::Main,
            buttons,
            mods: Default::default(),
            details: PointerDetails::default(),
            element: Default::default(),
            active_pointers: Default::default(),
        }
    }

    /// Hands a press, drag or release to Blitz (it selects text) and marks the picture as stale.
    fn pointer(&mut self, kind: fn(BlitzPointerEvent) -> UiEvent, point: Point, buttons: MouseEventButtons) {
        let event = self.event(point, buttons);
        self.doc.handle_ui_event(kind(event));
        self.doc.resolve(0.0);
        self.generation += 1;
    }

    fn link_at(&self, point: Point) -> Option<String> {
        let point = self.css(point);
        let hit = self.doc.hit(point.x, point.y)?;
        let mut node_id = self.doc.nearest_non_anonymous_ancestor(hit.node_id);
        while let Some(id) = node_id {
            let node = self.doc.get_node(id)?;
            if let Some(element) = node.element_data()
                && element.is_link()
            {
                return element.attr(local_name!("href")).and_then(allowed_link);
            }
            node_id = node.parent;
        }
        None
    }
}

/// The width the laid-out boxes reach, in CSS pixels: the widest right edge of any box. Taffy's overflow
/// rectangle of the root does not include a table that is wider than its parent, so the boxes are walked.
fn content_width(doc: &BaseDocument) -> f32 {
    let Some(root) = doc.try_root_element() else { return 0.0 };
    let mut widest = 0.0_f32;
    let mut stack = vec![(root.id, 0.0_f32)];
    while let Some((id, left)) = stack.pop() {
        let Some(node) = doc.get_node(id) else { continue };
        let layout = node.final_layout();
        let x = left + layout.location.x;
        widest = widest.max(x + layout.size.width);
        if let Some(children) = node.layout_children.borrow().as_ref() {
            stack.extend(children.iter().map(|&child| (child, x)));
        }
    }
    widest
}

/// Whether the mail paints a background of its own: on `html`, on `body` or on a wrapper around the
/// whole mail ([`wrapper_background`]).
fn own_background(doc: &BaseDocument) -> bool {
    let Some(root) = doc.try_root_element() else { return false };
    let coloured = |node: &blitz_dom::Node| {
        node.primary_styles().is_some_and(|styles| {
            !styles.clone_background_color().resolve_to_absolute(&styles.clone_color()).is_transparent()
        })
    };
    let body = root
        .children
        .iter()
        .filter_map(|&id| doc.get_node(id))
        .find(|node| node.data.is_element_with_tag_name(&local_name!("body")));
    coloured(root) || body.is_some_and(coloured) || wrapper_background(doc).is_some()
}

/// The colour of a wrapper around the whole mail, painted over the whole view under the mail like a
/// browser's canvas: the one element in `body` (besides `style` and white space), when it is as wide
/// as the body and has a background colour of its own.
///
/// Blitz already paints the background of `html`, else of `body`, over the whole view, as browsers
/// do. But the core's cleaning drops `body` with its colour (`bgcolor`, `style`), and newsletters
/// carry the same colour on such a wrapper for the mail apps that do so, too. Without this the
/// body's margin of 8 around the wrapper shows the reader's surface as a frame around the mail.
/// When `html` or `body` has a colour after all, Blitz paints it over this one.
fn wrapper_background(doc: &BaseDocument) -> Option<peniko::Color> {
    let root = doc.try_root_element()?;
    let body = root
        .children
        .iter()
        .filter_map(|&id| doc.get_node(id))
        .find(|node| node.data.is_element_with_tag_name(&local_name!("body")))?;
    let mut wrapper = None;
    for node in body.children.iter().filter_map(|&id| doc.get_node(id)) {
        if node.is_text_node() && !node.is_whitespace_node() {
            return None;
        }
        if !node.is_element() || node.data.is_element_with_tag_name(&local_name!("style")) {
            continue;
        }
        if wrapper.replace(node).is_some() {
            return None;
        }
    }
    let wrapper = wrapper?;
    if wrapper.final_layout().size.width + 1.0 < body.final_layout().content_box_width() {
        return None;
    }
    let styles = wrapper.primary_styles()?;
    let color = styles.clone_background_color().resolve_to_absolute(&styles.clone_color()).into_srgb_legacy();
    if color.is_transparent() {
        return None;
    }
    Some(peniko::Color::new(*color.raw_components()))
}

/// A link target the app may open: absolute `http`, `https` or `mailto`.
fn allowed_link(href: &str) -> Option<String> {
    let url = Url::parse(href.trim()).ok()?;
    matches!(url.scheme(), "http" | "https" | "mailto").then(|| url.to_string())
}

/// The font context all mails share: building one reads the system fonts (20–60 ms, trial M9.4).
fn fonts() -> FontContext {
    static FONTS: OnceLock<FontContext> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            use parley::fontique::{Collection, CollectionOptions, SourceCache};
            let mut fonts = FontContext {
                source_cache: SourceCache::new_shared(),
                collection: Collection::new(CollectionOptions { shared: true, system_fonts: true }),
            };
            // Blitz's own font for list bullets, as it registers it in a context of its own.
            let bullets = peniko::Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _);
            fonts.collection.register_fonts(bullets, None);
            fonts
        })
        .clone()
}

#[cfg(test)]
mod tests;
