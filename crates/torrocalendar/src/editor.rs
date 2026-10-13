//! Creating and editing an event (task K.12 in `docs/tasks/kalender-aufgaben.md`), shown in the
//! side panel since task U.4 in `docs/tasks/design.md`.
//!
//! The editor is the content of a `torro_gui::panel` ([`Editor::panel`]), laid out like the task
//! detail: the path "● Work › Wed, 7 Oct", the title as a large borderless field (`HEADLINE`,
//! wraps), then the properties as rows – "When" (start and end with date and time side by side,
//! all day), "Calendar", "Location" – and the notes as a quiet field. The footer holds "Delete
//! event" (`danger`, asks once). The app places the panel beside the calendar (wide) or over it
//! (medium, narrow); see `torro_gui::panel::place`.
//!
//! Saving works like the task detail: there is no Save button. A change of a property (all day,
//! calendar) is saved at once; typing is saved a moment after it stops (the app sends
//! [`Message::Flush`] while [`Editor::wants_autosave`] holds), on Enter in a field and when the
//! panel is left ([`Editor::leave`]). Every save is the same [`Action::Save`] the dialog sent
//! before (the core's `save_event`, which checks the etag). Only one save is under way at a time:
//! the next one waits for the answer and its new etag ([`Editor::stored`]). A new event is a draft
//! until its first valid state (it needs a title); that state is saved and creates it, and from
//! then on it is edited like any other.
//!
//! Errors stay hidden until Enter asks for a save. Then the field that is wrong is marked, the
//! message stands under it and the app gets an [`Action::Focus`] for it; from then on the message
//! follows the input live. (Iced 0.14 reports no "field left" event, so leaving a field alone does
//! not validate it.) A state that cannot be saved is not saved, also not when the panel is left.
//!
//! Recurring occurrences (`event.recurring`) and events of read-only calendars show the same panel
//! with plain values instead of fields, a note why, and no footer.
//!
//! The form works on strings, as the user typed them. [`Form::build`] turns it into an
//! [`Event`]: all-day events store an exclusive end date, so the form's inclusive end date
//! gets one day added on save and one day taken off when an event is shown.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, svg, text, text_editor, text_input, toggler};
use iced::{Alignment, Background, Border, Color, Element, Padding, Theme, keyboard};
use torro_core::i18n::tr;
use torro_gui::datetime::{format_date, format_time, parse_date, parse_time, to_local, to_utc};
use torro_gui::icon::{self, Icon, icon};
use torro_gui::panel::{self, Panel};
use torro_gui::theme::{self, palette, space};
use torro_gui::typo::text_width;
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, collection_color};
use torro_gui::{style, typo};
use torro_pim::{Collection, Event, EventTime};

use crate::views::short_date;

/// Id of the title field: the app focuses it when a new event opens ([`Action::Focus`] does the
/// same for a field with a problem).
pub const TITLE_ID: &str = "editor-title";
/// Ids of the inputs that [`Action::Focus`] can name.
const START_DATE_ID: &str = "editor-start-date";
const START_TIME_ID: &str = "editor-start-time";
const END_DATE_ID: &str = "editor-end-date";
const END_TIME_ID: &str = "editor-end-time";

/// Start and end time shown for an all-day event, so that switching it to timed gives
/// sensible values.
const DEFAULT_START: &str = "09:00";
const DEFAULT_END: &str = "10:00";

/// Why an event can only be looked at.
const RECURRING: &str = "Recurring events cannot be edited yet";
const READ_ONLY_CALENDAR: &str = "This calendar is read-only";

/// The editor's state: the event as stored, the form as typed, and where saving stands.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The event as last stored; the base of the next save (keeps uid, href and etag). `None`
    /// while a new event is a draft that has not been created yet.
    original: Option<Event>,
    /// The calendars to pick from.
    choices: Vec<CalendarChoice>,
    /// Why the event can only be looked at (recurring, read-only calendar); `None` = editable.
    read_only: Option<&'static str>,
    /// Today, for dates typed without a year.
    today: NaiveDate,
    /// The day the editor opened on, for the path while the start date cannot be read.
    day: NaiveDate,
    form: Form,
    /// The form as it was last stored (or opened): only a form that differs from it is saved, so
    /// opening an event never writes it back.
    base: Form,
    /// The form handed out with the save under way, or with the last one that failed: it is not
    /// sent again until something changes.
    sent: Option<Form>,
    /// A save is under way; the next one waits for its answer (new etag).
    saving: bool,
    /// The title as an editor widget; `form.title` mirrors its text on one line.
    title: text_editor::Content,
    /// The notes as an editor widget; `form.notes` mirrors its text.
    notes: text_editor::Content,
    /// The calendar menu is open (only drafts can choose their calendar).
    menu: bool,
    /// Set by the first press on "Delete event"; the second press confirms.
    confirm_delete: bool,
    /// Enter asked for a save once: from then on the first problem is shown at its field.
    attempted: bool,
}

