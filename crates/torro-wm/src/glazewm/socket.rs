//! GlazeWM's WebSocket on `ws://127.0.0.1:<port>`.
//!
//! Task X.7 in `docs/tasks/plattformen.md`. GlazeWM takes one text message per frame, written
//! like its command line (`query monitors`, `command --id <uuid> close`, `sub --events …`), and
//! answers each with a `client_response` frame that repeats the message. Requests here use one
//! connection each, like the Hyprland backend: connect, send, read the answer, close. The event
//! subscription keeps its connection open (see [`super::events`]).

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use super::json::{self, ServerMessage};
use crate::WmError;

/// How long a request may take; a GlazeWM that accepts but never answers must not block the daemon.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// An open WebSocket to GlazeWM.
pub type Connection = WebSocketStream<TcpStream>;

/// Sends `message` and returns the `data` of GlazeWM's answer (`Null` for answers without
/// data). A refused message is an error (see [`json::response_data`]); a closed port is
/// `NotRunning`.
pub async fn request(port: u16, message: &str) -> Result<Value, WmError> {
    let mut connection = connect(port).await?;
    let answer = send_and_answer(&mut connection, message).await;
    // Best effort: GlazeWM drops the connection on its own once we are gone.
    let _ = connection.close(None).await;
    answer
}

/// Opens a connection and sends `message` (a `sub --events …` line), returning the connection
/// once GlazeWM accepted the subscription. Event frames follow on the connection.
pub async fn subscribe(port: u16, message: &str) -> Result<Connection, WmError> {
    let mut connection = connect(port).await?;
    send_and_answer(&mut connection, message).await?;
    Ok(connection)
}

/// The text of the next frame; `None` when the connection closed or failed. Pings, pongs and
/// binary frames are skipped (tungstenite answers pings by itself).
pub async fn next_text(connection: &mut Connection) -> Option<String> {
    while let Some(Ok(frame)) = connection.next().await {
        match frame {
            Message::Text(text) => return Some(text.as_str().to_owned()),
            Message::Close(_) => return None,
            _ => {}
        }
    }
    None
}

async fn connect(port: u16) -> Result<Connection, WmError> {
    tokio::time::timeout(TIMEOUT, connect_unbounded(port))
        .await
        .map_err(|_| WmError::Io(format!("GlazeWM did not accept a connection within {} s", TIMEOUT.as_secs())))?
}

async fn connect_unbounded(port: u16) -> Result<Connection, WmError> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|err| match err.kind() {
        std::io::ErrorKind::ConnectionRefused => WmError::NotRunning(format!("no GlazeWM IPC on port {port}")),
        _ => WmError::Io(format!("connect to GlazeWM on port {port}: {err}")),
    })?;
    let (connection, _) = tokio_tungstenite::client_async(format!("ws://127.0.0.1:{port}"), stream)
        .await
        .map_err(|err| WmError::Protocol(format!("WebSocket handshake with GlazeWM: {err}")))?;
    Ok(connection)
}

async fn send_and_answer(connection: &mut Connection, message: &str) -> Result<Value, WmError> {
    tokio::time::timeout(TIMEOUT, send_and_answer_unbounded(connection, message))
        .await
        .map_err(|_| WmError::Io(format!("GlazeWM did not answer {message:?} within {} s", TIMEOUT.as_secs())))?
}

async fn send_and_answer_unbounded(connection: &mut Connection, message: &str) -> Result<Value, WmError> {
    connection.send(Message::text(message)).await.map_err(|err| WmError::Io(format!("send to GlazeWM: {err}")))?;
    loop {
        let Some(text) = next_text(connection).await else {
            return Err(WmError::Io(format!("GlazeWM closed the connection before answering {message:?}")));
        };
        // Only the answer to our own message counts; anything else on this connection (there
        // should be nothing) is skipped.
        if let ServerMessage::ClientResponse { client_message, data, error, success } = json::parse_message(&text)?
            && client_message == message
        {
            return json::response_data(data, error, success);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use futures_util::{SinkExt, StreamExt};
    use serde_json::Value;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    use super::*;

    /// A stand-in for GlazeWM's IPC server on a free local port. For each connection it reads
    /// the first text frame, asks `answer` for the frames to send back and then closes. Every
    /// received message is recorded in order.
    pub(crate) struct FakeGlazeWm {
        pub(crate) port: u16,
        received: Arc<Mutex<Vec<String>>>,
    }

    impl FakeGlazeWm {
        pub(crate) async fn start<F>(answer: F) -> FakeGlazeWm
        where
            F: Fn(&str) -> Vec<String> + Send + Sync + 'static,
        {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind fake GlazeWM");
            let port = listener.local_addr().expect("local address").port();
            let received = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&received);
            let answer = Arc::new(answer);
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let log = Arc::clone(&log);
                    let answer = Arc::clone(&answer);
                    tokio::spawn(async move {
                        let mut ws = tokio_tungstenite::accept_async(stream).await.expect("handshake");
                        let Some(Ok(Message::Text(text))) = ws.next().await else {
                            return;
                        };
                        log.lock().unwrap_or_else(PoisonError::into_inner).push(text.as_str().to_owned());
                        for frame in answer(text.as_str()) {
                            ws.send(Message::text(frame)).await.expect("send frame");
                        }
                        let _ = ws.close(None).await;
                    });
                }
            });
            FakeGlazeWm { port, received }
        }

        pub(crate) fn received(&self) -> Vec<String> {
            self.received.lock().unwrap_or_else(PoisonError::into_inner).clone()
        }
    }

    /// `fixture` (one client response) with `clientMessage` set to `message`, as GlazeWM echoes it.
    pub(crate) fn echo(fixture: &str, message: &str) -> String {
        let mut value: Value = serde_json::from_str(fixture).expect("fixture is JSON");
        value["clientMessage"] = Value::String(message.to_owned());
        value.to_string()
    }

    const MONITORS: &str = include_str!("../../tests/fixtures/glazewm-monitors.json");

    #[tokio::test]
    async fn request_returns_data_of_matching_answer() {
        // An unrelated answer first: only the one for our message counts.
        let fake =
            FakeGlazeWm::start(|message| vec![echo(MONITORS, "query something else"), echo(MONITORS, message)]).await;
        let data = request(fake.port, "query monitors").await.expect("answer");
        assert!(data["monitors"].is_array());
        assert_eq!(fake.received(), vec!["query monitors".to_owned()]);
    }

    #[tokio::test]
    async fn closed_port_is_not_running() {
        // Port 1 (tcpmux) is closed on any machine that runs the tests. A port bound and freed here
        // could go to a fake server of a test beside this one, which would answer (issue #318); a
        // port below 1024 is never handed out by `bind(0)`.
        let err = request(1, "query monitors").await.expect_err("nothing listens");
        assert!(matches!(err, WmError::NotRunning(_)), "{err:?}");
    }

    #[tokio::test]
    async fn connection_closed_without_answer_is_io_error() {
        let fake = FakeGlazeWm::start(|_| Vec::new()).await;
        let err = request(fake.port, "query monitors").await.expect_err("no answer");
        assert!(matches!(err, WmError::Io(_)), "{err:?}");
    }
}
