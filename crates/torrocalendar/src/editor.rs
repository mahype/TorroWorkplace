//! Creating and editing an event (task K.12 in `docs/tasks/kalender-aufgaben.md`).
//!
//! A card over the calendar with: title, calendar (pick list), all-day toggle, start date
//! and time, end date and time (German input, `torro_gui::datetime::parse_*`), location,
//! notes; buttons Save, Cancel and (for existing events) Delete. Invalid input shows a short
//! hint and disables Save. Recurring occurrences (`event.recurring`) open read-only with a
//! note that series cannot be edited yet, and only Close. Escape closes: [`subscription`]
//! reports it, and the app batches it into its own subscription while the editor is open.
//!
//! The form works on strings, as the user typed them. [`Form::build`] turns it into an
//! [`Event`]: all-day events store an exclusive end date, so the form's inclusive end date
//! gets one day added on save and one day taken off when an event is shown.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{button, column, container, pick_list, row, space, text, text_editor, text_input, toggler};
use iced::{Alignment, Background, Element, Length, Subscription, border, keyboard};
use torro_core::i18n::tr;
use torro_gui::datetime::{format_date, format_time, parse_date, parse_time, to_local, to_utc};
use torro_gui::theme;
use torro_pim::{Collection, Event, EventTime};

/// Start and end time shown for an all-day event, so that switching it to timed gives
/// sensible values.
const DEFAULT_START: &str = "09:00";
const DEFAULT_END: &str = "10:00";

/// The editor's state.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The event being edited; `None` for a new one.
    original: Option<Event>,
    /// The calendars to pick from.
    choices: Vec<CalendarChoice>,
    form: Form,
    /// The notes as an editor widget; `form.notes` mirrors its text.
    notes: text_editor::Content,
}

/// A calendar in the pick list: the id that goes into the event, the name that is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarChoice {
    id: String,
    name: String,
}

impl std::fmt::Display for CalendarChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Title(String),
    Calendar(CalendarChoice),
    AllDay(bool),
    StartDate(String),
    StartTime(String),
    EndDate(String),
    EndTime(String),
    Location(String),
    Notes(text_editor::Action),
    Save,
    Delete,
    Cancel,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save(Event),
    Delete(Event),
    Close,
}

/// Why the form cannot be saved yet. The first one found is shown as the hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    ChooseCalendar,
    CalendarMove,
    TitleEmpty,
    StartDate,
    StartTime,
    EndDate,
    EndTime,
    EndBeforeStart,
    EndDateBeforeStart,
}

impl Problem {
    fn message(self) -> &'static str {
        match self {
            Problem::ChooseCalendar => tr("Choose a calendar"),
            Problem::CalendarMove => tr("Moving an event to another calendar is not supported yet"),
            Problem::TitleEmpty => tr("Enter a title"),
            Problem::StartDate => tr("Start date is not valid, e.g. 11.10.2026"),
            Problem::StartTime => tr("Start time is not valid, e.g. 14:30"),
            Problem::EndDate => tr("End date is not valid, e.g. 11.10.2026"),
            Problem::EndTime => tr("End time is not valid, e.g. 15:30"),
            Problem::EndBeforeStart => tr("The end must be after the start"),
            Problem::EndDateBeforeStart => tr("The end date must not be before the start date"),
        }
    }
}

/// The form fields as the user sees them. Dates and times stay strings until saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    title: String,
    calendar: Option<CalendarChoice>,
    all_day: bool,
    start_date: String,
    start_time: String,
    /// Inclusive for all-day events: the last day the event covers.
    end_date: String,
    end_time: String,
    location: String,
    notes: String,
}

impl Form {
    /// A form for a span. For an all-day span, `end` only contributes its date, and that is
    /// the last day covered.
    fn from_span(all_day: bool, start: NaiveDateTime, end: NaiveDateTime, calendar: Option<CalendarChoice>) -> Form {
        let (start_time, end_time) = if all_day {
            (DEFAULT_START.to_string(), DEFAULT_END.to_string())
        } else {
            (format_time(start.time()), format_time(end.time()))
        };
        Form {
            title: String::new(),
            calendar,
            all_day,
            start_date: format_date(start.date()),
            start_time,
            end_date: format_date(end.date()),
            end_time,
            location: String::new(),
            notes: String::new(),
        }
    }

