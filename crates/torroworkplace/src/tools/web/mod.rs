//! The tool `web` in the core (issue #305, `docs/assistent.md` "Werkzeug Web"): searching the web with the
//! providers a person switched on, and (task WEB.3) reading one page.
//!
//! - [`SearchProvider`]: one search service, with a pure `parse` of its answer per provider
//!   ([`providers`]) and a thin HTTP part.
//! - [`router::Router`]: asks the switched-on providers in [`SearchProviderId::ORDER`], falls back on a block, a
//!   limit or a missing key, adds Wikipedia's hits, throttles, pauses and keeps answers an hour in memory.
//! - [`catalogue`]: what each provider is, where the words go and on what terms, for `tools.list_search_providers`.
//! - [`fetch::Fetcher`]: reads one page (task WEB.3), only from the public internet, as Markdown; with
//!   [`robots`], a reader of `robots.txt` after RFC 9309, for the addresses the model picks.
//! - [`Web`] (task WEB.4): the one router, the one page reader and the providers' keys of the service, with the
//!   handlers of `tools.web_search`, `tools.fetch_page`, `tools.list_search_providers`, `accounts.set_search_key`
//!   and the check of a new SearXNG address. The core checks switch, rights and selection before.
//!
//! Nothing here logs or keeps search words, addresses or a key in an error: errors carry fixed texts only
//! ([`SearchError`], [`fetch::FetchError`]), and no answer holds a key ([`SearchKeys`]).

pub mod catalogue;
pub mod fetch;
pub mod providers;
pub mod robots;
pub mod router;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use torro_core::account::Secret;
use torro_core::ai::BoxFuture;
use torro_core::i18n::{tr, trf};
use torro_core::settings::WebSearch;
use torro_ipc::Response;
use torro_ipc::web::{MAX_HITS, MAX_QUERY_CHARS, MAX_URL_BYTES, SearchHit, SearchProviderId};

use fetch::{FetchError, Fetcher};
use router::{NoAnswer, Router};

/// One search as the providers get it. Build it with [`Query::new`], which checks the count and the
/// language; the core has checked the words (`tools.web_search`: not blank, at most `MAX_QUERY_CHARS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The search words, trimmed. They go to a provider outside the suite: never log them.
    pub words: String,
    /// How many hits at most, 1 to [`MAX_HITS`].
    pub count: usize,
    /// The language of the hits, an ISO 639-1 code in lower case (`de`, `en`); always two letters, so a
    /// provider may put it in a host name (Wikipedia: `de.wikipedia.org`).
    pub language: String,
}

impl Query {
    /// The search for `words` (trimmed) with at most `count` hits (`None`: 5; clamped to 1 to [`MAX_HITS`]) in
    /// `language`: the given ISO 639-1 code when it is two ASCII letters (in lower case), else the suite's language
    /// (`en` or `de`).
    pub fn new(words: &str, count: Option<u8>, language: Option<&str>) -> Query {
        let count = count.map_or(DEFAULT_HITS, usize::from).clamp(1, MAX_HITS);
        let language = language
            .map(|code| code.trim().to_ascii_lowercase())
            .filter(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
            .unwrap_or_else(|| suite_language().to_string());
        Query { words: words.trim().to_string(), count, language }
    }
}

/// How many hits a search gives when it names no count (`tools.web_search`).
pub const DEFAULT_HITS: usize = 5;

/// The suite's language as an ISO 639-1 code: `en` or `de`.
pub fn suite_language() -> &'static str {
    match torro_core::i18n::lang() {
        torro_core::i18n::Lang::De => "de",
        torro_core::i18n::Lang::En => "en",
    }
}

/// What a provider can do, for the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The most hits one request may ask for; the provider asks for `min(query.count, max_hits)`.
    pub max_hits: usize,
    /// It takes [`Query::language`] into account (a parameter or a host of that language); a provider without
    /// one finds hits in any language.
    pub languages: bool,
    /// Its answers may be kept in the router's memory for an hour. `false` for Brave, whose terms allow results
    /// only for the answer; the router never keeps Brave's, whatever this says.
    pub cacheable: bool,
}

/// Why a provider gave no hits. Nothing of the request is in it: no words, no address, no key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    /// The provider says: too many requests (HTTP 429, a daily limit, no credits left). The router rests it for
    /// `retry_after` when the provider named a time, else an hour, and asks the next one.
    RateLimited {
        /// How long the provider asked to wait (`Retry-After` in seconds), when it said.
        retry_after: Option<Duration>,
    },
    /// The provider refused the core (a captcha, an anomaly page, 403, DuckDuckGo's 202). Never worked around:
    /// the router rests it for an hour and asks the next one.
    Blocked,
    /// The provider needs a key and none is stored, or it refused the key (401, 403). The router asks the next.
    NeedsKey,
    /// Anything else, with a fixed text for people and logs ([`OFFLINE`], [`BROKEN`]); the router stops there.
    Failed(&'static str),
}

