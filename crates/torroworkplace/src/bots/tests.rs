//! Tests of the bots' handlers (task B.1): making a bot with a client without rights, changing it,
//! deleting it with its client, the limits, and the files on disk. The rights rules (a bot's client
//! is refused `bots` and `assistant`, the assistant's own uses bots by its kind) are tested with the
//! whole core in `crate::core`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use torro_ipc::api::{AreaRights, BOT_CLIENT, Level, Rights};
use torro_ipc::assistant::{ConversationSummary, EntryContent, TEST_ENGINE};
use torro_ipc::bots::{Bot, MAX_BOTS, Skill};
use torro_ipc::{Request, Response};

use super::*;
use crate::assistant::Assistant;

/// An assistant in `dir` with the test engine and its bots in `dir/config/assistant/bots`, and the
/// clients in `dir`.
fn set_up(dir: &Path) -> (Arc<Assistant>, Clients) {
    let assistant = Arc::new(crate::assistant::tests::assistant_in(dir, Duration::ZERO));
    (assistant, crate::clients::tests::open_in(dir))
}

/// `bots.save_bot` for a bot named `name` with two skills; `id` to change one.
fn save(id: Option<&str>, name: &str) -> Request {
    Request::SaveBot {
        id: id.map(str::to_string),
        name: name.into(),
        description: "Plans trips by train.".into(),
        avatar: None,
        engine: TEST_ENGINE.into(),
        model: None,
        personality: "Calm and brief.".into(),
        instructions: "Suggest trains before flights.\r\nAsk for the budget.\u{1b}".into(),
        skills: vec![
            Skill { name: "Packing".into(), text: "Pack light.".into() },
            Skill { name: "Night trains".into(), text: "Lisbon has none.".into() },
        ],
    }
}

fn saved(response: Response) -> Bot {
    match response {
        Response::Bot { bot } => bot,
        other => panic!("expected a bot, got {other:?}"),
    }
}

fn code(response: &Response) -> &str {
    match response {
        Response::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

/// The client `id` as the core keeps it.
fn client_of(clients: &Clients, id: &str) -> Option<torro_ipc::ClientInfo> {
    clients.list().into_iter().find(|info| info.id == id)
}

#[tokio::test]
async fn a_new_bot_gets_a_client_of_its_own_without_any_rights() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (assistant, clients) = set_up(dir.path());

    let bot = saved(answer(&assistant, &clients, save(None, "  Travel planner ")).await);
    assert!(bot.id.len() == 8 && bot.id.bytes().all(|byte| byte.is_ascii_hexdigit()), "{}", bot.id);
    assert_eq!((bot.name.as_str(), bot.avatar.as_str()), ("Travel planner", "letter"), "cleaned, the letter");
    assert_eq!(bot.instructions, "Suggest trains before flights.\nAsk for the budget.", "no control characters");
    let client = client_of(&clients, &bot.client).expect("its client");
    assert_eq!((client.name.as_str(), client.kind.as_str()), ("Travel planner", BOT_CLIENT));
    assert_eq!(client.rights, Rights::none(), "no rights until a person grants some");

    // Read back whole, and listed without its texts.
    let Response::Bot { bot: read } = answer(&assistant, &clients, Request::GetBot { id: bot.id.clone() }).await else {
        panic!("the bot")
    };
    assert_eq!(read, bot);
    let listed = answer(&assistant, &clients, Request::ListBots).await;
    assert_eq!(listed, Response::Bots { bots: vec![bot.summary()] });

    // On disk: its folder 0700, its files 0600, the texts readable.
    let folder = dir.path().join("config/assistant/bots").join(&bot.id);
    assert_eq!(std::fs::read_to_string(folder.join("skills/2.md")).expect("the second skill"), "Lisbon has none.\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&dir.path().join("config/assistant/bots")), 0o700);
        assert_eq!(mode(&folder), 0o700);
        for file in ["bot.json", "persona.md", "instructions.md", "skills/1.md", "skills/2.md"] {
            assert_eq!(mode(&folder.join(file)), 0o600, "{file}");
        }
    }
}

#[tokio::test]
async fn changing_a_bot_keeps_its_client_and_rights_and_renames_the_client() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (assistant, clients) = set_up(dir.path());
    let bot = saved(answer(&assistant, &clients, save(None, "Travel planner")).await);
    let mut granted = Rights::none();
    granted.areas.insert(torro_ipc::api::Area::Calendar, AreaRights { level: Level::Read, admin: false });
    clients.update(&bot.client, granted.clone()).await.expect("a person grants it reading calendars");

    let mut change = save(Some(&bot.id), "Trip planner");
    if let Request::SaveBot { skills, avatar, .. } = &mut change {
        skills.truncate(1);
        *avatar = Some("sun".into());
    }
    let changed = saved(answer(&assistant, &clients, change).await);
    assert_eq!((changed.id.as_str(), changed.client.as_str()), (bot.id.as_str(), bot.client.as_str()));
    assert_eq!((changed.name.as_str(), changed.avatar.as_str(), changed.skills.len()), ("Trip planner", "sun", 1));
    let client = client_of(&clients, &bot.client).expect("the same client");
    assert_eq!((client.name.as_str(), &client.rights), ("Trip planner", &granted), "renamed, rights kept");
    assert_eq!(clients.list().len(), 1, "no second client");
    assert!(!dir.path().join("config/assistant/bots").join(&bot.id).join("skills/2.md").exists());

    // The person removed its client: the next save makes a new one, again without rights.
    clients.remove(&bot.client).await.expect("removed");
    let again = saved(answer(&assistant, &clients, save(Some(&bot.id), "Trip planner")).await);
    assert_ne!(again.client, bot.client);
    assert_eq!(client_of(&clients, &again.client).map(|info| info.rights), Some(Rights::none()));
}

