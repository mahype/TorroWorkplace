//! The OCS client every call shares: one Nextcloud account, its server and app password, the
//! headers OCS asks for, the envelope `{"ocs": {"meta": …, "data": …}}` and what a refusal means.
//!
//! OCS v2 (`/ocs/v2.php/…`) answers with real HTTP status codes, so the status alone tells the
//! errors apart; v1 would always say 200. `OCS-APIRequest: true` is required, without it Nextcloud
//! treats the call as a possible CSRF and refuses it.
//! <https://docs.nextcloud.com/server/latest/developer_manual/client_apis/OCS/ocs-api-overview.html>
//!
//! Two ways to call: [`Nextcloud::get`]/[`Nextcloud::call`] turn every refusal into an [`OcsError`];
//! [`Nextcloud::exchange`] (#643) hands refusals that mean something different per app (Talk's
//! 403 read-only, 412 lobby, 413 too long …) back with their status, carries the response headers
//! (`X-Chat-Last-Given`) and can wait longer than the client's 60 s (a long poll). Addresses for it
//! come from [`Nextcloud::endpoint`], which percent-encodes foreign text into one path segment or
//! the query. [`TalkError`] is the error of Talk's chat calls.

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

    /// The address `<server><path>`, then each of `segments` as one more path segment of its own
    /// (percent-encoded, so a `/`, `?` or `#` in a user id stays inside its segment), then `query`
    /// form-encoded. `path` is this crate's own, made only of fixed parts and checked ids;
    /// `segments` and `query` may hold foreign text. [`OcsError::Other`] for a segment that is
    /// empty, `.` or `..` (it would name another address).
    pub(crate) fn endpoint<V: AsRef<str>>(
        &self,
        path: &str,
        segments: &[&str],
        query: &[(&str, V)],
    ) -> Result<Url, OcsError> {
        let mut url = Url::parse(&format!("{}{path}", self.base))
            .map_err(|_| OcsError::Other("the address of the request could not be built".into()))?;
        if !segments.is_empty() {
            if segments.iter().any(|segment| matches!(*segment, "" | "." | "..")) {
                return Err(OcsError::Other("the request names no valid item".into()));
            }
            url.path_segments_mut()
                .map_err(|_| OcsError::Other("the address of the request could not be built".into()))?
                .extend(segments);
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url)
    }

    /// `method url` (an address from [`Nextcloud::endpoint`]), with `body` as JSON when given and
    /// `wait` as the time limit of this request instead of the client's 60 s (a long poll that
    /// waits on the server needs more). Unlike [`Nextcloud::call`], a refusal that the caller
    /// tells apart itself (400, 403, 404, 405, 412, 413 …) comes back as an [`Answer`] with its
    /// status; only what means the same for every call is an error here: no connection
    /// ([`OcsError::Offline`]), 401, 429, 503 and a redirect to another origin.
    pub(crate) async fn exchange(
        &self,
        method: Method,
        url: Url,
        body: Option<&Value>,
        wait: Option<Duration>,
    ) -> Result<Answer, OcsError> {
        let mut request = self.request_to(method, url);
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(wait) = wait {
            request = request.timeout(wait);
        }
        let response = request.send().await.map_err(unreachable)?;
        let status = response.status().as_u16();
        if matches!(status, 300..=303 | 305..=399 | 401 | 429 | 503) {
            return Err(refusal(status, response.headers()));
        }
        let headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(unreachable)?;
        let data = match status {
            204 | 304 => None,
            _ if bytes.iter().all(u8::is_ascii_whitespace) => None,
            200..=299 => {
                let answer: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| OcsError::Other("Nextcloud sent an answer that is not JSON".into()))?;
                Some(envelope(answer)?)
            }
            // A refusal's body is only looked at for Talk's short error words (`{"error": "age"}`);
            // one that is no envelope is no refusal of its own.
            _ => serde_json::from_slice(&bytes).ok().and_then(|answer| envelope(answer).ok()),
        };
        Ok(Answer { status, data, headers })
    }

    /// A request to `path` on the server with the app password and the headers OCS asks for.
    /// `path` is this crate's own, made only of fixed parts and checked ids.
    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.request_to(method, format!("{}{path}", self.base))
    }

    /// A request to `url` (an address from [`Nextcloud::endpoint`]) with the app password and
    /// `OCS-APIRequest`, but without asking for JSON: for WebDAV and pictures (`files`, #643), whose
    /// answers are no OCS envelope.
    pub(crate) fn plain(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .basic_auth(&self.login, Some(self.password.expose()))
            .header("OCS-APIRequest", "true")
    }

    /// A request to `url` (always on the server: built from [`Nextcloud::server`]) with the app
    /// password and the headers OCS asks for.
    fn request_to(&self, method: Method, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
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

/// What [`Nextcloud::exchange`] got back: the status, the envelope's `data` and the headers.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Answer {
    pub status: u16,
    /// The envelope's `data`; `None` for 204 and 304, for an empty body and for a refusal whose
    /// body is no envelope.
    pub data: Option<Value>,
    pub headers: HeaderMap,
}

