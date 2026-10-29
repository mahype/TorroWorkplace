//! Tests of engine B with a fake `claude`: a shell script that writes down how it was started
//! (arguments, the key in its environment, its input, the MCP configuration and its mode) and
//! then plays a recorded `stream-json` transcript. No test needs a real agent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use torro_core::ai::{AiError, BoxFuture};
use torro_core::i18n::{tr, trf};
use torro_ipc::api::{ASSISTANT_CLIENT, Area, AreaRights, Lack, Level, Rights, Selection};
use torro_ipc::assistant::{ConversationSummary, Engine, EngineKind, Entry, EntryContent};
use torro_ipc::{ClientInfo, Key, Request, Response};

use super::super::tests::assistant_in;
use super::super::{Assistant, Message, ModelAccess};
use super::*;
use crate::clients::{Caller, Clients};

/// The core as engine B sees it: the assistant's client in a real gate, which lends the keys.
struct FakeHost {
    clients: Clients,
    /// The areas every key was lent without.
    lent: Mutex<Vec<Vec<Area>>>,
    /// The person let mail go to cloud models (task C.17).
    mail_to_cloud: std::sync::atomic::AtomicBool,
    /// The assistant's own tools a person switched on (issue #225).
    tools_on: Mutex<std::collections::BTreeSet<torro_core::settings::ToolId>>,
}

impl Host for FakeHost {
    fn client(&self) -> Option<ClientInfo> {
        self.clients.list().into_iter().find(|info| info.kind == ASSISTANT_CLIENT)
    }

    fn model<'a>(&'a self, _engine: &'a str, _model: Option<&'a str>) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
        Box::pin(async { Err(AiError::NotFound("no AI accounts here".into())) })
    }

    fn run<'a>(&'a self, _request: Request, _engine: &'a str, _untrusted: bool) -> BoxFuture<'a, Response> {
        Box::pin(async { Response::Ok })
    }

    fn approval<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Response> {
        Box::pin(async { Response::Error { code: "not-found".into(), message: "none".into() } })
    }

    fn data_rule(&self) -> DataRule {
        DataRule { mail_to_cloud: self.mail_to_cloud.load(std::sync::atomic::Ordering::SeqCst) }
    }

    fn tools_on(&self) -> std::collections::BTreeSet<torro_core::settings::ToolId> {
        self.tools_on.lock().expect("lock").clone()
    }

    fn lend(&self, engine: &str, without: &[Area]) -> Option<crate::clients::LentKey> {
        self.lent.lock().expect("lock").push(without.to_vec());
        let client = self.client()?;
        self.clients.lend(&client.id, engine, without).ok()
    }
}

/// A test's folder: `bin/claude` (when installed), what it wrote down, the runs' folders.
struct Setup {
    dir: tempfile::TempDir,
    assistant: Arc<Assistant>,
    host: Arc<FakeHost>,
}

impl Setup {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
    }

    /// The key the fake agent found in its environment.
    fn token(&self) -> Key {
        Key::new(self.read("token"))
    }

    /// The arguments the fake agent was started with, one per line.
    fn arguments(&self) -> Vec<String> {
        self.read("args").lines().map(str::to_string).collect()
    }
}

