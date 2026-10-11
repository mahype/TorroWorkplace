//! Whether a GitHub search stays inside the repositories a client was granted (task G.2b).
//!
//! GitHub runs every search with the account's full token. If the core asked first and dropped
//! foreign matches afterwards, the answer would still tell a client about repositories it may not
//! see: an error for a private repository that "does not exist" against no matches, counts,
//! `truncated`, the time GitHub took. So for a client limited to some repositories the core checks
//! each search **before** GitHub is asked ([`confined`]) and refuses it otherwise; filtering the
//! matches stays as a second guard.
//!
//! The check reads the search the way GitHub's issue search does, and refuses whatever it cannot
//! read with certainty: the repository qualifiers (`repo:`, `org:`, `user:`) of one search are
//! joined with OR and everything else narrows the search (AND), so a search whose positive
//! repository qualifiers are all granted finds nothing outside them, unless boolean operators or
//! parentheses regroup it, which is why those are refused.

use crate::address::name_part;

/// The qualifiers that choose repositories: `repo:owner/name`, `org:owner`, `user:owner`.
const SCOPES: [&str; 3] = ["repo", "org", "user"];

/// Qualifiers that might choose repositories as well, or that GitHub might read as such. Never
/// allowed in a client's search, so nothing widens it that the check does not know.
const UNSURE: [&str; 3] = ["owner", "organization", "repository"];

/// The answer a client gets for a search [`confined`] refuses (code `forbidden`). Fixed: it never
/// quotes the search.
pub const OUTSIDE: &str = "This key may only search the GitHub repositories it was granted: each search needs \
                           repo:owner/name, org:owner or user:owner for granted ones and no other, and no OR, NOT \
                           or parentheses.";

/// Whether `query` (a search as [`crate::address::to_query`] makes it) searches only what
/// `reaches` grants. `reaches` gets `owner/name` for `repo:` and `owner` for `org:` and `user:`
/// (every repository of that owner) and says whether the client may see it; the core passes
/// `Rights::reaches_repo`.
///
/// `true` only when all of these hold, else the search is refused:
///
/// - It has at least one positive `repo:`, `org:` or `user:` qualifier, and `reaches` grants each
///   of them. Negated ones (`-repo:owner/name`) only narrow the search and are allowed for any
///   repository.
/// - Qualifier names may be in any case (`REPO:`); the value may be quoted as a whole
///   (`repo:"owner/name"`) and must be a plain GitHub name: `owner/name` for `repo:`, `owner` for
///   the others, of ASCII letters, digits, `-`, `_` and `.` (so no `@me`, no wildcards, no lists).
/// - No word is `OR` or `NOT` (any case), no `(` or `)`, no `\`, no control characters, every
///   quote is closed. Quoted phrases are words like others (`"this or that"` is fine).
/// - No word holds a repository qualifier anywhere but at its start (`label:x-repo:a`, a quoted
///   `"repo:a"`, `--repo:a`), and none holds `owner:`, `organization:` or `repository:`.
/// - No word holds both a `:` and a character outside ASCII, and there is no fullwidth colon,
///   so no look-alike can turn into a qualifier GitHub reads but the check does not.
pub fn confined(query: &str, reaches: impl Fn(&str) -> bool) -> bool {
    if query.chars().any(|c| matches!(c, '(' | ')' | '\\' | '\u{FF1A}') || c.is_control()) {
        return false;
    }
    let Some(words) = words(query) else {
        return false;
    };
    let mut scoped = false;
    for word in words {
        match qualifier(word) {
            Word::Text => {}
            Word::Refused => return false,
            Word::Scope { negated: true, .. } => {}
            Word::Scope { negated: false, name } => {
                if !reaches(name) {
                    return false;
                }
                scoped = true;
            }
        }
    }
    scoped
}

/// What one word of a search is for the check.
#[derive(Debug, PartialEq, Eq)]
enum Word<'a> {
    /// Narrows the search, or is plain text.
    Text,
    /// A repository qualifier with its name (`owner/name` or `owner`).
    Scope { negated: bool, name: &'a str },
    /// Anything the check cannot read with certainty.
    Refused,
}

fn qualifier(word: &str) -> Word<'_> {
    let (negated, body) = match word.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, word),
    };
    if [word, body].iter().any(|w| w.eq_ignore_ascii_case("or") || w.eq_ignore_ascii_case("not")) {
        return Word::Refused;
    }
    if !word.is_ascii() && word.contains(':') {
        return Word::Refused;
    }
    // Quotes taken out, so a quoted key (`"repo":x`) or a quoted value cannot hide a qualifier.
    let flat = body.replace('"', "").to_ascii_lowercase();
    let at_start = |key: &str| flat.match_indices(&format!("{key}:")).all(|(at, _)| at == 0);
    let named = |key: &str| flat.contains(&format!("{key}:"));
    if UNSURE.iter().any(|key| named(key)) || !SCOPES.iter().all(|key| at_start(key)) {
        return Word::Refused;
    }
    let Some(key) = SCOPES.iter().find(|key| named(key)) else {
        return Word::Text;
    };
    // The key itself is never quoted.
    let Some(head) = body.get(..key.len() + 1).filter(|head| head.eq_ignore_ascii_case(&format!("{key}:"))) else {
        return Word::Refused;
    };
    let value = &body[head.len()..];
    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
    let valid = match *key {
        "repo" => value.split_once('/').is_some_and(|(owner, name)| name_part(owner) && name_part(name)),
        _ => name_part(value),
    };
    match valid {
        true => Word::Scope { negated, name: value },
        false => Word::Refused,
    }
}

