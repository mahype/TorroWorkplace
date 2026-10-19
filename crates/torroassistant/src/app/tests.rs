//! Tests of the assistant window: what clicks and answers do, and that every demo state builds in
//! every size. The window's tasks do not run in tests; [`settle`] plays the core's part: it answers
//! every request the window sent with the demo and hands the answer back as the task would, with
//! the map the window gave it ([`App::replies`]). [`answer_of`] does the same for a request a test
//! took and answers itself.

use std::sync::Arc;

use iced::futures::executor::block_on;
use torro_ipc::ApprovalState;
use torro_ipc::assistant::TEST_ENGINE;

use super::*;
use crate::demo::{
    APPROVAL, APPROVAL_ID, ASKED, BOT_CHAT, FORBIDDEN, GONE, OLLAMA, PROPOSAL, RELEASE, STREAMING, TOOLS, TRAVEL,
};

const WIDE: Size = Size::new(1600.0, 1000.0);
const SIZES: [Size; 6] = [
    Size::new(240.0, 160.0),
    Size::new(320.0, 480.0),
    Size::new(1200.0, 200.0),
    Size::new(480.0, 900.0),
    Size::new(800.0, 600.0),
    WIDE,
];

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// A demo window started with `words` (`--demo` added), before any answer came.
fn start(words: &[&str]) -> App {
    let mut all = vec!["--demo"];
    all.extend_from_slice(words);
    let launch = Launch::parse(&args(&all)).expect("valid parameters");
    let setup = match launch.open {
        Some(Open::Setup) => demo::Setup { client: false, ..demo::Setup::default() },
        Some(Open::NoEngines) => demo::Setup { engines: false, ..demo::Setup::default() },
        Some(Open::Empty) => demo::Setup { conversations: false, ..demo::Setup::default() },
        _ => demo::Setup::default(),
    };
    let backend = Backend::Demo(Arc::new(demo::Demo::new(chrono::Utc::now(), setup)));
    let mut app = App::new(backend, Source::from_args(args(&["--demo"])), launch).0;
    // The first load follows once the core answers (issue #206); the demo's at once.
    let _ = app.update(Message::CoreReady);
    app
}

/// Issue #206: a window asks the core nothing until it answers, then lists the conversations.
#[test]
fn the_first_load_waits_for_the_core() {
    let launch = Launch::parse(&args(&["--demo"])).expect("valid parameters");
    let backend = Backend::Demo(Arc::new(demo::Demo::new(chrono::Utc::now(), demo::Setup::default())));
    let mut app = App::new(backend, Source::from_args(args(&["--demo"])), launch).0;
    assert!(app.sent.is_empty(), "{:?}", app.sent);
    let _ = app.update(Message::CoreReady);
    assert!(app.sent.iter().any(|request| matches!(request, Request::ListConversations)), "{:?}", app.sent);
}

/// A demo window with every answer in.
fn started(words: &[&str]) -> App {
    let mut app = start(words);
    settle(&mut app);
    app
}