/// An assistant with Claude Code installed as `bin/claude` playing `transcript` and then `tail`
/// (shell), stopped after `idle` without output; the assistant's client exists when `client`.
async fn set_up(transcript: &[Value], tail: &str, idle: Duration, client: bool) -> Setup {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    let lines: Vec<String> = transcript.iter().map(Value::to_string).collect();
    std::fs::write(root.join("transcript.jsonl"), lines.join("\n") + "\n").expect("transcript");
    let script = format!(
        r#"d='{dir}'
printf '%s\n' "$@" > "$d/args"
printf '%s' "$TORRO_TOKEN" > "$d/token"
echo $$ > "$d/pid"
while [ $# -gt 0 ]; do
  if [ "$1" = "--mcp-config" ]; then cp "$2" "$d/mcp.json"; ls -l "$2" | cut -c1-10 > "$d/mode"; fi
  shift
done
cat > "$d/stdin"
cat "$d/transcript.jsonl"
{tail}
"#,
        dir = root.display()
    );
    crate::apps::tests::fake_at(&root.join("bin").join("claude"), &script);
    install(dir, idle, client).await
}

/// The assistant on `dir`, its agents found in `dir/bin`.
async fn install(dir: tempfile::TempDir, idle: Duration, client: bool) -> Setup {
    let root = dir.path();
    let env = Env {
        home: root.join("home"),
        app_config: root.join("home").join(".config"),
        path: vec![root.join("bin")],
        program: root.join("torroworkplace"),
        config_dir: root.join("config"),
    };
    let agents = Agents::new(env, root.join("runs"), idle, Duration::from_secs(30));
    let assistant = Arc::new(assistant_in(root, Duration::ZERO).with_agents(agents));
    let clients = crate::clients::tests::open_in(root);
    if client {
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        clients.add("Assistant", ASSISTANT_CLIENT, rights).await.expect("the assistant's client");
    }
    let host = Arc::new(FakeHost {
        clients,
        lent: Mutex::new(Vec::new()),
        mail_to_cloud: Default::default(),
        tools_on: Default::default(),
    });
    let weak = Arc::downgrade(&host);
    let weak: std::sync::Weak<dyn Host> = weak;
    assistant.attach(weak);
    Setup { dir, assistant, host }
}

async fn ask(setup: &Setup, conversation: Option<&str>, text: &str, without: Vec<Area>) -> String {
    let message = Message {
        conversation: conversation.map(str::to_string),
        engine: "claude-code".into(),
        model: None,
        effort: None,
        text: text.into(),
        without,
        mode: torro_ipc::assistant::Mode::Default,
        bot: None,
        on_behalf: None,
        from: Some(crate::assistant::TEST_CLIENT.into()),
    };
    match setup.assistant.send(message, EngineKind::Agent, chrono::Utc::now()).await {
        Response::Conversation { conversation, .. } => conversation.id,
        other => panic!("expected a conversation, got {other:?}"),
    }
}

async fn read(assistant: &Assistant, id: &str, after: Option<u64>) -> (ConversationSummary, Vec<Entry>) {
    match assistant.get(id, after).await {
        Response::Conversation { conversation, entries } => (conversation, entries),
        other => panic!("expected a conversation, got {other:?}"),
    }
}

/// Reads conversation `id` until no answer runs, at most twenty seconds.
async fn finished(assistant: &Assistant, id: &str) -> Vec<Entry> {
    for _ in 0..2000 {
        let (summary, entries) = read(assistant, id, None).await;
        if !summary.running {
            return entries;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the answer of {id} never finished");
}

fn contents(entries: &[Entry]) -> Vec<EntryContent> {
    entries.iter().map(|entry| entry.content.clone()).collect()
}

fn init() -> Value {
    json!({"type": "system", "subtype": "init", "model": "claude-sonnet-4-5",
           "tools": ["mcp__torroworkplace__calendar_list_calendars"],
           "mcp_servers": [{"name": "torroworkplace", "status": "connected"}]})
}

fn piece(text: &str) -> Value {
    json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0,
           "delta": {"type": "text_delta", "text": text}}})
}

fn says(text: &str) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]},
           "parent_tool_use_id": null})
}

fn calls(id: &str, tool: &str, input: Value) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant",
           "content": [{"type": "tool_use", "id": id, "name": format!("mcp__torroworkplace__{tool}"), "input": input}]},
           "parent_tool_use_id": null})
}

fn result_of(id: &str, text: &str, error: bool) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
           "content": [{"type": "text", "text": text}], "is_error": error}]}, "parent_tool_use_id": null})
}

fn done(text: &str) -> Value {
    json!({"type": "result", "subtype": "success", "is_error": false, "result": text, "session_id": "s1"})
}

/// The process `pid` is gone or a zombie.
fn gone(pid: &str) -> bool {
    let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output().expect("ps");
    let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
    stat.is_empty() || stat.starts_with('Z')
}

