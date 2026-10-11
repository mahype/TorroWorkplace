# Roadmap

Die Arbeitspakete stehen als Issues in den [Meilensteinen](https://github.com/mahype/TorroWorkplace/milestones) auf GitHub, eine Phase pro Meilenstein.

## Phase 0: Planung und Gerüst ✅

- Repository, Lizenz (MIT OR Apache-2.0), Dokumente unter `docs/`
- workspace-setups mit Historie nach `apps/workspace-setups` übernommen (als Referenz)
- Rust-Workspace-Gerüst: `torro-core`, `torro-ipc`, `torro-pim`, `torro-pim-caldav`, `torro-tui`,
  `torroworkplace`, `torrotasks`, `torrocalendar`

## Phase 1: Kern-Prototyp – Tray-Icon und Workspace-Vorlagen (Linux)

Der Kern ist ein Dienst mit Tray-Icon, der Workspaces des Tiling-Managers als Vorlagen speichert
und wieder aufbaut. Konzept: [core.md](core.md). Neu in Rust, workspace-setups dient nur als Referenz.

1. **Versuch:** `ksni`-Icon mit Untermenüs im Tray der Omarchy-Leiste.
2. **`torro-wm`:** Trait `WindowManager`, Backend Hyprland (Abfragen, Steuern, Ereignisse).
3. **`torro-setups`:** Vorlagen (Schema 3), Erfassen aus Fensterrechtecken, Laden mit den Strategien
   „neu“ und „herholen“, Startbefehle unter Linux.
4. **Dienst `torroworkplace daemon`:** Tray-Menü, Benachrichtigungen, Einzelinstanz.
5. **`torro-ipc`, `ctl` und `status --json`.**
6. **CI** für Linux, macOS und Windows.

Fertig ist Phase 1, wenn das Icon unter Omarchy erscheint und sich Workspaces aus dem Menü und mit
`ctl` speichern und laden lassen (Abnahme in [core.md](core.md#prototyp-umfang-und-abnahme)).

## Phase 2: Kern auf allen Systemen

1. **Steuerzentrale** (`torroworkplace ui`, Iced): Vorlagen benennen, Strategien je Fenster, Einstellungen.
2. **Tastenplätze F1–F10** und **Wiederherstellen beim Anmelden**.
3. **Backend AeroSpace** (macOS), LaunchAgent, Tastenkürzel in `~/.aerospace.toml`.
4. **Backend GlazeWM** (Windows), Autostart, Tastenkürzel in der GlazeWM-Konfiguration.
5. Funktionen aus workspace-setups nachziehen: automatische Strategie, Floating und Vollbild,
   Terminalprogramme, Browser-Tabs, Import von Schema-2-Vorlagen.
6. **Installation:** `install.sh`, Release-CI, AUR, Homebrew-Tap, später Scoop.

Fertig ist Phase 2, wenn Vorlagen unter Hyprland, AeroSpace und GlazeWM gespeichert und geladen
werden und der Kern mit einem Befehl installiert ist.

## Phase 3: Konten, Sync und MCP im Kern

- Konten einmal im Kern einrichten, Passwörter im Schlüsselbund; Nextcloud zuerst, Anmeldung über
  Login Flow v2 (Browser statt Passwort, App-Passwort widerrufbar)
- CalDAV-Erkennung, `sync-collection`, SQLite-Cache, Sync-Engine; getestet gegen Radicale und Nextcloud
- App-Verwaltung: Suite-Apps finden, starten, fokussieren
- ein MCP-Server für die Suite (`torroworkplace mcp`)

## Phase 4: Apps auf dem Kern

- **Aufgaben** (`torrotasks`) und **Kalender** (`torrocalendar`), GUI mit Iced auf demselben Kern
- Kalender-Darstellung: Ideen und Prototypen (Tagesspirale, Jahresring) in [calendar-views.md](calendar-views.md)
- gemeinsames Theme aus den Torro-Tokens

## Phase 5: Ausbau

- Agenten-Übersicht, Verbrauch & Kosten, GitHub-Dashboard (siehe [app-catalog.md](app-catalog.md))
- Omarchy-Leistenwidgets
- weitere Provider: Google Calendar, Todoist
- TorroMail, TorroCast und TorroEQ in den Suite-Installer und das Tray-Menü aufnehmen

## Entschieden

- Der Kern synchronisiert für alle Apps.
- Tiling-Manager: **Hyprland** (Linux), **AeroSpace** (macOS), **GlazeWM** (Windows).
- workspace-setups wird nicht übernommen, sondern im Kern in Rust neu gebaut.
- GUI: Iced.

## Offene Fragen

- Zeigt die Omarchy-Leiste (Quickshell) Icon und Untermenüs von `ksni` sauber an?
- Wie genau lassen sich Layouts unter AeroSpace und GlazeWM aufbauen?
- Bundle-IDs und Signierung auf macOS (Team-ID wie bei TorroMail?)
- Was passiert mit dem bisherigen Omarchy-Plugin workspace-setups, sobald der Kern es ersetzt?
