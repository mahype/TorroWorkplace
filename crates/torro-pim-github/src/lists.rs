//! The definitions of the filter lists: `filter-lists.json` in the core's config directory, one
//! entry per GitHub account. GitHub keeps no saved searches an API could read, so the core does
//! (`docs/github.md`, "Filterliste").
//!
//! A list's id is random (16 hex digits) and never handed out twice: a client's selection in
//! `clients.json` may still name a deleted list, and a new list must not inherit that right.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use torro_pim::model::GitHubFilter;
use torro_pim::{Collection, CollectionKind};

use crate::Error;
use crate::address;

/// The file name in the core's config directory.
pub const FILE: &str = "filter-lists.json";

/// How many searches one list may run; each may cost up to ten calls per refresh.
pub const MAX_QUERIES: usize = 10;

/// The longest name a list may have, in characters.
const MAX_NAME: usize = 200;

/// One filter list as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterList {
    /// Random, 16 hex digits ([`new_id`]); the collection id is `<account-id>/<id>`.
    pub id: String,
    pub name: String,
    /// `#RRGGBB`, or none for the app's default.
    #[serde(default)]
    pub color: Option<String>,
    /// The searches as GitHub takes them, already made from addresses ([`address::to_query`]).
    pub queries: Vec<String>,
    /// Link ids of the pinned items (`github:<owner>/<repo>#<n>`).
    #[serde(default)]
    pub pinned: Vec<String>,
    /// Counts the changes of name, colour and searches, from 1.
    pub revision: u64,
}

impl FilterList {
    /// The list as `tasks.list_lists` shows it: id `<account>/<list id>`, task kind, never
    /// writable, the definition in `source` with `truncated` and `problem` from the last refresh.
    pub fn collection(&self, account: &str, truncated: bool, problem: Option<String>) -> Collection {
        Collection {
            id: torro_pim::account::collection_id(account, &self.id),
            name: self.name.clone(),
            kind: CollectionKind::Todos,
            color: self.color.clone(),
            writable: false,
            source: Some(GitHubFilter {
                queries: self.queries.clone(),
                pinned: self.pinned.clone(),
                revision: self.revision,
                truncated,
                problem,
            }),
        }
    }
}

/// What `filter-lists.json` holds: the lists of each account, by account id (`github:<login>`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterFile {
    #[serde(default)]
    pub accounts: BTreeMap<String, Vec<FilterList>>,
}

impl FilterFile {
    /// The lists of `account`; none when it has none.
    pub fn lists(&self, account: &str) -> &[FilterList] {
        self.accounts.get(account).map_or(&[], Vec::as_slice)
    }

    /// Whether any account has a list with id `id`.
    pub fn has_id(&self, id: &str) -> bool {
        self.accounts.values().flatten().any(|list| list.id == id)
    }
}

/// Reads `path`. A missing file means no lists; a damaged one is an error, so nothing writes
/// over the lists a person may still repair.
pub fn load(path: &Path) -> Result<FilterFile, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|err| format!("{} is damaged: {err}", path.display())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(FilterFile::default()),
        Err(err) => Err(format!("cannot read {}: {err}", path.display())),
    }
}

/// Writes `file` to `path` through a temporary file and a rename, so a crash never leaves half a
/// file; readable by the user only (0600 on Unix): the searches name private repositories.
pub fn save(path: &Path, file: &FilterFile) -> Result<(), String> {
    let fail = |err: io::Error| format!("cannot write {}: {err}", path.display());
    let dir = path.parent().ok_or_else(|| format!("{} has no directory", path.display()))?;
    fs::create_dir_all(dir).map_err(fail)?;
    let json = serde_json::to_string_pretty(file).map_err(|err| err.to_string())?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = write_private(&tmp, &json).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map_err(fail)
}

fn write_private(path: &Path, content: &str) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file: File = options.open(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

/// A new list id: 16 random hex digits that `taken` does not know.
pub fn new_id(taken: impl Fn(&str) -> bool) -> Result<String, String> {
    loop {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|err| format!("cannot make a list id: {err}"))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !taken(&id) {
            return Ok(id);
        }
    }
}

/// A list's name: trimmed, not empty, at most 200 characters.
pub fn check_name(name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Input("A filter list needs a name.".into()));
    }
    if name.chars().count() > MAX_NAME {
        return Err(Error::Input("The name is too long; use at most 200 characters.".into()));
    }
    Ok(name.to_string())
}

