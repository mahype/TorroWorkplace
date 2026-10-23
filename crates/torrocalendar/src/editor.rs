//! Creating and editing an event (task K.12 in `docs/tasks/kalender-aufgaben.md`), shown in the
//! side panel since task U.4 in `docs/tasks/design.md`, with the fields, the repeat and the scope of
//! a change since task TE.7 (`docs/termin-editor.md`, layout A).
//!
//! The editor is the content of a `torro_gui::panel` ([`Editor::panel`]), laid out like Linear's
//! properties: the path "● Work › Wed, 7 Oct", the title as a large borderless field (`HEADLINE`,
//! wraps), then one row per property with its name in front (`widgets::prop_line`), in groups
//! divided by a line – Start, End, All day, Repeat, Time zone, Calendar, Show as, Status | Location,
//! Link | Reminders, People | Notes – then the links found. The footer holds "Preview reminder" and
//! "Delete event" (`danger`, asks once; for a series with three buttons, one per scope). The app
//! places the panel beside the calendar (wide) or over it (medium, narrow); see
//! `torro_gui::panel::place`. Repeat, time zone, "Show as" and status are chosen in menus under their
//! row ([`Editor::layer`]); "Custom…" of the repeat opens the card "Custom repeat" ([`repeat`]).
//!
//! Saving works like the task detail: there is no Save button. A change of a property (all day,
//! calendar, repeat …) is saved at once; typing is saved a moment after it stops (the app sends
//! [`Message::Flush`] while [`Editor::wants_autosave`] holds), on Enter in a field and when the
//! panel is left ([`Editor::leave`]). Every save is an [`Action::Save`] (the core's `save_event`,
//! which checks the etag). Only one save is under way at a time: the next one waits for the answer
//! and its new etag ([`Editor::stored`]). A new event is a draft until its first valid state (it
//! needs a title); that state is saved and creates it, and from then on it is edited like any other.
//!
//! **Series** (task TE.7, decisions 3 to 6 of the concept): an occurrence shows the band "Changes
//! apply to: This event · Following · All" under the head (Outlook: the scope is chosen before the
//! change); every save of it carries that scope, "This event" at first. The repeat can only change
//! for following or all events (the row is off with a tooltip for "This event"), and only all events
//! can end the series ("Never"). Changing time, length or rule of "Following" resets the changed
//! occurrences after this one; with "All", a new rule or a move to other days resets those the new
//! series no longer has. The band says how many before ([`Editor::resets`], the condition of
//! `series::resets` in the CalDAV crate, counted from `Event::exceptions`) and, after the save, the
//! number the core reports. A new start date for following or all events moves a rule whose days
//! came from the old one (weekly on its weekday, monthly on its day or n-th weekday, yearly on its
//! date), as Google and Apple do; a rule on days of its own stays and the band warns before the save
//! when the new date is none of them. A series cannot switch between all day and timed for following
//! or all events (the core answers `unsupported`), so the switch is off there. An occurrence whose
//! rule could not be read (`recurring` without `repeat`) changes only alone: "Following" and "All"
//! are off with the reason as tooltip, so a save never ends the series without a word.
//!
//! Errors stay hidden until Enter asks for a save. Then the field that is wrong is marked, the
//! message stands under it and the app gets an [`Action::Focus`] for it; from then on the message
//! follows the input live. (Iced 0.14 reports no "field left" event, so leaving a field alone does
//! not validate it.) A state that cannot be saved is not saved, also not when the panel is left.
//!
//! **Reminders and place** (task TE.8): "Add reminder" opens the card of [`remind`] – chips for the
//! common leads and "Custom" (n minutes, hours, days or weeks before or after start or end, the day
//! before at a time, a fixed date), with how it shows. The location is searched only on Enter or
//! "Search" ([`Action::SearchPlaces`], the core's `search_places`), never while typing; the
//! suggestions ([`place`]) stand under the field with the service and the attribution, and picking
//! one sets location and coordinates (`Event::geo`), while typing on clears the coordinates (they
//! would no longer fit the text). With coordinates and maps on, the app fetches the small map the
//! editor asks for ([`Editor::map_wanted`]); "Open in Maps" works always ([`Action::OpenMaps`]). Maps
//! are off until a person turns them on in the control centre: then there is no search, and the first
//! Enter says so once, quietly.
//!
//! Events of read-only calendars show the same rows with plain values, a note why, and no footer.
//! Location and notes stay selectable there (read-only text editors: a selection and ⌘C / Ctrl+C
//! work, typing does not).
//!
//! Links in the location and the notes (issue #126, `torro_gui::links`) are listed under the notes
//! in "Links", each a row that opens it; a video call (Zoom, Teams, Meet, Talk …) comes first with
//! a "Join" button. Only `http`, `https` and `mailto` are opened ([`Action::OpenLink`]); the "Link"
//! of the event opens only `http` and `https`. "Copy notes" puts the whole notes on the clipboard
//! ([`Action::Copy`]). People (organiser and invitees) are only shown: the suite sends no
//! invitations, the server tells them about changes.
//!
//! The form works on strings, as the user typed them. [`Form::build`] turns it into an
//! [`Event`]: all-day events store an exclusive end date, so the form's inclusive end date
//! gets one day added on save and one day taken off when an event is shown. Times are on the wall
//! clock of the event's time zone ([`zone`]).

mod place;
mod remind;
mod repeat;
mod zone;

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, image, row, rule, scrollable, svg, text, text_editor, text_input, toggler};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme, keyboard};
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::Maps;
use torro_gui::datetime::{format_date, format_time, parse_date, parse_time};
use torro_gui::icon::{self, Icon, icon};
use torro_gui::links;
use torro_gui::panel::{self, Panel};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo::text_width;
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, collection_color};
use torro_gui::{style, typo};
use torro_ipc::Place;
use torro_ipc::pim::MAX_PLACE_QUERY_CHARS;
use torro_pim::model::{Attendee, EventStatus, Geo, PartStat, Repeat, SeriesScope};
use torro_pim::{Alarm, AlarmAnchor, AlarmStyle, AlarmTrigger, Collection, Event, EventTime, Transparency};

use crate::views::{initials, short_date};

pub use place::{Failure as PlaceFailure, MAP_HEIGHT, MAP_WIDTH, demo_map, demo_places, openable_map};
pub use remind::Edit as RemindEdit;
pub use repeat::Edit as CustomEdit;

/// Id of the title field: the app focuses it when a new event opens ([`Action::Focus`] does the
/// same for a field with a problem).
pub const TITLE_ID: &str = "editor-title";
/// Ids of the inputs that [`Action::Focus`] can name.
const START_DATE_ID: &str = "editor-start-date";
const START_TIME_ID: &str = "editor-start-time";
const END_DATE_ID: &str = "editor-end-date";
const END_TIME_ID: &str = "editor-end-time";
/// The search of the time zone menu, focused when it opens.
const ZONE_SEARCH_ID: &str = "editor-zone-search";

/// Start and end time shown for an all-day event, so that switching it to timed gives
/// sensible values.
const DEFAULT_START: &str = "09:00";
const DEFAULT_END: &str = "10:00";

/// Why an event can only be looked at.
const READ_ONLY_CALENDAR: &str = "This calendar is read-only";

/// Why an occurrence changes alone: its series rule could not be read (`Editor::rule_unknown`).
const UNKNOWN_RULE: &str = "The repeat of this series could not be read, so changes apply to this event only";

/// How many zones the search of the zone menu lists at most.
const ZONE_RESULTS: usize = 40;
/// How many avatars "People" shows before "+n".
const AVATARS: usize = 3;

/// The editor's state: the event as stored, the form as typed, and where saving stands.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The event as last stored; the base of the next save (keeps uid, href, etag and the key of an
    /// occurrence). `None` while a new event is a draft that has not been created yet.
    original: Option<Event>,
    /// The calendars to pick from.
    choices: Vec<CalendarChoice>,
    /// Why the event can only be looked at (a read-only calendar); `None` = editable.
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
    /// The band's scope when the last save went out: a save that fails takes the band back to it, so
    /// a change typed for "This event" is not sent again for "All" chosen meanwhile (the second
    /// review of PR #294).
    sent_scope: SeriesScope,
    /// A save is under way; the next one waits for its answer (new etag).
    saving: bool,
    /// The title as an editor widget; `form.title` mirrors its text on one line.
    title: text_editor::Content,
    /// The notes as an editor widget; `form.notes` mirrors its text.
    notes: text_editor::Content,
    /// The location as a read-only editor widget, so it can be selected in a read-only event (an
    /// editable one types into a `text_input`).
    location: text_editor::Content,
    /// "Copy notes" was pressed and nothing else since: the row says "Copied".
    copied: bool,
    /// The calendar menu is open (only drafts can choose their calendar).
    menu: bool,
    /// Set by the first press on "Delete event"; the second press confirms.
    confirm_delete: bool,
    /// Enter asked for a save once: from then on the first problem is shown at its field.
    attempted: bool,
    /// Which events of the series a change of an occurrence reaches; "This event" at first.
    scope: SeriesScope,
    /// The menu or card open under a property row (task TE.7).
    popup: Option<Popup>,
    /// What is typed in the search of the zone menu.
    zone_query: String,
    /// This computer's time zone, read once.
    machine_zone: Option<String>,
    /// How many changed occurrences the last save reset, as the core counted them.
    reset: u32,
    /// The rule [`Editor::follow_start`] moved with a new start date: while the form still holds it
    /// (or the stored one), the rule goes on following the date; a rule the person picked stays.
    followed: Option<Repeat>,
    /// The maps settings (task TE.8): off until the app hands them over ([`Editor::set_maps`]).
    maps: Maps,
    /// The query of the place search under way; its answer comes to [`Editor::places_found`].
    searched: Option<String>,
    /// The quiet line under the location: maps are off, the search failed.
    place_note: Option<PlaceNote>,
    /// "Turn on maps in Settings" was said once; it is not said again in this editor.
    told_maps_off: bool,
    /// The map shown under the location, when it is the map of `form.geo`.
    map: Option<MapPicture>,
    /// The map asked for and not answered yet: its point and whether it is dark.
    map_asked: Option<(Geo, bool)>,
    /// A point whose map could not be had; not asked for again in this editor.
    map_failed: Option<Geo>,
}

/// The quiet line under the location.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PlaceNote {
    /// Maps are off: "Turn on maps in Settings".
    MapsOff,
    /// The location is longer than a search may be ([`MAX_PLACE_QUERY_CHARS`]); nothing was sent.
    TooLong,
    /// The search failed: a translated sentence for why ([`PlaceFailure::message`]).
    Failed(PlaceFailure),
}

impl PlaceNote {
    /// The quiet line, translated.
    fn words(&self) -> String {
        match self {
            PlaceNote::MapsOff => tr("Turn on maps in Settings").to_string(),
            PlaceNote::TooLong => trf("A place search takes at most {n} characters", &[("n", &MAX_PLACE_QUERY_CHARS)]),
            PlaceNote::Failed(failure) => failure.message().to_string(),
        }
    }
}

/// The small map of a point, as the core drew it.
#[derive(Debug, Clone)]
struct MapPicture {
    geo: Geo,
    dark: bool,
    picture: image::Handle,
}

/// A menu or card under a property row.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Popup {
    /// The "Repeat" menu with the short list.
    Repeat,
    /// The card "Custom repeat", as typed.
    Custom(repeat::Custom),
    /// The time zone menu with its search.
    Zone,
    /// "Show as": busy or free.
    ShowAs,
    /// "Status": confirmed, tentative, cancelled.
    Status,
    /// The card "Add reminder", as typed (task TE.8).
    Remind(remind::Card),
    /// The suggestions of the place search for `query` (task TE.8).
    Places { query: String, places: Vec<Place> },
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
    /// The event's link as typed (task TE.7).
    Url(String),
    Notes(text_editor::Action),
    /// Selecting in the read-only location; edits are ignored.
    LocationText(text_editor::Action),
    /// A link of the "Links" section, "Join", or the event's link.
    OpenLink(String),
    /// "Copy notes": the whole notes to the clipboard.
    CopyNotes,
    /// "Add reminder": open its card, or close it (tasks ER2.3, TE.8).
    ToggleAlarms,
    /// A field of the card "Add reminder", or its Add and Cancel.
    Remind(RemindEdit),
    /// The ✕ of a reminder's chip.
    RemoveAlarm(usize),
    /// "Preview reminder": show the event's reminder now (`calendar.preview_reminder`).
    PreviewReminder,
    /// The band "Changes apply to" of an occurrence: which events the next changes reach.
    Scope(SeriesScope),
    /// The "Repeat" row: open its menu, or close it.
    ToggleRepeat,
    /// A rule from the "Repeat" menu; `None` is "Never" (only all events of a series can end it).
    PickRepeat(Option<Box<Repeat>>),
    /// "Custom…" in the "Repeat" menu: the card "Custom repeat".
    OpenCustom,
    /// A field of the card "Custom repeat", or its Done and Cancel.
    Custom(CustomEdit),
    /// The "Time zone" row: open its menu, or close it.
    ToggleZones,
    /// What is typed in the search of the zone menu.
    ZoneQuery(String),
    /// A zone picked in the menu, by its IANA name.
    PickZone(String),
    /// Enter in the search of the zone menu: the first zone it lists.
    ZoneFirst,
    /// The "Show as" row: open its menu, or close it.
    ToggleShowAs,
    /// Busy or free, from the "Show as" menu.
    ShowAs(Transparency),
    /// The "Status" row: open its menu, or close it.
    ToggleStatus,
    /// A status from the "Status" menu.
    Status(EventStatus),
    /// A click beside an open menu or card: close it.
    ClosePopup,
    /// Enter in a field: save now, and show what is missing if that is not possible.
    Save,
    /// Save what changed, quietly (the autosave tick, Escape in a field).
    Flush,
    /// "Delete event": ask first.
    Delete,
    /// The answer to the question: delete, for an occurrence with this scope (`None` for an event
    /// that is not part of a series).
    ConfirmDelete(Option<SeriesScope>),
    CancelDelete,
    /// The panel's ✕ or a click beside it.
    Close,
    /// Enter in the location or "Search": look the location up (task TE.8).
    SearchPlaces,
    /// A suggestion, by its place in the list shown.
    PickPlace(usize),
    /// "Use “…” as written": keep the text, without a map.
    AsWritten,
    /// "Open in Maps": the place in the maps app of the settings.
    OpenMaps,
    /// Escape: closes the innermost open thing (a menu or card, the delete question, then the
    /// panel).
    Escape,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    /// Save this event (the core's `save_event`) with this scope: `Some` for an occurrence of a
    /// series, as the band "Changes apply to" says, `None` for any other event. Report the answer
    /// with [`Editor::stored`] (the event as stored and the number of changed occurrences the core
    /// reset) or [`Editor::save_failed`].
    Save(Event, Option<SeriesScope>),
    /// Delete this event (the core's `delete_event`) with this scope: `Some` for an occurrence,
    /// `None` for any other event.
    Delete(Event, Option<SeriesScope>),
    /// Close the panel; the app saves [`Editor::leave`] first.
    Close,
    /// Give the keyboard focus to the input with this id (the first one with a problem, the search
    /// of the zone menu).
    Focus(&'static str),
    /// Open this address in the browser. It passed `torro_gui::links::openable`; check it again
    /// right before opening.
    OpenLink(String),
    /// Put this text on the clipboard.
    Copy(String),
    /// Show the reminder of the stored event at this `href` now (task ER2.3).
    Preview(String),
    /// Look this text up (the core's `search_places`, task TE.8; `near` none). Report the answer with
    /// [`Editor::places_found`] and the same text: `Ok` with the places, `Err(PlaceFailure::MapsOff)`
    /// for the code `unsupported`, `Err(PlaceFailure::Other(message))` for any other error.
    SearchPlaces(String),
    /// Open this place in a maps app (task TE.8): an `https` address or, on Linux, a `geo:` link.
    /// Check it with [`openable_map`] right before opening.
    OpenMaps(String),
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
    /// All events of a series, or the following ones, cannot switch between all day and timed.
    AllDaySeries,
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
            Problem::StartDate | Problem::StartTime | Problem::AllDaySeries => Field::Start,
            Problem::EndDate | Problem::EndTime | Problem::EndBeforeStart | Problem::EndDateBeforeStart => Field::End,
        }
    }

    /// The input to mark and focus. The calendar is a menu and "All day" a switch, which have no
    /// focus to give.
    fn input(self) -> Option<&'static str> {
        match self {
            Problem::ChooseCalendar | Problem::CalendarMove | Problem::AllDaySeries => None,
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
            Problem::AllDaySeries => tr("Only this event can switch between all day and timed"),
        }
    }
}

