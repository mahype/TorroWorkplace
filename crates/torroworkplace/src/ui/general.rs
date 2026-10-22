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
//!   glance; variant B of issue #291 since task L.37): three groups – "Design" (a compact list of
//!   the designs), "Font and colors" (the fonts as a row of buttons, the colour scheme as a
//!   dropdown) and "Display" ("Color mode" as a segment of System, Light, Dark with symbol and word,
//!   and the zoom of the suite with −, the value, + and "Reset", task Z.1).
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
//! # Design, font and colour scheme (tasks L.4, L.18, L.23 and L.37, concept `docs/design-pakete.md`)
//!
//! The three parts of the look are chosen apart, one entry per part as the core lists them
//! (`settings.list_designs`, `settings.list_fonts`, `settings.list_color_schemes`, asked by the
//! shell when the page opens and on "Reload"). The board "Appearance · Variant B (chosen)" of
//! issue #291 is the model:
//!
//! - **Design** – a row per design: a small mail window drawn from the design's own values in the
//!   active colour scheme ([`MailPreview`]; a design's `preview.png` instead, if it has one), its
//!   name and description, three dots and the word of its density (`DesignInfo::density`, never
//!   reckoned here), and the check mark at the one drawn. "version · author" as tooltip of the
//!   picture.
//! - **Font** – a row of buttons, each named by its family and written in it, the one drawn with a
//!   check mark; the font the drawn design suggests has a dot, and the row's sentence says so
//!   ([`FontHint`]). "pack · version · author · licence" as tooltip.
//! - **Colour scheme** – a dropdown: four stripes (`bg`, `panel`, `text`, `accent` in the window's
//!   light or dark mode) and the name, in the sections "Torro", "Light and dark" and "Dark only"
//!   (`ColorSchemeInfo::dark_only`); an empty section is left out ([`scheme_groups`]).
//!
//! Choosing is [`Action::Choose`]: the shell draws its own window with it at once and saves it
//! (`settings.set_setting design | font | color_scheme`); no question first. Choosing a design
//! lets the core set the font and mail view it suggests too (task L.33), never the colour scheme.
//! The page keeps what font and mail view were drawn before and compares them with the reload that
//! carries the new design ([`loaded`]): what changed is named under the group, "Also switched:
//! plain-text mail"; nothing changed, no hint.
//!
//! An entry that breaks a rule is greyed out with an alert mark. A click on it shows, under its
//! group, its name, "Can't be used:" and every reason in full, wrapping – a list of reasons is too
//! long for a tooltip and must be readable without a mouse over it. A second click, or a click on
//! another one, closes or swaps that ([`Message::Problems`]); one entry's reasons are shown at a
//! time on the whole page.
//!
//! "Reload" (at the heading "Design": it lists all three again) and "Open … folder" (created with
//! its examples if it is missing, its path as tooltip) are quiet actions at the right of the
//! headings. Opening a folder is a local action of the control centre without a request: it only
//! shows the file manager on this desktop (`docs/api.md`, footnote 9).
//!
//! The lists come from the core; the parts for the previews are read here, only for the ids the
//! core listed as usable, the way every window reads the part it draws with
//! (`torro_core::design::load`, `load_font`, `load_color_scheme`): [`Entry`].

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{button, canvas, column, container, responsive, row, rule, scrollable, space as blank, svg, text};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Fill, Length, Padding, Pixels, Point, Rectangle,
    Renderer, Size, Theme, mouse,
};
use torro_core::AppId;
use torro_core::design::{
    ColorScheme, DensityLevel, Design, FontRole, FontSet, Level, ListSelection, ListStyle, Mode, Part, STANDARD_ID,
    Weight,
};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{
    Component, ComponentSwitch, GeocoderKind, Language, MailFormat, Maps, MapsApp, Settings, Zoom, address_problem,
};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{Palette, palette, radius, size, space};
use torro_gui::widgets::{Align, Anchor, Placement};
use torro_gui::zoom::{self, Step};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{ColorSchemeInfo, DesignInfo, FontInfo, Request, SettingKey};
use torro_settings_ui::parts::{choice, field, notice, picker, row_frame, switch};

use super::setup::component_name;
use super::{Action, Data, Section, setting_row};

#[derive(Debug, Default)]
pub struct State {
    /// The designs as the core listed them last, the standard first; `Err` when it did not answer,
    /// `None` until the first answer. The fonts and colour schemes alike.
    designs: Option<Result<Vec<DesignEntry>, String>>,
    fonts: Option<Result<Vec<FontEntry>, String>>,
    schemes: Option<Result<Vec<SchemeEntry>, String>>,
    /// The entry that can't be used whose reasons are shown under its group (its part and id), after
    /// a click on it; `None` while no reasons are shown. The part, because one id can name a
    /// design, a font and a colour scheme at once (`plex`).
    problems: Option<(Part, String)>,
    /// The addresses of "Maps" as typed and not saved yet ([`maps`]).
    maps: MapsDraft,
    /// Whether the colour scheme's dropdown is open ([`overlay`]).
    schemes_open: bool,
    /// Where the dropdown's button is, so its menu hangs under it.
    anchor: Anchor,
    /// What was drawn when a design was chosen here, until the reload that carries the choice
    /// ([`loaded`]).
    before: Option<Before>,
    /// What the core switched with the design chosen here last, named under the group "Design";
    /// `None` when nothing was switched (or nothing chosen yet).
    switched: Option<Switched>,
}

/// The settings a design may switch (task L.33), as they were when the design `design` was chosen
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Before {
    design: String,
    font: String,
    mail_view: MailFormat,
}

