//! The settings pages General and Appearance (tasks S.9 and S.14 in `docs/tasks/steuerzentrale.md`):
//! settings groups, each with a footer sentence that explains the consequence.
//!
//! - **General** ([`view`]): "Language", a list of choices – one row each, the whole row is the
//!   button, a check mark at the chosen one (Deutsch is shown in its own name, like the language
//!   picker of any app); "Apps" ([`apps`], task S.21) has every part of the suite with its switch,
//!   its window class under the name (for window rules) and "Open", the App Feed among them;
//!   "Maps" ([`maps`], task TE.10) switches the calendar's place search and small map on and sets
//!   their services; "Startup" has the "Start at login" switch; "Setup" runs the first-run setup
//!   again.
//! - **Appearance** ([`appearance`], its own page in the sidebar since S.14, so it is found at a
//!   glance; compact since L.18): "Color mode" is one row with a segment of three parts, each with
//!   its symbol and word (System, Light, Dark); "Design", "Font" and "Colour scheme" are grids of
//!   small tiles (below); "Interface" has the zoom of the suite with −, the value, + and "Reset"
//!   (task Z.1).
//!
//! What a single app or window shows and how it behaves is set in that window, behind its gear
//! (task S.21): the calendar's night and reminders, how mail is read, the sources of the
//! Notification Hub. Only what holds for the whole suite is left here.
//!
//! Both pages share one state and one message type. There is no Save button: every change sends
//! `Request::SetSetting` with the one key that changed, and the shell reloads `Data` afterwards.
//! The zoom goes through the window's own zoom instead (`Action::Zoom`), which saves it and
//! applies it at once, also in the demo. The page shows `data.settings`, so it follows the daemon.
//!
//! # Design, font and colour scheme (tasks L.4, L.18 and L.23, concept `docs/design-pakete.md`)
//!
//! The three parts of the look are chosen apart, each in a grid of tiles that takes as many as fit
//! the width (`widgets::preview_grid`), one tile per entry as the core lists them
//! (`settings.list_designs`, `settings.list_fonts`, `settings.list_color_schemes`, asked by the
//! shell when the page opens and on "Reload"). Each tile shows only its own part:
//!
//! - **Design** – the design's `preview.png`, or a sample of its form (spacing, radius, text size)
//!   drawn in the active font and colour scheme; the name under it, "version · author" as tooltip.
//! - **Font** – "Aa 09" in the font's text family at the size the font draws body text; named by
//!   the family, "pack · version · author · licence" as tooltip.
//! - **Colour scheme** – stripes of its `bg`, `panel`, `accent` and `text` in the window's light or
//!   dark mode; the name, "version · author" as tooltip.
//!
//! The one each part is drawn with has a frame and a check mark. Choosing a tile is
//! [`Action::Choose`]: the shell draws its own window with it at once and saves it
//! (`settings.set_setting design | font | color_scheme`); no question first. Choosing a design
//! changes neither font nor colour scheme; when the design drawn suggests others than the active
//! ones (`DesignInfo::font`, `DesignInfo::color_scheme`), their tiles say "Suggested" under the name
//! with "Suggested for {design}" in the tooltip, and a click chooses them like any other.
//!
//! An entry that breaks a rule is a greyed-out tile with an alert mark in place of the sample. A
//! click on it shows, under its grid, its name, "Can't be used:" and every reason in full,
//! wrapping – a list of reasons is too long for a tooltip and must be readable without a mouse
//! over it. A second click, or a click on another tile, closes or swaps that
//! ([`Message::Problems`]); one entry's reasons are shown at a time on the whole page.
//!
//! "Reload" (once, at the heading "Design": it lists all three again) and "Open … folder" in each
//! heading (created with its examples if it is missing, its path as tooltip) are quiet actions at
//! the right of the headings. Opening a folder is a local action of the control centre without a
//! request: it only shows the file manager on this desktop (`docs/api.md`, footnote 9).
//!
//! The lists come from the core; the parts for the samples are read here, only for the ids the
//! core listed as usable, the way every window reads the part it draws with
//! (`torro_core::design::load`, `load_font`, `load_color_scheme`): [`Entry`].

use std::path::PathBuf;
use std::sync::Arc;

use iced::border::Radius;
use iced::widget::text::LineHeight;
use iced::widget::{column, container, responsive, row, space as blank, svg, text};
use iced::{Alignment, Background, Border, Color, ContentFit, Element, Fill, Length, Padding, Pixels, Size, Theme};
use torro_core::AppId;
use torro_core::design::{ColorScheme, Design, FontRole, FontSet, Mode, Part, STANDARD_ID, Weight};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, GeocoderKind, Language, Maps, MapsApp, Settings, Zoom};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{Palette, palette, radius, size, space};
use torro_gui::widgets::PreviewState;
use torro_gui::zoom::{self, Step};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{ColorSchemeInfo, DesignInfo, FontInfo, Request, SettingKey};
use torro_settings_ui::parts::{choice, field, notice, picker, switch};

use super::setup::component_name;
use super::{Action, Data, Section, setting_row};

#[derive(Debug, Default)]
pub struct State {
    /// The designs as the core listed them last, the standard first; `Err` when it did not answer,
    /// `None` until the first answer. The fonts and colour schemes alike.
    designs: Option<Result<Vec<DesignEntry>, String>>,
    fonts: Option<Result<Vec<FontEntry>, String>>,
    schemes: Option<Result<Vec<SchemeEntry>, String>>,
    /// The entry that can't be used whose reasons are shown under its grid (its part and id), after
    /// a click on its tile; `None` while no reasons are shown. The part, because one id can name a
    /// design, a font and a colour scheme at once (`plex`).
    problems: Option<(Part, String)>,
    /// The addresses of "Maps" as typed and not saved yet ([`maps`]).
    maps: MapsDraft,
}

/// One of the two addresses under "Maps": the place search service or the map tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    Geocoder,
    Tiles,
}

impl Address {
    /// The field of `settings.set_setting maps` it sets.
    fn field(self) -> &'static str {
        match self {
            Address::Geocoder => "geocoder",
            Address::Tiles => "tiles",
        }
    }

    /// The address as stored.
    fn stored(self, maps: &Maps) -> &str {
        match self {
            Address::Geocoder => &maps.geocoder,
            Address::Tiles => &maps.tiles,
        }
    }

    /// The other one of the two: typing into one leaves the other.
    fn other(self) -> Address {
        match self {
            Address::Geocoder => Address::Tiles,
            Address::Tiles => Address::Geocoder,
        }
    }
}

/// What is typed into the two address fields of "Maps" and not saved yet. An address is saved on
/// Enter, or when the person moves on to another control of the group (Iced 0.14 tells a page
/// nothing about a field losing the focus, so "leaving" is the next thing done in the group);
/// never on every key, because each save is a request and a half-typed address is no address.
#[derive(Debug, Default)]
struct MapsDraft {
    /// The place search address as typed; `None` while the field shows what is stored.
    geocoder: Option<Draft>,
    /// The map tile address as typed; `None` alike.
    tiles: Option<Draft>,
    /// The address whose last save was refused, and why: the service's own check
    /// (`Maps::merged`, which `settings.set_setting maps` runs), shown quietly under the group.
    problem: Option<(Address, String)>,
}

/// One address as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    text: String,
    /// Saved or refused since the last key: leaving the field again does not send it again (while
    /// the core is still answering, or after it refused), only Enter does.
    handed_in: bool,
}

impl MapsDraft {
    fn draft(&self, which: Address) -> Option<&Draft> {
        match which {
            Address::Geocoder => self.geocoder.as_ref(),
            Address::Tiles => self.tiles.as_ref(),
        }
    }

    fn draft_mut(&mut self, which: Address) -> &mut Option<Draft> {
        match which {
            Address::Geocoder => &mut self.geocoder,
            Address::Tiles => &mut self.tiles,
        }
    }

    /// Whether `which` holds typed text that differs from what is stored.
    fn dirty(&self, which: Address, maps: &Maps) -> bool {
        self.draft(which).is_some_and(|typed| typed.text.trim() != which.stored(maps))
    }

    /// Forgets the drafts that are stored now (saved, or the stored text typed again), so the
    /// fields follow the core again when another client changes them.
    fn settle(&mut self, maps: &Maps) {
        for which in [Address::Geocoder, Address::Tiles] {
            if !self.dirty(which, maps) {
                *self.draft_mut(which) = None;
            }
        }
    }
}

/// One entry of a list of the core – a design, a font or a colour scheme (`info`) – with the part
/// itself as read here for its sample.
#[derive(Debug, Clone)]
pub struct Entry<I, T> {
    pub info: I,
    /// The part as read from its folder: what the sample shows and what the demo draws with when it
    /// is chosen. `None` for one that can't be used (or whose folder changed since the core listed
    /// it).
    pub part: Option<Arc<T>>,
    /// A design's `preview.png` as an SVG image: Iced here draws SVG but not PNG (no `image`
    /// feature), and its SVG renderer takes a PNG embedded as a `data:` address. Built once per
    /// listing, so Iced rasterises it once. Always `None` for fonts and colour schemes.
    preview: Option<svg::Handle>,
}

/// A design of the list (`settings.list_designs`).
pub type DesignEntry = Entry<DesignInfo, Design>;
/// A font of the list (`settings.list_fonts`).
pub type FontEntry = Entry<FontInfo, FontSet>;
/// A colour scheme of the list (`settings.list_color_schemes`).
pub type SchemeEntry = Entry<ColorSchemeInfo, ColorScheme>;

impl<I, T> Entry<I, T> {
    /// The row `info` with `part` as read from its folder (`None` when it can't be used). For a
    /// design, use [`Entry::design`], which reads its picture too.
    pub fn new(info: I, part: Option<T>) -> Entry<I, T> {
        Entry { info, part: part.map(Arc::new), preview: None }
    }
}

