# Recherche: ähnliche Projekte, Techniken, Installation

Stand: 2026-10-05, Tiling-Manager ergänzt am 2026-10-11. Zahlen zum Speicherbedarf sind grobe Richtwerte aus gemischten Quellen.

## Fazit vorweg

- Eine **plattformübergreifende Suite kleiner, kachelbarer Einzel-Apps** gibt es so nicht.
  Am nächsten kommt **Omarchy**, das aber Linux-only ist und Fremd-Apps zusammenstellt.
- Das **Omarchy-Muster** ist die Blaupause: App = feste Fensterklasse, TUIs per `.desktop`-Launcher
  als eigene Apps, Platzierung über Compositor-Regeln, Installation per `curl | bash`.
- **Lücke:** Werkzeuge für den Überblick über KI-Agenten sind fast nur macOS-Menüleisten-Apps
  oder tmux-TUIs. Ein kachelbares Fenster „Agenten“ und eines für „Verbrauch“ gibt es
  plattformübergreifend nicht.
- **Technik:** Rust-Kern + ratatui-TUI (sehr leicht) als Grundlage für alle Systeme. Die GUI kommt danach; Favorit ist **Iced**, entschieden wird nach einem Vergleich (Phase 3).
- **Tiling-Manager:** Hyprland (Linux), AeroSpace (macOS), GlazeWM (Windows); siehe Abschnitt 6.
- **Tray-Icon:** Omarchy 4 hat eine Tray-Leiste (Quickshell, StatusNotifierItem). Linux: `ksni` (ohne GTK), macOS/Windows: `tray-icon`.

## 1. Suiten und „Desktop als Dashboard“

| Projekt | Link | Was wir lernen |
|---|---|---|
| Omarchy (DHH / Omacom) | https://omarchy.org/manual/tuis/, https://github.com/omacom/omarchy | `omarchy-tui-install` macht aus einem Terminalbefehl einen `.desktop`-Launcher mit eigener Klasse (float/tile); `omarchy-webapp-install` für Web-Apps; Installation in einem Rutsch. Härtung bei `.desktop`-Feldern: https://github.com/omacom/omarchy/pull/9150 |
| awesome-omarchy | https://github.com/aorumbayev/awesome-omarchy | Community-TUIs (hyprmon, hyprBluetooth) |
| Hypr-Ökosystem | https://wiki.hypr.land/Hypr-Ecosystem/ | Viele kleine Einzweck-Tools; Walker läuft als Dienst für schnellen Start |
| COSMIC (System76) | https://github.com/pop-os/libcosmic | Suite kleiner Apps auf einem gemeinsamen Toolkit (Iced) zeigt, wie eine Designsprache über getrennte Apps hinweg einheitlich bleibt |
| GNOME Circle / libadwaita | https://apps.gnome.org/ | Kleine Einzweck-Apps mit gemeinsamer HIG, aber Linux-only |

## 2. Kalender, Aufgaben, GitHub

| App | Link | Technik | Erkenntnis |
|---|---|---|---|
| calcure | https://pypi.org/project/calcure/ | Python-TUI | Kalender und Aufgaben zusammen, `.ics`-Import |
| khal + vdirsyncer | https://github.com/pimutils/khal | Python | Muster „Sync-Kern + dünne Oberfläche“ auf CalDAV |
| taskwarrior-tui | https://terminaltrove.com/taskwarrior-tui/ | Rust/ratatui | TUI über einer bestehenden Aufgaben-Engine |
| Planify | https://github.com/alainm23/planify | Vala/GTK4 | Todoist, Nextcloud, CalDAV; Erinnerungen werden in beide Richtungen synchronisiert |
| Errands | https://github.com/mrvladus/Errands | GTK4 | Einfache CalDAV-Aufgaben (vor allem Nextcloud) |
| gh-dash | https://www.gh-dash.dev/ | Go/Bubble Tea | Beste Vorlage für ein GitHub-Dashboard: Abschnitte per YAML, Tasten starten lazygit oder Claude-Reviews |
| Gitify | https://gitify.io/ | Electron | Benachrichtigungen von mehreren Forges, bewusst ohne Triage |