/// What the core switched together with a design (task L.37, "Also switched: …"): the new font's id
/// and the new mail view, each `None` when it stayed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Switched {
    font: Option<String>,
    mail_view: Option<MailFormat>,
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
    /// (`Maps::merged`, which `settings.set_setting maps` runs), its reason as a sentence for people
    /// (`address_problem`), translated and shown quietly under the group.
    problem: Option<(Address, &'static str)>,
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

/// What the design `id` suggests that the core would switch with it (task L.33), for the demo, which
/// has no core and does it itself: its font, only when it is listed as usable, and its mail view.
pub fn suggestions_of(state: &State, id: &str) -> (Option<String>, Option<MailFormat>) {
    let Some(info) = ok(&state.designs).and_then(|designs| designs.iter().find(|entry| entry.info.id == id)) else {
        return (None, None);
    };
    let info = &info.info;
    let usable = |font: &&String| {
        ok(&state.fonts).is_some_and(|fonts| fonts.iter().any(|entry| entry.info.id == **font && entry.info.valid))
    };
    (info.font.as_ref().filter(usable).cloned(), info.mail_view)
}

/// After a reload ([`Data`] with `settings`): once the reload carries the design chosen here –
/// `waiting` says whether the shell still waits for it to be saved –, names what the core switched
/// with it, compared with what was drawn before the choice. A design that was refused (the settings
/// name another one) leaves no hint. Nothing happens while no design was chosen here.
pub fn loaded(state: &mut State, settings: &Settings, waiting: bool) {
    if waiting || state.before.is_none() {
        return;
    }
    let Some(before) = state.before.take() else { return };
    if settings.design != before.design {
        return;
    }
    let font = (settings.font != before.font).then(|| settings.font.clone());
    let mail_view = (settings.mail_view != before.mail_view).then_some(settings.mail_view);
    state.switched = (font.is_some() || mail_view.is_some()).then_some(Switched { font, mail_view });
}

/// Leaving the page closes the colour scheme's dropdown.
pub fn leave(state: &mut State) {
    state.schemes_open = false;
}

/// What Escape does on the page: close the colour scheme's dropdown, if it is open.
pub fn escape(state: &State) -> Option<Message> {
    state.schemes_open.then_some(Message::CloseSchemes)
}

/// What the core switched with the design chosen here last, for the shell's tests.
#[cfg(test)]
pub fn switched(state: &State) -> Option<&Switched> {
    state.switched.as_ref()
}

/// A list as the page keeps it, when the core listed it.
fn ok<I, T>(list: &Option<Result<Vec<Entry<I, T>>, String>>) -> Option<&[Entry<I, T>]> {
    list.as_ref()?.as_ref().ok().map(Vec::as_slice)
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
    /// A design's row, a font's button or a colour scheme of the dropdown (`part`, its id): draw
    /// every window with it. Closes the reasons of an entry that can't be used and the dropdown. A
    /// design remembers what was drawn before, for the hint "Also switched" ([`loaded`]).
    Choose(Part, String),
    /// An entry that can't be used (`part`, its id): show its reasons under its group, or hide them
    /// if they are shown already. Closes the dropdown. Asks nothing of the shell.
    Problems(Part, String),
    /// The colour scheme's button: open or close its dropdown.
    ToggleSchemes,
    /// A click next to the open dropdown, or Escape: close it.
    CloseSchemes,
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
            state.schemes_open = false;
            if part == Part::Design {
                let settings = &data.settings;
                let before = Before { design: id.clone(), font: settings.font.clone(), mail_view: settings.mail_view };
                state.before = Some(before);
                state.switched = None;
            }
            return Action::Choose(part, id);
        }
        Message::Problems(part, id) => {
            let shown = state.problems.as_ref().is_some_and(|(p, i)| *p == part && *i == id);
            state.problems = if shown { None } else { Some((part, id)) };
            state.schemes_open = false;
            return Action::None;
        }
        Message::ToggleSchemes => {
            state.schemes_open = !state.schemes_open;
            return Action::None;
        }
        Message::CloseSchemes => {
            state.schemes_open = false;
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
    *state.maps.draft_mut(which) = Some(Draft { text: typed.clone(), handed_in: true });
    if maps.merged(&maps_field(which.field(), value.clone())).is_err() {
        // The reason as a sentence `tr` translates; the core's own text is English, for agents.
        let why = address_problem(&typed, which == Address::Tiles).unwrap_or("The address cannot be used.");
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

/// The links under "Maps": the usage rules of the OpenStreetMap services (map tiles and Nominatim on
/// one page), and the licence of the map data (ODbL), which asks to name OpenStreetMap wherever its
/// data is shown. Two quiet links rather than one per service, so the group stays calm.
const MAP_LINKS: [(&str, &str); 2] = [
    ("Usage rules", "https://operations.osmfoundation.org/policies/"),
    ("© OpenStreetMap contributors", "https://www.openstreetmap.org/copyright"),
];

/// The group "Maps" (task TE.10, concept `docs/termin-editor.md` "Ort und Karte"), part of the
/// calendar: the switch "Maps and place search", the place search service's address with its
/// protocol, the map tiles' address, and "Open in Maps with". Each address row says under its name
/// what that service gets, and "Press Enter to save" in the same line while its address is typed
/// and not saved, so the row never changes its height. The group's footer says that it is off until
/// a person turns it on and that the services are others'; under it two quiet links, the usage
/// rules and the licence. While maps are off the address fields are drawn at 40 % – they can still
/// be set (a service of one's own before turning maps on), they only do nothing yet.
///
/// Every row sets only its own field (`settings.set_setting maps`). Turning maps on and changing an
/// address is a person's alone; the control centre is that person (app key), so it may. The
/// address fields save on Enter or when the person moves on ([`MapsDraft`]); a refused address
/// says why in a quiet line under the group.
fn maps<'a>(state: &'a State, maps: &'a Maps) -> Element<'a, Message> {
    let draft = &state.maps;
    let off = !maps.enabled;
    let address = |which: Address, placeholder: &'static str| {
        let typed = draft.draft(which).map_or(which.stored(maps), |typed| typed.text.as_str());
        field(placeholder, typed)
            .on_input(move |text| Message::AddressInput(which, text))
            .on_submit(Message::SaveAddress(which))
            .style(move |theme, status| address_style(theme, status, off))
    };
    let hint = |which: Address| Some(address_detail(draft, maps, which).to_string());
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
    let footer = tr("Off until you turn it on. Both services are run by others, under their own rules.");
    let links = MAP_LINKS.map(|(label, url)| {
        widgets::text_button(Some(Icon::ArrowUpRight), tr(label), style::button::ghost, Some(Message::OpenLink(url)))
    });
    let mut block = column![widgets::group(tr("Maps"), rows, Some(footer.to_string()))].spacing(layout::item_gap());
    if let Some((_, why)) = &draft.problem {
        block = block.push(notice(trf("Not saved: {reason}", &[("reason", &tr(why))])));
    }
    block.push(row(links).spacing(space::xs()).wrap().vertical_spacing(space::xs())).into()
}

/// The line under the name of an address row of "Maps": "Press Enter to save" while its address is
/// typed and not saved, else what that service gets. Always a line, so the row keeps its height
/// when the hint comes and goes.
fn address_detail(draft: &MapsDraft, maps: &Maps, which: Address) -> &'static str {
    match which {
        _ if draft.dirty(which, maps) => tr("Press Enter to save"),
        Address::Geocoder => tr("Gets what you search for and this computer's IP address"),
        Address::Tiles => tr("Gets the coordinates of the place"),
    }
}

/// An address field of "Maps": an input of the suite, drawn at 40 % (`style::text_input`'s disabled
/// look) while maps are `off` and it does not have the focus. It still takes typing: a service of
/// one's own can be set before maps are turned on.
fn address_style(
    theme: &Theme,
    status: iced::widget::text_input::Status,
    off: bool,
) -> iced::widget::text_input::Style {
    let focused = matches!(status, iced::widget::text_input::Status::Focused { .. });
    let shown = if off && !focused { iced::widget::text_input::Status::Disabled } else { status };
    style::text_input(theme, shown)
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

/// The page Appearance (variant B of issue #291, task L.37): the groups "Design", "Font and colors"
/// and "Display", each laid out for the width the page has.
pub fn appearance<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    responsive(move |room: Size| page(state, settings, room.width)).height(Length::Shrink).into()
}

/// The three groups of [`appearance`] for a page `width` wide.
fn page<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    column![designs(state, settings, width), looks(state, settings, width), display(settings, width)]
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

/// The group "Display": "Color mode" as a segment of System, Light and Dark, and the zoom, with the
/// sentences that explain both under the card. The control centre itself follows the colour mode at
/// once: it reloads the settings after every change. Where the segment's words do not fit beside its
/// label (a narrow page `width`, a long translation) it shows its symbols alone, the words as
/// tooltips.
fn display<'a>(settings: &'a Settings, width: f32) -> Element<'a, Message> {
    let chosen = settings.appearance;
    let active = modes().iter().position(|(_, _, mode)| *mode == chosen).unwrap_or_default();
    let parts = modes().map(|(which, label, mode)| (which, label, Message::Appearance(mode)));
    let segment = widgets::icon_segmented(parts, active, width >= mode_words_need());
    let mode = setting_row(None, tr("Color mode"), None, segment);
    let zoom = setting_row(None, tr("Zoom"), None, zoom_control(settings.zoom));
    let footer = format!(
        "{} {}",
        tr("Light or dark colors for all Torro apps. “System” follows your desktop."),
        tr(
            "Makes all Torro apps larger or smaller, like zooming in a browser: Ctrl + Plus and Ctrl + Minus, Ctrl + 0 resets."
        ),
    );
    widgets::group(tr("Display"), vec![mode, zoom], Some(footer))
}

/// How wide the colour mode's row must be for the segment's words: 16 at both sides and between
/// label and segment (a settings row), the label, the segment with its words.
fn mode_words_need() -> f32 {
    let labels = modes().map(|(_, label, _)| label);
    3.0 * space::l() + typo::text_width(tr("Color mode"), typo::body()) + widgets::icon_segmented_width(&labels, true)
}

/// The group "Design": the heading with "Reload" (for all three lists) and "Open designs folder", a
/// row per design ([`design_row`]), the reasons of the one opened, what the last design chosen here
/// switched too ("Also switched: …"), and the sentence that explains it. On a page narrower than
/// [`size::form_max()`] each row puts its density under its description.
fn designs<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    let reload = widgets::icon_button(Icon::RefreshCw, tr("Reload"), Some(Message::Reload));
    let heading = widgets::group_heading(tr("Design"), vec![reload, open_folder(Part::Design, true)]);
    let mut block = column![heading].spacing(layout::item_gap());
    match listed_or(&state.designs, tr("The designs can't be listed.")) {
        Ok(entries) => {
            let drawn = drawn(entries, &settings.design);
            let stacked = width < size::form_max();
            let rows = entries.iter().map(|entry| design_row(entry, entry.info.id == drawn, stacked)).collect();
            block = block.push(card(rows));
            block =
                block.push(opened(entries, Part::Design, state.problems.as_ref()).map(|entry| problems(&entry.info)));
        }
        Err(placeholder) => block = block.push(card(vec![placeholder])),
    }
    block = block.push(state.switched.as_ref().map(|switched| switched_hint(switched, &state.fonts)));
    let footer = tr(
        "Spacing, sizes and corner radii of all Torro apps. More designs go into the designs folder; one that breaks a rule can't be used.",
    );
    block.push(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2)).into()
}

/// `rows` in the framed card of a settings group with a line between them, as `widgets::group` draws
/// them, for a group whose heading carries actions ([`widgets::group_heading`]).
fn card<'a>(rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut card = column![];
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.push(rule::horizontal(size::HAIRLINE).style(style::rule));
        }
        card = card.push(row);
    }
    container(card).width(Fill).style(style::container::group).into()
}

