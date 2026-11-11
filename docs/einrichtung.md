# Einrichtung beim ersten Start

Stand: 2026-11-10, Issue #326. Entwürfe: [prototypes/onboarding/](prototypes/onboarding/index.html), eine Seite je
Fenster; sie gelten als Zielbild. Älterer Entwurf zur KI-Verbindung: [ai-connect.html](prototypes/onboarding/ai-connect.html).

Wer Torro Workplace zum ersten Mal startet, wird durch **fünf Punkte** geführt, die oben als Schrittzeiger
stehen: **Mail und Kalender, Cloud, KI, Aussehen, Dokumente**. Davor eine Begrüßung, danach eine Übersicht.
Jeder Punkt sagt in einem Satz, wozu er da ist, in einem Kasten „Why this matters.“, warum er wichtig ist,
und fragt nur das Nötigste ab.

## Grundsätze

1. **Nichts ist Pflicht.** Jeder Punkt lässt sich überspringen. Ohne Nextcloud geht alles; Kalender und
   Aufgaben kommen dann mit dem Mail-Konto.
2. **Was verbunden ist, ist gespeichert.** Die Einrichtung ist ein geführter Weg durch dieselben Requests wie
   die Steuerzentrale, kein eigener Speicher. Wer mittendrin schließt, verliert nichts.
3. **Keine zweite Umsetzung.** Der Mail-Weg ist derselbe wie unter Accounts › Add (`wizard.rs`), nur auf der
   Seite statt im Dialog; die KI-Seite ist dieselbe wie unter Accounts › Add › AI (`ai_setup.rs`); das Aussehen
   nimmt die Bausteine von Settings › Appearance (`suite.rs`). Nextcloud ist ein Baustein in `ui/connect/`.
4. **Open First.** Lokale KI steht oben und wird von selbst gesucht; der Vorschlag für diesen Rechner ist
   lokal, wo es geht ([ki.md](ki.md), Grundsatz 4).
5. **Geheimnisse nur in der Oberfläche.** Passwörter und Schlüssel werden nur in Felder der Einrichtung
   getippt. Ein Agent kann die Einrichtung nicht stellvertretend durchlaufen (die `connect_*`-Befehle mit
   Geheimnis bleiben `Never`).

## Wann sie erscheint

| Anlass | Was passiert |
|---|---|
| Dienst startet, **kein Konto** und Einrichtung nie abgeschlossen | Der Dienst öffnet `torroworkplace ui --setup` (eine offene Steuerzentrale kommt nach vorn), bei jedem Start, bis sie abgeschlossen oder verschoben ist |
| „Set up later“ in der Einrichtung | Stand `skipped`; Overview zeigt oben die Karte „Finish setup“ mit „Continue setup“ |
| App ohne Konto (`torro-gui/src/connect.rs`) | Seit #410 je Anbieter ein Knopf, der „Add account“ in der App öffnet; die Einrichtung startet nicht. `--setup-if-open` bleibt für Aufrufer von außen |
| General › „Run setup again“ | Startet bei der Begrüßung; Verbundenes steht schon da, gewählte Modelle werden gezeigt |
| Neue Version bringt neue Schritte | `Setup::VERSION` steigt; Overview: „New in setup“, nicht ungefragt öffnen |

