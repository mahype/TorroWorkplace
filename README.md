# TorroWorkplace

An actively developed suite of small desktop apps around a shared Rust core — including
calendar, tasks, inbox/mail, assistant and workspace management. Apps can be arranged side by
side by a tiling window manager instead of living inside one large tabbed program.
The `torroworkplace` core provides the shared service, tray, workspace templates, settings,
IPC and agent-facing MCP integration. The codebase also contains dedicated apps and shared
libraries for calendars, tasks, inbox/mail, notifications and AI assistance. The intended
window-manager integrations are Hyprland on Linux, AeroSpace on macOS and GlazeWM on Windows.
See the architecture documents for platform-specific implementation details.

**Status: active development (November 2026).** The repository contains substantial
implementations, and recent merged pull requests cover calendar, tasks, inbox/mail, assistant
and control-centre functionality. This is not a declaration of production readiness or
feature completeness. For current scope and open work, see [roadmap](docs/roadmap.md),
[core architecture](docs/core.md), [vision](docs/vision.md) and the [issue workflow](docs/issue-workflow.md).

## Downloads and installation

Choose an app and your platform in [GitHub Releases](https://github.com/mahype/TorroWorkplace/releases/latest)
(`downloads.md` / **App downloads**). Native downloads are prepared for macOS (DMG), Windows x64
(all-users Setup) and Linux (DEB, RPM, direct pacman packages for Arch/Omarchy; AUR remains).
See the [installation guide](docs/install.md) for companion packages, checksums, updates and removal.
The first maintainer release and platform verification are still pending.

## What's in here

| Path | What |
|---|---|
| `crates/torro-core` | Shared config, paths, theme tokens |
| `crates/torroworkplace` | The core: service, tray icon, control centre, MCP (`torroworkplace`) |
| `crates/torro-ipc` | How apps talk to the core (local socket) |
| `crates/torro-wm` | Window-manager abstraction and platform backends (support varies by platform) |
| `crates/torro-setups` | Workspace template capture, storage and restoration |
| `crates/torro-pim` | Event/Todo model, provider traits, offline cache, sync |
| `crates/torro-pim-caldav` | CalDAV/iCal provider (Nextcloud, iCloud, …) |
| `crates/torro-tui` | Shared ratatui widgets and theme |
| `crates/torro-settings-ui` | Shared settings UI components |
| `crates/torroinbox`, `crates/torro-inbox-*` | Inbox, mail and notification views |
| `crates/torro-mail*` | Mail model, IMAP and backup components |
| `crates/torroassistant` | AI assistant app |
| `crates/torrochat` | Nextcloud Talk chat app |
| `crates/torro-contacts` | Contacts support |
| `crates/torrotasks` | Tasks app (`torrotasks`) |
| `crates/torrocalendar` | Calendar app (`torrocalendar`) |
| `apps/workspace-setups` | Reference: the earlier Omarchy plugin for workspace templates, being rebuilt in Rust |
| `integrations/` | Window rules and launchers for Hyprland, AeroSpace, Omarchy widgets |
| `packaging/` | Per-app DMG/Setup/DEB/RPM/pacman builders, AUR, Homebrew and Scoop templates |
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
