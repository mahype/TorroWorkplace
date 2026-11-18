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
    APPROVAL, APPROVAL_ID, ASKED, BOT_CHAT, CARDS, FORBIDDEN, GONE, MAIL_CHAT, OLLAMA, OPENROUTER, PDF, PROPOSAL,
    RELEASE, STREAMING, TOOLS, TRAVEL,
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
                Response::AiModels { models, .. } => Ok(models),
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
        Request::ChooseModel { id, engine, model, effort } => Message::ChoseModel(
            id.clone(),
            Choice { engine: engine.clone(), model: model.clone() },
            *effort,
            result.map(|_| ()),
        ),
        Request::DeleteConversation { id } => Message::Deleted(id.clone(), result.map(|_| ())),
        Request::ArchiveConversations { .. } => Message::Changed(result.map(|_| ()), "failed"),
        Request::DeleteConversations { ids } => Message::DeletedMany(ids.clone(), result.map(|_| ())),
        Request::OpenChatWindow { .. } => Message::WindowOpened(result.map(|_| ())),
        Request::GetProfile => Message::Profile(result.and_then(|response| match response {
            Response::Profile { profile } => Ok(profile),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::Settings => Message::Settings(result.and_then(|response| match response {
            Response::Settings { settings } => Ok(Box::new(settings)),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::ListBots => Message::Bots(result.and_then(|response| match response {
            Response::Bots { bots } => Ok(bots),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::GetBot { .. } => Message::BotLoaded(result.and_then(bots::bot_of)),
        Request::SaveBot { .. } => Message::BotSaved(result.and_then(bots::bot_of)),
        Request::DeleteBot { id } => Message::BotDeleted(id.clone(), result.map(|_| ())),
        Request::GetCards { conversation } => Message::Cards(
            conversation.clone(),
            result.and_then(|response| match response {
                Response::Cards { cards } => Ok(cards),
                other => Err(Refusal::unexpected(&other)),
            }),
        ),
        Request::OpenFile { .. } | Request::SaveFileAs { .. } | Request::OpenApp { .. } => {
            Message::FileDone(result.map(|_| None), "failed")
        }
        Request::VoiceStatus => Message::Voice(result.and_then(|response| match response {
            Response::Voice { voice } => Ok(voice),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::Dictate => Message::Dictated(result.and_then(|response| match response {
            Response::Transcript { text } => Ok(text),
            other => Err(Refusal::unexpected(&other)),
        })),
        Request::StopDictation => Message::DictationStopped,
        // Issue #745: the answer a reading belongs to is the test's to hand in (`Message::Read`).
        Request::SetSetting { key: torro_ipc::SettingKey::ReadAloud, value } => {
            Message::ReadAloudSet(value.as_bool().unwrap_or_default(), result.map(|_| ()))
        }
        Request::Speak { .. } | Request::StopSpeaking => Message::ReadingStopped,
        // Issue #784: the picture of a link card, and the buttons of content cards.
        Request::GetCardPicture { conversation, seq, .. } => Message::CardPicture(
            conversation.clone(),
            *seq,
            result.and_then(|response| match response {
                Response::ChatPicture { picture } => Ok(picture),
                other => Err(Refusal::unexpected(&other)),
            }),
        ),
        Request::ShowAssistantCard { .. }
        | Request::SaveTodo { .. }
        | Request::SendDraft { .. }
        | Request::SetSetting { key: torro_ipc::SettingKey::PrivacyProvider, .. } => {
            Message::CardAnswered(result.map(|_| ()), "failed")
        }
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
    for id in [TOOLS, APPROVAL, STREAMING, FORBIDDEN, "d000000000000005", CARDS] {
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

/// Issue #605: "Open in Mail" on a suggestion's card opens the mail window on that mail through the
/// core (`apps.open_app` with `message:<account>:<id>`); a refusal says so and asks for the cards again.
#[test]
fn open_in_mail_on_a_suggestion_opens_its_mail() {
    let mut app = started(&["--chat", MAIL_CHAT]);
    let suggestion = app.cards.values().find_map(|card| match card {
        Card::Suggestion { account, id, .. } => Some((account.clone(), id.clone())),
        _ => None,
    });
    let (account, id) = suggestion.expect("the demo's suggestion");
    sent(&mut app);
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    let _ = app.update(Message::OpenMail { account: account.clone(), id: id.clone() });
    let target = format!("message:{account}:{id}");
    assert_eq!(sent(&mut app), [Request::OpenApp { app: "torro-inbox-mail".into(), target: Some(target) }]);
    let refusal = Refusal { code: "not-found".into(), message: "no such mail".into() };
    let _ = app.update(Message::FileDone(Err(refusal), tr("Couldn't open the mail.")));
    assert!(sent(&mut app).iter().any(|request| matches!(request, Request::GetCards { .. })), "the cards again");
}

/// Task WZ.8: a conversation with cards asks the core what they show (`assistant.get_cards`) and
/// draws a card's content only once it is resolved (a placeholder until then, issue #784); the file card's buttons
/// each send their request – "Save as…" only after the person chose a place in the system's dialog.
#[test]
fn file_cards_come_from_the_core_and_each_button_sends_its_request() {
    // Before the core resolved them, nothing of them is known: they stand as placeholders.
    let mut app = start(&["--chat", CARDS]);
    let requests = sent(&mut app);
    let opened = requests.iter().find(|r| matches!(r, Request::GetConversation { .. })).expect("the conversation");
    let _ = app.update(answer_of(opened, block_on(app.backend.ask(opened.clone()))));
    let asked: Vec<Request> = sent(&mut app).into_iter().filter(|r| matches!(r, Request::GetCards { .. })).collect();
    assert_eq!(asked, [Request::GetCards { conversation: CARDS.into() }]);
    assert!(app.cards.is_empty());
    let _ = app.view();

    let mut app = started(&["--chat", CARDS]);
    let seqs = app.transcript.cards();
    assert_eq!(seqs.len(), 2, "two PDFs were made");
    let resolved: Vec<&Card> = seqs.iter().filter_map(|seq| app.cards.get(seq)).collect();
    assert!(matches!(resolved.as_slice(), [Card::Unavailable, Card::File(file)] if file.id == PDF), "{resolved:?}");
    sent(&mut app);

    let _ = app.update(Message::OpenFile(PDF.into()));
    assert_eq!(sent(&mut app), [Request::OpenFile { file: PDF.into() }]);
    let _ = app.update(Message::MailFile(PDF.into()));
    assert_eq!(sent(&mut app), [Request::OpenApp { app: "torroinbox".into(), target: Some(format!("file:{PDF}")) }]);
    // "Save as…" opens the system's dialog first: nothing goes to the core until a place is chosen.
    let _ = app.update(Message::SaveFileAs { id: PDF.into(), name: "Release checklist-2.pdf".into() });
    assert!(sent(&mut app).is_empty());
    let _ = app.update(Message::SavePicked(PDF.into(), None));
    assert!(sent(&mut app).is_empty(), "cancelled");
    let place = PathBuf::from("/Users/anna/Documents/Release checklist.pdf");
    let _ = app.update(Message::SavePicked(PDF.into(), Some(place)));
    let saved = sent(&mut app);
    assert_eq!(
        saved,
        [Request::SaveFileAs { file: PDF.into(), path: "/Users/anna/Documents/Release checklist.pdf".into() }]
    );
    let _ = app.update(Message::FileDone(Ok(Some(tr("Copy saved"))), "failed"));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Copy saved")));
    // A button that failed says so and asks for the cards again: the file may have gone.
    let _ =
        app.update(Message::FileDone(Err(Refusal::new("not-found", "No such file.")), tr("Couldn't open the file.")));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Couldn't open the file.")));
    assert_eq!(sent(&mut app), [Request::GetCards { conversation: CARDS.into() }]);
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    // Another conversation leaves its cards behind.
    let _ = app.update(Message::Select(TOOLS.into()));
    assert!(app.cards.is_empty());
}

/// S3 of issue #784: a card the core has not resolved yet stands as a placeholder (`State::Loading`), never as
/// nothing; a card whose item went is gone with the head of its kind; "Try again" asks for the cards again.
#[test]
fn unresolved_cards_load_gone_ones_say_so_and_try_again_asks_anew() {
    use crate::view::CardLook;
    let mut app = start(&["--chat", demo::CONTENT]);
    let requests = sent(&mut app);
    let opened = requests.iter().find(|r| matches!(r, Request::GetConversation { .. })).expect("the conversation");
    let _ = app.update(answer_of(opened, block_on(app.backend.ask(opened.clone()))));
    sent(&mut app);
    let seqs = app.transcript.cards();
    assert!(!seqs.is_empty());
    assert!(seqs.iter().all(|seq| app.card_look(*seq) == CardLook::Pending), "nothing resolved yet");
    for seq in &seqs {
        assert!(app.card(*seq).is_some(), "a placeholder in the card's size, not nothing");
    }

    let app = started(&["--chat", demo::CONTENT]);
    let gone = content_seq(&app, |card| matches!(card, Card::Unavailable));
    assert_eq!(app.card_look(gone), CardLook::Gone);
    assert!(app.card(gone).is_some() && app.card_line(gone).is_some());
    let shown = content_seq(&app, |card| matches!(card, Card::Mail { .. }));
    assert_eq!(app.card_look(shown), CardLook::Shown);

    let mut app = app;
    sent(&mut app);
    let _ = app.update(Message::RetryCards);
    assert_eq!(sent(&mut app), [Request::GetCards { conversation: demo::CONTENT.into() }]);
}

/// Design review of WZ.8: a tool line right above a file card leaves the file's name to the card;
/// above a card of a file that is gone it keeps the name, the card no longer shows it.
#[test]
fn a_tool_line_leaves_the_name_to_its_file_card() {
    let app = started(&["--chat", CARDS]);
    let made: Vec<u64> = app
        .transcript
        .entries
        .iter()
        .filter(|entry| matches!(&entry.content, EntryContent::Tool { name, .. } if name == "tools.create_pdf"))
        .map(|entry| entry.seq)
        .collect();
    assert_eq!(made.len(), 2, "two PDFs were made");
    assert!(!app.file_card_after(made[0]), "its file is gone");
    assert!(app.file_card_after(made[1]));
    let asked = app.transcript.entries.iter().find(|entry| matches!(entry.content, EntryContent::Tool { .. }));
    assert!(!app.file_card_after(asked.map_or(0, |entry| entry.seq)), "an answer follows the first tool");
}

/// Task WZ.8: a card that comes while the window follows the answer is resolved at once; the cards
/// of a conversation no longer open are not taken.
#[test]
fn a_new_card_is_resolved_at_once_and_only_for_the_open_conversation() {
    let mut app = started(&["--chat", CARDS]);
    let known = app.cards.clone();
    let _ = app.update(Message::Cards(TOOLS.into(), Ok(Vec::new())));
    assert_eq!(app.cards, known, "not the open conversation");
    let mut summary = app.transcript.summary.clone().expect("loaded");
    let seq = summary.last + 1;
    summary.last = seq;
    let card = Entry {
        seq,
        time: chrono::Utc::now(),
        content: EntryContent::Card { card: torro_ipc::assistant::CardRef::File { id: PDF.into() } },
    };
    sent(&mut app);
    let _ = app.update(Message::Followed(CARDS.into(), Ok((summary, vec![card]))));
    assert!(sent(&mut app).contains(&Request::GetCards { conversation: CARDS.into() }));
}

/// The entry of the first content card of `CONTENT` that `pick` takes.
fn content_seq(app: &App, pick: impl Fn(&Card) -> bool) -> u64 {
    let mut seqs: Vec<u64> = app.cards.iter().filter(|(_, card)| pick(card)).map(|(seq, _)| *seq).collect();
    seqs.sort_unstable();
    *seqs.first().expect("the demo has such a card")
}

/// What a click on the button `label` of the card `seq` sent to the core.
fn press(app: &mut App, seq: u64, label: &str) -> Vec<Request> {
    let actions = app.card_actions(seq);
    let action = actions.into_iter().find(|action| action.label == label).expect("the button");
    sent(app);
    let _ = app.update(action.on_press.expect("a button that does something"));
    sent(app)
}

/// Issue #784 (strand 2g): every button of a content card is the person's click with one request – mail, event,
/// task, contact and "Show all" open the app on the item (`apps.open_app`), "Done" saves the task done, the draft
/// with an approval only decides it, the question about a provider shows the card once or changes Settings ›
/// Privacy. A link's card opens the address in the browser (not pressed here).
#[test]
fn each_button_of_a_content_card_sends_its_request() {
    let mut app = started(&["--chat", demo::CONTENT]);
    let account = "mail:anna@work.example.org";

    let mail = content_seq(&app, |card| matches!(card, Card::Mail { .. }));
    let opened = press(&mut app, mail, tr("Open in Mail"));
    let target = Some(format!("message:{account}:m:c1"));
    assert_eq!(opened, [Request::OpenApp { app: "torro-inbox-mail".into(), target }]);

    let event = content_seq(&app, |card| matches!(card, Card::Event { .. }));
    let Some(Card::Event { event: shown, .. }) = app.cards.get(&event).cloned() else { panic!("an event") };
    let target = Some(format!("event:{}", shown.uid));
    assert_eq!(
        press(&mut app, event, tr("Open in calendar")),
        [Request::OpenApp { app: "torrocalendar".into(), target }]
    );

    let todo = content_seq(&app, |card| matches!(card, Card::Todo { .. }));
    let Some(Card::Todo { todo: task, .. }) = app.cards.get(&todo).cloned() else { panic!("a task") };
    assert!(!task.completed, "the demo's task is open");
    let target = Some(format!("task:{}", task.uid));
    assert_eq!(press(&mut app, todo, tr("Open in tasks")), [Request::OpenApp { app: "torrotasks".into(), target }]);
    let quiet: Vec<bool> = app.card_actions(todo).iter().map(|action| action.quiet).collect();
    assert_eq!(quiet, [false, true], "Done only lets the task go: ghost");
    let done = torro_pim::Todo { completed: true, ..*task };
    assert_eq!(press(&mut app, todo, tr("Done")), [Request::SaveTodo { todo: done }]);

    let draft = content_seq(&app, |card| matches!(card, Card::Draft { approval: Some(_), .. }));
    let allow = Request::DecideApproval { id: APPROVAL_ID.into(), allow: true, scope: None };
    assert_eq!(press(&mut app, draft, tr("Send")), [allow], "with an approval Send only allows it");
    let deny = Request::DecideApproval { id: APPROVAL_ID.into(), allow: false, scope: None };
    assert_eq!(press(&mut app, draft, tr("Discard")), [deny], "and Discard denies it");

    let contact = content_seq(&app, |card| matches!(card, Card::Contact { .. }));
    let target = Some("contact:demo/anna".to_owned());
    assert_eq!(
        press(&mut app, contact, tr("Open in contacts")),
        [Request::OpenApp { app: "torrocontacts".into(), target }]
    );

    let page = content_seq(&app, |card| matches!(card, Card::Link { card: Some(_), .. }));
    let actions = app.card_actions(page);
    assert!(matches!(&actions[0].on_press, Some(Message::OpenLink(url)) if url.starts_with("https://example.org/")));

    let asked = content_seq(&app, |card| matches!(card, Card::Link { ask: Some(_), .. }));
    let once = Request::ShowAssistantCard { conversation: demo::CONTENT.into(), seq: asked };
    assert_eq!(press(&mut app, asked, tr("Show once")), [once]);
    let choice = |choice: &str| Request::SetSetting {
        key: torro_ipc::SettingKey::PrivacyProvider,
        value: serde_json::json!({"provider": "news.example.com", "choice": choice}),
    };
    let always = trf("Always allow {provider}", &[("provider", &"news.example.com")]);
    assert_eq!(press(&mut app, asked, &always), [choice("allowed")]);
    let never = trf("Never for {provider}", &[("provider", &"news.example.com")]);
    assert_eq!(press(&mut app, asked, &never), [choice("blocked")]);

    let counted = content_seq(&app, |card| matches!(card, Card::Group { total: Some(_), .. }));
    assert_eq!(
        press(&mut app, counted, tr("Show all")),
        [Request::OpenApp { app: "torro-inbox-mail".into(), target: None }]
    );
    let listed = content_seq(&app, |card| matches!(card, Card::Group { total: None, .. }));
    assert!(app.card_actions(listed).iter().all(|action| action.label != tr("Show all")), "every item is shown");

    // A click that failed says so and asks for the cards again: the item may have changed.
    let _ =
        app.update(Message::CardAnswered(Err(Refusal::new("conflict", "changed")), tr("Couldn't mark the task done.")));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Couldn't mark the task done.")));
    assert!(sent(&mut app).contains(&Request::GetCards { conversation: demo::CONTENT.into() }));
}

/// Issue #784 (strand 2g): a draft without an open approval sends only with `mail.send_draft` (the person's click
/// with the app key); "Discard" is switched off and sends nothing, and nothing decides an approval.
#[test]
fn a_draft_without_an_approval_sends_only_through_send_draft() {
    let mut app = started(&["--chat", demo::CONTENT]);
    let seq = content_seq(&app, |card| matches!(card, Card::Draft { .. }));
    let Some(Card::Draft { draft, .. }) = app.cards.get(&seq).cloned() else { panic!("a draft") };
    app.cards.insert(seq, Card::Draft { draft: draft.clone(), approval: None });
    let actions = app.card_actions(seq);
    let discard = actions.iter().find(|action| action.label == tr("Discard")).expect("Discard");
    assert!(discard.on_press.is_none(), "Discard does nothing without an approval");
    let sending = press(&mut app, seq, tr("Send"));
    assert_eq!(sending, [Request::SendDraft { id: draft.id.clone().expect("a saved draft") }]);
    assert!(!sending.iter().any(|request| matches!(request, Request::DecideApproval { .. })));
    // A draft never saved has nothing to send.
    app.cards.insert(seq, Card::Draft { draft: Box::new(torro_mail::Draft { id: None, ..*draft }), approval: None });
    assert!(app.card_actions(seq).iter().all(|action| action.on_press.is_none()));
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

/// Issue #784 (strand 2g, "Grenzen" in `docs/inhaltskarten.md`): one to three items are full cards, four to ten
/// short forms, and with more found than shown the line "{n} … match" with "Show all" stands below them.
#[test]
fn a_group_shows_cards_short_forms_and_the_total() {
    let line = |total: u64| Some(trf("{count} mails match", &[("count", &total)]));
    assert_eq!(App::group_look(Area::Mail, 3, None), (true, None), "three are cards");
    assert_eq!(App::group_look(Area::Mail, 4, None), (false, None), "four are short forms");
    assert_eq!(App::group_look(Area::Mail, 10, Some(11)), (false, line(11)), "eleven found: ten lines and the total");
    assert_eq!(App::group_look(Area::Mail, 3, Some(3)), (true, None), "a total that is all shown says nothing");
    let tasks = App::group_look(Area::Tasks, 10, Some(25)).1;
    assert_eq!(tasks, Some(trf("{count} tasks match", &[("count", &25_u64)])));

    // Each of them draws in every size, from the demo's mails.
    let mut app = started(&["--chat", demo::CONTENT]);
    let seq = content_seq(&app, |card| matches!(card, Card::Group { .. }));
    let Some(Card::Group { items, area, .. }) = app.cards.get(&seq).cloned() else { panic!("a group") };
    let mail = items.first().cloned().expect("an item");
    for (count, total) in [(3, None), (4, None), (10, Some(11))] {
        app.cards.insert(seq, Card::Group { items: vec![mail.clone(); count], area, total });
        let opens = app.card_actions(seq).iter().filter(|action| action.label == tr("Open in Mail")).count();
        assert_eq!(opens, count, "every item keeps its Open");
        for size in SIZES {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }
}

/// Issue #785 (strand 2g): the picture of a link card is asked for once per card (`assistant.get_card_picture`),
/// decoded when it comes and drawn; a refusal keeps the icon and asks no more until the card changes.
#[test]
fn a_link_card_asks_for_its_picture_once() {
    use base64::Engine as _;
    let mut app = start(&["--chat", demo::CONTENT]);
    settle(&mut app);
    let page = content_seq(&app, |card| matches!(card, Card::Link { card: Some(_), .. }));
    let key = (demo::CONTENT.to_owned(), page);
    assert!(app.card_pictures_asked.contains(&key), "asked once the cards came");
    assert!(!app.card_pictures.contains_key(&key), "the demo has no picture: the icon stays");
    // The cards again: the card did not change, so its picture is not asked for again.
    let _ = app.update(Message::CardAnswered(Ok(()), "failed"));
    let again = sent(&mut app);
    let cards = again.iter().find(|request| matches!(request, Request::GetCards { .. })).expect("the cards");
    let _ = app.update(answer_of(cards, block_on(app.backend.ask(cards.clone()))));
    assert!(!sent(&mut app).iter().any(|request| matches!(request, Request::GetCardPicture { .. })));

    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(3, 2))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("a PNG");
    let picture = torro_ipc::chat::ChatPicture {
        content_type: "image/png".into(),
        base64: torro_ipc::chat::FileData(base64::engine::general_purpose::STANDARD.encode(png.into_inner())),
        width: Some(3),
        height: Some(2),
    };
    // A picture for a conversation no longer open is not kept, and is asked for anew there.
    let _ = app.update(Message::CardPicture(TOOLS.into(), page, Ok(picture.clone())));
    assert!(!app.card_pictures.contains_key(&(TOOLS.to_owned(), page)));
    let _ = app.update(Message::CardPicture(demo::CONTENT.into(), page, Ok(picture)));
    assert!(app.card_pictures.contains_key(&key), "decoded to draw");
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    // A card that changes asks for its picture anew.
    let mut changed = app.cards.clone();
    if let Some(Card::Link { card: Some(card), .. }) = changed.get_mut(&page) {
        card.title = "Another title".into();
    }
    let entries = changed.into_iter().map(|(seq, card)| CardEntry { seq, card }).collect();
    let _ = app.update(Message::Cards(demo::CONTENT.into(), Ok(entries)));
    assert!(!app.card_pictures.contains_key(&key));
    assert!(sent(&mut app).contains(&Request::GetCardPicture {
        conversation: demo::CONTENT.into(),
        seq: page,
        size: torro_ipc::chat::PreviewSize::Small,
    }));
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
    // Task S.26: only models turned on; gemma3:4b cannot call tools, llama3.3:70b is off.
    assert_eq!(offered, ["qwen3:14b", "qwen3:8b"]);
    let mut app = app;
    let pick = Choice { engine: OLLAMA.into(), model: Some("qwen3:8b".into()) };
    let _ = app.update(Message::Pick(pick.clone()));
    assert_eq!((app.choice.as_ref(), app.chosen, app.menu.as_ref()), (Some(&pick), Chosen::Person, None));
    // A later listing keeps the person's choice.
    let _ = app.update(Message::Tick);
    settle(&mut app);
    assert_eq!(app.choice, Some(pick));
}

/// Issue #852: a model picked in an open conversation is the core's at once, so the voice chat answers
/// with it too and the header names it; a new chat keeps it until its first message, a bot's chat its bot's.
#[test]
fn a_model_picked_in_a_conversation_goes_to_the_core_at_once() {
    let mut app = started(&["--chat", TOOLS]);
    sent(&mut app);
    let pick = Choice { engine: OPENROUTER.into(), model: Some("gpt".into()) };
    let _ = app.update(Message::Pick(pick.clone()));
    let requests = sent(&mut app);
    assert!(
        matches!(&requests[..], [Request::ChooseModel { id, engine, model, .. }]
            if id == TOOLS && engine == OPENROUTER && model.as_deref() == Some("gpt")),
        "{requests:?}"
    );
    // The core's answer: the header names the model chosen.
    let _ = app.update(Message::ChoseModel(TOOLS.into(), pick.clone(), None, Ok(())));
    let summary = app.transcript.summary.as_ref().expect("the open conversation");
    assert_eq!((summary.engine.as_str(), summary.model.as_deref()), (OPENROUTER, Some("gpt")));
    // So does the next look at the conversation, as the demo keeps the choice like the core.
    settle(&mut app);
    let _ = app.update(Message::Pick(Choice { engine: OLLAMA.into(), model: Some("qwen3:8b".into()) }));
    settle(&mut app);
    follow_once(&mut app);
    let summary = app.transcript.summary.as_ref().expect("the open conversation");
    assert_eq!((summary.engine.as_str(), summary.model.as_deref()), (OLLAMA, Some("qwen3:8b")));
    // The answer to an earlier pick that comes after a newer one changes nothing.
    let older = Choice { engine: OPENROUTER.into(), model: Some("older".into()) };
    let _ = app.update(Message::Pick(older.clone()));
    let _ = app.update(Message::Pick(Choice { engine: OLLAMA.into(), model: Some("qwen3:8b".into()) }));
    let _ = app.update(Message::ChoseModel(TOOLS.into(), older, None, Ok(())));
    let summary = app.transcript.summary.as_ref().expect("the open conversation");
    assert_eq!(summary.model.as_deref(), Some("qwen3:8b"), "the header keeps the newest pick");
    sent(&mut app);
    // A refusal says so.
    let _ = app.update(Message::Pick(pick.clone()));
    let _ = app.update(Message::ChoseModel(TOOLS.into(), pick.clone(), None, Err(Refusal::new("not-found", "gone"))));
    assert!(app.notice.is_some());
    let back = Choice { engine: OLLAMA.into(), model: Some("qwen3:8b".into()) };
    assert_eq!(app.choice, Some(back), "the window goes back to who answers there");

    let _ = app.update(Message::NewChat);
    sent(&mut app);
    let _ = app.update(Message::Pick(pick.clone()));
    let _ = app.update(Message::PickEffort(Some(torro_core::ai::Effort::High)));
    assert!(sent(&mut app).is_empty(), "a new chat keeps the choice until its first message");

    // Another conversation answers with its own model, not with a pick made elsewhere.
    let _ = app.update(Message::Select(TOOLS.into()));
    settle(&mut app);
    let _ = app.update(Message::Pick(pick.clone()));
    settle(&mut app);
    let _ = app.update(Message::Select(BOT_CHAT.into()));
    settle(&mut app);
    assert_ne!(app.choice.as_ref(), Some(&pick), "the pick stays with its conversation");
    let _ = app.update(Message::Pick(pick));
    assert!(
        !sent(&mut app).iter().any(|request| matches!(request, Request::ChooseModel { .. })),
        "a bot's model stays"
    );
}

/// Issue #131: Claude Code's own models and an effort only where the model takes one. Task S.26:
/// "Manage models…" opens the control centre's AI section, and a picker that opens asks every AI
/// account again for its models, closed ones too.
#[test]
fn the_effort_goes_with_the_message_where_the_model_thinks() {
    // Issue #699: the effort is a row in the model's menu; `--open effort` opens that menu.
    let mut app = started(&["--open", "effort"]);
    also_offered(&mut app, claude_code());
    assert_eq!(app.menu, Some(Menu::Picker));
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

    let _ = app.update(Message::ManageModels);
    // Issue #410: the section AI of this window's settings, not the control centre.
    assert!(app.sections.is_empty(), "not the control centre");
    assert_eq!(pane(&app).host.current().id, AI_PAGE, "where the models are turned on");
    assert_eq!(AI_PAGE, "ai");
    let _ = app.update(Message::CloseMenu);
    let _ = app.update(Message::ToggleMenu(Menu::Picker));
    let asked: Vec<bool> = sent(&mut app)
        .into_iter()
        .filter_map(|request| match request {
            Request::AiModels { all, .. } => Some(all),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [true, true], "each AI account again, closed models too");
    let _ = app.update(Message::ToggleMenu(Menu::Picker));
    assert!(sent(&mut app).is_empty(), "closing asks nothing");
}

/// Task S.26: a model a choice holds that is turned off – here the travel planner's – shows as
/// turned off in the picker and the bot editor; the core answers with "Automatic" then.
#[test]
fn a_model_that_is_off_is_named_as_turned_off() {
    let mut app = started(&[]);
    let _ = app.update(Message::NewBotChat(TRAVEL.into()));
    let off = Choice { engine: OLLAMA.into(), model: Some("llama3.3:70b".into()) };
    assert_eq!(app.choice.as_ref(), Some(&off), "the bot's model stays the choice");
    assert!(app.turned_off(&off));
    assert!(!app.turned_off(&Choice { engine: OLLAMA.into(), model: Some("qwen3:14b".into()) }), "on");
    assert!(!app.turned_off(&Choice { engine: OLLAMA.into(), model: None }), "Automatic");
    assert!(app.turned_off(&Choice { engine: OLLAMA.into(), model: Some("gone:1b".into()) }), "not listed");
    let _ = app.update(Message::ToggleMenu(Menu::Picker));
    settle(&mut app);
    let _ = app.view();

    let mut editor = started(&["--open", "bot-editor"]);
    settle(&mut editor);
    let draft = editor.bot_draft.as_ref().expect("the editor is open");
    assert_eq!(draft.choice.as_ref(), Some(&off));
    let _ = editor.update(Message::ToggleMenu(Menu::BotPicker));
    settle(&mut editor);
    let _ = editor.view();
}

/// Task S.26: AI accounts, but no model on at any – the core lists none as an engine: the welcome
/// says so and leads to the accounts, not "No AI account yet".
#[test]
fn without_a_model_on_the_welcome_says_where_to_turn_one_on() {
    let mut app = started(&["--open", "no-engines"]);
    assert!(app.engines_loaded && app.engines.is_empty());
    assert!(!app.no_model_on(), "the demo has no AI account then");
    assert!(app.sent.is_empty(), "asked once, answered");
    let _ = app.update(Message::AiAccounts(Ok(true)));
    assert!(app.no_model_on());
    let _ = app.view();
    let _ = app.update(Message::ManageModels);
    assert!(app.sections.is_empty());
    assert_eq!(pane(&app).host.current().id, AI_PAGE);

    // Once an AI account is an engine again, models are on.
    let mut on = started(&[]);
    assert!(on.ai_accounts && !on.no_model_on());
    let asked: Vec<Request> = sent(&mut on);
    assert!(!asked.iter().any(|request| matches!(request, Request::Accounts)), "nothing to ask: {asked:?}");
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
    let settings =
        Settings { ai_slots: torro_core::ai::AiSlots { chat: Some(default), decide: None }, ..Settings::default() };
    let _ = app.update(Message::Settings(Ok(Box::new(settings))));
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
            about: None,
            spoken: false,
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

    let _ = app.update(Message::Decide(APPROVAL_ID.into(), chat::Answer::Allow));
    assert_eq!(sent(&mut app), [chat::decide_request(APPROVAL_ID, chat::Answer::Allow)]);
    assert_eq!(app.deciding.as_deref(), Some(APPROVAL_ID), "the buttons are off until the core answered");
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), chat::Answer::Deny));
    assert!(sent(&mut app).is_empty(), "a second click sends nothing");

    let result = block_on(app.backend.ask(chat::decide_request(APPROVAL_ID, chat::Answer::Allow)));
    let _ = app.update(Message::Decided(APPROVAL_ID.into(), approval_of(result)));
    assert_eq!(app.deciding, None);
    assert!(matches!(app.approvals.get(APPROVAL_ID).map(|a| &a.state), Some(ApprovalState::Done { .. })));
    settle(&mut app);
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), chat::Answer::Deny));
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
    let _ = app.update(Message::Decide(APPROVAL_ID.into(), chat::Answer::Deny));
    assert_eq!(sent(&mut app), [Request::DecideApproval { id: APPROVAL_ID.into(), allow: false, scope: None }]);
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

/// Issue #697: the bin at a row asks first and deletes that one conversation.
#[test]
fn the_bin_at_a_row_asks_and_deletes_its_conversation() {
    let mut app = started(&[]);
    sent(&mut app);
    let row = app.conversations.iter().find(|c| !c.pinned && !c.archived).map(|c| c.id.clone()).expect("a recent one");
    let _ = app.update(Message::AskDelete(row.clone()));
    assert!(matches!(&app.dialog, Some(Dialog::Delete { id, .. }) if *id == row));
    assert!(sent(&mut app).is_empty(), "deleting asks first");
    let _ = app.update(Message::ConfirmDelete);
    assert_eq!(sent(&mut app), [Request::DeleteConversation { id: row }]);
}

/// Issue #697: "Archive all…" and "Delete all…" take exactly what stands under "Recent", whatever the
/// search shows – no pinned and no archived one –, ask with the number and go out as one request.
#[test]
fn archive_all_and_delete_all_take_the_recent_conversations() {
    let mut app = started(&[]);
    sent(&mut app);
    let recent: Vec<String> =
        app.conversations.iter().filter(|c| !c.pinned && !c.archived).map(|c| c.id.clone()).collect();
    let ordered: Vec<String> = chat::sidebar_groups(&app.conversations, "").1.iter().map(|c| c.id.clone()).collect();
    assert!(!recent.is_empty() && recent.len() < app.conversations.len());
    let _ = app.update(Message::Search("Lisbon".into()));
    sent(&mut app);

    let _ = app.update(Message::AskArchiveAll);
    assert_eq!(app.dialog, Some(Dialog::ArchiveMany { ids: ordered.clone() }), "the search narrows nothing");
    assert!(sent(&mut app).is_empty(), "it asks first");
    let _ = app.update(Message::ConfirmMany);
    assert_eq!(sent(&mut app), [Request::ArchiveConversations { ids: ordered.clone(), archived: true }]);
    assert!(app.conversations.iter().filter(|c| recent.contains(&c.id)).all(|c| c.archived));

    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::AskDeleteAll);
    assert_eq!(app.dialog, Some(Dialog::DeleteMany { ids: ordered.clone(), archive: false }));
    // One gone meanwhile (another window): the rest still go, so the core does not refuse them all.
    app.conversations.retain(|c| c.id != ordered[0]);
    let _ = app.update(Message::ConfirmMany);
    let requests = sent(&mut app);
    assert_eq!(requests, [Request::DeleteConversations { ids: ordered[1..].to_vec() }]);
    let result = block_on(app.backend.ask(requests[0].clone()));
    let _ = app.update(answer_of(&requests[0], result));
    assert!(app.conversations.iter().all(|c| !ordered.contains(&c.id)));
    assert!(app.conversations.iter().any(|c| c.pinned) && app.conversations.iter().any(|c| c.archived), "these stay");
}

/// Issue #697: archived conversations stand closed in "Archive"; restoring one and emptying the archive
/// go to the core.
#[test]
fn the_archive_restores_and_empties() {
    let mut app = started(&[]);
    sent(&mut app);
    let archived: Vec<String> = chat::archived_group(&app.conversations, "").iter().map(|c| c.id.clone()).collect();
    assert_eq!(archived.len(), 3, "the demo's archive");
    assert!(!app.archive_open, "closed at the start");
    let _ = app.update(Message::ToggleArchive);
    assert!(app.archive_open);

    let _ = app.update(Message::Archive(vec![archived[0].clone()], false));
    assert_eq!(sent(&mut app), [Request::ArchiveConversations { ids: vec![archived[0].clone()], archived: false }]);
    assert!(app.conversations.iter().any(|c| c.id == archived[0] && !c.archived), "shown at once");

    let _ = app.update(Message::AskEmptyArchive);
    assert_eq!(app.dialog, Some(Dialog::DeleteMany { ids: archived[1..].to_vec(), archive: true }));
    let _ = app.update(Message::ConfirmMany);
    assert_eq!(sent(&mut app), [Request::DeleteConversations { ids: archived[1..].to_vec() }]);
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
    // "Set up access" opens Access in this window (task S.34), no request of the window.
    assert_eq!(pane(&app).host.current().id, settings::access::ID);
    assert!(app.sections.is_empty(), "not the control centre");
    assert!(sent(&mut app).is_empty());

    let mut none = started(&["--open", "no-engines"]);
    assert!(none.engines_loaded && none.engines.is_empty());
    assert_eq!(none.choice, None, "nothing can answer");
    let _ = none.update(Message::AddAiAccount);
    // Issue #410: "Add an AI account" opens AI in this window's settings.
    assert!(none.sections.is_empty(), "not the control centre");
    assert_eq!(pane(&none).host.current().id, AI_PAGE, "\"Add an AI account\" opens AI");
}

/// Task S.34: "Change access" under a refused tool opens Access in this window, at the assistant –
/// or at the bot whose conversation is open.
#[test]
fn change_access_under_a_refused_tool_opens_access() {
    let mut app = started(&["--open", "forbidden"]);
    let _ = app.view();
    let _ = app.update(Message::SetUp);
    assert_eq!(pane(&app).host.current().id, settings::access::ID);
    settings_loaded(&mut app);
    assert!(settings::access::open_bot(&pane(&app).sections.access, pane(&app).host.data()).is_none());
    assert!(app.sections.is_empty(), "not the control centre");
    assert_eq!(AI_PAGE, "ai");

    let mut bot = started(&["--open", "bot"]);
    assert_eq!(bot.chat_bot(), Some(TRAVEL));
    let _ = bot.update(Message::SetUp);
    settings_loaded(&mut bot);
    let open = settings::access::open_bot(&pane(&bot).sections.access, pane(&bot).host.data());
    assert_eq!(open.map(|summary| summary.id.as_str()), Some(TRAVEL), "the bot's own rights");
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
        max_output: None,
        parameters: None,
        quantization: None,
        size: None,
        reasoning: Some(reasoning),
        remote: None,
        decision: None,
        enabled: false,
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

/// Issue #831: a conversation the core measured shows how full its context is in every size; one without a
/// measurement shows nothing; once shortened it says so at 100 %.
#[test]
fn the_header_shows_how_full_the_context_is() {
    let mut app = started(&["--chat", TOOLS]);
    let fill = app.context_fill().expect("the demo measured it");
    assert_eq!(crate::view::context_percent(fill), 65);
    assert_eq!(crate::view::context_label(fill), trf("{percent}%", &[("percent", &65)]));
    assert!(crate::view::context_tip(fill).contains(&torro_gui::units::format_count(32_000)));
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
    let shortened = torro_ipc::assistant::ContextFill { used: 40_000, shortened: true, ..fill };
    assert_eq!(crate::view::context_percent(shortened), 100);
    assert_eq!(crate::view::context_label(shortened), tr("Shortened"));
    let over = torro_ipc::assistant::ContextFill { used: 40_000, ..fill };
    assert_eq!(crate::view::context_percent(over), 100, "never past the bar");

    let app = started(&["--chat", RELEASE]);
    assert_eq!(app.context_fill(), None, "not measured");
    let app = started(&[]);
    assert_eq!(app.context_fill(), None, "a new chat");
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
    let travel = Choice { engine: OLLAMA.into(), model: Some("llama3.3:70b".into()) };
    assert_eq!(app.choice, Some(travel), "the bot's engine and model");
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
                    Request::UpdateClient { .. }
                        | Request::CreateClient { .. }
                        | Request::RemoveClient { .. }
                        | Request::NewClientKey { .. }
                        | Request::ClientActivity { .. }
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

/// Task S.34: "Access…" and the bot editor's "What the bot may use" open Access at the bot, in this
/// window, the settings open or not.
#[test]
fn access_opens_the_bots_rights_and_a_window_is_asked_of_the_core() {
    for open in [false, true] {
        let mut app = started(&["--open", "bot-editor"]);
        if open {
            let _ = app.update(Message::ToggleSettings);
        }
        sent(&mut app);
        let _ = app.update(Message::BotAccess(TRAVEL.into()));
        settings_loaded(&mut app);
        // A bot's rights are a page below Bots since issue #763.
        assert_eq!(pane(&app).host.current().id, settings::bots::BOTS);
        let bot = settings::access::open_bot(&pane(&app).sections.access, pane(&app).host.data());
        assert_eq!(bot.map(|summary| summary.id.as_str()), Some(TRAVEL));
        assert!(app.sections.is_empty() && sent(&mut app).is_empty(), "no request, no control centre");
    }
    let mut app = started(&[]);
    sent(&mut app);
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

/// What the sections ask of the app: "Edit" at a bot is the bot editor, "Access" the section
/// Access at this very bot (task S.34). Neither is a request of this window.
#[test]
fn the_sections_reach_the_editor_and_access() {
    use torro_settings_ui::assistant::bots as section;

    let mut app = started(&[]);
    let _ = app.update(Message::ToggleSettings);
    settings_loaded(&mut app);
    sent(&mut app);
    let _ = app.update(Message::SettingsPage(settings::Message::Bots(section::Message::Access(TRAVEL.into()))));
    assert_eq!(pane(&app).host.current().id, section::BOTS, "the bot's rights, below Bots (issue #763)");
    let bot = settings::access::open_bot(&pane(&app).sections.access, pane(&app).host.data());
    assert_eq!(bot.map(|summary| summary.id.as_str()), Some(TRAVEL));
    assert!(app.sections.is_empty(), "not the control centre");
    let _ = app.update(Message::SettingsPage(settings::Message::Bots(section::Message::Edit(TRAVEL.into()))));
    assert!(app.settings.is_none(), "the editor takes the content area");
    assert_eq!(sent(&mut app), [Request::GetBot { id: TRAVEL.into() }]);
}

/// Issue #547: the demo states of the settings open their section and press on its page once the
/// data is in – the profile editor under General (issue #763), with a changed name its save bar, the
/// tabs of Access and a bot's rights below Bots.
#[test]
fn the_settings_demo_states_open_their_page() {
    for (word, section) in [
        ("profile-editor", "general"),
        ("profile-changed", "general"),
        ("access-selections", "access"),
        ("access-bots", "bots"),
    ] {
        let mut app = started(&["--open", word]);
        assert_eq!(pane(&app).host.current().id, section, "{word}");
        settings_loaded(&mut app);
        assert!(app.settings_steps.is_empty(), "{word}: pressed once the data is in");
        let pane = pane(&app);
        let page = settings::page(&pane.sections, pane.host.data(), section, SizeClass::Wide);
        if section == "general" {
            assert!(page.sub_page.is_some(), "{word}: the editor is open");
            assert_eq!(page.save_bar.is_some(), word == "profile-changed", "{word}");
        }
        if section == "bots" {
            let name = pane.host.data().bots.iter().find(|bot| bot.id == TRAVEL).map(|bot| bot.name.clone());
            assert_eq!(page.sub_page.map(|(name, _)| name), name, "{word}: the travel planner's rights");
        }
    }
}

/// `--bot <id> --settings access` opens that bot's rights in the bot's window (task S.34), below
/// Bots since issue #763; the main window's `--settings access` the assistant's, under Access.
#[test]
fn settings_access_with_a_bot_starts_at_the_bot() {
    let mut app = started(&["--bot", TRAVEL, "--settings", "access"]);
    assert_eq!(pane(&app).host.current().id, settings::bots::BOTS);
    settings_loaded(&mut app);
    let bot = settings::access::open_bot(&pane(&app).sections.access, pane(&app).host.data());
    assert_eq!(bot.map(|summary| summary.id.as_str()), Some(TRAVEL));
    let main = started(&["--settings", "access"]);
    assert_eq!(pane(&main).host.current().id, settings::access::ID);
    assert!(settings::access::open_bot(&pane(&main).sections.access, pane(&main).host.data()).is_none());
}

/// A right changed a moment ago is not lost when the settings close before its clock (task
/// S.34): every way out sends it first, and the settings still close.
#[test]
fn leaving_the_settings_saves_a_waiting_right() {
    use torro_ipc::api::{Area, Level};
    use torro_settings_ui::rights;

    let level = || {
        Message::SettingsPage(settings::Message::Access(settings::access::Message::Rights(rights::Message::Level(
            Area::Mail,
            Level::Read,
        ))))
    };
    for way in [
        Message::ToggleSettings,
        Message::Escape,
        Message::Select(TOOLS.into()),
        Message::SettingsHost(torro_settings_ui::host::Message::Back),
    ] {
        let mut app = started(&["--settings", "access"]);
        settings_loaded(&mut app);
        let _ = app.update(level());
        let task = app.update(way.clone());
        assert!(app.settings.is_none(), "{way:?}: the settings close");
        assert!(task.units() > 0, "{way:?}: the change is on its way");
    }
    // Nothing waits: nothing extra goes.
    let mut app = started(&["--settings", "access"]);
    settings_loaded(&mut app);
    assert!(app.save_before_leaving(&Message::ToggleSettings).is_none());
}

/// Task S.34, the boundary in this process: the window's own code sends no `clients.*` but reading
/// the list – no client made, removed or changed, no key, no activity, no rights editor. Those are
/// the settings section Access's alone (`torro_settings_ui::assistant::access`), under its own test.
#[test]
fn the_window_itself_never_touches_clients() {
    // Spelled in pieces: this test's own words are not what it looks for.
    let forbidden = [
        ["Create", "Client"].concat(),
        ["Remove", "Client"].concat(),
        ["Update", "Client"].concat(),
        ["NewClient", "Key"].concat(),
        ["Client", "Activity"].concat(),
        ["rights", "::"].concat(),
        ["rights", " as"].concat(),
        ["Mail", "ToCloud"].concat(),
        // The window hands Access no message of its own: what changes rights comes from a person's
        // click in the section, never from the window's code.
        ["access::", "Message"].concat(),
    ];
    // This file names what it checks; every other file of the window is read whole.
    let this = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/tests.rs");
    let mut found = Vec::new();
    let mut files = 0;
    let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { panic!("cannot read {}", dir.display()) };
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") || path == this {
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&path).expect("a source file reads");
            let code = text.lines().take_while(|line| line.trim() != "#[cfg(test)]");
            for line in code.filter(|line| !line.trim_start().starts_with("//")) {
                found.extend(
                    forbidden
                        .iter()
                        .filter(|word| line.contains(word.as_str()))
                        .map(|word| format!("{}: {word}", path.display())),
                );
            }
        }
    }
    assert!(files > 10, "the window's sources were read");
    assert!(found.is_empty(), "the window reaches into the rights: {found:?}");
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
        content: EntryContent::Answer { text: text.into(), engine: OLLAMA.into(), model: None, spoken: None },
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

/// Issue #704: from two characters on the search asks the core; an answer for other words than those in
/// the field is dropped, and fewer than two characters filter the titles only.
#[test]
fn the_search_asks_the_core_and_drops_stale_hits() {
    let mut app = started(&[]);
    sent(&mut app);
    let _ = app.update(Message::Search("A".into()));
    assert!(sent(&mut app).is_empty(), "one character filters the titles");
    let _ = app.update(Message::Search(" An ".into()));
    assert_eq!(sent(&mut app), [Request::SearchConversations { query: "An".into(), limit: Some(MAX_HITS) }]);
    let _ = app.update(Message::Search("Ann".into()));
    sent(&mut app);
    let _ = app.update(Message::Hits("An".into(), Ok(Vec::new())));
    assert!(app.hits.is_none(), "stale");
    let _ = app.update(Message::Hits("Ann".into(), Ok(Vec::new())));
    assert_eq!((app.hits.clone(), app.hits_for.as_str()), (Some(Ok(Vec::new())), "Ann"));
    let _ = app.update(Message::Search("Anx".into()));
    assert_eq!((app.hits_for.as_str(), app.asked.as_str()), ("Ann", "Anx"), "the last hits stay while asking");
    let _ = app.update(Message::Hits("Anx".into(), Ok(Vec::new())));
    assert_eq!(app.hits_for, "Anx");
    let _ = app.update(Message::Search("A".into()));
    assert!(app.hits.is_none(), "back to the titles");
}

/// Issue #704: a hit opens its conversation with the entry marked and the history not following the
/// newest entry; choosing another conversation unmarks it.
#[test]
fn a_hit_opens_its_conversation_at_the_place() {
    let mut app = started(&[]);
    let _ = app.update(Message::Search(demo::SEARCH_WORD.into()));
    settle(&mut app);
    let hit = match &app.hits {
        Some(Ok(hits)) => hits.first().cloned().expect("the demo holds the word"),
        other => panic!("expected hits, got {other:?}"),
    };
    let seq = hit.seq.expect("an entry");
    let _ = app.update(Message::OpenHit(hit.conversation.clone(), Some(seq)));
    settle(&mut app);
    assert_eq!(app.current.as_deref(), Some(hit.conversation.as_str()));
    assert_eq!(app.marked, Some((hit.conversation.clone(), seq)));
    assert!(app.transcript.entries.iter().any(|entry| entry.seq == seq), "the entry is there");
    assert!(!app.follow_bottom && app.snap.is_none(), "scrolled to the hit, not to the newest");
    let other = app.conversations.iter().find(|c| c.id != hit.conversation).map(|c| c.id.clone()).expect("another");
    let _ = app.update(Message::Select(other));
    assert_eq!(app.marked, None);
    // A deleted conversation goes from the hits: listing again searches again.
    sent(&mut app);
    let _ = app.update(Message::Listed(Ok(app.conversations.clone())));
    assert!(sent(&mut app).iter().any(|request| matches!(request, Request::SearchConversations { .. })));
}

/// Issue #699: the microphone dictates into the field and sends nothing; a second press stops the
/// recording, the text then comes as it was heard.
#[test]
fn the_microphone_writes_into_the_field_without_sending() {
    let mut app = started(&[]);
    assert!(app.can_dictate(), "the demo has speech recognition");
    sent(&mut app);
    let _ = app.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Note:".into())))));
    let _ = app.update(Message::Dictate);
    assert_eq!(app.dictation, Dictation::Recording);
    let requests = sent(&mut app);
    assert!(matches!(requests.as_slice(), [Request::Dictate]), "{requests:?}");
    let _ = app.update(Message::Dictate);
    assert_eq!(app.dictation, Dictation::Stopping);
    assert!(matches!(sent(&mut app).as_slice(), [Request::StopDictation]));
    let _ = app.update(Message::Dictate);
    assert!(sent(&mut app).is_empty(), "nothing more while the speech turns into text");
    let _ = app.update(Message::Dictated(Ok("call Anna".into())));
    assert_eq!(app.dictation, Dictation::Idle);
    assert_eq!(app.editor.text().trim_end(), "Note: call Anna", "a space in front, at the caret");
    assert!(!sent(&mut app).iter().any(|request| matches!(request, Request::SendMessage { .. })), "nothing is sent");
    // At the start of the field nothing stands before the caret: no space in front.
    let _ = app.update(Message::Edit(text_editor::Action::Move(text_editor::Motion::DocumentStart)));
    let _ = app.update(Message::Dictate);
    let _ = app.update(Message::Dictated(Ok("Please".into())));
    assert!(
        app.editor.text().starts_with("Please Note: call Anna"),
        "a space behind, before the word: {:?}",
        app.editor.text()
    );
}

/// Issue #699: nothing understood, speech recognition gone and other failures each say so; the
/// field stays as it was.
#[test]
fn a_dictation_that_brings_nothing_says_why() {
    let mut app = started(&[]);
    let _ = app.update(Message::Dictate);
    let _ = app.update(Message::Dictated(Ok("  ".into())));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Nothing was understood.")));
    assert!(app.editor.text().trim().is_empty());
    sent(&mut app);
    let _ = app.update(Message::Dictate);
    let _ = app.update(Message::Dictated(Err(Refusal::new("unsupported", "whisper-cli is missing"))));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Speech recognition isn't set up.")));
    assert!(
        sent(&mut app).iter().any(|request| matches!(request, Request::VoiceStatus)),
        "asks again how voice stands"
    );
    let _ = app.update(Message::Dictate);
    let _ = app.update(Message::Dictated(Err(Refusal::new("error", "the recording failed"))));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Couldn't dictate.")));
    assert_eq!(app.dictation, Dictation::Idle);
}

/// Issue #699: without speech recognition the microphone opens the settings and records nothing; since
/// issue #823 at Assistant › Voice › Models, once the page has its data.
#[test]
fn without_speech_recognition_the_microphone_opens_the_settings() {
    let mut app = started(&[]);
    app.voice = Some(torro_ipc::VoiceStatus { installed: true, ..torro_ipc::VoiceStatus::default() });
    assert!(!app.can_dictate());
    sent(&mut app);
    let _ = app.update(Message::Dictate);
    assert_eq!(app.dictation, Dictation::Idle);
    assert_eq!(pane(&app).host.current().id, VOICE_PAGE);
    assert!(!sent(&mut app).iter().any(|request| matches!(request, Request::Dictate)));
    settings_loaded(&mut app);
    assert!(app.settings_steps.is_empty(), "pressed once the data is in");
    assert_eq!(pane(&app).sections.voice_tab, settings::VoiceTab::Models);
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

/// Issue #699: a bot's chat has neither the plus with the areas nor the mode; the assistant's has
/// both, and the composer builds in every size while it records.
#[test]
fn a_bots_chat_has_no_areas_and_no_mode() {
    assert!(started(&[]).offers_areas_and_mode());
    assert!(!started(&["--bot", TRAVEL]).offers_areas_and_mode());
    let mut app = started(&["--open", "dictating"]);
    assert_eq!(app.dictation, Dictation::Recording);
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

/// Issues #745 and #747 (decision 2): with the speaker on, a new answer is read aloud from its first
/// sentence by the core, once per message, also one in parts; an answer that was there on opening is
/// not, off reads nothing, and neither does the window's own conversation by voice.
#[test]
fn a_new_answer_is_read_aloud_from_its_first_sentence() {
    let reads = |requests: Vec<Request>| -> Vec<Request> {
        requests
            .into_iter()
            .filter(|request| matches!(request, Request::SpeakAnswer { .. } | Request::Speak { .. }))
            .collect()
    };
    let hello = || Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Hello".into()))));
    let mut app = started(&[]);
    app.read_aloud = true;
    let _ = app.update(hello());
    let _ = app.update(Message::Send);
    // The demo's answer comes at once: it is read as it begins, by the core.
    assert_eq!(reads(settled(&mut app, |_| None)), [Request::SpeakAnswer { conversation: id_of(&app) }]);
    // The answer as it goes on, and a second part after a tool.
    let last = app.transcript.entries.iter().map(|entry| entry.seq).max().unwrap_or_default();
    let entries = vec![answer_entry(last, "Let me look. Found it."), answer_entry(last + 1, "Three events.")];
    let _ = app.update(Message::Followed(id_of(&app), Ok((head(&app, last + 1, false), entries))));
    assert!(reads(sent(&mut app)).is_empty(), "read once, its parts with it");
    app.reading = Some(last);
    let conversation = app.current.clone();
    let _ = app.update(Message::Read(conversation, last, Ok(())));
    assert_eq!(app.reading, None);

    // A new chat stops what the old conversation reads, and [■] never stands at another's answer.
    app.reading = Some(last);
    let _ = app.update(Message::NewChat);
    assert!(sent(&mut app).contains(&Request::StopSpeaking));
    assert_eq!(app.reading, None);

    // The window's own conversation by voice: the listener has its answers read, the window does not.
    let mut talking = started(&["--chat", TOOLS]);
    talking.read_aloud = true;
    talking.talk =
        Some(torro_ipc::Talk { conversation: Some(TOOLS.into()), state: torro_ipc::VoiceState::Answering, tool: None });
    sent(&mut talking);
    let next = talking.transcript.last() + 1;
    let summary = head(&talking, next, false);
    let _ = talking.update(Message::Followed(TOOLS.into(), Ok((summary, vec![answer_entry(next, "Hi.")]))));
    assert!(reads(sent(&mut talking)).is_empty());

    // Off: a new answer is only shown.
    let mut quiet = started(&[]);
    let _ = quiet.update(hello());
    let _ = quiet.update(Message::Send);
    assert!(reads(settled(&mut quiet, |_| None)).is_empty());
    // Opening a conversation reads nothing of what is there.
    let mut opened = started(&["--chat", TOOLS]);
    opened.read_aloud = true;
    let _ = opened.update(Message::Select(TOOLS.into()));
    assert!(reads(settled(&mut opened, |_| None)).is_empty());
    assert_eq!(opened.reading, None);
}

/// Issue #747, done 5: "Talk" starts a conversation by voice in the open conversation; the line of
/// the listener takes the composer's place while it is the window's own, and "End talk" ends it. A
/// conversation by voice elsewhere leaves the composer as it is; "Talk" is off while an answer runs.
#[test]
fn talk_starts_a_conversation_by_voice_in_the_window() {
    use torro_ipc::{Talk, VoiceState};
    let mut app = started(&["--chat", TOOLS]);
    assert!(app.can_talk(), "the demo's microphone can record");
    sent(&mut app);
    let _ = app.update(Message::Talk);
    assert_eq!(sent(&mut app), [Request::StartTalk { conversation: Some(TOOLS.into()) }]);
    assert!(app.talk_starting, "turning arrows until the core answers");
    let _ = app.view();
    let _ = app.update(Message::Talk);
    assert!(sent(&mut app).is_empty(), "no second start");
    let talk = Talk { conversation: Some(TOOLS.into()), state: VoiceState::Conversing, tool: None };
    let _ = app.update(Message::TalkStarted(Ok(Some(talk.clone()))));
    assert_eq!(app.own_talk(), Some(&talk));
    for state in [VoiceState::Recording, VoiceState::Answering, VoiceState::Conversing] {
        let _ = app.update(Message::TalkStatus(Ok((Some(Talk { state, ..talk.clone() }), false))));
        let _ = app.view();
    }
    let _ = app.update(Message::EndTalk);
    assert_eq!(sent(&mut app), [Request::EndTalk]);
    assert_eq!(app.own_talk(), None);
    let _ = app.update(Message::TalkStarted(Err(Refusal::new("unsupported", "the assistant is switched off"))));
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(tr("Couldn't start the conversation.")));

    // In another conversation: the composer stays, the tray shows it.
    let _ = app.update(Message::TalkStatus(Ok((
        Some(Talk { conversation: Some("elsewhere".into()), ..talk.clone() }),
        false,
    ))));
    assert_eq!(app.own_talk(), None);
    let _ = app.update(Message::TalkStatus(Ok((None, false))));
    assert_eq!(app.talk, None);

    // Escape ends it, as "End talk" does.
    let _ = app.update(Message::TalkStatus(Ok((Some(talk.clone()), false))));
    sent(&mut app);
    let _ = app.update(Message::Escape);
    assert!(sent(&mut app).contains(&Request::EndTalk));
    assert_eq!(app.own_talk(), None);

    // Not in a bot's chat: the listener talks to the assistant only.
    assert!(!started(&["--bot", TRAVEL]).offers_talk());

    // Off while an answer runs.
    let mut busy = answering();
    assert!(!busy.can_talk());
    sent(&mut busy);
    let _ = busy.update(Message::Talk);
    assert!(sent(&mut busy).is_empty());
}

/// Issue #747: "Talk" in a new chat waits for the first sentence; once it started a conversation, the
/// window shows that one, and the conversation by voice goes on there.
#[test]
fn talk_in_a_new_chat_follows_into_the_new_conversation() {
    use torro_ipc::{Talk, VoiceState};
    let mut app = started(&[]);
    assert_eq!(app.current, None);
    sent(&mut app);
    let _ = app.update(Message::Talk);
    assert_eq!(sent(&mut app), [Request::StartTalk { conversation: None }]);
    let _ = app.update(Message::TalkStarted(Ok(Some(Talk {
        conversation: None,
        state: VoiceState::Conversing,
        tool: None,
    }))));
    assert!(app.own_talk().is_some(), "the line while the first sentence is not said");
    let moved = Talk { conversation: Some(TOOLS.into()), state: VoiceState::Answering, tool: None };
    let _ = app.update(Message::TalkStatus(Ok((Some(moved.clone()), false))));
    assert_eq!(app.current.as_deref(), Some(TOOLS), "the window shows the new conversation");
    assert_eq!(app.own_talk(), Some(&moved));
    // One begun in a conversation is not followed into it from a new chat.
    let mut left = started(&["--chat", TOOLS]);
    let _ = left.update(Message::TalkStarted(Ok(Some(Talk {
        conversation: Some(TOOLS.into()),
        state: VoiceState::Conversing,
        tool: None,
    }))));
    let _ = left.update(Message::NewChat);
    let _ = left.update(Message::TalkStatus(Ok((
        Some(Talk { conversation: Some(TOOLS.into()), state: VoiceState::Conversing, tool: None }),
        false,
    ))));
    assert_eq!(left.current, None, "the new chat stays");
    // One started elsewhere is not followed.
    let mut other = started(&[]);
    let _ = other.update(Message::TalkStatus(Ok((Some(moved), false))));
    assert_eq!(other.current, None);
}

/// Issue #747: the demo state `talk` shows the line of a conversation by voice in every size.
#[test]
fn the_line_of_a_conversation_by_voice_builds_in_every_size() {
    let mut app = started(&["--open", "talk"]);
    assert!(app.own_talk().is_some());
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

/// Issue #745: the speaker switches the setting and back when the core refuses; switching it off
/// and [■] stop what is read at once; a failure says why.
#[test]
fn the_speaker_switches_reading_aloud_and_stops_it() {
    let mut app = started(&["--open", "reading"]);
    assert!(app.read_aloud && app.reading == Some(demo::READ_ANSWER));
    sent(&mut app);
    let _ = app.update(Message::StopReading);
    assert_eq!(sent(&mut app), [Request::StopSpeaking]);
    assert_eq!(app.reading, None);

    // Off stops only what this window reads.
    let _ = app.update(Message::ToggleReadAloud);
    assert!(!sent(&mut app).contains(&Request::StopSpeaking), "nothing of this window reads");
    let _ = app.update(Message::ToggleReadAloud);
    sent(&mut app);
    app.reading = Some(demo::READ_ANSWER);
    let _ = app.update(Message::ToggleReadAloud);
    let requests = sent(&mut app);
    let off = Request::SetSetting { key: torro_ipc::SettingKey::ReadAloud, value: serde_json::json!(false) };
    assert!(requests.contains(&off) && requests.contains(&Request::StopSpeaking), "{requests:?}");
    assert!(!app.read_aloud && app.reading.is_none());
    let _ = app.update(Message::ToggleReadAloud);
    assert!(app.read_aloud);
    let _ = app.update(Message::ReadAloudSet(true, Err(Refusal::new("forbidden", "no"))));
    assert!(!app.read_aloud, "back as it was");

    // Escape stops reading first; settings loaded while the switch is on its way do not turn it back.
    app.reading = Some(4);
    let _ = app.update(Message::Escape);
    assert_eq!(app.reading, None);
    assert!(sent(&mut app).contains(&Request::StopSpeaking));
    let _ = app.update(Message::ToggleReadAloud);
    assert!(app.read_aloud);
    let mut stale = torro_core::settings::Settings::default();
    stale.voice.read_aloud = false;
    app.launch.open = None;
    let _ = app.update(Message::Settings(Ok(Box::new(stale.clone()))));
    assert!(app.read_aloud, "the old value of a load that overlapped the switch");
    let _ = app.update(Message::ReadAloudSet(true, Ok(())));
    let _ = app.update(Message::Settings(Ok(Box::new(stale))));
    assert!(!app.read_aloud, "switched in another window afterwards");

    let message = "Connect an OpenAI account in Settings › AI to read aloud with OpenAI.";
    app.reading = Some(4);
    let _ = app.update(Message::Read(app.current.clone(), 4, Err(Refusal::new("unsupported", message))));
    assert_eq!(app.reading, None);
    assert_eq!(app.notice.as_ref().map(|notice| notice.text.as_str()), Some(message), "the core's words");
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

/// Issue #774: with the speaker on a message asks for an answer in two parts, off it does not;
/// "Repeat" reads the spoken part again (`assistant.speak`) and "Stop" stops it
/// (`assistant.stop_speaking`); an answer without a spoken part has nothing to repeat.
#[test]
fn repeat_reads_the_spoken_part_again_and_stop_stops_it() {
    let asked_spoken = |requests: &[Request]| {
        requests.iter().find_map(|request| match request {
            Request::SendMessage { spoken, .. } => Some(*spoken),
            _ => None,
        })
    };
    let hello = || Message::Edit(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("Hello".into()))));
    for on in [true, false] {
        let mut app = started(&[]);
        app.read_aloud = on;
        let _ = app.update(hello());
        let _ = app.update(Message::Send);
        assert_eq!(asked_spoken(&sent(&mut app)), Some(on), "two parts only with the speaker on");
    }

    let mut app = started(&["--open", "reading"]);
    sent(&mut app);
    let spoken = app.transcript.entries.iter().find_map(|entry| match &entry.content {
        EntryContent::Answer { spoken: Some(spoken), .. } if entry.seq == demo::READ_ANSWER => Some(spoken.clone()),
        _ => None,
    });
    let spoken = spoken.expect("the demo's answer has a spoken part");
    // "Repeat" at the answer being read: the reading it replaces ends late and leaves "Stop" active.
    assert_eq!(app.reading, Some(demo::READ_ANSWER));
    let _ = app.update(Message::RepeatSpoken(demo::READ_ANSWER));
    assert_eq!(sent(&mut app), [Request::Speak { text: spoken.clone() }]);
    let _ = app.update(Message::Read(app.current.clone(), demo::READ_ANSWER, Ok(())));
    assert_eq!(app.reading, Some(demo::READ_ANSWER), "\"Stop\" is active meanwhile");
    // A second "Repeat": the first one's end does not take "Stop" away either; its own end does.
    let _ = app.update(Message::RepeatSpoken(demo::READ_ANSWER));
    sent(&mut app);
    let _ = app.update(Message::Repeated(1, Ok(())));
    assert_eq!(app.reading, Some(demo::READ_ANSWER));
    let _ = app.update(Message::Repeated(2, Ok(())));
    assert_eq!(app.reading, None, "read to its end");
    let _ = app.update(Message::RepeatSpoken(demo::READ_ANSWER));
    sent(&mut app);
    let _ = app.update(Message::StopReading);
    assert_eq!(sent(&mut app), [Request::StopSpeaking]);
    assert_eq!(app.reading, None);

    // An answer without a spoken part, another entry or an unknown number: nothing to read again.
    let plain = app.transcript.entries.iter().map(|entry| entry.seq).max().unwrap_or_default() + 1;
    app.transcript.entries.push(answer_entry(plain, "Just one part."));
    let _ = app.update(Message::RepeatSpoken(plain));
    let message = app.transcript.entries.iter().find(|entry| matches!(entry.content, EntryContent::Message { .. }));
    let _ = app.update(Message::RepeatSpoken(message.map_or(0, |entry| entry.seq)));
    let _ = app.update(Message::RepeatSpoken(999));
    app.transcript.entries.retain(|entry| entry.seq != plain);
    assert!(sent(&mut app).is_empty());

    // In its own conversation by voice "Stop" follows what the listener reads.
    app.talk = Some(torro_ipc::Talk {
        conversation: app.current.clone(),
        state: torro_ipc::VoiceState::Answering,
        tool: None,
    });
    let _ = app.update(Message::TalkStatus(Ok((app.talk.clone(), true))));
    assert!(app.talk_speaking);
    let _ = app.update(Message::StopReading);
    assert!(!app.talk_speaking && sent(&mut app).contains(&Request::StopSpeaking));
    for size in SIZES {
        let _ = app.update(Message::Resized(size));
        let _ = app.view();
    }
}

#[test]
fn the_history_follows_growth_but_not_a_person_who_scrolled_up() {
    // At the bottom, nothing seen yet: follows, jumps only when it was not following.
    assert_eq!(history_follow(true, None, 1000.0, 600.0, 600.0, 24.0), (true, false));
    assert_eq!(history_follow(false, None, 1000.0, 600.0, 600.0, 24.0), (true, true));
    // Content grew, the offset stayed (far from the new bottom): follows and jumps.
    assert_eq!(history_follow(true, Some((1000.0, 600.0)), 1400.0, 600.0, 1000.0, 24.0), (true, true));
    // The person scrolled up: stops following, no jump.
    assert_eq!(history_follow(true, Some((1000.0, 600.0)), 1000.0, 300.0, 600.0, 24.0), (false, false));
    // Scrolled up, then content grows with the offset unchanged: stays put.
    assert_eq!(history_follow(false, Some((1000.0, 300.0)), 1400.0, 300.0, 1000.0, 24.0), (false, false));
    // Scrolled back down to the bottom: follows again, jumps.
    assert_eq!(history_follow(false, Some((1000.0, 300.0)), 1000.0, 590.0, 600.0, 24.0), (true, true));
}
