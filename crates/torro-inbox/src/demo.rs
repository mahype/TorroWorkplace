//! Sample entries in memory, for building and trying the inbox app without the core.
//!
//! `torroinbox --demo` uses it (task E3.1). Changes live until the app closes. The sample is
//! built around "now" (which `--now` may fix, for repeatable screenshots), so the times in the
//! list always read "4 min", "2 h" and so on. It has every kind of entry the first steps bring:
//! two waiting agents, a finished one, an approval, an account problem, script notices in both
//! lanes, a snoozed and a done entry. The agents' sessions have names (task E2.6), except the done
//! one of Codex, which shows how an entry without a name looks. Quotes in titles and texts are
//! typographic (“…”, ’), as in the rest of the suite's texts.
//!
//! The remote sources of task E5.1 have one entry of each kind ([`remote_sample`]): a review
//! requested and a failed workflow run on GitHub, a file shared on Nextcloud, a one-to-one chat in
//! Talk and an ntfy alert "Backup failed" with priority 5 and a page to open. They are placed and
//! given their traits by the rules the core uses ([`rules::github_place`] …,
//! [`rules::remote_facets`]). GitHub also has a security alert seen three days ago (`demo-13`, until
//! task E5.15 a script named `github`), so its column has three entries. Their texts come from
//! servers in the real app, so they stay as they are, like mail.
//!
//! Mail (task E4.2): two mail accounts ([`MAIL_ACCOUNTS`], private and work) with their folders
//! and messages, and the entries the core would file for their new mail (task E4.1). The methods
//! that stand for the `mail.*` requests (`mail_folders`, `search_mail`, `get_mail`,
//! `mail_attachment`, `set_mail_flags`, `move_mail`, `move_mail_to`) are plain methods of
//! [`DemoSource`]; the app wraps them, as it wraps the core's client. They behave like the core
//! where the window can see it: reading a mail elsewhere or moving it out of the inbox closes its
//! entry, marking the entry done marks the mail read. The messages are foreign text in the real
//! app, so their subjects and texts stay as they are (not translated); folder names are the
//! server's.
//!
//! Writing (task E4.3): [`DemoSource::mail_senders`] lists the accounts a message can be written
//! from (with name and signature), [`DemoSource::save_draft`] keeps a draft in memory and
//! [`DemoSource::send_draft`] takes it out again; what was saved and sent is kept for the tests
//! ([`DemoSource::drafts`], [`DemoSource::sent`]). Like the core they refuse an unknown account,
//! an unknown draft id and a draft without a recipient.
//!
//! Saving attachments (issue #290): two save targets ([`save_targets`]) and a Downloads folder
//! ([`DEMO_DOWNLOADS`]); the invoice of the private inbox (`m:p3`) has two PDFs and a category the
//! model guessed. The app plans and answers saving from them without writing a file.
//!
//! The mail window's views of every account (task E18.3): [`DemoSource::by_role`] lists the mail of
//! one folder role, leaving out or giving only what is snoozed (the mail of a snoozed entry) and
//! what is hidden. One newsletter is hidden from the start ([`HIDDEN_MAIL`]), so "Hidden" is not
//! empty in a screenshot; [`DemoSource::set_mail_shown`] ("Show again") keeps the exception in
//! memory, as the core keeps it in `shown_mail`. The demo's hidden mail is a fixed fact: hiding a
//! sender or a conversation in the demo closes entries but hides no further mail.
//!
//! The piles and follow-ups of the mail window (task E19.3): two messages lie on "Reply later"
//! ([`REPLY_LATER_MAIL`]) and one on "Set aside" ([`SET_ASIDE_MAIL`]) from the start, kept on their
//! summaries ([`MessageSummary::pile`]), so every list names their pile and the window's "All"
//! (`pile: without`) leaves them out. [`DemoSource::set_mail_pile`] changes them in memory,
//! [`DemoSource::in_pile`] lists a pile, and a sent answer takes its conversation off "Reply later"
//! as the core does ([`DemoSource::send_draft`]). Anna's sent "Re: Sunday brunch" ([`FOLLOW_UP_MAIL`])
//! is followed up "only if no reply" ([`FOLLOW_UP_ENTRY`]); `inbox.snooze_item` follows up any
//! message by its `m:` id, as the core does.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use torro_core::i18n::{tr, trf};
use torro_mail::{
    AttachmentInfo, Category, DateSource, Draft, FileKind, Flag, Folder, MailAddress, MailCategory, MailPile, Message,
    MessageSummary, MessageTraits, Order, Role, SaveTarget, SaveTargetSpec,
};

use crate::item::{
    Action, AgentInfo, AgentKind, Facet, FacetKey, Hidden, HideScope, Item, Kind, Lane, LaneReason, Link, MuteEffect,
    MuteOption, MuteScope, Muted, Rule, RuleEffect, Sender, SourceInfo, SourceKind, SourceRef, State,
};
use crate::rules;
use crate::source::{InboxError, InboxSource, ItemFilter, TaskList};

/// The demo's mail hidden from the inbox (task E18.3): the work account's newsletter "Winter
/// jackets are here", which stands in "Hidden" and not in "All" until it is shown again.
pub const HIDDEN_MAIL: &str = "m:w14";

/// The approval behind the sample approval entry; its actions name it.
pub const APPROVAL_ID: &str = "demo-approval-1";

/// The demo's mail on "Reply later" (task E19.3): Clara's "Birthday dinner on the 24th" and Tom's
/// "Offsite: rooms are booked", both unread in an inbox.
pub const REPLY_LATER_MAIL: [&str; 2] = ["m:p7", "m:w13"];

/// The demo's mail on "Set aside" (task E19.3): the work account's "Planned maintenance on Saturday".
pub const SET_ASIDE_MAIL: &str = "m:w5";

/// The demo's sent mail followed up "only if no reply" (task E19.3): Anna's "Re: Sunday brunch".
pub const FOLLOW_UP_MAIL: &str = "m:p4";

/// The entry of the follow-up on [`FOLLOW_UP_MAIL`]: snoozed for two more days, `unless_reply`.
pub const FOLLOW_UP_ENTRY: &str = "demo-follow-p4";

#[derive(Debug)]
struct Data {
    items: Vec<Item>,
    muted: BTreeSet<SourceRef>,
    /// The ids of the entries dismissed ("✕"), as the core keeps them beside the entry: an entry
    /// opened again or done for real leaves it (issue #117, [`ItemFilter::dismissed`]).
    dismissed: BTreeSet<String>,
    /// The senders and domains hidden from the inbox (task E3.10) and the traits muted (task
    /// E8.6a), in memory only.
    hidden: Vec<Rule>,
    /// How many of these rules were made, for their ids: one removed leaves no gap to reuse.
    rules_made: usize,
    /// The tasks made from entries ("Make task", task E3.5), in memory only.
    tasks: Vec<DemoTask>,
    /// The folders of both mail accounts; their numbers are counted from `mail` when listed.
    folders: Vec<Folder>,
    /// Every message of both accounts, with its text.
    mail: Vec<Message>,
    /// The drafts saved from the compose window (task E4.3), in memory only.
    drafts: Vec<Draft>,
    /// The drafts that were sent.
    sent: Vec<Draft>,
    /// How many drafts were given an id.
    draft_count: u32,
    /// The mail hidden from the inbox while it lies in an inbox folder ([`HIDDEN_MAIL`]).
    hidden_mail: BTreeSet<String>,
    /// The hidden mail shown again (`inbox.set_mail_shown`, task E18.3): the exceptions.
    shown_mail: BTreeSet<String>,
    /// The answers sent from the demo – account, conversation and when –, so putting mail back on
    /// "Reply later" with an older time takes it off again at once, as the core's
    /// `take_off_answered` does (third review of #315).
    answers: Vec<(String, String, DateTime<Utc>)>,
}

impl Data {
    /// The rule hiding `sender` or `domain` in every mail account (task E3.11): the one already
    /// kept for it in any account, else a new one about `source`; with whether it is new.
    fn hiding_rule(
        &mut self,
        source: SourceRef,
        sender: Option<String>,
        domain: Option<String>,
        now: DateTime<Utc>,
    ) -> (Option<Rule>, bool) {
        let known =
            self.hidden.iter().find(|rule| rule.facet.is_none() && rule.sender == sender && rule.domain == domain);
        if let Some(known) = known {
            return (Some(known.clone()), false);
        }
        let id = self.next_rule_id();
        let rule =
            Rule { id, source, sender, domain, effect: RuleEffect::Muted, created: now, facet: None, until: None };
        self.hidden.push(rule.clone());
        (Some(rule), true)
    }

    fn next_rule_id(&mut self) -> String {
        self.rules_made += 1;
        format!("demo-rule-{}", self.rules_made)
    }

    /// The rule muting `facet` with `effect` until `until` (task E8.6a), as the core keeps it: the
    /// same rule already there for a source of the same kind (its end then the later of both), else
    /// a new one about `source`; with whether it is new.
    fn muting_rule(
        &mut self,
        source: SourceRef,
        facet: Facet,
        effect: RuleEffect,
        until: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> (Rule, bool) {
        let same = |rule: &Rule| {
            rule.facet.as_ref() == Some(&facet)
                && rule.effect == effect
                && rules::reaches(rule, &source)
                && rules::is_active(rule, now)
        };
        if let Some(known) = self.hidden.iter_mut().find(|rule| same(rule)) {
            known.until = rules::later_end(known.until, until);
            return (known.clone(), false);
        }
        let id = self.next_rule_id();
        let rule = Rule { id, source, sender: None, domain: None, effect, created: now, facet: Some(facet), until };
        self.hidden.push(rule.clone());
        (rule, true)
    }

    /// The entry `inbox.snooze_item` with `id` changes, as the core finds it (task E19.3): the entry
    /// `id`; for a mail message (`m:…`) the entry it was filed in, else the mail entry of its
    /// conversation, else – only when it is put off (`until`) – a new entry made from the message
    /// ([`rules::message_stand_in`], decision 8 of E19), kept with the id `demo-follow-<id>`. Without
    /// `until` such a message stays without an entry and the answer is the entry it would be.
    /// [`InboxError::NotFound`] for anything else.
    fn snooze_target(&mut self, id: &str, until: bool) -> Result<SnoozeTarget, InboxError> {
        if let Some(at) = self.items.iter().position(|item| item.id == id) {
            return Ok(SnoozeTarget::Entry(at));
        }
        if !rules::is_message_id(id) {
            return Err(InboxError::NotFound);
        }
        let summary =
            self.mail.iter().find(|m| m.summary.id == id).map(|m| m.summary.clone()).ok_or(InboxError::NotFound)?;
        let filed = |item: &Item| matches!(&item.link, Some(Link::Mail { message }) if message == id);
        let of_thread = |item: &Item| {
            item.kind == Kind::Message && item.thread_key == summary.thread_id && item.source.key() == summary.account
        };
        if let Some(at) = self.items.iter().position(filed).or_else(|| self.items.iter().position(of_thread)) {
            return Ok(SnoozeTarget::Entry(at));
        }
        let mut stand_in = rules::message_stand_in(&summary, None, false).ok_or(InboxError::NotFound)?;
        if !until {
            return Ok(SnoozeTarget::StandIn(Box::new(stand_in)));
        }
        stand_in.id = format!("demo-follow-{}", id.trim_start_matches("m:"));
        self.items.push(stand_in);
        Ok(SnoozeTarget::Entry(self.items.len() - 1))
    }

    /// What `inbox.list_mute_options` and `inbox.mute_item` with `id` mute by, as the core finds it
    /// (issue #123): the entry `id`; for a mail message (`m:…`) the entry it was filed in, else the
    /// message as an entry would hold it ([`rules::message_stand_in`], `true`: a stand-in, whose
    /// conversation is not offered). [`InboxError::NotFound`] for anything else.
    fn mute_subject(&self, id: &str) -> Result<(Item, bool), InboxError> {
        if let Some(item) = self.items.iter().find(|item| item.id == id) {
            return Ok((item.clone(), false));
        }
        if !rules::is_message_id(id) {
            return Err(InboxError::NotFound);
        }
        let filed = |item: &&Item| matches!(&item.link, Some(Link::Mail { message }) if message == id);
        if let Some(item) = self.items.iter().find(filed) {
            return Ok((item.clone(), false));
        }
        let message = self.mail.iter().find(|message| message.summary.id == id).ok_or(InboxError::NotFound)?;
        let traits = &message.traits;
        let bulk = traits.list_id.is_some() || traits.list_unsubscribe || traits.precedence.is_some();
        let stand_in = rules::message_stand_in(&message.summary, traits.list_id.as_deref(), bulk);
        stand_in.map(|item| (item, true)).ok_or(InboxError::NotFound)
    }
}

/// What `inbox.snooze_item` changes ([`Data::snooze_target`]).
enum SnoozeTarget {
    /// The entry at this place of [`Data::items`].
    Entry(usize),
    /// A message without an entry and no time to put it off to: the entry it would be, not kept.
    StandIn(Box<Item>),
}

/// A task the demo made from an entry: the list's id, the title and the due day.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoTask {
    list: String,
    title: String,
    due: Option<NaiveDate>,
}

/// The ids of the two task lists the demo offers for "Make task" (task E3.5).
pub const DEMO_LISTS: [&str; 2] = ["demo/personal", "demo/work"];

/// The two task lists of the demo, "Personal" and "Work", in the user's language.
fn demo_lists() -> Vec<TaskList> {
    let [personal, work] = DEMO_LISTS;
    vec![
        TaskList { id: personal.into(), name: tr("Personal").into() },
        TaskList { id: work.into(), name: tr("Work").into() },
    ]
}

/// See the module docs.
#[derive(Debug)]
pub struct DemoSource {
    data: Mutex<Data>,
    /// How far the demo's clock is ahead of the real one: "now" as given to [`DemoSource::new`]
    /// minus the real time then. Keeps a fixed `--now` and snoozing consistent while time passes.
    offset: TimeDelta,
}

impl DemoSource {
    /// The sample entries around `now`, which is also where the demo's clock starts.
    pub fn new(now: DateTime<Utc>) -> DemoSource {
        let mut items = sample(now);
        let mail = mail_sample(now);
        items.extend(mail_entries(&mail));
        items.extend(follow_up_entry(&mail, now));
        let data = Data {
            items,
            muted: BTreeSet::new(),
            dismissed: BTreeSet::new(),
            hidden: Vec::new(),
            rules_made: 0,
            tasks: Vec::new(),
            folders: folder_sample(),
            mail,
            drafts: Vec::new(),
            sent: Vec::new(),
            draft_count: 0,
            hidden_mail: BTreeSet::from([HIDDEN_MAIL.to_owned()]),
            shown_mail: BTreeSet::new(),
            answers: Vec::new(),
        };
        DemoSource { data: Mutex::new(data), offset: now - Utc::now() }
    }

    fn now(&self) -> DateTime<Utc> {
        Utc::now() + self.offset
    }

