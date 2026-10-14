//! The panel with one entry (pattern U.3/U.5: `torro_gui::panel`, beside the list in the wide
//! layout, over it otherwise).
//!
//! Head: the source and the view as path. Title: the entry's title, wrapping; for a coding agent's
//! session with a name (task E2.6) the name, with the entry's title below it in `text_2`
//! ([`heading`]). Body: "Properties"
//! (status as dot and word, lane, source, time with how long ago, merged reports), for coding
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
//! E3.10), which opens the menu "Hide from inbox": the entry, its sender or
//! domain, "Always to Info" and "Move to junk" for mail, muting the source for the others
//! (`App::hide_choices`). An entry with no way to hide it – an approval – has no "…". In the
//! notifications window the bell stands there instead ([`Footer::bell`], task E8.6b): it opens the
//! bell's menu, as `m` and the bell of the card do; it is the crossed-out bell (`BellOff`, task
//! E8.10). "Move to Info" has the circled "i" (`app::lane_icon`), never a bell. "Make task"
//! (`t`, task E3.5) is an
//! icon button next to "Later": its words would not fit beside the others in the panel's 400, and
//! so is "Make event" beside it (`c`, task E8.5b: the hub's dialog of an event).
//! Approvals have neither – they close once decided, and a task would not decide them – and no
//! "Later" either (task E8.5): an approval waits for yes or no and must not vanish by accident.
//!
//! A mail entry (task E4.2) shows its whole mail (`mail::body`: headers, actions, attachments,
//! text) in place of its short text; the footer stays the entry's.
//!
//! Task with a way back (task E8.7b): an entry made into a task shows it in a section "Task" (the
//! card of `mail::task_card` with "Open task"; a whole mail carries it over its text instead). The
//! suite's "You finished a task from a mail" (`hub::is_follow_up`) has Reply and "Mark mail done" in
//! its footer before "Done", and neither "Later" nor "Make task".
//!
//! Commands in a message ("cargo test --workspace") should not break at their hyphens: the message
//! shows them as non-breaking hyphens ([`unbroken`]), which Inter draws the same.

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::panel::{self, Panel};
use torro_gui::theme::{self, space};

use torro_gui::widgets::{self, Anchor, Mark, Status};
use torro_gui::{datetime, style, typo};
use torro_inbox::{Action, Item, Kind, Lane, Link, State, rules};

use crate::app::{self, Message};
use crate::keys::Command;
use crate::{hub, list, mail};

/// The triggers of the menus that open for an entry, made new in every `view` and shared with the
/// floating layer ([`widgets::floating`]).
#[derive(Debug, Clone, Default)]
pub struct Anchors {
    /// The row under the cursor in the list (registered by `list.rs`).
    pub row: Anchor,
    /// The footer's "Later": the snooze menu and the picker open here while the panel is open.
    pub snooze: Anchor,
    /// The footer's "Move to …": the lane question opens here while the panel is open.
    pub lane: Anchor,
    /// The footer's "Make task": its dialog opens here while the panel is open (task E3.5).
    pub task: Anchor,
    /// The footer's "Make event": its dialog opens here while the panel is open (task E8.5b).
    pub event: Anchor,
    /// The footer's "…": the menu "Hide from inbox" opens here while the panel is open (task
    /// E3.10); in the notifications window the footer's bell, where the bell's menu opens (task
    /// E8.6b).
    pub hide: Anchor,
}

/// What the footer needs besides the entry.
#[derive(Debug, Clone)]
pub struct Footer {
    pub anchors: Anchors,
    /// The menu "Hide from inbox" has entries for this entry (`App::hide_choices`): the footer
    /// shows its "…" – or its bell with [`Footer::bell`].
    pub hide: bool,
    /// The notifications window (task E8.6b): the footer's last button is the bell ("Mute…", `m`),
    /// which opens the bell's menu, instead of "…". Only shown with [`Footer::hide`].
    pub bell: bool,
    /// A decision of an approval is on its way: Allow and Deny are disabled until it is answered.
    pub deciding: bool,
    /// Narrow windows (the panel fills the window): "Move to …" shows only its icon, with the
    /// words as tooltip; wider panels show icon and words.
    pub compact: bool,
}

