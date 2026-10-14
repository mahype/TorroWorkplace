//! An ntfy server, its topics and the login for them: checked once here, so the requests in
//! [`crate::subscribe`] are built from nothing but checked parts.

use std::fmt;

use reqwest::Url;
use torro_core::account::Secret;

use crate::NtfyError;

/// The server an empty address means: the public one.
pub const DEFAULT_SERVER: &str = "https://ntfy.sh";

/// The longest topic name ntfy allows.
const MAX_TOPIC: usize = 64;

/// The base address of an ntfy server, made uniform: scheme and host in lower case, a port only
/// when it is not the scheme's own, no path, no trailing `/` (`https://ntfy.sh`,
/// `http://localhost:8080`, `https://[::1]:2586`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Server {
    base: String,
    /// Where the host starts in `base`, after `://`.
    host_at: usize,
}

impl Server {
    /// The server from what a person typed: empty (or only white space) is [`DEFAULT_SERVER`];
    /// without a scheme it is `https://`; `http://` and `https://` are taken as given. A trailing
    /// `/` is dropped. Anything else after the host (a path, a query, a fragment), a login in the
    /// address, white space inside it or a missing host is [`NtfyError::InvalidServer`]: a pasted
    /// `https://ntfy.sh/mytopic` is refused rather than read as a server.
    pub fn parse(input: &str) -> Result<Server, NtfyError> {
        let input = input.trim();
        if input.is_empty() {
            return Server::parse(DEFAULT_SERVER);
        }
        let (scheme, rest) = match input.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("https".to_string(), input),
        };
        if scheme != "http" && scheme != "https" {
            return Err(NtfyError::InvalidServer);
        }
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        let refused = |c: char| matches!(c, '/' | '?' | '#' | '@' | '\\') || c.is_whitespace() || c.is_control();
        if authority.is_empty() || authority.contains(refused) {
            return Err(NtfyError::InvalidServer);
        }
        let url = Url::parse(&format!("{scheme}://{authority}")).map_err(|_| NtfyError::InvalidServer)?;
        let host = url.host_str().filter(|host| !host.is_empty()).ok_or(NtfyError::InvalidServer)?;
        let base = match url.port() {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };
        Ok(Server { host_at: scheme.len() + 3, base })
    }

    /// The base address, as [`Server::parse`] made it uniform.
    pub fn as_str(&self) -> &str {
        &self.base
    }

    /// Host and port without the scheme (`ntfy.sh`, `localhost:8080`, `[::1]:2586`): the part of
    /// the account and source key `ntfy:<host>`.
    pub fn host(&self) -> &str {
        &self.base[self.host_at..]
    }

    /// Whether a login may be sent to this server: over `https`, or to this computer
    /// (`localhost`, `127.0.0.0/8`, `::1`), the rule API keys follow too
    /// ([`torro_core::ai::key_may_travel`]).
    pub fn may_send_login(&self) -> bool {
        torro_core::ai::key_may_travel(&self.base)
    }
}

impl fmt::Display for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base)
    }
}

/// The name of a topic: 1–64 of `A–Z a–z 0–9 _ -`, the characters ntfy allows, so it can stand in
/// an address as it is.
///
/// On a public server whoever knows a topic name can read and write it, so it is handled like a
/// password: `Debug` shows `Topic(…)`, there is no `Display`, and only [`Topic::as_str`] gives the
/// name, for the address of a request and for the account the person keeps it in.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Topic(String);

impl Topic {
    /// `name` as a topic, [`NtfyError::InvalidTopic`] when it is empty, longer than 64 or holds
    /// any other character (no trimming: a name with a space is a wrong name).
    pub fn parse(name: &str) -> Result<Topic, NtfyError> {
        let fits = !name.is_empty()
            && name.len() <= MAX_TOPIC
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if fits { Ok(Topic(name.to_string())) } else { Err(NtfyError::InvalidTopic) }
    }

    /// The name itself. Never log it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Topic(…)")
    }
}

/// How to sign in to an ntfy server. `Debug` shows the user name, never the secret ([`Secret`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// An access token (`tk_…`), sent as `Authorization: Bearer`.
    Token(Secret),
    /// A user name and password, sent as `Authorization: Basic`.
    Password { user: String, password: Secret },
}

