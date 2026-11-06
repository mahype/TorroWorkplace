//! Whether a mail comes from its sender's domain (issue #453, decision 1 of the plan): the core checks
//! the DKIM signatures of the message itself ([`SenderCheck::verified_domain`]), so a sender's logo
//! ([`super::logos`]) shows only for mail that domain signed. The `Authentication-Results` header of
//! one's own mail server is not read: a sender can write it into the mail, and not every server
//! adds one.
//!
//! A signature counts when it passes ([`DkimResult::Pass`]), signs the `From` header (RFC 6376
//! requires it; else the sender could be rewritten under the signature), signs the whole body (`l=`
//! would let anyone append text below a signed newsletter), is not `rsa-sha1` (RFC 8301) and its
//! domain `d=` is the sender's domain or a domain above it (`mail.paypal.de` signed by `paypal.de`):
//! that signing domain stands for the sender. A signer *below* the sender's domain does not count:
//! without the public suffix list, a subdomain handed out to others (`x.duckdns.org`) could sign for
//! the domain above it. Only a DNS failure ([`DkimResult::TempError`]) leaves the question open
//! ([`Checked::Retry`]); every other outcome is an answer.
//!
//! The keys (`<selector>._domainkey.<domain>` TXT records) are kept in a cache of their own,
//! [`TXT_CACHE_MAX`] entries for at most [`TXT_KEEP`]: a sync of a full inbox asks for the key of
//! one newsletter only once. A signing domain whose DNS did not answer is not asked again for
//! [`UNANSWERED_REST`], so a sync with many of its mails does not wait for each. Nothing here logs a
//! domain, an address or a header.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use mail_auth::{AuthenticatedMessage, DkimResult, MessageAuthenticator, Parameters, ResolverCache, Txt};
use torro_pim_caldav::discover::checked_domain;

/// At most this many TXT records are kept ([`TxtCache`]); a full cache drops what expired, and
/// everything when that is not enough.
pub const TXT_CACHE_MAX: usize = 1000;

/// A TXT record is kept at most this long, whatever its TTL says.
pub const TXT_KEEP: Duration = Duration::from_secs(60 * 60);

/// How long checking one message may take: the DNS lookups of its signatures.
const CHECK_DEADLINE: Duration = Duration::from_secs(10);

/// A signing domain whose DNS did not answer is not asked again for this long.
pub const UNANSWERED_REST: Duration = Duration::from_secs(5 * 60);

/// What the check of one message found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// A signature of this domain (in lower case, ASCII) passed: the sender's domain, or the domain
    /// above it that signed.
    Domain(String),
    /// No signature passed that belongs to the sender's domain (none at all, broken, foreign, cut
    /// with `l=`), or the message names no sender.
    None,
    /// The DNS could not be asked: check the message again later.
    Retry,
}

/// The core's DKIM check, one for all mail accounts.
pub struct SenderCheck {
    /// Built on first use: reading the system's DNS configuration is file work. `None` when no
    /// resolver could be built: then nothing is checked yet ([`Checked::Retry`]).
    auth: OnceLock<Option<MessageAuthenticator>>,
    txt: TxtCache,
    /// Signing domains whose DNS did not answer, and when ([`UNANSWERED_REST`]).
    unanswered: Mutex<HashMap<String, Instant>>,
}

impl SenderCheck {
    /// The check on the system's DNS configuration. Without one every signed message waits for a
    /// later try ([`Checked::Retry`]).
    pub fn system() -> SenderCheck {
        SenderCheck { auth: OnceLock::new(), txt: TxtCache::default(), unanswered: Mutex::default() }
    }

    /// A check that asks no DNS server: only the keys put into its cache ([`SenderCheck::know`])
    /// are found, every other lookup fails as when offline.
    #[cfg(test)]
    pub fn offline() -> SenderCheck {
        let auth = OnceLock::new();
        use mail_auth::hickory_resolver::config::{ResolverConfig, ResolverOpts};
        let config = ResolverConfig::from_name_servers(Vec::new());
        let _ = auth.set(MessageAuthenticator::new(config, ResolverOpts::default()).ok());
        SenderCheck { auth, txt: TxtCache::default(), unanswered: Mutex::default() }
    }

