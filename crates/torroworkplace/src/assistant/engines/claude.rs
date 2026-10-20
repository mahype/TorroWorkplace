//! Claude Code as an engine of the assistant (task C.7): the command line it is started with, the
//! MCP configuration it gets, the message it reads and the events it writes, all in `stream-json`
//! (one JSON object per line, in both directions).
//!
//! Everything that depends on Claude Code's flags is here, so a new version of Claude Code
//! changes this file only. Checked against Claude Code 2.1: `--tools ""` leaves it no built-in
//! tool at all (its `init` event lists `"tools": []`), `--strict-mcp-config` with `--mcp-config`
//! leaves it only the suite's MCP server, `--settings={"disableAllHooks":true}` keeps the
//! person's hooks out, and a stdio MCP server inherits Claude Code's environment, which is how
//! the key reaches `torroworkplace mcp` without being written anywhere.

use std::ffi::OsString;
use std::path::Path;

use serde_json::{Value, json};
use torro_core::ai::{AiModel, Effort};
use torro_core::settings::ToolId;
use torro_ipc::api::{Access, Area, CATALOGUE, Mcp};
use torro_ipc::assistant::Mode;

use crate::client_setup::SERVER_NAME;

/// Every tool of the suite's MCP server, as Claude Code names it: the only ones it may use.
pub const ALLOWED_TOOLS: &str = "mcp__torroworkplace__*";

/// What the tools of the suite's MCP server start with in Claude Code's events. A run whose agent
/// says it has any other tool is stopped before it does anything (`super::Failed::OwnTools`).
pub const TOOL_PREFIX: &str = "mcp__torroworkplace__";

/// Claude Code's built-in tools that touch the shell, files, the web or other agents, denied by
/// name as well, though `--tools ""` already leaves none of them: should a version ignore one
/// flag, the other still holds (decision 6 in `docs/assistent.md`: files and shell always off).
pub const DENIED_TOOLS: &str = "Bash,BashOutput,KillShell,KillBash,PowerShell,REPL,Monitor,Read,Edit,MultiEdit,Write,\
                                NotebookRead,NotebookEdit,Glob,Grep,LS,WebFetch,WebSearch,Task,Agent,Skill,\
                                SlashCommand,TodoWrite,ExitPlanMode";

/// The flags of every run, in one place. Claude Code runs without a terminal (`-p`), reads the
/// person's message and writes its events as `stream-json`, with the text as it grows
/// (`--include-partial-messages`); it keeps no session of its own (`--no-session-persistence`),
/// since the core keeps the conversation and its retention; it has no built-in tool, only the
/// suite's MCP server, and asks nobody for permission (`dontAsk` denies what is not allowed). The
/// person's hooks stay out (`disableAllHooks`): they are made for coding sessions, and the
/// suite's own would report every answer to the inbox as a session of Claude Code.
const FLAGS: &[&str] = &[
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--no-session-persistence",
    "--disable-slash-commands",
    "--strict-mcp-config",
    "--tools",
    "",
    "--allowedTools",
    ALLOWED_TOOLS,
    "--permission-mode",
    "dontAsk",
    r#"--settings={"disableAllHooks":true}"#,
];

/// The models offered for Claude Code (issue #131), as `--model` takes them: the aliases Claude
/// Code 2.1 names in its help, each for the newest model of its family, with their names and
/// whether they take `--effort` (Haiku does not). Another name Claude Code knows can still be sent
/// as `model` ([`is_model_name`]).
const MODELS: &[(&str, &str, bool)] =
    &[("fable", "Fable", true), ("opus", "Opus", true), ("sonnet", "Sonnet", true), ("haiku", "Haiku", false)];

/// The models `assistant.list_engines` lists for Claude Code ([`MODELS`]): each can call tools.
pub fn models() -> Vec<AiModel> {
    MODELS
        .iter()
        .map(|(id, name, effort)| AiModel {
            id: (*id).into(),
            name: (*name).into(),
            tools: Some(true),
            open_weights: Some(false),
            context: None,
            reasoning: Some(*effort),
            remote: None,
            decision: None,
            enabled: false,
        })
        .collect()
}

/// The MCP configuration of a run (`--mcp-config`): the suite's server only, `program mcp`. It
/// holds no key: the server reads `TORRO_TOKEN` from the environment it inherits from Claude Code.
pub fn mcp_config(program: &Path) -> Value {
    json!({
        "mcpServers": {
            SERVER_NAME: {
                "type": "stdio",
                "command": program.to_string_lossy(),
                "args": ["mcp"],
            }
        }
    })
}