/// [`SearchError::Failed`]: the provider cannot be reached (no connection, a time limit).
pub const OFFLINE: &str = "the search provider cannot be reached";

/// [`SearchError::Failed`]: the provider answered with an error or something that is not its answer.
pub const BROKEN: &str = "the search provider gave no usable answer";

/// One search service. Implementations hold their HTTP client, address and key; they never log the query, the
/// address or the key, and they never retry, change their user agent or work around a block: one request per
/// [`SearchProvider::search`].
pub trait SearchProvider: Send + Sync {
    /// Which provider this is.
    fn id(&self) -> SearchProviderId;

    /// Which instance of the provider this is, where a person may have several: SearXNG's base address. The router
    /// keeps answers per provider and instance, so one instance's answers never come back for another. Empty (the
    /// default) for a provider with one service.
    fn instance(&self) -> &str {
        ""
    }

    /// What it can do.
    fn capabilities(&self) -> Capabilities;

    /// Asks the provider for `query` once: its hits, best first, at most `query.count` (and
    /// [`Capabilities::max_hits`]), each with an absolute `http`/`https` address, a plain-text snippet and
    /// `source` set to [`SearchProvider::id`]. A provider without its key answers [`SearchError::NeedsKey`]
    /// without sending anything.
    fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>>;
}

/// Where the keys of the providers live (`accounts.set_search_key`): the system keychain in the service
/// ([`SystemKeys`]), a map in tests, so tests never touch the person's keychain. Calls block; errors never hold a
/// key.
pub trait SearchKeys: Send + Sync {
    /// The stored key of `provider`; `None` when none is stored.
    fn get(&self, provider: SearchProviderId) -> Result<Option<Secret>, String>;
    /// Stores `key` for `provider`, replacing what was there.
    fn set(&self, provider: SearchProviderId, key: &Secret) -> Result<(), String>;
    /// Removes the key of `provider`; a missing one is fine.
    fn delete(&self, provider: SearchProviderId) -> Result<(), String>;
}

/// The system keychain, under `websearch-<id>` next to the account secrets
/// ([`torro_core::account::search_key`]).
pub struct SystemKeys;

impl SearchKeys for SystemKeys {
    fn get(&self, provider: SearchProviderId) -> Result<Option<Secret>, String> {
        torro_core::account::search_key(provider).map(|key| key.map(Secret::new)).map_err(|err| err.to_string())
    }

    fn set(&self, provider: SearchProviderId, key: &Secret) -> Result<(), String> {
        torro_core::account::set_search_key(provider, key.expose()).map_err(|err| err.to_string())
    }

    fn delete(&self, provider: SearchProviderId) -> Result<(), String> {
        torro_core::account::delete_search_key(provider).map_err(|err| err.to_string())
    }
}

/// The tool `web` of the service (task WEB.4): one [`Router`] and one [`Fetcher`] for the person and every client
/// alike, so rests, turns, kept answers and kept `robots.txt` hold for all; the providers' keys. The HTTP clients
/// are built on first use: every start of the service would pay for them otherwise.
///
/// The handlers answer as the commands' docs in `torro_ipc` say. The core checks the tool's switch, a client's
/// rights and its selection before (`Lack::ToolOff`, `Lack::Tool`); nothing here waits for an approval.
pub struct Web {
    router: Router,
    keys: Arc<dyn SearchKeys>,
    /// The providers' client ([`providers::http_client`]), for searches and the SearXNG check.
    http: OnceLock<Option<reqwest::Client>>,
    /// The page reader ([`Fetcher::system`]).
    fetcher: OnceLock<Option<Fetcher>>,
    /// These providers instead of those the settings name, whatever is on (tests); `None` in the service.
    fixed: Option<Vec<Arc<dyn SearchProvider>>>,
}

impl Web {
    /// The service's: keys in the system keychain, the clients built on first use.
    #[cfg(not(test))]
    pub fn system() -> Web {
        Web::new(Arc::new(SystemKeys))
    }

    /// In tests nothing of the person's and no network: keys in memory, no providers' client, pages read with a
    /// DNS that knows no name ([`testing::offline`]).
    #[cfg(test)]
    pub fn system() -> Web {
        testing::offline()
    }

    /// The tool with the keys in `keys`; its HTTP clients are built on first use.
    pub fn new(keys: Arc<dyn SearchKeys>) -> Web {
        Web { router: Router::new(), keys, http: OnceLock::new(), fetcher: OnceLock::new(), fixed: None }
    }