/// A colour as `#RRGGBB` (any case); none stays none.
pub fn check_color(color: Option<&str>) -> Result<Option<String>, Error> {
    match color.map(str::trim).filter(|c| !c.is_empty()) {
        None => Ok(None),
        Some(c) if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|d| d.is_ascii_hexdigit()) => {
            Ok(Some(c.to_string()))
        }
        Some(_) => Err(Error::Input("A colour is written as #RRGGBB.".into())),
    }
}

/// The searches of a list: each input made a search ([`address::to_query`]), the same search
/// twice kept once, one to [`MAX_QUERIES`] of them.
pub fn check_queries(inputs: &[String]) -> Result<Vec<String>, Error> {
    let mut queries: Vec<String> = Vec::new();
    for input in inputs {
        let query = address::to_query(input)?;
        if !queries.contains(&query) {
            queries.push(query);
        }
    }
    match queries.len() {
        0 => Err(Error::Input("A filter list needs at least one search.".into())),
        n if n > MAX_QUERIES => Err(Error::Input(format!("A filter list may run at most {MAX_QUERIES} searches."))),
        _ => Ok(queries),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(id: &str) -> FilterList {
        FilterList {
            id: id.into(),
            name: "Bugs".into(),
            color: Some("#D73A4A".into()),
            queries: vec!["is:open label:bug".into()],
            pinned: vec!["github:mahype/TorroWorkplace#7".into()],
            revision: 1,
        }
    }

    #[test]
    fn lists_are_stored_per_account_privately_and_read_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config").join(FILE);
        assert_eq!(load(&path), Ok(FilterFile::default()), "no file, no lists");
        let mut file = FilterFile::default();
        file.accounts.insert("github:mahype".into(), vec![list("00ff")]);
        save(&path, &file).expect("saved");
        assert_eq!(load(&path), Ok(file.clone()));
        assert_eq!(file.lists("github:mahype").len(), 1);
        assert!(file.lists("github:other").is_empty());
        assert!(file.has_id("00ff") && !file.has_id("00fe"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let json = fs::read_to_string(&path).expect("text");
        assert!(json.contains("\"accounts\"") && json.contains("\"revision\": 1"), "{json}");

        fs::write(&path, "{ damaged").expect("damage");
        assert!(load(&path).is_err(), "a damaged file is not taken for an empty one");
    }

    #[test]
    fn ids_are_random_hex_and_never_one_that_is_taken() {
        let id = new_id(|_| false).expect("an id");
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        let asked = std::cell::Cell::new(0);
        let other = new_id(|candidate| {
            asked.set(asked.get() + 1);
            asked.get() == 1 || candidate == id
        });
        assert!(asked.get() >= 2, "a taken id is not handed out");
        assert!(other.is_ok_and(|other| other != id));
    }

    #[test]
    fn a_list_is_a_task_list_nobody_writes_to_with_its_definition() {
        let collection = list("00ff").collection("github:mahype", true, Some("offline".into()));
        assert_eq!(collection.id, "github:mahype/00ff");
        assert_eq!((collection.kind, collection.writable), (CollectionKind::Todos, false));
        let source = collection.source.expect("a source");
        assert_eq!((source.revision, source.truncated, source.problem.as_deref()), (1, true, Some("offline")));
        assert_eq!(source.pinned, ["github:mahype/TorroWorkplace#7"]);
    }

    #[test]
    fn names_colours_and_searches_are_checked() {
        assert_eq!(check_name("  Bugs "), Ok("Bugs".into()));
        assert!(check_name(" ").is_err() && check_name(&"x".repeat(201)).is_err());
        assert_eq!(check_color(Some("#a1B2c3")), Ok(Some("#a1B2c3".into())));
        assert_eq!(check_color(None), Ok(None));
        assert_eq!(check_color(Some("")), Ok(None));
        assert!(check_color(Some("red")).is_err() && check_color(Some("#12345")).is_err());

        let inputs = vec![
            "https://github.com/mahype/TorroWorkplace/issues?q=is%3Aopen".to_string(),
            "repo:mahype/TorroWorkplace  is:issue is:open".to_string(),
            "review-requested:@me".to_string(),
        ];
        assert_eq!(
            check_queries(&inputs),
            Ok(vec!["repo:mahype/TorroWorkplace is:issue is:open".into(), "review-requested:@me".into()])
        );
        assert!(check_queries(&[]).is_err());
        assert!(check_queries(&["https://github.com/mahype".to_string()]).is_err());
        let many: Vec<String> = (0..=MAX_QUERIES).map(|n| format!("label:l{n}")).collect();
        assert!(check_queries(&many).is_err());
    }
}