    fn lock(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies `change` to the entry `id` and returns it as changed.
    fn change(&self, id: &str, change: impl FnOnce(&mut Item)) -> Result<Item, InboxError> {
        let mut data = self.lock();
        let item = data.items.iter_mut().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
        change(item);
        Ok(item.clone())
    }
}

/// An open entry from `source`, placed by its kind, that arrived `minutes` before `now`. Set the
/// rest with struct update syntax.
fn entry(now: DateTime<Utc>, n: u32, source: SourceRef, kind: Kind, title: String, minutes: i64) -> Item {
    let (lane, urgent) = rules::placement(kind, None);
    let time = now - TimeDelta::minutes(minutes);
    Item {
        id: format!("demo-{n}"),
        thread_key: format!("demo-thread-{n}"),
        source,
        kind,
        lane,
        urgent,
        title,
        body: None,
        from: None,
        agent: None,
        received: time,
        updated: time,
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
        follow_up: None,
    }
}

/// A notice from the script `name` in `lane`, titled `title` (translated already).
fn notice(now: DateTime<Utc>, n: u32, name: &str, lane: Lane, title: &str, minutes: i64) -> Item {
    Item { lane, ..entry(now, n, SourceRef::script(name), Kind::Notice, title.into(), minutes) }
}

/// What a hook reports about a session of `agent` in `project` on `branch`, named `title` by the
/// agent (task E2.6). Names are foreign text, so they stay as they are, like mail subjects.
fn agent(agent: AgentKind, session: &str, project: &str, branch: &str, title: Option<&str>) -> Option<AgentInfo> {
    Some(AgentInfo {
        agent,
        session: session.into(),
        project: Some(project.into()),
        branch: Some(branch.into()),
        pids: Vec::new(),
        title: title.map(String::from),
    })
}

/// The sample entries, twelve of them and the six of [`remote_sample`]; times are relative to `now`.
pub fn sample(now: DateTime<Utc>) -> Vec<Item> {
    let claude = || SourceRef::agent(AgentKind::ClaudeCode.id());
    let codex = || SourceRef::agent(AgentKind::Codex.id());
    let waiting = |name: AgentKind| trf("{agent} is waiting for you", &[("agent", &name.label())]);
    let finished = |name: AgentKind| trf("{agent} finished", &[("agent", &name.label())]);
    vec![
        Item {
            thread_key: "claude-code:demo-claude-1".into(),
            body: Some(tr("Claude needs your permission to use Bash: cargo test --workspace").into()),
            agent: agent(
                AgentKind::ClaudeCode,
                "demo-claude-1",
                "TorroWorkplace",
                "feat/inbox",
                Some("Inbox: show session names"),
            ),
            link: Some(Link::Window),
            ..entry(now, 1, claude(), Kind::AgentWaiting, waiting(AgentKind::ClaudeCode), 4)
        },
        Item {
            thread_key: "codex:demo-codex-1".into(),
            body: Some(tr("Codex wants to run: npm install").into()),
            agent: agent(AgentKind::Codex, "demo-codex-1", "torro-design", "main", Some("Update design tokens")),
            link: Some(Link::Window),
            count: 2,
            received: now - TimeDelta::minutes(40),
            ..entry(now, 2, codex(), Kind::AgentWaiting, waiting(AgentKind::Codex), 12)
        },
        Item {
            thread_key: format!("approval:{APPROVAL_ID}"),
            // The summary quotes what the agent asked for, so it is foreign text.
            body: Some(tr("Requested by Claude Desktop over MCP. Deleting cannot be undone.").into()),
            actions: vec![
                Action::Allow { approval: APPROVAL_ID.into() },
                Action::Deny { approval: APPROVAL_ID.into() },
            ],
            ..entry(
                now,
                3,
                SourceRef::suite(),
                Kind::Approval,
                tr("Claude Desktop wants to delete the event “Team meeting”").into(),
                8,
            )
        },
        Item {
            thread_key: "claude-code:demo-claude-2".into(),
            body: Some(tr("Fixed the footer links on the website. 3 files changed.").into()),
            agent: agent(AgentKind::ClaudeCode, "demo-claude-2", "website", "fix/footer", Some("Fix footer links")),
            link: Some(Link::Window),
            state: State::Seen,
            ..entry(now, 4, claude(), Kind::AgentDone, finished(AgentKind::ClaudeCode), 35)
        },
        Item {
            thread_key: "account:nextcloud:anna@work.example.org".into(),
            body: Some(tr("Enter a new app password to keep calendars and tasks in sync.").into()),
            link: Some(Link::App { app: "torroworkplace".into() }),
            state: State::Seen,
            // Written by the suite itself, no foreign text.
            untrusted: false,
            ..entry(
                now,
                5,
                SourceRef::suite(),
                Kind::AccountProblem,
                trf("Sign-in to {server} was refused", &[("server", &"work.example.org")]),
                120,
            )
        },
        Item {
            body: Some(tr("Step “migrate” exited with code 1.").into()),
            link: Some(Link::Url { url: "https://ci.example.org/runs/1842".into() }),
            ..notice(now, 6, "deploy", Lane::ForMe, tr("Deploy to staging failed"), 50)
        },
        Item {
            body: Some(tr("212 GB copied, no errors.").into()),
            count: 3,
            received: now - TimeDelta::days(2),
            ..notice(now, 7, "backup", Lane::Info, tr("Backup finished"), 360)
        },
        Item {
            link: Some(Link::Url { url: "https://ci.example.org/runs/1841".into() }),
            state: State::Seen,
            ..notice(now, 8, "ci", Lane::Info, tr("Build passed on main"), 80)
        },
        notice(now, 9, "uptime", Lane::Info, tr("All 12 monitors are up again"), 180),
        Item {
            body: Some(tr("3 of the last 10 runs failed in the same test.").into()),
            state: State::Snoozed { until: now + TimeDelta::hours(20) },
            ..notice(now, 10, "ci", Lane::ForMe, tr("Nightly build is flaky"), 24 * 60)
        },
        Item {
            thread_key: "codex:demo-codex-0".into(),
            body: Some(tr("Refactored the settings module.").into()),
            agent: agent(AgentKind::Codex, "demo-codex-0", "torro-api", "refactor/settings", None),
            link: Some(Link::Window),
            state: State::Done { at: now - TimeDelta::hours(22) },
            ..entry(now, 11, codex(), Kind::AgentDone, finished(AgentKind::Codex), 26 * 60)
        },
        // Yesterday and earlier (task E3.11), so the overview shows every group of its columns.
        Item {
            body: Some(tr("2 invoices could not be matched to a customer.").into()),
            ..notice(now, 12, "invoices", Lane::ForMe, tr("Invoice run needs a check"), 22 * 60)
        },
    ]
    .into_iter()
    // The traits the core gives every report (task E8.6a).
    .map(|item| Item { facets: rules::facets(&item, None, false), ..item })
    .chain(remote_sample(now))
    .collect()
}

/// The traits `pairs` of a remote source of `kind`, as the core files them ([`rules::remote_facets`]).
fn remote_facets(kind: SourceKind, pairs: &[(FacetKey, &str)]) -> Vec<Facet> {
    rules::remote_facets(kind, pairs.iter().map(|&(key, value)| Facet { key, value: value.into() }).collect())
}

/// One entry of each remote source of task E5.1, `demo-14` to `demo-18`, and GitHub's security alert
/// `demo-13` (task E5.15; see the module docs).
pub fn remote_sample(now: DateTime<Utc>) -> Vec<Item> {
    let github = || SourceRef::github("anna-berg");
    let cloud = "anna@cloud.example.org";
    let review =
        [(FacetKey::Kind, "pull_request"), (FacetKey::Repo, "torro/workplace"), (FacetKey::Reason, "review_requested")];
    let shared = [(FacetKey::App, "files_sharing"), (FacetKey::Person, "Jonas Weber")];
    let run = [
        (FacetKey::Kind, "check_suite"),
        (FacetKey::Repo, "torro/workplace"),
        (FacetKey::Reason, "ci_activity"),
        (FacetKey::Result, "failed"),
    ];
    let alerts = [
        (FacetKey::Kind, "repository_vulnerability_alert"),
        (FacetKey::Repo, "torro/website"),
        (FacetKey::Reason, "security_alert"),
    ];
    let lena = Sender { name: "Lena Hoffmann".into(), address: None };
    let jonas = Sender { name: "Jonas Weber".into(), address: None };
    let (alert, alert_lane) = rules::ntfy_place(5);
    vec![
        // Seen three days ago, so the overview has an entry before yesterday (task E3.11). Until
        // task E5.15 a script named `github` reported it.
        Item {
            thread_key: "8812345513".into(),
            lane: rules::github_place("security_alert", "repository_vulnerability_alert", None),
            state: State::Seen,
            link: Some(Link::Url { url: "https://github.com/torro/website/security/dependabot".into() }),
            facets: remote_facets(SourceKind::Github, &alerts),
            ..entry(now, 13, github(), Kind::Notice, "2 vulnerable packages in website".into(), 3 * 24 * 60)
        },
        Item {
            thread_key: "8812345601".into(),
            lane: rules::github_place("review_requested", "pull_request", None),
            link: Some(Link::Url { url: "https://github.com/torro/workplace/pull/214".into() }),
            facets: remote_facets(SourceKind::Github, &review),
            ..entry(now, 14, github(), Kind::Notice, "Inbox: sources for GitHub, Nextcloud and ntfy".into(), 18)
        },
        Item {
            thread_key: "8812345602".into(),
            lane: rules::github_place("ci_activity", "check_suite", Some("failed")),
            link: Some(Link::Url { url: "https://github.com/torro/workplace/actions".into() }),
            facets: remote_facets(SourceKind::Github, &run),
            ..entry(now, 15, github(), Kind::Notice, "CI workflow run failed for main branch".into(), 45)
        },
        Item {
            thread_key: "files_sharing:share:1842".into(),
            lane: rules::nextcloud_place("files_sharing", "share"),
            from: Some(jonas),
            link: Some(Link::Url { url: "https://cloud.example.org/apps/files/?openfile=1842".into() }),
            facets: remote_facets(SourceKind::Nextcloud, &shared),
            ..entry(
                now,
                16,
                SourceRef::nextcloud(cloud),
                Kind::Notice,
                "Jonas Weber shared “Budget 2027.xlsx” with you".into(),
                70,
            )
        },
        Item {
            thread_key: "k7pq2xd9".into(),
            lane: rules::talk_place(rules::TalkRoom::Direct, false, rules::TalkLevel::Default).unwrap_or(Lane::ForMe),
            body: Some("Lena: Can we go through the sidebar drafts before 3?".into()),
            from: Some(lena),
            count: 2,
            link: Some(Link::Url { url: "https://cloud.example.org/call/k7pq2xd9".into() }),
            facets: remote_facets(
                SourceKind::Talk,
                &[(FacetKey::Kind, rules::TalkRoom::Direct.facet(false)), (FacetKey::Person, "Lena Hoffmann")],
            ),
            ..entry(now, 17, SourceRef::talk(cloud), Kind::Message, "Lena Hoffmann".into(), 6)
        },
        Item {
            thread_key: "backup:Backup failed".into(),
            lane: alert_lane,
            body: Some("rsync to the NAS stopped: “No space left on device”.".into()),
            // ntfy's `click`: the page the message points to.
            link: Some(Link::Url { url: "https://nas.example.org/backups".into() }),
            facets: remote_facets(
                SourceKind::Ntfy,
                &[
                    (FacetKey::Topic, "Server"),
                    (FacetKey::Priority, "5"),
                    (FacetKey::Tag, "warning"),
                    (FacetKey::Tag, "backup"),
                ],
            ),
            ..entry(now, 18, SourceRef::ntfy("ntfy.sh"), alert, "Backup failed".into(), 3)
        },
    ]
}

/// The two mail accounts of the demo: work and private, as `mail:<address>`, in the order the core lists them.
pub const MAIL_ACCOUNTS: [&str; 2] = ["mail:anna@work.example.org", "mail:anna@example.org"];

/// The Downloads folder of the demo (issue #290), as `mail.list_save_targets` names it. Nothing is
/// ever written there: the demo's saving answers without a file.
pub const DEMO_DOWNLOADS: &str = "/home/anna/Downloads";

/// The save targets of the demo (issue #290, `mail.list_save_targets`): "Invoices" by year and
/// month of each file's own date (from its name, else the mail's), suggested for invoices, and
/// "Contracts" by sender, suggested for documents that are PDFs. The demo's invoice (`m:p3`) is a
/// guess of the model, so "Invoices" is suggested for it but not sure.
pub fn save_targets() -> Vec<SaveTarget> {
    let mut invoices = SaveTargetSpec::new("Invoices", "/home/anna/Documents/Invoices");
    invoices.date_from = DateSource::FileName;
    invoices.suggest.categories = vec![Category::Invoice];
    let mut contracts = SaveTargetSpec::new("Contracts", "/home/anna/Documents/Contracts");
    contracts.subfolders = "{sender}".into();
    contracts.file_name = "{date}_{name}.{ext}".into();
    contracts.suggest.categories = vec![Category::Document];
    contracts.suggest.kinds = vec![FileKind::Pdf];
    vec![
        SaveTarget { id: "demo-invoices".into(), spec: invoices },
        SaveTarget { id: "demo-contracts".into(), spec: contracts },
    ]
}

/// A mail account a message can be written from (`accounts.list_accounts` in the core): the sender
/// name next to the address and the signature the core puts under new mail, replies and forwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoSender {
    /// `mail:<address>`.
    pub account: String,
    /// The name in `From:`.
    pub name: String,
    /// The account's signature; `None` for none.
    pub signature: Option<String>,
}

/// The folders with a role of one demo account: `(id suffix, name, role)`, in the order a server
/// lists them.
const FOLDERS: [(&str, &str, Role); 6] = [
    ("inbox", "Inbox", Role::Inbox),
    ("drafts", "Drafts", Role::Drafts),
    ("sent", "Sent", Role::Sent),
    ("archive", "Archive", Role::Archive),
    ("junk", "Junk", Role::Junk),
    ("trash", "Trash", Role::Trash),
];

/// The id of the folder `suffix` of the account `account` (an index into [`MAIL_ACCOUNTS`]):
/// `f:private-inbox`, `f:work-projects`.
fn folder_id(account: usize, suffix: &str) -> String {
    let who = if account == 0 { "work" } else { "private" };
    format!("f:{who}-{suffix}")
}

/// Both accounts' folders: the six with a role, and one of each account's own ("Receipts",
/// "Projects"). The numbers are counted when listed.
fn folder_sample() -> Vec<Folder> {
    let folder = |account: usize, suffix: &str, name: &str, role: Option<Role>| Folder {
        id: folder_id(account, suffix),
        account: MAIL_ACCOUNTS[account].into(),
        name: name.into(),
        path: if role == Some(Role::Inbox) { "INBOX".into() } else { format!("INBOX.{name}") },
        role,
        unread: 0,
        total: 0,
    };
    let mut folders = Vec::new();
    for (account, own) in [(0, "Projects"), (1, "Receipts")] {
        folders.extend(FOLDERS.iter().map(|&(suffix, name, role)| folder(account, suffix, name, Some(role))));
        folders.push(folder(account, &own.to_lowercase(), own, None));
    }
    folders
}

/// What a demo message is made of; [`letter`] turns it into a [`Message`].
struct Letter {
    id: &'static str,
    /// Index into [`MAIL_ACCOUNTS`].
    account: usize,
    /// The folder's id suffix ([`folder_id`]).
    folder: &'static str,
    /// The conversation; empty: a conversation of its own.
    thread: &'static str,
    /// `(name, address)` of the sender; empty: Anna herself (sent mail, drafts).
    from: (&'static str, &'static str),
    subject: &'static str,
    /// How long before "now" it arrived.
    minutes: i64,
    flags: &'static [Flag],
    text: &'static str,
    /// `(file name, MIME type, size)`.
    attachments: &'static [(&'static str, &'static str, u64)],
    from_html: bool,
    /// The cleaned HTML part, as the core keeps it (task M9.5: legacy attributes already written as
    /// CSS); the inbox draws it in its reader (task M9.7). `None`: text only.
    html: Option<&'static str>,
    /// The pictures the HTML shows from inline parts (`cid:`): their Content-IDs in
    /// [`PICTURES`].
    pictures: &'static [&'static str],
    /// Sent to a list or by a machine (`List-Id`): the inbox files it under "Info".
    bulk: bool,
    /// What the mail is (task E12.8), as the demo pretends a model or a rule decided; `None`:
    /// nothing said about it, so the sidebar's group shows how that reads too.
    category: Option<MailCategory>,
    /// The pile it lies on from the start (task E19.3); `None` for most.
    pile: Option<MailPile>,
}

impl Letter {
    const fn new(id: &'static str, account: usize, folder: &'static str, subject: &'static str) -> Letter {
        Letter {
            id,
            account,
            folder,
            thread: "",
            from: ("", ""),
            subject,
            minutes: 0,
            flags: &[],
            text: "",
            attachments: &[],
            from_html: false,
            html: None,
            pictures: &[],
            bulk: false,
            category: None,
            pile: None,
        }
    }
}

/// `name <address>`, or the address alone without a name.
fn address(name: &str, address: &str) -> MailAddress {
    MailAddress { name: (!name.is_empty()).then(|| name.to_owned()), address: address.to_owned() }
}

/// The message `letter` describes, `letter.minutes` before `now`.
fn letter(now: DateTime<Utc>, letter: Letter) -> Message {
    let me = MAIL_ACCOUNTS[letter.account].trim_start_matches("mail:");
    let (name, from) = letter.from;
    let (from, to) = if from.is_empty() {
        (address("Anna Berg", me), vec![address("", "team@work.example.org")])
    } else {
        (address(name, from), vec![address("Anna Berg", me)])
    };
    let snippet = letter.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
    let thread = if letter.thread.is_empty() { letter.id } else { letter.thread };
    let mut attachments: Vec<AttachmentInfo> = letter
        .attachments
        .iter()
        .enumerate()
        .map(|(i, &(file, kind, size))| AttachmentInfo {
            part: (i + 2).to_string(),
            filename: Some(file.into()),
            content_type: kind.into(),
            size,
            inline: false,
            content_id: None,
        })
        .collect();
    let files = attachments.len();
    // The pictures of the HTML follow as inline parts, as mail tools send them.
    attachments.extend(letter.pictures.iter().enumerate().map(|(i, &content_id)| AttachmentInfo {
        part: (files + i + 2).to_string(),
        filename: Some(picture_name(content_id)),
        content_type: "image/png".into(),
        size: picture(content_id).map_or(0, |bytes| bytes.len() as u64),
        inline: true,
        content_id: Some(content_id.into()),
    }));
    Message {
        summary: MessageSummary {
            id: letter.id.into(),
            account: MAIL_ACCOUNTS[letter.account].into(),
            folder: folder_id(letter.account, letter.folder),
            thread_id: format!("t:{thread}"),
            from: Some(from),
            to,
            subject: letter.subject.into(),
            date: now - TimeDelta::minutes(letter.minutes),
            snippet,
            flags: letter.flags.to_vec(),
            has_attachments: files > 0,
            invite: None,
            back: None,
            pile: letter.pile,
            category: letter.category,
            untrusted: true,
            // The sample's piles since the mail came (decision of #315: the undo keeps this time).
            pile_since: letter.pile.map(|_| now - TimeDelta::minutes(letter.minutes)),
        },
        cc: Vec::new(),
        reply_to: Vec::new(),
        text: Some(letter.text.into()),
        from_html: letter.from_html,
        html: letter.html.map(str::to_owned),
        attachments,
        traits: MessageTraits {
            list_id: letter.bulk.then(|| "<news.example>".to_owned()),
            list_unsubscribe: letter.bulk,
            ..MessageTraits::default()
        },
        invitation: false,
    }
}

/// How many attachments this process has placed, for the name of the file each is written to
/// before it is renamed into place ([`DemoSource::mail_attachment`]).
static PLACED: AtomicU64 = AtomicU64::new(0);

/// The pictures of the demo's HTML mail, by Content-ID: the shop's newsletter (`m:p2`) shows them
/// from inline parts (`cid:`), so it draws without the network. Abstract shapes and gradients, the
/// self-made samples of `torro-htmlview`'s tests.
const PICTURES: [(&str, &[u8]); 4] = [
    ("logo@shop.example", include_bytes!("../../torro-htmlview/tests/fixtures/cid/logo@torro.example.png")),
    ("hero@shop.example", include_bytes!("../../torro-htmlview/tests/fixtures/cid/hero@torro.example.png")),
    ("rain@shop.example", include_bytes!("../../torro-htmlview/tests/fixtures/cid/tiles@torro.example.png")),
    ("down@shop.example", include_bytes!("../../torro-htmlview/tests/fixtures/cid/tasks@torro.example.png")),
];

/// The bytes of the demo picture with this Content-ID ([`PICTURES`]).
fn picture(content_id: &str) -> Option<&'static [u8]> {
    PICTURES.iter().find(|(id, _)| *id == content_id).map(|(_, bytes)| *bytes)
}

/// The file name of a demo picture: its Content-ID's name before the `@`, as PNG.
fn picture_name(content_id: &str) -> String {
    format!("{}.png", content_id.split('@').next().unwrap_or(content_id))
}

