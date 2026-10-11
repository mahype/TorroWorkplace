//! What a hook of a coding agent means for the inbox (task E2.2a).
//!
//! Claude Code, Codex and Gemini CLI run a command on events such as "waits for permission" or
//! "finished its turn" and hand it a JSON object on stdin. [`interpret`] turns that object into a
//! [`HookIntent`]: report an entry, resolve the waiting of a session, resolve the whole session,
//! or nothing. It is pure: the caller (`torroworkplace hook`, E2.2) reads stdin and `.git/HEAD`,
//! adds the parent processes and sends the request.
//!
//! A hook must never disturb the agent, so nothing here fails: broken JSON, unknown events and
//! missing fields all give [`HookIntent::Nothing`].
//!
//! | Agent | Event | Intent |
//! |---|---|---|
//! | Claude Code | `Notification` with `permission_prompt`, `agent_needs_input`, `elicitation_dialog` | report `agent_waiting` |
//! | Claude Code | `Stop` | report `agent_done` |
//! | Claude Code | `UserPromptSubmit`, `PostToolUse` | resolve waiting |
//! | Claude Code | `SessionEnd` | resolve session |
//! | Codex | `PermissionRequest` | report `agent_waiting` (observed only, never answered) |
//! | Codex | `Stop` | report `agent_done` |
//! | Codex | `UserPromptSubmit`, `PostToolUse` | resolve waiting |
//! | Gemini CLI | `Notification` with `ToolPermission` | report `agent_waiting` |
//! | Gemini CLI | `AfterAgent` | report `agent_done` |
//!
//! Everything else is nothing, among it Claude Code's `idle_prompt` (its `Stop` already said the
//! turn is over) and `SubagentStop` (the session as a whole still runs).

use std::path::Path;

use serde_json::Value;

use crate::item::{AgentInfo, AgentKind, Kind};

/// What the inbox should do about one hook call. Built by [`interpret`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookIntent {
    /// Post an entry (`inbox.post_item`). The core writes the title itself with `tr()` from
    /// `kind` and `agent`, places it ([`crate::rules::placement`]) and merges it with the open
    /// entry of the same `thread_key`, so a `Stop` replaces the waiting entry of its session.
    Report {
        /// [`Kind::AgentWaiting`] or [`Kind::AgentDone`], never another kind.
        kind: Kind,
        /// Agent, session, project and branch. `pids` is empty; the caller fills it (E2.2).
        agent: AgentInfo,
        /// `<agent id>:<session id>`, e.g. `claude-code:abc123` ([`thread_key`]). The core puts
        /// the client in front (security rule 2).
        thread_key: String,
        /// The agent's own message (notification text, last answer, reason of a permission
        /// request), trimmed; `None` when it sent none. Foreign text: the entry is `untrusted`.
        body: Option<String>,
    },
    /// The session went on (the user answered, a tool ran): resolve its open
    /// [`Kind::AgentWaiting`] entry, leave a done entry alone.
    ResolveWaiting {
        /// Same form as in [`HookIntent::Report`].
        thread_key: String,
    },
    /// The session ended: resolve all its open entries.
    ResolveSession {
        /// Same form as in [`HookIntent::Report`].
        thread_key: String,
    },
    /// Nothing to do: an event the inbox ignores, or a payload it cannot read.
    Nothing,
}

/// The intent of the hook payload `payload` (the JSON the agent wrote to stdin) that `agent`
/// sent, mapped by the table in the module docs.
///
/// `git_head` is the content of `.git/HEAD` in the payload's `cwd`, read by the caller, or
/// `None` outside a repository; it gives the branch ([`branch_from_head`]). The project is the
/// last part of `cwd` (`None` when `cwd` is missing). Without `hook_event_name` or a non-empty
/// `session_id` the result is [`HookIntent::Nothing`]. Never fails and never panics.
pub fn interpret(agent: AgentKind, payload: &str, git_head: Option<&str>) -> HookIntent {
    let Ok(json) = serde_json::from_str::<Value>(payload) else {
        return HookIntent::Nothing;
    };
    let (Some(event), Some(session)) = (text(&json, "hook_event_name"), text(&json, "session_id")) else {
        return HookIntent::Nothing;
    };
    let thread_key = thread_key(agent, session);
    let report = |kind: Kind, body: Option<&str>| HookIntent::Report {
        kind,
        agent: AgentInfo {
            agent,
            session: session.into(),
            project: text(&json, "cwd").and_then(project_from_cwd),
            branch: git_head.and_then(branch_from_head),
            pids: Vec::new(),
        },
        thread_key: thread_key.clone(),
        body: body.map(str::trim).filter(|body| !body.is_empty()).map(String::from),
    };
    match (agent, event) {
        (AgentKind::ClaudeCode, "Notification") => match text(&json, "notification_type") {
            Some("permission_prompt" | "agent_needs_input" | "elicitation_dialog") => {
                report(Kind::AgentWaiting, text(&json, "message"))
            }
            _ => HookIntent::Nothing,
        },
        (AgentKind::ClaudeCode | AgentKind::Codex, "Stop") => {
            report(Kind::AgentDone, text(&json, "last_assistant_message"))
        }
        (AgentKind::ClaudeCode | AgentKind::Codex, "UserPromptSubmit" | "PostToolUse") => {
            HookIntent::ResolveWaiting { thread_key }
        }
        (AgentKind::ClaudeCode, "SessionEnd") => HookIntent::ResolveSession { thread_key },
        (AgentKind::Codex, "PermissionRequest") => {
            let reason = json.get("tool_input").and_then(|input| text(input, "description"));
            report(Kind::AgentWaiting, reason)
        }
        (AgentKind::Gemini, "Notification") => match text(&json, "notification_type") {
            Some("ToolPermission") => report(Kind::AgentWaiting, text(&json, "message")),
            _ => HookIntent::Nothing,
        },
        (AgentKind::Gemini, "AfterAgent") => report(Kind::AgentDone, text(&json, "prompt_response")),
        _ => HookIntent::Nothing,
    }
}

