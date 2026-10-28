//! Downloading a model into Ollama (issue #325, task MW.7): the wire format of `POST <root>/api/pull`,
//! pure and without a network; the core sends it and keeps the downloads
//! (`torroworkplace::account::pull`).
//!
//! Ollama answers a pull as NDJSON, one JSON object per line: `{"status": "pulling manifest"}`, then one
//! line per piece of a layer (`{"status": "pulling …", "digest": "sha256:…", "total": 2142590208,
//! "completed": 241970}`), and `{"status": "success"}` at the end – or `{"error": "…"}` when it fails. A
//! model has several layers (weights, template, licence …), each counted on its own, so the progress of
//! the whole download is the sum over the layers seen so far ([`Progress`]).
//!
//! The server's `error` never leaves here: it may name hosts, paths or the registry's own words, and
//! people get a sentence of the suite instead ([`server_failed`]).

use serde::Deserialize;
use serde_json::{Value, json};

use super::openai;
use crate::i18n::tr;

/// Where Ollama takes a pull for the OpenAI-compatible `base` (`http://localhost:11434/v1` →
/// `http://localhost:11434/api/pull`): its own API, next to `/v1`.
pub fn url(base: &str) -> String {
    format!("{}/api/pull", openai::ollama_root(base))
}

/// The body of `POST <root>/api/pull` for `model`, streamed: Ollama reports its progress line by line.
pub fn body(model: &str) -> Value {
    json!({ "model": model, "stream": true })
}

/// What one line of the stream says ([`read_line`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullEvent {
    /// The layer this line counts (`sha256:…`); `None` for lines about the whole model (manifest,
    /// verifying, success).
    pub digest: Option<String>,
    /// Bytes of that layer loaded so far.
    pub completed: Option<u64>,
    /// Bytes of that layer in all.
    pub total: Option<u64>,
    /// The download is complete (`"status": "success"`).
    pub done: bool,
    /// The download failed, in the suite's words ([`server_failed`] or [`unreadable`]), never the
    /// server's.
    pub error: Option<String>,
}

/// One line as Ollama writes it; everything else in it is ignored.
#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    error: Option<Value>,
}

/// Reads one line of the stream. `None` for an empty line. A line with `error` – whatever it says – and a
/// line that is no JSON object of the stream become a [`PullEvent`] with the suite's own sentence.
pub fn read_line(line: &[u8]) -> Option<PullEvent> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let Ok(line) = serde_json::from_str::<Line>(text) else {
        return Some(PullEvent { error: Some(unreadable()), ..PullEvent::default() });
    };
    if line.error.is_some() {
        return Some(PullEvent { error: Some(server_failed()), ..PullEvent::default() });
    }
    Some(PullEvent {
        done: line.status.as_deref() == Some("success"),
        digest: line.digest.filter(|digest| !digest.is_empty()),
        total: line.total,
        completed: line.completed,
        error: None,
    })
}

/// Why a download failed when Ollama said it did: one sentence for every reason it gives (the registry
/// does not answer, the disk is full, the model is gone), since its own words are not passed on.
pub fn server_failed() -> String {
    tr("Ollama could not download the model. Check the internet connection and the free disk space, then try again.")
        .to_string()
}

/// Why a download failed when Ollama's answer could not be read.
pub fn unreadable() -> String {
    tr("Ollama's answer to the download could not be read. Try again.").to_string()
}

/// Splits the bytes of the stream into its lines and reads each ([`read_line`]); pieces of a line wait for
/// the rest of it.
#[derive(Debug, Default)]
pub struct Decoder {
    /// Bytes after the last line break.
    pending: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// The events of every line `bytes` completes, in order.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<PullEvent> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            events.extend(read_line(&line));
        }
        events
    }

    /// The event of a last line without a line break, once the stream has ended.
    pub fn finish(self) -> Option<PullEvent> {
        read_line(&self.pending)
    }
}

/// The progress of the whole download: the bytes of every layer seen so far, added up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    /// Per layer, in the order they came: digest, completed, total.
    layers: Vec<(String, u64, u64)>,
}

