//! The sections behind the gear of the inbox's windows "Notification Hub" and "App Feed" (task
//! S.20 in `docs/tasks/steuerzentrale.md`, concept `docs/settings.md`): how loud the inbox is,
//! which source brings anything, and the lasting rules the person made in the app.
//!
//! ```text
//! Notifications  quiet during busy events
//! Sources        per account: Nextcloud notifications and Talk, GitHub, an ntfy server,
//!                and per mail account "Show new mail" (All / Only "For me" / Off)
//! Rules          every lasting rule with "Remove"
//! ```
//!
//! The mail window has its own sections ([`crate::mail`]); these two windows show these three and
//! nothing else, so no setting has two places (`docs/settings.md`, "Die Regel").
//!
//! **What is not here:** the subscriptions and labels of the ntfy topics, which belong to the
//! account and stay in the control centre ([`crate::Action::ControlCentre`] leads there), and
//! everything about accounts, rights and keys. Muting works on a whole source only: a single ntfy
//! topic cannot be muted (`inbox.mute_source` knows servers, not topics).
//!
//! Ported from the control centre's pages (`torroworkplace::ui::inbox` and the group "Notification
//! Hub" of its account pages), which task S.21 removes.

use std::collections::BTreeSet;

use chrono::Utc;
use iced::widget::column;
use iced::{Element, Fill};

use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::layout;
use torro_gui::widgets;
use torro_ipc::{AccountInfo, Request, SettingKey};

use crate::parts::{picker, setting_row, switch};
use crate::{Action, Data, Page, Section};

/// The sections, in the order of the list. [`Section::id`] is what `--settings <id>` takes.
pub const SECTIONS: [Section; 3] = [
    // Notifications is first: the gear lands on how loud the window is.
    Section { id: "notifications", icon: Icon::Bell, label: "Notifications" },
    // The sources lie one over the other like layers; the bell is taken and every source icon
    // stands for one kind only (design skill, section 5).
    Section { id: "sources", icon: Icon::Layers, label: "Sources" },
    // Rules sort what comes in, like a filter on the list.
    Section { id: "rules", icon: Icon::ListFilter, label: "Rules" },
];

/// What one load of these sections needs: the suite-wide settings (Notifications), every account
/// with its mail settings (Sources), the sources with their mutes and the rules.
pub fn wanted() -> Vec<Request> {
    // The catalogue names the categories of "Always for this sender?" rules (task E15.2).
    vec![
        Request::Settings,
        Request::Accounts,
        Request::ListSources,
        Request::ListRules,
        Request::ListCategories { account: None },
    ]
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// Rules: whose "Remove" is on its way. Notifications and Sources keep nothing – they show
    /// what [`Data`] holds.
    pub rules: rules::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Notifications(notifications::Message),
    Sources(sources::Message),
    Rules(rules::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::Notifications(message) => notifications::update(message),
        Message::Sources(message) => sources::update(message, data),
        Message::Rules(message) => rules::update(&mut state.rules, message),
    }
}

/// The page of the section with this id, as the host takes it. An id no section has shows
/// Notifications, the section the gear lands on; the host only ever asks for one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str) -> Page<'a, Message> {
    match section {
        "sources" => Page::new(sources::view(data).map(Message::Sources)),
        // Ends and "since" are said from now, as the core counts them.
        "rules" => Page::new(rules::view(&state.rules, data, Utc::now()).map(Message::Rules)),
        _ => Page::new(notifications::view(data).map(Message::Notifications)),
    }
}

/// A change or a load came back: nothing is on its way any more, so every "Remove" works again –
/// the removed rules are gone from the fresh [`Data`], a failed one shows its button once more.
/// The app calls this for the host's own answers (task S.20).
pub fn answered(state: &mut State) {
    rules::answered(&mut state.rules);
}

/// "Quiet during busy events": how loud the window is while a busy event runs (task E3.7).
pub mod notifications {
    use super::*;

    /// What this section changes (`settings.set_setting quiet_when_busy`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Message {
        QuietWhenBusy(bool),
    }

    pub fn update<A>(message: Message) -> Action<A> {
        match message {
            Message::QuietWhenBusy(quiet) => Action::send(Request::SetSetting {
                key: SettingKey::QuietWhenBusy,
                value: serde_json::Value::Bool(quiet),
            }),
        }
    }

    pub fn view(data: &Data) -> Element<'_, Message> {
        let row = setting_row(
            None,
            tr("Quiet during busy events"),
            Some(tr("Only urgent entries pop up while a busy event runs; a summary follows.").to_string()),
            switch(data.settings.quiet_when_busy, Message::QuietWhenBusy),
        );
        widgets::group(
            tr("Notifications"),
            vec![row],
            Some(
                tr("Approvals and waiting agents always come through. Events marked as free and all-day events keep the Notification Hub as it is.")
                    .to_string(),
            ),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The switch sends `set_setting quiet_when_busy` and nothing else.
        #[test]
        fn the_switch_sets_only_quiet_when_busy() {
            for quiet in [true, false] {
                let action: Action<()> = update(Message::QuietWhenBusy(quiet));
                assert_eq!(
                    action,
                    Action::send(Request::SetSetting {
                        key: SettingKey::QuietWhenBusy,
                        value: serde_json::json!(quiet)
                    })
                );
            }
        }

        #[test]
        fn the_page_builds() {
            let _: Element<'_, Message> = view(&Data::default());
        }
    }
}

