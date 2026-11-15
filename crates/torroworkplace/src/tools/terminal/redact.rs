//! Hides what looks like a secret in the output of a command of the assistant's terminal (issue #754, decision 5),
//! before anything of it reaches the model, the conversation's history or the approval card: sign-ins in addresses,
//! `Authorization` headers, private keys, the tokens of well-known services and lines that set a key, token, secret
//! or password. A rule of thumb like the patterns of commands: a secret in another form passes.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// What stands in place of a hidden value.
pub const REDACTED: &str = "[redacted]";

/// The rules, each with what it keeps of a hit (`$1` the name before the value).
static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // A private key, whole.
        (r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)", REDACTED),
        // A user and password in an address: https://anna:secret@host.
        (r"(?i)\b([a-z][a-z0-9+.-]*://)[^\s/@:]+:[^\s/@]+@", "${1}[redacted]@"),
        // An Authorization or a key header.
        (
            r"(?i)\b((?:proxy-)?authorization\s*[:=]\s*(?:bearer\s+|basic\s+|token\s+)?)[^\s,;]+",
            "${1}[redacted]",
        ),
        (r"(?i)\b(x-api-key\s*[:=]\s*)[^\s,;]+", "${1}[redacted]"),
        // Tokens of well-known services by their form.
        (r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}", REDACTED),
        (r"\bgithub_pat_[A-Za-z0-9_]{20,}", REDACTED),
        (r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{16,}", REDACTED),
        (r"\bxox[abprs]-[A-Za-z0-9-]{10,}", REDACTED),
        (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", REDACTED),
        (r"\bglpat-[A-Za-z0-9_-]{20,}", REDACTED),
        (r"\bAIza[0-9A-Za-z_-]{35}\b", REDACTED),
        // A variable or option that sets a key, token, secret or password: `API_KEY=…`, `password: …`.
        (
            r#"(?im)\b([A-Za-z0-9_.-]*(?:key|token|secret|passwd|password|pwd|credential)s?["']?\s*[:=]\s*["']?)[^\s"',;]{4,}"#,
            "${1}[redacted]",
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, keep)| match Regex::new(pattern) {
        Ok(regex) => Some((regex, keep)),
        Err(err) => {
            eprintln!("torroworkplace: a rule for hiding secrets does not compile: {err}");
            None
        }
    })
    .collect()
});

/// `text` with every value that looks like a secret replaced by [`REDACTED`], and whether anything was.
pub fn redact(text: &str) -> (String, bool) {
    let mut out = text.to_string();
    let mut hidden = false;
    for (regex, keep) in RULES.iter() {
        // A rule that cannot finish hides everything: in doubt, nothing goes out.
        let Ok(replaced) = regex.try_replacen(&out, 0, *keep) else { return (REDACTED.to_string(), true) };
        let replaced = replaced.into_owned();
        hidden |= replaced != out;
        out = replaced;
    }
    (out, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_compiles() {
        assert_eq!(RULES.len(), 12);
    }

    /// Erledigt point 7 of issue #754: a token in the output reaches the model only hidden.
    #[test]
    fn secrets_are_hidden_and_the_rest_stays() {
        let token = "ghp_0123456789abcdefghijABCDEFGHIJ0123";
        let (out, hidden) = redact(&format!("remote: using {token}\nok"));
        assert!(hidden && !out.contains(token) && out.contains("[redacted]") && out.ends_with("\nok"), "{out}");
        for (text, gone) in [
            ("https://anna:hunter2@example.com/x", "hunter2"),
            ("Authorization: Bearer abc.def.ghi", "abc.def.ghi"),
            ("export API_KEY=supersecretvalue", "supersecretvalue"),
            ("password: \"correct horse\"", "correct"),
            ("OPENAI=sk-proj-abcdefghijklmnop1234", "abcdefghijklmnop1234"),
            ("xoxb-1234567890-abcdefghij", "abcdefghij"),
            ("AKIAABCDEFGHIJKLMNOP", "AKIAABCDEFGHIJKLMNOP"),
            ("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Blbn\n-----END OPENSSH PRIVATE KEY-----", "b3Blbn"),
        ] {
            let (out, hidden) = redact(text);
            assert!(hidden && !out.contains(gone), "{text} -> {out}");
        }
        let plain = "Compiling torro v0.1.0\ntest result: ok. 12 passed; key points: none";
        assert_eq!(redact(plain), (plain.to_string(), false));
    }
}
