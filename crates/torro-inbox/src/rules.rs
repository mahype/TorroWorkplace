//! The rules of the inbox as pure functions, shared by the core (E1.2), the demo data and the
//! apps, so all of them merge, place, wake and sort entries the same way.
//!
//! The security rules of `docs/tasks/eingang.md` that depend only on an entry live here too
//! ([`admitted_kind`], [`human_only`], [`may_open`]); the core enforces them (E1.2, E2.4).
//! Nothing here reads a clock: whoever calls passes `now`, so tests can fix it.

use std::cmp::Ordering;

use chrono::{DateTime, TimeDelta, Utc};

use crate::item::{HideScope, Item, Kind, Lane, Link, Rule, RuleEffect, Sender, SourceKind, State};

/// Days a done entry is kept before [`expired`] says it goes.
pub const KEEP_DONE_DAYS: i64 = 30;

/// Who creates an entry, as the core sees the client behind a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poster {
    /// The core itself: approvals, account problems.
    Core,
    /// A client of the kind `hook` (E2.3): the hooks of coding agents.
    Hook,
    /// Any other client: agents over MCP, scripts, `torroworkplace notify`.
    Client,
}

/// Lane and urgency of an entry of `kind`; `wanted` is the lane the poster asked for.
///
/// Fixed for every kind but [`Kind::Notice`] and [`Kind::Message`], so nobody can make their
/// entries urgent: waiting agents and approvals are for me and urgent, finished agents and account
/// problems for me; a notice goes where it was wanted ([`Lane::Info`] when nothing was asked), a
/// message too ([`Lane::ForMe`] when nothing was asked, the core asks [`mail_lane`]); neither is
/// ever urgent.
pub fn placement(kind: Kind, wanted: Option<Lane>) -> (Lane, bool) {
    match kind {
        Kind::AgentWaiting | Kind::Approval => (Lane::ForMe, true),
        Kind::AgentDone | Kind::AccountProblem => (Lane::ForMe, false),
        Kind::Notice => (wanted.unwrap_or(Lane::Info), false),
        Kind::Message => (wanted.unwrap_or(Lane::ForMe), false),
    }
}

/// The lane of a new mail (task E4.1, `docs/eingang.md`, "Sortieren"): a lane the person
/// remembered for its sender (`remembered`, from moving one of its mails with "remember") first,
/// since the person decided it; else [`Lane::Info`] for mail sent by machines or to lists
/// (`bulk`: `List-Id`, `List-Unsubscribe`, `Precedence: bulk`, `Auto-Submitted`), else
/// [`Lane::ForMe`].
pub fn mail_lane(bulk: bool, remembered: Option<Lane>) -> Lane {
    remembered.unwrap_or(if bulk { Lane::Info } else { Lane::ForMe })
}

/// How a lane is kept for a mail's sender ([`crate::Rule::sender`]): the sender's address in small
/// letters, without surrounding space; `None` without an address.
pub fn sender_key(from: Option<&Sender>) -> Option<String> {
    let address = from?.address.as_deref()?.trim().to_lowercase();
    (!address.is_empty()).then_some(address)
}

/// The sender a lane is kept for when `item` is moved with "remember" (task E4.1): for an entry of
/// a mail account its [`sender_key`]; `None` for other entries (their source is remembered) and
/// for mail without a sender's address (nothing can be remembered).
pub fn mail_sender(item: &Item) -> Option<String> {
    (item.source.kind == SourceKind::Mail).then(|| sender_key(item.from.as_ref())).flatten()
}

/// The domain of a sender's address (task E3.10): what follows the last `@`, without surrounding
/// space and a final dot (`shop.example.` is `shop.example`), in small letters; `None` without an
/// `@` or with nothing after it. Exact: the domain of `news@mail.shop.example` is
/// `mail.shop.example`, which a rule for `shop.example` does not cover.
pub fn domain_of(address: &str) -> Option<String> {
    let (_, domain) = address.trim().rsplit_once('@')?;
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    (!domain.is_empty()).then_some(domain)
}