/// The entries of `list` once the core listed them; until then the row that stands in their place:
/// quiet "Loading …" (the core answers in a moment at most), or `failed` with the core's reason and
/// an alert mark.
fn listed_or<'a, I, T>(
    list: &'a Option<Result<Vec<Entry<I, T>>, String>>,
    failed: &'a str,
) -> Result<&'a [Entry<I, T>], Element<'a, Message>> {
    match list {
        None => Err(row_frame(typo::text(typo::body(), tr("Loading …")).style(torro_gui::theme::text_3), false)),
        Some(Err(problem)) => {
            let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
            Err(setting_row(Some(alert.into()), failed, Some(problem.clone()), None::<Element<'a, Message>>))
        }
        Some(Ok(entries)) => Ok(entries),
    }
}

/// A design's row: its [`thumbnail`] ("version · author" as tooltip), name and description, its
/// [`density`] and the check mark at the one `drawn`; the whole row is the button. One that can't be
/// used is greyed out, with an alert mark for the thumbnail and no density, and its click shows its
/// reasons. `stacked` (a narrow page) puts the density under the description, so the text keeps its
/// room.
fn design_row<'a>(entry: &'a DesignEntry, drawn: bool, stacked: bool) -> Element<'a, Message> {
    let info = &entry.info;
    let picture = thumbnail(entry);
    let picture = match byline([Some(info.version.as_str()), info.author.as_deref()]) {
        Some(tip) => widgets::with_tooltip(picture, tip),
        None => picture,
    };
    let tone: fn(&Theme) -> text::Style = if info.valid { torro_gui::theme::text } else { torro_gui::theme::text_3 };
    let mut words = column![typo::text(typo::body_strong(), info.name.as_str()).style(tone)].spacing(space::xs());
    if let Some(description) = info.description.as_deref().filter(|description| !description.is_empty()) {
        words = words.push(typo::text(typo::meta(), description).style(torro_gui::theme::text_2));
    }
    let density = info.valid.then(|| density(info.density, stacked));
    let content = match stacked {
        true => row![picture, words.push(density).width(Fill), check_mark(drawn)],
        false => row![picture, words.width(Fill)].push(density).push(check_mark(drawn)),
    };
    let press = match info.valid {
        true => Message::Choose(Part::Design, info.id.clone()),
        false => Message::Problems(Part::Design, info.id.clone()),
    };
    let content =
        container(content.spacing(space::l()).align_y(Alignment::Center)).width(Fill).padding([space::m(), space::l()]);
    button(content).padding(Padding::ZERO).width(Fill).style(style::button::list_row(false)).on_press(press).into()
}

/// The check mark at the end of the row or entry that is drawn, in `text`; the same room left empty
/// at the others, so their texts end in one column.
fn check_mark<'a>(on: bool) -> Element<'a, Message> {
    match on {
        true => {
            icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
        }
        false => blank().width(icon::SIZE).into(),
    }
}

/// The dots of a design's density.
const DENSITY_DOTS: usize = 3;

/// How many of the [`DENSITY_DOTS`] a density fills: one more with each step from airy (none) to
/// very dense (all).
fn density_dots(level: DensityLevel) -> usize {
    match level {
        DensityLevel::Airy => 0,
        DensityLevel::Balanced => 1,
        DensityLevel::Dense => 2,
        DensityLevel::VeryDense => 3,
    }
}

/// The word of a density.
fn density_label(level: DensityLevel) -> &'static str {
    match level {
        DensityLevel::Airy => tr("Airy"),
        DensityLevel::Balanced => tr("Balanced"),
        DensityLevel::Dense => tr("Dense"),
        DensityLevel::VeryDense => tr("Very dense"),
    }
}

/// A design's density as the core reports it (`DesignInfo::density`): the dots (filled in `text_2`,
/// the rest in `line_strong`, so the word is not needed to compare) and the word in `META` and
/// `text_3`; at the right end of the row one over the other, `stacked` side by side.
fn density<'a>(level: DensityLevel, stacked: bool) -> Element<'a, Message> {
    let filled = density_dots(level);
    let dots = row((0..DENSITY_DOTS).map(|index| density_dot(index < filled))).spacing(DOT_GAP);
    let word = typo::text(typo::meta(), density_label(level)).style(torro_gui::theme::text_3).wrapping(Wrapping::None);
    match stacked {
        true => row![dots, word].spacing(space::s()).align_y(Alignment::Center).into(),
        false => column![dots, word].spacing(space::xs()).align_x(Alignment::End).into(),
    }
}

/// The edge of a density dot and of the dot of a suggested font: the dot of a chip (6, as in
/// `torro_gui::widgets`), the same in every design – scaled with the spacing it would shrink to a
/// speck in Console.
const DOT_EDGE: f32 = 6.0;

/// Between the dots of a density: half a dot.
const DOT_GAP: f32 = DOT_EDGE / 2.0;

/// One dot of a [`density`], round in every design.
fn density_dot<'a>(filled: bool) -> Element<'a, Message> {
    round_dot(move |p: &Palette| if filled { p.text_2 } else { p.line_strong })
}

/// A round dot of [`DOT_EDGE`] in the colour `pick` takes from the window's palette.
fn round_dot<'a>(pick: impl Fn(&Palette) -> Color + 'a) -> Element<'a, Message> {
    let edge = DOT_EDGE;
    container(blank().width(edge).height(edge))
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(pick(&palette(t)))),
            border: Border { radius: (edge / 2.0).into(), ..Border::default() },
            ..container::Style::default()
        })
        .into()
}

/// The size of a design's thumbnail: [`PREVIEW_CONTROLS`] buttons wide (144 in the standard, less in
/// the denser designs, so the list follows the design it is drawn in) and in the proportions of
/// [`MAIL_WINDOW`].
fn thumbnail_size() -> Size {
    let width = PREVIEW_CONTROLS * size::control_sm();
    Size::new(width, width * MAIL_WINDOW.height / MAIL_WINDOW.width)
}

/// A design's thumbnail in a `line` frame on `panel`: its `preview.png` (cut to the frame), else a
/// small mail window drawn from its values ([`MailPreview`]); for one that can't be used an alert
/// mark in `text_3`, so it is told by a symbol, not by colour alone.
fn thumbnail<'a>(entry: &'a DesignEntry) -> Element<'a, Message> {
    let area = thumbnail_size();
    let content: Element<'a, Message> = match (&entry.preview, &entry.part) {
        (Some(picture), _) => svg(picture.clone()).content_fit(ContentFit::Cover).width(Fill).height(Fill).into(),
        (None, Some(design)) if entry.info.valid => {
            canvas(MailPreview { design: Arc::clone(design) }).width(Fill).height(Fill).into()
        }
        _ => container(
            icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) }),
        )
        .center(Fill)
        .into(),
    };
    container(content)
        .width(area.width)
        .height(area.height)
        .clip(true)
        .style(|t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).panel)),
            border: Border { color: palette(t).line, width: size::HAIRLINE, radius: radius::xs().into() },
            ..container::Style::default()
        })
        .into()
}

/// Under the group "Design": what the core switched with the design chosen here last, "Also
/// switched: font JetBrains Mono, plain-text mail", quiet, with an info mark.
fn switched_hint<'a>(switched: &Switched, fonts: &'a Option<Result<Vec<FontEntry>, String>>) -> Element<'a, Message> {
    let list = switched_list(switched, fonts);
    let mark = icon::icon(Icon::Info, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let words = typo::text(typo::meta(), trf("Also switched: {list}", &[("list", &list)]));
    row![mark, words.style(torro_gui::theme::text_2)].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The parts of [`switched_hint`], joined with ", ": the font by its family as listed (its id when it
/// is not listed), and the mail view.
fn switched_list(switched: &Switched, fonts: &Option<Result<Vec<FontEntry>, String>>) -> String {
    let mut parts = Vec::new();
    if let Some(id) = &switched.font {
        let listed = ok(fonts).and_then(|fonts| fonts.iter().find(|entry| entry.info.id == *id));
        let name = listed.map_or(id.as_str(), |entry| entry.info.name());
        parts.push(trf("font {font}", &[("font", &name)]));
    }
    if let Some(view) = switched.mail_view {
        let view = match view {
            MailFormat::Text => tr("plain-text mail"),
            MailFormat::Html => tr("HTML mail"),
        };
        parts.push(view.to_string());
    }
    parts.join(", ")
}

/// The group "Font and colors": the heading with "Open fonts folder" and "Open color schemes folder"
/// (their symbols alone where the words do not fit beside the title on a page `width` wide), the
/// font's row ([`font_row`]) and the colour scheme's ([`scheme_row`]), the reasons of the one opened,
/// and the sentences that explain both.
fn looks<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    let words = width >= looks_heading_need();
    let folders = vec![open_folder(Part::Font, words), open_folder(Part::ColorScheme, words)];
    let heading = widgets::group_heading(tr("Font and colors"), folders);
    let font = match listed_or(&state.fonts, tr("The fonts can't be listed.")) {
        Ok(entries) => font_row(state, settings, entries),
        Err(placeholder) => placeholder,
    };
    let scheme = match listed_or(&state.schemes, tr("The color schemes can't be listed.")) {
        Ok(entries) => scheme_row(state, settings, entries),
        Err(placeholder) => placeholder,
    };
    let open = state.problems.as_ref();
    let reasons = ok(&state.fonts)
        .and_then(|fonts| opened(fonts, Part::Font, open))
        .map(|entry| problems(&entry.info))
        .or_else(|| {
            ok(&state.schemes)
                .and_then(|schemes| opened(schemes, Part::ColorScheme, open))
                .map(|entry| problems(&entry.info))
        });
    let footer = format!(
        "{} {}",
        tr(
            "The typeface of all Torro apps, sized so that text reads alike in every font. More fonts go into the fonts folder."
        ),
        tr("The colors of all Torro apps, light and dark. More color schemes go into the color schemes folder."),
    );
    column![heading, card(vec![font, scheme])]
        .push(reasons)
        .push(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2))
        .spacing(layout::item_gap())
        .into()
}

