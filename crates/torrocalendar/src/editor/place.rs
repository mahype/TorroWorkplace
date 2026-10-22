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

use iced::widget::{Space, button, column, container, image, row, rule, stack, svg};
use iced::{Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{GeocoderKind, MapsApp};
use torro_gui::icon::{self, Icon, icon};
use torro_gui::theme::{self, palette, radius, size, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{Place, PlaceKind};
use torro_pim::model::Geo;

use super::Message;

/// The size of the map `calendar.map_preview` is asked for, in logical pixels (layout A); the
/// picture fills the width of the value column and keeps this height.
pub const MAP_WIDTH: u16 = 336;
pub const MAP_HEIGHT: u16 = 104;

/// Where the attribution of the map leads (ODbL).
pub(super) const OSM_COPYRIGHT: &str = "https://www.openstreetmap.org/copyright";

/// Why a place search shows no suggestions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Maps are off in the settings (the core's `unsupported`): nothing was sent.
    MapsOff,
    /// Any other failure, in the core's words (`busy`, `offline`, `error`), for a quiet line.
    Other(String),
}

/// The address "Open in Maps" opens for `location` (as typed) at `geo` with the app the settings
/// name, on this system: Apple Maps only on macOS and `geo:` only on Linux, otherwise
/// openstreetmap.org.
pub fn maps_link(app: MapsApp, location: &str, geo: Option<Geo>) -> String {
    link_for(on_this_system(app), location, geo)
}

/// `app`, or openstreetmap.org where this system cannot open it.
fn on_this_system(app: MapsApp) -> MapsApp {
    match app {
        MapsApp::Apple if !cfg!(target_os = "macos") => MapsApp::Osm,
        MapsApp::Geo if !cfg!(target_os = "linux") => MapsApp::Osm,
        app => app,
    }
}

/// The link of `app` (whatever the system): with `geo` the point, else `location` as a search, every
/// text percent-encoded. OpenStreetMap shows the point at zoom 17 as the concept says.
pub(super) fn link_for(app: MapsApp, location: &str, geo: Option<Geo>) -> String {
    let query = encode(location.trim());
    match (app, geo) {
        (MapsApp::Osm, Some(Geo { lat, lon })) => {
            format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}")
        }
        (MapsApp::Osm, None) => format!("https://www.openstreetmap.org/search?query={query}"),
        (MapsApp::Google, Some(Geo { lat, lon })) => {
            format!("https://www.google.com/maps/search/?api=1&query={lat},{lon}")
        }
        (MapsApp::Google, None) => format!("https://www.google.com/maps/search/?api=1&query={query}"),
        (MapsApp::Apple, Some(Geo { lat, lon })) if query.is_empty() => {
            format!("https://maps.apple.com/?ll={lat},{lon}&q={lat},{lon}")
        }
        (MapsApp::Apple, Some(Geo { lat, lon })) => format!("https://maps.apple.com/?ll={lat},{lon}&q={query}"),
        (MapsApp::Apple, None) => format!("https://maps.apple.com/?q={query}"),
        (MapsApp::Geo, Some(Geo { lat, lon })) => format!("geo:{lat},{lon}"),
        (MapsApp::Geo, None) => format!("geo:0,0?q={query}"),
    }
}

/// `text` percent-encoded for a query or fragment: everything but the unreserved characters of RFC
/// 3986 (letters, digits, `-._~`) as `%XX` of its UTF-8 bytes, so nothing typed can end the
/// parameter or add one.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
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
    list = list.push(caption(source));
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

/// A title or a quiet line in the suggestions.
fn caption<'a>(words: impl iced::widget::text::IntoFragment<'a>) -> Element<'a, Message> {
    container(typo::text(typo::caption(), words).style(theme::text_3))
        .padding(Padding::new(space::s()).bottom(space::xs()))
        .into()
}

/// The map under the location: the picture with a pin in the middle in `pin` (the calendar's
/// colour), ↗ "Open in Maps" (`open`) in the top right corner and "© OpenStreetMap" (a link to the
/// copyright page) in the bottom right one, in a frame with radius `md`.
pub(super) fn map_view<'a>(picture: &'a image::Handle, pin: Color, open: Message) -> Element<'a, Message> {
    let height = f32::from(MAP_HEIGHT);
    let picture =
        image(picture).width(Length::Fill).height(height).content_fit(ContentFit::Cover).border_radius(radius::md());
    let marker = icon(Icon::MapPin, space::xl()).style(move |_, _| svg::Style { color: Some(pin) });
    // The tip of the pin, not its middle, stands on the point.
    let marker = container(column![marker, Space::new().height(space::xl())]).center(Length::Fill);
    let open = widgets::boxed_button(Icon::ArrowUpRight, tr("Open in Maps"), Some(open));
    let credit = button(typo::text(typo::meta(), tr("© OpenStreetMap")).style(theme::text_2))
        .padding(Padding::ZERO.left(space::xs()).right(space::xs()))
        .style(style::button::secondary)
        .on_press(Message::OpenLink(OSM_COPYRIGHT.to_string()));
    let over = column![
        row![Space::new().width(Length::Fill), open],
        Space::new().height(Length::Fill),
        row![Space::new().width(Length::Fill), credit],
    ]
    .padding(Padding::new(space::xs()).bottom(0.0).right(0.0).top(space::xs()));
    let over = container(over).padding(Padding::ZERO.right(space::xs()));
    container(stack![picture, marker, over].width(Length::Fill).height(height))
        .width(Length::Fill)
        .style(frame)
        .clip(true)
        .into()
}

/// The frame of the map: `line_strong`, radius `md`.
fn frame(theme: &Theme) -> container::Style {
    container::Style {
        border: Border { color: palette(theme).line_strong, width: size::HAIRLINE, radius: radius::md().into() },
        background: Some(Background::Color(palette(theme).raised)),
        ..container::Style::default()
    }
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

/// The map `--demo` shows instead of asking the tile service: a calm sketch of streets and water in
/// the colours of `theme`, [`MAP_WIDTH`] × [`MAP_HEIGHT`] at twice the density, like the core's.
pub fn demo_map(theme: &Theme) -> image::Handle {
    let p = palette(theme);
    let (width, height) = (u32::from(MAP_WIDTH) * 2, u32::from(MAP_HEIGHT) * 2);
    let (land, road, water) = (p.hover.into_rgba8(), p.raised.into_rgba8(), p.bg.into_rgba8());
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let street = (height * 2 / 5..height / 2).contains(&y)
                || (width * 3 / 10..width * 3 / 10 + height / 16).contains(&x)
                || (width * 7 / 10..width * 7 / 10 + height / 16).contains(&x);
            let colour = if y > height * 4 / 5 {
                water
            } else if street {
                road
            } else {
                land
            };
            pixels.extend_from_slice(&colour);
        }
    }
    image::Handle::from_rgba(width, height, pixels)
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

    #[test]
    fn the_demo_needs_no_service() {
        assert!(demo_places().iter().all(|place| place.geo.on_map()));
        let _ = demo_map(&Theme::Dark);
        let _ = suggestions(&demo_places(), "Hafenstr 4", service_name(GeocoderKind::Photon));
        let _ = suggestions(&[], "Nowhere", service_name(GeocoderKind::Nominatim));
    }
}
