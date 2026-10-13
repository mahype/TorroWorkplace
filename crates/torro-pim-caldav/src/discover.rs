//! Finding the CalDAV service of a mail address without signing in (task M7.1, RFC 6764).
//!
//! [`Discoverer::discover`] takes an e-mail address or a domain. Only the domain is looked at and
//! asked about, never the local part. Then, until one step finds something:
//!
//! 1. Providers known to offer no CalDAV that works with a password (Gmail, Outlook.com and the
//!    other Microsoft domains) are answered from the table at once, "not supported" with the
//!    reason, before anything is sent: Google publishes SRV records for CalDAV, but signing in
//!    there needs OAuth with a registered client.
//! 2. DNS SRV `_caldavs._tcp.<domain>` and the TXT record `path=…` next to it (RFC 6764 §3, §4).
//! 3. `https://<domain>/.well-known/caldav`, then `https://caldav.<domain>/.well-known/caldav`,
//!    following at most [`MAX_REDIRECTS`] redirects, https only (RFC 6764 §5).
//! 4. The table of known providers: mailbox.org, Posteo, Fastmail, iCloud, Zoho.
//!
//! Every outcome is a normal answer ([`Discovery`]): candidate addresses, none ("not found"), or
//! "not supported" with the reason. Only a network that cannot be reached at all is an error.
//!
//! The command is open to agents (`accounts.discover_services`), so it must not turn into a probe
//! of the person's network:
//!
//! - no IP literals, neither typed nor as an SRV target nor in a redirect;
//! - every name is resolved through the [`Resolver`] and refused when one of its addresses is not
//!   on the public internet ([`is_public`]: loopback, private, link-local, ULA and more). The HTTP
//!   client connects only to the addresses that passed this check (its DNS is the same resolver,
//!   guarded), so a name cannot resolve one way for the check and another way for the connection;
//! - https only, no proxy, short timeouts ([`CONNECT_TIMEOUT`], [`REQUEST_TIMEOUT`], [`DEADLINE`]),
//!   two DNS questions, at most [`MAX_TARGETS`] SRV targets and two well-known addresses a call;
//! - the answer holds only addresses built from what was found, never a server's content or its
//!   headers. Logging is the caller's business: it names the command, never the address.
//!
//! The mail discovery of `torro-mail-imap` (task M2.2) asks through the same [`Resolver`] (with
//! its MX records) and keeps the same guards: [`checked_domain`] for every host name it is
//! handed, [`is_public`] for every address, and [`GuardedDns`] for its HTTP client.

use std::cmp::Reverse;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Method, Url};
/// The error of a [`Lookup`], here so that resolvers outside this crate (mail discovery, the
/// core's tests) need no dependency of their own on `torro-pim`.
pub use torro_pim::ProviderError;

/// How many redirects a well-known address may take (RFC 6764 asks clients to follow them).
pub const MAX_REDIRECTS: usize = 5;
/// How many SRV targets are looked at, best first.
pub const MAX_TARGETS: usize = 3;
/// How long connecting to one server may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long one request may take, redirects included.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the network steps of one call may take together; the table still answers after it.
pub const DEADLINE: Duration = Duration::from_secs(15);

/// What a search for the CalDAV service of a domain found. Every field is built from what was
/// found; nothing a server sent is passed on but the address it redirected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Who runs the service: the provider's name from the table (`mailbox.org`, `iCloud`) or
    /// else the domain itself.
    pub provider: String,
    /// Addresses to sign in at, best first: `https://dav.mailbox.org/`. Empty when nothing was
    /// found or the provider is not supported.
    pub urls: Vec<String>,
    /// The provider is known not to work with Torro Workplace; `urls` is empty then.
    pub unsupported: Option<Unsupported>,
}

/// Why a known provider does not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// Gmail and Google Workspace: CalDAV only with OAuth and a registered client.
    Google,
    /// Outlook.com, Hotmail, Live, Microsoft 365: no CalDAV at all, only Graph or EWS.
    Microsoft,
}

impl Unsupported {
    /// The reason in English, for people; the core translates it (`torro_core::i18n::tr`).
    pub fn reason(self) -> &'static str {
        match self {
            Unsupported::Google => {
                "Google calendars need a sign-in through Google, which Torro Workplace does not support yet."
            }
            Unsupported::Microsoft => {
                "Outlook.com and Microsoft 365 offer no CalDAV, so Torro Workplace cannot show their calendars."
            }
        }
    }
}

/// Why nothing could be searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverError {
    /// The input names no internet domain: empty, an IP address, `localhost`, a name without a
    /// dot or with characters a host name cannot have.
    NoDomain,
    /// Neither DNS nor any server answered, and the table does not know the domain.
    Offline,
}

/// What a name lookup returns: the records found, empty when there are none. `Err` only when
/// the lookup itself failed (no DNS server reachable).
pub type Lookup<'a, T> = Pin<Box<dyn Future<Output = Result<Vec<T>, ProviderError>> + Send + 'a>>;

/// One SRV record (RFC 2782).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    /// Lower is tried first.
    pub priority: u16,
    /// Among the same priority, higher is tried first.
    pub weight: u16,
    pub port: u16,
    /// The host name, with or without the final dot; `.` means "no such service".
    pub target: String,
}

/// One MX record (RFC 5321 §5.1): where mail for a domain goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MxRecord {
    /// Lower is tried first.
    pub preference: u16,
    /// The host name, with or without the final dot; `.` is a "null MX": the domain takes no
    /// mail (RFC 7505).
    pub exchange: String,
}