/// The HTML of the demo's newsletter made from HTML (`m:p2`), as the core keeps it cleaned: nested
/// tables with inline styles, pictures from inline parts, a button, two columns, a footer.
const AUTUMN_SALE_HTML: &str = r#"<div style="margin:0;padding:0;background-color:#f3eee8">
<table role="presentation" style="width:100%;border-spacing:0;background-color:#f3eee8">
<tr><td style="text-align:center;padding:24px 12px">
<table role="presentation" style="width:600px;max-width:100%;margin:0 auto;border-spacing:0;background:#ffffff;border-radius:8px;text-align:left">
<tr><td style="padding:20px 24px;border-bottom:1px solid #ece4da">
<table role="presentation" style="width:100%;border-spacing:0"><tr>
<td style="width:48px;vertical-align:middle;padding:0"><img src="cid:logo@shop.example" width="40" height="40" alt="Shop logo" style="display:block;border-radius:6px"></td>
<td style="vertical-align:middle;font-family:Helvetica,Arial,sans-serif;font-size:20px;font-weight:bold;color:#2b211a;padding:0 0 0 12px">Shop</td>
<td style="text-align:right;vertical-align:middle;font-family:Helvetica,Arial,sans-serif;font-size:12px;color:#7a6a5c;padding:0">Autumn sale &middot; until Sunday</td>
</tr></table>
</td></tr>
<tr><td style="padding:0"><a href="https://shop.example/sale"><img src="cid:hero@shop.example" width="600" height="240" alt="Hills at dusk" style="display:block;width:100%;max-width:600px;height:auto"></a></td></tr>
<tr><td style="padding:24px;font-family:Georgia,'Times New Roman',serif;font-size:16px;line-height:24px;color:#2b2d31">
<h1 style="margin:0 0 12px 0;font-family:Helvetica,Arial,sans-serif;font-size:26px;line-height:32px;color:#b5502a">20% off all jackets</h1>
<p style="margin:0 0 16px 0">Autumn is here, and so are our warmest jackets. Until Sunday every jacket costs a fifth less &ndash; in the shop and online.</p>
<table role="presentation" style="border-spacing:0"><tr><td style="background-color:#b5502a;border-radius:6px;padding:0"><a href="https://shop.example/sale" style="display:inline-block;padding:10px 18px;font-family:Helvetica,Arial,sans-serif;font-size:14px;font-weight:bold;color:#ffffff;text-decoration:none">Shop now</a></td></tr></table>
</td></tr>
<tr><td style="padding:0 24px 24px 24px">
<table role="presentation" style="width:100%;border-spacing:0"><tr>
<td style="width:50%;vertical-align:top;padding:0 8px 0 0">
<img src="cid:rain@shop.example" width="268" alt="Rain jacket" style="display:block;width:100%;height:auto;border-radius:4px">
<h2 style="font-family:Helvetica,Arial,sans-serif;font-size:16px;margin:12px 0 4px 0;color:#18191b">Rain jacket</h2>
<p style="font-family:Helvetica,Arial,sans-serif;font-size:14px;line-height:20px;margin:0;color:#4f535a">89 &euro; <s style="color:#8a8f98">111 &euro;</s></p>
</td>
<td style="width:50%;vertical-align:top;padding:0 0 0 8px">
<img src="cid:down@shop.example" width="268" alt="Down jacket" style="display:block;width:100%;height:auto;border-radius:4px">
<h2 style="font-family:Helvetica,Arial,sans-serif;font-size:16px;margin:12px 0 4px 0;color:#18191b">Down jacket</h2>
<p style="font-family:Helvetica,Arial,sans-serif;font-size:14px;line-height:20px;margin:0;color:#4f535a">159 &euro; <s style="color:#8a8f98">199 &euro;</s></p>
</td>
</tr></table>
</td></tr>
<tr><td style="text-align:center;padding:16px 24px;background:#faf7f3;border-top:1px solid #ece4da;border-radius:0 0 8px 8px;font-family:Helvetica,Arial,sans-serif;font-size:12px;line-height:18px;color:#7a6a5c">
You get this mail because you are a member at shop.example.<br>
<a href="https://shop.example/unsubscribe" style="color:#7a6a5c">Unsubscribe</a> &middot; <a href="mailto:service@shop.example" style="color:#7a6a5c">Write to us</a>
</td></tr>
</table>
</td></tr>
</table>
</div>"#;

/// The HTML part of the demo's newsletter with text and HTML (`m:w14`): short, no pictures.
const WINTER_JACKETS_HTML: &str = r#"<div style="font-family:Arial,sans-serif;max-width:600px;margin:0 auto;padding:8px 0">
<h1 style="font-size:24px;line-height:30px;color:#18191b;margin:0 0 12px 0">Winter jackets are here</h1>
<p style="font-size:15px;line-height:22px;color:#2b2d31;margin:0 0 12px 0">Members get <b>free shipping</b> until Friday.</p>
<p style="margin:0"><a href="https://shop.example/winter" style="color:#3b6ef0">See the collection</a></p>
</div>"#;