impl Progress {
    /// Counts what `event` says of its layer; lines without a layer or a total change nothing.
    pub fn add(&mut self, event: &PullEvent) {
        let (Some(digest), Some(total)) = (&event.digest, event.total) else { return };
        let completed = event.completed.unwrap_or(0).min(total);
        match self.layers.iter_mut().find(|layer| layer.0 == *digest) {
            Some(layer) => {
                layer.1 = completed.max(layer.1);
                layer.2 = total;
            }
            None => self.layers.push((digest.clone(), completed, total)),
        }
    }

    /// Bytes loaded so far; `None` before Ollama named a layer.
    pub fn completed(&self) -> Option<u64> {
        (!self.layers.is_empty()).then(|| self.layers.iter().map(|layer| layer.1).sum())
    }

    /// Bytes in all of the layers seen so far; `None` before Ollama named a layer.
    pub fn total(&self) -> Option<u64> {
        (!self.layers.is_empty()).then(|| self.layers.iter().map(|layer| layer.2).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pull_goes_to_ollamas_own_api_and_streams() {
        assert_eq!(url("http://localhost:11434/v1"), "http://localhost:11434/api/pull");
        assert_eq!(url("http://localhost:11434/v1/"), "http://localhost:11434/api/pull");
        assert_eq!(url("http://localhost:11434"), "http://localhost:11434/api/pull");
        assert_eq!(body("qwen3:14b"), json!({"model": "qwen3:14b", "stream": true}));
    }

    #[test]
    fn lines_say_their_layer_progress_and_the_end() {
        assert_eq!(read_line(b"{\"status\":\"pulling manifest\"}\n"), Some(PullEvent::default()));
        let layer = br#"{"status":"pulling a1b2","digest":"sha256:a1b2","total":2000,"completed":500}"#;
        assert_eq!(
            read_line(layer),
            Some(PullEvent {
                digest: Some("sha256:a1b2".into()),
                completed: Some(500),
                total: Some(2000),
                ..PullEvent::default()
            })
        );
        assert_eq!(read_line(br#"{"status":"success"}"#), Some(PullEvent { done: true, ..PullEvent::default() }));
        assert_eq!(read_line(b"  \r\n"), None, "an empty line says nothing");
    }

    #[test]
    fn the_servers_error_is_never_passed_on() {
        let failed = read_line(br#"{"error":"pull model manifest: Get \"https://evil.example/v2\": no such host"}"#)
            .expect("an event");
        assert_eq!(failed.error, Some(server_failed()));
        assert!(!failed.done);
        assert!(!server_failed().contains("evil"));
        let odd = read_line(b"<html>proxy error</html>").expect("an event");
        assert_eq!(odd.error, Some(unreadable()));
    }

    #[test]
    fn the_decoder_waits_for_the_rest_of_a_line() {
        let mut decoder = Decoder::new();
        assert_eq!(decoder.push(br#"{"status":"pulling a","digest":"a","tot"#), []);
        let events = decoder.push(b"al\":10,\"completed\":4}\n{\"status\":\"verifying\"}\n{\"status\":\"succ");
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].completed, events[0].total), (Some(4), Some(10)));
        assert_eq!(decoder.push(b"ess\"}"), []);
        assert_eq!(decoder.finish(), Some(PullEvent { done: true, ..PullEvent::default() }));
    }

    #[test]
    fn progress_adds_up_the_layers() {
        let mut progress = Progress::default();
        assert_eq!((progress.completed(), progress.total()), (None, None));
        let layer = |digest: &str, completed, total| PullEvent {
            digest: Some(digest.into()),
            completed: Some(completed),
            total: Some(total),
            ..PullEvent::default()
        };
        progress.add(&PullEvent::default());
        assert_eq!(progress.total(), None, "the manifest names no layer");
        progress.add(&layer("weights", 100, 1000));
        progress.add(&layer("licence", 10, 10));
        assert_eq!((progress.completed(), progress.total()), (Some(110), Some(1010)));
        progress.add(&layer("weights", 600, 1000));
        assert_eq!((progress.completed(), progress.total()), (Some(610), Some(1010)));
        progress.add(&layer("weights", 2000, 1000));
        assert_eq!(progress.completed(), Some(1010), "never more than the layer holds");
    }
}