/// The name lookups discovery needs. Behind a trait so the tests answer from a table and never
/// ask a real DNS server; the core uses [`SystemResolver`].
///
/// Implementations answer for exactly the name they are given (no search domains appended) and
/// return an empty list, not an error, for a name without such records.
pub trait Resolver: Send + Sync {
    /// The SRV records of `name` (`_caldavs._tcp.example.org`).
    fn srv<'a>(&'a self, name: &'a str) -> Lookup<'a, SrvRecord>;
    /// The TXT records of `name`, each as one string (its pieces joined).
    fn txt<'a>(&'a self, name: &'a str) -> Lookup<'a, String>;
    /// The MX records of `name` (`example.org`), in any order; the caller sorts them by
    /// preference. Mail discovery (task M2.2) only reads the host names, it never connects to
    /// them.
    fn mx<'a>(&'a self, name: &'a str) -> Lookup<'a, MxRecord>;
    /// The IPv4 and IPv6 addresses of `host`. The HTTP requests of discovery resolve through
    /// this too, after [`is_public`] (or the test's rule) checked every address.
    fn ips<'a>(&'a self, host: &'a str) -> Lookup<'a, IpAddr>;
}

/// The system's DNS: SRV and TXT through `hickory-resolver` on the system's configuration
/// (`/etc/resolv.conf`, the Windows registry), addresses through the operating system
/// (`getaddrinfo`), as every other connection of the suite. The resolver is built on first use.
#[derive(Default)]
pub struct SystemResolver {
    dns: OnceLock<Result<TokioResolver, String>>,
}

impl SystemResolver {
    pub fn new() -> SystemResolver {
        SystemResolver::default()
    }

    fn dns(&self) -> Result<&TokioResolver, ProviderError> {
        let built = self.dns.get_or_init(|| {
            TokioResolver::builder_tokio().and_then(|builder| builder.build()).map_err(|err| err.to_string())
        });
        built.as_ref().map_err(|err| ProviderError::Other(format!("no DNS configuration: {err}")))
    }
}

impl Resolver for SystemResolver {
    fn srv<'a>(&'a self, name: &'a str) -> Lookup<'a, SrvRecord> {
        Box::pin(async move {
            // A final dot: the name as it is, never with a search domain of the network appended.
            match self.dns()?.srv_lookup(format!("{name}.")).await {
                Ok(lookup) => Ok(lookup
                    .answers()
                    .iter()
                    .filter_map(|record| match &record.data {
                        RData::SRV(srv) => Some(SrvRecord {
                            priority: srv.priority,
                            weight: srv.weight,
                            port: srv.port,
                            target: srv.target.to_ascii(),
                        }),
                        _ => None,
                    })
                    .collect()),
                Err(err) if err.is_no_records_found() => Ok(Vec::new()),
                Err(_) => Err(ProviderError::Offline),
            }
        })
    }

    fn txt<'a>(&'a self, name: &'a str) -> Lookup<'a, String> {
        Box::pin(async move {
            match self.dns()?.txt_lookup(format!("{name}.")).await {
                Ok(lookup) => Ok(lookup
                    .answers()
                    .iter()
                    .filter_map(|record| match &record.data {
                        RData::TXT(txt) => {
                            Some(txt.txt_data.iter().map(|piece| String::from_utf8_lossy(piece)).collect())
                        }
                        _ => None,
                    })
                    .collect()),
                Err(err) if err.is_no_records_found() => Ok(Vec::new()),
                Err(_) => Err(ProviderError::Offline),
            }
        })
    }

    fn mx<'a>(&'a self, name: &'a str) -> Lookup<'a, MxRecord> {
        Box::pin(async move {
            match self.dns()?.mx_lookup(format!("{name}.")).await {
                Ok(lookup) => Ok(lookup
                    .answers()
                    .iter()
                    .filter_map(|record| match &record.data {
                        RData::MX(mx) => Some(MxRecord { preference: mx.preference, exchange: mx.exchange.to_ascii() }),
                        _ => None,
                    })
                    .collect()),
                Err(err) if err.is_no_records_found() => Ok(Vec::new()),
                Err(_) => Err(ProviderError::Offline),
            }
        })
    }

    fn ips<'a>(&'a self, host: &'a str) -> Lookup<'a, IpAddr> {
        Box::pin(async move {
            match tokio::net::lookup_host((host, 0)).await {
                Ok(found) => Ok(found.map(|address| address.ip()).collect()),
                Err(_) => Err(ProviderError::Offline),
            }
        })
    }
}

/// A resolver that knows no name. Discovery then finds nothing on the network and sends no
/// request (every connection needs an address), so only the table answers. For a service that
/// must not reach the network, as in tests.
pub struct NoDns;

