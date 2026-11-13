//! Content from other providers (issue #672, `docs/settings.md` "Privacy"): one place for the whole suite
//! that says whether pictures of a mail, link previews in the chat and embedded videos are loaded from
//! servers other than the person's own.
//!
//! - [`ForeignContent`] is the level: always, ask (the default) or never. Never is the switch that stops
//!   everything, also above the list.
//! - [`Privacy::providers`] holds only the providers a person decided on ("Always allow …", "Never for …");
//!   reading a mail or a chat writes nothing. Removing one means: ask again.
//! - A provider is named by its host ([`provider_of`]): known services with several domains under one name
//!   ([`KNOWN`], YouTube), an IP address as itself, else the registrable domain (`news.example.co.uk` →
//!   `example.co.uk`) from the Public Suffix List compiled in.
//! - [`Privacy::decide`] is the one rule every part of the suite follows; the service decides, the windows
//!   only ask.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// The most providers the list keeps: a file edited by hand with more is cut when it is read, and
/// `settings.set_setting privacy_provider` refuses to add one beyond it.
pub const MAX_PROVIDERS: usize = 1000;

/// The longest key of a provider: the longest host name DNS allows.
const MAX_KEY: usize = 253;

/// Whether content from other providers is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ForeignContent {
    /// `"always"`: loaded from every provider, without asking.
    Always,
    /// `"ask"` (the default): providers of the list follow it, a new one is asked about where its content
    /// would stand.
    #[default]
    Ask,
    /// `"never"`: nothing is loaded and nothing asked, whatever the list says.
    Never,
}

impl ForeignContent {
    /// Every level, in the order the settings offer them.
    pub const ALL: [ForeignContent; 3] = [ForeignContent::Always, ForeignContent::Ask, ForeignContent::Never];

    /// The level's name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            ForeignContent::Always => "always",
            ForeignContent::Ask => "ask",
            ForeignContent::Never => "never",
        }
    }
}

/// What a person decided for one provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderChoice {
    /// `"allowed"`: its content loads while the level is `ask`.
    Allowed,
    /// `"blocked"`: its content never loads, and nothing is asked.
    Blocked,
}

/// The setting: the level and the providers a person decided on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Privacy {
    /// Whether content from other providers is loaded; `ask` unless changed.
    #[serde(default)]
    pub content: ForeignContent,
    /// The providers a person allowed or blocked, by their key: `youtube` for a known service, else the
    /// domain or IP address. At most 1000 (`MAX_PROVIDERS`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderChoice>,
}

/// A provider of content: who learns that it was loaded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Provider {
    /// Its key in [`Privacy::providers`].
    pub key: String,
    /// Its name for people: `YouTube`, else the key.
    pub name: String,
}

/// What Settings › Privacy says now, read afresh for every use: what the services of the core are
/// given to ask (the settings file in the core, a fixed value in tests).
pub type Reader = std::sync::Arc<dyn Fn() -> Privacy + Send + Sync>;

/// What to do with content of one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Load it.
    Load,
    /// Ask the person first, naming this provider.
    Ask(Provider),
    /// Leave it out, without asking.
    Skip,
}

/// One change of the list, the value of `settings.set_setting privacy_provider`: `{"provider": "youtube",
/// "choice": "allowed" | "blocked" | null}`; `null` removes it, so it is asked about again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderChange {
    /// The provider's key ([`valid_key`]).
    pub provider: String,
    /// What to keep for it; `None` takes it off the list.
    pub choice: Option<ProviderChoice>,
}

impl ProviderChange {
    /// `value` as a change of the list, or why it does not fit: the fields above, a key [`valid_key`]
    /// accepts. Whether the list stays within [`MAX_PROVIDERS`] is checked when it is applied.
    pub fn from_value(value: &serde_json::Value) -> Result<ProviderChange, String> {
        let wrong = |why: String| {
            format!(
                "{value} is not a provider change: send {{\"provider\": \"<key>\", \"choice\": \"allowed\" | \"blocked\" | null}} ({why})"
            )
        };
        // A missing choice would read as `null` and remove the provider: it must be named.
        if value.get("choice").is_none() {
            return Err(wrong("the choice is missing".into()));
        }
        let change: ProviderChange = serde_json::from_value(value.clone()).map_err(|err| wrong(err.to_string()))?;
        if !valid_key(&change.provider) {
            return Err("the provider is a key such as \"youtube\", a domain or an IP address in small letters".into());
        }
        Ok(change)
    }
}