impl DesignEntry {
    /// [`Entry::new`] for a design: reads its `preview.png` if it has one (checked by
    /// `design::load`: a PNG of at most 2 MB and 1200 × 800); a picture that can't be read gives the
    /// sample.
    pub fn design(info: DesignInfo, design: Option<Design>) -> DesignEntry {
        let png = design.as_ref().and_then(|d| d.preview.as_ref()).and_then(|path| std::fs::read(path).ok());
        Entry { preview: png.as_deref().and_then(png_as_svg), ..Entry::new(info, design) }
    }
}

/// The three lists of one listing ("Reload" lists all three at once), each `Err` with what went wrong
/// when the core did not list it.
#[derive(Debug, Clone)]
pub struct Lists {
    pub designs: Result<Vec<DesignEntry>, String>,
    pub fonts: Result<Vec<FontEntry>, String>,
    pub schemes: Result<Vec<SchemeEntry>, String>,
}

impl Lists {
    /// Three lists that all failed for `problem` (no home directory, a worker that died).
    pub fn failed(problem: String) -> Lists {
        Lists { designs: Err(problem.clone()), fonts: Err(problem.clone()), schemes: Err(problem) }
    }
}

/// What the page needs of every list's rows, whichever part they list.
trait Listed {
    fn id(&self) -> &str;
    /// The name its tile shows.
    fn name(&self) -> &str;
    fn valid(&self) -> bool;
    fn problems(&self) -> &[String];
}

impl Listed for DesignInfo {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl Listed for FontInfo {
    fn id(&self) -> &str {
        &self.id
    }
    /// A font is named by its family (`IBM Plex Sans`), its pack's name goes in the tooltip; one
    /// that can't be used has no family, then the pack's name.
    fn name(&self) -> &str {
        self.family.as_deref().filter(|family| !family.is_empty()).unwrap_or(&self.name)
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl Listed for ColorSchemeInfo {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

/// Stores the lists of one listing ([`Lists`]).
pub fn listed(state: &mut State, lists: Lists) {
    state.designs = Some(lists.designs);
    state.fonts = Some(lists.fonts);
    state.schemes = Some(lists.schemes);
}

/// Whether nothing has been listed yet, so the shell asks the core when the page opens.
pub fn unlisted(state: &State) -> bool {
    state.designs.is_none()
}

/// The design `id` as read for its row, for a window that cannot read it from the folders
/// (`--demo`, whose entries live in memory). `None` when it is not listed or can't be used.
pub fn design_of(state: &State, id: &str) -> Option<Arc<Design>> {
    part_of(&state.designs, id)
}

/// [`design_of`] for a font.
pub fn font_of(state: &State, id: &str) -> Option<Arc<FontSet>> {
    part_of(&state.fonts, id)
}

/// [`design_of`] for a colour scheme.
pub fn scheme_of(state: &State, id: &str) -> Option<Arc<ColorScheme>> {
    part_of(&state.schemes, id)
}

fn part_of<I: Listed, T>(list: &Option<Result<Vec<Entry<I, T>>, String>>, id: &str) -> Option<Arc<T>> {
    let entries = list.as_ref()?.as_ref().ok()?;
    entries.iter().find(|entry| entry.info.id() == id)?.part.clone()
}

#[derive(Debug, Clone)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    /// −, + or "Reset" of the zoom.
    Zoom(Step),
    /// A part of the suite was switched on or off under Apps (task W.2).
    Component(Component, bool),
    /// "Open" of a part of the suite: its window, by window class and nothing else (task S.21).
    /// The window class alone, deliberately – a target such as `settings:<section>` has no place
    /// here, because a window's settings are opened by a person with its gear ([`apps`]).
    OpenApp(String),
    /// "Open" of Workspaces, which has no window of its own: the Workspaces page.
    OpenWorkspaces,
    /// The tile of a design, a font or a colour scheme (`part`, its id): draw every window with it.
    /// Closes the reasons of an entry that can't be used, if they are shown.
    Choose(Part, String),
    /// The tile of an entry that can't be used (`part`, its id): show its reasons under its grid,
    /// or hide them if they are shown already. Asks nothing of the shell.
    Problems(Part, String),
    /// "Reload": list designs, fonts and colour schemes again, after one was added to or changed
    /// in a folder.
    Reload,
    /// "Open designs folder", "Open fonts folder" or "Open color schemes folder".
    OpenFolder(Part),
    /// "Run setup again" (task W.9).
    RunSetup,
    /// The switch "Maps and place search" (task TE.10).
    MapsOn(bool),
    /// The protocol of the place search service, beside its address.
    GeocoderKind(GeocoderKind),
    /// "Open in Maps with".
    OpenWith(MapsApp),
    /// A key in one of the two address fields: kept as typed, not sent. Saves the other field's
    /// address if it was changed, since the person has left it.
    AddressInput(Address, String),
    /// Enter in an address field: saves it if it changed.
    SaveAddress(Address),
    /// One of the links under "Maps" (the rules of the services, the OpenStreetMap licence).
    OpenLink(&'static str),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    let maps = &data.settings.maps;
    state.maps.settle(maps);
    // The core takes the same names the settings file uses (`"german"`, `"dark"`, `true`), so the
    // values come from the enums' own serialisation. A value that cannot serialise (none here)
    // would become `null`, which the core refuses.
    let (key, value) = match message {
        Message::Language(language) => (SettingKey::Language, serde_json::to_value(language).unwrap_or_default()),
        Message::Appearance(appearance) => {
            (SettingKey::Appearance, serde_json::to_value(appearance).unwrap_or_default())
        }
        Message::StartAtLogin(start) => (SettingKey::StartAtLogin, serde_json::Value::Bool(start)),
        Message::Zoom(step) => return Action::Zoom(step),
        Message::Component(component, enabled) => {
            (SettingKey::Component, serde_json::to_value(ComponentSwitch { component, enabled }).unwrap_or_default())
        }
        Message::OpenApp(app) => return Action::Send(Request::OpenApp { app, target: None }),
        Message::OpenWorkspaces => return Action::Select(Section::Workspaces),
        // The shell draws its own window with it at once, then saves it.
        Message::Choose(part, id) => {
            state.problems = None;
            return Action::Choose(part, id);
        }
        Message::Problems(part, id) => {
            let shown = state.problems.as_ref().is_some_and(|(p, i)| *p == part && *i == id);
            state.problems = if shown { None } else { Some((part, id)) };
            return Action::None;
        }
        Message::Reload => return Action::ListLooks,
        Message::OpenFolder(part) => return Action::OpenFolder(part),
        Message::RunSetup => return Action::OpenSetup,
        Message::MapsOn(on) => return maps_change(state, maps, "enabled", serde_json::Value::Bool(on)),
        Message::GeocoderKind(kind) => {
            return maps_change(state, maps, "geocoderKind", serde_json::to_value(kind).unwrap_or_default());
        }
        Message::OpenWith(app) => {
            return maps_change(state, maps, "openWith", serde_json::to_value(app).unwrap_or_default());
        }
        Message::AddressInput(which, text) => {
            let left = leave_address(state, maps, which.other());
            *state.maps.draft_mut(which) = Some(Draft { text, handed_in: false });
            return left.map_or(Action::None, Action::Send);
        }
        Message::SaveAddress(which) => return save_address(state, maps, which).map_or(Action::None, Action::Send),
        Message::OpenLink(url) => return Action::OpenUrl(url.to_string()),
    };
    Action::Send(Request::SetSetting { key, value })
}

/// `settings.set_setting maps` with the one field `field` set to `value`: every row of "Maps" sets
/// only its own field, so two windows changing two fields never undo each other.
fn maps_request(field: &str, value: serde_json::Value) -> Request {
    Request::SetSetting { key: SettingKey::Maps, value: maps_field(field, value) }
}

/// `{field: value}`, the one field of a change to the maps.
fn maps_field(field: &str, value: serde_json::Value) -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::from_iter([(field.to_string(), value)]))
}

/// A control of "Maps" other than an address field: its own field, after the addresses the person
/// typed and left without Enter (each its own request; the core writes settings one at a time).
fn maps_change(state: &mut State, maps: &Maps, field: &str, value: serde_json::Value) -> Action {
    let mut requests: Vec<Request> =
        [Address::Geocoder, Address::Tiles].into_iter().filter_map(|which| leave_address(state, maps, which)).collect();
    requests.push(maps_request(field, value));
    match requests.len() {
        1 => requests.pop().map_or(Action::None, Action::Send),
        _ => Action::SendAll(requests),
    }
}

/// The request that saves the address `which` as typed (trimmed), or `None` when nothing is typed
/// or it is what is stored. An address the service would refuse (`Maps::merged`, the check
/// `settings.set_setting maps` runs: a query such as `?key=`, a sign-in, tiles without `{z}`,
/// `{x}` and `{y}`) is not sent: its reason is kept for the quiet line under the group, and the
/// field keeps the text so it can be fixed.
fn save_address(state: &mut State, maps: &Maps, which: Address) -> Option<Request> {
    let typed = state.maps.draft(which)?.text.trim().to_string();
    if typed == which.stored(maps) {
        return None;
    }
    let value = serde_json::Value::String(typed.clone());
    *state.maps.draft_mut(which) = Some(Draft { text: typed, handed_in: true });
    if let Err(why) = maps.merged(&maps_field(which.field(), value.clone())) {
        state.maps.problem = Some((which, why));
        return None;
    }
    if state.maps.problem.as_ref().is_some_and(|(refused, _)| *refused == which) {
        state.maps.problem = None;
    }
    Some(maps_request(which.field(), value))
}

/// [`save_address`] when the person has left the field `which` for another control of the group:
/// only once per typing, so keys in the other field do not send it again and again.
fn leave_address(state: &mut State, maps: &Maps, which: Address) -> Option<Request> {
    if state.maps.draft(which)?.handed_in {
        return None;
    }
    save_address(state, maps, which)
}

/// The page General: language, apps, maps, startup, setup.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    let language = |label: &'static str, language: Language| {
        choice(label, settings.language == language, Message::Language(language))
    };
    let language = widgets::group(
        tr("Language"),
        vec![
            language(tr("System"), Language::System),
            language("English", Language::English),
            // Shown in its own name on purpose: someone who cannot read the current language
            // must still find their way back.
            language("Deutsch", Language::German),
        ],
        Some(tr("Applies to apps you open afterwards. “System” follows your system language.").to_string()),
    );

