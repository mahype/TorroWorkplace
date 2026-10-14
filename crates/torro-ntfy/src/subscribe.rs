//! Asking an ntfy server: is it there ([`Ntfy::health`]), may these topics be read
//! ([`Ntfy::check`]), and the subscription itself ([`Ntfy::subscribe`]), a JSON stream that stays
//! open and is read line by line from [`reqwest::Response::chunk`] ([`Subscription::next`]).
//!
//! The client is built here and follows no redirect, so a login never leaves for another address.
//! A login is sent only where [`Server::may_send_login`] allows it; [`Ntfy::new`] refuses others.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use reqwest::{RequestBuilder, Response};

use crate::NtfyError;
use crate::message::{Event, MessageId, read_event};
use crate::server::{Login, Server, Topic};

/// The longest line of the stream that is read, in bytes without the line end; a longer one is
/// dropped whole, up to its line end (ntfy's own messages stay far below: 4 KiB of text).
pub const MAX_LINE: usize = 64 * 1024;

/// How long the stream may stay silent before it counts as broken ([`NtfyError::Offline`]): ntfy
/// sends a `keepalive` every 45 s by default, so two minutes without a byte means the connection
/// is gone even when no one closed it.
pub const QUIET: Duration = Duration::from_secs(120);

/// How long a connection may take to open.
const CONNECT: Duration = Duration::from_secs(10);

/// How long [`Ntfy::health`] and [`Ntfy::check`] may take in all.
const ASK: Duration = Duration::from_secs(15);

/// Talking to one ntfy server, with or without a login. `Debug` shows no secret ([`Login`]).
#[derive(Debug, Clone)]
pub struct Ntfy {
    http: reqwest::Client,
    server: Server,
    login: Option<Login>,
}