/// How wide the heading "Font and colors" must be for the words of its two folder buttons.
fn looks_heading_need() -> f32 {
    typo::text_width(tr("Font and colors"), typo::caption())
        + widgets::text_button_width(true, tr("Open fonts folder"))
        + widgets::text_button_width(true, tr("Open color schemes folder"))
        + space::s()
        + space::xs()
}

/// The font's row: its name, the sentence about the font the drawn design suggests ([`FontHint`]),
/// and a button per font under both, as many in a line as fit, the rest wrapping.
fn font_row<'a>(state: &'a State, settings: &'a Settings, entries: &'a [FontEntry]) -> Element<'a, Message> {
    let drawn = drawn(entries, &settings.font);
    let hint = font_hint(state, settings);
    let open = state.problems.as_ref().filter(|(part, _)| *part == Part::Font).map(|(_, id)| id.as_str());
    let buttons = entries.iter().map(|entry| {
        let id = entry.info.id.as_str();
        let suggested = hint.as_ref().filter(|hint| hint.marks(id)).map(FontHint::design);
        font_button(entry, id == drawn, open == Some(id), suggested)
    });
    let buttons = row(buttons).spacing(space::xs()).wrap().vertical_spacing(space::xs());
    widgets::segment_row(tr("Font"), hint.as_ref().map(FontHint::sentence), buttons, None)
}

/// A font's button in the look of a chip to choose (`style::button::choice_chip`): its family written
/// in its own text font at the size it draws body text, the check mark in front of the one `drawn`,
/// a dot after the one the drawn design suggests (`suggested`: the design's name, said in the
/// tooltip with "pack · version · author · licence"). One that can't be used is greyed out with an
/// alert mark; its click shows its reasons (`open`: they are shown).
fn font_button<'a>(entry: &'a FontEntry, drawn: bool, open: bool, suggested: Option<&'a str>) -> Element<'a, Message> {
    let info = &entry.info;
    let tip = byline([
        Some(info.name.as_str()),
        Some(info.version.as_str()),
        info.author.as_deref(),
        info.license.as_deref(),
    ]);
    let tip = suggested_tip(suggested, tip);
    let mut label = typo::text(typo::body(), info.name()).wrapping(Wrapping::None);
    if let Some(font) = entry.part.as_deref() {
        let level = font.apply(torro_gui::design::active().typo.body);
        label = label
            .font(torro_gui::design::font_of(font, FontRole::Text, Weight::Regular))
            .size(Pixels(level.size))
            .line_height(LineHeight::Absolute(Pixels(level.line)));
    }
    let mut content = row![].spacing(space::xs()).align_y(Alignment::Center);
    let button = if info.valid {
        if drawn {
            content = content.push(small_icon(Icon::Check, |p| p.text));
        }
        content = content.push(label);
        if suggested.is_some() {
            content = content.push(round_dot(|p: &Palette| p.text_3));
        }
        chip(content).style(style::button::choice_chip(drawn)).on_press(Message::Choose(Part::Font, info.id.clone()))
    } else {
        content = content.push(small_icon(Icon::CircleAlert, |p| p.text_3)).push(label);
        chip(content)
            .style(style::button::preview_tile(open, false))
            .on_press(Message::Problems(Part::Font, info.id.clone()))
    };
    match tip {
        Some(tip) => widgets::with_tooltip(button, tip),
        None => button.into(),
    }
}

/// `content` as a chip-sized button: [`size::control_sm()`] high, the padding of a chip.
fn chip<'a>(content: iced::widget::Row<'a, Message>) -> iced::widget::Button<'a, Message> {
    button(container(content).center_y(Fill)).height(size::control_sm()).padding(style::CHIP_PADDING)
}

/// `which` at the size of icons in buttons, in the colour `pick` takes from the window's palette.
fn small_icon<'a>(which: Icon, pick: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE_SM).style(move |t: &Theme, _| svg::Style { color: Some(pick(&palette(t))) }).into()
}

/// The tooltip of a font the design `suggested` suggests: "Suggested for {design}" over the rest of
/// it. Unchanged when it is not suggested.
fn suggested_tip(suggested: Option<&str>, tip: Option<String>) -> Option<String> {
    let Some(design) = suggested else { return tip };
    let why = trf("Suggested for {design}", &[("design", &design)]);
    Some(match tip {
        Some(tip) => format!("{why}\n{tip}"),
        None => why,
    })
}

/// What the font row says about the font the drawn design suggests (`DesignInfo::font`, task L.37).
/// The core sets that font with the design only when it can be used (task L.33).
#[derive(Debug, PartialEq, Eq)]
enum FontHint<'a> {
    /// It is listed, can be used and is drawn; `font` is its family.
    Drawn { design: &'a str, font: &'a str, id: &'a str },
    /// It is listed and can be used, another font is drawn; `font` is its family.
    Other { design: &'a str, font: &'a str, id: &'a str },
    /// It is not listed as usable – not in the fonts folder, or broken –, so the font stayed; `font`
    /// is its id, all there is to name it by.
    Missing { design: &'a str, font: &'a str },
}

impl<'a> FontHint<'a> {
    /// The name of the design that suggests it.
    fn design(&self) -> &'a str {
        match self {
            FontHint::Drawn { design, .. } | FontHint::Other { design, .. } | FontHint::Missing { design, .. } => {
                design
            }
        }
    }

    /// Whether the font `id` is the one suggested, so its button has the dot.
    fn marks(&self, id: &str) -> bool {
        match self {
            FontHint::Drawn { id: suggested, .. } | FontHint::Other { id: suggested, .. } => *suggested == id,
            FontHint::Missing { .. } => false,
        }
    }

    /// The sentence under "Font".
    fn sentence(&self) -> String {
        match self {
            FontHint::Drawn { design, font, .. } => {
                trf("{font} is suggested for {design}.", &[("font", font), ("design", design)])
            }
            FontHint::Other { design, font, .. } => {
                trf("{design} suggests {font}.", &[("design", design), ("font", font)])
            }
            FontHint::Missing { design, font } => {
                trf("{design} suggests {font}, which isn't installed.", &[("design", design), ("font", font)])
            }
        }
    }
}

/// The [`FontHint`] of the design drawn; `None` while designs or fonts are not listed, or when the
/// design suggests no font.
fn font_hint<'a>(state: &'a State, settings: &'a Settings) -> Option<FontHint<'a>> {
    let designs = ok(&state.designs)?;
    let id = drawn(designs, &settings.design);
    let info = &designs.iter().find(|entry| entry.info.id == id)?.info;
    let suggested = info.font.as_deref()?;
    let fonts = ok(&state.fonts)?;
    let design = info.name.as_str();
    Some(match fonts.iter().find(|entry| entry.info.id == suggested && entry.info.valid) {
        None => FontHint::Missing { design, font: suggested },
        Some(entry) if drawn(fonts, &settings.font) == suggested => {
            FontHint::Drawn { design, font: entry.info.name(), id: suggested }
        }
        Some(entry) => FontHint::Other { design, font: entry.info.name(), id: suggested },
    })
}

/// The colour scheme's row: its name and the dropdown's button with the scheme drawn
/// ([`scheme_button`]).
fn scheme_row<'a>(state: &'a State, settings: &'a Settings, entries: &'a [SchemeEntry]) -> Element<'a, Message> {
    let drawn = drawn(entries, &settings.color_scheme);
    let current = entries.iter().find(|entry| entry.info.id == drawn);
    setting_row(None, tr("Color scheme"), None, scheme_button(&state.anchor, current))
}

