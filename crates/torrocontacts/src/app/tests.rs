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
    let asked = app.load_asked + 1;
    app.load_asked = asked;
    let _ = app.update(Message::Loaded(asked, result));
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
    assert_eq!(app.shown().len(), 41);
    assert_eq!(app.rows().len(), 41, "one group each in the demo");
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
    let _ = app.update(Message::Loaded(app.load_asked, Err(torro_contacts::ContactsError::Offline)));
    assert_eq!(app.contacts.len(), 41);
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
    assert_eq!(app.shown().len(), 41);
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
    assert!(app.rows().len() < 41);
    let _ = app.update(Message::Fold(Key::Group("Work".into())));
    assert_eq!(app.rows().len(), 41);
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
    let _ = app.update(Message::Saved(
        id.clone(),
        Err(torro_contacts::ContactsError::Conflict { server_vcard: None, etag: None }),
    ));
    let draft = app.detail.as_ref().and_then(|d| d.draft.clone()).expect("kept");
    assert!(!draft.saving);
    assert_eq!(draft.org, "Berg Consulting");
    assert_eq!(draft.error.as_deref(), Some(tr("This contact changed meanwhile. Check and save again.")));
    let fresh = app.backend.get_contact(id.clone()).await;
    let _ = app.update(Message::Refetched(fresh));
    let saved = app.backend.update_contact(id.clone(), draft.etag.clone().unwrap_or_default(), draft.changes()).await;
    assert!(saved.is_ok(), "{saved:?}");
    let _ = app.update(Message::Saved(id.clone(), saved));
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
    let kept = app.backend.resolve_conflict(id.clone(), Keep::Theirs, contact.etag.clone().unwrap_or_default()).await;
    let _ = app.update(Message::Resolved(id, kept));
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

/// A summary of the demo with other groups, for the cases the demo does not have.
fn regroup(app: &mut App, uid: &str, groups: &[&str]) {
    let contact = app.contacts.iter_mut().find(|c| c.id.ends_with(&format!("/{uid}"))).expect("in the demo");
    contact.groups = groups.iter().map(|g| (*g).to_owned()).collect();
}

#[tokio::test]
async fn the_cursor_reaches_every_contact_when_one_stands_under_two_groups() {
    let mut app = loaded(&["--demo"]).await;
    regroup(&mut app, "demo-01", &["Family", "Work"]);
    let rows = app.rows();
    let unique: HashSet<&String> = rows.iter().collect();
    assert_eq!(rows.len(), unique.len(), "each contact once");
    assert_eq!(rows.len(), 41);
    for _ in 0..rows.len() {
        let _ = app.update(Message::Key(Press::Down));
    }
    assert_eq!(app.cursor.as_ref(), rows.last(), "↓ walks to the end");
}

#[tokio::test]
async fn an_answer_about_another_contact_leaves_the_panel_alone() {
    let mut app = loaded(&["--demo"]).await;
    let anna = opened(&mut app, "demo-01").await;
    let _ = app.update(Message::AddGroup("Work".into()));
    let changed = app
        .backend
        .add_to_group(anna.clone(), app.open_contact().and_then(|c| c.etag.clone()).unwrap_or_default(), "Work".into())
        .await;
    let max = opened(&mut app, "demo-06").await;
    let _ = app.update(Message::Changed(anna, changed));
    assert_eq!(app.open_contact().map(|c| c.id.clone()), Some(max), "Max stays open, not Anna");
}

#[tokio::test]
async fn nothing_kept_after_a_conflict_closes_the_panel() {
    let mut app = loaded(&["--demo"]).await;
    let id = opened(&mut app, "demo-04").await;
    let _ = app.update(Message::Resolved(id, Ok(Vec::new())));
    assert!(app.detail.is_none());
}

#[tokio::test]
async fn a_contact_in_conflict_is_not_edited_by_key() {
    let mut app = loaded(&["--demo"]).await;
    let _ = opened(&mut app, "demo-04").await;
    let _ = app.update(Message::Key(Press::Edit));
    assert!(!app.editing());
}

#[tokio::test]
async fn a_new_contact_with_only_an_address_is_created() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::NewContact(None, None));
    let _ = app.update(Message::Draft(edit::Msg::Add(List::Address)));
    let _ = app.update(Message::Draft(edit::Msg::Text(edit::Field::Town(0), "Kiel".into())));
    let _ = app.update(Message::Done);
    assert!(app.detail.as_ref().and_then(|d| d.draft.as_ref()).is_some_and(|d| d.saving), "sent, not dropped");
}

#[tokio::test]
async fn new_contact_waits_while_another_is_edited() {
    let mut app = loaded(&["--demo"]).await;
    let id = opened(&mut app, "demo-01").await;
    let _ = app.update(Message::Edit);
    let _ = app.update(Message::NewContact(None, None));
    assert!(app.is_open(&id) && app.editing(), "the form with its input stays");
}

