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

/// The id of the demo contact `uid`.
fn id_of(app: &App, uid: &str) -> String {
    app.contacts.iter().find(|c| c.id.ends_with(&format!("/{uid}"))).expect("in the demo").id.clone()
}

/// Issue #726: the demo's Anna Berg twice is one group; the view lists its contacts and walks no rows.
#[tokio::test]
async fn the_duplicates_come_with_the_load() {
    let mut app = loaded(&["--demo"]).await;
    assert_eq!(app.duplicates.len(), 1);
    let _ = app.update(Message::Show(View::Duplicates));
    assert_eq!(app.shown().len(), 2);
    assert!(app.rows().is_empty(), "cards, no rows");
    assert_eq!(app.view_name(&View::Duplicates), tr("Duplicates"));
}

/// Issue #726: the first card is kept until another is chosen, and the choice outlasts a load.
#[tokio::test]
async fn keep_follows_the_choice() {
    let mut app = loaded(&["--demo"]).await;
    let group = app.duplicates[0].clone();
    let name = duplicates::key(&group);
    assert_eq!(app.kept_of(&group).map(|c| c.id.clone()), Some(group.contacts[0].id.clone()), "the fullest");
    let other = group.contacts[1].id.clone();
    let _ = app.update(Message::Keep(name.clone(), other.clone()));
    let again = backend::load(app.backend.clone()).await;
    let asked = app.load_asked + 1;
    app.load_asked = asked;
    let _ = app.update(Message::Loaded(asked, again));
    assert_eq!(app.keep.get(&name), Some(&other));
    assert_eq!(app.kept_of(&app.duplicates[0].clone()).map(|c| c.id.clone()), Some(other));
}

/// Issue #726: a contact of a read-only book blocks "Merge…", so its dialog never opens.
#[tokio::test]
async fn merge_is_off_for_a_read_only_contact() {
    let mut app = loaded(&["--demo"]).await;
    app.duplicates[0].contacts[1].readonly = true;
    let name = duplicates::key(&app.duplicates[0]);
    let _ = app.update(Message::AskMerge(name));
    assert_eq!(app.dialog, None);
    assert!(app.merge.is_none());
}

/// Issue #726: "Merge" asks the core with every id and version, closes the dialog and says when some
/// contacts stayed.
#[tokio::test]
async fn merging_closes_the_dialog() {
    let mut app = loaded(&["--demo"]).await;
    let group = app.duplicates[0].clone();
    let _ = app.update(Message::AskMerge(duplicates::key(&group)));
    assert_eq!(app.dialog, Some(Dialog::Merge));
    let _ = app.update(Message::Merge);
    assert!(app.merge.as_ref().is_some_and(|m| m.busy));
    let _ = app.update(Message::CloseDialog);
    assert_eq!(app.dialog, Some(Dialog::Merge), "the dialog waits for its answer");
    let (ids, etags) = duplicates::ids_and_etags(&group);
    let keep = group.contacts[0].id.clone();
    let mut merged = app.backend.merge_contacts(ids, etags, keep).await.expect("merged");
    merged.left = vec![group.contacts[1].id.clone()];
    let _ = app.update(Message::MergedContacts(Ok(merged)));
    assert_eq!(app.dialog, None);
    assert!(app.merge.is_none());
    let notice = app.notice.as_ref().map(|n| n.text.clone()).unwrap_or_default();
    assert_eq!(notice, tr("Merged, but some contacts could not be deleted. They are still there."));
}

/// Issue #726: a failed merge keeps the dialog open with the reason.
#[tokio::test]
async fn a_failed_merge_says_why() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::AskMerge(duplicates::key(&app.duplicates[0])));
    let _ = app.update(Message::Merge);
    let _ = app.update(Message::MergedContacts(Err(ContactsError::Offline)));
    assert_eq!(app.dialog, Some(Dialog::Merge));
    assert!(app.merge.as_ref().is_some_and(|m| !m.busy && m.error.is_some()));
}

