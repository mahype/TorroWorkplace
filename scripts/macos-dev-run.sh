#!/bin/sh
# The Cargo runner on macOS (.cargo/config.toml, task X.14): signs the program `cargo run` starts
# with the identity "Torro Development" (scripts/macos-dev-sign.sh), then starts it. Test binaries
# (target/*/deps) are started as they are; without the identity nothing is signed.
set -eu

case "$1" in
*/deps/*) ;;
*) "$(dirname "$0")/macos-dev-sign.sh" "$1" ;;
esac
exec "$@"
