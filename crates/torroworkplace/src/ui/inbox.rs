//! The inbox page of the control centre (task E3.7 in `docs/tasks/eingang.md`, concept in
//! `docs/eingang.md`, "Ruhe: was das genau heißt"): how loud the inbox is, and the lasting rules
//! the user made in the Inbox app.
//!
//! Three settings groups, each with a footer sentence (`docs/settings.md`):
//!
//! - **Notifications**: the switch "Quiet during busy events" ([`Settings::quiet_when_busy`]). A
//!   flip sends `settings.set_setting { key: "quiet_when_busy", value }` at once, like the
//!   switches of the General page; there is no Save button.
//! - **Rules**: one row per rule of `inbox.list_rules` ([`Data::rules`]), remembered lanes first,
//!   then muted sources, then hidden senders and domains (task E3.10; such a rule holds for every
//!   account, task E3.11), then the traits muted with the bell's menu (task E8.6b), each with
//!   "Remove", which sends `inbox.remove_rule { id }`. Task E8.12 (finding 15): the title is what
//!   the rule is about ([`subject`]: "Mail from news@shop.example", "The project torro-workplace"),
//!   the quiet line below what it does, where it holds and what it caught since when
//!   ([`rule_line`]: "Hidden · All mail accounts · Hid 12 since Tuesday", "No popups until tomorrow
//!   09:00 · All agents · Kept 3 quiet since today", [`Data::rule_hits`] of task E8.8a), each on
//!   one line cut with "…". Without rules the group says where they come from.
//! - **App**: "Open" starts or raises Torro Inbox through `apps.open_app`, where the entries
//!   themselves are handled.
//!
//! "Remove" asks nothing. `docs/settings.md` confirms destructive actions, and removing a rule
//! destroys nothing: the entries in the inbox stay as they are, only new notices are sorted the
//! normal way again, and the same rule comes back with one action in the Inbox ("Always", "Mute").
//! A dialog for that would only be in the way. A second click while the request is on its way
//! sends nothing, so the core does not answer `not-found` for a rule that is already gone.
//!
//! Source names come from [`SourceRef::label`]: the product name of a known agent, else the name a
//! client chose. They are shown as plain text (an Iced `text` interprets nothing).
//!
//! [`Settings::quiet_when_busy`]: torro_core::settings::Settings::quiet_when_busy
//! [`SourceRef::label`]: torro_inbox::SourceRef::label

use std::collections::BTreeSet;

use chrono::{DateTime, Days, Utc};

use iced::widget::{Column, column, container, row, toggler};
use iced::{Alignment, Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, space};
use torro_gui::{datetime, layout, style, typo, widgets};
use torro_inbox::item::{FacetKey, MuteEffect};
use torro_inbox::{Lane, Rule, RuleEffect, rules as inbox_rules};
use torro_ipc::{Request, RuleHits, SettingKey};

use super::{Action, Data, setting_row};

/// Page state: the rules whose removal is on its way, so their "Remove" is off until the core
/// answered.
#[derive(Debug, Default)]
pub struct State {
    removing: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The switch "Quiet during busy events" was flipped to this value.
    QuietWhenBusy(bool),
    /// "Remove" was pressed on the rule with this id.
    Remove(String),
    /// "Open" in the group "App": start or raise Torro Inbox.
    OpenInbox,
}

pub fn update(state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::QuietWhenBusy(quiet) => {
            Action::Send(Request::SetSetting { key: SettingKey::QuietWhenBusy, value: serde_json::Value::Bool(quiet) })
        }
        Message::Remove(id) => {
            if !state.removing.insert(id.clone()) {
                return Action::None;
            }
            Action::Send(Request::RemoveRule { id })
        }
        Message::OpenInbox => Action::Send(Request::OpenApp { app: AppId::INBOX.binary.to_string(), target: None }),
    }
}

