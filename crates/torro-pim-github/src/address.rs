//! What a person pastes into a filter list, turned into what GitHub searches.
//!
//! The search is the truth (`docs/github.md`, "Filterliste"). A person may paste one, or the
//! whole address of an issue list on github.com; the core takes `q=` out of it and adds what the
//! page adds without saying so: `repo:`/`org:` from the path and `is:issue`/`is:pr` for the issue
//! and pull request pages, but only when the search does not name them itself.

use reqwest::Url;
use torro_pim::model::Link;

use crate::Error;

/// One issue or pull request: repository `owner/name` and number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueRef {
    /// `owner/name` as written; GitHub compares it without regard to case.
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    /// The link id of the item: `github:<owner>/<repo>#<number>` ([`Link::github`]).
    pub fn link_id(&self) -> String {
        Link::github(&self.repo, self.number).id
    }

    /// The same item as `other`, whatever the case of the repository.
    pub fn same(&self, other: &IssueRef) -> bool {
        self.number == other.number && self.repo.eq_ignore_ascii_case(&other.repo)
    }

    /// `owner` and `name` of the repository.
    pub fn owner_and_name(&self) -> (&str, &str) {
        self.repo.split_once('/').unwrap_or((&self.repo, ""))
    }
}

/// The search a filter list runs for `input`: a search as it is (spaces tidied), or the search
/// behind a github.com address of an issue list:
///
/// - `github.com/<owner>/<repo>/issues?q=…` and `…/pulls?q=…`: `q`, with `repo:<owner>/<repo>`
///   and `is:issue`/`is:pr` in front unless `q` names a repository, organisation or user (`repo:`,
///   `org:`, `user:`) or a type (`is:issue`, `is:pr`, `type:…`) itself; without `q`, the page's
///   default `is:open`;
/// - `github.com/orgs/<org>/issues` and `…/pulls`: the same with `org:<org>`;
/// - `github.com/issues` and `github.com/pulls` (the person's own lists): `q`, without it the
///   page's default `is:open author:@me archived:false`, with the type in front;
/// - `github.com/search?q=…&type=issues` (or `pullrequests`): `q` with the type; without `type`
///   issues and pull requests.
///
/// Anything else is [`Error::Input`]: an empty input, another host, a search for repositories
/// or code, the address of a single issue (pin it instead) or of a page that is no issue list.
pub fn to_query(input: &str) -> Result<String, Error> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::Input(
            "A search is empty. Enter a GitHub search or paste the address of an issue list.".into(),
        ));
    }
    let lower = input.to_ascii_lowercase();
    let address = if lower.starts_with("https://") || lower.starts_with("http://") {
        input.to_string()
    } else if lower.starts_with("github.com/") || lower.starts_with("www.github.com/") {
        format!("https://{input}")
    } else {
        return Ok(tidy(input));
    };
    let url = Url::parse(&address).map_err(|_| not_a_list())?;
    if !matches!(url.host_str(), Some("github.com" | "www.github.com")) {
        return Err(Error::Input("Only addresses on github.com can be pasted here.".into()));
    }
    let param = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
    let q = param("q").map(|q| tidy(&q)).filter(|q| !q.is_empty());
    let segments: Vec<&str> = url.path_segments().map(|s| s.filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let (scope, kind, default) = match segments.as_slice() {
        ["issues"] => (None, Some("is:issue"), MINE),
        ["pulls"] => (None, Some("is:pr"), MINE),
        ["search"] => {
            let kind = match param("type").as_deref() {
                None => None,
                Some(t) if t.eq_ignore_ascii_case("issues") => Some("is:issue"),
                Some(t) if t.eq_ignore_ascii_case("pullrequests") => Some("is:pr"),
                Some(_) => return Err(Error::Input("Only searches for issues and pull requests can be used.".into())),
            };
            (None, kind, "")
        }
        ["orgs", org, page @ ("issues" | "pulls")] if name_part(org) => {
            (Some(format!("org:{org}")), Some(type_of(page)), OPEN)
        }
        [owner, repo, page @ ("issues" | "pulls")] if name_part(owner) && name_part(repo) => {
            (Some(format!("repo:{owner}/{repo}")), Some(type_of(page)), OPEN)
        }
        [_, _, "issues" | "pull", number] if number.parse::<u64>().is_ok() => {
            return Err(Error::Input("This is the address of one issue. Pin it to a list instead.".into()));
        }
        _ => return Err(not_a_list()),
    };
    let q = q.unwrap_or_else(|| default.to_string());
    let words: Vec<String> = q.split_whitespace().map(str::to_ascii_lowercase).collect();
    let names = |prefixes: &[&str]| words.iter().any(|w| prefixes.iter().any(|p| w.starts_with(p)));
    let mut parts = Vec::new();
    if let Some(scope) = scope.filter(|_| !names(&["repo:", "org:", "user:"])) {
        parts.push(scope);
    }
    if let Some(kind) = kind.filter(|_| !names(&["is:issue", "is:pr", "is:pull-request", "type:"])) {
        parts.push(kind.to_string());
    }
    parts.push(q);
    Ok(tidy(&parts.join(" ")))
}

