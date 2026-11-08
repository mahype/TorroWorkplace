# Roadmap

> **Status note (November 2026):** This document includes historical phase plans and
> is not a complete feature-by-feature implementation report. The repository has since
> grown to include calendar, tasks, mail/inbox, assistant, notifications and settings
> implementations. Recent merged PRs include #590 (calendar), #588 (assistant),
> #585 (tasks), #583 (mail window), #581 (notifications) and #580 (control centre).
> A crate or merged PR proves code exists, **not** that a feature is fully tested,
> released or production-ready. Use current code, closed issues and merged PRs to
> establish the exact status of an individual feature. Work follows [CLAUDE.md](../CLAUDE.md)
> and [the issue workflow](issue-workflow.md).

Die Planung steht in diesem Dokument, die Aufgaben der aktuellen Phase unter [tasks/](tasks/). GitHub-Issues sind für Fehler und Einzelthemen da, nicht für die Vorausplanung.

## Phase 0: Planung und Gerüst ✅

- Repository, Lizenz (MIT OR Apache-2.0), Dokumente unter `docs/`
- workspace-setups mit Historie nach `apps/workspace-setups` übernommen (als Referenz)
- Rust-Workspace-Gerüst: `torro-core`, `torro-ipc`, `torro-pim`, `torro-pim-caldav`, `torro-tui`,
  `torroworkplace`, `torrotasks`, `torrocalendar`

## Phase 1: Kern-Prototyp – Tray-Icon und Workspace-Vorlagen (Linux)

Der Kern ist ein Dienst mit Tray-Icon, der Workspaces des Tiling-Managers als Vorlagen speichert
und wieder aufbaut. Konzept: [core.md](core.md). Neu in Rust, workspace-setups dient nur als Referenz.
Aufgaben, klein genug für Haiku: [tasks/phase-1.md](tasks/phase-1.md).

1. **Versuch:** `ksni`-Icon mit Untermenüs im Tray der Omarchy-Leiste.
2. **`torro-wm`:** Trait `WindowManager`, Backend Hyprland (Abfragen, Steuern, Ereignisse).
3. **`torro-setups`:** Vorlagen (Schema 3), Erfassen aus Fensterrechtecken, Laden mit den Strategien
   „neu“ und „herholen“, Startbefehle unter Linux.
4. **Dienst `torroworkplace daemon`:** Tray-Menü, Benachrichtigungen, Einzelinstanz.
5. **`torro-ipc`, `ctl` und `status --json`.**
6. **CI** für Linux, macOS und Windows.

