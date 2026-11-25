//! Qwen3-TTS as a program of its own (issue #808): the 0.6B Base model through Crane on candle, on the
//! first NVIDIA card. The reading helper of torroassistant starts it as a child process and talks to it
//! over JSON lines (`crates/torroassistant/src/voice/qwen.rs`); everything else (pieces, voices and
//! their transcripts, the silence before the first word) stays in the assistant. Built apart, in a
//! workspace of its own, so Crane never touches the versions or features of the suite (issue #808).
//!
//! `torro-qwen-tts --card` writes `{"card": "<name>, <n> GB"}`, or `{"card": null}` without a card.
//!
//! `torro-qwen-tts <model folder>` loads the model (with `speech_tokenizer/` inside) and writes
//! `{"ready": {"rate": …, "speaker_rate": …}}`, or `{"error": {"gpu": true|false, "message": …}}` and
//! ends. Then each line it reads gets one line back:
//!
//! - `{"voice": {"id": …, "words": …, "samples": [f32 …]}}`: a template at `speaker_rate` with its
//!   transcript, kept under `id`; answered `{"done": true}`.
//! - `{"speak": {"text": …, "language": …, "voice": …, "most": n}}`: `text` in the voice `id`, at most
//!   `n` frames; answered `{"sound": [f32 …]}` at `rate`.
//!
//! A request that fails is answered `{"error": "…"}`. When its input ends, it ends and its graphics
//! memory is free.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::ExitCode;

use crane_core::generation::SpeechOptions;
use crane_core::models::qwen3_tts::Model;
use crane_core::models::{DType, Device};
use serde_json::{Value, json};

/// The name this program goes by in its messages.
const NAME: &str = "torro-qwen-tts";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--card" => {
            say(&json!({ "card": graphics_card() }));
            ExitCode::SUCCESS
        }
        [folder] if !folder.starts_with('-') => serve(folder),
        _ => {
            eprintln!("usage: {NAME} <model folder> | --card");
            ExitCode::from(2)
        }
    }
}

/// Writes `line` to the reading helper; `false` when it no longer reads.
fn say(line: &Value) -> bool {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}").and_then(|()| out.flush()).is_ok()
}

/// The NVIDIA card it computes on, as the settings show it (`NVIDIA GeForce RTX 4090, 24 GB`); `None`
/// when there is none, or its libraries do not answer.
#[cfg(feature = "cuda")]
fn graphics_card() -> Option<String> {
    let Ok(Device::Cuda(device)) = Device::new_cuda(0) else { return None };
    let stream = device.cuda_stream();
    let context = stream.context();
    let name = context.name().ok()?;
    let gigabytes = context.total_mem().map(|bytes| bytes.div_ceil(1 << 30)).unwrap_or_default();
    Some(if gigabytes > 0 { format!("{name}, {gigabytes} GB") } else { name })
}

/// Without CUDA there is no card to compute on.
#[cfg(not(feature = "cuda"))]
fn graphics_card() -> Option<String> {
    None
}

