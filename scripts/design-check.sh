#!/usr/bin/env bash
#
# design-check.sh – finds raw design values in the app crates (F.6 in docs/tasks/design.md).
#
# Why: all apps must look like one suite. Sizes, spacing, radii, colours, fonts and button styles
# come from the tokens in torro_gui::{theme, typo, layout, style} – functions of the active design
# pack (space::s(), typo::body(), layout::page_padding() …, docs/design-pakete.md); a hand-typed
# number in a layout call
# or an Iced standard style breaks that quietly, and nobody sees it in review. This is a text
# scan, not a compiler check. It flags:
#   - a non-zero number literal in .size()/.spacing()/.padding()/.width()/.height()/.rounded()
#     (Length::… arguments, token calls and named constants are fine; a bare 0 means "no space")
#   - Color::from_rgb…
#   - Iced standard button styles (button::primary …), i.e. not style::button::…
#   - Font::DEFAULT
# Exceptions go on the line or the line before: `// design-check: allow <reason>`.
# Code after `#[cfg(test)]` (test modules) is not checked.
#
# Usage:
#   scripts/design-check.sh               same as --strict
#   scripts/design-check.sh --warn        list findings as file:line: rule, exit 0
#   scripts/design-check.sh --strict      list findings, exit 1 if there are any
#   scripts/design-check.sh --self-test   check the rules against an embedded fixture
#
# Only sh-level tools (bash, awk, find, sort, cut) are used, so it runs in CI as-is.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

TARGETS=(crates/torroassistant/src crates/torrocalendar/src crates/torroinbox/src crates/torrotasks/src crates/torro-settings-ui/src crates/torroworkplace/src/ui)

# The scanner. POSIX awk only (no gawk extensions), because CI also runs it on macOS.
awk_program() {
    cat <<'AWK'
# Prints "file:line: rule" for every finding that is not allowed by a marker.
function report(lineno, rule, allowed) {
    if (!allowed) print FILENAME ":" lineno ": " rule
}

# Walks s with the global depth (1 = inside the call's parenthesis). Returns the index of the
# closing ')' if the call ends in s, otherwise 0 and depth carries over to the next line.
function walk(s,   i, c) {
    for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c == "(") depth++
        else if (c == ")") {
            depth--
            if (depth == 0) return i
        }
    }
    return 0
}

# True if the argument contains a number other than zero.
function has_number(a,   t, tok) {
    t = a
    while (match(t, /(^|[^A-Za-z0-9_.])[0-9]+(\.[0-9]+)?/)) {
        tok = substr(t, RSTART, RLENGTH)
        sub(/^[^0-9]/, "", tok)
        if (tok + 0 != 0) return 1
        t = substr(t, RSTART + RLENGTH)
    }
    return 0
}

function evaluate(name, arg, lineno, allowed,   a) {
    a = arg
    sub(/^[ \t]+/, "", a)
    if (a ~ /^Length::/) return
    if (has_number(a)) report(lineno, "raw number in ." name "(): use a token", allowed)
}

# Brace balance of s, used to find the end of a #[cfg(test)] item.
function braces(s,   o, c) {
    o = gsub(/\{/, "{", s)
    c = gsub(/\}/, "}", s)
    return o - c
}

function check_line(s, lineno, allowed,   t, pre, end, name, rest) {
    if (s ~ /Color::from_rgb/) report(lineno, "Color::from_rgb: use a colour from torro_gui::theme", allowed)
    if (s ~ /Font::DEFAULT/) report(lineno, "Font::DEFAULT: use typo::… (the design font)", allowed)

    t = s
    while (match(t, /button::(primary|secondary|text|success|danger)/)) {
        pre = substr(t, 1, RSTART - 1)
        if (pre !~ /style::$/) {
            report(lineno, "Iced standard style " substr(t, RSTART, RLENGTH) ": use torro_gui::style", allowed)
        }
        t = substr(t, RSTART + RLENGTH)
    }

    # A call that started on an earlier line keeps collecting its argument here.
    if (collecting) {
        end = walk(s)
        if (!end) {
            arg_buf = arg_buf " " s
            return
        }
        evaluate(cname, arg_buf " " substr(s, 1, end - 1), cline, callow)
        collecting = 0
        s = substr(s, end + 1)
    }

    while (match(s, /\.(size|spacing|padding|width|height|rounded)\(/)) {
        name = substr(s, RSTART + 1, RLENGTH - 2)
        rest = substr(s, RSTART + RLENGTH)
        depth = 1
        end = walk(rest)
        if (end) {
            evaluate(name, substr(rest, 1, end - 1), lineno, allowed)
            s = substr(rest, end + 1)
        } else {
            collecting = 1
            cname = name
            cline = lineno
            callow = allowed
            arg_buf = rest
            return
        }
    }
}

FNR == 1 {
    collecting = 0
    depth = 0
    skip = 0
    pending = 0
    prev_allow = 0
}

{
    this_allow = ($0 ~ /design-check: allow[ \t]+[^ \t]/)
    allowed = (prev_allow || this_allow)
    prev_allow = this_allow

    # Strip string literals and comments so that only code is matched.
    s = $0
    gsub(/"([^"\\]|\\.)*"/, "\"\"", s)
    gsub(/'([^'\\]|\\.)'/, "''", s)
    sub(/\/\/.*$/, "", s)

    if (pending) {
        if (s ~ /\{/) {
            pending = 0
            skip = 1
            tdepth = 0
        } else if (s ~ /;/) {
            pending = 0
        }
        if (skip) {
            tdepth += braces(s)
            if (tdepth <= 0) skip = 0
        }
        next
    }
    if (skip) {
        tdepth += braces(s)
        if (tdepth <= 0) skip = 0
        next
    }
    if (s ~ /#\[cfg\(test\)\]/) {
        pending = 1
        next
    }

    check_line(s, FNR, allowed)
}
AWK
}