impl Ntfy {
    /// A client for `server` that sends `login` with [`Ntfy::check`] and [`Ntfy::subscribe`]
    /// (never with [`Ntfy::health`], which needs none).
    ///
    /// [`NtfyError::InsecureLogin`] when there is a login and the server is neither `https` nor
    /// this computer ([`Server::may_send_login`]). The client follows no redirect, opens a
    /// connection within 10 s and gives up on a read that waits longer than [`QUIET`].
    pub fn new(server: Server, login: Option<Login>) -> Result<Ntfy, NtfyError> {
        if login.is_some() && !server.may_send_login() {
            return Err(NtfyError::InsecureLogin);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT)
            .read_timeout(QUIET)
            .user_agent(concat!("torro-ntfy/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| NtfyError::Server("The HTTP client for ntfy could not be set up.".into()))?;
        Ok(Ntfy { http, server, login })
    }

    pub fn server(&self) -> &Server {
        &self.server
    }

    /// Whether the server is an ntfy server that is up: `GET /v1/health` answers 200 with
    /// `{"healthy": true}`. Without login. [`NtfyError::Offline`] when it cannot be reached,
    /// [`NtfyError::Server`] for any other answer, a redirect included.
    pub async fn health(&self) -> Result<(), NtfyError> {
        let url = format!("{}/v1/health", self.server);
        let response = self.http.get(url).timeout(ASK).send().await.map_err(unreachable)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(refusal(status));
        }
        let answer: serde_json::Value = response.json().await.map_err(|_| not_ntfy())?;
        match answer.get("healthy").and_then(serde_json::Value::as_bool) {
            Some(true) => Ok(()),
            Some(false) => Err(NtfyError::Server("The ntfy server reports that it is not healthy.".into())),
            None => Err(not_ntfy()),
        }
    }

    /// Whether `topics` may be read, with the login if there is one: polls the latest message
    /// (`GET /<t1>,<t2>/json?poll=1&since=latest`) and reads nothing of the answer.
    ///
    /// [`NtfyError::InvalidTopic`] without topics, [`NtfyError::Auth`] for 401 and 403 (a topic
    /// needs a login, or this one may not read it), [`NtfyError::Busy`] for 429,
    /// [`NtfyError::Offline`], [`NtfyError::Server`] for any other answer.
    pub async fn check(&self, topics: &[Topic]) -> Result<(), NtfyError> {
        let url = format!("{}?poll=1&since=latest", self.stream_url(topics)?);
        let response = self.with_login(self.http.get(url)).timeout(ASK).send().await.map_err(unreachable)?;
        match response.status().as_u16() {
            200 => Ok(()),
            status => Err(refusal(status)),
        }
    }

    /// Opens the stream of `topics` (`GET /<t1>,<t2>/json`), with the login if there is one.
    ///
    /// `since`: the last message seen, to get what came after it (`?since=<id>`); `None` gets
    /// only messages from now on, so the first connection of an account fetches nothing old.
    /// Errors as in [`Ntfy::check`], once the server has answered; later ones come from
    /// [`Subscription::next`].
    pub async fn subscribe(&self, topics: &[Topic], since: Option<&MessageId>) -> Result<Subscription, NtfyError> {
        let mut url = self.stream_url(topics)?;
        if let Some(since) = since {
            url.push_str("?since=");
            url.push_str(since.as_str());
        }
        let response = self.with_login(self.http.get(url)).send().await.map_err(unreachable)?;
        match response.status().as_u16() {
            200 => Ok(Subscription { response, lines: Lines::default(), topics: topics.to_vec(), ended: false }),
            status => Err(refusal(status)),
        }
    }

    /// `<server>/<t1>,<t2>/json`; [`NtfyError::InvalidTopic`] without topics.
    fn stream_url(&self, topics: &[Topic]) -> Result<String, NtfyError> {
        if topics.is_empty() {
            return Err(NtfyError::InvalidTopic);
        }
        let names: Vec<&str> = topics.iter().map(Topic::as_str).collect();
        Ok(format!("{}/{}/json", self.server, names.join(",")))
    }

    fn with_login(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.login {
            None => request,
            Some(Login::Token(token)) => request.bearer_auth(token.expose()),
            Some(Login::Password { user, password }) => request.basic_auth(user, Some(password.expose())),
        }
    }
}

/// An open stream of messages ([`Ntfy::subscribe`]). `Debug` shows how many topics, not which.
pub struct Subscription {
    response: Response,
    lines: Lines,
    topics: Vec<Topic>,
    ended: bool,
}

impl Subscription {
    /// The next event of the stream, waiting for it; `None` when the server closed the stream.
    ///
    /// Lines that are no known event or fail a check ([`read_event`]), that are longer than
    /// [`MAX_LINE`], and messages of a topic that was not subscribed are skipped. A last line
    /// without a line end still counts. [`NtfyError::Offline`] when the connection breaks or stays
    /// silent for [`QUIET`]; then open the stream again with the last id seen.
    pub async fn next(&mut self) -> Result<Option<Event>, NtfyError> {
        loop {
            while let Some(line) = self.lines.pop() {
                if let Some(event) = self.event(&line) {
                    return Ok(Some(event));
                }
            }
            if self.ended {
                return Ok(None);
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => self.lines.push(&chunk),
                Ok(None) => {
                    self.ended = true;
                    self.lines.finish();
                }
                Err(_) => return Err(NtfyError::Offline),
            }
        }
    }

    fn event(&self, line: &[u8]) -> Option<Event> {
        match read_event(line)? {
            Event::Message(message) if !self.topics.contains(&message.topic) => None,
            event => Some(event),
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription").field("topics", &self.topics.len()).field("ended", &self.ended).finish()
    }
}

/// Splits the bytes of the stream into lines, whatever the chunks: a line may end in a later
/// chunk, a chunk may hold many lines. A line longer than [`MAX_LINE`] is never kept in memory
/// whole; it is dropped up to its line end.
#[derive(Debug, Default)]
struct Lines {
    /// The line read so far, without its end.
    current: Vec<u8>,
    /// The current line is too long and is dropped up to its end.
    skipping: bool,
    /// Whole lines not yet taken.
    ready: VecDeque<Vec<u8>>,
}

impl Lines {
    fn push(&mut self, mut chunk: &[u8]) {
        while let Some(end) = chunk.iter().position(|b| *b == b'\n') {
            self.take(&chunk[..end]);
            self.end_line();
            chunk = &chunk[end + 1..];
        }
        self.take(chunk);
    }

    /// The end of the stream: a line without its end still counts.
    fn finish(&mut self) {
        if !self.current.is_empty() {
            self.end_line();
        }
        self.skipping = false;
    }

    fn pop(&mut self) -> Option<Vec<u8>> {
        self.ready.pop_front()
    }

    fn take(&mut self, part: &[u8]) {
        if self.skipping {
            return;
        }
        if self.current.len() + part.len() > MAX_LINE {
            self.current = Vec::new();
            self.skipping = true;
        } else {
            self.current.extend_from_slice(part);
        }
    }