    /// `self` asking `providers` instead of those the settings build (tests); the switches still decide.
    #[cfg(test)]
    pub fn with_providers(mut self, providers: Vec<Arc<dyn SearchProvider>>) -> Web {
        self.fixed = Some(providers);
        self
    }

    /// `self` with `http` as the providers' client (tests: SearXNG on a mock server).
    #[cfg(test)]
    pub fn with_http(self, http: reqwest::Client) -> Web {
        Web { http: OnceLock::from(Some(http)), ..self }
    }

    /// `self` reading pages with `fetcher` (tests: a fake DNS and a mock server).
    #[cfg(test)]
    pub fn with_fetcher(self, fetcher: Fetcher) -> Web {
        Web { fetcher: OnceLock::from(Some(fetcher)), ..self }
    }

    fn http(&self) -> Option<&reqwest::Client> {
        self.http.get_or_init(providers::http_client).as_ref()
    }

    fn fetcher(&self) -> Option<&Fetcher> {
        self.fetcher.get_or_init(Fetcher::system).as_ref()
    }

    /// `tools.web_search`: searches `words` (at most `count` hits, in `language`) with the providers `settings`
    /// switch on, through the router. Answers [`Response::SearchResults`]; `error` for blank words, more than
    /// [`MAX_QUERY_CHARS`] characters or a line break in them and a `count` above [`MAX_HITS`]; `not-connected`
    /// with no provider on; `busy` while every one that is on rests (naming them and until when) or waits for its
    /// turn; `auth` when only providers without their key are on; `offline` when the provider cannot be reached
    /// or no HTTP client can be built, else `error`. No message holds the words.
    pub async fn search(
        &self,
        settings: &WebSearch,
        words: &str,
        count: Option<u8>,
        language: Option<&str>,
    ) -> Response {
        if let Some(refused) = query_refusal(words, count) {
            return refused;
        }
        let query = Query::new(words, count, language);
        let providers = match self.providers_for(settings).await {
            Ok(providers) => providers,
            Err(refused) => return refused,
        };
        match self.router.search(&providers, &settings.providers, &query).await {
            Ok(results) => Response::SearchResults { results },
            Err(why) => self.no_answer(why, settings),
        }
    }

    /// The providers of a search with `settings`: the fixed ones of a test, else those that are on
    /// ([`providers::for_settings`]) with their keys, read off the threads that serve requests.
    async fn providers_for(&self, settings: &WebSearch) -> Result<Vec<Arc<dyn SearchProvider>>, Response> {
        if let Some(fixed) = &self.fixed {
            return Ok(fixed.clone());
        }
        if settings.askable().is_empty() {
            return Ok(Vec::new());
        }
        let Some(http) = self.http() else {
            return Err(offline(tr("The web cannot be searched: no connection to the internet could be set up.")));
        };
        let needing: Vec<_> = settings.providers.iter().copied().filter(|id| catalogue::entry(*id).needs_key).collect();
        let keys = self.stored_keys(needing).await;
        Ok(providers::for_settings(http, settings, |id| keys.get(&id).map(|key| key.expose().to_string())))
    }

    /// The stored keys of `ids`; a key the keychain cannot give counts as missing.
    async fn stored_keys(&self, ids: Vec<SearchProviderId>) -> HashMap<SearchProviderId, Secret> {
        if ids.is_empty() {
            return HashMap::new();
        }
        let keys = Arc::clone(&self.keys);
        let read = tokio::task::spawn_blocking(move || {
            ids.into_iter().filter_map(|id| Some((id, keys.get(id).ok().flatten()?))).collect()
        });
        read.await.unwrap_or_default()
    }

    /// The answer when the router found nothing to answer with.
    fn no_answer(&self, why: NoAnswer, settings: &WebSearch) -> Response {
        let (code, message) = match why {
            NoAnswer::NoneOn => (
                "not-connected",
                tr("No web search provider is on. A person chooses one in Torro Workplace under MCP servers.")
                    .to_string(),
            ),
            NoAnswer::Resting => ("busy", self.resting(settings)),
            NoAnswer::NeedsKey => (
                "auth",
                tr("The web search providers that are on need a key, and none is stored. \
                    A person adds one in Torro Workplace under MCP servers.")
                .to_string(),
            ),
            NoAnswer::Failed(OFFLINE) => ("offline", tr("The search provider cannot be reached.").to_string()),
            NoAnswer::Failed(_) => ("error", tr("The search provider gave no usable answer.").to_string()),
        };
        Response::Error { code: code.into(), message }
    }