    /// Puts the TXT record `record` of `name` (`<selector>._domainkey.<domain>`) into the cache.
    #[cfg(test)]
    pub fn know(&self, name: &str, record: &str) {
        use mail_auth::common::parse::TxtRecordParser;
        use mail_auth::common::verify::DomainKey;
        let value = Txt::from(DomainKey::parse(record.as_bytes()));
        self.txt.insert(fqdn(name), value, Instant::now() + TXT_KEEP);
    }

    /// Makes every lookup of `name` fail as a DNS server that does not answer would.
    #[cfg(test)]
    pub fn unreachable(&self, name: &str) {
        let error = mail_auth::Error::Dns(mail_auth::DnsError::Resolver("no answer".into()));
        self.txt.insert(fqdn(name), Txt::Error(error), Instant::now() + TXT_KEEP);
    }

    fn authenticator(&self) -> Option<&MessageAuthenticator> {
        self.auth.get_or_init(|| MessageAuthenticator::new_system_conf().ok()).as_ref()
    }

    /// Whether mail can be checked at all: there is a DNS configuration. Without one nothing is
    /// fetched only to be checked.
    pub fn ready(&self) -> bool {
        self.authenticator().is_some()
    }

    /// Whether the message `raw` comes from the domain of its sender, after the module docs. The
    /// sender is read from the message itself and must be exactly one address: a second `From`
    /// header (or address) is how a signed mail is made to show another sender.
    pub async fn verified_domain(&self, raw: &[u8]) -> Checked {
        let Some(message) = AuthenticatedMessage::parse(raw) else { return Checked::None };
        let [from] = message.from.as_slice() else { return Checked::None };
        let Some(sender) = sender_domain(from) else { return Checked::None };
        if message.dkim_headers.is_empty() {
            return Checked::None;
        }
        let Some(authenticator) = self.authenticator() else { return Checked::Retry };
        let signers: Vec<String> =
            message.dkim_headers.iter().map(|header| header.header.d.to_ascii_lowercase()).collect();
        if signers.iter().all(|signer| self.resting(signer)) {
            return Checked::Retry;
        }
        let parameters = Parameters::new(&message).with_txt_cache(&self.txt);
        let checked = tokio::time::timeout(CHECK_DEADLINE, authenticator.verify_dkim(parameters)).await;
        let Ok(outputs) = checked else {
            self.note_unanswered(&signers);
            return Checked::Retry;
        };
        let mut found: Option<String> = None;
        let mut unanswered = false;
        for output in &outputs {
            match (output.result(), output.signature()) {
                (DkimResult::Pass, Some(signature)) if counts(signature) => {
                    let Some(domain) = checked_domain(&signature.d) else { continue };
                    if let Some(standing) = standing_for(&sender, &domain)
                        && found.as_ref().is_none_or(|kept| standing.len() < kept.len())
                    {
                        found = Some(standing);
                    }
                }
                (DkimResult::TempError(_), signature) => {
                    unanswered = true;
                    if let Some(signature) = signature {
                        self.note_unanswered(&[signature.d.to_ascii_lowercase()]);
                    }
                }
                _ => {}
            }
        }
        match (found, unanswered) {
            (Some(domain), _) => Checked::Domain(domain),
            (None, true) => Checked::Retry,
            (None, false) => Checked::None,
        }
    }
}

impl SenderCheck {
    /// Whether the DNS of the signing domain `signer` did not answer within [`UNANSWERED_REST`].
    fn resting(&self, signer: &str) -> bool {
        let unanswered = self.unanswered.lock().unwrap_or_else(PoisonError::into_inner);
        unanswered.get(signer).is_some_and(|at| at.elapsed() < UNANSWERED_REST)
    }

