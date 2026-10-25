//! The panel with one entry (pattern U.3/U.5: `torro_gui::panel`, beside the list in the wide
//! layout, over it otherwise).
//!
//! Head: the source and the view as path. Title: the entry's title, wrapping; for a coding agent's
//! session with a name (task E2.6) the name, with the entry's title below it in `text_2`
//! ([`heading`]). Body: "Properties"
//! (status as dot and word – "Urgent" for an ntfy alert –, lane with why a rule put the entry there
//! – [`lane_text`], issue #128 –, source, for a service what its
//! traits say – repository and reason, app and person, topic and priority, task E5.15 –, time with
//! how long ago, merged reports), for coding
//! agents "Agent" (agent, project, branch and – for a hook's entry – "Go to window", E2.4), the
//! message with – right under it – a quiet note that the text comes from outside the suite, and a
//! link to open when it is safe ([`openable`]). Every link opens through the core
//! (`inbox.open_item`), the window of a coding agent included.
//! Footer: "Done" / "Reopen"; approvals show "Allow" and "Deny" instead, which send
//! `approvals.decide_approval` (task E3.8; disabled while a decision is on its way) – an approval
//! resolves itself once decided, so it has no "Done".
//!
//! The footer also carries the triage of E3.2 for the mouse, the same as the keys: "Later" (`s`)
//! opens the snooze menu – the words of the cards in all three windows (task E8.5: "Later" and
//! "Done", no more "Snooze" and "Mark as done") –, and at the right "Move to Info" / "Move to For me" (`l`) as a quiet
//! text button – only its icon in narrow windows, where the words do not fit ([`Footer::compact`]),
//! and beside "…", which leaves no room for them in the panel's 400 either – and "…" (`m`, task
//! E3.10), which opens the menu "Hide from Notification Hub": the entry, muting the source
//! (`App::hide_choices`). An entry with no way to hide it – an approval – has no "…". In the
//! notifications window and for mail (issue #123: mail's "Hide from Notification Hub" went into the bell's
//! menu) the bell stands there instead ([`Footer::bell`], task E8.6b): it opens the
//! bell's menu, as `m` and the bell of the card do; it is the crossed-out bell (`BellOff`, task
//! E8.10). "Move to Info" has the circled "i" (`app::lane_icon`), never a bell. "Make task"
//! (`t`, task E3.5) is an
//! icon button next to "Later": its words would not fit beside the others in the panel's 400, and
//! so is "Make event" beside it (`c`, task E8.5b: the hub's dialog of an event).
//! Approvals have neither – they close once decided, and a task would not decide them – and no
//! "Later" either (task E8.5): an approval waits for yes or no and must not vanish by accident.
//! Their dialogs fill themselves in with the person's AI on request (issue #120, the sparkles in the
//! dialog's head); there is no AI button of its own beside them. "Move to …" is its icon alone, as
//! the words would not fit beside them.
//!
//! A mail entry (task E4.2) shows its whole mail (`mail::body` with `mail::Bars::Split`: headers,
//! the mail standards, attachments, text) in place of its short text; the footer stays the entry's
//! and takes a second row (task E9.4, [`Footer::view`]): "Move to …", the bell and the mail's
//! switches "HTML | Text", "Light | Dark" and "Copy text", in the order of the reader's former Torro
//! bar (until task E11.3). So in the hub and the App Feed; the mail window's panel of a mail entry
//! is `mail::entry_panel` with the reader's head and the mail's action rail (tasks E11.4, E14.3),
//! with this panel's [`path`] and [`heading`].
//!
//! Task with a way back (task E8.7b): an entry made into a task shows it in a section "Task" (the
//! card of `mail::task_card` with "Open task"; a whole mail carries it over its text instead). The
//! suite's "You finished a task from a mail" (`hub::is_follow_up`) has Reply and "Mark mail done" in
//! its footer before "Done", and neither "Later" nor "Make task".
//!
//! The message can be selected with the mouse and copied (issue #119, `torro_gui::selectable`), and
//! "Copy text" beside its title puts all of it on the clipboard; the web addresses in it follow as
//! "Links". It shows exactly as written, so what is copied is what the source said – a command like
//! "cargo test --workspace" may break at its hyphens in a narrow panel.

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::panel::{self, Panel};
use torro_gui::selectable::{self, Selectable};
use torro_gui::theme::{self, space};

use torro_gui::widgets::{self, Anchor, Mark, RowAction, Status};
use torro_gui::{datetime, style, typo};
use torro_inbox::{Action, Item, Kind, Lane, Link, State, rules};

use crate::app::{self, Message};
use crate::keys::Command;
use crate::{hub, list, mail};

