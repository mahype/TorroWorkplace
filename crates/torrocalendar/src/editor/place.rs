//! The place of an event in the editor (task TE.8, `docs/termin-editor.md` "Ort und Karte"): the
//! suggestions of the place search under the location, the small map with the attribution the
//! OpenStreetMap licence asks for, the link "Open in Maps", and what `--demo` shows instead of the
//! services (no request leaves the window there).
//!
//! The editor never talks to a map service itself: it asks the core (`calendar.search_places`,
//! `calendar.map_preview`) through the app, only on Enter or "Search", never while typing. "Open in
//! Maps" needs no service and works with maps off: without coordinates the link searches the
//! address as text. Apple Maps opens only on macOS and `geo:` only on Linux; elsewhere both fall back
//! to openstreetmap.org, as the setting says.

use iced::widget::{button, column, container, image, row, rule, svg};
use iced::{Alignment, Color, Element, Length, Padding};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{GeocoderKind, MapsApp};
use torro_gui::icon::{self, Icon, icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{Place, PlaceKind};
use torro_pim::model::Geo;

use super::Message;

/// The size of the picture `calendar.map_preview` is asked for, and the map itself, shared with the
/// contacts app (`torro_gui::map`, issue #355).
pub use torro_gui::map::{MAP_HEIGHT, MAP_WIDTH, OSM_COPYRIGHT, demo_map};

/// Why a place search shows no suggestions. The editor says it in a translated sentence
/// ([`Failure::message`]), never in the core's English `message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Maps are off in the settings (the core's `unsupported`): nothing was sent.
    MapsOff,
    /// Searches came too fast, or the service asks to wait (`busy`).
    Busy,
    /// The service cannot be reached (`offline`).
    Offline,
    /// The core did not answer (it may still be starting).
    NoAnswer,
    /// Any other failure (`error`, an answer that is no list of places).
    Other,
}

impl Failure {
    /// The failure for the error `code` of `calendar.search_places`; an unknown code is [`Failure::Other`].
    pub fn from_code(code: &str) -> Failure {
        match code {
            "unsupported" => Failure::MapsOff,
            "busy" => Failure::Busy,
            "offline" => Failure::Offline,
            _ => Failure::Other,
        }
    }

    /// The quiet line under the location, translated: what happened and what to do.
    pub(super) fn message(self) -> &'static str {
        match self {
            Failure::MapsOff => tr("Turn on maps in Settings"),
            Failure::Busy => tr("Too many place searches. Try again in a moment."),
            Failure::Offline => tr("The place search service cannot be reached."),
            Failure::NoAnswer => tr("Torro Workplace isn't responding. It may still be starting."),
            Failure::Other => tr("The place search did not work. Try again later."),
        }
    }
}

/// The address "Open in Maps" opens for `location` (as typed) at `geo` with the app the settings
/// name, on this system: Apple Maps only on macOS and `geo:` only on Linux, otherwise
/// openstreetmap.org.
pub fn maps_link(app: MapsApp, location: &str, geo: Option<Geo>) -> String {
    torro_core::maps::maps_link(app, location, geo.map(|Geo { lat, lon }| (lat, lon)))
}

/// The link of `app` (whatever the system): with `geo` the point, else `location` as a search, every
/// text percent-encoded. OpenStreetMap shows the point at zoom 17 as the concept says.
#[cfg(test)]
pub(super) fn link_for(app: MapsApp, location: &str, geo: Option<Geo>) -> String {
    torro_core::maps::link_for(app, location, geo.map(|Geo { lat, lon }| (lat, lon)))
}

/// Whether the app may hand `url` to the system: a web address `torro_gui::links` allows, or a
/// `geo:` link as [`link_for`] builds it (the only scheme besides the web that "Open in Maps" uses).
pub fn openable_map(url: &str) -> bool {
    let geo = url
        .strip_prefix("geo:")
        .is_some_and(|rest| !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control()));
    geo || torro_gui::links::openable(url)
}

