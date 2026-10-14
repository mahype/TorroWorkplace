# Einstellungen: die Steuerzentrale

Stand: 2026-10-11. **Konzept zur Freigabe.**

Alles, was die Suite als Ganzes betrifft, wird an **einer** Stelle eingestellt: in der
Steuerzentrale des Kerns (`torroworkplace ui`). Die Apps (Kalender, Aufgaben, später weitere)
haben keine eigenen Kontoeinstellungen. Fehlt ihnen ein Konto, zeigen sie einen Hinweis mit dem
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
| **Accounts** | Liste der Konten als Karten, „Add account“, Detailseite je Konto (siehe unten) | v1 |
| **Workspaces** | Vorlagen: umbenennen, löschen, Tastenplatz F1–F10, beim Anmelden laden; Strategie je Fenster | v1 (Liste, Umbenennen, Löschen), Rest später |
| **Apps** | Die Apps der Suite: installiert ja/nein, öffnen, App-ID für Fensterregeln | v1 |
| **General** | Sprache (System / English / Deutsch), Start beim Anmelden, Tray-Icon zeigen; unter „Appearance“ hell/dunkel, Zoom und das Design-Paket ([design-pakete.md](design-pakete.md), L.4) | v1 |
| **Agents & API** | Agenten und Skripte verbinden (wie TorroMail: Karte je Client, eigener Schlüssel), Rechte je Bereich und Sammlung, Freigaben. Konzept: [api.md](api.md) | später |
| **Updates** | Automatisch prüfen, „Check for updates now“, zuletzt geprüft | später |
| **Diagnostics** | Log-Datei, Sync-Protokoll, Verbindung testen | später |
| **Help** | Kurzanleitung, Tastenkürzel, Links | später |

## Konten

### Anbieter-Katalog

„Add account“ öffnet zuerst die Auswahl des Anbieters, dann den Assistenten des Anbieters.

| Anbieter | Wofür | Anmeldung | |
|---|---|---|---|
| **Nextcloud** | Kalender, Aufgaben; später Kontakte, Dateien, Deck, Notizen, Talk | Login Flow v2 im Browser, App-Passwort im Schlüsselbund (gibt es schon im Kern) | v1 |
| **GitHub** | Issues und Projekte lesen; später Pull Requests, Reviews, Benachrichtigungen | Ohne eigene OAuth-App: **(a) Anmeldung der GitHub CLI übernehmen**, wenn `gh` angemeldet ist (Token per `gh auth token`, einmal bestätigen; für Projekte `gh auth refresh -s read:project`), oder **(b) Token einfügen** (Personal Access Token, nur Lesen; Link zur Token-Seite mit vorausgewählten Rechten). Token im Schlüsselbund. OAuth Device Flow mit eigener App höchstens später | nächste |
| **Slack** | Erwähnungen, Direktnachrichten, ausgewählte Kanäle | Weg noch offen (Token statt OAuth wahrscheinlich); Token im Schlüsselbund | später |
| **ntfy** | Themen abonnieren für den Eingang (CI, Uptime Kuma, Skripte) | Server (leer = `ntfy.sh`) und Themen mit Etikett; nur wenn ein Thema es braucht Benutzer und Passwort oder Token `tk_…` (`accounts.set_ntfy_login`, nur über `https` oder an diesen Rechner), im Schlüsselbund. Themen-Namen sieht nur die Steuerzentrale, Agenten das Etikett (E5.12) | E5 |
| **KI-Anbieter** (Anthropic, OpenAI, lokal) | Zusammenfassen, Agenten in der Suite | API-Schlüssel (maskiertes Feld, „Key stored“, entfernen), lokal: Adresse | später |

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
das Paket, lädt seine Schriften nach und zeichnet neu; das Tray-Symbol wechselt mit. Ist das Paket
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
