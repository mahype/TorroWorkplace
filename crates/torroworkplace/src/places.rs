//! Places for the event editor (task TE.5, `docs/termin-editor.md` "Ort und Karte"): the place
//! search of `calendar.search_places` and the small map of `calendar.map_preview`, asked of the
//! services the settings name (`maps`, Photon and the OpenStreetMap tiles unless changed).
//!
//! The core checks the limits and that a person turned maps on before it comes here
//! (`Core::places`); with maps off nothing in this module runs, so nothing is sent. Here:
//!
//! - **Earlier places first:** [`recent_places`] picks matching locations of the person's own events
//!   (the core hands in only events of calendars the asker may read), without the network.
//! - **Throttled** ([`Throttle`]): at most one search a second for the whole suite, as the
//!   services' rules ask. The person (the suite's apps, the app key) waits for the next second
//!   instead of being refused; clients share a small budget ([`CLIENT_BUDGET`] a minute) and get
//!   `busy`, so an agent cannot use up the searches of the person. A service that answers 403 or
//!   429 is not asked for an hour ([`PAUSE`]), by anyone: the refusal is the service's, whoever
//!   caused it, and asking on would risk a ban for every installation.
//! - **Kept:** search answers for a day in memory, map tiles for at least a week on disk in the
//!   service's state directory ([`TileCache`], directory 0700, files 0600: they show where the
//!   person was), refreshed with `ETag`/`If-None-Match`. Nothing is loaded ahead.
//! - **Quiet:** neither the query nor a point (a tile address holds one) ever goes into a message
//!   or a log. Errors carry fixed texts ([`Failure`]); a `reqwest::Error` loses its URL
//!   ([`reqwest::Error::without_url`]) before anything looks at it.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use chrono::{DateTime, TimeDelta, Utc};
use image::{ImageFormat, Rgba, RgbaImage};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use torro_core::AppId;
use torro_core::i18n::tr;
use torro_core::paths::AppPaths;
use torro_core::settings::{GeocoderKind, Maps};
use torro_ipc::{Place, PlaceKind, Response};
use torro_pim::model::Geo;
use torro_pim::{Event, EventTime};

/// The user agent both services see, as the OpenStreetMap Foundation's rules ask: who we are and
/// where to find us.
const USER_AGENT: &str =
    concat!("TorroWorkplace/", env!("CARGO_PKG_VERSION"), " (+https://github.com/mahype/TorroWorkplace)");

/// Least time between two searches of the whole suite (Nominatim's rule; Photon asks for
/// "reasonable" use).
const SEARCH_GAP: Duration = Duration::from_secs(1);

/// Searches, and maps, all clients together may ask for per [`CLIENT_WINDOW`].
pub const CLIENT_BUDGET: usize = 10;

/// The time [`CLIENT_BUDGET`] counts over.
const CLIENT_WINDOW: Duration = Duration::from_secs(60);

/// How long a service that answered 403 or 429 is left alone.
pub const PAUSE: Duration = Duration::from_secs(60 * 60);

/// How long a search answer is kept in memory.
const ANSWER_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// How many search answers are kept; the oldest goes first.
const MAX_ANSWERS: usize = 64;

/// How long a map tile on disk is used without asking the service again (its rules ask for at
/// least seven days).
pub const TILE_FRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How many places the search service is asked for.
const SEARCH_LIMIT: usize = 6;

/// How many earlier places come first.
const MAX_RECENT: usize = 3;

/// How many places an answer holds at most.
pub const MAX_PLACES: usize = 8;

/// The zoom of the map in logical pixels: streets and house numbers.
pub const MAP_ZOOM: u8 = 16;

/// Pixels per logical pixel of the map picture: drawn for screens with twice the density.
pub const DENSITY: u32 = 2;

/// The zoom of the tiles a map is put together from: one step deeper than [`MAP_ZOOM`] gives
/// [`DENSITY`] 2 at the same scale (as Leaflet does for such screens).
const TILE_ZOOM: u8 = MAP_ZOOM + 1;

/// The edge of a map tile in pixels.
const TILE_SIZE: u32 = 256;

/// The colour where no tile is (beyond the edge of the world): OpenStreetMap's land.
const BACKGROUND: Rgba<u8> = Rgba([242, 239, 233, 255]);

/// How far back and ahead of now [`recent_places`] looks, inside what the calendar service keeps
/// after a sync (90 days back, a year ahead), so the events come from its cache.
const RECENT_DAYS: i64 = 60;

/// Who asks: the person (the suite's own apps and the app key, `Reach::Suite`) or a client key
/// (an agent, a script). The person goes first ([`Throttle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asker {
    Person,
    Client,
}

/// Which service a request goes to; each has its own [`Throttle`] and its own texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Search,
    Map,
}

/// Why a service gave no answer, without anything of the request in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// Too many requests just now ([`Throttle`]).
    Busy,
    /// The service refused (403/429) within the last [`PAUSE`].
    Paused,
    /// It cannot be reached.
    Offline,
    /// It answered with something else than expected, or the picture could not be made.
    Broken,
    /// The address in the settings is not one to ask.
    Address,
}

impl Failure {
    /// The answer to the client: a fixed code and a fixed text.
    fn response(self, service: Service) -> Response {
        let (code, message) = match (self, service) {
            (Failure::Busy, Service::Search) => ("busy", tr("Too many place searches. Try again in a moment.")),
            (Failure::Busy, Service::Map) => ("busy", tr("Too many maps at once. Try again in a moment.")),
            (Failure::Paused, Service::Search) => {
                ("busy", tr("The place search service asked to wait. Try again later."))
            }
            (Failure::Paused, Service::Map) => ("busy", tr("The map service asked to wait. Try again later.")),
            (Failure::Offline, Service::Search) => ("offline", tr("The place search service cannot be reached.")),
            (Failure::Offline, Service::Map) => ("offline", tr("The map service cannot be reached.")),
            (Failure::Broken, Service::Search) => ("error", tr("The place search service gave no usable answer.")),
            (Failure::Broken, Service::Map) => ("error", tr("The map could not be drawn.")),
            (Failure::Address, _) => ("error", tr("The address of the service in the settings cannot be used.")),
        };
        Response::Error { code: code.into(), message: message.to_string() }
    }
}

/// What the throttle says to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// Ask now.
    Go,
    /// Ask after this long (the person's request, waiting for the next free second).
    After(Duration),
    /// Not now: too many requests.
    Busy,
    /// Not now: the service refused within the last [`PAUSE`].
    Paused,
}

