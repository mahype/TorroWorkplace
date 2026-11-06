//! The newest release of the suite on GitHub (issue #411, decision 2): `GET
//! <api>/repos/mahype/TorroWorkplace/releases/latest`, asked daily by [`super::Catalog::watch`] and on
//! `apps.check_updates`, at most every [`CHECK_GAP`] (GitHub limits how often it may be asked without a
//! key).
//!
//! What it answered is kept in `<state>/apps-catalog.json` ([`Stored`]): the version, the release's page
//! and its archives with size and address. A check that fails keeps that state and adds its code
//! (`offline`, `error`), so the catalog still shows the last versions. Only archives whose address lies
//! below the release downloads of the suite's repository are kept ([`Releases::downloads`]): a job never
//! loads from anywhere else.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use torro_core::version::Version;

/// GitHub's API.
pub const GITHUB_API: &str = "https://api.github.com";

/// The path of the newest release below the API.
pub const LATEST: &str = "/repos/mahype/TorroWorkplace/releases/latest";

/// Where the suite's release archives lie; no archive is loaded from elsewhere.
pub const DOWNLOADS: &str = "https://github.com/mahype/TorroWorkplace/releases/download/";

/// The release pages of the suite; another `html_url` is not passed on.
const RELEASE_PAGES: &str = "https://github.com/mahype/TorroWorkplace/releases/";

/// How long after GitHub last answered the catalog answers from that answer.
pub const CHECK_GAP: TimeDelta = TimeDelta::minutes(5);

/// The code of a check that did not reach GitHub.
pub const OFFLINE: &str = "offline";

/// The code of a check whose answer the core could not read (or GitHub refused).
pub const ERROR: &str = "error";

/// How long a check may take as a whole.
const CHECK_WAIT: Duration = Duration::from_secs(30);

/// How long after a check that did not reach GitHub the next one answers the stored state at once, so a
/// computer without network does not wait [`CHECK_WAIT`] on every look at the catalog (review of #411).
const OFFLINE_GAP: TimeDelta = TimeDelta::seconds(30);

/// How long building a connection and each read of a download may take; a download as a whole has no
/// limit (archives are tens of megabytes).
const CONNECT_WAIT: Duration = Duration::from_secs(20);
const READ_WAIT: Duration = Duration::from_secs(60);

/// The largest answer of the API that is read.
const MAX_ANSWER: usize = 2 * 1024 * 1024;

/// The catalog's state as stored in `apps-catalog.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    /// When GitHub last answered with a release (or that there is none yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<DateTime<Utc>>,
    /// When GitHub last answered at all, also with an error: the [`CHECK_GAP`] counts from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<DateTime<Utc>>,
    /// The newest release's version (`0.2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    /// [`OFFLINE`] or [`ERROR`] when the last check failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Stored {
    /// The archive `name` of the release.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// One file of the release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// `torrocalendar-x86_64-unknown-linux-gnu.tar.xz`.
    pub name: String,
    /// In bytes, as GitHub gives it.
    pub size: u64,
    /// Its address, below [`Releases::downloads`].
    pub url: String,
}

/// The release as GitHub answers it (only what is read).
#[derive(Debug, Deserialize)]
struct Latest {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// The releases on GitHub with the stored state.
pub struct Releases {
    /// The API's address; `None`: never asked (a catalog of tests without a server).
    api: Option<String>,
    /// Where archives may be loaded from ([`DOWNLOADS`]; the server of a test).
    downloads: String,
    /// `apps-catalog.json`; `None`: kept in memory only.
    file: Option<PathBuf>,
    http: OnceLock<Option<reqwest::Client>>,
    /// The state, read from [`Releases::file`] on first use.
    state: Mutex<Option<Stored>>,
    /// One check at a time; a second waits and then answers from the first.
    checking: tokio::sync::Mutex<()>,
    /// When a check last did not reach GitHub ([`OFFLINE_GAP`]); in memory only.
    offline_at: Mutex<Option<DateTime<Utc>>>,
}

impl Releases {
    /// GitHub, with the state in the core's state directory.
    pub fn system() -> Releases {
        let file = torro_core::paths::AppPaths::for_app(torro_core::AppId::WORKPLACE)
            .map(|paths| paths.state.join("apps-catalog.json"));
        Releases::new(Some(GITHUB_API.into()), DOWNLOADS.into(), file)
    }

    /// Asks no server and keeps nothing: every check is `offline`.
    pub fn none() -> Releases {
        Releases::new(None, DOWNLOADS.into(), None)
    }