    let startup = widgets::group(
        tr("Startup"),
        vec![setting_row(None, tr("Start at login"), None, switch(settings.start_at_login, Message::StartAtLogin))],
        Some(
            tr("Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready.")
                .to_string(),
        ),
    );

    let run = widgets::text_button(None, tr("Run setup again"), style::button::secondary, Some(Message::RunSetup));
    let setup = widgets::group(
        tr("Setup"),
        vec![setting_row(
            None,
            tr("First-run setup"),
            Some(tr("Nextcloud, components, accounts and AI, step by step").to_string()),
            run,
        )],
        Some(tr("What is connected stays; the steps show it.").to_string()),
    );

    column![language, apps(data), maps(state, &settings.maps), startup, setup].spacing(layout::section_gap()).into()
}

/// The links under "Maps": the rules of the standard services, and the licence of the map data
/// (ODbL), which asks to name OpenStreetMap wherever its data is shown.
const MAP_LINKS: [(&str, &str); 4] = [
    ("Tile usage policy", "https://operations.osmfoundation.org/policies/tiles/"),
    ("Photon", "https://photon.komoot.io/"),
    ("Nominatim usage policy", "https://operations.osmfoundation.org/policies/nominatim/"),
    ("© OpenStreetMap contributors", "https://www.openstreetmap.org/copyright"),
];

/// The group "Maps" (task TE.10, concept `docs/termin-editor.md` "Ort und Karte"), part of the
/// calendar: the switch "Maps and place search", the place search service's address with its
/// protocol, the map tiles' address, and "Open in Maps with". Under it, why it is off until a
/// person turns it on – what each service gets – and the links to their rules and the licence.
///
/// Every row sets only its own field (`settings.set_setting maps`). Turning maps on and changing an
/// address is a person's alone; the control centre is that person (app key), so it may. The
/// address fields save on Enter or when the person moves on ([`MapsDraft`]); a refused address
/// says why in a quiet line under the group.
fn maps<'a>(state: &'a State, maps: &'a Maps) -> Element<'a, Message> {
    let draft = &state.maps;
    let address = |which: Address, placeholder: &'static str| {
        let typed = draft.draft(which).map_or(which.stored(maps), |typed| typed.text.as_str());
        field(placeholder, typed)
            .on_input(move |text| Message::AddressInput(which, text))
            .on_submit(Message::SaveAddress(which))
    };
    // While an address is typed and not saved, its row says how to save it.
    let hint = |which: Address| draft.dirty(which, maps).then(|| tr("Press Enter to save").to_string());
    let kind =
        picker(KIND_CHOICES, KindChoice(maps.geocoder_kind), |chosen: KindChoice| Message::GeocoderKind(chosen.0));
    let search = row![address(Address::Geocoder, Maps::PHOTON), kind]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .width(Length::FillPortion(2));
    let tiles = container(address(Address::Tiles, Maps::OSM_TILES)).width(Length::FillPortion(2));
    let open_with = picker(app_choices(), AppChoice(maps.open_with), |chosen: AppChoice| Message::OpenWith(chosen.0));
    let rows = vec![
        setting_row(
            None,
            tr("Maps and place search"),
            Some(tr("Find places and see a small map in the calendar's event editor").to_string()),
            switch(maps.enabled, Message::MapsOn),
        ),
        setting_row(None, tr("Place search"), hint(Address::Geocoder), search),
        setting_row(None, tr("Map tiles"), hint(Address::Tiles), tiles),
        setting_row(None, tr("Open in Maps with"), None, open_with),
    ];
    let footer = tr(
        "Off until you turn it on. The place search service gets what you search for and the IP address of this computer; the map tile service gets the coordinates of the place. Both are services of others with their own rules, and nothing promises they are available.",
    );
    let links = MAP_LINKS.map(|(label, url)| {
        widgets::text_button(Some(Icon::ArrowUpRight), tr(label), style::button::ghost, Some(Message::OpenLink(url)))
    });
    let mut block = column![widgets::group(tr("Maps"), rows, None)].spacing(layout::item_gap());
    if let Some((_, why)) = &draft.problem {
        block = block.push(notice(trf("Not saved: {reason}", &[("reason", why)])));
    }
    block
        .push(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2))
        .push(row(links).spacing(space::xs()).wrap().vertical_spacing(space::xs()))
        .into()
}

/// A protocol of a place search service in its picker: the service's own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KindChoice(GeocoderKind);

const KIND_CHOICES: [KindChoice; 2] = [KindChoice(GeocoderKind::Photon), KindChoice(GeocoderKind::Nominatim)];

impl std::fmt::Display for KindChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            GeocoderKind::Photon => "Photon",
            GeocoderKind::Nominatim => "Nominatim",
        })
    }
}

/// What "Open in Maps" opens, in its picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppChoice(MapsApp);

impl std::fmt::Display for AppChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Names of services stay as they are; Apple calls its app by the system's language.
        f.write_str(match self.0 {
            MapsApp::Osm => "OpenStreetMap",
            MapsApp::Google => "Google Maps",
            MapsApp::Apple => tr("Apple Maps"),
            MapsApp::Geo => tr("System maps app (geo:)"),
        })
    }
}

/// The choices of "Open in Maps with" on this system: OpenStreetMap and Google Maps everywhere,
/// Apple Maps only on macOS, a `geo:` link only on Linux (where a program may be registered for it).
fn app_choices() -> Vec<AppChoice> {
    let mut choices = vec![AppChoice(MapsApp::Osm), AppChoice(MapsApp::Google)];
    if cfg!(target_os = "macos") {
        choices.push(AppChoice(MapsApp::Apple));
    }
    if cfg!(target_os = "linux") {
        choices.push(AppChoice(MapsApp::Geo));
    }
    choices
}

/// The group "Apps" (task S.21): one row per part of the suite with its switch – a part turned off
/// leaves the app switcher and the tray – and "Open", which brings its window to the front or
/// starts it (`apps.open_app` with the window class, so the inbox's three windows are told apart).
/// "Open" is off while the core has not reported the app as installed. Workspaces has no window of
/// its own: its "Open" shows the Workspaces page, where the templates are.
///
/// Under the name stands the window's class, quietly ([`app_detail`]): that is what a window rule
/// in Hyprland needs, and the control centre is the one place it is written down.
///
/// The App Feed ([`AppId::INBOX_NOTIFICATIONS`]) gets a row of its own after the Notification Hub,
/// with its class and "Open" but no switch: it is a second window of the same part of the suite,
/// not a part of its own. Nothing else in the control centre opens it.
///
/// What a window shows and how it behaves is set in that window, behind its gear; this page only
/// says so and leads there. No "Open" can name a section of the settings (`apps.open_app` with a
/// target `settings:<section>`), because [`Message::OpenApp`] carries nothing but the window class
/// and [`update`] fills in `target: None`: there is no place to put one. That is on purpose – the
/// settings of a window are opened by a person with its gear, not by a button elsewhere. Giving
/// this page such a way means widening the message first, which is where the decision belongs.
fn apps(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = Vec::new();
    for component in Component::ALL {
        let app = window_of(component);
        let open = match app {
            Some(app) => installed(data, app).then(|| Message::OpenApp(app.window_class.to_string())),
            None => Some(Message::OpenWorkspaces),
        };
        // A button without a message is drawn disabled, so a missing app cannot be opened.
        let button = widgets::text_button(None, tr("Open"), style::button::secondary, open);
        let controls = row![
            button,
            switch(data.settings.component_on(component), move |on| { Message::Component(component, on) })
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        rows.push(setting_row(None, component_name(component), app_detail(data, app), controls));
        if component == Component::Inbox {
            rows.push(second_window(data, AppId::INBOX_NOTIFICATIONS, tr("App Feed")));
        }
    }
    let footer = format!(
        "{} {}",
        tr("Its settings are behind the gear in its window."),
        tr(
            "Turned off, a part of the suite leaves the app switcher and the tray, and nothing runs for it in the background. Accounts and data stay."
        ),
    );
    widgets::group(tr("Apps"), rows, Some(footer))
}

/// A second window of a part of the suite (the App Feed of the Notification Hub): its name, its
/// class and "Open", without a switch – what switches it is the part's own row above.
fn second_window<'a>(data: &'a Data, app: AppId, label: &'a str) -> Element<'a, Message> {
    let open = installed(data, app).then(|| Message::OpenApp(app.window_class.to_string()));
    let button = widgets::text_button(None, tr("Open"), style::button::secondary, open);
    // The switch's place stays empty, so every "Open" of the group stands in the same column.
    let controls = row![button, blank().width(switch_width())].spacing(space::l()).align_y(Alignment::Center);
    setting_row(None, label, app_detail(data, Some(app)), controls)
}

/// How wide Iced draws a switch of the suite: twice its height, which is what a row leaves for it.
fn switch_width() -> f32 {
    style::CHECK_SIZE + style::CHECK_SIZE
}

/// The line under an app's name: its window class, for window rules, and "Not installed" while the
/// core does not report it. `None` for a part of the suite without a window of its own
/// (Workspaces), which has neither.
fn app_detail(data: &Data, app: Option<AppId>) -> Option<String> {
    let app = app?;
    let class = trf("Window class: {class}", &[("class", &app.window_class)]);
    match installed(data, app) {
        true => Some(class),
        false => Some(format!("{class} · {}", tr("Not installed"))),
    }
}

/// The window a part of the suite opens: mail is the inbox's mail window, the Notification Hub its
/// hub (task E8.1, each with its own class). Workspaces has none – it is the service itself.
fn window_of(component: Component) -> Option<AppId> {
    match component {
        Component::Calendar => Some(AppId::CALENDAR),
        Component::Tasks => Some(AppId::TASKS),
        Component::Mail => Some(AppId::INBOX_MAIL),
        Component::Inbox => Some(AppId::INBOX),
        Component::Assistant => Some(AppId::ASSISTANT),
        Component::Workspaces => None,
    }
}

/// Whether the core reported the window `app` as installed, so "Open" can start it; `false` until
/// the first answer (`apps.list_apps`).
fn installed(data: &Data, app: AppId) -> bool {
    data.apps
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|listed| listed.window_class == app.window_class && listed.installed)
}

