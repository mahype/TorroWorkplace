//! Creating and editing an event (task K.12 in `docs/tasks/kalender-aufgaben.md`).
//!
//! A dialog over the calendar, after Linear (task C.5): a large borderless title, then the
//! calendar (pick list with its colour dot), the all-day switch, start and end (date and time
//! side by side, `torro_gui::datetime::parse_*`), location and notes; Delete (danger) on the
//! left, Cancel (ghost) and Save (the one primary button, 36 high as in a Linear dialog) on the
//! right. The fields scroll when the window is too low for them; the actions stay below them.
//!
//! Errors stay hidden until the first attempt to save. Save is always active: with problems it
//! marks the field that is wrong, says why under it and hands the app an [`Action::Focus`] for
//! it, then the message follows the input live. (Iced 0.14 reports no "field left" event, so
//! leaving a field alone does not validate it.) Enter in a single-line field saves.
//!
//! Recurring occurrences (`event.recurring`) and events of read-only calendars open as plain
//! details with a note and only Close. Escape closes: [`subscription`] reports it, and the app
//! batches it into its own subscription while the editor is open.
//!
//! The form works on strings, as the user typed them. [`Form::build`] turns it into an
//! [`Event`]: all-day events store an exclusive end date, so the form's inclusive end date
//! gets one day added on save and one day taken off when an event is shown.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::{
    button, column, container, pick_list, row, rule, scrollable, space as blank, stack, svg, text, text_editor,
    text_input, toggler,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Subscription, Theme, keyboard};
use torro_core::i18n::tr;
use torro_gui::datetime::{format_date, format_time, parse_date, parse_time, to_local, to_utc};
use torro_gui::icon::{self, Icon, icon};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{collection_color, color_dot, icon_button, text_button};
use torro_gui::{style, typo};
use torro_pim::{Collection, Event, EventTime};

/// Width of the dialog; the app centres it at most this wide and lets it shrink in narrow windows.
pub const WIDTH: f32 = 520.0;

/// Ids of the inputs that [`Action::Focus`] can name.
const TITLE_ID: &str = "editor-title";
const START_DATE_ID: &str = "editor-start-date";
const START_TIME_ID: &str = "editor-start-time";
const END_DATE_ID: &str = "editor-end-date";
const END_TIME_ID: &str = "editor-end-time";

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
    /// Why the event can only be looked at (recurring, read-only calendar); `None` = editable.
    read_only: Option<&'static str>,
    form: Form,
    /// The notes as an editor widget; `form.notes` mirrors its text.
    notes: text_editor::Content,
    /// Save has been pressed once: from then on the first problem is shown at its field.
    attempted: bool,
}

/// A calendar in the pick list: the id that goes into the event, the name that is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarChoice {
    id: String,
    name: String,
    /// The calendar's colour as the server reports it (`#RRGGBB`), for the dot next to the name.
    color: Option<String>,
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
    /// Give the keyboard focus to the input with this id (the first one with a problem).
    Focus(&'static str),
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

/// The group of the form a problem belongs to, which is where its message is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Calendar,
    Title,
    Start,
    End,
}

impl Problem {
    fn field(self) -> Field {
        match self {
            Problem::ChooseCalendar | Problem::CalendarMove => Field::Calendar,
            Problem::TitleEmpty => Field::Title,
            Problem::StartDate | Problem::StartTime => Field::Start,
            Problem::EndDate | Problem::EndTime | Problem::EndBeforeStart | Problem::EndDateBeforeStart => Field::End,
        }
    }

    /// The input to mark and focus. The calendar is a pick list, which Iced cannot focus.
    fn input(self) -> Option<&'static str> {
        match self {
            Problem::ChooseCalendar | Problem::CalendarMove => None,
            Problem::TitleEmpty => Some(TITLE_ID),
            Problem::StartDate => Some(START_DATE_ID),
            Problem::StartTime => Some(START_TIME_ID),
            Problem::EndDate | Problem::EndDateBeforeStart => Some(END_DATE_ID),
            Problem::EndTime | Problem::EndBeforeStart => Some(END_TIME_ID),
        }
    }

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

