//! A map made of single tiles that is moved and zoomed (issue #357, task KO2.0): the map of all
//! contacts. The arithmetic of Web Mercator as the tiles of OpenStreetMap number them
//! ([`world_pixel`], [`geo_at`], [`visible_tiles`]), the view that shows a set of points ([`fit`]),
//! nearby pins put together ([`clusters`]), and the widget ([`tile_map`]) that draws tiles, pins
//! and clusters and says when the person drags, scrolls, double-clicks or clicks a pin.
//!
//! Nothing here talks to a map service: the app asks the core for each tile (`calendar.map_tile`)
//! and hands the pictures in; a tile not there yet is a plain surface.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::advanced::image::Handle;
use iced::mouse::{self, Cursor, ScrollDelta};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, Theme, alignment, window};

use crate::theme::palette;

/// The edge of a tile in logical pixels.
pub const TILE: f64 = 256.0;

/// The widest view: the whole world in a small window.
pub const MIN_ZOOM: u8 = 2;

/// The closest view: single houses, as deep as `calendar.map_tile` gives.
pub const MAX_ZOOM: u8 = 19;

/// The closest view [`fit`] chooses for a set of points: a street, not a doorstep.
pub const FIT_ZOOM: u8 = 14;

/// The furthest north and south a map shows (Web Mercator ends there).
pub const MAX_LAT: f64 = 85.0511;

/// Pins closer than this on the screen become one cluster, in logical pixels.
pub const CLUSTER_GAP: f32 = 48.0;

/// The radius of a pin with initials.
const PIN_RADIUS: f32 = 14.0;

/// The radius of a cluster with its count.
const CLUSTER_RADIUS: f32 = 17.0;

/// Space kept free at the edges when [`fit`] frames points.
const FIT_MARGIN: f64 = 48.0;

/// A press that moves less than this is a click, not a drag.
const CLICK_SLOP: f32 = 4.0;

/// Two clicks within this time and [`CLICK_SLOP`] × 2 are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Scrolled pixels (a touchpad) that make one step of zoom.
const WHEEL_STEP: f32 = 60.0;

/// One tile: zoom, column, row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// What the map looks at: the point in the middle and the zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub lat: f64,
    pub lon: f64,
    pub zoom: u8,
}

impl Camera {
    /// A camera at `lat`/`lon` and `zoom`, kept on the map: latitude within ±[`MAX_LAT`], longitude
    /// wrapped around the world, zoom within [`MIN_ZOOM`]..=[`MAX_ZOOM`].
    pub fn new(lat: f64, lon: f64, zoom: u8) -> Camera {
        let lat = if lat.is_finite() { lat.clamp(-MAX_LAT, MAX_LAT) } else { 0.0 };
        let lon = if lon.is_finite() { (lon + 180.0).rem_euclid(360.0) - 180.0 } else { 0.0 };
        Camera { lat, lon, zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM) }
    }

    /// The whole world, as a map without points starts: zoom [`MIN_ZOOM`] at 30° N, 10° E (Europe,
    /// Africa and the Americas' east in a wide window).
    pub fn world() -> Camera {
        Camera::new(30.0, 10.0, MIN_ZOOM)
    }

    /// The point in the middle in pixels of the world at the camera's zoom.
    fn center(&self) -> (f64, f64) {
        world_pixel(self.lat, self.lon, self.zoom)
    }

    /// One step closer (`steps` > 0) or further (`steps` < 0), keeping the point under `at` (a
    /// position in a view of `size`) where it is.
    pub fn zoomed(&self, steps: i32, at: Point, size: Size) -> Camera {
        let zoom = (i32::from(self.zoom) + steps).clamp(i32::from(MIN_ZOOM), i32::from(MAX_ZOOM)) as u8;
        if zoom == self.zoom {
            return *self;
        }
        let (cx, cy) = self.center();
        let (dx, dy) = (f64::from(at.x - size.width / 2.0), f64::from(at.y - size.height / 2.0));
        let scale = 2f64.powi(i32::from(zoom) - i32::from(self.zoom));
        let (px, py) = ((cx + dx) * scale, (cy + dy) * scale);
        let (lat, lon) = geo_at(px - dx, py - dy, zoom);
        Camera::new(lat, lon, zoom)
    }

    /// Moved by `dx`/`dy` pixels on the screen (the map follows the pointer: dragging to the right
    /// shows what lies west).
    pub fn moved(&self, dx: f32, dy: f32) -> Camera {
        let (cx, cy) = self.center();
        let (lat, lon) = geo_at(cx - f64::from(dx), cy - f64::from(dy), self.zoom);
        Camera::new(lat, lon, self.zoom)
    }

    /// Where `lat`/`lon` is in a view of `size`, the copy of the world nearest the middle.
    pub fn screen(&self, lat: f64, lon: f64, size: Size) -> Point {
        let (cx, cy) = self.center();
        let (mut x, y) = world_pixel(lat, lon, self.zoom);
        let world = world_size(self.zoom);
        x += ((cx - x) / world).round() * world;
        Point::new((x - cx + f64::from(size.width) / 2.0) as f32, (y - cy + f64::from(size.height) / 2.0) as f32)
    }
}