/// The page Appearance: light or dark, design, font, colour scheme, the zoom.
pub fn appearance<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    let interface = widgets::group(
        tr("Interface"),
        vec![setting_row(None, tr("Zoom"), None, zoom_control(settings.zoom))],
        Some(
            tr("Makes all Torro apps larger or smaller, like zooming in a browser: Ctrl + Plus and Ctrl + Minus, Ctrl + 0 resets.")
                .to_string(),
        ),
    );
    let suggested = suggestion(state, settings);
    column![
        color_mode(settings.appearance),
        designs(state, settings),
        fonts(state, settings, suggested.as_ref()),
        schemes(state, settings, suggested.as_ref()),
        interface
    ]
    .spacing(layout::section_gap())
    .into()
}

/// The three colour modes in the order of the segment, each with its symbol and word.
fn modes() -> [(Icon, &'static str, Appearance); 3] {
    [
        (Icon::Monitor, tr("System"), Appearance::System),
        (Icon::Sun, tr("Light"), Appearance::Light),
        (Icon::Moon, tr("Dark"), Appearance::Dark),
    ]
}

/// "Color mode": one row of a settings group, the label at the left and the segment at the right,
/// with the sentence that explains it under the card. The control centre itself follows at once:
/// it reloads the settings after every change. Where the words do not fit beside the label (a
/// narrow window, a long translation) the segment shows its symbols alone, the words as tooltips.
fn color_mode<'a>(chosen: Appearance) -> Element<'a, Message> {
    let active = modes().iter().position(|(_, _, mode)| *mode == chosen).unwrap_or_default();
    let line = responsive(move |room: Size| {
        let words = room.width >= mode_words_need();
        let parts = modes().map(|(which, label, mode)| (which, label, Message::Appearance(mode)));
        setting_row(None, tr("Color mode"), None, widgets::icon_segmented(parts, active, words))
    })
    .height(Length::Shrink);
    let footer = tr("Light or dark colors for all Torro apps. “System” follows your desktop.");
    column![
        container(line).width(Fill).style(style::container::group),
        typo::text(typo::meta(), footer).style(torro_gui::theme::text_2),
    ]
    .spacing(layout::item_gap())
    .into()
}

/// How wide the colour mode's row must be for the segment's words: 16 at both sides and between
/// label and segment (a settings row), the label, the segment with its words.
fn mode_words_need() -> f32 {
    let labels = modes().map(|(_, label, _)| label);
    3.0 * space::l() + typo::text_width(tr("Color mode"), typo::body()) + widgets::icon_segmented_width(&labels, true)
}

/// The block "Design": the heading with "Reload" (for all three lists) and "Open designs folder",
/// the grid, the reasons of the one opened, and the sentence that explains it.
fn designs<'a>(state: &'a State, settings: &'a Settings) -> Element<'a, Message> {
    let body = listing(&state.designs, tr("The designs can't be listed."), Part::Design, state, |entries, open| {
        let drawn = drawn(entries, &settings.design);
        let one = |entry: &'a DesignEntry| {
            let info = &entry.info;
            let tip = byline([Some(info.version.as_str()), info.author.as_deref()]);
            tile(entry, Part::Design, info.id == drawn, open, None, tip, || design_sample(entry))
        };
        entries.iter().map(one).collect()
    });
    let reload = widgets::icon_button(Icon::RefreshCw, tr("Reload"), Some(Message::Reload));
    let footer = tr(
        "Spacing, sizes and corner radii of all Torro apps. More designs go into the designs folder; one that breaks a rule can't be used.",
    );
    block(tr("Design"), vec![reload, open_folder(Part::Design)], body, footer)
}

/// The block "Font": the heading with "Open fonts folder", the grid, the reasons of the one opened,
/// and its sentence. The font the drawn design suggests says so under its name ([`suggestion`]).
fn fonts<'a>(state: &'a State, settings: &'a Settings, suggested: Option<&Suggestion<'a>>) -> Element<'a, Message> {
    let (mark, design) = suggested.map_or((None, ""), |s| (s.font, s.design));
    let body = listing(&state.fonts, tr("The fonts can't be listed."), Part::Font, state, |entries, open| {
        let drawn = drawn(entries, &settings.font);
        let one = |entry: &'a FontEntry| {
            let info = &entry.info;
            let tip = byline([
                Some(info.name.as_str()),
                Some(info.version.as_str()),
                info.author.as_deref(),
                info.license.as_deref(),
            ]);
            let (note, tip) = noted(mark == Some(info.id.as_str()), design, tip);
            tile(entry, Part::Font, info.id == drawn, open, note, tip, || font_sample(entry))
        };
        entries.iter().map(one).collect()
    });
    let footer = tr(
        "The typeface of all Torro apps, sized so that text reads alike in every font. More fonts go into the fonts folder.",
    );
    block(tr("Font"), vec![open_folder(Part::Font)], body, footer)
}

/// The block "Colour scheme": the heading with "Open color schemes folder", the grid, the reasons
/// of the one opened, and its sentence; the colour scheme the drawn design suggests says so.
fn schemes<'a>(state: &'a State, settings: &'a Settings, suggested: Option<&Suggestion<'a>>) -> Element<'a, Message> {
    let (mark, design) = suggested.map_or((None, ""), |s| (s.scheme, s.design));
    let failed = tr("The color schemes can't be listed.");
    let body = listing(&state.schemes, failed, Part::ColorScheme, state, |entries, open| {
        let drawn = drawn(entries, &settings.color_scheme);
        let one = |entry: &'a SchemeEntry| {
            let info = &entry.info;
            let tip = byline([Some(info.version.as_str()), info.author.as_deref()]);
            let (note, tip) = noted(mark == Some(info.id.as_str()), design, tip);
            tile(entry, Part::ColorScheme, info.id == drawn, open, note, tip, || scheme_sample(entry))
        };
        entries.iter().map(one).collect()
    });
    let footer =
        tr("The colors of all Torro apps, light and dark. More color schemes go into the color schemes folder.");
    block(tr("Color scheme"), vec![open_folder(Part::ColorScheme)], body, footer)
}

/// One block of the page: the heading with its quiet `actions`, `body` and the sentence `footer`.
fn block<'a>(
    title: &'a str,
    actions: Vec<Element<'a, Message>>,
    body: Element<'a, Message>,
    footer: &'a str,
) -> Element<'a, Message> {
    column![
        widgets::group_heading(title, actions),
        body,
        typo::text(typo::meta(), footer).style(torro_gui::theme::text_2),
    ]
    .spacing(layout::item_gap())
    .into()
}

/// The body of a block: quiet "Loading …" until the core answered, a row saying that `failed` with
/// the core's reason, or the grid of the tiles `tiles` builds – it gets the entries and the id of
/// the one whose reasons are open – with those reasons under it.
fn listing<'a, I: Listed, T>(
    list: &'a Option<Result<Vec<Entry<I, T>>, String>>,
    failed: &'a str,
    part: Part,
    state: &'a State,
    tiles: impl FnOnce(&'a [Entry<I, T>], Option<&'a str>) -> Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    match list {
        // Quiet until the core answers, which takes a moment at most.
        None => typo::text(typo::body(), tr("Loading …")).style(torro_gui::theme::text_3).into(),
        Some(Err(problem)) => {
            let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
            let row = setting_row(Some(alert.into()), failed, Some(problem.clone()), None::<Element<'a, Message>>);
            container(row).width(Fill).style(style::container::group).into()
        }
        Some(Ok(entries)) => {
            let open = opened(entries, part, state.problems.as_ref());
            let grid = widgets::preview_grid(tiles(entries, open.map(|entry| entry.info.id())));
            match open {
                Some(entry) => column![grid, problems(&entry.info)].spacing(layout::item_gap()).into(),
                None => grid,
            }
        }
    }
}

/// One entry as a tile: `sample` on top, its name, `note` under it ("Suggested"), `tip` as
/// tooltip, and the check mark at the one `drawn` with. One that can't be used is greyed out
/// without its sample; its click shows or hides its reasons (`open`: the id whose reasons are
/// shown).
fn tile<'a, I: Listed, T>(
    entry: &'a Entry<I, T>,
    part: Part,
    drawn: bool,
    open: Option<&str>,
    note: Option<&'a str>,
    tip: Option<String>,
    sample: impl FnOnce() -> Element<'a, Message>,
) -> Element<'a, Message> {
    let info = &entry.info;
    let id = info.id().to_string();
    if !info.valid() {
        let state = PreviewState::Unusable { open: open == Some(info.id()) };
        return widgets::preview_tile(None, info.name(), tip, state, Some(Message::Problems(part, id)));
    }
    let state = if drawn { PreviewState::Chosen } else { PreviewState::Plain };
    widgets::preview_tile_noted(Some(sample()), info.name(), note, tip, state, Some(Message::Choose(part, id)))
}

