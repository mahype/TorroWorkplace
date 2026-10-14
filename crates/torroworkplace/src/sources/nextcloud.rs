//! Nextcloud's notifications as a source of the inbox (task E5.8; scaffold since task E5.2): one
//! entry per matter of each Nextcloud account, `nextcloud:<login>@<host>`.
//!
//! What task E5.8 fills in (see there): the capabilities at the start and hourly (no
//! `notifications` app: health `missing_app`), a listing every 60 s with the ETag from the inbox's
//! cursors, the first one only of the last three days; matter `<app>:<object_type>:<object_id>`,
//! the notification ids as the server's ids; traits `app` and `person`, lane from
//! `rules::nextcloud_place`; a link only of the account's own origin; ids gone from the server
//! resolved.

use torro_inbox::Item;

use super::Sources;

/// Starts the notifications of every Nextcloud account on the current runtime and returns at once.
/// A muted source asks the server nothing. Nothing yet (task E5.8).
pub(super) fn start(_sources: Sources) {}

/// Deletes the notifications `ids` of the done entry `item` on the account's server, in the
/// background: spawn the calls on the current runtime and return at once. Failures only to the
/// log, with the account and a code. Nothing yet (task E5.8).
pub(super) fn settle_done(_sources: &Sources, _item: &Item, _ids: Vec<String>) {}
