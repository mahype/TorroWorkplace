//! Which provider a web search goes to (task WEB.2, decisions 21 and 24 in `docs/assistent.md`). One [`Router`] for
//! the whole service, so its rests and its throttle hold for the person and every client alike:
//!
//! - **Order and fallback:** the switched-on providers in [`SearchProviderId::ORDER`]; on a limit, a block or a
//!   missing key the next one, on any other failure none (the failure is the answer). Search words go only to
//!   switched-on providers.
//! - **Wikipedia besides:** never a fallback; asked after the others for at most [`WIKIPEDIA_HITS`] hits, which
//!   come after theirs, all within the count asked for (the others keep at least half of it).
//! - **Throttle** ([`gap`]): DuckDuckGo once in ten seconds, every other provider once a second. A search waits
//!   for the next free turn when only one is ahead of it; with more it passes the provider by as resting.
//! - **Rest:** a provider that blocked the core or hit a limit is not asked for an hour ([`PAUSE`]), or as long as
//!   its `Retry-After` said (at most [`MAX_PAUSE`]), by anyone.
//! - **Memory:** answers stay an hour ([`ANSWER_FOR`]), at most [`MAX_ANSWERS`], in memory only and never Brave's
//!   (its terms allow results only for the answer). An answer from memory asks no provider and costs no turn;
//!   [`Router::forget`] drops a provider's (the tool `web` does when SearXNG's address changes).
//!
//! Nothing here logs: errors are [`NoAnswer`] without the words.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use torro_ipc::web::{SearchHit, SearchProviderId, SearchResults};

use super::{Query, SearchError, SearchProvider};

/// How long a provider rests after it blocked the core or hit a limit without naming a time.
pub const PAUSE: Duration = Duration::from_secs(60 * 60);

/// The longest rest a provider's `Retry-After` can ask for; a longer one is cut to this.
pub const MAX_PAUSE: Duration = Duration::from_secs(24 * 60 * 60);

/// How long an answer is kept in memory.
pub const ANSWER_FOR: Duration = Duration::from_secs(60 * 60);

/// How many answers are kept; the oldest goes first.
pub const MAX_ANSWERS: usize = 64;

/// How many of Wikipedia's hits come besides the others'.
pub const WIKIPEDIA_HITS: usize = 3;

/// The least time between two requests to `provider`: DuckDuckGo, which has no interface for this, ten seconds
/// (a few a minute); every other one a second.
pub fn gap(provider: SearchProviderId) -> Duration {
    match provider {
        SearchProviderId::Duckduckgo => Duration::from_secs(10),
        _ => Duration::from_secs(1),
    }
}

/// Why a search found nothing to answer with, for the codes of `tools.web_search`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoAnswer {
    /// No provider is switched on (`not-connected`).
    NoneOn,
    /// Every switched-on provider that has its key rests after a block or limit, or waits for its turn (`busy`).
    Resting,
    /// Only providers without (a valid) key are on (`auth`).
    NeedsKey,
    /// A provider failed with this fixed text (`offline` for [`super::OFFLINE`], else `error`).
    Failed(&'static str),
}

/// Where the router takes "now" from; a parameter so tests move time on.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The router of the service: rests, turns and kept answers of every provider.
pub struct Router {
    clock: Clock,
    gates: Mutex<HashMap<SearchProviderId, Gate>>,
    /// Answers by [`answer_key`], with when they came.
    answers: Mutex<HashMap<String, (Instant, Vec<SearchHit>)>>,
}

impl Default for Router {
    fn default() -> Router {
        Router::new()
    }
}

/// Why one provider gave no hits, for the router's next step.
enum Skip {
    /// It rests or waits for its turn: ask the next.
    Resting,
    /// It lacks its key: ask the next.
    NeedsKey,
    /// It failed: stop.
    Failed(&'static str),
}

impl Router {
    /// A router with the system's clock and nothing kept.
    pub fn new() -> Router {
        Router::with_clock(Arc::new(Instant::now))
    }

    /// A router reading the time from `clock` (tests).
    pub fn with_clock(clock: Clock) -> Router {
        Router { clock, gates: Mutex::new(HashMap::new()), answers: Mutex::new(HashMap::new()) }
    }

