//! The hub (task E8.5 in `docs/tasks/eingang.md`; `docs/eingang.md`, "Drei Fenster"): the inbox
//! window that is about acting, not reading – "Act now".
//!
//! - **Sections** ([`Section`], [`Sections`]): the open entries of "For me" in three (four with
//!   Follow up, below), top to bottom:
//!   **Decide** (approvals: Allow · Deny · Go to window), **Agents** (coding agents waiting or done:
//!   Go to window · Later · Done) and **To sort** (everything else, each with its targets: Task ·
//!   Event · Later · Archive for mail – task E18.5: mail is archived, never done –, Open · Task ·
//!   Event · Later · ✕ for the rest). "Info" is not here: it is for
//!   reading, which the mail and the notifications window do.
//! - **Buttons** ([`acts`]): the concept's table "Knöpfe je Eintrag". An approval has **neither ✕
//!   nor Later** – it waits for yes or no and must not vanish by accident. Every button sends what
//!   its key sends at the entry (`Message::Act`, `Message::OpenRow`, `Message::Decide`).
//! - **Tiles** ([`tiles`]): over the sections one tile per origin (mail, agents, the suite, each
//!   script) with its number in `DISPLAY` and one line; a click filters the sections to it
//!   (`Message::Filter`), a second click shows everything again. As many as fit in one row
//!   ([`tile_columns`]), the busiest first.
//! - **Follow up** (task E8.7b, [`Section::FollowUp`]): between Decide and Agents the suite's
//!   question after a task made of a mail was finished ("You finished a task from a mail", which the
//!   core files in "Info"): Reply · Archive mail · Not now, built from the task it carries.
//! - **Handed off** and **Coming back** ([`side`]): in the wide layout a column at the right with the
//!   open tasks made of entries (task E8.7b, [`handed_off`]; a click opens the task in the tasks app),
//!   then the snoozed entries and when they wake, the soonest first.
//! - **Sizes** ([`layout`]): wide and medium lay Agents out as a grid of cards; Decide and Follow
//!   up take the full width where only two cards fit a line and stand side by side where three do
//!   ([`section_blocks`], task E8.12); To sort is one card per line, at most a reading width, its
//!   buttons at the right in fixed columns ([`SortSlots`]) and without a caption of its own while
//!   the window shows it alone; narrow shows the numbers instead of the tiles, one card per line,
//!   big Allow and Deny (for decisions only), and To sort as cards with their buttons below. The
//!   compact density draws Agents and To sort as rows with the action bar; approvals stay cards, so
//!   Allow and Deny are always there. Mini and strip show the glance, as every window.
//! - **Words on buttons** (task E8.12): a card's buttons carry all their words or none; only the
//!   lead (Open, Go to window, Reply) may keep its word alone (`widgets::BarWords`). What lets the
//!   entry go (Done, Archive, Dismiss, Not now) is `ghost`, every other visible button `secondary`.
//!
//! - **Quiet** and **Event** (task E8.5b): while a busy event keeps the inbox quiet
//!   (`inbox.get_quiet`), the column at the right starts with "Quiet" – until when and the event
//!   (or "A busy event" where the core keeps its title) –, and where there is no such column one
//!   line says the same under the tiles or the numbers ([`quiet_line`]). To sort has "Event" after
//!   "Task" ([`Act::Event`], `c`): a small dialog in the app makes an event of the entry
//!   (`inbox.create_event_from_item`).
//!
//! The app lists the entries (`inbox.list_items`, the snoozed ones for "Coming back") and sends the
//! changes; the hub lays out, and its heights are plain functions ([`span`]) so the app can scroll the
//! cursor into view. Its own requests – quiet, the calendars and the event – go through
//! [`HubSource`], so the demo can answer them too.

use std::future::Future;

use chrono::{DateTime, NaiveDate, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, responsive, row, rule, scrollable};
use iced::{Alignment, Color, Element, Fill, Padding, Theme};
use torro_core::AppId;
use torro_core::design::Density;
use torro_core::i18n::{tr, trc, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, BarWords, ButtonStyle, Mark, RowAction, TileFace, TileSize};
use torro_gui::{style, typo};
use torro_inbox::demo::DemoSource;
use torro_inbox::item::ItemTask;
use torro_inbox::source::TaskList;
use torro_inbox::{Action, InboxError, InboxSource, Item, Kind, Lane, Link, SourceKind, SourceRef, State};
use torro_ipc::Quiet;
use torro_ipc::inbox::InboxClient;

use crate::app::Message;
use crate::keys::Command;
use crate::list::{self, Rows};
use crate::{detail, glance, notifications};

/// How many lines of its text a card of Decide or Agents shows, as the notifications window's cards.
const TEXT_LINES: usize = 2;

/// What the hub asks the core for besides the entries (task E8.5b): whether it is quiet, the
/// calendars an event can go to, and the event of an entry. The core's client sends the requests;
/// the demo answers them from its own data.
pub trait HubSource: Send + Sync {
    /// `inbox.get_quiet`: `Some` while only urgent entries pop up, with until when and – for the app
    /// key always – the busy event's title; `None` when it is not quiet.
    fn get_quiet(&self) -> impl Future<Output = Result<Option<Quiet>, InboxError>> + Send;

    /// The calendars an event can be made in (`calendar.list_calendars`, the writable ones with
    /// events), each as id and name, in the core's order.
    fn list_calendars(&self) -> impl Future<Output = Result<Vec<TaskList>, InboxError>> + Send;

    /// `inbox.create_event_from_item`: an event of the entry `id` in the calendar `calendar` from
    /// `start` to `end`, titled `title` (`None`: the entry's title); the entry is done then.
    fn create_event_from_item(
        &self,
        id: String,
        calendar: String,
        title: Option<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;
}

impl HubSource for InboxClient {
    async fn get_quiet(&self) -> Result<Option<Quiet>, InboxError> {
        InboxClient::get_quiet(self).await
    }

    async fn list_calendars(&self) -> Result<Vec<TaskList>, InboxError> {
        InboxClient::list_calendars(self).await
    }

    async fn create_event_from_item(
        &self,
        id: String,
        calendar: String,
        title: Option<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<(), InboxError> {
        InboxClient::create_event_from_item(self, id, calendar, title, start, end).await
    }
}

/// The demo's calendars ([`HubSource::list_calendars`]): ids and names.
pub const DEMO_CALENDARS: [(&str, &str); 2] = [("demo/calendar/work", "Work"), ("demo/calendar/personal", "Personal")];

/// The demo's busy event: it keeps the demo quiet from its "now" for this many minutes.
const DEMO_QUIET_MINUTES: i64 = 90;

/// The demo is always quiet in a customer call that ends [`DEMO_QUIET_MINUTES`] after the newest
/// entry's time (its "now", as `--now` sets it); an event of an entry marks the entry done, as the
/// core does, and is kept nowhere.
impl HubSource for DemoSource {
    async fn get_quiet(&self) -> Result<Option<Quiet>, InboxError> {
        let all = self.list_items(torro_inbox::ItemFilter::default()).await?;
        let now = all.iter().map(|item| item.updated).max().unwrap_or_else(Utc::now);
        let until = now + chrono::TimeDelta::minutes(DEMO_QUIET_MINUTES);
        Ok(Some(Quiet { reason: torro_ipc::QuietReason::Busy, until, title: Some("Customer call".into()) }))
    }

    async fn list_calendars(&self) -> Result<Vec<TaskList>, InboxError> {
        Ok(DEMO_CALENDARS.iter().map(|(id, name)| TaskList { id: (*id).into(), name: (*name).into() }).collect())
    }

    async fn create_event_from_item(
        &self,
        id: String,
        _calendar: String,
        _title: Option<String>,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<(), InboxError> {
        self.set_item_done(id, true).await.map(drop)
    }
}

#[cfg(test)]
impl HubSource for crate::empty::EmptySource {
    async fn get_quiet(&self) -> Result<Option<Quiet>, InboxError> {
        Ok(None)
    }

    async fn list_calendars(&self) -> Result<Vec<TaskList>, InboxError> {
        Ok(Vec::new())
    }

    async fn create_event_from_item(
        &self,
        _id: String,
        _calendar: String,
        _title: Option<String>,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }
}

/// When quiet ends, in local time: "15:00", or "Wed, 14 Oct, 09:00" on another day than `now`'s.
fn quiet_end(quiet: &Quiet, now: DateTime<Utc>) -> String {
    let local = quiet.until.with_timezone(&chrono::Local);
    let today: NaiveDate = now.with_timezone(&chrono::Local).date_naive();
    let time = torro_gui::datetime::format_time(local.time());
    if local.date_naive() == today {
        return time;
    }
    format!("{}, {time}", torro_gui::datetime::format_short_date(local.date_naive(), today))
}

/// The busy event's title on one line, "A busy event" where the core keeps it from the app.
fn quiet_event(quiet: &Quiet) -> String {
    let title = quiet.title.as_deref().map(list::one_line).filter(|title| !title.is_empty());
    title.unwrap_or_else(|| tr("A busy event").to_owned())
}

/// What "Quiet" in the column at the right says: "Until 15:00" ([`quiet_end`]) over the busy event
/// ([`quiet_event`]).
pub fn quiet_words(quiet: &Quiet, now: DateTime<Utc>) -> (String, String) {
    (trf("Until {time}", &[("time", &quiet_end(quiet, now))]), quiet_event(quiet))
}

/// The line about quiet over the sections where no column at the right says it (task E8.5b): "Quiet
/// until 15:00 · Customer call", drawn with the moon in `text_2`.
pub fn quiet_line(quiet: &Quiet, now: DateTime<Utc>) -> String {
    format!("{} · {}", trf("Quiet until {time}", &[("time", &quiet_end(quiet, now))]), quiet_event(quiet))
}

/// The sections of "Act now", top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Approvals: the suite asks before an agent may do something.
    Decide,
    /// The suite's question after a task made of a mail was finished (task E8.7b, [`is_follow_up`]):
    /// answer the mail or mark it done.
    FollowUp,
    /// Coding agents waiting for the user or done with their turn.
    Agents,
    /// Every other entry for the user: to hand on (task, later) or to let go (done, ✕).
    ToSort,
}

impl Section {
    /// Top to bottom.
    pub const ALL: [Section; 4] = [Section::Decide, Section::FollowUp, Section::Agents, Section::ToSort];

    /// The section `item` stands in: approvals in Decide, the suite's "You finished a task from a
    /// mail" in Follow up (from "Info", where the core files it so that it never pops up), coding
    /// agents in Agents, every other open entry of "For me" in To sort. `None` for entries that are
    /// not open or in "Info".
    pub fn of(item: &Item) -> Option<Section> {
        if item.state.is_open() && is_follow_up(item) {
            return Some(Section::FollowUp);
        }
        if !item.state.is_open() || item.lane != Lane::ForMe {
            return None;
        }
        Some(match item.kind {
            Kind::Approval => Section::Decide,
            Kind::AgentWaiting | Kind::AgentDone => Section::Agents,
            Kind::Notice | Kind::AccountProblem | Kind::Message | Kind::Alert | Kind::Reminder => Section::ToSort,
        })
    }

    /// The caption over the section ("Decide · 2").
    pub fn title(self) -> &'static str {
        match self {
            Section::Decide => tr("Decide"),
            Section::FollowUp => tr("Follow up"),
            Section::Agents => tr("Agents"),
            Section::ToSort => tr("To sort"),
        }
    }