/// "Sources": per account a switch for every source it brings, and per mail account the choice
/// which new mail becomes an entry.
///
/// Muting goes through `inbox.mute_source`, which knows whole sources: a Nextcloud's
/// notifications, its Talk, a GitHub account, an ntfy **server**. A mail account has its own field
/// instead (`accounts.update_mail_account`, `notify`), with three steps.
pub mod sources {
    use torro_inbox::item::SourceHealth;
    use torro_inbox::{SourceKind, SourceRef};
    use torro_mail::Notify;

    use super::*;

    /// What a row of this section changes.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Message {
        /// The switch of one source (`inbox.mute_source`).
        Shown { source: SourceRef, shown: bool },
        /// "Show new mail" of the mail account with this id (`accounts.update_mail_account`,
        /// field `notify`).
        Notify { account: String, notify: Notify },
    }

    pub fn update<A>(message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Shown { source, shown } => Action::send(Request::MuteSource { source, muted: !shown }),
            Message::Notify { account, notify } => {
                let mut requests = vec![notify_request(account.clone(), notify)];
                // A mail source that the first-run setup muted with `inbox.mute_source` stays
                // quiet whatever `notify` says. Turning it on therefore lifts that old rule in the
                // same step, so nothing of it is left behind (task S.20); turning it off needs
                // `notify` alone – the one place this is stored from now on.
                if notify != Notify::Off
                    && let Ok(source) = account.parse::<SourceRef>()
                    && !is_shown(data, &source)
                {
                    requests.push(Request::MuteSource { source, muted: false });
                }
                Action::Send(requests)
            }
        }
    }

    /// `accounts.update_mail_account` with only `notify`; every other setting of the account stays
    /// as it is.
    fn notify_request(id: String, notify: Notify) -> Request {
        Request::UpdateMailAccount {
            id,
            sender_name: None,
            signature: None,
            notify: Some(notify),
            services: None,
            folder_roles: None,
            format: None,
        }
    }

    /// Whether the inbox shows `source`: everything is shown unless `inbox.list_sources` says
    /// muted.
    pub fn is_shown(data: &Data, source: &SourceRef) -> bool {
        !data.sources.iter().any(|info| info.source == *source && info.muted)
    }

    /// What a mail account's row shows: what the account stores, and "Off" while an old
    /// `inbox.mute_source` rule still mutes its source – that rule wins over `notify`, and the row
    /// must not promise mail that never arrives (task S.20).
    pub fn mail_notify(account: &AccountInfo, source: &SourceRef, data: &Data) -> Notify {
        let stored = account.mail.as_ref().map_or_else(Notify::default, |mail| mail.settings.notify);
        if is_shown(data, source) { stored } else { Notify::Off }
    }

    /// Why `source` brings nothing at the moment, as the core keeps it.
    fn health_of(data: &Data, source: &SourceRef) -> Option<SourceHealth> {
        data.sources.iter().find(|info| info.source == *source).and_then(|info| info.health)
    }

    /// Why a source of `kind` is quiet, in calm words that say what helps (ported from the control
    /// centre's account pages, task E5.1).
    pub fn health_text(kind: SourceKind, health: SourceHealth) -> &'static str {
        match (kind, health) {
            (SourceKind::Github, SourceHealth::MissingScope) => {
                tr("Notifications need a classic token with notifications or repo, or the GitHub CLI sign-in.")
            }
            (_, SourceHealth::MissingScope) => tr("This sign-in may not read notifications."),
            (SourceKind::Talk, SourceHealth::MissingApp) => tr("This Nextcloud has no Talk."),
            (_, SourceHealth::MissingApp) => tr("This Nextcloud has no notifications app, so nothing comes from it."),
            (_, SourceHealth::SignedOut) => tr("The server refused the sign-in. Sign in again to get notifications."),
            (_, SourceHealth::Offline) => tr("The server cannot be reached. Notifications come once it answers."),
        }
    }

    /// The page: one group per account that brings a source, in the order the accounts come; a
    /// quiet hint while none does.
    ///
    /// What muting means stands once, under the last group, as the suite's settings pages say such
    /// a sentence once; a kind with more to say (mail, ntfy) says it at its own group.
    pub fn view(data: &Data) -> Element<'_, Message> {
        let groups: Vec<(String, Vec<Element<'_, Message>>, Option<String>)> = data
            .accounts
            .iter()
            // A sign-in that still runs has no id and brings nothing yet.
            .filter(|account| !account.id.is_empty())
            .filter_map(|account| {
                let source = account.id.parse::<SourceRef>().ok()?;
                let rows = rows_of(account, &source, data);
                (!rows.is_empty()).then(|| (group_title(account, &source), rows, footer_of(&source)))
            })
            .collect();
        if groups.is_empty() {
            return widgets::empty_state(
                tr("No sources"),
                tr("Sources appear here once an account brings notifications or mail."),
            );
        }
        let muting = tr("A muted source brings nothing new; what is already in the Notification Hub stays.");
        let last = groups.len() - 1;
        let built = groups.into_iter().enumerate().map(|(at, (title, rows, own))| {
            let footer = match (at == last, own) {
                (true, Some(own)) => Some(format!("{own} {muting}")),
                (true, None) => Some(muting.to_string()),
                (false, own) => own,
            };
            widgets::group(title, rows, footer)
        });
        column(built).spacing(layout::section_gap()).into()
    }

    /// How a group is named: what kind of account it is and its name ("GitHub · anna-berg"), so a
    /// bare login or host says where it comes from. The product names are not translated; a mail
    /// account, whose name is its address, is "Mail".
    fn group_title(account: &AccountInfo, source: &SourceRef) -> String {
        let kind = match source.kind {
            SourceKind::Mail => tr("Mail").to_owned(),
            _ => source.label(),
        };
        format!("{kind} · {}", account.label)
    }

    /// The rows one account brings: its sources with a switch each, a mail account its three-step
    /// choice. Every other kind (an AI account, a CalDAV server) brings none.
    fn rows_of<'a>(account: &AccountInfo, source: &SourceRef, data: &Data) -> Vec<Element<'a, Message>> {
        match source.kind {
            SourceKind::Github => vec![switch_row(tr("Notifications"), None, source.clone(), data)],
            SourceKind::Nextcloud => vec![
                switch_row(tr("Notifications"), Some(tr("From Nextcloud and its apps")), source.clone(), data),
                switch_row(
                    tr("Talk"),
                    Some(tr("Unread conversations and mentions")),
                    SourceRef::talk(source.name.clone()),
                    data,
                ),
            ],
            // One switch for the whole server: `inbox.mute_source` knows no single topic.
            SourceKind::Ntfy => {
                vec![switch_row(tr("Notifications"), Some(tr("Every topic of this server")), source.clone(), data)]
            }
            SourceKind::Mail => vec![mail_row(account, source, data)],
            SourceKind::Suite | SourceKind::Agent | SourceKind::Script | SourceKind::Talk => Vec::new(),
        }
    }

    /// One source with its switch: its name, under it why it is quiet, else what it brings.
    fn switch_row<'a>(
        label: &'static str,
        brings: Option<&'static str>,
        source: SourceRef,
        data: &Data,
    ) -> Element<'a, Message> {
        let shown = is_shown(data, &source);
        let kind = source.kind;
        let detail = health_of(data, &source).map(|health| health_text(kind, health)).or(brings).map(str::to_string);
        let control = switch(shown, move |shown| Message::Shown { source: source.clone(), shown });
        setting_row(None, label, detail, control)
    }

    /// A mail account's "Show new mail": all of it, only what the inbox's rules put into "For me",
    /// or none.
    fn mail_row<'a>(account: &AccountInfo, source: &SourceRef, data: &Data) -> Element<'a, Message> {
        let id = account.id.clone();
        let chosen = NotifyChoice(mail_notify(account, source, data));
        let control = picker(NOTIFY_CHOICES, chosen, move |choice: NotifyChoice| Message::Notify {
            account: id.clone(),
            notify: choice.0,
        });
        setting_row(None, tr("Show new mail"), None, control)
    }

    /// What a group says under its rows beyond what muting means everywhere ([`view`]): only the
    /// kinds with something of their own to say.
    fn footer_of(source: &SourceRef) -> Option<String> {
        let sentence = match source.kind {
            SourceKind::Mail => tr("Only \"For me\" leaves out newsletters and other automatic mail."),
            SourceKind::Ntfy => tr(
                "Muting takes the whole server, not one topic. Topics and their labels stay with the account in Torro Workplace.",
            ),
            SourceKind::Github
            | SourceKind::Nextcloud
            | SourceKind::Suite
            | SourceKind::Agent
            | SourceKind::Script
            | SourceKind::Talk => return None,
        };
        Some(sentence.to_string())
    }

    /// "Show new mail" as a choice of a pick list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct NotifyChoice(pub Notify);

    const NOTIFY_CHOICES: [NotifyChoice; 3] =
        [NotifyChoice(Notify::All), NotifyChoice(Notify::ForMeOnly), NotifyChoice(Notify::Off)];

    impl std::fmt::Display for NotifyChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                Notify::All => tr("All"),
                Notify::ForMeOnly => tr("Only \"For me\""),
                Notify::Off => tr("Off"),
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use torro_inbox::SourceInfo;

        use super::*;
        use crate::demo::Demo;

        /// Everything the demo answers: accounts of every kind with their sources.
        async fn data() -> Data {
            let backend: std::sync::Arc<dyn crate::Backend> = std::sync::Arc::new(Demo::new());
            crate::load(backend, wanted()).await
        }

        fn muted(source: SourceRef) -> SourceInfo {
            SourceInfo { source, open: 0, muted: true, health: None }
        }

        /// A switch sends `inbox.mute_source` for its own source and nothing else.
        #[test]
        fn a_switch_mutes_and_unmutes_its_source() {
            let source = SourceRef::github("anna-berg");
            let off: Action<()> = update(Message::Shown { source: source.clone(), shown: false }, &Data::default());
            assert_eq!(off, Action::send(Request::MuteSource { source: source.clone(), muted: true }));
            let on: Action<()> = update(Message::Shown { source: source.clone(), shown: true }, &Data::default());
            assert_eq!(on, Action::send(Request::MuteSource { source, muted: false }));
        }

        /// "Show new mail" sends `accounts.update_mail_account` with only `notify`.
        #[test]
        fn show_new_mail_sets_only_notify() {
            let account = "mail:anna@mailbox.org";
            for notify in [Notify::All, Notify::ForMeOnly, Notify::Off] {
                let action: Action<()> = update(Message::Notify { account: account.into(), notify }, &Data::default());
                assert_eq!(
                    action,
                    Action::Send(vec![notify_request(account.into(), notify)]),
                    "an unmuted source needs nothing but its field"
                );
            }
        }

        /// Task S.20: a mail source the setup muted with `inbox.mute_source` reads "Off", and
        /// turning it on lifts that old rule in the same step – else the mute would win over
        /// `notify` and no mail would come.
        #[test]
        fn an_old_mute_reads_off_and_is_lifted_when_it_is_turned_on() {
            let id = "mail:anna@mailbox.org";
            let source = SourceRef::mail("anna@mailbox.org");
            let account: AccountInfo = serde_json::from_value(serde_json::json!({
                "id": id,
                "kind": "mail",
                "label": "anna@mailbox.org",
                "state": {"state": "connected", "server": "imap.mailbox.org", "login_name": "anna@mailbox.org"},
                "collections": [],
                "mail": {
                    "address": "anna@mailbox.org",
                    "sender_name": "Anna",
                    "imap": {"host": "imap.mailbox.org", "port": 993, "security": "tls"},
                    "smtp": {"host": "smtp.mailbox.org", "port": 465, "security": "tls"},
                    "settings": {"notify": "all"},
                    "mail": {}
                }
            }))
            .expect("a mail account");
            let data =
                Data { accounts: vec![account.clone()], sources: vec![muted(source.clone())], ..Data::default() };
            assert_eq!(mail_notify(&account, &source, &data), Notify::Off, "the mute wins over the stored field");

            let on: Action<()> = update(Message::Notify { account: id.into(), notify: Notify::All }, &data);
            assert_eq!(
                on,
                Action::Send(vec![
                    notify_request(id.into(), Notify::All),
                    Request::MuteSource { source: source.clone(), muted: false },
                ]),
                "the field and the old rule, in that order"
            );
            let off: Action<()> = update(Message::Notify { account: id.into(), notify: Notify::Off }, &data);
            assert_eq!(off, Action::Send(vec![notify_request(id.into(), Notify::Off)]), "off is only the field");

            // Without the old rule the row shows what the account stores.
            let plain = Data { accounts: vec![account.clone()], ..Data::default() };
            assert_eq!(mail_notify(&account, &source, &plain), Notify::All);
        }

        /// One group per account that brings a source; an account without one brings none.
        #[tokio::test]
        async fn the_page_groups_the_sources_by_account() {
            let data = data().await;
            let with_sources = data
                .accounts
                .iter()
                .filter(|account| account.id.parse::<SourceRef>().is_ok_and(|source| source.kind != SourceKind::Suite))
                .count();
            assert!(with_sources > 0, "the demo has an account with sources");
            let _: Element<'_, Message> = view(&data);
            let _: Element<'_, Message> = view(&Data::default());
        }

        /// Every quiet source says why in words a person can act on.
        #[test]
        fn every_health_has_its_sentence() {
            for health in
                [SourceHealth::MissingApp, SourceHealth::MissingScope, SourceHealth::SignedOut, SourceHealth::Offline]
            {
                for kind in [SourceKind::Github, SourceKind::Nextcloud, SourceKind::Talk, SourceKind::Ntfy] {
                    assert!(!health_text(kind, health).is_empty(), "{kind:?} {health:?}");
                }
            }
        }

        /// The three steps read as the control centre's page had them.
        #[test]
        fn the_choices_read_as_all_for_me_and_off() {
            let words: Vec<String> = NOTIFY_CHOICES.iter().map(ToString::to_string).collect();
            assert_eq!(words, [tr("All"), tr("Only \"For me\""), tr("Off")]);
        }
    }
}