/// The message the task of `request` would send with `result`.
fn answer_of(request: &Request, result: Result<Response, Refusal>) -> Message {
    match request {
        Request::ListEngines => Message::Engines(result.and_then(|response| match response {
            Response::Engines { engines } => Ok(engines),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::ListClients => Message::Client(result.and_then(|response| match response {
            Response::Clients { clients } => {
                Ok(clients.into_iter().find(|c| c.kind == torro_ipc::api::ASSISTANT_CLIENT).map(|c| c.rights))
            }
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::ListConversations => Message::Listed(result.and_then(|response| match response {
            Response::Conversations { conversations } => Ok(conversations),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::AiModels { account, .. } => Message::Models(
            account.clone(),
            result.and_then(|response| match response {
                Response::AiModels { models } => Ok(models),
                other => Err(Refusal::unexpected(&other)),
            }),
        ),
        Request::GetConversation { id, after: None } => Message::Opened(id.clone(), conversation_of(result)),
        Request::GetConversation { id, after: Some(_) } => Message::Followed(id.clone(), conversation_of(result)),
        Request::ApprovalStatus { id } => Message::Approval(id.clone(), approval_of(result)),
        Request::DecideApproval { id, .. } => Message::Decided(id.clone(), approval_of(result)),
        Request::SendMessage { .. } => Message::Sent(conversation_of(result)),
        Request::CancelAnswer { .. } => Message::Stopped(result.map(|_| ())),
        Request::RenameConversation { .. } | Request::PinConversation { .. } => {
            Message::Changed(result.map(|_| ()), "failed")
        }
        Request::DeleteConversation { id } => Message::Deleted(id.clone(), result.map(|_| ())),
        Request::OpenChatWindow { .. } => Message::WindowOpened(result.map(|_| ())),
        Request::GetProfile => Message::Profile(result.and_then(|response| match response {
            Response::Profile { profile } => Ok(profile),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::Settings => Message::Settings(result.and_then(|response| match response {
            Response::Settings { settings } => Ok(settings),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::ListBots => Message::Bots(result.and_then(|response| match response {
            Response::Bots { bots } => Ok(bots),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::GetBot { .. } => Message::BotLoaded(result.and_then(bots::bot_of)),
        Request::SaveBot { .. } => Message::BotSaved(result.and_then(bots::bot_of)),
        Request::DeleteBot { id } => Message::BotDeleted(id.clone(), result.map(|_| ())),
        Request::ConfirmBot { conversation, bot } => Message::BotConfirmed(
            crate::bots::Proposal { conversation: conversation.clone(), bot: bot.clone() },
            result.map(|_| ()),
        ),
        other => panic!("the window sent an unexpected request: {other:?}"),
    }
}

/// Answers the requests the window sent until it sends none: the core's part.
fn settle(app: &mut App) {
    settled(app, |_| None);
}

/// The requests sent since the last look, taken; their answers are left to the test.
fn sent(app: &mut App) -> Vec<Request> {
    app.replies.clear();
    std::mem::take(&mut app.sent)
}

/// Asks for the open conversation once and hands the answer in (one tick of [`FOLLOW`]).
fn follow_once(app: &mut App) {
    let _ = app.update(Message::Follow);
    settle(app);
}

#[test]
fn every_demo_state_builds_in_every_size() {
    let windows: [&[&str]; 4] = [&[], &["--chat", TOOLS], &["--bot", TRAVEL], &["--bot", "b00000ff"]];
    for window in windows {
        for (word, _) in Open::ALL {
            let mut words = window.to_vec();
            words.extend(["--open", word]);
            let mut app = started(&words);
            for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                let _ = app.update(Message::UiState(UiState { sidebar }));
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.view();
                    let _ = app.title();
                }
            }
        }
    }
}

#[test]
fn every_kind_of_entry_builds_unfolded_too() {
    for id in [TOOLS, APPROVAL, STREAMING, FORBIDDEN, "d000000000000005"] {
        let mut app = started(&["--chat", id]);
        let seqs: Vec<u64> = app.transcript.entries.iter().map(|entry| entry.seq).collect();
        for seq in seqs {
            let _ = app.update(Message::ToggleTool(seq));
        }
        for size in SIZES {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }
}

#[test]
fn a_main_window_starts_on_a_new_chat_with_the_last_engine_used() {
    let app = started(&[]);
    assert_eq!(app.current, None);
    assert!(app.listed && app.engines_loaded);
    assert_eq!(app.engines.first().map(|engine| engine.id.as_str()), Some(OLLAMA), "local first");
    // The newest demo conversation ("Draft a reply to Anna") answered with Ollama's qwen3:14b.
    assert_eq!(app.choice, Some(Choice { engine: OLLAMA.into(), model: Some("qwen3:14b".into()) }));
    assert_eq!(app.title(), trf("Assistant · {title}", &[("title", &tr("New chat"))]));
    assert_eq!(app.switchable(), [Area::Mail, Area::Calendar, Area::Tasks, Area::Inbox]);
}

#[test]
fn the_picker_offers_only_models_that_can_call_tools() {
    let app = started(&["--open", "picker"]);
    assert_eq!(app.menu, Some(Menu::Picker));
    let offered: Vec<String> = match app.offered_models(OLLAMA) {
        Some(Ok(models)) => models.into_iter().map(|model| model.id).collect(),
        other => panic!("the models are in: {other:?}"),
    };
    assert_eq!(offered, ["qwen3:14b", "qwen3:8b", "llama3.3:70b"], "gemma3:4b cannot call tools");
    let mut app = app;
    let pick = Choice { engine: OLLAMA.into(), model: Some("qwen3:8b".into()) };
    let _ = app.update(Message::Pick(pick.clone()));
    assert_eq!((app.choice.as_ref(), app.chosen, app.menu.as_ref()), (Some(&pick), Chosen::Person, None));
    // A later listing keeps the person's choice.
    let _ = app.update(Message::Tick);
    settle(&mut app);
    assert_eq!(app.choice, Some(pick));
}

/// Issue #131: Claude Code's own models, an effort only where the model takes one, and "Show all
/// models" lists the AI accounts anew with closed models.
#[test]
fn the_effort_goes_with_the_message_where_the_model_thinks() {
    let mut app = started(&["--open", "effort"]);
    also_offered(&mut app, claude_code());
    assert_eq!(app.menu, Some(Menu::Effort));
    assert!(app.effort_offered(), "qwen3:14b thinks");
    let _ = app.update(Message::PickEffort(Some(torro_core::ai::Effort::High)));
    assert_eq!((app.effort, app.menu.as_ref()), (Some(torro_core::ai::Effort::High), None));

    let claude: Vec<String> = match app.offered_models(CLAUDE_CODE) {
        Some(Ok(models)) => models.into_iter().map(|model| model.id).collect(),
        other => panic!("Claude Code's models are fixed: {other:?}"),
    };
    assert_eq!(claude, ["opus", "sonnet", "haiku"]);
    let _ = app.update(Message::Pick(Choice { engine: CLAUDE_CODE.into(), model: Some("haiku".into()) }));
    assert!(!app.effort_offered(), "Haiku takes no effort");
    assert_eq!(app.offered_effort(), None, "nothing is sent for it");
    let _ = app.update(Message::Pick(Choice { engine: CLAUDE_CODE.into(), model: Some("opus".into()) }));
    sent(&mut app);
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Plan".into())))));
    let _ = app.update(Message::Send);
    assert!(matches!(
        sent(&mut app).as_slice(),
        [Request::SendMessage { effort: Some(torro_core::ai::Effort::High), model: Some(model), .. }] if model == "opus"
    ));

    let _ = app.update(Message::ToggleAllModels);
    let asked: Vec<bool> = sent(&mut app)
        .into_iter()
        .filter_map(|request| match request {
            Request::AiModels { all, .. } => Some(all),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [true, true], "each AI account again, closed models too");
}

/// Issue #132: the mode picked under the composer goes with the message, a new chat starts with the
/// stored rights, and a conversation opened again keeps the mode of its last message.
#[test]
fn the_permission_mode_goes_with_the_message_and_stays_with_the_conversation() {
    let mut app = started(&["--open", "mode"]);
    assert_eq!((app.menu.as_ref(), app.mode), (Some(&Menu::Mode), PermissionMode::Default));
    let _ = app.update(Message::PickMode(PermissionMode::ReadOnly));
    assert_eq!((app.menu.as_ref(), app.mode), (None, PermissionMode::ReadOnly));
    sent(&mut app);
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Look".into())))));
    let _ = app.update(Message::Send);
    assert!(matches!(sent(&mut app).as_slice(), [Request::SendMessage { mode: PermissionMode::ReadOnly, .. }]));
    let _ = app.update(Message::Sent(Err(Refusal::new("error", "not now"))));

    let _ = app.update(Message::NewChat);
    assert_eq!(app.mode, PermissionMode::Default, "a new chat starts with the stored rights");
    settle(&mut app);
    let _ = app.update(Message::PickMode(PermissionMode::Ask));
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Plan".into())))));
    let _ = app.update(Message::Send);
    settle(&mut app);
    let id = app.current.clone().expect("a conversation");
    assert_eq!(app.mode, PermissionMode::Ask, "kept after sending");
    let _ = app.update(Message::NewChat);
    settle(&mut app);
    let _ = app.update(Message::Select(id));
    settle(&mut app);
    assert_eq!(app.mode, PermissionMode::Ask, "the conversation's last message");
}

/// Issue #131: a default set in the control centre is what a new chat starts with.
#[test]
fn a_new_chat_starts_with_the_default_of_the_control_centre() {
    let mut app = started(&[]);
    also_offered(&mut app, claude_code());
    let default = AssistantDefault {
        engine: CLAUDE_CODE.into(),
        model: Some("sonnet".into()),
        effort: Some(torro_core::ai::Effort::Low),
    };
    let settings = Settings { assistant_default: Some(default), ..Settings::default() };
    let _ = app.update(Message::Settings(Ok(settings)));
    assert_eq!(app.choice, Some(Choice { engine: CLAUDE_CODE.into(), model: Some("sonnet".into()) }));
    assert_eq!(app.effort, Some(torro_core::ai::Effort::Low));
}

#[test]
fn sending_starts_a_conversation_and_the_growing_answer_replaces_itself() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::ToggleArea(Area::Mail));
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(
        "What is on tomorrow?".into(),
    )))));
    let _ = app.update(Message::Send);
    assert!(app.sending);
    let requests = sent(&mut app);
    assert_eq!(
        requests,
        [Request::SendMessage {
            conversation: None,
            engine: OLLAMA.into(),
            model: Some("qwen3:14b".into()),
            effort: None,
            text: "What is on tomorrow?".into(),
            without: vec![Area::Mail],
            mode: torro_ipc::assistant::Mode::Default,
            bot: None,
        }]
    );
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    let id = app.current.clone().expect("the new conversation is open");
    assert!(!app.sending && app.editor.text().trim().is_empty(), "the text went");
    assert!(app.transcript.running());
    assert!(app.conversations.iter().any(|c| c.id == id), "listed at once");
    settle(&mut app);

    follow_once(&mut app);
    let growing = app.transcript.growing().map(|entry| entry.seq);
    assert_eq!(growing, Some(2), "the answer grows under last + 1");
    let first = app.transcript.last_answer_text().map(str::len);
    follow_once(&mut app);
    assert_eq!(app.transcript.entries.len(), 2, "still one answer");
    assert!(app.transcript.last_answer_text().map(str::len) > first, "longer");
    for _ in 0..40 {
        if !app.transcript.running() {
            break;
        }
        follow_once(&mut app);
    }
    assert!(!app.transcript.running());
    assert_eq!(app.transcript.growing(), None);
    assert_eq!(app.transcript.entries.len(), 2, "message and answer, stored");
    assert_eq!(app.transcript.last_answer_text(), Some(tr(demo::REPLY)));
    assert!(app.answers.contains_key(&2), "the answer is read as Markdown");
}

#[test]
fn nothing_is_sent_while_an_answer_runs_or_without_text() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::Send);
    assert!(sent(&mut app).is_empty(), "no text");
    let mut app = started(&["--open", "streaming"]);
    assert!(app.transcript.running());
    sent(&mut app);
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("And?".into())))));
    let _ = app.update(Message::Send);
    assert!(sent(&mut app).is_empty(), "the answer still runs");
    let _ = app.update(Message::Stop);
    assert_eq!(sent(&mut app), [Request::CancelAnswer { conversation: STREAMING.into() }]);
}

#[test]
fn the_approval_card_allows_only_on_a_click_and_once() {
    let mut app = started(&["--open", "approval"]);
    assert_eq!(app.current.as_deref(), Some(APPROVAL));
    let approval = app.approvals.get(APPROVAL_ID).expect("the card has the core's approval");
    assert!(approval.state.is_pending());
    assert_eq!(approval.summary, tr("Create the event \"Focus time\" on Tue 14 Oct, 14:00–18:00 in Work"));
    // Following the answer asks how the approval stands, and never decides it.
    let _ = app.update(Message::Follow);
    assert!(app.sent.iter().all(|request| !matches!(request, Request::DecideApproval { .. })));
    settle(&mut app);

    let _ = app.update(Message::Decide(APPROVAL_ID.into(), true));
    assert_eq!(sent(&mut app), [chat::decide_request(APPROVAL_ID, true)]);
    assert_eq!(app.deciding.as_deref(), Some(APPROVAL_ID), "the buttons are off until the core answered");
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), false));
    assert!(sent(&mut app).is_empty(), "a second click sends nothing");

    let result = block_on(app.backend.ask(chat::decide_request(APPROVAL_ID, true)));
    let _ = app.update(Message::Decided(APPROVAL_ID.into(), approval_of(result)));
    assert_eq!(app.deciding, None);
    assert!(matches!(app.approvals.get(APPROVAL_ID).map(|a| &a.state), Some(ApprovalState::Done { .. })));
    settle(&mut app);
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), false));
    assert!(sent(&mut app).is_empty(), "decided: nothing to send");
    let tools: Vec<&str> = app
        .transcript
        .entries
        .iter()
        .filter_map(|entry| match &entry.content {
            EntryContent::Tool { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(tools, ["workspaces.load_template", "calendar.save_event"], "the tool ran and shows as a line");
}

#[test]
fn deny_sends_the_same_request_with_no() {
    let mut app = started(&["--open", "approval"]);
    sent(&mut app);
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), false));
    assert_eq!(sent(&mut app), [Request::DecideApproval { id: APPROVAL_ID.into(), allow: false }]);
}

