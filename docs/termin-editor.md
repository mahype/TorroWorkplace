# Termin-Editor: Wiederholung, Erinnerungen, Felder, Ort mit Karte

Stand: 2026-10-22, **umgesetzt in #278 (Layout A); die Abnahme mit Nextcloud und Handy (TE.10) steht aus.** Aufgaben:
[tasks/termin-editor.md](tasks/termin-editor.md). Entwürfe:
[prototypes/calendar/termin-editor.html](prototypes/calendar/termin-editor.html) (Bilder daneben,
`termin-editor-*.png`).

Beantwortet die Frage: **Was muss man an einem Termin einstellen können, und wie sieht das in Torro aus?**
Vorbild sind die großen Kalender, aber aufgeräumter: eine Zeile je Feld, Selteneres erst auf Klick, alles
auch über API und MCP.

## Vergleich (recherchiert am 2026-10-22)

Quellen: Hilfeseiten von Apple, Google und Microsoft, Microsoft Graph und Google Calendar API, Quellcode von
Thunderbird (comm-central) und Nextcloud Calendar (`rfcProps.js`, `SaveButtons.vue`, calendar-js),
Flexibits-Hilfe. „–“ = gibt es nicht, „?“ = nicht belegt.

| Funktion | Apple Kalender | Google Kalender | Outlook (neu) | Thunderbird | Fantastical | **Torro** |
|---|---|---|---|---|---|---|
| Ort mit Vorschlägen | Adresse, Firma; Treffer → Karte + Wetter | Autovervollständigung (Google Maps) | Räume, Orte (Graph strukturiert) | Freitext | frühere und vorgeschlagene Orte | **Suche auf Enter, frühere Orte** |
| Karte im Termin | Apple Maps | Google Maps | ? | – | Karte + Wetter | **kleine OSM-Karte, opt-in** |
| „In Karten öffnen“ | ja | ja | ? | – | Karten, Citymapper | **ja (OSM, wählbar)** |
| URL-Feld | ja | – (in Beschreibung) | – | „Related Link“ | ? | **ja** |
| Notizen | Text | HTML | HTML | Rich Text | ? | Text (wie heute) |
| Teilnehmer | ja | ja | ja | ja | ja | **anzeigen, behalten** (keine Einladungen) |
| Zeitzone je Termin | ja, auch „Floating“ | ja, Start/Ende getrennt | ja (Graph getrennt) | ja, getrennt | ja | **eine Zone je Termin** |
| Status | ? | nur API | „abgesagt“ | 4 Stufen | ? | **bestätigt / vorläufig / abgesagt** |
| Frei/Belegt | ? | Busy/Free | 5 Stufen | Busy/Free | ? | **Busy/Free** (`TRANSP`) |
| Farbe/Kategorie je Termin | – (nur Kalender) | Farbe | Kategorien | Kategorien | Farbe | später |
| Anhänge | ja | ja (Drive) | ja | ja | Premium | später |
| Reisezeit | ja, mit Aufbruch-Hinweis | – | – | – | Premium | später (ER4) |
| Konferenzlink | FaceTime u. a. | Meet | Teams | ? | Premium | erkannt aus Ort/Notizen/URL (K.36) |
| Privat | Exchange | Default/Public/Private | Private | 3 Stufen | ? | später |

**Wiederholung.** Alle zeigen eine **Kurzliste aus dem Startdatum** („Weekly on Tuesday“, „Monthly on the
fourth Tuesday“, „Every weekday“) und „Custom…“ als eigenen Dialog oder Abschnitt mit Intervall,
Wochentagen, „Tag n“ oder „n-ter Wochentag“ und Ende (nie / nach n Mal / am Datum). Niemand zeigt die
RRULE. Fantastical nimmt natürliche Sprache („every Tuesday until 2/5“). Google begrenzt auf 730 Vorkommen.

