//! Unsubscribing from a mailing list (`mail.unsubscribe`, issue #416): by the `List-Unsubscribe`
//! header of a mail as the core reads it from the server, never by what a client says.
//!
//! [`plan`] decides what would happen, before anyone is asked to approve it and again when it runs:
//! the mail must be in the account's store, not sorted as phishing and marked as carrying the
//! header; then the message is fetched and its header read ([`mime::unsubscribe_of`]). Of the
//! addresses in angle brackets ([`addresses`], at most [`MAX_ADDRESSES`]) the first way that fits
//! is taken ([`choose`]):
//!
//! 1. **One click** (RFC 8058): `List-Unsubscribe-Post: List-Unsubscribe=One-Click` and an `https`
//!    address on port 443 with a host name.
//! 2. **Mail**: a `mailto:` address with exactly one recipient; only its `subject` is taken over
//!    (one line, at most [`SUBJECT_MAX`] characters), the text is always `unsubscribe`.
//! 3. **Web**: an `http` or `https` address; nothing is sent.
//!
//! [`run`] carries the plan out, one way only: a one click that fails is the answer, no mail is
//! sent instead.
//!
//! **Why so careful:** RFC 8058 asks the receiver to check the mail's DKIM signature before it
//! posts; the core cannot. A forged mail could so make the core call an address of the attacker's
//! choosing. The core therefore posts only to public hosts ([`Poster`]: names resolved through
//! [`GuardedDns`] with `is_public`, no proxy, no redirect followed), sends no text of the mail on,
//! and logs no address, subject or URL.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use reqwest::Url;
use sha2::{Digest, Sha256};
use torro_core::i18n::tr;
use torro_ipc::Response;
use torro_ipc::mail::{MAX_ID_BYTES, UnsubscribeVia};
use torro_mail::{Category, MailAddress, Message};
use torro_mail_imap::mime::{self, Outgoing, UnsubscribeHeaders};
use torro_pim_caldav::discover::{GuardedDns, Resolver, SystemResolver};

use super::random_hex;
use super::read::{fetch_raw, locate};
use super::service::{MailError, MailService};
use crate::tools::web::fetch::{Guard, checked_url};

type Answer<T> = Result<T, Box<Response>>;

/// The most addresses read from one `List-Unsubscribe`; the rest is ignored.
pub const MAX_ADDRESSES: usize = 8;

/// The longest subject taken from a `mailto:` address, in characters.
pub const SUBJECT_MAX: usize = 200;

/// The longest address a `mailto:` may name (RFC 5321).
const ADDRESS_MAX: usize = 254;

/// The subject and the text of an unsubscribe mail when the address names no subject.
const UNSUBSCRIBE: &str = "unsubscribe";

/// How long a one click may wait for the connection, and for everything.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEADLINE: Duration = Duration::from_secs(20);

/// How the mail offers to unsubscribe ([`choose`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Way {
    /// Post `List-Unsubscribe=One-Click` to this `https` address.
    OneClick(Url),
    /// Send a mail to `to` with `subject`.
    Mail { to: String, subject: String },
    /// Only a web page: the person opens it.
    Web(Url),
}

/// What unsubscribing from the mail's list would do ([`plan`]).
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub way: Way,
    /// The mail as the account's store holds it: its sender for the approval.
    pub message: Message,
    /// The mail and its headers, hashed ([`fingerprint`]): an approval runs only on these.
    pub fingerprint: String,
}