## 3. Überblick über KI-Agenten

| Tool | Link | Notiz |
|---|---|---|
| ccusage | https://github.com/ryoppippi/ccusage | Liest `~/.claude/projects/*.jsonl` (auch Codex, Gemini …), ohne API-Key. Die Datenquelle für eine „Verbrauch“-App |
| agent-deck | https://github.com/asheshgoplani/agent-deck | Rust/ratatui, auf tmux |
| bosun | https://github.com/yetidevworks/bosun | tmux-Dashboard |
| Recon | https://agent-wars.com/news/2026-03-14-recon-tmux-tui-claude-code-sessions | Tabelle mit Status, Modell, Kontext und Branch, gute Vorlage |
| Claude Squad | https://github.com/smtg-ai/claude-squad | tmux + Worktrees; Wartungsstatus prüfen |
| Vibe Kanban | https://github.com/BloopAI/vibe-kanban | Agenten-Tasks als Kanban, inzwischen Community-gepflegt |
| opcode (ex Claudia) | https://github.com/winfunc/opcode | Tauri-GUI, seit 10/2025 still |
| Emdash | https://github.com/generalaction/emdash | Electron, viele CLI-Provider, eigener Port pro Task |
| ClaudeBar u. a. | https://github.com/tddworks/ClaudeBar | Nur macOS-Menüleiste |
| Übersichten | https://nimbalyst.com/blog/best-claude-code-gui-tools-2026/, https://www.augmentcode.com/tools/open-source-agent-orchestrators | Marktüberblick 2026 |

## 4. Techniken für die Oberfläche

Wichtig ist der Speicherbedarf **pro App im Leerlauf**, weil 6 bis 10 Apps gleichzeitig laufen.
Benchmark-Sammlung: https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state

