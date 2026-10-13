//! Desktop popups for the inbox, and quiet during busy events (task E3.4).
//!
//! The core watches the inbox ([`crate::inbox::Inbox::changes`]) and shows a popup for every
//! entry "for me" that is new or came back: posted, merged with a new report, woken from snoozing.
//! The rules ([`Popups`]) are pure, with the clock passed in; [`run`] feeds them from the core and
//! puts their popups on the desktop ([`Desktop`]).
//!
//! - **Only "for me", only new ones.** Entries in "Info", seen, snoozed and done ones never pop
//!   up (`docs/eingang.md`, rule 5). Neither does anything that was open when the core started:
//!   the first look only learns what is there, so a restart does not repeat the day.
//! - **Approvals ask on their own path.** `approvals.rs` asks with "Allow" and "Deny" the moment
//!   an approval opens and closes the question once it is decided (tasks A.3, A.3b). Its inbox
//!   entry (task E2.1) therefore gets no second popup here ([`asks_elsewhere`]). Approvals are
//!   urgent, so quiet never holds them back on either path.
//! - **Buttons:** "Done" (`inbox.set_item_done`) and "Snooze 1 h" (`inbox.snooze_item`), sent as
//!   the core itself with all rights, like the tray. A click on the popup opens `torroinbox`
//!   (`apps.open_app`); jumping to the entry would need a way to tell the running app, which does
//!   not exist yet.
//! - **Mail** (task E4.4): a new mail entry in "For me" has the buttons "Archive" (`mail.move_messages`
//!   to the account's archive) and "Mark read" (`mail.set_flags`, `seen`) instead, applied to every
//!   message of the entry's conversation; the feed (task E4.1) then finishes the entry by itself.
//!   Mail arriving within [`MAIL_SETTLE`] of each other is collected and shown as **one** popup
//!   ("3 new messages from Anna, Ben, Chris", [`Popup::MailBundle`]); a single mail shows as itself.
//!   Subject and sender are foreign text: put on one line, cut, and without `<` and `>` ([`foreign_line`]),
//!   so no server reads a link or markup out of them.
//! - **Urgent ones** (an agent waits, an approval) come with critical urgency and a sound; the
//!   others ask the notification server to stay silent.
//! - **No floods:** at most [`BUNDLE_AFTER`] popups within [`BUNDLE_WITHIN`]. When more arrive,
//!   they wait for room and then come as one "{n} new for you" (four at once are one popup
//!   straight away). Waiting ones that were handled meanwhile are dropped.
//! - **Quiet** (`Settings::quiet_when_busy`, on by default): while a busy event runs (opaque, not
//!   all-day, in a calendar the suite shows), only urgent entries pop up. When no busy event runs
//!   any more, **one** message tells how many of the held-back entries are still open. It names
//!   the event when there was one: it is the person's own calendar on the person's own desktop,
//!   and `docs/eingang.md` asks for exactly that ("While you were in …"). Its title is shown as
//!   plain text like every other text here.
//!
//! - **No popup by choice** (task E8.6a): an entry a muted trait with "no popup" holds for comes
//!   without one ([`crate::inbox::Inbox::no_popup`], [`Popups::step_except`]), and not later either.
//! - **Counted** (task E8.8a): each entry that arrives for a popup counts once for the statistics,
//!   with those quiet holds back apart ([`Popups::arrivals`], `Inbox::count_popups`).
//! - **Agents name their session** (task E2.6): an entry of a hook whose session has a name pops
//!   up with that name as summary and the core's own words below it ("Claude Code is waiting for
//!   you · TorroWorkplace"), so the person knows which job it is ([`session_line`]).
//!
//! Titles of foreign sources are `untrusted`: the summary of a popup is plain text by the
//! notification specification, the body is escaped ([`crate::notify::plain`]) so it never becomes
//! markup or a link, as for approvals (task A.3b).

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_inbox::{Item, Kind, Lane, Link, SourceKind, State};
use torro_ipc::mail::{MoveRole, MoveTarget};
use torro_ipc::{Request, Response};
use torro_mail::Flag;
use torro_pim::{Event, EventTime};

use crate::core::Core;

/// The most popups within [`BUNDLE_WITHIN`]; more are bundled into one.
pub const BUNDLE_AFTER: usize = 3;

/// The span [`BUNDLE_AFTER`] counts in.
pub const BUNDLE_WITHIN: TimeDelta = TimeDelta::minutes(1);

/// How long "Snooze 1 h" puts an entry off.
pub const SNOOZE_FOR: TimeDelta = TimeDelta::hours(1);

/// How often the core looks again while something waits: popups held back for room, or a quiet
/// spell whose event may have ended. Nothing ticks otherwise.
const TICK: Duration = Duration::from_secs(15);

/// A burst of changes (a hook reporting twice, a merge) settles this long before the core looks.
const SETTLE: Duration = Duration::from_millis(300);

/// New mail waits this long for more to follow, so a handful arriving one after the other is one
/// popup ("3 new messages from …") instead of a flood. Counted from the first of them.
pub const MAIL_SETTLE: TimeDelta = TimeDelta::seconds(8);

/// How often the core looks again while mail is collected, so [`MAIL_SETTLE`] ends on time.
const MAIL_TICK: Duration = Duration::from_secs(2);

/// The longest subject of a mail popup, in characters.
const MAIL_SUBJECT_MAX: usize = 100;

/// The longest sender name in a mail popup, in characters.
const MAIL_SENDER_MAX: usize = 40;

/// The longest project name below an agent session's name in a popup, in characters.
const PROJECT_MAX: usize = 40;

/// How many sender names a summary of several mails lists; more are counted ("and 2 more").
const SENDERS_SHOWN: usize = 3;

/// `mail.set_flags` and `mail.move_messages` take at most this many messages.
const MAIL_IDS_MAX: usize = 100;

/// How long the core listens for the buttons of one popup; then it closes it. The entry stays in
/// the inbox.
#[cfg(all(unix, not(target_os = "macos")))]
const POPUP_LASTS: Duration = Duration::from_secs(60 * 60);

/// The longest event title in the message after quiet, in characters.
const EVENT_TITLE_MAX: usize = 60;

/// One entry as a popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPopup {
    /// The entry's id, for "Done" and "Snooze 1 h".
    pub id: String,
    /// The entry's title; foreign text when the entry is `untrusted`.
    pub title: String,
    /// Who reported it: the sender if the source names one, and the source.
    pub source: String,
    pub urgent: bool,
    /// For a mail entry the messages "Archive" and "Mark read" act on (`m:…`): the newest one
    /// from the entry, all of its conversation once the core looked them up ([`run`]). Empty for
    /// every other entry.
    pub mail: Vec<String>,
}

/// What to put on the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    /// One entry, with "Done" and "Snooze 1 h".
    Entry(EntryPopup),
    /// Several entries at once: "{n} new for you". Urgent when one of them is.
    Bundle { count: usize, urgent: bool },
    /// Several new mail entries that came within [`MAIL_SETTLE`]: "{n} new messages from …". Has no
    /// buttons, the messages are in the inbox. `senders` are the distinct names (cleaned, cut), newest
    /// first; it is empty when none of the mails names a sender.
    MailBundle { count: usize, senders: Vec<String> },
    /// After a busy event: how many entries held back during it are still open, and the event's
    /// title when quiet spanned only one event.
    AfterQuiet { count: usize, event: Option<String> },
}