/// The name of the place search service for "Search by …": the protocol the settings name (the
/// service's own name, not translated).
pub(super) fn service_name(kind: GeocoderKind) -> &'static str {
    match kind {
        GeocoderKind::Photon => "Photon",
        GeocoderKind::Nominatim => "Nominatim",
    }
}

/// The suggestions under the location: a row per place (earlier places with the clock of history),
/// "No places found" when there are none, "Use “…” as written" for the text without a map, and the
/// source line with the attribution.
pub(super) fn suggestions<'a>(places: &'a [Place], query: &str, service: &'static str) -> Element<'a, Message> {
    let mut list = column![caption(tr("Suggestions"))];
    if places.is_empty() {
        let none = typo::text(typo::body(), tr("No places found")).style(theme::text_3);
        list = list.push(container(none).padding(style::ITEM_PADDING.top(space::s()).bottom(space::s())));
    }
    for (index, place) in places.iter().enumerate() {
        let mark = match place.kind {
            PlaceKind::Recent => Icon::History,
            PlaceKind::Search => Icon::MapPin,
        };
        list = list.push(entry(mark, place.label.clone(), place.detail.clone(), Message::PickPlace(index)));
    }
    let written = trf("Use “{text}” as written", &[("text", &query.trim())]);
    list = list
        .push(container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]))
        .push(entry(Icon::SquarePen, written, tr("Without a map").to_string(), Message::AsWritten));
    let source = trf("Search by {service} · © OpenStreetMap contributors", &[("service", &service)]);
    list = list.push(source_line(source));
    container(list).width(widgets::POPOVER_WIDTH).padding(style::MENU_INSET).style(style::container::menu).into()
}

/// One suggestion: its mark, the label on the first line and where it is under it.
fn entry<'a>(mark: Icon, label: String, detail: String, on_press: Message) -> Element<'a, Message> {
    let mark = icon(mark, icon::SIZE).style(|t, _| svg::Style { color: Some(palette(t).text_2) });
    let mut words = column![typo::text(typo::body_strong(), label).style(theme::text)];
    if !detail.trim().is_empty() {
        words = words.push(typo::text(typo::meta(), detail).style(theme::text_3));
    }
    let content = row![mark, words].spacing(space::s()).align_y(Alignment::Center);
    button(content)
        .width(Length::Fill)
        .padding(style::ITEM_PADDING.top(space::xs()).bottom(space::xs()))
        .style(style::button::menu_item(false))
        .on_press(on_press)
        .into()
}

/// The title of the suggestions.
fn caption<'a>(words: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    container(typo::text(typo::caption(), words).style(theme::text_3))
        .padding(Padding::new(space::s()).bottom(space::xs()))
        .into()
}

/// The quiet line at the foot of the suggestions: who searched and whose data it is, metadata in
/// `META` and `text_3`, not a title.
fn source_line<'a>(words: String) -> Element<'a, Message> {
    container(typo::text(typo::meta(), words).style(theme::text_3))
        .padding(Padding::new(space::s()).top(space::xs()))
        .into()
}

/// The map under the location: the picture with a pin in the middle in `pin` (the calendar's
/// colour), ↗ "Open in Maps" (`open`) and "© OpenStreetMap" ([`torro_gui::map::map_view`]).
pub(super) fn map_view<'a>(picture: &'a image::Handle, pin: Color, open: Message) -> Element<'a, Message> {
    torro_gui::map::map_view(picture, pin, open, Message::OpenLink(OSM_COPYRIGHT.to_string()))
}

