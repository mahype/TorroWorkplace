//! Downloads of models into Ollama on this computer (issue #325, task MW.7): `ai.pull_model` starts one,
//! `ai.list_pulls` shows them, `ai.cancel_pull` stops one. The wire format is
//! `torro_core::ai::pull`; here it is sent and followed.
//!
//! Only a model the built-in recommendation list holds as a local Ollama model is downloaded
//! ([`super::recommend::List::ollama_model`]), under the name the list gives it: a free name could name
//! another registry host (`host.example/x/y`) Ollama would load from. Only into an Ollama account whose
//! address is this computer ([`torro_core::ai::is_this_computer`]): a download fills a disk, and that is
//! only this computer's to decide here.
//!
//! [`Pulls`] keeps every download of this run in memory, oldest first, the finished ones too, so a window
//! that asks once a second still sees how one ended. At most one runs per account and model. Each
//! download is a task that reads Ollama's stream; cancelling aborts that task, which drops the
//! connection, and Ollama stops when its connection ends (what it loaded so far it keeps for the next
//! try). After a download that succeeded the account's model list is read again
//! ([`AiAccounts::models`]) before the download counts as done, so `ai.list_models` lists it then.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use torro_core::ai::{self, AiError, AiProvider, pull};
use torro_core::i18n::{tr, trf};
use torro_ipc::{AiPull, AiPullState, Response};

use super::ai::AiAccounts;
use super::recommend::List;

/// Why a download was not started or not found; [`PullError::code`] is the code of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullError {
    /// No such account or download: a sentence that names it. `not-found`.
    NotFound(String),
    /// Not an Ollama account on this computer, or a model the list does not hold. `unsupported`.
    Unsupported(&'static str),
    /// The same model downloads to the same account already. `busy`.
    Busy,
    /// Ollama did not answer the request. `offline`.
    Offline,
    /// Anything else (no HTTP client, the built-in list does not read). `error`.
    Error(String),
}

impl PullError {
    /// The error code of the answer: `not-found`, `unsupported`, `busy`, `offline` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            PullError::NotFound(_) => "not-found",
            PullError::Unsupported(_) => "unsupported",
            PullError::Busy => "busy",
            PullError::Offline => "offline",
            PullError::Error(_) => "error",
        }
    }

    /// The error answer of a command.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.to_string() }
    }
}

impl fmt::Display for PullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PullError::NotFound(message) | PullError::Error(message) => f.write_str(message),
            PullError::Unsupported(message) => f.write_str(message),
            PullError::Busy => f.write_str(tr("This model is downloading to this account already.")),
            PullError::Offline => write!(f, "{}", AiError::Offline),
        }
    }
}

/// Where the task of a running download stands, apart from [`AiPull::state`]: what cancelling it has to do.
#[derive(Debug)]
enum Phase {
    /// The request is on its way; there is no task yet. Cancelling marks the download `cancelled` but
    /// keeps this phase until the request is back (review of #325): the model stays taken, so no second
    /// start of it runs beside the first POST ([`Pulls::reserve`]). Then the task, once there, is aborted
    /// at once ([`Pulls::streaming`]), and a request that failed leaves it listed as cancelled
    /// ([`Pulls::forget`], [`Pulls::end`]).
    Starting,
    /// The task reads the stream; cancelling aborts it.
    Streaming(tokio::task::AbortHandle),
    /// Ollama said `success`; the model list is read again. Too late to cancel.
    Finishing,
    /// Done, failed or cancelled.
    Ended,
}

#[derive(Debug)]
struct Entry {
    pull: AiPull,
    phase: Phase,
}

/// The downloads of this run ([`Pulls::start`], [`Pulls::list`], [`Pulls::cancel`]). Cheap to clone: every
/// clone holds the same downloads, so a download's task updates what the core lists.
#[derive(Debug, Clone, Default)]
pub struct Pulls {
    entries: Arc<Mutex<Vec<Entry>>>,
    /// The number of the last id given out (`p1`, `p2`, …).
    last: Arc<AtomicU64>,
}