/// Issue #725: "Export…" takes what the view shows.
#[tokio::test]
async fn export_takes_what_the_view_shows() {
    let mut app = loaded(&["--demo"]).await;
    assert_eq!(app.export_scope(), Scope::default(), "all contacts: every book not hidden");
    let book = torro_contacts::demo::TEAM.to_owned();
    let _ = app.update(Message::Show(View::Book(book.clone())));
    assert_eq!(app.export_scope(), Scope { book: Some(book), ..Scope::default() });
    let contacts = torro_contacts::demo::CONTACTS.to_owned();
    let _ = app.update(Message::Show(View::Group(contacts.clone(), "Work".into())));
    assert_eq!(app.export_scope(), Scope { book: Some(contacts), group: Some("Work".into()), ..Scope::default() });
    let _ = app.update(Message::Show(View::Favorites));
    let favorites = app.export_scope();
    assert_eq!(favorites.ids.len(), 5);
    assert!(favorites.book.is_none() && favorites.group.is_none());
    let _ = app.update(Message::Show(View::Duplicates));
    assert_eq!(app.export_scope().ids.len(), 2);
    // A search narrows any view to the contacts it lists.
    let _ = app.update(Message::Show(View::All));
    let _ = app.update(Message::Search("Berg".into()));
    let found = backend::search(app.backend.clone(), "Berg".into()).await;
    let _ = app.update(Message::Found("Berg".into(), found));
    let scope = app.export_scope();
    assert!(!scope.ids.is_empty() && scope.ids.len() < app.contacts.len(), "{scope:?}");
    assert!(app.can_export());
}

/// Issue #725: the window writes the cards to the file chosen, and nothing else.
#[tokio::test]
async fn an_export_writes_the_chosen_file() {
    let app = loaded(&["--demo"]).await;
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("Anna Berg.vcf");
    let scope = Scope { ids: vec![id_of(&app, "demo-01")], ..Scope::default() };
    let count = transfer::write_cards(&app.backend, scope, &path).await.expect("written");
    assert_eq!(count, 1);
    let text = std::fs::read_to_string(&path).expect("file");
    assert!(text.contains("BEGIN:VCARD") && text.contains("Anna"), "{text}");
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1, "only the chosen file");
}

/// Issue #725: the trial run counts for the book shown; an answer for the book before is dropped, and
/// "Import" imports for real.
#[tokio::test]
async fn a_trial_run_for_another_book_is_dropped() {
    let mut app = loaded(&["--demo"]).await;
    // Two writable books, so there is something to switch to.
    for book in &mut app.books {
        book.writable = true;
    }
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("kontakte.vcf");
    std::fs::write(&path, "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Kim Neu\r\nEND:VCARD\r\n").expect("file");
    let _ = app.update(Message::ImportChosen(Some(path.clone())));
    assert_eq!(app.dialog, Some(Dialog::Import));
    let first = app.import.as_ref().map(|i| (i.asked, i.book.clone())).expect("dialog");
    let other = app.books.iter().find(|b| b.id != first.1).map(|b| b.id.clone()).expect("a second book");
    let _ = app.update(Message::ImportBook(other.clone()));
    let stale = Imported { created: 7, duplicates: 0, skipped: 0 };
    let _ = app.update(Message::ImportCounted(first.0, Ok(stale)));
    assert_eq!(app.import.as_ref().and_then(|i| i.counts), None, "the answer for the book before");
    let asked = app.import.as_ref().map(|i| i.asked).expect("dialog");
    let counted = transfer::import(app.backend.clone(), first.1.clone(), path.clone(), true).await;
    let _ = app.update(Message::ImportCounted(asked, counted));
    assert_eq!(app.import.as_ref().map(|i| i.new_count()), Some(1));
    let _ = app.update(Message::ImportConfirm);
    assert!(app.import.as_ref().is_some_and(|i| i.busy));
    let done = transfer::import(app.backend.clone(), first.1, path, false).await;
    let _ = app.update(Message::Imported(done));
    assert_eq!(app.dialog, None);
    let notice = app.notice.as_ref().map(|n| n.text.clone()).unwrap_or_default();
    assert_eq!(notice, trf("{n} contacts imported", &[("n", &1)]));
}

