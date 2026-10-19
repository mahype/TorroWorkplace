//! The rules of the inbox as pure functions, shared by the core (E1.2), the demo data and the
//! apps, so all of them merge, place, wake and sort entries the same way.
//!
//! The security rules of `docs/tasks/eingang.md` that depend only on an entry live here too
//! ([`admitted_kind`], [`human_only`], [`may_open`]); the core enforces them (E1.2, E2.4).
//! Nothing here reads a clock: whoever calls passes `now`, so tests can fix it.
//!
//! Muting by traits (task E8.6a) lives here too: which traits an entry has ([`facets`]), what the
//! bell's menu offers ([`mute_options`]), the value a call mutes ([`mute_target`], always from the
//! stored entry) and which rules hold for an entry ([`muting`]).
//!
//! The remote sources of task E5 (GitHub, Nextcloud, Talk, ntfy) bring their traits through
//! [`remote_facets`] and get their lanes from [`github_place`], [`nextcloud_place`], [`talk_place`]
//! and [`ntfy_place`]; the core's source modules call them, so the rules can be tested without a
//! server.

use std::cmp::Ordering;

use chrono::{DateTime, TimeDelta, Utc};
use torro_core::i18n::{tr, trf};
use torro_mail::Order;

use crate::item::{
    AgentKind, Facet, FacetKey, HideScope, Item, Kind, Lane, LaneReason, Link, MuteEffect, MuteOption, MuteScope, Rule,
    RuleEffect, Sender, SourceKind, SourceRef, State,
};

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
/// entries urgent: waiting agents, approvals and alerts are for me and urgent, finished agents and
/// account problems for me; a notice goes where it was wanted ([`Lane::Info`] when nothing was
/// asked), a message too ([`Lane::ForMe`] when nothing was asked, the core asks [`mail_lane`] or
/// [`talk_place`]); neither is ever urgent.
pub fn placement(kind: Kind, wanted: Option<Lane>) -> (Lane, bool) {
    match kind {
        Kind::AgentWaiting | Kind::Approval | Kind::Alert => (Lane::ForMe, true),
        Kind::AgentDone | Kind::AccountProblem | Kind::Reminder => (Lane::ForMe, false),
        Kind::Notice => (wanted.unwrap_or(Lane::Info), false),
        Kind::Message => (wanted.unwrap_or(Lane::ForMe), false),
    }
}

/// The lane of a new mail (task E4.1, `docs/eingang.md`, "Sortieren"): a lane the person
/// remembered for its sender (`remembered`, from moving one of its mails with "remember") first,
/// since the person decided it; else [`Lane::Info`] for mail sent by machines or to lists
/// (`bulk`: `List-Id`, `Precedence: bulk`, `Auto-Submitted`; `List-Unsubscribe` alone is no sign,
/// issue #128), else [`Lane::ForMe`].
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
/// sender's address ([`mail_sender`]) or that address's domain ([`domain_of`]). `None` for
/// [`HideScope::Conversation`], which keeps no rule, for entries that are no mail and for mail
/// without a sender's address: there is nothing to hide by. An "address" that starts with `@`
/// names no sender: the core keeps a hidden domain as `@<domain>`, so such a sender's rule would be
/// taken for the domain's.
pub fn hide_target(item: &Item, scope: HideScope) -> Option<String> {
    let sender = mail_sender(item).filter(|sender| !sender.starts_with('@'))?;
    match scope {
        HideScope::Conversation => None,
        HideScope::Sender => Some(sender),
        HideScope::Domain => domain_of(&sender),
    }
}

/// Whether `item` can be hidden with `scope` (task E3.11): a conversation when it is mail (with or
/// without a sender's address), a sender or domain when there is one to keep a rule for
/// ([`hide_target`]). Everything else is no mail and is done with `inbox.set_item_done`.
pub fn may_hide(item: &Item, scope: HideScope) -> bool {
    match scope {
        HideScope::Conversation => item.source.kind == SourceKind::Mail,
        HideScope::Sender | HideScope::Domain => hide_target(item, scope).is_some(),
    }
}

/// Whether `rule` hides mail of one sender or domain (task E3.10): a muting rule of a mail account
/// with `sender` or `domain`. Such a rule holds for every mail account, not only the one it was
/// made in (task E3.11, [`reaches`]).
pub fn is_hiding(rule: &Rule) -> bool {
    rule.effect == RuleEffect::Muted
        && rule.facet.is_none()
        && rule.source.kind == SourceKind::Mail
        && (rule.sender.is_some() || rule.domain.is_some())
}

/// Whether `rule` is asked about new reports of `source`: every rule about `source` itself, and a
/// hiding rule ([`is_hiding`]) about any mail account when `source` is a mail account – a hidden
/// sender or domain is hidden in every account (task E3.11). Rules kept per account before E3.11
/// are read the same way, so they hold for every account from now on.
///
/// A rule with a trait (task E8.6a, [`Rule::facet`]) is asked about every source of its source's
/// kind – a sender muted from one mail account is muted in all, a project of one agent for every
/// agent –, one for a conversation only about its own source, where its thread key lives.
pub fn reaches(rule: &Rule, source: &SourceRef) -> bool {
    match &rule.facet {
        Some(facet) if facet.key == FacetKey::Conversation => rule.source == *source,
        Some(_) => rule.source.kind == source.kind,
        None => rule.source == *source || (is_hiding(rule) && source.kind == SourceKind::Mail),
    }
}

/// Whether `rule` drops a new report from `sender` (its [`sender_key`], `None` without one) of a
/// source it [`reaches`]; the caller passes only such rules. Only muting rules drop: one without
/// `sender` and `domain` everything, one with `sender` only that address, one with `domain` only
/// addresses of exactly that domain (task E3.10). A report without a sender's address is dropped
/// only by a rule for the whole source. A rule with a trait (task E8.6a) drops nothing here: it is
/// asked with the whole entry ([`muting`]).
pub fn drops(rule: &Rule, sender: Option<&str>) -> bool {
    if rule.effect != RuleEffect::Muted || rule.facet.is_some() {
        return false;
    }
    match (&rule.sender, &rule.domain) {
        (None, None) => true,
        (Some(hidden), _) => sender == Some(hidden.as_str()),
        (None, Some(hidden)) => sender.and_then(domain_of).as_deref() == Some(hidden.as_str()),
    }
}

/// Whether `rule` hides `item`: a hiding rule ([`is_hiding`]) that would drop the entry's mail
/// ([`drops`]), in whichever mail account the entry is (task E3.11). The entries it hides are done
/// when the rule is made; a rule for a whole source hides none, muting leaves the entries already
/// there.
pub fn hides(rule: &Rule, item: &Item) -> bool {
    is_hiding(rule) && reaches(rule, &item.source) && drops(rule, mail_sender(item).as_deref())
}

/// The longest value of a trait, in bytes; longer ones are cut at a character boundary, so the same
/// text always gives the same value.
pub const FACET_MAX: usize = 200;

