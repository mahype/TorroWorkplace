# Recherche: ähnliche Projekte, Techniken, Installation

Stand: 2026-10-05. Zahlen zum Speicherbedarf sind grobe Richtwerte aus gemischten Quellen.

## Fazit vorweg

- Eine **plattformübergreifende Suite kleiner, kachelbarer Einzel-Apps** gibt es so nicht.
  Am nächsten kommt **Omarchy**, das aber Linux-only ist und Fremd-Apps zusammenstellt.
- Das **Omarchy-Muster** ist die Blaupause: App = feste Fensterklasse, TUIs per `.desktop`-Launcher
  als eigene Apps, Platzierung über Compositor-Regeln, Installation per `curl | bash`.
- **Lücke:** Werkzeuge für den Überblick über KI-Agenten sind fast nur macOS-Menüleisten-Apps
  oder tmux-TUIs. Ein kachelbares Fenster „Agenten“ und eines für „Verbrauch“ gibt es
  plattformübergreifend nicht.
- **Technik:** Rust-Kern + ratatui-TUI (sehr leicht), danach eine native GUI mit **Iced**.

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
| **ratatui (TUI)** | 5–20 MB + Terminal | perfekt (Terminal-Fenster) | **Phase 1.** Überall gleich, kachelt ideal, passt zu TorroMail und TorroCast |
| **Iced** | 40–100 MB | sehr gut (COSMIC) | **Phase 2 (GUI).** Elm-Architektur, macOS und Windows ok |
| Slint | sehr gering | gut (winit) | Leichteste Alternative, eigene Markup-Sprache, Lizenzmodell beachten |
| egui | 30–60 MB | gut | Schnell gebaut, wirkt weniger nativ |
| GPUI (Zed) | gering | gut | Release-Stand unsicher, als Abhängigkeit riskant |
| Tauri 2 | 30–60 MB + WebView | **Probleme**: WebKitGTK zeigt leere Fenster bei explicit sync/NVIDIA (https://v2.tauri.app/develop/debug/linux-graphics/) | Verworfen |
| Wails v3 | ~10 MB + WebView | gleiche WebKitGTK-Probleme | Verworfen |
| Flutter | 80–150 MB | ok | Zu schwer für 10 Apps |
| Electron | 100–300 MB | ok | Verworfen |
| Quickshell / AGS / eww | — | nur Hyprland | Für Leisten und Overlays, nicht für normale Fenster; passt für Omarchy-Widgets |

## 5. Alles auf einmal installieren

- **Homebrew:** Tap `mahype/homebrew-torro` mit einer Formel pro App plus einer Meta-Formel
  oder einem Brewfile (`brew bundle`), siehe https://docs.brew.sh/Brew-Bundle-and-Brewfile.
- **AUR:** `torro<app>-bin` pro App und ein Metapaket `torroworkplace` mit `depends=()`.
  Vorlagen: `TorroMail/packaging/aur/torromail-bin/PKGBUILD` und die CI in `TorroMail/.github/workflows/`.
- **dist (cargo-dist):** baut aus dem Workspace Binärpakete, Homebrew-Formeln und Shell-/PowerShell-Installer.
- **Windows (später):** Scoop-Bucket; für winget eine `winget import`-Liste, weil es dort keine Metapakete gibt.
- **`install.sh`:** erkennt das OS und ruft brew, pacman/AUR oder einen Binär-Download auf.
  Auf Omarchy richtet es zusätzlich das workspace-setups-Plugin ein.
