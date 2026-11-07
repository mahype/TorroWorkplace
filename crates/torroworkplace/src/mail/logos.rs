//! Logos of known senders (issue #453): what the core shows in the circle of a mail whose sender's
//! domain signed it ([`super::check`]). Logos are marks of their owners, so the suite brings no
//! collection of its own: each logo is what the domain publishes itself, fetched once per domain
//! and kept on this computer ([`SenderLogos`]).
//!
//! **Where a logo comes from**, in this order ([`SenderLogos::find`]):
//!
//! 1. BIMI: the TXT record `default._bimi.<domain>` with `v=BIMI1` names an SVG (`l=`). It counts
//!    only while the domain's DMARC record says `p=quarantine` or `p=reject` for all its mail (no
//!    `pct` below 100), as the mail programs that show BIMI require. Its certificate (`a=`) is not
//!    checked (decision 2 of the plan): the website's symbol never has one either.
//! 2. `https://<domain>/apple-touch-icon.png`.
//! 3. The domain's start page, when it stays on the domain (or a domain below it): the largest
//!    `apple-touch-icon` it names, else an SVG `icon`, else a PNG `icon`.
//!
//! Only the domain itself and the addresses its own BIMI record or start page name are asked
//! (decision 3: PayPal, Amazon or GitHub keep their logos on CDNs of their own), never a service in
//! between. Every request goes only to the public internet ([`GuardedDns`], no proxy), only over
//! `https`, follows at most [`MAX_REDIRECTS`] redirects, each checked anew, and gives up after
//! [`CONNECT_TIMEOUT`] to connect and [`DEADLINE`] in all.
//!
//! **What is kept**: never the original. An SVG (at most [`SVG_MAX_BYTES`], never compressed) is
//! drawn without scripts and without anything it refers to: one that holds a script, an entity, a
//! foreign object or a picture is refused whole. A raster picture (at most [`RASTER_MAX_BYTES`],
//! [`RASTER_MAX_PIXELS`] on each side when decoded, at least [`RASTER_MIN_PIXELS`]) is decoded within
//! limits and fills four fifths of the circle, so its corners are not cut. Both are drawn on a
//! blocking thread, fitted onto white, [`LOGO_PIXELS`] square, cut round (so the logo is round in
//! every renderer), and kept as PNG in `<cache>/sender-logos/` (0700, files 0600): `<sha256 of the
//! domain>.png` and `index.json`, when each domain was asked and whether it had a logo; at most
//! [`MAX_DOMAINS`].
//!
//! **When**: a domain is asked when a checked mail names it ([`SenderLogos::want`]) and it was never
//! asked, its logo is older than [`FOUND_FOR`], or it had none for [`NOT_FOUND_FOR`]. Only a DNS that
//! does not answer counts as offline (asked again after an hour, nothing noted); a website that
//! cannot be reached is noted and asked again after [`UNREACHED_SITE_FOR`], a kept logo staying. A kept logo not found when asked again stays for one more round, so a
//! server failing for a moment does not take it away. One task asks
//! one domain after the other, [`GAP`] apart. Opening a mail asks nothing: the reading commands only
//! read what is kept ([`SenderLogos::has`], [`SenderLogos::png`]). While "Logos of known senders" is
//! off ([`SenderLogos::set_switch`]) nothing is asked or handed out.
//!
//! Nothing here logs a domain or an address.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::io::Cursor;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use image::{Rgba, RgbaImage};
use reqwest::Url;
use reqwest::header::{ACCEPT, LOCATION};
use resvg::tiny_skia::{Color, Pixmap, Transform};
use resvg::usvg;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use torro_pim_caldav::discover::{GuardedDns, Resolver, SystemResolver, checked_domain, is_public};

use crate::tools::web::fetch::{Guard, checked_url};

/// The side of a kept logo in pixels.
pub const LOGO_PIXELS: u32 = 128;

/// The largest SVG read (a BIMI logo is a few kilobytes).
pub const SVG_MAX_BYTES: usize = 32 * 1024;

/// The largest raster picture read.
pub const RASTER_MAX_BYTES: usize = 256 * 1024;

/// The most of a start page read for the symbols it names.
pub const PAGE_MAX_BYTES: usize = 512 * 1024;

/// A raster picture smaller than this on a side is too coarse for the circle.
pub const RASTER_MIN_PIXELS: u32 = 48;

/// A raster picture larger than this on a side is refused before it is decoded.
pub const RASTER_MAX_PIXELS: u32 = 2048;

/// How much of the circle's side a raster picture or a website's SVG symbol fills; a BIMI SVG is made
/// for the circle and fills all of it.
const RASTER_FILL_PERCENT: u32 = 80;

/// The most memory decoding one raster picture may take.
const RASTER_MAX_ALLOC: u64 = 64 << 20;

/// A logo is asked for again after this long.
pub const FOUND_FOR: TimeDelta = TimeDelta::days(30);

/// A domain without a logo is asked again after this long.
pub const NOT_FOUND_FOR: TimeDelta = TimeDelta::days(7);

/// A domain whose website could not be reached is asked again after this long: a network that lets
/// DNS through but blocks the web (a hotel's sign-in page) must not take logos away for a week.
pub const UNREACHED_SITE_FOR: TimeDelta = TimeDelta::days(1);

/// A domain that could not be reached (offline) is asked again after this long at the earliest.
const UNREACHED_FOR: Duration = Duration::from_secs(60 * 60);

/// The time between asking two domains.
pub const GAP: Duration = Duration::from_secs(1);

/// How many redirects one request follows.
pub const MAX_REDIRECTS: usize = 3;

/// How long connecting to a server may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one request may take altogether.
pub const DEADLINE: Duration = Duration::from_secs(20);

/// At most this many symbols of a start page are tried.
const PAGE_CANDIDATES: usize = 4;

/// The file of what was asked, beside the logos.
const INDEX_FILE: &str = "index.json";

/// At most this many domains are kept; beyond, those asked longest ago go with their logos.
pub const MAX_DOMAINS: usize = 2000;

/// Why a request brought nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebError {
    /// No connection, no DNS, or the deadline passed: ask again later, nothing is noted.
    Offline,
    /// Anything else (refused address, larger than allowed, a broken answer): there is no logo here.
    Failed,
    /// The DNS answered, but the domain's website could not be reached ([`SenderLogos::find`]):
    /// noted, and asked again after [`UNREACHED_SITE_FOR`].
    Unreached,
}

