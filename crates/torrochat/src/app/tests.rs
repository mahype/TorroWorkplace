//! The update logic of both windows against the demo: grouping and folding, the cursor, opening a
//! chat, the own status, sending and reading. Without a window: tasks are dropped, the answers they
//! would bring are fed in by hand from the demo, as the core would answer.

use iced::futures::executor::block_on;
use torro_chat::demo::{ACCOUNT, BEN, TEAM};
use torro_chat::{ChatError, NotifyLevel, Status, person_id};
use torro_core::i18n::{tr, trf};
use torro_gui::source::Source;

use super::chat::Msg;
use super::*;
use crate::launch::Launch;
use crate::timeline;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_string()).collect()
}

/// A window on a demo of its own, as started with `words` (always with `--demo`), and its backend.
fn start(words: &[&str]) -> App {
    let mut words = words.to_vec();
    words.insert(0, "--demo");
    let launch = Launch::parse(&args(&words)).expect("valid parameters");
    let suite = Source::from_args(args(&["--demo"]));
    let (app, _) = App::new(Backend::demo(), suite, launch);
    app
}

/// Feeds the window what the core (here: its demo) answers to its first load.
fn loaded(mut app: App) -> App {
    let backend = app.backend.clone();
    let people = block_on(backend.clone().people());
    let conversations = block_on(backend.clone().conversations());
    let _ = app.update(Message::People(people));
    let _ = app.update(Message::Conversations(conversations));
    let _ = app.update(Message::Own(block_on(backend.clone().own_status(ACCOUNT.into()))));
    if let Some(chat) = &app.chat {
        let page = block_on(backend.messages(chat.id.clone(), None, None));
        let _ = app.update(Message::Chat(Msg::Page(page)));
    }
    app
}

#[test]
fn the_contact_list_lists_people_by_group_and_the_cursor_walks_them() {
    let mut app = loaded(start(&[]));
    assert_eq!(app.title(), tr("Contacts"));
    let rows = app.rows();
    assert_eq!(rows.len(), 6, "every person of the demo");
    assert_eq!(rows[0], Target::Person(person_id(ACCOUNT, "gesa")), "her chat is a favourite: on top");
    assert_eq!(rows[1], Target::Person(person_id(ACCOUNT, "ben")), "then online");
    let _ = app.update(Message::Move(1));
    assert_eq!(app.cursor, Some(rows[0].clone()), "the first move goes to the top");
    let _ = app.update(Message::Move(1));
    let _ = app.update(Message::Move(-5));
    assert_eq!(app.cursor, Some(rows[0].clone()), "never past the ends");
    let _ = app.update(Message::Fold(Group::Online));
    assert!(app.folded(Group::Online));
    assert_eq!(app.rows().len(), 5, "Ben is folded away, Gesa stands in the favourites");
    let _ = app.update(Message::Search("ben".into()));
    assert_eq!(app.rows(), [Target::Person(person_id(ACCOUNT, "ben"))], "a search shows folded matches too");
}

// -- The search in messages (stage 7) -----------------------------------------------------------

/// What the demo finds for `query`, as the core answers `chat.search_messages`.
fn hits_for(app: &App, query: &str) -> Message {
    Message::Hits(query.into(), block_on(app.backend.clone().search_messages(query.into())))
}

#[test]
fn the_tab_messages_searches_what_the_field_holds() {
    let mut app = loaded(start(&[]));
    let _ = app.update(Message::SearchMessages);
    assert_eq!(app.tab, Tab::Messages, "Ctrl+Shift+F goes to the search in messages");
    let _ = app.update(Message::Search("r".into()));
    assert_eq!(app.found, MessageSearch::default(), "one letter asks nothing");
    let _ = app.update(Message::Search("re".into()));
    let _ = app.update(Message::Search("release".into()));
    assert!(app.found.searching && app.found.query == "release");
    // The answer to the older query comes late and is dropped.
    let late = hits_for(&app, "re");
    let _ = app.update(late);
    assert!(app.found.searching && app.found.hits.is_empty());
    let answer = hits_for(&app, "release");
    let _ = app.update(answer);
    assert!(!app.found.searching);
    let hit = torro_chat::message_id(TEAM, 2);
    assert_eq!(app.rows(), [Target::Hit(hit.clone())]);
    assert_eq!(app.cursor, Some(Target::Hit(hit.clone())), "the first hit is ready for Enter");
    assert_eq!(app.found.hits[0].title, "Dana Erb in Team Alpha", "author and conversation");
    let _ = app.update(Message::Open(Target::Hit(hit)));
    assert_eq!(app.creating, None, "a hit opens its conversation, nothing is created");
}

