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

/// Issue #355: opening a contact starts its maps once per version: maps off says so for every
/// address, a place found asks for its picture, an older answer is dropped, errors say "not
/// available", and the `--open map-…` states set the map without asking.
#[tokio::test]
async fn the_maps_of_an_open_contact_follow_its_version() {
    use torro_ipc::contacts::{ContactLocation, Geo, LocationSource};
    let mut app = loaded(&["--demo"]).await;
    let _ = opened(&mut app, "demo-01").await;
    let detail = app.detail.as_ref().expect("open");
    assert_eq!(detail.maps, [AddressMap::Finding], "one address, asked");
    let key = detail.maps_of.clone().expect("for this version");
    assert!(key.ends_with("|\"demo-0\""), "{key}");

    let _ = app.update(Message::MapSettings(key.clone(), None));
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::Unavailable], "the core did not answer");
    let _ = app.update(Message::MapSettings(key.clone(), Some(backend::Shown::default())));
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::MapsOff]);

    let found = ContactLocation::Found { geo: Geo { lat: 52.5, lon: 13.4 }, source: LocationSource::Lookup };
    let _ = app.update(Message::Located(key.clone(), 0, Ok(found)));
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::Finding], "its picture is asked for");
    let picture = torro_gui::map::demo_map(&Theme::Light);
    let _ = app.update(Message::MapPicture(key.clone(), 0, Some(picture.clone())));
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::Shown(picture)]);
    let _ = app.update(Message::MapPicture("other|\"1\"".into(), 0, None));
    assert!(matches!(app.detail.as_ref().expect("open").maps[0], AddressMap::Shown(_)), "another version's answer");
    let _ = app.update(Message::MapPicture(key.clone(), 0, None));
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::Unavailable]);
    for (answer, map) in [
        (Ok(ContactLocation::NoConsent), AddressMap::LookupOff),
        (Ok(ContactLocation::NotFound), AddressMap::NotFound),
        (Err("busy".to_string()), AddressMap::Unavailable),
    ] {
        let _ = app.update(Message::Located(key.clone(), 0, answer));
        assert_eq!(app.detail.as_ref().expect("open").maps, [map]);
    }
    let _ = app.update(Message::Located(key.clone(), 7, Ok(ContactLocation::NotFound)));

    // The same version asks nothing again; another contact starts afresh.
    let _ = app.update(Message::Refetch);
    assert_eq!(app.detail.as_ref().expect("open").maps_of.as_deref(), Some(key.as_str()));
    let _ = opened(&mut app, "demo-02").await;
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::Finding]);

    for (state, map) in [
        ("map-off", AddressMap::MapsOff),
        ("map-lookup-off", AddressMap::LookupOff),
        ("map-finding", AddressMap::Finding),
        ("map-not-found", AddressMap::NotFound),
        ("map-unavailable", AddressMap::Unavailable),
    ] {
        let mut app = loaded(&["--demo", "--open", state]).await;
        let _ = opened(&mut app, "demo-01").await;
        assert_eq!(app.detail.as_ref().expect("open").maps, [map], "{state}");
    }
    let _ = contact::address_map(&AddressMap::LookupOff, 0, iced::Color::BLACK);
}