#[test]
fn a_chat_window_shows_its_conversation_or_a_new_one_when_it_is_gone() {
    let app = started(&["--chat", TOOLS]);
    assert!(app.chat_only);
    assert_eq!(app.title(), tr("Plan for tomorrow"), "the window's title is the conversation's");
    assert_eq!(app.transcript.summary.as_ref().map(|s| s.id.as_str()), Some(TOOLS));

    let gone = started(&["--chat", "d0000000000000ff"]);
    assert_eq!(gone.current, None, "a deleted conversation becomes a new chat");
    assert!(gone.notice.is_some());
    assert_eq!(gone.title(), tr("New chat"));

    let new = started(&["--chat", "new"]);
    assert_eq!((new.chat_only, new.current.as_deref()), (true, None));
}

#[test]
fn the_menu_renames_pins_opens_and_deletes() {
    let mut app = started(&["--open", "tools"]);
    sent(&mut app);
    let _ = app.update(Message::OpenInWindow(TOOLS.into()));
    assert_eq!(sent(&mut app), [Request::OpenChatWindow { window: ChatWindow::Conversation(TOOLS.into()) }]);

    let _ = app.update(Message::AskRename(TOOLS.into()));
    assert_eq!(app.dialog, Some(Dialog::Rename { id: TOOLS.into(), title: tr("Plan for tomorrow").into() }));
    let _ = app.update(Message::RenameInput("  Tomorrow  ".into()));
    let _ = app.update(Message::ConfirmRename);
    assert_eq!(sent(&mut app), [Request::RenameConversation { id: TOOLS.into(), title: "Tomorrow".into() }]);
    assert_eq!(app.dialog, None);
    assert_eq!(app.conversation_title(), "Tomorrow");

    let _ = app.update(Message::Pin(TOOLS.into(), false));
    assert_eq!(sent(&mut app), [Request::PinConversation { id: TOOLS.into(), pinned: false }]);

    let _ = app.update(Message::AskDelete(TOOLS.into()));
    assert!(sent(&mut app).is_empty(), "deleting asks first");
    let _ = app.update(Message::ConfirmDelete);
    let requests = sent(&mut app);
    assert_eq!(requests, [Request::DeleteConversation { id: TOOLS.into() }]);
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    assert_eq!(app.current, None, "the open conversation went: a new chat");
    assert!(!app.conversations.iter().any(|c| c.id == TOOLS));
}

#[test]
fn opening_a_conversation_takes_its_engine_and_areas() {
    let mut app = started(&[]);
    let _ = app.update(Message::Select(FORBIDDEN.into()));
    assert_eq!(app.transcript, Transcript::default(), "nothing of the new chat stays");
    settle(&mut app);
    assert_eq!(app.choice, Some(Choice { engine: OLLAMA.into(), model: Some("qwen3:14b".into()) }));
    assert_eq!(app.chosen, Chosen::Conversation);
    assert!(app.without.is_empty());
    let _ = app.update(Message::NewChat);
    assert_eq!((app.current.as_ref(), app.transcript.summary.as_ref()), (None, None));
}