/// Issue #725: a file the core cannot take keeps the dialog with the reason, and "Import" off.
#[tokio::test]
async fn a_failed_trial_run_says_why() {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::ImportChosen(Some(PathBuf::from("/nowhere/kontakte.vcf"))));
    let asked = app.import.as_ref().map(|i| i.asked).expect("dialog");
    let _ = app.update(Message::ImportCounted(asked, Err(ContactsError::TooLarge)));
    let import = app.import.as_ref().expect("dialog");
    assert!(import.error.is_some());
    assert_eq!(import.new_count(), 0);
    let _ = app.update(Message::ImportConfirm);
    assert!(!app.import.as_ref().is_some_and(|i| i.busy), "nothing to import");
}

/// Issue #356: `--contact` opens the contact after the first load; an unknown one only the list.
#[tokio::test]
async fn a_contact_named_at_the_start_opens() {
    let plain = loaded(&["--demo"]).await;
    let anna = id_of(&plain, "demo-01");
    let app = loaded(&["--demo", &format!("--contact={anna}")]).await;
    assert!(app.is_open(&anna));
    let unknown = loaded(&["--demo", "--contact=nobody"]).await;
    assert!(unknown.detail.is_none());
}

/// The states `--open duplicates`, `import` and `merge` of the pictures.
#[tokio::test]
async fn the_states_of_import_and_duplicates() {
    let dups = loaded(&["--demo", "--open", "duplicates"]).await;
    assert_eq!(dups.view, View::Duplicates);
    let import = loaded(&["--demo", "--open", "import"]).await;
    assert_eq!(import.dialog, Some(Dialog::Import));
    assert_eq!(import.import.as_ref().map(|i| i.new_count()), Some(12));
    let merge = loaded(&["--demo", "--open", "merge"]).await;
    assert_eq!((merge.view.clone(), merge.dialog), (View::Duplicates, Some(Dialog::Merge)));
}

/// Two groups of duplicates: the demo's Anna Berg and a second pair made here.
async fn two_groups() -> App {
    let mut app = loaded(&["--demo"]).await;
    let mut second = app.duplicates[0].clone();
    for (n, contact) in second.contacts.iter_mut().enumerate() {
        contact.id = format!("other-{n}");
    }
    app.duplicates.push(second);
    app
}

/// Review of #726: the keys of "Duplicates" walk the groups, choose the card that stays, merge and mark.
#[tokio::test]
async fn the_keys_of_duplicates() {
    let mut app = two_groups().await;
    let _ = app.update(Message::Show(View::Duplicates));
    let keys: Vec<String> = app.duplicates.iter().map(duplicates::key).collect();
    let _ = app.update(Message::Key(Press::Down));
    assert_eq!(app.group_cursor.as_ref(), Some(&keys[0]));
    let _ = app.update(Message::Key(Press::Down));
    assert_eq!(app.group_cursor.as_ref(), Some(&keys[1]));
    let _ = app.update(Message::Key(Press::Down));
    assert_eq!(app.group_cursor.as_ref(), Some(&keys[1]), "stays at the last");
    let _ = app.update(Message::Key(Press::Up));
    assert_eq!(app.group_cursor.as_ref(), Some(&keys[0]));
    // → and ← choose the card; Space goes round.
    let group = app.duplicates[0].clone();
    let kept = |app: &App| app.kept_of(&app.duplicates[0].clone()).map(|c| c.id.clone());
    let _ = app.update(Message::Key(Press::Right));
    assert_eq!(kept(&app), Some(group.contacts[1].id.clone()));
    let _ = app.update(Message::Key(Press::Right));
    assert_eq!(kept(&app), Some(group.contacts[1].id.clone()), "stays at the last card");
    let _ = app.update(Message::Key(Press::Left));
    assert_eq!(kept(&app), Some(group.contacts[0].id.clone()));
    let _ = app.update(Message::Key(Press::Space));
    let _ = app.update(Message::Key(Press::Space));
    assert_eq!(kept(&app), Some(group.contacts[0].id.clone()), "after the last the first again");
    // Enter is "Merge…" of the group under the cursor.
    let _ = app.update(Message::Key(Press::Enter));
    assert_eq!(app.dialog, Some(Dialog::Merge));
    assert_eq!(app.merge.as_ref().map(|m| m.group.clone()), Some(keys[0].clone()));
    let _ = app.update(Message::CloseDialog);
    // D marks the group as different people.
    let _ = app.update(Message::Key(Press::Down));
    let _ = app.update(Message::Key(Press::Different));
    assert_eq!(app.dismissing.as_ref(), Some(&keys[1]));
    // Elsewhere the new keys do nothing.
    let mut list = loaded(&["--demo"]).await;
    let _ = list.update(Message::Key(Press::Different));
    assert!(list.dismissing.is_none() && list.group_cursor.is_none());
}