/// The form fields as the user sees them. Dates and times stay strings until saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    title: String,
    calendar: Option<CalendarChoice>,
    all_day: bool,
    /// On the wall clock of [`Form::time_zone`].
    start_date: String,
    start_time: String,
    /// Inclusive for all-day events: the last day the event covers.
    end_date: String,
    end_time: String,
    location: String,
    /// The coordinates of the location: set by picking a suggestion, cleared by typing (task TE.8).
    geo: Option<Geo>,
    notes: String,
    /// The event's own reminders (task ER2.3), in their order.
    alarms: Vec<Alarm>,
    /// The rule of the series; `None` for an event that does not repeat (task TE.7).
    repeat: Option<Repeat>,
    /// The IANA zone the times are in; `None` for this computer's zone.
    time_zone: Option<String>,
    /// The event's link, as typed.
    url: String,
    /// Busy or free.
    transparency: Transparency,
    /// Confirmed, tentative or cancelled; `None` when the event does not say.
    status: Option<EventStatus>,
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
            geo: None,
            notes: String::new(),
            alarms: Vec::new(),
            repeat: None,
            time_zone: None,
            url: String::new(),
            transparency: Transparency::Opaque,
            status: None,
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

    /// The form for an existing event, on the wall clock of its zone. An event without an end gets
    /// one hour; an all-day end date is shown inclusive.
    pub fn from_event(event: &Event, calendar: Option<CalendarChoice>) -> Form {
        let zone = event.time_zone.as_deref();
        let start = wall_start(event);
        let end = if event.all_day() {
            let last = match event.end {
                Some(EventTime::Date(day)) => day - Duration::days(1),
                Some(EventTime::DateTime(at)) => zone::wall(at, zone).date(),
                None => start.date(),
            };
            last.max(start.date()).and_time(NaiveTime::MIN)
        } else {
            match event.end {
                Some(EventTime::DateTime(at)) => zone::wall(at, zone),
                _ => start + Duration::hours(1),
            }
        };
        let mut form = Form::from_span(event.all_day(), start, end, calendar);
        form.title = event.summary.clone();
        form.location = event.location.clone().unwrap_or_default();
        form.geo = event.geo;
        form.notes = event.description.clone().unwrap_or_default();
        form.alarms = event.alarms.clone();
        form.repeat = event.repeat.as_deref().cloned();
        form.time_zone = event.time_zone.clone();
        form.url = event.url.clone().unwrap_or_default();
        form.transparency = event.transparency;
        form.status = event.status;
        form
    }

    /// The event this form describes. `original` is the event being edited: its uid, href,
    /// etag, key of the occurrence and what the form does not show (people) are kept. A form
    /// without a rule for an event that had one ends the series: `recurring` goes false with it.
    /// `today` resolves dates typed without a year.
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
        let zone = self.time_zone.as_deref();
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
            (EventTime::DateTime(zone::instant(start, zone)), EventTime::DateTime(zone::instant(end, zone)))
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
        });
        event.summary = title.to_string();
        event.start = start;
        event.end = Some(end);
        event.location = non_empty(&self.location);
        event.geo = self.geo;
        event.description = non_empty(&self.notes);
        event.alarms = self.alarms.clone();
        event.repeat = self.repeat.clone().map(Box::new);
        // Ending a series ("Never" for all events) says so twice: no rule and no longer recurring.
        // The core ends a series only then; a missing rule alone keeps it (an occurrence whose rule
        // it could not read, `Editor::rule_unknown`).
        if self.repeat.is_none() && original.is_some_and(|event| event.repeat.is_some()) {
            event.recurring = false;
        }
        event.time_zone = self.time_zone.clone();
        event.url = non_empty(&self.url);
        event.transparency = self.transparency;
        event.status = self.status;
        Ok(event)
    }
}

/// The start of `event` on the wall clock of its zone; an all-day event starts at midnight.
fn wall_start(event: &Event) -> NaiveDateTime {
    wall_of(event.start, event.time_zone.as_deref())
}

/// An event time on the wall clock of `zone`; a day at its midnight.
fn wall_of(time: EventTime, zone: Option<&str>) -> NaiveDateTime {
    match time {
        EventTime::Date(day) => day.and_time(NaiveTime::MIN),
        EventTime::DateTime(at) => zone::wall(at, zone),
    }
}

