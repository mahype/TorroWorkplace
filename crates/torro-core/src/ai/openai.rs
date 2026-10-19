//! The wire format of the OpenAI-compatible protocol (task KI.1a): what Ollama, LM Studio, vLLM,
//! llama.cpp and OpenRouter take and send at `<base>/chat/completions` and `<base>/models`, plus
//! Ollama's `/api/show` and `/api/tags`. Pure functions over bytes and JSON, without a network, so every rule is
//! tested here; the core sends them (`torroworkplace::account::ai::OpenAiCompatible`).
//!
//! A streamed answer comes as server-sent events, one JSON chunk per `data:` line and `[DONE]` at
//! the end ([`StreamDecoder`]). Text arrives in pieces; a tool call arrives in pieces too (name
//! first, then its arguments bit by bit, keyed by `index`), and is handed out whole.
//!
//! Servers' error texts can repeat what was sent, an API key included. [`scrub`] drops such a
//! text and only then shortens it, so no cut leaves a piece of a key behind; the core passes every
//! error through it.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{AiError, AiModel, AiProvider, ChatMessage, ChatReply, ChatRequest, Finish, Role, ToolCall, Usage};
use crate::i18n::{tr, trf};

/// The longest server text an error keeps: enough for "model 'x' not found", never a whole page.
const MAX_MESSAGE: usize = 300;

/// The body of `POST <base>/chat/completions` for `request`: model, messages, the tools as
/// `function` tools (left out when there are none), `max_tokens` when set, and `stream`.
pub fn chat_body(request: &ChatRequest, stream: bool) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(message_json).collect();
    let mut body = json!({ "model": request.model, "messages": messages, "stream": stream });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": { "name": tool.name, "description": tool.description, "parameters": tool.parameters },
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    if let Some(max) = request.max_tokens {
        body["max_tokens"] = json!(max);
    }
    body
}

/// [`chat_body`] as the server of `provider` takes it: with the request's [`ChatRequest::effort`]
/// where each one reads it (issue #131) – OpenRouter as `"reasoning": {"effort": "high"}`, OpenAI,
/// Gemini and the servers of one's own (Ollama, LM Studio, vLLM …) as OpenAI's `"reasoning_effort":
/// "high"`, which Ollama's `/v1` maps onto its thinking; the other cloud providers (task W.3) get
/// none. Without an effort the body is [`chat_body`]'s.
pub fn chat_body_for(provider: AiProvider, request: &ChatRequest, stream: bool) -> Value {
    let mut body = chat_body(request, stream);
    if let Some(effort) = request.effort {
        match provider {
            AiProvider::OpenRouter => body["reasoning"] = json!({ "effort": effort.id() }),
            // Gemini maps OpenAI's `reasoning_effort` onto its thinking budget.
            AiProvider::Ollama
            | AiProvider::LmStudio
            | AiProvider::OpenAiCompatible
            | AiProvider::OpenAi
            | AiProvider::Gemini => {
                body["reasoning_effort"] = json!(effort.id());
            }
            // Anthropic speaks its own protocol and never comes through here; its effort is
            // `output_config.effort` in `super::anthropic::chat_body`.
            AiProvider::Anthropic => {}
            // Not documented for their OpenAI-compatible APIs, and an unknown field may be refused:
            // these think as their models do.
            AiProvider::Mistral
            | AiProvider::DeepSeek
            | AiProvider::Qwen
            | AiProvider::Kimi
            | AiProvider::Glm
            | AiProvider::MiniMax => {}
        }
    }
    body
}

/// One message as the protocol writes it. An assistant message that only calls tools has
/// `content: null`, as OpenAI's own servers expect.
fn message_json(message: &ChatMessage) -> Value {
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut json = json!({ "role": role, "content": message.content });
    if !message.tool_calls.is_empty() {
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({ "id": call.id, "type": "function", "function": { "name": call.name, "arguments": call.arguments } })
            })
            .collect();
        json["tool_calls"] = Value::Array(calls);
        if message.content.is_empty() {
            json["content"] = Value::Null;
        }
    }
    if let Some(id) = &message.tool_call_id {
        json["tool_call_id"] = json!(id);
    }
    json
}