#[tokio::test]
async fn deleting_a_bot_removes_its_client_and_folder_and_keeps_its_conversations() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (assistant, clients) = set_up(dir.path());
    let ai = crate::assistant::tests::no_ai(dir.path());
    let bot = saved(answer(&assistant, &clients, save(None, "Travel planner")).await);
    let other = saved(answer(&assistant, &clients, save(None, "Release helper")).await);

    // A direct chat with the bot (the test engine runs no tool).
    let send = Request::SendMessage {
        conversation: None,
        engine: TEST_ENGINE.into(),
        model: None,
        effort: None,
        text: "Plan Lisbon".into(),
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
        bot: Some(bot.id.clone()),
    };
    let Response::Conversation { conversation, .. } = crate::assistant::answer(&assistant, &ai, send).await else {
        panic!("a conversation with the bot")
    };
    assert_eq!((conversation.bot.as_deref(), conversation.bot_deleted), (Some(bot.id.as_str()), false));

    assert_eq!(answer(&assistant, &clients, Request::DeleteBot { id: bot.id.clone() }).await, Response::Ok);
    assert!(client_of(&clients, &bot.client).is_none(), "its client is gone");
    assert!(client_of(&clients, &other.client).is_some(), "the other bot's stays");
    assert!(!dir.path().join("config/assistant/bots").join(&bot.id).exists(), "its folder is gone");
    assert_eq!(code(&answer(&assistant, &clients, Request::GetBot { id: bot.id.clone() }).await), "not-found");
    assert_eq!(code(&answer(&assistant, &clients, Request::DeleteBot { id: bot.id.clone() }).await), "not-found");

    // The conversation stays, marked as of a deleted bot; it cannot be continued.
    let listed: Vec<ConversationSummary> =
        match crate::assistant::answer(&assistant, &ai, Request::ListConversations).await {
            Response::Conversations { conversations } => conversations,
            other => panic!("expected conversations, got {other:?}"),
        };
    assert_eq!(listed.len(), 1);
    assert!(listed[0].bot_deleted && listed[0].bot.as_deref() == Some(bot.id.as_str()));
    let more = Request::SendMessage {
        conversation: Some(conversation.id.clone()),
        engine: TEST_ENGINE.into(),
        model: None,
        effort: None,
        text: "And Porto?".into(),
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
        bot: Some(bot.id.clone()),
    };
    assert_eq!(code(&crate::assistant::answer(&assistant, &ai, more).await), "not-found");
    let read =
        crate::assistant::answer(&assistant, &ai, Request::GetConversation { id: conversation.id, after: None }).await;
    assert!(matches!(read, Response::Conversation { conversation, .. } if conversation.bot_deleted));
}

#[tokio::test]
async fn bots_keep_their_limits() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (assistant, clients) = set_up(dir.path());
    assert_eq!(code(&answer(&assistant, &clients, save(None, " ")).await), "error", "a name");
    assert_eq!(code(&answer(&assistant, &clients, save(Some("0a0b0c0d"), "Bot")).await), "not-found");
    assert_eq!(code(&answer(&assistant, &clients, Request::GetBot { id: "../config".into() }).await), "not-found");
    assert!(clients.list().is_empty(), "nothing refused makes a client");

    for n in 0..MAX_BOTS {
        saved(answer(&assistant, &clients, save(None, &format!("Bot {n}"))).await);
    }
    let refused = answer(&assistant, &clients, save(None, "One too many")).await;
    assert_eq!(code(&refused), "error");
    assert!(assistant.bots().refusal(&save(None, "One too many")).await.is_some(), "refused before an approval");
    assert_eq!(clients.list().len(), MAX_BOTS, "and no client for it");
    let Response::Bots { bots } = answer(&assistant, &clients, Request::ListBots).await else { panic!("bots") };
    assert_eq!(bots.len(), MAX_BOTS);
    assert!(bots.windows(2).all(|pair| pair[0].name.to_lowercase() <= pair[1].name.to_lowercase()), "by name");
}

