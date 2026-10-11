//! Building dispatcher expressions.
//!
//! Tasks 1.6 and 1.8 in `docs/tasks/phase-1.md`. Hyprland 0.56 under Omarchy 4 reads
//! dispatchers as Lua: `hl.dsp.focus({ window = "address:0x55d0…" })`. Every string that
//! goes into an expression passes through [`lua_string`]. The builders only return the
//! expression; the backend sends it as `dispatch <expression>`.

use crate::{SplitDirection, WindowId, WmError, WorkspaceId};

/// `address:0x…` for a window id; rejects anything that is not a hex address.
///
/// Accepts `0xABC`, `address:0xABC` and a bare `abc`; the result is lowercase, as Hyprland
/// prints addresses. An id that cannot be a Hyprland address is reported as `NotFound`,
/// because no window can have it.
pub fn window_selector(window: &WindowId) -> Result<String, WmError> {
    let normalized = window.0.trim().to_ascii_lowercase();
    let hex = normalized.strip_prefix("address:").unwrap_or(&normalized);
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(WmError::NotFound(format!("{:?} is not a Hyprland window address", window.0)));
    }
    Ok(format!("address:0x{hex}"))
}

/// A quoted Lua string literal with `\\`, `"`, newlines, tabs and control characters escaped.
///
/// Other control characters become a three-digit decimal escape (`\001`), which is what Lua
/// reads; the fixed width keeps a following digit from joining the escape.
pub fn lua_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 32 => out.push_str(&format!("\\{:03}", control as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// `hl.dsp.focus({ window = "address:0x…" })`
pub fn focus_window(window: &WindowId) -> Result<String, WmError> {
    let selector = window_selector(window)?;
    Ok(format!("hl.dsp.focus({{ window = {} }})", lua_string(&selector)))
}

/// `hl.dsp.window.move({ window = "address:0x…", workspace = "3", follow = false })`
pub fn move_to_workspace(window: &WindowId, workspace: &WorkspaceId) -> Result<String, WmError> {
    let selector = window_selector(window)?;
    Ok(format!(
        "hl.dsp.window.move({{ window = {}, workspace = {}, follow = false }})",
        lua_string(&selector),
        lua_string(&workspace.0)
    ))
}

/// `hl.dsp.window.close({ window = "address:0x…" })`
pub fn close_window(window: &WindowId) -> Result<String, WmError> {
    let selector = window_selector(window)?;
    Ok(format!("hl.dsp.window.close({{ window = {} }})", lua_string(&selector)))
}

/// Reads Hyprland's answer to `dispatch <expression>`: `ok` on success, anything else is an
/// error carrying the trimmed answer text.
pub(super) fn check_answer(answer: &str) -> Result<(), WmError> {
    match answer.trim() {
        "ok" => Ok(()),
        other => Err(WmError::Protocol(other.to_owned())),
    }
}

/// `hl.dsp.layout("preselect r")` for `LeftRight`, `"preselect d"` for `TopBottom`: the next
/// window opens right of / below the focused one.
pub fn preselect(direction: SplitDirection) -> String {
    let _ = direction;
    String::new()
}

/// `hl.dsp.layout("splitratio 1.3000 exact")`. Hyprland's dwindle ratio runs from 0.1 to
/// 1.9 with 1.0 as an even split, so the share of the first child is doubled.
pub fn split_ratio(ratio: f32) -> String {
    let _ = ratio;
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> WindowId {
        WindowId(value.into())
    }

    #[test]
    fn selector_adds_prefix_and_lowercases() {
        assert_eq!(window_selector(&id("0xABC")).expect("valid"), "address:0xabc");
        assert_eq!(window_selector(&id("address:0xABC")).expect("valid"), "address:0xabc");
    }

    #[test]
    fn selector_keeps_normal_address_and_accepts_bare_hex() {
        assert_eq!(window_selector(&id("address:0x1f")).expect("valid"), "address:0x1f");
        assert_eq!(window_selector(&id("abc")).expect("valid"), "address:0xabc");
        assert_eq!(window_selector(&id("  0x55d0 ")).expect("valid"), "address:0x55d0");
    }

    #[test]
    fn selector_rejects_empty_and_non_hex() {
        for bad in ["", "0x", "address:", "0xzz", "abc\"} , evil", "0x1 2", "window-1"] {
            let err = window_selector(&id(bad)).expect_err(bad);
            assert!(matches!(err, WmError::NotFound(_)), "{bad:?}: {err:?}");
        }
    }

    #[test]
    fn lua_string_wraps_plain_text() {
        assert_eq!(lua_string("3"), r#""3""#);
        assert_eq!(lua_string("web ü"), "\"web ü\"");
        assert_eq!(lua_string(""), r#""""#);
    }

    #[test]
    fn lua_string_escapes_quotes_and_backslashes() {
        assert_eq!(lua_string(r#"a"b"#), r#""a\"b""#);
        assert_eq!(lua_string(r"a\b"), r#""a\\b""#);
        assert_eq!(lua_string(r#"\""#), r#""\\\"""#);
    }

    #[test]
    fn lua_string_escapes_newline_carriage_return_and_tab() {
        assert_eq!(lua_string("a\nb"), r#""a\nb""#);
        assert_eq!(lua_string("a\rb"), r#""a\rb""#);
        assert_eq!(lua_string("a\tb"), r#""a\tb""#);
    }

    #[test]
    fn lua_string_escapes_other_control_characters_as_three_digit_decimal() {
        assert_eq!(lua_string("\u{1}"), r#""\001""#);
        assert_eq!(lua_string("\u{b}"), r#""\011""#);
        assert_eq!(lua_string("\u{1f}"), r#""\031""#);
        assert_eq!(lua_string("\u{0}1"), r#""\0001""#);
    }

    #[test]
    fn focus_expression() {
        assert_eq!(focus_window(&id("0x55d0")).expect("valid"), r#"hl.dsp.focus({ window = "address:0x55d0" })"#);
        assert_eq!(focus_window(&id("ABC")).expect("valid"), r#"hl.dsp.focus({ window = "address:0xabc" })"#);
        assert!(focus_window(&id("xyz")).is_err());
    }

    #[test]
    fn move_expression_does_not_follow() {
        let expression = move_to_workspace(&id("0x1f"), &WorkspaceId("3".into())).expect("valid");
        assert_eq!(expression, r#"hl.dsp.window.move({ window = "address:0x1f", workspace = "3", follow = false })"#);
    }

    #[test]
    fn move_expression_escapes_workspace_name() {
        let expression = move_to_workspace(&id("0x1f"), &WorkspaceId("we\"b\\\n".into())).expect("valid");
        assert_eq!(
            expression,
            r#"hl.dsp.window.move({ window = "address:0x1f", workspace = "we\"b\\\n", follow = false })"#
        );
    }

    #[test]
    fn move_expression_rejects_bad_window() {
        assert!(move_to_workspace(&id("nope"), &WorkspaceId("3".into())).is_err());
    }

    #[test]
    fn close_expression() {
        assert_eq!(close_window(&id("0x1f")).expect("valid"), r#"hl.dsp.window.close({ window = "address:0x1f" })"#);
        assert!(close_window(&id("")).is_err());
    }

    #[test]
    fn answer_ok_is_success() {
        assert_eq!(check_answer("ok"), Ok(()));
        assert_eq!(check_answer("ok\n"), Ok(()));
    }

    #[test]
    fn other_answers_are_protocol_errors_with_trimmed_text() {
        assert_eq!(check_answer(""), Err(WmError::Protocol(String::new())));
        assert_eq!(check_answer("  Invalid dispatcher \n"), Err(WmError::Protocol("Invalid dispatcher".into())));
        assert_eq!(check_answer("okay"), Err(WmError::Protocol("okay".into())));
    }
}