/// Review of PR #735: the addresses of a contact are located one after the other: an answer for one
/// asks for the next, the last asks for nothing more.
#[tokio::test]
async fn each_address_is_located_after_the_one_before() {
    use torro_ipc::contacts::ContactLocation;
    let mut app = loaded(&["--demo"]).await;
    let _ = opened(&mut app, "demo-01").await;
    let detail = app.detail.as_mut().expect("open");
    let contact = detail.contact.as_mut().expect("fetched");
    let second = contact.addresses[0].clone();
    contact.addresses.push(second);
    detail.maps_of = None;
    let _ = app.update(Message::Used);
    let detail = app.detail.as_ref().expect("open");
    assert_eq!(detail.maps, [AddressMap::Finding, AddressMap::Finding]);
    let key = detail.maps_of.clone().expect("for this version");
    let first = app.update(Message::Located(key.clone(), 0, Ok(ContactLocation::NotFound)));
    assert!(first.units() > 0, "the second address is asked for");
    assert_eq!(app.detail.as_ref().expect("open").maps[1], AddressMap::Finding);
    let last = app.update(Message::Located(key.clone(), 1, Ok(ContactLocation::NotFound)));
    assert_eq!(last.units(), 0, "nothing after the last");
    assert_eq!(app.detail.as_ref().expect("open").maps, [AddressMap::NotFound, AddressMap::NotFound]);
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

/// Second review of #356: under an open dialog the keys of the cards do nothing, and a merge that runs
/// is not asked about again.
#[tokio::test]
async fn the_keys_of_duplicates_wait_under_a_dialog() {
    let mut app = two_groups().await;
    let _ = app.update(Message::Show(View::Duplicates));
    let _ = app.update(Message::Key(Press::Enter));
    assert_eq!(app.dialog, Some(Dialog::Merge));
    let kept = app.kept_of(&app.duplicates[0].clone()).map(|c| c.id.clone());
    let _ = app.update(Message::Key(Press::Right));
    let _ = app.update(Message::Key(Press::Different));
    assert_eq!(app.kept_of(&app.duplicates[0].clone()).map(|c| c.id.clone()), kept, "the card stays");
    assert!(app.dismissing.is_none(), "nothing marked under the dialog");
    if let Some(merge) = app.merge.as_mut() {
        merge.busy = true;
    }
    let group = app.merge.as_ref().map(|m| m.group.clone()).expect("merge");
    let _ = app.update(Message::AskMerge(group));
    assert!(app.merge.as_ref().is_some_and(|m| m.busy), "a running merge is not asked about again");
}

/// Review of PR #733: the narrow navigation keeps the keys of the cards; "Export…" in "Duplicates" takes
/// the cards whatever the search of another view held.
#[tokio::test]
async fn duplicates_keys_wait_under_the_navigation_and_ignore_the_search() {
    let mut app = two_groups().await;
    let _ = app.update(Message::Show(View::Duplicates));
    app.drawer = true;
    let _ = app.update(Message::Key(Press::Different));
    assert!(app.dismissing.is_none(), "nothing marked under the navigation");
    app.drawer = false;
    let all = app.export_scope().ids;
    assert!(!all.is_empty());
    app.search = "zzz nobody".into();
    assert!(app.can_export());
    assert_eq!(app.export_scope().ids, all, "the search of another view takes nothing away");
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

// -- The map of all contacts (issue #357) -------------------------------------------------------

/// The demo after its first load with the map open and its pins in.
async fn on_map() -> App {
    let mut app = loaded(&["--demo"]).await;
    let _ = app.update(Message::Map(MapMsg::Look(Look::Map)));
    let answer = app.backend.locations(app.locations_query()).await;
    let asked = app.map.asked;
    let _ = app.update(Message::Map(MapMsg::Locations(asked, answer)));
    app
}

#[tokio::test]
async fn the_map_is_offered_only_with_the_lookup_on() {
    let mut app = loaded(&["--demo"]).await;
    assert!(app.map_offered(), "the demo has maps and the lookup on");
    let _ = app.update(Message::Show(View::Birthdays));
    assert!(!app.map_offered(), "not for birthdays");
    let _ = app.update(Message::Show(View::All));
    app.map.shown.contact_lookup = false;
    assert!(!app.map_offered(), "no switch without the lookup");
    app.map.shown.contact_lookup = true;
    app.map.shown.maps = false;
    assert!(!app.map_offered(), "no switch without maps");
}

#[tokio::test]
async fn turning_the_lookup_off_goes_back_to_the_list() {
    let mut app = on_map().await;
    assert!(app.on_map());
    assert_eq!(app.map.pins.len(), 38);
    // The core says the lookup is off: the map goes, nothing more is asked.
    let pending = app.map.lookup.running.clone().expect("looking up the first address");
    let generation = app.map.lookup.generation;
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, pending, Ok(ContactLocation::NoConsent))));
    assert_eq!(app.map.look, Look::List);
    assert!(app.map.lookup.running.is_none() && app.map.lookup.queue.is_empty());
}

#[tokio::test]
async fn only_missing_tiles_are_asked_for() {
    let mut app = on_map().await;
    let size = app.map_size();
    let all = torro_gui::tile_map::visible_tiles(app.map.camera, size);
    assert!(!all.is_empty());
    // Every tile of the view was asked for once; none is missing while they are out.
    assert!(app.map.missing_tiles(size).is_empty());
    let dark = app.theme().extended_palette().is_dark;
    let picture = Handle::from_rgba(1, 1, vec![0; 4]);
    for (key, _) in &all {
        let _ = app.update(Message::Map(MapMsg::Tile(*key, dark, Some(picture.clone()))));
    }
    // A wide view at a low zoom shows the world twice: the same tile is kept once.
    let distinct: HashSet<_> = all.iter().map(|(key, _)| *key).collect();
    assert_eq!(app.map.tiles.len(), distinct.len());
    let _ = app.update(Message::Map(MapMsg::Zoom(1)));
    let missing = app.map.missing_tiles(size);
    assert!(missing.is_empty(), "the new tiles were asked for at once: {missing:?}");
    assert!(app.map.tiles.len() <= crate::map_view::MAX_TILES);
}