    /// The word after its number, in the header bar's summary and the narrow numbers ("2 to
    /// decide").
    pub fn word(self) -> &'static str {
        match self {
            Section::Decide => tr("to decide"),
            Section::FollowUp => tr("to follow up"),
            Section::Agents => tr("agents"),
            Section::ToSort => tr("to sort"),
        }
    }
}

/// Whether `item` is the suite's entry "You finished a task from a mail" (task E8.7a): the suite's
/// own, carrying the finished task with the open mail entry its mail waits in
/// ([`torro_inbox::item::ItemTask::mail_entry`]). Its buttons are Reply and "Archive mail" (tasks
/// E8.7b, E18.5), built from that task – the entry has no actions of its own.
pub fn is_follow_up(item: &Item) -> bool {
    item.source.kind == SourceKind::Suite && item.task.as_ref().is_some_and(|task| task.mail_entry.is_some())
}

/// The tasks handed off from entries that are still open (task E8.7b, "Handed off" beside "Act
/// now"): the [`Item::task`] of `items` and `done` (entries made into a task are done), not
/// finished, each task once (several mails of one conversation share it), in the order given. The
/// suite's [`is_follow_up`] entries carry finished tasks and never count.
pub fn handed_off<'a>(items: &'a [Item], done: &'a [Item]) -> Vec<&'a ItemTask> {
    let mut tasks: Vec<&ItemTask> = Vec::new();
    for task in items.iter().chain(done).filter(|item| !is_follow_up(item)).filter_map(|item| item.task.as_ref()) {
        if !task.completed && !tasks.iter().any(|known| known.uid == task.uid) {
            tasks.push(task);
        }
    }
    tasks
}

/// Where an entry comes from, as the tiles count it: all mail accounts together, all coding agents
/// together, the suite, every script on its own, and each connected service with all its accounts
/// together (task E5.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Mail,
    Agents,
    Suite,
    /// A script, by its client's name.
    Script(String),
    /// A connected service – [`SourceKind::Github`], [`SourceKind::Nextcloud`], [`SourceKind::Talk`]
    /// or [`SourceKind::Ntfy`] –, every account of it together, as all mail accounts are one tile.
    Service(SourceKind),
}

impl Origin {
    /// The origin of `item`, from its source.
    pub fn of(item: &Item) -> Origin {
        match item.source.kind {
            SourceKind::Mail => Origin::Mail,
            SourceKind::Agent => Origin::Agents,
            SourceKind::Suite => Origin::Suite,
            SourceKind::Script => Origin::Script(item.source.name.clone()),
            kind @ (SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy) => {
                Origin::Service(kind)
            }
        }
    }

    /// A source of the origin's kind without an account, for its name and icon.
    fn service(kind: SourceKind) -> SourceRef {
        SourceRef { kind, name: String::new() }
    }

    /// The name in the tile and in "Coming back": "Mail", "Agents", "Suite", the script's name, the
    /// service's ("GitHub", "Nextcloud", "Talk", "ntfy").
    pub fn name(&self) -> String {
        match self {
            Origin::Mail => tr("Mail").to_owned(),
            Origin::Agents => tr("Agents").to_owned(),
            Origin::Suite => tr("Suite").to_owned(),
            Origin::Script(name) => list::one_line(name),
            Origin::Service(kind) => Origin::service(*kind).label(),
        }
    }

    /// The icon in the tile, as the sources have them (`list::source_icon`).
    pub fn icon(&self) -> Icon {
        match self {
            Origin::Mail => Icon::Mail,
            Origin::Agents => Icon::Sparkles,
            Origin::Suite => Icon::LayoutGrid,
            Origin::Script(_) => Icon::AppWindow,
            Origin::Service(kind) => list::source_icon(&Origin::service(*kind)),
        }
    }
}

/// The entries of "Act now" by section, each in the order of the app's entries (waiting first,
/// then the newest).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sections<'a> {
    pub decide: Vec<&'a Item>,
    pub follow_up: Vec<&'a Item>,
    pub agents: Vec<&'a Item>,
    pub to_sort: Vec<&'a Item>,
}

impl<'a> Sections<'a> {
    /// The entries of `items` in their sections ([`Section::of`]); with `filter` (a tile clicked)
    /// only those of that origin.
    pub fn of(items: &'a [Item], filter: Option<&Origin>) -> Sections<'a> {
        let mut sections = Sections::default();
        for item in items.iter().filter(|item| filter.is_none_or(|origin| Origin::of(item) == *origin)) {
            match Section::of(item) {
                Some(Section::Decide) => sections.decide.push(item),
                Some(Section::FollowUp) => sections.follow_up.push(item),
                Some(Section::Agents) => sections.agents.push(item),
                Some(Section::ToSort) => sections.to_sort.push(item),
                None => {}
            }
        }
        sections
    }

    /// The entries of `section`.
    pub fn get(&self, section: Section) -> &[&'a Item] {
        match section {
            Section::Decide => &self.decide,
            Section::FollowUp => &self.follow_up,
            Section::Agents => &self.agents,
            Section::ToSort => &self.to_sort,
        }
    }

    /// The sections drawn, with their entries: those with entries, top to bottom; only To sort
    /// while the window shows To sort alone (`only_to_sort`).
    pub fn shown(&self, only_to_sort: bool) -> Vec<(Section, &[&'a Item])> {
        Section::ALL
            .into_iter()
            .filter(|section| !only_to_sort || *section == Section::ToSort)
            .map(|section| (section, self.get(section)))
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    /// The entries in the order the keys walk them: section after section, as drawn.
    pub fn order(&self, only_to_sort: bool) -> Vec<&'a Item> {
        self.shown(only_to_sort).into_iter().flat_map(|(_, items)| items.iter().copied()).collect()
    }

    /// Nothing to act on.
    pub fn is_empty(&self) -> bool {
        self.decide.is_empty() && self.follow_up.is_empty() && self.agents.is_empty() && self.to_sort.is_empty()
    }
}

/// The numbers of the sections that have entries, as `(number, word)`: "2" "to decide", "9" "to
/// sort".
pub fn numbers(sections: &Sections<'_>) -> Vec<(usize, &'static str)> {
    Section::ALL
        .into_iter()
        .map(|section| (sections.get(section).len(), section.word()))
        .filter(|(count, _)| *count > 0)
        .collect()
}

/// The header bar's note beside "Act now": "2 to decide · 3 agents · 9 to sort" ([`numbers`]);
/// empty when there is nothing to act on.
pub fn summary(sections: &Sections<'_>) -> String {
    numbers(sections).into_iter().map(|(count, word)| format!("{count} {word}")).collect::<Vec<_>>().join(" · ")
}

/// One tile over the sections: an origin, how many of its entries are for the user, and one line
/// that says what they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub origin: Origin,
    pub count: usize,
    /// Mail: the newest senders ("From Anna, Ben"); agents: how many wait and how many are done;
    /// anything else: the newest entry's title.
    pub line: String,
}

/// The tiles of `items`: one per origin of the entries in "Act now" ([`Section::of`]), the busiest
/// first (ties in the order of `items`). Unfiltered: a tile stays while its filter is on.
pub fn tiles(items: &[Item]) -> Vec<Tile> {
    let mut by_origin: Vec<(Origin, Vec<&Item>)> = Vec::new();
    for item in items.iter().filter(|item| Section::of(item).is_some()) {
        let origin = Origin::of(item);
        match by_origin.iter_mut().find(|(known, _)| *known == origin) {
            Some((_, entries)) => entries.push(item),
            None => by_origin.push((origin, vec![item])),
        }
    }
    by_origin.sort_by_key(|(_, entries)| std::cmp::Reverse(entries.len()));
    by_origin
        .into_iter()
        .map(|(origin, entries)| Tile { line: tile_line(&origin, &entries), count: entries.len(), origin })
        .collect()
}

/// The line of a tile of `origin` over its `entries` ([`Tile::line`]).
fn tile_line(origin: &Origin, entries: &[&Item]) -> String {
    match origin {
        Origin::Mail => {
            let owned: Vec<Item> = entries.iter().map(|item| (*item).clone()).collect();
            let senders = glance::latest_senders(&owned);
            if senders.is_empty() { String::new() } else { glance::senders_text(&senders) }
        }
        Origin::Agents => {
            let waiting = entries.iter().filter(|item| item.kind == Kind::AgentWaiting).count();
            let finished = entries.len() - waiting;
            let mut parts = Vec::new();
            if waiting > 0 {
                parts.push(trf("{n} waiting", &[("n", &waiting)]));
            }
            if finished > 0 {
                parts.push(trf("{n} finished", &[("n", &finished)]));
            }
            parts.join(" · ")
        }
        Origin::Suite | Origin::Script(_) | Origin::Service(_) => {
            entries.iter().max_by_key(|item| item.updated).map(|item| list::words(item).0).unwrap_or_default()
        }
    }
}

/// The narrowest a tile gets: two thirds of the properties column, room for a name, the number and
/// a short line.
fn tile_min() -> f32 {
    size::props() * 2.0 / 3.0
}

/// How many tiles fit in one row `width` wide, [`size::card_gap()`] apart; at least one.
pub fn tile_columns(width: f32) -> usize {
    let gap = size::card_gap();
    let count = ((width + gap) / (tile_min() + gap)).floor();
    if count >= 1.0 { count as usize } else { 1 }
}

/// What a button of the hub does (the concept's table "Knöpfe je Eintrag").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Allow the approval with this id (`approvals.decide_approval`).
    Allow(String),
    /// Deny the approval with this id.
    Deny(String),
    /// Bring the agent's window to the front (`inbox.open_item`).
    GoToWindow,
    /// Open the page or the app the entry links to (`inbox.open_item`).
    Open,
    /// Make a task of the entry (`t`, `inbox.create_task_from_item`).
    Task,
    /// Make an event of the entry (`c`, `inbox.create_event_from_item`, task E8.5b).
    Event,
    /// Put it off (`s`, `inbox.snooze_item`).
    Later,
    /// Mark it done (`e`, `inbox.set_item_done`).
    Done,
    /// Archive the mail of a mail entry (`e`, task E18.5, decision 5 of E18): `mail.move_messages` to
    /// the archive, then the entry closed – what `e` does at the entry (`Command::Done`).
    Archive,
    /// Let it go without doing anything (`x`, `inbox.set_item_done` with `dismissed`).
    Dismiss,
    /// Open a done entry again (`e`).
    Reopen,
    /// Answer the mail a finished task was made of (`r`, task E8.7b): the compose window for the mail
    /// of [`torro_inbox::item::ItemTask::mail_entry`], else the mail window with it.
    Reply,
    /// Archive the mail of a finished task and close its entry and the question (task E18.5,
    /// `Message::ArchiveMail`).
    ArchiveMail,
    /// Close the question without touching the mail (`e` on it).
    NotNow,
}

/// Whether `item` is a coding agent's: waiting, or done with its turn.
fn is_agent(item: &Item) -> bool {
    matches!(item.kind, Kind::AgentWaiting | Kind::AgentDone)
}

