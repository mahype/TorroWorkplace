//! GitHub's notifications as a source of the inbox (task E5.4; scaffold since task E5.2): one
//! entry per notification thread of each GitHub account, `github:<login>`.
//!
//! What task E5.4 fills in (see there): a loop per account at the larger of `X-Poll-Interval` and
//! 60 s with `Last-Modified` from the inbox's cursors, the first one only threads of the last three
//! days; matter and id the thread's id; traits and lane from `rules::github_place`; the link built
//! by `torro_pim_github::notifications::Thread::web_url`, never taken from the answer; a thread
//! missing from a complete listing resolved; health `signed_out` or `missing_scope`.

use torro_inbox::Item;

use super::Sources;

/// Starts the notifications of every GitHub account on the current runtime and returns at once.
/// A muted source asks GitHub nothing; a removed account ends its loop and its entries are retired
/// (`Inbox::retire_remote`). Nothing yet (task E5.4).
pub(super) fn start(_sources: Sources) {}

/// Marks the threads `ids` of the done entry `item` done on GitHub (`DELETE
/// /notifications/threads/{id}`, question 1 of E5), in the background: spawn the calls on the
/// current runtime and return at once. Failures only to the log, with the account and a code.
/// Nothing yet (task E5.4).
pub(super) fn settle_done(_sources: &Sources, _item: &Item, _ids: Vec<String>) {}
