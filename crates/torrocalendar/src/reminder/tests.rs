//! Tests of the reminder window (task ER2.1).

use chrono::TimeZone;

use super::*;

fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 15, hour, minute, second).single().expect("a time")
}

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn event(uid: &str, location: Option<&str>) -> Event {
    Event {
        uid: uid.into(),
        collection: "demo/work".into(),
        href: None,
        etag: None,
        summary: format!("Event {uid}"),
        start: EventTime::DateTime(at(10, 0, 0)),
        end: Some(EventTime::DateTime(at(10, 45, 0))),
        location: location.map(str::to_string),
        description: Some("Agenda\n1. Release\n2. Reminders\n3. Insights\n4. More".into()),
        recurring: false,
        transparency: torro_pim::Transparency::Opaque,
        alarms: Vec::new(),
        recurrence_id: None,
        repeat: None,
        time_zone: None,
        url: None,
        status: None,
        geo: None,
        organizer: None,
        attendees: Vec::new(),
    }
}

fn reminder(uid: &str, join: bool) -> Reminder {
    Reminder {
        id: format!("r-{uid}"),
        event: event(uid, Some("Room 3.12")),
        fires_at: at(9, 55, 0),
        style: AlarmStyle::FullScreen,
        state: ReminderState::Due,
        snoozed_until: None,
        join: join.then(|| Join { service: "Zoom".into(), url: "https://zoom.us/j/1".into() }),
        preview_by: None,
    }
}

/// A window on demo data showing `reminders` at 09:55, its key lock over.
fn app(reminders: Vec<Reminder>) -> App {
    let launch = Launch::parse(&args(&["--demo", "--open", "reminder"])).expect("valid");
    let source = Source::Demo(Arc::new(DemoSource::new(at(9, 0, 0).date_naive())));
    let (mut app, _) = App::new(source, launch);
    app.snapshot = Snapshot { reminders, ..Snapshot::default() };
    app.now = at(9, 55, 28);
    app.opened = Instant::now() - KEY_LOCK - Duration::from_millis(1);
    app
}

#[test]
fn the_countdown_shows_minutes_and_seconds_then_how_late() {
    let start = at(10, 0, 0);
    assert_eq!(countdown(start, at(9, 55, 28)), Countdown::Starts("4:32".into()));
    assert_eq!(countdown(start, at(8, 54, 59)), Countdown::Starts("1:05:01".into()));
    assert_eq!(countdown(start, at(10, 0, 0)), Countdown::Started(0));
    assert_eq!(countdown(start, at(10, 2, 30)), Countdown::Started(2));
}

#[test]
fn the_screen_picks_its_layout_by_size() {
    assert_eq!(Screen::of(Size::new(1280.0, 720.0)), Screen::Small, "a beamer");
    assert_eq!(Screen::of(Size::new(1000.0, 900.0)), Screen::Small);
    assert_eq!(Screen::of(Size::new(1440.0, 900.0)), Screen::Normal);
    assert_eq!(Screen::of(Size::new(1920.0, 1080.0)), Screen::Wide);
    assert_eq!(Screen::of(Size::new(3840.0, 1080.0)), Screen::Wide, "32:9, centred");
}

#[test]
fn the_window_starts_for_reminders_and_its_demo_states() {
    assert!(Launch::wanted(&args(&["--reminders"])));
    assert!(Launch::wanted(&args(&["--demo", "--open", "reminder-many"])));
    assert!(Launch::wanted(&args(&["--demo", "--open=reminder-light"])));
    assert!(!Launch::wanted(&args(&["--demo", "--open", "week"])), "the calendar's own states");
    let light = Launch::parse(&args(&["--demo", "--open", "reminder-light", "--size", "1920x1080"])).expect("valid");
    assert_eq!(
        (light.demo, light.size, light.appearance),
        (Some(Demo::Light), Some(Size::new(1920.0, 1080.0)), Some(Appearance::Light))
    );
    assert!(Launch::parse(&args(&["--open", "reminder"])).is_err(), "only with --demo");
    assert!(Launch::parse(&args(&["--demo", "--open", "reminder-x"])).is_err());
    assert!(Launch::parse(&args(&["--demo", "--size", "big"])).is_err());
    assert!(Launch::parse(&args(&["--reminders", "--size", "1920x1080"])).is_err(), "the real one is full screen");
    assert_eq!(Launch::parse(&args(&["--reminders"])).expect("valid").demo, None);
}