/// The buttons of `item` from the left, as the concept's table has them:
///
/// - an approval: Allow · Deny, and Go to window when its agent's window can be opened – **no ✕, no
///   Later, no Done**: it waits for yes or no;
/// - a done entry: Reopen;
/// - a coding agent: Go to window (when it can be opened) · Later · Done;
/// - mail: Task · Event · Later · Archive – mail is archived or hidden, never done nor dismissed
///   (task E18.5);
/// - the suite's "You finished a task from a mail" ([`is_follow_up`], task E8.7b): Reply · Archive
///   mail · Not now;
/// - everything else: Open (a page or an app, when [`detail::openable`] offers it) · Task · Event ·
///   Later · ✕ ([`notifications::dismissable`], else Done).
pub fn acts(item: &Item) -> Vec<Act> {
    if is_follow_up(item) && item.state.is_open() {
        return vec![Act::Reply, Act::ArchiveMail, Act::NotNow];
    }
    let window = detail::openable(item) == Some(Link::Window);
    if detail::is_approval(item) {
        let mut acts: Vec<Act> = item
            .actions
            .iter()
            .map(|action| match action {
                Action::Allow { approval } => Act::Allow(approval.clone()),
                Action::Deny { approval } => Act::Deny(approval.clone()),
            })
            .collect();
        if window {
            acts.push(Act::GoToWindow);
        }
        return acts;
    }
    if matches!(item.state, State::Done { .. }) {
        return vec![Act::Reopen];
    }
    if is_agent(item) {
        let lead = window.then_some(Act::GoToWindow);
        return lead.into_iter().chain([Act::Later, Act::Done]).collect();
    }
    if item.source.kind == SourceKind::Mail {
        return vec![Act::Task, Act::Event, Act::Later, Act::Archive];
    }
    let mut acts = Vec::new();
    match detail::openable(item) {
        Some(Link::Url { .. } | Link::App { .. }) => acts.push(Act::Open),
        Some(Link::Window) => acts.push(Act::GoToWindow),
        Some(Link::Mail { .. }) | None => {}
    }
    acts.extend([Act::Task, Act::Event, Act::Later]);
    acts.push(if notifications::dismissable(item) { Act::Dismiss } else { Act::Done });
    acts
}

impl Act {
    /// The button's icon.
    pub fn icon(&self) -> Icon {
        match self {
            Act::Allow(_) | Act::Done => Icon::Check,
            Act::Archive | Act::ArchiveMail => Icon::Archive,
            Act::Deny(_) | Act::Dismiss | Act::NotNow => Icon::X,
            Act::GoToWindow => Icon::AppWindow,
            Act::Open => Icon::ArrowUpRight,
            Act::Task => Icon::ListTodo,
            Act::Event => Icon::CalendarPlus,
            Act::Later => Icon::Clock,
            Act::Reopen => Icon::Inbox,
            Act::Reply => Icon::Reply,
        }
    }

    /// The button's word (its tooltip when it shows as icon).
    pub fn label(&self) -> &'static str {
        match self {
            Act::Allow(_) => tr("Allow"),
            Act::Deny(_) => tr("Deny"),
            Act::GoToWindow => tr("Go to window"),
            Act::Open => tr("Open"),
            Act::Task => tr("Task"),
            Act::Event => tr("Event"),
            Act::Later => tr("Later"),
            Act::Done => tr("Done"),
            Act::Archive => trc("action", "Archive"),
            Act::Dismiss => tr("Dismiss"),
            Act::Reopen => tr("Reopen"),
            Act::Reply => tr("Reply"),
            Act::ArchiveMail => tr("Archive mail"),
            Act::NotNow => tr("Not now"),
        }
    }

    /// The key that does the same at the entry, for the action bar's tooltip; Allow and Deny have
    /// none (they stand on cards, never in an action bar).
    fn key(&self) -> &'static str {
        match self {
            Act::Allow(_) | Act::Deny(_) | Act::ArchiveMail => "",
            Act::GoToWindow | Act::Open => "Enter",
            Act::Task => "T",
            Act::Event => "C",
            Act::Later => "S",
            Act::Done | Act::Archive | Act::Reopen | Act::NotNow => "E",
            Act::Dismiss => "X",
            Act::Reply => "R",
        }
    }

    /// What the button sends for `item`; `None` (disabled) for Allow and Deny while a decision is on
    /// its way (`deciding`) or the approval is closed.
    pub fn message(&self, item: &Item, deciding: bool) -> Option<Message> {
        let act = |command| Some(Message::Act(item.id.clone(), command));
        let decide = |approval: &String, allow| {
            (item.state.is_open() && !deciding).then(|| Message::Decide { approval: approval.clone(), allow })
        };
        match self {
            Act::Allow(approval) => decide(approval, true),
            Act::Deny(approval) => decide(approval, false),
            Act::GoToWindow | Act::Open => Some(Message::OpenRow(item.id.clone())),
            Act::Task => act(Command::MakeTask),
            Act::Event => act(Command::MakeEvent),
            Act::Later => act(Command::Snooze),
            Act::Done | Act::Archive | Act::Reopen | Act::NotNow => act(Command::Done),
            Act::Dismiss => act(Command::Dismiss),
            Act::Reply => act(Command::Reply),
            Act::ArchiveMail => Some(Message::ArchiveMail(item.id.clone())),
        }
    }

    /// The button's look, one rule for every card of the hub and the notifications window (task
    /// E8.12, finding 12, `notifications::button_look`): quiet (`ghost`) for what only lets the
    /// entry go – Done, Archive, Dismiss, Not now –, framed (`secondary`) for every other visible
    /// button.
    /// The narrow layout's big Allow and Deny ([`big_decide`]) are for decisions only.
    fn style(&self) -> ButtonStyle {
        notifications::button_look(self.lets_go())
    }

    /// Whether the button only lets the entry go – Done, Archive (task E18.5: as quiet as Done, whose
    /// place it takes), Dismiss, Not now –, which is drawn quietly ([`Act::style`]).
    fn lets_go(&self) -> bool {
        match self {
            Act::Done | Act::Archive | Act::Dismiss | Act::NotNow => true,
            Act::Allow(_)
            | Act::Deny(_)
            | Act::GoToWindow
            | Act::Open
            | Act::Task
            | Act::Event
            | Act::Later
            | Act::Reopen
            | Act::Reply
            | Act::ArchiveMail => false,
        }
    }

    /// Whether the button is the action the entry is about – Open, Go to window, Reply –, which as
    /// the first of its bar may keep its word while the others show icons (`widgets::BarWords`).
    fn leads(&self) -> bool {
        matches!(self, Act::Open | Act::GoToWindow | Act::Reply)
    }
}

/// The card button of `act` for `item` (its icon, word, message, look and whether it leads), as the
/// hub's cards and the notifications window's draw it.
pub fn card_button(act: &Act, item: &Item, deciding: bool) -> notifications::CardButton {
    notifications::CardButton {
        icon: act.icon(),
        label: act.label().to_owned(),
        on_press: act.message(item, deciding),
        lead: act.leads(),
        look: act.style(),
    }
}

/// How "Act now" lays itself out at a width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The narrow layout: the numbers instead of the tiles, one card per line, big Allow and Deny,
    /// To sort's buttons under its cards.
    pub narrow: bool,
    /// Agents and To sort as cards (the comfortable density); rows with the action bar in the
    /// compact one ([`as_rows`]). Decide is cards in both.
    pub cards: bool,
    /// Cards of Decide and Agents per line.
    pub columns: usize,
    /// Tiles in their row; none in the narrow layout.
    pub tiles: usize,
    /// The window shows To sort alone (its sidebar entry): no tiles, no numbers, no other section.
    pub only_to_sort: bool,
    /// A line about quiet under the tiles or the numbers ([`quiet_line`], task E8.5b): the app sets
    /// it while it is quiet and no column at the right says so. Never with [`Layout::only_to_sort`].
    pub quiet: bool,
}

/// The layout of "Act now" in a window of `class` whose content is `width` wide (beside sidebar,
/// rail, panel and "Coming back", without the hub's own inset), in `density`. Mini and strip show
/// the glance; they count as narrow here.
pub fn layout(width: f32, class: SizeClass, density: Density, only_to_sort: bool) -> Layout {
    let narrow = !matches!(class, SizeClass::Wide | SizeClass::Medium);
    let width = width.min(max_width() - sides(narrow));
    Layout {
        narrow,
        cards: density == Density::Comfortable,
        columns: if narrow { 1 } else { notifications::fit(width) },
        tiles: if narrow { 0 } else { tile_columns(width) },
        only_to_sort,
        quiet: false,
    }
}

/// The widest "Act now" gets, its inset included: two reading widths – a wide window is not
/// stretched; the room left stays free at the right.
pub fn max_width() -> f32 {
    2.0 * size::content_max()
}

/// The hub's inset around its content: a page's ([`layout::page_padding()`]), a panel's in the
/// narrow layout, where every point of width counts.
pub fn inset(narrow: bool) -> Padding {
    if narrow { layout::panel_padding() } else { layout::page_padding() }
}

/// What [`inset`] takes from the width.
pub fn sides(narrow: bool) -> f32 {
    let inset = inset(narrow);
    inset.left + inset.right
}

/// The cards of `section` per line: the layout's columns for Agents; for Decide and Follow up – a
/// question's title and what it is about need the room, and a decision should not look like one of
/// many – the full width where only two cards fit side by side (medium, task E8.12, finding 5), two
/// where three would.
fn per_line(section: Section, layout: Layout) -> usize {
    let columns = layout.columns.max(1);
    if !matches!(section, Section::Decide | Section::FollowUp) {
        return columns;
    }
    if pairs(layout) { 2 } else { 1 }
}

/// Whether Decide and Follow up stand side by side, each a column of its cards (task E8.12, finding
/// 5: wide windows put them next to each other instead of half wide one under the other): where
/// three cards fit in a line.
fn pairs(layout: Layout) -> bool {
    !layout.narrow && layout.columns >= 3
}

/// The sections drawn, as blocks one under the other: each a section of its own, but Decide and
/// Follow up side by side in one block where both have entries and [`pairs`] says so.
fn section_blocks<'s, 'a>(sections: &'s Sections<'a>, layout: Layout) -> Vec<Vec<(Section, &'s [&'a Item])>> {
    let shown = sections.shown(layout.only_to_sort);
    let both = shown.iter().any(|(section, _)| *section == Section::Decide)
        && shown.iter().any(|(section, _)| *section == Section::FollowUp);
    let mut blocks: Vec<Vec<(Section, &'s [&'a Item])>> = Vec::new();
    for (section, items) in shown {
        let joins = both && pairs(layout) && section == Section::FollowUp;
        match blocks.last_mut() {
            Some(block) if joins => block.push((section, items)),
            _ => blocks.push(vec![(section, items)]),
        }
    }
    blocks
}

/// The cards of `section` per line in a block of `width` sections side by side: one when it shares
/// its line ([`section_blocks`]), else [`per_line`].
fn per_line_in(section: Section, layout: Layout, width: usize) -> usize {
    if width > 1 { 1 } else { per_line(section, layout) }
}

/// Whether `section` has its caption over it: always, but To sort while the window shows it alone –
/// the header's "To sort 9" says it already (task E8.12).
fn has_caption(section: Section, layout: Layout) -> bool {
    !(layout.only_to_sort && section == Section::ToSort)
}

