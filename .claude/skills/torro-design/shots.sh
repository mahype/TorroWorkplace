#!/usr/bin/env bash
# Pictures of one app in the six size classes of the torro-design skill, without a window (#492).
# usage: shots.sh <app> <dir> [<state> ...] [-- <parameter> ...]
# Builds <app> and runs it once per state with `--demo --open <state> <parameter> --shots <dir>`
# (without a state once without --open). The app renders its view in memory (torro_gui::shots):
# no window, no Dock icon, no focus change, no Hyprland needed. Stops at the first failure with
# its exit code.
set -euo pipefail

usage() {
  echo "usage: $(basename "$0") <app> <dir> [<state> ...] [-- <parameter> ...]" >&2
  echo "  <app>: torrocalendar torrotasks torroinbox torroassistant torrochat torrocontacts torroworkplace" >&2
  echo "  e.g.:  $(basename "$0") torrocalendar /tmp/shots week editor -- --appearance dark" >&2
  exit 64
}

[ $# -ge 2 ] || usage
app=$1; out=$2; shift 2
case $app in
  torrocalendar|torrotasks|torroinbox|torroassistant|torrochat|torrocontacts) sub=() ;;
  torroworkplace) sub=(ui) ;;
  *) usage ;;
esac

states=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do states+=("$1"); shift; done
[ $# -gt 0 ] && shift
params=("$@")

# The folder as the caller meant it, before changing to the repository's root.
case $out in /*) ;; *) out="$PWD/$out" ;; esac
root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root"
cargo build -q -p "$app"
# Where cargo put it: CARGO_TARGET_DIR or `build.target-dir` of a cargo config may move `target/`.
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
binary="${target:-target}/debug/$app"

run() {
  # `${arr[@]+…}` keeps empty arrays from tripping `set -u` in the bash 3.2 of macOS.
  "$binary" ${sub[@]+"${sub[@]}"} --demo "$@" ${params[@]+"${params[@]}"} --shots "$out"
}

if [ ${#states[@]} -eq 0 ]; then
  run
else
  for state in "${states[@]}"; do run --open "$state"; done
fi