/// The thread key of a session: `<agent id>:<session id>`, e.g. `codex:thr_123`.
pub fn thread_key(agent: AgentKind, session: &str) -> String {
    format!("{}:{session}", agent.id())
}

/// The branch named by the content of `.git/HEAD`.
///
/// `ref: refs/heads/<branch>` gives `<branch>` (slashes kept, `feat/inbox`). A detached head (a
/// bare commit id, SHA-1 or SHA-256) gives its first 7 characters, as `git log --oneline` shows
/// it, so the user still sees where the agent works. Anything else, an empty branch name or a
/// ref outside `refs/heads/`, gives `None`. Surrounding whitespace is ignored.
pub fn branch_from_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let branch = reference.trim().strip_prefix("refs/heads/")?;
        return (!branch.is_empty()).then(|| branch.to_string());
    }
    let is_commit = matches!(head.len(), 40 | 64) && head.bytes().all(|b| b.is_ascii_hexdigit());
    is_commit.then(|| head[..7].to_string())
}

/// The last part of the working directory, e.g. `my-project` for `/home/user/my-project/`.
/// `None` for an empty path or a root.
fn project_from_cwd(cwd: &str) -> Option<String> {
    Path::new(cwd).file_name().and_then(|name| name.to_str()).map(String::from)
}