/// The writable calendars to offer. The event's own calendar is kept even when it is not
/// among them, so that it can still be shown.
fn calendar_choices(calendars: &[Collection], current: Option<&str>) -> Vec<CalendarChoice> {
    let mut choices: Vec<CalendarChoice> = calendars
        .iter()
        .filter(|c| c.kind.has_events() && c.writable)
        .map(|c| CalendarChoice { id: c.id.clone(), name: c.name.clone(), color: c.color.clone() })
        .collect();
    if let Some(id) = current
        && !choices.iter().any(|c| c.id == id)
    {
        let own = calendars.iter().find(|c| c.id == id);
        let name = own.map_or_else(|| id.to_string(), |c| c.name.clone());
        choices.insert(0, CalendarChoice { id: id.to_string(), name, color: own.and_then(|c| c.color.clone()) });
    }
    choices
}

impl Editor {
    /// A new event on `date` at `time` (one hour long), or all-day with `None`, in the first
    /// of `calendars`.
    pub fn new_event(calendars: &[Collection], date: NaiveDate, time: Option<NaiveTime>) -> Editor {
        let choices = calendar_choices(calendars, None);
        let form = Form::for_new(date, time, choices.first().cloned());
        Editor { original: None, choices, read_only: None, notes: text_editor::Content::new(), form, attempted: false }
    }