#[tokio::test]
async fn folded_months_stay_apart_from_the_groups() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Fold(Key::Month(2026, 11)));
    assert!(app.folded(&Key::Month(2026, 11)));
    assert!(app.collapsed.get(&GroupBy::Group).is_none_or(|words| words.is_empty()));
}

#[tokio::test]
async fn an_older_search_answer_does_not_filter_a_new_text() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Search("an".into()));
    let ids = backend::search(app.backend.clone(), "an".into()).await;
    let _ = app.update(Message::Found("an".into(), ids));
    let narrowed = app.shown().len();
    let _ = app.update(Message::Search("a".into()));
    assert!(app.shown().len() >= narrowed, "the names holding \"a\" until its answer comes");
    let _ = app.update(Message::Found("a".into(), Err(torro_contacts::ContactsError::Offline)));
    assert!(app.found.is_none());
}

#[tokio::test]
async fn groups_are_one_without_case_and_new_contacts_go_where_the_group_is() {
    let mut app = loaded(&["--demo"]).await;
    regroup(&mut app, "demo-01", &["work"]);
    let groups = app.groups_of(torro_contacts::demo::CONTACTS);
    assert_eq!(groups.iter().filter(|g| g.eq_ignore_ascii_case("work")).count(), 1, "{groups:?}");
    assert_eq!(app.book_of_group("Work").as_deref(), Some(torro_contacts::demo::CONTACTS));
}

#[tokio::test]
async fn a_search_asked_again_keeps_the_cursor() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Search("an".into()));
    let ids = backend::search(app.backend.clone(), "an".into()).await;
    let _ = app.update(Message::Found("an".into(), ids.clone()));
    let _ = app.update(Message::Key(Press::Down));
    let _ = app.update(Message::Key(Press::Down));
    let cursor = app.cursor.clone();
    assert_ne!(cursor, app.rows().first().cloned());
    let _ = app.update(Message::Found("an".into(), ids));
    assert_eq!(app.cursor, cursor);
}

#[tokio::test]
async fn a_late_created_answer_leaves_another_panel_alone() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::NewContact(None, None));
    let _ = app.update(Message::Draft(edit::Msg::Text(edit::Field::Given, "Max".into())));
    let _ = app.update(Message::Done);
    let fields = app.detail.as_ref().and_then(|d| d.draft.as_ref()).map(|d| d.changes()).expect("form");
    let created = app.backend.create_contact(torro_contacts::demo::CONTACTS.into(), fields).await;
    // The panel's ✕ still closes it while saving.
    let _ = app.update(Message::Close);
    let anna = opened(&mut app, "demo-01").await;
    let _ = app.update(Message::Created(None, created));
    assert!(app.is_open(&anna), "Anna stays open");
}

#[tokio::test]
async fn a_new_picture_while_editing_moves_the_form_to_the_new_version() {
    let mut app = loaded(&["--demo"]).await;
    let id = opened(&mut app, "demo-01").await;
    let _ = app.update(Message::Edit);
    let etag = app.open_contact().and_then(|c| c.etag.clone()).unwrap_or_default();
    let png = torro_contacts::demo::solid_png(64, [10, 20, 30]);
    let changed = app.backend.set_photo(id.clone(), etag, Some(png)).await;
    let new = changed.as_ref().ok().and_then(|c| c.etag.clone());
    let _ = app.update(Message::Changed(id, changed));
    assert_eq!(app.detail.as_ref().and_then(|d| d.draft.as_ref()).and_then(|d| d.etag.clone()), new);
}

#[tokio::test]
async fn an_older_load_never_overwrites_a_newer() {
    let mut app = loaded(&["--demo"]).await;
    let old = app.load_asked;
    let _ = app.load();
    let _ = app.update(Message::Loaded(old, Ok(backend::Loaded::default())));
    assert_eq!(app.contacts.len(), 41, "the older answer is dropped");
}

#[tokio::test]
async fn saving_waits_for_its_answer() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::NewContact(None, None));
    let _ = app.update(Message::Draft(edit::Msg::Text(edit::Field::Given, "Max".into())));
    let _ = app.update(Message::Done);
    let _ = app.update(Message::Cancel);
    assert!(app.editing(), "Cancel waits while saving");
}

#[tokio::test]
async fn a_picture_chosen_for_another_contact_is_not_set() {
    let mut app = loaded(&["--demo"]).await;
    let anna = opened(&mut app, "demo-01").await;
    let _ = opened(&mut app, "demo-06").await;
    let _ = app.update(Message::PhotoChosen(anna, Some(Ok(vec![1, 2, 3]))));
    assert!(app.detail.as_ref().is_some_and(|d| !d.busy), "nothing sent");
    assert!(app.notice.is_some());
}
