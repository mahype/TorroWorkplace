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
//! - [`scope`]: whether a search stays inside the repositories a client was granted, checked
//!   before GitHub is asked (task G.2b).
//!
//! Who may see which repository is not decided here: the core checks a client's searches with
//! [`scope::confined`] and filters every [`Item`] by its repository ([`Item::repo`]) before it
//! reads it further. Clients never get GitHub's own words, only [`Error::public_message`].

pub mod address;
pub mod api;
pub mod graphql;
pub mod item;
pub mod lists;
pub mod scope;

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

/// What a client reads instead of GitHub's own message ([`Error::Github`]): that message may
/// quote the search or tell about repositories the client may not see.
pub const GITHUB_FAILED: &str = "GitHub could not run the search. Check the search, or try again later.";

impl Error {
    /// The message for a client key: fixed per code. [`Error::Input`] is the core's own text and
    /// never quotes the input; [`Error::Github`] becomes [`GITHUB_FAILED`].
    pub fn public_message(&self) -> String {
        match self {
            Error::Github(_) => GITHUB_FAILED.to_string(),
            other => other.to_string(),
        }
    }
}

/// A list's problem (`source.problem`, the message of the error its last fetch ended with) as a
/// client may read it: the fixed texts of [`Error::Refused`] and [`Error::Offline`] stay, any
/// other becomes [`GITHUB_FAILED`].
pub fn public_problem(problem: &str) -> String {
    let fixed = [Error::Refused.to_string(), Error::Offline.to_string()];
    match fixed.iter().any(|text| text == problem) {
        true => problem.to_string(),
        false => GITHUB_FAILED.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_get_fixed_messages_never_githubs_words() {
        let github = Error::Github("GitHub could not run the search: mahype/secret does not exist".into());
        assert_eq!(github.public_message(), GITHUB_FAILED);
        assert_eq!(public_problem(&github.to_string()), GITHUB_FAILED);
        for fixed in [Error::Refused, Error::Offline] {
            assert_eq!(fixed.public_message(), fixed.to_string());
            assert_eq!(public_problem(&fixed.to_string()), fixed.to_string());
        }
        let input = Error::Input("A filter list needs a name.".into());
        assert_eq!(input.public_message(), input.to_string());
    }
}
