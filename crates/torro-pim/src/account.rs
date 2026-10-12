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

use crate::CollectionKind;

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
    /// For a GitHub account the OAuth scopes of its token, as the core lists them
    /// (`torro_ipc::AccountInfo::scopes`): `None` when GitHub does not tell (a fine-grained
    /// token) and for every other kind. Read by [`PimAccount::may_close_issues`].
    pub scopes: Option<Vec<String>>,
}

impl PimAccount {
    /// Whether ticking an issue of this account's filter lists may close it on GitHub (task G.5):
    /// a classic token or the GitHub CLI's sign-in needs the scope `repo` or `public_repo`. When the
    /// scopes are unknown (`None`: a fine-grained token) it may: the app tries and shows GitHub's
    /// refusal. Every other kind of account: `true`, it holds no filter lists.
    pub fn may_close_issues(&self) -> bool {
        match &self.scopes {
            Some(scopes) if self.kind == GITHUB_KIND => {
                scopes.iter().any(|scope| scope == "repo" || scope == "public_repo")
            }
            _ => true,
        }
    }

    /// The host of the account, the short name a sidebar shows it by (task K.31): the part of
    /// `label` after the last `@`, `cloud.example.org` for `anna@cloud.example.org`. Without an
    /// `@`, or with nothing after it, the whole `label`, so the name is never shown empty.
    pub fn host(&self) -> &str {
        match self.label.rsplit_once('@') {
            Some((_, host)) if !host.is_empty() => host,
            _ => &self.label,
        }
    }
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

/// [`combined_state`] of only those accounts that can hold collections of the kind `what`, which
/// is what an app showing one kind of collection shows: the calendar asks with
/// [`CollectionKind::Events`], the tasks app with [`CollectionKind::Todos`]. A GitHub account holds
/// task lists only (its filter lists, task G.2), so a setup with nothing but GitHub is
/// [`AccountState::NotConnected`] for the calendar and connected for the tasks app (task G.3).
/// Kinds this version does not know count for every `what`, so a newer core's accounts still show.
pub fn combined_state_for(accounts: &[PimAccount], what: CollectionKind) -> AccountState {
    let fitting: Vec<PimAccount> = accounts.iter().filter(|a| may_hold(&a.kind, what)).cloned().collect();
    combined_state(&fitting)
}

/// Whether an account of `kind` ([`PimAccount::kind`]) can hold collections of the kind `what`:
/// GitHub only task lists, every other kind both.
fn may_hold(kind: &str, what: CollectionKind) -> bool {
    kind != GITHUB_KIND || !what.has_events()
}

/// [`PimAccount::kind`] of a GitHub account.
pub const GITHUB_KIND: &str = "github";

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
        PimAccount { id: id.into(), label: id.into(), kind: "nextcloud".into(), state, scopes: None }
    }

    #[test]
    fn closing_issues_needs_a_writing_scope_unless_github_does_not_tell() {
        let github = |scopes: Option<&[&str]>| PimAccount {
            kind: GITHUB_KIND.into(),
            scopes: scopes.map(|scopes| scopes.iter().map(|s| s.to_string()).collect()),
            ..account("github:mahype", connected("mahype"))
        };
        assert!(github(Some(&["repo", "read:org"])).may_close_issues());
        assert!(github(Some(&["public_repo"])).may_close_issues());
        assert!(!github(Some(&["read:org", "repo:status"])).may_close_issues(), "reading is not enough");
        assert!(!github(Some(&[])).may_close_issues(), "a token without scopes");
        assert!(github(None).may_close_issues(), "fine-grained: tried, GitHub decides");
        assert!(account("nextcloud:anna@cloud", connected("anna")).may_close_issues());
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
    fn a_github_account_counts_for_task_lists_only() {
        let github = PimAccount { kind: GITHUB_KIND.into(), ..account("github:mahype", connected("mahype")) };
        let only_github = [github.clone()];
        assert_eq!(combined_state_for(&only_github, CollectionKind::Events), AccountState::NotConnected);
        assert_eq!(combined_state_for(&only_github, CollectionKind::Todos), connected("mahype"));
        let both = [github, account("nextcloud:anna@cloud", connected("anna"))];
        assert_eq!(combined_state_for(&both, CollectionKind::Events), connected("anna"), "the Nextcloud one");
        assert_eq!(combined_state_for(&both, CollectionKind::Todos), connected("mahype"), "the first one");
        let newer = [PimAccount { kind: "caldav".into(), ..account("caldav:x", connected("x")) }];
        assert_eq!(combined_state_for(&newer, CollectionKind::Events), connected("x"), "unknown kinds count");
    }

    #[test]
    fn the_host_is_the_part_after_the_last_at() {
        let host = |label: &str| account(label, connected("anna")).host().to_string();
        assert_eq!(host("anna@cloud.example.org"), "cloud.example.org");
        assert_eq!(host("anna@example.org@cloud.example.org"), "cloud.example.org", "the last @ counts");
        assert_eq!(host("cloud.example.org"), "cloud.example.org", "without @ the label itself");
        assert_eq!(host("anna@"), "anna@", "nothing after the @: the label itself, not an empty name");
        assert_eq!(host(""), "", "an empty label stays empty");
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