/// An answer to a request came back (the shell calls this for every one, then reloads): no
/// removal is on its way any more. A failed one shows its button again; a removed rule leaves the
/// list with the reload.
pub fn answered(state: &mut State) {
    state.removing.clear();
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    // Ends and "since" are said from now, as the core counts.
    let rules = rules(state, &data.rules, &data.rule_hits, Utc::now());
    column![notifications(data), rules, app(data)].spacing(layout::section_gap()).into()
}

/// The group "Notifications" with the one switch and what it means.
fn notifications<'a>(data: &Data) -> Element<'a, Message> {
    let switch = toggler(data.settings.quiet_when_busy)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(Message::QuietWhenBusy);
    let lines = column![
        typo::text(typo::body(), tr("Quiet during busy events")),
        typo::text(typo::meta(), tr("Only urgent entries pop up while a busy event runs; a summary follows."))
            .style(theme::text_2),
    ];
    widgets::group(
        tr("Notifications"),
        vec![wrapping_row(lines, switch)],
        Some(
            tr("Approvals and waiting agents always come through. Events marked as free and all-day events keep the Notification Hub as it is.")
                .to_string(),
        ),
    )
}

/// The group "Rules": the rules in the order [`ordered`] gives them, each with what it caught
/// (`hits`, by the rule's id) seen from `now`, or a sentence where they come from.
fn rules<'a>(state: &State, rules: &'a [Rule], hits: &[RuleHits], now: DateTime<Utc>) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = if rules.is_empty() {
        let hint = typo::text(
            typo::body(),
            tr("Rules appear here when you mute something with the bell, move notices to a lane for good or hide a sender in the Notification Hub."),
        )
        .style(theme::text_2);
        vec![container(hint).width(Fill).padding([space::m(), space::l()]).into()]
    } else {
        let caught = |rule: &Rule| hits.iter().find(|hit| hit.rule == rule.id);
        ordered(rules).into_iter().map(|rule| rule_row(state, rule, caught(rule), now)).collect()
    };
    widgets::group(
        tr("Rules"),
        rows,
        Some(
            tr("Rules apply to new notices. Removing one leaves the entries in the Notification Hub as they are.")
                .to_string(),
        ),
    )
}

/// The rules for the page: remembered lanes first, then muted sources, then hidden senders and
/// domains, then muted traits (task E8.6b), each kind in the core's order (oldest first), so a new
/// rule always lands at the end of its kind.
fn ordered(rules: &[Rule]) -> Vec<&Rule> {
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    ordered.sort_by_key(|rule| match rule.effect {
        _ if rule.facet.is_some() => 3,
        RuleEffect::Lane { .. } => 0,
        RuleEffect::Muted if is_hidden(rule) => 2,
        RuleEffect::Muted => 1,
        // Only rules with a trait keep no popups; one without would be about its whole source.
        RuleEffect::NoPopup => 3,
    });
    ordered
}

/// A rule that hides one sender or domain of a mail account (task E3.10), not a whole source.
fn is_hidden(rule: &Rule) -> bool {
    rule.effect == RuleEffect::Muted && (rule.sender.is_some() || rule.domain.is_some())
}

/// What a rule is about – the title of its row (task E8.12, finding 15): "Everything from Claude
/// Code" for a whole source, "Mail from news@shop.example" for a sender, "Everything from
/// shop.example" for a domain, and for a trait muted with the bell's menu its words ("The project
/// torro-workplace", "Newsletters and automatic mail", the Inbox's own). A conversation is named by
/// its source ("A conversation in Claude Code"): its thread key means nothing to a person.
/// Addresses and domains are shown as they are, like source names.
fn subject(rule: &Rule) -> String {
    if let Some(facet) = &rule.facet {
        return match facet.key {
            FacetKey::Conversation => trf("A conversation in {source}", &[("source", &rule.source.label())]),
            _ => inbox_rules::facet_label(facet),
        };
    }
    match (&rule.sender, &rule.domain) {
        (Some(sender), _) => trf("Mail from {sender}", &[("sender", sender)]),
        (None, Some(domain)) => trf("Everything from {domain}", &[("domain", domain)]),
        (None, None) => trf("Everything from {source}", &[("source", &rule.source.label())]),
    }
}