/// The id of the entry the windows draw with: `chosen` (`Settings::design`, `font` or
/// `color_scheme`) if it is listed and can be used, else the standard, which the suite falls back to
/// (concept: "Wählen und wechseln"). An empty `chosen` (a core from before task L.22) is the
/// standard too.
fn drawn<'a, I: Listed, T>(entries: &'a [Entry<I, T>], chosen: &'a str) -> &'a str {
    if entries.iter().any(|entry| entry.info.id() == chosen && entry.info.valid()) { chosen } else { STANDARD_ID }
}

/// The entry whose reasons are shown: the one `open` names, while its list is `part`'s and it is
/// listed and can't be used. After "Reload" an entry that was fixed shows its sample again, and
/// nothing is left open for it.
fn opened<'a, I: Listed, T>(
    entries: &'a [Entry<I, T>],
    part: Part,
    open: Option<&(Part, String)>,
) -> Option<&'a Entry<I, T>> {
    let (which, id) = open?;
    if *which != part {
        return None;
    }
    entries.iter().find(|entry| entry.info.id() == id && !entry.info.valid())
}

/// What the design drawn suggests and is not drawn: the name of the design, for the tooltip, and the
/// ids of the font and the colour scheme it suggests (`DesignInfo::font`, `color_scheme`), each only
/// when it is listed, can be used and is not the one drawn.
#[derive(Debug, PartialEq, Eq)]
struct Suggestion<'a> {
    design: &'a str,
    font: Option<&'a str>,
    scheme: Option<&'a str>,
}

/// The [`Suggestion`] of the design drawn; `None` while the designs are not listed.
fn suggestion<'a>(state: &'a State, settings: &'a Settings) -> Option<Suggestion<'a>> {
    let designs = state.designs.as_ref()?.as_ref().ok()?;
    let id = drawn(designs, &settings.design);
    let info = &designs.iter().find(|entry| entry.info.id == id)?.info;
    Some(Suggestion {
        design: &info.name,
        font: other(info.font.as_deref(), &state.fonts, &settings.font),
        scheme: other(info.color_scheme.as_deref(), &state.schemes, &settings.color_scheme),
    })
}

/// `suggested` if the list has it as usable and it is not the one drawn of `chosen`.
fn other<'a, I: Listed, T>(
    suggested: Option<&'a str>,
    list: &'a Option<Result<Vec<Entry<I, T>>, String>>,
    chosen: &'a str,
) -> Option<&'a str> {
    let suggested = suggested?;
    let entries = list.as_ref()?.as_ref().ok()?;
    let usable = entries.iter().any(|entry| entry.info.id() == suggested && entry.info.valid());
    (usable && suggested != drawn(entries, chosen)).then_some(suggested)
}

/// The note and the tooltip of a tile that is `suggested` for the design `design`: "Suggested" under
/// its name, "Suggested for {design}" over the rest of the tooltip. Unchanged when it is not.
fn noted(suggested: bool, design: &str, tip: Option<String>) -> (Option<&'static str>, Option<String>) {
    if !suggested {
        return (None, tip);
    }
    let why = trf("Suggested for {design}", &[("design", &design)]);
    let tip = match tip {
        Some(tip) => format!("{why}\n{tip}"),
        None => why,
    };
    (Some(tr("Suggested")), Some(tip))
}

/// Under the grid, for the entry that can't be used whose tile was clicked: its name, "Can't be
/// used:" and every reason, at full strength and wrapping – they say what to fix, one per line, so
/// an entry with many can be worked through like a list.
fn problems<'a, I: Listed>(info: &'a I) -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::CircleAlert, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let name = row![mark, typo::text(typo::body_strong(), info.name())].spacing(space::s()).align_y(Alignment::Center);
    let mut lines = column![name, typo::text(typo::meta(), tr("Can't be used:")).style(torro_gui::theme::text_2)]
        .spacing(space::xs());
    for reason in info.problems() {
        lines = lines.push(typo::text(typo::meta(), reason.as_str()).style(torro_gui::theme::text_2));
    }
    container(lines).width(Fill).padding(layout::panel_padding()).style(style::container::group).into()
}

/// The parts of a tooltip that are there and not empty, joined with " · " ("1.2.0 · Anna Berg");
/// `None` when none is.
fn byline<'s>(parts: impl IntoIterator<Item = Option<&'s str>>) -> Option<String> {
    let parts: Vec<&str> = parts.into_iter().flatten().filter(|part| !part.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The frame of every sample, at the tile's preview size (`widgets::preview_size`): a `line` frame
/// with the radius of the tile's inside, filled with the colour `ground` picks from the window's
/// palette, or nothing (a picture, an empty place).
fn frame<'a>(content: Element<'a, Message>, ground: Option<fn(&Palette) -> Color>) -> Element<'a, Message> {
    let area = widgets::preview_size();
    container(content)
        .width(area.width)
        .height(area.height)
        .clip(true)
        .style(move |t: &Theme| container::Style {
            background: ground.map(|pick| Background::Color(pick(&palette(t)))),
            border: Border { color: palette(t).line, width: size::HAIRLINE, radius: radius::xs().into() },
            ..container::Style::default()
        })
        .into()
}

/// A design's sample: its `preview.png`, or its form in small – the window's `bg` with its `panel`
/// set in by the design's own spacing and corner radius, on it "Aa" at the size of the design's body
/// text and a dot in `accent` – drawn in the active font and colour scheme, so the tile shows only
/// what the design changes.
fn design_sample<'a>(entry: &DesignEntry) -> Element<'a, Message> {
    if let Some(preview) = &entry.preview {
        return frame(svg(preview.clone()).content_fit(ContentFit::Cover).width(Fill).height(Fill).into(), None);
    }
    let Some(design) = entry.part.as_deref() else {
        return frame(blank().into(), None);
    };
    // The design's body level as the active font draws it; one line, so no room above or below it,
    // which the small panel does not have.
    let level = torro_gui::font::active().apply(design.typo.body);
    let aa = typo::text(typo::body(), "Aa")
        .size(Pixels(level.size))
        .line_height(LineHeight::Absolute(Pixels(level.size)))
        .style(|t: &Theme| text::Style { color: Some(palette(t).text) });
    let dot = container(blank().width(space::s()).height(space::s())).style(|t: &Theme| container::Style {
        background: Some(Background::Color(palette(t).accent)),
        border: Border { radius: (space::s() / 2.0).into(), ..Border::default() },
        ..container::Style::default()
    });
    let corner = design.radius.xs;
    let panel = container(row![aa, blank::horizontal(), dot].align_y(Alignment::Center))
        .padding([0.0, design.space.xs])
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).panel)),
            border: Border { radius: corner.into(), ..Border::default() },
            ..container::Style::default()
        });
    // The sidebar's strip at the left, the panel set in by the rest: the suite's frame in small.
    let (inset, strip) = (design.space.xs, design.space.s);
    let inset = Padding { top: inset, right: inset, bottom: inset, left: strip };
    frame(container(panel).padding(inset).into(), Some(|p: &Palette| p.bg))
}

/// A font's sample: "Aa 09" in its text family, Regular, at the size the font draws the active
/// design's body text (its size adjustment applied), in `text` on `panel`.
fn font_sample<'a>(entry: &FontEntry) -> Element<'a, Message> {
    let Some(font) = entry.part.as_deref() else {
        return frame(blank().into(), None);
    };
    let level = font.apply(torro_gui::design::active().typo.body);
    let sample = typo::text(typo::body(), "Aa 09")
        .font(torro_gui::design::font_of(font, FontRole::Text, Weight::Regular))
        .size(Pixels(level.size))
        .line_height(LineHeight::Absolute(Pixels(level.line)))
        .wrapping(text::Wrapping::None)
        .style(|t: &Theme| text::Style { color: Some(palette(t).text) });
    // At the left like the design's "Aa", so the check mark of the chosen one at the right never
    // covers it.
    let sample = container(sample).padding([0.0, space::xs()]).width(Fill).height(Fill).center_y(Fill);
    frame(sample.into(), Some(|p: &Palette| p.panel))
}

/// The four colours a colour scheme's sample shows, left to right.
fn stripes() -> [fn(&Palette) -> Color; 4] {
    [|p| p.bg, |p| p.panel, |p| p.accent, |p| p.text]
}

/// A colour scheme's sample: a stripe each of its `bg`, `panel`, `accent` and `text`, side by side,
/// in the mode the window is in. Inside the frame's line, the outer stripes rounded to follow it.
fn scheme_sample<'a>(entry: &SchemeEntry) -> Element<'a, Message> {
    let Some(scheme) = entry.part.clone() else {
        return frame(blank().into(), None);
    };
    let last = stripes().len() - 1;
    let inner = radius::xs() - size::HAIRLINE;
    let parts = stripes().into_iter().enumerate().map(|(index, pick)| {
        let scheme = Arc::clone(&scheme);
        let left = if index == 0 { inner } else { 0.0 };
        let right = if index == last { inner } else { 0.0 };
        let corners = Radius { top_left: left, bottom_left: left, top_right: right, bottom_right: right };
        let stripe = container(blank().width(Fill).height(Fill)).width(Fill).height(Fill);
        stripe
            .style(move |t: &Theme| container::Style {
                background: Some(Background::Color(pick(&colors(&scheme, t)))),
                border: Border { radius: corners, ..Border::default() },
                ..container::Style::default()
            })
            .into()
    });
    frame(container(row(parts)).padding(size::HAIRLINE).into(), None)
}

