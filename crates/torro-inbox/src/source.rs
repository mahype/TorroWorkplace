//! Where the inbox app gets its entries from.
//!
//! The app only knows [`InboxSource`]. In production that is the core over the socket
//! (`torro_ipc::inbox::InboxClient`, task E1.3); for building and trying the app it is
//! [`crate::demo::DemoSource`] with sample data in memory. [`ItemFilter`] is shared with the
//! request `inbox.list_items` (E1.1), so the filter means the same everywhere.

use std::fmt;
use std::future::Future;

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_core::i18n::tr;
use torro_mail::Order;

use crate::item::{
    Hidden, HideScope, Item, Lane, MuteEffect, MuteOption, MuteScope, Muted, SourceInfo, SourceRef, State,
};
use crate::rules;

/// What the app can do with the inbox. Every method is one `inbox.*` request; the core checks
/// rights and the security rules, a source never decides them itself.
pub trait InboxSource: Send + Sync {
    /// The entries `filter` selects, sorted by [`rules::order_in`] in `filter.order` and cut to
    /// `filter.limit`.
    /// Snoozed entries that are due are woken first ([`rules::wake`]).
    fn list_items(&self, filter: ItemFilter) -> impl Future<Output = Result<Vec<Item>, InboxError>> + Send;

    /// Every source that has entries or is muted, for the sidebar, ordered by
    /// [`SourceRef`] (suite, then agents, then scripts, then mail accounts, each by name).
    fn list_sources(&self) -> impl Future<Output = Result<Vec<SourceInfo>, InboxError>> + Send;

    /// Marks the entry done, or open again with `done: false` ([`rules::set_done`]). Returns it as
    /// stored. [`InboxError::NotFound`] for an unknown id, [`InboxError::Forbidden`] when the
    /// client may not change it ([`rules::human_only`]).
    fn set_item_done(&self, id: String, done: bool) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// Dismisses the entry ("✕", task E8.6a): done like [`InboxSource::set_item_done`] with `true`
    /// (`inbox.set_item_done` with `dismissed: true`), so the statistics (E8.8) tell dismissing
    /// apart from done. Undo is [`InboxSource::set_item_done`] with `false`. Returns the entry as
    /// stored; errors as [`InboxSource::set_item_done`].
    fn dismiss_item(&self, id: String) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// The ways to mute the entry `id` for the bell's menu (`inbox.list_mute_options`, task E8.6a),
    /// built by the core from the stored entry ([`rules::mute_options`]): the conversation first,
    /// then its traits, each with its words. Values and words marked `untrusted` are foreign text.
    /// Empty for the person's own entries. [`InboxError::NotFound`] for an unknown id.
    fn list_mute_options(&self, id: String) -> impl Future<Output = Result<Vec<MuteOption>, InboxError>> + Send;

    /// Mutes by the entry `id` (`inbox.mute_item`, task E8.6a): its conversation or one of its
    /// traits (`scope`, the value always from the stored entry), until `until` or for good (`None`),
    /// with `effect` – no popup, only "Info", or hidden. Except for the conversation hidden or moved
    /// for good ([`MuteScope::keeps_rule`]) a rule is kept (admin level); the open entries it holds
    /// for are done (`hide`) or moved to "Info" (`info`) at once. Returns what changed ([`Muted`]),
    /// so the app can take it back exactly: [`InboxSource::remove_rule`] for a new rule,
    /// [`InboxSource::set_item_done`] with `false` for each id in [`Muted::done`] and
    /// [`InboxSource::set_item_lane`] to "for me" for each in [`Muted::moved`].
    ///
    /// [`InboxError::NotFound`] for an unknown entry, [`InboxError::Forbidden`] for one the client
    /// may not change ([`rules::human_only`]), [`InboxError::Other`] for a trait the entry does not
    /// have or an `until` that has passed.
    fn mute_item(
        &self,
        id: String,
        scope: MuteScope,
        until: Option<DateTime<Utc>>,
        effect: MuteEffect,
    ) -> impl Future<Output = Result<Muted, InboxError>> + Send;