/// A calendar in the calendar menu: the id that goes into the event, the name that is shown.
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
    Title(text_editor::Action),
    /// A calendar picked in the calendar menu.
    Calendar(CalendarChoice),
    /// The calendar row: open the calendar menu, or close it.
    ToggleCalendars,
    /// Close the calendar menu (click beside it).
    CloseMenu,
    AllDay(bool),
    StartDate(String),
    StartTime(String),
    EndDate(String),
    EndTime(String),
    Location(String),
    Notes(text_editor::Action),
    /// Enter in a field: save now, and show what is missing if that is not possible.
    Save,
    /// Save what changed, quietly (the autosave tick, Escape in a field).
    Flush,
    /// "Delete event": ask first.
    Delete,
    ConfirmDelete,
    CancelDelete,
    /// The panel's ✕ or a click beside it.
    Close,
    /// Escape: closes the innermost open thing (calendar menu, delete question, then the panel).
    Escape,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    /// Save this event (the core's `save_event`); report the answer with [`Editor::stored`] or
    /// [`Editor::save_failed`].
    Save(Event),
    Delete(Event),
    /// Close the panel; the app saves [`Editor::leave`] first.
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

    /// The input to mark and focus. The calendar is a menu, which has no focus to give.
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
        let start = local_start(event);
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
            transparency: torro_pim::Transparency::Opaque,
        });
        event.summary = title.to_string();
        event.start = start;
        event.end = Some(end);
        event.location = non_empty(&self.location);
        event.description = non_empty(&self.notes);
        Ok(event)
    }
}

