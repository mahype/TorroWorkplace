//! OpenAI's Responses API wire format. Other OpenAI-compatible providers use `openai` instead.

use serde_json::{Value, json};

use super::{AiError, ChatMessage, ChatReply, ChatRequest, Finish, Role, ToolCall, Usage, openai};
use crate::i18n::tr;

/// Build a stateless request so a tool round can replay its encrypted reasoning items.
pub fn body(request: &ChatRequest, stream: bool) -> Value {
    let mut input = Vec::new();
    for message in &request.messages {
        append_message(&mut input, message);
    }
    let mut body = json!({
        "model": request.model,
        "input": input,
        "stream": stream,
        "store": false,
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(
            request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                        "strict": false,
                    })
                })
                .collect(),
        );
    }
    if let Some(effort) = request.effort {
        body["reasoning"] = json!({ "effort": effort.id() });
    }
    if let Some(max) = request.max_tokens {
        body["max_output_tokens"] = json!(max);
    }
    body
}

fn append_message(input: &mut Vec<Value>, message: &ChatMessage) {
    match message.role {
        Role::Tool => input.push(json!({
            "type": "function_call_output",
            "call_id": message.tool_call_id,
            "output": message.content,
        })),
        Role::Assistant => {
            // Replay every output item unchanged, including reasoning, message phase and tool
            // call IDs. The assistant loop carries these opaque items in `thinking`.
            if !message.thinking.is_empty() {
                input.extend(message.thinking.iter().cloned());
            } else {
                if !message.content.is_empty() {
                    input.push(json!({ "role": "assistant", "content": message.content }));
                }
                for call in &message.tool_calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }));
                }
            }
        }
        Role::System | Role::User => input.push(json!({
            "role": if message.role == Role::System { "system" } else { "user" },
            "content": message.content,
        })),
    }
}