    /// The releases at `api`, with archives below `downloads` and the state in `file`.
    pub fn new(api: Option<String>, downloads: String, file: Option<PathBuf>) -> Releases {
        Releases {
            api,
            downloads,
            file,
            http: OnceLock::new(),
            state: Mutex::new(None),
            checking: tokio::sync::Mutex::new(()),
            offline_at: Mutex::new(None),
        }
    }

    /// The HTTP client of checks and downloads, built on first use; `None` when none could be built.
    pub fn http(&self) -> Option<&reqwest::Client> {
        self.http
            .get_or_init(|| {
                let agent = format!("torroworkplace/{}", Version::current());
                reqwest::Client::builder()
                    .user_agent(agent)
                    .connect_timeout(CONNECT_WAIT)
                    .read_timeout(READ_WAIT)
                    .build()
                    .ok()
            })
            .as_ref()
    }

    /// The state as stored (none before the first check).
    pub fn stored(&self) -> Stored {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.get_or_insert_with(|| self.read()).clone()
    }

    fn read(&self) -> Stored {
        let Some(file) = &self.file else { return Stored::default() };
        std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn keep(&self, stored: Stored) -> Stored {
        if let Some(file) = &self.file
            && let Err(err) = write(file, &stored)
        {
            eprintln!("torroworkplace: cannot write {}: {err}", file.display());
        }
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = Some(stored.clone());
        stored
    }

    /// Asks GitHub for the newest release, unless it answered less than [`CHECK_GAP`] before `now`;
    /// answers the state after it. A failed check keeps the release known before, with its code.
    pub async fn check(&self, now: DateTime<Utc>) -> Stored {
        let _one = self.checking.lock().await;
        let stored = self.stored();
        if stored.asked.is_some_and(|asked| now - asked < CHECK_GAP && asked <= now) {
            return stored;
        }
        let offline_at = *self.offline_at.lock().unwrap_or_else(PoisonError::into_inner);
        if offline_at.is_some_and(|at| now - at < OFFLINE_GAP && at <= now) {
            return stored;
        }
        let answer = match tokio::time::timeout(CHECK_WAIT, self.fetch()).await {
            Ok(answer) => answer,
            Err(_) => Err(OFFLINE),
        };
        *self.offline_at.lock().unwrap_or_else(PoisonError::into_inner) = matches!(answer, Err(OFFLINE)).then_some(now);
        let next = match answer {
            Ok(release) => Stored {
                checked: Some(now),
                asked: Some(now),
                latest: release.as_ref().map(|(version, _, _)| version.clone()),
                release_url: release.as_ref().and_then(|(_, url, _)| url.clone()),
                error: None,
                assets: release.map(|(_, _, assets)| assets).unwrap_or_default(),
            },
            // GitHub answered, but not with a release it could read: it counts as asked.
            Err(ERROR) => Stored { asked: Some(now), error: Some(ERROR.into()), ..stored },
            Err(code) => Stored { error: Some(code.into()), ..stored },
        };
        self.keep(next)
    }

    /// The newest release: its version, page and archives; `Ok(None)` when there is no release yet
    /// (GitHub answers 404). `Err` with [`OFFLINE`] or [`ERROR`].
    async fn fetch(&self) -> Result<Option<(String, Option<String>, Vec<Asset>)>, &'static str> {
        let api = self.api.as_deref().ok_or(OFFLINE)?;
        let http = self.http().ok_or(ERROR)?;
        let response = http
            .get(format!("{}{LATEST}", api.trim_end_matches('/')))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|_| OFFLINE)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(ERROR);
        }
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|_| OFFLINE)? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_ANSWER {
                return Err(ERROR);
            }
        }
        let latest: Latest = serde_json::from_slice(&body).map_err(|_| ERROR)?;
        Ok(Some(self.read_release(latest)?))
    }

    /// What is kept of GitHub's answer: the tag as a version, the page only on the suite's release
    /// pages, archives only from [`Releases::downloads`] with a plain file name.
    fn read_release(&self, latest: Latest) -> Result<(String, Option<String>, Vec<Asset>), &'static str> {
        let version = latest.tag_name.parse::<Version>().map_err(|_| ERROR)?.to_string();
        let page = Some(latest.html_url).filter(|url| below(url, RELEASE_PAGES));
        let plain = |name: &str| {
            !name.is_empty()
                && !name.starts_with('.')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        };
        let assets = latest
            .assets
            .into_iter()
            .filter(|asset| plain(&asset.name) && below(&asset.browser_download_url, &self.downloads))
            .map(|asset| Asset { name: asset.name, size: asset.size, url: asset.browser_download_url })
            .collect();
        Ok((version, page, assets))
    }
}

