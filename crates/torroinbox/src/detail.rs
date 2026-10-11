//! The panel with one entry (pattern U.3/U.5: `torro_gui::panel`, beside the list in the wide
//! layout, over it otherwise).
//!
//! Head: the source and the view as path. Title: the entry's title, wrapping. Body: "Properties"
//! (status as dot and word, lane, source, time, merged reports), for coding agents "Agent" (agent,
//! project, branch), the message, a link to open when it is safe ([`openable`]), and a quiet note
//! that the text comes from outside the suite. Footer: "Mark as done" / "Reopen"; approvals show
//! "Allow" and "Deny" instead, which stay disabled until the core decides approvals from here
//! (E1.1/E2.1) – an approval resolves itself once decided, so it has no "Mark as done".
//!
//! The footer also carries the triage of E3.2 for the mouse, the same as the keys: "Snooze" (`s`)
//! opens the snooze menu, and at the right "Move to …" (`l`) and "Mute …" (`m`) as icon buttons.
//! The suite as a source is never muted, so its entries have no mute button.

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
}

/// What the footer needs besides the entry.
#[derive(Debug, Clone)]
pub struct Footer {
    pub anchors: Anchors,
    /// The entry's source is muted: the mute button unmutes.
    pub muted: bool,
}

/// The panel for `item`, reached through the view `view_name`; `now` for the times; the footer's
/// triage buttons register `footer.anchors`.
pub fn panel<'a>(item: &'a Item, view_name: String, now: DateTime<Utc>, footer: Footer) -> Panel<'a, Message> {
    let path = panel::path(Some(Mark::Icon(list::source_icon(&item.source))), [item.source.label(), view_name]);
    let mut body = column![widgets::prop_section(tr("Properties"), properties(item, now))].spacing(space::L);
    if let Some(agent) = &item.agent {
        let mut rows = vec![value_row(Icon::Sparkles, agent.agent.label().to_owned())];
        if let Some(project) = &agent.project {
            rows.push(value_row(Icon::Layers, list::one_line(project)));
        }
        if let Some(branch) = &agent.branch {
            rows.push(value_row(Icon::ChartGantt, list::one_line(branch)));
        }
        body = body.push(widgets::prop_section(tr("Agent"), rows));
    }
    if let Some(text) = item.body.as_deref().map(list::plain).filter(|text| !text.trim().is_empty()) {
        let message = typo::text(typo::BODY, text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        body = body.push(column![section(tr("Message")), panel::inset(message)].spacing(space::XS));
    }
    if let Some(link) = openable(item) {
        let row = widgets::prop_row(
            Some(Mark::Icon(Icon::ArrowUpRight)),
            one_line_text(link_label(&link)),
            Some(Message::OpenLink(link)),
        );
        body = body.push(widgets::prop_section(tr("Link"), vec![row]));
    }
    let mut notes = Vec::new();
    if item.untrusted {
        notes.push(note(tr("This text comes from outside the suite and is shown as plain text.")));
    }
    if is_approval(item) {
        notes.push(note(tr("Allow and Deny are not available here yet.")));
    }
    if !notes.is_empty() {
        body = body.push(panel::inset(column(notes).spacing(space::XS)));
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

/// The link the panel offers to open: `http`/`https` pages and apps of the suite, as
/// [`rules::may_open`] allows. The agent's window needs the core's lookup (E2.4) and is left out.
pub fn openable(item: &Item) -> Option<Link> {
    let link = item.link.clone()?;
    let known_app = |app: &str| AppId::ALL.iter().any(|known| known.binary == app);
    (!matches!(link, Link::Window) && rules::may_open(item, known_app)).then_some(link)
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

/// "Sat, 3 Oct · 10:26" in local time.
fn when(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (at, today) = (at.with_timezone(&Local), now.with_timezone(&Local).date_naive());
    format!("{} · {}", datetime::format_short_date(at.date_naive(), today), datetime::format_time(at.time()))
}

/// The buttons below the line: Allow/Deny for an approval (disabled for now), else done or reopen;
/// then "Snooze" for an entry that is not done, and at the right "Move to …" and "Mute …".
fn buttons<'a>(item: &'a Item, footer: &Footer) -> Vec<Element<'a, Message>> {
    let mut buttons = decide(item);
    if !matches!(item.state, State::Done { .. }) {
        let snooze = widgets::text_button(
            Some(Icon::Clock),
            tr("Snooze"),
            style::button::secondary,
            Some(Message::AskSnooze(item.id.clone())),
        );
        buttons.push(widgets::anchored(&footer.anchors.snooze, snooze));
    }
    buttons.push(iced::widget::space::horizontal().into());
    let lane = app::other_lane(item.lane);
    let move_to = widgets::icon_button(
        app::lane_icon(lane),
        trf("Move to {lane}", &[("lane", &lane_label(lane))]),
        Some(Message::AskLane(item.id.clone())),
    );
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

/// How the entry is closed: Allow/Deny for an approval (disabled for now), else done or reopen.
fn decide<'a>(item: &'a Item) -> Vec<Element<'a, Message>> {
    if is_approval(item) {
        // Deciding needs `approvals.decide_approval`, which no `InboxSource` offers; the core
        // wires it with E1.1/E2.1. Until then the buttons say what is coming, honestly disabled.
        return item
            .actions
            .iter()
            .map(|action| {
                let icon = match action {
                    Action::Allow { .. } => Icon::Check,
                    Action::Deny { .. } => Icon::X,
                };
                widgets::text_button(Some(icon), action.label(), style::button::secondary, None)
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
        assert_eq!(openable(&sample("demo-1")), None, "the agent's window needs the core (E2.4)");
        let local_file = Item { link: Some(Link::Url { url: "file:///etc/passwd".into() }), ..deploy.clone() };
        assert_eq!(openable(&local_file), None);
        let unknown_app = Item { link: Some(Link::App { app: "rm".into() }), ..deploy };
        assert_eq!(openable(&unknown_app), None);
    }

    #[test]
    fn approvals_offer_allow_and_deny_but_no_done() {
        let approval = sample("demo-3");
        assert!(is_approval(&approval));
        assert_eq!(decide(&approval).len(), 2, "Allow and Deny");
        assert!(!is_approval(&sample("demo-1")));
        assert_eq!(decide(&sample("demo-1")).len(), 1, "Mark as done");
    }

    fn footer(muted: bool) -> Footer {
        Footer { anchors: Anchors::default(), muted }
    }

    #[test]
    fn the_footer_offers_triage_to_the_mouse() {
        // Mark as done, Snooze, the gap, Move to, Mute.
        assert_eq!(buttons(&sample("demo-1"), &footer(false)).len(), 5);
        // Allow, Deny, Snooze, the gap, Move to: the suite is never muted.
        assert_eq!(buttons(&sample("demo-3"), &footer(false)).len(), 5);
        // Reopen, the gap, Move to, Mute: a done entry is not snoozed.
        assert_eq!(buttons(&sample("demo-11"), &footer(true)).len(), 4);
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
