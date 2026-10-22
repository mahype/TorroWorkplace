//! The shared model, shaped after iCalendar (RFC 5545) so CalDAV maps one to one and
//! other providers map onto it.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A calendar or task list on an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    /// `<account-id>/<provider id>`: the account it belongs to, then the provider's own id (for
    /// CalDAV the collection URL). Unique across all accounts; see [`crate::account`].
    pub id: String,
    pub name: String,
    pub kind: CollectionKind,
    /// `#RRGGBB` as the server reports it, if any.
    pub color: Option<String>,
    /// The user may add, change and delete items. Shared read-only calendars and
    /// Nextcloud's contact birthdays are not writable.
    #[serde(default = "writable_by_default")]
    pub writable: bool,
    /// Where the items come from when the collection is not one of a server: the definition of
    /// a GitHub filter list (task G.2). `None` for every calendar and task list of an account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<GitHubFilter>,
}

fn writable_by_default() -> bool {
    true
}

/// A GitHub filter list: saved GitHub searches whose matches the core keeps as a task list
/// (`github:<login>/<list-id>`, kind todos, never writable). The definition lives in the core,
/// not on GitHub; the items are what the searches find today plus the pinned ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubFilter {
    /// The searches, as GitHub's search box takes them (`is:open is:issue repo:owner/name
    /// label:bug`). Within one search everything must match; the list holds the matches of any.
    pub queries: Vec<String>,
    /// Issues and pull requests in the list whatever the searches find, as link ids
    /// (`github:<owner>/<repo>#<number>`, [`Link::id`]).
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Version of name, colour and searches. `tasks.update_filter_list` sends it back and is
    /// refused with `conflict` when the list changed meanwhile. Unpinning does not change it.
    pub revision: u64,
    /// A search found more than 1,000 matches; the list holds only the first 1,000 of it, as
    /// GitHub hands out no more.
    #[serde(default)]
    pub truncated: bool,
    /// Why the last refresh from GitHub failed, if it did; the items from before stay.
    #[serde(default)]
    pub problem: Option<String>,
}

/// What a collection holds. A CalDAV calendar may hold both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionKind {
    Events,
    Todos,
    Both,
}

impl CollectionKind {
    pub fn has_events(self) -> bool {
        matches!(self, CollectionKind::Events | CollectionKind::Both)
    }

    pub fn has_todos(self) -> bool {
        matches!(self, CollectionKind::Todos | CollectionKind::Both)
    }
}

/// When an event starts or ends, or a task is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventTime {
    /// A calendar day (all-day entries, VALUE=DATE).
    Date(NaiveDate),
    /// An instant.
    DateTime(DateTime<Utc>),
}

/// A calendar entry (VEVENT). A recurring event appears once per occurrence, each with
/// the start of that occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    /// iCalendar UID; shared by all occurrences of a recurring event.
    pub uid: String,
    /// Id of the calendar it belongs to, as `calendar.list_calendars` reports it. Must name a
    /// calendar that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for an event not saved yet;
    /// pass the event back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for an event not saved yet.
    pub etag: Option<String>,
    /// Title of the event.
    pub summary: String,
    /// Start: a calendar day for all-day events, otherwise an instant in UTC.
    pub start: EventTime,
    /// Exclusive end (DTEND, or DTSTART + DURATION); `None` when the entry has neither.
    pub end: Option<EventTime>,
    pub location: Option<String>,
    pub description: Option<String>,
    /// One occurrence of a recurring series. Changing or deleting it needs a `scope`
    /// (`calendar.save_event`, `calendar.delete_event`, [`SeriesScope`]); without one the core
    /// answers `unsupported`, so a client that does not know series never overwrites one. `true`
    /// exactly when [`Event::recurrence_id`] or [`Event::repeat`] is set (task TE.3 fills both in).
    /// Sent as `false` with `repeat: None` and scope `series` or `following`, it ends the series
    /// ([`Event::repeat`]).
    pub recurring: bool,
    /// Whether the event blocks the time (iCalendar `TRANSP`): `opaque` (busy) or
    /// `transparent` (free, e.g. a reminder or "working from home"). Missing means `opaque`, as
    /// RFC 5545 says; send the value back as read so a change of other fields keeps it.
    #[serde(default)]
    pub transparency: Transparency,
    /// The event's own reminders (iCalendar `VALARM` with `ACTION:DISPLAY` or `AUDIO`), in the
    /// stored order; empty when it has none, and then the suite's default applies (task ER1.3).
    /// Mail reminders (`ACTION:EMAIL`) are the server's and not listed. Send the list back as
    /// read: a save rewrites the event's reminders only when it differs.
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    /// Which occurrence of a series this is (iCalendar `RECURRENCE-ID`): the start the rule gives
    /// it, which stays when the occurrence is moved. `None` for an event that is not part of a
    /// series. Send it back as read: the core finds the occurrence by `href` and this key.
    #[serde(default)]
    pub recurrence_id: Option<EventTime>,
    /// The rule of the series (iCalendar `RRULE` of the series' main event), the same on every
    /// occurrence; `None` for an event that does not repeat, and for an occurrence whose stored
    /// object the core could not read. Setting it on a single event makes it a series. `None` on an
    /// occurrence saved with `scope: series` or `following` ends the series only together with
    /// `recurring: false`; with `recurring: true` the stored rule stays, so an occurrence that came
    /// without its rule is never turned into a single event by being sent back.
    // Boxed, like `organizer`: an event lives in many futures and answers, and the rule and the
    // organiser would double its size although most events have neither (the wire is the same).
    #[serde(default)]
    pub repeat: Option<Box<Repeat>>,
    /// The time zone the event's times belong to, as an IANA name (`Europe/Berlin`, iCalendar
    /// `TZID` of `DTSTART`). `None`: the zone of the computer when saving; UTC when the server
    /// gave none. `start` and `end` stay instants in UTC either way; the zone decides at which
    /// local time a series repeats across a change to or from daylight saving time.
    #[serde(default)]
    pub time_zone: Option<String>,
    /// A web address that belongs to the event (iCalendar `URL`), e.g. the agenda. The apps open
    /// only `http` and `https` addresses. `None` when there is none.
    #[serde(default)]
    pub url: Option<String>,
    /// Whether the event takes place (iCalendar `STATUS`); `None` when the event does not say.
    #[serde(default)]
    pub status: Option<EventStatus>,
    /// Where the event is, as coordinates (iCalendar `GEO`, also read from Apple's
    /// `X-APPLE-STRUCTURED-LOCATION`). `None` when unknown; then only `location` names the place.
    #[serde(default)]
    pub geo: Option<Geo>,
    /// Who organises the event (iCalendar `ORGANIZER`); `None` for an event without invitees.
    /// Read only: saving never writes it, the server's copy stays.
    #[serde(default)]
    pub organizer: Option<Box<Attendee>>,
    /// The people invited (iCalendar `ATTENDEE`), in the stored order; empty when none. Read only,
    /// like [`Event::organizer`]: the suite sends no invitations.
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    /// The changed occurrences of the series (iCalendar `RECURRENCE-ID` of each exception in the
    /// stored object), the same on every occurrence; empty for an event that is not part of a series
    /// or a series without changed occurrences. Read only: the core fills it in from the stored
    /// object, and a save ignores it. The event editor counts with it how many changed events a
    /// change of "this and following" would reset, before it is made (`docs/termin-editor.md`,
    /// decision 4).
    #[serde(default)]
    pub exceptions: Vec<EventTime>,
}

