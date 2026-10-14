//! The remote sources of the inbox (task E5.2, `docs/tasks/eingang.md` › E5): GitHub's
//! notifications ([`github`], task E5.4), Nextcloud's ([`nextcloud`], E5.8), Talk's conversations
//! ([`talk`], E5.9) and ntfy's topics ([`ntfy`], E5.13). Each module reads its server and files
//! what it finds through the inbox's storage for remote sources, so the four come about side by
//! side without changing `crate::inbox` at the same time:
//!
//! - [`crate::inbox::Inbox::file_remote`] files a report ([`crate::inbox::Remote`]), with the
//!   server's ids; [`crate::inbox::Inbox::resolve_remote`] takes off ids the server no longer
//!   holds, which is done by itself once none is left; [`crate::inbox::Inbox::open_remote`] lists
//!   the open entries with their ids, [`crate::inbox::Inbox::retire_remote`] closes those of
//!   accounts gone;
//! - [`crate::inbox::Inbox::cursor`] and `set_cursor` keep where a module stopped reading
//!   (`Last-Modified`, ETag, ntfy's `since`), [`crate::inbox::Inbox::set_source_health`] why a
//!   source is quiet (`inbox.list_sources`, `health`).
//!
//! What the inbox makes sure of, whatever a module hands it: the entries are owned by the
//! account's id, so no request merges into or resolves one (security rule 2); they are always
//! `untrusted` (rule 5); traits come through `torro_inbox::rules::remote_facets` only; a link is a
//! web page or nothing (rule 6; building it from checked parts is the module's, as task E5 says).
//! A muted source files nothing, but a module should not even ask its server then.
//!
//! **Reaching back** ([`settle_done`]): when the person or a client marks an entry done
//! (`inbox.set_item_done` with `done: true`, also dismissed), or makes a task or an event of it,
//! the core calls [`settle_done`] with the entry as stored; the module of its source then tells
//! the server, in the background: GitHub marks the thread done, Nextcloud deletes the
//! notifications, Talk marks the conversation read, ntfy is not told. Undo, snoozing, lanes,
//! hiding and muting never reach the server. The rights for it were checked before
//! (`crate::inbox`: write access to GitHub for a GitHub entry, before an approval and when the
//! command runs), so the modules check none.
//!
//! Logs name the account's id and an error code, never a title, a text, a trait's value or a
//! topic (rule 7).

mod github;
mod nextcloud;
mod ntfy;
mod talk;

use std::sync::Arc;

use torro_inbox::{Item, SourceKind, State};

use crate::account::github::GithubAccounts;
use crate::inbox::Inbox;
use crate::pim::PimService;

/// What the modules of the remote sources reach in the core. Cheap to clone: everything is shared.
#[derive(Clone)]
pub struct Sources {
    /// The inbox they file into.
    pub inbox: Arc<Inbox>,
    /// The GitHub accounts and their tokens (task E5.4: `GithubAccounts::notifications`).
    #[allow(dead_code, reason = "read by the source of task E5.4")]
    pub github: Arc<GithubAccounts>,
    /// The calendar service, which holds the Nextcloud accounts and their app passwords (tasks E5.8,
    /// E5.9: `PimService::nextcloud_access`).
    pub pim: Arc<PimService>,
}

/// Starts every remote source on the current runtime: each module runs its own loop and returns at
/// once. The core calls it once, from `Core::start`; tests start none.
pub fn start(sources: Sources) {
    github::start(sources.clone());
    nextcloud::start(sources.clone());
    talk::start(sources.clone());
    ntfy::start(sources);
}

/// Tells the server of `item`'s source that the entry is done (see the module docs), in the
/// background. Only for an entry of a remote source that is done; any other is left alone, so the
/// core may hand every entry it answered a done, a task or an event with. The server's ids are read
/// at once ([`Inbox::remote_ids`]), so a report filed meanwhile stays as it is on the server.
/// Failures go to the log; the entry stays done.
pub fn settle_done(sources: &Sources, item: &Item) {
    if !matches!(item.state, State::Done { .. }) {
        return;
    }
    let settle: fn(&Sources, &Item, Vec<String>) = match item.source.kind {
        SourceKind::Github => github::settle_done,
        SourceKind::Nextcloud => nextcloud::settle_done,
        SourceKind::Talk => talk::settle_done,
        SourceKind::Ntfy => ntfy::settle_done,
        SourceKind::Suite | SourceKind::Agent | SourceKind::Script | SourceKind::Mail => return,
    };
    match sources.inbox.remote_ids(&item.id) {
        Ok(ids) if !ids.is_empty() => settle(sources, item, ids),
        Ok(_) => {}
        Err(err) => eprintln!("torroworkplace: inbox: cannot read the ids of a done entry of {}: {err}", item.source),
    }
}