/// The instant of an event time, a day at its midnight in UTC: for comparing keys of occurrences
/// as the core does.
fn instant_of(time: EventTime) -> DateTime<Utc> {
    match time {
        EventTime::DateTime(at) => at,
        EventTime::Date(day) => day.and_time(NaiveTime::MIN).and_utc(),
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Whether `url` is a web address the "Link" row may open (`http`, `https`; not `mailto`, which the
/// links in the notes may).
fn web_link(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && links::openable(url)
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
    /// are offered, the event of a read-only one opens read-only). An occurrence of a series opens
    /// with the scope "This event".
    pub fn edit(event: &Event, calendars: &[Collection]) -> Editor {
        let choices = calendar_choices(calendars, Some(&event.collection));
        let calendar = choices.iter().find(|c| c.id == event.collection).cloned();
        let form = Form::from_event(event, calendar);
        let read_only = calendars.iter().any(|c| c.id == event.collection && !c.writable).then_some(READ_ONLY_CALENDAR);
        Editor::with(Some(event.clone()), choices, read_only, wall_start(event).date(), form)
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
            location: text_editor::Content::with_text(&form.location),
            copied: false,
            base: form.clone(),
            form,
            sent: None,
            sent_scope: SeriesScope::Occurrence,
            saving: false,
            menu: false,
            confirm_delete: false,
            attempted: false,
            scope: SeriesScope::Occurrence,
            popup: None,
            zone_query: String::new(),
            machine_zone: zone::machine(),
            reset: 0,
            followed: None,
            maps: Maps::default(),
            searched: None,
            place_note: None,
            told_maps_off: false,
            map: None,
            map_asked: None,
            map_failed: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        self.copied = matches!(message, Message::CopyNotes);
        // Links and copying work everywhere, also in a read-only event.
        if let Message::OpenLink(url) = &message {
            return if links::openable(url) { Action::OpenLink(url.clone()) } else { Action::None };
        }
        if matches!(message, Message::CopyNotes) {
            return Action::Copy(self.form.notes.clone());
        }
        if matches!(message, Message::OpenMaps) {
            return Action::OpenMaps(place::maps_link(self.maps.open_with, &self.form.location, self.form.geo));
        }
        // A read-only calendar: location and notes can be selected, the panel closes.
        if self.read_only.is_some() {
            return match message {
                Message::Notes(action) if !action.is_edit() => {
                    self.notes.perform(action);
                    Action::None
                }
                Message::LocationText(action) if !action.is_edit() => {
                    self.location.perform(action);
                    Action::None
                }
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
                self.close_menus();
                self.menu = !self.menu && self.can_choose_calendar();
            }
            Message::CloseMenu => self.menu = false,
            Message::AllDay(on) => {
                if self.can_switch_all_day() {
                    self.form.all_day = on;
                    return self.request_save(false);
                }
            }
            Message::StartDate(value) => {
                self.form.start_date = value;
                self.follow_start();
            }
            Message::StartTime(value) => self.form.start_time = value,
            Message::EndDate(value) => self.form.end_date = value,
            Message::EndTime(value) => self.form.end_time = value,
            Message::Location(value) => self.type_location(value),
            Message::Url(value) => self.form.url = value,
            Message::Notes(action) => {
                self.notes.perform(action);
                self.form.notes = self.notes.text();
            }
            // Handled above; the editable location is a `text_input`.
            Message::OpenLink(_) | Message::CopyNotes | Message::LocationText(_) | Message::OpenMaps => {}
            Message::Save => return self.request_save(true),
            Message::Flush => return self.request_save(false),
            Message::Delete => {
                self.close_menus();
                self.confirm_delete = self.original.is_some();
            }
            Message::ConfirmDelete(scope) => {
                self.confirm_delete = false;
                if let Some(event) = &self.original {
                    let scope = if event.recurring { Some(scope.unwrap_or(SeriesScope::Occurrence)) } else { None };
                    return Action::Delete(event.clone(), scope);
                }
            }
            Message::CancelDelete => self.confirm_delete = false,
            Message::Close => return Action::Close,
            Message::ToggleAlarms => {
                let open = matches!(self.popup, Some(Popup::Remind(_)));
                self.close_menus();
                if !open {
                    self.popup = Some(Popup::Remind(remind::Card::new(self.start_date())));
                }
            }
            Message::Remind(edit) => return self.remind(edit),
            Message::SearchPlaces => return self.search(),
            Message::PickPlace(index) => return self.pick_place(index),
            Message::AsWritten => {
                self.popup = None;
                return self.request_save(false);
            }
            Message::RemoveAlarm(index) => {
                if index < self.form.alarms.len() {
                    self.form.alarms.remove(index);
                    return self.request_save(false);
                }
            }
            Message::PreviewReminder => {
                if let Some(href) = self.original.as_ref().and_then(|event| event.href.clone()) {
                    return Action::Preview(href);
                }
            }
            Message::Scope(scope) => return self.choose_scope(scope),
            Message::ToggleRepeat => self.toggle(Popup::Repeat, self.can_change_repeat()),
            Message::PickRepeat(choice) => {
                self.popup = None;
                return self.set_repeat(choice.map(|rule| *rule));
            }
            Message::OpenCustom => {
                if self.can_change_repeat() {
                    let card = repeat::Custom::new(self.form.repeat.as_ref(), self.start_date());
                    self.popup = Some(Popup::Custom(card));
                }
            }
            Message::Custom(edit) => return self.custom(edit),
            Message::ToggleZones => {
                self.zone_query.clear();
                self.toggle(Popup::Zone, true);
                if self.popup == Some(Popup::Zone) {
                    return Action::Focus(ZONE_SEARCH_ID);
                }
            }
            Message::ZoneQuery(query) => self.zone_query = query,
            Message::PickZone(name) => return self.set_zone(name),
            Message::ZoneFirst => {
                let first = zone::search(&self.zone_query, 1).first().map(|name| name.to_string());
                if let Some(name) = first.or_else(|| self.machine_zone.clone()) {
                    return self.set_zone(name);
                }
            }
            Message::ToggleShowAs => self.toggle(Popup::ShowAs, true),
            Message::ShowAs(transparency) => {
                self.popup = None;
                self.form.transparency = transparency;
                return self.request_save(false);
            }
            Message::ToggleStatus => self.toggle(Popup::Status, true),
            Message::Status(status) => {
                self.popup = None;
                self.form.status = Some(status);
                return self.request_save(false);
            }
            Message::ClosePopup => self.popup = None,
            Message::Escape => {
                if self.popup.is_some() {
                    self.popup = None;
                } else if self.menu {
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

    /// Closes every menu, card and the delete question.
    fn close_menus(&mut self) {
        self.popup = None;
        self.menu = false;
        self.confirm_delete = false;
    }

    /// Opens `popup` when `allowed` and it is not open, else closes it; other menus close.
    fn toggle(&mut self, popup: Popup, allowed: bool) {
        let open = self.popup.as_ref() == Some(&popup);
        self.close_menus();
        if allowed && !open {
            self.popup = Some(popup);
        }
    }

    /// The scope of the next changes. "This event" cannot carry a new rule: one chosen for following
    /// or all events but not saved yet goes back. "Never" belongs to "All" only ([`Editor::can_end_series`]):
    /// with another scope the stored rule comes back, so ending the series never goes out with
    /// "Following" (the second review of #278, finding 6). While a save that ends the series is under
    /// way the scope stays: the answer decides whether there is a series left. Following or all events
    /// take a new start date's days ([`Editor::follow_start`]). An occurrence whose rule could not be
    /// read stays with "This event" ([`Editor::rule_unknown`]).
    ///
    /// What was typed under the old scope and not saved yet is saved with the old scope first, so a
    /// title typed for "This event" does not land on the whole series because "All" was clicked
    /// before the save (the review of PR #294). While a save is under way such a change cannot go out
    /// yet, so the scope stays until the answer; a form that cannot be saved yet (a date half typed)
    /// keeps the scope too and shows its problem, as Enter does. A form whose save failed
    /// ([`Editor::save_failed`]) goes out again with the old scope, else the next change would carry
    /// it to the new one (the review of PR #296); a retry that ends the series keeps "All".
    fn choose_scope(&mut self, scope: SeriesScope) -> Action {
        if scope != SeriesScope::Occurrence && self.rule_unknown() {
            return Action::None;
        }
        if scope != self.scope && self.ending_series() {
            return Action::None;
        }
        let switching = scope != self.scope;
        let mut pending = Action::None;
        if switching && self.form != self.base {
            if self.saving {
                // Only what was typed after the save under way waits; the sent form is decided by
                // the answer, and a failure takes the band back ([`Editor::save_failed`]).
                if self.changed() {
                    return Action::None;
                }
            } else {
                if let Err(problem) = self.check() {
                    self.attempted = true;
                    return problem.input().map_or(Action::None, Action::Focus);
                }
                // Not saving, `sent` is the form of a failed save: it was never stored.
                self.sent = None;
                pending = self.request_save(false);
                if self.ending_series() {
                    return pending;
                }
            }
        }
        self.scope = scope;
        self.reset = 0;
        // The form is out with the old scope (now or before): `original` and `base` are the state
        // before that save, so adjusting the rule to them now would undo what was sent, and the next
        // save would carry it with the new scope (the second review of PR #294). The answer
        // ([`Editor::stored`]) brings the rule as stored.
        if self.saving {
            return pending;
        }
        if let Some(original) = &self.original
            && (scope == SeriesScope::Occurrence || (scope != SeriesScope::Series && self.form.repeat.is_none()))
        {
            self.form.repeat = original.repeat.as_deref().cloned();
        }
        self.follow_start();
        Action::None
    }

    /// A save is under way that ends the series ("Never" with "All").
    fn ending_series(&self) -> bool {
        self.saving
            && self.occurrence().is_some_and(|original| original.repeat.is_some())
            && self.sent.as_ref().is_some_and(|sent| sent.repeat.is_none())
    }

    /// With "Following" or "All", a rule whose days came from the stored start moves with a new start
    /// date, as in Google and Apple ([`repeat::follow`]): the stored rule, moved from the stored start
    /// date to the one typed. Only while the form holds the stored rule or one this moved itself – a
    /// rule the person picked stays. A rule on days of its own stays too; the band warns when the new
    /// date is not one of them ([`Editor::date_off_rule`]). A date that cannot be read changes nothing.
    fn follow_start(&mut self) {
        if self.occurrence().is_none() || self.scope == SeriesScope::Occurrence {
            return;
        }
        let (Some(new), Some(old)) =
            (parse_date(&self.form.start_date, self.today), parse_date(&self.base.start_date, self.today))
        else {
            return;
        };
        let Some(stored) = self.base.repeat.as_ref() else { return };
        let untouched =
            self.form.repeat.as_ref() == Some(stored) || (self.followed.is_some() && self.form.repeat == self.followed);
        if let Some(moved) = repeat::follow(stored, old, new).filter(|_| untouched) {
            self.followed = Some(moved.clone());
            self.form.repeat = Some(moved);
        }
    }

    /// Whether the band should warn before the save that the rule does not move with the date: with
    /// "Following" or "All", a start date other than the stored one that is not one of the days of
    /// the rule in the form (or a custom rule, which cannot tell).
    fn date_off_rule(&self) -> bool {
        if self.occurrence().is_none() || self.scope == SeriesScope::Occurrence {
            return false;
        }
        let (Some(new), Some(old)) =
            (parse_date(&self.form.start_date, self.today), parse_date(&self.base.start_date, self.today))
        else {
            return false;
        };
        new != old && self.form.repeat.as_ref().is_some_and(|rule| !repeat::fits(rule, new, new))
    }

    /// Whether the stored occurrence's series rule could not be read: `recurring` without `repeat`
    /// (the core knew no raw object for it). Changes then go to this event alone – a save with
    /// "Following" or "All" would carry no rule and end the series without a word.
    fn rule_unknown(&self) -> bool {
        self.occurrence().is_some_and(|event| event.repeat.is_none())
    }

    /// The rule `choice` from the menu or the card, with the end of the rule there is; saved at once
    /// when it differs. `None` ends the series, which only all its events can.
    fn set_repeat(&mut self, choice: Option<Repeat>) -> Action {
        if !self.can_change_repeat() || (choice.is_none() && !self.can_end_series()) {
            return Action::None;
        }
        let start = self.start_date();
        if repeat::same(choice.as_ref(), self.form.repeat.as_ref(), start) {
            return Action::None;
        }
        self.form.repeat = choice;
        self.request_save(false)
    }

    /// A field of the card "Custom repeat"; Done takes the rule when it can be read, Cancel leaves it.
    fn custom(&mut self, edit: CustomEdit) -> Action {
        let (start, today, all_day) = (self.start_date(), self.today, self.form.all_day);
        let zone = self.form.time_zone.clone();
        let until = move |day: NaiveDate| repeat::until_of(day, all_day, |wall| zone::instant(wall, zone.as_deref()));
        let Some(Popup::Custom(card)) = self.popup.as_mut() else { return Action::None };
        match edit {
            CustomEdit::Cancel => self.popup = None,
            CustomEdit::Done => {
                card.update(CustomEdit::Done);
                if let Ok(rule) = card.build(start, today, until) {
                    self.popup = None;
                    let current = self.form.repeat.as_ref().filter(|current| !current.custom);
                    // An unchanged rule keeps its stored text.
                    let rule = match current {
                        Some(current) if repeat::same(Some(current), Some(&rule), start) => current.clone(),
                        _ => rule,
                    };
                    return self.set_repeat(Some(rule));
                }
            }
            edit => card.update(edit),
        }
        Action::None
    }

    /// The zone `name` from the menu, saved at once when it differs. This computer's zone stays "no
    /// zone of its own" for an event that had none.
    fn set_zone(&mut self, name: String) -> Action {
        self.popup = None;
        let value = if self.base.time_zone.is_none() && self.machine_zone.as_deref() == Some(name.as_str()) {
            None
        } else {
            Some(name)
        };
        if value == self.form.time_zone {
            return Action::None;
        }
        self.form.time_zone = value;
        self.request_save(false)
    }

    /// A field of the card "Add reminder"; Add adds the reminder when it can be read (once) and saves,
    /// Cancel leaves the reminders. The card counts from the start on the wall clock of the event's
    /// zone ([`Editor::shown_zone`]).
    fn remind(&mut self, edit: RemindEdit) -> Action {
        let (start, today) = (self.start_wall(), self.today);
        let zone = self.shown_zone().map(str::to_string);
        let Some(Popup::Remind(card)) = self.popup.as_mut() else { return Action::None };
        match edit {
            RemindEdit::Cancel => self.popup = None,
            RemindEdit::Add => {
                card.update(RemindEdit::Add);
                if let Ok(alarm) = card.build(start, today, |wall| zone::instant(wall, zone.as_deref())) {
                    self.popup = None;
                    if !self.form.alarms.contains(&alarm) {
                        self.form.alarms.push(alarm);
                    }
                    return self.request_save(false);
                }
            }
            edit => card.update(edit),
        }
        Action::None
    }

    /// The location as typed. Text that differs drops the coordinates of a place picked before (they
    /// would no longer fit), the suggestions, a search under way and the quiet line.
    fn type_location(&mut self, value: String) {
        if value != self.form.location {
            self.form.geo = None;
            self.searched = None;
        }
        self.form.location = value;
        self.place_note = None;
        if matches!(self.popup, Some(Popup::Places { .. })) {
            self.popup = None;
        }
    }

    /// Enter in the location or "Search": with maps on, a search for the text when it has no place
    /// yet; otherwise a save as Enter does in the other fields. With maps off the first Enter says so,
    /// quietly, once.
    fn search(&mut self) -> Action {
        let query = self.form.location.trim().to_string();
        if query.is_empty() || self.form.geo.is_some() {
            return self.request_save(true);
        }
        if !self.maps.enabled {
            self.say_maps_off();
            return self.request_save(true);
        }
        // The core takes no longer search (`calendar.search_places`); say so here, in words.
        if query.chars().count() > MAX_PLACE_QUERY_CHARS {
            self.place_note = Some(PlaceNote::TooLong);
            return self.request_save(true);
        }
        self.close_menus();
        self.place_note = None;
        self.searched = Some(query.clone());
        Action::SearchPlaces(query)
    }

    /// "Turn on maps in Settings" under the location, the first time only.
    fn say_maps_off(&mut self) {
        if !self.told_maps_off {
            self.told_maps_off = true;
            self.place_note = Some(PlaceNote::MapsOff);
        }
    }

    /// The suggestion at `index`: its name and address become the location, its point the event's
    /// coordinates; saved at once.
    fn pick_place(&mut self, index: usize) -> Action {
        let Some(Popup::Places { places, .. }) = &self.popup else { return Action::None };
        let Some(place) = places.get(index).cloned() else { return Action::None };
        self.popup = None;
        let detail = place.detail.trim();
        let location = if detail.is_empty() { place.label.clone() } else { format!("{}, {detail}", place.label) };
        self.location = text_editor::Content::with_text(&location);
        self.form.location = location;
        self.form.geo = Some(place.geo);
        self.place_note = None;
        self.request_save(false)
    }

    /// The start as typed on the wall clock of the event's zone: midnight for an all-day event or a
    /// time that cannot be read, the day the editor opened on while the date cannot be read.
    fn start_wall(&self) -> NaiveDateTime {
        let time = if self.form.all_day { None } else { parse_time(&self.form.start_time) };
        self.start_date().and_time(time.unwrap_or(NaiveTime::MIN))
    }

    /// Takes the maps settings (`Settings::maps`, task TE.8): whether search and map are on, the
    /// search service's name for the suggestions and what "Open in Maps" opens. Call it when the
    /// editor opens and whenever the settings are read again; until then maps count as off. Turning
    /// maps off takes the map away.
    pub fn set_maps(&mut self, maps: Maps) {
        if !maps.enabled {
            self.map = None;
        }
        self.maps = maps;
    }

    /// The app's answer to [`Action::SearchPlaces`] for `query`: the suggestions open under the
    /// location (also when there are none: "No places found" and "Use … as written"). An answer to
    /// an older search, or for text typed over since, is dropped. `MapsOff` says once that maps are
    /// off; any other failure stands as a quiet line under the location, in a translated sentence
    /// ([`PlaceFailure::message`]), never in the core's English words.
    pub fn places_found(&mut self, query: &str, answer: Result<Vec<Place>, PlaceFailure>) {
        if self.searched.as_deref() != Some(query) {
            return;
        }
        self.searched = None;
        if self.form.location.trim() != query {
            return;
        }
        match answer {
            Ok(places) => {
                self.close_menus();
                self.popup = Some(Popup::Places { query: query.to_string(), places });
            }
            Err(PlaceFailure::MapsOff) => {
                self.maps.enabled = false;
                self.say_maps_off();
            }
            Err(failure) => self.place_note = Some(PlaceNote::Failed(failure)),
        }
    }

    /// The point whose map the app should fetch now (the core's `map_preview`, [`MAP_WIDTH`] ×
    /// [`MAP_HEIGHT`], `dark` as the window is drawn), or `None`. Ask after every update and answer:
    /// it names a point once – while maps are on, the form has coordinates a map can show, and there
    /// is no map of them in this darkness, none asked for and none that failed – and remembers that
    /// it was asked. Report the answer with [`Editor::map_loaded`].
    pub fn map_wanted(&mut self, dark: bool) -> Option<Geo> {
        let geo = self.form.geo.filter(|geo| self.maps.enabled && geo.on_map())?;
        let have = self.map.as_ref().is_some_and(|map| map.geo == geo && map.dark == dark);
        if have || self.map_failed == Some(geo) || self.map_asked == Some((geo, dark)) {
            return None;
        }
        self.map_asked = Some((geo, dark));
        Some(geo)
    }

    /// The app's answer to [`Editor::map_wanted`]: the map of `geo` drawn `dark` or not, `None` when
    /// it could not be had (the map then stays away for this point; address and "Open in Maps"
    /// remain). An answer nobody waits for any more is dropped.
    pub fn map_loaded(&mut self, geo: Geo, dark: bool, picture: Option<image::Handle>) {
        if self.map_asked != Some((geo, dark)) {
            return;
        }
        self.map_asked = None;
        match picture {
            Some(picture) => self.map = Some(MapPicture { geo, dark, picture }),
            None => self.map_failed = Some(geo),
        }
    }

    /// The map to show: the one of the form's coordinates, while maps are on.
    fn shown_map(&self) -> Option<&image::Handle> {
        let map = self.map.as_ref().filter(|map| self.maps.enabled && Some(map.geo) == self.form.geo)?;
        Some(&map.picture)
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

    /// The event to save, with its scope, when the panel is closed or replaced: the form if it
    /// changed, can be saved and no save is under way. A form that cannot be saved (no title, a date
    /// that cannot be read) is dropped, as Linear drops an empty title; so is a draft nobody named.
    pub fn leave(&self) -> Option<(Event, Option<SeriesScope>)> {
        if self.wants_autosave() { self.check().ok().map(|event| (event, self.save_scope())) } else { None }
    }

    /// The app's answer to [`Action::Save`]: `saved` is the event as stored (with its uid, href
    /// and new etag; for a new event the first time; after "Following" split a series, the first
    /// occurrence of the new one, with its new href), `reset` the number of changed occurrences the
    /// core reset (`Response::Event.reset`, 0 where it cannot tell). The form stays as typed – the
    /// user may have typed on – and what changed meanwhile is sent next. A rule the server writes its
    /// own way (the rest of a count after a split) is taken as stored unless it changed since.
    pub fn stored(&mut self, saved: &Event, reset: u32) -> Action {
        self.saving = false;
        self.reset = reset;
        if let Some(sent) = self.sent.take() {
            let rule = saved.repeat.as_deref().cloned();
            if self.form.repeat == sent.repeat {
                self.form.repeat = rule.clone();
            }
            self.base = Form { repeat: rule, ..sent };
        }
        self.original = Some(saved.clone());
        self.request_save(false)
    }

    /// The app's answer to a failed [`Action::Save`]: the form stays, the next change tries again,
    /// with the scope the failed save went out with ([`Editor::choose_scope`] switched the band while
    /// it was under way).
    pub fn save_failed(&mut self) {
        self.saving = false;
        if self.scope != self.sent_scope {
            self.scope = self.sent_scope;
            self.reset = 0;
        }
    }

    /// Whether `saved`, the answer to a save, belongs to this editor's save under way. A series
    /// split with "Following" answers with a new uid.
    pub fn awaits(&self, saved: &Event) -> bool {
        self.saving && self.original.as_ref().is_none_or(|event| event.uid == saved.uid || event.recurring)
    }

    /// How many changed occurrences the change in the form would reset, before it is saved, with the
    /// condition of `series::resets` in the CalDAV crate: with "Following", once time, length or rule
    /// differ from the stored occurrence, the changed occurrences after it; with "All", see
    /// [`Editor::resets_of_all`]. 0 otherwise. The core's own count comes back with the save
    /// ([`Editor::stored`]).
    pub fn resets(&self) -> u32 {
        let Some(original) = &self.original else { return 0 };
        let Some(key) = original.recurrence_id else { return 0 };
        match self.scope {
            SeriesScope::Occurrence => 0,
            SeriesScope::Following if self.changes_grid(original) => self.changed_after(original, key),
            SeriesScope::Following => 0,
            SeriesScope::Series => self.resets_of_all(original, key),
        }
    }

    /// What "All" resets: nothing while time, length and rule stay; with the rule gone, every changed
    /// occurrence but this one; else those whose day, moved as the series moves, is no longer one of
    /// the rule's days ([`repeat::fits`]; the core keeps those that still fall on an event of the new
    /// series). Only a new time or length on the same days resets nothing; a custom rule that changed
    /// or moves to other days counts them all, as nothing here can tell.
    fn resets_of_all(&self, original: &Event, key: EventTime) -> u32 {
        if !self.changes_grid(original) {
            return 0;
        }
        let others: Vec<EventTime> =
            original.exceptions.iter().copied().filter(|other| instant_of(*other) != instant_of(key)).collect();
        let Some(rule) = self.form.repeat.as_ref() else {
            return if original.repeat.is_some() { others.len() as u32 } else { 0 };
        };
        let Ok(event) = self.form.build(self.today, Some(original)) else { return 0 };
        let (from, to) = (wall_start(original), wall_start(&event));
        let same_rule = repeat::same(Some(rule), original.repeat.as_deref(), from.date());
        if same_rule && from.date() == to.date() {
            return 0;
        }
        let shift = to - from;
        let zone = original.time_zone.as_deref();
        others
            .into_iter()
            .filter(|other| !repeat::fits(rule, to.date(), (wall_of(*other, zone) + shift).date()))
            .count() as u32
    }

    /// The changed occurrences of `original`'s series after the one at `key`.
    fn changed_after(&self, original: &Event, key: EventTime) -> u32 {
        original.exceptions.iter().filter(|other| instant_of(**other) > instant_of(key)).count() as u32
    }

    /// Whether the form moves `original`, makes it longer or shorter, or repeats it otherwise. A form
    /// that cannot be saved changes nothing yet.
    fn changes_grid(&self, original: &Event) -> bool {
        let Ok(event) = self.form.build(self.today, Some(original)) else { return false };
        let length = |e: &Event| e.end.map(|end| instant_of(end) - instant_of(e.start));
        let start = self.start_date();
        event.start != original.start
            || length(&event) != length(original)
            || !repeat::same(self.form.repeat.as_ref(), original.repeat.as_deref(), start)
    }

    /// The event the form describes today, or why it cannot be saved yet. An occurrence ends its
    /// series (`recurring: false`) only with "All" ([`Editor::save_scope`]): with another scope a form
    /// without a rule keeps the series, however the form got there (the second review of #278,
    /// finding 6).
    fn check(&self) -> Result<Event, Problem> {
        let mut event = self.form.build(self.today, self.original.as_ref())?;
        if let Some(original) = self.occurrence() {
            if self.scope != SeriesScope::Occurrence && event.all_day() != original.all_day() {
                return Err(Problem::AllDaySeries);
            }
            if self.save_scope() != Some(SeriesScope::Series) {
                event.recurring = original.recurring;
            }
        }
        Ok(event)
    }

    /// The stored event when it is an occurrence of a series.
    fn occurrence(&self) -> Option<&Event> {
        self.original.as_ref().filter(|event| event.recurring)
    }

    /// The scope a save carries: the band's for an occurrence, none for any other event; "This event"
    /// for an occurrence whose rule could not be read ([`Editor::rule_unknown`]).
    fn save_scope(&self) -> Option<SeriesScope> {
        let scope = if self.rule_unknown() { SeriesScope::Occurrence } else { self.scope };
        self.occurrence().map(|_| scope)
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
        self.sent_scope = self.scope;
        Action::Save(event, self.save_scope())
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

    /// The rule can change for a single event and, for an occurrence, for following or all events.
    fn can_change_repeat(&self) -> bool {
        self.read_only.is_none() && (self.occurrence().is_none() || self.scope != SeriesScope::Occurrence)
    }

    /// "Never" ends a series only for all its events (the core takes no rule away from an
    /// occurrence or a split).
    fn can_end_series(&self) -> bool {
        self.occurrence().is_none() || self.scope == SeriesScope::Series
    }

    /// An occurrence switches between all day and timed only alone ("This event").
    fn can_switch_all_day(&self) -> bool {
        self.read_only.is_none() && (self.occurrence().is_none() || self.scope == SeriesScope::Occurrence)
    }

    /// The start date as typed, or the day the editor opened on while it cannot be read.
    fn start_date(&self) -> NaiveDate {
        parse_date(&self.form.start_date, self.today).unwrap_or(self.day)
    }

    /// The zone the times are shown in: the event's, else this computer's.
    fn shown_zone(&self) -> Option<&str> {
        self.form.time_zone.as_deref().or(self.machine_zone.as_deref())
    }

    // -----------------------------------------------------------------------------------------
    // View
    // -----------------------------------------------------------------------------------------

    /// The side panel with this event, its messages wrapped with `wrap` (`Message::Editor`):
    /// path, the band "Changes apply to" for an occurrence, title, the property rows and the footer.
    /// `anchor` is registered by the row whose menu is open; pass the same one to [`Editor::layer`].
    /// Hand the panel to `torro_gui::panel::place`.
    pub fn panel<'a, M: Clone + 'a>(&'a self, anchor: &Anchor, wrap: fn(Message) -> M) -> Panel<'a, M> {
        let (title, body) = match self.read_only {
            Some(reason) => (self.read_only_title(reason), self.body(anchor, false)),
            None => (self.title_view(), self.body(anchor, true)),
        };
        let mut panel = panel::panel(self.path().map(wrap), title.map(wrap), body.map(wrap), wrap(Message::Close))
            .footer(self.footer().into_iter().map(|element| element.map(wrap)).collect());
        if let Some(band) = self.band() {
            panel = panel.bar(band.map(wrap));
        }
        panel
    }

    /// The open menu or card as a floating layer under its row (`anchor`), or `None`. Hand it to
    /// `torro_gui::panel::place` as its layer; a click beside it closes it.
    pub fn layer<'a, M: Clone + 'a>(&'a self, anchor: &Anchor, wrap: fn(Message) -> M) -> Option<Element<'a, M>> {
        let below = |card: Element<'a, Message>, close: Message| {
            Some(widgets::floating(anchor, Placement::Below(Align::Start), card, close).map(wrap))
        };
        match &self.popup {
            Some(Popup::Repeat) => return below(self.repeat_menu(), Message::ClosePopup),
            Some(Popup::Custom(card)) => {
                let (all_day, zone) = (self.form.all_day, self.form.time_zone.clone());
                let until = move |day| repeat::until_of(day, all_day, |wall| zone::instant(wall, zone.as_deref()));
                return below(card.view(self.start_date(), self.today, until), Message::ClosePopup);
            }
            Some(Popup::Zone) => return below(self.zone_menu(), Message::ClosePopup),
            Some(Popup::ShowAs) => return below(self.show_as_menu(), Message::ClosePopup),
            Some(Popup::Status) => return below(self.status_menu(), Message::ClosePopup),
            Some(Popup::Remind(card)) => {
                let zone = self.shown_zone().map(str::to_string);
                let instant = move |wall| zone::instant(wall, zone.as_deref());
                return below(card.view(self.start_wall(), self.today, instant), Message::ClosePopup);
            }
            Some(Popup::Places { query, places }) => {
                let service = place::service_name(self.maps.geocoder_kind);
                return below(place::suggestions(places, query, service), Message::ClosePopup);
            }
            None => {}
        }
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

    /// The "Repeat" menu: "Never" (where the series may end), the short list for the start date, and
    /// "Custom…". The rule there is now is marked.
    fn repeat_menu(&self) -> Element<'_, Message> {
        let start = self.start_date();
        let current = self.form.repeat.as_ref();
        let mut items = Vec::new();
        if self.can_end_series() {
            items.push(text_item(tr("Never"), Message::PickRepeat(None)).current(current.is_none()));
        }
        let mut matched = current.is_none();
        for choice in repeat::shortlist(start) {
            let on = current.is_some_and(|current| repeat::same_pattern(current, &choice, start));
            matched |= on;
            let pick = repeat::keeping_end(&choice, current);
            items.push(
                text_item(repeat::pattern(&choice, start), Message::PickRepeat(Some(Box::new(pick)))).current(on),
            );
        }
        let custom = MenuItem::new(Icon::SlidersHorizontal, tr("Custom…"), Message::OpenCustom).current(!matched);
        widgets::menu(vec![MenuSection::titled(tr("Repeat"), items), MenuSection::new(vec![custom])])
    }

    /// The time zone menu: a search over the IANA names, this computer's zone, the event's zone, and
    /// what the search finds (at most [`ZONE_RESULTS`]).
    fn zone_menu(&self) -> Element<'_, Message> {
        let search = text_input(tr("Search time zones"), &self.zone_query)
            .id(ZONE_SEARCH_ID)
            .on_input(Message::ZoneQuery)
            .on_submit(Message::ZoneFirst)
            .padding(style::INPUT_PADDING)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .style(style::text_input);
        let shown = self.shown_zone();
        let item = |name: &str| {
            MenuItem::new(Icon::Globe, zone::readable(name), Message::PickZone(name.to_string()))
                .current(shown == Some(name))
        };
        let mut list = column![container(search).padding(Padding::new(space::xs()))];
        if let Some(machine) = &self.machine_zone {
            list = list.push(caption(tr("Your time zone"))).push(widgets::menu_item(item(machine)));
        }
        if let Some(own) = self.form.time_zone.as_deref().filter(|own| Some(*own) != self.machine_zone.as_deref()) {
            list = list.push(caption(tr("This event"))).push(widgets::menu_item(item(own)));
        }
        let found = zone::search(&self.zone_query, ZONE_RESULTS);
        if !found.is_empty() {
            let rows = column(found.into_iter().map(|name| widgets::menu_item(item(name))));
            let rows = scrollable(rows)
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(style::scrollable)
                .height(Length::Shrink);
            list = list.push(caption(tr("Time zones"))).push(container(rows).max_height(zone_list_height()));
        } else if !self.zone_query.trim().is_empty() {
            let none = typo::text(typo::body(), tr("No time zone found")).style(theme::text_3);
            list = list.push(container(none).padding(style::ITEM_PADDING.top(space::s()).bottom(space::s())));
        }
        container(list).width(widgets::menu_width()).padding(style::MENU_INSET).style(style::container::menu).into()
    }

    /// "Show as": busy or free.
    fn show_as_menu(&self) -> Element<'_, Message> {
        let item = |transparency: Transparency| {
            let (mark, label) = show_as(transparency);
            MenuItem::new(mark, label, Message::ShowAs(transparency)).current(self.form.transparency == transparency)
        };
        widgets::menu(vec![MenuSection::titled(
            tr("Show as"),
            vec![item(Transparency::Opaque), item(Transparency::Transparent)],
        )])
    }

    /// "Status": confirmed, tentative, cancelled.
    fn status_menu(&self) -> Element<'_, Message> {
        let shown = self.form.status.unwrap_or(EventStatus::Confirmed);
        let item = |status: EventStatus| {
            let (mark, label) = status_words(status);
            MenuItem::new(mark, label, Message::Status(status)).current(shown == status)
        };
        let items = vec![item(EventStatus::Confirmed), item(EventStatus::Tentative), item(EventStatus::Cancelled)];
        widgets::menu(vec![MenuSection::titled(tr("Status"), items)])
    }

    /// The band "Changes apply to: This event · Following · All" of an occurrence, with what the
    /// scope resets in a quiet line under it: before a change of time or rule, after the save, or
    /// while one is about to be saved. `None` for an event that is not part of a series and in a
    /// read-only calendar.
    fn band(&self) -> Option<Element<'_, Message>> {
        self.occurrence().filter(|_| self.read_only.is_none())?;
        // A series whose rule could not be read changes this event alone (`rule_unknown`).
        let open = !self.rule_unknown();
        let scopes = [
            (tr("This event"), Some(Message::Scope(SeriesScope::Occurrence))),
            (tr("Following"), open.then_some(Message::Scope(SeriesScope::Following))),
            (tr("All"), open.then_some(Message::Scope(SeriesScope::Series))),
        ];
        let chosen = match self.scope {
            SeriesScope::Occurrence => 0,
            SeriesScope::Following => 1,
            SeriesScope::Series => 2,
        };
        let label = row![
            icon(Icon::Repeat, icon::SIZE_SM).style(|t, _| svg::Style { color: Some(palette(t).text_3) }),
            widgets::ellipsized(typo::body(), tr("Changes apply to"), theme::text_2),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let mut control = widgets::segmented_maybe(scopes, chosen);
        if !open {
            control = widgets::with_tooltip(control, tr(UNKNOWN_RULE));
        }
        let head = row![container(label).width(Length::Fill), control].spacing(space::s()).align_y(Alignment::Center);
        let mut band = column![head].spacing(space::xs());
        if let Some(note) = self.band_note() {
            band = band.push(note);
        }
        let band = container(band).width(Length::Fill).padding([space::s(), space::l()]);
        Some(column![band, rule::horizontal(size::HAIRLINE).style(style::rule)].into())
    }

    /// The quiet lines of the band: that the rule does not move with a new date
    /// ([`Editor::date_off_rule`]), and what the scope resets – the count before a change is saved
    /// ([`Editor::resets`]), the core's count after a save, and with "Following" what a change of
    /// time or rule would reset. `None` when there is nothing to say.
    fn band_note(&self) -> Option<Element<'_, Message>> {
        let mut lines: Vec<String> = Vec::new();
        if self.date_off_rule() {
            lines.push(tr("The repeat stays on its days; check that it fits the new date").to_string());
        }
        lines.extend(self.reset_words());
        if lines.is_empty() {
            return None;
        }
        let line = |words: String| -> Element<'_, Message> {
            let mark = icon(Icon::Info, icon::SIZE_SM).style(|_, _| svg::Style { color: Some(theme::attention()) });
            row![
                container(mark).center_y(typo::meta().line_height.to_absolute(typo::meta().size)),
                typo::text(typo::meta(), words).style(theme::text_2)
            ]
            .spacing(space::s())
            .align_y(Alignment::Start)
            .into()
        };
        Some(column(lines.into_iter().map(line)).spacing(space::xs()).into())
    }

    /// What the scope resets, in words, or `None` when nothing is reset ([`Editor::band_note`]).
    fn reset_words(&self) -> Option<String> {
        let original = self.occurrence()?;
        let pending = self.resets();
        if pending > 0 {
            let ends = self.form.repeat.is_none();
            return Some(match (self.scope, pending) {
                (SeriesScope::Series, 1) if ends => tr("Ending the series resets 1 changed event").to_string(),
                (SeriesScope::Series, n) if ends => trf("Ending the series resets {n} changed events", &[("n", &n)]),
                (SeriesScope::Series, 1) => tr("This change resets 1 changed event").to_string(),
                (SeriesScope::Series, n) => trf("This change resets {n} changed events", &[("n", &n)]),
                (_, 1) => tr("1 changed event after this date is reset").to_string(),
                (_, n) => trf("{n} changed events after this date are reset", &[("n", &n)]),
            });
        }
        if self.reset > 0 {
            return Some(match self.reset {
                1 => tr("1 changed event was reset").to_string(),
                n => trf("{n} changed events were reset", &[("n", &n)]),
            });
        }
        let key = original.recurrence_id?;
        match (self.scope, self.changed_after(original, key)) {
            (SeriesScope::Following, 1) => {
                Some(tr("Changing time or repeat resets 1 changed event after this date").to_string())
            }
            (SeriesScope::Following, n) if n > 1 => {
                Some(trf("Changing time or repeat resets {n} changed events after this date", &[("n", &n)]))
            }
            _ => None,
        }
    }

    /// "● Work › Wed, 7 Oct": the calendar with its colour and the day the event starts on.
    fn path(&self) -> Element<'_, Message> {
        let day = self.start_date();
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
            // Room for the focus ring beside the text, as little as keeps it off the letters.
            .padding(Padding::ZERO.left(space::xs()).right(space::xs()))
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text));
        let mut title = column![field].spacing(space::s());
        if let Some(problem) = self.shown_problem().filter(|p| p.field() == Field::Title) {
            title = title.push(problem_line(problem));
        }
        title.into()
    }

    /// The property rows in their groups, each divided by a line: when and how (Start, End, All
    /// day, Repeat, Time zone, Calendar, Show as, Status), where (Location, Link), who and what is
    /// said (Reminders, People), then Notes and Links. `editable` shows inputs and menus; read-only
    /// shows the values, leaves out what is not set, and lets location and notes be selected.
    fn body(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let shown = self.shown_problem();
        let problem_in = |field: Field| shown.filter(|problem| problem.field() == field);

        let mut when = vec![
            widgets::prop_line(tr("Start"), None, self.moment_value(true, editable), None),
            widgets::prop_line(tr("End"), None, self.moment_value(false, editable), None),
        ];
        when.extend(problem_in(Field::End).map(field_problem));
        when.insert(1, column(problem_in(Field::Start).map(field_problem)).into());
        when.push(self.all_day_line(editable));
        when.push(self.repeat_line(anchor, editable));
        if !self.form.all_day {
            when.push(self.zone_line(anchor, editable));
        }
        when.push(self.calendar_line(anchor, editable));
        when.extend(problem_in(Field::Calendar).map(field_problem));
        when.push(self.show_as_line(anchor, editable));
        when.push(self.status_line(anchor, editable));

        let mut place = self.location_lines(anchor, editable);
        if let Some(link) = self.link_line(editable) {
            place.push(link);
        }

        let mut who =
            if editable || !self.form.alarms.is_empty() { self.alarm_lines(anchor, editable) } else { Vec::new() };
        who.extend(self.people_lines());

        let mut body = column![column(when)].spacing(space::s());
        for group in [place, who] {
            if !group.is_empty() {
                body = body.push(divider()).push(column(group));
            }
        }
        let notes = if editable {
            Some(self.notes_editor())
        } else {
            (!self.form.notes.trim().is_empty()).then(|| selectable(&self.notes, Message::Notes, |t| palette(t).text_2))
        };
        if let Some(notes) = notes {
            body = body.push(divider()).push(widgets::prop_section(tr("Notes"), self.notes_rows(notes)));
        }
        if let Some(links) = self.links_section() {
            body = body.push(links);
        }
        body.into()
    }

    /// Start (`start`) or end: date and time inputs side by side, or the value as text.
    fn moment_value(&self, start: bool, editable: bool) -> Element<'_, Message> {
        let form = &self.form;
        let (date, time) = if start { (&form.start_date, &form.start_time) } else { (&form.end_date, &form.end_time) };
        if !editable {
            let value = if form.all_day { date.clone() } else { format!("{date}  {time}") };
            return value_text(value);
        }
        let shown = self.shown_problem();
        let invalid = |input: &str| shown.is_some_and(|problem| problem.input() == Some(input));
        let (date_id, time_id) = if start { (START_DATE_ID, START_TIME_ID) } else { (END_DATE_ID, END_TIME_ID) };
        let date = if start {
            Input::new(tr("Date"), date, Message::StartDate, date_id, invalid(date_id))
        } else {
            Input::new(tr("Date"), date, Message::EndDate, date_id, invalid(date_id))
        };
        let time = if start {
            Input::new(tr("Time"), time, Message::StartTime, time_id, invalid(time_id))
        } else {
            Input::new(tr("Time"), time, Message::EndTime, time_id, invalid(time_id))
        };
        moment(form.all_day, date, time)
    }

    /// "All day" with its switch; the row toggles it too. Off in a read-only event and for following
    /// or all events of a series (with the reason as tooltip).
    fn all_day_line(&self, editable: bool) -> Element<'_, Message> {
        let on = self.form.all_day;
        let can = editable && self.can_switch_all_day();
        let mut switch = toggler(on).size(style::CHECK_SIZE).style(style::toggler);
        if can {
            switch = switch.on_toggle(Message::AllDay);
        }
        // In line with the text of the inputs above it.
        let switch = container(switch).padding(Padding::ZERO.left(style::INPUT_PADDING.left));
        let line = widgets::prop_line(tr("All day"), None, switch, can.then_some(Message::AllDay(!on)));
        if editable && !can { widgets::with_tooltip(line, Problem::AllDaySeries.message()) } else { line }
    }

    /// "Repeat" with the sentence of the rule ("Never" without one); opens the menu where the rule
    /// can change, else says why in a tooltip.
    fn repeat_line(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let start = self.start_date();
        let value = match &self.form.repeat {
            Some(rule) => widgets::prop_value(repeat::sentence(rule, start)),
            None => widgets::prop_placeholder(tr("Never")),
        };
        let can = editable && self.can_change_repeat();
        let line = widgets::prop_line(
            tr("Repeat"),
            Some(Mark::Icon(Icon::Repeat)),
            value,
            can.then_some(Message::ToggleRepeat),
        );
        let open = matches!(self.popup, Some(Popup::Repeat | Popup::Custom(_)));
        if open {
            widgets::anchored(anchor, line)
        } else if editable && self.rule_unknown() {
            widgets::with_tooltip(line, tr(UNKNOWN_RULE))
        } else if editable && !can {
            widgets::with_tooltip(line, tr("Choose Following or All to change the repeat"))
        } else {
            line
        }
    }

    /// "Time zone": the city of the zone, its abbreviation and "your time zone" for this computer's.
    fn zone_line(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let name = self.shown_zone();
        let at = self.check().ok().map_or_else(Utc::now, |event| instant_of(event.start));
        let city = name.map_or_else(|| tr("This computer's time zone").to_string(), zone::city);
        let mut quiet: Vec<String> = name.and_then(|name| zone::abbreviation(name, at)).into_iter().collect();
        if name.is_some() && name == self.machine_zone.as_deref() {
            quiet.push(tr("your time zone").to_string());
        }
        let value = zone_value(city, quiet.join(" · "));
        let line = widgets::prop_line(
            tr("Time zone"),
            Some(Mark::Icon(Icon::Globe)),
            value,
            editable.then_some(Message::ToggleZones),
        );
        if self.popup == Some(Popup::Zone) { widgets::anchored(anchor, line) } else { line }
    }

    /// "Calendar" with its dot; a draft with more than one calendar opens the menu from here.
    fn calendar_line(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let on_press = (editable && self.can_choose_calendar()).then_some(Message::ToggleCalendars);
        let line = match &self.form.calendar {
            Some(choice) => widgets::prop_line(tr("Calendar"), None, calendar_value(choice), on_press),
            None => widgets::prop_line(
                tr("Calendar"),
                Some(Mark::Icon(Icon::Calendar)),
                widgets::prop_placeholder(tr("Choose a calendar")),
                on_press,
            ),
        };
        if self.menu { widgets::anchored(anchor, line) } else { line }
    }

    /// "Show as": busy or free.
    fn show_as_line(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let (mark, label) = show_as(self.form.transparency);
        let on_press = editable.then_some(Message::ToggleShowAs);
        let line = widgets::prop_line(tr("Show as"), Some(Mark::Icon(mark)), widgets::prop_value(label), on_press);
        if self.popup == Some(Popup::ShowAs) { widgets::anchored(anchor, line) } else { line }
    }

    /// "Status": confirmed (also when the event does not say), tentative or cancelled.
    fn status_line(&self, anchor: &Anchor, editable: bool) -> Element<'_, Message> {
        let (mark, label) = status_words(self.form.status.unwrap_or(EventStatus::Confirmed));
        let on_press = editable.then_some(Message::ToggleStatus);
        let line = widgets::prop_line(tr("Status"), Some(Mark::Icon(mark)), widgets::prop_value(label), on_press);
        if self.popup == Some(Popup::Status) { widgets::anchored(anchor, line) } else { line }
    }

    /// The rows of "Location" (task TE.8): the field (read-only: the selectable text) with "Search"
    /// while the text has no place yet and maps are on, and ↗ "Open in Maps" when there is no map;
    /// under it the map of the coordinates with ↗ on it, and the quiet line ("Searching…", maps off,
    /// a failed search). Read-only without a location: nothing.
    fn location_lines(&self, anchor: &Anchor, editable: bool) -> Vec<Element<'_, Message>> {
        let typed = !self.form.location.trim().is_empty();
        if !editable && !typed {
            return Vec::new();
        }
        let map = self.shown_map();
        let mut value = row![self.location_field(editable)].spacing(space::s()).align_y(Alignment::Center);
        if editable && typed && self.form.geo.is_none() && self.maps.enabled {
            let search = self.searched.is_none().then_some(Message::SearchPlaces);
            value = value.push(widgets::icon_button(Icon::Search, tr("Search"), search));
        }
        if typed && map.is_none() {
            value = value.push(widgets::icon_button(Icon::ArrowUpRight, tr("Open in Maps"), Some(Message::OpenMaps)));
        }
        let line = widgets::prop_line(tr("Location"), Some(Mark::Icon(Icon::MapPin)), value, None);
        let open = matches!(self.popup, Some(Popup::Places { .. }));
        let mut lines = vec![if open { widgets::anchored(anchor, line) } else { line }];
        if let Some(picture) = map {
            let pin = self.form.calendar.as_ref().map_or(collection_color(None), choice_color);
            lines.push(under_value(place::map_view(picture, pin, Message::OpenMaps)));
        }
        let note = match (&self.searched, &self.place_note) {
            (Some(_), _) => Some(tr("Searching…").to_string()),
            (None, Some(note)) => Some(note.words()),
            (None, None) => None,
        };
        if let Some(note) = note {
            lines.push(under_value(typo::text(typo::meta(), note).style(theme::text_3).into()));
        }
        lines
    }

    /// The location as a quiet field (Enter searches, [`Editor::search`]), or read-only as selectable
    /// text.
    fn location_field(&self, editable: bool) -> Element<'_, Message> {
        if !editable {
            return container(selectable(&self.location, Message::LocationText, |t| palette(t).text))
                .width(Length::Fill)
                .into();
        }
        text_input(tr("Add location"), &self.form.location)
            .on_input(Message::Location)
            .on_submit(Message::SearchPlaces)
            .padding(style::INPUT_PADDING)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .style(quiet(false))
            .into()
    }

    /// "Link": a quiet field with "Open link" beside a web address; read-only the address, which
    /// opens when it is a web address. `None` read-only without a link.
    fn link_line(&self, editable: bool) -> Option<Element<'_, Message>> {
        let url = self.form.url.trim();
        let open = web_link(url).then(|| Message::OpenLink(url.to_string()));
        let mark = Some(Mark::Icon(Icon::Link));
        if !editable {
            return (!url.is_empty()).then(|| widgets::prop_line(tr("Link"), mark, widgets::prop_value(url), open));
        }
        let field = text_input(tr("Add link"), &self.form.url)
            .on_input(Message::Url)
            .on_submit(Message::Save)
            .padding(style::INPUT_PADDING)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .style(quiet(false));
        let mut value = row![field].spacing(space::s()).align_y(Alignment::Center);
        if let Some(open) = open {
            value = value.push(widgets::icon_button(Icon::ArrowUpRight, tr("Open link"), Some(open)));
        }
        Some(widgets::prop_line(tr("Link"), mark, value, None))
    }

    /// The rows of "Reminders": a row per reminder (with ✕ where it can be changed), "Add
    /// reminder", and a quiet note when the event has none of its own. Only the first row carries
    /// the name.
    fn alarm_lines(&self, anchor: &Anchor, editable: bool) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<(Mark, Element<'_, Message>, Option<Message>)> = Vec::new();
        let start = zone::instant(self.start_wall(), self.shown_zone());
        for (index, alarm) in self.form.alarms.iter().enumerate() {
            let words = remind::day_before_words(alarm.trigger, start, |at| zone::wall(at, self.shown_zone()))
                .unwrap_or_else(|| alarm_when(alarm.trigger));
            // Cut with "…" and the whole time as tooltip, never silently (the design review of PR #294).
            let label = widgets::ellipsized(typo::body(), words, theme::text);
            let mut value = row![container(label).width(Length::Fill)].spacing(space::s()).align_y(Alignment::Center);
            if let Some(style) = alarm.style {
                value = value
                    .push(typo::text(typo::body(), style_word(style)).style(theme::text_3).wrapping(Wrapping::None));
            }
            if editable {
                value =
                    value.push(widgets::icon_button(Icon::X, tr("Remove reminder"), Some(Message::RemoveAlarm(index))));
            }
            let mark = match alarm.style {
                Some(AlarmStyle::FullScreen) => Icon::Maximize,
                _ => Icon::Bell,
            };
            rows.push((Mark::Icon(mark), value.into(), None));
        }
        if self.form.alarms.is_empty() {
            let quiet = two_part(tr("Default reminder").to_string(), tr("from Settings").to_string(), theme::text_2);
            rows.push((Mark::Icon(Icon::Bell), quiet, None));
        }
        if editable {
            let add = typo::text(typo::body(), tr("Add reminder")).style(theme::text_2);
            rows.push((Mark::Icon(Icon::Plus), add.into(), Some(Message::ToggleAlarms)));
        }
        let last = rows.len().saturating_sub(1);
        rows.into_iter()
            .enumerate()
            .map(|(i, (mark, value, on_press))| {
                let name = if i == 0 { tr("Reminders") } else { "" };
                let line = widgets::prop_line(name, Some(mark), value, on_press);
                let open = matches!(self.popup, Some(Popup::Remind(_)));
                if editable && i == last && open { widgets::anchored(anchor, line) } else { line }
            })
            .collect()
    }

    /// "People": the avatars of those who come (at most three and "+n") and who organises, with the
    /// whole list as tooltip, and under it that the server tells them about changes. Only shown,
    /// never changed. Empty without invitees.
    fn people_lines(&self) -> Vec<Element<'_, Message>> {
        let Some(event) = self.original.as_ref() else { return Vec::new() };
        if event.attendees.is_empty() && event.organizer.is_none() {
            return Vec::new();
        }
        let colour = self.form.calendar.as_ref().map_or(collection_color(None), choice_color);
        let coming: Vec<&Attendee> = event.attendees.iter().filter(|a| a.partstat != PartStat::Declined).collect();
        let faces: Vec<(String, Color)> =
            coming.iter().take(AVATARS).filter_map(|a| initials(a)).map(|letters| (letters, colour)).collect();
        let more = coming.len().saturating_sub(faces.len());
        let who = match event.organizer.as_deref().and_then(person_name) {
            Some(name) => trf("{name} organizes", &[("name", &name)]),
            None => trf("{n} people", &[("n", &event.attendees.len())]),
        };
        let everyone: Vec<String> =
            event.organizer.as_deref().into_iter().chain(&event.attendees).filter_map(person_name).collect();
        let mut everyone_once: Vec<String> = Vec::new();
        for name in everyone {
            if !everyone_once.contains(&name) {
                everyone_once.push(name);
            }
        }
        let mut value = row![].spacing(space::s()).align_y(Alignment::Center);
        if !faces.is_empty() || more > 0 {
            value = value.push(widgets::avatar_stack(&faces, more));
        }
        // Cut with "…"; the tooltip around the line names everyone (the design review of PR #294).
        let who = iced::widget::responsive(move |room| {
            let shown = typo::ellipsize(&who, typo::body(), room.width);
            typo::text(typo::body(), shown).style(theme::text_3).wrapping(Wrapping::None).into()
        })
        .height(Length::Shrink);
        value = value.push(who);
        let value = widgets::with_tooltip(container(value).width(Length::Fill).clip(true), everyone_once.join(", "));
        let line = widgets::prop_line(tr("People"), None, value, None);
        let note =
            typo::text(typo::meta(), tr("People in this event get an update from your server")).style(theme::text_3);
        vec![line, container(note).padding(Padding::ZERO.left(value_left()).bottom(space::xs())).into()]
    }

    /// The notes as a quiet field that wraps.
    fn notes_editor(&self) -> Element<'_, Message> {
        text_editor(&self.notes)
            .placeholder(tr("Add notes…"))
            .on_action(Message::Notes)
            .key_binding(|press| match press.key.as_ref() {
                _ if !is_focused(&press) => text_editor::Binding::from_key_press(press),
                keyboard::Key::Named(keyboard::key::Named::Escape) => Some(leave_field()),
                _ => text_editor::Binding::from_key_press(press),
            })
            .font(typo::body().font)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .padding(style::INPUT_PADDING)
            .wrapping(Wrapping::WordOrGlyph)
            .style(|t, s| bare_editor(t, s, palette(t).text_2))
            .into()
    }

    /// The rows of "Notes": the notes, then "Copy notes" ("Copied" right after a press) when there
    /// are any. The notes carry the input's padding themselves (room for the focus ring), which
    /// puts their text where `panel::inset` would.
    fn notes_rows<'a>(&'a self, notes: Element<'a, Message>) -> Vec<Element<'a, Message>> {
        let mut rows = vec![notes];
        if !self.form.notes.trim().is_empty() {
            let (mark, label) = if self.copied { (Icon::Check, tr("Copied")) } else { (Icon::Copy, tr("Copy notes")) };
            let label = typo::text(typo::body(), label).style(theme::text_2);
            rows.push(widgets::prop_row(Some(Mark::Icon(mark)), label, Some(Message::CopyNotes)));
        }
        rows
    }

    /// "Links" with a row per address of [`Editor::links`] (a video call with "Join"); `None`
    /// without any.
    fn links_section(&self) -> Option<Element<'_, Message>> {
        let found = self.links();
        if found.is_empty() {
            return None;
        }
        Some(widgets::prop_section(tr("Links"), found.into_iter().map(link_row).collect()))
    }

    /// The addresses in the location and the notes (as typed) that may be opened, each once, video
    /// calls first and otherwise in the order they appear.
    fn links(&self) -> Vec<&str> {
        let mut found = links::links(&self.form.location);
        for link in links::links(&self.form.notes) {
            if !found.contains(&link) {
                found.push(link);
            }
        }
        found.retain(|url| links::openable(url));
        // A call is what one opens when the event starts; the sort is stable.
        found.sort_by_key(|url| links::meeting(url).is_none());
        found
    }

    /// The title of an event in a read-only calendar, with the reason under it.
    fn read_only_title(&self, reason: &'static str) -> Element<'_, Message> {
        let note = typo::text(typo::meta(), tr(reason)).style(theme::text_2);
        column![panel::title(self.form.title.as_str()), note].spacing(space::s()).into()
    }

    /// "Preview reminder" and "Delete event" for a stored event that can be edited; after "Delete
    /// event" the question with Delete and Cancel, for a series with one button per scope. Drafts
    /// and read-only events have no footer.
    fn footer(&self) -> Vec<Element<'_, Message>> {
        if self.read_only.is_some() || self.original.is_none() {
            return Vec::new();
        }
        if !self.confirm_delete {
            let preview = widgets::text_button(
                Some(Icon::BellRing),
                tr("Preview reminder"),
                style::button::ghost,
                Some(Message::PreviewReminder),
            );
            return vec![
                preview,
                iced::widget::space::horizontal().into(),
                widgets::text_button(
                    Some(Icon::Trash2),
                    tr("Delete event"),
                    style::button::danger,
                    Some(Message::Delete),
                ),
            ];
        }
        if self.occurrence().is_none() {
            return vec![
                typo::text(typo::body(), tr("Delete this event?")).style(theme::text_2).into(),
                widgets::text_button(None, tr("Delete"), style::button::danger, Some(Message::ConfirmDelete(None))),
                widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelDelete)),
            ];
        }
        vec![self.series_question()]
    }

    /// The question before deleting an occurrence: which events go, one button per scope, and
    /// Cancel (Escape).
    fn series_question(&self) -> Element<'_, Message> {
        let title = trf("Delete “{title}”?", &[("title", &self.form.title.trim())]);
        let why = tr("It is part of a series. Other apps and your phone see the change.");
        let scope = |label: &'static str,
                     scope: SeriesScope,
                     look: fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style,
                     mark: Option<Icon>| {
            widgets::text_button(mark, label, look, Some(Message::ConfirmDelete(Some(scope))))
        };
        let cancel = row![
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CancelDelete)),
            widgets::kbd("Esc"),
        ]
        .spacing(space::xs())
        .align_y(Alignment::Center);
        let buttons = row![
            scope(tr("Only this event"), SeriesScope::Occurrence, style::button::secondary, None),
            scope(tr("This and following"), SeriesScope::Following, style::button::secondary, None),
            scope(tr("All events"), SeriesScope::Series, framed_danger, Some(Icon::Trash2)),
            cancel,
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .wrap()
        .vertical_spacing(space::s());
        column![typo::text(typo::body_strong(), title), typo::text(typo::meta(), why).style(theme::text_2), buttons,]
            .spacing(space::xs())
            .width(Length::Fill)
            .into()
    }
}