/// A whole answer (`stream: false`, or a server that ignored `stream: true`): the first choice's
/// text, tool calls and finish reason, and the usage.
pub fn parse_reply(body: &[u8]) -> Result<ChatReply, AiError> {
    let answer: Value = serde_json::from_slice(body).map_err(|_| not_json())?;
    if answer.get("error").is_some_and(|error| !error.is_null()) {
        return Err(error_of(500, body));
    }
    let message = answer.pointer("/choices/0/message").ok_or_else(|| format_error(tr("The answer has no message.")))?;
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(at, call)| ToolCall {
                    id: text_of(call.get("id")).filter(|id| !id.is_empty()).unwrap_or_else(|| made_up_id(at)),
                    name: text_of(call.pointer("/function/name")).unwrap_or_default(),
                    arguments: arguments_of(call.pointer("/function/arguments")),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatReply {
        text: text_of(message.get("content")).unwrap_or_default(),
        tool_calls,
        finish: finish_of(answer.pointer("/choices/0/finish_reason")).unwrap_or_default(),
        usage: usage_of(answer.get("usage")),
        // This protocol asks for nothing back; thinking, where a server reports it, is its own
        // field of the answer and not a block that has to travel again.
        thinking: Vec::new(),
    })
}

/// A tool call being put together from a stream.
#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Puts a streamed answer together: feed it the body's bytes as they come ([`StreamDecoder::push`],
/// in any pieces, also cut in the middle of a line or of a UTF-8 character), then take the reply
/// ([`StreamDecoder::finish`]).
///
/// Lines other than `data:` (comments such as OpenRouter's `: OPENROUTER PROCESSING`, `event:`,
/// empty ones) are skipped; `data: [DONE]` ends the answer and anything after it is ignored.
#[derive(Debug, Default)]
pub struct StreamDecoder {
    /// Bytes after the last line break, waiting for the rest of their line.
    pending: Vec<u8>,
    text: String,
    calls: Vec<PartialCall>,
    finish: Option<Finish>,
    usage: Option<Usage>,
    /// At least one chunk was read.
    started: bool,
    done: bool,
}

impl StreamDecoder {
    pub fn new() -> StreamDecoder {
        StreamDecoder::default()
    }

    /// Reads `bytes` of the stream; every new piece of text goes to `on_text`. An error event of
    /// the server (`data: {"error": …}`) ends the answer with that error.
    pub fn push(&mut self, bytes: &[u8], on_text: &mut dyn FnMut(&str)) -> Result<(), AiError> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.line(&line, on_text)?;
        }
        Ok(())
    }

    /// The answer, once the stream has ended. A last line without a line break counts. An answer
    /// without any chunk is a [`AiError::Format`].
    pub fn finish(mut self, on_text: &mut dyn FnMut(&str)) -> Result<ChatReply, AiError> {
        let rest = std::mem::take(&mut self.pending);
        self.line(&rest, on_text)?;
        if !self.started {
            return Err(format_error(tr("The answer was empty.")));
        }
        let tool_calls: Vec<ToolCall> = self
            .calls
            .into_iter()
            .enumerate()
            .map(|(at, call)| ToolCall {
                id: if call.id.is_empty() { made_up_id(at) } else { call.id },
                name: call.name,
                arguments: call.arguments,
            })
            .collect();
        Ok(ChatReply {
            text: self.text,
            tool_calls,
            finish: self.finish.unwrap_or_default(),
            usage: self.usage,
            thinking: Vec::new(),
        })
    }

    fn line(&mut self, line: &[u8], on_text: &mut dyn FnMut(&str)) -> Result<(), AiError> {
        if self.done {
            return Ok(());
        }
        let line = String::from_utf8_lossy(line);
        let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") else { return Ok(()) };
        let data = data.trim();
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        if data.is_empty() {
            return Ok(());
        }
        let chunk: Value = serde_json::from_str(data).map_err(|_| not_json())?;
        if chunk.get("error").is_some_and(|error| !error.is_null()) {
            let status = chunk.pointer("/error/code").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok());
            return Err(error_of(status.unwrap_or(500), data.as_bytes()));
        }
        self.started = true;
        if let Some(usage) = usage_of(chunk.get("usage")) {
            self.usage = Some(usage);
        }
        let Some(choice) = chunk.pointer("/choices/0") else { return Ok(()) };
        if let Some(piece) = choice.pointer("/delta/content").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            self.text.push_str(piece);
            on_text(piece);
        }
        if let Some(calls) = choice.pointer("/delta/tool_calls").and_then(Value::as_array) {
            for (at, call) in calls.iter().enumerate() {
                self.call_piece(at, call);
            }
        }
        if let Some(finish) = finish_of(choice.get("finish_reason")) {
            self.finish = Some(finish);
        }
        Ok(())
    }

    /// One piece of a tool call, the `at`-th of its chunk. Its `index` says which call it belongs
    /// to; a server that sends none (each call whole in one chunk) gets one call per new id.
    fn call_piece(&mut self, at: usize, piece: &Value) {
        let id = text_of(piece.get("id")).filter(|id| !id.is_empty());
        let index = match piece.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()) {
            Some(index) => index,
            None if id.as_ref().is_some_and(|id| !self.calls.iter().any(|call| call.id == *id)) => self.calls.len(),
            None if self.calls.is_empty() => at,
            None => self.calls.len() - 1,
        };
        while self.calls.len() <= index {
            self.calls.push(PartialCall::default());
        }
        let call = &mut self.calls[index];
        if let Some(id) = id {
            call.id = id;
        }
        if let Some(name) = text_of(piece.pointer("/function/name")) {
            call.name.push_str(&name);
        }
        match piece.pointer("/function/arguments") {
            Some(Value::String(part)) => call.arguments.push_str(part),
            // Ollama has sent the arguments as an object, whole.
            Some(value @ Value::Object(_)) => call.arguments = value.to_string(),
            _ => {}
        }
    }
}

