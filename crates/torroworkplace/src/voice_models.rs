//! Downloads of the models the assistant listens and speaks with (issue #762): `settings.list_voice_models`,
//! `settings.download_voice_model`, `settings.cancel_voice_download` and `settings.remove_voice_model`. The
//! catalogue is `torro_core::voice_models::MODELS`; nothing else is ever downloaded, and only from the
//! addresses it names, without any token or other credential.
//!
//! [`VoiceDownloads`] keeps one entry per model of this run in memory: running, failed or cancelled. Each
//! download is one task that loads the model's files one after the other into the model's folder
//! (`<voice>/models/<id>/`). A file is written to `<name>.part` first, streamed through SHA-256, checked
//! against its size and hash and only then renamed to its name, so a file with its final name is always
//! whole. A `.part` from a cancelled or broken download is continued with `Range` (its bytes are hashed
//! first); a server that answers the whole file instead starts it from zero. Cancelling aborts the task
//! and keeps the `.part`; removing cancels first and then deletes the folder.
//!
//! Everything else is read from the disk on each listing: a model whose files are all there with their
//! size is `ready`, one with some bytes of it `cancelled` (a download of an earlier run that stopped),
//! the rest `missing`. What memory says about a model of this run (running, failed, cancelled) goes first.
//!
//! The file is written synchronously between two reads of the stream: a task is aborted only while it
//! waits, so a `.part` always ends after a whole chunk.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sha2::{Digest, Sha256};
use torro_core::i18n::tr;
use torro_core::voice_models::{MODELS, Model, ModelFile};
use torro_ipc::{Response, VoiceModel, VoiceModelState};

/// How long the connection to Hugging Face may take to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the stream may stay silent before the download counts as broken off. No limit for the
/// whole download: a model is up to a gigabyte.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How much of a `.part` is read at once when it is hashed before a download goes on.
const HASH_BUFFER: usize = 1 << 20;

/// Why a command on the models was refused; [`VoiceModelError::code`] is the code of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceModelError {
    /// The catalogue has no model of that id. `not-found`.
    NotFound,
    /// The model downloads already. `busy`.
    Busy,
    /// Anything else, a sentence for people (no voice folder, no HTTP client, a folder that cannot be
    /// removed). `error`.
    Error(String),
}

impl VoiceModelError {
    /// The error code of the answer: `not-found`, `busy` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            VoiceModelError::NotFound => "not-found",
            VoiceModelError::Busy => "busy",
            VoiceModelError::Error(_) => "error",
        }
    }

    /// The error answer of a command.
    pub fn response(&self) -> Response {
        Response::Error { code: self.code().into(), message: self.to_string() }
    }
}

impl fmt::Display for VoiceModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceModelError::NotFound => f.write_str(tr("There is no such speech model.")),
            VoiceModelError::Busy => f.write_str(tr("This model is downloading already.")),
            VoiceModelError::Error(message) => f.write_str(message),
        }
    }
}

/// What this run knows about one model beyond the disk.
#[derive(Debug)]
struct Entry {
    /// `Running`, `Failed` or `Cancelled`; a download that ended well leaves no entry.
    state: VoiceModelState,
    /// Why it failed, with `Failed`.
    error: Option<String>,
    /// The task while it runs.
    task: Option<tokio::task::JoinHandle<()>>,
    /// A cancelled task until it has really stopped: abort only marks it, and it may still write the
    /// piece it holds; a new download waits for it with `busy` (review of #762).
    stopping: Option<tokio::task::AbortHandle>,
    /// [`VoiceDownloads::remove`] deletes the model's folder: a download waits with `busy` until it is
    /// gone, so the folder never takes the files of a new one with it (PR review of #762).
    removing: bool,
    /// Which start this is, so a task that was cancelled and started again does not end the new one.
    run: u64,
}