    /// Searches `query` with the providers among `providers` that `on` names (the person's switches,
    /// `WebSearch::providers`), in [`SearchProviderId::ORDER`] whatever order `providers` has: the first one that
    /// answers gives up to `query.count` hits; a limit, a block or a missing key goes on to the next, any other
    /// failure ends the search. Wikipedia, when on, is asked besides for [`WIKIPEDIA_HITS`] hits that come after the
    /// others' (without addresses they already have); without other hits its own are the answer, and when it is
    /// the only provider on it is asked for `query.count`. All together at most `query.count` (so at most
    /// [`torro_ipc::web::MAX_HITS`]): the others keep at least half of it, Wikipedia fills what they leave free, up to its three.
    ///
    /// [`SearchResults::asked`] names, in order, every provider that searched or answered from memory, not one that
    /// lacked its key. `Err` when no hits could be had: [`NoAnswer::NoneOn`] with nothing on, else the failure that
    /// ended the search, else [`NoAnswer::Resting`] when a provider rested, else [`NoAnswer::NeedsKey`].
    pub async fn search(
        &self,
        providers: &[Arc<dyn SearchProvider>],
        on: &BTreeSet<SearchProviderId>,
        query: &Query,
    ) -> Result<SearchResults, NoAnswer> {
        let chosen = |id: SearchProviderId| providers.iter().find(|p| p.id() == id).filter(|_| on.contains(&id));
        let main: Vec<_> = SearchProviderId::ORDER
            .into_iter()
            .filter(|id| *id != SearchProviderId::Wikipedia)
            .filter_map(chosen)
            .collect();
        let wikipedia = chosen(SearchProviderId::Wikipedia);
        if main.is_empty() && wikipedia.is_none() {
            return Err(NoAnswer::NoneOn);
        }
        let alone = main.is_empty();
        let mut asked = Vec::new();
        let mut skips = Vec::new();
        let mut hits = None;
        for provider in main {
            match self.ask(provider.as_ref(), query, &mut asked).await {
                Ok(found) => {
                    hits = Some(found);
                    break;
                }
                Err(skip @ Skip::Failed(_)) => {
                    skips.push(skip);
                    break;
                }
                Err(skip) => skips.push(skip),
            }
        }
        let extra = match wikipedia {
            Some(wikipedia) => {
                // Alone it is the search; besides the others it adds a few.
                let count = if alone { query.count } else { WIKIPEDIA_HITS.min(query.count) };
                let besides = Query { count, ..query.clone() };
                self.ask(wikipedia.as_ref(), &besides, &mut asked).await.map_err(|skip| skips.push(skip)).ok()
            }
            None => None,
        };
        let hits = match (hits, extra) {
            (Some(hits), extra) => together(hits, extra.unwrap_or_default(), query.count),
            (None, Some(mut extra)) if !extra.is_empty() || skips.is_empty() => {
                extra.truncate(query.count);
                extra
            }
            (None, _) => return Err(why(&skips)),
        };
        Ok(SearchResults::new(hits, asked))
    }

    /// Until when `provider` rests after a block or limit; `None` while it may be asked
    /// (`SearchProviderInfo::paused_until`).
    pub fn paused_until(&self, provider: SearchProviderId) -> Option<DateTime<Utc>> {
        let now = (self.clock)();
        let gates = self.gates.lock().unwrap_or_else(PoisonError::into_inner);
        let left = gates.get(&provider)?.paused_until?.checked_duration_since(now).filter(|left| !left.is_zero())?;
        Some(Utc::now() + chrono::TimeDelta::from_std(left).ok()?)
    }

    /// Drops every kept answer of `provider`: what it answered no longer holds (the person's SearXNG moved to
    /// another address, another instance). Rests and turns stay.
    pub fn forget(&self, provider: SearchProviderId) {
        let prefix = format!("{}|", provider.name());
        self.answers.lock().unwrap_or_else(PoisonError::into_inner).retain(|key, _| !key.starts_with(&prefix));
    }