/// The models of a `GET <base>/models` answer (`{"data": [{"id": …}, …]}`), in its order, with
/// what the answer tells about each:
///
/// - `name`, else the id;
/// - `context_length` as [`AiModel::context`];
/// - `supported_parameters` (OpenRouter): [`AiModel::tools`] is whether it holds `tools`,
///   [`AiModel::reasoning`] whether it holds `reasoning`;
/// - `hugging_face_id` (OpenRouter): [`AiModel::open_weights`] is whether it is a non-empty
///   text; a model without the field (any other server) leaves it unknown.
pub fn parse_models(body: &[u8]) -> Result<Vec<AiModel>, AiError> {
    let answer: Value = serde_json::from_slice(body).map_err(|_| not_json())?;
    let list = answer
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| format_error(tr("The answer has no model list.")))?;
    Ok(list
        .iter()
        .filter_map(|model| {
            let id = text_of(model.get("id")).filter(|id| !id.is_empty())?;
            let params = model.get("supported_parameters").and_then(Value::as_array);
            let supports = |name: &str| params.map(|params| params.iter().any(|param| param.as_str() == Some(name)));
            let (tools, reasoning) = (supports("tools"), supports("reasoning"));
            let open_weights = model
                .as_object()
                .filter(|m| m.contains_key("hugging_face_id"))
                .map(|m| m.get("hugging_face_id").and_then(Value::as_str).is_some_and(|hf| !hf.trim().is_empty()));
            Some(AiModel {
                name: text_of(model.get("name")).filter(|name| !name.is_empty()).unwrap_or_else(|| id.clone()),
                id,
                tools,
                open_weights,
                context: model.get("context_length").and_then(Value::as_u64),
                reasoning,
                remote: None,
                // Only Ollama says whether a model decides, and it says so per model in
                // `/api/show`, not in the list (task E12.3).
                decision: None,
                enabled: false,
            })
        })
        .collect())
}

/// The address of Ollama's own API for the OpenAI-compatible `base`: without its `/v1`
/// (`http://localhost:11434/v1` → `http://localhost:11434`).
pub fn ollama_root(base: &str) -> &str {
    let base = base.trim_end_matches('/');
    base.strip_suffix("/v1").unwrap_or(base)
}

/// The body of Ollama's `POST <root>/api/show` for `model`.
pub fn ollama_show_body(model: &str) -> Value {
    json!({ "model": model })
}

/// Whether Ollama's `/api/show` answer lists the capability `tools`; `None` for an answer without
/// `capabilities` (an Ollama older than 0.6).
pub fn ollama_tools(body: &[u8]) -> Option<bool> {
    ollama_capability(body, "tools")
}

/// Whether Ollama's `/api/show` answer lists the capability `thinking` (issue #131): the model takes
/// an effort. `None` for an answer without `capabilities`.
pub fn ollama_thinking(body: &[u8]) -> Option<bool> {
    ollama_capability(body, "thinking")
}

/// Whether Ollama's `/api/show` answer lists the capability `decision` (task E12.3, Ollama 0.35
/// and newer): the model decides instead of writing and is asked at `<base>/systemone`
/// ([`super::systemone`]). `None` for an answer without `capabilities`.
pub fn ollama_decision(body: &[u8]) -> Option<bool> {
    ollama_capability(body, "decision")
}

