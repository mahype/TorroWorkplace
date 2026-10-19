//! The wire format of Anthropic's Messages API (task KI.1b): what `<base>/messages` and
//! `<base>/models` take and send. Pure functions over bytes and JSON, without a network, so every
//! rule is tested here; the core sends them (`torroworkplace::account::ai::Anthropic`).
//!
//! The one protocol of the suite that is not OpenAI-compatible, and the difference is not only the
//! stream. A request body is shaped differently in four ways, each of which the server refuses
//! when it is got wrong:
//!
//! - the instructions before the conversation are the field `system`, not a message with a role;
//! - a message's content is a list of blocks, and a tool call is a `tool_use` block whose `input`
//!   is an object, where [`ToolCall::arguments`] is the text the model wrote;
//! - the result of a tool is a `tool_result` block in a message of the **person**, and the results
//!   of one round must sit in **one** message, else the model stops calling tools side by side;
//! - no block may be empty, and the first message must be the person's.
//!
//! [`chat_body`] does all four. A streamed answer comes as server-sent events, one JSON chunk per
//! `data:` line, each saying its own `type`; there is no `[DONE]` ([`StreamDecoder`]). Text and the
//! `input` of a tool call both arrive in pieces, keyed by the block's `index`, and are handed out
//! whole.
//!
//! Errors and keys are the OpenAI module's job, not because of that protocol but because neither
//! piece belongs to one: [`super::openai::error_of`] reads the message of an error body and
//! [`super::openai::scrub`] drops a server text that repeats the API key. The core passes every
//! error of this protocol through them as well.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::openai;
use super::{AiError, AiModel, ChatMessage, ChatReply, ChatRequest, Finish, Role, ToolCall, Usage};
use crate::i18n::tr;

/// The version of the Messages API this module writes and reads, for the sender's
/// `anthropic-version` header.
pub const VERSION: &str = "2023-06-01";

/// The ceiling for one answer when a [`ChatRequest`] names none, which it never does today: the
/// protocol demands the field. It is a ceiling, not a reservation – only what the model writes is
/// paid for, and an answer that reaches it ends as [`Finish::Length`].
///
/// 32768 for two reasons. The tokens a model spends thinking count against the same ceiling, so a
/// tight one would eat the thinking and then cut the answer; and it has to stay well under the
/// smallest output limit of the models Anthropic serves (64000), so a new model with a smaller
/// limit does not turn every request into an error.
pub const DEFAULT_MAX_TOKENS: u32 = 32_768;

/// What stands in for a block the suite would otherwise send empty, which the server refuses. It
/// is read by the model, not by people, so it is English and not translated.
const EMPTY: &str = "(empty)";

/// The body of `POST <base>/messages` for `request`: the model, the ceiling for the answer
/// ([`DEFAULT_MAX_TOKENS`] when none is named), `stream`, the conversation ([`messages_json`]), the
/// instructions as `system` and the tools as `input_schema` tools. Both are left out when there
/// are none.
///
/// With [`ChatRequest::effort`] the body also carries `output_config.effort` with the effort's
/// [`super::Effort::id`] and `thinking: {"type": "adaptive"}`, so the effort takes hold on the
/// models that do not think unless asked. Not `thinking.budget_tokens`: the current models answer
/// that with HTTP 400.
pub fn chat_body(request: &ChatRequest, stream: bool) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": stream,
        "messages": messages_json(&request.messages),
    });
    let system = system_of(&request.messages);
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| json!({ "name": tool.name, "description": tool.description, "input_schema": tool.parameters }))
            .collect();
        body["tools"] = Value::Array(tools);
    }
    if let Some(effort) = request.effort {
        body["output_config"] = json!({ "effort": effort.id() });
        body["thinking"] = json!({ "type": "adaptive" });
    }
    body
}