    /// Which of the providers a search can ask ([`WebSearch::askable`]: SearXNG only with an address) rest, and until
    /// when the first of them does; or that they wait for their turn. One that cannot be asked is not named, even
    /// with a rest from before.
    fn resting(&self, settings: &WebSearch) -> String {
        let paused: Vec<_> =
            settings.askable().into_iter().filter_map(|id| Some((id, self.router.paused_until(id)?))).collect();
        let Some(first) = paused.iter().map(|(_, until)| *until).min() else {
            return tr("The web search providers are busy. Try again in a few seconds.").to_string();
        };
        let names: Vec<&str> = paused.iter().map(|(id, _)| id.title()).collect();
        let time = first.with_timezone(&chrono::Local).format("%H:%M").to_string();
        trf(
            "The web search providers that are on rest after a block or a limit: {providers}. Try again after {time}.",
            &[("providers", &names.join(", ")), ("time", &time)],
        )
    }

    /// `tools.fetch_page`: reads the page at `url` ([`Fetcher::fetch`]); `by_person` only when the person wrote the
    /// address themselves (the assistant, task WEB.5), so `robots.txt` holds for every client. Answers
    /// [`Response::WebPage`]; `error` for an address over [`MAX_URL_BYTES`] bytes, `offline` without a page reader, else the code and reason of
    /// [`FetchError`] (`unsupported` for a refused address, `robots.txt`, type or size). No message holds the
    /// address.
    pub async fn fetch(&self, url: &str, by_person: bool) -> Response {
        if url.trim().len() > MAX_URL_BYTES {
            return error(tr(FetchError::TooLong.reason()));
        }
        let Some(fetcher) = self.fetcher() else {
            return offline(tr(FetchError::Offline.reason()));
        };
        match fetcher.fetch(url, by_person).await {
            Ok(page) => Response::WebPage { page },
            Err(err) => Response::Error { code: err.code().into(), message: tr(err.reason()).into() },
        }
    }

    /// What `tools.fetch_page` refuses `url` with before anything goes out (the core asks it before the person's yes
    /// to the web, task WEB.5): an address over [`MAX_URL_BYTES`] bytes (`error`), else the address checks of
    /// [`fetch::checked_url`] with the page reader's guard (`unsupported`: no `http`/`https`, a user name, an IP
    /// address, no public domain, another port). No network: what DNS, `robots.txt` and the page say comes later.
    /// `None` when the address passes.
    pub fn address_refusal(&self, url: &str) -> Option<Response> {
        if url.trim().len() > MAX_URL_BYTES {
            return Some(error(tr(FetchError::TooLong.reason())));
        }
        let guard = self.fetcher().map_or(fetch::Guard::PUBLIC, Fetcher::guard);
        let refused = fetch::checked_url(url, guard).err()?;
        Some(Response::Error { code: refused.code().into(), message: tr(refused.reason()).into() })
    }

    /// `tools.list_search_providers`: every provider in [`SearchProviderId::ORDER`] as [`catalogue::info`] has it,
    /// with the switches and address of `settings`, whether its key is stored (never the key) and its rest.
    /// Without `with_address` (a client that does not reach `web`) SearXNG comes without its address and host:
    /// they often point into the person's own network, and such a client has nothing to search there.
    pub async fn list(&self, settings: &WebSearch, with_address: bool) -> Response {
        let needing = SearchProviderId::ORDER.into_iter().filter(|id| catalogue::entry(*id).needs_key).collect();
        let stored = self.stored_keys(needing).await;
        let providers = SearchProviderId::ORDER
            .into_iter()
            .map(|id| catalogue::info(id, settings, stored.contains_key(&id), self.router.paused_until(id)))
            .map(|mut info| {
                if !with_address && info.address.is_some() {
                    info.address = None;
                    info.sends_to = String::new();
                }
                info
            })
            .collect();
        Response::SearchProviders { providers }
    }

    /// `accounts.set_search_key` (app key only; the core's rights keep every client out): stores `key`, trimmed,
    /// for `provider` in the keychain, or removes the stored one for `None`. [`Response::Ok`]; `error` for a
    /// provider that needs no key, a blank key or a keychain that refuses. No answer holds the key.
    pub async fn set_key(&self, provider: SearchProviderId, key: Option<Secret>) -> Response {
        if !catalogue::entry(provider).needs_key {
            return error(trf("{provider} needs no key.", &[("provider", &provider.title())]));
        }
        let key = match key {
            Some(key) if key.is_blank() => return error(tr("Paste the whole key.")),
            Some(key) => Some(Secret::new(key.expose().trim())),
            None => None,
        };
        let keys = Arc::clone(&self.keys);
        let stored = tokio::task::spawn_blocking(move || match &key {
            Some(key) => keys.set(provider, key),
            None => keys.delete(provider),
        });
        match stored.await {
            Ok(Ok(())) => Response::Ok,
            _ => error(tr("The system keychain did not take the key. Try again.")),
        }
    }