impl Popup {
    /// The summary (plain text) and body of the popup, in the core's language. The body is shown
    /// as plain text, so whoever shows it escapes it for servers that read markup.
    pub fn text(&self) -> (String, String) {
        let see = || tr("Open the inbox to see them.").to_string();
        match self {
            Popup::Entry(entry) => (entry.title.clone(), entry.source.clone()),
            Popup::Bundle { count, .. } => (trf("{n} new for you", &[("n", count)]), see()),
            Popup::MailBundle { count, senders } if senders.is_empty() => {
                (trf("{n} new for you", &[("n", count)]), see())
            }
            Popup::MailBundle { count, senders } => {
                (trf("{n} new messages from {senders}", &[("n", count), ("senders", &sender_list(senders))]), see())
            }
            Popup::AfterQuiet { count, event: Some(event) } if !event.trim().is_empty() => {
                let event = crate::inbox::cap(event.trim(), EVENT_TITLE_MAX);
                (trf("While you were in “{event}”: {n} for you", &[("event", &event), ("n", count)]), see())
            }
            Popup::AfterQuiet { count, .. } => {
                (trf("While you were in busy events: {n} for you", &[("n", count)]), see())
            }
        }
    }

    /// Comes with critical urgency and a sound. Only where popups carry urgency (Linux, the BSDs).
    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
    pub fn urgent(&self) -> bool {
        match self {
            Popup::Entry(entry) => entry.urgent,
            Popup::Bundle { urgent, .. } => *urgent,
            Popup::MailBundle { .. } | Popup::AfterQuiet { .. } => false,
        }
    }

    /// The entry "Done" and "Snooze 1 h" act on; `None` for the summaries, which have no buttons.
    pub fn entry(&self) -> Option<&str> {
        match self {
            Popup::Entry(entry) => Some(&entry.id),
            Popup::Bundle { .. } | Popup::MailBundle { .. } | Popup::AfterQuiet { .. } => None,
        }
    }

    /// The messages "Archive" and "Mark read" act on: set for a popup of one mail entry, `None`
    /// for everything else (which gets "Done" and "Snooze 1 h", or no buttons).
    pub fn messages(&self) -> Option<&[String]> {
        match self {
            Popup::Entry(entry) if !entry.mail.is_empty() => Some(&entry.mail),
            _ => None,
        }
    }
}

/// The names of `senders` for a summary: the first [`SENDERS_SHOWN`] separated by commas, the rest
/// counted.
fn sender_list(senders: &[String]) -> String {
    let shown = senders.iter().take(SENDERS_SHOWN).cloned().collect::<Vec<_>>().join(", ");
    match senders.len().saturating_sub(SENDERS_SHOWN) {
        0 => shown,
        more => trf("{names} and {n} more", &[("names", &shown), ("n", &more)]),
    }
}

/// What the person did with a popup. Closing it, or letting it time out, is no answer. Only
/// popups with buttons answer (Linux, the BSDs); on macOS and Windows none comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
pub enum Answer {
    /// "Done".
    Done,
    /// "Snooze 1 h".
    Snooze,
    /// A click on the popup itself.
    Open,
    /// "Archive", on a mail.
    Archive,
    /// "Mark read", on a mail.
    MarkRead,
}

/// The request `answer` to `popup` sends at `now`: "Done" marks the entry done, "Snooze 1 h" puts
/// it off until `now` + [`SNOOZE_FOR`], a click opens the inbox app. On a mail "Archive" moves its
/// messages to the account's archive and "Mark read" sets `seen` on them. `None` for buttons a
/// popup does not have ("Done" on a summary, "Archive" on anything but a mail).
pub fn request(answer: Answer, popup: &Popup, now: DateTime<Utc>) -> Option<Request> {
    let messages = || popup.messages().map(|ids| ids.iter().take(MAIL_IDS_MAX).cloned().collect::<Vec<_>>());
    match answer {
        Answer::Archive => Some(Request::MoveMail { ids: messages()?, to: MoveTarget::Role(MoveRole::Archive) }),
        Answer::MarkRead => Some(Request::SetMailFlags { ids: messages()?, add: vec![Flag::Seen], remove: Vec::new() }),
        Answer::Done => Some(Request::SetItemDone { id: popup.entry()?.to_string(), done: true, dismissed: false }),
        Answer::Snooze => Some(Request::SnoozeItem { id: popup.entry()?.to_string(), until: Some(now + SNOOZE_FOR) }),
        Answer::Open => Some(Request::OpenApp { app: AppId::INBOX.binary.to_string(), target: None }),
    }
}

/// Where popups go: the desktop ([`SystemDesktop`]), or a fake in tests.
pub trait Desktop: Send + Sync {
    /// Shows `popup` and calls `answer` at most once, with what the person chose. Must return at
    /// once: it runs in the core's watch over the inbox. Buttons are "Done" and "Snooze 1 h" when
    /// [`Popup::entry`] is set; a click on the popup is [`Answer::Open`].
    fn show(&self, popup: &Popup, answer: Box<dyn FnOnce(Answer) + Send>);
}

/// The kinds that have a popup of their own elsewhere, so the inbox does not show a second one.
/// Approvals ask with "Allow" and "Deny" in `approvals.rs` (module docs).
pub fn asks_elsewhere(kind: Kind) -> bool {
    kind == Kind::Approval
}

/// The rules of the popups: what pops up, what waits, what quiet holds back. Pure: the caller
/// passes the clock, the open entries and whether a busy event runs.
#[derive(Debug, Default)]
pub struct Popups {
    /// `updated` of every open entry "for me" at the last look; `None` before the first.
    known: Option<HashMap<String, DateTime<Utc>>>,
    /// When the popups of the last [`BUNDLE_WITHIN`] were shown, oldest first.
    shown: VecDeque<DateTime<Utc>>,
    /// Entries waiting for room, in the order they came.
    held: Vec<EntryPopup>,
    /// The entries quiet held back, while a busy event runs.
    quiet: Option<Spell>,
    /// New mail collected for [`MAIL_SETTLE`], oldest first (task E4.4).
    mail: Vec<MailArrival>,
}

/// One new mail entry waiting for more mail to come with it.
#[derive(Debug)]
struct MailArrival {
    entry: EntryPopup,
    /// The sender's name, cleaned and cut; `None` when the mail names none.
    sender: Option<String>,
    /// When it was first seen; a merge into the same entry keeps it, so a steady trickle cannot
    /// hold the popup back for ever.
    since: DateTime<Utc>,
}

/// One stretch of quiet: from the first entry held back until no busy event runs any more.
#[derive(Debug, Default)]
struct Spell {
    /// The titles of the busy events entries were held back in, each once.
    events: Vec<String>,
    /// The ids of the entries held back.
    entries: BTreeSet<String>,
}