/// "Rules": every lasting rule the person made in the inbox, with "Remove".
///
/// Ported from the control centre's inbox page (tasks E3.10, E3.11, E8.6b, E8.8a, E8.12): the
/// title says what a rule is about ([`subject`]), the quiet line under it what it does, where it
/// holds and what it caught ([`rule_line`]).
///
/// "Remove" asks nothing: removing a rule destroys nothing – the entries stay as they are, only
/// new notices are sorted the normal way again, and one action in the inbox ("Always", "Mute")
/// brings the same rule back.
pub mod rules {
    use chrono::{DateTime, Days};
    use iced::Alignment;
    use iced::widget::{column, row};
    use torro_gui::theme::{self, space};
    use torro_gui::{datetime, style, typo};
    use torro_inbox::item::{FacetKey, MuteEffect};
    use torro_inbox::{Lane, Rule, RuleEffect, rules as inbox_rules};
    use torro_ipc::RuleHits;
    use torro_ipc::inbox::CategoryInfo;

    use super::*;
    use crate::parts::row_frame;
    use torro_core::i18n::trf;

    /// The rules whose removal is on its way, so their "Remove" waits for the answer.
    #[derive(Debug, Default)]
    pub struct State {
        removing: BTreeSet<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Message {
        /// "Remove" was pressed on the rule with this id (`inbox.remove_rule`).
        Remove(String),
    }