    fn end_line(&mut self) {
        let mut line = std::mem::take(&mut self.current);
        if !std::mem::replace(&mut self.skipping, false) {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if !line.is_empty() {
                self.ready.push_back(line);
            }
        }
    }
}

/// The error for an answer with `status` other than 200. A redirect is not followed and counts as
/// a wrong answer: the address it names is never asked, so no login goes there.
fn refusal(status: u16) -> NtfyError {
    match status {
        401 | 403 => NtfyError::Auth,
        429 => NtfyError::Busy,
        300..=399 => NtfyError::Server(format!("The ntfy server redirected with HTTP {status}; it is not followed.")),
        status => NtfyError::Server(format!("The ntfy server answered with HTTP {status}.")),
    }
}

fn not_ntfy() -> NtfyError {
    NtfyError::Server("The server does not answer like an ntfy server.".into())
}

/// The error for a request that got no answer. The address is left out: it holds topic names.
fn unreachable(err: reqwest::Error) -> NtfyError {
    if err.is_connect() || err.is_timeout() {
        NtfyError::Offline
    } else {
        NtfyError::Server(format!("The ntfy server could not be asked: {}", err.without_url()))
    }
}

#[cfg(test)]
mod tests {
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use torro_core::account::Secret;

    use super::*;

    const MESSAGES: &str = include_str!("../tests/fixtures/messages.ndjson");

    fn topics(names: &[&str]) -> Vec<Topic> {
        names.iter().map(|name| Topic::parse(name).expect("topic")).collect()
    }

    fn server(mock: &MockServer) -> Server {
        Server::parse(&mock.base_url()).expect("the fake server's address")
    }

    fn ntfy(mock: &MockServer, login: Option<Login>) -> Ntfy {
        Ntfy::new(server(mock), login).expect("a client")
    }

    fn ids(events: &[Event]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Message(message) => Some(message.id.as_str()),
                _ => None,
            })
            .collect()
    }

