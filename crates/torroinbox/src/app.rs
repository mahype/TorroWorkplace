//! The inbox window (tasks E3.1 and E3.2 in `docs/tasks/eingang.md`): state, loading through
//! [`InboxSource`], and the Linear window around it – `torro_gui::shell` with the sidebar ("For
//! me" with its count, "Info", "Snoozed", "Done", the group "Sources") in the wide layout, the
//! rail of icons in the medium one (and in the wide one when the user collapses it: panel button
//! or `[`, remembered in `ui.json`), a navigation menu ("For me ▾") plus drawer in the medium and
//! narrow ones. The list (`list.rs`) and the panel (`detail.rs`) only render and report clicks as
//! [`Message`]s.
//!
//! The panel is `torro_gui::panel`: beside the list in the wide layout, over it in the medium and
//! narrow ones (full width in narrow). Mini and strip show the glance instead (`glance.rs`, E3.3a):
//! the numbers of "For me" and its top entries, no chrome and no keys; the panel, menus and the
//! drawer stay in the state and come back with a larger window.
//!
//! Entries are asked for on a timer ([`REFRESH`]) and after every change; the core has no
//! `subscribe` yet (E1.2).
//!
//! Triage (E3.2): a cursor marks one row (the highlighted one); the open panel always shows it.
//! The keys (`keys.rs`) and the panel's footer do the same things: mark done ([`Edit::Done`]),
//! snooze through a menu or the picker of "Pick…" ([`Edit::Snooze`]), move to the other lane,
//! asking "always for this source?" ([`Edit::Lane`]), mute or unmute the source at once (E3.2c:
//! no dialog – a change with an undo needs no question as well). Every change shows a hint for
//! [`UNDO_FOR`] with "Undo" (`u`), which sends the change that puts it back ([`Undo`]). A change
//! that takes the entry off the list moves the cursor – and the panel – to the row that took its
//! place, so `e`, `e`, `e` works through a list.
//!
//! Make task (E3.5): `t` or the panel's "Make task" opens a small dialog at the entry – the title
//! (the entry's, to change), the task list (from [`InboxSource::list_task_lists`]) and an optional
//! due day (Today, Tomorrow, Next week, as the snooze menu counts them). "Create task" or `Enter`
//! sends `inbox.create_task_from_item`: the core makes the task, links it back to the entry and
//! marks the entry done; the hint says in which list. There is no undo: taking the task back
//! would delete it, and deleting is for the tasks app.
//!
//! Security rule 1 (`docs/tasks/eingang.md`): approvals, account problems and the suite's entries
//! belong to the human. This window is the human (it holds the app key), so it may change them,
//! with two limits of its own: an approval has no "done" (it closes once decided), and the suite
//! as a source is neither muted nor given a remembered lane – that would silence approvals.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc};
use iced::theme::Mode;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, pick_list, row, stack, text_input};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::shell::{self, SidebarMode, Surface, Switcher};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{datetime, style, typo};
use torro_inbox::source::TaskList;
use torro_inbox::{InboxSource, Item, ItemFilter, Lane, Link, SourceInfo, SourceKind, SourceRef, State, StateFilter};

use crate::glance::{self, GlanceData};
use crate::keys::{self, Command, Focus, Key};
use crate::{detail, list};

/// The app's identity: window class `torro-inbox`, binary `torroinbox`, state in `torroinbox/`.
pub const INBOX: AppId = AppId::INBOX;

/// How often the window asks the source again. Short, because a waiting agent should show up
/// within seconds; the source is local and cheap to ask, and nothing moves on screen unless an
/// entry or a time in the list changed.
const REFRESH: Duration = Duration::from_secs(10);

/// How long the hint after a change shows, and with it "Undo" (`u`).
pub const UNDO_FOR: Duration = Duration::from_secs(5);

/// How often a shown hint checks whether its time is up. Only while a hint shows.
const HINT_TICK: Duration = Duration::from_secs(1);

/// "Later today" snoozes for this many hours.
const LATER_TODAY_HOURS: i64 = 3;

/// The hour (local time) "Tomorrow" and "Next week" wake at.
const MORNING: u32 = 9;

/// The times of the day "Pick…" offers, in local time.
pub const PICK_HOURS: [u32; 4] = [MORNING, 13, 17, 20];

/// How many days ahead "Pick…" goes.
const PICK_DAYS: u64 = 60;

/// The id of the title field in the dialog of "Make task", which takes the focus as it opens.
const TASK_TITLE: &str = "task-title";

/// When a task made of an entry is due (task E3.5): a day counted as the snooze menu counts it,
/// or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Due {
    #[default]
    None,
    Today,
    Tomorrow,
    /// Monday of next week.
    NextWeek,
}

impl Due {
    /// In the order the dialog lists them.
    pub const ALL: [Due; 4] = [Due::None, Due::Today, Due::Tomorrow, Due::NextWeek];

    /// The local day, seen from `now`; `None` for no due date.
    pub fn day(self, now: DateTime<Utc>) -> Option<NaiveDate> {
        let today = local_today(now);
        match self {
            Due::None => None,
            Due::Today => Some(today),
            Due::Tomorrow => Some(today + Days::new(1)),
            Due::NextWeek => Some(datetime::week_start(today) + Days::new(7)),
        }
    }
}

impl fmt::Display for Due {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Due::None => tr("No due date"),
            Due::Today => tr("Today"),
            Due::Tomorrow => tr("Tomorrow"),
            Due::NextWeek => tr("Next week"),
        })
    }
}

/// What the dialog of "Make task" holds while it is open, for the entry in `App::subject`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskDraft {
    /// The task's title; starts as the entry's. Left blank, the core takes the entry's.
    pub title: String,
    /// The list chosen; `None` until the lists are in (then the last one used, else the first).
    pub list: Option<TaskList>,
    pub due: Due,
}

/// A change of one entry, one [`InboxSource`] call each (`inbox.set_item_done`,
/// `inbox.snooze_item`, `inbox.set_item_lane`). Keys, menus and the panel's footer all end here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Mark done (`true`) or open again (`false`).
    Done { id: String, done: bool },
    /// Put off until `until`; `None` takes a snoozed entry back.
    Snooze { id: String, until: Option<DateTime<Utc>> },
    /// Move to `lane`; with `remember` also later entries of the same source (a rule in the core).
    Lane { id: String, lane: Lane, remember: bool },
}

impl Edit {
    /// The entry the edit changes.
    pub fn id(&self) -> &str {
        match self {
            Edit::Done { id, .. } | Edit::Snooze { id, .. } | Edit::Lane { id, .. } => id,
        }
    }

    /// The edit that puts `before` – the entry as it is before `self` – back: the other done
    /// state, the earlier snooze time (or none), the earlier lane. Undo never remembers a lane:
    /// a remembered lane is a rule in the core, which only `inbox.remove_rule` takes back, so such
    /// a change gets no undo at all ([`Edit::after`]).
    pub fn undo(&self, before: &Item) -> Edit {
        let id = self.id().to_owned();
        match self {
            Edit::Done { done, .. } => Edit::Done { id, done: !done },
            Edit::Snooze { .. } => {
                let until = match before.state {
                    State::Snoozed { until } => Some(until),
                    State::New | State::Seen | State::Done { .. } => None,
                };
                Edit::Snooze { id, until }
            }
            Edit::Lane { .. } => Edit::Lane { id, lane: before.lane, remember: false },
        }
    }

    /// The hint once the edit is done, with its undo for `before` (the entry as it was), at `now`:
    /// "Marked as done", "Snoozed until 18:30", "Moved to Info" … A remembered lane has no undo.
    pub fn after(&self, before: &Item, now: DateTime<Utc>) -> After {
        let text = match self {
            Edit::Done { done: true, .. } => tr("Marked as done").to_owned(),
            Edit::Done { done: false, .. } => tr("Reopened").to_owned(),
            Edit::Snooze { until: Some(until), .. } => {
                trf("Snoozed until {time}", &[("time", &list::wake_text(*until, now))])
            }
            Edit::Snooze { until: None, .. } => tr("No longer snoozed").to_owned(),
            Edit::Lane { lane, remember: false, .. } => trf("Moved to {lane}", &[("lane", &lane_name(*lane))]),
            Edit::Lane { lane, remember: true, .. } => trf(
                "Moved to {lane}, always for {source}",
                &[("lane", &lane_name(*lane)), ("source", &before.source.label())],
            ),
        };
        let undo = (!matches!(self, Edit::Lane { remember: true, .. })).then(|| self.undo(before));
        After { text, undo }
    }

    /// Sends the edit to `inbox`; the entry as stored, or the error as people read it.
    pub async fn apply<S: InboxSource>(self, inbox: Arc<S>) -> Result<Item, String> {
        let result = match self {
            Edit::Done { id, done } => inbox.set_item_done(id, done).await,
            Edit::Snooze { id, until } => inbox.snooze_item(id, until).await,
            Edit::Lane { id, lane, remember } => inbox.set_item_lane(id, lane, remember).await,
        };
        result.map_err(|err| err.to_string())
    }
}

/// What the hint says after a change, and the edit "Undo" sends (`None`: nothing to undo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct After {
    pub text: String,
    pub undo: Option<Edit>,
}

/// What "Undo" in the hint sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// The edit that puts an entry back ([`Edit::undo`]).
    Edit(Edit),
    /// Sets the source's "muted" back to the value given (`inbox.mute_source`).
    Mute(SourceRef, bool),
}

/// The hint at the bottom of the window: what just happened, with "Undo" when it can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub text: String,
    pub undo: Option<Undo>,
    /// When it appeared; it goes [`UNDO_FOR`] later.
    pub shown: Instant,
}

/// The day and time chosen in the picker of "Pick…".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    /// The local day.
    pub day: NaiveDate,
    /// Index into [`PICK_HOURS`].
    pub hour: usize,
}

impl Pick {
    /// The picker as it opens: tomorrow morning, seen from `now`.
    fn start(now: DateTime<Utc>) -> Pick {
        Pick { day: local_today(now) + Days::new(1), hour: 0 }
    }

    /// The chosen moment.
    pub fn at(self) -> DateTime<Utc> {
        at_hour(self.day, PICK_HOURS.get(self.hour).copied().unwrap_or(MORNING))
    }
}

/// `day` at `hour` o'clock, local time.
fn at_hour(day: NaiveDate, hour: u32) -> DateTime<Utc> {
    datetime::to_utc(day.and_time(NaiveTime::MIN) + TimeDelta::hours(i64::from(hour)))
}

/// The local date of `now`.
fn local_today(now: DateTime<Utc>) -> NaiveDate {
    datetime::to_local(now).date()
}

/// The times of the snooze menu, seen from `now`: "Later today" (in three hours), "Tomorrow"
/// (9:00) and "Next week" (Monday, 9:00), all in local time.
pub fn snooze_times(now: DateTime<Utc>) -> [DateTime<Utc>; 3] {
    let today = local_today(now);
    [
        now + TimeDelta::hours(LATER_TODAY_HOURS),
        at_hour(today + Days::new(1), MORNING),
        at_hour(datetime::week_start(today) + Days::new(7), MORNING),
    ]
}

/// "For me" or "Info".
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// "Today", "Tomorrow", else the short date ("Thu, 15 Oct"), seen from `today`.
fn day_name(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        tr("Today").to_owned()
    } else if day == today + Days::new(1) {
        tr("Tomorrow").to_owned()
    } else {
        datetime::format_short_date(day, today)
    }
}

/// The icon of a lane, as in the sidebar.
pub fn lane_icon(lane: Lane) -> Icon {
    match lane {
        Lane::ForMe => Icon::Inbox,
        Lane::Info => Icon::Bell,
    }
}

/// The lane an entry in `lane` moves to.
pub fn other_lane(lane: Lane) -> Lane {
    match lane {
        Lane::ForMe => Lane::Info,
        Lane::Info => Lane::ForMe,
    }
}

/// Whether the window offers to mute `source` or to remember a lane for it: not for the suite,
/// whose approvals and account problems must never be silenced by a rule.
pub fn rules_allowed(source: &SourceRef) -> bool {
    source.kind != SourceKind::Suite
}

/// One entry of a menu of choices (snooze, lane): picked by click or by its digit.
#[derive(Debug, Clone)]
pub struct Choice {
    pub icon: Icon,
    pub label: String,
    /// When the choice snoozes until ([`wake_short`]: "13:30", "Wed 09:00"), shown quietly before
    /// the digit; `None` for choices without a fixed time.
    pub when: Option<String>,
    pub message: Message,
}

/// When a snooze choice wakes, as short as it gets: the time when it is today ("13:30"), else the
/// short weekday and the time ("Wed 09:00"), in local time seen from `now`.
pub fn wake_short(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let at = datetime::to_local(at);
    let time = datetime::format_time(at.time());
    if at.date() == local_today(now) { time } else { format!("{} {time}", datetime::weekday_name(at.date(), true)) }
}

/// What a test can see of the calls the window started (tasks do not run in tests).
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Edit(Edit, Option<After>),
    /// The source, muted or not, and whether the hint with its undo follows (not after an undo).
    Mute(SourceRef, bool, bool),
    Link(String),
    /// `inbox.open_item` for the entry with this id.
    Open(String),
    /// The task lists were asked for (the dialog of "Make task" opened).
    Lists,
    /// `inbox.create_task_from_item`: entry, list, title (`None`: the entry's), due day.
    Task {
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    },
}

/// What the list shows. Each view is one [`ItemFilter`] ([`View::filter`]), so the app asks the
/// source exactly what the API offers (`inbox.list_items`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Open entries in the lane "For me"; the only view with a count.
    ForMe,
    /// Open entries in the lane "Info".
    Info,
    /// Snoozed entries, both lanes.
    Snoozed,
    /// Done entries, both lanes.
    Done,
    /// Open entries of one source, both lanes.
    Source(SourceRef),
}

impl View {
    /// The filter for `inbox.list_items` that selects the entries of this view.
    pub fn filter(&self) -> ItemFilter {
        let (lane, source, state) = match self {
            View::ForMe => (Some(Lane::ForMe), None, StateFilter::Open),
            View::Info => (Some(Lane::Info), None, StateFilter::Open),
            View::Snoozed => (None, None, StateFilter::Snoozed),
            View::Done => (None, None, StateFilter::Done),
            View::Source(source) => (None, Some(source.clone()), StateFilter::Open),
        };
        ItemFilter { lane, source, state: Some(state), ..ItemFilter::default() }
    }

    /// The name in the sidebar, the header and the panel's path.
    pub fn name(&self) -> String {
        match self {
            View::ForMe => tr("For me").into(),
            View::Info => tr("Info").into(),
            View::Snoozed => tr("Snoozed").into(),
            View::Done => tr("Done").into(),
            View::Source(source) => source.label(),
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            View::ForMe => Icon::Inbox,
            View::Info => Icon::Bell,
            View::Snoozed => Icon::Clock,
            View::Done => Icon::CircleCheck,
            View::Source(source) => list::source_icon(source),
        }
    }
}

/// The menu that is open over the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The app switcher at the top of the sidebar.
    Switcher,
    /// The navigation menu of the medium and narrow layouts ("For me ▾").
    Nav,
    /// The snooze menu of `App::subject`: Later today, Tomorrow, Next week, Pick….
    Snooze,
    /// The date and time picker of "Pick…".
    Pick,
    /// The lane question of `App::subject`: only this entry, or always for its source.
    Lane,
    /// The dialog that makes a task of `App::subject` (task E3.5).
    Task,
}