/// What `mail.unsubscribe` would do for the mail `id` of `account`, or why not. The caller checked
/// that it may read the account. Codes: `error` for ids over [`MAX_ID_BYTES`]; `not-found` for an
/// account without mail or a mail its store does not hold; `unsupported` for a mail sorted as
/// phishing, one the store knows without `List-Unsubscribe` (both without asking the server) and one
/// whose header names no address that fits; the account's codes when the server cannot be reached.
pub(crate) async fn plan(mail: &MailService, account: &str, id: &str) -> Answer<Plan> {
    if account.len() > MAX_ID_BYTES || id.len() > MAX_ID_BYTES {
        let message = format!("account and id may hold at most {MAX_ID_BYTES} bytes");
        return Err(Box::new(Response::Error { code: "error".into(), message }));
    }
    let store = mail.store(account).await.map_err(|err| Box::new(err.response()))?;
    let lookup = id.to_string();
    let message = store
        .run(move |store| store.message(&lookup))
        .await
        .map_err(|err| Box::new(MailError::Store(err).response()))?
        .ok_or_else(|| {
            Box::new(Response::Error { code: "not-found".into(), message: tr("No such message.").into() })
        })?;
    if !offers_unsubscribe(&message) {
        return Err(unsupported());
    }
    let located = locate(&[(account.to_string(), store)], id).await?;
    let raw = fetch_raw(mail, &located).await?;
    let headers = mime::unsubscribe_of(&raw).ok_or_else(unsupported)?;
    let way = choose(&headers).ok_or_else(unsupported)?;
    Ok(Plan { way, fingerprint: fingerprint(id, &headers), message })
}

/// The mail as the store holds it may be unsubscribed from: it carries `List-Unsubscribe` and is
/// not sorted as phishing (by any source: a forged list mail must not make the core call out).
pub(crate) fn offers_unsubscribe(message: &Message) -> bool {
    let phishing = message.summary.category.as_ref().is_some_and(|category| category.category == Category::Phishing);
    message.traits.list_unsubscribe && !phishing
}