/// The models of the catalogue and their downloads (see the module). Cheap to clone: every clone holds
/// the same downloads, so a download's task updates what the core lists.
#[derive(Debug, Clone)]
pub struct VoiceDownloads {
    /// The voice folder (`torro_core::voice_models::voice_folder`); `None` without a home directory.
    folder: Option<PathBuf>,
    /// The catalogue; tests bring a small one whose files a local server serves.
    models: &'static [Model],
    /// `None` when no client could be built: then every download fails at once.
    http: Option<reqwest::Client>,
    entries: Arc<Mutex<HashMap<&'static str, Entry>>>,
    runs: Arc<AtomicU64>,
}

impl VoiceDownloads {
    /// The catalogue in the user's voice folder.
    pub fn system() -> VoiceDownloads {
        VoiceDownloads::new(torro_core::voice_models::voice_folder(), &MODELS)
    }

    /// The models `models` in the voice folder `folder` (`None`: no folder, nothing can be downloaded and
    /// every model is `missing`). Tests pass a temp directory and a catalogue of their own.
    pub fn new(folder: Option<PathBuf>, models: &'static [Model]) -> VoiceDownloads {
        VoiceDownloads { folder, models, http: http_client(), entries: Arc::default(), runs: Arc::default() }
    }

    /// Whether the catalogue has the model `id`: the core refuses any other before anyone is asked.
    pub fn knows(&self, id: &str) -> bool {
        self.model(id).is_some()
    }

    /// Every model of the catalogue in its order, with where it stands (see the module).
    pub fn list(&self) -> Vec<VoiceModel> {
        let entries = self.entries();
        self.models.iter().map(|model| self.described(model, entries.get(model.id))).collect()
    }

    /// Starts downloading the model `id` in the background and answers the list with it `running`. A
    /// model that is there already answers the list as it is. Errors: [`VoiceModelError::NotFound`] for an
    /// id the catalogue does not have, [`VoiceModelError::Busy`] while it downloads, and
    /// [`VoiceModelError::Error`] without a voice folder or HTTP client. Must run inside a tokio runtime.
    pub fn download(&self, id: &str) -> Result<Vec<VoiceModel>, VoiceModelError> {
        let model = self.model(id).ok_or(VoiceModelError::NotFound)?;
        let Some(folder) = self.folder.clone() else { return Err(VoiceModelError::Error(unsaved())) };
        let Some(http) = self.http.clone() else { return Err(VoiceModelError::Error(unsaved())) };
        {
            let mut entries = self.entries();
            let busy = |entry: &Entry| {
                entry.state == VoiceModelState::Running
                    || entry.removing
                    || entry.stopping.as_ref().is_some_and(|task| !task.is_finished())
            };
            if entries.get(model.id).is_some_and(busy) {
                return Err(VoiceModelError::Busy);
            }
            if model.ready(&folder) {
                entries.remove(model.id);
            } else {
                let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
                let downloads = self.clone();
                let task = tokio::spawn(async move {
                    let outcome = fetch_model(&http, &model.folder(&folder), model.files).await;
                    downloads.end(model.id, run, outcome);
                });
                let entry = Entry {
                    state: VoiceModelState::Running,
                    error: None,
                    task: Some(task),
                    stopping: None,
                    removing: false,
                    run,
                };
                entries.insert(model.id, entry);
            }
        }
        Ok(self.list())
    }

    /// Cancels the running download of `id` and answers the list with it `cancelled`; what it loaded
    /// stays for the next download. Waits until its task has stopped, so nothing writes afterwards. A
    /// model that does not download stays as it is. [`VoiceModelError::NotFound`] for an unknown id.
    pub async fn cancel(&self, id: &str) -> Result<Vec<VoiceModel>, VoiceModelError> {
        let model = self.model(id).ok_or(VoiceModelError::NotFound)?;
        let task = {
            let mut entries = self.entries();
            match entries.get_mut(model.id) {
                Some(entry) if entry.state == VoiceModelState::Running => {
                    entry.state = VoiceModelState::Cancelled;
                    let task = aborted(entry.task.take());
                    entry.stopping = task.as_ref().map(tokio::task::JoinHandle::abort_handle);
                    task
                }
                _ => None,
            }
        };
        stopped(task).await;
        Ok(self.list())
    }