/// The height of `section`'s caption with the gap under it, nothing without one ([`has_caption`]).
fn caption_height(section: Section, layout: Layout) -> f32 {
    if has_caption(section, layout) { typo::line_height(typo::caption()) + caption_gap() } else { 0.0 }
}

/// Whether `section` shows its entries as rows with the action bar: in the compact density, all but
/// Decide and Follow up – an approval and the question after a finished task keep their cards, so
/// their buttons are always there.
fn as_rows(section: Section, layout: Layout) -> bool {
    !layout.cards && !matches!(section, Section::Decide | Section::FollowUp)
}

/// The gap between the blocks of "Act now" (tiles, sections).
fn block_gap(narrow: bool) -> f32 {
    if narrow { space::l() } else { layout::section_gap() }
}

/// The gap between a section's caption and its cards.
fn caption_gap() -> f32 {
    space::m()
}

/// The height of a tile: its inset, the name, the number and its line, [`space::xs()`] apart.
fn tile_height() -> f32 {
    let pad = layout::panel_padding();
    let lines = 2.0 * typo::line_height(typo::meta()) + typo::line_height(typo::display());
    pad.top + pad.bottom + lines + 2.0 * space::xs()
}

/// The height of a card's head: its tile, the title over the meta line, or the state's pill.
fn head_height() -> f32 {
    size::tile().max(typo::line_height(typo::body_strong()) + typo::line_height(typo::meta()))
}

/// The height the text of a card always takes, so a card's height never depends on its width.
fn text_height() -> f32 {
    TEXT_LINES as f32 * typo::line_height(typo::body())
}

/// The height of the buttons of `item`'s card: a row of buttons; in the narrow layout an
/// approval's big Allow and Deny, with "Go to window" below when it has one.
fn buttons_height(item: &Item, narrow: bool) -> f32 {
    if !(narrow && detail::is_approval(item)) {
        return size::control_sm();
    }
    let window = acts(item).contains(&Act::GoToWindow);
    size::control_lg() + if window { space::s() + size::control_sm() } else { 0.0 }
}

/// The height of the card of `item` in Decide or Agents: the card's inset, the head, the text when
/// there is one and the buttons, [`size::card_gap()`] between them.
pub fn card_height(item: &Item, narrow: bool) -> f32 {
    let pad = layout::panel_padding();
    let gap = size::card_gap();
    let text = if notifications::card_text(item).is_some() { text_height() + gap } else { 0.0 };
    pad.top + pad.bottom + head_height() + gap + text + buttons_height(item, narrow)
}

/// The height of `item` in To sort (and in Agents, while they are rows): one line of a card with its
/// buttons at the right, a card with its buttons below (narrow), or its row (compact).
pub fn sort_height(item: &Item, layout: Layout) -> f32 {
    let pad = layout::panel_padding();
    let lead = if item.source.kind == SourceKind::Mail { size::avatar() } else { size::tile_lg() };
    match (layout.cards, layout.narrow) {
        (false, _) => list::row_height_of(item),
        (true, false) => pad.top + pad.bottom + head_height().max(lead),
        (true, true) => pad.top + pad.bottom + head_height().max(lead) + size::card_gap() + size::control_sm(),
    }
}

/// The gap between the entries of To sort: the cards' gap, none between rows (they have lines).
fn sort_gap(layout: Layout) -> f32 {
    if layout.cards { size::card_gap() } else { 0.0 }
}

/// Where the card or row of `id` lies in the scrolling content of "Act now" with `sections` laid out
/// as `layout`, as `(top, bottom)`; the first of a section starts at its caption, so scrolling up to
/// it shows the caption. `None` for an entry not shown. Counts what [`view`] draws: the inset, the
/// tiles or the numbers, then section after section.
pub fn span(sections: &Sections<'_>, tiles: usize, layout: Layout, id: &str) -> Option<(f32, f32)> {
    let gap = block_gap(layout.narrow);
    let mut y = inset(layout.narrow).top;
    if !layout.only_to_sort {
        if layout.narrow && !numbers(sections).is_empty() {
            y += typo::line_height(typo::display()) + gap;
        } else if !layout.narrow && tiles > 0 {
            y += tile_height() + gap;
        }
        if layout.quiet {
            y += quiet_height() + gap;
        }
    }
    for block in section_blocks(sections, layout) {
        // Sections side by side start at the same height; the block ends under the longer one.
        let mut bottom = y;
        for &(section, items) in &block {
            match section_span(section, items, layout, per_line_in(section, layout, block.len()), y, id) {
                Ok(found) => return Some(found),
                Err(end) => bottom = bottom.max(end),
            }
        }
        y = bottom + gap;
    }
    None
}

/// [`span`] within one section starting at `top` with `per_line` cards a line: `Ok` with the span of
/// `id` when it is there (the first entry from the caption on), else `Err` with where it ends.
fn section_span(
    section: Section,
    items: &[&Item],
    layout: Layout,
    per_line: usize,
    top: f32,
    id: &str,
) -> Result<(f32, f32), f32> {
    let mut y = top + caption_height(section, layout);
    if section == Section::ToSort || as_rows(section, layout) {
        if as_rows(section, layout) {
            y += size::HAIRLINE;
        }
        for (i, item) in items.iter().enumerate() {
            let height = sort_height(item, layout);
            if item.id == id {
                return Ok((if i == 0 { top } else { y }, y + height));
            }
            y += height + sort_gap(layout);
        }
        return Err(y - sort_gap(layout));
    }
    for (i, line) in items.chunks(per_line.max(1)).enumerate() {
        let tallest = line.iter().map(|item| card_height(item, layout.narrow)).fold(0.0, f32::max);
        if line.iter().any(|item| item.id == id) {
            return Ok((if i == 0 { top } else { y }, y + tallest));
        }
        y += tallest + size::card_gap();
    }
    Err(y - size::card_gap())
}

/// The height of the line about quiet ([`quiet_line`]): one line of `BODY`, its icon no taller.
fn quiet_height() -> f32 {
    typo::line_height(typo::body()).max(icon::SIZE)
}

/// The entries of `snoozed` that come back, the soonest first ("Coming back").
pub fn coming_back(snoozed: &[Item]) -> Vec<&Item> {
    let mut back: Vec<&Item> = snoozed.iter().filter(|item| matches!(item.state, State::Snoozed { .. })).collect();
    back.sort_by_key(|item| match item.state {
        State::Snoozed { until } => Some(until),
        State::New | State::Seen | State::Done { .. } => None,
    });
    back
}

/// What "Act now" gets to draw.
pub struct HubData<'a> {
    /// The open entries, in the app's order (waiting first, then the newest).
    pub items: &'a [Item],
    /// The tile clicked, whose entries alone show; `None` for all.
    pub filter: Option<&'a Origin>,
    pub layout: Layout,
    /// The cursor, the time, the anchor for the menus, the mail accounts (`list::Rows`).
    pub rows: Rows<'a>,
    /// A decision of an approval is on its way: its Allow and Deny are disabled.
    pub deciding: bool,
    /// The theme's attention colour and its colour for "OK", for the states' pills.
    pub attention: Color,
    pub ok: Color,
    /// Whether it is quiet now (task E8.5b), for the line of [`Layout::quiet`].
    pub quiet: Option<&'a Quiet>,
}

/// "Act now": the empty state, or the tiles (the numbers in narrow windows) and the sections –
/// scrolling as one ([`list::SCROLL`], so the app keeps the cursor in view), capped at
/// [`max_width`].
pub fn view<'a>(data: HubData<'a>) -> Element<'a, Message> {
    let sections = Sections::of(data.items, data.filter);
    if sections.is_empty() && data.filter.is_none() {
        let (title, hint) = if data.layout.only_to_sort {
            (tr("Nothing to sort."), tr("Mail and notices for you show up here."))
        } else {
            (tr("Nothing to act on."), tr("Approvals, waiting agents and entries to sort show up here."))
        };
        return list::empty_state(Icon::Layers, title, hint);
    }
    let content = responsive(move |bounds| {
        let width = (bounds.width - sides(data.layout.narrow)).max(0.0);
        let content = blocks(&data, width);
        scrollable(container(content).padding(inset(data.layout.narrow)))
            .id(list::SCROLL)
            .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    });
    container(content).width(Fill).max_width(max_width()).height(Fill).into()
}

/// The blocks of "Act now" `width` wide (inside the inset): the tiles or the numbers, then every
/// section with entries.
fn blocks<'a>(data: &HubData<'a>, width: f32) -> Element<'a, Message> {
    let sections = Sections::of(data.items, data.filter);
    let layout = data.layout;
    let mut blocks: Vec<Element<'a, Message>> = Vec::new();
    if !layout.only_to_sort {
        if layout.narrow {
            let numbers = numbers(&sections);
            if !numbers.is_empty() {
                blocks.push(numbers_line(numbers));
            }
        } else {
            let tiles = tiles(data.items);
            if !tiles.is_empty() {
                blocks.push(tile_row(tiles, layout.tiles, data.filter, width));
            }
        }
        if let Some(quiet) = data.quiet.filter(|_| layout.quiet) {
            blocks.push(quiet_view(quiet_line(quiet, data.rows.now), width));
        }
    }
    for block in section_blocks(&sections, layout) {
        let side_by_side = block.len();
        let gap = size::card_gap();
        let room = (width - (side_by_side as f32 - 1.0) * gap) / side_by_side as f32;
        let parts: Vec<Element<'a, Message>> = block
            .into_iter()
            .map(|(section, items)| {
                let per_line = per_line_in(section, layout, side_by_side);
                container(section_view(data, section, items, per_line, room)).width(Fill).into()
            })
            .collect();
        blocks.push(if side_by_side > 1 { row(parts).spacing(gap).into() } else { column(parts).into() });
    }
    column(blocks).spacing(block_gap(layout.narrow)).width(Fill).into()
}

/// One section `width` wide with `per_line` cards a line: its caption ([`has_caption`]), then its
/// rows, its To sort cards (at most [`size::content_max()`] wide outside the narrow layout, task
/// E8.12, finding 8) or its grid of cards.
fn section_view<'a>(
    data: &HubData<'a>,
    section: Section,
    items: &[&'a Item],
    per_line: usize,
    width: f32,
) -> Element<'a, Message> {
    let layout = data.layout;
    let sorting = section == Section::ToSort;
    // A wide window does not stretch To sort: one card per line reads best at the reading width.
    let width = if sorting && !layout.narrow { width.min(size::content_max()) } else { width };
    let body: Element<'a, Message> = if as_rows(section, layout) {
        let rows = items.iter().map(|item| entry_row(item, data, width));
        column![rule::horizontal(size::HAIRLINE).style(style::rule), column(rows)].width(Fill).into()
    } else if sorting {
        let slots = SortSlots::of(items, layout.narrow, width);
        let cards = items.iter().map(|item| sort_card(item, data, width, &slots));
        column(cards).spacing(sort_gap(layout)).width(Fill).into()
    } else {
        grid(items.iter().map(|item| card(item, data)).collect(), per_line)
    };
    let mut parts = column![].spacing(caption_gap());
    if has_caption(section, layout) {
        parts = parts.push(caption(section.title(), items.len()));
    }
    let parts = parts.push(body);
    if sorting && !layout.narrow { container(parts).max_width(size::content_max()).into() } else { parts.into() }
}