/// How often one service may be asked (task TE.5). The time comes in as `now`, so the rules are
/// tested without a clock.
#[derive(Debug)]
struct Throttle {
    /// Least time between two requests of anyone; zero for none.
    gap: Duration,
    /// When the next request may go; `None` before the first.
    next_free: Option<Instant>,
    /// When clients asked within the last [`CLIENT_WINDOW`], oldest first.
    clients: VecDeque<Instant>,
    /// Until when the service is left alone after a 403 or 429.
    paused_until: Option<Instant>,
}

impl Throttle {
    fn new(gap: Duration) -> Throttle {
        Throttle { gap, next_free: None, clients: VecDeque::new(), paused_until: None }
    }

    /// Whether a request of `asker` may go at `now`, and takes its place when it may:
    ///
    /// - while paused ([`Throttle::pause`]): [`Turn::Paused`] for everyone;
    /// - a client: [`Turn::Go`] only when the gap since the last request is over and the clients
    ///   asked fewer than [`CLIENT_BUDGET`] times within [`CLIENT_WINDOW`], else [`Turn::Busy`];
    /// - the person: [`Turn::Go`] when the gap is over, else [`Turn::After`] the rest of it, so a
    ///   client's request never refuses the person. Only one request waits at a time: a second one
    ///   of the person within the gap is [`Turn::Busy`].
    fn turn(&mut self, now: Instant, asker: Asker) -> Turn {
        if self.paused_until.is_some_and(|until| now < until) {
            return Turn::Paused;
        }
        while self.clients.front().is_some_and(|at| now.saturating_duration_since(*at) >= CLIENT_WINDOW) {
            self.clients.pop_front();
        }
        let free = self.next_free.filter(|free| *free > now);
        match (asker, free) {
            (Asker::Client, Some(_)) => Turn::Busy,
            (Asker::Client, None) if self.clients.len() >= CLIENT_BUDGET => Turn::Busy,
            (Asker::Client, None) => {
                self.clients.push_back(now);
                self.next_free = Some(now + self.gap);
                Turn::Go
            }
            (Asker::Person, None) => {
                self.next_free = Some(now + self.gap);
                Turn::Go
            }
            (Asker::Person, Some(free)) if free - now <= self.gap => {
                self.next_free = Some(free + self.gap);
                Turn::After(free - now)
            }
            (Asker::Person, Some(_)) => Turn::Busy,
        }
    }

    /// The service refused at `now` (403 or 429): nobody asks it for [`PAUSE`].
    fn pause(&mut self, now: Instant) {
        self.paused_until = Some(now + PAUSE);
    }
}

/// What a GET brought back.
enum Fetched {
    /// The body, with the `ETag` the service gave.
    Body { bytes: Vec<u8>, etag: Option<String> },
    /// 304: the copy we have is still good.
    NotModified,
}

/// The place search and the map of the event editor, for the core (one for the whole service).
pub struct Places {
    /// Built on first use: a client costs time every core would pay at start. `None` when none
    /// could be built.
    http: OnceLock<Option<reqwest::Client>>,
    tiles: TileCache,
    searches: Mutex<Throttle>,
    maps: Mutex<Throttle>,
    /// Search answers by [`answer_key`], with when they came.
    answers: Mutex<HashMap<String, (Instant, Vec<Place>)>>,
}

impl Places {
    /// The service's places, with the tile cache in its state directory (`tiles`); in tests
    /// without a tile cache, so no test touches the user's directories.
    pub fn system() -> Places {
        if cfg!(test) {
            return Places::new(None);
        }
        Places::new(AppPaths::for_app(AppId::WORKPLACE).map(|paths| paths.state.join("tiles")))
    }

    /// Places with the tile cache in `tiles`, or without one (every map asks the service).
    pub fn new(tiles: Option<PathBuf>) -> Places {
        Places {
            http: OnceLock::new(),
            tiles: TileCache { dir: tiles },
            searches: Mutex::new(Throttle::new(SEARCH_GAP)),
            // Maps ask several tiles at once; only the clients' budget and the pause hold them back.
            maps: Mutex::new(Throttle::new(Duration::ZERO)),
            answers: Mutex::new(HashMap::new()),
        }
    }

    /// `calendar.search_places` with maps on (the core checked the limits and the switch):
    /// `recent` first (see [`recent_places`]), then what the search service of `maps` finds for
    /// `query` (near `near` with Photon), without repeats, at most [`MAX_PLACES`]. An answer of the
    /// last day for the same search comes from memory without asking. When the service gives no
    /// answer (`busy`, `offline`, `error`, see [`Failure`]) and there are earlier places, those are
    /// the answer; without any, the failure is.
    pub async fn search(
        &self,
        maps: &Maps,
        query: &str,
        near: Option<Geo>,
        asker: Asker,
        recent: Vec<Place>,
    ) -> Response {
        match self.found(maps, query, near, asker).await {
            Ok(found) => Response::Places { places: merged(recent, found) },
            Err(failure) if recent.is_empty() => failure.response(Service::Search),
            Err(_) => Response::Places { places: merged(recent, Vec::new()) },
        }
    }

    /// What the search service finds for `query`, from memory when asked within a day. An answer
    /// from memory asks no service, so it counts against no throttle, the clients' budget included.
    async fn found(&self, maps: &Maps, query: &str, near: Option<Geo>, asker: Asker) -> Result<Vec<Place>, Failure> {
        let lang = language();
        let key = answer_key(maps, query, near, lang);
        if let Some(places) = self.answer(&key, Instant::now()) {
            return Ok(places);
        }
        let url = search_url(maps, query, near, lang)?;
        self.wait_turn(Service::Search, asker).await?;
        let Fetched::Body { bytes, .. } = self.get(Service::Search, url, None).await? else {
            return Err(Failure::Broken);
        };
        let places = match maps.geocoder_kind {
            GeocoderKind::Photon => photon_places(&bytes)?,
            GeocoderKind::Nominatim => nominatim_places(&bytes)?,
        };
        self.keep_answer(key, places.clone(), Instant::now());
        Ok(places)
    }

    /// A kept answer for `key` younger than [`ANSWER_FOR`] at `now`.
    fn answer(&self, key: &str, now: Instant) -> Option<Vec<Place>> {
        let answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        let (at, places) = answers.get(key)?;
        (now.saturating_duration_since(*at) < ANSWER_FOR).then(|| places.clone())
    }

    /// Keeps `places` for `key`; at [`MAX_ANSWERS`] the oldest answer goes.
    fn keep_answer(&self, key: String, places: Vec<Place>, now: Instant) {
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        if answers.len() >= MAX_ANSWERS
            && !answers.contains_key(&key)
            && let Some(oldest) = answers.iter().min_by_key(|(_, (at, _))| *at).map(|(key, _)| key.clone())
        {
            answers.remove(&oldest);
        }
        answers.insert(key, (now, places));
    }