/// Whether `url` lies below `base`, read as URLs, not as text (review of #411): the same scheme, host and
/// port, a path that begins with the base's, no `.` or `..` segment (also encoded) that would lead out
/// of it, and neither query nor fragment.
fn below(url: &str, base: &str) -> bool {
    let (Ok(url), Ok(base)) = (reqwest::Url::parse(url), reqwest::Url::parse(base)) else { return false };
    let same = url.scheme() == base.scheme()
        && url.host_str() == base.host_str()
        && url.port_or_known_default() == base.port_or_known_default();
    let raw = url.path();
    let wandering = raw.split('/').any(|segment| {
        let lower = segment.to_ascii_lowercase();
        matches!(lower.as_str(), "." | ".." | "%2e" | "%2e%2e" | ".%2e" | "%2e.") || lower.contains("%2f")
    });
    same && !wandering && url.query().is_none() && url.fragment().is_none() && raw.starts_with(base.path())
}

/// Writes `stored` to `file` through a temporary file beside it, so a reader never sees half of it.
fn write(file: &std::path::Path, stored: &Stored) -> std::io::Result<()> {
    let dir = file.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(stored).map_err(std::io::Error::other)?;
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut temporary, &json)?;
    temporary.persist(file).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use httpmock::MockServer;

    /// Review of #411: download addresses are read as URLs; nothing leads out of the suite's releases.
    #[test]
    fn only_addresses_below_the_releases_count() {
        let ok = format!("{DOWNLOADS}v0.2.0/torrotasks-x86_64-pc-windows-msvc.zip");
        assert!(below(&ok, DOWNLOADS));
        for refused in [
            "https://github.com/mahype/TorroWorkplace/releases/download/../../evil/releases/download/x.zip",
            "https://github.com/mahype/TorroWorkplace/releases/download/%2e%2e/%2e%2e/evil/x.zip",
            "https://github.com/mahype/TorroWorkplace/releases/download/v1/x.zip?x=1",
            "https://github.com/mahype/TorroWorkplace/releases/download/v1/x.zip#y",
            "https://github.com.evil.example/mahype/TorroWorkplace/releases/download/v1/x.zip",
            "http://github.com/mahype/TorroWorkplace/releases/download/v1/x.zip",
            "https://github.com:444/mahype/TorroWorkplace/releases/download/v1/x.zip",
            "https://github.com/mahype/TorroWorkplace/releases/downloadx/v1/x.zip",
            "not a url",
        ] {
            assert!(!below(refused, DOWNLOADS), "{refused}");
        }
    }

    /// Review of #411: without network the next look within half a minute answers at once.
    #[tokio::test]
    async fn after_an_offline_check_the_next_answers_at_once() {
        let releases = Releases::new(Some(nowhere()), DOWNLOADS.into(), None);
        let now = Utc::now();
        assert_eq!(releases.check(now).await.error.as_deref(), Some(OFFLINE));
        let started = std::time::Instant::now();
        assert_eq!(releases.check(now + TimeDelta::seconds(10)).await.error.as_deref(), Some(OFFLINE));
        assert!(started.elapsed() < Duration::from_millis(200), "answered from the stored state");
    }

    pub(crate) fn release_json(server: &MockServer, version: &str) -> serde_json::Value {
        let base = server.url("/download/");
        serde_json::json!({
            "tag_name": format!("v{version}"),
            "html_url": format!("https://github.com/mahype/TorroWorkplace/releases/tag/v{version}"),
            "assets": [
                {"name": "torrocalendar-x86_64-unknown-linux-gnu.tar.xz", "size": 2_400_000,
                 "browser_download_url": format!("{base}torrocalendar-x86_64-unknown-linux-gnu.tar.xz")},
                {"name": "torrocalendar-x86_64-unknown-linux-gnu.tar.xz.sha256", "size": 120,
                 "browser_download_url": format!("{base}torrocalendar-x86_64-unknown-linux-gnu.tar.xz.sha256")},
                {"name": "evil.tar.xz", "size": 1, "browser_download_url": "https://elsewhere.example/evil.tar.xz"},
                {"name": "../up.zip", "size": 1, "browser_download_url": format!("{base}up.zip")}
            ]
        })
    }

    fn at(server: &MockServer, file: Option<PathBuf>) -> Releases {
        Releases::new(Some(server.base_url()), server.url("/download/"), file)
    }

    #[tokio::test]
    async fn a_check_keeps_the_version_and_only_the_suites_archives() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method("GET").path(LATEST).header("accept", "application/vnd.github+json");
                then.status(200).json_body(release_json(&server, "0.2.0"));
            })
            .await;
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("state/apps-catalog.json");
        let releases = at(&server, Some(file.clone()));
        let now = Utc::now();
        let stored = releases.check(now).await;
        mock.assert_async().await;
        assert_eq!(stored.latest.as_deref(), Some("0.2.0"));
        assert_eq!(stored.checked, Some(now));
        assert_eq!(stored.error, None);
        assert_eq!(stored.release_url.as_deref(), Some("https://github.com/mahype/TorroWorkplace/releases/tag/v0.2.0"));
        let names: Vec<_> = stored.assets.iter().map(|asset| asset.name.as_str()).collect();
        assert_eq!(
            names,
            ["torrocalendar-x86_64-unknown-linux-gnu.tar.xz", "torrocalendar-x86_64-unknown-linux-gnu.tar.xz.sha256"],
            "no archive from elsewhere, no path in a name"
        );
        // Kept for the next core.
        let again = at(&server, Some(file)).stored();
        assert_eq!(again, stored);
    }

    #[tokio::test]
    async fn within_five_minutes_the_last_answer_counts() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method("GET").path(LATEST);
                then.status(200).json_body(release_json(&server, "0.2.0"));
            })
            .await;
        let releases = at(&server, None);
        let now = Utc::now();
        releases.check(now).await;
        releases.check(now + TimeDelta::minutes(4)).await;
        assert_eq!(mock.calls_async().await, 1, "asked once within five minutes");
        releases.check(now + TimeDelta::minutes(6)).await;
        assert_eq!(mock.calls_async().await, 2);
    }

    /// An address on this computer where nothing listens: the port of a listener just closed.
    pub(crate) fn nowhere() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn without_network_the_last_state_stays_with_offline() {
        let server = MockServer::start_async().await;
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("apps-catalog.json");
        server
            .mock_async(|when, then| {
                when.method("GET").path(LATEST);
                then.status(200).json_body(release_json(&server, "0.2.0"));
            })
            .await;
        let now = Utc::now();
        let first = at(&server, Some(file.clone())).check(now).await;
        // The next core finds no network.
        let releases = Releases::new(Some(nowhere()), server.url("/download/"), Some(file));
        let later = releases.check(now + TimeDelta::hours(1)).await;
        assert_eq!(later.error.as_deref(), Some(OFFLINE));
        assert_eq!((later.latest, later.checked, later.assets), (first.latest, first.checked, first.assets));
        // An offline check does not count as asked: the next one tries again.
        assert_eq!(later.asked, first.asked);
    }

    #[tokio::test]
    async fn an_answer_the_core_cannot_read_is_an_error_and_counts_as_asked() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path(LATEST);
                then.status(403).body("rate limited");
            })
            .await;
        let releases = at(&server, None);
        let now = Utc::now();
        let stored = releases.check(now).await;
        assert_eq!((stored.error.as_deref(), stored.asked, stored.checked), (Some(ERROR), Some(now), None));
    }

    #[tokio::test]
    async fn before_the_first_release_the_check_succeeds_without_one() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path(LATEST);
                then.status(404).json_body(serde_json::json!({"message": "Not Found"}));
            })
            .await;
        let stored = at(&server, None).check(Utc::now()).await;
        assert!(stored.checked.is_some());
        assert_eq!((stored.latest, stored.error, stored.assets.len()), (None, None, 0));
    }

    #[tokio::test]
    async fn a_catalog_without_a_server_is_offline() {
        let stored = Releases::none().check(Utc::now()).await;
        assert_eq!(stored.error.as_deref(), Some(OFFLINE));
    }

    #[test]
    fn the_fixture_reads_as_a_release() {
        let text = include_str!("../../tests/fixtures/catalog/release.json");
        let latest: Latest = serde_json::from_str(text).expect("release");
        let releases = Releases::new(None, DOWNLOADS.into(), None);
        let (version, page, assets) = releases.read_release(latest).expect("read");
        assert_eq!(version, "0.2.0");
        assert!(page.is_some());
        assert!(assets.iter().any(|asset| asset.name == "torrocalendar-aarch64-apple-darwin.app.zip"));
        assert!(assets.iter().any(|asset| asset.name == "torro-reminder-aarch64-apple-darwin.app.zip.sha256"));
    }
}
