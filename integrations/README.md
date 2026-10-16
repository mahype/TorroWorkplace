# Integrations

Window-manager glue, one folder per target (filled in during the MVP):

- `hyprland/`: window rules on `class:^torro-<app>$` and `.desktop` launchers
- `aerospace/`: `[[on-window-detected]]` rules for `~/.aerospace.toml`
- `omarchy/`: bar widgets on top of `<app> status --json` (pattern: omarchy-torromail)
- `glazewm/`: a rule that keeps GlazeWM from tiling the reminder over the whole screen (task ER2.2)

App ids live in `crates/torro-core/src/app.rs`.

## Hyprland / Omarchy

Build and install the core (the binary goes to `~/.cargo/bin/torroworkplace`):

```sh
cargo install --path crates/torroworkplace
```

Then start the daemon at login, either way (not both):

**A: Lua autostart (Omarchy 4, Hyprland 0.56).** `hyprland/autostart.lua` contains
`o.launch_on_start("torroworkplace daemon")`. Put that line into `~/.config/hypr/autostart.lua`,
which Omarchy's `hyprland.lua` already loads. Or copy the file to `~/.config/hypr/torroworkplace.lua`
and add `require("hypr.torroworkplace")` to `~/.config/hypr/hyprland.lua` after
`require("hypr.autostart")`. Log out and back in; the tray icon should appear.

**B: systemd user unit.** Use this if you want systemd to restart the daemon on failure:

```sh
cp integrations/hyprland/torroworkplace.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now torroworkplace
```

If you installed the binary somewhere other than `~/.cargo/bin`, change `ExecStart` in the unit.

Check that the daemon runs:

```sh
torroworkplace status          # or: torroworkplace status --json
```

Bind a workspace template (saved with `torroworkplace ctl setups save <name>`) to a key. Find
its id with `torroworkplace ctl setups list`. Example for `~/.config/hypr/bindings.lua`:

```lua
o.bind("SUPER + ALT + 1", "Workspace: dev", "torroworkplace ctl setups load <id>")
```

**Opaque Torro windows (Omarchy).** Omarchy makes every window slightly transparent, so text behind
the very dark Torro windows shows through. `hyprland/windowrules.lua` keeps the windows of all Torro
apps (`^torro-.*$`) fully opaque. Install it with
`cp integrations/hyprland/windowrules.lua ~/.config/hypr/torro-windowrules.lua`, then add
`require("hypr.torro-windowrules")` to `~/.config/hypr/hyprland.lua` after `require("hypr.autostart")`.
The rule applies to windows opened afterwards; restart the Torro apps once.

## Reminders over the whole screen

A reminder in the style "full screen" opens `torrocalendar --reminders` (window class `torro-reminder`,
`docs/erinnerungen.md`). On **Hyprland** the core registers a rule per monitor before the window opens, so
nothing needs configuring. On **GlazeWM** add the rule in `glazewm/torro-reminder.yaml` to your config. On
**macOS** install "Torro Reminder.app" next to "Torro Workplace.app" (the release builds it): as an agent app
it shows over apps in full screen; without it the window covers only the desktop.
