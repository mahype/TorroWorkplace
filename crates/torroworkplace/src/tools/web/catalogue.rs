//! What each search provider of the tool `web` is, for `tools.list_search_providers` (task WEB.2; the setup and
//! Assistant › Settings › Tools show the same): where the search words go (host and seat), the terms that apply,
//! what to know before switching it on and whether it needs a key of the person's own. Changes here are changes of
//! what people are told: check them against the provider's own pages.

use chrono::{DateTime, Utc};
use torro_core::i18n::tr;
use torro_core::settings::WebSearch;
use torro_ipc::web::{KeyState, SearchProviderId, SearchProviderInfo};

use super::providers::{brave, duckduckgo, exa, mojeek, tavily};

/// The fixed facts of one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The provider.
    pub id: SearchProviderId,
    /// The host the words go to; empty for SearXNG, whose host is the person's address.
    pub host: &'static str,
    /// Where the provider has its seat, in English (shown through `tr`); empty for SearXNG, the person's own.
    pub seat: &'static str,
    /// The address of its terms of use or service; empty for SearXNG.
    pub terms: &'static str,
    /// It needs a key of the person's own (`accounts.set_search_key`).
    pub needs_key: bool,
}

/// The facts of `id` (checked 2026-10-24 on the providers' pages).
pub fn entry(id: SearchProviderId) -> Entry {
    let (host, seat, terms, needs_key) = match id {
        SearchProviderId::Searxng => ("", "", "", false),
        SearchProviderId::Brave => {
            (host_of(brave::BASE), "USA", "https://api-dashboard.search.brave.com/terms-of-service", true)
        }
        SearchProviderId::Tavily => (host_of(tavily::BASE), "USA", "https://www.tavily.com/terms", true),
        SearchProviderId::Exa => (host_of(exa::BASE), "USA", "https://exa.ai/terms", true),
        SearchProviderId::Mojeek => {
            (host_of(mojeek::BASE), "United Kingdom", "https://www.mojeek.com/about/terms.html", true)
        }
        SearchProviderId::Duckduckgo => (host_of(duckduckgo::BASE), "USA", "https://duckduckgo.com/terms", false),
        SearchProviderId::Wikipedia => {
            ("wikipedia.org", "USA", "https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use", false)
        }
    };
    Entry { id, host, seat, terms, needs_key }
}

