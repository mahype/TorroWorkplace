//! A scripted SMTP server on 127.0.0.1 for the tests of `smtp.rs` (after TorroMail's
//! `tests/support/smtp.rs`): real SMTP lines, no TLS, no delivery, nothing leaves the computer.
//! It takes any number of connections one after another, answers by its [`Mode`] and keeps what
//! it was sent, so a test can check the envelope, the data and how often it was asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

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
}

/// A running server; it stops with the test's runtime.
pub struct Server {
    pub port: u16,
    connections: Arc<AtomicUsize>,
    received: Arc<Mutex<Received>>,
}

impl Server {
    pub async fn start(mode: Mode) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind the test server");
        let port = listener.local_addr().expect("its address").port();
        let connections = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Received::default()));
        let (count, record) = (connections.clone(), received.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                // A broken connection ends only that conversation.
                let _ = converse(socket, mode, &record).await;
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

async fn converse(socket: TcpStream, mode: Mode, record: &Mutex<Received>) -> std::io::Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);
    let note = |change: &dyn Fn(&mut Received)| change(&mut record.lock().unwrap_or_else(PoisonError::into_inner));
    writer.write_all(b"220 test.invalid ready\r\n").await?;
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).await? == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&line).trim_end().to_owned();
        let command = text.to_ascii_uppercase();
        let reply: &[u8] = if command.starts_with("EHLO") {
            b"250-test.invalid\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n250 AUTH PLAIN\r\n"
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
            writer.write_all(b"354 go ahead\r\n").await?;
            let data = read_data(&mut reader).await?;
            note(&|received| received.data = data.clone());
            match mode {
                Mode::DropAfterData => return Ok(()),
                Mode::RejectData => b"554 5.6.0 message refused\r\n",
                _ => b"250 2.0.0 queued\r\n",
            }
        } else if command == "QUIT" {
            note(&|received| received.quit = true);
            writer.write_all(b"221 bye\r\n").await?;
            return Ok(());
        } else {
            b"250 ok\r\n"
        };
        writer.write_all(reply).await?;
    }
}

/// The lines up to the lone dot, with the leading dot of stuffed lines removed.
async fn read_data(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).await? == 0 || line == b".\r\n" {
            return Ok(data);
        }
        data.extend_from_slice(line.strip_prefix(b".").unwrap_or(&line));
    }
}