/// The instructions of a conversation: every [`Role::System`] message, in order, joined by a blank
/// line. Empty when there is none; the protocol has no system message, only this field.
fn system_of(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .filter(|message| message.role == Role::System)
        .map(|message| message.content.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The conversation as the protocol writes it: the system messages left out (they are `system`),
/// every other message turned into blocks, messages of the same role merged into one – so the
/// results of one round of tools travel together – and everything before the first message of the
/// person dropped, because the protocol starts with theirs.
fn messages_json(messages: &[ChatMessage]) -> Vec<Value> {
    let mut turns: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for message in messages {
        let Some((role, blocks)) = blocks_of(message) else { continue };
        match turns.last_mut() {
            Some((held, content)) if *held == role => content.extend(blocks),
            _ => turns.push((role, blocks)),
        }
    }
    let start = turns.iter().position(|(role, _)| *role == "user").unwrap_or(turns.len());
    turns[start..].iter().map(|(role, content)| json!({ "role": role, "content": content })).collect()
}

/// One message as a role and its content blocks; `None` for a message the body does not carry (a
/// [`Role::System`] one, or an assistant message that says and calls nothing).
fn blocks_of(message: &ChatMessage) -> Option<(&'static str, Vec<Value>)> {
    match message.role {
        Role::System => None,
        Role::User => Some(("user", vec![text_block(&message.content)])),
        Role::Assistant => {
            let mut blocks = Vec::new();
            // An assistant message that only calls tools gets no text block: an empty one is
            // refused.
            if !message.content.is_empty() {
                blocks.push(text_block(&message.content));
            }
            for call in &message.tool_calls {
                blocks.push(json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": input_of(&call.arguments),
                }));
            }
            (!blocks.is_empty()).then_some(("assistant", blocks))
        }
        // The result of a tool is a message of the person in this protocol.
        Role::Tool => Some((
            "user",
            vec![json!({
                "type": "tool_result",
                "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
                "content": filled(&message.content),
            })],
        )),
    }
}

fn text_block(text: &str) -> Value {
    json!({ "type": "text", "text": filled(text) })
}

/// `text`, or [`EMPTY`] where it is blank: the server refuses a block without content.
fn filled(text: &str) -> &str {
    match text.trim().is_empty() {
        true => EMPTY,
        false => text,
    }
}

/// The `input` of a tool call from the JSON text the model wrote ([`ToolCall::arguments`]): the
/// object it holds, else an empty one. The protocol takes an object and nothing else, and a small
/// model does not always write valid JSON.
fn input_of(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(object @ Value::Object(_)) => object,
        _ => json!({}),
    }
}

/// A whole answer (`stream: false`, or a server that answered whole): the text of its `text`
/// blocks, its `tool_use` blocks as calls, why it stopped and what it counted. Blocks of any other
/// kind – `thinking` above all – are not part of [`ChatReply::text`].
pub fn parse_reply(body: &[u8]) -> Result<ChatReply, AiError> {
    let answer: Value = serde_json::from_slice(body).map_err(|_| not_json())?;
    if answer.get("type").and_then(Value::as_str) == Some("error") {
        return Err(openai::error_of(500, body));
    }
    let content = answer
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| format_error(tr("The answer has no message.")))?;
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(block.get("text").and_then(Value::as_str).unwrap_or_default()),
            Some("tool_use") => {
                let at = tool_calls.len();
                tool_calls.push(ToolCall {
                    id: text_of(block.get("id")).filter(|id| !id.is_empty()).unwrap_or_else(|| made_up_id(at)),
                    name: text_of(block.get("name")).unwrap_or_default(),
                    arguments: block.get("input").map_or_else(String::new, Value::to_string),
                });
            }
            _ => {}
        }
    }
    Ok(ChatReply {
        text,
        tool_calls,
        finish: finish_of(answer.get("stop_reason")).unwrap_or_default(),
        usage: usage_of(answer.get("usage"), None),
    })
}

/// A tool call being put together from a stream, by the `index` of its block.
#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    /// The `input` as the model writes it, piece by piece (`input_json_delta`).
    arguments: String,
}