impl Event {
    /// All-day entries carry a date without a time.
    pub fn all_day(&self) -> bool {
        matches!(self.start, EventTime::Date(_))
    }

    /// The event makes its time busy: opaque, not all-day and not cancelled. All-day entries
    /// (holidays, birthdays), transparent ones and cancelled ones (`STATUS:CANCELLED`, task TE.2)
    /// leave the time free, whatever else they say.
    pub fn busy(&self) -> bool {
        self.transparency == Transparency::Opaque && !self.all_day() && self.status != Some(EventStatus::Cancelled)
    }
}

/// Free or busy (iCalendar `TRANSP`, RFC 5545 section 3.8.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transparency {
    /// The time is busy (`TRANSP:OPAQUE`); also when the event says nothing.
    #[default]
    Opaque,
    /// The time stays free (`TRANSP:TRANSPARENT`).
    Transparent,
}

/// One reminder of an event (`docs/erinnerungen.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Alarm {
    /// When it fires.
    pub trigger: AlarmTrigger,
    /// How it shows; `None` means as its calendar is set to show reminders.
    #[serde(default)]
    pub style: Option<AlarmStyle>,
}

/// When a reminder fires (iCalendar `TRIGGER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlarmTrigger {
    /// `minutes` before the start or the end of the event (`-PT15M`, `RELATED=END`). Negative
    /// minutes fire after it; seconds are dropped (`-PT90S` is one minute before).
    Before {
        minutes: i64,
        #[serde(default)]
        of: AlarmAnchor,
    },
    /// At a fixed instant (`TRIGGER;VALUE=DATE-TIME`).
    At(DateTime<Utc>),
}

/// What a relative reminder counts from (iCalendar `RELATED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlarmAnchor {
    /// The start of the event, the default.
    #[default]
    Start,
    /// The end of the event.
    End,
}

/// How a reminder shows (Torro's own `X-TORRO-STYLE` in the `VALARM`; other programs ignore it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlarmStyle {
    /// A desktop notification and an inbox entry.
    Notification,
    /// Over the whole screen until the person reacts.
    FullScreen,
}

