//! Issues and pull requests on GitHub (issue #790, `docs/inhaltskarten.md` "GitHub Issue / PR"): their links, and –
//! with strand 2c of issue #784 – their cards through the connected GitHub account (`torro_pim_github::api::Api`),
//! only with read access to `github` and the repository for the reader. Without an account, or when GitHub fails,
//! the link is a page ([`super::web`]).

use super::LinkKind;

/// What `url` names on `github.com`: `/<owner>/<repo>/issues/<number>` or `/<owner>/<repo>/pull/<number>`
/// ([`LinkKind::Github`]); `None` for any other address, another host as well.
///
/// Strand 2c (issue #790) fills it in; until then no address is an issue.
pub fn parse(url: &url::Url) -> Option<LinkKind> {
    let _ = url;
    None
}