/// The words of `query`: split at whitespace outside quotes, quotes kept. `None` when a quote is
/// left open, since GitHub might read the rest otherwise.
fn words(query: &str) -> Option<Vec<&str>> {
    let mut words = Vec::new();
    let (mut start, mut quoted) = (None, false);
    for (at, c) in query.char_indices() {
        if c.is_whitespace() && !quoted {
            if let Some(from) = start.take() {
                words.push(&query[from..at]);
            }
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        start.get_or_insert(at);
    }
    if let Some(from) = start {
        words.push(&query[from..]);
    }
    (!quoted).then_some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Granted: the repository `mahype/a` and every repository of `awesomemotive`, in any case,
    /// like `Rights::reaches_repo`.
    fn granted(name: &str) -> bool {
        let owner = name.split_once('/').map_or(name, |(owner, _)| owner);
        ["mahype/a", "awesomemotive"].iter().any(|g| g.eq_ignore_ascii_case(name) || g.eq_ignore_ascii_case(owner))
    }

    #[test]
    fn searches_inside_the_granted_repositories_pass() {
        for query in [
            "repo:mahype/a",
            "repo:mahype/a is:open is:issue label:bug",
            "REPO:Mahype/A",
            "Repo:mahype/a",
            "repo:\"mahype/a\" label:\"good first issue\"",
            "org:awesomemotive is:pr",
            "user:AwesomeMotive",
            "repo:awesomemotive/wpforms",
            "repo:mahype/a repo:awesomemotive/wpforms",
            "repo:mahype/a -repo:mahype/secret",
            "repo:mahype/a -org:other -user:someone -REPO:\"x/y\"",
            "repo:mahype/a \"this or that\" \"not now\"",
            "repo:mahype/a sort:created-asc",
            "repo:mahype/a AND is:open",
            "repo:mahype/a Fehlerbehebung für Käse",
            "  repo:mahype/a   is:open ",
            "repo:mahype/a review-requested:@me author:mahype",
        ] {
            assert!(confined(query, granted), "{query}");
        }
    }

    #[test]
    fn searches_that_might_reach_further_are_refused() {
        for query in [
            "",
            "is:open",
            "label:bug review-requested:@me",
            "-repo:mahype/secret",
            "repo:mahype/secret",
            "repo:mahype/a repo:mahype/secret",
            "repo:mahype/a org:mahype",
            "org:mahype",
            "user:mahype",
            "repo:mahype",
            "repo:",
            "repo:mahype/*",
            "repo:mahype/a,mahype/secret",
            "repo:mahype/a/b",
            "user:@me",
            "repo:mahype/a OR repo:mahype/secret",
            "repo:mahype/a OR label:bug",
            "repo:mahype/a or label:bug",
            "NOT repo:mahype/a",
            "repo:mahype/a not label:bug",
            "repo:mahype/a -OR label:bug",
            "(repo:mahype/a) label:bug",
            "repo:mahype/a (label:bug)",
            "repo:mahype/a label:\"P1 (urgent)\"",
            "\"repo:mahype/secret\" repo:mahype/a",
            "repo:mahype/a -\"repo:mahype/x\"",
            "\"repo\":mahype/a",
            "repo:mahype/a label:\"x repo:mahype/secret\"",
            "repo:mahype/a label:x-repo:mahype/secret",
            "repo:mahype/a label:org:x",
            "repo:mahype/a --repo:mahype/secret",
            "repo:mahype/a \"unclosed",
            "repo:\"mahype/a",
            "repo:\"mahype/a\"x",
            "repo:mahype/a -repo:\"weird name\"",
            "repo:mahype/a label:\\\"x",
            "repo:mahype/a owner:mahype",
            "repo:mahype/a organization:mahype",
            "repo:mahype/a repository:mahype/secret",
            "repo:mahype/a re\u{200B}po:mahype/secret",
            "repo:mahype/a repo\u{FF1A}mahype/secret",
            "repo:mahype/a\u{0}",
        ] {
            assert!(!confined(query, granted), "{query}");
        }
    }

    #[test]
    fn words_keep_quoted_phrases_together() {
        assert_eq!(words("a  \"b c\" d:\"e f\""), Some(vec!["a", "\"b c\"", "d:\"e f\""]));
        assert_eq!(words("a \"b"), None);
        assert_eq!(words(""), Some(vec![]));
    }

    #[test]
    fn the_refusal_names_the_rule_not_the_search() {
        assert!(OUTSIDE.contains("repo:") && OUTSIDE.contains("org:") && OUTSIDE.contains("user:"));
    }
}