    /// Removes the model `id` from this computer with what a cancelled download left, after cancelling a
    /// running one; answers the list with it `missing`. [`VoiceModelError::NotFound`] for an unknown id,
    /// [`VoiceModelError::Error`] when its folder cannot be deleted.
    pub async fn remove(&self, id: &str) -> Result<Vec<VoiceModel>, VoiceModelError> {
        let model = self.model(id).ok_or(VoiceModelError::NotFound)?;
        // The entry stays, marked, until the folder is gone: a download that comes meanwhile is `busy`.
        let task = {
            let mut entries = self.entries();
            let run = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
            let old = entries.remove(model.id);
            let entry = Entry {
                state: VoiceModelState::Cancelled,
                error: None,
                task: None,
                stopping: None,
                removing: true,
                run,
            };
            entries.insert(model.id, entry);
            aborted(old.and_then(|entry| entry.task))
        };
        stopped(task).await;
        let deleted = match &self.folder {
            Some(folder) => match std::fs::remove_dir_all(model.folder(folder)) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => {
                    eprintln!("torroworkplace: the voice model {} was not removed ({:?})", model.id, err.kind());
                    Err(VoiceModelError::Error(tr("The model could not be removed. Try again.").into()))
                }
            },
            None => Ok(()),
        };
        // Whatever happened, the mark goes: the disk tells how the model stands now.
        self.entries().remove(model.id);
        deleted.map(|()| self.list())
    }

    fn model(&self, id: &str) -> Option<&'static Model> {
        self.models.iter().find(|model| model.id == id)
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<&'static str, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends the run `run` of the model `id`: a download that ended well leaves no entry (the disk says
    /// `ready`), also when a cancel came while it finished; one that failed keeps its sentence. Nothing
    /// when it was started anew since, and no failure of a cancelled one.
    fn end(&self, id: &'static str, run: u64, outcome: Result<(), String>) {
        let mut entries = self.entries();
        let Some(entry) = entries.get_mut(id) else { return };
        if entry.run != run {
            return;
        }
        match outcome {
            Ok(()) => {
                entries.remove(id);
            }
            Err(_) if entry.state != VoiceModelState::Running => {}
            Err(error) => {
                entry.state = VoiceModelState::Failed;
                entry.error = Some(error);
                entry.task = None;
            }
        }
    }

    /// `model` as listed, with what this run knows of it in `entry`.
    fn described(&self, model: &Model, entry: Option<&Entry>) -> VoiceModel {
        let folder = self.folder.as_deref();
        let loaded = folder.map_or(0, |folder| on_disk(model, folder));
        let (state, error) = match entry {
            Some(entry) => (entry.state, entry.error.clone()),
            None if folder.is_some_and(|folder| model.ready(folder)) => (VoiceModelState::Ready, None),
            None if loaded > 0 => (VoiceModelState::Cancelled, None),
            None => (VoiceModelState::Missing, None),
        };
        let completed = if state == VoiceModelState::Ready { model.bytes() } else { loaded };
        VoiceModel {
            id: model.id.into(),
            name: model.name.into(),
            kind: model.kind,
            bytes: model.bytes(),
            recommended: model.recommended,
            state,
            completed,
            error: error.filter(|_| state == VoiceModelState::Failed),
        }
    }
}

/// `task`, told to stop while the caller still holds the lock of the entries (review of #762). Abort only
/// marks it; the entry keeps its [`tokio::task::AbortHandle`] until it ended, so a new download waits.
fn aborted(task: Option<tokio::task::JoinHandle<()>>) -> Option<tokio::task::JoinHandle<()>> {
    if let Some(task) = &task {
        task.abort();
    }
    task
}

/// Waits until `task`, aborted, has stopped.
async fn stopped(task: Option<tokio::task::JoinHandle<()>>) {
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
}