/// A span of minutes in words: "5 min", "1 hour", "3 hours", "1 day", "2 days".
fn span(minutes: i64) -> String {
    match minutes {
        m if m % 1440 == 0 && m / 1440 == 1 => tr("1 day").to_string(),
        m if m % 1440 == 0 => trf("{n} days", &[("n", &(m / 1440))]),
        m if m % 60 == 0 && m / 60 == 1 => tr("1 hour").to_string(),
        m if m % 60 == 0 => trf("{n} hours", &[("n", &(m / 60))]),
        m => trf("{n} min", &[("n", &m)]),
    }
}

/// When a reminder fires, in words: "At start", "15 min before", "1 day before", "10 min before the
/// end", "5 min after the start", or the day and time of a fixed one.
pub fn alarm_when(trigger: AlarmTrigger) -> String {
    match trigger {
        AlarmTrigger::At(at) => {
            let local = torro_gui::datetime::to_local(at);
            format!("{} {}", format_date(local.date()), format_time(local.time()))
        }
        AlarmTrigger::Before { minutes: 0, of: AlarmAnchor::Start } => tr("At start").to_string(),
        AlarmTrigger::Before { minutes: 0, of: AlarmAnchor::End } => tr("At the end").to_string(),
        AlarmTrigger::Before { minutes, of: AlarmAnchor::Start } if minutes > 0 => {
            trf("{span} before", &[("span", &span(minutes))])
        }
        AlarmTrigger::Before { minutes, of: AlarmAnchor::Start } => {
            trf("{span} after the start", &[("span", &span(-minutes))])
        }
        AlarmTrigger::Before { minutes, of: AlarmAnchor::End } if minutes > 0 => {
            trf("{span} before the end", &[("span", &span(minutes))])
        }
        AlarmTrigger::Before { minutes, of: AlarmAnchor::End } => {
            trf("{span} after the end", &[("span", &span(-minutes))])
        }
    }
}

