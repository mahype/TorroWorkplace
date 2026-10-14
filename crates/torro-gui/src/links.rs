//! Links in plain text (issue #126): cut a text into words and addresses ([`pieces`]), list the
//! addresses ([`links`]), tell which may be opened ([`openable`]) and which join a video call
//! ([`meeting`]). The calendar shows the links of an event's location and notes with them; the
//! inbox can use the same cut for mail and entries.
//!
//! Only `http://`, `https://` and `mailto:` count, written at the start of a word. Nothing else is
//! opened from text (no `file:`, no custom schemes), so a click never starts a program the person
//! did not see coming. A full stop, comma, quote or a closing bracket that has no opening one in
//! the address belongs to the sentence, not to the address: "(see https://example.org)." gives
//! `https://example.org`, while `https://en.wikipedia.org/wiki/Rust_(language)` stays whole.

/// The schemes a link may have, in lower case.
const SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

/// A piece of a text: words, or an address that may be opened by a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Words as written, white space included.
    Text(&'a str),
    /// An address as written (scheme in any case), whole: the link shows exactly where it goes.
    Link(&'a str),
}

/// `text` cut into words and addresses, in order; joined again they give `text` back. An address
/// starts with `http://`, `https://` or `mailto:` (any case) at the start of a word and runs to the
/// next white space or `<`, `>`, `"`; trailing punctuation and unbalanced closing brackets belong
/// to the sentence. A scheme with nothing after it stays words.
///
/// ```ignore
/// for piece in links::pieces(&notes) {
///     match piece {
///         Piece::Text(words) => spans.push(span(words)),
///         Piece::Link(url) => spans.push(span(url).underline(true).link(url.to_owned())),
///     }
/// }
/// ```
pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    // ASCII lower case keeps every byte where it is, so positions carry over to `text`.
    let lower = text.to_ascii_lowercase();
    let mut pieces = Vec::new();
    // Start of the words not handed out yet, and where to look for the next address.
    let (mut words_from, mut from) = (0, 0);
    while let Some((start, scheme)) = link_start(&lower, from) {
        let tail = &text[start..];
        let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(tail.len());
        let link = trim_end(&tail[..end]);
        if link.len() <= scheme {
            // Only the scheme: words, not a link.
            from = start + scheme;
            continue;
        }
        if start > words_from {
            pieces.push(Piece::Text(&text[words_from..start]));
        }
        pieces.push(Piece::Link(link));
        from = start + link.len();
        words_from = from;
    }
    if words_from < text.len() {
        pieces.push(Piece::Text(&text[words_from..]));
    }
    pieces
}

/// The addresses in `text` ([`pieces`]), each once, in the order they first appear.
pub fn links(text: &str) -> Vec<&str> {
    let mut found: Vec<&str> = Vec::new();
    for piece in pieces(text) {
        if let Piece::Link(link) = piece
            && !found.contains(&link)
        {
            found.push(link);
        }
    }
    found
}

/// Whether `url` may be opened: `http://`, `https://` or `mailto:` (any case) with something after
/// the scheme, and no white space or control characters. Check this right before handing an
/// address to the browser, also for addresses that came from [`pieces`].
pub fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let scheme = SCHEMES.iter().find(|scheme| lower.starts_with(**scheme));
    scheme.is_some_and(|scheme| url.len() > scheme.len()) && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The video call service `url` joins, by its address: Zoom, Microsoft Teams, Google Meet, Webex,
/// Jitsi Meet or Nextcloud Talk (a `/call/` path, on any server). `None` for any other page and
/// for an address that may not be opened ([`openable`]). The name is the service's own and is not
/// translated.
pub fn meeting(url: &str) -> Option<&'static str> {
    if !openable(url) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let after = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    // Drop a user before `@` and a port after `:`.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    let path = &after[end..];
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on("zoom.us") {
        Some("Zoom")
    } else if on("teams.microsoft.com") || on("teams.live.com") {
        Some("Microsoft Teams")
    } else if host == "meet.google.com" {
        Some("Google Meet")
    } else if on("webex.com") {
        Some("Webex")
    } else if host == "meet.jit.si" {
        Some("Jitsi Meet")
    } else if path.starts_with("/call/") || path.starts_with("/index.php/call/") {
        Some("Nextcloud Talk")
    } else {
        None
    }
}

/// Where the first address in `lower` (a text in ASCII lower case) at or after `from` starts at the
/// start of a word, and the length of its scheme.
fn link_start(lower: &str, from: usize) -> Option<(usize, usize)> {
    let word_start = |at: usize| at == 0 || !lower[..at].ends_with(|c: char| c.is_alphanumeric());
    SCHEMES
        .iter()
        .filter_map(|scheme| {
            let found = lower[from..].match_indices(scheme).map(|(at, _)| from + at).find(|&at| word_start(at));
            found.map(|at| (at, scheme.len()))
        })
        .min_by_key(|&(at, _)| at)
}

/// `link` without the punctuation of the sentence around it: full stops, commas, quotes and the
/// like at its end, and closing brackets that have no opening one in the address.
fn trim_end(mut link: &str) -> &str {
    while let Some(last) = link.chars().next_back() {
        let belongs_to_sentence = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' => true,
            ')' => unbalanced(link, '(', ')'),
            ']' => unbalanced(link, '[', ']'),
            '}' => unbalanced(link, '{', '}'),
            _ => false,
        };
        if !belongs_to_sentence {
            break;
        }
        link = &link[..link.len() - last.len_utf8()];
    }
    link
}