/// The edge of the world at `zoom` in pixels.
fn world_size(zoom: u8) -> f64 {
    TILE * f64::from(1u32 << zoom)
}

/// Where `lat`/`lon` lies in the world at `zoom`, in pixels of [`TILE`] tiles (Web Mercator, as
/// OpenStreetMap numbers its tiles): `(0, 0)` is the top left corner at 180° W and 85.0511° N.
pub fn world_pixel(lat: f64, lon: f64, zoom: u8) -> (f64, f64) {
    let size = world_size(zoom);
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.0) / 360.0 * size;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * size;
    (x, y)
}

/// The point at pixel `x`/`y` of the world at `zoom` ([`world_pixel`] turned around); the
/// longitude wrapped around the world.
pub fn geo_at(x: f64, y: f64, zoom: u8) -> (f64, f64) {
    let size = world_size(zoom);
    let lon = (x / size * 360.0).rem_euclid(360.0) - 180.0;
    let n = std::f64::consts::PI * (1.0 - 2.0 * y / size);
    let lat = n.sinh().atan().to_degrees();
    (lat.clamp(-MAX_LAT, MAX_LAT), lon)
}

/// The tiles a view of `size` at `camera` shows, with the top left corner of each on the screen:
/// columns wrap around the world, rows beyond its top or bottom are left out.
pub fn visible_tiles(camera: Camera, size: Size) -> Vec<(TileKey, Point)> {
    let (cx, cy) = camera.center();
    let left = cx - f64::from(size.width) / 2.0;
    let top = cy - f64::from(size.height) / 2.0;
    let count = 1i64 << camera.zoom;
    let first = |start: f64| (start / TILE).floor() as i64;
    let last = |start: f64, extent: f32| ((start + f64::from(extent) - 1.0) / TILE).floor() as i64;
    let mut tiles = Vec::new();
    for row in (first(top)..=last(top, size.height)).filter(|row| (0..count).contains(row)) {
        for column in first(left)..=last(left, size.width) {
            let key = TileKey { z: camera.zoom, x: column.rem_euclid(count) as u32, y: row as u32 };
            let at = Point::new((column as f64 * TILE - left) as f32, (row as f64 * TILE - top) as f32);
            tiles.push((key, at));
        }
    }
    tiles
}

