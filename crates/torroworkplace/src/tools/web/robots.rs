//! A small reader of `robots.txt` after RFC 9309 for the tool web (task WEB.3, decision 23 in `docs/assistent.md`).
//! It holds for every page the model picks itself; an address a person wrote is read like a browser would
//! (`fetch::Fetcher::fetch`, `by_person`).
//!
//! Our own instead of `texting_robots` (old dependencies). What it does, after the RFC:
//!
//! - **Groups:** one or more `user-agent` lines and the rules below them. The groups naming [`AGENT`] (case does not
//!   matter, `TorroWorkplace/1.0` counts) hold, all of them together; else the `*` groups; else nothing is ruled.
//!   A group naming [`AGENT`] without rules allows everything (RFC 9309 §5.1, `quxbot`).
//! - **Rules:** `allow` and `disallow` with `*` (any octets) and a final `$` (the end of the path). The longest
//!   matching rule wins, in octets; `allow` wins a tie. Nothing matches: allowed. `/robots.txt` is always allowed.
//!   Paths and rules are compared percent-encoded, with escaped unreserved characters decoded (`%62` is `b`).
//! - **Limits:** at most [`MAX_BYTES`] are read (the rest is ignored, as §2.5 allows); answers stay
//!   [`KEEP_FOR`] in memory per origin ([`RobotsCache`]), never on disk.
//! - **Status:** 2xx is read; other 4xx allow everything (§2.3.1.3); 5xx, 429 and an unreachable server allow
//!   nothing (§2.3.1.4; 429 counts as a server that cannot take us now, as Google does).
//!
//! This file is pure: the network part (fetching `robots.txt` through the same guards as the page, at most five
//! redirects) is in `fetch.rs`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The product token Torro gives in `robots.txt` matching (the start of `places::USER_AGENT`).
pub const AGENT: &str = "TorroWorkplace";

/// The most bytes of a `robots.txt` read; RFC 9309 §2.5 asks for at least 500 KiB.
pub const MAX_BYTES: usize = 500 * 1024;

/// How long an answer about an origin is kept.
pub const KEEP_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// How many origins are kept at most; the oldest goes first.
pub const MAX_ORIGINS: usize = 512;

/// What a site's `robots.txt` says for [`AGENT`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Robots {
    /// Every path may be read (no `robots.txt`, a 4xx, or no rule for us).
    AllowAll,
    /// No path may be read (a 5xx or a server that could not be reached).
    DisallowAll,
    /// The rules of the groups for us; a path no rule matches is allowed.
    Rules(Vec<Rule>),
}

/// One `allow` or `disallow` line, its pattern normalised ([`normalised`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// `true` for `allow`.
    pub allow: bool,
    /// The path pattern, percent-encoded, with `*` and an optional final `$`.
    pub pattern: String,
}

impl Robots {
    /// What an answer with HTTP `status` and `body` says: 2xx the rules for [`AGENT`] in `body`
    /// ([`Robots::parse`]); 429 and 5xx [`Robots::DisallowAll`]; any other status [`Robots::AllowAll`] (the file is
    /// unavailable). A redirect is no answer here: `fetch.rs` follows it.
    pub fn for_status(status: u16, body: &[u8]) -> Robots {
        match status {
            200..=299 => Robots::parse(body, AGENT),
            429 | 500..=599 => Robots::DisallowAll,
            _ => Robots::AllowAll,
        }
    }

    /// The rules of `body` (a `robots.txt`, at most its first [`MAX_BYTES`]) for the crawler `agent`, after the
    /// module docs: the groups naming `agent`, else the `*` groups. [`Robots::AllowAll`] when no group holds or the
    /// holding groups have no rule.
    pub fn parse(body: &[u8], agent: &str) -> Robots {
        let body = &body[..body.len().min(MAX_BYTES)];
        let text = String::from_utf8_lossy(body);
        let groups = groups(text.trim_start_matches('\u{feff}'));
        let named: Vec<&Group> = groups.iter().filter(|group| group.agents.iter().any(|a| names(a, agent))).collect();
        let holding = if named.is_empty() {
            groups.iter().filter(|group| group.agents.iter().any(|a| a == "*")).collect()
        } else {
            named
        };
        let rules: Vec<Rule> = holding.into_iter().flat_map(|group| group.rules.iter().cloned()).collect();
        if rules.is_empty() { Robots::AllowAll } else { Robots::Rules(rules) }
    }

