//! The links of "Open in Maps" (task TE.8, issue #355): the address the app of the settings
//! ([`MapsApp`]) opens for a place, with its point or with its address as a search. The calendar's
//! editor builds them in the app, the core for `contacts.open_in_maps`; both need no map service,
//! so they work with maps off.
//!
//! Apple Maps opens only on macOS and `geo:` only on Linux; elsewhere both fall back to
//! openstreetmap.org, as the setting says ([`on_this_system`]).

use crate::settings::MapsApp;

/// The address "Open in Maps" opens for `location` (as typed) at `point` (latitude, longitude)
/// with the app the settings name, on this system: Apple Maps only on macOS and `geo:` only on
/// Linux, otherwise openstreetmap.org.
pub fn maps_link(app: MapsApp, location: &str, point: Option<(f64, f64)>) -> String {
    link_for(on_this_system(app), location, point)
}

/// `app`, or openstreetmap.org where this system cannot open it.
pub fn on_this_system(app: MapsApp) -> MapsApp {
    match app {
        MapsApp::Apple if !cfg!(target_os = "macos") => MapsApp::Osm,
        MapsApp::Geo if !cfg!(target_os = "linux") => MapsApp::Osm,
        app => app,
    }
}

/// The link of `app` (whatever the system): with `point` (latitude, longitude) the point, else
/// `location` as a search, every text percent-encoded. OpenStreetMap shows the point at zoom 17 as
/// the concept says.
pub fn link_for(app: MapsApp, location: &str, point: Option<(f64, f64)>) -> String {
    let query = encode(location.trim());
    match (app, point) {
        (MapsApp::Osm, Some((lat, lon))) => {
            format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=17/{lat}/{lon}")
        }
        (MapsApp::Osm, None) => format!("https://www.openstreetmap.org/search?query={query}"),
        (MapsApp::Google, Some((lat, lon))) => format!("https://www.google.com/maps/search/?api=1&query={lat},{lon}"),
        (MapsApp::Google, None) => format!("https://www.google.com/maps/search/?api=1&query={query}"),
        (MapsApp::Apple, Some((lat, lon))) if query.is_empty() => {
            format!("https://maps.apple.com/?ll={lat},{lon}&q={lat},{lon}")
        }
        (MapsApp::Apple, Some((lat, lon))) => format!("https://maps.apple.com/?ll={lat},{lon}&q={query}"),
        (MapsApp::Apple, None) => format!("https://maps.apple.com/?q={query}"),
        (MapsApp::Geo, Some((lat, lon))) => format!("geo:{lat},{lon}"),
        (MapsApp::Geo, None) => format!("geo:0,0?q={query}"),
    }
}

/// `text` percent-encoded for a query or fragment: everything but the unreserved characters of RFC
/// 3986 (letters, digits, `-._~`) as `%XX` of its UTF-8 bytes, so nothing typed can end the
/// parameter or add one.
pub fn encode(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_wins_over_the_address() {
        let link = link_for(MapsApp::Osm, "Berlin", Some((52.5, 13.4)));
        assert_eq!(link, "https://www.openstreetmap.org/?mlat=52.5&mlon=13.4#map=17/52.5/13.4");
        assert_eq!(link_for(MapsApp::Geo, "a b", None), "geo:0,0?q=a%20b");
        assert_eq!(encode("ä&"), "%C3%A4%26");
    }

    #[test]
    fn apps_this_system_lacks_open_openstreetmap() {
        assert_eq!(on_this_system(MapsApp::Apple) == MapsApp::Apple, cfg!(target_os = "macos"));
        assert_eq!(on_this_system(MapsApp::Geo) == MapsApp::Geo, cfg!(target_os = "linux"));
        assert_eq!(on_this_system(MapsApp::Google), MapsApp::Google);
    }
}