/// The places `--demo` suggests for any search, without the network: two of the same street in
/// different towns and an earlier place of the demo events.
pub fn demo_places() -> Vec<Place> {
    let place = |label: &str, detail: &str, lat: f64, lon: f64, kind: PlaceKind| Place {
        label: label.into(),
        detail: detail.into(),
        geo: Geo { lat, lon },
        kind,
    };
    vec![
        place("Meeting room Elbe", "", 53.5458, 9.9672, PlaceKind::Recent),
        place("Hafenstraße 4", "20359 Hamburg, Germany", 53.5458, 9.9672, PlaceKind::Search),
        place("Hafenstraße 4", "21614 Buxtehude, Germany", 53.4769, 9.7011, PlaceKind::Search),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAMBURG: Geo = Geo { lat: 53.5458, lon: 9.9672 };

    /// Task TE.8: every way of "Open in Maps", with and without coordinates; typed text is encoded.
    #[test]
    fn open_in_maps_links_for_every_app() {
        let address = "Hafenstraße 4, 20359 Hamburg";
        let encoded = "Hafenstra%C3%9Fe%204%2C%2020359%20Hamburg";
        let cases = [
            (
                MapsApp::Osm,
                Some(HAMBURG),
                "https://www.openstreetmap.org/?mlat=53.5458&mlon=9.9672#map=17/53.5458/9.9672",
            ),
            (MapsApp::Osm, None, &format!("https://www.openstreetmap.org/search?query={encoded}")),
            (MapsApp::Google, Some(HAMBURG), "https://www.google.com/maps/search/?api=1&query=53.5458,9.9672"),
            (MapsApp::Google, None, &format!("https://www.google.com/maps/search/?api=1&query={encoded}")),
            (MapsApp::Apple, Some(HAMBURG), &format!("https://maps.apple.com/?ll=53.5458,9.9672&q={encoded}")),
            (MapsApp::Apple, None, &format!("https://maps.apple.com/?q={encoded}")),
            (MapsApp::Geo, Some(HAMBURG), "geo:53.5458,9.9672"),
            (MapsApp::Geo, None, &format!("geo:0,0?q={encoded}")),
        ];
        for (app, geo, want) in cases {
            assert_eq!(link_for(app, address, geo), want, "{app:?} {geo:?}");
            assert!(openable_map(&link_for(app, address, geo)), "{app:?} {geo:?}");
        }
        assert_eq!(
            link_for(MapsApp::Apple, " ", Some(HAMBURG)),
            "https://maps.apple.com/?ll=53.5458,9.9672&q=53.5458,9.9672"
        );
    }

    #[test]
    fn nothing_typed_can_leave_its_parameter() {
        let link = link_for(MapsApp::Google, "a&b=c #d?e/f+g%", None);
        assert_eq!(link, "https://www.google.com/maps/search/?api=1&query=a%26b%3Dc%20%23d%3Fe%2Ff%2Bg%25");
        assert!(!openable_map("geo:"), "a geo link needs a point");
        assert!(!openable_map("geo:1,2 3"));
        assert!(!openable_map("file:///etc/passwd"));
    }

    /// Apple Maps only on macOS, `geo:` only on Linux; elsewhere openstreetmap.org.
    #[test]
    fn apps_this_system_lacks_open_openstreetmap() {
        let apple = maps_link(MapsApp::Apple, "Berlin", None);
        assert_eq!(apple.starts_with("https://maps.apple.com/"), cfg!(target_os = "macos"));
        let geo = maps_link(MapsApp::Geo, "Berlin", None);
        assert_eq!(geo.starts_with("geo:"), cfg!(target_os = "linux"));
        assert!(maps_link(MapsApp::Osm, "Berlin", None).starts_with("https://www.openstreetmap.org/"));
    }

    /// Review of #278: the map is laid out by its token, not by the size it is fetched in, and
    /// both of its buttons ("Open in Maps", "© OpenStreetMap") are proper click targets that fit it.
    #[test]
    fn the_map_holds_two_buttons_of_full_height() {
        assert!(size::control_sm() >= 24.0, "a click target of at least 24");
        assert!(size::map_preview() >= 2.0 * size::control_sm() + 3.0 * space::xs());
        assert!(torro_gui::map::pin_size() > icon::SIZE);
        let picture = image::Handle::from_rgba(1, 1, vec![0; 4]);
        let _ = map_view(&picture, Color::WHITE, Message::OpenMaps);
    }

    #[test]
    fn the_demo_needs_no_service() {
        assert!(demo_places().iter().all(|place| place.geo.on_map()));
        let _ = demo_map(&iced::Theme::Dark);
        let _ = suggestions(&demo_places(), "Hafenstr 4", service_name(GeocoderKind::Photon));
        let _ = suggestions(&[], "Nowhere", service_name(GeocoderKind::Nominatim));
    }
}