/// The HTTP client of the downloads: it follows Hugging Face's redirects to its CDN (which keep the
/// `Range` header; reqwest drops only credentials across hosts, and none are sent), with time limits for
/// opening and for silence but none for the whole download.
fn http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(crate::places::USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(redirect))
        .build()
        .ok()
}

/// Where a download may be sent on: up to 10 times, and only over https, so a redirect never takes the
/// request to a plain address. Loopback addresses are the tests' servers.
fn redirect(attempt: reqwest::redirect::Attempt<'_>) -> reqwest::redirect::Action {
    let url = attempt.url();
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if attempt.previous().len() >= 10 || (url.scheme() != "https" && !loopback) {
        attempt.stop()
    } else {
        attempt.follow()
    }
}

/// The bytes of `model` in `folder`: of the files there with their name (at most their size) and of the
/// `.part` files of the others.
fn on_disk(model: &Model, folder: &Path) -> u64 {
    let len = |path: PathBuf| std::fs::metadata(path).ok().filter(|meta| meta.is_file()).map(|meta| meta.len());
    model
        .files
        .iter()
        .map(|file| match len(model.path(folder, file.name)) {
            Some(bytes) => bytes.min(file.bytes),
            None => len(part_path(&model.folder(folder), file)).unwrap_or(0).min(file.bytes),
        })
        .sum()
}

/// Where `file` is written before it was checked.
fn part_path(folder: &Path, file: &ModelFile) -> PathBuf {
    folder.join(format!("{}.part", file.name))
}

/// The sentence of a download that reached no server.
fn offline() -> String {
    tr("No connection to the internet. Check the network and try again.").into()
}

/// The sentence of a download the server refused.
fn refused() -> String {
    tr("Hugging Face did not deliver the model. Try again later.").into()
}

/// The sentence of a file that did not match its hash.
fn damaged() -> String {
    tr("The download was damaged and was removed. Try again.").into()
}

/// The sentence of a file that could not be written.
fn unsaved() -> String {
    tr("The model could not be saved. Check the free space.").into()
}

/// [`unsaved`] for the disk error `err`, which goes into the log by its kind only (no path).
fn disk(err: &io::Error) -> String {
    eprintln!("torroworkplace: a voice model could not be written ({:?})", err.kind());
    unsaved()
}

/// Loads every file of `files` into `folder`, one after the other.
async fn fetch_model(http: &reqwest::Client, folder: &Path, files: &[ModelFile]) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|err| disk(&err))?;
    for file in files {
        fetch_file(http, folder, file).await?;
    }
    Ok(())
}

/// Loads `file` into `folder` (see the module): skipped when it is there with its size, continued from
/// its `.part`, checked and renamed.
async fn fetch_file(http: &reqwest::Client, folder: &Path, file: &ModelFile) -> Result<(), String> {
    let path = folder.join(file.name);
    if std::fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes) {
        return Ok(());
    }
    let part = part_path(folder, file);
    let mut have = std::fs::metadata(&part).map_or(0, |meta| meta.len());
    if have > file.bytes {
        std::fs::remove_file(&part).map_err(|err| disk(&err))?;
        have = 0;
    }
    let mut hasher = match have {
        0 => Sha256::new(),
        _ => hashed(part.clone()).await.map_err(|err| disk(&err))?,
    };
    if have < file.bytes {
        let mut request = http.get(file.url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().await.map_err(|err| sent_failure(&err))?;
        let status = response.status();
        let continues = have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT && starts_at(&response, have);
        if !continues {
            if status != reqwest::StatusCode::OK {
                return Err(refused());
            }
            // The whole file after all: from zero.
            have = 0;
            hasher = Sha256::new();
        }
        let mut out = match have {
            0 => std::fs::File::create(&part),
            _ => std::fs::OpenOptions::new().append(true).open(&part),
        }
        .map_err(|err| disk(&err))?;
        while let Some(chunk) = response.chunk().await.map_err(|_| offline())? {
            if have + chunk.len() as u64 > file.bytes {
                return Err(thrown_away(&part));
            }
            out.write_all(&chunk).map_err(|err| disk(&err))?;
            hasher.update(&chunk);
            have += chunk.len() as u64;
        }
        out.flush().map_err(|err| disk(&err))?;
        // Broken off before its end: what came stays for the next download.
        if have < file.bytes {
            return Err(offline());
        }
    }
    if hex(&hasher.finalize()) != file.sha256 {
        return Err(thrown_away(&part));
    }
    std::fs::rename(&part, &path).map_err(|err| disk(&err))?;
    Ok(())
}