impl Answer {
    /// Whether the server did what was asked (2xx, also 304).
    pub fn is_success(&self) -> bool {
        matches!(self.status, 200..=299 | 304)
    }

    /// The `data` of a success; anything else is the error every OCS call gives for its status
    /// (404 [`OcsError::MissingApp`], 403 [`OcsError::Forbidden`] …).
    pub fn success(self) -> Result<Option<Value>, OcsError> {
        if self.is_success() { Ok(self.data) } else { Err(refusal(self.status, &self.headers)) }
    }

    /// The header `name` as a positive number (`X-Chat-Last-Given`, `X-Chat-Last-Common-Read`).
    pub fn number(&self, name: &str) -> Option<u64> {
        let value = self.headers.get(name)?.to_str().ok()?.trim();
        value.parse().ok().filter(|n| *n > 0)
    }

    /// The short error word Talk puts into a refusal (`{"error": "age"}`), for telling refusals
    /// apart only; it never goes into an error's text.
    pub fn error_word(&self) -> Option<&str> {
        self.data.as_ref()?.get("error")?.as_str()
    }
}

/// Why a Talk call failed: the errors of every OCS call, and the refusals Talk gives with a
/// status of their own. As [`OcsError`], no message holds the server's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TalkError {
    /// What every OCS call can say: 401 ([`OcsError::Refused`]), 429 ([`OcsError::Throttled`]),
    /// maintenance, offline, an unreadable answer, and 403 for something only others may do
    /// ([`OcsError::Forbidden`]: someone else's message).
    Ocs(OcsError),
    /// 404: the conversation or the message is gone, the person is no longer a participant, or
    /// Talk itself is not there (the capabilities tell that apart).
    NotFound,
    /// 403: the conversation is read-only (or the person may not write or react in it).
    ReadOnly,
    /// 412: the lobby is on and the person is no moderator.
    Lobby,
    /// 413: the text is longer than the server allows (`spreed.config.chat.max-length`).
    TooLong,
    /// 400 on editing or deleting: the message is older than Talk allows (24 h to edit, 6 h to
    /// delete on servers without `delete-messages-unlimited`).
    TooLate,
    /// Another 400 or a 405: the server does not take this request (an empty text, a system
    /// message, an unknown notification level …).
    Invalid,
}

impl fmt::Display for TalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TalkError::Ocs(error) => error.fmt(f),
            TalkError::NotFound => f.write_str("the conversation or message is not there"),
            TalkError::ReadOnly => f.write_str("the conversation is read-only"),
            TalkError::Lobby => f.write_str("the conversation's lobby is on"),
            TalkError::TooLong => f.write_str("the message is too long"),
            TalkError::TooLate => f.write_str("the message is too old to change"),
            TalkError::Invalid => f.write_str("Talk does not accept this request"),
        }
    }
}

impl std::error::Error for TalkError {}

