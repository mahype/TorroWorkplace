//! Dark colours for a mail written for white paper: a wrapper around the paint scene that changes colours
//! while Blitz paints (task M9.6, "Hell/Dunkel nur für den Inhalt").
//!
//! The rules, in OKLab so that the hue stays and lightness looks even:
//!
//! - **Surfaces** (backgrounds, gradients, shadows): light ones (L > 0.5) are mirrored into the dark range,
//!   white lands on the design's dark content surface. Dark ones stay: a mail that is dark already keeps its
//!   own look (also one with `prefers-color-scheme: dark` rules).
//! - **Text**: dark text (L < 0.5) is mirrored into the light range, black lands on the design's dark text
//!   colour. Then it is moved further, if needed, until it has 4.5:1 against the surface under it (the last
//!   surface painted under its first glyph). Text over a picture keeps its colour: the sender chose it for
//!   that picture, and the picture is not recoloured.
//! - **Lines** (strokes, thin fills and outlines such as border edges, rules, underlines) follow the surface
//!   they lie on: on a surface that was mirrored they are mirrored too, whatever their lightness (a black rule
//!   on white stays visible, a light grey one stays quiet); on a surface that kept its colours they keep theirs.
//! - **Pictures** are never touched.
//!
//! Mirroring keeps the order of lightness, so what stood out on white still stands out on dark.

use std::any::Any;
use std::sync::Arc;

use anyrender::{Filter, Glyph, NormalizedCoord, Paint, PaintRef, PaintScene, RenderContext, ResourceId};
use peniko::color::{AlphaColor, DynamicColor, Oklab, Srgb};
use peniko::kurbo::{self, Affine, Point, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, Gradient, StyleRef};

/// WCAG AA for text.
pub(crate) const MIN_TEXT_CONTRAST: f32 = 4.5;

/// The colour rules of dark mode for one design: where white and black land.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Recolor {
    /// The surface everything is painted on (the design's dark content surface).
    canvas: Color,
    /// OKLab lightness white is mirrored to (the canvas).
    low: f32,
    /// OKLab lightness black is mirrored to (the design's dark text colour).
    high: f32,
}

impl Recolor {
    /// `canvas`: the dark surface the mail is painted on; `text`: the dark text colour of the design.
    pub(crate) fn new(canvas: Color, text: Color) -> Self {
        Self { canvas, low: lightness(canvas), high: lightness(text) }
    }

    /// The surface the mail is painted on.
    pub(crate) fn canvas(&self) -> Color {
        self.canvas
    }

    /// A surface: light ones turn dark, dark ones stay.
    pub(crate) fn surface(&self, color: Color) -> Color {
        if mirrors(color) { self.mirror(color) } else { color }
    }

    /// A line on a surface that was mirrored (`true`, also the bare canvas: white paper) or kept its colours.
    pub(crate) fn line(&self, color: Color, on_mirrored: bool) -> Color {
        if on_mirrored { self.mirror(color) } else { color }
    }

    /// Text on `under` (already recoloured, opaque or not): dark text turns light, then at least 4.5:1.
    pub(crate) fn text(&self, color: Color, under: Color) -> Color {
        let color = if lightness(color) < 0.5 { self.mirror(color) } else { color };
        let under = over(under, self.canvas);
        with_contrast(color, under, MIN_TEXT_CONTRAST)
    }

    /// Lightness mirrored from [0, 1] onto [high, low], hue and chroma kept (and reduced where sRGB has no room).
    fn mirror(&self, color: Color) -> Color {
        let [l, a, b] = color.convert::<Oklab>().components_rgb();
        let l = self.low + (1.0 - l.clamp(0.0, 1.0)) * (self.high - self.low);
        from_oklab(l, a, b, color.components[3])
    }
}

/// Small helper: the three colour components without alpha.
trait Components {
    fn components_rgb(self) -> [f32; 3];
}

impl Components for AlphaColor<Oklab> {
    fn components_rgb(self) -> [f32; 3] {
        let [l, a, b, _] = self.components;
        [l, a, b]
    }
}

/// Whether [`Recolor::surface`] mirrors `color`: it is light.
fn mirrors(color: Color) -> bool {
    lightness(color) > 0.5
}

/// OKLab lightness, 0 black to 1 white.
pub(crate) fn lightness(color: Color) -> f32 {
    color.convert::<Oklab>().components[0]
}