    /// `calendar.map_preview` with maps on (the core checked the limits and the switch): the
    /// tiles of `maps` around `geo` at [`MAP_ZOOM`], put together at [`DENSITY`] times
    /// `width` × `height` pixels with `geo` in the middle, darkened for `dark` ([`darken`]),
    /// answered as [`Response::Image`]. A tile the service cannot give comes from an older copy in
    /// the cache when there is one; otherwise the map fails (see [`Failure`]).
    pub async fn map(&self, maps: &Maps, geo: Geo, width: u16, height: u16, dark: bool, asker: Asker) -> Response {
        match self.drawn(maps, geo, width, height, dark, asker).await {
            Ok(png) => Response::Image { png: base64::engine::general_purpose::STANDARD.encode(png) },
            Err(failure) => failure.response(Service::Map),
        }
    }

    /// The PNG of [`Places::map`].
    async fn drawn(
        &self,
        maps: &Maps,
        geo: Geo,
        width: u16,
        height: u16,
        dark: bool,
        asker: Asker,
    ) -> Result<Vec<u8>, Failure> {
        let view = View::new(geo, u32::from(width) * DENSITY, u32::from(height) * DENSITY);
        let tiles = view.tiles();
        let urls = tiles.iter().map(|tile| tile_url(&maps.tiles, tile)).collect::<Result<Vec<_>, _>>()?;
        self.wait_turn(Service::Map, asker).await?;
        let mut canvas = RgbaImage::from_pixel(view.width, view.height, BACKGROUND);
        for (tile, url) in tiles.iter().zip(urls) {
            let bytes = self.tile(url).await?;
            let mut picture = image::load_from_memory(&bytes).map_err(|_| Failure::Broken)?.to_rgba8();
            if picture.dimensions() != (TILE_SIZE, TILE_SIZE) {
                // Services with larger tiles (512) draw at the same scale once shrunk.
                picture =
                    image::imageops::resize(&picture, TILE_SIZE, TILE_SIZE, image::imageops::FilterType::Triangle);
            }
            image::imageops::overlay(&mut canvas, &picture, tile.left, tile.top);
        }
        if dark {
            darken(&mut canvas);
        }
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(canvas)
            .to_rgb8()
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|_| Failure::Broken)?;
        Ok(png.into_inner())
    }

    /// The tile at `url`: from the cache while fresh, else from the service (with the cached
    /// `ETag`), else the older copy.
    async fn tile(&self, url: reqwest::Url) -> Result<Vec<u8>, Failure> {
        let now = SystemTime::now();
        let stale = match self.tiles.read(url.as_str(), now) {
            Kept::Fresh(bytes) => return Ok(bytes),
            Kept::Stale { bytes, etag } => Some((bytes, etag)),
            Kept::Missing => None,
        };
        let etag = stale.as_ref().and_then(|(_, etag)| etag.clone());
        match self.get(Service::Map, url.clone(), etag.as_deref()).await {
            Ok(Fetched::Body { bytes, etag }) => {
                self.tiles.write(url.as_str(), &bytes, etag.as_deref());
                Ok(bytes)
            }
            Ok(Fetched::NotModified) => match stale {
                Some((bytes, _)) => {
                    self.tiles.touch(url.as_str(), now);
                    Ok(bytes)
                }
                None => Err(Failure::Broken),
            },
            Err(failure) => stale.map(|(bytes, _)| bytes).ok_or(failure),
        }
    }

    /// Waits for the turn of `asker` at `service` ([`Throttle::turn`]), or says why not.
    async fn wait_turn(&self, service: Service, asker: Asker) -> Result<(), Failure> {
        let turn = self.throttle(service).lock().unwrap_or_else(PoisonError::into_inner).turn(Instant::now(), asker);
        match turn {
            Turn::Go => Ok(()),
            Turn::After(wait) => {
                tokio::time::sleep(wait).await;
                Ok(())
            }
            Turn::Busy => Err(Failure::Busy),
            Turn::Paused => Err(Failure::Paused),
        }
    }

    fn throttle(&self, service: Service) -> &Mutex<Throttle> {
        match service {
            Service::Search => &self.searches,
            Service::Map => &self.maps,
        }
    }

    /// GET `url` of `service`, with `If-None-Match: etag` when given. 403 and 429 pause the service
    /// ([`Throttle::pause`]). The URL holds the query or the point: no error keeps it.
    async fn get(&self, service: Service, url: reqwest::Url, etag: Option<&str>) -> Result<Fetched, Failure> {
        let client = self.http.get_or_init(http_client).as_ref().ok_or(Failure::Broken)?;
        let mut request = client.get(url);
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(|err| net_failure(err.without_url()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Fetched::NotModified);
        }
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            self.throttle(service).lock().unwrap_or_else(PoisonError::into_inner).pause(Instant::now());
            return Err(Failure::Paused);
        }
        if !status.is_success() {
            return Err(Failure::Broken);
        }
        let etag = response.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = response.bytes().await.map_err(|err| net_failure(err.without_url()))?;
        Ok(Fetched::Body { bytes: bytes.to_vec(), etag })
    }
}

/// The HTTP client of both services, with time limits like the calendar's
/// (`torro_pim_caldav::http_client`).
fn http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .ok()
}

/// A failed request as a [`Failure`]; `err` has lost its URL already.
fn net_failure(err: reqwest::Error) -> Failure {
    if err.is_connect() || err.is_timeout() { Failure::Offline } else { Failure::Broken }
}

/// The language the services name places in (`en` or `de`), as the suite speaks.
fn language() -> &'static str {
    match torro_core::i18n::lang() {
        torro_core::i18n::Lang::De => "de",
        torro_core::i18n::Lang::En => "en",
    }
}

/// What a search answer is kept under: service, language, the query as typed (trimmed, in lower
/// case) and the point near which, to about 100 m.
fn answer_key(maps: &Maps, query: &str, near: Option<Geo>, lang: &str) -> String {
    let near = near.map(|geo| format!("{:.3},{:.3}", geo.lat, geo.lon)).unwrap_or_default();
    let kind = match maps.geocoder_kind {
        GeocoderKind::Photon => "photon",
        GeocoderKind::Nominatim => "nominatim",
    };
    format!("{kind}|{}|{lang}|{}|{near}", maps.geocoder, query.trim().to_lowercase())
}