/// The rule of a series (iCalendar `RRULE`, RFC 5545 section 3.3.10), as far as the event editor
/// shows it (`docs/termin-editor.md`). A rule with parts the editor cannot show keeps them in
/// [`Repeat::rule`] and says so with [`Repeat::custom`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Repeat {
    /// How often: every day, week, month or year (`FREQ`).
    pub freq: RepeatFreq,
    /// Every how many days, weeks, months or years (`INTERVAL`), 1 or more; 1 when left out.
    #[serde(default = "every_one")]
    pub interval: u16,
    /// On which weekdays (`BYDAY`), each optionally the n-th of the month or year: `nth: 4` is the
    /// fourth, `nth: -1` the last. Empty: the weekday of the start (weekly) or none.
    #[serde(default)]
    pub weekdays: Vec<RepeatDay>,
    /// On which days of the month (`BYMONTHDAY`), 1 to 31 or -1 (the last) to -31. Empty: none.
    #[serde(default)]
    pub month_days: Vec<i8>,
    /// In which months (`BYMONTH`), 1 to 12. Empty: none.
    #[serde(default)]
    pub months: Vec<u8>,
    /// When the series ends: never, after a number of events, or at a day or instant.
    #[serde(default)]
    pub end: RepeatEnd,
    /// The rule as read from the server, without `RRULE:` (`FREQ=WEEKLY;BYDAY=TU`); empty for a
    /// rule a client builds. Only read back: a save writes the stored rule (custom, or the fields
    /// unchanged) or one made from the fields, never this text, except for a custom rule of an event
    /// that has no rule yet. Sent back unchanged it may be any stored rule; any other text must be
    /// one line of the `RRULE` grammar (RFC 5545 parts with valid values, `X-NAME=value` in plain
    /// ASCII), else the save is refused with `error`.
    #[serde(default)]
    pub rule: String,
    /// The rule has parts the fields above do not show (`BYSETPOS`, `BYHOUR`, `BYWEEKNO`,
    /// `RDATE` …): the editor shows it as "Custom rule" and can only replace it whole. Sent with
    /// `custom` set, the stored rule stays as it is, whatever the fields and [`Repeat::rule`] say;
    /// to change it, send the new fields with `custom: false`.
    #[serde(default)]
    pub custom: bool,
}

fn every_one() -> u16 {
    1
}

/// How often a series repeats (iCalendar `FREQ`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RepeatFreq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// One weekday of a rule (iCalendar `BYDAY`: `TU`, `4TU`, `-1FR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct RepeatDay {
    /// The weekday.
    pub day: Weekday,
    /// The n-th of the month (monthly) or year (yearly): 1 to 53, or -1 (the last) to -53; `None`
    /// for every such weekday.
    #[serde(default)]
    pub nth: Option<i8>,
}

/// A day of the week, in a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<chrono::Weekday> for Weekday {
    fn from(day: chrono::Weekday) -> Weekday {
        match day {
            chrono::Weekday::Mon => Weekday::Monday,
            chrono::Weekday::Tue => Weekday::Tuesday,
            chrono::Weekday::Wed => Weekday::Wednesday,
            chrono::Weekday::Thu => Weekday::Thursday,
            chrono::Weekday::Fri => Weekday::Friday,
            chrono::Weekday::Sat => Weekday::Saturday,
            chrono::Weekday::Sun => Weekday::Sunday,
        }
    }
}

impl From<Weekday> for chrono::Weekday {
    fn from(day: Weekday) -> chrono::Weekday {
        match day {
            Weekday::Monday => chrono::Weekday::Mon,
            Weekday::Tuesday => chrono::Weekday::Tue,
            Weekday::Wednesday => chrono::Weekday::Wed,
            Weekday::Thursday => chrono::Weekday::Thu,
            Weekday::Friday => chrono::Weekday::Fri,
            Weekday::Saturday => chrono::Weekday::Sat,
            Weekday::Sunday => chrono::Weekday::Sun,
        }
    }
}

/// When a series ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RepeatEnd {
    /// It goes on (neither `COUNT` nor `UNTIL`).
    #[default]
    Never,
    /// After this many events, the first one counted (`COUNT`).
    Count(u32),
    /// With the last event starting on this day or at this instant (`UNTIL`, inclusive).
    Until(EventTime),
}

/// Which events of a series a change or a delete is about (`calendar.save_event`,
/// `calendar.delete_event`, `docs/termin-editor.md` decisions 3 to 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeriesScope {
    /// Only this occurrence: changing it stores an exception, deleting it excludes its date.
    Occurrence,
    /// This occurrence and all after it: the series is split here (the old one ends the second
    /// before), and later exceptions may be reset when time or rule change.
    Following,
    /// Every event of the series: the series itself changes, or is deleted whole.
    Series,
}

/// Whether an event takes place (iCalendar `STATUS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventStatus {
    /// It takes place (`CONFIRMED`).
    Confirmed,
    /// Not settled yet (`TENTATIVE`).
    Tentative,
    /// Called off (`CANCELLED`); it keeps no time busy and never reminds.
    Cancelled,
}

/// A point on Earth in degrees (WGS 84), as iCalendar `GEO` gives it.
///
/// `Eq` holds because a `Geo` never holds NaN: JSON has no NaN, and whoever makes one from other
/// text (an iCalendar parser) goes through [`Geo::new`], which refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Geo {
    /// Latitude, -90 (south) to 90 (north).
    pub lat: f64,
    /// Longitude, -180 (west) to 180 (east).
    pub lon: f64,
}

impl Eq for Geo {}

impl Geo {
    /// The farthest north or south a map shows: the edge of the Web Mercator projection that map
    /// tiles use. `calendar.map_preview` and `calendar.search_places` refuse points beyond it.
    pub const MAP_MAX_LAT: f64 = 85.0511;

    /// The point at `lat`, `lon`, or `None` when one is not a finite number or out of range
    /// (latitude beyond ±90, longitude beyond ±180).
    pub fn new(lat: f64, lon: f64) -> Option<Geo> {
        (lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0).then_some(Geo { lat, lon })
    }

    /// Whether a map can show the point: latitude within ±[`Geo::MAP_MAX_LAT`], longitude within
    /// ±180, both finite.
    pub fn on_map(&self) -> bool {
        self.lat.is_finite() && self.lon.is_finite() && self.lat.abs() <= Self::MAP_MAX_LAT && self.lon.abs() <= 180.0
    }
}

