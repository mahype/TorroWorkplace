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
//! | Claude Code | `PermissionRequest` for `AskUserQuestion` | report `agent_waiting` with the questions |
//! | Codex | `PermissionRequest` | report `agent_waiting` (observed only, never answered) |
//! | Codex | `Stop` | report `agent_done` |
//! | Codex | `UserPromptSubmit`, `PostToolUse` | resolve waiting |
//! | Gemini CLI | `Notification` with `ToolPermission` | report `agent_waiting` |
//! | Gemini CLI | `AfterAgent` | report `agent_done` |
//! | Gemini CLI | `BeforeAgent` | resolve waiting |
//! | all three | `SessionEnd` | resolve session |
//!
//! Everything else is nothing, among it Claude Code's `idle_prompt` (its `Stop` already said the
//! turn is over) and `SubagentStop` (the session as a whole still runs).
//!
//! **Answering permission requests** (task E2.5): with `--answer`, `torroworkplace hook` puts the
//! `PermissionRequest` of Claude Code and Codex to the person as a question
//! ([`permission_question`]) and, once the person decided, prints the agent's decision JSON
//! ([`permission_answer`]). Gemini CLI has no hook that takes an answer, so it gets none.
//!
//! **A question is no permission** (task E3.8): Claude Code asks its `AskUserQuestion` through a
//! `PermissionRequest` too, but the answer is a choice in its terminal, which Allow or Deny cannot
//! give. So [`permission_question`] leaves it out, and [`interpret`] reports "waiting" with the
//! questions and their choices as text ([`questions_text`]).
//!
//! **The session's name** (task E2.6): Claude Code and Codex name every session, and the inbox
//! shows that name, so the person knows which job waits. Both keep it in files of their own, in
//! formats nobody documents: Claude Code in the transcript (`{"type":"custom-title"}` when the
//! person named the session, else `{"type":"ai-title"}`), Codex in its `session_index.jsonl`.
//! [`title_source`] says where to look; the caller reads the end of that file
//! ([`TITLE_TAIL_MAX`]) only for a report or a question, never for `PostToolUse`, and hands it to
//! [`title_in_transcript`] or [`title_in_codex_index`], which take the title and nothing else.
//! Whatever is missing or looks different gives no name, never an error; [`session_title`] makes
//! the name one harmless line of at most [`SESSION_TITLE_MAX`] characters. Gemini CLI names no
//! session.

use std::path::{Path, PathBuf};

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
        /// Agent, session, project and branch. `pids` is empty and `title` is `None`; the caller
        /// fills them (E2.2, E2.6: [`title_source`]).
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
            title: None,
        },
        thread_key: thread_key.clone(),
        body: body.map(str::trim).filter(|body| !body.is_empty()).map(String::from),
    };
    match (agent, event) {
        (AgentKind::ClaudeCode, "PermissionRequest") if text(&json, "tool_name") == Some(ASK_TOOL) => {
            let questions = json.get("tool_input").map(questions_text).unwrap_or_default();
            report(Kind::AgentWaiting, Some(&questions))
        }
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
        // A closed session waits for nobody, whichever agent it was.
        (_, "SessionEnd") => HookIntent::ResolveSession { thread_key },
        (AgentKind::Codex, "PermissionRequest") => {
            let reason = json.get("tool_input").and_then(|input| text(input, "description"));
            report(Kind::AgentWaiting, reason)
        }
        (AgentKind::Gemini, "Notification") => match text(&json, "notification_type") {
            Some("ToolPermission") => report(Kind::AgentWaiting, text(&json, "message")),
            _ => HookIntent::Nothing,
        },
        (AgentKind::Gemini, "AfterAgent") => report(Kind::AgentDone, text(&json, "prompt_response")),
        // Gemini has no prompt-submit event; a new turn starting means the person is back.
        (AgentKind::Gemini, "BeforeAgent") => HookIntent::ResolveWaiting { thread_key },
        _ => HookIntent::Nothing,
    }
}

/// Claude Code's tool that asks the person questions in its terminal (`AskUserQuestion`): no
/// permission, so never a question for Allow or Deny (task E3.8).
pub const ASK_TOOL: &str = "AskUserQuestion";

