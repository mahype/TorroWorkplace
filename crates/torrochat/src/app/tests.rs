//! The update logic of both windows against the demo: grouping and folding, the cursor, opening a
//! chat, the own status, sending and reading. Without a window: tasks are dropped, the answers they
//! would bring are fed in by hand from the demo, as the core would answer.

use iced::futures::executor::block_on;
use torro_chat::demo::{ACCOUNT, BEN, TEAM};
use torro_chat::{ChatError, Status, person_id};
use torro_core::i18n::tr;
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
    assert_eq!(rows[0], Target::Person(person_id(ACCOUNT, "ben")), "online first, by name");
    let _ = app.update(Message::Move(1));
    assert_eq!(app.cursor, Some(rows[0].clone()), "the first move goes to the top");
    let _ = app.update(Message::Move(1));
    let _ = app.update(Message::Move(-5));
    assert_eq!(app.cursor, Some(rows[0].clone()), "never past the ends");
    let _ = app.update(Message::Fold(Group::Online));
    assert!(app.folded(Group::Online));
    assert_eq!(app.rows().len(), 4, "Ben and Gesa are folded away");
    let _ = app.update(Message::Search("gesa".into()));
    assert_eq!(app.rows(), [Target::Person(person_id(ACCOUNT, "gesa"))], "a search shows folded matches too");
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