#[tokio::test]
async fn without_a_folder_nothing_is_stored() {
    let dir = tempfile::tempdir().expect("temp dir");
    let assistant = Assistant::new(None, Some(Duration::ZERO), |_| Ok(()));
    let clients = crate::clients::tests::open_in(dir.path());
    assert_eq!(code(&answer(&assistant, &clients, save(None, "Travel planner")).await), "error");
    assert!(clients.list().is_empty(), "no client without a bot");
    assert_eq!(answer(&assistant, &clients, Request::ListBots).await, Response::Bots { bots: Vec::new() });
}

/// Task B.2: the assistant asks a bot only when the person's last message names it (any case, with
/// an article, with a simple ending) or the person confirmed it.
#[test]
fn the_assistant_asks_a_bot_only_when_the_person_names_it() {
    for (text, name) in [
        ("Ask the Travel Planner about Lisbon", "Travel planner"),
        ("ask the travel planner", "Travel planner"),
        ("Frag den Reiseplaner nach Lissabon.", "Reiseplaner"),
        ("Was meint der Reiseplaner?", "Der Reiseplaner"),
        ("Die Idee des Reiseplaners", "Reiseplaner"),
        ("What do the travel planners say?", "Travel planner"),
        ("@reiseplaner: Lissabon?", "Reiseplaner"),
    ] {
        assert!(names_bot(text, name), "{text:?} names {name:?}");
        assert!(assistant_may_ask(Some(text), name, false), "{text:?}");
    }
    for (text, name) in [
        ("Plan a trip to Lisbon", "Travel planner"),
        ("I need a travel plan", "Travel planner"),
        ("the planner travel", "Travel planner"),
        ("Die Reiseplanerin", "Reiseplaner"),
        ("the", "The"),
        ("Lisbon", "  "),
    ] {
        assert!(!names_bot(text, name), "{text:?} does not name {name:?}");
        assert!(!assistant_may_ask(Some(text), name, false), "{text:?}");
    }
    assert!(!assistant_may_ask(None, "Travel planner", false), "no message, no ask");
    assert!(assistant_may_ask(None, "Travel planner", true), "a confirmation is enough");
    assert!(assistant_may_ask(Some("Plan Lisbon"), "Travel planner", true));
}

/// The answer the test engine gives to a bot comes as to the assistant; the conversation keeps the
/// bot. Waits at most two seconds.
#[tokio::test]
async fn asking_a_bot_starts_its_conversation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (assistant, clients) = set_up(dir.path());
    let ai = crate::assistant::tests::no_ai(dir.path());
    let bot = saved(answer(&assistant, &clients, save(None, "Travel planner")).await);
    let ask = Request::AskBot { bot: bot.id.clone(), text: "Plan Lisbon".into(), conversation: None };
    let Response::Conversation { conversation, entries } = crate::assistant::answer(&assistant, &ai, ask).await else {
        panic!("a conversation")
    };
    assert_eq!(conversation.bot.as_deref(), Some(bot.id.as_str()));
    assert!(matches!(&entries[0].content, EntryContent::Message { engine, .. } if engine == TEST_ENGINE), "its engine");
    // The same conversation with the assistant or another bot is refused; with the bot it goes on.
    let id = conversation.id;
    for _ in 0..200 {
        let read = crate::assistant::answer(&assistant, &ai, Request::GetConversation { id: id.clone(), after: None });
        if matches!(read.await, Response::Conversation { conversation, .. } if !conversation.running) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let to_assistant = Request::SendMessage {
        conversation: Some(id.clone()),
        engine: TEST_ENGINE.into(),
        model: None,
        effort: None,
        text: "Hi".into(),
        without: Vec::new(),
        mode: torro_ipc::assistant::Mode::Default,
        bot: None,
    };
    assert_eq!(code(&crate::assistant::answer(&assistant, &ai, to_assistant).await), "error");
    let again = Request::AskBot { bot: bot.id.clone(), text: "And Porto?".into(), conversation: Some(id.clone()) };
    assert!(matches!(crate::assistant::answer(&assistant, &ai, again).await, Response::Conversation { .. }));
    let unknown = Request::AskBot { bot: "0a0b0c0d".into(), text: "Hi".into(), conversation: None };
    assert_eq!(code(&crate::assistant::answer(&assistant, &ai, unknown).await), "not-found");
}
