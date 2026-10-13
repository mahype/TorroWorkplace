# Eingang: eine Stelle für alles, was reinkommt

Stand: 2026-10-11. **Grundsätze entschieden** (siehe „Entscheidungen“), Einzelheiten ab E4 noch offen. Baut auf
[mail.md](mail.md) auf und fasst die Ideen „Benachrichtigungen“, „Freigaben-Eingang“ und
„Chat“ aus dem [App-Katalog](app-catalog.md) zusammen. Die Fragen zum Entscheiden stehen am Ende.

## Worum es geht

Die Suite soll die Übersicht zurückbringen ([vision.md](vision.md)). Heute kommt, was Aufmerksamkeit
will, aus vielen Ecken: Mail in mehreren Konten, Slack, Nextcloud Talk, GitHub, CI, Server, und seit
der Arbeit mit Agenten zusätzlich „Claude wartet auf Freigabe“, „Codex ist fertig“, „PR vom Agenten
ist offen“. Jede Quelle hat ihre eigene Glocke, ihr eigenes Rot und ihre eigene Vorstellung davon,
was wichtig ist. Die Folge: Man prüft zehn Stellen, und das Wichtige geht trotzdem unter.

Der Vorschlag: **Die Suite bekommt drei Säulen.**

| Säule | Frage | App |
|---|---|---|
| **Eingang** | Was ist reingekommen, und braucht es mich? | neu: **Inbox** (`torroinbox`) |
| **Termine** | Wann ist was? | `torrocalendar` |
| **Aufgaben** | Was muss ich tun? | `torrotasks` |

Alles, was von außen kommt, landet zuerst im Eingang, egal ob Mail, Chat-Nachricht,
Benachrichtigung oder Agent. Von dort wird es **erledigt, vertagt, beantwortet oder zu einer Aufgabe
bzw. einem Termin**. Der Eingang ist kein Archiv und kein Dashboard, er soll leer werden können.

```
 Mail ─┐                                   ┌─ erledigt
 Slack ─┤                                  ├─ später (kommt wieder)
 Talk ──┤      ┌──────────────────┐        ├─ beantwortet
 GitHub ┼────▶ │  Eingang (Kern)  │ ─────▶ ├─ → Aufgabe   (torrotasks)
 Agenten┤      │  sortieren, ruhig│        ├─ → Termin    (torrocalendar)
 Server ┤      └──────────────────┘        └─ Quelle stummschalten
 Suite ─┘        ▲            │
                 │ MCP        ▼ Desktop-Benachrichtigung nur für „Für mich“
              Agenten lesen, fassen zusammen, melden sich selbst
```

## Drei Arten von Dingen, nur eine gehört in den Eingang

Damit der Eingang nicht wieder zur Flut wird, trennen wir sauber:

| Art | Beispiel | Wohin |
|---|---|---|
| **Nachricht** | Mail, Slack-DM, Talk | Eingang. Hat einen Absender, man kann antworten |
| **Hinweis** | GitHub: Review angefragt, CI rot, Claude wartet, Server down, Nextcloud teilt Datei | Eingang. Zeigt auf etwas anderswo, Aktion ist „öffnen“ oder „erledigt“ |
| **Zustand** | Welche PRs sind offen? Welche Agenten laufen? Läuft der Server? | **Nicht** in den Eingang, sondern in eigene Apps (GitHub-Dashboard, Agenten-Übersicht). Der Eingang zeigt nur die **Änderung**, die mich braucht |

Damit bleiben die geplanten Apps „Agenten-Übersicht“ und „GitHub-Dashboard“ sinnvoll, aber der
„Freigaben-Eingang“ und „Benachrichtigungen“ gehen im Eingang auf.

## Mail: eigene App oder Quelle im Eingang?

Das ist die wichtigste Frage dieser Runde. Mail ist zugleich die größte Quelle und die einzige,
die Tiefe braucht (Ordner, Archiv durchsuchen, lange Texte, Anhänge, Schreiben).

| Weg | Wie | Für | Gegen |
|---|---|---|---|
| **A: Ein Eingang, Mail als Quelle (entschieden)** | Neue Mail ist ein Eintrag wie jeder andere. Öffnen zeigt die ganze Mail im Paneel. In der Seitenleiste hat jede Quelle eine Gruppe; unter „Mail ▸ mailbox.org“ liegen die Ordner, ein Klick zeigt den Ordner in derselben Liste. Schreiben in eigenem Fenster | Eine Stelle, eine Bedienung, ein Zähler. Passt zur Kontoentscheidung (Gruppen statt Leiste). Weniger Code: eine Liste, ein Paneel | Die App wird größer als Kalender und Aufgaben. Wer „nur Mail“ will, muss filtern (Fenster mit `--source mail` geht) |
| B: Eingang + eigene Mail-App | Eingang zeigt neue Mails, die Mail-App macht Ordner, Suche, Schreiben | Klare Rollen, die Mail-App kann Mailbird-artig voll werden | Zwei Orte für dieselbe Mail, gelesen/erledigt muss synchron sein, zwei Apps bauen |
| C: Nur Mail-App, Benachrichtigungen getrennt | Wie bisher in [mail.md](mail.md), dazu eine Benachrichtigungs-App | Bekannt | Löst das Grundproblem nicht: wieder zwei Glocken |