/// The triggers of the menus that open for an entry, made new in every `view` and shared with the
/// floating layer ([`widgets::floating`]). The window makes two sets (issue #122): one for the
/// buttons of the panel or the reader, one for the row (or card) under the cursor and its buttons –
/// a menu hangs at the button that opened it.
#[derive(Debug, Clone, Default)]
pub struct Anchors {
    /// The row or card under the cursor in the list (registered by `list.rs` and its kin); unused in
    /// the panel's set. In a bare list (the mail window, task E14.2: rows without buttons) every menu
    /// and dialog of the keys hangs here (`app::row_trigger`).
    pub row: Anchor,
    /// "Later": the snooze menu and the picker open here.
    pub snooze: Anchor,
    /// "Move to …": the lane question opens here.
    pub lane: Anchor,
    /// "Make task" / "Task": its dialog opens here (task E3.5).
    pub task: Anchor,
    /// "Make event": its dialog opens here (task E8.5b).
    pub event: Anchor,
    /// "…" / "Hide": the menu "Hide from Notification Hub" opens here (task E3.10); in the notifications window
    /// and for mail the bell, where the bell's menu opens (task E8.6b, issue #123).
    pub hide: Anchor,
    /// "Category…" of a mail entry (task E12.8): the list of categories and its question open here.
    pub category: Anchor,
    /// The triangle of the attachment whose menu is open (task M12.6): only that chip or row
    /// registers it (`mail::Reading::file_menu`), its menu opens here.
    pub attachment: Anchor,
    /// The triangle of "Save all ▾" over the attachments (task M12.6): the menu of all of them opens
    /// here.
    pub save_all: Anchor,
    /// "Save attachments (n)" in the rail's group File (task M12.6): the menu of "Save all" opened
    /// from the rail – or, through [`Anchors::with_rail`], at the group's icon – and from the list's
    /// context menu at the row.
    pub save_rail: Anchor,
    /// The icons of the groups of the mail's action rail (task E14.3, `mail::action_groups`): an
    /// open group hangs at its icon, and – through [`Anchors::with_rail`] – so do the menus and
    /// dialogs of its items while their row is not on screen.
    pub rail: RailAnchors,
}

/// The icons of the groups of the mail's action rail (task E14.3, decision 2 of E14 in
/// `docs/tasks/eingang.md`), one anchor per group, registered by `widgets::action_rail` (`RailGroup::at`)
/// and passed to `widgets::rail_flyout` for the group's open card. The rail lists the groups in this
/// order; a group without items is left out, its anchor then stays empty.
#[derive(Debug, Clone, Default)]
pub struct RailAnchors {
    /// Reply · Reply all · Forward.
    pub reply: Anchor,
    /// Done or Reopen · Later… · Move to … · Archive · Delete: the snooze menu and the lane
    /// question fall back to it.
    pub file: Anchor,
    /// Task · Event · Category… · Re-categorize: the dialogs of Task and Event and the list of
    /// categories fall back to it.
    pub torro: Anchor,
    /// Flag · Mark read.
    pub mark: Anchor,
    /// Mute…: the bell's menu falls back to it.
    pub mute: Anchor,
    /// Show as text or HTML · Light or Dark colours · Copy text · Close.
    pub more: Anchor,
}

impl Anchors {
    /// The anchor of a button that sends `message`, when that opens a menu or dialog: `Act` with
    /// the command of a key (`s`, `l`, `t`, `c`, `m`) from rows and cards, the `Ask…` messages of
    /// the panel and of the mail's action rail (task E14.3). `None` for every other message.
    pub fn of(&self, message: &Message) -> Option<&Anchor> {
        match message {
            Message::Act(_, Command::Snooze) | Message::AskSnooze(_) => Some(&self.snooze),
            Message::Act(_, Command::Lane) | Message::AskLane(_) => Some(&self.lane),
            Message::Act(_, Command::MakeTask) | Message::AskTask(_) | Message::AskMailTask(_) => Some(&self.task),
            Message::Act(_, Command::MakeEvent) | Message::AskEvent(_) => Some(&self.event),
            Message::Act(_, Command::Hide) | Message::AskHide(_) | Message::AskMuteMail(_) => Some(&self.hide),
            Message::AskCategory(_) => Some(&self.category),
            Message::AskSaveAttachments(_) => Some(&self.save_rail),
            _ => None,
        }
    }

    /// The same anchors with the items' anchors falling back to the icon of their group in the
    /// mail's action rail (task E14.3, [`RailAnchors`]): Later and "Move to …" to File, Task, Event
    /// and "Category…" to Torro, the bell to Mute. Expanded, an item's row registers its own anchor
    /// and its menu hangs there; collapsed – or once the open group closed on the click – the row
    /// is not on screen and the menu hangs at the group's icon. The rail is fixed beside the mail,
    /// so that icon is always in view. Where no rail is drawn (the hub, the App Feed) the fallbacks
    /// stay empty and change nothing. "Save attachments (n)" falls back to File too (task M12.6).
    /// Make the window's set with it once per `view`.
    pub fn with_rail(self) -> Anchors {
        let rail = self.rail.clone();
        Anchors {
            snooze: self.snooze.or(&rail.file),
            lane: self.lane.or(&rail.file),
            task: self.task.or(&rail.torro),
            event: self.event.or(&rail.torro),
            category: self.category.or(&rail.torro),
            hide: self.hide.or(&rail.mute),
            save_rail: self.save_rail.or(&rail.file),
            ..self
        }
    }