/// Review of #726: while "Mark as different" runs, no second request goes out and its "Merge…" waits.
#[tokio::test]
async fn marking_as_different_waits_for_its_answer() {
    let mut app = two_groups().await;
    let keys: Vec<String> = app.duplicates.iter().map(duplicates::key).collect();
    let _ = app.update(Message::NotDuplicates(keys[0].clone()));
    assert_eq!(app.dismissing.as_ref(), Some(&keys[0]));
    let _ = app.update(Message::NotDuplicates(keys[1].clone()));
    assert_eq!(app.dismissing.as_ref(), Some(&keys[0]), "no second request");
    let _ = app.update(Message::AskMerge(keys[0].clone()));
    assert_eq!(app.dialog, None, "its merge waits");
    let _ = app.update(Message::Dismissed(Ok(())));
    assert!(app.dismissing.is_none());
    assert_eq!(app.notice.as_ref().map(|n| n.text.clone()), Some(tr("Marked as different people").to_owned()));
    let _ = app.update(Message::NotDuplicates(keys[1].clone()));
    assert_eq!(app.dismissing.as_ref(), Some(&keys[1]));
    let _ = app.update(Message::Dismissed(Err(ContactsError::Offline)));
    assert!(app.dismissing.is_none(), "a failure frees the buttons too");
}

/// Review of #725: with hidden books shown, "All contacts" exports the contacts shown by id, since the
/// core leaves hidden books out without a scope.
#[tokio::test]
async fn all_contacts_with_hidden_books_go_by_id() {
    let mut app = loaded(&["--demo"]).await;
    assert_eq!(app.export_scope(), Scope::default());
    app.books[1].hidden_by_default = true;
    let scope = app.export_scope();
    assert!(scope.book.is_none() && scope.group.is_none());
    assert_eq!(scope.ids.len(), app.contacts.len());
}

/// Review of #356: `--contact` waits for a load that came through; a failed first one does not use it up.
#[tokio::test]
async fn a_contact_named_at_the_start_outlasts_a_failed_load() {
    let plain = loaded(&["--demo"]).await;
    let anna = id_of(&plain, "demo-01");
    let mut app = started(&["--demo", &format!("--contact={anna}")]);
    app.load_asked += 1;
    let _ = app.update(Message::Loaded(app.load_asked, Err(ContactsError::Offline)));
    assert!(app.error.is_some() && app.detail.is_none());
    let result = backend::load(app.backend.clone()).await;
    app.load_asked += 1;
    let _ = app.update(Message::Loaded(app.load_asked, result));
    assert!(app.error.is_none());
    assert!(app.is_open(&anna));
}

/// The states of the two "…" menus for the pictures.
#[tokio::test]
async fn the_states_of_the_menus() {
    let header = loaded(&["--demo", "--open", "header-menu"]).await;
    assert_eq!(header.menu, Some(Menu::Header));
    let contact = loaded(&["--demo", "--open", "contact-menu"]).await;
    assert_eq!(contact.menu, Some(Menu::More));
    assert!(contact.detail.is_some());
}