/// The button of the colour scheme's dropdown, in the look of a secondary button
/// [`size::sidebar()`] wide: the stripes and the name of the scheme drawn, a chevron; it opens the
/// dropdown under it ([`overlay`]).
fn scheme_button<'a>(anchor: &Anchor, current: Option<&'a SchemeEntry>) -> Element<'a, Message> {
    let name = current.map_or("", |entry| entry.info.name.as_str());
    let content = row![
        swatch(current.and_then(|entry| entry.part.as_ref())),
        container(typo::text(typo::body(), name).wrapping(Wrapping::None)).width(Fill).clip(true),
        small_icon(Icon::ChevronDown, |p| p.text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let open = button(container(content).center_y(Fill))
        .width(size::sidebar())
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::secondary)
        .on_press(Message::ToggleSchemes);
    widgets::anchored(anchor, open)
}

/// The page's layer over the window: the colour scheme's dropdown while it is open, under its
/// button, its right edge on the button's ([`scheme_menu`]). A click next to it closes it.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if !state.schemes_open {
        return None;
    }
    let entries = ok(&state.schemes)?;
    let menu = scheme_menu(entries, drawn(entries, &data.settings.color_scheme));
    Some(widgets::floating(&state.anchor, Placement::Below(Align::End), menu, Message::CloseSchemes))
}

/// The sections of the colour scheme's dropdown, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemeGroup {
    /// The standard colour scheme the suite ships.
    Torro,
    /// Every other one with a light and a dark mode.
    LightAndDark,
    /// The ones that are only dark (`ColorSchemeInfo::dark_only`).
    DarkOnly,
}

impl SchemeGroup {
    const ALL: [SchemeGroup; 3] = [SchemeGroup::Torro, SchemeGroup::LightAndDark, SchemeGroup::DarkOnly];

    /// The section of the colour scheme `info`.
    fn of(info: &ColorSchemeInfo) -> SchemeGroup {
        match (info.standard, info.dark_only) {
            (true, _) => SchemeGroup::Torro,
            (false, true) => SchemeGroup::DarkOnly,
            (false, false) => SchemeGroup::LightAndDark,
        }
    }

    /// The section's title: "Torro" is a name and stays as it is.
    fn title(self) -> &'static str {
        match self {
            SchemeGroup::Torro => "Torro",
            SchemeGroup::LightAndDark => tr("Light and dark"),
            SchemeGroup::DarkOnly => tr("Dark only"),
        }
    }
}

/// The colour schemes by [`SchemeGroup`], in the order of the sections and of the list within each;
/// a section without one is left out.
fn scheme_groups(entries: &[SchemeEntry]) -> Vec<(SchemeGroup, Vec<&SchemeEntry>)> {
    SchemeGroup::ALL
        .into_iter()
        .map(|group| (group, entries.iter().filter(|entry| SchemeGroup::of(&entry.info) == group).collect::<Vec<_>>()))
        .filter(|(_, members)| !members.is_empty())
        .collect()
}

/// The dropdown's card in the look of a menu ([`size::props()`] wide, scrolling past
/// [`size::panel()`]): each [`SchemeGroup`] with its title, a line between them, an entry per colour
/// scheme ([`scheme_item`]), the one `drawn` with the check mark.
fn scheme_menu<'a>(entries: &'a [SchemeEntry], drawn: &str) -> Element<'a, Message> {
    let mut list = column![];
    for (index, (group, members)) in scheme_groups(entries).into_iter().enumerate() {
        if index > 0 {
            list =
                list.push(container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]));
        }
        let title = typo::text(typo::caption(), group.title()).style(torro_gui::theme::text_3);
        list = list.push(container(title).padding(Padding::new(space::s()).bottom(space::xs())));
        list = list.extend(members.into_iter().map(|entry| scheme_item(entry, entry.info.id == drawn)));
    }
    let body = scrollable(list)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Length::Shrink)
        .width(Fill);
    container(container(body).max_height(size::panel()))
        .width(size::props())
        .padding(style::MENU_INSET)
        .style(style::container::menu)
        .into()
}

/// A colour scheme of the dropdown: its stripes, its name and the check mark at the one `drawn`, the
/// whole entry a menu item. One that can't be used has an alert mark for the stripes and its name in
/// `text_3`; its click shows its reasons under the group.
fn scheme_item<'a>(entry: &'a SchemeEntry, drawn: bool) -> Element<'a, Message> {
    let info = &entry.info;
    let name = typo::text(typo::body(), info.name.as_str()).wrapping(Wrapping::None);
    let (lead, name, press) = match (info.valid, entry.part.as_ref()) {
        (true, part) => (swatch(part), name, Message::Choose(Part::ColorScheme, info.id.clone())),
        (false, _) => (
            small_icon(Icon::CircleAlert, |p| p.text_3),
            name.style(torro_gui::theme::text_3),
            Message::Problems(Part::ColorScheme, info.id.clone()),
        ),
    };
    let content = row![lead, container(name).width(Fill).clip(true), check_mark(drawn)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(press)
        .into()
}

/// The four colours of a colour scheme's stripes, left to right: ground, surface, text, accent.
fn stripes() -> [fn(&Palette) -> Color; 4] {
    [|p| p.bg, |p| p.panel, |p| p.text, |p| p.accent]
}

/// A colour scheme's four [`stripes`] side by side in the window's light or dark mode (a scheme that
/// is only dark, dark in both), [`size::tile()`] wide and [`icon::SIZE`] high, in a `line_strong`
/// frame so a stripe of the window's own colour still shows its edge. Empty for a scheme that could
/// not be read.
fn swatch<'a>(scheme: Option<&Arc<ColorScheme>>) -> Element<'a, Message> {
    let parts = stripes().into_iter().map(|pick| -> Element<'a, Message> {
        let scheme = scheme.cloned();
        container(blank().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(move |t: &Theme| container::Style {
                background: scheme.as_deref().map(|scheme| Background::Color(pick(&colors(scheme, t)))),
                ..container::Style::default()
            })
            .into()
    });
    container(row(parts))
        .width(size::tile())
        .height(icon::SIZE)
        .padding(size::HAIRLINE)
        .clip(true)
        .style(|t: &Theme| container::Style {
            border: Border { color: palette(t).line_strong, width: size::HAIRLINE, radius: radius::xs().into() },
            ..container::Style::default()
        })
        .into()
}

/// The colours of `scheme` in the mode of `theme` (the window's light or dark).
fn colors(scheme: &ColorScheme, theme: &Theme) -> Palette {
    let mode = if theme.extended_palette().is_dark { Mode::Dark } else { Mode::Light };
    Palette::of(scheme, mode)
}

/// The id of the entry the windows draw with: `chosen` (`Settings::design`, `font` or
/// `color_scheme`) if it is listed and can be used, else the standard, which the suite falls back to
/// (concept: "Wählen und wechseln"). An empty `chosen` (a core from before task L.22) is the
/// standard too.
fn drawn<'a, I: Listed, T>(entries: &'a [Entry<I, T>], chosen: &'a str) -> &'a str {
    if entries.iter().any(|entry| entry.info.id() == chosen && entry.info.valid()) { chosen } else { STANDARD_ID }
}

/// The entry whose reasons are shown: the one `open` names, while its list is `part`'s and it is
/// listed and can't be used. After "Reload" an entry that was fixed is drawn again, and nothing is
/// left open for it.
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

/// Under the group, for the entry that can't be used that was clicked: its name, "Can't be used:"
/// and every reason, at full strength and wrapping – they say what to fix, one per line, so an
/// entry with many can be worked through like a list.
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

// ----- The small mail window of a design (task L.37) -------------------------------------------

/// The window a [`MailPreview`] shows in small, in logical pixels: the size of the boards of issue
/// #291, a mail window on a desktop.
const MAIL_WINDOW: Size = Size::new(1280.0, 800.0);

/// How many buttons ([`size::control_sm()`]) wide a design's thumbnail is.
const PREVIEW_CONTROLS: f32 = 4.0;

/// The share of a text size a line of letters covers seen from afar (about the x-height): how high
/// the bars are that stand for text in a [`MailPreview`].
const INK: f32 = 0.5;

/// The widths of lines of text in a [`MailPreview`] as shares of their room, repeated, so a
/// paragraph reads as text and not as stripes.
const LINES: [f32; 4] = [1.0, 0.92, 0.97, 0.58];

/// How many rows the sidebar of a [`MailPreview`] has, and how many icons its action rail.
const NAV_ROWS: usize = 7;
const RAIL_ICONS: usize = 5;