/// The local start of `event`; an all-day event starts at midnight.
fn local_start(event: &Event) -> NaiveDateTime {
    match event.start {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => to_local(at),
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
    /// A draft for a new event on `date` at `time` (one hour long), or all-day with `None`, in the
    /// first writable one of `calendars`. Nothing is stored until it has a title.
    pub fn new_event(calendars: &[Collection], date: NaiveDate, time: Option<NaiveTime>) -> Editor {
        let choices = calendar_choices(calendars, None);
        let form = Form::for_new(date, time, choices.first().cloned());
        Editor::with(None, choices, None, date, form)
    }

    /// The editor of the stored `event`; `calendars` are all event calendars (the writable ones
    /// are offered, the event of a read-only one opens read-only, as does a recurring occurrence).
    pub fn edit(event: &Event, calendars: &[Collection]) -> Editor {
        let choices = calendar_choices(calendars, Some(&event.collection));
        let calendar = choices.iter().find(|c| c.id == event.collection).cloned();
        let form = Form::from_event(event, calendar);
        let read_only = if event.recurring {
            Some(RECURRING)
        } else if calendars.iter().any(|c| c.id == event.collection && !c.writable) {
            Some(READ_ONLY_CALENDAR)
        } else {
            None
        };
        Editor::with(Some(event.clone()), choices, read_only, local_start(event).date(), form)
    }

    fn with(
        original: Option<Event>,
        choices: Vec<CalendarChoice>,
        read_only: Option<&'static str>,
        day: NaiveDate,
        form: Form,
    ) -> Editor {
        Editor {
            original,
            choices,
            read_only,
            today: Local::now().date_naive(),
            day,
            title: text_editor::Content::with_text(&form.title),
            notes: text_editor::Content::with_text(&form.notes),
            base: form.clone(),
            form,
            sent: None,
            saving: false,
            menu: false,
            confirm_delete: false,
            attempted: false,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        // A series or a read-only calendar: the panel only closes.
        if self.read_only.is_some() {
            return match message {
                Message::Close | Message::Escape => Action::Close,
                _ => Action::None,
            };
        }
        match message {
            Message::Title(action) => {
                self.title.perform(action);
                // A title is one line; pasted line breaks become spaces.
                self.form.title = self.title.text().replace(['\r', '\n'], " ");
            }
            Message::Calendar(choice) => {
                self.menu = false;
                self.form.calendar = Some(choice);
                return self.request_save(false);
            }
            Message::ToggleCalendars => {
                self.confirm_delete = false;
                self.menu = !self.menu && self.can_choose_calendar();
            }
            Message::CloseMenu => self.menu = false,
            Message::AllDay(on) => {
                self.form.all_day = on;
                return self.request_save(false);
            }
            Message::StartDate(value) => self.form.start_date = value,
            Message::StartTime(value) => self.form.start_time = value,
            Message::EndDate(value) => self.form.end_date = value,
            Message::EndTime(value) => self.form.end_time = value,
            Message::Location(value) => self.form.location = value,
            Message::Notes(action) => {
                self.notes.perform(action);
                self.form.notes = self.notes.text();
            }
            Message::Save => return self.request_save(true),
            Message::Flush => return self.request_save(false),
            Message::Delete => {
                self.menu = false;
                self.confirm_delete = self.original.is_some();
            }
            Message::ConfirmDelete => {
                self.confirm_delete = false;
                if let Some(event) = &self.original {
                    return Action::Delete(event.clone());
                }
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::Close => return Action::Close,
            Message::Escape => {
                if self.menu {
                    self.menu = false;
                } else if self.confirm_delete {
                    self.confirm_delete = false;
                } else {
                    return Action::Close;
                }
            }
        }
        Action::None
    }

    /// The event as last stored: the one the panel shows, for marking it in the calendar. `None`
    /// while a new event is a draft.
    pub fn event(&self) -> Option<&Event> {
        self.original.as_ref()
    }

    /// Whether the app should send [`Message::Flush`] after a short pause: the form changed, it
    /// can be saved and no save is under way. The app subscribes to a timer only while this
    /// holds, so an idle panel costs nothing.
    pub fn wants_autosave(&self) -> bool {
        !self.saving && self.read_only.is_none() && self.changed() && self.check().is_ok()
    }

    /// The event to save when the panel is closed or replaced: the form if it changed, can be
    /// saved and no save is under way. A form that cannot be saved (no title, a date that cannot
    /// be read) is dropped, as Linear drops an empty title; so is a draft nobody named.
    pub fn leave(&self) -> Option<Event> {
        if self.wants_autosave() { self.check().ok() } else { None }
    }

    /// The app's answer to [`Action::Save`]: `saved` is the event as stored (with its uid, href
    /// and new etag; for a new event the first time). The form stays as typed – the user may have
    /// typed on – and what changed meanwhile is sent next.
    pub fn stored(&mut self, saved: &Event) -> Action {
        self.saving = false;
        self.original = Some(saved.clone());
        if let Some(sent) = self.sent.take() {
            self.base = sent;
        }
        self.request_save(false)
    }

    /// The app's answer to a failed [`Action::Save`]: the form stays, the next change tries again.
    pub fn save_failed(&mut self) {
        self.saving = false;
    }

    /// Whether `saved`, the answer to a save, belongs to this editor's save under way.
    pub fn awaits(&self, saved: &Event) -> bool {
        self.saving && self.original.as_ref().is_none_or(|event| event.uid == saved.uid)
    }

    /// The event the form describes today, or why it cannot be saved yet.
    fn check(&self) -> Result<Event, Problem> {
        self.form.build(self.today, self.original.as_ref())
    }

    /// Whether the form differs from what is stored and from what was last handed out.
    fn changed(&self) -> bool {
        self.form != self.base && self.sent.as_ref() != Some(&self.form)
    }

    /// A save if the form can and needs to be saved. `explicit` is Enter in a field: then a
    /// problem is shown from now on and its field gets the focus.
    fn request_save(&mut self, explicit: bool) -> Action {
        let event = match self.check() {
            Ok(event) => event,
            Err(problem) => {
                if !explicit {
                    return Action::None;
                }
                self.attempted = true;
                return problem.input().map_or(Action::None, Action::Focus);
            }
        };
        if self.saving || !self.changed() {
            return Action::None;
        }
        self.saving = true;
        self.sent = Some(self.form.clone());
        Action::Save(event)
    }

    /// The first problem, once Enter asked for a save; before that the form stays quiet.
    fn shown_problem(&self) -> Option<Problem> {
        if self.attempted { self.check().err() } else { None }
    }

    /// Only a draft chooses its calendar (moving a stored event is not supported yet), and only
    /// when there is more than one.
    fn can_choose_calendar(&self) -> bool {
        self.original.is_none() && self.choices.len() > 1
    }

    // -----------------------------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------------------------

    /// The side panel with this event, its messages wrapped with `wrap` (`Message::Editor`):
    /// path, title, the property sections and the footer. `anchor` is registered by the calendar
    /// row while its menu is open; pass the same one to [`Editor::layer`]. Hand the panel to
    /// `torro_gui::panel::place`.
    pub fn panel<'a, M: Clone + 'a>(&'a self, anchor: &Anchor, wrap: fn(Message) -> M) -> Panel<'a, M> {
        let (title, body) = match self.read_only {
            Some(reason) => (self.read_only_title(reason), self.read_only_body()),
            None => (self.title_view(), self.form_body(anchor)),
        };
        panel::panel(self.path().map(wrap), title.map(wrap), body.map(wrap), wrap(Message::Close))
            .footer(self.footer().into_iter().map(|element| element.map(wrap)).collect())
    }

    /// The open calendar menu as a floating layer under the calendar row (`anchor`), or `None`.
    /// Hand it to `torro_gui::panel::place` as its layer; a click beside it closes it.
    pub fn layer<'a, M: Clone + 'a>(&'a self, anchor: &Anchor, wrap: fn(Message) -> M) -> Option<Element<'a, M>> {
        if !self.menu {
            return None;
        }
        let items = self
            .choices
            .iter()
            .map(|choice| {
                MenuItem::new(Mark::Dot(choice_color(choice)), choice.name.as_str(), Message::Calendar(choice.clone()))
                    .current(self.form.calendar.as_ref() == Some(choice))
            })
            .collect();
        let menu = widgets::menu(vec![MenuSection::titled(tr("Calendar"), items)]);
        Some(widgets::floating(anchor, Placement::Below(Align::Start), menu, Message::CloseMenu).map(wrap))
    }

    /// "● Work › Wed, 7 Oct": the calendar with its colour and the day the event starts on.
    fn path(&self) -> Element<'_, Message> {
        let day = parse_date(&self.form.start_date, self.today).unwrap_or(self.day);
        match &self.form.calendar {
            Some(choice) => panel::path(Some(Mark::Dot(choice_color(choice))), [choice.name.clone(), short_date(day)]),
            None => panel::path(None, [short_date(day)]),
        }
    }

    /// The title as a large borderless field that wraps, and under it its problem; Enter saves,
    /// Escape leaves the field.
    fn title_view(&self) -> Element<'_, Message> {
        let field = text_editor(&self.title)
            .id(TITLE_ID)
            .placeholder(tr("Event title"))
            .on_action(Message::Title)
            .key_binding(|press| match press.key.as_ref() {
                // Only while the field has focus: otherwise the keys belong to the window.
                _ if !is_focused(&press) => text_editor::Binding::from_key_press(press),
                keyboard::Key::Named(keyboard::key::Named::Enter) => Some(text_editor::Binding::Sequence(vec![
                    text_editor::Binding::Custom(Message::Save),
                    text_editor::Binding::Unfocus,
                ])),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(leave_field()),
                _ => text_editor::Binding::from_key_press(press),
            })
            .font(typo::headline().font)
            .size(typo::headline().size)
            .line_height(typo::headline().line_height)
            .padding(Padding::ZERO)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text));
        let mut title = column![field].spacing(space::s());
        if let Some(problem) = self.shown_problem().filter(|p| p.field() == Field::Title) {
            title = title.push(problem_line(problem));
        }
        title.into()
    }

    /// The editable properties: "When", "Calendar", "Location", "Notes".
    fn form_body(&self, anchor: &Anchor) -> Element<'_, Message> {
        let form = &self.form;
        let shown = self.shown_problem();
        let invalid = |input: &str| shown.is_some_and(|problem| problem.input() == Some(input));
        let problem_in = |field: Field| shown.filter(|problem| problem.field() == field);
        let width = label_width();

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
        let mut when = vec![labelled(width, tr("Start"), start, None)];
        when.extend(problem_in(Field::Start).map(|problem| field_problem(width, problem)));
        when.push(labelled(width, tr("End"), end, None));
        when.extend(problem_in(Field::End).map(|problem| field_problem(width, problem)));
        when.push(all_day_row(width, form.all_day, true));

        let mut calendar = vec![self.calendar_row(anchor)];
        calendar.extend(problem_in(Field::Calendar).map(|problem| field_problem(width, problem)));

        let location = text_input(tr("Add location"), &form.location)
            .on_input(Message::Location)
            .on_submit(Message::Save)
            .padding(Padding::ZERO)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .style(bare_input);
        let location = widgets::prop_row(Some(Mark::Icon(Icon::MapPin)), location, None);

        let notes = text_editor(&self.notes)
            .placeholder(tr("Add notes …"))
            .on_action(Message::Notes)
            .key_binding(|press| match press.key.as_ref() {
                _ if !is_focused(&press) => text_editor::Binding::from_key_press(press),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(leave_field()),
                _ => text_editor::Binding::from_key_press(press),
            })
            .font(typo::body().font)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .padding(Padding::ZERO)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text_2));

        column![
            widgets::prop_section(tr("When"), when),
            widgets::prop_section(tr("Calendar"), calendar),
            widgets::prop_section(tr("Location"), vec![location]),
            widgets::prop_section(tr("Notes"), vec![panel::inset(notes)]),
        ]
        .spacing(space::l())
        .into()
    }

    /// The calendar with its dot; a draft with more than one calendar opens the menu from here.
    fn calendar_row(&self, anchor: &Anchor) -> Element<'_, Message> {
        let on_press = self.can_choose_calendar().then_some(Message::ToggleCalendars);
        let row = match &self.form.calendar {
            Some(choice) => widgets::prop_row(None, calendar_value(choice), on_press),
            None => widgets::prop_row(
                Some(Mark::Icon(Icon::Calendar)),
                typo::text(typo::body(), tr("Choose a calendar")).style(theme::text_3),
                on_press,
            ),
        };
        if self.menu { widgets::anchored(anchor, row) } else { row }
    }

    /// The title of a series or of an event in a read-only calendar, with the reason under it.
    fn read_only_title(&self, reason: &'static str) -> Element<'_, Message> {
        let note = typo::text(typo::meta(), tr(reason)).style(theme::text_2);
        column![panel::title(self.form.title.as_str()), note].spacing(space::s()).into()
    }

    /// The same sections as the form, with the values as text; location and notes only when set.
    fn read_only_body(&self) -> Element<'_, Message> {
        let form = &self.form;
        let width = label_width();
        let at = |date: &str, time: &str| {
            if form.all_day { date.to_string() } else { format!("{date}  {time}") }
        };
        let when = vec![
            labelled(width, tr("Start"), value_text(at(&form.start_date, &form.start_time)), None),
            labelled(width, tr("End"), value_text(at(&form.end_date, &form.end_time)), None),
            all_day_row(width, form.all_day, false),
        ];
        let calendar = form.calendar.as_ref().map(|choice| widgets::prop_row(None, calendar_value(choice), None));
        let mut body = column![widgets::prop_section(tr("When"), when)].spacing(space::l());
        if let Some(calendar) = calendar {
            body = body.push(widgets::prop_section(tr("Calendar"), vec![calendar]));
        }
        if !form.location.trim().is_empty() {
            let place = typo::text(typo::body(), form.location.as_str()).wrapping(Wrapping::WordOrGlyph);
            let row = widgets::prop_row(Some(Mark::Icon(Icon::MapPin)), place, None);
            body = body.push(widgets::prop_section(tr("Location"), vec![row]));
        }
        if !form.notes.trim().is_empty() {
            let notes =
                typo::text(typo::body(), form.notes.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
            body = body.push(widgets::prop_section(tr("Notes"), vec![panel::inset(notes)]));
        }
        body.into()
    }

    /// "Delete event" for a stored event that can be edited; after the first press the question
    /// with Delete and Cancel. Drafts and read-only events have no footer.
    fn footer(&self) -> Vec<Element<'_, Message>> {
        if self.read_only.is_some() || self.original.is_none() {
            return Vec::new();
        }
        if !self.confirm_delete {
            return vec![widgets::text_button(None, tr("Delete event"), style::button::danger, Some(Message::Delete))];
        }
        vec![
            typo::text(typo::body(), tr("Really delete?")).style(theme::text_2).into(),
            widgets::text_button(None, tr("Delete"), style::button::danger, Some(Message::ConfirmDelete)),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelDelete)),
        ]
    }
}

