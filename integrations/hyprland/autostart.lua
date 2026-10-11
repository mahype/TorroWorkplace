-- Autostart for TorroWorkplace on Hyprland (Omarchy 4, Lua config).
--
-- Include it from your Hyprland config, after Omarchy's defaults are loaded:
--
--   Option 1: paste the line below into ~/.config/hypr/autostart.lua. Omarchy's
--             hyprland.lua already requires that file.
--
--   Option 2: copy this file to ~/.config/hypr/torroworkplace.lua and add
--             require("hypr.torroworkplace")
--             to ~/.config/hypr/hyprland.lua, next to require("hypr.autostart").
--
-- `o.launch_on_start` comes from Omarchy's default/hypr/helpers.lua. It runs the
-- command on "hyprland.start" wrapped in `uwsm-app --`, like Omarchy's own autostarts.
-- Do not enable the systemd unit (torroworkplace.service) at the same time: two
-- daemons would fight over the same socket.

o.launch_on_start("torroworkplace daemon")
