# App-Katalog

Ideensammlung. Jede App beantwortet **eine Frage** und läuft in ihrem eigenen Fenster.
Status: **Kern** = Phase 1–3 · **v0.1** = erste Apps (Phase 4) · **nächste** = direkt danach · **Idee** = gesammelt.

Alle Apps bauen auf dem Kern `torroworkplace` auf (Dienst mit Tray-Icon, Steuerzentrale, Konten, Sync, MCP).

## Eingang

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| **Inbox** (`torroinbox`; Fenster „Notification Hub“, „Mail“, „App Feed“, siehe [eingang.md](eingang.md), Entscheidung 21) | Was ist reingekommen, und braucht es mich? | Der Eingang im Kern: wartende Agenten (Hooks), Freigaben, Kontoprobleme, Meldungen von Skripten; später Mail, Nextcloud, GitHub, ntfy, Slack. Konzept: [eingang.md](eingang.md) | **nächste**, in Arbeit ([tasks/eingang.md](tasks/eingang.md)); Fenster je Ausschnitt mit `--lane for-me\|info` oder `--source <quelle>` |

## Planen

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| **Aufgaben** (`torrotasks`) | Was muss ich tun? | CalDAV-VTODO (Nextcloud Tasks, iCloud Erinnerungen über CalDAV …); später Todoist | **v0.1** |
| **Kalender** (`torrocalendar`) | Was steht an? | CalDAV-VEVENT; später Google Calendar. Darstellungsideen: [calendar-views.md](calendar-views.md) | **v0.1** |
| Tagesplan | Was ist heute dran, Termine und Aufgaben zusammen? | Kalender und Aufgaben | Idee |
| Fokus / Pomodoro / Zeiterfassung | Woran arbeite ich gerade und wie lange schon? | lokal; Export nach CalDAV | Idee |
| **Notizen** (`torronotes`) | Wo lege ich schnell etwas ab, auch für Agenten? | Nextcloud Notes (Markdown-Dateien), später auch ein Ordner per WebDAV oder lokal. Konzept: [notizen.md](notizen.md) | **Entwurf** ([tasks/notizen.md](tasks/notizen.md)) |
| **Kontakte** (`torrocontacts`) | Wer ist das, wie erreiche ich ihn, und wo ist das? | CardDAV (Nextcloud zuerst, dann iCloud, Fastmail, Radicale …), Karte mit OpenStreetMap ohne Schlüssel. Konzept: [kontakte.md](kontakte.md) | **Entwurf** ([tasks/kontakte.md](tasks/kontakte.md)) |

## Arbeiten mit KI

| App | Frage | Datenquelle | Status |
|---|---|---|---|
| **Assistent** (`torroassistant`) | Kannst du das für mich erledigen? | Chat (später per Weckwort) mit einem KI-Konto (Open First: Ollama, OpenRouter) oder einem verbundenen Agenten, Werkzeuge aus dem Befehlskatalog, Rechte je Bereich. Konzept: [assistent.md](assistent.md) | **Entwurf** ([tasks/assistent.md](tasks/assistent.md)) |
| **Agenten-Übersicht** | Welche Agenten laufen, wo, und wer wartet auf mich? | Claude-Code-/Codex-Sitzungen (`~/.claude/projects/*.jsonl`, Hooks), tmux | **nächste** |
| Verbrauch & Kosten | Wie viel habe ich heute und diesen Monat verbraucht, wie nah bin ich am Limit? | lokale JSONL-Logs (Prinzip von ccusage) | Idee |
| ~~Freigaben-Eingang~~ | Welche Berechtigungsanfragen von Agenten sind offen? | Claude-Code-Hooks / MCP | aufgegangen in **Inbox** |
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
| ~~Benachrichtigungen~~ | aufgegangen in **Inbox**: ein Fenster für alles, was die Suite und ihre Quellen melden |
| **Mail-Client der Suite** | Entschieden (Variante A in [eingang.md](eingang.md#mail-eigene-app-oder-quelle-im-eingang)): keine eigene Mail-App, Mail wird eine Quelle der **Inbox** (Ordner in der Seitenleiste, ganze Mail im Paneel, Schreiben in eigenem Fenster). Der Mail-Kern spricht IMAP/SMTP direkt ([mail.md](mail.md)); ein Fenster nur für Mail dann mit `torroinbox --source mail` (E4) |
| Chat (Nextcloud Talk) | Idee: erst ungelesene Nachrichten im Tray und schnell antworten, später eine schlanke Chat-App; nur Text, keine Anrufe |

## Suite-Infrastruktur

| Baustein | Status |
|---|---|
| Workspace-Vorlagen (Workspaces speichern und laden), neu in Rust nach dem Vorbild von workspace-setups | **Kern**, zuerst; Hyprland, dann AeroSpace und GlazeWM ([core.md](core.md)) |
| **Kern `torroworkplace`**: Dienst, Tray-Icon, Steuerzentrale; später Konten, Sync, MCP | **Kern** |
| Installer (`install.sh`, AUR-Metapaket, Homebrew-Tap) | v0.1 |
| Omarchy-Leistenwidgets pro App (`status --json` + QML) | Phase 5 |

