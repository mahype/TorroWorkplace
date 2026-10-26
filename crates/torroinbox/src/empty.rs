//! An inbox without entries, for the app's tests only.
//!
//! The app runs on the core's client (`torro_ipc::inbox::InboxClient`) or the demo source; the
//! tests use this one to see the empty state and views that start without any data. Its mail side
//! (`crate::mail::MailSource`) stands in `mail.rs`: no folders, no mail, "All" holds nothing and
//! nothing unread, "Show again" has nothing to remember, the piles hold nothing and no mail can be
//! put on one or answered.

use chrono::{DateTime, NaiveDate, Utc};
use torro_inbox::item::{HideScope, MuteEffect, MuteOption, MuteScope, Muted};
use torro_inbox::source::TaskList;
use torro_inbox::{Hidden, InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};

/// An inbox without entries and without sources. Listing gives nothing (no task lists either);
/// changing or opening an entry, making a task of it or deciding an approval gives
/// [`InboxError::NotFound`] (there is none), muting [`InboxError::Offline`] (nothing would keep it);
/// hiding a sender and removing a rule [`InboxError::NotFound`] (no entry to hide it by, no rule);
/// dismissing, listing the ways to mute and muting [`InboxError::NotFound`] too (no entry).
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptySource;

impl InboxSource for EmptySource {
    async fn list_items(&self, _filter: ItemFilter) -> Result<Vec<Item>, InboxError> {
        Ok(Vec::new())
    }

    async fn list_sources(&self) -> Result<Vec<SourceInfo>, InboxError> {
        Ok(Vec::new())
    }

    async fn set_item_done(&self, _id: String, _done: bool) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn dismiss_item(&self, _id: String) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn list_mute_options(&self, _id: String) -> Result<Vec<MuteOption>, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mute_item(
        &self,
        _id: String,
        _scope: MuteScope,
        _until: Option<DateTime<Utc>>,
        _effect: MuteEffect,
    ) -> Result<Muted, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn snooze_item(
        &self,
        _id: String,
        _until: Option<DateTime<Utc>>,
        _unless_reply: bool,
    ) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn set_item_lane(&self, _id: String, _lane: Lane, _remember: bool) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mute_source(&self, _source: SourceRef, _muted: bool) -> Result<(), InboxError> {
        Err(InboxError::Offline)
    }

    async fn hide_sender(&self, _id: String, _scope: HideScope) -> Result<Hidden, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn remove_rule(&self, _id: String) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn open_item(&self, _id: String) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn list_task_lists(&self) -> Result<Vec<TaskList>, InboxError> {
        Ok(Vec::new())
    }

    async fn create_task_from_item(
        &self,
        _id: String,
        _list: String,
        _title: Option<String>,
        _due: Option<NaiveDate>,
    ) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }

    async fn decide_approval(&self, _approval: String, _allow: bool) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;

    use super::*;

    /// Task E18.3: no mail of any role, nothing unread in "All", "Show again" answers `Ok`; task
    /// E19.3: no piles.
    #[test]
    fn has_no_mail_by_role_and_nothing_to_show_again() {
        use torro_ipc::inbox::{MailByRole, MailInPile};
        use torro_ipc::mail::PileFilter;
        use torro_mail::{MailPile, Order, Role};

        use crate::mail::{MailPage, MailSource};

        let source = EmptySource;
        let find = MailByRole {
            role: Role::Inbox,
            account: None,
            snoozed: Some(false),
            hidden: Some(false),
            pile: Some(PileFilter::Without),
            order: Order::NewestFirst,
            limit: 50,
            cursor: None,
        };
        assert_eq!(block_on(source.mail_by_role(find)), Ok(MailPage::of(Vec::new(), None)));
        assert_eq!(block_on(source.unread_in_all(None)), Ok(0));
        assert_eq!(block_on(source.set_mail_shown(vec!["m:1".into()], true)), Ok(()));
        // Task E19.3: no pile holds anything, and there is no mail to put on one or to answer.
        let pile = MailInPile {
            pile: MailPile::ReplyLater,
            account: None,
            order: Order::NewestFirst,
            limit: 50,
            cursor: None,
        };
        assert_eq!(block_on(source.mail_in_pile(pile)), Ok(MailPage::of(Vec::new(), None)));
        assert_eq!(block_on(source.count_in_pile(MailPile::SetAside, None)), Ok(0));
        let put = source.set_mail_pile(vec!["m:1".into()], Some(MailPile::ReplyLater));
        assert_eq!(block_on(put), Err(InboxError::NotFound));
        assert_eq!(block_on(source.send_reply("m:1".into(), "Thanks".into())), Err(InboxError::NotFound));
    }

    #[test]
    fn lists_nothing_and_changes_nothing() {
        let source = EmptySource;
        assert_eq!(block_on(source.list_items(ItemFilter::default())), Ok(Vec::new()));
        assert_eq!(block_on(source.list_sources()), Ok(Vec::new()));
        assert_eq!(block_on(source.set_item_done("x".into(), true)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.snooze_item("x".into(), None, false)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.set_item_lane("x".into(), Lane::Info, false)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.mute_source(SourceRef::suite(), true)), Err(InboxError::Offline));
        assert_eq!(block_on(source.hide_sender("x".into(), HideScope::Sender)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.remove_rule("r".into())), Err(InboxError::NotFound));
        assert_eq!(block_on(source.open_item("x".into())), Err(InboxError::NotFound));
        assert_eq!(block_on(source.list_task_lists()), Ok(Vec::new()));
        assert_eq!(
            block_on(source.create_task_from_item("x".into(), "l".into(), None, None)),
            Err(InboxError::NotFound)
        );
        assert_eq!(block_on(source.decide_approval("x".into(), true)), Err(InboxError::NotFound));
    }
}