    async fn all(subscription: &mut Subscription) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(event) = subscription.next().await.expect("an event") {
            events.push(event);
        }
        events
    }

    fn split(input: &[u8], size: usize) -> Vec<Vec<u8>> {
        let mut lines = Lines::default();
        for chunk in input.chunks(size) {
            lines.push(chunk);
        }
        lines.finish();
        std::iter::from_fn(|| lines.pop()).collect()
    }

    #[test]
    fn lines_are_whole_whatever_the_chunks() {
        let expected: Vec<Vec<u8>> = MESSAGES.lines().map(|line| line.as_bytes().to_vec()).collect();
        for size in 1..=MESSAGES.len() {
            assert_eq!(split(MESSAGES.as_bytes(), size), expected, "chunks of {size} bytes");
        }
        assert_eq!(split(b"a\r\n\r\nb", 2), [b"a".to_vec(), b"b".to_vec()], "CRLF and empty lines");
        assert_eq!(split(b"last line without end", 4), [b"last line without end".to_vec()]);
    }

    #[test]
    fn a_line_longer_than_the_limit_is_dropped_up_to_its_end() {
        let longest = vec![b'a'; MAX_LINE];
        let too_long = vec![b'b'; MAX_LINE + 1];
        let mut input = Vec::new();
        for line in [&b"first"[..], &too_long, &b"after"[..], &longest, &too_long] {
            input.extend_from_slice(line);
            input.push(b'\n');
        }
        input.extend_from_slice(&too_long);
        for size in [1000, 4096, MAX_LINE, input.len()] {
            let lines = split(&input, size);
            assert_eq!(lines, [b"first".to_vec(), b"after".to_vec(), longest.clone()], "chunks of {size}");
        }
    }

    #[tokio::test]
    async fn health_asks_v1_health_without_login() {
        let mock = MockServer::start_async().await;
        let health = mock
            .mock_async(|when, then| {
                when.method(GET).path("/v1/health").header_missing("authorization");
                then.status(200).body(r#"{"healthy":true}"#);
            })
            .await;
        let login = Login::Token(Secret::new("tk_SECRET"));
        ntfy(&mock, Some(login)).health().await.expect("healthy");
        assert_eq!(health.calls_async().await, 1);
    }

    #[tokio::test]
    async fn an_unhealthy_or_foreign_server_is_an_error() {
        for body in [r#"{"healthy":false}"#, r#"{"ok":true}"#, "<html></html>"] {
            let mock = MockServer::start_async().await;
            mock.mock_async(|when, then| {
                when.method(GET).path("/v1/health");
                then.status(200).body(body);
            })
            .await;
            let err = ntfy(&mock, None).health().await.expect_err(body);
            assert!(matches!(err, NtfyError::Server(_)), "{body}: {err:?}");
        }
        let mock = MockServer::start_async().await;
        mock.mock_async(|when, then| {
            when.method(GET).path("/v1/health");
            then.status(404);
        })
        .await;
        assert_eq!(
            ntfy(&mock, None).health().await,
            Err(NtfyError::Server("The ntfy server answered with HTTP 404.".into()))
        );
    }

    #[tokio::test]
    async fn check_polls_the_latest_message_of_all_topics_with_the_token() {
        let mock = MockServer::start_async().await;
        let check = mock
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/backups_x7Qp2,alerts_9Kd/json")
                    .query_param("poll", "1")
                    .query_param("since", "latest")
                    .header("authorization", "Bearer tk_SECRET");
                then.status(200).body(MESSAGES);
            })
            .await;
        let login = Login::new(None, Secret::new("tk_SECRET"));
        ntfy(&mock, Some(login)).check(&topics(&["backups_x7Qp2", "alerts_9Kd"])).await.expect("readable");
        assert_eq!(check.calls_async().await, 1);
    }

    #[tokio::test]
    async fn check_sends_user_and_password_as_basic() {
        let mock = MockServer::start_async().await;
        // "anna:PASSWORD" in Base64.
        let check = mock
            .mock_async(|when, then| {
                when.method(GET).path("/backups_x7Qp2/json").header("authorization", "Basic YW5uYTpQQVNTV09SRA==");
                then.status(200);
            })
            .await;
        let login = Login::new(Some("anna"), Secret::new("PASSWORD"));
        ntfy(&mock, Some(login)).check(&topics(&["backups_x7Qp2"])).await.expect("readable");
        assert_eq!(check.calls_async().await, 1);
    }

    #[tokio::test]
    async fn refusals_are_auth_or_busy_and_say_no_topic() {
        for (status, expected) in [(401, NtfyError::Auth), (403, NtfyError::Auth), (429, NtfyError::Busy)] {
            let mock = MockServer::start_async().await;
            mock.mock_async(|when, then| {
                when.method(GET).path("/backups_x7Qp2/json");
                then.status(status).body(r#"{"code":40101,"http":401,"error":"unauthorized"}"#);
            })
            .await;
            let client = ntfy(&mock, None);
            assert_eq!(client.check(&topics(&["backups_x7Qp2"])).await, Err(expected.clone()), "check {status}");
            let opened = client.subscribe(&topics(&["backups_x7Qp2"]), None).await;
            assert_eq!(opened.map(|_| ()), Err(expected), "subscribe {status}");
        }
        let mock = MockServer::start_async().await;
        mock.mock_async(|when, then| {
            when.method(GET).path("/backups_x7Qp2/json");
            then.status(500).body("backups_x7Qp2 broke");
        })
        .await;
        let err = ntfy(&mock, None).check(&topics(&["backups_x7Qp2"])).await.expect_err("500");
        assert_eq!(err.code(), "error");
        assert!(!err.to_string().contains("backups_x7Qp2") && !format!("{err:?}").contains("backups_x7Qp2"));
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let mock = MockServer::start_async().await;
        let elsewhere = mock
            .mock_async(|when, then| {
                when.method(GET).path("/elsewhere");
                then.status(200).body(r#"{"healthy":true}"#);
            })
            .await;
        mock.mock_async(|when, then| {
            when.method(GET).path_matches(r"^/(v1/health|backups_x7Qp2/json)$");
            then.status(302).header("Location", "/elsewhere");
        })
        .await;
        let client = ntfy(&mock, Some(Login::Token(Secret::new("tk_SECRET"))));
        assert!(matches!(client.health().await, Err(NtfyError::Server(_))));
        assert!(matches!(client.check(&topics(&["backups_x7Qp2"])).await, Err(NtfyError::Server(_))));
        assert!(matches!(client.subscribe(&topics(&["backups_x7Qp2"]), None).await, Err(NtfyError::Server(_))));
        assert_eq!(elsewhere.calls_async().await, 0, "the redirect went nowhere");
    }

    #[tokio::test]
    async fn the_stream_gives_the_events_of_the_subscribed_topics() {
        let mock = MockServer::start_async().await;
        let stream = mock
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/backups_x7Qp2/json")
                    .query_param_missing("since")
                    .query_param_missing("poll")
                    .header("authorization", "Bearer tk_SECRET");
                then.status(200).body(MESSAGES);
            })
            .await;
        let client = ntfy(&mock, Some(Login::Token(Secret::new("tk_SECRET"))));
        let mut subscription = client.subscribe(&topics(&["backups_x7Qp2"]), None).await.expect("open");
        let events = all(&mut subscription).await;
        assert_eq!(events[0], Event::Open);
        assert_eq!(events[2], Event::Keepalive);
        assert_eq!(ids(&events), ["hwQ2YpKdmg", "h8t4Fgaw7w", "Kp7Vw2XyZa"], "alerts_9Kd was not subscribed");
        assert_eq!(subscription.next().await, Ok(None), "the stream stays ended");
        assert_eq!(stream.calls_async().await, 1);
    }

    #[tokio::test]
    async fn the_stream_goes_on_after_the_last_id_seen() {
        let mock = MockServer::start_async().await;
        let stream = mock
            .mock_async(|when, then| {
                when.method(GET).path("/backups_x7Qp2,alerts_9Kd/json").query_param("since", "h8t4Fgaw7w");
                then.status(200).body(MESSAGES);
            })
            .await;
        let since = MessageId::parse("h8t4Fgaw7w").expect("id");
        let client = ntfy(&mock, None);
        let mut subscription =
            client.subscribe(&topics(&["backups_x7Qp2", "alerts_9Kd"]), Some(&since)).await.expect("open");
        let events = all(&mut subscription).await;
        assert_eq!(ids(&events), ["hwQ2YpKdmg", "h8t4Fgaw7w", "Zq3LmN8pRt", "Kp7Vw2XyZa"]);
        assert_eq!(stream.calls_async().await, 1);
    }

    #[tokio::test]
    async fn a_line_too_long_is_skipped_in_the_stream() {
        let mock = MockServer::start_async().await;
        let long = format!(
            r#"{{"id":"Long000001","time":1760427000,"event":"message","topic":"backups_x7Qp2","message":"{}"}}"#,
            "x".repeat(MAX_LINE)
        );
        let body = format!("{long}\n{MESSAGES}");
        mock.mock_async(|when, then| {
            when.method(GET).path("/backups_x7Qp2/json");
            then.status(200).body(body);
        })
        .await;
        let mut subscription = ntfy(&mock, None).subscribe(&topics(&["backups_x7Qp2"]), None).await.expect("open");
        let events = all(&mut subscription).await;
        assert_eq!(ids(&events), ["hwQ2YpKdmg", "h8t4Fgaw7w", "Kp7Vw2XyZa"]);
    }

    #[tokio::test]
    async fn no_topics_is_an_error_and_an_unreachable_server_offline() {
        let mock = MockServer::start_async().await;
        let client = ntfy(&mock, None);
        assert_eq!(client.check(&[]).await, Err(NtfyError::InvalidTopic));
        assert_eq!(client.subscribe(&[], None).await.map(|_| ()), Err(NtfyError::InvalidTopic));

        // A port on this computer where nothing listens any more.
        let free = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = free.local_addr().expect("address").port();
        drop(free);
        let gone =
            Ntfy::new(Server::parse(&format!("http://127.0.0.1:{port}")).expect("address"), None).expect("client");
        assert_eq!(gone.health().await, Err(NtfyError::Offline));
    }

    #[test]
    fn a_login_over_http_to_another_computer_is_refused() {
        let login = || Some(Login::Token(Secret::new("tk_SECRET")));
        let away = Server::parse("http://push.example.org").expect("address");
        assert_eq!(Ntfy::new(away.clone(), login()).map(|_| ()), Err(NtfyError::InsecureLogin));
        assert!(Ntfy::new(away, None).is_ok(), "without a login http is fine");
        assert!(Ntfy::new(Server::parse("https://push.example.org").expect("address"), login()).is_ok());
        assert!(Ntfy::new(Server::parse("http://localhost:8080").expect("address"), login()).is_ok());
    }

    #[test]
    fn debug_shows_no_secret() {
        let password = Login::Password { user: "anna".into(), password: Secret::new("PASSWORD") };
        for login in [Login::Token(Secret::new("tk_SECRET")), password] {
            let client = Ntfy::new(Server::parse("").expect("address"), Some(login)).expect("client");
            let shown = format!("{client:?}");
            assert!(!shown.contains("tk_SECRET") && !shown.contains("PASSWORD"), "{shown}");
        }
    }
}