#[test]
fn keys_join_snooze_and_dismiss_after_the_lock() {
    let mut window = app(vec![reminder("call", true)]);
    let key = |window: &App, pressed: Pressed| window.key_message(&pressed).map(|message| format!("{message:?}"));
    assert_eq!(key(&window, Pressed::Enter), Some("Join".into()));
    assert_eq!(key(&window, Pressed::Escape), Some("Dismiss".into()));
    assert_eq!(key(&window, Pressed::Char("s".into())), Some("ToggleSnooze".into()));
    window.snooze_open = true;
    assert_eq!(key(&window, Pressed::Char("5".into())), Some("Snooze(Minutes(5))".into()));
    assert_eq!(key(&window, Pressed::Char("0".into())), Some("Snooze(Minutes(10))".into()));
    assert_eq!(key(&window, Pressed::Char("b".into())), Some("Snooze(UntilStart)".into()));
    assert_eq!(key(&window, Pressed::Escape), Some("ToggleSnooze".into()), "Escape closes the menu first");
    window.now = at(10, 1, 0);
    assert_eq!(key(&window, Pressed::Char("b".into())), None, "the event has started");

    let without_call = app(vec![reminder("room", false)]);
    assert_eq!(key(&without_call, Pressed::Enter), Some("OpenCalendar".into()));

    // Right after it opened, a key typed elsewhere does nothing.
    let mut fresh = app(vec![reminder("call", true)]);
    fresh.opened = Instant::now();
    let _ = fresh.update(Message::Key(Pressed::Escape));
    assert_eq!(fresh.snapshot.reminders.len(), 1, "not dismissed by a stray key");
}

#[test]
fn dismissing_and_snoozing_take_the_reminder_off_and_page_on() {
    let mut window = app(vec![reminder("a", true), reminder("b", false)]);
    let _ = window.update(Message::Next);
    assert_eq!(window.current().map(|r| r.id.as_str()), Some("r-b"));
    let _ = window.update(Message::Next);
    assert_eq!(window.current().map(|r| r.id.as_str()), Some("r-a"), "around the end");
    let _ = window.update(Message::Previous);
    let _ = window.update(Message::Snooze(Snooze::Minutes(5)));
    assert_eq!(window.snapshot.reminders.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r-a"]);
    assert!(!window.snooze_open);
    let _ = window.update(Message::Dismiss);
    assert!(window.snapshot.reminders.is_empty());

    let mut many = app(vec![reminder("a", true), reminder("b", false)]);
    let _ = many.update(Message::DismissAll);
    assert!(many.snapshot.reminders.is_empty());
}

#[test]
fn notes_show_three_lines_until_asked() {
    let notes = "Agenda\n1. Release\n2. Reminders\n3. Insights\n4. More";
    assert_eq!(notes_shown(notes, false), ("Agenda\n1. Release\n2. Reminders…".to_string(), true));
    assert_eq!(notes_shown(notes, true), (notes.to_string(), false));
    assert_eq!(notes_shown("Short", false), ("Short".to_string(), false));
}

#[test]
fn the_place_goes_to_the_map_only_as_a_search() {
    assert_eq!(
        maps_url("Praxis Dr. Keller, Lindenallee 12"),
        "https://www.openstreetmap.org/search?query=Praxis+Dr.+Keller%2C+Lindenallee+12"
    );
    assert!(maps_url("Straße").contains("Stra%C3%9Fe"), "UTF-8 bytes");
}

#[test]
fn the_links_of_an_event_list_the_call_first_once() {
    let mut call = event("call", Some("Room 3.12"));
    call.description = Some("Board: https://example.org/board then https://zoom.us/j/1 and https://zoom.us/j/1".into());
    assert_eq!(event_links(&call), ["https://zoom.us/j/1", "https://example.org/board"]);
}

#[test]
fn every_screen_gets_a_window_with_the_setting_main_first() {
    let launch = |words: &[&str]| Launch::parse(&args(words)).expect("valid");
    let one = displays(&launch(&["--reminders"]), 3);
    assert_eq!(one, [Display { index: 0, monitor: None }], "without the setting only the main screen");
    let all = displays(&launch(&["--reminders", "--all-displays"]), 3);
    assert_eq!(all.iter().map(|d| d.index).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(all[0].is_main() && !all[1].is_main());
    let named = displays(&launch(&["--reminders", "--all-displays", "--monitors", "DP-1, HDMI-A-1"]), 1);
    assert_eq!(named.iter().map(|d| d.monitor.as_deref()).collect::<Vec<_>>(), [Some("DP-1"), Some("HDMI-A-1")]);
    assert_eq!(window_title(&named[1]), "Reminder · HDMI-A-1", "what the window manager's rule matches");
    let main_only = displays(&launch(&["--reminders", "--monitors", "DP-1,HDMI-A-1"]), 1);
    assert_eq!(main_only.len(), 1);
    let second = displays(&launch(&["--demo", "--open", "reminder-second"]), 1);
    assert!(!second[0].is_main(), "the demo of the calm version");
}