impl Popups {
    pub fn new() -> Popups {
        Popups::default()
    }

    /// Whether [`Popups::step`] needs to know about a busy event for `open`: a quiet spell runs
    /// (has its event ended?), or a new entry that is not urgent arrived (hold it back?). Asking
    /// the calendar only then keeps the idle core idle.
    pub fn needs_calendar(&self, open: &[Item]) -> bool {
        let Some(known) = &self.known else { return false };
        self.quiet.is_some() || !self.mail.is_empty() || open.iter().any(|item| arrived(known, item) && !item.urgent)
    }

    /// Whether something waits for a later look: popups held back for room, mail being collected,
    /// or a quiet spell.
    pub fn waiting(&self) -> bool {
        !self.held.is_empty() || !self.mail.is_empty() || self.quiet.is_some()
    }

    /// How long to wait for the next look while [`Popups::waiting`]: shorter while mail is collected,
    /// so its popup is not late by a whole [`TICK`].
    fn tick(&self) -> Duration {
        if self.mail.is_empty() { TICK } else { MAIL_TICK }
    }

    /// One look at `now` with `open`, the open entries "for me" (as `Inbox::for_me` lists them),
    /// and `busy`, the title of the busy event that holds popups back, or `None` when nothing
    /// does (no such event, or quiet turned off). Returns the popups to show now, in order.
    ///
    /// The first look only learns what is open and shows nothing. After it, an entry arrives
    /// when it is new (state [`State::New`]), in "for me", not [`asks_elsewhere`], and was not
    /// open at the last look or has a different `updated` (merged, woken).
    ///
    /// Every entry may pop up here; the core asks [`Popups::step_except`] with the entries a muted
    /// trait keeps quiet.
    #[cfg(test)]
    pub fn step(&mut self, now: DateTime<Utc>, open: &[Item], busy: Option<&str>) -> Vec<Popup> {
        self.step_except(now, open, busy, &BTreeSet::new())
    }

    /// The entries of `open` that [`Popups::step_except`] would take as arriving for a popup now,
    /// those of `no_popup` aside: for the statistics (task E8.8a), asked just before the step.
    /// Empty before the first look, which only learns what is open.
    pub fn arrivals<'a>(&self, open: &'a [Item], no_popup: &BTreeSet<String>) -> Vec<&'a Item> {
        let Some(known) = &self.known else { return Vec::new() };
        open.iter().filter(|item| arrived(known, item) && !no_popup.contains(&item.id)).collect()
    }

    /// [`Popups::step`], but the entries `no_popup` names (task E8.6a: a muted trait with "no
    /// popup" holds for them, `Inbox::no_popup`) arrive without a popup: they are known from now on,
    /// so they do not pop up later either, when the rule ends.
    pub fn step_except(
        &mut self,
        now: DateTime<Utc>,
        open: &[Item],
        busy: Option<&str>,
        no_popup: &BTreeSet<String>,
    ) -> Vec<Popup> {
        let current = open.iter().map(|item| (item.id.clone(), item.updated)).collect();
        let arrivals = self.arrivals(open, no_popup);
        if self.known.replace(current).is_none() {
            return Vec::new();
        }
        let mut popups = Vec::new();
        match busy {
            Some(event) => {
                // Mail collected before the event began is held back with the rest.
                for arrival in std::mem::take(&mut self.mail) {
                    self.silence(&arrival.entry.id, event);
                }
                for item in arrivals {
                    if item.urgent {
                        self.hold(entry(item));
                    } else {
                        self.silence(&item.id, event);
                    }
                }
            }
            None => {
                if let Some(popup) = self.end_quiet(open) {
                    popups.push(popup);
                }
                for item in arrivals {
                    if is_mail(item) {
                        self.collect(item, now);
                    } else {
                        self.hold(entry(item));
                    }
                }
            }
        }
        self.make_room(now, popups.len());
        self.held.retain(|held| open.iter().any(|item| item.id == held.id));
        self.mail.retain(|arrival| open.iter().any(|item| item.id == arrival.entry.id));
        popups.extend(self.flush_mail(now));
        popups.extend(self.release(now));
        popups
    }

    /// Holds back the entry `id` for the quiet spell of the busy event `event`.
    fn silence(&mut self, id: &str, event: &str) {
        let spell = self.quiet.get_or_insert_with(Spell::default);
        spell.entries.insert(id.to_string());
        if !spell.events.iter().any(|known| known == event) {
            spell.events.push(event.to_string());
        }
    }

    /// Collects the new mail `item` for [`Popups::flush_mail`]; a merge into an entry already
    /// collected replaces it.
    fn collect(&mut self, item: &Item, now: DateTime<Utc>) {
        let (entry, sender) = (entry(item), sender_name(item));
        match self.mail.iter_mut().find(|arrival| arrival.entry.id == entry.id) {
            Some(arrival) => {
                arrival.entry = entry;
                arrival.sender = sender;
            }
            None => self.mail.push(MailArrival { entry, sender, since: now }),
        }
    }

    /// Ends the collecting of mail once its first has waited [`MAIL_SETTLE`]: one mail is a popup of
    /// its own (it goes through [`Popups::release`] like any entry), several are one
    /// [`Popup::MailBundle`] while the minute has room, else they wait and bundle as "for you".
    fn flush_mail(&mut self, now: DateTime<Utc>) -> Option<Popup> {
        let first = self.mail.first()?;
        if now - first.since < MAIL_SETTLE {
            return None;
        }
        let group = std::mem::take(&mut self.mail);
        let room = BUNDLE_AFTER.saturating_sub(self.shown.len());
        if group.len() == 1 || room == 0 {
            for arrival in group {
                self.hold(arrival.entry);
            }
            return None;
        }
        let mut senders: Vec<String> = Vec::new();
        for sender in group.iter().filter_map(|arrival| arrival.sender.as_ref()) {
            if !senders.contains(sender) {
                senders.push(sender.clone());
            }
        }
        self.shown.push_back(now);
        Some(Popup::MailBundle { count: group.len(), senders })
    }

    /// Keeps `entry` for the next popup, once: a merge while it waits replaces it.
    fn hold(&mut self, entry: EntryPopup) {
        match self.held.iter_mut().find(|held| held.id == entry.id) {
            Some(held) => *held = entry,
            None => self.held.push(entry),
        }
    }

    /// Ends the quiet spell, if one runs: the message with the entries held back that are still
    /// open, or `None` when none are (handled during the event) or no spell ran.
    fn end_quiet(&mut self, open: &[Item]) -> Option<Popup> {
        let spell = self.quiet.take()?;
        let count = spell.entries.iter().filter(|id| open.iter().any(|item| &item.id == *id)).count();
        let event = match spell.events.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        };
        (count > 0).then_some(Popup::AfterQuiet { count, event })
    }

    /// Forgets popups older than [`BUNDLE_WITHIN`] and counts `just_shown` more at `now`.
    fn make_room(&mut self, now: DateTime<Utc>, just_shown: usize) {
        while self.shown.front().is_some_and(|&at| now - at >= BUNDLE_WITHIN) {
            self.shown.pop_front();
        }
        self.shown.extend(std::iter::repeat_n(now, just_shown));
    }

    /// The waiting entries as popups, if there is room: each on its own while they fit, else all
    /// in one bundle; with no room at all they wait.
    fn release(&mut self, now: DateTime<Utc>) -> Vec<Popup> {
        let room = BUNDLE_AFTER.saturating_sub(self.shown.len());
        if self.held.is_empty() || room == 0 {
            return Vec::new();
        }
        if self.held.len() <= room {
            let popups: Vec<Popup> = self.held.drain(..).map(Popup::Entry).collect();
            self.shown.extend(std::iter::repeat_n(now, popups.len()));
            return popups;
        }
        let urgent = self.held.iter().any(|held| held.urgent);
        let count = self.held.len();
        self.held.clear();
        self.shown.push_back(now);
        vec![Popup::Bundle { count, urgent }]
    }
}

