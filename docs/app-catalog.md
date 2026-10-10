# App-Katalog

Ideensammlung. Jede App beantwortet **eine Frage** und läuft in ihrem eigenen Fenster.
Status: **Kern** = Phase 1 · **v0.1** = erste Version (Phase 2) · **nächste** = direkt danach · **Idee** = gesammelt.

Alle Apps bauen auf dem Kern `torroworkplace` auf (Dienst mit Tray-Icon, Steuerzentrale, Konten, Sync, MCP).

## Planen

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| **Aufgaben** (`torrotasks`) | Was muss ich tun? | CalDAV-VTODO (Nextcloud Tasks, iCloud Erinnerungen über CalDAV …); später Todoist | **v0.1** |
| **Kalender** (`torrocalendar`) | Was steht an? | CalDAV-VEVENT; später Google Calendar. Darstellungsideen: [calendar-views.md](calendar-views.md) | **v0.1** |
| Tagesplan | Was ist heute dran, Termine und Aufgaben zusammen? | Kalender und Aufgaben | Idee |
| Fokus / Pomodoro / Zeiterfassung | Woran arbeite ich gerade und wie lange schon? | lokal; Export nach CalDAV | Idee |
| Notizen / Scratchpad | Wo lege ich schnell etwas ab, auch für Agenten? | Markdown-Dateien | Idee |

## Arbeiten mit KI

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| **Agenten-Übersicht** | Welche Agenten laufen, wo, und wer wartet auf mich? | Claude-Code-/Codex-Sitzungen (`~/.claude/projects/*.jsonl`, Hooks), tmux | **nächste** |
| Verbrauch & Kosten | Wie viel habe ich heute und diesen Monat verbraucht, wie nah bin ich am Limit? | lokale JSONL-Logs (Prinzip von ccusage) | Idee |
| Freigaben-Eingang | Welche Berechtigungsanfragen von Agenten sind offen? | Claude-Code-Hooks / MCP | Idee |
| Kontext-Board | Woran arbeiten die Agenten gerade (Branch, Ticket, Plan)? | Git-Worktrees, Plan-Dateien | Idee |

Gerade bei der Agenten-Übersicht und beim Verbrauch gibt es eine Lücke: Was es gibt, sind
macOS-Menüleisten-Apps oder tmux-TUIs (siehe [research.md](research.md)).

## Entwicklung

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| GitHub-Dashboard | Welche PRs, Reviews, CI-Läufe und Benachrichtigungen brauchen mich? | GitHub API (`gh`-Token) | **nächste** |
| Deploy- und Server-Status | Läuft alles? | Health-Checks, Hetzner, Monitoring | Idee |

## Kommunikation & Medien (bestehende Torro-Apps)

| App | Status |
|---|---|
| TorroMail | gibt es; in die Suite und den Installer aufnehmen, Fensterklasse festlegen |
| TorroCast | gibt es; ebenso |
| TorroEQ | gibt es; ebenso |
| Benachrichtigungen | Idee: ein Fenster für alle Benachrichtigungen der Suite |

## Suite-Infrastruktur

| Baustein | Status |
|---|---|
| workspace-setups (Fenster-Sets speichern und laden) | übernommen; aus dem Tray-Menü bedienbar (Kern), AeroSpace-Backend in Phase 4 |
| **Kern `torroworkplace`**: Dienst, Tray-Icon, Steuerzentrale, Konten, Sync, MCP | **Kern** |
| Installer (`install.sh`, AUR-Metapaket, Homebrew-Tap) | v0.1 |
| Omarchy-Leistenwidgets pro App (`status --json` + QML) | Phase 5 |