/// Loads the model from `folder` onto the first NVIDIA card and answers the reading helper until its
/// input ends.
fn serve(folder: &str) -> ExitCode {
    let device = match Device::new_cuda(0) {
        Ok(device) => device,
        Err(err) => {
            eprintln!("{NAME}: no graphics card: {err}");
            say(&json!({ "error": { "gpu": true, "message": err.to_string() } }));
            return ExitCode::FAILURE;
        }
    };
    let mut model = match Model::new(folder, &device, &DType::BF16) {
        Ok(model) => model,
        Err(err) => {
            say(&json!({ "error": { "gpu": false, "message": format!("did not load: {err}") } }));
            return ExitCode::FAILURE;
        }
    };
    let ready =
        json!({ "ready": { "rate": model.sample_rate(), "speaker_rate": model.speaker_encoder_sample_rate() } });
    if !say(&ready) {
        return ExitCode::SUCCESS;
    }
    let mut voices: HashMap<String, (Vec<f32>, String)> = HashMap::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = request(&line).and_then(|request| answer(&mut model, &mut voices, request));
        let line = answer.unwrap_or_else(|message| json!({ "error": message }));
        if !say(&line) {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// What the reading helper asks.
#[derive(Debug, PartialEq)]
enum Request {
    /// A template and its transcript, kept under `id`.
    Voice { id: String, words: String, samples: Vec<f32> },
    /// `text` in the voice `voice`, in `language` as Qwen3-TTS names it, at most `most` frames.
    Speak { text: String, language: String, voice: String, most: usize },
}

/// The request on `line`, or why it is none.
fn request(line: &str) -> Result<Request, String> {
    let value: Value = serde_json::from_str(line).map_err(|err| format!("not a JSON line: {err}"))?;
    let text = |object: &Value, key: &str| {
        object.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("no `{key}`"))
    };
    if let Some(voice) = value.get("voice") {
        let samples = voice
            .get("samples")
            .and_then(Value::as_array)
            .ok_or("no `samples`")?
            .iter()
            // Samples are in [-1, 1]; an f32 holds what the helper wrote.
            .map(|sample| sample.as_f64().map(|sample| sample as f32).ok_or("a sample is no number"))
            .collect::<Result<Vec<f32>, _>>()?;
        return Ok(Request::Voice { id: text(voice, "id")?, words: text(voice, "words")?, samples });
    }
    if let Some(speak) = value.get("speak") {
        let most = speak.get("most").and_then(Value::as_u64).ok_or("no `most`")?;
        return Ok(Request::Speak {
            text: text(speak, "text")?,
            language: text(speak, "language")?,
            voice: text(speak, "voice")?,
            most: usize::try_from(most).map_err(|err| err.to_string())?,
        });
    }
    Err("neither `voice` nor `speak`".to_string())
}

/// Answers `request` with the loaded `model` and the templates in `voices`.
fn answer(
    model: &mut Model,
    voices: &mut HashMap<String, (Vec<f32>, String)>,
    request: Request,
) -> Result<Value, String> {
    match request {
        Request::Voice { id, words, samples } => {
            voices.insert(id, (samples, words));
            Ok(json!({ "done": true }))
        }
        Request::Speak { text, language, voice, most } => {
            let (reference, words) = voices.get(&voice).ok_or_else(|| format!("no voice `{voice}`"))?;
            // The settings of the measurement, which are those of Crane and of Qwen's own examples.
            let options = SpeechOptions {
                max_new_tokens: most,
                temperature: 0.9,
                top_p: Some(1.0),
                repetition_penalty: 1.05,
                ..SpeechOptions::default()
            };
            let (audio, _) = model
                .generate_voice_clone(&text, &language, reference, words, &options)
                .map_err(|err| format!("made no sound: {err}"))?;
            let samples: Vec<f32> = audio
                .to_dtype(DType::F32)
                .and_then(|audio| audio.flatten_all())
                .and_then(|audio| audio.to_vec1())
                .map_err(|err| format!("made no sound: {err}"))?;
            Ok(json!({ "sound": samples }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_requests() {
        assert_eq!(
            request(r#"{"voice": {"id": "calm_man", "words": "Guten Tag.", "samples": [0.5, -0.25, 0]}}"#),
            Ok(Request::Voice {
                id: "calm_man".to_string(),
                words: "Guten Tag.".to_string(),
                samples: vec![0.5, -0.25, 0.0]
            })
        );
        assert_eq!(
            request(r#"{"speak": {"text": "Hallo.", "language": "german", "voice": "calm_man", "most": 60}}"#),
            Ok(Request::Speak {
                text: "Hallo.".to_string(),
                language: "german".to_string(),
                voice: "calm_man".to_string(),
                most: 60
            })
        );
    }

    #[test]
    fn says_what_is_wrong_with_a_line() {
        assert!(request("nonsense").is_err_and(|err| err.starts_with("not a JSON line")));
        assert_eq!(request(r#"{"speak": {"text": "Hallo."}}"#), Err("no `most`".to_string()));
        assert_eq!(
            request(r#"{"voice": {"id": "x", "words": "y", "samples": ["a"]}}"#),
            Err("a sample is no number".to_string())
        );
        assert_eq!(request("{}"), Err("neither `voice` nor `speak`".to_string()));
    }
}