/// `item` is new or came back since the look that saw `known` (rules at [`Popups::step`]).
fn arrived(known: &HashMap<String, DateTime<Utc>>, item: &Item) -> bool {
    item.lane == Lane::ForMe
        && item.state == State::New
        && !asks_elsewhere(item.kind)
        && known.get(&item.id) != Some(&item.updated)
}

/// `item` is mail the feed filed (task E4.1): foreign text, with buttons of its own.
fn is_mail(item: &Item) -> bool {
    item.source.kind == SourceKind::Mail && item.kind == Kind::Message
}

/// `text` from outside for a notification: on one line, without control and direction characters,
/// `<` and `>` turned into `‹` and `›` (a notification server may read markup, even in a summary),
/// cut to `max` characters.
fn foreign_line(text: &str, max: usize) -> String {
    let direction = |c: char| matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '<' => '‹',
            '>' => '›',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .filter(|&c| !c.is_control() && !direction(c))
        .collect();
    crate::inbox::cap(&cleaned.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// The sender of `item` for a popup: the name, else the address; `None` when it has neither.
fn sender_name(item: &Item) -> Option<String> {
    let from = item.from.as_ref()?;
    let name = foreign_line(&from.name, MAIL_SENDER_MAX);
    let named =
        if name.is_empty() { foreign_line(from.address.as_deref().unwrap_or_default(), MAIL_SENDER_MAX) } else { name };
    (!named.is_empty()).then_some(named)
}

/// The popup of one entry: its title, and who reported it ("Lena · backup"). Mail is cleaned and
/// cut ([`foreign_line`]) and carries its message for "Archive" and "Mark read".
fn entry(item: &Item) -> EntryPopup {
    if is_mail(item) {
        let account = item.source.label();
        let source = match sender_name(item) {
            Some(sender) => format!("{sender} · {account}"),
            None => account,
        };
        let mail = match &item.link {
            Some(Link::Mail { message }) => vec![message.clone()],
            _ => Vec::new(),
        };
        let title = foreign_line(&item.title, MAIL_SUBJECT_MAX);
        return EntryPopup { id: item.id.clone(), title, source, urgent: false, mail };
    }
    if let Some((title, source)) = session_line(item) {
        return EntryPopup { id: item.id.clone(), title, source, urgent: item.urgent, mail: Vec::new() };
    }
    let source = match &item.from {
        Some(from) if !from.name.trim().is_empty() => format!("{} · {}", from.name.trim(), item.source.label()),
        _ => item.source.label(),
    };
    EntryPopup { id: item.id.clone(), title: item.title.clone(), source, urgent: item.urgent, mail: Vec::new() }
}

/// Summary and body of a popup for an agent's session that has a name (task E2.6): the name,
/// cleaned and cut ([`foreign_line`], it is the agent's text), and below it the entry's title –
/// the core's own words about the agent's state – with the project when there is one. `None` for
/// every other entry, which keeps its title as summary.
fn session_line(item: &Item) -> Option<(String, String)> {
    let agent = item.agent.as_ref()?;
    let name = foreign_line(agent.title.as_deref()?, torro_inbox::hook::SESSION_TITLE_MAX);
    if name.is_empty() {
        return None;
    }
    let project = agent.project.as_deref().map(|project| foreign_line(project, PROJECT_MAX));
    let below = match project.filter(|project| !project.is_empty()) {
        Some(project) => format!("{} · {project}", item.title),
        None => item.title.clone(),
    };
    Some((name, below))
}

/// The busy event that makes it quiet at `now`, among `events` of the shown calendars: one that
/// is [`Event::busy`] (opaque, not all-day) and runs now (started, not yet ended; an event without
/// an end takes no time). The earliest started one when several run. `None` with
/// `quiet_when_busy` off.
pub fn quiet_during(quiet_when_busy: bool, events: &[Event], now: DateTime<Utc>) -> Option<&Event> {
    if !quiet_when_busy {
        return None;
    }
    let instant = |time: EventTime| match time {
        EventTime::DateTime(at) => Some(at),
        EventTime::Date(_) => None,
    };
    let runs = |event: &&Event| {
        let start = instant(event.start);
        let end = event.end.and_then(instant);
        matches!((start, end), (Some(start), Some(end)) if start <= now && now < end)
    };
    events.iter().filter(|event| event.busy()).filter(runs).min_by_key(|event| event.start)
}

/// Shows `popups` on `desktop`; each answer goes to `send` as the request [`request`] makes of it.
pub fn show_all(popups: Vec<Popup>, desktop: &dyn Desktop, send: &Arc<dyn Fn(Request) + Send + Sync>) {
    for popup in popups {
        let send = Arc::clone(send);
        let answered = popup.clone();
        desktop.show(
            &popup,
            Box::new(move |answer| {
                if let Some(request) = request(answer, &answered, Utc::now()) {
                    send(request);
                }
            }),
        );
    }
}

/// Watches the inbox of `core` and shows its popups on `desktop` until the inbox goes away. The
/// daemon runs it once, on its runtime. Looks after every change of the inbox, and every
/// [`TICK`] while something waits ([`Popups::waiting`]).
pub async fn run(core: Arc<Core>, desktop: Arc<dyn Desktop>) {
    let mut changes = core.inbox().changes();
    let send: Arc<dyn Fn(Request) + Send + Sync> = {
        let core = Arc::clone(&core);
        Arc::new(move |request: Request| {
            let core = Arc::clone(&core);
            tokio::spawn(async move {
                if let Response::Error { code, message } = core.handle(request).await {
                    eprintln!("torroworkplace: inbox popup: the answer failed ({code}): {message}");
                }
            });
        })
    };
    let mut popups = Popups::new();
    let mut failing = false;
    loop {
        let now = Utc::now();
        match core.inbox().for_me(now, usize::MAX) {
            Ok(for_me) => {
                failing = false;
                let busy = if popups.needs_calendar(&for_me.newest) { busy_title(&core, now).await } else { None };
                let quiet = core.inbox().no_popup(&for_me.newest, now);
                // Counted before the step, which learns them (task E8.8a); quiet holds back the calm ones.
                core.inbox().count_popups(&popups.arrivals(&for_me.newest, &quiet), busy.is_some(), now);
                let mut shown = popups.step_except(now, &for_me.newest, busy.as_deref(), &quiet);
                shown.iter_mut().for_each(|popup| whole_conversation(core.inbox(), popup));
                show_all(shown, desktop.as_ref(), &send);
            }
            // Logged once per outage; the tray reports the same file.
            Err(err) if !failing => {
                eprintln!("torroworkplace: inbox popups: {err}");
                failing = true;
            }
            Err(_) => {}
        }
        let (tick, every) = (popups.waiting(), popups.tick());
        tokio::select! {
            changed = changes.changed() => {
                if changed.is_err() {
                    return;
                }
                tokio::time::sleep(SETTLE).await;
                changes.borrow_and_update();
            }
            () = tokio::time::sleep(every), if tick => {}
        }
    }
}

/// Lets "Archive" and "Mark read" of a mail popup act on every message of its entry (the
/// conversation), not only the newest. An entry whose messages cannot be read keeps the newest.
fn whole_conversation(inbox: &crate::inbox::Inbox, popup: &mut Popup) {
    if let Popup::Entry(entry) = popup
        && !entry.mail.is_empty()
        && let Ok(all) = inbox.mail_messages(&entry.id)
        && !all.is_empty()
    {
        entry.mail = all;
    }
}

/// The title of the busy event that makes it quiet at `now` ([`quiet_during`]), from the settings
/// and the calendars the suite shows. A calendar that cannot be read keeps nothing quiet: better
/// a popup too many than a missed one.
async fn busy_title(core: &Core, now: DateTime<Utc>) -> Option<String> {
    let Response::Settings { settings } = core.handle(Request::Settings).await else { return None };
    if !settings.quiet_when_busy {
        return None;
    }
    let window = Request::Events { start: now, end: now + TimeDelta::minutes(1) };
    let Response::Events { events } = core.handle(window).await else { return None };
    quiet_during(true, &events, now).map(|event| event.summary.clone())
}

/// The desktop's notifications. Linux and the BSDs show the buttons; macOS and Windows show the
/// popup without them (notify-rust has no buttons there) under the app's own sender
/// ([`crate::notify::show`]), as for approvals.
pub struct SystemDesktop;

impl Desktop for SystemDesktop {
    fn show(&self, popup: &Popup, answer: Box<dyn FnOnce(Answer) + Send>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            eprintln!("torroworkplace: cannot show notification: no runtime to wait for the answer on");
            return;
        };
        let popup = popup.clone();
        runtime.spawn(async move {
            if let Err(err) = show_popup(popup, answer).await {
                eprintln!("torroworkplace: cannot show notification: {err}");
            }
        });
    }
}

/// Shows `popup` and waits for a button or a click, at most [`POPUP_LASTS`]; then closes it.
#[cfg(all(unix, not(target_os = "macos")))]
async fn show_popup(popup: Popup, answer: Box<dyn FnOnce(Answer) + Send>) -> notify_rust::error::Result<()> {
    use notify_rust::{Hint, NotificationResponse, Urgency};
    let (summary, body) = popup.text();
    let mut notification = notify_rust::Notification::new();
    notification
        .appname(crate::notify::APP_NAME)
        // The summary is plain text by the specification; only the body may carry markup.
        .summary(&summary)
        .body(&crate::notify::plain(&body))
        .action("default", tr("Open inbox"));
    if popup.messages().is_some() {
        notification.action("archive", tr("Archive")).action("read", tr("Mark read"));
    } else if popup.entry().is_some() {
        notification.action("done", tr("Done")).action("snooze", tr("Snooze 1 h"));
    }
    if popup.urgent() {
        notification.urgency(Urgency::Critical).sound_name("message-new-instant");
    } else {
        notification.urgency(Urgency::Normal).hint(Hint::SuppressSound(true));
    }
    let handle = notification.show_async().await?;
    let mut answer = Some(answer);
    let answered = handle.wait_for_action_async(|response| {
        let given = match response {
            NotificationResponse::Default => Some(Answer::Open),
            NotificationResponse::Action(action) => match action.as_str() {
                "default" => Some(Answer::Open),
                "done" => Some(Answer::Done),
                "snooze" => Some(Answer::Snooze),
                "archive" => Some(Answer::Archive),
                "read" => Some(Answer::MarkRead),
                _ => None,
            },
            _ => None,
        };
        if let (Some(given), Some(answer)) = (given, answer.take()) {
            answer(given);
        }
    });
    tokio::select! {
        () = answered => {}
        () = tokio::time::sleep(POPUP_LASTS) => handle.close_async().await,
    }
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
async fn show_popup(popup: Popup, _answer: Box<dyn FnOnce(Answer) + Send>) -> notify_rust::error::Result<()> {
    let (summary, body) = popup.text();
    crate::notify::show(summary, body);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;
    use torro_inbox::{Sender, SourceRef};
    use torro_pim::Transparency;

    use super::*;

    fn at(minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 10, minute, second).single().expect("valid time")
    }

    /// A new, open entry "for me" from the script `backup`, updated at `updated`.
    fn item(id: &str, updated: DateTime<Utc>) -> Item {
        Item {
            id: id.into(),
            source: SourceRef::script("backup"),
            thread_key: format!("t-{id}"),
            kind: Kind::Notice,
            lane: Lane::ForMe,
            urgent: false,
            title: format!("Entry {id}"),
            body: None,
            from: None,
            agent: None,
            received: updated,
            updated,
            count: 1,
            state: State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
            facets: Vec::new(),
            task: None,
        }
    }

    fn urgent(id: &str, updated: DateTime<Utc>) -> Item {
        Item { kind: Kind::AgentWaiting, urgent: true, ..item(id, updated) }
    }

    fn popup_of(item: &Item) -> Popup {
        Popup::Entry(entry(item))
    }

    /// Rules that already took their first look, at an empty inbox.
    fn started() -> Popups {
        let mut popups = Popups::new();
        assert_eq!(popups.step(at(0, 0), &[], None), []);
        popups
    }

    #[test]
    fn the_first_look_only_learns_what_is_open() {
        let mut popups = Popups::new();
        let open = [item("a", at(0, 0)), urgent("b", at(0, 0))];
        assert!(!popups.needs_calendar(&open), "nothing arrived before the first look");
        assert_eq!(popups.step(at(0, 1), &open, None), [], "a restart does not repeat the day");
        assert_eq!(popups.step(at(0, 2), &open, None), []);
    }

    /// Task E8.6a: entries a "no popup" rule holds for come quietly, and not later either.
    #[test]
    fn entries_without_a_popup_arrive_quietly() {
        let mut popups = started();
        let open = [item("a", at(1, 0)), item("b", at(1, 0))];
        let quiet = BTreeSet::from(["a".to_string()]);
        let shown = popups.step_except(at(1, 1), &open, None, &quiet);
        assert_eq!(shown, [popup_of(&open[1])], "only b pops up");
        assert_eq!(popups.step_except(at(1, 2), &open, None, &BTreeSet::new()), [], "a stays quiet when the rule ends");
    }

    /// Task E8.8a: what is counted as a popup is what the step takes as arriving, once.
    #[test]
    fn arrivals_are_what_the_next_step_takes_for_a_popup() {
        let mut popups = Popups::new();
        let open = [item("a", at(1, 0)), item("b", at(1, 0)), Item { lane: Lane::Info, ..item("c", at(1, 0)) }];
        assert!(popups.arrivals(&open, &BTreeSet::new()).is_empty(), "the first look counts nothing");
        popups.step(at(1, 0), &[], None);
        let quiet = BTreeSet::from(["a".to_string()]);
        let ids: Vec<&str> = popups.arrivals(&open, &quiet).iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["b"], "not the quiet one, not Info");
        popups.step_except(at(1, 1), &open, None, &quiet);
        assert!(popups.arrivals(&open, &quiet).is_empty(), "known from now on");
    }

    #[test]
    fn info_seen_and_approvals_never_pop_up() {
        let mut popups = started();
        let info = Item { lane: Lane::Info, ..item("info", at(0, 1)) };
        let seen = Item { state: State::Seen, ..item("seen", at(0, 1)) };
        let approval = Item { kind: Kind::Approval, urgent: true, ..item("approval", at(0, 1)) };
        assert_eq!(popups.step(at(0, 1), &[info, seen, approval], None), []);
        assert!(!popups.waiting());
    }

    #[test]
    fn a_new_entry_pops_up_once_and_again_when_it_comes_back() {
        let mut popups = started();
        let first = item("a", at(0, 1));
        assert_eq!(popups.step(at(0, 1), std::slice::from_ref(&first), None), [popup_of(&first)]);
        assert_eq!(popups.step(at(0, 2), std::slice::from_ref(&first), None), [], "only once");
        let merged = Item { count: 2, title: "Entry a again".into(), ..item("a", at(2, 0)) };
        assert_eq!(popups.step(at(2, 0), std::slice::from_ref(&merged), None), [popup_of(&merged)]);
        // Done and opened again (seen) or snoozed: gone from the open ones, no popup.
        assert_eq!(popups.step(at(2, 1), &[], None), []);
        assert_eq!(popups.step(at(2, 2), &[Item { state: State::Seen, ..merged.clone() }], None), []);
        // Woken from snoozing: new again, with the time it was due.
        let woken = item("a", at(3, 0));
        assert_eq!(popups.step(at(3, 0), std::slice::from_ref(&woken), None), [popup_of(&woken)]);
    }

    #[test]
    fn in_a_busy_event_only_urgent_ones_pop_up_and_one_message_follows() {
        let mut popups = started();
        let (calm, waiting) = (item("calm", at(0, 1)), urgent("agent", at(0, 1)));
        let open = [calm.clone(), waiting.clone()];
        assert!(popups.needs_calendar(&open), "a calm arrival asks whether it is quiet");
        assert_eq!(popups.step(at(0, 1), &open, Some("Client call")), [popup_of(&waiting)]);
        assert!(popups.waiting(), "the spell waits for the event to end");
        let later = item("later", at(5, 0));
        let open = [calm, waiting, later];
        assert_eq!(popups.step(at(5, 0), &open, Some("Client call")), []);
        assert_eq!(
            popups.step(at(30, 0), &open, None),
            [Popup::AfterQuiet { count: 2, event: Some("Client call".into()) }]
        );
        assert!(!popups.waiting());
        assert_eq!(popups.step(at(31, 0), &open, None), [], "one message only");
    }

    #[test]
    fn the_message_after_quiet_counts_only_what_is_still_open() {
        let mut popups = started();
        let (a, b) = (item("a", at(0, 1)), item("b", at(0, 2)));
        assert_eq!(popups.step(at(0, 2), &[a.clone(), b.clone()], Some("Workshop")), []);
        // Merged during the event: still one entry. Another event follows right away.
        let b2 = item("b", at(10, 0));
        assert_eq!(popups.step(at(10, 0), &[a.clone(), b2.clone()], Some("Review")), []);
        assert_eq!(
            popups.step(at(40, 0), std::slice::from_ref(&b2), None),
            [Popup::AfterQuiet { count: 1, event: None }],
            "a was handled meanwhile; two events are not named"
        );

        let mut popups = started();
        assert_eq!(popups.step(at(0, 1), std::slice::from_ref(&a), Some("Workshop")), []);
        assert_eq!(popups.step(at(30, 0), &[], None), [], "all handled: no message");
    }

    #[test]
    fn four_at_once_are_one_popup() {
        let mut popups = started();
        let open: Vec<Item> = ["a", "b", "c", "d"].iter().map(|id| item(id, at(0, 1))).collect();
        assert_eq!(popups.step(at(0, 1), &open, None), [Popup::Bundle { count: 4, urgent: false }]);
        let open: Vec<Item> = ["a", "b", "c"].iter().map(|id| item(id, at(5, 0))).collect();
        assert_eq!(popups.step(at(5, 0), &open, None).len(), 3, "three fit");
    }

    #[test]
    fn after_three_in_a_minute_the_rest_waits_and_comes_bundled() {
        let mut popups = started();
        let mut open = Vec::new();
        for (n, id) in ["a", "b", "c"].iter().enumerate() {
            let second = u32::try_from(n).expect("small") * 10;
            open.push(item(id, at(0, second)));
            assert_eq!(popups.step(at(0, second), &open, None).len(), 1, "{id} on its own");
        }
        open.push(urgent("d", at(0, 30)));
        assert_eq!(popups.step(at(0, 30), &open, None), [], "the fourth within a minute waits");
        open.push(item("e", at(0, 40)));
        assert_eq!(popups.step(at(0, 40), &open, None), []);
        assert!(popups.waiting());
        assert_eq!(popups.step(at(0, 59), &open, None), [], "no room yet");
        assert_eq!(popups.step(at(1, 0), &open, None), [Popup::Bundle { count: 2, urgent: true }]);
        assert!(!popups.waiting());
    }

    #[test]
    fn waiting_entries_handled_meanwhile_are_dropped() {
        let mut popups = started();
        let open: Vec<Item> = ["a", "b", "c"].iter().map(|id| item(id, at(0, 1))).collect();
        assert_eq!(popups.step(at(0, 1), &open, None).len(), 3);
        let d = item("d", at(0, 2));
        assert_eq!(popups.step(at(0, 2), &[open.clone(), vec![d.clone()]].concat(), None), []);
        assert_eq!(popups.step(at(0, 3), &open, None), [], "d was done");
        assert_eq!(popups.step(at(2, 0), &open, None), []);
        assert!(!popups.waiting());
    }

    fn event(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> Event {
        Event {
            uid: "e".into(),
            collection: "a/https://cloud.example.org/cal/work/".into(),
            href: None,
            etag: None,
            summary: "Client call".into(),
            start: EventTime::DateTime(start),
            end: end.map(EventTime::DateTime),
            location: None,
            description: None,
            recurring: false,
            transparency: Transparency::Opaque,
        }
    }

    #[test]
    fn only_a_busy_timed_event_that_runs_now_makes_it_quiet() {
        let now = at(30, 0);
        let running = event(at(0, 0), Some(at(45, 0)));
        assert_eq!(quiet_during(true, std::slice::from_ref(&running), now), Some(&running));
        assert_eq!(quiet_during(false, std::slice::from_ref(&running), now), None, "quiet turned off");
        let free = Event { transparency: Transparency::Transparent, ..running.clone() };
        let day = at(0, 0).date_naive();
        let all_day = Event {
            start: EventTime::Date(day),
            end: Some(EventTime::Date(day.succ_opt().expect("day"))),
            ..running.clone()
        };
        let ended = event(at(0, 0), Some(now));
        let coming = event(at(31, 0), Some(at(50, 0)));
        let no_end = event(at(0, 0), None);
        for event in [free, all_day, ended, coming, no_end] {
            assert_eq!(quiet_during(true, std::slice::from_ref(&event), now), None, "{event:?}");
        }
        let earlier = Event { summary: "Workshop".into(), ..event(at(10, 0) - TimeDelta::hours(1), Some(at(59, 0))) };
        assert_eq!(quiet_during(true, &[running, earlier.clone()], now), Some(&earlier), "the earliest started");
    }

    #[test]
    fn texts_name_the_entry_and_its_source() {
        let plain = item("a", at(0, 0));
        assert_eq!(popup_of(&plain).text(), ("Entry a".to_string(), "backup".to_string()));
        let from = Item { from: Some(Sender { name: "Lena".into(), address: None }), ..plain };
        assert_eq!(popup_of(&from).text().1, "Lena · backup");
        let bundle = Popup::Bundle { count: 5, urgent: false };
        assert_eq!(
            bundle.text(),
            (trf("{n} new for you", &[("n", &5)]), tr("Open the inbox to see them.").to_string())
        );
        let named = Popup::AfterQuiet { count: 4, event: Some(" Kundentermin ".into()) };
        assert_eq!(
            named.text().0,
            trf("While you were in “{event}”: {n} for you", &[("event", &"Kundentermin"), ("n", &4)])
        );
        let long = Popup::AfterQuiet { count: 1, event: Some("x".repeat(100)) };
        assert!(long.text().0.contains(&format!("{}…", "x".repeat(EVENT_TITLE_MAX - 1))), "long titles are cut");
        for unnamed in [None, Some(String::new())] {
            let popup = Popup::AfterQuiet { count: 2, event: unnamed };
            assert_eq!(popup.text().0, trf("While you were in busy events: {n} for you", &[("n", &2)]));
        }
    }

    #[test]
    fn an_agent_session_pops_up_with_its_name() {
        let session = |title: Option<&str>| torro_inbox::AgentInfo {
            agent: torro_inbox::AgentKind::ClaudeCode,
            session: "s1".into(),
            project: Some("TorroWorkplace".into()),
            branch: Some("main".into()),
            pids: Vec::new(),
            title: title.map(String::from),
        };
        let waiting = Item {
            title: "Claude Code is waiting for you".into(),
            agent: Some(session(Some("Login <b>bug</b>\u{202E}"))),
            ..urgent("w", at(0, 0))
        };
        let popup = popup_of(&waiting);
        assert_eq!(
            popup.text(),
            ("Login ‹b›bug‹/b›".to_string(), "Claude Code is waiting for you · TorroWorkplace".to_string())
        );
        assert!(popup.urgent(), "the name changes nothing about urgency");
        // Without a name, or with one that is nothing but control characters: as before.
        for title in [None, Some("\u{0e} ")] {
            let unnamed = Item { agent: Some(session(title)), ..waiting.clone() };
            assert_eq!(popup_of(&unnamed).text().0, "Claude Code is waiting for you", "{title:?}");
        }
    }

    #[test]
    fn answers_become_requests() {
        let popup = popup_of(&item("a", at(0, 0)));
        let now = at(10, 0);
        let done = Request::SetItemDone { id: "a".into(), done: true, dismissed: false };
        assert_eq!(request(Answer::Done, &popup, now), Some(done));
        assert_eq!(
            request(Answer::Snooze, &popup, now),
            Some(Request::SnoozeItem { id: "a".into(), until: Some(now + TimeDelta::hours(1)) })
        );
        let open = Some(Request::OpenApp { app: "torroinbox".into(), target: None });
        assert_eq!(request(Answer::Open, &popup, now), open);
        let bundle = Popup::Bundle { count: 4, urgent: false };
        assert_eq!((request(Answer::Done, &bundle, now), request(Answer::Snooze, &bundle, now)), (None, None));
        assert_eq!(request(Answer::Open, &bundle, now), open, "a summary opens the inbox too");
    }

    /// A new mail entry "for me" from `sender` in `anna@example.org`, updated at `updated`.
    fn mail(id: &str, sender: &str, updated: DateTime<Utc>) -> Item {
        Item {
            source: SourceRef::mail("anna@example.org"),
            kind: Kind::Message,
            title: format!("Subject {id}"),
            from: Some(Sender { name: sender.into(), address: Some(format!("{}@example.com", sender.to_lowercase())) }),
            link: Some(Link::Mail { message: format!("m:{id}") }),
            ..item(id, updated)
        }
    }

    #[test]
    fn a_single_mail_waits_for_company_and_then_pops_up_with_its_buttons() {
        let mut popups = started();
        let open = [mail("a", "Lena", at(0, 1))];
        assert_eq!(popups.step(at(0, 1), &open, None), [], "collected, in case more follow");
        assert!(popups.waiting() && popups.needs_calendar(&open));
        assert_eq!(popups.tick(), MAIL_TICK);
        assert_eq!(popups.step(at(0, 8), &open, None), [], "7 s are not enough");
        let [popup] = popups.step(at(0, 9), &open, None).try_into().expect("one popup");
        assert_eq!(popup, popup_of(&open[0]));
        assert_eq!(popup.text(), ("Subject a".to_string(), "Lena · anna@example.org".to_string()));
        assert_eq!(popup.messages(), Some(["m:a".to_string()].as_slice()), "Archive and Mark read act on it");
        assert!(!popups.waiting());
        assert_eq!(popups.tick(), TICK);
        assert_eq!(popups.step(at(0, 20), &open, None), [], "only once");
    }

    #[test]
    fn mail_one_after_the_other_is_one_popup_naming_the_senders() {
        let mut popups = started();
        let mut open = vec![mail("a", "Lena", at(0, 1))];
        assert_eq!(popups.step(at(0, 1), &open, None), []);
        open.insert(0, mail("b", "Ben", at(0, 4)));
        assert_eq!(popups.step(at(0, 4), &open, None), []);
        open.insert(0, mail("c", "Lena", at(0, 7)));
        assert_eq!(popups.step(at(0, 7), &open, None), []);
        // The wait counts from the first mail, not from the last.
        let bundle = Popup::MailBundle { count: 3, senders: vec!["Lena".into(), "Ben".into()] };
        assert_eq!(popups.step(at(0, 9), &open, None), vec![bundle.clone()]);
        assert_eq!(
            bundle.text(),
            (
                trf("{n} new messages from {senders}", &[("n", &3), ("senders", &"Lena, Ben")]),
                tr("Open the inbox to see them.").to_string()
            )
        );
        assert_eq!((bundle.urgent(), bundle.entry(), bundle.messages()), (false, None, None), "no buttons");
        assert!(!popups.waiting());
    }

    #[test]
    fn a_mail_that_was_handled_meanwhile_does_not_pop_up() {
        let mut popups = started();
        let (a, b) = (mail("a", "Lena", at(0, 1)), mail("b", "Ben", at(0, 2)));
        assert_eq!(popups.step(at(0, 2), &[a.clone(), b.clone()], None), []);
        let [popup] = popups.step(at(0, 10), std::slice::from_ref(&b), None).try_into().expect("b alone");
        assert_eq!(popup, popup_of(&b), "a was read on the phone: b is a popup of its own");
        assert_eq!(popups.step(at(0, 11), &[], None), [], "a was read meanwhile");
        assert!(!popups.waiting());
    }

    #[test]
    fn mail_is_quiet_in_a_busy_event_and_counted_afterwards() {
        let mut popups = started();
        let (a, b) = (mail("a", "Lena", at(0, 1)), mail("b", "Ben", at(5, 0)));
        assert_eq!(popups.step(at(0, 1), std::slice::from_ref(&a), None), []);
        // The event began while the mail was collected: it is held back with the rest.
        assert_eq!(popups.step(at(0, 3), std::slice::from_ref(&a), Some("Workshop")), []);
        assert_eq!(popups.step(at(5, 0), &[a.clone(), b.clone()], Some("Workshop")), []);
        assert_eq!(
            popups.step(at(30, 0), &[a, b], None),
            [Popup::AfterQuiet { count: 2, event: Some("Workshop".into()) }]
        );
        assert!(!popups.waiting());
    }

    #[test]
    fn mail_without_room_in_the_minute_waits_for_room() {
        let mut popups = started();
        let mut open: Vec<Item> = ["x", "y", "z"].iter().map(|id| item(id, at(0, 1))).collect();
        assert_eq!(popups.step(at(0, 1), &open, None).len(), 3, "the minute's room is used up");
        open.insert(0, mail("a", "Lena", at(0, 2)));
        open.insert(0, mail("b", "Ben", at(0, 3)));
        assert_eq!(popups.step(at(0, 3), &open, None), []);
        assert_eq!(popups.step(at(0, 12), &open, None), [], "no room: they wait, not collected any more");
        assert_eq!(popups.tick(), TICK);
        assert_eq!(popups.step(at(1, 5), &open, None), [popup_of(&open[0]), popup_of(&open[1])], "then on their own");
    }

    #[test]
    fn mail_texts_are_foreign_text_on_one_line_without_markup() {
        let nasty = "Pay <a href=\"https://evil.example\">now</a>\n\u{202E}now & then";
        let item = Item { title: nasty.into(), ..mail("a", "<b>Lena</b>\n", at(0, 1)) };
        let (summary, body) = popup_of(&item).text();
        assert_eq!(summary, "Pay ‹a href=\"https://evil.example\"›now‹/a› now & then");
        assert_eq!(body, "‹b›Lena‹/b› · anna@example.org");
        let long = Item { title: "x".repeat(300), ..mail("b", &"y".repeat(100), at(0, 1)) };
        let (summary, body) = popup_of(&long).text();
        assert_eq!(summary.chars().count(), MAIL_SUBJECT_MAX);
        assert!(summary.ends_with('…') && body.starts_with(&format!("{}…", "y".repeat(MAIL_SENDER_MAX - 1))));
        let unnamed = Item { from: Some(Sender { name: " ".into(), address: Some("pat@example.com".into()) }), ..long };
        assert_eq!(popup_of(&unnamed).text().1, "pat@example.com · anna@example.org");
        let nobody = Item { from: None, ..item.clone() };
        assert_eq!(popup_of(&nobody).text().1, "anna@example.org");
    }

    #[test]
    fn many_senders_are_counted_after_the_third() {
        let names: Vec<String> = ["A", "B", "C", "D", "E"].iter().map(ToString::to_string).collect();
        assert_eq!(sender_list(&names[..3]), "A, B, C");
        assert_eq!(sender_list(&names), trf("{names} and {n} more", &[("names", &"A, B, C"), ("n", &2)]));
        let nameless = Popup::MailBundle { count: 4, senders: Vec::new() };
        assert_eq!(nameless.text().0, trf("{n} new for you", &[("n", &4)]));
    }

    #[test]
    fn archive_and_mark_read_act_on_the_messages_of_a_mail() {
        let mut popup = popup_of(&mail("a", "Lena", at(0, 1)));
        if let Popup::Entry(entry) = &mut popup {
            entry.mail = vec!["m:1".into(), "m:2".into()];
        }
        let now = at(10, 0);
        assert_eq!(
            request(Answer::Archive, &popup, now),
            Some(Request::MoveMail { ids: vec!["m:1".into(), "m:2".into()], to: MoveTarget::Role(MoveRole::Archive) })
        );
        assert_eq!(
            request(Answer::MarkRead, &popup, now),
            Some(Request::SetMailFlags {
                ids: vec!["m:1".into(), "m:2".into()],
                add: vec![Flag::Seen],
                remove: vec![]
            })
        );
        // Nothing else has these buttons.
        let other = popup_of(&item("b", at(0, 1)));
        let bundle = Popup::MailBundle { count: 2, senders: vec!["Lena".into()] };
        for popup in [other, bundle] {
            assert_eq!((request(Answer::Archive, &popup, now), request(Answer::MarkRead, &popup, now)), (None, None));
        }
        let open = Some(Request::OpenApp { app: "torroinbox".into(), target: None });
        assert_eq!(request(Answer::Open, &popup, now), open, "a click opens the inbox");
    }

    /// Records what it was asked to show and answers each popup with `answer`.
    struct FakeDesktop {
        shown: Mutex<Vec<Popup>>,
        answer: Answer,
    }

    impl Desktop for FakeDesktop {
        fn show(&self, popup: &Popup, answer: Box<dyn FnOnce(Answer) + Send>) {
            self.shown.lock().expect("lock").push(popup.clone());
            answer(self.answer);
        }
    }

    #[test]
    fn popups_go_to_the_desktop_and_their_answers_to_the_core() {
        let desktop = FakeDesktop { shown: Mutex::default(), answer: Answer::Snooze };
        let sent: Arc<Mutex<Vec<Request>>> = Arc::default();
        let send: Arc<dyn Fn(Request) + Send + Sync> = {
            let sent = Arc::clone(&sent);
            Arc::new(move |request| sent.lock().expect("lock").push(request))
        };
        let popups = vec![popup_of(&item("a", at(0, 0))), Popup::Bundle { count: 4, urgent: true }];
        let before = Utc::now();
        show_all(popups.clone(), &desktop, &send);
        assert_eq!(*desktop.shown.lock().expect("lock"), popups);
        let sent = sent.lock().expect("lock");
        let [Request::SnoozeItem { id, until: Some(until) }] = sent.as_slice() else { panic!("one snooze: {sent:?}") };
        assert_eq!(id, "a");
        assert!(*until >= before + SNOOZE_FOR, "an hour from the answer");
    }
}