**Serie ändern.** Google und Nextcloud fragen beim Speichern („This event / This and following / All
events“), Apple kennt nur „nur dieser“ und „alle folgenden“, Thunderbird nur „dieser / alle“. **Outlook
lässt den Umfang vor dem Bearbeiten wählen** („Edit series“, „Edit this and all following“) – das passt zu
Torros Editor, der ohne Speichern-Knopf selbst speichert. „Dieser und alle folgenden“ **teilen** Google und
Nextcloud die Serie: Die alte bekommt `UNTIL` (eine Sekunde vor dem gewählten Termin), die neue eine neue
UID mit `RELATED-TO;RELTYPE=SIBLING`. `RANGE=THISANDFUTURE` (RFC 5545) schreibt Nextcloud nie, weil
andere Programme es schlecht können („The overall support for RANGE=THISANDFUTURE is really bad“,
calendar-js).

**Erinnerungen.** Apple und Thunderbird haben mehrere je Termin, eine Liste fester Zeiten und „Custom“:
n Minuten/Stunden/Tage vor oder nach Beginn oder Ende, oder ein festes Datum mit Uhrzeit. Google begrenzt
auf 5 und 4 Wochen, Outlook kennt nur eine. Ganztägige Termine haben bei Google „1 Tag vorher um 16:50“.

**Aufbau.** Überall schrittweise: Kurzform zuerst, „Mehr“ für den Rest (Apple-Popover, Google „More
options“, Nextcloud-Seitenleiste). Thunderbird zeigt alles auf einmal und wirkt dadurch voll.

### Was Torro übernimmt – und was nicht

- **Übernehmen:** Kurzliste + „Custom…“ für Wiederholungen; drei Umfänge beim Ändern und Löschen, gewählt
  *vor* dem Ändern (Outlook); Teilen der Serie für „folgende“ (Nextcloud, Google); mehrere Erinnerungen mit
  freier Zeit, „am Vortag um …“ und festem Datum (Apple, Thunderbird); URL, Frei/Belegt, Status, Zeitzone;
  Ort mit Karte und „In Karten öffnen“ (Apple).
- **Bewusst nicht:** Einladungen und Antworten (iTIP/iMIP, Abgrenzung des Issues); E-Mail-Erinnerungen
  (bleiben unangetastet im Termin, wie seit ER1.1); getrennte Zonen für Start und Ende (selten, verwirrt);
  Rich Text in Notizen; Autovervollständigung beim Tippen (verbieten die OSM-Dienste, siehe unten);
  natürliche Sprache (eigene Aufgabe, falls gewünscht – braucht Englisch *und* Deutsch).
- **Später:** Farbe/Kategorie je Termin, Anhänge, Reisezeit (mit „Leave now“ aus [erinnerungen.md](erinnerungen.md)
  ER4), Sichtbarkeit (`CLASS`).

## Entscheidungen (mit der Freigabe zu bestätigen)

1. **Server rechnet die Vorkommen weiter aus.** Die Liste kommt wie heute aus `calendar-query` mit
   `<c:expand>`. Für Regel, Zeitzone und Ausnahmen hält der Kern zusätzlich das **Rohobjekt** je `href`
   (geholt mit `calendar-multiget` ohne `expand`, nur wenn sich das ETag geändert hat). Verworfen: selbst
   expandieren (Regel-Bibliothek, Windows-Zonennamen, alle Ansichten und Erinnerungen hängen daran – das
   Risiko trägt das Issue nicht).
2. **Jedes Vorkommen kennt seine Serie.** `Event` bekommt `recurrence_id` (Schlüssel des Vorkommens) und
   `repeat` (die Regel der Serie, auf jedem Vorkommen gleich). Regeln, die der Editor nicht abbilden kann
   (`BYSETPOS`, `RDATE`, `BYHOUR` …), stehen als Text da und bleiben unverändert, solange niemand eine neue
   Regel wählt.
3. **Umfang als Feld, kein neuer Befehl.** `calendar.save_event` und `calendar.delete_event` bekommen das
   optionale Feld `scope` (`occurrence`, `following`, `series`). Fehlt es bei einem Vorkommen, bleibt es wie
   heute bei `unsupported` – ein alter Client kann keine Serie überschreiben.
