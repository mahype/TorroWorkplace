#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Stop this checkout's apps as well as the core, so an already open Mail window
# cannot keep running the previous build after a restart from the control centre.
for APP_BINARY in torroinbox torroassistant torrocalendar torrotasks torroworkplace; do
    PROCESS_PATTERN="$(printf '%s' "$ROOT_DIR/target/debug/$APP_BINARY" | sed 's/[][\.^$*+?(){}|]/\\&/g')"
    pkill -f "^${PROCESS_PATTERN}( |$)" >/dev/null 2>&1 || true
done

# Menu entries launch the suite's separate app binaries next to the core.
cargo build --workspace --bins --locked --target-dir "$ROOT_DIR/target"
exec "$ROOT_DIR/target/debug/torroworkplace" ui