/// `https://host` without its scheme.
fn host_of(base: &'static str) -> &'static str {
    base.strip_prefix("https://").unwrap_or(base)
}

/// What a person should know before switching `id` on, in the suite's language; `None` for nothing beyond the
/// facts. DuckDuckGo's words are fixed by the concept (`docs/assistent.md`, "Werkzeug Web").
pub fn notice(id: SearchProviderId) -> Option<String> {
    let text = match id {
        SearchProviderId::Searxng => {
            tr("Your own SearXNG instance. Torro checks that it answers in JSON when you enter its address.")
        }
        SearchProviderId::Brave => {
            tr("Brave allows its results to be used for the answer only, not stored. Torro does not keep them.")
        }
        SearchProviderId::Duckduckgo => {
            tr("DuckDuckGo offers no official interface for this. Torro reads the results page like a browser. \
             DuckDuckGo may block this at any time.")
        }
        SearchProviderId::Wikipedia => {
            tr("Asked in addition to the others. Its text is under CC BY-SA 4.0, named at every result.")
        }
        SearchProviderId::Tavily | SearchProviderId::Exa | SearchProviderId::Mojeek => return None,
    };
    Some(text.to_string())
}

/// `id` as `tools.list_search_providers` lists it, with the person's settings `web` (switch, SearXNG address),
/// whether its key is stored (`key_stored`, asked of the keychain only for providers that need one) and the rest
/// the router knows of (`router::Router::paused_until`). SearXNG's `sends_to` is the host of its address, empty
/// while none is set; its `address` is that address. Wikipedia's words go to the language's site
/// (`de.wikipedia.org`), named as `wikipedia.org`.
pub fn info(
    id: SearchProviderId,
    web: &WebSearch,
    key_stored: bool,
    paused_until: Option<DateTime<Utc>>,
) -> SearchProviderInfo {
    let entry = entry(id);
    let address = (id == SearchProviderId::Searxng).then(|| web.searxng.clone()).flatten();
    let sends_to = match &address {
        Some(address) => reqwest::Url::parse(address).ok().and_then(|url| url.host_str().map(str::to_string)),
        None => Some(entry.host.to_string()),
    };
    let key = match (entry.needs_key, key_stored) {
        (false, _) => KeyState::NotNeeded,
        (true, true) => KeyState::Stored,
        (true, false) => KeyState::Missing,
    };
    SearchProviderInfo {
        id,
        name: id.title().to_string(),
        sends_to: sends_to.unwrap_or_default(),
        seat: if entry.seat.is_empty() { String::new() } else { seat(entry.seat).to_string() },
        terms: entry.terms.to_string(),
        notice: notice(id),
        needs_key: entry.needs_key,
        key,
        address,
        enabled: web.is_on(id),
        paused_until,
    }
}

/// A seat of [`entry`] in the suite's language.
fn seat(english: &'static str) -> &'static str {
    match english {
        "USA" => tr("USA"),
        "United Kingdom" => tr("United Kingdom"),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_provider_says_where_the_words_go() {
        for id in SearchProviderId::ORDER {
            let entry = entry(id);
            if id == SearchProviderId::Searxng {
                assert_eq!((entry.host, entry.seat, entry.terms), ("", "", ""), "the person's own");
                continue;
            }
            assert!(!entry.host.is_empty() && !entry.host.contains('/'), "{id:?}: {}", entry.host);
            assert!(!entry.seat.is_empty(), "{id:?}");
            assert!(entry.terms.starts_with("https://"), "{id:?}: {}", entry.terms);
        }
        let keyed: Vec<_> = SearchProviderId::ORDER.into_iter().filter(|id| entry(*id).needs_key).collect();
        assert_eq!(
            keyed,
            [SearchProviderId::Brave, SearchProviderId::Tavily, SearchProviderId::Exa, SearchProviderId::Mojeek]
        );
        assert_eq!(entry(SearchProviderId::Duckduckgo).host, "html.duckduckgo.com");
        assert_eq!(entry(SearchProviderId::Brave).host, "api.search.brave.com");
    }

    #[test]
    fn duckduckgo_and_brave_warn_in_the_concepts_words() {
        let duckduckgo = notice(SearchProviderId::Duckduckgo).expect("a notice");
        assert_eq!(
            duckduckgo,
            tr("DuckDuckGo offers no official interface for this. Torro reads the results page like a browser. \
                DuckDuckGo may block this at any time.")
        );
        assert!(notice(SearchProviderId::Brave).is_some());
        assert!(notice(SearchProviderId::Tavily).is_none());
    }

    #[test]
    fn info_tells_switch_key_address_and_rest() {
        let web = WebSearch {
            providers: BTreeSet::from([SearchProviderId::Searxng, SearchProviderId::Brave]),
            searxng: Some("http://192.168.1.20:8080/searx".into()),
        };
        let searxng = info(SearchProviderId::Searxng, &web, false, None);
        assert_eq!(
            (searxng.sends_to.as_str(), searxng.address.as_deref(), searxng.key, searxng.enabled),
            ("192.168.1.20", Some("http://192.168.1.20:8080/searx"), KeyState::NotNeeded, true)
        );
        assert_eq!(info(SearchProviderId::Searxng, &WebSearch::default(), false, None).sends_to, "");
        let until = Utc::now();
        let brave = info(SearchProviderId::Brave, &web, false, Some(until));
        assert_eq!(
            (brave.key, brave.needs_key, brave.paused_until, brave.address),
            (KeyState::Missing, true, Some(until), None)
        );
        assert_eq!(brave.name, "Brave");
        assert_eq!(brave.seat, tr("USA"));
        assert_eq!(info(SearchProviderId::Brave, &web, true, None).key, KeyState::Stored);
        let mojeek = info(SearchProviderId::Mojeek, &web, false, None);
        assert_eq!((mojeek.enabled, mojeek.seat.as_str()), (false, tr("United Kingdom")));
    }
}