/// Removes the damaged `.part` at `part`; the sentence for it.
fn thrown_away(part: &Path) -> String {
    if let Err(err) = std::fs::remove_file(part) {
        eprintln!("torroworkplace: a damaged voice model file was not removed ({:?})", err.kind());
    }
    damaged()
}

/// Whether a `206` answer continues at byte `have` (its `Content-Range: bytes <have>-…`).
fn starts_at(response: &reqwest::Response, have: u64) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|range| range.strip_prefix("bytes "))
        .and_then(|range| range.split('-').next())
        .and_then(|start| start.trim().parse::<u64>().ok())
        == Some(have)
}

/// The sentence of a request that got no answer: no connection, except a redirect that went wrong.
fn sent_failure(err: &reqwest::Error) -> String {
    if err.is_redirect() { refused() } else { offline() }
}

/// The hash of the bytes in `part` so far, read off the runtime's threads: a gigabyte takes seconds.
async fn hashed(part: PathBuf) -> io::Result<Sha256> {
    let read = tokio::task::spawn_blocking(move || {
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(part)?;
        let mut buffer = vec![0_u8; HASH_BUFFER];
        loop {
            match file.read(&mut buffer)? {
                0 => return Ok(hasher),
                n => hasher.update(&buffer[..n]),
            }
        }
    });
    read.await.map_err(io::Error::other)?
}