#[test]
fn a_search_that_failed_says_why_and_the_field_serves_every_tab() {
    let mut app = loaded(start(&["--open", "message-search"]));
    assert_eq!((app.tab, app.found.query.as_str()), (Tab::Messages, "review"));
    let _ = app.update(Message::Hits("review".into(), Err(ChatError::Unsupported("No search here.".into()))));
    assert_eq!(app.found.error.as_deref(), Some("No search here."));
    let _ = app.update(Message::Tab(Tab::Groups));
    assert!(app.rows().is_empty(), "the same field filters the groups: none is called \"review\"");
    let _ = app.update(Message::Search(String::new()));
    assert!(matches!(app.rows().first(), Some(Target::Conversation(_))));
    let _ = app.update(Message::Tab(Tab::Messages));
    assert_eq!(app.found, MessageSearch::default(), "an empty field shows what to type");
}

#[test]
fn offline_folds_by_itself_only_when_it_is_long() {
    let mut app = loaded(start(&[]));
    assert!(!app.folded(Group::Offline), "one person offline");
    let template = app.people.iter().find(|person| person.status == Status::Offline).cloned().expect("Finn");
    for n in 0..people::FOLD_OFFLINE_FROM {
        let mut person = template.clone();
        person.id = person_id(ACCOUNT, &format!("p{n}"));
        app.people.push(person);
    }
    assert!(app.folded(Group::Offline), "more than twenty");
    let _ = app.update(Message::Fold(Group::Offline));
    assert!(!app.folded(Group::Offline), "a click unfolds it");
}

#[test]
fn the_groups_tab_lists_the_group_chats() {
    let mut app = loaded(start(&["--open", "groups"]));
    assert_eq!(app.tab, Tab::Groups);
    assert_eq!(app.rows(), [Target::Conversation(TEAM.into()), Target::Conversation(conversation_id("design4sync"))]);
    let _ = app.update(Message::Tab(Tab::People));
    assert!(matches!(app.rows().first(), Some(Target::Person(_))));
}

fn conversation_id(token: &str) -> String {
    torro_chat::conversation_id(ACCOUNT, token)
}

#[test]
fn opening_a_person_without_a_chat_creates_it_once() {
    let mut app = loaded(start(&[]));
    let cem = Target::Person(person_id(ACCOUNT, "cem"));
    let _ = app.update(Message::Open(cem.clone()));
    assert_eq!(app.creating, Some(person_id(ACCOUNT, "cem")), "chat.create_conversation is on its way");
    let _ = app.update(Message::Open(cem));
    let created = block_on(app.backend.clone().create_conversation(ACCOUNT.into(), person_id(ACCOUNT, "cem")));
    let _ = app.update(Message::Created(created));
    assert_eq!(app.creating, None);
    let cem = app.people.iter().find(|person| person.name == "Cem Demir").expect("Cem");
    assert!(people::direct_chat(&app.conversations, cem).is_some(), "the new chat is in the list at once");
}

#[test]
fn opening_a_person_with_a_chat_opens_it_without_creating() {
    let mut app = loaded(start(&[]));
    let _ = app.update(Message::Open(Target::Person(person_id(ACCOUNT, "ben"))));
    assert_eq!(app.creating, None);
    assert_eq!(app.cursor, Some(Target::Person(person_id(ACCOUNT, "ben"))));
}

#[test]
fn a_failed_open_says_so() {
    let mut app = loaded(start(&[]));
    let _ = app.update(Message::Opened(Err("no".into())));
    assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some(tr("The chat could not be opened.")));
    let _ = app.update(Message::Created(Err(ChatError::Offline)));
    assert_eq!(
        app.notice.as_ref().map(|n| n.text.as_str()),
        Some(tr("The server cannot be reached. Try again when you are online."))
    );
}

#[test]
fn the_own_status_is_set_through_the_menu() {
    let mut app = loaded(start(&[]));
    assert_eq!(app.own.as_ref().map(|own| own.status), Some(Status::Online));
    let _ = app.update(Message::ToggleStatusMenu);
    assert_eq!(app.menu, Some(Menu::Status));
    let _ = app.update(Message::SetStatus(Status::Dnd));
    assert_eq!(app.menu, None, "a choice closes the menu");
    // What `presence.set_own_status` answers.
    let change = StatusChange { status: Some(Status::Dnd), ..StatusChange::default() };
    let answer = block_on(app.backend.clone().set_own_status(ACCOUNT.into(), change));
    let _ = app.update(Message::StatusSet(answer));
    assert_eq!(app.own.as_ref().map(|own| own.status), Some(Status::Dnd));
}