#[tokio::test]
async fn claude_code_is_an_engine_only_when_installed_and_the_assistant_has_its_client() {
    let claude = Engine {
        id: "claude-code".into(),
        kind: EngineKind::Agent,
        name: "Claude Code".into(),
        problem: None,
        models: claude::models(),
    };
    let ai = crate::account::ai::AiAccounts::new(
        None,
        Arc::new(crate::account::github::tests::MemoryTokens::default()),
        None,
        Vec::new(),
    );

    let installed = set_up(&[], "", Duration::from_secs(5), true).await;
    assert!(installed.assistant.engines(&ai).await.contains(&claude));
    assert!(installed.assistant.refusal(&ai, &send_to_claude()).await.is_none());

    let no_client = set_up(&[], "", Duration::from_secs(5), false).await;
    assert!(!no_client.assistant.engines(&ai).await.contains(&claude), "no client, nothing to run with");
    let refused = no_client.assistant.refusal(&ai, &send_to_claude()).await;
    assert!(matches!(refused, Some(Response::Error { ref code, .. }) if code == "not-found"), "{refused:?}");

    let missing = install(tempfile::tempdir().expect("temp dir"), Duration::from_secs(5), true).await;
    assert!(!missing.assistant.engines(&ai).await.contains(&claude), "not installed, not offered");
    let refused = missing.assistant.refusal(&ai, &send_to_claude()).await;
    assert!(matches!(refused, Some(Response::Error { ref code, .. }) if code == "not-found"), "{refused:?}");
}

/// Task C.18, issue #219: the product does not offer agents. The command is where `locate` looks
/// and the assistant has its client, yet nothing is listed and no engine id finds an agent. Only
/// the test constructor offers them, which is why every test here still runs.
#[test]
fn agents_that_are_not_offered_are_no_engines_even_when_installed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    crate::apps::tests::fake_at(&root.join("bin").join("claude"), "exit 0\n");
    let env = Env {
        home: root.join("home"),
        app_config: root.join("home").join(".config"),
        path: vec![root.join("bin")],
        program: root.join("torroworkplace"),
        config_dir: root.join("config"),
    };
    let runs = root.join("runs");
    let (idle, total) = (Duration::from_secs(5), Duration::from_secs(30));

    let offered = Agents::new(env.clone(), runs.clone(), idle, total);
    assert_eq!(offered.engines(true).len(), 1, "the fake command is where locate looks");
    assert_eq!(offered.find("claude-code"), Some(Agent::ClaudeCode));

    let agents = Agents::new(env, runs, idle, total).not_offered();
    assert!(agents.engines(true).is_empty(), "the product offers no agent (#219)");
    assert!(agents.find("claude-code").is_none(), "and no engine id finds one");
    assert_eq!(Agent::of("claude-code"), Some(Agent::ClaudeCode), "the agent itself stays, for never_given");
    assert_eq!(never_given("claude-code"), [Area::Mail], "so an old conversation keeps its data rule");

    // The product's own constructor, so rebuilding `engines` on `Agent::ALL` cannot slip past this
    // test. The flag is read before anything is looked up, so no home directory is needed.
    assert!(Agents::system().engines(true).is_empty(), "the user's agents are not offered either");
    assert!(Agents::system().find("claude-code").is_none());
}

fn send_to_claude() -> Request {
    Request::SendMessage {
        conversation: None,
        engine: "claude-code".into(),
        model: None,
        effort: None,
        text: "Hi".into(),
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
        bot: None,
    }
}