/// A state the window can start in (`--demo --open <state>`), so every state can be photographed
/// from a script without clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The panel with the first entry of "For me" (a waiting agent in the demo).
    Detail,
    /// The panel with the first approval.
    Approval,
    /// The view "Info".
    Info,
    /// The view "Snoozed".
    Snoozed,
    /// The view "Done".
    Done,
    /// The navigation menu ("For me ▾"); only the medium and narrow header have it.
    Menu,
    /// The sidebar drawer of the medium and narrow layouts.
    Drawer,
    /// The app switcher at the top of the sidebar (in the drawer where there is no sidebar).
    Switcher,
    /// The error state with its "Retry".
    Error,
    /// The list with the error line above it, as after a change that did not go through; kept
    /// for good.
    Failed,
    /// No entries at all.
    Empty,
    /// The loading state, kept for good.
    Loading,
    /// The snooze menu of the first entry of "For me" (at its row).
    Snooze,
    /// The picker of "Pick…" for the first entry of "For me".
    Pick,
    /// The lane question for the first entry of "For me" not from the suite.
    Lane,
    /// The hint after muting the source of that entry, with "Undo", kept for good. Only the hint:
    /// the demo source is not muted, so nothing else changes.
    Mute,
    /// The hint after marking the first entry done, with "Undo", kept for good.
    Undo,
    /// The panel of the first entry of "For me" that is no approval, with the dialog of "Make
    /// task" open at its footer (task E3.5).
    Task,
}

impl Open {
    const ALL: [(&'static str, Open); 18] = [
        ("detail", Open::Detail),
        ("approval", Open::Approval),
        ("info", Open::Info),
        ("snoozed", Open::Snoozed),
        ("done", Open::Done),
        ("menu", Open::Menu),
        ("drawer", Open::Drawer),
        ("switcher", Open::Switcher),
        ("error", Open::Error),
        ("failed", Open::Failed),
        ("empty", Open::Empty),
        ("loading", Open::Loading),
        ("snooze", Open::Snooze),
        ("pick", Open::Pick),
        ("lane", Open::Lane),
        ("mute", Open::Mute),
        ("undo", Open::Undo),
        ("task", Open::Task),
    ];
}

/// The start parameters of the window:
///
/// - `--demo`: sample data ([`torro_inbox::demo::DemoSource`]) instead of the core.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--now <YYYY-MM-DDTHH:MM>` (local time): a fixed "now" and demo data around it, for pictures
///   that can be made again; needs `--demo`.
/// - `--lane for-me|info` or `--source <key>` (`suite`, `agent:claude-code`, `script:backup`): start
///   on that lane or source, for one window per slice of the inbox (E3.6). Works without `--demo`;
///   a workspace template stores the window's command line, so the slice comes back with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    pub now: Option<NaiveDateTime>,
    /// The view of `--lane` or `--source`; `None` starts on "For me".
    pub view: Option<View>,
}

/// Why the start parameters were refused; `main` prints it and exits with 64, like the other apps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// `--open` and friends without a value.
    MissingValue(&'static str),
    /// A value the flag does not know, with the list of valid ones.
    UnknownValue { flag: &'static str, value: String, valid: String },
    /// `--open` or `--now` without `--demo`: they only make sense on sample data.
    NeedsDemo(&'static str),
    /// `--now` is not a date and time like `2026-10-12T10:30`.
    BadNow(String),
    /// Two flags that each choose the start view, like `--lane` and `--source`.
    Conflict(&'static str, &'static str),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            LaunchError::UnknownValue { flag, value, valid } => {
                write!(f, "unknown value {value:?} for {flag}; valid values: {valid}")
            }
            LaunchError::NeedsDemo(flag) => write!(f, "{flag} only works together with --demo"),
            LaunchError::BadNow(value) => {
                write!(f, "--now expects YYYY-MM-DDTHH:MM, e.g. 2026-10-12T10:30; got {value:?}")
            }
            LaunchError::Conflict(one, other) => write!(f, "{one} and {other} cannot be used together"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// The value of `--flag value` or `--flag=value`; `None` when the flag is not there.
fn flag_value<'a>(args: &'a [String], flag: &'static str) -> Result<Option<&'a str>, LaunchError> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            return words.next().map(|value| Some(value.as_str())).ok_or(LaunchError::MissingValue(flag));
        }
        if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

impl Launch {
    /// Reads the start parameters from `args` (the words after the program name). Words it does
    /// not know are left alone.
    pub fn parse(args: &[String]) -> Result<Launch, LaunchError> {
        let demo = args.iter().any(|arg| arg == "--demo");
        let unknown = |flag, value: &str, valid: Vec<&str>| LaunchError::UnknownValue {
            flag,
            value: value.to_owned(),
            valid: valid.join(", "),
        };
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--open")),
            Some(word) => match Open::ALL.iter().find(|(name, _)| *name == word) {
                Some(&(_, open)) => Some(open),
                None => return Err(unknown("--open", word, Open::ALL.iter().map(|(name, _)| *name).collect())),
            },
        };
        let appearance = match flag_value(args, "--appearance")? {
            None => None,
            Some("light") => Some(Appearance::Light),
            Some("dark") => Some(Appearance::Dark),
            Some("system") => Some(Appearance::System),
            Some(other) => return Err(unknown("--appearance", other, vec!["light", "dark", "system"])),
        };
        let now = match flag_value(args, "--now")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--now")),
            Some(word) => Some(
                NaiveDateTime::parse_from_str(word, "%Y-%m-%dT%H:%M").map_err(|_| LaunchError::BadNow(word.into()))?,
            ),
        };
        let view = start_view(args)?;
        Ok(Launch { demo, open, appearance, now, view })
    }

    /// "Now" at start: the fixed `--now`, else the clock. The demo data are built around it.
    pub fn start(&self) -> DateTime<Utc> {
        self.now.map_or_else(Utc::now, datetime::to_utc)
    }
}

/// The view chosen by `--lane for-me|info` or `--source <key>`; `None` when neither is given.
/// Both at once are refused: no view shows one lane of one source.
fn start_view(args: &[String]) -> Result<Option<View>, LaunchError> {
    let lane = match flag_value(args, "--lane")? {
        None => None,
        Some("for-me") => Some(View::ForMe),
        Some("info") => Some(View::Info),
        Some(other) => {
            return Err(LaunchError::UnknownValue {
                flag: "--lane",
                value: other.to_owned(),
                valid: "for-me, info".to_owned(),
            });
        }
    };
    let source = match flag_value(args, "--source")? {
        None => None,
        Some(key) => match key.parse::<SourceRef>() {
            Ok(source) => Some(View::Source(source)),
            Err(_) => {
                return Err(LaunchError::UnknownValue {
                    flag: "--source",
                    value: key.to_owned(),
                    valid: "suite, agent:<name>, script:<name>".to_owned(),
                });
            }
        },
    };
    match (lane, source) {
        (Some(_), Some(_)) => Err(LaunchError::Conflict("--lane", "--source")),
        (lane, source) => Ok(lane.or(source)),
    }
}

/// The text of the forced error state.
const DEMO_ERROR: &str = "The server did not answer (demo)";

/// One answer of the source: the entries of a view, the open entries of "For me", the count for
/// "Info", the sources.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub items: Vec<Item>,
    /// Open entries in "For me", whatever the view: their number is the one in the sidebar, the
    /// first of them are what the glance shows (mini, strip).
    pub for_me: Vec<Item>,
    /// Open entries in "Info", the number in the sidebar. Together with `for_me` it is what the
    /// sources count (open entries, both lanes), so the sidebar's numbers add up.
    pub info: usize,
    pub sources: Vec<SourceInfo>,
}

/// Something that went wrong, as the window shows it: a whole sentence ("Couldn't load the
/// inbox."), and quietly below it the reason as the source gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What did not work, a sentence in the user's language.
    pub what: &'static str,
    /// Why, as the source or the core said it.
    pub why: String,
}

impl Failure {
    fn new(what: &'static str, why: impl Into<String>) -> Failure {
        Failure { what, why: why.into() }
    }
}

pub struct App<S> {
    inbox: Arc<S>,
    /// The suite behind the window: appearance, zoom, opening other apps (none with `--demo`).
    suite: Source,
    /// The start parameters; `open` also keeps the forced states (error, empty, loading) alive.
    launch: Launch,
    /// The one-time part of `launch.open` (opening a panel) has been done.
    opened: bool,
    /// How far "now" is ahead of the clock: zero, or `--now` minus the clock at start, so the
    /// times in the list match the demo data.
    offset: TimeDelta,
    now: DateTime<Utc>,
    /// The window size, for the size class ([`layout::of`]), kept from the window events.
    size: Size,
    view: View,
    items: Vec<Item>,
    /// The open entries of "For me", waiting ones first ([`Loaded::for_me`]).
    for_me: Vec<Item>,
    info: usize,
    sources: Vec<SourceInfo>,
    /// Whether entries have been loaded once; until then the list shows loading (or the error).
    loaded: bool,
    error: Option<Failure>,
    /// The entry in the panel, as last loaded or changed: always the one under the cursor. A
    /// copy, so the panel can show the answer of a change before the list is loaded again; when
    /// the entry has left the list by then, the panel moves on with the cursor.
    detail: Option<Item>,
    /// Id of the row under the cursor (highlighted), which the keys act on.
    cursor: Option<String>,
    /// The list's scroll offset and visible height, as last reported, to keep the cursor in view.
    scroll: Option<(f32, f32)>,
    /// The entry the open snooze menu, picker or lane question is for, as it was when opened.
    subject: Option<Item>,
    /// The day and time in the picker of "Pick…".
    pick: Pick,
    /// The dialog of "Make task" while [`Menu::Task`] is open.
    task: TaskDraft,
    /// The task lists for that dialog: `None` while they load, then the answer.
    lists: Option<Result<Vec<TaskList>, String>>,
    /// The id of the list the last task went into; the dialog chooses it first next time.
    last_list: Option<String>,
    /// The hint after the last change, with its undo.
    hint: Option<Hint>,
    /// The calls started, for the tests.
    #[cfg(test)]
    sent: Vec<Sent>,
    appearance: Appearance,
    system_mode: Mode,
    menu: Option<Menu>,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0), the factor of the whole window.
    zoom: Zoom,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The band above the content was pressed: drag the window (see [`shell::titlebar`]).
    DragWindow,
    /// The window has this size (at start and after every resize).
    Resized(Size),
    /// The source's answer for the view it was asked for; answers for another view are dropped.
    Loaded(View, Result<Loaded, String>),
    /// From the sidebar or the navigation menu.
    Show(View),
    /// From the list: open the entry with this id in the panel.
    Select(String),
    /// From the glance (mini, strip): the entry of "For me" with this id under the cursor and in
    /// the panel of "For me", which the window shows once it is larger again.
    OpenForMe(String),
    /// The panel's ✕, a click beside it, Escape.
    CloseDetail,
    /// From the panel: mark the entry done (`true`) or open again (`false`).
    SetDone(String, bool),
    /// The source's answer to a change: the entry as stored.
    Changed(Result<Box<Item>, String>),
    /// From a menu or the picker: send this change, with a hint and its undo.
    Edit(Edit),
    /// The source's answer to an [`Edit`], with the hint to show (`None` after an undo).
    Edited(Result<Box<Item>, String>, Option<After>),
    /// "Undo" in the hint, or `u`: send the hint's undo.
    Undo,
    /// The hint's timer: the hint goes once [`UNDO_FOR`] has passed.
    HintTick(Instant),
    /// A key from [`keys::listen`]; what it does depends on what is open ([`keys::command`]).
    Key(Key),
    /// The list scrolled (or was laid out): its offset and visible height.
    Scrolled(f32, f32),
    /// From the panel: the snooze menu for the entry with this id.
    AskSnooze(String),
    /// From the panel: move the entry with this id to the other lane (asking first where a rule
    /// is possible).
    AskLane(String),
    /// From the panel: mute the source of the entry with this id, or unmute it – at once, with
    /// an undo.
    AskMute(String),
    /// "Pick…" in the snooze menu: the date and time picker.
    ShowPicker,
    /// The picker's day.
    PickDay(NaiveDate),
    /// The picker's time, an index into [`PICK_HOURS`].
    PickHour(usize),
    /// The picker's "Snooze".
    PickConfirm,
    /// The source's answer to muting or unmuting, with the hint to show (`None` after an undo).
    Muted(Result<(), String>, Option<Hint>),
    /// From the panel: open what the entry with this id links to – its page, its app, the agent's
    /// window – as far as [`detail::openable`] offers it.
    OpenItem(String),
    /// The source's answer to `inbox.open_item`.
    ItemOpened(Result<(), String>),
    /// From the panel or `t`: the dialog that makes a task of the entry with this id.
    AskTask(String),
    /// The task lists for the dialog, or why they could not be read.
    TaskLists(Result<Vec<TaskList>, String>),
    /// The dialog's title field.
    TaskTitle(String),
    /// The dialog's list.
    TaskList(TaskList),
    /// The dialog's due day.
    TaskDue(Due),
    /// "Create task", or `Enter` in the dialog.
    TaskConfirm,
    /// The source's answer to `inbox.create_task_from_item`, with the name of the list for the
    /// hint.
    TaskCreated(Result<(), String>, String),
    /// The browser could not be started (or was); only logged, as in the other apps.
    LinkOpened(Result<(), String>),
    Refresh,
    Tick,
    /// The desktop's light/dark preference (once at start, then whenever it changes).
    SystemMode(Mode),
    /// The appearance stored in the suite settings; an error keeps the one we have.
    Appearance(Result<Appearance, String>),
    /// The zoom keys, the stored zoom and its badge.
    Zoom(zoom::Message),
    /// From the app switcher at the top of the sidebar.
    Switcher(Switcher),
    /// Open or close the navigation menu ("For me ▾").
    ToggleNav,
    /// Open the sidebar drawer (narrow layout).
    ShowDrawer,
    /// The panel button at the top of the sidebar, or `[`: collapses or expands the sidebar (wide),
    /// opens or closes the drawer (medium, narrow).
    ToggleSidebar,
    /// The view state stored in `ui.json`, loaded at start.
    UiState(UiState),
    /// Close the open menu and the drawer (click beside them).
    CloseMenu,
    /// Escape: closes the innermost open thing – menu or drawer, then the panel.
    Escape,
    /// Start or raise another app of the suite ([`AppId::binary`]) through the core.
    OpenApp(String),
    /// The core's answer to [`Message::OpenApp`].
    AppOpened(Result<(), String>),
    /// Open the control centre.
    OpenSettings,
}

/// Asks `inbox` for the entries of `view`, the counts for "For me" and "Info" and the sources.
/// `empty` (the forced empty state) asks nothing and answers an empty inbox.
pub async fn fetch<S: InboxSource>(inbox: Arc<S>, view: View, empty: bool) -> Result<Loaded, String> {
    if empty {
        return Ok(Loaded { items: Vec::new(), for_me: Vec::new(), info: 0, sources: Vec::new() });
    }
    let failed = |err: torro_inbox::InboxError| err.to_string();
    let items = inbox.list_items(view.filter()).await.map_err(failed)?;
    let for_me = match view {
        View::ForMe => items.clone(),
        _ => inbox.list_items(View::ForMe.filter()).await.map_err(failed)?,
    };
    let info = match view {
        View::Info => items.len(),
        _ => inbox.list_items(View::Info.filter()).await.map_err(failed)?.len(),
    };
    let sources = inbox.list_sources().await.map_err(failed)?;
    Ok(Loaded { items, for_me, info, sources })
}