/// Someone invited to an event, or its organiser (iCalendar `ATTENDEE`, `ORGANIZER`). Read only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Attendee {
    /// The display name (`CN`); `None` when the event gives none.
    #[serde(default)]
    pub name: Option<String>,
    /// The mail address, without `mailto:`; `None` when the event names the person otherwise.
    #[serde(default)]
    pub email: Option<String>,
    /// Their answer (`PARTSTAT`); `needs_action` when the event says nothing.
    #[serde(default)]
    pub partstat: PartStat,
    /// Their part in the event (`ROLE`); `required` when the event says nothing.
    #[serde(default)]
    pub role: AttendeeRole,
}

/// The answer of someone invited (iCalendar `PARTSTAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartStat {
    /// Not answered yet (`NEEDS-ACTION`), also when the event says nothing.
    #[default]
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
    /// Passed on to someone else (`DELEGATED`).
    Delegated,
}

/// The part someone invited has in an event (iCalendar `ROLE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttendeeRole {
    /// Leads it (`CHAIR`).
    Chair,
    /// Expected to come (`REQ-PARTICIPANT`), also when the event says nothing.
    #[default]
    Required,
    /// Welcome to come (`OPT-PARTICIPANT`).
    Optional,
    /// Only informed (`NON-PARTICIPANT`).
    NonParticipant,
}

/// A task (VTODO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Todo {
    /// iCalendar UID.
    pub uid: String,
    /// Id of the task list it belongs to, as `tasks.list_lists` reports it. Must name a list
    /// that can be written to when saving.
    pub collection: String,
    /// Where the item lives on the server (CalDAV URL). `None` only for a task not saved yet;
    /// pass the task back with its `href` and `etag` to change or delete it.
    pub href: Option<String>,
    /// Server version of the item, sent back on changes so the server can refuse stale writes.
    /// `None` only for a task not saved yet.
    pub etag: Option<String>,
    /// Title of the task.
    pub summary: String,
    /// When the task is due: a day or an instant. `None` when it has no due date.
    pub due: Option<EventTime>,
    /// iCalendar priority: 1 highest … 9 lowest, `None` undefined.
    pub priority: Option<u8>,
    pub completed: bool,
    /// UID of the parent task (RELATED-TO;RELTYPE=PARENT).
    pub parent: Option<String>,
    pub description: Option<String>,
    /// A web address that belongs to the task (iCalendar `URL`), e.g. the issue it is about.
    /// The apps open only `http` and `https` addresses. `None` when there is none.
    #[serde(default)]
    pub url: Option<String>,
    /// Free tags (iCalendar `CATEGORIES`), in the stored order. Empty when there are none;
    /// saving an empty list removes them. Empty tags are dropped, surrounding spaces trimmed.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Position in a list ordered by hand (`X-APPLE-SORT-ORDER`, as Nextcloud Tasks and Apple
    /// Reminders write it): smaller comes first. `None` when the task was never placed by hand.
    #[serde(default)]
    pub sort_order: Option<i64>,
    /// The item of another service the task stands for, e.g. a GitHub issue (`X-TORRO-LINK`).
    /// Only [`Link::id`] is stored with the task; the core fills in the rest when it reads.
    #[serde(default)]
    pub link: Option<Link>,
}

/// A task's link to an item of another service: a GitHub issue or pull request, or a mail.
///
/// The task stores only [`Link::id`]. What the service reports about the item is in
/// [`Link::github`] or [`Link::mail`]: filled in by the core when it reads and never written into
/// the stored task. Clients may send a link back as they got it; saving uses only the id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// Stable id of the linked item:
    ///
    /// - `github:<owner>/<repo>#<number>` for a GitHub issue or pull request, e.g.
    ///   `github:mahype/TorroWorkplace#123`;
    /// - `mail:<address>:<Message-ID>` for a mail (task E8.7a, [`Link::mail_ref`]): the mail
    ///   account's id (`mail:<address>`, as `accounts.list_accounts` lists it) and the mail's
    ///   `Message-ID` header without its angle brackets, as the core's mail store keeps it, e.g.
    ///   `mail:anna@mailbox.org:a1@example.org`. The Message-ID stays when the mail moves to another
    ///   folder;
    /// - `inbox:<entry id>` for the inbox entry a task was made of (task E3.5), valid while the
    ///   inbox keeps the entry.
    ///
    /// Ids of other forms are kept as they are.
    pub id: String,
    /// What GitHub reports for the item. `None` until the core fetched it, without a GitHub
    /// account, or when the id is not a GitHub one.
    #[serde(default)]
    pub github: Option<GitHubDetails>,
    /// What the core's mail store holds about the linked mail (task E8.7a): filled in only when
    /// the tasks are read, only for a caller that may read the mail account, never stored with the
    /// task. `None` for other ids, without that right, or when the store has no mail with this
    /// Message-ID (deleted, or the account is gone).
    #[serde(default)]
    pub mail: Option<Box<MailDetails>>,
}

/// The prefix of a GitHub [`Link::id`].
const GITHUB_PREFIX: &str = "github:";

/// The prefix of a mail [`Link::id`]; also the prefix of every mail account's id.
pub const MAIL_PREFIX: &str = "mail:";