/// An sRGB colour from OKLab, with chroma reduced until it fits into sRGB (hue and lightness kept).
fn from_oklab(l: f32, a: f32, b: f32, alpha: f32) -> Color {
    let at = |k: f32| AlphaColor::<Oklab>::new([l, a * k, b * k, alpha]).convert::<Srgb>();
    let fits = |color: Color| color.components[..3].iter().all(|c| (-0.001..=1.001).contains(c));
    let full = at(1.0);
    if fits(full) {
        return clamp(full);
    }
    let (mut inside, mut outside) = (0.0_f32, 1.0_f32);
    for _ in 0..12 {
        let middle = (inside + outside) / 2.0;
        if fits(at(middle)) {
            inside = middle;
        } else {
            outside = middle;
        }
    }
    clamp(at(inside))
}

fn clamp(color: Color) -> Color {
    let [r, g, b, a] = color.components;
    Color::new([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)])
}

/// `top` painted over the opaque `bottom`.
fn over(top: Color, bottom: Color) -> Color {
    let alpha = top.components[3].clamp(0.0, 1.0);
    let mix = |t: f32, b: f32| t * alpha + b * (1.0 - alpha);
    let [tr, tg, tb, _] = top.components;
    let [br, bg, bb, _] = bottom.components;
    Color::new([mix(tr, br), mix(tg, bg), mix(tb, bb), 1.0])
}

/// The WCAG contrast ratio of two opaque colours (1 to 21).
pub(crate) fn contrast(one: Color, other: Color) -> f32 {
    let (one, other) = (luminance(one), luminance(other));
    (one.max(other) + 0.05) / (one.min(other) + 0.05)
}

fn luminance(color: Color) -> f32 {
    let [r, g, b, _] = color.components;
    AlphaColor::<Srgb>::new([r, g, b, 1.0]).discard_alpha().relative_luminance()
}

/// `color` moved in lightness (towards white or black, whichever leaves more room against `under`) just far
/// enough for `min` contrast; white or black when even they do not reach it.
fn with_contrast(color: Color, under: Color, min: f32) -> Color {
    if contrast(color, under) >= min {
        return color;
    }
    let alpha = color.components[3];
    let towards_white = contrast(Color::WHITE, under) >= contrast(Color::BLACK, under);
    let end = if towards_white { 1.0 } else { 0.0 };
    let [l, a, b] = color.convert::<Oklab>().components_rgb();
    let at = |t: f32| from_oklab(l + (end - l) * t, a, b, alpha);
    if contrast(at(1.0), under) < min {
        return at(1.0);
    }
    let (mut short, mut enough) = (0.0_f32, 1.0_f32);
    for _ in 0..16 {
        let middle = (short + enough) / 2.0;
        if contrast(at(middle), under) >= min {
            enough = middle;
        } else {
            short = middle;
        }
    }
    at(enough)
}

/// What lies under a point, as far as the wrapper saw it painted.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Under {
    /// A surface in its new colour, and whether it was mirrored.
    Surface(Color, bool),
    Picture,
}

/// A [`PaintScene`] that recolours everything passing through it with a [`Recolor`] and hands it on.
pub(crate) struct DarkScene<'s, S> {
    inner: &'s mut S,
    recolor: Recolor,
    /// Fills no wider or taller than this (device pixels) count as lines.
    thin: f64,
    /// Surfaces in paint order (device space), to find what lies under text.
    under: Vec<(Rect, Under)>,
}

impl<'s, S: PaintScene> DarkScene<'s, S> {
    /// `scale`: device pixels per CSS pixel, so that a 1-px border counts as a line at any scale.
    pub(crate) fn new(inner: &'s mut S, recolor: Recolor, scale: f64) -> Self {
        Self { inner, recolor, thin: 3.0 * scale, under: Vec::new() }
    }

    /// Whether a fill of `shape` (bounding box `rect` in device pixels) is a line: thin, or an outline that
    /// covers little of its box (the edges of a border, filled as one path).
    fn is_line(&self, shape: &impl Shape, rect: Rect) -> bool {
        let own = shape.bounding_box().area();
        rect.width().min(rect.height()) <= self.thin || (own > 0.0 && shape.area().abs() < own / 2.0)
    }

