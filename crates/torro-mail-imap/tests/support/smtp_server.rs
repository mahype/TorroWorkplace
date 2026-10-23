//! A scripted SMTP server on 127.0.0.1 for the tests of `smtp.rs` (after TorroMail's
//! `tests/support/smtp.rs`): real SMTP lines, no delivery, nothing leaves the computer. It takes
//! any number of connections one after another, answers by its [`Mode`] and keeps what it was
//! sent, so a test can check the envelope, the data and how often it was asked.
//!
//! [`Server::start`] speaks plain text; [`Server::start_tls`] (task M11.2) TLS from the first
//! byte or STARTTLS, with a certificate of `tests/fixtures/imap-tls/` ([`Shows`]).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use torro_mail::Security;

/// How the server answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Takes everything.
    Accept,
    /// Refuses the login with 535.
    RejectLogin,
    /// Refuses every recipient with 550.
    RejectRecipient,
    /// Takes the data and refuses it with 554.
    RejectData,
    /// Takes the whole data and closes the connection without an answer.
    DropAfterData,
}

/// Which certificate a TLS server shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shows {
    /// `server.der`: for `127.0.0.1`, signed by the test CA (`ca.der`).
    TestCa,
    /// `self-signed.der`: signed by itself, for `mail.home.arpa`.
    SelfSigned,
}

/// What the server was sent, over all connections.
#[derive(Debug, Clone, Default)]
pub struct Received {
    /// The `MAIL FROM` lines.
    pub mail_from: Vec<String>,
    /// The `RCPT TO` lines.
    pub rcpt_to: Vec<String>,
    /// The data of the last message, dot-stuffing undone, with its CRLFs.
    pub data: Vec<u8>,
    /// An `AUTH` command came.
    pub auth_sent: bool,
    /// A `QUIT` came.
    pub quit: bool,
    /// A `STARTTLS` came.
    pub starttls: bool,
}

/// A running server; it stops with the test's runtime.
pub struct Server {
    pub port: u16,
    connections: Arc<AtomicUsize>,
    received: Arc<Mutex<Received>>,
}

impl Server {
    /// A server without TLS.
    pub async fn start(mode: Mode) -> Server {
        Server::spawn(mode, None).await
    }

    /// A server that speaks TLS as `security` says (from the first byte, or after `STARTTLS`,
    /// which it then offers) and shows the certificate `shows`. A handshake the client breaks off
    /// ends that connection; the next one is taken.
    pub async fn start_tls(mode: Mode, security: Security, shows: Shows) -> Server {
        Server::spawn(mode, Some((security, acceptor(shows)))).await
    }

    async fn spawn(mode: Mode, tls: Option<(Security, TlsAcceptor)>) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind the test server");
        let port = listener.local_addr().expect("its address").port();
        let connections = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Received::default()));
        let (count, record) = (connections.clone(), received.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                // A broken connection ends only that conversation.
                let _ = converse(socket, mode, tls.clone(), &record).await;
            }
        });
        Server { port, connections, received }
    }

    /// How many connections came.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn received(&self) -> Received {
        self.received.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/imap-tls/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).expect("test certificate")
}

/// The test CA's certificate (DER), for the client to trust.
pub fn test_ca() -> Vec<u8> {
    fixture("ca.der")
}

fn acceptor(shows: Shows) -> TlsAcceptor {
    let (certificate, key) = match shows {
        Shows::TestCa => ("server.der", "server.key.der"),
        Shows::SelfSigned => ("self-signed.der", "self-signed.key.der"),
    };
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture(key)));
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(fixture(certificate))], key)
        .expect("server certificate");
    TlsAcceptor::from(Arc::new(config))
}

async fn converse(
    socket: TcpStream,
    mode: Mode,
    tls: Option<(Security, TlsAcceptor)>,
    record: &Mutex<Received>,
) -> std::io::Result<()> {
    let note = |change: &dyn Fn(&mut Received)| change(&mut record.lock().unwrap_or_else(PoisonError::into_inner));
    let (stream, mut starttls): (Box<dyn Io>, _) = match tls {
        Some((Security::Tls, acceptor)) => (Box::new(acceptor.accept(socket).await?), None),
        Some((Security::StartTls, acceptor)) => (Box::new(socket), Some(acceptor)),
        None => (Box::new(socket), None),
    };
    let mut reader = BufReader::new(stream);
    reader.write_all(b"220 test.invalid ready\r\n").await?;
    reader.flush().await?;
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).await? == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&line).trim_end().to_owned();
        let command = text.to_ascii_uppercase();
        let reply: &[u8] = if command.starts_with("EHLO") && starttls.is_some() {
            b"250-test.invalid\r\n250-STARTTLS\r\n250 AUTH PLAIN\r\n"
        } else if command.starts_with("EHLO") {
            b"250-test.invalid\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 AUTH PLAIN\r\n"
        } else if command == "STARTTLS"
            && let Some(acceptor) = starttls.take()
        {
            note(&|received| received.starttls = true);
            reader.write_all(b"220 2.0.0 go ahead\r\n").await?;
            reader.flush().await?;
            let secured: Box<dyn Io> = Box::new(acceptor.accept(reader.into_inner()).await?);
            reader = BufReader::new(secured);
            continue;
        } else if command.starts_with("AUTH") {
            note(&|received| received.auth_sent = true);
            if mode == Mode::RejectLogin { b"535 5.7.8 authentication failed\r\n" } else { b"235 2.7.0 ok\r\n" }
        } else if command.starts_with("MAIL FROM") {
            note(&|received| received.mail_from.push(text.clone()));
            b"250 2.1.0 ok\r\n"
        } else if command.starts_with("RCPT TO") {
            note(&|received| received.rcpt_to.push(text.clone()));
            if mode == Mode::RejectRecipient { b"550 5.1.1 no such user\r\n" } else { b"250 2.1.5 ok\r\n" }
        } else if command == "DATA" {
            reader.write_all(b"354 go ahead\r\n").await?;
            reader.flush().await?;
            let data = read_data(&mut reader).await?;
            note(&|received| received.data = data.clone());
            match mode {
                Mode::DropAfterData => return Ok(()),
                Mode::RejectData => b"554 5.6.0 message refused\r\n",
                _ => b"250 2.0.0 queued\r\n",
            }
        } else if command == "QUIT" {
            note(&|received| received.quit = true);
            reader.write_all(b"221 bye\r\n").await?;
            reader.flush().await?;
            return Ok(());
        } else {
            b"250 ok\r\n"
        };
        reader.write_all(reply).await?;
        reader.flush().await?;
    }
}

/// The lines up to the lone dot, with the leading dot of stuffed lines removed.
async fn read_data(reader: &mut BufReader<Box<dyn Io>>) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).await? == 0 || line == b".\r\n" {
            return Ok(data);
        }
        data.extend_from_slice(line.strip_prefix(b".").unwrap_or(&line));
    }
}