    /// One provider's hits for `query`: from memory when kept, else once from the provider when its turn comes. It
    /// is named in `asked` when it answered from memory or searched, not when it rested or lacked its key.
    async fn ask(
        &self,
        provider: &dyn SearchProvider,
        query: &Query,
        asked: &mut Vec<SearchProviderId>,
    ) -> Result<Vec<SearchHit>, Skip> {
        let id = provider.id();
        let keeps = provider.capabilities().cacheable && id != SearchProviderId::Brave;
        let key = answer_key(id, query);
        if keeps && let Some(hits) = self.kept(&key) {
            asked.push(id);
            return Ok(hits);
        }
        let turn = self
            .gates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(id)
            .or_default()
            .turn((self.clock)(), gap(id));
        match turn {
            Turn::Go => {}
            Turn::After(wait) => tokio::time::sleep(wait).await,
            Turn::Busy | Turn::Paused => return Err(Skip::Resting),
        }
        let answer = provider.search(query).await;
        if !matches!(answer, Err(SearchError::NeedsKey)) {
            asked.push(id);
        }
        match answer {
            Ok(hits) => {
                if keeps {
                    self.keep(key, hits.clone());
                }
                Ok(hits)
            }
            Err(SearchError::RateLimited { retry_after }) => {
                self.pause(id, retry_after.unwrap_or(PAUSE).min(MAX_PAUSE));
                Err(Skip::Resting)
            }
            Err(SearchError::Blocked) => {
                self.pause(id, PAUSE);
                Err(Skip::Resting)
            }
            Err(SearchError::NeedsKey) => Err(Skip::NeedsKey),
            Err(SearchError::Failed(why)) => Err(Skip::Failed(why)),
        }
    }

    /// Nobody asks `provider` for `rest` from now.
    fn pause(&self, provider: SearchProviderId, rest: Duration) {
        let now = (self.clock)();
        self.gates.lock().unwrap_or_else(PoisonError::into_inner).entry(provider).or_default().paused_until =
            Some(now + rest);
    }

    /// A kept answer for `key` younger than [`ANSWER_FOR`].
    fn kept(&self, key: &str) -> Option<Vec<SearchHit>> {
        let now = (self.clock)();
        let answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        let (at, hits) = answers.get(key)?;
        (now.saturating_duration_since(*at) < ANSWER_FOR).then(|| hits.clone())
    }

    /// Keeps `hits` under `key`; at [`MAX_ANSWERS`] the oldest answer goes.
    fn keep(&self, key: String, hits: Vec<SearchHit>) {
        let now = (self.clock)();
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        if answers.len() >= MAX_ANSWERS
            && !answers.contains_key(&key)
            && let Some(oldest) = answers.iter().min_by_key(|(_, (at, _))| *at).map(|(key, _)| key.clone())
        {
            answers.remove(&oldest);
        }
        answers.insert(key, (now, hits));
    }
}

/// The hits of a provider and Wikipedia's besides them, at most `count` together: Wikipedia's come after, without
/// addresses the others have, and at most [`WIKIPEDIA_HITS`] of them. The others keep at least half of `count`
/// (rounded up); what they leave free Wikipedia may fill, up to its three.
fn together(mut hits: Vec<SearchHit>, extra: Vec<SearchHit>, count: usize) -> Vec<SearchHit> {
    hits.truncate(count);
    let extra: Vec<SearchHit> =
        extra.into_iter().filter(|hit| hits.iter().all(|other| other.url != hit.url)).take(WIKIPEDIA_HITS).collect();
    let room = (count / 2).max(count - hits.len()).min(extra.len());
    hits.truncate(count - room);
    hits.extend(extra.into_iter().take(room));
    hits
}

/// Why no hits came, from what each provider said ([`Router::search`]).
fn why(skips: &[Skip]) -> NoAnswer {
    if let Some(Skip::Failed(why)) = skips.iter().find(|skip| matches!(skip, Skip::Failed(_))) {
        return NoAnswer::Failed(why);
    }
    if skips.iter().any(|skip| matches!(skip, Skip::Resting)) { NoAnswer::Resting } else { NoAnswer::NeedsKey }
}

/// What an answer is kept under: provider, language, count and the words (in lower case).
fn answer_key(provider: SearchProviderId, query: &Query) -> String {
    format!("{}|{}|{}|{}", provider.name(), query.language, query.count, query.words.to_lowercase())
}

/// What the gate says to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// Ask now.
    Go,
    /// Ask after this long: one request is ahead.
    After(Duration),
    /// Not now: more requests are ahead.
    Busy,
    /// Not now: the provider rests.
    Paused,
}