/// The arguments of a run: [`FLAGS`], the denied tools ([`DENIED_TOOLS`] and every tool of the
/// suite in the areas `without`, [`denied`]), the MCP configuration in the file `config`, the
/// system prompt to add to Claude Code's own, the model when the message named one that looks
/// like a model's name (`sonnet`, `claude-sonnet-4-5`; another is left out, so nothing from a
/// client can become a flag of its own), and the effort (`--effort=high`, issue #131). In a
/// read-only answer (`mode`, issue #132) the suite's tools that change something are denied as
/// well, and the commands of every one of the assistant's own tools but `tools` (issue #225: those
/// switched on and in the client's selection). Never the key.
pub fn arguments(
    config: &Path,
    prompt: &str,
    model: Option<&str>,
    effort: Option<Effort>,
    without: &[Area],
    mode: Mode,
    tools: &[ToolId],
) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = FLAGS.iter().map(OsString::from).collect();
    arguments.push("--disallowedTools".into());
    arguments.push(denied(without, mode, tools).into());
    arguments.push("--mcp-config".into());
    arguments.push(config.as_os_str().to_owned());
    if let Some(model) = model.filter(|model| is_model_name(model)) {
        arguments.push(format!("--model={model}").into());
    }
    if let Some(effort) = effort {
        arguments.push(format!("--effort={}", effort.id()).into());
    }
    // `=`: the prompt is one value whatever it starts with.
    arguments.push(format!("--append-system-prompt={prompt}").into());
    arguments
}

/// [`DENIED_TOOLS`] and, as Claude Code names them, the suite's tools of the areas `without` (issue
/// #133): the areas the agent never gets and those switched off in the conversation; in a read-only
/// answer every tool that changes something (issue #132); the commands of the assistant's own tools
/// not in `tools`, the ones switched on and selected (issue #225). Claude Code leaves denied tools
/// out of what its model sees, so the model does not try mail and run into `forbidden`; the core
/// refuses them anyway.
fn denied(without: &[Area], mode: Mode, tools: &[ToolId]) -> String {
    let read_only = mode == Mode::ReadOnly;
    let unusable = |name: &str| ToolId::of_command(name).is_some_and(|tool| !tools.contains(&tool));
    let commands = CATALOGUE
        .iter()
        .filter(|command| {
            command.mcp == Mcp::Tool
                && (without.contains(&command.area)
                    || (read_only && command.access != Access::Read)
                    || unusable(command.name))
        })
        .map(|command| format!("{TOOL_PREFIX}{}", command.tool_name()));
    std::iter::once(DENIED_TOOLS.to_string()).chain(commands).collect::<Vec<_>>().join(",")
}

/// A model's name as Claude Code takes it: letters, digits and `.-_:[]/@`, at most 100.
fn is_model_name(model: &str) -> bool {
    (1..=100).contains(&model.len())
        && !model.starts_with('-')
        && model.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:[]/@".contains(c))
}

/// The person's message as the one line Claude Code reads on its input.
pub fn input(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}}).to_string()
}

/// What one line of Claude Code's output means for the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// It started (the `init` event): the model that answers and every tool it has, built in or
    /// from an MCP server.
    Started { model: String, tools: Vec<String> },
    /// A piece of the text it is writing.
    Piece(String),
    /// A finished block of text: an answer entry.
    Text(String),
    /// It calls the tool `name` with `input`; `id` pairs it with its [`Event::ToolResult`].
    ToolUse { id: String, name: String, input: Value },
    /// The result of the call `id`: the tool result's text, and whether it is an error.
    ToolResult { id: String, text: String, error: bool },
    /// The run is over: `error` with Claude Code's message when it failed, `text` its last answer.
    Done { error: Option<String>, text: String },
}