    /// Notes that the DNS of `signers` did not answer; at most [`TXT_CACHE_MAX`] are kept.
    fn note_unanswered(&self, signers: &[String]) {
        let mut unanswered = self.unanswered.lock().unwrap_or_else(PoisonError::into_inner);
        unanswered.retain(|_, at| at.elapsed() < UNANSWERED_REST);
        if unanswered.len() >= TXT_CACHE_MAX {
            unanswered.clear();
        }
        for signer in signers {
            unanswered.insert(signer.clone(), Instant::now());
        }
    }
}

/// Whether a passing signature counts (module docs): it signs `From` and the whole body, and not
/// with `rsa-sha1`.
fn counts(signature: &mail_auth::dkim::Signature) -> bool {
    signature.l == 0
        && signature.a != mail_auth::common::crypto::Algorithm::RsaSha1
        && signature.h.iter().any(|header| header.eq_ignore_ascii_case("from"))
}

/// The domain of the address `address` (after the last `@`), in lower case and ASCII; `None` when
/// it is no domain on the internet.
pub fn sender_domain(address: &str) -> Option<String> {
    let (_, domain) = address.trim().trim_end_matches('>').rsplit_once('@')?;
    checked_domain(domain)
}

/// The domain that stands for the sender `sender` when `signer` signed its mail: the signer, when it
/// is the sender's domain or a domain above it, else `None` (a signer below does not count, module
/// docs).
fn standing_for(sender: &str, signer: &str) -> Option<String> {
    let below = sender.strip_suffix(signer).is_some_and(|rest| rest.ends_with('.'));
    (sender == signer || below).then(|| signer.to_string())
}

/// `name` as the resolver keys it: lower case with a final dot.
#[cfg(test)]
fn fqdn(name: &str) -> Box<str> {
    let mut name = name.trim_end_matches('.').to_ascii_lowercase();
    name.push('.');
    name.into_boxed_str()
}

/// The TXT records the check asked for, by name, each until it expires ([`TXT_KEEP`] at most).
#[derive(Default)]
struct TxtCache {
    records: Mutex<HashMap<Box<str>, (Txt, Instant)>>,
}