    /// Whether `path` may be read: the path of the address with its query (`/a/b?c=d`), as `reqwest::Url` gives
    /// them (non-ASCII percent-encoded). `/robots.txt` always may.
    pub fn allows(&self, path: &str) -> bool {
        let rules = match self {
            Robots::AllowAll => return true,
            Robots::DisallowAll => return path == "/robots.txt",
            Robots::Rules(rules) => rules,
        };
        if path == "/robots.txt" {
            return true;
        }
        let path = normalised(if path.is_empty() { "/" } else { path });
        let best = rules
            .iter()
            .filter(|rule| matches(&rule.pattern, &path))
            .max_by_key(|rule| (rule.pattern.len(), rule.allow));
        best.is_none_or(|rule| rule.allow)
    }
}

/// One group of the file: its user agents (in lower case, `*` as is) and its rules.
#[derive(Default)]
struct Group {
    agents: Vec<String>,
    rules: Vec<Rule>,
}

/// The groups of `text`, in order. Lines are `key: value` with `#` comments; a `user-agent` after rules starts a
/// new group, consecutive ones share a group. Rules before any `user-agent`, other keys (`sitemap`,
/// `crawl-delay`) and broken lines are ignored; an empty `allow` or `disallow` rules nothing.
fn groups(text: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut in_rules = false;
    // RFC 9309 §2.2: a line ends with LF, CR or CR LF (CR LF gives an empty line between, which is skipped).
    for line in text.split(['\n', '\r']) {
        let line = line.split_once('#').map_or(line, |(before, _)| before);
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match key.as_str() {
            "user-agent" => {
                if in_rules || groups.is_empty() {
                    groups.push(Group::default());
                    in_rules = false;
                }
                if let Some(group) = groups.last_mut() {
                    group.agents.push(value.to_ascii_lowercase());
                }
            }
            "allow" | "disallow" => {
                let Some(group) = groups.last_mut() else { continue };
                in_rules = true;
                if !value.is_empty() {
                    group.rules.push(Rule { allow: key == "allow", pattern: normalised(value) });
                }
            }
            _ => {}
        }
    }
    groups
}

/// Whether the `user-agent` value `value` (lower case) names `agent`: its product token, the leading letters, `-`
/// and `_` (`torroworkplace/1.0` is `torroworkplace`), equal ignoring case. `*` names no one here.
fn names(value: &str, agent: &str) -> bool {
    let end = value.find(|c: char| !(c.is_ascii_alphabetic() || c == '-' || c == '_')).unwrap_or(value.len());
    let token = &value[..end];
    !token.is_empty() && token.eq_ignore_ascii_case(agent)
}

/// `text` (a path or a rule) for comparing (RFC 9309 §2.2.2): every octet outside printable ASCII
/// percent-encoded, escapes in upper case, and escaped unreserved characters (letters, digits, `-._~`) decoded.
pub fn normalised(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'%'
            && let Some(value) = bytes.get(at + 1..at + 3).and_then(|hex| std::str::from_utf8(hex).ok())
            && let Ok(value) = u8::from_str_radix(value, 16)
        {
            if value.is_ascii_alphanumeric() || matches!(value, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(value));
            } else {
                out.push_str(&format!("%{value:02X}"));
            }
            at += 3;
            continue;
        }
        if byte.is_ascii_graphic() {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
        at += 1;
    }
    out
}

/// Whether the normalised `pattern` matches the start of the normalised `path`: `*` stands for any octets, a final
/// `$` for the end of the path.
fn matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(pattern) => (pattern, true),
        None => (pattern, false),
    };
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or_default();
    let Some(mut rest) = path.strip_prefix(first) else { return false };
    let pieces: Vec<&str> = pieces.collect();
    let Some((last, middle)) = pieces.split_last() else {
        // No `*`: a plain prefix, or the whole path with `$`.
        return !anchored || rest.is_empty();
    };
    // The leftmost place of every middle piece leaves the most room for the ones after it.
    for piece in middle {
        let Some(found) = rest.find(piece) else { return false };
        rest = &rest[found + piece.len()..];
    }
    if anchored { rest.ends_with(last) } else { rest.contains(last) }
}

/// What the tool knows about the `robots.txt` of origins (`https://example.org:443`), each for [`KEEP_FOR`], at
/// most [`MAX_ORIGINS`]. In memory only; `now` is a parameter so tests move time on.
#[derive(Default)]
pub struct RobotsCache {
    kept: HashMap<String, (Instant, Robots)>,
}

impl RobotsCache {
    /// What is kept about `origin`, when it was put less than [`KEEP_FOR`] before `now`.
    pub fn get(&self, origin: &str, now: Instant) -> Option<Robots> {
        let (when, robots) = self.kept.get(origin)?;
        (now.saturating_duration_since(*when) < KEEP_FOR).then(|| robots.clone())
    }