/// The panel for `item`, reached through the view `view_name`; `now` for the times; the footer's
/// triage buttons register `footer.anchors`.
///
/// `letter` is the whole mail of a mail entry (task E4.2, `mail::body`: headers, actions,
/// attachments, text). It stands in place of the entry's own short text and brings its own note on
/// foreign text; `None` for every other entry.
pub fn panel<'a>(
    item: &'a Item,
    view_name: String,
    now: DateTime<Utc>,
    footer: Footer,
    letter: Option<Element<'a, Message>>,
) -> Panel<'a, Message> {
    let path = panel::path(Some(Mark::Icon(list::source_icon(&item.source))), [item.source.label(), view_name]);
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
    let mut untrusted = (item.untrusted && letter.is_none())
        .then(|| note(tr("This text comes from outside the suite and is shown as plain text.")));
    // The task made of the entry (task E8.7b); a whole mail shows it over its text itself.
    if let Some(task) = item.task.as_ref().filter(|_| letter.is_none()) {
        body = body.push(column![section(tr("Task")), panel::inset(mail::task_card(task, now))].spacing(space::xs()));
    }
    if let Some(letter) = letter {
        body = body.push(letter);
    } else if let Some(text) = item.body.as_deref().map(list::plain).filter(|text| !text.trim().is_empty()) {
        let message = typo::text(typo::body(), unbroken(&text)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        let mut block = column![message].spacing(space::s());
        if let Some(note) = untrusted.take() {
            block = block.push(note);
        }
        body = body.push(column![section(tr("Message")), panel::inset(block)].spacing(space::xs()));
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

/// The panel's title: the entry's title; for an agent's session with a name
/// ([`list::session_name`]) the name, and below it what the agent does ("Claude Code is waiting
/// for you"), so the state is not lost under the name.
fn heading<'a>(item: &Item) -> Element<'a, Message> {
    let Some(name) = list::session_name(item) else {
        return panel::title(list::plain(&item.title));
    };
    let state =
        typo::text(typo::body(), list::one_line(&item.title)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
    column![panel::title(name), state].spacing(space::xs()).into()
}

/// Status, lane, source, time and – from two on – the number of merged reports.
fn properties<'a>(item: &'a Item, now: DateTime<Utc>) -> Vec<Element<'a, Message>> {
    let (status, word) = status(item, now);
    let dot = container(widgets::status_dot(status)).center_x(icon::SIZE);
    let mut rows = vec![
        widgets::prop_row(None, row![dot, one_line_text(word)].spacing(space::s()).align_y(Alignment::Center), None),
        value_row(app::lane_icon(item.lane), lane_label(item.lane).to_owned()),
        value_row(list::source_icon(&item.source), item.source.label()),
        value_row(Icon::Clock, when(item.updated, now)),
    ];
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
        State::New | State::Seen if item.urgent => (Status::Attention, tr("Waiting for you").to_owned()),
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
/// way to hide it.
fn buttons<'a>(item: &'a Item, footer: &Footer) -> Vec<Element<'a, Message>> {
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
    buttons.push(iced::widget::space::horizontal().into());
    let lane = app::other_lane(item.lane);
    let (icon, label, ask) = (
        app::lane_icon(lane),
        trf("Move to {lane}", &[("lane", &lane_label(lane))]),
        Message::AskLane(item.id.clone()),
    );
    // Beside "…" – or beside Reply and "Mark mail done" – the words would push the last button out of
    // the panel's 400.
    let move_to = if footer.compact || footer.hide || follow_up {
        widgets::icon_button(icon, label, Some(ask))
    } else {
        widgets::text_button(Some(icon), label, style::button::ghost, Some(ask))
    };
    buttons.push(widgets::anchored(&footer.anchors.lane, move_to));
    if footer.hide {
        let hide = if footer.bell {
            widgets::icon_button(Icon::BellOff, tr("Mute…"), Some(Message::Act(item.id.clone(), Command::Hide)))
        } else {
            widgets::icon_button(Icon::Ellipsis, tr("Hide from inbox"), Some(Message::AskHide(item.id.clone())))
        };
        buttons.push(widgets::anchored(&footer.anchors.hide, hide));
    }
    buttons
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

/// `text` with every hyphen-minus as a non-breaking hyphen (U+2011, in Inter), so a command like
/// "cargo test --workspace" wraps before "--workspace" instead of between its dashes. Only for
/// showing: the entry keeps its text.
pub fn unbroken(text: &str) -> String {
    text.replace('-', "\u{2011}")
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
        Footer { anchors: Anchors::default(), hide, bell: false, deciding: false, compact: false }
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
        let _ = panel(&question, tr("Act now").to_owned(), now(), footer(false), None);
        let mut handed = sample("demo-11");
        app::demo_task(&mut handed, now());
        assert!(handed.task.as_ref().is_some_and(|task| !task.completed));
        let _ = panel(&handed, tr("Done").to_owned(), now(), footer(true), None);
    }

    #[test]
    fn narrow_panels_keep_the_same_buttons() {
        let compact = Footer { compact: true, ..footer(true) };
        assert_eq!(buttons(&sample("demo-1"), &compact).len(), buttons(&sample("demo-1"), &footer(true)).len());
    }

    #[test]
    fn commands_keep_their_dashes_together() {
        assert_eq!(unbroken("cargo test --workspace"), "cargo test \u{2011}\u{2011}workspace");
        assert_eq!(unbroken("no dashes"), "no dashes");
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

    #[test]
    fn every_sample_entry_builds_a_panel() {
        for item in demo::sample(now()) {
            let _ = panel(&item, tr("For me").to_owned(), now(), footer(false), None);
        }
    }
}