/// What one request brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The body, at most as long as asked.
    Body(Vec<u8>),
    /// A redirect to this address (as the server wrote it, maybe relative).
    Redirect(String),
    /// Another answer than the page.
    Status(u16),
}

/// A future of [`LogoWeb`].
pub type WebFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, WebError>> + Send + 'a>>;

/// The DNS and the web as the logos see them; [`SystemWeb`] in the service, a fake in tests.
pub trait LogoWeb: Send + Sync {
    /// The TXT records of `name`, each as one string; empty when it has none.
    fn txt<'a>(&'a self, name: &'a str) -> WebFuture<'a, Vec<String>>;
    /// Asks `url` once, without following a redirect, reading at most `max` bytes of its body (a
    /// longer one is [`WebError::Failed`]).
    fn get<'a>(&'a self, url: &'a Url, max: usize) -> WebFuture<'a, Reply>;
}

/// The system's DNS and an HTTP client that connects only to the public internet, without a proxy.
pub struct SystemWeb {
    resolver: Arc<dyn Resolver>,
    /// Built on first use; `None` when no client could be built: then every request is offline.
    http: OnceLock<Option<reqwest::Client>>,
}

impl SystemWeb {
    pub fn new() -> SystemWeb {
        SystemWeb { resolver: Arc::new(SystemResolver::new()), http: OnceLock::new() }
    }

    fn http(&self) -> Option<&reqwest::Client> {
        self.http
            .get_or_init(|| {
                reqwest::Client::builder()
                    .user_agent(crate::places::USER_AGENT)
                    // A proxy would resolve the names itself, past the guard.
                    .no_proxy()
                    // By hand, so every address is checked before it is asked.
                    .redirect(reqwest::redirect::Policy::none())
                    .https_only(true)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(DEADLINE)
                    .dns_resolver(Arc::new(GuardedDns::new(Arc::clone(&self.resolver), is_public)))
                    .build()
                    .ok()
            })
            .as_ref()
    }
}

impl LogoWeb for SystemWeb {
    fn txt<'a>(&'a self, name: &'a str) -> WebFuture<'a, Vec<String>> {
        Box::pin(async move { self.resolver.txt(name).await.map_err(|_| WebError::Offline) })
    }

    fn get<'a>(&'a self, url: &'a Url, max: usize) -> WebFuture<'a, Reply> {
        Box::pin(async move {
            let http = self.http().ok_or(WebError::Offline)?;
            let accept = "image/svg+xml, image/png, image/*;q=0.9, text/html;q=0.8";
            let sent = http.get(url.clone()).header(ACCEPT, accept).send().await;
            // The error loses its address before anything looks at it.
            let mut response = sent.map_err(|err| failure(&err.without_url()))?;
            let status = response.status();
            if status.is_redirection() {
                let location = response.headers().get(LOCATION).and_then(|value| value.to_str().ok());
                return Ok(location.map_or(Reply::Status(status.as_u16()), |to| Reply::Redirect(to.to_string())));
            }
            if !status.is_success() {
                return Ok(Reply::Status(status.as_u16()));
            }
            if response.content_length().is_some_and(|length| length > max as u64) {
                return Err(WebError::Failed);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|err| failure(&err.without_url()))? {
                if body.len() + chunk.len() > max {
                    return Err(WebError::Failed);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Reply::Body(body))
        })
    }
}

/// A failed request; `err` has lost its address already.
fn failure(err: &reqwest::Error) -> WebError {
    if err.is_connect() || err.is_timeout() { WebError::Offline } else { WebError::Failed }
}

/// Whether logos are on: "Logos of known senders" (`mail_sender_logos`), read afresh for each use.
pub type LogoSwitch = Arc<dyn Fn() -> bool + Send + Sync>;

/// When a domain was asked and whether it had a logo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Asked {
    at: DateTime<Utc>,
    found: bool,
    /// A logo is kept, but the last time it was asked again it was not found: kept once more, so a
    /// server that fails for a moment does not take it away; asked again after [`NOT_FOUND_FOR`].
    #[serde(default)]
    missed: bool,
    /// The last time it was asked its website could not be reached ([`WebError::Unreached`]): a kept
    /// logo stays as it is; asked again after [`UNREACHED_SITE_FOR`].
    #[serde(default)]
    unreached: bool,
}

impl Asked {
    /// Whether the domain is to be asked again at `now`.
    fn due(&self, now: DateTime<Utc>) -> bool {
        let age = now.signed_duration_since(self.at);
        age > match (self.unreached, self.found && !self.missed) {
            (true, _) => UNREACHED_SITE_FOR,
            (false, true) => FOUND_FOR,
            (false, false) => NOT_FOUND_FOR,
        }
    }
}

/// `index.json`: what was asked, by domain.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    domains: BTreeMap<String, Asked>,
}

/// What [`SenderLogos`] holds behind its lock.
#[derive(Default)]
struct State {
    /// Read from the disk on first use.
    index: Option<Index>,
    /// The domains to ask, in turn, and the same as a set.
    queue: VecDeque<String>,
    queued: HashSet<String>,
    /// A task is asking the queue's domains.
    running: bool,
    /// Domains that could not be reached, and when.
    unreached: HashMap<String, Instant>,
}

/// The logos of the domains of checked senders, one for all mail accounts.
pub struct SenderLogos {
    /// `<cache>/sender-logos`; `None` without a home directory: then nothing is asked or kept.
    dir: Option<PathBuf>,
    web: Arc<dyn LogoWeb>,
    switch: RwLock<LogoSwitch>,
    state: Mutex<State>,
    gap: Duration,
}

impl SenderLogos {
    /// The logos kept in `dir`, asked through `web`; on until [`SenderLogos::set_switch`] says
    /// otherwise.
    pub fn new(dir: Option<PathBuf>, web: Arc<dyn LogoWeb>) -> SenderLogos {
        SenderLogos { dir, web, switch: RwLock::new(Arc::new(|| true)), state: Mutex::default(), gap: GAP }
    }

    /// No time between two domains, for tests.
    #[cfg(test)]
    pub fn without_gap(mut self) -> SenderLogos {
        self.gap = Duration::ZERO;
        self
    }