/// The events of one line of output. A line that is no JSON, an event of a sub-agent and every
/// kind this does not know are no event: Claude Code adds kinds over time.
pub fn events(line: &str) -> Vec<Event> {
    let Ok(event) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    if !event.get("parent_tool_use_id").is_none_or(Value::is_null) {
        return Vec::new();
    }
    match event.get("type").and_then(Value::as_str) {
        Some("system") if text(&event, "subtype") == "init" => {
            let tools = event.get("tools").and_then(Value::as_array).into_iter().flatten();
            let tools = tools.map(|tool| tool.as_str().unwrap_or("?").to_string()).collect();
            vec![Event::Started { model: text(&event, "model"), tools }]
        }
        Some("stream_event") => {
            let delta = event.pointer("/event/delta");
            match delta.filter(|delta| delta.get("type").and_then(Value::as_str) == Some("text_delta")) {
                Some(delta) => vec![Event::Piece(text(delta, "text"))],
                None => Vec::new(),
            }
        }
        Some("assistant") => blocks(&event)
            .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("text") => Some(Event::Text(text(block, "text"))),
                Some("tool_use") => Some(Event::ToolUse {
                    id: text(block, "id"),
                    name: text(block, "name"),
                    input: block.get("input").cloned().unwrap_or_else(|| json!({})),
                }),
                _ => None,
            })
            .collect(),
        Some("user") => blocks(&event)
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|block| Event::ToolResult {
                id: text(block, "tool_use_id"),
                text: result_text(block.get("content")),
                error: block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect(),
        Some("result") => {
            let failed = event.get("is_error").and_then(Value::as_bool).unwrap_or(false)
                || !matches!(event.get("subtype").and_then(Value::as_str), Some("success") | None);
            let message = text(&event, "result");
            let error = failed.then(|| match message.trim() {
                "" => text(&event, "subtype"),
                message => message.to_string(),
            });
            vec![Event::Done { error, text: if failed { String::new() } else { message } }]
        }
        _ => Vec::new(),
    }
}

/// The content blocks of a message event.
fn blocks(event: &Value) -> impl Iterator<Item = &Value> {
    event.pointer("/message/content").and_then(Value::as_array).into_iter().flatten()
}