/// What a rule does, with its end – the first part of its row's quiet line (task E8.12, finding
/// 15): "Goes to “Info”" for a remembered lane, "Muted" for a whole source, "Hidden" for a sender
/// or domain, and for a trait "Hidden", "Only Info" or "No popups", with "until tomorrow 09:00"
/// while it ends ([`until_text`]). The Inbox's toast says the same after muting.
fn effect_words(rule: &Rule, now: DateTime<Utc>) -> String {
    if rule.facet.is_none() {
        return match rule.effect {
            RuleEffect::Lane { lane } => trf("Goes to “{lane}”", &[("lane", &lane_name(lane))]),
            RuleEffect::Muted if is_hidden(rule) => tr("Hidden").to_owned(),
            RuleEffect::Muted => tr("Muted").to_owned(),
            // The core keeps no popups only with a trait; without one it would hold for the source.
            RuleEffect::NoPopup => tr("No popups").to_owned(),
        };
    }
    // A lane "for me" mutes nothing; `inbox.mute_item` never keeps one, so it only names the lane.
    let Some(effect) = MuteEffect::of(rule.effect) else {
        return trf("Goes to “{lane}”", &[("lane", &lane_name(Lane::ForMe))]);
    };
    let Some(until) = rule.until else {
        return match effect {
            MuteEffect::Hide => tr("Hidden"),
            MuteEffect::Info => tr("Only Info"),
            MuteEffect::NoPopup => tr("No popups"),
        }
        .to_owned();
    };
    let when = until_text(until, now);
    match effect {
        MuteEffect::Hide => trf("Hidden until {when}", &[("when", &when)]),
        MuteEffect::Info => trf("Only Info until {when}", &[("when", &when)]),
        MuteEffect::NoPopup => trf("No popups until {when}", &[("when", &when)]),
    }
}

/// The quiet line under a rule's title (task E8.12, finding 15): what it does ([`effect_words`]),
/// where it holds ([`place`]) and what it caught since when ([`caught`]), joined by " · ".
fn rule_line(rule: &Rule, hits: Option<&RuleHits>, now: DateTime<Utc>) -> String {
    [Some(effect_words(rule, now)), place(rule), caught(rule, hits, now)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// When a rule ends, seen from `now` in local time, as the Inbox's toast says it: "15:30" today,
/// "tomorrow 09:00", else "Fri, 16 Oct 09:00".
fn until_text(until: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (at, today) = (datetime::to_local(until), datetime::to_local(now).date());
    let time = datetime::format_time(at.time());
    if at.date() == today {
        time
    } else if at.date() == today + Days::new(1) {
        trf("tomorrow {time}", &[("time", &time)])
    } else {
        trf("{day} {time}", &[("day", &datetime::format_short_date(at.date(), today)), ("time", &time)])
    }
}

/// What a rule caught since when (task E8.8a), for the quiet line below its title: "Hid 12 since
/// Tuesday", "Kept 3 quiet since today", "Sent 9 since yesterday" (the lane stands just before it,
/// [`effect_words`]), "Nothing caught since 3 Oct". `None` when the core told nothing (before E8.8a).
fn caught(rule: &Rule, hits: Option<&RuleHits>, now: DateTime<Utc>) -> Option<String> {
    let hits = hits?;
    let (n, day) = (hits.hits, since_text(hits.since, now));
    if n == 0 {
        return Some(trf("Nothing caught since {day}", &[("day", &day)]));
    }
    Some(match rule.effect {
        RuleEffect::Muted => trf("Hid {n} since {day}", &[("n", &n), ("day", &day)]),
        RuleEffect::NoPopup => trf("Kept {n} quiet since {day}", &[("n", &n), ("day", &day)]),
        RuleEffect::Lane { .. } => trf("Sent {n} since {day}", &[("n", &n), ("day", &day)]),
    })
}

/// Since when, seen from `now` in local time: "today", "yesterday", the weekday within the last
/// week ("Tuesday"), else the day and month ("3 Oct").
fn since_text(since: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let (day, today) = (datetime::to_local(since).date(), datetime::to_local(now).date());
    if day >= today {
        tr("today").to_owned()
    } else if day + Days::new(1) == today {
        tr("yesterday").to_owned()
    } else if day + Days::new(7) > today {
        datetime::weekday_name(day, false).to_owned()
    } else {
        datetime::format_day_month(day)
    }
}

/// "For me" or "Info", the names the Inbox app gives its lanes.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// One rule (task E8.12, finding 15): what it is about as its title ([`subject`]), below it quietly
/// what it does, where it holds and what it caught ([`rule_line`]), each on one line cut with "…"
/// (the whole as tooltip), and "Remove" – rows of one height, as a settings group has them.
fn rule_row<'a>(state: &State, rule: &'a Rule, hits: Option<&RuleHits>, now: DateTime<Utc>) -> Element<'a, Message> {
    let remove = (!state.removing.contains(&rule.id)).then(|| Message::Remove(rule.id.clone()));
    let button = widgets::text_button(None, tr("Remove"), style::button::secondary, remove);
    let lines = column![
        widgets::ellipsized(typo::body(), subject(rule), theme::text),
        widgets::ellipsized(typo::meta(), rule_line(rule, hits, now), theme::text_2),
    ];
    wrapping_row(lines, button)
}

