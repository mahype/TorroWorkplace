#!/bin/bash
# Build one signed, notarised drag-and-drop image from already stapled app bundles.
# Usage: make-dmg.sh <program> <version> <target> <output-dir> <app.app> [companion.app ...]
set -euo pipefail
if [ "$#" -lt 5 ]; then echo "Expected program, version, target, output folder and apps" >&2; exit 1; fi
program="$1" version="$2" target="$3" output="$4"
shift 4
case "$target" in aarch64-apple-darwin|x86_64-apple-darwin) ;; *) exit 1 ;; esac
[[ "$program" =~ ^[a-z][a-z0-9-]+$ && "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]] || exit 1
: "${MACOS_SIGN_IDENTITY:?Developer ID identity required}"
: "${APPLE_ID:?Apple ID required}" "${APPLE_TEAM_ID:?Team ID required}" "${APPLE_APP_SPECIFIC_PASSWORD:?Notarisation password required}"
mkdir -p "$output"
work="$(mktemp -d)"
mount="$work/mounted"
cleanup() {
  if mount | grep -F " on $mount " >/dev/null; then hdiutil detach "$mount" -quiet || hdiutil detach "$mount" -force -quiet; fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir "$work/image" "$mount"
for app in "$@"; do
  [ -d "$app" ] && [[ "$app" == *.app ]] || exit 1
  codesign --verify --deep --strict "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute "$app"
  ditto "$app" "$work/image/$(basename "$app")"
done
ln -s /Applications "$work/image/Applications"
cat > "$work/image/Install.txt" <<'TEXT'
Install TorroWorkplace

Drag ALL apps shown in this image to Applications, then eject the image.
Mail and App Feed include the Notification Hub (needed for composing mail).
Calendar includes Torro Reminder (needed for full-screen reminders).
The background service is already inside each app; do not copy it separately.
Open your app from Applications, not from this disk image.

To update, quit the apps and replace the copies in Applications.
To uninstall, quit the apps and move only the apps you no longer need to Trash.
Keep Notification Hub while Mail or App Feed is installed; keep Torro Reminder with Calendar.
Your accounts, settings and personal data are not removed.
TEXT
asset="$program-$version-$target.dmg"
hdiutil create -quiet -volname "TorroWorkplace $version" -srcfolder "$work/image" -format UDZO "$work/$asset"
codesign --timestamp --sign "$MACOS_SIGN_IDENTITY" "$work/$asset"
codesign --verify --strict "$work/$asset"
xcrun notarytool submit "$work/$asset" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" --wait --timeout 30m
xcrun stapler staple "$work/$asset"
xcrun stapler validate "$work/$asset"
spctl --assess --type open --context context:primary-signature "$work/$asset"
hdiutil verify "$work/$asset"
hdiutil attach "$work/$asset" -readonly -nobrowse -mountpoint "$mount" -quiet
[ -L "$mount/Applications" ] && [ "$(readlink "$mount/Applications")" = /Applications ]
for app in "$@"; do [ -d "$mount/$(basename "$app")" ]; done
hdiutil detach "$mount" -quiet
mv "$work/$asset" "$output/$asset"
(cd "$output" && shasum -a 256 "$asset" > "$asset.sha256")