/// Decode one complete response, including text, calls, token usage and encrypted reasoning.
pub fn parse_reply(bytes: &[u8]) -> Result<ChatReply, AiError> {
    let response: Value = serde_json::from_slice(bytes)
        .map_err(|_| AiError::Format(tr("The AI server sent an answer that is not JSON.").into()))?;
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return Err(openai::error_of(500, bytes));
    }
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::Format(tr("The answer has no message.").into()))?;
    let mut reply = ChatReply::default();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => reply.tool_calls.push(ToolCall {
                id: item.get("call_id").and_then(Value::as_str).unwrap_or_default().into(),
                name: item.get("name").and_then(Value::as_str).unwrap_or_default().into(),
                arguments: item.get("arguments").and_then(Value::as_str).unwrap_or_default().into(),
            }),
            Some("message") => {
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    for part in content {
                        if part.get("type").and_then(Value::as_str) == Some("output_text")
                            && let Some(text) = part.get("text").and_then(Value::as_str)
                        {
                            reply.text.push_str(text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !reply.tool_calls.is_empty() {
        reply.thinking = output.clone();
    }
    reply.finish = if !reply.tool_calls.is_empty() {
        Finish::ToolCalls
    } else if response.get("status").and_then(Value::as_str) == Some("incomplete") {
        Finish::Length
    } else {
        Finish::Stop
    };
    reply.usage = response.get("usage").filter(|usage| usage.is_object()).map(|usage| Usage {
        prompt_tokens: usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        completion_tokens: usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
    });
    Ok(reply)
}

/// Decode Responses SSE and emit text deltas as soon as they arrive. The completed event holds
/// the authoritative answer, including calls and encrypted reasoning items.
#[derive(Default)]
pub struct StreamDecoder {
    pending: Vec<u8>,
    completed: Option<Value>,
    text_sent: String,
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8], on_text: &mut dyn FnMut(&str)) -> Result<(), AiError> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data: ") else { continue };
            if data == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(data)
                .map_err(|_| AiError::Format(tr("The AI server sent an answer that is not JSON.").into()))?;
            match event.get("type").and_then(Value::as_str) {
                Some("response.output_text.delta") => {
                    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                        self.text_sent.push_str(delta);
                        on_text(delta);
                    }
                }
                Some("response.completed" | "response.incomplete" | "response.failed") => {
                    self.completed = event.get("response").cloned();
                }
                Some("error") => return Err(openai::error_of(500, data.as_bytes())),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn finish(self, on_text: &mut dyn FnMut(&str)) -> Result<ChatReply, AiError> {
        let response = self.completed.ok_or_else(|| AiError::Format(tr("The answer has no message.").into()))?;
        let reply = parse_reply(&serde_json::to_vec(&response).unwrap_or_default())?;
        if self.text_sent.is_empty() && !reply.text.is_empty() {
            on_text(&reply.text);
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ChatMessage, Effort, ToolSpec};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gpt-6.1-sol".into(),
            messages: vec![ChatMessage::system("Help."), ChatMessage::user("Move my tasks.")],
            tools: vec![ToolSpec {
                name: "tasks_list".into(),
                description: "List tasks".into(),
                parameters: json!({"type":"object","properties":{}}),
            }],
            max_tokens: Some(1024),
            effort: Some(Effort::High),
        }
    }

    fn tool_response() -> Value {
        json!({
            "status": "completed",
            "output": [
                {"id":"rs_1","type":"reasoning","summary":[],"encrypted_content":"opaque"},
                {"id":"fc_1","type":"function_call","call_id":"call_1","name":"tasks_list","arguments":"{}"}
            ],
            "usage":{"input_tokens":20,"output_tokens":8}
        })
    }

    #[test]
    fn tools_and_reasoning_use_responses_format() {
        let body = body(&request(), true);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["max_output_tokens"], 1024);
        assert_eq!(body["tools"][0]["name"], "tasks_list");
        assert_eq!(body["tools"][0]["strict"], false);
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["input"][0], json!({"role":"system","content":"Help."}));
    }

    #[test]
    fn a_tool_round_replays_reasoning_call_and_result() {
        let first = tool_response();
        let reply = parse_reply(&serde_json::to_vec(&first).unwrap()).unwrap();
        assert_eq!(reply.finish, Finish::ToolCalls);
        assert_eq!(reply.tool_calls[0].id, "call_1");
        assert_eq!(reply.usage, Some(Usage { prompt_tokens: 20, completion_tokens: 8 }));
        let mut request = request();
        request.messages.push(ChatMessage::assistant(reply.text, reply.tool_calls).with_thinking(reply.thinking));
        request.messages.push(ChatMessage::tool("call_1", "13 tasks"));
        let next = body(&request, true);
        assert_eq!(next["input"][2], first["output"][0]);
        assert_eq!(next["input"][3], first["output"][1]);
        assert_eq!(
            next["input"][4],
            json!({
                "type":"function_call_output","call_id":"call_1","output":"13 tasks"
            })
        );
    }

    #[test]
    fn streamed_text_is_delivered_once() {
        let final_response = json!({
            "status":"completed",
            "output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}],
            "usage":{"input_tokens":12,"output_tokens":2}
        });
        let stream = format!(
            "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"Do\"}}\n\n\
             data: {{\"type\":\"response.output_text.delta\",\"delta\":\"ne.\"}}\n\n\
             data: {}\n\ndata: [DONE]\n\n",
            json!({"type":"response.completed","response":final_response})
        );
        let mut decoder = StreamDecoder::new();
        let mut pieces = Vec::new();
        for bytes in stream.as_bytes().chunks(7) {
            decoder.push(bytes, &mut |text| pieces.push(text.to_string())).unwrap();
        }
        let reply = decoder.finish(&mut |text| pieces.push(text.to_string())).unwrap();
        assert_eq!(pieces, ["Do", "ne."]);
        assert_eq!(reply.text, "Done.");
        assert_eq!(reply.finish, Finish::Stop);
    }

    #[test]
    fn streamed_tool_call_keeps_complete_output_items() {
        let stream =
            format!("data: {}\n\ndata: [DONE]\n\n", json!({"type":"response.completed","response":tool_response()}));
        let mut decoder = StreamDecoder::new();
        for bytes in stream.as_bytes().chunks(5) {
            decoder.push(bytes, &mut |_| {}).unwrap();
        }
        let reply = decoder.finish(&mut |_| {}).unwrap();
        assert_eq!(reply.finish, Finish::ToolCalls);
        assert_eq!(reply.tool_calls[0].name, "tasks_list");
        assert_eq!(reply.thinking, tool_response()["output"].as_array().unwrap().clone());
    }
}
