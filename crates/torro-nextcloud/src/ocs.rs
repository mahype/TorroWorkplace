//! The OCS client every call shares: one Nextcloud account, its server and app password, the
//! headers OCS asks for, the envelope `{"ocs": {"meta": …, "data": …}}` and what a refusal means.
//!
//! OCS v2 (`/ocs/v2.php/…`) answers with real HTTP status codes, so the status alone tells the
//! errors apart; v1 would always say 200. `OCS-APIRequest: true` is required, without it Nextcloud
//! treats the call as a possible CSRF and refuses it.
//! <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/OCS/ocs-api-overview.html>

use std::fmt;
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Method, Url, redirect};
use serde_json::Value;
use torro_core::account::Secret;

/// How long to wait after a 429 that names no time in `Retry-After` (Nextcloud's brute-force
/// throttle usually names none).
pub const THROTTLE_WAIT: Duration = Duration::from_secs(60);

/// The longest wait a refusal can ask for; a larger `Retry-After` is cut to this.
pub const MAX_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

/// How many redirects on the account's own origin are followed at most.
const MAX_REDIRECTS: usize = 5;

/// One Nextcloud account as OCS sees it: the server's base address (`https://cloud.example.org`,
/// also in a subdirectory like `https://example.org/nextcloud`), the login name and the app
/// password from Login Flow v2.
///
/// The password goes into the `Authorization` header (Basic) of requests to this server only;
/// the HTTP client follows redirects only on the same origin ([`same_origin_only`]). `Debug` shows
/// no password ([`Secret`]).
#[derive(Debug, Clone)]
pub struct Nextcloud {
    http: reqwest::Client,
    base: String,
    origin: Url,
    login: String,
    password: Secret,
}

/// Why an OCS call failed. No message holds the app password or the server's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcsError {
    /// HTTP 401: the app password was revoked or the account is locked. The person has to sign in
    /// again (and the caller should check for a remote wipe); asking again would only feed
    /// Nextcloud's brute-force protection.
    Refused,
    /// HTTP 403: signed in, but not allowed to do this.
    Forbidden,
    /// HTTP 503: the server is in maintenance mode; ask again later, it is no failure to show.
    Maintenance,
    /// HTTP 429, also Nextcloud's brute-force throttle: wait `retry_after` before the next call
    /// (`Retry-After` in seconds, else [`THROTTLE_WAIT`], at most [`MAX_WAIT`]).
    Throttled { retry_after: Duration },
    /// HTTP 404: the app behind the endpoint is not installed or not enabled for this person.
    MissingApp,
    /// The server could not be reached (no connection, timeout).
    Offline,
    /// Anything else: an unexpected status, a redirect to another origin, an unreadable answer.
    /// The text is this crate's own, never the server's.
    Other(String),
}

impl fmt::Display for OcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OcsError::Refused => f.write_str("Nextcloud no longer accepts the app password"),
            OcsError::Forbidden => f.write_str("Nextcloud does not allow this"),
            OcsError::Maintenance => f.write_str("Nextcloud is in maintenance mode"),
            OcsError::Throttled { retry_after } => {
                write!(f, "Nextcloud asks to wait {} s before asking again", retry_after.as_secs())
            }
            OcsError::MissingApp => f.write_str("the Nextcloud app for this is not installed"),
            OcsError::Offline => f.write_str("Nextcloud could not be reached"),
            OcsError::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for OcsError {}

/// What a successful OCS call returned.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    /// 304: nothing changed since the ETag that was sent.
    NotModified,
    /// 204: nothing to say (the notifications app answers so when no app sends notifications).
    Empty,
    /// 2xx with an envelope: its `data` and the answer's `ETag`, if any.
    Data { data: Value, etag: Option<String> },
}