impl Pulls {
    /// Starts downloading `model` into the AI account `account` of `ai`, in the background, and answers
    /// with every download, the new one among them as `running` (or already ended, when Ollama refused it
    /// at once).
    ///
    /// The model must be a local Ollama model of the built-in list (`nimble` and `nimble:latest` are the
    /// same); it is downloaded and listed under the list's name. The account must be Ollama at an address
    /// of this computer. Errors: [`PullError::Unsupported`] for another model or account,
    /// [`PullError::NotFound`] for an account `ai` does not hold, [`PullError::Busy`] while the same model
    /// downloads to the same account, [`PullError::Offline`] when Ollama does not take the request (then
    /// nothing is listed) – unless the download was cancelled while the request was on its way: then the
    /// list with it `cancelled`, as `ai.cancel_pull` answered.
    pub async fn start(&self, ai: &Arc<AiAccounts>, account: &str, model: &str) -> Result<Vec<AiPull>, PullError> {
        let list = List::builtin().map_err(|err| PullError::Error(err.to_string()))?;
        let Some(listed) = list.ollama_model(model) else {
            return Err(PullError::Unsupported(tr("Torro downloads only the models it suggests for this computer.")));
        };
        let model = listed.model.clone();
        let Some((provider, base)) = ai.server(account).await else {
            return Err(PullError::NotFound(trf("There is no AI account {id}.", &[("id", &account)])));
        };
        if provider != AiProvider::Ollama || !ai::is_this_computer(&base) {
            return Err(PullError::Unsupported(tr("Models are downloaded only into Ollama on this computer.")));
        }
        let http = ai.http().map_err(|err| PullError::Error(err.to_string()))?.clone();
        let id = self.reserve(account, &model)?;
        let response = match http.post(pull::url(&base)).json(&pull::body(&model)).send().await {
            Ok(response) => response,
            Err(_) => return self.unreached(&id),
        };
        if !response.status().is_success() {
            // Its body is the server's words, which are not passed on.
            self.end(&id, Err(pull::server_failed()));
            return Ok(self.list());
        }
        let task = tokio::spawn(follow(self.clone(), Arc::clone(ai), id.clone(), account.to_string(), response));
        self.streaming(&id, task.abort_handle());
        Ok(self.list())
    }

    /// Every download of this run, oldest first, running and ended.
    pub fn list(&self) -> Vec<AiPull> {
        self.entries().iter().map(|entry| entry.pull.clone()).collect()
    }