/// The mail window of the suite in small, drawn from the values of `design` – sidebar, header, the
/// list of mails as the design draws lists (`[list]`: rows with avatar and preview or one line,
/// hairlines, the chosen row with the accent bar or in accent), the reader in its reading column
/// and the action rail at the right – in the colours of the window's palette, so only what the
/// design changes differs from row to row. Text is drawn as bars at the height of its level.
#[derive(Debug, Clone)]
struct MailPreview {
    design: Arc<Design>,
}

impl<M> canvas::Program<M> for MailPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        paint(&mut frame, &self.design, &palette(theme), bounds.size());
        vec![frame.into_geometry()]
    }
}

/// The regions of a [`MailPreview`] `width` wide, in its pixels: the design's sizes scaled from
/// [`MAIL_WINDOW`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Regions {
    /// Pixels of the preview per pixel of the window.
    scale: f32,
    /// The width of the sidebar (`size.sidebar`).
    sidebar: f32,
    /// The height of the header (`size.header`).
    header: f32,
    /// The width of the list (`size.list`).
    list: f32,
    /// The width of the collapsed action rail (`size.rail`).
    rail: f32,
    /// The height of a mail row (`size.row_message`).
    row: f32,
}

impl Regions {
    fn of(design: &Design, width: f32) -> Regions {
        let scale = width / MAIL_WINDOW.width;
        let size = &design.size;
        Regions {
            scale,
            sidebar: size.sidebar * scale,
            header: size.header * scale,
            list: size.list * scale,
            rail: size.rail * scale,
            row: size.row_message * scale,
        }
    }
}

/// Paints the mail window of `design` onto `frame` of `area` in the colours `p`.
fn paint(frame: &mut canvas::Frame, design: &Design, p: &Palette, area: Size) {
    let r = Regions::of(design, area.width);
    frame.fill_rectangle(Point::ORIGIN, area, p.panel);
    sidebar(frame, design, p, &r, area.height);
    let content = r.sidebar;
    fill(frame, content, 0.0, size::HAIRLINE, area.height, p.line);
    // The header: the title, a line under it across the content.
    let title_x = content + design.layout.row_padding.horizontal * r.scale;
    ink(frame, title_x, r.header / 2.0, r.list * 0.3, &design.typo.body_strong, r.scale, p.text);
    fill(frame, content, r.header - size::HAIRLINE, area.width - content, size::HAIRLINE, p.line);
    let rail_x = area.width - r.rail;
    mail_list(frame, design, p, &r, content, area.height);
    reader(frame, design, p, &r, content + r.list, rail_x, area.height);
    rail(frame, design, p, &r, rail_x, area.height);
}

/// A rectangle at least a hairline high.
fn fill(frame: &mut canvas::Frame, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    frame.fill_rectangle(Point::new(x, y), Size::new(width.max(0.0), height.max(size::HAIRLINE)), colour);
}

/// A bar that stands for a line of text of `level` (scaled by `scale`), its middle at `middle`.
fn ink(frame: &mut canvas::Frame, x: f32, middle: f32, width: f32, level: &Level, scale: f32, colour: Color) {
    let height = (level.size * INK * scale).max(size::HAIRLINE);
    fill(frame, x, middle - height / 2.0, width, height, colour);
}

/// The sidebar on `bg`: the app switcher (logo in accent, the app's name) in the header's height,
/// then its rows, the first one chosen.
fn sidebar(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, height: f32) {
    let k = r.scale;
    fill(frame, 0.0, 0.0, r.sidebar, height, p.bg);
    let (pad, mark, gap) = (design.space.m * k, icon::SIZE * k, design.space.s * k);
    let middle = r.header / 2.0;
    fill(frame, pad, middle - mark / 2.0, mark, mark, p.accent);
    ink(frame, pad + mark + gap, middle, r.sidebar * 0.3, &design.typo.body_strong, k, p.text);
    let step = (design.size.row_sidebar * k).max(size::HAIRLINE);
    for index in 0..NAV_ROWS {
        let top = r.header + index as f32 * step;
        if top + step > height {
            break;
        }
        let middle = top + step / 2.0;
        let tone = if index == 0 { p.text } else { p.text_2 };
        if index == 0 {
            fill(frame, pad / 2.0, top, r.sidebar - pad, step, p.selected);
        }
        fill(frame, pad, middle - mark / 2.0, mark, mark, p.text_3);
        let x = pad + mark + gap;
        let width = (r.sidebar - x - pad) * LINES[index % LINES.len()] * 0.6;
        ink(frame, x, middle, width, &design.typo.body, k, tone);
    }
}

/// The list of mails from `x`: a row of `size.row_message` after the other down to `height`, the
/// second one chosen; a line at its right edge.
fn mail_list(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, x: f32, height: f32) {
    let (k, look) = (r.scale, design.list);
    let pad = design.layout.row_padding;
    let (pad_x, pad_y) = (pad.horizontal * k, pad.vertical * k);
    let step = r.row.max(size::HAIRLINE);
    let mut top = r.header;
    let mut index = 0;
    while top < height {
        let chosen = index == 1;
        let filled = chosen && look.selection == ListSelection::Fill;
        if chosen {
            fill(frame, x, top, r.list, step, if filled { p.accent } else { p.selected });
            if !filled {
                fill(frame, x, top, 2.0 * size::HAIRLINE, step, p.accent);
            }
        }
        // Text, secondary text, quiet text, avatar.
        let tones = match filled {
            true => [p.on_accent; 4],
            false => [p.text, p.text_2, p.text_3, p.line_strong],
        };
        let cell = Rectangle { x: x + pad_x, y: top + pad_y, width: r.list - 2.0 * pad_x, height: step - 2.0 * pad_y };
        match look.style {
            ListStyle::Line => mail_line(frame, design, k, cell, tones),
            ListStyle::Rows | ListStyle::Cards => mail_rows(frame, design, k, cell, tones),
        }
        if look.separators {
            fill(frame, x, top + step - size::HAIRLINE, r.list, size::HAIRLINE, p.line);
        }
        top += step;
        index += 1;
    }
    fill(frame, x + r.list, r.header, size::HAIRLINE, height - r.header, p.line);
}

/// A mail as one line in fixed columns (`[list] style = "line"`): date, sender, subject.
fn mail_line(frame: &mut canvas::Frame, design: &Design, k: f32, cell: Rectangle, tones: [Color; 4]) {
    let (body, gap, middle) = (&design.typo.body, design.space.m * k, cell.y + cell.height / 2.0);
    let (date, sender) = (cell.width * 0.1, cell.width * 0.22);
    ink(frame, cell.x, middle, date * 0.8, body, k, tones[2]);
    ink(frame, cell.x + date + gap, middle, sender * 0.8, body, k, tones[0]);
    let subject = cell.x + date + sender + 2.0 * gap;
    ink(frame, subject, middle, (cell.x + cell.width - subject) * 0.7, body, k, tones[1]);
}

/// A mail as a row with its lines (`[list] style = "rows"`): the avatar (`[list] avatar`, none at 0),
/// sender and time, subject and `[list] preview_lines` of preview, each line of `body` height.
fn mail_rows(frame: &mut canvas::Frame, design: &Design, k: f32, cell: Rectangle, tones: [Color; 4]) {
    let look = design.list;
    let mut left = cell.x;
    if look.avatar > 0.0 {
        let edge = look.avatar * k;
        frame.fill(&canvas::Path::circle(Point::new(left + edge / 2.0, cell.y + edge / 2.0), edge / 2.0), tones[3]);
        left += edge + design.space.m * k;
    }
    let width = (cell.x + cell.width - left).max(0.0);
    let line = design.typo.body.line * k;
    let typo = &design.typo;
    let lines = [
        (0.4, &typo.body_strong, tones[0]),
        (0.75, &typo.body, tones[0]),
        (0.9, &typo.body, tones[1]),
        (0.7, &typo.body, tones[1]),
    ];
    let count = 2 + usize::from(look.preview_lines);
    for (index, (share, level, colour)) in lines.into_iter().take(count).enumerate() {
        let middle = cell.y + line * (index as f32 + 0.5);
        if middle > cell.y + cell.height {
            break;
        }
        ink(frame, left, middle, width * share, level, k, colour);
    }
    let time = width * 0.12;
    ink(frame, cell.x + cell.width - time, cell.y + line / 2.0, time, &typo.meta, k, tones[2]);
}

/// The reader between `left` and `right`: the subject and paragraphs in the reading column –
/// `size.reading_max` wide at most, `layout.reading_padding` inside.
fn reader(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, left: f32, right: f32, height: f32) {
    let k = r.scale;
    let pad = design.layout.reading_padding;
    let x = left + pad.horizontal * k;
    let column = (design.size.reading_max * k).min(right - left - 2.0 * pad.horizontal * k).max(0.0);
    let (title, read) = (&design.typo.title, &design.typo.read);
    let mut y = r.header + pad.vertical * k;
    ink(frame, x, y + title.line * k / 2.0, column * 0.7, title, k, p.text);
    let line = (read.line * k).max(size::HAIRLINE);
    y += title.line * k + line;
    let mut index = 0;
    while y < height {
        ink(frame, x, y + line / 2.0, column * LINES[index % LINES.len()], read, k, p.text_3);
        y += line;
        index += 1;
        // A blank line after each paragraph.
        if index % LINES.len() == 0 {
            y += line;
        }
    }
}