Bei A bleibt alles aus [mail.md](mail.md) gültig (Kern, Konten, Ordner-Regeln, Rechte). Nur die
Oberfläche ist nicht „Mail-App“, sondern „Eingang mit Mail-Tiefe“. Der Name TorroMail wäre dann
frei für später oder fiele weg.

## Was den Alltag leichter machen soll

Konkrete Szenen, an denen sich das Konzept messen lassen muss:

1. **Morgens:** Der Eingang zeigt „Für mich: 6“ und „Zur Info: 41“. Die sechs sind zwei Mails von
   Kunden, ein Review, ein roter CI-Lauf, eine Freigabe, eine Slack-Erwähnung. Die 41 lese ich, wenn
   ich will, oder frage Claude: „Fass mir die Info-Einträge seit gestern zusammen.“
2. **Agent wartet:** Claude Code fragt in einem Terminal auf Workspace 4 um Erlaubnis. Es kommt eine
   Desktop-Benachrichtigung „Claude (TorroWorkplace, feat/mail) waits for permission“. Ein Klick
   **springt in genau dieses Fenster**, der Kern kennt die Fenster über `torro-wm`. Geht es dort
   weiter, verschwindet der Eintrag von selbst.
3. **Agent fertig:** Codex meldet „done“ in einem Projekt. Eintrag „Codex finished: refactor
   settings“ unter „Für mich“, ohne Popup, wenn ich gerade im Fokus bin.
4. **Review und CI:** GitHub meldet „review requested“. Ist der PR gemergt oder das Review erledigt,
   erledigt sich der Eintrag selbst. Die GitHub-Mail zum selben Vorgang wird **nicht** ein zweiter
   Eintrag, sondern dem GitHub-Eintrag zugeordnet.
5. **Einladung:** Mail mit Termineinladung. Oben „Accept / Maybe / Decline“, der Termin landet im
   Kalender, der Eintrag ist erledigt.
6. **Slack im Fokusblock:** Während eines Termins mit „busy“ oder einer Fokus-Sitzung kommen
   Erwähnungen still rein und werden nach dem Block gesammelt gezeigt. Nur Ausnahmen (z. B. Server
   down, Freigabe) dürfen stören.
7. **Daraus eine Aufgabe:** Kundenmail „Bitte bis Freitag Angebot“. Taste `t`, die Aufgabe entsteht
   in `torrotasks` mit Fälligkeit Freitag und einem Link zurück auf die Mail.
8. **Server down:** Uptime Kuma meldet über ntfy. Eintrag unter „Für mich“, laut, auch im Fokus.
9. **Newsletter:** landet unter „Zur Info“, nie als Popup, nie im Zähler.
10. **Freigabe:** „Claude Desktop wants to delete event "Team meeting"“ mit „Allow / Deny“ direkt
    im Eintrag und in der Benachrichtigung ([api.md](api.md), Freigaben).

## Grundregeln

1. **Ein Vorgang, ein Eintrag.** 20 Nachrichten in einem Slack-Thread, fünf Kommentare an einem PR,
   eine Mail-Unterhaltung: je ein Eintrag mit Zähler. Neues schiebt ihn nach oben.
2. **Zwei Spuren:** **Für mich** (direkt an mich, wartet auf mich) und **Zur Info** (Rundschreiben,
   Bots, Erfolgsmeldungen, Kanäle). Nur „Für mich“ zählt, nur „Für mich“ darf Popups zeigen.
3. **Einträge erledigen sich selbst**, wenn der Anlass weg ist: Mail woanders gelesen, PR gemergt,
   Agent läuft weiter, CI wieder grün, Freigabe abgelaufen.
4. **Erledigt wirkt zurück**, wo die Quelle das kann: Mail als gelesen markieren, GitHub-Thread
   „done“, Slack „gelesen“, Nextcloud-Benachrichtigung löschen. Wo nicht, merkt sich der Kern es.
5. **Ruhe ist Standard.** Popups nur für „Für mich“, im Fokus nur für „dringend“. Zur-Info-Einträge
   kommen höchstens als Sammelmeldung.
6. **Erst sortieren nach festen Regeln, dann lernen, KI höchstens als Vorschlag.** Nichts wird von
   einer KI unsichtbar gemacht; Zusammenfassungen können falsch sein, das Original bleibt sichtbar.
7. **Nichts verlässt den Rechner**, was nicht muss. Der Eingang läuft lokal im Kern, keine eigene
   Cloud, kein Relais von Torro.

## Sortieren: wie etwas „Für mich“ wird

In dieser Reihenfolge, die erste passende Regel gewinnt:

1. **Feste Regeln je Quelle** (nicht einstellbar, Entscheidungen statt Optionen):
   - Freigaben, wartende Agenten, Server down, CI rot auf eigenem PR → Für mich, dringend.
   - DM, Erwähnung, Review angefragt, Mail direkt an mich von bekanntem Absender → Für mich.
   - Mail mit `List-Id`, `List-Unsubscribe`, `Precedence: bulk` oder `Auto-Submitted` → Zur Info.
   - Kanal-Nachrichten ohne Erwähnung, CI grün, Bots → Zur Info.
2. **Neue Absender fragt der Eingang einmal** (wie der „Screener“ bei HEY): „First mail from
   x@y.de: For me / Info / Block“. Die Antwort gilt dauerhaft für diesen Absender.
3. **Umsortieren lernt:** Schiebe ich etwas von Info nach Für mich (oder umgekehrt), fragt der
   Eingang „Always for this sender/channel?“.
4. **Optional, nur mit KI-Konto** ([ki.md](ki.md)): ein Modell schlägt eine Spur vor und schreibt
   eine Zeile Zusammenfassung. Nur als Vorschlag, mit Kennzeichnung; ohne KI-Konto fehlt die Funktion
   einfach, nichts anderes ändert sich.

**Ausblenden in Stufen (E3.10, E3.11):** „Hide“ ist kein Spam. Das Menü „Hide from inbox“ (`m`, „…“
im Paneel, „Hide“ in der Aktionsleiste) bietet für Mail „This conversation“ (`inbox.hide_sender` mit
`conversation`: erledigt, aber anders als `e` bleibt die Mail ungelesen; eine neue Antwort bringt
sie zurück), „Everything from <Adresse>“, „Everything from <Domain>“, dann „Always to Info“ und – nur
wenn das Konto einen Ordner mit Rolle Junk hat – „Move to junk“; für andere Quellen „This entry“
und „Mute <Quelle>“, die Suite nie. Absender und Domain sind eine Regel (`inbox.hide_sender`): Neue
Mail davon wird kein Eintrag, offene und vertagte passende Einträge werden erledigt. Die Mail selbst
bleibt **unverändert** im Postfach, nicht gelesen, nicht verschoben – anders als „erledigt“, das
die Mail als gelesen markiert. Domain heißt der Teil nach `@`, klein, genau (keine Subdomains); die
Regel gilt für **alle** Mail-Konten (E3.11), auch Regeln, die vorher für ein Konto angelegt wurden.
Die Regeln stehen in der Steuerzentrale › Inbox › Rules („Hidden: news@shop.example“, „Hidden:
everything from shop.example“, darunter „All mail accounts“) mit „Remove“.

**Rückgängig (E3.11):** Nach Done, Later und Hide erscheint unten mittig ein Hinweis
(`widgets::toast`) mit „Undo“; er verschwindet nach 5 s oder mit `Escape`. `u` in der Liste und
`Ctrl+Z` (⌘Z auf macOS) lösen Undo aus. Undo nimmt genau die Wirkung zurück, mit vorhandenen
Befehlen: Done → `set_item_done` wieder offen; Later → `snooze_item` ohne Zeit; Hide „This
conversation“ → wieder offen; Hide Absender/Domain → `remove_rule` (nur wenn der Aufruf die Regel
neu angelegt hat), dann die dadurch erledigten Einträge wieder offen (die Antwort von
`hide_sender` nennt sie; vorher vertagte kommen offen zurück). Done einer Mail hat die Mail schon
als gelesen markiert; Undo öffnet nur den Eintrag wieder.

**Dringend** ist eine kleine, feste Menge (Freigabe, Agent wartet, Server down, CI rot auf dem
eigenen PR) und ein Absender, den ich selbst als „VIP“ markiere. Mehr nicht.

**Fokus:** Läuft ein Kalendertermin mit „busy“ oder eine Fokus-Sitzung (App-Idee „Fokus“ im
Katalog), zeigt der Kern nur Dringendes, wenn der Schalter dafür an ist (siehe „Ruhe“ unten). Danach
kommt eine Sammelmeldung. Das ist eine Brücke zwischen Eingang und Kalender, die sonst niemand hat.

## Quellen

Was sich anbinden lässt, recherchiert am 2026-10-11. „Ohne Admin“ heißt: Ein Mensch kann es allein
einrichten, ohne Freigabe einer Firma oder eine bei Torro registrierte App.

### Stufe 1: ohne fremde Anmeldung oder schon vorhanden