/// The HTML part of Lena's answer (`m:w2`), written in Outlook as the core keeps it cleaned:
/// `MsoNormal` paragraphs under a style sheet, empty paragraphs as spacing, a signature in small
/// grey type and the quoted mail under a rule with "From" and "Subject".
const OUTLOOK_REPLY_HTML: &str = r#"<style>
p.MsoNormal, li.MsoNormal, div.MsoNormal { margin:0cm; font-size:11.0pt; font-family:"Aptos",sans-serif; }
a:link, span.MsoHyperlink { color:#467886; text-decoration:underline; }
</style>
<div class="WordSection1" lang="EN-US" style="word-wrap:break-word">
<p class="MsoNormal">Thursday at 14:00 works for me. I will bring the new sidebar drafts:</p>
<ul style="margin-top:0cm">
<li class="MsoNormal">the narrow sidebar with icons only,</li>
<li class="MsoNormal">the mail reader with the <b>new</b> switches.</li>
</ul>
<p class="MsoNormal">&nbsp;</p>
<p class="MsoNormal">Lena</p>
<p class="MsoNormal">&nbsp;</p>
<p class="MsoNormal"><b><span style="font-size:10.0pt;color:#1F3864">Lena Hoffmann</span></b></p>
<p class="MsoNormal"><span style="font-size:9.0pt;color:#595959">Design &middot; Work Example<br><a href="https://work.example.org/"><span style="color:#467886">work.example.org</span></a></span></p>
<p class="MsoNormal">&nbsp;</p>
<div style="border:none;border-top:solid #E1E1E1 1.0pt;padding:3.0pt 0cm 0cm 0cm">
<p class="MsoNormal"><b><span style="font-family:'Calibri',sans-serif">From:</span></b><span style="font-family:'Calibri',sans-serif"> Anna Berg &lt;anna@work.example.org&gt;<br><b>To:</b> Lena Hoffmann &lt;lena.hoffmann@work.example.org&gt;<br><b>Subject:</b> Design review on Thursday</span></p>
</div>
<p class="MsoNormal">&nbsp;</p>
<p class="MsoNormal">Hi Lena,</p>
<p class="MsoNormal">&nbsp;</p>
<p class="MsoNormal">can we move the design review to Thursday?</p>
<p class="MsoNormal">&nbsp;</p>
<p class="MsoNormal">Anna</p>
</div>"#;

const READ: &[Flag] = &[Flag::Seen];
const READ_FLAGGED: &[Flag] = &[Flag::Seen, Flag::Flagged];
const REPLIED: &[Flag] = &[Flag::Seen, Flag::Answered];

/// The messages of both accounts around `now`: unread mail in both inboxes (the entries of
/// [`mail_entries`]), read and flagged mail, attachments, a newsletter made from HTML, a
/// conversation with a sent reply, a draft, and a message in every other folder but the private
/// drafts, junk and trash, which are empty.
pub fn mail_sample(now: DateTime<Utc>) -> Vec<Message> {
    const WORK: usize = 0;
    const PRIVATE: usize = 1;
    let letters = [
        Letter {
            thread: "budget",
            from: ("Jonas Weber", "jonas.weber@work.example.org"),
            minutes: 25,
            text: "Hi Anna,\n\nthe numbers for Q4 are in. Could you look at the travel budget before Thursday? \
                   The details are in the attached sheet and on the intranet:\n\
                   https://intranet.work.example.org/budget/q4\n\nThanks,\nJonas",
            attachments: &[("budget-q4.csv", "text/csv", 2_340)],
            category: Some(MailCategory::by_model(Category::Request, 0.88)),
            ..Letter::new("m:w1", WORK, "inbox", "Budget review for Q4")
        },
        Letter {
            thread: "design",
            from: ("Lena Hoffmann", "lena.hoffmann@work.example.org"),
            minutes: 130,
            flags: &[Flag::Flagged],
            text: "Thursday at 14:00 works for me. I will bring the new sidebar drafts:\n\n\
                   - the narrow sidebar with icons only,\n\
                   - the mail reader with the new switches.\n\nLena\n\n\
                   Lena Hoffmann\nDesign · Work Example\nhttps://work.example.org/\n\n\
                   From: Anna Berg <anna@work.example.org>\nTo: Lena Hoffmann <lena.hoffmann@work.example.org>\n\
                   Subject: Design review on Thursday\n\n> Can we move the design review to Thursday?",
            // Written in Outlook: a text part and an HTML part (task M9.7).
            html: Some(OUTLOOK_REPLY_HTML),
            // A model too unsure to tell (issue #338): such mail stands in "Unsorted", to be looked
            // through on purpose, not under unwanted mail.
            category: Some(MailCategory::by_model(Category::Other, 0.45)),
            ..Letter::new("m:w2", WORK, "inbox", "Re: Design review on Thursday")
        },
        Letter {
            from: ("GitHub", "notifications@github.example"),
            minutes: 200,
            bulk: true,
            text: "New issue #212 in torro/workplace: the sidebar flickers when the window is resized.\n\n\
                   View it on GitHub: https://github.example/torro/workplace/issues/212",
            category: Some(MailCategory::by_model(Category::Task, 0.72)),
            ..Letter::new("m:w3", WORK, "inbox", "[torro/workplace] Sidebar flickers on resize (#212)")
        },
        Letter {
            from: ("Maria Schulz", "maria.schulz@work.example.org"),
            minutes: 26 * 60,
            flags: READ,
            text: "Lunch next week? Tuesday or Wednesday would suit me best.",
            ..Letter::new("m:w4", WORK, "inbox", "Lunch next week?")
        },
        Letter {
            from: ("IT Service Desk", "it@work.example.org"),
            minutes: 2 * 24 * 60,
            flags: READ_FLAGGED,
            text: "On Saturday from 08:00 to 12:00 the mail and file servers are down for maintenance.",
            category: Some(MailCategory::by_model(Category::ServiceNotice, 0.93)),
            pile: Some(MailPile::SetAside),
            ..Letter::new("m:w5", WORK, "inbox", "Planned maintenance on Saturday")
        },
        Letter {
            from: ("Jonas Weber", "jonas.weber@work.example.org"),
            minutes: 3 * 24 * 60,
            flags: READ,
            text: "Here are the minutes from Monday and the slides of every talk. Open points are at the end.",
            // Many files (design review of #290): the reader's block shows two lines of chips and the
            // panel four rows, the rest scrolls inside them.
            attachments: &[
                ("minutes-monday.txt", "text/plain", 1_180),
                ("slides-welcome.pdf", "application/pdf", 412_300),
                ("slides-roadmap-2027.pdf", "application/pdf", 1_820_400),
                ("slides-sidebar-drafts.pdf", "application/pdf", 2_310_900),
                ("slides-mail-reader.pdf", "application/pdf", 1_204_700),
                ("slides-calendar.pdf", "application/pdf", 988_100),
                ("slides-tasks.pdf", "application/pdf", 760_200),
                ("slides-settings.pdf", "application/pdf", 530_800),
                ("slides-accessibility.pdf", "application/pdf", 640_500),
                ("slides-release-plan.pdf", "application/pdf", 301_900),
                ("photo-whiteboard.jpg", "image/jpeg", 2_904_300),
                ("attendees.csv", "text/csv", 1_020),
            ],
            category: Some(MailCategory::by_model(Category::Document, 0.84)),
            ..Letter::new("m:w6", WORK, "inbox", "Minutes from Monday")
        },
        Letter {
            thread: "design",
            minutes: 26 * 60,
            flags: READ,
            text: "Hi Lena,\n\ncan we move the design review to Thursday?\n\nAnna",
            ..Letter::new("m:w7", WORK, "sent", "Design review on Thursday")
        },
        Letter {
            minutes: 4 * 60,
            flags: READ,
            text: "Dear Mr Keller,\n\nthank you for your request. For the relaunch we suggest",
            ..Letter::new("m:w8", WORK, "drafts", "Offer for the website relaunch")
        },
        Letter {
            from: ("Petra Lange", "petra.lange@work.example.org"),
            minutes: 30 * 24 * 60,
            flags: REPLIED,
            text: "Welcome to the team, Anna! Your laptop is ready at the front desk.",
            ..Letter::new("m:w9", WORK, "archive", "Welcome to the team")
        },
        Letter {
            from: ("Jonas Weber", "jonas.weber@work.example.org"),
            minutes: 6 * 24 * 60,
            flags: READ,
            text: "Notes from the kick-off are in the project wiki: https://wiki.work.example.org/relaunch",
            ..Letter::new("m:w10", WORK, "projects", "Kick-off notes")
        },
        Letter {
            from: ("Prize Center", "winner@prizes.example"),
            minutes: 20 * 60,
            text: "You have won! Claim your prize now at http://prizes.example/claim before it expires.",
            category: Some(MailCategory::by_rule(Category::Spam)),
            ..Letter::new("m:w11", WORK, "junk", "You have won a prize")
        },
        Letter {
            from: ("Office news", "news@work.example.org"),
            minutes: 5 * 24 * 60,
            flags: READ,
            bulk: true,
            text: "The canteen is closed on Friday.",
            ..Letter::new("m:w12", WORK, "trash", "Office news, week 41")
        },
        Letter {
            from: ("Paul Berg", "paul@example.org"),
            minutes: 50,
            text: "Are we still on for the hike on Saturday? The weather looks good. I can pick you up at nine.",
            category: Some(MailCategory::by_model(Category::Personal, 0.81)),
            ..Letter::new("m:p1", PRIVATE, "inbox", "Weekend plans")
        },
        Letter {
            from: ("Shop", "news@shop.example"),
            minutes: 5 * 60,
            bulk: true,
            from_html: true,
            html: Some(AUTUMN_SALE_HTML),
            pictures: &["logo@shop.example", "hero@shop.example", "rain@shop.example", "down@shop.example"],
            text: "Autumn sale: 20% off all jackets until Sunday.\n\nShop now: https://shop.example/sale\n\n\
                   Rain jacket: 89 € (was 111 €)\nDown jacket: 159 € (was 199 €)\n\n\
                   Unsubscribe: https://shop.example/unsubscribe",
            category: Some(MailCategory::by_model(Category::Promotion, 0.96)),
            ..Letter::new("m:p2", PRIVATE, "inbox", "Autumn sale: 20% off")
        },
        Letter {
            from: ("Mobile", "billing@mobile.example"),
            minutes: 4 * 24 * 60,
            flags: READ,
            text: "Your invoice for September is attached, with the usage of the month. The amount will be \
                   debited on 15 October.",
            // Two files (issue #290): "Save all" shows from two on; the invoice's own date in its name
            // and the usage without one show how the demo's target "Invoices" files them.
            attachments: &[
                ("invoice-2026-09-30.pdf", "application/pdf", 48_210),
                ("usage-september.pdf", "application/pdf", 96_480),
            ],
            // A guess of the model: "Invoices" is suggested with the sparkles, but a click never
            // saves by it (decision 4 of issue #290).
            category: Some(MailCategory::by_model(Category::Invoice, 0.91)),
            ..Letter::new("m:p3", PRIVATE, "inbox", "Your invoice for September")
        },
        Letter {
            minutes: 3 * 24 * 60,
            flags: READ,
            text: "Thanks, see you on Sunday!",
            ..Letter::new("m:p4", PRIVATE, "sent", "Re: Sunday brunch")
        },
        Letter {
            from: ("Book shop", "orders@books.example"),
            minutes: 10 * 24 * 60,
            flags: READ,
            text: "Thank you for your order #4711. It ships today.",
            ..Letter::new("m:p5", PRIVATE, "receipts", "Order confirmation #4711")
        },
        Letter {
            from: ("Paul Berg", "paul@example.org"),
            minutes: 40 * 24 * 60,
            flags: READ,
            text: "Photos from the summer trip are in the shared album.",
            ..Letter::new("m:p6", PRIVATE, "archive", "Summer photos")
        },
        // Unread mail of yesterday and before (task E3.11): every group of the overview has mail,
        // and the shop writes to both accounts, so hiding its domain shows in both.
        Letter {
            from: ("Tom Richter", "tom.richter@work.example.org"),
            minutes: 20 * 60,
            text: "The rooms for the offsite are booked: two meeting rooms and the terrace. The agenda follows \
                   on Monday.",
            pile: Some(MailPile::ReplyLater),
            ..Letter::new("m:w13", WORK, "inbox", "Offsite: rooms are booked")
        },
        Letter {
            from: ("Shop", "news@shop.example"),
            minutes: 30 * 60,
            bulk: true,
            // A text part and an HTML part, as most newsletters send: `from_html` stays false.
            html: Some(WINTER_JACKETS_HTML),
            text: "Winter jackets are here. Members get free shipping until Friday.",
            ..Letter::new("m:w14", WORK, "inbox", "Winter jackets are here")
        },
        Letter {
            from: ("Clara Berg", "clara@example.org"),
            minutes: 3 * 24 * 60,
            text: "Dinner for Mum’s birthday on the 24th? I would book the Italian place for seven.",
            pile: Some(MailPile::ReplyLater),
            ..Letter::new("m:p7", PRIVATE, "inbox", "Birthday dinner on the 24th")
        },
    ];
    letters.into_iter().map(|l| letter(now, l)).collect()
}

/// The inbox entries the core files for `mail` (task E4.1): one per unread message in an inbox,
/// in "Info" when sent to a list or by a machine, else in "For me"; titled with the subject, with
/// the sender and the start of the text, linking the message.
fn mail_entries(mail: &[Message]) -> Vec<Item> {
    mail.iter()
        .filter(|m| m.summary.folder.ends_with("-inbox") && !m.summary.flags.contains(&Flag::Seen))
        .enumerate()
        .map(|(n, m)| {
            let summary = &m.summary;
            let lane = rules::mail_lane(m.traits.list_id.is_some(), None);
            let from = summary.from.as_ref().map(|from| Sender {
                name: from.name.clone().unwrap_or_else(|| from.address.clone()),
                address: Some(from.address.clone()),
            });
            let entry = Item {
                id: format!("demo-mail-{}", n + 1),
                source: SourceRef::mail(summary.account.trim_start_matches("mail:")),
                thread_key: summary.thread_id.clone(),
                kind: Kind::Message,
                lane,
                urgent: false,
                title: summary.subject.clone(),
                body: Some(summary.snippet.clone()),
                from,
                agent: None,
                received: summary.date,
                updated: summary.date,
                count: 1,
                state: State::New,
                link: Some(Link::Mail { message: summary.id.clone() }),
                actions: Vec::new(),
                untrusted: true,
                facets: Vec::new(),
                task: None,
                invite: None,
                // Sent to a list: the reason it is in "Info" (issue #128).
                reason: m.traits.list_id.is_some().then_some(LaneReason::List),
                // Demo mail gets its categories with E12.8, where the pill is built.
                category: None,
                follow_up: None,
            };
            // The traits the core gives new mail (task E8.6a), its list and bulk from the headers.
            let list = m.traits.list_id.as_deref();
            Item { facets: rules::facets(&entry, list, list.is_some()), ..entry }
        })
        .collect()
}

/// The follow-up on [`FOLLOW_UP_MAIL`] (task E19.3): the entry the core makes for a sent message
/// followed up by its id (decision 8 of E19), set a few minutes after it was sent, back in two days
/// only if nobody replies. `None` if the sample lacks the message.
fn follow_up_entry(mail: &[Message], now: DateTime<Utc>) -> Option<Item> {
    let message = mail.iter().find(|message| message.summary.id == FOLLOW_UP_MAIL)?;
    let mut item = rules::message_stand_in(&message.summary, None, false)?;
    item.id = FOLLOW_UP_ENTRY.to_owned();
    rules::follow_up(&mut item, Some(now + TimeDelta::days(2)), true, message.summary.date + TimeDelta::minutes(5));
    Some(item)
}

/// What a demo attachment holds when it is opened: a few lines that fit its name.
fn attachment_text(name: &str) -> String {
    if name.ends_with(".csv") {
        "item;planned;spent\ntravel;12000;9400\nevents;8000;8150\ntraining;5000;2100\n".into()
    } else {
        format!("{name}\n\nSample attachment of the Torro Inbox demo.\n")
    }
}

/// `message` holds every one of `words` (small letters) in its subject, sender, text or
/// attachment names.
fn matches_words(message: &Message, words: &[String]) -> bool {
    let summary = &message.summary;
    let from =
        summary.from.as_ref().map(|from| format!("{} {}", from.name.as_deref().unwrap_or_default(), from.address));
    let names = message.attachments.iter().filter_map(|a| a.filename.as_deref());
    let haystack = [Some(summary.subject.as_str()), from.as_deref(), message.text.as_deref()]
        .into_iter()
        .flatten()
        .chain(names)
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    words.iter().all(|word| haystack.contains(word.as_str()))
}

impl DemoSource {
    /// Marks the entries of the messages `ids` done, as the core does when a mail was read
    /// elsewhere or left the inbox (task E4.1).
    fn close_entries(data: &mut Data, ids: &[String], now: DateTime<Utc>) {
        for item in &mut data.items {
            if let Some(Link::Mail { message }) = &item.link
                && ids.contains(message)
            {
                rules::set_done(item, true, now);
            }
        }
    }

    /// `message` is hidden from the inbox now (task E18.3): one of the demo's hidden mail, in an
    /// inbox folder, not shown again – as the core counts only mail of the inbox folder.
    fn is_hidden(data: &Data, message: &MessageSummary) -> bool {
        let in_inbox =
            data.folders.iter().any(|folder| folder.id == message.folder && folder.role == Some(Role::Inbox));
        in_inbox && data.hidden_mail.contains(&message.id) && !data.shown_mail.contains(&message.id)
    }

    /// `message` belongs to an entry snoozed now ("Later").
    fn is_snoozed(data: &Data, message: &MessageSummary, now: DateTime<Utc>) -> bool {
        data.items.iter().any(|item| {
            matches!(item.state, State::Snoozed { until } if until > now)
                && matches!(&item.link, Some(Link::Mail { message: linked }) if *linked == message.id)
        })
    }

    /// `mail.search_messages` with `in:<role>` (task E18.3): every message in a folder of `role`
    /// of both accounts (or of `account` only), in `order` by date, all of them – the window's
    /// source pages them. `snoozed` and `hidden` as the request has them: `Some(false)` leaves out
    /// the mail of a snoozed entry ([`DemoSource::is_snoozed`]) resp. the hidden mail
    /// ([`HIDDEN_MAIL`], only in an inbox), `Some(true)` gives only that, `None` both.
    pub fn by_role(
        &self,
        role: Role,
        account: Option<&str>,
        snoozed: Option<bool>,
        hidden: Option<bool>,
        order: Order,
    ) -> Vec<MessageSummary> {
        let now = self.now();
        let data = self.lock();
        let of_role: BTreeSet<&str> =
            data.folders.iter().filter(|folder| folder.role == Some(role)).map(|folder| folder.id.as_str()).collect();
        let mut found: Vec<MessageSummary> = data
            .mail
            .iter()
            .map(|m| &m.summary)
            .filter(|m| of_role.contains(m.folder.as_str()) && account.is_none_or(|account| m.account == account))
            .filter(|m| snoozed.is_none_or(|wanted| Self::is_snoozed(&data, m, now) == wanted))
            .filter(|m| hidden.is_none_or(|wanted| Self::is_hidden(&data, m) == wanted))
            .cloned()
            .collect();
        // Mail of the same minute by id, so oldest first is newest first turned round.
        found.sort_by(|a, b| order.apply(b.date.cmp(&a.date).then_with(|| b.id.cmp(&a.id))));
        found
    }

    /// `inbox.set_mail_shown` (task E18.3, "Show again"): `shown` takes the messages `ids` out of
    /// "Hidden" – remembered only for mail hidden now, as the core does –, `!shown` takes that
    /// back. [`InboxError::NotFound`] when one of them is unknown; nothing changes then.
    pub fn set_mail_shown(&self, ids: &[String], shown: bool) -> Result<(), InboxError> {
        let mut data = self.lock();
        let mut known = Vec::new();
        for id in ids {
            let message = data.mail.iter().find(|m| m.summary.id == *id).ok_or(InboxError::NotFound)?;
            known.push(message.summary.clone());
        }
        for message in known {
            if !shown {
                data.shown_mail.remove(&message.id);
            } else if Self::is_hidden(&data, &message) {
                data.shown_mail.insert(message.id);
            }
        }
        Ok(())
    }

    /// `inbox.set_mail_pile` (task E19.3): puts the messages `ids` on `pile`, moving them from the
    /// other one, or takes them off with `None`. Since when they lie there, as the core keeps it
    /// ([`MessageSummary::pile_since`], decision of #315): `since` (the undo's time, at most now),
    /// else now – the same pile again keeps its time, also with `since`. Put back on "Reply later"
    /// with a `since` before an answer the demo sent in the conversation, a mail comes off at once,
    /// as the core's `take_off_answered`. [`InboxError::NotFound`] when one of them is
    /// unknown; nothing changes then.
    pub fn set_mail_pile(
        &self,
        ids: &[String],
        pile: Option<MailPile>,
        since: Option<DateTime<Utc>>,
    ) -> Result<(), InboxError> {
        let now = self.now();
        let mut data = self.lock();
        if !ids.iter().all(|id| data.mail.iter().any(|m| m.summary.id == *id)) {
            return Err(InboxError::NotFound);
        }
        let answers = data.answers.clone();
        for message in data.mail.iter_mut().map(|m| &mut m.summary).filter(|m| ids.contains(&m.id)) {
            message.pile_since = match (pile, since) {
                (None, _) => None,
                // The same pile keeps its time, whatever `since` says (as the core).
                (Some(_), _) if message.pile == pile => message.pile_since.or(Some(since.unwrap_or(now).min(now))),
                (Some(_), Some(since)) => Some(since.min(now)),
                (Some(_), None) => Some(now),
            };
            message.pile = pile;
            // Back on "Reply later" from before an answer sent since: off at once.
            let answered = |at: DateTime<Utc>| {
                answers.iter().any(|(account, thread, sent)| {
                    *account == message.account && *thread == message.thread_id && *sent > at
                })
            };
            if pile == Some(MailPile::ReplyLater) && since.is_some() && message.pile_since.is_some_and(answered) {
                message.pile = None;
                message.pile_since = None;
            }
        }
        Ok(())
    }

    /// `mail.search_messages` of a pile (task E19.3): every message on `pile` of both accounts (or
    /// of `account` only) in any folder but the trash and spam, in `order` by date, all of them –
    /// the window's source pages them.
    pub fn in_pile(&self, pile: MailPile, account: Option<&str>, order: Order) -> Vec<MessageSummary> {
        let data = self.lock();
        let left_out: BTreeSet<&str> = data
            .folders
            .iter()
            .filter(|folder| matches!(folder.role, Some(Role::Trash | Role::Junk)))
            .map(|folder| folder.id.as_str())
            .collect();
        let mut found: Vec<MessageSummary> = data
            .mail
            .iter()
            .map(|m| &m.summary)
            .filter(|m| m.pile == Some(pile) && !left_out.contains(m.folder.as_str()))
            .filter(|m| account.is_none_or(|account| m.account == account))
            .cloned()
            .collect();
        // As in `by_role`: mail of the same minute by id, so oldest first is newest first turned round.
        found.sort_by(|a, b| order.apply(b.date.cmp(&a.date).then_with(|| b.id.cmp(&a.id))));
        found
    }

    /// `mail.list_folders`: the folders of both accounts with their unread and total numbers.
    pub fn mail_folders(&self) -> Result<Vec<Folder>, InboxError> {
        let data = self.lock();
        Ok(data
            .folders
            .iter()
            .map(|folder| {
                let inside = data.mail.iter().filter(|m| m.summary.folder == folder.id);
                let (total, unread) = inside.fold((0, 0), |(total, unread), m| {
                    (total + 1, unread + u32::from(!m.summary.flags.contains(&Flag::Seen)))
                });
                Folder { unread, total, ..folder.clone() }
            })
            .collect())
    }

    /// How much demo mail each category holds and how much of it is unread (task E12.8):
    /// `(category, unread, all)`, what the window turns into the answer of
    /// `inbox.list_categories`. Tuples, not the wire type – `torro-ipc` depends on this crate,
    /// not the other way round.
    pub fn category_counts(&self) -> Vec<(Category, u32, u32)> {
        self.category_counts_for(None)
    }

    /// Category counts restricted to one mail account.
    pub fn category_counts_for(&self, account: Option<&str>) -> Vec<(Category, u32, u32)> {
        let data = self.lock();
        Category::BUILT_IN
            .into_iter()
            .map(|category| {
                let of_category = data.mail.iter().filter(|m| {
                    account.is_none_or(|account| m.summary.account == account)
                        && m.summary.category.is_some_and(|held| held.category == category)
                });
                let (all, unread) = of_category.fold((0, 0), |(all, unread), m| {
                    (all + 1, unread + u32::from(!m.summary.flags.contains(&Flag::Seen)))
                });
                (category, unread, all)
            })
            .collect()
    }

    /// `mail.search_messages` filtered by `categories` (task E12.8): the demo's mail of one of
    /// them, newest first. Mail on a pile is left out, as the window asks with `pile: without`
    /// (task E19.3).
    pub fn by_category(&self, categories: &[Category], limit: u32, order: Order) -> Vec<MessageSummary> {
        let data = self.lock();
        let mut found: Vec<MessageSummary> = data
            .mail
            .iter()
            .filter(|m| m.summary.pile.is_none())
            .filter(|m| m.summary.category.is_some_and(|held| categories.contains(&held.category)))
            .map(|m| m.summary.clone())
            .collect();
        found.sort_by_key(|m| m.date);
        if order == Order::NewestFirst {
            found.reverse();
        }
        found.truncate(limit as usize);
        found
    }

    /// `inbox.set_item_category` (task E12.8): files the demo's mail `id` by hand.
    pub fn set_mail_category(&self, id: &str, category: Category) -> Result<Item, InboxError> {
        self.put_category(id, MailCategory::by_person(category))
    }

    /// `inbox.classify_item` in the demo: there is no model, so the mail keeps what it has, or
    /// becomes "other" when it has nothing.
    pub fn classified(&self, id: &str) -> Result<Item, InboxError> {
        let held = {
            let data = self.lock();
            data.mail
                .iter()
                .find(|m| m.summary.id == id)
                .and_then(|m| m.summary.category)
                .unwrap_or(MailCategory::by_model(Category::Other, 0.5))
        };
        self.put_category(id, held)
    }

    /// Puts `held` on the demo's mail `id` and on the entry that holds it, and gives that entry
    /// back; [`InboxError::NotFound`] for mail the demo does not have.
    fn put_category(&self, id: &str, held: MailCategory) -> Result<Item, InboxError> {
        let mut data = self.lock();
        let message = data.mail.iter_mut().find(|m| m.summary.id == id).ok_or(InboxError::NotFound)?;
        message.summary.category = Some(held);
        let thread = message.summary.thread_id.clone();
        let entry = data
            .items
            .iter_mut()
            .find(|item| {
                item.thread_key == thread || matches!(&item.link, Some(Link::Mail { message }) if message == id)
            })
            .ok_or(InboxError::NotFound)?;
        entry.category = Some(held);
        Ok(entry.clone())
    }

    /// `mail.search_messages` in the folder `folder`, in `order` by date: those holding every word of
    /// `query` (in subject, sender, text and attachment names, ignoring case), at most `limit` from
    /// `cursor` on; the cursor is the place of the next page. [`InboxError::NotFound`] for an
    /// unknown folder, [`InboxError::Other`] for a cursor the demo did not give out.
    pub fn search_mail(
        &self,
        folder: &str,
        query: Option<&str>,
        limit: u32,
        cursor: Option<&str>,
        order: Order,
    ) -> Result<(Vec<MessageSummary>, Option<String>), InboxError> {
        let data = self.lock();
        if !data.folders.iter().any(|known| known.id == folder) {
            return Err(InboxError::NotFound);
        }
        let start = match cursor {
            None => 0,
            Some(cursor) => cursor.parse::<usize>().map_err(|_| InboxError::Other("not a cursor".into()))?,
        };
        let words: Vec<String> = query.unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
        let mut found: Vec<&Message> =
            data.mail.iter().filter(|m| m.summary.folder == folder && matches_words(m, &words)).collect();
        found.sort_by(|a, b| order.apply(b.summary.date.cmp(&a.summary.date)));
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let page: Vec<MessageSummary> = found.iter().skip(start).take(limit).map(|m| m.summary.clone()).collect();
        let next = (start + page.len() < found.len()).then(|| (start + page.len()).to_string());
        Ok((page, next))
    }

    /// `mail.get_message` with its text and its HTML, as asked with `include_html` (the inbox's
    /// panel asks so, task M9.3); reading does not mark it read. [`InboxError::NotFound`] for an
    /// unknown id.
    pub fn get_mail(&self, id: &str) -> Result<Message, InboxError> {
        self.lock().mail.iter().find(|m| m.summary.id == id).cloned().ok_or(InboxError::NotFound)
    }

    /// `mail.get_attachment`: writes the attachment `part` of the message `id` as a small text
    /// file – a picture of the HTML as its PNG ([`PICTURES`]) – into `torro-inbox-demo` in the
    /// temporary directory, the demo's stand-in for the core's attachment folder, and answers its
    /// path. [`InboxError::NotFound`] for an unknown message or
    /// part, [`InboxError::Other`] when the file cannot be written.
    pub fn mail_attachment(&self, id: &str, part: &str) -> Result<PathBuf, InboxError> {
        let message = self.get_mail(id)?;
        let attachment = message.attachments.iter().find(|a| a.part == part).ok_or(InboxError::NotFound)?;
        // Only the last part of the name, as the core cleans it.
        let name = attachment
            .filename
            .as_deref()
            .and_then(|name| std::path::Path::new(name).file_name())
            .map_or_else(|| format!("attachment-{part}"), |name| name.to_string_lossy().into_owned());
        let dir = std::env::temp_dir().join("torro-inbox-demo");
        let path = dir.join(&name);
        let bytes = match attachment.content_id.as_deref().and_then(picture) {
            Some(bytes) => bytes.to_vec(),
            None => attachment_text(&name).into_bytes(),
        };
        // Written aside and renamed into place: tests (and other checkouts on the machine) place the
        // same file at once, and `write` empties it first, so a reader could find it half written.
        let aside = dir.join(format!(".{name}.{}-{}", std::process::id(), PLACED.fetch_add(1, Ordering::Relaxed)));
        let placed = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&aside, bytes))
            .and_then(|()| std::fs::rename(&aside, &path));
        if let Err(err) = placed {
            let _ = std::fs::remove_file(&aside);
            return Err(InboxError::Other(err.to_string()));
        }
        Ok(path)
    }

    /// `mail.set_flags`: sets `add` and clears `remove` on the messages `ids`. A mail marked read
    /// closes its entry. [`InboxError::NotFound`] when one of them is unknown; nothing changes
    /// then.
    pub fn set_mail_flags(&self, ids: &[String], add: &[Flag], remove: &[Flag]) -> Result<(), InboxError> {
        let now = self.now();
        let mut data = self.lock();
        if !ids.iter().all(|id| data.mail.iter().any(|m| m.summary.id == *id)) {
            return Err(InboxError::NotFound);
        }
        for message in data.mail.iter_mut().filter(|m| ids.contains(&m.summary.id)) {
            let flags = &mut message.summary.flags;
            flags.retain(|flag| !remove.contains(flag));
            for flag in add {
                if !flags.contains(flag) {
                    flags.push(*flag);
                }
            }
            flags.sort();
        }
        if add.contains(&Flag::Seen) {
            Self::close_entries(&mut data, ids, now);
        }
        Ok(())
    }

    /// `mail.move_messages` to the folder `folder`: the messages keep their ids, and those moved
    /// anywhere but an inbox close their entries. [`InboxError::NotFound`] for an unknown message
    /// or folder, [`InboxError::Unsupported`] when they are of several accounts or the folder is of
    /// another one, as in the core; nothing moves then.
    pub fn move_mail(&self, ids: &[String], folder: &str) -> Result<(), InboxError> {
        let now = self.now();
        let mut data = self.lock();
        let target = data.folders.iter().find(|known| known.id == folder).cloned().ok_or(InboxError::NotFound)?;
        let mut accounts = BTreeSet::new();
        for id in ids {
            let message = data.mail.iter().find(|m| m.summary.id == *id).ok_or(InboxError::NotFound)?;
            accounts.insert(message.summary.account.clone());
        }
        if accounts.len() != 1 || !accounts.contains(&target.account) {
            return Err(InboxError::Unsupported);
        }
        for message in data.mail.iter_mut().filter(|m| ids.contains(&m.summary.id)) {
            message.summary.folder.clone_from(&target.id);
        }
        if target.role != Some(Role::Inbox) {
            Self::close_entries(&mut data, ids, now);
        }
        Ok(())
    }

    /// `mail.move_messages` to the folder with `role` in the account of the first of `ids` (the
    /// app moves to the inbox, archive and trash; `mail.delete_messages` is this with
    /// [`Role::Trash`]). Errors as [`DemoSource::move_mail`].
    pub fn move_mail_to(&self, ids: &[String], role: Role) -> Result<(), InboxError> {
        let folder = {
            let data = self.lock();
            let first = ids.first().ok_or(InboxError::NotFound)?;
            let account = data
                .mail
                .iter()
                .find(|m| m.summary.id == *first)
                .map(|m| m.summary.account.clone())
                .ok_or(InboxError::NotFound)?;
            data.folders
                .iter()
                .find(|folder| folder.account == account && folder.role == Some(role))
                .map(|folder| folder.id.clone())
                .ok_or(InboxError::NotFound)?
        };
        self.move_mail(ids, &folder)
    }
}

