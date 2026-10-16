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
   - Mail mit `List-Id`, `Precedence: bulk` oder `Auto-Submitted` → Zur Info. `List-Unsubscribe` allein reicht
     nicht: Dienste setzen es auch auf Mail an genau eine Person (Issue #128). Der Eintrag nennt den Grund
     („Info · Mailing list (List-Id)“, `Item.reason`), ebenso einen für den Absender gemerkten Weg und eine
     Stummschaltung nach Info.
   - Kanal-Nachrichten ohne Erwähnung, CI grün, Bots → Zur Info.
2. **Neue Absender fragt der Eingang einmal** (wie der „Screener“ bei HEY): „First mail from
   x@y.de: For me / Info / Block“. Die Antwort gilt dauerhaft für diesen Absender.
3. **Umsortieren lernt:** Schiebe ich etwas von Info nach Für mich (oder umgekehrt), fragt der
   Eingang „Always for this sender/channel?“.
4. **Optional, nur mit KI-Konto** ([ki.md](ki.md)): ein Modell schlägt eine Spur vor und schreibt
   eine Zeile Zusammenfassung. Nur als Vorschlag, mit Kennzeichnung; ohne KI-Konto fehlt die Funktion
   einfach, nichts anderes ändert sich.

**Ausblenden in Stufen (E3.10, E3.11):** „Hide“ ist kein Spam. Das Menü „Hide from Notification Hub“ (`m`, „…“
im Paneel, „Hide“ in der Aktionsleiste) bietet für Mail „This conversation“ (`inbox.hide_sender` mit
`conversation`: erledigt, aber anders als `e` bleibt die Mail ungelesen; eine neue Antwort bringt
sie zurück), „Everything from <Adresse>“, „Everything from <Domain>“, dann „Always to Info“ und – nur
wenn das Konto einen Ordner mit Rolle Junk hat – „Move to junk“; für andere Quellen „This entry“
und „Mute <Quelle>“, die Suite nie. Absender und Domain sind eine Regel (`inbox.hide_sender`): Neue
Mail davon wird kein Eintrag, offene und vertagte passende Einträge werden erledigt. Die Mail selbst
bleibt **unverändert** im Postfach, nicht gelesen, nicht verschoben – anders als „erledigt“, das
die Mail als gelesen markiert. Domain heißt der Teil nach `@`, klein, genau (keine Subdomains); die
Regel gilt für **alle** Mail-Konten (E3.11), auch Regeln, die vorher für ein Konto angelegt wurden.
Die Regeln stehen in der Steuerzentrale › Notification Hub › Rules („Hidden: news@shop.example“, „Hidden:
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
| **Nextcloud** | Benachrichtigungen (OCS) und Talk (ungelesen, Erwähnungen); Push über `notify_push`, sonst Abfrage | ja (löschen, gelesen) | Konto gibt es schon. Benachrichtigungen seit E5.8: je Konto alle 60 s mit ETag, ein Eintrag je Gegenstand (`<app>:<object_type>:<object_id>`), Erledigt löscht sie auf dem Server ([api.md](api.md), Fußnote 17). Talk seit E5.9 als eigene Quelle `talk:`: ein Eintrag je ungelesener Unterhaltung nach der Benachrichtigungsstufe von Talk, Zähler = ungelesene Nachrichten, Erledigt markiert sie gelesen |
| **GitHub** | `GET /notifications`, Grund je Eintrag (`review_requested`, `mention`, `ci_activity`, `agent_session_finished` …), Abfrage im Takt von `X-Poll-Interval` | ja („done“ = Thread löschen) | Seit E5.4 im Kern (`sources/github.rs`): je Konto, nie weiter zurück als 3 Tage vor der ersten Abfrage, `Last-Modified` im Lesestand; braucht klassisches Token mit `notifications` (Fine-grained: `health: missing_scope`, keine Einträge). Seit E5.6 gehört GitHubs Mail zum selben Issue oder PR zum GitHub-Eintrag: GitHub-Mail nur, wenn `From: notifications@github.com` mit `X-GitHub-Reason`, `List-Id` (Repo) und `Message-ID` (Repo und Nummer) zusammenpassen. Gibt es einen offenen GitHub-Eintrag dazu, wird die Mail kein eigener Eintrag und zählt nicht; sonst ein gewöhnlicher Mail-Eintrag, der beim Kommen des GitHub-Eintrags in ihn aufgeht (erledigt, als zusammengeführt, nicht als erledigt gezählt). Erledigt am GitHub-Eintrag macht den Thread „done“ und markiert diese Mails gelesen (Clients: auch Schreiben in `mail` und das Konto; bewusst so: das `forbidden` verrät einem Client ohne diese Rechte nur, dass Mail anhängt, nicht welche). Schlimmstenfalls hängt eine gefälschte Mail an einem vorhandenen GitHub-Eintrag desselben Repos und derselben Nummer; ohne solchen Eintrag verschwindet keine Mail |
| **ntfy / Gotify** | Themen abonnieren (JSON-Strom/WebSocket) | nein | **Universeller Eingang** für alles, was selbst melden kann: CI, Home Assistant, Uptime Kuma, cron, eigene Skripte. Löst auch „Webhook braucht öffentliche URL“: Webhooks gehen an ntfy, der Kern abonniert. Seit E5.13 im Kern (`sources/ntfy.rs`): je Konto ein JSON-Strom für alle Themen; die erste Verbindung holt nichts nach, danach `since` mit der letzten Id (bzw. der Zeit der ersten Verbindung) aus dem Lesestand; Wiederverbinden nach 1 s bis 5 min, nach 2 min ohne Daten neu; geändertes Thema → neu verbinden ab jetzt; stumm oder Konto entfernt → getrennt, Verpasstes bleibt draußen; 401/403 → `health: signed_out` bis zur neuen Anmeldung. Ein Eintrag je Thema und Titel (Vorgang nur als Prüfsummen, das Thema mit einem zufälligen Salz der Quelle, weil `thread_key` an Clients und entfernte Modelle geht und Themen-Namen Passwörter sind); Priorität 5 → `alert` (dringend, auch in Ruhe), 4 → für mich, 1–3 → Zur Info; Merkmale Etikett, Priorität, Tags; Link nur `click` mit `http`/`https`. Drossel: mehr als 60 neue Vorgänge je Stunde und Konto landen bis zum Ende der Stunde in „More from {topic}“ (Zur Info) |
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
| Quelle stummschalten | Menü „Hide from Notification Hub“ (`m`) | `inbox.mute_source` · Admin | `inbox_mute_source` |
| Gespräch ausblenden, Mail bleibt ungelesen (E3.11) | Menü „Hide from Notification Hub“ › „This conversation“ | `inbox.hide_sender` mit `conversation` · Write | `inbox_hide_sender` |
| Absender oder Domain ausblenden, alle Konten (E3.10, E3.11) | Menü „Hide from Notification Hub“ (`m`, „…“ im Paneel), Steuerzentrale › Notification Hub › Rules | `inbox.hide_sender` mit `sender`/`domain` · Admin | `inbox_hide_sender` |
| Rückgängig nach Done, Later, Hide (E3.11) | Hinweis unten mit „Undo“, `u`, `Ctrl+Z` | vorhandene: `set_item_done`, `snooze_item`, `remove_rule` | dieselben Tools |
| Verwerfen, nur Benachrichtigungen (E8.4) | ✕ oben rechts auf der Karte | vorhandenes `inbox.set_item_done` · Write | `inbox_set_item_done` |
| Was sich an einem Eintrag stummschalten lässt (E8.6a) | Glocke auf der Karte öffnet das Menü | `inbox.list_mute_options` · Read | `inbox_list_mute_options` |
| Stummschalten nach Merkmal, für eine Zeit (E8.6a) | Menü der Glocke, Steuerzentrale › Notification Hub › Rules | `inbox.mute_item` · Write nur für die Unterhaltung, sonst Admin | `inbox_mute_item` |
| Aufgabe aus einer Mail in einem Ordner (E8.7a) | „Task“ im Mail-Fenster | `tasks.create_todo_from_mail` · Write (prüft auch Leserecht auf das Konto) | `tasks_create_todo_from_mail` |
| Mail bzw. Aufgabe des Rückbezugs öffnen (E8.7a) | „Open mail“, „Open task“ | `apps.open_app` mit `target` · wie bisher | `apps_open_app` |
| Statistik (E8.8a) | Zentrale › Insights | `inbox.get_stats` · Read | `inbox_get_stats` |
| Ruhe-Status (E8.5b) | Zentrale rechts „Quiet“, in Schmal und Mittel eine Zeile unter den Zahlen bzw. Kacheln | `inbox.get_quiet` · Read (Termintitel nur mit Kalender-Leserecht) | `inbox_get_quiet` |
| Termin aus einem Eintrag (E8.5b) | „Event“ bei „To sort“, Paneel, Taste `c` | `inbox.create_event_from_item` · Write (braucht auch Kalender-Schreibrecht) | `inbox_create_event_from_item` |
| Einladung beantworten (E8.3b) | Mail-Fenster: Karte (Accept · Maybe · Decline · Task), Leser und Paneel (Karte des Termins), Smart view „Invitations“ | `mail.respond_to_invitation` · Write + „Nach außen“ (prüft auch Kalender-Schreibrecht, beim Ablehnen Löschrecht) | `mail_respond_to_invitation` |
| Quellen verbinden (Slack, GitHub, ntfy …) | Steuerzentrale › Accounts | `accounts.connect_*` · Admin | wie `connect_nextcloud`: nur Start, Anmeldung im Browser oder in der Steuerzentrale |
| ntfy: Konto, Anmeldung, Themen (E5.12) | Steuerzentrale › Accounts (E5.14) | `accounts.connect_ntfy`, `accounts.set_ntfy_login`, `accounts.set_ntfy_topic` · Admin | `accounts_connect_ntfy` (Client: öffnet nur die Steuerzentrale), `accounts_set_ntfy_topic`; `set_ntfy_login` **Never** (Geheimnis) |
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
| Quellen im Kern | Je Quelle ein Modul: Mail in `mail/inbox_feed.rs`, GitHub, Nextcloud, Talk und ntfy in `sources/<quelle>.rs` (E5.2, ohne gemeinsamen Trait). Alle legen über den Eingang ab (`Inbox::file_remote` mit den Kennungen des Servers, `resolve_remote`, `retire_remote`, Lesestand in `cursors`, Zustand `health`); Zurückwirken nach Erledigt, Aufgabe oder Termin über `sources::settle_done` |
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

> **Geändert am 2026-10-13:** Aus einer Liste werden drei Fenster, siehe [Drei Fenster](#drei-fenster-entschieden-2026-10-13).
> Dieser Abschnitt beschreibt den Stand bis E3.11.

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
  `t` Aufgabe, `r` antworten, `Enter` öffnen, `m` ausblenden (Menü „Hide from Notification Hub“, siehe unten).
- **Schreiben** (Mail, längere Antworten) in einem eigenen Fenster, siehe [mail.md](mail.md).
- **Fenster je Ausschnitt** mit `--source mail` oder `--lane info`, gemerkt in der
  Workspace-Vorlage. So bekommt, wer will, eine reine Mail-Kachel neben einer Agenten-Kachel.

## Drei Fenster (entschieden 2026-10-13)

Rückmeldung zur Übersicht aus E3.9: Sie sieht aus wie ein halbes Mailprogramm und ist zu tabellarisch. Wer viel
auf einmal sehen will, braucht Luft und Knöpfe, die man sieht. Entwürfe zum Durchgehen:
https://claude.ai/artifact/HLPBoyvbSbt4U2zofqgjxV. Aufgaben: [tasks/eingang.md](tasks/eingang.md), Schritt E8.

`torroinbox` bekommt **drei Fensterarten über denselben Einträgen im Kern**. Was in einem Fenster erledigt,
vertagt oder stummgeschaltet wird, gilt sofort in allen, denn der Zustand liegt nur im Dienst. Damit fällt der
Einwand gegen Weg B weg („zwei Orte, muss synchron sein“). Weg A bleibt im Kern gültig: Mail ist eine Quelle
im Eingang, nur die Oberfläche teilt sich auf.

| Fenster | Frage | Inhalt |
|---|---|---|
| **Mail** | Was ist reingekommen? | Nur Mail. Je größer das Fenster, desto mehr Desktop-Client (Konten, Ordner, Leser); je kleiner, desto mehr nur das Neue als Karten |
| **App Feed** („App-Feed“, bis #127 „Benachrichtigungen“) | Was melden meine Apps? | Alles außer Mail, immer mit dem Icon der App: Slack, WhatsApp, GitHub, Nextcloud, Kalender, ntfy, Skripte. Breit als Spalten je App oder nach Zeit. Jedes Fenster hat seinen eigenen Quellen-Filter (eins nur Slack, eins nur WhatsApp) |
| **Notification Hub** („Mitteilungszentrale“, bis #127 „Inbox“), die Zentrale | Was muss ich entscheiden oder weitergeben? | Kein Lesen, sondern Handeln: Freigaben, wartende Agenten, Sortieren (→ Aufgabe, → Termin, Later, Erledigt). Daneben, was weitergegeben wurde und was wiederkommt. Dazu **Insights** |

- **Ein Programm, drei Einträge.** Ein Code, ein Binary (`torroinbox --view mail|notifications|hub`), aber drei
  Einträge im App-Umschalter, in `apps.list_apps` und in den Workspace-Vorlagen, jeder mit eigener Fensterklasse,
  damit Kachel-Regeln sie auseinanderhalten. Die Filter `--source` und `--lane` aus E3.6 bleiben.
- **Agenten und Freigaben** zeigt die Zentrale. Im App Feed lassen sie sich als Quelle zuschalten.
- **Keine Links zu anderen Apps unten in der Seitenleiste.** Die anderen Fenster erreicht man über den
  App-Umschalter, das Tray und die Tastatur. Weniger zeigen, damit das Wichtige übersichtlich bleibt.

### Zwei Dichten

Jedes Fenster hat oben rechts **Compact | Comfortable**, gemerkt je Fenster und mit der Workspace-Vorlage
wiederhergestellt (`--density`).

- **Compact** ist der Stand bis E3.11: eine Zeile je Eintrag, Aktionen an der Zeile unter Maus oder Cursor. Gut
  für kleine Kacheln.
- **Comfortable** zeigt **Karten**: mehr Abstand, Vorschau über zwei bis drei Zeilen, Chips (Anhang, „Answer by
  Fri“), und **alle Knöpfe immer sichtbar**, nicht erst beim Überfahren. Vorbild für Mail ist Canary Mail. Lesetext
  im Leser 14/22 (neue Schriftstufe).
- Mini und Leiste bleiben Anzeigen wie bisher.

### Knöpfe je Eintrag

| Eintrag | Sichtbar | Im Menü |
|---|---|---|
| Mail (Mail-Fenster) | Reply · Task · Done · Later | Forward, Event, Hide from Notification Hub |
| Einladung | Accept · Maybe · Decline · Later · Task | – |
| Chat (Slack, WhatsApp, Talk) | ✕ · Reply · Task · Later · Glocke | Open in app |
| GitHub, Nextcloud, ntfy, Skripte | ✕ · Open · Task · Later · Glocke | – |
| Erinnerung aus Kalender oder Aufgaben | ✕ · Open · In 5 min | – |
| Freigabe | Allow · Deny · Go to window | – |
| Agent wartet oder ist fertig | Go to window · Later · Done | Mute agent |

- **Verwerfen (✕)** gibt es auf jeder Benachrichtigung: weg, ohne etwas zu tun. Es wirkt wie Erledigt
  (`inbox.set_item_done`, auch das Zurückwirken auf die Quelle), heißt nur anders. **Nicht im Mail-Fenster**: Mails
  bleiben dort vollständig, sie werden erledigt oder ausgeblendet.
- **Later** (vertagen) gibt es überall, **Task** auf jeder Nachricht und jeder Mail, auch in Ordnern.
- **Freigaben** haben weder ✕ noch Later: Sie warten auf Ja oder Nein, das darf nicht versehentlich verschwinden.
- **Go to window / Open** holt das Fenster nach vorn und aktiviert es: bei Agenten das Terminal der Sitzung (E2.4,
  am Mac E8.9), bei App-Quellen das Fenster der App.

### Stummschalten nach Merkmalen

Die Glocke auf jeder Benachrichtigung öffnet ein Menü. Was darin steht, baut der **Kern** aus den Merkmalen des
Eintrags (`inbox.list_mute_options`), damit Oberfläche und Agenten dieselben Möglichkeiten sehen. Danach wählt man
**wie lange** (1 Stunde, bis morgen, immer) und **was passiert**:

- **No popup:** Der Eintrag kommt, aber ohne Popup und Ton.
- **Only Info:** Er kommt unter „Zur Info“ und zählt nicht.
- **Hide:** Er wird gar nicht erst Eintrag (wie `mute_source` und `hide_sender` heute).

Welche Merkmale eine Quelle mitbringt, und damit, wonach sich verlässlich stummschalten lässt:

| Quelle | Merkmale |
|---|---|
| Mail | Konto, Absender, Domain, Liste (`List-Id`), Massenmail (`Precedence`, `Auto-Submitted`), Unterhaltung |
| Slack (E7) | Arbeitsbereich, Kanal, Thread, Person, Art (Direktnachricht, Erwähnung, Kanal), Bot |
| GitHub (E5) | Repo, Grund (`review_requested`, `mention`, `ci_activity` …), Art (Pull Request, Issue, Prüflauf), Ergebnis (bestanden, fehlgeschlagen) |
| Nextcloud (E5) | App (Dateien, Talk, Kalender …), Person |
| ntfy (E5) | Thema, Priorität, Tags |
| Agenten | Agent, Projekt, Art (wartet, fertig) |
| Mitgelesene Desktop-Benachrichtigungen (WhatsApp und andere) | **nur** App, Titel (Chat oder Person) und Text, also grober |

Freigaben und Einträge der Suite lassen sich nie stummschalten (Sicherheitsregel 1). Alle Regeln stehen in der
Steuerzentrale › Notification Hub › Rules mit „Remove“; zeitlich begrenzte laufen von selbst ab.

**Kern (E8.6a, umgesetzt):** Der Dienst setzt die Merkmale beim Melden (`Item.facets`, nie vom Absender): Mail
Konto, Absender, Domain, Liste (`List-Id`), Massenmail; Agenten Agent, Projekt (nur von Hooks), Art; Skripte der
Client. Das **Thema** von Skripten fehlt noch: `inbox.post_item` hat kein Feld dafür. Die Unterhaltung ist kein
Merkmal am Eintrag, sondern der eigene Umfang `conversation`. Eine Regel mit Merkmal gilt für alle Quellen derselben
Art (ein Absender in allen Mail-Konten, ein Projekt für alle Agenten), eine für die Unterhaltung nur in ihrer
Quelle. „Hide“ und „Only Info“ für die Unterhaltung ohne Ablauf legen keine Regel an, sondern erledigen bzw.
verschieben nur diesen Eintrag. Befehle: [api.md](api.md), Fußnote 11.

**Mail-Fenster (Issue #123, E8.25):** Mail hat **ein** Menü, die Glocke „Mute…“ – „Hide from inbox“ ist darin
aufgegangen. Die Glocke steht in Zeilen und Karten (statt „…“), im Leser und im Paneel, `m` öffnet sie; für Mail-Einträge
in jedem Fenster. Dauer: 1 Stunde, bis morgen, 1 Woche, immer. Was „Hide from inbox“ bot, steht so darin: „This
conversation“ mit „Hide“ und „Always“ erledigt den Eintrag (die Mail bleibt ungelesen), Absender und Domain sind
Merkmale, „Always to Info“ ist Absender mit „Only Info“; „Move to junk“ bleibt als letzter Punkt. Auch Nachrichten
eines Ordners ohne Eintrag haben die Glocke: `inbox.mute_item` mit der Kennung der Mail (`m:…`), angeboten werden
nur ihre Merkmale (Absender, Domain, Liste, Konto, Massenmail), nicht die Unterhaltung.

### Aufgabe mit Rückbezug

Eine Aufgabe aus einer Nachricht behält ihre Herkunft, auch in Nextcloud. Gespeichert wird in der Eigenschaft
`X-TORRO-LINK` der Aufgabe (gibt es seit P.2), aber mit einer **festen Kennung der Quelle** statt der Kennung des
Eintrags: Einträge löscht der Kern 30 Tage nach dem Erledigen, die Kennung der Quelle bleibt.

| Quelle | Kennung in `X-TORRO-LINK` |
|---|---|
| Mail | `mail:<konto>:<Message-ID>`; die Message-ID bleibt, auch wenn die Mail den Ordner wechselt |
| GitHub | `github:<owner>/<repo>#<nummer>` (gibt es schon) |
| Slack (E7) | `slack:<arbeitsbereich>:<kanal>:<ts>` |
| alles andere | `inbox:<eintrag>` wie bisher (hält, solange der Kern den Eintrag kennt) |

Dazu schreibt der Kern immer eine **lesbare Zeile** in die Beschreibung („From mail: Anna Keller, “Angebot
Dachsanierung”, 13 Oct“), damit die Herkunft auch in Nextcloud Tasks und anderen Programmen sichtbar ist. Ob andere
Programme `X-TORRO-LINK` beim Bearbeiten stehen lassen, wird getestet (Nextcloud Tasks, Apple Erinnerungen);
verliert eines die Eigenschaft, bleibt die Zeile.

Weil der Kern beide Seiten kennt, kann er Bezug nehmen:

- Die **Mail** zeigt ihre Aufgabe (Titel, Liste, fällig, offen/erledigt) mit „Open task“.
- Die **Aufgabe** zeigt ihre Mail mit „Open mail“ und neue Antworten darauf, seit sie angelegt wurde.
- Wird die Aufgabe **erledigt**, fragt die Zentrale: „You finished … Anna is waiting for an answer“ mit „Reply“
  und „Mark mail done“.

**Kern (E8.7a, umgesetzt):** Die Kennung ist `mail:<adresse>:<Message-ID>`, also die feste Konto-Id
`mail:<adresse>` aus `accounts.list_accounts` (dieselbe wie in den Rechten und in der Quelle des Eintrags)
und die Message-ID ohne spitze Klammern, wie der Mail-Speicher sie hält. Gefunden wird die Mail über einen
Index auf der Message-ID in jedem Ordner. Ein Mail-Eintrag zeigt die Aufgabe, die auf eine seiner Mails oder
auf eine Mail derselben Unterhaltung zeigt; so bleibt der Bezug, auch wenn die erste Mail verschoben wurde
und der Eintrag nur noch die Antwort hält. Die Aufgabe zeigt ihre Mail mit Betreff, Absender, Datum und den
Antworten, die danach kamen (die eigenen nicht mitgezählt). „You finished a task from a mail“ prüft die
Routine der Suite jede Minute gegen den Aufgaben-Cache; wer erledigt hat (Suite, Nextcloud, ein Agent), ist
gleich. Gemeldet wird einmal je Aufgabe (`task_notes` in `inbox.sqlite`), nur wenn die Unterhaltung noch
offen im Eingang liegt, sonst nur vermerkt. Die Knöpfe „Reply“ und „Mark mail done“ baut E8.7b aus
`task.link` und `task.mail_entry` des Eintrags. `apps.open_app` mit Ziel startet immer ein neues Fenster,
weil ein laufendes Fenster noch keinen Weg hat, sich etwas anderes zeigen zu lassen. Befehle:
[api.md](api.md), Fußnoten 12 und 13.

### Insights

Die Zentrale zählt, damit Optimieren nicht beim Gefühl bleibt: je Tag, Quelle und Merkmal, was kam, was geöffnet,
verworfen, erledigt, vertagt oder zur Aufgabe wurde, wie viele Popups es gab und wie lange es bis zum Erledigen
dauerte. „Geöffnet“ zählt höchstens einmal je Eintrag und Tag, über „Open“ oder indem man die Mail im Mail-Fenster
liest. Ansicht für Woche, Monat und Quartal: Nachrichten je Tag nach Quelle, Stoßzeiten, und **„Where you could cut
back“** mit einem fertigen Vorschlag je Zeile („Mute channel“, „Only failed checks“, „Hide sender“).

- Gezählt wird **nur lokal** im Kern, in Tagessummen, höchstens 13 Monate. Keine Titel, keine Texte; Absender und
  Kanäle nur als Merkmal einer Zeile. Nichts davon kommt ins Protokoll.
- Agenten lesen die Zahlen über `inbox_get_stats` („Wo lohnt sich Aufräumen?“), aber nur für Quellen, die sie sehen
  dürfen.

**Kern (E8.8a, umgesetzt):** Die Tabelle `stats` in `inbox.sqlite` hält Tagessummen je lokalem Tag, Quelle und Merkmal.
Gezählt werden Meldungen (neue Einträge und zusammengeführte, z. B. die zweite Mail einer Unterhaltung), Geöffnetes
(`open_item` auf einem neuen Eintrag), Verworfenes, Erledigtes (von Hand, durch `resolve_items`, eine entschiedene
Freigabe, ein repariertes Konto oder eine anderswo gelesene Mail), Vertagtes, zur Aufgabe Gemachtes, Stummgeschaltetes
(Meldungen, die eine Regel verwirft, nach „Info“ legt oder ohne Popup lässt, und Einträge, die Ausblenden oder
Stummschalten schließt bzw. verschiebt) und Popups, davon die in Ruhe zurückgehaltenen. Dazu Meldungen je Stunde und
die Zeit bis erledigt in Stufen, aus denen der Median kommt. Merkmale, die die Quelle selbst sind (Konto, Agent,
Client), zählen nicht eigens. Eine Mail, die eine Regel fernhält, zählt einmal, auch wenn der Abgleich sie nach einem
verpassten Wechsel noch einmal anbietet. Vorschläge: ab zehn Meldungen im Zeitraum, höchstens jede fünfte geöffnet,
summiert über alle Quellen derselben Art (wie die Regel wirkt), höchstens fünf, nicht für schon stummgeschaltete
Merkmale. Jede Regel zählt außerdem, was sie gefangen hat, je Quelle; `inbox.list_rules` nennt es als `hits`.
Befehle: [api.md](api.md), Fußnote 14.

### Später für jede Mail (E9.1, entschieden 2026-10-15)

Im Mail-Fenster kann **jede** Mail auf später gelegt werden, auch eine ohne Eintrag und eine schon gelesene
(`inbox.snooze_item` mit `m:…`). Der Kern legt dann zuerst einen Eintrag an, wie der Abgleich neue Mail ablegt, und
stellt ihn zurück; hat die Unterhaltung schon einen Eintrag, kommt die Mail dort hinein. Zurückgestellt ist immer die
ganze Unterhaltung, so wie sie im Eintrag abgelegt ist.

- **Aus „All“ heraus:** `mail.search_messages` mit `snoozed: false` lässt jede Mail weg, die in einem gerade
  zurückgestellten Eintrag abgelegt ist; `snoozed: true` zeigt nur diese („Later“). Der Kern fragt dafür bei jeder Suche
  den Eingang (`Inbox::mail_later`) und nimmt die Mails über ihre Kennung schon in die Abfrage jedes Kontos auf
  (`SearchQuery::only`, `except`). So bleiben die Seiten voll und der Cursor (`c:<n>`, ein Versatz) stimmt.
- **Zurück und oben:** Wann ein Eintrag zurückkam, hält die Tabelle `woken` in `inbox.sqlite` fest (Schema 10): beim
  Aufwachen zur gewählten Zeit und wenn neue Mail die Unterhaltung zurückholt. Bei `snoozed: false` holt die Suche diese
  Mails eigens dazu, egal wie alt sie sind, und sortiert sie nach dem Zeitpunkt der Rückkehr statt nach ihrem Datum. So
  stehen sie oben in „All“, als wären sie eben angekommen. Jede Mail einer Suche trägt dann `back` (die Oberfläche zeigt
  „Back from later“). Ohne `snoozed` und bei `sort: relevance` bleibt die Reihenfolge wie bisher, `back` ist trotzdem
  gesetzt.
- **Bis wann zurück:** Bis die Mail im Mail-Fenster gelesen wird (`Inbox::mail_read`) oder der Eintrag erledigt bzw.
  wieder zurückgestellt ist. Danach steht sie wieder nach ihrem Datum.
- **Gelesen heißt nicht erledigt:** Einen zurückgestellten Eintrag erledigt der Abgleich erst, wenn keine seiner Mails
  mehr im Posteingang liegt (verschoben oder gelöscht). Dass sie gelesen ist, reicht nicht: Wer eine Mail auf später
  legt, hat sie oft schon gelesen.
- **Neue Mail holt zurück (Entscheidung 8 in E9):** Kommt in einer zurückgestellten Unterhaltung neue Mail an, ist sie wieder
  offen (`rules::merge_mail`), in „All“ oben mit `back` zur Zeit der neuen Mail und ebenso in der Zentrale. Eine ältere
  Mail, die erst später abgelegt wird, lässt die Unterhaltung zurückgestellt. Andere Quellen bleiben bei `rules::merge`:
  meldet etwa ein Agent dieselbe Sache noch einmal, bleibt sie zurückgestellt.
- **Regel verbirgt die Mail:** Dann gibt es keinen Eintrag zum Zurückstellen, die Antwort ist `unsupported`.

### Später, vorgemerkt

- **An einen Agenten geben:** ein Eintrag (Review, Kundenanfrage) geht als Auftrag an einen Agenten. Die Idee ist
  gut, wird aber eigens ausgearbeitet: welcher Agent, wie er startet, welche Rechte er dafür bekommt, wie das
  Ergebnis zurückkommt. Eigener Request, nie ohne Freigabe.
- **Fokus-Profile** wie der Fokus bei Apple: Profile („Work“, „Deep work“, „Evening“), wann sie angehen (Zeit,
  Kalendertermin, Workspace-Vorlage, von Hand), was durchkommt, was bis zum Ende wartet, eine Sammelmeldung danach.
  Erweitert „Ruhe“ (unten). Freigaben, wartende Agenten und „Server down“ kommen in jedem Profil durch.
- **Tastenkürzel im Mail-Fenster** (2026-10-15): Vorlage wählen (Torro, Apple Mail, Outlook, Thunderbird, Gmail) und
  frei belegen, in den Einstellungen des Mail-Fensters. Vorgabe je System: macOS Apple Mail, Linux Thunderbird, Windows
  Outlook; jede Vorlage geht auf jedem System. Die Torro-Belegung kommt zuerst, dann die drei Vorlagen als Vorgabe; Wahl
  und eigene Tasten warten auf die Einstellungen im Mail-Fenster (S.20)
  ([Aufgaben E10](tasks/eingang.md#e10-tastenkürzel-im-mail-fenster-vorgemerkt-2026-10-15)).
- **Slack und WhatsApp** als Quellen: Slack mit E7, WhatsApp über das Mitlesen von Desktop-Benachrichtigungen (unter
  Linux, unter macOS gibt es keinen offiziellen Weg) oder Beeper.

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
| **E8 Drei Fenster** | Mail, Benachrichtigungen, Zentrale; zwei Dichten; Verwerfen, Stummschalten nach Merkmal, Aufgabe mit Rückbezug, Insights, „Go to window“ am Mac | E3, E4 |
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

### Entscheidungen (2026-10-13)

| # | Frage | Entscheidung |
|---|---|---|
| 12 | Aufteilung | **Drei Fenster:** Mail, Benachrichtigungen, Inbox als Zentrale, aus einem Programm (siehe [Drei Fenster](#drei-fenster-entschieden-2026-10-13)). Ändert Weg A (Nr. 2) nur in der Oberfläche |
| 13 | Dichte | **Zwei Ansichten je Fenster:** kompakt wie bisher und luftig mit Karten und immer sichtbaren Knöpfen |
| 14 | Verwerfen | **Auf jeder Benachrichtigung**, nicht im Mail-Fenster, nicht bei Freigaben |
| 15 | Aufgabe | **Aus jeder Nachricht und jeder Mail**, der Bezug bleibt erhalten, auch in Nextcloud |
| 16 | Stummschalten | **Nach Quelle, Absender, Unterhaltung, Kanal oder Art**, für eine Zeit oder immer |
| 17 | Seitenleiste | **Keine Links zu anderen Apps** unten in der Seitenleiste |
| 18 | Insights | **Statistik in der Zentrale**, um gezielt zu optimieren |
| 19 | Später | **An einen Agenten geben** und **Fokus-Profile** werden eigens ausgearbeitet |
| 20 | Namen | ~~„Torro Inbox“, „Torro Inbox Mail“, „Torro Inbox Notifications“~~, abgelöst durch Nr. 21. TorroMail bleibt ein eigenes Produkt (eigene Konten, eigener MCP-Server); damit niemand das Mail-Fenster der Suite für TorroMail hält, heißt es lang „Torro Workplace Mail“ |
| 21 | Namen und Erkennung (2026-10-14, Issue #127) | „Inbox“ und „Mail“ verwechselte man. Die Fenster heißen jetzt **„Notification Hub“** (de „Mitteilungszentrale“, lang „Torro Notification Hub“), **„Mail“** (de „E-Mail“, lang „Torro Workplace Mail“) und **„App Feed“** (de „App-Feed“, lang „Torro App Feed“). Das Benachrichtigungen-Fenster bekam einen neuen Namen, weil „Notification Hub“ und „Notifications“ sich wieder ähnelten; die drei Namen teilen kein Wort. Wo die Oberfläche den ganzen Eingang meint (Steuerzentrale, Rechte, Freigaben, Assistent), steht „Notification Hub“; „Inbox“ heißt nur noch der Mail-Ordner. Jedes Fenster trägt oben links statt des Suite-Logos sein Zeichen: sein Icon (Tray, Brief, Glocke) in eigener Farbe auf getönter Kachel (`shell::app_mark`; Zentrale orange, Mail blau, App Feed pink), in Schmal (ohne Seitenleiste und Leiste) vorn in der Kopfleiste. Die erste Ansicht des Mail-Fensters („For me“) hat `UserCheck` statt des Trays. Auswahl in der Seitenleiste bleibt neutral (Design-Regel). Unverändert: Fensterklassen, `AppId`-Kennungen, `--view`-Werte und der API-Bereich `inbox` |

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
- Ob gerade Ruhe gilt und bis wann, sagt `inbox.get_quiet` (E8.5b): Die Zentrale zeigt es rechts als
  „Quiet“ („Until 15:00“, darunter der Termin), wo diese Spalte fehlt, als Zeile „Quiet until 15:00 ·
  Kundentermin“ unter den Zahlen bzw. Kacheln. Den Titel des Termins bekommen Agenten nur mit Leserecht auf
  den Kalender, sonst steht dort „A busy event“.

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