/// Issue #225: a command of one of the assistant's own tools refused with `forbidden` says why on
/// its tool line, as in engine A: the rights first, then the tool switched off, else not in the
/// client's selection.
#[tokio::test]
async fn a_refused_tool_says_whether_it_is_off_or_not_granted() {
    use torro_core::settings::ToolId;
    let input = json!({"title": "Offer", "markdown": "# Offer"});
    let transcript = [
        init(),
        calls("t1", "tools_create_pdf", input.clone()),
        result_of("t1", "forbidden: this client may not use tools.create_pdf", true),
        done("It did not work."),
    ];
    let setup = set_up(&transcript, "", Duration::from_secs(10), true).await;
    let client = setup.host.client().expect("the assistant's client");
    let with_tools = |tools: Selection| {
        let mut rights = client.rights.clone();
        rights.areas.insert(Area::Tools, AreaRights { level: Level::Write, admin: false });
        rights.tools = tools;
        rights
    };
    let line_of = |lack: Lack| EntryContent::Tool {
        name: "tools.create_pdf".into(),
        input: input.clone(),
        summary: tools::lack_summary(lack, "Claude Code"),
        error: Some("forbidden".into()),
        lack: Some(lack),
    };

    // Not granted and switched off: the rights say why, not the switch.
    let id = ask(&setup, None, "Make a PDF", Vec::new()).await;
    let entries = contents(&finished(&setup.assistant, &id).await);
    assert!(entries.contains(&line_of(Lack::Area { area: Area::Tools })), "{entries:?}");

    // Granted, but no person switched it on.
    setup.host.clients.update(&client.id, with_tools(Selection::All)).await.expect("rights");
    let id = ask(&setup, None, "Make a PDF", Vec::new()).await;
    let entries = contents(&finished(&setup.assistant, &id).await);
    assert!(entries.contains(&line_of(Lack::ToolOff { tool: ToolId::Pdf })), "{entries:?}");

    // Switched on, but not in the selection.
    setup.host.tools_on.lock().expect("lock").insert(ToolId::Pdf);
    setup.host.clients.update(&client.id, with_tools(Selection::Only(Vec::new()))).await.expect("rights");
    let id = ask(&setup, None, "Make a PDF", Vec::new()).await;
    let entries = contents(&finished(&setup.assistant, &id).await);
    assert!(entries.contains(&line_of(Lack::Tool { tool: ToolId::Pdf })), "{entries:?}");
}

#[tokio::test]
async fn the_stream_becomes_answers_and_tool_lines_and_the_key_stays_in_the_environment() {
    let collections = serde_json::to_string(&Response::Collections { collections: Vec::new() }).expect("json");
    let transcript = [
        init(),
        piece("Let me "),
        piece("look."),
        says("Let me look."),
        calls("t1", "calendar_list_calendars", json!({})),
        result_of("t1", &collections, false),
        calls("t2", "mail_list_folders", json!({"account": "mail:anna@example.org"})),
        result_of("t2", "forbidden: this client may not use mail.list_folders", true),
        says("You have no calendars."),
        done("You have no calendars."),
    ];
    let setup = set_up(&transcript, "", Duration::from_secs(10), true).await;
    let id = ask(&setup, None, "Which calendars do I have?", vec![Area::Tasks]).await;
    let entries = finished(&setup.assistant, &id).await;

    let answer = |text: &str| EntryContent::Answer {
        text: text.into(),
        engine: "claude-code".into(),
        model: Some("claude-sonnet-4-5".into()),
    };
    assert_eq!(
        contents(&entries)[1..],
        [
            answer("Let me look."),
            EntryContent::Tool {
                name: "calendar.list_calendars".into(),
                input: json!({}),
                summary: trf("{count} results", &[("count", &0)]),
                error: None,
                lack: None,
            },
            EntryContent::Tool {
                name: "mail.list_folders".into(),
                input: json!({"account": "mail:anna@example.org"}),
                // Issue #133: the line says why, and that no change of rights helps.
                summary: trf(
                    "{engine} does not get {area} (data rule)",
                    &[("engine", &"Claude Code"), ("area", &tr("Mail"))]
                ),
                error: Some("forbidden".into()),
                lack: Some(Lack::NeverToAgents { area: Area::Mail }),
            },
            answer("You have no calendars."),
        ]
    );

    // How it was started: the suite's tools only, the prompt of C.3, the message on its input.
    let arguments = setup.arguments();
    let after = |flag: &str| arguments.iter().position(|a| a == flag).map(|at| arguments[at + 1].clone());
    assert_eq!(after("--tools").as_deref(), Some(""));
    assert_eq!(after("--allowedTools").as_deref(), Some("mcp__torroworkplace__*"));
    assert!(arguments.contains(&"--strict-mcp-config".to_string()));
    let prompt = arguments.iter().find_map(|a| a.strip_prefix("--append-system-prompt=")).expect("the prompt");
    assert!(prompt.starts_with(super::super::prompt::skill().lines().next().expect("a line")), "{prompt}");
    let input: Value = serde_json::from_str(setup.read("stdin").trim()).expect("one line of stream-json");
    assert_eq!(input["message"]["content"][0]["text"], "Which calendars do I have?");

    // The MCP configuration: the suite's server only, private, without the key.
    let config: Value = serde_json::from_str(&setup.read("mcp.json")).expect("json");
    let servers = config["mcpServers"].as_object().expect("servers");
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["torroworkplace"]);
    assert_eq!(servers["torroworkplace"]["command"], setup.path("torroworkplace").display().to_string());
    assert_eq!(servers["torroworkplace"]["args"], json!(["mcp"]));
    assert_eq!(setup.read("mode").trim(), "-rw-------");

    // The key: in the agent's environment, nowhere else, and taken back after the run.
    let key = setup.token();
    assert!(key.expose().starts_with("torro_"), "a key was lent");
    for (what, text) in [
        ("arguments", setup.read("args")),
        ("configuration", setup.read("mcp.json")),
        ("input", setup.read("stdin")),
        ("entries", serde_json::to_string(&entries).expect("json")),
    ] {
        assert!(!text.contains(key.expose()), "the key is in the {what}");
    }
    assert!(setup.host.clients.admit(Some(&key), &Request::CalendarCollections).is_err(), "taken back");
    let config = after("--mcp-config").expect("the configuration");
    assert!(!Path::new(&config).exists(), "the run's folder is removed");
    assert_eq!(*setup.host.lent.lock().expect("lock"), [vec![Area::Tasks, Area::Mail]], "mail never goes to an agent");

    // The next message carries the conversation so far: the agent keeps no session.
    let _ = ask(&setup, Some(&id), "And tomorrow?", Vec::new()).await;
    finished(&setup.assistant, &id).await;
    let input: Value = serde_json::from_str(setup.read("stdin").trim()).expect("stream-json");
    let text = input["message"]["content"][0]["text"].as_str().expect("text");
    assert!(text.contains("Which calendars do I have?") && text.contains("You have no calendars."), "{text}");
    assert!(text.ends_with("And tomorrow?"), "{text}");
}