    pub fn update<A>(state: &mut State, message: Message) -> Action<A> {
        match message {
            Message::Remove(id) => {
                // A second click while the first is on its way sends nothing, so the core does not
                // answer `not-found` for a rule that is already gone.
                if !state.removing.insert(id.clone()) {
                    return Action::None;
                }
                Action::send(Request::RemoveRule { id })
            }
        }
    }

    /// An answer came back: no removal is on its way any more ([`super::answered`]).
    pub fn answered(state: &mut State) {
        state.removing.clear();
    }

    /// The page: every rule in the order [`ordered`] gives them, each with what it caught seen
    /// from `now`, or a sentence where rules come from.
    pub fn view<'a>(state: &'a State, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
        if data.rules.is_empty() {
            return widgets::empty_state(
                tr("No rules"),
                tr(
                    "A rule appears here when you mute something with the bell, move notices to a lane for good or hide a sender.",
                ),
            );
        }
        let caught = |rule: &Rule| data.rule_hits.iter().find(|hit| hit.rule == rule.id);
        let rows = ordered(&data.rules)
            .into_iter()
            .map(|rule| rule_row(state, rule, caught(rule), now, &data.categories))
            .collect::<Vec<_>>();
        widgets::group(
            tr("Rules"),
            rows,
            Some(
                tr("Rules apply to new notices. Removing one leaves the entries in the Notification Hub as they are.")
                    .to_string(),
            ),
        )
    }

