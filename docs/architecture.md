# Architektur

## Überblick

Ein Monorepo, ein Rust-Workspace. Alles gehört hier hinein: der Kern, die Apps, die
gemeinsamen Crates, workspace-setups, Integrationen für die Window Manager und die Pakete.

**Der Kern ist `torroworkplace`:** ein Hintergrunddienst mit Icon in der Taskleiste, von dem
aus sich die ganze Suite steuern lässt. Zuerst hält er die Workspaces des Tiling-Managers als
Vorlagen fest (Hyprland, AeroSpace, GlazeWM); das ausführliche Konzept steht in [core.md](core.md).
Die Apps (Aufgaben, Kalender, …) bauen auf ihm auf.

```
TorroWorkplace/
├── Cargo.toml                  # Workspace
├── crates/
│   ├── torro-core/             # App-IDs, Pfade pro OS, Theme-Tokens; später Config, Logging, Keychain
│   ├── torro-ipc/              # Protokoll zwischen Kern und Apps (lokaler Socket)
│   ├── torro-wm/               # Trait WindowManager; Backends Hyprland, AeroSpace, GlazeWM
│   ├── torro-setups/           # Workspace-Vorlagen: erfassen, speichern, laden
│   ├── torro-pim/              # Modell Event/Todo, Provider-Trait, Offline-Cache, Sync-Engine
│   ├── torro-pim-caldav/       # Provider: CalDAV + iCalendar (VEVENT/VTODO)
│   ├── torro-tui/              # gemeinsame ratatui-Widgets, Keymap, Theme
│   ├── torroworkplace/         # DER KERN → bin `torroworkplace` (daemon, tray, tui, status, ctl, mcp)
│   ├── torrotasks/             # App Aufgaben  → bin `torrotasks`
│   └── torrocalendar/          # App Kalender  → bin `torrocalendar`
├── gui/                        # Phase 3: GUIs (Technik nach Vergleich), gleicher Kern
├── apps/
│   └── workspace-setups/       # Referenz: das bisherige Omarchy-Plugin (Python + QML), wird in Rust neu gebaut
├── integrations/
│   ├── hyprland/               # Fensterregeln, .desktop-Launcher, Autostart
│   ├── aerospace/              # on-window-detected-Regeln, LaunchAgent
│   └── omarchy/                # Leistenwidgets (Muster: omarchy-torromail)
├── packaging/                  # AUR, Homebrew, später Scoop
├── install.sh                  # Ein Befehl für alles
└── docs/
```

## Schichten

```
  Tray-Icon (Menü)   Steuerzentrale (TUI)   torrotasks   torrocalendar   … weitere Apps
         \                  |                    |              |              /
          +-----------------+------ torro-ipc (lokaler Socket) -+-------------+
                                          |
                          torroworkplace daemon  ──  mcp (eine Suite für alle Agenten)
                 /            |                 |                 \
          Konten +      torro-pim          App-Verwaltung       torro-setups
          Schlüsselbund (Cache + Sync)     (starten/fokussieren) (Vorlagen) → torro-wm
                              |
                       Provider-Trait
                /             |              \
     torro-pim-caldav   (Google, später)  (Todoist, später)
```

### Der Kern: `torroworkplace`

| Befehl | Zweck |
|---|---|
| `torroworkplace daemon` | Hintergrunddienst mit Tray-Icon; `--headless` ohne Icon. Startet automatisch beim Anmelden |
| `torroworkplace ui` | Steuerzentrale (Iced): Vorlagen, später Apps, Konten, Sync-Status |
| `torroworkplace status --json` | Kurzstatus für Leisten-Widgets und Skripte |
| `torroworkplace ctl …` | Steuerung von außen (App starten, Sync anstoßen, Set laden) |
| `torroworkplace mcp` | **ein** MCP-Server (stdio) für die ganze Suite; leitet an den Dienst weiter |

Der Dienst ist für alles zuständig, was mehrere Apps betrifft:
- **Konten und Zugangsdaten:** werden einmal eingerichtet und gelten für alle Apps. Passwörter
  liegen im Schlüsselbund des Systems.
- **Synchronisation:** ein Cache, eine Sync-Engine für alle Apps. Kalender und Aufgaben teilen
  sich dasselbe CalDAV-Konto, ohne doppelt zu synchronisieren.
- **App-Verwaltung:** welche Suite-Apps installiert sind und laufen. Er startet eine App oder
  holt ihr Fenster nach vorn, unter Hyprland mit `hyprctl dispatch focuswindow class:…`, unter
  AeroSpace mit `aerospace focus --window-id …` bzw. `open`.
- **Workspace-Vorlagen:** Er speichert den aktiven Workspace als Vorlage und baut sie wieder auf,
  über ein Backend pro Tiling-Manager (`torro-wm`). Siehe [core.md](core.md).
- **API und MCP:** Jede Funktion gibt es in Oberfläche, API und MCP; der MCP-Server baut auf der
  API auf, Schlüssel und Rechte prüft nur der Dienst. Die Tools tragen den Bereich als Präfix:
  `calendar_*`, `tasks_*`, `workspaces_*`, `apps_*` … Konzept: [api.md](api.md).

### Tray-Icon

Das Menü enthält zuerst die Workspace-Vorlagen (laden, speichern, aktualisieren, löschen), später Apps starten/fokussieren, Sync-Status und die Steuerzentrale
öffnen, Beenden.