/// Issue #225 (PR review of #232): engine B leaves the PDF tool to the agent only with write access
/// to `tools`, as engine A offers it; switched on and selected but reading only, it is denied.
#[tokio::test]
async fn an_agent_gets_the_pdf_tool_only_with_write_access() {
    use torro_core::settings::ToolId;
    use torro_ipc::api::{AreaRights, Level, Selection};
    let denied = |arguments: &[String]| {
        let at = arguments.iter().position(|argument| argument == "--disallowedTools").expect("the flag");
        arguments[at + 1].split(',').any(|tool| tool.ends_with("tools_create_pdf"))
    };
    for (level, refused) in [(Level::Write, false), (Level::Read, true)] {
        let setup = set_up(&[init(), says("Done."), done("Done.")], "", Duration::from_secs(10), true).await;
        setup.host.tools_on.lock().expect("lock").insert(ToolId::Pdf);
        let client = setup.host.client().expect("the assistant's client");
        let mut rights = client.rights.clone();
        rights.areas.insert(Area::Tools, AreaRights { level, admin: false });
        rights.tools = Selection::Only(vec!["pdf".into()]);
        setup.host.clients.update(&client.id, rights).await.expect("rights");
        let id = ask(&setup, None, "Make a PDF", Vec::new()).await;
        finished(&setup.assistant, &id).await;
        assert_eq!(denied(&setup.arguments()), refused, "{level:?}");
    }
}

