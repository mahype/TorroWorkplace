//! Finding the IMAP and SMTP servers of a mail address without signing in (task M2.2).
//!
//! [`MailDiscoverer::discover`] takes an e-mail address or a domain. As in the CalDAV search
//! (`torro_pim_caldav::discover`, task M7.1) only the domain is looked at and sent anywhere, never
//! the part before `@`. Then, until one step finds something (order and reasons from TorroMail):
//!
//! 1. The table of known providers ([`KNOWN`]): GMX, WEB.DE, mailbox.org, Posteo, Fastmail,
//!    iCloud, Telekom, Yahoo, with their CalDAV address where known. Gmail and Outlook.com are
//!    "not supported" ([`MailUnsupported`]): both sign in with OAuth, which needs a registered
//!    client; Gmail still gets its servers and [`MailDiscovery::app_password`], since an app
//!    password works there with 2-step verification on. No network.
//! 2. MX of the domain: mail that goes to Google or Microsoft is "not supported" the same way.
//!    The MX is the domain owner's own word on where mail lives, so it is believed over the
//!    autoconfig a shared hoster keeps serving after the mailboxes moved.
//! 3. `https://autoconfig.<domain>/mail/config-v1.1.xml` (Mozilla autoconfig).
//! 4. `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml`.
//! 5. Thunderbird's ISPDB, `https://autoconfig.thunderbird.net/v1.1/<domain>`: here the domain
//!    leaves for a third party (the catalogue entry of `accounts.discover_services` says so).
//! 6. SPF (the domain's TXT `v=spf1 …`): `include:` of Google or Microsoft → not supported. Only
//!    down here, since SPF names who may send for the domain, a weaker claim than the MX.
//! 7. The base domain of the first MX (`mxext1.mailbox.org` → `mailbox.org`) against the table.
//! 8. DNS SRV `_imaps._tcp.<domain>` and `_submission._tcp.<domain>` (RFC 6186).
//!
//! No step connects to a mail server: unlike TorroMail nothing guesses `imap.<domain>` and knocks
//! on port 993, since that would be a TCP probe of foreign hosts on an agent's word. The network
//! steps have [`BUDGET`] together; past it the answer is "nothing found".
//!
//! The command is open to agents, so the guards of M7.1 hold for every host found on the way,
//! before it is asked or handed out:
//!
//! - autoconfig addresses go without `?emailaddress=`: the local part stays on this computer;
//! - every host name from MX, SRV or a document passes `checked_domain` (syntax, at most 253
//!   characters, no IP literal, no `localhost`/`.local`/`.lan` …), else it is dropped;
//! - IMAP ports only 993 and 143, SMTP only 465 and 587 ([`IMAP_PORTS`], [`SMTP_PORTS`]); a server
//!   on another port is dropped;
//! - a host handed out from DNS or a document must resolve to public addresses only
//!   ([`is_public`]); the table's own hosts are trusted as they are;
//! - HTTP goes through [`GuardedDns`] (it connects only to addresses that passed the check), https
//!   only, no proxy, at most [`MAX_REDIRECTS`] redirects, short timeouts, documents of at most
//!   [`MAX_DOCUMENT`] bytes, no DTD (`roxmltree` refuses them);
//! - the answer holds host names, ports and fixed texts only: never a document's display name or
//!   any other content of a server.

use std::cmp::Reverse;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use torro_mail::{Endpoint, Security};
use torro_pim_caldav::discover::{
    DiscoverError, GuardedDns, MAX_REDIRECTS, MAX_TARGETS, ProviderError, Resolver, SystemResolver, checked_domain,
    domain_of, is_public,
};

/// How long the network steps of one search may take together (as in TorroMail). The table
/// answers without the network and is not counted.
pub const BUDGET: Duration = Duration::from_secs(8);
/// How long connecting to one web server may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long one request may take, redirects included.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// The largest autoconfig document read; one is a few kilobytes, anything larger is not one.
pub const MAX_DOCUMENT: usize = 256 * 1024;
/// Thunderbird's shared table of mail providers (ISPDB).
pub const ISPDB_HOST: &str = "autoconfig.thunderbird.net";
/// The ports an IMAP server may have: 993 (TLS) and 143 (STARTTLS).
pub const IMAP_PORTS: [u16; 2] = [993, 143];
/// The ports an SMTP server may have: 465 (TLS) and 587 (submission with STARTTLS).
pub const SMTP_PORTS: [u16; 2] = [465, 587];

/// What a search for the mail servers of a domain found. "Nothing found" is an answer too: no
/// servers, no reason, no [`MailDiscovery::source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDiscovery {
    /// Who runs the mail: a name from the table (`mailbox.org`, `Gmail`), `Google Workspace` or
    /// `Microsoft 365` for a domain whose mail goes there, else the domain itself. Never a name a
    /// server sent.
    pub provider: String,
    /// The IMAP server to read mail from; `None` when nothing was found or the provider is not
    /// supported without an app password.
    pub imap: Option<Endpoint>,
    /// The SMTP server to send through; may be `None` while `imap` is set (an autoconfig document
    /// without one, no `_submission._tcp` record): a person enters it then.
    pub smtp: Option<Endpoint>,
    /// What to sign in with; `None` when the source does not say (a document naming a fixed
    /// user name).
    pub login: Option<LoginHint>,
    /// The provider wants an app password instead of the account password (iCloud, Fastmail,
    /// Yahoo, Gmail): the person creates one on the provider's site first.
    pub app_password: bool,
    /// Where the same provider keeps calendars and tasks (CalDAV), from the table only; the core
    /// uses it when the CalDAV search itself found nothing.
    pub caldav: Option<String>,
    /// The usual sign-in does not work with Torro Workplace. With `app_password` the servers are
    /// still given (Gmail); without, they are `None` (Microsoft).
    pub unsupported: Option<MailUnsupported>,
    /// Which step found it, for tests and the person's understanding; `None` for nothing found.
    pub source: Option<Source>,
}

impl MailDiscovery {
    /// Nothing found for a domain run by `provider`.
    pub fn nothing(provider: &str) -> MailDiscovery {
        MailDiscovery {
            provider: provider.to_string(),
            imap: None,
            smtp: None,
            login: None,
            app_password: false,
            caldav: None,
            unsupported: None,
            source: None,
        }
    }
}

/// What to sign in to the servers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginHint {
    /// The whole address, `anna@example.org` (autoconfig `%EMAILADDRESS%`; the table, SRV).
    Address,
    /// The part before `@`, `anna` (autoconfig `%EMAILLOCALPART%`).
    LocalPart,
}

/// Why a provider's mail does not work with a plain password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailUnsupported {
    /// Gmail and Google Workspace: OAuth with a registered client, or an app password.
    Google,
    /// Outlook.com, Hotmail, Live and Microsoft 365: OAuth only.
    Microsoft,
}