Gespeichert wird nur der Stand in `settings.json`: `setup: { state: pending | skipped | done, version }`. Mit
`version` erkennt die Suite, dass es seit dem letzten Durchlauf Neues gibt (4: die fünf Punkte, #326; 3: Documents,
#303; 2: Web search). `--step` nimmt `welcome`, `mail`, `cloud`, `ai`, `appearance`, `documents`, `done`; die Wörter
von früher führen zum Punkt, der sie jetzt enthält (`nextcloud` → Cloud, `calendar`, `tasks`, `inbox` → Mail,
`components` → Welcome, `web` → AI).

## Fenster

Ein **eigener Modus der Steuerzentrale** (`torroworkplace ui --setup`), kein Dialog und ohne Seitenleiste:

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ [T] Setup   ✓ Mail and calendar ─ ✓ Cloud ─ (3) AI ▬▭▭ ─ 4 Appearance ─ 5 Documents   Set up later │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│  Titel, ein Satz, Kasten „Why this matters.“, Gruppen der Seite (höchstens 640 breit)    │
├──────────────────────────────────────────────────────────────────────────────────────────┤
│ (Status / Fehler / leiser Link)                          Back   Skip   [Continue]        │
└──────────────────────────────────────────────────────────────────────────────────────────┘
schmal (< 600):  ‹  3/5 AI · 1/3  ▬▬▬▭▭                                    Later
                 Knöpfe untereinander, der Primärknopf oben
```

- **Schrittzeiger** im Kopf (`size::header()`): Logo, „Setup“, die fünf Punkte mit Nummer und Namen, durch eine
  Linie verbunden; erledigte mit Haken, der aktuelle in `TEXT` und mittlerem Gewicht, nie rot. Jeder Punkt ist
  anklickbar. Auf dem Punkt KI zeigt ein kleiner Balken, auf welcher der bis zu drei Unterseiten man steht.
  Rechts „Set up later“ (auf der Übersicht nicht: dort ist „Finish“ der Ausweg).
- **Mittel** (< 1024): nur die Nummern, die Namen als Tooltip.
- **Schmal** (< 600): „‹“, „3/5 · AI“ (auf KI mit „· 1/3“), ein Fortschritt `progress_bar`, „Later“; im Fuß stehen
  die Knöpfe untereinander.
- **Inhalt** höchstens `size::form_max()` breit (Aussehen so breit wie das Fenster, die Vorschau steht neben der
  Wahl): Titel `typo::headline()`, ein Satz in `TEXT_2`, der Kasten, dann Gruppen.
- **Fuß** mit Linie: links Status, Fehler oder ein leiser Link, rechts „Back“, der Weg zum Überspringen,
  weitere Wege des Punkts und **genau ein** Primärknopf. Enter = Primärknopf, Escape = Dropdown, Dialog oder
  Formular schließen, im Mail-Weg und auf den KI-Unterseiten zurück, sonst „Back“.

## Die Seiten

| Seite | Inhalt | Fuß |
|---|---|---|
| **Willkommen** | „Set up Torro Workplace“, die fünf Punkte mit je einem Satz, nur die Sprache (vom System, änderbar) | „Get started“ |
| **1 Mail und Kalender** | Kasten: ein Konto bringt Kalender und Aufgaben des Anbieters mit. „Add an account“: der Mail-Weg auf der Seite (Adresse → Anbieter erkannt → Passwort mit Hinweis auf App-Passwörter → Dienste). Gruppe „Connected“ mit jedem Konto und „Mail · 2 calendars · 1 task list“ | links „Take over accounts from TorroMail“ (Dialog); „Skip“, „Continue“ bzw. der Knopf des Mail-Wegs; danach „Add another account“ |
| **2 Cloud** | Der Server für Dateien, Kalender und Kontakte, heute Nextcloud. Adresse, Anmeldung im Browser, danach „What this server offers“ (Kalender, Aufgaben, Benachrichtigungen und Talk, Deck und Notes) und „Add another Nextcloud“ | „I don't use one“, „Connect“ bzw. „Continue“ |
| **3.1 KI · Connect** | Der gemessene Rechner, gefundene lokale KI (Ollama, LM Studio, Strata), Cloud über API-Schlüssel (ChatGPT erst mit #324), „Our suggestion for this PC“ mit Modell für Chat und für Sortieren. Sind schon Modelle gewählt, die Gruppe „Chosen“ | „Skip AI“, „Choose myself“, primär **„Use suggestion“** (springt direkt zu Aussehen, der Download läuft weiter); mit gewählten Modellen primär „Keep my choice“, „Use suggestion“ nur, wenn der Vorschlag abweicht |
| **3.2 KI · Chat** | „Model for chat“: Dropdown der Modelle mit Werkzeugen, nach Konto gruppiert, der Vorschlag markiert; ein noch nicht geladener Vorschlag als Zeile mit „Use“. Die Wahl geht sofort in den Platz Chat | „Back“, „Continue“ |
| **3.3 KI · Sortieren** | Kasten: Sortieren sieht jede neue Mail. Vier Wege: Entscheidermodell hier (Vorschlag, Dropdown), Jev (ohne Konto „Add key“), „Use the chat model“, „Rules only“. Ein Modell woanders fragt vorher (Zustimmung nur fürs Sortieren, #424) | „Back“, „Continue“ |
| **4 Aussehen** | Dieselben Bausteine wie Settings › Appearance: Farbschema als Dropdown mit Farbmodus (System, Light, Dark), Abstände als Segment (Comfort, Standard, Dense; #484), Schrift als Dropdown; daneben die Vorschau des Mail-Fensters | „Keep the default“, „Continue“ |
| **5 Dokumente** | Nur ein Hinweis: Torro arbeitet mit ArtCraft (WordCraft, DeckCraft, GridCraft, PdfCraft); Torro selbst bearbeitet nie Dateien; „Get ArtCraft“; eingerichtet wird unter Settings › MCP servers. Speichert nichts | „Continue“ |
| **Fertig** | Eine Zeile je Punkt mit Zustand und „Open …“ (Mail, Calendar, Assistant, Settings) bzw. „Set up“ oder „Get ArtCraft“ | links „Connect an agent…“ (MCP servers), „Finish“ |

„Choose myself“ führt über Chat und Sortieren; „Use suggestion“ lässt beide aus. Bei „Run setup again“ zeigt die
KI-Seite die bisherige Wahl; ein abweichender Vorschlag wird nur auf Klick übernommen.

### KI-Verbindung (#527, #587)

Die KI-Seite und Accounts › Add › AI zeigen denselben Verbindungs- und Vorbereitungsweg ([Programme und
Datenregel](ki.md)). Die Suche läuft beim Öffnen und alle fünf Sekunden nur auf der sichtbaren Seite; Formulare
und Verbindungen pausieren sie. Ohne Fund bleiben Online-Dienst und lokales Programm gleichwertig (`small` oder
unbekannt: online zuerst; ab `medium`: lokal zuerst). Nur Ollama lädt auf bestätigten Klick; bei anderen Programmen
wird das Modell genannt und sein Erscheinen geprüft. „Ollama Cloud“ steht im Anbietermenü nach OpenRouter, mit
„Cloud · US“, maskiertem Schlüssel und „Create an API key“; es schlägt `gpt-oss:20b` für Chat und `clef:27b` fürs
Sortieren vor. Formular und Zustimmung lassen sich nicht mit Enter oder dem Primärknopf übergehen. Läuft nach „Use
suggestion“ noch ein Download, geht die Einrichtung weiter; die Übersicht zeigt „Preparing model…“.

## Was weggefallen ist

Die Schritte Components, Calendar, Tasks, Inbox und Web search (#326). Welche Apps sichtbar sind, leitet „Finish“
aus den Verbindungen ab: Mail ⇔ Mail-Konto, Calendar ⇔ eine Sammlung mit Terminen, Tasks ⇔ eine Aufgabenliste oder
GitHub, Assistant ⇔ ein KI-Konto; Notification Hub und Workspaces bleiben, wie sie sind. Beim ersten Durchlauf
(Stand `pending`, `setup.version` 0) blendet „Finish“ Apps ohne Verbindung aus und mit Verbindung ein; sonst (nach
„Run setup again“ oder nach „Set up later“, das die Version schon festhält) blendet es nur ein, nie aus, weil sonst
eine bewusste Wahl unter Apps verloren ginge. Ein Nextcloud-Konto, dessen Dienste noch gelesen werden, ein
CalDAV-Konto und ein Mail-Konto mit CalDAV-Adresse zählen vor dem ersten Abgleich schon für Kalender und Aufgaben. Kalender, Listen und Eingangsquellen sind
standardmäßig an; die Websuche bleibt aus und wird nur noch unter MCP servers › Built-in tools eingeschaltet. Alles
bleibt in den Einstellungen änderbar.

## Komponenten an und aus

Eine Einstellung je Komponente (`calendar`, `tasks`, `mail`, `inbox`, `assistant`, `workspaces`), Standard **an**.
Eine abgeschaltete Komponente erscheint nicht im Tray-Menü und in der Overview (Apps zeigt sie mit „Turned off“,
„Turn on“ im Menü „…“; Workspaces unter General › Startup), verursacht keine Hintergrundarbeit, die nur ihr dient,
und behält ihre Konten und Daten. Ihre Requests antworten weiter.

## API und MCP

Die Einrichtung bekommt **keinen** eigenen Befehl: Sie ist eine Oberfläche über vorhandene Requests.

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Stand der Einrichtung | Start, Overview | `settings.get_settings` (`setup`) · Read | `settings_get_settings` |
| Abschließen, verschieben, neu starten | „Finish“, „Set up later“, „Run setup again“ | `settings.set_setting` (`setup`) · Admin | `settings_set_setting` |
| Apps aus den Verbindungen | „Finish“ | `settings.set_setting` (`component`, ein und aus) · Admin | `settings_set_setting` |
| Sprache | Willkommen | `settings.set_setting` (`language`) · Admin | `settings_set_setting` |
| Mail-Anbieter finden | Punkt 1 | `accounts.discover_services` · Read | `accounts_discover_services` |
| Mail verbinden, aus TorroMail übernehmen | Punkt 1, Dialog | `accounts.connect_mail`, `accounts.import_torromail_account` · Admin, nur App-Schlüssel; `accounts.list_torromail_accounts` · Read | **Never** (Passwort nur in der Oberfläche) |
| Nextcloud verbinden | Punkt 2 | `accounts.connect_nextcloud` · Admin | `accounts_connect_nextcloud` (Anmeldung im Browser macht der Mensch) |
| KI verbinden | Punkt 3 (Jev auf 3.3) | `accounts.connect_ai`, `accounts.connect_local_ai` · Admin, nur App-Schlüssel | **Never** (Datenziel und Schlüssel nur durch einen Menschen, auch lokal) |
| Vorschlag, Modelle, Plätze | Punkt 3 | `ai.recommend`, `ai.find_local_servers`, `ai.list_models`, `ai.list_pulls`, `ai.list_assignments` · Read; `ai.pull_model`, `ai.cancel_pull` · Write; `ai.set_model_enabled`, `ai.set_slot`, `ai.set_assignment` · Admin | gleichnamig (`ai_recommend`, `ai_set_slot` …) |
| Aussehen | Punkt 4 | `settings.list_designs`, `settings.list_fonts`, `settings.list_color_schemes` · Read; `settings.set_setting` (`appearance`, `design`, `color_scheme`, `font`) · Admin | `settings_list_designs` …, `settings_set_setting` |
| ArtCraft vorstellen | Punkt 5, Fertig | `extensions.list_catalog`, `extensions.list_extensions` · Read; speichert nichts | `extensions_list_catalog`, `extensions_list_extensions` |
| Apps öffnen | Fertig („Open …“) | `apps.open_app` · Write | `apps_open_app` |

## Entscheidungen

Bestätigt mit der Annahme des Plans zu #326 (2026-11-10):

1. ChatGPT-Anmeldung (#324) und Claude Code/Codex (#609) kommen mit eigenen Plänen; bis dahin nur der API-Schlüssel.
2. Punkt 2 heißt „Cloud“, Aussehen kommt vor Dokumente.
3. Dokumente richtet nichts ein, sondern verweist auf Settings › MCP servers.
4. Aussehen nutzt die Bausteine von Settings › Appearance mit der Vorschau des Mail-Fensters.
5. Die Fertig-Seite bleibt; „Finish“ schließt dort ab.
6. „Run setup again“ zeigt die bisherige Wahl der Modelle; ein abweichender Vorschlag nur auf Klick.
7. Mail wird auf der Seite verbunden, derselbe Weg wie Accounts › Add; die Übernahme aus TorroMail bleibt ein Dialog.
8. „Finish“ leitet die sichtbaren Apps aus den Verbindungen ab, beim ersten Durchlauf auch ausblendend.

Ältere Entscheidungen (2026-10-14) gelten weiter: Komponente aus betrifft Sichtbarkeit und Hintergrundarbeit;
`AiProvider` hat je Anbieter eine Variante; chinesische Anbieter mit internationalen Endpunkten als Standard.
