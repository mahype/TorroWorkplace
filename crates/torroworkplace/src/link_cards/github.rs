//! Issues and pull requests on GitHub (issue #790, `docs/inhaltskarten.md` "GitHub Issue / PR"): their links
//! ([`parse`]) and their cards ([`resolve`]) through a connected GitHub account (`torro_pim_github::api::Api`), only
//! with read access to `github` and the repository for the reader. Without an account, or when GitHub fails, the
//! link is a page ([`super::web`]).

use torro_chat::{GithubState, LinkCard, LinkCardKind, LinkGithub};
use torro_ipc::api::{Access, Area};
use torro_pim::model::{GitHubKind, GitHubState};
use torro_pim_github::{IssueRef, Item};
use url::Url;

use super::LinkKind;
use crate::account::github::GithubAccounts;
use crate::pim::Reach;

/// What `url` names on `github.com`: `/<owner>/<repo>/issues/<number>` or `/<owner>/<repo>/pull/<number>`, also
/// with more after it (`/pull/12/files`) ([`LinkKind::Github`]); `None` for any other address, another host as well.
pub fn parse(url: &Url) -> Option<LinkKind> {
    if !matches!(url.host_str(), Some("github.com" | "www.github.com")) {
        return None;
    }
    let mut segments = url.path_segments()?;
    let (owner, repo, kind, number) = (segments.next()?, segments.next()?, segments.next()?, segments.next()?);
    let is_pull = match kind {
        "issues" => false,
        "pull" => true,
        _ => return None,
    };
    if !is_owner(owner) || !is_repo(repo) {
        return None;
    }
    let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    let number = digits.then(|| number.parse::<u64>().ok()).flatten().filter(|number| *number > 0)?;
    Some(LinkKind::Github { repo: format!("{owner}/{repo}"), number, is_pull })
}

/// A GitHub account or organisation name: letters, digits and `-`, at most 39.
fn is_owner(name: &str) -> bool {
    (1..=39).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A repository name: letters, digits, `-`, `_` and `.`, at most 100, not `.` or `..`.
fn is_repo(name: &str) -> bool {
    let allowed = name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte));
    (1..=100).contains(&name.len()) && allowed && name != "." && name != ".."
}

/// Whether `reach` may see the issues and pull requests of `repo`: the app key (the person) always; a client with
/// read access to `github` and the repository or its owner in its rights ([`torro_ipc::api::Rights::reaches_repo`]).
fn reads_repo(reach: Reach<'_>, repo: &str) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.area(Area::Github).allows(Access::Read) && rights.reaches_repo(repo),
    }
}

/// The card of the issue or pull request `number` of `repo` (`owner/name`), linked as `link`, for `reach`.
///
/// - Checks the right first (read access to `github` and the repository for a client): without it nothing is asked
///   of GitHub and the answer is `None`.
/// - Then asks GitHub through each connected account in turn (`Api::items`) until one sees the item; a repository
///   GitHub reports under another name (renamed, moved) must be within the right as well.
/// - `None`: no right, no connected account, no account sees it, or GitHub failed. The caller then treats the link
///   as the page it is ([`LinkKind::Web`], with Settings › Privacy, [`super::web`]): no detail of the item.
///
/// Whether it is a pull request is what GitHub reports, not what the link says (GitHub sends `/pull/<n>` of an issue
/// to the issue). Logs no address. Each call asks GitHub; holding the card is the caller's.
pub async fn resolve(
    github: &GithubAccounts,
    reach: Reach<'_>,
    link: &str,
    repo: &str,
    number: u64,
) -> Option<LinkCard> {
    if !reads_repo(reach, repo) {
        return None;
    }
    let wanted = IssueRef { repo: repo.to_owned(), number };
    for account in github.infos().await {
        let Some(api) = github.api(&account.id).await else { continue };
        // A failure of one account (offline, refused token) leaves the others to try.
        let Ok(items) = api.items(std::slice::from_ref(&wanted)).await else { continue };
        let Some(item) = items.into_iter().find(|item| item.id.number == number) else { continue };
        if !reads_repo(reach, item.repo()) {
            return None;
        }
        return Some(card_of(link, item));
    }
    None
}

