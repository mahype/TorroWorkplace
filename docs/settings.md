# Einstellungen: die Steuerzentrale

Stand: 2026-10-20. **Konzept zur Freigabe**; die Einstellungen in den Apps sind am 2026-10-14
entschieden, ihre Ordnung am 2026-10-16 (#210, Entwurf B „Schlanke Zentrale“). Gebaut ist sie mit
S.16–S.21: Jede App hat ihre Einstellungen hinter dem Zahnrad im eigenen Fenster, die Steuerzentrale
hat nur noch die Abschnitte der Suite. Der Abschnitt [Bestand](#bestand-stand-2026-10-16) beschreibt
den Stand davor.

**Neu geordnet mit #231 (2026-10-19):** Accounts, API und MCP sind getrennt; „Agents & API“ entfällt.
Am 2026-10-20 ist AI als ständiger Abschnitt direkt nach Accounts hinzugekommen. Die Entscheidungen stehen in [Accounts, API und MCP](#accounts-api-und-mcp-231),
die Aufgaben in [tasks/steuerzentrale.md](tasks/steuerzentrale.md), Stufe 5. Nennt ein älterer Absatz (vor allem
im [Bestand](#bestand-stand-2026-10-16)) noch „Agents & API“ oder „Accounts › AI“, gilt die aktuelle Navigation oben.

### Die Regel

**In die Steuerzentrale gehört, was mehrere Apps teilen oder ohne das keine App läuft:** Konten und ihre
Verbindung (auch KI-Konten mit ihren Modellen und der Datenregel), Programme, die die Suite von außen nutzen,
mit Schlüsseln und Rechten (API und MCP), Freigaben, Aussehen, Sprache und welche Apps an sind. Die Rechte von
Assistent und Bots stehen seit #231 in deren Fenster (siehe [Accounts, API und MCP](#accounts-api-und-mcp-231)).

**In die App gehört, was nur ändert, wie diese App arbeitet:** welche Kalender oder Listen sie zeigt, wann
sie erinnert, was in den Eingang darf, wie Mail aussieht und abgeschickt wird, wer der Assistent ist. Das
gibt es **genau einmal**, hinter dem Zahnrad der App im selben Fenster; die Steuerzentrale zeigt es nicht
noch einmal, sie verweist nur hin. Einzige Ausnahme ist die Einrichtung beim ersten Start, die das Nötigste
(Kalender, Listen, Quellen, neue Chats) einmal abfragt.

Was eine App nur für ein Fenster merkt (Seitenleiste, Breite, Reihenfolge der Anzeige), ist keine
Einstellung und erscheint nirgends in den Einstellungen (`ui.json`, siehe [Bestand](#bestand-stand-2026-10-16)).
Die Dichte des Eingangs gehört seit Issue #226 nicht mehr dazu: Sie ist ein Wert des Designs (`layout.density`).

Die Apps haben keine eigenen Kontoeinstellungen. Fehlt ihnen ein Konto, zeigen sie einen Hinweis mit dem
Knopf „Open Settings“, der die Steuerzentrale an der richtigen Stelle öffnet.

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
| **Accounts** | Externe Konten in festen Gruppen (Nextcloud, Mail, GitHub, ntfy): Anbieter links, Konten rechts; Status, Verbindung und Dienste je Konto. | v1 |
| **AI** | Ständiger Abschnitt für KI-Anbieter, ihre Konten und Modelle sowie die Datenregel „Mail to cloud models“. | seit 2026-10-20 |
| **API** | Ob Programme die API mit eigenem Schlüssel nutzen dürfen; die Keys mit ihren Rechten bis auf einzelne Endpunkte. Konzept: [api.md](api.md) | #231 (S.35) |
| **MCP** | Ob Agenten Torro über MCP nutzen dürfen; die verbundenen Clients (Claude Code, Codex …) mit ihren Rechten bis auf einzelne Tools und ihren Hooks für den Notification Hub | #231 (S.35) |
| **Approvals** | Was Agenten zur Freigabe vorlegen, und was entschieden wurde | v1 |
| **Workspaces** | Fenstermanager, Tasten 1 … 0 mit wählbarem Modifikator, Vorlagen bearbeiten, aktualisieren, löschen, beim Anmelden laden. Strategie je Fenster später; die F1–F10 der alten App sind verworfen. Siehe [Workspaces (#243)](#workspaces-243) | #243 (WS.6), Rest später |
| **Appearance** | Hell/dunkel, Design-Paket ([design-pakete.md](design-pakete.md)), Zoom; gilt für alle Apps, die Apps verweisen hierher | v1 |
| **General** | Sprache (System / English / Deutsch), Start beim Anmelden, Einrichtung erneut; Gruppe **Apps**: je App „On“ (Komponente) und „Open“, dazu der Satz, dass ihre Einstellungen hinter dem Zahnrad liegen | v1, Gruppe Apps mit S.21 |
| **Updates** | Automatisch prüfen, „Check for updates now“, zuletzt geprüft | später |
| **Diagnostics** | Log-Datei, Sync-Protokoll, Verbindung testen | später |
| **Help** | Kurzanleitung, Tastenkürzel, Links | später |

Die Seitenleiste hat **eine** Liste: Overview, Accounts, AI, API, MCP, Approvals, Workspaces, Appearance; General
am Fuß (#231). Die heutigen Seiten **Apps**, **Assistant** und **Notification Hub** entfallen (entschieden
2026-10-16, #210): Apps geht in General auf, die Einstellungen von Assistent und Eingang ziehen in deren Fenster.
Mit #231 ziehen die Rechte von Assistent und Bots in das Fenster des Assistenten. Die Datenregel steht
seit 2026-10-20 unter AI; „Agents & API“ entfällt.

## Einstellungen in den Apps

Entschieden am 2026-10-14 (Mockup im Gespräch: der Assistent), geordnet am 2026-10-16 (#210,
Entwurf B, https://claude.ai/artifact/5pp7TeLH386QHgp9aHisMy).

Warum: Bis dahin öffnete das Zahnrad jeder App die Steuerzentrale als eigenes Fenster, immer auf
„Accounts“, und was man in der App suchte, stand weit weg (Bestandsaufnahme, Befunde 1 und 4).

### Wie es aussieht

```
┌ Seitenleiste der App ┐┌ Inhalt ──────────────────────────────────────────────────────┐
│ T Calendar ▾         ││ ← Settings › Reminders                                    ⚙  │
│ Today                │├────────────────┬─────────────────────────────────────────────┤
│ Week                 ││ Calendars      │ Reminders                                   │
│ Calendars            ││ Reminders      │  Show reminders                      [on]   │
│  ● Work              ││ Night          │  Default reminder         15 min before ▾   │
│  ● Family            ││ ────────────── │  On all displays                     [on]   │
│                      ││ Appearance ↗   │ Which events                                │
│                      ││ All settings ↗ │  Only events I accepted              [on]   │
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
  Zeilen, erklärender Satz). Unterseiten öffnen sich an derselben Stelle, der Pfad wird länger.
- **Unten in der Liste** zwei Verweise: „Appearance ↗“ (öffnet die Steuerzentrale bei Appearance; Farbe,
  Design und Zoom gelten für alle Apps) und „All settings ↗“ (öffnet die Steuerzentrale bei General).
- **Kleiner werden** wie überall (Design-Skill, Abschnitt 0): Mittel zeigt die Abschnitte als Menü
  in der Kopfleiste („Reminders ▾“), Schmal als Liste, aus der ein Abschnitt als eigene Seite mit
  „‹ Back“ aufgeht.
- **Direkt öffnen**: `--settings <abschnitt>` in jeder App (auch mit `--demo`, für die
  Sichtprüfung). Die Steuerzentrale öffnet eine App mit `apps.open_app` (holt ein laufendes Fenster nach vorn);
  dort die Einstellungen gleich aufzuschlagen bräuchte einen neuen Weg vom Kern zur App und ist nicht geplant. Hinweise in der App, die
  bisher die Steuerzentrale öffneten, öffnen den passenden Abschnitt im selben Fenster („All rules“ →
  Rules); das Mail-Fenster hat keine Rules, dort holt „All rules“ das Fenster des Notification Hub nach
  vorn, wo die Regeln stehen. „Change access“ unter einer abgelehnten Antwort und „Access…“ eines Bots öffnen
  seit #231 den Abschnitt Access im selben Fenster; Konten einrichten öffnet weiter Accounts in der Steuerzentrale.

### Was wohin gehört

| App | Abschnitte | kommt heute von |
|---|---|---|
| **Assistant** | General (Status an/aus mit Verweis auf Access; neue Chats: Engine, Modell, Denkstufe) · Profile (Profil und Gedächtnis) · **Access** (seit #231: Rechte des Assistenten und je Bot, „Turn on assistant“, „Turn off assistant…“) · Bots (Liste; je Bot „Edit“ öffnet den Bot-Editor, „Access“ öffnet Access beim Bot) · Tools (eigene Werkzeuge, #225: je Werkzeug Schalter, ob der Assistent es darf mit Verweis auf Access; PDF: Ablage-Ordner, Vorlagen Letter, Report, Note) · Voice (Sprechen, Antworten vorlesen, Mikrofon, Programme auf diesem Rechner) · History (Unterhaltungen aufheben) | Seite Assistant ohne Datenregel und „Engines“; Access aus Agents & API (#231) |
| **Calendar** | Calendars (welche die Suite zeigt, je Konto gruppiert; Art der Erinnerung je Kalender) · Reminders (an/aus, Vorlauf, alle Bildschirme, Ton, welche Termine) · Night (die Karte am Mond in der Zeitleiste bleibt als schneller Weg) | Kontodetail „Calendars and lists“; General › Calendar, › Reminders, › Which events, › Calendars |
| **Tasks** | Lists (welche die Suite zeigt, je Konto gruppiert) | Kontodetail „Calendars and lists“ |
| **Notification Hub** (Fensterarten Notification Hub und App Feed) | Notifications (still bei Terminen) · Sources (je Konto: Nextcloud-Benachrichtigungen und Talk, GitHub, je ntfy-Server stumm oder an, je Mail-Konto „Show new mail“) · Rules (Liste mit „Remove“) | Seite Notification Hub; Abschnitte „Notification Hub“ der Kontoseiten |
| **Mail** (Fensterart Mail) | Accounts (je Mail-Konto Absendername, Signatur, „Write new mail as“, offene Ordnerfragen, Backup und Restore) · Reading („Show mail as“, „Mail colors“) · Keyboard shortcuts (E10.3) | Kontodetail Mail; Appearance › Mail; Mail-Backup #267 |

Keine App zeigt „Appearance“ als eigene Seite (nur den Verweis). Die Fensterarten des Eingangs-Programms
zeigen je ihre Abschnitte: Notification Hub und App Feed die des Eingangs, das Mail-Fenster die von Mail.

In der Steuerzentrale bleiben: Konten (Status, Verbindung, welche Dienste ein Konto liefert,
Entfernen; KI-Konten mit Modellen und Datenregel, siehe [KI](#ki)), API und MCP (Programme von außen mit Rechten),
Approvals, Workspaces, Appearance, General (Sprache, Start beim Anmelden, Apps an/aus). Die Kontodetailseiten
werden dadurch kurz; wo eine Einstellung umzieht, steht dort eine Zeile, die zum neuen Ort führt
(„Calendars and lists are chosen in Calendar and Tasks.“) mit „Open“ je App.

Zusammengelegt (Befund 2): „Show new mail in the Notification Hub“ gibt es nur noch als Zeile je Mail-Konto
unter Notification Hub › Sources (`accounts.update_mail_account`, Feld `notify`); die Quelle „Mail“ der
Einrichtung schickt dasselbe statt `inbox.mute_source`. Die Rechte eines Bots stehen seit #231 nur unter
Assistant › Access; der Bot-Editor verweist dorthin. Die Nacht bleibt an zwei Stellen, aber beide in der Kalender-App
(Abschnitt Night und Karte am Mond).

Was im Fenster gewählt wird, gilt für die ganze Suite (wie heute in der Steuerzentrale). Die
spätere Auswahl **pro Fenster** (unten) kommt als eigener Schalter dazu, nicht als Umbau.

### Technik

- Die Seiten der Apps ziehen in eine neue Bibliothek `crates/torro-settings-ui`: die Zeilen-Bausteine
  (`setting_row`, `pressable`, `chevron`, `dialog`, `confirm`, die auch die Steuerzentrale nutzt) und ein
  **Host** für den Inhaltsbereich (Abschnittsliste, Kopfleiste mit Pfad, Seite, Laden und Senden). Die
  Steuerzentrale zeigt die Seiten der Apps **nicht**; sie nutzt aus der Bibliothek nur die Bausteine.
- Seiten bleiben wie heute: Sie geben eine `Action` zurück, der Host schickt den Request und lädt
  neu. Jede App lädt beim Öffnen der Einstellungen nur, was ihre Abschnitte brauchen.
  Den Weg zum Dienst gibt die App dem Host; mit `--demo` antwortet er im Speicher und ändert nichts
  Echtes.
- „Open“ unter General › Apps und an den Kontoseiten schickt `apps.open_app` mit der Fensterklasse (Mail:
  `torro-inbox-mail`).
- Der Schlüssel eines neuen Clients gelangt nie in einen App-Prozess. Bis #231 hieß das: Apps senden keine
  schreibenden `clients.*`. Seit #231 darf **nur** der Abschnitt Access des Assistenten `clients.update_client`,
  `clients.activity` und – für die Art `assistant` – `clients.create_client` und `clients.remove_client` senden;
  der Kern gibt für die Art `assistant` keinen Schlüssel heraus (der Kern führt die Werkzeuge des Assistenten
  selbst aus, niemand braucht ihn). `clients.new_key` und Clients anderer Arten bleiben der Steuerzentrale.
- Nur ein Mensch öffnet die Einstellungen. Antworten des Assistenten, Mails und Einträge im Eingang
  können keinen Abschnitt aufschlagen, und Agenten haben kein Ziel `settings:…` für `apps.open_app`.
- **Oberfläche, API, MCP:** keine neue Funktion. Alle Einstellungen gehen über Requests, die es
  schon gibt (`settings.set_setting`, `settings.set_collection_shown`, `inbox.mute_source`,
  `inbox.remove_rule`, `accounts.update_mail_account` …), nur an anderer
  Stelle der Oberfläche. Die Rechte von Assistent und Bots (`clients.*`, `Mcp::Never`) ändert weiter nur ein
  Mensch, nie ein Agent: seit #231 im Abschnitt Access des Assistenten, der wie jedes Fenster der Suite den
  App-Schlüssel hält (dieselbe Begründung wie beim Mikrofon, [api.md](api.md) Punkt 6).

## Konten

### Anbieter

Seit #231 gibt es keinen Katalog mehr: Die Seite Accounts zeigt eine feste Gruppe je Art, und das „+ Add“ der
Gruppe öffnet den Assistenten direkt bei diesem Anbieter (siehe [Accounts, API und MCP](#accounts-api-und-mcp-231)).
Die Tabelle sagt, was es gibt; Slack erscheint erst, wenn es gebaut ist.

| Anbieter | Wofür | Anmeldung | |
|---|---|---|---|
| **Nextcloud** | Kalender, Aufgaben; später Kontakte, Dateien, Deck, Notizen, Talk | Login Flow v2 im Browser, App-Passwort im Schlüsselbund (gibt es schon im Kern) | v1 |
| **GitHub** | Issues und Projekte lesen; später Pull Requests, Reviews, Benachrichtigungen | Ohne eigene OAuth-App: **(a) Anmeldung der GitHub CLI übernehmen**, wenn `gh` angemeldet ist (Token per `gh auth token`, einmal bestätigen; für Projekte `gh auth refresh -s read:project`), oder **(b) Token einfügen** (Personal Access Token, nur Lesen; Link zur Token-Seite mit vorausgewählten Rechten). Token im Schlüsselbund. OAuth Device Flow mit eigener App höchstens später | nächste |
| **Slack** | Erwähnungen, Direktnachrichten, ausgewählte Kanäle | Weg noch offen (Token statt OAuth wahrscheinlich); Token im Schlüsselbund | später |
| **ntfy** | Themen abonnieren für den Eingang (CI, Uptime Kuma, Skripte) | Server (leer = `ntfy.sh`) und Themen mit Etikett; nur wenn ein Thema es braucht Benutzer und Passwort oder Token `tk_…` (`accounts.set_ntfy_login`, nur über `https` oder an diesen Rechner), im Schlüsselbund. Themen-Namen sieht nur die Steuerzentrale, Agenten das Etikett (E5.12) | E5 |

Weitere Anbieter (Google Kalender, Todoist …) kommen als neue Gruppe bzw. als weiterer Weg im
Add-Dialog einer Gruppe hinzu, nie als Umbau.

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

### KI

Entschieden am 2026-10-15. Anlass: Ein verbundenes Ollama erschien weder unter „Agents & API“ noch auf
seiner Kontoseite mit Modellen; die Modelle fand man nur unter Assistant › New chats, und „Connect
agent“ verstand niemand als den Weg zu einem Modell. Aufgaben: S.23–S.26.

**Das Prinzip** gilt für die ganze Suite, nicht nur für KI:

1. **Zentral wird eingerichtet, was das ganze Programm nutzt.** KI-Anbieter sind so etwas: Ein
   Anbieter wird einmal eingerichtet, seine Modelle stehen danach jedem Modul zur Verfügung.
2. **Im Modul wird eingestellt, wie das Modul es nutzt.** Welches Modell der Assistent für neue Chats
   nimmt, welches ein Bot, welches später die Inbox zum Zusammenfassen: das steht in den Einstellungen
   des Moduls, in dessen Fenster ([Einstellungen in den Apps](#einstellungen-in-den-apps)), nicht in
   der Steuerzentrale.
3. **Rechte stehen beim Weg, über den ein Programm kommt** (geändert mit #231; vorher „Rechte stehen zentral“,
   #210). Programme von außen: Steuerzentrale › API (Skripte mit Key) bzw. › MCP (Agenten wie Claude Code).
   Der eigene Assistent und seine Bots: im Fenster des Assistenten, Abschnitt Access. Alle nutzen denselben
   Rechte-Editor. Bindet die Suite später **externe MCP-Server** ein, steuert man den Zugriff darauf unter MCP.

**KI-Konten stehen im eigenen Abschnitt AI** (seit 2026-10-20 direkt nach Accounts). Dort gibt es
„+ Add“; intern sind es Konten wie bisher (`accounts.json`, Schlüssel im Schlüsselbund). Unter den
Konten steht die Datenregel „Mail to cloud models“ (vorher in den Rechten des Assistenten).

- **Der Abschnitt „AI“:** eine Zeile je Anbieter mit Name, Adresse bzw. „Found on this computer“, Status
  (verbunden, Server aus, Schlüssel abgelehnt) und der Zahl angeschalteter Modelle. Reihenfolge Open
  First: zuerst, was auf diesem Rechner installiert ist (Ollama, LM Studio), dann
  OpenRouter, dann andere OpenAI-kompatible Server, dann geschlossene Anbieter (Anthropic, OpenAI,
  Google). „Installiert“ ist nicht „rechnet hier“: Die `:cloud`-Modelle von Ollama laufen über das
  lokale Ollama, rechnen aber auf `ollama.com`. Der Ort jedes Modells steht an seiner Zeile.
- **„+ Add“ des Abschnitts** öffnet den bekannten Dialog direkt bei den KI-Anbietern: „On this computer“
  (gefundene Server mit „Connect“), OpenRouter (API-Schlüssel), „Other OpenAI-compatible server“
  (Adresse, optional Schlüssel). Erst nach erfolgreicher Prüfung ist der Anbieter da.
- **Claude ist ein Anbieter wie jeder andere:** ein KI-Konto über das Anthropic-Protokoll mit eigenem
  Schlüssel (KI.1b–KI.1d), Ort „Cloud (Anthropic)“. **Claude Code** erscheint hier *nicht* — der Weg
  über die gefundene CLI (Antrieb B in [assistent.md](assistent.md)) wird seit Issue #219 nicht mehr
  angeboten: Er war nie eingerichtet, nur gefunden, also gab es nichts anzuzeigen und nichts
  abzuschalten. Codex bleibt ebenfalls außen vor (kein Weg, nur die Werkzeuge der Suite zu erlauben).
  Claude Code unter „MCP“ ist etwas anderes: Dort ruft der Agent die Suite über MCP.
- **Die Kontoseite eines KI-Kontos** („‹ AI › Ollama“): Status und Verbindung, darunter
  **„Models“**: eine Zeile je Modell mit Name, Merkmalen (Tools, Thinking, Context), dem Ort, an dem es
  rechnet („On this computer“, „Ollama Cloud“, „Cloud“), und einem **Schalter**. Modelle, die
  weder Werkzeuge nutzen noch entscheiden, stehen blass mit „Can't use tools or decide“ und ohne Schalter, weil
  kein Modul sie nutzen kann; ein Entscheidungsmodell (nimble) hat das Merkmal „Decides“ und einen Schalter, weil
  die Mail-Sortierung es braucht (#231).
  OpenRouter: Suche und „Show closed models“, weil es Hunderte sind. Ganz unten „Remove account…“.
- **Was angeschaltet ist, gilt überall:** Die Auswahlen der Module (Assistent › New chats, die Auswahl
  pro Chat, der Bot-Editor, später die KI-Funktionen der Inbox) bieten nur angeschaltete Modelle an.
  Ist keins an, sagen sie es in einem Satz mit „Turn on models in Settings › AI“.
- **Am Anfang an:** alle nutzbaren Modelle (mit Werkzeugen oder entscheidend, #231), die **auf diesem Rechner rechnen** (Ollama ohne
  `:cloud`, LM Studio, OpenAI-kompatible Server an einer Adresse dieses Rechners). Modelle, die anderswo
  rechnen (Ollama Cloud, OpenRouter, Anthropic, Server im Netz), sind aus, bis man sie anschaltet:
  Open First, und so geht ohne Zutun nichts in die Cloud. Ausnahme: Ein Modell, das ein Modul schon
  nutzt (gespeicherter Standard, ein Bot, eine Zuordnung wie die Mail-Sortierung), ist an; steht das Modul auf
  „Automatic“, bleibt das Modell an, das „Automatic“ dort vorher gewählt hätte. Neue Modelle, die hier rechnen, kommen an dazu,
  solange der Mensch an diesem Anbieter noch nichts ausgeschaltet hat.
- **Ein Modell anschalten, das anderswo rechnet, kann nur ein Mensch** (wie der Schalter
  `mail_to_cloud`, [api.md](api.md) Punkt 6). Abschalten dürfen auch Agenten, mit Freigabe.
- **Ein abgeschaltetes Modell, das ein Modul noch nutzt** (Standard neuer Chats, ein Bot, die Wahl in
  einer Unterhaltung): Das Modul zeigt es weiter mit dem Hinweis „Turned off in Settings › AI“ und
  nimmt für neue Antworten „Automatic“ unter den angeschalteten Modellen **desselben Anbieters**, bis
  jemand ein anderes wählt; erst danach bestimmt der Kern den Ort und wendet die Datenregel an.
  Laufende Antworten laufen zu Ende. Ist bei diesem Anbieter gar nichts mehr an, antwortet das Modul
  nicht mit einem anderen Anbieter, sondern sagt es: „Turn on a model of {provider} in Settings › AI.“
- **Ollama Cloud:** Modelle mit dem Zusatz `:cloud` laufen über das lokale Ollama auf `ollama.com` und
  gelten für die Datenregel als **entfernt** ([ki.md](ki.md), Grundsatz 4; umgesetzt mit S.23). Ein
  eigener Anbieter „Ollama Cloud“ (direkt mit API-Schlüssel) kommt erst, wenn jemand ihn ohne lokales
  Ollama braucht.

### Mehrere Konten

Das Modell erlaubt mehrere Konten je Anbieter (zwei Nextclouds, privat und Arbeit). Die
Detailseite bestimmt, welche Sammlungen in die Apps fließen.

### GitHub: Rechte

Issues lesen: Classic-Token mit `repo` (privat) bzw. ohne Rechte (öffentlich) oder Fine-grained
Token mit „Issues: Read“. Projekte (Projects v2, GraphQL): `read:project` beim Classic-Token bzw.
„Projects: Read“ beim Fine-grained Token (für Projekte einer Organisation). Die Detailseite zeigt,
welche Rechte das Token hat, und was ohne sie fehlt („Projects need the read:project permission“).

## Accounts, API und MCP (#231)

Entschieden am 2026-10-19 (#231). Anlass: „Add account“ zeigte einen Katalog aller Anbieter und dazu die Konten
aus TorroMail; unter „Agents & API“ standen Assistent, Bots, Coding-Agenten, Hooks und Skripte gemischt; die
Modelle eines KI-Kontos ließen sich nirgends an- und abschalten. Aufgaben: S.24, S.26, S.30–S.35 in
[tasks/steuerzentrale.md](tasks/steuerzentrale.md).

**Die Ordnung:** Accounts und AI für Dienste, API und MCP für Zugriffe auf die Suite.

| Abschnitt | Richtung | Was man dort tut |
|---|---|---|
| **Accounts** | Torro nutzt Dienste von außen | Nicht-KI-Konten hinzufügen, prüfen und entfernen |
| **AI** | Die Suite ruft KI-Modelle | KI-Anbieter hinzufügen, Modelle an/aus, Datenregel ändern |
| **API** | Programme nutzen Torro mit Key | API an/aus, Keys anlegen, je Key die Rechte bis auf einzelne Endpunkte |
| **MCP** | Agenten nutzen Torro über MCP | MCP an/aus, Clients verbinden, je Client die Rechte bis auf einzelne Tools, Hooks |

### Accounts

- **Feste Gruppen**, immer sichtbar, in dieser Reihenfolge: **Nextcloud**, **Mail** (mit den CalDAV-Konten
  eines Mail-Anbieters, Art `caldav`), **GitHub**, **ntfy**. Jeder Anbieter sagt in einem Satz unter seinem
  Namen, was hierher gehört; ohne Konto steht „No account yet.“ über seinem „Add“, statt dass er verschwindet.
- **Anbieter links, Konten rechts** (#244, S.37): Die Gruppen stehen als Anbieter in einer Spalte am linken
  Rand des Inhalts (Breit mit Namen und Zahl, Mittel nur mit Symbolen, Schmal als Chips über der Liste); das
  Symbol färbt sich, wenn ein Konto Aufmerksamkeit braucht oder einen Fehler hat. „Open accounts“ der Übersicht
  wählt den ersten Anbieter mit einem solchen Konto, „Add“ und der TorroMail-Import den Anbieter, dem sie ein
  Konto hinzufügen. Rechts steht nur der gewählte
  Anbieter: Name, darunter der Satz, was hierher gehört, dann seine Konten und „Add“. Ohne Wahl ist der erste
  Anbieter mit Konto gewählt; das Öffnen eines Kontos wählt seinen Anbieter, damit „Back“ dort landet. Entwurf:
  `docs/prototypes/accounts/index.html` (Variante A).
- **„+ Add“ je Gruppe** (seit #244 als letzte Zeile der Gruppe „Accounts“ des gewählten Anbieters) öffnet den
  Add-Dialog direkt beim ersten Schritt dieses Anbieters. Der Dialog hat keinen Katalog mehr und kein „Back“
  zu einem Katalog; „Back“ gibt es nur noch zwischen den Schritten eines Anbieters. Der globale Knopf „Add
  account“ in der Kopfleiste entfällt, ebenso die Zeile „Slack – Coming soon“.
- **TorroMail:** Die Übernahme steht nicht mehr im Add-Dialog. Die Gruppe Mail zeigt „Import from TorroMail…“
  nur, wenn TorroMail Konten hat, die die Suite übernehmen kann und noch nicht hat. Der Knopf öffnet denselben
  Dialog bei einem eigenen Schritt „TorroMail“, der nur die Liste zum Übernehmen zeigt (wie bisher oben im
  Katalog). Die Einrichtung beim ersten Start („Take over from TorroMail“) öffnet denselben Schritt.
- **KI-Konten:** eigener Abschnitt „AI“ und Kontoseite mit „Models“, siehe [KI](#ki). Unter den KI-Konten steht die Datenregel.

### API

- Gruppe **„API“:** Schalter **„Allow API keys“** (Standard an). Aus: Der Kern weist jeden Key der Art `script`
  ab (`unauthorized` mit dem Satz, dass die API in Torro Workplace ausgeschaltet ist). Später hier HTTP auf
  `127.0.0.1` mit Port (A.10).
- Gruppe **„Keys“:** eine Zeile je Key (Name, Rechte in Worten, zuletzt benutzt), „Create key“ in der
  Kopfleiste. „Create key“ fragt Name und Vorlage (Read only, Read and write, Full access) und zeigt den Key
  einmal mit „Copy“ (der bisherige Weg „Script“ des Agent-Dialogs).
- **Seite eines Keys:** der Rechte-Editor (unten) mit den Endpunkten (`calendar.list_events`), die letzten
  Aufrufe, „Create new key“, „Revoke…“.

### MCP

- Gruppe **„MCP server“:** Schalter **„Allow MCP“** (Standard an). Aus: Der Kern weist jeden Aufruf eines
  MCP-Clients ab, auch die seiner Hooks. Darunter die Befehlszeile zum Kopieren für Clients, die man von Hand
  einrichtet (`torroworkplace mcp` mit `TORRO_TOKEN`).
- Gruppe **„Connected clients“:** eine Zeile je Client (Claude Code, Codex, Gemini CLI, Claude Desktop,
  Cursor, „Other MCP client“), „Connect client“ in der Kopfleiste: der bisherige Agent-Dialog ohne „Script“,
  dafür mit „Other MCP client“ (Name, Key einmal zum Kopieren samt Befehlszeile).
- **Seite eines Clients:** der Rechte-Editor mit den MCP-Tools (`calendar_list_events`); Gruppe **„Notification
  Hub“** mit dem Schalter „Report when waiting or done“ (nur Claude Code, Codex, Gemini CLI) und dem Stand der
  Hooks (eingerichtet; bei Codex „Trust the hooks in Codex' /hooks“; Fehler mit Grund); die letzten Aufrufe,
  „Create new key“, „Disconnect…“.
- **Hooks** sind technisch weiter ein eigener Client der Art `hook` mit festen, kleinen Rechten
  (`Rights::hook`). Sie erscheinen nirgends mehr als eigene Zeile: Der Schalter auf der Seite des Agenten legt
  den Hook-Client an und schreibt die Hooks, bzw. nimmt beides heraus. Wie die Meldungen der Agenten im
  Eingang erscheinen, bleibt in den Einstellungen des Notification Hub; die Apps brauchen dafür nichts Eigenes.

### Der Rechte-Editor

Ein Editor für API, MCP und Access (Assistent, Bots), in `torro-settings-ui`, damit Steuerzentrale und
Assistent-Fenster denselben nutzen.

- **Je Bereich die Stufe** (No access · Read · Write · Full) und der Admin-Schalter, wie bisher. Die Stufe ist
  der schnelle Weg.
- **Neu: Bereich aufklappen** („Show commands“): eine Zeile je Befehl des Bereichs, den die Stufe erlaubt, mit
  Schalter, in API mit dem Endpunkt-Namen, in MCP und Access mit dem Tool-Namen, darunter die erste Zeile der
  Beschreibung aus dem Katalog. Abschalten trägt den Befehl in die **Sperrliste** des Clients ein
  (`Rights::denied`, Namen aus dem Katalog). Die Sperrliste schränkt nur ein: Was die Stufe nicht erlaubt,
  erscheint gar nicht, und kein Eintrag erlaubt mehr als die Stufe. Ein Bereich mit gesperrten Befehlen zeigt
  das in seiner Zeile („Write · 2 off“).
- Wie bisher: Auswahl der Kalender, Listen, Repositories, Inbox-Quellen, Mail-Konten und eigenen Werkzeuge des
  Assistenten (#225; ein Werkzeug, das in den Einstellungen des Assistenten aus ist, trägt ein Warnzeichen),
  „Ask first“, „Send on my behalf“. Gespeichert wird automatisch nach 500 ms mit `clients.update_client` (die ganzen Rechte).

### Assistent und Bots

- Assistent-Fenster › Zahnrad › **Access**: oben der Assistent (Status; ohne Client „Turn on assistant“ mit
  „Start from“ wie bisher; mit Client der Rechte-Editor und „Turn off assistant…“), darunter eine Zeile je Bot,
  die den Rechte-Editor des Bots öffnet. Ein Bot hat keinen Knopf zum Entfernen; ob es ihn gibt, entscheidet
  der Bot-Editor.
- Der Bot-Editor öffnet mit „What the bot may use“ den Abschnitt Access beim Bot, im selben Fenster.
- „Change access“ unter einer abgelehnten Antwort öffnet Access beim Assistenten bzw. beim Bot.
- Die Bereiche `assistant` und `bots` bleiben für Assistent und Bots ausgeblendet (`api::shows_area`).
- `--section assistant` und `--section bot:<id>` der Steuerzentrale holen das Assistent-Fenster nach vorn
  (`apps.open_app`; dort Access aufzuschlagen bräuchte einen neuen Weg vom Kern zur App, wie oben bei
  „Direkt öffnen“). Im Fenster selbst öffnen „Change access“ und „What the bot may use“ Access direkt;
  `torroassistant --settings access [--bot <id>]` schlägt es beim Start auf (auch für die Sichtprüfung).

### Schalter und Wege im Kern

- Jeder Aufruf trägt im Umschlag, über welchen Weg er kommt (`via`: `api` oder `mcp`, ohne Angabe `api`);
  `torroworkplace mcp` und der Hook-Befehl `torroworkplace hook` setzen `mcp`. Der Kern prüft nach dem Key:
  - Key der Art `script`: nur mit „Allow API keys“, und nur über `api`.
  - Key eines MCP-Clients (Claude Code, Codex, Gemini CLI, Claude Desktop, Cursor, Other MCP client): nur mit
    „Allow MCP“, und nur über `mcp`.
  - Key der Art `hook`: nur mit „Allow MCP“ (er gehört zu einem Agenten), über beide Wege. Der Hook-Befehl
    `torroworkplace hook` meldet sich als `mcp`, weil er ohne Schlüsseldatei `TORRO_TOKEN` nimmt, das der Key des
    Agenten sein kann.
  - Die Arten `assistant` und `bot` (auch ihre geliehenen Keys) hängen an keinem der beiden Schalter: Sie sind die
    Suite selbst.
  - Jede andere Art wird wie `script` geprüft; neue Clients gibt es nur in den genannten Arten.
- **Ehrlich festgehalten:** Der Weg im Umschlag ist eine Angabe des Programms, keine Prüfung. Er schützt vor
  Fehlbedienung (ein Agent-Key in einem Skript), nicht vor bösartigem Code unter demselben Benutzer – wie die
  Keys selbst ([api.md](api.md), „Was der Schlüssel schützt“).
- **Einschalten nur durch Menschen:** „Allow API keys“ und „Allow MCP“ einschalten bekommt von jedem Client
  `forbidden` (wie `mail_to_cloud`, [api.md](api.md) Punkt 6); ausschalten ist eine gewöhnliche Admin-Änderung.

### Was wohin zieht

| Bisher | Seit #231 |
|---|---|
| „Add account“ in der Kopfleiste, Katalog | „+ Add“ je Gruppe |
| Katalog › „Use accounts from TorroMail?“ | Mail › „Import from TorroMail…“ |
| Gruppe CalDAV | Gruppe Mail |
| geplanter Abschnitt „AI“ (S.25) | Kontoseite des KI-Kontos › Models (S.31) |
| Rechte des Assistenten › „Mail to cloud models“ | Accounts › unter der Gruppe AI |
| Agents & API › Agents | MCP › Connected clients |
| Agents & API › Scripts | API › Keys |
| Agents & API › Hooks | MCP › Seite des Agenten › Notification Hub |
| Agents & API › Assistant, Bots | Assistent-Fenster › Access |
| `--section agents` | `--section mcp` (`agents` bleibt als Wort dafür) |

### Entscheidungen (#231)

1. **Drei Abschnitte statt „Agents & API“:** Accounts, API, MCP. Verworfen: ein Abschnitt mit Gruppen je Art –
   genau das war das Durcheinander.
2. **„Add“ je Gruppe statt Katalog.** Spart einen Klick und jede Verwechslung. Verworfen: Katalog mit
   vorgewählter Gruppe.
3. **Überholt am 2026-10-20:** KI-Konten und ihre Modelle haben den eigenen Abschnitt AI direkt nach Accounts.
   Die Entscheidung vom 2026-10-19 sah sie noch unter Accounts vor.
4. **Rechte je Befehl als Sperrliste unter der Stufe.** Verworfen: nur Schalter je Befehl (über 100 Befehle,
   unbedienbar) und nur Stufen (zu grob).
5. **Rechte von Assistent und Bots im Assistent-Fenster.** Hebt „Rechte stehen zentral“ (#210) für diese beiden
   auf. Verworfen: weiter in der Steuerzentrale – dort stünden sie als einzige interne Programme zwischen API und
   MCP.
6. **Hooks gehören zum Agenten.** Verworfen: eigener Abschnitt oder eigene Einstellungen in den Apps.
7. **Weg im Umschlag, Schalter für API und MCP.** Verworfen: keine Schalter (dann ließe sich die API nicht
   abschalten) und eine echte Trennung über getrennte Sockets (Aufwand ohne Schutz gegen denselben Benutzer).

## Workspaces (#243)

Die Seite **Workspaces** der Steuerzentrale zeigt den Fenstermanager, die Tasten und die Vorlagen. Name, Taste und
Start sind Eigenschaften einer Vorlage. Jede hat einen eigenen Befehl (`workspaces.rename_template`,
`workspaces.set_template_shortcut`, `workspaces.set_template_startup`), damit eine Änderung nichts anderes löscht.

- **Fenstermanager:** Gruppe „Window manager“ mit Name, Version und Bildschirmen. Fehlt er, steht da, welcher
  erwartet wird und was zu tun ist.
- **Tastenkürzel:** Zeile „Shortcuts“ mit der Wahl des Modifikators (`set_setting workspace_shortcuts`). Vorgabe:
  Super + Alt unter Linux, Ctrl + Option unter macOS, Ctrl + Alt unter Windows. Die Zeile sagt, ob die Tasten wirken,
  oder warum nicht.
- **Vorlagen:** nach Taste sortiert (1 … 9, 0, dann ohne Taste). Je Zeile die Taste, der Name, Fensterzahl und
  Alter, „Beim Start · DP-1“ bei Start, „Laden“ (in den aktiven Workspace, wie „Hinzufügen“) und „…“ mit
  „Bearbeiten…“, „Aus aktuellem Workspace aktualisieren“ und „Löschen…“ (mit Rückfrage).
- **Workspace speichern…** speichert den aktiven Workspace und öffnet danach den Dialog.

**Dialog** „Vorlage bearbeiten“ (von der Seite oder aus dem Tray) und „Workspace speichern“ (direkt nach dem
Speichern):

- **Name.**
- **Taste:** „Keine“ oder 1 … 0. Hält eine andere Vorlage die Taste, sagt der Dialog es vorher; Speichern nimmt
  sie ihr weg.
- **Start:** Schalter „Beim Start laden“, dazu der Bildschirm als Skizze in echten Proportionen. Ein nicht
  angeschlossener Bildschirm bleibt wählbar. „Wenn der Bildschirm fehlt“: „Hauptbildschirm verwenden“ (Vorgabe)
  oder „Überspringen“.
- **Abbrechen** schließt im Modus „Vorlage bearbeiten“ ohne Änderung. Im Modus „Workspace speichern“ löscht es die
  gerade gespeicherte Vorlage wieder.
- Speichern schickt nur, was sich geändert hat. Ein Fehler bleibt im Dialog stehen.
- Fenster der Steuerzentrale und des Dialogs kommen nie in eine Vorlage.

**Tray-Menü:** Vorlagen mit Taste erscheinen mit ihrer Ziffer, „Aktualisieren“ und „Löschen“ gibt es nur auf dieser
Seite. Einzelheiten in [core.md](core.md#tray-menü-im-prototyp).

**Tasten:** Unter Hyprland setzt der Dienst die belegten Tasten zur Laufzeit, beim Start und nach jedem Neuladen der
Konfiguration erneut. Unter macOS und Windows meldet er sie beim System an; ein Druck lädt die Vorlage wie
„Hinzufügen“. Lässt sich eine Taste nicht anmelden, weil sie schon belegt ist, zeigt die Zeile „Shortcuts“ es an.

**Laden beim Start:** Einmal pro Anmeldung. Nach dem Start wartet der Dienst, bis der Fenstermanager die Bildschirme
stabil meldet. Dann lädt er jede Vorlage mit Start auf ihren Bildschirm, im Modus „Hinzufügen“, und gibt den Fokus an
den Bildschirm zurück, der ihn vorher hatte. Fehlt ein Bildschirm, lädt die Vorlage auf den Hauptbildschirm (der bei
0, 0 liegt, sonst der erste) oder wird übersprungen, je nach ihrer Einstellung. Ein Neustart des Dienstes in
derselben Anmeldung lädt nichts ein zweites Mal (Merker `startup.json` im State-Verzeichnis).

Die F1–F10 der alten Python-App (`SUPER + F1` … `F10`) sind verworfen; es gibt die Tasten 1 … 0.

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
| Einstellungen (Sprache, Autostart, ausgeblendete Sammlungen, gewähltes Design `design`, Schrift `font`, Farbschema `colorScheme` …) | `settings.json` im Config-Verzeichnis, atomar geschrieben |
| Designs, Schriften, Farbschemata | `designs/<id>/`, `fonts/<id>/`, `color-schemes/<id>/` im Datenverzeichnis; aufgelistet mit `settings.list_designs`, `settings.list_fonts`, `settings.list_color_schemes`, gewählt mit `settings.set_setting { key: "design" \| "font" \| "color_scheme" }` (L.3, L.22) |
| Vorlagen | wie bisher `templates/` im Datenverzeichnis; Felder `shortcut` und `startup` (#243) |
| Tastenmodifikator der Vorlagen | `settings.json`, Feld `workspaces.modifiers` (`super_alt`, `super_ctrl`, `ctrl_alt`, `ctrl_shift`) |
| Start-Merker (Vorlagen beim Anmelden) | `startup.json` im State-Verzeichnis |

Die Steuerzentrale liest und schreibt nie selbst Dateien, sondern fragt den Dienst über IPC
(`Settings`, `SetSetting`, `Accounts`, `AddAccount…`, `RemoveAccount`, `TestAccount`). So sehen
Tray, Apps und Steuerzentrale immer denselben Stand.

Design, Schrift und Farbschema gelten in allen offenen Fenstern sofort, auf demselben Weg wie hell/dunkel
und der Zoom: Jede App fragt die Einstellungen beim Start (vor dem ersten Bild) und bei jedem Takt, liest
den Teil neu, der sich geändert hat, lädt bei einer neuen Schrift deren Dateien nach und zeichnet neu; nur
das Tray-Symbol bleibt fest (Issue #114). Ist ein Teil weg oder ungültig, zeichnen alle mit dem Standard
dieses Teils, die Einstellung bleibt stehen. Zum Ausprobieren ohne Dienst: `--demo --design <id> --font <id>
--color-scheme <id>` in jeder App, jede id aus ihrem eigenen Ordner (L.22).

## Apps ohne Konto

Der heutige Anmeldebildschirm in Kalender und Aufgaben entfällt. Stattdessen:
„No calendar account yet.“ + „Open Settings“ (öffnet `torroworkplace ui --section accounts`).
Die App wartet und lädt von selbst, sobald ein Konto da ist.

## Bestand (Stand 2026-10-16)

Aufgabe S.27 (#210): jede Einstellung der Suite, am Code nachgesehen. Die Spalte **Urteil** ist mit der Wahl
von Entwurf B entschieden (2026-10-16, S.29) und mit S.16–S.21 gebaut. **„heute“ heißt in diesem Abschnitt: am
2026-10-16, vor Stufe 2.** Wo eine Einstellung jetzt steht, sagt [Was wohin gehört](#was-wohin-gehört).
Abkürzungen: **Seite** heißt Steuerzentrale,
`set_setting x` heißt `settings.set_setting` mit `SettingKey::X` (MCP `settings_set_setting`), MCP-Tools heißen wie
der Befehl mit `_` statt `.`. Ablage: `settings.json` mit JSON-Schlüssel, sonst Datei oder Modul.

### Steuerzentrale: Seitenleiste und Einstieg

Reihenfolge am 2026-10-16 (`Section::ALL`): Overview, Accounts, Workspaces, Apps, Assistant, Agents & API, Approvals,
Notification Hub (`inbox`), Appearance; General am Fuß. Das Zahnrad der Steuerzentrale öffnet General. Die Kurzansicht
(`glance.rs`) hat keine Einstellungen.

Mit S.21 gebaut: Overview, Accounts, Agents & API, Approvals, Workspaces, Appearance; General am Fuß. Die Seiten Apps,
Assistant und Notification Hub sind weg.

### Overview und Approvals

Beide Seiten haben keine Einstellung, nur Anzeigen und Aktionen; sie stehen hier, damit jede Seite aus `Section::ALL`
ihre Zeile hat.

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| – (Status von Kern, Fenstermanager, Konten, Vorlagen) | nur Anzeige | Overview › At a glance | – | `system.status`, `accounts.list_accounts`, `workspaces.list_templates` | alle | bleibt als Anzeige |
| Open setup, Continue setup, Finish setup, Open accounts | führen zur Einrichtung bzw. zu Accounts (Aktion) | Overview › Needs your attention, Setup | `setup` | `set_setting setup` | alle | bleibt als Aktion |
| Allow, Deny | Freigaben entscheiden (Aktion) | Approvals | `approvals.json` | `approvals.decide_approval`, MCP bewusst nie (Punkt 3) | alle | bleibt, Suite |

### General

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Language (System, English, Deutsch) | Sprache der Suite für danach gestartete Programme | General › Language; Einrichtung › Welcome | `language` | `set_setting language` | alle | bleibt, Suite |
| Start at login | Kern beim Anmelden starten | General › Startup | `startAtLogin` + Autostart-Eintrag | `set_setting start_at_login` | alle | bleibt, Suite |
| Run setup again | Einrichtung öffnen (Aktion) | General › Setup; Overview „Open setup“ | `setup` | `set_setting setup` | alle | bleibt, Suite |
| Night (From, Until) | Nacht für Zeitleiste und Radar des Kalenders | General › Calendar; Kalender › Karte am Mond | `night` | `set_setting night` | Kalender | umziehen nach Kalender, doppelt |
| Show reminders | Erinnerungen an Termine an oder aus | General › Reminders | `reminders.enabled` | `set_setting reminders` | Kalender | umziehen nach Kalender |
| Default reminder | Vorlauf für Termine ohne eigene Erinnerung | General › Reminders | `reminders.defaultLead` | `set_setting reminders` | Kalender | umziehen nach Kalender |
| On all displays | Vollbild-Erinnerung auf allen Bildschirmen | General › Reminders | `reminders.allDisplays` | `set_setting reminders` | Kalender | umziehen nach Kalender |
| Sound | Systemton oder still | General › Reminders | `reminders.sound` | `set_setting reminders` | Kalender | umziehen nach Kalender |
| Only events I accepted · All-day events · Events marked free · Outside work hours · Never full screen during a video call | welche Termine erinnern | General › Which events (ohne Erklärungssatz) | `reminders.*` | `set_setting reminders` | Kalender | umziehen nach Kalender, Satz ergänzen |
| je Kalender: Off, Notification, Full screen | Art der Erinnerung je Kalender | General › Calendars | `reminders.calendars` | `set_setting reminders` | Kalender | umziehen nach Kalender, zu „Calendars“ legen |
| Pause reminders (1 h, bis morgen), Resume | Erinnerungen für eine Zeit aus | Tray; Erinnerungsfenster „Pause for 1 hour“ | `<state>/reminders.json` | `calendar.pause_reminders` | Kalender | bleibt (Aktion, kein Ort in den Einstellungen) |

### Appearance

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Color mode (System, Light, Dark) | hell oder dunkel, alle Apps | Appearance › Color mode; Einrichtung › Welcome („Appearance“) | `appearance` | `set_setting appearance` | alle | bleibt, Suite; ein Name für beide Orte |
| Design (Pakete), Reload, Open designs folder | Design-Paket aller Fenster | Appearance › Design | `design`; `<data>/designs/` | `set_setting design`, `settings.list_designs` | alle | bleibt, Suite; #189 teilt es auf |
| Zoom (Zoom out, Zoom in, Reset) | Größe aller Fenster | Appearance › Interface; Strg/⌘ +, −, 0 in jeder App | `zoom` | `set_setting zoom` | alle | bleibt, Suite |
| Show mail as (Formatted, Plain text) | wie eine HTML-Mail im Leser zuerst erscheint | Appearance › Mail | `mailView` | `set_setting mail_view` | Mail | umziehen nach Mail |
| Mail colors (Like the app, Light, Dark) | Farben des Mail-Inhalts im Leser | Appearance › Mail | `mailAppearance` | `set_setting mail_appearance` | Mail | umziehen nach Mail |

### Apps

Die Seite ist mit S.21 weg; die Komponenten stehen als Gruppe **Apps** unter General, mit „Open“ und dem Satz, dass
die Einstellungen einer App hinter ihrem Zahnrad liegen. `--section apps` öffnet General.

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| je Komponente an/aus (Calendar, Tasks, Mail, Notification Hub, Assistant, Workspaces) | Komponente aus Umschalter, Tray und Übersicht nehmen, nichts im Hintergrund | Apps › Components; Einrichtung › Components | `componentsOff` | `set_setting component` | alle | bleibt, General › Apps |
| Open | App starten (Aktion) | Apps › Suite apps | – | `apps.open_app` | – | bleibt als Aktion |

### Accounts

Liste der Konten und „Add account“ (Assistent mit Katalog: Nextcloud, Mail, GitHub, ntfy, Slack bald, AI providers).
Was der Assistent beim Einrichten abfragt (Adresse, Passwort, Server, Schlüssel), ist Eingabe, keine Einstellung, und
steht hier nicht einzeln; Geheimnisse gehen nur über die Oberfläche (`api.md` Punkt 5).

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Calendars and lists (Schalter je Sammlung) | welche Kalender und Listen die Suite zeigt | Nextcloud/CalDAV-Konto › Calendars and lists; Einrichtung › Calendar, Tasks | `hiddenCollections` | `settings.set_collection_shown` | Kalender, Aufgaben | umziehen nach Kalender bzw. Aufgaben |
| Notifications, Talk | Quellen des Eingangs stumm oder an | Nextcloud-Konto › Notification Hub (ohne Satz); Einrichtung › Sources | Regeln in `<state>/inbox.sqlite` | `inbox.mute_source` | Eingang | umziehen nach Notification Hub › Sources |
| Show notifications in the Notification Hub | GitHub-Quelle stumm oder an | GitHub-Konto › Notification Hub; Einrichtung | wie oben | `inbox.mute_source` | Eingang | umziehen nach Notification Hub › Sources |
| Topics (Label, Add topic…, Unsubscribe…) | ntfy-Themen und ihre Etiketten | ntfy-Konto › Topics | `accounts.json` | `accounts.set_ntfy_topic` | Eingang | Abo und Etikett bleiben am Konto; stumm nur je Server in Notification Hub › Sources |
| Login (Change login…) | Benutzer/Token für ntfy | ntfy-Konto › Connection | Schlüsselbund | `accounts.set_ntfy_login`, MCP bewusst nie | – | bleibt am Konto |
| Sender name, Signature | Absendername und Signatur je Mail-Konto | Mail-Konto › Sender name, Signature | `accounts.json` (`MailSettings`) | `accounts.update_mail_account` | Mail | umziehen nach Mail |
| Write new mail as (Formatted, Plain text) | Format beim Schreiben je Konto | Mail-Konto › Writing | `accounts.json` | `accounts.update_mail_account` | Mail | umziehen nach Mail; leicht mit „Show mail as“ zu verwechseln |
| Show new mail in the Notification Hub (All, Only „For me“, Off) | welche neue Mail im Eingang erscheint | Mail-Konto › Notification Hub; Einrichtung › Sources schaltet „Mail“ über `mute_source` | `accounts.json` bzw. Regeln | `accounts.update_mail_account`; `inbox.mute_source` | Eingang | zusammenlegen: `notify` am Konto, Ort Notification Hub › Sources |
| Which folder holds sent mail? … (Use this folder) | offene Ordnerrollen klären | Mail-Konto › Folders (nur wenn offen) | `accounts.json` | `accounts.update_mail_account` | Mail | umziehen nach Mail |
| Services (Mail, Calendars, Tasks) | Dienste eines Mail-Kontos an/aus | Mail-Konto › Services | `accounts.json` | `accounts.update_mail_account` | alle | bleibt am Konto |
| Server settings…, Change password… | Server und Passwort ändern | Mail- und CalDAV-Konto › Connection | `accounts.json`, Schlüsselbund | `accounts.connect_mail`, `accounts.connect_caldav`, MCP bewusst nie | – | bleibt am Konto |
| Sync now, Remove account… | Aktionen | jedes Konto | – | `accounts.sync`, `accounts.remove_account` | – | bleibt am Konto |
| KI-Konto | nur Status, Server, Entfernen; keine Einstellung | Accounts | `accounts.json` | `accounts.connect_ai` (nie), `ai.list_models` | Assistent | umziehen nach AI (S.25) |

### Assistant (Seite und Unterseiten)

Die Seite ist mit S.21 weg: Status, Rechte und die Bots stehen unter Agents & API (`--section assistant`,
`--section bot:<id>`), alles andere im Fenster des Assistenten (General, Profile, Bots, Voice, History). Bis zu 18
Gruppen untereinander: Status, Profile, Bots, Engines, New chats, Model, Effort, What the assistant may use,
Approval, bis zu fünf Auswahlgruppen, Voice, Microphone, Programs on this computer, Keep conversations.

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Turn on assistant (mit „Start from“), Turn off assistant… | Client des Assistenten anlegen bzw. entfernen | Assistant › Status | `clients.json` | `clients.create_client`, `clients.remove_client`, MCP bewusst nie (Punkt 2) | Assistent | umziehen nach Agents & API (Schlüssel nie in eine App) |
| New chats (The one used last, Engine), Model, Effort | womit ein neuer Chat beginnt | Assistant › New chats, Model, Effort; Einrichtung › AI › Use for (nur Engine) | `assistantDefault` | `set_setting assistant_default` | Assistent | bleibt, Assistent |
| Engines (Zeile je KI-Konto) | nur Anzeige, führt zu Accounts | Assistant › Engines | – | `assistant.list_engines` | – | streichen, ersetzt durch Verweis auf AI |
| What the assistant may use (Stufe je Bereich), Approval (Ask first), Auswahl (All/Selected je Kalender, Liste, Repo, Quelle, Mail-Konto), Send on my behalf, Admin-Schalter | Rechte des Assistenten | Assistant › Rechte-Gruppen (`agents::rights_groups`) | `clients.json` | `clients.update_client`, MCP bewusst nie | Assistent | umziehen nach Agents & API |
| Mail to cloud models | Datenregel für Mail, **gilt für die ganze Suite** | Assistant; jede Bot-Seite | `mailToCloud` | `set_setting mail_to_cloud` (anschalten nur Menschen) | alle KI-Nutzer | umziehen nach AI, ein Ort |
| Profile: Name, Description, Picture, Personality | wer der Assistent ist und wie er spricht | Assistant › Profile (Unterseite) | `<config>/assistant/profile.json`, `persona.md` | `assistant.update_profile` | Assistent | bleibt, Assistent |
| Memory: Add note, Edit, Forget | Gedächtnis | Assistant › Profile › Memory | `memory.md` | `assistant.remember`, `assistant.forget` | Assistent | bleibt, Assistent |
| Bots (Zeile je Bot) › What the bot may use … | Rechte eines Bots | Assistant › Bots › Bot; Assistent-App › Bot-Editor › Access (nur Stufen und Ask first) | `clients.json` | `clients.update_client`, MCP bewusst nie | Assistent | umziehen nach Agents & API; der Bot-Editor verweist dorthin |
| Talk to the assistant | Sprache an/aus | Assistant › Voice; Tray „Mute microphone“ (nur aus) | `voice.enabled` | `set_setting voice_enabled` (an nur Menschen) | Assistent | bleibt, Assistent |
| Wake word | nur Anzeige („Hey Torro“) | Assistant › Voice | `voice.wakeWord` | `set_setting wake_word` | – | bleibt als Anzeige |
| Speak answers | Antworten vorlesen | Assistant › Voice | `voice.speakAnswers` | `set_setting speak_answers` | Assistent | bleibt, Assistent |
| Microphone (System default, Gerät) | welches Mikrofon | Assistant › Microphone (ohne Satz) | `voice.microphone` | `set_setting microphone` | Assistent | bleibt, Assistent |
| – (Sprachmodell, Stimme) | whisper- und Piper-Datei | keine Oberfläche, erste im Ordner | `voice.sttModel`, `voice.voice` | `set_setting stt_model`, `set_setting voice` | Assistent | bleibt ohne Oberfläche, bis es mehr als eine Datei gibt |
| Test the microphone, Check again | Prüfen (Aktion) | Assistant › Voice, Programs | – | `settings.test_voice` (nie), `settings.voice_status` | – | bleibt als Aktion |
| Keep conversations (30, 90 Tage, 1 Jahr, Forever) | wie lange Unterhaltungen bleiben | Assistant › Keep conversations | `assistantRetention` | `set_setting assistant_retention` | Assistent | bleibt, Assistent |
| Make PDFs (Schalter je Werkzeug) | ob es ein eigenes Werkzeug auf diesem Rechner gibt, für jeden Client (#225) | neu: Assistent › Tools (WZ.6) | `tools.enabled` | `set_setting tool_enabled` (an nur Menschen, Punkt 9); Liste `tools.list_tools` | Assistent | Assistent |
| Folder (Choose…, Use desktop) | wohin neue PDFs gehen, sonst Schreibtisch | neu: Assistent › Tools | `tools.pdf.folder` | `set_setting pdf_folder` (setzen nur Menschen, `null` jeder mit Admin) | Assistent | Assistent; Dialog des Systems in der App |
| Vorlagen Letter, Report, Note: Paper, Margins, Font size, Header, Footer, Page numbers, Logo | wie eine PDF gesetzt wird | neu: Assistent › Tools | `tools.pdf.letter`, `.report`, `.note` | `set_setting pdf_template` (ganze Vorlage; Logo setzen nur Menschen) | Assistent | Assistent; jede Änderung schickt die ganze Vorlage |
| Welche Werkzeuge ein Client darf | Rechte, Bereich `tools` mit Auswahl | Agents & API (WZ.7) | `clients.json` | `clients.update_client`, MCP bewusst nie | alle Clients | Rechte-Editor (seit #231: Assistent › Access, API, MCP); Tools zeigt es nur an |

### Notification Hub (Seite)

Die Seite ist mit S.21 weg; Notifications, Sources und Rules stehen im Fenster des Notification Hub.
`--section inbox` öffnet General.

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Quiet during busy events | während belegter Termine nur Dringendes | Notification Hub › Notifications; Einrichtung | `quietWhenBusy` | `set_setting quiet_when_busy` | Eingang | umziehen nach Notification Hub |
| Rules (Remove je Regel) | dauerhafte Regeln (Spur, stumm, verbergen) entfernen | Notification Hub › Rules; angelegt im Eingang | `<state>/inbox.sqlite` | `inbox.list_rules`, `inbox.remove_rule` | Eingang | umziehen nach Notification Hub |
| Open | Eingang starten (Aktion) | Notification Hub › App | – | `apps.open_app` | – | streichen (Seite entfällt) |

### Agents & API, Workspaces

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Connect agent (Programm, Name, Read only/Read and write/Full access, Hook) | Client anlegen | Agents & API › Connect agent | `clients.json`, Schlüsselbund | `clients.create_client`, MCP bewusst nie | Entwickler | bleibt, Suite |
| Rechte je Client (Stufen, Admin-Schalter, Send on my behalf, Ask first, Auswahl) | was ein Programm darf | Agent-Seite | `clients.json` | `clients.update_client`, MCP bewusst nie | Entwickler | bleibt, Suite (auch Assistent und Bots) |
| Create new key, Disconnect… | Schlüssel neu, Client entfernen | Agent-Seite › Key | Schlüsselbund | `clients.new_key`, `clients.remove_client`, nie | – | bleibt, Suite |
| Rename, Delete, Load | Vorlagen pflegen | Workspaces; Tray (Load, Save, Update, Delete) | `<data>/templates/` | `workspaces.*` | Workspaces | bleibt, Suite (später eigene App?) |

### In den Apps

Gilt in allen vier Apps: Das Zahnrad und „Settings“ öffneten am 2026-10-16 die Steuerzentrale als eigenes Fenster bei
Accounts (`connect::open_settings`, Befund 1) – Urteil: **ändern**, die Einstellungen der App erscheinen im selben
Fenster. Mit S.18–S.21 gebaut: Das Zahnrad öffnet die Einstellungen im Fenster der App. `connect::open_settings`
führt nur noch dorthin, wo es um Konten geht („Open Settings“ einer App ohne Konto).

#### Kalender

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Night (Karte am Mond) | wie General › Night | Kalender › Zeitleiste, Radar | `night` | `set_setting night` | Kalender | bleibt als schneller Weg |
| Kalender in der Seitenleiste abhaken | blendet einen Kalender aus, **nur bis zum Neustart** | Kalender › Seitenleiste | nur im Speicher | – | Kalender | offen (Offene Frage 3), bleibt bis dahin |

#### Aufgaben

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Order (Due date, Manual) | Reihenfolge einer Liste | Aufgaben › Display | `listOrders` | `set_setting list_order` | Aufgaben | bleibt im Menü der App |
| Group by, Show completed | Ansicht der Liste | Aufgaben › Display | nur im Speicher | – | Aufgaben | bleibt Ansicht |

#### Assistent

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Modell, Effort, Bereiche, Modus je Chat; Show all models | womit dieser Chat antwortet | Assistent › Eingabe | je Unterhaltung im Kern | `assistant.send_message` | Assistent | bleibt im Chat; „Show all models“ fällt mit S.26 |
| Bot-Editor (Name, Description, Picture, Personality, Instructions, Skills, Engine, Access) | ein Bot | Assistent › Bot-Editor | `<config>/assistant/bots/` | `bots.save_bot`; Access über `clients.update_client` | Assistent | bleibt in der App; Access verweist nach Agents & API |

#### Eingang (Notification Hub und App Feed)

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| Mute (Umfang, For, What happens), Hide from Notification Hub › Mute {source}, Move to {lane} › Always for {source}, Insights › Hide sender, Mute app | legt Regeln an bzw. schaltet Quellen stumm | Eingang (alle Fenster) | `<state>/inbox.sqlite` | `inbox.mute_item`, `inbox.mute_source`, `inbox.set_item_lane` | Eingang | bleibt im Eingang; Liste unter Notification Hub › Rules |
| All rules ↗ | öffnete die Seite Notification Hub der Steuerzentrale | Eingang › Mute-Menü | – | – | Eingang | geändert (S.20): Rules im selben Fenster; im Mail-Fenster, das keine Rules hat, holt es das Fenster des Notification Hub nach vorn (`apps.open_app`) |

#### Mail-Fenster

Backup und Wiederherstellung liegen im Mail-Fenster unter **Einstellungen → Konten → Konto**,
nicht in den allgemeinen Einstellungen. Die zuletzt verwendete Quelle, Datumsgrenze und
der Zielordner stehen im `MailSettings.backup` dieses Kontos; Archivname, Testlauf und
Bestätigung gelten jeweils für dieses Konto. Das offene ZIP64-Format und die CLI sind in
[Mail-Backup](mail-backup.md) beschrieben.

| Einstellung | bewirkt | heute | Ablage | API/MCP | wer | Urteil |
|---|---|---|---|---|---|---|
| HTML/Text, Light/Dark am Leser | nur für die offene Mail | Mail-Fenster › Fuß des Lesers | nur im Speicher; Vorgabe `mailView`, `mailAppearance` | – (Vorgabe: `set_setting`) | Mail | bleibt; Vorgabe nach Mail |
| From, Signatur beim Schreiben | Absender der neuen Mail | Mail › Schreiben | nicht gespeichert; Signatur aus dem Konto | – | Mail | bleibt |
| Tastenkürzel | fest „Torro“ | Mail-Fenster (`torro_core::shortcuts`) | Code | – | Mail | geplant (E10.1b, E10.2, E10.3) |

### Geplant, nicht gebaut

| Einstellung | bewirkt | geplant in | Ablage | API/MCP | Urteil |
|---|---|---|---|---|---|
| Modelle je Anbieter an/aus | welche Modelle die Module anbieten | S.24–S.26 (Steuerzentrale › AI) | `<config>/ai-models.json` (neu) | `ai.set_model_enabled` (neu) | bleibt, Suite › AI |
| Keyboard shortcuts (Preset, eigene Tasten) | Kürzel des Mail-Fensters | E10.1b, E10.2, E10.3 (#209) | `settings.json` (`mailShortcuts`) | `set_setting mail_shortcut_preset`, `mail_shortcut` (neu) | bleibt, Mail |
| Design, Schrift, Farbschema getrennt | Aussehen feiner wählen | #189 (Einstellungen, API, MCP: L.22; Seite: L.23) | `settings.json` (`design`, `font`, `colorScheme`) | `set_setting design\|font\|color_scheme`, `settings.list_fonts`, `settings.list_color_schemes` | bleibt, Suite › Appearance |
| Fenster-Profil (Kalender, Listen, Ansicht je Fenster) | mehrere Kalenderfenster nebeneinander | „pro Fenster (später)“ oben | State-Verzeichnis | – | bleibt geplant |

### Startargumente

Kein Ort für Einstellungen: Sie wählen beim Start, was ein Fenster zeigt, oder überschreiben eine Einstellung
**nur für dieses Fenster**; keines schreibt etwas in `settings.json`, keines geht durch eine API.

| Argument | bewirkt | wo | Urteil |
|---|---|---|---|
| `--section <wort>`, `--section bot:<id>` | öffnet die Steuerzentrale bei einem Abschnitt bzw. einem Programm (`Section::from_arg`) | Steuerzentrale | bleibt; mit S.21 führen `apps` und `inbox` auf General, `assistant` und `bot:<id>` auf Agents & API |
| `--setup`, `--setup-if-open`, `--step <schritt>` | öffnet die Einrichtung, auf Wunsch nur solange sie offen ist | Steuerzentrale | bleibt |
| `--settings <abschnitt>` | öffnet eine App mit offenen Einstellungen | alle Apps (S.18–S.20) | bleibt; kein Request, nur ein Startwort |
| `--demo` | Beispieldaten statt Dienst, schreibt nichts | alle | bleibt (Sichtprüfung) |
| `--appearance light\|dark\|system`, `--design <id>`, `--font <id>`, `--color-scheme <id>` | überschreibt `appearance`, `design`, `font` bzw. `colorScheme` für dieses Fenster (die drei Teile nur mit `--demo`, je eine id ihres Ordners) | alle | bleibt (Sichtprüfung) |
| `--mail-view`, `--mail-colors` | überschreiben `mailView` und `mailAppearance` für dieses Fenster | Eingang, Mail | bleibt (Sichtprüfung) |
| `--view`, `--lane`, `--source`, `--account`, `--open`, `--size`, `--now`, `--task` | wählen Fensterart, Ausschnitt, Größe und Zeitpunkt | Apps | bleibt, keine Einstellung |
| `--reminders`, `--all-displays`, `--monitors`, `--cover` | das Erinnerungsfenster, das der Kern selbst startet (`torrocalendar::reminder`) | Kalender | bleibt, keine Einstellung |

Das Tray öffnet mit „Settings…“ die Steuerzentrale ohne `--section` (`apps.open_app` auf `torroworkplace`, ohne
Ziel) und landet damit auf **Overview**; das Zahnrad der Steuerzentrale öffnet General. Das Zahnrad einer App führte
am 2026-10-16 auf Accounts (Befund 1) und öffnet seit S.18–S.21 die Einstellungen im Fenster der App.

### Kein Einstellungs-Ort

Fensterzustand in `ui.json` der App (`torro_gui::ui_state`), geht durch keine API und bleibt in der App: `sidebar`
(alle Apps), `lastRepo` (Aufgaben), `unfolded`, `order` (Newest/Oldest first),
`split` (Breite der Liste), `recentSearches` (alle im Eingang). Ein `density` aus älteren Versionen wird nicht mehr
gelesen oder geschrieben (Issue #226), bleibt aber in der Datei stehen. Ebenso nur im Speicher: Ansicht des Kalenders
(Day … Radar), Filter der Aufgaben, Spalten und Kacheln des Eingangs, „By app/By time“, Suchumfang.

### Befunde

1. **Das Zahnrad führt überall zu Accounts.** In allen Apps und im Mail-Fenster öffnete es die Steuerzentrale bei
   Accounts, auch wo die gesuchte Einstellung woanders steht; in der Steuerzentrale selbst öffnet es General, und
   „Settings…“ im Tray landet auf Overview. Drei Einstiege, drei Ziele. *Mit S.18–S.21 erledigt: Das Zahnrad einer
   App öffnet ihre Einstellungen im eigenen Fenster.*
2. **Doppelt:** Night (General und Karte am Mond), Language, Appearance, Components, Calendars and lists, Sources,
   Quiet und New chats (je Seite und Einrichtung, die Einrichtung ist gewollt), Rechte eines Bots (Bot-Seite und
   Bot-Editor), neue Mail im Eingang auf zwei Wegen (`notify` am Mail-Konto und die Quelle „Mail“ der Einrichtung).
3. **Die Datenregel sieht aus wie ein Recht**, gilt aber für die ganze Suite: Wer sie auf einer Bot-Seite ausschaltet,
   schaltet sie für Assistent und alle Bots aus, ohne dass die Zeile es sagt.
4. **Was eine App betrifft, steht weit weg:** Erinnerungen (13 Zeilen) unter General; Mail-Ansicht unter Appearance;
   Quellen des Eingangs verteilt auf Nextcloud-, GitHub-, ntfy- und Mail-Konto, die Seite Notification Hub hat keine
   Quellen. Absender und Signatur am Konto.
5. **Kalender abhaken in der App vergisst sich:** Die Haken in der Seitenleiste des Kalenders leben nur im Speicher,
   während „Calendars and lists“ am Konto dauerhaft für die Suite gilt. Zwei Bedeutungen, ein Aussehen.
6. **Leicht zu verwechseln:** „Show mail as“ (Leser, Suite) und „Write new mail as“ (Schreiben, je Konto) mit denselben
   Wahlen; „Color mode“ (Appearance) und „Appearance“ (Einrichtung) für dieselbe Einstellung.
7. **Ohne Erklärungssatz:** General › Which events; Assistant › Status (an), Model, Microphone; Profile › Name, Picture;
   Notification Hub-Gruppe am Nextcloud-Konto; Status jedes Kontos.
8. **API/MCP:** Jede Einstellung hat einen Request und, wo nicht bewusst nie (`api.md` Punkte 2, 3, 5, 6), ein
   MCP-Tool. Es fehlt kein Weg für eine gebaute Einstellung. Nur die Haken des Kalenders und der Fensterzustand
   gehen bewusst durch keine API. `inbox.hide_sender` gibt es noch, aber keine Oberfläche ruft ihn mehr.
9. **Abweichungen Konzept–Code:** S.15 („All rules“) ist gebaut, aber nicht abgehakt. Das Konzept nennt die Seite
   „Inbox“, die Oberfläche „Notification Hub“. Das Eingangs-Programm hat drei Fensterarten (Notification Hub, App
   Feed, Mail), das Konzept kennt Eingang und Mail. `ai-models.json` gibt es noch nicht (kommt mit S.24). Im
   Mail-Fenster wird die Listenbreite (`split`) auch mit `--demo` gespeichert, anders als `order`.

**Entwürfe der neuen Struktur (S.28):** https://claude.ai/artifact/5pp7TeLH386QHgp9aHisMy (A · Gespiegelt, B · Schlanke
Zentrale, C · Nach Themen). Gewählt wird mit dem Kommentar „Entscheidung Struktur“ an #210.

## Offene Fragen

1. Slack: welcher Weg (Bot-Token, Benutzer-Token, OAuth) – entscheiden, wenn Slack drankommt.
2. ~~Wie heißt der Knopf „Connect agent“, wenn unter Agents & API auch Assistent und Bots stehen?~~ Beantwortet
   mit S.21: Der Knopf heißt **„Connect program“**, die leere Liste „No programs connected“. Die Frage, wo Rechte
   stehen, ist mit #210 entschieden: zentral unter Agents & API, auch für Assistent und Bots.
3. Die Haken der Kalender in der Seitenleiste der Kalender-App gelten heute nur bis zum Neustart (Befund 5):
   dauerhaft für die Suite wie Calendar › Calendars, oder nur für dieses Fenster (dann als Teil des
   Fenster-Profils, „pro Fenster (später)“)? Bis zur Antwort bleiben sie, wie sie sind.

Entschieden am 2026-10-11: GitHub ohne eigene OAuth-App (CLI-Anmeldung übernehmen oder Token
einfügen). Kalender und Listen werden später pro Fenster gewählt, und die Workspace-Vorlagen
merken sich, welche Fenster mit welcher Auswahl offen waren.