    /// `actions` of the row or card under the cursor (`shown`), each that opens a menu registering
    /// its anchor ([`Anchors::of`]); other rows' as they are, so no anchor is registered twice.
    pub fn mark<'a>(&self, shown: bool, actions: Vec<RowAction<'a, Message>>) -> Vec<RowAction<'a, Message>> {
        if !shown {
            return actions;
        }
        actions
            .into_iter()
            .map(|action| match self.of(&action.on_press).cloned() {
                Some(anchor) => action.at(&anchor),
                None => action,
            })
            .collect()
    }

    /// `button`, which sends `message`, registering its anchor ([`Anchors::of`]) while its row or
    /// card is under the cursor (`shown`).
    pub fn wrap<'a>(
        &self,
        shown: bool,
        message: Option<&Message>,
        button: Element<'a, Message>,
    ) -> Element<'a, Message> {
        widgets::anchored_if(message.filter(|_| shown).and_then(|message| self.of(message)), button)
    }
}

/// What the footer needs besides the entry.
#[derive(Debug, Clone)]
pub struct Footer {
    pub anchors: Anchors,
    /// The entry has a menu for `m` (`App::hidable`): the footer shows its "…" – or its bell with
    /// [`Footer::bell`].
    pub hide: bool,
    /// The notifications window (task E8.6b) and mail (issue #123): the footer's last button is the
    /// bell ("Mute…", `m`), which opens the bell's menu, instead of "…". Only shown with
    /// [`Footer::hide`].
    pub bell: bool,
    /// A decision of an approval is on its way: Allow and Deny are disabled until it is answered.
    pub deciding: bool,
    /// Narrow windows (the panel fills the window): "Move to …" shows only its icon, with the
    /// words as tooltip; wider panels show icon and words.
    pub compact: bool,
    /// A mail entry's whole mail in the panel (task E9.4, `mail::Reading::view_tools`): the footer
    /// takes two rows – Done, Later and the task and event buttons with their AI ones in the first;
    /// "Move to …", the bell and the view group (`mail::view_controls`: "HTML | Text", "Light | Dark",
    /// "Copy text") in the second, in the order of the reader's former Torro bar. `None` for every other
    /// entry: one row as before.
    pub view: Option<mail::ViewTools>,
}

/// The panel for `item`, reached through the view `view_name`; `now` for the times; the footer's
/// triage buttons register `footer.anchors`.
///
/// `letter` is the whole mail of a mail entry (task E4.2, `mail::body` with `mail::Bars::Split`:
/// headers, actions, attachments, text). It stands in place of the entry's own short text and brings its own note on
/// foreign text; `None` for every other entry.
///
/// `text` is the entry's message as the window keeps it to select and copy (issue #119): made from
/// [`message_text`] of `item`; unused beside a `letter`.
pub fn panel<'a>(
    item: &'a Item,
    view_name: String,
    now: DateTime<Utc>,
    footer: Footer,
    letter: Option<Element<'a, Message>>,
    text: &'a Selectable,
) -> Panel<'a, Message> {
    let path = path(item, view_name);
    let mut body = column![widgets::prop_section(tr("Properties"), properties(item, now))].spacing(space::l());
    let link = openable(item);
    if let Some(agent) = &item.agent {
        let mut rows = vec![value_row(Icon::Sparkles, agent.agent.label().to_owned())];
        if let Some(project) = &agent.project {
            rows.push(value_row(Icon::FolderGit2, list::one_line(project)));
        }
        if let Some(branch) = &agent.branch {
            rows.push(value_row(Icon::GitBranch, list::one_line(branch)));
        }
        // The agent's window belongs to the agent, not under "Link".
        if link == Some(Link::Window) {
            rows.push(link_row(item, &Link::Window));
        }
        body = body.push(widgets::prop_section(tr("Agent"), rows));
    }
    // The note on foreign text belongs to the text: right under the message, or – without one –
    // at the end of the body, where it still speaks of the title. A whole mail has its own.
    let mut untrusted = (item.untrusted && letter.is_none()).then(|| note(foreign_note()));
    // The task made of the entry (task E8.7b); a whole mail shows it over its text itself.
    if let Some(task) = item.task.as_ref().filter(|_| letter.is_none()) {
        body = body.push(column![section(tr("Task")), panel::inset(mail::task_card(task, now))].spacing(space::xs()));
    }
    if let Some(letter) = letter {
        body = body.push(letter);
    } else if !text.text().trim().is_empty() {
        let mut block = column![text.view(typo::body(), theme::text_2, Message::Text)].spacing(space::s());
        if let Some(note) = untrusted.take() {
            block = block.push(note);
        }
        body = body.push(column![message_title(), panel::inset(block)].spacing(space::xs()));
        // The addresses in the text, but the entry's own link, which has its row below.
        let own = match &link {
            Some(Link::Url { url }) => Some(url.as_str()),
            _ => None,
        };
        let urls = selectable::web_links(text.text()).into_iter().filter(|url| Some(url.as_str()) != own).collect();
        if let Some(links) = selectable::links_section(urls, Message::OpenMailLink) {
            body = body.push(links);
        }
    }
    // A mail is shown in this app itself (task E4.2), not opened from a link row.
    if let Some(link) = link.filter(|link| !matches!(link, Link::Window | Link::Mail { .. })) {
        body = body.push(widgets::prop_section(tr("Link"), vec![link_row(item, &link)]));
    }
    if let Some(note) = untrusted {
        body = body.push(panel::inset(note));
    }
    panel::panel(path, heading(item), body, Message::CloseDetail).footer(buttons(item, &footer))
}