| Quelle | Weg | Zurückwirken | Bemerkung |
|---|---|---|---|
| **Suite selbst** | intern: Freigaben, Sync-Fehler, Konto braucht neue Anmeldung, Erinnerungen aus Kalender und Aufgaben | ja | Erste Quelle, ganz ohne Netz |
| **Claude Code** | Hooks: `Notification` (u. a. `permission_prompt`, `idle_prompt`, `agent_needs_input`, `agent_completed`), `Stop`, `SubagentStop`; Daten: `session_id`, `cwd`, `message` | indirekt | Hook ruft `torroworkplace notify` (über `core.sock`) oder, mit HTTP-Hooks, später einen lokalen Endpunkt |
| **Codex CLI** | Hooks (`Stop`, `PermissionRequest` …) oder `notify` | indirekt | wie Claude Code |
| **Gemini CLI, Cursor** | Hooks (`Notification` bzw. `stop`) | nein | Cursor hat keinen Hook für „wartet“ |
| **Mail** | IMAP IDLE aus dem Mail-Kern ([mail.md](mail.md)) | gelesen, archivieren | größte Quelle |
| **Nextcloud** | Benachrichtigungen (OCS) und Talk (ungelesen, Erwähnungen); Push über `notify_push`, sonst Abfrage | ja (löschen, gelesen) | Konto gibt es schon, steht in [api.md](api.md) als vorgemerkt |
| **GitHub** | `GET /notifications`, Grund je Eintrag (`review_requested`, `mention`, `ci_activity`, `agent_session_finished` …), Abfrage im Takt von `X-Poll-Interval` | ja („done“ = Thread löschen) | Konto ist in [settings.md](settings.md) geplant; braucht klassisches Token mit `notifications` |
| **ntfy / Gotify** | Themen abonnieren (JSON-Strom/WebSocket) | nein | **Universeller Eingang** für alles, was selbst melden kann: CI, Home Assistant, Uptime Kuma, cron, eigene Skripte. Löst auch „Webhook braucht öffentliche URL“: Webhooks gehen an ntfy, der Kern abonniert |
| **Eigene Skripte** | `torroworkplace notify --title … --source backup` bzw. `inbox.post_item` mit Skript-Schlüssel | – | eine Zeile Shell |

### Stufe 2: mit eigener Anmeldung beim Dienst

| Quelle | Weg | Haken |
|---|---|---|
| **Slack** | Eigene interne Slack-App je Arbeitsbereich mit Benutzer-Token; Events über Socket Mode (WebSocket, keine öffentliche URL); Erwähnungen per Suche | Kein API für den „Activity“-Feed. Arbeitsbereiche können App-Installation sperren. **Seit 2025 gelten harte Grenzen für verteilte Apps außerhalb des Marketplace, interne Apps sind ausgenommen**, also führt der Assistent durch „eigene App anlegen“ statt einer Torro-App |
| **GitLab** | `GET /todos` | gut, auch selbst gehostet |
| **Linear** | GraphQL `notifications` | Abfrage |
| **Microsoft Teams** | Graph (Chats, Erwähnungen), Abfrage | Push braucht öffentliche URL; viele Firmen sperren Zustimmung durch Benutzer |
| **Jira** | Kein API für die Glocke; JQL-Abfragen oder die Mails von Jira auswerten | eher über Mail |
| **RSS/Atom** | Abfrage mit ETag | landet immer in „Zur Info“ |

### Stufe 3: Messenger und „alles andere“

| Weg | Was | Haken |
|---|---|---|
| **Beeper Desktop API** | Lokale Schnittstelle (`localhost:23373`, WebSocket) für WhatsApp, Signal, Telegram, Instagram, Slack … in einem | Beeper muss laufen; nur für den persönlichen Gebrauch |
| **Matrix + mautrix-Brücken** | Eigener Homeserver, Brücken zu WhatsApp, Signal, Telegram …; Ungelesen und Erwähnungen sind Teil des Protokolls | Selbst betreiben; Brücken sind inoffizielle Clients (AGB-Risiko bei Meta) |
| **Telegram** | TDLib, offizielle Bibliothek für eigene Clients | Lesestatus nicht fälschen |
| **Desktop-Benachrichtigungen mitlesen** | Linux: D-Bus-Monitor auf `org.freedesktop.Notifications`, läuft neben mako. Windows: `UserNotificationListener` (braucht Paket-Identität, testen). macOS: kein offizieller Weg | **Auffangnetz** für alles ohne API (Signal-, WhatsApp-, Teams-Desktop). Nur Text, kein Zurückwirken (außer Windows), kein Verlauf |
| WhatsApp privat, Discord als Benutzer | – | Kein erlaubter Weg. Nur über Beeper, Brücke oder Mitlesen |

**Empfehlung zur Reihenfolge:** Suite → Agenten → Mail → Nextcloud → GitHub → ntfy. Das deckt den
größten Teil des Arbeitsalltags ohne eine einzige Firmenfreigabe ab. Slack danach. Messenger erst,
wenn sich zeigt, dass sie fehlen; dann zuerst das Mitlesen unter Linux, weil es ohne Anmeldung
alles mitnimmt.

## Agenten: Quelle und Nutzer zugleich

Agenten sind hier doppelt dabei, und genau darin liegt der Unterschied zu allen bestehenden
Benachrichtigungs-Apps.