#[test]
fn the_first_start_asks_to_set_up_the_assistant() {
    let mut app = started(&["--open", "setup"]);
    assert_eq!(app.access, Access::Missing);
    assert!(app.switchable().is_empty(), "without a client there is nothing to switch off");
    sent(&mut app);
    let _ = app.update(Message::SetUp);
    // "Set up access" opens the control centre's page "Assistant" itself, no request.
    assert_eq!(app.sections, [ASSISTANT_PAGE]);
    assert!(sent(&mut app).is_empty());

    let mut none = started(&["--open", "no-engines"]);
    assert!(none.engines_loaded && none.engines.is_empty());
    assert_eq!(none.choice, None, "nothing can answer");
    let _ = none.update(Message::AddAiAccount);
    assert_eq!(none.sections, [ACCOUNTS_PAGE], "\"Add an AI account\" opens the accounts");
    assert!(none.settings.is_none(), "the control centre, not the settings in the window");
}

#[test]
fn change_access_under_a_refused_tool_opens_the_assistant_page() {
    let mut app = started(&["--open", "forbidden"]);
    let _ = app.view();
    let _ = app.update(Message::SetUp);
    assert_eq!(app.sections, ["assistant"]);
    assert_eq!(ASSISTANT_PAGE, "assistant", "the section `torroworkplace ui` takes");
    assert_eq!(ACCOUNTS_PAGE, "accounts");
}

#[test]
fn escape_closes_the_dialog_then_menus() {
    let mut app = started(&["--open", "rename"]);
    let _ = app.update(Message::ToggleMenu(Menu::Picker));
    let _ = app.update(Message::Escape);
    assert_eq!(app.dialog, None);
    assert_eq!(app.menu, Some(Menu::Picker));
    let _ = app.update(Message::Escape);
    assert_eq!(app.menu, None);
}

#[test]
fn an_older_answer_for_another_conversation_is_dropped() {
    let mut app = started(&["--open", "tools"]);
    let before = app.transcript.clone();
    let stray = block_on(app.backend.ask(Request::GetConversation { id: STREAMING.into(), after: None }));
    let _ = app.update(Message::Followed(STREAMING.into(), conversation_of(stray)));
    assert_eq!(app.transcript, before);
}

#[test]
fn enter_sends_and_shift_enter_breaks_the_line() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let press = |modifiers| text_editor::KeyPress {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        modifiers,
        text: None,
        status: text_editor::Status::Focused { is_hovered: false },
    };
    assert!(matches!(composer_binding(press(Modifiers::empty())), Some(Binding::Custom(Message::Send))));
    assert!(matches!(composer_binding(press(Modifiers::SHIFT)), Some(Binding::Enter)));
}

/// A coding agent as an engine, with its fixed models (issue #131). The demo offers none since
/// task C.19, issue #219 — the product does not — but `EngineKind::Agent` stays in the protocol,
/// so the window must still be able to show one: the tests bring their own.
fn claude_code() -> Engine {
    let model = |id: &str, name: &str, reasoning: bool| torro_core::ai::AiModel {
        id: id.into(),
        name: name.into(),
        tools: Some(true),
        open_weights: Some(false),
        context: None,
        reasoning: Some(reasoning),
        remote: None,
    };
    Engine {
        id: CLAUDE_CODE.into(),
        kind: EngineKind::Agent,
        name: "Claude Code".into(),
        problem: None,
        models: vec![model("opus", "Opus", true), model("sonnet", "Sonnet", true), model("haiku", "Haiku", false)],
    }
}

/// The engine id of a coding agent, as `assistant.send_message` takes it.
const CLAUDE_CODE: &str = "claude-code";

/// `app` with `engine` added to what the core listed, as a later listing would bring it.
fn also_offered(app: &mut App, engine: Engine) {
    let mut engines = app.engines.clone();
    engines.push(engine);
    let _ = app.update(Message::Engines(Ok(engines)));
}

#[test]
fn the_test_engine_is_offered_by_its_name() {
    let mut app = started(&["--open", "picker"]);
    let engine = Engine {
        id: TEST_ENGINE.into(),
        kind: EngineKind::Test,
        name: "Test".into(),
        problem: None,
        models: Vec::new(),
    };
    let mut engines = app.engines.clone();
    engines.push(engine);
    let _ = app.update(Message::Engines(Ok(engines)));
    assert_eq!(app.engines.last().map(|engine| engine.id.as_str()), Some(TEST_ENGINE), "last");
    let _ = app.view();
}

/// Task C.10: the window reads the assistant's profile with everything else and shows its name and
/// picture; before the core answered, or with a core that has no profiles, the default name.
#[test]
fn the_window_shows_the_assistants_name_and_picture() {
    let app = start(&[]);
    assert_eq!((app.assistant_name(), app.assistant_avatar()), ("Torro", "letter"), "before the answer");
    let mut app = started(&[]);
    assert_eq!(app.profile.as_ref().map(|profile| profile.avatar.as_str()), Some("sparkles"));
    let renamed = Profile { name: "Jarvis".into(), ..demo::profile() };
    let _ = app.update(Message::Profile(Ok(renamed)));
    assert_eq!(app.assistant_name(), "Jarvis");
    let _ = app.update(Message::Profile(Err(Refusal::new("unsupported", "an old core"))));
    assert_eq!(app.assistant_name(), "Jarvis", "an error keeps what was shown");
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    assert!(crate::view::avatar_icon("dragon").is_none(), "an unknown picture is the letter");
    for id in torro_ipc::assistant::AVATARS {
        assert_eq!(crate::view::avatar_icon(id).is_none(), *id == torro_ipc::assistant::LETTER_AVATAR, "{id}");
    }
}

// -- Bots (task B.3) ------------------------------------------------------------------------------

/// The text the editor's personality, instructions or a skill holds, typed in.
fn typed(text: &str) -> text_editor::Action {
    text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text.into())))
}

#[test]
fn a_bot_window_opens_the_bots_newest_conversation_with_its_name_in_the_title() {
    let app = started(&["--bot", TRAVEL]);
    assert!(app.chat_only);
    assert_eq!(app.current.as_deref(), Some(BOT_CHAT), "its newest conversation");
    assert_eq!(app.chat_bot(), Some(TRAVEL));
    assert_eq!(
        app.title(),
        trf("{bot} · {title}", &[("bot", &tr("Travel planner")), ("title", &tr("Train to Lisbon"))])
    );

    // A bot without conversations: a new chat with it, at its engine, and the message names it.
    let mut new = started(&["--bot", RELEASE]);
    assert_eq!(new.current, None);
    assert_eq!(new.title(), trf("{bot} · {title}", &[("bot", &tr("Release helper")), ("title", &tr("New chat"))]));
    assert_eq!(new.choice, Some(Choice { engine: demo::OPENROUTER.into(), model: Some("qwen/qwen3-32b".into()) }));
    sent(&mut new);
    let _ = new.update(Message::Edit(typed("What is still open?")));
    let _ = new.update(Message::Send);
    let requests = sent(&mut new);
    assert!(
        matches!(&requests[..], [Request::SendMessage { bot: Some(bot), without, conversation: None, .. }] if bot == RELEASE && without.is_empty()),
        "{requests:?}"
    );
    let result = block_on(new.backend.ask(requests[0].clone()));
    let _ = new.update(answer_of(&requests[0], result));
    let _ = new.update(Message::NewChat);
    assert_eq!(new.chat_bot(), Some(RELEASE), "a new chat in the bot's window stays with the bot");
}