/// A known service whose content comes from several domains: one name, one choice for all of them.
pub struct Known {
    /// Its key in the list.
    pub key: &'static str,
    /// Its name for people.
    pub name: &'static str,
    /// Its domains; their subdomains belong to it too.
    pub domains: &'static [&'static str],
}

/// The services that are named as one ([`provider_of`]).
pub const KNOWN: &[Known] = &[Known {
    key: "youtube",
    name: "YouTube",
    domains: &["youtube.com", "youtu.be", "youtube-nocookie.com", "ytimg.com"],
}];

impl Privacy {
    /// What to do with content from `host` ([`normalize_host`] is applied): `never` skips it, `always` loads
    /// it; with `ask` a provider of the list follows its choice, a new one is asked about, and a host that
    /// names no provider (`localhost`, a bare suffix) is skipped.
    pub fn decide(&self, host: &str) -> Decision {
        match self.content {
            ForeignContent::Never => Decision::Skip,
            ForeignContent::Always => Decision::Load,
            ForeignContent::Ask => match provider_of(host) {
                None => Decision::Skip,
                Some(provider) => match self.providers.get(&provider.key) {
                    Some(ProviderChoice::Allowed) => Decision::Load,
                    Some(ProviderChoice::Blocked) => Decision::Skip,
                    None => Decision::Ask(provider),
                },
            },
        }
    }

    /// Puts `change` into the list; whether it then holds too many is the caller's check
    /// ([`Privacy::check_providers`]).
    pub fn change_provider(&mut self, change: ProviderChange) {
        match change.choice {
            Some(choice) => {
                self.providers.insert(change.provider, choice);
            }
            None => {
                self.providers.remove(&change.provider);
            }
        }
    }

    /// Whether the list may be stored: at most [`MAX_PROVIDERS`] providers.
    pub fn check_providers(&self) -> Result<(), String> {
        match self.providers.len() > MAX_PROVIDERS {
            true => Err(format!("at most {MAX_PROVIDERS} providers can be allowed or blocked; forget some first")),
            false => Ok(()),
        }
    }

    /// Whether this is the default: `ask`, nobody on the list. Not written to the settings file then.
    pub fn is_default(&self) -> bool {
        self == &Privacy::default()
    }
}

/// `privacy` as written, without entries whose key or choice does not fit and with at most
/// [`MAX_PROVIDERS`]; a level that does not read is `ask`. Never makes the settings file fail.
pub fn lenient<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Privacy, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let content = value.get("content").and_then(|level| serde_json::from_value(level.clone()).ok()).unwrap_or_default();
    let providers = value
        .get("providers")
        .and_then(serde_json::Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter(|(key, _)| valid_key(key))
                .filter_map(|(key, choice)| Some((key.clone(), serde_json::from_value(choice.clone()).ok()?)))
                .take(MAX_PROVIDERS)
                .collect()
        })
        .unwrap_or_default();
    Ok(Privacy { content, providers })
}

/// Whether `key` can name a provider: 1 to 253 characters, small ASCII letters, digits, `.`, `-` and the
/// `:` of an IPv6 address – what [`provider_of`] gives.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY
        && key.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-:".contains(&byte))
}