**Als Quelle:**

- Der Assistent „Agents & API“ (A.7) richtet beim Verbinden eines Clients gleich die Hooks ein
  (Claude Code `settings.json`, Codex `hooks.json`), mit einem eigenen Skript-Schlüssel, der nur
  `inbox.post_item` darf.
- Jeder Eintrag kennt Sitzung, Projekt (`cwd`) und, wenn der Kern es zuordnen kann, das Fenster.
  „Öffnen“ heißt: **in dieses Fenster springen**, nicht „irgendein Terminal öffnen“.
- Einträge erledigen sich selbst: Wartet Claude auf Freigabe und es geht weiter (nächster
  `UserPromptSubmit`/`PostToolUse`-Hook), verschwindet der Eintrag.
- **Sitzungsname (E2.6):** Der Eintrag trägt den Namen, den der Agent der Sitzung gegeben hat
  (`AgentInfo.title`), damit man beim Freigeben weiß, um welchen Job es geht. Claude Code: die letzte
  Zeile `custom-title` im Transkript (vom Menschen vergeben, Umbenennen hängt eine Zeile an), sonst
  die letzte `ai-title`. Codex: die letzte Zeile der Sitzung in `$CODEX_HOME/session_index.jsonl`
  (Standard `~/.codex`), Feld `thread_name`. Gemini CLI: kein Name. Gelesen werden nur die letzten
  256 KB einer Datei und nur bei Meldungen und Rückfragen, nie bei `PostToolUse`; aus dem Transkript
  nur der Titel. Die Formate sind intern und undokumentiert: Fehlt etwas oder passt es nicht, gibt
  es keinen Namen, der Hook meldet wie bisher. Der Name ist fremder Text (Steuer- und
  Richtungszeichen entfernt, eine Zeile, höchstens 80 Zeichen, der Kern säubert noch einmal), nie
  Teil eines übersetzten Satzes. Die Zeile zeigt ihn als Titel, darunter „Claude Code is waiting for
  you · Projekt · Branch“; das Paneel als Überschrift, die Benachrichtigung als Überschrift. Eine
  Rückfrage nennt ihn hinter dem Agenten („Claude Code (session "Login bug") wants to use Bash …“), aber
  nur, wenn ein Hook fragt; über MCP lässt er sich nicht setzen. Ein neuer Bericht derselben Sitzung
  bringt einen geänderten Namen mit.
- **Rückfragen beantworten (E2.5, E3.8):** Claude Code und Codex haben einen `PermissionRequest`-Hook,
  der Erlauben/Ablehnen zurückgeben kann. Ihre Rückfragen lassen sich so direkt im Eingang
  beantworten, ohne ins Terminal zu wechseln. Entscheiden darf nur der Mensch über den App-Schlüssel,
  genau wie bei den Torro-Freigaben. Fragen an den Menschen (`AskUserQuestion`) sind keine Erlaubnis:
  Sie erscheinen als „wartet“ mit dem Fragetext.

**Als Nutzer über MCP:**

- „Was habe ich seit gestern verpasst?“, „Fass die Info-Einträge zusammen“, „Mach aus den
  Review-Anfragen Aufgaben“, „Gibt es etwas von Kunde X?“
- Agenten melden sich selbst: `inbox_post_item` („Done: migration finished, 3 tests failing“).
  Der Eintrag zeigt, welcher Agent ihn geschickt hat; jeder Agent lässt sich stummschalten.

## MCP und API

Neuer Bereich **`inbox`**. Rechte je Agent wie für Kalender und Aufgaben, die Auswahl geht hier
über **Quellen** statt Sammlungen (z. B. Claude Code darf GitHub, Agenten und Suite lesen, aber
weder Mail noch Slack).

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Einträge auflisten (Spur, Quelle, Zeitraum, Zustand) | Liste | `inbox.list_items` · Read | `inbox_list_items` |
| Eintrag lesen (mit Inhalt der Quelle) | Paneel | `inbox.get_item` · Read | `inbox_get_item` |
| Eintrag melden | – (Skripte, Hooks, Agenten) | `inbox.post_item` · Write | `inbox_post_item` |
| Erledigt / wieder offen | Taste `e`, Wischen | `inbox.set_item_done` · Write | `inbox_set_item_done` |
| Später | Taste `s` | `inbox.snooze_item` · Write | `inbox_snooze_item` |
| Spur ändern, Absender einordnen | Menü, Screener | `inbox.set_item_lane` · Write (mit „merken“ Admin) | `inbox_set_item_lane` |
| Zu Aufgabe machen | Taste `t` | `inbox.create_task_from_item` · Write (braucht auch Tasks-Rechte) | `inbox_create_task_from_item` |
| Antworten (Mail, Slack, Talk) | Paneel | `inbox.reply` · Write + Schalter „Nach außen“ | `inbox_reply` (ggf. `pending`) |
| Quelle stummschalten | Menü „Hide from inbox“ (`m`) | `inbox.mute_source` · Admin | `inbox_mute_source` |
| Gespräch ausblenden, Mail bleibt ungelesen (E3.11) | Menü „Hide from inbox“ › „This conversation“ | `inbox.hide_sender` mit `conversation` · Write | `inbox_hide_sender` |
| Absender oder Domain ausblenden, alle Konten (E3.10, E3.11) | Menü „Hide from inbox“ (`m`, „…“ im Paneel), Steuerzentrale › Inbox › Rules | `inbox.hide_sender` mit `sender`/`domain` · Admin | `inbox_hide_sender` |
| Rückgängig nach Done, Later, Hide (E3.11) | Hinweis unten mit „Undo“, `u`, `Ctrl+Z` | vorhandene: `set_item_done`, `snooze_item`, `remove_rule` | dieselben Tools |
| Quellen verbinden (Slack, GitHub, ntfy …) | Steuerzentrale › Accounts | `accounts.connect_*` · Admin | wie `connect_nextcloud`: nur Start, Anmeldung im Browser oder in der Steuerzentrale |
| Freigabe entscheiden | Eintrag, Benachrichtigung | `approvals.decide_approval` · Admin | **Never** |