4. **„Folgende“ teilt die Serie** wie Nextcloud: alte Serie `UNTIL` = gewählter Termin − 1 s (aus `COUNT`
   wird `UNTIL`), neue Serie mit neuer UID und `RELATED-TO;RELTYPE=SIBLING`. **Ausnahmen bleiben**: frühere
   bei der alten Serie, spätere ziehen in die neue um, solange das Raster der Termine gleich bleibt (nur
   Titel, Ort, Erinnerungen … geändert). Ändert sich Zeit oder Regel, gehen spätere Ausnahmen nicht stumm
   verloren: Der Editor sagt vorher „2 changed events after this date are reset“.
5. **„Alle“ verschiebt Ausnahmen mit.** Ändert sich der Beginn der Serie, rücken `RECURRENCE-ID` der
   Ausnahmen und `EXDATE` um denselben Abstand mit (wie Apple und Google); ihre eigenen Änderungen bleiben.
6. **Löschen:** dieser = `EXDATE` (und eine vorhandene Ausnahme fällt weg), folgende = `UNTIL`, alle =
   Objekt löschen (wie heute, mit ETag).
7. **Zeitzonen werden geschrieben.** Neue und geänderte Zeiten gehen mit `TZID` und `VTIMEZONE` an den
   Server, nicht mehr in UTC: Eine wöchentliche Serie um 10 Uhr bliebe sonst nach der Zeitumstellung um 9
   oder 11 Uhr. Standard ist die Zone des Rechners; eine andere Zone ist ein Feld des Termins
   (`time_zone`, IANA-Name). Neue Abhängigkeiten: `chrono-tz` (Zonen) und `rrule` (Vorkommen zählen und
   vorschauen, z. B. für `COUNT` beim Teilen).
8. **Weitere Felder:** `url` (`URL`), `status` (`STATUS`: bestätigt, vorläufig, abgesagt), `geo` (`GEO`,
   gelesen auch aus `X-APPLE-STRUCTURED-LOCATION`; ändert sich der Ort, fällt die Apple-Angabe weg, weil sie
   sonst auf den alten Ort zeigt), Frei/Belegt kommt in den Editor. Abgesagte Termine werden nicht mehr
   übersprungen, sondern gelesen und durchgestrichen gezeigt (K.26); Erinnerungen lösen für sie nicht aus.
9. **Ort und Karte über den Dienst, opt-in.** Die Oberfläche fragt keinen Kartendienst. Zwei neue Befehle
   `calendar.search_places` und `calendar.map_preview`. **Aus, bis ein Mensch es einschaltet** (Einstellung
   `maps`, nur mit dem App-Schlüssel einzuschalten, wie `mail_to_cloud`), weil getippte Adressen und die
   IP-Adresse an Dritte gehen.

## Ort und Karte: Dienste, Regeln, Lizenz

Recherche am 2026-10-22 bei den Betreibern. Torro hat keinen eigenen Server; jede Installation fragt selbst.

