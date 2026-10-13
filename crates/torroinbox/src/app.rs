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
//! header bar "Dismiss all" with an undo ([`Undo::Dismissed`]); `m` and the bell mute the source.
//!
//! The hub (task E8.5, `hub.rs`) is about acting: its overview is "Act now" ([`App::is_hub`]) – one
//! tile per origin over the sections Decide (approvals), Agents and To sort, each entry with its
//! buttons always shown, and in the wide layout "Coming back" at the right (the snoozed entries,
//! [`Loaded::snoozed`]). A click on a tile filters the sections ([`App::filter`]). Its sidebar lists
//! Act now, To sort (the section alone, [`App::sorting`]), Later, Done today and the sources. An
//! approval has neither ✕ nor Later anywhere: `s` on it only says that it waits for a decision.
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
//! ([`App::rows`]). Every row has an action bar (Task, Done, Later, Hide), whose buttons do what the
//! keys do on the row ([`Message::Act`]).
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
//! asking "always for this source?" ([`Edit::Lane`]), and hide it from the inbox in steps (`m`
//! or the panel's "…", task E3.10, [`Menu::Hide`]): for mail "This conversation" (`inbox.hide_sender`
//! with `conversation`: done, but the mail stays unread, task E3.11), "Everything from" its sender or
//! its sender's domain (`inbox.hide_sender`: a rule for every mail account; the mail stays unread and
//! in place), "Always to Info" and "Move to junk" (only with a junk folder in the account); for
//! other sources "This entry" (done) and muting or unmuting the source at once (E3.2c: no dialog –
//! a change with an undo needs no question as well). Every change shows a toast for
//! [`UNDO_FOR`] (`widgets::toast`, task E3.11) with "Undo" (`u`, and Ctrl + Z / ⌘ + Z in the whole
//! window), which sends the change that puts it back ([`Undo`]); Escape takes the toast away. Hiding
//! is undone exactly: a conversation is opened again, a sender or domain loses the rule the call made
//! and the entries it closed open again ([`Undo::Hidden`]). A change that takes the entry off the
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
use iced::widget::{Space, column, container, pick_list, row, rule, stack, text_input};
use iced::{Alignment, Element, Fill, Padding, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::connect;
use torro_gui::design::{self, Follow};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::shell::{self, Cover, SidebarMode, Surface, Switcher, TitleButtons};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::ui_state::{self, Density, UiState};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, ToastAction, Trail};
use torro_gui::zoom::{self, Zoom};
use torro_gui::{datetime, style, typo};
use torro_inbox::item::HideScope;
use torro_inbox::source::TaskList;
use torro_inbox::{
    Hidden, InboxError, InboxSource, Item, ItemFilter, Lane, Link, Rule, SourceInfo, SourceKind, SourceRef, State,
    StateFilter, rules,
};
use torro_ipc::mail::{MoveRole, MoveTarget};
use torro_mail::{Flag, Folder, Message as Mail, MessageSummary, Role};

use crate::compose;
use crate::glance::{self, GlanceData};
use crate::hub::{self, Origin};
use crate::keys::{self, Command, Focus, Key};
use crate::list::Look;
use crate::mail::{self, Compose, FolderView, MailAction, MailEdit, MailPage, MailSource, Opened, Reading};
use crate::notifications::{self, Arrange, Board, Shown};
use crate::overview::{self, Column, Sections};
use crate::{detail, list};

/// The app's identity: window class `torro-inbox`, binary `torroinbox`, state in `torroinbox/`.
pub const INBOX: AppId = AppId::INBOX;

/// How often the window asks the source again. Short, because a waiting agent should show up
/// within seconds; the source is local and cheap to ask, and nothing moves on screen unless an
/// entry or a time in the list changed.
const REFRESH: Duration = Duration::from_secs(10);

