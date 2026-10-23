//! TLS for the servers of a mail account (task M11.2): which certificates a connection trusts,
//! and trusting one certificate a person confirmed.
//!
//! A home server or a NAS often shows a certificate it signed itself. The person sees its
//! [`Fingerprint`] once (SHA-256 of the certificate, as the core asks them), confirms it, and the
//! account remembers exactly that fingerprint (`MailAccountConfig::trusted_certificates`). From
//! then on [`Trust::Pinned`] takes that one certificate and nothing else.
//!
//! **Security.** A pin counts only when the SHA-256 of the DER of the server's certificate is
//! equal; then name and dates are not checked (a self-made certificate often names another host
//! or has run out), everything else the system decides as before. The signatures of the handshake
//! are always checked with the aws-lc-rs provider, so a server that shows a pinned certificate
//! without holding its private key is refused. [`client_config`] reports what it saw ([`Seen`]),
//! so a refused certificate can be shown to the person and only pins that were used are kept.

use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

/// The SHA-256 of a server certificate's DER: what a person confirms and an account remembers.
///
/// Written ([`fmt::Display`]) exactly as `torro_mail::account::certificate_fingerprint` writes it:
/// 32 bytes in capital hex digits, joined by `:` (`AB:CD:…:EF`, 95 characters). Read
/// ([`FromStr`]) in the spellings that function takes: small or capital letters, with a `:`
/// between every pair or none at all, white space around.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// The fingerprint of the certificate `der` (the end-entity certificate as the server sent it).
    pub fn of(der: &[u8]) -> Fingerprint {
        Fingerprint(Sha256::digest(der).into())
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = InvalidFingerprint;

    fn from_str(text: &str) -> Result<Fingerprint, InvalidFingerprint> {
        // One reader for both places that take a fingerprint from a person: the core's check of
        // `trusted_certificates` and this one cannot disagree about what is valid.
        let written = torro_mail::account::certificate_fingerprint(text).ok_or(InvalidFingerprint)?;
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(written.split(':')) {
            *byte = u8::from_str_radix(pair, 16).map_err(|_| InvalidFingerprint)?;
        }
        Ok(Fingerprint(bytes))
    }
}

/// A text that is not a fingerprint in the format of [`Fingerprint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFingerprint;

impl fmt::Display for InvalidFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a SHA-256 certificate fingerprint (AB:CD:…, 32 pairs of hex digits)")
    }
}

impl std::error::Error for InvalidFingerprint {}

/// Which certificates a connection trusts.
#[derive(Debug, Clone, Default)]
pub enum Trust {
    /// The operating system's trust store (`rustls-platform-verifier`); the webpki roots where
    /// that cannot be set up. What the core uses for an account without trusted certificates.
    #[default]
    System,
    /// Only these root certificates (DER). For tests against a local server with a certificate
    /// of their own; the core never passes this. An empty list trusts no certificate.
    Only(Vec<CertificateDer<'static>>),
    /// `base`, and besides it exactly the certificates whose [`Fingerprint`] is one of `pins`,
    /// whatever host they name and whenever they run out. Made with [`Trust::with_pins`].
    Pinned {
        /// What decides about every certificate that is not pinned.
        base: Box<Trust>,
        /// The certificates a person confirmed.
        pins: Vec<Fingerprint>,
    },
}

impl Trust {
    /// This trust plus the certificates `pins`. Without pins it is returned unchanged; pins of an
    /// already pinned trust are kept, each fingerprint once.
    pub fn with_pins(self, pins: Vec<Fingerprint>) -> Trust {
        if pins.is_empty() {
            return self;
        }
        let (base, mut all) = match self {
            Trust::Pinned { base, pins } => (base, pins),
            other => (Box::new(other), Vec::new()),
        };
        for pin in pins {
            if !all.contains(&pin) {
                all.push(pin);
            }
        }
        Trust::Pinned { base, pins: all }
    }

    /// The trust that decides about certificates that are not pinned, and all pins.
    fn split(&self) -> (&Trust, Vec<Fingerprint>) {
        match self {
            Trust::Pinned { base, pins } => {
                let (base, mut inner) = base.split();
                for pin in pins {
                    if !inner.contains(pin) {
                        inner.push(*pin);
                    }
                }
                (base, inner)
            }
            other => (other, Vec::new()),
        }
    }
}

/// What the certificate check of a [`client_config`] saw. Clones share what they see, so the
/// caller keeps one and reads it after connecting with the configuration.
///
/// The configuration may serve many connections (an HTTP client): each field holds what the last
/// handshake that got that far reported.
#[derive(Debug, Clone, Default)]
pub struct Seen(Arc<Mutex<Sightings>>);

#[derive(Debug, Default)]
struct Sightings {
    rejected: Option<Fingerprint>,
    accepted: Option<Fingerprint>,
}

impl Seen {
    /// The certificate the check refused: neither pinned nor trusted by the base. Read it after a
    /// failed handshake; `Some` means the server showed a certificate a person could confirm,
    /// `None` that the handshake failed for another reason (or did not get that far).
    pub fn rejected(&self) -> Option<Fingerprint> {
        self.lock().rejected
    }