impl DemoSource {
    /// The accounts a message can be written from, in the order [`MAIL_ACCOUNTS`] lists them: both
    /// are Anna's, the work one with a signature, the private one without.
    pub fn mail_senders(&self) -> Vec<DemoSender> {
        let signature = ["Anna Berg\nProduct, Torro Workplace", ""];
        MAIL_ACCOUNTS
            .iter()
            .zip(signature)
            .map(|(account, signature)| DemoSender {
                account: (*account).to_owned(),
                name: "Anna Berg".to_owned(),
                signature: (!signature.is_empty()).then(|| signature.to_owned()),
            })
            .collect()
    }

    /// `mail.save_draft`: keeps `draft` and answers it with its id (a new one when it had none).
    /// [`InboxError::NotFound`] for an account the demo does not have and for an id it did not give
    /// out, as the core answers for a draft that is not there.
    pub fn save_draft(&self, draft: Draft) -> Result<Draft, InboxError> {
        let mut data = self.lock();
        if !MAIL_ACCOUNTS.contains(&draft.account.as_str()) {
            return Err(InboxError::NotFound);
        }
        match draft.id.clone() {
            Some(id) => {
                let old = data.drafts.iter_mut().find(|old| old.id.as_deref() == Some(id.as_str()));
                *old.ok_or(InboxError::NotFound)? = draft.clone();
                Ok(draft)
            }
            None => {
                data.draft_count += 1;
                let saved = Draft { id: Some(format!("m:draft-{}", data.draft_count)), ..draft };
                data.drafts.push(saved.clone());
                Ok(saved)
            }
        }
    }

    /// `mail.send_draft`: the draft `id` leaves the drafts and is kept as sent.
    /// [`InboxError::NotFound`] for an unknown id, [`InboxError::Other`] for a draft without
    /// recipient (nothing is sent then).
    ///
    /// An answer takes the mail of its conversation off "Reply later" (task E19.3), as the core does
    /// once the account's own mail turns up there (decision 5 of E19); "Set aside" stays.
    pub fn send_draft(&self, id: &str) -> Result<(), InboxError> {
        let now = self.now();
        let mut data = self.lock();
        let at = data.drafts.iter().position(|draft| draft.id.as_deref() == Some(id)).ok_or(InboxError::NotFound)?;
        let draft = &data.drafts[at];
        if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
            return Err(InboxError::Other(tr("The draft has no recipient.").to_owned()));
        }
        let sent = data.drafts.remove(at);
        let answered = sent.in_reply_to.as_deref().and_then(|answered| {
            data.mail.iter().find(|m| m.summary.id == answered).map(|m| m.summary.thread_id.clone())
        });
        if let Some(thread) = answered {
            data.answers.push((sent.account.clone(), thread.clone(), now));
            let replied =
                data.mail.iter_mut().map(|m| &mut m.summary).filter(|m| {
                    m.account == sent.account && m.thread_id == thread && m.pile == Some(MailPile::ReplyLater)
                });
            for message in replied {
                message.pile = None;
                message.pile_since = None;
            }
        }
        data.sent.push(sent);
        Ok(())
    }

    /// `mail.delete_messages` for a draft: it leaves the drafts. [`InboxError::NotFound`] for an
    /// unknown id.
    pub fn discard_draft(&self, id: &str) -> Result<(), InboxError> {
        let mut data = self.lock();
        let before = data.drafts.len();
        data.drafts.retain(|draft| draft.id.as_deref() != Some(id));
        if data.drafts.len() == before { Err(InboxError::NotFound) } else { Ok(()) }
    }

    /// The drafts saved and not sent, in the order they were first saved.
    pub fn drafts(&self) -> Vec<Draft> {
        self.lock().drafts.clone()
    }

    /// The drafts that were sent, in the order they went.
    pub fn sent(&self) -> Vec<Draft> {
        self.lock().sent.clone()
    }
}

impl InboxSource for DemoSource {
    async fn list_items(&self, filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        let now = self.now();
        let mut data = self.lock();
        for item in &mut data.items {
            rules::wake(item, now);
        }
        data.items.retain(|item| !rules::expired(item, now));
        // The filter cannot see the mark on an entry (`ItemFilter::matches`): applied here, as the
        // core does with its column, and only on entries still done.
        let data = &*data;
        let shown = data.items.iter().filter(|item| {
            let dismissed = matches!(item.state, State::Done { .. }) && data.dismissed.contains(&item.id);
            filter.dismissed.is_none_or(|wanted| wanted == dismissed)
        });
        Ok(filter.apply(shown.cloned()))
    }

    async fn list_sources(&self) -> Result<Vec<SourceInfo>, InboxError> {
        let data = self.lock();
        let sources: BTreeSet<&SourceRef> = data.items.iter().map(|item| &item.source).chain(&data.muted).collect();
        Ok(sources
            .into_iter()
            .map(|source| SourceInfo {
                source: source.clone(),
                open: u32::try_from(
                    data.items.iter().filter(|item| item.source == *source && item.state.is_open()).count(),
                )
                .unwrap_or(u32::MAX),
                muted: data.muted.contains(source),
                health: None,
            })
            .collect())
    }

    /// A mail entry marked done also marks its mail read, as the core does (task E4.1).
    async fn set_item_done(&self, id: String, done: bool) -> Result<Item, InboxError> {
        let now = self.now();
        let item = self.change(&id, |item| rules::set_done(item, done, now))?;
        // Done for real or open again: no longer dismissed ([`DemoSource::dismiss_item`] marks it after).
        self.lock().dismissed.remove(&id);
        if let (true, Some(Link::Mail { message })) = (done, &item.link) {
            self.set_mail_flags(std::slice::from_ref(message), &[Flag::Seen], &[])?;
        }
        Ok(item)
    }

    /// Done like [`InboxSource::set_item_done`] and marked dismissed, so "Done" leaves it out
    /// (issue #117); demo mode keeps no statistics.
    async fn dismiss_item(&self, id: String) -> Result<Item, InboxError> {
        let item = self.set_item_done(id.clone(), true).await?;
        self.lock().dismissed.insert(id);
        Ok(item)
    }

    /// As the core does: an entry's conversation and traits; a mail message (`m:…`, issue #123)
    /// stands for its entry, or – without one – offers its traits only ([`Data::mute_subject`]).
    async fn list_mute_options(&self, id: String) -> Result<Vec<MuteOption>, InboxError> {
        let (item, stand_in) = self.lock().mute_subject(&id)?;
        Ok(if stand_in { rules::message_mute_options(&item) } else { rules::mute_options(&item) })
    }

    /// As the core does (task E8.6a): the value from the entry, a rule unless the conversation is
    /// hidden or moved for good, then the entries it holds for changed at once. Demo mode gets no new
    /// reports, so the rule changes nothing later. [`InboxError::Forbidden`] for the person's own
    /// entries is the app key's: the demo is the person, so they are [`InboxError::Other`] here like
    /// a trait the entry lacks.
    async fn mute_item(
        &self,
        id: String,
        scope: MuteScope,
        until: Option<DateTime<Utc>>,
        effect: MuteEffect,
    ) -> Result<Muted, InboxError> {
        let now = self.now();
        let mut data = self.lock();
        let (item, stand_in) = data.mute_subject(&id)?;
        if until.is_some_and(|until| until <= now) {
            return Err(InboxError::Other("the end of muting has passed".into()));
        }
        if stand_in && scope == MuteScope::Conversation {
            return Err(InboxError::Other("a mail without an entry is muted by its traits".into()));
        }
        let id = item.id.clone();
        let target = rules::mute_target(&item, scope)
            .ok_or_else(|| InboxError::Other("this entry has nothing to mute it by".into()))?;
        let source = item.source.clone();
        let (rule, new_rule) = if scope.keeps_rule(effect, until.is_some()) {
            let (rule, new_rule) = data.muting_rule(source, target, effect.rule_effect(), until, now);
            (Some(rule), new_rule)
        } else {
            (None, false)
        };
        let mut muted = Muted { rule: rule.clone(), new_rule, ..Muted::default() };
        for entry in &mut data.items {
            let holds = match &rule {
                Some(rule) => rules::mutes(rule, entry, now),
                None => entry.id == id,
            };
            if !holds {
                continue;
            }
            match rules::mute_entry(entry, effect, now) {
                Some(rules::MutedEntry::Done) => muted.done.push(entry.id.clone()),
                Some(rules::MutedEntry::Moved) => muted.moved.push(entry.id.clone()),
                None => {}
            }
        }
        Ok(muted)
    }

    /// As the core does (task E19.1): a mail entry is followed up ([`rules::follow_up`]), any other
    /// one snoozed, and `unless_reply` with `until` on one that is no mail is
    /// [`InboxError::Unsupported`]. With `keep_open` a mail entry that is not done gets its follow-up
    /// without being put off, a snoozed one opened again ([`rules::follow_up_open`], review of
    /// #315); anything else is [`InboxError::Unsupported`]. A
    /// mail message's id (`m:…`) names its entry, or – with `until` – a new one
    /// ([`Data::snooze_target`]).
    async fn snooze_item(
        &self,
        id: String,
        until: Option<DateTime<Utc>>,
        unless_reply: bool,
        keep_open: bool,
    ) -> Result<Item, InboxError> {
        let now = self.now();
        let mut data = self.lock();
        if keep_open && until.is_none() {
            return Err(InboxError::Unsupported);
        }
        // As the core: a mail filed in an entry of its own stands for that entry (a snoozed one comes
        // back). Only a mail without one, whose conversation's entry is snoozed or done, is refused
        // with `keep_open` – filing it would wake that entry on top or reopen a done one.
        let filed = |item: &Item| matches!(&item.link, Some(Link::Mail { message }) if *message == id);
        let by_mail = rules::is_message_id(&id) && !data.items.iter().any(|item| item.id == id || filed(item));
        let at = match data.snooze_target(&id, until.is_some())? {
            SnoozeTarget::Entry(at) => at,
            SnoozeTarget::StandIn(item) => return Ok(*item),
        };
        let item = &mut data.items[at];
        if keep_open && by_mail && !item.state.is_open() {
            return Err(InboxError::Unsupported);
        }
        if keep_open {
            let done = matches!(item.state, State::Done { .. });
            let (Some(until), true, false) = (until, item.source.kind == SourceKind::Mail, done) else {
                return Err(InboxError::Unsupported);
            };
            rules::follow_up_open(item, until, unless_reply, now);
        } else if item.source.kind == SourceKind::Mail {
            rules::follow_up(item, until, unless_reply, now);
        } else if unless_reply && until.is_some() {
            return Err(InboxError::Unsupported);
        } else {
            rules::snooze(item, until);
        }
        Ok(item.clone())
    }

    /// Demo mode keeps no rules, so `remember` changes nothing beyond the entry.
    async fn set_item_lane(&self, id: String, lane: Lane, _remember: bool) -> Result<Item, InboxError> {
        self.change(&id, |item| rules::set_lane(item, lane))
    }

    async fn mute_source(&self, source: SourceRef, muted: bool) -> Result<(), InboxError> {
        let mut data = self.lock();
        if muted {
            data.muted.insert(source);
        } else {
            data.muted.remove(&source);
        }
        Ok(())
    }

    /// As the core does (tasks E3.10, E3.11): a conversation is done; a sender or domain gets its
    /// rule once – for every mail account, the same rule made again from another account is the one
    /// already there – and the open and snoozed entries it hides ([`rules::hides`]) are done. Unlike
    /// "done", their mail stays as it is – not read, not moved. Demo mode gets no new mail, so the
    /// rule drops nothing later. [`InboxError::NotFound`] for an unknown entry,
    /// [`InboxError::Other`] for one that cannot be hidden that way ([`rules::may_hide`]).
    async fn hide_sender(&self, id: String, scope: HideScope) -> Result<Hidden, InboxError> {
        let now = self.now();
        let mut data = self.lock();
        let item = data.items.iter().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
        if !rules::may_hide(item, scope) {
            return Err(InboxError::Other("this entry names no mail sender to hide".into()));
        }
        let (source, target) = (item.source.clone(), rules::hide_target(item, scope));
        let (rule, new_rule) = match (scope, target) {
            (HideScope::Sender, Some(target)) => data.hiding_rule(source, Some(target), None, now),
            (HideScope::Domain, Some(target)) => data.hiding_rule(source, None, Some(target), now),
            _ => (None, false),
        };
        let mut done = Vec::new();
        for entry in data.items.iter_mut().filter(|entry| !matches!(entry.state, State::Done { .. })) {
            let hidden = match &rule {
                Some(rule) => rules::hides(rule, entry),
                None => entry.id == id,
            };
            if hidden {
                rules::set_done(entry, true, now);
                done.push(entry.id.clone());
            }
        }
        Ok(Hidden { rule, new_rule, done })
    }

    /// Removes a hidden sender or domain or a muted trait (the only rules demo mode keeps); muted sources are taken
    /// back with [`InboxSource::mute_source`]. [`InboxError::NotFound`] for an unknown rule.
    async fn remove_rule(&self, id: String) -> Result<(), InboxError> {
        let mut data = self.lock();
        let before = data.hidden.len();
        data.hidden.retain(|rule| rule.id != id);
        if data.hidden.len() == before { Err(InboxError::NotFound) } else { Ok(()) }
    }

    /// Demo mode has no window manager and no browser behind it: nothing comes to the front (the
    /// app opens pages and apps itself with `--demo`), the entry is only seen, as after opening.
    /// [`InboxError::Unsupported`] for an entry without a link.
    async fn open_item(&self, id: String) -> Result<(), InboxError> {
        let mut data = self.lock();
        let item = data.items.iter_mut().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
        if item.link.is_none() {
            return Err(InboxError::Unsupported);
        }
        if item.state == State::New {
            item.state = State::Seen;
        }
        Ok(())
    }

    /// "Personal" and "Work": the demo has no account behind it.
    async fn list_task_lists(&self) -> Result<Vec<TaskList>, InboxError> {
        Ok(demo_lists())
    }

    /// Keeps the task in memory and marks the entry done, as the core does for the app key.
    /// [`InboxError::NotFound`] for an unknown entry or list; nothing changes then.
    async fn create_task_from_item(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> Result<(), InboxError> {
        if !DEMO_LISTS.contains(&list.as_str()) {
            return Err(InboxError::NotFound);
        }
        let now = self.now();
        let mut data = self.lock();
        let item = data.items.iter_mut().find(|item| item.id == id).ok_or(InboxError::NotFound)?;
        let title = title.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| item.title.clone());
        rules::set_done(item, true, now);
        data.tasks.push(DemoTask { list, title: title.trim().to_owned(), due });
        Ok(())
    }

    /// Demo mode runs nothing: deciding closes the approval's entry, as the core does once the
    /// approval is decided. [`InboxError::Settled`] when its entry is done already,
    /// [`InboxError::NotFound`] when no entry carries the approval.
    async fn decide_approval(&self, approval: String, _allow: bool) -> Result<(), InboxError> {
        let now = self.now();
        let mut data = self.lock();
        let carries = |item: &Item| {
            item.actions.iter().any(|action| match action {
                Action::Allow { approval: id } | Action::Deny { approval: id } => *id == approval,
            })
        };
        let item = data.items.iter_mut().find(|item| carries(item)).ok_or(InboxError::NotFound)?;
        if !item.state.is_open() {
            return Err(InboxError::Settled);
        }
        rules::set_done(item, true, now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::TimeZone;

    use super::*;
    use crate::source::StateFilter;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 13, 10, 30, 0).single().expect("valid time")
    }