/// A Message-ID as the core's mail store keeps it: without spaces and angle brackets around it
/// (`<a1@example.org>` → `a1@example.org`). Look a link's Message-ID up in this form, since a link
/// written by hand or by another program may still carry the brackets.
pub fn bare_message_id(message_id: &str) -> &str {
    let id = message_id.trim();
    id.strip_prefix('<').and_then(|id| id.strip_suffix('>')).unwrap_or(id).trim()
}

impl Link {
    /// The link with the id `id` alone, without details: what a stored task holds.
    pub fn new(id: impl Into<String>) -> Link {
        Link { id: id.into(), github: None, mail: None }
    }

    /// The link to the mail with `message_id` (its `Message-ID` header) in the mail account
    /// `account` (`mail:<address>`; a bare address gets the prefix), without details:
    /// `mail:<address>:<Message-ID>`. Spaces and the angle brackets around the Message-ID are
    /// dropped ([`bare_message_id`]), so `<a1@example.org>` and `a1@example.org` give the same link.
    pub fn mail(account: &str, message_id: &str) -> Link {
        let account = account.trim();
        let address = account.strip_prefix(MAIL_PREFIX).unwrap_or(account);
        Link::new(format!("{MAIL_PREFIX}{address}:{}", bare_message_id(message_id)))
    }

    /// `(account id, Message-ID)` when the id is a mail one (`mail:<address>:<Message-ID>`): the
    /// account as `mail:<address>`, the Message-ID as stored. The address ends at the first `:`
    /// after its `@` (an address has one `@`, and its domain no `:`), so a Message-ID with `:` in
    /// it stays whole. `None` for other ids and when the address or the Message-ID is empty.
    pub fn mail_ref(&self) -> Option<(&str, &str)> {
        let rest = self.id.strip_prefix(MAIL_PREFIX)?;
        let at = rest.find('@')?;
        let colon = at + rest[at..].find(':')?;
        let message_id = &rest[colon + 1..];
        if at == 0 || colon == at + 1 || message_id.is_empty() {
            return None;
        }
        Some((&self.id[..MAIL_PREFIX.len() + colon], message_id))
    }

    /// The link to issue or pull request `number` in `repo` (`owner/name`), without details.
    pub fn github(repo: &str, number: u64) -> Link {
        Link::new(format!("{GITHUB_PREFIX}{repo}#{number}"))
    }

    /// `("owner/name", number)` when the id is a GitHub one (`github:<owner>/<repo>#<number>`,
    /// owner and repo made of ASCII letters, digits, `-`, `_` and `.`); `None` otherwise.
    pub fn github_ref(&self) -> Option<(&str, u64)> {
        let (repo, number) = self.id.strip_prefix(GITHUB_PREFIX)?.rsplit_once('#')?;
        let (owner, name) = repo.split_once('/')?;
        let part =
            |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !(part(owner) && part(name)) {
            return None;
        }
        Some((repo, number.parse().ok()?))
    }

    /// The GitHub link `input` names, as a person pastes it (task G.6): the address of an issue or
    /// pull request on github.com (`https://github.com/owner/repo/issues/12`, `…/pull/12/files`,
    /// with or without `https://` or `www.`, anything after the number ignored), `owner/repo#12`
    /// or a link id `github:owner/repo#12`; surrounding spaces do not count. `None` for anything
    /// else, so the tasks app can tell a pasted link from a title. The core reads the same forms
    /// (`torro_pim_github::address::issue_ref`) and decides; this only recognises them.
    pub fn pasted(input: &str) -> Option<Link> {
        let input = input.trim();
        let id = Link::new(input);
        if id.github_ref().is_some() {
            return Some(id);
        }
        let part =
            |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        let number = |n: &str| n.parse::<u64>().ok();
        if let Some((repo, n)) = input.split_once('#')
            && let Some((owner, name)) = repo.split_once('/')
            && part(owner)
            && part(name)
        {
            return Some(Link::github(repo, number(n)?));
        }
        let rest = input.strip_prefix("https://").or_else(|| input.strip_prefix("http://")).unwrap_or(input);
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let path = rest.strip_prefix("github.com/")?.split(['?', '#']).next().unwrap_or_default();
        let mut segments = path.split('/');
        let (owner, name, page, n) = (segments.next()?, segments.next()?, segments.next()?, segments.next()?);
        (part(owner) && part(name) && matches!(page, "issues" | "pull"))
            .then(|| number(n).map(|n| Link::github(&format!("{owner}/{name}"), n)))
            .flatten()
    }

    /// The item's page: `https://github.com/<owner>/<repo>/issues/<number>` for a GitHub id
    /// (GitHub forwards it to the pull request when it is one); `None` for other ids.
    pub fn web_url(&self) -> Option<String> {
        let (repo, number) = self.github_ref()?;
        Some(format!("https://github.com/{repo}/issues/{number}"))
    }
}