    pub fn edit(event: &Event, calendars: &[Collection]) -> Editor {
        let choices = calendar_choices(calendars, Some(&event.collection));
        let calendar = choices.iter().find(|c| c.id == event.collection).cloned();
        let form = Form::from_event(event, calendar);
        let read_only = if event.recurring {
            Some("Recurring events cannot be edited yet")
        } else if calendars.iter().any(|c| c.id == event.collection && !c.writable) {
            Some("This calendar is read-only")
        } else {
            None
        };
        Editor {
            original: Some(event.clone()),
            choices,
            read_only,
            notes: text_editor::Content::with_text(&form.notes),
            form,
            attempted: false,
        }
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
                match self.check() {
                    Ok(event) => return Action::Save(event),
                    Err(problem) => {
                        self.attempted = true;
                        return problem.input().map_or(Action::None, Action::Focus);
                    }
                }
            }
            Message::Delete => {
                if let Some(event) = self.original.as_ref().filter(|_| self.read_only.is_none()) {
                    return Action::Delete(event.clone());
                }
            }
            Message::Cancel => return Action::Close,
        }
        Action::None
    }

    pub fn view(&self) -> Element<'_, Message> {
        self.view_in(Frame::Dialog)
    }

    /// The editor filling the window (narrow layout, task K.19): the same content on the dialog
    /// surface, without the dialog's border and shadow, scrolling within the window's height.
    pub fn view_full_window(&self) -> Element<'_, Message> {
        self.view_in(Frame::Window)
    }

    fn view_in(&self, frame: Frame) -> Element<'_, Message> {
        if self.is_recurring() { self.read_only_view(frame) } else { self.form_view(frame) }
    }

    fn is_recurring(&self) -> bool {
        self.read_only.is_some()
    }

    /// The event the form describes today, or why it cannot be saved yet.
    fn check(&self) -> Result<Event, Problem> {
        self.form.build(Local::now().date_naive(), self.original.as_ref())
    }

    /// The first problem, once Save has been pressed; before that the form stays quiet.
    fn shown_problem(&self) -> Option<Problem> {
        if self.attempted { self.check().err() } else { None }
    }

    fn form_view(&self, frame: Frame) -> Element<'_, Message> {
        let form = &self.form;
        let shown = self.shown_problem();
        let invalid = |input: &str| shown.is_some_and(|problem| problem.input() == Some(input));
        let problem_in = |field: Field| shown.filter(|problem| problem.field() == field);

        let heading = if self.original.is_some() { tr("Edit event") } else { tr("New event") };
        let head = row![
            typo::text(typo::CAPTION, heading).style(theme::text_3),
            blank::horizontal(),
            icon_button(Icon::X, tr("Close"), Some(Message::Cancel)),
        ]
        .align_y(Alignment::Center);
        let title = group(None, title_input(&form.title), problem_in(Field::Title));

        let calendar_invalid = problem_in(Field::Calendar).is_some();
        let mut calendar = row![].spacing(space::S).align_y(Alignment::Center);
        if let Some(choice) = &form.calendar {
            calendar = calendar.push(color_dot(collection_color(choice.color.as_deref()), space::S));
        }
        let choices = pick_list(self.choices.as_slice(), form.calendar.clone(), Message::Calendar)
            .placeholder(tr("Choose a calendar"))
            // Iced's handle is a glyph from a font; the design wants the Lucide chevron, which is an
            // SVG, so it lies over the list instead ([`chevron_over`]) and the text keeps clear of it.
            .handle(pick_list::Handle::None)
            .padding(Padding { right: style::INPUT_PADDING.right + icon::SIZE_SM + space::S, ..style::INPUT_PADDING })
            .text_size(typo::BODY.size)
            .text_line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .width(Length::Fill)
            .style(move |theme, status| {
                let mut look = style::pick_list(theme, status);
                if calendar_invalid {
                    look.border.color = palette(theme).error_text;
                }
                look
            })
            .menu_style(style::pick_list_menu);
        calendar = calendar.push(chevron_over(choices));

        let all_day = toggler(form.all_day)
            .label(tr("All day"))
            .on_toggle(Message::AllDay)
            .size(style::CHECK_SIZE)
            .text_size(typo::BODY.size)
            .text_line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .spacing(space::S)
            .style(style::toggler);

        let start = moment(
            form.all_day,
            Input::new(tr("Date"), &form.start_date, Message::StartDate, START_DATE_ID, invalid(START_DATE_ID)),
            Input::new(tr("Time"), &form.start_time, Message::StartTime, START_TIME_ID, invalid(START_TIME_ID)),
        );
        let end = moment(
            form.all_day,
            Input::new(tr("Date"), &form.end_date, Message::EndDate, END_DATE_ID, invalid(END_DATE_ID)),
            Input::new(tr("Time"), &form.end_time, Message::EndTime, END_TIME_ID, invalid(END_TIME_ID)),
        );

        let location = text_input("", &form.location)
            .on_input(Message::Location)
            .on_submit(Message::Save)
            .padding(style::INPUT_PADDING)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .style(flagged(false));
        let notes = text_editor(&self.notes)
            .on_action(Message::Notes)
            .padding(style::INPUT_PADDING)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .min_height(size::CONTROL * NOTES_MIN_ROWS)
            .max_height(size::CONTROL * NOTES_MAX_ROWS)
            .style(style::text_editor);

        // Save stays active so that a click always answers: it saves, or shows what is missing.
        let mut actions = row![].spacing(space::S).align_y(Alignment::Center);
        if self.original.is_some() {
            actions = actions.push(text_button(None, tr("Delete"), style::button::danger, Some(Message::Delete)));
        }
        actions = actions
            .push(blank::horizontal())
            .push(text_button(None, tr("Cancel"), style::button::ghost, Some(Message::Cancel)))
            .push(dialog_primary(tr("Save"), Message::Save));

        let fields = column![
            column![head, title].spacing(space::S),
            group(Some(tr("Calendar")), calendar, problem_in(Field::Calendar)),
            all_day,
            group(Some(tr("Start")), start, problem_in(Field::Start)),
            group(Some(tr("End")), end, problem_in(Field::End)),
            group(Some(tr("Location")), location, None),
            group(Some(tr("Notes")), notes, None),
        ]
        .spacing(space::L);
        card(fields, actions, frame)
    }

    /// A recurring occurrence or an event in a read-only calendar: the details as text, a
    /// note, and Close.
    fn read_only_view(&self, frame: Frame) -> Element<'_, Message> {
        let form = &self.form;
        let when = if form.all_day {
            format!("{} · {} – {}", tr("All day"), form.start_date, form.end_date)
        } else {
            format!("{} {} – {} {}", form.start_date, form.start_time, form.end_date, form.end_time)
        };
        let calendar = form.calendar.as_ref().map(|c| (collection_color(c.color.as_deref()), c.name.clone()));

        let head = row![
            typo::text(typo::TITLE, form.title.clone()).width(Length::Fill),
            icon_button(Icon::X, tr("Close"), Some(Message::Cancel)),
        ]
        .spacing(space::S)
        .align_y(Alignment::Start);
        let mut details = column![].spacing(space::L);
        if let Some((color, name)) = calendar {
            let value = row![color_dot(color, space::S), typo::text(typo::BODY, name)]
                .spacing(space::S)
                .align_y(Alignment::Center);
            details = details.push(detail(tr("Calendar"), value));
        }
        details = details.push(detail(tr("When"), typo::text(typo::BODY.tabular(), when)));
        if !form.location.is_empty() {
            details = details.push(detail(tr("Location"), typo::text(typo::BODY, form.location.clone())));
        }
        if !form.notes.is_empty() {
            details = details.push(detail(tr("Notes"), typo::text(typo::BODY, form.notes.clone())));
        }
        let mut body = column![head, details].spacing(space::L);
        if let Some(reason) = self.read_only {
            body = body.push(typo::text(typo::META, tr(reason)).style(theme::text_2));
        }
        let close =
            row![blank::horizontal(), text_button(None, tr("Close"), style::button::secondary, Some(Message::Cancel))]
                .align_y(Alignment::Center);
        card(body, close, frame)
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