    /// Whether `address`, a new SearXNG address of a person (`settings.set_setting searxng_address`), answers in
    /// JSON: one request `GET <address>/search?q=torro&format=json` with the providers' client (no redirect,
    /// [`providers::MAX_ANSWER_BYTES`] at most); the address may lie in the local network, the person set it.
    /// `Err` with the answer: `unsupported` when no SearXNG answer with `results` comes, `offline` when it cannot be
    /// reached, `busy` when its limiter refuses.
    pub async fn check_searxng(&self, address: &str) -> Result<(), Response> {
        let Some(http) = self.http() else {
            return Err(offline(tr("The SearXNG address cannot be checked: no connection could be set up.")));
        };
        let check = Query::new("torro", Some(1), None);
        match providers::searxng::Searxng::new(http.clone(), address).search(&check).await {
            Ok(_) => Ok(()),
            Err(SearchError::Failed(OFFLINE)) => Err(Response::Error {
                code: "offline".into(),
                message: tr("This SearXNG instance cannot be reached. Check the address.").into(),
            }),
            Err(SearchError::RateLimited { .. }) => Err(Response::Error {
                code: "busy".into(),
                message: tr("This SearXNG instance asks to wait. Try again later.").into(),
            }),
            Err(_) => Err(Response::Error {
                code: "unsupported".into(),
                message: tr("This SearXNG instance does not answer in JSON. Turn on the json format in its settings.")
                    .into(),
            }),
        }
    }
}

/// The `error` for search words or a count `tools.web_search` does not take; `None` when they fit: no
/// words, more than [`MAX_QUERY_CHARS`] characters or a line break in them, a count above
/// [`MAX_HITS`]. [`Web::search`] checks it, and the core before it asks the person for the web's yes
/// (`Core::web_gate`, task WEB.5), so nobody is asked about a search that would fail anyway.
pub(crate) fn query_refusal(words: &str, count: Option<u8>) -> Option<Response> {
    let words = words.trim();
    let message = if words.is_empty() {
        tr("Enter the search words.")
    } else if words.chars().count() > MAX_QUERY_CHARS || words.chars().any(char::is_control) {
        tr("The search words are one line of at most 400 characters.")
    } else if count.is_some_and(|count| usize::from(count) > MAX_HITS) {
        tr("A web search gives at most 10 results.")
    } else {
        return None;
    };
    Some(error(message))
}

/// `offline` with `message`: no HTTP client could be built, so nothing can go out.
fn offline(message: impl Into<String>) -> Response {
    Response::Error { code: "offline".into(), message: message.into() }
}

/// An `error` with `message`.
fn error(message: impl Into<String>) -> Response {
    Response::Error { code: "error".into(), message: message.into() }
}

#[cfg(test)]
pub(crate) mod testing {
    //! What tests of the web, here and in the core, share: keys in memory, a provider that answers what the test
    //! says, and a web that reaches nothing.

    use std::net::IpAddr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use torro_pim_caldav::discover::{Lookup, MxRecord, NoDns, Resolver, SrvRecord, is_public};

    use super::*;

    /// Keys in a map instead of the keychain.
    #[derive(Default)]
    pub struct MemoryKeys(Mutex<HashMap<SearchProviderId, String>>);

    impl MemoryKeys {
        /// The key stored for `provider`.
        pub fn stored(&self, provider: SearchProviderId) -> Option<String> {
            self.0.lock().expect("lock").get(&provider).cloned()
        }
    }

    impl SearchKeys for MemoryKeys {
        fn get(&self, provider: SearchProviderId) -> Result<Option<Secret>, String> {
            Ok(self.stored(provider).map(Secret::new))
        }

        fn set(&self, provider: SearchProviderId, key: &Secret) -> Result<(), String> {
            self.0.lock().expect("lock").insert(provider, key.expose().to_string());
            Ok(())
        }

        fn delete(&self, provider: SearchProviderId) -> Result<(), String> {
            self.0.lock().expect("lock").remove(&provider);
            Ok(())
        }
    }

    /// A provider that answers what the test set and counts how often it was asked; never kept in memory.
    pub struct FakeProvider {
        pub id: SearchProviderId,
        pub answer: Result<Vec<SearchHit>, SearchError>,
        calls: AtomicUsize,
    }

    impl FakeProvider {
        /// `id` answering `answer`.
        pub fn new(id: SearchProviderId, answer: Result<Vec<SearchHit>, SearchError>) -> Arc<FakeProvider> {
            Arc::new(FakeProvider { id, answer, calls: AtomicUsize::new(0) })
        }

        /// `id` finding one page of its own.
        pub fn finding(id: SearchProviderId) -> Arc<FakeProvider> {
            let hit = SearchHit {
                title: format!("{} result", id.title()),
                url: format!("https://{}.example.org/", id.name()),
                snippet: String::new(),
                source: id,
                license: None,
            };
            FakeProvider::new(id, Ok(vec![hit]))
        }