    /// A blank form for a new event: all-day without a time, else one hour from `time`.
    pub fn for_new(date: NaiveDate, time: Option<NaiveTime>, calendar: Option<CalendarChoice>) -> Form {
        match time {
            Some(time) => {
                let start = date.and_time(time);
                Form::from_span(false, start, start + Duration::hours(1), calendar)
            }
            None => {
                let day = date.and_time(NaiveTime::MIN);
                Form::from_span(true, day, day, calendar)
            }
        }
    }

    /// The form for an existing event, in local time. An event without an end gets one
    /// hour; an all-day end date is shown inclusive.
    pub fn from_event(event: &Event, calendar: Option<CalendarChoice>) -> Form {
        let start = match event.start {
            EventTime::Date(day) => day.and_time(NaiveTime::MIN),
            EventTime::DateTime(at) => to_local(at),
        };
        let end = if event.all_day() {
            let last = match event.end {
                Some(EventTime::Date(day)) => day - Duration::days(1),
                Some(EventTime::DateTime(at)) => to_local(at).date(),
                None => start.date(),
            };
            last.max(start.date()).and_time(NaiveTime::MIN)
        } else {
            match event.end {
                Some(EventTime::DateTime(at)) => to_local(at),
                _ => start + Duration::hours(1),
            }
        };
        let mut form = Form::from_span(event.all_day(), start, end, calendar);
        form.title = event.summary.clone();
        form.location = event.location.clone().unwrap_or_default();
        form.notes = event.description.clone().unwrap_or_default();
        form
    }

