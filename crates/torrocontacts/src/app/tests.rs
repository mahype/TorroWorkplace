//! Tests of the window's state: loading, views, search, the panel and editing on the demo.

use super::*;

pub(super) fn started(words: &[&str]) -> App {
    let args: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
    let launch = Launch::parse(&args).expect("valid");
    let backend = Backend::Demo(std::sync::Arc::new(torro_contacts::demo::DemoSource::new(today())));
    let (mut app, _) = App::new(backend, Source::from_args(["--demo".to_owned()]), launch);
    app.today = today();
    app
}

pub(super) fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 28).expect("date")
}

/// The app after its first load, as `--open` and the demo leave it.
async fn loaded(words: &[&str]) -> App {
    let mut app = started(words);
    let result = backend::load(app.backend.clone()).await;
    let _ = app.update(Message::Loaded(result));
    app
}

/// Opens `uid` of the demo and hands the app the fetched contact.
async fn opened(app: &mut App, uid: &str) -> String {
    let id = app.contacts.iter().find(|c| c.id.ends_with(&format!("/{uid}"))).expect("in the demo").id.clone();
    let _ = app.update(Message::Open(id.clone()));
    let contact = app.backend.get_contact(id.clone()).await;
    let _ = app.update(Message::Fetched(id.clone(), contact));
    id
}

#[tokio::test]
async fn the_views_filter_the_list() {
    let mut app = loaded(&["--demo"]).await;
    assert!(app.loaded && app.error.is_none());
    assert_eq!(app.shown().len(), 40);
    assert_eq!(app.rows().len(), 40, "one group each in the demo");
    let _ = app.update(Message::Show(View::Favorites));
    assert_eq!(app.shown().len(), 5);
    let _ = app.update(Message::Show(View::Book(torro_contacts::demo::TEAM.into())));
    assert_eq!(app.shown().len(), 8);
    assert!(app.shown().iter().all(|c| c.readonly));
    let _ = app.update(Message::Show(View::Group(torro_contacts::demo::CONTACTS.into(), "Work".into())));
    assert!(!app.shown().is_empty() && app.shown().iter().all(|c| c.groups.contains(&"Work".to_owned())));
    let _ = app.update(Message::Show(View::Birthdays));
    assert!(!app.shown_birthdays().is_empty());
    assert!(app.soon() >= 5, "the demo has birthdays in the next two weeks");
}

#[tokio::test]
async fn the_states_after_loading() {
    let empty = loaded(&["--demo", "--open", "empty"]).await;
    assert!(empty.contacts.is_empty() && !empty.books.is_empty());
    let none = loaded(&["--demo", "--open", "no-account"]).await;
    assert!(none.books.is_empty());
    let failed = loaded(&["--demo", "--open", "error"]).await;
    assert!(failed.error.is_some() && failed.loaded);
    let mut retry = failed;
    let _ = retry.update(Message::Retry);
    assert!(retry.error.is_none() && !retry.loaded, "loading again");
}

#[tokio::test]
async fn a_later_failure_keeps_the_list() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Loaded(Err(torro_contacts::ContactsError::Offline)));
    assert_eq!(app.contacts.len(), 40);
    assert!(app.error.is_none());
    assert!(app.notice.is_some());
}

#[tokio::test]
async fn the_search_takes_the_answer_to_what_is_typed() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Search("berg".into()));
    assert!(app.shown().iter().all(|c| c.display_name.to_lowercase().contains("berg")), "names while asking");
    let ids = backend::search(app.backend.clone(), "berg".into()).await;
    let _ = app.update(Message::Found("old".into(), Ok(Vec::new())));
    assert!(app.found.is_none(), "an answer to something else is dropped");
    let _ = app.update(Message::Found("berg".into(), ids));
    let shown: Vec<String> = app.shown().iter().map(|c| c.display_name.clone()).collect();
    assert!(shown.contains(&"Anna Berg".to_owned()), "{shown:?}");
    assert_eq!(app.cursor.as_deref(), app.rows().first().map(String::as_str));
    let _ = app.update(Message::Search(String::new()));
    assert_eq!(app.shown().len(), 40);
    // A pause that ended after another key asks nothing.
    let stale = app.search_asked;
    let _ = app.update(Message::Search("x".into()));
    let _ = app.update(Message::SearchDue(stale));
    assert!(app.found.is_none());
}

#[tokio::test]
async fn heads_fold_and_the_grouping_changes() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Fold(Key::Group("Work".into())));
    assert!(app.folded(&Key::Group("Work".into())));
    assert!(app.rows().len() < 40);
    let _ = app.update(Message::Fold(Key::Group("Work".into())));
    assert_eq!(app.rows().len(), 40);
    let _ = app.update(Message::GroupBy(GroupBy::City));
    assert_eq!(app.group_by, GroupBy::City);
    assert!(!app.folded(&Key::Group("Work".into())), "folds belong to their grouping");
}

#[tokio::test]
async fn keys_walk_the_list_and_open() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Key(Press::Down));
    let first = app.rows().first().cloned();
    assert_eq!(app.cursor, first);
    let _ = app.update(Message::Key(Press::Down));
    assert_eq!(app.cursor, app.rows().get(1).cloned());
    let _ = app.update(Message::Key(Press::Up));
    assert_eq!(app.cursor, first);
    let _ = app.update(Message::Key(Press::Enter));
    assert_eq!(app.detail.as_ref().and_then(|d| d.id.clone()), first);
    let _ = app.update(Message::Escape);
    assert!(app.detail.is_none());
}

