# TorroWorkplace

A suite of small, separate desktop apps around one core — calendar, tasks, and more to
come — built to be tiled side by side by a tiling window manager instead of living inside
one big program.
The core, `torroworkplace`, is a background service with a tray icon: it starts the apps,
holds accounts and sync for all of them, loads window sets and offers one MCP server for AI
agents. Each app is its own process and its own window with a stable app id, so Hyprland,
AeroSpace (and later komorebi on Windows) can place it, and **workspace-setups** can save
and restore whole window sets.

**Status: planning / scaffold.** Nothing usable yet. See [docs/roadmap.md](docs/roadmap.md).

## What's in here

| Path | What |
|---|---|
| `crates/torro-core` | Shared config, paths, theme tokens |
| `crates/torroworkplace` | The core: service, tray icon, control centre TUI, MCP (`torroworkplace`) |
| `crates/torro-ipc` | How apps talk to the core (local socket) |
| `crates/torro-pim` | Event/Todo model, provider traits, offline cache, sync |
| `crates/torro-pim-caldav` | CalDAV/iCal provider (Nextcloud, iCloud, …) |
| `crates/torro-tui` | Shared ratatui widgets and theme |
| `crates/torrotasks` | Tasks app (`torrotasks`) |
| `crates/torrocalendar` | Calendar app (`torrocalendar`) |
| `apps/workspace-setups` | Save and restore window sets (Omarchy/Hyprland; AeroSpace planned) |
| `integrations/` | Window rules and launchers for Hyprland, AeroSpace, Omarchy widgets |
| `packaging/` | AUR, Homebrew, later Scoop |
| `docs/` | Vision, research, architecture, app catalog, roadmap |

Every app is a client of the core and offers the same outside interface: a TUI first (the
base on every system), a GUI later, and `status --json` and `ctl` for bars and scripts. AI
agents reach all of it through `torroworkplace mcp`.

## Build it

```
cargo check --workspace
```

## License

MIT OR Apache-2.0, at your option.
