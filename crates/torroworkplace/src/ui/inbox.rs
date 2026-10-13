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
//!   then muted sources, then hidden senders and domains (task E3.10: "Hidden:
//!   news@shop.example", "Hidden: everything from shop.example", with "All mail accounts" below –
//!   such a rule holds for every account, task E3.11), each
//!   with "Remove", which sends `inbox.remove_rule { id }`. Without rules the group says where they
//!   come from.
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

use iced::widget::{Column, column, container, row, toggler};
use iced::{Alignment, Element, Fill};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, space};
use torro_gui::{layout, style, typo, widgets};
use torro_inbox::{Lane, Rule, RuleEffect};
use torro_ipc::{Request, SettingKey};

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
    column![notifications(data), rules(state, &data.rules), app(data)].spacing(layout::section_gap()).into()
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
            tr("Approvals and waiting agents always come through. Events marked as free and all-day events keep the inbox as it is.")
                .to_string(),
        ),
    )
}

/// The group "Rules": the rules in the order [`ordered`] gives them, or a sentence where they come
/// from.
fn rules<'a>(state: &State, rules: &'a [Rule]) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = if rules.is_empty() {
        let hint = typo::text(
            typo::body(),
            tr("Rules appear here when you move notices to a lane for good, mute a source or hide a sender in the Inbox."),
        )
        .style(theme::text_2);
        vec![container(hint).width(Fill).padding([space::m(), space::l()]).into()]
    } else {
        ordered(rules).into_iter().map(|rule| rule_row(state, rule)).collect()
    };
    widgets::group(
        tr("Rules"),
        rows,
        Some(tr("Rules apply to new notices. Removing one leaves the entries in the Inbox as they are.").to_string()),
    )
}

/// The rules for the page: remembered lanes first, then muted sources, then hidden senders and
/// domains, each kind in the core's order (oldest first), so a new rule always lands at the end of
/// its kind.
fn ordered(rules: &[Rule]) -> Vec<&Rule> {
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    ordered.sort_by_key(|rule| match rule.effect {
        RuleEffect::Lane { .. } => 0,
        RuleEffect::Muted if is_hidden(rule) => 2,
        RuleEffect::Muted => 1,
        // Muted traits (task E8.6a) get their own words with task E8.6b.
        RuleEffect::NoPopup => 3,
    });
    ordered
}

/// A rule that hides one sender or domain of a mail account (task E3.10), not a whole source.
fn is_hidden(rule: &Rule) -> bool {
    rule.effect == RuleEffect::Muted && (rule.sender.is_some() || rule.domain.is_some())
}

/// What a rule does, as a sentence: "Notices of Claude Code go to “Info”", "Mail from
/// news@shop.example goes to “For me”", "backup is muted", "Hidden: news@shop.example", "Hidden:
/// everything from shop.example". Addresses and domains are shown as they are, like source names.
fn sentence(rule: &Rule) -> String {
    let source = rule.source.label();
    match (rule.effect, &rule.sender, &rule.domain) {
        (RuleEffect::Lane { lane }, Some(sender), _) => {
            trf("Mail from {sender} goes to “{lane}”", &[("sender", sender), ("lane", &lane_name(lane))])
        }
        (RuleEffect::Lane { lane }, None, _) => {
            trf("Notices of {source} go to “{lane}”", &[("source", &source), ("lane", &lane_name(lane))])
        }
        (RuleEffect::Muted, Some(sender), _) => trf("Hidden: {sender}", &[("sender", sender)]),
        (RuleEffect::Muted, None, Some(domain)) => trf("Hidden: everything from {domain}", &[("domain", domain)]),
        (RuleEffect::Muted, None, None) => trf("{source} is muted", &[("source", &source)]),
        // Words that name the trait come with task E8.6b.
        (RuleEffect::NoPopup, _, _) => trf("{source}: no popups", &[("source", &source)]),
    }
}

/// "For me" or "Info", the names the Inbox app gives its lanes.
fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::ForMe => tr("For me"),
        Lane::Info => tr("Info"),
    }
}

/// One rule: its sentence, wrapped rather than cut off (a source name can be long), and "Remove".
/// A rule for one sender names where it holds below, quietly: a hidden sender or domain "All mail
/// accounts" (task E3.11), a lane remembered for a sender its account, which it holds for alone.
fn rule_row<'a>(state: &State, rule: &'a Rule) -> Element<'a, Message> {
    let remove = (!state.removing.contains(&rule.id)).then(|| Message::Remove(rule.id.clone()));
    let button = widgets::text_button(None, tr("Remove"), style::button::secondary, remove);
    let mut lines = column![typo::text(typo::body(), sentence(rule))];
    if let Some(place) = place(rule) {
        lines = lines.push(typo::text(typo::meta(), place).style(theme::text_2));
    }
    wrapping_row(lines, button)
}

/// Where a rule for one sender or domain holds, for the quiet line below its sentence: "All mail
/// accounts" for a hidden one ([`is_hidden`], task E3.11), the account for a remembered lane;
/// `None` for rules about a whole source, whose sentence names it.
fn place(rule: &Rule) -> Option<String> {
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
            tr("Entries are handled in the Inbox: mark them done, snooze them, move them or mute their source.")
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

    #[test]
    fn hidden_senders_and_domains_read_with_their_word() {
        let sender = hidden("h", Some("news@shop.example"), None);
        assert_eq!(sentence(&sender), trf("Hidden: {sender}", &[("sender", &"news@shop.example")]));
        let domain = hidden("h", None, Some("shop.example"));
        assert_eq!(sentence(&domain), trf("Hidden: everything from {domain}", &[("domain", &"shop.example")]));
        let lane = Rule {
            sender: Some("news@shop.example".into()),
            ..rule("l", SourceRef::mail("anna@example.org"), RuleEffect::Lane { lane: Lane::ForMe })
        };
        assert_eq!(
            sentence(&lane),
            trf("Mail from {sender} goes to “{lane}”", &[("sender", &"news@shop.example"), ("lane", &tr("For me"))])
        );
        assert_eq!(place(&sender).as_deref(), Some(tr("All mail accounts")), "hidden in every account");
        assert_eq!(place(&lane).as_deref(), Some("anna@example.org"), "a lane holds for its account");
        assert_eq!(place(&rule("m", SourceRef::script("cron"), RuleEffect::Muted)), None, "the sentence names it");
        let remove = update(&mut State::default(), Message::Remove("h".into()), &Data::default());
        assert!(matches!(remove, Action::Send(Request::RemoveRule { ref id }) if id == "h"), "removed like any rule");
    }

    #[test]
    fn rules_read_as_sentences_with_the_source_label() {
        let lane = rule("l", SourceRef::agent("claude-code"), RuleEffect::Lane { lane: Lane::Info });
        let source = "Claude Code".to_string();
        assert_eq!(
            sentence(&lane),
            trf("Notices of {source} go to “{lane}”", &[("source", &source), ("lane", &tr("Info"))])
        );
        let muted = rule("m", SourceRef::script("backup"), RuleEffect::Muted);
        assert_eq!(sentence(&muted), trf("{source} is muted", &[("source", &"backup")]));
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
        for data in [
            Data::default(),
            Data { rules: rules.clone(), settings: quiet_off, apps: Some(vec![inbox_app(true)]), ..Data::default() },
        ] {
            drop(view(&state, &data));
        }
        let _ = update(&mut state, Message::Remove("l".into()), &Data::default());
        drop(view(&state, &Data { rules, ..Data::default() }));
    }
}