    /// One rule: what it is about as its title, under it quietly what it does, where it holds and
    /// what it caught, each on one line cut with "…", and "Remove" at the end.
    fn rule_row<'a>(
        state: &State,
        rule: &'a Rule,
        hits: Option<&RuleHits>,
        now: DateTime<Utc>,
        categories: &[CategoryInfo],
    ) -> Element<'a, Message> {
        let remove = (!state.removing.contains(&rule.id)).then(|| Message::Remove(rule.id.clone()));
        let button = widgets::text_button(None, tr("Remove"), style::button::secondary, remove);
        let lines = column![
            widgets::ellipsized(typo::body(), subject(rule), theme::text),
            widgets::ellipsized(typo::meta(), rule_line(rule, hits, now, categories), theme::text_2),
        ];
        row_frame(row![lines.width(Fill), button].spacing(space::l()).align_y(Alignment::Center), true)
    }

    /// The rules for the page: remembered lanes first, then muted sources, then hidden senders and
    /// domains, then muted traits, each kind in the core's order (oldest first), so a new rule
    /// always lands at the end of its kind.
    fn ordered(rules: &[Rule]) -> Vec<&Rule> {
        let mut ordered: Vec<&Rule> = rules.iter().collect();
        ordered.sort_by_key(|rule| match rule.effect {
            _ if rule.facet.is_some() => 3,
            RuleEffect::Lane { .. } => 0,
            RuleEffect::Muted if is_hidden(rule) => 2,
            RuleEffect::Muted => 1,
            // Only rules with a trait keep no popups; one without would be about its whole source.
            RuleEffect::NoPopup => 3,
            // Kept categories last (task E12.2): they say what mail is, they hide nothing.
            RuleEffect::Category { .. } => 4,
        });
        ordered
    }

    /// A rule that hides one sender or domain of a mail account (task E3.10), not a whole source.
    fn is_hidden(rule: &Rule) -> bool {
        rule.effect == RuleEffect::Muted && (rule.sender.is_some() || rule.domain.is_some())
    }

    /// What a rule is about – the title of its row: "Everything from Claude Code" for a whole
    /// source, "Mail from news@shop.example" for a sender, "Everything from shop.example" for a
    /// domain, and for a trait muted with the bell's menu its words. A conversation is named by
    /// its source: its thread key means nothing to a person. Addresses, domains and source names
    /// are shown as they are (a `text` interprets nothing).
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

    /// What a rule does, with its end: "Goes to “Info”" for a remembered lane, "Muted" for a whole
    /// source, "Hidden" for a sender or domain, and for a trait "Hidden", "Only Info" or "No
    /// popups", with "until tomorrow 09:00" while it ends.
    fn effect_words(rule: &Rule, now: DateTime<Utc>, categories: &[CategoryInfo]) -> String {
        if rule.facet.is_none() {
            return match rule.effect {
                RuleEffect::Lane { lane } => trf("Goes to “{lane}”", &[("lane", &lane_name(lane))]),
                RuleEffect::Muted if is_hidden(rule) => tr("Hidden").to_owned(),
                RuleEffect::Muted => tr("Muted").to_owned(),
                // The core keeps no popups only with a trait; without one it would hold for the
                // whole source.
                RuleEffect::NoPopup => tr("No popups").to_owned(),
                // "Always for this sender?" after the person sorted one mail by hand (E12.2).
                RuleEffect::Category { category } => {
                    trf("Filed as “{category}”", &[("category", &crate::category_name_in(categories, category))])
                }
            };
        }
        // A lane "for me" mutes nothing; `inbox.mute_item` never keeps one, so it only names the
        // lane.
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

    /// The quiet line under a rule's title: what it does, where it holds and what it caught since
    /// when, joined by " · ".
    fn rule_line(rule: &Rule, hits: Option<&RuleHits>, now: DateTime<Utc>, categories: &[CategoryInfo]) -> String {
        [Some(effect_words(rule, now, categories)), place(rule), caught(rule, hits, now)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// When a rule ends, seen from `now` in local time, as the inbox's toast says it: "15:30"
    /// today, "tomorrow 09:00", else "Fri, 16 Oct 09:00".
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

    /// What a rule caught since when (task E8.8a): "Hid 12 since Tuesday", "Kept 3 quiet since
    /// today", "Sent 9 since yesterday", "Nothing caught since 3 Oct". `None` when the core told
    /// nothing.
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
            RuleEffect::Category { .. } => trf("Filed {n} since {day}", &[("n", &n), ("day", &day)]),
        })
    }

    /// Since when, seen from `now` in local time: "today", "yesterday", the weekday within the
    /// last week ("Tuesday"), else the day and month ("3 Oct").
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

    /// "For me" or "Info", the names the inbox gives its lanes.
    fn lane_name(lane: Lane) -> &'static str {
        match lane {
            Lane::ForMe => tr("For me"),
            Lane::Info => tr("Info"),
        }
    }

    /// Where a rule for less than a whole source holds: "All mail accounts" for a hidden or muted
    /// sender, domain, list or newsletters (a trait holds for every source of its kind, task
    /// E8.6b), "All agents" for a muted project or kind of report, the account for a remembered
    /// lane; `None` for rules about a whole source or one conversation, whose title names it.
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

    #[cfg(test)]
    mod tests {
        use torro_inbox::SourceRef;
        use torro_inbox::item::Facet;

        use super::*;

        fn now() -> DateTime<Utc> {
            DateTime::parse_from_rfc3339("2026-10-14T08:00:00Z").expect("time").with_timezone(&Utc)
        }

        fn rule(id: &str, source: SourceRef, effect: RuleEffect) -> Rule {
            let created = DateTime::parse_from_rfc3339("2026-10-12T08:00:00Z").expect("time").with_timezone(&Utc);
            Rule { id: id.into(), source, sender: None, domain: None, effect, created, facet: None, until: None }
        }

        fn hidden(id: &str, sender: Option<&str>, domain: Option<&str>) -> Rule {
            Rule {
                sender: sender.map(String::from),
                domain: domain.map(String::from),
                ..rule(id, SourceRef::mail("anna@example.org"), RuleEffect::Muted)
            }
        }

        /// "Remove" sends `inbox.remove_rule` once until the answer came.
        #[test]
        fn remove_sends_the_rule_once_until_the_answer() {
            let mut state = State::default();
            let first: Action<()> = update(&mut state, Message::Remove("r1".into()));
            assert_eq!(first, Action::send(Request::RemoveRule { id: "r1".into() }));
            let again: Action<()> = update(&mut state, Message::Remove("r1".into()));
            assert_eq!(again, Action::None, "a second click while the first is on its way sends nothing");
            let other: Action<()> = update(&mut state, Message::Remove("r2".into()));
            assert_eq!(other, Action::send(Request::RemoveRule { id: "r2".into() }));
            answered(&mut state);
            let retry: Action<()> = update(&mut state, Message::Remove("r1".into()));
            assert_eq!(retry, Action::send(Request::RemoveRule { id: "r1".into() }), "after the answer it works again");
        }

        /// The title is what a rule is about, the line under it what it does and where it holds.
        #[test]
        fn rules_read_as_subject_effect_and_place() {
            let lane = rule("l", SourceRef::agent("claude-code"), RuleEffect::Lane { lane: Lane::Info });
            assert_eq!(subject(&lane), trf("Everything from {source}", &[("source", &"Claude Code")]));
            assert_eq!(effect_words(&lane, now(), &[]), trf("Goes to “{lane}”", &[("lane", &tr("Info"))]));
            let sender = hidden("h", Some("news@shop.example"), None);
            assert_eq!(subject(&sender), trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
            assert_eq!(effect_words(&sender, now(), &[]), tr("Hidden"));
            assert_eq!(place(&sender).as_deref(), Some(tr("All mail accounts")));
            let muted = rule("m", SourceRef::script("backup"), RuleEffect::Muted);
            assert_eq!(effect_words(&muted, now(), &[]), tr("Muted"));
            assert_eq!(place(&muted), None, "the title names the source");
            let hits = RuleHits { rule: "m".into(), hits: 31, since: now() };
            let caught = trf("Hid {n} since {day}", &[("n", &31), ("day", &tr("today"))]);
            assert_eq!(rule_line(&muted, Some(&hits), now(), &[]), [tr("Muted").to_owned(), caught].join(" · "));
            assert_eq!(rule_line(&muted, None, now(), &[]), tr("Muted"), "a core before E8.8a tells nothing");
        }

        /// A muted trait reads with its words, its end and where its kind reaches, and comes after
        /// the other rules (tasks E8.6b, E8.12).
        #[test]
        fn muted_traits_read_with_effect_end_and_place() {
            let mail = SourceRef::mail("anna@example.org");
            let trait_of = |id: &str, key, value: &str, effect| Rule {
                facet: Some(Facet { key, value: value.into() }),
                ..rule(id, mail.clone(), effect)
            };
            let bulk = trait_of("b", FacetKey::Bulk, "bulk", RuleEffect::Lane { lane: Lane::Info });
            assert_eq!(subject(&bulk), tr("Newsletters and automatic mail"));
            assert_eq!(effect_words(&bulk, now(), &[]), tr("Only Info"));
            let tomorrow = now() + chrono::Duration::days(1);
            let project = Rule {
                until: Some(tomorrow),
                facet: Some(Facet { key: FacetKey::Project, value: "torro".into() }),
                ..rule("p", SourceRef::agent("claude-code"), RuleEffect::NoPopup)
            };
            let when = until_text(tomorrow, now());
            assert_eq!(effect_words(&project, now(), &[]), trf("No popups until {when}", &[("when", &when)]));
            assert_eq!(place(&project).as_deref(), Some(tr("All agents")));
            let rules = vec![
                bulk,
                rule("l", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }),
                hidden("h", None, Some("shop.example")),
            ];
            let ids: Vec<&str> = ordered(&rules).iter().map(|rule| rule.id.as_str()).collect();
            assert_eq!(ids, ["l", "h", "b"], "lanes, hidden senders, traits last");
        }

        /// What a rule caught, since when, in the words of the kind of rule.
        #[test]
        fn rules_say_what_they_caught_since_when() {
            let hits = |n, days| RuleHits { rule: "r".into(), hits: n, since: now() - chrono::Duration::days(days) };
            let muted = rule("r", SourceRef::script("backup"), RuleEffect::Muted);
            assert_eq!(
                caught(&muted, Some(&hits(12, 0)), now()),
                Some(trf("Hid {n} since {day}", &[("n", &12), ("day", &tr("today"))]))
            );
            let quiet = rule("r", SourceRef::script("backup"), RuleEffect::NoPopup);
            assert_eq!(
                caught(&quiet, Some(&hits(3, 1)), now()),
                Some(trf("Kept {n} quiet since {day}", &[("n", &3), ("day", &tr("yesterday"))]))
            );
            let lane = rule("r", SourceRef::script("backup"), RuleEffect::Lane { lane: Lane::Info });
            let three = since_text(now() - chrono::Duration::days(3), now());
            assert_eq!(
                three,
                datetime::weekday_name(datetime::to_local(now() - chrono::Duration::days(3)).date(), false)
            );
            assert_eq!(
                caught(&lane, Some(&hits(9, 3)), now()),
                Some(trf("Sent {n} since {day}", &[("n", &9), ("day", &three)]))
            );
            let old = since_text(now() - chrono::Duration::days(30), now());
            assert_eq!(
                caught(&muted, Some(&hits(0, 30)), now()),
                Some(trf("Nothing caught since {day}", &[("day", &old)]))
            );
            assert_eq!(caught(&muted, None, now()), None);
        }

        /// With and without rules the page builds; without any it says where rules come from.
        #[test]
        fn the_page_builds_with_and_without_rules() {
            let mut state = State::default();
            let rules = vec![
                rule("l", SourceRef::agent("codex"), RuleEffect::Lane { lane: Lane::ForMe }),
                rule("m", SourceRef::script("a-script-with-a-rather-long-name-that-is-cut"), RuleEffect::Muted),
                hidden("h", None, Some("shop.example")),
            ];
            let full = Data {
                rules: rules.clone(),
                rule_hits: vec![RuleHits { rule: "m".into(), hits: 4, since: now() }],
                ..Data::default()
            };
            let _: Element<'_, Message> = view(&state, &Data::default(), now());
            let _: Element<'_, Message> = view(&state, &full, now());
            let _: Action<()> = update(&mut state, Message::Remove("l".into()));
            let _: Element<'_, Message> = view(&state, &full, now());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::demo::Demo;
    use crate::{Backend, Host};

    /// Every section builds its page, with and without data (design skill: empty states too).
    #[tokio::test]
    async fn every_section_builds_with_and_without_data() {
        let backend: Arc<dyn Backend> = Arc::new(Demo::new());
        let data = crate::load(backend, wanted()).await;
        let state = State::default();
        for data in [&data, &Data::default()] {
            for section in SECTIONS {
                let page = page(&state, data, section.id);
                let _ = (page.body, page.sub_page, page.actions);
            }
            // An id no section has is Notifications, not an empty page.
            let _ = page(&state, data, "mail").body;
        }
    }

    /// The sections' ids and names, as the list and `--settings <section>` use them.
    #[test]
    fn the_sections_are_reachable_by_their_words() {
        let ids: Vec<&str> = SECTIONS.iter().map(|section| section.id).collect();
        assert_eq!(ids, ["notifications", "sources", "rules"]);
        assert_eq!(SECTIONS[0].name(), tr("Notifications"));
        let (mut host, _) = Host::new(Arc::new(Demo::new()), SECTIONS.to_vec(), wanted());
        for id in ids {
            assert!(host.open(id), "{id}");
        }
        assert!(!host.open("accounts"), "a section of the mail window is not one of these");
    }

    /// One load asks for what the three sections show, and for nothing else.
    #[test]
    fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(
            wanted(),
            vec![
                Request::Settings,
                Request::Accounts,
                Request::ListSources,
                Request::ListRules,
                Request::ListCategories { account: None }
            ]
        );
    }

    /// An answer that came back lets every "Remove" work again.
    #[test]
    fn an_answer_clears_what_was_on_its_way() {
        let mut state = State::default();
        let first: Action<()> =
            update(&mut state, Message::Rules(rules::Message::Remove("r".into())), &Data::default());
        assert_eq!(first, Action::send(Request::RemoveRule { id: "r".into() }));
        let again: Action<()> =
            update(&mut state, Message::Rules(rules::Message::Remove("r".into())), &Data::default());
        assert_eq!(again, Action::None);
        answered(&mut state);
        let retry: Action<()> =
            update(&mut state, Message::Rules(rules::Message::Remove("r".into())), &Data::default());
        assert_eq!(retry, Action::send(Request::RemoveRule { id: "r".into() }));
    }
}