    /// The pin the server's certificate matched. Read it after a successful handshake: `None`
    /// means the base trusted the certificate and no pin was needed. (A pinned certificate is
    /// noted before the handshake's signature is checked, so after a failed handshake this says
    /// nothing.)
    pub fn accepted(&self) -> Option<Fingerprint> {
        self.lock().accepted
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Sightings> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The TLS client settings for `trust`, always with the aws-lc-rs provider and rustls' safe
/// default protocol versions, and the [`Seen`] its certificate check reports to.
///
/// The value is meant to be wrapped in an `Arc` for `tokio-rustls` or handed to reqwest's
/// `use_preconfigured_tls` as it is. No ALPN is set: whoever uses it for HTTP sets
/// `alpn_protocols` (`h2`, `http/1.1`). Errors when the provider or a root certificate of
/// [`Trust::Only`] cannot be used.
pub fn client_config(trust: &Trust) -> Result<(ClientConfig, Seen), rustls::Error> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let (base, pins) = trust.split();
    let seen = Seen::default();
    let verifier =
        PinVerifier { base: base_verifier(base, &provider)?, pins, provider: provider.clone(), seen: seen.clone() };
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok((config, seen))
}

/// The check of `trust` (never [`Trust::Pinned`], see [`Trust::split`]); `None` when it trusts
/// no certificate at all (an empty [`Trust::Only`]).
fn base_verifier(
    trust: &Trust,
    provider: &Arc<CryptoProvider>,
) -> Result<Option<Arc<dyn ServerCertVerifier>>, rustls::Error> {
    let roots = match trust {
        Trust::System => match rustls_platform_verifier::Verifier::new(provider.clone()) {
            Ok(verifier) => return Ok(Some(Arc::new(verifier))),
            Err(_) => RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() },
        },
        Trust::Only(certificates) if certificates.is_empty() => return Ok(None),
        Trust::Only(certificates) => {
            let mut roots = RootCertStore::empty();
            for certificate in certificates {
                roots.add(certificate.clone())?;
            }
            roots
        }
        Trust::Pinned { base, .. } => return base_verifier(base, provider),
    };
    let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|error| rustls::Error::General(error.to_string()))?;
    Ok(Some(verifier))
}