| Dienst | Was | Schlüssel | Für viele Desktop-Installationen | Tippen = Suchen | Quelle |
|---|---|---|---|---|---|
| OSMF-Kacheln `tile.openstreetmap.org` | Raster-PNG | nein | erlaubt mit eigenem User-Agent, Cache (≥ 7 Tage oder laut Header), ohne Vorabladen und Offline; Sperre ohne Vorwarnung möglich | – | [Tile Usage Policy](https://operations.osmfoundation.org/policies/tiles/) |
| Nominatim (OSMF) | Geocoding | nein | 1 Anfrage/s **für die ganze App**, „moderate“ Nutzerzahl, Proxy empfohlen | **verboten**, auch im Client | [Nominatim Policy](https://operations.osmfoundation.org/policies/nominatim/) |
| Photon `photon.komoot.io` | Geocoding | nein | „reasonable“, starke Nutzung wird gedrosselt oder gesperrt, keine Zusage | als Funktion beworben, für die öffentliche Instanz nicht geregelt | [photon.komoot.io](https://photon.komoot.io/) |
| OpenFreeMap | nur Vektor | nein | ja, ohne Grenzen | – | [openfreemap.org/tos](https://openfreemap.org/tos/) |
| MapTiler, Stadia, Thunderforest, CARTO | Raster/Vektor | ja | kostenlos meist nur nicht-kommerziell, Kontingente | – | Preisseiten der Anbieter |
| Geoapify, LocationIQ | Geocoding, Karten | ja | kostenlos mit Tageskontingent | ja | Preisseiten der Anbieter |

**Gewählt (Entscheidung 9, Standard, alles änderbar):**

- **Suche:** Photon, nur auf Enter oder Klick auf „Search“, nie beim Tippen; höchstens eine Anfrage je
  Sekunde; Ergebnisse im Cache. Basis-URL einstellbar (eigene Photon-Instanz oder Nominatim-kompatibler Dienst);
  Anbieter mit eigenem Schlüssel kommen später mit Schlüssel im Schlüsselbund, nie in der URL der Einstellung. Nominatim der OSMF nur als bewusste Wahl. Dazu frühere Orte aus
  den eigenen Terminen (lokal, ohne Netz) als erste Vorschläge.
- **Karte:** OSMF-Rasterkacheln, nur geladen, wenn ein Termin mit Koordinaten offen ist; Zoom 16,
  zusammengesetzt im Dienst zu einem PNG (2-fache Dichte), im dunklen Erscheinungsbild abgedunkelt; Cache
  auf der Platte im State-Verzeichnis des Dienstes, `ETag`/`If-None-Match`, mindestens 7 Tage. Kachel-URL
  einstellbar. Antwortet der Dienst mit 403 oder 429 oder gar nicht, bleibt die Karte weg; Adresse und
  „Open in Maps“ bleiben.
- **User-Agent** für beide: `TorroWorkplace/<version> (+https://github.com/mahype/TorroWorkplace)`.
- **Namensnennung (ODbL):** „© OpenStreetMap“ unten rechts in jeder Karte, als Link auf
  `https://www.openstreetmap.org/copyright`; unter den Suchvorschlägen „Search by Photon · © OpenStreetMap
  contributors“; Kachel- und Suchdienst zusätzlich in der Steuerzentrale bei der Einstellung.
- **„Open in Maps“:** `https://www.openstreetmap.org/?mlat=…&mlon=…#map=17/…/…` auf allen Systemen, auf
  Linux `geo:` wenn ein Programm dafür eingetragen ist; wählbar: OpenStreetMap, Google Maps, Apple Maps (nur
  macOS). Ohne Koordinaten sucht der Link die Adresse als Text.
- **Was nicht geht:** Die OSMF kann eine Sperre nicht ohne Update an alle Installationen verteilen; deshalb
  sind beide Dienste abschaltbar und die URLs einstellbar. Keine Verfügbarkeitszusage – das steht in der
  Steuerzentrale bei der Einstellung.

**Datenschutz.** Die Suche schickt den getippten Text, die Sprache der Suite, bei Photon die Koordinaten
von `near` und die IP-Adresse an den gewählten Dienst, die Karte die Koordinaten. Darum: aus, bis ein Mensch es einschaltet; die Einstellung nennt die Dienste mit Link;
Termine aus Einladungen werden nie von selbst gesucht, eine Karte kommt nur für Termine mit Koordinaten.

**Einstellung in der Steuerzentrale** (General › Maps, TE.10): Schalter „Maps and place search“, Adresse der
Ortssuche mit ihrer Art (Photon oder Nominatim), Adresse der Kacheln und „Open in Maps with“ (OpenStreetMap,
Google Maps, Apple Maps nur auf macOS, `geo:` nur auf Linux). Jede Zeile setzt nur ihr Feld
(`settings.set_setting maps`). Die Adressen werden mit Enter gespeichert oder wenn man in der Gruppe
weitermacht (anderes Feld, Schalter, Auswahl), nie bei jedem Tastendruck. Eine Adresse, die der Dienst ablehnt
(`?key=…`, Anmeldung `user@`, Kacheln ohne `{z}`/`{x}`/`{y}`), wird gar nicht erst geschickt: Die Prüfung ist
dieselbe wie im Dienst (`Maps::merged`), der Grund steht leise unter der Gruppe, das Feld behält den Text.
Darunter der Satz, wer was bekommt und dass es keine Verfügbarkeitszusage gibt, und Links zu den Regeln der
Kacheln, zu Photon, zu den Regeln von Nominatim und „© OpenStreetMap contributors“ (Seite `copyright`).

## Datenmodell (neu in `Event`, alles optional, `#[serde(default)]`)

| Feld | iCalendar | Bedeutung |
|---|---|---|
| `recurrence_id` | `RECURRENCE-ID` | Schlüssel des Vorkommens; `None` bei einzelnen Terminen |
| `repeat` | `RRULE` (+ `EXDATE` gezählt) | Regel der Serie: Frequenz, Intervall, Wochentage, Tag im Monat oder n-ter Wochentag, Monat, Ende (nie, Anzahl, Datum), dazu der Regeltext; `None` = keine Serie |
| `time_zone` | `TZID` an `DTSTART` | IANA-Name; `None` = Zone des Rechners (oder UTC beim Lesen) |
| `url` | `URL` | Link zum Termin |
| `status` | `STATUS` | `confirmed`, `tentative`, `cancelled`; `None` = nicht angegeben |
| `geo` | `GEO` | Breite und Länge des Ortes |
| `organizer`, `attendees` | `ORGANIZER`, `ATTENDEE` | nur lesen (K.26), beim Speichern unverändert |
| `exceptions` | `RECURRENCE-ID` der Ausnahmen | nur lesen: geänderte Vorkommen der Serie, vom Dienst aus dem Rohobjekt gesetzt; daraus zählt der Editor vorab, wie viele „folgende“ zurücksetzt (Entscheidung bei der Umsetzung, 2026-10-22) |

`recurring` bleibt (verträglich), ist `true` genau dann, wenn `recurrence_id` oder `repeat` gesetzt ist.

### Nach dem Review (2026-10-22)

- **Regel beenden nur ausdrücklich:** `repeat: None` beendet eine Serie (Umfang `series` oder `following`)
  nur zusammen mit `recurring: false`. Mit `recurring: true` bleibt die gespeicherte Regel – ein Vorkommen,
  das ohne Regel beim Client ankam (Rohobjekt nicht zur Hand), macht die Serie beim Zurückschicken nie zum
  Einzeltermin. Termine außerhalb des Cache-Fensters holt der Dienst samt Rohobjekt (`calendar-multiget`),
  damit `repeat`, `time_zone` und `exceptions` auch dort gesetzt sind.
- **Regeltext nur lesen:** `repeat.rule` schreibt der Dienst nie; er schreibt die gespeicherte Regel
  (`custom`, oder Felder unverändert) oder eine aus den Feldern. Nur ein neuer Termin mit eigener Regel
  (`custom`, noch keine gespeichert) bekommt den Text – wenn er eine Zeile der RRULE-Grammatik ist (keine
  Leerzeichen, genau ein gültiges `FREQ`, `COUNT`/`INTERVAL` positive Zahlen, `UNTIL` als Datum oder
  Zeitpunkt, `BY…`-Listen mit gültigen Werten, `X-NAME=wert` ohne `:`/`;`/`"`). Sonst `error`, auf jedem
  Weg, bevor etwas geschrieben wird – außer der Text ist genau die gespeicherte Regel: Eine Serie, deren
  gespeicherte Regel die Prüfung nicht besteht (ohne `FREQ`, `X-`-Teil mit `:` …), lässt sich weiter
  ändern; der Dienst vergleicht dazu mit dem Rohobjekt im Cache oder liest es vom Server.
- **Antwort:** immer das geschriebene Vorkommen mit seinem neuen `recurrence_id`, nach „folgende“ mit `uid`
  und Regel der neuen Serie; liegt es außerhalb des Fensters, baut der Dienst es aus dem geschriebenen
  Objekt.
- **„Folgende“ auf einer verschobenen Ausnahme** (zweites Review): Auf die neue Serie geht nur, was der
  Client gegenüber dem gespeicherten Vorkommen (der Ausnahme) geändert hat; was die Ausnahme selbst
  geändert hatte (Zeit, abgesagt, Titel, Notizen, Erinnerungen, Länge), bleibt ihr eigenes und bleibt bei
  ihr. Zeit: Lässt der Client Beginn und Länge wie gespeichert, bleibt die neue Serie im Raster der alten
  (10:00 → 14:00 verschoben, nur neuer Titel: Serie um 10:00, nichts zurückgesetzt, spätere `EXDATE`
  bleiben, kein Löschrecht nötig). Ändert er sie, beginnt die neue Serie bei seiner Zeit, gemessen vom
  Raster (14:00 → 15:00: Serie um 15:00), und spätere Ausnahmen werden zurückgesetzt. Kern (`drops`,
  `resets`) und Editor (Band) zählen mit derselben Bedingung.
- **„Folgende“ und Zusatztermine** (Review von PR #294): `RDATE` anderer Programme ab dem Termin ziehen in
  die neue Serie und rücken mit ihr, wie bei „alle“. An einem `RDATE`, das kein Datum der Regel ist, teilt der
  Dienst nicht (`unsupported`): die Regel würde sonst auf diesem Datum neu verankert (dienstags 10:00 würde
  donnerstags 15:00). Gezählt wird ab dem Termin, auch in Serien, die vor mehr als zehn Jahren begannen; der
  Rest eines `COUNT` ebenso. Wo der Dienst nicht so weit zählen kann (über 5000 Termine), teilt er nicht.
- **Zählhorizont:** Termine zählt der Dienst bis zehn Jahre (höchstens 5000). Eine neue Regel, die danach
  früher endet, und ein späterer Beginn unter einem festen `UNTIL` jenseits davon zählen als gelöscht
  (mindestens 1). Daten außerhalb der Jahre 1 bis 9999 sind `error`.
- **Umfang wechseln im Editor:** Was unter dem bisherigen Umfang getippt und noch nicht gespeichert ist,
  speichert der Editor vor dem Wechsel mit diesem Umfang; ist es noch nicht speicherbar oder läuft ein
  Speichern, bleibt der Umfang, bis es gespeichert ist.
- **Um Tage verschieben:** Bleibt `BYDAY`/`BYMONTHDAY` stehen, ist das zulässig (RFC 5545: `DTSTART` zählt
  dann als zusätzliches Vorkommen); was wegfällt, zählt als gelöscht (Löschrecht, Freigabe). Der Editor
  schickt die Regel mit dem neuen Wochentag mit.
- **Windows-Zonen:** Die häufigsten Windows-Namen (`W. Europe Standard Time` …) gelten als ihre IANA-Zone;
  Serien in unbekannter oder ohne Zone zählt der Dienst in der Zone des Rechners, in der er sie liest.
- **Umzug einer Serie** in einen anderen Kalender: Lehnt der Server das Löschen des alten Objekts ab
  (HTTP-Fehler, z. B. 412/403), löscht der Dienst das neue wieder; ebenso beim Teilen („folgende“), wenn der
  Server das Kürzen der alten Serie ablehnt. Kommt keine Antwort (Verbindung weg, Zeitüberschreitung),
  nimmt er nichts zurück – der Server hat den Schritt vielleicht schon getan, und Zurücknehmen könnte Termine
  verlieren. Er meldet den Fehler; der nächste Sync zeigt beide Objekte, wie der Server sie hält.
- **„Never“ nur mit „All“:** Der Editor schickt das Ende einer Serie (`recurring: false`) nur mit dem
  Umfang „All“. Wechselt der Umfang, kommt die gespeicherte Regel zurück; solange das Speichern läuft, das
  die Serie beendet, bleibt der Umfang stehen.

## Oberfläche

Drei Layouts liegen als Entwurf vor (`?layout=a|b|c`), jedes mit eigenem Vorschlag für Wiederholung und
Karte. **Welches gilt, entscheidet der Mensch in der Planung** (Ergebnis unten in „Auswahl“).

- **A – Eigenschaften (Linear):** eine Zeile je Feld wie heute (`widgets::prop_line`), bearbeitet wird in
  schwebenden Karten. Wiederholung: Menü mit Kurzliste aus dem Startdatum, „Custom…“ öffnet eine Karte mit
  Intervall, Wochentagen, Ende und dem Satz „Every week on Tuesday, until 22 Dec 2026 · 9 events“. Karte:
  336 × 104 unter der Adresse, Knopf ↗ „Open in Maps“.
- **B – Abschnitte:** alles offen im Paneel (Time, Repeat, Place, Reminders, Details, Notes), Eingaben
  direkt sichtbar. Wiederholung: Segmente None/Day/Week/Month/Year, darunter Wochentage oder „Tag 27 / der
  vierte Dienstag“, Ende als Segment. Karte: 352 × 140 mit Knopf „Open in Maps“, Suchvorschläge unter dem
  Feld.
- **C – Kompakt:** Karte als Kopf des Paneels (400 × 132), Chips unter dem Titel (Wiederholung,
  Erinnerungen, Kalender, Busy, Status, Zone), Notizen groß. Wiederholung: Karte mit Textzeile („every week on
  tue until 22.12.“), Segmenten und Monatsblatt, das die Termine und Ausnahmen zeigt.

Für alle gleich: Termine einer Serie zeigen unter dem Kopf das Band **„Changes apply to: This event ·
Following · All“** (Standard „This event“); jede Änderung speichert sofort mit diesem Umfang. „Delete event“
fragt im Fuß nach dem Umfang. In Schmal steht das Paneel wie heute über dem Kalender.

### Auswahl

**Layout A** (in der Planung am 2026-10-22 entschieden). Dazu: Karte und Ortssuche opt-in mit Photon und
OSM-Kacheln, Teilnehmer nur anzeigen. B und C bleiben als Entwurf liegen; die Monatsblatt-Vorschau aus C und
die Segmente aus B sind Ideen für später.

## Oberfläche, API, MCP

| Funktion | Oberfläche | API | Zugriff | MCP |
|---|---|---|---|---|
| Serie anlegen, Regel ändern | Editor „Repeat“ | `calendar.save_event` (Feld `repeat`) | Write | `calendar_save_event` (kurz: `repeat`) |
| Vorkommen, folgende, alle ändern | Band „Changes apply to“ | `calendar.save_event` (Feld `scope`) | Write | `calendar_save_event` (`scope`) |
| Serie kürzen, Regel entfernen, Ausnahmen zurücksetzen | Editor „Repeat“, Band | `calendar.save_event` | Write **und Löschrecht** (Freigabe wie Löschen) | `calendar_save_event` |
| Vorkommen, folgende, alle löschen | Fuß „Delete event“ | `calendar.delete_event` (Feld `scope`) | Delete | `calendar_delete_event` (`scope`) |
| Erinnerungen mit freier Zeit | Editor „Reminders“ | `calendar.save_event` (Feld `alarms`, gibt es) | Write | `calendar_save_event` (kurz: `reminder_minutes`, gibt es) |
| URL, Frei/Belegt, Status, Zeitzone, Koordinaten | Editor | `calendar.save_event` | Write | `calendar_save_event` (kurz: `url`, `status`, `time_zone`, `free`) |
| Ort suchen | Editor „Location“ | `calendar.search_places` | Read | `calendar_search_places` |
| Karte zeigen | Editor | `calendar.map_preview` | Read | `calendar_map_preview` (Bild; nicht für den Assistenten: das PNG passt in kein Tool-Ergebnis) |
| „Open in Maps“ | Editor, Knopf ↗ unter der Karte | – (kein Befehl) | – | – (kein Tool) |
| Karten und Suche ein/aus, Dienste | Steuerzentrale › General › Maps | `settings.set_setting` (Schlüssel `maps`) | Admin; einschalten nur App-Schlüssel | `settings_set_setting`: Clients mit Admin dürfen ausschalten und `geocoderKind`/`openWith` setzen, nie einschalten |

„Open in Maps“ ist kein Befehl: Die App baut aus `geo` bzw. `location` einen Link und öffnet ihn lokal im
Browser oder Kartenprogramm; der Dienst liest und schreibt dabei nichts. Ein Agent hat `geo` und `location`
aus `calendar.list_events` und baut denselben Link selbst, wenn er ihn braucht.

Freigaben: Die Zusammenfassung nennt den Umfang aus dem Cache („Change all events of “Weekly product sync”“,
„Delete “Weekly product sync” on Tue, 27 Oct and all following“). Protokoll ohne Suchtext und ohne
Koordinaten.

## Später

- Natürliche Sprache für Wiederholungen (Idee aus Layout C), Farbe/Kategorie je Termin, Anhänge, Reisezeit,
  Sichtbarkeit – je ein eigenes Issue.
