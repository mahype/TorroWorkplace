# TorroWorkplace

A suite of small, separate desktop apps — calendar, tasks, and more to come — built to be
tiled side by side by a tiling window manager instead of living inside one big program.
Each app is its own process and its own window with a stable app id, so Hyprland,
AeroSpace (and later komorebi on Windows) can place it, and **workspace-setups** can save
and restore whole window sets.

**Status: planning / scaffold.** Nothing usable yet. See [docs/roadmap.md](docs/roadmap.md).

## What's in here

| Path | What |
|---|---|
| `crates/torro-core` | Shared config, paths, theme tokens |
| `crates/torro-pim` | Event/Todo model, provider traits, offline cache, sync |
| `crates/torro-pim-caldav` | CalDAV/iCal provider (Nextcloud, iCloud, …) |
| `crates/torro-tui` | Shared ratatui widgets and theme |
| `crates/torrotasks` | Tasks app (`torrotasks`) |
| `crates/torrocalendar` | Calendar app (`torrocalendar`) |
| `apps/workspace-setups` | Save and restore window sets (Omarchy/Hyprland; AeroSpace planned) |
| `integrations/` | Window rules and launchers for Hyprland, AeroSpace, Omarchy widgets |
| `packaging/` | AUR, Homebrew, later Scoop |
| `docs/` | Vision, research, architecture, app catalog, roadmap |

Every app offers the same outside interface: a TUI (now), a native GUI (next),
`status --json` and `ctl` for bars and scripts, and `mcp` so AI agents can read and write
its data.

## Build it

```
cargo check --workspace
```

## License

MIT OR Apache-2.0, at your option.