/// A tool result's content as text: a string, or the text of its text blocks.
fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => {
            blocks.iter().filter_map(|block| block.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_command_line_leaves_only_the_suites_tools_and_never_a_key() {
        let arguments = arguments(
            Path::new("/run/mcp.json"),
            "## Prompt",
            Some("sonnet"),
            Some(Effort::High),
            &[Area::Mail],
            Mode::Default,
            &[],
        );
        let arguments: Vec<String> = arguments.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        let after = |flag: &str| {
            let at = arguments.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} is missing"));
            arguments[at + 1].clone()
        };
        assert_eq!(after("--tools"), "", "no built-in tool");
        assert_eq!(after("--allowedTools"), "mcp__torroworkplace__*");
        for tool in ["Bash", "Read", "Edit", "Write", "WebFetch", "WebSearch", "Task"] {
            assert!(after("--disallowedTools").split(',').any(|denied| denied == tool), "{tool}");
        }
        // Issue #133: the mail tools are not offered at all, the others are.
        let denied: Vec<String> = after("--disallowedTools").split(',').map(str::to_string).collect();
        assert!(denied.contains(&"mcp__torroworkplace__mail_search_messages".to_string()));
        assert!(denied.contains(&"mcp__torroworkplace__mail_send_draft".to_string()));
        assert!(!denied.iter().any(|tool| tool.starts_with("mcp__torroworkplace__calendar_")));
        assert_eq!(arguments.iter().filter(|a| *a == "--disallowedTools").count(), 1, "one list");
        assert_eq!(after("--mcp-config"), "/run/mcp.json");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert!(arguments.contains(&"--strict-mcp-config".to_string()));
        assert!(arguments.contains(&r#"--settings={"disableAllHooks":true}"#.to_string()), "no hooks");
        assert!(arguments.contains(&"--model=sonnet".to_string()));
        assert!(arguments.contains(&"--effort=high".to_string()), "issue #131");
        assert_eq!(arguments.last().map(String::as_str), Some("--append-system-prompt=## Prompt"));

        let odd = super::arguments(
            Path::new("c"),
            "p",
            Some("--dangerously-skip-permissions"),
            None,
            &[],
            Mode::Default,
            &[],
        );
        assert!(odd.iter().all(|a| !a.to_string_lossy().contains("dangerously")), "no flag from a client");
        assert!(odd.iter().all(|a| !a.to_string_lossy().starts_with("--effort")), "no effort unless asked");
    }

    /// Issue #132: a read-only answer denies every tool of the suite that changes something.
    #[test]
    fn a_read_only_run_denies_the_tools_that_change_something() {
        let arguments = arguments(Path::new("c"), "p", None, None, &[], Mode::ReadOnly, &[]);
        let at = arguments.iter().position(|a| a == "--disallowedTools").expect("the list");
        let denied = arguments[at + 1].to_string_lossy().into_owned();
        let denied: Vec<&str> = denied.split(',').collect();
        assert!(denied.contains(&"mcp__torroworkplace__calendar_save_event"));
        assert!(denied.contains(&"mcp__torroworkplace__tasks_delete_todo"));
        assert!(!denied.contains(&"mcp__torroworkplace__calendar_list_events"), "reading stays");
        let default = super::arguments(Path::new("c"), "p", None, None, &[], Mode::Default, &[]);
        let at = default.iter().position(|a| a == "--disallowedTools").expect("the list");
        assert!(!default[at + 1].to_string_lossy().contains("calendar_save_event"));
    }

    /// Issue #225: the commands of an own tool that is switched off or not selected are denied, those
    /// of a usable one and listing the tools are not.
    #[test]
    fn a_run_denies_the_tools_it_may_not_use() {
        let denied_with = |tools: &[ToolId]| {
            let arguments = arguments(Path::new("c"), "p", None, None, &[], Mode::Default, tools);
            let at = arguments.iter().position(|a| a == "--disallowedTools").expect("the list");
            arguments[at + 1].to_string_lossy().split(',').map(str::to_string).collect::<Vec<_>>()
        };
        let none = denied_with(&[]);
        for command in ["tools_create_pdf", "tools_open_file"] {
            assert!(none.contains(&format!("{TOOL_PREFIX}{command}")), "{command}");
        }
        assert!(!none.contains(&format!("{TOOL_PREFIX}tools_list_tools")), "listing stays");
        let pdf = denied_with(&[ToolId::Pdf]);
        assert!(!pdf.iter().any(|tool| tool.starts_with(&format!("{TOOL_PREFIX}tools_"))), "{pdf:?}");
    }

    /// Issue #131: every offered model is one `--model` takes, and only Haiku takes no effort.
    #[test]
    fn the_offered_models_are_model_names() {
        let models = models();
        assert!(models.iter().all(|model| is_model_name(&model.id) && model.tools == Some(true)));
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert!(ids.contains(&"opus") && ids.contains(&"sonnet"));
        let without: Vec<&str> =
            models.iter().filter(|model| model.reasoning == Some(false)).map(|model| model.id.as_str()).collect();
        assert_eq!(without, ["haiku"]);
    }

    #[test]
    fn the_events_of_a_run_are_read() {
        let lines = [
            r#"{"type":"system","subtype":"init","model":"claude-sonnet-4-5","tools":["mcp__torroworkplace__system_ping"]}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Let "}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Let me look."},{"type":"tool_use","id":"t1","name":"mcp__torroworkplace__calendar_list_calendars","input":{}}]},"parent_tool_use_id":null}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"{\"result\":\"ok\"}"}],"is_error":false}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"forbidden: no","is_error":true}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"}]},"parent_tool_use_id":"t9"}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Done."}"#,
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
            "not json",
            r#"{"type":"rate_limit_event"}"#,
        ];
        let events: Vec<Event> = lines.iter().flat_map(|line| events(line)).collect();
        assert_eq!(
            events,
            [
                Event::Started {
                    model: "claude-sonnet-4-5".into(),
                    tools: vec!["mcp__torroworkplace__system_ping".into()]
                },
                Event::Piece("Let ".into()),
                Event::Text("Let me look.".into()),
                Event::ToolUse {
                    id: "t1".into(),
                    name: "mcp__torroworkplace__calendar_list_calendars".into(),
                    input: json!({})
                },
                Event::ToolResult { id: "t1".into(), text: r#"{"result":"ok"}"#.into(), error: false },
                Event::ToolResult { id: "t2".into(), text: "forbidden: no".into(), error: true },
                Event::Done { error: None, text: "Done.".into() },
                Event::Done { error: Some("error_during_execution".into()), text: String::new() },
            ]
        );
    }

    #[test]
    fn the_mcp_configuration_names_the_suites_server_only() {
        let config = mcp_config(Path::new("/usr/bin/torroworkplace"));
        let servers = config["mcpServers"].as_object().expect("servers");
        assert_eq!(servers.keys().collect::<Vec<_>>(), ["torroworkplace"]);
        assert_eq!(servers["torroworkplace"]["args"], json!(["mcp"]));
        assert!(servers["torroworkplace"].get("env").is_none(), "the key comes through the environment");
        assert_eq!(serde_json::from_str::<Value>(&input("Hi")).expect("json")["message"]["content"][0]["text"], "Hi");
    }
}