impl Login {
    /// The login an account keeps (`NtfyConfig.user` and the secret from the keychain): without a
    /// user name (or a blank one) the secret is a [`Login::Token`], with one a
    /// [`Login::Password`].
    pub fn new(user: Option<&str>, secret: Secret) -> Login {
        match user.map(str::trim).filter(|user| !user.is_empty()) {
            Some(user) => Login::Password { user: user.to_string(), password: secret },
            None => Login::Token(secret),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_made_uniform() {
        let base = |input: &str| Server::parse(input).expect(input).as_str().to_string();
        assert_eq!(base(""), "https://ntfy.sh");
        assert_eq!(base("   "), "https://ntfy.sh");
        assert_eq!(base("ntfy.sh"), "https://ntfy.sh");
        assert_eq!(base("NTFY.example.org/"), "https://ntfy.example.org");
        assert_eq!(base("HTTPS://ntfy.sh:443"), "https://ntfy.sh");
        assert_eq!(base("http://localhost:8080/"), "http://localhost:8080");
        assert_eq!(base("localhost:8080"), "https://localhost:8080", "without a scheme always https");
        assert_eq!(base("http://[::1]:2586"), "http://[::1]:2586");
        assert_eq!(base(" https://push.example.org:8443 "), "https://push.example.org:8443");
    }

    #[test]
    fn wrong_addresses_are_refused() {
        for wrong in [
            "ftp://ntfy.sh",
            "https://",
            "https:///",
            "https://ntfy.sh/mytopic",
            "https://ntfy.sh/a/",
            "https://ntfy.sh?x=1",
            "https://ntfy.sh#x",
            "https://anna:pw@ntfy.sh",
            "anna@ntfy.sh",
            "https://ntfy .sh",
            "https://ntfy.sh:99999",
            "javascript:alert(1)",
        ] {
            assert_eq!(Server::parse(wrong), Err(NtfyError::InvalidServer), "{wrong:?}");
        }
    }

    #[test]
    fn the_host_is_the_address_without_its_scheme() {
        let host = |input: &str| Server::parse(input).expect(input).host().to_string();
        assert_eq!(host(""), "ntfy.sh");
        assert_eq!(host("http://localhost:8080"), "localhost:8080");
        assert_eq!(host("https://[::1]:2586"), "[::1]:2586");
    }

    #[test]
    fn a_login_travels_over_tls_or_to_this_computer_only() {
        let may = |input: &str| Server::parse(input).expect(input).may_send_login();
        assert!(may("ntfy.sh"));
        assert!(may("https://push.example.org"));
        assert!(may("http://localhost:8080"));
        assert!(may("http://127.0.0.1:2586"));
        assert!(may("http://[::1]"));
        assert!(!may("http://push.example.org"));
        assert!(!may("http://192.168.1.20:8080"));
        assert!(!may("http://localhost.example.org"));
    }

    #[test]
    fn topics_are_checked_strictly_and_never_shown() {
        let longest = "a".repeat(64);
        for good in ["backups", "my_topic-1", "A", longest.as_str()] {
            assert_eq!(Topic::parse(good).expect(good).as_str(), good);
        }
        let too_long = "a".repeat(65);
        for bad in ["", " backups", "back ups", "a/b", "a,b", "a?b", "ä", too_long.as_str(), "a.b"] {
            assert_eq!(Topic::parse(bad), Err(NtfyError::InvalidTopic), "{bad:?}");
        }
        let topic = Topic::parse("secret_topic").expect("topic");
        assert_eq!(format!("{topic:?}"), "Topic(…)");
    }

    #[test]
    fn logins_are_tokens_or_passwords_and_never_shown() {
        let token = Login::new(None, Secret::new("tk_SECRET"));
        assert_eq!(token, Login::Token(Secret::new("tk_SECRET")));
        assert_eq!(Login::new(Some("  "), Secret::new("tk_SECRET")), token);
        let password = Login::new(Some(" anna "), Secret::new("PASSWORD"));
        assert_eq!(password, Login::Password { user: "anna".into(), password: Secret::new("PASSWORD") });
        for login in [token, password] {
            let shown = format!("{login:?}");
            assert!(!shown.contains("SECRET") && !shown.contains("PASSWORD"), "{shown}");
        }
    }
}
