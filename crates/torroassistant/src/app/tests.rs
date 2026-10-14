//! Tests of the assistant window: what clicks and answers do, and that every demo state builds in
//! every size. The window's tasks do not run in tests; [`settle`] plays the core's part: it answers
//! every request the window sent with the demo and hands the answer back as the task would.

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
    App::new(backend, Source::from_args(args(&["--demo"])), launch).0
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
    for _ in 0..50 {
        let requests = std::mem::take(&mut app.sent);
        if requests.is_empty() {
            return;
        }
        for request in requests {
            let result = block_on(app.backend.ask(request.clone()));
            let _ = app.update(answer_of(&request, result));
        }
    }
    panic!("the window keeps asking");
}

/// The requests sent since the last look, taken.
fn sent(app: &mut App) -> Vec<Request> {
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
            text: "What is on tomorrow?".into(),
            without: vec![Area::Mail],
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
    let _ = none.update(Message::OpenSettings);
    assert_eq!(none.sections, [ACCOUNTS_PAGE], "\"Add an AI account\" opens the accounts");
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

#[test]
fn the_test_engine_is_offered_by_its_name() {
    let mut app = started(&["--open", "picker"]);
    let engine = Engine { id: TEST_ENGINE.into(), kind: EngineKind::Test, name: "Test".into(), problem: None };
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
    for words in [["--open", "bot-editor"], ["--open", "bot-new"], ["--open", "bot-delete"], ["--open", "bot-menu"]] {
        let app = started(&words);
        match words[1] {
            "bot-editor" => assert!(app.bot_draft.as_ref().is_some_and(|draft| draft.id.is_some())),
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