/// Where a rule for less than a whole source holds, for the quiet line below its title: "All
/// mail accounts" for a hidden sender or domain ([`is_hidden`], task E3.11) and for a muted sender,
/// domain, list or newsletters (task E8.6b: a trait holds for every source of the kind), "All
/// agents" for a muted project or kind of report, the account for a remembered lane; `None` for
/// rules about a whole source or one conversation, whose title names it.
fn place(rule: &Rule) -> Option<String> {
    if let Some(facet) = &rule.facet {
        return match facet.key {
            FacetKey::Sender | FacetKey::Domain | FacetKey::List | FacetKey::Bulk => {
                Some(tr("All mail accounts").to_owned())
            }
            FacetKey::Project | FacetKey::Kind => Some(tr("All agents").to_owned()),
            FacetKey::Account | FacetKey::Agent | FacetKey::Client | FacetKey::Conversation => None,
            // The traits of the remote sources (task E5.1); where they hold is shown with E5.14.
            FacetKey::Repo
            | FacetKey::Reason
            | FacetKey::Result
            | FacetKey::App
            | FacetKey::Person
            | FacetKey::Topic
            | FacetKey::Priority
            | FacetKey::Tag => None,
        };
    }
    if is_hidden(rule) {
        Some(tr("All mail accounts").to_owned())
    } else if rule.sender.is_some() || rule.domain.is_some() {
        Some(rule.source.label())
    } else {
        None
    }
}

/// The group "App": Torro Inbox with "Open"; off while the core has not reported the app as
/// installed.
fn app<'a>(data: &Data) -> Element<'a, Message> {
    // A button without a message is drawn disabled.
    let open = inbox_installed(data).then_some(Message::OpenInbox);
    let open = widgets::text_button(None, tr("Open"), style::button::secondary, open);
    widgets::group(
        tr("App"),
        vec![setting_row(None, AppId::INBOX.display_name, None, open)],
        Some(
            tr("Entries are handled in the Notification Hub: mark them done, put them off for later, move them or mute them.")
                .to_string(),
        ),
    )
}

/// Whether the core reported Torro Inbox as installed, so "Open" can start it.
fn inbox_installed(data: &Data) -> bool {
    data.apps.as_deref().unwrap_or_default().iter().any(|app| app.binary == AppId::INBOX.binary && app.installed)
}