impl Resolver for NoDns {
    fn srv<'a>(&'a self, _name: &'a str) -> Lookup<'a, SrvRecord> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn txt<'a>(&'a self, _name: &'a str) -> Lookup<'a, String> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn mx<'a>(&'a self, _name: &'a str) -> Lookup<'a, MxRecord> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn ips<'a>(&'a self, _host: &'a str) -> Lookup<'a, IpAddr> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// Whether `ip` is an address on the public internet. Refused are loopback, private (RFC 1918),
/// link-local, unspecified, broadcast, multicast, documentation, shared (CGNAT), benchmarking and
/// reserved IPv4 ranges, and for IPv6 loopback, unspecified, multicast, unique local (ULA),
/// link-local, site-local, documentation and the forms that carry an IPv4 address inside
/// (mapped, compatible, NAT64, 6to4, Teredo; mapped ones are judged by that IPv4 address, the
/// others refused, which costs nothing for a mail provider).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            let refused = v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xffc0) == 0xfec0
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x0064 && s[1] == 0xff9b)
                || s[0] == 0x2002
                || (s[0] == 0x2001 && s[1] == 0)
                || s[..6].iter().all(|&segment| segment == 0);
            !refused
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let refused = ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240;
    !refused
}

/// The domain of `address` (`anna@mailbox.org`, or `mailbox.org` itself), lowercased and in
/// ASCII (an international name becomes `xn--…`); `None` when it names no internet domain
/// ([`DiscoverError::NoDomain`]). Everything before the last `@` is dropped here and never used.
pub fn domain_of(address: &str) -> Option<String> {
    let address = address.trim();
    let domain = address.rsplit_once('@').map_or(address, |(_, domain)| domain);
    checked_domain(domain)
}

/// Names that never lead to the internet, as the last label of a domain.
const LOCAL_NAMES: &[&str] =
    &["localhost", "local", "localdomain", "internal", "lan", "home", "arpa", "test", "invalid"];

/// `name` as a lowercase ASCII domain with at least two labels, or `None`: IP addresses (also
/// the odd IPv4 forms like `0x7f.1`), ports, paths, user names, special-use names
/// ([`LOCAL_NAMES`]), names longer than 253 characters and anything a host name cannot hold are
/// refused. A final dot is dropped. For every host name discovery is handed by DNS or a server
/// (SRV and MX targets, hosts in autoconfig documents) before it is asked or passed on.
pub fn checked_domain(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches('.');
    let odd = |c: char| c.is_whitespace() || matches!(c, '/' | '\\' | ':' | '@' | '[' | ']' | '?' | '#' | '%');
    if name.is_empty() || name.len() > 253 || name.contains(odd) {
        return None;
    }
    // The URL parser does what browsers do: IDNA for international names, and it reads every
    // spelling of an IPv4 address as one, so `domain()` is `None` for them.
    let url = Url::parse(&format!("https://{name}/")).ok()?;
    let domain = url.domain()?.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    let well_formed = labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    let last = labels.last()?;
    let local = LOCAL_NAMES.contains(last) || last.bytes().all(|b| b.is_ascii_digit());
    (well_formed && !local).then_some(domain)
}

/// A provider of the table: its domains, its name, and where its CalDAV lives or why it does not
/// work. To be checked against the real servers in task M7.3.
struct Known {
    domains: &'static [&'static str],
    name: &'static str,
    url: Option<&'static str>,
    unsupported: Option<Unsupported>,
}

/// The providers discovery knows by name (task M7.1). The addresses are the start of each
/// provider's CalDAV; the client finds the user's calendars from there.
const KNOWN: &[Known] = &[
    Known { domains: &["mailbox.org"], name: "mailbox.org", url: Some("https://dav.mailbox.org/"), unsupported: None },
    Known {
        domains: &[
            "posteo.de",
            "posteo.at",
            "posteo.ch",
            "posteo.eu",
            "posteo.net",
            "posteo.org",
            "posteo.me",
            "posteo.uk",
            "posteo.us",
            "posteo.be",
            "posteo.nl",
            "posteo.se",
        ],
        name: "Posteo",
        url: Some("https://posteo.de:8443/"),
        unsupported: None,
    },
    Known {
        domains: &["fastmail.com", "fastmail.fm", "fastmail.net", "fastmail.org", "fastmail.us", "fastmail.de"],
        name: "Fastmail",
        url: Some("https://caldav.fastmail.com/"),
        unsupported: None,
    },
    Known {
        domains: &["icloud.com", "me.com", "mac.com"],
        name: "iCloud",
        url: Some("https://caldav.icloud.com/"),
        unsupported: None,
    },
    Known {
        domains: &["zoho.com", "zohomail.com"],
        name: "Zoho",
        url: Some("https://calendar.zoho.com/"),
        unsupported: None,
    },
    Known {
        domains: &["zoho.eu", "zohomail.eu"],
        name: "Zoho",
        url: Some("https://calendar.zoho.eu/"),
        unsupported: None,
    },
    Known {
        domains: &["gmail.com", "googlemail.com"],
        name: "Gmail",
        url: None,
        unsupported: Some(Unsupported::Google),
    },
    Known {
        domains: &[
            "outlook.com",
            "outlook.de",
            "outlook.at",
            "outlook.fr",
            "hotmail.com",
            "hotmail.de",
            "hotmail.co.uk",
            "hotmail.fr",
            "live.com",
            "live.de",
            "msn.com",
            "windowslive.com",
        ],
        name: "Outlook.com",
        url: None,
        unsupported: Some(Unsupported::Microsoft),
    },
];

fn known(domain: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|known| known.domains.contains(&domain))
}

/// What to call the provider of `domain`: its name from the table, else the domain. For an
/// account whose server a person typed by hand.
pub fn provider_name(domain: &str) -> String {
    known(domain).map_or_else(|| domain.to_string(), |known| known.name.to_string())
}