Fertig ist Phase 1, wenn das Icon unter Omarchy erscheint und sich Workspaces aus dem Menü und mit
`ctl` speichern und laden lassen (Abnahme in [core.md](core.md#prototyp-umfang-und-abnahme)).

## Vorgezogen: Kalender und To-dos (erste Version)

Am 2026-10-11 entschieden: Die beiden Apps kommen direkt nach dem Kern-Prototyp, mit Nextcloud im
Kern (Konto, CalDAV, Zwischenspeicher). Aufgaben: [tasks/kalender-aufgaben.md](tasks/kalender-aufgaben.md).
Damit ziehen Teile aus Phase 3 und 4 nach vorn.

## Als Nächstes: Steuerzentrale (zentrale Einstellungen)

Alle suite-weiten Einstellungen, vor allem die Konten, an einer Stelle in der Haupt-App, nach dem
Vorbild von TorroWhisper und TorroMail. Konzept: [settings.md](settings.md). Vorgemerkte Konten:
**GitHub** (Issues und Pull Requests als Filterlisten und Verweise, Konzept: [github.md](github.md)), **Slack** (Erwähnungen, Nachrichten),
**KI-Anbieter** (API-Schlüssel, Agenten).

## Vorgemerkt: Tiling- und Fenster-Modus (grundlegend)

Am 2026-10-11 vorgemerkt: Nicht jeder hat einen Tiling-Manager. Neben dem Tiling-Modus kommt ein
Fenster-Modus mit einer Torro-Arbeitsfläche, auf der die Elemente der Suite frei (im Raster)
angeordnet werden, mit fertigem Startaufbau und eigenen Vorlagen. Konzept: [modes.md](modes.md).
Die ersten Schritte (Apps in Bibliothek + Programm teilen, Element-Interface) sollen vor weiteren
großen App-Aufgaben kommen, weil sie später teurer werden.

## Vorgemerkt: weitere Nextcloud-Funktionen

Am 2026-10-11 aus der Übersicht über die Nextcloud-Schnittstellen ausgewählt. Jede Funktion wird
über die Capabilities des Servers erkannt; fehlt sie, sagt die Oberfläche es freundlich.

- **Avatar:** Profilbild des eigenen Kontos und anderer Benutzer (`/index.php/avatar/<uid>/<größe>`).
- **Notifications:** Benachrichtigungen des Servers im Tray und als Desktop-Benachrichtigung, mit
  ihren Aktionen als Knöpfe; neue Benachrichtigungen per `notify_push` statt Abfragen.
- **Unified Search:** eine Suche über alle Nextcloud-Apps; Treffer öffnen in der passenden
  Torro-App, alles andere im Browser.
- **Talk ausprobieren:** erst Text-Chat (ungelesene Nachrichten, schnell antworten), keine Anrufe.
- **Nextcloud Mail** nutzen wir nicht: Die App ist ein Webmailer auf dem Server und gibt keine
  Zugangsdaten heraus. Mail läuft über einen eigenen Client (siehe [app-catalog.md](app-catalog.md)).

## Phase 2: Kern auf allen Systemen

1. **Steuerzentrale** (`torroworkplace ui`, Iced): Vorlagen benennen, Strategien je Fenster, Einstellungen.
2. **Tastenplätze 1–0** und **Wiederherstellen beim Anmelden** (in #243 bereits umgesetzt; hier als historische Phase-2-Planung aufgeführt).
3. **Backend AeroSpace** (macOS), LaunchAgent, Tastenkürzel in `~/.aerospace.toml`.
4. **Backend GlazeWM** (Windows), Autostart, Tastenkürzel in der GlazeWM-Konfiguration.
5. Funktionen aus workspace-setups nachziehen: automatische Strategie, Floating und Vollbild,
   Terminalprogramme, Browser-Tabs, Import von Schema-2-Vorlagen.
6. **Installation:** `install.sh`, Release-CI, AUR, Homebrew-Tap, später Scoop.

Fertig ist Phase 2, wenn Vorlagen unter Hyprland, AeroSpace und GlazeWM gespeichert und geladen
werden und der Kern mit einem Befehl installiert ist.

Aufgaben für macOS und Windows (Named Pipes, Tray, AeroSpace, GlazeWM, Autostart, Signatur):
[tasks/plattformen.md](tasks/plattformen.md).

## Phase 3: Konten, Sync und MCP im Kern (historischer Phasenplan; Teile bereits implementiert)

- Konten einmal im Kern einrichten, Passwörter im Schlüsselbund; Nextcloud zuerst, Anmeldung über
  Login Flow v2 (Browser statt Passwort, App-Passwort widerrufbar)
- CalDAV-Erkennung, `sync-collection`, SQLite-Cache, Sync-Engine; getestet gegen Radicale und Nextcloud
- App-Verwaltung: Suite-Apps finden, starten, fokussieren
- ein MCP-Server für die Suite (`torroworkplace mcp`)

## Phase 4: Apps auf dem Kern (historischer Phasenplan; App-Entwicklung bereits aktiv)

- **Aufgaben** (`torrotasks`) und **Kalender** (`torrocalendar`), GUI mit Iced auf demselben Kern
- Kalender-Darstellung: Ideen und Prototypen (Tagesspirale, Jahresring) in [calendar-views.md](calendar-views.md)
- gemeinsames Theme aus den Torro-Tokens

## Phase 5: Ausbau

- Agenten-Übersicht, Verbrauch & Kosten, GitHub-Dashboard (siehe [app-catalog.md](app-catalog.md))
- Omarchy-Leistenwidgets
- weitere Provider: Google Calendar, Todoist
- TorroMail, TorroCast und TorroEQ in den Suite-Installer und das Tray-Menü aufnehmen

## Aktueller Entwicklungsstand (Orientierung)

- **Workspace-Kern:** Prototyp und Vorlagenfunktionen sind in [core.md](core.md) und Issue #243 dokumentiert.
- **Kalender und Aufgaben:** Implementierungen vorhanden; aktuelle Änderungen in PRs #590 und #585.
- **Inbox, Mail und Benachrichtigungen:** Implementierungen vorhanden; aktuelle Änderungen u. a. in PRs #583, #584 und #581.
- **KI-Assistent und Steuerzentrale:** Implementierungen vorhanden; aktuelle Änderungen in PRs #588 und #580.
- **Reifegrad:** Für jede Funktion gesondert anhand Code, Tests und Release-Stand prüfen; diese Übersicht behauptet keine allgemeine Produktionsreife.

## Entschieden

- Der Kern synchronisiert für alle Apps.
- Tiling-Manager: **Hyprland** (Linux), **AeroSpace** (macOS), **GlazeWM** (Windows).
- workspace-setups wird nicht übernommen, sondern im Kern in Rust neu gebaut; das Omarchy-Plugin wird nicht weiterentwickelt.
- GUI: Iced.

## Offene Fragen

- **Fenster-Modus:** Wie heißen die Elemente (Widgets?) und die Fläche (Workplace, Board, Desk?)?
  Siehe [modes.md](modes.md#offen).

- Zeigt die Omarchy-Leiste (Quickshell) Icon und Untermenüs von `ksni` sauber an?
- Wie genau lassen sich Layouts unter AeroSpace und GlazeWM aufbauen?
- Bundle-IDs und Signierung auf macOS (Team-ID wie bei TorroMail?)
- **Mail-Client der Suite:** Wie passt Mail ins Tiling-Schema („eine App, eine Frage, ein Fenster“)?
  Ein Fenster oder mehrere kleine (z. B. Eingang sichten, Lesen, Schreiben)? Wie verhält er sich zu
  TorroMail: Teile übernehmen, ersetzen oder getrennt weiterführen? Liegen die Mail-Konten im Kern?