impl MailUnsupported {
    /// The reason in English, for people; the core translates it (`torro_core::i18n::tr`).
    pub fn reason(self) -> &'static str {
        match self {
            MailUnsupported::Google => {
                "Gmail needs a sign-in through Google, which Torro Workplace does not support yet. With 2-Step \
                 Verification on, an app password from your Google account works instead."
            }
            MailUnsupported::Microsoft => {
                "Outlook.com and Microsoft 365 need a sign-in through Microsoft, which Torro Workplace does not \
                 support yet."
            }
        }
    }
}

/// Which step of the module docs found the servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Step 1, the table.
    Table,
    /// Step 2, the MX named Google or Microsoft.
    Mx,
    /// Step 3, `autoconfig.<domain>`.
    Autoconfig,
    /// Step 4, `<domain>/.well-known/autoconfig`.
    WellKnown,
    /// Step 5, Thunderbird's ISPDB.
    Ispdb,
    /// Step 6, SPF named Google or Microsoft.
    Spf,
    /// Step 7, the MX's base domain is in the table.
    MxTable,
    /// Step 8, DNS SRV (RFC 6186).
    Srv,
}

/// The two kinds of server, with their ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Service {
    Imap,
    Smtp,
}

impl Service {
    /// The port a document without `<port>` means.
    fn default_port(self) -> u16 {
        match self {
            Service::Imap => 993,
            Service::Smtp => 587,
        }
    }

    /// `host:port` as an endpoint, if the host is a plain internet name and the port one of this
    /// service's ([`IMAP_PORTS`], [`SMTP_PORTS`]). The security is `stated` or what the port
    /// implies (993/465 TLS, 143/587 STARTTLS); there is no plain connection.
    fn endpoint(self, host: &str, port: u16, stated: Option<Security>) -> Option<Endpoint> {
        let implied = match (self, port) {
            (Service::Imap, 993) | (Service::Smtp, 465) => Security::Tls,
            (Service::Imap, 143) | (Service::Smtp, 587) => Security::StartTls,
            _ => return None,
        };
        let host = checked_domain(host)?;
        Some(Endpoint { host, port, security: stated.unwrap_or(implied) })
    }
}

/// What a known provider offers for mail.
enum Offer {
    /// IMAP on 993 with TLS and SMTP on `smtp_port`, signing in with the address.
    Servers { imap: &'static str, smtp: &'static str, smtp_port: u16, app_password: bool },
    /// Not with a password ([`hyperscaler`]).
    Unsupported(MailUnsupported),
}

/// A provider of the table: its domains, its name, its servers and its CalDAV address.
struct Known {
    domains: &'static [&'static str],
    name: &'static str,
    offer: Offer,
    caldav: Option<&'static str>,
}

/// The providers mail discovery knows by name (servers from TorroMail; CalDAV addresses as in
/// `torro_pim_caldav::discover`, those of GMX, WEB.DE and Yahoo to be checked in task M7.3).
const KNOWN: &[Known] = &[
    Known {
        domains: &["gmx.de", "gmx.net", "gmx.at", "gmx.ch"],
        name: "GMX",
        offer: Offer::Servers { imap: "imap.gmx.net", smtp: "mail.gmx.net", smtp_port: 587, app_password: false },
        caldav: Some("https://caldav.gmx.net/"),
    },
    Known {
        domains: &["web.de"],
        name: "WEB.DE",
        offer: Offer::Servers { imap: "imap.web.de", smtp: "smtp.web.de", smtp_port: 587, app_password: false },
        caldav: Some("https://caldav.web.de/"),
    },
    Known {
        domains: &["mailbox.org"],
        name: "mailbox.org",
        offer: Offer::Servers {
            imap: "imap.mailbox.org",
            smtp: "smtp.mailbox.org",
            smtp_port: 587,
            app_password: false,
        },
        caldav: Some("https://dav.mailbox.org/"),
    },
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
        offer: Offer::Servers { imap: "posteo.de", smtp: "posteo.de", smtp_port: 587, app_password: false },
        caldav: Some("https://posteo.de:8443/"),
    },
    Known {
        domains: &["fastmail.com", "fastmail.fm", "fastmail.net", "fastmail.org", "fastmail.us", "fastmail.de"],
        name: "Fastmail",
        offer: Offer::Servers {
            imap: "imap.fastmail.com",
            smtp: "smtp.fastmail.com",
            smtp_port: 587,
            app_password: true,
        },
        caldav: Some("https://caldav.fastmail.com/"),
    },
    Known {
        domains: &["icloud.com", "me.com", "mac.com"],
        name: "iCloud",
        offer: Offer::Servers {
            imap: "imap.mail.me.com",
            smtp: "smtp.mail.me.com",
            smtp_port: 587,
            app_password: true,
        },
        caldav: Some("https://caldav.icloud.com/"),
    },
    Known {
        domains: &["t-online.de", "magenta.de"],
        name: "Telekom",
        offer: Offer::Servers {
            imap: "secureimap.t-online.de",
            smtp: "securesmtp.t-online.de",
            smtp_port: 587,
            app_password: false,
        },
        caldav: None,
    },
    Known {
        domains: &["yahoo.com", "yahoo.de", "ymail.com"],
        name: "Yahoo",
        offer: Offer::Servers {
            imap: "imap.mail.yahoo.com",
            smtp: "smtp.mail.yahoo.com",
            smtp_port: 587,
            app_password: true,
        },
        caldav: Some("https://caldav.calendar.yahoo.com/"),
    },
    Known {
        domains: &["gmail.com", "googlemail.com"],
        name: "Gmail",
        offer: Offer::Unsupported(MailUnsupported::Google),
        caldav: None,
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
        offer: Offer::Unsupported(MailUnsupported::Microsoft),
        caldav: None,
    },
];

/// The table entry of `domain` (lowercase ASCII, as `domain_of` makes it).
fn known(domain: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|known| known.domains.contains(&domain))
}

impl Known {
    /// This entry as an answer found by `source`.
    fn found(&self, source: Source) -> MailDiscovery {
        let mut found = match self.offer {
            Offer::Servers { imap, smtp, smtp_port, app_password } => MailDiscovery {
                imap: Some(Endpoint { host: imap.into(), port: 993, security: Security::Tls }),
                smtp: Service::Smtp.endpoint(smtp, smtp_port, None),
                login: Some(LoginHint::Address),
                app_password,
                source: Some(source),
                ..MailDiscovery::nothing(self.name)
            },
            Offer::Unsupported(which) => hyperscaler_found(which, self.name, source),
        };
        found.caldav = self.caldav.map(str::to_string);
        found
    }
}