/// A server address a person typed for signing in (`dav.example.org`,
/// `https://dav.example.org/caldav/`) as a URL: `https://` added when no scheme is given, query
/// and fragment dropped, no user name or password in it. Plain `http` only for a server on this
/// computer (`localhost`, `127.0.0.1`, `[::1]`): anywhere else the password would cross the
/// network unencrypted.
///
/// Unlike discovery this takes IP addresses and private networks: only a person signs in, with
/// the app key, and a server at home is theirs to use.
pub fn server_address(input: &str) -> Result<String, ProviderError> {
    let input = input.trim();
    let not_an_address = || ProviderError::Other(format!("{input:?} is not a server address"));
    if input.is_empty() {
        return Err(not_an_address());
    }
    let with_scheme = if input.contains("://") { input.to_string() } else { format!("https://{input}") };
    let mut url = Url::parse(&with_scheme).map_err(|_| not_an_address())?;
    let host = url.host_str().ok_or_else(not_an_address)?;
    let local = match url.domain() {
        Some(domain) => domain.eq_ignore_ascii_case("localhost"),
        None => host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()),
    };
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        "http" => {
            return Err(ProviderError::Other("use an https address: the password must not travel unencrypted".into()));
        }
        _ => return Err(not_an_address()),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(not_an_address());
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

/// How discovery talks to the web: the scheme of every address and redirect, the port of the
/// well-known addresses (`None`: the scheme's own) and which addresses it may connect to.
/// Always [`Web::HTTPS`], except in this module's tests, whose server is plain `http` on the
/// loopback interface.
#[derive(Clone, Copy)]
struct Web {
    scheme: &'static str,
    port: Option<u16>,
    allowed: fn(IpAddr) -> bool,
}

impl Web {
    const HTTPS: Web = Web { scheme: "https", port: None, allowed: is_public };
}

/// The DNS of discovery's HTTP client: the [`Resolver`], with every name refused that has an
/// address `allowed` refuses. reqwest connects only to what this returns
/// (`ClientBuilder::dns_resolver`); together with `no_proxy()` a name cannot resolve one way for
/// the check and another way for the connection.
pub struct GuardedDns {
    resolver: Arc<dyn Resolver>,
    allowed: fn(IpAddr) -> bool,
}

impl GuardedDns {
    /// Names resolved through `resolver`, refused unless every address passes `allowed`
    /// ([`is_public`] outside tests).
    pub fn new(resolver: Arc<dyn Resolver>, allowed: fn(IpAddr) -> bool) -> GuardedDns {
        GuardedDns { resolver, allowed }
    }
}

impl Resolve for GuardedDns {
    fn resolve(&self, name: Name) -> Resolving {
        let (resolver, allowed, host) = (Arc::clone(&self.resolver), self.allowed, name.as_str().to_string());
        Box::pin(async move {
            let ips = resolver.ips(&host).await.map_err(|err| err.to_string())?;
            if ips.is_empty() || !ips.iter().all(|ip| allowed(*ip)) {
                return Err("refused: not an address on the public internet".into());
            }
            let addresses: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addresses)
        })
    }
}

/// What one well-known request found.
enum Probe {
    /// A CalDAV server answered there (207, or 401 asking to sign in), at this final address.
    Found(String),
    /// A server answered, but not as CalDAV, or redirected where discovery does not follow.
    Missing,
    /// No server could be reached (also: its name was refused).
    Unreachable,
}

/// The body of the well-known request: the one property RFC 6764 clients ask for first.
const PROBE_BODY: &str =
    r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;

/// Finds the CalDAV service of a mail address; see the module docs.
pub struct Discoverer {
    resolver: Arc<dyn Resolver>,
    http: reqwest::Client,
    web: Web,
}

impl Discoverer {
    /// Discovery on the system's DNS ([`SystemResolver`]), as the core runs it.
    pub fn system() -> Result<Discoverer, ProviderError> {
        Discoverer::new(Arc::new(SystemResolver::new()))
    }

    /// Discovery with `resolver` for every name, https only and public addresses only. Fails only
    /// when no HTTP client can be built.
    pub fn new(resolver: Arc<dyn Resolver>) -> Result<Discoverer, ProviderError> {
        Discoverer::with_web(resolver, Web::HTTPS)
    }