/// Issue #305 ("Werkzeug Web"): a lent key never gets the web. Switched on, selected and readable,
/// engine B still denies the agent its tools and the prompt does not name it; the PDF tool next to it
/// stays.
#[tokio::test]
async fn an_agent_never_gets_the_web() {
    use torro_core::settings::ToolId;
    use torro_ipc::api::{AreaRights, Level, Selection};
    let setup = set_up(&[init(), says("Done."), done("Done.")], "", Duration::from_secs(10), true).await;
    setup.host.tools_on.lock().expect("lock").extend([ToolId::Pdf, ToolId::Web]);
    let client = setup.host.client().expect("the assistant's client");
    let mut rights = client.rights.clone();
    rights.areas.insert(Area::Tools, AreaRights { level: Level::Write, admin: false });
    rights.tools = Selection::Only(vec!["pdf".into(), "web".into()]);
    setup.host.clients.update(&client.id, rights).await.expect("rights");
    let id = ask(&setup, None, "Search the web", Vec::new()).await;
    finished(&setup.assistant, &id).await;
    let arguments = setup.arguments();
    let at = arguments.iter().position(|argument| argument == "--disallowedTools").expect("the flag");
    let denied: Vec<&str> = arguments[at + 1].split(',').collect();
    for tool in ["tools_web_search", "tools_fetch_page"] {
        assert!(denied.iter().any(|denied| denied.ends_with(tool)), "{tool}: {denied:?}");
    }
    assert!(!denied.iter().any(|denied| denied.ends_with("tools_create_pdf")), "{denied:?}");
    let all = setup.read("args");
    let prompt = all.split_once("--append-system-prompt=").map(|(_, rest)| rest).expect("the prompt");
    // The line of the tools it may use now; the skill's general text above it stays the same.
    let own = prompt.lines().find(|line| line.starts_with("- Your own tools")).expect("the tools' line");
    assert!(own.contains("tools_create_pdf") && !own.contains("tools_web_search"), "{own}");
}

/// Task C.17: once the person lets mail go to cloud models, the run's key is lent
/// with mail, the agent's tools include it, and the prompt no longer says it is kept from it.
#[tokio::test]
async fn with_mail_let_out_the_agent_gets_mail_as_the_rights_give_it() {
    let setup = set_up(&[init(), says("Two folders."), done("Two folders.")], "", Duration::from_secs(10), true).await;
    setup.host.mail_to_cloud.store(true, std::sync::atomic::Ordering::SeqCst);
    let id = ask(&setup, None, "Which mail folders do I have?", Vec::new()).await;
    finished(&setup.assistant, &id).await;
    assert_eq!(*setup.host.lent.lock().expect("lock"), [Vec::<Area>::new()], "lent with mail");
    let arguments = setup.read("args");
    let prompt = arguments.split_once("--append-system-prompt=").map(|(_, rest)| rest).expect("the prompt");
    assert!(prompt.contains("  - mail: read"), "{prompt}");
    assert!(!prompt.contains("not given to you"), "{prompt}");
    assert!(prompt.contains("lets mail go to cloud models"), "{prompt}");
}

