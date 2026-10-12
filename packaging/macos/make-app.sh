#!/usr/bin/env bash
# Wraps a released GUI binary into a macOS .app bundle (run on macOS).
#
#   make-app.sh <bin> <app-id> <title> <version> <binary-path> <out-dir> [icon.png]
#   make-app.sh torrocalendar torro-calendar "Torro Calendar" 0.1.0 ./torrocalendar ./out [icon-1024.png]
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

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/$bin"
chmod +x "$app/Contents/MacOS/$bin"

# CFBundleVersion and CFBundleShortVersionString allow only digits and dots: 0.2.0-rc.1 becomes 0.2.0.
bundle_version="${version%%-*}"

sed -e "s|@TITLE@|$title|g" -e "s|@BIN@|$bin|g" -e "s|@APPID@|$appid|g" -e "s|@VERSION@|$bundle_version|g" \
  "$here/Info.plist.in" > "$app/Contents/Info.plist"

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