/// The widest date and time the inputs have to show: figures are tabular, so any will do.
const DATE_SAMPLE: &str = "00.00.0000";
const TIME_SAMPLE: &str = "00:00";

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

    /// A 32-high quiet input in tabular figures, so that dates and times line up; Enter saves.
    fn view(self) -> text_input::TextInput<'a, Message> {
        text_input(self.placeholder, self.value)
            .id(self.id)
            .on_input(self.on_input)
            .on_submit(Message::Save)
            .padding(style::INPUT_PADDING)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().tabular().font)
            .style(quiet(self.invalid))
    }

    /// [`Input::view`] as wide as `sample` (or the placeholder, if wider) with the input's padding
    /// and a little room for the caret.
    fn view_sized(self, sample: &str) -> text_input::TextInput<'a, Message> {
        let level = typo::body().tabular();
        let text = text_width(sample, level).max(text_width(self.placeholder, level));
        let width = text + style::INPUT_PADDING.left + style::INPUT_PADDING.right + space::s();
        self.view().width(width.ceil())
    }
}

/// Date and time side by side, each as wide as its value, so they read as one moment ("05.10.2026
/// 10:00") rather than two columns. All-day events have no time.
fn moment<'a>(all_day: bool, date: Input<'a>, time: Input<'a>) -> Element<'a, Message> {
    let date = date.view_sized(DATE_SAMPLE);
    if all_day {
        return date.into();
    }
    row![date, time.view_sized(TIME_SAMPLE)].spacing(space::xs()).into()
}