/// The line about quiet `width` wide: the moon and the words in `text_2`, cut with "…".
fn quiet_view<'a>(line: String, width: f32) -> Element<'a, Message> {
    let mark = icon::icon(Icon::Moon, icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let words = typo::ellipsize(&line, typo::body(), (width - icon::SIZE - space::s()).max(0.0));
    let words = typo::text(typo::body(), words).style(theme::text_2).wrapping(Wrapping::None);
    container(row![mark, words].spacing(space::s()).align_y(Alignment::Center)).height(quiet_height()).into()
}

/// The caption over a section: "Decide · 2" in `CAPTION` and `text_3`, sentence case.
fn caption<'a>(label: &str, count: usize) -> Element<'a, Message> {
    // No "0": an empty "Coming back" says so in its line below.
    let words = if count > 0 { format!("{label} · {count}") } else { label.to_owned() };
    typo::text(typo::caption(), words).style(theme::text_3).wrapping(Wrapping::None).into()
}

/// `cells` in lines of `columns`, [`size::card_gap()`] apart; the last line filled up with room, so
/// its cells keep the width of the others.
fn grid<'a>(cells: Vec<Element<'a, Message>>, columns: usize) -> Element<'a, Message> {
    let columns = columns.max(1);
    let gap = size::card_gap();
    let mut lines = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let mut line: Vec<Element<'a, Message>> =
            cells.by_ref().take(columns).map(|cell| container(cell).width(Fill).into()).collect();
        let filler = columns - line.len();
        line.extend((0..filler).map(|_| Space::new().width(Fill).into()));
        lines.push(row(line).spacing(gap).into());
    }
    column(lines).spacing(gap).width(Fill).into()
}

/// The numbers of the narrow layout: each section's number in `DISPLAY`, its word beside it.
fn numbers_line<'a>(numbers: Vec<(usize, &'static str)>) -> Element<'a, Message> {
    let parts = numbers.into_iter().map(|(count, word)| {
        row![
            typo::text(typo::display().tabular(), count.to_string()).style(theme::text).wrapping(Wrapping::None),
            typo::text(typo::body(), word).style(theme::text_2).wrapping(Wrapping::None),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    });
    row(parts).spacing(space::xl()).align_y(Alignment::Center).into()
}

/// The row of tiles `width` wide: the first `columns` of `tiles`, the chosen one (`filter`) drawn
/// selected.
fn tile_row<'a>(tiles: Vec<Tile>, columns: usize, filter: Option<&Origin>, width: f32) -> Element<'a, Message> {
    let columns = columns.max(1);
    let gap = size::card_gap();
    let pad = layout::panel_padding();
    let inner = ((width - (columns as f32 - 1.0) * gap) / columns as f32 - pad.left - pad.right).max(0.0);
    let cells = tiles.into_iter().take(columns).map(|tile| {
        let chosen = filter == Some(&tile.origin);
        tile_view(tile, chosen, inner)
    });
    let mut line: Vec<Element<'a, Message>> = cells.map(|cell| container(cell).width(Fill).into()).collect();
    let filler = columns.saturating_sub(line.len());
    line.extend((0..filler).map(|_| Space::new().width(Fill).into()));
    row(line).spacing(gap).into()
}

/// One tile, its text `inner` wide: the origin's icon and name, the number in `DISPLAY`, the line –
/// a card (`widgets::card`), whose click filters the sections to the origin.
fn tile_view<'a>(tile: Tile, chosen: bool, inner: f32) -> Element<'a, Message> {
    let mark = icon::icon(tile.origin.icon(), icon::SIZE_SM)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) });
    let name = typo::ellipsize(&tile.origin.name(), typo::meta(), inner - icon::SIZE_SM - space::s());
    let head = row![mark, typo::text(typo::meta(), name).style(theme::text_3).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let figure =
        typo::text(typo::display().tabular(), tile.count.to_string()).style(theme::text).wrapping(Wrapping::None);
    let line = typo::ellipsize(&tile.line, typo::meta(), inner);
    let line = typo::text(typo::meta(), line).style(theme::text_3).wrapping(Wrapping::None);
    let words = column![head, figure, container(line).clip(true)].spacing(space::xs());
    widgets::card(vec![words.into()], chosen, Some(Message::Filter(tile.origin)))
}

/// The meta line of a card of Decide or Agents: the agent (an approval says none – the suite asks),
/// the number of reports from two on, and the time.
fn card_meta(item: &Item, rows: &Rows<'_>) -> String {
    let mut parts = Vec::new();
    if is_agent(item) {
        parts.push(item.source.label());
    }
    if item.count > 1 {
        parts.push(trf("{n} reports", &[("n", &item.count)]));
    }
    parts.push(list::time_text_in(item, rows.view, rows.now, rows.bare));
    parts.join(" · ")
}

/// The pill at the right of a card's head: "Waiting" while the entry waits, "Finished" for an agent
/// done with its turn; colour only in its icon.
fn state<'a>(item: &Item, data: &HubData<'_>) -> Option<Element<'a, Message>> {
    if list::is_waiting(item) {
        return Some(widgets::state_pill(Mark::Tinted(Icon::CircleDotDashed, data.attention), tr("Waiting")));
    }
    (item.kind == Kind::AgentDone)
        .then(|| widgets::state_pill(Mark::Tinted(Icon::CircleCheck, data.ok), tr("Finished")))
}

/// The card of `item` in Decide or Agents, as high as [`card_height`]: the head (the source's tile,
/// the title, the meta line, the state), the text on [`TEXT_LINES`] lines, the buttons ([`acts`]) –
/// in the narrow layout an approval's big Allow and Deny. A click opens it in the panel; under the
/// cursor it is selected and registers the anchor.
fn card<'a>(item: &'a Item, data: &HubData<'a>) -> Element<'a, Message> {
    let selected = data.rows.selected == Some(item.id.as_str());
    let (title, _) = list::words(item);
    let mut head = row![widgets::card_head(Some(list::source_tile(item)), title, card_meta(item, &data.rows), None)]
        .spacing(space::m())
        .align_y(Alignment::Center);
    if let Some(pill) = state(item, data) {
        head = head.push(pill);
    }
    let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
    if let Some(text) = notifications::card_text(item) {
        let lines = responsive(move |bounds| {
            let lines = list::clamp_lines(&text, typo::body(), bounds.width, TEXT_LINES)
                .into_iter()
                .map(|line| typo::text(typo::body(), line).style(theme::text_2).wrapping(Wrapping::None).into());
            column(lines).into()
        });
        parts.push(container(lines).width(Fill).height(text_height()).clip(true).into());
    }
    let acts = acts(item);
    if data.layout.narrow && detail::is_approval(item) {
        parts.push(big_decide(item, &acts, data.deciding));
    } else {
        parts.push(buttons(item, acts, data.deciding, (data.rows.anchors.clone(), selected)));
    }
    let card = widgets::card(parts, selected, Some(Message::Select(item.id.clone())));
    if selected { widgets::anchored(&data.rows.anchors.row, card) } else { card }
}

/// The buttons of `item` in one row, from the left: all with their words where they fit, else
/// icons with the word as tooltip – only the lead (what the entry is about) may keep its word alone
/// (`notifications::worded`, task E8.12). Under the cursor (the `bool` of `at`) each button that
/// opens a menu registers its anchor (issue #122, `detail::Anchors::wrap`).
fn buttons<'a>(item: &Item, acts: Vec<Act>, deciding: bool, at: (detail::Anchors, bool)) -> Element<'a, Message> {
    let pressed: Vec<notifications::CardButton> = acts.iter().map(|act| card_button(act, item, deciding)).collect();
    let (anchors, shown) = at;
    let line = responsive(move |bounds| {
        let words = notifications::worded(&pressed, false, bounds.width);
        let line = pressed.iter().enumerate().map(|(i, button)| {
            let element = notifications::card_button(button.clone(), words.worded(i));
            anchors.wrap(shown, button.on_press.as_ref(), element)
        });
        row(line).spacing(space::s()).align_y(Alignment::Center).into()
    });
    container(line).width(Fill).height(size::control_sm()).into()
}

/// The narrow layout's buttons of an approval: Allow and Deny side by side, each half the card wide
/// and [`size::control_lg()`] high, "Go to window" below them when the agent's window can be opened.
fn big_decide<'a>(item: &Item, acts: &[Act], deciding: bool) -> Element<'a, Message> {
    let big = acts.iter().filter(|act| matches!(act, Act::Allow(_) | Act::Deny(_))).map(|act| {
        let mark = icon::icon(act.icon(), icon::SIZE_SM)
            .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text) });
        let words = row![mark, typo::text(typo::caption(), act.label()).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center);
        button(container(words).center_x(Fill).center_y(Fill))
            .width(Fill)
            .height(size::control_lg())
            .padding(style::button_padding())
            .style(style::button::secondary)
            .on_press_maybe(act.message(item, deciding))
            .into()
    });
    let mut block = column![row(big).spacing(space::s())].spacing(space::s());
    if acts.contains(&Act::GoToWindow) {
        let window = Act::GoToWindow;
        block = block.push(widgets::text_button(
            Some(window.icon()),
            window.label(),
            style::button::ghost,
            window.message(item, deciding),
        ));
    }
    block.into()
}

/// The meta line of an entry of To sort: the sender of a mail, else the source; "Urgent" or "Waiting
/// for you" while it waits; what a service's traits say ("torro/workplace · Review requested",
/// `list::facet_line`, task E5.15); the number of reports from two on; the time.
fn sort_meta(item: &Item, rows: &Rows<'_>) -> String {
    let who = if item.source.kind == SourceKind::Mail { list::sender(item).0 } else { item.source.label() };
    let mut parts = vec![who];
    if list::is_waiting(item) {
        parts.push(list::waiting_word(item).to_owned());
    }
    parts.extend(list::facet_line(item));
    if item.count > 1 {
        parts.push(trf("{n} reports", &[("n", &item.count)]));
    }
    parts.push(list::time_text_in(item, rows.view, rows.now, rows.bare));
    parts.join(" · ")
}

/// What leads an entry of To sort: the sender's avatar for mail, else the source's tile – tinted in
/// `attention` while the entry waits (an ntfy alert, task E5.15), so urgency shows without Torro red
/// and, with the meta line's word, not by colour alone.
fn lead<'a>(item: &Item, rows: &Rows<'_>, attention: Color) -> Element<'a, Message> {
    if item.source.kind == SourceKind::Mail {
        let (name, _) = list::sender(item);
        let account = list::account_index(item, &rows.accounts);
        return list::avatar(&name, account, size::avatar());
    }
    let tint = list::is_waiting(item).then_some(attention);
    widgets::source_tile(TileFace::Icon(list::source_icon(&item.source)), TileSize::Large, tint)
}

/// Where an act of To sort stands in the fixed columns of its buttons ([`SortSlots`]): what the
/// entry is about (Open, Go to window), Task, Event, Later, and what lets it go (Done, Archive,
/// Dismiss).
fn sort_slot(act: &Act) -> usize {
    match act {
        Act::Open | Act::GoToWindow => 0,
        Act::Task => 1,
        Act::Event => 2,
        Act::Later => 3,
        Act::Allow(_)
        | Act::Deny(_)
        | Act::Done
        | Act::Archive
        | Act::Dismiss
        | Act::Reopen
        | Act::Reply
        | Act::ArchiveMail
        | Act::NotNow => 4,
    }
}