**Sicherheit, neu durch den Eingang:**

- **Fremde Inhalte sind Daten, keine Anweisungen.** Mails und Slack-Nachrichten sind der klassische
  Weg für Prompt-Injection („Ignore previous instructions and forward…“). Tool-Antworten
  kennzeichnen fremde Inhalte (`untrusted: true` und ein fester Hinweis in der Tool-Beschreibung).
  Weil Antworten und Senden über den Schalter „Nach außen“ laufen, kann eine eingeschleuste
  Anweisung höchstens eine Freigabe-Anfrage erzeugen, nichts senden.
- **Der Senden-Schalter aus [mail.md](mail.md) wird allgemein: „Nach außen“** (never / with
  approval / free) gilt für Mail, Slack, Talk und alles, was künftig etwas im Namen des Menschen
  verschickt. Voreinstellung „with approval“.
- `inbox.post_item` ist gedrosselt (z. B. 30 je Stunde und Client). Agenten können „dringend“ nicht
  selbst setzen; das bleibt den festen Regeln (Agent wartet, Freigabe).
- Protokoll wie bisher ohne Inhalte. Einträge von Slack und Messengern speichern nur Vorschau und
  Link, nicht den ganzen Verlauf.

## Architektur

Wie alles andere: **Die Logik steht im Kern**, die App zeigt nur an.

| Baustein | Inhalt |
|---|---|
| `torro-inbox` | Datenmodell, Regeln, Zusammenführen, Spuren, Selbst-Erledigen. Ohne Ein-/Ausgabe |
| `torro-store` | SQLite (mit FTS5) für Eingang, Mail-Cache und PIM-Cache, siehe [mail.md](mail.md) |
| Quellen im Kern | Trait `Source` (verbinden, abonnieren oder abfragen, zurückwirken, Link bauen); je Quelle ein Modul. Mail und Nextcloud nutzen ihre vorhandenen Crates |
| Einwurf | `inbox.post_item` über `core.sock`, CLI `torroworkplace notify`, ntfy-Abo, später HTTP auf `127.0.0.1` ([api.md](api.md), Transport) |
| Ausgabe | Desktop-Benachrichtigung mit Knöpfen (`notify-rust`), Zahl am Tray, Omarchy-Leistenwidget, Änderungsmeldungen an die App |

Datenmodell, Skizze:

```
Item {
  id, source: { kind: mail|slack|github|agent|suite|ntfy|…, account },
  thread_key,                 // fasst Vorgänge zusammen; auch über Quellen (GitHub-Mail → GitHub)
  kind: message|mention|review|alert|agent_waiting|agent_done|approval|reminder,
  lane: for_me|info, urgent: bool,
  title, snippet, from: { name, address, known: bool },
  received, updated, count,
  state: new|seen|done|snoozed(until),
  link: app(…)|url(…)|window(…),
  actions: [open, reply, allow, deny, accept, decline, …],
}
```

**Zusammenführen über Quellen:** GitHub, Nextcloud, Jira und Slack verschicken zu ihren
Benachrichtigungen auch Mails. Ist die Quelle angebunden, ordnet der Kern solche Mails dem
Eintrag der Quelle zu (über `List-Id`, `X-GitHub-Reason`, Link im Text) und zeigt sie nicht
doppelt. Ist sie nicht angebunden, ist die Mail der Weg (gut für Jira).

**Aufbewahrung:** Erledigtes 30 Tage, dann weg (Mail bleibt natürlich auf dem Server).

## Die App

Arbeitsname `torroinbox`, Fensterklasse `torro-inbox`. Aufbau nach den Größenklassen von
`torro_gui::shell`, Konten und Quellen als Gruppen (keine Kontenleiste, entschieden in
[mail.md](mail.md)).