/// The width of the label column in "When": the widest of its labels in this language.
fn label_width() -> f32 {
    [tr("Start"), tr("End"), tr("All day")]
        .into_iter()
        .map(|label| text_width(label, typo::body()))
        .fold(0.0, f32::max)
        .ceil()
}

/// A property row with a label in `text_3` in a column `width` wide and `value` beside it, as
/// Linear lists properties; with `on_press` the whole row is a button.
fn labelled<'a>(
    width: f32,
    label: &'static str,
    value: impl Into<Element<'a, Message>>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let label = container(typo::text(typo::body(), label).style(theme::text_3).wrapping(Wrapping::None)).width(width);
    widgets::prop_row(None, row![label, value.into()].spacing(space::xs()).align_y(Alignment::Center), on_press)
}

/// "All day" with its switch; the row toggles it too. Read-only: the switch is shown, disabled.
fn all_day_row<'a>(width: f32, on: bool, editable: bool) -> Element<'a, Message> {
    let mut switch = toggler(on).size(style::CHECK_SIZE).style(style::toggler);
    if editable {
        switch = switch.on_toggle(Message::AllDay);
    }
    // In line with the text of the inputs above it.
    let switch = container(switch).padding(Padding::ZERO.left(style::INPUT_PADDING.left));
    labelled(width, tr("All day"), switch, editable.then_some(Message::AllDay(!on)))
}

/// A value as plain text where the form has an input, starting where the input's text does.
fn value_text<'a>(value: String) -> Element<'a, Message> {
    container(typo::text(typo::body().tabular(), value).wrapping(Wrapping::None))
        .padding(Padding::ZERO.left(style::INPUT_PADDING.left))
        .into()
}

/// A problem under a "When" or calendar row, starting where the values of the rows start.
fn field_problem<'a>(width: f32, problem: Problem) -> Element<'a, Message> {
    let left = style::ITEM_PADDING.left + width + space::xs() + style::INPUT_PADDING.left;
    container(problem_line(problem)).padding(Padding::ZERO.left(left).bottom(space::xs())).into()
}