    /// The event this form describes. `original` is the event being edited: its uid, href,
    /// etag and recurrence are kept. `today` resolves dates typed without a year.
    pub fn build(&self, today: NaiveDate, original: Option<&Event>) -> Result<Event, Problem> {
        let calendar = self.calendar.as_ref().ok_or(Problem::ChooseCalendar)?;
        // The server copy of an event lives in its calendar; moving it is not supported yet.
        if original.is_some_and(|event| event.collection != calendar.id) {
            return Err(Problem::CalendarMove);
        }
        let title = self.title.trim();
        if title.is_empty() {
            return Err(Problem::TitleEmpty);
        }
        let start_date = parse_date(&self.start_date, today).ok_or(Problem::StartDate)?;
        let end_date = parse_date(&self.end_date, today).ok_or(Problem::EndDate)?;
        let (start, end) = if self.all_day {
            if end_date < start_date {
                return Err(Problem::EndDateBeforeStart);
            }
            // The end is exclusive: the day after the last day covered.
            (EventTime::Date(start_date), EventTime::Date(end_date + Duration::days(1)))
        } else {
            let start = start_date.and_time(parse_time(&self.start_time).ok_or(Problem::StartTime)?);
            let end = end_date.and_time(parse_time(&self.end_time).ok_or(Problem::EndTime)?);
            if end <= start {
                return Err(Problem::EndBeforeStart);
            }
            (EventTime::DateTime(to_utc(start)), EventTime::DateTime(to_utc(end)))
        };

        let mut event = original.cloned().unwrap_or_else(|| Event {
            uid: String::new(),
            collection: calendar.id.clone(),
            href: None,
            etag: None,
            summary: String::new(),
            start,
            end: None,
            location: None,
            description: None,
            recurring: false,
        });
        event.summary = title.to_string();
        event.start = start;
        event.end = Some(end);
        event.location = non_empty(&self.location);
        event.description = non_empty(&self.notes);
        Ok(event)
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The calendars to offer. The event's own calendar is kept even when the list does not
/// have it, so that the event can still be saved.
fn calendar_choices(calendars: &[Collection], current: Option<&str>) -> Vec<CalendarChoice> {
    let mut choices: Vec<CalendarChoice> = calendars
        .iter()
        .filter(|c| c.kind.has_events())
        .map(|c| CalendarChoice { id: c.id.clone(), name: c.name.clone() })
        .collect();
    if let Some(id) = current
        && !choices.iter().any(|c| c.id == id)
    {
        choices.insert(0, CalendarChoice { id: id.to_string(), name: id.to_string() });
    }
    choices
}

impl Editor {
    /// A new event on `date` at `time` (one hour long), or all-day with `None`, in the first
    /// of `calendars`.
    pub fn new_event(calendars: &[Collection], date: NaiveDate, time: Option<NaiveTime>) -> Editor {
        let choices = calendar_choices(calendars, None);
        let form = Form::for_new(date, time, choices.first().cloned());
        Editor { original: None, choices, notes: text_editor::Content::new(), form }
    }

    pub fn edit(event: &Event, calendars: &[Collection]) -> Editor {
        let choices = calendar_choices(calendars, Some(&event.collection));
        let calendar = choices.iter().find(|c| c.id == event.collection).cloned();
        let form = Form::from_event(event, calendar);
        Editor { original: Some(event.clone()), choices, notes: text_editor::Content::with_text(&form.notes), form }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Title(value) => self.form.title = value,
            Message::Calendar(choice) => self.form.calendar = Some(choice),
            Message::AllDay(on) => self.form.all_day = on,
            Message::StartDate(value) => self.form.start_date = value,
            Message::StartTime(value) => self.form.start_time = value,
            Message::EndDate(value) => self.form.end_date = value,
            Message::EndTime(value) => self.form.end_time = value,
            Message::Location(value) => self.form.location = value,
            Message::Notes(action) => {
                self.notes.perform(action);
                self.form.notes = self.notes.text();
            }
            Message::Save => {
                if self.is_recurring() {
                    return Action::None;
                }
                if let Ok(event) = self.check() {
                    return Action::Save(event);
                }
            }
            Message::Delete => {
                if let Some(event) = self.original.as_ref().filter(|event| !event.recurring) {
                    return Action::Delete(event.clone());
                }
            }
            Message::Cancel => return Action::Close,
        }
        Action::None
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.is_recurring() { self.read_only_view() } else { self.form_view() }
    }

    fn is_recurring(&self) -> bool {
        self.original.as_ref().is_some_and(|event| event.recurring)
    }

    /// The event the form describes today, or why it cannot be saved yet.
    fn check(&self) -> Result<Event, Problem> {
        self.form.build(Local::now().date_naive(), self.original.as_ref())
    }

    fn form_view(&self) -> Element<'_, Message> {
        let form = &self.form;
        let problem = self.check().err();

        let mut start = row![text_input(tr("Date"), &form.start_date).on_input(Message::StartDate).width(Length::Fill)]
            .spacing(8)
            .width(Length::Fill);
        let mut end = row![text_input(tr("Date"), &form.end_date).on_input(Message::EndDate).width(Length::Fill)]
            .spacing(8)
            .width(Length::Fill);
        if !form.all_day {
            start =
                start.push(text_input(tr("Time"), &form.start_time).on_input(Message::StartTime).width(Length::Fill));
            end = end.push(text_input(tr("Time"), &form.end_time).on_input(Message::EndTime).width(Length::Fill));
        }

        let calendar = pick_list(self.choices.as_slice(), form.calendar.clone(), Message::Calendar)
            .placeholder(tr("Choose a calendar"))
            .width(Length::Fill);

        let mut actions = row![].spacing(8).width(Length::Fill).align_y(Alignment::Center);
        if self.original.is_some() {
            actions = actions.push(button(text(tr("Delete"))).style(button::danger).on_press(Message::Delete));
        }
        actions = actions
            .push(space().width(Length::Fill))
            .push(button(text(tr("Cancel"))).style(button::secondary).on_press(Message::Cancel))
            .push(
                button(text(tr("Save")))
                    .style(button::primary)
                    .on_press_maybe(problem.is_none().then_some(Message::Save)),
            );

        let heading = if self.original.is_some() { tr("Edit event") } else { tr("New event") };
        let mut body = column![
            text(heading).size(18),
            field(tr("Title"), text_input("", &form.title).on_input(Message::Title)),
            field(tr("Calendar"), calendar),
            toggler(form.all_day).label(tr("All day")).on_toggle(Message::AllDay),
            field(tr("Start"), start),
            field(tr("End"), end),
            field(tr("Location"), text_input("", &form.location).on_input(Message::Location)),
            field(tr("Notes"), text_editor(&self.notes).on_action(Message::Notes).height(120.0)),
        ]
        .spacing(12)
        .width(Length::Fill);
        if let Some(problem) = problem {
            body = body.push(text(problem.message()).size(13).color(theme::attention()));
        }
        card(body.push(actions))
    }