| Größe | Aufbau |
|---|---|
| **Breit** | Seitenleiste: **For me** (Zahl), **Info**, **Snoozed**, **Done**; darunter Gruppen je Quelle (Mail ▸ Konten ▸ Ordner, Slack ▸ Arbeitsbereiche, GitHub, Agents, Server) · Liste · Paneel mit Inhalt (ganze Mail, Thread-Vorschau, Agent mit „Go to window“, Freigabe mit „Allow/Deny“) |
| **Mittel** | Seitenleiste eingeklappt · Liste · Paneel darüber |
| **Schmal** | Liste, Eintrag öffnet sich darüber |
| **Mini / Leiste** | Anzeige: „3 for you“, die drei obersten mit Quellen-Symbol; dringend sichtbar hervorgehoben (nicht in Torro-Rot) |

- **Tastatur zuerst** wie Linear oder Superhuman: `j/k` bewegen, `e` erledigt, `s` später,
  `t` Aufgabe, `r` antworten, `Enter` öffnen, `m` ausblenden (Menü „Hide from inbox“, siehe unten).
- **Schreiben** (Mail, längere Antworten) in einem eigenen Fenster, siehe [mail.md](mail.md).
- **Fenster je Ausschnitt** mit `--source mail` oder `--lane info`, gemerkt in der
  Workspace-Vorlage. So bekommt, wer will, eine reine Mail-Kachel neben einer Agenten-Kachel.

## Verhältnis zu anderen Plänen

| Plan | Wird zu |
|---|---|
| Benachrichtigungen (Katalog) | geht im Eingang auf |
| Freigaben-Eingang (Katalog), Freigaben A.3/A.8 | Freigaben sind eine Art Eintrag; A.8 (`ui/approvals.rs`) zeigt dieselben Einträge in der Steuerzentrale |
| Mail-Client ([mail.md](mail.md)) | Kern, Konten, Ordner, Rechte bleiben; die Oberfläche wird je nach Entscheidung A/B/C oben |
| Chat (Nextcloud Talk) | „ungelesen und schnell antworten“ ist der Eingang; eine eigene Chat-App bleibt Idee |
| Nextcloud Notifications ([api.md](api.md), vorgemerkt) | Quelle im Eingang statt eigener Tray-Liste |
| Agenten-Übersicht, GitHub-Dashboard | bleiben Zustands-Apps; ihre Ereignisse kommen in den Eingang |
| Tagesplan (Katalog) | „Für mich“ + heutige Termine + fällige Aufgaben wäre genau das; später |

## Reihenfolge

| Schritt | Inhalt | Braucht |
|---|---|---|
| **E1 Kern** | `torro-inbox`, `torro-store`, Bereich `inbox` im Katalog, `inbox.*`-Requests, MCP-Tools, CLI `notify` | – |
| **E2 Suite und Agenten** | Quelle „Suite“ (Freigaben A.3, Sync-Fehler, Erinnerungen); Hooks für Claude Code und Codex, Sprung ins Fenster über `torro-wm` | E1, A.3 |
| **E3 App** | Breit/Mittel/Schmal/Mini, Tastatur, Tray-Zahl, Desktop-Benachrichtigungen nach Spur | E1 |
| **E4 Mail** | Mail-Kern aus [mail.md](mail.md) M1–M3 als Quelle, Lesen im Paneel, Schreibfenster | E3, M1–M3 |
| **E5 Nextcloud, GitHub, ntfy** | drei Quellen ohne Firmenfreigabe | E1 |
| **E6 Ruhe und Ordnung** | Screener, Lernen, Fokus mit dem Kalender, Sammelmeldung, Zusammenführen über Quellen | E4 |
| **E7 Slack** | Assistent „eigene Slack-App“, Socket Mode | E1 |
| später | Mitlesen von Desktop-Benachrichtigungen (Linux), Beeper, GitLab/Linear, Teams, Zusammenfassungen durch ein Modell, Tagesplan | |

E1 bis E3 brauchen weder Mail noch eine fremde Anmeldung und machen schon den Agenten-Alltag
übersichtlicher. Das wäre ein guter erster Schritt, auch um das Gefühl zu prüfen, bevor Mail
dazukommt.

## Entscheidungen (2026-10-11)