/// How a reminder with a style of its own shows, quietly after its time: "Notification", "Full
/// screen".
fn style_word(style: AlarmStyle) -> &'static str {
    match style {
        AlarmStyle::Notification => tr("Notification"),
        AlarmStyle::FullScreen => tr("Full screen"),
    }
}

/// The icon and the word of "Show as".
fn show_as(transparency: Transparency) -> (Icon, &'static str) {
    match transparency {
        Transparency::Opaque => (Icon::CircleDot, tr("Busy")),
        Transparency::Transparent => (Icon::Circle, tr("Free")),
    }
}

/// The icon and the word of a status: a check, dashed like a tentative event in the views, a cross.
fn status_words(status: EventStatus) -> (Icon, &'static str) {
    match status {
        EventStatus::Confirmed => (Icon::CircleCheck, tr("Confirmed")),
        EventStatus::Tentative => (Icon::CircleDashed, tr("Tentative")),
        EventStatus::Cancelled => (Icon::CircleX, trc("event", "Cancelled")),
    }
}

/// The value of "Time zone": the city in `text`, then the abbreviation and "your time zone" quietly
/// in `text_3`. Where they do not fit, the quiet part goes and the city is cut with "…"
/// ([`typo::ellipsize`]); then the whole value shows as tooltip, never cut silently.
fn zone_value<'a>(city: String, quiet: String) -> Element<'a, Message> {
    two_part(city, quiet, theme::text)
}

/// `city` (the first part) in `colour`, then `quiet` in `text_3`, cut as [`zone_value`] cuts them.
fn two_part<'a>(
    city: String,
    quiet: String,
    colour: fn(&iced::Theme) -> iced::widget::text::Style,
) -> Element<'a, Message> {
    let level = typo::body();
    let whole = if quiet.is_empty() { city.clone() } else { format!("{city} · {quiet}") };
    iced::widget::responsive(move |room| {
        let quiet_width = if quiet.is_empty() { 0.0 } else { text_width(&quiet, level) + space::s() };
        let both = text_width(&city, level) + quiet_width <= room.width;
        let shown = if both { city.clone() } else { typo::ellipsize(&city, level, room.width) };
        let mut line = row![typo::text(level, shown.clone()).style(colour).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center);
        if both && !quiet.is_empty() {
            line = line.push(typo::text(level, quiet.clone()).style(theme::text_3).wrapping(Wrapping::None));
        }
        let line = container(line).width(Length::Fill).clip(true);
        let cut = shown != city || (!both && !quiet.is_empty());
        if cut { widgets::with_tooltip(line, whole.clone()) } else { line.into() }
    })
    .height(Length::Shrink)
    .into()
}

/// The answer "All events" to the question before deleting an occurrence: the frame and surface of
/// the other answers ([`style::button::secondary`]) with the words in the error colour of
/// [`style::button::danger`], so the three read as one row of choices and the destructive one still
/// stands out.
fn framed_danger(theme: &Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let framed = style::button::secondary(theme, status);
    iced::widget::button::Style { text_color: style::button::danger(theme, status).text_color, ..framed }
}

/// A menu entry without a mark.
fn text_item<'a>(label: impl text::IntoFragment<'a>, on_press: Message) -> MenuItem<'a, Message> {
    MenuItem { mark: None, ..MenuItem::new(Icon::Repeat, label, on_press) }
}

/// A section title inside a menu card that is more than entries (the zone menu).
fn caption<'a>(title: &'static str) -> Element<'a, Message> {
    container(typo::text(typo::caption(), title).style(theme::text_3))
        .padding(Padding::new(space::s()).bottom(space::xs()))
        .into()
}

/// How tall the list of found zones may get before it scrolls: eight entries.
fn zone_list_height() -> f32 {
    style::MENU_ITEM_HEIGHT * 8.0
}

/// A line between the groups of rows: with the column's spacing 16 above and below it, the gap
/// between groups the design rules ask for (the design review of PR #294).
fn divider<'a>() -> Element<'a, Message> {
    container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::s(), 0.0]).into()
}