scan() {
    awk "$(awk_program)" "$@"
}

list_files() {
    local dir
    for dir in "${TARGETS[@]}"; do
        find "$dir" -name '*.rs' -type f
    done | LC_ALL=C sort
}

# Fixture for the self-test. A line ending in `// want` must be flagged, every other line must not.
self_test_fixture() {
    cat <<'FIXTURE'
//! Fixture: lines ending in `// want` must be flagged, all others must pass.
/// Doc example: text("x").size(14) is not code.
fn view() -> Element<'_, Message> {
    let ok_tokens = row![x].spacing(space::xs()).padding(Padding::new(0.0).top(space::s())); // ok
    let ok_zero = container(x).padding(0).width(0); // ok
    let ok_length = Space::new().width(Length::Fixed(40.0)).height(Length::FillPortion(2)); // ok
    let ok_const = Space::new().width(TIME_WIDTH).height(line); // ok
    let ok_size = text("x").size(typo::body().size).size(style::CHECK_SIZE); // ok
    let ok_style = button("x").style(style::button::primary); // ok
    let ok_text = "text(\"x\").size(14) and Font::DEFAULT in a string"; // ok
    let ok_base = Color::BLACK; // ok
    let ok_allow = text("x").size(14); // design-check: allow fixed glyph size from the mock-up
    // design-check: allow the grid needs a 18 px cell from the mock-up
    let ok_allow_above = Space::new().height(18);
    let bad_size = text("x").size(14); // want
    let bad_padding = container(x).padding(12.0); // want
    let bad_array = container(x).padding([8, 12]); // want
    let bad_expr = Space::new().width(space::xs() * 2.0); // want
    let bad_rounded = container(x).rounded(6); // want
    let bad_no_allow = Space::new().height(18); // want
    let bad_color = Color::from_rgb(1.0, 0.0, 0.0); // want
    let bad_font = Font::DEFAULT; // want
    let bad_button = button("x").style(button::primary); // want
    let bad_button_text = button("x").style(button::text); // want
    let bad_multi = container(x)
        .padding( // want
            12.0,
        );
    row![a, b].spacing(space::s()).padding(Padding::new(0.0).left(space::l())); // ok
    row![a, b].spacing(9).padding(Padding::new(0.0).left(space::l())); // want
    column![].spacing(12.0); // want
    column![].spacing(space::s());
    let ok_cfg = 1;
}

#[cfg(test)]
mod tests {
    fn helper() {
        let c = Color::from_rgb(1.0, 0.0, 0.0); // ok, test code
        text("x").size(14); // ok, test code
    }
}

#[cfg(test)]
use std::collections::HashMap;

fn after_tests() {
    text("x").size(14); // want
}
FIXTURE
}

self_test() {
    local fixture actual expected
    fixture=$(mktemp "${TMPDIR:-/tmp}/design-check.XXXXXX")
    self_test_fixture > "$fixture"

    expected=$(awk '/\/\/ want$/ { print NR }' "$fixture" | sort -n)
    actual=$(scan "$fixture" | cut -d: -f2 | sort -n)
    rm -f "$fixture"

    if [[ -z "$expected" ]]; then
        echo "design-check self-test: fixture has no expected findings" >&2
        return 1
    fi
    if [[ "$actual" != "$expected" ]]; then
        echo "design-check self-test: FAILED (lines flagged vs. lines marked '// want')" >&2
        diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
        return 1
    fi
    echo "design-check self-test: ok ($(printf '%s\n' "$expected" | wc -l | tr -d ' ') cases)"
}

usage() {
    cat <<'USAGE'
usage: scripts/design-check.sh [--warn | --strict | --self-test]
  --warn       list findings, exit 0
  --strict     list findings, exit 1 if there are any (default)
  --self-test  check the rules against the embedded fixture
USAGE
}

mode=strict
case "${1:---strict}" in
    --warn) mode=warn ;;
    --strict) mode=strict ;;
    --self-test)
        if self_test; then exit 0; else exit 1; fi
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

files=()
while IFS= read -r file; do
    files+=("$file")
done < <(list_files)

findings=$(scan "${files[@]}")
count=0
if [[ -n "$findings" ]]; then
    printf '%s\n' "$findings"
    count=$(printf '%s\n' "$findings" | wc -l | tr -d ' ')
fi

echo "design-check: $count finding(s) in ${#files[@]} files ($mode)" >&2

if [[ "$mode" == strict && "$count" -gt 0 ]]; then
    exit 1
fi
