//! The settings pages General and Appearance (tasks S.9 and S.14 in `docs/tasks/steuerzentrale.md`):
//! settings groups, each with a footer sentence that explains the consequence.
//!
//! - **General** ([`view`]): "Language" ([`suite::language`]); "Maps" ([`maps`], task TE.10) switches
//!   the calendar's place search and small map on and sets their services; "Startup" ([`startup`])
//!   has the "Start at login" switch and the one of Workspaces; "Setup" runs the first-run setup
//!   again. The group "Apps" of task S.21 is the section Apps since issue #411: each app's card
//!   switches it, opens it and names its window class there (`super::apps`).
//! - **Appearance** ([`appearance`], its own page in the sidebar since S.14, so it is found at a
//!   glance): the suite's page of `torro_settings_ui::suite` (issue #408), which an app without the
//!   main application shows too; how it looks and works is described there.
//!
//! What a single app or window shows and how it behaves is set in that window, behind its gear
//! (task S.21): the calendar's night and reminders, how mail is read, the sources of the
//! Notification Hub. Only what holds for the whole suite is left here.
//!
//! Both pages share one state and one message type; the suite's part of both is
//! [`Message::Suite`], whose actions map onto the control centre's ([`update`]). There is no Save
//! button: every change sends `Request::SetSetting` with the one key that changed, and the shell
//! reloads `Data` afterwards. The zoom goes through the window's own zoom instead (`Action::Zoom`),
//! which saves it and applies it at once, also in the demo. Choosing a design, font or colour scheme
//! draws this window with it at once (`Action::Choose`). The page shows `data.settings`, so it
//! follows the daemon.

use iced::widget::{column, container, row};
use iced::{Alignment, Element, Length, Theme};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, GeocoderKind, Maps, MapsApp, Settings, address_problem};
use torro_gui::icon::Icon;
use torro_gui::theme::space;
use torro_gui::{layout, style, widgets};
use torro_ipc::{Request, SettingKey};
use torro_settings_ui::parts::{field, notice, picker, switch};
use torro_settings_ui::suite::{self, Place};

use super::setup::component_name;
use super::{Action, Data, setting_row};

/// The page's own state: the suite's pages ([`suite::State`]) and the addresses of "Maps".
#[derive(Debug, Default)]
pub struct State {
    /// Appearance, Language and Startup (issue #408).
    pub suite: suite::State,
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

#[derive(Debug, Clone)]
pub enum Message {
    /// Appearance, Language and Startup: the suite's pages (issue #408).
    Suite(suite::Message),
    /// A part of the suite was switched on or off: Workspaces under Startup (task W.2; the apps are
    /// switched on their cards in the section Apps since issue #411).
    Component(Component, bool),
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
        Message::Suite(message) => {
            return match suite::update(&mut state.suite, message, &data.settings) {
                suite::Action::None => Action::None,
                suite::Action::Send(request) => Action::Send(request),
                suite::Action::Zoom(step) => Action::Zoom(step),
                suite::Action::Focus(field) => Action::Focus(field.into()),
                suite::Action::ListLooks => Action::ListLooks,
                suite::Action::Choose(part, id) => Action::Choose(part, id),
                suite::Action::OpenFolder(part) => Action::OpenFolder(part),
            };
        }
        Message::Component(component, enabled) => {
            (SettingKey::Component, serde_json::to_value(ComponentSwitch { component, enabled }).unwrap_or_default())
        }
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

/// The page General: language, maps, startup, setup.
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    // Its groups side by side where the page is wide (issue #464).
    widgets::page_columns(move || {
        let settings = &data.settings;
        let language = suite::language(settings, Place::ControlCentre).map(Message::Suite);
        vec![language, maps(state, &settings.maps), startup(settings), setup()]
    })
}

/// The group "Startup": "Start at login" as the suite's page has it ([`suite::startup`]), and the
/// switch of Workspaces, which came here from the group "Apps" (issue #411): Workspaces has no
/// window and no card in the app catalog, and what it does at login – the templates that load there –
/// belongs with starting.
fn startup(settings: &Settings) -> Element<'_, Message> {
    let at_login = switch(settings.start_at_login, |on| Message::Suite(suite::Message::StartAtLogin(on)));
    let workspaces =
        switch(settings.component_on(Component::Workspaces), |on| Message::Component(Component::Workspaces, on));
    let rows = vec![
        setting_row(None, tr("Start at login"), None, at_login),
        setting_row(
            None,
            component_name(Component::Workspaces),
            Some(tr("Save and restore window layouts").to_string()),
            workspaces,
        ),
    ];
    let footer =
        tr("Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready.");
    widgets::group(tr("Startup"), rows, Some(footer.to_string()))
}

/// The group "Setup": run the first-run setup again.
fn setup<'a>() -> Element<'a, Message> {
    let run = widgets::text_button(None, tr("Run setup again"), style::button::secondary, Some(Message::RunSetup));
    widgets::group(
        tr("Setup"),
        vec![setting_row(
            None,
            tr("First-run setup"),
            Some(tr("Nextcloud, components, accounts and AI, step by step").to_string()),
            run,
        )],
        Some(tr("What is connected stays; the steps show it.").to_string()),
    )
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

/// The page Appearance: the suite's page ([`suite::appearance`]).
pub fn appearance<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    suite::appearance(&state.suite, &data.settings).map(Message::Suite)
}

/// The page's layer over the window: the dropdown of Appearance that is open ([`suite::overlay`]).
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    suite::overlay(&state.suite, &data.settings).map(|layer| layer.map(Message::Suite))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::settings::{Language, Settings};
    use torro_core::theme::Appearance;

    /// The key and value the page asked the shell to set.
    fn sent(action: Action) -> (SettingKey, serde_json::Value) {
        let Action::Send(Request::SetSetting { key, value }) = action else {
            panic!("expected SetSetting, got {action:?}");
        };
        (key, value)
    }

    /// Issue #411: General switches Workspaces under Startup, next to "Start at login"; the apps are
    /// switched on their cards in the section Apps.
    #[test]
    fn startup_switches_workspaces_and_start_at_login() {
        let data = Data::default();
        let set = |message| sent(update(&mut State::default(), message, &data));
        assert_eq!(
            set(Message::Component(Component::Workspaces, false)),
            (SettingKey::Component, serde_json::json!({"component": "workspaces", "enabled": false}))
        );
        assert_eq!(
            set(Message::Suite(suite::Message::StartAtLogin(false))),
            (SettingKey::StartAtLogin, serde_json::Value::Bool(false))
        );
        let off = Settings { components_off: [Component::Workspaces].into_iter().collect(), ..Settings::default() };
        drop(startup(&off));
        drop(view(&State::default(), &data));
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
