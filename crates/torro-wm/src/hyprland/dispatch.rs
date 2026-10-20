//! Building dispatcher expressions.
//!
//! Tasks 1.6 and 1.8 in `docs/tasks/phase-1.md`. Hyprland 0.56 under Omarchy 4 reads
//! dispatchers as Lua: `hl.dsp.focus({ window = "address:0x55d0…" })`. Every string that
//! goes into an expression passes through [`lua_string`]. The builders only return the
//! expression; the backend sends it as `dispatch <expression>` (dispatchers) or `eval <expression>`
//! (binds and window rules, task WS.1).
//!
//! The Lua forms follow the Hyprland wiki for 0.56 (`Configuring/Basics/Dispatchers`, `…/Binds`,
//! `…/Window-Rules`): `hl.dsp.focus({ monitor = … })`, `hl.bind(keys, hl.dsp.exec_cmd(cmd))`,
//! `hl.unbind(keys)` and `hl.window_rule({ name = …, match = { class = … }, … })`.

use crate::{KeyBind, SplitDirection, WindowId, WmError, WorkspaceId};

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

/// `hl.dsp.focus({ monitor = "DP-1" })`: the monitor of that name gets the focus, no window moves.
pub fn focus_monitor(monitor: &str) -> String {
    format!("hl.dsp.focus({{ monitor = {} }})", lua_string(monitor))
}

/// `hl.bind("SUPER + ALT + 1", hl.dsp.exec_cmd("'/path/torroworkplace' 'load' …"))` for `bind`.
///
/// `exec_cmd` hands its text to `sh -c`, so the command becomes one shell line ([`shell_line`]) and that
/// line one Lua string: neither the shell nor Lua can read an argument as code. Errors as [`shell_line`].
pub fn bind_key(bind: &KeyBind) -> Result<String, WmError> {
    let line = shell_line(&bind.command)?;
    Ok(format!("hl.bind({}, hl.dsp.exec_cmd({}))", lua_string(&bind.keys), lua_string(&line)))
}

/// `hl.unbind("SUPER + ALT + 1")`. `keys` must be spelled exactly as it was bound: Hyprland compares
/// case-sensitively (`SUPER + Tab` does not remove `SUPER + TAB`).
pub fn unbind_key(keys: &str) -> String {
    format!("hl.unbind({})", lua_string(keys))
}

/// `hl.window_rule({ … })` that floats and centres every window of the class `class` (task WS.1, the
/// dialog after "Save workspace…"). Named after the class, so a second registration replaces the
/// first; the class is matched as a whole regex with its characters escaped.
pub fn float_rule(class: &str) -> String {
    format!(
        "hl.window_rule({{ name = {}, match = {{ class = {} }}, float = true, center = true }})",
        lua_string(&format!("torro-float-{class}")),
        lua_string(&format!("^{}$", regex_escape(class))),
    )
}

/// `command` as one line for `sh -c`: every element in single quotes, a `'` inside as `'\''`, joined by
/// spaces. Single quotes keep everything literal (spaces, `$`, `"`, backslashes, newlines).
///
/// Errors: `Protocol` for an empty command and for a NUL character, which no shell line can carry.
pub fn shell_line(command: &[String]) -> Result<String, WmError> {
    if command.is_empty() {
        return Err(WmError::Protocol("a key bind needs a command".into()));
    }
    if command.iter().any(|argument| argument.contains('\0')) {
        return Err(WmError::Protocol("a command argument holds a NUL character".into()));
    }
    let quoted: Vec<String> = command.iter().map(|argument| format!("'{}'", argument.replace('\'', r"'\''"))).collect();
    Ok(quoted.join(" "))
}

/// Reads Hyprland's answer to `dispatch <expression>`: `ok` on success, anything else is an
/// error carrying the trimmed answer text.
pub(super) fn check_answer(answer: &str) -> Result<(), WmError> {
    match answer.trim() {
        "ok" => Ok(()),
        other => Err(WmError::Protocol(other.to_owned())),
    }
}