impl<S: InboxSource + 'static> App<S> {
    pub fn new(inbox: Arc<S>, suite: Source, launch: Launch) -> (App<S>, Task<Message>) {
        let open = launch.open;
        let offset = launch.now.map_or(TimeDelta::zero(), |now| datetime::to_utc(now) - Utc::now());
        let app = App {
            inbox,
            zoom: Zoom::for_source(&suite),
            suite,
            opened: false,
            offset,
            now: Utc::now() + offset,
            size: torro_gui::window(INBOX).size,
            // A demo state that shows a view wins over `--lane`/`--source`: it is asked for by name.
            view: match open {
                Some(Open::Info) => View::Info,
                Some(Open::Snoozed) => View::Snoozed,
                Some(Open::Done) => View::Done,
                _ => launch.view.clone().unwrap_or(View::ForMe),
            },
            items: Vec::new(),
            for_me: Vec::new(),
            info: 0,
            sources: Vec::new(),
            loaded: false,
            error: None,
            detail: None,
            cursor: None,
            scroll: None,
            subject: None,
            pick: Pick::start(Utc::now() + offset),
            task: TaskDraft::default(),
            lists: None,
            last_list: None,
            hint: None,
            #[cfg(test)]
            sent: Vec::new(),
            appearance: Appearance::System,
            system_mode: Mode::None,
            menu: match open {
                Some(Open::Menu) => Some(Menu::Nav),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            launch,
        };
        let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
        let task = Task::batch([
            ui_state::load_task(INBOX).map(Message::UiState),
            app.load(),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    pub fn title(&self) -> String {
        trf("Inbox · {view}", &[("view", &self.view.name())])
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(REFRESH).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Zoom keys work everywhere, the glance included.
            self.zoom.subscription().map(Message::Zoom),
        ];
        // Mini and strip are displays: no keys, and nothing open that Escape could close. (A key
        // like `e` must not mark an entry done that nobody sees.)
        if !self.is_glance() {
            subscriptions.push(iced::event::listen_with(keys::listen));
            // Escape closes an open menu, drawer or the panel; nothing listens while none is open.
            if self.menu.is_some() || self.drawer || self.detail.is_some() {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
        }
        if self.hint.is_some() {
            subscriptions.push(iced::time::every(HINT_TICK).map(Message::HintTick));
        }
        Subscription::batch(subscriptions)
    }

    /// The Iced theme: the appearance of `--appearance`, else the stored one, with "System"
    /// following the desktop.
    pub fn theme(&self) -> Theme {
        theme::for_appearance(self.launch.appearance.unwrap_or(self.appearance), self.system_mode)
    }

    /// The factor the whole window draws at (`.scale_factor` in `main`): the suite-wide zoom.
    pub fn scale_factor(&self) -> f32 {
        self.zoom.factor()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Resized(size) => self.size = size,
            Message::Loaded(view, result) => {
                // An answer for a view the user has left since would show the wrong entries.
                if view != self.view {
                    return Task::none();
                }
                match result {
                    Ok(loaded) => return self.take(loaded),
                    Err(err) => self.error = Some(load_failed(&err)),
                }
            }
            Message::Show(view) => {
                self.close_menus();
                self.detail = None;
                self.cursor = None;
                if view != self.view {
                    self.view = view;
                    return self.load();
                }
            }
            Message::Select(id) => {
                self.close_menus();
                self.detail = self.items.iter().find(|item| item.id == id).cloned();
                if self.detail.is_some() {
                    self.cursor = Some(id);
                }
            }
            Message::OpenForMe(id) => return self.open_for_me(id),
            Message::CloseDetail => self.detail = None,
            Message::SetDone(id, done) => return self.edit(Edit::Done { id, done }),
            Message::Edit(edit) => return self.edit(edit),
            Message::Edited(result, after) => return self.edited(result, after),
            Message::Undo => return self.undo(),
            Message::HintTick(now) => {
                // The forced hints of `--open undo` and `--open mute` stay, so they can be photographed.
                let forced = matches!(self.launch.open, Some(Open::Undo | Open::Mute));
                if !forced
                    && self.hint.as_ref().is_some_and(|hint| now.saturating_duration_since(hint.shown) >= UNDO_FOR)
                {
                    self.hint = None;
                }
            }
            Message::Key(key) => {
                if let Some(command) = keys::command(self.focus(), key) {
                    return self.run(command);
                }
            }
            Message::Scrolled(offset, height) => self.scroll = Some((offset, height)),
            Message::AskSnooze(id) => self.ask_snooze(&id),
            Message::AskLane(id) => return self.ask_lane(&id),
            Message::AskMute(id) => return self.ask_mute(&id),
            Message::ShowPicker => {
                self.pick = Pick::start(self.now);
                self.menu = Some(Menu::Pick);
            }
            Message::PickDay(day) => {
                let today = local_today(self.now);
                self.pick.day = day.clamp(today, today + Days::new(PICK_DAYS));
            }
            Message::PickHour(hour) => self.pick.hour = hour.min(PICK_HOURS.len() - 1),
            Message::PickConfirm => {
                let at = self.pick.at();
                if let Some(subject) = self.subject.as_ref().filter(|_| at > self.now) {
                    let edit = Edit::Snooze { id: subject.id.clone(), until: Some(at) };
                    return self.edit(edit);
                }
            }
            Message::Muted(Ok(()), hint) => {
                if let Some(hint) = hint {
                    self.hint = Some(Hint { shown: Instant::now(), ..hint });
                }
                return self.load();
            }
            Message::Muted(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't change the source."), err));
            }
            Message::LinkOpened(Ok(())) => {}
            Message::LinkOpened(Err(err)) => eprintln!("cannot open the link in a browser: {err}"),
            Message::Changed(Ok(item)) => {
                if let Some(detail) = self.detail.as_mut().filter(|detail| detail.id == item.id) {
                    *detail = *item;
                }
                return self.load();
            }
            Message::Changed(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't change the entry."), err));
            }
            Message::OpenItem(id) => return self.open_item(&id),
            Message::AskTask(id) => return self.ask_task(&id),
            Message::TaskLists(result) => self.take_lists(result),
            Message::TaskTitle(title) => self.task.title = title,
            Message::TaskList(list) => self.task.list = Some(list),
            Message::TaskDue(due) => self.task.due = due,
            Message::TaskConfirm => return self.create_task(),
            Message::TaskCreated(Ok(()), list) => {
                self.note(trf("Task created in {list}", &[("list", &list)]));
                return self.load();
            }
            Message::TaskCreated(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't create the task."), err));
            }
            // The entry is seen now; the list shows it.
            Message::ItemOpened(Ok(())) => return self.load(),
            Message::ItemOpened(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the entry."), err));
            }
            Message::Refresh => return self.load(),
            Message::Tick => {
                self.now = Utc::now() + self.offset;
                return Task::batch([self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom), self.load()]);
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            Message::Zoom(message) => {
                let before = self.zoom.factor();
                let task = self.zoom.update(message).map(Message::Zoom);
                if self.zoom.factor() == before {
                    return task;
                }
                // The window keeps its pixels, so its logical size changes with the zoom: at once
                // by arithmetic, then exactly from the window.
                self.size = zoom::rescale(self.size, before, self.zoom.factor());
                let size = iced::window::latest().and_then(iced::window::size).map(Message::Resized);
                return Task::batch([task, size]);
            }
            Message::Switcher(Switcher::Toggle) => self.toggle_menu(Menu::Switcher),
            Message::Switcher(Switcher::OpenApp(app)) => return self.update(Message::OpenApp(app.to_owned())),
            // The inbox has nothing to create and no search yet; the buttons only close what is open.
            Message::Switcher(Switcher::New | Switcher::Search) => self.close_menus(),
            Message::ToggleNav => self.toggle_menu(Menu::Nav),
            Message::ShowDrawer => {
                self.menu = None;
                self.drawer = true;
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::CloseMenu => self.close_menus(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                } else {
                    self.detail = None;
                }
            }
            Message::OpenApp(app) => {
                self.close_menus();
                let suite = self.suite.clone();
                return Task::perform(async move { suite.open_app(&app).await }, Message::AppOpened);
            }
            Message::AppOpened(Ok(())) => {}
            Message::AppOpened(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the app."), err));
            }
            Message::OpenSettings => {
                self.close_menus();
                connect::open_settings();
            }
        }
        Task::none()
    }

    /// Opens the entry `id` if it is the one in the panel: a message can outlive the entry it came
    /// from.
    fn open_item(&mut self, id: &str) -> Task<Message> {
        match self.detail.as_ref().filter(|item| item.id == id).cloned() {
            Some(item) => self.open_entry(&item),
            None => Task::none(),
        }
    }

    /// Opens what `item` links to, if [`detail::openable`] offers it, through the core
    /// (`inbox.open_item`, task E2.4): the core focuses the agent's window, starts the browser or
    /// the app and checks security rule 6 again; this window opens nothing itself.
    ///
    /// With `--demo` there is no core behind the source: pages still open in the browser and apps
    /// through the suite from here, as before; "Go to window" reaches the demo source, which only
    /// marks the entry seen.
    fn open_entry(&mut self, item: &Item) -> Task<Message> {
        let Some(link) = detail::openable(item) else { return Task::none() };
        if self.launch.demo {
            match link {
                Link::Url { url } => return self.open_url(url),
                Link::App { app } if app == AppId::WORKPLACE.binary => return self.update(Message::OpenSettings),
                Link::App { app } => return self.update(Message::OpenApp(app)),
                Link::Window => {}
            }
        }
        #[cfg(test)]
        self.sent.push(Sent::Open(item.id.clone()));
        let (inbox, id) = (self.inbox.clone(), item.id.clone());
        Task::perform(async move { inbox.open_item(id).await.map_err(|err| err.to_string()) }, Message::ItemOpened)
    }

    /// Opens a web page in the browser, for `--demo` only (without it the core opens pages). Only
    /// called with what [`detail::openable`] allowed. Like the other apps: a browser that cannot
    /// be started is only logged.
    fn open_url(&mut self, url: String) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(Sent::Link(url.clone()));
        Task::perform(async move { webbrowser::open(&url).map_err(|err| err.to_string()) }, Message::LinkOpened)
    }

    /// Takes the source's answer for the view: entries, count and sources. The cursor stays on
    /// its entry; when the entry left the list (done, snoozed, moved), it moves to the row that
    /// took its place, and the panel that showed it moves along (or closes with an empty list).
    ///
    /// Waiting entries ([`list::is_waiting`]) go first, keeping the source's order otherwise. The
    /// source already sorts them so ([`torro_inbox::rules`]); this makes sure that the group
    /// "Waiting for you" holds all of them and that the keys walk the rows as they are shown.
    fn take(&mut self, loaded: Loaded) -> Task<Message> {
        let place = self.cursor_place();
        let followed = self.detail.as_ref().is_some_and(|detail| self.cursor.as_deref() == Some(detail.id.as_str()));
        self.items = loaded.items;
        self.items.sort_by_key(|item| !list::is_waiting(item));
        self.for_me = loaded.for_me;
        self.for_me.sort_by_key(|item| !list::is_waiting(item));
        self.info = loaded.info;
        self.sources = loaded.sources;
        self.loaded = true;
        // The forced error line stays over every load, so it can be photographed.
        self.error = (self.launch.open == Some(Open::Failed))
            .then(|| Failure::new(tr("Couldn't change the entry."), DEMO_ERROR));
        if self.cursor_item().is_none() {
            let last = self.items.len().checked_sub(1);
            self.cursor =
                place.zip(last).and_then(|(place, last)| self.items.get(place.min(last))).map(|i| i.id.clone());
        }
        if let Some(detail) = &self.detail {
            // The panel follows the entry when it changed (a new report, woken up).
            let fresh = self.items.iter().find(|item| item.id == detail.id).cloned();
            match fresh {
                Some(fresh) => self.detail = Some(fresh),
                None if followed => self.detail = self.cursor_item().cloned(),
                None => {}
            }
        }
        self.open_launched()
    }

    /// From the glance: the entry `id` of "For me" under the cursor and in the panel, with "For me"
    /// as the view (loaded anew when another view was chosen). The glance highlights it; a larger
    /// window shows it in the panel. An id no longer in "For me" does nothing.
    fn open_for_me(&mut self, id: String) -> Task<Message> {
        let Some(item) = self.for_me.iter().find(|item| item.id == id).cloned() else { return Task::none() };
        self.close_menus();
        self.cursor = Some(id);
        self.detail = Some(item);
        if self.view == View::ForMe {
            return Task::none();
        }
        self.view = View::ForMe;
        self.load()
    }

    /// The entry under the cursor, if it is in the list.
    fn cursor_item(&self) -> Option<&Item> {
        let id = self.cursor.as_deref()?;
        self.items.iter().find(|item| item.id == id)
    }

    /// Where the cursor's row is in the list.
    fn cursor_place(&self) -> Option<usize> {
        let id = self.cursor.as_deref()?;
        self.items.iter().position(|item| item.id == id)
    }

    /// The entry with `id`: in the list, or the one in the panel or an open menu.
    fn find(&self, id: &str) -> Option<&Item> {
        self.items.iter().chain(self.detail.as_ref()).chain(self.subject.as_ref()).find(|item| item.id == id)
    }

    /// Where the keys go: a menu of choices, the picker, another menu or the drawer, else the list.
    fn focus(&self) -> Focus {
        match self.menu {
            Some(Menu::Snooze | Menu::Lane) => Focus::Choices,
            Some(Menu::Pick) => Focus::Picker,
            Some(Menu::Task) => Focus::Task,
            Some(Menu::Nav | Menu::Switcher) => Focus::Elsewhere,
            None if self.drawer => Focus::Elsewhere,
            None => Focus::List,
        }
    }

    /// Carries out what a key does ([`keys::command`]).
    fn run(&mut self, command: Command) -> Task<Message> {
        let cursor = self.cursor_item().map(|item| item.id.clone());
        match command {
            Command::Next => self.step(true),
            Command::Previous => self.step(false),
            Command::Open => self.open_cursor(),
            Command::Done => match self.cursor_item().cloned() {
                Some(item) if detail::is_approval(&item) => {
                    self.note(tr("An approval closes once it is decided.").to_owned());
                    Task::none()
                }
                Some(item) => {
                    let done = !matches!(item.state, State::Done { .. });
                    self.edit(Edit::Done { id: item.id, done })
                }
                None => Task::none(),
            },
            Command::Undo => self.undo(),
            Command::Snooze => {
                if let Some(id) = cursor {
                    self.ask_snooze(&id);
                }
                Task::none()
            }
            Command::Lane => cursor.map_or_else(Task::none, |id| self.ask_lane(&id)),
            Command::Mute => cursor.map_or_else(Task::none, |id| self.ask_mute(&id)),
            Command::MakeTask => cursor.map_or_else(Task::none, |id| self.ask_task(&id)),
            Command::Choose(n) => match self.choices().into_iter().nth(n.wrapping_sub(1)) {
                Some(choice) => self.update(choice.message),
                None => Task::none(),
            },
            Command::EarlierDay => self.update(Message::PickDay(self.pick.day - Days::new(1))),
            Command::LaterDay => self.update(Message::PickDay(self.pick.day + Days::new(1))),
            Command::EarlierTime => self.update(Message::PickHour(self.pick.hour.saturating_sub(1))),
            Command::LaterTime => self.update(Message::PickHour(self.pick.hour + 1)),
            Command::Confirm if self.menu == Some(Menu::Task) => self.create_task(),
            Command::Confirm => self.update(Message::PickConfirm),
            Command::Sidebar => self.toggle_sidebar(),
        }
    }

    /// Moves the cursor one row down or up and stops at the first and last row; with no cursor
    /// yet, down chooses the first row and up the last. The open panel shows the new row.
    fn step(&mut self, forward: bool) -> Task<Message> {
        let Some(last) = self.items.len().checked_sub(1) else { return Task::none() };
        let next = match (self.cursor_place(), forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(place), true) => (place + 1).min(last),
            (Some(place), false) => place.saturating_sub(1),
        };
        let item = self.items[next].clone();
        self.cursor = Some(item.id.clone());
        if self.detail.is_some() {
            self.detail = Some(item);
        }
        self.reveal(next)
    }

    /// Scrolls the list the least that shows row `index` completely – with its group head, when
    /// it is the first of its group ([`list::row_span`]).
    fn reveal(&mut self, index: usize) -> Task<Message> {
        let Some((offset, height)) = self.scroll else { return Task::none() };
        let row = list::row_height(self.two_line());
        let Some((top, bottom)) = list::row_span(&self.items, index, row) else { return Task::none() };
        let target = if top < offset {
            top
        } else if bottom > offset + height {
            bottom - height
        } else {
            return Task::none();
        };
        // The viewport is only reported on the next frame; keys can repeat faster than that.
        self.scroll = Some((target, height));
        operation::scroll_to(list::SCROLL, AbsoluteOffset { x: None, y: Some(target) })
    }

    /// Enter: opens what the entry under the cursor links to – its page, its app, the agent's
    /// window – as [`App::open_entry`] does; an entry without such a link opens in the panel.
    fn open_cursor(&mut self) -> Task<Message> {
        let Some(item) = self.cursor_item().cloned() else { return Task::none() };
        match detail::openable(&item) {
            Some(_) => self.open_entry(&item),
            None => self.update(Message::Select(item.id)),
        }
    }

    /// Opens the snooze menu for the entry with `id`. Done entries are not snoozed.
    fn ask_snooze(&mut self, id: &str) {
        let Some(item) = self.find(id).filter(|item| !matches!(item.state, State::Done { .. })).cloned() else {
            return;
        };
        self.drawer = false;
        self.subject = Some(item);
        self.menu = Some(Menu::Snooze);
    }

    /// Moves the entry with `id` to the other lane. Where a rule is possible ([`rules_allowed`])
    /// the lane question opens first ("Only this entry" / "Always for …").
    fn ask_lane(&mut self, id: &str) -> Task<Message> {
        let Some(item) = self.find(id).cloned() else { return Task::none() };
        if !rules_allowed(&item.source) {
            return self.edit(Edit::Lane { id: item.id, lane: other_lane(item.lane), remember: false });
        }
        self.drawer = false;
        self.subject = Some(item);
        self.menu = Some(Menu::Lane);
        Task::none()
    }

    /// Mutes the source of the entry with `id`, or unmutes it, at once; the hint offers the undo
    /// (design: confirm *or* undo, never both). The suite is never muted ([`rules_allowed`]).
    fn ask_mute(&mut self, id: &str) -> Task<Message> {
        let Some(source) = self.find(id).map(|item| item.source.clone()) else { return Task::none() };
        if !rules_allowed(&source) {
            self.note(tr("Entries from the suite can't be muted.").to_owned());
            return Task::none();
        }
        self.close_menus();
        let muted = !self.is_muted(&source);
        self.mute(source, muted, true)
    }

    /// Opens the dialog that makes a task of the entry with `id` (task E3.5): its title to change,
    /// no due day, the lists asked for anew (a list made meanwhile shows up), the title field
    /// focused. An approval closes once it is decided; it becomes no task.
    fn ask_task(&mut self, id: &str) -> Task<Message> {
        let Some(item) = self.find(id).cloned() else { return Task::none() };
        if detail::is_approval(&item) {
            self.note(tr("An approval closes once it is decided.").to_owned());
            return Task::none();
        }
        self.drawer = false;
        self.task = TaskDraft { title: list::one_line(&item.title), list: None, due: Due::None };
        self.subject = Some(item);
        self.menu = Some(Menu::Task);
        self.lists = None;
        #[cfg(test)]
        self.sent.push(Sent::Lists);
        let inbox = self.inbox.clone();
        let lists = async move { inbox.list_task_lists().await.map_err(|err| err.to_string()) };
        Task::batch([Task::perform(lists, Message::TaskLists), operation::focus(TASK_TITLE)])
    }

    /// The task lists for the open dialog: the list chosen is the one used last, else the first.
    fn take_lists(&mut self, result: Result<Vec<TaskList>, String>) {
        if self.menu != Some(Menu::Task) {
            return;
        }
        if let Ok(lists) = &result {
            let last = lists.iter().find(|list| Some(&list.id) == self.last_list.as_ref());
            self.task.list = last.or(lists.first()).cloned();
        }
        self.lists = Some(result);
    }

    /// "Create task": sends `inbox.create_task_from_item` for the dialog's entry, list, title
    /// (none when blank: the core takes the entry's) and due day, and closes the dialog. Nothing
    /// without a list.
    fn create_task(&mut self) -> Task<Message> {
        let (Some(item), Some(list)) = (self.subject.clone(), self.task.list.clone()) else { return Task::none() };
        let title = Some(self.task.title.trim().to_owned()).filter(|title| !title.is_empty());
        let due = self.task.due.day(self.now);
        self.last_list = Some(list.id.clone());
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::Task { id: item.id.clone(), list: list.clone(), title: title.clone(), due });
        let inbox = self.inbox.clone();
        let TaskList { id: list_id, name } = list;
        Task::perform(
            async move { inbox.create_task_from_item(item.id, list_id, title, due).await.map_err(|err| err.to_string()) },
            move |result| Message::TaskCreated(result, name.clone()),
        )
    }

    /// Whether `source` is muted, as the source list says.
    fn is_muted(&self, source: &SourceRef) -> bool {
        self.sources.iter().any(|info| info.source == *source && info.muted)
    }

    /// Mutes or unmutes `source` (`inbox.mute_source`); the answer comes as [`Message::Muted`],
    /// with the hint and its undo when `with_hint` (an undo itself shows none, as for edits).
    fn mute(&mut self, source: SourceRef, muted: bool, with_hint: bool) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(Sent::Mute(source.clone(), muted, with_hint));
        let hint = with_hint.then(|| mute_hint(&source, muted));
        let inbox = self.inbox.clone();
        Task::perform(
            async move { inbox.mute_source(source, muted).await.map_err(|err| err.to_string()) },
            move |result| Message::Muted(result, hint.clone()),
        )
    }

    /// Sends `edit` with its hint and undo, worked out from the entry as it is now.
    fn edit(&mut self, edit: Edit) -> Task<Message> {
        let Some(after) = self.find(edit.id()).map(|before| edit.after(before, self.now)) else {
            return Task::none();
        };
        self.close_menus();
        self.send(edit, Some(after))
    }

    /// "Undo": sends the undo of the hint, which then goes. Nothing while no hint can be undone.
    fn undo(&mut self) -> Task<Message> {
        let Some(undo) = self.hint.take().and_then(|hint| hint.undo) else { return Task::none() };
        self.close_menus();
        match undo {
            Undo::Edit(edit) => self.send(edit, None),
            Undo::Mute(source, muted) => self.mute(source, muted, false),
        }
    }

    fn send(&mut self, edit: Edit, after: Option<After>) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(Sent::Edit(edit.clone(), after.clone()));
        let inbox = self.inbox.clone();
        Task::perform(edit.apply(inbox), move |result| Message::Edited(result.map(Box::new), after.clone()))
    }

    /// The answer to an edit: the hint (with undo) for a change, the cursor back on the entry
    /// after an undo; then as any change ([`Message::Changed`]).
    fn edited(&mut self, result: Result<Box<Item>, String>, after: Option<After>) -> Task<Message> {
        if let Ok(item) = &result {
            match after {
                Some(After { text, undo }) => {
                    self.hint = Some(Hint { text, undo: undo.map(Undo::Edit), shown: Instant::now() });
                }
                None => {
                    // Undone: the entry is back in its place, and the cursor (and panel) with it.
                    self.cursor = Some(item.id.clone());
                    if self.detail.is_some() {
                        self.detail = Some(Item::clone(item));
                    }
                }
            }
        }
        self.update(Message::Changed(result))
    }

    /// A hint without undo: what happened, or why nothing did.
    fn note(&mut self, text: String) {
        self.hint = Some(Hint { text, undo: None, shown: Instant::now() });
    }

    /// The entries of the open menu of choices, in order; their digits are their place from 1.
    pub fn choices(&self) -> Vec<Choice> {
        let Some(item) = &self.subject else { return Vec::new() };
        let id = || item.id.clone();
        let choice = |icon, label: String, message| Choice { icon, label, when: None, message };
        match self.menu {
            Some(Menu::Snooze) => {
                let [later, tomorrow, next_week] = snooze_times(self.now);
                let snooze = |icon, label: &str, until| Choice {
                    icon,
                    label: label.to_owned(),
                    when: Some(wake_short(until, self.now)),
                    message: Message::Edit(Edit::Snooze { id: id(), until: Some(until) }),
                };
                let mut choices = vec![
                    snooze(Icon::Clock, tr("Later today"), later),
                    snooze(Icon::Sun, tr("Tomorrow"), tomorrow),
                    snooze(Icon::CalendarDays, tr("Next week"), next_week),
                    choice(Icon::CalendarClock, tr("Pick…").to_owned(), Message::ShowPicker),
                ];
                if matches!(item.state, State::Snoozed { .. }) {
                    let wake = Message::Edit(Edit::Snooze { id: id(), until: None });
                    choices.push(choice(Icon::Inbox, tr("Unsnooze").to_owned(), wake));
                }
                choices
            }
            Some(Menu::Lane) => {
                let lane = other_lane(item.lane);
                let edit = |remember| Message::Edit(Edit::Lane { id: id(), lane, remember });
                let always = trf("Always for {source}", &[("source", &item.source.label())]);
                vec![
                    choice(lane_icon(lane), tr("Only this entry").to_owned(), edit(false)),
                    choice(Icon::Repeat, always, edit(true)),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// The panel button and `[`: in the wide layout the other sidebar mode, remembered for the
    /// next start (not in the demo, which stores nothing); else the drawer opens or closes.
    fn toggle_sidebar(&mut self) -> Task<Message> {
        if !shell::has_sidebar(self.class()) {
            self.menu = None;
            self.drawer = !self.drawer;
            return Task::none();
        }
        self.close_menus();
        self.sidebar = self.sidebar.toggled();
        if self.launch.demo {
            return Task::none();
        }
        ui_state::save_task(INBOX, UiState { sidebar: self.sidebar })
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// Mini or strip: a display without header, sidebar, menus, panel or keys.
    fn is_glance(&self) -> bool {
        matches!(self.class(), SizeClass::Mini | SizeClass::Strip)
    }

    /// The window with the zoom badge over it; in mini and strip the glance.
    pub fn view(&self) -> Element<'_, Message> {
        let class = self.class();
        if self.is_glance() {
            // The panel, menus, the drawer and the hint stay in the state and come back with a
            // larger window.
            // Before the first load the glance is the loading or error state on the window itself.
            let surface = if self.loaded { Surface::of(class) } else { Surface::Window };
            return widgets::ctrl_guard(shell::titlebar(
                self.zoom.view(self.glance(class)),
                self.zoom.factor(),
                surface,
                Message::DragWindow,
            ));
        }
        // Always two layers, so the window keeps its state when the hint comes and goes.
        let window = stack![self.window(class), self.hint_layer()].width(Fill).height(Fill);
        let surface = Surface::of(class);
        widgets::ctrl_guard(shell::titlebar(self.zoom.view(window), self.zoom.factor(), surface, Message::DragWindow))
    }

    /// The hint at the bottom centre, over everything but the zoom badge; empty without a hint.
    fn hint_layer(&self) -> Element<'_, Message> {
        let Some(hint) = &self.hint else { return Space::new().into() };
        let mut content = row![typo::text(typo::BODY, hint.text.as_str()).style(theme::text).wrapping(Wrapping::None)]
            .spacing(space::S)
            .align_y(Alignment::Center);
        if hint.undo.is_some() {
            content = content
                .push(widgets::text_button(None, tr("Undo"), style::button::ghost, Some(Message::Undo)))
                .push(widgets::kbd("U"));
        }
        let card = container(content)
            .center_y(size::CONTROL_LG)
            .padding(Padding::new(0.0).left(space::M).right(space::S))
            .style(style::container::menu);
        container(card).width(Fill).height(Fill).center_x(Fill).align_bottom(Fill).padding(space::XL).into()
    }

    /// Mini or strip: loading and the error of a first load as in every layout, then the glance at
    /// "For me" (`glance.rs`).
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let data = GlanceData {
            items: &self.for_me,
            // Only an entry open in the panel: a keyboard cursor left over from a larger window is no
            // reason to single out a row of a display.
            selected: self.detail.as_ref().map(|item| item.id.as_str()),
            now: self.now,
            error: self.error.as_ref().map(|failure| failure.what),
        };
        match class {
            SizeClass::Strip => glance::strip(&data, self.size),
            _ => glance::mini(&data, self.size),
        }
    }

    /// Loading, or the error of the first load, until entries have come once; none afterwards.
    fn unloaded(&self) -> Option<Element<'_, Message>> {
        if self.loaded {
            return None;
        }
        Some(match &self.error {
            Some(failure) => failure_state(failure),
            None => widgets::loading(),
        })
    }

    /// The whole window in the layout of `class` (not mini or strip: [`App::view`] shows the glance
    /// there; the arms for them only keep the matches whole).
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let (switcher, nav) = (Anchor::new(), Anchor::new());
        let sidebar = self.sidebar(SidebarMode::Expanded, &switcher);
        let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
            .then(|| self.sidebar(SidebarMode::Rail, &switcher));
        let header = self.header(class, &nav);
        let anchors = detail::Anchors::default();
        // Menus for an entry open at the panel's buttons while it is open (the row may be under
        // it), else at the cursor's row.
        let (snooze_at, lane_at, task_at) = if self.detail.is_some() {
            (&anchors.snooze, &anchors.lane, &anchors.task)
        } else {
            (&anchors.row, &anchors.row, &anchors.row)
        };
        let below = Placement::Below(Align::End);
        let layer = match self.menu {
            None => None,
            Some(Menu::Switcher) => Some(widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(INBOX, Message::Switcher),
                Message::CloseMenu,
            )),
            Some(Menu::Nav) => {
                Some(widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu))
            }
            Some(Menu::Snooze) => {
                Some(widgets::floating(snooze_at, below, self.choice_menu(tr("Snooze until")), Message::CloseMenu))
            }
            Some(Menu::Lane) => {
                let title = self.subject.as_ref().map_or_else(String::new, |item| {
                    trf("Move to {lane}", &[("lane", &lane_name(other_lane(item.lane)))])
                });
                Some(widgets::floating(lane_at, below, self.choice_menu(title), Message::CloseMenu))
            }
            Some(Menu::Pick) => Some(widgets::floating(snooze_at, below, self.picker(), Message::CloseMenu)),
            Some(Menu::Task) => Some(widgets::floating(task_at, below, self.task_dialog(), Message::CloseMenu)),
        };
        let detail = self.detail.as_ref().map(|item| {
            let footer = detail::Footer {
                anchors: anchors.clone(),
                muted: self.is_muted(&item.source),
                // The panel fills narrow windows: no room for the words of "Move to …".
                compact: !matches!(class, SizeClass::Wide | SizeClass::Medium),
            };
            detail::panel(item, self.view.name(), self.now, footer)
        });
        let placed = panel::place(class, self.body(class, &anchors.row), detail, layer);
        shell::shell(class, sidebar, header, placed.body, None)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    /// The header: the view's title (wide with the full sidebar) or the navigation menu (with the
    /// rail, medium, narrow), its count, and
    /// "Refresh". The inbox creates nothing, so there is no primary button.
    fn header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Message> {
        let (name, icon) = (self.view.name(), self.view.icon());
        // Nothing listed: the empty state says so, a "0" beside the title would only repeat it.
        let note = (self.loaded && !self.items.is_empty()).then(|| self.count_note());
        let trailing = [widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh))];
        // The rail has no sources (see `sidebar`), so with it the title is the navigation menu,
        // as in the medium layout, and every view stays one click away.
        let rail = shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail);
        match class {
            SizeClass::Wide if !rail => shell::header_bar([], shell::header_title(Some(icon), name, note), trailing),
            SizeClass::Wide | SizeClass::Medium => {
                // The count right at the button, as at the title in the wide header: the button's
                // own padding is the gap.
                let mut title =
                    row![widgets::menu_button(nav, Some(icon), name, Message::ToggleNav)].align_y(Alignment::Center);
                if let Some(note) = note {
                    title = title.push(typo::text(typo::BODY, note).style(theme::text_3).wrapping(Wrapping::None));
                }
                // The rail at the left has the panel button.
                shell::header_bar([], title, trailing)
            }
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                [shell::sidebar_toggle(Message::ShowDrawer)],
                widgets::menu_button(nav, Some(icon), name, Message::ToggleNav),
                trailing,
            ),
        }
    }

    /// The quiet number after the title: how many entries the view lists. The title already says
    /// which view it is, so the number stands alone (as in torrotasks' header).
    fn count_note(&self) -> String {
        self.items.len().to_string()
    }

    /// Two lines per row (title over preview): mini and narrow windows. The strip is wide enough
    /// for one line.
    fn two_line(&self) -> bool {
        matches!(self.class(), SizeClass::Narrow | SizeClass::Mini)
    }

    /// The content below the header: an error line if there is one, and the list – left-aligned
    /// under the header's title (design: "links mit Seitenleiste"); in the wide layout at most
    /// `CONTENT_MAX` wide, also while the panel stands beside it (its column takes its own room,
    /// and a narrower list would only leave a gap). Loading and the error of a first load stand in
    /// for both.
    fn body(&self, class: SizeClass, row_anchor: &Anchor) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let data = list::ListData {
            items: &self.items,
            view: &self.view,
            selected: self.cursor.as_deref(),
            now: self.now,
            anchor: row_anchor.clone(),
            two_line: self.two_line(),
            attention: palette(&self.theme()).attention_text,
        };
        let mut content = column![];
        if let Some(failure) = &self.error {
            content = content.push(container(failure_line(failure)).padding(Padding::new(space::S).left(space::L)));
        }
        let list = container(content.push(list::view(data)).width(Fill).height(Fill)).width(Fill);
        // Only the wide layout caps the width: in the medium one the content is at most a little
        // over `CONTENT_MAX` anyway, and a cap there would leave a thin empty strip at the right
        // (at 800 wide about 20) that reads as a mistake rather than as a margin.
        match class {
            SizeClass::Wide => list.max_width(size::CONTENT_MAX).into(),
            SizeClass::Medium | SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => list.into(),
        }
    }

    /// The views with their icon, then one entry per source.
    ///
    /// Every number is what the view behind it lists: open entries. "For me" and "Info" count
    /// their lane, a source its open entries in both lanes – so the sources add up to "For me"
    /// plus "Info". "Snoozed" and "Done" are put away and have no number. No "0" anywhere.
    fn nav_entries(&self) -> (Vec<NavEntry>, Vec<NavEntry>) {
        let entry = |view: View, count: Option<usize>| NavEntry {
            mark: Mark::Icon(view.icon()),
            label: view.name(),
            count,
            active: view == self.view,
            view,
        };
        let views = vec![
            // Like a badge: no "0" when nothing waits.
            entry(View::ForMe, Some(self.for_me.len()).filter(|&n| n > 0)),
            entry(View::Info, Some(self.info).filter(|&n| n > 0)),
            entry(View::Snoozed, None),
            entry(View::Done, None),
        ];
        let sources = self
            .sources
            .iter()
            .map(|info| {
                let view = View::Source(info.source.clone());
                let label = if info.muted {
                    trf("{source} (muted)", &[("source", &info.source.label())])
                } else {
                    info.source.label()
                };
                let count = usize::try_from(info.open).ok().filter(|&open| open > 0);
                NavEntry { mark: Mark::Icon(view.icon()), label, count, active: view == self.view, view }
            })
            .collect();
        (views, sources)
    }

    /// The sidebar in `mode`: expanded (wide, drawer) or as the rail of icons. The rail leaves the
    /// sources out: all agents share one icon and all scripts another, so as bare icons they
    /// could not be told apart; the navigation menu and the drawer still list them.
    fn sidebar<'a>(&'a self, mode: SidebarMode, switcher: &Anchor) -> Element<'a, Message> {
        let (views, sources) = self.nav_entries();
        let row = |e: NavEntry| {
            let trail = e.trail();
            widgets::sidebar_item_in(mode, e.mark, e.label, trail, e.active, Message::Show(e.view))
        };
        let mut items: Vec<Element<'a, Message>> = views.into_iter().map(row).collect();
        if !sources.is_empty() && mode == SidebarMode::Expanded {
            items.push(widgets::sidebar_group_in(mode, tr("Sources"), None));
            items.extend(sources.into_iter().map(row));
        }
        let footer = vec![widgets::sidebar_item_in(
            mode,
            Icon::Settings,
            tr("Settings"),
            Trail::None,
            false,
            Message::OpenSettings,
        )];
        let head = shell::sidebar_head(mode, INBOX, switcher, Message::Switcher, Message::ToggleSidebar);
        shell::sidebar(head, items, footer)
    }

    /// The navigation menu of the medium and narrow layouts: the sidebar's rows as a menu.
    fn nav_menu(&self) -> Element<'_, Message> {
        let (views, sources) = self.nav_entries();
        let item = |e: NavEntry| {
            let trail = e.trail();
            MenuItem::new(e.mark, e.label, Message::Show(e.view)).trail(trail).current(e.active)
        };
        let mut sections = vec![MenuSection::new(views.into_iter().map(item).collect())];
        if !sources.is_empty() {
            sections.push(MenuSection::titled(tr("Sources"), sources.into_iter().map(item).collect()));
        }
        sections.push(MenuSection::new(vec![MenuItem::new(Icon::Settings, tr("Settings"), Message::OpenSettings)]));
        widgets::menu(sections)
    }

    /// The open menu of choices ([`App::choices`]) under `title`, each entry with its time (when it
    /// has one) and its digit. The menu card of `widgets::menu`, built from the same pieces here:
    /// its entries end in one trail only, and these need the time quietly before the key hint.
    fn choice_menu<'a>(&self, title: impl Into<String>) -> Element<'a, Message> {
        let head = typo::text(typo::CAPTION, title.into()).style(theme::text_3);
        let entries =
            self.choices().into_iter().enumerate().map(|(i, choice)| choice_entry(choice, keys::digit(i + 1)));
        let list = column![container(head).padding(Padding::new(space::S).bottom(space::XS))].extend(entries);
        container(list).width(widgets::MENU_WIDTH).padding(style::MENU_INSET).style(style::container::menu).into()
    }

    /// The picker of "Pick…" as a floating card: the chosen moment as title ("Wed, 14 Oct · 09:00"),
    /// the day with previous / next, the time of day as segments, and "Snooze" at the right (off
    /// for a time already past). Iced has no date picker and the suite none of its own yet, so it
    /// steps through days instead of showing a month; ←/→ and ↑/↓ do the same from the keyboard.
    fn picker(&self) -> Element<'_, Message> {
        let today = local_today(self.now);
        let (day, at) = (self.pick.day, self.pick.at());
        // The full date: the day row below already says "Tomorrow", the title should not repeat it.
        let title = format!(
            "{} · {}",
            datetime::format_short_date(day, today),
            datetime::format_time(datetime::to_local(at).time())
        );
        let previous = (day > today).then(|| Message::PickDay(day - Days::new(1)));
        let next = (day < today + Days::new(PICK_DAYS)).then(|| Message::PickDay(day + Days::new(1)));
        let day_row = row![
            widgets::icon_button(Icon::ChevronLeft, tr("Previous day"), previous),
            typo::text(typo::BODY, day_name(day, today)).style(theme::text).wrapping(Wrapping::None),
            widgets::icon_button(Icon::ChevronRight, tr("Next day"), next),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);
        let hours = PICK_HOURS.iter().enumerate().map(|(i, &hour)| {
            let time = NaiveTime::MIN + TimeDelta::hours(i64::from(hour));
            (datetime::format_time(time), Message::PickHour(i))
        });
        let rows = vec![
            widgets::prop_row(Some(Mark::Icon(Icon::Calendar)), day_row, None),
            widgets::prop_row(Some(Mark::Icon(Icon::Clock)), widgets::segmented(hours, self.pick.hour), None),
        ];
        let snooze = (at > self.now).then_some(Message::PickConfirm);
        // At the right, where the action of a dialog is.
        let actions = vec![
            iced::widget::space::horizontal().into(),
            widgets::text_button(Some(Icon::Clock), tr("Snooze"), style::button::primary, snooze),
        ];
        widgets::popover(widgets::section_title(tr("Snooze until")), title, rows, actions)
    }

    /// The dialog of "Make task" as a floating card at the entry (the shape of `widgets::popover`,
    /// whose title is text only; here the title is the field): "Make task" over the title field,
    /// the list and the due day as properties, and at the right "Cancel" and "Create task" (off
    /// until a list is chosen). The list row says so while the lists load, when there are none and
    /// when they could not be read.
    fn task_dialog(&self) -> Element<'_, Message> {
        let inset = Padding::new(0.0).left(space::S).right(space::S);
        let title = text_input(tr("Task title"), &self.task.title)
            .id(TASK_TITLE)
            .size(typo::BODY.size)
            .line_height(typo::BODY.line_height)
            .font(typo::BODY.font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::TaskTitle)
            .on_submit(Message::TaskConfirm);
        let list: Element<'_, Message> = match &self.lists {
            None => widgets::prop_placeholder(tr("Loading lists…")),
            Some(Err(_)) => widgets::prop_placeholder(tr("Couldn't load the task lists.")),
            Some(Ok(lists)) if lists.is_empty() => widgets::prop_placeholder(tr("No task lists yet")),
            Some(Ok(lists)) => choice(lists.clone(), self.task.list.clone(), Message::TaskList),
        };
        let due = choice(Due::ALL.to_vec(), Some(self.task.due), Message::TaskDue);
        let rows = column![
            widgets::prop_line(tr("List"), Some(Mark::Icon(Icon::ListTodo)), list, None),
            widgets::prop_line(tr("Due"), Some(Mark::Icon(Icon::Calendar)), due, None),
        ];
        let create = self.task.list.is_some().then_some(Message::TaskConfirm);
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseMenu)),
            widgets::text_button(Some(Icon::Check), tr("Create task"), style::button::primary, create),
        ]
        .spacing(space::S)
        .align_y(Alignment::Center);
        let card = column![
            container(widgets::section_title(tr("Make task"))).padding(inset.bottom(space::XS)),
            container(title).padding(Padding::new(0.0).bottom(space::S)),
            rows,
            column![iced::widget::rule::horizontal(size::HAIRLINE).style(style::rule), actions]
                .spacing(space::M)
                .padding(inset.top(space::M)),
        ];
        container(card)
            .width(widgets::POPOVER_WIDTH)
            .padding([space::L, space::S])
            .style(style::container::popover)
            .into()
    }

    fn toggle_menu(&mut self, menu: Menu) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
    }

    fn close_menus(&mut self) {
        self.menu = None;
        self.drawer = false;
        self.subject = None;
    }

    /// Asks the core for the stored appearance, on every tick, so a change in the control centre
    /// reaches the open window.
    fn fetch_appearance(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { theme::stored_appearance(&suite).await }, Message::Appearance)
    }

    /// The panel states of `--open`, once the first entries are in: a panel needs an entry. The
    /// task asks for what a state needs (the task lists of `--open task`).
    fn open_launched(&mut self) -> Task<Message> {
        if std::mem::replace(&mut self.opened, true) {
            return Task::none();
        }
        let first = self.items.first().map(|item| item.id.clone());
        // The first entry not from the suite: one with a lane question and a mute button.
        let script = self.items.iter().find(|item| rules_allowed(&item.source)).cloned();
        match self.launch.open {
            Some(Open::Detail) => self.detail = self.items.first().cloned(),
            Some(Open::Approval) => self.detail = self.items.iter().find(|item| detail::is_approval(item)).cloned(),
            Some(Open::Snooze | Open::Pick) => {
                self.cursor = first.clone();
                self.ask_snooze(first.as_deref().unwrap_or_default());
                if self.launch.open == Some(Open::Pick) {
                    let _ = self.update(Message::ShowPicker);
                }
            }
            Some(Open::Lane) => {
                let id = script.map(|item| item.id).unwrap_or_default();
                self.cursor = Some(id.clone());
                let _ = self.ask_lane(&id);
            }
            Some(Open::Mute) => {
                if let Some(item) = script {
                    self.cursor = Some(item.id.clone());
                    self.hint = Some(mute_hint(&item.source, true));
                }
            }
            Some(Open::Undo) => {
                if let Some(item) = self.items.first() {
                    self.hint = Some(Hint {
                        text: tr("Marked as done").to_owned(),
                        undo: Some(Undo::Edit(Edit::Done { id: item.id.clone(), done: false })),
                        shown: Instant::now(),
                    });
                }
            }
            Some(Open::Task) => {
                if let Some(item) = self.items.iter().find(|item| !detail::is_approval(item)).cloned() {
                    self.cursor = Some(item.id.clone());
                    self.detail = Some(item.clone());
                    return self.ask_task(&item.id);
                }
            }
            _ => {}
        }
        if let Some(detail) = &self.detail {
            self.cursor = Some(detail.id.clone());
        }
        Task::none()
    }

    fn load(&self) -> Task<Message> {
        // The forced states: loading never ends, the error comes at once, empty has no entries.
        let view = self.view.clone();
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => return Task::done(Message::Loaded(view, Err(DEMO_ERROR.into()))),
            _ => {}
        }
        let empty = self.launch.open == Some(Open::Empty);
        let inbox = self.inbox.clone();
        Task::perform(fetch(inbox, view.clone(), empty), move |result| Message::Loaded(view.clone(), result))
    }
}