/// Date and time side by side, in these proportions (a date is wider than "14:30").
const DATE_PORTION: u16 = 3;
const TIME_PORTION: u16 = 2;
/// The notes start three input heights tall and grow to five before they scroll.
const NOTES_MIN_ROWS: f32 = 3.0;
const NOTES_MAX_ROWS: f32 = 5.0;

/// One single-line field of the form, with what it needs to be built.
struct Input<'a> {
    placeholder: &'static str,
    value: &'a str,
    on_input: fn(String) -> Message,
    id: &'static str,
    invalid: bool,
}

impl<'a> Input<'a> {
    fn new(
        placeholder: &'static str,
        value: &'a str,
        on_input: fn(String) -> Message,
        id: &'static str,
        invalid: bool,
    ) -> Self {
        Input { placeholder, value, on_input, id, invalid }
    }

    /// A 32-high input in tabular figures, so that dates and times line up; Enter saves.
    fn view(self) -> text_input::TextInput<'a, Message> {
        text_input(self.placeholder, self.value)
            .id(self.id)
            .on_input(self.on_input)
            .on_submit(Message::Save)
            .padding(style::INPUT_PADDING)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.tabular().font)
            .style(flagged(self.invalid))
    }
}

/// Date and time side by side. All-day events have no time; the empty cell keeps the date as wide
/// as when timed.
fn moment<'a>(all_day: bool, date: Input<'a>, time: Input<'a>) -> Element<'a, Message> {
    let time: Element<'a, Message> = if all_day {
        blank::horizontal().width(Length::FillPortion(TIME_PORTION)).into()
    } else {
        time.view().width(Length::FillPortion(TIME_PORTION)).into()
    };
    row![date.view().width(Length::FillPortion(DATE_PORTION)), time].spacing(space::S).into()
}

/// The input style, with the border in the error colour while the field has the problem. The
/// message under the field says what is wrong, so the colour is never the only sign.
fn flagged(invalid: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let mut look = style::text_input(theme, status);
        if invalid {
            look.border.color = palette(theme).error_text;
        }
        look
    }
}

/// The event's title: large (`TITLE`) and without a frame, like the title of a Linear issue. It
/// has no horizontal padding so that its text sits on the same edge as the labels below.
fn title_input(value: &str) -> Element<'_, Message> {
    text_input(tr("Title"), value)
        .id(TITLE_ID)
        .on_input(Message::Title)
        .on_submit(Message::Save)
        .padding(Padding::new(0.0).top(space::XS).bottom(space::XS))
        .size(typo::TITLE.size)
        .line_height(typo::TITLE.line_height)
        .font(typo::TITLE.font)
        .style(|theme, status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            ..style::text_input(theme, status)
        })
        .into()
}

/// A label (`CAPTION`, `text_2`) 4 above its input, and under it the problem with this field.
fn group<'a>(
    label: Option<&'static str>,
    input: impl Into<Element<'a, Message>>,
    problem: Option<Problem>,
) -> Element<'a, Message> {
    let mut group = column![].spacing(space::XS).width(Length::Fill);
    if let Some(label) = label {
        group = group.push(typo::text(typo::CAPTION, label).style(theme::text_2));
    }
    group = group.push(input.into());
    if let Some(problem) = problem {
        group = group.push(problem_line(problem));
    }
    group.into()
}