/// The camera that shows every point of `points` (latitude, longitude) in a view of `size`, at most
/// at `max_zoom` ([`FIT_ZOOM`] for a whole selection): the closest zoom at which they fit with
/// [`FIT_MARGIN`] around them, their middle in the middle. Without points the world
/// ([`Camera::world`]).
pub fn fit(points: &[(f64, f64)], size: Size, max_zoom: u8) -> Camera {
    if points.is_empty() {
        return Camera::world();
    }
    let at_zero: Vec<(f64, f64)> = points.iter().map(|(lat, lon)| world_pixel(*lat, *lon, 0)).collect();
    let (min_x, max_x) = at_zero.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (x, _)| (lo.min(*x), hi.max(*x)));
    let (min_y, max_y) = at_zero.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, y)| (lo.min(*y), hi.max(*y)));
    let room_x = (f64::from(size.width) - 2.0 * FIT_MARGIN).max(1.0);
    let room_y = (f64::from(size.height) - 2.0 * FIT_MARGIN).max(1.0);
    let max_zoom = max_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let zoom = (MIN_ZOOM..=max_zoom)
        .rev()
        .find(|zoom| {
            let scale = f64::from(1u32 << zoom);
            (max_x - min_x) * scale <= room_x && (max_y - min_y) * scale <= room_y
        })
        .unwrap_or(MIN_ZOOM);
    let (lat, lon) = geo_at((min_x + max_x) / 2.0, (min_y + max_y) / 2.0, 0);
    Camera::new(lat, lon, zoom)
}

/// Pins put together on the screen ([`clusters`]): the indices of its points and where it is
/// drawn (the middle of them).
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub members: Vec<usize>,
    pub at: Point,
}

/// The points of `points` (latitude, longitude) as a view of `size` at `camera` shows them: points
/// closer than [`CLUSTER_GAP`] to a cluster's first point join it, in their order (greedy, so the
/// same points always give the same clusters). Points well outside the view are left out.
pub fn clusters(points: &[(f64, f64)], camera: Camera, size: Size) -> Vec<Cluster> {
    let cell = |p: Point| ((p.x / CLUSTER_GAP).floor() as i32, (p.y / CLUSTER_GAP).floor() as i32);
    let mut found: Vec<(Point, Cluster)> = Vec::new();
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let inside = |p: Point| {
        p.x >= -CLUSTER_GAP
            && p.y >= -CLUSTER_GAP
            && p.x <= size.width + CLUSTER_GAP
            && p.y <= size.height + CLUSTER_GAP
    };
    for (index, (lat, lon)) in points.iter().enumerate() {
        let p = camera.screen(*lat, *lon, size);
        if !inside(p) {
            continue;
        }
        let (cx, cy) = cell(p);
        let near = (cx - 1..=cx + 1)
            .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
            .filter_map(|key| grid.get(&key))
            .flatten()
            .copied()
            .filter(|&c| p.distance(found[c].0) < CLUSTER_GAP)
            .min();
        match near {
            Some(c) => found[c].1.members.push(index),
            None => {
                grid.entry((cx, cy)).or_default().push(found.len());
                found.push((p, Cluster { members: vec![index], at: p }));
            }
        }
    }
    found
        .into_iter()
        .map(|(_, mut cluster)| {
            let n = cluster.members.len() as f32;
            let (sx, sy) = cluster.members.iter().fold((0.0, 0.0), |(sx, sy), &i| {
                let p = camera.screen(points[i].0, points[i].1, size);
                (sx + p.x, sy + p.y)
            });
            cluster.at = Point::new(sx / n, sy / n);
            cluster
        })
        .collect()
}

/// One pin of the map: whose, where, the initials it shows and its colour.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPin {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    pub initials: String,
    pub color: Color,
}

/// What the person did on the map.
#[derive(Debug, Clone, PartialEq)]
pub enum MapEvent {
    /// The view changed (dragged, scrolled, double-clicked, or the widget got another size): the
    /// new camera and the size of the widget.
    Viewport(Camera, Size),
    /// A pin was clicked.
    Pin(String),
    /// A cluster was clicked: the ids of its pins.
    Cluster(Vec<String>),
}