impl From<OcsError> for TalkError {
    fn from(error: OcsError) -> TalkError {
        TalkError::Ocs(error)
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
pub(crate) fn refusal(status: u16, headers: &HeaderMap) -> OcsError {
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

pub(crate) fn unreachable(err: reqwest::Error) -> OcsError {
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
    fn endpoints_keep_foreign_text_in_its_segment_and_query() {
        let account =
            Nextcloud::new("https://example.org/nextcloud", "anna", Secret::new(PASSWORD)).expect("an account");
        let url = account.endpoint(PATH, &["a/b?c#d"], &[("search", "x&y=z #"), ("ids[]", "0")]).expect("an address");
        assert_eq!(
            url.as_str(),
            "https://example.org/nextcloud/ocs/v2.php/apps/example/api/v1/thing/a%2Fb%3Fc%23d?search=x%26y%3Dz+%23&ids%5B%5D=0"
        );
        let bare = account.endpoint::<&str>(PATH, &[], &[]).expect("an address");
        assert_eq!(bare.as_str(), format!("https://example.org/nextcloud{PATH}"));
        for segment in ["", ".", ".."] {
            assert!(account.endpoint::<&str>(PATH, &[segment], &[]).is_err(), "{segment:?}");
        }
    }

    /// What [`Nextcloud::exchange`] makes of an answer with `status` and `body`.
    async fn exchanged(status: u16, body: &'static str) -> Result<Answer, OcsError> {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(PATH).header("authorization", AUTHORIZATION).header("ocs-apirequest", "true");
                then.status(status).header("X-Chat-Last-Given", "42").body(body);
            })
            .await;
        let account = account(&server);
        let url = account.endpoint::<&str>(PATH, &[], &[]).expect("an address");
        account.exchange(Method::GET, url, None, None).await
    }

    #[tokio::test]
    async fn an_exchange_gives_refusals_back_with_their_status() {
        let refused = r#"{"ocs":{"meta":{"status":"failure","message":"Words"},"data":{"error":"age"}}}"#;
        let answer = exchanged(400, refused).await.expect("an answer");
        assert_eq!((answer.status, answer.error_word()), (400, Some("age")));
        assert!(!answer.is_success());
        assert_eq!(answer.clone().success(), Err(OcsError::Other("Nextcloud answered with HTTP 400".into())));
        let lobby = exchanged(412, "<html>no envelope</html>").await.expect("an answer");
        assert_eq!((lobby.status, lobby.data), (412, None));
        assert_eq!(exchanged(404, "").await.expect("an answer").success(), Err(OcsError::MissingApp));
        assert_eq!(exchanged(401, "").await, Err(OcsError::Refused));
        assert_eq!(exchanged(503, "").await, Err(OcsError::Maintenance));
        assert!(matches!(exchanged(429, "").await, Err(OcsError::Throttled { .. })));
    }

    #[tokio::test]
    async fn an_exchange_reads_data_headers_and_empty_answers() {
        let answer = exchanged(201, r#"{"ocs":{"meta":{},"data":[1]}}"#).await.expect("an answer");
        assert_eq!((answer.status, answer.data.clone()), (201, Some(serde_json::json!([1]))));
        assert_eq!((answer.number("X-Chat-Last-Given"), answer.number("X-Chat-Last-Common-Read")), (Some(42), None));
        assert_eq!(answer.success(), Ok(Some(serde_json::json!([1]))));
        let none = exchanged(304, "").await.expect("an answer");
        assert_eq!((none.is_success(), none.data), (true, None));
        assert!(matches!(exchanged(200, "not json").await, Err(OcsError::Other(_))));
    }

    #[tokio::test]
    async fn an_exchange_waits_as_long_as_asked() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(PATH);
                then.status(200).delay(Duration::from_millis(600)).body(r#"{"ocs":{"data":[]}}"#);
            })
            .await;
        let account = account(&server);
        let url = || account.endpoint::<&str>(PATH, &[], &[]).expect("an address");
        let short = account.exchange(Method::GET, url(), None, Some(Duration::from_millis(100))).await;
        assert_eq!(short, Err(OcsError::Offline), "a request that waits too long counts as lost");
        let long = account.exchange(Method::GET, url(), None, Some(Duration::from_secs(5))).await;
        assert_eq!(long.map(|answer| answer.status), Ok(200));
    }

    #[test]
    fn talk_errors_say_nothing_of_the_server() {
        for error in [
            TalkError::NotFound,
            TalkError::ReadOnly,
            TalkError::Lobby,
            TalkError::TooLong,
            TalkError::TooLate,
            TalkError::Invalid,
            TalkError::from(OcsError::Refused),
        ] {
            assert!(!error.to_string().is_empty());
            assert!(!format!("{error:?}").contains(PASSWORD));
        }
        assert_eq!(TalkError::from(OcsError::Offline).to_string(), OcsError::Offline.to_string());
    }

    #[test]
    fn the_envelope_gives_its_data() {
        let data = envelope(serde_json::json!({"ocs": {"meta": {"statuscode": 200}, "data": [1]}}));
        assert_eq!(data, Ok(serde_json::json!([1])));
        assert!(envelope(serde_json::json!({"data": [1]})).is_err());
        assert!(envelope(serde_json::json!({"ocs": {"meta": {}}})).is_err());
    }
}
