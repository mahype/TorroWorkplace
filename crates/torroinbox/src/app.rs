//! The inbox window (tasks E3.1 and E3.2 in `docs/tasks/eingang.md`): state, loading through
//! [`InboxSource`], and the Linear window around it – `torro_gui::shell` with the sidebar
//! ("Overview" and "For me" with their count, "Info", "Snoozed", "Done", the group "Sources") in the wide layout, the
//! rail of icons in the medium one (and in the wide one when the user collapses it: panel button
//! or `[`, remembered in `ui.json`), a navigation menu ("For me ▾") plus drawer in the medium and
//! narrow ones. The list (`list.rs`), the overview (`overview.rs`) and the panel (`detail.rs`) only
//! render and report clicks as [`Message`]s.
//!
//! Three windows (task E8.1, [`Kind`], `--view mail|notifications|hub`): each with its own window
//! class, title and entry in the app switcher. They keep different entries ([`Kind::keeps`]: mail,
//! everything else, all) and draw their overviews in their own way (below). Each has "Compact |
//! Comfortable" in its header bar (task E8.2, [`App::density`]), remembered per window and slice in
//! `ui.json` ([`Launch::density_key`]), `--density` winning.
//!
//! The mail window (task E8.3, `docs/eingang.md` "Drei Fenster") holds mail only ([`Kind::keeps`]).
//! Its sidebar lists For me (the overview's one column: the mail for the user by day, "Info"
//! folded at the end), Info, Later and Done ([`Kind::view_name`]), the group "Smart views" with
//! "With attachments" ([`View::Attachments`]) and the folders of every account; no sources. In the
//! comfortable density mail shows as cards with their buttons always shown ([`App::look`],
//! `list::Look`); in the wide layout an open mail stands in the reader beside the list
//! ([`App::wide_reader`], `mail::reader`) instead of in the panel. The narrow layout's menu lists no
//! folders (the drawer still does), and its comfortable "For me" starts with how many mails are new
//! for the user ([`App::lead`]). Nothing is dismissed in the mail window: mail is done, snoozed or
//! hidden.
//!
//! The notifications window (task E8.4, `notifications.rs`) shows what the apps report: its sidebar
//! lists New (its overview, both lanes), Later and Done, and under "This window shows" the sources
//! to switch on and off ([`App::shown`]: `--source`, repeated, at start; chips over the board where
//! the sidebar is folded away). Its views are a board ([`App::is_board`]): a column per app or a grid
//! "By time" in the wide layout ([`Arrange`]), one column with "For me" and "Info" in narrow ones;
//! cards in the comfortable density, rows with their action bar in the compact one. Every
//! notification has the ✕ (`x`, [`Edit::Dismiss`]: done, sent as dismissing) but approvals, and the
//! header bar "Dismiss all" with an undo ([`Undo::Dismissed`]); `m` and the bell open the bell's
//! menu (below).
//!
//! The bell's menu (task E8.6b, [`Menu::Mute`], `docs/eingang.md` "Stummschalten nach Merkmalen"):
//! under "Mute" the ways the core offers for the entry (`inbox.list_mute_options`: this
//! conversation, then its traits – the sender, the project, the client …), under "For" how long
//! ([`MuteFor`]: 1 hour, until tomorrow, 1 week, always) and under "What happens" what muting does (no
//! popup, only Info, hide), and at the end "All rules", which opens the control centre. Picking a
//! way sends `inbox.mute_item` with the "For" and "What happens" chosen; the toast says what holds
//! now in the words of the control centre's rules ([`muted_note`]) and its undo takes it back
//! exactly ([`Undo::Muted`]). "For" and "What happens" stay as chosen while the window runs. The
//! panel's bell opens the same menu.
//!
//! Mail has one menu to mute (issue #123, [`App::mutes_by_bell`]): its "Hide from Notification Hub" went into
//! the bell's menu, in every window. Rows and cards of mail show the bell "Mute…" where "…" was, the
//! reader and the panel too, and `m` opens it: "This conversation" with "Hide" and "Always" is what
//! "This conversation" was (done, the mail stays unread), sender and domain are traits, "Always to
//! Info" is the sender with "Only Info", and "Move to junk" stays as the menu's last way. A folder's
//! message without an entry has the bell as well ([`App::ask_mute_message`]): `inbox.mute_item` with
//! the message's id, the core offering its traits only.
//!
//! The hub (task E8.5, `hub.rs`) is about acting: its overview is "Act now" ([`App::is_hub`]) – one
//! tile per origin over the sections Decide (approvals), Agents and To sort, each entry with its
//! buttons always shown, and in the wide layout "Coming back" at the right (the snoozed entries,
//! [`Loaded::snoozed`]). A click on a tile filters the sections ([`App::filter`]). Its sidebar lists
//! Act now, To sort (the section alone, [`App::sorting`]), Later, Done today and the sources. An
//! approval has neither ✕ nor Later anywhere: `s` on it only says that it waits for a decision.
//!
//! Insights (task E8.8b, `insights.rs`): the hub's last sidebar entry shows what the inbox counted
//! (`inbox.get_stats`) instead of "Act now", on the overview's entries, which keep the sidebar's
//! numbers ([`App::insights`]). Every load asks for the numbers again, so a suggestion muted from
//! the page goes away; its button is the bell menu's `inbox.mute_item` with the window's "For" and
//! "What happens", with the same toast and undo. The keys of entries do nothing there.
//!
//! Task with a way back (task E8.7b, `docs/eingang.md` "Aufgabe mit Rückbezug"): a mail shows the
//! task made of it as a card over its text ([`App::task_of`]: its entry's [`Item::task`], or the one
//! "Task" made of a folder's message, `tasks.create_todo_from_mail`, [`App::ask_mail_task`]); "Open
//! task" opens it in the tasks app through the core ([`open_target`]). The hub asks after a finished
//! task (`hub::Section::FollowUp`: Reply, "Mark mail done" – [`App::mark_mail_done`] – and "Not
//! now") and lists the open tasks made of entries under "Handed off" ([`Loaded::done`]).
//! `--message=m:…` starts the mail window on that mail ([`App::open_start_mail`]). With `--demo`
//! the entries get the demo's tasks ([`demo_task`]).
//!
//! The mail window starts on the overview (task E3.9, [`View::Overview`]): its open mail in the
//! column "Mail" (the overview's other column, "Notifications", was the hub's until E8.5). The app
//! keeps which column has the
//! cursor ([`App::column`]) and which "Info" is unfolded (in `ui.json`, task E3.11); the keys walk the
//! column
//! ([`App::rows`]). Every row has an action bar (Task, Done, Later, Hide; for mail first Archive and
//! Delete, issue #118, a folder's message too), whose buttons do what the keys do on the row
//! ([`Message::Act`], [`Message::Mail`]). Reply is no button of rows and cards: it stands in the
//! reader, where the mail has been read.
//!
//! HTML mail (task M9.7, decision 9 in `docs/mail.md`): a mail that came with HTML
//! (`mail::shows_html`) shows as HTML right in the panel and the reader ([`App::html`], drawn by
//! `torro_htmlview`). Two switches over it – "HTML | Text" (`v`) and "Light | Dark" for the mail's
//! content only – start from the settings `mail_view` and `mail_appearance` and hold until those
//! change ([`App::take_look`]); `app` follows the window's appearance. Links in the HTML open in
//! the browser ([`Message::HtmlLink`]).
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
//! asking "always for this source?" ([`Edit::Lane`]), and hide it from the inbox (`m` or the
//! panel's "…", task E3.10, [`Menu::Hide`]): "This entry" (done) and muting or unmuting the source
//! at once (E3.2c: no dialog – a change with an undo needs no question as well); mail has the
//! bell's menu instead (above). Every change shows a toast for
//! [`UNDO_FOR`] (`widgets::toast`, task E3.11) with "Undo" (`u`, and Ctrl + Z / ⌘ + Z in the whole
//! window), which sends the change that puts it back ([`Undo`]); Escape takes the toast away. Muting
//! is undone exactly: a rule the call made is removed and the entries it closed or moved come back
//! ([`Undo::Muted`]). A change that takes the entry off the
//! list moves the cursor – and the panel – to the row that took its place, so `e`, `e`, `e` works
//! through a list.
//!
//! Make task (E3.5): `t` or the panel's "Make task" opens a small dialog at the entry – the title
//! (the entry's, to change), the task list (from [`InboxSource::list_task_lists`]) and an optional
//! due day (Today, Tomorrow, Next week, as the snooze menu counts them). "Create task" or `Enter`
//! sends `inbox.create_task_from_item`: the core makes the task, links it back to the entry and
//! marks the entry done; the hint says in which list. There is no undo: taking the task back
//! would delete it, and deleting is for the tasks app.
//!
//! AI task and AI event (issue #120, `suggest.rs`): beside "Make task" and "Make event" in the panel
//! and after "Task" in the reader, off with a tooltip until an AI is set up ([`App::ai_ready`], asked
//! with every tick). They open the same dialogs at once, with a quiet line while the AI is asked
//! ([`Suggesting`]); its suggestion fills in title, due day and a note, or day, start, length, place
//! and who takes part. Nothing is saved before "Create"; the note and place go along
//! (`inbox.create_task_from_item` / `inbox.create_event_from_item`).
//!
//! Make event (task E8.5b): `c`, "Event" in the hub's To sort and the panel's "Make event" open the
//! same kind of dialog ([`App::ask_event`]) – the title, the calendar (from
//! [`HubSource::list_calendars`]), the day, the start (every half hour, the next one first) and the
//! duration. "Create event" or `Enter` sends `inbox.create_event_from_item`; the core makes the event
//! with the way back in its text and marks the entry done. No undo either, as for tasks. The hub
//! also asks whether it is quiet (`inbox.get_quiet`, [`Loaded::quiet`]) with its entries.
//!
//! Mail (task E4.2, `mail.rs`): the sidebar lists the folders of the mail accounts with their
//! unread numbers – under "Mail" with one account, under each account's address with two or more
//! (foldable, as the calendars group by account). A folder is a view of its own
//! ([`View::Folder`]): its messages in the same list, a page at a time ("Load more"), with a search
//! field above them. The panel shows the whole mail (`mail.get_message`), for a folder's message
//! and for a mail entry of "For me" or "Info" alike, with Reply, Reply all, Forward, Archive,
//! Delete, Flag and Mark unread. Opening a folder's message marks it read, as mail apps do; an
//! entry is read once it is done. Archive (`e` in a folder) and Delete (`#`) move the message and
//! offer an undo that moves it back. Reply (`r`), Reply all (`a`) and Forward (`f`) start the
//! compose window (task E4.3) as its own process. `--source mail` makes a window of mail only: its
//! sidebar holds the folders alone and it starts on the first account's inbox ([`View::Mail`]
//! until the folders are in).
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
use iced::widget::{Space, column, container, pick_list, row, rule, stack, text_editor, text_input};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{MailAppearance, MailFormat};
use torro_core::shortcuts;
use torro_gui::connect;
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::selectable::Selectable;
use torro_gui::shell::{self, Cover, SidebarMode, Surface, Switcher, TitleButtons};
use torro_gui::source::Source;
use torro_gui::split::{self, Limits, Split, SplitEvent};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, Density, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, ToastAction, Trail};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{datetime, style, typo};
use torro_htmlview::{Content, Scheme};
use torro_inbox::item::{FacetKey, MuteEffect, MuteOption, MuteScope, Muted};
use torro_inbox::source::TaskList;
use torro_inbox::{
    InboxError, InboxSource, Item, ItemFilter, Lane, Link, Order, Rule, SourceInfo, SourceKind, SourceRef, State,
    StateFilter, rules,
};
use torro_ipc::inbox::{CalendarEvent, EventFields};
use torro_ipc::mail::{Delivery, MoveRole, MoveTarget};
use torro_ipc::mail::{MailHighlight, MailSort};
use torro_ipc::{EventSuggestion, TaskSuggestion};
use torro_mail::{Flag, Folder, InvitationAnswer, Message as Mail, MessageSummary, Role};

use crate::compose;
use crate::glance::{self, GlanceData};
use crate::hub::{self, HubSource, Origin};
use crate::insights::{self, Insights, InsightsData, StatsSource};
use crate::keys::{self, Command, Focus, Key};
use crate::list::Look;
use crate::mail::{self, Compose, FolderView, MailAction, MailEdit, MailLook, MailPage, MailSource, Opened, Reading};
use crate::notifications::{self, Arrange, Board, Shown};
use crate::overview::{self, Column, Sections};
use crate::search::{self, Found, Scope, ScopeChoice, SearchView, Sort, Suggestion};
use crate::suggest::{self as ai, SuggestSource};
use crate::{detail, list};

/// The app's identity: window class `torro-inbox`, binary `torroinbox`, state in `torroinbox/`.
pub const INBOX: AppId = AppId::INBOX;

/// The name `ui.json` keeps for the sidebar's "Sources" while it is unfolded (issue #121).
const SOURCES_KEY: &str = "sidebar.sources";

/// The start of the names `ui.json` keeps for the mail accounts unfolded in the sidebar
/// ([`account_key`], issue #121).
const ACCOUNT_KEY: &str = "sidebar.mail.";

/// The name `ui.json` keeps while the folders of `account` are unfolded: `sidebar.mail.<account>`.
fn account_key(account: &str) -> String {
    format!("{ACCOUNT_KEY}{account}")
}

/// How often the window asks the source again. Short, because a waiting agent should show up
/// within seconds; the source is local and cheap to ask, and nothing moves on screen unless an
/// entry or a time in the list changed.
const REFRESH: Duration = Duration::from_secs(10);

/// How long the toast after a change shows, and with it "Undo" (`u`, Ctrl + Z): the suite's
/// [`widgets::TOAST_FOR`].
pub const UNDO_FOR: Duration = widgets::TOAST_FOR;

/// How often a shown hint checks whether its time is up. Only while a hint shows.
const HINT_TICK: Duration = Duration::from_secs(1);

/// "In 5 min" puts a reminder's entry off for this many minutes (task ER1.4).
const REMINDER_SNOOZE_MINUTES: i64 = 5;

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

/// The id of the title field in the dialog of "Make event" (task E8.5b).
const EVENT_TITLE: &str = "event-title";

/// The start times the dialog of "Make event" offers: every half hour of the day.
const EVENT_STEP_MINUTES: u32 = 30;

/// When an event made of an entry starts (task E8.5b): a time of the day in local time, shown as
/// "09:30" in the dialog's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StartTime(pub NaiveTime);

impl StartTime {
    /// Every half hour of a day, from midnight.
    pub fn all() -> Vec<StartTime> {
        (0..24 * 60 / EVENT_STEP_MINUTES)
            .filter_map(|step| NaiveTime::from_num_seconds_from_midnight_opt(step * EVENT_STEP_MINUTES * 60, 0))
            .map(StartTime)
            .collect()
    }
}

impl StartTime {
    /// What the dialog offers: [`StartTime::all`], with `self` in its place when it is between two
    /// of them (an AI's 10:15, issue #120).
    pub fn offered(self) -> Vec<StartTime> {
        let mut all = StartTime::all();
        if !all.contains(&self) {
            all.push(self);
            all.sort();
        }
        all
    }
}

impl fmt::Display for StartTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&datetime::format_time(self.0))
    }
}

/// How long an event made of an entry takes (task E8.5b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Length {
    Quarter,
    #[default]
    Half,
    Hour,
    TwoHours,
    /// Any other length in minutes, as an AI suggested it (issue #120): offered beside the others
    /// while the dialog holds it.
    Minutes(i64),
}

impl Length {
    /// In the order the dialog lists them.
    pub const ALL: [Length; 4] = [Length::Quarter, Length::Half, Length::Hour, Length::TwoHours];

    /// The length in minutes.
    pub fn minutes(self) -> i64 {
        match self {
            Length::Quarter => 15,
            Length::Half => 30,
            Length::Hour => 60,
            Length::TwoHours => 120,
            Length::Minutes(minutes) => minutes,
        }
    }

    /// The length of `minutes`: one of [`Length::ALL`] when it is one, else [`Length::Minutes`].
    pub fn of_minutes(minutes: i64) -> Length {
        Length::ALL.into_iter().find(|length| length.minutes() == minutes).unwrap_or(Length::Minutes(minutes))
    }

    /// What the dialog offers: [`Length::ALL`], and `self` after them when it is none of them.
    pub fn offered(self) -> Vec<Length> {
        let mut all = Length::ALL.to_vec();
        if !all.contains(&self) {
            all.push(self);
        }
        all
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Length::Quarter => tr("15 minutes"),
            Length::Half => tr("30 minutes"),
            Length::Hour => tr("1 hour"),
            Length::TwoHours => tr("2 hours"),
            Length::Minutes(minutes) => return f.write_str(&trf("{n} minutes", &[("n", &minutes)])),
        })
    }
}

/// What the dialog of "Make event" holds while it is open, for the entry in `App::subject` (task
/// E8.5b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    /// The event's title; starts as the entry's. Left blank, the core takes the entry's.
    pub title: String,
    /// The calendar chosen; `None` until the calendars are in (then the last one used, else the
    /// first).
    pub calendar: Option<TaskList>,
    /// The local day it starts on.
    pub day: NaiveDate,
    /// The local time it starts at.
    pub start: StartTime,
    pub length: Length,
    /// Where it takes place (issue #120): what an AI suggested, to change; empty for nowhere.
    pub location: String,
    /// A note put before the entry's text (issue #120): who takes part, as an AI found them.
    pub notes: String,
}

impl EventDraft {
    /// A draft for an entry titled `title`, seen from `now`: today (or tomorrow late at night) at
    /// the next half hour, half an hour long.
    pub fn new(title: String, now: DateTime<Utc>) -> EventDraft {
        let local = datetime::to_local(now);
        let step = i64::from(EVENT_STEP_MINUTES) * 60;
        let seconds = i64::from(chrono::Timelike::num_seconds_from_midnight(&local.time()));
        let next = local.date().and_time(NaiveTime::MIN) + TimeDelta::seconds((seconds / step + 1) * step);
        EventDraft {
            title,
            calendar: None,
            day: next.date(),
            start: StartTime(next.time()),
            length: Length::Half,
            location: String::new(),
            notes: String::new(),
        }
    }

    /// When the event starts and ends, as instants.
    pub fn span(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        let start = datetime::to_utc(self.day.and_time(self.start.0));
        (start, start + TimeDelta::minutes(self.length.minutes()))
    }
}

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
    /// Another day, as an AI suggested it (issue #120): offered beside the others while the dialog
    /// holds it.
    On(NaiveDate),
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
            Due::On(day) => Some(day),
        }
    }

    /// The due date of `day`, seen from `now`: one of [`Due::ALL`] when it names that day, else
    /// [`Due::On`].
    pub fn of_day(day: NaiveDate, now: DateTime<Utc>) -> Due {
        Due::ALL.into_iter().find(|due| due.day(now) == Some(day)).unwrap_or(Due::On(day))
    }

    /// What the dialog offers: [`Due::ALL`], and `self` after them when it is none of them.
    pub fn offered(self) -> Vec<Due> {
        let mut all = Due::ALL.to_vec();
        if !all.contains(&self) {
            all.push(self);
        }
        all
    }
}

impl fmt::Display for Due {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Due::None => tr("No due date"),
            Due::Today => tr("Today"),
            Due::Tomorrow => tr("Tomorrow"),
            Due::NextWeek => tr("Next week"),
            Due::On(day) => {
                return f.write_str(&datetime::format_short_date(*day, datetime::to_local(Utc::now()).date()));
            }
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
    /// A note put before the entry's text (issue #120): what an AI suggested, to change; empty for
    /// none.
    pub notes: String,
}

/// What the AI does for the open dialog of "Make task" or "Make event" (issue #120, "AI task" and
/// "AI event"): the dialog shows it in a quiet line under the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggesting {
    /// Asked for the entry with this id; the dialog fills in once the answer is in.
    Asking(String),
    /// The dialog holds the AI's suggestion: to check before creating.
    Done,
    /// No suggestion, and why; the dialog stays as it opened.
    Failed(String),
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
    /// Dismiss a notification (task E8.4, the ✕, `x`): done like [`Edit::Done`], but sent as
    /// `inbox.set_item_done` with `dismissed: true` ([`InboxSource::dismiss_item`]), so the
    /// statistics tell it apart. Undone by opening the entry again.
    Dismiss { id: String },
}

impl Edit {
    /// The entry the edit changes.
    pub fn id(&self) -> &str {
        match self {
            Edit::Done { id, .. } | Edit::Snooze { id, .. } | Edit::Lane { id, .. } | Edit::Dismiss { id } => id,
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
            Edit::Dismiss { .. } => Edit::Done { id, done: false },
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
    /// "Done", "Later until 18:30", "Moved to Info" … – the words of the buttons (task E8.1b). A
    /// remembered lane has no undo.
    pub fn after(&self, before: &Item, now: DateTime<Utc>) -> After {
        let text = match self {
            Edit::Done { done: true, .. } => tr("Done").to_owned(),
            Edit::Done { done: false, .. } => tr("Reopened").to_owned(),
            Edit::Dismiss { .. } => tr("Dismissed").to_owned(),
            Edit::Snooze { until: Some(until), .. } => {
                trf("Later until {time}", &[("time", &list::wake_text(*until, now))])
            }
            Edit::Snooze { until: None, .. } => tr("Brought back").to_owned(),
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
            Edit::Dismiss { id } => inbox.dismiss_item(id).await,
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
    /// Moves archived or deleted mail back where it was (`mail.move_messages`), and opens the
    /// entry `reopen` again – the core closed it when its mail left the inbox.
    Mail { edit: MailEdit, reopen: Option<String> },
    /// Takes "Dismiss all" back (task E8.4), and "Mark mail done" (task E8.7b): opens the entries it
    /// closed again (`inbox.set_item_done` with `false`, one by one).
    Dismissed(Vec<String>),
    /// Takes muting back (task E8.6b), exactly as `inbox.mute_item` answered ([`Muted`]): removes
    /// the rule `rule` – only one the call made –, opens the entries `reopen` again and moves the
    /// entries `moved` back to "For me" (`inbox.set_item_lane` without `remember`).
    Muted { rule: Option<String>, reopen: Vec<String>, moved: Vec<String> },
}

impl Undo {
    /// The undo of muting that answered `muted`; `None` when it changed nothing (no new rule, no
    /// entry closed or moved – the same rule was there already, or "No popup" for good).
    pub fn of_muted(muted: &Muted) -> Option<Undo> {
        let rule = muted.rule.as_ref().filter(|_| muted.new_rule).map(|rule| rule.id.clone());
        let changed = rule.is_some() || !muted.done.is_empty() || !muted.moved.is_empty();
        changed.then(|| Undo::Muted { rule, reopen: muted.done.clone(), moved: muted.moved.clone() })
    }
}

/// The hint at the bottom of the window: what just happened, with "Undo" when it can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub text: String,
    pub undo: Option<Undo>,
    /// When it appeared; it goes [`UNDO_FOR`] later.
    pub shown: Instant,
}

/// A message archived or deleted in this window (issue #205), kept out of what loads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Leaving {
    /// The message's id (`m:…`).
    id: String,
    /// The core moved it: once a load no longer holds it or its entry, it is forgotten.
    answered: bool,
}

/// Whether `item` is the entry of the message `id`.
fn shows_mail(item: &Item, id: &str) -> bool {
    matches!(&item.link, Some(Link::Mail { message }) if message == id)
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

/// The icon of a lane, as in the sidebar, and of "Move to …". "Info" has the circled "i" – for
/// your information (task E8.10) –, never a bell: the bells are for notifications and muting.
pub fn lane_icon(lane: Lane) -> Icon {
    match lane {
        Lane::ForMe => Icon::Inbox,
        Lane::Info => Icon::Info,
    }
}

/// The lane an entry in `lane` moves to.
pub fn other_lane(lane: Lane) -> Lane {
    match lane {
        Lane::ForMe => Lane::Info,
        Lane::Info => Lane::ForMe,
    }
}

/// How long the bell's menu mutes (task E8.6b, "For"; "1 week" since issue #123): window state, kept
/// while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MuteFor {
    /// One hour from now.
    OneHour,
    /// Until tomorrow morning (9:00 local time, as "Tomorrow" of the snooze menu): the rest of today.
    UntilTomorrow,
    /// A week from now (issue #123).
    OneWeek,
    /// For good: no end.
    #[default]
    Always,
}

impl MuteFor {
    /// In the order of the segments, shortest first.
    pub const ALL: [MuteFor; 4] = [MuteFor::OneHour, MuteFor::UntilTomorrow, MuteFor::OneWeek, MuteFor::Always];

    /// The word of the segment.
    pub fn name(self) -> &'static str {
        match self {
            MuteFor::OneHour => tr("1 hour"),
            MuteFor::UntilTomorrow => tr("Until tomorrow"),
            MuteFor::OneWeek => tr("1 week"),
            MuteFor::Always => tr("Always"),
        }
    }

    /// The end of muting seen from `now` (`inbox.mute_item`'s `until`); `None` for good.
    pub fn until(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            MuteFor::OneHour => Some(now + TimeDelta::hours(1)),
            MuteFor::UntilTomorrow => Some(at_hour(local_today(now) + Days::new(1), MORNING)),
            MuteFor::OneWeek => Some(now + TimeDelta::days(7)),
            MuteFor::Always => None,
        }
    }
}

/// What muting does, in the order of the segments of "What happens": mildest first.
pub const MUTE_EFFECTS: [MuteEffect; 3] = [MuteEffect::NoPopup, MuteEffect::Info, MuteEffect::Hide];

/// The word of a segment of "What happens" (task E8.6b).
pub fn effect_name(effect: MuteEffect) -> &'static str {
    match effect {
        MuteEffect::NoPopup => tr("No popup"),
        MuteEffect::Info => tr("Only Info"),
        MuteEffect::Hide => tr("Hide"),
    }
}

/// The neighbour of `current` in `all`, one step to the end (`forward`) or the start, stopping at
/// both ends: what the arrows do in the bell's menu.
fn step_in<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let at = all.iter().position(|item| *item == current).unwrap_or_default();
    let next = if forward { (at + 1).min(all.len().saturating_sub(1)) } else { at.saturating_sub(1) };
    all.get(next).copied().unwrap_or(current)
}

/// The icon of a way to mute in the bell's menu: the conversation, the person, the domain, the
/// account, the list, newsletters, the agent, its project, what it reports, the client.
pub fn mute_icon(scope: MuteScope) -> Icon {
    match scope {
        MuteScope::Conversation | MuteScope::Facet { key: FacetKey::Conversation, .. } => Icon::ListTree,
        MuteScope::Facet { key, .. } => match key {
            FacetKey::Sender => Icon::User,
            FacetKey::Domain => Icon::Users,
            FacetKey::Account => Icon::Inbox,
            FacetKey::List => Icon::List,
            FacetKey::Bulk => Icon::Mail,
            FacetKey::Agent => Icon::Sparkles,
            FacetKey::Project => Icon::FolderGit2,
            FacetKey::Kind => Icon::CircleDot,
            FacetKey::Client => Icon::AppWindow,
            FacetKey::Conversation => Icon::ListTree,
            FacetKey::Repo => Icon::FolderGit2,
            FacetKey::Reason | FacetKey::Result => Icon::CircleDot,
            FacetKey::App => Icon::Cloud,
            FacetKey::Topic => Icon::RadioTower,
            FacetKey::Person => Icon::User,
            FacetKey::Priority => Icon::SignalHigh,
            FacetKey::Tag => Icon::Tag,
        },
    }
}

/// When a muting ends, as short as it gets seen from `now`, in local time: "15:30" today,
/// "tomorrow 09:00", else "Fri, 16 Oct 09:00". The control centre's rules say it the same way.
pub fn until_text(until: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (at, today) = (datetime::to_local(until), local_today(now));
    let time = datetime::format_time(at.time());
    if at.date() == today {
        time
    } else if at.date() == today + Days::new(1) {
        trf("tomorrow {time}", &[("time", &time)])
    } else {
        trf("{day} {time}", &[("day", &datetime::format_short_date(at.date(), today)), ("time", &time)])
    }
}

/// What a muting rule does, in the words of the control centre's rules (task E8.6b): its effect,
/// its end and what it mutes – "No popups until tomorrow 09:00: The project torro", "Hidden: Mail
/// from news@shop.example", "Only Info: Everything from backup". Values are shown as they are
/// (foreign text, an Iced `text` interprets nothing). A rule without a trait names its source.
pub fn mute_text(rule: &Rule, now: DateTime<Utc>) -> String {
    let what = rule.facet.as_ref().map_or_else(|| rule.source.label(), rules::facet_label);
    // A lane "for me" mutes nothing; `inbox.mute_item` never keeps one, so it only names the lane.
    let Some(effect) = MuteEffect::of(rule.effect) else {
        return trf("Notices of {source} go to “{lane}”", &[("source", &what), ("lane", &lane_name(Lane::ForMe))]);
    };
    let Some(until) = rule.until else {
        return match effect {
            MuteEffect::Hide => trf("Hidden: {what}", &[("what", &what)]),
            MuteEffect::Info => trf("Only Info: {what}", &[("what", &what)]),
            MuteEffect::NoPopup => trf("No popups: {what}", &[("what", &what)]),
        };
    };
    let pairs: [(&str, &dyn fmt::Display); 2] = [("what", &what), ("when", &until_text(until, now))];
    match effect {
        MuteEffect::Hide => trf("Hidden until {when}: {what}", &pairs),
        MuteEffect::Info => trf("Only Info until {when}: {what}", &pairs),
        MuteEffect::NoPopup => trf("No popups until {when}: {what}", &pairs),
    }
}

/// The text of the toast after muting with `effect` (task E8.6b): the rule in the control centre's
/// words ([`mute_text`]), or – where no rule was kept, the conversation hidden or moved for good –
/// what became of the entry: "Conversation hidden", "Moved to Info".
pub fn muted_note(muted: &Muted, effect: MuteEffect, now: DateTime<Utc>) -> String {
    match (&muted.rule, effect) {
        (Some(rule), _) => mute_text(rule, now),
        (None, MuteEffect::Info) => trf("Moved to {lane}", &[("lane", &lane_name(Lane::Info))]),
        (None, MuteEffect::Hide | MuteEffect::NoPopup) => tr("Conversation hidden").to_owned(),
    }
}

/// Whether the window offers to mute `source` or to remember a lane for it: not for the suite,
/// whose approvals and account problems must never be silenced by a rule.
pub fn rules_allowed(source: &SourceRef) -> bool {
    source.kind != SourceKind::Suite
}

/// One entry of a menu of choices (snooze, lane, hide): picked by click or by its digit.
#[derive(Debug, Clone)]
pub struct Choice {
    pub icon: Icon,
    pub label: String,
    /// When the choice snoozes until ([`wake_short`]: "13:30", "Wed 09:00"), shown quietly before
    /// the digit; `None` for choices without a fixed time.
    pub when: Option<String>,
    pub message: Message,
    /// A line above it: it starts another kind of choices (the hide menu's lane and junk after
    /// the ways to hide).
    pub line: bool,
}

impl Choice {
    fn new(icon: Icon, label: impl Into<String>, message: Message) -> Choice {
        Choice { icon, label: label.into(), when: None, message, line: false }
    }
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
    /// `approvals.decide_approval`: the approval, allowed or not.
    Decide(String, bool),
    /// The task lists were asked for (the dialog of "Make task" opened).
    Lists,
    /// `inbox.create_task_from_item`: entry, list, title (`None`: the entry's), due day and the note
    /// (issue #120; `None` for none).
    Task {
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
        notes: Option<String>,
    },
    /// `inbox.suggest_task_from_item` (issue #120) for the entry with this id.
    SuggestTask(String),
    /// `inbox.suggest_event_from_item` (issue #120) for the entry with this id.
    SuggestEvent(String),
    /// A change of mail (`mail.set_flags`, `mail.move_messages`, `mail.delete_messages`).
    Mail(MailEdit, Option<Hint>),
    /// `mail.get_message` for the message with this id.
    Letter(String),
    /// The compose window, started with these arguments ([`mail::compose_args`]).
    Compose(Vec<String>),
    /// `mail.get_attachment`: message and part, to open it.
    Attachment(String, String),
    /// The next page of the folder, from this cursor.
    More(String),
    /// "Dismiss all" (task E8.4): `inbox.set_item_done` with `dismissed: true` for these entries.
    DismissAll(Vec<String>),
    /// Its undo: these entries opened again.
    Reopen(Vec<String>),
    /// `mail.respond_to_invitation` (task E8.3b): account, message, answer and the calendar chosen
    /// (task E8.3c; `None`: the core's choice).
    Answer(String, String, InvitationAnswer, Option<String>),
    /// `calendar.list_events` around an invitation (task E8.3c): the message and the span asked.
    Events(String, DateTime<Utc>, DateTime<Utc>),
    /// `tasks.create_todo_from_mail` (task E8.7b): account, message, list, title (`None`: the
    /// subject) and due day (task E8.1b).
    MailTask {
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    },
    /// `apps.open_app` with a target (task E8.7b): the app and `task:<uid>` or `mail:<…>`.
    Target(String, String),
    /// "Mark mail done" (task E8.7b): `inbox.set_item_done` for these entries – the mail's, then the
    /// question's.
    MailDone(Vec<String>),
    /// The calendars were asked for (the dialog of "Make event" opened, task E8.5b).
    Calendars,
    /// `inbox.create_event_from_item`: entry, calendar, title (`None`: the entry's), start and end,
    /// place and note (issue #120; `None` for none).
    Event {
        id: String,
        calendar: TaskList,
        title: Option<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        location: Option<String>,
        notes: Option<String>,
    },
    /// `inbox.list_mute_options` for the entry with this id (the bell's menu opened, task E8.6b).
    MuteOptions(String),
    /// `inbox.mute_item`: entry, scope, end and effect.
    MuteItem(String, MuteScope, Option<DateTime<Utc>>, MuteEffect),
    /// The undo of muting: the rule removed, if any, the entries opened again and moved back.
    Unmute(Option<String>, Vec<String>, Vec<String>),
    /// `accounts.sync` (task E10.1, the shortcut "fetch").
    Fetch,
}

/// What the list shows. Each view is one [`ItemFilter`] ([`View::filter`]), so the app asks the
/// source exactly what the API offers (`inbox.list_items`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Every open entry in two columns, mail and the rest (task E3.9); the start view.
    Overview,
    /// Open entries in the lane "For me"; with the overview the only view with a count.
    ForMe,
    /// Open entries in the lane "Info".
    Info,
    /// Snoozed entries, both lanes.
    Snoozed,
    /// Done entries, both lanes.
    Done,
    /// Open entries of one source, both lanes.
    Source(SourceRef),
    /// The messages of a mail folder, searched or whole (task E4.2): no entries, `mail.*` only.
    Folder(FolderView),
    /// A window of mail only (`--source mail`) before its folders are in: it becomes the first
    /// account's inbox, or says that there is no mail account.
    Mail,
    /// The smart view "With attachments" of the mail window (task E8.3): the messages with
    /// attachments of every account's inbox (`mail::attachments`), `mail.*` only.
    Attachments,
    /// The smart view "Invitations" of the mail window (task E8.3b): the messages of every account's
    /// inbox that invite to an event not over yet, the soonest first (`mail::invitations`).
    Invitations,
    /// A search of mail (issue #136, the field over the list): the messages found, a page at a
    /// time, by relevance or by date; `mail.*` only.
    Search(SearchView),
}

impl View {
    /// The filter for `inbox.list_items` that selects the entries of this view; `None` for the
    /// views of mail, which list messages instead of entries.
    pub fn filter(&self) -> Option<ItemFilter> {
        let (lane, source, state) = match self {
            View::Overview => (None, None, StateFilter::Open),
            View::ForMe => (Some(Lane::ForMe), None, StateFilter::Open),
            View::Info => (Some(Lane::Info), None, StateFilter::Open),
            View::Snoozed => (None, None, StateFilter::Snoozed),
            View::Done => (None, None, StateFilter::Done),
            View::Source(source) => (None, Some(source.clone()), StateFilter::Open),
            View::Folder(_) | View::Mail | View::Attachments | View::Invitations | View::Search(_) => return None,
        };
        // "Done" shows what was done for real: a dismissed entry is simply gone (issue #117), its
        // toast's undo the only way back.
        let dismissed = (state == StateFilter::Done).then_some(false);
        Some(ItemFilter { lane, source, state: Some(state), dismissed, ..ItemFilter::default() })
    }

    /// The name in the sidebar, the header and the panel's path.
    pub fn name(&self) -> String {
        match self {
            View::Overview => tr("Overview").into(),
            View::ForMe => tr("For me").into(),
            View::Info => tr("Info").into(),
            View::Snoozed => tr("Later").into(),
            View::Done => tr("Done").into(),
            View::Source(source) => source.label(),
            View::Folder(folder) => folder.name.clone(),
            View::Mail => tr("Mail").into(),
            View::Attachments => tr("With attachments").into(),
            View::Invitations => tr("Invitations").into(),
            View::Search(_) => tr("Search").into(),
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            View::Overview => Icon::Layers,
            View::ForMe => Icon::Inbox,
            View::Info => lane_icon(Lane::Info),
            View::Snoozed => Icon::Clock,
            View::Done => Icon::CircleCheck,
            View::Source(source) => list::source_icon(source),
            View::Folder(folder) => mail::folder_icon(folder.role),
            View::Mail => Icon::Mail,
            View::Attachments => Icon::Paperclip,
            View::Invitations => Icon::Calendar,
            View::Search(_) => Icon::Search,
        }
    }

    /// The folder this view shows, if it is one.
    pub fn folder(&self) -> Option<&FolderView> {
        match self {
            View::Folder(folder) => Some(folder),
            _ => None,
        }
    }

    /// A view of mail: a folder, a smart view, a search, or a window of mail waiting for its
    /// folders.
    pub fn is_mail(&self) -> bool {
        matches!(self, View::Folder(_) | View::Mail | View::Attachments | View::Invitations | View::Search(_))
    }

    /// The search this view shows, if it is one.
    pub fn search(&self) -> Option<&SearchView> {
        match self {
            View::Search(search) => Some(search),
            _ => None,
        }
    }

    /// What the messages held belong to, for a view that lists messages: the folder's id, or
    /// "attachments" for the smart view. Another view's messages are not shown as this one's.
    pub fn listing(&self) -> Option<String> {
        match self {
            View::Folder(folder) => Some(folder.id.clone()),
            View::Attachments => Some("attachments".to_owned()),
            View::Invitations => Some("invitations".to_owned()),
            // One listing for every search: the results before stay while the next ones come, so
            // typing does not flash "loading" (issue #136).
            View::Search(_) => Some("search".to_owned()),
            _ => None,
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
    /// The dialog that makes an event of `App::subject` (task E8.5b).
    Event,
    /// "Hide from Notification Hub" for `App::subject` (task E3.10): the ways to hide it, by `m` or the
    /// panel's "…" ([`App::hide_choices`]).
    Hide,
    /// The bell's menu for `App::subject` (task E8.6b): the ways to mute it from
    /// `inbox.list_mute_options`, "For" and "What happens"; the bell and `m` of the notifications
    /// window ([`App::ask_mute_menu`]).
    Mute,
    /// "Move to folder" for [`App::mail_subject`] (task E10.1, the shortcut "move_to_folder"): the
    /// folders of its account but the one it lies in, picked by click or digit ([`App::move_choices`]).
    Move,
    /// The question "Delete permanently?" for [`App::mail_subject`] (task E10.1): a dialog over the
    /// window ([`App::question_layer`]), `Enter` or "Delete" removes the message for good.
    DeleteForGood,
}

/// Which buttons a menu for an entry hangs at (issue #122): the button clicked, else where the
/// keys act ([`App::menu_from_panel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MenuAt {
    /// Opened by a key (or a start state): at the panel's or reader's button while it is open, else
    /// at the cursor row's.
    #[default]
    Keys,
    /// Opened by a button of a row or card in the list (`Message::Act`): at that button.
    Row,
    /// Opened by a button of the panel or the reader (the `Ask…` messages): at that button.
    Panel,
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
    /// The panel of the first entry of To sort, with the dialog of "Make event" open at its footer
    /// (task E8.5b).
    Event,
    /// The first account's inbox as a folder (task E4.2).
    Folder,
    /// That inbox with its newest message in the panel.
    Mail,
    /// A search of all mail for "budget" from the first account's inbox (issue #136), with its
    /// results highlighted and the scopes of that folder.
    Search,
    /// "For me" with its first mail entry in the panel, the whole mail loaded.
    MailEntry,
    /// The menu of `m` of the first mail entry of "For me", at its row (task E3.10): the bell's menu
    /// since issue #123.
    Hide,
    /// That entry in the panel, with the menu at the panel's bell.
    HidePanel,
    /// The first account's inbox with the bell's menu of its first message that has no entry, at its
    /// row (issue #123): the message's own traits.
    MuteMail,
    /// The overview (task E3.9) with the cursor on the first row of "Mail", so its action bar
    /// shows.
    Overview,
    /// The overview with "Info" unfolded in both columns.
    OverviewInfo,
    /// The overview with the cursor on the first card of "Notifications": the column the medium
    /// and narrow layouts show then.
    OverviewNotices,
    /// The smart view "With attachments" of the mail window (task E8.3).
    Attachments,
    /// The smart view "Invitations" of the mail window (task E8.3b).
    Invitations,
    /// "Invitations" with its first invitation open (in the reader of the wide window, else the
    /// panel), so its card with Accept · Maybe · Decline shows.
    Invitation,
    /// The notifications window arranged "By time" (task E8.4), where it has room for columns.
    ByTime,
    /// The hub's To sort alone (task E8.5), as its sidebar entry shows it.
    ToSort,
    /// The hub's "Act now" filtered to its first tile (task E8.5).
    Tile,
    /// The bell's menu (task E8.6b) of the first entry that has a bell, at its card or row – in the
    /// notifications window (`--view notifications`).
    MuteMenu,
    /// That entry in the panel, with the menu at the panel's bell.
    MutePanel,
    /// The hub's Insights (task E8.8b) with its charts.
    Insights,
    /// Insights with "Table view" on.
    InsightsTable,
    /// The panel of the first entry that is no approval, with the dialog of "Make task" that "AI
    /// task" opens, filled in by the demo's AI (issue #120).
    AiTask,
    /// That entry with the dialog of "Make event" that "AI event" opens (issue #120).
    AiEvent,
    /// That entry in the panel without an AI: the AI buttons are off (issue #120).
    NoAi,
    /// The first account's inbox with its newest message in the reader and "Move to folder" open for
    /// it (task E10.1).
    MoveMail,
    /// That message with the question "Delete permanently?" over the window (task E10.1).
    DeleteMail,
}

impl Open {
    const ALL: [(&'static str, Open); 44] = [
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
        ("event", Open::Event),
        ("folder", Open::Folder),
        ("mail", Open::Mail),
        ("mail-entry", Open::MailEntry),
        ("search", Open::Search),
        ("hide", Open::Hide),
        ("hide-panel", Open::HidePanel),
        ("mute-mail", Open::MuteMail),
        ("overview", Open::Overview),
        ("overview-info", Open::OverviewInfo),
        ("overview-notices", Open::OverviewNotices),
        ("attachments", Open::Attachments),
        ("invitations", Open::Invitations),
        ("invitation", Open::Invitation),
        ("by-time", Open::ByTime),
        ("to-sort", Open::ToSort),
        ("tile", Open::Tile),
        ("mute-menu", Open::MuteMenu),
        ("mute-panel", Open::MutePanel),
        ("insights", Open::Insights),
        ("insights-table", Open::InsightsTable),
        ("ai-task", Open::AiTask),
        ("ai-event", Open::AiEvent),
        ("no-ai", Open::NoAi),
        ("move-mail", Open::MoveMail),
        ("delete-mail", Open::DeleteMail),
    ];
}

/// The start parameters of the window:
///
/// - `--demo`: sample data ([`torro_inbox::demo::DemoSource`]) instead of the core.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
/// - `--mail-view html|text` and `--mail-colors app|light|dark`: show mail this way in this process
///   instead of the settings `mail_view` and `mail_appearance` (task M9.7, [`Launch::look`]), as the
///   switches over a mail start. Nothing is stored.
/// - `--design <id>`: draw with this design pack instead of the standard (task L.3); needs `--demo`.
///   An id that is not a valid pack is refused with the valid ones.
/// - `--now <YYYY-MM-DDTHH:MM>` (local time): a fixed "now" and demo data around it, for pictures
///   that can be made again; needs `--demo`.
/// - `--view mail|notifications|hub`: which of the three windows this is (task E8.1, [`Kind`]);
///   without it the hub. Works without `--demo`; the core starts each kind with it
///   ([`torro_core::AppId::args`]) and a workspace template stores it with the command line.
/// - `--density compact|comfortable`: the density the window starts in (task E8.2, [`Density`]).
///   It wins over the density remembered for the window in `ui.json`, so a workspace template that
///   stores it brings the window back as it was; a switch in the header bar changes it (and is
///   remembered) as usual. Works without `--demo`.
/// - `--lane for-me|info` or `--source <key>` (`suite`, `agent:claude-code`, `script:backup`): start
///   on that lane or source, for one window per slice of the inbox (E3.6), inside the window's
///   kind. Works without `--demo`;
///   a workspace template stores the window's command line, so the slice comes back with it.
///   `--source mail` is a window of mail only (task E4.2): the folders of the mail accounts, starting
///   on the first account's inbox ([`View::Mail`]). In the notifications window (task E8.4)
///   `--source` instead chooses the sources the window shows ([`Launch::shown`]), and may be given
///   more than once (`--source script:ci --source script:deploy`); mail is refused there.
/// - `--compose` (with `--reply <id>`, `--reply-all <id>` or `--forward <id>`, and `--account <id>`)
///   is the compose window of task E4.3 ([`compose`]), which the panel's Reply, Reply all and
///   Forward start ([`mail::compose_command`]) and the "New" button starts without an id. It is a
///   window of its own and shows no inbox; its `--open` states are [`compose::Open`]'s.
///   `--reply`, `--reply-all`, `--forward` and `--account` without `--compose` are refused
///   ([`LaunchError::NeedsCompose`]), so a mistyped command never opens the inbox instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub demo: bool,
    pub open: Option<Open>,
    pub appearance: Option<Appearance>,
    /// `--mail-view`: HTML or text instead of the setting `mail_view`.
    pub mail_view: Option<MailFormat>,
    /// `--mail-colors`: the mail's colours instead of the setting `mail_appearance`.
    pub mail_colors: Option<MailAppearance>,
    pub now: Option<NaiveDateTime>,
    pub design: Option<String>,
    /// The window of `--view`; the hub without it.
    pub kind: Kind,
    /// The density of `--density`; `None` takes the one remembered for the window (task E8.2).
    pub density: Option<Density>,
    /// The view of `--lane` or `--source`; `None` starts on the overview.
    pub view: Option<View>,
    /// The sources the window shows (task E8.4): in the notifications window those of `--source`
    /// ([`Shown::from_args`]: the apps without one), everything in the others.
    pub shown: Shown,
    /// `--compose`: the window writes a message instead of showing the inbox.
    pub compose: Option<compose::Start>,
    /// The mail the window starts on (task E8.7b): `--message=m:…`, read and checked by `main`
    /// (`start_message`), as the core passes it for `apps.open_app` with a `mail:` target. The
    /// window shows it once its first entries are in ([`App::open_start_mail`]).
    pub message: Option<String>,
}

/// Which of the inbox's three windows this process is (task E8.1, `--view`; `docs/eingang.md`,
/// "Drei Fenster"). One program, three entries of [`torro_core::AppId::ALL`]: each kind has its own
/// window class, title and entry in the app switcher, so tiling rules and workspace templates tell
/// them apart. All three show the same entries of the core – what one marks done is done in all.
///
/// The mail window shows the overview's "Mail" column (task E8.3), the notifications window the
/// other entries on its board (task E8.4, `notifications.rs`), the hub "Act now" with all of them
/// (task E8.5, `hub.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    /// `--view mail`: what came in by mail.
    Mail,
    /// `--view notifications`: what the apps report, everything but mail.
    Notifications,
    /// `--view hub`, or no `--view`: what to decide or hand on.
    #[default]
    Hub,
}

impl Kind {
    /// The values of `--view`, in the order the error message lists them.
    pub const ALL: [(&'static str, Kind); 3] =
        [("mail", Kind::Mail), ("notifications", Kind::Notifications), ("hub", Kind::Hub)];

    /// The word of `--view` for this kind (`mail`, `notifications`, `hub`).
    pub fn word(self) -> &'static str {
        match self {
            Kind::Mail => "mail",
            Kind::Notifications => "notifications",
            Kind::Hub => "hub",
        }
    }

    /// The app entry of this kind: its window class, name and start arguments.
    pub fn app(self) -> AppId {
        match self {
            Kind::Mail => AppId::INBOX_MAIL,
            Kind::Notifications => AppId::INBOX_NOTIFICATIONS,
            Kind::Hub => AppId::INBOX,
        }
    }

    /// The overview's columns of the entries this window keeps, left to right; never empty. The hub
    /// keeps both, though its "Act now" sorts them into sections of its own (`hub.rs`).
    pub fn columns(self) -> &'static [Column] {
        match self {
            Kind::Mail => &[Column::Mail],
            Kind::Notifications => &[Column::Notices],
            Kind::Hub => &Column::ALL,
        }
    }

    /// `column` when this window shows it, else the window's first: where the overview's cursor may
    /// stand. An entry of the other column (opened from "For me", say) never moves it off the window.
    pub fn column_or_first(self, column: Column) -> Column {
        let columns = self.columns();
        if columns.contains(&column) { column } else { columns.first().copied().unwrap_or_default() }
    }

    /// Whether this window shows `item` (task E8.3): the entry stands in one of its columns – the
    /// mail window shows mail only, the notifications window everything else, the hub all.
    pub fn keeps(self, item: &Item) -> bool {
        self.columns().contains(&Column::of(item))
    }

    /// The name of `view` in this window: in the mail window (task E8.3) the overview is "For me" –
    /// its one column is the mail for the user –, in the notifications window (task E8.4) "New", in
    /// the hub (task E8.5) "Act now", and the hub's "Done" is "Done today" (it lists what was done
    /// today, `App::take`); in all three "Snoozed" is "Later", as the buttons say; elsewhere the
    /// view's own name ([`View::name`]).
    pub fn view_name(self, view: &View) -> String {
        match (self, view) {
            (Kind::Mail, View::Overview) => tr("For me").into(),
            (Kind::Notifications, View::Overview) => tr("New").into(),
            (Kind::Hub, View::Overview) => tr("Act now").into(),
            (Kind::Hub, View::Done) => tr("Done today").into(),
            (_, View::Snoozed) => tr("Later").into(),
            _ => view.name(),
        }
    }

    /// The icon of `view` in this window: "For me" of the mail window is `UserCheck` – never the
    /// inbox tray, which is the mail folder "Inbox" there and so must not stand for the window's first
    /// view (issue #127) –, "New" of the notifications window the bell.
    pub fn view_icon(self, view: &View) -> Icon {
        match (self, view) {
            (Kind::Mail, View::Overview) => Icon::UserCheck,
            (Kind::Notifications, View::Overview) => Icon::Bell,
            _ => view.icon(),
        }
    }

    /// The window title for `view`: the kind's name, then the view's ("Mail · For me",
    /// [`Kind::view_name`]).
    pub fn title(self, view: &View) -> String {
        let name = self.view_name(view);
        let pairs: [(&str, &dyn fmt::Display); 1] = [("view", &name)];
        match self {
            Kind::Mail => trf("Mail · {view}", &pairs),
            Kind::Notifications => trf("App Feed · {view}", &pairs),
            Kind::Hub => trf("Notification Hub · {view}", &pairs),
        }
    }

    /// The kind of `--view`; the hub without it.
    fn parse(args: &[String]) -> Result<Kind, LaunchError> {
        let Some(word) = flag_value(args, "--view")? else { return Ok(Kind::Hub) };
        match Kind::ALL.iter().find(|(name, _)| *name == word) {
            Some(&(_, kind)) => Ok(kind),
            None => Err(LaunchError::UnknownValue {
                flag: "--view",
                value: word.to_owned(),
                valid: Kind::ALL.map(|(name, _)| name).join(", "),
            }),
        }
    }
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
    /// `--reply`, `--reply-all`, `--forward` or `--account` without `--compose`.
    NeedsCompose(&'static str),
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
            LaunchError::NeedsCompose(flag) => write!(f, "{flag} only works together with --compose"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Every value of `--flag value` or `--flag=value`, in order; empty when the flag is not there.
fn flag_values<'a>(args: &'a [String], flag: &'static str) -> Result<Vec<&'a str>, LaunchError> {
    let mut values = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            values.push(words.next().map(String::as_str).ok_or(LaunchError::MissingValue(flag))?);
        } else if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            values.push(value);
        }
    }
    Ok(values)
}

/// The value of `--flag value` or `--flag=value`; `None` when the flag is not there.
pub(crate) fn flag_value<'a>(args: &'a [String], flag: &'static str) -> Result<Option<&'a str>, LaunchError> {
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
        let compose = compose::Start::parse(args, demo)?;
        // The compose window reads `--open` itself, with states of its own.
        let open = match flag_value(args, "--open")? {
            None => None,
            Some(_) if compose.is_some() => None,
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
        let mail_view = match flag_value(args, "--mail-view")? {
            None => None,
            Some("html") => Some(MailFormat::Html),
            Some("text") => Some(MailFormat::Text),
            Some(other) => return Err(unknown("--mail-view", other, vec!["html", "text"])),
        };
        let mail_colors = match flag_value(args, "--mail-colors")? {
            None => None,
            Some("app") => Some(MailAppearance::App),
            Some("light") => Some(MailAppearance::Light),
            Some("dark") => Some(MailAppearance::Dark),
            Some(other) => return Err(unknown("--mail-colors", other, vec!["app", "light", "dark"])),
        };
        let now = match flag_value(args, "--now")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--now")),
            Some(word) => Some(
                NaiveDateTime::parse_from_str(word, "%Y-%m-%dT%H:%M").map_err(|_| LaunchError::BadNow(word.into()))?,
            ),
        };
        let design = match flag_value(args, "--design")? {
            None => None,
            Some(_) if !demo => return Err(LaunchError::NeedsDemo("--design")),
            Some(id) if design::is_choice(id) => Some(id.to_owned()),
            Some(id) => {
                let valid = design::choices();
                return Err(unknown("--design", id, valid.iter().map(String::as_str).collect()));
            }
        };
        let kind = Kind::parse(args)?;
        let density = match flag_value(args, "--density")? {
            None => None,
            Some(word) => match Density::parse(word) {
                Some(density) => Some(density),
                None => return Err(unknown("--density", word, Density::ALL.map(|(name, _)| name).to_vec())),
            },
        };
        let (view, shown) = start_view(args, kind)?;
        Ok(Launch {
            demo,
            open,
            appearance,
            mail_view,
            mail_colors,
            now,
            design,
            kind,
            density,
            view,
            shown,
            compose,
            message: None,
        })
    }

    /// The mail settings `stored` with `--mail-view` and `--mail-colors` over them (task M9.7).
    pub fn look(&self, stored: MailLook) -> MailLook {
        MailLook { format: self.mail_view.unwrap_or(stored.format), colors: self.mail_colors.unwrap_or(stored.colors) }
    }

    /// The name of this window's density in `ui.json` (task E8.2, `ui_state::density_from`): the
    /// kind's word of `--view`, and after a "/" the slice of `--lane` or `--source` – `mail`,
    /// `hub/lane:info`, `notifications/source:agent:claude-code`. So every window kind and every
    /// window per slice keeps a density of its own, though all of them share one state directory.
    ///
    /// The notifications window names the sources it was started with (task E8.4,
    /// [`Shown::slice`]): `notifications/source:script:ci+script:deploy`.
    pub fn density_key(&self) -> String {
        let kind = self.kind.word();
        let lane = match &self.view {
            Some(View::ForMe) => Some("lane:for-me".to_owned()),
            Some(View::Info) => Some("lane:info".to_owned()),
            Some(View::Mail) => Some("source:mail".to_owned()),
            Some(View::Source(source)) => Some(format!("source:{source}")),
            // No slice; `--lane` and `--source` start on no other view.
            _ => None,
        };
        let parts: Vec<String> = std::iter::once(kind.to_owned()).chain(lane).chain(self.shown.slice()).collect();
        parts.join("/")
    }

    /// "Now" at start: the fixed `--now`, else the clock. The demo data are built around it.
    pub fn start(&self) -> DateTime<Utc> {
        self.now.map_or_else(Utc::now, datetime::to_utc)
    }
}

/// The view chosen by `--lane for-me|info` or `--source <key>`; `None` when neither is given.
/// Both at once are refused: no view shows one lane of one source.
///
/// In the notifications window (task E8.4) `--source` is no view but the window's sources, given
/// as often as needed and together with `--lane` if wanted; mail is no source of this window.
fn start_view(args: &[String], kind: Kind) -> Result<(Option<View>, Shown), LaunchError> {
    if kind == Kind::Notifications {
        let sources = flag_values(args, "--source")?
            .into_iter()
            .map(|key| match key.parse::<SourceRef>() {
                Ok(source) if source.kind != SourceKind::Mail => Ok(source),
                _ => Err(LaunchError::UnknownValue {
                    flag: "--source",
                    value: key.to_owned(),
                    valid: "suite, agent:<name>, script:<name>".to_owned(),
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok((lane_view(args)?, Shown::from_args(sources)));
    }
    let lane = lane_view(args)?;
    let source = match flag_value(args, "--source")? {
        None => None,
        Some("mail") => Some(View::Mail),
        Some(key) => match key.parse::<SourceRef>() {
            Ok(source) => Some(View::Source(source)),
            Err(_) => {
                return Err(LaunchError::UnknownValue {
                    flag: "--source",
                    value: key.to_owned(),
                    valid: "suite, mail, agent:<name>, script:<name>, mail:<address>".to_owned(),
                });
            }
        },
    };
    match (lane, source) {
        (Some(_), Some(_)) => Err(LaunchError::Conflict("--lane", "--source")),
        (lane, source) => Ok((lane.or(source), Shown::Everything)),
    }
}

/// The view of `--lane for-me|info`; `None` without it.
fn lane_view(args: &[String]) -> Result<Option<View>, LaunchError> {
    match flag_value(args, "--lane")? {
        None => Ok(None),
        Some("for-me") => Ok(Some(View::ForMe)),
        Some("info") => Ok(Some(View::Info)),
        Some(other) => {
            Err(LaunchError::UnknownValue { flag: "--lane", value: other.to_owned(), valid: "for-me, info".to_owned() })
        }
    }
}

/// The text of the forced error state.
const DEMO_ERROR: &str = "The server did not answer (demo)";

/// One answer of the source: the entries of a view, the open entries of "For me", the count for
/// "Info", the sources.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The order the lists were asked in (task E8.15); an answer in another order than the window's
    /// now is dropped.
    pub order: Order,
    pub items: Vec<Item>,
    /// Open entries in "For me", whatever the view: their number is the one in the sidebar, the
    /// first of them are what the glance shows (mini, strip).
    pub for_me: Vec<Item>,
    /// Open entries in "Info", the number in the sidebar. Together with `for_me` it is what the
    /// sources count (open entries, both lanes), so the sidebar's numbers add up.
    pub info: usize,
    pub sources: Vec<SourceInfo>,
    /// The folders of the mail accounts, for the sidebar (task E4.2); empty without mail, and
    /// when they could not be read – the entries still show then.
    pub folders: Vec<Folder>,
    /// The messages of a folder's view: its first page (as many as were listed before, when the
    /// view is loaded again). `None` in the other views.
    pub mail: Option<MailPage>,
    /// The snoozed entries, for "Coming back" beside the hub's "Act now" (task E8.5); empty in every
    /// other window and view.
    pub snoozed: Vec<Item>,
    /// The done entries, for "Handed off" beside the hub's "Act now" (task E8.7b: an entry made into
    /// a task is done, its [`Item::task`] still open); empty in every other window and view.
    pub done: Vec<Item>,
    /// Whether it is quiet now, for the hub's "Quiet" (task E8.5b, `inbox.get_quiet`); `None` when it
    /// is not, in every other window and view, and when the core could not say.
    pub quiet: Option<torro_ipc::Quiet>,
    /// What the core said about a search beyond its messages (issue #136); `None` in other views.
    pub found: Option<Box<Found>>,
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

/// The whole mail of one message, for the panel: `loaded` is `None` while `mail.get_message` runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Letter {
    pub id: String,
    pub loaded: Option<Result<Mail, String>>,
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
    /// The overview's column with the cursor, and the one the medium and narrow layouts show.
    column: Column,
    /// "Info" unfolded at the end of the overview's columns, by [`Column::index`]. Kept while the
    /// window runs.
    info_open: [bool; 2],
    /// The sidebar's "Sources" unfolded (issue #121): folded until the person opens it, then as
    /// `ui.json` keeps it ([`SOURCES_KEY`]).
    sources_open: bool,
    /// The names `ui.json` keeps as unfolded, as last loaded or saved: a save keeps those this window
    /// does not decide (another window's parts), [`App::unfolded`].
    unfolded_names: Vec<String>,
    /// The scroll offset and visible height of the overview's columns, by [`Column::index`].
    column_scroll: [Option<(f32, f32)>; 2],
    /// The entry the open snooze menu, picker or lane question is for, as it was when opened.
    subject: Option<Item>,
    /// The message "Move to folder" or the question "Delete permanently?" is for (task E10.1).
    mail_subject: Option<String>,
    /// The day and time in the picker of "Pick…".
    pick: Pick,
    /// The dialog of "Make task" while [`Menu::Task`] is open.
    task: TaskDraft,
    /// The task lists for that dialog: `None` while they load, then the answer.
    lists: Option<Result<Vec<TaskList>, String>>,
    /// The id of the list the last task went into; the dialog chooses it first next time.
    last_list: Option<String>,
    /// The dialog of "Make event" while [`Menu::Event`] is open (task E8.5b).
    event: EventDraft,
    /// The calendars for that dialog: `None` while they load, then the answer.
    calendars: Option<Result<Vec<TaskList>, String>>,
    /// The id of the calendar the last event went into; the dialog chooses it first next time.
    last_calendar: Option<String>,
    /// An AI is set up for "AI task" and "AI event" (issue #120): asked at start and on every tick
    /// ([`SuggestSource::ai_ready`]); off until the answer is in.
    ai_ready: bool,
    /// What the AI does for the open dialog (issue #120); `None` for a dialog opened without it.
    ai_suggesting: Option<Suggesting>,
    /// Whether it is quiet now (task E8.5b), as the last load of the hub's "Act now" said.
    quiet: Option<torro_ipc::Quiet>,
    /// The ways to mute `subject` for the bell's menu (task E8.6b): `None` while they load, then
    /// the core's answer.
    mute_options: Option<Result<Vec<MuteOption>, String>>,
    /// "For" of the bell's menu; kept while the window runs.
    mute_for: MuteFor,
    /// "What happens" of the bell's menu; kept while the window runs.
    mute_effect: MuteEffect,
    /// The hint after the last change, with its undo.
    hint: Option<Hint>,
    /// The messages archived or deleted here whose move is on its way or not yet seen in a load
    /// (issue #205): they and their entries stay out of every list until the core is through.
    leaving: Vec<Leaving>,
    /// The approval whose decision is on its way (task E3.8): its Allow and Deny are disabled
    /// until the core answered, so a second click cannot decide it twice.
    deciding: Option<String>,
    /// The message whose invitation is being answered (task E8.3b): its Accept · Maybe · Decline take
    /// no click until the core answered.
    answering: Option<String>,
    /// The answers this window gave to invitations while it runs, by message id (task E8.3b): kept
    /// over what loads (the demo keeps no answers, and the core's sync may lag behind).
    answers: Vec<(String, InvitationAnswer)>,
    /// The calendars an invitation's event can go to (task E8.3c), asked once the first invitation
    /// is read: `None` until they are in, then the answer.
    invite_calendars: Option<Result<Vec<TaskList>, String>>,
    /// The calendar picked on an invitation's card (task E8.3c, [`App::invite_calendar`]); `None`
    /// until one is picked.
    invite_pick: Option<String>,
    /// The events of the shown calendars around the invitation of the message read (task E8.3c):
    /// the message's id and what `calendar.list_events` answered for it.
    invite_events: Option<(String, Result<Vec<CalendarEvent>, String>)>,
    /// The folders of the mail accounts (task E4.2), as last loaded.
    folders: Vec<Folder>,
    /// The messages of the folder's view, newest first; the cursor and the panel name them by id.
    messages: Vec<MessageSummary>,
    /// The cursor of the folder's next page; `None` on its last page.
    more: Option<String>,
    /// The panel shows the message under the cursor (a folder's view): the counterpart of `detail`
    /// for messages, which have no entry to keep.
    reading: bool,
    /// The whole mail in the panel: of the message under the cursor in a folder, or of the mail
    /// entry in the panel ([`App::wanted_letter`]).
    letter: Option<Letter>,
    /// The text of the mail or the entry the panel or reader shows, to select and copy (issue #119):
    /// follows them after every message ([`App::follow_text`]).
    text: Selectable,
    /// The folder whose messages `messages` holds; another folder's view shows loading until its
    /// messages are in.
    listed: Option<String>,
    /// What is typed into the search field over the list (issue #136).
    search: String,
    /// When what is typed should be searched for: [`search::DEBOUNCE`] after the last key.
    search_due: Option<Instant>,
    /// The view the search started from: "Clear search" goes back to it, and from a folder the bar
    /// offers its account and the folder itself as scopes.
    search_from: Option<View>,
    /// The suggestions under the field show: from a key in it until the search is made, a
    /// suggestion picked or Escape.
    suggesting: bool,
    /// The searches last made, newest first (`ui.json`, never sent anywhere).
    recent: Vec<String>,
    /// What the core said about the search shown beyond its messages.
    found: Found,
    /// Where the words of the search shown matched, per message.
    highlights: Vec<MailHighlight>,
    /// The accounts folded away or unfolded by the person (`true`: unfolded), also as `ui.json`
    /// kept them (issue #121, [`account_key`]); the others follow [`App::account_open`].
    folds: Vec<(String, bool)>,
    /// A window of mail only (`--source mail`): the sidebar lists the folders alone.
    mail_only: bool,
    /// The sources the window shows (task E8.4): `--source` at start, then the sidebar's checks and
    /// the chips. Window state, kept while it runs.
    shown: Shown,
    /// "By app | By time" of the notifications window (task E8.4).
    arrange: Arrange,
    /// The snoozed entries beside the hub's "Act now" ("Coming back", task E8.5), as last loaded.
    snoozed: Vec<Item>,
    /// The done entries beside the hub's "Act now", for "Handed off" (task E8.7b), as last loaded.
    done: Vec<Item>,
    /// The folder's message the dialog of "Make task" is for (task E8.7b, `t` or "Task" in a
    /// folder): `tasks.create_todo_from_mail` instead of `inbox.create_task_from_item` for
    /// [`App::subject`].
    task_mail: Option<MessageSummary>,
    /// The tasks "Task" made of folder messages while the window runs (task E8.7b), by message id:
    /// a message is no entry, so nothing else tells the reader its task.
    made: Vec<(String, torro_inbox::item::ItemTask)>,
    /// The mail of `--message` still to show ([`Launch::message`]); taken once the first entries are
    /// in ([`App::open_start_mail`]).
    start_mail: Option<String>,
    /// The mail of `--message` shown in its folder: it stays readable there though it may not be on
    /// the folder's first page ([`App::wanted_letter`]).
    pinned_mail: Option<String>,
    /// The HTML of the mail held ([`App::letter`]), parsed for the panel and the reader (task
    /// M9.7, [`App::follow_html`]); `None` while no mail with HTML is held.
    html: Option<mail::HtmlMail>,
    /// The settings `mail_view` and `mail_appearance` as last read, with `--mail-view` and
    /// `--mail-colors` over them ([`App::take_look`]).
    look: MailLook,
    /// HTML or text, as the switch "HTML | Text" (or `v`) stands: [`App::look`] until it is used,
    /// and again whenever the setting changes.
    mail_format: MailFormat,
    /// Light or dark, as the switch "Light | Dark" set it for the mail's content; `None` takes
    /// [`App::look`] (with `app` the window's appearance). Reset when the setting changes.
    mail_scheme: Option<Scheme>,
    /// The window's scale factor (device pixels per logical pixel, without the zoom), so the HTML
    /// is drawn sharp ([`App::html_scale`]).
    window_scale: f32,
    /// The tile of "Act now" clicked (task E8.5): its origin's entries alone show. Window state.
    filter: Option<Origin>,
    /// The hub shows To sort alone (its sidebar entry "To sort"), on the entries of its overview.
    sorting: bool,
    /// The hub's Insights (task E8.8b, `insights.rs`): shown instead of "Act now" while
    /// `insights.shown`, on the overview's entries (they keep the sidebar's numbers); its span,
    /// table switch and numbers stay while the window runs.
    insights: Insights,
    /// The calls started, for the tests.
    #[cfg(test)]
    sent: Vec<Sent>,
    appearance: Appearance,
    system_mode: Mode,
    menu: Option<Menu>,
    /// Which buttons the open menu for an entry hangs at (issue #122).
    menu_at: MenuAt,
    /// Whether the sidebar drawer is open (medium and narrow layouts).
    drawer: bool,
    /// The sidebar the user chose for the wide layout, kept in `ui.json`.
    sidebar: SidebarMode,
    /// The density chosen for this window (task E8.2): `--density`, else the one remembered in
    /// `ui.json`, else `None` – then the layout decides ([`App::density`]).
    density: Option<Density>,
    /// Newest or oldest first in this window's lists (task E8.15): remembered in `ui.json` under
    /// the density's key ([`Launch::density_key`]), newest first until chosen.
    order: Order,
    /// The order was turned in this window before `ui.json` answered: what it remembers is late.
    order_chosen: bool,
    /// The divider between the wide mail window's list and reader (issue #130): the width the person
    /// dragged the list to, remembered in `ui.json` under the density's key.
    split: Split,
    /// The suite-wide zoom (Ctrl + Plus / Minus / 0), the factor of the whole window.
    zoom: Zoom,
    /// Follows the design pack the suite settings name (task L.3).
    design: Follow,
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
    /// From a row's action bar (task E3.9): the cursor to the entry with this id, then what the
    /// key does there – make a task, done, snooze or hide.
    Act(String, keys::Command),
    /// The overview's "Info" at the end of this column: fold or unfold it.
    ToggleInfo(Column),
    /// "Mail | Notifications" in the header bar: show this column of the overview.
    ShowColumn(Column),
    /// A column of the overview scrolled (or was laid out): its offset and visible height.
    ColumnScrolled(Column, f32, f32),
    /// From a card of the overview: open what the entry with this id links to ("Go to window").
    OpenRow(String),
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
    /// A shortcut with Ctrl or ⌘ from [`keys::shortcut`] (task E10.1); it acts once the window knows
    /// that no text field holds the keyboard ([`App::shortcut`]).
    Shortcut(shortcuts::Command),
    /// That shortcut, and whether the search field held the keyboard when it came.
    ShortcutChecked(shortcuts::Command, bool),
    /// The list scrolled (or was laid out): its offset and visible height.
    Scrolled(f32, f32),
    /// From the panel: the snooze menu for the entry with this id.
    AskSnooze(String),
    /// From the panel: move the entry with this id to the other lane (asking first where a rule
    /// is possible).
    AskLane(String),
    /// From the hide menu: mute the source of the entry with this id, or unmute it – at once,
    /// with an undo.
    AskMute(String),
    /// From the panel's "…": the menu "Hide from Notification Hub" for the entry with this id (task E3.10); the
    /// bell's menu for mail and in the notifications window (task E8.6b, issue #123).
    AskHide(String),
    /// From the bell of a folder's message in the reader or the panel (issue #123): the bell's menu
    /// for the message with this id – its entry's, or its own traits without one.
    AskMuteMail(String),
    /// The ways to mute the entry with this id (`inbox.list_mute_options`, task E8.6b), or why
    /// they could not be read; dropped when the bell's menu is no longer open for it.
    MuteOptions(String, Result<Vec<MuteOption>, String>),
    /// A segment of "For" in the bell's menu.
    MuteFor(MuteFor),
    /// A segment of "What happens" in the bell's menu.
    MuteHow(MuteEffect),
    /// A way to mute in the bell's menu: `inbox.mute_item` for the entry with this id, with "For"
    /// and "What happens" as chosen.
    MuteBy(String, MuteScope),
    /// A suggestion of Insights' "Where you could cut back" (task E8.12, finding 14): `inbox.mute_item`
    /// for the entry with this id and scope with the suggestion's own effect ("Hide sender" hides,
    /// "Mute app" keeps popups away), for as long as "For" says.
    MuteSuggested(String, MuteScope, MuteEffect),
    /// The core's answer to `inbox.mute_item`, with the effect asked for (for the toast).
    MutedBy(Result<Muted, String>, MuteEffect),
    /// An undo that is no edit (hiding, task E3.11) is through, or failed.
    Undone(Result<(), String>, Option<String>),
    /// The parts unfolded in `ui.json` (task E3.11): which "Info" of the overview is open.
    Unfolded(Vec<String>),
    /// From the hide menu: move the mail of the entry with this id to its account's junk folder.
    Junk(String),
    /// A folder of "Move to folder" (task E10.1): move the message (first id) to the folder (second).
    MoveMail(String, String),
    /// "Delete" in the question "Delete permanently?" (task E10.1): remove its message for good.
    DeleteForGood,
    /// The answer to fetching mail now (task E10.1, `accounts.sync`).
    Fetched(Result<(), String>),
    /// "Pick…" in the snooze menu: the date and time picker.
    ShowPicker,
    /// The picker's day.
    PickDay(NaiveDate),
    /// The picker's time, an index into [`PICK_HOURS`].
    PickHour(usize),
    /// The picker's "Later".
    PickConfirm,
    /// The source's answer to muting or unmuting, with the hint to show (`None` after an undo).
    Muted(Result<(), String>, Option<Hint>),
    /// From the panel: open what the entry with this id links to – its page, its app, the agent's
    /// window – as far as [`detail::openable`] offers it.
    OpenItem(String),
    /// The source's answer to `inbox.open_item`.
    ItemOpened(Result<(), String>),
    /// From the panel of an approval: "Allow" (`allow: true`) or "Deny" for the approval with this
    /// id (task E3.8).
    Decide {
        approval: String,
        allow: bool,
    },
    /// The source's answer to deciding, with the decision for the hint.
    Decided(Result<(), InboxError>, bool),
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
    /// From the panel, "Event" or `c`: the dialog that makes an event of the entry with this id
    /// (task E8.5b).
    AskEvent(String),
    /// The calendars for that dialog, or why they could not be read.
    Calendars(Result<Vec<TaskList>, String>),
    /// The dialog's title field.
    EventTitle(String),
    /// The dialog's calendar.
    EventCalendar(TaskList),
    /// The dialog's day.
    EventDay(NaiveDate),
    /// The dialog's start time.
    EventStart(StartTime),
    /// The dialog's duration.
    EventLength(Length),
    /// "Create event", or `Enter` in the dialog.
    EventConfirm,
    /// The source's answer to `inbox.create_event_from_item`, with the calendar's name for the hint.
    EventCreated(Result<(), String>, String),
    /// "AI task" (issue #120): the dialog of "Make task" for the entry with this id, filled in by the
    /// AI's suggestion once it is in.
    SuggestTask(String),
    /// "AI event" (issue #120): the dialog of "Make event", filled in alike.
    SuggestEvent(String),
    /// Whether an AI is set up ([`SuggestSource::ai_ready`]).
    AiReady(bool),
    /// The AI's task for the entry with this id, or why there is none.
    TaskSuggested(String, Result<TaskSuggestion, String>),
    /// The AI's event for the entry with this id, or why there is none.
    EventSuggested(String, Result<EventSuggestion, String>),
    /// The note field of the dialog of "Make task" (issue #120).
    TaskNotes(String),
    /// The place field of the dialog of "Make event" (issue #120).
    EventLocation(String),
    /// The note field of the dialog of "Make event" (issue #120).
    EventNotes(String),
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
    /// The design pack stored in the suite settings, and its fonts loaded.
    Design(design::Message),
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
    /// "Compact | Comfortable" in the header bar (task E8.2): show the entries so, and remember it.
    SetDensity(Density),
    /// The density remembered for this window in `ui.json`, loaded at start (none: not chosen yet).
    StoredDensity(Option<Density>),
    /// "Newest first | Oldest first" in the header bar (task E8.15): turn the order, remember it and
    /// list anew.
    ToggleOrder,
    /// The order remembered for this window in `ui.json`, loaded at start (none: not chosen yet).
    StoredOrder(Option<Order>),
    /// The divider between the list and the reader (issue #130): grabbed, dragged, released, reset.
    Split(SplitEvent),
    /// The list's width remembered for this window in `ui.json`, loaded at start (none: the default).
    StoredSplit(Option<f32>),
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
    /// Open the control centre on the Inbox page, where the rules are ("All rules" in the bell's menu).
    OpenRules,
    /// From a folder's list: the message with this id under the cursor and in the panel.
    SelectMail(String),
    /// The whole mail for the panel, or why it could not be read.
    LetterLoaded(String, Result<Box<Mail>, String>),
    /// From the panel or a key: do this with the message with this id.
    Mail(MailAction, String),
    /// The source's answer to a [`MailEdit`] of these messages, with the hint to show (`None`:
    /// none, or an undo).
    MailEdited(Result<(), String>, Vec<String>, Option<Hint>),
    /// "Load more" at the end of a folder's messages.
    MoreMail,
    /// The next page of the folder's view it was asked for.
    MoreLoaded(View, Order, Result<MailPage, String>),
    /// What is typed into the search field: it is searched for [`search::DEBOUNCE`] later.
    SearchInput(String),
    /// `Enter` in the search field: search now and remember the search.
    Search,
    /// "Clear search" (or Escape in a search): back to the view the search started from.
    ClearSearch,
    /// The timer of a search that waits ([`search::TICK`]): it runs once it is due.
    SearchTick(Instant),
    /// A suggestion under the field: this query in the field, searched for soon.
    Suggest(String),
    /// Where the search looks: all mail, this account, this folder.
    SearchScope(Scope),
    /// How the results are ordered: by relevance or by date.
    SearchSort(MailSort),
    /// The searches last made, as `ui.json` keeps them.
    RecentSearches(Vec<String>),
    /// "Clear" beside the searches last made: forget them.
    ForgetSearches,
    /// An attachment of a message (message id, part) to fetch and open.
    OpenAttachment(String, String),
    /// What became of opening an attachment.
    AttachmentOpened(Result<Opened, String>),
    /// A web address under a mail's or an entry's text ("Links") was clicked.
    OpenMailLink(String),
    /// A click, drag or key on the text in the panel or the reader (issue #119): selects, never
    /// changes it (`Selectable::perform`).
    Text(text_editor::Action),
    /// "Copy text": the whole text in the panel or the reader to the clipboard, with "Copied".
    CopyText,
    /// The title of an account's group: fold its folders away or show them, and remember it.
    ToggleAccount(String),
    /// The title of the sidebar's "Sources": fold them away or show them, and remember it (issue #121).
    ToggleSources,
    /// The compose window could not be started.
    ComposeStarted(Result<(), String>),
    /// The switch "HTML | Text" over a mail with HTML (task M9.7), as `v` does.
    ShowMailAs(MailFormat),
    /// The switch "Light | Dark" (task M9.7): the colours of the mail's content, not the window's.
    MailColors(Scheme),
    /// A link in a mail's HTML was clicked (task M9.7): `http`, `https` and `mailto` open in the
    /// browser, anything else is dropped.
    HtmlLink(String),
    /// The settings `mail_view` and `mail_appearance`; an error keeps what the window has.
    MailLook(Result<MailLook, String>),
    /// The window's scale factor, at start and when it changes (another screen).
    WindowScale(f32),
    /// "This window shows" in the sidebar, or a chip (task E8.4): switch this source on or off.
    ToggleSource(SourceRef),
    /// "By app | By time" in the header bar of the notifications window.
    Arrange(Arrange),
    /// "Dismiss all" in the header bar: every notification shown, but approvals and the suite's.
    DismissAll,
    /// The answer to "Dismiss all": the entries dismissed, and why the rest was not, if it stopped.
    DismissedAll(Vec<String>, Option<String>),
    /// A tile of the hub's "Act now" (task E8.5): show only the entries of this origin, or – for the
    /// tile already chosen – all of them again.
    Filter(Origin),
    /// "To sort" in the hub's sidebar: the section To sort alone, on the overview's entries.
    ShowToSort,
    /// "Task" on a folder's message, or `t` in a folder (task E8.7b): the dialog of "Make task" for
    /// the message with this id, sent as `tasks.create_todo_from_mail`.
    AskMailTask(String),
    /// Accept · Maybe · Decline on an invitation (task E8.3b): account, message and the answer, sent
    /// as `mail.respond_to_invitation`.
    AnswerInvitation(String, String, InvitationAnswer),
    /// The core's answer for the invitation in the message with this id.
    InvitationAnswered(String, InvitationAnswer, Result<Delivery, String>),
    /// A calendar picked on an invitation's card (task E8.3c): its event goes there.
    InviteCalendar(TaskList),
    /// The calendars an invitation's event can go to (task E8.3c, `calendar.list_calendars`).
    InviteCalendars(Result<Vec<TaskList>, String>),
    /// The events around the invitation in the message with this id (task E8.3c,
    /// `calendar.list_events`).
    InviteEvents(String, Result<Vec<CalendarEvent>, String>),
    /// The answer to `tasks.create_todo_from_mail`: the task made of the message with this id.
    MailTaskCreated(String, Result<torro_inbox::item::ItemTask, String>),
    /// "Open task" (the card of a mail, "Handed off", task E8.7b): the task with this uid in the
    /// tasks app, through the core (`apps.open_app` with `task:<uid>`).
    OpenTask(String),
    /// "Mark mail done" on the suite's "You finished a task from a mail" with this id (task E8.7b):
    /// its mail entry done, then the question.
    MarkMailDone(String),
    /// The answer to "Mark mail done": the entries closed, and why the rest was not, if it stopped.
    MailDone(Vec<String>, Option<String>),
    /// The mail of `--message` that has no entry, loaded to find its folder (task E8.7b).
    StartMail(String, Result<Box<Mail>, String>),
    /// "Insights" in the hub's sidebar (task E8.8b): the page of what the inbox counted.
    ShowInsights,
    /// "Week | Month | Quarter" of Insights: count over this span.
    StatsRange(torro_ipc::StatsRange),
    /// "Table view" of Insights: the numbers as tables (`true`) or as charts.
    StatsTable(bool),
    /// The answer to `inbox.get_stats` for this span; dropped when another span is chosen by then.
    Stats(torro_ipc::StatsRange, Result<torro_ipc::InboxStats, String>),
}

/// Asks `inbox` for the entries of `view`, the counts for "For me" and "Info" and the sources.
/// `empty` (the forced empty state) asks nothing and answers an empty inbox.
///
/// Also the folders of the mail accounts, and for a folder's view its first `limit` messages
/// (task E4.2). Folders that cannot be read leave the sidebar without mail rather than the inbox
/// without entries; a folder's messages that cannot be read are the view's error.
///
/// Only the entries the window of `kind` shows ([`Kind::keeps`], task E8.3): the mail window counts
/// and lists mail alone. The smart view "With attachments" asks for the first `limit` messages of
/// every inbox and keeps those with attachments (`mail::attachments`), "Invitations" (task E8.3b)
/// those that invite to an event not over yet (`mail::invitations`, as of the demo's "now").
///
/// And only those of the sources `shown` (task E8.4, the notifications window's choice): with one
/// source the requests ask for it alone (the API's `source`), with more the entries of the others
/// are left out here.
///
/// The hub's overview (task E8.5) also asks for the snoozed entries, for "Coming back", the done
/// ones, for "Handed off" (task E8.7b), and whether it is quiet (task E8.5b) – a core that cannot
/// say leaves "Quiet" out rather than the entries.
///
/// `demo` is the demo's "now" with `--demo` (else `None`): the entries get the demo's tasks
/// ([`demo_task`]) before they are sorted into the window, as the core's would come.
/// Whether the open entry `item` holds the message `id`: it links to it ([`Link::Mail`]).
fn holds(item: &Item, id: &str) -> bool {
    matches!(&item.link, Some(Link::Mail { message }) if message == id) && item.state.is_open()
}

/// The entry `item` with the answer the window gave to the invitation of its mail (task E8.3c), as
/// [`keep_answer`] does for messages.
fn keep_entry_answer(answers: &[(String, InvitationAnswer)], item: &mut Item) {
    let Some(Link::Mail { message }) = &item.link else { return };
    let given = answers.iter().find(|(id, _)| id == message).map(|(_, answer)| *answer);
    if let (Some(answer), Some(invite)) = (given, item.invite.as_mut()) {
        invite.answered = Some(answer);
    }
}

/// `message` with the answer the window gave to its invitation, if `answers` holds one (task E8.3b,
/// [`App::answers`]).
fn keep_answer(answers: &[(String, InvitationAnswer)], message: &mut MessageSummary) {
    let given = answers.iter().find(|(id, _)| *id == message.id).map(|(_, answer)| *answer);
    if let (Some(answer), Some(invite)) = (given, message.invite.as_mut()) {
        invite.answered = Some(answer);
    }
}

/// What `view` lists, asked of `inbox`: its entries and, in a view of mail, the first `limit`
/// messages, both in `order` (task E8.15); beside them what the sidebar, the glance and the hub need
/// – "For me" always newest first, as the glance shows it.
pub async fn fetch<S: InboxSource + MailSource + HubSource>(
    inbox: Arc<S>,
    view: View,
    empty: bool,
    (limit, order): (u32, Order),
    (kind, shown): (Kind, Shown),
    demo: Option<DateTime<Utc>>,
) -> Result<Loaded, String> {
    if empty {
        let none = Loaded {
            order,
            items: Vec::new(),
            for_me: Vec::new(),
            info: 0,
            sources: Vec::new(),
            folders: Vec::new(),
            mail: view.listing().map(|_| MailPage::of(Vec::new(), None)),
            snoozed: Vec::new(),
            done: Vec::new(),
            quiet: None,
            found: None,
        };
        return Ok(none);
    }
    let failed = |err: torro_inbox::InboxError| err.to_string();
    let keep = |items: Vec<Item>| {
        items
            .into_iter()
            .map(|mut item| {
                if let Some(now) = demo {
                    demo_task(&mut item, now);
                    // The invitation of an entry's mail, as the core fills it in (task E8.3c).
                    item.invite = mail::demo_entry_invite(&item);
                }
                item
            })
            .filter(|item| kind.keeps(item) && shown.shows(&item.source))
            .collect::<Vec<_>>()
    };
    // One source shown: ask for it alone, unless the view names a source of its own.
    let narrow = |filter: ItemFilter| match (shown.single(), &filter.source) {
        (Some(source), None) => ItemFilter { source: Some(source.clone()), ..filter },
        _ => filter,
    };
    let for_me_filter = || narrow(View::ForMe.filter().unwrap_or_default());
    let items = match view.filter() {
        Some(filter) => keep(inbox.list_items(ItemFilter { order, ..narrow(filter) }).await.map_err(failed)?),
        None => Vec::new(),
    };
    // The overview lists both lanes: its entries hold "For me" and "Info" already.
    let mut for_me = match view {
        View::ForMe => items.clone(),
        View::Overview => items.iter().filter(|item| item.lane == Lane::ForMe).cloned().collect(),
        _ => keep(inbox.list_items(for_me_filter()).await.map_err(failed)?),
    };
    if order != Order::NewestFirst {
        rules::sort(&mut for_me);
    }
    let info = match view {
        View::Info => items.len(),
        View::Overview => items.iter().filter(|item| item.lane == Lane::Info).count(),
        _ => keep(inbox.list_items(narrow(View::Info.filter().unwrap_or_default())).await.map_err(failed)?).len(),
    };
    let sources = inbox.list_sources().await.map_err(failed)?;
    let folders = inbox.mail_folders().await.unwrap_or_default();
    let mut found = None;
    let mail = match &view {
        View::Folder(folder) => {
            Some(inbox.search_mail(folder.id.clone(), None, limit, None, order).await.map_err(failed)?)
        }
        // Nothing typed yet: nothing to ask.
        View::Search(search) if search.query.is_empty() => Some(MailPage::of(Vec::new(), None)),
        View::Search(search) => {
            let page = inbox.find_mail(search.find(order, limit.max(search::PAGE), None)).await.map_err(failed)?;
            // How far the index is, only while matches may be missing; without an answer the line
            // still says that some may be.
            let index = if page.complete { Vec::new() } else { inbox.mail_index_status().await.unwrap_or_default() };
            found = Some(Box::new(Found { complete: page.complete, total: page.total, index }));
            Some(MailPage { messages: page.messages, next: page.cursor, highlights: page.highlights })
        }
        View::Attachments => {
            let mut pages = Vec::new();
            for inbox_folder in folders.iter().filter(|folder| folder.role == Some(Role::Inbox)) {
                let page =
                    inbox.search_mail(inbox_folder.id.clone(), None, limit, None, order).await.map_err(failed)?;
                pages.push(page.messages);
            }
            Some(MailPage::of(mail::attachments(pages, order), None))
        }
        // The smart view "Invitations" (task E8.3b) the same way, from what the summaries say.
        View::Invitations => {
            let mut pages = Vec::new();
            for inbox_folder in folders.iter().filter(|folder| folder.role == Some(Role::Inbox)) {
                // The soonest event first: the order of arrival does not apply.
                let newest = Order::NewestFirst;
                let page =
                    inbox.search_mail(inbox_folder.id.clone(), None, limit, None, newest).await.map_err(failed)?;
                pages.push(page.messages);
            }
            Some(MailPage::of(mail::invitations(pages, demo.unwrap_or_else(Utc::now)), None))
        }
        _ => None,
    };
    // "Coming back" (task E8.5) and "Handed off" (task E8.7b) beside the hub's "Act now".
    let (snoozed, done, quiet) = match (kind, &view) {
        (Kind::Hub, View::Overview) => (
            keep(inbox.list_items(View::Snoozed.filter().unwrap_or_default()).await.map_err(failed)?),
            keep(inbox.list_items(View::Done.filter().unwrap_or_default()).await.map_err(failed)?),
            inbox.get_quiet().await.ok().flatten(),
        ),
        _ => (Vec::new(), Vec::new(), None),
    };
    Ok(Loaded { order, items, for_me, info, sources, folders, mail, snoozed, done, quiet, found })
}

impl<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static> App<S> {
    pub fn new(inbox: Arc<S>, suite: Source, launch: Launch) -> (App<S>, Task<Message>) {
        let open = launch.open;
        let offset = launch.now.map_or(TimeDelta::zero(), |now| datetime::to_utc(now) - Utc::now());
        let app = App {
            inbox,
            zoom: Zoom::for_source(&suite),
            design: Follow::for_source(&suite),
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
                // The first inbox, once the folders are in.
                // The search starts there too, so the bar offers the folder and its account.
                Some(Open::Folder | Open::Mail | Open::Search | Open::MuteMail | Open::MoveMail | Open::DeleteMail) => {
                    View::Mail
                }
                // The mail window's "For me" is its overview (task E8.3).
                Some(Open::MailEntry) if launch.kind == Kind::Mail => View::Overview,
                Some(Open::MailEntry) => View::ForMe,
                Some(Open::Attachments) => View::Attachments,
                Some(Open::Invitations | Open::Invitation) => View::Invitations,
                Some(
                    Open::Overview
                    | Open::OverviewInfo
                    | Open::OverviewNotices
                    | Open::ToSort
                    | Open::Tile
                    | Open::Insights
                    | Open::InsightsTable,
                ) => View::Overview,
                _ => launch.view.clone().unwrap_or(View::Overview),
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
            column: launch.kind.column_or_first(Column::default()),
            info_open: [open == Some(Open::OverviewInfo); 2],
            sources_open: false,
            unfolded_names: Vec::new(),
            column_scroll: [None; 2],
            subject: None,
            mail_subject: None,
            pick: Pick::start(Utc::now() + offset),
            task: TaskDraft::default(),
            lists: None,
            last_list: None,
            event: EventDraft::new(String::new(), Utc::now() + offset),
            calendars: None,
            last_calendar: None,
            ai_ready: false,
            ai_suggesting: None,
            quiet: None,
            mute_options: None,
            mute_for: MuteFor::default(),
            mute_effect: MuteEffect::Info,
            hint: None,
            leaving: Vec::new(),
            deciding: None,
            answering: None,
            answers: Vec::new(),
            invite_calendars: None,
            invite_pick: None,
            invite_events: None,
            folders: Vec::new(),
            messages: Vec::new(),
            more: None,
            reading: false,
            letter: None,
            text: Selectable::default(),
            listed: None,
            search: String::new(),
            search_due: None,
            search_from: None,
            suggesting: false,
            recent: Vec::new(),
            found: Found::default(),
            highlights: Vec::new(),
            folds: Vec::new(),
            mail_only: launch.view == Some(View::Mail),
            shown: launch.shown.clone(),
            arrange: if open == Some(Open::ByTime) { Arrange::ByTime } else { Arrange::default() },
            snoozed: Vec::new(),
            done: Vec::new(),
            task_mail: None,
            made: Vec::new(),
            start_mail: launch.message.clone(),
            pinned_mail: None,
            html: None,
            look: launch.look(MailLook::default()),
            mail_format: launch.look(MailLook::default()).format,
            mail_scheme: None,
            window_scale: 1.0,
            filter: None,
            sorting: open == Some(Open::ToSort),
            // Insights is the hub's (task E8.8b).
            insights: Insights {
                shown: launch.kind == Kind::Hub && matches!(open, Some(Open::Insights | Open::InsightsTable)),
                table: open == Some(Open::InsightsTable),
                ..Insights::default()
            },
            #[cfg(test)]
            sent: Vec::new(),
            appearance: Appearance::System,
            system_mode: Mode::None,
            menu: match open {
                Some(Open::Menu) => Some(Menu::Nav),
                Some(Open::Switcher) => Some(Menu::Switcher),
                _ => None,
            },
            menu_at: MenuAt::default(),
            // Without a sidebar the app switcher lives in the drawer.
            drawer: matches!(open, Some(Open::Drawer | Open::Switcher)),
            sidebar: SidebarMode::default(),
            density: launch.density,
            order: Order::default(),
            order_chosen: false,
            split: Split::default(),
            launch,
        };
        // The size, and the traffic lights onto the title band's line (U.11).
        let size = Task::batch([
            iced::window::latest().and_then(iced::window::size).map(Message::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        // `--density` wins over what `ui.json` remembers, so with it nothing is asked.
        let density = match app.launch.density {
            Some(_) => Task::none(),
            None => ui_state::load_density_task(INBOX, app.launch.density_key()).map(Message::StoredDensity),
        };
        let order = ui_state::load_order_task(INBOX, app.launch.density_key()).map(Message::StoredOrder);
        // Only the mail window has a divider (issue #130).
        let split = match app.launch.kind {
            Kind::Mail => ui_state::load_split_task(INBOX, app.launch.density_key()).map(Message::StoredSplit),
            _ => Task::none(),
        };
        let task = Task::batch([
            ui_state::load_task(INBOX).map(Message::UiState),
            ui_state::load_unfolded_task(INBOX).map(Message::Unfolded),
            ui_state::load_searches_task(INBOX).map(Message::RecentSearches),
            density,
            order,
            split,
            app.load(),
            app.fetch_appearance(),
            app.fetch_look(),
            iced::window::latest().and_then(iced::window::scale_factor).map(Message::WindowScale),
            app.fetch_ai_ready(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    /// The window title: the window's kind and its view ("Mail · Overview", [`Kind::title`]); the
    /// hub's To sort alone is "Inbox · To sort", its Insights "Inbox · Insights".
    pub fn title(&self) -> String {
        if self.is_hub() && (self.sorting || self.insights.shown) {
            return trf("Notification Hub · {view}", &[("view", &self.view_label())]);
        }
        self.launch.kind.title(&self.view)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(REFRESH).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Another screen, another scale: the HTML of a mail is drawn for it (task M9.7).
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Window(iced::window::Event::Rescaled(scale)) => Some(Message::WindowScale(scale)),
                _ => None,
            }),
            // Zoom keys work everywhere, the glance included.
            self.zoom.subscription().map(Message::Zoom),
            // The design pack polls the core every 2 s itself and wakes the window only for a change.
            self.design.subscription().map(Message::Design),
        ];
        // Mini and strip are displays: no keys, and nothing open that Escape could close. (A key
        // like `e` must not mark an entry done that nobody sees.)
        if !self.is_glance() {
            subscriptions.push(iced::event::listen_with(keys::listen));
            // The shortcuts with Ctrl or ⌘ (task E10.1; ⌘F / Ctrl + F searches, issue #136), also when
            // the search field or the window's guard took them; they all act on mail.
            if !self.folders.is_empty() {
                subscriptions.push(iced::event::listen_with(keys::shortcut));
            }
            // Escape closes an open menu, drawer, the toast or the panel; nothing listens while none is
            // open.
            if self.menu.is_some()
                || self.drawer
                || self.panel_open()
                || self.hint.is_some()
                || self.view.search().is_some()
            {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
            // Ctrl + Z (⌘ + Z) undoes while the toast offers it – not while the dialog of "Make task"
            // or the picker has the keys.
            if self.can_undo()
                && !matches!(self.menu, Some(Menu::Task | Menu::Event | Menu::Pick | Menu::DeleteForGood))
            {
                subscriptions.push(widgets::undo_key().map(|()| Message::Undo));
            }
        }
        if self.hint.is_some() {
            subscriptions.push(iced::time::every(HINT_TICK).map(Message::HintTick));
        }
        // A search waits for the typing to pause (issue #136).
        if self.search_due.is_some() {
            subscriptions.push(iced::time::every(search::TICK).map(Message::SearchTick));
        }
        // The end of a drag of the divider, wherever the button is released (issue #130).
        if self.split.dragging {
            subscriptions.push(split::release().map(|()| Message::Split(SplitEvent::Release)));
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
        let task = self.handle(message);
        // Whatever changed, the panel's mail follows the cursor and the entry in the panel.
        let letter = self.follow_letter();
        self.follow_text();
        self.follow_html();
        Task::batch([task, letter])
    }

    /// The parsed HTML of the mail held (task M9.7, [`App::html`]): made when a mail with HTML is
    /// in, kept while it stays, dropped with it – which stops its pictures still loading. The demo
    /// loads nothing from the web.
    fn follow_html(&mut self) {
        let mail = match &self.letter {
            Some(Letter { loaded: Some(Ok(mail)), .. }) if mail::shows_html(mail) => mail,
            _ => {
                self.html = None;
                return;
            }
        };
        if self.html.as_ref().is_some_and(|html| html.id == mail.summary.id) {
            return;
        }
        self.html = mail::HtmlMail::new(mail, &self.inbox, self.launch.demo);
    }

    /// The parsed HTML of the mail in the panel or reader, if it has HTML (task M9.7).
    fn html_for_letter(&self) -> Option<&Content> {
        let letter = self.letter.as_ref()?;
        self.html.as_ref().filter(|html| html.id == letter.id).map(|html| &html.content)
    }

    /// The colours of the mail's content (task M9.7): the switch's, else the setting's – with `app`
    /// those of the window, so "System" follows the desktop as the window does.
    fn mail_scheme(&self) -> Scheme {
        let dark = self.theme().extended_palette().is_dark;
        self.mail_scheme.unwrap_or_else(|| mail::scheme_for(self.look.colors, dark))
    }

    /// Device pixels per logical pixel for the HTML: the window's scale factor times the zoom, so
    /// its text is as sharp as the rest of the window.
    fn html_scale(&self) -> f32 {
        self.window_scale * self.zoom.factor()
    }

    /// Takes the settings `mail_view` and `mail_appearance` (task M9.7), with `--mail-view` and
    /// `--mail-colors` over them. When they changed, both switches go back to them; the same answer
    /// again (every tick) leaves the switches as the person set them.
    fn take_look(&mut self, stored: MailLook) {
        let look = self.launch.look(stored);
        if look != self.look {
            self.look = look;
            self.mail_format = look.format;
            self.mail_scheme = None;
        }
    }

    /// Asks the core for the mail settings (task M9.7), on every tick like the appearance.
    fn fetch_look(&self) -> Task<Message> {
        let suite = self.suite.clone();
        Task::perform(async move { mail::stored_look(&suite).await }, Message::MailLook)
    }

    /// The text shown in the panel or the reader ([`App::text`]): the loaded mail's, else the
    /// entry's message; empty for neither. Made new only when it changes, so a selection stays.
    fn follow_text(&mut self) {
        let shown = match (&self.letter, &self.detail) {
            (Some(Letter { loaded: Some(Ok(mail)), .. }), _) => mail::text_of(mail),
            (_, Some(item)) => detail::message_text(item),
            _ => String::new(),
        };
        if shown != self.text.text() {
            self.text = Selectable::new(&shown);
        }
    }

    /// What `message` does; [`App::update`] lets the panel's mail follow afterwards.
    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::DragWindow => return torro_gui::drag_window(),
            Message::Resized(size) => self.size = size,
            Message::Loaded(view, result) => {
                // An answer for a view the user has left since would show the wrong entries; one in
                // the order before the last turn (task E8.15) the wrong way round.
                if view != self.view || result.as_ref().is_ok_and(|loaded| loaded.order != self.order) {
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
                self.reading = false;
                self.cursor = None;
                self.pinned_mail = None;
                // "Act now" again, all of it (task E8.5), and no more Insights (task E8.8b).
                self.sorting = false;
                self.filter = None;
                self.insights.shown = false;
                // Another view leaves the search: its field starts empty.
                if view.search().is_none() {
                    self.leave_search_state();
                }
                if view != self.view {
                    self.view = view;
                    return self.load();
                }
            }
            Message::Select(id) => self.select(id),
            Message::Act(id, command) => return self.act(&id, command),
            Message::ToggleInfo(column) => return self.toggle_info(column),
            Message::ShowColumn(column) => return self.switch_column(column),
            Message::ColumnScrolled(column, offset, height) => {
                self.column_scroll[column.index()] = Some((offset, height));
            }
            Message::OpenRow(id) => {
                if let Some(item) = self.items.iter().find(|item| item.id == id).cloned() {
                    return self.open_entry(&item);
                }
            }
            Message::OpenForMe(id) => return self.open_for_me(id),
            Message::CloseDetail => self.close_panel(),
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
                    // Keys inside an open menu (its digits, the bell's arrows) leave it where it is.
                    if self.menu.is_none() {
                        self.menu_at = MenuAt::Keys;
                    }
                    return self.run(command);
                }
            }
            Message::Shortcut(command) => return self.shortcut(command),
            Message::ShortcutChecked(command, typing) => return self.shortcut_checked(command, typing),
            Message::Scrolled(offset, height) => self.scroll = Some((offset, height)),
            Message::AskSnooze(id) => {
                self.menu_at = MenuAt::Panel;
                self.ask_snooze(&id);
            }
            Message::AskLane(id) => {
                self.menu_at = MenuAt::Panel;
                return self.ask_lane(&id);
            }
            Message::AskMute(id) => return self.ask_mute(&id),
            // The bell of the notifications window's panel and of mail (task E8.6b, issue #123), "…"
            // of the others.
            Message::AskHide(id) => {
                self.menu_at = MenuAt::Panel;
                return self.ask_hide_or_mute(&id);
            }
            // The bell of a folder's message in the reader or the panel (issue #123).
            Message::AskMuteMail(id) => {
                self.menu_at = MenuAt::Panel;
                let entry =
                    mail::entry_of(&self.for_me, &id).map(|item| item.id.clone()).or_else(|| self.entry_of(&id));
                return match entry {
                    Some(entry) => self.ask_mute_menu(&entry),
                    None => self.ask_mute_message(&id),
                };
            }
            Message::MuteOptions(id, options) => {
                if self.menu == Some(Menu::Mute) && self.subject.as_ref().is_some_and(|item| item.id == id) {
                    self.mute_options = Some(options);
                }
            }
            Message::MuteFor(span) => self.mute_for = span,
            Message::MuteHow(effect) => self.mute_effect = effect,
            Message::MuteBy(id, scope) => return self.mute_by(id, scope, self.mute_effect),
            Message::MuteSuggested(id, scope, effect) => return self.mute_by(id, scope, effect),
            Message::MutedBy(Ok(muted), effect) => {
                let text = muted_note(&muted, effect, self.now);
                self.hint = Some(Hint { text, undo: Undo::of_muted(&muted), shown: Instant::now() });
                return self.load();
            }
            Message::MutedBy(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't mute the entry."), err));
            }
            Message::Undone(Ok(()), back) => {
                // Undone: the entry is back in its place, and the cursor with it.
                if back.is_some() {
                    self.cursor = back;
                }
                return self.load();
            }
            Message::Undone(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't undo the change."), err));
                return self.load();
            }
            Message::Unfolded(names) => {
                // A forced `--open overview-info` keeps both unfolded, so it can be photographed.
                if self.launch.open != Some(Open::OverviewInfo) {
                    self.info_open = Column::ALL.map(|column| names.iter().any(|name| name == column.info_key()));
                }
                self.sources_open = names.iter().any(|name| name == SOURCES_KEY);
                // An account the person folded or unfolded before the file answered keeps that.
                for account in names.iter().filter_map(|name| name.strip_prefix(ACCOUNT_KEY)) {
                    if !self.folds.iter().any(|(folded, _)| folded == account) {
                        self.folds.push((account.to_owned(), true));
                    }
                }
                self.unfolded_names = names;
            }
            Message::Junk(id) => return self.move_to_junk(&id),
            Message::MoveMail(id, folder) => return self.move_mail(id, folder),
            Message::DeleteForGood => return self.delete_for_good(),
            Message::Fetched(Ok(())) => return self.load(),
            Message::Fetched(Err(err)) => {
                // `accounts.sync` fetches calendars, tasks and GitHub too and answers the first failure of
                // any of them: the mail may well have arrived.
                self.error = Some(Failure::new(tr("Couldn't sync every account."), err));
                return self.load();
            }
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
                let mut item = *item;
                if self.launch.demo {
                    demo_task(&mut item, self.now);
                    item.invite = mail::demo_entry_invite(&item);
                }
                if let Some(detail) = self.detail.as_mut().filter(|detail| detail.id == item.id) {
                    *detail = item;
                }
                return self.load();
            }
            Message::Changed(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't change the entry."), err));
            }
            Message::OpenItem(id) => return self.open_item(&id),
            Message::AskTask(id) => {
                self.menu_at = MenuAt::Panel;
                return self.ask_task(&id);
            }
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
            Message::AskEvent(id) => {
                self.menu_at = MenuAt::Panel;
                return self.ask_event(&id);
            }
            Message::Calendars(result) => self.take_calendars(result),
            Message::EventTitle(title) => self.event.title = title,
            Message::EventCalendar(calendar) => self.event.calendar = Some(calendar),
            Message::EventDay(day) => self.event.day = day,
            Message::EventStart(start) => self.event.start = start,
            Message::EventLength(length) => self.event.length = length,
            Message::EventConfirm => return self.create_event(),
            Message::EventCreated(Ok(()), calendar) => {
                self.note(trf("Event created in {calendar}", &[("calendar", &calendar)]));
                return self.load();
            }
            Message::EventCreated(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't create the event."), err));
            }
            Message::SuggestTask(id) => {
                self.menu_at = MenuAt::Panel;
                return self.suggest_task(&id);
            }
            Message::SuggestEvent(id) => {
                self.menu_at = MenuAt::Panel;
                return self.suggest_event(&id);
            }
            // `--open no-ai` keeps the buttons off, to see them so in the demo.
            Message::AiReady(ready) => self.ai_ready = ready && self.launch.open != Some(Open::NoAi),
            Message::TaskSuggested(id, result) => self.take_task_suggestion(&id, result),
            Message::EventSuggested(id, result) => self.take_event_suggestion(&id, result),
            Message::TaskNotes(notes) => self.task.notes = notes,
            Message::EventLocation(location) => self.event.location = location,
            Message::EventNotes(notes) => self.event.notes = notes,
            // The entry is seen now; the list shows it.
            Message::ItemOpened(Ok(())) => return self.load(),
            Message::ItemOpened(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the entry."), err));
            }
            Message::Decide { approval, allow } => return self.decide(approval, allow),
            Message::Decided(result, allow) => return self.decided(result, allow),
            Message::Refresh => return self.load(),
            Message::Tick => {
                self.now = Utc::now() + self.offset;
                return Task::batch([
                    self.fetch_appearance(),
                    self.fetch_look(),
                    self.fetch_ai_ready(),
                    self.zoom.fetch().map(Message::Zoom),
                    self.load(),
                ]);
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            // The core may be starting up; keep the colours we have.
            Message::Appearance(Err(_)) => {}
            // No canvas keeps a cache: the next frame draws with the new design, the task loads its
            // fonts.
            Message::Design(message) => {
                if let Some(task) = self.design.update(message) {
                    return task.map(Message::Design);
                }
            }
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
            // The search button focuses the field over the list of mail, or opens a search where the
            // view has none (issue #136). "New" writes a message when there is a mail account to
            // write from (task E4.3); the inbox has nothing else to create. Elsewhere the buttons only
            // close what is open.
            Message::Switcher(Switcher::Search) if !self.folders.is_empty() => return self.focus_search(),
            Message::Switcher(Switcher::New) if !self.folders.is_empty() => {
                self.close_menus();
                return self.compose_new();
            }
            Message::Switcher(Switcher::New | Switcher::Search) => self.close_menus(),
            Message::ToggleNav => self.toggle_menu(Menu::Nav),
            Message::ShowDrawer => {
                self.menu = None;
                self.drawer = true;
            }
            Message::ToggleSidebar => return self.toggle_sidebar(),
            Message::UiState(state) => self.sidebar = state.sidebar,
            Message::SetDensity(density) => return self.set_density(density),
            Message::StoredDensity(stored) => {
                // `--density` wins; a choice made before the file answered as well.
                if self.density.is_none() {
                    self.density = stored;
                }
            }
            Message::ToggleOrder => return self.set_order(self.order.toggled()),
            Message::Split(event) => {
                if self.split.update(event, self.split_limits()) {
                    return ui_state::save_split_task(INBOX, self.launch.density_key(), self.split.chosen);
                }
            }
            Message::StoredSplit(stored) => {
                // A width dragged before the file answered wins.
                if self.split == Split::default() {
                    self.split = Split::with(stored);
                }
            }
            Message::StoredOrder(stored) => {
                // A turn made before the file answered wins; the first lists went out newest first.
                if let Some(stored) = stored.filter(|stored| !self.order_chosen && *stored != self.order) {
                    self.order = stored;
                    self.more = None;
                    return self.load();
                }
            }
            Message::CloseMenu => self.close_menus(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                } else if self.hint.is_some() {
                    // The toast goes first: it lies over everything, and the panel stays.
                    self.hint = None;
                } else if self.suggesting {
                    self.suggesting = false;
                } else if self.panel_open() || self.view.search().is_none() {
                    self.close_panel();
                } else {
                    // A search with nothing open in front of it: back to where it started.
                    return self.clear_search();
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
            Message::OpenRules => {
                self.close_menus();
                connect::open_section("inbox");
            }
            Message::SelectMail(id) => {
                if self.select_mail(id) {
                    return self.keep_opened_search();
                }
            }
            Message::LetterLoaded(id, result) => {
                let mut result = result.map(|mail| *mail);
                if let Ok(mail) = &mut result {
                    keep_answer(&self.answers, &mut mail.summary);
                }
                let invite = result.as_ref().ok().and_then(|mail| mail.summary.invite.clone());
                if let Some(letter) = self.letter.as_mut().filter(|letter| letter.id == id) {
                    letter.loaded = Some(result);
                    return self.ask_invite_side(&id, invite.as_ref());
                }
            }
            Message::Mail(action, id) => return self.mail_action(action, id),
            Message::MailEdited(Ok(()), ids, hint) => {
                // Moved: out of the core's lists soon, then they need no hiding any more.
                for leaving in self.leaving.iter_mut().filter(|leaving| ids.contains(&leaving.id)) {
                    leaving.answered = true;
                }
                if let Some(hint) = hint {
                    self.hint = Some(Hint { shown: Instant::now(), ..hint });
                }
                return self.load();
            }
            Message::MailEdited(Err(err), ids, _) => {
                // Not moved: the load brings them back. The load clears the error line, so a
                // message that comes back says why in the hint as well (issue #205).
                let before = self.leaving.len();
                self.leaving.retain(|leaving| !ids.contains(&leaving.id));
                if self.leaving.len() < before {
                    self.note(tr("Couldn't change the mail.").to_owned());
                }
                self.error = Some(Failure::new(tr("Couldn't change the mail."), err));
                return self.load();
            }
            Message::MoreMail => return self.more_mail(),
            Message::MoreLoaded(view, order, result) => {
                // A page of a view left since, or asked before the order turned, does not belong.
                if view != self.view || order != self.order {
                    return Task::none();
                }
                match result {
                    Ok(mut page) => {
                        search::with_passages(&mut page.messages, &page.highlights);
                        let known: Vec<String> = self.messages.iter().map(|message| message.id.clone()).collect();
                        self.messages.extend(page.messages.into_iter().filter(|message| !known.contains(&message.id)));
                        self.highlights.extend(page.highlights);
                        self.more = page.next;
                    }
                    Err(err) => self.error = Some(Failure::new(tr("Couldn't load more messages."), err)),
                }
            }
            Message::SearchInput(text) => return self.type_search(text),
            Message::Search => {
                self.suggesting = false;
                let remembered = self.remember_search();
                return Task::batch([self.run_search(), remembered]);
            }
            Message::ClearSearch => return self.clear_search(),
            Message::SearchTick(now) => {
                if self.search_due.is_some_and(|due| now >= due) {
                    return self.run_search();
                }
            }
            Message::Suggest(query) => {
                // An operator picked waits for its value: its suggestions stay; a whole word is done.
                let more = query.ends_with(':');
                let typed = self.type_search(query);
                self.suggesting = more;
                return Task::batch([typed, operation::focus(search::FIELD)]);
            }
            Message::SearchScope(scope) => {
                if let View::Search(shown) = &self.view {
                    let view = View::Search(SearchView { scope, ..shown.clone() });
                    return self.show_search(view);
                }
            }
            Message::SearchSort(sort) => {
                if let View::Search(shown) = &self.view {
                    let view = View::Search(SearchView { sort, ..shown.clone() });
                    return self.show_search(view);
                }
            }
            Message::RecentSearches(recent) => self.recent = recent,
            Message::ForgetSearches => {
                self.recent.clear();
                return ui_state::save_searches_task(INBOX, Vec::new());
            }
            Message::OpenAttachment(id, part) => {
                #[cfg(test)]
                self.sent.push(Sent::Attachment(id.clone(), part.clone()));
                let inbox = self.inbox.clone();
                return Task::perform(mail::open_attachment(inbox, id, part), Message::AttachmentOpened);
            }
            Message::AttachmentOpened(Ok(opened)) => self.note(mail::opened_text(&opened)),
            Message::AttachmentOpened(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the attachment."), err));
            }
            // Only web addresses become links ([`mail::pieces`]); checked again here all the same.
            Message::OpenMailLink(url) if url.starts_with("http://") || url.starts_with("https://") => {
                return self.open_url(url);
            }
            Message::OpenMailLink(_) => {}
            Message::Text(action) => self.text.perform(action),
            Message::CopyText => {
                if self.text.text().trim().is_empty() {
                    return Task::none();
                }
                self.note(tr("Copied").to_owned());
                return iced::clipboard::write(self.text.text().to_owned());
            }
            Message::ToggleAccount(account) => {
                let open = !self.account_open(&account);
                self.folds.retain(|(folded, _)| *folded != account);
                self.folds.push((account, open));
                return self.save_unfolded();
            }
            Message::ToggleSources => {
                self.sources_open = !self.sources_open;
                return self.save_unfolded();
            }
            Message::ComposeStarted(Ok(())) => {}
            Message::ComposeStarted(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the compose window."), err));
            }
            Message::ShowMailAs(format) => self.mail_format = format,
            Message::MailColors(scheme) => self.mail_scheme = Some(scheme),
            Message::HtmlLink(url) if mail::opens_in_browser(&url) => return self.open_url(url),
            Message::HtmlLink(_) => {}
            Message::MailLook(Ok(look)) => self.take_look(look),
            // The core may be starting up; keep what the window shows.
            Message::MailLook(Err(_)) => {}
            Message::WindowScale(scale) => self.window_scale = scale,
            Message::ToggleSource(source) => return self.toggle_source(&source),
            Message::Arrange(arrange) => self.arrange = arrange,
            Message::DismissAll => return self.dismiss_all(),
            Message::DismissedAll(done, failed) => return self.dismissed_all(done, failed),
            Message::Filter(origin) => self.toggle_filter(origin),
            Message::ShowToSort => return self.show_to_sort(),
            Message::AskMailTask(id) => {
                self.menu_at = MenuAt::Panel;
                return self.ask_mail_task(&id);
            }
            Message::AnswerInvitation(account, id, answer) => return self.answer_invitation(account, id, answer),
            Message::InvitationAnswered(id, answer, result) => return self.invitation_answered(id, answer, result),
            Message::InviteCalendar(calendar) => self.invite_pick = Some(calendar.id),
            Message::InviteCalendars(result) => self.invite_calendars = Some(result),
            Message::InviteEvents(id, result) => {
                if self.letter.as_ref().is_some_and(|letter| letter.id == id) {
                    self.invite_events = Some((id, result));
                }
            }
            Message::MailTaskCreated(id, Ok(task)) => {
                self.note(trf("Task created in {list}", &[("list", &task.list)]));
                self.made.retain(|(message, _)| *message != id);
                self.made.push((id, task));
            }
            Message::MailTaskCreated(_, Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't create the task."), err));
            }
            Message::OpenTask(uid) => return self.open_task(&uid),
            Message::MarkMailDone(id) => return self.mark_mail_done(&id),
            Message::MailDone(done, failed) => {
                if let Some(err) = failed {
                    self.error = Some(Failure::new(tr("Couldn't change the entry."), err));
                }
                if !done.is_empty() {
                    let text = tr("Mail marked as done").to_owned();
                    self.hint = Some(Hint { text, undo: Some(Undo::Dismissed(done)), shown: Instant::now() });
                }
                return self.load();
            }
            Message::StartMail(id, result) => return self.show_start_mail(id, result),
            Message::ShowInsights => return self.show_insights(),
            Message::StatsRange(range) => {
                if range != self.insights.range {
                    self.insights.range = range;
                    self.insights.stats = None;
                    return self.load_stats();
                }
            }
            Message::StatsTable(table) => self.insights.table = table,
            Message::Stats(range, stats) => {
                if range == self.insights.range {
                    self.insights.stats = Some(stats);
                }
            }
        }
        Task::none()
    }

    /// Whether the content is the hub's "Act now" or its To sort alone (task E8.5, `hub.rs`).
    fn is_hub(&self) -> bool {
        self.launch.kind == Kind::Hub && self.view == View::Overview
    }

    /// The name of what the window shows, in the header, the panel's path and the title: To sort
    /// while the hub shows it alone, Insights while it shows them (task E8.8b), else the view's name
    /// in this window ([`Kind::view_name`]).
    fn view_label(&self) -> String {
        if self.is_insights() {
            tr("Insights").to_owned()
        } else if self.is_hub() && self.sorting {
            tr("To sort").to_owned()
        } else {
            self.launch.kind.view_name(&self.view)
        }
    }

    /// Whether the content is the hub's Insights (task E8.8b, `insights.rs`).
    fn is_insights(&self) -> bool {
        self.is_hub() && self.insights.shown
    }

    /// "Insights" in the hub's sidebar: the page on the overview's entries, its numbers asked anew.
    fn show_insights(&mut self) -> Task<Message> {
        self.close_menus();
        self.detail = None;
        self.cursor = None;
        self.sorting = false;
        self.filter = None;
        self.insights.shown = true;
        if self.view != View::Overview {
            self.view = View::Overview;
        }
        self.load()
    }

    /// Asks for the numbers of Insights' span up to the window's day (`inbox.get_stats`); the forced
    /// states of `--open` as for the entries: loading never ends, the error comes at once.
    fn load_stats(&self) -> Task<Message> {
        let range = self.insights.range;
        match self.launch.open {
            Some(Open::Loading) => return Task::none(),
            Some(Open::Error) => return Task::done(Message::Stats(range, Err(DEMO_ERROR.into()))),
            _ => {}
        }
        let until = Some(local_today(self.now));
        let inbox = self.inbox.clone();
        Task::perform(async move { inbox.get_stats(range, until).await.map_err(|err| err.to_string()) }, move |stats| {
            Message::Stats(range, stats)
        })
    }

    /// A tile of "Act now": its origin's entries alone, or all again for the tile already chosen. A
    /// cursor on an entry the filter hides leaves it.
    fn toggle_filter(&mut self, origin: Origin) {
        self.filter = if self.filter.as_ref() == Some(&origin) { None } else { Some(origin) };
        let shown = self.cursor.as_deref().is_some_and(|id| self.rows().iter().any(|item| item.id == id));
        if !shown {
            self.cursor = None;
        }
    }

    /// "To sort" in the hub's sidebar: the overview's entries with To sort alone – no view of its own,
    /// so the entries stay those of "Act now" and nothing is asked anew when it was shown already.
    fn show_to_sort(&mut self) -> Task<Message> {
        self.close_menus();
        self.detail = None;
        self.cursor = None;
        self.sorting = true;
        self.filter = None;
        self.insights.shown = false;
        if self.view != View::Overview {
            self.view = View::Overview;
            return self.load();
        }
        Task::none()
    }

    /// The layout of the hub's content now ([`hub::layout`]), at the width it gets: the window less
    /// the sidebar or rail with the inset beside the content, the panel beside it and "Coming back"
    /// in the wide layout, and the hub's own inset. An estimate from the window's size, as
    /// [`App::board_width`], because the keys need it as well as the drawing.
    fn hub_layout(&self) -> hub::Layout {
        let class = self.class();
        let chrome = match shell::sidebar_mode(class, self.sidebar) {
            Some(SidebarMode::Expanded) => size::sidebar() + layout::inset(),
            Some(SidebarMode::Rail) => shell::rail() + layout::inset(),
            None => 0.0,
        };
        let panel = if class == SizeClass::Wide && self.panel_open() { size::panel() } else { 0.0 };
        let side = if self.hub_side() { size::props() } else { 0.0 };
        let layout = hub::layout(0.0, class, self.density(), self.sorting);
        let width = self.size.width - chrome - panel - side - hub::sides(layout.narrow);
        // Quiet as a line where the column at the right does not say it (task E8.5b).
        let quiet = self.quiet.is_some() && !self.hub_side() && !self.sorting;
        hub::Layout { quiet, ..hub::layout(width.max(0.0), class, self.density(), self.sorting) }
    }

    /// Whether "Coming back" stands beside "Act now": in the wide layout, while no panel takes its
    /// place.
    fn hub_side(&self) -> bool {
        self.is_hub() && !self.sorting && !self.insights.shown && self.class() == SizeClass::Wide && !self.panel_open()
    }

    /// Whether the content is the board of the notifications window (task E8.4,
    /// `notifications.rs`): its views New, Later and Done. A `--lane` slice keeps the list.
    fn is_board(&self) -> bool {
        self.launch.kind == Kind::Notifications && matches!(self.view, View::Overview | View::Snoozed | View::Done)
    }

    /// The width the board's entries get (task E8.4): the window less the sidebar or rail with the
    /// inset beside the content, the panel beside the board in the wide layout and the board's own
    /// inset. An estimate from the window's size, because the keys need the columns as well as the
    /// drawing; a few points off only move a column's edge.
    fn board_width(&self) -> f32 {
        let class = self.class();
        let chrome = match shell::sidebar_mode(class, self.sidebar) {
            Some(SidebarMode::Expanded) => size::sidebar() + layout::inset(),
            Some(SidebarMode::Rail) => shell::rail() + layout::inset(),
            None => 0.0,
        };
        let panel = if class == SizeClass::Wide && self.panel_open() { size::panel() } else { 0.0 };
        (self.size.width - chrome - panel - list::cards_side()).max(0.0)
    }

    /// How the board lays out the view now ([`notifications::board`]).
    fn board(&self) -> Board {
        notifications::board(self.board_width(), self.class(), self.arrange, self.density())
    }

    /// The sources "This window shows" lists ([`notifications::listed`]).
    fn listed_sources(&self) -> Vec<SourceRef> {
        notifications::listed(&self.sources, &self.shown)
    }

    /// A click in "This window shows" or on a chip (task E8.4): `source` on or off for this window,
    /// then its entries anew. The last shown source stays ([`Shown::toggled`]).
    fn toggle_source(&mut self, source: &SourceRef) -> Task<Message> {
        let shown = self.shown.toggled(source, &self.listed_sources());
        if shown == self.shown {
            return Task::none();
        }
        self.shown = shown;
        self.load()
    }

    /// "Dismiss all" (task E8.4): every notification of "New" with the ✕, but those of the suite
    /// ([`notifications::dismiss_all`]), one `inbox.set_item_done` with `dismissed: true` each. The
    /// answer comes as [`Message::DismissedAll`], whose toast takes them back.
    fn dismiss_all(&mut self) -> Task<Message> {
        if !self.is_board() || self.view != View::Overview {
            return Task::none();
        }
        let ids = notifications::dismiss_all(&self.items);
        if ids.is_empty() {
            return Task::none();
        }
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::DismissAll(ids.clone()));
        let inbox = self.inbox.clone();
        let dismissed = async move {
            let mut done = Vec::new();
            for id in ids {
                if let Err(err) = inbox.dismiss_item(id.clone()).await {
                    return (done, Some(err.to_string()));
                }
                done.push(id);
            }
            (done, None)
        };
        Task::perform(dismissed, |(done, failed)| Message::DismissedAll(done, failed))
    }

    /// The answer to "Dismiss all": the toast "n dismissed" with its undo for those that went
    /// ([`Undo::Dismissed`]), the error line when one did not, then the view anew.
    fn dismissed_all(&mut self, done: Vec<String>, failed: Option<String>) -> Task<Message> {
        if let Some(err) = failed {
            self.error = Some(Failure::new(tr("Couldn't change the entry."), err));
        }
        if !done.is_empty() {
            let text = trf("{n} dismissed", &[("n", &done.len())]);
            self.hint = Some(Hint { text, undo: Some(Undo::Dismissed(done)), shown: Instant::now() });
        }
        self.load()
    }

    /// `h`/`l` on the board "By app" (task E8.4): the cursor – and an open panel – to the same place
    /// in the column beside ([`notifications::beside`]).
    fn step_beside(&mut self, right: bool) -> Task<Message> {
        let Some(id) = self.cursor_item().map(|item| item.id.clone()) else { return Task::none() };
        let Some(to) = notifications::beside(&self.items, self.board(), &id, right) else { return Task::none() };
        if self.detail.is_some() {
            self.detail = self.items.iter().find(|item| item.id == to).cloned();
        }
        let index = self.rows().iter().position(|item| item.id == to);
        self.cursor = Some(to);
        index.map_or_else(Task::none, |index| self.reveal(index))
    }

    /// Whether the panel is open: an entry's, or a folder's message.
    fn panel_open(&self) -> bool {
        self.detail.is_some() || (self.view.is_mail() && self.reading)
    }

    /// Whether the open menu for an entry hangs at the panel's (or reader's) buttons rather than at
    /// the cursor row's (issue #122): where it was clicked; opened by a key, at the panel while it is
    /// open – the row may lie under it.
    fn menu_from_panel(&self) -> bool {
        match self.menu_at {
            MenuAt::Row => false,
            MenuAt::Panel => true,
            MenuAt::Keys => self.panel_open(),
        }
    }

    /// Closes the panel, whichever it is.
    fn close_panel(&mut self) {
        self.detail = None;
        self.reading = false;
    }

    /// The message whose whole mail the panel wants: in a folder the one under the cursor while
    /// the panel is open, elsewhere the mail of the entry in the panel.
    fn wanted_letter(&self) -> Option<String> {
        if self.view.is_mail() {
            let id = self.cursor.as_ref().filter(|_| self.reading)?;
            let listed = self.messages.iter().any(|message| message.id == *id);
            return (listed || self.pinned_mail.as_ref() == Some(id)).then(|| id.clone());
        }
        match self.detail.as_ref()?.link.as_ref()? {
            Link::Mail { message } => Some(message.clone()),
            Link::Url { .. } | Link::App { .. } | Link::Window => None,
        }
    }

    /// Asks for the mail the panel wants ([`App::wanted_letter`]) when it is another than the one
    /// held, and drops it when the panel wants none. A folder's message opened in the panel is
    /// marked read, as mail apps do; an entry's mail is read once the entry is done (the core).
    fn follow_letter(&mut self) -> Task<Message> {
        let wanted = self.wanted_letter();
        if wanted.as_deref() == self.letter.as_ref().map(|letter| letter.id.as_str()) {
            return Task::none();
        }
        let Some(id) = wanted else {
            self.letter = None;
            return Task::none();
        };
        self.letter = Some(Letter { id: id.clone(), loaded: None });
        #[cfg(test)]
        self.sent.push(Sent::Letter(id.clone()));
        let inbox = self.inbox.clone();
        let asked = id.clone();
        let fetch = Task::perform(
            async move { inbox.get_mail(asked).await.map(Box::new).map_err(|err| err.to_string()) },
            move |result| Message::LetterLoaded(id.clone(), result),
        );
        let unread = self
            .letter
            .as_ref()
            .and_then(|letter| self.messages.iter().find(|message| message.id == letter.id))
            .filter(|message| self.view.is_mail() && !message.flags.contains(&Flag::Seen))
            .map(|message| message.id.clone());
        match unread {
            Some(id) => Task::batch([fetch, self.mail_edit(mail::edit_of(MailAction::Read(true), &id), None)]),
            None => fetch,
        }
    }

    /// Carries out a mail button or key for the message `id`: Reply, Reply all and Forward open the
    /// compose window; Archive, Delete and Junk move it and offer to move it back; Flag and read
    /// change its flags.
    fn mail_action(&mut self, action: MailAction, id: String) -> Task<Message> {
        self.close_menus();
        if let Some(kind) = Compose::of(action) {
            return self.compose(kind, &id);
        }
        let Some(edit) = mail::edit_of(action, &id) else { return Task::none() };
        let delete = action == MailAction::Delete;
        if !matches!(action, MailAction::Archive | MailAction::Delete | MailAction::Junk) {
            return self.mail_edit(Some(edit), None);
        }
        if delete && self.in_trash(&id) {
            self.note(tr("This message is in the trash already.").to_owned());
            return Task::none();
        }
        let back = MailEdit::Move { ids: vec![id.clone()], to: self.home_of(&id) };
        let reopen = self.entry_of(&id);
        let text = if action == MailAction::Junk { tr("Moved to junk") } else { mail::moved_text(delete) };
        let hint = Hint { text: text.to_owned(), undo: Some(Undo::Mail { edit: back, reopen }), shown: Instant::now() };
        self.leaving.push(Leaving { id: id.clone(), answered: false });
        self.take_out(&id);
        self.mail_edit(Some(edit), Some(hint))
    }

    /// Sends `edit` (nothing for `None`), changing the flags shown at once; the answer comes as
    /// [`Message::MailEdited`] with `hint`.
    fn mail_edit(&mut self, edit: Option<MailEdit>, hint: Option<Hint>) -> Task<Message> {
        let Some(edit) = edit else { return Task::none() };
        for message in self.messages.iter_mut().filter(|message| edit.ids().contains(&message.id)) {
            message.flags = mail::flags_after(&message.flags, &edit);
        }
        if let Some(Letter { loaded: Some(Ok(mail)), .. }) = self.letter.as_mut()
            && edit.ids().contains(&mail.summary.id)
        {
            mail.summary.flags = mail::flags_after(&mail.summary.flags, &edit);
        }
        #[cfg(test)]
        self.sent.push(Sent::Mail(edit.clone(), hint.clone()));
        let inbox = self.inbox.clone();
        let ids = edit.ids().to_vec();
        Task::perform(edit.apply(inbox), move |result| Message::MailEdited(result, ids.clone(), hint.clone()))
    }

    /// Takes the message `id` off a folder's list at once (archived, deleted), and its entries
    /// off the lists of entries (issue #205): the cursor – and the panel – move to the row that
    /// took its place, so `e`, `e`, `e` works through a folder or "For me".
    fn take_out(&mut self, id: &str) {
        // A mail the reader shows beyond the folder's first page (`--message`) is in no row: the
        // reader closes, so the next key does not act on it again.
        if self.pinned_mail.as_deref() == Some(id) {
            self.pinned_mail = None;
            if self.cursor.as_deref() == Some(id) {
                self.cursor = self.messages.first().map(|message| message.id.clone());
                self.reading = false;
            }
        }
        if let Some(place) = self.messages.iter().position(|message| message.id == id) {
            self.messages.remove(place);
            if self.cursor.as_deref() == Some(id) {
                let last = self.messages.len().checked_sub(1);
                self.cursor = last.map(|last| self.messages[place.min(last)].id.clone());
                if self.cursor.is_none() {
                    self.reading = false;
                }
            }
        }
        let place = self.cursor_item().filter(|item| shows_mail(item, id)).and_then(|_| self.cursor_place());
        let followed = self.detail.as_ref().is_some_and(|detail| self.cursor.as_deref() == Some(detail.id.as_str()));
        self.items.retain(|item| !shows_mail(item, id));
        self.for_me.retain(|item| !shows_mail(item, id));
        if let Some(place) = place {
            let rows = self.rows();
            let last = rows.len().checked_sub(1);
            self.cursor = last.and_then(|last| rows.get(place.min(last))).map(|item| item.id.clone());
        }
        if self.detail.as_ref().is_some_and(|detail| shows_mail(detail, id)) {
            self.detail = if followed { self.cursor_item().cloned() } else { None };
        }
    }

    /// Keeps the messages on their way out ([`App::leaving`]) and their entries out of what just
    /// loaded; those the core moved and that no longer come back are forgotten.
    fn keep_out_leaving(&mut self) {
        let (messages, items, for_me) = (&self.messages, &self.items, &self.for_me);
        let held = |id: &str| {
            messages.iter().any(|message| message.id == id)
                || items.iter().chain(for_me).any(|item| shows_mail(item, id) && item.state.is_open())
        };
        self.leaving.retain(|leaving| !leaving.answered || held(&leaving.id));
        for leaving in &self.leaving {
            let id = leaving.id.as_str();
            self.messages.retain(|message| message.id != id);
            self.items.retain(|item| !shows_mail(item, id));
            self.for_me.retain(|item| !shows_mail(item, id));
        }
    }

    /// Where the message `id` goes back to on undo: its folder, as listed or loaded; an entry's
    /// mail lives in its account's inbox.
    fn home_of(&self, id: &str) -> MoveTarget {
        let listed = self.messages.iter().find(|message| message.id == id).map(|message| message.folder.clone());
        let loaded = || match &self.letter {
            Some(Letter { loaded: Some(Ok(mail)), .. }) if mail.summary.id == id => Some(mail.summary.folder.clone()),
            _ => None,
        };
        listed.or_else(loaded).map_or(MoveTarget::Role(MoveRole::Inbox), MoveTarget::Folder)
    }

    /// The open entry of the message `id` in the list, which the core closes when its mail leaves
    /// the inbox; undo opens it again.
    fn entry_of(&self, id: &str) -> Option<String> {
        self.items
            .iter()
            .find(|item| matches!(&item.link, Some(Link::Mail { message }) if message == id) && item.state.is_open())
            .map(|item| item.id.clone())
    }

    /// Whether the message `id` lies in a trash folder.
    fn in_trash(&self, id: &str) -> bool {
        let folder = match self.messages.iter().find(|message| message.id == id) {
            Some(message) => Some(message.folder.as_str()),
            None => match &self.letter {
                Some(Letter { loaded: Some(Ok(mail)), .. }) if mail.summary.id == id => {
                    Some(mail.summary.folder.as_str())
                }
                _ => None,
            },
        };
        folder.is_some_and(|folder| self.folders.iter().any(|f| f.id == folder && f.role == Some(Role::Trash)))
    }

    /// Starts the compose window for the message `id` ([`mail::compose_command`], task E4.3).
    fn compose(&mut self, kind: Compose, id: &str) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(Sent::Compose(mail::compose_args(kind, id, self.launch.demo)));
        let command = mail::compose_command(kind, id, self.launch.demo);
        Task::perform(async move { mail::start(command) }, Message::ComposeStarted)
    }

    /// `v` (task M9.7): the mail in the panel or reader switches between HTML and text – only a
    /// mail with HTML; nothing otherwise.
    fn switch_view(&mut self) -> Task<Message> {
        if self.html_for_letter().is_some() {
            self.mail_format = match self.mail_format {
                MailFormat::Html => MailFormat::Text,
                MailFormat::Text => MailFormat::Html,
            };
        }
        Task::none()
    }

    /// The entry `id` under the cursor and in the panel; "Coming back" opens snoozed entries, which
    /// the overview does not list (task E8.5).
    fn select(&mut self, id: String) {
        self.close_menus();
        self.detail = self.items.iter().chain(&self.snoozed).find(|item| item.id == id).cloned();
        if let Some(item) = &self.detail {
            self.column = self.launch.kind.column_or_first(Column::of(item));
            self.cursor = Some(id);
        }
    }

    /// The folder's message `id` under the cursor and in the panel; `false` when the folder does not
    /// list it.
    fn select_mail(&mut self, id: String) -> bool {
        self.close_menus();
        let listed = self.messages.iter().any(|message| message.id == id);
        if listed {
            self.cursor = Some(id);
            self.reading = true;
        }
        listed
    }

    /// Starts the compose window for a new message ([`compose::new_command`], task E4.3).
    fn compose_new(&mut self) -> Task<Message> {
        #[cfg(test)]
        self.sent.push(Sent::Compose(compose::new_args(self.launch.demo)));
        let command = compose::new_command(self.launch.demo);
        Task::perform(async move { mail::start(command) }, Message::ComposeStarted)
    }

    /// The message a mail key (`r`, `a`, `f`, `#`, `e` in a folder) acts on: in a folder the one
    /// under the cursor, elsewhere the mail of the entry under the cursor.
    fn mail_target(&self) -> Option<String> {
        if self.view.is_mail() {
            let id = self.cursor.as_ref()?;
            // The reader may show a mail that is not on the folder's first page (`--message`).
            let listed = self.messages.iter().any(|message| message.id == *id);
            return (listed || self.pinned_mail.as_ref() == Some(id)).then(|| id.clone());
        }
        match self.cursor_item()?.link.as_ref()? {
            Link::Mail { message } => Some(message.clone()),
            Link::Url { .. } | Link::App { .. } | Link::Window => None,
        }
    }

    /// A mail key: `action` for [`App::mail_target`]; nothing without one.
    fn key_mail(&mut self, action: MailAction) -> Task<Message> {
        match self.mail_target() {
            Some(id) => self.mail_action(action, id),
            None => Task::none(),
        }
    }

    /// A shortcut with Ctrl or ⌘ ([`keys::shortcut`], task E10.1). It acts only where the keys of the
    /// list act (nothing open in front of it), and – but searching – only while no text field holds
    /// the keyboard: the field keeps its own keys there (copy, paste, select all).
    ///
    /// The key event cannot tell where the keyboard is: the window's `widgets::ctrl_guard` captures
    /// `⌘ + letter` on macOS whatever has the focus, and a text field does not capture
    /// `Ctrl + Shift + letter`. So the window asks Iced whether its one field beside the list, the
    /// search field, has the focus (`operation::is_focused`), and acts on the answer
    /// ([`Message::ShortcutChecked`]). The dialogs with fields ("Make task", "Make event") take no
    /// shortcut at all ([`Focus::Task`]). Without a search field there is nothing to ask.
    fn shortcut(&mut self, command: shortcuts::Command) -> Task<Message> {
        if !matches!(self.focus(), Focus::List | Focus::Overview) {
            return Task::none();
        }
        if command.works_in_text_field() || !self.shows_search() {
            return self.shortcut_checked(command, false);
        }
        // A field that is not laid out (a narrow reader over the list) answers nothing: no focus.
        operation::is_focused(search::FIELD)
            .collect()
            .map(move |focused| Message::ShortcutChecked(command, focused.contains(&true)))
    }

    /// The shortcut `command`, `typing` when the search field held the keyboard: what its single key
    /// or button does ([`keys::of_shortcut`]) – unless the field keeps the key, or something opened
    /// in front of the list meanwhile.
    fn shortcut_checked(&mut self, command: shortcuts::Command, typing: bool) -> Task<Message> {
        if typing && !command.works_in_text_field() {
            return Task::none();
        }
        let Some(command) = keys::of_shortcut(command) else { return Task::none() };
        if !matches!(self.focus(), Focus::List | Focus::Overview) {
            return Task::none();
        }
        self.menu_at = MenuAt::Keys;
        self.run(command)
    }

    /// Whether the message `id` carries `flag`, as listed or loaded. A mail the window holds neither
    /// way (an entry's, not opened yet) counts as without: the shortcut then reads or flags it.
    fn mail_has(&self, id: &str, flag: Flag) -> bool {
        let listed = self.messages.iter().find(|message| message.id == id).map(|message| message.flags.contains(&flag));
        let loaded = || match &self.letter {
            Some(Letter { loaded: Some(Ok(mail)), .. }) if mail.summary.id == id => {
                Some(mail.summary.flags.contains(&flag))
            }
            _ => None,
        };
        listed.or_else(loaded).unwrap_or(false)
    }

    /// The account (`mail:<address>`) and folder of the message `id`, as listed or loaded; an entry's
    /// mail lies in its account's inbox.
    fn mail_home(&self, id: &str) -> Option<(String, Option<String>)> {
        if let Some(message) = self.messages.iter().find(|message| message.id == id) {
            return Some((message.account.clone(), Some(message.folder.clone())));
        }
        if let Some(Letter { loaded: Some(Ok(mail)), .. }) = &self.letter
            && mail.summary.id == id
        {
            return Some((mail.summary.account.clone(), Some(mail.summary.folder.clone())));
        }
        let holds = |item: &&Item| matches!(&item.link, Some(Link::Mail { message }) if message == id);
        let item = self.items.iter().chain(&self.for_me).chain(self.detail.as_ref()).find(holds)?;
        let account = item.source.key();
        let inbox = self.folders.iter().find(|folder| folder.account == account && folder.role == Some(Role::Inbox));
        Some((account, inbox.map(|folder| folder.id.clone())))
    }

    /// Opens "Move to folder" for the message `id` (task E10.1, as Outlook moves): the folders of its
    /// account but its own, as a menu of choices with digits ([`App::move_choices`]); a note when
    /// there is no other folder.
    fn ask_move(&mut self, id: String) -> Task<Message> {
        self.close_menus();
        self.mail_subject = Some(id);
        if self.move_choices().is_empty() {
            self.mail_subject = None;
            self.note(tr("There is no other folder to move it to.").to_owned());
            return Task::none();
        }
        self.menu = Some(Menu::Move);
        Task::none()
    }

    /// The entries of "Move to folder": the folders of [`App::mail_subject`]'s account in the
    /// sidebar's order ([`mail::folder_order`]), without the one it lies in.
    fn move_choices(&self) -> Vec<Choice> {
        let Some(id) = &self.mail_subject else { return Vec::new() };
        let Some((account, here)) = self.mail_home(id) else { return Vec::new() };
        let mut folders: Vec<&Folder> = self
            .folders
            .iter()
            .filter(|folder| folder.account == account && Some(&folder.id) != here.as_ref())
            .collect();
        folders.sort_by_key(|folder| mail::folder_order(folder));
        folders
            .into_iter()
            .map(|folder| {
                let to = Message::MoveMail(id.clone(), folder.id.clone());
                Choice::new(mail::folder_icon(folder.role), mail::folder_name(folder), to)
            })
            .collect()
    }

    /// Moves the message `id` to `folder` (task E10.1, "Move to folder", `mail.move_messages`): off the
    /// list at once, with a toast whose undo moves it back, as archiving does.
    fn move_mail(&mut self, id: String, folder: String) -> Task<Message> {
        self.close_menus();
        let name = self.folders.iter().find(|f| f.id == folder).map(mail::folder_name).unwrap_or_default();
        let back = MailEdit::Move { ids: vec![id.clone()], to: self.home_of(&id) };
        let hint = Hint {
            text: trf("Moved to {folder}", &[("folder", &name)]),
            undo: Some(Undo::Mail { edit: back, reopen: self.entry_of(&id) }),
            shown: Instant::now(),
        };
        self.take_out(&id);
        self.mail_edit(Some(MailEdit::Move { ids: vec![id], to: MoveTarget::Folder(folder) }), Some(hint))
    }

    /// Asks "Delete permanently?" for the message `id` (task E10.1) in a dialog over the window,
    /// because nothing brings the message back. The question belongs to the command, not to its
    /// keys: every way to delete for good comes here.
    fn ask_delete_for_good(&mut self, id: String) -> Task<Message> {
        self.close_menus();
        self.mail_subject = Some(id);
        self.menu = Some(Menu::DeleteForGood);
        Task::none()
    }

    /// "Delete" (or `Enter`) in the question: removes [`App::mail_subject`] for good
    /// (`mail.delete_messages` with `permanent`), off the list at once; the toast says so, without
    /// an undo.
    fn delete_for_good(&mut self) -> Task<Message> {
        let Some(id) = self.mail_subject.clone() else { return Task::none() };
        self.close_menus();
        let hint = Hint { text: tr("Deleted permanently").to_owned(), undo: None, shown: Instant::now() };
        self.take_out(&id);
        self.mail_edit(Some(MailEdit::DeleteForGood { ids: vec![id] }), Some(hint))
    }

    /// Fetches mail from the servers now (task E10.1, the shortcut "fetch", `accounts.sync`) and lists
    /// anew once it is through; nothing without a mail account.
    fn fetch_mail(&mut self) -> Task<Message> {
        if self.folders.is_empty() {
            return Task::none();
        }
        #[cfg(test)]
        self.sent.push(Sent::Fetch);
        let inbox = self.inbox.clone();
        Task::perform(async move { inbox.sync_mail().await.map_err(|err| err.to_string()) }, Message::Fetched)
    }

    /// Whether the field of the search stands over the list (issue #136): in every view of the mail
    /// window, and in the views of mail of the others – once there is a mail account.
    fn shows_search(&self) -> bool {
        !self.folders.is_empty() && (self.launch.kind == Kind::Mail || self.view.is_mail())
    }

    /// Puts the keyboard into the search field (`/`, ⌘F / Ctrl + F, the search button); where the
    /// view has no field, an empty search opens with it.
    fn focus_search(&mut self) -> Task<Message> {
        self.close_menus();
        if self.folders.is_empty() {
            return Task::none();
        }
        if !self.shows_search() {
            self.search_from = Some(self.view.clone());
            self.view = View::Search(SearchView::new("", Scope::All, Sort::Relevance));
            self.cursor = None;
            self.detail = None;
            self.reading = false;
            self.messages.clear();
            self.listed = self.view.listing();
        }
        operation::focus(search::FIELD)
    }

    /// Something was typed into the field: it is searched for [`search::DEBOUNCE`] from now; the
    /// suggestions show. A blank field goes back to where the search started.
    fn type_search(&mut self, text: String) -> Task<Message> {
        self.search = text;
        self.suggesting = true;
        if self.search.trim().is_empty() {
            self.search_due = None;
            if self.view.search().is_none() {
                return Task::none();
            }
            // Where the view the search came from has no field (the hub's views), the empty search
            // stays, with the searches last made; elsewhere the field stands over that view's list.
            let from_field = self.launch.kind == Kind::Mail || self.search_from.as_ref().is_some_and(View::is_mail);
            if from_field {
                return self.clear_search();
            }
            return self.show_search(View::Search(SearchView::new("", Scope::All, Sort::Relevance)));
        }
        self.search_due = Some(Instant::now() + search::DEBOUNCE);
        Task::none()
    }

    /// Searches for what is typed now: the search view with it, in the scope and order shown (all
    /// mail, by relevance, for a new search).
    fn run_search(&mut self) -> Task<Message> {
        self.search_due = None;
        let (scope, sort) = match &self.view {
            View::Search(shown) => (shown.scope.clone(), shown.sort),
            _ => (Scope::All, Sort::Relevance),
        };
        let query = self.search.trim();
        if query.is_empty() {
            return Task::none();
        }
        self.show_search(View::Search(SearchView::new(query, scope, sort)))
    }

    /// Shows `view`, a search, remembering where the search started; the same search again asks
    /// nothing.
    fn show_search(&mut self, view: View) -> Task<Message> {
        if view == self.view {
            return Task::none();
        }
        if self.view.search().is_none() {
            self.search_from = Some(self.view.clone());
            self.reading = false;
            self.detail = None;
        }
        self.view = view;
        self.cursor = None;
        self.load()
    }

    /// A result of the search opened: the search was worth keeping (issue #136), and its
    /// suggestions close. Nothing outside a search.
    fn keep_opened_search(&mut self) -> Task<Message> {
        if self.view.search().is_none() {
            return Task::none();
        }
        self.suggesting = false;
        self.remember_search()
    }

    /// Keeps the search shown among the searches last made, in `ui.json`.
    fn remember_search(&mut self) -> Task<Message> {
        let recent = search::remember(&self.recent, &self.search);
        if recent == self.recent {
            return Task::none();
        }
        self.recent = recent.clone();
        ui_state::save_searches_task(INBOX, recent)
    }

    /// "Clear search": the field empty, and back to the view the search started from (the window's
    /// first view without one).
    fn clear_search(&mut self) -> Task<Message> {
        let back = self.search_from.clone();
        self.leave_search_state();
        if self.view.search().is_none() {
            return Task::none();
        }
        // Without one: the window's first view.
        let back = back.unwrap_or(View::Overview);
        self.update(Message::Show(back))
    }

    /// Forgets what belongs to a search: the field, the timer, where it started, its results' extras.
    fn leave_search_state(&mut self) {
        self.search.clear();
        self.search_due = None;
        self.search_from = None;
        self.suggesting = false;
        self.found = Found::default();
        self.highlights.clear();
    }

    /// The scopes the bar offers: from a folder, its account and the folder itself.
    fn search_scopes(&self) -> Option<ScopeChoice> {
        let folder = self.search_from.as_ref().and_then(View::folder).or(self.view.folder())?;
        let chosen = self.view.search().map_or(Scope::All, |search| search.scope.clone());
        Some(ScopeChoice { folder: folder.clone(), chosen })
    }

    /// The suggestions under the field while they show.
    fn search_suggestions(&self) -> Vec<Suggestion> {
        let empty_search = self.view.search().is_some_and(|search| search.query.is_empty());
        if !(self.suggesting || (empty_search && self.search.trim().is_empty())) {
            return Vec::new();
        }
        search::suggestions(&self.search, &self.recent, &search::people(&self.messages), &self.folders)
    }

    /// `Tab`: the first suggestion, as a click on it.
    fn take_suggestion(&mut self) -> Task<Message> {
        match self.search_suggestions().into_iter().next() {
            Some(first) => self.update(Message::Suggest(first.query)),
            None => Task::none(),
        }
    }

    /// The bar over the list: field, suggestions, scope and order, the line on the results.
    fn search_bar(&self) -> Element<'_, Message> {
        let search = self.view.search();
        let status = search
            .filter(|search| !search.query.is_empty() && self.listed == self.view.listing() && self.loaded)
            .map(|_| search::status_line(self.messages.len(), self.more.is_some(), &self.found));
        search::bar(search::Bar {
            input: &self.search,
            search,
            scopes: self.search_scopes(),
            suggestions: self.search_suggestions(),
            status,
        })
    }

    /// "Load more": the next page of the folder or the search.
    fn more_mail(&mut self) -> Task<Message> {
        let Some(cursor) = self.more.clone() else { return Task::none() };
        let (inbox, view, order) = (self.inbox.clone(), self.view.clone(), self.order);
        let next: std::pin::Pin<Box<dyn std::future::Future<Output = Result<MailPage, String>> + Send>> = match &view {
            View::Folder(folder) => {
                let folder = folder.id.clone();
                let cursor = cursor.clone();
                Box::pin(async move {
                    inbox
                        .search_mail(folder, None, mail::PAGE, Some(cursor), order)
                        .await
                        .map_err(|err| err.to_string())
                })
            }
            View::Search(shown) => {
                let find = shown.find(order, search::PAGE, Some(cursor.clone()));
                Box::pin(async move {
                    let page = inbox.find_mail(find).await.map_err(|err| err.to_string())?;
                    Ok(MailPage { messages: page.messages, next: page.cursor, highlights: page.highlights })
                })
            }
            _ => return Task::none(),
        };
        #[cfg(test)]
        self.sent.push(Sent::More(cursor));
        Task::perform(next, move |result| Message::MoreLoaded(view.clone(), order, result))
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
                // A mail is shown in this app (task E4.2); the demo source only marks it seen.
                Link::Window | Link::Mail { .. } => {}
            }
        }
        #[cfg(test)]
        self.sent.push(Sent::Open(item.id.clone()));
        let (inbox, id) = (self.inbox.clone(), item.id.clone());
        Task::perform(async move { inbox.open_item(id).await.map_err(|err| err.to_string()) }, Message::ItemOpened)
    }

    /// Allows or denies `approval` through the source (`approvals.decide_approval`, task E3.8).
    /// Nothing while a decision is on its way; the panel's buttons are disabled then anyway.
    fn decide(&mut self, approval: String, allow: bool) -> Task<Message> {
        if self.deciding.is_some() {
            return Task::none();
        }
        self.deciding = Some(approval.clone());
        #[cfg(test)]
        self.sent.push(Sent::Decide(approval.clone(), allow));
        let inbox = self.inbox.clone();
        Task::perform(async move { inbox.decide_approval(approval, allow).await }, move |result| {
            Message::Decided(result, allow)
        })
    }

    /// The source's answer to [`App::decide`]: a hint with the decision, or with why there was
    /// none, and the list again – the approval's entry is done now, or soon (the core files it).
    /// An approval decided elsewhere or expired is no failure, only a hint.
    fn decided(&mut self, result: Result<(), InboxError>, allow: bool) -> Task<Message> {
        self.deciding = None;
        match result {
            Ok(()) => self.note(if allow { tr("Allowed") } else { tr("Denied") }.to_owned()),
            Err(InboxError::Settled) => self.note(InboxError::Settled.to_string()),
            Err(err) => self.error = Some(Failure::new(tr("Couldn't decide."), err.to_string())),
        }
        self.load()
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
        self.done = loaded.done;
        self.items = loaded.items;
        for item in &mut self.items {
            keep_entry_answer(&self.answers, item);
        }
        self.items.sort_by_key(|item| !list::is_waiting(item));
        // The hub's "Done today" (task E8.5): what was done today, local time.
        if self.launch.kind == Kind::Hub && self.view == View::Done {
            let today = local_today(self.now);
            self.items.retain(|item| matches!(item.state, State::Done { at } if local_today(at) == today));
        }
        self.snoozed = loaded.snoozed;
        self.quiet = loaded.quiet;
        self.for_me = loaded.for_me;
        self.for_me.sort_by_key(|item| !list::is_waiting(item));
        self.info = loaded.info;
        self.sources = loaded.sources;
        self.folders = loaded.folders;
        let page = loaded.mail.unwrap_or(MailPage::of(Vec::new(), None));
        self.messages = page.messages;
        for message in &mut self.messages {
            keep_answer(&self.answers, message);
        }
        search::with_passages(&mut self.messages, &page.highlights);
        self.highlights = page.highlights;
        self.found = loaded.found.map(|found| *found).unwrap_or_default();
        self.more = page.next;
        self.keep_out_leaving();
        self.listed = self.view.listing();
        self.loaded = true;
        // The forced error line stays over every load, so it can be photographed.
        self.error = (self.launch.open == Some(Open::Failed))
            .then(|| Failure::new(tr("Couldn't change the entry."), DEMO_ERROR));
        // A window of mail starts on the first account's inbox once the folders are known.
        if self.view == View::Mail
            && let Some(inbox) = mail::first_inbox(&self.folders)
        {
            self.view = View::Folder(FolderView::of(inbox));
            return self.load();
        }
        if self.view.is_mail() {
            // The mail of `--message` keeps the cursor, listed or not (task E8.7b).
            let pinned = self.pinned_mail.is_some() && self.cursor == self.pinned_mail;
            if !pinned && self.cursor.as_ref().is_none_or(|id| !self.messages.iter().any(|message| message.id == *id)) {
                let last = self.messages.len().checked_sub(1);
                let row = place.zip(last).and_then(|(place, last)| self.messages.get(place.min(last)));
                self.cursor = row.map(|message| message.id.clone());
            }
        } else if self.cursor_item().is_none() {
            let rows = self.rows();
            let last = rows.len().checked_sub(1);
            let next = place.zip(last).and_then(|(place, last)| rows.get(place.min(last))).map(|i| i.id.clone());
            self.cursor = next;
        }
        if let Some(detail) = &self.detail {
            // The panel follows the entry when it changed (a new report, woken up); one opened from
            // "Coming back" is among the snoozed ones.
            let fresh = self.items.iter().chain(&self.snoozed).find(|item| item.id == detail.id).cloned();
            match fresh {
                Some(fresh) => self.detail = Some(fresh),
                None if followed => self.detail = self.cursor_item().cloned(),
                None => {}
            }
        }
        let start = self.open_start_mail();
        Task::batch([start, self.open_launched()])
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

    /// Where the cursor's row is among the rows the keys walk ([`App::rows`]).
    fn cursor_place(&self) -> Option<usize> {
        let id = self.cursor.as_deref()?;
        if self.view.is_mail() {
            return self.messages.iter().position(|message| message.id == id);
        }
        self.rows().iter().position(|item| item.id == id)
    }

    /// The entries the keys walk, top to bottom: in the overview the column with the cursor
    /// ([`Sections::order`], "Info" only while unfolded), else the view's list.
    fn rows(&self) -> Vec<&Item> {
        // The notifications window walks its board (task E8.4).
        if self.is_board() {
            return notifications::order(&self.items, self.board(), &self.view, self.order);
        }
        // The hub walks its sections (task E8.5): Decide, Agents, To sort – or To sort alone.
        if self.is_hub() {
            return hub::Sections::of(&self.items, self.filter.as_ref()).order(self.sorting);
        }
        if self.view == View::Overview {
            Sections::of(&self.items, self.column, self.now)
                .ordered(self.order)
                .order(self.info_open[self.column.index()])
        } else {
            self.items.iter().collect()
        }
    }

    /// A button of a row's action bar (task E3.9): the cursor – and an open panel – to the entry
    /// `id`, then `command` as its key would do it, so a menu opens at the row.
    fn act(&mut self, id: &str, command: Command) -> Task<Message> {
        // Its menu hangs at the button clicked, also while the panel is open (issue #122).
        self.menu_at = MenuAt::Row;
        if self.view.is_mail() {
            return self.act_for_message(id, command);
        }
        let Some(item) = self.items.iter().find(|item| item.id == id).cloned() else { return Task::none() };
        self.close_menus();
        self.column = self.launch.kind.column_or_first(Column::of(&item));
        self.cursor = Some(item.id.clone());
        if self.detail.is_some() {
            self.detail = Some(item);
        }
        self.run(command)
    }

    /// A button of a message's card for the message's entry `id` in "For me" (task E8.11: "Later" and
    /// "…" of an invitation in the smart view, `mail::message_buttons`): the cursor to the message,
    /// whose card the menu then opens at, and the snooze menu or the bell's menu for the entry, as
    /// under "For me". Nothing for an entry the window does not hold, or other commands.
    fn act_for_message(&mut self, id: &str, command: Command) -> Task<Message> {
        // A message's own button ("Task", task E8.7b): the cursor to it, then what its key does there.
        if self.messages.iter().any(|message| message.id == id) {
            self.close_menus();
            self.cursor = Some(id.to_owned());
            return self.run(command);
        }
        let Some(Link::Mail { message }) = self.for_me.iter().find(|item| item.id == id).and_then(|i| i.link.clone())
        else {
            return Task::none();
        };
        self.close_menus();
        self.cursor = Some(message);
        match command {
            Command::Snooze => self.ask_snooze(id),
            // Mail is muted through the bell's menu (issue #123).
            Command::Hide => return self.ask_mute_menu(id),
            _ => {}
        }
        Task::none()
    }

    /// The entry in "For me" of the message under the cursor (task E8.11), in the views of mail.
    fn cursor_message_entry(&self) -> Option<String> {
        let id = self.cursor.as_deref()?;
        mail::entry_of(&self.for_me, id).map(|item| item.id.clone())
    }

    /// Folds or unfolds "Info" at the end of `column` and remembers it in `ui.json` (task E3.11). A
    /// cursor on a row folded away leaves it.
    fn toggle_info(&mut self, column: Column) -> Task<Message> {
        let open = &mut self.info_open[column.index()];
        *open = !*open;
        if !*open && self.cursor_item().is_some_and(|item| Column::of(item) == column && item.lane == Lane::Info) {
            self.cursor = None;
        }
        self.save_unfolded()
    }

    /// Remembers what is unfolded in `ui.json` ([`App::unfolded`]).
    fn save_unfolded(&mut self) -> Task<Message> {
        self.unfolded_names = self.unfolded();
        ui_state::save_unfolded_task(INBOX, self.unfolded_names.clone())
    }

    /// The names of the parts unfolded, as `ui.json` keeps them: "Info" of the overview's columns
    /// ([`Column::info_key`]), "Sources" ([`SOURCES_KEY`]) and the accounts unfolded by the person
    /// ([`account_key`], issue #121). Names this window does not decide – an account it has no
    /// choice for, a part of a later version – stay as they were loaded.
    pub fn unfolded(&self) -> Vec<String> {
        let info = Column::ALL.into_iter().map(Column::info_key);
        let decided = |name: &str| {
            info.clone().any(|key| key == name)
                || name == SOURCES_KEY
                || self.folds.iter().any(|(account, _)| account_key(account) == name)
        };
        let kept = self.unfolded_names.iter().filter(|name| !decided(name)).cloned();
        let open_info = Column::ALL.into_iter().filter(|column| self.info_open[column.index()]);
        let sources = self.sources_open.then(|| SOURCES_KEY.to_owned());
        let accounts = self.folds.iter().filter(|(_, open)| *open).map(|(account, _)| account_key(account));
        kept.chain(open_info.map(|c| c.info_key().to_owned())).chain(sources).chain(accounts).collect()
    }

    /// The overview: the cursor to the column `to`, on the row at the same place (or the column's
    /// last), the open panel along with it. Without a cursor only the column changes (medium and
    /// narrow show it). Nothing in the other views, nor to a column this window's kind does not
    /// show (the mail window has no "Notifications", [`Kind::columns`]).
    fn switch_column(&mut self, to: Column) -> Task<Message> {
        // The hub's "Act now" has sections, no columns (task E8.5).
        let columns = self.launch.kind.columns();
        if self.view != View::Overview || self.is_hub() || to == self.column || !columns.contains(&to) {
            return Task::none();
        }
        let place = self.cursor_place();
        self.column = to;
        let Some(place) = place.or(self.cursor.as_ref().map(|_| 0)) else { return Task::none() };
        let rows = self.rows();
        let index = place.min(rows.len().saturating_sub(1));
        let Some(item) = rows.get(index).map(|item| (*item).clone()) else {
            self.cursor = None;
            return Task::none();
        };
        self.cursor = Some(item.id.clone());
        if self.detail.is_some() {
            self.detail = Some(item);
        }
        self.reveal(index)
    }

    /// The entry with `id`: in the list, or the one in the panel or an open menu, or in "For me" –
    /// the views of mail list messages, whose entries are there (task E8.11).
    fn find(&self, id: &str) -> Option<&Item> {
        let held = self.items.iter().chain(self.detail.as_ref()).chain(self.subject.as_ref());
        held.chain(&self.for_me).find(|item| item.id == id)
    }

    /// Where the keys go: a menu of choices, the picker, another menu or the drawer, else the list.
    fn focus(&self) -> Focus {
        match self.menu {
            Some(Menu::Snooze | Menu::Lane | Menu::Hide | Menu::Move) => Focus::Choices,
            Some(Menu::DeleteForGood) => Focus::Question,
            Some(Menu::Mute) => Focus::Mute,
            Some(Menu::Pick) => Focus::Picker,
            Some(Menu::Task | Menu::Event) => Focus::Task,
            Some(Menu::Nav | Menu::Switcher) => Focus::Elsewhere,
            None if self.drawer => Focus::Elsewhere,
            // Insights has no rows: the keys of entries must not act on "Act now" hidden behind it.
            None if self.is_insights() => Focus::Elsewhere,
            // The hub's "Act now" is one list of sections: `h`/`l` change no column there.
            None if self.view == View::Overview && !self.is_hub() => Focus::Overview,
            None => Focus::List,
        }
    }

    /// Carries out what a key does ([`keys::command`]).
    fn run(&mut self, command: Command) -> Task<Message> {
        let cursor = self.cursor_item().map(|item| item.id.clone());
        match command {
            Command::Next => self.step(true),
            Command::Previous => self.step(false),
            Command::ColumnLeft if self.is_board() => self.step_beside(false),
            Command::ColumnRight if self.is_board() => self.step_beside(true),
            Command::ColumnLeft => self.switch_column(Column::Mail),
            Command::ColumnRight => self.switch_column(Column::Notices),
            Command::Open => self.open_cursor(),
            // In a folder `e` archives: a message has no "done" of its own.
            Command::Done if self.view.is_mail() => self.key_mail(MailAction::Archive),
            // The question after a finished task answers the mail it waits for (task E8.7b).
            Command::Reply if self.cursor_item().is_some_and(hub::is_follow_up) => self.reply_follow_up(),
            Command::Reply => self.key_mail(MailAction::Reply),
            Command::ReplyAll => self.key_mail(MailAction::ReplyAll),
            Command::Forward => self.key_mail(MailAction::Forward),
            Command::Delete => self.key_mail(MailAction::Delete),
            // The shortcuts of task E10.1 that have no single key.
            Command::DeleteForGood => self.mail_target().map_or_else(Task::none, |id| self.ask_delete_for_good(id)),
            Command::Archive => self.key_mail(MailAction::Archive),
            Command::Junk => self.key_mail(MailAction::Junk),
            Command::ToggleRead => match self.mail_target() {
                Some(id) => {
                    let read = self.mail_has(&id, Flag::Seen);
                    self.mail_action(MailAction::Read(!read), id)
                }
                None => Task::none(),
            },
            Command::Flag => match self.mail_target() {
                Some(id) => {
                    let flagged = self.mail_has(&id, Flag::Flagged);
                    self.mail_action(MailAction::Flag(!flagged), id)
                }
                None => Task::none(),
            },
            Command::MoveToFolder => self.mail_target().map_or_else(Task::none, |id| self.ask_move(id)),
            Command::NewMail if !self.folders.is_empty() => {
                self.close_menus();
                self.compose_new()
            }
            Command::NewMail => Task::none(),
            Command::Fetch => self.fetch_mail(),
            Command::Search => self.focus_search(),
            Command::SwitchView => self.switch_view(),
            Command::Complete => self.take_suggestion(),
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
            // The ✕ (task E8.4): notifications only – the mail window dismisses nothing.
            Command::Dismiss => match self.cursor_item().cloned() {
                Some(item) if self.launch.kind != Kind::Mail && notifications::dismissable(&item) => {
                    self.edit(Edit::Dismiss { id: item.id })
                }
                _ => Task::none(),
            },
            Command::Undo => self.undo(),
            // In the views of mail `s` and `m` act on the message's entry in "For me", as its card's
            // "Later" and "…" do (task E8.11).
            Command::Snooze | Command::Hide if self.view.is_mail() => match self.cursor_message_entry() {
                Some(entry) => self.act_for_message(&entry, command),
                // A message without an entry is muted by its own traits (issue #123).
                None if command == Command::Hide => {
                    self.cursor.clone().map_or_else(Task::none, |id| self.ask_mute_message(&id))
                }
                None => Task::none(),
            },
            Command::Snooze => {
                if let Some(id) = cursor {
                    self.ask_snooze(&id);
                }
                Task::none()
            }
            Command::Lane => cursor.map_or_else(Task::none, |id| self.ask_lane(&id)),
            // In the notifications window and for mail `m` is the bell (task E8.6b, issue #123): its
            // menu; "This entry" of the hide menu is the ✕ there. Else "Hide from Notification Hub".
            Command::Hide => cursor.map_or_else(Task::none, |id| self.ask_hide_or_mute(&id)),
            // In a folder the message under the cursor becomes a task (task E8.7b).
            Command::MakeTask if self.view.is_mail() => {
                self.cursor.clone().map_or_else(Task::none, |id| self.ask_mail_task(&id))
            }
            Command::MakeTask => cursor.map_or_else(Task::none, |id| self.ask_task(&id)),
            // A folder's message becomes an event only through its entry (task E8.5b).
            Command::MakeEvent if self.view.is_mail() => match self.cursor.clone().and_then(|id| self.entry_of(&id)) {
                Some(entry) => self.ask_event(&entry),
                None => Task::none(),
            },
            Command::MakeEvent => cursor.map_or_else(Task::none, |id| self.ask_event(&id)),
            Command::Choose(n) => match self.choices().into_iter().nth(n.wrapping_sub(1)) {
                Some(choice) => self.update(choice.message),
                None => Task::none(),
            },
            Command::MuteFor { longer } => {
                self.mute_for = step_in(&MuteFor::ALL, self.mute_for, longer);
                Task::none()
            }
            Command::MuteHow { stronger } => {
                self.mute_effect = step_in(&MUTE_EFFECTS, self.mute_effect, stronger);
                Task::none()
            }
            Command::EarlierDay => self.update(Message::PickDay(self.pick.day - Days::new(1))),
            Command::LaterDay => self.update(Message::PickDay(self.pick.day + Days::new(1))),
            Command::EarlierTime => self.update(Message::PickHour(self.pick.hour.saturating_sub(1))),
            Command::LaterTime => self.update(Message::PickHour(self.pick.hour + 1)),
            Command::Confirm if self.menu == Some(Menu::Task) => self.create_task(),
            Command::Confirm if self.menu == Some(Menu::Event) => self.create_event(),
            Command::Confirm if self.menu == Some(Menu::DeleteForGood) => self.delete_for_good(),
            Command::Confirm => self.update(Message::PickConfirm),
            Command::Sidebar => self.toggle_sidebar(),
        }
    }

    /// Moves the cursor one row down or up and stops at the first and last row; with no cursor
    /// yet, down chooses the first row and up the last. The open panel shows the new row.
    fn step(&mut self, forward: bool) -> Task<Message> {
        let rows = if self.view.is_mail() { self.messages.len() } else { self.rows().len() };
        let Some(last) = rows.checked_sub(1) else { return Task::none() };
        let next = match (self.cursor_place(), forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(place), true) => (place + 1).min(last),
            (Some(place), false) => place.saturating_sub(1),
        };
        if self.view.is_mail() {
            // The panel, when open, follows the cursor ([`App::follow_letter`]).
            self.cursor = Some(self.messages[next].id.clone());
            return self.reveal(next);
        }
        let Some(item) = self.rows().get(next).map(|item| (*item).clone()) else { return Task::none() };
        self.cursor = Some(item.id.clone());
        if self.detail.is_some() {
            self.detail = Some(item);
        }
        self.reveal(next)
    }

    /// Scrolls the list the least that shows row `index` completely – with its group head, when
    /// it is the first of its group ([`list::row_span`]).
    fn reveal(&mut self, index: usize) -> Task<Message> {
        let overview = self.view == View::Overview && !self.is_board() && !self.is_hub();
        let (scroll, span, id) = if self.is_hub() {
            // "Act now" scrolls as one (task E8.5), its tiles or numbers above the sections.
            let sections = hub::Sections::of(&self.items, self.filter.as_ref());
            let tiles = hub::tiles(&self.items).len();
            let layout = self.hub_layout();
            let span = self.rows().get(index).and_then(|item| hub::span(&sections, tiles, layout, &item.id));
            (self.scroll, span, list::SCROLL)
        } else if self.is_board() {
            // The board scrolls as one; "By app" has its heads above the scrolling part.
            let (board, density) = (self.board(), self.density());
            let span = self
                .rows()
                .get(index)
                .and_then(|item| notifications::span(&self.items, board, &self.view, density, &item.id, self.order));
            (self.scroll, span, list::SCROLL)
        } else if overview {
            // The overview: the column with the cursor, which scrolls on its own.
            let column = self.column;
            let sections = Sections::of(&self.items, column, self.now).ordered(self.order);
            let open = self.info_open[column.index()];
            let (look, lead, width) = (self.look(), self.lead().is_some(), self.card_width());
            let span = sections
                .order(open)
                .get(index)
                .and_then(|item| overview::row_span(&sections, open, &item.id, look, lead, width));
            (self.column_scroll[column.index()], span, column.scroll_id())
        } else if self.view.is_mail() {
            // A folder's messages have no group heads: every row is as high as the next; a card is
            // as high as what it shows.
            let width = self.card_width();
            let span = self.messages.get(index).map(|message| match self.look() {
                Look::Cards => {
                    let above = self.messages[..index].iter().map(|message| mail::message_card_height(message, width));
                    list::card_span(above, mail::message_card_height(message, width))
                }
                Look::Rows => {
                    // The cursor's row carries the strip with its actions below its lines.
                    let row = list::message_height();
                    let top = index as f32 * row;
                    (top, top + row + list::strip_height())
                }
            });
            (self.scroll, span, list::SCROLL)
        } else {
            (self.scroll, list::row_span(&self.items, index, self.look(), self.card_width()), list::SCROLL)
        };
        let (Some((offset, height)), Some((top, bottom))) = (scroll, span) else { return Task::none() };
        let target = if top < offset {
            top
        } else if bottom > offset + height {
            bottom - height
        } else {
            return Task::none();
        };
        // The viewport is only reported on the next frame; keys can repeat faster than that.
        if overview {
            self.column_scroll[self.column.index()] = Some((target, height));
        } else {
            self.scroll = Some((target, height));
        }
        operation::scroll_to(id, AbsoluteOffset { x: None, y: Some(target) })
    }

    /// Enter: opens what the entry under the cursor links to – its page, its app, the agent's
    /// window – as [`App::open_entry`] does; an entry without such a link, and a mail (task E4.2),
    /// opens in the panel. In a folder Enter opens the message under the cursor in the panel.
    fn open_cursor(&mut self) -> Task<Message> {
        if self.view.is_mail() {
            return match self.cursor.clone() {
                Some(id) => self.update(Message::SelectMail(id)),
                None => Task::none(),
            };
        }
        let Some(item) = self.cursor_item().cloned() else { return Task::none() };
        match detail::openable(&item) {
            Some(Link::Mail { .. }) | None => self.update(Message::Select(item.id)),
            Some(_) => self.open_entry(&item),
        }
    }

    /// Opens the snooze menu for the entry with `id`. Done entries are not snoozed, and an approval is
    /// never put off (task E8.5, "Knöpfe je Eintrag"): it waits for yes or no, a note says so.
    fn ask_snooze(&mut self, id: &str) {
        let Some(item) = self.find(id).filter(|item| !matches!(item.state, State::Done { .. })).cloned() else {
            return;
        };
        if detail::is_approval(&item) {
            self.note(tr("An approval closes once it is decided.").to_owned());
            return;
        }
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
    /// (design: confirm *or* undo, never both). The suite is never muted ([`rules_allowed`]). The
    /// hide menu's "Mute …" ends here.
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

    /// Whether the menu of `m`, "…" and the bell is the bell's menu for `item` (task E8.6b): in the
    /// notifications window, and for mail in every window – mail's "Hide from Notification Hub" went into the
    /// bell's menu (issue #123), so mail has one menu to mute it.
    fn mutes_by_bell(&self, item: &Item) -> bool {
        self.launch.kind == Kind::Notifications || item.source.kind == SourceKind::Mail
    }

    /// Whether `item` shows the button of `m` – the bell or "…" – and `m` opens a menu for it: in the
    /// notifications window where it has a bell ([`notifications::bell`]); for every mail entry (a
    /// done one too: its sender can still be muted); else while "Hide from Notification Hub" has entries for it
    /// ([`App::hide_choices`]).
    pub fn hidable(&self, item: &Item) -> bool {
        match self.launch.kind {
            Kind::Notifications => notifications::bell(item).is_some(),
            _ if item.source.kind == SourceKind::Mail => true,
            _ => !self.hide_choices(item).is_empty(),
        }
    }

    /// `m`, "…" or the bell for the entry `id`: the bell's menu where [`App::mutes_by_bell`] says
    /// so, else "Hide from Notification Hub".
    fn ask_hide_or_mute(&mut self, id: &str) -> Task<Message> {
        if self.find(id).is_some_and(|item| self.mutes_by_bell(item)) {
            return self.ask_mute_menu(id);
        }
        self.ask_hide(id);
        Task::none()
    }

    /// Opens the bell's menu for the entry with `id` (task E8.6b) and asks the core for its ways to
    /// mute (`inbox.list_mute_options`); "For" and "What happens" keep what was chosen last. The
    /// suite's entries – approvals among them – are never muted (security rule 1): a note instead.
    fn ask_mute_menu(&mut self, id: &str) -> Task<Message> {
        let Some(item) = self.find(id).cloned() else { return Task::none() };
        if !rules_allowed(&item.source) || detail::is_approval(&item) {
            self.note(tr("Entries from the suite can't be muted.").to_owned());
            return Task::none();
        }
        self.open_mute_menu(item)
    }

    /// Opens the bell's menu for the message `id` of a folder or smart view that has no entry (issue
    /// #123). The core offers its traits – sender, domain, list … as its mail store holds them – for
    /// the message's id (`inbox.list_mute_options` and `inbox.mute_item` with `m:…`). The menu's
    /// subject is the message as an entry would be ([`rules::message_stand_in`]); only the menu
    /// reads it.
    fn ask_mute_message(&mut self, id: &str) -> Task<Message> {
        let Some(summary) = self.message_summary(id) else { return Task::none() };
        match rules::message_stand_in(&summary, None, false) {
            Some(stand_in) => self.open_mute_menu(stand_in),
            None => Task::none(),
        }
    }

    /// The bell's menu for `item` – an entry, or a message standing in for one –, its ways to mute
    /// asked for anew.
    fn open_mute_menu(&mut self, item: Item) -> Task<Message> {
        let id = item.id.clone();
        self.drawer = false;
        self.subject = Some(item);
        self.mute_options = None;
        self.menu = Some(Menu::Mute);
        #[cfg(test)]
        self.sent.push(Sent::MuteOptions(id.clone()));
        let inbox = self.inbox.clone();
        Task::perform(
            async move {
                let options = inbox.list_mute_options(id.clone()).await.map_err(|err| err.to_string());
                (id, options)
            },
            |(id, options)| Message::MuteOptions(id, options),
        )
    }

    /// Mutes by the entry `id` with `scope` (`inbox.mute_item`), for as long as "For" says and with
    /// `effect` (the bell's menu passes its "What happens", a suggestion of Insights its own), and
    /// closes the menu; the answer comes as [`Message::MutedBy`].
    fn mute_by(&mut self, id: String, scope: MuteScope, effect: MuteEffect) -> Task<Message> {
        self.close_menus();
        let until = self.mute_for.until(self.now);
        #[cfg(test)]
        self.sent.push(Sent::MuteItem(id.clone(), scope, until, effect));
        let inbox = self.inbox.clone();
        Task::perform(
            async move { inbox.mute_item(id, scope, until, effect).await.map_err(|err| err.to_string()) },
            move |result| Message::MutedBy(result, effect),
        )
    }

    /// The ways to mute of the bell's menu as choices (picked by click or digit): those the core
    /// offered, each with its icon, then after a line – while the entry's whole source is muted
    /// (`inbox.mute_source`) – "Unmute <source>", which takes that back at once, and for a mail
    /// entry "Move to junk" (`mail.move_messages`, where its account has a junk folder), which was
    /// in mail's "Hide from Notification Hub" before (issue #123).
    fn mute_choices(&self, item: &Item) -> Vec<Choice> {
        let options = match &self.mute_options {
            Some(Ok(options)) => options.as_slice(),
            Some(Err(_)) | None => &[],
        };
        let mut choices: Vec<Choice> = options
            .iter()
            .map(|option| {
                let mute = Message::MuteBy(item.id.clone(), option.scope);
                Choice::new(mute_icon(option.scope), option.label.clone(), mute)
            })
            .collect();
        let mut rest = Vec::new();
        if self.is_muted(&item.source) {
            let unmute = trf("Unmute {source}", &[("source", &item.source.label())]);
            rest.push(Choice::new(Icon::BellOff, unmute, Message::AskMute(item.id.clone())));
        }
        // An entry's mail only: a message without an entry has its own "Delete" and "Move to …".
        let entry_mail = matches!(item.link, Some(Link::Mail { .. })) && !rules::is_message_id(&item.id);
        if entry_mail && self.junk_folder(&item.source).is_some() {
            rest.push(Choice::new(Icon::ShieldAlert, tr("Move to junk"), Message::Junk(item.id.clone())));
        }
        if let Some(first) = rest.first_mut() {
            first.line = !choices.is_empty();
        }
        choices.extend(rest);
        choices
    }

    /// Opens the menu "Hide from Notification Hub" for the entry with `id` (task E3.10). An entry with no way
    /// to hide it – an approval, which closes once decided and whose source is never muted – gets
    /// a note instead.
    fn ask_hide(&mut self, id: &str) {
        let Some(item) = self.find(id).cloned() else { return };
        if self.hide_choices(&item).is_empty() {
            self.note(tr("An approval closes once it is decided.").to_owned());
            return;
        }
        self.drawer = false;
        self.subject = Some(item);
        self.menu = Some(Menu::Hide);
    }

    /// The ways to hide `item` that is no mail, in the order of the menu (task E3.10): "This entry"
    /// (done) and "Mute <source>" / "Unmute <source>"; the suite is never muted ([`rules_allowed`]),
    /// an approval has no done, and done entries have no "This entry": they are out of the inbox
    /// already. Mail has none: it is muted through the bell's menu ([`App::mutes_by_bell`], issue
    /// #123), whose "This conversation", sender, domain and "Move to junk" replace this menu's.
    pub fn hide_choices(&self, item: &Item) -> Vec<Choice> {
        let id = || item.id.clone();
        let mut choices = Vec::new();
        if item.source.kind == SourceKind::Mail {
            return choices;
        }
        let open = !matches!(item.state, State::Done { .. }) && !detail::is_approval(item);
        if open {
            choices.push(Choice::new(Icon::Check, tr("This entry"), Message::SetDone(id(), true)));
        }
        if rules_allowed(&item.source) {
            let label = item.source.label();
            let mute = if self.is_muted(&item.source) {
                trf("Unmute {source}", &[("source", &label)])
            } else {
                trf("Mute {source}", &[("source", &label)])
            };
            choices.push(Choice::new(Icon::BellOff, mute, Message::AskMute(id())));
        }
        choices
    }

    /// The junk folder of the mail account `source`, if it has one.
    fn junk_folder(&self, source: &SourceRef) -> Option<&torro_mail::Folder> {
        let account = source.key();
        self.folders.iter().find(|folder| folder.account == account && folder.role == Some(Role::Junk))
    }

    /// "Move to junk" for the mail entry `id`: its newest mail to the account's junk folder
    /// (`mail.move_messages`), with an undo that moves it back to the inbox and opens the entry
    /// again – the core closes it once its mail left the inbox, as for archiving.
    fn move_to_junk(&mut self, id: &str) -> Task<Message> {
        self.close_menus();
        let Some(Link::Mail { message }) = self.find(id).and_then(|item| item.link.clone()) else {
            return Task::none();
        };
        let ids = vec![message];
        let edit = MailEdit::Move { ids: ids.clone(), to: MoveTarget::Role(MoveRole::Junk) };
        let back = MailEdit::Move { ids, to: MoveTarget::Role(MoveRole::Inbox) };
        let hint = Hint {
            text: tr("Moved to junk").to_owned(),
            undo: Some(Undo::Mail { edit: back, reopen: Some(id.to_owned()) }),
            shown: Instant::now(),
        };
        self.mail_edit(Some(edit), Some(hint))
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
        self.ai_suggesting = None;
        self.task = TaskDraft { title: list::one_line(&item.title), ..TaskDraft::default() };
        self.subject = Some(item);
        self.task_mail = None;
        self.open_task_dialog()
    }

    /// The message `id` the window holds: in the list of the view, else the one in the reader.
    fn message_summary(&self, id: &str) -> Option<MessageSummary> {
        let loaded = || match &self.letter {
            Some(Letter { loaded: Some(Ok(mail)), .. }) if mail.summary.id == id => Some(mail.summary.clone()),
            _ => None,
        };
        self.messages.iter().find(|message| message.id == id).cloned().or_else(loaded)
    }

    /// Opens the dialog that makes a task of the folder's message `id` (task E8.7b): its subject as
    /// title, the lists asked for anew, no due day yet – the dialog asks for one as for an entry
    /// (task E8.1b). A message with an entry goes through the entry, as `t` on it in "For me" does.
    fn ask_mail_task(&mut self, id: &str) -> Task<Message> {
        if let Some(entry) = self.entry_of(id) {
            return self.ask_task(&entry);
        }
        let Some(summary) = self.message_summary(id) else { return Task::none() };
        self.close_menus();
        // The dialog opens at the cursor's row, as for entries.
        self.cursor = Some(summary.id.clone());
        self.task = TaskDraft { title: mail::subject(&summary), ..TaskDraft::default() };
        self.subject = None;
        self.task_mail = Some(summary);
        self.open_task_dialog()
    }

    /// Shows the dialog of "Make task" and asks for the lists, the title field focused.
    fn open_task_dialog(&mut self) -> Task<Message> {
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
        if let Some(message) = self.task_mail.clone() {
            return self.create_mail_task(message);
        }
        let (Some(item), Some(list)) = (self.subject.clone(), self.task.list.clone()) else { return Task::none() };
        let title = Some(self.task.title.trim().to_owned()).filter(|title| !title.is_empty());
        let due = self.task.due.day(self.now);
        let notes = filled(&self.task.notes);
        self.last_list = Some(list.id.clone());
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::Task {
            id: item.id.clone(),
            list: list.clone(),
            title: title.clone(),
            due,
            notes: notes.clone(),
        });
        let inbox = self.inbox.clone();
        let TaskList { id: list_id, name } = list;
        Task::perform(
            async move {
                SuggestSource::create_task(&*inbox, item.id, list_id, title, due, notes)
                    .await
                    .map_err(|err| err.to_string())
            },
            move |result| Message::TaskCreated(result, name.clone()),
        )
    }

    /// Accept · Maybe · Decline on the invitation in the message `id` of `account` (task E8.3b):
    /// sends `mail.respond_to_invitation` with `answer`, unless an answer is on its way already
    /// ([`App::answering`]: the buttons take no second click). The core replies to the organizer and
    /// puts the event in a calendar; the window waits for its answer ([`App::invitation_answered`]).
    fn answer_invitation(&mut self, account: String, id: String, answer: InvitationAnswer) -> Task<Message> {
        if self.answering.is_some() {
            return Task::none();
        }
        self.answering = Some(id.clone());
        let collection = self.invite_calendar().map(|calendar| calendar.id.clone());
        #[cfg(test)]
        self.sent.push(Sent::Answer(account.clone(), id.clone(), answer, collection.clone()));
        let inbox = self.inbox.clone();
        let asked = id.clone();
        Task::perform(
            async move {
                inbox.respond_to_invitation(account, asked, answer, collection).await.map_err(|err| err.to_string())
            },
            move |result| Message::InvitationAnswered(id.clone(), answer, result),
        )
    }

    /// The calendar an answer to an invitation puts its event in (task E8.3c), as the card shows it:
    /// the one picked on the card, else the one used last for an event, else the first – the core's
    /// own choice without a copy made before. `None` while the calendars are not in (or could not
    /// be read): the core chooses then. A copy made before stays in its calendar whatever is sent.
    pub fn invite_calendar(&self) -> Option<&TaskList> {
        let Some(Ok(calendars)) = &self.invite_calendars else { return None };
        let by_id = |id: &Option<String>| calendars.iter().find(|calendar| Some(&calendar.id) == id.as_ref());
        by_id(&self.invite_pick).or_else(|| by_id(&self.last_calendar)).or(calendars.first())
    }

    /// What the card of the invitation `invite` in the message `id` needs besides it (task E8.3c),
    /// once the mail is read: the events of the shown calendars around it, for what it overlaps,
    /// and – the first time – the calendars to choose. Nothing for a mail without an invitation, one
    /// answered already or one whose event is over: the card shows neither then.
    fn ask_invite_side(&mut self, id: &str, invite: Option<&torro_mail::Invite>) -> Task<Message> {
        let Some(invite) = invite.filter(|invite| invite.answered.is_none() && !mail::invite_over(invite, self.now))
        else {
            return Task::none();
        };
        let (start, end) = mail::invite_span(invite);
        #[cfg(test)]
        self.sent.push(Sent::Events(id.to_owned(), start, end));
        let inbox = self.inbox.clone();
        let asked = id.to_owned();
        let events = Task::perform(
            async move { inbox.list_events(start, end).await.map_err(|err| err.to_string()) },
            move |result| Message::InviteEvents(asked.clone(), result),
        );
        if self.invite_calendars.is_some() {
            return events;
        }
        let inbox = self.inbox.clone();
        let calendars = async move { inbox.list_calendars().await.map_err(|err| err.to_string()) };
        Task::batch([events, Task::perform(calendars, Message::InviteCalendars)])
    }

    /// The core's answer to an invitation answered (task E8.3b): the answer kept on the message (in
    /// the list and the reader, also over later loads, [`keep_answer`]) and a hint that says
    /// what happened; the error line when it failed. Then the window loads again.
    ///
    /// The mail's open entry, wherever the window holds it, is marked done (task E8.3c,
    /// `inbox.set_item_done`), and the hint's "Undo" opens it again – only the entry: a sent answer
    /// cannot be taken back. An answer that may not have gone out ([`Delivery::Unknown`]) leaves
    /// the entry open, without undo.
    fn invitation_answered(
        &mut self,
        id: String,
        answer: InvitationAnswer,
        result: Result<Delivery, String>,
    ) -> Task<Message> {
        self.answering = None;
        let delivery = match result {
            Ok(delivery) => delivery,
            Err(err) => {
                self.error = Some(Failure::new(tr("Couldn't answer the invitation."), err));
                return Task::none();
            }
        };
        self.answers.retain(|(message, _)| *message != id);
        self.answers.push((id.clone(), answer));
        for message in self.messages.iter_mut().filter(|message| message.id == id) {
            if let Some(invite) = &mut message.invite {
                invite.answered = Some(answer);
            }
        }
        if let Some(Letter { loaded: Some(Ok(mail)), .. }) = self.letter.as_mut().filter(|letter| letter.id == id)
            && let Some(invite) = &mut mail.summary.invite
        {
            invite.answered = Some(answer);
        }
        let text = match (delivery, answer) {
            (Delivery::Unknown, _) => tr("The answer may not have gone out. Check your sent mail."),
            (_, InvitationAnswer::Accept) => tr("Accepted. The event is in your calendar."),
            (_, InvitationAnswer::Tentative) => tr("Answered maybe. The event is in your calendar."),
            (_, InvitationAnswer::Decline) => tr("Declined. The organizer was told."),
        };
        // Answered is dealt with (task E8.3c, scenario 5 in `docs/eingang.md`): the mail's entry is
        // done, and "Undo" opens the entry again – the answer is sent, nothing takes it back. An
        // answer that may not have gone out leaves the entry open, to check and answer again.
        // The entry wherever the window holds it: the list, "For me" (also beside a folder), the panel.
        let entry = self.detail.iter().chain(&self.items).chain(&self.for_me).find(|item| holds(item, &id));
        match entry.map(|item| item.id.clone()).filter(|_| delivery == Delivery::Accepted) {
            Some(entry) => {
                let undo = Some(Edit::Done { id: entry.clone(), done: false });
                self.send(Edit::Done { id: entry, done: true }, Some(After { text: text.to_owned(), undo }))
            }
            None => {
                self.note(text.to_owned());
                self.load()
            }
        }
    }

    /// "Create task" for a folder's message (task E8.7b): sends `tasks.create_todo_from_mail` with
    /// the dialog's list, title (none when blank: the core takes the subject) and due day (task
    /// E8.1b) and closes the dialog. The mail stays as it is; the reader shows the task's card once
    /// the answer is in.
    fn create_mail_task(&mut self, message: MessageSummary) -> Task<Message> {
        let Some(list) = self.task.list.clone() else { return Task::none() };
        let title = Some(self.task.title.trim().to_owned()).filter(|title| !title.is_empty());
        let due = self.task.due.day(self.now);
        self.last_list = Some(list.id.clone());
        self.close_menus();
        self.task_mail = None;
        let MessageSummary { id, account, .. } = message;
        #[cfg(test)]
        self.sent.push(Sent::MailTask {
            account: account.clone(),
            id: id.clone(),
            list: list.clone(),
            title: title.clone(),
            due,
        });
        let inbox = self.inbox.clone();
        let asked = id.clone();
        Task::perform(
            async move {
                inbox.create_todo_from_mail(account, asked, list, title, due).await.map_err(|err| err.to_string())
            },
            move |result| Message::MailTaskCreated(id.clone(), result),
        )
    }

    /// Opens the dialog that makes an event of the entry with `id` (task E8.5b): its title to change,
    /// the next half hour today for half an hour, the calendars asked for anew, the title field
    /// focused. An approval closes once it is decided; it becomes no event.
    fn ask_event(&mut self, id: &str) -> Task<Message> {
        let Some(item) = self.find(id).cloned() else { return Task::none() };
        if detail::is_approval(&item) {
            self.note(tr("An approval closes once it is decided.").to_owned());
            return Task::none();
        }
        self.drawer = false;
        self.ai_suggesting = None;
        self.event = EventDraft::new(list::one_line(&item.title), self.now);
        self.subject = Some(item);
        self.menu = Some(Menu::Event);
        self.calendars = None;
        #[cfg(test)]
        self.sent.push(Sent::Calendars);
        let inbox = self.inbox.clone();
        let calendars = async move { inbox.list_calendars().await.map_err(|err| err.to_string()) };
        Task::batch([Task::perform(calendars, Message::Calendars), operation::focus(EVENT_TITLE)])
    }

    /// The calendars for the open dialog: the one chosen is the one used last, else the first.
    fn take_calendars(&mut self, result: Result<Vec<TaskList>, String>) {
        if self.menu != Some(Menu::Event) {
            return;
        }
        if let Ok(calendars) = &result {
            let last = calendars.iter().find(|calendar| Some(&calendar.id) == self.last_calendar.as_ref());
            self.event.calendar = last.or(calendars.first()).cloned();
        }
        self.calendars = Some(result);
    }

    /// "Create event": sends `inbox.create_event_from_item` for the dialog's entry, calendar, title
    /// (none when blank: the core takes the entry's) and times, and closes the dialog. Nothing
    /// without a calendar.
    fn create_event(&mut self) -> Task<Message> {
        let (Some(item), Some(calendar)) = (self.subject.clone(), self.event.calendar.clone()) else {
            return Task::none();
        };
        let title = Some(self.event.title.trim().to_owned()).filter(|title| !title.is_empty());
        let (start, end) = self.event.span();
        let (location, notes) = (filled(&self.event.location), filled(&self.event.notes));
        self.last_calendar = Some(calendar.id.clone());
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::Event {
            id: item.id.clone(),
            calendar: calendar.clone(),
            title: title.clone(),
            start,
            end,
            location: location.clone(),
            notes: notes.clone(),
        });
        let inbox = self.inbox.clone();
        let TaskList { id: calendar_id, name } = calendar;
        let fields = EventFields { title, start, end, location, notes };
        Task::perform(
            async move {
                SuggestSource::create_event(&*inbox, item.id, calendar_id, fields).await.map_err(|err| err.to_string())
            },
            move |result| Message::EventCreated(result, name.clone()),
        )
    }

    /// Asks whether an AI is set up (issue #120, [`SuggestSource::ai_ready`]), for the AI buttons.
    fn fetch_ai_ready(&self) -> Task<Message> {
        let inbox = self.inbox.clone();
        Task::perform(async move { inbox.ai_ready().await }, Message::AiReady)
    }

    /// "AI task" (issue #120): the dialog of "Make task" for the entry `id`, as `t` opens it, and the
    /// AI asked for its suggestion, which fills it in once it is in ([`App::take_task_suggestion`]).
    /// Without an AI only a hint says to set one up (the buttons are off then anyway).
    fn suggest_task(&mut self, id: &str) -> Task<Message> {
        if !self.ai_ready {
            self.note(tr("Set up an AI first").to_owned());
            return Task::none();
        }
        let dialog = self.ask_task(id);
        if self.menu != Some(Menu::Task) {
            return dialog;
        }
        self.ai_suggesting = Some(Suggesting::Asking(id.to_owned()));
        #[cfg(test)]
        self.sent.push(Sent::SuggestTask(id.to_owned()));
        let (inbox, asked, id) = (self.inbox.clone(), id.to_owned(), id.to_owned());
        let suggested = Task::perform(
            async move { inbox.suggest_task(asked).await.map_err(|err| err.to_string()) },
            move |result| Message::TaskSuggested(id.clone(), result),
        );
        Task::batch([dialog, suggested])
    }

    /// "AI event" (issue #120): the dialog of "Make event" for the entry `id` and the AI asked, as
    /// [`App::suggest_task`] does.
    fn suggest_event(&mut self, id: &str) -> Task<Message> {
        if !self.ai_ready {
            self.note(tr("Set up an AI first").to_owned());
            return Task::none();
        }
        let dialog = self.ask_event(id);
        if self.menu != Some(Menu::Event) {
            return dialog;
        }
        self.ai_suggesting = Some(Suggesting::Asking(id.to_owned()));
        #[cfg(test)]
        self.sent.push(Sent::SuggestEvent(id.to_owned()));
        let (inbox, asked, id) = (self.inbox.clone(), id.to_owned(), id.to_owned());
        let suggested = Task::perform(
            async move { inbox.suggest_event(asked).await.map_err(|err| err.to_string()) },
            move |result| Message::EventSuggested(id.clone(), result),
        );
        Task::batch([dialog, suggested])
    }

    /// Whether the AI's answer for the entry `id` still has its dialog: the one asked for is open.
    fn awaits(&self, id: &str, menu: Menu) -> bool {
        self.menu == Some(menu) && self.ai_suggesting == Some(Suggesting::Asking(id.to_owned()))
    }

    /// The AI's task for the entry `id` into the open dialog (issue #120): title, due day and note;
    /// the line under the title says to check it. An answer for a dialog closed or opened anew
    /// meanwhile is dropped. Without one, the line says why and the dialog stays as it is.
    fn take_task_suggestion(&mut self, id: &str, result: Result<TaskSuggestion, String>) {
        if !self.awaits(id, Menu::Task) {
            return;
        }
        match result {
            Ok(suggestion) => {
                self.task.title = suggestion.title;
                self.task.due = ai::due_of(suggestion.due).map_or(Due::None, |day| Due::of_day(day, self.now));
                self.task.notes = suggestion.notes.unwrap_or_default();
                self.ai_suggesting = Some(Suggesting::Done);
            }
            Err(err) => self.ai_suggesting = Some(Suggesting::Failed(err)),
        }
    }

    /// The AI's event for the entry `id` into the open dialog (issue #120): title, day, start and
    /// length ([`ai::event_times`]; a suggestion without a day keeps the dialog's), place, and who
    /// takes part as its note. As [`App::take_task_suggestion`] otherwise.
    fn take_event_suggestion(&mut self, id: &str, result: Result<EventSuggestion, String>) {
        if !self.awaits(id, Menu::Event) {
            return;
        }
        match result {
            Ok(suggestion) => {
                if let Some((day, start, minutes)) = ai::event_times(&suggestion, self.event.length.minutes()) {
                    self.event.day = day;
                    self.event.start = StartTime(start);
                    self.event.length = Length::of_minutes(minutes);
                }
                self.event.notes = ai::attendees_note(&suggestion).unwrap_or_default();
                self.event.title = suggestion.title;
                self.event.location = suggestion.location.unwrap_or_default();
                self.ai_suggesting = Some(Suggesting::Done);
            }
            Err(err) => self.ai_suggesting = Some(Suggesting::Failed(err)),
        }
    }

    /// The quiet line under the title of a dialog the AI fills in (issue #120): asking, done – to
    /// check –, or why it gave nothing. `None` for a dialog opened without it.
    fn suggesting_line(&self) -> Option<Element<'_, Message>> {
        let (text, error) = match self.ai_suggesting.as_ref()? {
            Suggesting::Asking(_) => (tr("Asking your AI…").to_owned(), false),
            Suggesting::Done => (tr("Suggested by your AI. Check it before you create it.").to_owned(), false),
            Suggesting::Failed(why) => (trf("No suggestion: {why}", &[("why", why)]), true),
        };
        let mark = icon::icon(if error { Icon::CircleAlert } else { Icon::Sparkles }, icon::SIZE_SM).style(
            move |t: &Theme, _| iced::widget::svg::Style {
                color: Some(if error { palette(t).error_text } else { palette(t).text_3 }),
            },
        );
        let words = typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
        Some(row![mark, words].spacing(space::s()).align_y(Alignment::Center).into())
    }

    /// The task made of the message `id` (task E8.7b), for the card over its text: the one the entry
    /// in the panel carries when this is its mail, else the one of the entry holding the mail, else
    /// the one "Task" made of it in this window.
    fn task_of(&self, id: &str) -> Option<&torro_inbox::item::ItemTask> {
        let holds = |item: &&Item| matches!(&item.link, Some(Link::Mail { message }) if message == id);
        self.detail
            .iter()
            .chain(&self.items)
            .filter(holds)
            .find_map(|item| item.task.as_ref())
            .or_else(|| self.made.iter().find(|(message, _)| message == id).map(|(_, task)| task))
    }

    /// Opens the task `uid` in the tasks app (task E8.7b, "Open task"): through the core
    /// (`apps.open_app` with `task:<uid>`), which checks it and starts the app with it. The demo has
    /// no core: it starts the tasks app's demo on the task itself.
    fn open_task(&mut self, uid: &str) -> Task<Message> {
        self.close_menus();
        let target = format!("task:{uid}");
        #[cfg(test)]
        self.sent.push(Sent::Target(AppId::TASKS.binary.to_owned(), target.clone()));
        if self.launch.demo {
            let mut command = std::process::Command::new(torro_gui::source::sibling(AppId::TASKS.binary));
            command.arg("--demo").arg(format!("--task={uid}"));
            if let Some(now) = self.launch.now {
                command.arg("--now").arg(now.format("%Y-%m-%dT%H:%M").to_string());
            }
            return Task::perform(async move { mail::start(command) }, Message::AppOpened);
        }
        let suite = self.suite.clone();
        Task::perform(open_target(suite, AppId::TASKS.binary, target), Message::AppOpened)
    }

    /// Reply on the suite's "You finished a task from a mail" under the cursor (task E8.7b): the
    /// compose window for the mail of its waiting entry ([`ItemTask::mail_entry`]) when the window
    /// holds that entry, else the mail window on the task's mail (`apps.open_app` with the task's
    /// `mail:` link), where it can be answered.
    ///
    /// [`ItemTask::mail_entry`]: torro_inbox::item::ItemTask::mail_entry
    fn reply_follow_up(&mut self) -> Task<Message> {
        let Some(task) = self.cursor_item().and_then(|item| item.task.clone()) else { return Task::none() };
        let message = task.mail_entry.as_deref().and_then(|entry| self.items.iter().find(|item| item.id == entry));
        if let Some(Link::Mail { message }) = message.and_then(|item| item.link.clone()) {
            return self.mail_action(MailAction::Reply, message);
        }
        if task.link.is_empty() || self.launch.demo {
            self.note(tr("The mail is no longer in the Notification Hub.").to_owned());
            return Task::none();
        }
        #[cfg(test)]
        self.sent.push(Sent::Target(AppId::INBOX_MAIL.window_class.to_owned(), task.link.clone()));
        let suite = self.suite.clone();
        Task::perform(open_target(suite, AppId::INBOX_MAIL.window_class, task.link), Message::AppOpened)
    }

    /// "Mark mail done" on the suite's "You finished a task from a mail" `id` (task E8.7b): its mail
    /// entry done, then the question itself, one `inbox.set_item_done` each; the toast takes both
    /// back ([`Undo::Dismissed`]). Nothing for another entry.
    fn mark_mail_done(&mut self, id: &str) -> Task<Message> {
        let Some(item) = self.find(id).filter(|item| hub::is_follow_up(item)).cloned() else { return Task::none() };
        let mail = item.task.and_then(|task| task.mail_entry);
        let ids: Vec<String> = mail.into_iter().chain([item.id]).collect();
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::MailDone(ids.clone()));
        let inbox = self.inbox.clone();
        let closed = async move {
            let mut done = Vec::new();
            for id in ids {
                if let Err(err) = inbox.set_item_done(id.clone(), true).await {
                    return (done, Some(err.to_string()));
                }
                done.push(id);
            }
            (done, None)
        };
        Task::perform(closed, |(done, failed)| Message::MailDone(done, failed))
    }

    /// The mail of `--message` (task E8.7b), once the first entries are in: its entry in the panel
    /// when the window holds one (in the mail window's "For me"), else the mail is loaded to find
    /// its folder ([`App::show_start_mail`]). Only the mail window shows mail; the others ignore it.
    fn open_start_mail(&mut self) -> Task<Message> {
        if self.launch.kind != Kind::Mail {
            self.start_mail = None;
            return Task::none();
        }
        let Some(id) = self.start_mail.take() else { return Task::none() };
        let entry = self
            .items
            .iter()
            .find(|item| matches!(&item.link, Some(Link::Mail { message }) if *message == id))
            .cloned();
        if let Some(entry) = entry {
            self.cursor = Some(entry.id.clone());
            self.column = self.launch.kind.column_or_first(Column::of(&entry));
            self.detail = Some(entry);
            return Task::none();
        }
        #[cfg(test)]
        self.sent.push(Sent::Letter(id.clone()));
        let inbox = self.inbox.clone();
        let asked = id.clone();
        Task::perform(
            async move { inbox.get_mail(asked).await.map(Box::new).map_err(|err| err.to_string()) },
            move |result| Message::StartMail(id.clone(), result),
        )
    }

    /// The mail of `--message` without an entry, loaded: its folder becomes the view, with the mail
    /// under the cursor and in the reader – kept there ([`App::pinned_mail`]) though it may lie
    /// beyond the folder's first page. A mail that cannot be read says so.
    fn show_start_mail(&mut self, id: String, result: Result<Box<Mail>, String>) -> Task<Message> {
        let mail = match result {
            Ok(mail) => *mail,
            Err(err) => {
                self.error = Some(Failure::new(tr("Couldn't open the mail."), err));
                return Task::none();
            }
        };
        let Some(folder) = self.folders.iter().find(|folder| folder.id == mail.summary.folder) else {
            self.error = Some(Failure::new(tr("Couldn't open the mail."), tr("Its folder is not listed.")));
            return Task::none();
        };
        self.view = View::Folder(FolderView::of(folder));
        self.detail = None;
        self.cursor = Some(id.clone());
        self.reading = true;
        self.pinned_mail = Some(id.clone());
        // Its invitation's calendars and overlaps (task E8.3c), as for a mail read from the list.
        let invite = self.ask_invite_side(&id, mail.summary.invite.as_ref());
        self.letter = Some(Letter { id, loaded: Some(Ok(mail)) });
        Task::batch([self.load(), invite])
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

    /// Whether the toast offers "Undo" now.
    fn can_undo(&self) -> bool {
        self.hint.as_ref().is_some_and(|hint| hint.undo.is_some())
    }

    /// "Undo": sends the undo of the hint, which then goes. Nothing while no hint can be undone.
    fn undo(&mut self) -> Task<Message> {
        let Some(undo) = self.hint.take().and_then(|hint| hint.undo) else { return Task::none() };
        self.close_menus();
        match undo {
            Undo::Edit(edit) => self.send(edit, None),
            Undo::Mute(source, muted) => self.mute(source, muted, false),
            Undo::Mail { edit, reopen } => {
                #[cfg(test)]
                self.sent.push(Sent::Mail(edit.clone(), None));
                // Coming back: the load after the move back shows them again.
                let ids = edit.ids().to_vec();
                self.leaving.retain(|leaving| !ids.contains(&leaving.id));
                // First the mail back into its folder, then the entry open again: the other way
                // round the core could close the entry once more while the mail is still away.
                let inbox = self.inbox.clone();
                let back = async move {
                    edit.apply(inbox.clone()).await?;
                    if let Some(id) = reopen {
                        inbox.set_item_done(id, false).await.map_err(|err| err.to_string())?;
                    }
                    Ok(())
                };
                Task::perform(back, move |result| Message::MailEdited(result, ids.clone(), None))
            }
            Undo::Muted { rule, reopen, moved } => {
                #[cfg(test)]
                self.sent.push(Sent::Unmute(rule.clone(), reopen.clone(), moved.clone()));
                // First the rule, then the entries, as for hiding: else the rule could close or move
                // them once more.
                let back = reopen.first().or(moved.first()).cloned();
                let inbox = self.inbox.clone();
                let undone = async move {
                    if let Some(id) = rule {
                        inbox.remove_rule(id).await.map_err(|err| err.to_string())?;
                    }
                    for id in reopen {
                        inbox.set_item_done(id, false).await.map_err(|err| err.to_string())?;
                    }
                    for id in moved {
                        inbox.set_item_lane(id, Lane::ForMe, false).await.map_err(|err| err.to_string())?;
                    }
                    Ok(())
                };
                Task::perform(undone, move |result| Message::Undone(result, back.clone()))
            }
            Undo::Dismissed(reopen) => {
                #[cfg(test)]
                self.sent.push(Sent::Reopen(reopen.clone()));
                let back = reopen.first().cloned();
                let inbox = self.inbox.clone();
                let undone = async move {
                    for id in reopen {
                        inbox.set_item_done(id, false).await.map_err(|err| err.to_string())?;
                    }
                    Ok(())
                };
                Task::perform(undone, move |result| Message::Undone(result, back.clone()))
            }
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
        if self.menu == Some(Menu::Move) {
            return self.move_choices();
        }
        let Some(item) = &self.subject else { return Vec::new() };
        let id = || item.id.clone();
        let choice = |icon, label: String, message| Choice::new(icon, label, message);
        match self.menu {
            Some(Menu::Snooze) => {
                let [later, tomorrow, next_week] = snooze_times(self.now);
                let snooze = |icon, label: &str, until| Choice {
                    when: Some(wake_short(until, self.now)),
                    ..Choice::new(icon, label, Message::Edit(Edit::Snooze { id: id(), until: Some(until) }))
                };
                let mut choices = vec![
                    snooze(Icon::Clock, tr("Later today"), later),
                    snooze(Icon::Sun, tr("Tomorrow"), tomorrow),
                    snooze(Icon::CalendarDays, tr("Next week"), next_week),
                    choice(Icon::CalendarClock, tr("Pick…").to_owned(), Message::ShowPicker),
                ];
                // A reminder of an event comes back soon (task ER1.4): snoozing its entry snoozes
                // the reminder, so the event is not missed.
                if item.kind == torro_inbox::Kind::Reminder {
                    choices.insert(
                        0,
                        snooze(Icon::Clock, tr("In 5 min"), self.now + TimeDelta::minutes(REMINDER_SNOOZE_MINUTES)),
                    );
                }
                if matches!(item.state, State::Snoozed { .. }) {
                    let wake = Message::Edit(Edit::Snooze { id: id(), until: None });
                    choices.push(choice(Icon::Inbox, tr("Bring back now").to_owned(), wake));
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
            Some(Menu::Hide) => self.hide_choices(item),
            Some(Menu::Mute) => self.mute_choices(item),
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

    /// "Compact | Comfortable" (task E8.2): the window shows its entries in `density` from now on,
    /// and remembers it for its key ([`Launch::density_key`]) for the next start – not in the
    /// demo, which stores nothing.
    fn set_density(&mut self, density: Density) -> Task<Message> {
        self.density = Some(density);
        if self.launch.demo {
            return Task::none();
        }
        ui_state::save_density_task(INBOX, self.launch.density_key(), density)
    }

    /// "Newest first | Oldest first" (task E8.15): the window lists in `order` from now on – asked
    /// anew, since a folder's next pages and an entry list's `limit` follow the order –, and
    /// remembers it for its key ([`Launch::density_key`]) – not in the demo, which stores nothing.
    fn set_order(&mut self, order: Order) -> Task<Message> {
        self.order = order;
        self.order_chosen = true;
        self.more = None;
        let load = self.load();
        if self.launch.demo {
            return load;
        }
        Task::batch([load, ui_state::save_order_task(INBOX, self.launch.density_key(), order)])
    }

    /// Whether the header offers "Newest first | Oldest first" (task E8.15): where the window lists
    /// entries or messages by arrival – not on the hub's "Act now" and Insights, which are sorted by
    /// what to do, nor in "Invitations", sorted by the event.
    fn orderable(&self) -> bool {
        !self.is_hub() && self.view != View::Invitations
    }

    /// The density the window shows its entries in (task E8.2): the one chosen (`--density`, the
    /// switch, `ui.json`), else the layout's ([`Density::default_for`]: compact when narrow).
    /// Mini and strip are glances and ignore it.
    pub fn density(&self) -> Density {
        self.density.unwrap_or_else(|| Density::default_for(self.class()))
    }

    /// How the lists draw mail (task E8.3): cards in the comfortable density of the mail window –
    /// their buttons carry their words as far as the card has room (task E8.11), in every layout –,
    /// rows otherwise. The notifications window draws its own cards (task E8.4, `notifications.rs`);
    /// the hub keeps its rows until E8.5.
    pub fn look(&self) -> Look {
        if self.launch.kind != Kind::Mail || self.density() != Density::Comfortable {
            return Look::Rows;
        }
        Look::Cards
    }

    /// The width the mail window's cards get (task E8.11; their previews take the lines they need at
    /// it): the list's width less the cards' inset – in the wide layout the list beside the reader
    /// (as wide as its divider says, [`App::split_limits`]), else the window less the rail with the inset beside the content (as
    /// [`App::board_width`]) – the whole of it, as the list without a reader fills it (issue #129,
    /// [`App::list_cap`]). An estimate from the window's size, because the keys need the cards'
    /// heights as well as the drawing; a few points off only move the scrolling a little.
    fn card_width(&self) -> f32 {
        let class = self.class();
        let list = if class == SizeClass::Wide {
            self.split.width(self.split_limits())
        } else {
            let room = self.content_width();
            match self.list_cap(1) {
                Some(cap) if self.view == View::Overview => room.min(cap),
                _ => room,
            }
        };
        (list - list::cards_side()).max(0.0)
    }

    /// How wide the list of `columns` columns may grow: [`overview::width`] – lists of entries stay
    /// readable, more room goes to what stands beside them. `None` in the mail window (issue #129):
    /// its list fills the room it has – beside the reader its column, without one the whole window –
    /// instead of ending in an empty strip that reads as broken.
    fn list_cap(&self, columns: usize) -> Option<f32> {
        (self.launch.kind != Kind::Mail).then(|| overview::width(columns))
    }

    /// The width of the content beside the sidebar or rail and its inset: the room of the list (and
    /// of the reader beside it). An estimate from the window's size, as [`App::card_width`] needs it.
    fn content_width(&self) -> f32 {
        let chrome = match shell::sidebar_mode(self.class(), self.sidebar) {
            Some(SidebarMode::Expanded) => size::sidebar() + layout::inset(),
            Some(SidebarMode::Rail) => shell::rail() + layout::inset(),
            None => 0.0,
        };
        self.size.width - chrome
    }

    /// What the divider between the wide mail window's list and reader moves in (issue #130): the
    /// list [`size::list()`] wide by default, in the content's width ([`App::content_width`]); the
    /// split keeps both at least `split::min_pane()`.
    fn split_limits(&self) -> Limits {
        Limits::new(size::list(), self.content_width())
    }

    /// What the narrow mail window's "For me" leads with (task E8.3, "N new for you"; task E8.11 in
    /// both densities): the number of mails for the user and their newest senders ("From Anna, Ben,
    /// Chris"). `None` elsewhere, and while nothing is for the user (the calm line says so).
    fn lead(&self) -> Option<(usize, String)> {
        let narrow = self.class() == SizeClass::Narrow;
        if self.launch.kind != Kind::Mail || !narrow || self.view != View::Overview {
            return None;
        }
        let count = Sections::of(&self.items, Column::Mail, self.now).for_me();
        let senders = glance::latest_senders(&self.for_me);
        let senders = if senders.is_empty() { String::new() } else { glance::senders_text(&senders) };
        (count > 0).then_some((count, senders))
    }

    /// The reader beside the list (task E8.3): in the wide mail window, for the mail open in the
    /// panel – a folder's message, or the mail of an entry, whose Done, Later and Task it carries.
    /// `None` elsewhere, and for an entry without a mail: the panel shows those.
    fn wide_reader<'a>(&'a self, class: SizeClass, anchors: &detail::Anchors) -> Option<Element<'a, Message>> {
        if self.launch.kind != Kind::Mail || class != SizeClass::Wide || !self.panel_open() {
            return None;
        }
        let reading = self.reading()?;
        let entry = match self.view.is_mail() {
            true => None,
            false => {
                let item = self.detail.as_ref()?;
                Some(mail::ReaderEntry { item, hide: self.hidable(item), ai: self.ai_ready })
            }
        };
        Some(mail::reader(reading, entry, anchors.clone(), self.density() == Density::Comfortable))
    }

    /// The reader's column of the wide mail window while no mail is open (task E8.11,
    /// `mail::empty_reader`): the list keeps the width it has beside a mail (its divider's, issue #130) and
    /// its scrollbar stands at its edge, instead of a wide empty space beside it. `None` elsewhere,
    /// while a panel is open, and before the first load (the loading or error state fills the window).
    fn empty_reader<'a>(&self, class: SizeClass) -> Option<Element<'a, Message>> {
        let shown = self.launch.kind == Kind::Mail && class == SizeClass::Wide && self.loaded;
        (shown && !self.panel_open()).then(mail::empty_reader)
    }

    /// The layout class of the window.
    fn class(&self) -> SizeClass {
        layout::of(self.size)
    }

    /// The room under the title band, which the glances fill ([`shell::below_titlebar`]).
    fn content_size(&self) -> Size {
        shell::below_titlebar(self.size, self.zoom.factor())
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
                self.cover(),
                TitleButtons::none(),
                Message::DragWindow,
            ));
        }
        // Always three layers, so the window keeps its state when the hint or the question comes and
        // goes.
        let window = stack![self.window(class), self.hint_layer(), self.question_layer()].width(Fill).height(Fill);
        let (surface, cover) = (Surface::of(class), self.cover());
        let content = self.zoom.view(window);
        // The panel button and "Settings" in the band above the window (U.10).
        let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
        let buttons = TitleButtons::none().sidebar(label, Message::ToggleSidebar).settings(Message::OpenSettings);
        widgets::ctrl_guard(shell::titlebar(content, self.zoom.factor(), surface, cover, buttons, Message::DragWindow))
    }

    /// What lies over the top of the window, which the title band repeats: the panel of an entry
    /// over the list, else the drawer. A glance has neither.
    fn cover(&self) -> Cover {
        let class = self.class();
        if self.is_glance() {
            return Cover::Nothing;
        }
        let question = self.menu == Some(Menu::DeleteForGood);
        Cover::dialog(question).or(Cover::panel(class, self.panel_open())).or(Cover::drawer(class, self.drawer))
    }

    /// The question "Delete permanently?" (task E10.1) as a dialog over the window – dimmed, "Cancel"
    /// and "Delete" (danger), a click beside it cancels, as the other questions of the suite do;
    /// empty while it is not asked.
    fn question_layer(&self) -> Element<'_, Message> {
        if self.menu != Some(Menu::DeleteForGood) {
            return Space::new().into();
        }
        let actions = row![
            Space::new().width(Fill),
            // Weighted as the other dialogs' ("Make task"): a quiet "Cancel", the action stands out.
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseMenu)),
            widgets::text_button(None, tr("Delete"), style::button::danger, Some(Message::DeleteForGood)),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let content = column![
            typo::text(typo::title(), tr("Delete permanently?")),
            typo::text(typo::body(), tr("The message is removed for good. This can't be undone.")).style(theme::text_2),
            actions,
        ]
        .spacing(space::l());
        let card = container(content).padding(space::xl()).width(Fill).style(style::container::popover);
        widgets::modal(Space::new().width(Fill).height(Fill), card, widgets::POPOVER_WIDTH, Message::CloseMenu)
    }

    /// The toast at the bottom centre (`widgets::toast`, task E3.11), over everything but the zoom
    /// badge; empty without a hint.
    fn hint_layer(&self) -> Element<'_, Message> {
        let Some(hint) = &self.hint else { return Space::new().into() };
        widgets::toast(hint.text.as_str(), hint.undo.is_some().then(|| ToastAction::undo(Message::Undo)))
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
            // Only the mail window reads mail: the notifications window shows none (task E8.4), the
            // hub acts on it (task E8.5), so their glances have no block of unread mail.
            folders: if self.launch.kind == Kind::Mail { &self.folders } else { &[] },
            now: self.now,
            error: self.error.as_ref().map(|failure| failure.what),
            // Its numbers count mail: "From …" right under them (task E8.11).
            mail: self.launch.kind == Kind::Mail,
        };
        match class {
            SizeClass::Strip => glance::strip(&data, self.content_size()),
            _ => glance::mini(&data, self.content_size()),
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
        // Two sets (issue #122): the buttons of the panel or the reader (`anchors`), and the row or
        // card under the cursor with its buttons (`rows`). A menu hangs at the button that opened it.
        let (anchors, rows) = (detail::Anchors::default(), detail::Anchors::default());
        let from_panel = self.menu_from_panel();
        let at = |pick: fn(&detail::Anchors) -> &Anchor| menu_anchor(from_panel, &anchors, &rows, pick);
        let (snooze_at, lane_at, task_at, event_at, hide_at) =
            (at(|a| &a.snooze), at(|a| &a.lane), at(|a| &a.task), at(|a| &a.event), at(|a| &a.hide));
        let below = Placement::Below(Align::End);
        let dialog = dialog_placement(from_panel);
        let layer = match self.menu {
            None => None,
            Some(Menu::Switcher) => Some(widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(self.launch.kind.app(), Message::Switcher),
                Message::CloseMenu,
            )),
            Some(Menu::Nav) => {
                Some(widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Message::CloseMenu))
            }
            Some(Menu::Snooze) => {
                Some(widgets::floating(&snooze_at, below, self.choice_menu(tr("Later until")), Message::CloseMenu))
            }
            Some(Menu::Lane) => {
                let title = self.subject.as_ref().map_or_else(String::new, |item| {
                    trf("Move to {lane}", &[("lane", &lane_name(other_lane(item.lane)))])
                });
                Some(widgets::floating(&lane_at, below, self.choice_menu(title), Message::CloseMenu))
            }
            Some(Menu::Pick) => Some(widgets::floating(&snooze_at, below, self.picker(), Message::CloseMenu)),
            Some(Menu::Task) => Some(widgets::floating(&task_at, dialog, self.task_dialog(), Message::CloseMenu)),
            Some(Menu::Event) => Some(widgets::floating(&event_at, dialog, self.event_dialog(), Message::CloseMenu)),
            Some(Menu::Hide) => Some(widgets::floating(
                &hide_at,
                below,
                self.choice_menu(tr("Hide from Notification Hub")),
                Message::CloseMenu,
            )),
            Some(Menu::Mute) => Some(widgets::floating(&hide_at, below, self.mute_menu(), Message::CloseMenu)),
            // At the bell (in the reader's Torro bar, task E9.4; above it when there is no room
            // below), else at the cursor's row.
            // In the panel it opens to the right of the bell, like "Make task": aligned at its end the
            // menu ran out of the panel over the list.
            Some(Menu::Move) => {
                Some(widgets::floating(&hide_at, dialog, self.choice_menu(tr("Move to folder")), Message::CloseMenu))
            }
            // A question over the whole window ([`App::question_layer`]), not at a button.
            Some(Menu::DeleteForGood) => None,
        };
        // The wide mail window reads beside the list (task E8.3), without the panel; while nothing is
        // open the reader's column says what shows up there (task E8.11).
        if let Some(reader) = self.wide_reader(class, &anchors).or_else(|| self.empty_reader(class)) {
            // The divider between them drags (issue #130).
            let width = self.split.width(self.split_limits());
            let body = split::split(self.body(class, &rows), reader, width, self.split.dragging, Message::Split);
            let placed = panel::place(class, body, None, layer);
            return shell::shell(class, sidebar, header, placed.body, None)
                .rail(rail)
                .drawer(self.drawer, Message::CloseMenu)
                .overlay(placed.overlay)
                .into();
        }
        let detail = if self.view.is_mail() {
            // A folder's message: the mail alone, no entry around it.
            self.reading().filter(|_| self.reading).map(|reading| mail::panel(&reading, self.view.folder(), &anchors))
        } else {
            self.detail.as_ref().map(|item| {
                // The notifications window's panel and mail have the bell where the others have "…"
                // (task E8.6b, issue #123).
                let footer = detail::Footer {
                    anchors: anchors.clone(),
                    hide: self.hidable(item),
                    bell: self.mutes_by_bell(item),
                    deciding: self.deciding.is_some(),
                    // The panel fills narrow windows: no room for the words of "Move to …".
                    compact: !matches!(class, SizeClass::Wide | SizeClass::Medium),
                    ai: self.ai_ready,
                    // A mail's switches and "Copy text" in the footer, as in the reader's Torro bar
                    // (task E9.4).
                    view: self.reading().map(|reading| reading.view_tools()),
                };
                // A mail entry shows its whole mail in place of the entry's short text.
                let letter = self.reading().map(|reading| mail::body(&reading));
                detail::panel(item, self.view_label(), self.now, footer, letter, &self.text)
            })
        };
        let placed = panel::place(class, self.body(class, &rows), detail, layer);
        // "Coming back" beside the hub's "Act now" in the wide layout (task E8.5), in the shell's
        // column at the right.
        let side = self.hub_side().then(|| {
            let handed = hub::handed_off(&self.items, &self.done);
            hub::side(&self.snoozed, handed, self.quiet.as_ref(), self.now, self.cursor.as_deref())
        });
        shell::shell(class, sidebar, header, placed.body, side)
            .rail(rail)
            .drawer(self.drawer, Message::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    /// The header: the view's title (wide with the full sidebar) or the navigation menu (with the
    /// rail, medium, narrow), its count, and
    /// "Refresh". The inbox creates nothing, so there is no primary button.
    fn header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Message> {
        if self.is_insights() {
            return self.insights_header(class, nav);
        }
        let kind = self.launch.kind;
        let icon = if self.is_hub() && self.sorting { Icon::Inbox } else { kind.view_icon(&self.view) };
        let name = self.view_label();
        // Nothing listed: the empty state says so, a "0" beside the title would only repeat it. The
        // narrow hub says its numbers big on the page (task E8.5), and the header has no room.
        let narrow_hub = self.is_hub() && !matches!(class, SizeClass::Wide | SizeClass::Medium);
        let note = (self.loaded && !self.lists_nothing() && !narrow_hub)
            .then(|| self.count_note())
            .filter(|note| !note.is_empty());
        let mut trailing = Vec::new();
        // The overview shows one column below the wide layout: "Mail | Notifications" chooses it
        // where a window has two columns – none since the hub has sections (task E8.5); the mail and
        // the notifications window have one column only.
        let columns = self.launch.kind.columns();
        let choose_column =
            self.view == View::Overview && !self.is_hub() && class != SizeClass::Wide && columns.len() > 1;
        if choose_column {
            let chosen = columns.iter().position(|&column| column == self.column).unwrap_or_default();
            let columns = columns.iter().map(|&column| (column.name(), Message::ShowColumn(column)));
            trailing.push(widgets::segmented(columns, chosen));
        }
        // "By app | By time" (task E8.4) where the notifications window's board has room for columns.
        let arrange =
            self.is_board() && !self.items.is_empty() && notifications::has_arrange(self.board_width(), class);
        if arrange {
            let chosen = Arrange::ALL.iter().position(|&arrange| arrange == self.arrange).unwrap_or_default();
            trailing.push(widgets::segmented(
                Arrange::ALL.map(|arrange| (arrange.name(), Message::Arrange(arrange))),
                chosen,
            ));
        }
        // "Compact | Comfortable" in every window kind (task E8.2), with its words where there is
        // room: wide, and medium unless "Mail | Notifications" or "By app | By time" takes the room;
        // narrow icons only.
        let words = class == SizeClass::Wide || (class == SizeClass::Medium && !choose_column && !arrange);
        // "Newest first | Oldest first" (task E8.15) where the window lists by arrival, with its word
        // where the density switch has its words.
        if self.orderable() {
            trailing.push(order_button(self.order, words));
        }
        trailing.push(widgets::density_switch(self.density(), words, Message::SetDensity));
        // "Dismiss all" in "New" of the notifications window (task E8.4), with its word in the wide
        // layout; off while nothing can be dismissed.
        if self.is_board() && self.view == View::Overview {
            let dismiss = (!notifications::dismiss_all(&self.items).is_empty()).then_some(Message::DismissAll);
            trailing.push(if class == SizeClass::Wide {
                widgets::text_button(Some(Icon::X), tr("Dismiss all"), style::button::ghost, dismiss)
            } else {
                widgets::icon_button(Icon::X, tr("Dismiss all"), dismiss)
            });
        }
        trailing.push(widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh)));
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
                    title = title.push(typo::text(typo::body(), note).style(theme::text_3).wrapping(Wrapping::None));
                }
                // The rail at the left has the panel button and the window's mark (issue #127).
                shell::header_bar([], title, trailing)
            }
            // Where the band has the panel button (macOS), the header leaves it out.
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
                let mut title =
                    row![widgets::menu_button(nav, Some(icon), name, Message::ToggleNav)].align_y(Alignment::Center);
                // The notifications window counts its view in every size ("New 5", task E8.12); the
                // mail window's narrow "For me" says it big on the page, the hub has no room.
                if let Some(note) = note.filter(|_| narrow_count(self.launch.kind)) {
                    title = title.push(typo::text(typo::body(), note).style(theme::text_3).wrapping(Wrapping::None));
                }
                shell::header_bar(self.narrow_leading(), title, trailing)
            }
        }
    }

    /// The header of Insights (task E8.8b): its title with the span counted ("7 Oct – 13 Oct"), then
    /// "Week | Month | Quarter" and "Table view" where there is room (wide, medium; narrow has them
    /// at the top of the page) and "Refresh". No density switch: Insights has no rows.
    fn insights_header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Message> {
        let name = self.view_label();
        let note = match &self.insights.stats {
            Some(Ok(stats)) => Some(insights::span_text(stats)),
            _ => None,
        };
        let mut trailing = Vec::new();
        if matches!(class, SizeClass::Wide | SizeClass::Medium) {
            trailing.extend(insights::controls(&self.insights));
        }
        trailing.push(widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::Refresh)));
        let rail = shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail);
        let icon = Some(Icon::ChartColumn);
        match class {
            SizeClass::Wide if !rail => shell::header_bar([], shell::header_title(icon, name, note), trailing),
            SizeClass::Wide | SizeClass::Medium => {
                let mut title =
                    row![widgets::menu_button(nav, icon, name, Message::ToggleNav)].align_y(Alignment::Center);
                if let Some(note) = note {
                    title = title.push(typo::text(typo::body(), note).style(theme::text_3).wrapping(Wrapping::None));
                }
                shell::header_bar([], title, trailing)
            }
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                self.narrow_leading(),
                widgets::menu_button(nav, icon, name, Message::ToggleNav),
                trailing,
            ),
        }
    }

    /// The start of the narrow header: the drawer button where the window has no band (the band has
    /// it on macOS), then the window's mark (issue #127, [`shell::app_mark`]): without sidebar or rail
    /// nothing else says whether this is the hub, mail or the app feed.
    fn narrow_leading<'a>(&self) -> Vec<Element<'a, Message>> {
        let toggle = (!shell::has_band()).then(|| shell::sidebar_toggle(Message::ShowDrawer));
        toggle.into_iter().chain([shell::app_mark(self.launch.kind.app())]).collect()
    }

    /// The quiet number after the title: how many entries the view lists. The title already says
    /// which view it is, so the number stands alone (as in torrotasks' header).
    ///
    /// A folder counts all its messages (a page shows only the newest); a search counts what it
    /// found so far.
    ///
    /// The hub's "Act now" (task E8.5) says what is in its sections ("1 to decide · 3 agents · 9 to
    /// sort", [`hub::summary`]), its To sort alone how many there are to sort.
    fn count_note(&self) -> String {
        if self.is_hub() {
            let sections = hub::Sections::of(&self.items, None);
            return if self.sorting { sections.to_sort.len().to_string() } else { hub::summary(&sections) };
        }
        match self.view.folder() {
            Some(folder) => {
                let all = self.folders.iter().find(|f| f.id == folder.id).map(|f| f.total as usize);
                all.unwrap_or(self.messages.len()).to_string()
            }
            // A search counts what the core says it found in all, else what it shows (issue #136).
            None if self.view.search().is_some_and(|search| !search.query.is_empty()) => {
                self.found.total.map_or_else(|| self.messages.len().to_string(), |total| total.to_string())
            }
            None if matches!(self.view, View::Attachments | View::Invitations) => self.messages.len().to_string(),
            None if self.view.is_mail() => String::new(),
            // As in the sidebar: what is for the user; "Info" lies folded. The notifications window
            // counts all it shows in "New" (task E8.4): its cards show both lanes.
            None if self.view == View::Overview && self.launch.kind != Kind::Notifications => {
                self.for_me.len().to_string()
            }
            None => self.items.len().to_string(),
        }
    }

    /// Whether the view lists nothing (the empty state says so; no "0" beside the title).
    fn lists_nothing(&self) -> bool {
        if self.is_hub() {
            return hub::Sections::of(&self.items, None).order(self.sorting).is_empty();
        }
        if self.view.is_mail() { self.messages.is_empty() } else { self.items.is_empty() }
    }

    /// The panel's view of the mail held ([`App::letter`]): headers from the list while the whole
    /// of it loads, links in the text colour, the flag in the attention colour.
    fn reading(&self) -> Option<Reading<'_>> {
        let letter = self.letter.as_ref()?;
        let colours = palette(&self.theme());
        let summary = self.messages.iter().find(|message| message.id == letter.id);
        let account = match &letter.loaded {
            Some(Ok(mail)) => Some(mail.summary.account.as_str()),
            _ => summary.map(|summary| summary.account.as_str()),
        };
        let accounts = mail::by_account(&self.folders);
        Some(Reading {
            id: &letter.id,
            summary,
            loaded: letter.loaded.as_ref(),
            now: self.now,
            text: &self.text,
            attention: colours.attention_text,
            in_trash: self.in_trash(&letter.id),
            dated: self.view.is_mail(),
            read: self.look().cards(),
            account: account.and_then(|account| accounts.iter().position(|group| group.account == account)),
            task: self.task_of(&letter.id),
            answering: self.answering.as_deref() == Some(letter.id.as_str()),
            invite: mail::InviteSide {
                calendars: match &self.invite_calendars {
                    Some(Ok(calendars)) => Some(calendars),
                    _ => None,
                },
                calendar: self.invite_calendar(),
                events: match &self.invite_events {
                    Some((id, Ok(events))) if *id == letter.id => Some(events),
                    _ => None,
                },
            },
            html: self.html_for_letter().map(|content| mail::HtmlReading {
                content,
                format: self.mail_format,
                scheme: self.mail_scheme(),
                scale: self.html_scale(),
            }),
        })
    }

    /// The content below the header: an error line if there is one, and the list – left-aligned
    /// under the header's title (design: "links mit Seitenleiste"); in the wide layout at most
    /// `CONTENT_MAX` wide, also while the panel stands beside it (its column takes its own room,
    /// and a narrower list would only leave a gap). Loading and the error of a first load stand in
    /// for both.
    fn body(&self, class: SizeClass, rows: &detail::Anchors) -> Element<'_, Message> {
        if let Some(unloaded) = self.unloaded() {
            return unloaded;
        }
        let attention = palette(&self.theme()).attention_text;
        let mut content = column![];
        // The search stands above the error line: it belongs to the mail, the error to the list.
        if self.shows_search() {
            content = content.push(self.search_bar());
        }
        if let Some(failure) = &self.error {
            content = content.push(container(failure_line(failure)).padding(Padding::new(space::s()).left(space::l())));
        }
        let rows = if self.view.is_mail() {
            // Another folder's messages are still held until this one's come: show loading.
            let listing = self.view.listing();
            if listing.is_some() && self.listed != listing {
                widgets::loading()
            } else {
                mail::list(mail::MailList {
                    messages: &self.messages,
                    view: &self.view,
                    no_accounts: self.folders.is_empty(),
                    selected: self.cursor.as_deref(),
                    now: self.now,
                    anchors: rows.clone(),
                    accounts: mail::by_account(&self.folders)
                        .into_iter()
                        .map(|group| mail::address(group.account))
                        .collect(),
                    more: self.more.is_some(),
                    attention,
                    look: self.look(),
                    // An invitation's card offers its entry's Later and bell (task E8.11).
                    entries: &self.for_me,
                    folders: &self.folders,
                    highlights: &self.highlights,
                    text: palette(&self.theme()).text,
                })
            }
        } else if self.is_board() {
            // The notifications window (task E8.4): the sources as chips where the sidebar is folded
            // away, then the board, which caps its own width.
            if shell::sidebar_mode(class, self.sidebar) != Some(SidebarMode::Expanded) {
                // On the board's left axis: the rows' text, or the cards' edge (task E8.12).
                let flush = self.board().flush(self.density());
                content = content.push(notifications::source_chips(&self.listed_sources(), &self.shown, flush));
            }
            let board = notifications::view(notifications::BoardData {
                items: &self.items,
                board: self.board(),
                density: self.density(),
                rows: self.rows_data(rows),
                deciding: self.deciding.is_some(),
                single_source: self.shown.single().is_some(),
                icon: self.launch.kind.view_icon(&self.view),
                sources: &self.sources,
                attention: palette(&self.theme()).attention,
                sort: self.order,
            });
            return container(content.push(board).width(Fill).height(Fill)).width(Fill).into();
        } else if self.is_insights() {
            // The hub's Insights (task E8.8b), which caps its own width.
            let page = insights::view(InsightsData { insights: &self.insights, class, today: local_today(self.now) });
            return container(content.push(page).width(Fill).height(Fill)).width(Fill).into();
        } else if self.is_hub() {
            // The hub's "Act now" (task E8.5), which caps its own width.
            let colours = palette(&self.theme());
            let hub = hub::view(hub::HubData {
                items: &self.items,
                filter: self.filter.as_ref(),
                layout: self.hub_layout(),
                rows: self.rows_data(rows),
                deciding: self.deciding.is_some(),
                attention: colours.attention,
                ok: colours.ok,
                quiet: self.quiet.as_ref(),
            });
            return container(content.push(hub).width(Fill).height(Fill)).width(Fill).into();
        } else if self.view == View::Overview {
            // Its columns cap their own width; the cap below would squeeze two into one.
            let columns = self.overview(class, rows, attention);
            return container(content.push(columns).width(Fill).height(Fill)).width(Fill).into();
        } else {
            list::view(list::ListData { items: &self.items, rows: self.rows_data(rows), attention })
        };
        let list = container(content.push(rows).width(Fill).height(Fill)).width(Fill);
        // Only the wide layout caps the width: in the medium one the content is at most a little
        // over `CONTENT_MAX` anyway, and a cap there would leave a thin empty strip at the right
        // (at 800 wide about 20) that reads as a mistake rather than as a margin. The mail window's
        // list never stops short (issue #129, [`App::list_cap`]).
        match (class, self.list_cap(1)) {
            (SizeClass::Wide, Some(cap)) => list.max_width(cap).into(),
            _ => list.into(),
        }
    }

    /// What the rows of entries need (`list::Rows`): the cursor, the time, the mail accounts in the
    /// sidebar's order (their colours) and which entries have a menu "Hide from Notification Hub".
    fn rows_data<'a>(&'a self, anchors: &detail::Anchors) -> list::Rows<'a> {
        list::Rows {
            view: &self.view,
            selected: self.cursor.as_deref(),
            now: self.now,
            anchors: anchors.clone(),
            accounts: mail::by_account(&self.folders).into_iter().map(|group| mail::address(group.account)).collect(),
            hidable: self.items.iter().filter(|item| self.hidable(item)).map(|item| item.id.as_str()).collect(),
            look: self.look(),
            folders: &self.folders,
            ai: self.ai_ready,
        }
    }

    /// The overview (task E3.9): both columns side by side in the wide layout, each under its head –
    /// with an open panel only the column of its entry, the panel takes the other's place –; one
    /// column, [`App::column`], in the medium and narrow ones.
    fn overview<'a>(
        &'a self,
        class: SizeClass,
        anchors: &detail::Anchors,
        attention: iced::Color,
    ) -> Element<'a, Message> {
        let wide = class == SizeClass::Wide;
        let shown = overview::shown(class, self.detail.as_ref(), self.column, self.launch.kind.columns());
        let cap = self.list_cap(shown.len());
        let mut columns = row![].width(Fill).height(Fill);
        for (i, column) in shown.into_iter().enumerate() {
            if i > 0 {
                columns = columns.push(rule::vertical(size::HAIRLINE).style(style::rule));
            }
            columns = columns.push(overview::column_view(overview::ColumnData {
                column,
                sections: Sections::of(&self.items, column, self.now).ordered(self.order),
                info_open: self.info_open[column.index()],
                rows: self.rows_data(anchors),
                deciding: self.deciding.is_some(),
                attention,
                // The mail window has one column, which its header bar names already ("For me").
                head: wide && self.launch.kind != Kind::Mail,
                lead: self.lead(),
            }));
        }
        // Each column at most `CONTENT_MAX`, as the list of a view; the mail window's fills the width.
        let columns = container(columns).width(Fill);
        match cap {
            Some(cap) => columns.max_width(cap).into(),
            None => columns.into(),
        }
    }

    /// The views with their icon, then one entry per source.
    ///
    /// Every number is what the view behind it lists: open entries. "For me" and "Info" count
    /// their lane, a source its open entries in both lanes – so the sources add up to "For me"
    /// plus "Info". "Snoozed" and "Done" are put away and have no number. No "0" anywhere.
    ///
    /// The mail window (task E8.3) lists For me (its overview), Info, Later and Done, and no
    /// sources: they are all mail accounts, whose folders follow.
    fn nav_entries(&self) -> (Vec<NavEntry>, Vec<NavEntry>) {
        let kind = self.launch.kind;
        let entry = |view: View, count: Option<usize>| NavEntry {
            mark: Mark::Icon(kind.view_icon(&view)),
            label: kind.view_name(&view),
            count,
            active: view == self.view && !self.sorting && !self.is_insights(),
            view,
            target: Target::View,
        };
        // Like a badge: no "0" when nothing waits. The overview counts what is for the user.
        let for_me = Some(self.for_me.len()).filter(|&n| n > 0);
        let info = Some(self.info).filter(|&n| n > 0);
        let views = match kind {
            Kind::Mail => vec![
                entry(View::Overview, for_me),
                entry(View::Info, info),
                entry(View::Snoozed, None),
                entry(View::Done, None),
            ],
            // New, Later, Done (task E8.4); "New" counts all it shows, both lanes. The sources are
            // the window's choice, not views ([`App::sidebar`]).
            Kind::Notifications => {
                let new = Some(self.for_me.len() + self.info).filter(|&n| n > 0);
                return (
                    vec![entry(View::Overview, new), entry(View::Snoozed, None), entry(View::Done, None)],
                    Vec::new(),
                );
            }
            // Act now, To sort, Later, Done today (task E8.5); "Act now" counts what is for the user,
            // To sort what of it is to sort ([`hub::Section::of`]).
            Kind::Hub => {
                let to_sort = self.for_me.iter().filter(|item| hub::Section::of(item) == Some(hub::Section::ToSort));
                let to_sort = Some(to_sort.count()).filter(|&n| n > 0);
                let sort = NavEntry {
                    mark: Mark::Icon(Icon::Inbox),
                    label: tr("To sort").to_owned(),
                    count: to_sort,
                    active: self.is_hub() && self.sorting,
                    view: View::Overview,
                    target: Target::ToSort,
                };
                // What the inbox counted (task E8.8b), after the views of entries.
                let insights = NavEntry {
                    mark: Mark::Icon(Icon::ChartColumn),
                    label: tr("Insights").to_owned(),
                    count: None,
                    active: self.is_insights(),
                    view: View::Overview,
                    target: Target::Insights,
                };
                vec![entry(View::Overview, for_me), sort, entry(View::Snoozed, None), entry(View::Done, None), insights]
            }
        };
        if kind == Kind::Mail {
            return (views, Vec::new());
        }
        let sources = self
            .sources
            .iter()
            .map(|info| {
                let view = View::Source(info.source.clone());
                // Two accounts of one service are told apart by the account (task E5.15).
                let name = list::source_name(&info.source, self.sources.iter().map(|info| &info.source));
                let label = if info.muted { trf("{source} (muted)", &[("source", &name)]) } else { name };
                let count = usize::try_from(info.open).ok().filter(|&open| open > 0);
                NavEntry {
                    mark: Mark::Icon(view.icon()),
                    label,
                    count,
                    active: view == self.view,
                    view,
                    target: Target::View,
                }
            })
            .collect();
        (views, sources)
    }

    /// The smart views of the mail window (task E8.3), under "Smart views": "Invitations" (task
    /// E8.3b) and "With attachments", while there is a mail account. None in the other windows.
    fn smart_entries(&self) -> Vec<NavEntry> {
        if self.launch.kind != Kind::Mail || self.folders.is_empty() {
            return Vec::new();
        }
        [View::Invitations, View::Attachments]
            .into_iter()
            .map(|view| NavEntry {
                mark: Mark::Icon(view.icon()),
                label: view.name(),
                count: None,
                active: view == self.view,
                view,
                target: Target::View,
            })
            .collect()
    }

    /// The folders of the mail accounts by account (task E4.2), for the sidebar and the menu: one
    /// group titled "Mail" with a single account, else one per account titled with its address,
    /// as the calendars group by account. A folder counts its unread messages where that is news
    /// ([`mail::counts_unread`]); no "0".
    fn mail_groups(&self) -> Vec<MailGroup> {
        let groups = mail::by_account(&self.folders);
        let single = groups.len() == 1;
        groups
            .into_iter()
            .map(|group| MailGroup {
                title: if single { tr("Mail").to_owned() } else { mail::address(group.account).to_owned() },
                account: group.account.to_owned(),
                open: single || self.account_open(group.account),
                rows: group
                    .folders
                    .into_iter()
                    .map(|folder| {
                        let count = usize::try_from(folder.unread)
                            .ok()
                            .filter(|&unread| unread > 0 && mail::counts_unread(folder.role));
                        NavEntry {
                            mark: Mark::Icon(mail::folder_icon(folder.role)),
                            label: mail::folder_name(folder),
                            count,
                            active: self.view.folder().is_some_and(|shown| shown.id == folder.id),
                            view: View::Folder(FolderView::of(folder)),
                            target: Target::View,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Whether the folders of `account` show: as the person folded or unfolded it (now or at an
    /// earlier start, `ui.json`), else only the account of the folder shown. Folded by default
    /// (issue #121): two accounts with all their folders would push the rest of the sidebar out of a
    /// window of ordinary height, and a first start stays calm.
    fn account_open(&self, account: &str) -> bool {
        if let Some(&(_, open)) = self.folds.iter().find(|(folded, _)| folded == account) {
            return open;
        }
        self.view.folder().is_some_and(|folder| folder.account == account)
    }

    /// The sidebar in `mode`: expanded (wide, drawer) or as the rail of icons. The rail leaves the
    /// sources out: all agents share one icon and all scripts another, so as bare icons they
    /// could not be told apart; the navigation menu and the drawer still list them. The same goes
    /// for the folders of the mail accounts, except in a window of mail only, whose rail would be
    /// empty without them (their names are the tooltips).
    fn sidebar<'a>(&'a self, mode: SidebarMode, switcher: &Anchor) -> Element<'a, Message> {
        let (views, sources) = self.nav_entries();
        let row = |e: NavEntry| {
            let trail = e.trail();
            let message = e.message();
            widgets::sidebar_item_in(mode, e.mark, e.label, trail, e.active, message)
        };
        let mut items: Vec<Element<'a, Message>> =
            if self.mail_only { Vec::new() } else { views.into_iter().map(row).collect() };
        let smart = self.smart_entries();
        if !smart.is_empty() {
            items.push(widgets::sidebar_group_in(mode, tr("Smart views"), None));
            items.extend(smart.into_iter().map(row));
        }
        // The notifications window lists no mail (task E8.4), but "This window shows": its sources
        // to switch on and off, expanded only (the rail has no room for names; the chips stand in).
        if self.launch.kind == Kind::Notifications {
            let listed = self.listed_sources();
            if mode == SidebarMode::Expanded && !listed.is_empty() {
                let colour = palette(&self.theme()).text_2;
                items.push(widgets::sidebar_group_in(mode, tr("This window shows"), None));
                items.extend(notifications::source_rows(mode, &listed, &self.shown, &self.sources, colour));
            }
        } else if (mode == SidebarMode::Expanded && self.launch.kind != Kind::Hub) || self.mail_only {
            // The hub acts and does not read (task E8.5): no folders, unless it is a window of mail
            // only (`--source mail`).
            let groups = self.mail_groups();
            let single = groups.len() == 1;
            for group in groups {
                let toggle = (!single).then(|| (group.open, Message::ToggleAccount(group.account.clone())));
                items.push(widgets::sidebar_group_in(mode, group.title, toggle));
                match (group.open, mode) {
                    (true, _) => items.extend(group.rows.into_iter().map(row)),
                    // The rail has no title to unfold with: a folded account keeps its inbox there.
                    (false, SidebarMode::Rail) => {
                        items.extend(group.rows.into_iter().filter(NavEntry::is_inbox).map(row))
                    }
                    (false, SidebarMode::Expanded) => {}
                }
            }
        }
        if !sources.is_empty() && mode == SidebarMode::Expanded && !self.mail_only {
            // Folded until the person opens it (issue #121).
            let toggle = Some((self.sources_open, Message::ToggleSources));
            items.push(widgets::sidebar_group_in(mode, tr("Sources"), toggle));
            if self.sources_open {
                items.extend(sources.into_iter().map(row));
            }
        }
        // Where the band has it (macOS, U.10), "Settings" sits in its top right corner.
        let footer = (!shell::has_band())
            .then(|| {
                widgets::sidebar_item_in(
                    mode,
                    Icon::Settings,
                    tr("Settings"),
                    Trail::None,
                    false,
                    Message::OpenSettings,
                )
            })
            .into_iter()
            .collect();
        let head =
            shell::sidebar_head(mode, self.launch.kind.app(), switcher, Message::Switcher, Message::ToggleSidebar);
        shell::sidebar(head, items, footer)
    }

    /// The navigation menu of the medium and narrow layouts: the sidebar's rows as a menu.
    fn nav_menu(&self) -> Element<'_, Message> {
        let (views, sources) = self.nav_entries();
        let item = |e: NavEntry| {
            let trail = e.trail();
            let message = e.message();
            MenuItem::new(e.mark, e.label, message).trail(trail).current(e.active)
        };
        let mut sections = Vec::new();
        if !self.mail_only {
            sections.push(MenuSection::new(views.into_iter().map(item).collect()));
        }
        let smart = self.smart_entries();
        if !smart.is_empty() {
            sections.push(MenuSection::titled(tr("Smart views"), smart.into_iter().map(item).collect()));
        }
        // A menu has no folding: every account is a section named after it, with all its folders
        // only while one of them is shown (or for a single account), else its inbox alone – the
        // drawer unfolds the rest. So the menu fits a window of ordinary height. The narrow mail
        // window lists no folders (task E8.3): there it is about what is new; the drawer has them.
        let narrow_mail = self.launch.kind == Kind::Mail && self.class() == SizeClass::Narrow && !self.mail_only;
        // The notifications window has no mail at all (task E8.4); its sources are the chips.
        // Nor has the hub, which acts and does not read (task E8.5), unless it is a window of mail only.
        let hub = self.launch.kind == Kind::Hub && !self.mail_only;
        let no_mail = narrow_mail || hub || self.launch.kind == Kind::Notifications;
        let groups = if no_mail { Vec::new() } else { self.mail_groups() };
        let single = groups.len() == 1;
        for group in groups {
            let open = single || self.view.folder().is_some_and(|folder| folder.account == group.account);
            let rows = group.rows.into_iter().filter(|row| open || row.is_inbox()).map(item).collect();
            sections.push(MenuSection::titled(group.title, rows));
        }
        if !sources.is_empty() && !self.mail_only {
            sections.push(MenuSection::titled(tr("Sources"), sources.into_iter().map(item).collect()));
        }
        sections.push(MenuSection::new(vec![MenuItem::new(Icon::Settings, tr("Settings"), Message::OpenSettings)]));
        widgets::menu(sections)
    }

    /// The open menu of choices ([`App::choices`]) under `title`, each entry with its time (when it
    /// has one) and its digit. The menu card of `widgets::menu`, built from the same pieces here:
    /// its entries end in one trail only, and these need the time quietly before the key hint.
    ///
    /// The menu is as wide as its longest entry needs ([`choice_menu_width`]), so the addresses and
    /// domains of "Hide from Notification Hub" show whole.
    fn choice_menu<'a>(&self, title: impl Into<String>) -> Element<'a, Message> {
        let choices = self.choices();
        let width = choice_menu_width(&choices);
        let entries = choices
            .into_iter()
            .enumerate()
            .map(|(i, choice)| choice_entry(choice, keys::digit(i + 1), KeyLook::Kbd, width));
        let list = column![menu_caption(&title.into())].extend(entries);
        container(list).width(width).padding(style::MENU_INSET).style(style::container::menu).into()
    }

    /// The bell's menu (task E8.6b, the design's "Stummschalten · Menü an der Karte"): the section
    /// "Mute" with the ways to mute ([`App::mute_choices`], digits as in every menu of choices, shown
    /// quietly in `META` ([`KeyLook::Meta`], task E8.12); a
    /// quiet line while they load or when they could not be read), a line, "For" and "What happens"
    /// as segments (`widgets::segmented`, ←/→ and ↑/↓), a line and "All rules ↗", which opens the
    /// control centre, where every rule is listed with "Remove".
    ///
    /// As wide as its longest way to mute or its segments need ([`mute_menu_width`]), so a whole
    /// address shows.
    fn mute_menu<'a>(&self) -> Element<'a, Message> {
        let choices = self.choices();
        let width = mute_menu_width(&choices);
        let mut list = column![menu_caption(tr("Mute"))];
        let quiet = match &self.mute_options {
            None => Some(tr("Loading…")),
            Some(Err(_)) => Some(tr("Couldn't read the ways to mute this entry.")),
            Some(Ok(_)) if choices.is_empty() => Some(tr("Nothing to mute this entry by.")),
            Some(Ok(_)) => None,
        };
        if let Some(quiet) = quiet {
            let line = typo::text(typo::meta(), quiet).style(theme::text_3);
            list =
                list.push(container(line).height(style::MENU_ITEM_HEIGHT).center_y(Fill).padding(style::ITEM_PADDING));
        }
        // The digits quietly in `META` (task E8.12): boxed they read like counts here.
        let entries = choices
            .into_iter()
            .enumerate()
            .map(|(i, choice)| choice_entry(choice, keys::digit(i + 1), KeyLook::Meta, width));
        list = list.extend(entries);
        let span = MuteFor::ALL.iter().position(|span| *span == self.mute_for).unwrap_or_default();
        let how = MUTE_EFFECTS.iter().position(|effect| *effect == self.mute_effect).unwrap_or_default();
        let spans = widgets::segmented(MuteFor::ALL.map(|span| (span.name(), Message::MuteFor(span))), span);
        let effects =
            widgets::segmented(MUTE_EFFECTS.map(|effect| (effect_name(effect), Message::MuteHow(effect))), how);
        let segment = |control: Element<'a, Message>| {
            container(control).padding(Padding::ZERO.left(space::s()).right(space::s()).bottom(space::xs()))
        };
        let rules = rules_entry();
        list = list
            .push(menu_line())
            .push(menu_caption(tr("For")))
            .push(segment(spans))
            .push(menu_caption(tr("What happens")))
            .push(segment(effects))
            .push(menu_line())
            .push(widgets::menu_item(rules));
        container(list).width(width).padding(style::MENU_INSET).style(style::container::menu).into()
    }

    /// The picker of "Pick…" as a floating card: the chosen moment as title ("Wed, 14 Oct · 09:00"),
    /// the day with previous / next, the time of day as segments, and "Later" at the right (off
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
            typo::text(typo::body(), day_name(day, today)).style(theme::text).wrapping(Wrapping::None),
            widgets::icon_button(Icon::ChevronRight, tr("Next day"), next),
        ]
        .spacing(space::xs())
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
            widgets::text_button(Some(Icon::Clock), tr("Later"), style::button::primary, snooze),
        ];
        widgets::popover(widgets::section_title(tr("Later until")), title, rows, actions)
    }

    /// The dialog of "Make task" as a floating card at the entry (the shape of `widgets::popover`,
    /// whose title is text only; here the title is the field): "Make task" over the title field,
    /// the list and the due day as properties, and at the right "Cancel" and "Create task" (off
    /// until a list is chosen). The list row says so while the lists load, when there are none and
    /// when they could not be read.
    fn task_dialog(&self) -> Element<'_, Message> {
        let inset = Padding::new(0.0).left(space::s()).right(space::s());
        let title = text_input(tr("Task title"), &self.task.title)
            .id(TASK_TITLE)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
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
        // A folder's message asks for its due day as an entry does (task E8.1b).
        let due = choice(self.task.due.offered(), Some(self.task.due), Message::TaskDue);
        let mut rows = column![
            widgets::prop_line(tr("List"), Some(Mark::Icon(Icon::ListTodo)), list, None),
            widgets::prop_line(tr("Due"), Some(Mark::Icon(Icon::Calendar)), due, None),
        ];
        // The AI's note (issue #120), only where it filled in the dialog or one is kept.
        if self.ai_suggesting.is_some() || !self.task.notes.is_empty() {
            let notes = dialog_field(tr("Notes"), &self.task.notes, Message::TaskNotes);
            rows = rows.push(widgets::prop_line(tr("Notes"), Some(Mark::Icon(Icon::AlignLeft)), notes, None));
        }
        let create = self.task.list.is_some().then_some(Message::TaskConfirm);
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseMenu)),
            widgets::text_button(Some(Icon::Check), tr("Create task"), style::button::primary, create),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        // The field inside the same inset as the rows and the line (task E8.12, finding 7): its
        // edges in line with theirs.
        let card = column![
            container(widgets::section_title(tr("Make task"))).padding(inset.bottom(space::xs())),
            container(title).padding(inset.bottom(space::s())),
            self.suggesting_line().map(|line| container(line).padding(inset.bottom(space::s()))),
            rows,
            column![iced::widget::rule::horizontal(size::HAIRLINE).style(style::rule), actions]
                .spacing(space::m())
                .padding(inset.top(space::m())),
        ];
        container(card)
            .width(widgets::POPOVER_WIDTH)
            .padding([space::l(), space::s()])
            .style(style::container::popover)
            .into()
    }

    /// The dialog of "Make event" (task E8.5b) as a floating card at the entry, built as the one of
    /// "Make task": "Make event" over the title field, the calendar, the day (with previous / next,
    /// as the picker), the start and the duration as properties, and at the right "Cancel" and
    /// "Create event" (off until a calendar is chosen). The calendar row says so while the calendars
    /// load, when there are none and when they could not be read.
    fn event_dialog(&self) -> Element<'_, Message> {
        let inset = Padding::new(0.0).left(space::s()).right(space::s());
        let title = text_input(tr("Event title"), &self.event.title)
            .id(EVENT_TITLE)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .font(typo::body().font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::EventTitle)
            .on_submit(Message::EventConfirm);
        let calendar: Element<'_, Message> = match &self.calendars {
            None => widgets::prop_placeholder(tr("Loading calendars…")),
            Some(Err(_)) => widgets::prop_placeholder(tr("Couldn't load the calendars.")),
            Some(Ok(calendars)) if calendars.is_empty() => widgets::prop_placeholder(tr("No calendars yet")),
            Some(Ok(calendars)) => choice(calendars.clone(), self.event.calendar.clone(), Message::EventCalendar),
        };
        let (today, day) = (local_today(self.now), self.event.day);
        let previous = (day > today).then(|| Message::EventDay(day - Days::new(1)));
        let next = (day < today + Days::new(PICK_DAYS)).then(|| Message::EventDay(day + Days::new(1)));
        let day_row = row![
            widgets::icon_button(Icon::ChevronLeft, tr("Previous day"), previous),
            typo::text(typo::body(), day_name(day, today)).style(theme::text).wrapping(Wrapping::None),
            widgets::icon_button(Icon::ChevronRight, tr("Next day"), next),
        ]
        .spacing(space::xs())
        .align_y(Alignment::Center);
        let start = choice(self.event.start.offered(), Some(self.event.start), Message::EventStart);
        let length = choice(self.event.length.offered(), Some(self.event.length), Message::EventLength);
        let mut rows = column![
            widgets::prop_line(tr("Calendar"), Some(Mark::Icon(Icon::Calendar)), calendar, None),
            widgets::prop_line(tr("Day"), Some(Mark::Icon(Icon::CalendarDays)), day_row, None),
            widgets::prop_line(tr("Starts"), Some(Mark::Icon(Icon::CalendarClock)), start, None),
            widgets::prop_line(tr("Duration"), Some(Mark::Icon(Icon::Clock)), length, None),
        ];
        // The AI's place and who takes part (issue #120), only where it filled in the dialog or one
        // is kept.
        if self.ai_suggesting.is_some() || !self.event.location.is_empty() || !self.event.notes.is_empty() {
            let place = dialog_field(tr("Location"), &self.event.location, Message::EventLocation);
            let notes = dialog_field(tr("Notes"), &self.event.notes, Message::EventNotes);
            rows = rows
                .push(widgets::prop_line(tr("Location"), Some(Mark::Icon(Icon::MapPin)), place, None))
                .push(widgets::prop_line(tr("Notes"), Some(Mark::Icon(Icon::AlignLeft)), notes, None));
        }
        let create = self.event.calendar.is_some().then_some(Message::EventConfirm);
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseMenu)),
            widgets::text_button(Some(Icon::Check), tr("Create event"), style::button::primary, create),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        // The field inside the same inset as the rows and the line (task E8.12, finding 7).
        let card = column![
            container(widgets::section_title(tr("Make event"))).padding(inset.bottom(space::xs())),
            container(title).padding(inset.bottom(space::s())),
            self.suggesting_line().map(|line| container(line).padding(inset.bottom(space::s()))),
            rows,
            column![iced::widget::rule::horizontal(size::HAIRLINE).style(style::rule), actions]
                .spacing(space::m())
                .padding(inset.top(space::m())),
        ];
        container(card)
            .width(widgets::POPOVER_WIDTH)
            .padding([space::l(), space::s()])
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
        self.mail_subject = None;
        self.ai_suggesting = None;
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
                        text: tr("Done").to_owned(),
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
            Some(Open::AiTask | Open::AiEvent | Open::NoAi) => {
                if let Some(item) = self.items.iter().find(|item| !detail::is_approval(item)).cloned() {
                    self.cursor = Some(item.id.clone());
                    self.detail = Some(item.clone());
                    // The demo has an AI; its answer to "is one set up?" may come after this.
                    self.ai_ready = self.launch.open != Some(Open::NoAi);
                    match self.launch.open {
                        Some(Open::AiTask) => return self.suggest_task(&item.id),
                        Some(Open::AiEvent) => return self.suggest_event(&item.id),
                        _ => {}
                    }
                }
            }
            Some(Open::Event) => {
                let sort = hub::Sections::of(&self.items, None).to_sort.first().map(|item| (*item).clone());
                if let Some(item) = sort.or_else(|| self.items.iter().find(|item| !detail::is_approval(item)).cloned())
                {
                    self.cursor = Some(item.id.clone());
                    self.detail = Some(item.clone());
                    return self.ask_event(&item.id);
                }
            }
            Some(Open::Mail | Open::Invitation) => {
                self.cursor = self.messages.first().map(|message| message.id.clone());
                self.reading = self.cursor.is_some();
            }
            Some(Open::Search) => {
                self.search = "budget".to_owned();
                return self.run_search();
            }
            Some(Open::MuteMail) => {
                // Read mail never became an entry (the feed files unread mail only).
                let alone = |message: &&MessageSummary| {
                    message.flags.contains(&Flag::Seen)
                        && mail::entry_of(&self.for_me, &message.id).is_none()
                        && self.entry_of(&message.id).is_none()
                };
                if let Some(id) = self.messages.iter().find(alone).map(|message| message.id.clone()) {
                    self.cursor = Some(id.clone());
                    self.menu_at = MenuAt::Row;
                    return self.ask_mute_message(&id);
                }
            }
            Some(Open::MailEntry) => {
                self.detail = self.items.iter().find(|item| matches!(item.link, Some(Link::Mail { .. }))).cloned();
            }
            Some(open @ (Open::MoveMail | Open::DeleteMail)) => {
                if let Some(id) = self.messages.first().map(|message| message.id.clone()) {
                    self.cursor = Some(id.clone());
                    self.reading = true;
                    return if open == Open::MoveMail { self.ask_move(id) } else { self.ask_delete_for_good(id) };
                }
            }
            Some(open @ (Open::Overview | Open::OverviewNotices)) => {
                let column = if open == Open::Overview { Column::Mail } else { Column::Notices };
                self.column = self.launch.kind.column_or_first(column);
                self.cursor = self.rows().first().map(|item| item.id.clone());
            }
            Some(Open::Tile) => self.filter = hub::tiles(&self.items).first().map(|tile| tile.origin.clone()),
            Some(open @ (Open::MuteMenu | Open::MutePanel)) => {
                if let Some(item) = self.items.iter().find(|item| notifications::bell(item).is_some()).cloned() {
                    self.cursor = Some(item.id.clone());
                    if open == Open::MutePanel {
                        self.detail = Some(item.clone());
                    }
                    return self.ask_mute_menu(&item.id);
                }
            }
            Some(open @ (Open::Hide | Open::HidePanel)) => {
                if let Some(item) = self.items.iter().find(|item| item.source.kind == SourceKind::Mail).cloned() {
                    self.cursor = Some(item.id.clone());
                    if open == Open::HidePanel {
                        self.detail = Some(item.clone());
                    }
                    return self.ask_hide_or_mute(&item.id);
                }
            }
            _ => {}
        }
        if let Some(detail) = &self.detail {
            self.cursor = Some(detail.id.clone());
            self.column = self.launch.kind.column_or_first(Column::of(detail));
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
        let limit = self.page_limit();
        let window = (self.launch.kind, self.shown.clone());
        let demo = self.launch.demo.then_some(self.now);
        let asked = (limit, self.order);
        let entries = Task::perform(fetch(inbox, view.clone(), empty, asked, window, demo), move |result| {
            Message::Loaded(view.clone(), result)
        });
        // Insights asks for its numbers with every load (task E8.8b): after muting, after an undo, on
        // the timer – so a suggestion that was taken goes away.
        if self.is_insights() { Task::batch([entries, self.load_stats()]) } else { entries }
    }

    /// How many messages a load of the view asks for: a page, or as many as "Load more" brought
    /// in already when the same folder is loaded again (at most what one request may ask).
    fn page_limit(&self) -> u32 {
        let same = self.view.folder().is_some_and(|folder| self.listed.as_ref() == Some(&folder.id));
        let shown = if same { u32::try_from(self.messages.len()).unwrap_or(u32::MAX) } else { 0 };
        shown.clamp(mail::PAGE, torro_ipc::mail::MAX_LIMIT)
    }
}

/// The folders of one mail account in the sidebar and the navigation menu ([`App::mail_groups`]).
struct MailGroup {
    /// "Mail", or the account's address when there are several.
    title: String,
    /// `mail:<address>`, for folding.
    account: String,
    /// Its folders show ([`App::account_open`]); always with a single account.
    open: bool,
    rows: Vec<NavEntry>,
}

/// A row of the sidebar and the navigation menu.
struct NavEntry {
    mark: Mark,
    label: String,
    view: View,
    /// Shown as counter where there is one.
    count: Option<usize>,
    active: bool,
    /// What the row shows: its view, or a page of the hub on the overview's entries.
    target: Target,
}

/// What a row of the sidebar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Its [`NavEntry::view`].
    View,
    /// The hub's "To sort" (task E8.5): the section alone on the overview, not a view.
    ToSort,
    /// The hub's Insights (task E8.8b): what the inbox counted, not a view of entries.
    Insights,
}

impl NavEntry {
    /// What a click on the row sends: the view, the hub's To sort alone or its Insights.
    fn message(&self) -> Message {
        match self.target {
            Target::View => Message::Show(self.view.clone()),
            Target::ToSort => Message::ShowToSort,
            Target::Insights => Message::ShowInsights,
        }
    }

    /// The row of an inbox folder, which a folded account keeps in the rail and the menu.
    fn is_inbox(&self) -> bool {
        self.view.folder().is_some_and(|folder| folder.role == Some(Role::Inbox))
    }

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

/// The title of a section of a menu card built here, as `widgets::menu` draws one: `typo::caption()`
/// in `text_3`, 8 around and 4 below.
fn menu_caption<'a>(title: &str) -> Element<'a, Message> {
    let head = typo::text(typo::caption(), title.to_owned()).style(theme::text_3);
    container(head).padding(Padding::new(space::s()).bottom(space::xs())).into()
}

/// The line between the sections of a menu card built here, as `widgets::menu` draws it.
fn menu_line<'a>() -> Element<'a, Message> {
    container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]).into()
}

/// The width a `widgets::segmented` of `labels` takes: the labels in `typo::caption()`, each with
/// the button's padding, and the frame with its inset and line.
fn segmented_width(labels: &[&str]) -> f32 {
    let padding = style::button_padding();
    let segments: f32 =
        labels.iter().map(|label| typo::text_width(label, typo::caption()) + padding.left + padding.right).sum();
    segments + 2.0 * (style::SEGMENT_INSET + size::HAIRLINE)
}

/// "All rules ↗" at the foot of the bell's menu. It opens the Inbox page of the control centre, not
/// the accounts page the gear opens ([`Message::OpenSettings`]), because the rules are listed there.
fn rules_entry<'a>() -> MenuItem<'a, Message> {
    MenuItem::new(Icon::Settings, tr("All rules"), Message::OpenRules).trail(Trail::Icon(Icon::ArrowUpRight))
}

/// The width of the bell's menu with `choices`: as a menu of choices ([`choice_menu_width`]), and
/// at least what its segments need beside the menu's and their insets – "Until tomorrow" and
/// "Only Info" are long in some languages.
fn mute_menu_width(choices: &[Choice]) -> f32 {
    let spans = MuteFor::ALL.map(MuteFor::name);
    let effects = MUTE_EFFECTS.map(effect_name);
    let frame = 2.0 * (style::MENU_INSET + space::s());
    let segments = segmented_width(&spans).max(segmented_width(&effects)) + frame;
    choice_menu_width(choices).max(segments.ceil()).min(size::panel())
}

/// The header's button that turns the order (task E8.15): the icon and word of the order shown
/// now ("Newest first", "Oldest first"), the word beside the icon with `words`, else as tooltip; a
/// click turns it.
fn order_button<'a>(order: Order, words: bool) -> Element<'a, Message> {
    let (icon, word) = match order {
        Order::NewestFirst => (Icon::ArrowDownWideNarrow, tr("Newest first")),
        Order::OldestFirst => (Icon::ArrowUpNarrowWide, tr("Oldest first")),
    };
    let toggle = Some(Message::ToggleOrder);
    if words {
        widgets::text_button(Some(icon), word, style::button::ghost, toggle)
    } else {
        widgets::icon_button(icon, word, toggle)
    }
}

/// Whether a narrow window of `kind` counts its view beside the title, as wider ones do: the
/// notifications window ("New 5", task E8.12); the mail window's narrow "For me" leads with its
/// number on the page and the hub with its numbers, so their narrow headers leave it out.
fn narrow_count(kind: Kind) -> bool {
    kind == Kind::Notifications
}

/// How the digit that picks a choice shows at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyLook {
    /// As a key hint (`widgets::kbd`), as in "Later until" and "Hide from Notification Hub".
    Kbd,
    /// Quietly in `META`, tabular, `text_3` – the bell's menu (task E8.12), where boxed digits beside
    /// "This conversation" and "Everything from deploy" read like counts.
    Meta,
}

/// The dialogs of "Make task" and "Make event" open below their trigger with its right edge, or –
/// while the panel is open (`panel`), at its footer's button – with their left edge at it (task
/// E8.12, finding 7): the panel ends at the window's edge, so the card stays over the panel, above
/// the footer where there is no room below (`widgets::Placement::Below`).
/// Where a menu for an entry hangs (issue #122): at the button `pick` names (Later, Move to …, Task,
/// Make event, "…") – of the panel or reader (`panel`) when `from_panel`, else of the row or card
/// under the cursor (`rows`). A button that is not on screen gives way to the row's, then to the
/// row itself; with none of them the menu stands in the window's middle (`widgets::floating`).
fn menu_anchor(
    from_panel: bool,
    panel: &detail::Anchors,
    rows: &detail::Anchors,
    pick: fn(&detail::Anchors) -> &Anchor,
) -> Anchor {
    let row = pick(rows).or(&rows.row);
    if from_panel { pick(panel).or(&row) } else { row }
}

fn dialog_placement(panel: bool) -> Placement {
    Placement::Below(if panel { Align::Start } else { Align::End })
}

/// One entry of [`App::choice_menu`], as `widgets::menu` draws one: icon, label, then – quietly,
/// tabular – when it snoozes until, and the digit that picks it (in `look`); with [`Choice::line`]
/// the line `widgets::menu` draws between its sections above it.
fn choice_entry<'a>(choice: Choice, key: &'static str, look: KeyLook, width: f32) -> Element<'a, Message> {
    let line = choice.line;
    let entry = choice_button(choice, key, look, width);
    if !line {
        return entry;
    }
    column![menu_line(), entry].into()
}

/// The button of [`choice_entry`] in a menu `width` wide.
///
/// A label longer than its room – only beyond [`choice_menu_width`]'s widest menu – ends in "…"
/// ([`choice_label`]) and shows whole as tooltip.
fn choice_button<'a>(choice: Choice, key: &'static str, look: KeyLook, width: f32) -> Element<'a, Message> {
    let mark = icon::icon(choice.icon, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text) });
    let shown = choice_label(&choice.label, choice.when.as_deref(), key, width);
    let cut = (shown != choice.label).then(|| choice.label.clone());
    let label = container(typo::text(typo::body(), shown).wrapping(Wrapping::None)).width(Fill).clip(true);
    let mut content = row![mark, label].spacing(space::s()).align_y(Alignment::Center);
    if let Some(when) = choice.when {
        content = content.push(typo::text(typo::meta().tabular(), when).style(theme::text_3).wrapping(Wrapping::None));
    }
    content = content.push(match look {
        KeyLook::Kbd => widgets::kbd(key),
        KeyLook::Meta => typo::text(typo::meta().tabular(), key).style(theme::text_3).wrapping(Wrapping::None).into(),
    });
    let button = iced::widget::button(container(content).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(choice.message);
    match cut {
        Some(whole) => widgets::with_tooltip(button, whole),
        None => button.into(),
    }
}

/// `label` as it fits in a choice of [`choice_button`] in a menu `width` wide, beside its time
/// `when` and its key hint `key`: whole, or cut with "…" ([`typo::ellipsize`]).
fn choice_label(label: &str, when: Option<&str>, key: &str, width: f32) -> String {
    typo::ellipsize(label, typo::body(), width - choice_frame(when, key))
}

/// What a choice takes from its menu's width besides its label: the menu's and the entry's
/// insets, the icon, the gaps, the time `when` and the key hint `key`.
fn choice_frame(when: Option<&str>, key: &str) -> f32 {
    let gap = space::s();
    let kbd = typo::text_width(key, typo::caption()) + style::KBD_PADDING.left + style::KBD_PADDING.right;
    let when = when.map_or(0.0, |when| typo::text_width(when, typo::meta()) + gap);
    let frame = 2.0 * style::MENU_INSET + style::ITEM_PADDING.left + style::ITEM_PADDING.right;
    frame + icon::SIZE + 2.0 * gap + kbd + when
}

/// The width of a menu of `choices`: [`widgets::menu_width`], wider as far as its longest label
/// needs to show whole – the address or domain of "Hide from Notification Hub" (task E3.10) –, at most
/// [`size::panel()`], beyond which a label ends in "…".
fn choice_menu_width(choices: &[Choice]) -> f32 {
    let needed = choices.iter().enumerate().map(|(i, choice)| {
        choice_frame(choice.when.as_deref(), keys::digit(i + 1)) + typo::text_width(&choice.label, typo::body())
    });
    needed.fold(widgets::menu_width(), f32::max).ceil().min(size::panel())
}

/// A pick list in the suite's look for a property of the dialog of "Make task" (and the calendar of
/// an invitation's card, task E8.3c, `mail::invite_card`): `options` shown
/// by their names, `selected` in it, `on_pick` with the one picked. As high as a small button,
/// as wide as its row.
pub(crate) fn choice<'a, T>(options: Vec<T>, selected: Option<T>, on_pick: fn(T) -> Message) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_pick)
        .width(Fill)
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
        .into()
}

/// `text` without the spaces around it, `None` when nothing is left: a note or place of a dialog
/// as the core takes it (issue #120).
fn filled(text: &str) -> Option<String> {
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// A one-line field of a dialog in the suite's look: placeholder `hint`, `value`, `on_input`.
fn dialog_field<'a>(hint: &'a str, value: &'a str, on_input: fn(String) -> Message) -> Element<'a, Message> {
    text_input(hint, value)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .font(typo::body().font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(on_input)
        .into()
}

/// The failure when the entries could not be loaded; the error state's "Retry" is what the user
/// can do about it.
/// Asks the core to open `app` on `target` (task E8.7b, `apps.open_app` with `target`): `task:<uid>`
/// in the tasks app, `mail:<address>:<Message-ID>` in the mail window. The core checks the target
/// and starts the app with it; nothing reaches a shell. With `--demo` there is no core: nothing
/// opens and the answer is `Ok(())` (as `Source::open_app`). An `Err` carries the core's message.
pub async fn open_target(suite: Source, app: &'static str, target: String) -> Result<(), String> {
    let Source::Core(client) = suite else { return Ok(()) };
    let request = torro_ipc::Request::OpenApp { app: app.to_owned(), target: Some(target) };
    match torro_ipc::client::call(&client.socket, &request).await {
        Ok(torro_ipc::Response::Ok) => Ok(()),
        Ok(torro_ipc::Response::Error { message, .. }) => Err(message),
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// The demo's way back between mail and tasks (task E8.7b), laid over its entries so every state of
/// the design can be seen without a core (the inbox's demo data know no tasks):
///
/// - `demo-mail-1` (Jonas's "Budget review for Q4") was made into the task "Write quarterly report"
///   – the uid of that task in the tasks app's demo, so "Open task" opens it there –, still open;
/// - `demo-11` (Codex's finished refactoring, done) into "Review the settings refactor", open;
/// - `demo-9` becomes the suite's "You finished a task from a mail": "Bring the sidebar drafts" is
///   done, and Lena's mail (`demo-mail-2`) waits for an answer.
///
/// Every other entry stays as the demo source answers it.
pub fn demo_task(item: &mut Item, now: DateTime<Utc>) {
    let today = local_today(now);
    let task = |uid: &str, title: &str, due: Option<NaiveDate>, link: &str| torro_inbox::item::ItemTask {
        uid: uid.to_owned(),
        collection: torro_inbox::demo::DEMO_LISTS[1].to_owned(),
        list: tr("Work").to_owned(),
        title: title.to_owned(),
        due: due.map(torro_inbox::item::TaskDue::Date),
        completed: false,
        link: link.to_owned(),
        mail_entry: None,
    };
    match item.id.as_str() {
        "demo-mail-1" => {
            let link = "mail:anna@work.example.org:budget-q4@work.example.org";
            item.task =
                Some(task("quarterly-report@torro-demo", "Write quarterly report", Some(today + Days::new(9)), link));
        }
        "demo-11" => {
            let link = "inbox:demo-11";
            item.task =
                Some(task("demo-review-settings", "Review the settings refactor", Some(today + Days::new(1)), link));
        }
        "demo-9" => {
            let done = torro_inbox::item::ItemTask {
                completed: true,
                mail_entry: Some("demo-mail-2".to_owned()),
                ..task(
                    "sidebar-drafts@torro-demo",
                    "Bring the sidebar drafts",
                    None,
                    "mail:anna@work.example.org:design@work.example.org",
                )
            };
            let title = format!("\u{201c}{}\u{201d}", done.title);
            item.source = SourceRef::suite();
            item.kind = torro_inbox::Kind::Notice;
            item.lane = Lane::Info;
            item.title = tr("You finished a task from a mail").to_owned();
            item.body = Some(trf(
                "{task} is done. {sender} is waiting for an answer.",
                &[("task", &title), ("sender", &"Lena Hoffmann")],
            ));
            item.link = None;
            item.facets = Vec::new();
            item.task = Some(done);
        }
        _ => {}
    }
}

fn load_failed(err: &str) -> Failure {
    Failure::new(tr("Couldn't load the entries."), err)
}

/// The error state of a first load: the alert icon, the sentence (`BODY_STRONG`, like the title of
/// the empty state), the reason quietly below it (`META`, `text_3`) and "Retry". The shared
/// `widgets::error_state` has one text only, so this is it with the reason as its own line.
fn failure_state(failure: &Failure) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let words = column![
        typo::text(typo::body_strong(), failure.what).align_x(Alignment::Center),
        typo::text(typo::meta(), failure.why.as_str())
            .style(theme::text_3)
            .align_x(Alignment::Center)
            .wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(space::xs())
    .align_x(Alignment::Center);
    let content = column![
        alert,
        words,
        widgets::text_button(Some(Icon::RefreshCw), tr("Retry"), style::button::secondary, Some(Message::Refresh)),
    ]
    .spacing(space::m())
    .align_x(Alignment::Center)
    .max_width(size::form_max() / 2.0);
    iced::widget::center(content).padding(space::xl()).into()
}

/// The line above the list when a later call failed: the alert icon in the error colour, the
/// sentence in `text_2` and the reason after it in `text_3` (status colour only on the icon).
fn failure_line(failure: &Failure) -> Element<'_, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).error_text) });
    let what = typo::text(typo::meta(), failure.what).style(theme::text_2).wrapping(Wrapping::None);
    let why = typo::text(typo::meta(), failure.why.as_str()).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
    row![alert, row![what, why].spacing(space::xs())].spacing(space::s()).align_y(Alignment::Center).into()
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
    fn load<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(app: &mut App<S>) {
        let empty = app.launch.open == Some(Open::Empty);
        let window = (app.launch.kind, app.shown.clone());
        let demo = app.launch.demo.then_some(app.now);
        let asked = (app.page_limit(), app.order);
        let result = block_on(fetch(app.inbox.clone(), app.view.clone(), empty, asked, window, demo));
        let _ = app.update(Message::Loaded(app.view.clone(), result));
    }

    /// A demo app at the demo's fixed time, loaded – on "For me" unless `words` choose a view (the
    /// window starts on the overview, task E3.9; [`overview_at`] tests that).
    fn loaded(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T10:30"];
        if !words.iter().any(|word| word.starts_with("--lane") || word.starts_with("--source")) {
            all.extend(["--lane", "for-me"]);
        }
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    /// The open entries of "For me" in the demo, waiting first: the four urgent ones (the ntfy
    /// alert of task E5.1 and the three waiting ones), then by time, the mail entries and those of
    /// GitHub, Nextcloud and Talk among them.
    const FOR_ME: [&str; 18] = [
        "demo-18",
        "demo-1",
        "demo-3",
        "demo-2",
        "demo-17",
        "demo-14",
        "demo-mail-1",
        "demo-4",
        "demo-15",
        "demo-6",
        "demo-mail-4",
        "demo-16",
        "demo-5",
        "demo-mail-2",
        "demo-mail-6",
        "demo-12",
        "demo-13",
        "demo-mail-8",
    ];

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
        let launch = Launch::parse(&args(&["--demo", "--design", "torro"])).expect("the standard is always there");
        assert_eq!(launch.design.as_deref(), Some("torro"));
        assert_eq!(Launch::parse(&args(&["--design", "torro"])), Err(LaunchError::NeedsDemo("--design")));
        assert_eq!(Launch::parse(&args(&["--demo", "--design"])), Err(LaunchError::MissingValue("--design")));
    }

    /// The message "design changed" (task L.3): the stored setting arrives and the window draws
    /// with that pack from the next frame; a broken one gives the standard.
    #[test]
    fn the_stored_design_is_drawn_and_a_broken_one_gives_the_standard() {
        let dir = tempfile::tempdir().expect("temp dir");
        // The standard's values under another id, so the tests beside this one see the same tokens.
        for (id, toml) in [
            ("twin", "format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n"),
            ("pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n"),
        ] {
            std::fs::create_dir_all(dir.path().join(id)).expect("pack folder");
            std::fs::write(dir.path().join(id).join("design.toml"), toml).expect("design.toml");
        }
        let mut app = loaded(&[]);
        app.design = Follow::new(None).in_dir(dir.path().to_path_buf());
        let switched = app.update(Message::Design(design::Message::Stored(Ok("twin".into()))));
        assert_eq!(design::active().id, "twin");
        assert!(switched.units() > 0, "the fonts of the new design are loaded");
        let _ = app.view();
        let _ = app.update(Message::Design(design::Message::Stored(Ok("pale".into()))));
        assert!(design::active().is_standard(), "a broken pack is never drawn");
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

    /// Task E8.1: `--view` chooses one of the three windows, the hub without it; `--lane` and
    /// `--source` still choose the start view inside it.
    #[test]
    fn view_chooses_the_window_kind() {
        let kind = |words: &[&str]| Launch::parse(&args(words)).map(|launch| launch.kind);
        assert_eq!(kind(&[]), Ok(Kind::Hub));
        assert_eq!(kind(&["--view", "hub"]), Ok(Kind::Hub));
        assert_eq!(kind(&["--view", "mail"]), Ok(Kind::Mail));
        assert_eq!(kind(&["--view=notifications"]), Ok(Kind::Notifications));
        assert_eq!(kind(&["--view"]), Err(LaunchError::MissingValue("--view")));
        let refused = kind(&["--view", "inbox"]).expect_err("not a kind");
        assert_eq!(
            refused,
            LaunchError::UnknownValue {
                flag: "--view",
                value: "inbox".into(),
                valid: "mail, notifications, hub".into()
            }
        );
        assert_eq!(
            refused.to_string(),
            "unknown value \"inbox\" for --view; valid values: mail, notifications, hub",
            "main prints this and exits with 64"
        );
        let launch = Launch::parse(&args(&["--view", "mail", "--lane", "info"])).expect("valid");
        assert_eq!((launch.kind, launch.view), (Kind::Mail, Some(View::Info)));
    }

    /// Task E8.2: `--density` is read, an unknown one refused with the valid ones (exit 64).
    #[test]
    fn density_is_read_from_the_command_line() {
        let density = |words: &[&str]| Launch::parse(&args(words)).map(|launch| launch.density);
        assert_eq!(density(&[]), Ok(None), "without it the window takes what it remembers");
        assert_eq!(density(&["--density", "compact"]), Ok(Some(Density::Compact)));
        assert_eq!(density(&["--view", "mail", "--density=comfortable"]), Ok(Some(Density::Comfortable)));
        assert_eq!(density(&["--density"]), Err(LaunchError::MissingValue("--density")));
        let refused = density(&["--density", "cozy"]).expect_err("not a density");
        assert_eq!(
            refused,
            LaunchError::UnknownValue { flag: "--density", value: "cozy".into(), valid: "compact, comfortable".into() }
        );
    }

    /// Task E8.2: every window kind and every window per slice has a key of its own in `ui.json`.
    #[test]
    fn every_window_remembers_its_density_under_its_own_key() {
        let key = |words: &[&str]| Launch::parse(&args(words)).expect("valid").density_key();
        assert_eq!(key(&[]), "hub");
        assert_eq!(key(&["--view", "mail"]), "mail");
        assert_eq!(key(&["--view", "notifications"]), "notifications");
        assert_eq!(key(&["--lane", "info"]), "hub/lane:info");
        assert_eq!(key(&["--view", "mail", "--lane", "for-me"]), "mail/lane:for-me");
        assert_eq!(key(&["--source", "mail"]), "hub/source:mail");
        assert_eq!(
            key(&["--view", "notifications", "--source", "agent:claude-code"]),
            "notifications/source:agent:claude-code"
        );
        assert_eq!(key(&["--density", "compact", "--view", "mail"]), "mail", "the density is no part of its key");
    }

    /// Task E8.2: `--density` wins over `ui.json`, which wins over the layout's default; the switch
    /// changes it in any case.
    #[test]
    fn density_follows_the_command_line_then_ui_json_then_the_layout() {
        let mut app = demo(&["--demo", "--view", "mail", "--density", "compact"]);
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::StoredDensity(Some(Density::Comfortable)));
        assert_eq!(app.density(), Density::Compact, "the command line wins over ui.json");
        let _ = app.update(Message::SetDensity(Density::Comfortable));
        assert_eq!(app.density(), Density::Comfortable, "the switch changes it");

        let mut app = demo(&["--demo", "--view", "mail"]);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!(app.density(), Density::Comfortable, "nothing chosen: comfortable in the wide layout");
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!(app.density(), Density::Compact, "nothing chosen: compact in the narrow one");
        let _ = app.update(Message::StoredDensity(Some(Density::Comfortable)));
        assert_eq!(app.density(), Density::Comfortable, "what ui.json remembers wins over the layout");
        let _ = app.update(Message::SetDensity(Density::Compact));
        let _ = app.update(Message::StoredDensity(Some(Density::Comfortable)));
        assert_eq!(app.density(), Density::Compact, "a late answer of ui.json does not undo a choice");
    }

    /// Task E8.2: the switch is in the header bar of every window kind, in every size with a header.
    #[test]
    fn every_window_kind_draws_the_density_switch() {
        for kind in ["mail", "notifications", "hub"] {
            for density in ["compact", "comfortable"] {
                let mut app = demo(&["--demo", "--view", kind, "--density", density]);
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _: Element<'_, Message> = app.view();
                }
            }
        }
    }

    /// Every kind is an entry of the suite's apps, and the arguments the core starts it with
    /// (`AppId::args`) start that kind again – with its own window class.
    #[test]
    fn every_kind_starts_from_the_arguments_of_its_app_entry() {
        for (_, kind) in Kind::ALL {
            let app = kind.app();
            assert!(AppId::ALL.contains(&app), "{} is a suite app", app.window_class);
            assert_eq!(app.binary, INBOX.binary);
            let launch = Launch::parse(&app.command("torroinbox")[1..]).expect("valid parameters");
            assert_eq!(launch.kind, kind, "{}", app.window_class);
        }
        let classes = Kind::ALL.map(|(_, kind)| kind.app().window_class);
        assert_eq!(classes, ["torro-inbox-mail", "torro-inbox-notifications", "torro-inbox"]);
    }

    /// A workspace template stores the mail window's command line and starts it again on load: it
    /// comes back as the mail window, with its window class and title.
    #[test]
    fn a_restored_mail_window_is_a_mail_window_again() {
        let stored = AppId::INBOX_MAIL.command("/usr/bin/torroinbox");
        let launch = Launch::parse(&stored[1..]).expect("valid parameters");
        let app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        assert_eq!(app.launch.kind.app().window_class, "torro-inbox-mail");
        assert_eq!((app.view.clone(), app.column), (View::Overview, Column::Mail));
        assert_eq!(app.title(), trf("Mail · {view}", &[("view", &tr("For me"))]), "its overview is \"For me\"");
    }

    /// A workspace template stores the window's command line (`/proc/<pid>/cmdline`) and starts it
    /// again on load; the restored window must come back on the same slice.
    #[test]
    fn a_restored_command_line_starts_on_its_view() {
        let stored = args(&["/usr/bin/torroinbox", "--lane", "info"]);
        let launch = Launch::parse(&stored[1..]).expect("valid parameters");
        let app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        assert_eq!(app.view, View::Info);
        assert_eq!(app.title(), trf("Notification Hub · {view}", &[("view", &tr("Info"))]));

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
        assert_eq!(
            View::ForMe.filter(),
            Some(ItemFilter { lane: Some(Lane::ForMe), state: open, ..ItemFilter::default() })
        );
        assert_eq!(
            View::Info.filter(),
            Some(ItemFilter { lane: Some(Lane::Info), state: open, ..ItemFilter::default() })
        );
        assert_eq!(View::Snoozed.filter().and_then(|f| f.state), Some(StateFilter::Snoozed));
        assert_eq!(View::Done.filter().and_then(|f| f.state), Some(StateFilter::Done));
        assert_eq!(View::Done.filter().and_then(|f| f.dismissed), Some(false), "issue #117: without the dismissed");
        let codex = View::Source(SourceRef::agent("codex")).filter().expect("entries");
        assert_eq!((codex.lane, codex.source, codex.state), (None, Some(SourceRef::agent("codex")), open));
        assert_eq!(View::ForMe.name(), tr("For me"));
        assert_eq!(View::Source(SourceRef::agent("claude-code")).name(), "Claude Code");
    }

    #[test]
    fn for_me_shows_urgent_entries_first_and_counts_them() {
        let app = loaded(&[]);
        assert_eq!(ids(&app), FOR_ME);
        assert_eq!(app.for_me.len(), 18);
        assert_eq!(app.count_note(), "18", "only the number: the title already says For me");
        let groups = list::groups(&app.items);
        assert_eq!(groups, [(list::Group::Waiting, 0..4), (list::Group::Other, 4..18)], "waiting entries get a head");
        assert_eq!(
            app.sources.len(),
            14,
            "suite, two agents, five scripts, two mail accounts, GitHub, Nextcloud, Talk, ntfy"
        );
        assert_eq!(app.title(), trf("Notification Hub · {view}", &[("view", &tr("For me"))]));
    }

    #[test]
    fn other_views_keep_the_for_me_count() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-8", "demo-9", "demo-mail-3", "demo-mail-5", "demo-7", "demo-mail-7"]);
        assert_eq!(app.for_me.len(), 18, "the sidebar still counts For me");
        assert_eq!(app.count_note(), "6");
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
        assert_eq!(for_me, FOR_ME, "not Info's entries");
        assert_eq!(glance::waiting(&app.for_me), 4, "the alert and three waiting");
    }

    #[test]
    fn the_title_band_repeats_what_lies_over_the_window() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Resized(MEDIUM));
        assert_eq!(app.cover(), Cover::Nothing);
        app.drawer = true;
        assert_eq!(app.cover(), Cover::Drawer, "the drawer over a medium window");

        let mut detail = loaded(&["--open", "detail"]);
        detail.drawer = true;
        for (size, expected) in
            [(MEDIUM, Cover::Panel), (Size::new(420.0, 780.0), Cover::Filled(Surface::Panel)), (WIDE, Cover::Nothing)]
        {
            let _ = detail.update(Message::Resized(size));
            assert_eq!(detail.cover(), expected, "{size:?}: the entry's panel lies above the drawer");
        }
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
            // The keys, ⌘F of the search (the demo has mail) and Escape.
            let expected = if glance { wide - 3 } else { wide };
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

    /// [`loaded`] late in the demo's day, when the codex session done 22 hours before was done
    /// today: the hub's "Done today" lists it (task E8.5).
    fn loaded_late(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T23:30", "--lane", "for-me"];
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    /// Task E8.15: "Oldest first" turns the folder and its "Load more", and the entry lists with
    /// the waiting ones on top; a page asked in the order before is dropped.
    #[test]
    fn the_order_turns_folders_and_entry_lists() {
        let mut app = in_folder("f:work-inbox");
        let newest: Vec<String> = app.messages.iter().map(|message| message.id.clone()).collect();
        assert!(app.orderable());
        let _ = app.update(Message::ToggleOrder);
        assert_eq!(app.order, Order::OldestFirst);
        load(&mut app);
        let oldest: Vec<String> = app.messages.iter().map(|message| message.id.clone()).collect();
        let all = block_on(MailSource::search_mail(
            app.inbox.as_ref(),
            "f:work-inbox".into(),
            None,
            50,
            None,
            Order::OldestFirst,
        ))
        .expect("the demo answers");
        let first: Vec<String> = all.messages.iter().take(oldest.len()).map(|message| message.id.clone()).collect();
        assert_eq!(oldest, first, "the oldest page first");
        assert_ne!(oldest.first(), newest.first());
        // A page of "Load more" asked newest first does not belong any more.
        let late = Ok(MailPage::of(all.messages.clone(), None));
        let _ = app.update(Message::MoreLoaded(app.view.clone(), Order::NewestFirst, late));
        assert_eq!(app.messages.len(), oldest.len(), "dropped");

        let mut app = loaded(&["--view", "notifications"]);
        let _ = app.update(Message::ToggleOrder);
        load(&mut app);
        let waiting = app.items.iter().take_while(|item| list::is_waiting(item)).count();
        let rest: Vec<_> = app.items[waiting..].iter().map(|item| item.updated).collect();
        assert!(rest.windows(2).all(|pair| pair[0] <= pair[1]), "the others oldest first");
        assert!(app.items[waiting..].iter().all(|item| !list::is_waiting(item)), "waiting ones on top");
    }

    /// Task E8.15: the remembered order lists anew; a turn made before the file answered wins.
    #[test]
    fn the_remembered_order_comes_unless_turned_before() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::StoredOrder(Some(Order::OldestFirst)));
        assert_eq!(app.order, Order::OldestFirst);
        let mut turned = in_folder("f:work-inbox");
        let _ = turned.update(Message::ToggleOrder);
        let _ = turned.update(Message::StoredOrder(Some(Order::NewestFirst)));
        assert_eq!(turned.order, Order::OldestFirst, "the turn wins");
        let mut hub = loaded(&[]);
        assert!(hub.orderable(), "the hub's For me lists by arrival");
        hub.view = View::Overview;
        assert!(!hub.orderable(), "the hub's Act now is sorted by what to do");
        let _ = hub.update(Message::StoredOrder(None));
        assert_eq!(hub.order, Order::NewestFirst);
    }

    #[test]
    fn an_answer_for_a_view_left_behind_is_dropped() {
        let mut app = loaded_late(&[]);
        let for_me = block_on(fetch(
            app.inbox.clone(),
            View::ForMe,
            false,
            (mail::PAGE, Order::NewestFirst),
            (app.launch.kind, Shown::Everything),
            None,
        ));
        let _ = app.update(Message::Show(View::Done));
        let _ = app.update(Message::Loaded(View::ForMe, for_me));
        assert_eq!(ids(&app).len(), FOR_ME.len(), "still the old list, not taken for Done");
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
        assert_eq!(app.for_me.len(), FOR_ME.len() - 1);
        let next = Some("demo-mail-4");
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), next, "the row in its place");
        assert_eq!(app.cursor.as_deref(), next);
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
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-18"));
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
        let _ = app.update(Message::Loaded(app.view.clone(), Err(DEMO_ERROR.into())));
        assert_eq!(app.error, Some(Failure { what: tr("Couldn't load the entries."), why: DEMO_ERROR.into() }));
        assert_eq!(demo(&["--demo", "--open", "menu"]).menu, Some(Menu::Nav));
        assert!(demo(&["--demo", "--open", "drawer"]).drawer);
        // The triage states of E3.2.
        let app = loaded(&["--open", "snooze"]);
        assert_eq!((app.menu, app.cursor.as_deref()), (Some(Menu::Snooze), Some("demo-18")));
        assert_eq!(loaded(&["--open", "pick"]).menu, Some(Menu::Pick));
        let app = loaded(&["--open", "lane"]);
        assert_eq!((app.menu, app.cursor.as_deref()), (Some(Menu::Lane), Some("demo-18")));
        // The first entry's source: the ntfy alert (task E5.1).
        let first = SourceRef::ntfy("ntfy.sh");
        let failed = loaded(&["--open", "failed"]);
        assert!(failed.loaded && !failed.items.is_empty(), "the list is there");
        assert_eq!(failed.error, Some(Failure { what: tr("Couldn't change the entry."), why: DEMO_ERROR.into() }));
        let app = loaded(&["--open", "mute"]);
        let hint = app.hint.as_ref().expect("the hint after muting");
        assert_eq!(hint.text, trf("New entries from {source} muted", &[("source", &first.label())]));
        assert_eq!(hint.undo, Some(Undo::Mute(first, false)));
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
        assert_eq!(
            labels,
            [tr("Act now"), tr("To sort"), tr("Later"), tr("Done today"), tr("Insights")],
            "the hub's (tasks E8.5, E8.8b)"
        );
        assert_eq!(
            views.iter().map(NavEntry::trail).collect::<Vec<_>>()[..2],
            [Trail::Count(18), Trail::Count(14)],
            "Act now counts what is for the user, To sort what of it is to sort"
        );
        let by_source: usize = sources.iter().filter_map(|e| e.count).sum();
        assert_eq!(by_source, 18 + 6, "the sources add up to For me plus Info");
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

    fn press<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
        app: &mut App<S>,
        key: Key,
    ) -> Task<Message> {
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
            Some(Sent::Decide(approval, allow)) => {
                let result = block_on(app.inbox.decide_approval(approval, allow));
                let _ = app.update(Message::Decided(result, allow));
            }
            Some(Sent::DismissAll(ids)) => {
                for id in &ids {
                    block_on(app.inbox.dismiss_item(id.clone())).expect("the demo dismisses");
                }
                let _ = app.update(Message::DismissedAll(ids, None));
            }
            Some(Sent::MuteOptions(id)) => {
                let result = block_on(app.inbox.list_mute_options(id.clone())).map_err(|err| err.to_string());
                let _ = app.update(Message::MuteOptions(id, result));
            }
            Some(Sent::MuteItem(id, scope, until, effect)) => {
                let result = block_on(app.inbox.mute_item(id, scope, until, effect)).map_err(|err| err.to_string());
                let _ = app.update(Message::MutedBy(result, effect));
            }
            Some(Sent::Unmute(rule, reopen, moved)) => {
                let back = reopen.first().or(moved.first()).cloned();
                let result: Result<(), String> = (|| {
                    if let Some(rule) = rule {
                        block_on(app.inbox.remove_rule(rule)).map_err(|err| err.to_string())?;
                    }
                    for id in reopen {
                        block_on(app.inbox.set_item_done(id, false)).map_err(|err| err.to_string())?;
                    }
                    for id in moved {
                        block_on(app.inbox.set_item_lane(id, Lane::ForMe, false)).map_err(|err| err.to_string())?;
                    }
                    Ok(())
                })();
                let _ = app.update(Message::Undone(result, back));
            }
            Some(Sent::Reopen(ids)) => {
                let back = ids.first().cloned();
                let result = ids.into_iter().try_for_each(|id| {
                    block_on(app.inbox.set_item_done(id, false)).map(|_| ()).map_err(|err| err.to_string())
                });
                let _ = app.update(Message::Undone(result, back));
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
        assert_eq!(cursor(&app), Some("demo-18"), "down starts at the top");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-1"));
        let _ = press(&mut app, Key::Up);
        let _ = press(&mut app, Key::Up);
        assert_eq!(cursor(&app), Some("demo-18"), "stops at the first row");
        let mut app = loaded(&[]);
        let _ = press(&mut app, Key::Up);
        assert_eq!(cursor(&app), Some("demo-mail-8"), "up starts at the bottom");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-mail-8"), "stops at the last row");
        let mut empty = loaded(&["--open", "empty"]);
        let _ = press(&mut empty, Key::Down);
        assert_eq!(cursor(&empty), None);
    }

    #[test]
    fn moving_scrolls_the_cursor_into_view() {
        let mut app = loaded(&[]);
        // The group head and two rows fit.
        let (_, second) = list::row_span(&app.items, 1, Look::Rows, 480.0).expect("a second row");
        let _ = app.update(Message::Scrolled(0.0, second));
        assert_eq!(press(&mut app, Key::Down).units(), 0, "the first row is in view");
        let _ = press(&mut app, Key::Down);
        assert!(press(&mut app, Key::Down).units() > 0, "the third row needs a scroll");
        let (_, third) = list::row_span(&app.items, 2, Look::Rows, 480.0).expect("a third row");
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
        assert_eq!(cursor(&app), Some("demo-17"), "the Talk chat after the waiting ones");
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
        let after = After { text: tr("Done").into(), undo: Some(reopen.clone()) };
        assert_eq!(app.sent.last(), Some(&Sent::Edit(done, Some(after))));
        deliver(&mut app);
        assert!(!ids(&app).contains(&"demo-1"));
        assert_eq!(cursor(&app), Some("demo-3"), "on to the next row");
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Done")));
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(reopen, None)));
        assert!(app.hint.is_none(), "the hint goes with the undo");
        deliver(&mut app);
        assert_eq!(ids(&app)[..2], ["demo-18", "demo-1"], "back in its place, after the alert");
        assert_eq!(cursor(&app), Some("demo-1"), "and the cursor with it");
        assert!(press(&mut app, Key::U).units() == 0 && app.sent.is_empty(), "nothing left to undo");
    }

    #[test]
    fn e_reopens_a_done_entry() {
        let mut app = loaded_late(&["--open", "done"]);
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
    fn a_reminder_comes_back_in_five_minutes_first() {
        let mut app = at("demo-2");
        let _ = press(&mut app, Key::S);
        if let Some(item) = app.subject.as_mut() {
            item.kind = torro_inbox::Kind::Reminder;
        }
        let choices = app.choices();
        assert_eq!(choices[0].label, tr("In 5 min"));
        let soon = Edit::Snooze { id: "demo-2".into(), until: Some(app.now + TimeDelta::minutes(5)) };
        assert!(matches!(&choices[0].message, Message::Edit(edit) if *edit == soon), "snoozes the entry for 5 min");
        assert_eq!(choices.len(), 5, "and the usual ones after it");
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
        let text = trf("Later until {time}", &[("time", &list::wake_text(tomorrow, app.now))]);
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
        assert_eq!(app.choices().last().map(|c| c.label.clone()), Some(tr("Bring back now").to_owned()));
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

    /// `m`, then the hide menu's last entry ("Mute …" for a script's notice).
    fn mute_by_keys(app: &mut App<DemoSource>) {
        let _ = press(app, Key::M);
        assert_eq!((app.menu, app.focus()), (Some(Menu::Hide), Focus::Choices));
        let last = app.choices().len();
        let _ = press(app, Key::Digit(u8::try_from(last).expect("a digit")));
    }

    #[test]
    fn m_offers_this_entry_and_muting_for_a_notice() {
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::M);
        let labels: Vec<String> = app.choices().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [tr("This entry").to_owned(), trf("Mute {source}", &[("source", &"deploy")])]);
        let _ = press(&mut app, Key::Digit(1));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { done: true, .. }, Some(_)))), "done, undo");
        assert!(app.menu.is_none());
    }

    #[test]
    fn the_hide_menu_mutes_at_once_and_u_unmutes() {
        let mut app = at("demo-6");
        let deploy = SourceRef::script("deploy");
        mute_by_keys(&mut app);
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
        mute_by_keys(&mut app);
        deliver(&mut app);
        let _ = press(&mut app, Key::M);
        assert_eq!(
            app.choices().last().map(|c| c.label.clone()),
            Some(trf("Unmute {source}", &[("source", &"deploy")]))
        );
        let _ = press(&mut app, Key::Digit(2));
        assert_eq!(app.sent.last(), Some(&Sent::Mute(deploy.clone(), false, true)));
        deliver(&mut app);
        assert_eq!(app.hint.and_then(|hint| hint.undo), Some(Undo::Mute(deploy, true)));
    }

    #[test]
    fn the_suite_is_never_muted_and_an_approval_not_hidden() {
        let mut app = at("demo-3");
        let _ = press(&mut app, Key::M);
        assert!(app.sent.is_empty() && app.menu.is_none());
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("An approval closes once it is decided."))
        );
        // An account problem of the suite: done, but never muted.
        let mut app = at("demo-5");
        let _ = press(&mut app, Key::M);
        let labels: Vec<String> = app.choices().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [tr("This entry").to_owned()]);
        let _ = app.update(Message::AskMute("demo-5".into()));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Mute(..))));
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("Entries from the suite can't be muted."))
        );
    }

    /// The first mail entry of "For me", under the cursor, and its sender's address.
    fn at_mail() -> (App<DemoSource>, Item, String) {
        let app = loaded(&[]);
        let item = app.items.iter().find(|item| item.source.kind == SourceKind::Mail).cloned().expect("mail in For me");
        let address = rules::mail_sender(&item).expect("a sender");
        (at(&item.id), item, address)
    }

    /// Opens the bell's menu of the entry under the cursor with `m` and lets the core answer.
    fn bell_by_keys(app: &mut App<DemoSource>) {
        let _ = press(app, Key::M);
        assert_eq!((app.menu, app.focus()), (Some(Menu::Mute), Focus::Mute));
        deliver(app);
    }

    /// Issue #123: mail has one menu to mute – `m` opens the bell's menu with the core's ways for the
    /// entry, then "Move to junk" after a line; the domain picked mutes for as long as "For" says,
    /// the mail stays unread, and the undo brings the entry back.
    #[test]
    fn m_opens_the_bells_menu_for_mail() {
        let mut app = loaded(&["--open", "info"]);
        let sale = app.items.iter().find(|item| item.title == "Autumn sale: 20% off").cloned().expect("the sale");
        app.cursor = Some(sale.id.clone());
        bell_by_keys(&mut app);
        let offered: Vec<String> =
            rules::mute_options(&stored(&app, &sale.id)).into_iter().map(|option| option.label).collect();
        let choices = app.choices();
        let labels: Vec<String> = choices.iter().map(|choice| choice.label.clone()).collect();
        assert_eq!(labels[..offered.len()], offered[..], "the core's ways first");
        assert_eq!(labels.last().map(String::as_str), Some(tr("Move to junk")), "no \"Always to Info\" any more");
        assert!(choices.last().is_some_and(|choice| choice.line), "after a line");
        let domain = MuteScope::Facet { key: FacetKey::Domain, nth: 0 };
        let place = choices.iter().position(|c| matches!(c.message, Message::MuteBy(_, scope) if scope == domain));
        let digit = place.map(|at| u8::try_from(at + 1).expect("a digit")).expect("the domain is offered");
        let _ = app.update(Message::MuteFor(MuteFor::OneWeek));
        let _ = app.update(Message::MuteHow(MuteEffect::Hide));
        let _ = press(&mut app, Key::Digit(digit));
        let week = Some(app.now + TimeDelta::days(7));
        assert_eq!(app.sent.last(), Some(&Sent::MuteItem(sale.id.clone(), domain, week, MuteEffect::Hide)));
        deliver(&mut app);
        assert!(!ids(&app).contains(&sale.id.as_str()), "out of the inbox");
        let Some(Link::Mail { message }) = &sale.link else { panic!("a mail entry") };
        let flags = app.inbox.get_mail(message).map(|mail| mail.summary.flags).expect("the mail");
        assert!(!flags.contains(&torro_mail::Flag::Seen), "muting leaves the mail unread");
        let _ = press(&mut app, Key::U);
        deliver(&mut app);
        assert!(ids(&app).contains(&sale.id.as_str()), "the undo brings it back");
    }

    /// Issue #123 (acceptance): "This conversation" for a week keeps a rule, so further mail of the
    /// conversation stays out of "For me"; hidden for good it is done without marking the mail read,
    /// as "Hide from Notification Hub" did, and the undo of the suite takes it back.
    #[test]
    fn this_conversation_is_muted_without_marking_the_mail_read_and_undoes() {
        let mut app = loaded(&[]);
        let plans = app.items.iter().find(|item| item.title == "Weekend plans").cloned().expect("a mail entry");
        app.cursor = Some(plans.id.clone());
        bell_by_keys(&mut app);
        let _ = app.update(Message::MuteHow(MuteEffect::Hide));
        let _ = press(&mut app, Key::Digit(1));
        assert_eq!(
            app.sent.last(),
            Some(&Sent::MuteItem(plans.id.clone(), MuteScope::Conversation, None, MuteEffect::Hide))
        );
        deliver(&mut app);
        let hint = app.hint.clone().expect("a hint");
        let undo = Undo::Muted { rule: None, reopen: vec![plans.id.clone()], moved: Vec::new() };
        assert_eq!((hint.text.as_str(), hint.undo), (tr("Conversation hidden"), Some(undo)));
        assert!(!ids(&app).contains(&plans.id.as_str()), "out of the inbox");
        let Some(Link::Mail { message }) = &plans.link else { panic!("a mail entry") };
        let flags = app.inbox.get_mail(message).map(|mail| mail.summary.flags).expect("the mail");
        assert!(!flags.contains(&torro_mail::Flag::Seen), "unlike done, the mail stays unread");
        let _ = app.update(Message::Undo);
        deliver(&mut app);
        assert!(ids(&app).contains(&plans.id.as_str()), "back");

        // For a week: a rule for the conversation, which holds for its next mail.
        bell_by_keys(&mut app);
        let _ = app.update(Message::MuteFor(MuteFor::OneWeek));
        let _ = press(&mut app, Key::Digit(1));
        let week = Some(app.now + TimeDelta::days(7));
        assert_eq!(
            app.sent.last(),
            Some(&Sent::MuteItem(plans.id.clone(), MuteScope::Conversation, week, MuteEffect::Hide))
        );
        deliver(&mut app);
        let Some(Undo::Muted { rule: Some(rule), .. }) = app.hint.clone().and_then(|hint| hint.undo) else {
            panic!("a rule to take back")
        };
        // Asked again, the demo answers the rule already kept.
        let again = block_on(app.inbox.mute_item(plans.id.clone(), MuteScope::Conversation, week, MuteEffect::Hide));
        let kept = again.ok().and_then(|muted| muted.rule).expect("the rule");
        assert_eq!(kept.id, rule);
        let next = Item { id: "next".into(), state: State::New, ..stored(&app, &plans.id) };
        assert!(rules::mutes(&kept, &next, app.now), "a further mail of the conversation is held back");
    }

    #[test]
    fn later_and_done_undo_and_escape_takes_the_toast_away() {
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::S);
        let _ = press(&mut app, Key::Digit(1));
        deliver(&mut app);
        let hint = app.hint.clone().expect("a toast");
        assert!(matches!(hint.undo, Some(Undo::Edit(Edit::Snooze { ref id, until: None })) if id == "demo-6"));
        let _ = app.update(Message::Undo);
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Snooze { until: None, .. }, None))), "no longer later");
        deliver(&mut app);
        assert!(ids(&app).contains(&"demo-6"));

        let _ = press(&mut app, Key::E);
        deliver(&mut app);
        assert!(app.can_undo(), "done offers undo");
        let _ = app.update(Message::Select("demo-1".into()));
        let _ = app.update(Message::Escape);
        assert!(app.hint.is_none() && app.detail.is_some(), "Escape takes the toast first, the panel stays");
        let _ = press(&mut app, Key::U);
        assert!(app.sent.is_empty(), "nothing left to undo");
    }

    /// Issue #123: the sender is muted from the bell's menu – the toast in the words of the rules –,
    /// and "Move to junk", the menu's last way, moves the mail with an undo.
    #[test]
    fn the_sender_is_muted_and_mail_goes_to_junk_with_an_undo() {
        let (mut app, offer, address) = at_mail();
        bell_by_keys(&mut app);
        let _ = app.update(Message::MuteHow(MuteEffect::Hide));
        let sender = MuteScope::Facet { key: FacetKey::Sender, nth: 0 };
        let place =
            app.choices().iter().position(|c| matches!(c.message, Message::MuteBy(_, scope) if scope == sender));
        let _ =
            press(&mut app, Key::Digit(place.map(|at| u8::try_from(at + 1).expect("a digit")).expect("the sender")));
        assert_eq!(app.sent.last(), Some(&Sent::MuteItem(offer.id.clone(), sender, None, MuteEffect::Hide)));
        deliver(&mut app);
        let what = trf("Mail from {sender}", &[("sender", &address)]);
        let text = trf("Hidden: {what}", &[("what", &what)]);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.clone()), Some(text));

        let (mut app, item, _) = at_mail();
        bell_by_keys(&mut app);
        let junk = app.choices().len();
        let _ = press(&mut app, Key::Digit(u8::try_from(junk).expect("a digit")));
        let Some(Sent::Mail(MailEdit::Move { to, .. }, Some(hint))) = app.sent.last() else {
            panic!("a move with a hint: {:?}", app.sent.last())
        };
        assert_eq!(*to, MoveTarget::Role(MoveRole::Junk));
        assert!(matches!(&hint.undo, Some(Undo::Mail { reopen: Some(id), .. }) if *id == item.id));
    }

    #[test]
    fn the_bells_menu_is_wide_enough_for_whole_addresses() {
        let (mut app, _, _) = at_mail();
        bell_by_keys(&mut app);
        let choices = app.choices();
        let width = mute_menu_width(&choices);
        assert!(width > widgets::menu_width(), "wider than a plain menu: {width}");
        for (i, choice) in choices.iter().enumerate() {
            let shown = choice_label(&choice.label, None, keys::digit(i + 1), width);
            assert_eq!(shown, choice.label, "whole, not \"jonas.…\"");
        }
        let short = Choice::new(Icon::Sun, "Soon", Message::Refresh);
        assert_eq!(choice_menu_width(&[short]), widgets::menu_width(), "short labels: a plain menu");
        let snooze = loaded(&["--open", "snooze"]);
        let width = choice_menu_width(&snooze.choices());
        for (i, choice) in snooze.choices().iter().enumerate() {
            let shown = choice_label(&choice.label, choice.when.as_deref(), keys::digit(i + 1), width);
            assert_eq!(shown, choice.label, "every language's snooze times fit");
        }
        // Beyond the widest menu a label still ends in "…".
        let long = Choice::new(Icon::User, "x".repeat(200), Message::Refresh);
        let width = choice_menu_width(std::slice::from_ref(&long));
        assert_eq!(width, size::panel());
        let shown = choice_label(&long.label, None, "1", width);
        assert!(shown.ends_with(typo::ELLIPSIS) && shown.len() < long.label.len(), "{shown}");
    }

    /// `--open hide` and `hide-panel` show the menu of `m` of the first mail entry – the bell's menu
    /// since issue #123 –, `mute-mail` the bell's menu of a folder's message without an entry.
    #[test]
    fn open_hide_shows_the_bells_menu_of_the_first_mail_entry() {
        for (open, panel) in [("hide", false), ("hide-panel", true)] {
            let mut app = loaded(&["--open", open]);
            assert_eq!(app.menu, Some(Menu::Mute), "{open}");
            let subject = app.subject.clone().expect("a subject");
            assert_eq!((subject.source.kind, app.cursor.as_deref()), (SourceKind::Mail, Some(subject.id.as_str())));
            assert_eq!(app.detail.is_some(), panel, "{open}");
            if !panel {
                deliver(&mut app);
            }
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
        let mut app = loaded(&["--open", "mute-mail"]);
        load(&mut app);
        assert_eq!(app.menu, Some(Menu::Mute));
        let subject = app.subject.clone().expect("the message");
        assert!(rules::is_message_id(&subject.id) && app.cursor.as_deref() == Some(subject.id.as_str()));
        deliver(&mut app);
        assert!(!app.choices().is_empty(), "its traits");
        for size in SIZES {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    /// Issue #123: a folder's message without an entry is muted by its own traits – no
    /// conversation, no junk –, from its row (`m`, the bell) and from the reader; one with an entry
    /// goes through its entry.
    #[test]
    fn a_message_without_an_entry_is_muted_by_its_traits() {
        let mut app = loaded(&["--open", "folder"]);
        load(&mut app);
        // No entry at all, not even a snoozed or done one (that one's menu it would open).
        let all = block_on(app.inbox.list_items(ItemFilter::default())).expect("the demo answers");
        let filed =
            |id: &str| all.iter().any(|item| matches!(&item.link, Some(Link::Mail { message }) if message == id));
        let alone = app.messages.iter().map(|message| message.id.clone()).find(|id| !filed(id));
        let alone = alone.expect("a message without an entry");
        let _ = app.update(Message::Act(alone.clone(), Command::Hide));
        assert_eq!(app.sent.last(), Some(&Sent::MuteOptions(alone.clone())));
        assert_eq!((app.menu, app.menu_at), (Some(Menu::Mute), MenuAt::Row));
        deliver(&mut app);
        let choices = app.choices();
        assert!(!choices.is_empty(), "its traits");
        assert!(
            choices.iter().all(|c| matches!(c.message, Message::MuteBy(_, MuteScope::Facet { .. }))),
            "no conversation, no junk"
        );
        let sender = MuteScope::Facet { key: FacetKey::Sender, nth: 0 };
        let place = choices.iter().position(|c| matches!(c.message, Message::MuteBy(_, scope) if scope == sender));
        let _ =
            press(&mut app, Key::Digit(place.map(|at| u8::try_from(at + 1).expect("a digit")).expect("the sender")));
        assert_eq!(app.sent.last(), Some(&Sent::MuteItem(alone.clone(), sender, None, app.mute_effect)));
        deliver(&mut app);
        assert!(app.hint.as_ref().is_some_and(|hint| hint.undo.is_some()), "a new rule to take back");

        // The reader's bell, and `m` on the row, do the same.
        let _ = app.update(Message::AskMuteMail(alone.clone()));
        assert_eq!((app.menu, app.menu_at), (Some(Menu::Mute), MenuAt::Panel));
        let _ = app.update(Message::CloseMenu);
        app.cursor = Some(alone.clone());
        let _ = press(&mut app, Key::M);
        assert_eq!(app.sent.last(), Some(&Sent::MuteOptions(alone)));

        // A message of an entry opens its entry's menu.
        let filed = app.messages.iter().find_map(|message| mail::entry_of(&app.for_me, &message.id)).cloned();
        let entry = filed.expect("a message with an entry in the inbox");
        let Some(Link::Mail { message }) = entry.link.clone() else { panic!("a mail entry") };
        let _ = app.update(Message::AskMuteMail(message));
        assert_eq!(app.sent.last(), Some(&Sent::MuteOptions(entry.id)));
    }

    #[test]
    fn mail_has_no_hide_menu_but_the_bell() {
        let (mut app, item, _) = at_mail();
        assert!(app.hide_choices(&item).is_empty(), "mail's \"Hide from inbox\" went into the bell's menu");
        assert!(app.hidable(&item), "the bell instead");
        let done = Item { state: State::Done { at: app.now }, ..item.clone() };
        assert!(app.hidable(&done), "a done mail's sender can still be muted");
        bell_by_keys(&mut app);
        assert!(app.choices().iter().any(|c| c.label == tr("Move to junk")));
        app.folders.retain(|folder| folder.role != Some(Role::Junk));
        assert!(!app.choices().iter().any(|c| c.label == tr("Move to junk")), "no junk folder, no junk");
        let other = app.items.iter().find(|item| item.source.kind != SourceKind::Mail && !detail::is_approval(item));
        let other = other.cloned().expect("an entry of another source");
        assert!(!app.hide_choices(&other).is_empty(), "the other sources keep \"Hide from inbox\"");
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
        let _ = app.update(Message::AskHide("demo-6".into()));
        assert_eq!(app.menu, Some(Menu::Hide));
        let _ = app.update(Message::AskMute("demo-6".into()));
        assert!(app.menu.is_none(), "muting closes the menu");
        assert_eq!(app.sent.last(), Some(&Sent::Mute(SourceRef::script("deploy"), true, true)));
        let _ = app.update(Message::SetDone("demo-6".into(), true));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { done: true, .. }, Some(_)))), "with undo");
    }

    // ----- Deciding approvals (E3.8) ----------------------------------------------------------

    /// The approval of the demo, opened in the panel.
    fn with_approval() -> (App<DemoSource>, String) {
        let app = loaded(&["--open", "approval"]);
        let detail = app.detail.as_ref().expect("the approval in the panel");
        let approval = detail.actions.iter().find_map(|action| match action {
            torro_inbox::Action::Allow { approval } => Some(approval.clone()),
            torro_inbox::Action::Deny { .. } => None,
        });
        (app, approval.expect("an approval carries Allow"))
    }

    #[test]
    fn allow_decides_once_then_the_entry_is_done_with_a_hint() {
        let (mut app, approval) = with_approval();
        let entry = app.detail.as_ref().map(|item| item.id.clone()).expect("panel");
        let _ = app.update(Message::Decide { approval: approval.clone(), allow: true });
        assert_eq!(app.deciding.as_deref(), Some(approval.as_str()), "the buttons wait for the answer");
        // A second click before the answer sends nothing.
        let _ = app.update(Message::Decide { approval: approval.clone(), allow: false });
        assert_eq!(app.sent, [Sent::Decide(approval.clone(), true)]);
        deliver(&mut app);
        assert_eq!(app.deciding, None);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Allowed")));
        assert!(!ids(&app).contains(&entry.as_str()), "decided, the approval left For me");
    }

    #[test]
    fn deny_says_denied_and_a_settled_approval_is_only_a_hint() {
        let (mut app, approval) = with_approval();
        let _ = app.update(Message::Decide { approval: approval.clone(), allow: false });
        deliver(&mut app);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Denied")));

        let _ = app.update(Message::Decide { approval, allow: true });
        deliver(&mut app);
        let settled = InboxError::Settled.to_string();
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.clone()), Some(settled));
        assert!(app.error.is_none(), "decided elsewhere or expired is no failure");
    }

    #[test]
    fn a_failed_decision_is_an_error_and_frees_the_buttons() {
        let (mut app, approval) = with_approval();
        let _ = app.update(Message::Decide { approval, allow: true });
        let _ = app.update(Message::Decided(Err(InboxError::Offline), true));
        assert_eq!(app.deciding, None);
        assert!(app.error.is_some());
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
        let Some(Sent::Task { id, list, title, due, .. }) = app.sent.pop() else { panic!("no task sent") };
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
                notes: None,
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

    // ----- Make event and quiet (task E8.5b) --------------------------------------------------

    /// The demo's calendars, handed to `update` as the task of the dialog would.
    /// Issue #120: "AI task" opens the dialog of "Make task" while the AI is asked, fills it in with
    /// the suggestion, and "Create task" sends its note too; nothing is saved before.
    #[test]
    fn ai_task_fills_the_dialog_and_creates_the_task_with_its_note() {
        let mut app = at("demo-6");
        let _ = app.update(Message::AiReady(true));
        let _ = app.update(Message::SuggestTask("demo-6".into()));
        assert_eq!(
            (app.menu, app.ai_suggesting.clone()),
            (Some(Menu::Task), Some(Suggesting::Asking("demo-6".into())))
        );
        assert_eq!(app.sent.last(), Some(&Sent::SuggestTask("demo-6".into())));
        assert!(app.sent.contains(&Sent::Lists), "the lists are asked for as for Make task");
        let lists = block_on(app.inbox.list_task_lists()).map_err(|err| err.to_string());
        let _ = app.update(Message::TaskLists(lists));
        let suggestion = block_on(app.inbox.suggest_task("demo-6".into())).expect("the demo's AI");
        let due = ai::due_of(suggestion.due).expect("a day");
        let _ = app.update(Message::TaskSuggested("demo-6".into(), Ok(suggestion.clone())));
        assert_eq!(app.ai_suggesting, Some(Suggesting::Done));
        assert_eq!(app.task.title, suggestion.title);
        assert_eq!(app.task.due.day(app.now), Some(due), "the date the AI read");
        assert!(!app.task.notes.is_empty(), "the demo's note");
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Task { .. })), "nothing saved yet");
        let _ = app.update(Message::TaskNotes("Bring the logs".into()));
        let _ = press(&mut app, Key::Enter);
        let Some(Sent::Task { id, title, due: sent_due, notes, .. }) = app.sent.last().cloned() else {
            panic!("a task: {:?}", app.sent.last())
        };
        assert_eq!((id.as_str(), title, sent_due), ("demo-6", Some(suggestion.title), Some(due)));
        assert_eq!(notes.as_deref(), Some("Bring the logs"));
        assert_eq!(app.ai_suggesting, None, "the dialog closed");
    }

    /// Issue #120: without an AI the buttons are off; their message only says to set one up, and an
    /// answer that comes after its dialog closed changes nothing.
    #[test]
    fn without_an_ai_nothing_is_asked_and_late_answers_are_dropped() {
        let mut app = at("demo-6");
        let _ = app.update(Message::AiReady(false));
        let _ = app.update(Message::SuggestEvent("demo-6".into()));
        assert_eq!(app.menu, None);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Set up an AI first")));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::SuggestEvent(_))));

        let _ = app.update(Message::AiReady(true));
        let _ = app.update(Message::SuggestEvent("demo-6".into()));
        let _ = app.update(Message::CloseMenu);
        let late = block_on(app.inbox.suggest_event("demo-6".into())).map_err(|err| err.to_string());
        let _ = app.update(Message::EventSuggested("demo-6".into(), late));
        assert_eq!((app.menu, app.ai_suggesting.clone()), (None, None));
        // "Make event" by hand: no AI line, no answer taken.
        let _ = press(&mut app, Key::C);
        let title = app.event.title.clone();
        let late = block_on(app.inbox.suggest_event("demo-6".into())).map_err(|err| err.to_string());
        let _ = app.update(Message::EventSuggested("demo-6".into(), late));
        assert_eq!((app.ai_suggesting.clone(), app.event.title.clone()), (None, title));
        // An AI that fails says why in the dialog, which stays.
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::SuggestTask("demo-6".into()));
        let _ = app.update(Message::TaskSuggested("demo-6".into(), Err("The AI server could not be reached.".into())));
        assert_eq!(app.ai_suggesting, Some(Suggesting::Failed("The AI server could not be reached.".into())));
        assert_eq!(app.menu, Some(Menu::Task));
    }

    /// Issue #120: "AI event" fills in the day, start and length the AI read, the place and who
    /// takes part, and "Create event" sends place and note.
    #[test]
    fn ai_event_fills_the_dialog_with_its_time_and_place() {
        let mut app = at("demo-6");
        let _ = app.update(Message::AiReady(true));
        let _ = app.update(Message::SuggestEvent("demo-6".into()));
        assert_eq!(app.menu, Some(Menu::Event));
        let calendars = block_on(app.inbox.list_calendars()).map_err(|err| err.to_string());
        let _ = app.update(Message::Calendars(calendars));
        let suggestion = block_on(app.inbox.suggest_event("demo-6".into())).expect("the demo's AI");
        let _ = app.update(Message::EventSuggested("demo-6".into(), Ok(suggestion.clone())));
        let (day, start, minutes) = ai::event_times(&suggestion, 30).expect("a time");
        assert_eq!((app.event.day, app.event.start, app.event.length), (day, StartTime(start), Length::Minutes(45)));
        assert_eq!(minutes, 45);
        assert_eq!(app.event.location, "Room 2");
        assert_eq!(app.event.notes, ai::attendees_note(&suggestion).unwrap_or_default());
        assert!(Length::Minutes(45).offered().contains(&Length::Minutes(45)));
        assert_eq!(Length::of_minutes(60), Length::Hour);
        let _ = press(&mut app, Key::Enter);
        let Some(Sent::Event { location, end, start: from, .. }) = app.sent.last().cloned() else {
            panic!("an event: {:?}", app.sent.last())
        };
        assert_eq!((location.as_deref(), end - from), (Some("Room 2"), TimeDelta::minutes(45)));
    }

    /// Issue #120: `--open ai-task` opens the dialog the AI fills in, `--open no-ai` keeps the AI
    /// buttons off whatever the demo answers.
    #[test]
    fn the_ai_states_open_with_the_demo() {
        let mut app = demo(&["--demo", "--open", "ai-task", "--now", "2026-10-13T10:30"]);
        load(&mut app);
        assert!(app.ai_ready);
        assert_eq!(app.menu, Some(Menu::Task));
        assert!(matches!(app.ai_suggesting, Some(Suggesting::Asking(_))));
        let mut off = demo(&["--demo", "--open", "no-ai", "--now", "2026-10-13T10:30"]);
        load(&mut off);
        let _ = off.update(Message::AiReady(true));
        assert!(!off.ai_ready && off.detail.is_some());
    }

    #[test]
    fn a_suggested_day_is_offered_with_its_date() {
        let now = Utc.with_ymd_and_hms(2026, 10, 14, 8, 0, 0).single().expect("valid time");
        let today = local_today(now);
        assert_eq!(Due::of_day(today + Days::new(1), now), Due::Tomorrow);
        let later = today + Days::new(9);
        assert_eq!(Due::of_day(later, now), Due::On(later));
        assert_eq!(Due::On(later).offered().last(), Some(&Due::On(later)));
        assert_eq!(Due::Today.offered(), Due::ALL.to_vec());
        let quarter = StartTime(NaiveTime::from_hms_opt(10, 15, 0).expect("time"));
        let offered = quarter.offered();
        assert!(offered.windows(2).all(|pair| pair[0] < pair[1]) && offered.contains(&quarter));
    }

    fn deliver_calendars(app: &mut App<DemoSource>) {
        assert_eq!(app.sent.last(), Some(&Sent::Calendars), "the dialog asked for the calendars");
        let calendars = block_on(app.inbox.list_calendars()).map_err(|err| err.to_string());
        let _ = app.update(Message::Calendars(calendars));
    }

    fn demo_calendar(index: usize) -> TaskList {
        let (id, name) = hub::DEMO_CALENDARS[index];
        TaskList { id: id.into(), name: name.into() }
    }

    #[test]
    fn c_opens_the_event_dialog_and_create_sends_the_event_then_the_entry_is_done() {
        let mut app = at("demo-6");
        let title = app.find("demo-6").map(|item| item.title.clone()).expect("demo-6");
        let _ = press(&mut app, Key::C);
        assert_eq!((app.menu, app.subject.as_ref().map(|i| i.id.as_str())), (Some(Menu::Event), Some("demo-6")));
        assert_eq!(app.event.title, title, "the entry's title to change");
        assert_eq!(app.event.calendar, None, "no calendar before the calendars are in");
        deliver_calendars(&mut app);
        assert_eq!(app.event.calendar, Some(demo_calendar(0)), "the first calendar");

        let tomorrow = local_today(app.now) + Days::new(1);
        let nine = StartTime(NaiveTime::from_hms_opt(9, 0, 0).expect("a time"));
        let _ = app.update(Message::EventTitle("Call the CI team".into()));
        let _ = app.update(Message::EventCalendar(demo_calendar(1)));
        let _ = app.update(Message::EventDay(tomorrow));
        let _ = app.update(Message::EventStart(nine));
        let _ = app.update(Message::EventLength(Length::Hour));
        // The keys of the list stay out while the dialog is open; Enter creates the event.
        let _ = press(&mut app, Key::E);
        assert_eq!(app.menu, Some(Menu::Event), "e is no triage key in the dialog");
        let _ = press(&mut app, Key::Enter);
        let start = datetime::to_utc(tomorrow.and_time(nine.0));
        assert_eq!(
            app.sent.last(),
            Some(&Sent::Event {
                id: "demo-6".into(),
                calendar: demo_calendar(1),
                title: Some("Call the CI team".into()),
                start,
                end: start + TimeDelta::hours(1),
                location: None,
                notes: None,
            })
        );
        assert!(app.menu.is_none(), "the dialog closes");
        let Some(Sent::Event { id, calendar, title, start, end, .. }) = app.sent.pop() else { panic!("no event sent") };
        let made = block_on(app.inbox.create_event_from_item(id, calendar.id, title, start, end));
        let _ = app.update(Message::EventCreated(made.map_err(|err| err.to_string()), calendar.name.clone()));
        load(&mut app);
        let hint = app.hint.as_ref().expect("a hint");
        assert_eq!(hint.text, trf("Event created in {calendar}", &[("calendar", &calendar.name)]));
        assert!(!ids(&app).contains(&"demo-6"), "the entry is done and left For me");

        // The next one starts in the calendar used last.
        let next = app.items.iter().find(|item| !detail::is_approval(item)).map(|item| item.id.clone());
        let _ = app.update(Message::AskEvent(next.expect("another entry")));
        deliver_calendars(&mut app);
        assert_eq!(app.event.calendar, Some(demo_calendar(1)), "the calendar used last");
    }

    #[test]
    fn create_needs_a_calendar_and_an_approval_becomes_no_event() {
        let mut app = at("demo-6");
        let _ = press(&mut app, Key::C);
        let _ = app.update(Message::EventConfirm);
        assert_eq!(app.sent.last(), Some(&Sent::Calendars), "nothing sent while the calendars load");
        let _ = app.update(Message::Calendars(Ok(Vec::new())));
        let _ = app.update(Message::EventConfirm);
        assert_eq!((app.menu, app.sent.last()), (Some(Menu::Event), Some(&Sent::Calendars)), "no calendar");
        let _ = app.update(Message::Calendars(Ok(vec![demo_calendar(0)])));
        let _ = app.update(Message::EventTitle("   ".into()));
        let _ = app.update(Message::EventConfirm);
        assert!(matches!(app.sent.last(), Some(Sent::Event { title: None, .. })), "{:?}", app.sent.last());
        let _ = app.update(Message::EventCreated(Err("offline".into()), "Work".into()));
        assert_eq!(app.error.as_ref().map(|failure| failure.what), Some(tr("Couldn't create the event.")));

        let mut approval = at("demo-3");
        let _ = press(&mut approval, Key::C);
        assert!(approval.menu.is_none());
        assert!(approval.sent.is_empty(), "the calendars are not even asked for");
    }

    #[test]
    fn an_event_starts_at_the_next_half_hour_for_half_an_hour() {
        use chrono::Timelike;
        let at = |h, m| datetime::to_utc(local_today(Utc::now()).and_hms_opt(h, m, 0).expect("a time"));
        let draft = EventDraft::new("Call".into(), at(10, 30));
        assert_eq!((draft.start.0.hour(), draft.start.0.minute(), draft.length), (11, 0, Length::Half));
        assert_eq!(EventDraft::new("Call".into(), at(10, 10)).start.0.minute(), 30);
        let late = EventDraft::new("Call".into(), at(23, 45));
        assert_eq!((late.day, late.start.0.hour()), (local_today(at(23, 45)) + Days::new(1), 0), "after midnight");
        let (start, end) = draft.span();
        assert_eq!((start, end - start), (at(11, 0), TimeDelta::minutes(30)));
        assert_eq!(StartTime::all().len(), 48, "every half hour");
        assert_eq!(Length::ALL.map(Length::minutes), [15, 30, 60, 120]);
    }

    #[test]
    fn the_event_state_opens_the_dialog_at_the_panel_and_every_size_draws_it() {
        let mut app = overview_at(&["--view", "hub", "--open", "event"]);
        assert_eq!(app.menu, Some(Menu::Event));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), app.subject.as_ref().map(|i| i.id.as_str()));
        let calendars =
            [None, Some(Ok(Vec::new())), Some(Err("offline".to_string())), Some(Ok(vec![demo_calendar(0)]))];
        for calendars in calendars {
            app.calendars = calendars;
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
    }

    /// Task E8.5b: the hub knows whether it is quiet; the line shows where no column at the right
    /// says it, and To sort offers "Event".
    #[test]
    fn the_hub_says_when_it_is_quiet() {
        let mut hub = overview_at(&["--view", "hub"]);
        let quiet = hub.quiet.clone().expect("the demo is quiet");
        assert_eq!(quiet.title.as_deref(), Some("Customer call"));
        let _ = hub.update(Message::Resized(Size::new(1600.0, 1000.0)));
        assert!(hub.hub_side() && !hub.hub_layout().quiet, "wide: the column at the right says it");
        let _ = hub.view();
        let _ = hub.update(Message::Resized(Size::new(480.0, 900.0)));
        assert!(hub.hub_layout().quiet, "narrow: a line under the numbers");
        let _ = hub.view();
        // Only the hub asks.
        assert!(loaded(&[]).quiet.is_none());
        let sort = hub::Sections::of(&hub.items, None).to_sort.first().map(|item| (*item).clone()).expect("to sort");
        assert!(hub::acts(&sort).contains(&hub::Act::Event));
    }

    // Mail (task E4.2).

    /// The view of the demo folder `id`.
    fn folder_view<S>(app: &App<S>, id: &str) -> View {
        View::Folder(FolderView::of(app.folders.iter().find(|folder| folder.id == id).expect("a demo folder")))
    }

    /// The demo app on the work account's inbox, loaded.
    fn in_folder(id: &str) -> App<DemoSource> {
        let mut app = loaded(&[]);
        let view = folder_view(&app, id);
        let _ = app.update(Message::Show(view));
        load(&mut app);
        app
    }

    fn message_ids<S>(app: &App<S>) -> Vec<&str> {
        app.messages.iter().map(|message| message.id.as_str()).collect()
    }

    /// Runs the last mail change the window started against the demo, hands the answer to
    /// `update` as the task would, and loads the view again.
    fn deliver_mail(app: &mut App<DemoSource>) {
        let Some(Sent::Mail(edit, hint)) = app.sent.pop() else { panic!("no mail change to deliver") };
        let ids = edit.ids().to_vec();
        let result = block_on(edit.apply(app.inbox.clone()));
        let _ = app.update(Message::MailEdited(result, ids, hint));
        load(app);
    }

    /// The whole mail `id` from the demo, handed to the panel as the task would.
    fn deliver_letter(app: &mut App<DemoSource>, id: &str) {
        let mail =
            block_on(MailSource::get_mail(app.inbox.as_ref(), id.into())).map(Box::new).map_err(|err| err.to_string());
        let _ = app.update(Message::LetterLoaded(id.into(), mail));
    }

    fn draw_everywhere<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
        app: &mut App<S>,
    ) {
        for size in SIZES {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    #[test]
    fn a_window_of_mail_starts_on_the_first_inbox_and_lists_folders_only() {
        let launch = Launch::parse(&args(&["--source", "mail"])).expect("valid parameters");
        assert_eq!(launch.view, Some(View::Mail));
        let mut app = loaded(&["--source", "mail"]);
        assert!(app.mail_only);
        assert_eq!(app.listed, None, "the folders are in and the inbox chosen, not yet its mail");
        load(&mut app);
        assert_eq!(app.view.folder().map(|folder| folder.id.as_str()), Some("f:work-inbox"));
        let newest_first = ["m:w1", "m:w2", "m:w3", "m:w13", "m:w4", "m:w14", "m:w5", "m:w6"];
        assert_eq!(message_ids(&app), newest_first, "newest first");
        assert_eq!(app.count_note(), "8");
        assert_eq!(app.title(), trf("Notification Hub · {view}", &[("view", &tr("Inbox"))]));
        draw_everywhere(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::UiState(UiState { sidebar: SidebarMode::Rail }));
        let _ = app.view();
    }

    #[test]
    fn without_a_mail_account_the_window_of_mail_says_so() {
        let launch = Launch::parse(&args(&["--source", "mail"])).expect("valid parameters");
        let mut app = App::new(Arc::new(EmptySource), Source::from_args(args(&["--demo"])), launch).0;
        load(&mut app);
        assert_eq!(app.view, View::Mail, "nothing to start on");
        assert!(app.loaded && app.folders.is_empty());
        draw_everywhere(&mut app);
    }

    #[test]
    fn the_compose_window_reads_what_the_panel_starts_it_with() {
        for (kind, flag) in
            [(Compose::Reply, "--reply"), (Compose::ReplyAll, "--reply-all"), (Compose::Forward, "--forward")]
        {
            let started = mail::compose_args(kind, "m:w1", true);
            let launch = Launch::parse(&started).expect("the arguments the panel starts the window with");
            let compose = launch.compose.expect("a compose window");
            assert_eq!(compose.kind.message(), Some("m:w1"), "{flag}");
            assert_eq!((launch.open, launch.view), (None, None), "it shows no inbox");
        }
        let new = Launch::parse(&compose::new_args(false)).expect("a new message");
        assert_eq!(new.compose.map(|start| start.kind), Some(compose::Kind::New));
        let account = Launch::parse(&args(&["--compose", "--account", "anna@example.org"])).expect("an account");
        assert_eq!(account.compose.and_then(|start| start.account), Some("mail:anna@example.org".into()));
    }

    #[test]
    fn compose_flags_are_refused_where_they_make_no_sense() {
        let refused = |words: &[&str]| Launch::parse(&args(words)).expect_err("refused");
        assert_eq!(refused(&["--reply", "m:w1"]), LaunchError::NeedsCompose("--reply"));
        assert_eq!(refused(&["--account", "anna@example.org"]), LaunchError::NeedsCompose("--account"));
        assert_eq!(
            refused(&["--compose", "--reply", "m:w1", "--forward", "m:w2"]),
            LaunchError::Conflict("--reply", "--forward")
        );
        assert_eq!(refused(&["--compose", "--reply"]), LaunchError::MissingValue("--reply"));
        assert_eq!(refused(&["--compose", "--open", "cc"]), LaunchError::NeedsDemo("--open"));
        assert!(matches!(refused(&["--compose", "--demo", "--open", "nope"]), LaunchError::UnknownValue { .. }));
        let demo = Launch::parse(&args(&["--compose", "--demo", "--open", "cc"])).expect("a state of the window");
        assert_eq!(demo.compose.and_then(|start| start.open), Some(compose::Open::Cc));
    }

    #[test]
    fn the_new_button_writes_a_message_when_there_is_a_mail_account() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Switcher(Switcher::New));
        assert_eq!(app.sent.last(), Some(&Sent::Compose(vec!["--compose".into(), "--demo".into()])));
        let mut none = demo(&["--demo"]);
        let _ = none.update(Message::Switcher(Switcher::New));
        assert!(!none.sent.iter().any(|sent| matches!(sent, Sent::Compose(_))), "no account, no window");
    }

    /// Issue #121: "Sources" and the mail accounts start folded; unfolded, `ui.json` keeps them and
    /// the next start shows them unfolded; folded again, they are forgotten.
    #[test]
    fn sources_and_accounts_start_folded_and_come_back_as_left() {
        let account = "mail:anna@example.org";
        let mut app = loaded(&[]);
        let _ = app.update(Message::Resized(WIDE));
        assert!(!app.sources_open, "a new start: Sources folded");
        assert!(app.mail_groups().iter().all(|group| !group.open), "a new start: accounts folded");
        assert!(app.unfolded().is_empty());
        let _ = app.update(Message::ToggleSources);
        let _ = app.update(Message::ToggleAccount(account.into()));
        let _ = app.view();
        let names = app.unfolded();
        assert_eq!(names, [SOURCES_KEY.to_owned(), account_key(account)], "what ui.json keeps");
        assert_eq!(app.unfolded_names, names, "saved as it is");

        let mut next = loaded(&[]);
        let _ = next.update(Message::Unfolded(names));
        assert!(next.sources_open, "the next start: Sources unfolded");
        assert_eq!(next.mail_groups().iter().map(|group| group.open).collect::<Vec<_>>(), [false, true]);
        let _ = next.update(Message::ToggleAccount(account.into()));
        let _ = next.update(Message::ToggleSources);
        assert!(next.unfolded().is_empty(), "folded again: forgotten");
    }

    #[test]
    fn the_sidebar_groups_the_folders_by_account() {
        let mut app = loaded(&[]);
        let groups = app.mail_groups();
        let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();
        assert_eq!(titles, ["anna@work.example.org", "anna@example.org"], "two accounts: one group each");
        assert_eq!(groups.iter().map(|group| group.open).collect::<Vec<_>>(), [false, false], "folded (issue #121)");
        let work: Vec<(&str, Option<usize>)> =
            groups[0].rows.iter().map(|row| (row.label.as_str(), row.count)).collect();
        assert_eq!(
            work,
            [
                (tr("Inbox"), Some(5)),
                (tr("Drafts"), None),
                (tr("Sent"), None),
                (tr("Archive"), None),
                (tr("Junk"), None),
                (tr("Trash"), None),
                ("Projects", None)
            ],
            "unread counted where it is news, no 0"
        );
        let _ = app.update(Message::ToggleAccount("mail:anna@work.example.org".into()));
        assert!(app.mail_groups()[0].open, "unfolded by hand");
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.view();
        let _ = app.update(Message::ToggleAccount("mail:anna@work.example.org".into()));
        assert!(!app.mail_groups()[0].open, "folded again");
        let view = folder_view(&app, "f:private-inbox");
        let _ = app.update(Message::Show(view));
        let groups = app.mail_groups();
        assert!(groups[1].open, "the account of the folder shown unfolds");
        assert!(groups[1].rows[0].active, "the folder shown is the active row");
    }

    #[test]
    fn opening_a_message_loads_the_whole_mail_and_marks_it_read() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::SelectMail("m:w1".into()));
        assert!(app.panel_open());
        assert_eq!(app.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w1"));
        assert!(app.sent.contains(&Sent::Letter("m:w1".into())));
        assert!(app.messages[0].flags.contains(&Flag::Seen), "read at once in the list");
        let read = MailEdit::Flags { ids: vec!["m:w1".into()], add: vec![Flag::Seen], remove: Vec::new() };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(read, None)), "and on the server");
        deliver_letter(&mut app, "m:w1");
        assert!(matches!(app.letter.as_ref().and_then(|letter| letter.loaded.as_ref()), Some(Ok(_))));
        draw_everywhere(&mut app);
        let _ = press(&mut app, Key::Down);
        assert_eq!(app.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w2"), "the panel follows");
        let _ = app.update(Message::Escape);
        assert!(!app.panel_open() && app.letter.is_none());
        let _ = press(&mut app, Key::Enter);
        assert_eq!(app.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w2"), "Enter opens it again");
    }

    #[test]
    fn e_archives_in_a_folder_and_undo_moves_it_back() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::E);
        assert_eq!(cursor(&app), Some("m:w2"), "the next row takes its place");
        let ids = vec!["m:w1".to_owned()];
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("an archive with a hint") };
        assert_eq!(edit, MailEdit::Move { ids: ids.clone(), to: MoveTarget::Role(MoveRole::Archive) });
        assert_eq!(hint.text, tr("Archived"));
        let back = MailEdit::Move { ids, to: MoveTarget::Folder("f:work-inbox".into()) };
        assert_eq!(hint.undo, Some(Undo::Mail { edit: back, reopen: None }));
        deliver_mail(&mut app);
        assert!(!message_ids(&app).contains(&"m:w1"));
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Archived")));
        let _ = press(&mut app, Key::U);
        deliver_mail(&mut app);
        assert!(message_ids(&app).contains(&"m:w1"), "back in the inbox, with the same id");
    }

    // Shortcuts (task E10.1).

    /// A shortcut as it arrives once the window knows whether the search field held the keyboard.
    fn shortcut(app: &mut App<DemoSource>, command: shortcuts::Command, typing: bool) -> Task<Message> {
        app.update(Message::ShortcutChecked(command, typing))
    }

    /// Archive, junk, read, flag, reply, new mail and fetch run what their keys and buttons run, on
    /// the message under the cursor; archiving works at an entry too, where `e` is Done.
    #[test]
    fn shortcuts_act_on_the_mail_under_the_cursor() {
        use shortcuts::Command as S;
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("m:w1"));
        let _ = shortcut(&mut app, S::ToggleRead, false);
        let read = MailEdit::Flags { ids: vec!["m:w1".into()], add: vec![Flag::Seen], remove: Vec::new() };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(read, None)), "unread: marked read");
        let _ = shortcut(&mut app, S::ToggleRead, false);
        let unread = MailEdit::Flags { ids: vec!["m:w1".into()], add: Vec::new(), remove: vec![Flag::Seen] };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(unread, None)), "then unread again");
        let _ = shortcut(&mut app, S::Flag, false);
        let flag = MailEdit::Flags { ids: vec!["m:w1".into()], add: vec![Flag::Flagged], remove: Vec::new() };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(flag, None)));
        let _ = shortcut(&mut app, S::Reply, false);
        assert_eq!(app.sent.last(), Some(&Sent::Compose(mail::compose_args(Compose::Reply, "m:w1", true))));
        let _ = shortcut(&mut app, S::NewMail, false);
        assert_eq!(app.sent.last(), Some(&Sent::Compose(compose::new_args(true))));
        let _ = shortcut(&mut app, S::Fetch, false);
        assert_eq!(app.sent.last(), Some(&Sent::Fetch));
        let _ = app.update(Message::Fetched(Err("offline".into())));
        assert_eq!(app.error.as_ref().map(|failure| failure.what), Some(tr("Couldn't sync every account.")));

        let _ = shortcut(&mut app, S::Junk, false);
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("junk with a hint") };
        assert_eq!(edit, MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Role(MoveRole::Junk) });
        assert_eq!(hint.text, tr("Moved to junk"));
        let back = MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Folder("f:work-inbox".into()) };
        assert_eq!(hint.undo, Some(Undo::Mail { edit: back, reopen: None }));
        assert_eq!(cursor(&app), Some("m:w2"), "the next row takes its place");
        let _ = shortcut(&mut app, S::Archive, false);
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("archive with a hint") };
        assert_eq!(edit, MailEdit::Move { ids: vec!["m:w2".into()], to: MoveTarget::Role(MoveRole::Archive) });
        assert_eq!(hint.text, tr("Archived"));

        // At a mail entry, where `e` marks done, the shortcut archives its mail.
        let mut app = at("demo-mail-1");
        let _ = shortcut(&mut app, S::Archive, false);
        let Some(Sent::Mail(edit, Some(_))) = app.sent.last().cloned() else { panic!("archive at an entry") };
        assert_eq!(edit, MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Role(MoveRole::Archive) });
        let _ = press(&mut app, Key::E);
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { .. }, _))), "e stays Done there");
    }

    /// A mail opened with `--message` that is not on the folder's first page takes the shortcuts
    /// as its buttons do (review of #200).
    #[test]
    fn shortcuts_act_on_a_pinned_mail_beyond_the_page() {
        let mut app = in_folder("f:work-inbox");
        app.cursor = Some("m:far".into());
        app.pinned_mail = Some("m:far".into());
        app.reading = true;
        let _ = shortcut(&mut app, shortcuts::Command::Archive, false);
        // The window then asks for the letter it shows; the edit came before.
        let archived = MailEdit::Move { ids: vec!["m:far".into()], to: MoveTarget::Role(MoveRole::Archive) };
        assert!(app.sent.iter().any(|sent| matches!(sent, Sent::Mail(edit, Some(_)) if *edit == archived)));
        assert!(!app.reading, "the reader closes: the mail is gone from the folder");
        assert_ne!(cursor(&app), Some("m:far"), "the next key does not archive it again");
    }

    /// While the search field holds the keyboard it keeps its keys: no shortcut acts but searching;
    /// behind a menu or a dialog none does.
    #[test]
    fn a_text_field_keeps_its_keys() {
        use shortcuts::Command as S;
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let before = app.sent.len();
        for command in [S::Archive, S::Junk, S::ToggleRead, S::Flag, S::Reply, S::MoveToFolder, S::DeletePermanently] {
            let _ = shortcut(&mut app, command, true);
            assert_eq!((app.sent.len(), app.menu), (before, None), "{command:?} while typing");
        }
        let _ = shortcut(&mut app, S::Search, true);
        assert_eq!(app.sent.len(), before, "search only moves the keyboard");
        // Behind the dialog of "Make task" (its title field) and a menu, nothing.
        let _ = press(&mut app, Key::T);
        assert_eq!(app.menu, Some(Menu::Task));
        let _ = app.update(Message::Shortcut(S::Archive));
        let _ = shortcut(&mut app, S::Archive, false);
        assert_eq!(app.menu, Some(Menu::Task), "the dialog stays");
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Mail(MailEdit::Move { .. }, _))));
    }

    /// Shift + Delete asks first; Escape leaves the mail, `Enter` (or "Delete") removes it for good,
    /// without an undo.
    #[test]
    fn shift_delete_asks_before_it_deletes_for_good() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::DeleteForGood);
        assert_eq!((app.menu, app.mail_subject.as_deref()), (Some(Menu::DeleteForGood), Some("m:w1")));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Mail(..))), "nothing before the answer");
        assert_eq!(app.cover(), Cover::Dialog);
        draw_everywhere(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        for key in [Key::Down, Key::E, Key::Delete, Key::Hash] {
            let _ = press(&mut app, key);
            assert_eq!(app.menu, Some(Menu::DeleteForGood), "{key:?} waits for the answer");
        }
        let _ = app.update(Message::Escape);
        assert_eq!((app.menu, app.mail_subject.as_deref()), (None, None));
        assert!(message_ids(&app).contains(&"m:w1"), "cancelled");

        // The shortcut asks as well: the question belongs to the command.
        let _ = shortcut(&mut app, shortcuts::Command::DeletePermanently, false);
        assert_eq!(app.menu, Some(Menu::DeleteForGood));
        let _ = press(&mut app, Key::Enter);
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("deleted with a hint") };
        assert_eq!(edit, MailEdit::DeleteForGood { ids: vec!["m:w1".into()] });
        assert_eq!((hint.text.as_str(), hint.undo), (tr("Deleted permanently"), None));
        assert_eq!((app.menu, cursor(&app)), (None, Some("m:w2")));
        deliver_mail(&mut app);
        assert!(!message_ids(&app).contains(&"m:w1"));

        // The dialog's button does the same.
        let _ = press(&mut app, Key::DeleteForGood);
        let _ = app.update(Message::DeleteForGood);
        assert!(matches!(app.sent.last(), Some(Sent::Mail(MailEdit::DeleteForGood { ids }, _)) if ids == &["m:w2"]));
    }

    /// "Move to folder" lists the other folders of the mail's account, picked by digit, and the
    /// toast's undo moves the mail back.
    #[test]
    fn move_to_folder_offers_the_accounts_other_folders() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = shortcut(&mut app, shortcuts::Command::MoveToFolder, false);
        assert_eq!(app.menu, Some(Menu::Move));
        let work: Vec<&Folder> = app
            .folders
            .iter()
            .filter(|f| f.account == "mail:anna@work.example.org" && f.id != "f:work-inbox")
            .collect();
        let choices = app.choices();
        assert_eq!(choices.len(), work.len(), "every other folder of the account");
        assert!(!choices.iter().any(|choice| choice.label == tr("Inbox")), "not the one it lies in");
        let Message::MoveMail(id, first) = choices[0].message.clone() else { panic!("a move") };
        assert_eq!(id, "m:w1");
        draw_everywhere(&mut app);
        let _ = press(&mut app, Key::Digit(1));
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("moved with a hint") };
        assert_eq!(edit, MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Folder(first.clone()) });
        let name = app.folders.iter().find(|f| f.id == first).map(mail::folder_name).expect("the folder");
        assert_eq!(hint.text, trf("Moved to {folder}", &[("folder", &name)]));
        let back = MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Folder("f:work-inbox".into()) };
        assert_eq!(hint.undo, Some(Undo::Mail { edit: back, reopen: None }));
        assert_eq!(app.menu, None);
    }

    #[test]
    fn the_states_of_the_shortcuts_open_their_menu_and_question() {
        let mut app = loaded(&["--open", "move-mail"]);
        load(&mut app);
        assert_eq!(app.menu, Some(Menu::Move));
        draw_everywhere(&mut app);
        let mut app = loaded(&["--open", "delete-mail"]);
        load(&mut app);
        assert_eq!(app.menu, Some(Menu::DeleteForGood));
        draw_everywhere(&mut app);
    }

    // HTML mail in the reader (task M9.7).

    /// A window of the folder `folder` with its mail `id` open and loaded.
    fn reading_mail(folder: &str, id: &str) -> App<DemoSource> {
        let mut app = in_folder(folder);
        let _ = app.update(Message::SelectMail(id.into()));
        deliver_letter(&mut app, id);
        app
    }

    /// How the mail open shows: HTML or text and its colours; `None` without HTML (only its text).
    fn shown(app: &App<DemoSource>) -> Option<(MailFormat, Scheme)> {
        app.reading().and_then(|reading| reading.html).map(|html| (html.format, html.scheme))
    }

    fn look(format: MailFormat, colors: MailAppearance) -> Message {
        Message::MailLook(Ok(MailLook { format, colors }))
    }

    /// `mail_view = html` (the default) shows a mail's HTML, `text` its text; a mail without HTML
    /// always shows its text, and the parsed HTML goes with the mail.
    #[test]
    fn the_setting_html_shows_the_html_and_text_the_text() {
        let mut app = reading_mail("f:private-inbox", "m:p2");
        assert!(app.html.as_ref().is_some_and(|html| html.id == "m:p2"), "parsed while it is open");
        assert_eq!(shown(&app).map(|(format, _)| format), Some(MailFormat::Html));
        draw_everywhere(&mut app);
        let _ = app.update(look(MailFormat::Text, MailAppearance::App));
        assert_eq!(shown(&app).map(|(format, _)| format), Some(MailFormat::Text));
        draw_everywhere(&mut app);

        let _ = app.update(Message::SelectMail("m:p1".into()));
        deliver_letter(&mut app, "m:p1");
        assert!(app.html.is_none() && shown(&app).is_none(), "another mail without HTML: dropped, its text");

        let outlook = reading_mail("f:work-inbox", "m:w2");
        assert_eq!(shown(&outlook).map(|(format, _)| format), Some(MailFormat::Html), "text and HTML part");
        let plain = reading_mail("f:work-inbox", "m:w1");
        assert!(plain.html.is_none() && shown(&plain).is_none(), "a text mail: its text, no switches");
    }

    /// `v` and the switch "HTML | Text" turn a mail with HTML to its text and back; a mail without
    /// HTML has nothing to switch.
    #[test]
    fn v_switches_only_a_mail_with_html() {
        let mut app = reading_mail("f:work-inbox", "m:w2");
        draw_everywhere(&mut app);
        let _ = press(&mut app, Key::V);
        assert_eq!(shown(&app).map(|(format, _)| format), Some(MailFormat::Text));
        let _ = press(&mut app, Key::V);
        assert_eq!(shown(&app).map(|(format, _)| format), Some(MailFormat::Html));
        let _ = app.update(Message::ShowMailAs(MailFormat::Text));
        assert_eq!(shown(&app).map(|(format, _)| format), Some(MailFormat::Text), "the switch");

        let mut plain = reading_mail("f:work-inbox", "m:w1");
        draw_everywhere(&mut plain);
        let _ = press(&mut plain, Key::V);
        assert_eq!(plain.mail_format, MailFormat::Html, "no HTML: nothing switched");
    }

    /// "Light | Dark" changes the colours of the mail's content, never those of the window.
    #[test]
    fn light_and_dark_change_only_the_mail() {
        let mut app = reading_mail("f:private-inbox", "m:p2");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let window = app.theme();
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Light));
        let _ = app.update(Message::MailColors(Scheme::Dark));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Dark));
        assert_eq!(app.theme(), window, "the window keeps its colours");
        draw_everywhere(&mut app);
        let _ = app.update(Message::MailColors(Scheme::Light));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Light));
    }

    /// `mail_appearance = app` takes the window's colours as they change; `light` and `dark` hold.
    #[test]
    fn like_the_app_follows_the_appearance() {
        let mut app = reading_mail("f:private-inbox", "m:p2");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Light));
        let _ = app.update(Message::Appearance(Ok(Appearance::Dark)));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Dark));
        let _ = app.update(look(MailFormat::Html, MailAppearance::Light));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Light), "light though the app is dark");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(look(MailFormat::Html, MailAppearance::Dark));
        assert_eq!(shown(&app).map(|(_, scheme)| scheme), Some(Scheme::Dark), "dark though the app is light");
    }

    /// The switches hold for the window while the setting stays as it was (it is read every tick);
    /// a changed setting sets both back to it. `--mail-view` and `--mail-colors` win over it.
    #[test]
    fn the_switches_hold_until_the_setting_changes() {
        let mut app = reading_mail("f:private-inbox", "m:p2");
        let _ = app.update(Message::Appearance(Ok(Appearance::Light)));
        let _ = app.update(Message::ShowMailAs(MailFormat::Text));
        let _ = app.update(Message::MailColors(Scheme::Dark));
        let _ = app.update(look(MailFormat::Html, MailAppearance::App));
        let _ = app.update(Message::MailLook(Err("core is not running".into())));
        assert_eq!(shown(&app), Some((MailFormat::Text, Scheme::Dark)), "the same setting: the switches hold");
        let _ = app.update(look(MailFormat::Html, MailAppearance::Light));
        assert_eq!(shown(&app), Some((MailFormat::Html, Scheme::Light)), "a new setting: back to it");

        let launch = Launch::parse(&args(&["--mail-view", "text", "--mail-colors", "dark"])).expect("valid");
        assert_eq!((launch.mail_view, launch.mail_colors), (Some(MailFormat::Text), Some(MailAppearance::Dark)));
        let stored = MailLook { format: MailFormat::Html, colors: MailAppearance::Light };
        assert_eq!(launch.look(stored), MailLook { format: MailFormat::Text, colors: MailAppearance::Dark });
        let mut forced = in_folder("f:private-inbox");
        forced.launch = launch;
        let _ = forced.update(look(MailFormat::Html, MailAppearance::Light));
        let _ = forced.update(Message::SelectMail("m:p2".into()));
        deliver_letter(&mut forced, "m:p2");
        assert_eq!(shown(&forced), Some((MailFormat::Text, Scheme::Dark)), "the start parameters win");
        assert!(matches!(
            Launch::parse(&args(&["--mail-view", "pdf"])),
            Err(LaunchError::UnknownValue { flag: "--mail-view", .. })
        ));
        assert!(matches!(
            Launch::parse(&args(&["--mail-colors", "system"])),
            Err(LaunchError::UnknownValue { flag: "--mail-colors", .. })
        ));
    }

    /// A link clicked in the HTML opens in the browser (`open_url`, which the tests record instead of
    /// starting it) – web and mail addresses only.
    #[test]
    fn a_link_in_the_html_opens_in_the_browser() {
        let mut app = reading_mail("f:private-inbox", "m:p2");
        let _ = app.update(Message::HtmlLink("https://shop.example/sale".into()));
        assert_eq!(app.sent.last(), Some(&Sent::Link("https://shop.example/sale".into())));
        let _ = app.update(Message::HtmlLink("mailto:service@shop.example".into()));
        assert_eq!(app.sent.last(), Some(&Sent::Link("mailto:service@shop.example".into())));
        let before = app.sent.len();
        for url in ["javascript:alert(1)", "file:///etc/passwd", "cid:logo@shop.example", "shop.example"] {
            let _ = app.update(Message::HtmlLink(url.into()));
        }
        assert_eq!(app.sent.len(), before, "nothing else opens");
    }

    /// No mail window any more (task M9.7): Enter and two quick presses on a row of a mail with HTML
    /// open it in the reader, as for any mail, and start nothing.
    #[test]
    fn enter_and_a_double_click_only_open_the_reader() {
        let mut app = in_folder("f:private-inbox");
        app.cursor = Some("m:p2".into());
        let _ = press(&mut app, Key::Enter);
        deliver_letter(&mut app, "m:p2");
        let _ = app.update(Message::SelectMail("m:p2".into()));
        let _ = app.update(Message::SelectMail("m:p2".into()));
        assert!(app.reading && shown(&app).is_some(), "the reader shows the HTML");
        let started = app.sent.iter().filter(|sent| matches!(sent, Sent::Compose(_) | Sent::Link(_))).count();
        assert_eq!(started, 0, "{:?}", app.sent);
    }

    /// Issue #118: Delete (the key, or the row's button, which sends the same) moves a folder's
    /// message to the trash with its toast, and undo moves it back.
    #[test]
    fn delete_from_a_folders_row_moves_to_the_trash_and_undo_moves_back() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::Delete);
        let ids = vec!["m:w1".to_owned()];
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("a delete with a hint") };
        assert_eq!(edit, MailEdit::Delete { ids: ids.clone() });
        assert_eq!(hint.text, tr("Moved to Trash"));
        let back = MailEdit::Move { ids, to: MoveTarget::Folder("f:work-inbox".into()) };
        assert_eq!(hint.undo, Some(Undo::Mail { edit: back, reopen: None }));
        deliver_mail(&mut app);
        assert!(!message_ids(&app).contains(&"m:w1"));
        let _ = press(&mut app, Key::U);
        deliver_mail(&mut app);
        assert!(message_ids(&app).contains(&"m:w1"), "back in the inbox");

        // The row's button: the same message as the key.
        let _ = app.update(Message::Mail(MailAction::Delete, "m:w2".into()));
        assert!(matches!(app.sent.last(), Some(Sent::Mail(MailEdit::Delete { ids }, Some(_))) if ids == &["m:w2"]));
    }

    #[test]
    fn mail_keys_work_on_mail_entries_too() {
        let mut app = at("demo-mail-1");
        for (key, kind) in [(Key::R, Compose::Reply), (Key::A, Compose::ReplyAll), (Key::F, Compose::Forward)] {
            let _ = press(&mut app, key);
            assert_eq!(app.sent.last(), Some(&Sent::Compose(mail::compose_args(kind, "m:w1", true))), "{key:?}");
        }
        let _ = press(&mut app, Key::Hash);
        let Some(Sent::Mail(edit, Some(hint))) = app.sent.last().cloned() else { panic!("a delete with a hint") };
        assert_eq!(edit, MailEdit::Delete { ids: vec!["m:w1".into()] });
        assert_eq!(hint.text, tr("Moved to Trash"));
        let back = MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Role(MoveRole::Inbox) };
        assert_eq!(hint.undo, Some(Undo::Mail { edit: back, reopen: Some("demo-mail-1".into()) }));
        deliver_mail(&mut app);
        assert!(!ids(&app).contains(&"demo-mail-1"), "its mail left the inbox: the entry is done");

        let mut app = at("demo-1");
        let before = app.sent.len();
        for key in [Key::R, Key::A, Key::F, Key::Hash] {
            let _ = press(&mut app, key);
        }
        assert_eq!(app.sent.len(), before, "no mail behind a waiting agent");
    }

    #[test]
    fn enter_opens_a_mail_entry_in_the_panel_with_its_mail() {
        let mut app = at("demo-mail-1");
        let _ = press(&mut app, Key::Enter);
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-mail-1"));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Open(_))), "shown here, not opened elsewhere");
        assert_eq!(app.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w1"));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Mail(..))), "read once the entry is done");
        draw_everywhere(&mut app);
        deliver_letter(&mut app, "m:w1");
        draw_everywhere(&mut app);
        let _ = app.update(Message::Select("demo-1".into()));
        assert!(app.letter.is_none(), "no mail behind an agent's entry");
    }

    #[test]
    fn mail_in_the_trash_is_not_deleted_again() {
        let mut app = in_folder("f:work-trash");
        let _ = press(&mut app, Key::Down);
        let before = app.sent.len();
        let _ = press(&mut app, Key::Hash);
        assert_eq!(app.sent.len(), before);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("This message is in the trash already.")));
    }

    /// Issue #205: a deleted message stays out of the folder while the core still moves it – a load
    /// that answers before (the mark as read, the timer) does not bring it back.
    #[test]
    fn a_deleted_message_stays_gone_before_the_core_answers() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::Hash);
        assert!(!message_ids(&app).contains(&"m:w1"));
        assert_eq!(cursor(&app), Some("m:w2"));
        load(&mut app);
        assert!(!message_ids(&app).contains(&"m:w1"), "a load before the answer still holds it: kept out");
        assert_eq!(cursor(&app), Some("m:w2"), "the cursor stays on the row that took its place");
        deliver_mail(&mut app);
        assert!(!message_ids(&app).contains(&"m:w1"));
        assert!(app.leaving.is_empty(), "moved and no longer loaded: forgotten");

        // Undo: back once the core moved it back.
        let _ = press(&mut app, Key::U);
        assert!(app.leaving.is_empty());
        deliver_mail(&mut app);
        assert!(message_ids(&app).contains(&"m:w1"), "back in the inbox");
    }

    /// Issue #205: a move that fails brings the message back, with the error line.
    #[test]
    fn a_failed_delete_brings_the_message_back() {
        let mut app = in_folder("f:work-inbox");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::Hash);
        let _ = app.update(Message::MailEdited(Err("boom".into()), vec!["m:w1".into()], None));
        assert!(app.error.is_some(), "the error line");
        load(&mut app);
        assert!(message_ids(&app).contains(&"m:w1"), "not moved: back in its folder");
        let hint = app.hint.as_ref().expect("the hint says why, after the load as well");
        assert_eq!((hint.text.as_str(), hint.undo.is_none()), (tr("Couldn't change the mail."), true));
        assert!(app.leaving.is_empty());
    }

    /// Issue #205: deleting the mail of an entry takes the entry out at once, before the core's
    /// answer and over a load before it; the cursor moves to the row that took its place.
    #[test]
    fn deleting_a_mail_entry_takes_it_out_at_once() {
        let mut app = at("demo-mail-1");
        let place = app.rows().iter().position(|item| item.id == "demo-mail-1").expect("listed");
        let next = app.rows().get(place + 1).map(|item| item.id.clone());
        let _ = press(&mut app, Key::Hash);
        assert!(!ids(&app).contains(&"demo-mail-1"), "out of the list before the core answers");
        assert!(!app.for_me.iter().any(|item| item.id == "demo-mail-1"), "and out of what \"For me\" counts");
        assert_eq!(app.cursor, next, "the next row takes its place");
        load(&mut app);
        assert!(!ids(&app).contains(&"demo-mail-1"), "a load before the answer does not bring it back");
        assert_eq!(app.cursor, next);
        deliver_mail(&mut app);
        assert!(!ids(&app).contains(&"demo-mail-1"), "the core closed it");
        assert!(app.leaving.is_empty(), "moved and no longer loaded: forgotten");
        let _ = press(&mut app, Key::U);
        let back = MailEdit::Move { ids: vec!["m:w1".into()], to: MoveTarget::Role(MoveRole::Inbox) };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(back, None)), "undo moves it back");
    }

    /// Issue #136: a search looks in all mail by default and shows what it found; clearing it goes
    /// back to the folder it started from.
    #[test]
    fn a_search_shows_what_it_found_and_clearing_it_the_whole_folder() {
        let mut app = in_folder("f:work-inbox");
        let folder = app.view.clone();
        let _ = app.update(Message::SearchInput("minutes".into()));
        let _ = app.update(Message::Search);
        let shown = app.view.search().cloned().expect("a search");
        assert_eq!((shown.query.as_str(), &shown.scope, shown.sort), ("minutes", &Scope::All, Sort::Relevance));
        load(&mut app);
        assert_eq!(message_ids(&app), ["m:w6"]);
        assert_eq!(app.count_note(), "1", "what it found");
        assert_eq!(app.recent, ["minutes"], "Enter keeps the search");
        draw_everywhere(&mut app);
        let _ = app.update(Message::ClearSearch);
        assert_eq!((app.search.as_str(), &app.view), ("", &folder), "back where it started");
        load(&mut app);
        assert_eq!(app.messages.len(), 8);
        let _ = app.update(Message::SearchInput("nothing like this".into()));
        let _ = app.update(Message::Search);
        load(&mut app);
        assert!(app.messages.is_empty());
        draw_everywhere(&mut app);
    }

    /// Issue #136: typing searches once the keys pause, not at every key; a blank field goes back.
    #[test]
    fn typing_searches_after_a_pause() {
        let mut app = in_folder("f:work-inbox");
        let folder = app.view.clone();
        let _ = app.update(Message::SearchInput("bud".into()));
        let due = app.search_due.expect("a search waits");
        assert!(app.view.search().is_none(), "not at once");
        let _ = app.update(Message::SearchTick(due - Duration::from_millis(10)));
        assert!(app.view.search().is_none(), "still typing");
        let _ = app.update(Message::SearchInput("budget".into()));
        let due = app.search_due.expect("a search waits");
        let _ = app.update(Message::SearchTick(due));
        assert_eq!(app.view.search().map(|search| search.query.as_str()), Some("budget"));
        assert!(app.search_due.is_none());
        assert!(app.recent.is_empty(), "typing alone keeps no search");
        load(&mut app);
        assert!(!app.messages.is_empty() && !app.highlights.is_empty(), "found and highlighted");
        let first = app.messages[0].id.clone();
        let _ = app.update(Message::SelectMail(first));
        assert_eq!(app.recent, ["budget"], "opening a result keeps it");
        let _ = app.update(Message::SearchInput(String::new()));
        assert_eq!(app.view, folder, "a blank field goes back");
    }

    /// Issue #136: from a folder the search can look in its account or the folder alone, by date;
    /// an incomplete index is said, with how far it is.
    #[test]
    fn a_search_narrows_to_the_account_or_folder_and_says_what_is_missing() {
        let mut app = in_folder("f:work-inbox");
        let folder = app.view.folder().cloned().expect("a folder");
        let _ = app.update(Message::SearchInput("budget".into()));
        let _ = app.update(Message::Search);
        load(&mut app);
        assert!(!app.found.complete, "the demo still indexes an account");
        assert!(!app.found.index.is_empty(), "how far, asked");
        let scopes = app.search_scopes().expect("from a folder");
        assert_eq!(scopes.folder, folder);
        let _ = app.update(Message::SearchScope(Scope::Account(folder.account.clone())));
        load(&mut app);
        assert!(app.found.complete, "the work account is indexed");
        assert!(app.messages.iter().all(|message| message.account == folder.account));
        let _ = app.update(Message::SearchScope(Scope::Folder(folder.clone())));
        let _ = app.update(Message::SearchSort(Sort::Date));
        load(&mut app);
        let shown = app.view.search().cloned().expect("a search");
        assert_eq!((shown.scope, shown.sort), (Scope::Folder(folder.clone()), Sort::Date));
        assert!(app.messages.iter().all(|message| message.folder == folder.id));
        draw_everywhere(&mut app);
    }

    /// Issue #136: suggestions under the field – operators, people, folders, searches last made –,
    /// Tab takes the first; ⌘F / Ctrl + F and `/` focus the field, the button opens a search where
    /// the view has no field.
    #[test]
    fn suggestions_complete_the_search_and_the_keys_reach_the_field() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::RecentSearches(vec!["budget review".into()]));
        let _ = app.update(Message::SearchInput("from:jo".into()));
        let suggestions = app.search_suggestions();
        assert!(suggestions.iter().any(|s| s.query.starts_with("from:jonas")), "{suggestions:?}");
        let _ = app.update(Message::Key(Key::Tab));
        assert!(app.search.starts_with("from:jonas") && app.search.ends_with(' '), "{}", app.search);
        assert!(!app.suggesting, "taken");
        let _ = app.update(Message::Escape);
        let command =
            if cfg!(target_os = "macos") { iced::keyboard::Modifiers::LOGO } else { iced::keyboard::Modifiers::CTRL };
        let press = |modifiers| {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character("f".into()),
                modified_key: iced::keyboard::Key::Character("f".into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: Some("f".into()),
                repeat: false,
            })
        };
        let captured = iced::event::Status::Captured;
        let heard = keys::shortcut(press(command), captured, iced::window::Id::unique());
        assert!(matches!(heard, Some(Message::Shortcut(shortcuts::Command::Search))), "also when the guard took it");
        let plain = keys::shortcut(press(iced::keyboard::Modifiers::empty()), captured, iced::window::Id::unique());
        assert!(plain.is_none(), "a plain f is no search");
        assert_eq!(keys::of_shortcut(shortcuts::Command::Search), Some(Command::Search));
        // In the hub, which lists no mail, the button opens an empty search with the searches last made.
        let mut hub = overview_at(&["--view", "hub"]);
        let _ = hub.update(Message::RecentSearches(vec!["budget review".into()]));
        let _ = hub.update(Message::Switcher(Switcher::Search));
        assert_eq!(hub.view.search().map(|search| search.query.as_str()), Some(""));
        assert_eq!(hub.search_suggestions().first().map(|s| s.query.as_str()), Some("budget review"));
        draw_everywhere(&mut hub);
        let _ = hub.update(Message::ForgetSearches);
        assert!(hub.recent.is_empty() && hub.search_suggestions().is_empty());
        let _ = hub.update(Message::Escape);
        assert_eq!(hub.view, View::Overview, "Escape goes back");
    }

    #[test]
    fn flags_change_at_once_and_on_the_server() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::SelectMail("m:w4".into()));
        assert!(!app.sent.iter().any(|sent| matches!(sent, Sent::Mail(..))), "read already");
        let _ = app.update(Message::Mail(MailAction::Flag(true), "m:w4".into()));
        let w4 = app.messages.iter().find(|message| message.id == "m:w4").expect("m:w4");
        assert!(w4.flags.contains(&Flag::Flagged));
        let flag = MailEdit::Flags { ids: vec!["m:w4".into()], add: vec![Flag::Flagged], remove: Vec::new() };
        assert_eq!(app.sent.last(), Some(&Sent::Mail(flag, None)));
        deliver_mail(&mut app);
        let _ = app.update(Message::Mail(MailAction::Read(false), "m:w4".into()));
        deliver_mail(&mut app);
        let w4 = app.messages.iter().find(|message| message.id == "m:w4").expect("listed");
        assert_eq!(w4.flags, [Flag::Flagged], "flagged and unread, from the server");
    }

    #[test]
    fn load_more_asks_for_the_next_page_and_adds_it() {
        let mut app = in_folder("f:work-inbox");
        app.more = Some("4".into());
        let _ = app.update(Message::MoreMail);
        assert_eq!(app.sent.last(), Some(&Sent::More("4".into())));
        let more =
            MailSource::search_mail(app.inbox.as_ref(), "f:work-inbox".into(), None, 4, Some("4".into()), app.order);
        let page = block_on(more).map_err(|err| err.to_string());
        let _ = app.update(Message::MoreLoaded(app.view.clone(), app.order, page));
        assert_eq!(app.messages.len(), 8, "nothing twice");
        assert_eq!(app.more, None);
        assert_eq!(app.page_limit(), mail::PAGE, "a load asks for a page at least");
    }

    #[test]
    fn attachments_and_links_open_only_what_they_name() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::OpenAttachment("m:w1".into(), "2".into()));
        assert_eq!(app.sent.last(), Some(&Sent::Attachment("m:w1".into(), "2".into())));
        let before = app.sent.len();
        let _ = app.update(Message::OpenMailLink("file:///etc/passwd".into()));
        assert_eq!(app.sent.len(), before, "only web addresses");
        let _ = app.update(Message::OpenMailLink("https://intranet.work.example.org/budget/q4".into()));
        assert_eq!(app.sent.last(), Some(&Sent::Link("https://intranet.work.example.org/budget/q4".into())));
        let opened = Opened::Folder(std::path::PathBuf::from("/tmp/setup.exe"));
        let _ = app.update(Message::AttachmentOpened(Ok(opened.clone())));
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.clone()), Some(mail::opened_text(&opened)));
    }

    /// Issue #119: the text in the reader and the panel can be selected – never changed – and "Copy
    /// text" puts all of it on the clipboard, as written, with "Copied".
    #[test]
    fn copy_text_puts_the_whole_plain_text_on_the_clipboard() {
        let mut app = in_folder("f:work-inbox");
        assert!(app.update(Message::CopyText).units() == 0 && app.hint.is_none(), "nothing to copy yet");
        let _ = app.update(Message::SelectMail("m:w1".into()));
        deliver_letter(&mut app, "m:w1");
        let Some(Letter { loaded: Some(Ok(mail)), .. }) = app.letter.clone() else { panic!("the mail is loaded") };
        let plain = mail::text_of(&mail);
        assert!(!plain.trim().is_empty());
        assert_eq!(app.text.text(), plain, "the reader's text follows the mail");
        let _ = app.update(Message::Text(text_editor::Action::SelectAll));
        let _ = app.update(Message::Text(text_editor::Action::Edit(text_editor::Edit::Delete)));
        assert_eq!(app.text.text(), plain, "selecting and typing leave it as it is");
        assert_eq!(app.update(Message::CopyText).units(), 1, "one clipboard::write");
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Copied")));
        draw_everywhere(&mut app);

        // An entry's message in the panel.
        let mut entry = loaded(&["--open", "detail"]);
        let item = entry.detail.clone().expect("an entry in the panel");
        assert_eq!(entry.text.text(), detail::message_text(&item));
        draw_everywhere(&mut entry);
    }

    #[test]
    fn the_mail_states_open_and_draw_in_every_size() {
        let mut folder = loaded(&["--open", "folder"]);
        load(&mut folder);
        assert_eq!(folder.view.folder().map(|f| f.id.as_str()), Some("f:work-inbox"));
        assert!(!folder.panel_open());
        draw_everywhere(&mut folder);

        let mut open = loaded(&["--open", "mail"]);
        load(&mut open);
        assert!(open.panel_open());
        assert_eq!(open.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w1"));
        deliver_letter(&mut open, "m:w1");
        draw_everywhere(&mut open);

        let mut entry = loaded(&["--open", "mail-entry"]);
        assert_eq!(entry.detail.as_ref().map(|item| item.id.as_str()), Some("demo-mail-1"));
        assert_eq!(entry.letter.as_ref().map(|letter| letter.id.as_str()), Some("m:w1"));
        deliver_letter(&mut entry, "m:w1");
        draw_everywhere(&mut entry);
    }

    // ----- Overview (E3.9) -------------------------------------------------------------------

    /// The demo window as it starts – on the overview –, loaded; `words` add to its parameters.
    fn overview_at(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T10:30"];
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    fn rows_of<S: InboxSource + MailSource + StatsSource + HubSource + SuggestSource + 'static>(
        app: &App<S>,
    ) -> Vec<&str> {
        app.rows().iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn the_window_starts_on_the_overview_with_every_open_entry() {
        let app = overview_at(&[]);
        assert_eq!(app.view, View::Overview);
        assert!(app.is_hub(), "the hub's overview is Act now (task E8.5)");
        assert_eq!(app.items.len(), FOR_ME.len() + 6, "both lanes in one answer");
        assert_eq!((app.for_me.len(), app.info), (18, 6), "the sidebar's numbers from the same answer");
        assert_eq!(app.count_note(), hub::summary(&hub::Sections::of(&app.items, None)));
        assert!(app.nav_entries().0[0].active, "the first entry of the sidebar");
        assert_eq!(app.title(), trf("Notification Hub · {view}", &[("view", &tr("Act now"))]));
        let rows = rows_of(&app);
        assert_eq!(rows[..4], ["demo-3", "demo-9", "demo-1", "demo-2"], "Decide, Follow up, then Agents …");
        // Everything for the user, and of Info only the question after a finished task (task E8.7b).
        assert_eq!(rows.len(), FOR_ME.len() + 1);
        assert_eq!(app.snoozed.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["demo-10"], "Coming back");
    }

    /// Task E8.1: the notifications window shows the overview's other column alone, the mail window
    /// the mail column; `h`/`l` and "Mail | Notifications" cannot leave it. The hub keeps both.
    #[test]
    fn each_window_kind_shows_its_columns_of_the_overview() {
        // The notifications window shows what the apps report (task E8.4): the scripts of the demo and,
        // for now like scripts, its GitHub, Nextcloud, Talk and ntfy entries (task E5.1), GitHub first.
        let mut notices = overview_at(&["--view", "notifications"]);
        assert_eq!((notices.view.clone(), notices.column), (View::Overview, Column::Notices));
        assert_eq!(rows_of(&notices).first(), Some(&"demo-14"), "the keys start on its first card");
        assert!(rows_of(&notices).iter().all(|id| !id.starts_with("demo-mail")), "no mail");
        let _ = press(&mut notices, Key::H);
        let _ = notices.update(Message::ShowColumn(Column::Mail));
        assert_eq!(notices.column, Column::Notices, "there is no mail column to go to");
        assert_eq!(notices.title(), trf("App Feed · {view}", &[("view", &tr("New"))]));

        let mail = overview_at(&["--view", "mail"]);
        assert_eq!(mail.column, Column::Mail);
        assert!(rows_of(&mail).iter().all(|id| id.starts_with("demo-mail")), "only mail");
        assert_eq!(mail.title(), trf("Mail · {view}", &[("view", &tr("For me"))]));

        assert_eq!(overview_at(&["--view", "hub"]).launch.kind.columns(), Column::ALL);
        for size in SIZES {
            let mut app = overview_at(&["--view", "mail"]);
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    /// Task E8.5: the keys walk the hub's sections as one list; a tile filters them.
    #[test]
    fn the_hubs_keys_walk_its_sections_and_a_tile_filters_them() {
        let mut app = overview_at(&[]);
        assert_eq!(app.focus(), Focus::List, "no columns to change");
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-9"), "from Decide into Follow up (task E8.7b)");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-1"), "then into Agents");
        let _ = press(&mut app, Key::H);
        assert_eq!(cursor(&app), Some("demo-1"), "h does nothing");
        let _ = app.update(Message::Filter(Origin::Mail));
        assert!(rows_of(&app).iter().all(|id| id.starts_with("demo-mail")), "the mail tile: mail alone");
        assert_eq!(cursor(&app), None, "the cursor's entry is filtered away");
        let _ = app.update(Message::Filter(Origin::Mail));
        assert_eq!(rows_of(&app).len(), FOR_ME.len() + 1, "a second click shows everything, Follow up too");
        let _ = app.update(Message::Filter(Origin::Agents));
        let _ = app.update(Message::Show(View::Overview));
        assert_eq!(app.filter, None, "Act now from the sidebar shows everything");
    }

    /// Task E8.5: "To sort" in the sidebar shows the section alone, and "Act now" all again.
    #[test]
    fn to_sort_shows_its_section_alone() {
        let mut app = overview_at(&[]);
        let _ = app.update(Message::ShowToSort);
        assert!(app.sorting && app.view == View::Overview);
        assert_eq!(rows_of(&app).len(), 14);
        assert!(rows_of(&app).iter().all(|id| *id != "demo-3"), "no approval to sort");
        assert_eq!(app.title(), trf("Notification Hub · {view}", &[("view", &tr("To sort"))]));
        assert_eq!(app.count_note(), "14");
        assert!(app.nav_entries().0[1].active && !app.nav_entries().0[0].active);
        let _ = app.update(Message::Show(View::Overview));
        assert!(!app.sorting);
        assert_eq!(rows_of(&app).first(), Some(&"demo-3"));
        assert!(overview_at(&["--open", "to-sort"]).sorting, "its demo state");
        assert_eq!(overview_at(&["--open", "tile"]).filter, Some(Origin::Mail), "the busiest tile chosen");
    }

    /// Task E8.5: an approval is never put off – not by `s`, not from its card or the panel.
    #[test]
    fn approvals_are_not_put_off() {
        let mut app = overview_at(&[]);
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-3"));
        let _ = press(&mut app, Key::S);
        assert_eq!(app.menu, None, "no snooze menu");
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("An approval closes once it is decided."))
        );
        let _ = app.update(Message::AskSnooze("demo-3".into()));
        assert_eq!(app.menu, None);
        let _ = press(&mut app, Key::X);
        assert!(app.sent.is_empty(), "nor dismissed");
    }

    /// Task E8.5: the hub's "Done today" lists only what was done today.
    #[test]
    fn done_today_is_today() {
        let early = loaded(&["--open", "done"]);
        assert!(ids(&early).is_empty(), "done yesterday 12:30");
        let late = loaded_late(&["--open", "done"]);
        assert_eq!(ids(&late), ["demo-11"], "done today 01:30");
        assert_eq!(late.title(), trf("Notification Hub · {view}", &[("view", &tr("Done today"))]));
        let mut notices =
            demo(&["--demo", "--now", "2026-10-13T10:30", "--view", "notifications", "--source", "agent:codex"]);
        let _ = notices.update(Message::Show(View::Done));
        load(&mut notices);
        assert_eq!(ids(&notices), ["demo-11"], "other windows keep all of Done");
    }

    #[test]
    fn moving_in_act_now_scrolls_it() {
        let mut app = overview_at(&[]);
        let sections = hub::Sections::of(&app.items, None);
        let tiles = hub::tiles(&app.items).len();
        let (_, first) = hub::span(&sections, tiles, app.hub_layout(), "demo-3").expect("the approval");
        let _ = app.update(Message::Scrolled(0.0, first));
        assert_eq!(press(&mut app, Key::Down).units(), 0, "the approval is in view");
        assert!(press(&mut app, Key::Down).units() > 0, "the first agent needs a scroll");
        assert!(app.scroll.is_some_and(|(offset, _)| offset > 0.0));
    }

    #[test]
    fn info_unfolds_with_a_click_and_the_keys_follow() {
        let mut app = overview_at(&["--view", "mail"]);
        for _ in 0..5 {
            let _ = press(&mut app, Key::Down);
        }
        assert_eq!(cursor(&app), Some("demo-mail-8"));
        let _ = app.update(Message::ToggleInfo(Column::Mail));
        assert_eq!(app.info_open, [true, false], "only that column");
        assert_eq!(app.unfolded(), ["overview.mail.info"], "what ui.json keeps (task E3.11)");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-mail-3"), "on into Info");
        let _ = app.update(Message::ToggleInfo(Column::Mail));
        assert_eq!(cursor(&app), None, "folded away with its row");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-mail-1"));
        assert_eq!(overview_at(&["--open", "overview-info"]).info_open, [true, true]);
    }

    #[test]
    fn info_comes_back_unfolded_as_ui_json_kept_it() {
        let mut app = overview_at(&[]);
        let _ = app.update(Message::Unfolded(vec!["overview.notices.info".into(), "something.else".into()]));
        assert_eq!(app.info_open, [false, true]);
        assert_eq!(app.unfolded(), ["something.else", "overview.notices.info"], "another window's parts stay");
        let _ = app.update(Message::Unfolded(Vec::new()));
        assert_eq!(app.info_open, [false, false]);
        let mut forced = overview_at(&["--open", "overview-info"]);
        let _ = forced.update(Message::Unfolded(Vec::new()));
        assert_eq!(forced.info_open, [true, true], "a forced state stays for its picture");
    }

    #[test]
    fn a_row_action_puts_the_cursor_there_and_does_what_its_key_does() {
        let mut app = overview_at(&[]);
        let _ = app.update(Message::Act("demo-6".into(), Command::Snooze));
        assert_eq!((app.menu, cursor(&app), app.column), (Some(Menu::Snooze), Some("demo-6"), Column::Notices));
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::Act("demo-mail-4".into(), Command::Hide));
        assert_eq!((app.menu, app.column), (Some(Menu::Mute), Column::Mail));
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::Act("demo-4".into(), Command::Done));
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { id, done: true }, _)) if id == "demo-4"));
        let _ = app.update(Message::Act("demo-3".into(), Command::Done));
        assert_eq!(
            app.hint.as_ref().map(|hint| hint.text.as_str()),
            Some(tr("An approval closes once it is decided."))
        );
        let _ = app.update(Message::Act("demo-mail-1".into(), Command::MakeTask));
        assert_eq!((app.menu, cursor(&app)), (Some(Menu::Task), Some("demo-mail-1")));
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::Act("gone".into(), Command::Snooze));
        assert_eq!((app.menu, cursor(&app)), (None, Some("demo-mail-1")), "an unknown entry does nothing");
        // With the panel open it follows the row.
        let _ = app.update(Message::Select("demo-6".into()));
        let _ = app.update(Message::Act("demo-mail-2".into(), Command::Snooze));
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-mail-2"));
    }

    #[test]
    fn go_to_window_on_a_card_opens_the_agents_window() {
        let mut app = overview_at(&[]);
        let _ = app.update(Message::OpenRow("demo-1".into()));
        assert_eq!(app.sent.last(), Some(&Sent::Open("demo-1".into())));
        let _ = app.update(Message::OpenRow("gone".into()));
        assert_eq!(app.sent.len(), 1);
    }

    /// Task E8.5: the hub has sections, no columns – "Mail | Notifications" changes nothing.
    #[test]
    fn the_hub_has_no_columns_to_choose() {
        let mut app = overview_at(&[]);
        for size in [MEDIUM, NARROW] {
            let _ = app.update(Message::Resized(size));
            let _ = app.update(Message::ShowColumn(Column::Notices));
            assert_eq!((app.column, cursor(&app)), (Column::Mail, None), "{size:?}");
            let _ = app.view();
        }
    }

    #[test]
    fn open_overview_shows_the_action_bar_on_the_first_mail() {
        let app = overview_at(&["--view", "mail", "--open", "overview"]);
        assert_eq!((app.view.clone(), app.column, cursor(&app)), (View::Overview, Column::Mail, Some("demo-mail-1")));
        let app = loaded(&["--open", "overview"]);
        assert_eq!(app.view, View::Overview, "a demo state wins over the lane");
        assert_eq!(cursor(&app), Some("demo-3"), "the hub: the first entry of Act now");
        let app = overview_at(&["--view", "notifications", "--open", "overview-notices"]);
        assert_eq!((app.column, cursor(&app)), (Column::Notices, Some("demo-14")), "the first card");
    }

    #[test]
    fn the_overview_builds_in_every_size_with_panel_menus_and_cards() {
        for open in
            [&["--open", "overview"][..], &["--open", "overview-info"], &["--open", "detail"], &["--open", "empty"]]
        {
            let mut app = overview_at(open);
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
                let _ = press(&mut app, Key::Down);
                let _ = app.update(Message::Act("demo-6".into(), Command::Snooze));
                let _ = app.view();
                let _ = app.update(Message::CloseMenu);
                let _ = app.update(Message::ShowDrawer);
                let _ = app.view();
                let _ = app.update(Message::CloseMenu);
                let _ = app.update(Message::Select("demo-3".into()));
                let _ = app.view();
                let _ = app.update(Message::CloseDetail);
            }
        }
    }

    /// The mail window at the demo's fixed time in `density`, loaded on its start view ("For me").
    fn mail_window(density: &str, more: &[&str]) -> App<DemoSource> {
        let mut words = vec!["--view", "mail", "--density", density];
        words.extend_from_slice(more);
        overview_at(&words)
    }

    /// Task E8.3: the mail window lists, counts and walks mail alone, in every view of its sidebar.
    #[test]
    fn the_mail_window_shows_mail_only() {
        let mut app = mail_window("comfortable", &[]);
        assert!(!app.items.is_empty());
        for view in [View::Overview, View::Info, View::Snoozed, View::Done] {
            let _ = app.update(Message::Show(view.clone()));
            load(&mut app);
            assert!(app.items.iter().all(|item| item.source.kind == SourceKind::Mail), "{view:?}");
        }
        assert!(app.for_me.iter().all(|item| item.source.kind == SourceKind::Mail), "the count is of mail");
        let hub = overview_at(&["--view", "hub"]);
        assert!(hub.items.iter().any(|item| item.source.kind != SourceKind::Mail), "the hub keeps everything");
        assert!(app.for_me.len() < hub.for_me.len());
    }

    /// Task E8.3: For me, Info, Later, Done, then "Smart views" and the folders; no sources, and
    /// "For me" is the window's overview.
    #[test]
    fn the_mail_windows_sidebar_lists_its_views_smart_views_and_folders() {
        let app = mail_window("comfortable", &[]);
        let (views, sources) = app.nav_entries();
        let labels: Vec<String> = views.iter().map(|entry| entry.label.clone()).collect();
        assert_eq!(labels, [tr("For me"), tr("Info"), tr("Later"), tr("Done")]);
        assert_eq!(views[0].view, View::Overview);
        assert!(sources.is_empty(), "every source is a mail account, listed by its folders");
        let smart: Vec<String> = app.smart_entries().into_iter().map(|entry| entry.label).collect();
        assert_eq!(smart, [tr("Invitations"), tr("With attachments")]);
        assert!(!app.mail_groups().is_empty(), "the folders of every account");
        assert!(overview_at(&["--view", "hub"]).smart_entries().is_empty(), "smart views are the mail window's");
        for size in SIZES {
            let mut app = mail_window("comfortable", &["--open", "menu"]);
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    /// Task E8.3: cards in the comfortable mail window, rows elsewhere; task E8.11: the same cards in
    /// every layout (their words follow the room, `list::spoken`), the wide cards beside the reader.
    #[test]
    fn comfortable_mail_is_cards_and_everything_else_rows() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!(app.look(), Look::Cards);
        assert_eq!(app.card_width(), size::list() - list::cards_side(), "the list beside the reader");
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!(app.look(), Look::Cards);
        assert_eq!(app.card_width(), NARROW.width - list::cards_side(), "the window's width");
        let compact = mail_window("compact", &[]);
        assert_eq!(compact.look(), Look::Rows);
        let hub = overview_at(&["--view", "hub", "--density", "comfortable"]);
        assert_eq!(hub.look(), Look::Rows, "the hub's cards come with E8.5");
    }

    /// Issue #129: without a reader beside it the mail window's list fills the window – in "For me"
    /// as in a folder, in both densities –; the lists of the other windows keep their reading width.
    #[test]
    fn the_mail_list_fills_the_window_without_a_reader() {
        let medium = Size::new(1000.0, 700.0);
        let room = medium.width - shell::rail() - layout::inset();
        for density in ["comfortable", "compact"] {
            let mut app = mail_window(density, &[]);
            let _ = app.update(Message::Resized(medium));
            assert_eq!(app.list_cap(1), None, "{density}");
            assert!(room > overview::width(1), "a window where the cap would show");
            let _ = app.view();
        }
        let mut cards = mail_window("comfortable", &[]);
        let _ = cards.update(Message::Resized(medium));
        assert_eq!(cards.card_width(), room - list::cards_side(), "the cards take the whole room");
        let hub = overview_at(&["--view", "hub"]);
        assert_eq!(hub.list_cap(2), Some(overview::width(2)), "other windows keep the reading width");
    }

    /// Issue #130: the divider between the wide mail window's list and reader drags the list's width
    /// – kept between both panes' least –, the cards follow it, a double click goes back to the
    /// default, and what `ui.json` remembers counts only until the person drags.
    #[test]
    fn the_divider_drags_the_list_width() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Resized(WIDE));
        let limits = app.split_limits();
        assert_eq!(app.split.width(limits), size::list(), "the default");
        let _ = app.update(Message::StoredSplit(Some(600.0)));
        assert_eq!(app.split.width(limits), 600.0, "as remembered");
        let _ = app.update(Message::Split(SplitEvent::Grab));
        let _ = app.view();
        let _ = app.update(Message::Split(SplitEvent::Drag(700.0)));
        let _ = app.update(Message::Split(SplitEvent::Release));
        assert_eq!((app.split.chosen, app.split.dragging), (Some(700.0), false));
        assert_eq!(app.card_width(), 700.0 - list::cards_side(), "the cards follow the list");
        let _ = app.update(Message::StoredSplit(Some(500.0)));
        assert_eq!(app.split.chosen, Some(700.0), "a late answer of the file loses");
        let _ = app.update(Message::Split(SplitEvent::Grab));
        let _ = app.update(Message::Split(SplitEvent::Drag(WIDE.width)));
        assert_eq!(app.split.chosen, Some(limits.clamp(WIDE.width)), "the reader keeps its least");
        assert!(app.split.width(limits) < app.content_width());
        let _ = app.update(Message::Split(SplitEvent::Reset));
        assert_eq!((app.split.chosen, app.split.width(limits)), (None, size::list()));
        for size in SIZES {
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    /// Task E8.3: the narrow comfortable "For me" leads with how many mails are new for the user.
    #[test]
    fn the_narrow_mail_window_says_how_many_are_new_for_you() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!(app.lead(), None, "only narrow");
        let _ = app.update(Message::Resized(NARROW));
        let (count, senders) = app.lead().expect("a lead in narrow");
        assert_eq!(count, app.for_me.len());
        assert!(senders.starts_with(tr("From {senders}").trim_end_matches("{senders}")), "{senders}");
        // Task E8.11 (finding 7): in both densities.
        let mut compact = mail_window("compact", &[]);
        let _ = compact.update(Message::Resized(NARROW));
        assert_eq!(compact.lead(), app.lead(), "compact leads the same");
        let _ = compact.update(Message::Resized(WIDE));
        assert_eq!(compact.lead(), None);
        for size in SIZES {
            let _ = compact.update(Message::Resized(size));
            let _ = compact.view();
        }
    }

    /// Task E8.11 (finding 4): the wide mail window keeps the reader's column while nothing is open,
    /// with the empty state, so the list keeps its width; the panel and the other layouts have none.
    #[test]
    fn the_wide_mail_window_keeps_an_empty_reader() {
        for density in ["comfortable", "compact"] {
            let mut app = mail_window(density, &[]);
            let _ = app.update(Message::Resized(WIDE));
            assert!(!app.panel_open());
            assert!(app.empty_reader(SizeClass::Wide).is_some(), "{density}: the empty reader");
            assert!(app.empty_reader(SizeClass::Medium).is_none(), "{density}: medium has no reader");
            let _ = app.view();
        }
        let mut open = mail_window("comfortable", &["--open", "mail-entry"]);
        let _ = open.update(Message::Resized(WIDE));
        assert!(open.empty_reader(SizeClass::Wide).is_none(), "a mail is open");
        let hub = overview_at(&["--view", "hub"]);
        assert!(hub.empty_reader(SizeClass::Wide).is_none(), "the hub has no reader");
    }

    /// Issue #122: a menu hangs at the button clicked – the row's, also while the reader shows the
    /// same entry beside it, or the reader's –, and one opened by a key at the reader while it is
    /// open, else at the row. Keys inside the open menu leave it where it is.
    #[test]
    fn a_menu_hangs_at_the_button_clicked() {
        for density in ["compact", "comfortable"] {
            let mut app = mail_window(density, &["--open", "mail-entry"]);
            let _ = app.update(Message::Resized(WIDE));
            let entry = app.detail.as_ref().expect("the entry in the reader").id.clone();
            let _ = app.update(Message::Act(entry.clone(), Command::Snooze));
            assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Snooze), false), "{density}: the row's");
            let _ = app.view();
            let _ = app.update(Message::CloseMenu);
            let _ = app.update(Message::AskSnooze(entry.clone()));
            assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Snooze), true), "{density}: the reader's");
            let _ = app.view();
            let _ = app.update(Message::CloseMenu);
            let _ = app.update(Message::Act(entry.clone(), Command::Hide));
            let _ = press(&mut app, Key::Down);
            assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Mute), false), "{density}: keys in the menu");
            let _ = app.update(Message::CloseMenu);
            let _ = press(&mut app, Key::S);
            assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Snooze), true), "{density}: s, reader open");
            let _ = app.update(Message::CloseMenu);
            let _ = app.update(Message::CloseDetail);
            let _ = app.update(Message::Select(entry.clone()));
            let _ = app.update(Message::CloseDetail);
            let _ = press(&mut app, Key::S);
            assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Snooze), false), "{density}: s at the row");
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
    }

    /// Issue #122: a folder message's "Task" in its row does what `t` does there, so its dialog opens
    /// at that button; the reader's "Task" opens it at the reader's.
    #[test]
    fn a_messages_task_opens_at_the_button_clicked() {
        let mut app = in_folder("f:work-inbox");
        let message = app.messages.iter().find(|m| app.entry_of(&m.id).is_none()).expect("a message").id.clone();
        let _ = app.update(Message::Act(message.clone(), Command::MakeTask));
        assert_eq!(app.menu, Some(Menu::Task));
        assert_eq!(app.task_mail.as_ref().map(|m| m.id.as_str()), Some(message.as_str()), "the message's task");
        assert!(!app.menu_from_panel());
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::AskMailTask(message));
        assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Task), true));
    }

    /// Issue #122: the bell in the notifications window's panel opens the bell's menu at itself.
    #[test]
    fn the_panels_bell_opens_its_menu_at_the_panel() {
        let mut app = demo(&["--demo", "--now", "2026-10-13T10:30", "--view", "notifications"]);
        load(&mut app);
        let _ = app.update(Message::AskHide("demo-6".into()));
        assert_eq!((app.menu, app.menu_from_panel()), (Some(Menu::Mute), true));
    }

    /// Task E8.11 (finding 3): in "Invitations" the card's "Later" and "…" act on the mail's entry in
    /// "For me", and `s` does the same on the message under the cursor.
    #[test]
    fn an_invitation_card_puts_off_and_hides_its_entry() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Show(View::Invitations));
        load(&mut app);
        let (message, entry) = app
            .messages
            .iter()
            .find_map(|message| {
                mail::entry_of(&app.for_me, &message.id).map(|item| (message.id.clone(), item.id.clone()))
            })
            .expect("an invitation with its entry in For me");
        let _ = app.update(Message::Act(entry.clone(), Command::Snooze));
        assert_eq!(app.menu, Some(Menu::Snooze));
        assert_eq!(app.subject.as_ref().map(|item| item.id.as_str()), Some(entry.as_str()), "the entry");
        assert_eq!(app.cursor.as_deref(), Some(message.as_str()), "the menu opens at the message's card");
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::Act(entry.clone(), Command::Hide));
        assert_eq!(app.menu, Some(Menu::Mute));
        let _ = app.update(Message::CloseMenu);
        let _ = press(&mut app, Key::S);
        assert_eq!(app.menu, Some(Menu::Snooze), "s on the message");
        let _ = app.update(Message::CloseMenu);
        let _ = app.update(Message::Act("gone".into(), Command::Snooze));
        assert_eq!(app.menu, None, "an entry the window does not hold");
        let _ = app.view();
    }

    /// Task E8.3: in the wide mail window an open mail stands in the reader beside the list; the
    /// other layouts and windows keep the panel.
    #[test]
    fn the_wide_mail_window_reads_beside_the_list() {
        let mut app = mail_window("comfortable", &["--open", "mail-entry"]);
        let id = app.detail.as_ref().map(|item| item.id.clone()).expect("a mail entry in the panel");
        let _ = app.update(Message::Resized(WIDE));
        let anchors = detail::Anchors::default();
        assert!(app.wide_reader(SizeClass::Wide, &anchors).is_some(), "the entry's mail is held");
        assert!(app.wide_reader(SizeClass::Medium, &anchors).is_none(), "medium: the panel");
        let _ = app.update(Message::CloseDetail);
        assert!(app.wide_reader(SizeClass::Wide, &anchors).is_none(), "nothing open, nothing to read");
        let _ = app.update(Message::Select(id));
        let mut hub = overview_at(&["--view", "hub", "--open", "mail-entry"]);
        let _ = hub.update(Message::Resized(WIDE));
        assert!(hub.wide_reader(SizeClass::Wide, &anchors).is_none(), "the hub keeps its panel");
        for density in ["comfortable", "compact"] {
            for open in ["mail-entry", "mail", "attachments", "overview"] {
                let mut app = mail_window(density, &["--open", open]);
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.view();
                }
            }
        }
    }

    /// Task E8.3: the keys work in the mail window as before – on cards as on rows.
    #[test]
    fn the_keys_of_the_list_work_on_the_mail_windows_cards() {
        let mut app = mail_window("comfortable", &["--open", "overview"]);
        let first = cursor(&app).map(str::to_owned).expect("the cursor on the first mail");
        let _ = press(&mut app, Key::R);
        assert!(matches!(app.sent.last(), Some(Sent::Compose(args)) if args.contains(&"--reply".to_owned())), "r");
        let _ = press(&mut app, Key::S);
        assert_eq!(app.menu, Some(Menu::Snooze), "s");
        let _ = app.update(Message::CloseMenu);
        let _ = press(&mut app, Key::M);
        assert_eq!(app.menu, Some(Menu::Mute), "m");
        let _ = app.update(Message::CloseMenu);
        let _ = press(&mut app, Key::T);
        assert_eq!(app.menu, Some(Menu::Task), "t");
        let _ = app.update(Message::CloseMenu);
        let _ = press(&mut app, Key::E);
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Done { id, done: true }, _)) if *id == first), "e");
        // The card's buttons send what the keys do.
        let _ = app.update(Message::Act(first.clone(), Command::Snooze));
        assert_eq!((app.menu, cursor(&app)), (Some(Menu::Snooze), Some(first.as_str())));
    }

    /// Task E8.3: the smart view lists the messages with attachments of every account.
    #[test]
    fn with_attachments_lists_the_mail_with_attachments_of_every_account() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Show(View::Attachments));
        load(&mut app);
        assert!(!app.messages.is_empty());
        assert!(app.messages.iter().all(|message| message.has_attachments));
        let accounts: Vec<&str> = app.messages.iter().map(|message| message.account.as_str()).collect();
        assert!(accounts.iter().any(|account| *account != accounts[0]), "both accounts: {accounts:?}");
        assert_eq!(app.title(), trf("Mail · {view}", &[("view", &tr("With attachments"))]));
        let _ = press(&mut app, Key::Down);
        let _ = press(&mut app, Key::Enter);
        assert!(app.panel_open(), "Enter reads the message");
    }

    /// Task E8.3b: "Invitations" lists the invitations of every account, the soonest first; Accept
    /// sends `mail.respond_to_invitation` once until the core answered, and the answer stays on the
    /// card and in the reader, also over later loads.
    #[test]
    fn invitations_are_listed_and_answered_once_at_a_time() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Show(View::Invitations));
        load(&mut app);
        assert_eq!(app.title(), trf("Mail · {view}", &[("view", &tr("Invitations"))]));
        let starts: Vec<_> = app.messages.iter().filter_map(|m| m.invite.as_ref().map(|i| i.start)).collect();
        assert_eq!(starts.len(), app.messages.len(), "only invitations");
        assert_eq!(starts.len(), 2, "the demo has two");
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]), "the soonest first");
        let first = app.messages[0].clone();
        let answer = |answer| Message::AnswerInvitation(first.account.clone(), first.id.clone(), answer);
        let _ = app.update(answer(InvitationAnswer::Accept));
        let _ = app.update(answer(InvitationAnswer::Decline));
        let asked: Vec<&Sent> = app.sent.iter().filter(|sent| matches!(sent, Sent::Answer(..))).collect();
        assert_eq!(asked.len(), 1, "one answer at a time");
        assert!(matches!(asked[0], Sent::Answer(_, id, InvitationAnswer::Accept, _) if *id == first.id));
        let _ = app.view();

        let _ =
            app.update(Message::InvitationAnswered(first.id.clone(), InvitationAnswer::Accept, Ok(Delivery::Accepted)));
        assert_eq!(app.answering, None);
        // The hike's mail has an entry: the hint comes with marking it done (task E8.3c).
        let hint = match app.sent.last() {
            Some(Sent::Edit(Edit::Done { done: true, .. }, Some(after))) => Some(after.text.as_str()),
            _ => app.hint.as_ref().map(|hint| hint.text.as_str()),
        };
        assert_eq!(hint, Some(tr("Accepted. The event is in your calendar.")));
        load(&mut app);
        let shown = app.messages.iter().find(|m| m.id == first.id).and_then(|m| m.invite.clone());
        assert_eq!(shown.and_then(|invite| invite.answered), Some(InvitationAnswer::Accept), "kept over the load");

        let _ = app.update(answer(InvitationAnswer::Tentative));
        let failed = Err("the outgoing mail server cannot be reached".to_owned());
        let _ = app.update(Message::InvitationAnswered(first.id.clone(), InvitationAnswer::Tentative, failed));
        assert!(app.error.is_some() && app.answering.is_none());
        load(&mut app);
        let shown = app.messages.iter().find(|m| m.id == first.id).and_then(|m| m.invite.clone());
        assert_eq!(shown.and_then(|invite| invite.answered), Some(InvitationAnswer::Accept), "a failed answer is none");
        for size in SIZES {
            let mut app = mail_window("comfortable", &["--open", "invitation"]);
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
            let mut compact = mail_window("compact", &["--open", "invitation"]);
            let _ = compact.update(Message::Resized(size));
            let _ = compact.view();
        }
    }

    /// Task E8.3c: an entry of "For me" carries the invitation of its mail; answering it marks the
    /// entry done, and the hint's undo opens the entry alone again. An answer that may not have gone
    /// out leaves it open.
    #[test]
    fn answering_an_invitation_marks_its_entry_done() {
        let mut app = mail_window("comfortable", &[]);
        let hike = app.items.iter().find(|item| item.id == "demo-mail-4").cloned().expect("Paul's mail");
        assert!(hike.invite.as_ref().is_some_and(|invite| invite.all_day), "the hike, all Saturday");
        let Some(Link::Mail { message }) = hike.link.clone() else { panic!("a mail entry links its mail") };
        let accept = list::invite_actions(&hike, app.now).remove(0).on_press;
        let _ = app.update(accept);
        assert!(
            matches!(app.sent.last(), Some(Sent::Answer(account, id, InvitationAnswer::Accept, None))
                if *account == hike.source.key() && *id == message),
            "{:?}",
            app.sent.last()
        );
        let _ = app.view();
        let _ =
            app.update(Message::InvitationAnswered(message.clone(), InvitationAnswer::Accept, Ok(Delivery::Accepted)));
        let text = tr("Accepted. The event is in your calendar.").to_owned();
        let undo = Some(Edit::Done { id: hike.id.clone(), done: false });
        let done = Sent::Edit(Edit::Done { id: hike.id.clone(), done: true }, Some(After { text, undo }));
        assert_eq!(app.sent.last(), Some(&done));

        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::AnswerInvitation(hike.source.key(), message.clone(), InvitationAnswer::Tentative));
        let before = app.sent.len();
        let _ = app.update(Message::InvitationAnswered(message, InvitationAnswer::Tentative, Ok(Delivery::Unknown)));
        assert!(app.sent[before..].iter().all(|sent| !matches!(sent, Sent::Edit(..))), "the entry stays open");
        let hint = app.hint.as_ref().expect("a hint");
        assert_eq!(
            (hint.text.as_str(), hint.undo.is_none()),
            (tr("The answer may not have gone out. Check your sent mail."), true)
        );
    }

    /// Task E8.3c: reading an invitation asks for the events around it and – once – the calendars;
    /// the card offers the calendar, and the one picked goes with the answer: the first one before a
    /// pick, none while the calendars are not in (the core chooses then).
    #[test]
    fn the_calendar_picked_goes_with_the_answer() {
        let mut app = mail_window("comfortable", &["--open", "invitation"]);
        let summary = app.messages.first().cloned().expect("the first invitation");
        let invite = summary.invite.clone().expect("an invitation");
        assert_eq!(app.letter.as_ref().map(|letter| letter.id.as_str()), Some(summary.id.as_str()), "read at once");
        deliver_letter(&mut app, &summary.id);
        let (start, end) = mail::invite_span(&invite);
        assert!(app.sent.contains(&Sent::Events(summary.id.clone(), start, end)), "{:?}", app.sent);

        let answer = |app: &mut App<DemoSource>| {
            let _ = app.update(Message::AnswerInvitation(
                summary.account.clone(),
                summary.id.clone(),
                InvitationAnswer::Accept,
            ));
            let sent = app.sent.iter().rev().find_map(|sent| match sent {
                Sent::Answer(_, _, _, collection) => Some(collection.clone()),
                _ => None,
            });
            let failed = Err("the outgoing mail server cannot be reached".to_owned());
            let _ = app.update(Message::InvitationAnswered(summary.id.clone(), InvitationAnswer::Accept, failed));
            sent.expect("an answer")
        };
        assert_eq!(answer(&mut app), None, "no calendars yet: the core's choice");
        let calendars = block_on(app.inbox.list_calendars()).map_err(|err| err.to_string());
        let _ = app.update(Message::InviteCalendars(calendars));
        assert_eq!(app.invite_calendar(), Some(&demo_calendar(0)), "the first, as the core would take");
        assert_eq!(answer(&mut app), Some(demo_calendar(0).id));
        let _ = app.update(Message::InviteCalendar(demo_calendar(1)));
        assert_eq!(answer(&mut app), Some(demo_calendar(1).id), "the one picked");

        let events = block_on(MailSource::list_events(app.inbox.as_ref(), start, end)).map_err(|err| err.to_string());
        let _ = app.update(Message::InviteEvents(summary.id.clone(), events));
        let reading = app.reading().expect("the invitation is read");
        assert!(reading.invite.events.is_some() && reading.invite.calendars.is_some());
        assert_eq!(reading.invite.calendar, Some(&demo_calendar(1)));
        draw_everywhere(&mut app);
        let _ = app.update(Message::InviteEvents("m:other".into(), Ok(Vec::new())));
        assert!(
            app.invite_events.as_ref().is_some_and(|(id, _)| *id == summary.id),
            "another mail's events are dropped"
        );
    }

    // ----- Notifications window (E8.4) ------------------------------------------------------

    /// The notifications window at the demo's fixed time, loaded on "New"; `more` add to its
    /// parameters.
    fn notifications_window(more: &[&str]) -> App<DemoSource> {
        let mut words = vec!["--view", "notifications"];
        words.extend_from_slice(more);
        overview_at(&words)
    }

    /// What the demo source holds for `id`, whatever the window shows.
    fn stored(app: &App<DemoSource>, id: &str) -> Item {
        let all = block_on(app.inbox.list_items(ItemFilter::default())).expect("the demo answers");
        all.into_iter().find(|item| item.id == id).expect("in the demo")
    }

    /// Task E8.4: `--source` chooses the sources of the notifications window, as often as given;
    /// mail is no source of it. The other windows keep `--source` as their start view.
    #[test]
    fn source_chooses_the_sources_of_the_notifications_window() {
        let shown = |words: &[&str]| Launch::parse(&args(words)).map(|launch| (launch.view, launch.shown));
        assert_eq!(shown(&["--view", "notifications"]), Ok((None, Shown::Apps)));
        let two = shown(&["--view", "notifications", "--source", "script:ci", "--source=agent:codex"]);
        assert_eq!(two, Ok((None, Shown::Only(vec![SourceRef::script("ci"), SourceRef::agent("codex")]))));
        let lane = shown(&["--view", "notifications", "--lane", "info", "--source", "script:ci"]);
        assert_eq!(lane, Ok((Some(View::Info), Shown::Only(vec![SourceRef::script("ci")]))), "with a lane");
        for mail in ["mail", "mail:anna@example.org"] {
            assert!(matches!(
                shown(&["--view", "notifications", "--source", mail]),
                Err(LaunchError::UnknownValue { flag: "--source", .. })
            ));
        }
        assert_eq!(
            shown(&["--source", "script:ci"]),
            Ok((Some(View::Source(SourceRef::script("ci"))), Shown::Everything))
        );
        let key =
            Launch::parse(&args(&["--view", "notifications", "--source", "script:ci", "--source", "script:deploy"]))
                .expect("valid")
                .density_key();
        assert_eq!(key, "notifications/source:script:ci+script:deploy", "each window its own density");
    }

    /// Task E8.4: two notifications windows with different sources show only theirs – in the list,
    /// the counts and the glance.
    #[test]
    fn two_windows_with_different_sources_show_only_theirs() {
        let ci = notifications_window(&["--source", "script:ci"]);
        let theirs = notifications_window(&["--source", "script:deploy", "--source", "agent:claude-code"]);
        assert_eq!(ids(&ci), ["demo-8"]);
        let mut both = ids(&theirs);
        both.sort_unstable();
        assert_eq!(both, ["demo-1", "demo-4", "demo-6"]);
        assert_eq!((ci.for_me.len(), ci.info), (0, 1));
        assert!(theirs.for_me.iter().all(|item| theirs.shown.shows(&item.source)), "the glance too");
        // Without `--source`: what the apps report – no agents, no approvals, no mail.
        let apps = notifications_window(&[]);
        assert!(!apps.items.is_empty());
        let no_agents_suite_or_mail =
            |item: &Item| !matches!(item.source.kind, SourceKind::Agent | SourceKind::Suite | SourceKind::Mail);
        assert!(apps.items.iter().all(no_agents_suite_or_mail), "{:?}", ids(&apps));
        assert_eq!(apps.count_note(), apps.items.len().to_string(), "New counts both lanes");
        assert_eq!(apps.nav_entries().0.len(), 3, "New, Later, Done");
    }

    /// Task E8.4: the sidebar's checks and the chips switch a source on and off; the last one stays.
    #[test]
    fn a_source_switches_on_and_off_and_the_last_one_stays() {
        let mut app = notifications_window(&["--source", "script:ci"]);
        let _ = app.update(Message::ToggleSource(SourceRef::script("deploy")));
        load(&mut app);
        let mut shown = ids(&app);
        shown.sort_unstable();
        assert_eq!(shown, ["demo-6", "demo-8"]);
        let _ = app.update(Message::ToggleSource(SourceRef::script("ci")));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-6"]);
        let _ = app.update(Message::ToggleSource(SourceRef::script("deploy")));
        assert_eq!(app.shown, Shown::Only(vec![SourceRef::script("deploy")]), "a window always shows a source");
        let agents = app.listed_sources();
        assert!(agents.contains(&SourceRef::agent("claude-code")), "agents can be switched on");
    }

    /// Task E8.4: the ✕ dismisses – done, sent as dismissing –, and undo opens the entry again, as
    /// after done. `x` does the same; approvals and the mail window have no ✕.
    #[test]
    fn dismissing_marks_the_entry_done_and_undo_opens_it_again() {
        let mut app = notifications_window(&[]);
        let _ = app.update(Message::Act("demo-6".into(), Command::Dismiss));
        let dismiss = Edit::Dismiss { id: "demo-6".into() };
        let reopen = Edit::Done { id: "demo-6".into(), done: false };
        let after = After { text: tr("Dismissed").into(), undo: Some(reopen.clone()) };
        assert_eq!(app.sent.last(), Some(&Sent::Edit(dismiss, Some(after))));
        deliver(&mut app);
        assert!(!ids(&app).contains(&"demo-6"), "gone from New");
        assert!(matches!(stored(&app, "demo-6").state, State::Done { .. }), "done in the source, as after Done");
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(tr("Dismissed")));
        // Issue #117: simply gone – "Done" lists only what was done for real.
        let mut done = notifications_window(&[]);
        block_on(done.inbox.dismiss_item("demo-6".into())).expect("the demo dismisses");
        block_on(done.inbox.set_item_done("demo-7".into(), true)).expect("the demo marks it done");
        let _ = done.update(Message::Show(View::Done));
        load(&mut done);
        assert!(!ids(&done).contains(&"demo-6"), "not in Done: {:?}", ids(&done));
        assert!(ids(&done).contains(&"demo-7"), "done for real: {:?}", ids(&done));
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Edit(reopen, None)));
        deliver(&mut app);
        assert!(ids(&app).contains(&"demo-6"), "back");
        assert!(stored(&app, "demo-6").state.is_open());

        // `x` on the cursor's row.
        let _ = press(&mut app, Key::X);
        assert!(matches!(app.sent.last(), Some(Sent::Edit(Edit::Dismiss { id }, _)) if id == "demo-6"));
        // An approval is never dismissed, nor is mail in the mail window.
        let mut suite = notifications_window(&["--source", "suite"]);
        let _ = suite.update(Message::Act("demo-3".into(), Command::Dismiss));
        assert!(suite.sent.is_empty(), "an approval waits for yes or no");
        let mut mail = mail_window("comfortable", &[]);
        let _ = press(&mut mail, Key::Down);
        let _ = press(&mut mail, Key::X);
        assert!(mail.sent.is_empty(), "the mail window dismisses nothing");
    }

    /// Task E8.4: "Dismiss all" dismisses what "New" shows, but approvals and the suite's entries,
    /// and its undo opens them all again.
    #[test]
    fn dismiss_all_dismisses_what_new_shows_and_undoes_it() {
        let mut app =
            notifications_window(&["--source", "suite", "--source", "script:deploy", "--source", "script:ci"]);
        let _ = app.update(Message::DismissAll);
        let Some(Sent::DismissAll(ids)) = app.sent.last().cloned() else { panic!("no Dismiss all: {:?}", app.sent) };
        assert_eq!(ids, ["demo-6", "demo-8"], "the approval and the account problem stay");
        deliver(&mut app);
        let text = trf("{n} dismissed", &[("n", &2)]);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.as_str()), Some(text.as_str()));
        let mut left = self::ids(&app);
        left.sort_unstable();
        // The suite's entries stay, its "You finished a task from a mail" of the demo too (E8.7b).
        assert_eq!(left, ["demo-3", "demo-5", "demo-9"]);
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Reopen(ids)));
        deliver(&mut app);
        assert_eq!(app.items.len(), 5, "all back");
        // Nothing to dismiss in Later.
        let _ = app.update(Message::Show(View::Snoozed));
        load(&mut app);
        let sent = app.sent.len();
        let _ = app.update(Message::DismissAll);
        assert_eq!(app.sent.len(), sent);
    }

    /// Task E8.6b: `m` and the bell open the bell's menu in the notifications window with the ways
    /// the core offers (`inbox.list_mute_options`); the suite is never muted.
    #[test]
    fn the_bell_opens_its_menu_with_the_ways_the_core_offers() {
        let mut app = notifications_window(&[]);
        let _ = app.update(Message::Act("demo-6".into(), Command::Hide));
        assert_eq!((app.menu, app.focus()), (Some(Menu::Mute), Focus::Mute));
        assert_eq!(app.sent.last(), Some(&Sent::MuteOptions("demo-6".into())));
        assert!(app.choices().is_empty(), "nothing to pick while they load");
        let _ = app.view();
        deliver(&mut app);
        let offered: Vec<String> = rules::mute_options(&stored(&app, "demo-6")).into_iter().map(|o| o.label).collect();
        let labels: Vec<String> = app.choices().into_iter().map(|choice| choice.label).collect();
        assert_eq!(labels, offered, "the core's words, in its order");
        assert_eq!(labels.first().map(String::as_str), Some(tr("This conversation")));
        let _ = app.view();
        // An answer for another entry is dropped.
        let _ = app.update(Message::MuteOptions("demo-1".into(), Ok(Vec::new())));
        assert_eq!(app.choices().len(), offered.len());

        let mut suite = notifications_window(&["--source", "suite"]);
        let _ = suite.update(Message::Act("demo-5".into(), Command::Hide));
        assert!(suite.sent.is_empty() && suite.menu.is_none());
        assert_eq!(suite.hint.map(|hint| hint.text), Some(tr("Entries from the suite can't be muted.").to_owned()));
    }

    /// Task E8.6b: "For" and "What happens" go with the way picked; the toast says the rule as the
    /// control centre does, and its undo removes the rule and opens what it closed.
    #[test]
    fn muting_by_a_trait_keeps_a_rule_and_undo_takes_it_back() {
        let mut app = notifications_window(&[]);
        let _ = app.update(Message::Act("demo-6".into(), Command::Hide));
        deliver(&mut app);
        assert_eq!((app.mute_for, app.mute_effect), (MuteFor::Always, MuteEffect::Info), "the defaults");
        // ← ← : one hour; ↓ : hide.
        for key in [Key::Left, Key::Left, Key::Left, Key::Down] {
            let _ = press(&mut app, key);
        }
        assert_eq!((app.mute_for, app.mute_effect), (MuteFor::OneHour, MuteEffect::Hide), "stops at the ends");
        let client = MuteScope::Facet { key: FacetKey::Client, nth: 0 };
        let place =
            app.choices().iter().position(|c| matches!(c.message, Message::MuteBy(_, scope) if scope == client));
        let digit = place.map(|at| u8::try_from(at + 1).expect("a digit")).expect("the client is offered");
        let _ = press(&mut app, Key::Digit(digit));
        let until = Some(app.now + TimeDelta::hours(1));
        assert_eq!(app.sent.last(), Some(&Sent::MuteItem("demo-6".into(), client, until, MuteEffect::Hide)));
        assert_eq!(app.menu, None, "picking closes the menu");
        deliver(&mut app);
        let hint = app.hint.clone().expect("a toast");
        let Some(Undo::Muted { rule: Some(rule), reopen, moved }) = hint.undo.clone() else {
            panic!("an undo with the rule: {hint:?}")
        };
        assert!(reopen.contains(&"demo-6".to_owned()) && moved.is_empty());
        let deploy = trf("Everything from {source}", &[("source", &"deploy")]);
        let when = until_text(app.now + TimeDelta::hours(1), app.now);
        assert_eq!(hint.text, trf("Hidden until {when}: {what}", &[("what", &deploy), ("when", &when)]));
        assert!(!ids(&app).contains(&"demo-6"), "hidden");

        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Unmute(Some(rule.clone()), reopen, Vec::new())));
        deliver(&mut app);
        assert!(stored(&app, "demo-6").state.is_open(), "open again");
        assert_eq!(block_on(app.inbox.remove_rule(rule)), Err(InboxError::NotFound), "and the rule is gone");
        assert_eq!((app.mute_for, app.mute_effect), (MuteFor::OneHour, MuteEffect::Hide), "kept for the next time");
    }

    /// Task E8.6b: the conversation moved to Info for good keeps no rule; the undo moves it back.
    #[test]
    fn the_conversation_for_good_moves_only_this_entry() {
        let mut app = notifications_window(&[]);
        let id = app.items.iter().find(|item| item.lane == Lane::ForMe && notifications::bell(item).is_some());
        let id = id.map(|item| item.id.clone()).expect("an entry for me with a bell");
        let _ = app.update(Message::Act(id.clone(), Command::Hide));
        deliver(&mut app);
        let _ = app.update(Message::MuteHow(MuteEffect::Info));
        let _ = app.update(Message::MuteFor(MuteFor::Always));
        let _ = press(&mut app, Key::Digit(1));
        assert_eq!(app.sent.last(), Some(&Sent::MuteItem(id.clone(), MuteScope::Conversation, None, MuteEffect::Info)));
        deliver(&mut app);
        let hint = app.hint.clone().expect("a toast");
        assert_eq!(hint.text, trf("Moved to {lane}", &[("lane", &tr("Info"))]));
        assert_eq!(hint.undo, Some(Undo::Muted { rule: None, reopen: Vec::new(), moved: vec![id.clone()] }));
        assert_eq!(stored(&app, &id).lane, Lane::Info);
        let _ = app.update(Message::Undo);
        deliver(&mut app);
        assert_eq!(stored(&app, &id).lane, Lane::ForMe, "back in For me");
    }

    /// Task E8.6b: a source muted as a whole is unmuted from the bell's menu.
    #[test]
    fn the_bells_menu_unmutes_a_muted_source() {
        let mut app = notifications_window(&[]);
        block_on(app.inbox.mute_source(SourceRef::script("deploy"), true)).expect("mute");
        load(&mut app);
        let _ = app.update(Message::Act("demo-6".into(), Command::Hide));
        deliver(&mut app);
        let last = app.choices().pop().expect("choices");
        assert_eq!(last.label, trf("Unmute {source}", &[("source", &"deploy")]));
        assert!(last.line && matches!(last.message, Message::AskMute(ref id) if id == "demo-6"));
    }

    /// Task S.15: "All rules" sends its own message, which opens the control centre's Inbox page;
    /// it is not the gear's `OpenSettings`, which opens the accounts page.
    #[test]
    fn all_rules_opens_the_inbox_page_not_the_accounts_page() {
        let rules = rules_entry();
        assert!(matches!(rules.on_press, Message::OpenRules));
        assert!(!matches!(rules.on_press, Message::OpenSettings));
    }

    /// Task E8.6b: the rule's words – effect, end and what it mutes – and when a muting ends.
    #[test]
    fn muting_reads_as_the_rules_of_the_control_centre() {
        let now = DateTime::parse_from_rfc3339("2026-10-14T08:00:00Z").expect("time").with_timezone(&Utc);
        let sender = torro_inbox::item::Facet { key: FacetKey::Sender, value: "news@shop.example".into() };
        let rule = |effect, until| Rule {
            id: "r".into(),
            source: SourceRef::mail("anna@example.org"),
            sender: None,
            domain: None,
            effect,
            created: now,
            facet: Some(sender.clone()),
            until,
        };
        let what = trf("Mail from {sender}", &[("sender", &"news@shop.example")]);
        let tomorrow = MuteFor::UntilTomorrow.until(now).expect("an end");
        let when = until_text(tomorrow, now);
        assert_eq!(when, trf("tomorrow {time}", &[("time", &"09:00")]));
        let hidden = rule(torro_inbox::RuleEffect::Muted, None);
        assert_eq!(mute_text(&hidden, now), trf("Hidden: {what}", &[("what", &what)]));
        let quiet = rule(torro_inbox::RuleEffect::NoPopup, Some(tomorrow));
        assert_eq!(mute_text(&quiet, now), trf("No popups until {when}: {what}", &[("what", &what), ("when", &when)]));
        let info = rule(torro_inbox::RuleEffect::Lane { lane: Lane::Info }, None);
        assert_eq!(mute_text(&info, now), trf("Only Info: {what}", &[("what", &what)]));
        let muted = Muted { rule: Some(info), new_rule: true, ..Muted::default() };
        assert_eq!(muted_note(&muted, MuteEffect::Info, now), trf("Only Info: {what}", &[("what", &what)]));
        assert_eq!(Undo::of_muted(&muted), Some(Undo::Muted { rule: Some("r".into()), reopen: vec![], moved: vec![] }));
        let again = Muted { new_rule: false, ..muted };
        assert_eq!(Undo::of_muted(&again), None, "a rule that was there stays");
        assert_eq!(muted_note(&Muted::default(), MuteEffect::Hide, now), tr("Conversation hidden"));
        assert_eq!(MuteFor::OneHour.until(now), Some(now + TimeDelta::hours(1)));
        assert_eq!(MuteFor::Always.until(now), None);
        assert_eq!(until_text(now + TimeDelta::hours(1), now).len(), 5, "today: the time alone");
    }

    /// Issue #127: the three windows say which one they are – "Notification Hub", "Mail" and "App
    /// Feed" in the title –, and the mail window's first view is not the tray of the folder "Inbox".
    #[test]
    fn the_windows_name_themselves_apart() {
        let view = View::Overview;
        assert_eq!(Kind::Hub.title(&view), trf("Notification Hub · {view}", &[("view", &tr("Act now"))]));
        assert_eq!(Kind::Mail.title(&view), trf("Mail · {view}", &[("view", &tr("For me"))]));
        assert_eq!(Kind::Notifications.title(&view), trf("App Feed · {view}", &[("view", &tr("New"))]));
        assert_ne!(Kind::Mail.view_icon(&view), Icon::Inbox, "the tray is the folder \"Inbox\"");
        assert_eq!(Kind::Mail.view_icon(&view), Icon::UserCheck);
        for kind in [Kind::Hub, Kind::Mail, Kind::Notifications] {
            assert!(!kind.app().display_name.contains("Inbox"), "{kind:?}");
        }
    }

    /// Task E8.6b: the bell's menu opens at the card or row, and at the panel's bell; it draws in
    /// every size and density, and "Move to Info" no longer has the bell. Task E8.10: the bell
    /// that mutes is crossed out, "Info" has its own icon and neither is the notifications' bell.
    #[test]
    fn the_bells_menu_draws_at_card_row_and_panel() {
        assert_eq!(lane_icon(Lane::Info), Icon::Info, "Info is for your information");
        assert!(![Icon::Bell, Icon::BellOff].contains(&lane_icon(Lane::Info)), "the bells notify and mute");
        assert_eq!(Kind::Notifications.view_icon(&View::Overview), Icon::Bell, "the bell stays for notifications");
        for density in ["compact", "comfortable"] {
            for open in ["mute-menu", "mute-panel"] {
                let mut app = notifications_window(&["--density", density, "--open", open]);
                assert_eq!(app.menu, Some(Menu::Mute), "{open}");
                assert_eq!(app.detail.is_some(), open == "mute-panel");
                deliver(&mut app);
                assert!(!app.choices().is_empty());
                let item = app.subject.clone().expect("the menu has its entry");
                let bell = crate::notifications::bell(&item).map(|bell| bell.icon);
                assert_eq!(bell, Some(Icon::BellOff), "{open}: the bell that mutes is crossed out");
                for size in SIZES {
                    let _ = app.update(Message::Resized(size));
                    let _ = app.view();
                }
            }
        }
        let menu = mute_menu_width(&[]);
        assert!(menu >= widgets::menu_width() && menu <= size::panel());
    }

    /// Task E8.4: the wide window shows a column per app; `j`/`k` walk it column after column, `h`/`l`
    /// go across; "By time" walks newest first, the narrow window "For me", then "Info".
    #[test]
    fn the_keys_walk_the_board_as_it_is_drawn() {
        let mut app = notifications_window(&[
            "--source",
            "script:deploy",
            "--source",
            "agent:claude-code",
            "--density",
            "comfortable",
        ]);
        let _ = app.update(Message::Resized(WIDE));
        assert!(matches!(app.board(), Board::Columns(_)), "{:?}", app.board());
        let columns = notifications::app_columns(&app.items, 3);
        let (first, second) = (columns[0].items[0].id.clone(), columns[1].items[0].id.clone());
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some(first.as_str()));
        let _ = press(&mut app, Key::L);
        assert_eq!(cursor(&app), Some(second.as_str()), "l: the next column");
        let _ = press(&mut app, Key::H);
        assert_eq!(cursor(&app), Some(first.as_str()), "h: back");
        assert_eq!(app.menu, None, "l asks no lane question here");

        let _ = app.update(Message::Arrange(Arrange::ByTime));
        assert!(matches!(app.board(), Board::Grid(_)));
        let newest = notifications::by_time(&app.items, Order::NewestFirst);
        let newest = newest.into_iter().map(|item| item.id.as_str()).collect::<Vec<_>>();
        assert_eq!(rows_of(&app), newest);

        let _ = app.update(Message::Resized(NARROW));
        assert_eq!(app.board(), Board::Single, "narrow: one column");
        let lanes: Vec<Lane> = app.rows().iter().map(|item| item.lane).collect();
        assert!(lanes.windows(2).all(|pair| pair[0] == pair[1] || pair[0] == Lane::ForMe), "For me, then Info");
    }

    /// Task E8.4: moving the cursor scrolls the board, which scrolls as one.
    #[test]
    fn moving_on_the_board_scrolls_it() {
        let mut app = notifications_window(&["--density", "comfortable"]);
        let _ = app.update(Message::Resized(NARROW));
        let first = app.rows()[0].id.clone();
        let (_, bottom) =
            notifications::span(&app.items, app.board(), &app.view, app.density(), &first, app.order).expect("shown");
        let _ = app.update(Message::Scrolled(0.0, bottom));
        assert_eq!(press(&mut app, Key::Down).units(), 0, "the first card is in view");
        assert!(press(&mut app, Key::Down).units() > 0, "the second needs a scroll");
        assert!(app.scroll.is_some_and(|(offset, _)| offset > 0.0));
    }

    /// Task E8.4: the notifications window draws in every size and both densities, by app and by
    /// time, with the panel, the menus, the drawer and the toast, in every view of its sidebar.
    #[test]
    fn the_notifications_window_builds_in_every_size() {
        for density in ["compact", "comfortable"] {
            for open in [
                &[][..],
                &["--open", "by-time"],
                &["--open", "empty"],
                &["--source", "suite", "--source", "agent:claude-code"],
            ] {
                let mut words = vec!["--density", density];
                words.extend_from_slice(open);
                let mut app = notifications_window(&words);
                for view in [View::Overview, View::Snoozed, View::Done] {
                    let _ = app.update(Message::Show(view));
                    load(&mut app);
                    for sidebar in [SidebarMode::Expanded, SidebarMode::Rail] {
                        let _ = app.update(Message::UiState(UiState { sidebar }));
                        for size in SIZES {
                            let _ = app.update(Message::Resized(size));
                            let _ = app.view();
                            let _ = press(&mut app, Key::Down);
                            let _ = app.view();
                            let _ = press(&mut app, Key::S);
                            let _ = app.view();
                            let _ = app.update(Message::CloseMenu);
                            let _ = app.update(Message::ShowDrawer);
                            let _ = app.view();
                            let _ = app.update(Message::CloseMenu);
                            if let Some(id) = app.cursor.clone() {
                                let _ = app.update(Message::Select(id));
                                let _ = app.view();
                                let _ = app.update(Message::CloseDetail);
                            }
                            app.note("A hint".into());
                            let _ = app.view();
                            app.hint = None;
                        }
                    }
                }
            }
        }
    }

    // Task with a way back (task E8.7b).

    /// The open mail window of the demo started with `--message=<message>`, loaded.
    fn mail_window_on(message: &str) -> App<DemoSource> {
        let mut launch =
            Launch::parse(&args(&["--demo", "--now", "2026-10-13T10:30", "--view", "mail"])).expect("valid parameters");
        launch.message = Some(message.into());
        let inbox = Arc::new(DemoSource::new(launch.start()));
        let mut app = App::new(inbox, Source::from_args(args(&["--demo"])), launch).0;
        load(&mut app);
        app
    }

    #[test]
    fn the_hub_asks_after_a_finished_task_and_marks_its_mail_done() {
        let mut app = overview_at(&[]);
        let question = app.find("demo-9").cloned().expect("the demo's question");
        assert!(hub::is_follow_up(&question));
        assert_eq!(hub::Section::of(&question), Some(hub::Section::FollowUp), "from Info into Act now");
        assert_eq!(hub::acts(&question), [hub::Act::Reply, hub::Act::MarkMailDone, hub::Act::NotNow]);
        // Reply answers the mail it waits in: Lena's, of the entry `demo-mail-2`.
        let _ = app.update(Message::Act("demo-9".into(), Command::Reply));
        assert_eq!(app.sent.last(), Some(&Sent::Compose(mail::compose_args(Compose::Reply, "m:w2", true))));
        // "Mark mail done": the mail's entry, then the question; the toast opens both again.
        let _ = app.update(Message::MarkMailDone("demo-9".into()));
        let ids = vec!["demo-mail-2".to_owned(), "demo-9".to_owned()];
        assert_eq!(app.sent.last(), Some(&Sent::MailDone(ids.clone())));
        for id in &ids {
            block_on(app.inbox.set_item_done(id.clone(), true)).expect("the demo closes it");
        }
        let _ = app.update(Message::MailDone(ids.clone(), None));
        load(&mut app);
        assert_eq!(app.hint.as_ref().and_then(|hint| hint.undo.clone()), Some(Undo::Dismissed(ids.clone())));
        assert!(app.find("demo-mail-2").is_none() && app.find("demo-9").is_none(), "both done");
        let _ = press(&mut app, Key::U);
        assert_eq!(app.sent.last(), Some(&Sent::Reopen(ids)));
        // Another entry has no "Mark mail done".
        let before = app.sent.len();
        let _ = app.update(Message::MarkMailDone("demo-1".into()));
        assert_eq!(app.sent.len(), before);
    }

    /// Task E8.1b: the notifications window showing the suite asks after a finished task too, with
    /// the hub's Reply · Mark mail done · Not now, and closes the mail's entry it does not show.
    #[test]
    fn the_notifications_window_asks_after_a_finished_task_too() {
        // Two sources, so the window asks for every entry: the demo's question is `uptime`'s notice
        // until `demo_task` makes it the suite's.
        let mut app = notifications_window(&["--source", "suite", "--source", "script:uptime"]);
        let question = app.find("demo-9").cloned().expect("the suite's question");
        assert!(hub::is_follow_up(&question));
        let words: Vec<String> =
            notifications::card_buttons(&question, false, false).into_iter().map(|b| b.label).collect();
        assert_eq!(words, [tr("Reply"), tr("Mark mail done"), tr("Not now")]);
        draw_everywhere(&mut app);
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.update(Message::MarkMailDone("demo-9".into()));
        let ids = vec!["demo-mail-2".to_owned(), "demo-9".to_owned()];
        assert_eq!(app.sent.last(), Some(&Sent::MailDone(ids)), "the mail's entry, though this window shows no mail");
    }

    #[test]
    fn handed_off_lists_the_open_tasks_made_of_entries() {
        let app = overview_at(&[]);
        let handed: Vec<&str> =
            hub::handed_off(&app.items, &app.done).into_iter().map(|task| task.uid.as_str()).collect();
        assert_eq!(handed, ["quarterly-report@torro-demo", "demo-review-settings"], "an open entry's, a done one's");
        assert!(!handed.contains(&"sidebar-drafts@torro-demo"), "a finished task is not handed off");
        let mut app = app;
        let _ = app.update(Message::OpenTask("quarterly-report@torro-demo".into()));
        assert_eq!(
            app.sent.last(),
            Some(&Sent::Target("torrotasks".into(), "task:quarterly-report@torro-demo".into())),
            "the tasks app on the task, through the core"
        );
        draw_everywhere(&mut app);
    }

    #[test]
    fn a_mail_shows_its_task_and_a_folder_message_becomes_one() {
        // A mail entry's mail carries the entry's task.
        let mut app = overview_at(&["--view", "mail"]);
        let _ = app.update(Message::Select("demo-mail-1".into()));
        let _ = app.update(Message::Resized(WIDE));
        deliver_letter(&mut app, "m:w1");
        let task = app.reading().and_then(|reading| reading.task).map(|task| task.uid.clone());
        assert_eq!(task.as_deref(), Some("quarterly-report@torro-demo"));
        draw_everywhere(&mut app);
        // A folder's message: `t` opens the dialog, which asks for a due day as for an entry (task
        // E8.1b); "Create task" sends `tasks.create_todo_from_mail` with it, and the reader shows the
        // task with its day.
        let mut app = in_folder("f:work-archive");
        let _ = press(&mut app, Key::Down);
        let id = cursor(&app).map(str::to_owned).expect("a message");
        let _ = press(&mut app, Key::T);
        assert_eq!((app.menu, app.task_mail.as_ref().map(|m| m.id.clone())), (Some(Menu::Task), Some(id.clone())));
        assert_eq!(app.task.due, Due::None, "no due day until one is chosen");
        deliver_lists(&mut app);
        let _ = app.view();
        let _ = app.update(Message::TaskDue(Due::Tomorrow));
        let _ = app.update(Message::TaskConfirm);
        let Some(Sent::MailTask { account, id: sent, list, title, due }) = app.sent.pop() else {
            panic!("no mail task")
        };
        assert_eq!((account.as_str(), sent.as_str()), (torro_inbox::demo::MAIL_ACCOUNTS[0], id.as_str()));
        let tomorrow = local_today(app.now) + Days::new(1);
        assert_eq!(due, Some(tomorrow), "the day chosen");
        let made =
            block_on(app.inbox.create_todo_from_mail(account, sent, list, title, due)).map_err(|e| e.to_string());
        let _ = app.update(Message::MailTaskCreated(id.clone(), made));
        let task = app.task_of(&id).expect("the reader shows the task made");
        assert_eq!(task.due, Some(torro_inbox::item::TaskDue::Date(tomorrow)));
        let _ = app.update(Message::Mail(MailAction::Read(true), id));
        draw_everywhere(&mut app);
    }

    #[test]
    fn the_mail_window_starts_on_the_mail_of_message() {
        // A mail with an entry: the entry in the panel.
        let app = mail_window_on("m:w2");
        assert_eq!(app.detail.as_ref().map(|item| item.id.as_str()), Some("demo-mail-2"));
        // A mail without one: loaded, then its folder with the mail in the reader.
        let mut app = mail_window_on("m:w9");
        assert_eq!(app.sent.last(), Some(&Sent::Letter("m:w9".into())));
        let mail = block_on(MailSource::get_mail(app.inbox.as_ref(), "m:w9".into())).map(Box::new);
        let _ = app.update(Message::StartMail("m:w9".into(), mail.map_err(|err| err.to_string())));
        load(&mut app);
        assert_eq!(app.view.folder().map(|folder| folder.id.as_str()), Some("f:work-archive"));
        assert_eq!((cursor(&app), app.reading, app.wanted_letter()), (Some("m:w9"), true, Some("m:w9".into())));
        // Any other window ignores it.
        let mut launch = Launch::parse(&args(&["--demo", "--view", "hub"])).expect("valid");
        launch.message = Some("m:w9".into());
        let mut hub =
            App::new(Arc::new(DemoSource::new(launch.start())), Source::from_args(args(&["--demo"])), launch).0;
        load(&mut hub);
        assert!(hub.detail.is_none() && !hub.sent.contains(&Sent::Letter("m:w9".into())));
    }

    /// The numbers of Insights' span, as the task of [`App::load_stats`] would hand them in.
    fn deliver_stats(app: &mut App<DemoSource>) {
        let range = app.insights.range;
        let stats = block_on(app.inbox.get_stats(range, Some(local_today(app.now)))).map_err(|err| err.to_string());
        let _ = app.update(Message::Stats(range, stats));
    }

    /// Task E8.8b: "Insights" in the hub's sidebar shows the page on the overview's entries; no key
    /// of an entry reaches "Act now" behind it, and "Act now" brings it back.
    #[test]
    fn insights_is_a_page_of_the_hub() {
        let mut hub = overview_at(&["--view", "hub"]);
        let _ = hub.update(Message::ShowInsights);
        assert!(hub.is_insights() && hub.view == View::Overview);
        assert_eq!(hub.title(), trf("Notification Hub · {view}", &[("view", &tr("Insights"))]));
        assert_eq!(hub.focus(), Focus::Elsewhere, "j, e and the others act on no hidden row");
        let active: Vec<String> =
            hub.nav_entries().0.into_iter().filter(|entry| entry.active).map(|entry| entry.label).collect();
        assert_eq!(active, [tr("Insights")]);
        assert!(hub.insights.stats.is_none(), "loading");
        deliver_stats(&mut hub);
        assert!(matches!(&hub.insights.stats, Some(Ok(stats)) if stats.range == torro_ipc::StatsRange::Week));
        draw_everywhere(&mut hub);
        let _ = hub.update(Message::Show(View::Overview));
        assert!(!hub.is_insights() && hub.is_hub(), "Act now again");
        let _ = hub.update(Message::ShowInsights);
        let _ = hub.update(Message::ShowToSort);
        assert!(!hub.is_insights() && hub.sorting);
        // The other windows have no Insights, not even from `--open`.
        assert!(!overview_at(&["--view", "notifications", "--open", "insights"]).is_insights());
    }

    /// Task E8.8b: another span is asked anew, an answer for the span left is dropped, and the table
    /// switch only changes how the numbers show.
    #[test]
    fn insights_switches_span_and_table() {
        let mut hub = overview_at(&["--view", "hub", "--open", "insights"]);
        deliver_stats(&mut hub);
        let week = hub.insights.stats.clone();
        let _ = hub.update(Message::StatsRange(torro_ipc::StatsRange::Quarter));
        assert!(hub.insights.stats.is_none(), "the quarter loads");
        let _ = hub.update(Message::Stats(torro_ipc::StatsRange::Week, week.clone().expect("loaded")));
        assert!(hub.insights.stats.is_none(), "the week's late answer is dropped");
        deliver_stats(&mut hub);
        assert!(matches!(&hub.insights.stats, Some(Ok(stats)) if stats.days.len() == 90));
        let _ = hub.update(Message::StatsTable(true));
        assert!(hub.insights.table && hub.insights.stats.is_some());
        draw_everywhere(&mut hub);
        let table = overview_at(&["--view", "hub", "--open", "insights-table"]);
        assert!(table.is_insights() && table.insights.table);
        let mut failed = overview_at(&["--view", "hub", "--open", "insights"]);
        let _ = failed.update(Message::Stats(torro_ipc::StatsRange::Week, Err(DEMO_ERROR.into())));
        draw_everywhere(&mut failed);
    }

    /// Task E8.8b: a suggestion's button mutes its entry by the suggestion's way with the window's
    /// "For", and the toast offers the undo; task E8.12 (finding 14): with the effect of its ready
    /// suggestion – a newsletter's list is hidden ("Hide list"), not the window's "Only Info".
    #[test]
    fn a_suggestion_mutes_with_an_undo() {
        let mut hub = overview_at(&["--view", "hub", "--open", "insights"]);
        deliver_stats(&mut hub);
        let Some(Ok(stats)) = hub.insights.stats.clone() else { panic!("numbers") };
        let cut = insights::cuts(&stats).into_iter().next().expect("a suggestion");
        assert_eq!(cut.word, tr("Hide list"));
        let suggestion = &stats.suggestions[0];
        let _ = hub.update(cut.mute);
        let expected = Sent::MuteItem(suggestion.item.clone(), suggestion.option.scope, None, MuteEffect::Hide);
        assert_eq!(hub.sent.last(), Some(&expected), "always (the window's For), hidden (the suggestion's)");
        assert_eq!(hub.mute_effect, MuteEffect::Info, "the bell's own choice stays");
        deliver(&mut hub);
        let hint = hub.hint.clone().expect("a toast");
        assert!(hint.undo.is_some(), "{hint:?}");
        assert!(hub.is_insights(), "the page stays");
    }

    /// Task E8.12 (finding 7, small things): the dialogs open at the panel's button with their left
    /// edge, so they stay over the panel; the narrow notifications window counts its view ("New 5");
    /// the bell's menu shows its digits quietly, the other menus as keys; every size still draws.
    #[test]
    fn dialogs_keep_to_the_panel_and_narrow_notifications_count() {
        assert_eq!(dialog_placement(true), Placement::Below(Align::Start));
        assert_eq!(dialog_placement(false), Placement::Below(Align::End));
        assert!(narrow_count(Kind::Notifications));
        assert!(!narrow_count(Kind::Mail) && !narrow_count(Kind::Hub), "the page says it big there");
        let mut app = notifications_window(&["--open", "mute-menu"]);
        assert_eq!(app.count_note(), app.items.len().to_string(), "the number beside “New”");
        draw_everywhere(&mut app);
        let mut panel = notifications_window(&["--open", "mute-panel"]);
        draw_everywhere(&mut panel);
        let mut event = overview_at(&["--view", "hub", "--open", "event"]);
        draw_everywhere(&mut event);
    }
}