/// The map: `tiles` (by [`TileKey`], those not there yet drawn as a plain surface), `pins`
/// (clustered, [`clusters`]) with a ring around `selected`, at `camera`, filling its space.
/// `on_event` turns what the person does into the app's message.
pub fn tile_map<'a, Message: 'a>(
    camera: Camera,
    tiles: &'a HashMap<TileKey, Handle>,
    pins: &'a [MapPin],
    selected: Option<&'a str>,
    on_event: impl Fn(MapEvent) -> Message + 'a,
) -> Element<'a, Message> {
    Canvas::new(TileMap { camera, tiles, pins, selected, on_event: Box::new(on_event) })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct TileMap<'a, Message> {
    camera: Camera,
    tiles: &'a HashMap<TileKey, Handle>,
    pins: &'a [MapPin],
    selected: Option<&'a str>,
    on_event: Box<dyn Fn(MapEvent) -> Message + 'a>,
}

/// What the widget remembers between events.
#[derive(Debug, Default)]
struct State {
    /// The size last told to the app.
    size: Option<Size>,
    /// A press not yet released: where, the camera then, and whether it moved beyond a click.
    press: Option<(Point, Camera, bool)>,
    /// The last click, for a double click.
    last_click: Option<(Instant, Point)>,
    /// Scrolled pixels not yet a step.
    wheel: f32,
}

impl<Message> TileMap<'_, Message> {
    fn points(&self) -> Vec<(f64, f64)> {
        self.pins.iter().map(|pin| (pin.lat, pin.lon)).collect()
    }

    /// The cluster under `at`, in a view of `size`.
    fn hit(&self, at: Point, size: Size) -> Option<Cluster> {
        clusters(&self.points(), self.camera, size).into_iter().find(|cluster| {
            let radius = if cluster.members.len() > 1 { CLUSTER_RADIUS } else { PIN_RADIUS };
            cluster.at.distance(at) <= radius + 2.0
        })
    }

    fn viewport(&self, camera: Camera, size: Size) -> Option<canvas::Action<Message>> {
        Some(canvas::Action::publish((self.on_event)(MapEvent::Viewport(camera, size))).and_capture())
    }
}