/// How long the toast after a change shows, and with it "Undo" (`u`, Ctrl + Z): the suite's
/// [`widgets::TOAST_FOR`].
pub const UNDO_FOR: Duration = widgets::TOAST_FOR;

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
    /// "Marked as done", "Snoozed until 18:30", "Moved to Info" … A remembered lane has no undo.
    pub fn after(&self, before: &Item, now: DateTime<Utc>) -> After {
        let text = match self {
            Edit::Done { done: true, .. } => tr("Marked as done").to_owned(),
            Edit::Done { done: false, .. } => tr("Reopened").to_owned(),
            Edit::Dismiss { .. } => tr("Dismissed").to_owned(),
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
    /// Takes hiding back (task E3.11): removes the rule `rule` – only one the hiding made, never one
    /// that was there before (`inbox.remove_rule`) –, then opens the entries `reopen` again
    /// (`inbox.set_item_done` with `false`), those the hiding closed ([`Hidden::done`]).
    Hidden { rule: Option<String>, reopen: Vec<String> },
    /// Takes "Dismiss all" back (task E8.4), and "Mark mail done" (task E8.7b): opens the entries it
    /// closed again (`inbox.set_item_done` with `false`, one by one).
    Dismissed(Vec<String>),
}

impl Undo {
    /// The undo of hiding that answered `hidden`; `None` when it changed nothing (a conversation done
    /// already, a rule there before that closed nothing more).
    pub fn of_hidden(hidden: &Hidden) -> Option<Undo> {
        let rule = hidden.rule.as_ref().filter(|_| hidden.new_rule).map(|rule| rule.id.clone());
        (rule.is_some() || !hidden.done.is_empty()).then(|| Undo::Hidden { rule, reopen: hidden.done.clone() })
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

/// The text of the toast after hiding (tasks E3.10, E3.11): "Conversation hidden" without a rule,
/// else the words of the control centre's rules ([`hidden_text`]).
pub fn hidden_note(hidden: &Hidden) -> String {
    match &hidden.rule {
        Some(rule) => hidden_text(rule),
        None => tr("Conversation hidden").to_owned(),
    }
}

/// The text of the hint after hiding with `rule` (task E3.10), in the words of the control
/// centre's rules: "Hidden: news@shop.example", "Hidden: everything from shop.example".
pub fn hidden_text(rule: &Rule) -> String {
    match (&rule.sender, &rule.domain) {
        (Some(sender), _) => trf("Hidden: {sender}", &[("sender", sender)]),
        (None, Some(domain)) => trf("Hidden: everything from {domain}", &[("domain", domain)]),
        (None, None) => trf("New entries from {source} muted", &[("source", &rule.source.label())]),
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
    /// `inbox.hide_sender` for the entry with this id (task E3.10).
    Hide(String, HideScope),
    /// The undo of hiding (task E3.11): the rule removed, if any, and the entries opened again.
    Unhide(Option<String>, Vec<String>),
    Link(String),
    /// `inbox.open_item` for the entry with this id.
    Open(String),
    /// `approvals.decide_approval`: the approval, allowed or not.
    Decide(String, bool),
    /// The task lists were asked for (the dialog of "Make task" opened).
    Lists,
    /// `inbox.create_task_from_item`: entry, list, title (`None`: the entry's), due day.
    Task {
        id: String,
        list: TaskList,
        title: Option<String>,
        due: Option<NaiveDate>,
    },
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
    /// `tasks.create_todo_from_mail` (task E8.7b): account, message, list, title (`None`: the
    /// subject).
    MailTask {
        account: String,
        id: String,
        list: TaskList,
        title: Option<String>,
    },
    /// `apps.open_app` with a target (task E8.7b): the app and `task:<uid>` or `mail:<…>`.
    Target(String, String),
    /// "Mark mail done" (task E8.7b): `inbox.set_item_done` for these entries – the mail's, then the
    /// question's.
    MailDone(Vec<String>),
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
            View::Folder(_) | View::Mail | View::Attachments => return None,
        };
        Some(ItemFilter { lane, source, state: Some(state), ..ItemFilter::default() })
    }

    /// The name in the sidebar, the header and the panel's path.
    pub fn name(&self) -> String {
        match self {
            View::Overview => tr("Overview").into(),
            View::ForMe => tr("For me").into(),
            View::Info => tr("Info").into(),
            View::Snoozed => tr("Snoozed").into(),
            View::Done => tr("Done").into(),
            View::Source(source) => source.label(),
            View::Folder(folder) => folder.name.clone(),
            View::Mail => tr("Mail").into(),
            View::Attachments => tr("With attachments").into(),
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            View::Overview => Icon::Layers,
            View::ForMe => Icon::Inbox,
            View::Info => Icon::Bell,
            View::Snoozed => Icon::Clock,
            View::Done => Icon::CircleCheck,
            View::Source(source) => list::source_icon(source),
            View::Folder(folder) => mail::folder_icon(folder.role),
            View::Mail => Icon::Mail,
            View::Attachments => Icon::Paperclip,
        }
    }

    /// The folder this view shows, if it is one.
    pub fn folder(&self) -> Option<&FolderView> {
        match self {
            View::Folder(folder) => Some(folder),
            _ => None,
        }
    }

    /// A view of mail: a folder, the smart view of attachments, or a window of mail waiting for its
    /// folders.
    pub fn is_mail(&self) -> bool {
        matches!(self, View::Folder(_) | View::Mail | View::Attachments)
    }

    /// What the messages held belong to, for a view that lists messages: the folder's id, or
    /// "attachments" for the smart view. Another view's messages are not shown as this one's.
    pub fn listing(&self) -> Option<String> {
        match self {
            View::Folder(folder) => Some(folder.id.clone()),
            View::Attachments => Some("attachments".to_owned()),
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
    /// "Hide from inbox" for `App::subject` (task E3.10): the ways to hide it, by `m` or the
    /// panel's "…" ([`App::hide_choices`]).
    Hide,
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
    /// The first account's inbox as a folder (task E4.2).
    Folder,
    /// That inbox with its newest message in the panel.
    Mail,
    /// "For me" with its first mail entry in the panel, the whole mail loaded.
    MailEntry,
    /// The menu "Hide from inbox" of the first mail entry of "For me", at its row (task E3.10).
    Hide,
    /// That entry in the panel, with the menu at the panel's "…".
    HidePanel,
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
    /// The notifications window arranged "By time" (task E8.4), where it has room for columns.
    ByTime,
    /// The hub's To sort alone (task E8.5), as its sidebar entry shows it.
    ToSort,
    /// The hub's "Act now" filtered to its first tile (task E8.5).
    Tile,
}

impl Open {
    const ALL: [(&'static str, Open); 30] = [
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
        ("folder", Open::Folder),
        ("mail", Open::Mail),
        ("mail-entry", Open::MailEntry),
        ("hide", Open::Hide),
        ("hide-panel", Open::HidePanel),
        ("overview", Open::Overview),
        ("overview-info", Open::OverviewInfo),
        ("overview-notices", Open::OverviewNotices),
        ("attachments", Open::Attachments),
        ("by-time", Open::ByTime),
        ("to-sort", Open::ToSort),
        ("tile", Open::Tile),
    ];
}

/// The start parameters of the window:
///
/// - `--demo`: sample data ([`torro_inbox::demo::DemoSource`]) instead of the core.
/// - `--open <state>`: start in this [`Open`] state; needs `--demo`.
/// - `--appearance light|dark|system`: use this appearance in this process instead of the stored
///   one. Nothing is stored.
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

    /// The icon of `view` in this window: "For me" of the mail window has the inbox's, "New" of the
    /// notifications window the bell.
    pub fn view_icon(self, view: &View) -> Icon {
        match (self, view) {
            (Kind::Mail, View::Overview) => Icon::Inbox,
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
            Kind::Notifications => trf("Notifications · {view}", &pairs),
            Kind::Hub => trf("Inbox · {view}", &pairs),
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
        Ok(Launch { demo, open, appearance, now, design, kind, density, view, shown, compose, message: None })
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
    /// The scroll offset and visible height of the overview's columns, by [`Column::index`].
    column_scroll: [Option<(f32, f32)>; 2],
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
    /// The approval whose decision is on its way (task E3.8): its Allow and Deny are disabled
    /// until the core answered, so a second click cannot decide it twice.
    deciding: Option<String>,
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
    /// The folder whose messages `messages` holds; another folder's view shows loading until its
    /// messages are in.
    listed: Option<String>,
    /// What is typed into the folder's search field.
    search: String,
    /// The accounts folded away or unfolded by the person (`true`: unfolded); the others follow
    /// [`App::account_open`].
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
    /// The tile of "Act now" clicked (task E8.5): its origin's entries alone show. Window state.
    filter: Option<Origin>,
    /// The hub shows To sort alone (its sidebar entry "To sort"), on the entries of its overview.
    sorting: bool,
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
    /// The density chosen for this window (task E8.2): `--density`, else the one remembered in
    /// `ui.json`, else `None` – then the layout decides ([`App::density`]).
    density: Option<Density>,
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
    /// From the panel's "…": the menu "Hide from inbox" for the entry with this id (task E3.10).
    AskHide(String),
    /// From the hide menu: hide the sender (or its domain) of the mail entry with this id.
    HideSender(String, HideScope),
    /// The source's answer to hiding: the rule as stored, or why not.
    Hidden(Result<Hidden, String>),
    /// An undo that is no edit (hiding, task E3.11) is through, or failed.
    Undone(Result<(), String>, Option<String>),
    /// The parts unfolded in `ui.json` (task E3.11): which "Info" of the overview is open.
    Unfolded(Vec<String>),
    /// From the hide menu: move the mail of the entry with this id to its account's junk folder.
    Junk(String),
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
    /// From a folder's list: the message with this id under the cursor and in the panel.
    SelectMail(String),
    /// The whole mail for the panel, or why it could not be read.
    LetterLoaded(String, Result<Box<Mail>, String>),
    /// From the panel or a key: do this with the message with this id.
    Mail(MailAction, String),
    /// The source's answer to a [`MailEdit`], with the hint to show (`None`: none, or an undo).
    MailEdited(Result<(), String>, Option<Hint>),
    /// "Load more" at the end of a folder's messages.
    MoreMail,
    /// The next page of the folder's view it was asked for.
    MoreLoaded(View, Result<MailPage, String>),
    /// What is typed into the search field.
    SearchInput(String),
    /// `Enter` in the search field: show the folder searched for it.
    Search,
    /// "Clear search": the whole folder again.
    ClearSearch,
    /// An attachment of a message (message id, part) to fetch and open.
    OpenAttachment(String, String),
    /// What became of opening an attachment.
    AttachmentOpened(Result<Opened, String>),
    /// A web address in a mail's text was clicked.
    OpenMailLink(String),
    /// The title of an account's group: fold its folders away or show them.
    ToggleAccount(String),
    /// The compose window could not be started.
    ComposeStarted(Result<(), String>),
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
/// every inbox and keeps those with attachments (`mail::attachments`).
///
/// And only those of the sources `shown` (task E8.4, the notifications window's choice): with one
/// source the requests ask for it alone (the API's `source`), with more the entries of the others
/// are left out here.
///
/// The hub's overview (task E8.5) also asks for the snoozed entries, for "Coming back", and the done
/// ones, for "Handed off" (task E8.7b).
///
/// `demo` is the demo's "now" with `--demo` (else `None`): the entries get the demo's tasks
/// ([`demo_task`]) before they are sorted into the window, as the core's would come.
pub async fn fetch<S: InboxSource + MailSource>(
    inbox: Arc<S>,
    view: View,
    empty: bool,
    limit: u32,
    (kind, shown): (Kind, Shown),
    demo: Option<DateTime<Utc>>,
) -> Result<Loaded, String> {
    if empty {
        let none = Loaded {
            items: Vec::new(),
            for_me: Vec::new(),
            info: 0,
            sources: Vec::new(),
            folders: Vec::new(),
            mail: view.listing().map(|_| MailPage { messages: Vec::new(), next: None }),
            snoozed: Vec::new(),
            done: Vec::new(),
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
        Some(filter) => keep(inbox.list_items(narrow(filter)).await.map_err(failed)?),
        None => Vec::new(),
    };
    // The overview lists both lanes: its entries hold "For me" and "Info" already.
    let for_me = match view {
        View::ForMe => items.clone(),
        View::Overview => items.iter().filter(|item| item.lane == Lane::ForMe).cloned().collect(),
        _ => keep(inbox.list_items(for_me_filter()).await.map_err(failed)?),
    };
    let info = match view {
        View::Info => items.len(),
        View::Overview => items.iter().filter(|item| item.lane == Lane::Info).count(),
        _ => keep(inbox.list_items(narrow(View::Info.filter().unwrap_or_default())).await.map_err(failed)?).len(),
    };
    let sources = inbox.list_sources().await.map_err(failed)?;
    let folders = inbox.mail_folders().await.unwrap_or_default();
    let mail = match &view {
        View::Folder(folder) => {
            Some(inbox.search_mail(folder.id.clone(), folder.query.clone(), limit, None).await.map_err(failed)?)
        }
        View::Attachments => {
            let mut pages = Vec::new();
            for inbox_folder in folders.iter().filter(|folder| folder.role == Some(Role::Inbox)) {
                let page = inbox.search_mail(inbox_folder.id.clone(), None, limit, None).await.map_err(failed)?;
                pages.push(page.messages);
            }
            Some(MailPage { messages: mail::attachments(pages), next: None })
        }
        _ => None,
    };
    // "Coming back" (task E8.5) and "Handed off" (task E8.7b) beside the hub's "Act now".
    let (snoozed, done) = match (kind, &view) {
        (Kind::Hub, View::Overview) => (
            keep(inbox.list_items(View::Snoozed.filter().unwrap_or_default()).await.map_err(failed)?),
            keep(inbox.list_items(View::Done.filter().unwrap_or_default()).await.map_err(failed)?),
        ),
        _ => (Vec::new(), Vec::new()),
    };
    Ok(Loaded { items, for_me, info, sources, folders, mail, snoozed, done })
}

impl<S: InboxSource + MailSource + 'static> App<S> {
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
                Some(Open::Folder | Open::Mail) => View::Mail,
                // The mail window's "For me" is its overview (task E8.3).
                Some(Open::MailEntry) if launch.kind == Kind::Mail => View::Overview,
                Some(Open::MailEntry) => View::ForMe,
                Some(Open::Attachments) => View::Attachments,
                Some(Open::Overview | Open::OverviewInfo | Open::OverviewNotices | Open::ToSort | Open::Tile) => {
                    View::Overview
                }
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
            column_scroll: [None; 2],
            subject: None,
            pick: Pick::start(Utc::now() + offset),
            task: TaskDraft::default(),
            lists: None,
            last_list: None,
            hint: None,
            deciding: None,
            folders: Vec::new(),
            messages: Vec::new(),
            more: None,
            reading: false,
            letter: None,
            listed: None,
            search: String::new(),
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
            filter: None,
            sorting: open == Some(Open::ToSort),
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
            density: launch.density,
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
        let task = Task::batch([
            ui_state::load_task(INBOX).map(Message::UiState),
            ui_state::load_unfolded_task(INBOX).map(Message::Unfolded),
            density,
            app.load(),
            app.fetch_appearance(),
            app.zoom.fetch().map(Message::Zoom),
            app.design.fetch().map(Message::Design),
            iced::system::theme().map(Message::SystemMode),
            size,
        ]);
        (app, task)
    }

    /// The window title: the window's kind and its view ("Mail · Overview", [`Kind::title`]); the
    /// hub's To sort alone is "Inbox · To sort".
    pub fn title(&self) -> String {
        if self.is_hub() && self.sorting {
            return trf("Inbox · {view}", &[("view", &self.view_label())]);
        }
        self.launch.kind.title(&self.view)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(REFRESH).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Zoom keys work everywhere, the glance included.
            self.zoom.subscription().map(Message::Zoom),
            // The design pack polls the core every 2 s itself and wakes the window only for a change.
            self.design.subscription().map(Message::Design),
        ];
        // Mini and strip are displays: no keys, and nothing open that Escape could close. (A key
        // like `e` must not mark an entry done that nobody sees.)
        if !self.is_glance() {
            subscriptions.push(iced::event::listen_with(keys::listen));
            // Escape closes an open menu, drawer, the toast or the panel; nothing listens while none is
            // open.
            if self.menu.is_some() || self.drawer || self.panel_open() || self.hint.is_some() {
                subscriptions.push(widgets::escape().map(|()| Message::Escape));
            }
            // Ctrl + Z (⌘ + Z) undoes while the toast offers it – not while the dialog of "Make task"
            // or the picker has the keys.
            if self.can_undo() && !matches!(self.menu, Some(Menu::Task | Menu::Pick)) {
                subscriptions.push(widgets::undo_key().map(|()| Message::Undo));
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
        let task = self.handle(message);
        // Whatever changed, the panel's mail follows the cursor and the entry in the panel.
        let letter = self.follow_letter();
        Task::batch([task, letter])
    }

    /// What `message` does; [`App::update`] lets the panel's mail follow afterwards.
    fn handle(&mut self, message: Message) -> Task<Message> {
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
                self.reading = false;
                self.cursor = None;
                self.pinned_mail = None;
                // "Act now" again, all of it (task E8.5).
                self.sorting = false;
                self.filter = None;
                // A folder from the sidebar comes whole; its search field starts empty.
                self.search = view.folder().and_then(|folder| folder.query.clone()).unwrap_or_default();
                if view != self.view {
                    self.view = view;
                    return self.load();
                }
            }
            Message::Select(id) => {
                self.close_menus();
                // "Coming back" opens snoozed entries, which the overview does not list (task E8.5).
                self.detail = self.items.iter().chain(&self.snoozed).find(|item| item.id == id).cloned();
                if let Some(item) = &self.detail {
                    self.column = self.launch.kind.column_or_first(Column::of(item));
                    self.cursor = Some(id);
                }
            }
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
                    return self.run(command);
                }
            }
            Message::Scrolled(offset, height) => self.scroll = Some((offset, height)),
            Message::AskSnooze(id) => self.ask_snooze(&id),
            Message::AskLane(id) => return self.ask_lane(&id),
            Message::AskMute(id) => return self.ask_mute(&id),
            Message::AskHide(id) => self.ask_hide(&id),
            Message::HideSender(id, scope) => return self.hide_sender(id, scope),
            Message::Hidden(Ok(hidden)) => {
                self.hint =
                    Some(Hint { text: hidden_note(&hidden), undo: Undo::of_hidden(&hidden), shown: Instant::now() });
                return self.load();
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
            }
            Message::Hidden(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't hide the sender."), err));
            }
            Message::Junk(id) => return self.move_to_junk(&id),
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
            Message::Decide { approval, allow } => return self.decide(approval, allow),
            Message::Decided(result, allow) => return self.decided(result, allow),
            Message::Refresh => return self.load(),
            Message::Tick => {
                self.now = Utc::now() + self.offset;
                return Task::batch([self.fetch_appearance(), self.zoom.fetch().map(Message::Zoom), self.load()]);
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
            // The search is a folder's (task E4.2): the button focuses its field there. "New" writes a
            // message when there is a mail account to write from (task E4.3); the inbox has nothing
            // else to create. Elsewhere the buttons only close what is open.
            Message::Switcher(Switcher::Search) if self.view.folder().is_some() => return self.focus_search(),
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
            Message::CloseMenu => self.close_menus(),
            Message::Escape => {
                if self.menu.is_some() || self.drawer {
                    self.close_menus();
                } else if self.hint.is_some() {
                    // The toast goes first: it lies over everything, and the panel stays.
                    self.hint = None;
                } else {
                    self.close_panel();
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
            Message::SelectMail(id) => {
                self.close_menus();
                if self.messages.iter().any(|message| message.id == id) {
                    self.cursor = Some(id);
                    self.reading = true;
                }
            }
            Message::LetterLoaded(id, result) => {
                if let Some(letter) = self.letter.as_mut().filter(|letter| letter.id == id) {
                    letter.loaded = Some(result.map(|mail| *mail));
                }
            }
            Message::Mail(action, id) => return self.mail_action(action, id),
            Message::MailEdited(Ok(()), hint) => {
                if let Some(hint) = hint {
                    self.hint = Some(Hint { shown: Instant::now(), ..hint });
                }
                return self.load();
            }
            Message::MailEdited(Err(err), _) => {
                self.error = Some(Failure::new(tr("Couldn't change the mail."), err));
                return self.load();
            }
            Message::MoreMail => return self.more_mail(),
            Message::MoreLoaded(view, result) => {
                if view != self.view {
                    return Task::none();
                }
                match result {
                    Ok(page) => {
                        let known: Vec<String> = self.messages.iter().map(|message| message.id.clone()).collect();
                        self.messages.extend(page.messages.into_iter().filter(|message| !known.contains(&message.id)));
                        self.more = page.next;
                    }
                    Err(err) => self.error = Some(Failure::new(tr("Couldn't load more messages."), err)),
                }
            }
            Message::SearchInput(text) => self.search = text,
            Message::Search => return self.search_folder(self.search.clone()),
            Message::ClearSearch => {
                self.search.clear();
                return self.search_folder(String::new());
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
            Message::ToggleAccount(account) => {
                let first = mail::by_account(&self.folders).first().is_some_and(|group| group.account == account);
                let open = !self.account_open(&account, first);
                self.folds.retain(|(folded, _)| *folded != account);
                self.folds.push((account, open));
            }
            Message::ComposeStarted(Ok(())) => {}
            Message::ComposeStarted(Err(err)) => {
                self.error = Some(Failure::new(tr("Couldn't open the compose window."), err));
            }
            Message::ToggleSource(source) => return self.toggle_source(&source),
            Message::Arrange(arrange) => self.arrange = arrange,
            Message::DismissAll => return self.dismiss_all(),
            Message::DismissedAll(done, failed) => return self.dismissed_all(done, failed),
            Message::Filter(origin) => self.toggle_filter(origin),
            Message::ShowToSort => return self.show_to_sort(),
            Message::AskMailTask(id) => return self.ask_mail_task(&id),
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
        }
        Task::none()
    }

    /// Whether the content is the hub's "Act now" or its To sort alone (task E8.5, `hub.rs`).
    fn is_hub(&self) -> bool {
        self.launch.kind == Kind::Hub && self.view == View::Overview
    }

    /// The name of what the window shows, in the header, the panel's path and the title: To sort
    /// while the hub shows it alone, else the view's name in this window ([`Kind::view_name`]).
    fn view_label(&self) -> String {
        if self.is_hub() && self.sorting { tr("To sort").to_owned() } else { self.launch.kind.view_name(&self.view) }
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
        hub::layout(width.max(0.0), class, self.density(), self.sorting)
    }

    /// Whether "Coming back" stands beside "Act now": in the wide layout, while no panel takes its
    /// place.
    fn hub_side(&self) -> bool {
        self.is_hub() && !self.sorting && self.class() == SizeClass::Wide && !self.panel_open()
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
    /// compose window; Archive and Delete move it and offer to move it back; Flag and read change
    /// its flags.
    fn mail_action(&mut self, action: MailAction, id: String) -> Task<Message> {
        self.close_menus();
        if let Some(kind) = Compose::of(action) {
            return self.compose(kind, &id);
        }
        let Some(edit) = mail::edit_of(action, &id) else { return Task::none() };
        let delete = action == MailAction::Delete;
        if !matches!(action, MailAction::Archive | MailAction::Delete) {
            return self.mail_edit(Some(edit), None);
        }
        if delete && self.in_trash(&id) {
            self.note(tr("This message is in the trash already.").to_owned());
            return Task::none();
        }
        let back = MailEdit::Move { ids: vec![id.clone()], to: self.home_of(&id) };
        let reopen = self.entry_of(&id);
        let hint = Hint {
            text: mail::moved_text(delete).to_owned(),
            undo: Some(Undo::Mail { edit: back, reopen }),
            shown: Instant::now(),
        };
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
        Task::perform(edit.apply(inbox), move |result| Message::MailEdited(result, hint.clone()))
    }

    /// Takes the message `id` off a folder's list at once (archived, deleted): the cursor – and
    /// the panel – move to the row that took its place, so `e`, `e`, `e` works through a folder.
    fn take_out(&mut self, id: &str) {
        let Some(place) = self.messages.iter().position(|message| message.id == id) else { return };
        self.messages.remove(place);
        if self.cursor.as_deref() == Some(id) {
            let last = self.messages.len().checked_sub(1);
            self.cursor = last.map(|last| self.messages[place.min(last)].id.clone());
            if self.cursor.is_none() {
                self.reading = false;
            }
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
            return self.messages.iter().any(|message| message.id == *id).then(|| id.clone());
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

    /// Shows the folder searched for `query` (the whole folder for a blank one).
    fn search_folder(&mut self, query: String) -> Task<Message> {
        let Some(folder) = self.view.folder() else { return Task::none() };
        let searched = folder.searching(&query);
        if searched == *folder {
            return Task::none();
        }
        self.view = View::Folder(searched);
        self.cursor = None;
        self.reading = false;
        self.load()
    }

    /// Puts the keyboard into the search field of a folder.
    fn focus_search(&mut self) -> Task<Message> {
        self.close_menus();
        if self.view.folder().is_none() {
            return Task::none();
        }
        operation::focus(mail::SEARCH)
    }

    /// "Load more": the folder's next page.
    fn more_mail(&mut self) -> Task<Message> {
        let (Some(folder), Some(cursor)) = (self.view.folder().cloned(), self.more.clone()) else {
            return Task::none();
        };
        #[cfg(test)]
        self.sent.push(Sent::More(cursor.clone()));
        let (inbox, view) = (self.inbox.clone(), self.view.clone());
        let next = async move {
            inbox.search_mail(folder.id, folder.query, mail::PAGE, Some(cursor)).await.map_err(|err| err.to_string())
        };
        Task::perform(next, move |result| Message::MoreLoaded(view.clone(), result))
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
        self.items.sort_by_key(|item| !list::is_waiting(item));
        // The hub's "Done today" (task E8.5): what was done today, local time.
        if self.launch.kind == Kind::Hub && self.view == View::Done {
            let today = local_today(self.now);
            self.items.retain(|item| matches!(item.state, State::Done { at } if local_today(at) == today));
        }
        self.snoozed = loaded.snoozed;
        self.for_me = loaded.for_me;
        self.for_me.sort_by_key(|item| !list::is_waiting(item));
        self.info = loaded.info;
        self.sources = loaded.sources;
        self.folders = loaded.folders;
        let page = loaded.mail.unwrap_or(MailPage { messages: Vec::new(), next: None });
        self.messages = page.messages;
        self.more = page.next;
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
            return notifications::order(&self.items, self.board(), &self.view);
        }
        // The hub walks its sections (task E8.5): Decide, Agents, To sort – or To sort alone.
        if self.is_hub() {
            return hub::Sections::of(&self.items, self.filter.as_ref()).order(self.sorting);
        }
        if self.view == View::Overview {
            Sections::of(&self.items, self.column, self.now).order(self.info_open[self.column.index()])
        } else {
            self.items.iter().collect()
        }
    }

    /// A button of a row's action bar (task E3.9): the cursor – and an open panel – to the entry
    /// `id`, then `command` as its key would do it, so a menu opens at the row.
    fn act(&mut self, id: &str, command: Command) -> Task<Message> {
        let Some(item) = self.items.iter().find(|item| item.id == id).cloned() else { return Task::none() };
        self.close_menus();
        self.column = self.launch.kind.column_or_first(Column::of(&item));
        self.cursor = Some(item.id.clone());
        if self.detail.is_some() {
            self.detail = Some(item);
        }
        self.run(command)
    }

    /// Folds or unfolds "Info" at the end of `column` and remembers it in `ui.json` (task E3.11). A
    /// cursor on a row folded away leaves it.
    fn toggle_info(&mut self, column: Column) -> Task<Message> {
        let open = &mut self.info_open[column.index()];
        *open = !*open;
        if !*open && self.cursor_item().is_some_and(|item| Column::of(item) == column && item.lane == Lane::Info) {
            self.cursor = None;
        }
        ui_state::save_unfolded_task(INBOX, self.unfolded())
    }

    /// The names of the parts unfolded, as `ui.json` keeps them ([`Column::info_key`]).
    pub fn unfolded(&self) -> Vec<String> {
        Column::ALL
            .into_iter()
            .filter(|column| self.info_open[column.index()])
            .map(|c| c.info_key().to_owned())
            .collect()
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

    /// The entry with `id`: in the list, or the one in the panel or an open menu.
    fn find(&self, id: &str) -> Option<&Item> {
        self.items.iter().chain(self.detail.as_ref()).chain(self.subject.as_ref()).find(|item| item.id == id)
    }

    /// Where the keys go: a menu of choices, the picker, another menu or the drawer, else the list.
    fn focus(&self) -> Focus {
        match self.menu {
            Some(Menu::Snooze | Menu::Lane | Menu::Hide) => Focus::Choices,
            Some(Menu::Pick) => Focus::Picker,
            Some(Menu::Task) => Focus::Task,
            Some(Menu::Nav | Menu::Switcher) => Focus::Elsewhere,
            None if self.drawer => Focus::Elsewhere,
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
            Command::Search => self.focus_search(),
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
            Command::Snooze => {
                if let Some(id) = cursor {
                    self.ask_snooze(&id);
                }
                Task::none()
            }
            Command::Lane => cursor.map_or_else(Task::none, |id| self.ask_lane(&id)),
            // In the notifications window `m` is the bell (task E8.4): it mutes the source at once
            // (the bell's menu comes with E8.6b); "This entry" of the hide menu is the ✕ there.
            Command::Hide if self.launch.kind == Kind::Notifications => {
                cursor.map_or_else(Task::none, |id| self.ask_mute(&id))
            }
            Command::Hide => {
                if let Some(id) = cursor {
                    self.ask_hide(&id);
                }
                Task::none()
            }
            // In a folder the message under the cursor becomes a task (task E8.7b).
            Command::MakeTask if self.view.is_mail() => {
                self.cursor.clone().map_or_else(Task::none, |id| self.ask_mail_task(&id))
            }
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
                .and_then(|item| notifications::span(&self.items, board, &self.view, density, &item.id));
            (self.scroll, span, list::SCROLL)
        } else if overview {
            // The overview: the column with the cursor, which scrolls on its own.
            let column = self.column;
            let sections = Sections::of(&self.items, column, self.now);
            let open = self.info_open[column.index()];
            let (look, lead) = (self.look(), self.lead().is_some());
            let span = sections
                .order(open)
                .get(index)
                .and_then(|item| overview::row_span(&sections, open, &item.id, look, lead));
            (self.column_scroll[column.index()], span, column.scroll_id())
        } else if self.view.is_mail() {
            // A folder's messages have no group heads: every row is as high as the next; a card is
            // as high as what it shows.
            let span = self.messages.get(index).map(|message| match self.look() {
                Look::Cards { words } => {
                    let above = self.messages[..index].iter().map(|message| mail::message_card_height(message, words));
                    list::card_span(above, mail::message_card_height(message, words))
                }
                Look::Rows => {
                    let row = list::message_height();
                    let top = index as f32 * row;
                    (top, top + row)
                }
            });
            (self.scroll, span, list::SCROLL)
        } else {
            (self.scroll, list::row_span(&self.items, index, self.look()), list::SCROLL)
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

    /// Opens the menu "Hide from inbox" for the entry with `id` (task E3.10). An entry with no way
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

    /// The ways to hide `item`, in the order of the menu (task E3.10):
    ///
    /// - mail: "This conversation" (`inbox.hide_sender` with `conversation`: done, the mail stays
    ///   unread; a new reply brings it back), "Everything from <address>" and "Everything from
    ///   <domain>" (`inbox.hide_sender`, in every mail account), then after a line "Always to Info"
    ///   (`inbox.set_item_lane` with `remember`) and "Move to junk" (`mail.move_messages`, only
    ///   when the account has a folder with the role junk). Without a sender's address only the
    ///   first and the junk folder;
    /// - other sources: "This entry" (done) and "Mute <source>" / "Unmute <source>"; the suite is
    ///   never muted ([`rules_allowed`]), an approval has no done.
    ///
    /// Done entries have no "This …": they are out of the inbox already.
    pub fn hide_choices(&self, item: &Item) -> Vec<Choice> {
        let id = || item.id.clone();
        let mut choices = Vec::new();
        let open = !matches!(item.state, State::Done { .. }) && !detail::is_approval(item);
        if item.source.kind == SourceKind::Mail {
            // Hidden, not done: the mail stays unread (task E3.11).
            if open {
                let conversation = Message::HideSender(id(), HideScope::Conversation);
                choices.push(Choice::new(Icon::Check, tr("This conversation"), conversation));
            }
            let hide = |scope| rules::hide_target(item, scope);
            if let Some(sender) = hide(HideScope::Sender) {
                let label = trf("Everything from {sender}", &[("sender", &sender)]);
                choices.push(Choice::new(Icon::User, label, Message::HideSender(id(), HideScope::Sender)));
            }
            if let Some(domain) = hide(HideScope::Domain) {
                let label = trf("Everything from {domain}", &[("domain", &domain)]);
                choices.push(Choice::new(Icon::Users, label, Message::HideSender(id(), HideScope::Domain)));
            }
            let mut rest = Vec::new();
            if rules::mail_sender(item).is_some() {
                let always = Edit::Lane { id: id(), lane: Lane::Info, remember: true };
                rest.push(Choice::new(lane_icon(Lane::Info), tr("Always to Info"), Message::Edit(always)));
            }
            if matches!(item.link, Some(Link::Mail { .. })) && self.junk_folder(&item.source).is_some() {
                rest.push(Choice::new(Icon::ShieldAlert, tr("Move to junk"), Message::Junk(id())));
            }
            if let Some(first) = rest.first_mut() {
                first.line = !choices.is_empty();
            }
            choices.extend(rest);
            return choices;
        }
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
            choices.push(Choice::new(Icon::Moon, mute, Message::AskMute(id())));
        }
        choices
    }

    /// The junk folder of the mail account `source`, if it has one.
    fn junk_folder(&self, source: &SourceRef) -> Option<&torro_mail::Folder> {
        let account = source.key();
        self.folders.iter().find(|folder| folder.account == account && folder.role == Some(Role::Junk))
    }

    /// Sends `inbox.hide_sender` for the entry `id` and closes the menu; the answer comes as
    /// [`Message::Hidden`], whose toast names what was hidden and offers to take it back exactly
    /// ([`Undo::of_hidden`], task E3.11).
    fn hide_sender(&mut self, id: String, scope: HideScope) -> Task<Message> {
        self.close_menus();
        #[cfg(test)]
        self.sent.push(Sent::Hide(id.clone(), scope));
        let inbox = self.inbox.clone();
        Task::perform(async move { inbox.hide_sender(id, scope).await.map_err(|err| err.to_string()) }, Message::Hidden)
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
        self.task = TaskDraft { title: list::one_line(&item.title), list: None, due: Due::None };
        self.subject = Some(item);
        self.task_mail = None;
        self.open_task_dialog()
    }

    /// Opens the dialog that makes a task of the folder's message `id` (task E8.7b): its subject as
    /// title, the lists asked for anew, no due day ([`MailSource::create_todo_from_mail`] takes none).
    /// A message with an entry goes through the entry, as `t` on it in "For me" does.
    fn ask_mail_task(&mut self, id: &str) -> Task<Message> {
        if let Some(entry) = self.entry_of(id) {
            return self.ask_task(&entry);
        }
        let loaded = || match &self.letter {
            Some(Letter { loaded: Some(Ok(mail)), .. }) if mail.summary.id == id => Some(mail.summary.clone()),
            _ => None,
        };
        let Some(summary) = self.messages.iter().find(|message| message.id == id).cloned().or_else(loaded) else {
            return Task::none();
        };
        self.close_menus();
        // The dialog opens at the cursor's row, as for entries.
        self.cursor = Some(summary.id.clone());
        self.task = TaskDraft { title: mail::subject(&summary), list: None, due: Due::None };
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

    /// "Create task" for a folder's message (task E8.7b): sends `tasks.create_todo_from_mail` with
    /// the dialog's list and title (none when blank: the core takes the subject) and closes the
    /// dialog. The mail stays as it is; the reader shows the task's card once the answer is in.
    fn create_mail_task(&mut self, message: MessageSummary) -> Task<Message> {
        let Some(list) = self.task.list.clone() else { return Task::none() };
        let title = Some(self.task.title.trim().to_owned()).filter(|title| !title.is_empty());
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
        });
        let inbox = self.inbox.clone();
        let asked = id.clone();
        Task::perform(
            async move { inbox.create_todo_from_mail(account, asked, list, title).await.map_err(|err| err.to_string()) },
            move |result| Message::MailTaskCreated(id.clone(), result),
        )
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
            self.note(tr("The mail is no longer in the inbox.").to_owned());
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
        self.letter = Some(Letter { id, loaded: Some(Ok(mail)) });
        self.load()
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
                Task::perform(back, |result| Message::MailEdited(result, None))
            }
            Undo::Hidden { rule, reopen } => {
                #[cfg(test)]
                self.sent.push(Sent::Unhide(rule.clone(), reopen.clone()));
                // First the rule, then the entries: the other way round nothing would stop mail of the
                // sender, and the entries would be open with the rule still there.
                let back = reopen.first().cloned();
                let inbox = self.inbox.clone();
                let undone = async move {
                    if let Some(id) = rule {
                        inbox.remove_rule(id).await.map_err(|err| err.to_string())?;
                    }
                    for id in reopen {
                        inbox.set_item_done(id, false).await.map_err(|err| err.to_string())?;
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
            Some(Menu::Hide) => self.hide_choices(item),
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

    /// The density the window shows its entries in (task E8.2): the one chosen (`--density`, the
    /// switch, `ui.json`), else the layout's ([`Density::default_for`]: compact when narrow).
    /// Mini and strip are glances and ignore it.
    pub fn density(&self) -> Density {
        self.density.unwrap_or_else(|| Density::default_for(self.class()))
    }

    /// How the lists draw mail (task E8.3): cards in the comfortable density of the mail window –
    /// with their buttons' words in the narrow layout, where a card has the window's width –, rows
    /// otherwise. The notifications window draws its own cards (task E8.4, `notifications.rs`); the
    /// hub keeps its rows until E8.5.
    pub fn look(&self) -> Look {
        if self.launch.kind != Kind::Mail || self.density() != Density::Comfortable {
            return Look::Rows;
        }
        Look::Cards { words: self.class() == SizeClass::Narrow }
    }

    /// What the narrow mail window's comfortable "For me" leads with (task E8.3, "N new for you"):
    /// the number of mails for the user and their newest senders ("From Anna, Ben, Chris"). `None`
    /// elsewhere, and while nothing is for the user (the calm line says so).
    fn lead(&self) -> Option<(usize, String)> {
        if self.look() != (Look::Cards { words: true }) || self.view != View::Overview {
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
                Some(mail::ReaderEntry { item, hide: !self.hide_choices(item).is_empty() })
            }
        };
        Some(mail::reader(reading, entry, anchors.clone(), self.density() == Density::Comfortable))
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
        // Always two layers, so the window keeps its state when the hint comes and goes.
        let window = stack![self.window(class), self.hint_layer()].width(Fill).height(Fill);
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
        Cover::panel(class, self.panel_open()).or(Cover::drawer(class, self.drawer))
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
        let anchors = detail::Anchors::default();
        // Menus for an entry open at the panel's buttons while it is open (the row may be under
        // it), else at the cursor's row.
        let (snooze_at, lane_at, task_at, hide_at) = if self.detail.is_some() {
            (&anchors.snooze, &anchors.lane, &anchors.task, &anchors.hide)
        } else {
            (&anchors.row, &anchors.row, &anchors.row, &anchors.row)
        };
        let below = Placement::Below(Align::End);
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
            Some(Menu::Hide) => {
                Some(widgets::floating(hide_at, below, self.choice_menu(tr("Hide from inbox")), Message::CloseMenu))
            }
        };
        // The wide mail window reads beside the list (task E8.3), without the panel.
        if let Some(reader) = self.wide_reader(class, &anchors) {
            let list = container(self.body(class, &anchors.row)).width(size::list()).height(Fill);
            let body = row![list, rule::vertical(size::HAIRLINE).style(style::rule), reader];
            let placed = panel::place(class, body, None, layer);
            return shell::shell(class, sidebar, header, placed.body, None)
                .rail(rail)
                .drawer(self.drawer, Message::CloseMenu)
                .overlay(placed.overlay)
                .into();
        }
        let detail = if self.view.is_mail() {
            // A folder's message: the mail alone, no entry around it.
            self.reading().filter(|_| self.reading).map(|reading| mail::panel(&reading, self.view.folder()))
        } else {
            self.detail.as_ref().map(|item| {
                let footer = detail::Footer {
                    anchors: anchors.clone(),
                    hide: !self.hide_choices(item).is_empty(),
                    deciding: self.deciding.is_some(),
                    // The panel fills narrow windows: no room for the words of "Move to …".
                    compact: !matches!(class, SizeClass::Wide | SizeClass::Medium),
                };
                // A mail entry shows its whole mail in place of the entry's short text.
                let letter = self.reading().map(|reading| mail::body(&reading));
                detail::panel(item, self.view_label(), self.now, footer, letter)
            })
        };
        let placed = panel::place(class, self.body(class, &anchors.row), detail, layer);
        // "Coming back" beside the hub's "Act now" in the wide layout (task E8.5), in the shell's
        // column at the right.
        let side = self.hub_side().then(|| {
            hub::side(&self.snoozed, hub::handed_off(&self.items, &self.done), self.now, self.cursor.as_deref())
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
                // The rail at the left has the panel button.
                shell::header_bar([], title, trailing)
            }
            // Where the band has the panel button (macOS), the header leaves it out.
            SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => shell::header_bar(
                (!shell::has_band()).then(|| shell::sidebar_toggle(Message::ShowDrawer)),
                widgets::menu_button(nav, Some(icon), name, Message::ToggleNav),
                trailing,
            ),
        }
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
            Some(folder) if folder.query.is_none() => {
                let all = self.folders.iter().find(|f| f.id == folder.id).map(|f| f.total as usize);
                all.unwrap_or(self.messages.len()).to_string()
            }
            Some(_) => self.messages.len().to_string(),
            None if self.view == View::Attachments => self.messages.len().to_string(),
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
            link: colours.text,
            attention: colours.attention_text,
            in_trash: self.in_trash(&letter.id),
            dated: self.view.is_mail(),
            read: self.look().cards(),
            account: account.and_then(|account| accounts.iter().position(|group| group.account == account)),
            task: self.task_of(&letter.id),
        })
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
        let attention = palette(&self.theme()).attention_text;
        let mut content = column![];
        // The search stands above the error line: it belongs to the folder, the error to the list.
        if let Some(folder) = self.view.folder() {
            content = content.push(mail::search_bar(&self.search, folder));
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
                    anchor: row_anchor.clone(),
                    accounts: mail::by_account(&self.folders)
                        .into_iter()
                        .map(|group| mail::address(group.account))
                        .collect(),
                    more: self.more.is_some(),
                    attention,
                    look: self.look(),
                })
            }
        } else if self.is_board() {
            // The notifications window (task E8.4): the sources as chips where the sidebar is folded
            // away, then the board, which caps its own width.
            if shell::sidebar_mode(class, self.sidebar) != Some(SidebarMode::Expanded) {
                content = content.push(notifications::source_chips(&self.listed_sources(), &self.shown));
            }
            let board = notifications::view(notifications::BoardData {
                items: &self.items,
                board: self.board(),
                density: self.density(),
                rows: self.rows_data(row_anchor),
                deciding: self.deciding.is_some(),
                muted: self.sources.iter().filter(|info| info.muted).map(|info| info.source.clone()).collect(),
                single_source: self.shown.single().is_some(),
                icon: self.launch.kind.view_icon(&self.view),
            });
            return container(content.push(board).width(Fill).height(Fill)).width(Fill).into();
        } else if self.is_hub() {
            // The hub's "Act now" (task E8.5), which caps its own width.
            let colours = palette(&self.theme());
            let hub = hub::view(hub::HubData {
                items: &self.items,
                filter: self.filter.as_ref(),
                layout: self.hub_layout(),
                rows: self.rows_data(row_anchor),
                deciding: self.deciding.is_some(),
                attention: colours.attention,
                ok: colours.ok,
            });
            return container(content.push(hub).width(Fill).height(Fill)).width(Fill).into();
        } else if self.view == View::Overview {
            // Its columns cap their own width; the cap below would squeeze two into one.
            let columns = self.overview(class, row_anchor, attention);
            return container(content.push(columns).width(Fill).height(Fill)).width(Fill).into();
        } else {
            list::view(list::ListData { items: &self.items, rows: self.rows_data(row_anchor), attention })
        };
        let list = container(content.push(rows).width(Fill).height(Fill)).width(Fill);
        // Only the wide layout caps the width: in the medium one the content is at most a little
        // over `CONTENT_MAX` anyway, and a cap there would leave a thin empty strip at the right
        // (at 800 wide about 20) that reads as a mistake rather than as a margin.
        match class {
            SizeClass::Wide => list.max_width(size::content_max()).into(),
            SizeClass::Medium | SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => list.into(),
        }
    }

    /// What the rows of entries need (`list::Rows`): the cursor, the time, the mail accounts in the
    /// sidebar's order (their colours) and which entries have a menu "Hide from inbox".
    fn rows_data<'a>(&'a self, anchor: &Anchor) -> list::Rows<'a> {
        list::Rows {
            view: &self.view,
            selected: self.cursor.as_deref(),
            now: self.now,
            anchor: anchor.clone(),
            accounts: mail::by_account(&self.folders).into_iter().map(|group| mail::address(group.account)).collect(),
            hidable: self
                .items
                .iter()
                .filter(|item| !self.hide_choices(item).is_empty())
                .map(|item| item.id.as_str())
                .collect(),
            look: self.look(),
        }
    }

    /// The overview (task E3.9): both columns side by side in the wide layout, each under its head –
    /// with an open panel only the column of its entry, the panel takes the other's place –; one
    /// column, [`App::column`], in the medium and narrow ones.
    fn overview<'a>(&'a self, class: SizeClass, anchor: &Anchor, attention: iced::Color) -> Element<'a, Message> {
        let wide = class == SizeClass::Wide;
        let shown = overview::shown(class, self.detail.as_ref(), self.column, self.launch.kind.columns());
        let cap = overview::width(shown.len());
        let mut columns = row![].width(Fill).height(Fill);
        for (i, column) in shown.into_iter().enumerate() {
            if i > 0 {
                columns = columns.push(rule::vertical(size::HAIRLINE).style(style::rule));
            }
            columns = columns.push(overview::column_view(overview::ColumnData {
                column,
                sections: Sections::of(&self.items, column, self.now),
                info_open: self.info_open[column.index()],
                rows: self.rows_data(anchor),
                deciding: self.deciding.is_some(),
                attention,
                // The mail window has one column, which its header bar names already ("For me").
                head: wide && self.launch.kind != Kind::Mail,
                lead: self.lead(),
            }));
        }
        // Each column at most `CONTENT_MAX`, as the list of a view.
        container(columns).width(Fill).max_width(cap).into()
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
            active: view == self.view && !self.sorting,
            view,
            sort: false,
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
                    sort: true,
                };
                vec![entry(View::Overview, for_me), sort, entry(View::Snoozed, None), entry(View::Done, None)]
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
                let label = if info.muted {
                    trf("{source} (muted)", &[("source", &info.source.label())])
                } else {
                    info.source.label()
                };
                let count = usize::try_from(info.open).ok().filter(|&open| open > 0);
                NavEntry { mark: Mark::Icon(view.icon()), label, count, active: view == self.view, view, sort: false }
            })
            .collect();
        (views, sources)
    }

    /// The smart views of the mail window (task E8.3), under "Smart views": "With attachments", while
    /// there is a mail account. None in the other windows.
    fn smart_entries(&self) -> Vec<NavEntry> {
        if self.launch.kind != Kind::Mail || self.folders.is_empty() {
            return Vec::new();
        }
        let view = View::Attachments;
        vec![NavEntry {
            mark: Mark::Icon(view.icon()),
            label: view.name(),
            count: None,
            active: view == self.view,
            view,
            sort: false,
        }]
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
            .enumerate()
            .map(|(i, group)| MailGroup {
                title: if single { tr("Mail").to_owned() } else { mail::address(group.account).to_owned() },
                account: group.account.to_owned(),
                open: single || self.account_open(group.account, i == 0),
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
                            sort: false,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Whether the folders of `account` (the `first` one listed or not) show: as the person folded
    /// or unfolded it, else only the account of the folder shown – or the first account while no
    /// folder is shown. Two accounts with all their folders would push the rest of the sidebar out
    /// of a window of ordinary height.
    fn account_open(&self, account: &str, first: bool) -> bool {
        if let Some(&(_, open)) = self.folds.iter().find(|(folded, _)| folded == account) {
            return open;
        }
        match self.view.folder() {
            Some(folder) => folder.account == account,
            None => first,
        }
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
            items.push(widgets::sidebar_group_in(mode, tr("Sources"), None));
            items.extend(sources.into_iter().map(row));
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
    /// domains of "Hide from inbox" show whole.
    fn choice_menu<'a>(&self, title: impl Into<String>) -> Element<'a, Message> {
        let head = typo::text(typo::caption(), title.into()).style(theme::text_3);
        let choices = self.choices();
        let width = choice_menu_width(&choices);
        let entries =
            choices.into_iter().enumerate().map(|(i, choice)| choice_entry(choice, keys::digit(i + 1), width));
        let list = column![container(head).padding(Padding::new(space::s()).bottom(space::xs()))].extend(entries);
        container(list).width(width).padding(style::MENU_INSET).style(style::container::menu).into()
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
        let mut rows = column![widgets::prop_line(tr("List"), Some(Mark::Icon(Icon::ListTodo)), list, None)];
        // A folder's message (task E8.7b) gets no due day: its request cannot carry one from here.
        if self.task_mail.is_none() {
            let due = choice(Due::ALL.to_vec(), Some(self.task.due), Message::TaskDue);
            rows = rows.push(widgets::prop_line(tr("Due"), Some(Mark::Icon(Icon::Calendar)), due, None));
        }
        let create = self.task.list.is_some().then_some(Message::TaskConfirm);
        let actions = row![
            iced::widget::space::horizontal(),
            widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseMenu)),
            widgets::text_button(Some(Icon::Check), tr("Create task"), style::button::primary, create),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let card = column![
            container(widgets::section_title(tr("Make task"))).padding(inset.bottom(space::xs())),
            container(title).padding(Padding::new(0.0).bottom(space::s())),
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
            Some(Open::Mail) => {
                self.cursor = self.messages.first().map(|message| message.id.clone());
                self.reading = self.cursor.is_some();
            }
            Some(Open::MailEntry) => {
                self.detail = self.items.iter().find(|item| matches!(item.link, Some(Link::Mail { .. }))).cloned();
            }
            Some(open @ (Open::Overview | Open::OverviewNotices)) => {
                let column = if open == Open::Overview { Column::Mail } else { Column::Notices };
                self.column = self.launch.kind.column_or_first(column);
                self.cursor = self.rows().first().map(|item| item.id.clone());
            }
            Some(Open::Tile) => self.filter = hub::tiles(&self.items).first().map(|tile| tile.origin.clone()),
            Some(open @ (Open::Hide | Open::HidePanel)) => {
                if let Some(item) = self.items.iter().find(|item| item.source.kind == SourceKind::Mail).cloned() {
                    self.cursor = Some(item.id.clone());
                    if open == Open::HidePanel {
                        self.detail = Some(item.clone());
                    }
                    self.ask_hide(&item.id);
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
        Task::perform(fetch(inbox, view.clone(), empty, limit, window, demo), move |result| {
            Message::Loaded(view.clone(), result)
        })
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
    /// The hub's "To sort" (task E8.5): it shows the section alone on the overview, not a view.
    sort: bool,
}

impl NavEntry {
    /// What a click on the row sends: the view, or the hub's To sort alone.
    fn message(&self) -> Message {
        if self.sort { Message::ShowToSort } else { Message::Show(self.view.clone()) }
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

/// One entry of [`App::choice_menu`], as `widgets::menu` draws one: icon, label, then – quietly,
/// tabular – when it snoozes until, and the digit that picks it; with [`Choice::line`] the line
/// `widgets::menu` draws between its sections above it.
fn choice_entry<'a>(choice: Choice, key: &'static str, width: f32) -> Element<'a, Message> {
    let line = choice.line;
    let entry = choice_button(choice, key, width);
    if !line {
        return entry;
    }
    let rule = container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]);
    column![rule, entry].into()
}

/// The button of [`choice_entry`] in a menu `width` wide.
///
/// A label longer than its room – only beyond [`choice_menu_width`]'s widest menu – ends in "…"
/// ([`choice_label`]) and shows whole as tooltip.
fn choice_button<'a>(choice: Choice, key: &'static str, width: f32) -> Element<'a, Message> {
    let mark = icon::icon(choice.icon, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text) });
    let shown = choice_label(&choice.label, choice.when.as_deref(), key, width);
    let cut = (shown != choice.label).then(|| choice.label.clone());
    let label = container(typo::text(typo::body(), shown).wrapping(Wrapping::None)).width(Fill).clip(true);
    let mut content = row![mark, label].spacing(space::s()).align_y(Alignment::Center);
    if let Some(when) = choice.when {
        content = content.push(typo::text(typo::meta().tabular(), when).style(theme::text_3).wrapping(Wrapping::None));
    }
    content = content.push(widgets::kbd(key));
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
/// needs to show whole – the address or domain of "Hide from inbox" (task E3.10) –, at most
/// [`size::panel()`], beyond which a label ends in "…".
fn choice_menu_width(choices: &[Choice]) -> f32 {
    let needed = choices.iter().enumerate().map(|(i, choice)| {
        choice_frame(choice.when.as_deref(), keys::digit(i + 1)) + typo::text_width(&choice.label, typo::body())
    });
    needed.fold(widgets::menu_width(), f32::max).ceil().min(size::panel())
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
        .padding(style::pick_list_padding())
        .text_size(typo::caption().size)
        .text_line_height(typo::caption().line_height)
        .font(typo::caption().font)
        .style(style::pick_list)
        .menu_style(style::pick_list_menu)
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
    Failure::new(tr("Couldn't load the inbox."), err)
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
    fn load<S: InboxSource + MailSource + 'static>(app: &mut App<S>) {
        let empty = app.launch.open == Some(Open::Empty);
        let window = (app.launch.kind, app.shown.clone());
        let demo = app.launch.demo.then_some(app.now);
        let result = block_on(fetch(app.inbox.clone(), app.view.clone(), empty, app.page_limit(), window, demo));
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

    /// The open entries of "For me" in the demo, waiting first: the three waiting ones, then by time,
    /// the mail entries among them.
    const FOR_ME: [&str; 13] = [
        "demo-1",
        "demo-3",
        "demo-2",
        "demo-mail-1",
        "demo-4",
        "demo-6",
        "demo-mail-4",
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
        let codex = View::Source(SourceRef::agent("codex")).filter().expect("entries");
        assert_eq!((codex.lane, codex.source, codex.state), (None, Some(SourceRef::agent("codex")), open));
        assert_eq!(View::ForMe.name(), tr("For me"));
        assert_eq!(View::Source(SourceRef::agent("claude-code")).name(), "Claude Code");
    }

    #[test]
    fn for_me_shows_urgent_entries_first_and_counts_them() {
        let app = loaded(&[]);
        assert_eq!(ids(&app), FOR_ME);
        assert_eq!(app.for_me.len(), 13);
        assert_eq!(app.count_note(), "13", "only the number: the title already says For me");
        let groups = list::groups(&app.items);
        assert_eq!(groups, [(list::Group::Waiting, 0..3), (list::Group::Other, 3..13)], "waiting entries get a head");
        assert_eq!(app.sources.len(), 11, "suite, two agents, six scripts, two mail accounts");
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("For me"))]));
    }

    #[test]
    fn other_views_keep_the_for_me_count() {
        let mut app = loaded(&[]);
        let _ = app.update(Message::Show(View::Info));
        load(&mut app);
        assert_eq!(ids(&app), ["demo-8", "demo-9", "demo-mail-3", "demo-mail-5", "demo-7", "demo-mail-7"]);
        assert_eq!(app.for_me.len(), 13, "the sidebar still counts For me");
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
        assert_eq!(glance::waiting(&app.for_me), 3);
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

    /// [`loaded`] late in the demo's day, when the codex session done 22 hours before was done
    /// today: the hub's "Done today" lists it (task E8.5).
    fn loaded_late(words: &[&str]) -> App<DemoSource> {
        let mut all = vec!["--demo", "--now", "2026-10-13T23:30", "--lane", "for-me"];
        all.extend_from_slice(words);
        let mut app = demo(&all);
        load(&mut app);
        app
    }

    #[test]
    fn an_answer_for_a_view_left_behind_is_dropped() {
        let mut app = loaded_late(&[]);
        let for_me = block_on(fetch(
            app.inbox.clone(),
            View::ForMe,
            false,
            mail::PAGE,
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
        let _ = app.update(Message::Loaded(app.view.clone(), Err(DEMO_ERROR.into())));
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
        assert_eq!(labels, [tr("Act now"), tr("To sort"), tr("Later"), tr("Done today")], "the hub's (task E8.5)");
        assert_eq!(
            views.iter().map(NavEntry::trail).collect::<Vec<_>>()[..2],
            [Trail::Count(13), Trail::Count(9)],
            "Act now counts what is for the user, To sort what of it is to sort"
        );
        let by_source: usize = sources.iter().filter_map(|e| e.count).sum();
        assert_eq!(by_source, 13 + 6, "the sources add up to For me plus Info");
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

    fn press<S: InboxSource + MailSource + 'static>(app: &mut App<S>, key: Key) -> Task<Message> {
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
            Some(Sent::Hide(id, scope)) => {
                let result = block_on(app.inbox.hide_sender(id, scope)).map_err(|err| err.to_string());
                let _ = app.update(Message::Hidden(result));
            }
            Some(Sent::Unhide(rule, reopen)) => {
                let back = reopen.first().cloned();
                let result: Result<(), String> = (|| {
                    if let Some(rule) = rule {
                        block_on(app.inbox.remove_rule(rule)).map_err(|err| err.to_string())?;
                    }
                    for id in reopen {
                        block_on(app.inbox.set_item_done(id, false)).map_err(|err| err.to_string())?;
                    }
                    Ok(())
                })();
                let _ = app.update(Message::Undone(result, back));
            }
            Some(Sent::DismissAll(ids)) => {
                for id in &ids {
                    block_on(app.inbox.dismiss_item(id.clone())).expect("the demo dismisses");
                }
                let _ = app.update(Message::DismissedAll(ids, None));
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
        assert_eq!(cursor(&app), Some("demo-1"), "down starts at the top");
        let _ = press(&mut app, Key::Down);
        assert_eq!(cursor(&app), Some("demo-3"));
        let _ = press(&mut app, Key::Up);
        let _ = press(&mut app, Key::Up);
        assert_eq!(cursor(&app), Some("demo-1"), "stops at the first row");
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
        let (_, second) = list::row_span(&app.items, 1, Look::Rows).expect("a second row");
        let _ = app.update(Message::Scrolled(0.0, second));
        assert_eq!(press(&mut app, Key::Down).units(), 0, "the first row is in view");
        let _ = press(&mut app, Key::Down);
        assert!(press(&mut app, Key::Down).units() > 0, "the third row needs a scroll");
        let (_, third) = list::row_span(&app.items, 2, Look::Rows).expect("a third row");
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
        assert_eq!(cursor(&app), Some("demo-mail-1"));
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

    #[test]
    fn m_offers_the_steps_for_mail() {
        let mut app = loaded(&["--open", "info"]);
        let sale = app.items.iter().find(|item| item.title == "Autumn sale: 20% off").cloned().expect("the sale");
        app.cursor = Some(sale.id.clone());
        let _ = press(&mut app, Key::M);
        assert_eq!((app.menu, app.focus()), (Some(Menu::Hide), Focus::Choices));
        let choices = app.choices();
        let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                tr("This conversation"),
                trf("Everything from {sender}", &[("sender", &"news@shop.example")]).as_str(),
                trf("Everything from {domain}", &[("domain", &"shop.example")]).as_str(),
                tr("Always to Info"),
                tr("Move to junk"),
            ]
        );
        let lines: Vec<bool> = choices.iter().map(|c| c.line).collect();
        assert_eq!(lines, [false, false, false, true, false], "a line before the lane and junk");

        let _ = press(&mut app, Key::Digit(3));
        assert_eq!(app.sent.last(), Some(&Sent::Hide(sale.id.clone(), HideScope::Domain)));
        assert!(app.menu.is_none());
        deliver(&mut app);
        let text = trf("Hidden: everything from {domain}", &[("domain", &"shop.example")]);
        let hint = app.hint.clone().expect("a hint");
        // The work account's mail from the shop is hidden too (task E3.11), and Undo takes both back.
        let all = block_on(app.inbox.list_items(ItemFilter::default())).expect("the demo answers");
        let winter = all.into_iter().find(|item| item.title == "Winter jackets are here");
        let winter = winter.expect("the shop's mail in the work account").id;
        let undo = Undo::Hidden { rule: Some("demo-rule-1".into()), reopen: vec![sale.id.clone(), winter.clone()] };
        assert_eq!((hint.text, hint.undo), (text, Some(undo.clone())), "an exact undo");
        assert!(!ids(&app).contains(&sale.id.as_str()), "out of the inbox");
        let Some(Link::Mail { message }) = &sale.link else { panic!("a mail entry") };
        let mail = app.inbox.get_mail(message);
        let flags = mail.map(|mail| mail.summary.flags).expect("the mail");
        assert!(!flags.contains(&torro_mail::Flag::Seen), "hiding leaves the mail unread");

        let _ = press(&mut app, Key::U);
        let Undo::Hidden { rule, reopen } = undo else { panic!("hiding's undo") };
        assert_eq!(app.sent.last(), Some(&Sent::Unhide(rule, reopen)));
        deliver(&mut app);
        assert!(ids(&app).contains(&sale.id.as_str()) && ids(&app).contains(&winter.as_str()), "both back");
        assert_eq!(cursor(&app), Some(sale.id.as_str()), "the cursor back on the entry");
        let again = block_on(app.inbox.hide_sender(sale.id.clone(), HideScope::Domain)).expect("hidden again");
        assert!(again.new_rule, "the rule was removed, so hiding makes it anew");
    }

    #[test]
    fn this_conversation_hides_without_marking_the_mail_read_and_undoes() {
        let mut app = loaded(&[]);
        let plans = app.items.iter().find(|item| item.title == "Weekend plans").cloned().expect("a mail entry");
        app.cursor = Some(plans.id.clone());
        let _ = press(&mut app, Key::M);
        let _ = press(&mut app, Key::Digit(1));
        assert_eq!(app.sent.last(), Some(&Sent::Hide(plans.id.clone(), HideScope::Conversation)));
        deliver(&mut app);
        let hint = app.hint.clone().expect("a hint");
        let undo = Undo::Hidden { rule: None, reopen: vec![plans.id.clone()] };
        assert_eq!((hint.text.as_str(), hint.undo), (tr("Conversation hidden"), Some(undo)));
        assert!(!ids(&app).contains(&plans.id.as_str()), "out of the inbox");
        let Some(Link::Mail { message }) = &plans.link else { panic!("a mail entry") };
        let flags = app.inbox.get_mail(message).map(|mail| mail.summary.flags).expect("the mail");
        assert!(!flags.contains(&torro_mail::Flag::Seen), "unlike done, the mail stays unread");
        // The undo key of the suite does what "Undo" does.
        let _ = app.update(Message::Undo);
        assert_eq!(app.sent.last(), Some(&Sent::Unhide(None, vec![plans.id.clone()])));
        deliver(&mut app);
        assert!(ids(&app).contains(&plans.id.as_str()), "back");
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

    #[test]
    fn the_sender_is_hidden_and_mail_goes_to_junk_with_an_undo() {
        let (mut app, offer, address) = at_mail();
        let _ = press(&mut app, Key::M);
        let _ = press(&mut app, Key::Digit(2));
        assert_eq!(app.sent.last(), Some(&Sent::Hide(offer.id.clone(), HideScope::Sender)));
        deliver(&mut app);
        let text = trf("Hidden: {sender}", &[("sender", &address)]);
        assert_eq!(app.hint.as_ref().map(|hint| hint.text.clone()), Some(text));

        let (mut app, item, _) = at_mail();
        let _ = press(&mut app, Key::M);
        let junk = app.choices().len();
        let _ = press(&mut app, Key::Digit(u8::try_from(junk).expect("a digit")));
        let Some(Sent::Mail(MailEdit::Move { to, .. }, Some(hint))) = app.sent.last() else {
            panic!("a move with a hint: {:?}", app.sent.last())
        };
        assert_eq!(*to, MoveTarget::Role(MoveRole::Junk));
        assert!(matches!(&hint.undo, Some(Undo::Mail { reopen: Some(id), .. }) if *id == item.id));
    }

    #[test]
    fn the_hide_menu_is_wide_enough_for_whole_addresses() {
        let (mut app, _, _) = at_mail();
        let _ = press(&mut app, Key::M);
        let choices = app.choices();
        let width = choice_menu_width(&choices);
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

    #[test]
    fn open_hide_shows_the_menu_of_the_first_mail_entry() {
        for (open, panel) in [("hide", false), ("hide-panel", true)] {
            let mut app = loaded(&["--open", open]);
            assert_eq!(app.menu, Some(Menu::Hide), "{open}");
            let subject = app.subject.clone().expect("a subject");
            assert_eq!((subject.source.kind, app.cursor.as_deref()), (SourceKind::Mail, Some(subject.id.as_str())));
            assert_eq!(app.detail.is_some(), panel, "{open}");
            for size in SIZES {
                let _ = app.update(Message::Resized(size));
                let _ = app.view();
            }
        }
    }

    #[test]
    fn mail_without_a_junk_folder_or_sender_offers_less() {
        let (mut app, item, _) = at_mail();
        app.folders.retain(|folder| folder.role != Some(Role::Junk));
        let anonymous = Item { from: None, ..item };
        let labels: Vec<String> = app.hide_choices(&anonymous).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [tr("This conversation").to_owned()], "nothing to hide by, nowhere to move");
        let done = Item { state: State::Done { at: app.now }, ..anonymous };
        assert!(app.hide_choices(&done).is_empty());
        let _ = press(&mut app, Key::M);
        assert_eq!(app.menu, Some(Menu::Hide), "the mail itself still has its sender");
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
        let result = block_on(edit.apply(app.inbox.clone()));
        let _ = app.update(Message::MailEdited(result, hint));
        load(app);
    }

    /// The whole mail `id` from the demo, handed to the panel as the task would.
    fn deliver_letter(app: &mut App<DemoSource>, id: &str) {
        let mail =
            block_on(MailSource::get_mail(app.inbox.as_ref(), id.into())).map(Box::new).map_err(|err| err.to_string());
        let _ = app.update(Message::LetterLoaded(id.into(), mail));
    }

    fn draw_everywhere<S: InboxSource + MailSource + 'static>(app: &mut App<S>) {
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
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("Inbox"))]));
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

    #[test]
    fn the_sidebar_groups_the_folders_by_account() {
        let mut app = loaded(&[]);
        let groups = app.mail_groups();
        let titles: Vec<&str> = groups.iter().map(|group| group.title.as_str()).collect();
        assert_eq!(titles, ["anna@work.example.org", "anna@example.org"], "two accounts: one group each");
        assert_eq!(groups.iter().map(|group| group.open).collect::<Vec<_>>(), [true, false], "the first unfolded");
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
        assert!(!app.mail_groups()[0].open, "folded by hand");
        let _ = app.update(Message::Resized(WIDE));
        let _ = app.view();
        let _ = app.update(Message::ToggleAccount("mail:anna@work.example.org".into()));
        assert!(app.mail_groups()[0].open);
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

    #[test]
    fn a_search_shows_what_it_found_and_clearing_it_the_whole_folder() {
        let mut app = in_folder("f:work-inbox");
        let _ = app.update(Message::SearchInput("minutes".into()));
        let _ = app.update(Message::Search);
        assert_eq!(app.view.folder().and_then(|folder| folder.query.as_deref()), Some("minutes"));
        load(&mut app);
        assert_eq!(message_ids(&app), ["m:w6"]);
        assert_eq!(app.count_note(), "1", "what it found");
        draw_everywhere(&mut app);
        let _ = app.update(Message::ClearSearch);
        assert_eq!((app.search.as_str(), app.view.folder().and_then(|f| f.query.clone())), ("", None));
        load(&mut app);
        assert_eq!(app.messages.len(), 8);
        let _ = app.update(Message::SearchInput("nothing like this".into()));
        let _ = app.update(Message::Search);
        load(&mut app);
        assert!(app.messages.is_empty());
        draw_everywhere(&mut app);
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
        let page =
            block_on(MailSource::search_mail(app.inbox.as_ref(), "f:work-inbox".into(), None, 4, Some("4".into())))
                .map_err(|err| err.to_string());
        let _ = app.update(Message::MoreLoaded(app.view.clone(), page));
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

    fn rows_of<S: InboxSource + MailSource + 'static>(app: &App<S>) -> Vec<&str> {
        app.rows().iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn the_window_starts_on_the_overview_with_every_open_entry() {
        let app = overview_at(&[]);
        assert_eq!(app.view, View::Overview);
        assert!(app.is_hub(), "the hub's overview is Act now (task E8.5)");
        assert_eq!(app.items.len(), FOR_ME.len() + 6, "both lanes in one answer");
        assert_eq!((app.for_me.len(), app.info), (13, 6), "the sidebar's numbers from the same answer");
        assert_eq!(app.count_note(), hub::summary(&hub::Sections::of(&app.items, None)));
        assert!(app.nav_entries().0[0].active, "the first entry of the sidebar");
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("Act now"))]));
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
        // The notifications window shows what the apps report (task E8.4): the scripts of the demo,
        // its "New" starting with the deploy notice.
        let mut notices = overview_at(&["--view", "notifications"]);
        assert_eq!((notices.view.clone(), notices.column), (View::Overview, Column::Notices));
        assert_eq!(rows_of(&notices).first(), Some(&"demo-6"), "the keys start on its first card");
        assert!(rows_of(&notices).iter().all(|id| !id.starts_with("demo-mail")), "no mail");
        let _ = press(&mut notices, Key::H);
        let _ = notices.update(Message::ShowColumn(Column::Mail));
        assert_eq!(notices.column, Column::Notices, "there is no mail column to go to");
        assert_eq!(notices.title(), trf("Notifications · {view}", &[("view", &tr("New"))]));

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
        assert_eq!(rows_of(&app).len(), 9);
        assert!(rows_of(&app).iter().all(|id| *id != "demo-3"), "no approval to sort");
        assert_eq!(app.title(), trf("Inbox · {view}", &[("view", &tr("To sort"))]));
        assert_eq!(app.count_note(), "9");
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
        assert_eq!(late.title(), trf("Inbox · {view}", &[("view", &tr("Done today"))]));
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
        assert_eq!(app.unfolded(), ["overview.notices.info"]);
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
        assert_eq!((app.menu, app.column), (Some(Menu::Hide), Column::Mail));
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
        assert_eq!((app.column, cursor(&app)), (Column::Notices, Some("demo-6")), "the first card");
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
        assert_eq!(smart, [tr("With attachments")]);
        assert!(!app.mail_groups().is_empty(), "the folders of every account");
        assert!(overview_at(&["--view", "hub"]).smart_entries().is_empty(), "smart views are the mail window's");
        for size in SIZES {
            let mut app = mail_window("comfortable", &["--open", "menu"]);
            let _ = app.update(Message::Resized(size));
            let _ = app.view();
        }
    }

    /// Task E8.3: cards in the comfortable mail window – with words when narrow –, rows elsewhere.
    #[test]
    fn comfortable_mail_is_cards_and_everything_else_rows() {
        let mut app = mail_window("comfortable", &[]);
        let _ = app.update(Message::Resized(WIDE));
        assert_eq!(app.look(), Look::Cards { words: false });
        let _ = app.update(Message::Resized(NARROW));
        assert_eq!(app.look(), Look::Cards { words: true });
        let compact = mail_window("compact", &[]);
        assert_eq!(compact.look(), Look::Rows);
        let hub = overview_at(&["--view", "hub", "--density", "comfortable"]);
        assert_eq!(hub.look(), Look::Rows, "the hub's cards come with E8.5");
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
        assert_eq!(app.menu, Some(Menu::Hide), "m");
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
        assert!(apps.items.iter().all(|item| item.source.kind == SourceKind::Script), "{:?}", ids(&apps));
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

    /// Task E8.4: `m` (and the bell) mutes the source at once in the notifications window; the
    /// suite is never muted.
    #[test]
    fn the_bell_mutes_the_source_at_once() {
        let mut app = notifications_window(&[]);
        let _ = app.update(Message::Act("demo-6".into(), Command::Hide));
        assert_eq!(app.sent.last(), Some(&Sent::Mute(SourceRef::script("deploy"), true, true)));
        assert_eq!(app.menu, None, "no menu until E8.6b");
        let mut suite = notifications_window(&["--source", "suite"]);
        let _ = suite.update(Message::Act("demo-5".into(), Command::Hide));
        assert!(suite.sent.is_empty());
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
        let newest = notifications::by_time(&app.items).into_iter().map(|item| item.id.as_str()).collect::<Vec<_>>();
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
            notifications::span(&app.items, app.board(), &app.view, app.density(), &first).expect("shown");
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
        // A folder's message: `t` opens the dialog without a due day, "Create task" sends
        // `tasks.create_todo_from_mail`, and the reader shows the task.
        let mut app = in_folder("f:work-archive");
        let _ = press(&mut app, Key::Down);
        let id = cursor(&app).map(str::to_owned).expect("a message");
        let _ = press(&mut app, Key::T);
        assert_eq!((app.menu, app.task_mail.as_ref().map(|m| m.id.clone())), (Some(Menu::Task), Some(id.clone())));
        deliver_lists(&mut app);
        let _ = app.view();
        let _ = app.update(Message::TaskConfirm);
        let Some(Sent::MailTask { account, id: sent, list, title }) = app.sent.pop() else { panic!("no mail task") };
        assert_eq!((account.as_str(), sent.as_str()), (torro_inbox::demo::MAIL_ACCOUNTS[0], id.as_str()));
        let made = block_on(app.inbox.create_todo_from_mail(account, sent, list, title)).map_err(|e| e.to_string());
        let _ = app.update(Message::MailTaskCreated(id.clone(), made));
        assert!(app.task_of(&id).is_some(), "the reader shows the task made");
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
}