#[test]
fn online_without_the_heartbeat_is_refused_in_words() {
    let mut app = loaded(start(&[]));
    if let Some(own) = &mut app.own {
        own.heartbeat = false;
    }
    let _ = app.update(Message::SetStatus(Status::Online));
    assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some(tr("Turn on “Show others when I am online” first.")));
}

#[test]
fn a_status_message_is_one_short_line() {
    let mut app = loaded(start(&["--open", "status-message"]));
    let _ = app.update(Message::StatusText(format!("two\nlines {}", "x".repeat(200))));
    let Some(Dialog::StatusMessage { text, .. }) = &app.dialog else { panic!("the dialog is open") };
    assert!(!text.contains('\n'));
    assert_eq!(text.chars().count(), torro_ipc::chat::MAX_STATUS_MESSAGE);
    let _ = app.update(Message::StatusClear(Some(ClearAfter::Today)));
    let _ = app.update(Message::StatusClear(Some(ClearAfter::Today)));
    assert!(matches!(&app.dialog, Some(Dialog::StatusMessage { clear: None, .. })), "a second click takes it back");
    let _ = app.update(Message::SaveStatusText);
    assert!(matches!(&app.dialog, Some(Dialog::StatusMessage { saving: true, .. })), "shows that it saves");
}

#[test]
fn the_end_of_today_and_of_the_week_lie_ahead() {
    let now = chrono::Utc::now();
    for clear in ClearAfter::ALL {
        assert!(clear.at(now) > now, "{clear:?}");
    }
    assert!(ClearAfter::Week.at(now) >= ClearAfter::Today.at(now));
}

#[test]
fn new_chat_drops_answers_to_an_older_query() {
    let mut app = loaded(start(&[]));
    let _ = app.update(Message::NewChat);
    let _ = app.update(Message::NewQuery("b".into()));
    assert!(matches!(&app.dialog, Some(Dialog::NewChat { searching: false, .. })), "one letter asks nothing");
    let _ = app.update(Message::NewQuery("ben".into()));
    let found = block_on(app.backend.clone().find_chat_people(ACCOUNT.into(), "be".into()));
    let _ = app.update(Message::Found("be".into(), found));
    assert!(matches!(&app.dialog, Some(Dialog::NewChat { searching: true, found, .. }) if found.is_empty()));
    let found = block_on(app.backend.clone().find_chat_people(ACCOUNT.into(), "ben".into()));
    let _ = app.update(Message::Found("ben".into(), found));
    let Some(Dialog::NewChat { found, .. }) = &app.dialog else { panic!("the dialog is open") };
    assert_eq!(found.iter().map(|person| person.name.as_str()).collect::<Vec<_>>(), ["Ben Koch"]);
}

#[test]
fn escape_closes_the_menu_before_the_dialog() {
    let mut app = loaded(start(&["--open", "new"]));
    let _ = app.update(Message::ToggleStatusMenu);
    let _ = app.update(Message::Escape);
    assert!(app.menu.is_none() && app.dialog.is_some());
    let _ = app.update(Message::Escape);
    assert!(app.dialog.is_none());
}

// -- Find on server and "Add to list" -----------------------------------------------------------

/// What the demo's server finds for `query`, as the core answers `presence.find_people`.
fn server_found(app: &App, query: &str) -> Message {
    Message::ServerFound(query.into(), block_on(app.backend.clone().find_people(ACCOUNT.into(), query.into())))
}