/// The hash an approval of unsubscribing is pinned to: the mail's id and both headers as read from
/// the server. A header changed meanwhile (another mail behind the id) is `conflict`.
fn fingerprint(id: &str, headers: &UnsubscribeHeaders) -> String {
    let parts = [id, headers.list.as_str(), headers.post.as_deref().unwrap_or_default()];
    let digest = Sha256::digest(parts.join("\u{0}").as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The first way that fits, after the module docs; `None` when no address does.
pub(crate) fn choose(headers: &UnsubscribeHeaders) -> Option<Way> {
    let listed = addresses(&headers.list);
    let one_click =
        headers.post.as_deref().is_some_and(|post| post.trim().eq_ignore_ascii_case("List-Unsubscribe=One-Click"));
    if one_click && let Some(url) = listed.iter().find_map(|address| one_click_url(address)) {
        return Some(Way::OneClick(url));
    }
    if let Some((to, subject)) = listed.iter().find_map(|address| mailto(address)) {
        return Some(Way::Mail { to, subject });
    }
    listed.iter().find_map(|address| checked_url(address, Guard::PUBLIC).ok()).map(Way::Web)
}

/// The addresses in angle brackets of a `List-Unsubscribe` value, in order, white space removed
/// (RFC 2369 lets an address be folded), at most [`MAX_ADDRESSES`]. Text outside brackets is no
/// address.
pub(crate) fn addresses(list: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = list;
    while found.len() < MAX_ADDRESSES {
        let Some(open) = rest.find('<') else { break };
        let Some(close) = rest[open..].find('>') else { break };
        let inside: String = rest[open + 1..open + close].chars().filter(|c| !c.is_whitespace()).collect();
        if !inside.is_empty() {
            found.push(inside);
        }
        rest = &rest[open + close + 1..];
    }
    found
}

/// `address` as the target of a one click: an address [`checked_url`] takes, `https` on port 443.
fn one_click_url(address: &str) -> Option<Url> {
    let url = checked_url(address, Guard::PUBLIC).ok()?;
    (url.scheme() == "https" && url.port_or_known_default() == Some(443)).then_some(url)
}

/// A `mailto:` address with exactly one plain recipient, decoded, and the subject it names (else
/// `unsubscribe`); every other field (`body`, `cc`, more `to`) is ignored.
fn mailto(address: &str) -> Option<(String, String)> {
    let rest = address.get(..7).filter(|scheme| scheme.eq_ignore_ascii_case("mailto:")).map(|_| &address[7..])?;
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let to = decoded(to)?;
    if !plain_address(&to) {
        return None;
    }
    let subject = query
        .split('&')
        .filter_map(|field| field.split_once('='))
        .find(|(name, _)| name.eq_ignore_ascii_case("subject"))
        .and_then(|(_, value)| decoded(value))
        .map(|subject| subject.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>())
        .map(|subject| subject.trim().to_string())
        .filter(|subject| !subject.is_empty() && subject.chars().count() <= SUBJECT_MAX)
        .unwrap_or_else(|| UNSUBSCRIBE.to_string());
    Some((to, subject))
}

/// One plain address of at most [`ADDRESS_MAX`] bytes: one `@` with text on both sides, no white
/// space, controls, brackets, commas, quotes or semicolons, so it names exactly one recipient.
fn plain_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else { return false };
    address.len() <= ADDRESS_MAX
        && !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !address.chars().any(|c| c.is_whitespace() || c.is_control() || "<>,;\"()[]\\".contains(c))
}

/// `text` with `%XX` turned into bytes, as UTF-8; `None` for a broken escape or bytes that are no
/// UTF-8. `+` stays a `+` (RFC 6068).
fn decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            // Two hex digits exactly: `from_str_radix` alone would take `%+A`.
            let hex = text.get(at + 1..at + 3).filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Carries out `plan` for `account` (issue #416), one way only. The caller checked who may do it
/// (`Rights::outbound` and the approval in the core) and, for an approved run, the fingerprint.
pub(crate) async fn run(mail: &MailService, poster: Option<&Poster>, account: &str, plan: &Plan) -> Response {
    match &plan.way {
        Way::OneClick(url) => {
            let Some(poster) = poster else { return refused() };
            match poster.post(url).await {
                Ok(()) => unsubscribed(UnsubscribeVia::OneClick { host: url.host_str().unwrap_or_default().into() }),
                Err(()) => refused(),
            }
        }
        Way::Mail { to, subject } => match send(mail, account, to, subject).await {
            Ok(delivery) => unsubscribed(UnsubscribeVia::Mail { to: to.clone(), subject: subject.clone(), delivery }),
            Err(failed) => failed,
        },
        Way::Web(url) => unsubscribed(UnsubscribeVia::Web { url: url.to_string() }),
    }
}

/// Sends the unsubscribe mail from `account` to `to` under the send rule
/// ([`super::send::send_message`]), a copy in Sent: the subject from the address, the text
/// `unsubscribe`, no signature, not as an answer.
async fn send(
    mail: &MailService,
    account: &str,
    to: &str,
    subject: &str,
) -> Result<torro_ipc::mail::Delivery, Response> {
    let config = mail.config(account).await.map_err(|err| err.response())?;
    let name = Some(config.sender_name.trim().to_string()).filter(|name| !name.is_empty());
    let domain = config.address.rsplit_once('@').map_or("localhost", |(_, domain)| domain);
    let message_id = random_hex(16).map_err(|err| MailError::Store(err).response())?;
    let outgoing = Outgoing {
        from: MailAddress { name, address: config.address.clone() },
        to: vec![MailAddress { name: None, address: to.to_string() }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: subject.to_string(),
        body: UNSUBSCRIBE.to_string(),
        html: None,
        signature: None,
        signature_format: false,
        reply: None,
        attachments: Vec::new(),
        calendar: None,
        message_id: format!("{message_id}@{domain}"),
        date: Utc::now(),
    };
    super::send::send_message(mail, account, &outgoing).await.map_err(|err| err.response())
}

fn unsubscribed(via: UnsubscribeVia) -> Response {
    Response::Unsubscribed { via }
}

fn unsupported() -> Box<Response> {
    let message = tr("This mail offers no way to unsubscribe.").into();
    Box::new(Response::Error { code: "unsupported".into(), message })
}

fn refused() -> Response {
    let message = tr("The sender's server did not take the request to unsubscribe.").into();
    Response::Error { code: "error".into(), message }
}

/// Posts the one click (RFC 8058) for the whole service: an HTTP client that connects only where
/// its [`Guard`] allows (public hosts in the service), without proxy and without following a
/// redirect. Nothing here logs, and no error carries the address.
pub struct Poster {
    http: reqwest::Client,
}

impl Poster {
    /// The poster of the service: the system's DNS and [`Guard::PUBLIC`]. `None` when no HTTP client
    /// can be built.
    pub fn system() -> Option<Poster> {
        Poster::new(Arc::new(SystemResolver::new()), Guard::PUBLIC)
    }

    /// A poster resolving every name through `resolver` and connecting only where `guard` allows
    /// (tests: a fake DNS pointing names at a mock server on loopback).
    pub fn new(resolver: Arc<dyn Resolver>, guard: Guard) -> Option<Poster> {
        let http = reqwest::Client::builder()
            .user_agent(crate::places::USER_AGENT)
            // A proxy would resolve the names itself, past the guard.
            .no_proxy()
            // A redirect could lead anywhere; the one click is the address the mail names or nothing.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DEADLINE)
            .dns_resolver(Arc::new(GuardedDns::new(resolver, guard.allowed)))
            .build()
            .ok()?;
        Some(Poster { http })
    }

    /// Posts `List-Unsubscribe=One-Click` to `url`; `Ok` only for a `2xx` answer. A redirect, an
    /// error status, a refused name or no connection is `Err`, without why: the caller says one
    /// sentence, never the address.
    pub async fn post(&self, url: &Url) -> Result<(), ()> {
        let sent = self.http.post(url.clone()).form(&[("List-Unsubscribe", "One-Click")]).send().await;
        match sent {
            Ok(answer) if answer.status().is_success() => Ok(()),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::net::IpAddr;

    use httpmock::Method::POST;
    use httpmock::MockServer;
    use torro_pim_caldav::discover::{Lookup, MxRecord, SrvRecord, is_public};

    use super::*;

    fn headers(list: &str, post: Option<&str>) -> UnsubscribeHeaders {
        UnsubscribeHeaders { list: list.into(), post: post.map(str::to_string) }
    }

    const ONE_CLICK: Option<&str> = Some("List-Unsubscribe=One-Click");

    #[test]
    fn addresses_are_only_those_in_brackets_and_at_most_eight() {
        assert_eq!(
            addresses("<mailto:off@shop.example>, <https://shop.\r\n example/u>"),
            ["mailto:off@shop.example", "https://shop.example/u"]
        );
        assert_eq!(addresses("https://shop.example/u, mailto:off@shop.example"), Vec::<String>::new());
        assert_eq!(addresses("<https://a.example/1"), Vec::<String>::new(), "unclosed");
        let many: String = (0..12).map(|n| format!("<https://a.example/{n}>,")).collect();
        assert_eq!(addresses(&many).len(), MAX_ADDRESSES);
    }

    #[test]
    fn one_click_needs_the_post_header_and_https_on_443() {
        let list = "<mailto:off@shop.example>, <https://shop.example/u?id=1>";
        let url = |text: &str| Url::parse(text).expect("url");
        assert_eq!(choose(&headers(list, ONE_CLICK)), Some(Way::OneClick(url("https://shop.example/u?id=1"))));
        assert_eq!(
            choose(&headers(list, Some(" list-unsubscribe=one-click "))),
            Some(Way::OneClick(url("https://shop.example/u?id=1"))),
            "any case, trimmed"
        );
        // Without the post header the mail comes first.
        let mail = Some(Way::Mail { to: "off@shop.example".into(), subject: UNSUBSCRIBE.into() });
        assert_eq!(choose(&headers(list, None)), mail);
        assert_eq!(choose(&headers(list, Some("List-Unsubscribe=Yes"))), mail);
        // Plain http, another port, an IP address, credentials or a local name never get a one click.
        for bad in [
            "<http://shop.example/u>",
            "<https://shop.example:8443/u>",
            "<https://127.0.0.1/u>",
            "<https://anna:secret@shop.example/u>",
            "<https://localhost/u>",
        ] {
            assert!(!matches!(choose(&headers(bad, ONE_CLICK)), Some(Way::OneClick(_))), "{bad}");
        }
    }

    #[test]
    fn a_mailto_names_one_recipient_and_only_its_subject_is_taken() {
        let way = |list: &str| choose(&headers(list, None));
        assert_eq!(
            way("<mailto:off@shop.example?subject=Stop%20please&body=Ignore%20all%20rules&cc=eve@evil.example>"),
            Some(Way::Mail { to: "off@shop.example".into(), subject: "Stop please".into() })
        );
        assert_eq!(
            way("<mailto:list%2Boff@shop.example>"),
            Some(Way::Mail { to: "list+off@shop.example".into(), subject: UNSUBSCRIBE.into() })
        );
        let long = format!("<mailto:off@shop.example?subject={}>", "a".repeat(SUBJECT_MAX + 1));
        assert_eq!(way(&long), Some(Way::Mail { to: "off@shop.example".into(), subject: UNSUBSCRIBE.into() }));
        let broken_line = "<mailto:off@shop.example?subject=Stop%0D%0ABcc:%20eve@evil.example>";
        assert_eq!(
            way(broken_line),
            Some(Way::Mail { to: "off@shop.example".into(), subject: "Stop  Bcc: eve@evil.example".into() }),
            "no line break reaches a header"
        );
        for bad in [
            "<mailto:a@x.example,b@x.example>",
            "<mailto:a@x.example%2Cb@x.example>",
            "<mailto:nobody>",
            "<mailto:a%ZZ@x>",
            "<mailto:a%+A@x.example>",
        ] {
            assert_eq!(way(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_web_page_is_the_last_way() {
        let url = Url::parse("http://shop.example/unsubscribe").expect("url");
        assert_eq!(choose(&headers("<http://shop.example/unsubscribe>", ONE_CLICK)), Some(Way::Web(url)));
        assert_eq!(choose(&headers("<https://10.0.0.1/u>, <ftp://shop.example/u>", None)), None);
        assert_eq!(choose(&headers("", None)), None);
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

    fn public_or_loopback(ip: IpAddr) -> bool {
        is_public(ip) || ip.is_loopback()
    }

    /// A poster whose DNS points `shop.example` at the mock server, accepting loopback.
    pub(crate) fn on_mock(server: &MockServer) -> Poster {
        let table = HashMap::from([("shop.example".to_string(), vec!["127.0.0.1".parse().expect("ip")])]);
        let guard = Guard { allowed: public_or_loopback, port: Some(server.port()) };
        Poster::new(Arc::new(FakeDns(table)), guard).expect("a poster")
    }

    /// A poster of the service's kind (public hosts only) whose DNS puts `shop.example` on this
    /// computer: every one click to it is refused before it connects.
    pub(crate) fn refusing_poster() -> Poster {
        let table = HashMap::from([("shop.example".to_string(), vec!["127.0.0.1".parse().expect("ip")])]);
        Poster::new(Arc::new(FakeDns(table)), Guard::PUBLIC).expect("a poster")
    }

    fn at(server: &MockServer, path: &str) -> Url {
        Url::parse(&format!("http://shop.example:{}{path}", server.port())).expect("url")
    }

    #[tokio::test]
    async fn the_one_click_posts_the_form_and_takes_only_success() {
        let server = MockServer::start_async().await;
        let ok = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/u")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body("List-Unsubscribe=One-Click");
                then.status(200);
            })
            .await;
        let poster = on_mock(&server);
        assert_eq!(poster.post(&at(&server, "/u")).await, Ok(()));
        ok.assert_async().await;

        let moved = server
            .mock_async(|when, then| {
                when.method(POST).path("/moved");
                then.status(302).header("location", "http://127.0.0.1/inside");
            })
            .await;
        assert_eq!(poster.post(&at(&server, "/moved")).await, Err(()), "a redirect is not followed");
        moved.assert_async().await;
        server
            .mock_async(|when, then| {
                when.method(POST).path("/broken");
                then.status(500);
            })
            .await;
        assert_eq!(poster.post(&at(&server, "/broken")).await, Err(()));
    }

    #[tokio::test]
    async fn a_name_in_the_local_network_is_never_asked() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200);
            })
            .await;
        let table = HashMap::from([("shop.example".to_string(), vec!["127.0.0.1".parse().expect("ip")])]);
        let public = Guard { allowed: is_public, port: Some(server.port()) };
        let poster = Poster::new(Arc::new(FakeDns(table)), public).expect("a poster");
        assert_eq!(poster.post(&at(&server, "/u")).await, Err(()));
        mock.assert_calls_async(0).await;
    }
}