#[tokio::test]
async fn a_pin_selects_its_row_and_opens_its_card() {
    let mut app = on_map().await;
    let pin = app.map.pins[0].clone();
    let key = pin_key(&pin.id, pin.address);
    let _ = app.update(Message::Map(MapMsg::Event(torro_gui::tile_map::MapEvent::Pin(key.clone()))));
    assert_eq!(app.map.selected.as_deref(), Some(key.as_str()));
    assert!(app.in_view().iter().any(|p| pin_key(&p.id, p.address) == key), "its row is in view");
    let contact = app.backend.get_contact(pin.id.clone()).await.expect("contact");
    let _ = app.update(Message::Map(MapMsg::Card(pin.id.clone(), Ok(Box::new(contact)))));
    assert!(app.map.card.is_some());
    let _ = app.update(Message::Escape);
    assert!(app.map.selected.is_none(), "Escape closes the card");
}

#[tokio::test]
async fn the_lookup_pauses_offline_and_stops_when_the_map_is_left() {
    let mut app = on_map().await;
    assert_eq!(app.map.status.pending.len(), 2);
    let first = app.map.lookup.running.clone().expect("the first address is asked");
    let generation = app.map.lookup.generation;
    // Found: a pin more, the next address is asked.
    let geo = torro_ipc::contacts::Geo { lat: 52.5, lon: 13.4 };
    let found = Ok(ContactLocation::Found { geo, source: torro_ipc::contacts::LocationSource::Lookup });
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, first, found)));
    assert_eq!(app.map.pins.len(), 39);
    let second = app.map.lookup.running.clone().expect("the second address is asked");
    // Offline: paused, nothing more asked.
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, second, Err("offline".into()))));
    assert!(app.map.lookup.paused && app.map.lookup.running.is_none());
    assert!(app.map.status_shown());
    // Leaving the map stops it; an answer of the old run is dropped.
    let _ = app.update(Message::Map(MapMsg::Look(Look::List)));
    assert!(!app.map.lookup.paused && app.map.lookup.queue.is_empty());
    let stale = torro_ipc::contacts::AddressRef { id: "x".into(), address: 0 };
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, stale, Ok(ContactLocation::NotFound))));
    assert_eq!(app.map.status.not_found, 0, "an answer of the run before is dropped");
}

#[tokio::test]
async fn another_selection_stops_the_lookup_and_a_pin_comes_once() {
    let mut app = on_map().await;
    let first = app.map.lookup.running.clone().expect("looking up");
    let generation = app.map.lookup.generation;
    // A load read the place the core just kept: the answer adds no second pin.
    let pins = app.map.pins.len();
    let geo = torro_ipc::contacts::Geo { lat: 52.5, lon: 13.4 };
    app.map.pins.push(torro_ipc::contacts::ContactPin {
        id: first.id.clone(),
        name: "X".into(),
        address: first.address,
        geo,
        source: torro_ipc::contacts::LocationSource::Lookup,
        untrusted: true,
    });
    let found = Ok(ContactLocation::Found { geo, source: torro_ipc::contacts::LocationSource::Lookup });
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, first, found)));
    assert_eq!(app.map.pins.len(), pins + 1, "once");
    // Another book: the lookup of the old selection is dropped.
    let generation = app.map.lookup.generation;
    let _ = app.update(Message::Show(View::Book(torro_contacts::demo::TEAM.into())));
    assert_ne!(app.map.lookup.generation, generation);
    assert!(app.map.lookup.running.is_none() && app.map.lookup.queue.is_empty());
    // `busy` waits a second: no other message asks again before the retry.
    let _ = app.update(Message::Show(View::All));
    let answer = app.backend.locations(app.locations_query()).await;
    let asked = app.map.asked;
    let _ = app.update(Message::Map(MapMsg::Locations(asked, answer)));
    let busy = app.map.lookup.running.clone().expect("the new selection looks up");
    let generation = app.map.lookup.generation;
    let _ = app.update(Message::Map(MapMsg::LookedUp(generation, busy, Err("busy".into()))));
    let _ = app.update(Message::Map(MapMsg::Zoom(1)));
    assert!(app.map.lookup.running.is_none() && app.map.lookup.waiting, "waiting, not asking");
    let _ = app.update(Message::Map(MapMsg::Retry(generation)));
    assert!(app.map.lookup.running.is_some() && !app.map.lookup.waiting);
}