    fn with_web(resolver: Arc<dyn Resolver>, web: Web) -> Result<Discoverer, ProviderError> {
        let scheme = web.scheme;
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            // As reqwest's own limit: the first address is the request itself, no redirect.
            if attempt.previous().len() > MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if usable(attempt.url(), scheme) {
                attempt.follow()
            } else {
                attempt.error("a redirect to an address discovery does not follow")
            }
        });
        let guarded = GuardedDns::new(Arc::clone(&resolver), web.allowed);
        let http = reqwest::Client::builder()
            // No device name here: discovery asks servers that are not the user's yet.
            .user_agent("Torro Workplace")
            // A proxy would resolve the names itself, past the guard.
            .no_proxy()
            .https_only(scheme == "https")
            .redirect(policy)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .dns_resolver(Arc::new(guarded))
            .build()
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        Ok(Discoverer { resolver, http, web })
    }

    /// Searches the CalDAV service of `address` (an e-mail address or a domain), in the order of
    /// the module docs, without signing in. `Err(NoDomain)` for an input that names no internet
    /// domain; `Err(Offline)` when neither DNS nor any server answered and the table does not
    /// know the domain. Everything else is an `Ok`, "not found" and "not supported" included.
    pub async fn discover(&self, address: &str) -> Result<Discovery, DiscoverError> {
        let domain = domain_of(address).ok_or(DiscoverError::NoDomain)?;
        let known = known(&domain);
        let provider = provider_name(&domain);
        if let Some(why) = known.and_then(|known| known.unsupported) {
            return Ok(Discovery { provider, urls: Vec::new(), unsupported: Some(why) });
        }
        let mut reached = false;
        let urls = tokio::time::timeout(DEADLINE, self.search(&domain, &mut reached)).await.unwrap_or_default();
        let urls = match known.and_then(|known| known.url) {
            Some(url) if urls.is_empty() => vec![url.to_string()],
            _ => urls,
        };
        if urls.is_empty() && !reached {
            return Err(DiscoverError::Offline);
        }
        Ok(Discovery { provider, urls, unsupported: None })
    }

    /// Steps 2 and 3 of the module docs; `reached` is set once DNS or a server answered.
    async fn search(&self, domain: &str, reached: &mut bool) -> Vec<String> {
        let found = self.srv_targets(domain, reached).await;
        if !found.is_empty() {
            return found;
        }
        for host in [domain.to_string(), format!("caldav.{domain}")] {
            match self.probe(&format!("{}/.well-known/caldav", origin(self.web.scheme, &host, self.web.port))).await {
                Probe::Found(url) => {
                    *reached = true;
                    return vec![url];
                }
                Probe::Missing => *reached = true,
                Probe::Unreachable => {}
            }
        }
        Vec::new()
    }

    /// The addresses the SRV records of `_caldavs._tcp.<domain>` name (RFC 6764 §3): each target
    /// that resolves to public addresses only, at the TXT record's `path` or, without one, where
    /// its well-known address leads (else its root).
    async fn srv_targets(&self, domain: &str, reached: &mut bool) -> Vec<String> {
        let name = format!("_caldavs._tcp.{domain}");
        let Ok(mut records) = self.resolver.srv(&name).await else { return Vec::new() };
        *reached = true;
        if records.is_empty() {
            return Vec::new();
        }
        let path = self.resolver.txt(&name).await.ok().and_then(|texts| context_path(&texts));
        records.sort_by_key(|record| (record.priority, Reverse(record.weight)));
        let mut urls: Vec<String> = Vec::new();
        for record in records.into_iter().take(MAX_TARGETS) {
            let Some(host) = checked_domain(&record.target) else { continue };
            if !self.resolves_publicly(&host).await {
                continue;
            }
            let origin = origin(self.web.scheme, &host, Some(record.port));
            let url = match &path {
                Some(path) => Url::parse(&origin)
                    .and_then(|base| base.join(path))
                    .ok()
                    .filter(|url| url.host_str() == Some(host.as_str()))
                    .and_then(|url| clean(&url, self.web.scheme)),
                None => match self.probe(&format!("{origin}/.well-known/caldav")).await {
                    Probe::Found(url) => Some(url),
                    Probe::Missing | Probe::Unreachable => Some(format!("{origin}/")),
                },
            };
            if let Some(url) = url
                && !urls.contains(&url)
            {
                urls.push(url);
            }
        }
        urls
    }

    /// Whether `host` has addresses and every one of them may be connected to.
    async fn resolves_publicly(&self, host: &str) -> bool {
        match self.resolver.ips(host).await {
            Ok(ips) => !ips.is_empty() && ips.iter().all(|ip| (self.web.allowed)(*ip)),
            Err(_) => false,
        }
    }

    /// Asks `url` (a well-known address) for the current user principal, without signing in.
    /// Only status and final address are looked at; the body is never read.
    async fn probe(&self, url: &str) -> Probe {
        let Ok(propfind) = Method::from_bytes(b"PROPFIND") else { return Probe::Missing };
        let sent = self
            .http
            .request(propfind, url)
            .header("Depth", "0")
            .header(reqwest::header::CONTENT_TYPE, "application/xml; charset=utf-8")
            .body(PROBE_BODY)
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(err) if err.is_redirect() => return Probe::Missing,
            Err(_) => return Probe::Unreachable,
        };
        let caldav = matches!(response.status().as_u16(), 207 | 401);
        match caldav.then(|| clean(response.url(), self.web.scheme)).flatten() {
            Some(url) => Probe::Found(url),
            None => Probe::Missing,
        }
    }
}

/// Whether discovery follows or hands out `url`: the expected scheme, a domain name (no IP
/// literal) and no user name or password in it.
fn usable(url: &Url, scheme: &str) -> bool {
    url.scheme() == scheme && url.domain().is_some() && url.username().is_empty() && url.password().is_none()
}