#[test]
fn a_search_without_a_match_finds_on_the_server_and_adds_to_the_list() {
    let mut app = loaded(start(&[]));
    let _ = app.update(Message::Search("h".into()));
    assert!(!app.finds_on_server(), "one letter offers nothing");
    let _ = app.update(Message::Search("hanna".into()));
    assert!(app.rows().is_empty() && app.finds_on_server() && !app.server_asked(), "\"Find on server…\" shows");
    let _ = app.update(Message::SearchSubmit);
    assert!(app.server.searching && app.server_asked(), "Enter without a match asks the server");
    let answer = server_found(&app, "hanna");
    let _ = app.update(answer);
    assert!(!app.server.searching);
    let hanna = app.server.found.first().cloned().expect("Hanna");
    assert_eq!(hanna.name, "Hanna Vogt");
    assert!(!app.on_list(&hanna.id), "only on the server");

    let _ = app.update(Message::AddPerson(hanna.clone()));
    assert_eq!(app.server.adding, std::slice::from_ref(&hanna.id), "the button turns busy");
    let _ = app.update(Message::AddPerson(hanna.clone()));
    assert_eq!(app.server.adding.len(), 1, "one ask per person");
    let added = block_on(app.backend.clone().add_person(ACCOUNT.into(), hanna.id.clone()));
    let _ = app.update(Message::Added(hanna.clone(), added));
    assert!(app.server.adding.is_empty());
    assert_eq!(app.rows(), [Target::Person(hanna.id.clone())], "on the list at once, where the search finds her");
    assert!(app.people.iter().any(|p| p.id == hanna.id && p.added), "marked as added by hand");

    let people = block_on(app.backend.clone().people());
    let _ = app.update(Message::People(people));
    assert!(app.people.iter().any(|p| p.id == hanna.id && p.added), "the core's list has her too");
    let _ = app.update(Message::Remove(hanna.id.clone()));
    assert!(!app.on_list(&hanna.id), "\"Remove from list\" takes her off again");
}

#[test]
fn the_servers_hits_belong_to_what_the_field_holds() {
    let mut app = loaded(start(&["--open", "server-found"]));
    assert_eq!((app.search.as_str(), app.server.query.as_str()), ("an", "an"), "the demo state asks at once");
    assert!(app.server.searching);
    assert_eq!(app.rows().len(), 2, "Dana and Gesa match on the list");
    let _ = app.update(Message::Search("ann".into()));
    assert_eq!(app.server, ServerSearch::default(), "typing drops what was asked for the old text");
    let late = server_found(&app, "an");
    let _ = app.update(late);
    assert!(app.server.found.is_empty(), "an answer to the old text is dropped");

    let _ = app.update(Message::FindOnServer);
    let _ = app.update(Message::ServerFound("ann".into(), Err(ChatError::Offline)));
    assert_eq!(app.server.error.as_deref(), Some(tr("The server cannot be reached. Try again when you are online.")));
    let _ = app.update(Message::FindOnServer);
    assert!(app.server.searching && app.server.error.is_none(), "a failed search can be asked again");
    let answer = server_found(&app, "ann");
    let _ = app.update(answer);
    let names: Vec<&str> = app.server.found.iter().map(|person| person.name.as_str()).collect();
    assert_eq!(names, ["Hanna Vogt"]);
}

#[test]
fn a_person_who_could_not_be_added_stays_off_the_list() {
    let mut app = loaded(start(&["--open", "find-on-server"]));
    assert_eq!(app.search, "hanna");
    let _ = app.update(Message::FindOnServer);
    let answer = server_found(&app, "hanna");
    let _ = app.update(answer);
    let hanna = app.server.found.first().cloned().expect("Hanna");
    let _ = app.update(Message::AddPerson(hanna.clone()));
    let _ = app.update(Message::Added(hanna.clone(), Err(ChatError::Busy)));
    assert!(!app.on_list(&hanna.id) && app.server.adding.is_empty(), "\"Add to list\" is back");
    assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some(tr("The person could not be added to the list.")));
}

// -- A chat window ------------------------------------------------------------------------------

fn chat(app: &App) -> &ChatState {
    app.chat.as_ref().expect("a chat window")
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        let action = iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert(c));
        let _ = app.update(Message::Chat(Msg::Edit(action)));
    }
}

#[test]
fn the_chat_window_shows_the_conversation_and_marks_it_read() {
    let mut app = loaded(start(&["--open", "chat"]));
    assert_eq!(app.title(), "Ben Koch");
    assert_eq!(chat(&app).new_after, Some(3), "two of Ben's five are new");
    assert_eq!(chat(&app).read_up_to, Some(5), "seen at once: chat.mark_read is on its way");
    let _ = app.update(Message::Chat(Msg::MarkUnread));
    assert!(chat(&app).hold_read);
    let _ = app.update(Message::Chat(Msg::MarkedUnread(Ok(()))));
    assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some(tr("Marked as unread")));
    let _ = app.update(Message::Focus(true));
    assert_eq!(chat(&app).read_up_to, None, "not read again until someone writes");
}