/// The traits of `item` its source reports reliably (task E8.6a, table in `docs/eingang.md`), in
/// the order of [`FacetKey`], for [`Item::facets`]. The core calls it when a report arrives; `list`
/// (`List-Id`) and `bulk` come from a mail's headers and count only for mail.
///
/// - Mail: the account, the sender's address and its domain (when there is an address), the list
///   (in small letters) and whether it is bulk mail.
/// - Agents: the agent, the project a hook reported, and whether it waits or finished.
/// - Scripts: the client. (A topic would need a field in `inbox.post_item` first.)
/// - The person's own entries ([`human_only`]) have none: no rule may touch them.
/// - The remote sources of task E5 have none here: the core files them with the traits it read
///   from the server, through [`remote_facets`].
///
/// Values are trimmed and cut to [`FACET_MAX`] bytes; empty ones are left out.
pub fn facets(item: &Item, list: Option<&str>, bulk: bool) -> Vec<Facet> {
    let mut facets = Vec::new();
    if human_only(item) {
        return facets;
    }
    let mut add = |key: FacetKey, value: &str| {
        let value = cut(value.trim(), FACET_MAX);
        if !value.is_empty() {
            facets.push(Facet { key, value });
        }
    };
    match item.source.kind {
        SourceKind::Suite | SourceKind::Github | SourceKind::Nextcloud | SourceKind::Talk | SourceKind::Ntfy => {}
        SourceKind::Mail => {
            add(FacetKey::Account, &item.source.name);
            if let Some(sender) = mail_sender(item).filter(|sender| !sender.starts_with('@')) {
                add(FacetKey::Sender, &sender);
                if let Some(domain) = domain_of(&sender) {
                    add(FacetKey::Domain, &domain);
                }
            }
            if let Some(list) = list {
                add(FacetKey::List, &list.to_lowercase());
            }
            if bulk {
                add(FacetKey::Bulk, "bulk");
            }
        }
        SourceKind::Agent => {
            add(FacetKey::Agent, &item.source.name);
            if let Some(project) = item.agent.as_ref().and_then(|agent| agent.project.as_deref()) {
                add(FacetKey::Project, project);
            }
            match item.kind {
                Kind::AgentWaiting => add(FacetKey::Kind, "agent_waiting"),
                Kind::AgentDone => add(FacetKey::Kind, "agent_done"),
                Kind::Notice | Kind::Approval | Kind::AccountProblem | Kind::Message | Kind::Alert | Kind::Reminder => {
                }
            }
        }
        SourceKind::Script => add(FacetKey::Client, &item.source.name),
    }
    facets
}

/// The most tags ([`FacetKey::Tag`]) one entry keeps; ntfy sends at most this many, too.
const TAGS_MAX: usize = 10;

/// The traits a remote source of `kind` may bring (task E5.1, table in `docs/tasks/eingang.md`), in
/// the order of [`FacetKey`]:
///
/// | Source | Traits | foreign |
/// |---|---|---|
/// | GitHub | `kind` (what the thread is about), `repo`, `reason`, `result` (workflow runs only) | `repo` |
/// | Nextcloud | `app`, `person` | both |
/// | Talk | `kind` (`direct`, `mention`, `group`), `person` | `person` |
/// | ntfy | `topic` (its label in the account), `priority` (`1`–`5`), `tag` (several) | `tag` |
///
/// Empty for the suite, agents, scripts and mail, which take theirs from [`facets`].
pub fn allowed_facets(kind: SourceKind) -> &'static [FacetKey] {
    match kind {
        SourceKind::Github => &[FacetKey::Kind, FacetKey::Repo, FacetKey::Reason, FacetKey::Result],
        SourceKind::Nextcloud => &[FacetKey::App, FacetKey::Person],
        SourceKind::Talk => &[FacetKey::Kind, FacetKey::Person],
        SourceKind::Ntfy => &[FacetKey::Topic, FacetKey::Priority, FacetKey::Tag],
        SourceKind::Suite | SourceKind::Agent | SourceKind::Script | SourceKind::Mail => &[],
    }
}

/// The traits the core files with an entry of a remote source of `kind` (task E5.1), from those
/// its source module read from the server – so traits keep coming from the core alone, whatever the
/// server sends:
///
/// - only the keys of [`allowed_facets`] for `kind`; any other is dropped;
/// - values trimmed and cut to [`FACET_MAX`] bytes; empty ones dropped;
/// - keys that are the suite's own words ([`FacetKey::is_foreign`] `false`) only with such words:
///   `priority` `1`–`5`, `result` `failed`, `succeeded` or `cancelled`, Talk's `kind` `direct`,
///   `mention` or `group`, GitHub's `kind` and `reason` in small letters, digits and `_`; the
///   topic's label is the person's own and taken as it is;
/// - at most one per key, the first; only [`FacetKey::Tag`] several, each value once and at most
///   ten, in the order given;
/// - sorted by key in the order of [`FacetKey`] ([`Item::facets`]).
pub fn remote_facets(kind: SourceKind, facets: Vec<Facet>) -> Vec<Facet> {
    let allowed = allowed_facets(kind);
    let mut kept: Vec<Facet> = Vec::new();
    for facet in facets {
        let value = cut(facet.value.trim(), FACET_MAX);
        if !allowed.contains(&facet.key) || value.is_empty() || !admits(kind, facet.key, &value) {
            continue;
        }
        let mut same = kept.iter().filter(|known| known.key == facet.key);
        let room = if facet.key == FacetKey::Tag {
            same.clone().count() < TAGS_MAX && !same.any(|known| known.value == value)
        } else {
            same.next().is_none()
        };
        if room {
            kept.push(Facet { key: facet.key, value });
        }
    }
    // Stable, so several tags keep the order they were sent in.
    kept.sort_by_key(|facet| facet.key);
    kept
}

/// Whether `value` may be the trait `key` of a remote source of `kind` ([`remote_facets`]): the
/// suite's own words only from the few words the core writes for them.
fn admits(kind: SourceKind, key: FacetKey, value: &str) -> bool {
    let word = || value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    match key {
        FacetKey::Priority => matches!(value, "1" | "2" | "3" | "4" | "5"),
        FacetKey::Result => matches!(value, "failed" | "succeeded" | "cancelled"),
        FacetKey::Kind if kind == SourceKind::Talk => matches!(value, "direct" | "mention" | "group"),
        FacetKey::Kind | FacetKey::Reason => word(),
        _ => true,
    }
}

/// The lane of a GitHub notification (task E5.1) from its trait values: GitHub's `reason`
/// (`review_requested`, …), what it is about (`kind`, `check_suite` for a workflow run) and how a
/// workflow run ended (`result`, `failed` …, `None` when the title does not say).
///
/// For me when the person is asked or named – `review_requested`, `mention`, `team_mention`,
/// `assign`, `approval_requested`, `security_alert`, `invitation` – or it is their own thread
/// (`author`: a reply to what they opened is meant for them), and a workflow run of theirs that
/// failed (`ci_activity`, `check_suite`, `failed`). "Info" for the rest: `comment` (their own
/// comment only subscribed them), `state_change`, `subscribed` (watching a repository), `manual`,
/// runs that passed or were cancelled and every reason the suite does not know. Never urgent
/// (question 3 of E5: a run does not reliably point to its pull request); the entry is a
/// [`Kind::Notice`] in this lane.
pub fn github_place(reason: &str, kind: &str, result: Option<&str>) -> Lane {
    match reason {
        "review_requested" | "mention" | "team_mention" | "assign" | "approval_requested" | "security_alert"
        | "invitation" | "author" => Lane::ForMe,
        "ci_activity" if kind == "check_suite" && result == Some("failed") => Lane::ForMe,
        _ => Lane::Info,
    }
}

