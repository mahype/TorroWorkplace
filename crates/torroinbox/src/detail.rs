//! The panel with one entry (pattern U.3/U.5: `torro_gui::panel`, beside the list in the wide
//! layout, over it otherwise).
//!
//! Head: the source and the view as path. Title: the entry's title, wrapping. Body: "Properties"
//! (status as dot and word, lane, source, time with how long ago, merged reports), for coding
//! agents "Agent" (agent, project, branch and – for a hook's entry – "Go to window", E2.4), the
//! message with – right under it – a quiet note that the text comes from outside the suite, and a
//! link to open when it is safe ([`openable`]). Every link opens through the core
//! (`inbox.open_item`), the window of a coding agent included.
//! Footer: "Mark as done" / "Reopen"; approvals show "Allow" and "Deny" instead, which send
//! `approvals.decide_approval` (task E3.8; disabled while a decision is on its way) – an approval
//! resolves itself once decided, so it has no "Mark as done".
//!
//! The footer also carries the triage of E3.2 for the mouse, the same as the keys: "Snooze" (`s`)
//! opens the snooze menu, and at the right "Move to Info" / "Move to For me" (`l`) as a quiet
//! text button – only its icon in narrow windows, where the words do not fit ([`Footer::compact`])
//! – and "Mute …" (`m`) as an icon button, which mutes at once with an undo (E3.2c). The suite as a
//! source is never muted, so its entries have no mute button. "Make task" (`t`, task E3.5) is an
//! icon button next to "Snooze": its words would not fit beside the others in the panel's 400.
//! Approvals have none – they close once decided, and a task would not decide them.
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
use crate::list;

/// The triggers of the menus that open for an entry, made new in every `view` and shared with the
/// floating layer ([`widgets::floating`]).
#[derive(Debug, Clone, Default)]
pub struct Anchors {
    /// The row under the cursor in the list (registered by `list.rs`).
    pub row: Anchor,
    /// The footer's "Snooze": the snooze menu and the picker open here while the panel is open.
    pub snooze: Anchor,
    /// The footer's "Move to …": the lane question opens here while the panel is open.
    pub lane: Anchor,
    /// The footer's "Make task": its dialog opens here while the panel is open (task E3.5).
    pub task: Anchor,
}

/// What the footer needs besides the entry.
#[derive(Debug, Clone)]
pub struct Footer {
    pub anchors: Anchors,
    /// The entry's source is muted: the mute button unmutes.
    pub muted: bool,
    /// A decision of an approval is on its way: Allow and Deny are disabled until it is answered.
    pub deciding: bool,
    /// Narrow windows (the panel fills the window): "Move to …" shows only its icon, with the
    /// words as tooltip; wider panels show icon and words.
    pub compact: bool,
}

/// The panel for `item`, reached through the view `view_name`; `now` for the times; the footer's
/// triage buttons register `footer.anchors`.
pub fn panel<'a>(item: &'a Item, view_name: String, now: DateTime<Utc>, footer: Footer) -> Panel<'a, Message> {
    let path = panel::path(Some(Mark::Icon(list::source_icon(&item.source))), [item.source.label(), view_name]);
    let mut body = column![widgets::prop_section(tr("Properties"), properties(item, now))].spacing(space::L);
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
    // at the end of the body, where it still speaks of the title.
    let mut untrusted =
        item.untrusted.then(|| note(tr("This text comes from outside the suite and is shown as plain text.")));
    if let Some(text) = item.body.as_deref().map(list::plain).filter(|text| !text.trim().is_empty()) {
        let message = typo::text(typo::BODY, unbroken(&text)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        let mut block = column![message].spacing(space::S);
        if let Some(note) = untrusted.take() {
            block = block.push(note);
        }
        body = body.push(column![section(tr("Message")), panel::inset(block)].spacing(space::XS));
    }
    if let Some(link) = link.filter(|link| *link != Link::Window) {
        body = body.push(widgets::prop_section(tr("Link"), vec![link_row(item, &link)]));
    }
    if let Some(note) = untrusted {
        body = body.push(panel::inset(note));
    }
    panel::panel(path, panel::title(list::plain(&item.title)), body, Message::CloseDetail)
        .footer(buttons(item, &footer))
}

/// Status, lane, source, time and – from two on – the number of merged reports.
fn properties<'a>(item: &'a Item, now: DateTime<Utc>) -> Vec<Element<'a, Message>> {
    let (status, word) = status(item, now);
    let dot = container(widgets::status_dot(status)).center_x(icon::SIZE);
    let mut rows = vec![
        widgets::prop_row(None, row![dot, one_line_text(word)].spacing(space::S).align_y(Alignment::Center), None),
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
            (Status::Inactive, trf("Snoozed until {time}", &[("time", &list::wake_text(until, now))]))
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
    let known_app = |app: &str| AppId::ALL.iter().any(|known| known.binary == app);
    rules::may_open(item, known_app).then_some(link)
}