    /// A recurring occurrence: the details as text, a note, and Close.
    fn read_only_view(&self) -> Element<'_, Message> {
        let form = &self.form;
        let when = if form.all_day {
            format!("{} · {} – {}", tr("All day"), form.start_date, form.end_date)
        } else {
            format!("{} {} – {} {}", form.start_date, form.start_time, form.end_date, form.end_time)
        };
        let calendar = form.calendar.as_ref().map(|c| c.name.clone()).unwrap_or_default();

        let mut body =
            column![text(form.title.clone()).size(18), detail(tr("Calendar"), calendar), detail(tr("When"), when)]
                .spacing(12)
                .width(Length::Fill);
        if !form.location.is_empty() {
            body = body.push(detail(tr("Location"), form.location.clone()));
        }
        if !form.notes.is_empty() {
            body = body.push(detail(tr("Notes"), form.notes.clone()));
        }
        body = body.push(text(tr("Recurring events cannot be edited yet")).size(13).color(theme::MUTED));
        let close = row![
            space().width(Length::Fill),
            button(text(tr("Close"))).style(button::secondary).on_press(Message::Cancel)
        ]
        .width(Length::Fill)
        .align_y(Alignment::Center);
        card(body.push(close))
    }
}

/// Escape closes the editor. The app adds this to its subscriptions while an editor is open.
/// The event is listened to whatever the widgets did with it, since a focused text field
/// takes Escape for itself.
pub fn subscription() -> Subscription<Message> {
    iced::event::listen_with(escape_to_cancel)
}

fn escape_to_cancel(
    event: iced::event::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::event::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Cancel),
        _ => None,
    }
}

/// A label above an input.
fn field<'a>(label: &'static str, input: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(12).color(theme::MUTED), input.into()].spacing(4).width(Length::Fill).into()
}

/// A label above a value, for the read-only view.
fn detail<'a>(label: &'static str, value: String) -> Element<'a, Message> {
    column![text(label).size(12).color(theme::MUTED), text(value).size(14)].spacing(4).width(Length::Fill).into()
}