impl Nextcloud {
    /// The account `login` on `server` with its `app_password`. `server` is the base address as
    /// the account stores it; a trailing `/`, a query and a fragment are dropped.
    /// [`OcsError::Other`] when it is no `http`/`https` address or the HTTP client cannot be built.
    pub fn new(server: &str, login: impl Into<String>, app_password: Secret) -> Result<Nextcloud, OcsError> {
        let mut url = Url::parse(server.trim()).map_err(|_| OcsError::Other("the server is no web address".into()))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.origin().is_tuple()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(OcsError::Other("the server is no http or https address".into()));
        }
        url.set_query(None);
        url.set_fragment(None);
        let base = url.as_str().trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .user_agent("Torro Workplace")
            .redirect(same_origin_only())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| OcsError::Other("the HTTP client could not be set up".into()))?;
        Ok(Nextcloud { http, base, origin: url, login: login.into(), password: app_password })
    }

    /// The server's base address without a trailing `/`.
    pub fn server(&self) -> &str {
        &self.base
    }

    /// `link` as an absolute address, only when it lies on the server's origin; relative links are
    /// read against the server. `None` for anything else (another host, `javascript:`, empty).
    pub(crate) fn own_link(&self, link: &str) -> Option<String> {
        let link = link.trim();
        if link.is_empty() {
            return None;
        }
        let url = self.origin.join(link).ok()?;
        (url.origin().is_tuple() && url.origin() == self.origin.origin()).then(|| url.to_string())
    }

    /// `GET <server><path>`; with `etag`, sent as `If-None-Match` so the server can answer 304.
    pub(crate) async fn get(&self, path: &str, etag: Option<&str>) -> Result<Reply, OcsError> {
        let mut request = self.request(Method::GET, path);
        if let Some(value) = etag.and_then(|etag| HeaderValue::from_str(etag).ok()) {
            request = request.header(header::IF_NONE_MATCH, value);
        }
        self.send(request).await
    }

    /// `method <server><path>`, with `body` as JSON when given.
    pub(crate) async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Reply, OcsError> {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(body);
        }
        self.send(request).await
    }

    /// A request to `path` on the server with the app password and the headers OCS asks for.
    /// `path` is this crate's own, made only of fixed parts and checked ids.
    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .basic_auth(&self.login, Some(self.password.expose()))
            .header("OCS-APIRequest", "true")
            .header(header::ACCEPT, "application/json")
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Reply, OcsError> {
        let response = request.send().await.map_err(unreachable)?;
        let status = response.status().as_u16();
        match status {
            204 => Ok(Reply::Empty),
            304 => Ok(Reply::NotModified),
            200..=299 => {
                let etag = response.headers().get(header::ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
                let answer: Value = response
                    .json()
                    .await
                    .map_err(|_| OcsError::Other("Nextcloud sent an answer that is not JSON".into()))?;
                Ok(Reply::Data { data: envelope(answer)?, etag })
            }
            status => Err(refusal(status, response.headers())),
        }
    }
}

/// The `data` of an OCS envelope `{"ocs": {"meta": …, "data": …}}`.
pub(crate) fn envelope(answer: Value) -> Result<Value, OcsError> {
    match answer {
        Value::Object(mut outer) => match outer.remove("ocs") {
            Some(Value::Object(mut ocs)) => {
                ocs.remove("data").ok_or_else(|| OcsError::Other("Nextcloud sent an answer without data".into()))
            }
            _ => Err(OcsError::Other("Nextcloud sent an answer that is no OCS answer".into())),
        },
        _ => Err(OcsError::Other("Nextcloud sent an answer that is no OCS answer".into())),
    }
}

/// The error for a refused request with `status`.
fn refusal(status: u16, headers: &HeaderMap) -> OcsError {
    match status {
        401 => OcsError::Refused,
        403 => OcsError::Forbidden,
        404 => OcsError::MissingApp,
        429 => OcsError::Throttled { retry_after: retry_after(headers) },
        503 => OcsError::Maintenance,
        300..=399 => OcsError::Other("Nextcloud sent the request to another address".into()),
        status => OcsError::Other(format!("Nextcloud answered with HTTP {status}")),
    }
}