/// What is wrong, in the error colour with an icon: "(!) Enter a title".
fn problem_line<'a>(problem: Problem) -> Element<'a, Message> {
    let mark =
        icon(Icon::CircleAlert, icon::SIZE_SM).style(|theme, _| svg::Style { color: Some(palette(theme).error_text) });
    row![
        // The icon is centred on the first line of the message, which may wrap in a narrow window.
        container(mark).center_y(typo::meta().line_height.to_absolute(typo::meta().size)),
        typo::text(typo::meta(), problem.message())
            .style(|theme| text::Style { color: Some(palette(theme).error_text) }),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Start)
    .into()
}

/// The calendar's dot and name. The dot stands in a cell as wide as an icon, so the name lines up
/// with the values of the rows that have an icon (the location); a `Mark::Dot` would not.
fn calendar_value(choice: &CalendarChoice) -> Element<'_, Message> {
    let dot = container(widgets::color_dot(choice_color(choice), space::s())).center_x(icon::SIZE);
    row![dot, typo::text(typo::body(), choice.name.as_str())].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The colour of a calendar's dot.
fn choice_color(choice: &CalendarChoice) -> Color {
    collection_color(choice.color.as_deref())
}

/// The input style without frame or surface until it is hovered or focused, so dates and times
/// read as values like the other properties and still show where to type; the border is in the
/// error colour while the field has the problem (the message under it says what is wrong, so the
/// colour is never the only sign).
fn quiet(invalid: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let mut look = style::text_input(theme, status);
        if matches!(status, text_input::Status::Active) {
            look.background = Background::Color(Color::TRANSPARENT);
            look.border.color = Color::TRANSPARENT;
        }
        if invalid {
            look.border.color = palette(theme).error_text;
        }
        look
    }
}

/// A text input without frame or surface (the location): text where the property values are.
fn bare_input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        ..style::text_input(theme, status)
    }
}

/// A text editor without frame or surface (Linear's title and description): text in `value`,
/// placeholder `text_3`. Focus shows as the caret; the field sits where its text is.
fn bare_editor(theme: &Theme, status: text_editor::Status, value: Color) -> text_editor::Style {
    let base = style::text_editor(theme, status);
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: palette(theme).text_3,
        value,
        ..base
    }
}

/// Whether the editor that got `press` has the keyboard focus.
fn is_focused(press: &text_editor::KeyPress) -> bool {
    matches!(press.status, text_editor::Status::Focused { .. })
}