    /// Keeps `robots` for `origin` from `now`; drops what is out of date and, when full, the oldest.
    pub fn put(&mut self, origin: &str, robots: Robots, now: Instant) {
        self.kept.retain(|_, (when, _)| now.saturating_duration_since(*when) < KEEP_FOR);
        if self.kept.len() >= MAX_ORIGINS
            && !self.kept.contains_key(origin)
            && let Some(oldest) = self.kept.iter().min_by_key(|(_, (when, _))| *when).map(|(key, _)| key.clone())
        {
            self.kept.remove(&oldest);
        }
        self.kept.insert(origin.to_string(), (now, robots));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9309 §5.1, "Simple Example".
    const SIMPLE: &str = "User-Agent: *\n\
        Disallow: *.gif$\n\
        Disallow: /example/\n\
        Allow: /publications/\n\
        \n\
        User-Agent: foobot\n\
        Disallow:/\n\
        Allow:/example/page.html\n\
        Allow:/example/allowed.gif\n\
        \n\
        User-Agent: barbot\n\
        User-Agent: bazbot\n\
        Disallow: /example/page.html\n\
        \n\
        User-Agent: quxbot\n";

    fn for_agent(text: &str, agent: &str) -> Robots {
        Robots::parse(text.as_bytes(), agent)
    }

    #[test]
    fn the_simple_example_of_the_rfc() {
        let table: &[(&str, &str, bool)] = &[
            ("foobot", "/example/page.html", true),
            ("foobot", "/example/allowed.gif", true),
            ("foobot", "/example/other.html", false),
            ("foobot", "/", false),
            ("barbot", "/example/page.html", false),
            ("bazbot", "/example/page.html", false),
            ("barbot", "/example/other.html", true),
            ("quxbot", "/example/page.html", true),
            ("quxbot", "/a.gif", true),
            ("otherbot", "/a.gif", false),
            ("otherbot", "/a.gif?x", true),
            ("otherbot", "/example/page.html", false),
            ("otherbot", "/publications/one", true),
            ("otherbot", "/", true),
        ];
        for (agent, path, allowed) in table {
            assert_eq!(for_agent(SIMPLE, agent).allows(path), *allowed, "{agent} {path}");
        }
    }

    #[test]
    fn the_longest_match_wins_as_in_the_rfc() {
        // RFC 9309 §5.2, "Longest Match".
        let robots =
            for_agent("User-Agent: foobot\nAllow: /example/page/\nDisallow: /example/page/disallowed.gif\n", "foobot");
        assert!(robots.allows("/example/page/"));
        assert!(robots.allows("/example/page/other.gif"));
        assert!(!robots.allows("/example/page/disallowed.gif"));
        // Equal length: allow wins.
        let tie = for_agent("User-agent: *\nDisallow: /same\nAllow: /same\n", AGENT);
        assert!(tie.allows("/same/page"));
        // A longer wildcard rule beats a shorter plain one, by octets.
        let wild = for_agent("User-agent: *\nAllow: /shop\nDisallow: /shop/*/cart\n", AGENT);
        assert!(!wild.allows("/shop/de/cart"));
        assert!(wild.allows("/shop/de/list"));
    }

    #[test]
    fn special_characters_as_in_the_rfc() {
        // RFC 9309 §2.2.3: `#` starts a comment, `$` ends the path, `*` stands for anything.
        let robots = for_agent(
            "User-agent: * # all of them\n\
             Disallow: /\n\
             Allow: /this/path/exactly$ # only this\n\
             Allow: /that/*/exactly\n",
            AGENT,
        );
        assert!(robots.allows("/this/path/exactly"));
        assert!(!robots.allows("/this/path/exactly/not"));
        assert!(robots.allows("/that/is/exactly/more"));
        assert!(!robots.allows("/that/is/not"));
        let end = for_agent("User-agent: *\nDisallow: /*.php$\n", AGENT);
        assert!(!end.allows("/index.php"));
        assert!(!end.allows("/a/b.php"));
        assert!(end.allows("/index.php?x=1"));
        assert!(end.allows("/index.php5"));
    }

    #[test]
    fn paths_are_compared_percent_encoded_as_in_the_rfc() {
        // RFC 9309 §2.2.2, the table of encodings.
        let table: &[(&str, &str)] = &[
            ("/foo/bar?baz=quz", "/foo/bar?baz=quz"),
            ("/foo/bar/\u{30c4}", "/foo/bar/%E3%83%84"),
            ("/foo/bar/%E3%83%84", "/foo/bar/%E3%83%84"),
            ("/foo/bar/%62%61%7A", "/foo/bar/baz"),
        ];
        for (rule, path) in table {
            let robots = for_agent(&format!("User-agent: *\nDisallow: {rule}\n"), AGENT);
            assert!(!robots.allows(path), "{rule} matches {path}");
        }
        assert_eq!(normalised("/a%2fb%7e%e3"), "/a%2Fb~%E3");
    }

    #[test]
    fn groups_are_found_by_our_product_token() {
        let text = "User-agent: *\nDisallow: /\n\nUser-agent: torroworkplace/1.0\nDisallow: /private\n";
        let robots = Robots::parse(text.as_bytes(), AGENT);
        assert!(robots.allows("/public"), "our group, not the one for all");
        assert!(!robots.allows("/private/x"));
        // Two groups for us count together.
        let two = "User-agent: TorroWorkplace\nDisallow: /a\n\nUser-agent: other\nDisallow: /\n\n\
                   User-agent: TORROWORKPLACE\nDisallow: /b\n";
        let robots = Robots::parse(two.as_bytes(), AGENT);
        assert!(!robots.allows("/a") && !robots.allows("/b") && robots.allows("/c"));
        // A similar name is someone else.
        let similar = "User-agent: TorroWorkplaceBot\nDisallow: /\n";
        assert_eq!(Robots::parse(similar.as_bytes(), AGENT), Robots::AllowAll);
    }

    #[test]
    fn odd_files_rule_nothing_they_do_not_say() {
        for text in [
            "",
            "Disallow: /\nUser-agent: *\n",
            "Sitemap: https://example.org/sitemap.xml\n",
            "User-agent: *\nDisallow:\n",
            "<html><body>Not found</body></html>",
        ] {
            assert!(Robots::parse(text.as_bytes(), AGENT).allows("/page"), "{text:?}");
        }
        // CRLF, a BOM, odd spacing and case in the keys, a sitemap between the rules.
        let text = "\u{feff}USER-AGENT : *\r\nSitemap: https://example.org/s.xml\r\n  DisAllow :  /secret  \r\n";
        let robots = Robots::parse(text.as_bytes(), AGENT);
        assert!(!robots.allows("/secret/a"));
        assert!(robots.allows("/open"));
        // Lines ending with CR alone (RFC 9309 §2.2; found in the review of PR #311).
        let old_mac = Robots::parse(b"User-agent: *\rDisallow: /secret\rAllow: /secret/open\r", AGENT);
        assert!(!old_mac.allows("/secret/a"));
        assert!(old_mac.allows("/secret/open/a"));
        // `/robots.txt` itself is always allowed.
        assert!(Robots::parse(b"User-agent: *\nDisallow: /\n", AGENT).allows("/robots.txt"));
        assert!(Robots::DisallowAll.allows("/robots.txt"));
    }

    #[test]
    fn only_the_first_500_kib_are_read() {
        let mut text = String::from("User-agent: *\nDisallow: /early\n");
        text.push_str(&"# filler\n".repeat(MAX_BYTES / 9 + 1));
        text.push_str("Disallow: /late\n");
        let robots = Robots::parse(text.as_bytes(), AGENT);
        assert!(!robots.allows("/early"));
        assert!(robots.allows("/late"), "beyond the limit");
    }

    #[test]
    fn the_status_decides_before_the_body() {
        let body = b"User-agent: *\nDisallow: /\n";
        assert!(!Robots::for_status(200, body).allows("/a"));
        for unavailable in [400, 401, 403, 404, 410] {
            assert_eq!(Robots::for_status(unavailable, body), Robots::AllowAll, "{unavailable}");
        }
        for unreachable in [429, 500, 502, 503] {
            assert_eq!(Robots::for_status(unreachable, b""), Robots::DisallowAll, "{unreachable}");
        }
        assert!(!Robots::DisallowAll.allows("/"));
    }

    #[test]
    fn answers_are_kept_a_day_per_origin() {
        let start = Instant::now();
        let mut cache = RobotsCache::default();
        cache.put("https://example.org", Robots::DisallowAll, start);
        assert_eq!(cache.get("https://example.org", start + Duration::from_secs(60)), Some(Robots::DisallowAll));
        assert_eq!(cache.get("http://example.org", start), None, "another origin");
        assert_eq!(cache.get("https://example.org", start + KEEP_FOR), None, "out of date");
        let mut cache = RobotsCache::default();
        for n in 0..=MAX_ORIGINS {
            cache.put(&format!("https://{n}.example.org"), Robots::AllowAll, start + Duration::from_secs(n as u64));
        }
        assert_eq!(cache.kept.len(), MAX_ORIGINS);
        assert_eq!(cache.get("https://0.example.org", start + Duration::from_secs(1000)), None, "the oldest went");
    }
}