#[tokio::test]
async fn while_it_runs_the_key_is_the_assistants_without_mail_and_cancel_kills_the_agent() {
    let setup = set_up(&[init(), piece("Thinking")], "exec sleep 30", Duration::from_secs(20), true).await;
    let id = ask(&setup, None, "Plan my week", Vec::new()).await;

    let mut growing = None;
    for _ in 0..1500 {
        if let Some(entry) = read(&setup.assistant, &id, Some(1)).await.1.into_iter().next() {
            growing = Some(entry);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let growing = growing.expect("the growing answer shows");
    assert!(matches!(&growing.content, EntryContent::Answer { text, .. } if text == "Thinking"), "{growing:?}");

    let key = setup.token();
    let Ok(Caller::Client { rights, .. }) = setup.host.clients.admit(Some(&key), &Request::CalendarCollections) else {
        panic!("the lent key is the assistant's");
    };
    assert!(!rights.areas.contains_key(&Area::Mail), "mail stays with the person's computers");
    assert!(setup.host.clients.admit(Some(&key), &Request::MailFolders { account: None }).is_err());

    let pid = setup.read("pid").trim().to_string();
    assert_eq!(setup.assistant.cancel(&id, chrono::Utc::now()).await, Response::Ok);
    let mut killed = false;
    for _ in 0..200 {
        if gone(&pid) {
            killed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(killed, "the agent was killed");
    assert!(setup.host.clients.admit(Some(&key), &Request::CalendarCollections).is_err(), "the key is back");
    let runs: Vec<_> = std::fs::read_dir(setup.path("runs")).expect("runs").collect();
    assert!(runs.is_empty(), "the run's folder is removed: {runs:?}");
    let (summary, entries) = read(&setup.assistant, &id, None).await;
    assert!(!summary.running);
    assert_eq!(entries.last().map(|entry| entry.content.clone()), Some(EntryContent::Stopped));
}

#[tokio::test]
async fn an_agent_that_falls_silent_is_stopped() {
    let setup = set_up(&[init()], "exec sleep 30", Duration::from_secs(3), true).await;
    let id = ask(&setup, None, "Hi", Vec::new()).await;
    let entries = finished(&setup.assistant, &id).await;
    let message = trf("{agent} took too long and was stopped.", &[("agent", &"Claude Code")]);
    assert_eq!(
        entries.last().map(|entry| entry.content.clone()),
        Some(EntryContent::Error { code: "error".into(), message })
    );
    // A fresh script can take seconds to start on a busy machine (macOS checks new programs);
    // one that never got to write its pid was stopped before it ran.
    let Ok(pid) = std::fs::read_to_string(setup.path("pid")) else { return };
    let pid = pid.trim().to_string();
    for _ in 0..200 {
        if gone(&pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the silent agent was not killed");
}

#[tokio::test]
async fn errors_become_error_entries_without_the_key() {
    // Claude Code says it failed.
    let failed = json!({"type": "result", "subtype": "success", "is_error": true,
                        "result": "Invalid API key · Please run /login"});
    let setup = set_up(&[init(), failed], "", Duration::from_secs(10), true).await;
    let id = ask(&setup, None, "Hi", Vec::new()).await;
    let entries = finished(&setup.assistant, &id).await;
    let message = trf(
        "{agent} reported an error: {why}",
        &[("agent", &"Claude Code"), ("why", &"Invalid API key · Please run /login")],
    );
    assert_eq!(
        entries.last().map(|entry| entry.content.clone()),
        Some(EntryContent::Error { code: "error".into(), message })
    );

    // It ends without a result and names the key on stderr.
    let crash = r#"printf 'cannot sign in with %s\n' "$TORRO_TOKEN" >&2; exit 3"#;
    let setup = set_up(&[init()], crash, Duration::from_secs(10), true).await;
    let id = ask(&setup, None, "Hi", Vec::new()).await;
    let entries = finished(&setup.assistant, &id).await;
    let key = setup.token();
    let message = trf(
        "{agent} stopped without an answer: {why}",
        &[("agent", &"Claude Code"), ("why", &"cannot sign in with [key]")],
    );
    assert_eq!(
        entries.last().map(|entry| entry.content.clone()),
        Some(EntryContent::Error { code: "error".into(), message })
    );
    assert!(!serde_json::to_string(&entries).expect("json").contains(key.expose()));

    // An agent that has tools of its own is stopped before it answers.
    let own = json!({"type": "system", "subtype": "init", "model": "m", "tools": ["Bash", "mcp__torroworkplace__x"]});
    let setup = set_up(&[own, says("I ran a shell")], "", Duration::from_secs(10), true).await;
    let id = ask(&setup, None, "Hi", Vec::new()).await;
    let entries = finished(&setup.assistant, &id).await;
    let message = trf(
        "{agent} was stopped before it answered: it offered tools besides Torro Workplace ({tools}).",
        &[("agent", &"Claude Code"), ("tools", &"Bash")],
    );
    assert_eq!(contents(&entries)[1..], [EntryContent::Error { code: "error".into(), message }]);

    // A pending approval is told as such.
    let line = tool_line(
        "mcp__torroworkplace__workspaces_delete_template",
        json!({"id": "x"}),
        "pending: waiting",
        true,
        None,
        "Claude Code",
    );
    assert_eq!(
        line,
        EntryContent::Tool {
            name: "workspaces.delete_template".into(),
            input: json!({"id": "x"}),
            summary: tr("Waiting for your approval").into(),
            error: Some("pending".into()),
            lack: None,
        }
    );
}

#[test]
fn details_lose_the_key_and_stay_short() {
    let key = Key::new("torro_ab12cd34_secret");
    let (run, home) = (Path::new("/home/anna/.local/state/torroworkplace/assistant-runs/ab"), Path::new("/home/anna"));
    let said = "failed:\n  torro_ab12cd34_secret  is bad in /home/anna/.local/state/torroworkplace/assistant-runs/ab \
                and /home/anna/.claude";
    assert_eq!(detail(said, &key, &[run, home]), "failed: [key] is bad in […] and […]/.claude");
    let long = "x".repeat(DETAIL_CHARS * 2);
    assert_eq!(detail(&long, &key, &[]).chars().count(), DETAIL_CHARS + 1);
    assert_eq!(error_code("not-found: no calendar"), "not-found");
    assert_eq!(error_code("Claude requested permissions to use X"), "error");
}

/// Task C.10: the agent's prompt carries the personality first and the notes of the memory, but no
/// note kept from remote models – the agent runs elsewhere.
#[tokio::test]
async fn the_agent_reads_the_personality_and_no_local_note() {
    let transcript = [init(), says("Hello."), done("Hello.")];
    let setup = set_up(&transcript, "", Duration::from_secs(10), true).await;
    let change = super::super::profile::Change {
        personality: Some("Answer like a ship's captain.".into()),
        ..super::super::profile::Change::default()
    };
    assert!(matches!(setup.assistant.update_profile(change).await, Response::Profile { .. }));
    assert!(matches!(
        setup.assistant.remember("Anna works in sales.", None, false, true).await,
        Response::Memory { .. }
    ));
    assert!(matches!(setup.assistant.remember("ACME owes 400 €.", None, true, true).await, Response::Memory { .. }));

    let id = ask(&setup, None, "Hi", Vec::new()).await;
    finished(&setup.assistant, &id).await;
    let arguments = setup.read("args");
    let prompt = arguments.split_once("--append-system-prompt=").map(|(_, rest)| rest).expect("the prompt");
    assert!(prompt.starts_with(super::super::prompt::PERSONALITY_HEADING), "{prompt}");
    assert!(prompt.contains("> Answer like a ship's captain."), "{prompt}");
    assert!(prompt.contains("] Anna works in sales."), "{prompt}");
    assert!(!arguments.contains("ACME"), "{arguments}");
}

/// Issue #131: the model and the effort of the message reach Claude Code's command line, and the
/// conversation keeps them for the next message.
#[tokio::test]
async fn the_model_and_the_effort_go_to_claude_code() {
    let transcript = [init(), says("Hello."), done("Hello.")];
    let setup = set_up(&transcript, "", Duration::from_secs(10), true).await;
    let message = Message {
        conversation: None,
        engine: "claude-code".into(),
        model: Some("opus".into()),
        effort: Some(torro_core::ai::Effort::High),
        text: "Hi".into(),
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
        bot: None,
        on_behalf: None,
        from: Some(crate::assistant::TEST_CLIENT.into()),
    };
    let Response::Conversation { conversation, .. } =
        setup.assistant.send(message, EngineKind::Agent, chrono::Utc::now()).await
    else {
        panic!("expected a conversation");
    };
    finished(&setup.assistant, &conversation.id).await;
    let arguments = setup.arguments();
    assert!(arguments.contains(&"--model=opus".to_string()), "{arguments:?}");
    assert!(arguments.contains(&"--effort=high".to_string()), "{arguments:?}");
    let (summary, _) = read(&setup.assistant, &conversation.id, None).await;
    assert_eq!((summary.model.as_deref(), summary.effort), (Some("opus"), Some(torro_core::ai::Effort::High)));
}

/// Decision 5 in `docs/kontakte.md`: contacts are not kept from agents; what one sees is decided by
/// the address books ticked for it alone, so a person who does not want a book at an AI provider
/// leaves it out there.
#[test]
fn contacts_are_not_kept_from_agents() {
    assert!(!NEVER_TO_AGENTS.contains(&Area::Contacts));
    assert!(!never_given("claude-code").contains(&Area::Contacts));
    assert!(!withheld_from_agents(DataRule::default()).contains(&Area::Contacts));
}