| Option | ca. RAM | Wayland/Hyprland | Bewertung |
|---|---|---|---|
| **ratatui (TUI)** | 5–20 MB + Terminal | perfekt (Terminal-Fenster) | **Grundlage.** Überall gleich, kachelt ideal, passt zu TorroMail und TorroCast |
| **Iced** | 40–100 MB | sehr gut (COSMIC) | **Favorit für die GUI (Phase 3).** Elm-Architektur, macOS und Windows ok |
| Slint | sehr gering | gut (winit) | Leichteste Alternative, eigene Markup-Sprache, Lizenzmodell beachten |
| egui | 30–60 MB | gut | Schnell gebaut, wirkt weniger nativ |
| GPUI (Zed) | gering | gut | Release-Stand unsicher, als Abhängigkeit riskant |
| Tauri 2 | 30–60 MB + WebView | **Probleme**: WebKitGTK zeigt leere Fenster bei explicit sync/NVIDIA (https://v2.tauri.app/develop/debug/linux-graphics/) | Verworfen |
| Wails v3 | ~10 MB + WebView | gleiche WebKitGTK-Probleme | Verworfen |
| Flutter | 80–150 MB | ok | Zu schwer für 10 Apps |
| Electron | 100–300 MB | ok | Verworfen |
| Quickshell / AGS / eww | — | nur Hyprland | Für Leisten und Overlays, nicht für normale Fenster; passt für Omarchy-Widgets |

## 5. Tray-Icon für den Kern

| Weg | Plattform | Notiz |
|---|---|---|
| Omarchy 4-Leiste | Linux (Omarchy) | Die Leiste läuft auf Quickshell und hat einen System-Tray (StatusNotifierItem): https://quickshell.org/docs/master/types/Quickshell.Services.SystemTray, Release-Notes: https://github.com/basecamp/omarchy/releases/tag/v4.0.0. Laut Quickshell ist die SNI-Umsetzung „roughly conforming“, also unter Omarchy testen |
| `ksni` | Linux | Reines StatusNotifierItem über D-Bus, ohne GTK: https://docs.rs/crate/ksni/latest/ |
| `tray-icon` | macOS, Windows (Linux nur mit GTK + libappindicator) | Verbreitetster Weg; braucht eine Ereignisschleife auf dem Thread des Icons: https://docs.rs/crate/tray-icon/ |
| `ldtray` | alle | Lädt Toolkits zur Laufzeit; jung (0.1.x): https://docs.rs/crate/ldtray/ |

Plan: eine kleine Abstraktion im Kern, `ksni` unter Linux und `tray-icon` auf macOS und Windows.

## 6. Tiling-Manager pro System

Recherche vom 2026-10-11. Entschieden: Hyprland, AeroSpace, GlazeWM.

| Tiling-Manager | System | Stand | Schnittstelle | Bewertung |
|---|---|---|---|---|
| **Hyprland** | Linux | 0.56 (hier unter Omarchy 4.0.4) | `hyprctl -j`, Befehls- und Ereignis-Socket | Gesetzt; workspace-setups zeigt, dass Erfassen und Aufbauen mit Teilungsverhältnissen funktionieren |
| **AeroSpace** | macOS | v0.21.3-Beta (Juli 2026), noch vor 1.0: https://github.com/nikitabobko/AeroSpace/releases | CLI `aerospace list-windows --json`, `list-workspaces`, `move-node-to-workspace`, `join-with`, `resize` | **Gewählt.** Kein Abschalten von SIP, eigene virtuelle Workspaces, i3-artiger Baum. Keine Ereignisschnittstelle (nur Callbacks in der Konfiguration), Teilungsverhältnisse nur näherungsweise |
| yabai | macOS | — | `yabai -m query` (JSON), BSP mit Verhältnissen | Für Spaces muss SIP teilweise aus; verworfen |
| Amethyst | macOS | — | kaum steuerbar | Verworfen |
| **GlazeWM** | Windows | v3.10.1 (März 2026), GPL-3, Rust: https://github.com/glzr-io/glazewm | WebSocket `ws://127.0.0.1:6123` mit `query` (windows, workspaces, monitors), `command` und `sub` (Ereignisse); Crate `wm-ipc-client` im Repo | **Gewählt.** Freie Lizenz, i3-artig wie AeroSpace |
| komorebi | Windows | v0.1.41 (Mai 2026), Rust: https://github.com/LGUG2Z/komorebi | `komorebic`, Socket, `state` als JSON, Ereignis-Abo, „apply state“ | Technisch stark, aber Komorebi-Lizenz (Fork von PolyForm Strict): nur private Nutzung, beruflich kostenpflichtig. Höchstens später als zweites Backend |

Folge: AeroSpace und GlazeWM arbeiten mit i3-artigen Bäumen, Hyprland mit dwindle. Vorlagen speichern
das Layout deshalb neutral als Binärbaum aus Teilungen (Richtung und Verhältnis), erfasst aus den
Fensterrechtecken; jedes Backend baut ihn so genau auf, wie es geht.

## 7. Alles auf einmal installieren

- **Homebrew:** Tap `mahype/homebrew-torro` mit einer Formel pro App plus einer Meta-Formel
  oder einem Brewfile (`brew bundle`), siehe https://docs.brew.sh/Brew-Bundle-and-Brewfile.
- **AUR:** `torro<app>-bin` pro App und ein Metapaket `torroworkplace` mit `depends=()`.
  Vorlagen: `TorroMail/packaging/aur/torromail-bin/PKGBUILD` und die CI in `TorroMail/.github/workflows/`.
- **dist (cargo-dist):** baut aus dem Workspace Binärpakete, Homebrew-Formeln und Shell-/PowerShell-Installer.
- **Windows (später):** Scoop-Bucket; für winget eine `winget import`-Liste, weil es dort keine Metapakete gibt.
- **`install.sh`:** erkennt das OS und ruft brew, pacman/AUR oder einen Binär-Download auf.
  Auf Omarchy richtet es zusätzlich das workspace-setups-Plugin ein.