/// `Retry-After` in seconds, at most [`MAX_WAIT`]; [`THROTTLE_WAIT`] without a number there (an
/// HTTP date is not read: Nextcloud sends seconds when it sends anything).
fn retry_after(headers: &HeaderMap) -> Duration {
    let seconds = headers.get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok());
    seconds.map(Duration::from_secs).unwrap_or(THROTTLE_WAIT).min(MAX_WAIT)
}

/// Follows a redirect only on the origin of the first request, at most [`MAX_REDIRECTS`] times;
/// any other is not followed and its 3xx comes back as the answer. That keeps the app password on
/// the account's server: reqwest drops `Authorization` on a redirect to another host, but a
/// request there would still tell that host what was asked.
pub(crate) fn same_origin_only() -> redirect::Policy {
    redirect::Policy::custom(|attempt| {
        let own = attempt.previous().first().map(Url::origin);
        let next = attempt.url().origin();
        let same = next.is_tuple() && own.as_ref() == Some(&next);
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if same {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn unreachable(err: reqwest::Error) -> OcsError {
    if err.is_connect() || err.is_timeout() {
        OcsError::Offline
    } else {
        OcsError::Other(format!("Nextcloud could not be asked: {}", err.without_url()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;

    use super::*;

    /// The app password every test uses; no test may find it anywhere but the header.
    pub(crate) const PASSWORD: &str = "Fake-App-Pass-Word1";

    /// `Basic base64("anna:Fake-App-Pass-Word1")`.
    pub(crate) const AUTHORIZATION: &str = "Basic YW5uYTpGYWtlLUFwcC1QYXNzLVdvcmQx";

    pub(crate) fn account(server: &MockServer) -> Nextcloud {
        Nextcloud::new(&server.base_url(), "anna", Secret::new(PASSWORD)).expect("an account")
    }

    const PATH: &str = "/ocs/v2.php/apps/example/api/v1/thing";

    async fn answer(status: u16, headers: &[(&str, &str)]) -> Result<Reply, OcsError> {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(PATH);
                let mut then = then.status(status).body("{}");
                for (name, value) in headers {
                    then = then.header(*name, *value);
                }
            })
            .await;
        account(&server).get(PATH, None).await
    }

    #[tokio::test]
    async fn sends_the_ocs_headers_and_the_password_only_in_authorization() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(PATH)
                    .header("authorization", AUTHORIZATION)
                    .header("ocs-apirequest", "true")
                    .header("accept", "application/json")
                    .query_param_missing("password");
                then.status(200)
                    .header("ETag", "\"abc\"")
                    .body(r#"{"ocs":{"meta":{"status":"ok","statuscode":200,"message":"OK"},"data":{"x":1}}}"#);
            })
            .await;
        let reply = account(&server).get(PATH, None).await.expect("an answer");
        assert_eq!(reply, Reply::Data { data: serde_json::json!({"x": 1}), etag: Some("\"abc\"".into()) });
        assert_eq!(mock.calls_async().await, 1);
    }

    #[tokio::test]
    async fn statuses_become_their_errors() {
        assert_eq!(answer(401, &[]).await, Err(OcsError::Refused));
        assert_eq!(answer(403, &[]).await, Err(OcsError::Forbidden));
        assert_eq!(answer(404, &[]).await, Err(OcsError::MissingApp));
        assert_eq!(answer(503, &[]).await, Err(OcsError::Maintenance));
        assert_eq!(answer(304, &[]).await, Ok(Reply::NotModified));
        assert_eq!(answer(204, &[]).await, Ok(Reply::Empty));
        assert!(matches!(answer(500, &[]).await, Err(OcsError::Other(_))));
    }

    #[tokio::test]
    async fn a_throttle_says_how_long_to_wait() {
        let throttled = |seconds: u64| Err(OcsError::Throttled { retry_after: Duration::from_secs(seconds) });
        assert_eq!(answer(429, &[("Retry-After", "30")]).await, throttled(30));
        assert_eq!(answer(429, &[]).await, throttled(THROTTLE_WAIT.as_secs()));
        assert_eq!(answer(429, &[("Retry-After", "9999999")]).await, throttled(MAX_WAIT.as_secs()));
    }

    #[tokio::test]
    async fn an_unreachable_server_is_offline() {
        // A port that was free a moment ago and has nobody listening now (httpmock keeps its
        // servers running in a pool, so a dropped one would still answer).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        let address = format!("http://{}", listener.local_addr().expect("its address"));
        drop(listener);
        let account = Nextcloud::new(&address, "anna", Secret::new(PASSWORD)).expect("an account");
        assert_eq!(account.get(PATH, None).await, Err(OcsError::Offline));
    }

    #[tokio::test]
    async fn a_redirect_to_another_origin_is_not_followed() {
        let server = MockServer::start_async().await;
        let elsewhere = MockServer::start_async().await;
        let away = elsewhere
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).body(r#"{"ocs":{"data":{}}}"#);
            })
            .await;
        let target = format!("{}{PATH}", elsewhere.base_url());
        server
            .mock_async(|when, then| {
                when.method(GET).path(PATH);
                then.status(302).header("Location", target.as_str());
            })
            .await;
        let reply = account(&server).get(PATH, None).await;
        assert!(matches!(reply, Err(OcsError::Other(_))), "{reply:?}");
        assert_eq!(away.calls_async().await, 0, "nothing goes to another origin");
    }

    #[tokio::test]
    async fn a_redirect_on_the_same_origin_is_followed() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(PATH);
                then.status(302).header("Location", "/moved");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/moved").header("authorization", AUTHORIZATION);
                then.status(200).body(r#"{"ocs":{"data":[]}}"#);
            })
            .await;
        let reply = account(&server).get(PATH, None).await;
        assert_eq!(reply, Ok(Reply::Data { data: serde_json::json!([]), etag: None }));
    }

    #[test]
    fn the_app_password_never_shows_in_debug_or_errors() {
        let account = Nextcloud::new("https://cloud.example.org/", "anna", Secret::new(PASSWORD)).expect("an account");
        assert!(!format!("{account:?}").contains(PASSWORD));
        assert_eq!(account.server(), "https://cloud.example.org");
        for error in [
            OcsError::Refused,
            OcsError::Forbidden,
            OcsError::Maintenance,
            OcsError::Throttled { retry_after: THROTTLE_WAIT },
            OcsError::MissingApp,
            OcsError::Offline,
        ] {
            assert!(!format!("{error} {error:?}").contains(PASSWORD));
        }
    }

    #[test]
    fn only_web_addresses_are_servers() {
        let new = |server: &str| Nextcloud::new(server, "anna", Secret::new(PASSWORD));
        assert!(new("ftp://cloud.example.org").is_err());
        assert!(new("cloud.example.org").is_err());
        assert!(new("https://user:pw@cloud.example.org").is_err());
        let sub = new("https://example.org/nextcloud/?x=1#y").expect("an account");
        assert_eq!(sub.server(), "https://example.org/nextcloud");
    }

    #[test]
    fn links_stay_on_the_server() {
        let account = Nextcloud::new("https://cloud.example.org", "anna", Secret::new(PASSWORD)).expect("an account");
        let own = |link: &str| account.own_link(link);
        assert_eq!(
            own("https://cloud.example.org/apps/files/?fileid=1"),
            Some("https://cloud.example.org/apps/files/?fileid=1".into())
        );
        assert_eq!(own("/call/abcd1234"), Some("https://cloud.example.org/call/abcd1234".into()));
        assert_eq!(own("https://evil.example.com/apps/files"), None);
        assert_eq!(own("//evil.example.com/apps/files"), None);
        assert_eq!(own("http://cloud.example.org/apps/files"), None);
        assert_eq!(own("javascript:alert(1)"), None);
        assert_eq!(own(""), None);
    }

    #[test]
    fn the_envelope_gives_its_data() {
        let data = envelope(serde_json::json!({"ocs": {"meta": {"statuscode": 200}, "data": [1]}}));
        assert_eq!(data, Ok(serde_json::json!([1])));
        assert!(envelope(serde_json::json!({"data": [1]})).is_err());
        assert!(envelope(serde_json::json!({"ocs": {"meta": {}}})).is_err());
    }
}