/// Whether `link` closes more brackets than it opens.
fn unbalanced(link: &str, open: char, close: char) -> bool {
    link.matches(close).count() > link.matches(open).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The addresses `pieces` finds in `text`.
    fn found(text: &str) -> Vec<&str> {
        pieces(text)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Link(link) => Some(link),
                Piece::Text(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_pieces_give_the_text_back() {
        let texts = [
            "",
            "no link here",
            "See https://example.org/a?b=c#d, then mailto:anna@example.org.",
            "(https://example.org) and [http://x.de/y]!",
            "http:// alone, https://ok.org and http://",
            "Grüße – https://bäckerei.de/öffnungszeiten.",
        ];
        for text in texts {
            let whole: String = pieces(text)
                .into_iter()
                .map(|piece| match piece {
                    Piece::Text(words) | Piece::Link(words) => words,
                })
                .collect();
            assert_eq!(whole, text);
        }
        assert_eq!(pieces(""), []);
        assert_eq!(pieces("Room 3"), [Piece::Text("Room 3")]);
    }

    #[test]
    fn punctuation_at_the_end_belongs_to_the_sentence() {
        assert_eq!(found("Agenda: https://example.org/agenda."), ["https://example.org/agenda"]);
        assert_eq!(
            found("https://a.org, https://b.org; https://c.org!"),
            ["https://a.org", "https://b.org", "https://c.org"]
        );
        assert_eq!(found("Really https://example.org/?"), ["https://example.org/"]);
        assert_eq!(found("'https://example.org/x'"), ["https://example.org/x"]);
        assert_eq!(found("\"https://example.org/x\""), ["https://example.org/x"]);
        assert_eq!(found("<https://example.org/x>"), ["https://example.org/x"]);
    }

    #[test]
    fn brackets_close_only_what_the_address_opened() {
        assert_eq!(found("(see https://example.org/page)"), ["https://example.org/page"]);
        assert_eq!(found("(see https://example.org/page)."), ["https://example.org/page"]);
        assert_eq!(found("[https://example.org]"), ["https://example.org"]);
        assert_eq!(
            found("Read https://en.wikipedia.org/wiki/Rust_(language) first"),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
        assert_eq!(
            found("(https://en.wikipedia.org/wiki/Rust_(language))"),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
    }

    #[test]
    fn only_web_and_mail_addresses_at_the_start_of_a_word_are_links() {
        assert_eq!(found("HTTPS://Example.org/Path"), ["HTTPS://Example.org/Path"]);
        assert_eq!(found("Write to mailto:anna@example.org"), ["mailto:anna@example.org"]);
        assert_eq!(found("file:///etc/passwd ftp://x.org javascript:alert(1) tel:+491234"), Vec::<&str>::new());
        assert_eq!(found("xhttps://example.org"), Vec::<&str>::new(), "inside a word");
        assert_eq!(found("http:// and mailto: alone"), Vec::<&str>::new());
        assert_eq!(found("http://, then https://ok.org"), ["https://ok.org"]);
    }

    #[test]
    fn links_lists_each_address_once_in_order() {
        let text = "Zoom: https://zoom.us/j/1\nAgenda https://example.org then again https://zoom.us/j/1.";
        assert_eq!(links(text), ["https://zoom.us/j/1", "https://example.org"]);
    }

    #[test]
    fn only_http_https_and_mailto_may_be_opened() {
        for ok in ["https://example.org", "HTTP://example.org", "mailto:anna@example.org"] {
            assert!(openable(ok), "{ok}");
        }
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.org",
            "smb://server/share",
            "https://",
            "mailto:",
            "https://example.org/a b",
            "https://example.org/\u{7}",
            "example.org",
            "",
        ] {
            assert!(!openable(refused), "{refused:?}");
        }
    }

    #[test]
    fn meeting_links_name_their_service() {
        let cases = [
            ("https://us02web.zoom.us/j/8123456789?pwd=abc", Some("Zoom")),
            ("https://zoom.us/j/1", Some("Zoom")),
            ("https://teams.microsoft.com/l/meetup-join/19%3a", Some("Microsoft Teams")),
            ("https://teams.live.com/meet/123", Some("Microsoft Teams")),
            ("https://meet.google.com/abc-defg-hij", Some("Google Meet")),
            ("https://acme.webex.com/meet/anna", Some("Webex")),
            ("https://meet.jit.si/TorroStandup", Some("Jitsi Meet")),
            ("https://cloud.example.org/call/x7k2p9", Some("Nextcloud Talk")),
            ("https://cloud.example.org/index.php/call/x7k2p9", Some("Nextcloud Talk")),
            ("https://cloud.example.org:8443/call/x7k2p9", Some("Nextcloud Talk")),
            ("https://example.org/agenda", None),
            ("https://notzoom.us/j/1", None),
            ("https://zoom.us.evil.example/j/1", None),
            ("mailto:anna@zoom.us", None),
            ("javascript://zoom.us/j/1", None),
        ];
        for (url, service) in cases {
            assert_eq!(meeting(url), service, "{url}");
        }
    }
}
