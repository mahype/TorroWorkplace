#!/usr/bin/env bash
# Automatic checks of issue #917 on a running Hyprland session: draws after resize and tiling (screenshots
# in shots/), typing with umlauts, undo, select all across the pictures, paste of HTML. Run it as
#   ./auto-check.sh default        and        WEBKIT_DISABLE_DMABUF_RENDERER=1 ./auto-check.sh nodmabuf
# The window's own log (stdout) is the evidence: out-<label>.log. It takes over the keyboard for a few
# seconds, so keep your hands off.
set -u
label=${1:-default}
here=$(cd "$(dirname "$0")" && pwd)
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}
if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
  HYPRLAND_INSTANCE_SIGNATURE=$(ls "$XDG_RUNTIME_DIR/hypr" | head -n1)
  export HYPRLAND_INSTANCE_SIGNATURE
fi
log=${OUT_DIR:-$here/shots}/out-$label.log
mkdir -p "$(dirname "$log")"
shot() { # name: screenshot of the window, then its pixel statistics
  local g
  g=$(hyprctl clients -j | jq -r '.[] | select(.class=="editor-webview") | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')
  grim -g "$g" "$here/shots/$label-$1.png"
  python3 -I - "$here/shots/$label-$1.png" "$g" <<'PY'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("L")
px = list(im.get_flattened_data() if hasattr(im, 'get_flattened_data') else im.getdata())
dark = sum(1 for p in px if p < 128)
print(f"shot {sys.argv[1].split('/')[-1]} window={sys.argv[2]} size={im.size} distinct_levels={len(set(px))} dark_pixels={dark}")
PY
}
W='window = "class:editor-webview"'
dsp() { hyprctl dispatch "$1" >/dev/null; }
focus() { dsp "hl.dsp.focus({ $W })"; sleep 0.4; }
key() { wtype "$@"; sleep 0.3; }
"$here/target/release/editor-webview" >"$log" 2>"$log.err" &
pid=$!
sleep 3
focus
shot 1-start
dsp "hl.dsp.window.resize({ x = 150, y = 0, relative = true, $W })"; sleep 1; shot 2-tiled-wider
dsp "hl.dsp.window.resize({ x = -300, y = 0, relative = true, $W })"; sleep 1; shot 3-tiled-narrower
dsp "hl.dsp.window.float({ action = \"toggle\", $W })"; sleep 0.5
dsp "hl.dsp.window.resize({ x = 520, y = 420, $W })"; sleep 1; shot 4-floating-small
dsp "hl.dsp.window.resize({ x = 900, y = 700, $W })"; sleep 1; shot 5-floating-large
focus
# Typing: umlauts go through wtype (virtual keyboard), the end of the text first.
key -M ctrl -k End -m ctrl
key "Zweite Zeile: Größe äöü ß „Öl“"
shot 6-typed
key -M ctrl z -m ctrl; key -M ctrl z -m ctrl
shot 7-undone
key -M ctrl -M shift z -m shift -m ctrl
# Select all: does the selection run across the pictures?
key -M ctrl a -m ctrl
sleep 0.5
shot 8-select-all
key -k Right
# Paste HTML as Office/Browser would offer it.
wl-copy --type text/html <"$here/fixtures/paste-synthetic.html"
key -M ctrl v -m ctrl
sleep 0.5
shot 9-pasted
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
echo "--- log $log"
cat "$log"