/// The throttle and rest of one provider.
#[derive(Debug, Default)]
struct Gate {
    /// When the next request may go; `None` before the first.
    next_free: Option<Instant>,
    /// Until when the provider rests.
    paused_until: Option<Instant>,
}

impl Gate {
    /// Whether a request may go at `now` with `gap` between two, and takes its turn when it may: while resting
    /// [`Turn::Paused`]; with the gap over [`Turn::Go`]; with one request ahead [`Turn::After`] the rest of the gap;
    /// with more ahead [`Turn::Busy`].
    fn turn(&mut self, now: Instant, gap: Duration) -> Turn {
        if self.paused_until.is_some_and(|until| now < until) {
            return Turn::Paused;
        }
        match self.next_free.filter(|free| *free > now) {
            None => {
                self.next_free = Some(now + gap);
                Turn::Go
            }
            Some(free) if free - now <= gap => {
                self.next_free = Some(free + gap);
                Turn::After(free - now)
            }
            Some(_) => Turn::Busy,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use torro_core::ai::BoxFuture;

    use super::*;
    use crate::tools::web::{BROKEN, Capabilities, OFFLINE};

    use SearchProviderId::{Brave, Duckduckgo, Exa, Mojeek, Searxng, Tavily, Wikipedia};

    /// A provider that answers what the test sets and counts how often it was asked.
    struct Fake {
        id: SearchProviderId,
        answer: Mutex<Result<Vec<SearchHit>, SearchError>>,
        calls: AtomicUsize,
        cacheable: bool,
    }

    impl Fake {
        fn new(id: SearchProviderId, answer: Result<Vec<SearchHit>, SearchError>) -> Arc<Fake> {
            Arc::new(Fake { id, answer: Mutex::new(answer), calls: AtomicUsize::new(0), cacheable: true })
        }

        /// A provider finding `n` pages of its own.
        fn finding(id: SearchProviderId, n: usize) -> Arc<Fake> {
            Fake::new(id, Ok(hits(id, n)))
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn answers(&self, answer: Result<Vec<SearchHit>, SearchError>) {
            *self.answer.lock().expect("lock") = answer;
        }
    }

    impl SearchProvider for Fake {
        fn id(&self) -> SearchProviderId {
            self.id
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities { max_hits: 20, languages: true, cacheable: self.cacheable }
        }

        fn search<'a>(&'a self, query: &'a Query) -> BoxFuture<'a, Result<Vec<SearchHit>, SearchError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let answer =
                self.answer.lock().expect("lock").clone().map(|hits| hits.into_iter().take(query.count).collect());
            Box::pin(async move { answer })
        }
    }

    fn hits(id: SearchProviderId, n: usize) -> Vec<SearchHit> {
        (0..n)
            .map(|i| SearchHit {
                title: format!("{} {i}", id.name()),
                url: format!("https://{}.example.org/{i}", id.name()),
                snippet: String::new(),
                source: id,
                license: None,
            })
            .collect()
    }

    /// A clock that moves only when the test says.
    #[derive(Clone)]
    struct Time {
        start: Instant,
        millis: Arc<AtomicU64>,
    }

    impl Time {
        fn new() -> Time {
            Time { start: Instant::now(), millis: Arc::new(AtomicU64::new(0)) }
        }

        fn router(&self) -> Router {
            let time = self.clone();
            Router::with_clock(Arc::new(move || time.start + Duration::from_millis(time.millis.load(Ordering::SeqCst))))
        }

        /// Moves on by `by`.
        fn pass(&self, by: Duration) {
            self.millis.fetch_add(u64::try_from(by.as_millis()).expect("short"), Ordering::SeqCst);
        }
    }

    fn all(fakes: &[&Arc<Fake>]) -> Vec<Arc<dyn SearchProvider>> {
        fakes.iter().map(|fake| Arc::clone(fake) as Arc<dyn SearchProvider>).collect()
    }

    fn on(ids: &[SearchProviderId]) -> BTreeSet<SearchProviderId> {
        ids.iter().copied().collect()
    }

    fn sources(results: &SearchResults) -> Vec<SearchProviderId> {
        results.hits.iter().map(|hit| hit.source).collect()
    }

    #[tokio::test]
    async fn asks_the_first_switched_on_provider_in_the_fixed_order() {
        let time = Time::new();
        let router = time.router();
        let (duckduckgo, searxng, brave) =
            (Fake::finding(Duckduckgo, 2), Fake::finding(Searxng, 2), Fake::finding(Brave, 2));
        let providers = all(&[&duckduckgo, &brave, &searxng]);
        let results = router
            .search(&providers, &on(&[Duckduckgo, Searxng, Brave]), &Query::new("x", None, None))
            .await
            .expect("hits");
        assert_eq!(results.asked, [Searxng]);
        assert_eq!(sources(&results), [Searxng, Searxng]);
        assert!(results.untrusted);
        assert_eq!((brave.calls(), duckduckgo.calls()), (0, 0));
    }

    #[tokio::test]
    async fn falls_back_on_a_block_a_limit_or_a_missing_key() {
        let time = Time::new();
        let router = time.router();
        let searxng = Fake::new(Searxng, Err(SearchError::Blocked));
        let brave = Fake::new(Brave, Err(SearchError::NeedsKey));
        let tavily = Fake::new(Tavily, Err(SearchError::RateLimited { retry_after: None }));
        let duckduckgo = Fake::finding(Duckduckgo, 1);
        let providers = all(&[&searxng, &brave, &tavily, &duckduckgo]);
        let switched = on(&[Searxng, Brave, Tavily, Duckduckgo]);
        let results = router.search(&providers, &switched, &Query::new("x", None, None)).await.expect("hits");
        assert_eq!(sources(&results), [Duckduckgo]);
        assert_eq!(results.asked, [Searxng, Tavily, Duckduckgo], "Brave sent nothing without its key");
    }

    #[tokio::test]
    async fn any_other_failure_ends_the_search() {
        let time = Time::new();
        let router = time.router();
        let searxng = Fake::new(Searxng, Err(SearchError::Failed(OFFLINE)));
        let brave = Fake::finding(Brave, 1);
        let providers = all(&[&searxng, &brave]);
        let answer = router.search(&providers, &on(&[Searxng, Brave]), &Query::new("x", None, None)).await;
        assert_eq!(answer, Err(NoAnswer::Failed(OFFLINE)));
        assert_eq!(brave.calls(), 0, "the words go no further");
    }

    #[tokio::test]
    async fn only_switched_on_providers_are_asked() {
        let time = Time::new();
        let router = time.router();
        let fakes: Vec<Arc<Fake>> = SearchProviderId::ORDER.into_iter().map(|id| Fake::finding(id, 1)).collect();
        let providers: Vec<Arc<dyn SearchProvider>> =
            fakes.iter().map(|fake| Arc::clone(fake) as Arc<dyn SearchProvider>).collect();
        let results = router.search(&providers, &on(&[Exa]), &Query::new("x", None, None)).await.expect("hits");
        assert_eq!(results.asked, [Exa]);
        let calls: Vec<(SearchProviderId, usize)> = fakes.iter().map(|fake| (fake.id, fake.calls())).collect();
        assert_eq!(
            calls,
            [(Searxng, 0), (Brave, 0), (Tavily, 0), (Exa, 1), (Mojeek, 0), (Duckduckgo, 0), (Wikipedia, 0)]
        );
        assert_eq!(
            router.search(&providers, &BTreeSet::new(), &Query::new("x", None, None)).await,
            Err(NoAnswer::NoneOn)
        );
        assert_eq!(router.search(&[], &on(&[Exa]), &Query::new("x", None, None)).await, Err(NoAnswer::NoneOn));
    }

    #[tokio::test]
    async fn a_blocked_provider_rests_an_hour_for_everyone() {
        let time = Time::new();
        let router = time.router();
        let duckduckgo = Fake::new(Duckduckgo, Err(SearchError::Blocked));
        let providers = all(&[&duckduckgo]);
        let query = Query::new("x", None, None);
        assert_eq!(router.search(&providers, &on(&[Duckduckgo]), &query).await, Err(NoAnswer::Resting));
        let until = router.paused_until(Duckduckgo).expect("resting");
        let left = until - Utc::now();
        assert!(left > chrono::TimeDelta::minutes(59) && left <= chrono::TimeDelta::minutes(60), "{left}");
        duckduckgo.answers(Ok(hits(Duckduckgo, 1)));
        time.pass(Duration::from_secs(30 * 60));
        assert_eq!(router.search(&providers, &on(&[Duckduckgo]), &query).await, Err(NoAnswer::Resting));
        assert_eq!(duckduckgo.calls(), 1, "not asked while it rests");
        time.pass(Duration::from_secs(30 * 60));
        assert!(router.paused_until(Duckduckgo).is_none());
        assert!(router.search(&providers, &on(&[Duckduckgo]), &query).await.is_ok());
        assert_eq!(duckduckgo.calls(), 2);
    }

    #[tokio::test]
    async fn a_limit_rests_as_long_as_the_provider_asked() {
        let time = Time::new();
        let router = time.router();
        let tavily = Fake::new(Tavily, Err(SearchError::RateLimited { retry_after: Some(Duration::from_secs(30)) }));
        let providers = all(&[&tavily]);
        let query = Query::new("x", None, None);
        assert_eq!(router.search(&providers, &on(&[Tavily]), &query).await, Err(NoAnswer::Resting));
        time.pass(Duration::from_secs(31));
        tavily.answers(Ok(hits(Tavily, 1)));
        assert!(router.search(&providers, &on(&[Tavily]), &query).await.is_ok());
        assert_eq!(tavily.calls(), 2);
    }

    #[tokio::test]
    async fn kept_answers_come_from_memory_for_an_hour() {
        let time = Time::new();
        let router = time.router();
        let searxng = Fake::finding(Searxng, 2);
        let providers = all(&[&searxng]);
        let switched = on(&[Searxng]);
        router.search(&providers, &switched, &Query::new("Lisbon trams", None, None)).await.expect("hits");
        time.pass(Duration::from_secs(5));
        let again = router.search(&providers, &switched, &Query::new(" lisbon TRAMS", None, None)).await.expect("hits");
        assert_eq!(searxng.calls(), 1, "from memory");
        assert_eq!(again.asked, [Searxng], "named although it answered from memory");
        assert_eq!(again.hits.len(), 2);
        router.search(&providers, &switched, &Query::new("Lisbon trams", Some(1), None)).await.expect("hits");
        assert_eq!(searxng.calls(), 2, "another count is another answer");
        time.pass(ANSWER_FOR);
        router.search(&providers, &switched, &Query::new("Lisbon trams", None, None)).await.expect("hits");
        assert_eq!(searxng.calls(), 3, "an hour later the provider is asked again");
    }

    #[tokio::test]
    async fn brave_is_never_kept() {
        let time = Time::new();
        let router = time.router();
        // Even a Brave that says its answers may be kept.
        let brave = Fake::finding(Brave, 1);
        let providers = all(&[&brave]);
        for _ in 0..2 {
            router.search(&providers, &on(&[Brave]), &Query::new("x", None, None)).await.expect("hits");
            time.pass(Duration::from_secs(2));
        }
        assert_eq!(brave.calls(), 2);
        assert!(router.answers.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn wikipedia_comes_besides_and_after() {
        let time = Time::new();
        let router = time.router();
        let duckduckgo = Fake::finding(Duckduckgo, 10);
        let wikipedia = Fake::finding(Wikipedia, 5);
        let providers = all(&[&wikipedia, &duckduckgo]);
        let switched = on(&[Duckduckgo, Wikipedia]);
        let five = router.search(&providers, &switched, &Query::new("x", Some(5), None)).await.expect("hits");
        assert_eq!(five.asked, [Duckduckgo, Wikipedia]);
        assert_eq!(sources(&five), [[Duckduckgo; 3].as_slice(), &[Wikipedia; 2]].concat(), "five together");
        time.pass(Duration::from_secs(20));
        let ten = router.search(&providers, &switched, &Query::new("y", Some(10), None)).await.expect("hits");
        assert_eq!(sources(&ten), [[Duckduckgo; 7].as_slice(), &[Wikipedia; 3]].concat(), "three of Wikipedia's");
    }

    /// Found in the review of #305: `count` holds for all hits together. The others keep at least half of it,
    /// Wikipedia gets at most three of the rest; with few hits of the others it fills up, never past `count`.
    #[tokio::test]
    async fn wikipedia_stays_within_the_count() {
        let time = Time::new();
        let router = time.router();
        let duckduckgo = Fake::finding(Duckduckgo, 10);
        let wikipedia = Fake::finding(Wikipedia, 5);
        let providers = all(&[&duckduckgo, &wikipedia]);
        let switched = on(&[Duckduckgo, Wikipedia]);
        for (n, count, expected) in
            [(0, 1, (1, 0)), (1, 2, (1, 1)), (2, 3, (2, 1)), (3, 4, (2, 2)), (4, 6, (3, 3)), (5, 9, (6, 3))]
        {
            time.pass(Duration::from_secs(20));
            let query = Query::new(&format!("q{n}"), Some(count), None);
            let results = router.search(&providers, &switched, &query).await.expect("hits");
            let wanted = [vec![Duckduckgo; expected.0], vec![Wikipedia; expected.1]].concat();
            assert_eq!(sources(&results), wanted, "count {count}");
        }
        // Few of the others: Wikipedia fills up to three, within the count.
        duckduckgo.answers(Ok(hits(Duckduckgo, 1)));
        time.pass(Duration::from_secs(20));
        let few = router.search(&providers, &switched, &Query::new("few", Some(5), None)).await.expect("hits");
        assert_eq!(sources(&few), [Duckduckgo, Wikipedia, Wikipedia, Wikipedia]);
        time.pass(Duration::from_secs(20));
        let one = router.search(&providers, &switched, &Query::new("few", Some(2), None)).await.expect("hits");
        assert_eq!(sources(&one), [Duckduckgo, Wikipedia]);
    }

    #[tokio::test]
    async fn wikipedia_is_no_fallback_but_answers_alone() {
        let time = Time::new();
        let router = time.router();
        let duckduckgo = Fake::new(Duckduckgo, Err(SearchError::Blocked));
        let wikipedia = Fake::finding(Wikipedia, 5);
        let providers = all(&[&duckduckgo, &wikipedia]);
        let besides = router.search(&providers, &on(&[Duckduckgo, Wikipedia]), &Query::new("x", None, None)).await;
        assert_eq!(sources(&besides.expect("hits")), [Wikipedia; 3], "its own hits when the others have none");
        time.pass(Duration::from_secs(2));
        let alone = router.search(&providers, &on(&[Wikipedia]), &Query::new("y", None, None)).await.expect("hits");
        assert_eq!((alone.asked.as_slice(), alone.hits.len()), ([Wikipedia].as_slice(), 5), "alone: up to the count");
        time.pass(Duration::from_secs(2));
        let one = router.search(&providers, &on(&[Wikipedia]), &Query::new("y1", Some(1), None)).await.expect("hits");
        assert_eq!(one.hits.len(), 1);
        time.pass(Duration::from_secs(2));
        let besides = router.search(&providers, &on(&[Duckduckgo, Wikipedia]), &Query::new("w", Some(2), None)).await;
        assert_eq!(besides.expect("hits").hits.len(), 2, "its own hits stay within the count too");
        wikipedia.answers(Ok(Vec::new()));
        time.pass(Duration::from_secs(2));
        let nothing = router.search(&providers, &on(&[Duckduckgo, Wikipedia]), &Query::new("z", None, None)).await;
        assert_eq!(nothing, Err(NoAnswer::Resting), "DuckDuckGo still rests and Wikipedia found nothing");
    }

    #[tokio::test]
    async fn wikipedia_leaves_out_pages_the_others_found() {
        let time = Time::new();
        let router = time.router();
        let mut found = hits(Duckduckgo, 1);
        found[0].url = "https://wikipedia.example.org/0".into();
        let duckduckgo = Fake::new(Duckduckgo, Ok(found));
        let wikipedia = Fake::finding(Wikipedia, 2);
        let providers = all(&[&duckduckgo, &wikipedia]);
        let results = router.search(&providers, &on(&[Duckduckgo, Wikipedia]), &Query::new("x", None, None)).await;
        let urls: Vec<String> = results.expect("hits").hits.into_iter().map(|hit| hit.url).collect();
        assert_eq!(urls, ["https://wikipedia.example.org/0", "https://wikipedia.example.org/1"]);
    }

    #[tokio::test]
    async fn says_why_nothing_came() {
        let time = Time::new();
        let router = time.router();
        let brave = Fake::new(Brave, Err(SearchError::NeedsKey));
        let exa = Fake::new(Exa, Err(SearchError::NeedsKey));
        let providers = all(&[&brave, &exa]);
        let answer = router.search(&providers, &on(&[Brave, Exa]), &Query::new("x", None, None)).await;
        assert_eq!(answer, Err(NoAnswer::NeedsKey));
        time.pass(Duration::from_secs(2));
        let mojeek = Fake::new(Mojeek, Err(SearchError::RateLimited { retry_after: None }));
        let providers = all(&[&brave, &mojeek]);
        let answer = router.search(&providers, &on(&[Brave, Mojeek]), &Query::new("x", None, None)).await;
        assert_eq!(answer, Err(NoAnswer::Resting), "a rest outweighs a missing key");
        let tavily = Fake::new(Tavily, Err(SearchError::Failed(BROKEN)));
        let providers = all(&[&tavily]);
        assert_eq!(
            router.search(&providers, &on(&[Tavily]), &Query::new("x", None, None)).await,
            Err(NoAnswer::Failed(BROKEN))
        );
    }

    #[test]
    fn the_gate_lets_one_wait_and_turns_more_away() {
        let mut gate = Gate::default();
        let start = Instant::now();
        let gap = gap(Duckduckgo);
        assert_eq!(gap, Duration::from_secs(10));
        assert_eq!(gate.turn(start, gap), Turn::Go);
        assert_eq!(gate.turn(start + Duration::from_secs(4), gap), Turn::After(Duration::from_secs(6)));
        assert_eq!(gate.turn(start + Duration::from_secs(5), gap), Turn::Busy, "a second one would wait too long");
        assert_eq!(gate.turn(start + Duration::from_secs(20), gap), Turn::Go);
        gate.paused_until = Some(start + Duration::from_secs(100));
        assert_eq!(gate.turn(start + Duration::from_secs(99), gap), Turn::Paused);
        assert_eq!(gate.turn(start + Duration::from_secs(100), gap), Turn::Go);
        assert_eq!(super::gap(Searxng), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn the_throttle_passes_a_busy_provider_by() {
        let time = Time::new();
        let router = time.router();
        let duckduckgo = Fake::finding(Duckduckgo, 1);
        let providers = all(&[&duckduckgo]);
        let switched = on(&[Duckduckgo]);
        router.search(&providers, &switched, &Query::new("a", None, None)).await.expect("hits");
        // A second search within the gap would wait; take its turn away by pretending one already waits.
        router.gates.lock().expect("lock").entry(Duckduckgo).or_default().next_free =
            Some((router.clock)() + Duration::from_secs(20));
        assert_eq!(router.search(&providers, &switched, &Query::new("b", None, None)).await, Err(NoAnswer::Resting));
        assert_eq!(duckduckgo.calls(), 1);
    }

    #[tokio::test]
    async fn forgetting_a_provider_drops_only_its_answers() {
        let time = Time::new();
        let router = time.router();
        let (searxng, mojeek) = (Fake::finding(Searxng, 1), Fake::finding(Mojeek, 1));
        let providers = all(&[&searxng, &mojeek]);
        let query = Query::new("x", None, None);
        for id in [Searxng, Mojeek] {
            router.search(&providers, &on(&[id]), &query).await.expect("hits");
        }
        router.forget(Searxng);
        time.pass(Duration::from_secs(2));
        for id in [Searxng, Mojeek] {
            router.search(&providers, &on(&[id]), &query).await.expect("hits");
        }
        assert_eq!((searxng.calls(), mojeek.calls()), (2, 1), "Mojeek's answer is still kept");
    }

    #[test]
    fn at_most_so_many_answers_are_kept() {
        let time = Time::new();
        let router = time.router();
        for n in 0..=MAX_ANSWERS {
            router.keep(format!("key {n}"), Vec::new());
            time.pass(Duration::from_millis(1));
        }
        let answers = router.answers.lock().expect("lock");
        assert_eq!(answers.len(), MAX_ANSWERS);
        assert!(!answers.contains_key("key 0"), "the oldest went first");
    }
}