/// The lane of a Nextcloud notification (task E5.1) from the app that sent it (`app`, its id) and
/// what it is about (`object_type`).
///
/// For me when someone turns to the person: `files_sharing` (a file or folder shared with them),
/// `comments` on a file (`object_type` `files`: the comments app notifies only about mentions),
/// `spreed` (a call or a chat, while Talk is no source of its own), `deck` (a card assigned or
/// due), `dav` (a reminder of an event) and every `twofactor_*` app (a sign-in to confirm). "Info"
/// for `updatenotification`, `announcementcenter`, `firstrunwizard`, `activity` and every app the
/// suite does not know. The entry is a [`Kind::Notice`] in this lane.
pub fn nextcloud_place(app: &str, object_type: &str) -> Lane {
    match app {
        "files_sharing" | "spreed" | "deck" | "dav" => Lane::ForMe,
        "comments" if object_type == "files" => Lane::ForMe,
        _ if app.starts_with("twofactor_") => Lane::ForMe,
        _ => Lane::Info,
    }
}

/// The kind of a Talk conversation, for [`talk_place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TalkRoom {
    /// One to one (Talk's type 1).
    Direct,
    /// Any conversation of more people: group, public, and a former one-to-one.
    Group,
}

impl TalkRoom {
    /// The value of the trait [`FacetKey::Kind`] of a Talk entry: `direct`, `mention` for a group
    /// conversation that mentions the person (`mention`), else `group`.
    pub fn facet(self, mention: bool) -> &'static str {
        match self {
            TalkRoom::Direct => "direct",
            TalkRoom::Group if mention => "mention",
            TalkRoom::Group => "group",
        }
    }
}

/// When Talk notifies the person about a conversation: its notification level, which they set per
/// conversation in Talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TalkLevel {
    /// The server's default: one-to-one always, other conversations on mentions only.
    Default,
    /// Every message.
    Always,
    /// Only messages that mention the person.
    Mentions,
    /// Never.
    Never,
}

impl TalkLevel {
    /// The level Talk's `notificationLevel` names: 1 always, 2 mentions, 3 never, 0 and any other
    /// number the default.
    pub fn from_talk(level: i64) -> TalkLevel {
        match level {
            1 => TalkLevel::Always,
            2 => TalkLevel::Mentions,
            3 => TalkLevel::Never,
            _ => TalkLevel::Default,
        }
    }
}

/// The lane of the entry for an unread Talk conversation of `room` (task E5.1), as far as the
/// person's notification `level` for it asks for one; `mention` says that an unread message
/// mentions them. `None`: no entry. The entry is a [`Kind::Message`] in this lane.
///
/// The person decided in Talk how much a conversation may disturb them, so the inbox follows it
/// (question 7 of E5): never – no entry; mentions only – only with a mention, for me; always – for
/// me in a one-to-one or with a mention, else in "Info"; the default – a one-to-one always, any
/// other conversation only with a mention, for me.
pub fn talk_place(room: TalkRoom, mention: bool, level: TalkLevel) -> Option<Lane> {
    match (level, room) {
        (TalkLevel::Never, _) => None,
        (TalkLevel::Mentions, _) => mention.then_some(Lane::ForMe),
        (TalkLevel::Always | TalkLevel::Default, TalkRoom::Direct) => Some(Lane::ForMe),
        (TalkLevel::Always, TalkRoom::Group) => Some(if mention { Lane::ForMe } else { Lane::Info }),
        (TalkLevel::Default, TalkRoom::Group) => mention.then_some(Lane::ForMe),
    }
}

