# Architektur

## Überblick

Ein Monorepo, ein Rust-Workspace. Alles gehört hier hinein: die Apps, die gemeinsamen
Crates, workspace-setups, Integrationen für die Window Manager und die Pakete.

```
TorroWorkplace/
├── Cargo.toml                  # Workspace
├── crates/
│   ├── torro-core/             # App-IDs, Pfade pro OS, Theme-Tokens; später Config, Logging, Keychain
│   ├── torro-pim/              # Modell Event/Todo, Provider-Trait, Offline-Cache, Sync-Engine
│   ├── torro-pim-caldav/       # Provider: CalDAV + iCalendar (VEVENT/VTODO)
│   ├── torro-tui/              # gemeinsame ratatui-Widgets, Keymap, Theme
│   ├── torrotasks/             # App Aufgaben  → bin `torrotasks`
│   └── torrocalendar/          # App Kalender  → bin `torrocalendar`
├── gui/                        # Phase 3: Iced-Frontends, ein Crate pro App, gleicher Kern
├── apps/
│   └── workspace-setups/       # Fenster-Sets speichern und laden (Python + QML), Historie übernommen
├── integrations/
│   ├── hyprland/               # Fensterregeln, .desktop-Launcher
│   ├── aerospace/              # on-window-detected-Regeln
│   └── omarchy/                # Leistenwidgets (Muster: omarchy-torromail)
├── packaging/                  # AUR, Homebrew, später Scoop
├── install.sh                  # Ein Befehl für alles
└── docs/
```

## Schichten

```
   TUI (ratatui)      GUI (Iced)      status/ctl (JSON)      mcp (Agenten)
          \               |                 |                   /
           +--------------+--------+--------+------------------+
                                   |
                         torro-pim (Cache + Sync)
                                   |
                          Provider-Trait
                     /             |              \
           torro-pim-caldav   (Google, später)  (Todoist, später)
```

- **Die Oberflächen kennen nur `torro-pim`.** Sie lesen und schreiben den lokalen Cache (SQLite).
- **Die Sync-Engine** gleicht den Cache mit den Providern ab: `sync-collection` mit Sync-Token,
  Schreiben nur mit passendem ETag, Konflikte werden sichtbar gemacht statt still überschrieben.
- **Provider** implementieren `torro_pim::Provider` (`crates/torro-pim/src/provider.rs`). Ein neuer
  Anbieter ist ein neues Crate. An den Apps ändert sich dafür nichts.
- **Das Modell** ist eng an iCalendar (RFC 5545) angelehnt, damit CalDAV eins zu eins passt
  (`crates/torro-pim/src/model.rs`).
- **Mehrere Prozesse:** Kalender- und Aufgaben-App können sich ein CalDAV-Konto teilen. Jede App
  hat ihren eigenen Cache. Ob später ein gemeinsamer Sync-Daemon sinnvoll ist, entscheiden wir mit
  dem MVP.

## Gemeinsame Schnittstelle jeder App

| Befehl | Zweck |
|---|---|
| `<app>` / `<app> tui` | Terminal-Oberfläche |
| `<app> status --json` | Kurzstatus für Leisten-Widgets (Omarchy, SketchyBar) und Skripte |
| `<app> ctl …` | Steuerung von außen (z. B. „Aufgabe anlegen“, „zu heute springen“) |
| `<app> mcp` | MCP-Server über stdio für Claude Code, Codex & Co. |

Exit-Codes, JSON-Fehler auf stderr und Pfade folgen denselben Konventionen wie bei TorroMail,
TorroCast und workspace-setups.

## App-ID und Fenster

Jede App hat eine feste ID (`crates/torro-core/src/app.rs`):

| App | Fensterklasse / App-ID | Binary |
|---|---|---|
| Aufgaben | `torro-tasks` | `torrotasks` |
| Kalender | `torro-calendar` | `torrocalendar` |