/// The panel's path: the source's icon, the source and `view_name` ("Mail › For me"). The mail
/// window's panel of a mail entry has the same head (task E11.4, `mail::entry_panel`).
pub(crate) fn path<'a>(item: &Item, view_name: String) -> Element<'a, Message> {
    panel::path(Some(Mark::Icon(list::source_icon(&item.source))), [item.source.label(), view_name])
}

/// The panel's title: the entry's title; for an agent's session with a name
/// ([`list::session_name`]) the name, and below it what the agent does ("Claude Code is waiting
/// for you"), so the state is not lost under the name. Also the title of the mail window's panel of
/// a mail entry (task E11.4, `mail::entry_panel`).
pub(crate) fn heading<'a>(item: &Item) -> Element<'a, Message> {
    let Some(name) = list::session_name(item) else {
        return panel::title(list::plain(&item.title));
    };
    let state =
        typo::text(typo::body(), list::one_line(&item.title)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
    column![panel::title(name), state].spacing(space::xs()).into()
}

/// Status, lane, source, what a service's traits say (task E5.15, `list::facet_words`: repository
/// and reason, app and person, kind of conversation, topic and priority – each a row with its
/// icon, foreign values as plain text), time and – from two on – the number of merged reports.
pub(crate) fn properties<'a>(item: &'a Item, now: DateTime<Utc>) -> Vec<Element<'a, Message>> {
    let (status, word) = status(item, now);
    let dot = container(widgets::status_dot(status)).center_x(icon::SIZE);
    let mut rows = vec![
        widgets::prop_row(None, row![dot, one_line_text(word)].spacing(space::s()).align_y(Alignment::Center), None),
        value_row(app::lane_icon(item.lane), lane_text(item)),
        value_row(list::source_icon(&item.source), item.source.label()),
    ];
    rows.extend(list::facet_words(item).into_iter().map(|(mark, word)| value_row(mark, word)));
    rows.push(value_row(Icon::Clock, when(item.updated, now)));
    if item.count > 1 {
        rows.push(value_row(Icon::Repeat, trf("{n} reports", &[("n", &item.count)])));
    }
    rows
}

/// The state as dot and word (design: status never by colour alone).
pub fn status(item: &Item, now: DateTime<Utc>) -> (Status, String) {
    match item.state {
        State::Done { .. } => (Status::Ok, tr("Done").to_owned()),
        State::Snoozed { until } => {
            // "Later", as the buttons say it in all three windows (task E8.1b).
            (Status::Inactive, trf("Later until {time}", &[("time", &list::wake_text(until, now))]))
        }
        State::New | State::Seen if item.kind == Kind::AccountProblem => {
            (Status::Error, tr("Needs your attention").to_owned())
        }
        // "Urgent" for an alert (task E5.15), which waits for nobody.
        State::New | State::Seen if item.urgent => (Status::Attention, list::waiting_word(item).to_owned()),
        State::New => (Status::Inactive, tr("New").to_owned()),
        State::Seen => (Status::Inactive, tr("Seen").to_owned()),
    }
}

/// An approval: decided through its own buttons, not marked done.
pub fn is_approval(item: &Item) -> bool {
    item.kind == Kind::Approval
}

/// The link the panel offers to open, as [`rules::may_open`] allows: `http`/`https` pages, apps of
/// the suite, and the window of the agent behind a hook's entry (task E2.4). The core links that
/// window only when it can be looked for (Linux, not inside tmux), so elsewhere there is none to
/// offer. The core checks again when it opens ([`Message::OpenItem`]).
pub fn openable(item: &Item) -> Option<Link> {
    let link = item.link.clone()?;
    // A binary or a window class (task E8.1b), as the core reads it; anything else is no app.
    let known_app = |app: &str| AppId::find(app).is_some();
    rules::may_open(item, known_app).then_some(link)
}

/// The row that opens `link` of `item`: "Go to window" with a window, else the page or the app.
fn link_row<'a>(item: &Item, link: &Link) -> Element<'a, Message> {
    let mark = match link {
        Link::Window => Icon::AppWindow,
        Link::Url { .. } | Link::App { .. } | Link::Mail { .. } => Icon::ArrowUpRight,
    };
    widgets::prop_row(Some(Mark::Icon(mark)), one_line_text(link_label(link)), Some(Message::OpenItem(item.id.clone())))
}

