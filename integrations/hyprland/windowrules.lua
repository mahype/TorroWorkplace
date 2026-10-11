-- Opaque Torro windows on Hyprland (Omarchy 4, Lua config).
--
-- Omarchy makes almost every window slightly transparent (tag "default-opacity", opacity
-- "0.985 0.96", see default/hypr/windows.lua). The Torro windows are very dark, so text from
-- whatever lies behind them shows through. This rule keeps all Torro windows fully opaque.
--
-- Include it from your Hyprland config, after Omarchy's defaults are loaded:
--
--   copy this file to ~/.config/hypr/torro-windowrules.lua and add
--   require("hypr.torro-windowrules")
--   to ~/.config/hypr/hyprland.lua, after require("hypr.autostart").
--
-- The match is the window class (app id) of every Torro app: torro-workplace, torro-tasks,
-- torro-calendar. They are set in crates/torro-core/src/app.rs.
-- Omarchy's own exceptions (default/hypr/apps/*.lua) remove the tag the same way, and the
-- later rule wins, so the opacity "1 1" set here overrides Omarchy's value.

o.window("^torro-.*$", { tag = "-default-opacity", opacity = "1 1" })