/// The string field `key` of `json`; `None` when it is missing, no string or empty.
fn text<'a>(json: &'a Value, key: &str) -> Option<&'a str> {
    json.get(key).and_then(Value::as_str).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "ref: refs/heads/feat/inbox\n";

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks").join(format!("{name}.json"));
        std::fs::read_to_string(&path).expect("fixture exists")
    }

    fn report(agent: AgentKind, session: &str, kind: Kind, body: &str) -> HookIntent {
        HookIntent::Report {
            kind,
            agent: AgentInfo {
                agent,
                session: session.into(),
                project: Some("my-project".into()),
                branch: Some("feat/inbox".into()),
                pids: Vec::new(),
            },
            thread_key: thread_key(agent, session),
            body: Some(body.into()),
        }
    }

    fn run(agent: AgentKind, name: &str) -> HookIntent {
        interpret(agent, &fixture(name), Some(HEAD))
    }

    #[test]
    fn claude_code_waits_on_permission_input_and_elicitation() {
        use AgentKind::ClaudeCode;
        let cases = [
            ("claude-code-notification-permission", "Claude needs your permission to use Bash"),
            ("claude-code-notification-needs-input", "Claude is waiting for your input"),
            ("claude-code-notification-elicitation", "The MCP server asks for a value"),
        ];
        for (name, body) in cases {
            assert_eq!(run(ClaudeCode, name), report(ClaudeCode, "abc123", Kind::AgentWaiting, body), "{name}");
        }
    }

    #[test]
    fn claude_code_stop_is_done() {
        let expected = report(
            AgentKind::ClaudeCode,
            "abc123",
            Kind::AgentDone,
            "I've completed the refactoring. Here's a summary...",
        );
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-stop"), expected);
    }

    #[test]
    fn claude_code_going_on_resolves_the_waiting() {
        let expected = HookIntent::ResolveWaiting { thread_key: "claude-code:abc123".into() };
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-user-prompt-submit"), expected);
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-post-tool-use"), expected);
    }

    #[test]
    fn claude_code_session_end_resolves_the_session() {
        let expected = HookIntent::ResolveSession { thread_key: "claude-code:abc123".into() };
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-session-end"), expected);
    }

    #[test]
    fn claude_code_ignores_idle_prompt_and_subagents() {
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-notification-idle"), HookIntent::Nothing);
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-subagent-stop"), HookIntent::Nothing);
    }

    #[test]
    fn codex_waits_finishes_and_goes_on() {
        use AgentKind::Codex;
        let waiting = report(Codex, "thr_123", Kind::AgentWaiting, "Run the test suite outside the sandbox");
        assert_eq!(run(Codex, "codex-permission-request"), waiting);
        let done =
            report(Codex, "thr_123", Kind::AgentDone, "All tests pass. I updated the parser and added two fixtures.");
        assert_eq!(run(Codex, "codex-stop"), done);
        let going_on = HookIntent::ResolveWaiting { thread_key: "codex:thr_123".into() };
        assert_eq!(run(Codex, "codex-user-prompt-submit"), going_on);
        assert_eq!(run(Codex, "codex-post-tool-use"), going_on);
    }

    #[test]
    fn gemini_waits_and_finishes() {
        use AgentKind::Gemini;
        let waiting = report(Gemini, "7f3a", Kind::AgentWaiting, "Gemini CLI wants to run: npm test");
        assert_eq!(run(Gemini, "gemini-notification"), waiting);
        let done =
            report(Gemini, "7f3a", Kind::AgentDone, "The test failed because of a missing import. It passes now.");
        assert_eq!(run(Gemini, "gemini-after-agent"), done);
    }

    #[test]
    fn events_outside_the_table_are_nothing() {
        assert_eq!(run(AgentKind::Codex, "codex-session-end"), HookIntent::Nothing);
        assert_eq!(run(AgentKind::Gemini, "gemini-before-agent"), HookIntent::Nothing);
        // A payload handed to the wrong agent's command: Codex has no `Notification`.
        assert_eq!(run(AgentKind::Codex, "claude-code-notification-permission"), HookIntent::Nothing);
        assert_eq!(run(AgentKind::Gemini, "claude-code-notification-permission"), HookIntent::Nothing);
        let unknown = r#"{"session_id":"s","hook_event_name":"SomethingNew"}"#;
        for agent in AgentKind::ALL {
            assert_eq!(interpret(agent, unknown, None), HookIntent::Nothing);
        }
    }

    #[test]
    fn broken_payloads_are_nothing() {
        let broken = [
            "",
            "not json",
            "{\"session_id\": \"abc\"",
            "[]",
            "null",
            r#"{"hook_event_name":"Stop"}"#,
            r#"{"hook_event_name":"Stop","session_id":""}"#,
            r#"{"hook_event_name":"Stop","session_id":42}"#,
            r#"{"session_id":"abc"}"#,
            r#"{"hook_event_name":"Notification","session_id":"abc"}"#,
        ];
        for agent in AgentKind::ALL {
            for payload in broken {
                assert_eq!(interpret(agent, payload, Some(HEAD)), HookIntent::Nothing, "{agent:?} {payload:?}");
            }
        }
    }

    #[test]
    fn missing_optional_fields_still_report() {
        let payload = r#"{"session_id":"abc","hook_event_name":"Stop","last_assistant_message":"  "}"#;
        let expected = HookIntent::Report {
            kind: Kind::AgentDone,
            agent: AgentInfo {
                agent: AgentKind::ClaudeCode,
                session: "abc".into(),
                project: None,
                branch: None,
                pids: Vec::new(),
            },
            thread_key: "claude-code:abc".into(),
            body: None,
        };
        assert_eq!(interpret(AgentKind::ClaudeCode, payload, None), expected);
    }

    #[test]
    fn branch_from_a_ref_and_from_a_detached_commit() {
        assert_eq!(branch_from_head("ref: refs/heads/main\n").as_deref(), Some("main"));
        assert_eq!(branch_from_head("ref: refs/heads/feat/ui-feedback").as_deref(), Some("feat/ui-feedback"));
        assert_eq!(branch_from_head("f98261431a2b7c9d0e1f2a3b4c5d6e7f80912345\n").as_deref(), Some("f982614"));
        let sha256 = "a".repeat(64);
        assert_eq!(branch_from_head(&sha256).as_deref(), Some("aaaaaaa"));
        for none in ["", "ref: refs/heads/", "ref: refs/tags/v1", "f982614", "not a head", &"g".repeat(40)] {
            assert_eq!(branch_from_head(none), None, "{none:?}");
        }
    }

    #[test]
    fn project_is_the_last_part_of_cwd() {
        assert_eq!(project_from_cwd("/home/user/my-project").as_deref(), Some("my-project"));
        assert_eq!(project_from_cwd("/home/user/my-project/").as_deref(), Some("my-project"));
        assert_eq!(project_from_cwd("/"), None);
        assert_eq!(project_from_cwd(""), None);
    }
}