    /// Puts the entry off until `until`, or takes it back with `None` ([`rules::snooze`]). `id` may
    /// also be a mail message's id (`m:…`): its entry, or the entry the core makes for it.
    ///
    /// On a mail entry it is a follow-up ([`rules::follow_up`], task E19.1): with `unless_reply`
    /// it comes back at `until` only if nobody replied by then ([`crate::item::FollowUp`]); `None`
    /// calls any follow-up off. `unless_reply` on an entry that is no mail is
    /// [`InboxError::Unsupported`]; it is ignored without `until`. With `keep_open` (review of #315,
    /// the Undo of "Cancel follow-up" or of a follow-up date on an open entry) a mail entry gets its
    /// follow-up without being put off: an open one stays as it is, a snoozed one is open again
    /// ([`rules::follow_up_open`]); [`InboxError::Unsupported`] without `until`, on a done entry and
    /// on one that is no mail. Returns the entry as stored; other errors as
    /// [`InboxSource::set_item_done`].
    fn snooze_item(
        &self,
        id: String,
        until: Option<DateTime<Utc>>,
        unless_reply: bool,
        keep_open: bool,
    ) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// Moves the entry to `lane` ([`rules::set_lane`]). With `remember` the core also keeps the
    /// lane for later notices of the same source (a rule, `inbox.list_rules`); that needs the
    /// admin level. Returns the entry as stored; errors as [`InboxSource::set_item_done`].
    fn set_item_lane(
        &self,
        id: String,
        lane: Lane,
        remember: bool,
    ) -> impl Future<Output = Result<Item, InboxError>> + Send;

    /// Mutes the source (its new reports are dropped from then on) or unmutes it. Entries already
    /// there stay. A permanent rule, admin level.
    fn mute_source(&self, source: SourceRef, muted: bool) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// Hides the mail entry `id` from the inbox (`inbox.hide_sender`, tasks E3.10 and E3.11), with
    /// [`HideScope::Conversation`] only this conversation: the entry is done, no rule is kept, a
    /// new reply brings it back. With [`HideScope::Sender`] every later mail from its sender's
    /// address, with [`HideScope::Domain`] every later mail from that address's domain (exactly,
    /// no subdomains), in **every** mail account: a lasting rule ([`crate::RuleEffect::Muted`] with
    /// `sender` or `domain`, [`rules::reaches`]); making it again answers the rule already there.
    /// The entry and every other open or snoozed mail entry the rule matches ([`rules::hides`])
    /// are done at once.
    ///
    /// Hiding is no spam and no "done" of the mail: the mail stays as it is on its server – not
    /// read, not moved –, whatever the scope. Returns what changed ([`Hidden`]), so the app can
    /// take it back exactly: [`InboxSource::remove_rule`] for a new rule, then
    /// [`InboxSource::set_item_done`] with `false` for each id in [`Hidden::done`].
    ///
    /// [`InboxError::NotFound`] for an unknown entry, [`InboxError::Forbidden`] for one the client
    /// may not change ([`rules::human_only`]), [`InboxError::Other`] for an entry that cannot be
    /// hidden that way ([`rules::may_hide`]: no mail, or no sender's address for a rule).
    fn hide_sender(&self, id: String, scope: HideScope) -> impl Future<Output = Result<Hidden, InboxError>> + Send;

    /// Removes the lasting rule `id` (`inbox.remove_rule`, task E3.11 for the app's "Undo" after
    /// hiding a sender or domain): its source's reports, or the hidden sender's mail, come in again
    /// from now on; entries already done stay done. Admin level. [`InboxError::NotFound`] for an
    /// unknown rule (also one removed already).
    fn remove_rule(&self, id: String) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// Opens the entry on the person's screen (`inbox.open_item`, task E2.4): the core brings the
    /// window of the agent behind a hook's entry to the front, opens its web page in the browser
    /// or starts the app of the suite it names, as far as [`rules::may_open`] allows; a new entry
    /// is [`State::Seen`] afterwards. The app calls it for "Go to window" and `Enter` instead of
    /// opening anything itself. [`InboxError::NotFound`] for an unknown id,
    /// [`InboxError::Unsupported`] when there is nothing safe to open or no window was found.
    fn open_item(&self, id: String) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// The task lists a task can be made in, for the dialog of "Make task" (task E3.5): the
    /// lists of `tasks.list_lists` that can be written to (no GitHub filter lists), in the order
    /// the core lists them. Empty without a connected account. Errors as the other calls
    /// ([`InboxError::Offline`] when the core does not answer).
    fn list_task_lists(&self) -> impl Future<Output = Result<Vec<TaskList>, InboxError>> + Send;

    /// Turns the entry `id` into a task in the list `list` (an id from
    /// [`InboxSource::list_task_lists`]) and marks the entry done (`inbox.create_task_from_item`,
    /// task E3.5). `title` `None` or blank takes the entry's title; `due` is a local day or none.
    /// The core links the task back to the entry and checks rights and lists; the entry stays
    /// open when the task could not be saved. [`InboxError::NotFound`] for an unknown entry or
    /// list, [`InboxError::Forbidden`] when the entry may not be changed ([`rules::human_only`]).
    fn create_task_from_item(
        &self,
        id: String,
        list: String,
        title: Option<String>,
        due: Option<NaiveDate>,
    ) -> impl Future<Output = Result<(), InboxError>> + Send;