/// Escape in a text field: leave it and save what was typed.
fn leave_field() -> text_editor::Binding<Message> {
    text_editor::Binding::Sequence(vec![text_editor::Binding::Unfocus, text_editor::Binding::Custom(Message::Flush)])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
        Collection {
            id: id.into(),
            name: name.into(),
            kind: CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        }
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
            transparency: torro_pim::Transparency::Opaque,
        }
    }

    fn timed() -> Event {
        stored(
            "Review",
            EventTime::DateTime(to_utc(at(2026, 10, 11, 14, 30))),
            Some(EventTime::DateTime(to_utc(at(2026, 10, 11, 16, 0)))),
        )
    }

    fn set_title(editor: &mut Editor, title: &str) {
        editor.update(Message::Title(text_editor::Action::SelectAll));
        let paste = text_editor::Edit::Paste(Arc::new(title.to_string()));
        editor.update(Message::Title(text_editor::Action::Edit(paste)));
    }

    /// The event the form describes now.
    fn built(editor: &Editor) -> Event {
        editor.check().expect("the form should be valid")
    }

    /// The event an update hands out to be saved, or a panic when it saves nothing.
    fn saves(editor: &mut Editor, message: Message) -> Event {
        match editor.update(message) {
            Action::Save(event) => event,
            other => panic!("expected Save, got {other:?}"),
        }
    }

    /// The core's answer to a save: the event as stored, with a uid and href for a new one and a
    /// new etag.
    fn store(editor: &mut Editor, mut event: Event, etag: &str) -> Action {
        if event.uid.is_empty() {
            event.uid = "new-1".into();
            event.href = Some("/work/new-1.ics".into());
        }
        event.etag = Some(etag.into());
        editor.stored(&event)
    }

    fn problem(editor: &Editor) -> Problem {
        editor.check().expect_err("the form should not be valid")
    }

    #[test]
    fn new_timed_event_starts_at_the_given_time_and_lasts_an_hour() {
        let mut editor =
            Editor::new_event(&calendars(), day(2026, 10, 11), Some(NaiveTime::from_hms_opt(14, 30, 0).unwrap()));
        set_title(&mut editor, "  Dentist  ");
        let event = saves(&mut editor, Message::Save);

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
        let event = built(&editor);
        assert_eq!(event.start, EventTime::Date(day(2026, 10, 11)));
        assert_eq!(event.end, Some(EventTime::Date(day(2026, 10, 12))));

        // Covering three days: 11.10. to 13.10. inclusive ends at 14.10. exclusive.
        editor.update(Message::EndDate("13.10.2026".into()));
        assert_eq!(built(&editor).end, Some(EventTime::Date(day(2026, 10, 14))));
    }

    #[test]
    fn switching_to_timed_keeps_sensible_times_and_saves_at_once() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        set_title(&mut editor, "Walk");
        let event = saves(&mut editor, Message::AllDay(false));
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
        assert!(matches!(editor.update(Message::Flush), Action::None), "nor does the autosave tick");
        assert_eq!(editor.shown_problem(), None);

        // Enter shows the first problem and asks for the focus on its field.
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
        // A menu has no focus to give: nothing to focus, but the problem is shown.
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
        editor.update(Message::ToggleCalendars);
        assert!(!editor.menu, "a stored event offers no other calendar");
        editor.update(Message::Calendar(CalendarChoice { id: "home".into(), name: "Home".into(), color: None }));
        assert_eq!(problem(&editor), Problem::CalendarMove);
    }

    #[test]
    fn a_draft_picks_its_calendar_from_the_menu() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        let anchor = Anchor::new();
        assert!(editor.layer(&anchor, |m| m).is_none());
        editor.update(Message::ToggleCalendars);
        assert!(editor.menu && editor.layer(&anchor, |m| m).is_some());
        let home = editor.choices[1].clone();
        // Without a title the pick is kept but nothing is stored yet.
        assert!(matches!(editor.update(Message::Calendar(home)), Action::None));
        assert!(!editor.menu, "picking closes the menu");
        set_title(&mut editor, "Garden");
        assert_eq!(built(&editor).collection, "home");
    }

    #[test]
    fn prefill_from_a_timed_event_shows_local_times() {
        let editor = Editor::edit(&timed(), &calendars());
        assert_eq!(editor.form.title, "Review");
        assert_eq!(editor.title.text(), "Review");
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
    fn an_unchanged_event_keeps_its_identity_and_is_never_written_back() {
        let event = timed();
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(built(&editor), event);
        assert!(!editor.wants_autosave());
        assert!(matches!(editor.update(Message::Flush), Action::None));
        assert!(matches!(editor.update(Message::Save), Action::None), "Enter on an unchanged event saves nothing");
        assert!(editor.leave().is_none());

        let all_day = stored("Holiday", EventTime::Date(day(2026, 10, 11)), Some(EventTime::Date(day(2026, 10, 14))));
        assert_eq!(built(&Editor::edit(&all_day, &calendars())), all_day);
        // An event without an end would gain one when built; opening it still writes nothing.
        let open_ended = stored("Call", EventTime::DateTime(to_utc(at(2026, 10, 11, 9, 0))), None);
        assert!(!Editor::edit(&open_ended, &calendars()).wants_autosave());
    }

    #[test]
    fn clearing_location_and_notes_stores_none() {
        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let mut editor = Editor::edit(&event, &calendars());
        editor.update(Message::Location("   ".into()));
        editor.update(Message::Notes(text_editor::Action::SelectAll));
        editor.update(Message::Notes(text_editor::Action::Edit(text_editor::Edit::Delete)));
        assert!(editor.wants_autosave());
        let saved = saves(&mut editor, Message::Flush);
        assert_eq!(saved.location, None);
        assert_eq!(saved.description, None);
        assert_eq!(saved.end, Some(EventTime::Date(day(2026, 10, 12))));
        assert_eq!(saved.etag.as_deref(), Some("\"7\""), "the etag the server checks");
    }

    #[test]
    fn typing_waits_for_the_autosave_tick() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Location("Room 4".into()));
        assert!(editor.wants_autosave());
        assert_eq!(saves(&mut editor, Message::Flush).location.as_deref(), Some("Room 4"));
        assert!(!editor.wants_autosave(), "nothing more while the save is under way");
    }

    #[test]
    fn one_save_at_a_time_and_the_next_carries_the_new_etag() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Location("Room 4".into()));
        let first = saves(&mut editor, Message::Flush);
        // Typed while the first save is under way: waits.
        set_title(&mut editor, "Review v2");
        assert!(matches!(editor.update(Message::Save), Action::None));
        assert!(editor.leave().is_none(), "leaving does not race the save under way");
        match store(&mut editor, first, "\"8\"") {
            Action::Save(next) => {
                assert_eq!(next.etag.as_deref(), Some("\"8\""));
                assert_eq!(next.summary, "Review v2");
                assert_eq!(next.location.as_deref(), Some("Room 4"));
                assert!(matches!(store(&mut editor, next, "\"9\""), Action::None), "nothing left to save");
            }
            other => panic!("expected the follow-up save, got {other:?}"),
        }
        assert_eq!(editor.title.text(), "Review v2", "the typed title stays in the field");
        assert!(!editor.wants_autosave());
    }

    #[test]
    fn a_draft_is_created_by_its_first_valid_state_and_then_updated() {
        let mut editor =
            Editor::new_event(&calendars(), day(2026, 10, 11), Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap()));
        assert!(editor.event().is_none());
        editor.update(Message::StartTime("15:00".into()));
        editor.update(Message::EndTime("16:00".into()));
        assert!(!editor.wants_autosave(), "without a title it stays a draft");
        assert!(editor.leave().is_none(), "and closing drops it");

        set_title(&mut editor, "Dentist");
        assert!(editor.wants_autosave());
        let created = saves(&mut editor, Message::Flush);
        assert!(created.href.is_none(), "the first save creates the event");
        assert!(editor.awaits(&Event { uid: "new-1".into(), ..created.clone() }), "a new event's answer is ours");
        assert!(matches!(store(&mut editor, created, "\"1\""), Action::None));
        assert_eq!(editor.event().and_then(|e| e.href.as_deref()), Some("/work/new-1.ics"));

        editor.update(Message::Location("Dr. Weber".into()));
        let update = saves(&mut editor, Message::Flush);
        assert_eq!((update.uid.as_str(), update.etag.as_deref()), ("new-1", Some("\"1\"")));
    }

    #[test]
    fn a_failed_save_waits_for_the_next_change() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Location("Room 4".into()));
        saves(&mut editor, Message::Flush);
        editor.save_failed();
        assert!(!editor.wants_autosave(), "the same state is not sent again and again");
        editor.update(Message::Location("Room 5".into()));
        assert_eq!(saves(&mut editor, Message::Flush).location.as_deref(), Some("Room 5"));
    }

    #[test]
    fn answers_for_other_events_are_not_ours() {
        let mut editor = Editor::edit(&timed(), &calendars());
        let other = Event { uid: "other".into(), ..timed() };
        assert!(!editor.awaits(&timed()), "no save is under way");
        editor.update(Message::Location("Room 4".into()));
        saves(&mut editor, Message::Flush);
        assert!(editor.awaits(&timed()));
        assert!(!editor.awaits(&other));
    }

    #[test]
    fn leaving_saves_what_is_pending_and_drops_what_cannot_be_saved() {
        let mut editor = Editor::edit(&timed(), &calendars());
        assert!(editor.leave().is_none());
        editor.update(Message::Location("Room 4".into()));
        assert_eq!(editor.leave().and_then(|e| e.location), Some("Room 4".into()));
        editor.update(Message::StartTime("25:00".into()));
        assert!(editor.leave().is_none());
    }

    #[test]
    fn delete_asks_once_and_only_for_stored_events() {
        let mut new = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        new.update(Message::Delete);
        assert!(!new.confirm_delete, "a draft has nothing to delete");
        assert!(matches!(new.update(Message::ConfirmDelete), Action::None));
        assert!(new.footer().is_empty());

        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let mut existing = Editor::edit(&event, &calendars());
        assert_eq!(existing.footer().len(), 1, "Delete event");
        assert!(matches!(existing.update(Message::Delete), Action::None));
        assert!(existing.confirm_delete);
        assert_eq!(existing.footer().len(), 3, "the question, Delete and Cancel");
        existing.update(Message::CancelDelete);
        assert!(!existing.confirm_delete);
        existing.update(Message::Delete);
        match existing.update(Message::ConfirmDelete) {
            Action::Delete(deleted) => assert_eq!(deleted, event),
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn escape_closes_menu_then_question_then_panel() {
        let mut editor = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        editor.update(Message::ToggleCalendars);
        assert!(matches!(editor.update(Message::Escape), Action::None));
        assert!(!editor.menu);
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Delete);
        assert!(matches!(editor.update(Message::Escape), Action::None));
        assert!(!editor.confirm_delete);
        assert!(matches!(editor.update(Message::Escape), Action::Close));
        assert!(matches!(editor.update(Message::Close), Action::Close));
    }

    #[test]
    fn recurring_occurrences_are_read_only() {
        let mut event = stored("Standup", EventTime::DateTime(to_utc(at(2026, 10, 11, 9, 0))), None);
        event.recurring = true;
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.read_only, Some(RECURRING));
        set_title(&mut editor, "Changed");
        assert_eq!(editor.form.title, "Standup", "nothing can be typed");
        assert!(matches!(editor.update(Message::Save), Action::None));
        assert!(matches!(editor.update(Message::Delete), Action::None));
        assert!(editor.footer().is_empty());
        assert!(!editor.wants_autosave() && editor.leave().is_none());
        assert!(matches!(editor.update(Message::Escape), Action::Close));
        let _ = editor.panel(&Anchor::new(), |m| m);
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
        assert_eq!(editor.read_only, Some(READ_ONLY_CALENDAR));
        assert!(matches!(editor.update(Message::Delete), Action::None));
        assert!(matches!(editor.update(Message::Save), Action::None));
    }

    #[test]
    fn the_panel_builds_for_drafts_stored_events_and_with_problems() {
        let anchor = Anchor::new();
        let mut draft = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        draft.update(Message::Save);
        assert!(draft.attempted);
        let _ = draft.panel(&anchor, |m| m);
        draft.update(Message::ToggleCalendars);
        let _layer: Option<Element<'_, Message>> = draft.layer(&anchor, |m| m);

        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Delete);
        let _ = editor.panel(&anchor, |m| m);
        // The path names the day the start field holds, or the day it opened on.
        editor.update(Message::StartDate("nonsense".into()));
        let _path = editor.path();
        let _ = Editor::new_event(&[], day(2026, 10, 11), None).panel(&anchor, |m| m);
    }

    #[test]
    fn the_label_column_fits_every_label() {
        let width = label_width();
        for label in [tr("Start"), tr("End"), tr("All day")] {
            assert!(text_width(label, typo::body()) <= width, "{label}");
        }
    }
}