#[test]
fn nothing_is_marked_read_while_another_window_has_the_keyboard() {
    let mut app = start(&["--open", "chat"]);
    let _ = app.update(Message::Focus(false));
    let app = loaded(app);
    assert_eq!(chat(&app).read_up_to, None);
}

#[test]
fn sending_goes_through_the_demo_and_the_poll_brings_it() {
    let mut app = loaded(start(&["--open", "chat"]));
    type_text(&mut app, "On it.");
    let _ = app.update(Message::Chat(Msg::Send));
    assert!(chat(&app).sending);
    let sent = block_on(app.backend.clone().send(torro_chat::Outgoing {
        conversation: BEN.into(),
        text: "On it.".into(),
        ..torro_chat::Outgoing::default()
    }));
    let _ = app.update(Message::Chat(Msg::Sent(sent)));
    assert!(!chat(&app).sending);
    assert!(chat(&app).editor.text().trim().is_empty(), "the field is empty again");
    let after = chat(&app).messages.last().map(|message| message.id.clone());
    let page = block_on(app.backend.clone().messages(BEN.into(), None, after));
    let _ = app.update(Message::Chat(Msg::Polled(page)));
    let last = chat(&app).messages.last().expect("a message");
    assert_eq!((last.text.as_str(), last.author.own), ("On it.", true));
}

#[test]
fn a_message_that_could_not_go_stays_in_the_field() {
    let mut app = loaded(start(&["--open", "chat"]));
    type_text(&mut app, "Later");
    let _ = app.update(Message::Chat(Msg::Send));
    let _ = app.update(Message::Chat(Msg::Sent(Err(ChatError::Offline))));
    assert_eq!(chat(&app).editor.text(), "Later");
    assert_eq!(
        chat(&app).send_error.as_deref(),
        Some(tr("The server cannot be reached. Try again when you are online."))
    );
    type_text(&mut app, "!");
    assert_eq!(chat(&app).send_error, None, "typing again clears it");
}

#[test]
fn an_empty_field_sends_nothing() {
    let mut app = loaded(start(&["--open", "chat"]));
    type_text(&mut app, "   ");
    let _ = app.update(Message::Chat(Msg::Send));
    assert!(!chat(&app).sending);
}

#[test]
fn typing_an_at_sign_asks_for_mentions_and_a_pick_writes_the_name() {
    let mut app = loaded(start(&["--open", "chat"]));
    type_text(&mut app, "Ask @ge");
    assert_eq!(chat(&app).asked.as_deref(), Some("ge"));
    let found = block_on(app.backend.clone().suggest_mentions(BEN.into(), "ge".into()));
    let _ = app.update(Message::Chat(Msg::Suggested("ge".into(), found)));
    let gesa = chat(&app).suggestions.first().cloned().expect("Gesa");
    assert_eq!(gesa.name, "Gesa Lange");
    let _ = app.update(Message::Chat(Msg::Pick(gesa.clone())));
    assert_eq!(chat(&app).editor.text(), "Ask @Gesa Lange ");
    assert!(chat(&app).suggestions.is_empty());
    assert_eq!(chat(&app).picked, [gesa]);
    type_text(&mut app, "please");
    assert_eq!(chat(&app).editor.text(), "Ask @Gesa Lange please", "the cursor stands after the name");
    assert_eq!(timeline::outgoing(&chat(&app).editor.text(), &chat(&app).picked), "Ask @\"gesa\" please");
}

#[test]
fn reply_quotes_and_escape_takes_it_back() {
    let mut app = loaded(start(&["--open", "chat"]));
    let _ = app.update(Message::Chat(Msg::Reply(torro_chat::message_id(BEN, 5))));
    assert_eq!(chat(&app).reply.as_ref().map(|quote| quote.author.as_str()), Some("Ben Koch"));
    let _ = app.update(Message::Escape);
    assert!(chat(&app).reply.is_none());
}

#[test]
fn the_demo_state_reply_quotes_and_suggests() {
    let app = loaded(start(&["--open", "reply"]));
    assert!(chat(&app).reply.is_some());
    assert_eq!(chat(&app).asked.as_deref(), Some(""), "right after the @");
}

#[test]
fn a_conversation_that_went_says_so() {
    let mut app = start(&["--open", "chat"]);
    let _ = app.update(Message::Chat(Msg::Page(Err(ChatError::NotFound))));
    assert!(chat(&app).gone);
    let _ = app.update(Message::Chat(Msg::Poll));
    assert!(!chat(&app).polling, "nothing is asked for a conversation that went");
}

