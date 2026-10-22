#!/usr/bin/env bash
# Builds the suite's macOS bundles from the development programs in target/debug (task AP.5, issue #279), so
# starting apps and the core through their bundles can be checked by hand without a release.
#
#   scripts/macos-dev-bundles.sh [out-dir]     default: target/debug/bundles
#   scripts/macos-dev-bundles.sh ~/Applications/Torro\ Dev
#
# Makes nine bundles with packaging/macos/make-app.sh, named and laid out as torro_gui::source::bundle_program and
# SERVICE_PROGRAM look for them (`<AppId::display_name>.app/Contents/MacOS/<AppId::program_file>`): the seven apps,
# the core's service as an agent bundle inside "Torro Workplace.app", and the reminder. Each is signed with the
# development identity (scripts/macos-dev-sign.sh, the embedded service before the outer bundle); without that
# identity each bundle is signed ad hoc as a whole, since the linker's signature of the bare program does not cover
# the bundle's resources and `codesign --verify` would reject the bundle. Each bundle gets its icon from packaging/macos/icons/ (task AP.7): the service the
# control centre's, the reminder the calendar's. The service keeps the signing identifier of the program
# `torroworkplace`, so the keychain entries of the development core stay readable.
#
# It builds nothing with cargo: run `cargo build --workspace` first. It opens and starts nothing either.
set -euo pipefail

[ "$(uname -s)" = Darwin ] || { echo "macos-dev-bundles.sh: macOS only" >&2; exit 2; }

root="$(cd "$(dirname "$0")/.." && pwd)"
programs="$root/target/debug"
out="${1:-$programs/bundles}"
make_app="$root/packaging/macos/make-app.sh"
icons="$root/packaging/macos/icons"
sign="$root/scripts/macos-dev-sign.sh"

# The version of the workspace ([workspace.package] in Cargo.toml), for CFBundleVersion.
version="$(awk '/^\[workspace.package\]/ { inside = 1; next } /^\[/ { inside = 0 }
  inside && $1 == "version" { gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.toml")"
[ -n "$version" ] || { echo "macos-dev-bundles.sh: no version in Cargo.toml" >&2; exit 1; }

# One line per bundle: program file in the bundle | program in target/debug | bundle id suffix | title | agent app |
# icon in packaging/macos/icons. Program file, id suffix (the window class) and title follow AppId::program_file,
# window_class and display_name in crates/torro-core/src/app.rs; the reminder follows torrocalendar's reminder (task
# ER2.2) and shows the calendar's icon.
apps='torroworkplace|torroworkplace|torro-workplace|Torro Workplace|0|torro-workplace
torroinbox|torroinbox|torro-inbox|Torro Notification Hub|0|torro-inbox
torro-inbox-mail|torro-inbox-mail|torro-inbox-mail|Torro Workplace Mail|0|torro-inbox-mail
torro-inbox-notifications|torro-inbox-notifications|torro-inbox-notifications|Torro App Feed|0|torro-inbox-notifications
torrocalendar|torrocalendar|torro-calendar|Torro Calendar|0|torro-calendar
torrotasks|torrotasks|torro-tasks|Torro Tasks|0|torro-tasks
torroassistant|torroassistant|torro-assistant|Torro Assistant|0|torro-assistant
torro-reminder|torrocalendar|torro-reminder|Torro Reminder|1|torro-calendar'

missing=""
for program in torroworkplace torroinbox torro-inbox-mail torro-inbox-notifications torrocalendar torrotasks \
    torroassistant; do
    [ -x "$programs/$program" ] || missing="$missing $program"
done
if [ -n "$missing" ]; then
    echo "macos-dev-bundles.sh: missing in target/debug:$missing (run cargo build --workspace)" >&2
    exit 1
fi

mkdir -p "$out"

# Signs `bundle` through its program `program`: with the development identity when there is one, else ad hoc.
if security find-identity -v -p codesigning 2>/dev/null | grep -q '"Torro Development"'; then
    sign_bundle() { "$sign" "$2"; }
else
    sign_bundle() { codesign --force --sign - "$1" >/dev/null 2>&1 || echo "torro: could not sign $1" >&2; }
fi

# The core's service first: an agent bundle (no Dock icon), signed, then embedded into "Torro Workplace.app".
service_dir="$(mktemp -d)"
trap 'rm -rf "$service_dir"' EXIT
AGENT_APP=1 "$make_app" torroworkplace torro-workplace-service "Torro Workplace Service" "$version" \
    "$programs/torroworkplace" "$service_dir" "$icons/torro-workplace.png" >/dev/null
sign_bundle "$service_dir/Torro Workplace Service.app" \
    "$service_dir/Torro Workplace Service.app/Contents/MacOS/torroworkplace"

while IFS='|' read -r file program appid title agent icon; do
    embed=""
    [ "$appid" = torro-workplace ] && embed="$service_dir/Torro Workplace Service.app"
    AGENT_APP="$agent" EMBED_APP="$embed" "$make_app" "$file" "$appid" "$title" "$version" "$programs/$program" \
        "$out" "$icons/$icon.png" >/dev/null
    # Signing the program in a bundle signs the bundle; the embedded service is signed already.
    sign_bundle "$out/$title.app" "$out/$title.app/Contents/MacOS/$file"
    echo "$out/$title.app"
done <<< "$apps"
echo "$out/Torro Workplace.app/Contents/Library/Torro Workplace Service.app"
