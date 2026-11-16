//! The map of all contacts (issue #357, draft C in `docs/prototypes/kontakte/`): the second look of
//! the same selection as the list ("List | Map" in the header), offered only while the person turned
//! on maps and the lookup of contact addresses ([`App::map_offered`]).
//!
//! - **Pins** come from the core (`contacts.list_locations`), drawn with the map of single tiles of
//!   [`torro_gui::tile_map`]; nearby pins become a cluster, a click on it zooms in until it falls apart,
//!   at the deepest zoom "In view" lists its contacts.
//! - **Tiles** are asked one by one (`calendar.map_tile`), only those the view lacks, and kept while the
//!   map is open (at most [`MAX_TILES`], the oldest go first); the light and the dark ones apart.
//! - **"In view"** lists the contacts in the visible part, beside the map (wide) or as a drawer at its
//!   foot (medium, narrow); a click selects the pin.
//! - **The pin card** shows the selected contact with Mail and Call (when it has them), "Open in Maps"
//!   and "Open", which opens the contact in the side panel as the list does.
//! - **Looking up** (decision 2 of the plan): while the map is open the window asks the core for the
//!   addresses nobody looked up yet, one after the other (`contacts.locate_contact`); the core keeps
//!   to one a second. `busy` waits a second, `offline` and `error` pause it, leaving the map, closing
//!   the window or turning the lookup off stop it. The status card says how far it is.
//!
//! Nothing here reads or writes data itself: every answer comes through [`Backend`].

use std::collections::{HashMap, HashSet, VecDeque};

use iced::widget::image::Handle;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, stack};
use iced::{Element, Fill, Length, Size, Task};
use torro_contacts::{Contact, ContactsError, ContactsSource};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::tile_map::{self, Camera, MapEvent, MapPin, TileKey};
use torro_gui::{style, typo, widgets};
use torro_ipc::contacts::{AddressRef, ContactLocation, ContactPin, LocationsQuery, LocationsStatus};

use crate::app::{App, Message, View};
use crate::backend::Shown;
use crate::contact;
use crate::list::{face, hue, initials};

/// How many tiles the window keeps while the map is open; the oldest go first.
pub const MAX_TILES: usize = 256;

/// How long the lookup waits after `busy` before it asks again.
pub const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The width of "In view" beside the map (wide).
pub fn in_view_width() -> f32 {
    size::panel() - 7.0 * space::s()
}

/// The two looks of a selection: the list or the map. Not kept between starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    #[default]
    List,
    Map,
}

/// What the person does on the map, and the answers for it.
#[derive(Debug, Clone)]
pub enum MapMsg {
    /// "List | Map".
    Look(Look),
    /// The answer of `contacts.list_locations` number `asked`.
    Locations(u64, Result<(Vec<ContactPin>, LocationsStatus), String>),
    /// Dragged, scrolled, clicked on the map.
    Event(MapEvent),
    /// "+" (1) and "−" (−1).
    Zoom(i32),
    /// A tile came, for the dark or the light map; `None` when it cannot be had.
    Tile(TileKey, bool, Option<Handle>),
    /// The lookup number `generation` of an address answered.
    LookedUp(u64, AddressRef, Result<ContactLocation, String>),
    /// The wait after `busy` is over, for the lookup number `generation`.
    Retry(u64),
    /// A row of "In view": its pin.
    Select(String),
    /// The contact of the selected pin, for Mail and Call.
    Card(String, Result<Box<Contact>, ContactsError>),
    /// ✕ of the pin card.
    CloseCard,
    /// "Open in Maps" of the pin card: contact and address.
    OpenInMaps(String, usize),
}

/// The lookup of addresses while the map is open.
#[derive(Debug, Clone, Default)]
pub struct Lookup {
    /// What is still to be asked, in the order of the list.
    pub queue: VecDeque<AddressRef>,
    /// The address being asked.
    pub running: Option<AddressRef>,
    /// The place search could not be reached: nothing more is asked until the map opens again.
    pub paused: bool,
    /// Answers of an older generation (the map was left meanwhile) are dropped.
    pub generation: u64,
}