/// A row of the sidebar and the navigation menu.
struct NavEntry {
    mark: Mark,
    label: String,
    view: View,
    /// Shown as counter where there is one.
    count: Option<usize>,
    active: bool,
}

impl NavEntry {
    fn trail(&self) -> Trail {
        self.count.map_or(Trail::None, Trail::Count)
    }
}

/// The hint after muting (`muted`) or unmuting `source`: "Muted deploy" with the undo that sets it
/// back.
fn mute_hint(source: &SourceRef, muted: bool) -> Hint {
    let label = source.label();
    let text = if muted {
        trf("New entries from {source} muted", &[("source", &label)])
    } else {
        trf("Unmuted {source}", &[("source", &label)])
    };
    Hint { text, undo: Some(Undo::Mute(source.clone(), !muted)), shown: Instant::now() }
}

/// One entry of [`App::choice_menu`], as `widgets::menu` draws one: icon, label, then – quietly,
/// tabular – when it snoozes until, and the digit that picks it.
fn choice_entry<'a>(choice: Choice, key: &'static str) -> Element<'a, Message> {
    let mark = icon::icon(choice.icon, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text) });
    let label = container(typo::text(typo::BODY, choice.label).wrapping(Wrapping::None)).width(Fill).clip(true);
    let mut content = row![mark, label].spacing(space::S).align_y(Alignment::Center);
    if let Some(when) = choice.when {
        content = content.push(typo::text(typo::META.tabular(), when).style(theme::text_3).wrapping(Wrapping::None));
    }
    content = content.push(widgets::kbd(key));
    iced::widget::button(container(content).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(choice.message)
        .into()
}

