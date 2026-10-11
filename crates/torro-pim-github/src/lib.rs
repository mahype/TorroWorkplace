//! GitHub filter lists (task G.2, concept `docs/github.md`): saved GitHub searches whose
//! matches the core keeps as a task list.
//!
//! - [`address`]: what a person pastes, a search or the address of an issue list on github.com,
//!   becomes the search the list runs ([`address::to_query`]); issue addresses and link ids become
//!   an [`address::IssueRef`].
//! - [`graphql`]: the GraphQL documents the core sends and how GitHub's answers are read into
//!   [`Item`]s. Pure functions, tested with recorded answers in `tests/fixtures/`.
//! - [`item`]: an issue or pull request as GitHub reports it, and as a [`torro_pim::Todo`].
//! - [`api`]: asking GitHub over HTTP: a search with its pages (up to 1,000 matches), the pinned
//!   items, a whole list.
//! - [`lists`]: the definitions in `filter-lists.json` (config directory, one entry per account)
//!   and the checks of names, colours and searches.
//!
//! Who may see which repository is not decided here: the core filters every [`Item`] by its
//! repository ([`Item::repo`]) before it reads it further.

pub mod address;
pub mod api;
pub mod graphql;
pub mod item;
pub mod lists;

use std::fmt;

pub use address::IssueRef;
pub use api::Api;
pub use item::Item;
pub use lists::FilterList;

/// Why a filter list could not be built or fetched. No message ever holds the token. GitHub's
/// own messages ([`Error::Github`]) may quote a search, so logs carry only [`Error::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// What the caller sent does not fit (empty name, no search, an address that is not an
    /// issue list); the text says what to change.
    Input(String),
    /// GitHub refused the token (HTTP 401).
    Refused,
    /// GitHub could not be reached.
    Offline,
    /// GitHub answered, but not with what was asked for (rate limit, an error in the answer).
    Github(String),
}

impl Error {
    /// The error code of the API answer (`docs/api.md`).
    pub fn code(&self) -> &'static str {
        match self {
            Error::Refused => "auth",
            Error::Offline => "offline",
            Error::Input(_) | Error::Github(_) => "error",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Input(message) | Error::Github(message) => f.write_str(message),
            Error::Refused => f.write_str("GitHub no longer accepts the token. Sign in to GitHub again."),
            Error::Offline => f.write_str("GitHub could not be reached. Check the internet connection."),
        }
    }
}

impl std::error::Error for Error {}