/// The answer for mail at Google or Microsoft, named `provider`: "not supported"; for Google with
/// its servers and an app password (it lets one in with 2-step verification on).
fn hyperscaler_found(which: MailUnsupported, provider: &str, source: Source) -> MailDiscovery {
    let nothing = MailDiscovery { unsupported: Some(which), source: Some(source), ..MailDiscovery::nothing(provider) };
    match which {
        MailUnsupported::Google => MailDiscovery {
            imap: Some(Endpoint { host: "imap.gmail.com".into(), port: 993, security: Security::Tls }),
            smtp: Some(Endpoint { host: "smtp.gmail.com".into(), port: 465, security: Security::Tls }),
            login: Some(LoginHint::Address),
            app_password: true,
            ..nothing
        },
        MailUnsupported::Microsoft => nothing,
    }
}

/// What a domain whose mail goes to Google or Microsoft is called.
fn hosted_name(which: MailUnsupported) -> &'static str {
    match which {
        MailUnsupported::Google => "Google Workspace",
        MailUnsupported::Microsoft => "Microsoft 365",
    }
}

/// Whether `host` (an MX or a server of a document) belongs to Google or Microsoft: the host
/// itself or a name below `google.com`, `googlemail.com`, `gmail.com`, `outlook.com` or
/// `office365.com`. A look at the name only, nothing is asked.
fn hyperscaler(host: &str) -> Option<MailUnsupported> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let under = |base: &str| host == base || host.strip_suffix(base).is_some_and(|rest| rest.ends_with('.'));
    if ["google.com", "googlemail.com", "gmail.com"].into_iter().any(under) {
        Some(MailUnsupported::Google)
    } else if ["outlook.com", "office365.com"].into_iter().any(under) {
        Some(MailUnsupported::Microsoft)
    } else {
        None
    }
}

/// Google or Microsoft as an SPF record names them (`include:_spf.google.com`,
/// `include:spf.protection.outlook.com`); `None` for a TXT record that is no SPF record or names
/// neither. Microsoft wins when both are named: a tenant that moved keeps the old include longer.
fn from_spf(record: &str) -> Option<MailUnsupported> {
    let record = record.trim().to_ascii_lowercase();
    if !record.starts_with("v=spf1") {
        return None;
    }
    let includes = |name: &str| record.split_whitespace().any(|term| term.trim_start_matches(['+', '~', '?']) == name);
    if includes("include:spf.protection.outlook.com") {
        Some(MailUnsupported::Microsoft)
    } else if includes("include:_spf.google.com") {
        Some(MailUnsupported::Google)
    } else {
        None
    }
}

/// The last two labels of `host` (`aspmx.l.google.com` → `google.com`). Crude on purpose: it
/// feeds a table lookup that either hits or does not.
fn base_domain(host: &str) -> String {
    let labels: Vec<&str> = host.split('.').filter(|label| !label.is_empty()).collect();
    labels[labels.len().saturating_sub(2)..].join(".")
}

/// What an autoconfig document says, checked by syntax only (host names, ports).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Document {
    imap: Endpoint,
    smtp: Option<Endpoint>,
    login: Option<LoginHint>,
}

/// Reads Mozilla's client configuration (`clientConfig`, what autoconfig and the ISPDB serve):
/// the first `incomingServer type="imap"` whose host and port pass [`Service::endpoint`], the
/// first such SMTP `outgoingServer`, and the user name of the IMAP server. POP is skipped.
/// `socketType` `SSL`/`TLS`/`STARTTLS` is kept, `plain` and anything else fall back to what the
/// port implies. `None` without a usable IMAP server, or for anything that is not XML (a DTD is
/// refused by the parser).
fn parse_autoconfig(xml: &str) -> Option<Document> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let elements = |tag: &'static str| {
        document.descendants().filter(move |node| node.is_element() && node.tag_name().name() == tag)
    };
    let of_type =
        |node: &roxmltree::Node<'_, '_>, kind: &str| node.attribute("type").map(|t| t.eq_ignore_ascii_case(kind));
    let (imap, login) = elements("incomingServer")
        .filter(|node| of_type(node, "imap") == Some(true))
        .find_map(|node| Some((server_of(node, Service::Imap)?, login_of(node))))?;
    // An outgoing server without a type is SMTP, the only kind the format knows.
    let smtp = elements("outgoingServer")
        .filter(|node| of_type(node, "smtp") != Some(false))
        .find_map(|node| server_of(node, Service::Smtp));
    Some(Document { imap, smtp, login })
}

/// The text of the first child element `name` of `node`, trimmed.
fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children().find(|child| child.is_element() && child.tag_name().name() == name)?.text().map(str::trim)
}

/// The server a `incomingServer`/`outgoingServer` element names, if usable.
fn server_of(node: roxmltree::Node<'_, '_>, service: Service) -> Option<Endpoint> {
    let host = child_text(node, "hostname")?;
    let port = match child_text(node, "port") {
        Some(port) => port.parse().ok()?,
        None => service.default_port(),
    };
    let stated = child_text(node, "socketType").and_then(|kind| match kind.to_ascii_uppercase().as_str() {
        "SSL" | "TLS" => Some(Security::Tls),
        "STARTTLS" => Some(Security::StartTls),
        _ => None,
    });
    service.endpoint(host, port, stated)
}

/// The login an `incomingServer` asks for: none stated or `%EMAILADDRESS%` is the address,
/// `%EMAILLOCALPART%` the part before `@`, anything else (a fixed name) unknown.
fn login_of(node: roxmltree::Node<'_, '_>) -> Option<LoginHint> {
    match child_text(node, "username") {
        None | Some("%EMAILADDRESS%") => Some(LoginHint::Address),
        Some("%EMAILLOCALPART%") => Some(LoginHint::LocalPart),
        Some(_) => None,
    }
}

/// How the search talks to the web: the scheme of every address and redirect, the port of the
/// web servers (`None`: the scheme's own), which addresses it may connect to and its time.
/// Always [`Web::HTTPS`], except in this module's tests, whose server is plain `http` on the
/// loopback interface.
#[derive(Clone, Copy)]
struct Web {
    scheme: &'static str,
    port: Option<u16>,
    allowed: fn(IpAddr) -> bool,
    budget: Duration,
}

impl Web {
    const HTTPS: Web = Web { scheme: "https", port: None, allowed: is_public, budget: BUDGET };

    /// `scheme://host` with `:port` unless it is the scheme's own.
    fn origin(&self, host: &str) -> String {
        let own = if self.scheme == "https" { 443 } else { 80 };
        match self.port {
            Some(port) if port != own => format!("{}://{host}:{port}", self.scheme),
            _ => format!("{}://{host}", self.scheme),
        }
    }
}

/// Finds the mail servers of an address; see the module docs.
pub struct MailDiscoverer {
    resolver: Arc<dyn Resolver>,
    http: reqwest::Client,
    web: Web,
}