impl<Message> canvas::Program<Message> for TileMap<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        match event {
            canvas::Event::Window(window::Event::RedrawRequested(_)) if state.size != Some(size) => {
                state.size = Some(size);
                Some(canvas::Action::publish((self.on_event)(MapEvent::Viewport(self.camera, size))))
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                let now = Instant::now();
                let double = state.last_click.is_some_and(|(when, where_)| {
                    now.duration_since(when) < DOUBLE_CLICK && where_.distance(at) < CLICK_SLOP * 2.0
                });
                if double {
                    state.last_click = None;
                    state.press = None;
                    return self.viewport(self.camera.zoomed(1, at, size), size);
                }
                state.press = Some((at, self.camera, false));
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (origin, camera, moved) = state.press.as_mut()?;
                let at = cursor.position_from(bounds.position())?;
                if !*moved && at.distance(*origin) < CLICK_SLOP {
                    return None;
                }
                *moved = true;
                let camera = camera.moved(at.x - origin.x, at.y - origin.y);
                self.viewport(camera, size)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let (origin, _, moved) = state.press.take()?;
                if moved {
                    return Some(canvas::Action::capture());
                }
                state.last_click = Some((Instant::now(), origin));
                let cluster = self.hit(origin, size)?;
                let ids: Vec<String> = cluster.members.iter().map(|&i| self.pins[i].id.clone()).collect();
                let event = match ids.as_slice() {
                    [one] => MapEvent::Pin(one.clone()),
                    _ => MapEvent::Cluster(ids),
                };
                Some(canvas::Action::publish((self.on_event)(event)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let at = cursor.position_in(bounds)?;
                let steps = match delta {
                    ScrollDelta::Lines { y, .. } => y.signum() as i32,
                    ScrollDelta::Pixels { y, .. } => {
                        state.wheel += y;
                        let steps = (state.wheel / WHEEL_STEP).trunc();
                        state.wheel -= steps * WHEEL_STEP;
                        steps as i32
                    }
                };
                if steps == 0 {
                    return Some(canvas::Action::capture());
                }
                self.viewport(self.camera.zoomed(steps, at, size), size)
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let p = palette(theme);
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        frame.fill_rectangle(Point::ORIGIN, size, p.raised);
        let edge = TILE as f32;
        for (key, at) in visible_tiles(self.camera, size) {
            match self.tiles.get(&key) {
                Some(handle) => frame.draw_image(Rectangle::new(at, Size::new(edge, edge)), handle),
                None => frame.stroke(
                    &Path::rectangle(at, Size::new(edge, edge)),
                    Stroke::default().with_color(p.line).with_width(1.0),
                ),
            }
        }
        for cluster in clusters(&self.points(), self.camera, size) {
            match cluster.members.as_slice() {
                [one] => {
                    let pin = &self.pins[*one];
                    let selected = self.selected == Some(pin.id.as_str());
                    if selected {
                        frame.fill(&Path::circle(cluster.at, PIN_RADIUS + 4.0), p.focus_ring);
                    }
                    frame.fill(&Path::circle(cluster.at, PIN_RADIUS + 1.5), p.bg);
                    frame.fill(&Path::circle(cluster.at, PIN_RADIUS), pin.color);
                    frame.fill_text(label(&pin.initials, cluster.at, Color::WHITE, 11.0));
                }
                members => {
                    let halo = Color { a: 0.25, ..p.accent };
                    frame.fill(&Path::circle(cluster.at, CLUSTER_RADIUS + 5.0), halo);
                    frame.fill(&Path::circle(cluster.at, CLUSTER_RADIUS), p.accent);
                    let ringed = self.selected.is_some_and(|id| members.iter().any(|&i| self.pins[i].id == id));
                    if ringed {
                        frame.stroke(
                            &Path::circle(cluster.at, CLUSTER_RADIUS + 3.0),
                            Stroke::default().with_color(p.focus_ring).with_width(2.0),
                        );
                    }
                    frame.fill_text(label(&members.len().to_string(), cluster.at, p.on_accent, 12.0));
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: Cursor) -> mouse::Interaction {
        if state.press.is_some_and(|(_, _, moved)| moved) {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_in(bounds) {
            Some(at) if self.hit(at, bounds.size()).is_some() => mouse::Interaction::Pointer,
            Some(_) => mouse::Interaction::Grab,
            None => mouse::Interaction::default(),
        }
    }
}

/// A short text centred on `at`: a pin's initials, a cluster's count.
fn label(content: &str, at: Point, color: Color, size: f32) -> Text {
    Text {
        content: content.to_string(),
        position: at,
        color,
        size: Pixels(size),
        font: crate::theme::font_semibold(),
        align_x: iced::widget::text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..Text::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_of(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
        let (x, y) = world_pixel(lat, lon, zoom);
        ((x / TILE) as u32, (y / TILE) as u32)
    }

    #[test]
    fn points_lie_on_the_tiles_openstreetmap_numbers() {
        assert_eq!(tile_of(52.52, 13.405, 10), (550, 335), "Berlin");
        assert_eq!(tile_of(-33.8688, 151.2093, 10), (942, 614), "Sydney");
        let (lat, lon) = geo_at(world_pixel(-33.8688, 151.2093, 12).0, world_pixel(-33.8688, 151.2093, 12).1, 12);
        assert!((lat + 33.8688).abs() < 1e-9 && (lon - 151.2093).abs() < 1e-9, "and back: {lat} {lon}");
        // The poles are cut where Web Mercator ends.
        assert_eq!(world_pixel(90.0, 0.0, 0).1.round(), 0.0);
        assert_eq!(geo_at(0.0, -500.0, 0).0, MAX_LAT);
    }

    #[test]
    fn a_camera_stays_on_the_map_and_wraps_at_the_date_line() {
        assert_eq!(Camera::new(89.0, 190.0, 25), Camera { lat: MAX_LAT, lon: -170.0, zoom: MAX_ZOOM });
        assert_eq!(Camera::new(0.0, 0.0, 0).zoom, MIN_ZOOM);
        // Dragging east past the date line comes back from the west.
        let pacific = Camera::new(0.0, 179.0, 4).moved(-200.0, 0.0);
        assert!((pacific.lon - (179.0 + 200.0 / 4096.0 * 360.0 - 360.0)).abs() < 1e-6, "{pacific:?}");
        // The tiles of a view across the date line wrap around.
        let size = Size::new(600.0, 300.0);
        let tiles = visible_tiles(Camera::new(0.0, 180.0, 3), size);
        let columns: Vec<u32> = tiles.iter().filter(|(key, _)| key.y == 3).map(|(key, _)| key.x).collect();
        assert_eq!(columns, [6, 7, 0, 1]);
        // At the pole no row beyond the edge.
        let north = visible_tiles(Camera::new(MAX_LAT, 0.0, 2), size);
        assert!(north.iter().all(|(key, _)| key.y < 4));
        // A point on the other side of the date line is drawn near, not a world away.
        let at = Camera::new(0.0, 179.0, 4).screen(0.0, -179.0, size);
        assert!((at.x - (300.0 + 2.0 / 360.0 * 4096.0)).abs() < 1.0, "{at:?}");
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let size = Size::new(800.0, 600.0);
        let camera = Camera::new(52.52, 13.405, 10);
        let at = Point::new(600.0, 150.0);
        let before = geo_at(
            camera.center().0 + f64::from(at.x - 400.0),
            camera.center().1 + f64::from(at.y - 300.0),
            camera.zoom,
        );
        let closer = camera.zoomed(2, at, size);
        assert_eq!(closer.zoom, 12);
        let after = closer.screen(before.0, before.1, size);
        assert!(after.distance(at) < 0.5, "{after:?}");
        assert_eq!(camera.zoomed(20, at, size).zoom, MAX_ZOOM);
        assert_eq!(Camera::new(0.0, 0.0, MIN_ZOOM).zoomed(-1, at, size).zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_frames_every_point() {
        let size = Size::new(800.0, 600.0);
        assert_eq!(fit(&[], size, FIT_ZOOM), Camera::world());
        assert_eq!(fit(&[(52.52, 13.405)], size, FIT_ZOOM).zoom, FIT_ZOOM, "one point: a street");
        let germany = [(53.55, 9.99), (48.14, 11.58), (50.94, 6.96), (52.52, 13.405)];
        let camera = fit(&germany, size, FIT_ZOOM);
        assert_eq!(camera.zoom, 6);
        for (lat, lon) in germany {
            let at = camera.screen(lat, lon, size);
            assert!(at.x >= 48.0 && at.x <= 752.0 && at.y >= 48.0 && at.y <= 552.0, "{at:?}");
        }
        assert_eq!(fit(&[(52.52, 13.405), (52.52, 13.405)], size, MAX_ZOOM).zoom, MAX_ZOOM);
    }

    #[test]
    fn near_pins_become_one_cluster() {
        let size = Size::new(800.0, 600.0);
        let points = [(52.52, 13.405), (52.53, 13.41), (48.14, 11.58), (-33.87, 151.21)];
        let far = clusters(&points, Camera::new(51.0, 12.0, 6), size);
        let members: Vec<Vec<usize>> = far.iter().map(|c| c.members.clone()).collect();
        assert_eq!(members, [vec![0, 1], vec![2]], "Berlin twice together, Munich alone, Sydney out of view");
        let close = clusters(&points, Camera::new(52.525, 13.407, 14), size);
        assert_eq!(close.len(), 2, "apart when closer");
        let (a, b) = (close[0].at, close[1].at);
        assert!(a.distance(b) >= CLUSTER_GAP);
    }
}
