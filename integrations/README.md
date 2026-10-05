# Integrations

Window-manager glue, one folder per target (filled in during the MVP):

- `hyprland/`: window rules on `class:^torro-<app>$` and `.desktop` launchers
- `aerospace/`: `[[on-window-detected]]` rules for `~/.aerospace.toml`
- `omarchy/`: bar widgets on top of `<app> status --json` (pattern: omarchy-torromail)

App ids live in `crates/torro-core/src/app.rs`.