impl MailDiscoverer {
    /// The search on the system's DNS ([`SystemResolver`]), as the core runs it.
    pub fn system() -> Result<MailDiscoverer, ProviderError> {
        MailDiscoverer::new(Arc::new(SystemResolver::new()))
    }

    /// The search with `resolver` for every name, https only and public addresses only. Fails
    /// only when no HTTP client can be built.
    pub fn new(resolver: Arc<dyn Resolver>) -> Result<MailDiscoverer, ProviderError> {
        MailDiscoverer::with_web(resolver, Web::HTTPS)
    }

    fn with_web(resolver: Arc<dyn Resolver>, web: Web) -> Result<MailDiscoverer, ProviderError> {
        let (scheme, port) = (web.scheme, web.port);
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if usable(attempt.url(), scheme, port) {
                attempt.follow()
            } else {
                attempt.error("a redirect to an address discovery does not follow")
            }
        });
        let http = reqwest::Client::builder()
            .user_agent("Torro Workplace")
            // A proxy would resolve the names itself, past the guard.
            .no_proxy()
            .https_only(scheme == "https")
            .redirect(policy)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .dns_resolver(Arc::new(GuardedDns::new(Arc::clone(&resolver), web.allowed)))
            .build()
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        Ok(MailDiscoverer { resolver, http, web })
    }

    /// Searches the mail servers of `address` (an e-mail address or a domain) in the order of
    /// the module docs, without signing in. `Err(NoDomain)` for an input that names no internet
    /// domain; `Err(Offline)` when the table does not know the domain and neither DNS nor any
    /// web server answered. Everything else is an `Ok`: servers, "not supported", or nothing
    /// ([`MailDiscovery::nothing`], also when [`BUDGET`] ran out).
    pub async fn discover(&self, address: &str) -> Result<MailDiscovery, DiscoverError> {
        let domain = domain_of(address).ok_or(DiscoverError::NoDomain)?;
        if let Some(known) = known(&domain) {
            return Ok(known.found(Source::Table));
        }
        let mut reached = false;
        let found = tokio::time::timeout(self.web.budget, self.search(&domain, &mut reached)).await.ok().flatten();
        match found {
            Some(found) => Ok(found),
            None if reached => Ok(MailDiscovery::nothing(&domain)),
            None => Err(DiscoverError::Offline),
        }
    }

    /// Steps 2 to 8 of the module docs; `reached` is set once DNS or a web server answered.
    async fn search(&self, domain: &str, reached: &mut bool) -> Option<MailDiscovery> {
        let exchangers = self.exchangers(domain, reached).await;
        if let Some(which) = exchangers.iter().find_map(|host| hyperscaler(host)) {
            return Some(hyperscaler_found(which, hosted_name(which), Source::Mx));
        }
        let documents = [
            (format!("{}/mail/config-v1.1.xml", self.web.origin(&format!("autoconfig.{domain}"))), Source::Autoconfig),
            (format!("{}/.well-known/autoconfig/mail/config-v1.1.xml", self.web.origin(domain)), Source::WellKnown),
            (format!("{}/v1.1/{domain}", self.web.origin(ISPDB_HOST)), Source::Ispdb),
        ];
        for (url, source) in documents {
            if let Some(found) = self.read_document(&url, domain, source, reached).await {
                return Some(found);
            }
        }
        if let Some(which) = self.spf(domain, reached).await {
            return Some(hyperscaler_found(which, hosted_name(which), Source::Spf));
        }
        if let Some(base) = exchangers.first().map(|host| base_domain(host)).filter(|base| base != domain)
            && let Some(entry) = known(&base)
        {
            return Some(entry.found(Source::MxTable));
        }
        self.ask_srv(domain, reached).await
    }

    /// The MX hosts of `domain` that pass `checked_domain`, best first; a null MX (`.`) and
    /// odd names are dropped.
    async fn exchangers(&self, domain: &str, reached: &mut bool) -> Vec<String> {
        let Ok(mut records) = self.resolver.mx(domain).await else { return Vec::new() };
        *reached = true;
        records.sort_by_key(|record| record.preference);
        records.iter().filter_map(|record| checked_domain(&record.exchange)).collect()
    }

    /// Google or Microsoft as the SPF record of `domain` names them.
    async fn spf(&self, domain: &str, reached: &mut bool) -> Option<MailUnsupported> {
        let records = self.resolver.txt(domain).await.ok()?;
        *reached = true;
        records.iter().find_map(|record| from_spf(record))
    }

    /// The servers the autoconfig document at `url` names, once they passed [`Self::checked`].
    async fn read_document(
        &self,
        url: &str,
        domain: &str,
        source: Source,
        reached: &mut bool,
    ) -> Option<MailDiscovery> {
        let document = parse_autoconfig(&self.fetch(url, reached).await?)?;
        self.checked(domain, document, source).await
    }

    /// The servers `_imaps._tcp.<domain>` and `_submission._tcp.<domain>` name (RFC 6186).
    async fn ask_srv(&self, domain: &str, reached: &mut bool) -> Option<MailDiscovery> {
        let imap = self.srv_endpoint(&format!("_imaps._tcp.{domain}"), Service::Imap, reached).await?;
        let smtp = self.srv_endpoint(&format!("_submission._tcp.{domain}"), Service::Smtp, reached).await;
        self.checked(domain, Document { imap, smtp, login: Some(LoginHint::Address) }, Source::Srv).await
    }

    /// The best of the first [`MAX_TARGETS`] SRV records of `name` that is a usable endpoint of
    /// `service` and resolves to allowed addresses only; `.` ("no such service"), IP literals and
    /// other ports are dropped.
    async fn srv_endpoint(&self, name: &str, service: Service, reached: &mut bool) -> Option<Endpoint> {
        let mut records = self.resolver.srv(name).await.ok()?;
        *reached = true;
        records.sort_by_key(|record| (record.priority, Reverse(record.weight)));
        for record in records.iter().take(MAX_TARGETS) {
            if let Some(endpoint) = service.endpoint(&record.target, record.port, None)
                && self.resolves_publicly(&endpoint.host).await
            {
                return Some(endpoint);
            }
        }
        None
    }

    /// `document` as an answer for `domain`: Google's or Microsoft's servers make it "not
    /// supported"; else its IMAP server must resolve to allowed addresses only (or the step found
    /// nothing), and so must its SMTP server (or it is left out).
    async fn checked(&self, domain: &str, document: Document, source: Source) -> Option<MailDiscovery> {
        if let Some(which) = hyperscaler(&document.imap.host) {
            return Some(hyperscaler_found(which, hosted_name(which), source));
        }
        if !self.resolves_publicly(&document.imap.host).await {
            return None;
        }
        let smtp = match document.smtp {
            Some(smtp) if self.resolves_publicly(&smtp.host).await => Some(smtp),
            _ => None,
        };
        Some(MailDiscovery {
            imap: Some(document.imap),
            smtp,
            login: document.login,
            source: Some(source),
            ..MailDiscovery::nothing(domain)
        })
    }

    /// Whether `host` has addresses and every one of them may be connected to.
    async fn resolves_publicly(&self, host: &str) -> bool {
        match self.resolver.ips(host).await {
            Ok(ips) => !ips.is_empty() && ips.iter().all(|ip| (self.web.allowed)(*ip)),
            Err(_) => false,
        }
    }

    /// The body of `url` when it answers `200 OK` with at most [`MAX_DOCUMENT`] bytes of UTF-8.
    /// `reached` is set when a web server answered at all.
    async fn fetch(&self, url: &str, reached: &mut bool) -> Option<String> {
        let mut response = match self.http.get(url).header(reqwest::header::ACCEPT, "application/xml").send().await {
            Ok(response) => response,
            Err(err) => {
                *reached |= err.is_redirect();
                return None;
            }
        };
        *reached = true;
        if response.status() != reqwest::StatusCode::OK
            || response.content_length().is_some_and(|length| length > MAX_DOCUMENT as u64)
        {
            return None;
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_DOCUMENT {
                return None;
            }
        }
        String::from_utf8(body).ok()
    }
}