#[tokio::test]
async fn done_sends_only_what_changed_and_keeps_the_form_on_a_conflict() {
    let mut app = loaded(&["--demo"]).await;
    let id = opened(&mut app, "demo-01").await;
    let _ = app.update(Message::Edit);
    let draft = app.detail.as_ref().and_then(|d| d.draft.clone()).expect("editing");
    assert!(draft.changes().is_empty());
    // Nothing changed: the form closes without a request.
    let _ = app.update(Message::Done);
    assert!(!app.editing());
    let _ = app.update(Message::Edit);
    let _ = app.update(Message::Draft(edit::Msg::Text(edit::Field::Org, "Berg Consulting".into())));
    let _ = app.update(Message::Done);
    let draft = app.detail.as_ref().and_then(|d| d.draft.clone()).expect("still open while saving");
    assert!(draft.saving);
    assert_eq!(draft.changes().names(), ["org"]);
    // The server says the contact changed meanwhile: the form stays with the person's input.
    let _ = app.update(Message::Saved(Err(torro_contacts::ContactsError::Conflict { server_vcard: None, etag: None })));
    let draft = app.detail.as_ref().and_then(|d| d.draft.clone()).expect("kept");
    assert!(!draft.saving);
    assert_eq!(draft.org, "Berg Consulting");
    assert_eq!(draft.error.as_deref(), Some(tr("This contact changed meanwhile. Check and save again.")));
    let fresh = app.backend.get_contact(id.clone()).await;
    let _ = app.update(Message::Refetched(fresh));
    let saved = app.backend.update_contact(id.clone(), draft.etag.clone().unwrap_or_default(), draft.changes()).await;
    assert!(saved.is_ok(), "{saved:?}");
    let _ = app.update(Message::Saved(saved));
    assert!(!app.editing());
    assert_eq!(app.open_contact().and_then(|c| c.org.as_deref()), Some("Berg Consulting"));
}

#[tokio::test]
async fn a_read_only_contact_is_not_edited() {
    let mut app = loaded(&["--demo"]).await;
    let _ = opened(&mut app, "demo-05").await;
    assert!(app.open_contact().is_some_and(|c| c.readonly));
    let _ = app.update(Message::Edit);
    assert!(!app.editing());
}

#[tokio::test]
async fn a_conflict_is_resolved_with_the_choice() {
    let mut app = loaded(&["--demo"]).await;
    let id = opened(&mut app, "demo-04").await;
    let contact = app.open_contact().cloned().expect("fetched");
    assert_eq!(contact.state, torro_contacts::ContactState::Conflict);
    let _ = app.update(Message::Resolve(Keep::Theirs));
    assert!(app.detail.as_ref().is_some_and(|d| d.busy), "the buttons wait");
    let kept = app.backend.resolve_conflict(id, Keep::Theirs, contact.etag.clone().unwrap_or_default()).await;
    let _ = app.update(Message::Resolved(kept));
    let now = app.open_contact().expect("still open");
    assert_eq!(now.state, torro_contacts::ContactState::Synced);
    assert!(!app.detail.as_ref().is_some_and(|d| d.busy));
}

#[tokio::test]
async fn a_new_contact_goes_into_the_book_and_group_of_its_head() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::NewContact(None, Some("Work".into())));
    let detail = app.detail.clone().expect("the form");
    assert_eq!(detail.new_in, Some((torro_contacts::demo::CONTACTS.to_owned(), Some("Work".to_owned()))));
    // The read-only book is never chosen.
    let _ = app.update(Message::Show(View::Book(torro_contacts::demo::TEAM.into())));
    assert_eq!(app.new_book().as_deref(), Some(torro_contacts::demo::CONTACTS));
    // Nothing typed: "Done" closes the form.
    let _ = app.update(Message::Done);
    assert!(app.detail.is_none());
}

#[test]
fn copy_takes_name_mail_and_number() {
    let contact = torro_contacts::demo::sample(today()).1[0].summary();
    let text = copy_text(&contact);
    assert_eq!(text.lines().count(), 3);
    assert!(text.starts_with("Anna Berg\n"));
}

#[test]
fn a_large_picture_is_scaled_down_to_jpeg() {
    let png = torro_contacts::demo::solid_png(900, [200, 20, 20]);
    let jpeg = prepare_photo(&png).expect("a picture");
    assert_eq!(&jpeg[..2], [0xFF, 0xD8], "JPEG");
    let back = image::load_from_memory(&jpeg).expect("readable");
    assert!(back.width() <= PHOTO_EDGE && back.height() <= PHOTO_EDGE);
    assert!(prepare_photo(b"not a picture").is_err());
}

#[tokio::test]
async fn open_states_apply_once_after_loading() {
    let app = loaded(&["--demo", "--open", "delete"]).await;
    assert_eq!(app.dialog, Some(Dialog::Delete));
    assert!(app.detail.is_some());
    let app = loaded(&["--demo", "--open", "group-menu"]).await;
    assert_eq!(app.menu, Some(Menu::GroupBy));
    let app = loaded(&["--demo", "--open", "birthdays"]).await;
    assert_eq!(app.view, View::Birthdays);
    let app = loaded(&["--demo", "--open", "by-city"]).await;
    assert_eq!(app.group_by, GroupBy::City);
}