/// Puts a streamed answer together: feed it the body's bytes as they come ([`StreamDecoder::push`],
/// in any pieces, also cut in the middle of a line or of a UTF-8 character), then take the reply
/// ([`StreamDecoder::finish`]).
///
/// Every chunk arrives on a `data:` line and says its own `type`; the `event:` lines that go with
/// them, empty lines and `ping` are skipped. `message_stop` ends the answer and anything after it
/// is ignored. There is no `[DONE]` in this protocol.
#[derive(Debug, Default)]
pub struct StreamDecoder {
    /// Bytes after the last line break, waiting for the rest of their line.
    pending: Vec<u8>,
    text: String,
    /// The `tool_use` blocks by their `index`; blocks of other kinds are not held.
    calls: HashMap<usize, PartialCall>,
    /// The order the `tool_use` blocks started in, so the calls come out as the server sent them.
    order: Vec<usize>,
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

    /// Reads `bytes` of the stream; every new piece of the answer's text goes to `on_text`. What the
    /// model thinks (`thinking_delta`) does not: it is no part of the answer. An error event of the
    /// server ends the answer with that error.
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
        let order = std::mem::take(&mut self.order);
        let tool_calls: Vec<ToolCall> = order
            .iter()
            .enumerate()
            .filter_map(|(at, index)| self.calls.remove(index).map(|call| (at, call)))
            .map(|(at, call)| ToolCall {
                id: if call.id.is_empty() { made_up_id(at) } else { call.id },
                name: call.name,
                arguments: call.arguments,
            })
            .collect();
        Ok(ChatReply { text: self.text, tool_calls, finish: self.finish.unwrap_or_default(), usage: self.usage })
    }

    fn line(&mut self, line: &[u8], on_text: &mut dyn FnMut(&str)) -> Result<(), AiError> {
        if self.done {
            return Ok(());
        }
        let line = String::from_utf8_lossy(line);
        let Some(data) = line.trim_end_matches(['\r', '\n']).strip_prefix("data:") else { return Ok(()) };
        let data = data.trim();
        if data.is_empty() {
            return Ok(());
        }
        let chunk: Value = serde_json::from_str(data).map_err(|_| not_json())?;
        let kind = chunk.get("type").and_then(Value::as_str).unwrap_or_default();
        if kind == "error" {
            return Err(openai::error_of(500, data.as_bytes()));
        }
        self.started = true;
        match kind {
            "message_start" => self.usage = usage_of(chunk.pointer("/message/usage"), self.usage),
            "content_block_start" => self.block_start(&chunk),
            "content_block_delta" => self.block_delta(&chunk, on_text),
            "message_delta" => {
                if let Some(finish) = finish_of(chunk.pointer("/delta/stop_reason")) {
                    self.finish = Some(finish);
                }
                self.usage = usage_of(chunk.get("usage"), self.usage);
            }
            "message_stop" => self.done = true,
            // `ping`, `content_block_stop` and whatever the protocol adds later.
            _ => {}
        }
        Ok(())
    }

    /// A block that starts: only a `tool_use` one is held, with the id and name the server gives it
    /// here; its `input` follows piece by piece.
    fn block_start(&mut self, chunk: &Value) {
        let Some(index) = index_of(chunk) else { return };
        if chunk.pointer("/content_block/type").and_then(Value::as_str) != Some("tool_use") {
            return;
        }
        let call = PartialCall {
            id: text_of(chunk.pointer("/content_block/id")).unwrap_or_default(),
            name: text_of(chunk.pointer("/content_block/name")).unwrap_or_default(),
            arguments: String::new(),
        };
        if self.calls.insert(index, call).is_none() {
            self.order.push(index);
        }
    }

    /// One piece of a block: text goes to the answer, the JSON of a call's `input` to that call.
    /// What the model thinks is dropped.
    fn block_delta(&mut self, chunk: &Value, on_text: &mut dyn FnMut(&str)) {
        let piece = chunk.pointer("/delta/text").and_then(Value::as_str);
        match chunk.pointer("/delta/type").and_then(Value::as_str) {
            Some("text_delta") => {
                if let Some(piece) = piece.filter(|piece| !piece.is_empty()) {
                    self.text.push_str(piece);
                    on_text(piece);
                }
            }
            Some("input_json_delta") => {
                let Some(index) = index_of(chunk) else { return };
                let part = chunk.pointer("/delta/partial_json").and_then(Value::as_str).unwrap_or_default();
                if let Some(call) = self.calls.get_mut(&index) {
                    call.arguments.push_str(part);
                }
            }
            // `thinking_delta` and `signature_delta`: no part of the answer.
            _ => {}
        }
    }
}