    /// Cancels the running download `id` and answers with every download, this one `cancelled`. One that
    /// ended already – or has just been loaded completely and only waits for the model list – stays as it
    /// is. [`PullError::NotFound`] for an id this run never gave out.
    pub fn cancel(&self, id: &str) -> Result<Vec<AiPull>, PullError> {
        {
            let mut entries = self.entries();
            let Some(entry) = entries.iter_mut().find(|entry| entry.pull.id == id) else {
                return Err(PullError::NotFound(trf("There is no download {id}.", &[("id", &id)])));
            };
            match &entry.phase {
                // The request is still on its way: the phase holds the model until it is back.
                Phase::Starting => entry.pull.state = AiPullState::Cancelled,
                Phase::Streaming(task) => {
                    task.abort();
                    entry.phase = Phase::Ended;
                    entry.pull.state = AiPullState::Cancelled;
                }
                Phase::Finishing | Phase::Ended => {}
            }
        }
        Ok(self.list())
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new running download of `model` into `account`, its id; [`PullError::Busy`] while one runs – or
    /// while the request of one cancelled in [`Phase::Starting`] is still on its way.
    fn reserve(&self, account: &str, model: &str) -> Result<String, PullError> {
        let mut entries = self.entries();
        let held = |entry: &Entry| running(entry) || matches!(entry.phase, Phase::Starting);
        let same = |entry: &Entry| held(entry) && entry.pull.account == account && entry.pull.model == model;
        if entries.iter().any(same) {
            return Err(PullError::Busy);
        }
        let id = format!("p{}", self.last.fetch_add(1, Ordering::SeqCst) + 1);
        let pull = AiPull {
            id: id.clone(),
            account: account.to_string(),
            model: model.to_string(),
            state: AiPullState::Running,
            completed: None,
            total: None,
            error: None,
        };
        entries.push(Entry { pull, phase: Phase::Starting });
        Ok(id)
    }

    /// Takes back the download `id` that never reached Ollama. One cancelled while its request was on the
    /// way stays listed as `cancelled`, as the answer to `ai.cancel_pull` said; it only lets the model go.
    /// `true` for such a one.
    fn forget(&self, id: &str) -> bool {
        let mut entries = self.entries();
        match entries.iter_mut().find(|entry| entry.pull.id == id) {
            Some(entry) if entry.pull.state == AiPullState::Cancelled => {
                entry.phase = Phase::Ended;
                true
            }
            _ => {
                entries.retain(|entry| entry.pull.id != id);
                false
            }
        }
    }

    /// The answer to `ai.pull_model` when its request did not reach Ollama: [`PullError::Offline`], unless
    /// the download was cancelled meanwhile (review of #325) – then the list with it `cancelled`, as
    /// `ai.cancel_pull` answered, so the start does not report a failure the person already ended.
    fn unreached(&self, id: &str) -> Result<Vec<AiPull>, PullError> {
        match self.forget(id) {
            true => Ok(self.list()),
            false => Err(PullError::Offline),
        }
    }

    /// The task of `id` reads the stream now. When the download was cancelled while its request was on the
    /// way, the task is aborted at once.
    fn streaming(&self, id: &str, task: tokio::task::AbortHandle) {
        let mut entries = self.entries();
        let Some(entry) = entries.iter_mut().find(|entry| entry.pull.id == id) else { return task.abort() };
        match entry.phase {
            Phase::Starting if entry.pull.state == AiPullState::Cancelled => {
                task.abort();
                entry.phase = Phase::Ended;
            }
            Phase::Starting => entry.phase = Phase::Streaming(task),
            Phase::Ended if entry.pull.state == AiPullState::Cancelled => task.abort(),
            _ => {}
        }
    }

    /// The bytes loaded of `id` so far, while it runs.
    fn progress(&self, id: &str, progress: &pull::Progress) {
        if let Some(entry) = self.entries().iter_mut().find(|entry| entry.pull.id == id && running(entry)) {
            entry.pull.completed = progress.completed();
            entry.pull.total = progress.total();
        }
    }

    /// Ollama has the model: from now on it cannot be cancelled. `false` when it was cancelled before.
    fn finishing(&self, id: &str) -> bool {
        match self.entries().iter_mut().find(|entry| entry.pull.id == id && running(entry)) {
            Some(entry) => {
                entry.phase = Phase::Finishing;
                true
            }
            None => false,
        }
    }

    /// Ends `id` as done (`Ok`) or failed with the sentence in `Err` – unless it ended before (cancelled);
    /// one cancelled while its request was on the way only lets the model go.
    fn end(&self, id: &str, outcome: Result<(), String>) {
        let mut entries = self.entries();
        let Some(entry) = entries.iter_mut().find(|entry| entry.pull.id == id) else { return };
        if !running(entry) {
            if matches!(entry.phase, Phase::Starting) {
                entry.phase = Phase::Ended;
            }
            return;
        }
        entry.phase = Phase::Ended;
        match outcome {
            Ok(()) => entry.pull.state = AiPullState::Done,
            Err(error) => {
                entry.pull.state = AiPullState::Failed;
                entry.pull.error = Some(error);
            }
        }
    }
}

/// The download still runs: neither ended nor cancelled.
fn running(entry: &Entry) -> bool {
    entry.pull.state == AiPullState::Running
}

/// Why a download failed whose stream ended without `success` or broke off.
fn stopped() -> String {
    tr("The download stopped before it was complete. Try again.").to_string()
}

/// The task of the download `id`: reads Ollama's stream of `response` line by line into its progress
/// until `success` or an error, then – after a success – reads the model list of `account` again and
/// ends the download. Aborted when it is cancelled, which drops the connection.
async fn follow(pulls: Pulls, ai: Arc<AiAccounts>, id: String, account: String, mut response: reqwest::Response) {
    let mut decoder = pull::Decoder::new();
    let mut progress = pull::Progress::default();
    let outcome = loop {
        let events = match response.chunk().await {
            Ok(Some(bytes)) => decoder.push(&bytes),
            Ok(None) => {
                let last = std::mem::take(&mut decoder).finish();
                break read(&pulls, &id, last.into_iter().collect(), &mut progress).unwrap_or_else(|| Err(stopped()));
            }
            Err(_) => break Err(stopped()),
        };
        if let Some(outcome) = read(&pulls, &id, events, &mut progress) {
            break outcome;
        }
    };
    // Ollama ends its side once the stream is over; nothing more is read.
    drop(response);
    if outcome.is_ok() {
        if !pulls.finishing(&id) {
            return;
        }
        // The log names the account, never the model.
        if let Err(err) = ai.models(&account, true).await {
            eprintln!("torroworkplace: {account}: the models were not read again after a download ({})", err.code());
        }
    }
    pulls.end(&id, outcome);
}

/// Counts `events` into `progress` and the download `id`; `Some` once one of them ends it: `Ok` for
/// `success`, `Err` with the suite's sentence for an error.
fn read(
    pulls: &Pulls,
    id: &str,
    events: Vec<pull::PullEvent>,
    progress: &mut pull::Progress,
) -> Option<Result<(), String>> {
    let mut outcome = None;
    for event in &events {
        progress.add(event);
        if let Some(error) = &event.error {
            outcome = Some(Err(error.clone()));
            break;
        }
        if event.done {
            outcome = Some(Ok(()));
            break;
        }
    }
    pulls.progress(id, progress);
    outcome
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use httpmock::MockServer;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{mpsc, oneshot};
    use torro_core::Account;

    use super::*;
    use crate::account::ai::tests::{NOBODY, service};
    use crate::account::github::tests::{MemoryTokens, accounts_file};

    /// The AI service in `dir` with accounts `stored`, loaded without asking any server.
    async fn accounts(dir: &std::path::Path, stored: &[Account]) -> Arc<AiAccounts> {
        torro_core::account::save_to(&accounts_file(dir), stored).expect("accounts saved");
        let ai = service(dir, &MemoryTokens::default(), Vec::new());
        ai.load().await.expect("loaded");
        Arc::new(ai)
    }

    /// An Ollama account at `base`, as stored.
    fn ollama(base: &str) -> Account {
        Account::ai(AiProvider::Ollama, base)
    }

    /// The download `id` once it ended, asked every 10 ms for up to 5 s.
    async fn ended(pulls: &Pulls, id: &str) -> AiPull {
        for _ in 0..500 {
            let pull = pulls.list().into_iter().find(|pull| pull.id == id).expect("listed");
            if pull.state != AiPullState::Running {
                return pull;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("download {id} did not end");
    }

    /// An Ollama whose pull of `model` answers `lines` (NDJSON) and whose model list names it.
    async fn fake_pull(model: &str, lines: &[serde_json::Value]) -> MockServer {
        let server = MockServer::start_async().await;
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        server
            .mock_async(|when, then| {
                when.method("POST").path("/api/pull").json_body(json!({"model": model, "stream": true}));
                then.status(200).header("content-type", "application/x-ndjson").body(body);
            })
            .await;
        server
    }

    #[tokio::test]
    async fn a_download_adds_up_its_layers_and_is_done_once_the_models_are_read_again() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = fake_pull(
            "nimble",
            &[
                json!({"status": "pulling manifest"}),
                json!({"status": "pulling a", "digest": "sha256:a", "total": 9000, "completed": 0}),
                json!({"status": "pulling a", "digest": "sha256:a", "total": 9000, "completed": 4000}),
                json!({"status": "pulling b", "digest": "sha256:b", "total": 1000, "completed": 1000}),
                json!({"status": "pulling a", "digest": "sha256:a", "total": 9000, "completed": 9000}),
                json!({"status": "verifying sha256 digest"}),
                json!({"status": "writing manifest"}),
                json!({"status": "success"}),
            ],
        )
        .await;
        let listed = server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(200)
                    .json_body(json!({"object": "list", "data": [{"id": "nimble:latest", "object": "model"}]}));
            })
            .await;
        let account = ollama(&server.url("/v1"));
        let ai = accounts(dir.path(), std::slice::from_ref(&account)).await;
        let pulls = Pulls::default();

        let started = pulls.start(&ai, &account.id, "nimble:latest").await.expect("started");
        assert_eq!(started.len(), 1);
        assert_eq!((started[0].model.as_str(), started[0].account.as_str()), ("nimble", account.id.as_str()));
        let done = ended(&pulls, &started[0].id).await;
        assert_eq!(done.state, AiPullState::Done);
        assert_eq!((done.completed, done.total, done.error.as_deref()), (Some(10_000), Some(10_000), None));
        assert_eq!(listed.calls_async().await, 1, "the model list was read again before it counted as done");
        assert_eq!(pulls.list(), std::slice::from_ref(&done), "an ended download stays listed");
        assert_eq!(pulls.cancel(&done.id), Ok(vec![done]), "a download that ended stays as it is");
    }