/// The name of someone invited, else their address.
fn person_name(person: &Attendee) -> Option<String> {
    let name = person.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
    name.or(person.email.as_deref()).map(str::to_string)
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

/// Where the values of the property rows start: the row's inner margin, the name column and the gap
/// after it.
fn value_left() -> f32 {
    style::ITEM_PADDING.left + widgets::prop_label_width() + space::s()
}

/// `element` under a property row, starting where the values start.
fn under_value(element: Element<'_, Message>) -> Element<'_, Message> {
    container(element).padding(Padding::ZERO.left(value_left()).bottom(space::xs())).into()
}

/// A value as plain text where the form has an input, starting where the input's text does.
fn value_text<'a>(value: String) -> Element<'a, Message> {
    container(typo::text(typo::body().tabular(), value).wrapping(Wrapping::None))
        .padding(Padding::ZERO.left(style::INPUT_PADDING.left))
        .into()
}

/// A read-only text editor over `content` in `BODY`, coloured by `value`: it can be selected and
/// copied (⌘C / Ctrl+C), typing does nothing ([`Editor::update`] drops edits). Iced 0.14 has no
/// selectable text widget, so this is the way to select text that cannot be changed.
fn selectable<'a>(
    content: &'a text_editor::Content,
    on_action: fn(text_editor::Action) -> Message,
    value: fn(&Theme) -> Color,
) -> Element<'a, Message> {
    text_editor(content)
        .on_action(on_action)
        .font(typo::body().font)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        // As an input's: the text where the editable fields put theirs, room for the focus ring.
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .style(move |t, s| bare_editor(t, s, value(t)))
        .into()
}

/// One row of "Links". A video call: the service's name and "Join" (the address as its tooltip);
/// any other address: the whole address, cut with "…", the row opens it.
fn link_row(url: &str) -> Element<'_, Message> {
    let open = Message::OpenLink(url.to_owned());
    match links::meeting(url) {
        Some(service) => {
            let join = widgets::text_button(None, tr("Join"), style::button::secondary, Some(open));
            let value =
                row![container(widgets::prop_value(service)).width(Length::Fill), widgets::with_tooltip(join, url)]
                    .spacing(space::s())
                    .align_y(Alignment::Center);
            widgets::prop_row(Some(Mark::Icon(Icon::Video)), value, None)
        }
        None => widgets::prop_row(Some(Mark::Icon(Icon::ArrowUpRight)), widgets::prop_value(url), Some(open)),
    }
}

/// A problem under a row, starting where the text of the inputs starts.
fn field_problem<'a>(problem: Problem) -> Element<'a, Message> {
    let left = value_left() + style::INPUT_PADDING.left;
    container(problem_line(problem)).padding(Padding::ZERO.left(left).bottom(space::xs())).into()
}

/// What is wrong, in the error colour with an icon: "(!) Enter a title".
fn problem_line<'a>(problem: Problem) -> Element<'a, Message> {
    problem_note(problem.message())
}

/// `message` in the error colour with the alert icon in front, in `META`: what is wrong in the form
/// or in a card ("Add reminder", "Custom repeat"), always with the icon, so colour is never the only
/// sign.
pub(super) fn problem_note<'a>(message: &'static str) -> Element<'a, Message> {
    let mark =
        icon(Icon::CircleAlert, icon::SIZE_SM).style(|theme, _| svg::Style { color: Some(palette(theme).error_text) });
    row![
        // The icon is centred on the first line of the message, which may wrap in a narrow window.
        container(mark).center_y(typo::meta().line_height.to_absolute(typo::meta().size)),
        typo::text(typo::meta(), message).style(|theme| text::Style { color: Some(palette(theme).error_text) }),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Start)
    .into()
}

/// The framed input of a card ([`style::text_input`]) with its border in the error colour while
/// `invalid`: the field the card's problem is about (the message under it says what is wrong).
pub(super) fn card_input(invalid: bool) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |theme, status| {
        let mut look = style::text_input(theme, status);
        if invalid {
            look.border.color = palette(theme).error_text;
        }
        look
    }
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

