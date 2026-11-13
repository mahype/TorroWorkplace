#!/usr/bin/env bash
# Builds the suite's macOS bundles from the development programs in target/debug (task AP.5, issue #279), so
# starting apps and the core through their bundles can be checked by hand without a release.
#
#   scripts/macos-dev-bundles.sh [out-dir]     default: target/debug/bundles
#   scripts/macos-dev-bundles.sh ~/Applications/Torro\ Dev
#   TORRO_PROGRAMS_DIR=target/release scripts/macos-dev-bundles.sh   programs from another folder (issue #629)
#
# Makes the bundles with packaging/macos/make-app.sh, named and laid out as torro_gui::source::bundle_program and
# SERVICE_PROGRAM look for them (`<AppId::display_name>.app/Contents/MacOS/<AppId::program_file>`): the eight apps,
# each with the core's service as an agent bundle inside (`Contents/Library/LoginItems/Torro Workplace Service.app`,
# issue #411, like the release in .github/workflows/publish-macos-app.yml), and the reminder without it. Each is
# signed with the development identity (scripts/macos-dev-sign.sh, the embedded service before the outer bundle); without that
# identity each bundle is signed ad hoc as a whole, since the linker's signature of the bare program does not cover
# the bundle's resources and `codesign --verify` would reject the bundle. Each bundle gets its icon from packaging/macos/icons/ (task AP.7): the service the
# control centre's, the reminder the calendar's. The service keeps the signing identifier of the program
# `torroworkplace`, so the keychain entries of the development core stay readable.
#
# It builds nothing with cargo: run `cargo build --workspace` first. It opens and starts nothing either.
set -euo pipefail

[ "$(uname -s)" = Darwin ] || { echo "macos-dev-bundles.sh: macOS only" >&2; exit 2; }

root="$(cd "$(dirname "$0")/.." && pwd)"
programs="${TORRO_PROGRAMS_DIR:-$root/target/debug}"
out="${1:-$programs/bundles}"
make_app="$root/packaging/macos/make-app.sh"
icons="$root/packaging/macos/icons"
sign="$root/scripts/macos-dev-sign.sh"

# The version of the workspace ([workspace.package] in Cargo.toml), for CFBundleVersion.
version="$(awk '/^\[workspace.package\]/ { inside = 1; next } /^\[/ { inside = 0 }
  inside && $1 == "version" { gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.toml")"
[ -n "$version" ] || { echo "macos-dev-bundles.sh: no version in Cargo.toml" >&2; exit 1; }

# One line per bundle: program file in the bundle | program in target/debug | bundle id suffix | title | agent app |
# icon in packaging/macos/icons (empty: none yet, the chat has no PNG) | embeds the service. Program file, id suffix
# (the window class) and title follow AppId::program_file, window_class and display_name in
# crates/torro-core/src/app.rs; the reminder follows
# torrocalendar's reminder (task ER2.2), shows the calendar's icon and has no service (the calendar's bundle has it).
apps='torroworkplace|torroworkplace|torro-workplace|Torro Workplace|0|torro-workplace|1
torroinbox|torroinbox|torro-inbox|Torro Notification Hub|0|torro-inbox|1
torro-inbox-mail|torro-inbox-mail|torro-inbox-mail|Torro Workplace Mail|0|torro-inbox-mail|1
torro-inbox-notifications|torro-inbox-notifications|torro-inbox-notifications|Torro App Feed|0|torro-inbox-notifications|1
torrocalendar|torrocalendar|torro-calendar|Torro Calendar|0|torro-calendar|1
torrotasks|torrotasks|torro-tasks|Torro Tasks|0|torro-tasks|1
torroassistant|torroassistant|torro-assistant|Torro Assistant|0|torro-assistant|1
torrochat|torrochat|torro-chat|Torro Chat|0||1
torro-reminder|torrocalendar|torro-reminder|Torro Reminder|1|torro-calendar|0'

missing=""
for program in torroworkplace torroinbox torro-inbox-mail torro-inbox-notifications torrocalendar torrotasks \
    torroassistant torrochat; do
    [ -x "$programs/$program" ] || missing="$missing $program"
done
if [ -n "$missing" ]; then
    case "$programs" in
        */release) build="cargo build --release" ;;
        *) build="cargo build --workspace" ;;
    esac
    echo "macos-dev-bundles.sh: missing in $programs:$missing (run $build)" >&2
    exit 1
fi

mkdir -p "$out"

# Signs `bundle` through its program `program`: with the development identity when there is one, else ad hoc.
if security find-identity -v -p codesigning 2>/dev/null | grep -q '"Torro Development"'; then
    sign_bundle() { "$sign" "$2"; }
else
    sign_bundle() { codesign --force --sign - "$1" >/dev/null 2>&1 || echo "torro: could not sign $1" >&2; }
fi

# The core's service first: an agent bundle (no Dock icon), signed once, then copied into every app's bundle. The
# copies are the same bundle (id io.github.mahype.torro-workplace-service), so the keychain trusts each of them.
service_dir="$(mktemp -d)"
trap 'rm -rf "$service_dir"' EXIT
AGENT_APP=1 "$make_app" torroworkplace torro-workplace-service "Torro Workplace Service" "$version" \
    "$programs/torroworkplace" "$service_dir" "$icons/torro-workplace.png" >/dev/null
sign_bundle "$service_dir/Torro Workplace Service.app" \
    "$service_dir/Torro Workplace Service.app/Contents/MacOS/torroworkplace"

while IFS='|' read -r file program appid title agent icon service; do
    embed=""
    [ "$service" = 1 ] && embed="$service_dir/Torro Workplace Service.app"
    AGENT_APP="$agent" EMBED_APP="$embed" "$make_app" "$file" "$appid" "$title" "$version" "$programs/$program" \
        "$out" ${icon:+"$icons/$icon.png"} >/dev/null
    # Signing the program in a bundle signs the bundle; the embedded service is signed already.
    sign_bundle "$out/$title.app" "$out/$title.app/Contents/MacOS/$file"
    echo "$out/$title.app"
    if [ "$service" = 1 ]; then
        echo "$out/$title.app/Contents/Library/LoginItems/Torro Workplace Service.app"
    fi
done <<< "$apps"
