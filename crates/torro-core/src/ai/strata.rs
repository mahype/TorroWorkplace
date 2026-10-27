//! How the suite recognises Strata (issue #322, <https://github.com/Niko1221/Strata>), a local
//! server for large open models. It speaks the OpenAI-compatible protocol ([`super::openai`]), so
//! this is no wire format of its own, only the one thing the protocol cannot tell: whether the
//! server on port 8080 is Strata. llama.cpp's server runs there too, and both answer
//! `GET /v1/models` alike.
//!
//! Strata's `GET /health` says it, and unlike `/v1/*` it is not behind the API key: it answers
//! `{"status": "ok", …, "service": "strata"}`. Pure, without a network; the core sends the request
//! (`torroworkplace::account::ai`).

use serde_json::Value;

use super::openai;

/// The `service` of Strata's `/health` answer.
const SERVICE: &str = "strata";

/// Where Strata's `/health` is for the OpenAI-compatible `base` (`http://127.0.0.1:8080/v1` →
/// `http://127.0.0.1:8080/health`): next to `/v1`, not below it.
pub fn health_url(base: &str) -> String {
    format!("{}/health", openai::ollama_root(base))
}

/// Whether the answer of `/health` comes from Strata: `service` is `strata`. Another server's
/// `{"status": "ok"}` is not enough, so llama.cpp on the same port is not taken for it.
pub fn is_strata(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body).is_ok_and(|health| health["service"] == SERVICE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_next_to_v1() {
        assert_eq!(health_url("http://127.0.0.1:8080/v1"), "http://127.0.0.1:8080/health");
        assert_eq!(health_url("http://127.0.0.1:8080/v1/"), "http://127.0.0.1:8080/health");
        assert_eq!(health_url("http://127.0.0.1:8080"), "http://127.0.0.1:8080/health");
    }

    #[test]
    fn only_a_health_answer_of_strata_counts() {
        let strata = br#"{"status": "ok", "max_context": 32768, "model": "qwen", "loaded": true, "service": "strata"}"#;
        assert!(is_strata(strata));
        assert!(!is_strata(br#"{"status": "ok"}"#), "llama.cpp answers like this");
        assert!(!is_strata(br#"{"status": "ok", "service": "llama"}"#));
        assert!(!is_strata(b"OK"), "not JSON");
        assert!(!is_strata(b""), "nothing");
        assert!(!is_strata(b"[\"strata\"]"), "not an object");
    }
}