/// `bytes` as lower-case hex, the form of the catalogue's hashes.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use httpmock::MockServer;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use torro_core::voice_models::VoiceModelKind;

    use super::*;

    fn leak(text: &str) -> &'static str {
        Box::leak(text.to_string().into_boxed_str())
    }

    /// A catalogue of one model `test` whose files `files` (name, content) are served under `base`.
    fn catalogue(base: &str, files: &[(&str, &[u8])]) -> &'static [Model] {
        let files: Vec<ModelFile> = files
            .iter()
            .map(|(name, body)| ModelFile {
                name: leak(name),
                url: leak(&format!("{base}/{name}")),
                sha256: leak(&hex(&Sha256::digest(body))),
                bytes: body.len() as u64,
            })
            .collect();
        let files: &'static [ModelFile] = Box::leak(files.into_boxed_slice());
        let model =
            Model { id: "test", name: "Test model", kind: VoiceModelKind::Recognition, recommended: true, files };
        Box::leak(vec![model].into_boxed_slice())
    }

    /// The model `test` once its download is no longer running, asked every 10 ms for up to 5 s.
    async fn settled(downloads: &VoiceDownloads) -> VoiceModel {
        for _ in 0..500 {
            let model = downloads.list().remove(0);
            if model.state != VoiceModelState::Running {
                return model;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the download did not end");
    }

    const SMALL: &[u8] = b"tokens of the test model";

    /// Bytes of a size that comes in several chunks.
    fn large() -> Vec<u8> {
        (0..200_000_u32).map(|n| (n % 251) as u8).collect()
    }

    #[tokio::test]
    async fn a_download_loads_every_file_checks_it_and_is_ready() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = MockServer::start_async().await;
        let body = large();
        for (name, content) in [("encoder.onnx", body.as_slice()), ("tokens.txt", SMALL)] {
            server
                .mock_async(|when, then| {
                    when.method("GET").path(format!("/{name}")).header_missing("authorization").header_missing("range");
                    then.status(200).body(content);
                })
                .await;
        }
        let models = catalogue(&server.base_url(), &[("encoder.onnx", &body), ("tokens.txt", SMALL)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        assert_eq!(downloads.list()[0].state, VoiceModelState::Missing, "nothing on the disk yet");

        let started = downloads.download("test").expect("started");
        assert_eq!(started[0].state, VoiceModelState::Running);
        let done = settled(&downloads).await;
        assert_eq!((done.state, done.completed, done.error), (VoiceModelState::Ready, done.bytes, None));
        let folder = models[0].folder(dir.path());
        assert_eq!(std::fs::read(folder.join("encoder.onnx")).expect("file"), body);
        assert!(!folder.join("encoder.onnx.part").exists() && !folder.join("tokens.txt.part").exists());
        // Ready: a second download only answers the list.
        assert_eq!(downloads.download("test").expect("listed")[0].state, VoiceModelState::Ready);
        assert_eq!(downloads.download("../etc"), Err(VoiceModelError::NotFound));
        assert!(!downloads.knows("evil\nApprove") && downloads.knows("test"));
    }

    /// A server on a socket of its own: it answers a request without `Range` with the head of the whole
    /// file and only its first half, then holds the connection; one with `Range: bytes=<n>-` with `206`
    /// and the rest. The ranges asked for are sent to `ranges`.
    async fn halving_server(body: Vec<u8>) -> (String, tokio::sync::mpsc::UnboundedReceiver<Option<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bound");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let (ranges, asked) = tokio::sync::mpsc::unbounded_channel();
        let body = Arc::new(body);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let (ranges, body) = (ranges.clone(), Arc::clone(&body));
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&buffer[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                    let range = head.lines().find_map(|line| line.strip_prefix("range:")).map(|r| r.trim().to_string());
                    let _ = ranges.send(range.clone());
                    let from = range
                        .as_deref()
                        .and_then(|range| range.strip_prefix("bytes="))
                        .and_then(|range| range.trim_end_matches('-').parse::<usize>().ok());
                    match from {
                        Some(from) => {
                            let rest = &body[from..];
                            let answer = format!(
                                "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {from}-{}/{}\r\n\r\n",
                                rest.len(),
                                body.len() - 1,
                                body.len()
                            );
                            let _ = socket.write_all(answer.as_bytes()).await;
                            let _ = socket.write_all(rest).await;
                        }
                        None => {
                            let answer = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len());
                            let _ = socket.write_all(answer.as_bytes()).await;
                            let _ = socket.write_all(&body[..body.len() / 2]).await;
                            let _ = socket.flush().await;
                            // Holds the connection until the client goes.
                            let _ = socket.read(&mut buffer).await;
                        }
                    }
                });
            }
        });
        (base, asked)
    }

    #[tokio::test]
    async fn a_cancelled_download_keeps_its_part_and_the_next_one_goes_on_with_range() {
        let dir = tempfile::tempdir().expect("temp dir");
        let body = large();
        let (base, mut asked) = halving_server(body.clone()).await;
        let models = catalogue(&base, &[("encoder.onnx", &body)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);

        downloads.download("test").expect("started");
        let half = (body.len() / 2) as u64;
        let mut seen = 0;
        for _ in 0..500 {
            seen = downloads.list()[0].completed;
            if seen == half {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(seen, half, "the first half arrived");
        assert_eq!(downloads.download("test"), Err(VoiceModelError::Busy));
        let cancelled = downloads.cancel("test").await.expect("cancelled");
        assert_eq!((cancelled[0].state, cancelled[0].completed), (VoiceModelState::Cancelled, half));
        let part = models[0].folder(dir.path()).join("encoder.onnx.part");
        assert_eq!(std::fs::metadata(&part).expect("kept").len(), half, "what came stays");

        // After a restart the disk says the same.
        let again = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        assert_eq!((again.list()[0].state, again.list()[0].completed), (VoiceModelState::Cancelled, half));
        again.download("test").expect("started again");
        let done = settled(&again).await;
        assert_eq!(done.state, VoiceModelState::Ready, "{:?}", done.error);
        assert_eq!(std::fs::read(models[0].path(dir.path(), "encoder.onnx")).expect("file"), body);
        assert_eq!(asked.recv().await, Some(None), "the first asked for the whole file");
        assert_eq!(asked.recv().await, Some(Some(format!("bytes={half}-"))), "the second for the rest");
    }

    /// Hugging Face sends its files from a CDN: the redirect is followed and `Range` goes along.
    #[tokio::test]
    async fn a_redirect_is_followed_with_its_range() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = MockServer::start_async().await;
        let body = large();
        let have = 1000;
        let redirect = server
            .mock_async(|when, then| {
                when.method("GET").path("/encoder.onnx");
                then.status(302).header("location", server.url("/cdn/encoder.onnx"));
            })
            .await;
        let rest = server
            .mock_async(|when, then| {
                when.method("GET").path("/cdn/encoder.onnx").header("range", format!("bytes={have}-"));
                then.status(206)
                    .header("content-range", format!("bytes {have}-{}/{}", body.len() - 1, body.len()))
                    .body(&body[have..]);
            })
            .await;
        let models = catalogue(&server.base_url(), &[("encoder.onnx", &body)]);
        let folder = models[0].folder(dir.path());
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("encoder.onnx.part"), &body[..have]).expect("part");
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);

        downloads.download("test").expect("started");
        let done = settled(&downloads).await;
        assert_eq!(done.state, VoiceModelState::Ready, "{:?}", done.error);
        assert_eq!((redirect.calls_async().await, rest.calls_async().await), (1, 1));
        assert_eq!(std::fs::read(folder.join("encoder.onnx")).expect("file"), body);
    }

    /// A server that ignores `Range` and sends the whole file: it starts from zero.
    #[tokio::test]
    async fn a_whole_answer_to_a_range_starts_the_file_anew() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/tokens.txt");
                then.status(200).body(SMALL);
            })
            .await;
        let models = catalogue(&server.base_url(), &[("tokens.txt", SMALL)]);
        let folder = models[0].folder(dir.path());
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("tokens.txt.part"), b"tok").expect("part");
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);

        downloads.download("test").expect("started");
        assert_eq!(settled(&downloads).await.state, VoiceModelState::Ready);
        assert_eq!(std::fs::read(folder.join("tokens.txt")).expect("file"), SMALL);
    }

    #[tokio::test]
    async fn a_wrong_hash_fails_and_throws_the_part_away() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/tokens.txt");
                then.status(200).body(b"tokens of the evil model");
            })
            .await;
        let models = catalogue(&server.base_url(), &[("tokens.txt", SMALL)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);

        downloads.download("test").expect("started");
        let failed = settled(&downloads).await;
        assert_eq!((failed.state, failed.error), (VoiceModelState::Failed, Some(damaged())));
        let folder = models[0].folder(dir.path());
        assert!(!folder.join("tokens.txt.part").exists() && !folder.join("tokens.txt").exists());
        assert_eq!(failed.completed, 0);
    }

    #[tokio::test]
    async fn without_a_connection_or_with_an_error_of_the_server_it_fails_in_words_for_people() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A port nothing listens on.
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bound");
        let address = closed.local_addr().expect("address");
        drop(closed);
        let models = catalogue(&format!("http://{address}"), &[("tokens.txt", SMALL)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        downloads.download("test").expect("started");
        let failed = settled(&downloads).await;
        assert_eq!(failed.state, VoiceModelState::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some(tr("No connection to the internet. Check the network and try again."))
        );

        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/tokens.txt");
                then.status(503).body("the server's own words");
            })
            .await;
        let models = catalogue(&server.base_url(), &[("tokens.txt", SMALL)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        downloads.download("test").expect("started");
        let failed = settled(&downloads).await;
        assert_eq!(failed.error.as_deref(), Some(tr("Hugging Face did not deliver the model. Try again later.")));
        // A failed one may be downloaded again.
        assert!(downloads.download("test").is_ok());
    }

    #[tokio::test]
    async fn removing_cancels_a_running_download_and_deletes_the_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let body = large();
        let (base, _asked) = halving_server(body.clone()).await;
        let models = catalogue(&base, &[("encoder.onnx", &body)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        downloads.download("test").expect("started");
        for _ in 0..500 {
            if downloads.list()[0].completed > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let removed = downloads.remove("test").await.expect("removed");
        assert_eq!((removed[0].state, removed[0].completed), (VoiceModelState::Missing, 0));
        assert!(!models[0].folder(dir.path()).exists());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!models[0].folder(dir.path()).exists(), "nothing writes afterwards");
        assert_eq!(downloads.remove("test").await.expect("nothing there")[0].state, VoiceModelState::Missing);
        assert_eq!(downloads.remove("parakeet").await, Err(VoiceModelError::NotFound));
        assert_eq!(downloads.cancel("parakeet").await, Err(VoiceModelError::NotFound));
        assert_eq!(downloads.cancel("test").await.expect("nothing runs")[0].state, VoiceModelState::Missing);
    }

    /// PR review of #762: while a removal deletes the folder, a download of the same model waits with
    /// `busy` instead of losing its files; afterwards it may start.
    #[tokio::test]
    async fn a_download_waits_while_the_model_is_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let models = catalogue("http://127.0.0.1:9", &[("tokens.txt", SMALL)]);
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), models);
        let marked = Entry {
            state: VoiceModelState::Cancelled,
            error: None,
            task: None,
            stopping: None,
            removing: true,
            run: 1,
        };
        downloads.entries().insert("test", marked);
        assert_eq!(downloads.download("test"), Err(VoiceModelError::Busy));
        downloads.entries().clear();
        downloads.remove("test").await.expect("removed");
        assert!(downloads.entries().is_empty(), "the mark goes with the removal");
        assert!(downloads.download("test").is_ok(), "afterwards it starts");
        downloads.cancel("test").await.expect("cancelled");
    }

    #[tokio::test]
    async fn the_list_reads_the_disk_and_without_a_folder_everything_is_missing() {
        let nowhere = VoiceDownloads::new(None, &MODELS);
        let listed = nowhere.list();
        // Qwen3-TTS only in builds with it (issue #764).
        let expected = ["whisper-turbo", "parakeet", "pocket-de", "qwen3-tts"];
        assert_eq!(
            listed.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            expected[..3 + usize::from(torro_core::settings::QWEN_BUILT)]
        );
        assert!(listed.iter().all(|model| model.state == VoiceModelState::Missing && model.completed == 0));
        assert_eq!(listed[0].bytes, MODELS[0].bytes());
        assert!(matches!(nowhere.download("parakeet"), Err(VoiceModelError::Error(_))));
        assert_eq!(VoiceModelError::NotFound.code(), "not-found");

        let dir = tempfile::tempdir().expect("temp dir");
        let downloads = VoiceDownloads::new(Some(dir.path().to_path_buf()), &MODELS);
        assert!(downloads.list().iter().all(|model| model.state == VoiceModelState::Missing), "no folder yet");
        // Parakeet whole, Pocket with a part: as an earlier run left them.
        let parakeet = &MODELS[1];
        std::fs::create_dir_all(parakeet.folder(dir.path())).expect("folder");
        for file in parakeet.files {
            std::fs::File::create(parakeet.path(dir.path(), file.name))
                .expect("file")
                .set_len(file.bytes)
                .expect("size");
        }
        let pocket = &MODELS[2];
        std::fs::create_dir_all(pocket.folder(dir.path())).expect("folder");
        std::fs::write(part_path(&pocket.folder(dir.path()), &pocket.files[0]), [0_u8; 10]).expect("part");
        let listed = downloads.list();
        assert_eq!((listed[1].state, listed[1].completed), (VoiceModelState::Ready, parakeet.bytes()));
        assert_eq!((listed[2].state, listed[2].completed), (VoiceModelState::Cancelled, 10));
        assert_eq!(listed[0].state, VoiceModelState::Missing);
    }
}