    /// The surface under `point`: the last one painted there, the canvas when none.
    fn under(&self, point: Point) -> Under {
        self.under
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(point))
            .map_or(Under::Surface(self.recolor.canvas(), true), |(_, under)| *under)
    }

    /// The colour of a line in `color` around `rect`, after the surface under its middle.
    fn line(&self, color: Color, rect: Rect) -> Color {
        match self.under(rect.center()) {
            Under::Surface(_, mirrored) => self.recolor.line(color, mirrored),
            Under::Picture => color,
        }
    }

    fn note(&mut self, rect: Rect, under: Under) {
        // Translucent washes do not change what text stands on enough to count.
        if let Under::Surface(color, _) = under
            && color.components[3] < 0.5
        {
            return;
        }
        self.under.push((rect, under));
    }
}

/// `gradient` with every stop passed through `map`.
fn gradient_with(gradient: &Gradient, map: impl Fn(Color) -> Color) -> Gradient {
    let mut gradient = gradient.clone();
    for stop in gradient.stops.0.iter_mut() {
        stop.color = DynamicColor::from_alpha_color(map(stop.color.to_alpha_color::<Srgb>()));
    }
    gradient
}

/// The middle colour of a gradient: what text over it stands on, roughly.
fn gradient_middle(gradient: &Gradient) -> Option<Color> {
    let stops = &gradient.stops.0;
    stops.get(stops.len() / 2).map(|stop| stop.color.to_alpha_color::<Srgb>())
}

impl<S: PaintScene> RenderContext for DarkScene<'_, S> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<ResourceId, anyrender::RegisterResourceError> {
        self.inner.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        self.inner.unregister_resource(resource_id);
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        self.inner.renderer_specific_context()
    }
}

