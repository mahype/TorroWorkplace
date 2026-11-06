//! Version numbers of the suite's programs (issue #411).
//!
//! Every app brings a copy of the service along, so several copies of different versions can lie
//! on one computer. They are compared by [`Version`]: the newest copy starts, an older one never
//! works on data a newer one used, and the app catalog tells when a release is newer than what is
//! installed. Only the form the suite's releases carry is read: `x.y.z`, optionally with a
//! pre-release (`0.2.0-beta.1`) and a leading `v` as in a tag.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A version `major.minor.patch` with an optional pre-release, ordered as semantic versioning
/// orders them: a pre-release is older than its release (`0.2.0-beta.1` < `0.2.0`), pre-releases
/// compare part by part, numbers as numbers (`beta.2` < `beta.10`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The parts after `-`, empty for a release.
    pub pre: Vec<String>,
}

/// A text that is no version of the form [`Version`] reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionError(pub String);

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a version: {:?}", self.0)
    }
}

impl std::error::Error for VersionError {}

impl Version {
    /// The version of the program this is compiled into (`CARGO_PKG_VERSION` of `torro-core`, which
    /// shares the workspace version with every program of the suite).
    pub fn current() -> Version {
        // The workspace version always has the form; a fallback keeps this free of `unwrap`.
        Version::from_str(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre: Vec::new(),
        })
    }

    /// Whether it is a pre-release (`0.2.0-rc.1`).
    pub fn is_pre_release(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl FromStr for Version {
    type Err = VersionError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || VersionError(text.to_string());
        let trimmed = text.trim();
        let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
        // Build metadata (`+abc`) does not order versions; it is dropped.
        let trimmed = trimmed.split_once('+').map_or(trimmed, |(version, _)| version);
        let (core, pre) = match trimmed.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (trimmed, None),
        };
        let mut numbers = core.split('.').map(|part| {
            let digits = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
            if digits { part.parse::<u64>().ok() } else { None }
        });
        let major = numbers.next().flatten().ok_or_else(error)?;
        let minor = numbers.next().flatten().ok_or_else(error)?;
        let patch = numbers.next().flatten().ok_or_else(error)?;
        if numbers.next().is_some() {
            return Err(error());
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let parts: Vec<String> = pre.split('.').map(str::to_string).collect();
                let valid = |part: &String| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric());
                if !parts.iter().all(valid) {
                    return Err(error());
                }
                parts
            }
        };
        Ok(Version { major, minor, patch, pre })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release is newer than any of its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pre-releases part by part: numbers as numbers and older than words, words by their letters; with
/// equal parts the one with more parts is newer.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (left, right) in a.iter().zip(b) {
        let order = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        text.parse().expect("a version")
    }

    #[test]
    fn reads_releases_tags_and_pre_releases() {
        assert_eq!(v("0.1.0"), Version { major: 0, minor: 1, patch: 0, pre: Vec::new() });
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("0.2.0-beta.1").pre, ["beta", "1"]);
        assert_eq!(v("1.0.0+build.5"), v("1.0.0"));
        assert_eq!(v("0.2.0-rc.1").to_string(), "0.2.0-rc.1");
        assert!(v("0.2.0-rc.1").is_pre_release());
    }

    #[test]
    fn refuses_other_text() {
        for text in ["", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-", "1.2.3-beta..1", "-1.2.3", "1. 2.3"] {
            assert!(text.parse::<Version>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn orders_as_semantic_versioning() {
        let ordered = [
            "0.1.0",
            "0.1.1",
            "0.2.0-alpha",
            "0.2.0-alpha.1",
            "0.2.0-beta.2",
            "0.2.0-beta.10",
            "0.2.0",
            "0.10.0",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(ordered.iter().map(|t| v(t)).max(), Some(v("1.0.0")));
    }

    #[test]
    fn knows_its_own_version() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
