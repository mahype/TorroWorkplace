# Aufgaben: Termin-Editor – Wiederholung, Erinnerungen, Felder, Ort mit Karte (TE.1–TE.10)

Konzept: [termin-editor.md](../termin-editor.md). Entwurf: [prototypes/calendar/termin-editor.html](../prototypes/calendar/termin-editor.html)
**Layout A** (`?layout=a`, Zustände `repeat`, `remind`, `delete`; Bilder `termin-editor-a*.png`). Issue #278.
Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`. Wer einen Request anfasst, lädt
`torro-api`; nach jeder Funktion prüft `torro-api-review`. Wer iCalendar schreibt, lädt `nextcloud` (Agent
`nextcloud-expert` geeignet). Nach jeder UI-Änderung prüft `torro-design-review`.

**In der Planung entschieden (2026-10-22):** Layout A; Karte und Ortssuche opt-in (Photon + OSM-Kacheln,
einstellbar); Teilnehmer nur anzeigen (K.26 geht in TE.2 und TE.9 auf); Anhänge, Wegzeit, Farbe/Kategorie,
Sichtbarkeit später.

**Für alle Aufgaben gilt:**
- Titel, Ort, Notizen, URL, Teilnehmer und Suchtexte sind `untrusted`; ins Protokoll kommen nur Kennung und
  Befehl, nie Suchtext oder Koordinaten.
- Neue Felder in `Event` mit `#[serde(default)]`; ein Client, der sie nicht kennt, darf beim Speichern nichts
  verlieren (Round-Trip über `merge_ics_event`, nur geänderte Felder ersetzen).