/// The card of `item`. A closed issue counts as done ([`GithubState::Merged`], `CircleCheck` in `torro-design`):
/// GitHub's answer does not say whether it was closed as not planned.
fn card_of(link: &str, item: Item) -> LinkCard {
    let is_pull = item.kind == GitHubKind::PullRequest;
    let state = match item.state {
        GitHubState::Open if is_pull && item.draft => GithubState::Draft,
        GitHubState::Open => GithubState::Open,
        GitHubState::Merged => GithubState::Merged,
        GitHubState::Closed if is_pull => GithubState::Closed,
        GitHubState::Closed => GithubState::Merged,
    };
    let domain = Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default();
    let github = LinkGithub { repo: item.id.repo, number: item.id.number, is_pull, state };
    LinkCard {
        url: link.to_owned(),
        title: item.title.split_whitespace().collect::<Vec<_>>().join(" "),
        description: None,
        domain,
        kind: LinkCardKind::Github,
        github: Some(Box::new(github)),
        ..LinkCard::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_core::account::Secret;
    use torro_ipc::api::{RightsTemplate, Selection};

    use super::super::classify;
    use super::*;
    use crate::account::github::tests::{MemoryTokens, NO_CLI, fake_github, service, viewer};

    fn kind(link: &str) -> Option<LinkKind> {
        parse(&Url::parse(link).expect("an address"))
    }

    #[test]
    fn issues_and_pull_requests_on_github_are_github_and_anything_else_a_page() {
        let issue = Some(LinkKind::Github { repo: "torro/workplace".into(), number: 142, is_pull: false });
        let pull = Some(LinkKind::Github { repo: "torro/workplace".into(), number: 143, is_pull: true });
        assert_eq!(kind("https://github.com/torro/workplace/issues/142"), issue);
        assert_eq!(kind("https://github.com/torro/workplace/issues/142#issuecomment-1"), issue);
        assert_eq!(kind("https://www.github.com/torro/workplace/pull/143/files"), pull);
        for none in [
            "https://github.com/torro/workplace",
            "https://github.com/torro/workplace/issues",
            "https://github.com/torro/workplace/issues/new",
            "https://github.com/torro/workplace/discussions/5",
            "https://github.com/torro/workplace/pull/0",
            "https://gist.github.com/torro/workplace/issues/1",
            "https://github.example.org/torro/workplace/issues/1",
        ] {
            assert_eq!(kind(none), None, "{none}");
        }
        let own = Some("https://cloud.example.org");
        assert_eq!(Some(classify("https://github.com/torro/workplace/pull/143", own)), pull);
        assert_eq!(Some(classify("https://github.com/torro/workplace/pull/143", None)), pull);
    }

    /// GitHub's answer for one item, or the viewer for the sign-in.
    async fn connected() -> (Arc<GithubAccounts>, Arc<std::sync::Mutex<Vec<crate::account::github::tests::Seen>>>) {
        let item = include_str!("../../tests/fixtures/link_cards/github_pull.json");
        let (url, record) = fake_github(move |seen| match seen.body.contains("viewer") {
            true => (200, Some("repo"), viewer("anna-berg")),
            false => (200, None, item.to_string()),
        })
        .await;
        let dir = tempfile::tempdir().expect("temp dir");
        let accounts = Arc::new(service(dir.path(), &MemoryTokens::default(), &url, NO_CLI));
        accounts.connect_token(Secret::new("ghp_testTOKEN")).await.expect("connected");
        record.lock().expect("record").clear();
        (accounts, record)
    }

    const LINK: &str = "https://github.com/torro/workplace/pull/143";

    #[tokio::test]
    async fn a_connected_account_gives_the_card_of_a_draft_pull_request() {
        let (accounts, _record) = connected().await;
        let card = resolve(&accounts, Reach::Suite, LINK, "torro/workplace", 143).await.expect("a card");
        assert_eq!(
            (card.kind, card.title.as_str(), card.domain.as_str()),
            (LinkCardKind::Github, "Fix the week view", "github.com")
        );
        let github = card.github.expect("details");
        assert_eq!(
            *github,
            LinkGithub { repo: "torro/workplace".into(), number: 143, is_pull: true, state: GithubState::Draft }
        );
    }

    #[tokio::test]
    async fn without_an_account_or_when_github_fails_it_is_a_page() {
        let dir = tempfile::tempdir().expect("temp dir");
        let none = service(dir.path(), &MemoryTokens::default(), "http://127.0.0.1:9/graphql", NO_CLI);
        assert_eq!(resolve(&none, Reach::Suite, LINK, "torro/workplace", 143).await, None);

        let (url, _record) = fake_github(|seen| match seen.body.contains("viewer") {
            true => (200, Some("repo"), viewer("anna-berg")),
            false => (502, None, "{}".into()),
        })
        .await;
        let failing = service(dir.path(), &MemoryTokens::default(), &url, NO_CLI);
        failing.connect_token(Secret::new("ghp_testTOKEN")).await.expect("connected");
        assert_eq!(resolve(&failing, Reach::Suite, LINK, "torro/workplace", 143).await, None);
    }

    /// A client without read access to `github`, or without the repository, gets no card, and GitHub is not asked.
    #[tokio::test]
    async fn a_client_without_the_right_to_github_gets_a_page_and_nothing_is_asked() {
        let (accounts, record) = connected().await;
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.areas.remove(&Area::Github);
        assert_eq!(resolve(&accounts, Reach::Client(&rights), LINK, "torro/workplace", 143).await, None);

        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.github_repos = Selection::Only(vec!["someone/else".into()]);
        assert_eq!(resolve(&accounts, Reach::Client(&rights), LINK, "torro/workplace", 143).await, None);
        assert!(record.lock().expect("record").is_empty(), "nothing asked of GitHub before the right");

        rights.github_repos = Selection::Only(vec!["torro".into()]);
        let card = resolve(&accounts, Reach::Client(&rights), LINK, "torro/workplace", 143).await;
        assert!(card.is_some(), "with the owner in the rights");
    }
}