    /// Allows (`allow`) or denies the approval `approval` – the id an approval entry carries in its
    /// [`crate::Action::Allow`] / [`crate::Action::Deny`] – (`approvals.decide_approval`, task E3.8).
    /// Only the app key may: the window is the person (security rule 1). Allowed, the core runs the
    /// command now; either way the approval's entry is done afterwards. [`InboxError::Settled`]
    /// when it was decided already or expired, [`InboxError::NotFound`] for an unknown approval.
    fn decide_approval(&self, approval: String, allow: bool) -> impl Future<Output = Result<(), InboxError>> + Send;
}

/// A task list a task can be made in ([`InboxSource::list_task_lists`]): its id as
/// `tasks.list_lists` gives it, and its name. Shown by its name in pickers ([`fmt::Display`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskList {
    /// `<account-id>/<collection URL>`, what `inbox.create_task_from_item` takes as `collection`.
    pub id: String,
    /// The list's name, as the server calls it.
    pub name: String,
}

impl fmt::Display for TaskList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// Which entries [`InboxSource::list_items`] returns. Every field left at `None` selects all;
/// the default selects everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemFilter {
    /// Only entries in this lane.
    #[serde(default)]
    pub lane: Option<Lane>,
    /// Only entries of this source.
    #[serde(default)]
    pub source: Option<SourceRef>,
    /// Only open, snoozed or done entries.
    #[serde(default)]
    pub state: Option<StateFilter>,
    /// Only entries `updated` at this instant or later.
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// At most this many entries, the first ones by [`rules::order_in`] in `order`.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Newest or oldest first by `updated` (task E8.15); urgent entries come first either way.
    #[serde(default)]
    pub order: Order,
    /// `Some(false)` leaves out the entries dismissed ([`InboxSource::dismiss_item`]), `Some(true)`
    /// keeps only those (issue #117: the app's "Done" shows only what was done for real). An
    /// [`Item`] does not say whether it was dismissed, so [`ItemFilter::matches`] ignores this
    /// field: the source applies it from what it keeps beside the entry (the core's column
    /// `dismissed`, the demo's set of dismissed ids).
    #[serde(default)]
    pub dismissed: Option<bool>,
    /// Only entries whose mail has one of these categories (task E12.4, `docs/kategorien.md`);
    /// empty: every entry, with a category or without. Entries that are no mail never have one,
    /// so a filter always leaves them out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<torro_mail::Category>,
}

/// The states the views of the app show: "For me" and "Info" show open entries, "Snoozed" and
/// "Done" the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateFilter {
    /// New or seen ([`State::is_open`]).
    Open,
    Snoozed,
    Done,
}

impl StateFilter {
    pub fn matches(self, state: &State) -> bool {
        match self {
            StateFilter::Open => state.is_open(),
            StateFilter::Snoozed => matches!(state, State::Snoozed { .. }),
            StateFilter::Done => matches!(state, State::Done { .. }),
        }
    }
}

impl ItemFilter {
    /// The entry passes every field that is set, `limit` aside.
    pub fn matches(&self, item: &Item) -> bool {
        self.lane.is_none_or(|lane| item.lane == lane)
            && self.source.as_ref().is_none_or(|source| item.source == *source)
            && self.state.is_none_or(|state| state.matches(&item.state))
            && self.since.is_none_or(|since| item.updated >= since)
            && (self.categories.is_empty()
                || item.category.is_some_and(|held| self.categories.contains(&held.category)))
    }

    /// The entries of `items` that match, sorted by [`rules::order_in`] in `order` and cut to
    /// `limit`: the answer of [`InboxSource::list_items`] for entries already woken.
    pub fn apply(&self, items: impl IntoIterator<Item = Item>) -> Vec<Item> {
        let mut selected: Vec<Item> = items.into_iter().filter(|item| self.matches(item)).collect();
        rules::sort_in(&mut selected, self.order);
        if let Some(limit) = self.limit {
            selected.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        selected
    }
}

/// Why an inbox call failed. The variants match the error codes of the `inbox.*` requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxError {
    /// No entry with this id, or one the client may not see (`not-found`).
    NotFound,
    /// The client may see the entry but not change it (`forbidden`, [`rules::human_only`]).
    Forbidden,
    /// Too many reports in the last hour (`busy`).
    Busy,
    /// Nothing that may be opened, or the agent's window was not found (`unsupported`,
    /// [`InboxSource::open_item`]).
    Unsupported,
    /// The approval was decided already or expired ([`InboxSource::decide_approval`], `conflict`).
    Settled,
    /// The core does not answer.
    Offline,
    /// Anything else, with the message of the core (`error`).
    Other(String),
}