/// A pick list in the suite's look for a property of the dialog of "Make task": `options` shown
/// by their names, `selected` in it, `on_pick` with the one picked. As high as a small button,
/// as wide as its row.
fn choice<'a, T>(options: Vec<T>, selected: Option<T>, on_pick: fn(T) -> Message) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_pick)
        .width(Fill)
        .padding(style::PICK_LIST_PADDING)
        .text_size(typo::CAPTION.size)
        .text_line_height(typo::CAPTION.line_height)
        .font(typo::CAPTION.font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into()
}

/// The failure when the entries could not be loaded; the error state's "Retry" is what the user
/// can do about it.
fn load_failed(err: &str) -> Failure {
    Failure::new(tr("Couldn't load the inbox."), err)
}

/// The error state of a first load: the alert icon, the sentence (`BODY_STRONG`, like the title of
/// the empty state), the reason quietly below it (`META`, `text_3`) and "Retry". The shared
/// `widgets::error_state` has one text only, so this is it with the reason as its own line.
fn failure_state(failure: &Failure) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let words = column![
        typo::text(typo::BODY_STRONG, failure.what).align_x(Alignment::Center),
        typo::text(typo::META, failure.why.as_str())
            .style(theme::text_3)
            .align_x(Alignment::Center)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::XS)
    .align_x(Alignment::Center);
    let content = column![
        alert,
        words,
        widgets::text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::secondary, Some(Message::Refresh)),
    ]
    .spacing(space::M)
    .align_x(Alignment::Center)
    .max_width(size::FORM_MAX / 2.0);
    iced::widget::center(content).padding(space::XL).into()
}