- **TUI:** Ein Launcher startet das Terminal mit dieser Klasse, z. B. `ghostty --class=torro-tasks -e torrotasks`
  oder `kitty --class torro-tasks torrotasks` (unter Omarchy: `omarchy-launch-or-focus-tui`).
- **GUI:** Iced setzt die Wayland-App-ID bzw. die Bundle-ID (`de.torro.tasks`) direkt.
- **Hyprland:** `windowrule`s auf `class:^torro-tasks$`.
- **AeroSpace:** `[[on-window-detected]]` auf die App-ID bzw. den Fenstertitel. Bei Terminal-TUIs
  auf dem Mac unterscheidet sich die Bundle-ID nicht vom Terminal, deshalb setzt der Launcher einen
  festen Fenstertitel.
- **workspace-setups** speichert die Klasse und den Startbefehl und findet das Fenster beim Laden wieder.

## Pfade

`crates/torro-core/src/paths.rs`:

| OS | config | data | state |
|---|---|---|---|
| Linux | `$XDG_CONFIG_HOME/<app>` | `$XDG_DATA_HOME/<app>` | `$XDG_STATE_HOME/<app>` |
| macOS | `~/Library/Application Support/<app>` | dto. | dto. |
| Windows | `%APPDATA%\<app>` | `%APPDATA%\<app>` | `%LOCALAPPDATA%\<app>` |

`TORRO_{CONFIG,DATA,STATE}_HOME` überschreiben die Wurzeln (für Tests und portable Installationen).

## Zugangsdaten

Passwörter und Tokens liegen im Schlüsselbund des Systems: macOS Keychain, unter Linux der
Secret Service, unter Windows der Credential Manager. Vorlage für macOS ist
`TorroMail/crates/torromail-keychain`.

## Design

Die Farben kommen aus `torro-design/tokens/tokens.json` und liegen in `crates/torro-core/src/theme.rs`.
Torro-Rot ist der Akzent, Status nutzt eigene Farben (grün, orange, rot, grau).
Die Regeln stehen in `torro-design/app-design.md`.

## workspace-setups im Monorepo

Heute: Python (nur Standardbibliothek) und QML, nur Omarchy/Hyprland. Übernommen mit Historie
(`git subtree`) nach `apps/workspace-setups`.

Für AeroSpace müssen vier Stellen austauschbar werden:

| Bereich | Hyprland (heute) | AeroSpace (neu) |
|---|---|---|
| Fenster abfragen und steuern | `hyprctl -j clients`, `dispatch` | `aerospace list-windows --json`, `move-node-to-workspace`, `focus` |
| Layout | dwindle-Baum mit Teilungsverhältnissen | i3-ähnlicher Baum aus Kacheln und Akkordeon (`layout`, `join-with`, `resize`); Teilungsverhältnisse nur näherungsweise |
| Startbefehl ermitteln | `/proc/<pid>/cmdline`, `.desktop` | Bundle-ID + `open -b`/`open -a`, `ps` für Terminal-Apps |
| Browser-Tabs | `~/.config/chromium/…/Sessions` | `~/Library/Application Support/Google/Chrome/…/Sessions` (gleiches SNSS-Format) |
| Oberfläche | Quickshell-Panel, Lua-Tastenkürzel | CLI, Tastenkürzel in `~/.aerospace.toml`, später GUI oder SketchyBar |

Vorgehen:
1. Übernehmen ohne Funktionsänderung (erledigt). Unter Omarchy richtet `install.sh` das Plugin aus
   dem Monorepo ein. Ob `omarchy plugin add` ein Unterverzeichnis kann, ist offen.
2. Ein Interface `WindowManagerBackend` einziehen: `hyprland.py` ist die erste Umsetzung, `aerospace.py`
   die zweite. Das Template-Schema 3 bekommt ein Feld `wm` und speichert das Layout neutral.
3. Später den Kern nach Rust verlegen (`crates/torro-setups`), mit gemeinsamen Pfaden und
   Windows-Backend (komorebi).
4. `mahype/workspace-setups` archivieren und auf das Monorepo verweisen.