/// Whether the search follows a redirect to `url`: the expected scheme on the web servers' port
/// (`port`, `None` for the scheme's own: a document's server cannot send the search to any port
/// of a public host), a domain name (no IP literal) and no user name or password in it.
fn usable(url: &Url, scheme: &str, port: Option<u16>) -> bool {
    let own = if scheme == "https" { 443 } else { 80 };
    url.scheme() == scheme
        && url.port_or_known_default() == Some(port.unwrap_or(own))
        && url.domain().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use httpmock::MockServer;
    use serde_json::Value;
    use torro_pim_caldav::discover::{Lookup, MxRecord, SrvRecord};

    use super::*;

    /// TorroMail's shared cases (`contracts/provider-discovery.json`): table, MX, SPF, autoconfig
    /// and app passwords. `mailbox_names` belongs to the folders (task M2.3).
    const CASES: &str = include_str!("../tests/fixtures/provider-discovery.json");

    fn cases(section: &str) -> Vec<Value> {
        let all: Value = serde_json::from_str(CASES).expect("the fixture is JSON");
        all[section].as_array().cloned().expect("a section of cases")
    }

    fn security(name: &str) -> Security {
        match name {
            "tls" => Security::Tls,
            "starttls" => Security::StartTls,
            other => panic!("unknown security {other}"),
        }
    }

    /// What TorroMail's expected answer means here: OAuth is "not supported" (Google with its
    /// servers and an app password, Microsoft without servers), an empty SMTP host is none.
    /// IMAP, SMTP, app password and reason: what the fixture's cases compare.
    type Answer = (Option<Endpoint>, Option<Endpoint>, bool, Option<MailUnsupported>);

    fn expected(case: &Value) -> Option<Answer> {
        let expected = case["expected"].as_object()?;
        let field = |name: &str| expected[name].as_str().expect("a text").to_string();
        let port = |name: &str| u16::try_from(expected[name].as_u64().expect("a port")).expect("a port");
        let endpoint = |kind: &str| {
            let host = field(&format!("{kind}_host"));
            (!host.is_empty()).then(|| Endpoint {
                host,
                port: port(&format!("{kind}_port")),
                security: security(&field(&format!("{kind}_security"))),
            })
        };
        let auth = &expected["auth"];
        match (auth["kind"].as_str(), auth["issuer"].as_str()) {
            (Some("oauth"), Some("microsoft")) => Some((None, None, false, Some(MailUnsupported::Microsoft))),
            (Some("oauth"), Some("google")) => {
                Some((endpoint("imap"), endpoint("smtp"), true, Some(MailUnsupported::Google)))
            }
            (Some(kind), _) => Some((endpoint("imap"), endpoint("smtp"), kind == "app_password", None)),
            (None, _) => panic!("no auth in {case}"),
        }
    }

    fn answer(found: &MailDiscovery) -> Answer {
        (found.imap.clone(), found.smtp.clone(), found.app_password, found.unsupported)
    }

    #[test]
    fn the_table_answers_as_torromail_does() {
        for case in cases("domains") {
            let input = case["input"].as_str().expect("a domain");
            let found = domain_of(input).and_then(|domain| known(&domain)).map(|known| known.found(Source::Table));
            assert_eq!(found.as_ref().map(answer), expected(&case), "{input}");
        }
        let mailbox = known("mailbox.org").expect("known").found(Source::Table);
        assert_eq!(mailbox.caldav.as_deref(), Some("https://dav.mailbox.org/"));
        assert_eq!((mailbox.login, mailbox.provider.as_str()), (Some(LoginHint::Address), "mailbox.org"));
    }

    #[test]
    fn mx_and_spf_name_google_and_microsoft_only() {
        for case in cases("mx") {
            let input = case["input"].as_str().expect("an MX host");
            let found = hyperscaler(input).map(|which| hyperscaler_found(which, hosted_name(which), Source::Mx));
            assert_eq!(found.as_ref().map(answer), expected(&case), "{input}");
        }
        for case in cases("spf") {
            let input = case["input"].as_str().expect("a TXT record");
            let found = from_spf(input).map(|which| hyperscaler_found(which, hosted_name(which), Source::Spf));
            assert_eq!(found.as_ref().map(answer), expected(&case), "{input}");
        }
        // A name that only ends like Google's is not Google's.
        assert_eq!(hyperscaler("mx.notgoogle.com"), None);
        assert_eq!(hyperscaler("evil-outlook.com."), None);
    }

    #[test]
    fn autoconfig_documents_are_read_as_torromail_does() {
        for case in cases("autoconfig") {
            let name = case["name"].as_str().expect("a name");
            let document = parse_autoconfig(case["xml"].as_str().expect("xml"));
            let found = document.map(|document| match hyperscaler(&document.imap.host) {
                Some(which) => hyperscaler_found(which, hosted_name(which), Source::Autoconfig),
                None => MailDiscovery {
                    imap: Some(document.imap),
                    smtp: document.smtp,
                    ..MailDiscovery::nothing("example.org")
                },
            });
            assert_eq!(found.as_ref().map(answer), expected(&case), "{name}");
        }
    }

    #[test]
    fn only_gmail_falls_back_to_an_app_password() {
        for case in cases("app_password_fallback") {
            let input = case["input"].as_str().expect("a domain");
            let found = known(input).expect("in the table").found(Source::Table);
            let fallback = found.unsupported.is_some() && found.app_password;
            assert_eq!(fallback, !case["expected"].is_null(), "{input}");
        }
    }

    #[test]
    fn the_domain_of_an_address_as_torromail_takes_it() {
        // Deliberately different: discovery also takes a bare domain and the domain after the
        // last `@` (a quoted local part may hold one); only that domain is ever searched.
        let ours = HashMap::from([("a@b@c.example", Some("c.example")), ("@example.com", Some("example.com"))]);
        for case in cases("email_domains") {
            let input = case["input"].as_str().expect("an address");
            let wanted = ours.get(input).copied().unwrap_or(case["expected"].as_str());
            assert_eq!(domain_of(input).as_deref(), wanted, "{input}");
        }
    }

    #[test]
    fn documents_with_odd_hosts_or_ports_give_nothing() {
        let document = |host: &str, port: &str| {
            format!(
                r#"<clientConfig version="1.1"><emailProvider id="x"><incomingServer type="imap">
                <hostname>{host}</hostname><port>{port}</port></incomingServer></emailProvider></clientConfig>"#
            )
        };
        let too_long = format!("{}.example.org", ["imap"; 50].join("."));
        assert!(too_long.len() > 253);
        for (host, port) in [
            ("10.0.0.1", "993"),
            ("[::1]", "993"),
            ("0x7f.1", "993"),
            ("nas.local", "993"),
            ("localhost", "993"),
            ("imap.example.org/x", "993"),
            (too_long.as_str(), "993"),
            ("imap.example.org", "110"),
            ("imap.example.org", "2525"),
            ("imap.example.org", "nine"),
        ] {
            assert_eq!(parse_autoconfig(&document(host, port)), None, "{host}:{port}");
        }
        assert!(parse_autoconfig(&document("imap.example.org", "993")).is_some());
        let dtd = r#"<!DOCTYPE x [<!ENTITY a "imap.example.org">]><clientConfig><emailProvider>
            <incomingServer type="imap"><hostname>&a;</hostname></incomingServer></emailProvider></clientConfig>"#;
        assert_eq!(parse_autoconfig(dtd), None, "no DTD");
        let local_part = r#"<clientConfig><emailProvider><incomingServer type="imap"><hostname>imap.example.org
            </hostname><port>143</port><socketType>plain</socketType><username>%EMAILLOCALPART%</username>
            </incomingServer></emailProvider></clientConfig>"#;
        let read = parse_autoconfig(local_part).expect("a server");
        assert_eq!(read.imap, Endpoint { host: "imap.example.org".into(), port: 143, security: Security::StartTls });
        assert_eq!((read.smtp, read.login), (None, Some(LoginHint::LocalPart)));
    }

    #[test]
    fn base_domains_are_the_last_two_labels() {
        assert_eq!(base_domain("mxext1.mailbox.org"), "mailbox.org");
        assert_eq!(base_domain("mailbox.org"), "mailbox.org");
        assert_eq!(base_domain("org"), "org");
    }

    /// A DNS that answers from tables and notes every name it is asked about.
    #[derive(Default)]
    struct FakeDns {
        mx: HashMap<String, Vec<MxRecord>>,
        txt: HashMap<String, Vec<String>>,
        srv: HashMap<String, Vec<SrvRecord>>,
        ips: HashMap<String, Vec<IpAddr>>,
        /// Every lookup fails, as without network.
        offline: bool,
        /// SRV lookups never answer, as on a network that drops them.
        stalled_srv: bool,
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

        fn with_mx(mut self, domain: &str, hosts: &[&str]) -> FakeDns {
            let records = hosts
                .iter()
                .enumerate()
                .map(|(n, host)| MxRecord { preference: 10 * n as u16, exchange: format!("{host}.") })
                .collect();
            self.mx.insert(domain.into(), records);
            self
        }

        fn with_txt(mut self, domain: &str, records: &[&str]) -> FakeDns {
            self.txt.insert(domain.into(), records.iter().map(|record| record.to_string()).collect());
            self
        }

        fn with_srv(mut self, name: &str, target: &str, port: u16, priority: u16) -> FakeDns {
            let record = SrvRecord { priority, weight: 0, port, target: target.into() };
            self.srv.entry(name.into()).or_default().push(record);
            self
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().expect("lock").clone()
        }
    }

    impl Resolver for FakeDns {
        fn srv<'a>(&'a self, name: &'a str) -> Lookup<'a, SrvRecord> {
            Box::pin(async move {
                if self.stalled_srv {
                    std::future::pending::<()>().await;
                }
                self.answer(&self.srv, name)
            })
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

    /// A public address for the servers documents name (never contacted).
    const PUBLIC: &str = "93.184.215.14";

    /// The test's rule: the public internet and, since the mock server lives there, loopback.
    fn public_or_loopback(ip: IpAddr) -> bool {
        is_public(ip) || ip.is_loopback()
    }

    /// The search over plain http against the mock server on `port`.
    fn on_mock(dns: FakeDns, port: u16) -> (MailDiscoverer, Arc<FakeDns>) {
        on_mock_with(dns, port, public_or_loopback, BUDGET)
    }

    fn on_mock_with(
        dns: FakeDns,
        port: u16,
        allowed: fn(IpAddr) -> bool,
        budget: Duration,
    ) -> (MailDiscoverer, Arc<FakeDns>) {
        let dns = Arc::new(dns);
        let web = Web { scheme: "http", port: Some(port), allowed, budget };
        let resolver: Arc<dyn Resolver> = Arc::clone(&dns) as Arc<dyn Resolver>;
        (MailDiscoverer::with_web(resolver, web).expect("client"), dns)
    }

    const HOSTER: &str = r#"<clientConfig version="1.1"><emailProvider id="hoster"><displayName>Evil &lt;b&gt;</displayName>
        <incomingServer type="imap"><hostname>imap.hoster.example</hostname><port>993</port><socketType>SSL</socketType></incomingServer>
        <outgoingServer type="smtp"><hostname>smtp.hoster.example</hostname><port>465</port><socketType>SSL</socketType></outgoingServer>
        </emailProvider></clientConfig>"#;

    fn tls(host: &str, port: u16) -> Option<Endpoint> {
        Some(Endpoint { host: host.into(), port, security: Security::Tls })
    }

    #[tokio::test]
    async fn a_known_domain_asks_the_network_nothing() {
        let dns = Arc::new(FakeDns::default());
        let discoverer = MailDiscoverer::new(Arc::clone(&dns) as Arc<dyn Resolver>).expect("client");
        let found = discoverer.discover("anna@posteo.net").await.expect("an answer");
        assert_eq!((found.imap, found.source), (tls("posteo.de", 993), Some(Source::Table)));
        let gmail = discoverer.discover("anna@gmail.com").await.expect("an answer");
        assert_eq!((gmail.unsupported, gmail.app_password), (Some(MailUnsupported::Google), true));
        let outlook = discoverer.discover("hotmail.de").await.expect("an answer");
        assert_eq!((outlook.unsupported, outlook.imap), (Some(MailUnsupported::Microsoft), None));
        assert!(dns.asked().is_empty(), "{:?}", dns.asked());
        // Even without any network.
        let offline = MailDiscoverer::new(Arc::new(FakeDns { offline: true, ..FakeDns::default() })).expect("client");
        assert_eq!(offline.discover("web.de").await.map(|found| found.provider), Ok("WEB.DE".into()));
        assert_eq!(offline.discover("anna@example.org").await, Err(DiscoverError::Offline));
        assert_eq!(offline.discover("anna@127.0.0.1").await, Err(DiscoverError::NoDomain));
    }

    #[tokio::test]
    async fn the_mx_is_believed_over_a_stale_autoconfig() {
        let server = MockServer::start_async().await;
        let anything = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).body(HOSTER);
            })
            .await;
        let dns = FakeDns::default()
            .with_mx("firma.example", &["firma-example.mail.protection.outlook.com"])
            .with_ip("autoconfig.firma.example", "127.0.0.1");
        let (discoverer, dns) = on_mock(dns, server.port());
        let found = discoverer.discover("a@firma.example").await.expect("an answer");
        assert_eq!((found.unsupported, found.source), (Some(MailUnsupported::Microsoft), Some(Source::Mx)));
        assert_eq!(found.provider, "Microsoft 365");
        assert_eq!(dns.asked(), ["firma.example"], "nothing but the MX was asked");
        assert_eq!(anything.calls_async().await, 0);
    }

    #[tokio::test]
    async fn the_domains_own_autoconfig_beats_the_ispdb_and_spf_and_keeps_the_local_part_home() {
        let server = MockServer::start_async().await;
        let port = server.port();
        let with_address = server
            .mock_async(|when, then| {
                when.query_param_exists("emailaddress");
                then.status(500);
            })
            .await;
        let well_known = server
            .mock_async(|when, then| {
                when.path("/.well-known/autoconfig/mail/config-v1.1.xml")
                    .header("host", format!("verein.example:{port}"));
                then.status(200).body(HOSTER);
            })
            .await;
        let ispdb = server
            .mock_async(|when, then| {
                when.path("/v1.1/verein.example");
                then.status(200).body(HOSTER);
            })
            .await;
        let dns = FakeDns::default()
            .with_mx("verein.example", &["mx.hoster.example"])
            .with_txt("verein.example", &["v=spf1 include:_spf.google.com ~all"])
            .with_ip("verein.example", "127.0.0.1")
            .with_ip(ISPDB_HOST, "127.0.0.1")
            .with_ip("imap.hoster.example", PUBLIC)
            .with_ip("smtp.hoster.example", PUBLIC);
        let (discoverer, dns) = on_mock(dns, port);
        let found = discoverer.discover("anna.secret@verein.example").await.expect("an answer");
        let expected = MailDiscovery {
            imap: tls("imap.hoster.example", 993),
            smtp: tls("smtp.hoster.example", 465),
            login: Some(LoginHint::Address),
            source: Some(Source::WellKnown),
            ..MailDiscovery::nothing("verein.example")
        };
        assert_eq!(found, expected, "the provider is the domain, never the document's display name");
        assert_eq!((well_known.calls_async().await, ispdb.calls_async().await), (1, 0));
        assert_eq!(with_address.calls_async().await, 0);
        assert!(dns.asked().iter().all(|name| !name.contains("anna")), "{:?}", dns.asked());
        // `autoconfig.verein.example` has no address, so nothing was sent there.
        assert!(dns.asked().contains(&"autoconfig.verein.example".to_string()));
    }

    #[tokio::test]
    async fn the_ispdb_gets_the_domain_when_the_domain_publishes_nothing() {
        let server = MockServer::start_async().await;
        let ispdb = server
            .mock_async(|when, then| {
                when.path("/v1.1/klein.example");
                then.status(200).body(HOSTER);
            })
            .await;
        let dns = FakeDns::default()
            .with_ip(ISPDB_HOST, "127.0.0.1")
            .with_ip("imap.hoster.example", PUBLIC)
            .with_ip("smtp.hoster.example", PUBLIC);
        let (discoverer, _) = on_mock(dns, server.port());
        let found = discoverer.discover("klein.example").await.expect("an answer");
        assert_eq!((found.imap, found.source), (tls("imap.hoster.example", 993), Some(Source::Ispdb)));
        assert_eq!(ispdb.calls_async().await, 1);
    }

    #[tokio::test]
    async fn servers_of_a_document_on_private_addresses_are_not_handed_out() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/v1.1/firma.example");
                then.status(200).body(HOSTER);
            })
            .await;
        // The IMAP server is inside the person's network: the step found nothing.
        let dns = FakeDns::default()
            .with_ip(ISPDB_HOST, "127.0.0.1")
            .with_ip("imap.hoster.example", "10.0.0.5")
            .with_ip("smtp.hoster.example", PUBLIC);
        let (discoverer, _) = on_mock(dns, server.port());
        assert_eq!(discoverer.discover("firma.example").await, Ok(MailDiscovery::nothing("firma.example")));
        // Only the SMTP server is: it is left out.
        let dns = FakeDns::default()
            .with_ip(ISPDB_HOST, "127.0.0.1")
            .with_ip("imap.hoster.example", PUBLIC)
            .with_ip("smtp.hoster.example", "192.168.1.2");
        let (discoverer, _) = on_mock(dns, server.port());
        let found = discoverer.discover("firma.example").await.expect("an answer");
        assert_eq!((found.imap.is_some(), found.smtp), (true, None));
    }

    #[tokio::test]
    async fn web_servers_on_private_addresses_are_never_contacted() {
        let server = MockServer::start_async().await;
        let anything = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(200).body(HOSTER);
            })
            .await;
        // The production rule: every web server of the search resolves to loopback.
        let dns = FakeDns::default()
            .with_ip("autoconfig.firma.example", "127.0.0.1")
            .with_ip("firma.example", "127.0.0.1")
            .with_ip(ISPDB_HOST, "127.0.0.1");
        let (discoverer, _) = on_mock_with(dns, server.port(), is_public, BUDGET);
        assert_eq!(discoverer.discover("firma.example").await, Ok(MailDiscovery::nothing("firma.example")));
        assert_eq!(anything.calls_async().await, 0);
    }

    #[tokio::test]
    async fn redirects_to_private_hosts_or_ip_literals_are_not_followed() {
        let server = MockServer::start_async().await;
        let port = server.port();
        server
            .mock_async(|when, then| {
                when.path("/mail/config-v1.1.xml").header("host", format!("autoconfig.firma.example:{port}"));
                then.status(302).header("Location", format!("http://intranet.firma.example:{port}/config.xml"));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.path("/.well-known/autoconfig/mail/config-v1.1.xml");
                then.status(302).header("Location", format!("http://127.0.0.1:{port}/config.xml"));
            })
            .await;
        let target = server
            .mock_async(|when, then| {
                when.path("/config.xml");
                then.status(200).body(HOSTER);
            })
            .await;
        let dns = FakeDns::default()
            .with_ip("autoconfig.firma.example", "127.0.0.1")
            .with_ip("firma.example", "127.0.0.1")
            .with_ip("intranet.firma.example", "10.0.0.7")
            .with_ip("imap.hoster.example", PUBLIC);
        let (discoverer, _) = on_mock(dns, port);
        assert_eq!(discoverer.discover("firma.example").await, Ok(MailDiscovery::nothing("firma.example")));
        assert_eq!(target.calls_async().await, 0);
    }

    #[test]
    fn redirects_stay_on_the_web_servers_port() {
        let url = |text: &str| Url::parse(text).expect("a URL");
        assert!(usable(&url("https://hoster.example/config.xml"), "https", None));
        assert!(usable(&url("https://hoster.example:443/config.xml"), "https", None));
        assert!(!usable(&url("https://hoster.example:25/config.xml"), "https", None));
        assert!(!usable(&url("http://hoster.example/config.xml"), "https", None));
        assert!(!usable(&url("https://user@hoster.example/"), "https", None));
        assert!(usable(&url("http://hoster.example:8080/"), "http", Some(8080)));
        assert!(!usable(&url("http://hoster.example/"), "http", Some(8080)));
    }

    #[tokio::test]
    async fn documents_that_are_too_large_are_not_read() {
        let server = MockServer::start_async().await;
        let padding = " ".repeat(MAX_DOCUMENT);
        server
            .mock_async(|when, then| {
                when.path("/v1.1/firma.example");
                then.status(200).body(format!("{HOSTER}{padding}"));
            })
            .await;
        let dns = FakeDns::default().with_ip(ISPDB_HOST, "127.0.0.1").with_ip("imap.hoster.example", PUBLIC);
        let (discoverer, _) = on_mock(dns, server.port());
        assert_eq!(discoverer.discover("firma.example").await, Ok(MailDiscovery::nothing("firma.example")));
    }

    #[tokio::test]
    async fn spf_rescues_a_tenant_hidden_behind_a_filtering_gateway() {
        let dns = FakeDns::default()
            .with_mx("kanzlei.example", &["mx01.hornetsecurity.com"])
            .with_txt("kanzlei.example", &["some-verification=abc", "v=spf1 include:spf.protection.outlook.com -all"]);
        let discoverer = MailDiscoverer::new(Arc::new(dns)).expect("client");
        let found = discoverer.discover("a@kanzlei.example").await.expect("an answer");
        assert_eq!((found.unsupported, found.source), (Some(MailUnsupported::Microsoft), Some(Source::Spf)));
    }

    #[tokio::test]
    async fn a_third_party_mx_leads_back_to_the_table() {
        let dns = FakeDns::default().with_mx("familie.example", &["mxext1.mailbox.org", "mxext2.mailbox.org"]);
        let discoverer = MailDiscoverer::new(Arc::new(dns)).expect("client");
        let found = discoverer.discover("a@familie.example").await.expect("an answer");
        assert_eq!((found.imap, found.provider.as_str()), (tls("imap.mailbox.org", 993), "mailbox.org"));
        assert_eq!((found.caldav.as_deref(), found.source), (Some("https://dav.mailbox.org/"), Some(Source::MxTable)));
    }

    #[tokio::test]
    async fn srv_names_the_servers_and_odd_targets_are_dropped() {
        let dns = FakeDns::default()
            .with_srv("_imaps._tcp.uni.example", "10.0.0.1", 993, 0)
            .with_srv("_imaps._tcp.uni.example", "intern.uni.example.", 993, 1)
            .with_srv("_imaps._tcp.uni.example", "mailhost.uni.example.", 993, 2)
            .with_srv("_submission._tcp.uni.example", "smtp.uni.example.", 2525, 0)
            .with_srv("_submission._tcp.uni.example", "smtp.uni.example.", 587, 1)
            .with_ip("intern.uni.example", "10.0.0.2")
            .with_ip("mailhost.uni.example", PUBLIC)
            .with_ip("smtp.uni.example", PUBLIC);
        let discoverer = MailDiscoverer::new(Arc::new(dns)).expect("client");
        let found = discoverer.discover("a@uni.example").await.expect("an answer");
        // The IP literal, the private target and port 2525 are passed over.
        assert_eq!((found.imap, found.source), (tls("mailhost.uni.example", 993), Some(Source::Srv)));
        let submission = Endpoint { host: "smtp.uni.example".into(), port: 587, security: Security::StartTls };
        assert_eq!((found.smtp, found.login), (Some(submission), Some(LoginHint::Address)));

        let dns = FakeDns::default().with_srv("_imaps._tcp.uni.example", ".", 993, 0);
        let discoverer = MailDiscoverer::new(Arc::new(dns)).expect("client");
        assert_eq!(discoverer.discover("uni.example").await, Ok(MailDiscovery::nothing("uni.example")));
    }

    #[tokio::test]
    async fn nothing_is_probed_and_an_empty_answer_beats_a_guess() {
        let dns = FakeDns::default().with_ip("imap.nichts.example", PUBLIC).with_ip("mail.nichts.example", PUBLIC);
        let (discoverer, dns) = on_mock_with(dns, 1, public_or_loopback, BUDGET);
        assert_eq!(discoverer.discover("a@nichts.example").await, Ok(MailDiscovery::nothing("nichts.example")));
        let asked = dns.asked();
        assert!(!asked.iter().any(|name| name.starts_with("imap.") || name.starts_with("mail.")), "{asked:?}");
        assert_eq!(asked.last().map(String::as_str), Some("_imaps._tcp.nichts.example"), "the last step");
    }

    #[tokio::test]
    async fn the_budget_ends_the_search_where_it_stands() {
        let dns = FakeDns { stalled_srv: true, ..FakeDns::default() };
        let (discoverer, _) = on_mock_with(dns, 1, public_or_loopback, Duration::from_millis(200));
        let started = std::time::Instant::now();
        assert_eq!(discoverer.discover("a@langsam.example").await, Ok(MailDiscovery::nothing("langsam.example")));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_reasons_say_what_still_works() {
        assert!(MailUnsupported::Google.reason().contains("app password"));
        assert!(MailUnsupported::Microsoft.reason().ends_with('.'));
    }
}
