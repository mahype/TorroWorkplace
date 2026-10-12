//! A scripted IMAP server on 127.0.0.1 for the tests of `imap` and `folders` (after TorroMail's
//! `imap_contract.rs`): it plays a fixed list of steps over TLS (or plain until STARTTLS) and
//! records every line the client sent. No network beyond the loopback, no real server.
//!
//! The certificates in `tests/fixtures/imap-tls/` (a test CA and a server certificate for
//! `127.0.0.1`, P-256, valid 2020–2120) were made once with `openssl`; the client trusts only
//! that CA ([`trust`]).

use std::sync::{Arc, Mutex};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use torro_mail::{Endpoint, Security};
use torro_mail_imap::imap::Trust;

/// One step of the server's script.
#[derive(Debug, Clone)]
pub enum Step {
    /// Send this text and a line end (it may hold more lines, sent in one write).
    Send(String),
    /// Read one line from the client and record it; a different line ends the script.
    Expect(String),
    /// Read a literal of this many bytes and record it as `<n bytes: …>`.
    Literal(usize),
    /// Start TLS on the connection (after answering `STARTTLS`).
    StartTls,
}

/// `Step::Send`.
pub fn send(text: &str) -> Step {
    Step::Send(text.to_owned())
}

/// `Step::Expect`.
pub fn expect(text: &str) -> Step {
    Step::Expect(text.to_owned())
}

/// A running scripted server.
pub struct Server {
    pub endpoint: Endpoint,
    log: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Server {
    /// Waits for the script to end and returns every line the client sent, in order. A line the
    /// script did not expect shows up as `UNEXPECTED <line>` (the script stops there).
    pub async fn received(self) -> Vec<String> {
        self.task.await.expect("the server task ran");
        self.log.lock().expect("log").clone()
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// The client side's trust: only the test CA.
pub fn trust() -> Trust {
    Trust::Only(vec![CertificateDer::from(fixture("ca.der"))])
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/imap-tls/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).expect("test certificate")
}

fn acceptor() -> TlsAcceptor {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture("server.key.der")));
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(fixture("server.der"))], key)
        .expect("server certificate");
    TlsAcceptor::from(Arc::new(config))
}

/// Starts a server for one connection that plays `steps`. With `Security::Tls` the connection
/// is TLS from the first byte; with `StartTls` it is plain until a `Step::StartTls`.
pub async fn serve(security: Security, steps: Vec<Step>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
    let port = listener.local_addr().expect("address").port();
    let log = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&log);
    let task = tokio::spawn(async move {
        let Ok((tcp, _)) = listener.accept().await else { return };
        let acceptor = acceptor();
        let stream: Box<dyn Io> = match security {
            Security::Tls => match acceptor.accept(tcp).await {
                Ok(tls) => Box::new(tls),
                Err(_) => return,
            },
            Security::StartTls => Box::new(tcp),
        };
        play(BufReader::new(stream), acceptor, steps, recorded).await;
    });
    let endpoint = Endpoint { host: "127.0.0.1".into(), port, security };
    Server { endpoint, log, task }
}

async fn play(mut io: BufReader<Box<dyn Io>>, acceptor: TlsAcceptor, steps: Vec<Step>, log: Arc<Mutex<Vec<String>>>) {
    let record = |line: String| log.lock().expect("log").push(line);
    for step in steps {
        match step {
            Step::Send(text) => {
                if io.write_all(format!("{text}\r\n").as_bytes()).await.is_err() || io.flush().await.is_err() {
                    return;
                }
            }
            Step::Expect(wanted) => {
                let mut line = String::new();
                match io.read_line(&mut line).await {
                    Ok(0) | Err(_) => {
                        record(format!("UNEXPECTED end, wanted {wanted}"));
                        return;
                    }
                    Ok(_) => {}
                }
                let line = line.trim_end_matches(['\r', '\n']).to_owned();
                if line != wanted {
                    record(format!("UNEXPECTED {line}"));
                    return;
                }
                record(line);
            }
            Step::Literal(size) => {
                let mut bytes = vec![0; size];
                if io.read_exact(&mut bytes).await.is_err() {
                    record("UNEXPECTED short literal".into());
                    return;
                }
                record(format!("<{size} bytes: {}>", String::from_utf8_lossy(&bytes)));
            }
            Step::StartTls => {
                let inner = io.into_inner();
                let Ok(tls) = acceptor.accept(inner).await else {
                    record("UNEXPECTED no TLS handshake".into());
                    return;
                };
                io = BufReader::new(Box::new(tls));
            }
        }
    }
}