    #[tokio::test]
    async fn an_error_line_fails_the_download_in_torros_words() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = fake_pull(
            "qwen3:14b",
            &[
                json!({"status": "pulling a", "digest": "sha256:a", "total": 100, "completed": 30}),
                json!({"error": "write /Users/anna/.ollama/models/blobs/sha256-a: no space left on device"}),
                json!({"status": "success"}),
            ],
        )
        .await;
        let listed = server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(200).json_body(json!({"object": "list", "data": []}));
            })
            .await;
        let account = ollama(&server.url("/v1"));
        let ai = accounts(dir.path(), std::slice::from_ref(&account)).await;
        let pulls = Pulls::default();

        let started = pulls.start(&ai, &account.id, "qwen3:14b").await.expect("started");
        let failed = ended(&pulls, &started[0].id).await;
        assert_eq!(failed.state, AiPullState::Failed);
        assert_eq!(failed.error, Some(pull::server_failed()), "never the server's words");
        assert_eq!(failed.completed, Some(30), "what it loaded before stays shown");
        assert_eq!(listed.calls_async().await, 0, "nothing new to list");
    }

    #[tokio::test]
    async fn a_stream_that_ends_without_success_or_a_refused_request_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = fake_pull(
            "qwen3:4b",
            &[json!({"status": "pulling a", "digest": "sha256:a", "total": 100, "completed": 30})],
        )
        .await;
        server
            .mock_async(|when, then| {
                when.method("POST").path("/api/pull").json_body_includes(r#"{"model":"nimble"}"#);
                then.status(500).json_body(json!({"error": "registry.example says no"}));
            })
            .await;
        let account = ollama(&server.url("/v1"));
        let ai = accounts(dir.path(), std::slice::from_ref(&account)).await;
        let pulls = Pulls::default();

        let cut = pulls.start(&ai, &account.id, "qwen3:4b").await.expect("started");
        let cut = ended(&pulls, &cut[0].id).await;
        assert_eq!((cut.state, cut.error), (AiPullState::Failed, Some(stopped())));

        let refused = pulls.start(&ai, &account.id, "nimble").await.expect("listed as failed");
        let refused = refused.into_iter().find(|pull| pull.model == "nimble").expect("the second download");
        assert_eq!((refused.state, refused.error), (AiPullState::Failed, Some(pull::server_failed())));
    }

    #[tokio::test]
    async fn only_listed_models_go_only_to_ollama_on_this_computer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let here = ollama("http://localhost:11434/v1");
        let lan = ollama("http://gpu.example:11434/v1");
        let cloud = Account::ai(AiProvider::OpenRouter, "https://openrouter.ai/api/v1");
        let away = ollama(NOBODY);
        let ai = accounts(dir.path(), &[here.clone(), lan.clone(), cloud.clone(), away.clone()]).await;
        let pulls = Pulls::default();
        let unsupported = |answer: Result<Vec<AiPull>, PullError>| matches!(answer, Err(PullError::Unsupported(_)));

        for model in ["host.example/x/y", "library/qwen3:14b", "llama3:70b", "qwen3.6-35b-a3b", ""] {
            assert!(unsupported(pulls.start(&ai, &here.id, model).await), "{model} is not in the list");
        }
        assert!(unsupported(pulls.start(&ai, &lan.id, "qwen3:14b").await), "Ollama elsewhere");
        assert!(unsupported(pulls.start(&ai, &cloud.id, "qwen3:14b").await), "no Ollama");
        let gone = pulls.start(&ai, "ai:ollama@nowhere:11434", "qwen3:14b").await;
        assert!(matches!(&gone, Err(PullError::NotFound(_))), "{gone:?}");
        assert_eq!(pulls.start(&ai, &away.id, "qwen3:14b").await, Err(PullError::Offline));
        assert!(pulls.list().is_empty(), "nothing reached Ollama, nothing is listed");
        assert!(matches!(pulls.cancel("p1"), Err(PullError::NotFound(_))));
        assert_eq!(PullError::Offline.code(), "offline");
        assert_eq!(
            PullError::Busy.response(),
            Response::Error { code: "busy".into(), message: PullError::Busy.to_string() }
        );
    }

    /// An Ollama on a socket of its own that answers one pull with the lines the test sends, one at a time,
    /// and says when the connection was closed from the other side. httpmock answers only whole bodies.
    struct Stalling {
        base: String,
        lines: mpsc::UnboundedSender<serde_json::Value>,
        closed: oneshot::Receiver<()>,
    }

    async fn stalling_ollama() -> Stalling {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bound");
        let base = format!("http://{}/v1", listener.local_addr().expect("address"));
        let (lines, mut next) = mpsc::unbounded_channel::<serde_json::Value>();
        let (gone, closed) = oneshot::channel();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            // One pull only: a second connection is refused.
            drop(listener);
            let mut buffer = vec![0_u8; 8192];
            let mut request = Vec::new();
            while !whole_request(&request) {
                match socket.read(&mut buffer).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
            }
            let head = "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ntransfer-encoding: chunked\r\n\r\n";
            if socket.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            loop {
                tokio::select! {
                    line = next.recv() => {
                        let Some(line) = line else { return };
                        let line = format!("{line}\n");
                        let chunk = format!("{:x}\r\n{line}\r\n", line.len());
                        if socket.write_all(chunk.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                    read = socket.read(&mut buffer) => {
                        if matches!(read, Ok(0) | Err(_)) {
                            let _ = gone.send(());
                            return;
                        }
                    }
                }
            }
        });
        Stalling { base, lines, closed }
    }

    /// The head of `request` arrived and as many bytes of its body as its `content-length` says.
    fn whole_request(request: &[u8]) -> bool {
        let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else { return false };
        let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        request.len() >= end + 4 + length
    }

    #[tokio::test]
    async fn a_running_download_shows_its_progress_runs_once_and_is_cancelled_mid_stream() {
        let dir = tempfile::tempdir().expect("temp dir");
        let ollama_here = stalling_ollama().await;
        let account = ollama(&ollama_here.base);
        let ai = accounts(dir.path(), std::slice::from_ref(&account)).await;
        let pulls = Pulls::default();

        let started = pulls.start(&ai, &account.id, "qwen3:4b").await.expect("started");
        let id = started[0].id.clone();
        assert_eq!((started[0].state, started[0].completed), (AiPullState::Running, None));
        ollama_here.lines.send(json!({"status": "pulling manifest"})).expect("sent");
        ollama_here
            .lines
            .send(json!({"status": "pulling a", "digest": "sha256:a", "total": 1000, "completed": 250}))
            .expect("sent");
        let mut seen = None;
        for _ in 0..500 {
            let now = pulls.list().remove(0);
            if now.completed.is_some() {
                seen = Some(now);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let seen = seen.expect("the progress arrived");
        assert_eq!((seen.state, seen.completed, seen.total), (AiPullState::Running, Some(250), Some(1000)));

        assert_eq!(pulls.start(&ai, &account.id, "qwen3:4b").await, Err(PullError::Busy));

        let cancelled = pulls.cancel(&id).expect("cancelled");
        assert_eq!((cancelled[0].state, cancelled[0].completed), (AiPullState::Cancelled, Some(250)));
        tokio::time::timeout(Duration::from_secs(5), ollama_here.closed)
            .await
            .expect("Ollama saw the connection end")
            .expect("told");
        let _ = ollama_here.lines.send(json!({"status": "success"}));
        assert_eq!(pulls.cancel(&id).expect("listed")[0].state, AiPullState::Cancelled, "it stays cancelled");

        // A cancelled download no longer holds the model: the next start reaches for Ollama again (which
        // took one pull only and is gone now).
        assert_eq!(pulls.start(&ai, &account.id, "qwen3:4b").await, Err(PullError::Offline));
        assert_eq!(pulls.list().len(), 1);
    }

    /// Review of #325 (F6): cancelled while its request is on the way, a download keeps the model until the
    /// request is back – no second start beside the first POST –, and stays listed as `cancelled` when the
    /// request then fails, is refused or reaches Ollama (whose task is aborted at once).
    #[tokio::test]
    async fn a_download_cancelled_while_starting_holds_the_model_until_its_request_is_back() {
        let pulls = Pulls::default();
        let state = |id: &str| pulls.list().into_iter().find(|pull| pull.id == id).map(|pull| pull.state);

        // The request fails (`send` errs): listed as cancelled, the model free again – and the start
        // answers with that list, not `offline` (review of #325).
        let id = pulls.reserve("ai:ollama@localhost:11434", "nimble").expect("reserved");
        assert_eq!(pulls.cancel(&id).expect("cancelled")[0].state, AiPullState::Cancelled);
        assert_eq!(pulls.reserve("ai:ollama@localhost:11434", "nimble"), Err(PullError::Busy), "the POST is out");
        let answer = pulls.unreached(&id).expect("the list, not offline");
        assert_eq!(
            answer.iter().map(|pull| (pull.id.as_str(), pull.state)).collect::<Vec<_>>(),
            [(id.as_str(), AiPullState::Cancelled)]
        );
        assert_eq!(state(&id), Some(AiPullState::Cancelled), "still listed as the cancel answered");
        let second = pulls.reserve("ai:ollama@localhost:11434", "nimble").expect("free again");

        // Ollama refuses it: the same.
        pulls.cancel(&second).expect("cancelled");
        pulls.end(&second, Err(pull::server_failed()));
        assert_eq!(state(&second), Some(AiPullState::Cancelled), "not failed: the person cancelled it");
        let third = pulls.reserve("ai:ollama@localhost:11434", "nimble").expect("free again");

        // Ollama takes it: the task is aborted at once, and the model is free.
        pulls.cancel(&third).expect("cancelled");
        let task = tokio::spawn(std::future::pending::<()>());
        pulls.streaming(&third, task.abort_handle());
        assert!(task.await.expect_err("aborted").is_cancelled());
        assert_eq!(state(&third), Some(AiPullState::Cancelled));
        assert!(pulls.reserve("ai:ollama@localhost:11434", "nimble").is_ok());
        assert_eq!(pulls.list().len(), 4);
    }

    /// A request that does not reach Ollama, not cancelled: `offline`, and nothing is listed.
    #[test]
    fn a_start_that_reaches_no_ollama_is_offline_and_listed_nowhere() {
        let pulls = Pulls::default();
        let id = pulls.reserve("ai:ollama@localhost:11434", "nimble").expect("reserved");
        assert_eq!(pulls.unreached(&id), Err(PullError::Offline));
        assert!(pulls.list().is_empty());
    }
}