    /// Sets what says whether logos are on (the setting `mail_sender_logos`).
    pub fn set_switch(&self, switch: LogoSwitch) {
        *self.switch.write().unwrap_or_else(PoisonError::into_inner) = switch;
    }

    /// Whether logos are on now: checks, requests and answers happen only then.
    pub fn on(&self) -> bool {
        let switch = Arc::clone(&self.switch.read().unwrap_or_else(PoisonError::into_inner));
        switch()
    }

    /// Whether a logo of `domain` is kept and logos are on.
    pub fn has(&self, domain: &str) -> bool {
        self.on() && self.kept(domain)
    }

    /// Whether a logo of `domain` is kept, whether logos are on or not.
    pub fn kept(&self, domain: &str) -> bool {
        self.with_index(|index| index.domains.get(domain).is_some_and(|asked| asked.found))
    }

    /// The kept logo of `domain` as PNG, while logos are on.
    pub fn png(&self, domain: &str) -> Option<Vec<u8>> {
        if !self.has(domain) {
            return None;
        }
        std::fs::read(self.dir.as_ref()?.join(file_name(domain))).ok()
    }

    /// A checked mail names `domain` (issue #453): it is asked in the background when it is due
    /// (never asked, its logo older than [`FOUND_FOR`], no logo for [`NOT_FOUND_FOR`]) and was not
    /// unreachable within the last hour. Nothing happens while logos are off, without a home
    /// directory or outside a Tokio runtime.
    pub fn want(self: &Arc<Self>, domain: &str, now: DateTime<Utc>) {
        if self.dir.is_none() || !self.on() {
            return;
        }
        let Some(domain) = checked_domain(domain) else { return };
        let start = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let due = self.index_in(&mut state).domains.get(&domain).is_none_or(|asked| asked.due(now));
            let resting = state.unreached.get(&domain).is_some_and(|at| at.elapsed() < UNREACHED_FOR);
            if !due || resting || state.queued.contains(&domain) {
                return;
            }
            state.queued.insert(domain.clone());
            state.queue.push_back(domain);
            !std::mem::replace(&mut state.running, true)
        };
        if start {
            match tokio::runtime::Handle::try_current() {
                Ok(runtime) => drop(runtime.spawn(Arc::clone(self).ask_queue())),
                Err(_) => self.state.lock().unwrap_or_else(PoisonError::into_inner).running = false,
            }
        }
    }

    /// Asks the queued domains one after the other, [`GAP`] apart, until the queue is empty. However
    /// it ends, a later [`SenderLogos::want`] starts a new one ([`Running`]).
    async fn ask_queue(self: Arc<Self>) {
        let _running = Running(Arc::clone(&self));
        loop {
            let next = {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                match state.queue.pop_front() {
                    Some(domain) => domain,
                    None => {
                        state.running = false;
                        return;
                    }
                }
            };
            // Switched off meanwhile: nothing more is asked.
            let found = if self.on() { Some(self.find(&next).await) } else { None };
            if let Some(found) = found {
                self.keep(&next, found, Utc::now());
            }
            self.state.lock().unwrap_or_else(PoisonError::into_inner).queued.remove(&next);
            tokio::time::sleep(self.gap).await;
        }
    }

    /// Notes what asking `domain` found: its logo, none, or that it could not be reached. A kept logo
    /// that is not found once stays for one more round ([`Asked::missed`]). At most [`MAX_DOMAINS`]
    /// are kept.
    fn keep(&self, domain: &str, found: Result<Option<Vec<u8>>, WebError>, now: DateTime<Utc>) {
        let Some(dir) = &self.dir else { return };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let site_down = found == Err(WebError::Unreached);
        let logo = match found {
            Ok(logo) => logo,
            Err(WebError::Unreached) => None,
            Err(_) => {
                state.unreached.insert(domain.to_string(), Instant::now());
                return;
            }
        };
        state.unreached.remove(domain);
        let before = self.index_in(&mut state).domains.get(domain).copied();
        let file = dir.join(file_name(domain));
        let unreached = site_down;
        let (asked, written) = match &logo {
            Some(png) => (
                Asked { at: now, found: true, missed: false, unreached },
                crate::clients::create_private_dir(dir)
                    .map_err(|err| err.kind())
                    .and_then(|()| crate::clients::write_private(&file, png).map_err(|_| std::io::ErrorKind::Other)),
            ),
            // The website was not there: a kept logo stays as it was, and is asked again in a day.
            None if site_down => match before {
                Some(kept) if kept.found => (Asked { at: now, unreached, ..kept }, Ok(())),
                _ => (Asked { at: now, found: false, missed: false, unreached }, remove(&file)),
            },
            None if before.is_some_and(|asked| asked.found && !asked.missed) => {
                (Asked { at: now, found: true, missed: true, unreached }, Ok(()))
            }
            None => (Asked { at: now, found: false, missed: false, unreached }, remove(&file)),
        };
        if let Err(kind) = written {
            eprintln!("torroworkplace: sender logos: cannot keep a logo: {kind}");
            return;
        }
        let index = self.index_in(&mut state);
        index.domains.insert(domain.to_string(), asked);
        while index.domains.len() > MAX_DOMAINS {
            let Some(oldest) = index.domains.iter().min_by_key(|(_, asked)| asked.at).map(|(name, _)| name.clone())
            else {
                break;
            };
            index.domains.remove(&oldest);
            let _ = remove(&dir.join(file_name(&oldest)));
        }
        let json = serde_json::to_vec(index).unwrap_or_default();
        let stored = crate::clients::create_private_dir(dir).map_err(|err| err.kind()).and_then(|()| {
            crate::clients::write_private(&dir.join(INDEX_FILE), &json).map_err(|_| std::io::ErrorKind::Other)
        });
        if let Err(kind) = stored {
            eprintln!("torroworkplace: sender logos: cannot write what was asked: {kind}");
        }
    }

    fn with_index<T>(&self, read: impl FnOnce(&Index) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        read(self.index_in(&mut state))
    }

    /// The index in `state`, read from the disk the first time; a missing or broken file is empty.
    fn index_in<'a>(&self, state: &'a mut State) -> &'a mut Index {
        state.index.get_or_insert_with(|| {
            self.dir
                .as_ref()
                .and_then(|dir| std::fs::read(dir.join(INDEX_FILE)).ok())
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        })
    }

    /// The logo of `domain` after the module docs, as a round PNG of [`LOGO_PIXELS`] on white;
    /// `Ok(None)` when the domain has none, [`WebError::Offline`] when neither the DNS for its BIMI
    /// record nor its website answered, [`WebError::Unreached`] when the DNS answered but neither of
    /// the domain's own addresses (its touch icon and start page) could be reached: a domain that
    /// only sends mail, or a network that blocks the web. A DNS failing for BIMI alone is no BIMI.
    pub async fn find(&self, domain: &str) -> Result<Option<Vec<u8>>, WebError> {
        // A DNS that fails for the BIMI or DMARC record (SERVFAIL of one zone) does not stop the
        // website's symbol; only when the website cannot be reached either is it offline.
        let bimi = self.bimi(domain).await;
        let dns_down = bimi.is_err();
        if let Ok(Some(svg)) = bimi
            && let Some(logo) = drawn(svg, Drawn::Bimi).await
        {
            return Ok(Some(logo));
        }
        let Ok(touch) = Url::parse(&format!("https://{domain}/apple-touch-icon.png")) else { return Ok(None) };
        let touch = self.fetch(touch, RASTER_MAX_BYTES).await;
        let touch_down = touch == Err(WebError::Offline);
        if let Ok(Some((_, picture))) = touch
            && let Some(logo) = drawn(picture, Drawn::Raster).await
        {
            return Ok(Some(logo));
        }
        let Ok(start) = Url::parse(&format!("https://{domain}/")) else { return Ok(None) };
        let (at, page) = match self.fetch(start, PAGE_MAX_BYTES).await {
            Ok(Some(found)) => found,
            Err(WebError::Offline) if touch_down && dns_down => return Err(WebError::Offline),
            Err(WebError::Offline) if touch_down => return Err(WebError::Unreached),
            _ => return Ok(None),
        };
        let own = at.host_str().is_some_and(|host| host == domain || host.ends_with(&format!(".{domain}")));
        if !own {
            return Ok(None);
        }
        for symbol in symbols(&String::from_utf8_lossy(&page), &at).into_iter().take(PAGE_CANDIDATES) {
            let max = if symbol.svg { SVG_MAX_BYTES } else { RASTER_MAX_BYTES };
            let Some((_, picture)) = self.reach(symbol.url, max).await else { continue };
            let logo = drawn(picture, if symbol.svg { Drawn::Svg } else { Drawn::Raster }).await;
            if logo.is_some() {
                return Ok(logo);
            }
        }
        Ok(None)
    }

    /// The SVG the BIMI record of `domain` names, when the record and the domain's DMARC policy
    /// allow it; `Ok(None)` otherwise.
    async fn bimi(&self, domain: &str) -> Result<Option<Vec<u8>>, WebError> {
        let records = self.web.txt(&format!("default._bimi.{domain}")).await?;
        let Some(location) = records.iter().filter_map(|record| bimi_location(record)).next() else { return Ok(None) };
        let policy = self.web.txt(&format!("_dmarc.{domain}")).await?;
        if !policy.iter().any(|record| dmarc_enforced(record)) {
            return Ok(None);
        }
        let Ok(url) = Url::parse(&location) else { return Ok(None) };
        Ok(self.reach(url, SVG_MAX_BYTES).await.map(|(_, svg)| svg))
    }

    /// [`SenderLogos::fetch`], with a server that cannot be reached as no answer.
    async fn reach(&self, url: Url, max: usize) -> Option<(Url, Vec<u8>)> {
        self.fetch(url, max).await.ok().flatten()
    }

    /// The body at `url` and the address it came from, after at most [`MAX_REDIRECTS`] redirects,
    /// each address `https` on the public internet ([`checked_url`]); `Ok(None)` for an address
    /// refused, another answer than the page, or a failed request. Only [`WebError::Offline`] fails.
    async fn fetch(&self, mut url: Url, max: usize) -> Result<Option<(Url, Vec<u8>)>, WebError> {
        for _ in 0..=MAX_REDIRECTS {
            if url.scheme() != "https" {
                return Ok(None);
            }
            let Ok(checked) = checked_url(url.as_str(), Guard::PUBLIC) else { return Ok(None) };
            url = checked;
            match self.web.get(&url, max).await {
                Ok(Reply::Body(body)) => return Ok(Some((url, body))),
                Ok(Reply::Redirect(to)) => match url.join(&to) {
                    Ok(next) => url = next,
                    Err(_) => return Ok(None),
                },
                Ok(Reply::Status(_)) | Err(WebError::Failed) => return Ok(None),
                // A request either reaches its server or not; "unreached" is what `find` makes of it.
                Err(WebError::Offline | WebError::Unreached) => return Err(WebError::Offline),
            }
        }
        Ok(None)
    }
}

