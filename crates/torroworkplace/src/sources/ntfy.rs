//! ntfy's topics as a source of the inbox (task E5.13; scaffold since task E5.2): one entry per
//! matter of each ntfy account, `ntfy:<host>`.
//!
//! What task E5.13 fills in (see there): one subscription per account for all its topics, with
//! `since` from the inbox's cursors; matter `<topic>:<title>`, the message id as the server's id;
//! kind and lane from `rules::ntfy_place` (priority 5 an alert); traits `topic` (the person's label,
//! never the topic's name), `priority`, `tag`; the link only an `http`/`https` `click`.

use torro_inbox::Item;

use super::Sources;

/// Starts the subscriptions of every ntfy account on the current runtime and returns at once. A
/// muted source or a removed account ends its subscription. Nothing yet (task E5.13).
pub(super) fn start(_sources: Sources) {}

/// ntfy is never told of a done entry: a topic has nothing to mark. Kept so every remote source
/// answers [`super::settle_done`] the same way.
pub(super) fn settle_done(_sources: &Sources, _item: &Item, _ids: Vec<String>) {}