/// The text of the link row: the address of a page, "Open Torro Workplace" for an app.
fn link_label(link: &Link) -> String {
    match link {
        Link::Url { url } => url.clone(),
        Link::App { app } => {
            let name = AppId::find(app).map_or(app.as_str(), |known| known.display_name);
            trf("Open {app}", &[("app", &name)])
        }
        Link::Window => tr("Go to window").to_owned(),
        Link::Mail { .. } => trf("Open {app}", &[("app", &AppId::INBOX.display_name)]),
    }
}

/// "Sat, 3 Oct · 10:26 · 4 min ago": the moment in local time and how long ago it was.
fn when(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (local, today) = (at.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    format!(
        "{} · {} · {}",
        datetime::format_short_date(local.date_naive(), today),
        datetime::format_time(local.time()),
        list::ago_long(at, now)
    )
}

/// The buttons below the line: Allow/Deny for an approval, else done or reopen;
/// then "Later" for an entry that is not done and no approval, "Make task" and "Make event" for one
/// that is no approval, and at the right "Move to …" and "…" (hide from the inbox) when there is a
/// way to hide it. With a whole mail ([`Footer::view`], task E9.4) in two rows: those that finish the
/// entry first, "Move to …", the bell and the mail's view group under them.
fn buttons<'a>(item: &'a Item, footer: &Footer) -> Vec<Element<'a, Message>> {
    let (finish, view) = button_groups(item, footer);
    match footer.view {
        None => finish.into_iter().chain([iced::widget::space::horizontal().into()]).chain(view).collect(),
        Some(_) => {
            let row = |buttons: Vec<Element<'a, Message>>| row(buttons).spacing(space::s()).align_y(Alignment::Center);
            vec![column![row(finish), row(view)].spacing(space::s()).into()]
        }
    }
}

/// The footer's buttons in two groups ([`buttons`]): what finishes the entry – decide, Done, Later,
/// make a task or an event – and what moves, mutes or shows it – "Move to …",
/// "…" or the bell, and for a whole mail its view group (`mail::view_controls`).
fn button_groups<'a>(item: &'a Item, footer: &Footer) -> (Vec<Element<'a, Message>>, Vec<Element<'a, Message>>) {
    let follow_up = hub::is_follow_up(item) && item.state.is_open();
    let mut buttons = Vec::new();
    // "You finished a task from a mail" (task E8.7b): answer the mail or mark it done, before
    // closing the question; it is a task already and waits for nothing, so no "Make task", no "Later".
    if follow_up {
        let id = || item.id.clone();
        let reply = Some(Message::Act(id(), Command::Reply));
        buttons.push(widgets::text_button(Some(Icon::Reply), tr("Reply"), style::button::secondary, reply));
        let done = Some(Message::MarkMailDone(id()));
        buttons.push(widgets::text_button(Some(Icon::Check), tr("Mark mail done"), style::button::secondary, done));
    }
    buttons.extend(decide(item, footer.deciding));
    if !matches!(item.state, State::Done { .. }) && !is_approval(item) && !follow_up {
        let snooze = widgets::text_button(
            Some(Icon::Clock),
            tr("Later"),
            style::button::secondary,
            Some(Message::AskSnooze(item.id.clone())),
        );
        buttons.push(widgets::anchored(&footer.anchors.snooze, snooze));
    }
    if !is_approval(item) && !follow_up {
        let task = widgets::icon_button(Icon::ListTodo, tr("Make task"), Some(Message::AskTask(item.id.clone())));
        buttons.push(widgets::anchored(&footer.anchors.task, task));
        let event =
            widgets::icon_button(Icon::CalendarPlus, tr("Make event"), Some(Message::AskEvent(item.id.clone())));
        buttons.push(widgets::anchored(&footer.anchors.event, event));
    }
    let mut side = Vec::new();
    let lane = app::other_lane(item.lane);
    let (icon, label, ask) = (
        app::lane_icon(lane),
        trf("Move to {lane}", &[("lane", &lane_label(lane))]),
        Message::AskLane(item.id.clone()),
    );
    // Beside "…" – or beside Reply and "Mark mail done", or "Make task" and "Make event" – the words
    // would push the last button out of the panel's 400.
    let move_to = if footer.compact || footer.hide || follow_up || !is_approval(item) {
        widgets::icon_button(icon, label, Some(ask))
    } else {
        widgets::text_button(Some(icon), label, style::button::ghost, Some(ask))
    };
    side.push(widgets::anchored(&footer.anchors.lane, move_to));
    if footer.hide {
        let hide = if footer.bell {
            // `AskHide` like "…", not `Act`: the bell's menu hangs at this bell (issue #122), not at
            // the row; in the notifications window and for mail it opens the bell's menu.
            widgets::icon_button(Icon::BellOff, tr("Mute…"), Some(Message::AskHide(item.id.clone())))
        } else {
            widgets::icon_button(
                Icon::Ellipsis,
                tr("Hide from Notification Hub"),
                Some(Message::AskHide(item.id.clone())),
            )
        };
        side.push(widgets::anchored(&footer.anchors.hide, hide));
    }
    side.extend(footer.view.map(mail::view_controls).unwrap_or_default());
    (buttons, side)
}