/// The surface the editor sits on: a rounded card of at most 460 px.
fn card<'a>(body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(body)
        .padding(24)
        .width(Length::Fill)
        .max_width(460.0)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::SURFACE)),
            border: border::rounded(12.0),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use torro_gui::datetime::to_utc;
    use torro_pim::CollectionKind;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        day(y, m, d).and_hms_opt(h, min, 0).unwrap()
    }

    fn calendar(id: &str, name: &str) -> Collection {
        Collection { id: id.into(), name: name.into(), kind: CollectionKind::Events, color: None }
    }

    fn calendars() -> Vec<Collection> {
        vec![calendar("work", "Work"), calendar("home", "Home")]
    }

    fn stored(summary: &str, start: EventTime, end: Option<EventTime>) -> Event {
        Event {
            uid: "uid-1".into(),
            collection: "work".into(),
            href: Some("/work/uid-1.ics".into()),
            etag: Some("\"7\"".into()),
            summary: summary.into(),
            start,
            end,
            location: Some("Room 3".into()),
            description: Some("Bring slides".into()),
            recurring: false,
        }
    }

    fn set_title(editor: &mut Editor, title: &str) {
        editor.update(Message::Title(title.into()));
    }

    fn saved(action: Action) -> Event {
        match action {
            Action::Save(event) => event,
            other => panic!("expected Save, got {other:?}"),
        }
    }

    fn problem(editor: &Editor) -> Problem {
        editor.check().expect_err("the form should not be valid")
    }

    #[test]
    fn new_timed_event_starts_at_the_given_time_and_lasts_an_hour() {
        let mut editor =
            Editor::new_event(&calendars(), day(2026, 10, 11), Some(NaiveTime::from_hms_opt(14, 30, 0).unwrap()));
        set_title(&mut editor, "  Dentist  ");
        let event = saved(editor.update(Message::Save));

        assert_eq!(event.summary, "Dentist");
        assert_eq!(event.uid, "");
        assert_eq!(event.collection, "work");
        assert!(event.href.is_none() && event.etag.is_none());
        assert_eq!(event.start, EventTime::DateTime(to_utc(at(2026, 10, 11, 14, 30))));
        assert_eq!(event.end, Some(EventTime::DateTime(to_utc(at(2026, 10, 11, 15, 30)))));
        assert!(!event.all_day());
    }

    #[test]
    fn new_all_day_event_stores_an_exclusive_end() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        assert!(editor.form.all_day);
        set_title(&mut editor, "Conference");
        // A one-day event: the inclusive end date is the start date, the stored end is the next day.
        let event = saved(editor.update(Message::Save));
        assert_eq!(event.start, EventTime::Date(day(2026, 10, 11)));
        assert_eq!(event.end, Some(EventTime::Date(day(2026, 10, 12))));

        // Covering three days: 11.10. to 13.10. inclusive ends at 14.10. exclusive.
        editor.update(Message::EndDate("13.10.2026".into()));
        let event = saved(editor.update(Message::Save));
        assert_eq!(event.end, Some(EventTime::Date(day(2026, 10, 14))));
    }

    #[test]
    fn switching_to_timed_keeps_sensible_times() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        set_title(&mut editor, "Walk");
        editor.update(Message::AllDay(false));
        let event = saved(editor.update(Message::Save));
        assert_eq!(event.start, EventTime::DateTime(to_utc(at(2026, 10, 11, 9, 0))));
        assert_eq!(event.end, Some(EventTime::DateTime(to_utc(at(2026, 10, 11, 10, 0)))));
    }

    #[test]
    fn validation_reports_the_first_problem() {
        let mut editor =
            Editor::new_event(&calendars(), day(2026, 10, 11), Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap()));
        assert_eq!(problem(&editor), Problem::TitleEmpty);
        assert!(matches!(editor.update(Message::Save), Action::None));

        set_title(&mut editor, "Call");
        assert!(editor.check().is_ok());

        editor.update(Message::StartDate("31.02.2026".into()));
        assert_eq!(problem(&editor), Problem::StartDate);
        editor.update(Message::StartDate("11.10.".into()));

        editor.update(Message::StartTime("25:00".into()));
        assert_eq!(problem(&editor), Problem::StartTime);
        editor.update(Message::StartTime("14:00".into()));

        editor.update(Message::EndDate("gestern".into()));
        assert_eq!(problem(&editor), Problem::EndDate);
        editor.update(Message::EndDate("11.10.2026".into()));

        editor.update(Message::EndTime("13:59".into()));
        assert_eq!(problem(&editor), Problem::EndBeforeStart);
        editor.update(Message::EndTime("14:00".into()));
        assert_eq!(problem(&editor), Problem::EndBeforeStart, "a zero-length event is not allowed");
        editor.update(Message::EndTime("14:01".into()));
        assert!(editor.check().is_ok());
    }

    #[test]
    fn all_day_end_date_may_not_be_before_start_date() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        set_title(&mut editor, "Trip");
        editor.update(Message::EndDate("10.10.2026".into()));
        assert_eq!(problem(&editor), Problem::EndDateBeforeStart);
        // Times are hidden for all-day events, so a bad time does not block saving.
        editor.update(Message::EndDate("11.10.2026".into()));
        editor.update(Message::StartTime("nonsense".into()));
        assert!(editor.check().is_ok());
    }

    #[test]
    fn calendar_is_required_and_fixed_for_existing_events() {
        let mut editor = Editor::new_event(&[], day(2026, 10, 11), None);
        set_title(&mut editor, "Orphan");
        assert_eq!(problem(&editor), Problem::ChooseCalendar);

        let event = stored("Standup", EventTime::Date(day(2026, 10, 11)), Some(EventTime::Date(day(2026, 10, 12))));
        let mut editor = Editor::edit(&event, &calendars());
        assert!(editor.check().is_ok());
        editor.update(Message::Calendar(CalendarChoice { id: "home".into(), name: "Home".into() }));
        assert_eq!(problem(&editor), Problem::CalendarMove);
    }

    #[test]
    fn prefill_from_a_timed_event_shows_local_times() {
        let event = stored(
            "Review",
            EventTime::DateTime(to_utc(at(2026, 10, 11, 14, 30))),
            Some(EventTime::DateTime(to_utc(at(2026, 10, 11, 16, 0)))),
        );
        let editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.form.title, "Review");
        assert_eq!(editor.form.calendar.as_ref().map(|c| c.id.as_str()), Some("work"));
        assert!(!editor.form.all_day);
        assert_eq!((editor.form.start_date.as_str(), editor.form.start_time.as_str()), ("11.10.2026", "14:30"));
        assert_eq!((editor.form.end_date.as_str(), editor.form.end_time.as_str()), ("11.10.2026", "16:00"));
        assert_eq!(editor.form.location, "Room 3");
        assert_eq!(editor.form.notes, "Bring slides");
    }

    #[test]
    fn prefill_from_an_all_day_event_shows_the_inclusive_end() {
        let event = stored("Holiday", EventTime::Date(day(2026, 10, 11)), Some(EventTime::Date(day(2026, 10, 14))));
        let editor = Editor::edit(&event, &calendars());
        assert!(editor.form.all_day);
        assert_eq!(editor.form.start_date, "11.10.2026");
        assert_eq!(editor.form.end_date, "13.10.2026");
    }

    #[test]
    fn saving_an_unchanged_event_keeps_its_identity() {
        let event = stored(
            "Review",
            EventTime::DateTime(to_utc(at(2026, 10, 11, 14, 30))),
            Some(EventTime::DateTime(to_utc(at(2026, 10, 11, 16, 0)))),
        );
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(saved(editor.update(Message::Save)), event);

        let all_day = stored("Holiday", EventTime::Date(day(2026, 10, 11)), Some(EventTime::Date(day(2026, 10, 14))));
        let mut editor = Editor::edit(&all_day, &calendars());
        assert_eq!(saved(editor.update(Message::Save)), all_day);
    }

    #[test]
    fn clearing_location_and_notes_stores_none() {
        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let mut editor = Editor::edit(&event, &calendars());
        editor.update(Message::Location("   ".into()));
        editor.form.notes = String::new();
        let saved = saved(editor.update(Message::Save));
        assert_eq!(saved.location, None);
        assert_eq!(saved.description, None);
        assert_eq!(saved.end, Some(EventTime::Date(day(2026, 10, 12))));
    }

    #[test]
    fn delete_only_for_existing_events() {
        let mut new = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        assert!(matches!(new.update(Message::Delete), Action::None));

        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let mut existing = Editor::edit(&event, &calendars());
        match existing.update(Message::Delete) {
            Action::Delete(deleted) => assert_eq!(deleted, event),
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn cancel_and_escape_close() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        assert!(matches!(editor.update(Message::Cancel), Action::Close));

        let escape = iced::event::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        });
        let window = iced::window::Id::unique();
        assert!(matches!(escape_to_cancel(escape, iced::event::Status::Ignored, window), Some(Message::Cancel)));
    }

    #[test]
    fn recurring_occurrences_are_read_only() {
        let mut event = stored("Standup", EventTime::DateTime(to_utc(at(2026, 10, 11, 9, 0))), None);
        event.recurring = true;
        let mut editor = Editor::edit(&event, &calendars());
        set_title(&mut editor, "Changed");
        assert!(matches!(editor.update(Message::Save), Action::None));
        assert!(matches!(editor.update(Message::Delete), Action::None));
        assert!(matches!(editor.update(Message::Cancel), Action::Close));
        // The view of a series builds without the form widgets.
        let _view: Element<'_, Message> = editor.view();
    }

    #[test]
    fn form_view_builds_for_new_and_existing_events() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        editor.update(Message::Notes(text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
            "line one\nline two".into(),
        )))));
        assert_eq!(editor.form.notes, "line one\nline two");
        let _view: Element<'_, Message> = editor.view();

        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let editor = Editor::edit(&event, &calendars());
        let _view: Element<'_, Message> = editor.view();
    }
}