impl ResolverCache<Box<str>, Txt> for TxtCache {
    fn get<Q>(&self, name: &Q) -> Option<Txt>
    where
        Box<str>: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        records.get(name).filter(|(_, until)| *until > Instant::now()).map(|(record, _)| record.clone())
    }

    fn remove<Q>(&self, name: &Q) -> Option<Txt>
    where
        Box<str>: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.records.lock().unwrap_or_else(PoisonError::into_inner).remove(name).map(|(record, _)| record)
    }

    fn insert(&self, name: Box<str>, record: Txt, valid_until: Instant) {
        let now = Instant::now();
        let until = valid_until.min(now + TXT_KEEP);
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        if records.len() >= TXT_CACHE_MAX && !records.contains_key(&name) {
            records.retain(|_, (_, until)| *until > now);
            if records.len() >= TXT_CACHE_MAX {
                records.clear();
            }
        }
        records.insert(name, (record, until));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use mail_auth::common::crypto::Ed25519Key;
    use mail_auth::common::headers::HeaderWriter;
    use mail_auth::dkim::DkimSigner;

    use super::*;

    /// The fixed Ed25519 key pair of the tests (the seed and its public key, Base64): a signature made
    /// now verifies with the record [`key_record`] puts into the cache.
    const SEED: &str = "nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A=";
    const PUBLIC: &str = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";

    fn base64(text: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(text).expect("base64")
    }

    pub(crate) fn key_record() -> String {
        format!("v=DKIM1; k=ed25519; p={PUBLIC}")
    }

    /// The mail of the fixture `paypal.eml`, sent by `from`, as it comes from the server (CRLF).
    fn mail(from: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/sender_logos/paypal.eml", env!("CARGO_MANIFEST_DIR"));
        let text = String::from_utf8(std::fs::read(path).expect("fixture")).expect("text").replace('\r', "");
        let text = text.replace("service@paypal.example", from);
        text.replace('\n', "\r\n").into_bytes()
    }

    /// [`mail`] with a DKIM signature of `domain` under the selector `torro`; with `body_length` the
    /// signature covers only that much of the body (`l=`).
    fn signed(from: &str, domain: &str, body_length: bool) -> Vec<u8> {
        sign(&mail(from), domain, body_length)
    }

    /// `mail` (CRLF) with a DKIM signature of `domain` under the selector `torro`, whose key
    /// [`key_record`] is.
    pub(crate) fn sign(mail: &[u8], domain: &str, body_length: bool) -> Vec<u8> {
        sign_headers(mail, domain, body_length, &["From", "To", "Subject"])
    }

    /// [`sign`] over the headers `headers`.
    fn sign_headers(mail: &[u8], domain: &str, body_length: bool, headers: &[&str]) -> Vec<u8> {
        let key = Ed25519Key::from_seed_and_public_key(&base64(SEED), &base64(PUBLIC)).expect("key");
        let signer =
            DkimSigner::from_key(key).domain(domain).selector("torro").headers(headers.iter().map(|h| h.to_string()));
        let signer = if body_length { signer.body_length(true) } else { signer };
        let signature = signer.sign(mail).expect("signed");
        let mut out = signature.to_header().into_bytes();
        out.extend_from_slice(mail);
        out
    }

    const SENDER: &str = "service@paypal.example";

    fn check(domains: &[&str]) -> SenderCheck {
        let check = SenderCheck::offline();
        for domain in domains {
            check.know(&format!("torro._domainkey.{domain}"), &key_record());
        }
        check
    }

    #[tokio::test]
    async fn a_signature_of_the_senders_domain_passes() {
        let raw = signed(SENDER, "paypal.example", false);
        let checked = check(&["paypal.example"]).verified_domain(&raw).await;
        assert_eq!(checked, Checked::Domain("paypal.example".into()));
    }

    #[tokio::test]
    async fn a_domain_above_the_sender_counts_and_one_below_does_not() {
        let raw = signed("news@mail.paypal.example", "paypal.example", false);
        let below = check(&["paypal.example"]).verified_domain(&raw).await;
        assert_eq!(below, Checked::Domain("paypal.example".into()), "signed by the parent");
        let raw = signed("Service@PayPal.example", "PayPal.example", false);
        let same = check(&["paypal.example"]).verified_domain(&raw).await;
        assert_eq!(same, Checked::Domain("paypal.example".into()), "in any case");
        // A subdomain may be someone else's (`x.duckdns.org`): it never signs for the domain above it.
        let raw = signed(SENDER, "e.paypal.example", false);
        let below = check(&["e.paypal.example"]).verified_domain(&raw).await;
        assert_eq!(below, Checked::None, "signed below the sender");
    }

    #[tokio::test]
    async fn a_foreign_signer_does_not_count() {
        let raw = signed(SENDER, "mailer.example", false);
        let checked = check(&["mailer.example"]).verified_domain(&raw).await;
        assert_eq!(checked, Checked::None, "a mailing service signs only for itself");
        let raw = signed(SENDER, "paypal.example.evil.example", false);
        let checked = check(&["paypal.example.evil.example"]).verified_domain(&raw).await;
        assert_eq!(checked, Checked::None, "a name that only starts like the sender's");
    }

    #[tokio::test]
    async fn a_signature_of_part_of_the_body_does_not_count() {
        let raw = signed(SENDER, "paypal.example", true);
        let checked = check(&["paypal.example"]).verified_domain(&raw).await;
        assert_eq!(checked, Checked::None, "with l= anyone may append text");
    }

    #[tokio::test]
    async fn a_changed_body_fails() {
        let mut raw = signed(SENDER, "paypal.example", false);
        let at = raw.len() - 4;
        raw[at] = b'X';
        let checked = check(&["paypal.example"]).verified_domain(&raw).await;
        assert_eq!(checked, Checked::None);
    }

    #[tokio::test]
    async fn an_unanswered_lookup_is_asked_again_later() {
        let raw = signed(SENDER, "paypal.example", false);
        let check = SenderCheck::offline();
        check.unreachable("torro._domainkey.paypal.example");
        assert_eq!(check.verified_domain(&raw).await, Checked::Retry);
        // The key there now, the domain still rests: a sync with many of its mails does not wait for each.
        check.know("torro._domainkey.paypal.example", &key_record());
        assert_eq!(check.verified_domain(&raw).await, Checked::Retry, "resting");
        check.unanswered.lock().expect("lock").clear();
        assert_eq!(check.verified_domain(&raw).await, Checked::Domain("paypal.example".into()), "asked again");
    }

    #[tokio::test]
    async fn a_signature_that_leaves_out_the_sender_does_not_count() {
        let raw = sign_headers(&mail(SENDER), "paypal.example", false, &["To", "Subject"]);
        assert_eq!(check(&["paypal.example"]).verified_domain(&raw).await, Checked::None, "From not signed");
    }

    #[tokio::test]
    async fn mail_without_a_signature_or_one_sender_has_no_domain() {
        let check = check(&["paypal.example"]);
        assert_eq!(check.verified_domain(&mail(SENDER)).await, Checked::None, "not signed");
        let raw = signed("nobody", "paypal.example", false);
        assert_eq!(check.verified_domain(&raw).await, Checked::None, "no domain");
        // A second sender added above a signed mail: the signature still passes, and a mail program
        // may show the added one.
        let mut raw = b"From: Attacker <attacker@evil.example>\r\n".to_vec();
        raw.extend(signed(SENDER, "paypal.example", false));
        assert_eq!(check.verified_domain(&raw).await, Checked::None, "two senders");
        let raw = signed("service@paypal.example, other@evil.example", "paypal.example", false);
        assert_eq!(check.verified_domain(&raw).await, Checked::None, "two addresses");
    }

    /// The service's check reads the system's DNS configuration only when it checks a signed mail.
    #[tokio::test]
    async fn the_system_check_asks_nothing_for_mail_without_a_signature() {
        assert_eq!(SenderCheck::system().verified_domain(&mail(SENDER)).await, Checked::None);
    }

    #[test]
    fn the_cache_keeps_at_most_its_size_and_an_hour() {
        let cache = TxtCache::default();
        let far = Instant::now() + Duration::from_secs(24 * 60 * 60);
        for n in 0..TXT_CACHE_MAX {
            cache.insert(format!("{n}.example.").into(), Txt::Error(mail_auth::Error::ParseError), far);
        }
        let kept = cache.records.lock().expect("lock").get("0.example.").map(|(_, until)| *until).expect("kept");
        assert!(kept <= Instant::now() + TXT_KEEP, "never longer than an hour");
        cache.insert("one-more.example.".into(), Txt::Error(mail_auth::Error::ParseError), far);
        assert!(cache.records.lock().expect("lock").len() <= TXT_CACHE_MAX);
        assert!(cache.get("one-more.example.").is_some());
    }

    #[test]
    fn a_senders_domain_is_the_part_after_the_at() {
        assert_eq!(sender_domain("Anna@Example.ORG"), Some("example.org".into()));
        assert_eq!(sender_domain("anna@localhost"), None);
        assert_eq!(sender_domain("anna"), None);
        assert_eq!(standing_for("mail.paypal.example", "paypal.example"), Some("paypal.example".into()));
        assert_eq!(standing_for("paypal.example", "mail.paypal.example"), None);
        assert_eq!(standing_for("paypal.example", "xpaypal.example"), None);
    }
}
