//! The approvals page of the control centre (task A.8 in `docs/tasks/api-mcp.md`, concept in
//! `docs/api.md`, "Freigaben: der Mensch entscheidet"): what agents wait for, and what was decided.
//!
//! The page shows the answer of `approvals.list_approvals` from [`Data::approvals`]; the shell
//! reloads it every few seconds like everything else, which also keeps the time left current.
//! Two groups:
//!
//! - **Waiting for you**: one row per open approval, the one expiring first on top. The summary the
//!   core built (`approvals::summary`, with titles and names from its own cache) on top, wrapped
//!   rather than cut off, because it is what the person decides on; below it the client's name and
//!   the time left. At the right "Deny" and "Allow", which send `approvals.decide_approval` through
//!   [`Action::Send`]. The control centre talks with the app key, the only key that may decide.
//!   An approval with details (sending mail, task M3.5: every recipient, the attachments, the
//!   start of the text; a tool of an MCP server, issue #302: every argument whole as the agent gave
//!   it, which the summary cuts at 500 characters) has "Show details" under it, which lists them one
//!   per line, wrapped.
//! - **Recently decided**: done, failed, denied and expired ones of the last day, quieter, one line
//!   each, with their state as dot and word.
//!
//! Summary, details, client name and error messages hold text from outside (a client's, a mail's).
//! They are shown as plain text (an Iced `text` interprets nothing: no markup, no links), never as
//! anything else; the core made them harmless and short already.

use std::collections::BTreeSet;

use chrono::{DateTime, Local, Utc};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::theme::{self, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{layout, style, typo};
use torro_ipc::{ApprovalInfo, ApprovalState, Request, Response};

use super::{Action, Data, one_line, row_frame};

/// Page state: the approvals whose decision is on its way, so a second click cannot send it again
/// (the core would answer `conflict`), and those whose details are shown.
#[derive(Debug, Default)]
pub struct State {
    deciding: BTreeSet<String>,
    /// The ids of the approvals whose details are open; all are closed at first.
    expanded: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// "Allow" (`allow: true`) or "Deny" was pressed for the approval with this id.
    Decide { id: String, allow: bool },
    /// "Show details" or "Hide details" of the approval with this id.
    ToggleDetails(String),
}

pub fn update(state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::Decide { id, allow } => {
            if !state.deciding.insert(id.clone()) {
                return Action::None;
            }
            Action::Send(Request::DecideApproval { id, allow })
        }
        Message::ToggleDetails(id) => {
            if !state.expanded.remove(&id) {
                state.expanded.insert(id);
            }
            Action::None
        }
    }
}

/// An answer to a request came back (the shell calls this for every one, then reloads): the
/// decisions are no longer on their way. A failed one shows its buttons again; a successful one
/// leaves the open list with the reload.
pub fn answered(state: &mut State) {
    state.deciding.clear();
}

/// The open approvals in `approvals`, in the order the core sent them. The sidebar counts them,
/// and so will the inbox once it merges its count with this one (task E3.3b).
pub fn pending(approvals: &[ApprovalInfo]) -> impl Iterator<Item = &ApprovalInfo> {
    approvals.iter().filter(|approval| approval.state.is_pending())
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    if data.approvals.is_empty() {
        return widgets::empty_state(
            tr("No requests waiting."),
            tr("When an agent wants to change or delete something, you decide here."),
        );
    }
    let now = Utc::now();
    let mut open: Vec<&ApprovalInfo> = pending(&data.approvals).collect();
    // The one that expires first needs the person first.
    open.sort_by_key(|approval| approval.expires);
    let waiting: Element<'a, Message> = if open.is_empty() {
        column![
            widgets::section_title(tr("Waiting for you")),
            typo::text(typo::meta(), tr("Nothing waits for you right now.")).style(theme::text_3),
        ]
        .spacing(space::s())
        .into()
    } else {
        widgets::group(
            tr("Waiting for you"),
            open.into_iter().map(|approval| open_row(state, approval, now)).collect(),
            Some(
                tr("Allowed commands run with the agent's rights at that moment. Unanswered requests expire after ten minutes.")
                    .to_string(),
            ),
        )
    };
    let decided: Vec<Element<'a, Message>> =
        data.approvals.iter().filter(|approval| !approval.state.is_pending()).map(decided_row).collect();
    let mut page = column![waiting].spacing(layout::section_gap());
    if !decided.is_empty() {
        page = page.push(widgets::group(
            tr("Recently decided"),
            decided,
            Some(tr("Decided and expired requests stay here for a day.").to_string()),
        ));
    }
    page.into()
}