/// A row of a settings group whose text may wrap: `lines` on the left, `control` at the right,
/// 12 above and below and 16 at the sides. One line of `BODY` makes it 44 high like
/// `setting_row`; a longer text makes it taller instead of cutting it off.
fn wrapping_row<'a>(lines: Column<'a, Message>, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let content = row![lines.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use torro_core::settings::Settings;
    use torro_inbox::SourceRef;
    use torro_inbox::item::Facet;
    use torro_ipc::AppInfo;

    use super::*;

    fn rule(id: &str, source: SourceRef, effect: RuleEffect) -> Rule {
        let created = DateTime::parse_from_rfc3339("2026-10-12T08:00:00Z").expect("time").with_timezone(&Utc);
        Rule { id: id.into(), source, sender: None, domain: None, effect, created, facet: None, until: None }
    }

    fn inbox_app(installed: bool) -> AppInfo {
        AppInfo {
            binary: AppId::INBOX.binary.into(),
            name: AppId::INBOX.display_name.into(),
            window_class: AppId::INBOX.window_class.into(),
            installed,
        }
    }

    #[test]
    fn the_switch_sets_only_quiet_when_busy() {
        for quiet in [true, false] {
            let action = update(&mut State::default(), Message::QuietWhenBusy(quiet), &Data::default());
            let Action::Send(Request::SetSetting { key, value }) = action else { panic!("expected SetSetting") };
            assert_eq!((key, value), (SettingKey::QuietWhenBusy, serde_json::json!(quiet)));
        }
    }

    #[test]
    fn remove_sends_the_rule_once_until_the_answer() {
        let mut state = State::default();
        let first = update(&mut state, Message::Remove("r1".into()), &Data::default());
        assert!(matches!(first, Action::Send(Request::RemoveRule { ref id }) if id == "r1"));
        let again = update(&mut state, Message::Remove("r1".into()), &Data::default());
        assert!(matches!(again, Action::None), "a second click while the first is on its way sends nothing");
        let other = update(&mut state, Message::Remove("r2".into()), &Data::default());
        assert!(matches!(other, Action::Send(Request::RemoveRule { ref id }) if id == "r2"));

        answered(&mut state);
        let retry = update(&mut state, Message::Remove("r1".into()), &Data::default());
        assert!(matches!(retry, Action::Send(_)), "after the answer the button works again");
    }

    #[test]
    fn open_starts_torro_inbox_through_the_core() {
        let action = update(&mut State::default(), Message::OpenInbox, &Data::default());
        assert!(matches!(action, Action::Send(Request::OpenApp { ref app, .. }) if app == "torroinbox"));
    }

    #[test]
    fn open_is_off_until_the_core_reports_the_inbox_installed() {
        assert!(!inbox_installed(&Data::default()), "no answer yet");
        assert!(!inbox_installed(&Data { apps: Some(vec![inbox_app(false)]), ..Data::default() }));
        assert!(inbox_installed(&Data { apps: Some(vec![inbox_app(true)]), ..Data::default() }));
    }

    fn hidden(id: &str, sender: Option<&str>, domain: Option<&str>) -> Rule {
        Rule {
            sender: sender.map(String::from),
            domain: domain.map(String::from),
            ..rule(id, SourceRef::mail("anna@example.org"), RuleEffect::Muted)
        }
    }

    #[test]
    fn lanes_come_before_mutes_then_hidden_senders_and_keep_their_order() {
        let rules = vec![
            hidden("h1", None, Some("shop.example")),
            rule("m1", SourceRef::script("nightly-build"), RuleEffect::Muted),
            rule("l1", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }),
            hidden("h2", Some("news@shop.example"), None),
            rule("m2", SourceRef::script("cron"), RuleEffect::Muted),
            rule("l2", SourceRef::script("backup"), RuleEffect::Lane { lane: Lane::Info }),
        ];
        let ids: Vec<&str> = ordered(&rules).iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(ids, ["l1", "l2", "m1", "m2", "h1", "h2"]);
    }

    /// Task E8.12 (finding 15): a hidden sender or domain is the title, "Hidden · All mail
    /// accounts" the line below.
    #[test]
    fn hidden_senders_and_domains_read_with_their_word() {
        let sender = hidden("h", Some("news@shop.example"), None);
        assert_eq!(subject(&sender), trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
        assert_eq!(effect_words(&sender, now()), tr("Hidden"));
        let domain = hidden("h", None, Some("shop.example"));
        assert_eq!(subject(&domain), trf("Everything from {domain}", &[("domain", &"shop.example")]));
        let lane = Rule {
            sender: Some("news@shop.example".into()),
            ..rule("l", SourceRef::mail("anna@example.org"), RuleEffect::Lane { lane: Lane::ForMe })
        };
        assert_eq!(subject(&lane), trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
        assert_eq!(effect_words(&lane, now()), trf("Goes to “{lane}”", &[("lane", &tr("For me"))]));
        assert_eq!(place(&sender).as_deref(), Some(tr("All mail accounts")), "hidden in every account");
        assert_eq!(place(&lane).as_deref(), Some("anna@example.org"), "a lane holds for its account");
        assert_eq!(place(&rule("m", SourceRef::script("cron"), RuleEffect::Muted)), None, "the title names it");
        let line = rule_line(&sender, None, now());
        assert_eq!(line, [tr("Hidden"), tr("All mail accounts")].join(" · "), "effect · scope");
        let remove = update(&mut State::default(), Message::Remove("h".into()), &Data::default());
        assert!(matches!(remove, Action::Send(Request::RemoveRule { ref id }) if id == "h"), "removed like any rule");
    }

    /// Task E8.12 (finding 15): the title is what the rule is about, the line below what it does.
    #[test]
    fn rules_read_as_subject_and_effect_with_the_source_label() {
        let lane = rule("l", SourceRef::agent("claude-code"), RuleEffect::Lane { lane: Lane::Info });
        assert_eq!(subject(&lane), trf("Everything from {source}", &[("source", &"Claude Code")]));
        assert_eq!(effect_words(&lane, now()), trf("Goes to “{lane}”", &[("lane", &tr("Info"))]));
        let muted = rule("m", SourceRef::script("backup"), RuleEffect::Muted);
        assert_eq!(subject(&muted), trf("Everything from {source}", &[("source", &"backup")]));
        assert_eq!(effect_words(&muted, now()), tr("Muted"));
        let hits = RuleHits { rule: "m".into(), hits: 31, since: now() };
        let caught = trf("Hid {n} since {day}", &[("n", &31), ("day", &tr("today"))]);
        assert_eq!(rule_line(&muted, Some(&hits), now()), [tr("Muted").to_owned(), caught].join(" · "));
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-14T08:00:00Z").expect("time").with_timezone(&Utc)
    }

    fn muted(id: &str, source: SourceRef, key: FacetKey, value: &str, effect: RuleEffect) -> Rule {
        Rule { facet: Some(Facet { key, value: value.into() }), ..rule(id, source, effect) }
    }

    /// Task E8.6b, E8.12: a muted trait reads with what it mutes as title, its effect and end below,
    /// holds where its kind reaches and comes after the other rules.
    #[test]
    fn muted_traits_read_with_effect_end_and_what() {
        let mail = SourceRef::mail("anna@example.org");
        let sender = muted("s", mail.clone(), FacetKey::Sender, "news@shop.example", RuleEffect::Muted);
        assert_eq!(subject(&sender), trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
        assert_eq!(effect_words(&sender, now()), tr("Hidden"));
        assert_eq!(place(&sender).as_deref(), Some(tr("All mail accounts")), "a sender in every account");

        let tomorrow = now() + chrono::Duration::days(1);
        let project = Rule {
            until: Some(tomorrow),
            ..muted("p", SourceRef::agent("claude-code"), FacetKey::Project, "torro", RuleEffect::NoPopup)
        };
        assert_eq!(subject(&project), trf("The project {project}", &[("project", &"torro")]));
        let when = until_text(tomorrow, now());
        assert!(when.starts_with(&trf("tomorrow {time}", &[("time", &"")])), "{when}");
        assert_eq!(effect_words(&project, now()), trf("No popups until {when}", &[("when", &when)]));
        assert_eq!(place(&project).as_deref(), Some(tr("All agents")));

        let info = RuleEffect::Lane { lane: Lane::Info };
        let bulk = muted("b", mail.clone(), FacetKey::Bulk, "bulk", info);
        assert_eq!(subject(&bulk), tr("Newsletters and automatic mail"));
        assert_eq!(effect_words(&bulk, now()), tr("Only Info"));

        let thread = muted("c", SourceRef::script("backup"), FacetKey::Conversation, "t-1", RuleEffect::NoPopup);
        assert_eq!(subject(&thread), trf("A conversation in {source}", &[("source", &"backup")]));
        assert_eq!(effect_words(&thread, now()), tr("No popups"));
        assert_eq!(place(&thread), None, "the title names it");

        let rules =
            vec![thread.clone(), rule("l", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }), sender];
        let ids: Vec<&str> = ordered(&rules).iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(ids, ["l", "c", "s"], "traits last, in the core's order");
    }

    /// Task E8.8a/E8.6b: below the title, what the rule caught since when.
    #[test]
    fn rules_say_what_they_caught_since_when() {
        let hits = |n, days| RuleHits { rule: "r".into(), hits: n, since: now() - chrono::Duration::days(days) };
        let hidden = rule("r", SourceRef::script("backup"), RuleEffect::Muted);
        let today = tr("today");
        assert_eq!(
            caught(&hidden, Some(&hits(12, 0)), now()),
            Some(trf("Hid {n} since {day}", &[("n", &12), ("day", &today)]))
        );
        let yesterday = tr("yesterday");
        let quiet = rule("r", SourceRef::script("backup"), RuleEffect::NoPopup);
        assert_eq!(
            caught(&quiet, Some(&hits(3, 1)), now()),
            Some(trf("Kept {n} quiet since {day}", &[("n", &3), ("day", &yesterday)]))
        );
        let three = since_text(now() - chrono::Duration::days(3), now());
        let weekday = datetime::weekday_name(datetime::to_local(now() - chrono::Duration::days(3)).date(), false);
        assert_eq!(three, weekday, "within the week its weekday");
        let lane = rule("r", SourceRef::script("backup"), RuleEffect::Lane { lane: Lane::Info });
        assert_eq!(
            caught(&lane, Some(&hits(9, 3)), now()),
            Some(trf("Sent {n} since {day}", &[("n", &9), ("day", &three)])),
            "the lane stands before it, in what the rule does"
        );
        let old = since_text(now() - chrono::Duration::days(30), now());
        assert_eq!(
            caught(&hidden, Some(&hits(0, 30)), now()),
            Some(trf("Nothing caught since {day}", &[("day", &old)]))
        );
        assert_eq!(caught(&hidden, None, now()), None, "a core before E8.8a tells nothing");
    }

    #[test]
    fn the_page_builds_with_and_without_rules() {
        let mut state = State::default();
        let rules = vec![
            rule("l", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }),
            rule("m", SourceRef::script("a-script-with-a-rather-long-name-that-wraps"), RuleEffect::Muted),
            hidden("h", None, Some("shop.example")),
        ];
        let quiet_off = Settings { quiet_when_busy: false, ..Settings::default() };
        let rule_hits = vec![RuleHits { rule: "m".into(), hits: 4, since: now() }];
        for data in [
            Data::default(),
            Data {
                rules: rules.clone(),
                rule_hits,
                settings: quiet_off,
                apps: Some(vec![inbox_app(true)]),
                ..Data::default()
            },
        ] {
            drop(view(&state, &data));
        }
        let _ = update(&mut state, Message::Remove("l".into()), &Data::default());
        drop(view(&state, &Data { rules, ..Data::default() }));
    }
}