/// The state of the map (one per window).
#[derive(Debug, Clone)]
pub struct MapState {
    pub look: Look,
    /// What the settings said at the last load.
    pub shown: Shown,
    pub camera: Camera,
    /// The size the map widget told last.
    pub size: Option<Size>,
    /// The person moved or zoomed: a new answer no longer fits the view to the pins.
    pub moved: bool,
    pub pins: Vec<ContactPin>,
    pub status: LocationsStatus,
    /// The number of the last `contacts.list_locations` asked for.
    pub asked: u64,
    /// The selection the pins are of ([`App::selection_key`]); `None`: ask again.
    pub of: Option<String>,
    /// The first answer came.
    pub loaded: bool,
    pub tiles: HashMap<TileKey, Handle>,
    order: VecDeque<TileKey>,
    /// Tiles asked for and not back yet, and those that could not be had (until the next answer).
    wanted: HashSet<TileKey>,
    failed: HashSet<TileKey>,
    /// Whether `tiles` are the dark ones.
    dark: bool,
    /// The selected pin ([`pin_key`]).
    pub selected: Option<String>,
    /// The contact of the selected pin once fetched.
    pub card: Option<Box<Contact>>,
    /// A cluster at the deepest zoom: "In view" lists only its pins.
    pub only: Option<Vec<String>>,
    pub lookup: Lookup,
    /// `--open map-lookup`: the picture of a lookup under way, nothing is asked.
    pub frozen: bool,
}

impl Default for MapState {
    fn default() -> MapState {
        MapState {
            look: Look::List,
            shown: Shown::default(),
            camera: Camera::world(),
            size: None,
            moved: false,
            pins: Vec::new(),
            status: LocationsStatus::default(),
            asked: 0,
            of: None,
            loaded: false,
            tiles: HashMap::new(),
            order: VecDeque::new(),
            wanted: HashSet::new(),
            failed: HashSet::new(),
            dark: false,
            selected: None,
            card: None,
            only: None,
            lookup: Lookup::default(),
            frozen: false,
        }
    }
}

/// The key of a pin: the contact and which address.
pub fn pin_key(id: &str, address: usize) -> String {
    format!("{id}#{address}")
}

impl MapState {
    /// Stops the lookup: what is asked now is dropped when it answers.
    fn stop_lookup(&mut self) {
        self.lookup.generation += 1;
        self.lookup.running = None;
        self.lookup.queue.clear();
        self.lookup.paused = false;
    }

    fn pin(&self, key: &str) -> Option<&ContactPin> {
        self.pins.iter().find(|pin| pin_key(&pin.id, pin.address) == key)
    }

    /// Keeps `handle` for `key`; past [`MAX_TILES`] the oldest goes.
    fn keep_tile(&mut self, key: TileKey, handle: Handle) {
        if self.tiles.insert(key, handle).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MAX_TILES {
            if let Some(old) = self.order.pop_front() {
                self.tiles.remove(&old);
            }
        }
    }

    /// The tiles of the view that are neither here nor asked for nor failed.
    pub fn missing_tiles(&self, size: Size) -> Vec<TileKey> {
        tile_map::visible_tiles(self.camera, size)
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| !self.tiles.contains_key(key) && !self.wanted.contains(key) && !self.failed.contains(key))
            .collect()
    }

    /// Whether the status card shows: while looking up, paused, with addresses left or contacts
    /// without an address.
    pub fn status_shown(&self) -> bool {
        self.lookup.running.is_some()
            || self.lookup.paused
            || !self.status.pending.is_empty()
            || self.status.without_address > 0
    }
}

impl App {
    /// Whether "List | Map" is offered: maps and the lookup of contact addresses on, in a view of
    /// contacts (all, favourites, a book, a group), not in the glance.
    pub fn map_offered(&self) -> bool {
        self.map.shown.maps
            && self.map.shown.contact_lookup
            && matches!(self.view, View::All | View::Favorites | View::Book(_) | View::Group(..))
    }

    /// Whether the map is what the window shows.
    pub fn on_map(&self) -> bool {
        self.map.look == Look::Map && self.map_offered()
    }

    /// The selection of the list as `contacts.list_locations` takes it: the view and the search the
    /// core answered.
    pub fn locations_query(&self) -> LocationsQuery {
        let query = self.found.as_ref().map(|(text, _)| text.clone());
        match &self.view {
            View::Favorites => LocationsQuery { favorites: true, query, ..LocationsQuery::default() },
            View::Book(book) => LocationsQuery { book: Some(book.clone()), query, ..LocationsQuery::default() },
            View::Group(book, group) => LocationsQuery {
                book: Some(book.clone()),
                group: Some(group.clone()),
                query,
                ..LocationsQuery::default()
            },
            _ => LocationsQuery { query, ..LocationsQuery::default() },
        }
    }