// -- Changing messages and the conversation (stage 7) -------------------------------------------

/// The person's own message in "Team Alpha" of the demo, three hours old.
fn own() -> String {
    torro_chat::message_id(TEAM, 4)
}

fn message<'a>(app: &'a App, id: &str) -> &'a torro_chat::Message {
    chat(app).message(id).expect("the message")
}

#[test]
fn edit_takes_the_message_into_the_field_and_escape_gives_the_draft_back() {
    let mut app = loaded(start(&["--open", "group"]));
    type_text(&mut app, "half a thought");
    let _ = app.update(Message::Chat(Msg::StartEdit(own())));
    assert_eq!(chat(&app).editing.as_ref().map(|editing| editing.id.clone()), Some(own()));
    assert_eq!(chat(&app).editor.text(), "I can review tomorrow morning.");
    let _ = app.update(Message::Escape);
    assert!(chat(&app).editing.is_none());
    assert_eq!(chat(&app).editor.text(), "half a thought", "the draft is back");
    let _ = app.update(Message::Chat(Msg::StartEdit(torro_chat::message_id(TEAM, 3))));
    assert!(chat(&app).editing.is_none(), "Cem's message is not the person's");
}

#[test]
fn enter_saves_the_edit_and_the_history_shows_it_edited() {
    let mut app = loaded(start(&["--open", "group"]));
    let _ = app.update(Message::Chat(Msg::StartEdit(own())));
    type_text(&mut app, " At nine.");
    let _ = app.update(Message::Chat(Msg::Send));
    assert!(chat(&app).sending, "chat.edit_message is on its way");
    let text = "I can review tomorrow morning. At nine.".to_owned();
    let result = block_on(app.backend.clone().edit_message(own(), text.clone()));
    let _ = app.update(Message::Chat(Msg::Edited { id: own(), text: text.clone(), mentions: Vec::new(), result }));
    assert!(!chat(&app).sending && chat(&app).editing.is_none());
    assert_eq!((message(&app, &own()).text.as_str(), message(&app, &own()).edited), (text.as_str(), true));
    assert!(chat(&app).editor.text().is_empty(), "the field holds what it held before");
}

#[test]
fn an_unchanged_edit_sends_nothing() {
    let mut app = loaded(start(&["--open", "group"]));
    let _ = app.update(Message::Chat(Msg::StartEdit(own())));
    let _ = app.update(Message::Chat(Msg::Send));
    assert!(!chat(&app).sending && chat(&app).editing.is_none());
}

#[test]
fn a_refused_edit_stays_in_the_field_with_a_calm_line() {
    let mut app = loaded(start(&["--open", "chat"]));
    let yesterday = torro_chat::message_id(BEN, 2);
    let _ = app.update(Message::Chat(Msg::StartEdit(yesterday.clone())));
    type_text(&mut app, "!");
    let _ = app.update(Message::Chat(Msg::Send));
    let result = block_on(app.backend.clone().edit_message(yesterday.clone(), "x".into()));
    let ChatError::Unsupported(why) = result.clone().expect_err("too old") else { panic!("unsupported") };
    let _ = app.update(Message::Chat(Msg::Edited { id: yesterday, text: "x".into(), mentions: Vec::new(), result }));
    assert!(chat(&app).editing.is_some(), "nothing typed is lost");
    assert_eq!(chat(&app).note, Some(super::chat::Note { text: why, waiting: false }));
    let _ = app.update(Message::Chat(Msg::Edited {
        id: own(),
        text: "x".into(),
        mentions: Vec::new(),
        result: Err(ChatError::Pending),
    }));
    let note = chat(&app).note.clone().expect("a note");
    assert_eq!((note.text.as_str(), note.waiting), (tr("The command waits for approval."), true));
    let _ = app.update(Message::Chat(Msg::DismissNote));
    assert!(chat(&app).note.is_none());
}

