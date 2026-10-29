#!/usr/bin/env bash
#
# ci-local.sh – runs the checks of .github/workflows/ci.yml on this machine.
#
# Why: GitHub Actions only runs for releases (tags) because every runner minute is billed. Instead the
# checks run here, before a commit (fast) and before a push (scaled to the change), via the hooks in .githooks/.
# Linux and Windows are only covered by the release run; this script checks the host system.
#
# Usage:
#   scripts/ci-local.sh                     full set: fmt, design check, clippy, tests (same order as ci.yml)
#   scripts/ci-local.sh --quick             fmt and design check only (seconds, used by pre-commit)
#   scripts/ci-local.sh --changed <range>   picks the stage from `git diff --name-only <range>` (used by pre-push):
#                                           1 quick    no Rust code changed
#                                           2 i18n     only crates/torro-core/src/i18n/** changed
#                                           3 targeted clippy and tests of the changed crates (default and
#                                                      TORRO_LANG=de), only the targets the paths belong to
#                                           4 full     Cargo manifests/toolchain, unmappable paths, > 5 crates
#   scripts/ci-local.sh --scope <area>      clippy and tests of an area's crates, for manual runs (see scope_crates)
#
# Stage 3 does not check dependent crates: they only break through interfaces, which small changes must not touch.
# The next full run (PR, release) catches the rest. Choosing is by path only, never by the content of the diff.
#
# Skip once (e.g. a WIP push): git push --no-verify, git commit --no-verify.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

MAX_CRATES=5

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

done_ok() { printf '\n\033[1;32mAll CI checks passed.\033[0m\n'; }

check_basics() {
    step "cargo fmt --all --check"
    cargo fmt --all --check

    step "design check"
    scripts/design-check.sh --self-test
    scripts/design-check.sh --strict
}

run_full() {
    step "cargo clippy --workspace --all-targets -- -D warnings"
    cargo clippy --workspace --all-targets -- -D warnings

    step "cargo test --workspace"
    cargo test --workspace
}

# Crates of an area, for --scope; the areas follow the area labels of the issue workflow.
scope_crates() {
    case "$1" in
        core) echo "torro-core torro-ipc torroworkplace" ;;
        mail) echo "torro-mail torro-mail-imap torro-mail-backup" ;;
        gui) echo "torro-gui torro-settings-ui torro-htmlview torro-appkit torrocalendar torrotasks torroinbox torroassistant" ;;
        calendar) echo "torrocalendar torro-pim torro-pim-caldav" ;;
        tasks) echo "torrotasks torro-pim torro-pim-caldav torro-pim-github" ;;
        inbox) echo "torro-inbox torro-inbox-mail torro-inbox-notifications torroinbox" ;;
        caldav) echo "torro-pim-caldav torro-nextcloud" ;;
        contacts) echo "torro-contacts torro-nextcloud" ;;
        wm) echo "torro-wm torro-setups torro-tui" ;;
        ai) echo "torroassistant" ;;
        *) return 1 ;;
    esac
}