impl<S: PaintScene> PaintScene for DarkScene<'_, S> {
    fn reset(&mut self) {
        self.under.clear();
        self.inner.reset();
    }

    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        self.inner.push_layer(fill, blend, alpha, transform, clip, filter, backdrop_filter);
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        self.inner.push_clip_layer(fill, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let rect = transform.transform_rect_bbox(shape.bounding_box());
        match brush.into() {
            Paint::Solid(color) => {
                let color = self.line(color, rect);
                self.inner.stroke(style, transform, color, brush_transform, shape);
            }
            Paint::Gradient(gradient) => {
                let gradient = gradient_with(gradient, |color| self.line(color, rect));
                self.inner.stroke(style, transform, &gradient, brush_transform, shape);
            }
            other => self.inner.stroke(style, transform, other, brush_transform, shape),
        }
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let rect = transform.transform_rect_bbox(shape.bounding_box());
        let line = self.is_line(shape, rect);
        let recolor = self.recolor;
        match brush.into() {
            Paint::Solid(color) if line => {
                let color = self.line(color, rect);
                self.inner.fill(style, transform, color, brush_transform, shape);
            }
            Paint::Solid(color) => {
                let shown = recolor.surface(color);
                self.note(rect, Under::Surface(shown, mirrors(color)));
                self.inner.fill(style, transform, shown, brush_transform, shape);
            }
            Paint::Gradient(gradient) if line => {
                let gradient = gradient_with(gradient, |color| self.line(color, rect));
                self.inner.fill(style, transform, &gradient, brush_transform, shape);
            }
            Paint::Gradient(gradient) => {
                let middle = gradient_middle(gradient);
                let gradient = gradient_with(gradient, |color| recolor.surface(color));
                if let Some(middle) = middle {
                    self.note(rect, Under::Surface(recolor.surface(middle), mirrors(middle)));
                }
                self.inner.fill(style, transform, &gradient, brush_transform, shape);
            }
            Paint::Image(image) => {
                self.note(rect, Under::Picture);
                self.inner.fill(style, transform, image, brush_transform, shape);
            }
            other => self.inner.fill(style, transform, other, brush_transform, shape),
        }
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: kurbo::Vec2,
        style: impl Into<StyleRef<'a>>,
        brush: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        let brush = match brush.into() {
            Paint::Solid(color) => {
                let first = glyphs.clone().next().map_or(Point::ZERO, |glyph| {
                    transform * Point::new(f64::from(glyph.x), f64::from(glyph.y - font_size * 0.3))
                });
                match self.under(first) {
                    Under::Picture => Paint::Solid(color),
                    Under::Surface(under, _) => Paint::Solid(self.recolor.text(color, under)),
                }
            }
            // Gradient text (`background-clip: text`) and the rest are rare in mail: left as they are.
            other => other,
        };
        self.inner.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
        );
    }

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, brush: Color, radius: f64, std_dev: f64) {
        self.inner.draw_box_shadow(transform, rect, self.recolor.surface(brush), radius, std_dev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgb8(r, g, b)
    }

    /// The standard design's dark content surface and text (`#0F1011`, `#EDEEF0`).
    fn standard() -> Recolor {
        Recolor::new(rgb(0x0F, 0x10, 0x11), rgb(0xED, 0xEE, 0xF0))
    }

    fn close(one: Color, other: Color) -> bool {
        one.components.iter().zip(other.components).all(|(a, b)| (a - b).abs() < 0.01)
    }

    #[test]
    fn white_surfaces_turn_into_the_dark_canvas() {
        let recolor = standard();
        let white = recolor.surface(Color::WHITE);
        assert!(close(white, recolor.canvas()), "{white:?}");
        assert!(lightness(recolor.surface(rgb(0xF4, 0xF5, 0xF6))) < 0.3);
    }

    #[test]
    fn black_text_turns_into_the_dark_text_colour() {
        let recolor = standard();
        let black = recolor.text(Color::BLACK, recolor.surface(Color::WHITE));
        assert!(lightness(black) > 0.9, "{black:?}");
        assert!(contrast(black, recolor.canvas()) >= MIN_TEXT_CONTRAST);
    }

    #[test]
    fn dark_surfaces_and_light_text_stay() {
        let recolor = standard();
        let navy = rgb(0x1F, 0x38, 0x64);
        assert_eq!(recolor.surface(navy), navy);
        let white_on_navy = recolor.text(Color::WHITE, navy);
        assert_eq!(white_on_navy, Color::WHITE);
    }

    #[test]
    fn the_hue_stays() {
        let recolor = standard();
        let pale_yellow = rgb(0xFF, 0xF3, 0xCD);
        let dark = recolor.surface(pale_yellow);
        let hue = |color: Color| {
            let [_, a, b] = color.convert::<Oklab>().components_rgb();
            b.atan2(a)
        };
        assert!(lightness(dark) < 0.35);
        assert!((hue(dark) - hue(pale_yellow)).abs() < 0.2, "{dark:?}");
    }

    #[test]
    fn text_keeps_four_and_a_half_to_one_on_every_surface() {
        let recolor = standard();
        let texts = [Color::BLACK, rgb(0x22, 0x22, 0x22), rgb(0x62, 0x66, 0x6D), rgb(0x3B, 0x6E, 0xF0)];
        let texts = texts.into_iter().chain([rgb(0xD5, 0x0C, 0x0C), rgb(0x99, 0x99, 0x99), Color::WHITE]);
        let surfaces = [Color::WHITE, rgb(0xEE, 0xF1, 0xF5), rgb(0xF8, 0xF9, 0xFA), rgb(0x1F, 0x38, 0x64)];
        let surfaces = surfaces.into_iter().chain([rgb(0xD5, 0x0C, 0x0C), rgb(0x77, 0x77, 0x77), Color::BLACK]);
        let surfaces: Vec<Color> = surfaces.collect();
        for text in texts {
            for &surface in &surfaces {
                let under = recolor.surface(surface);
                let shown = recolor.text(text, under);
                assert!(
                    contrast(shown, under) >= MIN_TEXT_CONTRAST - 0.01,
                    "{text:?} on {surface:?}: {:.2}",
                    contrast(shown, under)
                );
            }
        }
    }

    #[test]
    fn lines_follow_their_surface() {
        let recolor = standard();
        assert!(lightness(recolor.line(Color::BLACK, true)) > 0.9, "black rule on white paper");
        assert!(lightness(recolor.line(rgb(0xDD, 0xDD, 0xDD), true)) < 0.3, "light rule on white paper");
        let dark_border = rgb(0x2A, 0x2C, 0x31);
        assert_eq!(recolor.line(dark_border, false), dark_border, "a dark mail's own border stays");
    }

    #[test]
    fn contrast_follows_wcag() {
        assert!((contrast(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.01);
        assert!((contrast(rgb(0x77, 0x77, 0x77), Color::WHITE) - 4.48).abs() < 0.02);
    }
}
