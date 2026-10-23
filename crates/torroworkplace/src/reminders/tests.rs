//! Tests of the reminder rules and commands (task ER1.3), with "now" passed in.

use chrono::TimeZone;

use super::*;

const WORK: &str = "nextcloud:anna/https://cloud.example.org/cal/work/";
const HOME: &str = "nextcloud:anna/https://cloud.example.org/cal/home/";

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 15, hour, minute, 0).single().expect("a time")
}

fn event(uid: &str, collection: &str, start: DateTime<Utc>, minutes: i64) -> Event {
    Event {
        uid: uid.into(),
        collection: collection.into(),
        href: Some(format!("/cal/{uid}.ics")),
        etag: Some("\"1\"".into()),
        summary: format!("Event {uid}"),
        start: EventTime::DateTime(start),
        end: Some(EventTime::DateTime(start + TimeDelta::minutes(minutes))),
        location: None,
        description: None,
        recurring: false,
        transparency: Transparency::Opaque,
        alarms: Vec::new(),
        recurrence_id: None,
        repeat: None,
        time_zone: None,
        url: None,
        status: None,
        geo: None,
        organizer: None,
        attendees: Vec::new(),
        exceptions: Vec::new(),
    }
}

fn before(minutes: i64, style: Option<AlarmStyle>) -> Alarm {
    Alarm { trigger: AlarmTrigger::Before { minutes, of: AlarmAnchor::Start }, style }
}

fn settings() -> Settings {
    Settings::default()
}

fn full_screen(calendar: &str) -> Settings {
    Settings { calendars: BTreeMap::from([(calendar.to_string(), CalendarReminders::FullScreen)]), ..settings() }
}

fn states(reminders: &[Reminder]) -> Vec<(&str, ReminderState)> {
    reminders.iter().map(|r| (r.event.uid.as_str(), r.state)).collect()
}