/// `hl.window_rule({ … })` for `cover` (task ER2.2, Hyprland 0.56): a named rule (named after the
/// monitor, so a second registration replaces the first) that puts the window of that class and
/// exact title full screen on that monitor's visible workspace, without switching to it; windows
/// on other monitors do not take the focus. Not `pin`: pinning only works for floating windows,
/// and a floating one would stay under the bar. Class and title are matched as whole regexes,
/// with their characters escaped.
pub fn cover_rule(cover: &crate::Cover) -> String {
    let exact = |text: &str| format!("^{}$", regex_escape(text));
    format!(
        "hl.window_rule({{ name = {}, match = {{ class = {}, title = {} }}, monitor = {}, fullscreen = true, \
         no_initial_focus = {} }})",
        lua_string(&format!("{}-{}", cover.class, cover.monitor)),
        lua_string(&exact(&cover.class)),
        lua_string(&exact(&cover.title)),
        lua_string(&cover.monitor),
        !cover.focus,
    )
}

/// `text` with every regex metacharacter escaped, so a monitor name like `DP-1.2` matches itself.
/// Also used for AeroSpace's monitor patterns ([`crate::aerospace::cli::monitor_pattern`]).
pub(crate) fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if "\\.+*?()|[]{}^$".contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// `hl.dsp.layout("preselect r")` for `LeftRight`, `"preselect d"` for `TopBottom`: the next
/// window opens right of / below the focused one.
pub fn preselect(direction: SplitDirection) -> String {
    let flag = match direction {
        SplitDirection::LeftRight => "r",
        SplitDirection::TopBottom => "d",
    };
    format!("hl.dsp.layout({})", lua_string(&format!("preselect {flag}")))
}