/// What `inbox.hide_sender` with `scope` keeps a rule for when it hides `item` (task E3.10): its
/// sender's address ([`mail_sender`]) or that address's domain ([`domain_of`]). `None` for entries
/// that are no mail and for mail without a sender's address: there is nothing to hide by. An
/// "address" that starts with `@` names no sender: the core keeps a hidden domain as `@<domain>`,
/// so such a sender's rule would be taken for the domain's.
pub fn hide_target(item: &Item, scope: HideScope) -> Option<String> {
    let sender = mail_sender(item).filter(|sender| !sender.starts_with('@'))?;
    match scope {
        HideScope::Sender => Some(sender),
        HideScope::Domain => domain_of(&sender),
    }
}

/// Whether `rule` drops a new report from `sender` (its [`sender_key`], `None` without one) of
/// the rule's source; the caller passes only rules of that source. Only muting rules drop: one
/// without `sender` and `domain` everything, one with `sender` only that address, one with
/// `domain` only addresses of exactly that domain (task E3.10). A report without a sender's
/// address is dropped only by a rule for the whole source.
pub fn drops(rule: &Rule, sender: Option<&str>) -> bool {
    if rule.effect != RuleEffect::Muted {
        return false;
    }
    match (&rule.sender, &rule.domain) {
        (None, None) => true,
        (Some(hidden), _) => sender == Some(hidden.as_str()),
        (None, Some(hidden)) => sender.and_then(domain_of).as_deref() == Some(hidden.as_str()),
    }
}

/// Whether `rule` hides `item`: a muting rule for one sender or domain (task E3.10) of the entry's
/// source that would drop its mail ([`drops`]). The entries it hides are done when the rule is
/// made; a rule for a whole source hides none, muting leaves the entries already there.
pub fn hides(rule: &Rule, item: &Item) -> bool {
    (rule.sender.is_some() || rule.domain.is_some())
        && rule.source == item.source
        && drops(rule, mail_sender(item).as_deref())
}

/// The kind an entry posted as `kind` by `poster` gets; `None` when the post is refused
/// (`forbidden`). Security rule 3: approvals and account problems come only from the core,
/// agent states only from hooks (or the core) and turn into a [`Kind::Notice`] from anyone else;
/// messages (mail) only from the core, a [`Kind::Notice`] from anyone else.
pub fn admitted_kind(kind: Kind, poster: Poster) -> Option<Kind> {
    match (kind, poster) {
        (Kind::Approval | Kind::AccountProblem, Poster::Core) => Some(kind),
        (Kind::Approval | Kind::AccountProblem, _) => None,
        (Kind::AgentWaiting | Kind::AgentDone, Poster::Core | Poster::Hook) => Some(kind),
        (Kind::AgentWaiting | Kind::AgentDone, Poster::Client) => Some(Kind::Notice),
        (Kind::Message, Poster::Core) => Some(Kind::Message),
        (Kind::Message | Kind::Notice, _) => Some(Kind::Notice),
    }
}

/// Only the app key may change the entry (done, snooze, lane), every other client gets
/// `forbidden`. Security rule 1: approvals, account problems and anything from the suite belong
/// to the user, or an agent could push away an approval.
pub fn human_only(item: &Item) -> bool {
    matches!(item.kind, Kind::Approval | Kind::AccountProblem) || item.source.kind == SourceKind::Suite
}

/// The entry's link may be opened. Security rule 6: web links only with `http` or `https`,
/// apps only those `known_app` accepts (the apps of `apps.list_apps` and the control centre),
/// windows only on entries a hook reported (the agent kinds, which only hooks create), messages
/// only on entries of a mail source (which only the core files). `false` without a link.
pub fn may_open(item: &Item, known_app: impl Fn(&str) -> bool) -> bool {
    match &item.link {
        None => false,
        Some(Link::Url { url }) => is_web(url),
        Some(Link::App { app }) => known_app(app),
        Some(Link::Window) => matches!(item.kind, Kind::AgentWaiting | Kind::AgentDone),
        Some(Link::Mail { .. }) => item.source.kind == SourceKind::Mail,
    }
}