/// The entry's lane and, when a rule put it there, why (issue #128): "Info · Mailing list
/// (List-Id)". A mail missing under "For me" says so wherever it shows: the panel's lane row and
/// the reader of the mail window (`mail::reader`).
pub(crate) fn lane_text(item: &Item) -> String {
    let lane = lane_label(item.lane);
    match item.reason {
        Some(reason) => trf("{lane} · {reason}", &[("lane", &lane), ("reason", &reason.label())]),
        None => lane.to_owned(),
    }
}

/// "For me" or "Info".
fn lane_label(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// How the entry is closed: Allow/Deny for an approval – disabled while a decision is on its way
/// (`deciding`) and once the entry is done –, else done or reopen. The overview's card of a
/// waiting approval shows the same buttons (task E3.9).
pub(crate) fn decide(item: &Item, deciding: bool) -> Vec<Element<'_, Message>> {
    if is_approval(item) {
        let open = item.state.is_open() && !deciding;
        return item
            .actions
            .iter()
            .map(|action| {
                let (icon, approval, allow) = match action {
                    Action::Allow { approval } => (Icon::Check, approval, true),
                    Action::Deny { approval } => (Icon::X, approval, false),
                };
                let decide = open.then(|| Message::Decide { approval: approval.clone(), allow });
                widgets::text_button(Some(icon), action.label(), style::button::secondary, decide)
            })
            .collect();
    }
    let button = match item.state {
        State::Done { .. } => {
            widgets::text_button(None, tr("Reopen"), style::button::secondary, Some(done(item, false)))
        }
        State::New | State::Seen | State::Snoozed { .. } => {
            widgets::text_button(Some(Icon::Check), tr("Done"), style::button::secondary, Some(done(item, true)))
        }
    };
    vec![button]
}

fn done(item: &Item, done: bool) -> Message {
    Message::SetDone(item.id.clone(), done)
}

/// A property row with an icon and a one-line value.
fn value_row<'a>(which: Icon, value: String) -> Element<'a, Message> {
    widgets::prop_row(Some(Mark::Icon(which)), one_line_text(value), None)
}

fn one_line_text<'a>(value: String) -> Element<'a, Message> {
    typo::text(typo::body(), value).style(theme::text).wrapping(Wrapping::None).into()
}

/// A title over free content in the body, lined up with the property sections' titles.
fn section<'a>(title: &'a str) -> Element<'a, Message> {
    panel::inset(widgets::section_title(title))
}

/// The entry's message as the panel shows, selects and copies it (issue #119): its body, plain
/// ([`list::plain`]); empty without one.
pub fn message_text(item: &Item) -> String {
    item.body.as_deref().map(list::plain).unwrap_or_default()
}

/// "Message" over the entry's text, with "Copy text" at the right (issue #119, `mail::copy_button`).
fn message_title<'a>() -> Element<'a, Message> {
    let title = container(widgets::section_title(tr("Message"))).width(Fill);
    panel::inset(row![title, mail::copy_button()].spacing(space::s()).align_y(Alignment::Center))
}

/// The note under foreign text, in this panel and under a whole mail (`mail::body`, task E8.13):
/// short enough for one line in the wide panel (`note_room`), so no word stands alone on a second
/// line.
pub(crate) fn foreign_note() -> &'static str {
    tr("Text from outside the suite, shown as plain text.")
}

/// The width a [`note`] has in the panel beside the list (`size::panel()`): less the padding of
/// the panel's body and of `panel::inset`, and the scroller beside them.
#[cfg(test)]
fn note_room() -> f32 {
    theme::size::panel() - 6.0 * space::s()
}