    /// The selection the pins belong to, as a word: a new one asks again.
    fn selection_key(&self) -> String {
        format!("{:?}", self.locations_query())
    }

    /// The size of the map: as the widget told it, else what the window leaves it.
    pub fn map_size(&self) -> Size {
        self.map.size.unwrap_or_else(|| {
            let height = self.content_size().height - size::header() - size::control() - 2.0 * space::s();
            Size::new(self.list_width().max(1.0), height.max(1.0))
        })
    }

    /// After every message: back to the list when the map is no longer offered (the lookup stops),
    /// the pins of a new selection, the tiles the view lacks.
    pub(crate) fn follow_map(&mut self) -> Task<Message> {
        if self.map.look == Look::Map && !self.map_offered() {
            self.map.look = Look::List;
            self.map.stop_lookup();
            self.map.selected = None;
            self.map.card = None;
        }
        if self.map.look != Look::Map {
            return Task::none();
        }
        let key = self.selection_key();
        let pins = if self.map.of.as_deref() != Some(key.as_str()) {
            self.map.of = Some(key);
            self.ask_locations()
        } else {
            Task::none()
        };
        Task::batch([pins, self.want_tiles()])
    }

    /// Asks the core for the pins of the selection.
    fn ask_locations(&mut self) -> Task<Message> {
        self.map.asked += 1;
        let asked = self.map.asked;
        let backend = self.backend.clone();
        let query = self.locations_query();
        Task::perform(async move { backend.locations(query).await }, move |result| {
            Message::Map(MapMsg::Locations(asked, result))
        })
    }

    /// Asks for the tiles the view lacks, in the darkness the window is drawn in; another darkness
    /// drops the tiles kept.
    fn want_tiles(&mut self) -> Task<Message> {
        let theme = self.theme();
        let dark = theme.extended_palette().is_dark;
        if dark != self.map.dark {
            self.map.dark = dark;
            self.map.tiles.clear();
            self.map.order.clear();
            self.map.wanted.clear();
            self.map.failed.clear();
        }
        let missing = self.map.missing_tiles(self.map_size());
        let mut tasks = Vec::new();
        for key in missing {
            self.map.wanted.insert(key);
            if self.backend.is_demo() {
                tasks.push(Task::done(Message::Map(MapMsg::Tile(key, dark, Some(tile_map::demo_tile(&theme))))));
            } else {
                let backend = self.backend.clone();
                tasks.push(Task::perform(async move { backend.tile(key, dark).await }, move |bytes| {
                    Message::Map(MapMsg::Tile(key, dark, bytes.as_deref().and_then(crate::app::picture)))
                }));
            }
        }
        Task::batch(tasks)
    }

    /// Fits the view to the pins, unless the person moved it.
    fn fit_pins(&mut self) {
        if self.map.moved {
            return;
        }
        let points: Vec<(f64, f64)> = self.map.pins.iter().map(|pin| (pin.geo.lat, pin.geo.lon)).collect();
        self.map.camera = tile_map::fit(&points, self.map_size(), tile_map::FIT_ZOOM);
    }

    /// The next address to look up, while the map is open and nothing is asked.
    fn next_lookup(&mut self) -> Task<Message> {
        let lookup = &mut self.map.lookup;
        if self.map.look != Look::Map || self.map.frozen || lookup.paused || lookup.running.is_some() {
            return Task::none();
        }
        let Some(next) = lookup.queue.pop_front() else { return Task::none() };
        lookup.running = Some(next.clone());
        let generation = lookup.generation;
        let backend = self.backend.clone();
        let (id, address) = (next.id.clone(), next.address);
        Task::perform(async move { backend.locate(id, address).await }, move |result| {
            Message::Map(MapMsg::LookedUp(generation, next.clone(), result))
        })
    }