#[test]
fn a_bot_window_of_a_deleted_bot_sends_nothing() {
    let mut app = started(&["--bot", "b00000ff"]);
    assert!(app.bot_gone());
    sent(&mut app);
    let _ = app.update(Message::Edit(typed("Hello?")));
    let _ = app.update(Message::Send);
    assert!(sent(&mut app).is_empty());

    let mut gone = started(&["--chat", GONE]);
    assert!(gone.transcript.summary.as_ref().is_some_and(|summary| summary.bot_deleted));
    assert!(gone.bot_gone(), "its conversation can be read, not continued");
    sent(&mut gone);
    let _ = gone.update(Message::Send);
    assert!(sent(&mut gone).is_empty());
}

#[test]
fn the_sidebar_opens_a_bots_chat_and_a_new_chat_goes_back_to_the_assistant() {
    let mut app = started(&[]);
    assert!(app.bots_loaded);
    assert_eq!(app.bots.iter().map(|bot| bot.id.as_str()).collect::<Vec<_>>(), [RELEASE, TRAVEL], "by name");
    let _ = app.update(Message::OpenBot(TRAVEL.into()));
    settle(&mut app);
    assert_eq!((app.current.as_deref(), app.chat_bot()), (Some(BOT_CHAT), Some(TRAVEL)));
    let _ = app.update(Message::NewBotChat(TRAVEL.into()));
    assert_eq!((app.current.as_deref(), app.chat_bot()), (None, Some(TRAVEL)));
    assert_eq!(app.choice, Some(Choice { engine: OLLAMA.into(), model: Some("qwen3:14b".into()) }), "the bot's engine");
    let _ = app.update(Message::NewChat);
    assert_eq!(app.chat_bot(), None, "a new chat in the main window is the assistant's");
    let _ = app.update(Message::Select(BOT_CHAT.into()));
    assert_eq!(app.chat_bot(), Some(TRAVEL), "a bot's conversation answers with its bot");
    let _ = app.update(Message::Select(TOOLS.into()));
    assert_eq!(app.chat_bot(), None);
}

#[test]
fn editing_a_bot_saves_it_whole_and_keeps_the_limits() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::EditBot(TRAVEL.into()));
    assert_eq!(sent(&mut app), [Request::GetBot { id: TRAVEL.into() }], "the whole bot first");
    let _ = app.update(Message::EditBot(TRAVEL.into()));
    settle(&mut app);
    let draft = app.bot_draft.as_ref().expect("the editor is open");
    assert_eq!((draft.id.as_deref(), draft.skills.len()), (Some(TRAVEL), 1));

    let _ = app.update(Message::Draft(DraftEdit::Name("Trip planner".into())));
    let _ = app.update(Message::Draft(DraftEdit::AddSkill));
    let _ = app.update(Message::SaveBot);
    assert!(sent(&mut app).is_empty(), "a skill without a name is not sent");
    assert_eq!(
        app.bot_draft.as_ref().and_then(|draft| draft.problem().filter(|_| draft.tried)),
        Some(tr("Give every skill a name.").to_owned()),
        "and says so after the first Save"
    );
    let _ = app.update(Message::Draft(DraftEdit::SkillName(1, "Night trains".into())));
    let _ = app.update(Message::Draft(DraftEdit::SkillText(1, typed("Lisbon has none."))));
    let _ = app.update(Message::Draft(DraftEdit::Avatar("sun".into())));
    let _ = app.update(Message::SaveBot);
    let requests = sent(&mut app);
    let [Request::SaveBot { id, name, avatar, skills, engine, .. }] = &requests[..] else {
        panic!("one bots.save_bot: {requests:?}");
    };
    assert_eq!((id.as_deref(), name.as_str(), avatar.as_deref()), (Some(TRAVEL), "Trip planner", Some("sun")));
    assert_eq!((skills.len(), engine.as_str()), (2, OLLAMA));
    let _ = app.update(Message::SaveBot);
    assert!(sent(&mut app).is_empty(), "no second click while saving");
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    assert!(app.bot_draft.is_none(), "saved: the editor closes");
    assert_eq!(app.bot_name(TRAVEL), "Trip planner");
}

#[test]
fn a_new_bot_needs_a_name_and_opens_its_chat_once_saved() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::NewBot);
    settle(&mut app);
    let _ = app.update(Message::SaveBot);
    assert!(sent(&mut app).is_empty());
    assert_eq!(app.bot_draft.as_ref().and_then(BotDraft::problem), Some(tr("Give the bot a name.").to_owned()));
    let _ = app.update(Message::Draft(DraftEdit::Name("Gardener".into())));
    let _ = app.update(Message::Draft(DraftEdit::Instructions(typed("Water on Mondays."))));
    let _ = app.update(Message::SaveBot);
    let requests = sent(&mut app);
    assert!(matches!(&requests[..], [Request::SaveBot { id: None, name, .. }] if name == "Gardener"), "{requests:?}");
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    let new = app.bots.iter().find(|bot| bot.name == "Gardener").map(|bot| bot.id.clone()).expect("listed");
    assert_eq!((app.chat_bot(), app.current.as_ref()), (Some(new.as_str()), None), "a new chat with the new bot");
}

/// [`settle`], with `refuse` answering a request in the core's place where it gives a refusal;
/// returns every request it answered.
fn settled(app: &mut App, refuse: impl Fn(&Request) -> Option<Refusal>) -> Vec<Request> {
    let mut all = Vec::new();
    for _ in 0..50 {
        let requests = app.sent.clone();
        if requests.is_empty() {
            return all;
        }
        all.extend(requests);
        let replies = std::mem::take(&mut app.replies);
        let requests = std::mem::take(&mut app.sent);
        for (request, reply) in requests.into_iter().zip(replies) {
            let result = match refuse(&request) {
                Some(refusal) => Err(refusal),
                None => block_on(app.backend.ask(request)),
            };
            let _ = app.update(reply(result));
        }
    }
    panic!("the window keeps asking");
}

/// A demo window with the editor of a new bot named `name` open.
fn new_bot_named(name: &str) -> App {
    let mut app = started(&[]);
    sent(&mut app);
    app.new_bot();
    let _ = app.update(Message::Draft(DraftEdit::Name(name.into())));
    app
}

