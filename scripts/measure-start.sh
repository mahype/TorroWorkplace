#!/usr/bin/env bash
#
# measure-start.sh – how fast an app of the suite starts and shows its data (issue #206,
# docs/tempo.md, task TP.1 in docs/tasks/tempo.md).
#
# Why: "the window is slow" needs numbers before anything changes, and the same numbers after.
# The apps write timestamps on stderr when TORRO_TIMING is set (torro_core::timing); this script
# starts an app several times, waits for its mark "first data", ends it and prints the marks.
#
# Usage:
#   scripts/measure-start.sh [--release] [--cold] [--runs N] <app> [args…]
#   scripts/measure-start.sh [--release] [--runs N] --core
#
#   <app>       torroinbox, torrocalendar, torrotasks, torroassistant or torroworkplace (ui)
#   --release   the programs from target/release instead of target/debug
#   --cold      ends the running core before each run (torroworkplace ctl quit), so the app starts
#               it itself; open apps and MCP servers start it again later
#   --runs N    measured runs, 5 by default; one more run before them is not counted (macOS checks a
#               freshly built program once on its first start, which is not the app's time)
#   --core      measures the core alone: ends it, starts `torroworkplace daemon` and prints its
#               marks up to "socket task started"; the core it started keeps running
#
# Prints one line per run with the marks in milliseconds since the program started ("-" when a
# mark did not come within 15 s), then the median. Builds nothing: run `cargo build` first.
#
# On macOS the programs are signed first with the development identity (scripts/macos-dev-sign.sh,
# as `cargo run` does): a core built by `cargo build` is signed ad hoc, macOS asks for its keychain
# access, and the core waits on that dialog instead of starting – which measures the dialog.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

profile=debug
cold=false
core=false
runs=5
while [ $# -gt 0 ]; do
    case "$1" in
        --release) profile=release; shift ;;
        --cold) cold=true; shift ;;
        --core) core=true; shift ;;
        --runs)
            [ $# -ge 2 ] || { echo "--runs needs a number" >&2; exit 64; }
            runs="$2"; shift 2 ;;
        -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
        --*) echo "unknown option $1" >&2; exit 64 ;;
        *) break ;;
    esac
done

bin="target/$profile"
workplace="$bin/torroworkplace"
[ -x "$workplace" ] || { echo "$workplace is missing; build it first (cargo build, --release for target/release)" >&2; exit 1; }
if ! $core; then
    [ $# -ge 1 ] || { echo "which app? e.g. torroinbox --view mail" >&2; exit 64; }
    app="$1"; shift
    [ -x "$bin/$app" ] || { echo "$bin/$app is missing; build it first" >&2; exit 1; }
fi

programs=("$workplace")
$core || programs+=("$bin/$app")
scripts/macos-dev-sign.sh "${programs[@]}"

# How long a run waits for its last mark, in steps of WAIT_STEP seconds.
WAIT_STEP=0.02
WAIT_STEPS=750

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Ends the running core and waits until it no longer answers.
quit_core() {
    "$workplace" ctl quit >/dev/null 2>&1 || true
    for _ in $(seq 1 "$WAIT_STEPS"); do
        "$workplace" status >/dev/null 2>&1 || return 0
        sleep "$WAIT_STEP"
    done
    echo "the core did not end" >&2
    exit 1
}

# Waits until `pattern` is in `file`, at most WAIT_STEPS steps.
wait_for() {
    local file="$1" pattern="$2"
    for _ in $(seq 1 "$WAIT_STEPS"); do
        grep -q -- "$pattern" "$file" 2>/dev/null && return 0
        sleep "$WAIT_STEP"
    done
    return 1
}

# The time of the first line with `step` in `file`, or "-": "+123.4ms first data" → 123.4.
at() {
    awk -v step="$2" '
        $1 == "torro-timing" {
            line = $0; sub(/^torro-timing [^ ]+ \+[0-9.]+ms /, "", line); sub(/ took=.*$/, "", line)
            if (line == step) { t = $3; sub(/^\+/, "", t); sub(/ms$/, "", t); print t; found = 1; exit }
        }
        END { if (!found) print "-" }' "$1"
}

# The duration of the first line with `step` in `file`, or "-": "load took=3.2ms" → 3.2.
took() {
    awk -v step="$2" '
        $1 == "torro-timing" && $4 == step && $5 ~ /^took=/ {
            t = $5; sub(/^took=/, "", t); sub(/ms$/, "", t); print t; found = 1; exit
        }
        END { if (!found) print "-" }' "$1"
}

# The median of the numbers in the lines of `file` (lines with "-" are left out).
median() {
    grep -v -- '^-$' "$1" | sort -n | awk '{ v[NR] = $1 } END {
        if (NR == 0) { print "-"; exit }
        if (NR % 2) print v[(NR + 1) / 2]; else printf "%.1f\n", (v[NR / 2] + v[NR / 2 + 1]) / 2 }'
}

if $core; then
    printf '%-4s %10s %10s %10s %10s %10s %14s\n' run clients store designs pim core socket
    columns=(clients store designs pim core "socket task started")
    for run in $(seq 0 "$runs"); do
        quit_core
        log="$work/core-$run.log"
        TORRO_TIMING=1 "$workplace" daemon >/dev/null 2>"$log" &
        wait_for "$log" "socket task started" || true
        label=$run; [ "$run" -eq 0 ] && label="(0)"
        row=("$label")
        for i in "${!columns[@]}"; do
            value=$(at "$log" "${columns[$i]}")
            row+=("$value")
            [ "$run" -gt 0 ] && echo "$value" >>"$work/col-$i"
        done
        printf '%-4s %10s %10s %10s %10s %10s %14s\n' "${row[@]}"
    done
    row=(median)
    for i in "${!columns[@]}"; do row+=("$(median "$work/col-$i")"); done
    printf '%-4s %10s %10s %10s %10s %10s %14s\n' "${row[@]}"
    echo "(0) is not counted; the core started last keeps running."
    exit 0
fi

printf '%-4s %12s %12s %10s\n' run "first frame" "first data" load
for run in $(seq 0 "$runs"); do
    $cold && quit_core
    log="$work/app-$run.log"
    TORRO_TIMING=1 "$bin/$app" "$@" >/dev/null 2>"$log" &
    pid=$!
    wait_for "$log" "first data" || true
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    frame=$(at "$log" "first frame")
    data=$(at "$log" "first data")
    load=$(took "$log" load)
    label=$run; [ "$run" -eq 0 ] && label="(0)"
    printf '%-4s %12s %12s %10s\n' "$label" "$frame" "$data" "$load"
    if [ "$run" -gt 0 ]; then
        echo "$frame" >>"$work/frame"
        echo "$data" >>"$work/data"
        echo "$load" >>"$work/load"
    fi
done
printf '%-4s %12s %12s %10s\n' median "$(median "$work/frame")" "$(median "$work/data")" "$(median "$work/load")"
echo "(0) is not counted. Times in ms since the program started; load is the first load's answer."