# Clippy and tests for one crate; the remaining arguments are cargo target flags (default: everything).
check_crate() {
    local crate="$1"
    shift
    local clippy_targets=("$@") test_targets=("$@")
    if [[ $# -eq 0 ]]; then
        clippy_targets=(--all-targets)
        test_targets=()
    fi

    step "cargo clippy -p $crate ${clippy_targets[*]-} -- -D warnings"
    cargo clippy -p "$crate" ${clippy_targets[@]+"${clippy_targets[@]}"} -- -D warnings

    step "cargo test -p $crate ${test_targets[*]-}"
    cargo test -p "$crate" ${test_targets[@]+"${test_targets[@]}"}

    step "TORRO_LANG=de cargo test -p $crate ${test_targets[*]-}"
    TORRO_LANG=de cargo test -p "$crate" ${test_targets[@]+"${test_targets[@]}"}
}

run_scope() {
    local crates
    if ! crates=$(scope_crates "$1"); then
        echo "unknown scope '$1'; known: core mail gui calendar tasks inbox caldav contacts wm ai" >&2
        exit 2
    fi
    check_basics
    local crate
    for crate in $crates; do
        check_crate "$crate"
    done
    done_ok
}

run_i18n() {
    step "cargo test -p torro-core --lib i18n"
    cargo test -p torro-core --lib i18n
    step "TORRO_LANG=de cargo test -p torro-core --lib i18n"
    TORRO_LANG=de cargo test -p torro-core --lib i18n
}

# Reads the changed paths on stdin and prints the stage plus, for stage 3, one "<crate> <flag>" line per
# target; a crate with the flag "all" is checked completely. Output: first line "stage <n> <reason>".
plan_changes() {
    local lines="" path crate rest
    local stage=1 reason="no Rust code changed"
    while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        case "$path" in
            Cargo.toml | Cargo.lock | */Cargo.toml | rust-toolchain* | .cargo/config* | clippy.toml)
                echo "stage 4 $path changes dependencies or toolchain"
                return
                ;;
            crates/torro-core/src/i18n/*)
                if [[ $stage -lt 2 ]]; then
                    stage=2 reason="only translations changed"
                fi
                lines+="torro-core src"$'\n'
                ;;
            crates/*/*)
                crate="${path#crates/}"
                rest="${crate#*/}"
                crate="${crate%%/*}"
                stage=3 reason="Rust code changed"
                case "$rest" in
                    src/main.rs | src/bin/*) lines+="$crate bins"$'\n' ;;
                    src/*) lines+="$crate src"$'\n' ;;
                    tests/*/*) lines+="$crate all"$'\n' ;;
                    tests/*.rs) rest="${rest#tests/}"; lines+="$crate test:${rest%.rs}"$'\n' ;;
                    *) lines+="$crate all"$'\n' ;;
                esac
                ;;
            docs/* | scripts/* | .claude/* | .github/* | .githooks/* | .enon-tools/* | apps/* | spikes/* | *.md) ;;
            *)
                echo "stage 4 $path cannot be mapped to a crate"
                return
                ;;
        esac
    done
    # i18n files also queue the torro-core lib, so a mixed change still tests them in stage 3.
    echo "stage $stage $reason"
    printf '%s' "$lines"
}

run_changed() {
    local files plan stage_line
    files=$(for range in "$@"; do git diff --name-only "$range"; done | sort -u)
    plan=$(plan_changes <<<"$files")
    stage_line=$(head -n 1 <<<"$plan")
    step "changed files: $(grep -c . <<<"$files" || true) – $stage_line"

    case "$stage_line" in
        "stage 1"*)
            check_basics
            ;;
        "stage 2"*)
            check_basics
            run_i18n
            ;;
        "stage 3"*)
            local crates crate flags n
            crates=$(tail -n +2 <<<"$plan" | cut -d' ' -f1 | sort -u)
            n=$(grep -c . <<<"$crates" || true)
            if [[ $n -gt $MAX_CRATES ]]; then
                echo "stage 4: $n crates changed (more than $MAX_CRATES), running the full set"
                check_basics
                run_full
                done_ok
                return
            fi
            check_basics
            for crate in $crates; do
                local args=() has_src=0 has_all=0 has_bins=0 line flag
                while IFS= read -r line; do
                    flag="${line#"$crate" }"
                    case "$flag" in
                        all) has_all=1 ;;
                        src) has_src=1 ;;
                        bins) has_bins=1 ;;
                        test:*) args+=(--test "${flag#test:}") ;;
                    esac
                done < <(tail -n +2 <<<"$plan" | grep "^$crate ")
                if [[ $has_all -eq 1 ]]; then
                    check_crate "$crate"
                    continue
                fi
                if [[ $has_src -eq 1 && -f "crates/$crate/src/lib.rs" ]]; then
                    args+=(--lib)
                fi
                # A binary-only crate has no lib target, so its src changes mean the bins.
                if [[ $has_bins -eq 1 || ($has_src -eq 1 && ! -f "crates/$crate/src/lib.rs") ]]; then
                    args+=(--bins)
                fi
                check_crate "$crate" ${args[@]+"${args[@]}"}
            done
            ;;
        *)
            echo "${stage_line#stage 4 }: running the full set"
            check_basics
            run_full
            ;;
    esac
    done_ok
}

case "${1:-}" in
    "")
        check_basics
        run_full
        done_ok
        ;;
    --quick)
        check_basics
        ;;
    --changed)
        shift
        if [[ $# -eq 0 ]]; then
            echo "usage: scripts/ci-local.sh --changed <range>..." >&2
            exit 2
        fi
        run_changed "$@"
        ;;
    --scope)
        if [[ $# -ne 2 ]]; then
            echo "usage: scripts/ci-local.sh --scope <area>" >&2
            exit 2
        fi
        run_scope "$2"
        ;;
    *)
        echo "usage: scripts/ci-local.sh [--quick | --changed <range>... | --scope <area>]" >&2
        exit 2
        ;;
esac