fn ollama_capability(body: &[u8], name: &str) -> Option<bool> {
    let answer: Value = serde_json::from_slice(body).ok()?;
    let capabilities = answer.get("capabilities")?.as_array()?;
    Some(capabilities.iter().any(|capability| capability.as_str() == Some(name)))
}

/// Which of Ollama's models run elsewhere, from its `GET <root>/api/tags` answer (task S.23): each
/// model's `name` and `model` with `true` where it has a `remote_host` (a cloud model, run on
/// `ollama.com` through the local Ollama), `false` where it has none. `None` for an answer that is
/// no model list.
pub fn ollama_remote(body: &[u8]) -> Option<HashMap<String, bool>> {
    let answer: Value = serde_json::from_slice(body).ok()?;
    let mut remote = HashMap::new();
    for model in answer.get("models")?.as_array()? {
        let elsewhere = model.get("remote_host").and_then(Value::as_str).is_some_and(|host| !host.trim().is_empty());
        for key in ["name", "model"] {
            if let Some(id) = text_of(model.get(key)).filter(|id| !id.is_empty()) {
                remote.insert(id, elsewhere);
            }
        }
    }
    Some(remote)
}

/// Whether the name of an Ollama model says it is a cloud model: it ends in `:cloud`
/// (`kimi-k2.6:cloud`) or `-cloud` (`gpt-oss:120b-cloud`).
pub fn ollama_cloud_name(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.ends_with(":cloud") || id.ends_with("-cloud")
}

/// Whether the Ollama model `id` runs elsewhere, given what `/api/tags` said (`tags`,
/// [`ollama_remote`]; `None` when it did not answer): a cloud name always does, else what the
/// tags say. Without tags, or for a model they do not list, it counts as elsewhere: in doubt mail
/// stays home, and a cloud model copied with `ollama cp` no longer carries its suffix.
pub fn ollama_runs_elsewhere(id: &str, tags: Option<&HashMap<String, bool>>) -> bool {
    ollama_cloud_name(id) || tags.is_none_or(|tags| tags.get(id).copied().unwrap_or(true))
}

/// The error of an answer with HTTP `status` and `body`: 401 and 403 are [`AiError::Auth`] (never
/// with the server's words), 404 [`AiError::NotFound`], 402, 429 and 529 [`AiError::Busy`], everything
/// else [`AiError::Server`] with the server's message on one line. The message is read from
/// `{"error": {"message": …}}`, `{"error": "…"}` or `{"message": …}`, else the body as text. It is
/// whole: always pass the result through [`scrub`], which checks it for the key and then shortens
/// it to [`MAX_MESSAGE`] characters.
///
/// Reads no field that belongs to one protocol, so [`super::anthropic`] uses it as well: its error
/// bodies are `{"type": "error", "error": {"type": …, "message": …}}`.
pub fn error_of(status: u16, body: &[u8]) -> AiError {
    let message = message_of(body);
    match status {
        401 | 403 => AiError::Auth,
        402 => AiError::Busy(tr("The credit of this AI account is used up.").into()),
        // 529 is Anthropic's "overloaded", which is a server that is busy and no fault of the
        // request; the trait's contract puts it with 429.
        429 | 529 => AiError::Busy(tr("The AI server is busy or limits requests. Try again later.").into()),
        404 if message.is_empty() => {
            AiError::NotFound(tr("The AI server knows no such address or model. Check the address.").into())
        }
        404 => {
            AiError::NotFound(trf("The AI server knows no such address or model: {message}", &[("message", &message)]))
        }
        _ => AiError::Server { status, message },
    }
}

/// `error` without any server text that holds part of `key` (eight characters in a row or more):
/// some servers repeat a key they refuse ("Incorrect API key: sk-or-…"). Then every server text
/// is shortened to [`MAX_MESSAGE`] characters – after the check, so a cut never hides a piece of
/// the key from it. Without a key only the shortening happens.
///
/// Belongs to no protocol: [`super::anthropic`] passes its errors through this too, and every
/// implementation of [`super::Provider`] must.
pub fn scrub(error: AiError, key: Option<&str>) -> AiError {
    let error = without_key(error, key);
    match error {
        AiError::Server { status, message } => AiError::Server { status, message: shortened(message) },
        AiError::NotFound(message) => AiError::NotFound(shortened(message)),
        AiError::Format(message) => AiError::Format(shortened(message)),
        error => error,
    }
}