/// Task S.18: "Save" in the bot editor is `bots.save_bot` and nothing else – no `clients.*`, so
/// the editor never writes what a bot may use. That stays in the control centre, where the row
/// "Access" of the editor points ([`Message::BotAccess`]).
#[test]
fn the_bot_editor_saves_the_bot_and_never_its_access() {
    for words in [vec![], vec!["--open", "bot-editor"]] {
        let mut app = if words.is_empty() { new_bot_named("Gardener") } else { started(&words) };
        sent(&mut app);
        let _ = app.update(Message::SaveBot);
        let requests = settled(&mut app, |_| None);
        assert!(matches!(requests.first(), Some(Request::SaveBot { .. })), "{requests:?}");
        for request in &requests {
            assert!(
                !matches!(
                    request,
                    Request::UpdateClient { .. } | Request::CreateClient { .. } | Request::RemoveClient { .. }
                ),
                "the editor touches a client: {request:?}"
            );
        }
        assert!(app.bot_draft.is_none(), "saved: the editor closes");
    }
}

/// The editor opens with the bot alone: no rights, no calendars, no sources are read for it any
/// more (task S.18).
#[test]
fn the_editor_asks_only_for_the_bot() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::EditBot(TRAVEL.into()));
    let requests = sent(&mut app);
    assert_eq!(requests, [Request::GetBot { id: TRAVEL.into() }]);
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    assert!(app.bot_draft.as_ref().is_some_and(|draft| draft.id.is_some()), "the editor is open");
    assert!(sent(&mut app).is_empty(), "nothing follows");
}

#[test]
fn deleting_a_bot_asks_first_and_the_main_window_goes_back_to_the_assistant() {
    let mut app = started(&["--open", "bot"]);
    assert_eq!(app.chat_bot(), Some(TRAVEL));
    sent(&mut app);
    let _ = app.update(Message::AskDeleteBot(TRAVEL.into()));
    assert!(sent(&mut app).is_empty(), "asks first");
    assert!(matches!(&app.dialog, Some(Dialog::DeleteBot { id, .. }) if id == TRAVEL));
    let _ = app.update(Message::ConfirmDeleteBot);
    let requests = sent(&mut app);
    assert_eq!(requests, [Request::DeleteBot { id: TRAVEL.into() }]);
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    settle(&mut app);
    assert!(app.listed_bot(TRAVEL).is_none());
    assert_eq!((app.current.as_ref(), app.chat_bot()), (None, None), "a new chat with the assistant");
    let lisbon = app.conversations.iter().find(|c| c.id == BOT_CHAT).expect("its conversation stays");
    assert!(lisbon.bot_deleted);
}

#[test]
fn access_opens_the_bots_rights_and_a_window_is_asked_of_the_core() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::BotAccess(TRAVEL.into()));
    assert_eq!(app.sections, ["bot:b0000001"], "torroworkplace ui --section bot:<id>");
    assert!(sent(&mut app).is_empty(), "no request: the control centre opens");
    let _ = app.update(Message::OpenBotWindow(TRAVEL.into()));
    assert_eq!(sent(&mut app), [Request::OpenChatWindow { window: ChatWindow::Bot(TRAVEL.into()) }]);
}

#[test]
fn a_tool_line_that_asked_a_bot_opens_its_conversation() {
    let app = started(&["--open", "bot-asked"]);
    assert_eq!(app.current.as_deref(), Some(ASKED));
    let tool = app.transcript.entries.iter().find(|e| matches!(e.content, EntryContent::Tool { .. })).expect("a tool");
    assert!(app.expanded.contains(&tool.seq), "unfolded");
    let EntryContent::Tool { name, input, .. } = &tool.content else { unreachable!() };
    let asked = crate::bots::asked_bot(name, input).expect("asked a bot");
    assert!(matches!(app.open_asked(asked.clone()), Message::Select(id) if id == BOT_CHAT));
    let window = started(&["--chat", ASKED]);
    assert!(matches!(window.open_asked(asked), Message::OpenInWindow(id) if id == BOT_CHAT), "its own window");
}

#[test]
fn the_card_of_a_proposal_confirms_with_the_app_request_and_says_yes() {
    let mut app = started(&["--open", "bot-proposal"]);
    assert_eq!(app.current.as_deref(), Some(PROPOSAL));
    assert!(app.proposal_card().is_some(), "the assistant waits for the person's word");
    sent(&mut app);
    let proposal = crate::bots::Proposal { conversation: PROPOSAL.into(), bot: TRAVEL.into() };
    let _ = app.update(Message::ConfirmBot(proposal.clone()));
    let requests = sent(&mut app);
    assert_eq!(requests, [Request::ConfirmBot { conversation: PROPOSAL.into(), bot: TRAVEL.into() }]);
    let _ = app.update(Message::ConfirmBot(proposal.clone()));
    assert!(sent(&mut app).is_empty(), "one click at a time");
    let _ = app.update(Message::BotConfirmed(proposal, Ok(())));
    let requests = sent(&mut app);
    let yes = crate::bots::confirm_text(tr("Travel planner"));
    assert!(
        matches!(&requests[..], [Request::SendMessage { conversation: Some(id), text, bot: None, .. }] if id == PROPOSAL && *text == yes),
        "the person's yes follows in the chat: {requests:?}"
    );
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    assert!(app.proposal_card().is_none(), "answered: the card goes");

    // Not in a bot's chat, and not for an ask that ran.
    let asked = started(&["--open", "bot-asked"]);
    assert!(asked.proposal_card().is_none());
}