/// `url` as discovery hands it out: without query and fragment, `None` when it is not
/// [`usable`].
fn clean(url: &Url, scheme: &str) -> Option<String> {
    if !usable(url, scheme) {
        return None;
    }
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

/// `https://host` with `:port` unless it is the scheme's own.
fn origin(scheme: &str, host: &str, port: Option<u16>) -> String {
    let own = if scheme == "https" { 443 } else { 80 };
    match port {
        Some(port) if port != own => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    }
}

/// The context path of a `_caldavs` TXT record (`path=/dav/`, RFC 6764 §4): an absolute path of
/// at most 256 characters without spaces, query or fragment; `None` without one. Never `//…`,
/// which a URL reads as another host.
fn context_path(texts: &[String]) -> Option<String> {
    texts
        .iter()
        .filter_map(|text| text.trim().strip_prefix("path="))
        .find(|path| {
            path.starts_with('/')
                && !path.starts_with("//")
                && path.len() <= 256
                && !path.contains(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '?' | '#' | '\\'))
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use httpmock::MockServer;

    use super::*;

    /// A DNS that answers from tables and notes every name it is asked about.
    #[derive(Default)]
    struct FakeDns {
        srv: HashMap<String, Vec<SrvRecord>>,
        txt: HashMap<String, Vec<String>>,
        ips: HashMap<String, Vec<IpAddr>>,
        mx: HashMap<String, Vec<MxRecord>>,
        /// Every lookup fails, as without network.
        offline: bool,
        asked: Mutex<Vec<String>>,
    }

    impl FakeDns {
        fn answer<T: Clone>(&self, table: &HashMap<String, Vec<T>>, name: &str) -> Result<Vec<T>, ProviderError> {
            self.asked.lock().expect("lock").push(name.to_string());
            match self.offline {
                true => Err(ProviderError::Offline),
                false => Ok(table.get(name).cloned().unwrap_or_default()),
            }
        }

        fn with_ip(mut self, host: &str, ip: &str) -> FakeDns {
            self.ips.insert(host.into(), vec![ip.parse().expect("an IP address")]);
            self
        }

        fn with_srv(mut self, domain: &str, target: &str, port: u16, path: Option<&str>) -> FakeDns {
            let name = format!("_caldavs._tcp.{domain}");
            let record = SrvRecord { priority: 10, weight: 0, port, target: target.into() };
            self.srv.entry(name.clone()).or_default().push(record);
            if let Some(path) = path {
                self.txt.insert(name, vec!["txtvers=1".into(), format!("path={path}")]);
            }
            self
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().expect("lock").clone()
        }
    }

    impl Resolver for FakeDns {
        fn srv<'a>(&'a self, name: &'a str) -> Lookup<'a, SrvRecord> {
            Box::pin(async move { self.answer(&self.srv, name) })
        }

        fn txt<'a>(&'a self, name: &'a str) -> Lookup<'a, String> {
            Box::pin(async move { self.answer(&self.txt, name) })
        }

        fn mx<'a>(&'a self, name: &'a str) -> Lookup<'a, MxRecord> {
            Box::pin(async move { self.answer(&self.mx, name) })
        }

        fn ips<'a>(&'a self, host: &'a str) -> Lookup<'a, IpAddr> {
            Box::pin(async move { self.answer(&self.ips, host) })
        }
    }

    /// The test's rule: the public internet and, since the mock server lives there, loopback.
    fn public_or_loopback(ip: IpAddr) -> bool {
        is_public(ip) || ip.is_loopback()
    }

    /// Discovery over plain http against the mock server on `port`, with `allowed` deciding which
    /// addresses it may connect to.
    fn on_mock(dns: FakeDns, port: u16, allowed: fn(IpAddr) -> bool) -> (Discoverer, Arc<FakeDns>) {
        let dns = Arc::new(dns);
        let web = Web { scheme: "http", port: Some(port), allowed };
        let resolver: Arc<dyn Resolver> = Arc::clone(&dns) as Arc<dyn Resolver>;
        (Discoverer::with_web(resolver, web).expect("client"), dns)
    }

    fn found(urls: &[String], provider: &str) -> Result<Discovery, DiscoverError> {
        Ok(Discovery { provider: provider.into(), urls: urls.to_vec(), unsupported: None })
    }

    #[test]
    fn only_internet_domains_are_searched_and_the_local_part_is_dropped() {
        assert_eq!(domain_of("Anna.Smith@MailBox.org").as_deref(), Some("mailbox.org"));
        assert_eq!(domain_of(" mailbox.org. ").as_deref(), Some("mailbox.org"));
        assert_eq!(domain_of("\"a@b\"@posteo.de").as_deref(), Some("posteo.de"));
        assert_eq!(domain_of("anna@bücher.de").as_deref(), Some("xn--bcher-kva.de"));
        for refused in [
            "",
            "anna@",
            "anna@127.0.0.1",
            "anna@[::1]",
            "anna@0x7f.1",
            "2130706433",
            "anna@10.0.0.1",
            "localhost",
            "anna@localhost",
            "nas.local",
            "router.lan",
            "printer.home",
            "box.internal",
            "1.0.0.127.in-addr.arpa",
            "intranet",
            "example.org:8080",
            "example.org/path",
            "anna@example.org:8080",
            "exa mple.org",
            "-bad.example.org",
            "example.org%2f",
        ] {
            assert_eq!(domain_of(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn only_public_addresses_pass() {
        for public in ["93.184.215.14", "8.8.8.8", "2a00:1450:4001:82b::200e", "::ffff:93.184.215.14"] {
            assert!(is_public(public.parse().expect("ip")), "{public}");
        }
        for refused in [
            "127.0.0.1",
            "127.1.2.3",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.178.1",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fc12::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
            "::7f00:1",
        ] {
            assert!(!is_public(refused.parse().expect("ip")), "{refused}");
        }
    }

    #[tokio::test]
    async fn unsupported_providers_are_answered_without_asking_anyone() {
        let dns = Arc::new(FakeDns::default());
        let discoverer = Discoverer::new(Arc::clone(&dns) as Arc<dyn Resolver>).expect("client");
        let gmail = discoverer.discover("anna@gmail.com").await.expect("an answer");
        assert_eq!(gmail, Discovery { provider: "Gmail".into(), urls: vec![], unsupported: Some(Unsupported::Google) });
        let outlook = discoverer.discover("anna@hotmail.de").await.expect("an answer");
        assert_eq!(outlook.unsupported, Some(Unsupported::Microsoft));
        assert!(outlook.urls.is_empty());
        assert!(dns.asked().is_empty(), "{:?}", dns.asked());
        assert!(!Unsupported::Google.reason().is_empty() && Unsupported::Microsoft.reason().ends_with('.'));
    }

    #[tokio::test]
    async fn known_providers_come_from_the_table_when_the_network_finds_nothing() {
        let dns = Arc::new(FakeDns::default());
        let discoverer = Discoverer::new(Arc::clone(&dns) as Arc<dyn Resolver>).expect("client");
        let found_at = discoverer.discover("anna@mailbox.org").await;
        assert_eq!(found_at, found(&["https://dav.mailbox.org/".into()], "mailbox.org"));
        assert_eq!(discoverer.discover("me.com").await, found(&["https://caldav.icloud.com/".into()], "iCloud"));
        // Only names of the domain were asked, never the local part.
        assert!(dns.asked().iter().all(|name| !name.contains("anna")), "{:?}", dns.asked());
        assert!(dns.asked().contains(&"_caldavs._tcp.mailbox.org".to_string()));

        // Without any network the table still answers; an unknown domain is offline then.
        let offline = Discoverer::new(Arc::new(FakeDns { offline: true, ..FakeDns::default() })).expect("client");
        assert_eq!(offline.discover("posteo.de").await, found(&["https://posteo.de:8443/".into()], "Posteo"));
        assert_eq!(offline.discover("anna@example.org").await, Err(DiscoverError::Offline));
    }

    #[tokio::test]
    async fn nothing_found_is_an_answer_too() {
        let discoverer = Discoverer::new(Arc::new(NoDns)).expect("client");
        assert_eq!(discoverer.discover("anna@example.org").await, found(&[], "example.org"));
        assert_eq!(discoverer.discover("anna@127.0.0.1").await, Err(DiscoverError::NoDomain));
    }

    #[tokio::test]
    async fn srv_with_a_path_names_the_service_directly() {
        let dns = FakeDns::default()
            .with_srv("example.org", "dav.example.org.", 8443, Some("/caldav/"))
            .with_ip("dav.example.org", "93.184.215.14");
        let discoverer = Discoverer::new(Arc::new(dns)).expect("client");
        let answer = discoverer.discover("anna@example.org").await;
        assert_eq!(answer, found(&["https://dav.example.org:8443/caldav/".into()], "example.org"));
    }

    #[tokio::test]
    async fn srv_targets_on_private_addresses_or_ip_literals_are_dropped() {
        let dns = FakeDns::default()
            .with_srv("example.org", "nas.example.org.", 443, Some("/dav/"))
            .with_srv("example.org", "192.168.1.10", 443, Some("/dav/"))
            .with_srv("example.org", ".", 443, Some("/dav/"))
            .with_ip("nas.example.org", "192.168.1.10");
        let discoverer = Discoverer::new(Arc::new(dns)).expect("client");
        assert_eq!(discoverer.discover("example.org").await, found(&[], "example.org"));
    }

    #[tokio::test]
    async fn srv_without_a_path_follows_the_targets_well_known_address() {
        let server = MockServer::start_async().await;
        let port = server.port();
        let well_known = server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav");
                then.status(301).header("Location", format!("http://dav.example.org:{port}/remote.php/dav/"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/remote.php/dav/");
                then.status(401).header("WWW-Authenticate", "Basic realm=\"dav\"");
            })
            .await;
        let dns = FakeDns::default()
            .with_srv("example.org", "dav.example.org", port, None)
            .with_ip("dav.example.org", "127.0.0.1");
        let (discoverer, _) = on_mock(dns, port, public_or_loopback);
        let answer = discoverer.discover("anna@example.org").await;
        assert_eq!(answer, found(&[format!("http://dav.example.org:{port}/remote.php/dav/")], "example.org"));
        assert_eq!(well_known.calls_async().await, 1);
    }

    #[tokio::test]
    async fn the_well_known_address_is_followed_through_redirects() {
        let server = MockServer::start_async().await;
        let port = server.port();
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav");
                then.status(301).header("Location", "/dav/");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/dav/");
                then.status(308).header("Location", "/dav/calendars/?session=1#top");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/dav/calendars/");
                then.status(207).body("<d:multistatus xmlns:d=\"DAV:\"/>");
            })
            .await;
        let (discoverer, dns) =
            on_mock(FakeDns::default().with_ip("example.org", "127.0.0.1"), port, public_or_loopback);
        let answer = discoverer.discover("anna@example.org").await;
        assert_eq!(answer, found(&[format!("http://example.org:{port}/dav/calendars/")], "example.org"));
        assert!(dns.asked().iter().all(|name| !name.contains("anna")));
    }

    #[tokio::test]
    async fn the_caldav_host_is_tried_after_the_domain() {
        let server = MockServer::start_async().await;
        let port = server.port();
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav");
                then.status(401);
            })
            .await;
        let dns = FakeDns::default().with_ip("caldav.example.org", "127.0.0.1");
        let (discoverer, _) = on_mock(dns, port, public_or_loopback);
        let answer = discoverer.discover("example.org").await;
        assert_eq!(answer, found(&[format!("http://caldav.example.org:{port}/.well-known/caldav")], "example.org"));
    }

    #[tokio::test]
    async fn a_web_page_that_is_no_caldav_server_is_not_found() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav");
                then.status(200).body("<html>Welcome</html>");
            })
            .await;
        let (discoverer, _) =
            on_mock(FakeDns::default().with_ip("example.org", "127.0.0.1"), server.port(), public_or_loopback);
        assert_eq!(discoverer.discover("example.org").await, found(&[], "example.org"));
    }

    #[tokio::test]
    async fn loopback_and_private_targets_are_never_contacted() {
        let server = MockServer::start_async().await;
        let port = server.port();
        let anything = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(207);
            })
            .await;
        // The production rule: example.org resolves to loopback, so nothing is sent.
        let (discoverer, _) = on_mock(FakeDns::default().with_ip("example.org", "127.0.0.1"), port, is_public);
        assert_eq!(discoverer.discover("example.org").await, found(&[], "example.org"));
        assert_eq!(anything.calls_async().await, 0);
        // A name with a public and a private address is refused as a whole.
        let mut mixed = FakeDns::default();
        mixed
            .ips
            .insert("example.org".into(), vec!["93.184.215.14".parse().expect("ip"), "10.0.0.1".parse().expect("ip")]);
        let (discoverer, _) = on_mock(mixed, port, is_public);
        assert_eq!(discoverer.discover("example.org").await, found(&[], "example.org"));
        assert_eq!(anything.calls_async().await, 0);
    }

    #[tokio::test]
    async fn redirects_to_private_hosts_ip_literals_or_too_far_are_not_followed() {
        let server = MockServer::start_async().await;
        let port = server.port();
        let hops = |n: u32| format!("/hop/{n}");
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav").header("host", format!("intranet-link.example.org:{port}"));
                then.status(302).header("Location", format!("http://intranet.example.org:{port}/dav/"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav").header("host", format!("literal.example.org:{port}"));
                then.status(302).header("Location", format!("http://127.0.0.1:{port}/dav/"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/caldav").header("host", format!("far.example.org:{port}"));
                then.status(302).header("Location", hops(1));
            })
            .await;
        for n in 1..=MAX_REDIRECTS as u32 {
            server
                .mock_async(|when, then| {
                    when.path(hops(n));
                    then.status(302).header("Location", hops(n + 1));
                })
                .await;
        }
        let beyond = server
            .mock_async(|when, then| {
                when.path(hops(MAX_REDIRECTS as u32 + 1));
                then.status(207);
            })
            .await;
        let target = server
            .mock_async(|when, then| {
                when.path("/dav/");
                then.status(207);
            })
            .await;
        let dns = FakeDns::default()
            .with_ip("intranet-link.example.org", "127.0.0.1")
            .with_ip("intranet.example.org", "10.0.0.7")
            .with_ip("literal.example.org", "127.0.0.1")
            .with_ip("far.example.org", "127.0.0.1");
        let (discoverer, _) = on_mock(dns, port, public_or_loopback);
        for domain in ["intranet-link.example.org", "literal.example.org", "far.example.org"] {
            assert_eq!(discoverer.discover(domain).await, found(&[], domain), "{domain}");
        }
        assert_eq!(target.calls_async().await, 0, "no redirect reached /dav/");
        assert_eq!(beyond.calls_async().await, 0, "the sixth redirect is not followed");
    }

    #[test]
    fn context_paths_are_absolute_and_plain() {
        let texts = |items: &[&str]| items.iter().map(|text| text.to_string()).collect::<Vec<_>>();
        assert_eq!(context_path(&texts(&["txtvers=1", "path=/dav/"])).as_deref(), Some("/dav/"));
        // `//host/` would join to another host, past the address check of the SRV target.
        for refused in ["path=dav/", "path=/a b/", "path=/x?y", "path=//evil.example.org/"] {
            assert_eq!(context_path(&texts(&[refused])), None, "{refused}");
        }
        assert_eq!(context_path(&texts(&[])), None);
    }

    #[test]
    fn typed_server_addresses_get_https_and_lose_query_and_user() {
        assert_eq!(server_address("dav.example.org").as_deref().ok(), Some("https://dav.example.org/"));
        assert_eq!(
            server_address(" https://dav.example.org/caldav/?x=1#y ").as_deref().ok(),
            Some("https://dav.example.org/caldav/")
        );
        assert_eq!(server_address("http://localhost:5232/").as_deref().ok(), Some("http://localhost:5232/"));
        assert_eq!(server_address("http://127.0.0.1:5232/").as_deref().ok(), Some("http://127.0.0.1:5232/"));
        assert_eq!(server_address("https://192.168.1.10/dav/").as_deref().ok(), Some("https://192.168.1.10/dav/"));
        for refused in ["", "http://dav.example.org/", "ftp://dav.example.org/", "https://anna:pw@dav.example.org/"] {
            assert!(server_address(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn origins_leave_out_the_schemes_own_port() {
        assert_eq!(origin("https", "dav.example.org", Some(443)), "https://dav.example.org");
        assert_eq!(origin("https", "dav.example.org", Some(8443)), "https://dav.example.org:8443");
        assert_eq!(origin("http", "example.org", None), "http://example.org");
        assert_eq!(provider_name("posteo.net"), "Posteo");
        assert_eq!(provider_name("example.org"), "example.org");
    }
}