#[test]
fn an_event_without_reminders_gets_the_default_a_minute_before_as_a_notification() {
    let events = [event("sync", WORK, at(10, 0), 45)];
    let ledger = Ledger::default();
    let upcoming = plan(&events, &settings(), &ledger, at(9, 0));
    assert_eq!(states(&upcoming), [("sync", ReminderState::Upcoming)]);
    assert_eq!(upcoming[0].fires_at, at(9, 59));
    assert_eq!(upcoming[0].style, AlarmStyle::Notification);
    assert_eq!(upcoming[0].id.len(), 32);
    assert!(upcoming[0].id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(states(&plan(&events, &settings(), &ledger, at(9, 59))), [("sync", ReminderState::Due)]);
    let none = Settings { default_lead: None, ..settings() };
    assert!(plan(&events, &none, &ledger, at(9, 59)).is_empty(), "no default reminder");
    let off = Settings { enabled: false, ..settings() };
    assert!(plan(&events, &off, &ledger, at(9, 59)).is_empty(), "reminders switched off");
}

#[test]
fn own_reminders_come_first_and_choose_their_style() {
    let mut sync = event("sync", WORK, at(10, 0), 45);
    sync.alarms = vec![before(5, Some(AlarmStyle::FullScreen)), before(60, None)];
    let reminders = plan(&[sync], &settings(), &Ledger::default(), at(8, 0));
    let fired: Vec<_> = reminders.iter().map(|r| (r.fires_at, r.style)).collect();
    assert_eq!(fired, [(at(9, 0), AlarmStyle::Notification), (at(9, 55), AlarmStyle::FullScreen)], "soonest first");

    let reminders = plan(&[event("x", WORK, at(10, 0), 30)], &full_screen(WORK), &Ledger::default(), at(9, 0));
    assert_eq!(reminders[0].style, AlarmStyle::FullScreen, "the calendar's style");
    let mut end = event("y", HOME, at(10, 0), 60);
    end.alarms = vec![Alarm { trigger: AlarmTrigger::Before { minutes: 10, of: AlarmAnchor::End }, style: None }];
    assert_eq!(plan(&[end], &settings(), &Ledger::default(), at(9, 0))[0].fires_at, at(10, 50));
}

#[test]
fn off_calendars_all_day_and_free_events_stay_quiet_unless_asked() {
    let mut day = event("day", WORK, at(0, 0), 0);
    day.start = EventTime::Date(at(0, 0).date_naive() + chrono::Days::new(1));
    day.end = None;
    let mut free = event("free", WORK, at(10, 0), 60);
    free.transparency = Transparency::Transparent;
    let off = event("off", HOME, at(10, 0), 60);
    let events = [day, free, off];
    let quiet = Settings { calendars: BTreeMap::from([(HOME.into(), CalendarReminders::Off)]), ..settings() };
    assert!(plan(&events, &quiet, &Ledger::default(), at(9, 0)).is_empty());
    let all = Settings { all_day: true, free_events: true, ..quiet };
    let uids: Vec<_> = plan(&events, &all, &Ledger::default(), at(9, 0)).into_iter().map(|r| r.event.uid).collect();
    assert_eq!(uids, ["free", "day"], "an off calendar never reminds");
}

#[test]
fn a_reminder_never_shown_comes_late_only_within_the_grace() {
    let events = [event("sync", WORK, at(10, 0), 60)];
    let ledger = Ledger::default();
    // Woken ten minutes after the start: the meeting still runs.
    assert_eq!(states(&plan(&events, &settings(), &ledger, at(10, 10))), [("sync", ReminderState::Due)]);
    // A core started in the afternoon does not bring back the morning.
    assert!(plan(&events, &settings(), &ledger, at(10, 40)).is_empty());
    // One that was shown stays until the event ends, and goes with it.
    let id = plan(&events, &settings(), &ledger, at(9, 59))[0].id.clone();
    let fired = Shown { at: at(9, 59), ends: at(11, 0) };
    let shown = Ledger { shown: BTreeMap::from([(id, fired)]), ..Ledger::default() };
    assert_eq!(states(&plan(&events, &settings(), &shown, at(10, 40))), [("sync", ReminderState::Due)]);
    assert!(plan(&events, &settings(), &shown, at(11, 0)).is_empty(), "past is past");
}

#[test]
fn full_screen_becomes_a_notification_during_a_call() {
    let mut call = event("call", HOME, at(9, 30), 60);
    call.location = Some("https://us02web.zoom.us/j/123".into());
    let next = event("next", WORK, at(10, 0), 30);
    let calm = plan(std::slice::from_ref(&next), &full_screen(WORK), &Ledger::default(), at(9, 59));
    assert_eq!(calm[0].style, AlarmStyle::FullScreen);
    let during = plan(&[call.clone(), next.clone()], &full_screen(WORK), &Ledger::default(), at(9, 59));
    let next_one = during.iter().find(|r| r.event.uid == "next").expect("listed");
    assert_eq!(next_one.style, AlarmStyle::Notification);
    let anyway = Settings { never_during_call: false, ..full_screen(WORK) };
    let during = plan(&[call.clone(), next], &anyway, &Ledger::default(), at(9, 59));
    assert!(during.iter().any(|r| r.event.uid == "next" && r.style == AlarmStyle::FullScreen));
    assert_eq!(join_of(&call).map(|join| join.service), Some("Zoom".into()));
}

/// Task TE.2: a cancelled event is read now, but it reminds of nothing and holds no call.
#[test]
fn a_cancelled_event_reminds_of_nothing_and_is_no_call() {
    let mut gone = event("gone", WORK, at(10, 0), 60);
    gone.alarms = vec![before(15, Some(AlarmStyle::FullScreen))];
    gone.status = Some(EventStatus::Cancelled);
    let all = Settings { all_day: true, free_events: true, ..full_screen(WORK) };
    assert!(plan(std::slice::from_ref(&gone), &all, &Ledger::default(), at(9, 50)).is_empty());
    let tentative = Event { status: Some(EventStatus::Tentative), ..gone.clone() };
    assert_eq!(states(&plan(&[tentative], &all, &Ledger::default(), at(9, 50))), [("gone", ReminderState::Due)]);

    // A called-off call that would still run does not turn full screen into a notification.
    let mut call = event("call", HOME, at(9, 30), 60);
    call.location = Some("https://us02web.zoom.us/j/123".into());
    call.status = Some(EventStatus::Cancelled);
    let next = event("next", WORK, at(10, 0), 30);
    let reminders = plan(&[call, next], &full_screen(WORK), &Ledger::default(), at(9, 59));
    assert_eq!(states(&reminders), [("next", ReminderState::Due)]);
    assert_eq!(reminders[0].style, AlarmStyle::FullScreen);
}

#[test]
fn a_moved_event_is_a_new_reminder() {
    let reminders = Reminders::new(None);
    let events = [event("sync", WORK, at(10, 0), 45)];
    let (due, _) = reminders.take_due(&events, &settings(), at(9, 59));
    assert_eq!(reminders.dismiss(&due[0].id, &events, &settings(), at(9, 59)), Response::Ok);
    assert!(reminders.take_due(&events, &settings(), at(9, 59)).0.is_empty());
    let moved = [event("sync", WORK, at(11, 0), 45)];
    let (again, _) = reminders.take_due(&moved, &settings(), at(10, 59));
    assert_eq!(again.len(), 1, "a dismissed reminder comes again when its event moves");
    assert_ne!(again[0].id, due[0].id);
}

#[test]
fn a_reminder_fires_once_and_again_after_its_snooze() {
    let reminders = Reminders::new(None);
    let events = [event("sync", WORK, at(10, 0), 45)];
    let (due, paused) = reminders.take_due(&events, &settings(), at(9, 59));
    assert_eq!((states(&due), paused), (vec![("sync", ReminderState::Due)], false));
    assert!(reminders.take_due(&events, &settings(), at(9, 59)).0.is_empty(), "once");
    let id = due[0].id.clone();
    assert_eq!(reminders.snooze(&id, Snooze::Minutes(5), &events, &settings(), at(9, 59)), Response::Ok);
    let Response::Reminders { reminders: listed, .. } = reminders.list(&events, &settings(), at(10, 1), false) else {
        panic!("reminders")
    };
    assert_eq!((listed[0].state, listed[0].snoozed_until), (ReminderState::Snoozed, Some(at(10, 4))));
    assert_eq!(reminders.next_wake(&events, &settings(), at(10, 1)), at(10, 2), "within a minute");
    assert_eq!(reminders.next_wake(&events, &settings(), at(10, 3) + TimeDelta::seconds(30)), at(10, 4));
    assert_eq!(states(&reminders.take_due(&events, &settings(), at(10, 4)).0), [("sync", ReminderState::Due)]);

    // Until it starts.
    let early = [event("late", HOME, at(12, 0), 30)];
    let mut late = early[0].clone();
    late.alarms = vec![before(30, None)];
    let early = [late];
    let (due, _) = reminders.take_due(&early, &settings(), at(11, 30));
    let id = due[0].id.clone();
    assert_eq!(reminders.snooze(&id, Snooze::UntilStart, &early, &settings(), at(11, 31)), Response::Ok);
    assert!(reminders.take_due(&early, &settings(), at(11, 59)).0.is_empty());
    assert_eq!(reminders.take_due(&early, &settings(), at(12, 0)).0.len(), 1, "at the start");
    let code = |answer: Response| match answer {
        Response::Error { code, .. } => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(code(reminders.snooze(&id, Snooze::UntilStart, &early, &settings(), at(12, 1))), "unsupported");
}

#[test]
fn only_fired_reminders_are_snoozed_or_dismissed() {
    let reminders = Reminders::new(None);
    let events = [event("sync", WORK, at(10, 0), 45)];
    let id = plan(&events, &settings(), &Ledger::default(), at(9, 0))[0].id.clone();
    let code = |answer: Response| match answer {
        Response::Error { code, .. } => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(code(reminders.dismiss(&id, &events, &settings(), at(9, 0))), "conflict", "not ahead of time");
    assert_eq!(code(reminders.snooze(&id, Snooze::Minutes(5), &events, &settings(), at(9, 0))), "conflict");
    assert_eq!(code(reminders.dismiss("nope", &events, &settings(), at(9, 0))), "not-found");
    for minutes in [0, MAX_SNOOZE + 1] {
        assert_eq!(code(reminders.snooze(&id, Snooze::Minutes(minutes), &events, &settings(), at(9, 59))), "error");
    }
    assert_eq!(reminders.dismiss(&id, &events, &settings(), at(9, 59)), Response::Ok);
    assert_eq!(reminders.dismiss(&id, &events, &settings(), at(9, 59)), Response::Ok, "again is fine");
    assert_eq!(code(reminders.snooze(&id, Snooze::Minutes(5), &events, &settings(), at(9, 59))), "conflict");
    let Response::Reminders { reminders: due, .. } = reminders.list(&events, &settings(), at(9, 59), true) else {
        panic!("reminders")
    };
    assert!(due.is_empty(), "a dismissed one is not due");
}

#[test]
fn a_pause_holds_reminders_back_for_at_most_a_day() {
    let reminders = Reminders::new(None);
    let events = [event("sync", WORK, at(10, 0), 45)];
    let code = |answer: Response| match answer {
        Response::Error { code, .. } => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(code(reminders.pause(Some(at(9, 0) + TimeDelta::hours(25)), at(9, 0))), "error");
    assert_eq!(reminders.pause(Some(at(10, 30)), at(9, 0)), Response::Ok);
    let (due, paused) = reminders.take_due(&events, &settings(), at(9, 59));
    assert!(paused && due.len() == 1, "it still comes, quietly");
    let Response::Reminders { paused_until, .. } = reminders.list(&events, &settings(), at(9, 59), false) else {
        panic!("reminders")
    };
    assert_eq!(paused_until, Some(at(10, 30)));
    assert_eq!(reminders.pause(None, at(10, 0)), Response::Ok);
    assert_eq!(reminders.ledger(at(10, 0)).paused(at(10, 0)), None, "resumed");
}

#[test]
fn previews_show_once_and_a_client_may_ask_once_a_minute() {
    let reminders = Reminders::new(None);
    let mut call = event("call", HOME, at(9, 30), 60);
    call.location = Some("https://meet.google.com/abc-defg-hij".into());
    let sync = event("sync", WORK, at(16, 0), 45);
    let events = [call, sync.clone()];
    let busy = |answer: Response| matches!(answer, Response::Error { ref code, .. } if code == "busy");
    assert_eq!(reminders.preview(&sync, &full_screen(WORK), &events, "Claude", Some("c1"), at(9, 40)), Response::Ok);
    assert!(busy(reminders.preview(&sync, &full_screen(WORK), &events, "Claude", Some("c1"), at(9, 40))));
    assert_eq!(reminders.preview(&sync, &full_screen(WORK), &events, SUITE, None, at(9, 40)), Response::Ok, "a person");
    let (due, _) = reminders.take_due(&events, &full_screen(WORK), at(9, 40));
    let previews: Vec<_> = due.iter().filter(|r| r.preview_by.is_some()).collect();
    assert_eq!(previews.len(), 2);
    assert_eq!(previews[0].style, AlarmStyle::Notification, "the agent's preview keeps the call in mind");
    assert_eq!(previews[1].style, AlarmStyle::FullScreen);
    assert!(reminders.take_due(&events, &full_screen(WORK), at(9, 40)).0.iter().all(|r| r.preview_by.is_none()));
    assert_eq!(reminders.dismiss(&previews[0].id, &events, &settings(), at(9, 41)), Response::Ok);
    assert_eq!(reminders.preview(&sync, &settings(), &events, "Claude", Some("c1"), at(9, 41)), Response::Ok);
    assert_eq!(reminders.pause(Some(at(11, 0)), at(9, 42)), Response::Ok);
    assert!(busy(reminders.preview(&sync, &settings(), &events, "Claude", Some("c2"), at(9, 43))), "paused");
}

#[test]
fn the_ledger_is_kept_private_and_tidied() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("state").join(FILE);
    let reminders = Reminders::new(Some(file.clone()));
    let events = [event("sync", WORK, at(10, 0), 45)];
    let (due, _) = reminders.take_due(&events, &settings(), at(9, 59));
    assert_eq!(reminders.dismiss(&due[0].id, &events, &settings(), at(9, 59)), Response::Ok);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).expect("written").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let again = Reminders::new(Some(file.clone()));
    assert!(again.ledger(at(10, 0)).dismissed.contains_key(&due[0].id), "read back");
    let week_later = at(10, 0) + TimeDelta::days(8);
    assert_eq!(again.pause(Some(week_later + TimeDelta::hours(1)), week_later), Response::Ok, "any change tidies");
    assert!(again.ledger(at(10, 0)).dismissed.is_empty(), "a week after the event it is gone");
    std::fs::write(&file, "not json").expect("broken");
    assert_eq!(Reminders::new(Some(file)).ledger(at(10, 0)), Ledger::default(), "a broken file starts over");
}

#[test]
fn reminders_follow_their_inbox_entries() {
    use crate::inbox::ReminderAnswer;
    let reminders = Reminders::new(None);
    let events = [event("a", WORK, at(10, 0), 45), event("b", WORK, at(10, 0), 45), event("c", WORK, at(10, 0), 45)];
    let (due, _) = reminders.take_due(&events, &settings(), at(9, 59));
    let id = |uid: &str| due.iter().find(|r| r.event.uid == uid).map(|r| r.id.clone()).expect("fired");
    let standing = reminders.standing(&events, &settings(), at(9, 59));
    assert_eq!(standing.len(), 3);
    assert!(standing.iter().all(|(_, fired)| *fired == at(9, 59)));
    let answers = [
        (id("a"), ReminderAnswer::Done { at: at(10, 0) }),
        (id("b"), ReminderAnswer::Snoozed { until: at(10, 5) }),
        // Done before it fired: an entry the inbox closed itself, from an earlier round.
        (id("c"), ReminderAnswer::Done { at: at(9, 0) }),
    ];
    reminders.follow_inbox(&answers, &events, &settings(), at(10, 0));
    let ledger = reminders.ledger(at(10, 0));
    assert!(ledger.dismissed.contains_key(&id("a")));
    assert_eq!(ledger.snoozed.get(&id("b")).map(|s| s.until), Some(at(10, 5)));
    assert!(ledger.shown.contains_key(&id("c")) && !ledger.dismissed.contains_key(&id("c")));
    // Snoozed past its event: as good as dismissed.
    let late = [(id("c"), ReminderAnswer::Snoozed { until: at(12, 0) })];
    reminders.follow_inbox(&late, &events, &settings(), at(10, 1));
    assert!(reminders.ledger(at(10, 1)).dismissed.contains_key(&id("c")));
}

#[test]
fn the_buttons_of_a_notification_send_their_commands() {
    use crate::inbox_notify::ReminderAnswer;
    assert_eq!(
        notification_request("r1".into(), ReminderAnswer::Snooze),
        Some(torro_ipc::Request::SnoozeReminder { id: "r1".into(), until: Snooze::Minutes(5) })
    );
    assert_eq!(
        notification_request("r1".into(), ReminderAnswer::Dismiss),
        Some(torro_ipc::Request::DismissReminder { id: "r1".into() })
    );
    assert_eq!(
        notification_request("r1".into(), ReminderAnswer::Open),
        Some(torro_ipc::Request::OpenApp { app: "torrocalendar".into(), target: None })
    );
    assert_eq!(notification_request("r1".into(), ReminderAnswer::Join), None, "the browser opens the call");
}

#[test]
fn a_notification_says_how_soon() {
    use crate::inbox_notify::reminder_text;
    let reminders = plan(&[event("sync", WORK, at(10, 0), 45)], &settings(), &Ledger::default(), at(9, 0));
    let sync = &reminders[0];
    let (soon, body) = reminder_text(sync, at(9, 55));
    assert_eq!(soon, torro_core::i18n::trf("{title} · in {n} min", &[("title", &"Event sync"), ("n", &5)]));
    assert!(body.contains(" – "), "{body}");
    let (now, _) = reminder_text(sync, at(10, 0));
    assert_eq!(now, torro_core::i18n::trf("{title} · now", &[("title", &"Event sync")]));
    let (late, _) = reminder_text(sync, at(10, 3));
    assert_eq!(late, torro_core::i18n::trf("{title} · started {n} min ago", &[("title", &"Event sync"), ("n", &3)]));
    assert!(crate::inbox_notify::asks_elsewhere(torro_inbox::Kind::Reminder), "no second popup from the inbox");
}

#[test]
fn a_full_screen_reminder_gets_no_notification_besides() {
    let mut events = [event("call", WORK, at(10, 0), 30), event("quiet", HOME, at(10, 0), 30)];
    events[0].alarms = vec![before(1, Some(AlarmStyle::FullScreen))];
    let due = plan(&events, &settings(), &Ledger::default(), at(9, 59));
    let (notify, full) = route(&due);
    assert!(full, "the window is wanted");
    assert_eq!(notify.iter().map(|r| r.event.uid.as_str()).collect::<Vec<_>>(), ["quiet"]);
    let quiet: Vec<Reminder> = due.iter().filter(|r| r.event.uid == "quiet").cloned().collect();
    let (notify, full) = route(&quiet);
    assert!(!full && notify.len() == 1);
}

#[test]
fn the_window_starts_on_the_screens_the_setting_and_the_window_manager_name() {
    assert_eq!(window_arguments(false, &[]), ["--reminders"]);
    assert_eq!(window_arguments(true, &[]), ["--reminders", "--all-displays"]);
    let monitors = ["DP-1".to_string(), "HDMI-A-1".to_string()];
    assert_eq!(window_arguments(true, &monitors), ["--reminders", "--all-displays", "--monitors", "DP-1,HDMI-A-1"]);
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_agent_app_beside_the_core_comes_first() {
    use std::path::{Path, PathBuf};
    let core = Path::new("/Volumes/Apps/Torro Workplace.app/Contents/MacOS/torroworkplace");
    let beside = PathBuf::from("/Volumes/Apps/Torro Reminder.app/Contents/MacOS/torro-reminder");
    let installed = PathBuf::from("/Applications/Torro Reminder.app/Contents/MacOS/torro-reminder");
    assert_eq!(reminder_program(Some(core), None, |p| p == beside || p == installed), beside);
    assert_eq!(reminder_program(Some(core), None, |p| p == installed), installed);
    let fallback = reminder_program(Some(core), None, |_| false);
    assert!(fallback.ends_with("torrocalendar"), "{fallback:?}");
}