/// `hl.dsp.layout("splitratio 1.3000 exact")`. Hyprland's dwindle ratio runs from 0.1 to
/// 1.9 with 1.0 as an even split, so the share of the first child is doubled. The doubled
/// value is clamped to that range, which means `ratio` is effectively limited to 0.05..=0.95.
pub fn split_ratio(ratio: f32) -> String {
    let hyprland_ratio = (ratio * 2.0).clamp(0.1, 1.9);
    format!("hl.dsp.layout({})", lua_string(&format!("splitratio {hyprland_ratio:.4} exact")))
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
    fn preselect_expressions_per_direction() {
        assert_eq!(preselect(SplitDirection::LeftRight), r#"hl.dsp.layout("preselect r")"#);
        assert_eq!(preselect(SplitDirection::TopBottom), r#"hl.dsp.layout("preselect d")"#);
    }

    #[test]
    fn split_ratio_doubles_and_formats_with_four_decimals() {
        assert_eq!(split_ratio(0.5), r#"hl.dsp.layout("splitratio 1.0000 exact")"#);
        assert_eq!(split_ratio(0.35), r#"hl.dsp.layout("splitratio 0.7000 exact")"#);
    }

    #[test]
    fn split_ratio_clamps_to_hyprland_range() {
        assert_eq!(split_ratio(0.01), r#"hl.dsp.layout("splitratio 0.1000 exact")"#);
        assert_eq!(split_ratio(0.99), r#"hl.dsp.layout("splitratio 1.9000 exact")"#);
        assert_eq!(split_ratio(0.0), r#"hl.dsp.layout("splitratio 0.1000 exact")"#);
        assert_eq!(split_ratio(1.0), r#"hl.dsp.layout("splitratio 1.9000 exact")"#);
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

    #[test]
    fn a_cover_rule_names_the_monitor_and_matches_the_exact_title() {
        let cover = crate::Cover {
            class: "torro-reminder".into(),
            title: "Reminder · DP-1.2".into(),
            monitor: "DP-1.2".into(),
            focus: false,
        };
        assert_eq!(
            cover_rule(&cover),
            r#"hl.window_rule({ name = "torro-reminder-DP-1.2", match = { class = "^torro-reminder$", title = "^Reminder · DP-1\\.2$" }, monitor = "DP-1.2", fullscreen = true, no_initial_focus = true })"#
        );
        let main = crate::Cover { focus: true, ..cover };
        assert!(cover_rule(&main).ends_with("no_initial_focus = false })"));
        let sneaky = crate::Cover { title: "x\" }) os.exit()".into(), ..main };
        assert!(!cover_rule(&sneaky).contains("\" })"), "a title cannot end the Lua string");
    }

    fn command(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn focus_monitor_names_the_monitor() {
        assert_eq!(focus_monitor("DP-1"), r#"hl.dsp.focus({ monitor = "DP-1" })"#);
        assert_eq!(focus_monitor("a\"b"), r#"hl.dsp.focus({ monitor = "a\"b" })"#);
    }

    #[test]
    fn shell_line_quotes_every_argument() {
        let line = shell_line(&command(&["/opt/my apps/torroworkplace", "load", "it's $HOME"])).expect("valid");
        assert_eq!(line, r#"'/opt/my apps/torroworkplace' 'load' 'it'\''s $HOME'"#);
        assert_eq!(shell_line(&command(&[""])).expect("valid"), "''");
        assert!(matches!(shell_line(&[]), Err(WmError::Protocol(_))));
        assert!(matches!(shell_line(&command(&["a\0b"])), Err(WmError::Protocol(_))));
    }

    #[test]
    fn a_bind_runs_the_quoted_command() {
        let bind = KeyBind {
            keys: "SUPER + ALT + 1".into(),
            command: command(&["/home/anna/my apps/torroworkplace", "load", "template:a'b\"c]]d"]),
        };
        // Shell line: '/home/anna/my apps/torroworkplace' 'load' 'template:a'\''b"c]]d'
        // In Lua, the backslash and the double quote of that line are escaped once more.
        assert_eq!(
            bind_key(&bind).expect("valid"),
            r#"hl.bind("SUPER + ALT + 1", hl.dsp.exec_cmd("'/home/anna/my apps/torroworkplace' 'load' 'template:a'\\''b\"c]]d'"))"#
        );
    }

    #[test]
    fn a_bind_cannot_end_its_lua_strings() {
        let bind = KeyBind { keys: "SUPER + 1\"), os.exit(".into(), command: command(&["x\")) os.exit() --"]) };
        let expression = bind_key(&bind).expect("valid");
        assert!(expression.starts_with(r#"hl.bind("SUPER + 1\"), os.exit(", "#), "{expression}");
        assert!(expression.ends_with(r#"hl.dsp.exec_cmd("'x\")) os.exit() --'"))"#), "{expression}");
        assert!(bind_key(&KeyBind { keys: "SUPER + 1".into(), command: Vec::new() }).is_err());
    }

    #[test]
    fn unbind_keeps_the_spelling() {
        assert_eq!(unbind_key("SUPER + ALT + 1"), r#"hl.unbind("SUPER + ALT + 1")"#);
        assert_eq!(unbind_key("SUPER + Tab"), r#"hl.unbind("SUPER + Tab")"#);
    }

    #[test]
    fn a_float_rule_matches_the_exact_class() {
        assert_eq!(
            float_rule("torro.dialog"),
            r#"hl.window_rule({ name = "torro-float-torro.dialog", match = { class = "^torro\\.dialog$" }, float = true, center = true })"#
        );
        // The quote stays inside both strings: escaped for Lua in the name, and for Lua in the regex.
        let sneaky = float_rule("x\" }) os.exit(");
        assert!(sneaky.starts_with(r#"hl.window_rule({ name = "torro-float-x\" }) os.exit(", match"#), "{sneaky}");
        assert!(sneaky.contains(r#"class = "^x\" \\}\\) os\\.exit\\($" }"#), "{sneaky}");
    }
}