/// An open approval: the summary (wrapped, so nothing of what would run is hidden), the client and
/// the time left below it, "Deny" and "Allow" at the right. While its decision is on its way both
/// buttons are disabled. With details, "Show details" below opens them ([`details_block`]).
fn open_row<'a>(state: &State, approval: &'a ApprovalInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let detail = format!("{} · {}", approval.client_name, time_left(approval.expires, now));
    let lines = column![
        typo::text(typo::body(), approval.summary.as_str()),
        one_line(typo::text(typo::meta().tabular(), detail).style(theme::text_2)),
    ];
    let on =
        |allow| (!state.deciding.contains(&approval.id)).then(|| Message::Decide { id: approval.id.clone(), allow });
    let buttons = row![
        widgets::text_button(Some(Icon::X), tr("Deny"), style::button::secondary, on(false)),
        widgets::text_button(Some(Icon::Check), tr("Allow"), style::button::secondary, on(true)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let content = row![lines.width(Fill), buttons].spacing(space::l()).align_y(Alignment::Center);
    let content = column![content].push(details_block(state, approval)).spacing(space::xs());
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

/// "Show details" with a chevron, and when open the details one per line, wrapped (an address or
/// the start of a text can be long); `None` for an approval without details.
fn details_block<'a>(state: &State, approval: &'a ApprovalInfo) -> Option<Element<'a, Message>> {
    if approval.details.is_empty() {
        return None;
    }
    let open = state.expanded.contains(&approval.id);
    let (icon, label) = match open {
        true => (Icon::ChevronDown, tr("Hide details")),
        false => (Icon::ChevronRight, tr("Show details")),
    };
    let toggle = widgets::text_button(
        Some(icon),
        label,
        style::button::ghost,
        Some(Message::ToggleDetails(approval.id.clone())),
    );
    let mut block = column![super::accounts::on_axis(toggle)].spacing(space::xs());
    if open {
        let lines =
            approval.details.iter().map(|line| typo::text(typo::meta(), line.as_str()).style(theme::text_2).into());
        block = block.push(column(lines).spacing(space::xs()));
    }
    Some(block.into())
}

/// A decided or expired approval, quieter than the open ones: the summary in `text_2` on one line,
/// the client and the time of the decision below, the state as dot and word at the right.
fn decided_row(approval: &ApprovalInfo) -> Element<'_, Message> {
    let (status, word) = outcome(&approval.state);
    let when = approval.decided.unwrap_or(approval.created).with_timezone(&Local).format("%H:%M").to_string();
    let mut detail = format!("{} · {when}", approval.client_name);
    if let Some(message) = failure(&approval.state) {
        detail = format!("{detail} · {message}");
    }
    let lines = column![
        one_line(typo::text(typo::body(), approval.summary.as_str()).style(theme::text_2)),
        one_line(typo::text(typo::meta().tabular(), detail).style(theme::text_3)),
    ];
    let state =
        row![widgets::status_dot(status), typo::text(typo::meta(), word).style(theme::text_2).wrapping(Wrapping::None)]
            .spacing(space::s())
            .align_y(Alignment::Center);
    row_frame(row![lines.width(Fill), state].spacing(space::l()).align_y(Alignment::Center), true)
}

/// The state of a decided approval as status colour and word. The colour is only in the dot.
fn outcome(state: &ApprovalState) -> (Status, &'static str) {
    match state {
        ApprovalState::Pending => (Status::Attention, tr("Waiting")),
        ApprovalState::Running => (Status::Attention, tr("Running")),
        ApprovalState::Done { response } if matches!(response.as_ref(), Response::Error { .. }) => {
            (Status::Error, tr("Failed"))
        }
        ApprovalState::Done { .. } => (Status::Ok, tr("Done")),
        ApprovalState::Denied => (Status::Inactive, tr("Denied")),
        ApprovalState::Expired => (Status::Inactive, tr("Expired")),
    }
}

/// Why an allowed command failed, as the core answered it, on one line: control characters
/// (line breaks, tabs) become spaces, so the row keeps its height.
fn failure(state: &ApprovalState) -> Option<String> {
    let ApprovalState::Done { response } = state else { return None };
    let Response::Error { message, .. } = response.as_ref() else { return None };
    Some(message.chars().map(|c| if c.is_control() { ' ' } else { c }).collect())
}