/// The questions of an `AskUserQuestion` input as text for the inbox: each question on a line,
/// its choices below it as `- label`, questions apart by an empty line. Empty when there are none.
pub fn questions_text(tool_input: &Value) -> String {
    let questions = tool_input.get("questions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    questions
        .iter()
        .filter_map(|question| {
            let mut lines = vec![text(question, "question")?.trim().to_string()];
            let options = question.get("options").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            lines.extend(
                options.iter().filter_map(|option| text(option, "label")).map(|label| format!("- {}", label.trim())),
            );
            Some(lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A coding agent's request for permission to use a tool, as a question for the person (task
/// E2.5, request `approvals.ask_permission`). Built by [`permission_question`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionQuestion {
    /// The tool's name as the agent gives it (`tool_name`): `Bash`, `Edit`, `apply_patch`,
    /// `mcp__fs__write`. Not checked here; the core refuses names it does not take.
    pub tool: String,
    /// What the tool would do, in full and unchanged ([`permission_details`]). The core shows it
    /// to the person without shortening it.
    pub details: String,
    /// The last part of the payload's `cwd`; `None` without one.
    pub project: Option<String>,
    /// The session's name (task E2.6, [`session_title`]), so the person knows which job asks.
    /// [`permission_question`] leaves it `None`; the caller reads it ([`title_source`]) and fills
    /// it in.
    pub title: Option<String>,
}

/// The question in the `PermissionRequest` payload `payload` of `agent`; `None` for every other
/// event, for Gemini CLI (it has no `PermissionRequest` hook to answer), for Claude Code's
/// [`ASK_TOOL`] (a question, no permission), for broken JSON and without a non-empty `tool_name`.
/// Never fails and never panics.
///
/// Both agents send `tool_name` and `tool_input` (Claude Code: hooks reference, "PermissionRequest
/// input"; Codex: hooks reference, "PermissionRequest"); `session_id` is not needed, a question
/// is bound to its approval id.
pub fn permission_question(agent: AgentKind, payload: &str) -> Option<PermissionQuestion> {
    if agent == AgentKind::Gemini {
        return None;
    }
    let json = serde_json::from_str::<Value>(payload).ok()?;
    if text(&json, "hook_event_name") != Some("PermissionRequest") {
        return None;
    }
    let tool = text(&json, "tool_name")?.to_string();
    if agent == AgentKind::ClaudeCode && tool == ASK_TOOL {
        return None;
    }
    let details = json.get("tool_input").map(permission_details).unwrap_or_default();
    Some(PermissionQuestion { tool, details, project: text(&json, "cwd").and_then(project_from_cwd), title: None })
}

/// What a tool's input means for the person: the shell command alone when `tool_input` holds
/// nothing but `command` (a string, or a list of words joined with spaces) and maybe the agent's
/// own `description` of it (Claude Code's and Codex's plain `Bash`, Codex's `apply_patch`, whose
/// command is the patch). Any other input is shown whole as compact JSON, `description` included:
/// an edit's file, old and new text; an MCP tool's arguments; and a command with more fields
/// (`dangerouslyDisableSandbox`, a host, a working directory), since those may be the very reason
/// for the question. Nothing is left out or cut: a long input is the core's to refuse, and the
/// agent then asks in its own terminal.
pub fn permission_details(tool_input: &Value) -> String {
    let only_a_command =
        tool_input.as_object().is_some_and(|fields| fields.keys().all(|key| key == "command" || key == "description"));
    match tool_input.get("command") {
        Some(Value::String(command)) if only_a_command => command.clone(),
        Some(Value::Array(words)) if only_a_command && words.iter().all(Value::is_string) => {
            words.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")
        }
        _ => serde_json::to_string(tool_input).unwrap_or_default(),
    }
}

/// What the hook tells `agent` when the person denied, in [`permission_answer`]'s `message`:
/// the agent reads it as the reason. English like the API's descriptions, since agents read it.
pub const DENIED_MESSAGE: &str = "The person denied this in Torro Workplace.";

/// The JSON the hook prints on stdout for `agent` when the person allowed (`allow`) or denied a
/// permission request, as one line; `None` for Gemini CLI, which takes no answer.
///
/// The same shape for Claude Code (hooks reference, "PermissionRequest decision control") and
/// Codex (hooks reference, "PermissionRequest", output): `hookSpecificOutput` with
/// `hookEventName: "PermissionRequest"` and `decision.behavior` `allow` or `deny`, a denial with
/// `message` ([`DENIED_MESSAGE`]). Nothing else: no `updatedInput`, `updatedPermissions` or
/// `interrupt` (Codex fails closed on them today, and the person allowed this one request only).
pub fn permission_answer(agent: AgentKind, allow: bool) -> Option<String> {
    let decision = match allow {
        true => serde_json::json!({ "behavior": "allow" }),
        false => serde_json::json!({ "behavior": "deny", "message": DENIED_MESSAGE }),
    };
    match agent {
        AgentKind::ClaudeCode | AgentKind::Codex => Some(
            serde_json::json!({
                "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision }
            })
            .to_string(),
        ),
        AgentKind::Gemini => None,
    }
}

/// The longest session name kept, in characters ([`session_title`]); a longer one ends in "…".
pub const SESSION_TITLE_MAX: usize = 80;

/// How much of the end of a transcript or of Codex's index is read for a session's name: 256 KiB.
/// Claude Code writes its title line again and again while a session runs, and Codex appends to
/// its index, so the name stands near the end; reading more would only slow the hook down.
pub const TITLE_TAIL_MAX: u64 = 256 * 1024;

/// Where the name of a session is kept (task E2.6), from [`title_source`]. The caller reads the
/// last [`TITLE_TAIL_MAX`] bytes of the file and hands them to the parser named here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleSource {
    /// Claude Code: the session's transcript, a JSONL file (`transcript_path` of the payload);
    /// its end goes to [`title_in_transcript`].
    Transcript(PathBuf),
    /// Codex: `session_index.jsonl` in `$CODEX_HOME` (default `~/.codex`), which the caller
    /// finds; its end goes to [`title_in_codex_index`] with `session`.
    CodexIndex {
        /// The payload's `session_id`, the `id` of the session's lines in the index.
        session: String,
    },
}

/// Where to look for the name of the session in `payload` of `agent`: Claude Code's
/// `transcript_path` (absolute, not empty), Codex's index for its `session_id` (not empty).
/// `None` for Gemini CLI (it names no session), for broken JSON and without the field. Looks at
/// no event and reads no file: the caller asks only for reports and questions.
pub fn title_source(agent: AgentKind, payload: &str) -> Option<TitleSource> {
    let json = serde_json::from_str::<Value>(payload).ok()?;
    match agent {
        AgentKind::ClaudeCode => {
            let path = PathBuf::from(text(&json, "transcript_path")?);
            path.is_absolute().then_some(TitleSource::Transcript(path))
        }
        AgentKind::Codex => Some(TitleSource::CodexIndex { session: text(&json, "session_id")?.to_string() }),
        AgentKind::Gemini => None,
    }
}

/// The session's name in `tail`, the end of a Claude Code transcript: the last
/// `{"type":"custom-title","customTitle":…}` (the name the person gave, renaming adds a line),
/// else the last `{"type":"ai-title","aiTitle":…}`, made harmless by [`session_title`]. Only these
/// lines are read, nothing else of the conversation. Broken lines (the first one is usually cut
/// in half) are skipped; `None` when no line names the session.
pub fn title_in_transcript(tail: &str) -> Option<String> {
    let (mut custom, mut generated) = (None, None);
    // A transcript is mostly messages; only lines that may hold a title are parsed at all.
    for line in tail.lines().filter(|line| line.contains("-title")) {
        let Ok(json) = serde_json::from_str::<Value>(line) else { continue };
        let (slot, field) = match text(&json, "type") {
            Some("custom-title") => (&mut custom, "customTitle"),
            Some("ai-title") => (&mut generated, "aiTitle"),
            _ => continue,
        };
        if let Some(title) = text(&json, field).and_then(session_title) {
            *slot = Some(title);
        }
    }
    custom.or(generated)
}

/// The name of the Codex session `session` in `tail`, the end of Codex's `session_index.jsonl`:
/// `thread_name` of the last line whose `id` is `session` (renaming adds a line), made harmless by
/// [`session_title`]. Broken lines are skipped; `None` when no line is about the session or its
/// last one has no name.
pub fn title_in_codex_index(tail: &str, session: &str) -> Option<String> {
    // From the end: the newest line of the session is the one that counts.
    tail.lines()
        .rev()
        .filter(|line| line.contains(session))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|json| text(json, "id") == Some(session))
        .and_then(|json| text(&json, "thread_name").and_then(session_title))
}

/// `raw`, a session's name from the agent's files, as one harmless line: control characters
/// (`\u{0e}` was seen in a Codex name), direction and invisible format characters go, every run
/// of white space becomes one space, the ends are trimmed, and a name longer than
/// [`SESSION_TITLE_MAX`] characters is cut to that length, ending in "…". `None` when nothing is
/// left. The core applies it again to what a hook sends.
pub fn session_title(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| !c.is_control() && !is_invisible(c))
        .collect();
    let line = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= SESSION_TITLE_MAX {
        return Some(line);
    }
    let mut cut: String = line.chars().take(SESSION_TITLE_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    Some(cut)
}

/// Characters that show nothing yet change what a person reads: the bidi controls (they turn
/// text around), zero-width spaces and joiners, the byte order mark.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
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
                title: None,
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
    fn claude_code_asking_questions_waits_with_the_questions_and_is_no_permission() {
        let body = "Where should the fix be committed?\n- Own branch (Recommended)\n- Current branch\n\n\
                    Open a pull request?\n- Yes\n- No";
        assert_eq!(
            run(AgentKind::ClaudeCode, "claude-code-ask-user-question"),
            report(AgentKind::ClaudeCode, "abc123", Kind::AgentWaiting, body)
        );
        assert_eq!(permission_question(AgentKind::ClaudeCode, &fixture("claude-code-ask-user-question")), None);
        // Any other tool's request stays a question, and its plain hook reports nothing.
        assert!(permission_question(AgentKind::ClaudeCode, &fixture("claude-code-permission-request")).is_some());
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-permission-request"), HookIntent::Nothing);
    }

    #[test]
    fn questions_without_choices_or_text_stay_readable() {
        let input = serde_json::json!({ "questions": [{ "question": " Go on? " }, { "header": "no question" }] });
        assert_eq!(questions_text(&input), "Go on?");
        assert_eq!(questions_text(&serde_json::json!({})), "");
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
    fn gemini_waits_finishes_and_goes_on() {
        use AgentKind::Gemini;
        let waiting = report(Gemini, "7f3a", Kind::AgentWaiting, "Gemini CLI wants to run: npm test");
        assert_eq!(run(Gemini, "gemini-notification"), waiting);
        let done =
            report(Gemini, "7f3a", Kind::AgentDone, "The test failed because of a missing import. It passes now.");
        assert_eq!(run(Gemini, "gemini-after-agent"), done);
        let going_on = HookIntent::ResolveWaiting { thread_key: "gemini:7f3a".into() };
        assert_eq!(run(Gemini, "gemini-before-agent"), going_on);
    }

    #[test]
    fn every_agent_resolves_its_session_on_end() {
        let ended = |key: &str| HookIntent::ResolveSession { thread_key: key.into() };
        assert_eq!(run(AgentKind::Codex, "codex-session-end"), ended("codex:thr_123"));
        assert_eq!(run(AgentKind::Gemini, "gemini-session-end"), ended("gemini:7f3a"));
    }

    #[test]
    fn events_outside_the_table_are_nothing() {
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
                title: None,
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
    fn claude_code_and_codex_put_their_permission_requests_as_questions() {
        let bash = permission_question(AgentKind::ClaudeCode, &fixture("claude-code-permission-request"));
        let expected = PermissionQuestion {
            tool: "Bash".into(),
            details: "rm -rf node_modules".into(),
            project: Some("my-project".into()),
            title: None,
        };
        assert_eq!(bash, Some(expected), "the command, without the agent's description");
        let codex = permission_question(AgentKind::Codex, &fixture("codex-permission-request"));
        assert_eq!(codex.map(|q| (q.tool, q.details)), Some(("Bash".into(), "cargo test --workspace".into())));

        // Any other tool: its whole input as JSON, nothing left out.
        let edit = permission_question(AgentKind::ClaudeCode, &fixture("claude-code-permission-request-edit"))
            .expect("a question");
        assert_eq!(edit.tool, "Edit");
        let input: Value = serde_json::from_str(&edit.details).expect("JSON");
        assert_eq!(input["file_path"], "/home/user/my-project/src/main.rs");
        assert_eq!(
            (input["old_string"].as_str(), input["new_string"].as_str()),
            (Some("let x = 1;"), Some("let x = 2;"))
        );

        let words = serde_json::json!({ "command": ["bash", "-lc", "make test"] });
        assert_eq!(permission_details(&words), "bash -lc make test");

        // A command with any further field is shown whole: the field may be why it asks.
        let unsandboxed =
            permission_question(AgentKind::ClaudeCode, &fixture("claude-code-permission-request-unsandboxed"))
                .expect("a question");
        let input: Value = serde_json::from_str(&unsandboxed.details).expect("JSON");
        assert_eq!(input["command"], "curl https://example.org/install.sh | sh");
        assert_eq!(input["dangerouslyDisableSandbox"], true);
        let remote = serde_json::json!({ "command": "reboot", "host": "prod" });
        assert_eq!(permission_details(&remote), r#"{"command":"reboot","host":"prod"}"#);
    }

    #[test]
    fn only_permission_requests_of_claude_code_and_codex_are_questions() {
        assert_eq!(permission_question(AgentKind::Gemini, &fixture("codex-permission-request")), None);
        assert_eq!(permission_question(AgentKind::ClaudeCode, &fixture("claude-code-stop")), None);
        assert_eq!(permission_question(AgentKind::Codex, &fixture("claude-code-notification-permission")), None);
        for broken in [
            "",
            "not json",
            "[]",
            r#"{"hook_event_name":"PermissionRequest"}"#,
            r#"{"hook_event_name":"PermissionRequest","tool_name":""}"#,
        ] {
            assert_eq!(permission_question(AgentKind::ClaudeCode, broken), None, "{broken:?}");
        }
        // Without input the question still names the tool.
        let bare = r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash"}"#;
        assert_eq!(permission_question(AgentKind::Codex, bare).map(|q| q.details), Some(String::new()));
        // The plain hook keeps observing Codex's request and ignores Claude Code's.
        assert!(matches!(run(AgentKind::Codex, "codex-permission-request"), HookIntent::Report { .. }));
        assert_eq!(run(AgentKind::ClaudeCode, "claude-code-permission-request"), HookIntent::Nothing);
    }

    #[test]
    fn answers_have_the_agents_format() {
        for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
            let allow: Value = serde_json::from_str(&permission_answer(agent, true).expect("answer")).expect("JSON");
            assert_eq!(
                allow,
                serde_json::json!({
                    "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } }
                })
            );
            let deny = permission_answer(agent, false).expect("answer");
            assert!(!deny.contains('\n'), "one line");
            let deny: Value = serde_json::from_str(&deny).expect("JSON");
            assert_eq!(deny["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
            assert_eq!(
                deny["hookSpecificOutput"]["decision"],
                serde_json::json!({ "behavior": "deny", "message": DENIED_MESSAGE })
            );
        }
        assert_eq!(permission_answer(AgentKind::Gemini, true), None);
    }

    fn fixture_file(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks").join(name);
        std::fs::read_to_string(&path).expect("fixture exists")
    }

    #[test]
    fn the_last_custom_title_names_a_claude_code_session() {
        // Renamed once: the later name wins over the earlier one and over every ai-title, also
        // one that comes after it; agent names and messages that mention a title are no title.
        let transcript = fixture_file("claude-code-transcript.jsonl");
        assert_eq!(title_in_transcript(&transcript).as_deref(), Some("Login redirect (staging)"));
    }

    #[test]
    fn without_a_custom_title_the_last_ai_title_names_it() {
        let transcript = fixture_file("claude-code-transcript-ai-title.jsonl");
        assert_eq!(title_in_transcript(&transcript).as_deref(), Some("Update README install steps"));
    }

    #[test]
    fn broken_lines_in_a_transcript_are_skipped() {
        // The first line is cut in half (the tail starts mid-line), others are no JSON, hold no
        // string or a blank one: only the one good title counts.
        let transcript = fixture_file("claude-code-transcript-broken.jsonl");
        assert_eq!(title_in_transcript(&transcript).as_deref(), Some("Release notes"));
        for none in ["", "\n\n", "not json", r#"{"type":"user","message":"custom-title"}"#] {
            assert_eq!(title_in_transcript(none), None, "{none:?}");
        }
    }

    #[test]
    fn the_last_line_of_a_codex_session_names_it() {
        let index = fixture_file("codex-session-index.jsonl");
        assert_eq!(title_in_codex_index(&index, "thr_123").as_deref(), Some("Parser fixtures and docs"));
        assert_eq!(title_in_codex_index(&index, "thr_1234").as_deref(), Some("Not this one"), "the whole id");
        // The control character Codex left in a name goes.
        assert_eq!(title_in_codex_index(&index, "thr_200").as_deref(), Some("Old name"));
        // The last line of a session has no name (the line before it is broken): no name.
        assert_eq!(title_in_codex_index(&index, "thr_300"), None);
        assert_eq!(title_in_codex_index(&index, "thr_999"), None);
        assert_eq!(title_in_codex_index("", "thr_123"), None);
    }

    #[test]
    fn titles_are_looked_for_where_each_agent_keeps_them() {
        // The fixture's path is absolute on Unix only; Windows wants a drive, so there it is no
        // transcript to read.
        let transcript = PathBuf::from("/home/user/.claude/projects/-home-user-my-project/abc123.jsonl");
        let found = title_source(AgentKind::ClaudeCode, &fixture("claude-code-stop"));
        if transcript.is_absolute() {
            assert_eq!(found, Some(TitleSource::Transcript(transcript)));
        } else {
            assert_eq!(found, None, "not absolute on this system");
        }
        // An absolute path of this system is taken as it is.
        let here = std::env::temp_dir().join("abc123.jsonl");
        let payload = serde_json::json!({ "session_id": "abc", "transcript_path": here.to_string_lossy() }).to_string();
        assert_eq!(title_source(AgentKind::ClaudeCode, &payload), Some(TitleSource::Transcript(here)));
        assert_eq!(
            title_source(AgentKind::Codex, &fixture("codex-stop")),
            Some(TitleSource::CodexIndex { session: "thr_123".into() })
        );
        // Gemini CLI names no session, though it sends a transcript.
        assert_eq!(title_source(AgentKind::Gemini, &fixture("gemini-after-agent")), None);
        for broken in ["", "not json", r#"{"session_id":"abc"}"#, r#"{"transcript_path":"relative.jsonl"}"#] {
            assert_eq!(title_source(AgentKind::ClaudeCode, broken), None, "{broken:?}");
        }
        assert_eq!(title_source(AgentKind::Codex, r#"{"session_id":""}"#), None);
    }

    #[test]
    fn session_titles_are_one_harmless_line_of_at_most_80_characters() {
        assert_eq!(session_title("  Fix\tthe\n\nlogin  bug ").as_deref(), Some("Fix the login bug"));
        assert_eq!(session_title("Build\u{0e} fix\u{1b}[31m").as_deref(), Some("Build fix[31m"));
        assert_eq!(session_title("abc\u{202E}fed\u{200B}\u{2066}x\u{FEFF}").as_deref(), Some("abcfedx"));
        for none in ["", "   ", "\u{0e}\u{202E}\n"] {
            assert_eq!(session_title(none), None, "{none:?}");
        }
        let exact = "x".repeat(SESSION_TITLE_MAX);
        assert_eq!(session_title(&exact).as_deref(), Some(exact.as_str()));
        let long = session_title(&"ä".repeat(200)).expect("a title");
        assert_eq!(long.chars().count(), SESSION_TITLE_MAX);
        assert!(long.ends_with('…'), "{long}");
        // No blank before the "…".
        let words = session_title(&format!("{} tail", "y".repeat(SESSION_TITLE_MAX - 2))).expect("a title");
        assert_eq!(words, format!("{}…", "y".repeat(SESSION_TITLE_MAX - 2)));
    }

    #[test]
    fn interpreting_never_reads_a_title() {
        let HookIntent::Report { agent, .. } = run(AgentKind::ClaudeCode, "claude-code-stop") else {
            panic!("a report");
        };
        assert_eq!(agent.title, None, "the caller fills it");
    }

    #[test]
    fn project_is_the_last_part_of_cwd() {
        assert_eq!(project_from_cwd("/home/user/my-project").as_deref(), Some("my-project"));
        assert_eq!(project_from_cwd("/home/user/my-project/").as_deref(), Some("my-project"));
        assert_eq!(project_from_cwd("/"), None);
        assert_eq!(project_from_cwd(""), None);
    }
}
