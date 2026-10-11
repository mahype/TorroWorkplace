//! The accounts as the apps see them, and how collection ids name their account.
//!
//! The core holds several accounts (task M1.1). Every collection id starts with the id of the
//! account it belongs to: `<account-id>/<provider id>`, for CalDAV the collection URL, e.g.
//! `nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/work/`.
//! Item hrefs stay the provider's own (the CalDAV URL of the item).
//!
//! Account ids may contain `/` themselves (a Nextcloud in a sub-path:
//! `nextcloud:anna@example.org/nextcloud`), so an id is never split at a `/`; it is matched
//! against the known accounts instead ([`account_of`], longest id wins).
//!
//! The front part of an id says which account an id *claims*, not that the account has such a
//! collection. Whatever sends an account's password (the core's writes) looks the id up exactly
//! among the collections the account listed (task M1.4); [`account_of`] is for showing and
//! sorting only.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The state of one account. One Nextcloud account per entry of [`PimAccount`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum AccountState {
    /// No account set up; the apps show the connect screen.
    NotConnected,
    /// Login Flow v2 is running: the user signs in at `login_url` in the browser.
    SigningIn { server: String, login_url: String },
    Connected {
        server: String,
        login_name: String,
        /// When data was last fetched from the server.
        last_sync: Option<DateTime<Utc>>,
        /// Why the last sync failed, if it did; the cached data stays usable.
        problem: Option<String>,
    },
}

/// One account behind the data ([`crate::PimSource::accounts`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimAccount {
    /// Stable id (`torro_core::Account::id`, e.g. `nextcloud:anna@cloud.example.org`), the
    /// front part of every collection id of this account ([`collection_id`]). Empty only for a
    /// sign-in that is still running and has no account yet.
    pub id: String,
    /// The name to show: `anna@cloud.example.org`; while signing in, the server as typed.
    pub label: String,
    /// The provider, as `torro_core::account::AccountKind` is written: `nextcloud` (later more).
    /// A string, not the enum, so an app keeps working when a newer core knows more kinds.
    pub kind: String,
    pub state: AccountState,
}

/// What an app that shows one account state shows (the connect screen, the tray's status line):
/// the first connected account, else the first sign-in that runs, else
/// [`AccountState::NotConnected`].
pub fn combined_state(accounts: &[PimAccount]) -> AccountState {
    let first = |wanted: fn(&AccountState) -> bool| accounts.iter().map(|a| &a.state).find(|s| wanted(s)).cloned();
    first(|s| matches!(s, AccountState::Connected { .. }))
        .or_else(|| first(|s| matches!(s, AccountState::SigningIn { .. })))
        .unwrap_or(AccountState::NotConnected)
}

/// The collection id of the provider's collection `local` (CalDAV: its URL) in account
/// `account_id`: `<account_id>/<local>`.
pub fn collection_id(account_id: &str, local: &str) -> String {
    format!("{account_id}/{local}")
}

/// The provider's own id (CalDAV: the URL) in `collection_id`, when the collection belongs to
/// account `account_id`; `None` otherwise. Does not look at other accounts: when one account id
/// is the start of another, ask [`account_of`] first.
pub fn local_id<'a>(collection_id: &'a str, account_id: &str) -> Option<&'a str> {
    collection_id.strip_prefix(account_id)?.strip_prefix('/').filter(|local| !local.is_empty())
}

/// The account `collection_id` names: the one with the longest id it starts with (`<id>/`), so
/// `nextcloud:anna@example.org/nc` wins over `nextcloud:anna@example.org`. Accounts with an
/// empty id (a running sign-in) own nothing. Only for showing and sorting: the rest of the id is
/// not checked, so never pick the account to send a password with this way (task M1.4).
pub fn account_of<'a, A>(collection_id: &str, accounts: &'a [A], id: impl Fn(&A) -> &str) -> Option<&'a A> {
    accounts
        .iter()
        .filter(|account| !id(account).is_empty() && local_id(collection_id, id(account)).is_some())
        .max_by_key(|account| id(account).len())
}

/// A collection id from before several accounts existed (task M1.1): a bare CalDAV URL. Ids
/// with an account in front never start with a URL scheme.
pub fn is_legacy_id(id: &str) -> bool {
    id.starts_with("https://") || id.starts_with("http://")
}

/// `id` with `account_id` in front when it is a legacy id ([`is_legacy_id`]); `None` when it
/// needs no change. Before M1.1 there was exactly one account, so the owner is not in doubt.
pub fn qualify_legacy_id(id: &str, account_id: &str) -> Option<String> {
    is_legacy_id(id).then(|| collection_id(account_id, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, state: AccountState) -> PimAccount {
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state }
    }

    fn connected(login_name: &str) -> AccountState {
        AccountState::Connected {
            server: "https://cloud".into(),
            login_name: login_name.into(),
            last_sync: None,
            problem: None,
        }
    }

    fn signing_in() -> AccountState {
        AccountState::SigningIn { server: "cloud".into(), login_url: "https://cloud/login".into() }
    }

    #[test]
    fn the_combined_state_prefers_a_connected_account() {
        assert_eq!(combined_state(&[]), AccountState::NotConnected);
        assert_eq!(combined_state(&[account("", signing_in())]), signing_in());
        let both = [account("", signing_in()), account("a", connected("anna")), account("b", connected("bob"))];
        assert_eq!(combined_state(&both), connected("anna"));
    }

    #[test]
    fn collection_ids_carry_their_account() {
        let id = collection_id("nextcloud:anna@cloud", "https://cloud/dav/work/");
        assert_eq!(id, "nextcloud:anna@cloud/https://cloud/dav/work/");
        assert_eq!(local_id(&id, "nextcloud:anna@cloud"), Some("https://cloud/dav/work/"));
        assert_eq!(local_id(&id, "nextcloud:anna@clou"), None, "a prefix of the id is not the account");
        assert_eq!(local_id(&id, "nextcloud:bob@cloud"), None);
        assert_eq!(local_id("nextcloud:anna@cloud/", "nextcloud:anna@cloud"), None);
    }

    #[test]
    fn the_longest_account_id_owns_a_collection() {
        let accounts = ["nextcloud:anna@example.org", "nextcloud:anna@example.org/nc", ""];
        let owner = |id: &str| account_of(id, &accounts, |a| a).copied();
        assert_eq!(owner("nextcloud:anna@example.org/nc/https://example.org/nc/dav/work/"), Some(accounts[1]));
        assert_eq!(owner("nextcloud:anna@example.org/https://example.org/dav/work/"), Some(accounts[0]));
        assert_eq!(owner("https://example.org/dav/work/"), None);
        assert_eq!(owner("/https://example.org/dav/work/"), None, "the sign-in without id owns nothing");
    }

    #[test]
    fn only_bare_urls_are_rewritten() {
        let owner = "nextcloud:anna@cloud";
        assert_eq!(
            qualify_legacy_id("https://cloud/dav/work/", owner),
            Some("nextcloud:anna@cloud/https://cloud/dav/work/".into())
        );
        assert_eq!(qualify_legacy_id("http://cloud/dav/work/", owner), Some(format!("{owner}/http://cloud/dav/work/")));
        assert_eq!(qualify_legacy_id("nextcloud:anna@cloud/https://cloud/dav/work/", owner), None);
        assert_eq!(qualify_legacy_id("/cal/work/", owner), None);
    }
}
