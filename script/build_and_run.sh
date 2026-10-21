#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

# Stop this checkout's apps as well as the core, so an already open Mail window
# cannot keep running the previous build after a restart from the control centre.
# Inspect executable files: argv[0] may be a relative path, and must not cause us
# to miss an old daemon or terminate an app belonging to another checkout.
command -v lsof >/dev/null 2>&1 || { echo "lsof is required to stop this checkout's apps." >&2; exit 1; }
PROCESS_ID=""
while IFS= read -r PROCESS_FILE; do
    case "$PROCESS_FILE" in
        p*) PROCESS_ID="${PROCESS_FILE#p}" ;;
        n*)
            APP_PATH="${PROCESS_FILE#n}"
            APP_PATH="${APP_PATH% (deleted)}"
            case "$APP_PATH" in
                "$ROOT_DIR/target/debug/torroinbox"|"$ROOT_DIR/target/debug/torroassistant"|\
                "$ROOT_DIR/target/debug/torrocalendar"|"$ROOT_DIR/target/debug/torrotasks"|\
                "$ROOT_DIR/target/debug/torroworkplace")
                    [[ -z "$PROCESS_ID" ]] || kill "$PROCESS_ID" >/dev/null 2>&1 || true
                    ;;
            esac
            ;;
    esac
done < <(lsof -nP -a -c torro -d txt -Fpn 2>/dev/null || true)

# Menu entries launch the suite's separate app binaries next to the core.
cargo build --workspace --bins --locked --target-dir "$ROOT_DIR/target"
"$ROOT_DIR/scripts/macos-dev-sign.sh"
exec "$ROOT_DIR/target/debug/torroworkplace" ui