/// How long an open approval still waits: "8 min left", rounded up so a fresh one says "10 min
/// left"; under a minute, and once the core is about to expire it, words instead of "0 min".
fn time_left(expires: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (expires - now).num_seconds();
    if seconds <= 0 {
        return tr("Expires now").to_string();
    }
    if seconds < 60 {
        return tr("Less than a minute left").to_string();
    }
    let minutes = (seconds + 59) / 60;
    trf("{n} min left", &[("n", &minutes)])
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn approval(id: &str, state: ApprovalState, expires_in: Duration) -> ApprovalInfo {
        let now = Utc::now();
        ApprovalInfo {
            id: id.into(),
            client: "claude".into(),
            client_name: "Claude Code".into(),
            command: "calendar.delete_event".into(),
            summary: "Delete event \"Team meeting\" on Tue, 14 Oct 09:00 from \"Work\"".into(),
            details: vec!["To: bernd@example.com".into(), "Text: \"Dear Bernd\"".into()],
            created: now - Duration::minutes(10) + expires_in,
            expires: now + expires_in,
            decided: (!state.is_pending()).then_some(now),
            state,
        }
    }

    fn data(approvals: Vec<ApprovalInfo>) -> Data {
        Data { approvals, ..Data::default() }
    }

    #[test]
    fn allow_and_deny_send_the_decision_once() {
        let mut state = State::default();
        let allow = update(&mut state, Message::Decide { id: "9c1f".into(), allow: true }, &Data::default());
        assert!(matches!(allow, Action::Send(Request::DecideApproval { ref id, allow: true }) if id == "9c1f"));
        let again = update(&mut state, Message::Decide { id: "9c1f".into(), allow: false }, &Data::default());
        assert!(matches!(again, Action::None), "a second click while the first is on its way sends nothing");

        let deny = update(&mut state, Message::Decide { id: "77aa".into(), allow: false }, &Data::default());
        assert!(matches!(deny, Action::Send(Request::DecideApproval { ref id, allow: false }) if id == "77aa"));

        answered(&mut state);
        let retry = update(&mut state, Message::Decide { id: "9c1f".into(), allow: true }, &Data::default());
        assert!(matches!(retry, Action::Send(_)), "after the answer the buttons work again");
    }

    #[test]
    fn only_open_approvals_are_counted() {
        let approvals = vec![
            approval("a", ApprovalState::Pending, Duration::minutes(4)),
            approval("b", ApprovalState::Denied, Duration::minutes(-5)),
            approval("c", ApprovalState::Expired, Duration::minutes(-1)),
            approval("d", ApprovalState::Pending, Duration::minutes(9)),
            approval("e", ApprovalState::Done { response: Box::new(Response::Ok) }, Duration::zero()),
        ];
        let ids: Vec<&str> = pending(&approvals).map(|approval| approval.id.as_str()).collect();
        assert_eq!(ids, ["a", "d"]);
    }

    #[test]
    fn the_time_left_is_rounded_up_and_ends_in_words() {
        let now = Utc::now();
        assert_eq!(time_left(now + Duration::minutes(10), now), trf("{n} min left", &[("n", &10)]));
        assert_eq!(time_left(now + Duration::seconds(61), now), trf("{n} min left", &[("n", &2)]));
        assert_eq!(time_left(now + Duration::seconds(60), now), trf("{n} min left", &[("n", &1)]));
        assert_eq!(time_left(now + Duration::seconds(30), now), tr("Less than a minute left"));
        assert_eq!(time_left(now - Duration::seconds(5), now), tr("Expires now"));
    }

    #[test]
    fn decided_approvals_name_their_outcome_and_a_failure_on_one_line() {
        let failed = ApprovalState::Done {
            response: Box::new(Response::Error {
                code: "forbidden".into(),
                message: "the client may no longer\nwrite to calendar".into(),
            }),
        };
        assert_eq!(outcome(&failed), (Status::Error, tr("Failed")));
        assert_eq!(failure(&failed).as_deref(), Some("the client may no longer write to calendar"));
        let done = ApprovalState::Done { response: Box::new(Response::Ok) };
        assert_eq!(outcome(&done), (Status::Ok, tr("Done")));
        assert_eq!(failure(&done), None);
        assert_eq!(outcome(&ApprovalState::Denied), (Status::Inactive, tr("Denied")));
        assert_eq!(outcome(&ApprovalState::Expired), (Status::Inactive, tr("Expired")));
        assert_eq!(outcome(&ApprovalState::Running).0, Status::Attention);
    }

    #[test]
    fn the_page_builds_empty_open_only_decided_only_and_mixed() {
        let mut state = State::default();
        let open = approval("a", ApprovalState::Pending, Duration::minutes(4));
        let denied = approval("b", ApprovalState::Denied, Duration::minutes(-5));
        for approvals in [vec![], vec![open.clone()], vec![denied.clone()], vec![open, denied]] {
            drop(view(&state, &data(approvals)));
        }
        let _ = update(&mut state, Message::Decide { id: "a".into(), allow: true }, &Data::default());
        drop(view(&state, &data(vec![approval("a", ApprovalState::Pending, Duration::minutes(4))])));
    }

    /// Task M3.5: the details of an approval are closed at first; a press opens them, another
    /// closes them, each approval on its own. Without details there is nothing to open.
    #[test]
    fn details_open_and_close_per_approval() {
        let mut state = State::default();
        let open = approval("a", ApprovalState::Pending, Duration::minutes(4));
        let plain = ApprovalInfo { id: "b".into(), details: Vec::new(), ..open.clone() };
        assert!(details_block(&state, &plain).is_none(), "nothing to show");
        assert!(details_block(&state, &open).is_some() && state.expanded.is_empty(), "closed at first");
        let toggle = |state: &mut State, id: &str| update(state, Message::ToggleDetails(id.into()), &Data::default());
        assert!(matches!(toggle(&mut state, "a"), Action::None), "nothing is sent");
        assert!(state.expanded.contains("a") && !state.expanded.contains("b"));
        drop(view(&state, &data(vec![open, plain])));
        toggle(&mut state, "a");
        assert!(state.expanded.is_empty());
    }
}