/// The address of the search for `query` at the service of `maps`: Photon
/// `/api?q=…&limit=6&lang=…` (with `lat`/`lon` of `near`), Nominatim
/// `/search?format=jsonv2&q=…&limit=6&accept-language=…` (Nominatim has no "near"). Only `http`
/// and `https`.
fn search_url(maps: &Maps, query: &str, near: Option<Geo>, lang: &str) -> Result<reqwest::Url, Failure> {
    let base = maps.geocoder.trim().trim_end_matches('/');
    let limit = SEARCH_LIMIT.to_string();
    let query = query.trim();
    let url = match maps.geocoder_kind {
        GeocoderKind::Photon => {
            let mut params = vec![("q", query.to_string()), ("limit", limit), ("lang", lang.to_string())];
            if let Some(near) = near {
                params.push(("lat", near.lat.to_string()));
                params.push(("lon", near.lon.to_string()));
            }
            reqwest::Url::parse_with_params(&format!("{base}/api"), &params)
        }
        GeocoderKind::Nominatim => reqwest::Url::parse_with_params(
            &format!("{base}/search"),
            &[("format", "jsonv2"), ("q", query), ("limit", &limit), ("accept-language", lang)],
        ),
    };
    url.ok().filter(|url| matches!(url.scheme(), "http" | "https")).ok_or(Failure::Address)
}

/// The address of `tile` from the template of the settings (`{z}`, `{x}`, `{y}`); only `http`
/// and `https`.
fn tile_url(template: &str, tile: &TileAt) -> Result<reqwest::Url, Failure> {
    let address = template
        .trim()
        .replace("{z}", &tile.z.to_string())
        .replace("{x}", &tile.x.to_string())
        .replace("{y}", &tile.y.to_string());
    reqwest::Url::parse(&address).ok().filter(|url| matches!(url.scheme(), "http" | "https")).ok_or(Failure::Address)
}

/// One Photon answer: GeoJSON features.
#[derive(Debug, Deserialize)]
struct PhotonAnswer {
    #[serde(default)]
    features: Vec<PhotonFeature>,
}

#[derive(Debug, Deserialize)]
struct PhotonFeature {
    geometry: PhotonPoint,
    #[serde(default)]
    properties: PhotonProperties,
}

#[derive(Debug, Deserialize)]
struct PhotonPoint {
    /// Longitude, latitude.
    coordinates: Vec<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PhotonProperties {
    name: Option<String>,
    street: Option<String>,
    housenumber: Option<String>,
    postcode: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
}

/// The places of a Photon answer, in its order. The label is the name, else street and house
/// number, else the city; the detail the rest of the address (street, postcode and city or else
/// the state, country) without what the label says. Features without a label or off the map are
/// left out. `Err` when `body` is no Photon answer.
fn photon_places(body: &[u8]) -> Result<Vec<Place>, Failure> {
    let answer: PhotonAnswer = serde_json::from_slice(body).map_err(|_| Failure::Broken)?;
    let place = |feature: PhotonFeature| {
        let [lon, lat] = feature.geometry.coordinates[..] else { return None };
        let geo = Geo::new(lat, lon).filter(Geo::on_map)?;
        let p = feature.properties;
        let street = match (text(&p.street), text(&p.housenumber)) {
            (Some(street), Some(number)) => Some(format!("{street} {number}")),
            (street, _) => street.map(str::to_string),
        };
        let label = text(&p.name)
            .map(str::to_string)
            .or_else(|| street.clone())
            .or_else(|| text(&p.city).map(str::to_string))?;
        let town = match (text(&p.postcode), text(&p.city).or(text(&p.state))) {
            (Some(code), Some(town)) => Some(format!("{code} {town}")),
            (None, Some(town)) => Some(town.to_string()),
            (code, None) => code.map(str::to_string),
        };
        let parts = [street, town, text(&p.country).map(str::to_string)];
        let detail = parts.into_iter().flatten().filter(|part| *part != label).collect::<Vec<_>>().join(", ");
        Some(Place { label, detail, geo, kind: PlaceKind::Search })
    };
    Ok(answer.features.into_iter().filter_map(place).take(SEARCH_LIMIT).collect())
}

/// `value` trimmed, or `None` when it is missing or blank.
fn text(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

/// One place of a Nominatim answer (`format=jsonv2`).
#[derive(Debug, Deserialize)]
struct NominatimPlace {
    lat: String,
    lon: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: String,
}

/// The places of a Nominatim answer, in its order. The label is the name, else the first part of
/// the full name (with the street before a bare house number: "Alexanderplatz 1"); the detail is
/// the rest of the full name. Places off the map or without coordinates are left out. `Err` when
/// `body` is no Nominatim answer.
fn nominatim_places(body: &[u8]) -> Result<Vec<Place>, Failure> {
    let answer: Vec<NominatimPlace> = serde_json::from_slice(body).map_err(|_| Failure::Broken)?;
    let place = |found: NominatimPlace| {
        let geo = Geo::new(found.lat.trim().parse().ok()?, found.lon.trim().parse().ok()?).filter(Geo::on_map)?;
        let mut parts: VecDeque<&str> =
            found.display_name.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
        let label = match found.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => {
                if parts.front() == Some(&name) {
                    parts.pop_front();
                }
                name.to_string()
            }
            None => {
                let first = parts.pop_front()?;
                match parts.front() {
                    Some(street) if first.chars().all(|c| c.is_ascii_digit()) => {
                        let label = format!("{street} {first}");
                        parts.pop_front();
                        label
                    }
                    _ => first.to_string(),
                }
            }
        };
        let detail = parts.into_iter().collect::<Vec<_>>().join(", ");
        Some(Place { label, detail, geo, kind: PlaceKind::Search })
    };
    Ok(answer.into_iter().filter_map(place).take(SEARCH_LIMIT).collect())
}

/// `recent` and then `found`, each place once (same label, ignoring case, or the same point to
/// about 10 m), at most [`MAX_PLACES`].
fn merged(recent: Vec<Place>, found: Vec<Place>) -> Vec<Place> {
    let point = |geo: &Geo| ((geo.lat * 10_000.0).round() as i64, (geo.lon * 10_000.0).round() as i64);
    let mut places: Vec<Place> = Vec::new();
    for place in recent.into_iter().chain(found) {
        let repeated = places.iter().any(|kept| {
            kept.label.to_lowercase() == place.label.to_lowercase() || point(&kept.geo) == point(&place.geo)
        });
        if !repeated && places.len() < MAX_PLACES {
            places.push(place);
        }
    }
    places
}

/// The time [`recent_places`] looks at around `now`: [`RECENT_DAYS`] back and ahead, inside what
/// the calendar service keeps after a sync, so reading it asks no server.
pub fn recent_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (now - TimeDelta::days(RECENT_DAYS), now + TimeDelta::days(RECENT_DAYS))
}