#[test]
fn every_bot_state_builds_in_every_size() {
    let states = [["--open", "bot-editor"], ["--open", "bot-new"], ["--open", "bot-delete"], ["--open", "bot-menu"]];
    for words in states {
        let app = started(&words);
        match words[1] {
            "bot-editor" => assert!(app.bot_draft.as_ref().is_some_and(|draft| draft.id.is_some()), "{words:?}"),
            "bot-new" => assert!(app.bot_draft.as_ref().is_some_and(|draft| draft.tried && draft.problem().is_some())),
            "bot-delete" => assert!(matches!(app.dialog, Some(Dialog::DeleteBot { .. }))),
            _ => assert_eq!(app.menu, Some(Menu::Bot { id: TRAVEL.into() })),
        }
    }
    let mut app = started(&["--open", "bot-editor"]);
    for _ in 0..torro_ipc::bots::MAX_SKILLS {
        let _ = app.update(Message::Draft(DraftEdit::AddSkill));
    }
    let _ = app.update(Message::ToggleMenu(Menu::BotPicker));
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

// -- The settings in the content area (task S.18) ----------------------------------------------

/// Hands the open settings the data of their first load: the host's tasks do not run in tests,
/// and the demo of `torro-settings-ui` answers from memory.
fn settings_loaded(app: &mut App) {
    let data = block_on(torro_settings_ui::load(app.backend.settings(), settings::wanted()));
    let _ = app.update(Message::SettingsHost(torro_settings_ui::host::Message::Loaded(Box::new(data))));
}

/// The sections of the open settings; panics without them.
fn pane(app: &App) -> &SettingsPane {
    app.settings.as_ref().expect("the settings are open")
}

/// The gear turns the content area into the settings and back. The chat behind them is untouched,
/// and the settings never go through the window's own requests ([`App::ask`]) – they have their
/// own way to the core ([`Backend::settings`]).
#[test]
fn the_gear_opens_the_settings_in_the_window_and_closes_them_again() {
    let mut app = started(&["--open", "tools"]);
    sent(&mut app);
    let _ = app.update(Message::ToggleSettings);
    assert_eq!(pane(&app).host.current().id, "general", "the gear lands on General");
    assert!(sent(&mut app).is_empty(), "no request of the window");
    settings_loaded(&mut app);
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    let _ = app.update(Message::Resized(WIDE));
    let _ = app.update(Message::ToggleSettings);
    assert!(app.settings.is_none(), "the gear leads back out");
    assert_eq!(app.current.as_deref(), Some(TOOLS), "the chat is where it was");
}

/// The arrow in front of the path and Escape close the settings, like the gear.
#[test]
fn the_arrow_and_escape_close_the_settings() {
    for back in [Message::SettingsHost(torro_settings_ui::host::Message::Back), Message::Escape] {
        let mut app = started(&[]);
        let _ = app.update(Message::ToggleSettings);
        settings_loaded(&mut app);
        let _ = app.update(back);
        assert!(app.settings.is_none());
    }
}

/// Escape goes to the open section first – a note being written stops, the settings stay – and
/// only then out of the settings (task S.17's `escape`).
#[test]
fn escape_stops_the_open_question_before_it_closes_the_settings() {
    use torro_settings_ui::assistant::profile;

    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    let _ = app.update(Message::SettingsPage(settings::Message::Profile(profile::Message::OpenMemory)));
    let _ = app.update(Message::SettingsPage(settings::Message::Profile(profile::Message::AddNote)));
    assert!(settings::escape(&pane(&app).sections).is_some(), "a note is being written");
    let _ = app.update(Message::Escape);
    assert!(app.settings.is_some(), "the note went, the settings stay");
    assert!(settings::escape(&pane(&app).sections).is_none());
    let _ = app.update(Message::Escape);
    assert!(app.settings.is_none());
}

/// The clock of the Voice section runs through the window's own subscription: switched on, the
/// section asks how the listener stands again until it reports what it does – else the row stays
/// at "Starting …" for good (task S.17).
#[test]
fn the_window_carries_the_clock_of_the_voice_section() {
    use torro_settings_ui::assistant::voice;

    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    let at_rest = app.subscription().units();
    let _ = app.update(Message::SettingsPage(settings::Message::Voice(voice::Message::Enabled(true))));
    assert_eq!(app.subscription().units(), at_rest + 1, "the section's clock is in the window's subscription");
    // Closed, the clock goes with the settings.
    let _ = app.update(Message::ToggleSettings);
    assert!(app.subscription().units() < at_rest + 1);
}

/// A change behind the gear reaches the window at once: when the host reports it stored
/// ([`torro_settings_ui::Outcome::Changed`]), the window asks for what it shows itself – the
/// conversations, the engines, the client, the profile – instead of waiting for its next tick. A
/// change that failed loads nothing.
#[test]
fn a_change_in_the_settings_loads_the_window_again() {
    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    let fresh = Box::new(Ok(torro_settings_ui::Data::default()));
    let task =
        app.update(Message::SettingsHost(torro_settings_ui::host::Message::Sent { changed: true, result: fresh }));
    assert!(task.units() > 0, "the window's own data is on its way again");
    assert!(app.settings.is_some(), "and the settings stay open");
    let failed = Box::new(Err("not connected".to_string()));
    let task =
        app.update(Message::SettingsHost(torro_settings_ui::host::Message::Sent { changed: true, result: failed }));
    assert_eq!(task.units(), 0, "nothing changed, so there is nothing to load");
}

/// A click in the sidebar – a chat, a bot, "New chat", "New bot" – brings the content area back.
#[test]
fn the_sidebar_closes_the_settings() {
    for message in [
        Message::Select(TOOLS.into()),
        Message::OpenBot(TRAVEL.into()),
        Message::NewChat,
        Message::NewBot,
        Message::EditBot(TRAVEL.into()),
    ] {
        let mut app = started(&[]);
        let _ = app.update(Message::ToggleSettings);
        settings_loaded(&mut app);
        let _ = app.update(message.clone());
        assert!(app.settings.is_none(), "{message:?}");
    }
}

/// An unsaved profile is never dropped silently (found in the review of #210): the gear, Escape
/// and a click in the sidebar first ask "Discard changes?" – the question of the section itself
/// (`profile::closing`) – and only "Discard" lets the settings go. Before this, all three threw
/// the typed text away without a word.
#[test]
fn an_unsaved_profile_asks_before_the_settings_close() {
    use torro_settings_ui::assistant::profile;

    let profile_page = |message: profile::Message| Message::SettingsPage(settings::Message::Profile(message));
    for way in [Message::ToggleSettings, Message::Escape, Message::Select(TOOLS.into())] {
        let mut app = started(&[]);
        let _ = app.update(Message::ToggleSettings);
        settings_loaded(&mut app);
        let _ = app.update(profile_page(profile::Message::OpenProfile));
        let _ = app.update(profile_page(profile::Message::Name("Marvin".into())));

        let _ = app.update(way.clone());
        assert!(app.settings.is_some(), "{way:?}: the question stands instead");
        let sections = &pane(&app).sections;
        assert!(settings::overlay(sections, pane(&app).host.data()).is_some(), "{way:?}: \"Discard changes?\"");

        // Escape cancels the question: the settings and what is typed stay where they are.
        let _ = app.update(Message::Escape);
        assert!(app.settings.is_some(), "{way:?}: cancelled, nothing lost");
        assert!(settings::closing(&pane(&app).sections, pane(&app).host.data()).is_some(), "{way:?}: still typed");

        let _ = app.update(way.clone());
        let _ = app.update(profile_page(profile::Message::Leave));
        assert!(app.settings.is_none(), "{way:?}: \"Discard\" lets them go");
    }

    // With nothing typed every way out closes them at once, as it always did.
    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    let _ = app.update(profile_page(profile::Message::OpenProfile));
    let _ = app.update(Message::ToggleSettings);
    assert!(app.settings.is_none(), "nothing to ask about");
}

/// `--settings <section>` opens the window with that section of the settings.
#[test]
fn settings_voice_starts_at_the_voice_section() {
    for section in settings::SECTIONS {
        let app = started(&["--settings", section.id]);
        assert_eq!(pane(&app).host.current().id, section.id);
    }
    let app = started(&["--settings", "voice"]);
    assert_eq!(pane(&app).host.current().id, "voice");
    assert!(!pane(&app).host.loaded(), "its data is on its way");
}

/// What the sections ask of the app: "Edit" at a bot is the bot editor, "Access ↗" the control
/// centre at this very bot. Neither is a request of this window.
#[test]
fn the_sections_reach_the_editor_and_the_control_centre() {
    use torro_settings_ui::assistant::bots as section;

    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    sent(&mut app);
    let _ = app.update(Message::SettingsPage(settings::Message::Bots(section::Message::Access(TRAVEL.into()))));
    assert_eq!(app.sections, ["bot:b0000001"], "torroworkplace ui --section bot:<id>");
    assert!(app.settings.is_some(), "the control centre is another window");
    let _ = app.update(Message::SettingsPage(settings::Message::Bots(section::Message::Edit(TRAVEL.into()))));
    assert!(app.settings.is_none(), "the editor takes the content area");
    assert_eq!(sent(&mut app), [Request::GetBot { id: TRAVEL.into() }]);
}

/// Task S.18, not negotiable: only a person opens the settings. A link of an answer opens a web
/// page or nothing at all – no scheme of its own, no path, no start parameter.
#[test]
fn a_link_in_an_answer_opens_only_the_web() {
    for url in [
        "torro://settings/voice",
        "torroworkplace://ui?section=assistant",
        "/etc/passwd",
        "file:///etc/passwd",
        "--settings voice",
        "torroassistant --settings voice",
        "javascript:alert(1)",
        "mailto:anna@example.org",
        "HTTP://example.org",
        "",
    ] {
        assert_eq!(web_link(url), None, "{url:?}");
    }
    for url in ["http://localhost:8080/docs", "https://example.org/a?b=c"] {
        assert_eq!(web_link(url), Some(url));
    }
    let mut app = started(&["--open", "tools"]);
    sent(&mut app);
    let _ = app.update(Message::OpenLink("torro://settings/voice".into()));
    assert!(app.settings.is_none(), "no answer opens the settings");
    assert!(app.sections.is_empty() && sent(&mut app).is_empty(), "and nothing else happens");
}

/// Plays `count` frames of the window at 60 a second after `now`; the time of the last one.
fn play(app: &mut App, mut now: Instant, count: u32) -> Instant {
    for _ in 0..count {
        now += FRAME;
        let _ = app.update(Message::Frame(now));
    }
    now
}

/// What shows of the answer `seq`.
fn shown(app: &App, seq: u64) -> String {
    app.answers.get(&seq).map(|(text, _)| text.clone()).unwrap_or_default()
}

fn id_of(app: &App) -> String {
    app.current.clone().expect("a conversation is open")
}

/// A demo window with the answer to "Hello" just begun (numbered 2), followed once.
fn answering() -> App {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Hello".into())))));
    let _ = app.update(Message::Send);
    settle(&mut app);
    follow_once(&mut app);
    app
}

/// The open conversation's head with `last` and `running` changed, as the core would answer.
fn head(app: &App, last: u64, running: bool) -> ConversationSummary {
    let mut summary = app.transcript.summary.clone().expect("a conversation is open");
    summary.last = last;
    summary.running = running;
    summary
}

fn answer_entry(seq: u64, text: &str) -> Entry {
    Entry {
        seq,
        time: chrono::Utc::now(),
        content: EntryContent::Answer { text: text.into(), engine: OLLAMA.into(), model: None },
    }
}

#[test]
fn an_answer_that_comes_while_watching_appears_letter_by_letter() {
    let mut app = answering();
    let stream = app.stream.as_ref().expect("the new answer appears letter by letter");
    assert_eq!(stream.seq, 2);
    assert!(stream.smoother.revealing(), "the window asks for frames");
    assert!(!app.answer_shows(2), "nothing shows before the first frame, and takes no room");

    let mut now = play(&mut app, Instant::now(), 4);
    let first = shown(&app, 2);
    assert!(!first.is_empty() && tr(demo::REPLY).starts_with(first.as_str()), "the first letters: {first:?}");
    let growing = app.transcript.last_answer_text().map(str::len).unwrap_or_default();
    assert!(first.len() < growing, "fewer than came: they come one after the other");

    for _ in 0..60 {
        if !app.transcript.running() {
            break;
        }
        follow_once(&mut app);
        now = play(&mut app, now, 3);
    }
    assert!(!app.transcript.running());
    for _ in 0..120 {
        if app.stream.is_none() {
            break;
        }
        now = play(&mut app, now, 1);
    }
    assert!(app.stream.is_none(), "the stream ends once the answer shows whole");
    assert_eq!(shown(&app, 2), tr(demo::REPLY), "exactly the whole answer, as if it never streamed");
}

#[test]
fn an_answer_that_was_there_on_opening_shows_whole() {
    let app = started(&["--open", "streaming"]);
    assert_eq!(app.stream, None, "nothing to catch up");
    let growing = app.transcript.growing().expect("the model still writes");
    let EntryContent::Answer { text, .. } = &growing.content else { panic!("an answer grows") };
    assert_eq!(&shown(&app, growing.seq), text);
}

#[test]
fn the_entries_after_an_appearing_answer_wait_for_it() {
    let mut app = answering();
    let now = play(&mut app, Instant::now(), 2);
    // The answer is stored and a tool runs: the tool line waits for the answer's last letter.
    let tool = Entry {
        seq: 3,
        time: chrono::Utc::now(),
        content: EntryContent::Tool {
            name: "calendar.list_events".into(),
            input: serde_json::json!({}),
            summary: "2 events".into(),
            error: None,
            lack: None,
        },
    };
    let entries = vec![answer_entry(2, "Let me look."), tool];
    let _ = app.update(Message::Followed(id_of(&app), Ok((head(&app, 3, true), entries))));
    assert_eq!(app.appearing(), Some(2), "the tool line waits");
    let _ = app.view();
    play(&mut app, now, 60);
    assert_eq!(app.appearing(), None, "shown whole, the tool line follows");
    assert_eq!(shown(&app, 2), "Let me look.");

    // A new answer while one still appears: the first shows whole at once, the new one appears.
    let mut app = answering();
    let entries = vec![answer_entry(2, "Let me look."), answer_entry(3, "Found it")];
    let _ = app.update(Message::Followed(id_of(&app), Ok((head(&app, 2, true), entries))));
    assert_eq!(shown(&app, 2), "Let me look.");
    assert_eq!(app.appearing(), Some(3));
}

#[test]
fn an_answer_stored_under_another_number_goes_on_where_it_was() {
    let mut app = answering();
    play(&mut app, Instant::now(), 10);
    let before = shown(&app, 2);
    assert!(!before.is_empty());
    let text = app.transcript.last_answer_text().unwrap_or_default().to_owned();
    // The core stored something else under 2; the text grows on under 3.
    let other = Entry { seq: 2, time: chrono::Utc::now(), content: EntryContent::Stopped };
    let entries = vec![other, answer_entry(3, &format!("{text} more"))];
    let _ = app.update(Message::Followed(id_of(&app), Ok((head(&app, 2, true), entries))));
    assert_eq!(app.appearing(), Some(3));
    assert_eq!(shown(&app, 3), before, "no letter is written twice");
}

#[test]
fn an_answer_waiting_for_approval_is_followed_slowly() {
    assert!(started(&["--open", "approval"]).waits_for_approval());
    assert!(!started(&["--open", "streaming"]).waits_for_approval());
}