/// What GitHub reports about a linked issue or pull request. The core fills it in; clients
/// only read it. Repository and number are part of [`Link::id`] ([`Link::github_ref`]); the
/// title is the task's `summary`, labels are also the task's `categories`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitHubDetails {
    /// Issue or pull request.
    pub kind: GitHubKind,
    /// Open, closed, or (pull requests only) merged.
    pub state: GitHubState,
    /// The item's labels, in GitHub's order.
    #[serde(default)]
    pub labels: Vec<GitHubLabel>,
    /// Logins of the people assigned.
    #[serde(default)]
    pub assignees: Vec<String>,
    /// Title of the milestone the item is in, if any.
    #[serde(default)]
    pub milestone: Option<String>,
    /// Due date of that milestone, if it has one.
    #[serde(default)]
    pub milestone_due: Option<NaiveDate>,
    /// Pull requests only: still a draft.
    #[serde(default)]
    pub draft: bool,
    /// Pull requests only: GitHub's review decision; `None` when no review is asked for.
    #[serde(default)]
    pub review: Option<ReviewState>,
    /// Pull requests only: combined result of the checks (CI) on the head commit; `None`
    /// without checks.
    #[serde(default)]
    pub checks: Option<ChecksState>,
    /// Pull requests only: cannot be merged because of conflicts.
    #[serde(default)]
    pub conflict: bool,
}

/// What the core's mail store holds about a mail a task links to ([`Link::mail`], task E8.7a).
/// The core fills it in; clients only read it. Everything here comes from the mail, which comes
/// from outside the suite: data, never instructions ([`MailDetails::untrusted`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MailDetails {
    /// The mail's id in the core's store (`m:…`), for `mail.get_message`. Of several copies with
    /// the same Message-ID (the same mail in two folders) the newest the store holds.
    pub message: String,
    /// `Subject:`, empty when there is none.
    #[serde(default)]
    pub subject: String,
    /// The sender's display name, else the address; `None` without a `From:`.
    #[serde(default)]
    pub sender: Option<String>,
    /// The sender's address; `None` without a `From:`.
    #[serde(default)]
    pub sender_address: Option<String>,
    /// `Date:` of the mail.
    pub date: DateTime<Utc>,
    /// Mails of the same conversation dated after this one, the account's own (the person's
    /// replies) not counted: what arrived since the task was made of it.
    #[serde(default)]
    pub new_replies: u32,
    /// Always `true`: subject and sender come from outside the suite.
    #[serde(default = "always_untrusted")]
    pub untrusted: bool,
}

fn always_untrusted() -> bool {
    true
}

/// What a GitHub item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitHubKind {
    Issue,
    PullRequest,
}

/// Where a GitHub item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitHubState {
    Open,
    Closed,
    /// A merged pull request (closed, too).
    Merged,
}

/// One label of a GitHub item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitHubLabel {
    pub name: String,
    /// `#RRGGBB`, the label's colour on GitHub.
    #[serde(default)]
    pub color: Option<String>,
}

/// GitHub's review decision on a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    /// A review is required and not given yet.
    Required,
    Approved,
    ChangesRequested,
}