#[test]
fn delete_asks_in_the_menu_then_leaves_the_message_deleted() {
    let mut app = loaded(start(&["--open", "group"]));
    let _ = app.update(Message::Chat(Msg::MoreMenu(own())));
    assert_eq!(app.menu, Some(Menu::Message { id: own(), at: None, confirm: false }));
    let _ = app.update(Message::Chat(Msg::AskDelete(own())));
    assert_eq!(app.menu, Some(Menu::Message { id: own(), at: None, confirm: true }), "the menu asks");
    let _ = app.update(Message::Chat(Msg::Delete(own())));
    assert_eq!(app.menu, None);
    assert_eq!(chat(&app).changing.get(&own()), Some(&super::chat::Change::Delete));
    let _ = app.update(Message::Chat(Msg::Deleted(own(), Err(ChatError::Offline))));
    assert!(chat(&app).changing.is_empty() && chat(&app).note.is_some());
    assert_eq!(message(&app, &own()).kind, torro_chat::MessageKind::Message, "still there");
    let _ = app.update(Message::Chat(Msg::Delete(own())));
    let result = block_on(app.backend.clone().delete_message(own()));
    let _ = app.update(Message::Chat(Msg::Deleted(own(), result)));
    let gone = message(&app, &own());
    assert_eq!((gone.kind, gone.text.as_str()), (torro_chat::MessageKind::Deleted, ""));
}

#[test]
fn a_reaction_shows_at_once_and_goes_back_when_refused() {
    let mut app = loaded(start(&["--open", "group"]));
    let cem = torro_chat::message_id(TEAM, 3);
    let _ = app.update(Message::Chat(Msg::React(cem.clone(), "🎉".into())));
    assert_eq!(message(&app, &cem).reactions.get("🎉"), Some(&2), "shown before the core answers");
    let _ = app.update(Message::Chat(Msg::React(cem.clone(), "😂".into())));
    assert!(!message(&app, &cem).reactions.contains_key("😂"), "one reaction at a time per message");
    let refused = Err(ChatError::Forbidden("Not allowed.".into()));
    let _ =
        app.update(Message::Chat(Msg::Reacted {
            id: cem.clone(), emoji: "🎉".into(), remove: false, result: refused
        }));
    assert_eq!(message(&app, &cem).reactions.get("🎉"), Some(&1), "back as it was");
    assert_eq!(chat(&app).note.as_ref().map(|note| note.text.as_str()), Some("Not allowed."));
    // A click on the person's own reaction takes it back.
    let _ = app.update(Message::Chat(Msg::React(cem.clone(), "👍".into())));
    assert_eq!(message(&app, &cem).reactions.get("👍"), Some(&1));
    assert!(message(&app, &cem).own_reactions.is_empty());
    let _ =
        app.update(Message::Chat(Msg::Reacted { id: cem.clone(), emoji: "👍".into(), remove: true, result: Ok(()) }));
    assert!(chat(&app).changing.is_empty());
}

#[test]
fn favourite_and_notifications_change_at_once_and_say_so() {
    let mut app = loaded(start(&["--open", "chat"]));
    let ben = |app: &App| app.conversations.iter().find(|c| c.id == BEN).cloned().expect("Ben");
    let _ = app.update(Message::Chat(Msg::HeadMenu));
    let _ = app.update(Message::Chat(Msg::ToggleFavorite));
    assert!(ben(&app).favorite && app.menu.is_none());
    let _ = app.update(Message::Chat(Msg::FavoriteSet(true, Ok(()))));
    assert_eq!(app.notice.as_ref().map(|n| n.text.as_str()), Some(tr("Added to favorites")));
    let _ = app.update(Message::Chat(Msg::ToggleFavorite));
    let _ = app.update(Message::Chat(Msg::FavoriteSet(false, Err(ChatError::Offline))));
    assert!(ben(&app).favorite, "refused: still a favourite");

    let _ = app.update(Message::Chat(Msg::SetLevel(NotifyLevel::Never)));
    assert_eq!(ben(&app).notify, NotifyLevel::Never);
    let done = Msg::LevelSet { previous: NotifyLevel::Default, level: NotifyLevel::Never, result: Ok(()) };
    let _ = app.update(Message::Chat(done));
    let said = trf("Notifications: {level}", &[("level", &tr("Never"))]);
    assert_eq!(app.notice.as_ref().map(|n| n.text.clone()), Some(said));
    let _ = app.update(Message::Chat(Msg::SetLevel(NotifyLevel::Mention)));
    let refused =
        Msg::LevelSet { previous: NotifyLevel::Never, level: NotifyLevel::Mention, result: Err(ChatError::Busy) };
    let _ = app.update(Message::Chat(refused));
    assert_eq!(ben(&app).notify, NotifyLevel::Never, "back as it was");
    assert_eq!(chat(&app).note.as_ref().map(|n| n.text.as_str()), Some(tr("Too many calls. Try again later.")));
}