- Tests ohne Netz und ohne Uhr: Fixtures unter `crates/torro-pim-caldav/tests/fixtures/`, „jetzt“ übergeben.
- Übersetzungen trägt jede Aufgabe selbst in `crates/torro-core/src/i18n/de.rs` ein (Abschnitt
  „// Event editor (#278)“); die Aufgaben laufen nacheinander.

```
TE.1 ─┬─ TE.2 ─┬─ TE.3 ── TE.4 ─┬─ TE.7 ── TE.8
      │        └─ TE.9          ├─ TE.6
      └─ TE.5 ──────────────────┴─ TE.8, TE.10
```

## TE.1 Gerüst: Modell, Requests, Katalog, Einstellung
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-pim/src/{model,source,demo}.rs`,
  `crates/torro-ipc/src/{lib,api,pim}.rs`, `crates/torro-core/src/settings.rs`, `crates/torro-gui/src/source.rs`,
  `crates/torroworkplace/src/{pim,core,settings_service,mcp}.rs`, alle Stellen mit `Event { … }`-Literalen (der
  Compiler nennt sie), `docs/api.md`, `docs/api-reference.md`
- **Modell** (`torro_pim::model`, alle mit `#[serde(default)]`, Doc-Kommentar je Feld): in `Event`
  `recurrence_id: Option<EventTime>`, `repeat: Option<Repeat>`, `time_zone: Option<String>` (IANA),
  `url: Option<String>`, `status: Option<EventStatus>` (`Confirmed`, `Tentative`, `Cancelled`, lowercase),
  `geo: Option<Geo>` (`lat`, `lon` als `f64`), `organizer: Option<Attendee>`, `attendees: Vec<Attendee>`
  (`name`, `email`, `partstat`: `NeedsAction|Accepted|Declined|Tentative|Delegated`, `role`). `Repeat` mit
  `freq` (`Daily|Weekly|Monthly|Yearly`), `interval: u16`, `weekdays: Vec<RepeatDay>` (Wochentag + optional
  n-ter, auch negativ), `month_days: Vec<i8>`, `months: Vec<u8>`, `end: RepeatEnd` (`Never|Count(u32)|Until(EventTime)`),
  `rule: String` (der gelesene `RRULE`-Text) und `custom: bool` (Regel hat Teile, die der Editor nicht abbildet).
  Neues `SeriesScope` (`Occurrence|Following|Series`, snake_case). `Event` bekommt **kein** `Default`; jede
  Literal-Stelle setzt die neuen Felder.
- **API:** `Request::SaveEvent { event, #[serde(default)] scope: Option<SeriesScope> }` und
  `Request::DeleteEvent { event, scope }`. Doc-Kommentare: Ohne `scope` bleibt ein Vorkommen `unsupported`
  wie heute. Neu `Request::SearchPlaces { query: String, #[serde(default)] near: Option<Geo> }` →
  `Response::Places { places: Vec<Place> }` (`label`, `detail`, `geo`, `kind`) und
  `Request::MapPreview { geo: Geo, width: u16, height: u16, #[serde(default)] dark: bool }` →
  `Response::Image { png: String }` (Base64). Grenzen im Doc-Kommentar und im Dienst: `query` höchstens 200
  Zeichen, `width` ≤ 800, `height` ≤ 400, `lat` in ±85,05, `lon` in ±180, sonst `error`. `Response::Event`
  bekommt `#[serde(default)] reset: u32` (Zahl der Ausnahmen, die beim Ändern wegfielen, TE.4). Mit `scope`, aber
  `href: None` → `error`; Umzug in einen anderen Kalender zusammen mit `scope` `occurrence`/`following` →
  `unsupported`. Fehlercodes: `unsupported` (Karten aus), `offline`, `busy` (Drosselung), `error`.
- **Katalog** (`api.rs`, Bereich `Calendar`, `Mcp::Tool`): `calendar.search_places` (Read; Text: schickt `query`
  und die IP des Rechners an den eingestellten Suchdienst, Photon als Standard; `unsupported`, solange Karten aus
  sind; höchstens eine Suche je Sekunde), `calendar.map_preview` (Read; schickt die Koordinaten an den
  Kacheldienst). `SAVE_EVENT` und `DELETE_EVENT`: Satz „Recurring events cannot be … yet“ ersetzen durch die drei
  Umfänge; `following` teilt die Serie und kann spätere Ausnahmen zurücksetzen; was Termine entfernt, braucht
  Löschrecht (TE.4).
- **Einstellung:** `Settings.maps: Maps` (`enabled: bool` Standard `false`, `geocoder: String` Standard
  `https://photon.komoot.io`, `geocoder_kind` `Photon|Nominatim`, `tiles: String` Standard
  `https://tile.openstreetmap.org/{z}/{x}/{y}.png`, `open_with` `Osm|Google|Apple|Geo`), nachsichtig gelesen wie
  `reminders`. `SettingKey::Maps` (ein Objekt, ändert nur genannte Felder). Nur für Menschen
  (`Request::kept_for_people`): `SetSetting { Maps }`, wenn `enabled` vorkommt und nicht `false` ist oder
  `geocoder`/`tiles` vorkommen; dazu `Core::switches_on_for_people` für `settings.update_settings`, das
  `maps.enabled` einschaltet oder `geocoder`/`tiles` gegenüber dem gespeicherten Stand ändert (wie `pdf_folder`);
  `update_settings` ohne `maps` behält den Stand. Ausschalten darf jeder mit Admin (mit Freigabe), Satz der Freigabe
  „Turn off maps and place search“ aus dem Wert. URLs mit Query-Parametern oder Zugangsdaten (`user:pass@`)
  lehnt der Dienst mit `error` ab: **Anbieter mit eigenem Schlüssel sind ein späteres Issue** (Schlüssel gehören in
  den Schlüsselbund, nie in Einstellungen). `docs/api.md` „Was nie nach außen geht“, Punkt 6: „Ebenso Karten und
  Ortssuche einschalten“ neben `mail_to_cloud`.
- **Trait:** `PimSource::save_event(event, scope)` und `delete_event(event, scope)` mit
  `Option<SeriesScope>`; `PimClient`, `torro_gui::source`, `DemoSource` ziehen nach (Demo: Umfang vorerst
  ignorieren). Handler im Kern: `SearchPlaces`/`MapPreview` antworten `unsupported` bis TE.5; `scope` wird bis
  TE.4 durchgereicht, `series()` bleibt.
- **MCP:** `calendar_search_places` und `calendar_map_preview` je auf genau ihren Request (das Bild vorerst als
  Base64-Text; Bildinhalt in TE.6), damit der Vertragstest Tool ↔ Katalog grün bleibt.
- **Abnahme:** `TORRO_BLESS=1 cargo test -p torro-ipc`, `docs/api-reference.md` committet; Rechte-Tabelle und
  MCP-Vertragstest grün; Tests wie bei `mail_to_cloud` (in `torro-ipc` und im Kern): `maps.enabled: true` bzw. neue
  `tiles` von einem Client → `forbidden`, über `update_settings` ebenso, ausschalten mit Freigabe geht; URL mit
  `?key=` → `error`; Grenzen von `map_preview`/`search_places`; Serde: altes JSON ohne die Felder liest sich; ein
  Vorkommen ohne `scope` bleibt `unsupported`.

## TE.2 iCalendar: neue Felder, Zeitzonen, Teilnehmer
- [x] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** TE.1 · **Dateien:** `crates/torro-pim-caldav/src/{ical,write}.rs`,
  `crates/torro-pim-caldav/Cargo.toml`, `Cargo.toml` (Wurzel), `crates/torro-pim-caldav/tests/fixtures/`,
  `crates/torro-pim/src/model.rs` (`Event::busy`), `crates/torroworkplace/src/reminders.rs`
- **Abhängigkeiten:** `chrono-tz` und `iana-time-zone` (Name der Zone des Rechners; kommt schon über `chrono`
  mit) über `[workspace.dependencies]` (aktuelle Version, Lizenz prüfen).
- **Lesen** (`to_event`, `parse_time`): `URL`, `STATUS` (abgesagte Termine **nicht mehr überspringen**, sondern
  `status: Cancelled`), `GEO` (`lat;lon`), sonst `X-APPLE-STRUCTURED-LOCATION` (`geo:`-Wert), `RECURRENCE-ID`
  nach `recurrence_id`, `ORGANIZER`/`ATTENDEE` (`CN`, `mailto:`, `PARTSTAT`, `ROLE`). `TZID` mit IANA-Namen über
  `chrono-tz` umrechnen und in `time_zone` merken; unbekannte Namen (Windows) weiter als Ortszeit wie heute.
- **Schreiben** (`event_fields`, `time_line`): `URL`, `STATUS`, `GEO`; hat sich `location` oder `geo` geändert,
  `X-APPLE-STRUCTURED-LOCATION` entfernen. Zeiten mit `time_zone` (sonst der Zone des Rechners aus
  `iana-time-zone`) als `DTSTART;TZID=…` und dazu ein `VTIMEZONE` aus `chrono-tz`
  (STANDARD/DAYLIGHT mit Jahresregel), einmal je Zone im Objekt; UTC nur, wenn keine Zone ermittelbar ist.
  Teilnehmer werden nie geschrieben (bleiben im Original).
- **Abgesagte:** `Event::busy` ist für `status: Cancelled` falsch; `crates/torroworkplace/src/reminders.rs` plant für
  sie nichts. Andere Leser von `events` (Eingang, Assistent, Glance) nachsehen und im Bericht nennen.
- **Abnahme:** Fixtures von Nextcloud (Termin mit TZID Europe/Berlin, abgesagt, mit Apple-Ort, mit
  Teilnehmern) lesen und unverändert zurückschreiben; neue Zeit in Berlin schreibt `TZID` + `VTIMEZONE` und
  liest sich wieder gleich; Sommer-/Winterzeit-Test; fmt, clippy, test (auch `TORRO_LANG=de`).

## TE.3 Serien lesen: Rohobjekte und Regel
- [x] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** TE.2 · **Dateien:** `crates/torro-pim-caldav/src/{client,ical}.rs`,
  `crates/torro-pim-caldav/src/recur.rs` (neu), `crates/torroworkplace/src/pim.rs`, Fixtures
- `recur.rs`: `RRULE` ↔ `Repeat` (lesen, schreiben, `custom` bei `BYSETPOS`, `BYHOUR`, `BYMINUTE`, `BYWEEKNO`,
  `BYYEARDAY`, `RDATE`, `EXRULE`), Satz für die Oberfläche kommt nicht hierher (TE.7).
- `CalDavClient::objects(collection, hrefs)`: `calendar-multiget` **ohne** `expand`, liefert je `href` ETag und
  ICS. Der Kern hält die Rohobjekte je `href` im `Slot` und holt nach jedem Sync nur `href`s, deren ETag neu ist
  oder fehlt (eine Anfrage je Sammlung), und vergisst verschwundene.
- Nach dem Sync setzt der Kern auf jedem Vorkommen `repeat` (aus dem Master), `time_zone` (aus `DTSTART` des
  Masters bzw. der Ausnahme) und `recurrence_id` (prüfen, ob Nextclouds `expand` sie mitliefert; sonst aus dem
  Rohobjekt ableiten – Fixture belegt den Fall). `recurring` bleibt wie heute.
- Demo (`crates/torro-pim/src/demo.rs`): „Daily Standup“ bekommt **eine** `href` für alle Vorkommen,
  `recurrence_id` und `repeat` (werktags), wie ein echter Server.
- **Abnahme:** Tests für RRULE-Runden (WEEKLY;BYDAY=TU, MONTHLY;BYDAY=4TU, MONTHLY;BYMONTHDAY=-1,
  YEARLY, COUNT, UNTIL mit Datum/Zeit, BYSETPOS → `custom`); Multiget-Antwort als Fixture; nur geänderte ETags
  werden geholt; fmt, clippy, test.

## TE.4 Serien schreiben: dieser, folgende, alle
- [x] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** TE.3 · **Dateien:** `crates/torro-pim-caldav/src/series.rs`
  (neu), `crates/torro-pim-caldav/src/{write,lib}.rs`, `crates/torro-pim-caldav/Cargo.toml`, `Cargo.toml`,
  `crates/torroworkplace/src/{pim,approvals,core}.rs`, `crates/torro-pim/src/demo.rs`, Fixtures
- **Abhängigkeit:** `rrule` über `[workspace.dependencies]` (Vorkommen zählen für `COUNT`, Vorschau „9 events“).
- `series.rs`, reine Funktionen auf dem gespeicherten ICS (Entscheidungen 4–6 im Konzept):
  - **Anlegen/Regel ändern:** `repeat` → `RRULE` am Master; `repeat: None` entfernt `RRULE` (Serie endet zu
    einem einzelnen Termin – nur mit `scope: Series`).
  - **Occurrence ändern:** Ausnahme-`VEVENT` mit derselben UID und `RECURRENCE-ID` (Kopie des Masters ohne
    `RRULE`/`EXDATE`/`RDATE`, dann die geänderten Felder) anlegen oder ersetzen.
  - **Following ändern:** teilen – Master `UNTIL` = Vorkommen − 1 s (`COUNT` → `UNTIL`), spätere `EXDATE`,
    `RDATE` und Ausnahmen dort entfernen; neues Objekt mit neuer UID, `RELATED-TO;RELTYPE=SIBLING`, `DTSTART`
    am Vorkommen, Rest-`COUNT`; spätere Ausnahmen und `EXDATE` ziehen um, wenn Zeit und Regel gleich bleiben,
    sonst fallen sie weg und die Antwort sagt wie viele. Ist das Vorkommen das erste, gilt `Series`.
  - **Series ändern:** Master ändern; verschiebt sich der Beginn, rücken `RECURRENCE-ID` aller Ausnahmen und alle
    `EXDATE` um denselben Abstand; Felder der Ausnahmen, die sie selbst geändert haben, bleiben.
  - **Löschen:** Occurrence → `EXDATE` (in der Zone von `DTSTART`) und Ausnahme entfernen; Following → wie
    oben ohne neues Objekt; Series → Objekt löschen (`CalDavClient::delete_event`).
- **Was Termine entfernt, braucht Löschrecht:** reine Funktion `series::drops(raw, event, scope) -> u32` aus dem
  Rohobjekt im Cache (nie aus Angaben des Clients): `repeat: None`, kürzeres `UNTIL`/`COUNT`, andere Regel,
  `following` mit neuer Zeit oder Regel (wegfallende Ausnahmen). Ist sie > 0 oder entfällt die Regel, prüft der Kern
  bei der Rechteprüfung (`core.rs`, dort wo `request.operation()` geprüft wird) `SaveEvent` wie
  `calendar.delete_event` (`Access::Delete`, Freigabe nach `needs_approval_of`).
- Neue `href` beim Teilen erzeugt der Kern in der Sammlung des Masters, nie aus dem Request; `out_of_reach` prüft
  dieselbe Sammlung.
- Kern (`PimService::save_event`, `delete_event`): Mit `scope` kein `series()` mehr; `check_existing` mit ETag
  wie heute; das Teilen schreibt erst das neue Objekt (`If-None-Match: *`), dann den Master (`If-Match`); scheitert
  der zweite Schritt, wird das neue gelöscht. Danach die Vorkommen der betroffenen `href`s neu holen (Abfrage
  mit `expand`, nur diese Objekte) und im Cache ersetzen; Antwort ist das Vorkommen mit derselben
  `recurrence_id` (bzw. das erste der neuen Serie), `reset` = Zahl weggefallener Ausnahmen.
- Freigaben (`approvals.rs`, `event_saved`, Zweig `DeleteEvent`; Nachschlagen in `PimService::facts`/`touched`
  über `href` **und** `recurrence_id`, nicht mehr das erste Vorkommen; ohne passendes Vorkommen „unknown event“): Umfang im Satz („this event“,
  „this and all following“, „all events of the series“), Titel und Datum aus dem Cache, neues Feld in `Facts` für die
  Zahl zurückgesetzter Ausnahmen („resets 2 changed events“) aus dem Rohobjekt.
- Demo: `DemoSource` kann alle drei Umfänge im Speicher (für `--demo`).
- **Abnahme:** Tests je Umfang × Ändern/Löschen auf Fixtures (Nextcloud-Serie mit `EXDATE` und zwei Ausnahmen,
  Serie mit `COUNT`, ganztägige Serie, Serie mit TZID über die Zeitumstellung): keine Ausnahme geht verloren,
  wo das Konzept es zusagt; fremde Eigenschaften bleiben; Handler-Test im Kern für ETag-Konflikt und
  Rücknahme beim Teilen; Freigabe-Texte, auch wenn der Client einen falschen `start` schickt (Datum aus dem Cache);
  Rechte-Fall: Client mit Write ohne Delete kann eine Serie nicht über `save_event` kürzen oder die Regel
  entfernen; Freigabe-Fall unter `Confirm::OnDelete`; fmt, clippy, test.

## TE.5 Orte im Dienst: Suche und Karte
- [x] erledigt
- **Modell:** Opus · **Braucht:** TE.1 · **Dateien:** `crates/torroworkplace/src/places.rs` (neu),
  `crates/torroworkplace/src/{core,main}.rs`, `crates/torroworkplace/Cargo.toml`, `crates/torroworkplace/tests/fixtures/`
- Suche: Photon (`/api?q=…&limit=6&lang=en|de`, mit `near` als `lat`/`lon`) oder Nominatim (`/search?format=jsonv2`)
  nach `Settings.maps`; Ergebnis → `Place`. Höchstens eine Anfrage je Sekunde für den ganzen Dienst, sonst `busy`;
  Anfragen mit App-Schlüssel haben Vorrang, Clients teilen sich ein kleineres Budget (z. B. 10 je Minute), damit ein
  Agent die Suche des Menschen nicht lahmlegt. Cache der letzten Antworten (Speicher, 24 h). Zuerst passende frühere
  Orte aus dem Termin-Cache (ohne Netz, `kind: recent`) – **nur aus Kalendern, die der Client lesen darf
  (`Reach`), gefiltert vor dem Lesen**.
- Karte: Zoom 16, Kacheln mit 2-facher Dichte zusammensetzen (`image`), auf `width`×`height` zuschneiden, Mitte
  auf `geo`; `dark` dunkelt ab (Helligkeit umkehren, Farbton halten). Kachel-Cache im State-Verzeichnis des
  Dienstes, `ETag`/`If-None-Match`, mindestens 7 Tage, kein Vorabladen.
- HTTP: eigener Client, User-Agent `TorroWorkplace/<version> (+https://github.com/mahype/TorroWorkplace)`,
  Zeitlimits wie `torro_pim_caldav::http_client`; 403/429 → `busy` mit Pause 1 h, ohne Netz `offline`.
  `maps.enabled == false` → `unsupported` ohne jede Anfrage. Fehler nur mit festen Texten und
  `reqwest::Error::without_url()`: Suchtext und Kachel-URL (= Koordinaten) nie in `message`, nie ins Protokoll.
  Kachel-Cache: Dateien 0600, Verzeichnis 0700 (zeigt, wo der Mensch war).
- **Abnahme:** Parser-Tests mit Fixtures (Photon, Nominatim), Kachel-Rechnung (lat/lon → x/y/Pixel), Abdunkeln,
  Drosselung ohne Uhr (auch Vorrang des App-Schlüssels), „aus“ fragt nie; Fehlertest: Suchtext und `lat` stehen
  nicht in `message`; Rechte-Test: frühere Orte aus fremden Kalendern fehlen; Dateirechte des Caches; Handler-Test
  über `Request`; fmt, clippy, test.

## TE.6 MCP-Werkzeuge
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** TE.4, TE.5 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- `calendar_save_event`: `scope` ist neben `event` erlaubt (Ausnahme von „nur `event`“), in der kurzen Form
  verboten (die legt nur an); kurze Form zusätzlich `repeat` (`freq`, `interval`, `weekdays`, `until`,
  `count`), `url`, `status`, `time_zone`, `free`. `calendar_delete_event`: `scope`. Neu `calendar_search_places`
  (`query`, `near`) bekommen bequeme Eingaben; `calendar_map_preview` gibt das PNG als MCP-Bildinhalt
  (`type: image`, `mimeType: image/png`) zurück – die Inhaltsart in `mcp.rs` ergänzen.
- **Abnahme:** Vertragstest Tool ↔ Katalog; Tests, dass die kurzen Formen den richtigen Request bauen;
  `{event, scope}` baut `SaveEvent { scope: Some(…) }`, kurze Form mit `scope` ist ein Eingabefehler; Bild-Antwort.

## TE.7 Editor: Felder, Wiederholung, Umfang (Layout A)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** TE.4 · **Dateien:** `crates/torrocalendar/src/{editor,app}.rs`,
  `crates/torrocalendar/src/editor/` (neu, falls `editor.rs` zu groß wird: `repeat.rs`), `crates/torro-gui/src/icon.rs`,
  `crates/torro-gui/assets/icons/`, `crates/torro-core/src/i18n/de.rs`
- Nach `termin-editor-a.png`: Zeilen When · Repeat · Time zone · Calendar · Show as · Status | Location · Link |
  Reminders · People | Notes. Werte über `widgets::prop_line`, Menüs und Karten über `widgets::menu`/`popover`.
- **Repeat:** Menü mit Kurzliste aus dem Startdatum (Never, Every day, Every weekday, Every week on {Tag}, Every 2
  weeks, Every month on the {n-ten Tag}, Every year on {Datum}, Custom…); „Custom…“ als Karte
  (`termin-editor-a-repeat.png`): Every n + Einheit, Wochentage, Monat: Tag n oder n-ter Wochentag, Ende Never /
  On / After n, Satz darunter. `custom`-Regeln zeigen den Satz „Custom rule“ und lassen sich nur ersetzen.
- **Band „Changes apply to“** unter dem Kopf bei Vorkommen einer Serie (`segmented`: This event · Following ·
  All, Standard This event); jede Änderung speichert mit diesem Umfang. Würden spätere Ausnahmen wegfallen
  (TE.4), steht vorher ein Satz mit der Zahl im Band. Read-only nur noch für schreibgeschützte Kalender und
  Änderungen der Regel bei „This event“ (Repeat-Zeile dann aus mit Tooltip).
- **Time zone:** Menü mit Suche über `chrono-tz`-Namen, oben die Zone des Rechners; leise „your time zone“.
  **Show as:** Busy/Free (`transparency`). **Status:** Confirmed/Tentative/Cancelled. **Link:** Eingabe, nur
  `http`/`https` klickbar (`torro_gui::links`). **People:** Avatare wie K.26, nur lesen.
- Hat der Termin Teilnehmer, steht im Band bzw. über „People“ leise „People in this event get an update from
  your server“ (Nextcloud verschickt bei Änderungen des Organisators selbst Mails, Sabre-Scheduling).
- **Delete event** im Fuß: bei Serien Rückfrage mit drei Knöpfen (`termin-editor-a-delete.png`), Escape bricht ab.
- `--open editor-series` (neu) öffnet ein Vorkommen der Demo-Serie; `editor-read-only` bleibt für
  schreibgeschützte Kalender.
- **Abnahme:** Tests: Kurzliste aus Datum (4. Dienstag, letzter Tag), Satz der Regel in beiden Sprachen, Umfang
  geht mit dem Request, Lesen-nur-Fälle; Sichtprüfung `torro-design-review` in allen Größen; fmt, clippy, test.

## TE.8 Editor: Erinnerungen frei, Ort mit Karte
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** TE.5, TE.7 · **Dateien:** `crates/torrocalendar/src/{editor,app}.rs`,
  `crates/torrocalendar/Cargo.toml`, `Cargo.toml` (iced-Feature), `crates/torro-core/src/i18n/de.rs`
- **Reminders** (`termin-editor-a-remind.png`): „Add reminder“ öffnet eine Karte mit Chips (At start, 5, 10, 15,
  30 min, 1 hour, 1 day) und „Custom“: n Minuten/Stunden/Tage/Wochen vor oder nach Beginn oder Ende · Tag
  davor um hh:mm (relativ zum Beginn gerechnet, als `Before` gespeichert) · festes Datum mit Uhrzeit (`At`);
  Art als Segment Calendar default / Notification / Full screen. Höchstgrenze `Reminders::MAX_LEAD` beachten.
- **Location:** Eingabe; Enter oder „Search“ schickt `calendar.search_places` (nie beim Tippen); Vorschläge als
  Menü unter dem Feld mit Quellenzeile „Search by {dienst} · © OpenStreetMap contributors“, letzter Eintrag
  „Use “…” as written“. Wahl setzt `location` und `geo`. Mit `geo` und eingeschalteten Karten:
  `calendar.map_preview` (336 × 104, `dark` nach Erscheinungsbild), Bild mit Radius `md`, „© OpenStreetMap“
  unten rechts (Link), ↗ „Open in Maps“ nach `maps.open_with`. Karten aus: keine Suche, Hinweis-Zeile „Turn on
  maps in Settings“ einmal leise; „Open in Maps“ geht immer (Adresse als Suchtext).
- iced `image`-Feature für `torrocalendar` (PNG aus `Response::Image`).
- **Abnahme:** Tests der Zeit-Rechnung (Tag davor um 18:00 bei 10:00-Termin = 960 min), der Links für
  „Open in Maps“, keine Suche ohne Enter; Sichtprüfung; fmt, clippy, test.

## TE.9 Ansichten: abgesagt, vorläufig, Teilnehmer
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** TE.2 · **Dateien:** `crates/torrocalendar/src/views/{week,month,agenda,timeline,radar,glance}.rs`,
  `crates/torrocalendar/src/views/mod.rs`
- Rest von K.26: abgesagte Termine durchgestrichen und 55 % (eine gemeinsame Hilfsfunktion in `views/mod.rs`),
  vorläufige gestrichelt ohne Fläche (Regel „Termine“ in `torro-design`); Avatare (höchstens 3 + „+N“) in der
  Agenda-Zeile. Abgesagte zählen nicht als belegt.
- **Abnahme:** Tests der Hilfsfunktion; Sichtprüfung Woche, Monat, Agenda, Zeitstrahl, Radar; fmt, clippy, test.
  K.26 in `kalender-aufgaben.md` abhaken mit Verweis.

## TE.10 Steuerzentrale: Karten einstellen, Abnahme mit Nextcloud
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** TE.5 (Abnahme: alle) · **Dateien:** `crates/torroworkplace/src/ui/general.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/termin-editor.md`, `docs/tasks/kalender-aufgaben.md` (Zeile „vorerst nur
  lesbar“)
- Gruppe „Maps“ unter General › Calendar: Schalter „Maps and place search“ mit Satz, welche Dienste was bekommen
  (Links zu den Nutzungsregeln), Felder für Such- und Kachel-URL, „Open in Maps with“; jede Zeile setzt nur ihr Feld
  über `settings.set_setting maps`.
- **Abnahme von Hand** (im Bericht festhalten): Nextcloud + Handy (DAVx5 oder iOS): Serie anlegen, ein Vorkommen
  verschieben, „folgende“ ändern, „alle“ verschieben, einzeln und ab hier löschen – überall gleiche Termine,
  Ausnahmen da; Zeitumstellung (Serie über den 25.10./29.03.); Karte und Suche ein- und ausgeschaltet.