/// The collapsed action rail from `x` to the right edge, a line at its left: an icon per group, the
/// first (Reply) in accent.
fn rail(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, x: f32, height: f32) {
    let k = r.scale;
    fill(frame, x, r.header, size::HAIRLINE, height - r.header, p.line);
    let (mark, step) = (icon::SIZE * k, design.size.control_lg * k);
    for index in 0..RAIL_ICONS {
        let middle = r.header + design.space.s * k + step * (index as f32 + 0.5);
        let colour = if index == 0 { p.accent } else { p.text_3 };
        fill(frame, x + (r.rail - mark) / 2.0, middle - mark / 2.0, mark, mark, colour);
    }
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
/// heading that names the folder (from home, as `~/…`) in its tooltip; without `words` (no room for
/// them beside the title) its symbol alone, the words as tooltip.
fn open_folder<'a>(part: Part, words: bool) -> Element<'a, Message> {
    let label = match part {
        Part::Design => tr("Open designs folder"),
        Part::Font => tr("Open fonts folder"),
        Part::ColorScheme => tr("Open color schemes folder"),
    };
    let press = Some(Message::OpenFolder(part));
    if !words {
        return widgets::icon_button(Icon::Folder, label, press);
    }
    let open = widgets::text_button(Some(Icon::Folder), label, style::button::ghost, press);
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
        for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
            let settings = Settings { appearance, ..Settings::default() };
            drop(display(&settings, size::content_max()));
            drop(display(&settings, size::props()));
        }
    }

    /// The demo's lists, as the page keeps them.
    fn demo() -> State {
        let mut state = State::default();
        listed(&mut state, super::super::launch::demo_looks());
        state
    }

    /// Tasks L.18, L.23 and L.37: a click on an entry that can't be used shows its reasons, a second
    /// click hides them, a click on another one swaps them – also to another part's, the same id in
    /// another list being another entry; choosing anything closes them. None of it asks the shell.
    #[test]
    fn an_unusable_entry_opens_and_closes_its_reasons() {
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

    /// Task L.23: designs and colour schemes say version and author, fonts the pack, its version,
    /// author and licence; a font is named by its family, one that can't be used by its pack.
    #[test]
    fn the_tooltip_of_an_entry_says_who_made_it() {
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
        assert_eq!(drawn(designs, "compact"), "compact");
        assert_eq!(drawn(designs, "torro"), "torro");
        assert_eq!(drawn(designs, "neon"), "torro", "a broken design is not drawn: the standard is");
        assert_eq!(drawn(designs, "gone"), "torro", "nor a missing one");
        let fonts = ok(&state.fonts).expect("fonts");
        assert_eq!(drawn(fonts, "tabular"), "tabular");
        assert_eq!(drawn(fonts, ""), "torro", "a core from before task L.22 names none");
        assert_eq!(drawn(ok(&state.schemes).expect("schemes"), "neon"), "torro");
    }

    /// Task L.37: the font's row says what the drawn design suggests – drawn already, another font
    /// drawn, or a font that is not there – and marks the suggested font's button; a design without
    /// a suggestion, and a broken one (the standard is drawn), say nothing.
    #[test]
    fn the_font_row_says_what_the_drawn_design_suggests() {
        let mut state = demo();
        let mut settings = Settings::default();
        assert_eq!(font_hint(&state, &settings), None, "the standard suggests no font");
        settings.design = "console".into();
        let missing = font_hint(&state, &settings).expect("Console suggests one");
        assert_eq!(missing, FontHint::Missing { design: "Console", font: "jetbrains-mono" });
        assert!(!missing.marks("jetbrains-mono"), "no button to mark");
        let sentence: [(&str, &dyn std::fmt::Display); 2] = [("design", &"Console"), ("font", &"jetbrains-mono")];
        assert_eq!(missing.sentence(), trf("{design} suggests {font}, which isn't installed.", &sentence));

        // Compact suggests the demo's Tabular, for this test.
        if let Some(Ok(designs)) = state.designs.as_mut() {
            let compact = designs.iter_mut().find(|entry| entry.info.id == "compact").expect("compact");
            compact.info.font = Some("tabular".into());
        }
        settings.design = "compact".into();
        let other = font_hint(&state, &settings).expect("a hint");
        assert_eq!(other, FontHint::Other { design: "Compact", font: "Inter Tabular", id: "tabular" });
        assert!(other.marks("tabular") && !other.marks("torro"));
        assert_eq!(other.design(), "Compact");
        let sentence: [(&str, &dyn std::fmt::Display); 2] = [("design", &"Compact"), ("font", &"Inter Tabular")];
        assert_eq!(other.sentence(), trf("{design} suggests {font}.", &sentence));
        settings.font = "tabular".into();
        let drawn = font_hint(&state, &settings).expect("a hint");
        assert_eq!(drawn, FontHint::Drawn { design: "Compact", font: "Inter Tabular", id: "tabular" });
        assert!(drawn.marks("tabular"));
        assert_eq!(drawn.sentence(), trf("{font} is suggested for {design}.", &sentence));
        settings.design = "neon".into();
        assert_eq!(font_hint(&state, &settings), None, "a broken one is not drawn: the standard is");
        assert_eq!(font_hint(&State::default(), &settings), None, "nothing listed yet");

        let why = trf("Suggested for {design}", &[("design", &"Compact")]);
        assert_eq!(suggested_tip(Some("Compact"), Some("1.2.0".into())), Some(format!("{why}\n1.2.0")));
        assert_eq!(suggested_tip(None, Some("1.2.0".into())), Some("1.2.0".into()));
        assert_eq!(suggested_tip(Some("Compact"), None), Some(why));
    }

    /// Task L.37: choosing a design keeps what was drawn; the reload that carries it names what the
    /// core switched with it – the font by its family, the mail view –, nothing when nothing changed.
    /// A reload from before the save waits, a refused design leaves no hint, and choosing again
    /// clears the last hint.
    #[test]
    fn the_hint_names_what_the_core_switched_with_a_design() {
        let data = Data::default();
        let mut state = demo();
        let action = update(&mut state, Message::Choose(Part::Design, "console".into()), &data);
        assert!(matches!(action, Action::Choose(Part::Design, ref id) if id == "console"), "{action:?}");
        assert_eq!(state.switched, None);

        // The core switched the mail view and the font.
        let carried = Settings {
            design: "console".into(),
            font: "tabular".into(),
            mail_view: MailFormat::Text,
            ..data.settings.clone()
        };
        loaded(&mut state, &carried, true);
        assert_eq!(state.switched, None, "still waiting for the save");
        loaded(&mut state, &carried, false);
        let switched = Switched { font: Some("tabular".into()), mail_view: Some(MailFormat::Text) };
        assert_eq!(state.switched.as_ref(), Some(&switched));
        let list = format!("{}, {}", trf("font {font}", &[("font", &"Inter Tabular")]), tr("plain-text mail"));
        assert_eq!(switched_list(&switched, &state.fonts), list);
        drop(appearance(&state, &data));
        drop(designs(&state, &data.settings, size::content_max()));

        // Nothing changed: no hint; and choosing again cleared the last one.
        let _ = update(&mut state, Message::Choose(Part::Design, "compact".into()), &data);
        assert_eq!(state.switched, None);
        loaded(&mut state, &Settings { design: "compact".into(), ..data.settings.clone() }, false);
        assert_eq!(state.switched, None, "nothing was switched");

        // Refused: the settings still name the design before.
        let _ = update(&mut state, Message::Choose(Part::Design, "console".into()), &data);
        loaded(&mut state, &data.settings, false);
        assert_eq!((state.switched.as_ref(), state.before.as_ref()), (None, None));

        // Back to HTML, with a font that is not listed: named by its id.
        let back = Switched { font: Some("plex".into()), mail_view: Some(MailFormat::Html) };
        let list = format!("{}, {}", trf("font {font}", &[("font", &"plex")]), tr("HTML mail"));
        assert_eq!(switched_list(&back, &state.fonts), list);
    }

    /// Task L.37 (the demo has no core): the suggestions of a design as the core would apply them –
    /// Console's mail view; its font only when it is listed as usable.
    #[test]
    fn the_demo_knows_what_a_design_suggests() {
        let mut state = demo();
        assert_eq!(suggestions_of(&state, "console"), (None, Some(MailFormat::Text)), "JetBrains Mono is missing");
        assert_eq!(suggestions_of(&state, "torro"), (None, None));
        assert_eq!(suggestions_of(&state, "gone"), (None, None));
        if let Some(Ok(designs)) = state.designs.as_mut() {
            let console = designs.iter_mut().find(|entry| entry.info.id == "console").expect("console");
            console.info.font = Some("tabular".into());
        }
        assert_eq!(suggestions_of(&state, "console"), (Some("tabular".into()), Some(MailFormat::Text)));
    }

    /// Task L.37: the colour schemes' dropdown has the sections "Torro", "Light and dark" and "Dark
    /// only" in this order, each scheme in its own, and leaves out a section without one.
    #[test]
    fn the_scheme_dropdown_groups_its_entries() {
        let state = demo();
        let schemes = ok(&state.schemes).expect("schemes");
        let ids = |groups: Vec<(SchemeGroup, Vec<&SchemeEntry>)>| -> Vec<(SchemeGroup, Vec<String>)> {
            groups
                .into_iter()
                .map(|(group, members)| (group, members.iter().map(|e| e.info.id.clone()).collect()))
                .collect()
        };
        let strings = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ids(scheme_groups(schemes)),
            vec![
                (SchemeGroup::Torro, strings(&["torro"])),
                (SchemeGroup::LightAndDark, strings(&["harbor", "paper", "neon"])),
                (SchemeGroup::DarkOnly, strings(&["midnight"])),
            ]
        );
        assert_eq!(SchemeGroup::Torro.title(), "Torro", "a name");
        assert_eq!(SchemeGroup::DarkOnly.title(), tr("Dark only"));
        let without: Vec<SchemeEntry> = schemes.iter().filter(|e| !e.info.dark_only).cloned().collect();
        let groups: Vec<SchemeGroup> = scheme_groups(&without).into_iter().map(|(group, _)| group).collect();
        assert_eq!(groups, [SchemeGroup::Torro, SchemeGroup::LightAndDark], "no empty section");
    }

    /// Task L.37: the colour scheme's button opens and closes its dropdown, a layer over the window;
    /// a click next to it, Escape, choosing a scheme, opening the reasons of a broken one and leaving
    /// the page close it. Choosing a scheme or a font goes to the shell, which saves it with
    /// `set_setting` (`Ui::choose`).
    #[test]
    fn the_scheme_dropdown_opens_and_closes() {
        let data = Data::default();
        let mut state = demo();
        assert!(overlay(&state, &data).is_none() && escape(&state).is_none());
        let action = update(&mut state, Message::ToggleSchemes, &data);
        assert!(matches!(action, Action::None), "{action:?}");
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseSchemes)));
        let _ = update(&mut state, Message::CloseSchemes, &data);
        assert!(overlay(&state, &data).is_none());

        let _ = update(&mut state, Message::ToggleSchemes, &data);
        let choose = update(&mut state, Message::Choose(Part::ColorScheme, "midnight".into()), &data);
        assert!(matches!(choose, Action::Choose(Part::ColorScheme, ref id) if id == "midnight"), "{choose:?}");
        assert!(!state.schemes_open, "choosing closes it");
        assert_eq!(state.before, None, "only a design is compared after the reload");

        let _ = update(&mut state, Message::ToggleSchemes, &data);
        let _ = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data);
        assert!(!state.schemes_open, "the reasons show under the group");
        let _ = update(&mut state, Message::ToggleSchemes, &data);
        leave(&mut state);
        assert!(overlay(&state, &data).is_none(), "leaving the page closes it");
        assert!(overlay(&State::default(), &Data { ..Data::default() }).is_none(), "nothing listed: nothing to open");
    }

    /// Task L.37: a design's density comes from the core (`DesignInfo::density`) as dots and a word,
    /// one more dot per step.
    #[test]
    fn the_density_has_its_dots_and_word() {
        let levels = [DensityLevel::Airy, DensityLevel::Balanced, DensityLevel::Dense, DensityLevel::VeryDense];
        assert_eq!(levels.map(density_dots), [0, 1, 2, 3]);
        assert_eq!(levels.map(density_label), [tr("Airy"), tr("Balanced"), tr("Dense"), tr("Very dense")]);
        let state = demo();
        let designs = ok(&state.designs).expect("designs");
        let listed: Vec<(&str, DensityLevel)> =
            designs.iter().filter(|e| e.info.valid).map(|e| (e.info.id.as_str(), e.info.density)).collect();
        assert_eq!(
            listed,
            [("torro", DensityLevel::Balanced), ("compact", DensityLevel::Dense), ("console", DensityLevel::VeryDense)]
        );
        for level in levels {
            drop(density(level, true));
            drop(density(level, false));
        }
    }

    /// Task L.37: the small mail window is drawn from the design's own values, scaled from the window
    /// to the thumbnail: the denser the design, the lower its rows and the narrower its sidebar and
    /// rail; Console's list is wider. The thumbnail follows the design it is drawn in.
    #[test]
    fn the_mail_preview_is_drawn_from_the_design() {
        let state = demo();
        let of = |id: &str| design_of(&state, id).expect(id);
        let width = MAIL_WINDOW.width / 10.0;
        let [standard, compact, console] = [of("torro"), of("compact"), of("console")].map(|d| Regions::of(&d, width));
        assert_eq!(standard.scale, 0.1);
        assert_eq!((standard.sidebar, standard.header, standard.row), (24.0, 6.4, 10.0));
        assert!(standard.row > compact.row && compact.row > console.row);
        assert!(standard.sidebar > compact.sidebar && compact.sidebar > console.sidebar);
        assert!(standard.rail > console.rail && console.list > standard.list);

        let thumbnail = thumbnail_size();
        assert_eq!(thumbnail.width, PREVIEW_CONTROLS * size::control_sm());
        assert_eq!(thumbnail.width / thumbnail.height, MAIL_WINDOW.width / MAIL_WINDOW.height);
        let entries = ok(&state.designs).expect("designs");
        for entry in entries {
            for (drawn, stacked) in [(true, false), (false, true)] {
                drop(design_row(entry, drawn, stacked));
            }
        }
    }

    #[test]
    fn listed_parts_are_kept_with_what_they_draw_with() {
        let mut state = State::default();
        assert!(unlisted(&state));
        listed(&mut state, super::super::launch::demo_looks());
        assert!(!unlisted(&state));
        assert!(design_of(&state, "console").is_some(), "an entry that can be used carries its part");
        assert!(font_of(&state, "tabular").is_some());
        assert!(scheme_of(&state, "harbor").is_some());
        assert!(design_of(&state, "neon").is_none(), "one that can't, none");
        assert!(font_of(&state, "brush").is_none());
        assert!(scheme_of(&state, "gone").is_none());
        listed(&mut state, Lists::failed("core is not running".into()));
        assert!(design_of(&state, "console").is_none());
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
        let marked = Data { settings: Settings { design: "console".into(), ..Settings::default() }, ..Data::default() };
        let _page: Element<'_, Message> = appearance(&listing, &marked);
        // The page itself is laid out for its width; build it wide and narrow, in every state.
        for state in [&loading, &listing, &failed, &partly] {
            for width in [size::content_max(), size::props()] {
                drop(page(state, &marked.settings, width));
                drop(looks(state, &marked.settings, width));
            }
        }
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
        assert!(Maps::default().merged(&serde_json::json!({"tiles": keyed})).is_err(), "refused by the core");
        let why = address_problem(keyed, true).expect("a reason for people");
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

    /// Review of #278: an address row says what its service gets, and "Press Enter to save" in the
    /// same line while its address is typed, so the row keeps its height; with maps off the fields
    /// are drawn faded and still take typing.
    #[test]
    fn an_address_row_keeps_its_height_and_fades_while_maps_are_off() {
        let data = Data::default();
        let mut state = State::default();
        let maps = &data.settings.maps;
        let before = address_detail(&state.maps, maps, Address::Geocoder);
        assert_eq!(before, tr("Gets what you search for and this computer's IP address"));
        let _ =
            update(&mut state, Message::AddressInput(Address::Geocoder, "https://photon.example.org".into()), &data);
        assert_eq!(address_detail(&state.maps, maps, Address::Geocoder), tr("Press Enter to save"));
        assert_eq!(address_detail(&state.maps, maps, Address::Tiles), tr("Gets the coordinates of the place"));

        let theme = Theme::Dark;
        let active = iced::widget::text_input::Status::Active;
        let faded = address_style(&theme, active, true);
        assert_eq!(faded.value, style::text_input(&theme, iced::widget::text_input::Status::Disabled).value);
        assert_eq!(address_style(&theme, active, false).value, style::text_input(&theme, active).value);
        let focused = iced::widget::text_input::Status::Focused { is_hovered: false };
        assert_eq!(address_style(&theme, focused, true).border, style::text_input(&theme, focused).border);
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