/// Kind and lane of the entry for an ntfy message of `priority` (task E5.1, ntfy's 1 to 5, 3 when
/// the sender gave none): 5 is an [`Kind::Alert`] (for me and urgent, even during quiet times –
/// question 9 of E5, only in topics the person subscribed to), 4 a [`Kind::Notice`] for me, 1 to 3
/// and any number ntfy does not have a notice in "Info".
pub fn ntfy_place(priority: u8) -> (Kind, Lane) {
    match priority {
        5 => (Kind::Alert, Lane::ForMe),
        4 => (Kind::Notice, Lane::ForMe),
        _ => (Kind::Notice, Lane::Info),
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The trait `inbox.mute_item` with `scope` mutes of `item`: for the conversation its thread key,
/// for a trait the value **the stored entry** has – never one from the request, so nobody can mute
/// what the entry does not say –, the `nth` of its traits with that key (counted from 0; several
/// only for [`FacetKey::Tag`]). `None` when the entry has no such trait (also for `facet` with
/// [`FacetKey::Conversation`], which is never among [`Item::facets`], and for an `nth` beyond its
/// traits) and for the person's own entries ([`human_only`]).
pub fn mute_target(item: &Item, scope: MuteScope) -> Option<Facet> {
    if human_only(item) {
        return None;
    }
    match scope {
        MuteScope::Conversation => Some(Facet { key: FacetKey::Conversation, value: item.thread_key.clone() }),
        MuteScope::Facet { key, nth } => {
            let nth = usize::try_from(nth).ok()?;
            item.facets.iter().filter(|facet| facet.key == key).nth(nth).cloned()
        }
    }
}

/// The name to show for the Nextcloud app `app` (its id, [`FacetKey::App`]): "Files" for
/// `files` and `files_sharing`, "Talk" (a product name) for `spreed`, else the id itself, which
/// is foreign text.
pub fn app_label(app: &str) -> String {
    match app {
        "files" | "files_sharing" => tr("Files").into(),
        "spreed" => "Talk".into(),
        _ => app.into(),
    }
}

/// The words for muting `facet`, in the user's language: "Mail from news@shop.example", "This
/// conversation", "Review requests", "The tag backup". For the bell's menu, the rules page and the
/// summary of an approval. The value is shown as it is: foreign text for [`FacetKey::is_foreign`]
/// keys, which the caller marks as such. A word of the suite's own the suite has no words for
/// (a GitHub reason such as `subscribed`) is shown as it is, too.
pub fn facet_label(facet: &Facet) -> String {
    let value = facet.value.as_str();
    match facet.key {
        FacetKey::Account => trf("All mail to {account}", &[("account", &value)]),
        FacetKey::Sender => trf("Mail from {sender}", &[("sender", &value)]),
        FacetKey::Domain => trf("Everything from {domain}", &[("domain", &value)]),
        FacetKey::List => trf("The list {list}", &[("list", &value)]),
        FacetKey::Bulk => tr("Newsletters and automatic mail").into(),
        FacetKey::Agent => {
            let name: &str = match AgentKind::from_id(value) {
                Some(agent) => agent.label(),
                None => value,
            };
            trf("Everything from {source}", &[("source", &name)])
        }
        FacetKey::Project => trf("The project {project}", &[("project", &value)]),
        FacetKey::Kind => kind_label(value),
        FacetKey::Client => trf("Everything from {source}", &[("source", &value)]),
        FacetKey::Conversation => tr("This conversation").into(),
        FacetKey::Repo => trf("Everything from {repo}", &[("repo", &value)]),
        FacetKey::Reason => match value {
            "review_requested" => tr("Review requests").into(),
            "mention" | "team_mention" => tr("Mentions").into(),
            _ => value.to_string(),
        },
        FacetKey::Result => match value {
            "failed" => tr("Failed checks").into(),
            "succeeded" => tr("Successful checks").into(),
            "cancelled" => tr("Cancelled checks").into(),
            _ => value.to_string(),
        },
        FacetKey::App => trf("Everything from {source}", &[("source", &app_label(value))]),
        FacetKey::Person => trf("Messages from {person}", &[("person", &value)]),
        FacetKey::Topic => trf("The topic {topic}", &[("topic", &value)]),
        FacetKey::Priority => trf("Priority {n}", &[("n", &value)]),
        FacetKey::Tag => trf("The tag {tag}", &[("tag", &value)]),
    }
}

/// The words for muting by [`FacetKey::Kind`]: the agents' states, Talk's kinds of conversation
/// and what GitHub threads are about; any other value as it is.
fn kind_label(value: &str) -> String {
    match value {
        "agent_waiting" => tr("Agents waiting for you").into(),
        "agent_done" => tr("Agents that finished").into(),
        "direct" => tr("Direct messages").into(),
        "mention" => tr("Mentions").into(),
        "group" => tr("Group conversations").into(),
        "pull_request" => tr("Pull requests").into(),
        "issue" => tr("Issues").into(),
        "check_suite" => tr("Checks").into(),
        _ => value.to_string(),
    }
}

/// The ways to mute `item` for the bell's menu (`inbox.list_mute_options`, task E8.6a): its
/// conversation first, then each of its traits in the order of [`FacetKey`] – every tag on its own,
/// with its place among them as `nth` (task E5.1) –, each with its words ([`facet_label`]), whether
/// they hold foreign text and whether every choice needs admin access. Empty for the person's own
/// entries ([`human_only`], security rule 1).
pub fn mute_options(item: &Item) -> Vec<MuteOption> {
    if human_only(item) {
        return Vec::new();
    }
    let conversation = MuteOption {
        scope: MuteScope::Conversation,
        value: None,
        label: tr("This conversation").into(),
        untrusted: false,
        admin: false,
    };
    let traits =
        item.facets.iter().enumerate().filter(|(_, facet)| facet.key != FacetKey::Conversation).map(|(at, facet)| {
            let before = item.facets[..at].iter().filter(|other| other.key == facet.key).count();
            MuteOption {
                scope: MuteScope::Facet { key: facet.key, nth: u32::try_from(before).unwrap_or(u32::MAX) },
                value: Some(facet.value.clone()),
                label: facet_label(facet),
                untrusted: facet.key.is_foreign(),
                admin: true,
            }
        });
    std::iter::once(conversation).chain(traits).collect()
}

/// The start of every mail message's id in the core's mail store (`m:…`). No inbox entry's id
/// starts so, which is how `inbox.list_mute_options` and `inbox.mute_item` tell a message from an
/// entry (issue #123, [`is_message_id`]).
pub const MESSAGE_ID_PREFIX: &str = "m:";

/// `id` names a mail message (`m:…`) rather than an inbox entry (issue #123): muting may name a
/// folder's message that never became an entry.
pub fn is_message_id(id: &str) -> bool {
    id.starts_with(MESSAGE_ID_PREFIX)
}

/// The entry a mail message would be, never stored (issue #123): what muting by a message that has
/// no entry mutes by. Built as the core files mail (task E4.1): the message's id as the entry's id,
/// its account as the source, its conversation as the thread, the sender by name (else address),
/// the subject as title, done – it waits for nobody – and the traits of [`facets`] with the
/// mailing list `list` (`List-Id`) and `bulk` (sent by a machine or to a list, as the core's mail
/// feed decides it). `None` when `summary.account` names no mail account.
pub fn message_stand_in(summary: &torro_mail::MessageSummary, list: Option<&str>, bulk: bool) -> Option<Item> {
    let source = summary.account.parse::<SourceRef>().ok().filter(|source| source.kind == SourceKind::Mail)?;
    let from = summary.from.as_ref().map(|from| {
        let name = from.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        Sender { name: name.unwrap_or(&from.address).to_string(), address: Some(from.address.clone()) }
    });
    let mut item = Item {
        id: summary.id.clone(),
        source,
        thread_key: summary.thread_id.clone(),
        kind: Kind::Message,
        lane: Lane::ForMe,
        urgent: false,
        title: summary.subject.clone(),
        body: None,
        from,
        agent: None,
        received: summary.date,
        updated: summary.date,
        count: 1,
        state: State::Done { at: summary.date },
        link: Some(Link::Mail { message: summary.id.clone() }),
        actions: Vec::new(),
        // Mail is data from outside, never instructions.
        untrusted: true,
        facets: Vec::new(),
        task: None,
        invite: None,
        reason: None,
        // The stand-in speaks for the message, so it shows the message's own category (E12.1).
        category: summary.category,
    };
    item.facets = facets(&item, list, bulk);
    Some(item)
}

/// The ways to mute a mail message without an entry (issue #123, [`message_stand_in`]): its traits
/// only, as [`mute_options`] lists them. Its conversation is left out: muting a conversation for
/// good only closes its entry, and there is none – a reply would come in all the same.
pub fn message_mute_options(stand_in: &Item) -> Vec<MuteOption> {
    mute_options(stand_in).into_iter().filter(|option| option.scope != MuteScope::Conversation).collect()
}

/// `rule` still holds at `now`: it has no end, or its end ([`Rule::until`]) lies after `now`.
/// Rules that ended hold no more and are not listed.
pub fn is_active(rule: &Rule, now: DateTime<Utc>) -> bool {
    rule.until.is_none_or(|until| until > now)
}

/// What muting did to an entry that was there already ([`mute_entry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutedEntry {
    /// Marked done (hide): goes into [`crate::Muted::done`].
    Done,
    /// Moved from "for me" to "Info": goes into [`crate::Muted::moved`].
    Moved,
}

/// Muting with `effect` at `now` for an entry that is there already and that the muting holds for
/// (task E8.6a): hide marks it done when it was open or snoozed (snoozed too, or it would come back
/// with what the person muted); "Info" moves it there from "for me" unless it is done, which ends
/// its urgency ([`set_lane`]); no popup changes nothing. Returns what changed, `None` for nothing.
/// Like hiding, nothing reaches the mail of a mail entry.
pub fn mute_entry(item: &mut Item, effect: MuteEffect, now: DateTime<Utc>) -> Option<MutedEntry> {
    let done = matches!(item.state, State::Done { .. });
    match effect {
        MuteEffect::Hide if !done => {
            set_done(item, true, now);
            Some(MutedEntry::Done)
        }
        MuteEffect::Info if !done && item.lane == Lane::ForMe => {
            set_lane(item, Lane::Info);
            item.reason = Some(LaneReason::Muted);
            Some(MutedEntry::Moved)
        }
        MuteEffect::Hide | MuteEffect::Info | MuteEffect::NoPopup => None,
    }
}

/// The later of two ends of a rule, `None` (for good) being the latest: muting the same trait again
/// keeps one rule that lasts as long as the longer of both asked for.
pub fn later_end(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    a.zip(b).map(|(a, b)| a.max(b))
}

/// Whether the rule with a trait `rule` (task E8.6a) holds for `item` at `now`: it is active
/// ([`is_active`]), [`reaches`] the entry's source and the entry has the rule's trait – for the
/// conversation its thread key. Never for the person's own entries ([`human_only`], security rule
/// 1), never for rules without a trait (those ask [`drops`] and [`hides`]).
pub fn mutes(rule: &Rule, item: &Item, now: DateTime<Utc>) -> bool {
    let Some(facet) = &rule.facet else { return false };
    if !is_active(rule, now) || human_only(item) || !reaches(rule, &item.source) {
        return false;
    }
    match facet.key {
        FacetKey::Conversation => item.thread_key == facet.value,
        _ => item.facets.contains(facet),
    }
}

/// The strongest muting among the rules in `rules` that hold for `item` at `now` ([`mutes`]): hide
/// before "Info" before no popup; `None` when none holds. The core asks it for every report (hide
/// drops it, "Info" moves it there) and before every popup (no popup).
pub fn muting(rules: &[Rule], item: &Item, now: DateTime<Utc>) -> Option<MuteEffect> {
    rules.iter().filter(|rule| mutes(rule, item, now)).filter_map(|rule| MuteEffect::of(rule.effect)).max()
}

/// The kind an entry posted as `kind` by `poster` gets; `None` when the post is refused
/// (`forbidden`). Security rule 3: approvals and account problems come only from the core,
/// agent states only from hooks (or the core) and turn into a [`Kind::Notice`] from anyone else;
/// messages (mail, chat) and alerts (ntfy, task E5.1) only from the core, a [`Kind::Notice`] from
/// anyone else, so no client or hook makes its entries urgent.
pub fn admitted_kind(kind: Kind, poster: Poster) -> Option<Kind> {
    match (kind, poster) {
        (Kind::Approval | Kind::AccountProblem | Kind::Reminder, Poster::Core) => Some(kind),
        (Kind::Approval | Kind::AccountProblem | Kind::Reminder, _) => None,
        (Kind::AgentWaiting | Kind::AgentDone, Poster::Core | Poster::Hook) => Some(kind),
        (Kind::AgentWaiting | Kind::AgentDone, Poster::Client) => Some(Kind::Notice),
        (Kind::Message | Kind::Alert, Poster::Core) => Some(kind),
        (Kind::Message | Kind::Alert | Kind::Notice, _) => Some(Kind::Notice),
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

/// [`merge`] for a new mail of the conversation `existing` holds (task E9.1, decision 8 of
/// `docs/tasks/eingang.md`, E9): as [`merge`], but a snoozed conversation opens again
/// ([`State::New`]), as a reply brings a conversation back from "Later" in mail programs. Other
/// sources keep [`merge`]: a report again of a matter the person put off stays put off.
pub fn merge_mail(existing: Item, incoming: Item) -> Item {
    let mut merged = merge(existing, incoming);
    if matches!(merged.state, State::Snoozed { .. }) {
        merged.state = State::New;
    }
    merged
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
/// ([`placement`]): an entry the user moved to "Info" does not interrupt any more. The reason it
/// stood where it was ([`Item::reason`]) no longer holds and is cleared; a caller moving it by a
/// rule sets the rule's reason after.
pub fn set_lane(item: &mut Item, lane: Lane) {
    item.lane = lane;
    item.urgent = lane == Lane::ForMe && placement(item.kind, None).1;
    item.reason = None;
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
/// every app shows the same order. [`order_in`] with [`Order::NewestFirst`].
pub fn order(a: &Item, b: &Item) -> Ordering {
    order_in(a, b, Order::NewestFirst)
}

/// The order of the list in `order` (task E8.15): urgent entries first either way – what waits
/// stays on top –, then by `updated` newest or oldest first; ties by id ascending in both, so
/// every app shows the same order.
pub fn order_in(a: &Item, b: &Item, order: Order) -> Ordering {
    b.urgent.cmp(&a.urgent).then_with(|| order.apply(b.updated.cmp(&a.updated))).then_with(|| a.id.cmp(&b.id))
}

/// Sorts `items` by [`order`].
pub fn sort(items: &mut [Item]) {
    sort_in(items, Order::NewestFirst);
}

/// Sorts `items` by [`order_in`] in `order`.
pub fn sort_in(items: &mut [Item], order: Order) {
    items.sort_by(|a, b| order_in(a, b, order));
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::item::{Action, AgentInfo, AgentKind};

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
            facets: Vec::new(),
            task: None,
            invite: None,
            reason: None,
            category: None,
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
            facet: None,
            until: None,
        }
    }

    fn facet(key: FacetKey, value: &str) -> Facet {
        Facet { key, value: value.into() }
    }

    /// A rule of `inbox.mute_item` about `source`, muting `facet` with `effect` until `until`.
    fn muting_rule(source: SourceRef, facet: Facet, effect: MuteEffect, until: Option<DateTime<Utc>>) -> Rule {
        Rule {
            id: "f".into(),
            source,
            sender: None,
            domain: None,
            effect: effect.rule_effect(),
            created: at(8),
            facet: Some(facet),
            until,
        }
    }

    /// A newsletter of the list `<news.shop.example>` in `account`, with its traits.
    fn newsletter(account: &str) -> Item {
        let mail = Item { source: SourceRef::mail(account), ..mail_from(Some("News@Shop.Example")) };
        Item { facets: facets(&mail, Some(" <News.Shop.Example> "), true), ..mail }
    }

    /// A hook's entry of Claude Code waiting in the project `project`, with its traits.
    fn waiting_agent(project: &str) -> Item {
        let agent = AgentInfo {
            agent: AgentKind::ClaudeCode,
            session: "s1".into(),
            project: Some(project.into()),
            branch: None,
            pids: Vec::new(),
            title: None,
        };
        let entry =
            Item { source: SourceRef::agent("claude-code"), agent: Some(agent), ..item("a", Kind::AgentWaiting) };
        Item { facets: facets(&entry, None, false), ..entry }
    }

    #[test]
    fn each_source_brings_the_traits_it_reports_reliably() {
        let keys = |item: &Item| item.facets.iter().map(|facet| (facet.key, facet.value.clone())).collect::<Vec<_>>();
        assert_eq!(
            keys(&newsletter("anna@example.org")),
            [
                (FacetKey::Account, "anna@example.org".into()),
                (FacetKey::Sender, "news@shop.example".into()),
                (FacetKey::Domain, "shop.example".into()),
                (FacetKey::List, "<news.shop.example>".into()),
                (FacetKey::Bulk, "bulk".into()),
            ]
        );
        assert_eq!(
            keys(&waiting_agent("TorroWorkplace")),
            [
                (FacetKey::Agent, "claude-code".into()),
                (FacetKey::Project, "TorroWorkplace".into()),
                (FacetKey::Kind, "agent_waiting".into()),
            ]
        );
        assert_eq!(facets(&item("s", Kind::Notice), None, true), [facet(FacetKey::Client, "backup")], "no mail");
        let without_address = mail_from(None);
        assert_eq!(keys(&Item { facets: facets(&without_address, None, false), ..without_address }).len(), 1);
        let approval = Item { source: SourceRef::suite(), ..item("p", Kind::Approval) };
        assert!(facets(&approval, None, false).is_empty(), "the person's own entries have none");
        let long = "x".repeat(300);
        assert_eq!(facets(&item("l", Kind::Notice), Some(&long), true).len(), 1);
        let listed = facets(&mail_from(Some("a@b.example")), Some(&long), false);
        assert_eq!(listed.last().map(|facet| facet.value.len()), Some(FACET_MAX), "cut to {FACET_MAX} bytes");
    }

    #[test]
    fn the_value_to_mute_comes_from_the_stored_entry() {
        let mail = newsletter("anna@example.org");
        assert_eq!(
            mute_target(&mail, MuteScope::Facet { key: FacetKey::Sender, nth: 0 }),
            Some(facet(FacetKey::Sender, "news@shop.example"))
        );
        assert_eq!(mute_target(&mail, MuteScope::Facet { key: FacetKey::Project, nth: 0 }), None, "a trait it lacks");
        assert_eq!(mute_target(&mail, MuteScope::Facet { key: FacetKey::Conversation, nth: 0 }), None);
        assert_eq!(mute_target(&mail, MuteScope::Conversation), Some(facet(FacetKey::Conversation, "thread-m")));
        let approval = Item { source: SourceRef::suite(), ..item("p", Kind::Approval) };
        assert_eq!(mute_target(&approval, MuteScope::Conversation), None, "never the person's own");
    }

    #[test]
    fn the_menu_offers_the_conversation_and_each_trait() {
        let options = mute_options(&newsletter("anna@example.org"));
        let scopes: Vec<MuteScope> = options.iter().map(|option| option.scope).collect();
        assert_eq!(scopes[0], MuteScope::Conversation);
        assert_eq!(scopes.len(), 6);
        assert_eq!(options[0].label, tr("This conversation"));
        assert!(!options[0].admin && options[1..].iter().all(|option| option.admin), "only traits are always rules");
        let sender = &options[2];
        assert_eq!(sender.label, trf("Mail from {sender}", &[("sender", &"news@shop.example")]));
        assert!(sender.untrusted, "a sender is foreign text");
        assert!(!options[1].untrusted && !options[5].untrusted, "the account and bulk are not");
        let agent = mute_options(&waiting_agent("TorroWorkplace"));
        assert_eq!(agent[1].label, trf("Everything from {source}", &[("source", &"Claude Code")]));
        assert_eq!(agent[3].label, tr("Agents waiting for you"));
        let approval = Item { source: SourceRef::suite(), ..item("p", Kind::Approval) };
        assert!(mute_options(&approval).is_empty(), "approvals are never muted");
    }

    /// A message of anna@example.org from the shop's newsletter, as the mail store lists it.
    fn shop_message(id: &str, account: &str) -> torro_mail::MessageSummary {
        torro_mail::MessageSummary {
            id: id.into(),
            account: account.into(),
            folder: "f:archive".into(),
            thread_id: "t:sale".into(),
            from: Some(torro_mail::MailAddress { name: Some(" ".into()), address: "News@Shop.Example".into() }),
            to: Vec::new(),
            subject: "Winter sale".into(),
            date: at(7),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            invite: None,
            back: None,
            category: None,
            untrusted: true,
        }
    }

    #[test]
    fn a_message_without_an_entry_is_muted_by_its_own_traits() {
        assert!(is_message_id("m:1") && !is_message_id("0a1b2c3d4e5f6a7b") && !is_message_id("demo-1"));
        let message = shop_message("m:7", "mail:anna@example.org");
        let stand_in = message_stand_in(&message, Some(" <News.Shop.Example> "), true).expect("a mail account");
        assert_eq!((stand_in.id.as_str(), stand_in.thread_key.as_str()), ("m:7", "t:sale"));
        assert_eq!(stand_in.from.as_ref().map(|from| from.name.as_str()), Some("News@Shop.Example"), "no blank name");
        assert!(!stand_in.state.is_open(), "it waits for nobody");
        assert_eq!(stand_in.facets, newsletter("anna@example.org").facets, "the traits an entry would have");
        let options = message_mute_options(&stand_in);
        assert!(options.iter().all(|option| option.scope != MuteScope::Conversation), "no entry to close");
        assert_eq!(options.len(), mute_options(&stand_in).len() - 1);
        assert_eq!(
            mute_target(&stand_in, MuteScope::Facet { key: FacetKey::Domain, nth: 0 }),
            Some(facet(FacetKey::Domain, "shop.example"))
        );
        assert_eq!(message_stand_in(&shop_message("m:8", "script:backup"), None, false), None, "no mail account");
        assert_eq!(message_stand_in(&shop_message("m:9", "nonsense"), None, false), None);
    }

    #[test]
    fn a_trait_holds_for_every_source_of_its_kind_until_it_ends() {
        let sender = facet(FacetKey::Sender, "news@shop.example");
        let rule = muting_rule(SourceRef::mail("anna@example.org"), sender, MuteEffect::Hide, Some(at(12)));
        let work = newsletter("anna@work.example.org");
        assert!(mutes(&rule, &work, at(11)), "every mail account");
        assert!(!mutes(&rule, &work, at(12)), "ended");
        let other = Item { facets: facets(&mail_from(Some("b@shop.example")), None, false), ..mail_from(None) };
        assert!(!mutes(&rule, &other, at(9)), "another sender");
        let project = facet(FacetKey::Project, "TorroWorkplace");
        let agents = muting_rule(SourceRef::agent("codex"), project, MuteEffect::Info, None);
        assert!(mutes(&agents, &waiting_agent("TorroWorkplace"), at(9)), "every agent");
        assert!(!mutes(&agents, &waiting_agent("website"), at(9)));
        assert!(!mutes(&agents, &newsletter("anna@example.org"), at(9)), "no other kind of source");
        // Never the person's own, whatever the rule says.
        let suite = Item {
            source: SourceRef::suite(),
            facets: vec![facet(FacetKey::Client, "backup")],
            task: None,
            ..item("p", Kind::Approval)
        };
        let client = muting_rule(SourceRef::suite(), facet(FacetKey::Client, "backup"), MuteEffect::Hide, None);
        assert!(!mutes(&client, &suite, at(9)));
    }

    #[test]
    fn muting_hides_or_moves_what_is_there_already() {
        let mut waiting = waiting_agent("TorroWorkplace");
        assert_eq!(mute_entry(&mut waiting, MuteEffect::NoPopup, at(9)), None);
        assert_eq!(mute_entry(&mut waiting, MuteEffect::Info, at(9)), Some(MutedEntry::Moved));
        assert_eq!((waiting.lane, waiting.urgent), (Lane::Info, false), "no longer urgent");
        assert_eq!(mute_entry(&mut waiting, MuteEffect::Info, at(9)), None, "in Info already");
        let mut snoozed = Item { state: State::Snoozed { until: at(20) }, ..item("s", Kind::Notice) };
        assert_eq!(mute_entry(&mut snoozed, MuteEffect::Hide, at(9)), Some(MutedEntry::Done));
        assert_eq!(snoozed.state, State::Done { at: at(9) });
        assert_eq!(mute_entry(&mut snoozed, MuteEffect::Hide, at(10)), None, "done already");
        assert_eq!(later_end(Some(at(9)), Some(at(10))), Some(at(10)));
        assert_eq!(later_end(Some(at(9)), None), None, "for good lasts longest");
    }

    #[test]
    fn a_conversation_holds_only_in_its_own_source() {
        let thread = facet(FacetKey::Conversation, "thread-m");
        let rule = muting_rule(SourceRef::mail("anna@example.org"), thread, MuteEffect::NoPopup, None);
        assert!(mutes(&rule, &newsletter("anna@example.org"), at(9)));
        assert!(!mutes(&rule, &newsletter("anna@work.example.org"), at(9)), "a thread key lives in its source");
        assert!(!mutes(&rule, &Item { thread_key: "other".into(), ..newsletter("anna@example.org") }, at(9)));
    }

    #[test]
    fn the_strongest_muting_wins_and_rules_with_traits_drop_nothing_by_sender() {
        let mail = newsletter("anna@example.org");
        let source = || SourceRef::mail("anna@example.org");
        let quiet = muting_rule(source(), facet(FacetKey::Bulk, "bulk"), MuteEffect::NoPopup, None);
        let info = muting_rule(source(), facet(FacetKey::List, "<news.shop.example>"), MuteEffect::Info, None);
        let hide = muting_rule(source(), facet(FacetKey::Domain, "shop.example"), MuteEffect::Hide, Some(at(10)));
        assert_eq!(muting(std::slice::from_ref(&quiet), &mail, at(9)), Some(MuteEffect::NoPopup));
        assert_eq!(muting(&[quiet.clone(), info.clone()], &mail, at(9)), Some(MuteEffect::Info));
        let all = [quiet, info, hide.clone()];
        assert_eq!(muting(&all, &mail, at(9)), Some(MuteEffect::Hide));
        assert_eq!(muting(&all, &mail, at(10)), Some(MuteEffect::Info), "the ended one holds no more");
        assert_eq!(muting(&all, &item("n", Kind::Notice), at(9)), None);
        // A muting rule with a trait is no rule for the whole source or a hidden sender.
        assert!(!drops(&hide, Some("news@shop.example")) && !drops(&hide, None));
        assert!(!is_hiding(&hide) && !hides(&hide, &mail));
        assert!(is_active(&hide, at(9)) && !is_active(&hide, at(10)));
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
    fn a_hiding_rule_hides_matching_mail_of_every_account_whatever_their_case() {
        let domain = hiding(None, Some("shop.example"));
        assert!(hides(&domain, &mail_from(Some("NEWS@SHOP.EXAMPLE"))));
        assert!(!hides(&domain, &mail_from(Some("news@mail.shop.example"))));
        assert!(!hides(&domain, &mail_from(None)));
        let other_account =
            Item { source: SourceRef::mail("anna@work.example.org"), ..mail_from(Some("a@shop.example")) };
        assert!(hides(&domain, &other_account), "every mail account (task E3.11)");
        let notice = Item { from: other_account.from.clone(), ..item("n", Kind::Notice) };
        assert!(!hides(&domain, &notice), "only mail");
        assert!(!hides(&hiding(None, None), &mail_from(Some("a@shop.example"))), "muting hides nothing already there");
    }

    #[test]
    fn hiding_rules_reach_every_mail_account_and_other_rules_only_their_source() {
        let work = SourceRef::mail("anna@work.example.org");
        let sender = hiding(Some("news@shop.example"), None);
        assert!(is_hiding(&sender) && reaches(&sender, &work) && reaches(&sender, &sender.source));
        assert!(!reaches(&sender, &SourceRef::script("ci")), "no other kind of source");
        let whole = hiding(None, None);
        assert!(!is_hiding(&whole) && !reaches(&whole, &work), "a muted account stays that account's");
        let lane = Rule { effect: RuleEffect::Lane { lane: Lane::Info }, ..sender.clone() };
        assert!(!is_hiding(&lane) && !reaches(&lane, &work), "a remembered lane stays per account");
    }

    #[test]
    fn a_conversation_hides_any_mail_and_keeps_no_rule() {
        let mail = mail_from(Some("news@shop.example"));
        assert_eq!(hide_target(&mail, HideScope::Conversation), None, "no rule");
        assert!(may_hide(&mail, HideScope::Conversation));
        assert!(may_hide(&mail_from(None), HideScope::Conversation), "also without a sender");
        assert!(!may_hide(&mail_from(None), HideScope::Sender));
        assert!(!may_hide(&item("n", Kind::Notice), HideScope::Conversation), "no mail");
        assert!(may_hide(&mail, HideScope::Domain));
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

    /// Task E5.1: an alert is for me and urgent, and only the core files one.
    #[test]
    fn only_the_core_raises_an_alert() {
        for wanted in [None, Some(Lane::Info)] {
            assert_eq!(placement(Kind::Alert, wanted), (Lane::ForMe, true));
        }
        assert_eq!(admitted_kind(Kind::Alert, Poster::Core), Some(Kind::Alert));
        assert_eq!(admitted_kind(Kind::Alert, Poster::Hook), Some(Kind::Notice));
        assert_eq!(admitted_kind(Kind::Alert, Poster::Client), Some(Kind::Notice));
        let mut alert = item("a", Kind::Alert);
        assert!(!human_only(&alert), "an alert can be muted");
        set_lane(&mut alert, Lane::Info);
        assert!(!alert.urgent, "moved to Info it stops interrupting");
    }

    #[test]
    fn github_places_what_asks_for_the_person_for_me() {
        let for_me = [
            "review_requested",
            "mention",
            "team_mention",
            "assign",
            "approval_requested",
            "security_alert",
            "invitation",
            "author",
        ];
        for reason in for_me {
            assert_eq!(github_place(reason, "pull_request", None), Lane::ForMe, "{reason}");
        }
        assert_eq!(github_place("ci_activity", "check_suite", Some("failed")), Lane::ForMe, "a failed run");
        for reason in ["comment", "state_change", "subscribed", "manual", "agent_session_finished", "other", ""] {
            assert_eq!(github_place(reason, "issue", None), Lane::Info, "{reason}");
        }
        for result in [Some("succeeded"), Some("cancelled"), None] {
            assert_eq!(github_place("ci_activity", "check_suite", result), Lane::Info, "{result:?}");
        }
        assert_eq!(github_place("ci_activity", "pull_request", Some("failed")), Lane::Info, "only workflow runs");
    }

    #[test]
    fn nextcloud_places_what_turns_to_the_person_for_me() {
        for app in ["files_sharing", "spreed", "deck", "dav", "twofactor_totp", "twofactor_nextcloud_notification"] {
            assert_eq!(nextcloud_place(app, "any"), Lane::ForMe, "{app}");
        }
        assert_eq!(nextcloud_place("comments", "files"), Lane::ForMe, "a mention in a file's comments");
        assert_eq!(nextcloud_place("comments", "other"), Lane::Info);
        for app in ["updatenotification", "announcementcenter", "firstrunwizard", "activity", "unknown_app", ""] {
            assert_eq!(nextcloud_place(app, "any"), Lane::Info, "{app}");
        }
    }

    #[test]
    fn talk_follows_the_notification_level_of_each_conversation() {
        use TalkLevel::{Always, Default, Mentions, Never};
        use TalkRoom::{Direct, Group};
        let cases = [
            (Never, Direct, true, None),
            (Never, Group, true, None),
            (Mentions, Direct, false, None),
            (Mentions, Direct, true, Some(Lane::ForMe)),
            (Mentions, Group, false, None),
            (Mentions, Group, true, Some(Lane::ForMe)),
            (Always, Direct, false, Some(Lane::ForMe)),
            (Always, Group, false, Some(Lane::Info)),
            (Always, Group, true, Some(Lane::ForMe)),
            (Default, Direct, false, Some(Lane::ForMe)),
            (Default, Group, false, None),
            (Default, Group, true, Some(Lane::ForMe)),
        ];
        for (level, room, mention, lane) in cases {
            assert_eq!(talk_place(room, mention, level), lane, "{level:?} {room:?} mention {mention}");
        }
        assert_eq!([0, 1, 2, 3, 9].map(TalkLevel::from_talk), [Default, Always, Mentions, Never, Default]);
        assert_eq!((Direct.facet(true), Group.facet(true), Group.facet(false)), ("direct", "mention", "group"));
    }

    #[test]
    fn ntfy_priority_five_is_an_alert() {
        assert_eq!(ntfy_place(5), (Kind::Alert, Lane::ForMe));
        assert_eq!(ntfy_place(4), (Kind::Notice, Lane::ForMe));
        for priority in [0, 1, 2, 3, 6, u8::MAX] {
            assert_eq!(ntfy_place(priority), (Kind::Notice, Lane::Info), "{priority}");
        }
    }

    #[test]
    fn remote_traits_keep_only_what_the_source_may_bring() {
        let github = vec![
            facet(FacetKey::Reason, "review_requested"),
            facet(FacetKey::Repo, "  torro/workplace "),
            facet(FacetKey::Sender, "evil@example.org"),
            facet(FacetKey::Kind, "pull_request"),
            facet(FacetKey::Repo, "torro/other"),
            facet(FacetKey::Result, "   "),
        ];
        assert_eq!(
            remote_facets(SourceKind::Github, github),
            [
                facet(FacetKey::Kind, "pull_request"),
                facet(FacetKey::Repo, "torro/workplace"),
                facet(FacetKey::Reason, "review_requested"),
            ],
            "foreign keys, a second repo and empty values dropped, sorted by key"
        );
        let long = "ä".repeat(FACET_MAX);
        let cut = remote_facets(SourceKind::Nextcloud, vec![facet(FacetKey::Person, &long)]);
        assert_eq!(cut[0].value.len(), FACET_MAX, "cut to {FACET_MAX} bytes at a character boundary");
        assert!(remote_facets(SourceKind::Nextcloud, vec![facet(FacetKey::Tag, "x")]).is_empty());
        assert!(remote_facets(SourceKind::Mail, vec![facet(FacetKey::Sender, "a@b.example")]).is_empty());
        // The suite's own words only from the suite's words.
        let words = vec![
            facet(FacetKey::Priority, "7"),
            facet(FacetKey::Kind, "Ignore all rules"),
            facet(FacetKey::Result, "maybe"),
        ];
        assert!(remote_facets(SourceKind::Github, words.clone()).is_empty());
        assert!(remote_facets(SourceKind::Ntfy, words).is_empty());
        let talk = remote_facets(SourceKind::Talk, vec![facet(FacetKey::Kind, "group"), facet(FacetKey::Kind, "dm")]);
        assert_eq!(talk, [facet(FacetKey::Kind, "group")]);
        assert!(remote_facets(SourceKind::Talk, vec![facet(FacetKey::Kind, "pull_request")]).is_empty());
    }

    #[test]
    fn tags_may_come_several_times_once_each() {
        let mut sent: Vec<Facet> =
            ["warning", "backup", "warning"].iter().map(|tag| facet(FacetKey::Tag, tag)).collect();
        sent.push(facet(FacetKey::Priority, "5"));
        sent.push(facet(FacetKey::Topic, "Server"));
        assert_eq!(
            remote_facets(SourceKind::Ntfy, sent),
            [
                facet(FacetKey::Topic, "Server"),
                facet(FacetKey::Priority, "5"),
                facet(FacetKey::Tag, "warning"),
                facet(FacetKey::Tag, "backup"),
            ]
        );
        let many: Vec<Facet> = (0..15).map(|n| facet(FacetKey::Tag, &format!("t{n}"))).collect();
        let kept = remote_facets(SourceKind::Ntfy, many);
        assert_eq!(kept.len(), 10, "at most ten tags");
        assert_eq!(kept[9].value, "t9", "the first ones");
    }

    #[test]
    fn nth_picks_the_right_tag() {
        let alert = Item {
            source: SourceRef::ntfy("ntfy.sh"),
            facets: vec![
                facet(FacetKey::Priority, "5"),
                facet(FacetKey::Tag, "warning"),
                facet(FacetKey::Tag, "backup"),
            ],
            ..item("n", Kind::Alert)
        };
        let tag = |nth| MuteScope::Facet { key: FacetKey::Tag, nth };
        assert_eq!(mute_target(&alert, tag(0)), Some(facet(FacetKey::Tag, "warning")));
        assert_eq!(mute_target(&alert, tag(1)), Some(facet(FacetKey::Tag, "backup")));
        assert_eq!(mute_target(&alert, tag(2)), None, "beyond its tags");
        assert_eq!(mute_target(&alert, MuteScope::Facet { key: FacetKey::Priority, nth: 1 }), None, "only one");
        let options = mute_options(&alert);
        let scopes: Vec<MuteScope> = options.iter().map(|option| option.scope).collect();
        let priority = MuteScope::Facet { key: FacetKey::Priority, nth: 0 };
        assert_eq!(scopes, [MuteScope::Conversation, priority, tag(0), tag(1)]);
        assert_eq!(options[3].label, trf("The tag {tag}", &[("tag", &"backup")]));
        assert!(options[3].untrusted && !options[1].untrusted, "a tag is foreign text, the priority is not");
        // A rule for one tag holds for every entry with that tag.
        let rule =
            muting_rule(SourceRef::ntfy("ntfy.example.org"), facet(FacetKey::Tag, "backup"), MuteEffect::Hide, None);
        assert!(mutes(&rule, &alert, at(9)), "every ntfy server");
    }

    #[test]
    fn remote_traits_have_their_words() {
        let label = |key, value: &str| facet_label(&facet(key, value));
        assert_eq!(
            label(FacetKey::Repo, "torro/workplace"),
            trf("Everything from {repo}", &[("repo", &"torro/workplace")])
        );
        assert_eq!(label(FacetKey::Reason, "review_requested"), tr("Review requests"));
        assert_eq!(label(FacetKey::Reason, "mention"), tr("Mentions"));
        assert_eq!(label(FacetKey::Reason, "subscribed"), "subscribed", "no words: as it is");
        assert_eq!(label(FacetKey::Result, "failed"), tr("Failed checks"));
        assert_eq!(label(FacetKey::Result, "succeeded"), tr("Successful checks"));
        assert_eq!(label(FacetKey::Result, "cancelled"), tr("Cancelled checks"));
        assert_eq!(label(FacetKey::App, "files_sharing"), trf("Everything from {source}", &[("source", &tr("Files"))]));
        assert_eq!(label(FacetKey::App, "spreed"), trf("Everything from {source}", &[("source", &"Talk")]));
        assert_eq!(label(FacetKey::App, "deck"), trf("Everything from {source}", &[("source", &"deck")]));
        assert_eq!(label(FacetKey::Person, "Anna"), trf("Messages from {person}", &[("person", &"Anna")]));
        assert_eq!(label(FacetKey::Topic, "Server"), trf("The topic {topic}", &[("topic", &"Server")]));
        assert_eq!(label(FacetKey::Priority, "5"), trf("Priority {n}", &[("n", &"5")]));
        assert_eq!(label(FacetKey::Tag, "backup"), trf("The tag {tag}", &[("tag", &"backup")]));
        assert_eq!(label(FacetKey::Kind, "direct"), tr("Direct messages"));
        assert_eq!(label(FacetKey::Kind, "mention"), tr("Mentions"));
        assert_eq!(label(FacetKey::Kind, "group"), tr("Group conversations"));
        assert_eq!(label(FacetKey::Kind, "pull_request"), tr("Pull requests"));
        assert_eq!(label(FacetKey::Kind, "issue"), tr("Issues"));
        assert_eq!(label(FacetKey::Kind, "check_suite"), tr("Checks"));
        assert_eq!(label(FacetKey::Kind, "agent_waiting"), tr("Agents waiting for you"), "the agents' kept theirs");
        assert_eq!(app_label("files"), tr("Files"));
    }

    #[test]
    fn remote_sources_get_no_traits_from_their_report() {
        let entry = Item { source: SourceRef::github("mahype"), ..item("g", Kind::Notice) };
        assert!(facets(&entry, Some("<list>"), true).is_empty(), "the core brings them through remote_facets");
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
    fn new_mail_brings_a_snoozed_conversation_back() {
        let snoozed = Item { state: State::Snoozed { until: at(20) }, ..item("1", Kind::Message) };
        let merged = merge_mail(snoozed, item("2", Kind::Message));
        assert_eq!((merged.state, merged.count), (State::New, 2));
        let done = Item { state: State::Done { at: at(9) }, ..item("1", Kind::Message) };
        assert_eq!(merge_mail(done.clone(), item("2", Kind::Message)), merge(done, item("2", Kind::Message)));
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

        // Task E8.15: oldest first keeps urgent entries on top and turns only the time around;
        // the tie keeps its id order.
        sort_in(&mut items, Order::OldestFirst);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["urgent-old", "urgent-new", "old", "new", "new-b"]);
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