/// `host` as the suite compares hosts: small letters, without port, brackets and the dot at the end. A
/// host from `url::Url::host_str` is already Punycode and small; this makes one written elsewhere
/// (`Example.COM.:443`) the same.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim();
    let host = match host.strip_prefix('[') {
        // `[::1]:443`: the address between the brackets.
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => match host.rsplit_once(':') {
            // One colon is a port; more are an IPv6 address without brackets.
            Some((name, port)) if !name.contains(':') && port.bytes().all(|byte| byte.is_ascii_digit()) => name,
            _ => host,
        },
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The provider of content from `host` ([`normalize_host`] is applied): a [`KNOWN`] service when the host is
/// one of its domains or below one, an IP address as itself, else its registrable domain. `None` for a host
/// that has none (`localhost`, `co.uk`, empty).
pub fn provider_of(host: &str) -> Option<Provider> {
    let host = normalize_host(host);
    if host.is_empty() {
        return None;
    }
    let below = |domain: &str| host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'));
    if let Some(known) = KNOWN.iter().find(|known| known.domains.iter().any(|domain| below(domain))) {
        return Some(Provider { key: known.key.to_string(), name: known.name.to_string() });
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let key = ip.to_string();
        return Some(Provider { name: key.clone(), key });
    }
    // Only a name of more than one label has a registrable domain; `psl` would take a single label for one
    // under its catch-all rule.
    if !host.contains('.') {
        return None;
    }
    let domain = psl::domain_str(&host)?;
    valid_key(domain).then(|| Provider { key: domain.to_string(), name: domain.to_string() })
}

/// What a mail whose HTML names `hosts` may load (issue #672, `mail.get_message`), by the rule of
/// [`Privacy::decide`]: everything at "Always show"; at "Ask" the hosts of allowed providers in `load`
/// and the providers not asked about yet in `ask`, each with its hosts; a blocked provider, a host
/// without one and everything at "Never show" in neither. `None` without hosts: nothing to load.
pub fn external_content(privacy: &Privacy, hosts: &BTreeSet<String>) -> Option<torro_mail::ExternalContent> {
    if hosts.is_empty() {
        return None;
    }
    let mut external =
        torro_mail::ExternalContent { all: privacy.content == ForeignContent::Always, ..Default::default() };
    if external.all {
        return Some(external);
    }
    let mut ask: BTreeMap<Provider, BTreeSet<String>> = BTreeMap::new();
    for host in hosts {
        match privacy.decide(host) {
            Decision::Load => {
                external.load.insert(normalize_host(host));
            }
            Decision::Ask(provider) => {
                ask.entry(provider).or_default().insert(normalize_host(host));
            }
            Decision::Skip => {}
        }
    }
    external.ask = ask
        .into_iter()
        .map(|(provider, hosts)| torro_mail::ExternalProvider { key: provider.key, name: provider.name, hosts })
        .collect();
    Some(external)
}

/// The name of the provider with `key`, for the list in the settings: a [`KNOWN`] service's, else the key.
pub fn name_of(key: &str) -> String {
    KNOWN.iter().find(|known| known.key == key).map_or_else(|| key.to_string(), |known| known.name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(key: &str, name: &str) -> Option<Provider> {
        Some(Provider { key: key.into(), name: name.into() })
    }

    #[test]
    fn hosts_are_compared_alike() {
        assert_eq!(normalize_host("Example.COM."), "example.com");
        assert_eq!(normalize_host("img.example.com:8443"), "img.example.com");
        assert_eq!(normalize_host("[2001:DB8::1]:443"), "2001:db8::1");
        assert_eq!(normalize_host("2001:db8::1"), "2001:db8::1");
        assert_eq!(normalize_host("xn--bcher-kva.example"), "xn--bcher-kva.example");
    }

    #[test]
    fn providers_are_named_by_service_address_or_domain() {
        // Known services: every domain and subdomain under one name.
        for host in ["youtube.com", "www.youtube.com", "youtu.be", "i.ytimg.com", "www.youtube-nocookie.com"] {
            assert_eq!(provider_of(host), provider("youtube", "YouTube"), "{host}");
        }
        // Not a subdomain: only the end of a label counts.
        assert_eq!(provider_of("notyoutube.com"), provider("notyoutube.com", "notyoutube.com"));
        // The registrable domain, also below a public suffix of two labels.
        assert_eq!(provider_of("news.mailchimp.com"), provider("mailchimp.com", "mailchimp.com"));
        assert_eq!(provider_of("img.shop.example.co.uk"), provider("example.co.uk", "example.co.uk"));
        assert_eq!(provider_of("Tracker.Example.ORG."), provider("example.org", "example.org"));
        // An address is its own provider.
        assert_eq!(provider_of("203.0.113.7"), provider("203.0.113.7", "203.0.113.7"));
        assert_eq!(provider_of("[2001:db8::1]"), provider("2001:db8::1", "2001:db8::1"));
        // No provider: a single label, a bare suffix, nothing.
        assert_eq!(provider_of("localhost"), None);
        assert_eq!(provider_of("co.uk"), None);
        assert_eq!(provider_of(""), None);
    }

    #[test]
    fn the_level_and_the_list_decide() {
        let mut privacy = Privacy::default();
        assert_eq!(privacy.content, ForeignContent::Ask, "ask unless changed");
        assert_eq!(
            privacy.decide("i.ytimg.com"),
            Decision::Ask(Provider { key: "youtube".into(), name: "YouTube".into() })
        );
        assert_eq!(privacy.decide("localhost"), Decision::Skip, "no provider, nothing to ask about");
        privacy.providers.insert("youtube".into(), ProviderChoice::Allowed);
        privacy.providers.insert("example.org".into(), ProviderChoice::Blocked);
        assert_eq!(privacy.decide("youtu.be"), Decision::Load);
        assert_eq!(privacy.decide("cdn.example.org"), Decision::Skip);
        // Never stops everything, also what the list allows.
        privacy.content = ForeignContent::Never;
        assert_eq!(privacy.decide("youtu.be"), Decision::Skip);
        assert_eq!(privacy.decide("other.net"), Decision::Skip);
        // Always loads everything, also what the list blocks.
        privacy.content = ForeignContent::Always;
        assert_eq!(privacy.decide("cdn.example.org"), Decision::Load);
        assert_eq!(privacy.decide("other.net"), Decision::Load);
    }

    #[test]
    fn changes_are_checked_and_applied() {
        let allow = ProviderChange::from_value(&serde_json::json!({"provider": "youtube", "choice": "allowed"}))
            .expect("a change");
        let forget =
            ProviderChange::from_value(&serde_json::json!({"provider": "youtube", "choice": null})).expect("a removal");
        let mut privacy = Privacy::default();
        privacy.change_provider(allow);
        assert_eq!(privacy.providers.get("youtube"), Some(&ProviderChoice::Allowed));
        privacy.change_provider(forget);
        assert!(privacy.providers.is_empty());
        for bad in [
            serde_json::json!({"provider": "YouTube", "choice": "allowed"}),
            serde_json::json!({"provider": "", "choice": "allowed"}),
            serde_json::json!({"provider": "a b", "choice": "blocked"}),
            serde_json::json!({"provider": "youtube", "choice": "maybe"}),
            serde_json::json!({"provider": "youtube"}),
            serde_json::json!("youtube"),
        ] {
            assert!(ProviderChange::from_value(&bad).is_err(), "{bad}");
        }
        privacy.providers = (0..=MAX_PROVIDERS).map(|n| (format!("p{n}.example"), ProviderChoice::Blocked)).collect();
        assert!(privacy.check_providers().is_err(), "one more than the most");
    }

    #[test]
    fn a_mail_loads_what_the_level_and_the_list_allow() {
        let hosts: BTreeSet<String> =
            ["i.ytimg.com", "img.mailchimp.com", "track.mailchimp.com", "cdn.example.org", "localhost"]
                .into_iter()
                .map(String::from)
                .collect();
        assert_eq!(external_content(&Privacy::default(), &BTreeSet::new()), None, "nothing to load");
        let mut privacy = Privacy::default();
        privacy.providers.insert("youtube".into(), ProviderChoice::Allowed);
        privacy.providers.insert("example.org".into(), ProviderChoice::Blocked);
        let asked = external_content(&privacy, &hosts).expect("hosts");
        assert!(!asked.all);
        assert_eq!(asked.load, BTreeSet::from(["i.ytimg.com".to_string()]));
        assert_eq!(asked.ask.len(), 1, "the blocked one and the host without provider are in neither");
        assert_eq!((asked.ask[0].key.as_str(), asked.ask[0].name.as_str()), ("mailchimp.com", "mailchimp.com"));
        assert_eq!(
            asked.ask[0].hosts,
            BTreeSet::from(["img.mailchimp.com".to_string(), "track.mailchimp.com".to_string()])
        );
        privacy.content = ForeignContent::Always;
        let always = external_content(&privacy, &hosts).expect("hosts");
        assert!(always.all && always.ask.is_empty());
        privacy.content = ForeignContent::Never;
        let never = external_content(&privacy, &hosts).expect("hosts");
        assert!(!never.all && never.load.is_empty() && never.ask.is_empty());
    }

    #[test]
    fn the_setting_is_read_leniently() {
        #[derive(Deserialize)]
        struct File {
            #[serde(deserialize_with = "lenient")]
            privacy: Privacy,
        }
        let read = |json: serde_json::Value| serde_json::from_value::<File>(json).expect("reads").privacy;
        let privacy = read(serde_json::json!({"privacy": {
            "content": "never",
            "providers": {"youtube": "allowed", "Bad Key": "allowed", "example.org": "perhaps", "mailchimp.com": "blocked"}
        }}));
        assert_eq!(privacy.content, ForeignContent::Never);
        assert_eq!(
            privacy.providers,
            BTreeMap::from([
                ("mailchimp.com".into(), ProviderChoice::Blocked),
                ("youtube".into(), ProviderChoice::Allowed)
            ])
        );
        assert_eq!(read(serde_json::json!({"privacy": {"content": 7}})), Privacy::default());
        assert_eq!(read(serde_json::json!({"privacy": "never"})), Privacy::default());
        assert_eq!(name_of("youtube"), "YouTube");
        assert_eq!(name_of("example.org"), "example.org");
    }
}
