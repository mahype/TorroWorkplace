//! Sample entries in memory, for building and trying the inbox app without the core.
//!
//! `torroinbox --demo` uses it (task E3.1). Changes live until the app closes. The sample is
//! built around "now" (which `--now` may fix, for repeatable screenshots), so the times in the
//! list always read "4 min", "2 h" and so on. It has every kind of entry the first steps bring:
//! two waiting agents, a finished one, an approval, an account problem, script notices in both
//! lanes, a snoozed and a done entry.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, TimeDelta, Utc};
use torro_core::i18n::{tr, trf};

use crate::item::{Action, AgentInfo, AgentKind, Item, Kind, Lane, Link, SourceInfo, SourceRef, State};
use crate::rules;
use crate::source::{InboxError, InboxSource, ItemFilter};

/// The approval behind the sample approval entry; its actions name it.
pub const APPROVAL_ID: &str = "demo-approval-1";

#[derive(Debug)]
struct Data {
    items: Vec<Item>,
    muted: BTreeSet<SourceRef>,
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
        DemoSource { data: Mutex::new(Data { items: sample(now), muted: BTreeSet::new() }), offset: now - Utc::now() }
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
    }
}

/// A notice from the script `name` in `lane`, titled `title` (translated already).
fn notice(now: DateTime<Utc>, n: u32, name: &str, lane: Lane, title: &str, minutes: i64) -> Item {
    Item { lane, ..entry(now, n, SourceRef::script(name), Kind::Notice, title.into(), minutes) }
}

/// What a hook reports about a session of `agent` in `project` on `branch`.
fn agent(agent: AgentKind, session: &str, project: &str, branch: &str) -> Option<AgentInfo> {
    Some(AgentInfo {
        agent,
        session: session.into(),
        project: Some(project.into()),
        branch: Some(branch.into()),
        pids: Vec::new(),
    })
}

/// The sample entries, eleven of them; times are relative to `now`.
pub fn sample(now: DateTime<Utc>) -> Vec<Item> {
    let claude = || SourceRef::agent(AgentKind::ClaudeCode.id());
    let codex = || SourceRef::agent(AgentKind::Codex.id());
    let waiting = |name: AgentKind| trf("{agent} is waiting for your permission", &[("agent", &name.label())]);
    let finished = |name: AgentKind| trf("{agent} finished", &[("agent", &name.label())]);
    vec![
        Item {
            thread_key: "claude-code:demo-claude-1".into(),
            body: Some(tr("Claude needs your permission to use Bash: cargo test --workspace").into()),
            agent: agent(AgentKind::ClaudeCode, "demo-claude-1", "TorroWorkplace", "feat/inbox"),
            link: Some(Link::Window),
            ..entry(now, 1, claude(), Kind::AgentWaiting, waiting(AgentKind::ClaudeCode), 4)
        },
        Item {
            thread_key: "codex:demo-codex-1".into(),
            body: Some(tr("Codex wants to run: npm install").into()),
            agent: agent(AgentKind::Codex, "demo-codex-1", "torro-design", "main"),
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
                tr("Claude Desktop wants to delete the event \"Team meeting\"").into(),
                8,
            )
        },
        Item {
            thread_key: "claude-code:demo-claude-2".into(),
            body: Some(tr("Fixed the footer links on the website. 3 files changed.").into()),
            agent: agent(AgentKind::ClaudeCode, "demo-claude-2", "website", "fix/footer"),
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
            body: Some(tr("Step \"migrate\" exited with code 1.").into()),
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
            agent: agent(AgentKind::Codex, "demo-codex-0", "torro-api", "refactor/settings"),
            link: Some(Link::Window),
            state: State::Done { at: now - TimeDelta::hours(22) },
            ..entry(now, 11, codex(), Kind::AgentDone, finished(AgentKind::Codex), 26 * 60)
        },
    ]
}

impl InboxSource for DemoSource {
    async fn list_items(&self, filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        let now = self.now();
        let mut data = self.lock();
        for item in &mut data.items {
            rules::wake(item, now);
        }
        data.items.retain(|item| !rules::expired(item, now));
        Ok(filter.apply(data.items.iter().cloned()))
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
            })
            .collect())
    }

    async fn set_item_done(&self, id: String, done: bool) -> Result<Item, InboxError> {
        let now = self.now();
        self.change(&id, |item| rules::set_done(item, done, now))
    }

    async fn snooze_item(&self, id: String, until: Option<DateTime<Utc>>) -> Result<Item, InboxError> {
        self.change(&id, |item| rules::snooze(item, until))
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
        let waiting = trf("{agent} is waiting for your permission", &[("agent", &"Claude Code")]);
        assert!(titles.contains(&waiting.as_str()));
        let approval = items.iter().find(|i| i.kind == Kind::Approval).expect("an approval");
        let labels: Vec<&str> = approval.actions.iter().map(Action::label).collect();
        assert_eq!(labels, [tr("Allow"), tr("Deny")]);
    }

    #[tokio::test]
    async fn for_me_lists_urgent_entries_first() {
        let demo = DemoSource::new(now());
        let for_me = demo.list_items(open_in(Lane::ForMe)).await.expect("list");
        assert_eq!(ids(&for_me), ["demo-1", "demo-3", "demo-2", "demo-4", "demo-6", "demo-5"]);
        let info = demo.list_items(open_in(Lane::Info)).await.expect("list");
        assert_eq!(ids(&info), ["demo-8", "demo-9", "demo-7"]);
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
        assert_eq!(ids(&demo.list_items(open_in(Lane::ForMe)).await.expect("list"))[0], "demo-1");
    }

    #[tokio::test]
    async fn snoozed_entries_wake_when_due() {
        let demo = DemoSource::new(now());
        let snoozed = demo.snooze_item("demo-7".into(), Some(now() + TimeDelta::hours(1))).await.expect("snooze");
        assert_eq!(snoozed.state, State::Snoozed { until: now() + TimeDelta::hours(1) });
        assert!(!ids(&demo.list_items(open_in(Lane::Info)).await.expect("list")).contains(&"demo-7"));
        // Snoozed until a moment that has passed: woken on the next listing, at the top.
        demo.snooze_item("demo-7".into(), Some(now() - TimeDelta::minutes(1))).await.expect("snooze");
        let info = demo.list_items(open_in(Lane::Info)).await.expect("list");
        assert_eq!(ids(&info)[0], "demo-7");
        assert_eq!(info[0].state, State::New);
        let back = demo.snooze_item("demo-10".into(), None).await.expect("take back");
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
                "script:uptime"
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
        assert_eq!(demo.snooze_item("nope".into(), None).await, Err(InboxError::NotFound));
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
}