    /// Handles a message of the map.
    pub(crate) fn map_update(&mut self, message: MapMsg) -> Task<Message> {
        match message {
            MapMsg::Look(look) => {
                self.map.look = look;
                self.menu = None;
                if look == Look::List {
                    self.map.stop_lookup();
                    self.map.selected = None;
                    self.map.card = None;
                } else {
                    // Asked again on the way back: the lookup may have found more meanwhile.
                    self.map.of = None;
                }
            }
            MapMsg::Locations(asked, _) if asked != self.map.asked => {}
            MapMsg::Locations(_, Ok((pins, status))) => {
                self.map.loaded = true;
                self.map.failed.clear();
                self.map.pins = pins;
                if self.map.selected.as_deref().is_some_and(|key| self.map.pin(key).is_none()) {
                    self.map.selected = None;
                    self.map.card = None;
                }
                let running = self.map.lookup.running.clone();
                self.map.lookup.queue =
                    status.pending.iter().filter(|pending| Some(*pending) != running.as_ref()).cloned().collect();
                self.map.status = status;
                self.fit_pins();
                return self.next_lookup();
            }
            MapMsg::Locations(_, Err(_)) => {
                self.map.loaded = true;
                self.notify(tr("The contacts could not be placed on the map."));
            }
            MapMsg::Event(MapEvent::Viewport(camera, size)) => {
                let resized = self.map.size != Some(size);
                self.map.size = Some(size);
                if camera != self.map.camera {
                    self.map.camera = camera;
                    self.map.moved = true;
                    self.map.only = None;
                } else if resized {
                    self.fit_pins();
                }
            }
            MapMsg::Event(MapEvent::Pin(key)) | MapMsg::Select(key) => return self.select_pin(key),
            MapMsg::Event(MapEvent::Cluster(keys)) => {
                let points: Vec<(f64, f64)> =
                    keys.iter().filter_map(|key| self.map.pin(key)).map(|pin| (pin.geo.lat, pin.geo.lon)).collect();
                let current = self.map.camera;
                if current.zoom >= tile_map::MAX_ZOOM {
                    self.map.only = Some(keys);
                } else {
                    let mut camera = tile_map::fit(&points, self.map_size(), tile_map::MAX_ZOOM);
                    camera.zoom = camera.zoom.max(current.zoom + 1).min(tile_map::MAX_ZOOM);
                    self.map.camera = camera;
                    self.map.moved = true;
                    self.map.only = (camera.zoom >= tile_map::MAX_ZOOM).then_some(keys);
                }
            }
            MapMsg::Zoom(steps) => {
                let size = self.map_size();
                let middle = iced::Point::new(size.width / 2.0, size.height / 2.0);
                self.map.camera = self.map.camera.zoomed(steps, middle, size);
                self.map.moved = true;
                self.map.only = None;
            }
            MapMsg::Tile(key, dark, picture) => {
                self.map.wanted.remove(&key);
                if dark == self.map.dark {
                    match picture {
                        Some(handle) => self.map.keep_tile(key, handle),
                        None => {
                            self.map.failed.insert(key);
                        }
                    }
                }
            }
            MapMsg::LookedUp(generation, _, _) if generation != self.map.lookup.generation => {}
            MapMsg::LookedUp(_, address, result) => return self.looked_up(address, result),
            MapMsg::Retry(generation) if generation == self.map.lookup.generation => return self.next_lookup(),
            MapMsg::Retry(_) => {}
            MapMsg::Card(id, Ok(contact)) => {
                if self.map.selected.as_deref().is_some_and(|key| key.starts_with(&format!("{id}#"))) {
                    self.map.card = Some(contact);
                }
            }
            // Mail and Call stay away; the rest of the card shows.
            MapMsg::Card(_, Err(_)) => {}
            MapMsg::CloseCard => {
                self.map.selected = None;
                self.map.card = None;
            }
            MapMsg::OpenInMaps(id, address) => {
                let backend = self.backend.clone();
                return Task::perform(async move { backend.open_in_maps(id, address).await }, Message::OpenedInMaps);
            }
        }
        Task::none()
    }

    /// Selects the pin `key` and fetches its contact for the card.
    fn select_pin(&mut self, key: String) -> Task<Message> {
        let Some(pin) = self.map.pin(&key) else { return Task::none() };
        let id = pin.id.clone();
        self.map.selected = Some(key);
        if self.map.card.as_ref().is_some_and(|card| card.id == id) {
            return Task::none();
        }
        self.map.card = None;
        let backend = self.backend.clone();
        Task::perform(async move { backend.get_contact(id.clone()).await.map(|c| (id, c)) }, |result| match result {
            Ok((id, contact)) => Message::Map(MapMsg::Card(id, Ok(Box::new(contact)))),
            Err(err) => Message::Map(MapMsg::Card(String::new(), Err(err))),
        })
    }

