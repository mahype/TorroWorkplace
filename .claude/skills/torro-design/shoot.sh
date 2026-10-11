#!/usr/bin/env bash
# Screenshots one window (by PID) in the six size classes of the torro-design skill.
# usage: shoot.sh <pid> <prefix> <out-dir> ["W H" ...]
# Needs Hyprland >= 0.56 (Lua dispatchers), grim and jq. Touches only the window of <pid>.
set -euo pipefail
pid=$1; prefix=$2; out=$3; shift 3
sizes=("$@")
[ ${#sizes[@]} -eq 0 ] && sizes=("240 160" "320 480" "1200 200" "480 900" "800 600" "1600 1000")
mkdir -p "$out"
hyprctl dispatch "hl.dsp.window.float({ action = 'enable', window = 'pid:$pid' })" >/dev/null
for s in "${sizes[@]}"; do
  read -r w h <<<"$s"
  hyprctl dispatch "hl.dsp.window.resize({ x = $w, y = $h, window = 'pid:$pid' })" >/dev/null
  hyprctl dispatch "hl.dsp.window.move({ x = 60, y = 60, window = 'pid:$pid' })" >/dev/null
  sleep 1.2
  geo=$(hyprctl clients -j | jq -r --argjson p "$pid" \
    '.[] | select(.pid==$p) | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"' | head -1)
  grim -g "$geo" "$out/$prefix-${w}x${h}.png"
  # The real size differs when the app's minimum size wins - that is a finding.
  echo "${w}x${h} -> $geo"
done
