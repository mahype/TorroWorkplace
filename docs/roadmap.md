# Roadmap

Die Arbeitspakete stehen als Issues in den [Meilensteinen](https://github.com/mahype/TorroWorkplace/milestones) auf GitHub, eine Phase pro Meilenstein.

## Phase 0: Planung und Gerüst ✅

- Repository, Lizenz (MIT OR Apache-2.0), Dokumente unter `docs/`
- workspace-setups mit Historie nach `apps/workspace-setups` übernommen
- Rust-Workspace-Gerüst: `torro-core`, `torro-ipc`, `torro-pim`, `torro-pim-caldav`, `torro-tui`,
  `torroworkplace`, `torrotasks`, `torrocalendar`

## Phase 1: Kern – `torroworkplace`

Der Kern ist ein Hintergrunddienst mit Icon in der Taskleiste, von dem aus sich alles steuern lässt.

1. **`torro-ipc`:** lokaler Socket, Protokoll (JSON-RPC), Client startet den Kern bei Bedarf.
2. **Dienst `torroworkplace daemon`:** Lebenszyklus, `--headless`, Autostart auf Hyprland und macOS.
3. **Tray-Icon:** `ksni` unter Linux (Omarchy 4-Leiste), `tray-icon` auf macOS und Windows. Menü:
   Apps, Fenster-Sets, Sync-Status, Steuerzentrale, Beenden.
4. **App-Verwaltung:** installierte Apps finden, starten oder fokussieren (Hyprland, AeroSpace).
5. **Konten und Sync:** CalDAV-Erkennung, `sync-collection`, iCalendar lesen und schreiben,
   SQLite-Cache, Sync-Engine, Passwörter im Schlüsselbund. Getestet gegen Radicale, Nextcloud, iCloud.
6. **Steuerzentrale (TUI):** Apps, Konten einrichten, Sync-Status, Fenster-Sets.
7. **Fenster-Sets:** workspace-setups aus Menü und Steuerzentrale (zunächst Linux).
8. **Ein MCP-Server** für die Suite (`torroworkplace mcp`).
9. **`status --json` und `ctl`.**
10. **Integrationen:** Hyprland-Regeln, `.desktop`-Launcher, AeroSpace-Regeln.

Fertig ist Phase 1, wenn das Icon auf Omarchy und dem Mac erscheint, die Nextcloud synchronisiert
wird und sich Fenster-Sets unter Omarchy aus dem Menü laden lassen.

## Phase 2: Aufgaben und Kalender auf dem Kern (erste Version v0.1)

1. **`torrotasks`-TUI:** Listen, Fälligkeit, Priorität, Unteraufgaben, schnelles Erfassen.
2. **`torrocalendar`-TUI:** Tag, Woche, Agenda; Termin anlegen und bearbeiten; „nächster Termin“.
   Ideen für neue Ansichten: [calendar-views.md](calendar-views.md).
3. **MCP-Tools** für Aufgaben und Termine im Suite-Server.
4. **Installation:** `install.sh`, Release-CI, AUR (`-bin` + Metapaket `torroworkplace`), Homebrew-Tap.

Fertig ist v0.1, wenn Kern, Aufgaben und Kalender mit einem Befehl installiert sind, unter
Hyprland und AeroSpace nebeneinander gekachelt laufen und workspace-setups sie unter Omarchy
wiederherstellt.

## Phase 3: GUI

- **Technik wählen:** eine plattformübergreifende Rust-GUI (Iced als Favorit, Slint, egui) oder native
  Oberflächen pro System. Kleiner Prototyp-Vergleich, Prüfstein ist die Tagesspirale.
- GUIs für Steuerzentrale, Aufgaben und Kalender auf demselben Kern
- Kalender-Darstellung: Ideen und Prototypen (Tagesspirale, Jahresring) in [calendar-views.md](calendar-views.md)
- gemeinsames Theme aus den Torro-Tokens
- Paketierung: `.app` für macOS (signiert), `.desktop` und Icon für Linux

## Phase 4: workspace-setups für AeroSpace

- `WindowManagerBackend` einziehen, Backend für AeroSpace, Template-Schema 3 mit Feld `wm`
- Browser-Tabs auf dem Mac (Chrome/Brave unter `~/Library/Application Support`)
- Fenster-Sets auf dem Mac aus dem Tray-Menü, Tastenkürzel in `~/.aerospace.toml`

## Phase 5: Ausbau

- Agenten-Übersicht, Verbrauch & Kosten, GitHub-Dashboard (siehe [app-catalog.md](app-catalog.md))
- Omarchy-Leistenwidgets
- weitere Provider: Google Calendar, Todoist
- Windows: komorebi/GlazeWM-Regeln, Scoop-Bucket, workspace-setups-Backend
- TorroMail, TorroCast und TorroEQ in den Suite-Installer und das Tray-Menü aufnehmen

## Offene Fragen

- ~~Teilen sich Kalender und Aufgaben einen Sync-Daemon?~~ Entschieden: Der Kern synchronisiert für alle.
- Kann `omarchy plugin add` ein Plugin aus einem Unterverzeichnis installieren?
- Zeigt die Omarchy-Leiste (Quickshell) Icon und Menü von `ksni` sauber an?
- GUI-Technik: eine für alle Systeme oder native pro System?
- Bundle-IDs und Signierung auf macOS (Team-ID wie bei TorroMail?)
- workspace-setups: wann in Rust neu schreiben, damit es direkt im Kern läuft?