        /// How often it was asked.
        pub fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SearchProvider for FakeProvider {
        fn id(&self) -> SearchProviderId {
            self.id
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities { max_hits: MAX_HITS, languages: true, cacheable: false }
        }

        fn search<'a>(&'a self, _query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let answer = self.answer.clone();
            Box::pin(async move { answer })
        }
    }

    /// A web that reaches nothing: keys in memory, no providers' client, pages read with a DNS that knows no name.
    pub fn offline() -> Web {
        offline_with(Arc::new(MemoryKeys::default()))
    }

    /// [`offline`] with the keys in `keys`, which the test keeps to look into.
    pub fn offline_with(keys: Arc<dyn SearchKeys>) -> Web {
        let fetcher = Fetcher::new(Arc::new(NoDns), fetch::Guard::PUBLIC);
        Web { fetcher: OnceLock::from(fetcher), http: OnceLock::from(None), ..Web::new(keys) }
    }

    /// A DNS that knows a few hosts.
    struct FakeDns(HashMap<String, Vec<IpAddr>>);

    impl Resolver for FakeDns {
        fn srv<'a>(&'a self, _name: &'a str) -> Lookup<'a, SrvRecord> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn txt<'a>(&'a self, _name: &'a str) -> Lookup<'a, String> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn mx<'a>(&'a self, _name: &'a str) -> Lookup<'a, MxRecord> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn ips<'a>(&'a self, host: &'a str) -> Lookup<'a, IpAddr> {
            Box::pin(async move { Ok(self.0.get(host).cloned().unwrap_or_default()) })
        }
    }

    /// The tests' rule: the public internet and, since a mock server lives there, loopback.
    fn public_or_loopback(ip: IpAddr) -> bool {
        is_public(ip) || ip.is_loopback()
    }

