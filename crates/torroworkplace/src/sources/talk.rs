//! Nextcloud Talk as a source of the inbox (task E5.9; scaffold since task E5.2): one entry per
//! unread conversation of each Nextcloud account with Talk, `talk:<login>@<host>`, a source of its
//! own so it can be granted, muted and shown apart from the account's notifications.
//!
//! What task E5.9 fills in (see there): the conversations every 60 s, an entry where
//! `rules::talk_place` says so (kind `message`, matter the conversation's token), traits `person`
//! and `kind`, the link `<server>/call/<token>`; a conversation read elsewhere resolved.

use torro_inbox::Item;

use super::Sources;

/// Starts Talk of every Nextcloud account that has it on the current runtime and returns at once.
/// A muted source asks the server nothing. Nothing yet (task E5.9).
pub(super) fn start(_sources: Sources) {}

/// Marks the conversation of the done entry `item` read up to the last message the inbox knows of
/// (among `ids`) and deletes the matching chat notifications, in the background: spawn the calls on
/// the current runtime and return at once. Failures only to the log, with the account and a code.
/// Nothing yet (task E5.9).
pub(super) fn settle_done(_sources: &Sources, _item: &Item, _ids: Vec<String>) {}