/// Earlier places of the person for `query`: the locations of `events` with coordinates whose
/// text holds every word of `query` (ignoring case), each location once, those of events closest
/// to `now` first, at most [`MAX_RECENT`], as [`PlaceKind::Recent`] with the location as label.
///
/// `events` must be only events the asker may read: the caller filters by calendar before it reads
/// them (`Reach`), as the core does with `calendar.list_events`.
pub fn recent_places(events: &[Event], query: &str, now: DateTime<Utc>) -> Vec<Place> {
    let words: Vec<String> =
        query.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(day) => day.and_time(chrono::NaiveTime::MIN).and_utc(),
    };
    let mut matching: Vec<(&Event, String, Geo)> = events
        .iter()
        .filter_map(|event| {
            let label = event.location.as_deref()?.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>();
            let label = label.join(", ");
            let text = label.to_lowercase();
            (!words.is_empty() && words.iter().all(|word| text.contains(word.as_str())))
                .then_some((event, label, event.geo?))
        })
        .collect();
    matching.sort_by_key(|(event, _, _)| (instant(event.start) - now).abs());
    let mut places: Vec<Place> = Vec::new();
    for (_, label, geo) in matching {
        if places.len() < MAX_RECENT && !places.iter().any(|place| place.label.to_lowercase() == label.to_lowercase()) {
            places.push(Place { label, detail: String::new(), geo, kind: PlaceKind::Recent });
        }
    }
    places
}

/// One tile of a map and where it goes on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileAt {
    z: u8,
    /// Column, wrapped around the world.
    x: u32,
    y: u32,
    /// Its top left corner on the picture, in pixels (negative: cut off at the left or top).
    left: i64,
    top: i64,
}

/// The picture of a map: its size in pixels and where its top left corner lies in the world at
/// [`TILE_ZOOM`], in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct View {
    width: u32,
    height: u32,
    left: i64,
    top: i64,
}

/// Where `geo` lies in the world at `zoom`, in pixels of [`TILE_SIZE`] tiles (Web Mercator, as
/// the tiles of OpenStreetMap): `(0, 0)` is the top left corner at 180° west and 85.0511° north.
fn world_pixel(geo: Geo, zoom: u8) -> (f64, f64) {
    let size = f64::from(TILE_SIZE) * f64::from(1u32 << zoom);
    let lat = geo.lat.to_radians();
    let x = (geo.lon + 180.0) / 360.0 * size;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * size;
    (x, y)
}

impl View {
    /// A picture of `width` × `height` pixels at [`TILE_ZOOM`] with `geo` in the middle.
    fn new(geo: Geo, width: u32, height: u32) -> View {
        let (x, y) = world_pixel(geo, TILE_ZOOM);
        let left = (x - f64::from(width) / 2.0).round() as i64;
        let top = (y - f64::from(height) / 2.0).round() as i64;
        View { width, height, left, top }
    }

    /// The tiles the picture needs, row by row: columns wrap around the world, rows beyond its top
    /// or bottom are left out (the picture shows [`BACKGROUND`] there).
    fn tiles(&self) -> Vec<TileAt> {
        let size = i64::from(TILE_SIZE);
        let count = 1i64 << TILE_ZOOM;
        let columns = self.left.div_euclid(size)..=(self.left + i64::from(self.width) - 1).div_euclid(size);
        let rows = self.top.div_euclid(size)..=(self.top + i64::from(self.height) - 1).div_euclid(size);
        let mut tiles = Vec::new();
        for row in rows.filter(|row| (0..count).contains(row)) {
            for column in columns.clone() {
                tiles.push(TileAt {
                    z: TILE_ZOOM,
                    x: column.rem_euclid(count) as u32,
                    y: row as u32,
                    left: column * size - self.left,
                    top: row * size - self.top,
                });
            }
        }
        tiles
    }
}

/// Darkens a map for the dark appearance: the lightness of each pixel turned around, its hue and
/// saturation kept (HSL). Moving every channel by `1 − max − min` does exactly that: light land
/// becomes dark, dark text light, and a red road stays red.
fn darken(picture: &mut RgbaImage) {
    for pixel in picture.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let shift = 255 - i16::from(max) - i16::from(min);
        let moved = |channel: u8| (i16::from(channel) + shift).clamp(0, 255) as u8;
        pixel.0 = [moved(r), moved(g), moved(b), a];
    }
}

/// Map tiles on disk (task TE.5): one file per tile named by the SHA-256 of its address (the
/// address holds where the person looked; the name does not), with its `ETag` beside it. The
/// directory is 0700 and the files 0600 on Unix. Without a directory nothing is kept. Errors of the
/// cache only cost a new request; they are never reported.
struct TileCache {
    dir: Option<PathBuf>,
}