/// The row that opens `link` of `item`: "Go to window" with a window, else the page or the app.
fn link_row<'a>(item: &Item, link: &Link) -> Element<'a, Message> {
    let mark = match link {
        Link::Window => Icon::AppWindow,
        Link::Url { .. } | Link::App { .. } => Icon::ArrowUpRight,
    };
    widgets::prop_row(Some(Mark::Icon(mark)), one_line_text(link_label(link)), Some(Message::OpenItem(item.id.clone())))
}

/// The text of the link row: the address of a page, "Open Torro Workplace" for an app.
fn link_label(link: &Link) -> String {
    match link {
        Link::Url { url } => url.clone(),
        Link::App { app } => {
            let name =
                AppId::ALL.iter().find(|known| known.binary == app).map_or(app.as_str(), |known| known.display_name);
            trf("Open {app}", &[("app", &name)])
        }
        Link::Window => tr("Go to window").to_owned(),
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
/// then "Snooze" for an entry that is not done and "Make task" for one that is no approval, and at
/// the right "Move to …" and "Mute …".
fn buttons<'a>(item: &'a Item, footer: &Footer) -> Vec<Element<'a, Message>> {
    let mut buttons = decide(item, footer.deciding);
    if !matches!(item.state, State::Done { .. }) {
        let snooze = widgets::text_button(
            Some(Icon::Clock),
            tr("Snooze"),
            style::button::secondary,
            Some(Message::AskSnooze(item.id.clone())),
        );
        buttons.push(widgets::anchored(&footer.anchors.snooze, snooze));
    }
    if !is_approval(item) {
        let task = widgets::icon_button(Icon::ListTodo, tr("Make task"), Some(Message::AskTask(item.id.clone())));
        buttons.push(widgets::anchored(&footer.anchors.task, task));
    }
    buttons.push(iced::widget::space::horizontal().into());
    let lane = app::other_lane(item.lane);
    let (icon, label, ask) = (
        app::lane_icon(lane),
        trf("Move to {lane}", &[("lane", &lane_label(lane))]),
        Message::AskLane(item.id.clone()),
    );
    let move_to = if footer.compact {
        widgets::icon_button(icon, label, Some(ask))
    } else {
        widgets::text_button(Some(icon), label, style::button::ghost, Some(ask))
    };
    buttons.push(widgets::anchored(&footer.anchors.lane, move_to));
    if app::rules_allowed(&item.source) {
        let label = item.source.label();
        let tooltip = if footer.muted {
            trf("Unmute {source}", &[("source", &label)])
        } else {
            trf("Mute {source}", &[("source", &label)])
        };
        buttons.push(widgets::icon_button(Icon::Moon, tooltip, Some(Message::AskMute(item.id.clone()))));
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
/// (`deciding`) and once the entry is done –, else done or reopen.
fn decide(item: &Item, deciding: bool) -> Vec<Element<'_, Message>> {
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
        State::New | State::Seen | State::Snoozed { .. } => widgets::text_button(
            Some(Icon::Check),
            tr("Mark as done"),
            style::button::secondary,
            Some(done(item, true)),
        ),
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
    typo::text(typo::BODY, value).style(theme::text).wrapping(Wrapping::None).into()
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
    typo::text(typo::META, text).style(theme::text_3).wrapping(Wrapping::WordOrGlyph).into()
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
        assert!(word.starts_with(tr("Snoozed until {time}").trim_end_matches("{time}")), "{word}");
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
        let unknown_app = Item { link: Some(Link::App { app: "rm".into() }), ..deploy };
        assert_eq!(openable(&unknown_app), None);
    }

    #[test]
    fn approvals_offer_allow_and_deny_but_no_done() {
        let approval = sample("demo-3");
        assert!(is_approval(&approval));
        assert_eq!(decide(&approval, false).len(), 2, "Allow and Deny");
        assert!(!is_approval(&sample("demo-1")));
        assert_eq!(decide(&sample("demo-1"), false).len(), 1, "Mark as done");
    }

    fn footer(muted: bool) -> Footer {
        Footer { anchors: Anchors::default(), muted, deciding: false, compact: false }
    }

    #[test]
    fn the_footer_offers_triage_to_the_mouse() {
        // Mark as done, Snooze, Make task, the gap, Move to, Mute.
        assert_eq!(buttons(&sample("demo-1"), &footer(false)).len(), 6);
        // Allow, Deny, Snooze, the gap, Move to: the suite is never muted, an approval no task.
        assert_eq!(buttons(&sample("demo-3"), &footer(false)).len(), 5);
        // Reopen, Make task, the gap, Move to, Mute: a done entry is not snoozed.
        assert_eq!(buttons(&sample("demo-11"), &footer(true)).len(), 5);
    }

    #[test]
    fn narrow_panels_keep_the_same_buttons() {
        let compact = Footer { compact: true, ..footer(false) };
        assert_eq!(buttons(&sample("demo-1"), &compact).len(), buttons(&sample("demo-1"), &footer(false)).len());
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
        assert_eq!(link_label(&Link::Url { url: "https://x.example".into() }), "https://x.example");
    }

    #[test]
    fn every_sample_entry_builds_a_panel() {
        for item in demo::sample(now()) {
            let _ = panel(&item, tr("For me").to_owned(), now(), footer(false));
        }
    }
}
