# Roadmap

## Phase 0: Planung und Gerüst ✅

- Repository, Lizenz (MIT OR Apache-2.0), Dokumente unter `docs/`
- workspace-setups mit Historie nach `apps/workspace-setups` übernommen
- Rust-Workspace-Gerüst: `torro-core`, `torro-pim`, `torro-pim-caldav`, `torro-tui`, `torrotasks`, `torrocalendar`

## Phase 1: MVP als TUI (Aufgaben und Kalender)

1. **`torro-pim-caldav`:** Erkennung (`current-user-principal`, `calendar-home-set`), Kalenderliste,
   `sync-collection`, iCalendar lesen und schreiben (VEVENT, VTODO), Schreiben nur mit passendem ETag.
   Getestet gegen Radicale (lokal, Container), Nextcloud und iCloud.
2. **`torro-pim`:** SQLite-Cache, Sync-Engine, Umgang mit Konflikten.
3. **Konten einrichten:** Assistent in der TUI, Passwort im Schlüsselbund.
4. **`torrotasks`-TUI:** Listen, Fälligkeit, Priorität, Unteraufgaben, schnelles Erfassen, erledigte ausblenden.
5. **`torrocalendar`-TUI:** Tag, Woche, Agenda; Termin anlegen und bearbeiten; „nächster Termin“.
6. **`status --json`, `ctl`, `mcp`** für beide Apps.
7. **Integrationen:** Hyprland-Regeln, `.desktop`-Launcher, AeroSpace-Regeln.
8. **Installation:** `install.sh`, AUR (`-bin` + Metapaket `torroworkplace`), Homebrew-Tap, Release-CI.

Fertig ist Phase 1, wenn beide Apps unter Hyprland und AeroSpace nebeneinander gekachelt laufen,
mit der eigenen Nextcloud synchronisieren und workspace-setups sie wiederherstellt.

## Phase 2: Native GUI (Iced)

- `gui/torrotasks-gui`, `gui/torrocalendar-gui` auf demselben Kern
- gemeinsames Iced-Theme aus den Torro-Tokens
- Paketierung als `.app` (macOS, signiert) und mit `.desktop` und Icon (Linux)

## Phase 3: workspace-setups für AeroSpace

- `WindowManagerBackend` einziehen, Backend für AeroSpace, Template-Schema 3 mit Feld `wm`
- Browser-Tabs auf dem Mac (Chrome/Brave unter `~/Library/Application Support`)
- Bedienung auf dem Mac: CLI, Tastenkürzel in `~/.aerospace.toml`, später SketchyBar oder GUI

## Phase 4: Ausbau

- Agenten-Übersicht und GitHub-Dashboard (siehe [app-catalog.md](app-catalog.md))
- Omarchy-Leistenwidgets pro App
- weitere Provider: Google Calendar, Todoist
- Windows: komorebi/GlazeWM-Regeln, Scoop-Bucket, workspace-setups-Backend
- TorroMail, TorroCast und TorroEQ in den Suite-Installer aufnehmen

## Offene Fragen

- Teilen sich Kalender und Aufgaben einen Sync-Daemon oder hat jede App einen eigenen Cache?
- Kann `omarchy plugin add` ein Plugin aus einem Unterverzeichnis installieren?
- Bundle-IDs und Signierung der macOS-GUIs (Team-ID wie bei TorroMail?)
- workspace-setups: wann in Rust neu schreiben, vor oder nach dem AeroSpace-Backend?