/// What GitHub's issue pages of a repository or organisation show without a search.
const OPEN: &str = "is:open";
/// What `github.com/issues` and `github.com/pulls` show without a search.
const MINE: &str = "is:open author:@me archived:false";

fn type_of(page: &str) -> &'static str {
    if page == "pulls" { "is:pr" } else { "is:issue" }
}

fn not_a_list() -> Error {
    Error::Input(
        "This address is not a list of issues or pull requests on github.com. Open the list on GitHub and copy its \
         address."
            .into(),
    )
}

/// One space between words, none around.
fn tidy(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An owner or repository name as GitHub allows it: ASCII letters, digits, `-`, `_` and `.`.
pub(crate) fn name_part(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The issue or pull request `input` names: a link id (`github:owner/repo#12`), `owner/repo#12`
/// or its address on github.com (`…/owner/repo/issues/12`, `…/pull/12`, with or without
/// `https://`, anything after the number ignored). `None` for anything else.
pub fn issue_ref(input: &str) -> Option<IssueRef> {
    let input = input.trim();
    if let Some((repo, number)) = (Link { id: input.to_string(), github: None }).github_ref() {
        return Some(IssueRef { repo: repo.to_string(), number });
    }
    if let Some((repo, number)) = input.split_once('#')
        && let Some((owner, name)) = repo.split_once('/')
        && name_part(owner)
        && name_part(name)
    {
        return Some(IssueRef { repo: repo.to_string(), number: number.parse().ok()? });
    }
    let rest = input.strip_prefix("https://").or_else(|| input.strip_prefix("http://")).unwrap_or(input);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = rest.strip_prefix("github.com/")?;
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/');
    let (owner, name, page, number) = (segments.next()?, segments.next()?, segments.next()?, segments.next()?);
    if !(name_part(owner) && name_part(name) && matches!(page, "issues" | "pull")) {
        return None;
    }
    Some(IssueRef { repo: format!("{owner}/{name}"), number: number.parse().ok()? })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    /// One case of `tests/fixtures/addresses.json`: what a person pastes and the search it
    /// becomes, or `null` when it is refused.
    #[derive(Deserialize)]
    struct Case {
        input: String,
        query: Option<String>,
    }

    #[test]
    fn addresses_and_searches_become_the_search_of_the_fixture() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../tests/fixtures/addresses.json")).expect("valid fixture");
        assert!(cases.len() >= 15);
        for case in cases {
            let got = to_query(&case.input);
            match case.query {
                Some(query) => assert_eq!(got.as_deref(), Ok(query.as_str()), "{}", case.input),
                None => assert!(matches!(got, Err(Error::Input(_))), "{} gave {got:?}", case.input),
            }
        }
    }

    #[test]
    fn refusals_say_what_to_do_without_quoting_the_input() {
        let err = to_query("https://example.org/mahype/secret/issues?q=label:private").expect_err("not github");
        assert!(!err.to_string().contains("private") && !err.to_string().contains("secret"));
        assert_eq!(err.code(), "error");
    }

    #[test]
    fn issues_are_named_by_link_id_short_form_or_address() {
        let wanted = IssueRef { repo: "mahype/TorroWorkplace".into(), number: 12 };
        for input in [
            "github:mahype/TorroWorkplace#12",
            "mahype/TorroWorkplace#12",
            "https://github.com/mahype/TorroWorkplace/issues/12",
            "https://github.com/mahype/TorroWorkplace/pull/12/files",
            "github.com/mahype/TorroWorkplace/issues/12#issuecomment-1",
            " https://www.github.com/mahype/TorroWorkplace/issues/12?x=1 ",
        ] {
            assert_eq!(issue_ref(input), Some(wanted.clone()), "{input}");
        }
        for input in [
            "https://github.com/mahype/TorroWorkplace/issues",
            "https://example.org/mahype/TorroWorkplace/issues/12",
            "https://github.com/mahype/Torro Workplace/issues/12",
            "mahype#12",
            "github:a/b#x",
        ] {
            assert_eq!(issue_ref(input), None, "{input}");
        }
        assert_eq!(wanted.link_id(), "github:mahype/TorroWorkplace#12");
        assert!(wanted.same(&IssueRef { repo: "Mahype/torroworkplace".into(), number: 12 }));
        assert_eq!(wanted.owner_and_name(), ("mahype", "TorroWorkplace"));
    }
}