/// While the task asking the queue runs; however it ends, even by a panic, it notes that none runs.
struct Running(Arc<SenderLogos>);

impl Drop for Running {
    fn drop(&mut self) {
        // Only for a panic: a task that ends normally noted it under the lock already, and a new one
        // may have started since.
        if std::thread::panicking() {
            self.0.state.lock().unwrap_or_else(PoisonError::into_inner).running = false;
        }
    }
}

/// Removes `file`; one that is not there is fine.
fn remove(file: &std::path::Path) -> Result<(), std::io::ErrorKind> {
    match std::fs::remove_file(file) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.kind()),
        _ => Ok(()),
    }
}

/// `bytes` drawn as a logo ([`from_svg`] or [`from_raster`]) on a blocking thread: decoding takes a
/// while, and a picture that makes the decoder fail hard is a picture without logo.
async fn drawn(bytes: Vec<u8>, kind: Drawn) -> Option<Vec<u8>> {
    let draw = move || match kind {
        Drawn::Bimi => from_svg(&bytes),
        Drawn::Svg => svg_filling(&bytes, RASTER_FILL_PERCENT),
        Drawn::Raster => from_raster(&bytes),
    };
    tokio::task::spawn_blocking(draw).await.ok().flatten()
}

/// The file of the logo of `domain`: the SHA-256 of its name, so no name from outside picks a path.
fn file_name(domain: &str) -> String {
    let hash: String = Sha256::digest(domain.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{hash}.png")
}

/// The tags of a TXT record (`v=BIMI1; l=https://…`), names in lower case.
fn tags(record: &str) -> Vec<(String, String)> {
    record
        .split(';')
        .filter_map(|tag| tag.split_once('='))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

/// The logo address `l=` of a BIMI record, when it is one (`v=BIMI1` first) and names an `https`
/// address.
fn bimi_location(record: &str) -> Option<String> {
    let tags = tags(record);
    let (name, version) = tags.first()?;
    if name != "v" || !version.eq_ignore_ascii_case("BIMI1") {
        return None;
    }
    let location = tags.iter().find(|(name, _)| name == "l").map(|(_, value)| value.clone())?;
    location.to_ascii_lowercase().starts_with("https://").then_some(location)
}

/// Whether a DMARC record (`v=DMARC1` first) asks to quarantine or reject all mail that fails.
fn dmarc_enforced(record: &str) -> bool {
    let tags = tags(record);
    let Some((name, version)) = tags.first() else { return false };
    if name != "v" || !version.eq_ignore_ascii_case("DMARC1") {
        return false;
    }
    let value =
        |wanted: &str| tags.iter().find(|(name, _)| name == wanted).map(|(_, value)| value.to_ascii_lowercase());
    let policy = matches!(value("p").as_deref(), Some("quarantine" | "reject"));
    let all = value("pct").is_none_or(|pct| pct == "100");
    policy && all
}

/// A symbol a start page names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Symbol {
    url: Url,
    svg: bool,
}

/// The symbols the HTML page `html` at `base` names, best first: `apple-touch-icon` (the largest
/// first), an SVG `icon`, a PNG `icon` (the largest first). Only `https` addresses.
fn symbols(html: &str, base: &Url) -> Vec<Symbol> {
    let document = dom_query::Document::from(html);
    // (rank, size, symbol): rank 0 the touch icons, 1 SVG, 2 PNG.
    let mut found: Vec<(u8, u32, Symbol)> = Vec::new();
    for link in document.select("link[rel][href]").iter() {
        let rel = link.attr("rel").map(|rel| rel.to_ascii_lowercase()).unwrap_or_default();
        let rels: Vec<&str> = rel.split_ascii_whitespace().collect();
        let Some(href) = link.attr("href") else { continue };
        let Ok(url) = base.join(href.trim()) else { continue };
        if url.scheme() != "https" {
            continue;
        }
        let kind = link.attr("type").map(|kind| kind.to_ascii_lowercase()).unwrap_or_default();
        let path = url.path().to_ascii_lowercase();
        let size = link
            .attr("sizes")
            .and_then(|sizes| {
                sizes.split_ascii_whitespace().filter_map(|size| size.split(['x', 'X']).next()?.parse().ok()).max()
            })
            .unwrap_or(0);
        let touch = rels.iter().any(|rel| rel.starts_with("apple-touch-icon"));
        let icon = rels.contains(&"icon");
        if touch {
            found.push((0, size, Symbol { url, svg: false }));
        } else if icon && (kind == "image/svg+xml" || path.ends_with(".svg")) {
            found.push((1, size, Symbol { url, svg: true }));
        } else if icon && (kind == "image/png" || path.ends_with(".png")) {
            found.push((2, size, Symbol { url, svg: false }));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    found.into_iter().map(|(_, _, symbol)| symbol).collect()
}

/// An SVG drawn after the module docs; `None` when it is too large, holds something refused, or
/// cannot be read.
pub fn from_svg(bytes: &[u8]) -> Option<Vec<u8>> {
    svg_filling(bytes, 100)
}

/// What a picture to draw is ([`drawn`]).
#[derive(Debug, Clone, Copy)]
enum Drawn {
    /// A BIMI logo: an SVG made for the circle, filling all of it.
    Bimi,
    /// An SVG symbol of a website, which may reach its corners: it fills as much as a raster one.
    Svg,
    Raster,
}

/// [`from_svg`], the SVG filling `percent` of the circle's side.
fn svg_filling(bytes: &[u8], percent: u32) -> Option<Vec<u8>> {
    // Compressed SVG (`svgz`) would unpack past every limit before anything here could look at it.
    if bytes.len() > SVG_MAX_BYTES || bytes.starts_with(&[0x1f, 0x8b]) {
        return None;
    }
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if ["<script", "<!entity", "<foreignobject", "javascript:"].iter().any(|refused| lower.contains(refused)) {
        return None;
    }
    // Every picture it refers to, in the file or outside, makes the whole logo refused.
    let referred = std::sync::atomic::AtomicBool::new(false);
    let options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| {
                referred.store(true, std::sync::atomic::Ordering::Relaxed);
                None
            }),
            resolve_string: Box::new(|_, _| {
                referred.store(true, std::sync::atomic::Ordering::Relaxed);
                None
            }),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options).ok()?;
    if referred.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let (width, height) = (tree.size().width(), tree.size().height());
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let side = LOGO_PIXELS as f32;
    let inner = side * percent as f32 / 100.0;
    let scale = (inner / width).min(inner / height);
    let (x, y) = ((side - width * scale) / 2.0, (side - height * scale) / 2.0);
    let mut pixmap = Pixmap::new(LOGO_PIXELS, LOGO_PIXELS)?;
    pixmap.fill(Color::WHITE);
    resvg::render(&tree, Transform::from_row(scale, 0.0, 0.0, scale, x, y), &mut pixmap.as_mut());
    // On opaque white every pixel is opaque: premultiplied and straight colours are the same.
    png(RgbaImage::from_raw(LOGO_PIXELS, LOGO_PIXELS, pixmap.take())?)
}

/// A raster picture (PNG, JPEG, GIF, WebP) decoded within the limits of the module docs and fitted
/// onto white; `None` when it is too large, too small or cannot be read.
pub fn from_raster(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() > RASTER_MAX_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(RASTER_MAX_PIXELS);
    limits.max_image_height = Some(RASTER_MAX_PIXELS);
    limits.max_alloc = Some(RASTER_MAX_ALLOC);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    if picture.width().min(picture.height()) < RASTER_MIN_PIXELS {
        return None;
    }
    // Within the circle's safe square: a touch icon reaches its corners, which the circle would cut.
    let inner = LOGO_PIXELS * RASTER_FILL_PERCENT / 100;
    let fitted = picture.resize(inner, inner, image::imageops::FilterType::Triangle).to_rgba8();
    let mut canvas = RgbaImage::from_pixel(LOGO_PIXELS, LOGO_PIXELS, Rgba([255, 255, 255, 255]));
    let x = (LOGO_PIXELS - fitted.width()) / 2;
    let y = (LOGO_PIXELS - fitted.height()) / 2;
    image::imageops::overlay(&mut canvas, &fitted, i64::from(x), i64::from(y));
    png(canvas)
}

/// `canvas` cut round (transparent outside the circle, with a soft edge of a pixel) as PNG: round in
/// every renderer, also one that cannot clip a picture.
fn png(mut canvas: RgbaImage) -> Option<Vec<u8>> {
    let radius = canvas.width().min(canvas.height()) as f32 / 2.0;
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let distance = ((x as f32 + 0.5 - radius).powi(2) + (y as f32 + 0.5 - radius).powi(2)).sqrt();
        let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
        pixel.0[3] = (f32::from(pixel.0[3]) * coverage).round() as u8;
    }
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/fixtures/sender_logos/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
    }

    /// A picture of `width` × `height` in one colour, encoded as `format`.
    pub(crate) fn picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([20, 90, 200, 255]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image).write_to(&mut out, format).expect("encoded");
        out.into_inner()
    }

    fn decoded(png: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(png, image::ImageFormat::Png).expect("a PNG").to_rgba8()
    }

    /// The DNS and the web of a test: TXT records and answers by address, and every address asked.
    #[derive(Default)]
    pub(crate) struct FakeWeb {
        txt: HashMap<String, Vec<String>>,
        pages: HashMap<String, Reply>,
        offline: bool,
        /// The DNS answers, but no website does.
        http_offline: bool,
        /// No TXT record can be asked, but the websites answer.
        dns_offline: bool,
        asked: Mutex<Vec<String>>,
    }

    impl FakeWeb {
        fn txt(mut self, name: &str, record: &str) -> FakeWeb {
            self.txt.entry(name.to_string()).or_default().push(record.to_string());
            self
        }

        pub(crate) fn page(mut self, url: &str, reply: Reply) -> FakeWeb {
            self.pages.insert(url.to_string(), reply);
            self
        }

        pub(crate) fn asked(&self) -> Vec<String> {
            self.asked.lock().expect("lock").clone()
        }
    }

    impl LogoWeb for FakeWeb {
        fn txt<'a>(&'a self, name: &'a str) -> WebFuture<'a, Vec<String>> {
            Box::pin(async move {
                if self.offline || self.dns_offline {
                    return Err(WebError::Offline);
                }
                Ok(self.txt.get(name).cloned().unwrap_or_default())
            })
        }

        fn get<'a>(&'a self, url: &'a Url, max: usize) -> WebFuture<'a, Reply> {
            Box::pin(async move {
                self.asked.lock().expect("lock").push(url.to_string());
                if self.offline || self.http_offline {
                    return Err(WebError::Offline);
                }
                match self.pages.get(url.as_str()) {
                    Some(Reply::Body(body)) if body.len() > max => Err(WebError::Failed),
                    Some(reply) => Ok(reply.clone()),
                    None => Ok(Reply::Status(404)),
                }
            })
        }
    }

    const BIMI: &str = "v=BIMI1; l=https://cdn.paypal-objects.example/logo.svg; a=";
    const DMARC: &str = "v=DMARC1; p=reject; rua=mailto:d@paypal.example";

    fn with_web(web: FakeWeb) -> (Arc<FakeWeb>, SenderLogos) {
        let web = Arc::new(web);
        (Arc::clone(&web), SenderLogos::new(None, Arc::clone(&web) as Arc<dyn LogoWeb>))
    }

    #[tokio::test]
    async fn bimi_comes_first_with_a_dmarc_policy_that_rejects() {
        let (web, logos) = with_web(
            FakeWeb::default()
                .txt("default._bimi.paypal.example", BIMI)
                .txt("_dmarc.paypal.example", DMARC)
                .page("https://cdn.paypal-objects.example/logo.svg", Reply::Body(fixture("logo.svg")))
                .page(
                    "https://paypal.example/apple-touch-icon.png",
                    Reply::Body(picture(180, 180, image::ImageFormat::Png)),
                ),
        );
        let logo = logos.find("paypal.example").await.expect("asked").expect("a logo");
        let drawn = decoded(&logo);
        assert_eq!(drawn.dimensions(), (LOGO_PIXELS, LOGO_PIXELS));
        assert_eq!(drawn.get_pixel(64, 64), &Rgba([255, 204, 0, 255]), "the SVG's yellow, not the icon's blue");
        assert_eq!(web.asked(), ["https://cdn.paypal-objects.example/logo.svg"], "nothing else asked");
    }

    #[tokio::test]
    async fn bimi_counts_only_with_an_enforced_dmarc_policy() {
        for dmarc in [None, Some("v=DMARC1; p=none"), Some("v=DMARC1; p=quarantine; pct=50")] {
            let mut web = FakeWeb::default()
                .txt("default._bimi.paypal.example", BIMI)
                .page("https://cdn.paypal-objects.example/logo.svg", Reply::Body(fixture("logo.svg")));
            if let Some(dmarc) = dmarc {
                web = web.txt("_dmarc.paypal.example", dmarc);
            }
            let (web, logos) = with_web(web);
            assert_eq!(logos.find("paypal.example").await, Ok(None), "{dmarc:?}");
            assert!(!web.asked().iter().any(|url| url.contains("cdn.")), "{dmarc:?}: the SVG is not asked");
        }
        assert!(dmarc_enforced("v=DMARC1; p=quarantine; pct=100"));
        assert!(!dmarc_enforced("p=reject; v=DMARC1"), "v first");
        assert_eq!(bimi_location("v=BIMI1; l=http://example.org/logo.svg"), None, "https only");
        assert_eq!(bimi_location("v=BIMI1; l="), None);
    }

    #[tokio::test]
    async fn without_bimi_the_touch_icon_and_then_the_start_page() {
        let (_, logos) = with_web(
            FakeWeb::default()
                .page("https://github.example/apple-touch-icon.png", Reply::Redirect("/fluidicon.png".into()))
                .page("https://github.example/fluidicon.png", Reply::Body(picture(512, 256, image::ImageFormat::Png))),
        );
        let logo = decoded(&logos.find("github.example").await.expect("asked").expect("a logo"));
        assert_eq!(logo.get_pixel(64, 64), &Rgba([20, 90, 200, 255]));
        assert_eq!(logo.get_pixel(64, 4), &Rgba([255, 255, 255, 255]), "fitted on white");

        let page = r#"<html><head>
            <link rel="icon" href="/favicon.png" type="image/png" sizes="32x32">
            <link rel="icon" href="https://assets.github-cdn.example/mark.svg" type="image/svg+xml">
            <link rel="apple-touch-icon" sizes="120x120" href="https://assets.github-cdn.example/touch-120.png">
            <link rel="apple-touch-icon" sizes="180x180" href="https://assets.github-cdn.example/touch-180.png">
        </head></html>"#;
        let (web, logos) = with_web(
            FakeWeb::default()
                .page("https://github.example/", Reply::Redirect("https://www.github.example/".into()))
                .page("https://www.github.example/", Reply::Body(page.as_bytes().to_vec()))
                .page(
                    "https://assets.github-cdn.example/touch-180.png",
                    Reply::Body(picture(180, 180, image::ImageFormat::Png)),
                ),
        );
        assert!(logos.find("github.example").await.expect("asked").is_some());
        let asked = web.asked();
        assert_eq!(asked.last().map(String::as_str), Some("https://assets.github-cdn.example/touch-180.png"));
        assert!(!asked.iter().any(|url| url.contains("touch-120")), "the largest first: {asked:?}");
    }

    #[tokio::test]
    async fn a_start_page_that_leaves_the_domain_names_no_symbol() {
        let page = r#"<link rel="apple-touch-icon" href="https://evil.example/touch.png">"#;
        let (web, logos) = with_web(
            FakeWeb::default()
                .page("https://shop.example/", Reply::Redirect("https://other.example/".into()))
                .page("https://other.example/", Reply::Body(page.as_bytes().to_vec()))
                .page("https://evil.example/touch.png", Reply::Body(picture(180, 180, image::ImageFormat::Png))),
        );
        assert_eq!(logos.find("shop.example").await, Ok(None));
        assert!(!web.asked().iter().any(|url| url.contains("evil")));
    }

    #[tokio::test]
    async fn redirects_are_limited_and_only_to_https() {
        let mut web = FakeWeb::default();
        for hop in 0..=MAX_REDIRECTS {
            web = web.page(&format!("https://loop.example/{hop}"), Reply::Redirect(format!("/{}", hop + 1)));
        }
        let (web, logos) =
            with_web(web.page(
                "https://plain.example/apple-touch-icon.png",
                Reply::Redirect("http://plain.example/i.png".into()),
            ));
        let url = Url::parse("https://loop.example/0").expect("url");
        assert_eq!(logos.fetch(url, 1024).await, Ok(None));
        assert_eq!(web.asked().len(), MAX_REDIRECTS + 1, "the first request and three redirects");
        assert_eq!(logos.find("plain.example").await, Ok(None));
        assert!(!web.asked().iter().any(|url| url.starts_with("http:")), "never plain http");
        let local = Url::parse("https://localhost/x").expect("url");
        assert_eq!(logos.fetch(local, 1024).await, Ok(None), "no name on the internet");
    }

    /// The service's web builds without asking anything; tests never use it.
    #[test]
    fn the_system_web_is_built_without_a_request() {
        let web: Arc<dyn LogoWeb> = Arc::new(SystemWeb::new());
        let logos = SenderLogos::new(None, web);
        assert!(logos.on() && !logos.has("github.example"), "on by default, nothing kept");
    }

    #[tokio::test]
    async fn an_unreachable_domain_is_offline_not_without_logo() {
        let (_, logos) = with_web(FakeWeb { offline: true, ..FakeWeb::default() });
        assert_eq!(logos.find("paypal.example").await, Err(WebError::Offline));
        // The DNS answers, the website does not: a domain that only sends mail, without a logo.
        let (web, logos) = with_web(FakeWeb { http_offline: true, ..FakeWeb::default() });
        assert_eq!(logos.find("mailer.example").await, Err(WebError::Unreached));
        assert_eq!(web.asked().len(), 2, "the touch icon and the start page, once each");

        // The DNS fails for BIMI only (SERVFAIL of one zone): the website's symbol still counts.
        let touch = Reply::Body(picture(180, 180, image::ImageFormat::Png));
        let (_, logos) = with_web(
            FakeWeb { dns_offline: true, ..FakeWeb::default() }
                .page("https://zone.example/apple-touch-icon.png", touch),
        );
        assert!(logos.find("zone.example").await.expect("asked").is_some());
    }

    #[test]
    fn a_logo_is_round_on_white() {
        let logo = decoded(&from_raster(&picture(180, 180, image::ImageFormat::Png)).expect("a logo"));
        assert_eq!(logo.get_pixel(0, 0).0[3], 0, "transparent outside the circle");
        assert_eq!(logo.get_pixel(64, 64).0[3], 255);
        assert_eq!(logo.get_pixel(64, 8), &Rgba([255, 255, 255, 255]), "four fifths: a white rim inside the circle");
    }

    #[test]
    fn svgs_without_scripts_references_or_too_many_bytes() {
        assert!(from_svg(&fixture("logo.svg")).is_some());
        let symbol = decoded(&svg_filling(&fixture("logo.svg"), RASTER_FILL_PERCENT).expect("a symbol"));
        assert_eq!(symbol.get_pixel(64, 8), &Rgba([255, 255, 255, 255]), "a website's SVG keeps a white rim");
        assert_eq!(from_svg(&fixture("script.svg")), None, "a script");
        assert_eq!(from_svg(&fixture("external.svg")), None, "a picture from the web");
        let mut large = fixture("logo.svg");
        large.splice(large.len() - 6..large.len() - 6, std::iter::repeat_n(b' ', SVG_MAX_BYTES));
        assert_eq!(from_svg(&large), None, "larger than 32 KiB");
        assert_eq!(from_svg(b"<svg xmlns='http://www.w3.org/2000/svg'"), None, "broken");
        let mut packed = vec![0x1f, 0x8b];
        packed.extend(fixture("logo.svg"));
        assert_eq!(from_svg(&packed), None, "compressed SVG unpacks past every limit");
    }

    #[test]
    fn raster_pictures_within_their_limits() {
        assert!(from_raster(&picture(48, 48, image::ImageFormat::Png)).is_some());
        assert!(from_raster(&picture(200, 100, image::ImageFormat::Jpeg)).is_some());
        assert_eq!(from_raster(&picture(32, 32, image::ImageFormat::Png)), None, "too small");
        // Small on the disk, huge when decoded: refused by its size before it is.
        let bomb = picture(3000, 3000, image::ImageFormat::Png);
        assert!(bomb.len() <= RASTER_MAX_BYTES, "the bomb fits the bytes: {}", bomb.len());
        assert_eq!(from_raster(&bomb), None, "too large when decoded");
        assert_eq!(from_raster(&vec![0; RASTER_MAX_BYTES + 1]), None, "too many bytes");
        assert_eq!(from_raster(b"<html>"), None, "no picture");
    }

    #[tokio::test]
    async fn each_domain_is_asked_once_and_again_only_when_due() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().join("sender-logos");
        let web = Arc::new(FakeWeb::default().page(
            "https://github.example/apple-touch-icon.png",
            Reply::Body(picture(180, 180, image::ImageFormat::Png)),
        ));
        let logos = Arc::new(SenderLogos::new(Some(dir.clone()), Arc::clone(&web) as Arc<dyn LogoWeb>).without_gap());
        let now = Utc::now();
        logos.want("github.example", now);
        logos.want("GitHub.example", now);
        logos.want("none.example", now);
        settled(&logos).await;
        let asked = web.asked();
        assert_eq!(asked.iter().filter(|url| url.contains("github")).count(), 1, "once per domain: {asked:?}");
        assert!(logos.has("github.example") && !logos.has("none.example"));
        assert!(logos.png("github.example").is_some_and(|png| png.starts_with(b"\x89PNG")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(file_name("github.example"))).expect("kept").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Kept on the disk: a new start knows them.
        let again = Arc::new(SenderLogos::new(Some(dir.clone()), Arc::clone(&web) as Arc<dyn LogoWeb>).without_gap());
        assert!(again.has("github.example"));
        again.want("github.example", now + TimeDelta::days(29));
        again.want("none.example", now + TimeDelta::days(6));
        assert!(again.state.lock().expect("lock").queue.is_empty(), "nothing due yet");
        again.want("none.example", now + TimeDelta::days(8));
        let before = web.asked().len();
        settled(&again).await;
        assert!(web.asked().len() > before, "no logo for a week: asked again");

        // Off: nothing is handed out or asked.
        again.set_switch(Arc::new(|| false));
        assert!(!again.has("github.example") && again.png("github.example").is_none());
        let before = web.asked().len();
        again.want("new.example", now);
        settled(&again).await;
        assert_eq!(web.asked().len(), before);
    }

    /// A kept logo not found once stays for one more round; a second miss takes it away. Beyond
    /// [`MAX_DOMAINS`] the domains asked longest ago go.
    #[test]
    fn a_logo_survives_one_miss_and_the_index_stays_bounded() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let (_, logos) = with_web(FakeWeb::default());
        let logos = SenderLogos { dir: Some(tmp.path().to_path_buf()), ..logos };
        let now = Utc::now();
        logos.keep("github.example", Ok(Some(b"png".to_vec())), now);
        logos.keep("github.example", Ok(None), now + TimeDelta::days(31));
        assert!(logos.kept("github.example") && logos.png("github.example").is_some(), "kept once more");
        let asked = logos.with_index(|index| index.domains["github.example"]);
        assert!(!asked.due(now + TimeDelta::days(37)) && asked.due(now + TimeDelta::days(39)), "a week, not a month");
        logos.keep("github.example", Ok(None), now + TimeDelta::days(39));
        assert!(!logos.kept("github.example") && logos.png("github.example").is_none(), "gone after two");
        logos.keep("github.example", Err(WebError::Offline), now);
        assert!(!logos.kept("github.example"), "offline notes nothing");

        // A website that cannot be reached (a network blocking the web): a kept logo stays, and the
        // domain is asked again after a day, not a week or a month.
        logos.keep("shop.example", Ok(Some(b"png".to_vec())), now);
        logos.keep("shop.example", Err(WebError::Unreached), now + TimeDelta::days(31));
        let asked = logos.with_index(|index| index.domains["shop.example"]);
        assert!(logos.kept("shop.example") && logos.png("shop.example").is_some(), "the logo stays");
        assert!(!asked.missed, "not counted as a miss");
        assert!(
            !asked.due(now + TimeDelta::hours(31 * 24 + 23))
                && asked.due(now + TimeDelta::days(32) + TimeDelta::hours(1))
        );
        logos.keep("mailer.example", Err(WebError::Unreached), now);
        let asked = logos.with_index(|index| index.domains["mailer.example"]);
        assert!(!asked.found && asked.due(now + TimeDelta::days(2)), "no logo, asked again after a day");
        logos.keep("mailer.example", Ok(None), now);
        let asked = logos.with_index(|index| index.domains["mailer.example"]);
        assert!(!asked.due(now + TimeDelta::days(2)), "a website without a logo waits a week");

        {
            let mut state = logos.state.lock().expect("lock");
            let index = logos.index_in(&mut state);
            for n in 0..MAX_DOMAINS {
                let asked =
                    Asked { at: now + TimeDelta::seconds(n as i64), found: false, missed: false, unreached: false };
                index.domains.insert(format!("d{n}.example"), asked);
            }
        }
        logos.keep("new.example", Ok(None), now + TimeDelta::days(1));
        let names = logos.with_index(|index| index.domains.keys().cloned().collect::<Vec<_>>());
        assert_eq!(names.len(), MAX_DOMAINS);
        assert!(!names.contains(&"d0.example".to_string()), "the one asked longest ago went");
    }

    /// Waits until the task asking the queue is done.
    async fn settled(logos: &SenderLogos) {
        for _ in 0..400 {
            if !logos.state.lock().expect("lock").running {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