    /// Several tests (and the tests of other checkouts) place the same demo picture at once: a
    /// reader never finds the file empty or half written.
    #[test]
    fn a_placed_attachment_is_never_read_half_written() {
        let demo = std::sync::Arc::new(DemoSource::new(now()));
        let mail = demo.get_mail("m:p2").expect("the newsletter");
        let part = mail.attachments.into_iter().find(|a| a.content_id.is_some()).expect("a picture").part;
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let (demo, part) = (std::sync::Arc::clone(&demo), part.clone());
                std::thread::spawn(move || {
                    (0..100).all(|_| {
                        let path = demo.mail_attachment("m:p2", &part).expect("placed");
                        std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"\x89PNG"))
                    })
                })
            })
            .collect();
        assert!(workers.into_iter().all(|worker| worker.join().unwrap_or(false)), "always the whole picture");
    }

    fn open_in(lane: Lane) -> ItemFilter {
        ItemFilter { lane: Some(lane), state: Some(StateFilter::Open), ..ItemFilter::default() }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn the_sample_has_every_kind_of_entry() {
        let items = sample(now());
        let count = |f: &dyn Fn(&Item) -> bool| items.iter().filter(|i| f(i)).count();
        let waiting: HashSet<&str> = items
            .iter()
            .filter(|i| i.kind == Kind::AgentWaiting && i.state.is_open())
            .map(|i| i.source.name.as_str())
            .collect();
        assert_eq!(waiting, HashSet::from(["claude-code", "codex"]));
        assert_eq!(count(&|i| i.kind == Kind::AgentDone && i.state.is_open()), 1);
        assert_eq!(count(&|i| i.kind == Kind::Approval), 1);
        assert_eq!(count(&|i| i.kind == Kind::AccountProblem), 1);
        assert!(count(&|i| i.source.kind == crate::item::SourceKind::Script && i.lane == Lane::Info) >= 2);
        assert_eq!(count(&|i| matches!(i.state, State::Snoozed { .. })), 1);
        assert_eq!(count(&|i| matches!(i.state, State::Done { .. })), 1);
    }

    /// Task E8.13: titles and texts quote typographically – no straight quote or apostrophe in the
    /// entries or the mail, in either language. Task E5.15: GitHub's entries come from a GitHub
    /// source, none from a script that calls itself `github`.
    #[test]
    fn the_sample_quotes_typographically_and_has_an_app_of_its_own() {
        let items = sample(now());
        let mail = mail_sample(now());
        let texts = items
            .iter()
            .flat_map(|i| [Some(i.title.as_str()), i.body.as_deref()])
            .chain(mail.iter().flat_map(|m| [Some(m.summary.subject.as_str()), m.text.as_deref()]))
            .flatten();
        for text in texts {
            assert!(!text.contains(['"', '\'']), "straight quote in {text:?}");
        }
        assert!(!items.iter().any(|i| i.source == SourceRef::script("github")), "no script dressed as GitHub");
        let github = items.iter().filter(|i| i.source.kind == SourceKind::Github).map(|i| i.id.as_str());
        assert_eq!(github.collect::<Vec<_>>(), ["demo-13", "demo-14", "demo-15"], "GitHub's column");
    }

    #[test]
    fn the_sample_follows_the_rules() {
        let items = sample(now());
        let ids: HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), items.len(), "ids are unique");
        let threads: HashSet<(String, &str)> = items.iter().map(|i| (i.source.key(), i.thread_key.as_str())).collect();
        assert_eq!(threads.len(), items.len(), "one entry per matter");
        for item in &items {
            let wanted = (item.kind == Kind::Notice).then_some(item.lane);
            assert_eq!(rules::placement(item.kind, wanted), (item.lane, item.urgent), "{}", item.id);
            let agent_kind = matches!(item.kind, Kind::AgentWaiting | Kind::AgentDone);
            assert_eq!(item.agent.is_some(), agent_kind, "{}", item.id);
            assert!(item.link.is_none() || rules::may_open(item, |app| app == "torroworkplace"), "{}", item.id);
            assert!(item.received <= item.updated && item.updated <= now(), "{}", item.id);
            // Only what the suite writes itself is trusted.
            assert_eq!(item.untrusted, item.kind != Kind::AccountProblem, "{}", item.id);
        }
    }

    #[test]
    fn texts_are_translated() {
        let items = sample(now());
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&tr("Backup finished")));
        let waiting = trf("{agent} is waiting for you", &[("agent", &"Claude Code")]);
        assert!(titles.contains(&waiting.as_str()));
        let approval = items.iter().find(|i| i.kind == Kind::Approval).expect("an approval");
        let labels: Vec<&str> = approval.actions.iter().map(Action::label).collect();
        assert_eq!(labels, [tr("Allow"), tr("Deny")]);
    }

    #[tokio::test]
    async fn for_me_lists_urgent_entries_first() {
        let demo = DemoSource::new(now());
        let for_me = demo.list_items(open_in(Lane::ForMe)).await.expect("list");
        // The ntfy alert (task E5.1) is urgent like a waiting agent.
        let urgent_first = [
            "demo-18",
            "demo-1",
            "demo-3",
            "demo-2",
            "demo-17",
            "demo-14",
            "demo-mail-1",
            "demo-4",
            "demo-15",
            "demo-6",
            "demo-mail-4",
            "demo-16",
            "demo-5",
            "demo-mail-2",
            "demo-mail-6",
            "demo-12",
            "demo-13",
            "demo-mail-8",
        ];
        assert_eq!(ids(&for_me), urgent_first);
        let info = demo.list_items(open_in(Lane::Info)).await.expect("list");
        assert_eq!(ids(&info), ["demo-8", "demo-9", "demo-mail-3", "demo-mail-5", "demo-7", "demo-mail-7"]);
    }

    #[tokio::test]
    async fn done_and_open_again_in_memory() {
        let demo = DemoSource::new(now());
        let done = demo.set_item_done("demo-1".into(), true).await.expect("done");
        assert!(matches!(done.state, State::Done { .. }));
        let open = demo.list_items(open_in(Lane::ForMe)).await.expect("list");
        assert!(!ids(&open).contains(&"demo-1"));
        let done_view = ItemFilter { state: Some(StateFilter::Done), ..ItemFilter::default() };
        assert_eq!(ids(&demo.list_items(done_view).await.expect("list")), ["demo-1", "demo-11"]);
        let back = demo.set_item_done("demo-1".into(), false).await.expect("open again");
        assert_eq!(back.state, State::Seen);
        assert_eq!(ids(&demo.list_items(open_in(Lane::ForMe)).await.expect("list"))[..2], ["demo-18", "demo-1"]);
    }

    #[tokio::test]
    async fn snoozed_entries_wake_when_due() {
        let demo = DemoSource::new(now());
        let snoozed =
            demo.snooze_item("demo-7".into(), Some(now() + TimeDelta::hours(1)), false, false).await.expect("snooze");
        assert_eq!(snoozed.state, State::Snoozed { until: now() + TimeDelta::hours(1) });
        assert!(!ids(&demo.list_items(open_in(Lane::Info)).await.expect("list")).contains(&"demo-7"));
        // Snoozed until a moment that has passed: woken on the next listing, at the top.
        demo.snooze_item("demo-7".into(), Some(now() - TimeDelta::minutes(1)), false, false).await.expect("snooze");
        let info = demo.list_items(open_in(Lane::Info)).await.expect("list");
        assert_eq!(ids(&info)[0], "demo-7");
        assert_eq!(info[0].state, State::New);
        let back = demo.snooze_item("demo-10".into(), None, false, false).await.expect("take back");
        assert_eq!(back.state, State::Seen);
    }

    #[tokio::test]
    async fn lanes_change_in_memory() {
        let demo = DemoSource::new(now());
        let moved = demo.set_item_lane("demo-2".into(), Lane::Info, true).await.expect("move");
        assert_eq!((moved.lane, moved.urgent), (Lane::Info, false));
        assert!(ids(&demo.list_items(open_in(Lane::Info)).await.expect("list")).contains(&"demo-2"));
    }

    #[tokio::test]
    async fn sources_count_open_entries_and_remember_muting() {
        let demo = DemoSource::new(now());
        let sources = demo.list_sources().await.expect("sources");
        let keys: Vec<String> = sources.iter().map(|s| s.source.key()).collect();
        assert_eq!(
            keys,
            [
                "suite",
                "agent:claude-code",
                "agent:codex",
                "script:backup",
                "script:ci",
                "script:deploy",
                "script:invoices",
                "script:uptime",
                "mail:anna@example.org",
                "mail:anna@work.example.org",
                "github:anna-berg",
                "nextcloud:anna@cloud.example.org",
                "talk:anna@cloud.example.org",
                "ntfy:ntfy.sh",
            ]
        );
        let open = |sources: &[SourceInfo], key: &str| {
            sources.iter().find(|s| s.source.key() == key).map(|s| (s.open, s.muted)).expect("listed")
        };
        assert_eq!(open(&sources, "suite"), (2, false));
        assert_eq!(open(&sources, "agent:codex"), (1, false), "the done one does not count");
        assert_eq!(open(&sources, "script:ci"), (1, false), "the snoozed one does not count");

        demo.mute_source(SourceRef::script("backup"), true).await.expect("mute");
        demo.mute_source(SourceRef::script("cron"), true).await.expect("mute");
        let sources = demo.list_sources().await.expect("sources");
        assert_eq!(open(&sources, "script:backup"), (1, true));
        assert_eq!(open(&sources, "script:cron"), (0, true), "muted sources stay listed");
        demo.mute_source(SourceRef::script("cron"), false).await.expect("unmute");
        let sources = demo.list_sources().await.expect("sources");
        assert!(sources.iter().all(|s| s.source.key() != "script:cron"));
    }

    #[tokio::test]
    async fn unknown_entries_are_not_found() {
        let demo = DemoSource::new(now());
        assert_eq!(demo.set_item_done("nope".into(), true).await, Err(InboxError::NotFound));
        assert_eq!(demo.snooze_item("nope".into(), None, false, false).await, Err(InboxError::NotFound));
        assert_eq!(demo.set_item_lane("nope".into(), Lane::Info, false).await, Err(InboxError::NotFound));
        assert_eq!(demo.open_item("nope".into()).await, Err(InboxError::NotFound));
    }

    #[tokio::test]
    async fn opening_marks_a_new_entry_seen_and_needs_a_link() {
        let demo = DemoSource::new(now());
        let state = |items: &[Item], id: &str| items.iter().find(|i| i.id == id).map(|i| i.state).expect("listed");
        demo.open_item("demo-1".into()).await.expect("the agent's window");
        let items = demo.list_items(ItemFilter::default()).await.expect("items");
        assert_eq!(state(&items, "demo-1"), State::Seen);
        assert_eq!(demo.open_item("demo-7".into()).await, Err(InboxError::Unsupported), "no link");
        let items = demo.list_items(ItemFilter::default()).await.expect("items");
        assert_eq!(state(&items, "demo-7"), State::New, "nothing opened, nothing seen");
    }

    #[tokio::test]
    async fn making_a_task_keeps_it_and_marks_the_entry_done() {
        let demo = DemoSource::new(now());
        let lists = demo.list_task_lists().await.expect("lists");
        assert_eq!(lists.iter().map(|list| list.id.as_str()).collect::<Vec<_>>(), DEMO_LISTS);
        assert_eq!(lists[0].to_string(), tr("Personal"), "shown by its name");
        let due = NaiveDate::from_ymd_opt(2026, 10, 14);
        demo.create_task_from_item("demo-6".into(), DEMO_LISTS[1].into(), Some("  ".into()), due).await.expect("made");
        let items = demo.list_items(ItemFilter::default()).await.expect("items");
        let entry = items.iter().find(|i| i.id == "demo-6").expect("still there");
        assert!(matches!(entry.state, State::Done { .. }), "the entry is done");
        let task = DemoTask { list: DEMO_LISTS[1].into(), title: entry.title.clone(), due };
        assert_eq!(demo.lock().tasks, vec![task], "a blank title takes the entry's");
    }

    #[tokio::test]
    async fn a_task_needs_a_known_entry_and_list() {
        let demo = DemoSource::new(now());
        let make = |id: &str, list: &str| demo.create_task_from_item(id.into(), list.into(), None, None);
        assert_eq!(make("nope", DEMO_LISTS[0]).await, Err(InboxError::NotFound));
        assert_eq!(make("demo-6", "demo/elsewhere").await, Err(InboxError::NotFound));
        let items = demo.list_items(ItemFilter::default()).await.expect("items");
        assert!(items.iter().any(|i| i.id == "demo-6" && i.state.is_open()), "nothing changed");
        assert!(demo.lock().tasks.is_empty());
    }

    #[tokio::test]
    async fn hiding_a_sender_closes_its_entries_but_leaves_the_mail_unread() {
        let demo = DemoSource::new(now());
        let sale = demo.lock().items.iter().find(|item| item.title == "Autumn sale: 20% off").cloned().expect("sale");
        let other = demo.lock().items.iter().find(|item| item.title == "Winter jackets are here").cloned();
        let other = other.expect("mail from the same domain in the work account");
        assert_ne!(other.source, sale.source, "two accounts");
        let Some(Link::Mail { message }) = sale.link.clone() else { panic!("a mail entry links its mail") };
        let hidden = demo.hide_sender(sale.id.clone(), HideScope::Domain).await.expect("hidden");
        let rule = hidden.rule.clone().expect("a rule");
        assert_eq!((rule.sender.as_deref(), rule.domain.as_deref()), (None, Some("shop.example")));
        assert_eq!((rule.source.clone(), rule.effect, hidden.new_rule), (sale.source.clone(), RuleEffect::Muted, true));
        assert_eq!(hidden.done, [sale.id.clone(), other.id.clone()], "in every mail account (task E3.11)");
        assert!(matches!(entry_state(&demo, &other.id), State::Done { .. }));
        let mail = demo.lock().mail.iter().find(|m| m.summary.id == message).cloned().expect("the mail");
        assert!(!mail.summary.flags.contains(&Flag::Seen), "not read");
        assert!(mail.summary.folder.ends_with("-inbox"), "not moved");
        let again = demo.hide_sender(other.id.clone(), HideScope::Domain).await.expect("hidden again");
        assert_eq!(again, Hidden { rule: Some(rule.clone()), new_rule: false, done: Vec::new() }, "the same rule");
        let agent = demo.hide_sender("demo-1".into(), HideScope::Sender).await;
        assert!(matches!(agent, Err(InboxError::Other(_))), "no mail: {agent:?}");
        assert_eq!(demo.hide_sender("nope".into(), HideScope::Sender).await, Err(InboxError::NotFound));

        // Undo, as the app does it: the rule goes, the entries open again.
        demo.remove_rule(rule.id.clone()).await.expect("removed");
        assert_eq!(demo.remove_rule(rule.id).await, Err(InboxError::NotFound), "once");
        for id in hidden.done {
            demo.set_item_done(id.clone(), false).await.expect("open again");
            assert!(entry_state(&demo, &id).is_open(), "{id}");
        }
    }

    #[tokio::test]
    async fn hiding_a_conversation_leaves_its_mail_unread_and_keeps_no_rule() {
        let demo = DemoSource::new(now());
        let plans = demo.lock().items.iter().find(|item| item.title == "Weekend plans").cloned().expect("mail");
        let Some(Link::Mail { message }) = plans.link.clone() else { panic!("a mail entry links its mail") };
        let hidden = demo.hide_sender(plans.id.clone(), HideScope::Conversation).await.expect("hidden");
        assert_eq!(hidden, Hidden { rule: None, new_rule: false, done: vec![plans.id.clone()] });
        assert!(matches!(entry_state(&demo, &plans.id), State::Done { .. }));
        let mail = demo.lock().mail.iter().find(|m| m.summary.id == message).cloned().expect("the mail");
        assert!(!mail.summary.flags.contains(&Flag::Seen), "not read, unlike done");
        assert!(demo.lock().hidden.is_empty(), "no rule");
        let twice = demo.hide_sender(plans.id.clone(), HideScope::Conversation).await.expect("hidden again");
        assert!(twice.done.is_empty(), "nothing was open any more");
        let notice = demo.hide_sender("demo-6".into(), HideScope::Conversation).await;
        assert!(matches!(notice, Err(InboxError::Other(_))), "only mail: {notice:?}");
    }

    /// Task E8.6a: the menu is built from the entry's traits, and the person's own entries have none.
    #[tokio::test]
    async fn the_demo_offers_the_traits_of_its_entries() {
        let demo = DemoSource::new(now());
        let waiting = demo.list_mute_options("demo-1".into()).await.expect("options");
        let keys: Vec<MuteScope> = waiting.iter().map(|option| option.scope).collect();
        assert_eq!(
            keys,
            [
                MuteScope::Conversation,
                MuteScope::Facet { key: crate::item::FacetKey::Agent, nth: 0 },
                MuteScope::Facet { key: crate::item::FacetKey::Project, nth: 0 },
                MuteScope::Facet { key: crate::item::FacetKey::Kind, nth: 0 },
            ]
        );
        assert!(demo.list_mute_options("demo-3".into()).await.expect("approval").is_empty());
        assert_eq!(demo.list_mute_options("nope".into()).await, Err(InboxError::NotFound));
        let mail =
            demo.lock().items.iter().find(|item| item.lane == Lane::Info && item.id.starts_with("demo-mail")).cloned();
        let mail = mail.expect("a newsletter");
        let list = mail.facets.iter().find(|facet| facet.key == crate::item::FacetKey::List);
        assert_eq!(list.map(|facet| facet.value.as_str()), Some("<news.example>"), "from List-Id");
    }

    /// Task E8.6a: muting hides or moves at once and answers what to take back.
    #[tokio::test]
    async fn muting_in_the_demo_changes_the_entries_it_holds_for() {
        let demo = DemoSource::new(now());
        let project = MuteScope::Facet { key: crate::item::FacetKey::Kind, nth: 0 };
        let muted = demo.mute_item("demo-1".into(), project, None, MuteEffect::Info).await.expect("muted");
        assert!(muted.new_rule && muted.rule.is_some());
        assert_eq!(muted.moved, ["demo-1", "demo-2"], "both waiting agents");
        assert_eq!(demo.lock().items[0].lane, Lane::Info);
        let again = demo.mute_item("demo-2".into(), project, None, MuteEffect::Info).await.expect("again");
        assert!(!again.new_rule && again.moved.is_empty(), "the same rule, nothing more to move");
        let conversation =
            demo.mute_item("demo-6".into(), MuteScope::Conversation, None, MuteEffect::Hide).await.expect("hidden");
        assert_eq!(conversation, Muted { done: vec!["demo-6".into()], ..Muted::default() }, "no rule");
        let past = demo.mute_item("demo-7".into(), MuteScope::Conversation, Some(now()), MuteEffect::Hide).await;
        assert!(matches!(past, Err(InboxError::Other(_))), "{past:?}");
        let lacking = MuteScope::Facet { key: crate::item::FacetKey::Sender, nth: 0 };
        let lacking = demo.mute_item("demo-7".into(), lacking, None, MuteEffect::Hide).await;
        assert!(matches!(lacking, Err(InboxError::Other(_))), "a script has no sender: {lacking:?}");
        let rule = muted.rule.expect("a rule");
        demo.remove_rule(rule.id).await.expect("removed like any rule");
        let dismissed = demo.dismiss_item("demo-8".into()).await.expect("dismissed");
        assert!(matches!(dismissed.state, State::Done { .. }));
    }

    /// Issue #117: a dismissed entry is neither open nor in the done entries that leave the
    /// dismissed out; opened again it is an entry like any other.
    #[tokio::test]
    async fn dismissed_entries_leave_the_done_list_that_asks_so() {
        let demo = DemoSource::new(now());
        let ids = |items: Vec<Item>| items.into_iter().map(|item| item.id).collect::<Vec<_>>();
        let list = |state, dismissed| ItemFilter { state: Some(state), dismissed, ..ItemFilter::default() };
        demo.dismiss_item("demo-8".into()).await.expect("dismissed");
        let open = ids(demo.list_items(list(StateFilter::Open, None)).await.expect("listed"));
        assert!(!open.contains(&"demo-8".to_owned()));
        let real = ids(demo.list_items(list(StateFilter::Done, Some(false))).await.expect("listed"));
        assert!(!real.contains(&"demo-8".to_owned()), "{real:?}");
        let gone = ids(demo.list_items(list(StateFilter::Done, Some(true))).await.expect("listed"));
        assert_eq!(gone, ["demo-8"]);

        demo.set_item_done("demo-8".into(), false).await.expect("opened again");
        let open = ids(demo.list_items(list(StateFilter::Open, Some(false))).await.expect("listed"));
        assert!(open.contains(&"demo-8".to_owned()));
        assert!(demo.list_items(list(StateFilter::Done, Some(true))).await.expect("listed").is_empty());
    }

    /// Task E5.1: one entry of each remote source, placed and given its traits by the core's rules.
    #[test]
    fn the_sample_has_an_entry_of_each_remote_source() {
        let items = remote_sample(now());
        let of = |id: &str| items.iter().find(|item| item.id == id).expect("in the sample");
        let keys = |item: &Item| item.facets.iter().map(|facet| facet.key).collect::<Vec<_>>();
        let review = of("demo-14");
        assert_eq!((review.source.kind, review.kind, review.lane), (SourceKind::Github, Kind::Notice, Lane::ForMe));
        assert_eq!(keys(review), [FacetKey::Kind, FacetKey::Repo, FacetKey::Reason]);
        let run = of("demo-15");
        assert_eq!(run.lane, Lane::ForMe, "a failed run of the person's");
        assert!(run.facets.contains(&Facet { key: FacetKey::Result, value: "failed".into() }));
        let shared = of("demo-16");
        assert_eq!((shared.source.kind, shared.lane), (SourceKind::Nextcloud, Lane::ForMe));
        assert_eq!(keys(shared), [FacetKey::App, FacetKey::Person]);
        let chat = of("demo-17");
        assert_eq!((chat.source.kind, chat.kind, chat.lane), (SourceKind::Talk, Kind::Message, Lane::ForMe));
        assert_eq!(chat.facets[0], Facet { key: FacetKey::Kind, value: "direct".into() });
        let alert = of("demo-18");
        assert_eq!((alert.source.key().as_str(), alert.kind), ("ntfy:ntfy.sh", Kind::Alert));
        assert!(alert.urgent && alert.lane == Lane::ForMe, "priority 5 is urgent");
        assert_eq!(keys(alert), [FacetKey::Topic, FacetKey::Priority, FacetKey::Tag, FacetKey::Tag]);
        assert_eq!(alert.title, "Backup failed");
        assert!(items.iter().all(|item| item.untrusted), "foreign text");
        let old = of("demo-13");
        assert_eq!((old.source.kind, old.state, old.lane), (SourceKind::Github, State::Seen, Lane::ForMe));
        assert!(items.iter().filter(|item| item.id != "demo-13").all(|item| item.state == State::New), "the rest new");
        // Talk and Nextcloud share the account, Talk with its own key.
        assert_eq!(shared.source.name, chat.source.name);
        assert_eq!(chat.source.key(), "talk:anna@cloud.example.org");
    }

    /// Task E5.1: every tag of the alert is offered on its own, by its place among the tags.
    #[tokio::test]
    async fn each_tag_is_muted_on_its_own() {
        let demo = DemoSource::new(now());
        let options = demo.list_mute_options("demo-18".into()).await.expect("options");
        let tags: Vec<(MuteScope, Option<&str>)> = options
            .iter()
            .filter(|option| matches!(option.scope, MuteScope::Facet { key: FacetKey::Tag, .. }))
            .map(|option| (option.scope, option.value.as_deref()))
            .collect();
        let tag = |nth| MuteScope::Facet { key: FacetKey::Tag, nth };
        assert_eq!(tags, [(tag(0), Some("warning")), (tag(1), Some("backup"))]);
        let muted = demo.mute_item("demo-18".into(), tag(1), None, MuteEffect::NoPopup).await.expect("muted");
        let facet = muted.rule.and_then(|rule| rule.facet).expect("a rule with the trait");
        assert_eq!(facet, Facet { key: FacetKey::Tag, value: "backup".into() }, "the second tag");
    }

    /// Issue #123: a folder's message without an entry is muted by its traits; one filed in an entry
    /// stands for that entry.
    #[tokio::test]
    async fn a_message_is_muted_by_its_traits_or_through_its_entry() {
        let demo = DemoSource::new(now());
        let filed = |message: &str| {
            let data = demo.lock();
            data.items.iter().any(|item| matches!(&item.link, Some(Link::Mail { message: m }) if m == message))
        };
        assert!(!filed("m:w6"), "the minutes were read and have no entry");
        let options = demo.list_mute_options("m:w6".into()).await.expect("options of a message");
        assert!(options.iter().all(|option| option.scope != MuteScope::Conversation), "{options:?}");
        let sender = MuteScope::Facet { key: FacetKey::Sender, nth: 0 };
        assert!(options.iter().any(|option| option.scope == sender));
        let conversation = demo.mute_item("m:w6".into(), MuteScope::Conversation, None, MuteEffect::Hide).await;
        assert!(matches!(conversation, Err(InboxError::Other(_))), "no entry to close: {conversation:?}");
        let muted = demo.mute_item("m:w6".into(), sender, None, MuteEffect::Info).await.expect("muted");
        let facet = muted.rule.and_then(|rule| rule.facet).expect("a rule with the trait");
        assert_eq!(facet, Facet { key: FacetKey::Sender, value: "jonas.weber@work.example.org".into() });
        assert_eq!(demo.list_mute_options("m:nope".into()).await, Err(InboxError::NotFound));
        assert!(filed("m:w1"), "the budget mail is an entry");
        let through = demo.list_mute_options("m:w1".into()).await.expect("its entry's options");
        assert_eq!(through.first().map(|option| option.scope), Some(MuteScope::Conversation), "the entry's own");
    }

    fn folder<'a>(folders: &'a [Folder], id: &str) -> &'a Folder {
        folders.iter().find(|folder| folder.id == id).expect("a demo folder")
    }

    fn entry_state(demo: &DemoSource, id: &str) -> State {
        demo.lock().items.iter().find(|item| item.id == id).map(|item| item.state).expect("an entry")
    }

    #[test]
    fn two_mail_accounts_have_their_folders_and_unread_mail() {
        let demo = DemoSource::new(now());
        let folders = demo.mail_folders().expect("folders");
        for account in MAIL_ACCOUNTS {
            let roles: Vec<Option<Role>> =
                folders.iter().filter(|folder| folder.account == account).map(|folder| folder.role).collect();
            assert_eq!(roles.len(), 7, "six roles and one folder of its own");
            assert_eq!(roles.iter().filter(|role| role.is_none()).count(), 1);
        }
        let inbox = folder(&folders, "f:work-inbox");
        assert_eq!((inbox.unread, inbox.total), (5, 8));
        assert_eq!(folder(&folders, "f:private-trash").total, 0, "an empty folder");
        let mail = mail_sample(now());
        assert!(mail.iter().all(|m| m.summary.untrusted), "mail is foreign text");
        assert!(mail.iter().any(|m| m.summary.flags.contains(&Flag::Flagged)));
        assert!(mail.iter().any(|m| m.from_html && m.html.is_some()), "a newsletter made from HTML, with it");
        assert!(mail.iter().any(|m| !m.from_html && m.html.is_some()), "a mail with a text and an HTML part");
        assert!(mail.iter().any(|m| m.html.is_none()), "mail without HTML");
        assert!(mail.iter().any(|m| !m.attachments.is_empty()));
        let ids: HashSet<&str> = mail.iter().map(|m| m.summary.id.as_str()).collect();
        assert_eq!(ids.len(), mail.len(), "ids are unique");
    }

    /// The newsletter's pictures are inline parts with a Content-ID (task M9.7): they do not count as
    /// attachments, and `mail.get_attachment` hands out the PNG its HTML names.
    #[test]
    fn the_newsletter_carries_its_pictures_as_inline_parts() {
        let demo = DemoSource::new(now());
        let sale = demo.get_mail("m:p2").expect("the newsletter");
        let html = sale.html.as_deref().expect("its HTML");
        assert!(!sale.summary.has_attachments, "pictures are no attachments");
        let pictures: Vec<&AttachmentInfo> = sale.attachments.iter().filter(|a| a.inline).collect();
        assert_eq!(pictures.len(), 4);
        for part in pictures {
            let id = part.content_id.as_deref().expect("a Content-ID");
            assert!(html.contains(&format!("cid:{id}")), "the HTML shows {id}");
            let path = demo.mail_attachment("m:p2", &part.part).expect("placed");
            let bytes = std::fs::read(path).expect("written");
            assert!(bytes.starts_with(b"\x89PNG"), "{id} is a PNG");
        }
    }

    #[test]
    fn unread_inbox_mail_is_filed_as_the_core_files_it() {
        let entries = mail_entries(&mail_sample(now()));
        assert_eq!(entries.len(), 8, "every unread mail in an inbox, nothing else");
        for item in &entries {
            assert_eq!((item.kind, item.urgent, item.untrusted), (Kind::Message, false, true), "{}", item.id);
            assert!(rules::may_open(item, |_| false), "a mail entry opens its mail: {}", item.id);
            assert_eq!(item.source.kind, crate::item::SourceKind::Mail);
        }
        let info: Vec<&str> =
            entries.iter().filter(|item| item.lane == Lane::Info).map(|item| item.title.as_str()).collect();
        assert_eq!(
            info,
            ["[torro/workplace] Sidebar flickers on resize (#212)", "Autumn sale: 20% off", "Winter jackets are here"]
        );
    }

    #[test]
    fn folders_are_searched_a_page_at_a_time() {
        let demo = DemoSource::new(now());
        let newest = Order::NewestFirst;
        let (first, next) = demo.search_mail("f:work-inbox", None, 4, None, newest).expect("a page");
        let ids: Vec<&str> = first.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["m:w1", "m:w2", "m:w3", "m:w13"], "newest first");
        let (rest, last) = demo.search_mail("f:work-inbox", None, 4, next.as_deref(), newest).expect("the next page");
        assert_eq!((rest.len(), last), (4, None));
        let (found, _) = demo.search_mail("f:work-inbox", Some("JONAS minutes"), 50, None, newest).expect("found");
        assert_eq!(found.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["m:w6"], "every word, any case");
        let (by_file, _) = demo.search_mail("f:work-inbox", Some("budget-q4.csv"), 50, None, newest).expect("found");
        assert_eq!(by_file.len(), 1, "attachment names count");
        assert_eq!(demo.search_mail("f:nope", None, 50, None, newest), Err(InboxError::NotFound));

        // Task E8.15: oldest first, a page at a time as well – the reverse of newest first.
        let all = |order| demo.search_mail("f:work-inbox", None, 50, None, order).expect("all").0;
        let mut reversed = all(newest);
        reversed.reverse();
        let (first, next) = demo.search_mail("f:work-inbox", None, 4, None, Order::OldestFirst).expect("a page");
        assert_eq!(first, reversed[..4]);
        let (rest, _) = demo.search_mail("f:work-inbox", None, 4, next.as_deref(), Order::OldestFirst).expect("more");
        assert_eq!(rest, reversed[4..]);
    }

    #[tokio::test]
    async fn reading_or_moving_a_mail_closes_its_entry() {
        let demo = DemoSource::new(now());
        // demo-mail-1 is m:w1, demo-mail-2 m:w2, demo-mail-4 m:p1.
        demo.set_mail_flags(&["m:w1".into()], &[Flag::Seen, Flag::Flagged], &[]).expect("read");
        assert!(matches!(entry_state(&demo, "demo-mail-1"), State::Done { .. }), "read elsewhere: done");
        assert_eq!(demo.get_mail("m:w1").expect("there").summary.flags, [Flag::Seen, Flag::Flagged]);
        demo.set_mail_flags(&["m:w1".into()], &[], &[Flag::Seen]).expect("unread again");
        assert_eq!(demo.get_mail("m:w1").expect("there").summary.flags, [Flag::Flagged]);

        demo.move_mail_to(&["m:w2".into()], Role::Archive).expect("archived");
        assert_eq!(demo.get_mail("m:w2").expect("same id").summary.folder, "f:work-archive");
        assert!(matches!(entry_state(&demo, "demo-mail-2"), State::Done { .. }), "left the inbox: done");
        demo.move_mail(&["m:w2".into()], "f:work-inbox").expect("moved back");
        assert_eq!(demo.get_mail("m:w2").expect("same id").summary.folder, "f:work-inbox");

        demo.set_item_done("demo-mail-4".into(), true).await.expect("done");
        assert!(demo.get_mail("m:p1").expect("there").summary.flags.contains(&Flag::Seen), "done marks it read");
    }

    fn role_ids(demo: &DemoSource, role: Role, snoozed: Option<bool>, hidden: Option<bool>) -> Vec<String> {
        demo.by_role(role, None, snoozed, hidden, Order::NewestFirst).into_iter().map(|m| m.id).collect()
    }

    /// Task E18.3: "Archive" lists the archived mail of both accounts, "All" leaves the hidden
    /// newsletter out and "Hidden" gives only it; "Show again" turns that round and its undo back.
    #[test]
    fn mail_is_listed_by_role_without_or_with_the_hidden_newsletter() {
        let demo = DemoSource::new(now());
        assert_eq!(role_ids(&demo, Role::Archive, None, None), ["m:w9", "m:p6"], "both accounts, newest first");
        assert_eq!(role_ids(&demo, Role::Archive, None, Some(true)), Vec::<String>::new(), "only inbox mail hides");
        let all = role_ids(&demo, Role::Inbox, Some(false), Some(false));
        assert!(!all.iter().any(|id| id == HIDDEN_MAIL), "the newsletter is hidden");
        assert!(all.iter().any(|id| id == "m:w1") && all.iter().any(|id| id == "m:p1"), "both inboxes");
        assert_eq!(role_ids(&demo, Role::Inbox, None, Some(true)), [HIDDEN_MAIL]);
        assert_eq!(role_ids(&demo, Role::Inbox, None, None).len(), all.len() + 1, "without the field: every one");
        let hidden = demo.get_mail(HIDDEN_MAIL).expect("the newsletter");
        assert!(hidden.traits.list_id.is_some(), "a newsletter: {}", hidden.summary.subject);
        let private = demo.by_role(Role::Inbox, Some(MAIL_ACCOUNTS[1]), Some(false), Some(false), Order::NewestFirst);
        assert!(private.iter().all(|m| m.account == MAIL_ACCOUNTS[1]) && !private.is_empty(), "one account");
        let oldest = demo.by_role(Role::Inbox, None, Some(false), Some(false), Order::OldestFirst);
        assert_eq!(oldest.into_iter().rev().map(|m| m.id).collect::<Vec<_>>(), all, "oldest first");

        demo.set_mail_shown(&[HIDDEN_MAIL.into()], true).expect("shown again");
        assert!(role_ids(&demo, Role::Inbox, Some(false), Some(false)).iter().any(|id| id == HIDDEN_MAIL));
        assert_eq!(role_ids(&demo, Role::Inbox, None, Some(true)), Vec::<String>::new(), "nothing hidden");
        demo.set_mail_shown(&[HIDDEN_MAIL.into()], false).expect("undone");
        assert_eq!(role_ids(&demo, Role::Inbox, Some(false), Some(false)), all);
        assert_eq!(role_ids(&demo, Role::Inbox, None, Some(true)), [HIDDEN_MAIL]);
    }

    /// As the core: showing mail that is not hidden remembers nothing, and an unknown id changes
    /// nothing at all.
    #[test]
    fn only_hidden_mail_is_shown_again_and_unknown_mail_is_not_found() {
        let demo = DemoSource::new(now());
        demo.set_mail_shown(&["m:w1".into()], true).expect("nothing to remember");
        assert!(demo.lock().shown_mail.is_empty());
        let both = [HIDDEN_MAIL.to_owned(), "m:nope".to_owned()];
        assert_eq!(demo.set_mail_shown(&both, true), Err(InboxError::NotFound));
        assert_eq!(role_ids(&demo, Role::Inbox, None, Some(true)), [HIDDEN_MAIL], "still hidden");
        // Moved out of the inbox, the newsletter is hidden no more; back in, it is again.
        demo.move_mail_to(&[HIDDEN_MAIL.into()], Role::Archive).expect("archived");
        assert!(role_ids(&demo, Role::Archive, None, Some(false)).iter().any(|id| id == HIDDEN_MAIL));
        demo.move_mail_to(&[HIDDEN_MAIL.into()], Role::Inbox).expect("back");
        assert_eq!(role_ids(&demo, Role::Inbox, None, Some(true)), [HIDDEN_MAIL]);
    }

    /// The mail of a snoozed entry is "Later", not "All".
    #[tokio::test]
    async fn the_mail_of_a_snoozed_entry_is_snoozed() {
        let demo = DemoSource::new(now());
        let entry = demo
            .lock()
            .items
            .iter()
            .find(|item| matches!(&item.link, Some(Link::Mail { message }) if message == "m:w1"))
            .map(|item| item.id.clone())
            .expect("the entry of m:w1");
        assert_eq!(role_ids(&demo, Role::Inbox, Some(true), None), Vec::<String>::new(), "nothing snoozed");
        demo.snooze_item(entry, Some(now() + TimeDelta::days(1)), false, false).await.expect("snoozed");
        assert_eq!(role_ids(&demo, Role::Inbox, Some(true), None), ["m:w1"]);
        assert!(!role_ids(&demo, Role::Inbox, Some(false), Some(false)).iter().any(|id| id == "m:w1"), "not in All");
    }

    #[test]
    fn mail_moves_only_within_its_account() {
        let demo = DemoSource::new(now());
        assert_eq!(demo.move_mail(&["m:w1".into()], "f:private-archive"), Err(InboxError::Unsupported));
        assert_eq!(demo.move_mail(&["m:w1".into(), "m:p1".into()], "f:work-archive"), Err(InboxError::Unsupported));
        assert_eq!(demo.move_mail(&["m:nope".into()], "f:work-archive"), Err(InboxError::NotFound));
        assert_eq!(demo.get_mail("m:w1").expect("there").summary.folder, "f:work-inbox", "nothing moved");
        demo.move_mail_to(&["m:p1".into()], Role::Trash).expect("deleted");
        assert_eq!(demo.get_mail("m:p1").expect("in the trash").summary.folder, "f:private-trash");
    }

    /// Issue #290: the demo's two save targets are valid ones – absolute folders, schemes that
    /// resolve, a condition each – and its invoice has two PDFs and a guessed category.
    #[test]
    fn the_demo_has_save_targets_and_an_invoice_with_two_files() {
        let targets = save_targets();
        assert_eq!(targets.iter().map(|t| t.spec.name.as_str()).collect::<Vec<_>>(), ["Invoices", "Contracts"]);
        let invoice = DemoSource::new(now()).get_mail("m:p3").expect("the invoice");
        let files: Vec<&AttachmentInfo> = invoice.attachments.iter().filter(|a| !a.inline).collect();
        assert_eq!(files.len(), 2, "two files, so the reader offers \"Save all\"");
        assert!(files.iter().all(|a| a.content_type == "application/pdf"));
        assert!(invoice.summary.category.is_some_and(|c| c.category == Category::Invoice && c.is_suggestion()));
        let facts = torro_mail::FileFacts {
            mail_date: NaiveDate::from_ymd_opt(2026, 10, 9).expect("a day"),
            file_name: files[0].filename.clone(),
            content_type: files[0].content_type.clone(),
            sender_name: Some("Mobile".into()),
            sender_address: "billing@mobile.example".into(),
            account: "anna@example.org".into(),
            subject: invoice.summary.subject.clone(),
            month_names: std::array::from_fn(|i| format!("M{i}")),
        };
        for target in &targets {
            assert!(target.spec.folder.starts_with('/'), "{}: absolute", target.spec.name);
            assert!(torro_mail::save_target::resolve(&target.spec, &facts).is_ok(), "{}", target.spec.name);
            assert!(!target.spec.suggest.categories.is_empty(), "{}: suggested for something", target.spec.name);
        }
        let resolved = torro_mail::save_target::resolve(&targets[0].spec, &facts).expect("resolves");
        assert_eq!(resolved.folders, ["2026", "09"], "the invoice by the date in its name");
    }

    #[test]
    fn an_attachment_lands_in_a_file() {
        let demo = DemoSource::new(now());
        let path = demo.mail_attachment("m:w1", "2").expect("written");
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("budget-q4.csv"));
        assert!(std::fs::read_to_string(&path).expect("readable").starts_with("item;"));
        assert_eq!(demo.mail_attachment("m:w1", "9"), Err(InboxError::NotFound));
        assert_eq!(demo.mail_attachment("m:w4", "2"), Err(InboxError::NotFound), "no attachments");
    }

    #[tokio::test]
    async fn deciding_closes_the_approval_once() {
        let demo = DemoSource::new(now());
        assert_eq!(demo.decide_approval(APPROVAL_ID.into(), true).await, Ok(()));
        let items = demo.list_items(ItemFilter::default()).await.expect("items");
        let entry = items.iter().find(|item| item.thread_key == format!("approval:{APPROVAL_ID}")).expect("entry");
        assert!(matches!(entry.state, State::Done { .. }), "decided, the entry is done");
        assert_eq!(demo.decide_approval(APPROVAL_ID.into(), false).await, Err(InboxError::Settled));
        assert_eq!(demo.decide_approval("nope".into(), true).await, Err(InboxError::NotFound));
    }

    fn draft(account: &str, to: &[&str]) -> Draft {
        Draft {
            html: None,
            id: None,
            account: account.into(),
            to: to.iter().map(|to| address("", to)).collect(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Lunch".into(),
            body: "Shall we?".into(),
            in_reply_to: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn the_demo_lists_both_accounts_to_write_from() {
        let senders = DemoSource::new(now()).mail_senders();
        let accounts: Vec<&str> = senders.iter().map(|sender| sender.account.as_str()).collect();
        assert_eq!(accounts, MAIL_ACCOUNTS);
        assert!(senders[0].signature.is_some() && senders[1].signature.is_none(), "one signature, one without");
    }

    #[test]
    fn a_saved_draft_gets_an_id_and_is_replaced_by_it() {
        let demo = DemoSource::new(now());
        let first = demo.save_draft(draft(MAIL_ACCOUNTS[1], &["bob@example.org"])).expect("saved");
        assert_eq!(first.id.as_deref(), Some("m:draft-1"));
        let changed = Draft { subject: "Lunch tomorrow".into(), ..first.clone() };
        assert_eq!(demo.save_draft(changed).expect("replaced").id, first.id);
        let kept = demo.drafts();
        assert_eq!((kept.len(), kept[0].subject.as_str()), (1, "Lunch tomorrow"), "replaced, not added");
        let second = demo.save_draft(draft(MAIL_ACCOUNTS[0], &[])).expect("a second one");
        assert_eq!(second.id.as_deref(), Some("m:draft-2"));
        assert_eq!(demo.drafts().len(), 2);
    }

    #[test]
    fn the_demo_refuses_what_the_core_refuses() {
        let demo = DemoSource::new(now());
        assert_eq!(
            demo.save_draft(draft("mail:nobody@example.org", &[])),
            Err(InboxError::NotFound),
            "no such account"
        );
        let unknown = Draft { id: Some("m:draft-9".into()), ..draft(MAIL_ACCOUNTS[0], &[]) };
        assert_eq!(demo.save_draft(unknown), Err(InboxError::NotFound), "no such draft");
        assert_eq!(demo.send_draft("m:draft-9"), Err(InboxError::NotFound));
        let empty = demo.save_draft(draft(MAIL_ACCOUNTS[0], &[])).expect("saved");
        let id = empty.id.expect("an id");
        assert_eq!(demo.send_draft(&id), Err(InboxError::Other(tr("The draft has no recipient.").into())));
        assert_eq!(demo.drafts().len(), 1, "a refused send keeps the draft");
        assert_eq!(demo.discard_draft("m:draft-9"), Err(InboxError::NotFound));
    }

    #[test]
    fn a_sent_draft_leaves_the_drafts() {
        let demo = DemoSource::new(now());
        let saved = demo.save_draft(draft(MAIL_ACCOUNTS[1], &["bob@example.org"])).expect("saved");
        let id = saved.id.expect("an id");
        demo.send_draft(&id).expect("sent");
        assert!(demo.drafts().is_empty());
        assert_eq!(demo.sent().len(), 1);
        assert_eq!(demo.send_draft(&id), Err(InboxError::NotFound), "never twice");
        let other = demo.save_draft(draft(MAIL_ACCOUNTS[1], &["bob@example.org"])).expect("saved");
        demo.discard_draft(&other.id.expect("an id")).expect("discarded");
        assert!(demo.drafts().is_empty() && demo.sent().len() == 1);
    }

    fn pile_ids(demo: &DemoSource, pile: MailPile, account: Option<&str>) -> Vec<String> {
        demo.in_pile(pile, account, Order::NewestFirst).into_iter().map(|m| m.id).collect()
    }

    /// Task E19.3: two messages lie on "Reply later" and one on "Set aside" from the start, each
    /// naming its pile wherever it is listed; a pile is listed per account and leaves out the trash.
    #[test]
    fn the_demo_has_mail_on_both_piles() {
        let demo = DemoSource::new(now());
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, None), ["m:w13", "m:p7"], "newest first");
        assert_eq!(pile_ids(&demo, MailPile::SetAside, None), [SET_ASIDE_MAIL]);
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, Some(MAIL_ACCOUNTS[1])), ["m:p7"], "one account");
        let oldest: Vec<String> =
            demo.in_pile(MailPile::ReplyLater, None, Order::OldestFirst).into_iter().map(|m| m.id).collect();
        assert_eq!(oldest, ["m:p7", "m:w13"]);
        for id in REPLY_LATER_MAIL {
            assert_eq!(demo.get_mail(id).expect("the mail").summary.pile, Some(MailPile::ReplyLater), "{id}");
        }
        let inbox = demo.by_role(Role::Inbox, None, None, None, Order::NewestFirst);
        let set_aside = inbox.iter().find(|m| m.id == SET_ASIDE_MAIL).expect("still in its folder");
        assert_eq!(set_aside.pile, Some(MailPile::SetAside), "the summary names it");
        // In the trash a message is on no pile's list, out of it again it is.
        demo.move_mail_to(&[SET_ASIDE_MAIL.into()], Role::Trash).expect("deleted");
        assert!(pile_ids(&demo, MailPile::SetAside, None).is_empty());
        demo.move_mail_to(&[SET_ASIDE_MAIL.into()], Role::Inbox).expect("back");
        assert_eq!(pile_ids(&demo, MailPile::SetAside, None), [SET_ASIDE_MAIL]);
        // The categories leave pile mail out, as the window asks them.
        let notices = demo.by_category(&[Category::ServiceNotice], 50, Order::NewestFirst);
        assert!(!notices.iter().any(|m| m.id == SET_ASIDE_MAIL), "on a pile, not in its category");
    }

    /// Task E19.3: putting mail on a pile moves it from the other one, taking it off empties it, and
    /// an unknown id changes nothing.
    #[test]
    fn mail_is_put_on_moved_between_and_taken_off_the_piles() {
        let demo = DemoSource::new(now());
        demo.set_mail_pile(&["m:w1".into()], Some(MailPile::ReplyLater), None).expect("put on");
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, None), ["m:w1", "m:w13", "m:p7"]);
        demo.set_mail_pile(&["m:w1".into(), "m:p7".into()], Some(MailPile::SetAside), None).expect("moved");
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, None), ["m:w13"]);
        assert_eq!(pile_ids(&demo, MailPile::SetAside, None), ["m:w1", SET_ASIDE_MAIL, "m:p7"]);
        demo.set_mail_pile(&["m:w1".into()], None, None).expect("taken off");
        assert_eq!(demo.get_mail("m:w1").expect("the mail").summary.pile, None);
        let unknown = ["m:w13".to_owned(), "m:nope".to_owned()];
        assert_eq!(demo.set_mail_pile(&unknown, None, None), Err(InboxError::NotFound));
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, None), ["m:w13"], "nothing changed");
    }

    /// Decision of #315: the demo keeps since when a mail lies on its pile, as the core does – the
    /// sample's piles since their mail came, now when put on, the same pile again keeps its time, an
    /// undo's `since` (at most now) puts it back with its time, off a pile it has none.
    #[test]
    fn a_pile_keeps_its_time() {
        let now = Utc::now();
        let demo = DemoSource::new(now);
        let since = |id: &str| demo.get_mail(id).expect("the mail").summary.pile_since;
        let date = |id: &str| demo.get_mail(id).expect("the mail").summary.date;
        assert_eq!(since("m:p7"), Some(date("m:p7")), "the sample's piles since their mail came");
        assert_eq!(since("m:w1"), None, "on no pile");
        demo.set_mail_pile(&["m:w1".into()], Some(MailPile::ReplyLater), None).expect("put on");
        let put = since("m:w1").expect("since now");
        assert!(put >= now, "now");
        demo.set_mail_pile(&["m:p7".into()], Some(MailPile::ReplyLater), None).expect("again");
        assert_eq!(since("m:p7"), Some(date("m:p7")), "the same pile keeps its time");
        let earlier = now - TimeDelta::days(3);
        demo.set_mail_pile(&["m:w1".into()], Some(MailPile::SetAside), Some(earlier)).expect("undone");
        assert_eq!(since("m:w1"), Some(earlier), "the undo's time");
        // Third review of #315: on the same pile the time stays, also when a `since` comes.
        demo.set_mail_pile(&["m:w1".into()], Some(MailPile::SetAside), Some(now - TimeDelta::days(9))).expect("same");
        assert_eq!(since("m:w1"), Some(earlier), "the same pile keeps its time, whatever since says");
        demo.set_mail_pile(&["m:w1".into()], None, None).expect("taken off");
        assert_eq!(since("m:w1"), None);
        demo.set_mail_pile(&["m:w1".into()], Some(MailPile::SetAside), Some(now + TimeDelta::days(3))).expect("ahead");
        assert!(since("m:w1").is_some_and(|at| at <= demo.now()), "never ahead of now");
        // Third review of #315: back on "Reply later" from before an answer sent since – off at once;
        // from after it, it stays.
        let before = since("m:p7").expect("on Reply later");
        demo.set_mail_pile(&["m:p7".into()], None, None).expect("taken off");
        let draft = Draft { in_reply_to: Some("m:p7".into()), ..draft(MAIL_ACCOUNTS[1], &["clara@example.org"]) };
        let saved = demo.save_draft(draft).expect("saved");
        demo.send_draft(&saved.id.expect("an id")).expect("answered");
        demo.set_mail_pile(&["m:p7".into()], Some(MailPile::ReplyLater), Some(before)).expect("undone");
        let back = demo.get_mail("m:p7").expect("the mail").summary;
        assert_eq!((back.pile, back.pile_since), (None, None), "answered since: off at once");
        demo.set_mail_pile(&["m:p7".into()], Some(MailPile::ReplyLater), Some(demo.now())).expect("after");
        assert_eq!(demo.get_mail("m:p7").expect("the mail").summary.pile, Some(MailPile::ReplyLater), "stays");
        demo.set_mail_pile(&["m:p7".into()], Some(MailPile::SetAside), Some(before)).expect("set aside");
        assert_eq!(demo.get_mail("m:p7").expect("the mail").summary.pile, Some(MailPile::SetAside), "not Reply later");
    }

    /// Task E19.3, decision 5 of E19: a sent answer takes its conversation off "Reply later", never
    /// off "Set aside", and leaves other conversations where they are.
    #[test]
    fn an_answer_takes_its_conversation_off_reply_later() {
        let demo = DemoSource::new(now());
        let answer = |to: &str, account: &str| {
            let draft = Draft { in_reply_to: Some(to.into()), ..draft(account, &["someone@example.org"]) };
            let saved = demo.save_draft(draft).expect("saved");
            demo.send_draft(&saved.id.expect("an id")).expect("sent");
        };
        answer("m:p7", MAIL_ACCOUNTS[1]);
        assert_eq!(pile_ids(&demo, MailPile::ReplyLater, None), ["m:w13"], "Clara answered");
        answer(SET_ASIDE_MAIL, MAIL_ACCOUNTS[0]);
        assert_eq!(pile_ids(&demo, MailPile::SetAside, None), [SET_ASIDE_MAIL], "set aside stays");
    }

    /// Task E19.3: the sample follows up Anna's sent mail "only if no reply", and any message can be
    /// followed up by its id – a new entry for one without, the condition kept; an entry that is no
    /// mail refuses the condition.
    #[tokio::test]
    async fn mail_is_followed_up_only_if_no_reply() {
        let demo = DemoSource::new(now());
        let held = demo.lock().items.iter().find(|item| item.id == FOLLOW_UP_ENTRY).cloned().expect("the entry");
        let follow_up = held.follow_up.expect("a follow-up");
        assert!(follow_up.unless_reply && follow_up.at > now() && follow_up.since < now());
        assert_eq!(held.state, State::Snoozed { until: follow_up.at });
        assert!(matches!(&held.link, Some(Link::Mail { message }) if message == FOLLOW_UP_MAIL));

        let until = now() + TimeDelta::days(3);
        let kept = demo.snooze_item("m:w10".into(), Some(until), true, false).await.expect("followed up");
        assert_eq!((kept.id.as_str(), kept.state), ("demo-follow-w10", State::Snoozed { until }));
        assert!(kept.follow_up.is_some_and(|follow_up| follow_up.unless_reply && follow_up.at == until));
        let again = demo.snooze_item("m:w10".into(), None, false, false).await.expect("called off");
        assert_eq!((again.id.as_str(), again.follow_up), ("demo-follow-w10", None), "the same entry");
        // Anna's sent mail of a conversation with an entry follows up that entry.
        let sent = demo.snooze_item("m:w7".into(), Some(until), true, false).await.expect("followed up");
        assert_eq!(sent.thread_key, "t:design", "the entry of Lena's answer: {}", sent.id);
        // Without a time a message without an entry stays without one.
        let before = demo.lock().items.len();
        let stand_in = demo.snooze_item("m:p6".into(), None, false, false).await.expect("the entry it would be");
        assert_eq!((stand_in.id.as_str(), demo.lock().items.len()), ("m:p6", before));
        // A mail entry by its message's id; an entry that is no mail refuses the condition.
        let filed = demo.snooze_item("m:w1".into(), Some(until), false, false).await.expect("its entry");
        assert!(filed.id.starts_with("demo-mail-") && filed.follow_up.is_some_and(|f| !f.unless_reply));
        assert_eq!(demo.snooze_item("demo-7".into(), Some(until), true, false).await, Err(InboxError::Unsupported));
        assert_eq!(demo.snooze_item("m:nope".into(), Some(until), false, false).await, Err(InboxError::NotFound));
        // Review of #315: an open mail entry is followed up without being put off, as the core does.
        demo.snooze_item("m:w1".into(), None, false, false).await.expect("called off");
        let open = demo.snooze_item("m:w1".into(), Some(until), false, true).await.expect("kept open");
        assert!(open.state.is_open() && open.follow_up.is_some_and(|f| f.at == until), "{open:?}");
        demo.snooze_item("demo-follow-w10".into(), Some(until), false, false).await.expect("put off");
        // As the core: a mail filed in its own entry stands for it – a snoozed one comes back by the
        // mail's id as by the entry's.
        let by_mail = demo.snooze_item("m:w10".into(), Some(until), false, true).await.expect("by its mail");
        assert_eq!((by_mail.id.as_str(), by_mail.state), ("demo-follow-w10", State::Seen), "its own entry back");
        demo.snooze_item("demo-follow-w10".into(), Some(until), false, false).await.expect("put off again");
        let reopened = demo.snooze_item("demo-follow-w10".into(), Some(until), false, true).await.expect("reopened");
        assert_eq!((reopened.state, reopened.follow_up.map(|f| f.at)), (State::Seen, Some(until)), "a snoozed entry");
        demo.set_item_done("demo-follow-w10".into(), true).await.expect("done");
        let refused = demo.snooze_item("demo-follow-w10".into(), Some(until), false, true).await;
        assert_eq!(refused, Err(InboxError::Unsupported), "a done entry");
        let by_mail = demo.snooze_item("m:w10".into(), Some(until), false, true).await;
        assert_eq!(by_mail, Err(InboxError::Unsupported), "a done entry by its mail's id");
        // A mail without an entry of its own (the sent "m:w7") in a conversation whose entry is snoozed
        // or done is refused; the entry's mail ("m:w2") brings it back.
        let design = demo
            .lock()
            .items
            .iter()
            .find(|item| matches!(&item.link, Some(Link::Mail { message }) if message == "m:w2"))
            .map(|item| item.id.clone())
            .expect("the design conversation's entry");
        demo.snooze_item(design.clone(), Some(until), false, false).await.expect("put off");
        let sent = demo.snooze_item("m:w7".into(), Some(until), false, true).await;
        assert_eq!(sent, Err(InboxError::Unsupported), "a snoozed conversation by a mail without its own entry");
        let back = demo.snooze_item("m:w2".into(), Some(until), false, true).await.expect("by the entry's mail");
        assert_eq!((back.id, back.state), (design.clone(), State::Seen));
        demo.set_item_done(design, true).await.expect("done");
        let sent = demo.snooze_item("m:w7".into(), Some(until), false, true).await;
        assert_eq!(sent, Err(InboxError::Unsupported), "a done conversation by a mail without its own entry");
        assert_eq!(demo.snooze_item("demo-7".into(), Some(until), false, true).await, Err(InboxError::Unsupported));
        assert_eq!(demo.snooze_item("m:w1".into(), None, false, true).await, Err(InboxError::Unsupported));
    }
}