impl fmt::Display for InboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InboxError::NotFound => f.write_str(tr("This entry no longer exists.")),
            InboxError::Forbidden => f.write_str(tr("Only you can change this entry, in the Notification Hub.")),
            InboxError::Busy => f.write_str(tr("Too many entries at once. Try again later.")),
            InboxError::Unsupported => f.write_str(tr("Can't open this from here. The window may have closed.")),
            InboxError::Settled => f.write_str(tr("This was decided already or has expired.")),
            InboxError::Offline => f.write_str(tr("Torro Workplace isn't responding. It may still be starting.")),
            InboxError::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for InboxError {}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::item::Kind;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, hour, 0, 0).single().expect("valid time")
    }

    fn item(id: &str, lane: Lane, state: State, hour: u32) -> Item {
        Item {
            id: id.into(),
            source: SourceRef::script(if id.starts_with('b') { "backup" } else { "ci" }),
            thread_key: id.into(),
            kind: Kind::Notice,
            lane,
            urgent: false,
            title: id.into(),
            body: None,
            from: None,
            agent: None,
            received: at(hour),
            updated: at(hour),
            count: 1,
            state,
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

    fn sample() -> Vec<Item> {
        vec![
            item("b1", Lane::ForMe, State::New, 8),
            item("c1", Lane::Info, State::Seen, 9),
            item("b2", Lane::ForMe, State::Snoozed { until: at(20) }, 10),
            item("c2", Lane::ForMe, State::Done { at: at(12) }, 11),
            item("c3", Lane::ForMe, State::Seen, 12),
        ]
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn the_default_filter_selects_everything_sorted() {
        assert_eq!(ids(&ItemFilter::default().apply(sample())), ["c3", "c2", "b2", "c1", "b1"]);
    }

    #[test]
    fn filters_combine() {
        let for_me = ItemFilter { lane: Some(Lane::ForMe), state: Some(StateFilter::Open), ..ItemFilter::default() };
        assert_eq!(ids(&for_me.apply(sample())), ["c3", "b1"]);
        let snoozed = ItemFilter { state: Some(StateFilter::Snoozed), ..ItemFilter::default() };
        assert_eq!(ids(&snoozed.apply(sample())), ["b2"]);
        let done = ItemFilter { state: Some(StateFilter::Done), ..ItemFilter::default() };
        assert_eq!(ids(&done.apply(sample())), ["c2"]);
        let backup = ItemFilter { source: Some(SourceRef::script("backup")), ..ItemFilter::default() };
        assert_eq!(ids(&backup.apply(sample())), ["b2", "b1"]);
        let since = ItemFilter { since: Some(at(10)), limit: Some(2), ..ItemFilter::default() };
        assert_eq!(ids(&since.apply(sample())), ["c3", "c2"]);
    }

    /// Task E8.15: oldest first, and `limit` takes the first ones of that order.
    #[test]
    fn the_order_turns_and_the_limit_follows_it() {
        let oldest = ItemFilter { order: Order::OldestFirst, ..ItemFilter::default() };
        assert_eq!(ids(&oldest.apply(sample())), ["b1", "c1", "b2", "c2", "c3"]);
        let first_two = ItemFilter { limit: Some(2), ..oldest.clone() };
        assert_eq!(ids(&first_two.apply(sample())), ["b1", "c1"]);
        let newest_two = ItemFilter { limit: Some(2), ..ItemFilter::default() };
        assert_eq!(ids(&newest_two.apply(sample())), ["c3", "c2"]);
    }

    #[test]
    fn filters_survive_a_serde_round_trip() {
        let filter = ItemFilter {
            lane: Some(Lane::Info),
            source: Some(SourceRef::agent("codex")),
            state: Some(StateFilter::Open),
            since: Some(at(9)),
            limit: Some(50),
            order: Order::OldestFirst,
            dismissed: Some(false),
            categories: Vec::new(),
        };
        let json = serde_json::to_string(&filter).expect("serialize");
        assert_eq!(serde_json::from_str::<ItemFilter>(&json).expect("deserialize"), filter);
        assert_eq!(serde_json::from_str::<ItemFilter>("{}").expect("deserialize"), ItemFilter::default());
    }
}