| Plattform | Umsetzung |
|---|---|
| Linux (Omarchy 4, Waybar, KDE …) | StatusNotifierItem über `ksni`, ohne GTK. Omarchy 4 zeigt Tray-Icons in seiner Quickshell-Leiste |
| macOS | Menüleisten-Symbol über `tray-icon` |
| Windows | Infobereich über `tray-icon` |

Zusätzlich kann unter Omarchy später ein eigenes Leistenwidget auf `status --json` aufsetzen.

### Apps als Clients

- Die Apps sprechen über `torro-ipc` mit dem Kern (Unix-Socket `core.sock` im State-Verzeichnis
  des Kerns, unter Windows eine Named Pipe). Läuft kein Kern, startet die App ihn.
- Sie lesen Daten, schreiben Änderungen und bekommen Änderungsmeldungen, damit z. B. eine im
  Kalender angelegte Aufgabe sofort in der Aufgaben-App erscheint.
- Jede App hat zusätzlich `status --json` und `ctl`.
- **Offline zuerst:** Der Cache liegt beim Kern (SQLite), die Sync-Engine arbeitet im Hintergrund.
  Die Oberflächen warten nie auf das Netz.

### Tiling- und Fenster-Modus

Neben dem Tiling-Modus (ein Fenster pro App) gibt es den Fenster-Modus: eine Arbeitsfläche, die die
Apps als Elemente einbettet. Dafür wird jede App eine Bibliothek (Zustand, `update`, `view`) plus ein
dünnes Programm, und `torro-gui` bekommt ein gemeinsames Element-Interface. Konzept und Schritte:
[modes.md](modes.md).

### Provider

Provider implementieren `torro_pim::Provider` (`crates/torro-pim/src/provider.rs`). Ein neuer
Anbieter ist ein neues Crate, das der Kern lädt. An den Apps ändert sich dafür nichts. Das
Modell lehnt sich an iCalendar (RFC 5545) an (`crates/torro-pim/src/model.rs`).

## App-ID und Fenster

Jede App hat eine feste ID (`crates/torro-core/src/app.rs`):

| App | Fensterklasse / App-ID | Binary |
|---|---|---|
| Kern / Steuerzentrale | `torro-workplace` | `torroworkplace` |
| Aufgaben | `torro-tasks` | `torrotasks` |
| Kalender | `torro-calendar` | `torrocalendar` |

- **TUI:** Ein Launcher startet das Terminal mit dieser Klasse, z. B. `ghostty --class=torro-tasks -e torrotasks`
  oder `kitty --class torro-tasks torrotasks` (unter Omarchy: `omarchy-launch-or-focus-tui`).
- **GUI:** setzt die Wayland-App-ID bzw. eine Bundle-ID direkt.
- **Hyprland:** `windowrule`s auf `class:^torro-tasks$`.
- **AeroSpace:** `[[on-window-detected]]` auf die App-ID bzw. den Fenstertitel. Bei Terminal-TUIs
  auf dem Mac unterscheidet sich die Bundle-ID nicht vom Terminal, deshalb setzt der Launcher einen
  festen Fenstertitel.
- **workspace-setups** speichert die Klasse und den Startbefehl und findet das Fenster beim Laden wieder.

## Autostart des Kerns

| Plattform | Weg |
|---|---|
| Hyprland/Omarchy | `exec-once = torroworkplace daemon` oder ein systemd-User-Service |
| macOS | LaunchAgent in `~/Library/LaunchAgents` |
| Windows | Autostart-Eintrag |

## Pfade

`crates/torro-core/src/paths.rs`:

| OS | config | data | state |
|---|---|---|---|
| Linux | `$XDG_CONFIG_HOME/<app>` | `$XDG_DATA_HOME/<app>` | `$XDG_STATE_HOME/<app>` |
| macOS | `~/Library/Application Support/<app>` | dto. | dto. |
| Windows | `%APPDATA%\<app>` | `%APPDATA%\<app>` | `%LOCALAPPDATA%\<app>` |

`TORRO_{CONFIG,DATA,STATE}_HOME` überschreiben die Wurzeln (für Tests und portable Installationen).

## Design

Die Farben kommen aus `torro-design/tokens/tokens.json` und liegen in `crates/torro-core/src/theme.rs`.
Torro-Rot ist der Akzent, Status nutzt eigene Farben (grün, orange, rot, grau).
Die Regeln stehen in `torro-design/app-design.md`.

## GUI

Die GUI baut auf Iced (entschieden). Die Steuerzentrale läuft als eigener Prozess
(`torroworkplace ui`) und spricht über IPC mit dem Dienst, damit der Dienst leicht bleibt und auf
dem Mac Tray-Icon und GUI nicht um den Haupt-Thread streiten. Die Kalender-Ideen in
[calendar-views.md](calendar-views.md) (Tagesspirale) sind ein Prüfstein für Zeichnen und Zoom.

## workspace-setups

`apps/workspace-setups` ist das bisherige Omarchy-Plugin (Python und QML, nur Hyprland), mit
Historie übernommen. Es dient als **Referenz** für Funktionen und Erfahrungen, etwa das Erfassen
des Kachelbaums aus Fensterrechtecken, die Strategien je Fenster, Browser-Tabs und Terminalprogramme.
Der Kern baut diese Funktionen in Rust neu (`torro-wm`, `torro-setups`), mit einem Backend pro
Tiling-Manager. Plan und Funktionsliste: [core.md](core.md).