/// A quiet sentence under the content (`META`, `text_3`).
fn note<'a>(text: &'a str) -> Element<'a, Message> {
    typo::text(typo::meta(), text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use torro_inbox::demo;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    fn sample(id: &str) -> Item {
        demo::sample(now()).into_iter().find(|i| i.id == id).expect("in the sample")
    }

    /// Issue #122: every button that opens a menu registers the anchor of that menu – the same for
    /// a row's `Act` and the panel's `Ask…` –, other buttons none.
    #[test]
    fn a_button_that_opens_a_menu_names_its_anchor() {
        let anchors = Anchors::default();
        let id = || "demo-6".to_owned();
        let at = |message: Message, which: &Anchor| anchors.of(&message).is_some_and(|got| std::ptr::eq(got, which));
        assert!(at(Message::Act(id(), Command::Snooze), &anchors.snooze));
        assert!(at(Message::AskSnooze(id()), &anchors.snooze));
        assert!(at(Message::Act(id(), Command::Lane), &anchors.lane));
        assert!(at(Message::AskLane(id()), &anchors.lane));
        assert!(at(Message::Act(id(), Command::MakeTask), &anchors.task));
        assert!(at(Message::AskMailTask(id()), &anchors.task));
        assert!(at(Message::AskEvent(id()), &anchors.event));
        assert!(at(Message::Act(id(), Command::Hide), &anchors.hide));
        assert!(at(Message::AskHide(id()), &anchors.hide));
        assert!(at(Message::AskMuteMail(id()), &anchors.hide));
        // Task E14.3: "Category…" in the mail's action rail.
        assert!(at(Message::AskCategory(id()), &anchors.category));
        assert!(anchors.of(&Message::Act(id(), Command::Done)).is_none(), "Done opens no menu");
        let actions = vec![
            RowAction::new(Icon::Check, "Done", "E", Message::Act(id(), Command::Done)),
            RowAction::new(Icon::Clock, "Later", "S", Message::Act(id(), Command::Snooze)),
        ];
        let marked = anchors.mark(true, actions.clone());
        assert!(marked[0].anchor.is_none() && marked[1].anchor.is_some(), "only Later");
        assert!(anchors.mark(false, actions).iter().all(|action| action.anchor.is_none()), "not under the cursor");
    }

    #[test]
    fn states_read_as_dot_and_word() {
        assert_eq!(status(&sample("demo-1"), now()), (Status::Attention, tr("Waiting for you").to_owned()));
        assert_eq!(status(&sample("demo-5"), now()), (Status::Error, tr("Needs your attention").to_owned()));
        assert_eq!(status(&sample("demo-6"), now()), (Status::Inactive, tr("New").to_owned()));
        assert_eq!(status(&sample("demo-8"), now()), (Status::Inactive, tr("Seen").to_owned()));
        assert_eq!(status(&sample("demo-11"), now()), (Status::Ok, tr("Done").to_owned()));
        let (dot, word) = status(&sample("demo-10"), now());
        assert_eq!(dot, Status::Inactive);
        assert!(word.starts_with(tr("Later until {time}").trim_end_matches("{time}")), "{word}");
        // Task E5.15: an alert is urgent, nobody waits there.
        assert_eq!(status(&sample("demo-18"), now()), (Status::Attention, tr("Urgent").to_owned()));
    }

    /// Task E5.15: a service's traits stand among the properties, one row each.
    #[test]
    fn a_service_shows_its_traits_as_properties() {
        let plain = properties(&sample("demo-6"), now()).len();
        assert_eq!(properties(&sample("demo-14"), now()).len(), plain + 2, "repository and reason");
        assert_eq!(properties(&sample("demo-16"), now()).len(), plain + 2, "app and person");
        assert_eq!(properties(&sample("demo-17"), now()).len() - 1, plain + 1, "kind, and two reports");
        assert_eq!(properties(&sample("demo-18"), now()).len(), plain + 2, "topic and priority");
    }

    #[test]
    fn only_safe_links_are_offered() {
        let deploy = sample("demo-6");
        assert!(matches!(openable(&deploy), Some(Link::Url { .. })), "https pages open");
        assert_eq!(openable(&sample("demo-5")), Some(Link::App { app: "torroworkplace".into() }));
        assert_eq!(openable(&sample("demo-1")), Some(Link::Window), "a hook's agent: its window");
        let notice_with_window = Item { link: Some(Link::Window), ..deploy.clone() };
        assert_eq!(openable(&notice_with_window), None, "a window only on entries of hooks");
        let local_file = Item { link: Some(Link::Url { url: "file:///etc/passwd".into() }), ..deploy.clone() };
        assert_eq!(openable(&local_file), None);
        let unknown_app = Item { link: Some(Link::App { app: "rm".into() }), ..deploy.clone() };
        assert_eq!(openable(&unknown_app), None);
        // Task E8.1b: a window of the suite by its class; an unknown class is no app.
        let mail = Link::App { app: "torro-inbox-mail".into() };
        assert_eq!(openable(&Item { link: Some(mail.clone()), ..deploy.clone() }), Some(mail));
        let unknown_class = Item { link: Some(Link::App { app: "torro-inbox-secret".into() }), ..deploy };
        assert_eq!(openable(&unknown_class), None);
    }

    #[test]
    fn approvals_offer_allow_and_deny_but_no_done() {
        let approval = sample("demo-3");
        assert!(is_approval(&approval));
        assert_eq!(decide(&approval, false).len(), 2, "Allow and Deny");
        assert!(!is_approval(&sample("demo-1")));
        assert_eq!(decide(&sample("demo-1"), false).len(), 1, "Done");
    }

    fn footer(hide: bool) -> Footer {
        Footer { anchors: Anchors::default(), hide, bell: false, deciding: false, compact: false, view: None }
    }

    #[test]
    fn the_footer_offers_triage_to_the_mouse() {
        // Done, Later, Make task, Make event (task E8.5b), the gap, Move to, "…".
        assert_eq!(buttons(&sample("demo-1"), &footer(true)).len(), 7);
        // Allow, Deny, the gap, Move to: an approval has no way to hide it, no task, no event and no
        // Later.
        assert_eq!(buttons(&sample("demo-3"), &footer(false)).len(), 4);
        // Reopen, Make task, Make event, the gap, Move to, "…": a done entry is not snoozed.
        assert_eq!(buttons(&sample("demo-11"), &footer(true)).len(), 6);
    }

    /// Task E8.7b: the question after a finished task answers or closes the mail, and is no task to
    /// make or to put off; an entry made into a task shows it.
    #[test]
    fn a_finished_task_asks_to_reply_or_mark_the_mail_done() {
        let mut question = sample("demo-9");
        app::demo_task(&mut question, now());
        assert!(hub::is_follow_up(&question));
        // Reply, Mark mail done, Done, the gap, Move to.
        assert_eq!(buttons(&question, &footer(false)).len(), 5);
        let text = Selectable::new(&message_text(&question));
        let _ = panel(&question, tr("Act now").to_owned(), now(), footer(false), None, &text);
        let mut handed = sample("demo-11");
        app::demo_task(&mut handed, now());
        assert!(handed.task.as_ref().is_some_and(|task| !task.completed));
        let text = Selectable::new(&message_text(&handed));
        let _ = panel(&handed, tr("Done").to_owned(), now(), footer(true), None, &text);
    }

    /// Task E9.4: with a whole mail the footer takes two rows – what finishes the entry, then "Move
    /// to …", the bell and the mail's view group –; every other entry keeps its one row.
    #[test]
    fn a_mail_footer_takes_the_view_group_in_a_second_row() {
        let item = sample("demo-1");
        let view = mail::ViewTools {
            format: Some(torro_core::settings::MailFormat::Html),
            scheme: Some(torro_htmlview::Scheme::Light),
            copy: true,
        };
        let with = Footer { view: Some(view), ..footer(true) };
        let (finish, side) = button_groups(&item, &with);
        let (plain_finish, plain_side) = button_groups(&item, &footer(true));
        assert_eq!(finish.len(), plain_finish.len(), "Done, Later, the task and event buttons");
        assert_eq!(plain_side.len(), 2, "Move to and \"…\"");
        assert_eq!(side.len(), plain_side.len() + 3, "HTML | Text, Light | Dark, Copy text");
        assert_eq!(buttons(&item, &with).len(), 1, "two rows in one column");
        let loading = Footer { view: Some(mail::ViewTools::default()), ..footer(true) };
        assert_eq!(button_groups(&item, &loading).1.len(), 2, "nothing to switch or copy while it loads");
    }

    #[test]
    fn narrow_panels_keep_the_same_buttons() {
        let compact = Footer { compact: true, ..footer(true) };
        assert_eq!(buttons(&sample("demo-1"), &compact).len(), buttons(&sample("demo-1"), &footer(true)).len());
    }

    /// Issue #119: the message is shown and copied as written, so a command keeps its dashes.
    #[test]
    fn the_message_is_copied_as_written() {
        let mut item = sample("demo-1");
        item.body = Some("Run cargo test --workspace\u{7}".into());
        assert_eq!(message_text(&item), "Run cargo test --workspace");
        item.body = None;
        assert_eq!(message_text(&item), "");
    }

    /// Issue #128: an entry a rule put in its lane says why beside the lane; moved by hand, only
    /// the lane.
    #[test]
    fn the_lane_says_why_a_rule_put_the_entry_there() {
        let plain = sample("demo-6");
        assert_eq!(lane_text(&plain), lane_label(plain.lane));
        let listed = Item { lane: Lane::Info, reason: Some(torro_inbox::LaneReason::List), ..plain };
        let reason = torro_inbox::LaneReason::List.label();
        assert_eq!(lane_text(&listed), trf("{lane} · {reason}", &[("lane", &tr("Info")), ("reason", &reason)]));
    }

    #[test]
    fn the_time_says_how_long_ago_too() {
        let item = sample("demo-1");
        assert!(when(item.updated, now()).ends_with(&trf("{n} min ago", &[("n", &4)])));
    }

    #[test]
    fn app_links_name_the_app() {
        assert_eq!(
            link_label(&Link::App { app: "torroworkplace".into() }),
            trf("Open {app}", &[("app", &"Torro Workplace")])
        );
        assert_eq!(
            link_label(&Link::App { app: "torro-inbox-mail".into() }),
            trf("Open {app}", &[("app", &AppId::INBOX_MAIL.display_name)]),
            "a window class names its window"
        );
        assert_eq!(link_label(&Link::Url { url: "https://x.example".into() }), "https://x.example");
    }

    /// Task E8.13: the note on foreign text takes one line in the wide panel, in either language –
    /// no word of its own on a second line.
    #[test]
    fn the_note_on_foreign_text_fits_one_line_in_the_wide_panel() {
        assert!(typo::text_width(foreign_note(), typo::meta()) <= note_room(), "{}", foreign_note());
    }

    #[test]
    fn every_sample_entry_builds_a_panel() {
        for item in demo::sample(now()) {
            let text = Selectable::new(&message_text(&item));
            let _ = panel(&item, tr("For me").to_owned(), now(), footer(false), None, &text);
        }
    }
}