/// The colours of `scheme` in the mode of `theme` (the window's light or dark).
fn colors(scheme: &ColorScheme, theme: &Theme) -> Palette {
    let mode = if theme.extended_palette().is_dark { Mode::Dark } else { Mode::Light };
    Palette::of(scheme, mode)
}

/// The folder of `part` in the data directory (`designs/`, `fonts/`, `color-schemes/`); `None`
/// without a home directory.
pub fn folder(part: Part) -> Option<PathBuf> {
    match part {
        Part::Design => torro_core::design::dir(),
        Part::Font => torro_core::design::font_dir(),
        Part::ColorScheme => torro_core::design::color_scheme_dir(),
    }
}

/// "Open designs folder", "Open fonts folder" or "Open color schemes folder", a quiet action of a
/// heading that names the folder (from home, as `~/…`) in its tooltip.
fn open_folder<'a>(part: Part) -> Element<'a, Message> {
    let label = match part {
        Part::Design => tr("Open designs folder"),
        Part::Font => tr("Open fonts folder"),
        Part::ColorScheme => tr("Open color schemes folder"),
    };
    let open = widgets::text_button(Some(Icon::Folder), label, style::button::ghost, Some(Message::OpenFolder(part)));
    match folder(part) {
        Some(dir) => widgets::with_tooltip(open, home_relative(&dir)),
        None => open,
    }
}

/// `path` with the home directory written as `~`, so the part that matters fits the row.
fn home_relative(path: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(std::path::Path::to_path_buf)) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// A PNG as an SVG image for Iced, or `None` when it is not a PNG ([`png_svg`]).
fn png_as_svg(png: &[u8]) -> Option<svg::Handle> {
    png_svg(png).map(|svg| svg::Handle::from_memory(svg.into_bytes()))
}

/// The text of an SVG that shows `png` at its own size, embedded as a `data:` address (the one
/// kind of reference the SVG renderer resolves without touching the disk). `None` when it is not a
/// PNG: no signature, or no `IHDR` chunk with the size at its place.
fn png_svg(png: &[u8]) -> Option<String> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !png.starts_with(SIGNATURE) || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><image width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{}\"/></svg>",
        base64(png)
    ))
}

