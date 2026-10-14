//! Nextcloud's rich object strings as plain text.
//!
//! Notifications (`subjectRich`, `messageRich`) and Talk messages (`message`) carry placeholders
//! like `{user} shared {file} with you`, with a map of parameters
//! `{"user": {"type": "user", "id": "anna", "name": "Anna"}, …}`. Shown, a placeholder is the
//! parameter's `name`.
//! <https://github.com/nextcloud/server/blob/master/lib/public/RichObjectStrings/Definitions.php>
//!
//! All of it is foreign text. [`plain`] also drops control characters and the characters that
//! change the writing direction (U+202E and friends), so a name cannot hide or reorder what is
//! shown around it; [`line`] makes one line of it, for titles.

use serde_json::Value;

/// `text` with every `{key}` that names a parameter replaced by the parameter's `name`, without
/// control and direction characters; line breaks stay, tabs become spaces, and the result is
/// trimmed. A placeholder without a parameter (or without a `name`) stays as it is.
///
/// `parameters` is the map as Nextcloud sends it; anything else (PHP sends an empty map as `[]`)
/// counts as no parameters. Names are inserted once and never read for placeholders again.
pub fn plain(text: &str, parameters: &Value) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name = after.find('}').and_then(|close| {
            let key = &after[..close];
            let name = parameters.get(key).and_then(|parameter| parameter.get("name")).and_then(Value::as_str);
            name.map(|name| (name, close))
        });
        match name {
            Some((name, close)) => {
                out.push_str(name);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    clean(&out)
}

/// [`plain`] as one line: every run of white space (line breaks included) becomes one space.
pub fn line(text: &str, parameters: &Value) -> String {
    plain(text, parameters).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` without control and direction characters, line breaks kept, tabs as spaces, trimmed.
fn clean(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter_map(|c| match c {
            '\n' => Some('\n'),
            '\t' => Some(' '),
            c if c.is_control() || is_direction(c) => None,
            c => Some(c),
        })
        .collect();
    kept.trim().to_string()
}

/// The characters that mark or override the writing direction: the Arabic letter mark, the
/// left-to-right and right-to-left marks, the embeddings and overrides, and the isolates.
fn is_direction(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn placeholders_become_the_names_of_their_parameters() {
        let parameters = json!({
            "user": {"type": "user", "id": "anna", "name": "Anna Example"},
            "file": {"type": "file", "id": "42", "name": "Plan.pdf", "path": "Plan.pdf"},
        });
        assert_eq!(plain("{user} shared {file} with you", &parameters), "Anna Example shared Plan.pdf with you");
    }

    #[test]
    fn unknown_placeholders_and_braces_stay() {
        let parameters = json!({"user": {"type": "user", "id": "anna"}});
        assert_eq!(plain("{user} said {nothing} {", &parameters), "{user} said {nothing} {");
        assert_eq!(plain("a {b", &json!([])), "a {b");
        assert_eq!(plain("{{user}}", &json!({"user": {"name": "Anna"}})), "{Anna}");
    }

    #[test]
    fn names_are_not_read_for_placeholders_again() {
        let parameters = json!({"a": {"name": "{b}"}, "b": {"name": "B"}});
        assert_eq!(plain("{a}", &parameters), "{b}");
    }

    #[test]
    fn an_empty_php_map_is_no_parameters() {
        assert_eq!(plain("You have a {thing}", &json!([])), "You have a {thing}");
        assert_eq!(plain("Plain", &Value::Null), "Plain");
    }

    #[test]
    fn control_and_direction_characters_are_dropped() {
        let parameters = json!({"user": {"name": "Ann\u{202E}a\u{0007}"}});
        assert_eq!(plain("\u{2066}{user}\u{2069}\tsays\r\nhi\u{200F}", &parameters), "Anna says\nhi");
    }

    #[test]
    fn a_title_is_one_line() {
        assert_eq!(line("  Two\nlines \n\n and  more ", &json!([])), "Two lines and more");
        let parameters = json!({"user": {"name": "Anna\nExample"}});
        assert_eq!(line("{user} called", &parameters), "Anna Example called");
    }
}