/// How many fixed columns the buttons of To sort have ([`sort_slot`]).
const SORT_SLOTS: usize = 5;

/// The fixed columns of the buttons of To sort in the one-line cards (task E8.12, finding 8): every
/// card puts Task under Task and Later under Later, whatever else it offers. A column is there when
/// an entry of the section has an act in it, as wide as its widest button; one choice of words or
/// icons holds for the whole section, so the columns line up.
#[derive(Debug, Clone, PartialEq)]
pub struct SortSlots {
    /// Per column ([`SORT_SLOTS`]) its width, `None` where no entry has a button there.
    pub widths: [Option<f32>; SORT_SLOTS],
    /// The buttons show their words (all of them, `widgets::BarWords::All`), else icons only.
    pub words: bool,
}

impl SortSlots {
    /// The columns of `items` in cards `width` wide: with words when the widest buttons of every
    /// column fit beside the lead and at least half the properties column for title and meta, else
    /// icons ([`sort_buttons_room`]). The `narrow` layout has no columns: its buttons stand under
    /// the card's head.
    pub fn of(items: &[&Item], narrow: bool, width: f32) -> SortSlots {
        let mut widest: [Option<&'static str>; SORT_SLOTS] = [None; SORT_SLOTS];
        for act in items.iter().flat_map(|item| acts(item)) {
            let slot = &mut widest[sort_slot(&act)];
            let label = act.label();
            let wider = slot
                .is_none_or(|known| widgets::text_button_width(true, label) > widgets::text_button_width(true, known));
            if wider {
                *slot = Some(label);
            }
        }
        let labels: Vec<&str> = widest.iter().flatten().copied().collect();
        let words = !narrow && widgets::bar_words(&labels, false, 0, sort_buttons_room(width)) == BarWords::All;
        let widths = widest.map(|label| {
            label.map(|label| if words { widgets::text_button_width(true, label) } else { size::control_sm() })
        });
        SortSlots { widths, words }
    }
}

/// The room the buttons of a one-line To sort card `width` wide get: the card's inner width less
/// the lead (avatar or tile), the gaps beside it and half the properties column, which title and
/// meta keep at least (cut with "…" and a tooltip beyond that).
pub fn sort_buttons_room(width: f32) -> f32 {
    let pad = layout::panel_padding();
    let lead = size::avatar().max(size::tile_lg());
    (width - widgets::card_mark_inset() - pad.right - lead - 2.0 * space::m() - size::props() / 2.0).max(0.0)
}

/// The card of `item` in To sort `width` wide: one line – the lead, title and meta, the buttons at
/// the right in the section's fixed columns (`slots`, task E8.12) – or, in the narrow layout, the
/// head with the buttons below it, all with their words or none but the lead. A click opens it in
/// the panel.
fn sort_card<'a>(item: &'a Item, data: &HubData<'a>, width: f32, slots: &SortSlots) -> Element<'a, Message> {
    let selected = data.rows.selected == Some(item.id.as_str());
    let mail = item.source.kind == SourceKind::Mail;
    let unread = mail && item.state == State::New;
    let title_level = if mail { list::mail_text_level(unread) } else { typo::body_strong() };
    let (title, _) = list::words(item);
    let meta = sort_meta(item, &data.rows);
    let pad = layout::panel_padding();
    let inner = (width - if mail { widgets::card_mark_inset() } else { pad.left } - pad.right).max(0.0);
    let pressed: Vec<(usize, notifications::CardButton)> =
        acts(item).iter().map(|act| (sort_slot(act), card_button(act, item, data.deciding))).collect();
    let parts: Vec<Element<'a, Message>> = if data.layout.narrow {
        let buttons: Vec<notifications::CardButton> = pressed.into_iter().map(|(_, button)| button).collect();
        let words = notifications::worded(&buttons, false, inner);
        let line = buttons.into_iter().enumerate().map(|(i, button)| {
            let message = button.on_press.clone();
            let element = notifications::card_button(button, words.worded(i));
            data.rows.anchors.wrap(selected, message.as_ref(), element)
        });
        let line = row(line).spacing(space::s()).align_y(Alignment::Center);
        vec![
            widgets::card_head_with_level(Some(lead(item, &data.rows, data.attention)), title, meta, title_level, None),
            line.into(),
        ]
    } else {
        let mut line = row![].spacing(space::s()).align_y(Alignment::Center);
        for (slot, width) in slots.widths.iter().enumerate() {
            let Some(width) = *width else { continue };
            let cell: Element<'a, Message> = match pressed.iter().find(|(at, _)| *at == slot) {
                Some((_, button)) => {
                    let element = notifications::card_button(button.clone(), slots.words);
                    data.rows.anchors.wrap(selected, button.on_press.as_ref(), element)
                }
                None => Space::new().into(),
            };
            line = line.push(container(cell).width(width));
        }
        let words = container(widgets::card_head_with_level(None, title, meta, title_level, None)).width(Fill);
        vec![
            row![lead(item, &data.rows, data.attention), words, line]
                .spacing(space::m())
                .align_y(Alignment::Center)
                .into(),
        ]
    };
    let on_press = Message::Select(item.id.clone());
    let card = if mail {
        list::mail_card_mark(parts, selected, unread, on_press)
    } else {
        widgets::card(parts, selected, Some(on_press))
    };
    if selected { widgets::anchored(&data.rows.anchors.row, card) } else { card }
}

/// The row of `item` in Agents or To sort `width` wide (compact, [`as_rows`]): the mail or notice
/// row of every view with the action bar of its [`acts`] on its first line, each button with its
/// key.
fn entry_row<'a>(item: &'a Item, data: &HubData<'a>, width: f32) -> Element<'a, Message> {
    let selected = data.rows.selected == Some(item.id.as_str());
    let height = list::row_height_of(item);
    let content = if item.source.kind == SourceKind::Mail {
        list::mail_content(list::MailLines::of(item, &data.rows), width, selected)
    } else {
        list::notice_content(item, &data.rows, width)
    };
    let unread = (item.source.kind == SourceKind::Mail).then_some(item.state == State::New);
    let line = list::sized_row(content, selected, unread, Message::Select(item.id.clone()), height);
    let actions: Vec<RowAction<'a, Message>> = acts(item)
        .into_iter()
        .filter_map(|act| {
            let message = act.message(item, data.deciding)?;
            Some(RowAction::new(act.icon(), act.label(), act.key(), message))
        })
        .collect();
    let actions = data.rows.anchors.mark(selected, actions);
    let words = widgets::row_actions_words(&actions, width);
    let middle = list::first_line_middle(height, list::line_count(item));
    let line = widgets::row_actions_at(line, actions, selected, words, middle);
    if selected { widgets::anchored(&data.rows.anchors.row, line) } else { line }
}

/// The width the text of a row of "Coming back" gets: the properties column less its inset, the
/// row's padding, the icon and the gap after it.
fn side_room() -> f32 {
    size::props() - 2.0 * space::s() - 2.0 * space::s() - icon::SIZE - space::m()
}

/// The line under a handed-off task in "Handed off": its list and when it is due ("Work · due Fri,
/// 17 Oct"), "Task" for a list the core cannot name.
pub fn handed_meta(task: &ItemTask, now: DateTime<Utc>) -> String {
    let list = list::one_line(&task.list);
    let mut parts = vec![if list.is_empty() { tr("Task").to_owned() } else { list }];
    if let Some(due) = task.due {
        let today = now.with_timezone(&chrono::Local).date_naive();
        let day = torro_gui::datetime::format_short_date(crate::mail::due_day(due), today);
        parts.push(trf("due {day}", &[("day", &day)]));
    }
    parts.join(" · ")
}

/// One row of the column at the right: `mark`, the `title` over the `meta` line, cut to the column;
/// a click sends `on_press`, `chosen` draws it selected.
fn side_row<'a>(mark: Icon, title: &str, meta: &str, chosen: bool, on_press: Message) -> Element<'a, Message> {
    let title = typo::ellipsize(title, typo::body(), side_room());
    let meta = typo::ellipsize(meta, typo::meta(), side_room());
    let mark =
        icon::icon(mark, icon::SIZE).style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let words = column![
        typo::text(typo::body(), title).style(theme::text).wrapping(Wrapping::None),
        typo::text(typo::meta().tabular(), meta).style(theme::text_3).wrapping(Wrapping::None),
    ];
    let line = row![mark, words].spacing(space::m()).align_y(Alignment::Center);
    button(line)
        .width(Fill)
        .padding(Padding::new(space::xs()).left(space::s()).right(space::s()))
        .style(style::button::list_row(chosen))
        .on_press(on_press)
        .into()
}

/// What a click on a handed-off task does: open it in the tasks app while that is `installed` (issue
/// #408); `None` without it, and the row is no button.
fn handed_open(task: &ItemTask, installed: impl Fn(AppId) -> bool) -> Option<Message> {
    installed(AppId::TASKS).then(|| Message::OpenTask(task.uid.clone()))
}

/// The line between the blocks of the column at the right.
fn side_rule<'a>() -> Element<'a, Message> {
    container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::m(), 0.0]).into()
}

/// A row of the column at the right that is no button (quiet ends by itself): laid out as
/// [`side_row`], its padding included, so the blocks line up.
fn side_line<'a>(mark: Icon, title: &str, meta: &str) -> Element<'a, Message> {
    let title = typo::ellipsize(title, typo::body(), side_room());
    let meta = typo::ellipsize(meta, typo::meta(), side_room());
    let mark =
        icon::icon(mark, icon::SIZE).style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let words = column![
        typo::text(typo::body(), title).style(theme::text).wrapping(Wrapping::None),
        typo::text(typo::meta(), meta).style(theme::text_3).wrapping(Wrapping::None),
    ];
    let line = row![mark, words].spacing(space::m()).align_y(Alignment::Center);
    container(line).width(Fill).padding(Padding::new(space::xs()).left(space::s()).right(space::s())).into()
}

/// A quiet line under an empty block of the column at the right: what comes here.
fn side_hint<'a>(text: &'a str) -> Element<'a, Message> {
    let hint = typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph);
    container(hint).padding(Padding::new(0.0).left(space::s()).right(space::s())).into()
}