/// The line above the list when a later call failed: the alert icon in the error colour, the
/// sentence in `text_2` and the reason after it in `text_3` (status colour only on the icon).
fn failure_line(failure: &Failure) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let what = typo::text(typo::META, failure.what).style(theme::text_2).wrapping(Wrapping::None);
    let why = typo::text(typo::META, failure.why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
    row![alert, row![what, why].spacing(space::XS)].spacing(space::S).align_y(Alignment::Center).into()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use iced::futures::executor::block_on;
    use torro_inbox::demo::DemoSource;

    use super::*;
    use crate::empty::EmptySource;

    const WIDE: Size = Size::new(1600.0, 1000.0);
    const MEDIUM: Size = Size::new(800.0, 600.0);
    const NARROW: Size = Size::new(480.0, 900.0);
    const SIZES: [Size; 6] =
        [Size::new(240.0, 160.0), Size::new(320.0, 480.0), Size::new(1200.0, 200.0), NARROW, MEDIUM, WIDE];

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn demo(words: &[&str]) -> App<DemoSource> {
        let launch = Launch::parse(&args(words)).expect("valid parameters");
        let inbox = Arc::new(DemoSource::new(launch.start()));
        App::new(inbox, Source::from_args(args(&["--demo"])), launch).0
    }

    /// What the source answers for the app's view, handed to `update` as the task would.
    fn load<S: InboxSource + 'static>(app: &mut App<S>) {
        let empty = app.launch.open == Some(Open::Empty);
        let result = block_on(fetch(app.inbox.clone(), app.view.clone(), empty));
        let _ = app.update(Message::Loaded(app.view.clone(), result));
    }

    /// A demo app at the demo's fixed time, loaded.
    fn loaded(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T10:30"];
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    fn ids<S>(app: &App<S>) -> Vec<&str> {
        app.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn launch_parameters_need_the_demo_and_known_values() {
        assert_eq!(Launch::parse(&args(&["--open", "detail"])), Err(LaunchError::NeedsDemo("--open")));
        assert_eq!(Launch::parse(&args(&["--now", "2026-10-13T10:30"])), Err(LaunchError::NeedsDemo("--now")));
        assert!(matches!(
            Launch::parse(&args(&["--demo", "--open", "nope"])),
            Err(LaunchError::UnknownValue { flag: "--open", .. })
        ));
        assert_eq!(Launch::parse(&args(&["--demo", "--now", "today"])), Err(LaunchError::BadNow("today".into())));
        assert_eq!(Launch::parse(&args(&["--demo", "--open"])), Err(LaunchError::MissingValue("--open")));
        let launch = Launch::parse(&args(&["--demo", "--open=approval", "--appearance", "light"])).expect("valid");
        assert_eq!(
            (launch.demo, launch.open, launch.appearance),
            (true, Some(Open::Approval), Some(Appearance::Light))
        );
        assert!(!Launch::parse(&args(&[])).expect("valid").demo);
    }

    #[test]
    fn a_fixed_now_starts_the_demo_there() {
        let launch = Launch::parse(&args(&["--demo", "--now", "2026-10-13T10:30"])).expect("valid");
        let local = chrono::Local.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid");
        assert_eq!(launch.start(), local.to_utc());
    }

    #[test]
    fn lane_and_source_choose_the_start_view() {
        let view = |words: &[&str]| Launch::parse(&args(words)).map(|launch| launch.view);
        assert_eq!(view(&[]), Ok(None));
        assert_eq!(view(&["--lane", "for-me"]), Ok(Some(View::ForMe)));
        assert_eq!(view(&["--lane=info"]), Ok(Some(View::Info)));
        assert_eq!(view(&["--source", "suite"]), Ok(Some(View::Source(SourceRef::suite()))));
        assert_eq!(view(&["--source", "agent:claude-code"]), Ok(Some(View::Source(SourceRef::agent("claude-code")))));
        assert_eq!(view(&["--source=script:backup"]), Ok(Some(View::Source(SourceRef::script("backup")))));
        assert!(matches!(view(&["--lane", "later"]), Err(LaunchError::UnknownValue { flag: "--lane", .. })));
        assert!(matches!(view(&["--source", "mailbox"]), Err(LaunchError::UnknownValue { flag: "--source", .. })));
        assert_eq!(view(&["--lane"]), Err(LaunchError::MissingValue("--lane")));
        assert_eq!(view(&["--lane", "info", "--source", "suite"]), Err(LaunchError::Conflict("--lane", "--source")));
    }

    /// A workspace template stores the window's command line (`/proc/<pid>/cmdline`) and starts it
    /// again on load; the restored window must come back on the same slice.
    #[test]
    fn a_restored_command_line_starts_on_its_view() {
        let stored = args(&["/usr/bin/torroinbox", "--lane", "info"]);
        let launch = Launch::parse(&stored[1..]).expect("valid parameters");
        let app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        assert_eq!(app.view, View::Info);
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("Info"))]));

        let stored = args(&["/usr/bin/torroinbox", "--source", "agent:claude-code"]);
        let launch = Launch::parse(&stored[1..]).expect("valid parameters");
        let app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        assert_eq!(app.view, View::Source(SourceRef::agent("claude-code")));
    }

    #[test]
    fn a_demo_state_with_a_view_wins_over_the_lane() {
        assert_eq!(demo(&["--demo", "--open", "done", "--lane", "info"]).view, View::Done);
        assert_eq!(demo(&["--demo", "--open", "detail", "--lane", "info"]).view, View::Info);
    }

    #[test]
    fn views_ask_for_their_entries() {
        let open = Some(StateFilter::Open);
        assert_eq!(View::ForMe.filter(), ItemFilter { lane: Some(Lane::ForMe), state: open, ..ItemFilter::default() });
        assert_eq!(View::Info.filter(), ItemFilter { lane: Some(Lane::Info), state: open, ..ItemFilter::default() });
        assert_eq!(View::Snoozed.filter().state, Some(StateFilter::Snoozed));
        assert_eq!(View::Done.filter().state, Some(StateFilter::Done));
        let codex = View::Source(SourceRef::agent("codex")).filter();
        assert_eq!((codex.lane, codex.source, codex.state), (None, Some(SourceRef::agent("codex")), open));
        assert_eq!(View::ForMe.name(), tr("For me"));
        assert_eq!(View::Source(SourceRef::agent("claude-code")).name(), "Claude Code");
    }

    #[test]
    fn for_me_shows_urgent_entries_first_and_counts_them() {
        let app = loaded(&[]);
        assert_eq!(ids(&app), ["demo-1", "demo-3", "demo-2", "demo-4", "demo-6", "demo-5"]);
        assert_eq!(app.for_me.len(), 6);
        assert_eq!(app.count_note(), "6", "only the number: the title already says For me");
        let groups = list::groups(&app.items);
        assert_eq!(groups, [(list::Group::Waiting, 0..3), (list::Group::Other, 3..6)], "waiting entries get a head");
        assert_eq!(app.sources.len(), 7, "suite, two agents, four scripts");
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("For me"))]));
    }

    #[test]
    fn other_views_keep_the_for_me_count() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-8", "demo-9", "demo-7"]);
        assert_eq!(app.for_me.len(), 6, "the sidebar still counts For me");
        assert_eq!(app.count_note(), "3");
        let _ = app.update(Message::Show(View::Source(SourceRef::suite())));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-3", "demo-5"]);
    }

    #[test]
    fn the_glance_has_for_me_in_every_view_waiting_first() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        let for_me: Vec<&str> = app.for_me.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(for_me, ["demo-1", "demo-3", "demo-2", "demo-4", "demo-6", "demo-5"], "not Info's entries");
        assert_eq!(glance::waiting(&app.for_me), 3);
    }

    #[test]
    fn mini_and_strip_show_the_glance_without_keys_or_escape() {
        let mut app = loaded(&["--open", "detail"]);
        assert!(app.detail.is_some());
        let _ = app.update(Message::Resized(WIDE));
        let wide = app.subscription().units();
        for (size, glance) in
            SIZES.iter().map(|&size| (size, matches!(layout::of(size), SizeClass::Mini | SizeClass::Strip)))
        {
            let _ = app.update(Message::Resized(size));
            assert_eq!(app.is_glance(), glance, "{size:?}");
            let expected = if glance { wide - 2 } else { wide };
            assert_eq!(app.subscription().units(), expected, "no keys and no Escape in a glance: {size:?}");
            let _ = app.view();
        }
        assert!(app.detail.is_some(), "the panel waits for a larger window");
    }

    #[test]
    fn a_click_in_the_glance_opens_the_entry_in_for_me() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        let _ = app.update(Message::Resized(Size::new(240.0, 160.0)));
        assert!(app.update(Message::OpenForMe("demo-3".into())).units() > 0, "For me is loaded");
        assert_eq!(app.view, View::ForMe);
        assert_eq!(app.cursor.as_deref(), Some("demo-3"), "the glance highlights it");
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-3"));
        load(&mut app);
        assert_eq!(app.cursor.as_deref(), Some("demo-3"), "and keeps it once For me is in");
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-3"), "the panel shows it");
        // Already in For me: nothing to load; an entry no longer there does nothing.
        assert_eq!(app.update(Message::OpenForMe("demo-1".into())).units(), 0);
        assert_eq!(app.cursor.as_deref(), Some("demo-1"));
        let _ = app.update(Message::OpenForMe("demo-8".into()));
        assert_eq!(app.cursor.as_deref(), Some("demo-1"), "Info's entry is not in the glance");
    }

    #[test]
    fn an_answer_for_a_view_left_behind_is_dropped() {
        let mut app = loaded(&[]);
        let for_me = block_on(fetch(app.inbox.clone(), View::ForMe, false));
        let _ = app.update(Message::Show(View::Done));
        let _ = app.update(Message::Loaded(View::ForMe, for_me));
        assert_eq!(ids(&app).len(), 6, "still the old list, not taken for Done");
        load(&mut app);
        assert_eq!(ids(&app), ["demo-11"]);
    }

    #[test]
    fn selecting_opens_the_panel_and_a_view_closes_it() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-2".into()));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-2"));
        let _ = app.update(Message::Select("gone".into()));
        assert!(app.detail.is_none(), "an unknown id opens nothing");
        let _ = app.update(Message::Select("demo-2".into()));
        let _ = app.update(Message::Show(View::Info));
        assert!(app.detail.is_none());
    }

    #[test]
    fn escape_closes_menu_and_drawer_before_the_panel() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-1".into()));
        let _ = app.update(Message::ToggleNav);
        let _ = app.update(Message::Escape);
        assert!(app.menu.is_none() && app.detail.is_some(), "the menu first");
        let _ = app.update(Message::ShowDrawer);
        let _ = app.update(Message::Escape);
        assert!(!app.drawer && app.detail.is_some(), "then the drawer");
        let _ = app.update(Message::Escape);
        assert!(app.detail.is_none(), "then the panel");
        let _ = app.update(Message::Select("demo-1".into()));
        let _ = app.update(Message::CloseDetail);
        assert!(app.detail.is_none());
    }

    #[test]
    fn marking_done_shows_the_answer_then_the_panel_moves_on() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-6".into()));
        // What the task of SetDone does, run here.
        let changed = block_on(app.inbox.set_item_done("demo-6".into(), true)).map(Box::new).map_err(|e| e.to_string());
        let _ = app.update(Message::Changed(changed));
        assert!(
            matches!(app.detail.as_ref().map(|item| item.state), Some(State::Done { .. })),
            "the panel shows the answer at once"
        );
        load(&mut app);
        assert!(!ids(&app).contains(&"demo-6"));
        assert_eq!(app.for_me.len(), 5);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-5"), "the row in its place");
        assert_eq!(app.cursor.as_deref(), Some("demo-5"));
        let reopened =
            block_on(app.inbox.set_item_done("demo-6".into(), false)).map(Box::new).map_err(|e| e.to_string());
        let _ = app.update(Message::Changed(reopened));
        load(&mut app);
        assert!(ids(&app).contains(&"demo-6"));
    }

    #[test]
    fn a_failed_change_says_so() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Changed(Err("boom".into())));
        assert_eq!(app.error, Some(Failure { what: tr("Couldn't change the entry."), why: "boom".into() }));
        load(&mut app);
        assert_eq!(app.error, None, "the next load clears it");
    }

    #[test]
    fn launch_states_open_their_view_or_panel() {
        let app = loaded(&["--open", "detail"]);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-1"));
        let app = loaded(&["--open", "approval"]);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-3"));
        let app = loaded(&["--open", "snoozed"]);
        assert_eq!((app.view.clone(), ids(&app)), (View::Snoozed, vec!["demo-10"]));
        let app = loaded(&["--open", "empty"]);
        assert!(app.loaded && app.items.is_empty() && app.sources.is_empty());
        let mut app = demo(&["--demo", "--open", "loading"]);
        let _ = app.update(Message::Tick);
        assert!(!app.loaded, "loading stays");
        let mut app = demo(&["--demo", "--open", "error"]);
        let _ = app.update(Message::Loaded(View::ForMe, Err(DEMO_ERROR.into())));
        assert_eq!(app.error, Some(Failure { what: tr("Couldn't load the inbox."), why: DEMO_ERROR.into() }));
        assert_eq!(demo(&["--demo", "--open", "menu"]).menu, Some(Menu::Nav));
        assert!(demo(&["--demo", "--open", "drawer"]).drawer);
        // The triage states of E3.2.
        let app = loaded(&["--open", "snooze"]);
        assert_eq!((app.menu, app.cursor.as_deref()), (Some(Menu::Snooze), Some("demo-1")));
        assert_eq!(loaded(&["--open", "pick"]).menu, Some(Menu::Pick));
        let app = loaded(&["--open", "lane"]);
        assert_eq!((app.menu, app.cursor.as_deref()), (Some(Menu::Lane), Some("demo-1")));
        let claude = SourceRef::agent("claude-code");
        let failed = loaded(&["--open", "failed"]);
        assert!(failed.loaded && !failed.items.is_empty(), "the list is there");
        assert_eq!(failed.error, Some(Failure { what: tr("Couldn't change the entry."), why: DEMO_ERROR.into() }));
        let app = loaded(&["--open", "mute"]);
        let hint = app.hint.as_ref().expect("the hint after muting");
        assert_eq!(hint.text, trf("New entries from {source} muted", &[("source", &claude.label())]));
        assert_eq!(hint.undo, Some(Undo::Mute(claude, false)));
        for state in ["undo", "mute"] {
            let mut app = loaded(&["--open", state]);
            let shown = app.hint.as_ref().map(|hint| hint.shown).expect("the hint");
            let _ = app.update(Message::HintTick(shown + UNDO_FOR));
            assert!(app.hint.is_some(), "{state}: the forced hint stays");
        }
    }

    #[test]
    fn without_the_demo_the_inbox_is_empty() {
        let launch = Launch::default();
        let mut app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        load(&mut app);
        assert!(app.loaded && app.items.is_empty() && app.for_me.is_empty());
        assert_eq!(app.nav_entries().0[0].trail(), Trail::None, "no 0 beside For me");
        assert_eq!(list::empty_text(&app.view).0, tr("Nothing for you right now."));
        let _ = app.view();
    }

    #[test]
    fn the_sidebar_lists_views_then_sources() {
        let mut app = loaded(&[]);
        let (views, sources) = app.nav_entries();
        let labels: Vec<&str> = views.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, [tr("For me"), tr("Info"), tr("Snoozed"), tr("Done")]);
        assert_eq!(
            views.iter().map(NavEntry::trail).collect::<Vec<_>>()[..2],
            [Trail::Count(6), Trail::Count(3)],
            "For me and Info count their open entries"
        );
        let by_source: usize = sources.iter().filter_map(|e| e.count).sum();
        assert_eq!(by_source, 6 + 3, "the sources add up to For me plus Info");
        assert_eq!(sources.first().map(|e| e.label.clone()), Some(tr("Suite").to_owned()));
        assert_eq!(sources.first().map(NavEntry::trail), Some(Trail::Count(2)));
        block_on(app.inbox.mute_source(SourceRef::script("backup"), true)).expect("mute");
        load(&mut app);
        let (_, sources) = app.nav_entries();
        assert!(sources.iter().any(|e| e.label == trf("{source} (muted)", &[("source", &"backup")])));
    }

    #[test]
    fn the_sidebar_collapses_in_wide_and_opens_the_drawer_elsewhere() {
        let mut app = loaded(&[]);
        assert_eq!(app.sidebar, SidebarMode::Expanded, "expanded until ui.json says otherwise");
        let _ = app.update(Message::UiState(UiState { sidebar: SidebarMode::Rail }));
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!((app.sidebar, app.drawer), (SidebarMode::Expanded, false), "wide: the choice flips");
        for size in [MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ToggleSidebar);
            assert!(app.drawer, "{size:?}: the panel button opens the drawer");
            let _ = app.update(Message::ToggleSidebar);
            assert!(!app.drawer, "{size:?}: and closes it again");
            assert_eq!(app.sidebar, SidebarMode::Expanded, "{size:?}: the wide choice stays");
        }
    }

    #[test]
    fn menus_toggle_and_a_view_closes_them() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, Some(Menu::Nav));
        let _ = app.update(Message::ToggleNav);
        assert_eq!(app.menu, None, "a second click closes the menu");
        let _ = app.update(Message::Switcher(Switcher::Toggle));
        assert_eq!(app.menu, Some(Menu::Switcher));
        let _ = app.update(Message::ShowDrawer);
        assert!(app.drawer && app.menu.is_none(), "the drawer replaces an open menu");
        let _ = app.update(Message::Show(View::Done));
        assert!(!app.drawer && app.menu.is_none());
        assert_eq!(app.view, View::Done);
    }

    #[test]
    fn links_open_only_from_the_open_panel() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::OpenItem("demo-6".into()));
        assert!(app.detail.is_none() && app.sent.is_empty(), "nothing open, nothing happens");
        let _ = app.update(Message::Select("demo-1".into()));
        assert_eq!(app.open_item("demo-6").units(), 0, "not the open entry");
        assert!(app.sent.is_empty());
    }

    #[test]
    fn the_window_builds_in_every_size_with_panel_menus_and_drawer() {
        for open in [
            &[][..],
            &["--open", "detail"],
            &["--open", "approval"],
            &["--open", "done"],
            &["--open", "empty"],
            &["--open", "failed"],
        ] {
            let mut app = loaded(open);
            for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                let _ = app.update(Message::UiState(UiState { sidebar }));
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.view();
                    let _ = app.update(Message::ToggleNav);
                    let _ = app.view();
                    let _ = app.update(Message::ShowDrawer);
                    let _ = app.view();
                    let _ = app.update(Message::CloseMenu);
                    // The triage layers: snooze menu, picker, lane question, mute (a hint), hint.
                    let _ = press(&mut app, Key::Down);
                    for key in [Key::S, Key::Digit(4)] {
                        let _ = press(&mut app, key);
                        let _ = app.view();
                    }
                    let _ = app.update(Message::CloseMenu);
                    for key in [Key::L, Key::M] {
                        let _ = press(&mut app, key);
                        let _ = app.view();
                        let _ = app.update(Message::Escape);
                    }
                    app.note("A hint".into());
                    let _ = app.view();
                    app.hint = None;
                }
            }
        }
        let mut unloaded = demo(&["--demo", "--open", "loading"]);
        let _ = unloaded.view();
        let _ = unloaded.update(Message::Loaded(View::ForMe, Err("x".into())));
        let _ = unloaded.view();
    }

    #[test]
    fn theme_follows_appearance_and_desktop() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::SystemMode(Mode::Light));
        assert_eq!(app.theme(), theme::light(), "System follows the desktop");
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(app.theme(), theme::theme());
        let _ = app.update(Message::Appearance(Err("core is not running".into())));
        assert_eq!(app.theme(), theme::theme(), "a failed request keeps the appearance");
        let forced = loaded(&["--appearance", "light"]);
        assert_eq!(forced.theme(), theme::light());
    }

    // ----- Triage (E3.2) ---------------------------------------------------------------------

    fn press<S: InboxSource + 'static>(app: &mut App<S>, key: Key) -> Task<Message> {
        app.update(Message::Key(key))
    }

    /// Runs the last call the window started against the demo source, hands its answer to
    /// `update` as the task would, and loads the view again.
    fn deliver(app: &mut App<DemoSource>) {
        match app.sent.pop() {
            Some(Sent::Edit(edit, after)) => {
                let result = block_on(edit.apply(app.inbox.clone())).map(Box::new);
                let _ = app.update(Message::Edited(result, after));
            }
            Some(Sent::Mute(source, muted, with_hint)) => {
                let hint = with_hint.then(|| mute_hint(&source, muted));
                let result = block_on(app.inbox.mute_source(source, muted)).map_err(|err| err.to_string());
                let _ = app.update(Message::Muted(result, hint));
            }
            Some(Sent::Open(id)) => {
                let result = block_on(app.inbox.open_item(id)).map_err(|err| err.to_string());
                let _ = app.update(Message::ItemOpened(result));
            }
            other => panic!("no call to deliver: {other:?}"),
        }
        load(app);
    }

    fn cursor<S>(app: &App<S>) -> Option<&str> {
        app.cursor.as_deref()
    }

    /// The demo app with the cursor on `id`, through the keys.
    fn at(id: &str) -> App<DemoSource> {
        let mut app = loaded(&[]);
        while cursor(&app) != Some(id) {
            let before = app.cursor.clone();
            let _ = press(&mut app, Key::Down);
            assert_ne!(app.cursor, before, "{id} is in For me");
        }
        app
    }

    #[test]
    fn j_and_k_move_the_cursor_and_stop_at_the_ends() {
        let mut app = loaded(&[]);
        assert_eq!(cursor(&app), None);
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-1"), "down starts at the top");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-3"));
        let _ = press(&mut app, Key::Up);
        let _ = press(&mut app, Key::Up);
        assert_eq!(cursor(&app), Some("demo-1"), "stops at the first row");
        let mut app = loaded(&[]);
        let _ = press(&mut app, Key::Up);
        assert_eq!(cursor(&app), Some("demo-5"), "up starts at the bottom");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-5"), "stops at the last row");
        let mut empty = loaded(&["--open", "empty"]);
        let _ = press(&mut empty, Key::Down);
        assert_eq!(cursor(&empty), None);
    }

    #[test]
    fn moving_scrolls_the_cursor_into_view() {
        let mut app = loaded(&[]);
        // The group head and two rows fit.
        let row = list::row_height(app.two_line());
        let (_, second) = list::row_span(&app.items, 1, row).expect("a second row");
        let _ = app.update(Message::Scrolled(0.0, second));
        assert_eq!(press(&mut app, Key::Down).units(), 0, "the first row is in view");
        let _ = press(&mut app, Key::Down);
        assert!(press(&mut app, Key::Down).units() > 0, "the third row needs a scroll");
        let (_, third) = list::row_span(&app.items, 2, row).expect("a third row");
        assert_eq!(app.scroll, Some((third - second, second)));
        // Back up to the first row: the list scrolls to its group head at the top.
        let _ = press(&mut app, Key::Up);
        assert!(press(&mut app, Key::Up).units() > 0);
        assert_eq!(app.scroll, Some((0.0, second)));
    }

    #[test]
    fn the_open_panel_follows_the_cursor() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-3".into()));
        assert_eq!(cursor(&app), Some("demo-3"), "a click puts the cursor there");
        let _ = press(&mut app, Key::Down);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-2"));
        let _ = app.update(Message::CloseDetail);
        let _ = press(&mut app, Key::Down);
        assert!(app.detail.is_none(), "a closed panel stays closed");
        assert_eq!(cursor(&app), Some("demo-4"));
    }

    #[test]
    fn enter_opens_the_link_and_else_the_panel() {
        // The demo has no core: a page still opens in the browser from here.
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::Enter);
        assert_eq!(app.sent, [Sent::Link("https://ci.example.org/runs/1842".into())], "https in the browser");
        assert!(app.detail.is_none());
        // The agent's window goes to the source (`inbox.open_item`), in the demo too.
        let mut app = at("demo-1");
        let _ = press(&mut app, Key::Enter);
        assert_eq!(app.sent, [Sent::Open("demo-1".into())]);
        assert!(app.detail.is_none());
        // Nothing to open: the panel opens.
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::Enter);
        assert!(app.sent.is_empty());
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-3"));
    }

    /// Without `--demo` the core opens everything (E2.4): the agent's window, apps and pages alike
    /// go out as `inbox.open_item`, from `Enter` and from the panel's row.
    #[test]
    fn the_button_and_enter_send_open_item_to_the_core() {
        for id in ["demo-1", "demo-5", "demo-6"] {
            let mut app = at(id);
            app.launch.demo = false;
            let _ = press(&mut app, Key::Enter);
            assert_eq!(app.sent, [Sent::Open(id.into())], "Enter on {id}");
            let _ = app.update(Message::Select(id.into()));
            let _ = app.update(Message::OpenItem(id.into()));
            assert_eq!(app.sent, [Sent::Open(id.into()), Sent::Open(id.into())], "the panel's row on {id}");
        }
    }

    #[test]
    fn going_to_the_window_marks_the_entry_seen_and_a_failure_says_so() {
        let mut app = at("demo-1");
        let _ = press(&mut app, Key::Enter);
        deliver(&mut app);
        let item = app.items.iter().find(|item| item.id == "demo-1").expect("still open");
        assert_eq!(item.state, State::Seen);
        assert_eq!(app.error, None);
        let _ = app.update(Message::ItemOpened(Err(torro_inbox::InboxError::Unsupported.to_string())));
        let failure = Failure::new(tr("Couldn't open the entry."), torro_inbox::InboxError::Unsupported.to_string());
        assert_eq!(app.error, Some(failure));
    }

    #[test]
    fn e_marks_done_and_u_undoes_it() {
        let mut app = at("demo-1");
        let _ = press(&mut app, Key::E);
        let done = Edit::Done { id: "demo-1".into(), done: true };
        let reopen = Edit::Done { id: "demo-1".into(), done: false };
        let after = After { text: tr("Marked as done").into(), undo: Some(reopen.clone()) };
        assert_eq!(app.sent.last(), Some(&Sent::Edit(done, Some(after))));
        deliver(&mut app);
        assert!(!ids(&app).contains(&"demo-1"));
        assert_eq!(cursor(&app), Some("demo-3"), "on to the next row");
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Marked as done")));
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(reopen, None)));
        assert!(app.hint.is_none(), "the hint goes with the undo");
        deliver(&mut app);
        assert_eq!(ids(&app)[0], "demo-1", "back in its place");
        assert_eq!(cursor(&app), Some("demo-1"), "and the cursor with it");
        assert!(press(&mut app, Key::U).units() == 0 && app.sent.is_empty(), "nothing left to undo");
    }

    #[test]
    fn e_reopens_a_done_entry() {
        let mut app = loaded(&["--open", "done"]);
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::E);
        let reopen = Edit::Done { id: "demo-11".into(), done: false };
        assert!(matches!(app.sent.last(), Some(Sent::Edit(edit, _)) if *edit == reopen));
        deliver(&mut app);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Reopened")));
    }

    #[test]
    fn approvals_have_no_done() {
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::E);
        assert!(app.sent.is_empty(), "nothing is sent");
        let hint = app.hint.as_ref().expect("a hint says why");
        assert_eq!((hint.text.as_str(), &hint.undo), (tr("An approval closes once it is decided."), &None));
    }

    #[test]
    fn s_snoozes_from_the_menu_and_u_brings_it_back() {
        let mut app = at("demo-2");
        let _ = press(&mut app, Key::S);
        assert_eq!((app.menu, app.focus()), (Some(Menu::Snooze), Focus::Choices));
        let labels: Vec<String> = app.choices().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [tr("Later today"), tr("Tomorrow"), tr("Next week"), tr("Pick…")]);
        let [later, tomorrow, _] = snooze_times(app.now);
        let _ = press(&mut app, Key::E);
        assert!(app.sent.is_empty(), "e does not reach the list behind the menu");
        let _ = press(&mut app, Key::Digit(2));
        let snooze = Edit::Snooze { id: "demo-2".into(), until: Some(tomorrow) };
        let back = Edit::Snooze { id: "demo-2".into(), until: None };
        let text = trf("Snoozed until {time}", &[("time", &list::wake_text(tomorrow, app.now))]);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(snooze, Some(After { text, undo: Some(back.clone()) }))));
        assert!(app.menu.is_none(), "the menu closes");
        deliver(&mut app);
        assert!(!ids(&app).contains(&"demo-2"));
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(back, None)));
        deliver(&mut app);
        assert!(ids(&app).contains(&"demo-2"));
        // Later today is three hours on.
        assert_eq!(later - app.now, TimeDelta::hours(3));
    }

    #[test]
    fn snoozing_again_undoes_to_the_old_time() {
        let mut app = loaded(&["--open", "snoozed"]);
        let State::Snoozed { until: old } = app.items[0].state else { panic!("demo-10 is snoozed") };
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::S);
        assert_eq!(app.choices().last().map(|c| c.label.clone()), Some(tr("Unsnooze").to_owned()));
        let _ = press(&mut app, Key::Digit(1));
        let Some(Sent::Edit(_, Some(after))) = app.sent.last() else { panic!("an edit with a hint") };
        assert_eq!(after.undo, Some(Edit::Snooze { id: "demo-10".into(), until: Some(old) }));
        let _ = press(&mut app, Key::S);
        let _ = press(&mut app, Key::Digit(5));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Snooze { until: None, .. }, _))), "Unsnooze");
    }

    #[test]
    fn the_snooze_times_are_three_hours_tomorrow_and_monday_morning() {
        // Tuesday, 13 October 2026, 10:30 local time.
        let now = datetime::to_utc(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 13)
                .expect("valid")
                .and_time(NaiveTime::from_hms_opt(10, 30, 0).expect("valid")),
        );
        let local = |at: DateTime<Utc>| datetime::to_local(at).format("%a %d %H:%M").to_string();
        let [later, tomorrow, next_week] = snooze_times(now);
        assert_eq!([local(later), local(tomorrow), local(next_week)], ["Tue 13 13:30", "Wed 14 09:00", "Mon 19 09:00"]);
        // On a Monday, next week is the Monday after.
        let monday = now + TimeDelta::days(6);
        assert_eq!(local(snooze_times(monday)[2]), "Mon 26 09:00");
    }

    #[test]
    fn the_snooze_menu_says_when_each_choice_wakes() {
        let mut app = at("demo-2");
        let _ = press(&mut app, Key::S);
        let [later, tomorrow, next_week] = snooze_times(app.now);
        let whens: Vec<Option<String>> = app.choices().into_iter().map(|c| c.when).collect();
        let short = |at| Some(wake_short(at, app.now));
        assert_eq!(whens, [short(later), short(tomorrow), short(next_week), None], "Pick… has no time of its own");
        // Tuesday, 13 October 2026, 10:30: today only the time, other days with their weekday.
        assert_eq!(wake_short(later, app.now), "13:30");
        let wed = datetime::weekday_name(local_today(app.now) + Days::new(1), true);
        assert_eq!(wake_short(tomorrow, app.now), format!("{wed} 09:00"));
    }

    #[test]
    fn pick_steps_through_days_and_times() {
        let mut app = at("demo-2");
        let _ = press(&mut app, Key::S);
        let _ = press(&mut app, Key::Digit(4));
        assert_eq!((app.menu, app.focus()), (Some(Menu::Pick), Focus::Picker));
        let today = local_today(app.now);
        assert_eq!(app.pick, Pick { day: today + Days::new(1), hour: 0 }, "tomorrow morning");
        let _ = press(&mut app, Key::Right);
        let _ = press(&mut app, Key::Down);
        assert_eq!(app.pick, Pick { day: today + Days::new(2), hour: 1 });
        for _ in 0..PICK_HOURS.len() {
            let _ = press(&mut app, Key::Down);
        }
        assert_eq!(app.pick.hour, PICK_HOURS.len() - 1, "stops at the last time");
        for _ in 0..4 {
            let _ = press(&mut app, Key::Left);
        }
        assert_eq!(app.pick.day, today, "not before today");
        let _ = app.update(Message::PickHour(0));
        let _ = press(&mut app, Key::Enter);
        assert!(app.sent.is_empty(), "09:00 today has passed at 10:30");
        let _ = app.update(Message::PickHour(1));
        let _ = press(&mut app, Key::Enter);
        let until = Some(at_hour(today, PICK_HOURS[1]));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Snooze { until: u, .. }, _)) if *u == until));
        assert!(app.menu.is_none());
    }

    #[test]
    fn l_asks_always_for_this_source() {
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::L);
        assert_eq!((app.menu, app.focus()), (Some(Menu::Lane), Focus::Choices));
        let labels: Vec<String> = app.choices().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [tr("Only this entry").to_owned(), trf("Always for {source}", &[("source", &"deploy")])]);
        let _ = press(&mut app, Key::Digit(1));
        let only = Edit::Lane { id: "demo-6".into(), lane: Lane::Info, remember: false };
        let back = Edit::Lane { id: "demo-6".into(), lane: Lane::ForMe, remember: false };
        let text = trf("Moved to {lane}", &[("lane", &tr("Info"))]);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(only, Some(After { text, undo: Some(back.clone()) }))));
        deliver(&mut app);
        assert!(!ids(&app).contains(&"demo-6"), "gone to Info");
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(back, None)));
        deliver(&mut app);
        assert!(ids(&app).contains(&"demo-6"));
        // Always: a rule in the core, which undo cannot take back, so there is no undo.
        let _ = press(&mut app, Key::L);
        let _ = press(&mut app, Key::Digit(2));
        let Some(Sent::Edit(edit, Some(after))) = app.sent.last() else { panic!("an edit with a hint") };
        assert_eq!(edit, &Edit::Lane { id: "demo-6".into(), lane: Lane::Info, remember: true });
        assert_eq!(after.undo, None);
    }

    #[test]
    fn l_moves_suite_entries_without_a_rule() {
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::L);
        assert!(app.menu.is_none(), "no question: the suite gets no remembered lane");
        assert!(matches!(
            app.sent.last(),
            Some(Sent::Edit(Edit::Lane { lane: Lane::Info, remember: false, .. }, Some(_)))
        ));
    }

    #[test]
    fn m_mutes_at_once_and_u_unmutes() {
        let mut app = at("demo-6");
        let deploy = SourceRef::script("deploy");
        let _ = press(&mut app, Key::M);
        assert_eq!(app.sent.last(), Some(&Sent::Mute(deploy.clone(), true, true)), "no question first");
        assert_eq!(app.focus(), Focus::List, "nothing in front of the list");
        deliver(&mut app);
        assert!(app.is_muted(&deploy));
        let hint = app.hint.clone().expect("the hint");
        assert_eq!(hint.text, trf("New entries from {source} muted", &[("source", &"deploy")]));
        assert_eq!(hint.undo, Some(Undo::Mute(deploy.clone(), false)));
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Mute(deploy.clone(), false, false)), "undo unmutes");
        assert!(app.hint.is_none(), "the hint goes with the undo");
        deliver(&mut app);
        assert!(!app.is_muted(&deploy));
        assert!(app.hint.is_none(), "an undo shows no new hint");
        // Unmuting works the same way, with its own undo.
        let _ = press(&mut app, Key::M);
        deliver(&mut app);
        let _ = press(&mut app, Key::M);
        assert_eq!(app.sent.last(), Some(&Sent::Mute(deploy.clone(), false, true)));
        deliver(&mut app);
        assert_eq!(app.hint.and_then(|hint| hint.undo), Some(Undo::Mute(deploy, true)));
    }

    #[test]
    fn the_suite_is_never_muted() {
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::M);
        assert!(app.sent.is_empty());
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("Entries from the suite can't be muted."))
        );
    }

    #[test]
    fn keys_stay_out_of_other_menus_and_the_drawer() {
        let mut app = at("demo-1");
        let _ = app.update(Message::ToggleNav);
        for key in [Key::Down, Key::E, Key::S, Key::M, Key::Enter] {
            let _ = press(&mut app, key);
        }
        assert_eq!((cursor(&app), app.menu, app.sent.len()), (Some("demo-1"), Some(Menu::Nav), 0));
        let _ = app.update(Message::ShowDrawer);
        let _ = press(&mut app, Key::E);
        assert!(app.sent.is_empty());
        let _ = press(&mut app, Key::Sidebar);
        assert!(!app.drawer, "[ closes the drawer");
    }

    #[test]
    fn the_hint_goes_after_five_seconds_and_undo_with_it() {
        let mut app = at("demo-1");
        let _ = press(&mut app, Key::E);
        deliver(&mut app);
        let shown = app.hint.as_ref().map(|hint| hint.shown).expect("a hint");
        let _ = app.update(Message::HintTick(shown + Duration::from_secs(4)));
        assert!(app.hint.is_some());
        let _ = app.update(Message::HintTick(shown + UNDO_FOR));
        assert!(app.hint.is_none());
        let _ = press(&mut app, Key::U);
        assert!(app.sent.is_empty(), "too late to undo");
    }

    #[test]
    fn the_panel_footer_reaches_the_same_menus() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-6".into()));
        let _ = app.update(Message::AskSnooze("demo-6".into()));
        assert_eq!((app.menu, app.subject.as_ref().map(|i| i.id.as_str())), (Some(Menu::Snooze), Some("demo-6")));
        let _ = app.update(Message::AskLane("demo-6".into()));
        assert_eq!(app.menu, Some(Menu::Lane));
        let _ = app.update(Message::AskMute("demo-6".into()));
        assert!(app.menu.is_none(), "muting closes the menu");
        assert_eq!(app.sent.last(), Some(&Sent::Mute(SourceRef::script("deploy"), true, true)));
        let _ = app.update(Message::SetDone("demo-6".into(), true));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { done: true, .. }, Some(_)))), "with undo");
    }

    // ----- Make task (E3.5) ------------------------------------------------------------------

    /// The demo's task lists, handed to `update` as the task of the dialog would.
    fn deliver_lists(app: &mut App<DemoSource>) {
        assert_eq!(app.sent.last(), Some(&Sent::Lists), "the dialog asked for the lists");
        let lists = block_on(app.inbox.list_task_lists()).map_err(|err| err.to_string());
        let _ = app.update(Message::TaskLists(lists));
    }

    /// Runs the task the dialog sent against the demo source and hands its answer to `update`.
    fn deliver_task(app: &mut App<DemoSource>) {
        let Some(Sent::Task { id, list, title, due }) = app.sent.pop() else { panic!("no task sent") };
        let result = block_on(app.inbox.create_task_from_item(id, list.id, title, due)).map_err(|err| err.to_string());
        let _ = app.update(Message::TaskCreated(result, list.name));
        load(app);
    }

    fn demo_list(index: usize) -> TaskList {
        let lists = block_on(DemoSource::new(Utc::now()).list_task_lists()).expect("demo lists");
        lists[index].clone()
    }

    #[test]
    fn t_opens_the_dialog_and_create_sends_the_task_then_the_entry_is_done() {
        let mut app = at("demo-6");
        let title = app.find("demo-6").map(|item| item.title.clone()).expect("demo-6");
        let _ = press(&mut app, Key::T);
        assert_eq!((app.menu, app.subject.as_ref().map(|i| i.id.as_str())), (Some(Menu::Task), Some("demo-6")));
        assert_eq!(app.task.title, title, "the entry's title to change");
        assert_eq!(app.task.list, None, "no list before the lists are in");
        deliver_lists(&mut app);
        assert_eq!(app.task.list, Some(demo_list(0)), "the first list");

        let _ = app.update(Message::TaskTitle("Fix the deploy".into()));
        let _ = app.update(Message::TaskList(demo_list(1)));
        let _ = app.update(Message::TaskDue(Due::Tomorrow));
        // The keys of the list stay out while the dialog is open; Enter creates the task.
        let _ = press(&mut app, Key::E);
        assert_eq!(app.menu, Some(Menu::Task), "e is no triage key in the dialog");
        let _ = press(&mut app, Key::Enter);
        let tomorrow = local_today(app.now) + Days::new(1);
        assert_eq!(
            app.sent.last(),
            Some(&Sent::Task {
                id: "demo-6".into(),
                list: demo_list(1),
                title: Some("Fix the deploy".into()),
                due: Some(tomorrow),
            })
        );
        assert!(app.menu.is_none(), "the dialog closes");
        deliver_task(&mut app);
        let hint = app.hint.as_ref().expect("a hint");
        assert_eq!(hint.text, trf("Task created in {list}", &[("list", &demo_list(1).name)]));
        assert!(hint.undo.is_none(), "no undo: that would delete the task");
        assert!(!ids(&app).contains(&"demo-6"), "the entry is done and left For me");
    }

    #[test]
    fn the_panel_button_opens_the_same_dialog_and_remembers_the_list() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Select("demo-6".into()));
        let _ = app.update(Message::AskTask("demo-6".into()));
        assert_eq!(app.menu, Some(Menu::Task));
        deliver_lists(&mut app);
        let _ = app.update(Message::TaskList(demo_list(1)));
        let _ = app.update(Message::TaskConfirm);
        deliver_task(&mut app);

        let next = app.items.iter().find(|item| !detail::is_approval(item)).map(|item| item.id.clone());
        let _ = app.update(Message::AskTask(next.expect("another entry")));
        deliver_lists(&mut app);
        assert_eq!(app.task.list, Some(demo_list(1)), "the list used last");
    }

    #[test]
    fn create_needs_a_list_and_a_blank_title_is_left_to_the_core() {
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::T);
        let _ = app.update(Message::TaskConfirm);
        assert_eq!(app.sent.last(), Some(&Sent::Lists), "nothing sent while the lists load");
        let _ = app.update(Message::TaskLists(Ok(Vec::new())));
        let _ = app.update(Message::TaskConfirm);
        assert_eq!((app.menu, app.sent.last()), (Some(Menu::Task), Some(&Sent::Lists)), "no list, no task");
        let _ = app.update(Message::TaskLists(Ok(vec![demo_list(0)])));
        let _ = app.update(Message::TaskTitle("   ".into()));
        let _ = app.update(Message::TaskConfirm);
        assert!(matches!(app.sent.last(), Some(Sent::Task { title: None, due: None, .. })), "{:?}", app.sent.last());
    }

    #[test]
    fn an_approval_becomes_no_task() {
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::T);
        assert!(app.menu.is_none());
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("An approval closes once it is decided."))
        );
        assert!(app.sent.is_empty(), "the lists are not even asked for");
    }

    #[test]
    fn a_task_that_could_not_be_made_says_so_and_escape_closes_the_dialog() {
        let mut app = at("demo-6");
        let _ = app.update(Message::TaskCreated(Err("The server did not answer".into()), "Work".into()));
        assert_eq!(app.error.as_ref().map(|failure| failure.what), Some(tr("Couldn't create the task.")));
        assert!(app.hint.is_none());

        let _ = press(&mut app, Key::T);
        let _ = app.update(Message::TaskLists(Err("offline".into())));
        assert!(matches!(app.lists, Some(Err(_))));
        let _ = app.update(Message::Escape);
        assert!(app.menu.is_none() && app.subject.is_none());
        // Lists that arrive after the dialog closed change nothing.
        let _ = app.update(Message::TaskLists(Ok(vec![demo_list(0)])));
        assert_eq!(app.task.list, None);
    }

    #[test]
    fn the_due_days_count_like_the_snooze_menu() {
        let now = Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time");
        let today = local_today(now);
        assert_eq!(Due::None.day(now), None);
        assert_eq!(Due::Today.day(now), Some(today));
        assert_eq!(Due::Tomorrow.day(now), Some(today + Days::new(1)));
        let next_week = Due::NextWeek.day(now).expect("a day");
        assert_eq!(datetime::to_local(snooze_times(now)[2]).date(), next_week, "Monday, as \"Next week\" snoozes");
        assert_eq!(Due::NextWeek.to_string(), tr("Next week"));
    }

    #[test]
    fn the_task_state_opens_the_dialog_at_the_panel_and_every_size_draws_it() {
        let mut app = loaded(&["--open", "task"]);
        assert_eq!(app.menu, Some(Menu::Task));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), app.subject.as_ref().map(|i| i.id.as_str()));
        for lists in [None, Some(Ok(Vec::new())), Some(Err("offline".to_string())), Some(Ok(vec![demo_list(0)]))] {
            app.lists = lists;
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
    }
}