    /// The answer for `address`: a pin, one more not found, a wait after `busy`, a pause, or the lookup
    /// turned off (back to the list); then the next address.
    fn looked_up(&mut self, address: AddressRef, result: Result<ContactLocation, String>) -> Task<Message> {
        self.map.lookup.running = None;
        let done = |status: &mut LocationsStatus| status.pending.retain(|pending| *pending != address);
        match result {
            Ok(ContactLocation::Found { geo, source }) => {
                done(&mut self.map.status);
                let name = self.summary(&address.id).map(|c| c.display_name.clone()).unwrap_or_default();
                self.map.pins.push(ContactPin {
                    id: address.id.clone(),
                    name,
                    address: address.address,
                    geo,
                    source,
                    untrusted: true,
                });
                self.map.status.located += 1;
            }
            Ok(ContactLocation::NotFound | ContactLocation::NotLookedUp) => {
                done(&mut self.map.status);
                self.map.status.not_found += 1;
            }
            // Turned off meanwhile: the switch is gone, the window goes back to the list.
            Ok(ContactLocation::NoConsent) => {
                self.map.shown.contact_lookup = false;
                return Task::none();
            }
            Err(code) if code == "busy" => {
                self.map.lookup.queue.push_front(address);
                let generation = self.map.lookup.generation;
                return Task::perform(tokio::time::sleep(BUSY_WAIT), move |()| Message::Map(MapMsg::Retry(generation)));
            }
            // The contact is gone: nothing to place.
            Err(code) if code == "not-found" => done(&mut self.map.status),
            Err(_) => {
                self.map.lookup.queue.push_front(address);
                self.map.lookup.paused = true;
                return Task::none();
            }
        }
        self.next_lookup()
    }

    /// The pins "In view" lists: those in the visible part, or the cluster at the deepest zoom.
    pub fn in_view(&self) -> Vec<&ContactPin> {
        let size = self.map_size();
        let mut pins: Vec<&ContactPin> = self
            .map
            .pins
            .iter()
            .filter(|pin| match &self.map.only {
                Some(keys) => keys.contains(&pin_key(&pin.id, pin.address)),
                None => {
                    let at = self.map.camera.screen(pin.geo.lat, pin.geo.lon, size);
                    at.x >= 0.0 && at.y >= 0.0 && at.x <= size.width && at.y <= size.height
                }
            })
            .collect();
        pins.sort_by_key(|pin| pin.name.to_lowercase());
        pins
    }

    // -- View -------------------------------------------------------------------------------------