/// [`scrub`]'s first half: server texts that hold part of `key` dropped.
fn without_key(error: AiError, key: Option<&str>) -> AiError {
    let Some(key) = key.filter(|key| !key.is_empty()) else { return error };
    let leaks = |message: &str| {
        let chars: Vec<char> = key.chars().collect();
        let window = chars.len().min(8);
        chars.windows(window).any(|part| message.contains(&part.iter().collect::<String>()))
    };
    match error {
        AiError::Server { status, message } if leaks(&message) => AiError::Server { status, message: String::new() },
        AiError::NotFound(message) | AiError::Format(message) if leaks(&message) => {
            AiError::Format(tr("The AI server's answer could not be shown.").into())
        }
        error => error,
    }
}

/// `message` cut to [`MAX_MESSAGE`] characters, with `…` where it was cut.
fn shortened(message: String) -> String {
    match message.char_indices().nth(MAX_MESSAGE) {
        Some((cut, _)) => format!("{}…", &message[..cut]),
        None => message,
    }
}

/// The server's message in an error body, on one line, whole ([`scrub`] shortens it).
fn message_of(body: &[u8]) -> String {
    let text = match serde_json::from_slice::<Value>(body) {
        Ok(json) => [json.pointer("/error/message"), json.get("error"), json.get("message"), json.get("detail")]
            .into_iter()
            .flatten()
            .find_map(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text_of(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// Arguments as text: the protocol sends a JSON text; a server that sends an object gets it
/// written as one.
fn arguments_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn finish_of(value: Option<&Value>) -> Option<Finish> {
    Some(match value?.as_str()? {
        "stop" => Finish::Stop,
        "tool_calls" | "function_call" => Finish::ToolCalls,
        "length" => Finish::Length,
        _ => Finish::Other,
    })
}

fn usage_of(value: Option<&Value>) -> Option<Usage> {
    let usage = value.filter(|usage| usage.is_object())?;
    Some(Usage {
        prompt_tokens: usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
        completion_tokens: usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// The id of a call whose server sent none.
fn made_up_id(at: usize) -> String {
    format!("call_{at}")
}

fn not_json() -> AiError {
    format_error(tr("The AI server sent an answer that is not JSON."))
}

fn format_error(message: &str) -> AiError {
    AiError::Format(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ToolSpec;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "qwen3:8b".into(),
            messages: vec![
                ChatMessage::system("You help."),
                ChatMessage::user("What is on today?"),
                ChatMessage::assistant(
                    "",
                    vec![ToolCall { id: "c1".into(), name: "calendar_list_events".into(), arguments: "{}".into() }],
                ),
                ChatMessage::tool("c1", "[]"),
            ],
            tools: vec![ToolSpec {
                name: "calendar_list_events".into(),
                description: "Lists events.".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
            max_tokens: Some(512),
            effort: None,
        }
    }

    /// Issue #131: each provider gets the effort where it reads it, and nothing without one.
    #[test]
    fn the_effort_goes_where_each_provider_reads_it() {
        let asked = ChatRequest { effort: Some(crate::ai::Effort::High), ..request() };
        let openrouter = chat_body_for(AiProvider::OpenRouter, &asked, true);
        assert_eq!(openrouter["reasoning"], json!({"effort": "high"}));
        assert!(openrouter.get("reasoning_effort").is_none());
        let reads = [
            AiProvider::Ollama,
            AiProvider::LmStudio,
            AiProvider::OpenAiCompatible,
            AiProvider::OpenAi,
            AiProvider::Gemini,
        ];
        for provider in reads {
            let body = chat_body_for(provider, &asked, true);
            assert_eq!(body["reasoning_effort"], "high", "{provider:?}");
            assert!(body.get("reasoning").is_none(), "{provider:?}");
        }
        for provider in [AiProvider::Mistral, AiProvider::DeepSeek, AiProvider::Kimi] {
            assert_eq!(chat_body_for(provider, &asked, true), chat_body(&asked, true), "{provider:?}: nothing extra");
        }
        for provider in AiProvider::ALL {
            assert_eq!(chat_body_for(provider, &request(), true), chat_body(&request(), true), "{provider:?}");
        }
    }

    #[test]
    fn the_body_carries_messages_tools_and_calls_as_the_protocol_says() {
        let body = chat_body(&request(), true);
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["messages"][0], json!({"role": "system", "content": "You help."}));
        assert_eq!(body["messages"][2]["content"], Value::Null, "only calls: content null");
        assert_eq!(body["messages"][2]["tool_calls"][0]["function"]["name"], "calendar_list_events");
        assert_eq!(body["messages"][2]["tool_calls"][0]["type"], "function");
        assert_eq!(body["messages"][3], json!({"role": "tool", "content": "[]", "tool_call_id": "c1"}));
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");

        let plain = chat_body(&ChatRequest { tools: Vec::new(), max_tokens: None, ..request() }, false);
        assert!(plain.get("tools").is_none() && plain.get("max_tokens").is_none(), "{plain}");
    }

    #[test]
    fn a_whole_answer_is_read_with_its_calls_and_usage() {
        let body = br#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
            {"id":"call_9","type":"function","function":{"name":"calendar_list_events","arguments":"{\"start\":\"today\"}"}},
            {"type":"function","function":{"name":"tasks_list_todos","arguments":{"list":"x"}}}]},
            "finish_reason":"tool_calls"}],"usage":{"prompt_tokens":12,"completion_tokens":7}}"#;
        let reply = parse_reply(body).expect("reply");
        assert_eq!(reply.text, "");
        assert_eq!(reply.finish, Finish::ToolCalls);
        assert_eq!(reply.usage, Some(Usage { prompt_tokens: 12, completion_tokens: 7 }));
        assert_eq!(reply.tool_calls[0].arguments, r#"{"start":"today"}"#);
        assert_eq!(
            (reply.tool_calls[1].id.as_str(), reply.tool_calls[1].arguments.as_str()),
            ("call_1", r#"{"list":"x"}"#)
        );
        assert!(matches!(parse_reply(b"<html>"), Err(AiError::Format(_))));
        assert!(matches!(parse_reply(br#"{"choices":[]}"#), Err(AiError::Format(_))));
    }

    /// A stream as OpenRouter sends it: a comment, text in pieces, a tool call whose name and
    /// arguments come in pieces, usage in the last chunk.
    const STREAM: &str = ": OPENROUTER PROCESSING\n\n\
        data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Let me \"}}]}\n\n\
        data: {\"choices\":[{\"delta\":{\"content\":\"look. ✓\"}}]}\n\n\
        data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"calendar_list_events\",\"arguments\":\"\"}}]}}]}\n\n\
        data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"start\\\":\"}}]}}]}\n\n\
        data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"today\\\"}\"}}]}}]}\n\n\
        data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":30,\"completion_tokens\":9}}\n\n\
        data: [DONE]\n\n\
        data: {\"choices\":[{\"delta\":{\"content\":\"after the end\"}}]}\n";

    fn decode(pieces: &[&[u8]]) -> (Result<ChatReply, AiError>, Vec<String>) {
        let mut seen = Vec::new();
        let mut decoder = StreamDecoder::new();
        for piece in pieces {
            if let Err(err) = decoder.push(piece, &mut |text| seen.push(text.to_string())) {
                return (Err(err), seen);
            }
        }
        (decoder.finish(&mut |text| seen.push(text.to_string())), seen)
    }

    #[test]
    fn a_stream_is_put_together_in_any_pieces() {
        let bytes = STREAM.as_bytes();
        // Whole, byte by byte (which also cuts the ✓ in the middle), and in uneven pieces.
        let whole: Vec<&[u8]> = vec![bytes];
        let single: Vec<&[u8]> = bytes.chunks(1).collect();
        let uneven: Vec<&[u8]> = bytes.chunks(37).collect();
        for pieces in [whole, single, uneven] {
            let (reply, seen) = decode(&pieces);
            let reply = reply.expect("reply");
            assert_eq!(reply.text, "Let me look. ✓");
            assert_eq!(seen.concat(), reply.text, "every piece once, in order");
            assert_eq!(reply.finish, Finish::ToolCalls);
            assert_eq!(
                reply.tool_calls,
                [ToolCall {
                    id: "call_a".into(),
                    name: "calendar_list_events".into(),
                    arguments: r#"{"start":"today"}"#.into()
                }]
            );
            assert_eq!(reply.usage, Some(Usage { prompt_tokens: 30, completion_tokens: 9 }));
        }
    }

    #[test]
    fn calls_without_an_index_are_told_apart_by_their_id() {
        // Ollama sends each call whole.
        let stream = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"a\",\"function\":{\"name\":\"one\",\"arguments\":{\"x\":1}}}]}}]}\n\
                      data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"b\",\"function\":{\"name\":\"two\",\"arguments\":\"{}\"}}]}}]}\n\
                      data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n";
        let (reply, _) = decode(&[stream.as_bytes()]);
        let reply = reply.expect("reply");
        let names: Vec<(&str, &str, &str)> =
            reply.tool_calls.iter().map(|c| (c.id.as_str(), c.name.as_str(), c.arguments.as_str())).collect();
        assert_eq!(names, [("a", "one", r#"{"x":1}"#), ("b", "two", "{}")]);
        assert_eq!(reply.finish, Finish::Stop, "as the server said; the calls are what counts");
    }

    #[test]
    fn an_error_event_or_an_empty_stream_fails() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\
                       data: {\"error\":{\"code\":429,\"message\":\"Rate limit exceeded\"}}\n";
        let (reply, seen) = decode(&[stream]);
        assert!(matches!(reply, Err(AiError::Busy(_))), "{reply:?}");
        assert_eq!(seen, ["Hi"]);
        assert!(matches!(decode(&[b": keep-alive\n\n"]).0, Err(AiError::Format(_))));
        assert!(matches!(decode(&[b"data: {not json}\n"]).0, Err(AiError::Format(_))));
    }

    #[test]
    fn model_lists_tell_tools_open_weights_and_context_where_the_server_does() {
        let openrouter = br#"{"data":[
            {"id":"qwen/qwen3-14b","name":"Qwen: Qwen3 14B","context_length":40960,"hugging_face_id":"Qwen/Qwen3-14B","supported_parameters":["tools","temperature","reasoning"]},
            {"id":"openai/gpt-4o","name":"OpenAI: GPT-4o","context_length":128000,"hugging_face_id":null,"supported_parameters":["tools"]},
            {"id":"google/gemini-2.5-flash","name":"Gemini","hugging_face_id":"","supported_parameters":["temperature"]}]}"#;
        let models = parse_models(openrouter).expect("models");
        /// Id, tools, open weights and context of one model.
        type Facts<'a> = (&'a str, Option<bool>, Option<bool>, Option<u64>);
        let facts: Vec<Facts> = models.iter().map(|m| (m.id.as_str(), m.tools, m.open_weights, m.context)).collect();
        assert_eq!(
            facts,
            [
                ("qwen/qwen3-14b", Some(true), Some(true), Some(40960)),
                ("openai/gpt-4o", Some(true), Some(false), Some(128000)),
                ("google/gemini-2.5-flash", Some(false), Some(false), None),
            ]
        );
        assert_eq!(models[0].name, "Qwen: Qwen3 14B");
        let reasoning: Vec<Option<bool>> = models.iter().map(|m| m.reasoning).collect();
        assert_eq!(reasoning, [Some(true), Some(false), Some(false)], "issue #131");

        let ollama = br#"{"object":"list","data":[{"id":"qwen3:8b","object":"model","owned_by":"library"},{"id":""}]}"#;
        let models = parse_models(ollama).expect("models");
        assert_eq!(
            models,
            [AiModel {
                id: "qwen3:8b".into(),
                name: "qwen3:8b".into(),
                tools: None,
                open_weights: None,
                context: None,
                reasoning: None,
                remote: None,
                decision: None,
                enabled: false,
            }]
        );
        assert!(matches!(parse_models(br#"{"models":[]}"#), Err(AiError::Format(_))));
    }

    #[test]
    fn ollama_says_which_models_call_tools() {
        assert_eq!(ollama_tools(br#"{"capabilities":["completion","tools","thinking"]}"#), Some(true));
        assert_eq!(ollama_tools(br#"{"capabilities":["completion","vision"]}"#), Some(false));
        assert_eq!(ollama_tools(br#"{"modelfile":"FROM qwen3"}"#), None);
        assert_eq!(ollama_thinking(br#"{"capabilities":["completion","tools","thinking"]}"#), Some(true));
        assert_eq!(ollama_thinking(br#"{"capabilities":["completion","tools"]}"#), Some(false));
        assert_eq!(ollama_thinking(br#"{"modelfile":"FROM qwen3"}"#), None);
        assert_eq!(ollama_root("http://localhost:11434/v1/"), "http://localhost:11434");
        assert_eq!(ollama_root("http://localhost:11434"), "http://localhost:11434");
        assert_eq!(ollama_show_body("qwen3:8b"), json!({"model": "qwen3:8b"}));
    }

    /// Task S.23: Ollama's cloud models run on `ollama.com`; `/api/tags` names them by `remote_host`.
    #[test]
    fn ollama_tags_tell_which_models_run_elsewhere() {
        let tags = br#"{"models":[
            {"name":"glm-5.3:cloud","model":"glm-5.3:cloud","remote_model":"glm-5.3",
             "remote_host":"https://ollama.com:443","size":384},
            {"name":"my-glm:latest","model":"my-glm:latest","remote_host":"https://ollama.com:443"},
            {"name":"qwen3:8b","model":"qwen3:8b","size":5225388164,"remote_host":""}]}"#;
        let remote = ollama_remote(tags).expect("tags");
        assert_eq!(remote.get("glm-5.3:cloud"), Some(&true));
        assert_eq!(remote.get("qwen3:8b"), Some(&false));
        assert_eq!(ollama_remote(br#"{"data":[]}"#), None);
        assert_eq!(ollama_remote(b"<html>"), None);

        assert!(ollama_runs_elsewhere("glm-5.3:cloud", Some(&remote)));
        assert!(ollama_runs_elsewhere("my-glm:latest", Some(&remote)), "copied, without the suffix");
        assert!(!ollama_runs_elsewhere("qwen3:8b", Some(&remote)));
        assert!(ollama_runs_elsewhere("unlisted:7b", Some(&remote)), "unknown counts as elsewhere");
        assert!(ollama_runs_elsewhere("qwen3:8b", None), "without tags every model counts as elsewhere");
        assert!(ollama_runs_elsewhere("kimi-k2.6:cloud", Some(&HashMap::from([("kimi-k2.6:cloud".into(), false)]))));
        assert!(ollama_cloud_name("gpt-oss:120b-cloud") && ollama_cloud_name("Kimi-K2.6:CLOUD"));
        assert!(!ollama_cloud_name("qwen3:8b") && !ollama_cloud_name("cloudy:7b"));
    }

    #[test]
    fn errors_are_mapped_by_status_and_keep_a_short_message() {
        assert_eq!(error_of(401, br#"{"error":{"message":"Incorrect API key sk-or-v1-abcdef"}}"#), AiError::Auth);
        assert_eq!(error_of(403, b""), AiError::Auth);
        assert!(matches!(error_of(429, b""), AiError::Busy(_)));
        // Anthropic's "overloaded" is 529 and means the same for people.
        assert!(matches!(error_of(529, b""), AiError::Busy(_)));
        assert!(matches!(error_of(402, b""), AiError::Busy(_)));
        let AiError::NotFound(missing) =
            error_of(404, br#"{"error":"model \"qwen9\" not found, try pulling it first"}"#)
        else {
            panic!("not found")
        };
        assert!(missing.contains("qwen9"), "{missing}");
        assert_eq!(
            error_of(500, b"{\"message\":\"out of\\n memory\"}"),
            AiError::Server { status: 500, message: "out of memory".into() }
        );
        let AiError::Server { message, .. } = scrub(error_of(502, "x".repeat(1000).as_bytes()), None) else {
            panic!("server")
        };
        assert_eq!(message.chars().count(), MAX_MESSAGE + 1, "cut with an ellipsis");
    }

    #[test]
    fn server_texts_that_repeat_the_key_are_dropped() {
        let key = "sk-or-v1-0123456789abcdef";
        let leaked = AiError::Server { status: 400, message: "bad key sk-or-v1-0123…".into() };
        assert_eq!(scrub(leaked, Some(key)), AiError::Server { status: 400, message: String::new() });
        let part = AiError::NotFound("no such key 89abcdef".into());
        assert!(matches!(scrub(part, Some(key)), AiError::Format(message) if !message.contains("89abcdef")));
        let fine = AiError::Server { status: 500, message: "out of memory".into() };
        assert_eq!(scrub(fine.clone(), Some(key)), fine);
        assert_eq!(scrub(fine.clone(), None), fine);
        // A key right where the text is cut is found before the cut.
        let at_the_cut = format!("{}{key}", "x".repeat(MAX_MESSAGE - 3));
        let cut = scrub(error_of(500, at_the_cut.as_bytes()), Some(key));
        assert_eq!(cut, AiError::Server { status: 500, message: String::new() });
    }
}