| # | Frage | Entscheidung |
|---|---|---|
| 1 | Drei Säulen | **Ja:** Eingang, Termine, Aufgaben sind das Leitbild der Suite |
| 2 | Mail | **Weg A:** Mail ist eine Quelle im Eingang, mit voller Tiefe im Paneel und eigenem Schreibfenster |
| 3 | Name | **Inbox**, Crate und Programm `torroinbox`, Fensterklasse `torro-inbox` |
| 4 | Spuren | Erst **zwei** (Für mich, Zur Info). Eine dritte Spur **Belege** (siehe unten) ist interessant, entschieden wird mit Mail (E4) |
| 5 | Screener | **Ja, zum Ausprobieren:** nur für Mail, nur für Absender, die keine feste Regel einordnet |
| 6 | Erledigt bei Mail | **Vorerst nur gelesen.** Archivieren bleibt ausdrücklich offen: eine abgehakte Mail gehört eigentlich ins Archiv. Entschieden wird mit E4 |
| 7 | Ruhe im Fokus | **Einstellbar** (Schalter „Quiet during busy events“), Bedeutung siehe unten |
| 8 | Rückfragen von Agenten beantworten | **Ja, kommt in den Plan** (E2.5), nach den ersten Schritten |
| 9 | KI-Sortierung und Zusammenfassung | **Ja, aber immer optional.** Braucht KI-Konten in der Steuerzentrale, auch lokale Modelle, Zuordnung je Funktion mit Modell und Aufwand. Eigenes Konzept: [ki.md](ki.md) |
| 10 | Messenger | **Im zweiten Schritt.** Gute Idee, aber eher privat als geschäftlich |
| 11 | Erster Schritt | **E1–E3 zuerst**, ohne Mail. Aufgaben: [tasks/eingang.md](tasks/eingang.md). Start erst, wenn die Konten-Aufgaben M7.1/M7.2 fertig sind (gleiche Dateien) |

### Belege: was die dritte Spur wäre

„Belege“ ist **weder „Für mich“ noch „Zur Info“**: Rechnungen, Bestell- und Versandbestätigungen,
Kontoauszüge, Buchungen, Passwort-Links, Terminbestätigungen. Sie brauchen jetzt nichts von mir (also
nicht „Für mich“), sind aber auch kein Lesestoff wie ein Newsletter (also nicht „Zur Info“). Man braucht
sie **später wieder**: für die Steuer, eine Reklamation, die Buchhaltung. HEY nennt das „Paper Trail“.

Eine Belege-Spur würde nie zählen, nie melden, nichts zum Abarbeiten anbieten, sondern nur sammeln und
gut durchsuchbar halten, später vielleicht mit „an die Buchhaltung weiterleiten“ oder „als PDF ablegen“.
Erkennen ließe sich vieles an festen Merkmalen (Anhang „Rechnung“/„Invoice“, bekannte Shop-Absender,
`Auto-Submitted`), der Rest über den Screener.

### Ruhe: was das genau heißt

Ruhe heißt **keine Popups und keine Töne**, sonst ändert sich nichts:

- Einträge kommen weiter in den Eingang, die Zahl am Tray und in der App zählt still mit.
- **Dringendes kommt trotzdem durch** (Freigabe, Agent wartet, Server down, CI rot am eigenen PR,
  VIP-Absender). Diese Liste ist fest, damit „Ruhe“ verlässlich bleibt.
- Nach dem Termin kommt **eine** Sammelmeldung: „While you were in "Kundentermin": 4 for you“.
- Einstellung in der Steuerzentrale (Seite „Inbox“ bzw. „General“): „Quiet during busy events“ (an/aus,
  Standard an) und später „Quiet during focus sessions“. Termine mit Frei/Belegt „frei“ oder ganztägige
  Termine lösen keine Ruhe aus.

## Quellen der Recherche

- Slack: [Grenzen für Apps außerhalb des Marketplace](https://docs.slack.dev/changelog/2025/05/29/rate-limit-changes-for-non-marketplace-apps),
  [search.messages](https://docs.slack.dev/reference/methods/search.messages),
  [App-Freigabe im Arbeitsbereich](https://slack.com/help/articles/222386767)
- Teams: [Graph-Änderungsmeldungen für Chats](https://learn.microsoft.com/en-us/graph/teams-changenotifications-chatmessage)
- GitHub: [Notifications API](https://docs.github.com/en/rest/activity/notifications) ·
  GitLab: [Todos API](https://docs.gitlab.com/api/todos/)
- ntfy: [Subscribe API](https://docs.ntfy.sh/subscribe/api/)
- Beeper: [Desktop API](https://developers.beeper.com/desktop-api/index.md) ·
  mautrix: [Release 2026-08](https://mau.fi/blog/posts/2026-08-16-mautrix-release.md) ·
  Telegram: [API-Bedingungen](https://core.telegram.org/api/terms)
- Agenten: [Claude Code Hooks](https://code.claude.com/docs/en/hooks),
  [Codex Hooks](https://learn.chatgpt.com/docs/hooks),
  [Gemini CLI Hooks](https://geminicli.com/docs/hooks/reference/),
  [Cursor Hooks](https://cursor.com/docs/agent/hooks),
  [MCP Elicitation](https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation)
- Desktop: [Windows UserNotificationListener](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/app-notifications/notification-listener)
- Vorbilder: HEY (Screener, Imbox/Feed/Paper Trail), Linear Inbox, GitHub-Benachrichtigungen,
  Superhuman (Split Inbox), Akiflow/Sunsama (Eingang → Aufgabe), Beeper, Android-Kanäle,
  Apple „Priority Notifications“