    /// The map with its cards: "In view", the status, the pin card, zoom and the attribution.
    pub fn map_view(&self) -> Element<'_, Message> {
        let theme = self.theme();
        let p = palette(&theme);
        let pins: Vec<MapPin> = self
            .map
            .pins
            .iter()
            .map(|pin| MapPin {
                id: pin_key(&pin.id, pin.address),
                lat: pin.geo.lat,
                lon: pin.geo.lon,
                initials: initials(&pin.name),
                color: hue(&p, &pin.name),
            })
            .collect();
        let map = tile_map::tile_map(self.map.camera, &self.map.tiles, pins, self.map.selected.clone(), |event| {
            Message::Map(MapMsg::Event(event))
        });
        let wide = self.class() == SizeClass::Wide;
        let gap = space::m();
        let zoom_in = widgets::icon_button(Icon::Plus, tr("Zoom in"), Some(Message::Map(MapMsg::Zoom(1))));
        let zoom_out = widgets::icon_button(Icon::Minus, tr("Zoom out"), Some(Message::Map(MapMsg::Zoom(-1))));
        let credit = widgets::text_button(
            None,
            tr("© OpenStreetMap contributors"),
            style::button::secondary,
            Some(Message::OpenUri(torro_gui::map::OSM_COPYRIGHT.into())),
        );
        let status = self.map.status_shown().then(|| self.status_card());
        let mut layers: Vec<Element<'_, Message>> = vec![map];
        if wide {
            // "In view" at the left, the status at the top right, zoom and the attribution at the
            // bottom right, the pin card beside "In view" at the foot.
            layers.push(container(self.in_view_card(true)).padding(gap).height(Fill).into());
            let zoom = container(column![zoom_in, zoom_out]).padding(space::xs()).style(style::container::menu);
            let foot = column![zoom, credit].spacing(space::s()).align_x(iced::Alignment::End);
            layers.push(container(foot).padding(gap).align_right(Fill).align_bottom(Fill).into());
            if let Some(status) = status {
                layers.push(container(status).padding(gap).align_right(Fill).into());
            }
            if let Some(card) = self.pin_card() {
                let left = in_view_width() + 2.0 * gap;
                let card = container(card).padding(iced::Padding { left, ..iced::Padding::new(gap) });
                layers.push(card.align_bottom(Fill).into());
            }
        } else {
            // Medium and narrow: zoom and the attribution in a row at the top, the status under it;
            // at the foot the pin card, else "In view" as a drawer.
            let zoom = container(row![zoom_in, zoom_out]).padding(space::xs()).style(style::container::menu);
            let top = row![zoom, iced::widget::Space::new().width(Fill), credit].align_y(iced::Alignment::Center);
            let mut head = column![top].spacing(space::s());
            if let Some(status) = status {
                head = head.push(status);
            }
            layers.push(container(head).padding(gap).width(Fill).into());
            let foot = match self.pin_card() {
                Some(card) => card,
                None => self.in_view_card(false),
            };
            layers.push(container(foot).padding(gap).width(Fill).align_bottom(Fill).into());
        }
        stack(layers).width(Fill).height(Fill).into()
    }

    /// How high "In view" is as a drawer (medium, narrow): a quarter of the map, at least two rows.
    fn drawer_height(&self) -> f32 {
        (self.map_size().height / 4.0).max(size::row_contact() * 2.0)
    }

    /// "In view · n" with a row per contact in the visible part.
    fn in_view_card(&self, wide: bool) -> Element<'_, Message> {
        let pins = self.in_view();
        let head = row![
            typo::text(typo::body_strong(), tr("In view")),
            typo::text(typo::meta(), pins.len().to_string()).style(theme::text_3),
        ]
        .spacing(space::s())
        .align_y(iced::Alignment::Center);
        let colours = palette(&self.theme());
        let rows: Vec<Element<'_, Message>> = pins
            .iter()
            .map(|pin| {
                let key = pin_key(&pin.id, pin.address);
                let selected = self.map.selected.as_deref() == Some(key.as_str());
                let town = self.summary(&pin.id).and_then(|c| c.locality.clone()).unwrap_or_default();
                let picture = self.summary(&pin.id).and_then(|c| {
                    let (version, picture) = self.photos.get(&c.id)?;
                    (c.photo.as_ref().map(|p| &p.version) == Some(version)).then_some(picture.as_ref()).flatten()
                });
                let line = row![
                    face(&pin.name, picture, size::tile(), &colours),
                    typo::text(typo::body(), pin.name.clone()).wrapping(Wrapping::None).width(Fill),
                    typo::text(typo::meta(), town).style(theme::text_3).wrapping(Wrapping::None),
                ]
                .spacing(space::s())
                .align_y(iced::Alignment::Center);
                button(container(line).height(size::row_sidebar()).center_y(Length::Shrink))
                    .padding(iced::Padding::ZERO.left(space::s()).right(space::s()))
                    .width(Fill)
                    .style(style::button::list_row(selected))
                    .on_press(Message::Map(MapMsg::Select(key)))
                    .into()
            })
            .collect();
        let body: Element<'_, Message> = if rows.is_empty() {
            let words = if self.map.loaded { tr("No contact here. Move or zoom out.") } else { tr("Loading…") };
            typo::text(typo::meta(), words).style(theme::text_3).into()
        } else {
            scrollable(column(rows))
                .height(Fill)
                .style(style::scrollable)
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .into()
        };
        let card = container(column![head, body].spacing(space::s()))
            .padding(layout::panel_padding())
            .style(style::container::popover);
        if wide {
            card.width(in_view_width()).height(Fill).into()
        } else {
            card.width(Fill).height(self.drawer_height()).into()
        }
    }

    /// "148 / 212", what the lookup does, how many have no address.
    fn status_card(&self) -> Element<'_, Message> {
        let status = &self.map.status;
        let total = status.located + status.not_found + status.pending.len() as u32;
        let mut lines: Vec<Element<'_, Message>> =
            vec![typo::text(typo::body_strong(), format!("{} / {}", status.located, total)).into()];
        let doing = if self.map.lookup.paused {
            Some(tr("Lookup paused. The place search cannot be reached.").to_owned())
        } else if self.map.lookup.running.is_some() || !status.pending.is_empty() {
            Some(tr("One address a second while the map is open.").to_owned())
        } else {
            None
        };
        if let Some(doing) = doing {
            lines.push(typo::text(typo::meta(), doing).style(theme::text_2).into());
        }
        if status.not_found > 0 {
            let words = trf("Addresses not found: {n}", &[("n", &status.not_found)]);
            lines.push(typo::text(typo::meta(), words).style(theme::text_3).into());
        }
        if status.without_address > 0 {
            let words = trf("Contacts without an address: {n}", &[("n", &status.without_address)]);
            lines.push(typo::text(typo::meta(), words).style(theme::text_3).into());
        }
        container(column(lines).spacing(space::xs()))
            .padding(layout::panel_padding())
            .max_width(size::panel() - 2.0 * space::xxl())
            .style(style::container::popover)
            .into()
    }

    /// The card of the selected pin: name, address, Mail and Call when the contact has them, "Open in
    /// Maps" and "Open".
    fn pin_card(&self) -> Option<Element<'_, Message>> {
        let key = self.map.selected.as_deref()?;
        let pin = self.map.pin(key)?;
        let card = self.map.card.as_ref().filter(|card| card.id == pin.id);
        let address = card.and_then(|c| c.addresses.get(pin.address)).map(|a| a.value.one_line()).unwrap_or_default();
        let head = row![
            column![
                typo::text(typo::body_strong(), pin.name.clone()).wrapping(Wrapping::None),
                typo::text(typo::meta(), address).style(theme::text_3),
            ]
            .width(Fill)
            .spacing(space::xs()),
            widgets::icon_button(Icon::X, tr("Close"), Some(Message::Map(MapMsg::CloseCard))),
        ]
        .spacing(space::s());
        let mut buttons: Vec<Element<'_, Message>> = Vec::new();
        let mail = card.and_then(|c| c.emails.first()).and_then(|m| contact::mail_link(&m.value));
        if let Some(link) = mail {
            buttons.push(widgets::text_button(
                Some(Icon::Mail),
                tr("Mail"),
                style::button::secondary,
                Some(Message::OpenUri(link)),
            ));
        }
        let call = card.and_then(|c| c.phones.first()).and_then(|t| contact::tel_link(&t.value));
        if let Some(link) = call {
            buttons.push(widgets::text_button(
                Some(Icon::Phone),
                tr("Call"),
                style::button::secondary,
                Some(Message::OpenUri(link)),
            ));
        }
        buttons.push(widgets::text_button(
            Some(Icon::ArrowUpRight),
            tr("Open in Maps"),
            style::button::secondary,
            Some(Message::Map(MapMsg::OpenInMaps(pin.id.clone(), pin.address))),
        ));
        buttons.push(widgets::text_button(
            Some(Icon::User),
            tr("Open"),
            style::button::secondary,
            Some(Message::Open(pin.id.clone())),
        ));
        let buttons = iced::widget::Row::with_children(buttons).spacing(space::s()).wrap();
        Some(
            container(column![head, buttons].spacing(size::card_gap()))
                .padding(layout::panel_padding())
                .max_width(size::form_max())
                .style(style::container::popover)
                .into(),
        )
    }

    /// The switch "List | Map" of the header; narrow one button that shows the other look, so "New
    /// contact" keeps its room.
    pub fn look_switch(&self, narrow: bool) -> Element<'_, Message> {
        if narrow {
            return match self.map.look {
                Look::List => {
                    widgets::icon_button(Icon::MapPin, tr("Map"), Some(Message::Map(MapMsg::Look(Look::Map))))
                }
                Look::Map => widgets::icon_button(Icon::List, tr("List"), Some(Message::Map(MapMsg::Look(Look::List)))),
            };
        }
        let active = usize::from(self.map.look == Look::Map);
        widgets::segmented(
            [(tr("List"), Message::Map(MapMsg::Look(Look::List))), (tr("Map"), Message::Map(MapMsg::Look(Look::Map)))],
            active,
        )
    }
}