/// The models of a `GET <base>/models` answer (`{"data": [{"id": …}, …]}`), in its order, with what
/// the answer tells about each:
///
/// - `display_name` as the name, else the id;
/// - `max_input_tokens` as [`AiModel::context`];
/// - `capabilities.effort.supported` as [`AiModel::reasoning`]: whether the model takes an
///   [`super::Effort`]. `None` for an answer that does not say, so apps still offer one.
///
/// [`AiModel::tools`] is `true` for every model: they all call tools over this API, which is why
/// the list does not mention it. [`AiModel::open_weights`] stays unknown, as with every other cloud
/// provider – `false` would make [`super::open_weights_only`] drop every Claude model, and whether
/// the weights are published is said by the provider's own row, not per model.
/// [`AiModel::remote`] stays unknown too; that a cloud provider's models run elsewhere is decided
/// by the account, not the model (`torroworkplace::assistant::place_of`).
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
            Some(AiModel {
                name: text_of(model.get("display_name")).filter(|name| !name.is_empty()).unwrap_or_else(|| id.clone()),
                id,
                tools: Some(true),
                open_weights: None,
                context: model.get("max_input_tokens").and_then(Value::as_u64),
                reasoning: model.pointer("/capabilities/effort/supported").and_then(Value::as_bool),
                remote: None,
            })
        })
        .collect())
}

/// The tokens a turn used: `input_tokens` and `output_tokens` where the chunk names them, over what
/// an earlier chunk said (`message_start` counts the input, `message_delta` the output). `None`
/// when neither has.
fn usage_of(value: Option<&Value>, so_far: Option<Usage>) -> Option<Usage> {
    let usage = value.filter(|usage| usage.is_object())?;
    let held = so_far.unwrap_or_default();
    Some(Usage {
        prompt_tokens: usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(held.prompt_tokens),
        completion_tokens: usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(held.completion_tokens),
    })
}

fn finish_of(value: Option<&Value>) -> Option<Finish> {
    Some(match value?.as_str()? {
        "end_turn" | "stop_sequence" => Finish::Stop,
        "tool_use" => Finish::ToolCalls,
        "max_tokens" => Finish::Length,
        // `refusal`, `pause_turn` and whatever else the protocol names.
        _ => Finish::Other,
    })
}

fn index_of(chunk: &Value) -> Option<usize> {
    chunk.get("index").and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok())
}