#[test]
fn the_demo_states_of_stage_seven_open_what_they_name() {
    let app = loaded(start(&["--open", "own-menu"]));
    assert_eq!(app.menu, Some(Menu::Message { id: own(), at: None, confirm: false }));
    let app = loaded(start(&["--open", "delete"]));
    assert_eq!(app.menu, Some(Menu::Message { id: own(), at: None, confirm: true }));
    let app = loaded(start(&["--open", "chat-menu"]));
    assert_eq!(app.menu, Some(Menu::Head));
    let app = loaded(start(&["--open", "edit"]));
    assert_eq!(chat(&app).editing.as_ref().map(|editing| editing.id.clone()), Some(own()));
    let app = loaded(start(&["--open", "change-error"]));
    assert!(chat(&app).sending, "the edit of yesterday's message is on its way");
}

#[test]
fn polls_do_not_overlap() {
    let mut app = loaded(start(&["--open", "chat"]));
    let _ = app.update(Message::Chat(Msg::Poll));
    assert!(chat(&app).polling);
    let held = chat(&app).messages.len();
    let page = block_on(app.backend.clone().messages(BEN.into(), None, None));
    let _ = app.update(Message::Chat(Msg::Polled(page)));
    assert_eq!(chat(&app).messages.len(), held, "known messages are not added twice");
}

#[test]
fn a_first_page_that_failed_is_asked_again_on_retry() {
    let mut app = start(&["--open", "chat"]);
    let _ = app.update(Message::Chat(Msg::Refresh));
    assert!(chat(&app).polling, "the first page is asked for");
    let _ = app.update(Message::Chat(Msg::Page(Err(ChatError::Offline))));
    assert!(!chat(&app).loaded, "a failed first page is not loaded");
    assert_eq!(chat(&app).error.as_deref(), Some(tr("The server cannot be reached. Try again when you are online.")));
    let _ = app.update(Message::Chat(Msg::Poll));
    assert!(!chat(&app).polling, "no poll without a first page");
    let _ = app.update(Message::Chat(Msg::Retry));
    assert!(chat(&app).polling, "\"Retry\" asks for the first page again");
    assert_eq!(chat(&app).error, None, "the placeholder shows while it is asked for");
    let _ = app.update(Message::Chat(Msg::Page(Err(ChatError::Busy))));
    let _ = app.update(Message::Chat(Msg::Refresh));
    assert!(chat(&app).polling, "the tick asks again too");
    assert!(chat(&app).error.is_some(), "and the error stays while it does");
    let page = block_on(app.backend.clone().messages(BEN.into(), None, None));
    let _ = app.update(Message::Chat(Msg::Page(page)));
    assert!(chat(&app).loaded && !chat(&app).messages.is_empty());
    assert_eq!(chat(&app).error, None, "the history shows");
}

/// Review of PR #644: every [`REFRESH_EVERY`]th poll asks for the newest page itself, so a
/// reaction of someone else to a message the window holds already shows.
#[test]
fn a_refresh_brings_a_reaction_to_a_message_held_already() {
    let mut app = loaded(start(&["--open", "chat"]));
    let held = chat(&app).messages.first().map(|message| message.id.clone()).expect("a message");
    block_on(app.backend.clone().react(held.clone(), "🎉".into(), false)).expect("reacted");
    if let Some(chat) = app.chat.as_mut() {
        chat.polls = crate::app::chat::REFRESH_EVERY - 1;
    }
    let _ = app.update(Message::Chat(Msg::Poll));
    assert_eq!(chat(&app).polls % crate::app::chat::REFRESH_EVERY, 0, "this one is the newest page");
    let page = block_on(app.backend.clone().messages(BEN.into(), None, None));
    let _ = app.update(Message::Chat(Msg::Polled(page)));
    let shown = chat(&app).messages.iter().find(|message| message.id == held).expect("still held");
    assert!(shown.reactions.contains_key("🎉"), "{:?}", shown.reactions);
}

#[test]
fn a_poll_that_came_clears_the_error() {
    let mut app = loaded(start(&["--open", "chat"]));
    if let Some(chat) = app.chat.as_mut() {
        chat.error = Some("An old failure".into());
    }
    let _ = app.update(Message::Chat(Msg::Poll));
    let after = chat(&app).messages.last().map(|message| message.id.clone());
    let page = block_on(app.backend.clone().messages(BEN.into(), None, after));
    let _ = app.update(Message::Chat(Msg::Polled(page)));
    assert_eq!(chat(&app).error, None);
}