/// `url` starts with `http://` or `https://`, in any case.
fn is_web(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// `existing` after the report `incoming` of the same matter (same `source` and `thread_key`;
/// the caller finds `existing` by them).
///
/// Keeps id, source, thread key and `received`; takes everything the report says (kind, lane,
/// urgency, texts, agent, link, actions, `untrusted`) and its `updated`; counts one more report.
/// A done or seen entry is new again, so a matter that comes back is not missed; a snoozed one
/// stays snoozed, the user put it off on purpose.
pub fn merge(existing: Item, incoming: Item) -> Item {
    let state = match existing.state {
        State::Snoozed { until } => State::Snoozed { until },
        State::New | State::Seen | State::Done { .. } => State::New,
    };
    Item {
        id: existing.id,
        source: existing.source,
        thread_key: existing.thread_key,
        received: existing.received,
        count: existing.count.saturating_add(1),
        state,
        ..incoming
    }
}

/// Marks the entry done at `now`, or open again: a done entry becomes [`State::Seen`] (the
/// user has seen it), any other stays as it is. `updated` does not change, so undoing puts the
/// entry back where it was in the list.
pub fn set_done(item: &mut Item, done: bool, now: DateTime<Utc>) {
    if done {
        if !matches!(item.state, State::Done { .. }) {
            item.state = State::Done { at: now };
        }
    } else if matches!(item.state, State::Done { .. }) {
        item.state = State::Seen;
    }
}

/// Puts the entry off until `until`; `None` takes a snoozed entry back to [`State::Seen`] and
/// leaves any other as it is.
pub fn snooze(item: &mut Item, until: Option<DateTime<Utc>>) {
    match until {
        Some(until) => item.state = State::Snoozed { until },
        None if matches!(item.state, State::Snoozed { .. }) => item.state = State::Seen,
        None => {}
    }
}

/// Moves the entry to `lane`. It is urgent only in [`Lane::ForMe`] and only if its kind is
/// ([`placement`]): an entry the user moved to "Info" does not interrupt any more.
pub fn set_lane(item: &mut Item, lane: Lane) {
    item.lane = lane;
    item.urgent = lane == Lane::ForMe && placement(item.kind, None).1;
}

/// Wakes the entry if it is snoozed until `now` or earlier: [`State::New`] with `updated` at the
/// time it was due, so it comes back at the top. Returns whether it woke.
pub fn wake(item: &mut Item, now: DateTime<Utc>) -> bool {
    match item.state {
        State::Snoozed { until } if until <= now => {
            item.state = State::New;
            item.updated = until;
            true
        }
        _ => false,
    }
}

/// The entry was done [`KEEP_DONE_DAYS`] or more before `now` and is removed.
pub fn expired(item: &Item, now: DateTime<Utc>) -> bool {
    match item.state {
        State::Done { at } => now - at >= TimeDelta::days(KEEP_DONE_DAYS),
        _ => false,
    }
}

/// The order of the list: urgent entries first, then the most recently `updated`; ties by id so
/// every app shows the same order.
pub fn order(a: &Item, b: &Item) -> Ordering {
    b.urgent.cmp(&a.urgent).then_with(|| b.updated.cmp(&a.updated)).then_with(|| a.id.cmp(&b.id))
}

/// Sorts `items` by [`order`].
pub fn sort(items: &mut [Item]) {
    items.sort_by(order);
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::item::{Action, AgentInfo, AgentKind, SourceRef};

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).single().expect("valid time")
    }

    fn item(id: &str, kind: Kind) -> Item {
        let (lane, urgent) = placement(kind, None);
        Item {
            id: id.into(),
            source: SourceRef::script("backup"),
            thread_key: format!("thread-{id}"),
            kind,
            lane,
            urgent,
            title: "Backup finished".into(),
            body: None,
            from: None,
            agent: None,
            received: at(8),
            updated: at(8),
            count: 1,
            state: State::New,
            link: None,
            actions: Vec::new(),
            untrusted: true,
        }
    }

    #[test]
    fn fixed_lanes() {
        for wanted in [None, Some(Lane::Info), Some(Lane::ForMe)] {
            assert_eq!(placement(Kind::AgentWaiting, wanted), (Lane::ForMe, true));
            assert_eq!(placement(Kind::Approval, wanted), (Lane::ForMe, true));
            assert_eq!(placement(Kind::AgentDone, wanted), (Lane::ForMe, false));
            assert_eq!(placement(Kind::AccountProblem, wanted), (Lane::ForMe, false));
        }
        assert_eq!(placement(Kind::Notice, None), (Lane::Info, false));
        assert_eq!(placement(Kind::Notice, Some(Lane::ForMe)), (Lane::ForMe, false), "never urgent");
        assert_eq!(placement(Kind::Message, None), (Lane::ForMe, false));
        assert_eq!(placement(Kind::Message, Some(Lane::Info)), (Lane::Info, false), "never urgent");
    }

    #[test]
    fn mail_from_lists_and_machines_is_info_unless_the_person_said_otherwise() {
        assert_eq!(mail_lane(false, None), Lane::ForMe);
        assert_eq!(mail_lane(true, None), Lane::Info);
        assert_eq!(mail_lane(true, Some(Lane::ForMe)), Lane::ForMe, "a remembered sender beats the headers");
        assert_eq!(mail_lane(false, Some(Lane::Info)), Lane::Info);
    }

    #[test]
    fn a_mails_lane_is_kept_for_its_sender_in_small_letters() {
        let from = |address: Option<&str>| Sender { name: "Shop".into(), address: address.map(String::from) };
        assert_eq!(sender_key(Some(&from(Some(" News@Shop.Example ")))).as_deref(), Some("news@shop.example"));
        assert_eq!(sender_key(Some(&from(Some("  ")))), None);
        assert_eq!(sender_key(Some(&from(None))), None);
        assert_eq!(sender_key(None), None);
        let mail = Item {
            source: SourceRef::mail("anna@example.org"),
            from: Some(from(Some("News@Shop.example"))),
            ..item("1", Kind::Message)
        };
        assert_eq!(mail_sender(&mail).as_deref(), Some("news@shop.example"));
        assert_eq!(mail_sender(&Item { from: mail.from.clone(), ..item("2", Kind::Notice) }), None, "not mail");
    }

    fn mail_from(address: Option<&str>) -> Item {
        Item {
            source: SourceRef::mail("anna@example.org"),
            from: Some(Sender { name: "Shop".into(), address: address.map(String::from) }),
            ..item("m", Kind::Message)
        }
    }

    fn hiding(sender: Option<&str>, domain: Option<&str>) -> Rule {
        Rule {
            id: "r".into(),
            source: SourceRef::mail("anna@example.org"),
            sender: sender.map(String::from),
            domain: domain.map(String::from),
            effect: RuleEffect::Muted,
            created: at(8),
        }
    }

    #[test]
    fn a_domain_is_what_follows_the_last_at_in_small_letters() {
        assert_eq!(domain_of("news@Shop.Example").as_deref(), Some("shop.example"));
        assert_eq!(domain_of(" \"a@b\"@shop.example ").as_deref(), Some("shop.example"));
        assert_eq!(domain_of("news@mail.shop.example").as_deref(), Some("mail.shop.example"));
        assert_eq!(domain_of("no-at-sign"), None);
        assert_eq!(domain_of("news@"), None);
        assert_eq!(domain_of("news@shop.example.").as_deref(), Some("shop.example"), "a final dot is the same");
    }

    #[test]
    fn hiding_needs_a_mail_with_a_sender() {
        let mail = mail_from(Some("News@Shop.Example"));
        assert_eq!(hide_target(&mail, HideScope::Sender).as_deref(), Some("news@shop.example"));
        assert_eq!(hide_target(&mail, HideScope::Domain).as_deref(), Some("shop.example"));
        assert_eq!(hide_target(&mail_from(None), HideScope::Sender), None, "no address");
        assert_eq!(hide_target(&mail_from(Some("@shop.example")), HideScope::Sender), None, "no sender");
        let notice = Item { from: mail.from.clone(), ..item("n", Kind::Notice) };
        assert_eq!(hide_target(&notice, HideScope::Domain), None, "not mail");
    }

    #[test]
    fn a_hidden_sender_or_domain_drops_only_its_mail() {
        let sender = hiding(Some("news@shop.example"), None);
        assert!(drops(&sender, Some("news@shop.example")));
        assert!(!drops(&sender, Some("orders@shop.example")));
        assert!(!drops(&sender, None));
        let domain = hiding(None, Some("shop.example"));
        assert!(drops(&domain, Some("news@shop.example")));
        assert!(drops(&domain, Some("orders@shop.example")));
        assert!(!drops(&domain, Some("news@mail.shop.example")), "no subdomains");
        assert!(!drops(&domain, Some("news@othershop.example")));
        assert!(!drops(&domain, None));
        let whole = hiding(None, None);
        assert!(drops(&whole, Some("anyone@example.org")) && drops(&whole, None), "a muted source drops all");
        let lane = Rule { effect: RuleEffect::Lane { lane: Lane::Info }, ..hiding(Some("news@shop.example"), None) };
        assert!(!drops(&lane, Some("news@shop.example")), "a lane drops nothing");
    }

    #[test]
    fn a_hiding_rule_hides_matching_entries_of_its_source_whatever_their_case() {
        let domain = hiding(None, Some("shop.example"));
        assert!(hides(&domain, &mail_from(Some("NEWS@SHOP.EXAMPLE"))));
        assert!(!hides(&domain, &mail_from(Some("news@mail.shop.example"))));
        assert!(!hides(&domain, &mail_from(None)));
        let other_account =
            Item { source: SourceRef::mail("anna@work.example.org"), ..mail_from(Some("a@shop.example")) };
        assert!(!hides(&domain, &other_account), "only the rule's account");
        assert!(!hides(&hiding(None, None), &mail_from(Some("a@shop.example"))), "muting hides nothing already there");
    }

    #[test]
    fn only_the_core_creates_approvals_and_only_hooks_agent_states() {
        for kind in [Kind::Approval, Kind::AccountProblem] {
            assert_eq!(admitted_kind(kind, Poster::Core), Some(kind));
            assert_eq!(admitted_kind(kind, Poster::Hook), None);
            assert_eq!(admitted_kind(kind, Poster::Client), None);
        }
        for kind in [Kind::AgentWaiting, Kind::AgentDone] {
            assert_eq!(admitted_kind(kind, Poster::Hook), Some(kind));
            assert_eq!(admitted_kind(kind, Poster::Core), Some(kind));
            assert_eq!(admitted_kind(kind, Poster::Client), Some(Kind::Notice));
        }
        for poster in [Poster::Core, Poster::Hook, Poster::Client] {
            assert_eq!(admitted_kind(Kind::Notice, poster), Some(Kind::Notice));
        }
        assert_eq!(admitted_kind(Kind::Message, Poster::Core), Some(Kind::Message));
        for poster in [Poster::Hook, Poster::Client] {
            assert_eq!(admitted_kind(Kind::Message, poster), Some(Kind::Notice), "only the core files mail");
        }
    }

    #[test]
    fn approvals_account_problems_and_the_suite_belong_to_the_user() {
        assert!(human_only(&item("1", Kind::Approval)));
        assert!(human_only(&item("2", Kind::AccountProblem)));
        assert!(human_only(&Item { source: SourceRef::suite(), ..item("3", Kind::Notice) }));
        assert!(!human_only(&item("4", Kind::Notice)));
        assert!(!human_only(&Item { source: SourceRef::agent("codex"), ..item("5", Kind::AgentWaiting) }));
    }

    #[test]
    fn only_safe_links_open() {
        let with = |kind: Kind, link: Link| Item { link: Some(link), ..item("1", kind) };
        let known = |app: &str| app == "torrocalendar";
        let url = |u: &str| Link::Url { url: u.into() };
        assert!(may_open(&with(Kind::Notice, url("https://example.org")), known));
        assert!(may_open(&with(Kind::Notice, url("HTTP://example.org")), known));
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "httpx://a", "ftp://a", ""] {
            assert!(!may_open(&with(Kind::Notice, url(bad)), known), "{bad}");
        }
        assert!(may_open(&with(Kind::Notice, Link::App { app: "torrocalendar".into() }), known));
        assert!(!may_open(&with(Kind::Notice, Link::App { app: "sh".into() }), known));
        assert!(may_open(&with(Kind::AgentWaiting, Link::Window), known));
        assert!(may_open(&with(Kind::AgentDone, Link::Window), known));
        assert!(!may_open(&with(Kind::Notice, Link::Window), known), "window only on hook entries");
        assert!(!may_open(&item("1", Kind::Notice), known), "no link");
        let message = Link::Mail { message: "m:1".into() };
        let mail = Item { source: SourceRef::mail("anna@example.org"), ..with(Kind::Message, message.clone()) };
        assert!(may_open(&mail, known));
        assert!(!may_open(&with(Kind::Notice, message), known), "a message only on mail entries");
    }

    #[test]
    fn merging_counts_takes_the_report_and_keeps_the_identity() {
        let existing = Item { state: State::Seen, ..item("1", Kind::AgentWaiting) };
        let incoming = Item {
            id: "ignored".into(),
            received: at(10),
            updated: at(10),
            title: "Codex finished".into(),
            body: Some("All tests pass".into()),
            agent: Some(AgentInfo {
                agent: AgentKind::Codex,
                session: "s1".into(),
                project: None,
                branch: None,
                pids: Vec::new(),
                title: Some("Parser fixtures and docs".into()),
            }),
            ..item("2", Kind::AgentDone)
        };
        let merged = merge(existing.clone(), incoming.clone());
        assert_eq!(merged.id, existing.id);
        assert_eq!(merged.thread_key, existing.thread_key);
        assert_eq!(merged.received, at(8));
        assert_eq!(merged.updated, at(10));
        assert_eq!(merged.count, 2);
        assert_eq!(merged.state, State::New);
        assert_eq!((merged.kind, merged.lane, merged.urgent), (Kind::AgentDone, Lane::ForMe, false));
        assert_eq!(merged.title, incoming.title);
        assert_eq!(merged.body, incoming.body);
        // A session renamed meanwhile shows its new name (task E2.6).
        assert_eq!(merged.agent, incoming.agent);
        assert_eq!(merged.agent, incoming.agent);
    }

    #[test]
    fn merging_brings_back_done_but_not_snoozed() {
        let done = Item { state: State::Done { at: at(9) }, ..item("1", Kind::Notice) };
        assert_eq!(merge(done, item("2", Kind::Notice)).state, State::New);
        let snoozed = State::Snoozed { until: at(20) };
        let merged = merge(Item { state: snoozed, ..item("1", Kind::Notice) }, item("2", Kind::Notice));
        assert_eq!(merged.state, snoozed);
        let many = Item { count: u32::MAX, ..item("1", Kind::Notice) };
        assert_eq!(merge(many, item("2", Kind::Notice)).count, u32::MAX);
    }

    #[test]
    fn done_and_open_again() {
        let mut entry = item("1", Kind::Notice);
        set_done(&mut entry, true, at(9));
        assert_eq!(entry.state, State::Done { at: at(9) });
        set_done(&mut entry, true, at(10));
        assert_eq!(entry.state, State::Done { at: at(9) }, "done twice keeps the first time");
        set_done(&mut entry, false, at(11));
        assert_eq!(entry.state, State::Seen);
        assert_eq!(entry.updated, at(8), "keeps its place");
        let mut fresh = item("2", Kind::Notice);
        set_done(&mut fresh, false, at(11));
        assert_eq!(fresh.state, State::New);
    }

    #[test]
    fn snoozing_and_taking_it_back() {
        let mut entry = item("1", Kind::Notice);
        snooze(&mut entry, None);
        assert_eq!(entry.state, State::New, "not snoozed, nothing to take back");
        snooze(&mut entry, Some(at(20)));
        assert_eq!(entry.state, State::Snoozed { until: at(20) });
        snooze(&mut entry, None);
        assert_eq!(entry.state, State::Seen);
    }

    #[test]
    fn moving_to_info_ends_urgency_and_back_restores_it() {
        let mut waiting = item("1", Kind::AgentWaiting);
        set_lane(&mut waiting, Lane::Info);
        assert_eq!((waiting.lane, waiting.urgent), (Lane::Info, false));
        set_lane(&mut waiting, Lane::ForMe);
        assert_eq!((waiting.lane, waiting.urgent), (Lane::ForMe, true));
        let mut notice = item("2", Kind::Notice);
        set_lane(&mut notice, Lane::ForMe);
        assert_eq!((notice.lane, notice.urgent), (Lane::ForMe, false));
    }

    #[test]
    fn snoozed_entries_wake_at_their_time() {
        let mut entry = Item { state: State::Snoozed { until: at(12) }, ..item("1", Kind::Notice) };
        assert!(!wake(&mut entry, at(11)));
        assert_eq!(entry.state, State::Snoozed { until: at(12) });
        assert!(wake(&mut entry, at(12)));
        assert_eq!(entry.state, State::New);
        assert_eq!(entry.updated, at(12), "back at the top");
        assert!(!wake(&mut entry, at(13)), "wakes once");
        let mut open = item("2", Kind::Notice);
        assert!(!wake(&mut open, at(13)));
    }

    #[test]
    fn done_entries_expire_after_30_days() {
        let done = Item { state: State::Done { at: at(8) }, ..item("1", Kind::Notice) };
        let days = |n: i64| at(8) + TimeDelta::days(n);
        assert!(!expired(&done, days(29)));
        assert!(!expired(&done, days(KEEP_DONE_DAYS) - TimeDelta::seconds(1)));
        assert!(expired(&done, days(KEEP_DONE_DAYS)));
        for open in [State::New, State::Seen, State::Snoozed { until: at(9) }] {
            assert!(!expired(&Item { state: open, ..item("2", Kind::Notice) }, days(365)));
        }
    }

    #[test]
    fn urgent_first_then_newest() {
        let mut items = vec![
            Item { updated: at(9), ..item("old", Kind::Notice) },
            Item { updated: at(7), ..item("urgent-old", Kind::AgentWaiting) },
            Item { updated: at(11), ..item("new", Kind::Notice) },
            Item { updated: at(10), ..item("urgent-new", Kind::Approval) },
            Item { updated: at(11), ..item("new-b", Kind::Notice) },
        ];
        sort(&mut items);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["urgent-new", "urgent-old", "new", "new-b", "old"]);
    }

    #[test]
    fn items_survive_a_serde_round_trip() {
        let full = Item {
            body: Some("Claude needs your permission to use Bash".into()),
            from: Some(crate::item::Sender { name: "Anna".into(), address: Some("anna@example.org".into()) }),
            agent: Some(AgentInfo {
                agent: AgentKind::ClaudeCode,
                session: "abc".into(),
                project: Some("TorroWorkplace".into()),
                branch: Some("feat/inbox".into()),
                pids: vec![42, 7],
                title: Some("Login bug".into()),
            }),
            state: State::Snoozed { until: at(20) },
            link: Some(Link::Window),
            actions: vec![Action::Allow { approval: "a1".into() }, Action::Deny { approval: "a1".into() }],
            ..item("1", Kind::AgentWaiting)
        };
        for entry in
            [full, item("2", Kind::Notice), Item { state: State::Done { at: at(9) }, ..item("3", Kind::Notice) }]
        {
            let json = serde_json::to_string(&entry).expect("serialize");
            assert_eq!(serde_json::from_str::<Item>(&json).expect("deserialize"), entry, "{json}");
        }
    }

    #[test]
    fn optional_fields_may_be_left_out() {
        let json = serde_json::json!({
            "id": "1",
            "source": { "kind": "suite" },
            "thread_key": "approval:1",
            "kind": "approval",
            "lane": "for_me",
            "urgent": true,
            "title": "Approve?",
            "received": "2026-10-11T08:00:00Z",
            "updated": "2026-10-11T08:00:00Z",
            "count": 1,
            "state": "new",
            "untrusted": true
        });
        let entry: Item = serde_json::from_value(json).expect("deserialize");
        assert_eq!(entry.source, SourceRef::suite());
        assert_eq!((entry.body, entry.link, entry.actions), (None, None, Vec::new()));
    }
}