fn text_of(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
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
    use crate::ai::{Effort, ToolSpec};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "claude-opus-5".into(),
            messages: vec![
                ChatMessage::system("You help."),
                ChatMessage::user("What is on today?"),
                ChatMessage::assistant(
                    "",
                    vec![ToolCall {
                        id: "toolu_1".into(),
                        name: "calendar_list_events".into(),
                        arguments: r#"{"start":"today"}"#.into(),
                    }],
                ),
                ChatMessage::tool("toolu_1", "[]"),
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

    #[test]
    fn the_body_lifts_the_system_text_out_and_writes_blocks_as_the_protocol_says() {
        let body = chat_body(&request(), true);
        assert_eq!(body["model"], "claude-opus-5");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["system"], "You help.", "the instructions are a field, not a message");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            body["messages"][0],
            json!({"role": "user", "content": [{"type": "text", "text": "What is on today?"}]})
        );
        assert_eq!(
            body["messages"][1],
            json!({"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "calendar_list_events", "input": {"start": "today"}},
            ]}),
            "only calls: no text block, and the arguments became an object"
        );
        assert_eq!(
            body["messages"][2],
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "[]"}]}),
            "a tool result is a message of the person"
        );
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert!(body["tools"][0].get("parameters").is_none(), "the schema is input_schema here");
        assert!(body.get("output_config").is_none() && body.get("thinking").is_none(), "no effort asked: {body}");
    }

    #[test]
    fn without_tools_or_a_ceiling_the_body_still_names_one_and_leaves_tools_out() {
        let body = chat_body(&ChatRequest { tools: Vec::new(), max_tokens: None, ..request() }, false);
        assert!(body.get("tools").is_none(), "{body}");
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS, "the protocol demands a ceiling");
        assert_eq!(body["stream"], false);
        const { assert!(DEFAULT_MAX_TOKENS < 64_000, "it has to stay under the smallest output limit") };
    }

    #[test]
    fn the_effort_is_the_output_config_and_turns_thinking_on() {
        for effort in Effort::ALL {
            let body = chat_body(&ChatRequest { effort: Some(effort), ..request() }, true);
            assert_eq!(body["output_config"], json!({"effort": effort.id()}), "{effort:?}");
            assert_eq!(body["thinking"], json!({"type": "adaptive"}), "{effort:?}");
            assert!(body["thinking"].get("budget_tokens").is_none(), "a budget is refused with HTTP 400");
        }
    }

    #[test]
    fn the_results_of_one_round_travel_in_one_message_and_no_block_is_empty() {
        let asked = ChatRequest {
            messages: vec![
                ChatMessage::user("Do both."),
                ChatMessage::assistant(
                    "On it.",
                    vec![
                        ToolCall { id: "a".into(), name: "one".into(), arguments: "{}".into() },
                        ToolCall { id: "b".into(), name: "two".into(), arguments: "not json".into() },
                    ],
                ),
                ChatMessage::tool("a", "[]"),
                ChatMessage::tool("b", ""),
            ],
            ..request()
        };
        let body = chat_body(&asked, true);
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3, "the two results merged into one message: {body}");
        assert_eq!(body["messages"][1]["content"][0], json!({"type": "text", "text": "On it."}));
        assert_eq!(body["messages"][1]["content"][2]["input"], json!({}), "arguments that are no object");
        let results = body["messages"][2]["content"].as_array().expect("results");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "a");
        assert_eq!(results[1]["content"], EMPTY, "an empty result would be refused");
    }

    #[test]
    fn the_conversation_starts_with_the_person_and_system_texts_join() {
        let asked = ChatRequest {
            messages: vec![
                ChatMessage::system("First."),
                ChatMessage::assistant("Left over.", Vec::new()),
                ChatMessage::system("Second."),
                ChatMessage::user("Now."),
            ],
            ..request()
        };
        let body = chat_body(&asked, true);
        assert_eq!(body["system"], "First.\n\nSecond.");
        assert_eq!(body["messages"].as_array().map(Vec::len), Some(1), "the assistant's turn before it fell away");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn a_whole_answer_is_read_with_its_calls_and_usage() {
        let body = br#"{"type":"message","role":"assistant","content":[
            {"type":"thinking","thinking":"hm"},
            {"type":"text","text":"Two events."},
            {"type":"tool_use","id":"toolu_9","name":"calendar_list_events","input":{"start":"today"}},
            {"type":"tool_use","name":"tasks_list_todos","input":{}}],
            "stop_reason":"tool_use","usage":{"input_tokens":12,"output_tokens":7}}"#;
        let reply = parse_reply(body).expect("reply");
        assert_eq!(reply.text, "Two events.", "what it thinks is no part of the answer");
        assert_eq!(reply.finish, Finish::ToolCalls);
        assert_eq!(reply.usage, Some(Usage { prompt_tokens: 12, completion_tokens: 7 }));
        assert_eq!(reply.tool_calls.len(), 2);
        assert_eq!(reply.tool_calls[0].id, "toolu_9");
        assert_eq!(reply.tool_calls[0].arguments, r#"{"start":"today"}"#);
        assert_eq!(reply.tool_calls[1].id, "call_1", "a call the server did not name");
    }

    #[test]
    fn every_reason_to_stop_is_read_and_an_error_body_is_an_error() {
        for (reason, finish) in [
            ("end_turn", Finish::Stop),
            ("stop_sequence", Finish::Stop),
            ("tool_use", Finish::ToolCalls),
            ("max_tokens", Finish::Length),
            ("refusal", Finish::Other),
        ] {
            let body = format!(r#"{{"content":[],"stop_reason":"{reason}"}}"#);
            assert_eq!(parse_reply(body.as_bytes()).expect(reason).finish, finish, "{reason}");
        }
        let refused = br#"{"type":"error","error":{"type":"invalid_request_error","message":"model: wrong"}}"#;
        assert!(
            matches!(parse_reply(refused), Err(AiError::Server { status: 500, message }) if message.contains("wrong")),
            "{:?}",
            parse_reply(refused)
        );
        assert!(matches!(parse_reply(b"{}"), Err(AiError::Format(_))), "no content at all");
        assert!(matches!(parse_reply(b"<html>"), Err(AiError::Format(_))));
    }

    /// The events of one streamed answer, as the server sends them.
    fn stream() -> String {
        [
            r#"event: message_start"#,
            r#"data: {"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":20,"output_tokens":1}}}"#,
            r#""#,
            r#"event: ping"#,
            r#"data: {"type":"ping"}"#,
            r#""#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"quietly"}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Zwei "}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Termine ☕"}}"#,
            r#"data: {"type":"content_block_stop","index":1}"#,
            r#"data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_7","name":"calendar_list_events"}}"#,
            r#"data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"start\":"}}"#,
            r#"data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"today\"}"}}"#,
            r#"data: {"type":"content_block_stop","index":2}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]
        .join("\n")
            + "\n"
    }

    /// `bytes` in pieces of `size`, which cuts lines and the UTF-8 of "☕" apart.
    fn pushed(bytes: &[u8], size: usize) -> (ChatReply, Vec<String>) {
        let mut pieces = Vec::new();
        let mut decoder = StreamDecoder::new();
        {
            let mut on_text = |piece: &str| pieces.push(piece.to_string());
            for chunk in bytes.chunks(size) {
                decoder.push(chunk, &mut on_text).expect("push");
            }
            let reply = decoder.finish(&mut on_text).expect("finish");
            (reply, pieces)
        }
    }

    #[test]
    fn a_stream_in_any_pieces_gives_the_text_in_order_and_the_call_whole() {
        let body = stream();
        for size in [1, 3, 17, 64, 512, body.len()] {
            let (reply, pieces) = pushed(body.as_bytes(), size);
            assert_eq!(reply.text, "Zwei Termine ☕", "size {size}");
            assert_eq!(pieces.concat(), reply.text, "every piece once and in order, size {size}");
            assert!(!pieces.iter().any(|piece| piece.contains("quietly")), "thinking is not the answer");
            assert_eq!(reply.finish, Finish::ToolCalls, "size {size}");
            assert_eq!(reply.usage, Some(Usage { prompt_tokens: 20, completion_tokens: 9 }), "size {size}");
            assert_eq!(reply.tool_calls.len(), 1, "size {size}");
            assert_eq!(reply.tool_calls[0].id, "toolu_7");
            assert_eq!(reply.tool_calls[0].name, "calendar_list_events");
            assert_eq!(reply.tool_calls[0].arguments, r#"{"start":"today"}"#, "size {size}");
        }
    }

    #[test]
    fn the_calls_of_a_stream_keep_the_servers_order() {
        let body = [
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"b","name":"second"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"a","name":"first"}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]
        .join("\n");
        let (reply, _) = pushed(body.as_bytes(), 9);
        let names: Vec<&str> = reply.tool_calls.iter().map(|call| call.name.as_str()).collect();
        assert_eq!(names, ["second", "first"]);
        assert_eq!(reply.tool_calls[1].arguments, "", "a call whose input never came");
    }

    #[test]
    fn a_stream_that_says_nothing_or_breaks_is_a_format_error() {
        let nothing = StreamDecoder::new().finish(&mut |_| {});
        assert!(matches!(nothing, Err(AiError::Format(_))), "{nothing:?}");

        let mut comments = StreamDecoder::new();
        comments.push(b"event: ping\n\n: hello\n", &mut |_| {}).expect("push");
        assert!(matches!(comments.finish(&mut |_| {}), Err(AiError::Format(_))), "lines without data say nothing");

        let mut broken = StreamDecoder::new();
        assert!(matches!(broken.push(b"data: {oops}\n", &mut |_| {}), Err(AiError::Format(_))));

        let mut refused = StreamDecoder::new();
        let event = br#"data: {"type":"error","error":{"type":"overloaded_error","message":"busy now"}}
"#;
        let failed = refused.push(event, &mut |_| {});
        assert!(matches!(&failed, Err(AiError::Server { message, .. }) if message.contains("busy now")), "{failed:?}");
    }

    #[test]
    fn anything_after_the_last_event_is_ignored() {
        let body = concat!(
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Done\"}}\n",
            "data: {\"type\":\"message_stop\"}\n",
            "data: {oops}\n",
        );
        let (reply, _) = pushed(body.as_bytes(), 11);
        assert_eq!(reply.text, "Done");
    }

    #[test]
    fn the_model_list_reads_what_the_answer_tells_and_leaves_the_weights_open() {
        let body = br#"{"data":[
            {"type":"model","id":"claude-opus-5","display_name":"Claude Opus 5","max_input_tokens":1000000,
             "max_tokens":128000,"capabilities":{"image_input":{"supported":true},
             "effort":{"supported":true,"low":{"supported":true},"max":{"supported":true}},
             "thinking":{"supported":true}}},
            {"type":"model","id":"claude-haiku-4-5","display_name":"","max_input_tokens":200000,
             "capabilities":{"effort":{"supported":false}}},
            {"type":"model","id":"claude-old"},
            {"type":"model","display_name":"No id"}],"has_more":false}"#;
        let models = parse_models(body).expect("models");
        assert_eq!(models.len(), 3, "the one without an id is dropped");
        assert_eq!(models[0].id, "claude-opus-5");
        assert_eq!(models[0].name, "Claude Opus 5");
        assert_eq!(models[0].context, Some(1_000_000));
        assert_eq!(models[0].reasoning, Some(true));
        assert_eq!(models[1].name, "claude-haiku-4-5", "no name given: the id");
        assert_eq!(models[1].reasoning, Some(false), "so apps offer no effort for it");
        assert_eq!(models[2].reasoning, None, "an answer that does not say");
        assert_eq!(models[2].context, None);
        for model in &models {
            assert_eq!(model.tools, Some(true), "{}: they all call tools", model.id);
            assert_eq!(model.open_weights, None, "{}: never Some(false), or the filter drops it", model.id);
            assert_eq!(model.remote, None, "{}: the account decides that, not the model", model.id);
        }
        assert_eq!(crate::ai::open_weights_only(models).len(), 3, "Open First keeps them all");
        assert!(matches!(parse_models(b"{}"), Err(AiError::Format(_))));
        assert!(matches!(parse_models(b"nope"), Err(AiError::Format(_))));
    }
}