/// A text editor without frame or surface at rest (Linear's title and description): text in
/// `value`, placeholder `text_3`. Focused it shows the suite's focus ring (`focus_ring`, 2 px, as
/// [`quiet`] inputs do), so the keyboard's place is always visible; the field's padding keeps the
/// ring off the text.
fn bare_editor(theme: &Theme, status: text_editor::Status, value: Color) -> text_editor::Style {
    let base = style::text_editor(theme, status);
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: if focused { base.border } else { Border::default() },
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
    use torro_pim::model::{RepeatDay, Weekday};

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
            Action::Save(event, _) => event,
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
        editor.stored(&event, 0)
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
        assert!(matches!(editor.update(Message::Save), Action::Save(_, None)));
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
            Action::Save(next, _) => {
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
        assert_eq!(editor.leave().and_then(|(e, _)| e.location), Some("Room 4".into()));
        editor.update(Message::StartTime("25:00".into()));
        assert!(editor.leave().is_none());
    }

    #[test]
    fn delete_asks_once_and_only_for_stored_events() {
        let mut new = Editor::new_event(&calendars(), day(2026, 10, 11), None);
        new.update(Message::Delete);
        assert!(!new.confirm_delete, "a draft has nothing to delete");
        assert!(matches!(new.update(Message::ConfirmDelete(None)), Action::None));
        assert!(new.footer().is_empty());

        let event = stored("Review", EventTime::Date(day(2026, 10, 11)), None);
        let mut existing = Editor::edit(&event, &calendars());
        assert_eq!(existing.footer().len(), 3, "Preview reminder, the gap and Delete event");
        assert!(matches!(existing.update(Message::Delete), Action::None));
        assert!(existing.confirm_delete);
        assert_eq!(existing.footer().len(), 3, "the question, Delete and Cancel");
        existing.update(Message::CancelDelete);
        assert!(!existing.confirm_delete);
        existing.update(Message::Delete);
        match existing.update(Message::ConfirmDelete(Some(SeriesScope::Series))) {
            Action::Delete(deleted, scope) => {
                assert_eq!((deleted, scope), (event, None), "a single event has no scope")
            }
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
        // Nothing opens and nothing changes in a read-only calendar (task TE.7).
        for message in [Message::ToggleRepeat, Message::ToggleZones, Message::ToggleShowAs, Message::ToggleStatus] {
            assert!(matches!(editor.update(message), Action::None));
            assert!(editor.popup.is_none());
        }
        assert!(matches!(editor.update(Message::Status(EventStatus::Cancelled)), Action::None));
        assert!(matches!(editor.update(Message::Url("https://example.org".into())), Action::None));
        assert!(editor.footer().is_empty() && !editor.wants_autosave() && editor.leave().is_none());
        assert!(matches!(editor.update(Message::Escape), Action::Close));
        let _ = editor.panel(&Anchor::new(), |m| m);
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

    /// Issue #126: a read-only event can be selected and copied, but not changed.
    #[test]
    fn a_read_only_event_can_be_selected_but_not_changed() {
        let event = stored("Standup", EventTime::DateTime(to_utc(at(2026, 10, 11, 9, 0))), None);
        let calendars = vec![Collection { writable: false, ..calendar("work", "Work") }];
        let mut editor = Editor::edit(&event, &calendars);
        assert!(matches!(editor.update(Message::Notes(text_editor::Action::SelectAll)), Action::None));
        assert_eq!(editor.notes.selection().as_deref(), Some("Bring slides"), "the notes can be selected");
        editor.update(Message::LocationText(text_editor::Action::SelectAll));
        assert_eq!(editor.location.selection().as_deref(), Some("Room 3"), "and the location");
        let typed = text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new("changed".to_string())));
        editor.update(Message::Notes(typed.clone()));
        editor.update(Message::LocationText(typed));
        assert_eq!((editor.notes.text(), editor.location.text()), ("Bring slides".into(), "Room 3".into()));
        assert_eq!(editor.form.notes, "Bring slides");
        assert!(matches!(editor.update(Message::CopyNotes), Action::Copy(text) if text == "Bring slides"));
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    #[test]
    fn only_web_and_mail_links_are_opened() {
        let mut editor = Editor::edit(&timed(), &calendars());
        for url in ["https://example.org/agenda", "mailto:anna@example.org"] {
            assert!(matches!(editor.update(Message::OpenLink(url.into())), Action::OpenLink(open) if open == url));
        }
        for url in ["file:///etc/passwd", "javascript:alert(1)", "smb://server/share"] {
            assert!(matches!(editor.update(Message::OpenLink(url.into())), Action::None), "{url}");
        }
    }

    #[test]
    fn the_links_of_location_and_notes_come_once_with_calls_first() {
        let mut event = timed();
        event.location = Some("Room 3, or https://example.org/room".into());
        event.description =
            Some("Agenda: https://example.org/agenda.\nCall (https://meet.google.com/abc-defg-hij)\nhttps://example.org/room".into());
        let editor = Editor::edit(&event, &calendars());
        assert_eq!(
            editor.links(),
            ["https://meet.google.com/abc-defg-hij", "https://example.org/room", "https://example.org/agenda"]
        );
        assert!(Editor::edit(&timed(), &calendars()).links().is_empty(), "no links, no section");
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    #[test]
    fn copy_notes_copies_the_notes_as_typed_and_says_so() {
        let mut editor = Editor::edit(&timed(), &calendars());
        let paste = text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(" and the plan".to_string())));
        editor.update(Message::Notes(text_editor::Action::Move(text_editor::Motion::DocumentEnd)));
        editor.update(Message::Notes(paste));
        assert!(matches!(editor.update(Message::CopyNotes), Action::Copy(text) if text == "Bring slides and the plan"));
        assert!(editor.copied, "the row says Copied");
        editor.update(Message::Location("Room 4".into()));
        assert!(!editor.copied, "until anything else happens");
    }

    /// The names in front of the rows fit their column in this language (task TE.7, layout A).
    #[test]
    fn every_row_name_fits_its_column() {
        let names = [
            tr("Start"),
            tr("End"),
            tr("All day"),
            tr("Repeat"),
            tr("Time zone"),
            tr("Calendar"),
            tr("Show as"),
            tr("Status"),
            tr("Location"),
            tr("Link"),
            tr("Reminders"),
            tr("People"),
        ];
        for name in names {
            assert!(text_width(name, typo::meta()) <= widgets::prop_label_width(), "{name}");
        }
    }

    // Reminders (task ER2.3).

    #[test]
    fn reminder_chips_say_when_and_how() {
        let before = |minutes| AlarmTrigger::Before { minutes, of: AlarmAnchor::Start };
        assert_eq!(alarm_when(before(0)), tr("At start"));
        assert_eq!(alarm_when(before(15)), trf("{span} before", &[("span", &trf("{n} min", &[("n", &15)]))]));
        assert_eq!(alarm_when(before(60)), trf("{span} before", &[("span", &tr("1 hour"))]));
        assert_eq!(alarm_when(before(1440)), trf("{span} before", &[("span", &tr("1 day"))]));
        assert_eq!(alarm_when(before(-5)), trf("{span} after the start", &[("span", &trf("{n} min", &[("n", &5)]))]));
        let end = AlarmTrigger::Before { minutes: 10, of: AlarmAnchor::End };
        assert_eq!(alarm_when(end), trf("{span} before the end", &[("span", &trf("{n} min", &[("n", &10)]))]));
        assert_eq!(style_word(AlarmStyle::FullScreen), tr("Full screen"));
    }

    #[test]
    fn adding_and_removing_a_reminder_saves_the_event() {
        let mut editor = Editor::edit(&timed(), &calendars());
        let _ = editor.update(Message::ToggleAlarms);
        assert!(matches!(editor.popup, Some(Popup::Remind(_))));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the card shows");
        let _ = editor.update(Message::Remind(RemindEdit::Preset(5)));
        let _ = editor.update(Message::Remind(RemindEdit::Style(Some(AlarmStyle::FullScreen))));
        let trigger = AlarmTrigger::Before { minutes: 5, of: AlarmAnchor::Start };
        let saved = saves(&mut editor, Message::Remind(RemindEdit::Add));
        assert_eq!(saved.alarms, [Alarm { trigger, style: Some(AlarmStyle::FullScreen) }]);
        assert!(editor.popup.is_none(), "the card closes");
        let _ = editor.panel(&Anchor::new(), |m| m);
        let _ = store(&mut editor, saved, "2");
        let saved = saves(&mut editor, Message::RemoveAlarm(0));
        assert!(saved.alarms.is_empty());
    }

    #[test]
    fn a_stored_event_previews_its_reminder() {
        let mut editor = Editor::edit(&timed(), &calendars());
        assert!(
            matches!(editor.update(Message::PreviewReminder), Action::Preview(href) if Some(href.clone()) == timed().href)
        );
        let _ = editor.update(Message::ToggleAlarms);
        let _ = editor.update(Message::Escape);
        assert!(editor.popup.is_none(), "Escape closes the card first");
    }

    // Reminders and place (task TE.8).

    /// The day before at 18:00 of a 10:00 event is stored as 960 minutes before the start, counted
    /// in the event's zone, and its row says so.
    #[test]
    fn the_day_before_at_six_is_stored_as_960_minutes_before() {
        let mut event = timed();
        event.time_zone = Some("Europe/Berlin".into());
        event.start = EventTime::DateTime(at(2026, 10, 27, 9, 0).and_utc());
        event.end = Some(EventTime::DateTime(at(2026, 10, 27, 10, 0).and_utc()));
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.form.start_time, "10:00");
        editor.update(Message::ToggleAlarms);
        editor.update(Message::Remind(RemindEdit::DayBefore("18:00".into())));
        let saved = saves(&mut editor, Message::Remind(RemindEdit::Add));
        assert_eq!(saved.alarms[0].trigger, AlarmTrigger::Before { minutes: 960, of: AlarmAnchor::Start });
        assert_eq!(editor.alarm_lines(&Anchor::new(), true).len(), 2, "the reminder and \"Add reminder\"");
    }

    #[test]
    fn a_reminder_beyond_the_longest_lead_is_not_added() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::ToggleAlarms);
        editor.update(Message::Remind(RemindEdit::Unit(remind::Unit::Weeks)));
        editor.update(Message::Remind(RemindEdit::Amount("2".into())));
        assert!(matches!(editor.update(Message::Remind(RemindEdit::Add)), Action::None));
        assert!(matches!(editor.popup, Some(Popup::Remind(_))), "the card stays and says why");
        assert!(editor.form.alarms.is_empty());
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the card shows");
        editor.update(Message::Remind(RemindEdit::Cancel));
        assert!(editor.popup.is_none());
    }

    /// A stored event with maps on (the settings as the app hands them over).
    fn with_maps() -> Editor {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.set_maps(Maps { enabled: true, ..Maps::default() });
        editor
    }

    fn hamburg() -> Place {
        Place {
            label: "Hafenstraße 4".into(),
            detail: "20359 Hamburg, Germany".into(),
            geo: Geo { lat: 53.5458, lon: 9.9672 },
            kind: torro_ipc::PlaceKind::Search,
        }
    }

    /// Typing never searches; Enter or "Search" does.
    #[test]
    fn there_is_no_search_without_enter() {
        let mut editor = with_maps();
        for typed in ["H", "Ha", "Hafenstr 4 hamb"] {
            assert!(matches!(editor.update(Message::Location(typed.into())), Action::None), "{typed}");
        }
        assert!(editor.searched.is_none());
        assert!(matches!(editor.update(Message::SearchPlaces), Action::SearchPlaces(q) if q == "Hafenstr 4 hamb"));
        assert_eq!(editor.searched.as_deref(), Some("Hafenstr 4 hamb"));
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    /// Picking a suggestion sets location and coordinates; typing on clears the coordinates.
    #[test]
    fn picking_sets_geo_and_typing_clears_it() {
        let mut editor = with_maps();
        editor.update(Message::Location("Hafenstr 4 hamb".into()));
        let _ = editor.update(Message::SearchPlaces);
        // An answer to another text is not this one's.
        editor.places_found("Somewhere", Ok(vec![hamburg()]));
        assert!(editor.popup.is_none());
        editor.places_found("Hafenstr 4 hamb", Ok(vec![hamburg()]));
        assert!(matches!(&editor.popup, Some(Popup::Places { places, .. }) if places.len() == 1));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the suggestions show");

        let saved = saves(&mut editor, Message::PickPlace(0));
        assert_eq!(saved.location.as_deref(), Some("Hafenstraße 4, 20359 Hamburg, Germany"));
        assert_eq!(saved.geo, Some(hamburg().geo));
        assert!(editor.popup.is_none());
        let _ = store(&mut editor, saved, "\"8\"");

        editor.update(Message::Location("Hafenstraße 4, 20359 Hamburg, Germany, 3rd floor".into()));
        let (saved, _) = saves_with(&mut editor, Message::Flush);
        assert_eq!(saved.geo, None, "the point no longer fits the text");
    }

    #[test]
    fn as_written_keeps_the_text_without_a_map() {
        let mut editor = with_maps();
        editor.update(Message::Location("Garden".into()));
        let _ = editor.update(Message::SearchPlaces);
        editor.places_found("Garden", Ok(Vec::new()));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("\"No places found\" shows");
        let (saved, _) = saves_with(&mut editor, Message::AsWritten);
        assert_eq!((saved.location.as_deref(), saved.geo), (Some("Garden"), None));
    }

    /// Maps off: no search; the first Enter says so once, quietly, and saves as Enter does.
    #[test]
    fn with_maps_off_enter_says_so_once() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::Location("Garden".into()));
        assert!(matches!(editor.update(Message::SearchPlaces), Action::Save(..)));
        assert_eq!(editor.place_note, Some(PlaceNote::MapsOff));
        editor.update(Message::Location("Garden 2".into()));
        let _ = editor.update(Message::SearchPlaces);
        assert_eq!(editor.place_note, None, "once");

        // The core says so too, when the settings changed meanwhile.
        let mut editor = with_maps();
        editor.update(Message::Location("Garden".into()));
        let _ = editor.update(Message::SearchPlaces);
        editor.places_found("Garden", Err(PlaceFailure::MapsOff));
        assert_eq!(editor.place_note, Some(PlaceNote::MapsOff));
        assert!(!editor.maps.enabled);
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    #[test]
    fn the_map_is_asked_for_once_per_point_and_darkness() {
        let mut event = timed();
        event.geo = Some(hamburg().geo);
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.map_wanted(true), None, "maps are off until the settings say otherwise");
        editor.set_maps(Maps { enabled: true, ..Maps::default() });
        assert_eq!(editor.map_wanted(true), Some(hamburg().geo));
        assert_eq!(editor.map_wanted(true), None, "asked already");
        editor.map_loaded(hamburg().geo, true, Some(image::Handle::from_rgba(1, 1, vec![0; 4])));
        assert!(editor.shown_map().is_some());
        assert_eq!(editor.map_wanted(true), None);
        assert_eq!(editor.map_wanted(false), Some(hamburg().geo), "the light appearance has its own map");
        editor.map_loaded(hamburg().geo, false, None);
        assert_eq!(editor.map_wanted(false), None, "a map that failed is not asked for again");
        let _ = editor.panel(&Anchor::new(), |m| m);

        editor.update(Message::Location("Elsewhere".into()));
        assert!(editor.shown_map().is_none() && editor.map_wanted(true).is_none(), "no point, no map");
    }

    #[test]
    fn open_in_maps_works_with_maps_off_and_read_only() {
        let mut event = timed();
        event.location = Some("Room 3, Hafenstraße 4".into());
        let calendars = vec![Collection { writable: false, ..calendar("work", "Work") }];
        let mut editor = Editor::edit(&event, &calendars);
        match editor.update(Message::OpenMaps) {
            Action::OpenMaps(url) => {
                assert!(url.contains("Room%203%2C%20Hafenstra%C3%9Fe%204"), "{url}");
                assert!(openable_map(&url));
            }
            other => panic!("expected OpenMaps, got {other:?}"),
        }
        assert!(matches!(editor.update(Message::OpenLink(place::OSM_COPYRIGHT.into())), Action::OpenLink(_)));
    }

    // Series, scope and the new fields (task TE.7).

    /// An occurrence of a weekly series on Tuesdays, keyed by its start, with changed occurrences a
    /// week before and one and two weeks after it.
    fn occurrence() -> Event {
        let start = EventTime::DateTime(to_utc(at(2026, 10, 13, 10, 0)));
        let week = |weeks: i64| EventTime::DateTime(to_utc(at(2026, 10, 13, 10, 0) + Duration::weeks(weeks)));
        let rule = repeat::shortlist(day(2026, 10, 13))[2].clone();
        Event {
            recurring: true,
            recurrence_id: Some(start),
            repeat: Some(Box::new(Repeat { rule: "FREQ=WEEKLY;BYDAY=TU".into(), ..rule })),
            exceptions: vec![week(-1), week(1), week(2)],
            ..stored("Weekly sync", start, Some(EventTime::DateTime(to_utc(at(2026, 10, 13, 10, 45)))))
        }
    }

    /// The scope an update hands out with its save.
    fn saves_with(editor: &mut Editor, message: Message) -> (Event, Option<SeriesScope>) {
        match editor.update(message) {
            Action::Save(event, scope) => (event, scope),
            other => panic!("expected Save, got {other:?}"),
        }
    }

    #[test]
    fn the_scope_of_the_band_goes_with_every_save_of_an_occurrence() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        assert_eq!(editor.scope, SeriesScope::Occurrence, "This event at first");
        assert!(editor.band().is_some());
        editor.update(Message::Location("Room 5".into()));
        let (event, scope) = saves_with(&mut editor, Message::Flush);
        assert_eq!(scope, Some(SeriesScope::Occurrence));
        assert_eq!(event.recurrence_id, occurrence().recurrence_id, "the key as read");
        assert_eq!(event.repeat, occurrence().repeat, "the rule unchanged, as read");
        let _ = store(&mut editor, event, "\"8\"");

        editor.update(Message::Scope(SeriesScope::Following));
        editor.update(Message::Location("Room 6".into()));
        assert_eq!(saves_with(&mut editor, Message::Flush).1, Some(SeriesScope::Following));
        assert_eq!(editor.leave(), None, "nothing while the save is under way");

        // A single event saves without a scope, and has no band.
        let mut single = Editor::edit(&timed(), &calendars());
        assert!(single.band().is_none());
        single.update(Message::Location("Room 4".into()));
        assert_eq!(saves_with(&mut single, Message::Flush).1, None);
        assert_eq!(single.leave(), None);
        let mut leaving = Editor::edit(&occurrence(), &calendars());
        leaving.update(Message::Scope(SeriesScope::Series));
        leaving.update(Message::Location("Room 7".into()));
        assert_eq!(leaving.leave().map(|(_, scope)| scope), Some(Some(SeriesScope::Series)), "leaving keeps the scope");
    }

    #[test]
    fn a_change_typed_before_another_scope_is_chosen_keeps_its_scope() {
        // Typed for "This event", "All" clicked before the save: the title goes to this event only.
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Location("Room 5".into()));
        let (event, scope) = saves_with(&mut editor, Message::Scope(SeriesScope::Series));
        assert_eq!((event.location.as_deref(), scope), (Some("Room 5"), Some(SeriesScope::Occurrence)));
        assert_eq!(editor.scope, SeriesScope::Series, "the band shows the new scope");
        let _ = store(&mut editor, event, "\"8\"");
        editor.update(Message::Location("Room 6".into()));
        assert_eq!(saves_with(&mut editor, Message::Flush).1, Some(SeriesScope::Series), "what follows goes to all");

        // And back: typed for "All", "This event" clicked before the save.
        let mut back = Editor::edit(&occurrence(), &calendars());
        back.update(Message::Scope(SeriesScope::Series));
        back.update(Message::Location("Room 7".into()));
        assert_eq!(saves_with(&mut back, Message::Scope(SeriesScope::Occurrence)).1, Some(SeriesScope::Series));

        // While a save is under way, a change typed since keeps the scope until the answer.
        let mut busy = Editor::edit(&occurrence(), &calendars());
        busy.update(Message::Location("Room 8".into()));
        let (event, _) = saves_with(&mut busy, Message::Flush);
        busy.update(Message::Location("Room 9".into()));
        assert!(matches!(busy.update(Message::Scope(SeriesScope::Series)), Action::None));
        assert_eq!(busy.scope, SeriesScope::Occurrence, "the scope waits for the answer");
        let (event, scope) = match store(&mut busy, event, "\"9\"") {
            Action::Save(event, scope) => (event, scope),
            other => panic!("expected the pending change to be saved, got {other:?}"),
        };
        assert_eq!((event.location.as_deref(), scope), (Some("Room 9"), Some(SeriesScope::Occurrence)));
        let _ = store(&mut busy, event, "\"10\"");
        assert!(matches!(busy.update(Message::Scope(SeriesScope::Series)), Action::None), "nothing left to save");
        assert_eq!(busy.scope, SeriesScope::Series);

        // The second review of PR #294: a date moved for this event alone does not move the rule of
        // the series once "All" is chosen; after the answer nothing more goes out.
        let mut moved = Editor::edit(&occurrence(), &calendars());
        moved.update(Message::StartDate("14.10.2026".into()));
        moved.update(Message::EndDate("14.10.2026".into()));
        let (event, scope) = saves_with(&mut moved, Message::Scope(SeriesScope::Series));
        assert_eq!(scope, Some(SeriesScope::Occurrence));
        assert_eq!(moved.form.repeat.as_ref(), occurrence().repeat.as_deref(), "the rule stays on Tuesdays");
        let answer = Event { recurrence_id: occurrence().recurrence_id, ..event };
        assert!(matches!(store(&mut moved, answer, "\"11\""), Action::None), "no save for all events follows");

        // A save that fails takes the band back: the next change goes to this event, not to all.
        let mut failing = Editor::edit(&occurrence(), &calendars());
        failing.update(Message::Location("Room 12".into()));
        let _ = saves_with(&mut failing, Message::Scope(SeriesScope::Series));
        failing.save_failed();
        assert_eq!(failing.scope, SeriesScope::Occurrence);
        // "All" clicked after the failure sends the failed change again with "This event" first, so
        // the next change does not carry it to the whole series (the review of PR #296).
        let (retried, scope) = saves_with(&mut failing, Message::Scope(SeriesScope::Series));
        assert_eq!((retried.location.as_deref(), scope), (Some("Room 12"), Some(SeriesScope::Occurrence)));
        failing.save_failed();
        assert_eq!(failing.scope, SeriesScope::Occurrence);
        failing.update(Message::Location("Room 13".into()));
        assert_eq!(saves_with(&mut failing, Message::Flush).1, Some(SeriesScope::Occurrence));
    }

    #[test]
    fn the_repeat_of_an_occurrence_changes_only_for_following_or_all_events() {
        let start = day(2026, 10, 13);
        let mut editor = Editor::edit(&occurrence(), &calendars());
        let daily = Box::new(repeat::shortlist(start)[0].clone());
        // "This event": the row is off.
        assert!(matches!(editor.update(Message::ToggleRepeat), Action::None));
        assert!(editor.popup.is_none());
        assert!(matches!(editor.update(Message::PickRepeat(Some(daily.clone()))), Action::None));
        assert!(matches!(editor.update(Message::OpenCustom), Action::None) && editor.popup.is_none());

        editor.update(Message::Scope(SeriesScope::Following));
        editor.update(Message::ToggleRepeat);
        assert_eq!(editor.popup, Some(Popup::Repeat));
        assert!(
            matches!(editor.update(Message::PickRepeat(None)), Action::None),
            "following events cannot end the series"
        );
        let (event, scope) = saves_with(&mut editor, Message::PickRepeat(Some(daily)));
        assert_eq!(
            (event.repeat.map(|r| r.freq), scope),
            (Some(torro_pim::model::RepeatFreq::Daily), Some(SeriesScope::Following))
        );
        assert!(editor.popup.is_none(), "picking closes the menu");

        // All events can end it.
        let mut all = Editor::edit(&occurrence(), &calendars());
        all.update(Message::Scope(SeriesScope::Series));
        let (event, scope) = saves_with(&mut all, Message::PickRepeat(None));
        assert!(!event.recurring, "ending the series says so: no rule and not recurring");
        assert_eq!((event.repeat, scope), (None, Some(SeriesScope::Series)));

        // The same rule again saves nothing, and keeps the stored text.
        let mut same = Editor::edit(&occurrence(), &calendars());
        same.update(Message::Scope(SeriesScope::Series));
        let weekly = Box::new(repeat::shortlist(start)[2].clone());
        assert!(matches!(same.update(Message::PickRepeat(Some(weekly))), Action::None));

        // A single event and a draft choose freely, also "Never".
        let mut draft = Editor::new_event(&calendars(), start, Some(NaiveTime::from_hms_opt(10, 0, 0).unwrap()));
        set_title(&mut draft, "Sync");
        let (created, scope) =
            saves_with(&mut draft, Message::PickRepeat(Some(Box::new(repeat::shortlist(start)[4].clone()))));
        assert!(created.repeat.is_some() && scope.is_none());
    }

    /// The second review of #278, finding 6: "Never" with "All", then "Following" – the stored rule
    /// comes back, and no save with another scope than "All" ends the series. While the save that
    /// ends it is under way, the scope stays.
    #[test]
    fn never_goes_out_only_with_all_events() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Series));
        let (ending, scope) = saves_with(&mut editor, Message::PickRepeat(None));
        assert_eq!((ending.repeat.is_none(), ending.recurring, scope), (true, false, Some(SeriesScope::Series)));
        assert!(matches!(editor.update(Message::Scope(SeriesScope::Following)), Action::None));
        assert_eq!(editor.scope, SeriesScope::Series, "the scope stays while the series is being ended");
        // The save failed: another scope sends the end again with "All" first, and the scope stays
        // while it is under way (the review of PR #296).
        editor.save_failed();
        let (again, scope) = saves_with(&mut editor, Message::Scope(SeriesScope::Following));
        assert_eq!((again.repeat.is_none(), scope), (true, Some(SeriesScope::Series)));
        assert_eq!(editor.scope, SeriesScope::Series);
        editor.save_failed();
        // With the rule picked again nothing is pending: now the scope may change.
        editor.update(Message::PickRepeat(occurrence().repeat.clone()));
        assert!(matches!(editor.update(Message::Scope(SeriesScope::Following)), Action::None));
        assert_eq!(editor.scope, SeriesScope::Following);
        assert_eq!(editor.form.repeat.as_ref(), occurrence().repeat.as_deref(), "the stored rule is back");
        editor.update(Message::Location("Room 8".into()));
        let (event, scope) = saves_with(&mut editor, Message::Flush);
        assert_eq!(scope, Some(SeriesScope::Following));
        assert!(event.recurring && event.repeat.is_some(), "the series goes on: {event:?}");

        // However the form lost its rule, only "All" sends the end of the series.
        let mut forced = Editor::edit(&occurrence(), &calendars());
        forced.update(Message::Scope(SeriesScope::Following));
        forced.form.repeat = None;
        let built = forced.check().expect("valid");
        assert!(built.recurring, "Following keeps the series");
        forced.scope = SeriesScope::Series;
        assert!(!forced.check().expect("valid").recurring, "All ends it");
    }

    /// The second review of #278, findings 2 and 3: on an occurrence moved by itself (10:00 to
    /// 14:00), "Following" with a new title resets nothing, as the core counts it; another time resets
    /// the changed occurrences after it, as the core does.
    #[test]
    fn following_on_a_moved_occurrence_counts_as_the_core_does() {
        let key = occurrence().recurrence_id.expect("a key");
        let moved = Event {
            start: EventTime::DateTime(to_utc(at(2026, 10, 13, 14, 0))),
            end: Some(EventTime::DateTime(to_utc(at(2026, 10, 13, 14, 45)))),
            exceptions: [occurrence().exceptions, vec![key]].concat(),
            ..occurrence()
        };
        let mut editor = Editor::edit(&moved, &calendars());
        editor.update(Message::Scope(SeriesScope::Following));
        set_title(&mut editor, "Product sync");
        assert_eq!(editor.resets(), 0, "a new title keeps the clock of the series");
        editor.update(Message::StartTime("13:30".into()));
        assert_eq!(editor.resets(), 2, "a new time: the two changed occurrences after it");
        editor.update(Message::StartTime("14:00".into()));
        assert_eq!(editor.resets(), 0, "back at its own time");
    }

    #[test]
    fn going_back_to_this_event_saves_a_rule_with_the_scope_it_was_chosen_for() {
        // A rule chosen for "Following" goes out with "Following" before "This event" takes over
        // (the second review of PR #294); the answer brings the rule as stored.
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Following));
        let daily = repeat::shortlist(day(2026, 10, 13))[0].clone();
        editor.form.repeat = Some(daily.clone());
        let (event, scope) = saves_with(&mut editor, Message::Scope(SeriesScope::Occurrence));
        assert_eq!((event.repeat.as_deref(), scope), (Some(&daily), Some(SeriesScope::Following)));
        assert_eq!(editor.form.repeat.as_ref(), Some(&daily), "not taken back while the save is under way");
        assert!(matches!(store(&mut editor, event, "\"8\""), Action::None), "nothing more goes out");
        assert_eq!(editor.scope, SeriesScope::Occurrence);
        // Without a save under way, "This event" still shows the stored rule.
        let mut idle = Editor::edit(&occurrence(), &calendars());
        idle.update(Message::Scope(SeriesScope::Following));
        idle.update(Message::Scope(SeriesScope::Occurrence));
        assert_eq!(idle.form.repeat.as_ref(), occurrence().repeat.as_deref());
    }

    #[test]
    fn the_custom_card_sets_the_rule_on_done_and_leaves_it_on_cancel() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Series));
        editor.update(Message::OpenCustom);
        assert!(matches!(editor.popup, Some(Popup::Custom(_))));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the card shows");
        editor.update(Message::Custom(CustomEdit::Every("2".into())));
        assert!(matches!(editor.update(Message::Custom(CustomEdit::Cancel)), Action::None));
        assert!(editor.popup.is_none());
        assert_eq!(editor.form.repeat.as_ref(), occurrence().repeat.as_deref(), "Cancel leaves the rule");

        editor.update(Message::OpenCustom);
        editor.update(Message::Custom(CustomEdit::Every("x".into())));
        assert!(matches!(editor.update(Message::Custom(CustomEdit::Done)), Action::None));
        assert!(editor.popup.is_some(), "a number that cannot be read keeps the card open");
        editor.update(Message::Custom(CustomEdit::Every("2".into())));
        editor.update(Message::Custom(CustomEdit::Ends(repeat::Ends::After)));
        editor.update(Message::Custom(CustomEdit::Count("5".into())));
        let (event, scope) = saves_with(&mut editor, Message::Custom(CustomEdit::Done));
        let rule = event.repeat.expect("a rule");
        assert_eq!(
            (rule.interval, rule.end, scope),
            (2, torro_pim::model::RepeatEnd::Count(5), Some(SeriesScope::Series))
        );
        assert!(editor.popup.is_none());
    }

    /// Task TE.7: before a change of "Following" is saved, the band counts the changed occurrences
    /// it resets – exactly when time, length or rule change (`series::resets`).
    #[test]
    fn the_band_counts_the_changed_events_a_change_of_following_resets() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::StartTime("09:30".into()));
        assert_eq!(editor.resets(), 0, "this event alone resets nothing");
        editor.update(Message::Scope(SeriesScope::Following));
        assert_eq!(editor.resets(), 2, "the two changed occurrences after this one");
        editor.update(Message::StartTime("10:00".into()));
        assert_eq!(editor.resets(), 0, "back at its time");
        editor.update(Message::EndTime("11:00".into()));
        assert_eq!(editor.resets(), 2, "a new length");
        editor.update(Message::EndTime("10:45".into()));
        set_title(&mut editor, "Weekly product sync");
        assert_eq!(editor.resets(), 0, "a new title keeps the grid");
        editor.form.repeat = Some(repeat::shortlist(day(2026, 10, 13))[0].clone());
        assert_eq!(editor.resets(), 2, "a new rule");
        editor.update(Message::StartTime("25:00".into()));
        assert_eq!(editor.resets(), 0, "a form that cannot be saved changes nothing yet");

        // All events: only ending the series resets, then every other changed occurrence.
        let mut all = Editor::edit(&occurrence(), &calendars());
        all.update(Message::Scope(SeriesScope::Series));
        all.update(Message::StartTime("09:30".into()));
        assert_eq!(all.resets(), 0, "the changed occurrences move along");
        all.form.repeat = None;
        assert_eq!(all.resets(), 3);
        let _ = all.band_note().expect("the band says so");

        // The core's count after the save.
        let mut saved = Editor::edit(&occurrence(), &calendars());
        saved.update(Message::Scope(SeriesScope::Following));
        assert!(saved.band_note().is_some(), "with Following it says what a change of time would reset");
        saved.update(Message::StartTime("09:30".into()));
        let (event, _) = saves_with(&mut saved, Message::Flush);
        let _ = saved.stored(&Event { exceptions: Vec::new(), ..event }, 2);
        assert_eq!((saved.resets(), saved.reset), (0, 2));
        assert!(saved.band_note().is_some());
        saved.update(Message::Scope(SeriesScope::Series));
        assert_eq!(saved.reset, 0, "a new scope starts over");
    }

    #[test]
    fn after_a_split_the_editor_goes_on_with_the_new_series() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Following));
        editor.update(Message::StartTime("09:30".into()));
        let (sent, _) = saves_with(&mut editor, Message::Flush);
        // The core answers with the first occurrence of the new series: new uid and href, the rule
        // written its own way.
        let mut answer = Event {
            uid: "split-1".into(),
            href: Some("/work/split-1.ics".into()),
            etag: Some("\"1\"".into()),
            ..sent.clone()
        };
        answer.repeat = Some(Box::new(Repeat {
            rule: "FREQ=WEEKLY;BYDAY=TU;COUNT=3".into(),
            end: torro_pim::model::RepeatEnd::Count(3),
            ..*sent.repeat.clone().expect("a rule")
        }));
        assert!(editor.awaits(&answer), "a new uid after a split is still our answer");
        assert!(matches!(editor.stored(&answer, 0), Action::None), "nothing left to save");
        assert_eq!(editor.event().and_then(|e| e.href.as_deref()), Some("/work/split-1.ics"));
        assert_eq!(editor.form.repeat.as_ref(), answer.repeat.as_deref(), "the rule as stored");
        editor.update(Message::Location("Room 9".into()));
        let (next, _) = saves_with(&mut editor, Message::Flush);
        assert_eq!((next.href.as_deref(), next.etag.as_deref()), (Some("/work/split-1.ics"), Some("\"1\"")));
        assert_eq!(next.repeat, answer.repeat, "the stored rule goes back unchanged");
    }

    /// Review of #278 (6): an occurrence whose rule could not be read (`recurring` without `repeat`)
    /// changes alone – a save with "Following" or "All" would carry no rule and end the series.
    #[test]
    fn an_occurrence_without_a_readable_rule_changes_alone() {
        let unread = Event { repeat: None, ..occurrence() };
        let mut editor = Editor::edit(&unread, &calendars());
        for scope in [SeriesScope::Following, SeriesScope::Series] {
            assert!(matches!(editor.update(Message::Scope(scope)), Action::None));
            assert_eq!(editor.scope, SeriesScope::Occurrence, "{scope:?} is off");
        }
        assert!(matches!(editor.update(Message::ToggleRepeat), Action::None) && editor.popup.is_none());
        assert!(matches!(editor.update(Message::PickRepeat(None)), Action::None));
        assert!(editor.band().is_some(), "the band still shows, with the reason");
        editor.update(Message::StartTime("09:30".into()));
        let (event, scope) = saves_with(&mut editor, Message::Flush);
        assert_eq!((event.repeat, scope), (None, Some(SeriesScope::Occurrence)));
        assert!(event.recurring, "a rule it could not read stays: the series goes on");
        // Never with another scope, however it got there.
        editor.scope = SeriesScope::Series;
        assert_eq!(editor.save_scope(), Some(SeriesScope::Occurrence));
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    /// Review of #278 (7): with "Following" or "All", a new start date moves a rule whose days came
    /// from the old one (Google, Apple); a rule the person picked stays; a rule on days of its own
    /// stays and the band warns before the save.
    #[test]
    fn a_new_date_moves_a_rule_taken_from_the_old_one() {
        let wednesday = vec![RepeatDay { day: Weekday::Wednesday, nth: None }];
        let mut alone = Editor::edit(&occurrence(), &calendars());
        alone.update(Message::StartDate("14.10.2026".into()));
        assert_eq!(alone.form.repeat.as_ref(), occurrence().repeat.as_deref(), "This event keeps the rule");
        // A date typed for this event alone is no change of the series: choosing "Following" first
        // asks for the end date, which is before the start now (the second review of PR #294).
        assert!(matches!(alone.update(Message::Scope(SeriesScope::Following)), Action::Focus(_)));
        assert_eq!(alone.scope, SeriesScope::Occurrence);

        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Following));
        editor.update(Message::StartDate("14.10.2026".into()));
        assert_eq!(editor.form.repeat.as_ref().map(|r| r.weekdays.clone()), Some(wednesday.clone()), "taken over");
        // Half-typed dates on the way do not stick: the rule is always moved from the stored one.
        for typed in ["1", "15.10.", "14.10.2026"] {
            editor.update(Message::StartDate(typed.into()));
        }
        editor.update(Message::EndDate("14.10.2026".into()));
        let (event, scope) = saves_with(&mut editor, Message::Flush);
        assert_eq!(event.repeat.map(|r| r.weekdays), Some(wednesday.clone()));
        assert_eq!(
            (event.start, scope),
            (EventTime::DateTime(to_utc(at(2026, 10, 14, 10, 0))), Some(SeriesScope::Following))
        );
        assert!(!editor.date_off_rule(), "the rule fits the new date");

        // All events: the changed occurrences move along to Wednesdays and stay.
        let mut all = Editor::edit(&occurrence(), &calendars());
        all.update(Message::Scope(SeriesScope::Series));
        all.update(Message::StartDate("14.10.2026".into()));
        all.update(Message::EndDate("14.10.2026".into()));
        assert_eq!(all.form.repeat.as_ref().map(|r| r.weekdays.clone()), Some(wednesday));
        assert!(all.check().is_ok() && all.changes_grid(&occurrence()), "a change of the grid");
        assert_eq!(all.resets(), 0);

        // A rule the person picked stays.
        let mut picked = Editor::edit(&occurrence(), &calendars());
        picked.update(Message::Scope(SeriesScope::Following));
        let daily = repeat::shortlist(day(2026, 10, 13))[0].clone();
        let _ = saves_with(&mut picked, Message::PickRepeat(Some(Box::new(daily.clone()))));
        picked.update(Message::StartDate("15.10.2026".into()));
        assert_eq!(picked.form.repeat, Some(daily));

        // A rule on days of its own (Tue and Thu) stays; the band warns while the date is off it.
        let days = |all: &[Weekday]| all.iter().map(|day| RepeatDay { day: *day, nth: None }).collect::<Vec<_>>();
        let rule = Repeat {
            weekdays: days(&[Weekday::Tuesday, Weekday::Thursday]),
            rule: String::new(),
            ..repeat::shortlist(day(2026, 10, 13))[2].clone()
        };
        let own = Event { repeat: Some(Box::new(rule.clone())), ..occurrence() };
        let mut editor = Editor::edit(&own, &calendars());
        editor.update(Message::Scope(SeriesScope::Series));
        editor.update(Message::StartDate("14.10.2026".into()));
        assert_eq!(editor.form.repeat.as_ref(), Some(&rule));
        assert!(editor.date_off_rule() && editor.band_note().is_some(), "Wednesday is none of its days");
        editor.update(Message::StartDate("15.10.2026".into()));
        assert!(!editor.date_off_rule(), "Thursday is");
    }

    /// Review of #278 (8): "All" with a new rule or a move to other days counts before the save the
    /// changed occurrences the new series no longer has (the condition of `series::resets`).
    #[test]
    fn all_with_a_new_rule_counts_what_it_resets_before_the_save() {
        let start = day(2026, 10, 13);
        let mut all = Editor::edit(&occurrence(), &calendars());
        all.update(Message::Scope(SeriesScope::Series));
        all.form.repeat = Some(repeat::shortlist(start)[3].clone());
        assert_eq!(all.resets(), 2, "every two weeks: a week before and a week after fall out of step");
        let _ = all.band_note().expect("the band says so before the save");
        all.form.repeat = Some(repeat::shortlist(start)[0].clone());
        assert_eq!(all.resets(), 0, "every day still has them all");
        all.form.repeat = Some(repeat::shortlist(start)[4].clone());
        assert_eq!(all.resets(), 3, "the second Tuesday of the month has none of them");

        // Days of its own, moved to Wednesday: the Tuesdays become Wednesdays, which it does not have.
        let mut moved = Editor::edit(&occurrence(), &calendars());
        moved.update(Message::Scope(SeriesScope::Series));
        let tue_thu =
            vec![RepeatDay { day: Weekday::Tuesday, nth: None }, RepeatDay { day: Weekday::Thursday, nth: None }];
        moved.form.repeat =
            Some(Repeat { weekdays: tue_thu, rule: String::new(), ..repeat::shortlist(start)[2].clone() });
        moved.update(Message::StartDate("14.10.2026".into()));
        moved.update(Message::EndDate("14.10.2026".into()));
        assert_eq!(moved.resets(), 3);
    }

    /// Review of #278 (9): a location longer than a search may be is not sent, and every failure is
    /// said in a translated sentence, never in the core's English words.
    #[test]
    fn a_too_long_place_is_not_searched_and_failures_are_said_in_words() {
        let mut editor = with_maps();
        editor.update(Message::Location("x".repeat(MAX_PLACE_QUERY_CHARS + 1)));
        assert!(!matches!(editor.update(Message::SearchPlaces), Action::SearchPlaces(_)));
        assert_eq!(editor.place_note, Some(PlaceNote::TooLong));
        assert_eq!(
            PlaceNote::TooLong.words(),
            trf("A place search takes at most {n} characters", &[("n", &MAX_PLACE_QUERY_CHARS)])
        );
        let longest = "ä".repeat(MAX_PLACE_QUERY_CHARS);
        editor.update(Message::Location(longest.clone()));
        assert!(matches!(editor.update(Message::SearchPlaces), Action::SearchPlaces(_)), "characters, not bytes");
        editor.places_found(&longest, Err(PlaceFailure::from_code("error")));
        assert_eq!(
            editor.place_note.as_ref().map(PlaceNote::words),
            Some(tr("The place search did not work. Try again later.").to_string())
        );
        let _ = editor.panel(&Anchor::new(), |m| m);
        let codes = [
            ("busy", tr("Too many place searches. Try again in a moment.")),
            ("offline", tr("The place search service cannot be reached.")),
            ("something new", tr("The place search did not work. Try again later.")),
        ];
        for (code, words) in codes {
            assert_eq!(PlaceFailure::from_code(code).message(), words, "{code}");
        }
        assert_eq!(PlaceFailure::from_code("unsupported"), PlaceFailure::MapsOff);
    }

    #[test]
    fn a_series_switches_between_all_day_and_timed_only_for_this_event() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Scope(SeriesScope::Series));
        assert!(matches!(editor.update(Message::AllDay(true)), Action::None));
        assert!(!editor.form.all_day, "the switch is off");
        // A switch made for this event alone is not sent for all of them: choosing "All" saves it for
        // this event first.
        editor.update(Message::Scope(SeriesScope::Occurrence));
        editor.form.all_day = true;
        let (event, scope) = saves_with(&mut editor, Message::Scope(SeriesScope::Series));
        assert!(event.all_day() && scope == Some(SeriesScope::Occurrence));
        assert_eq!(problem(&editor), Problem::AllDaySeries, "until the answer, the form is no change for all");
    }

    #[test]
    fn deleting_an_occurrence_asks_which_events() {
        let mut editor = Editor::edit(&occurrence(), &calendars());
        editor.update(Message::Delete);
        assert_eq!(editor.footer().len(), 1, "one question with a button per scope");
        assert!(matches!(editor.update(Message::Escape), Action::None) && !editor.confirm_delete, "Escape cancels");
        for scope in [SeriesScope::Occurrence, SeriesScope::Following, SeriesScope::Series] {
            editor.update(Message::Delete);
            match editor.update(Message::ConfirmDelete(Some(scope))) {
                Action::Delete(event, chosen) => assert_eq!((event, chosen), (occurrence(), Some(scope))),
                other => panic!("expected Delete, got {other:?}"),
            }
        }
    }

    #[test]
    fn show_as_status_and_link_are_saved_at_once_or_when_typed() {
        let mut editor = Editor::edit(&timed(), &calendars());
        editor.update(Message::ToggleShowAs);
        assert_eq!(editor.popup, Some(Popup::ShowAs));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the menu shows");
        let (event, _) = saves_with(&mut editor, Message::ShowAs(Transparency::Transparent));
        assert_eq!(event.transparency, Transparency::Transparent);
        let _ = store(&mut editor, event, "\"8\"");
        editor.update(Message::ToggleStatus);
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the menu shows");
        let (event, _) = saves_with(&mut editor, Message::Status(EventStatus::Tentative));
        assert_eq!(event.status, Some(EventStatus::Tentative));
        let _ = store(&mut editor, event, "\"9\"");
        editor.update(Message::Url("  https://example.org/agenda ".into()));
        let (event, _) = saves_with(&mut editor, Message::Flush);
        assert_eq!(event.url.as_deref(), Some("https://example.org/agenda"));
        assert!(web_link("https://example.org") && !web_link("mailto:anna@example.org") && !web_link("file:///etc"));
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    #[test]
    fn another_zone_keeps_the_wall_clock() {
        let mut event = timed();
        event.time_zone = Some("Europe/Berlin".into());
        event.start = EventTime::DateTime(at(2026, 10, 27, 9, 0).and_utc());
        event.end = Some(EventTime::DateTime(at(2026, 10, 27, 10, 0).and_utc()));
        let mut editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.form.start_time, "10:00", "on Berlin's clock");
        assert!(matches!(editor.update(Message::ToggleZones), Action::Focus(ZONE_SEARCH_ID)));
        editor.update(Message::ZoneQuery("new york".into()));
        let _ = editor.layer(&Anchor::new(), |m| m).expect("the menu shows");
        let (saved, _) = saves_with(&mut editor, Message::ZoneFirst);
        assert_eq!(saved.time_zone.as_deref(), Some("America/New_York"));
        assert_eq!(saved.start, EventTime::DateTime(at(2026, 10, 27, 14, 0).and_utc()), "10:00 in New York");
        assert!(editor.popup.is_none());
    }

    #[test]
    fn people_are_only_shown() {
        let anna = Attendee {
            name: Some("Anna Berg".into()),
            email: Some("anna@example.org".into()),
            partstat: PartStat::Accepted,
            role: torro_pim::model::AttendeeRole::Chair,
        };
        let ben = Attendee {
            name: None,
            email: Some("ben@example.org".into()),
            partstat: PartStat::Declined,
            ..anna.clone()
        };
        let event = Event { organizer: Some(Box::new(anna.clone())), attendees: vec![anna.clone(), ben], ..timed() };
        let editor = Editor::edit(&event, &calendars());
        assert_eq!(editor.people_lines().len(), 2, "the row and the note about the server");
        assert!(Editor::edit(&timed(), &calendars()).people_lines().is_empty());
        assert_eq!((initials(&anna).as_deref(), person_name(&anna).as_deref()), (Some("AB"), Some("Anna Berg")));
        let _ = editor.panel(&Anchor::new(), |m| m);
    }

    #[test]
    fn the_panel_of_an_occurrence_builds_with_every_menu() {
        let anchor = Anchor::new();
        let mut editor = Editor::edit(&occurrence(), &calendars());
        let _ = editor.panel(&anchor, |m| m);
        editor.update(Message::Scope(SeriesScope::Following));
        for open in [Message::ToggleRepeat, Message::ToggleZones, Message::ToggleShowAs, Message::ToggleStatus] {
            editor.update(open);
            assert!(editor.popup.is_some());
            let _ = editor.panel(&anchor, |m| m);
            let _ = editor.layer(&anchor, |m| m).expect("a layer");
            assert!(
                matches!(editor.update(Message::Escape), Action::None) && editor.popup.is_none(),
                "Escape closes it"
            );
        }
        editor.update(Message::Delete);
        let _ = editor.panel(&anchor, |m| m);
    }
}