/// Standard base64 with padding (RFC 4648), for the `data:` address of a preview.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, byte)| n | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// −, the level, + and "Reset". The level keeps the width of the widest one ("200%"), so the
/// buttons stay where they are; a button that would change nothing is off.
fn zoom_control<'a>(level: Zoom) -> Element<'a, Message> {
    let press = |step: Step| (step.apply(level) != level).then_some(Message::Zoom(step));
    let width = typo::text_width(&zoom::label(Zoom::MAX), typo::body().tabular());
    let value = container(typo::text(typo::body().tabular(), zoom::label(level))).center_x(width);
    row![
        widgets::icon_button(Icon::Minus, tr("Zoom out"), press(Step::Out)),
        value,
        widgets::icon_button(Icon::Plus, tr("Zoom in"), press(Step::In)),
        widgets::text_button(None, tr("Reset"), style::button::secondary, press(Step::Reset)),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::settings::Settings;

    /// The key and value the page asked the shell to set.
    fn sent(action: Action) -> (SettingKey, serde_json::Value) {
        let Action::Send(Request::SetSetting { key, value }) = action else {
            panic!("expected SetSetting, got {action:?}");
        };
        (key, value)
    }

    #[test]
    fn each_language_choice_sets_only_the_language() {
        let cases = [(Language::System, "system"), (Language::English, "english"), (Language::German, "german")];
        for (language, name) in cases {
            let action = update(&mut State::default(), Message::Language(language), &Data::default());
            assert_eq!(sent(action), (SettingKey::Language, serde_json::json!(name)), "choice {language:?}");
        }
    }

    #[test]
    fn each_appearance_choice_sets_only_the_appearance() {
        let cases = [(Appearance::System, "system"), (Appearance::Light, "light"), (Appearance::Dark, "dark")];
        for (appearance, name) in cases {
            let action = update(&mut State::default(), Message::Appearance(appearance), &Data::default());
            assert_eq!(sent(action), (SettingKey::Appearance, serde_json::json!(name)), "choice {appearance:?}");
        }
    }

    #[test]
    fn start_at_login_sets_only_the_switch() {
        let on = update(&mut State::default(), Message::StartAtLogin(true), &Data::default());
        assert_eq!(sent(on), (SettingKey::StartAtLogin, serde_json::json!(true)));
        let off = update(&mut State::default(), Message::StartAtLogin(false), &Data::default());
        assert_eq!(sent(off), (SettingKey::StartAtLogin, serde_json::json!(false)));
    }

    #[test]
    fn the_zoom_buttons_zoom_the_window() {
        for step in [Step::In, Step::Out, Step::Reset] {
            let action = update(&mut State::default(), Message::Zoom(step), &Data::default());
            assert!(matches!(action, Action::Zoom(sent) if sent == step), "{step:?}: {action:?}");
        }
    }

    /// Tasks L.18 and L.23: a tile of each part draws with it, "Reload" lists all three again and
    /// each heading opens its own folder – each without a question first.
    #[test]
    fn each_tile_draws_with_its_part_and_the_headings_list_and_open() {
        for part in [Part::Design, Part::Font, Part::ColorScheme] {
            let choose = update(&mut State::default(), Message::Choose(part, "harbor".into()), &Data::default());
            assert!(matches!(choose, Action::Choose(p, ref id) if p == part && id == "harbor"), "{choose:?}");
            let open = update(&mut State::default(), Message::OpenFolder(part), &Data::default());
            assert!(matches!(open, Action::OpenFolder(p) if p == part), "{open:?}");
        }
        let reload = update(&mut State::default(), Message::Reload, &Data::default());
        assert!(matches!(reload, Action::ListLooks), "{reload:?}");
        assert_eq!(folder(Part::Font), torro_core::design::font_dir());
        assert_eq!(folder(Part::ColorScheme), torro_core::design::color_scheme_dir());
    }

    /// Task L.18: the colour mode's segment sets the appearance, one segment per mode in the order
    /// System, Light, Dark, with the monitor, the sun and the moon.
    #[test]
    fn each_segment_of_the_colour_mode_sets_its_mode() {
        let symbols = modes().map(|(which, _, mode)| (which, mode));
        assert_eq!(
            symbols,
            [(Icon::Monitor, Appearance::System), (Icon::Sun, Appearance::Light), (Icon::Moon, Appearance::Dark)]
        );
        for (_, _, mode) in modes() {
            let action = update(&mut State::default(), Message::Appearance(mode), &Data::default());
            assert_eq!(sent(action).0, SettingKey::Appearance, "{mode:?}");
        }
        assert!(mode_words_need() < size::content_max(), "a wide page shows the words");
        for mode in [Appearance::System, Appearance::Light, Appearance::Dark] {
            let _: Element<'_, Message> = color_mode(mode);
        }
    }

    /// The demo's lists, as the page keeps them.
    fn demo() -> State {
        let mut state = State::default();
        listed(&mut state, super::super::launch::demo_looks());
        state
    }

    /// Tasks L.18 and L.23: a click on a tile that can't be used shows its reasons, a second click
    /// hides them, a click on another one swaps them – also to another part's, the same id in
    /// another list being another entry; choosing anything closes them. None of it asks the shell.
    #[test]
    fn an_unusable_tile_opens_and_closes_its_reasons() {
        let data = Data::default();
        let mut state = demo();
        let open = |state: &State, part: Part| match part {
            Part::Design => {
                ok(&state.designs).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
            Part::Font => {
                ok(&state.fonts).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
            Part::ColorScheme => {
                ok(&state.schemes).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
        };
        assert_eq!(open(&state, Part::Design), None);

        let action = update(&mut state, Message::Problems(Part::Design, "neon".into()), &data);
        assert!(matches!(action, Action::None), "{action:?}");
        assert_eq!(open(&state, Part::Design).as_deref(), Some("neon"));
        assert_eq!(open(&state, Part::ColorScheme), None, "the scheme \"neon\" is another entry");
        let _: Element<'_, Message> = appearance(&state, &data);

        let _ = update(&mut state, Message::Problems(Part::Design, "neon".into()), &data);
        assert_eq!(state.problems, None, "a second click hides them");

        let _ = update(&mut state, Message::Problems(Part::Design, "neon".into()), &data);
        let _ = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data);
        assert_eq!((open(&state, Part::Design), open(&state, Part::ColorScheme).as_deref()), (None, Some("neon")));
        let _ = update(&mut state, Message::Problems(Part::Font, "brush".into()), &data);
        assert_eq!(open(&state, Part::Font).as_deref(), Some("brush"), "another tile swaps them");
        let _: Element<'_, Message> = appearance(&state, &data);
        let _ = update(&mut state, Message::Problems(Part::Font, "tabular".into()), &data);
        assert_eq!(open(&state, Part::Font), None, "nor does an entry that can be used");

        let _ = update(&mut state, Message::Problems(Part::Font, "brush".into()), &data);
        let _ = update(&mut state, Message::Choose(Part::ColorScheme, "harbor".into()), &data);
        assert_eq!(state.problems, None, "choosing closes them");
    }

    /// A list as the page keeps it, when the core listed it.
    fn ok<I, T>(list: &Option<Result<Vec<Entry<I, T>>, String>>) -> Option<&[Entry<I, T>]> {
        list.as_ref()?.as_ref().ok().map(Vec::as_slice)
    }

    /// Task L.23: designs and colour schemes say version and author, fonts the pack, its version,
    /// author and licence; a font is named by its family, one that can't be used by its pack.
    #[test]
    fn the_tooltip_of_a_tile_says_who_made_it() {
        assert_eq!(byline([Some("1.2.0"), Some("Torro samples")]).as_deref(), Some("1.2.0 · Torro samples"));
        assert_eq!(byline([Some(""), None]), None);
        let state = demo();
        let fonts = ok(&state.fonts).expect("fonts");
        let tabular = &fonts.iter().find(|e| e.info.id == "tabular").expect("tabular").info;
        assert_eq!(tabular.name(), "Inter Tabular", "named by its family");
        let tip = byline([
            Some(tabular.name.as_str()),
            Some(tabular.version.as_str()),
            tabular.author.as_deref(),
            tabular.license.as_deref(),
        ]);
        assert_eq!(tip.as_deref(), Some("Tabular · 1.0.0 · Torro samples · OFL-1.1"));
        let brush = &fonts.iter().find(|e| e.info.id == "brush").expect("brush").info;
        assert_eq!(brush.name(), "Brush", "no family: its pack's name");
    }

    /// The check mark of each part is at the one the windows draw with.
    #[test]
    fn the_check_mark_is_at_the_part_the_windows_draw_with() {
        let state = demo();
        let designs = ok(&state.designs).expect("designs");
        assert_eq!(drawn(designs, "harbor"), "harbor");
        assert_eq!(drawn(designs, "torro"), "torro");
        assert_eq!(drawn(designs, "neon"), "torro", "a broken design is not drawn: the standard is");
        assert_eq!(drawn(designs, "gone"), "torro", "nor a missing one");
        let fonts = ok(&state.fonts).expect("fonts");
        assert_eq!(drawn(fonts, "tabular"), "tabular");
        assert_eq!(drawn(fonts, ""), "torro", "a core from before task L.22 names none");
        assert_eq!(drawn(ok(&state.schemes).expect("schemes"), "neon"), "torro");
    }

    /// Task L.23: the tiles of the font and the colour scheme the drawn design suggests say so, as
    /// long as they are not drawn, are listed and can be used; the standard suggests nothing.
    #[test]
    fn the_suggestions_of_the_drawn_design_are_marked() {
        let state = demo();
        let mut settings = Settings { design: "harbor".into(), ..Settings::default() };
        let marked = suggestion(&state, &settings).expect("listed");
        assert_eq!(marked, Suggestion { design: "Harbor", font: Some("tabular"), scheme: Some("harbor") });

        settings.font = "tabular".into();
        assert_eq!(suggestion(&state, &settings).map(|s| s.font), Some(None), "drawn already");
        settings.design = "paper".into();
        let paper = suggestion(&state, &settings).expect("listed");
        assert_eq!((paper.font, paper.scheme), (None, Some("paper")), "Paper suggests no font");
        settings.design = "neon".into();
        let standard = suggestion(&state, &settings).expect("listed");
        assert_eq!(standard, Suggestion { design: "Torro", font: None, scheme: None }, "a broken one is not drawn");
        assert_eq!(suggestion(&State::default(), &settings), None);

        let (note, tip) = noted(true, "Harbor", Some("1.2.0".into()));
        assert_eq!(note, Some(tr("Suggested")));
        let why = trf("Suggested for {design}", &[("design", &"Harbor")]);
        assert_eq!(tip, Some(format!("{why}\n1.2.0")));
        assert_eq!(noted(false, "Harbor", Some("1.2.0".into())), (None, Some("1.2.0".into())));
        assert_eq!(noted(true, "Harbor", None).1, Some(why));

        // The other suggestion of a design that is not there (or broken) marks nothing.
        let broken = Some("neon");
        assert_eq!(other(broken, &state.schemes, "torro"), None);
        assert_eq!(other(Some("gone"), &state.fonts, "torro"), None);
    }

    #[test]
    fn listed_parts_are_kept_with_what_they_draw_with() {
        let mut state = State::default();
        assert!(unlisted(&state));
        listed(&mut state, super::super::launch::demo_looks());
        assert!(!unlisted(&state));
        assert!(design_of(&state, "paper").is_some(), "an entry that can be used carries its part");
        assert!(font_of(&state, "tabular").is_some());
        assert!(scheme_of(&state, "harbor").is_some());
        assert!(design_of(&state, "neon").is_none(), "one that can't, none");
        assert!(font_of(&state, "brush").is_none());
        assert!(scheme_of(&state, "gone").is_none());
        listed(&mut state, Lists::failed("core is not running".into()));
        assert!(design_of(&state, "paper").is_none());
        assert!(font_of(&state, "tabular").is_none());
    }

    #[test]
    fn page_builds_while_loading_listed_and_failed() {
        let data = Data::default();
        let (loading, listing, mut failed) = (State::default(), demo(), State::default());
        listed(&mut failed, Lists::failed("core is not running".into()));
        // One list failed, the others listed.
        let mut partly = demo();
        partly.fonts = Some(Err("no fonts".into()));
        for state in [&loading, &listing, &failed, &partly] {
            let _page: Element<'_, Message> = view(state, &data);
            let _page: Element<'_, Message> = appearance(state, &data);
        }
        let marked = Data { settings: Settings { design: "harbor".into(), ..Settings::default() }, ..Data::default() };
        let _page: Element<'_, Message> = appearance(&listing, &marked);
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        let cases = [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg==")];
        for (plain, coded) in cases.into_iter().chain([("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]) {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain:?}");
        }
    }

    /// A pack's `preview.png` is drawn through the SVG renderer: the SVG built around it must show
    /// the picture's pixels at its size.
    #[test]
    fn a_preview_png_is_drawn_as_an_svg_image() {
        use resvg::tiny_skia::{Color, Pixmap};
        let mut picture = Pixmap::new(6, 4).expect("pixmap");
        picture.fill(Color::from_rgba8(0x14, 0xB8, 0xA6, 0xFF));
        let png = picture.encode_png().expect("png");
        let svg = png_svg(&png).expect("a PNG");
        assert!(svg.contains("width=\"6\" height=\"4\""), "{svg}");

        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("svg");
        let mut drawn = Pixmap::new(6, 4).expect("pixmap");
        resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut drawn.as_mut());
        let pixel = drawn.pixel(3, 2).expect("pixel");
        assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (0x14, 0xB8, 0xA6, 0xFF));

        assert!(png_svg(b"GIF89a").is_none(), "not a PNG");
        assert!(png_svg(&png[..20]).is_none(), "cut short");
        assert!(png_as_svg(&png).is_some());
    }

    #[test]
    fn the_folder_is_shown_from_home() {
        if let Some(home) = std::env::home_dir() {
            let inside = home.join("designs");
            assert_eq!(home_relative(&inside), format!("~{}designs", std::path::MAIN_SEPARATOR));
        }
        let outside = std::path::Path::new("/opt/torro/designs");
        assert_eq!(home_relative(outside), outside.display().to_string());
    }

    /// Task S.21: General › Apps switches a part of the suite and opens its window by window
    /// class; Workspaces has no window and leads to its page instead. "Open" stays off while the
    /// core has not reported an app as installed.
    #[test]
    fn the_apps_group_switches_a_part_and_opens_its_window() {
        let data = Data::default();
        let set = |component, on| sent(update(&mut State::default(), Message::Component(component, on), &data));
        assert_eq!(
            set(Component::Calendar, false),
            (SettingKey::Component, serde_json::json!({"component": "calendar", "enabled": false}))
        );
        assert_eq!(
            set(Component::Mail, true),
            (SettingKey::Component, serde_json::json!({"component": "mail", "enabled": true}))
        );
        // The inbox's windows share a binary: mail is opened by its own class (task E8.1).
        assert_eq!(window_of(Component::Mail).map(|app| app.window_class), Some("torro-inbox-mail"));
        assert_eq!(window_of(Component::Inbox).map(|app| app.window_class), Some("torro-inbox"));
        assert_eq!(window_of(Component::Workspaces), None, "no window of its own");
        let open = update(&mut State::default(), Message::OpenApp("torro-inbox-mail".into()), &data);
        assert!(matches!(open, Action::Send(Request::OpenApp { ref app, target: None }) if app == "torro-inbox-mail"));
        let workspaces = update(&mut State::default(), Message::OpenWorkspaces, &data);
        assert!(matches!(workspaces, Action::Select(Section::Workspaces)), "{workspaces:?}");

        assert!(!installed(&data, AppId::CALENDAR), "no answer yet");
        let listed = |class: &str, installed: bool| torro_ipc::AppInfo {
            binary: "torroinbox".into(),
            name: "Torro".into(),
            window_class: class.into(),
            installed,
        };
        let apps = Data { apps: Some(vec![listed("torro-inbox-mail", true), listed("torro-inbox", false)]), ..data };
        assert!(installed(&apps, AppId::INBOX_MAIL));
        assert!(!installed(&apps, AppId::INBOX));
        drop(view(&State::default(), &apps));
    }

    /// Found in the review of task S.22: the window class belongs on the row (window rules in
    /// Hyprland need it and the control centre is the one place it is written down), and the App
    /// Feed must be reachable – it is a window of the Notification Hub, not a part of the suite,
    /// so it gets no switch of its own.
    #[test]
    fn every_app_row_names_its_window_class_and_the_app_feed_can_be_opened() {
        let listed = |class: &str, installed: bool| torro_ipc::AppInfo {
            binary: "torroinbox".into(),
            name: "Torro".into(),
            window_class: class.into(),
            installed,
        };
        let class_of = |app: AppId| trf("Window class: {class}", &[("class", &app.window_class)]);

        // Every window of the suite is in the list; only Workspaces has none.
        let all: Vec<torro_ipc::AppInfo> =
            AppId::ALL.iter().map(|app| listed(app.window_class, true)).collect::<Vec<_>>();
        let data = Data { apps: Some(all), ..Data::default() };
        for component in Component::ALL {
            let app = window_of(component);
            assert_eq!(app_detail(&data, app), app.map(class_of), "{component:?}");
        }
        assert_eq!(window_of(Component::Workspaces), None);

        // Not installed: the class stays, the state follows it.
        let none = Data::default();
        assert_eq!(
            app_detail(&none, Some(AppId::CALENDAR)),
            Some(format!("{} · {}", class_of(AppId::CALENDAR), tr("Not installed")))
        );

        // The App Feed opens by its own class, and only while the core reports it.
        assert_eq!(AppId::INBOX_NOTIFICATIONS.window_class, "torro-inbox-notifications");
        let feed =
            update(&mut State::default(), Message::OpenApp(AppId::INBOX_NOTIFICATIONS.window_class.into()), &data);
        assert!(
            matches!(feed, Action::Send(Request::OpenApp { ref app, target: None })
                if app == "torro-inbox-notifications"),
            "{feed:?}"
        );
        drop(view(&State::default(), &data));
        drop(second_window(&none, AppId::INBOX_NOTIFICATIONS, tr("App Feed")));
    }

    /// Task TE.10: the switch, the protocol of the place search and "Open in Maps with" each set
    /// only their own field of `maps`, under the names the core reads.
    #[test]
    fn each_maps_row_sets_only_its_field() {
        let data = Data::default();
        let set = |message| sent(update(&mut State::default(), message, &data));
        assert_eq!(set(Message::MapsOn(true)), (SettingKey::Maps, serde_json::json!({"enabled": true})));
        assert_eq!(set(Message::MapsOn(false)), (SettingKey::Maps, serde_json::json!({"enabled": false})));
        assert_eq!(
            set(Message::GeocoderKind(GeocoderKind::Nominatim)),
            (SettingKey::Maps, serde_json::json!({"geocoderKind": "nominatim"}))
        );
        assert_eq!(
            set(Message::OpenWith(MapsApp::Google)),
            (SettingKey::Maps, serde_json::json!({"openWith": "google"}))
        );
        // Each change reads as a change of the core's own settings, touching nothing else.
        for message in [Message::MapsOn(true), Message::OpenWith(MapsApp::Geo)] {
            let (_, value) = set(message);
            let merged = Maps::default().merged(&value).expect("the core takes it");
            let untouched = Maps { enabled: merged.enabled, open_with: merged.open_with, ..Maps::default() };
            assert_eq!(merged, untouched);
        }
    }

    /// Task TE.10: an address is sent on Enter, trimmed and alone, not on every key; Enter on what
    /// is stored sends nothing.
    #[test]
    fn an_address_is_saved_on_enter_with_only_its_field() {
        let data = Data::default();
        let mut state = State::default();
        let typed =
            update(&mut state, Message::AddressInput(Address::Geocoder, " https://photon.example.org ".into()), &data);
        assert!(matches!(typed, Action::None), "no request while typing: {typed:?}");
        assert!(state.maps.dirty(Address::Geocoder, &data.settings.maps));
        assert_eq!(
            sent(update(&mut state, Message::SaveAddress(Address::Geocoder), &data)),
            (SettingKey::Maps, serde_json::json!({"geocoder": "https://photon.example.org"}))
        );

        let tiles = "https://tiles.example.org/{z}/{x}/{y}.png";
        let _ = update(&mut state, Message::AddressInput(Address::Tiles, tiles.into()), &data);
        assert_eq!(
            sent(update(&mut state, Message::SaveAddress(Address::Tiles), &data)),
            (SettingKey::Maps, serde_json::json!({"tiles": tiles}))
        );

        let mut same = State::default();
        let _ = update(&mut same, Message::AddressInput(Address::Tiles, Maps::OSM_TILES.into()), &data);
        let enter = update(&mut same, Message::SaveAddress(Address::Tiles), &data);
        assert!(matches!(enter, Action::None), "unchanged: {enter:?}");
        assert!(matches!(update(&mut State::default(), Message::SaveAddress(Address::Tiles), &data), Action::None));
    }

    /// Task TE.10: moving on to the other field or another control of the group saves the address
    /// left behind, each field in its own request.
    #[test]
    fn leaving_an_address_saves_it() {
        let data = Data::default();
        let mut state = State::default();
        let _ =
            update(&mut state, Message::AddressInput(Address::Geocoder, "https://photon.example.org".into()), &data);
        let left = update(&mut state, Message::AddressInput(Address::Tiles, "h".into()), &data);
        assert_eq!(sent(left), (SettingKey::Maps, serde_json::json!({"geocoder": "https://photon.example.org"})));
        // The next keys in the other field send it no second time, while the core answers.
        let again = update(&mut state, Message::AddressInput(Address::Tiles, "ht".into()), &data);
        assert!(matches!(again, Action::None), "{again:?}");

        let mut state = State::default();
        let tiles = "https://tiles.example.org/{z}/{x}/{y}.png";
        let _ = update(&mut state, Message::AddressInput(Address::Tiles, tiles.into()), &data);
        let Action::SendAll(requests) = update(&mut state, Message::MapsOn(true), &data) else {
            panic!("expected two requests");
        };
        let values: Vec<serde_json::Value> = requests
            .into_iter()
            .map(|request| match request {
                Request::SetSetting { key: SettingKey::Maps, value } => value,
                other => panic!("expected maps, got {other:?}"),
            })
            .collect();
        assert_eq!(values, [serde_json::json!({"tiles": tiles}), serde_json::json!({"enabled": true})]);
    }

    /// Task TE.10: an address the service refuses (a key in the query) is not sent; the field keeps
    /// it and the group says why, with the service's own reason, until it is fixed.
    #[test]
    fn a_refused_address_says_why_and_is_not_sent() {
        let data = Data::default();
        let mut state = State::default();
        let keyed = "https://tiles.example.org/{z}/{x}/{y}.png?key=secret";
        let _ = update(&mut state, Message::AddressInput(Address::Tiles, keyed.into()), &data);
        let enter = update(&mut state, Message::SaveAddress(Address::Tiles), &data);
        assert!(matches!(enter, Action::None), "{enter:?}");
        let why = Maps::default().merged(&serde_json::json!({"tiles": keyed})).expect_err("refused by the core");
        assert_eq!(state.maps.problem, Some((Address::Tiles, why)));
        assert_eq!(state.maps.draft(Address::Tiles).map(|d| d.text.as_str()), Some(keyed), "kept to be fixed");
        // Over to the other field (typing what is stored there, so it has nothing to save).
        let away = update(&mut state, Message::AddressInput(Address::Geocoder, Maps::PHOTON.into()), &data);
        assert!(matches!(away, Action::None), "refused once, leaving does not try again: {away:?}");
        assert!(state.maps.problem.is_some(), "and still says why");
        let _: Element<'_, Message> = view(&state, &data);

        let fixed = "https://tiles.example.org/{z}/{x}/{y}.png";
        let _ = update(&mut state, Message::AddressInput(Address::Tiles, fixed.into()), &data);
        assert_eq!(sent(update(&mut state, Message::SaveAddress(Address::Tiles), &data)).1["tiles"], fixed);
        assert_eq!(state.maps.problem, None);

        // Once the core stores it, the field follows the core again.
        let stored = Data {
            settings: Settings { maps: Maps { tiles: fixed.into(), ..Maps::default() }, ..Settings::default() },
            ..Data::default()
        };
        let _ = update(&mut state, Message::OpenLink(MAP_LINKS[0].1), &stored);
        assert_eq!(state.maps.draft(Address::Tiles), None);
    }

    /// Task TE.10: the links under the group open the services' rules and the licence; Apple Maps
    /// is offered on macOS only, the `geo:` link on Linux only.
    #[test]
    fn the_maps_links_open_and_the_apps_fit_the_system() {
        for (label, url) in MAP_LINKS {
            let open = update(&mut State::default(), Message::OpenLink(url), &Data::default());
            assert!(matches!(open, Action::OpenUrl(ref sent) if sent == url), "{label}: {open:?}");
        }
        assert!(MAP_LINKS.iter().any(|(_, url)| *url == "https://www.openstreetmap.org/copyright"));
        let apps: Vec<MapsApp> = app_choices().into_iter().map(|choice| choice.0).collect();
        assert_eq!(apps.contains(&MapsApp::Apple), cfg!(target_os = "macos"));
        assert_eq!(apps.contains(&MapsApp::Geo), cfg!(target_os = "linux"));
        assert_eq!(AppChoice(MapsApp::Apple).to_string(), tr("Apple Maps"));
        assert_eq!(AppChoice(MapsApp::Geo).to_string(), tr("System maps app (geo:)"));
        assert_eq!(KIND_CHOICES.map(|choice| choice.to_string()), ["Photon", "Nominatim"]);
    }

    #[test]
    fn page_builds_with_default_and_stored_settings() {
        let mut settings = Settings {
            language: Language::German,
            appearance: Appearance::Dark,
            start_at_login: false,
            design: "plex".into(),
            ..Settings::default()
        };
        settings.zoom = torro_core::settings::Zoom::MAX;
        settings.components_off = [Component::Mail].into_iter().collect();
        let stored = Data { settings, ..Data::default() };
        let (state, default) = (State::default(), Data::default());
        let _default: Element<'_, Message> = view(&state, &default);
        let _stored: Element<'_, Message> = view(&state, &stored);
        let _default: Element<'_, Message> = appearance(&state, &default);
        let _stored: Element<'_, Message> = appearance(&state, &stored);
    }
}