/// What is wrong, in the error colour with an icon: "(!) Enter a title".
fn problem_line<'a>(problem: Problem) -> Element<'a, Message> {
    let mark =
        icon(Icon::CircleAlert, icon::SIZE_SM).style(|theme, _| svg::Style { color: Some(palette(theme).error_text) });
    row![
        // The icon is centred on the first line of the message, which may wrap in a narrow window.
        container(mark).center_y(typo::META.line_height.to_absolute(typo::META.size)),
        typo::text(typo::META, problem.message()).style(|theme| text::Style { color: Some(palette(theme).error_text) }),
    ]
    .spacing(space::XS)
    .align_y(Alignment::Start)
    .into()
}

/// A label above a value, for the read-only view.
fn detail<'a>(label: &'static str, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![typo::text(typo::CAPTION, label).style(theme::text_2), value.into()]
        .spacing(space::XS)
        .width(Length::Fill)
        .into()
}

/// Where the editor sits: in a dialog over the window, or filling the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Dialog,
    Window,
}

/// Height of the footer under the fields: the line, the gap above the buttons, the 36-high
/// primary button and the card's padding below it.
const FOOTER: f32 = size::HAIRLINE + space::L + size::CONTROL_LG + space::XL;

/// The primary button of a dialog: 36 high ([`size::CONTROL_LG`]), the label in `BODY_STRONG`.
fn dialog_primary<'a>(label: &'a str, on_press: Message) -> Element<'a, Message> {
    button(container(typo::text(typo::BODY_STRONG, label)).center_y(Length::Fill))
        .height(size::CONTROL_LG)
        .padding(style::BUTTON_PADDING)
        .style(style::button::primary)
        .on_press(on_press)
        .into()
}

/// `list` with Lucide's `chevron-down` in `text_3` over its right end, where Iced draws its own
/// arrow. The icon takes no input, so clicks on it reach the list.
fn chevron_over<'a>(list: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let chevron =
        icon(Icon::ChevronDown, icon::SIZE_SM).style(|theme, _| svg::Style { color: Some(palette(theme).text_3) });
    let chevron = container(chevron)
        .padding(Padding::ZERO.right(style::INPUT_PADDING.right))
        .align_right(Length::Fill)
        .center_y(Length::Fill);
    stack![list.into(), chevron].width(Length::Fill).into()
}