/// The column at the right of "Act now" (the wide layout): **Quiet** (task E8.5b) while it is quiet –
/// the moon, until when and the busy event ([`quiet_words`]), no button: it ends by itself –, then
/// **Handed off** (task E8.7b) – the open
/// tasks made of entries ([`handed_off`]), each with its title, list and due day, a click opens it in
/// the tasks app (`apps.open_app` with `task:<uid>`) – then, under a line, **Coming back**: the
/// snoozed entries, the soonest first, each with its title and its origin and when it wakes; a click
/// opens it in the panel. An empty block says in a quiet line what comes there.
pub fn side<'a>(
    snoozed: &'a [Item],
    handed: Vec<&'a ItemTask>,
    quiet: Option<&Quiet>,
    now: DateTime<Utc>,
    selected: Option<&'a str>,
) -> Element<'a, Message> {
    let inset = Padding::new(0.0).left(space::s());
    let mut content = column![].spacing(space::xs()).width(Fill);
    if let Some(quiet) = quiet {
        let (until, what) = quiet_words(quiet, now);
        content = content.push(container(caption(tr("Quiet"), 0)).padding(inset));
        content = content.push(side_line(Icon::Moon, &until, &what));
        content = content.push(side_rule());
    }
    content = content.push(container(caption(tr("Handed off"), handed.len())).padding(inset));
    if handed.is_empty() {
        content = content.push(side_hint(tr("Tasks you make of entries show up here until they are done.")));
    }
    for task in handed {
        let (title, meta) = (list::one_line(&task.title), handed_meta(task, now));
        content = content.push(match handed_open(task, torro_gui::installed::is_installed) {
            Some(open) => side_row(Icon::ListTodo, &title, &meta, false, open),
            // Without the tasks app the row only says what was handed off (issue #408).
            None => side_line(Icon::ListTodo, &title, &meta),
        });
    }
    let back = coming_back(snoozed);
    content = content.push(side_rule());
    content = content.push(container(caption(tr("Coming back"), back.len())).padding(inset));
    if back.is_empty() {
        content = content.push(side_hint(tr("What you put off comes back here.")));
    }
    for item in back {
        let until = match item.state {
            State::Snoozed { until } => list::wake_text(until, now),
            State::New | State::Seen | State::Done { .. } => String::new(),
        };
        let meta = format!("{} · {until}", Origin::of(item).name());
        let chosen = selected == Some(item.id.as_str());
        let open = Message::Select(item.id.clone());
        content = content.push(side_row(Icon::Clock, &list::words(item).0, &meta, chosen, open));
    }
    content.into()
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeDelta, TimeZone};
    use torro_inbox::{ItemFilter, StateFilter};

    use super::*;

    fn now() -> DateTime<Utc> {
        Local.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time").to_utc()
    }

    /// Issue #408: a handed-off task opens in the tasks app only while it is installed; without it
    /// the row only says what was handed off.
    #[test]
    fn a_handed_off_task_opens_only_with_the_tasks_app() {
        let task = ItemTask {
            uid: "budget@demo".into(),
            collection: "demo/work".into(),
            list: "Work".into(),
            title: "Check the travel budget".into(),
            due: None,
            completed: false,
            link: String::new(),
            mail_entry: None,
        };
        assert!(matches!(handed_open(&task, |_| true), Some(Message::OpenTask(ref uid)) if uid == "budget@demo"));
        assert!(handed_open(&task, |app| app != AppId::TASKS).is_none());
    }

    /// The entries of the demo in `state`, waiting first – what the app holds.
    fn entries(state: StateFilter) -> Vec<Item> {
        let demo = DemoSource::new(now());
        let filter = ItemFilter { state: Some(state), ..ItemFilter::default() };
        let mut items = iced::futures::executor::block_on(demo.list_items(filter)).expect("the demo answers");
        items.sort_by_key(|item| !list::is_waiting(item));
        items
    }

    fn ids<'a>(items: &[&'a Item]) -> Vec<&'a str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    fn find(items: &[Item], id: &str) -> Item {
        items.iter().find(|item| item.id == id).cloned().expect("in the demo")
    }

    #[test]
    fn for_me_goes_into_decide_agents_and_to_sort() {
        let items = entries(StateFilter::Open);
        let sections = Sections::of(&items, None);
        assert_eq!(ids(&sections.decide), ["demo-3"], "the approval");
        assert_eq!(ids(&sections.agents), ["demo-1", "demo-2", "demo-4"], "waiting first, then the finished one");
        assert_eq!(sections.to_sort.len(), 14, "with GitHub, Nextcloud, Talk and the ntfy alert (task E5.1)");
        assert!(sections.to_sort.iter().all(|item| item.lane == Lane::ForMe), "no Info");
        let for_me = items.iter().filter(|item| item.lane == Lane::ForMe).count();
        assert_eq!(sections.order(false).len(), for_me, "every entry for the user, once");
        assert_eq!(sections.order(true), sections.to_sort, "To sort alone");
        assert_eq!(summary(&sections), format!("1 {} · 3 {} · 14 {}", tr("to decide"), tr("agents"), tr("to sort")));
        let done = entries(StateFilter::Done);
        assert!(Sections::of(&done, None).is_empty(), "nothing done is to act on");
    }

    /// Task E8.5's acceptance: an approval has no button to dismiss or put it off – the list of
    /// buttons per kind of entry.
    #[test]
    fn approvals_have_neither_dismiss_nor_later() {
        let items = entries(StateFilter::Open);
        let approval = find(&items, "demo-3");
        let acts_of = acts(&approval);
        let id = approval_id(&approval);
        assert_eq!(acts_of, [Act::Allow(id.clone()), Act::Deny(id)]);
        for act in &acts_of {
            assert!(!matches!(act, Act::Dismiss | Act::Later | Act::Done | Act::Task), "{act:?}");
        }
        assert_eq!(acts(&find(&items, "demo-1")), [Act::GoToWindow, Act::Later, Act::Done], "an agent");
        let mail = items.iter().find(|item| item.source.kind == SourceKind::Mail).expect("a mail entry");
        assert_eq!(acts(mail), [Act::Task, Act::Event, Act::Later, Act::Archive], "mail is archived (task E18.5)");
        // Archive does what `e` does at the entry, quietly as Done did; a done mail entry reopens.
        let archive = card_button(&Act::Archive, mail, false);
        assert_eq!((archive.icon, archive.label.as_str()), (Icon::Archive, trc("action", "Archive")));
        assert!(matches!(&archive.on_press, Some(Message::Act(id, Command::Done)) if *id == mail.id));
        assert!(Act::Archive.key() == "E" && Act::Archive.lets_go(), "its key, and quiet");
        let done = Item { state: State::Done { at: now() }, ..mail.clone() };
        assert_eq!(acts(&done), [Act::Reopen], "\"Done today\" keeps Reopen (decision 9 of E18)");
        assert_eq!(
            acts(&find(&items, "demo-6")),
            [Act::Open, Act::Task, Act::Event, Act::Later, Act::Dismiss],
            "a notice"
        );
        assert_eq!(acts(&find(&items, "demo-12")), [Act::Task, Act::Event, Act::Later, Act::Dismiss], "no link");
        assert!(!acts_of.contains(&Act::Event), "an approval becomes no event either");
        let done = entries(StateFilter::Done);
        assert_eq!(acts(&find(&done, "demo-11")), [Act::Reopen]);
    }

    fn approval_id(item: &Item) -> String {
        match item.actions.first() {
            Some(Action::Allow { approval } | Action::Deny { approval }) => approval.clone(),
            None => String::new(),
        }
    }

    #[test]
    fn buttons_send_what_their_keys_send() {
        let items = entries(StateFilter::Open);
        let approval = find(&items, "demo-3");
        let id = approval_id(&approval);
        assert!(matches!(Act::Allow(id.clone()).message(&approval, false), Some(Message::Decide { allow: true, .. })));
        assert!(Act::Deny(id).message(&approval, true).is_none(), "disabled while deciding");
        let notice = find(&items, "demo-6");
        assert!(matches!(Act::Dismiss.message(&notice, false), Some(Message::Act(_, Command::Dismiss))));
        assert!(matches!(Act::Later.message(&notice, false), Some(Message::Act(_, Command::Snooze))));
        assert!(matches!(Act::Open.message(&notice, false), Some(Message::OpenRow(_))));
        assert!(matches!(Act::Event.message(&notice, false), Some(Message::Act(_, Command::MakeEvent))));
    }

    /// Task E8.5b: quiet says until when and which event – "A busy event" where the core keeps its
    /// title –, and its line counts in the spans.
    #[test]
    fn quiet_names_its_end_and_its_event() {
        let until = now() + TimeDelta::minutes(90);
        let named = Quiet { reason: torro_ipc::QuietReason::Busy, until, title: Some(" Customer\ncall ".into()) };
        let time = torro_gui::datetime::format_time(until.with_timezone(&Local).time());
        assert_eq!(quiet_words(&named, now()), (trf("Until {time}", &[("time", &time)]), "Customer call".to_owned()));
        let hidden = Quiet { title: None, ..named.clone() };
        assert_eq!(quiet_words(&hidden, now()).1, tr("A busy event"));
        assert_eq!(
            quiet_line(&named, now()),
            format!("{} · Customer call", trf("Quiet until {time}", &[("time", &time)]))
        );
        let tomorrow = Quiet { until: until + TimeDelta::days(1), ..named };
        assert!(quiet_words(&tomorrow, now()).0.len() > trf("Until {time}", &[("time", &time)]).len(), "with its day");

        let items = entries(StateFilter::Open);
        let sections = Sections::of(&items, None);
        let narrow = layout(440.0, SizeClass::Narrow, Density::Compact, false);
        let quiet = Layout { quiet: true, ..narrow };
        let (top, _) = span(&sections, 0, narrow, "demo-3").expect("the approval");
        let (later, _) = span(&sections, 0, quiet, "demo-3").expect("the approval");
        assert_eq!(later - top, quiet_height() + block_gap(true), "the line under the numbers");
    }

    /// Task E8.5b: the demo is quiet, offers its calendars, and an event of an entry marks it done.
    #[test]
    fn the_demo_answers_quiet_calendars_and_events() {
        let demo = DemoSource::new(now());
        use iced::futures::executor::block_on as block;
        let quiet = block(demo.get_quiet()).expect("the demo answers").expect("quiet");
        assert_eq!(quiet.title.as_deref(), Some("Customer call"));
        let calendars = block(demo.list_calendars()).expect("calendars");
        assert_eq!(calendars.len(), DEMO_CALENDARS.len());
        let id = "demo-6".to_owned();
        block(demo.create_event_from_item(id.clone(), calendars[0].id.clone(), None, now(), now())).expect("made");
        let done = block(demo.list_items(ItemFilter { state: Some(StateFilter::Done), ..ItemFilter::default() }));
        assert!(done.expect("the demo answers").iter().any(|item| item.id == id), "the entry is done");
    }

    #[test]
    fn tiles_count_the_origins_busiest_first_and_filter() {
        let items = entries(StateFilter::Open);
        let tiles = tiles(&items);
        assert_eq!(tiles.first().map(|tile| (&tile.origin, tile.count)), Some((&Origin::Mail, 5)));
        assert_eq!(tiles.iter().map(|tile| tile.count).sum::<usize>(), 18, "every entry for the user");
        let agents = tiles.iter().find(|tile| tile.origin == Origin::Agents).expect("an agents tile");
        assert_eq!(
            agents.line,
            format!("{} · {}", trf("{n} waiting", &[("n", &2)]), trf("{n} finished", &[("n", &1)]))
        );
        let mail = Sections::of(&items, Some(&Origin::Mail));
        assert!(mail.decide.is_empty() && mail.agents.is_empty());
        assert!(mail.to_sort.iter().all(|item| item.source.kind == SourceKind::Mail));
        assert_eq!(tile_columns(0.0), 1);
        assert!(tile_columns(1200.0) > tile_columns(600.0));
    }

    /// Task E5.15: one tile per service, all its accounts together, with the service's icon; its
    /// entries in To sort say what their traits say, an alert that it is urgent.
    #[test]
    fn services_have_a_tile_each_and_say_their_traits() {
        let mut items = entries(StateFilter::Open);
        // A second GitHub account joins the tile of the first.
        let mut other = find(&items, "demo-14");
        other.id = "demo-14b".into();
        other.source = SourceRef::github("anna-work");
        items.push(other);
        let tiles = tiles(&items);
        let service = |kind| tiles.iter().find(|tile| tile.origin == Origin::Service(kind)).map(|tile| tile.count);
        assert_eq!(service(SourceKind::Github), Some(4), "both accounts, Info not counted");
        assert_eq!([SourceKind::Nextcloud, SourceKind::Talk, SourceKind::Ntfy].map(service), [Some(1); 3]);
        assert!(!tiles.iter().any(|tile| matches!(&tile.origin, Origin::Script(name) if name == "github")));
        let names = [SourceKind::Github, SourceKind::Nextcloud, SourceKind::Talk, SourceKind::Ntfy]
            .map(|kind| (Origin::Service(kind).name(), Origin::Service(kind).icon()));
        assert_eq!(
            names,
            [
                ("GitHub".to_owned(), Icon::Github),
                ("Nextcloud".to_owned(), Icon::Cloud),
                ("Talk".to_owned(), Icon::MessageCircle),
                ("ntfy".to_owned(), Icon::RadioTower)
            ]
        );
        let github = Sections::of(&items, Some(&Origin::Service(SourceKind::Github)));
        assert_eq!(github.to_sort.len(), 4);

        let view = crate::app::View::Overview;
        let rows = Rows {
            selected_mail: &[],
            categories: &[],
            view: &view,
            selected: None,
            now: now(),
            anchors: detail::Anchors::default(),
            accounts: Vec::new(),
            hidable: Vec::new(),
            look: list::Look::Rows,
            folders: &[],
            bare: false,
        };
        let review = find(&items, "demo-14");
        let time = list::time_text_in(&review, &view, now(), false);
        assert_eq!(
            sort_meta(&review, &rows),
            format!("GitHub · torro/workplace · {} · {time}", tr("Review requested"))
        );
        let alert = find(&items, "demo-18");
        assert!(sort_meta(&alert, &rows).contains(tr("Urgent")), "an alert says it is urgent");
        assert_eq!(acts(&alert), [Act::Open, Act::Task, Act::Event, Act::Later, Act::Dismiss]);
        assert_eq!(acts(&find(&items, "demo-17")), [Act::Open, Act::Task, Act::Event, Act::Later, Act::Dismiss]);
    }

    #[test]
    fn layouts_change_with_the_size_and_the_density() {
        let wide = layout(1000.0, SizeClass::Wide, Density::Comfortable, false);
        assert!(!wide.narrow && wide.cards && wide.columns >= 2 && wide.tiles >= 3);
        let narrow = layout(440.0, SizeClass::Narrow, Density::Compact, false);
        assert!(narrow.narrow && !narrow.cards);
        assert_eq!((narrow.columns, narrow.tiles), (1, 0), "one card per line, the numbers instead of tiles");
        assert_eq!(
            layout(9000.0, SizeClass::Wide, Density::Compact, false),
            layout(max_width() - sides(false), SizeClass::Wide, Density::Compact, false),
            "a huge window is not stretched"
        );
    }

    #[test]
    fn spans_count_tiles_captions_cards_and_rows() {
        let items = entries(StateFilter::Open);
        let sections = Sections::of(&items, None);
        let wide = layout(1000.0, SizeClass::Wide, Density::Comfortable, false);
        let top = inset(false).top;
        let tiles = tiles(&items).len();
        let first = top + tile_height() + block_gap(false);
        let caption = typo::line_height(typo::caption()) + caption_gap();
        let decide = card_height(sections.decide[0], false);
        assert_eq!(span(&sections, tiles, wide, "demo-3"), Some((first, first + caption + decide)));
        let agents = first + caption + decide + block_gap(false);
        let (start, _) = span(&sections, tiles, wide, "demo-1").expect("the first agent");
        assert_eq!(start, agents, "the first line starts at its caption");
        let sort = span(&sections, tiles, wide, &sections.to_sort[1].id).expect("the second to sort");
        let height = sort_height(sections.to_sort[1], wide);
        assert_eq!(sort.1 - sort.0, height);
        assert_eq!(span(&sections, tiles, wide, "demo-8"), None, "Info is not here");
        // Narrow and compact: the numbers, then rows under a line.
        let narrow = layout(440.0, SizeClass::Narrow, Density::Compact, false);
        let (_, end) = span(&sections, 0, narrow, "demo-3").expect("the approval");
        assert_eq!(
            end,
            inset(true).top
                + typo::line_height(typo::display())
                + block_gap(true)
                + caption
                + card_height(sections.decide[0], true)
        );
        assert!(card_height(sections.decide[0], true) > card_height(sections.decide[0], false), "big Allow and Deny");
        // Compact: the agents are rows under a line, the approval keeps its card.
        let agents = end + block_gap(true);
        let (top_agent, bottom_agent) = span(&sections, 0, narrow, "demo-1").expect("the first agent");
        assert_eq!(top_agent, agents);
        assert_eq!(bottom_agent, agents + caption + size::HAIRLINE + list::row_height_of(sections.agents[0]));
        // To sort alone has no caption (task E8.12): the header says "To sort 9" already.
        let alone = layout(1000.0, SizeClass::Wide, Density::Comfortable, true);
        let first = &sections.to_sort[0].id;
        assert_eq!(span(&sections, tiles, alone, first), Some((top, top + sort_height(sections.to_sort[0], alone))));
        assert_eq!(span(&sections, tiles, alone, "demo-3"), None);
        assert!(!has_caption(Section::ToSort, alone) && has_caption(Section::ToSort, wide));
    }

    /// Task E8.12 (finding 5): a medium window gives Decide and Follow up the full width; a wide one
    /// puts them side by side, each a column of its cards starting at the same height.
    #[test]
    fn decide_and_follow_up_stand_side_by_side_where_it_is_wide() {
        let items = entries(StateFilter::Open);
        let approval = find(&items, "demo-3");
        let question = find(&items, "demo-6");
        let sections = Sections { decide: vec![&approval], follow_up: vec![&question], ..Sections::default() };
        let wide = layout(1000.0, SizeClass::Wide, Density::Comfortable, false);
        let blocks = section_blocks(&sections, wide);
        assert_eq!(blocks.len(), 1, "one block");
        assert_eq!(
            blocks[0].iter().map(|(section, _)| *section).collect::<Vec<_>>(),
            [Section::Decide, Section::FollowUp]
        );
        let (decide, _) = span(&sections, 0, wide, "demo-3").expect("the approval");
        let (follow, _) = span(&sections, 0, wide, "demo-6").expect("the question");
        assert_eq!(decide, follow, "side by side");
        assert_eq!(per_line_in(Section::Decide, wide, 2), 1, "a column of cards each");
        assert_eq!(per_line(Section::Decide, wide), 2, "alone: two a line");

        let medium = layout(684.0, SizeClass::Medium, Density::Comfortable, false);
        assert_eq!(medium.columns, 2);
        assert_eq!(per_line(Section::Decide, medium), 1, "the full width");
        assert_eq!(per_line(Section::Agents, medium), 2, "agents keep their grid");
        assert_eq!(section_blocks(&sections, medium).len(), 2, "one under the other");
        let (below, _) = span(&sections, 0, medium, "demo-6").expect("the question");
        assert!(below > decide, "under Decide");
    }

    /// Task E8.12 (finding 12): one rule for the looks – what lets the entry go is quiet, every
    /// other visible button framed; the lead keeps its word longest.
    #[test]
    fn what_lets_an_entry_go_is_quiet_and_the_rest_is_framed() {
        for quiet in [Act::Done, Act::Archive, Act::Dismiss, Act::NotNow] {
            assert!(quiet.lets_go(), "{quiet:?}");
        }
        for framed in
            [Act::Later, Act::GoToWindow, Act::Task, Act::Event, Act::Open, Act::Reply, Act::Reopen, Act::ArchiveMail]
        {
            assert!(!framed.lets_go(), "{framed:?}");
        }
        assert!(Act::Open.leads() && Act::GoToWindow.leads() && Act::Reply.leads());
        assert!(!Act::Task.leads() && !Act::Allow("a".into()).leads());
        let items = entries(StateFilter::Open);
        let agent = find(&items, "demo-1");
        let buttons: Vec<notifications::CardButton> =
            acts(&agent).iter().map(|act| card_button(act, &agent, false)).collect();
        assert_eq!(buttons.iter().map(|button| button.lead).collect::<Vec<_>>(), [true, false, false]);
        let labels: Vec<&str> = buttons.iter().map(|button| button.label.as_str()).collect();
        let lead = widgets::bar_width(&labels, BarWords::Lead, 0);
        assert_eq!(notifications::worded(&buttons, false, lead), BarWords::Lead, "Go to window keeps its word");
        assert_eq!(notifications::worded(&buttons, false, lead - 1.0), BarWords::None, "else none – never mixed");
    }

    /// Task E8.12 (finding 8): To sort's buttons stand in fixed columns – Task under Task, whatever
    /// else an entry offers –, with words for all or for none.
    #[test]
    fn to_sort_buttons_stand_in_fixed_columns() {
        let items = entries(StateFilter::Open);
        let sections = Sections::of(&items, None);
        // A card in a wide window. Since issue #291 the standard's buttons are larger (36, 12 at the
        // sides): at the reading width (720) the German words no longer fit, from about 760 they do.
        let slots = SortSlots::of(&sections.to_sort, false, 960.0);
        assert!(slots.words, "a wide card has room for the words");
        assert!(slots.widths.iter().all(Option::is_some), "mail and notices: every column");
        let task = slots.widths[sort_slot(&Act::Task)].expect("Task");
        assert_eq!(task, widgets::text_button_width(true, tr("Task")));
        let end = slots.widths[sort_slot(&Act::Archive)].expect("Archive or Dismiss");
        assert_eq!(
            end,
            widgets::text_button_width(true, trc("action", "Archive"))
                .max(widgets::text_button_width(true, tr("Dismiss")))
        );
        let tight = SortSlots::of(&sections.to_sort, false, 500.0);
        assert!(!tight.words && tight.widths.iter().flatten().all(|width| *width == size::control_sm()), "icons");
        let mail: Vec<&Item> =
            sections.to_sort.iter().copied().filter(|item| item.source.kind == SourceKind::Mail).collect();
        let only_mail = SortSlots::of(&mail, false, size::content_max());
        assert_eq!(only_mail.widths[sort_slot(&Act::Open)], None, "no Open: no column for it");
        assert!(!SortSlots::of(&sections.to_sort, true, size::content_max()).words, "narrow: no columns");
    }

    #[test]
    fn coming_back_lists_the_snoozed_soonest_first() {
        // The CI notice (20 h) and the demo's follow-up on a sent mail (two days, task E19.3).
        let mut snoozed = entries(StateFilter::Snoozed);
        let ids = |snoozed: &[Item]| coming_back(snoozed).iter().map(|item| item.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&snoozed), ["demo-10", "demo-follow-p4"], "soonest first");
        let mut sooner = snoozed[0].clone();
        sooner.id = "sooner".into();
        sooner.state = State::Snoozed { until: now() + TimeDelta::hours(1) };
        snoozed.push(sooner);
        assert_eq!(ids(&snoozed), ["sooner", "demo-10", "demo-follow-p4"]);
        assert!(coming_back(&entries(StateFilter::Open)).is_empty());
    }
}
