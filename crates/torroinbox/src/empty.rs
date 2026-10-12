//! An inbox without entries, for the app's tests only.
//!
//! The app runs on the core's client (`torro_ipc::inbox::InboxClient`) or the demo source; the
//! tests use this one to see the empty state and views that start without any data.

use chrono::{DateTime, Utc};
use torro_inbox::{InboxError, InboxSource, Item, ItemFilter, Lane, SourceInfo, SourceRef};

/// An inbox without entries and without sources. Listing gives nothing; changing or opening an
/// entry gives [`InboxError::NotFound`] (there is none), muting [`InboxError::Offline`] (nothing
/// would keep it).
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

    async fn snooze_item(&self, _id: String, _until: Option<DateTime<Utc>>) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn set_item_lane(&self, _id: String, _lane: Lane, _remember: bool) -> Result<Item, InboxError> {
        Err(InboxError::NotFound)
    }

    async fn mute_source(&self, _source: SourceRef, _muted: bool) -> Result<(), InboxError> {
        Err(InboxError::Offline)
    }

    async fn open_item(&self, _id: String) -> Result<(), InboxError> {
        Err(InboxError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use iced::futures::executor::block_on;

    use super::*;

    #[test]
    fn lists_nothing_and_changes_nothing() {
        let source = EmptySource;
        assert_eq!(block_on(source.list_items(ItemFilter::default())), Ok(Vec::new()));
        assert_eq!(block_on(source.list_sources()), Ok(Vec::new()));
        assert_eq!(block_on(source.set_item_done("x".into(), true)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.snooze_item("x".into(), None)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.set_item_lane("x".into(), Lane::Info, false)), Err(InboxError::NotFound));
        assert_eq!(block_on(source.mute_source(SourceRef::suite(), true)), Err(InboxError::Offline));
        assert_eq!(block_on(source.open_item("x".into())), Err(InboxError::NotFound));
    }
}
