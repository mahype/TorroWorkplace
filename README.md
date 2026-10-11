# TorroWorkplace

A suite of small, separate desktop apps around one core — calendar, tasks, and more to
come — built to be tiled side by side by a tiling window manager instead of living inside
one big program.
The core, `torroworkplace`, is a background service with a tray icon. It comes first: it saves
the active workspace of the tiling window manager as a template and rebuilds it on demand —
Hyprland on Linux, AeroSpace on macOS, GlazeWM on Windows. Later it starts the apps, holds
accounts and sync for all of them and offers one MCP server for AI agents. Each app is its own
process and its own window with a stable app id, so the window manager can place it.

**Status: planning / scaffold.** Nothing usable yet. See [docs/core.md](docs/core.md) and
[docs/roadmap.md](docs/roadmap.md).

## What's in here

| Path | What |
|---|---|
| `crates/torro-core` | Shared config, paths, theme tokens |
| `crates/torroworkplace` | The core: service, tray icon, control centre, MCP (`torroworkplace`) |
| `crates/torro-ipc` | How apps talk to the core (local socket) |
| `crates/torro-wm` | Window manager backends: Hyprland, AeroSpace, GlazeWM (planned) |
| `crates/torro-setups` | Workspace templates: capture, save, restore (planned) |
| `crates/torro-pim` | Event/Todo model, provider traits, offline cache, sync |
| `crates/torro-pim-caldav` | CalDAV/iCal provider (Nextcloud, iCloud, …) |
| `crates/torro-tui` | Shared ratatui widgets and theme |
| `crates/torrotasks` | Tasks app (`torrotasks`) |
| `crates/torrocalendar` | Calendar app (`torrocalendar`) |
| `apps/workspace-setups` | Reference: the earlier Omarchy plugin for workspace templates, being rebuilt in Rust |
| `integrations/` | Window rules and launchers for Hyprland, AeroSpace, Omarchy widgets |
| `packaging/` | AUR, Homebrew, later Scoop |
| `docs/` | Vision, research, architecture, app catalog, roadmap |

Every app is a client of the core and offers the same outside interface: a GUI (Iced), and
`status --json` and `ctl` for bars and scripts. AI
agents reach all of it through `torroworkplace mcp`.

## Build it

```
cargo check --workspace
```

## License

MIT OR Apache-2.0, at your option.
