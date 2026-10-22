#!/usr/bin/env bash
# Wraps a released GUI binary into a macOS .app bundle (run on macOS).
#
#   make-app.sh <bin> <app-id> <title> <version> <binary-path> <out-dir> [icon.png]
#   make-app.sh torrocalendar torro-calendar "Torro Calendar" 0.1.0 ./torrocalendar ./out [icon-1024.png]
#
# With AGENT_APP=1 the bundle is an agent app (LSUIElement): no Dock icon, not in the app switcher. The
# reminder over the whole screen needs it to show over apps in full screen (task ER2.2, docs/erinnerungen.md):
#   AGENT_APP=1 make-app.sh torro-reminder torro-reminder "Torro Reminder" 0.1.0 ./torrocalendar ./out
# The calendar binary sees its name `torro-reminder` and starts as the reminder window.
#
# With EMBED_APP=<path of a finished .app> that bundle is copied into `Contents/Library/LoginItems/` of this one, where
# Apple wants helper apps (a bundle directly in `Contents/Library/` is sealed as a resource). The control
# centre carries the core's service this way (decision 7 in docs/lebenszyklus.md, issue #279): an agent bundle
# without a Dock icon, where `torro_gui::source::SERVICE_PROGRAM` looks for it:
#   AGENT_APP=1 make-app.sh torroworkplace torro-workplace-service "Torro Workplace Service" 0.1.0 ./torroworkplace ./svc
#   EMBED_APP="./svc/Torro Workplace Service.app" make-app.sh torroworkplace torro-workplace "Torro Workplace" \
#     0.1.0 ./torroworkplace ./out
# Sign the embedded bundle before the outer one: the outer signature seals the nested code as it is.
#
# Signing and notarisation are separate steps (see .github/workflows/publish-macos-app.yml).
set -euo pipefail

if [ "$#" -lt 6 ]; then
  echo "usage: $0 <bin> <app-id> <title> <version> <binary-path> <out-dir> [icon.png]" >&2
  exit 2
fi

bin="$1"; appid="$2"; title="$3"; version="$4"; binary="$5"; out="$6"; icon_png="${7:-}"
here="$(cd "$(dirname "$0")" && pwd)"
app="$out/$title.app"

# Checked before anything is made, so a wrong path leaves no half bundle behind.
if [ -n "${EMBED_APP:-}" ]; then
  case "$EMBED_APP" in
    *.app | *.app/) ;;
    *) echo "EMBED_APP must name an .app bundle: $EMBED_APP" >&2; exit 2 ;;
  esac
  if [ ! -d "$EMBED_APP" ]; then
    echo "EMBED_APP is not a directory: $EMBED_APP" >&2
    exit 2
  fi
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/$bin"
chmod +x "$app/Contents/MacOS/$bin"

# CFBundleVersion and CFBundleShortVersionString allow only digits and dots: 0.2.0-rc.1 becomes 0.2.0.
bundle_version="${version%%-*}"

sed -e "s|@TITLE@|$title|g" -e "s|@BIN@|$bin|g" -e "s|@APPID@|$appid|g" -e "s|@VERSION@|$bundle_version|g" \
  "$here/Info.plist.in" > "$app/Contents/Info.plist"
if [ "${AGENT_APP:-}" = "1" ]; then
  plutil -insert LSUIElement -bool true "$app/Contents/Info.plist"
fi

if [ -n "${EMBED_APP:-}" ]; then
  mkdir -p "$app/Contents/Library/LoginItems"
  # ditto keeps symlinks, permissions and extended attributes of the bundle (and a signature, if it has one).
  ditto "$EMBED_APP" "$app/Contents/Library/LoginItems/$(basename "$EMBED_APP")"
fi

# Optional icon: a 1024x1024 PNG becomes an .icns through sips + iconutil (both ship with macOS).
if [ -n "$icon_png" ]; then
  iconset="$(mktemp -d)/$appid.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$icon_png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" "$icon_png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/$appid.icns"
fi

echo "$app"