/// The certificate check of [`client_config`]: a pinned certificate passes, every other one goes
/// to the base; the handshake's signatures are checked here, with the provider, in both cases.
#[derive(Debug)]
struct PinVerifier {
    base: Option<Arc<dyn ServerCertVerifier>>,
    pins: Vec<Fingerprint>,
    provider: Arc<CryptoProvider>,
    seen: Seen,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = Fingerprint::of(end_entity);
        if self.pins.contains(&fingerprint) {
            self.seen.lock().accepted = Some(fingerprint);
            return Ok(ServerCertVerified::assertion());
        }
        let verdict = match &self.base {
            Some(base) => base.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now),
            None => Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)),
        };
        if verdict.is_err() {
            self.seen.lock().rejected = Some(fingerprint);
        }
        verdict
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::sign::{CertifiedKey, SingleCertAndKey};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    use super::*;

    /// `openssl x509 -fingerprint -sha256` of `tests/fixtures/imap-tls/self-signed.der`.
    const SELF_SIGNED: &str =
        "E4:A8:B0:8A:89:6F:2B:06:7C:E1:92:8D:34:0D:16:14:5A:56:57:E7:9E:7E:4A:62:E1:30:3D:36:3C:21:76:28";

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/imap-tls/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).expect("test certificate")
    }

    fn test_ca() -> Trust {
        Trust::Only(vec![CertificateDer::from(fixture("ca.der"))])
    }

    fn self_signed() -> Fingerprint {
        SELF_SIGNED.parse().expect("fingerprint")
    }

    /// A TLS server for one handshake on 127.0.0.1 showing `certificate`, signing with `key`
    /// (both file names of the fixtures). The key need not belong to the certificate: that is
    /// the server that shows a pinned certificate it does not hold.
    async fn server(certificate: &str, key: &str) -> u16 {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture(key)));
        let signing = provider.key_provider.load_private_key(key).expect("key");
        let certified = CertifiedKey::new(vec![CertificateDer::from(fixture(certificate))], signing);
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(SingleCertAndKey::from(certified)));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            let Ok((tcp, _)) = listener.accept().await else { return };
            if let Ok(mut tls) = TlsAcceptor::from(Arc::new(config)).accept(tcp).await {
                let _ = tls.write_all(b"hi").await;
                let _ = tls.shutdown().await;
            }
        });
        port
    }

    /// A handshake with `trust` against `port`, checked for the name `127.0.0.1`.
    async fn handshake(trust: &Trust, port: u16) -> (Result<(), String>, Seen) {
        let (config, seen) = client_config(trust).expect("config");
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("connected");
        let name = ServerName::try_from("127.0.0.1").expect("name");
        let outcome = match TlsConnector::from(Arc::new(config)).connect(name, tcp).await {
            Ok(mut tls) => {
                let mut greeting = [0; 2];
                tls.read_exact(&mut greeting).await.map(drop).map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        (outcome, seen)
    }

    #[test]
    fn a_fingerprint_is_written_and_read_in_the_format_of_the_account() {
        let fingerprint = Fingerprint::of(&fixture("self-signed.der"));
        assert_eq!(fingerprint.to_string(), SELF_SIGNED, "the SHA-256 of the DER, as openssl names it");
        assert_eq!(fingerprint.to_string().len(), 95);
        assert_eq!(
            torro_mail::account::certificate_fingerprint(&fingerprint.to_string()).as_deref(),
            Some(SELF_SIGNED),
            "the account takes what is written here unchanged"
        );
        for spelling in
            [SELF_SIGNED.to_string(), SELF_SIGNED.to_lowercase(), format!(" {} ", SELF_SIGNED.replace(':', ""))]
        {
            assert_eq!(spelling.parse::<Fingerprint>(), Ok(fingerprint), "{spelling:?}");
        }
        for wrong in ["", "E4:A8", &SELF_SIGNED[3..], &format!("{SELF_SIGNED}:00"), &SELF_SIGNED.replacen(':', "", 1)] {
            assert_eq!(wrong.parse::<Fingerprint>(), Err(InvalidFingerprint), "{wrong:?}");
        }
        assert_eq!(format!("{fingerprint:?}"), format!("Fingerprint({SELF_SIGNED})"));
    }

    #[test]
    fn pins_are_added_once_and_no_pins_change_nothing() {
        assert!(matches!(Trust::System.with_pins(Vec::new()), Trust::System));
        let other = Fingerprint([7; 32]);
        let trust = Trust::System.with_pins(vec![self_signed()]).with_pins(vec![self_signed(), other]);
        let Trust::Pinned { base, pins } = &trust else { panic!("pinned: {trust:?}") };
        assert!(matches!(**base, Trust::System));
        assert_eq!(pins, &[self_signed(), other]);
    }

    #[tokio::test]
    async fn a_certificate_the_base_does_not_trust_is_refused_and_seen() {
        let port = server("self-signed.der", "self-signed.key.der").await;
        let (outcome, seen) = handshake(&test_ca(), port).await;
        assert!(outcome.is_err(), "refused: {outcome:?}");
        assert_eq!((seen.rejected(), seen.accepted()), (Some(self_signed()), None));

        let port = server("self-signed.der", "self-signed.key.der").await;
        let (outcome, seen) = handshake(&test_ca().with_pins(vec![Fingerprint([0; 32])]), port).await;
        assert!(outcome.is_err(), "a wrong pin is no pin: {outcome:?}");
        assert_eq!((seen.rejected(), seen.accepted()), (Some(self_signed()), None));

        let port = server("self-signed.der", "self-signed.key.der").await;
        let (outcome, seen) = handshake(&Trust::Only(Vec::new()), port).await;
        assert!(outcome.is_err(), "no roots trust nothing: {outcome:?}");
        assert_eq!(seen.rejected(), Some(self_signed()));
    }

    /// The certificate names `mail.home.arpa`, the connection `127.0.0.1`: a pin takes it anyway.
    #[tokio::test]
    async fn a_pinned_certificate_is_taken_whatever_host_it_names() {
        let port = server("self-signed.der", "self-signed.key.der").await;
        let (outcome, seen) = handshake(&test_ca().with_pins(vec![self_signed()]), port).await;
        assert_eq!(outcome, Ok(()));
        assert_eq!((seen.rejected(), seen.accepted()), (None, Some(self_signed())));
    }

    #[tokio::test]
    async fn a_certificate_the_base_trusts_needs_no_pin() {
        let port = server("server.der", "server.key.der").await;
        let (outcome, seen) = handshake(&test_ca().with_pins(vec![self_signed()]), port).await;
        assert_eq!(outcome, Ok(()));
        assert_eq!((seen.rejected(), seen.accepted()), (None, None), "no pin was used");
    }

    /// The fingerprint of a certificate is public: anyone can show it. Without the private key the
    /// handshake's signature does not check out, so the pin does not help.
    #[tokio::test]
    async fn a_server_showing_a_pinned_certificate_without_its_key_is_refused() {
        let port = server("self-signed.der", "server.key.der").await;
        let (outcome, _) = handshake(&test_ca().with_pins(vec![self_signed()]), port).await;
        assert!(outcome.is_err(), "the signature is checked: {outcome:?}");
    }
}