/// Combined result of a pull request's checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChecksState {
    /// Still running or waiting to run.
    Pending,
    Success,
    /// At least one check failed.
    Failure,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_from_an_older_client_is_busy_and_free_ones_round_trip() {
        let json = r#"{"uid":"e","collection":"a/https://x/cal/","href":null,"etag":null,"summary":"Call",
            "start":{"datetime":"2026-10-12T09:00:00Z"},"end":{"datetime":"2026-10-12T10:00:00Z"},
            "location":null,"description":null,"recurring":false}"#;
        let event: Event = serde_json::from_str(json).expect("old event");
        assert_eq!(event.transparency, Transparency::Opaque, "missing TRANSP means busy (RFC 5545)");
        assert!(event.busy());
        let free = Event { transparency: Transparency::Transparent, ..event.clone() };
        let json = serde_json::to_string(&free).expect("json");
        assert!(json.contains(r#""transparency":"transparent""#), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).expect("back"), free);
        assert!(!free.busy(), "free time is not busy");
        let day = NaiveDate::from_ymd_opt(2026, 10, 12).expect("date");
        let all_day = Event { start: EventTime::Date(day), end: None, ..event.clone() };
        assert!(!all_day.busy(), "all-day entries never make the time busy");
        // Task TE.2: a cancelled event keeps no time busy; tentative and confirmed ones do.
        let status = |status| Event { status: Some(status), ..event.clone() };
        assert!(!status(EventStatus::Cancelled).busy(), "cancelled events leave the time free");
        assert!(status(EventStatus::Tentative).busy() && status(EventStatus::Confirmed).busy());
    }

    /// Task TE.1: an event from an older client reads without the fields of the event editor, and
    /// one with all of them comes back as it went.
    #[test]
    fn an_event_reads_without_the_editor_fields_and_round_trips_with_them() {
        let json = r#"{"uid":"e","collection":"a/https://x/cal/","href":null,"etag":null,"summary":"Call",
            "start":{"datetime":"2026-10-13T09:00:00Z"},"end":null,"location":null,"description":null,
            "recurring":true}"#;
        let old: Event = serde_json::from_str(json).expect("old event");
        assert_eq!((old.recurrence_id, &old.repeat, &old.time_zone, &old.url), (None, &None, &None, &None));
        assert_eq!((old.status, old.geo, &old.organizer, old.attendees.len()), (None, None, &None, 0));

        let start = EventTime::DateTime(DateTime::from_timestamp(1_791_882_000, 0).expect("instant"));
        let until = EventTime::Date(NaiveDate::from_ymd_opt(2026, 12, 22).expect("date"));
        let full = Event {
            recurrence_id: Some(start),
            repeat: Some(Box::new(Repeat {
                freq: RepeatFreq::Monthly,
                interval: 2,
                weekdays: vec![RepeatDay { day: Weekday::Tuesday, nth: Some(-1) }],
                month_days: vec![-1],
                months: vec![10],
                end: RepeatEnd::Until(until),
                rule: "FREQ=MONTHLY;INTERVAL=2;BYDAY=-1TU".into(),
                custom: false,
            })),
            time_zone: Some("Europe/Berlin".into()),
            url: Some("https://example.org/agenda".into()),
            status: Some(EventStatus::Tentative),
            geo: Geo::new(52.52, 13.405),
            organizer: Some(Box::new(Attendee {
                name: Some("Anna".into()),
                email: Some("anna@example.org".into()),
                partstat: PartStat::Accepted,
                role: AttendeeRole::Chair,
            })),
            attendees: vec![Attendee {
                name: None,
                email: Some("ben@example.org".into()),
                partstat: PartStat::NeedsAction,
                role: AttendeeRole::Optional,
            }],
            ..old
        };
        let value = serde_json::to_value(&full).expect("json");
        assert_eq!(value["status"], "tentative");
        assert_eq!(value["repeat"]["freq"], "monthly");
        assert_eq!(value["repeat"]["weekdays"][0], serde_json::json!({"day": "tuesday", "nth": -1}));
        assert_eq!(value["repeat"]["end"], serde_json::json!({"until": {"date": "2026-12-22"}}));
        assert_eq!(value["attendees"][0]["partstat"], "needs_action");
        assert_eq!(value["organizer"]["role"], "chair");
        assert_eq!(serde_json::from_value::<Event>(value).expect("back"), full);

        let short: Repeat = serde_json::from_str(r#"{"freq":"weekly"}"#).expect("a rule with defaults");
        assert_eq!((short.interval, short.end, short.custom, short.rule.as_str()), (1, RepeatEnd::Never, false, ""));
        let count: RepeatEnd = serde_json::from_str(r#"{"count":9}"#).expect("count");
        assert_eq!(count, RepeatEnd::Count(9));
        let someone: Attendee = serde_json::from_str("{}").expect("an attendee without anything");
        assert_eq!((someone.partstat, someone.role), (PartStat::NeedsAction, AttendeeRole::Required));
    }

    #[test]
    fn series_scopes_are_snake_case() {
        for (scope, name) in [
            (SeriesScope::Occurrence, "occurrence"),
            (SeriesScope::Following, "following"),
            (SeriesScope::Series, "series"),
        ] {
            assert_eq!(serde_json::to_value(scope).expect("json"), serde_json::json!(name));
        }
    }

    #[test]
    fn a_geo_is_a_finite_point_and_a_map_shows_only_some() {
        assert_eq!(Geo::new(52.52, 13.405), Some(Geo { lat: 52.52, lon: 13.405 }));
        for (lat, lon) in [(f64::NAN, 0.0), (0.0, f64::INFINITY), (90.1, 0.0), (0.0, -180.5)] {
            assert_eq!(Geo::new(lat, lon), None, "{lat} {lon}");
        }
        assert!(Geo { lat: 85.05, lon: -180.0 }.on_map());
        assert!(!Geo { lat: 85.06, lon: 0.0 }.on_map(), "beyond Web Mercator");
        assert!(!Geo { lat: 0.0, lon: 180.1 }.on_map());
        let weekday: chrono::Weekday = Weekday::from(chrono::Weekday::Sun).into();
        assert_eq!(weekday, chrono::Weekday::Sun);
    }

    #[test]
    fn a_task_from_an_older_client_reads_without_the_new_fields() {
        let json = r#"{"uid":"t","collection":"a/https://x/tasks/","href":null,"etag":null,"summary":"Milk",
            "due":null,"priority":null,"completed":false,"parent":null,"description":null}"#;
        let todo: Todo = serde_json::from_str(json).expect("old task");
        assert_eq!((todo.url, todo.categories, todo.sort_order, todo.link), (None, Vec::new(), None, None));
    }

    #[test]
    fn a_collection_reads_without_a_source_and_round_trips_with_one() {
        let json = r#"{"id":"a/https://x/tasks/","name":"Home","kind":"todos","color":null}"#;
        let plain: Collection = serde_json::from_str(json).expect("a collection of an older core");
        assert_eq!((plain.writable, plain.source.clone()), (true, None));
        assert!(!serde_json::to_string(&plain).expect("json").contains("source"), "older apps see nothing new");
        let filter = Collection {
            id: "github:mahype/00ff".into(),
            writable: false,
            source: Some(GitHubFilter {
                queries: vec!["is:open label:bug".into()],
                pinned: vec!["github:mahype/TorroWorkplace#7".into()],
                revision: 2,
                truncated: true,
                problem: None,
            }),
            ..plain
        };
        let json = serde_json::to_string(&filter).expect("json");
        assert_eq!(serde_json::from_str::<Collection>(&json).expect("back"), filter);
    }

    #[test]
    fn a_link_reads_without_details_and_round_trips_with_them() {
        let link: Link = serde_json::from_str(r#"{"id":"github:mahype/TorroWorkplace#123"}"#).expect("link");
        assert_eq!(link, Link::github("mahype/TorroWorkplace", 123));
        let details = GitHubDetails {
            kind: GitHubKind::PullRequest,
            state: GitHubState::Open,
            labels: vec![GitHubLabel { name: "bug".into(), color: Some("#D73A4A".into()) }],
            assignees: vec!["mahype".into()],
            milestone: Some("1.0".into()),
            milestone_due: NaiveDate::from_ymd_opt(2026, 11, 1),
            draft: true,
            review: Some(ReviewState::ChangesRequested),
            checks: Some(ChecksState::Failure),
            conflict: false,
        };
        let full = Link { github: Some(details), ..link };
        let json = serde_json::to_string(&full).expect("json");
        assert!(json.contains(r#""kind":"pull_request""#), "{json}");
        assert!(json.contains(r#""review":"changes_requested""#), "{json}");
        assert_eq!(serde_json::from_str::<Link>(&json).expect("back"), full);
        // Details from an older core, without the pull request fields.
        let short: GitHubDetails = serde_json::from_str(r#"{"kind":"issue","state":"closed"}"#).expect("short");
        assert_eq!((short.labels.len(), short.draft, short.review), (0, false, None));
    }

    #[test]
    fn github_ids_name_repository_number_and_page() {
        let link = Link::github("mahype/TorroWorkplace", 123);
        assert_eq!(link.id, "github:mahype/TorroWorkplace#123");
        assert_eq!(link.github_ref(), Some(("mahype/TorroWorkplace", 123)));
        assert_eq!(link.web_url().as_deref(), Some("https://github.com/mahype/TorroWorkplace/issues/123"));
        for other in [
            "jira:ABC-1",
            "github:mahype#1",
            "github:mahype/TorroWorkplace#x",
            "github:mahype/Torro Workplace#1",
            "github:/x#1",
            "github:a/b/c#1",
            "github:a/b?x#1",
            "github:a/b#-1",
        ] {
            let link = Link::new(other);
            assert_eq!((link.github_ref(), link.web_url()), (None, None), "{other}");
        }
    }

    /// Task E8.7a: a mail link is the account's id and the Message-ID.
    #[test]
    fn mail_ids_name_account_and_message_id() {
        let link = Link::mail("mail:anna@mailbox.org", "a1@example.org");
        assert_eq!(link.id, "mail:anna@mailbox.org:a1@example.org");
        assert_eq!(link.mail_ref(), Some(("mail:anna@mailbox.org", "a1@example.org")));
        assert_eq!(
            Link::mail("anna@mailbox.org", " <a1@example.org> "),
            link,
            "brackets and prefix as the store has them"
        );
        assert_eq!((link.github_ref(), link.web_url()), (None, None));
        // A Message-ID with a colon in it stays whole; one written with brackets is looked up bare.
        let odd = Link::new("mail:anna@mailbox.org:<x:y@example.org>");
        assert_eq!(odd.mail_ref(), Some(("mail:anna@mailbox.org", "<x:y@example.org>")));
        assert_eq!(odd.mail_ref().map(|(_, id)| bare_message_id(id)), Some("x:y@example.org"));
        assert_eq!(bare_message_id(" <> "), "");
        for other in
            ["inbox:0a1b", "mail:anna@mailbox.org", "mail:anna@mailbox.org:", "mail:@x.org:<a@b>", "mail:a@:<a@b>"]
        {
            assert_eq!(Link::new(other).mail_ref(), None, "{other}");
        }
    }

    /// Task E8.7a: what the core fills in about a mail comes back as it went and is always foreign.
    #[test]
    fn mail_details_round_trip_and_are_untrusted() {
        let link: Link = serde_json::from_str(r#"{"id":"mail:a@b.org:<1@b.org>","github":null}"#).expect("older");
        assert_eq!(link, Link::new("mail:a@b.org:<1@b.org>"));
        let json = r#"{"message":"m:1","date":"2026-10-13T09:00:00Z"}"#;
        let details: MailDetails = serde_json::from_str(json).expect("short details");
        assert!(details.untrusted && details.subject.is_empty() && details.new_replies == 0);
        let full = Link { mail: Some(Box::new(MailDetails { new_replies: 2, ..details })), ..link };
        let json = serde_json::to_string(&full).expect("json");
        assert_eq!(serde_json::from_str::<Link>(&json).expect("back"), full);
    }

    #[test]
    fn pasted_links_are_recognised_and_titles_are_not() {
        let wanted = Some(Link::github("mahype/TorroWorkplace", 12));
        for input in [
            "github:mahype/TorroWorkplace#12",
            "mahype/TorroWorkplace#12",
            " https://github.com/mahype/TorroWorkplace/issues/12 ",
            "https://github.com/mahype/TorroWorkplace/pull/12/files",
            "github.com/mahype/TorroWorkplace/issues/12#issuecomment-1",
            "https://www.github.com/mahype/TorroWorkplace/issues/12?x=1",
        ] {
            assert_eq!(Link::pasted(input), wanted, "{input}");
        }
        for title in [
            "Buy oat milk",
            "Fix #12",
            "mahype#12",
            "https://github.com/mahype/TorroWorkplace/issues",
            "https://example.org/mahype/TorroWorkplace/issues/12",
            "https://github.com/mahype/Torro Workplace/issues/12",
            "a/b#x",
        ] {
            assert_eq!(Link::pasted(title), None, "{title}");
        }
    }
}
