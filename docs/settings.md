# Einstellungen: die Steuerzentrale

Stand: 2026-10-14. **Konzept zur Freigabe**; die Einstellungen in den Apps (unten) sind am
2026-10-14 entschieden.

Alles, was die Suite als Ganzes betrifft, wird an **einer** Stelle eingestellt: in der
Steuerzentrale des Kerns (`torroworkplace ui`). Die Apps (Kalender, Aufgaben, später weitere)
haben keine eigenen Kontoeinstellungen. Fehlt ihnen ein Konto, zeigen sie einen Hinweis mit dem
Knopf „Open Settings“, der die Steuerzentrale an der richtigen Stelle öffnet.

Was nur eine App betrifft (welche Kalender sie zeigt, die Bots des Assistenten, die Regeln des
Eingangs …), stellt man **in der App selbst** ein: Das Zahnrad zeigt die Einstellungen dieser App im
Inhaltsbereich desselben Fensters, ohne eigenes Fenster und ohne Dialog darüber. Siehe
[Einstellungen in den Apps](#einstellungen-in-den-apps).

Vorbild sind die Einstellungen von **TorroWhisper** (Aufbau, Seiten, Speichern) und
**TorroMail** (Konten, Anbieter-Katalog, Einrichtungsassistent, Zugriff für KI-Clients). Beide
folgen bewusst demselben Muster; die Steuerzentrale übernimmt es, nur mit Iced statt SwiftUI.

## Fenster und Navigation

- Eigener Prozess `torroworkplace ui`, Iced, App-ID `torro-workplace`. Spricht über IPC mit dem
  Dienst; der Dienst bleibt leicht. Geöffnet aus dem Tray („Settings…“), aus den Apps oder mit
  `torroworkplace ui --section accounts`.
- **Links eine Seitenleiste** (220 px) mit Abschnitten als Symbol + Wort; unten leise
  „Torro Workplace · Version x.y“. **Rechts die Seite** des Abschnitts. Keine Reiter.
- Jede Seite besteht aus **Gruppen**: Überschrift, Zeilen, darunter eine Fußzeile, die in einem
  ganzen Satz die Folgen erklärt („Applies to all apps of the suite.“).
- **Schmale Kachel** (< 640 px): Die Seitenleiste wird zur Liste; ein Abschnitt öffnet sich als
  eigene Seite mit Zurück-Knopf. Mindestgröße 360×320 wie alle Apps.
- **Speichern automatisch**, kurz verzögert (500 ms), kein Speichern-Knopf. Ausnahmen sind
  Vorgänge mit Prüfung (Konto einrichten, Schlüssel eintragen): erst nach erfolgreicher Prüfung
  wird gespeichert.
- **Destruktives wird bestätigt**, mit Frage als Titel, Folge als Satz und einem Knopf mit Verb
  („Remove account“), daneben „Cancel“. Der abgedunkelte Dialog aus `torro_gui::widgets::modal`.
- **Leere Bereiche** sagen in einem Satz, was hier erscheinen wird und was zu tun ist.
- **Status nur bei Abweichung**, immer als Symbol + Wort: grün verbunden, orange wartet auf dich,
  grau nicht eingerichtet, rot kaputt (mit dem Satz des Servers). Torro-Rot bleibt Marke.

## Abschnitte

| Abschnitt | Inhalt | |
|---|---|---|
| **Overview** | Auf einen Blick: Dienst läuft, Tiling-Manager erkannt, Konten (Zahl, Probleme), letzter Sync, „Needs your attention“ mit Konten, die eine neue Anmeldung brauchen | v1 |
| **Accounts** | Liste der Konten als Karten, „Add account“, Detailseite je Konto (siehe unten) | v1 |
| **Workspaces** | Vorlagen: umbenennen, löschen, Tastenplatz F1–F10, beim Anmelden laden; Strategie je Fenster | v1 (Liste, Umbenennen, Löschen), Rest später |
| **Apps** | Die Apps der Suite: installiert ja/nein, öffnen, App-ID für Fensterregeln | v1 |
| **General** | Sprache (System / English / Deutsch), Start beim Anmelden, Tray-Icon zeigen; unter „Appearance“ hell/dunkel, Zoom und das Design-Paket ([design-pakete.md](design-pakete.md), L.4) | v1 |
| **Agents & API** | Agenten und Skripte verbinden (wie TorroMail: Karte je Client, eigener Schlüssel), Rechte je Bereich und Sammlung, Freigaben. Konzept: [api.md](api.md) | später |
| **Updates** | Automatisch prüfen, „Check for updates now“, zuletzt geprüft | später |
| **Diagnostics** | Log-Datei, Sync-Protokoll, Verbindung testen | später |
| **Help** | Kurzanleitung, Tastenkürzel, Links | später |

Mit den Einstellungen in den Apps (unten) bekommt die Seitenleiste zwei Gruppen: **Suite**
(Overview, Accounts, Agents & API, Approvals, Workspaces, Appearance, General) und **Apps**
(Assistant, Calendar, Tasks, Inbox, Mail). Ein Eintrag unter „Apps“ zeigt dieselben Seiten wie das
Zahnrad in der App, dazu oben „Open“ und ob die App installiert ist; die heutige Seite **Apps**
geht darin auf.

## Einstellungen in den Apps

Entschieden am 2026-10-14 (Mockup im Gespräch: der Assistent).

Warum: Heute öffnet das Zahnrad jeder App die Steuerzentrale als eigenes Fenster, immer auf
„Accounts“. Was man in der App sucht, steht dort weit weg: die Kalender in der Detailseite jedes
Kontos, die Quellen des Eingangs verteilt auf vier Kontoseiten, die Seite Assistant mit 14 Gruppen
untereinander. Die Nacht des Kalenders steht an zwei Stellen.

### Wie es aussieht

```
┌ Seitenleiste der App ┐┌ Inhalt ──────────────────────────────────────────────────────┐
│ + Neuer Chat         ││ ← Settings › General                                         │
│ Heute                │├────────────────┬─────────────────────────────────────────────┤
│  Wochenplanung       ││ General        │ Status                                      │
│  Mail an Jonas       ││ Profile        │  ● On                     [Turn off…]       │
│ Bots                 ││ Bots           │ New chats                                   │
│  Release-Bot         ││ Access         │  Engine · Model · Effort                    │
│                      ││ Voice          │ Engines                                     │
│                      ││ History        │  Anthropic ● connected                      │
│                      ││ ────────────── │                                             │
│                      ││ Appearance     │                                             │
│                      ││ ↗ All settings │                                             │
└──────────────────────┘└────────────────┴─────────────────────────────────────────────┘
```

- **Das Zahnrad schaltet den Inhaltsbereich um**: statt Chat, Woche oder Liste die Einstellungen
  der App. Kein eigenes Fenster, kein abgedunkelter Dialog. Die Seitenleiste der App bleibt stehen;
  ein Klick dort (ein Chat, ein Kalender, eine Liste) schließt die Einstellungen und öffnet ihn.
- **Kopfleiste** (44): „← Settings › <Abschnitt>“. Der Pfeil, Esc oder ein zweiter Klick aufs
  Zahnrad führen zurück dorthin, wo man war. Das Zahnrad ist hervorgehoben, solange die
  Einstellungen offen sind.
- **Links im Inhalt die Abschnitte** als ruhige Liste (Zeilen wie in der Seitenleiste, ohne
  Rahmen), rechts die Seite. Eine Seite besteht aus Gruppen wie in der Steuerzentrale (Überschrift,
  Zeilen, erklärender Satz). Unterseiten (ein Bot, das Gedächtnis) öffnen sich an derselben Stelle,
  der Pfad wird länger: „← Settings › Bots › Release-Bot“.
- **Unten in der Liste**: „Appearance“ (in jeder App gleich, mit dem Satz „Applies to all apps of the
  suite.“) und „All settings ↗“, das die Steuerzentrale beim Eintrag dieser App öffnet.
- **Kleiner werden** wie überall (Design-Skill, Abschnitt 0): Mittel zeigt die Abschnitte als Menü
  in der Kopfleiste („General ▾“), Schmal als Liste, aus der ein Abschnitt als eigene Seite mit
  „‹ Back“ aufgeht.
- **Direkt öffnen**: `--settings <abschnitt>` in jeder App (auch mit `--demo`, für die
  Sichtprüfung). Hinweise in der App, die heute die Steuerzentrale öffnen („Change access“ unter
  einer abgelehnten Antwort, „Access…“ eines Bots, „All rules“ im Eingang), öffnen künftig den
  passenden Abschnitt im selben Fenster. Konten einrichten bleibt in der Steuerzentrale.

### Was wohin gehört

| App | Abschnitte | kommt heute von |
|---|---|---|
| **Assistant** | General (an/aus, neue Chats: Engine, Modell, Denkstufe; Liste der Engines) · Profile (Profil und Gedächtnis) · Bots (je Bot eine Unterseite: was er benutzen darf) · Access (was der Assistent benutzen darf, Freigabe, Auswahl der Sammlungen) · Voice (Sprechen, Mikrofon, Programme auf diesem Rechner) · History (Unterhaltungen aufheben) | Seite Assistant, aufgeteilt; Bot-Seite |
| **Calendar** | Calendars (welche die Suite zeigt, je Konto gruppiert) · Night | Kontodetail „Calendars and lists“; General › Calendar (die Karte am Mond in der Zeitleiste bleibt als schneller Weg) |
| **Tasks** | Lists (welche die Suite zeigt, je Konto gruppiert) | Kontodetail „Calendars and lists“ |
| **Inbox** | Notifications (still bei Terminen) · Sources (je Konto: Nextcloud-Benachrichtigungen und Talk, GitHub, ntfy-Themen, „Show new mail in Inbox“) · Rules | Seite Inbox; Abschnitte „Inbox“ und „Topics“ der Kontoseiten |
| **Mail** (im Fenster des Eingangs) | Sender (Absendername, Signatur je Konto) · Folders (offene Ordnerfragen) | Kontodetail Mail |

In der Steuerzentrale bleiben: Konten (Status, Verbindung, welche Dienste ein Konto liefert,
Entfernen), Agents & API, Approvals, Workspaces, General (Sprache, Start beim Anmelden). Die
Kontodetailseiten werden dadurch kurz; wo eine Einstellung umzieht, steht dort eine Zeile, die zum
neuen Ort führt („Calendars and lists are chosen in Calendar and Tasks.“): Sie schlägt den Eintrag der
App unter „Apps“ in der Steuerzentrale auf.

Was im Fenster gewählt wird, gilt für die ganze Suite (wie heute in der Steuerzentrale). Die
spätere Auswahl **pro Fenster** (unten) kommt als eigener Schalter dazu, nicht als Umbau.

### Technik

- Die Seiten ziehen aus dem Programm `torroworkplace` in eine neue Bibliothek
  `crates/torro-settings-ui` um: die Seiten, die Zeilen-Bausteine (`setting_row`, `pressable`,
  `chevron`, `dialog`, `confirm`) und ein **Host** für den Inhaltsbereich (Abschnittsliste,
  Kopfleiste mit Pfad, Seite, Laden und Senden). Die Steuerzentrale und die Apps zeigen dieselben
  Seiten; nichts wird doppelt gebaut.
- Seiten bleiben wie heute: Sie geben eine `Action` zurück, der Host schickt den Request und lädt
  neu. Jede App lädt beim Öffnen der Einstellungen nur, was ihre Abschnitte brauchen.
  Den Weg zum Dienst gibt die App dem Host; mit `--demo` antwortet er im Speicher und ändert nichts
  Echtes.
- Nur ein Mensch öffnet die Einstellungen. Antworten des Assistenten, Mails und Einträge im Eingang
  können keinen Abschnitt aufschlagen, und Agenten haben kein Ziel `settings:…` für `apps.open_app`.
- **Oberfläche, API, MCP:** keine neue Funktion. Alle Einstellungen gehen über Requests, die es
  schon gibt (`settings.set_setting`, `settings.set_collection_shown`, `inbox.mute_source`,
  `inbox.remove_rule`, `accounts.update_mail_account`, `clients.update_client` …), nur an anderer
  Stelle der Oberfläche. Die Apps nutzen den App-Schlüssel wie die Steuerzentrale; die Rechte von
  Assistent und Bots (`clients.*`, `Mcp::Never`) ändert weiter nur ein Mensch, nie ein Agent.

## Konten

### Anbieter-Katalog

„Add account“ öffnet zuerst die Auswahl des Anbieters, dann den Assistenten des Anbieters.

| Anbieter | Wofür | Anmeldung | |
|---|---|---|---|
| **Nextcloud** | Kalender, Aufgaben; später Kontakte, Dateien, Deck, Notizen, Talk | Login Flow v2 im Browser, App-Passwort im Schlüsselbund (gibt es schon im Kern) | v1 |
| **GitHub** | Issues und Projekte lesen; später Pull Requests, Reviews, Benachrichtigungen | Ohne eigene OAuth-App: **(a) Anmeldung der GitHub CLI übernehmen**, wenn `gh` angemeldet ist (Token per `gh auth token`, einmal bestätigen; für Projekte `gh auth refresh -s read:project`), oder **(b) Token einfügen** (Personal Access Token, nur Lesen; Link zur Token-Seite mit vorausgewählten Rechten). Token im Schlüsselbund. OAuth Device Flow mit eigener App höchstens später | nächste |
| **Slack** | Erwähnungen, Direktnachrichten, ausgewählte Kanäle | Weg noch offen (Token statt OAuth wahrscheinlich); Token im Schlüsselbund | später |
| **ntfy** | Themen abonnieren für den Eingang (CI, Uptime Kuma, Skripte) | Server (leer = `ntfy.sh`) und Themen mit Etikett; nur wenn ein Thema es braucht Benutzer und Passwort oder Token `tk_…` (`accounts.set_ntfy_login`, nur über `https` oder an diesen Rechner), im Schlüsselbund. Themen-Namen sieht nur die Steuerzentrale, Agenten das Etikett (E5.12) | E5 |
| **KI-Anbieter** (Ollama, LM Studio, OpenRouter, OpenAI-kompatibel) | Modelle für Assistent, Bots und KI-Funktionen der Apps | lokal: Adresse; sonst API-Schlüssel (maskiertes Feld, „Key stored“, entfernen). Steht im Katalog **zuoberst** (S.25), siehe [KI-Konten](#ki-konten) | gibt es (KI.1a) |

Weitere Anbieter (Google Kalender, Todoist, IMAP über TorroMail …) kommen als neue Einträge im
Katalog hinzu, nie als Umbau.

### Einrichtungsassistent

Ein Dialog mit wechselndem Untertitel je Schritt, Fehler links unten, „Cancel“ und Primärknopf
rechts unten:

1. **Wo:** Adresse (Nextcloud) bzw. nichts (GitHub, Slack).
2. **Anmelden:** „<Anbieter> handles the sign-in. You sign in on its own page; Torro Workplace
   never sees your password.“ Browser öffnet sich; als Rückfall die URL zum Kopieren bzw. der
   Code (GitHub).
3. **Prüfen:** „Checking the connection…“, erst danach wird gespeichert. Abbrechen räumt
   angelegte Schlüssel wieder weg.
4. **Fertig:** was jetzt verfügbar ist („Your calendars and task lists are ready.“).

### Detailseite eines Kontos

- **Status:** verbunden / Problem (Satz des Servers) / zuletzt synchronisiert vor … ; Knöpfe
  „Sync now“ und, wenn nötig, „Sign in again“.
- **Was genutzt wird:** je Fähigkeit (Kalender, Aufgaben, Issues …) die Sammlungen mit Schalter,
  z. B. welche Kalender die Suite zeigt. Schreibgeschützte sind gekennzeichnet.
- **Verbindung:** Server, Benutzer, App-Passwort „stored in the system keychain“.
- **„Remove account…“**: „This only removes the account from Torro Workplace. Your data stays on
  the server.“ Das App-Passwort wird dabei, wenn möglich, auch auf dem Server widerrufen.

### KI-Konten

Ergänzt am 2026-10-15. Anlass: Ein verbundenes Ollama erschien weder unter „Agents & API“ noch auf
seiner Kontoseite mit Modellen. Die Modelle fand man nur unter Assistant › New chats, nachdem man dort
Ollama gewählt hatte, und die Zeile „Engines“ führte zurück zu den Konten. Aufgaben: S.23–S.26.

**Drei Dinge, die leicht alle „Agent“ heißen**, und wo sie stehen:

| Was | Richtung | Ort | Wort in der Oberfläche |
|---|---|---|---|
| Claude Code, Codex, Gemini CLI, Skripte | bedienen Torro mit einem Schlüssel | Steuerzentrale › Agents & API | „Connect agent“ |
| Ollama, LM Studio, OpenRouter, OpenAI-kompatibel | liefern Torro Modelle | Steuerzentrale › Accounts (KI-Konto) | „AI provider“, „Model“ |
| Woher der Assistent antwortet (ein KI-Konto oder Claude Code) | antwortet im Chat | Einstellungen des Assistenten › General | „Engine“ |

- **Katalog:** „AI providers“ steht in „Add account“ an erster Stelle, mit dem Untertitel „Ollama, LM
  Studio, OpenRouter and more“. Im Dialog wie bisher zuerst „On this computer“ (Open First), dann
  OpenRouter, dann „Other OpenAI-compatible server“ (S.25).
- **Kontoseite eines KI-Kontos:** zwischen Status und Verbindung die Gruppe „Models“, eine Zeile je
  Modell mit Name, Merkmalen (Tools, Thinking, Context) und dem Ort, an dem es rechnet: „On this
  computer“, „Ollama Cloud“ oder „Cloud“. Modelle ohne Werkzeuge stehen blass mit „Can't use tools“.
  „Use for new chats“ setzt das Modell als Standard des Assistenten (S.24). Die Kontoseite bleibt die
  Seite des Kontos; Engine, Modell und Denkstufe neuer Chats wählt man weiter in den Einstellungen
  des Assistenten (S.17).
- **„Connect agent“** weist alle, die eigentlich ein Modell suchen, mit einer Zeile auf die KI-Konten
  hin (S.26).
- **Ollama Cloud:** Modelle mit dem Zusatz `:cloud` laufen über das lokale Ollama auf `ollama.com`.
  Für die Datenregel ([ki.md](ki.md), Grundsatz 4) gelten sie als **entfernt**, nicht als lokal,
  erkannt an `remote_host` in Ollamas `/api/tags` (S.23). Ein eigener Anbieter „Ollama Cloud“ (direkt
  mit API-Schlüssel, ohne lokales Ollama) kommt erst, wenn jemand ihn ohne lokales Ollama braucht.

### Mehrere Konten

Das Modell erlaubt mehrere Konten je Anbieter (zwei Nextclouds, privat und Arbeit). Die
Detailseite bestimmt, welche Sammlungen in die Apps fließen.

### GitHub: Rechte

Issues lesen: Classic-Token mit `repo` (privat) bzw. ohne Rechte (öffentlich) oder Fine-grained
Token mit „Issues: Read“. Projekte (Projects v2, GraphQL): `read:project` beim Classic-Token bzw.
„Projects: Read“ beim Fine-grained Token (für Projekte einer Organisation). Die Detailseite zeigt,
welche Rechte das Token hat, und was ohne sie fehlt („Projects need the read:project permission“).

## Kalender und Listen pro Fenster (später)

Jedes App-Fenster bekommt ein eigenes **Fenster-Profil**: welche Kalender bzw. Listen es zeigt,
welche Ansicht (Tag, Woche, Monat, Agenda). So lassen sich mehrere Kalenderfenster nebeneinander
kacheln, etwa „Arbeit · Woche“ und „Familie · Monat“.

- Jedes Fenster hat eine feste Kennung (`--window <id>`); ohne Angabe erzeugt die App eine neue.
- Die App legt ihr Profil unter dieser Kennung im State-Verzeichnis ab und lädt es beim Start.
- Die **Workspace-Vorlagen** speichern den Startbefehl samt `--window <id>`. Beim Laden öffnen sich
  dieselben Fenster mit denselben Kalendern und derselben Ansicht.
- Die Auswahl im Fenster ist eine Teilmenge dessen, was die Steuerzentrale für die Suite freigibt.

## Ablage

| Was | Wo |
|---|---|
| Konten (ohne Geheimnisse) | `accounts.json` im Config-Verzeichnis des Kerns (gibt es) |
| Geheimnisse (App-Passwörter, Tokens, API-Schlüssel) | Schlüsselbund des Systems, Dienst `torroworkplace` (gibt es) |
| Einstellungen (Sprache, Autostart, ausgeblendete Sammlungen, gewähltes Design `design` …) | `settings.json` im Config-Verzeichnis, atomar geschrieben |
| Design-Pakete | `designs/<id>/` im Datenverzeichnis; aufgelistet mit `settings.list_designs`, gewählt mit `settings.set_setting { key: "design" }` (L.3) |
| Vorlagen | wie bisher `templates/` im Datenverzeichnis |

Die Steuerzentrale liest und schreibt nie selbst Dateien, sondern fragt den Dienst über IPC
(`Settings`, `SetSetting`, `Accounts`, `AddAccount…`, `RemoveAccount`, `TestAccount`). So sehen
Tray, Apps und Steuerzentrale immer denselben Stand.

Das gewählte Design gilt in allen offenen Fenstern sofort, auf demselben Weg wie hell/dunkel und
der Zoom: Jede App fragt die Einstellungen beim Start (vor dem ersten Bild) und bei jedem Takt, lädt
das Paket, lädt seine Schriften nach und zeichnet neu; nur das Tray-Symbol bleibt fest (Issue #114). Ist das Paket
weg oder ungültig, zeichnen alle mit dem Standardpaket, die Einstellung bleibt stehen. Zum
Ausprobieren ohne Dienst: `--demo --design <id>` in jeder App.

## Apps ohne Konto

Der heutige Anmeldebildschirm in Kalender und Aufgaben entfällt. Stattdessen:
„No calendar account yet.“ + „Open Settings“ (öffnet `torroworkplace ui --section accounts`).
Die App wartet und lädt von selbst, sobald ein Konto da ist.

## Offene Fragen

1. Slack: welcher Weg (Bot-Token, Benutzer-Token, OAuth) – entscheiden, wenn Slack drankommt.

Entschieden am 2026-10-11: GitHub ohne eigene OAuth-App (CLI-Anmeldung übernehmen oder Token
einfügen). Kalender und Listen werden später pro Fenster gewählt, und die Workspace-Vorlagen
merken sich, welche Fenster mit welcher Auswahl offen waren.