/// What the cache holds for a tile.
#[derive(Debug, PartialEq, Eq)]
enum Kept {
    /// Stored less than [`TILE_FRESH`] ago: use it without asking.
    Fresh(Vec<u8>),
    /// Older: ask again with its `ETag`, use it when the service says it is unchanged or cannot
    /// be asked.
    Stale {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
    Missing,
}

impl TileCache {
    /// The files of the tile at `url`: the picture and its `ETag`.
    fn files(&self, url: &str) -> Option<(PathBuf, PathBuf)> {
        let dir = self.dir.as_ref()?;
        let name: String = Sha256::digest(url.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
        Some((dir.join(format!("{name}.tile")), dir.join(format!("{name}.etag"))))
    }

    /// What is kept for `url` at `now`.
    fn read(&self, url: &str, now: SystemTime) -> Kept {
        let Some((tile, etag)) = self.files(url) else { return Kept::Missing };
        let Ok(bytes) = std::fs::read(&tile) else { return Kept::Missing };
        let stored = std::fs::metadata(&tile).and_then(|meta| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        // A time in the future (a clock set back) counts as fresh.
        if !now.duration_since(stored).is_ok_and(|age| age >= TILE_FRESH) {
            return Kept::Fresh(bytes);
        }
        let etag = std::fs::read_to_string(&etag).ok().map(|etag| etag.trim().to_string()).filter(|e| !e.is_empty());
        Kept::Stale { bytes, etag }
    }

    /// Keeps `bytes` and `etag` for `url` (without an `ETag` the old one goes).
    fn write(&self, url: &str, bytes: &[u8], etag: Option<&str>) {
        let Some((tile, etag_file)) = self.files(url) else { return };
        if let Some(dir) = tile.parent()
            && crate::clients::create_private_dir(dir).is_err()
        {
            return;
        }
        if crate::clients::write_private(&tile, bytes).is_err() {
            return;
        }
        match etag {
            Some(etag) => {
                let _ = crate::clients::write_private(&etag_file, etag.as_bytes());
            }
            None => {
                let _ = std::fs::remove_file(&etag_file);
            }
        }
    }

    /// The service said the tile at `url` is unchanged: fresh again from `now`.
    fn touch(&self, url: &str, now: SystemTime) {
        if let Some((tile, _)) = self.files(url) {
            let _ = std::fs::File::options().write(true).open(&tile).and_then(|file| file.set_modified(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/places").join(name);
        std::fs::read(path).expect("fixture")
    }

    fn maps_at(server: &MockServer, kind: GeocoderKind) -> Maps {
        Maps {
            enabled: true,
            geocoder: server.base_url(),
            geocoder_kind: kind,
            tiles: format!("{}/tiles/{{z}}/{{x}}/{{y}}.png", server.base_url()),
            ..Maps::default()
        }
    }

    fn geo(lat: f64, lon: f64) -> Geo {
        Geo::new(lat, lon).expect("a point")
    }

    /// A tile of one colour, as PNG.
    fn tile_png(colour: [u8; 3]) -> Vec<u8> {
        let tile = image::RgbImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgb(colour));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(tile).write_to(&mut png, ImageFormat::Png).expect("encoded");
        png.into_inner()
    }

    fn picture(response: &Response) -> RgbaImage {
        let Response::Image { png } = response else { panic!("a picture, got {response:?}") };
        let bytes = base64::engine::general_purpose::STANDARD.decode(png).expect("base64");
        image::load_from_memory_with_format(&bytes, ImageFormat::Png).expect("a PNG").to_rgba8()
    }

    fn failed(response: &Response) -> (&str, &str) {
        match response {
            Response::Error { code, message } => (code, message),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn photon_answers_become_places_with_label_and_detail() {
        let places = photon_places(&fixture("photon.json")).expect("a Photon answer");
        let shown: Vec<(&str, &str)> = places.iter().map(|p| (p.label.as_str(), p.detail.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Alexanderplatz", "10178 Berlin, Germany"),
                ("Alexanderplatz 1", "10178 Berlin, Germany"),
                ("Café am Alex", "Rathausstraße 5, 10178 Berlin, Germany"),
                ("Berlin", "Germany"),
            ],
            "off the map and without a name are left out"
        );
        assert_eq!(places[0].geo, geo(52.5219184, 13.4132147), "GeoJSON is longitude first");
        assert!(places.iter().all(|place| place.kind == PlaceKind::Search));
        assert_eq!(photon_places(b"<html>"), Err(Failure::Broken));
    }

    #[test]
    fn nominatim_answers_become_places_with_label_and_detail() {
        let places = nominatim_places(&fixture("nominatim.json")).expect("a Nominatim answer");
        let shown: Vec<(&str, &str)> = places.iter().map(|p| (p.label.as_str(), p.detail.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("Alexanderplatz", "Mitte, Berlin, 10178, Deutschland"),
                ("Alexanderplatz 1", "Mitte, Berlin, 10178, Deutschland"),
            ],
            "coordinates that are no number are left out"
        );
        assert_eq!(places[1].geo, geo(52.5225, 13.4146));
        assert_eq!(nominatim_places(b"{}"), Err(Failure::Broken));
    }

    #[test]
    fn search_addresses_follow_the_service() {
        let maps = Maps { geocoder: "https://photon.example.org/".into(), ..Maps::default() };
        let url = search_url(&maps, " Alexanderplatz 1 ", Some(geo(52.5, 13.4)), "de").expect("an address");
        assert_eq!(url.as_str(), "https://photon.example.org/api?q=Alexanderplatz+1&limit=6&lang=de&lat=52.5&lon=13.4");
        let nominatim = Maps {
            geocoder: "https://nominatim.example.org".into(),
            geocoder_kind: GeocoderKind::Nominatim,
            ..Maps::default()
        };
        let url = search_url(&nominatim, "Alex", Some(geo(52.5, 13.4)), "en").expect("an address");
        assert_eq!(
            url.as_str(),
            "https://nominatim.example.org/search?format=jsonv2&q=Alex&limit=6&accept-language=en"
        );
        let ftp = Maps { geocoder: "ftp://photon.example.org".into(), ..Maps::default() };
        assert_eq!(search_url(&ftp, "Alex", None, "en"), Err(Failure::Address));
    }

    #[test]
    fn a_point_lies_on_the_tile_the_map_service_numbers() {
        // The origin of the world is the top left corner; 0°/0° its middle.
        assert_eq!(world_pixel(geo(0.0, 0.0), 1), (256.0, 256.0));
        let (x, y) = world_pixel(geo(0.0, -180.0), 0);
        assert!(x.abs() < 1e-9 && (y - 128.0).abs() < 1e-9);
        // Alexanderplatz at zoom 16 lies on tile 35209/21492.
        let (x, y) = world_pixel(geo(52.5219, 13.4132), 16);
        assert_eq!(((x / 256.0) as u32, (y / 256.0) as u32), (35209, 21492));
        assert!((x - 9_013_708.4).abs() < 1.0 && (y - 5_502_029.0).abs() < 1.0, "{x} {y}");
    }

    #[test]
    fn a_map_takes_the_tiles_around_its_middle() {
        // 336 × 104 logical pixels at twice the density: 672 × 208 pixels at zoom 17.
        let view = View::new(geo(52.5219, 13.4132), 336 * DENSITY, 104 * DENSITY);
        let (x, y) = world_pixel(geo(52.5219, 13.4132), 17);
        assert_eq!((view.left, view.top), ((x - 336.0).round() as i64, (y - 104.0).round() as i64));
        let tiles = view.tiles();
        let first = tiles[0];
        assert_eq!((first.z, i64::from(first.x)), (17, view.left.div_euclid(256)));
        assert!(first.left <= 0 && first.left > -256 && first.top <= 0 && first.top > -256, "{first:?}");
        // Every pixel is covered: the tiles span the picture, no more than one tile beyond.
        let columns = tiles.iter().map(|t| t.x).collect::<std::collections::BTreeSet<_>>().len() as i64;
        let rows = tiles.iter().map(|t| t.y).collect::<std::collections::BTreeSet<_>>().len() as i64;
        assert!(columns * 256 >= 672 && (columns - 1) * 256 < 672 + 256, "{columns}");
        assert!(rows * 256 >= 208 && (rows - 1) * 256 < 208 + 256, "{rows}");
        assert_eq!(tiles.len() as i64, columns * rows);
    }

    #[test]
    fn a_map_wraps_at_the_date_line_and_leaves_out_what_is_beyond_the_poles() {
        let east = View::new(geo(0.0, 180.0), 512, 256).tiles();
        assert!(east.iter().any(|t| t.x == 0) && east.iter().any(|t| t.x == (1 << 17) - 1), "{east:?}");
        let north = View::new(geo(Geo::MAP_MAX_LAT, 0.0), 512, 512).tiles();
        // The top of the world lies inside the picture: no row above it, background there.
        assert!(north.iter().all(|t| t.y <= 1) && north[0].y == 0 && north[0].top > 0, "{north:?}");
    }

    #[test]
    fn darkening_turns_lightness_around_and_keeps_the_hue() {
        let mut picture = RgbaImage::from_fn(5, 1, |x, _| match x {
            0 => Rgba([255, 255, 255, 255]),
            1 => Rgba([0, 0, 0, 255]),
            2 => Rgba([255, 0, 0, 255]),
            3 => Rgba([242, 239, 233, 255]),
            _ => Rgba([100, 150, 200, 128]),
        });
        darken(&mut picture);
        let pixels: Vec<[u8; 4]> = picture.pixels().map(|p| p.0).collect();
        assert_eq!(pixels[0], [0, 0, 0, 255], "white land becomes black");
        assert_eq!(pixels[1], [255, 255, 255, 255], "black text becomes white");
        assert_eq!(pixels[2], [255, 0, 0, 255], "red keeps its lightness of one half");
        assert_eq!(pixels[3], [22, 19, 13, 255], "light beige becomes dark brown");
        // A blue keeps its hue: the distances between the channels stay.
        assert_eq!(pixels[4], [55, 105, 155, 128], "alpha kept");
    }

    #[test]
    fn the_person_goes_first_and_clients_share_a_budget() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut throttle = Throttle::new(SEARCH_GAP);
        assert_eq!(throttle.turn(at(0), Asker::Client), Turn::Go);
        assert_eq!(throttle.turn(at(100), Asker::Client), Turn::Busy, "one a second");
        // The person waits for the rest of the second instead of being refused.
        assert_eq!(throttle.turn(at(300), Asker::Person), Turn::After(Duration::from_millis(700)));
        assert_eq!(throttle.turn(at(400), Asker::Person), Turn::Busy, "only one waits");
        assert_eq!(
            throttle.turn(at(1500), Asker::Client),
            Turn::Busy,
            "the person's turn is at 1000, the next at 2000"
        );
        assert_eq!(throttle.turn(at(2000), Asker::Person), Turn::Go);

        // Clients together: ten a minute, then busy until the oldest is a minute old.
        let mut throttle = Throttle::new(SEARCH_GAP);
        for second in 0..CLIENT_BUDGET as u64 {
            assert_eq!(throttle.turn(at(second * 1000), Asker::Client), Turn::Go, "{second}");
        }
        assert_eq!(throttle.turn(at(30_000), Asker::Client), Turn::Busy, "budget used up");
        assert_eq!(throttle.turn(at(30_000), Asker::Person), Turn::Go, "the person is not counted");
        assert_eq!(throttle.turn(at(60_000), Asker::Client), Turn::Go, "the first is a minute old");

        // Maps have no gap: the person never waits, clients keep their budget.
        let mut maps = Throttle::new(Duration::ZERO);
        assert_eq!(maps.turn(at(0), Asker::Person), Turn::Go);
        assert_eq!(maps.turn(at(0), Asker::Person), Turn::Go);
    }

    #[test]
    fn a_refusal_pauses_the_service_for_an_hour() {
        let start = Instant::now();
        let mut throttle = Throttle::new(SEARCH_GAP);
        throttle.pause(start);
        assert_eq!(throttle.turn(start + Duration::from_secs(10), Asker::Person), Turn::Paused);
        assert_eq!(throttle.turn(start + PAUSE - Duration::from_secs(1), Asker::Client), Turn::Paused);
        assert_eq!(throttle.turn(start + PAUSE, Asker::Person), Turn::Go);
    }

    fn event_at(location: Option<&str>, geo: Option<Geo>, start: DateTime<Utc>) -> Event {
        Event {
            uid: "u".into(),
            collection: "work".into(),
            href: None,
            etag: None,
            summary: "Lunch".into(),
            start: EventTime::DateTime(start),
            end: None,
            location: location.map(str::to_string),
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
            alarms: Vec::new(),
            recurrence_id: None,
            repeat: None,
            time_zone: None,
            url: None,
            status: None,
            geo,
            organizer: None,
            attendees: Vec::new(),
            exceptions: Vec::new(),
        }
    }

    #[test]
    fn earlier_places_match_every_word_closest_first_once() {
        let now = Utc::now();
        let alex = geo(52.5219, 13.4132);
        let events = [
            event_at(Some("Alexanderplatz 1\nBerlin"), Some(alex), now - TimeDelta::days(20)),
            event_at(Some("alexanderplatz 1, berlin"), Some(alex), now - TimeDelta::days(1)),
            event_at(Some("Alexanderplatz 5, Berlin"), Some(geo(52.52, 13.41)), now + TimeDelta::days(3)),
            event_at(Some("Alexanderplatz, Hamburg"), Some(geo(53.5, 10.0)), now),
            event_at(Some("Alexanderplatz 9, Berlin"), None, now),
            event_at(None, Some(alex), now),
        ];
        let places = recent_places(&events, "alexanderplatz, BERLIN", now);
        let labels: Vec<&str> = places.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["alexanderplatz 1, berlin", "Alexanderplatz 5, Berlin"], "once, closest first, with geo");
        assert!(places.iter().all(|p| p.kind == PlaceKind::Recent && p.detail.is_empty()));
        assert!(recent_places(&events, " ", now).is_empty(), "no words, no places");
    }

    #[test]
    fn answers_put_earlier_places_first_without_repeats() {
        let place = |label: &str, lat: f64, kind| Place {
            label: label.into(),
            detail: String::new(),
            geo: geo(lat, 13.4),
            kind,
        };
        let recent = vec![place("Alexanderplatz 1, Berlin", 52.5225, PlaceKind::Recent)];
        let found = vec![
            place("Alexanderplatz 1", 52.52251, PlaceKind::Search),
            place("ALEXANDERPLATZ 1, BERLIN", 52.0, PlaceKind::Search),
            place("Alexanderplatz", 52.5219, PlaceKind::Search),
        ];
        let labels: Vec<String> = merged(recent, found).into_iter().map(|p| p.label).collect();
        assert_eq!(labels, ["Alexanderplatz 1, Berlin", "Alexanderplatz"]);
        let many = (0..20).map(|n| place(&format!("Place {n}"), f64::from(n), PlaceKind::Search)).collect();
        assert_eq!(merged(Vec::new(), many).len(), MAX_PLACES);
    }

    #[tokio::test]
    async fn a_search_asks_the_service_once_a_day_and_says_who_it_is() {
        let server = MockServer::start_async().await;
        let photon = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api")
                    .query_param("q", "Alexanderplatz")
                    .query_param("limit", "6")
                    .header("user-agent", USER_AGENT);
                then.status(200).body(fixture("photon.json"));
            })
            .await;
        let places = Places::new(None);
        let maps = maps_at(&server, GeocoderKind::Photon);
        let Response::Places { places: found } =
            places.search(&maps, "Alexanderplatz", None, Asker::Person, Vec::new()).await
        else {
            panic!("places")
        };
        assert_eq!(found.len(), 4);
        // The same search again (other case and spaces) comes from memory, also for a client.
        let again = places.search(&maps, " alexanderplatz ", None, Asker::Client, Vec::new()).await;
        assert_eq!(again, Response::Places { places: found });
        assert_eq!(photon.calls_async().await, 1);
    }

    #[tokio::test]
    async fn a_refusing_service_is_left_alone_and_nothing_names_the_query() {
        let server = MockServer::start_async().await;
        let refusing = server
            .mock_async(|when, then| {
                when.method(GET).path("/search");
                then.status(429);
            })
            .await;
        let places = Places::new(None);
        let maps = maps_at(&server, GeocoderKind::Nominatim);
        let answer = places.search(&maps, "Alexanderplatz", Some(geo(52.5, 13.4)), Asker::Person, Vec::new()).await;
        let (code, message) = failed(&answer);
        assert_eq!(code, "busy");
        assert!(!message.contains("Alexanderplatz") && !message.contains("52.5"), "{message}");
        let later = places.search(&maps, "Rathaus", None, Asker::Person, Vec::new()).await;
        assert_eq!(failed(&later).0, "busy", "paused for an hour");
        assert_eq!(refusing.calls_async().await, 1, "not asked again");

        // Earlier places are the answer when the service gives none.
        let recent = vec![Place {
            label: "Rathaus".into(),
            detail: String::new(),
            geo: geo(52.5, 13.4),
            kind: PlaceKind::Recent,
        }];
        let kept = places.search(&maps, "Rathaus", None, Asker::Person, recent.clone()).await;
        assert_eq!(kept, Response::Places { places: recent });
    }

    #[tokio::test]
    async fn an_unreachable_service_is_offline_without_the_query_or_point() {
        // A port nobody listens on.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("address");
        drop(listener);
        let maps = Maps {
            enabled: true,
            geocoder: format!("http://{address}"),
            tiles: format!("http://{address}/{{z}}/{{x}}/{{y}}.png"),
            ..Maps::default()
        };
        let places = Places::new(None);
        let answer =
            places.search(&maps, "Alexanderplatz", Some(geo(52.5219, 13.4132)), Asker::Person, Vec::new()).await;
        let (code, message) = failed(&answer);
        assert_eq!(code, "offline");
        assert!(!message.contains("Alexanderplatz") && !message.contains("52.52"), "{message}");
        let map = places.map(&maps, geo(52.5219, 13.4132), 336, 104, false, Asker::Person).await;
        let (code, message) = failed(&map);
        assert_eq!(code, "offline");
        assert!(!message.contains("52.52") && !message.contains("17/"), "{message}");
    }

    #[tokio::test]
    async fn a_map_is_put_together_from_kept_tiles_and_darkened() {
        let server = MockServer::start_async().await;
        let tiles = server
            .mock_async(|when, then| {
                when.method(GET).path_includes("/tiles/17/").header("user-agent", USER_AGENT);
                then.status(200).header("ETag", "\"v1\"").body(tile_png([242, 239, 233]));
            })
            .await;
        let dir = tempfile::tempdir().expect("temp dir");
        let places = Places::new(Some(dir.path().join("tiles")));
        let maps = maps_at(&server, GeocoderKind::Photon);
        let point = geo(52.5219, 13.4132);

        let light = picture(&places.map(&maps, point, 336, 104, false, Asker::Person).await);
        assert_eq!(light.dimensions(), (672, 208), "twice the density");
        assert_eq!(light.get_pixel(336, 104).0, [242, 239, 233, 255]);
        let asked = tiles.calls_async().await;
        assert!(asked >= 3, "{asked} tiles");

        let dark = picture(&places.map(&maps, point, 336, 104, true, Asker::Person).await);
        assert_eq!(dark.get_pixel(336, 104).0, [22, 19, 13, 255]);
        assert_eq!(tiles.calls_async().await, asked, "the second map comes from the cache");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = places.tiles.dir.as_deref().expect("a tile cache");
            assert_eq!(std::fs::metadata(dir).expect("dir").permissions().mode() & 0o777, 0o700);
            let files: Vec<_> = std::fs::read_dir(dir).expect("files").map(|f| f.expect("file").path()).collect();
            assert_eq!(files.len(), asked * 2, "a tile and its ETag each");
            for file in files {
                assert_eq!(std::fs::metadata(&file).expect("file").permissions().mode() & 0o777, 0o600, "{file:?}");
                // Named by a checksum of the address, which holds the place.
                let stem = file.file_stem().expect("name").to_string_lossy().into_owned();
                assert!(stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
            }
        }
    }

    #[tokio::test]
    async fn an_old_tile_is_asked_again_with_its_etag() {
        let server = MockServer::start_async().await;
        let unchanged = server
            .mock_async(|when, then| {
                when.method(GET).path("/tiles/17/1/2.png").header("if-none-match", "\"v1\"");
                then.status(304);
            })
            .await;
        let dir = tempfile::tempdir().expect("temp dir");
        let cache = TileCache { dir: Some(dir.path().to_path_buf()) };
        let url = format!("{}/tiles/17/1/2.png", server.base_url());
        cache.write(&url, b"tile", Some("\"v1\""));
        let now = SystemTime::now();
        assert_eq!(cache.read(&url, now), Kept::Fresh(b"tile".to_vec()));
        let later = now + TILE_FRESH + Duration::from_secs(60);
        assert_eq!(cache.read(&url, later), Kept::Stale { bytes: b"tile".to_vec(), etag: Some("\"v1\"".into()) });

        let places = Places { tiles: cache, ..Places::new(None) };
        // Made old on disk, so the request below asks with the ETag.
        let (file, _) = places.tiles.files(&url).expect("files");
        let old = now - TILE_FRESH - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&file).and_then(|f| f.set_modified(old)).expect("made old");
        let bytes = places.tile(reqwest::Url::parse(&url).expect("url")).await.expect("the kept tile");
        assert_eq!(bytes, b"tile");
        assert_eq!(unchanged.calls_async().await, 1);
        assert!(matches!(places.tiles.read(&url, SystemTime::now()), Kept::Fresh(_)), "fresh again");
    }
}