/// The surface the editor sits on: a dialog ([`style::container::popover`]: `menu`, radius 10),
/// or the whole window on the same `menu` colour without border and shadow. `body` scrolls when
/// the window is too low for it; `actions` stay fixed below it, under a line.
///
/// Iced lays out a column from the top and would give the actions what the scrolling body left,
/// which is nothing in a low window. So the body is the base of a stack and keeps the footer's
/// height free below it, and the footer is a layer aligned to the bottom of that base: the stack
/// is as high as the base, at most the window, and the actions always show.
fn card<'a>(
    body: impl Into<Element<'a, Message>>,
    actions: impl Into<Element<'a, Message>>,
    frame: Frame,
) -> Element<'a, Message> {
    let inner = container(body).padding(Padding::new(space::XL).bottom(space::L)).width(Length::Fill);
    let mut scroll = scrollable(inner)
        .width(Length::Fill)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable);
    if frame == Frame::Window {
        scroll = scroll.height(Length::Fill);
    }
    let base = container(scroll).padding(Padding::ZERO.bottom(FOOTER)).width(Length::Fill);
    let footer = column![rule::horizontal(1).style(style::rule), container(actions).center_y(Length::Fill)]
        .spacing(space::L)
        .height(FOOTER - space::XL);
    let footer = container(footer).padding([0.0, space::XL]).align_bottom(Length::Fill).width(Length::Fill);
    let footer = container(footer).padding(Padding::ZERO.bottom(space::XL)).height(Length::Fill);
    let content = stack![base, footer].width(Length::Fill);
    match frame {
        Frame::Dialog => container(content).width(Length::Fill).style(style::container::popover).into(),
        Frame::Window => container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|t: &Theme| container::Style {
                background: Some(Background::Color(palette(t).menu)),
                ..container::Style::default()
            })
            .into(),
    }
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
        Collection { id: id.into(), name: name.into(), kind: CollectionKind::Events, color: None, writable: true }
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
        assert!(matches!(editor.update(Message::Save), Action::Focus(TITLE_ID)));

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
    fn errors_wait_for_the_first_attempt_to_save() {
        let mut editor =
            Editor::new_event(&calendars(), day(2026, 10, 11), Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap()));
        // An empty title is a problem, but nobody has asked to save yet.
        assert_eq!(problem(&editor), Problem::TitleEmpty);
        assert_eq!(editor.shown_problem(), None);
        editor.update(Message::StartTime("25:00".into()));
        assert_eq!(editor.shown_problem(), None, "typing in another field does not reveal anything either");

        // Save stays possible: it shows the first problem and asks for the focus on its field.
        assert!(matches!(editor.update(Message::Save), Action::Focus(TITLE_ID)));
        assert_eq!(editor.shown_problem(), Some(Problem::TitleEmpty));

        // From then on the message follows what is typed.
        set_title(&mut editor, "Call");
        assert_eq!(editor.shown_problem(), Some(Problem::StartTime));
        assert!(matches!(editor.update(Message::Save), Action::Focus(START_TIME_ID)));
        editor.update(Message::StartTime("14:00".into()));
        assert_eq!(editor.shown_problem(), None);
        assert!(matches!(editor.update(Message::Save), Action::Save(_)));
    }

    #[test]
    fn a_problem_in_the_calendar_shows_without_a_focus_target() {
        let mut editor = Editor::new_event(&[], day(2026, 10, 11), None);
        set_title(&mut editor, "Orphan");
        // Iced cannot focus a pick list: nothing to focus, but the problem is shown.
        assert!(matches!(editor.update(Message::Save), Action::None));
        assert_eq!(editor.shown_problem(), Some(Problem::ChooseCalendar));
    }

    #[test]
    fn every_problem_belongs_to_a_group_and_most_to_an_input() {
        let all = [
            (Problem::ChooseCalendar, Field::Calendar, None),
            (Problem::CalendarMove, Field::Calendar, None),
            (Problem::TitleEmpty, Field::Title, Some(TITLE_ID)),
            (Problem::StartDate, Field::Start, Some(START_DATE_ID)),
            (Problem::StartTime, Field::Start, Some(START_TIME_ID)),
            (Problem::EndDate, Field::End, Some(END_DATE_ID)),
            (Problem::EndTime, Field::End, Some(END_TIME_ID)),
            (Problem::EndBeforeStart, Field::End, Some(END_TIME_ID)),
            (Problem::EndDateBeforeStart, Field::End, Some(END_DATE_ID)),
        ];
        for (problem, field, input) in all {
            assert_eq!(problem.field(), field, "{problem:?}");
            assert_eq!(problem.input(), input, "{problem:?}");
        }
    }

    #[test]
    fn form_view_builds_with_problems_shown() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        editor.update(Message::Save);
        assert!(editor.attempted);
        let _view: Element<'_, Message> = editor.view();
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
        editor.update(Message::Calendar(CalendarChoice { id: "home".into(), name: "Home".into(), color: None }));
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
        // The narrow layout fills the window with the same editor (K.19).
        let _view: Element<'_, Message> = editor.view_full_window();
    }

    #[test]
    fn read_only_calendars_are_not_offered_and_their_events_open_read_only() {
        let birthdays = Collection { writable: false, ..calendar("birthdays", "Birthdays") };
        let calendars = vec![calendar("work", "Work"), birthdays];
        let new = Editor::new_event(&calendars, day(2026, 10, 12), None);
        assert_eq!(new.choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["work"]);
        let start = EventTime::Date(day(2026, 10, 12));
        let mut event = stored("Anna", start, Some(EventTime::Date(day(2026, 10, 13))));
        event.collection = "birthdays".into();
        let mut editor = Editor::edit(&event, &calendars);
        assert_eq!(editor.read_only, Some("This calendar is read-only"));
        assert!(matches!(editor.update(Message::Delete), Action::None));
        assert!(matches!(editor.update(Message::Save), Action::None));
    }
}