    /// A page reader whose DNS points every name of `hosts` at loopback and that accepts a mock server's `port`.
    pub fn fetcher_on(port: u16, hosts: &[&str]) -> Fetcher {
        let loopback = IpAddr::from([127, 0, 0, 1]);
        let table = hosts.iter().map(|host| (host.to_string(), vec![loopback])).collect();
        let guard = fetch::Guard { allowed: public_or_loopback, port: Some(port) };
        Fetcher::new(Arc::new(FakeDns(table)), guard).expect("a page reader")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::testing::{FakeProvider, MemoryKeys, offline, offline_with};
    use super::*;

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("an error, got {other:?}"),
        }
    }

    fn message(response: &Response) -> &str {
        match response {
            Response::Error { message, .. } => message,
            other => panic!("an error, got {other:?}"),
        }
    }

    fn on(ids: &[SearchProviderId]) -> WebSearch {
        WebSearch { providers: ids.iter().copied().collect::<BTreeSet<_>>(), searxng: None }
    }

    #[test]
    fn the_clients_are_built_on_first_use() {
        let web = Web::new(Arc::new(SystemKeys));
        assert!(web.http.get().is_none() && web.fetcher.get().is_none(), "a start of the service pays nothing");
        assert!(web.fixed.is_none());
    }

    #[tokio::test]
    async fn search_words_and_count_are_checked_before_anything_is_asked() {
        let duckduckgo = FakeProvider::finding(SearchProviderId::Duckduckgo);
        let web = offline().with_providers(vec![duckduckgo.clone()]);
        let settings = on(&[SearchProviderId::Duckduckgo]);
        let long = "Lisbon ".repeat(60);
        for (words, count) in [("  ", None), (long.as_str(), None), ("Lisbon\ntrams", None), ("Lisbon", Some(11))] {
            let refused = web.search(&settings, words, count, None).await;
            assert_eq!(code(&refused), "error", "{words:?} {count:?}");
            assert!(!message(&refused).contains("Lisbon"), "no message repeats the words");
        }
        assert_eq!(duckduckgo.calls(), 0);
        let Response::SearchResults { results } = web.search(&settings, "Lisbon", Some(10), Some("pt")).await else {
            panic!("results")
        };
        assert_eq!((results.asked.as_slice(), results.untrusted), (&[SearchProviderId::Duckduckgo][..], true));
    }

    #[tokio::test]
    async fn no_answer_has_its_code() {
        use SearchProviderId::{Brave, Duckduckgo, Tavily};
        let search = |web: Web, settings: WebSearch| async move { web.search(&settings, "Lisbon", None, None).await };
        let none = search(offline(), WebSearch::default()).await;
        assert_eq!(code(&none), "not-connected");
        assert_eq!(
            message(&none),
            tr("No web search provider is on. A person chooses one in Torro Workplace under MCP servers.")
        );
        // Only providers without their key: `auth`.
        let keyless = offline().with_providers(vec![FakeProvider::new(Brave, Err(SearchError::NeedsKey))]);
        assert_eq!(code(&search(keyless, on(&[Brave])).await), "auth");
        // Blocked: `busy`, with the provider and until when it rests.
        let blocked = offline().with_providers(vec![
            FakeProvider::new(Duckduckgo, Err(SearchError::Blocked)),
            FakeProvider::new(Tavily, Err(SearchError::NeedsKey)),
        ]);
        let busy = blocked.search(&on(&[Duckduckgo, Tavily]), "Lisbon", None, None).await;
        assert_eq!(code(&busy), "busy");
        assert!(message(&busy).contains("DuckDuckGo") && !message(&busy).contains("Tavily"), "{busy:?}");
        let until = blocked.router.paused_until(Duckduckgo).expect("it rests");
        let time = until.with_timezone(&chrono::Local).format("%H:%M").to_string();
        assert!(message(&busy).contains(&time), "{busy:?}");
        let offline_provider =
            offline().with_providers(vec![FakeProvider::new(Brave, Err(SearchError::Failed(OFFLINE)))]);
        assert_eq!(code(&search(offline_provider, on(&[Brave])).await), "offline");
        let broken = offline().with_providers(vec![FakeProvider::new(Brave, Err(SearchError::Failed(BROKEN)))]);
        assert_eq!(code(&search(broken, on(&[Brave])).await), "error");
        // A provider of the test that is off is never asked.
        let off = FakeProvider::finding(Brave);
        assert_eq!(code(&search(offline().with_providers(vec![off.clone()]), on(&[Tavily])).await), "not-connected");
        assert_eq!(off.calls(), 0);
    }

    /// Found in the second review of #305: the `busy` message names only providers a search can ask
    /// (`WebSearch::askable`). SearXNG without an address, resting from before, is neither named nor sets the time.
    #[tokio::test]
    async fn the_busy_message_names_only_providers_that_can_be_asked() {
        use SearchProviderId::{Duckduckgo, Searxng};
        let web = offline().with_providers(vec![
            FakeProvider::new(Searxng, Err(SearchError::RateLimited { retry_after: Some(Duration::from_secs(60)) })),
            FakeProvider::new(Duckduckgo, Err(SearchError::Blocked)),
        ]);
        let mut settings = on(&[Searxng, Duckduckgo]);
        settings.searxng = Some("https://search.example.org".into());
        assert_eq!(code(&web.search(&settings, "Lisbon", None, None).await), "busy", "both rest now");
        settings.searxng = None;
        let busy = web.search(&settings, "Lisbon", None, None).await;
        assert_eq!(code(&busy), "busy");
        assert!(message(&busy).contains("DuckDuckGo") && !message(&busy).contains("SearXNG"), "{busy:?}");
        let until = web.router.paused_until(Duckduckgo).expect("it rests");
        let time = until.with_timezone(&chrono::Local).format("%H:%M").to_string();
        assert!(message(&busy).contains(&time), "DuckDuckGo's hour, not SearXNG's minute: {busy:?}");
    }

    #[tokio::test]
    async fn keys_go_to_the_keychain_and_are_never_listed() {
        let keys = Arc::new(MemoryKeys::default());
        let web = offline_with(keys.clone());
        for (provider, key) in [
            (SearchProviderId::Duckduckgo, Some(Secret::new("x"))),
            (SearchProviderId::Searxng, None),
            (SearchProviderId::Brave, Some(Secret::new(" \t"))),
        ] {
            assert_eq!(code(&web.set_key(provider, key).await), "error", "{provider:?}");
        }
        assert_eq!(web.set_key(SearchProviderId::Brave, Some(Secret::new(" BSA_secret\n"))).await, Response::Ok);
        assert_eq!(keys.stored(SearchProviderId::Brave).as_deref(), Some("BSA_secret"), "trimmed");

        let Response::SearchProviders { providers } = web.list(&on(&[SearchProviderId::Brave]), true).await else {
            panic!("the providers")
        };
        let ids: Vec<_> = providers.iter().map(|info| info.id).collect();
        assert_eq!(ids, SearchProviderId::ORDER);
        let brave = &providers[1];
        assert_eq!((brave.key, brave.enabled), (torro_ipc::web::KeyState::Stored, true));
        assert_eq!(providers[2].key, torro_ipc::web::KeyState::Missing);
        assert_eq!(providers[5].key, torro_ipc::web::KeyState::NotNeeded);
        let json = serde_json::to_string(&Response::SearchProviders { providers }).expect("json");
        assert!(!json.contains("BSA_secret"), "{json}");

        assert_eq!(web.set_key(SearchProviderId::Brave, None).await, Response::Ok);
        assert_eq!(keys.stored(SearchProviderId::Brave), None, "removed");
        assert_eq!(web.set_key(SearchProviderId::Brave, None).await, Response::Ok, "removing none is fine");
    }

    #[tokio::test]
    async fn a_searxng_address_must_answer_in_json() {
        let server = MockServer::start_async().await;
        let json = server
            .mock_async(|when, then| {
                when.method(GET).path("/json/search").query_param("q", "torro").query_param("format", "json");
                then.status(200).header("content-type", "application/json").body(r#"{"results":[]}"#);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/html/search");
                then.status(200).header("content-type", "text/html").body("<html>SearXNG</html>");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/off/search");
                then.status(403).body("Forbidden");
            })
            .await;
        let moved = server
            .mock_async(|when, then| {
                when.method(GET).path("/moved/search");
                then.status(302).header("location", "/json/search?q=torro&format=json");
            })
            .await;
        let web = offline().with_http(providers::http_client().expect("a client"));
        assert_eq!(web.check_searxng(&server.url("/json")).await, Ok(()));
        assert_eq!(json.calls_async().await, 1, "one request");
        for refused in ["/html", "/off", "/moved"] {
            let answer = web.check_searxng(&server.url(refused)).await.expect_err(refused);
            assert_eq!(code(&answer), "unsupported", "{refused}");
            assert_eq!(
                message(&answer),
                tr("This SearXNG instance does not answer in JSON. Turn on the json format in its settings.")
            );
        }
        assert_eq!((moved.calls_async().await, json.calls_async().await), (1, 1), "no redirect followed");
    }

    /// Found in the reviews of #305: answers of SearXNG are kept for its address. Another address is another
    /// instance, asked anew; each address again answers from memory, the old one too (its answer is under its own
    /// address, not dropped when the address changed).
    #[tokio::test]
    async fn another_searxng_address_is_asked_anew() {
        let server = MockServer::start_async().await;
        let mut instances = Vec::new();
        for name in ["old", "new"] {
            let answer = format!(r#"{{"results":[{{"title":"{name}","url":"https://{name}.example.org/"}}]}}"#);
            let mock = server
                .mock_async(|when, then| {
                    when.method(GET).path(format!("/{name}/search"));
                    then.status(200).header("content-type", "application/json").body(answer);
                })
                .await;
            instances.push(mock);
        }
        let web = offline().with_http(providers::http_client().expect("a client"));
        let at = |name: &str| WebSearch {
            providers: BTreeSet::from([SearchProviderId::Searxng]),
            searxng: Some(server.url(format!("/{name}"))),
        };
        let titles = |answer: Response| match answer {
            Response::SearchResults { results } => results.hits.into_iter().map(|hit| hit.title).collect::<Vec<_>>(),
            other => panic!("results, got {other:?}"),
        };
        assert_eq!(titles(web.search(&at("old"), "Lisbon", None, None).await), ["old"]);
        assert_eq!(titles(web.search(&at("new"), "Lisbon", None, None).await), ["new"], "not the old one's");
        assert_eq!(titles(web.search(&at("new"), "Lisbon", None, None).await), ["new"]);
        assert_eq!(titles(web.search(&at("old"), "Lisbon", None, None).await), ["old"]);
        assert_eq!((instances[0].calls_async().await, instances[1].calls_async().await), (1, 1), "then from memory");
    }

    #[tokio::test]
    async fn pages_are_refused_with_a_reason_and_the_right_code() {
        let web = offline();
        let long = format!("https://example.org/{}", "a".repeat(MAX_URL_BYTES));
        assert_eq!(code(&web.fetch(&long, false).await), "error", "too long, as the request says");
        for (url, why) in [
            ("https://10.0.0.1/", FetchError::IpAddress),
            ("http://nas.local/", FetchError::NoDomain),
            ("ftp://example.org/", FetchError::NotWeb),
            ("https://user:pass@example.org/", FetchError::Credentials),
            ("https://example.org:8080/", FetchError::Port),
        ] {
            let refused = web.fetch(url, true).await;
            assert_eq!((code(&refused), message(&refused)), ("unsupported", tr(why.reason())), "{url}");
        }
    }

    #[test]
    fn a_query_keeps_count_and_language_in_bounds() {
        let query = Query::new("  Lisbon trams ", None, Some("PT"));
        assert_eq!(query, Query { words: "Lisbon trams".into(), count: DEFAULT_HITS, language: "pt".into() });
        assert_eq!(Query::new("x", Some(0), None).count, 1);
        assert_eq!(Query::new("x", Some(99), None).count, MAX_HITS);
        for odd in ["evil.example.org/", "d", "deu", "d1", ""] {
            assert_eq!(Query::new("x", None, Some(odd)).language, suite_language(), "{odd:?} is no language");
        }
    }
}
