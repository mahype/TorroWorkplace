# Aufgaben: Fotos (`torrophotos`) und Bilderrahmen (`torroframe`), Stufen PH1–PH4 und FR1

Konzept: [fotos.md](../fotos.md). Entwürfe: [prototypes/photos/](../prototypes/photos/index.html).
Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`. Wer einen Request anfasst,
lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`. Wer mit Nextcloud spricht, lädt den
Skill `nextcloud` (Agent `nextcloud-expert` geeignet). Nach jeder UI-Änderung prüft
`torro-design-review` gegen die Entwürfe.

Jede Stufe ist für sich nutzbar und über Oberfläche, API und MCP erreichbar. Request und
Katalogeintrag einer Stufe kommen in ihre erste Kern-Aufgabe (PH1.1, PH2.1, PH3.1, PH4.1, FR1.1).

**Für alle Aufgaben gilt** (Entscheidungen 4, 7 und 8 in [fotos.md](../fotos.md)):
- Albennamen, Ortsnamen, Personennamen, Tags, Datei- und Ordnernamen und EXIF-Texte sind immer
  `untrusted: true`.
- Rechte gelten je Konto (`Rights.photo_accounts`). Was ein Client nicht sehen darf, ist `not-found`
  und wird vor dem Lesen ausgefiltert, auch in Zählern, Tagen, Alben und Orten.
- Bilder bekommt ein Client nur mit `Rights.image_content`; sonst `forbidden`.
- Ins Protokoll kommen keine Namen, Daten, Orte oder Texte.
- Version 1 lädt nichts hoch und ändert keine Bilddatei.

Aufgaben, die den Katalog ändern (PH1.1, PH2.1, PH3.1, PH4.1, FR1.1), laufen nie parallel zu anderen
Katalog-Aufgaben.

```
PH1.1a ─┬─ PH1.6 ─┬─ PH1.7 ── PH1.8          ← sofort möglich (neue Crates, Demo-Daten)
        │         └─ PH1.9
        ├─ PH1.2 ─────┐
        └─ PH1.1 ─────┴─ PH1.3 ── PH1.4 ─┬─ PH1.5
                                         ├─ PH1.10 (App an den Kern, braucht auch PH1.7–PH1.9)
                                         ├─ PH1.11
                                         └─ PH1.12
PH2.1 (braucht PH1.4) ── PH2.2 (braucht PH1.10)
PH3.1 (braucht PH1.4) ── PH3.2 (braucht PH1.10)
PH4.1 (braucht PH1.4) ── PH4.2 (braucht PH1.10)
FR1.1 (braucht PH1.1) ── FR1.2 (braucht PH1.4) ─┬─ FR1.3 ── FR1.4 (braucht PH1.10)
```

## PH1 Fotos ansehen und ordnen

### PH1.1a Modell-Crate `torro-photos`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-photos/` (neu: `Cargo.toml`,
  `src/{lib,model,ids,provider,source,demo}.rs`), Wurzel-`Cargo.toml` (Mitglied und Eintrag in
  `[workspace.dependencies]`)
- Ohne Ein-/Ausgabe und ohne Abhängigkeit zu `torro-ipc`. Vorbild `torro-pim` und `torro-notes`. Alle
  Typen mit `Serialize`, `Deserialize` und `JsonSchema`, weil PH1.1 sie als Wire-Format benutzt.
- **Typen:**
  - `PhotoSummary { id, account, kind: photo|video|live, taken, width, height, duration?, place?, favorite, etag, untrusted }`
  - `Photo` = `PhotoSummary` plus `file_name`, `folder`, `size_bytes`, `camera?`, `lens?`, `exposure?`,
    `gps?`, `albums`, `people`, `tags`, `taken_from: exif|corrected|file`
  - `Album { id, account, name, location?, from?, to?, count, cover?, shared_by?, collaborators, public_link: bool, public_url? }`
    (`public_url` füllt der Kern nur für den App-Schlüssel)
  - `Place { id, name, country?, center?, count, cover? }` (`center` = Koordinaten für `set_details`),
    `Tag { id, name, automatic, count }`, `GeoPoint { lat, lon }`,
    `Day { date, count, place? }`
  - `PhotoQuery` mit den Filtern aus [fotos.md](../fotos.md#befehle-bereich-photos)
  - `Frame { id, source, every, order, fit, caption }` mit
    `source: album|person|place|tag|favorites|on_this_day|all|photo` (für FR1)
  - Kennungen `PhotoId`, `AlbumId` … mit Zerlegung `<konto>/<rest>` am ersten `/`
- **`provider.rs`:** Trait `PhotosProvider` für den Kern, synchron wie `torro_pim::Provider`:
  `search(since, page)`, `photo(id)`, `albums()`, `places()`, `tags()`, `preview(id, w, h)`,
  `create_album`, `update_album`, `add_to_album`, `remove_from_album`, `delete_album`, `set_favorite`,
  `set_tags`, `set_details`, `delete`, `folders`, `set_folders`. Fehler als eigenes Enum: `Auth`,
  `Offline`, `Conflict`, `NotFound`, `Unsupported { app }`, `Locked`, `Other`.
- **`source.rs`:** Trait `PhotosSource` für die Apps, async wie `PimSource`, mit den Befehlen von PH1.
- **`DemoSource`** für `--demo`, Änderungen wirken im Speicher:
  - 600 Fotos über acht Jahre, 20 Videos, fünf Orte, acht Alben (zwei geteilt, eins mit öffentlichem
    Link), zwölf Tags (davon sechs automatisch)
  - Bilder **ohne Netz und ohne Dateien im Repo**: einfache Verläufe und Formen, aus einer Zahl erzeugt,
    damit Tests und Screenshots wiederholbar sind
  - Sichtbare Texte englisch mit `tr()`, Übersetzungen im Bericht nennen
- **API/MCP:** keine. Requests und Katalog kommen in PH1.1.
- **Abnahme:**
  - Tests der Kennungen (Zerlegen, Konto mit `/` im Namen abgelehnt), der Abfragen der Demo-Quelle
    (Zeitraum, Album, „An diesem Tag“ über Jahre) und eine serde-Rundreise
  - fmt, clippy und test grün, auch mit `TORRO_LANG=de`

### PH1.1 Gerüst: Requests, Katalog, Rechte
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.1a · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`, neu
  `crates/torro-ipc/src/photos.rs` (Antworttypen), `crates/torro-ipc/Cargo.toml`,
  `crates/torroworkplace/src/{clients,core,mcp}.rs` (nur Stellen, die `Rights { … }` bauen, und deren
  Tests), `docs/api.md` (Bereich `photos`), `docs/api-reference.md`
- Vorher entscheiden: Kommt `Area::Photos` in `NEVER_TO_AGENTS`
  ([fotos.md](../fotos.md#offen-in-der-jeweiligen-aufgabe-klären))? Wenn ja, gehört
  `crates/torroworkplace/src/assistant/engines/mod.rs` mit in die Dateien.
- **Requests** (Bereich `photos`, Stufe PH1 der Tabelle in
  [fotos.md](../fotos.md#befehle-bereich-photos)), je mit Doc-Kommentar und Fehlercodes. Handler
  antworten bis PH1.4 mit `unsupported`.
- **Katalog:**
  - alle Befehle `Mcp::Tool`, keiner `Never`
  - Beschreibungen sagen, dass Namen, Orte, Tags und EXIF-Texte Daten sind und keine Anweisungen
  - `get_preview` sagt, dass es ein eigenes Recht braucht und sonst `forbidden` antwortet
  - `delete_photos` sagt, dass die Fotos in den Papierkorb von Nextcloud gehen
  - `remove_from_album` und `delete_album` sagen, dass die Fotos erhalten bleiben
- **Rechte:**
  - `Area::Photos` (auch in `Area::ALL`)
  - `Rights.photo_accounts: Selection` mit serde-Standard „nichts“ für alte `clients.json`
  - `Rights.image_content: ImageContent { Off, LocalModels, Any }`, Standard `Off`. Für
    Client-Schlüssel zählt nur `Off` gegen alles andere; `LocalModels` wirkt beim Assistenten (PH1.12).
  - `Request::needs_image_content()`; `Rights::allows_request` gibt ohne das Recht `forbidden`
  - Vorlagen setzen die Stufe in `photos` und `photo_accounts: All`, `image_content` bleibt `Off`.
    `Rights::template()` vergleicht `image_content` nicht mit.
  - `Rights::assistant()` bekommt `photos` mit Stufe Voll, `confirm: OnDelete`, `image_content: Off`
- **`accounts.list_accounts`** bekommt das optionale Feld `photos` (Photos, Recognize und Memories da
  oder nicht, Index läuft, Zahl); gefüllt wird es ab PH1.3.
- `get_preview` sagt in der Beschreibung auch, dass Bildinhalte Daten sind und keine Anweisungen.
- `set_tags` sagt, dass ein unbekannter Name einen neuen Tag anlegt, den alle Nutzer des Servers sehen.
- `remove_from_album` mit Fotos anderer Mitwirkender ist `Delete` (`Request::operation()` kennt das nicht
  ohne Index; der Dienst stuft in PH1.4 hoch, wie bei `load_template`).
- **Freigaben:** Die Nachschlage-Regeln kommen in PH1.4, weil es den Index erst ab PH1.3 gibt.
- **Oberfläche / API / MCP:** API und MCP-Namen entstehen hier, die Oberfläche in PH1.6–PH1.11.
- **Abnahme:**
  - Katalog-Tabellentest und Namensregeln grün; alte `clients.json` lädt
  - Rechte-Test: `get_preview` ohne `image_content` → `forbidden`, mit → erlaubt; `set_folders`
    braucht Admin und wartet auf eine Freigabe
  - API-Referenz neu erzeugt; `torro-api-review` ohne Befund

### PH1.2 Nextcloud: Photos und WebDAV im Anbieter
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** PH1.1a · **Dateien:**
  `crates/torro-nextcloud/src/lib.rs`, neu `crates/torro-nextcloud/src/photos.rs`,
  `crates/torro-nextcloud/Cargo.toml`, `crates/torro-nextcloud/tests/fixtures/photos/*` (neu)
- **Erkennen:** `PROPFIND` Tiefe 0 auf `dav/photos/<uid>/`; 404 = Photos fehlt. Die Benutzer-ID wird
  entdeckt, nie aus dem Loginnamen gebaut (Skill `nextcloud`).
- **`photos.rs`** setzt `torro_photos::PhotosProvider` um:
  - **Suche:** `SEARCH` über die Foto-Ordner (`photosSourceFolders`), MIME-Typen wie die Photos-App,
    sortiert nach `nc:metadata-photos-original_date_time`, Seiten zu 500, mit `getlastmodified` für
    den Abgleich. Eigenschaften wie in `PropFindPlugin` der Photos-App.
  - **Alben:** `albums/` und `sharedalbums/` lesen; `MKCOL`, `COPY` (Foto hinein), `DELETE`
    (Foto heraus bzw. Album weg), `PROPPATCH` (`nc:location`, `nc:last-photo`). Umbenennen über `MOVE`.
  - **Orte:** `places/` lesen.
  - **Favorit, Tags, Datum und Ort:** `PROPPATCH` von `oc:favorite`, System-Tags über
    `systemtags-relations`, `nc:metadata-photos-original_date_time` und `nc:metadata-photos-gps`.
  - **Vorschau:** `/index.php/core/preview?fileId=…&x=…&y=…&a=1`.
  - **Löschen:** `DELETE` auf die Datei (Papierkorb).
  - **Foto-Ordner:** lesen und `PUT /apps/photos/api/v1/config/photosSourceFolders`.
  - Statuscodes: 404 → `NotFound`, 412 → `Conflict`, 423 → `Locked`, 507 → `Other` mit Meldung.
- **Wie im übrigen `torro-nextcloud`:** Kennungen werden geprüft, bevor sie in einer Adresse stehen; das
  App-Passwort geht nur an den eigenen Server; Weiterleitungen zu anderen Servern werden nicht verfolgt;
  kein Passwort in `Debug` oder Fehlern.
- **Messen:** `SEARCH` gegen einen Server mit vielen Fotos (manuell, `#[ignore]`), Ergebnis im Bericht.
- **Klären:** wie „Reset“ von Datum und Ort geht ([fotos.md](../fotos.md#offen-in-der-jeweiligen-aufgabe-klären));
  Ergebnis in `fotos.md` festhalten.
- **API/MCP:** keine (Anbieter).
- **Abnahme:** Tests ohne Netz (`httpmock`, Fixtures aus der Photos-App abgeleitet, anonymisiert): Suche in
  zwei Seiten, Album anlegen mit Namenskonflikt, Foto hinein und heraus, Datum korrigieren, Photos fehlt
  → `Unsupported`, Weiterleitung auf fremden Host wird nicht verfolgt.

### PH1.3 Kern: Index, Abgleich, Vorschau-Cache
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.1, PH1.2 · **Dateien:** neu
  `crates/torroworkplace/src/photos/{mod,store,sync,previews}.rs`, `crates/torroworkplace/src/{core,account}.rs`
  (Anbindung, Konto entfernen, Remote Wipe), `crates/torroworkplace/Cargo.toml`
- **`store.rs`:** SQLite im Datenverzeichnis, Datei 0600, Vorbild `mail/store.rs`: Fotos, Alben und
  Zuordnung, Orte, Tags und Zuordnung, Tage (Zahl, häufigster Ort).
- **`sync.rs`:** je Konto mit Photos, wie in [fotos.md](../fotos.md#aufbau): `etag` der Foto-Ordner,
  `SEARCH` seit dem letzten Abgleich, voller Abgleich der Kennungen höchstens täglich, Alben, Orte und
  Tags jedes Mal. Erster Abgleich im Hintergrund mit Fortschritt (für „Indexing …“).
- **`previews.rs`:** Cache im Cache-Verzeichnis, Dateien 0600, drei Größen, Grenze aus den
  Einstellungen (Standard 2 GB), älteste zuerst raus. Nie Originale.
- **Erkennen:** Jeder Abgleich (auch `accounts.sync`) prüft neu, ob Photos, Recognize und Memories da sind.
- **Konto entfernen und Remote Wipe** löschen Index und Cache des Kontos.
- **API/MCP:** keine neuen Befehle; der Kern füllt das Feld `photos` in `accounts.list_accounts` (aus PH1.1).
- **Abnahme:** Tests mit einem Fake-Anbieter in Temp-Verzeichnissen: erster Abgleich, Änderung,
  Löschung, Konto entfernen; Cache räumt über der Grenze auf; Dateirechte 0600.

### PH1.4 Kern: Befehle, Rechte-Filter, Freigaben
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.3 · **Dateien:** neu `crates/torroworkplace/src/photos/service.rs`,
  `crates/torroworkplace/src/{core,approvals,audit}.rs`
- Handler für alle PH1-Befehle. Lesen aus dem Index, Schreiben zuerst an den Server, dann in den Index.
- **Filter:** `photo_accounts` vor dem Lesen, auch für Zähler, Tage, Alben und Orte.
- **Bildinhalte:** `get_preview` nur mit `image_content` (der App-Schlüssel hat es immer).
- **Freigabe-Fakten** (`approvals::Facts`) aus dem Index für **jeden** Schreib-, Lösch- und Admin-Befehl
  von PH1 (Freigabe bei jeder Änderung), z. B. „Delete 3 photos from Oct 4, 2025“, „Delete album Tuscany
  2024 (214 photos stay)“, „Change the date of 3 photos to Oct 4, 2025“, „Add 5 photos to Tuscany 2024“,
  „Create the new tag sunset, visible to all users“, „Show photos from the folders …“. Texte entschärft
  wie in [api.md](../api.md#freigaben-der-mensch-entscheidet).
- **`remove_from_album`** mit Fotos anderer Mitwirkender (geteiltes Album) gilt als `Delete`: Der Besitzer
  kann sie nicht selbst wieder hinzufügen.
- **Öffentliche Links:** `public_url` nur für den App-Schlüssel.
- **Protokoll:** Kennungen nur als Konto plus Prüfsumme; Datumsfilter und die Ordner aus `set_folders`
  nie.
- **Oberfläche / API / MCP:** die API-Befehle aus PH1.1 werden hier lebendig; Tools in PH1.5.
- **Abnahme:** Handler-Tests je Befehl; ein Client ohne das Konto sieht weder Fotos noch Zähler; ein
  Client-Schlüssel bekommt kein `public_url`; `remove_from_album` mit Fotos anderer Mitwirkender gibt
  einem Client mit „Lesen und Schreiben“ `forbidden` und mit „Voll“ und Freigabe beim Löschen `pending`;
  `delete_photos` des Assistenten wartet auf eine Freigabe mit den Fakten aus dem Index;
  `torro-api-review` ohne Befund.

### PH1.5 MCP-Tools
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.4 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Je Befehl ein Tool `photos_*`, genau ein Request. Bequeme Eingaben: Datum als `YYYY-MM-DD`,
  `on_this_day: true`, Album auch per Namen, wenn er eindeutig ist (sonst `conflict` mit den Treffern).
- `photos_get_preview` antwortet mit einem **MCP-Bildinhalt** (`image/jpeg`, Größe `tile` als Standard),
  nicht mit Base64 im Text.
- **Abnahme:** Vertragstest Tool ↔ Katalog grün; Tests, dass die bequemen Eingaben den richtigen Request
  bauen und dass `photos_get_preview` ohne Recht den Fehler `forbidden` meldet.

### PH1.6 App-Gerüst `torrophotos` mit Demo
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.1a · **Dateien:** `crates/torrophotos/` (neu), Wurzel-`Cargo.toml`,
  `crates/torro-gui/src/icon.rs` und `crates/torro-gui/assets/icons/` (neue Icons: `images`,
  `book-image`, `scan-face`, `history`, `map-pin`, `play`, `pause`, `frame`), `crates/torro-ipc/src/lib.rs`
  (nur Eintrag der App in der App-Liste)
- `torro_gui::shell` mit Seitenleiste und Kopfleiste wie im Entwurf, Fensterklasse `torro-photos`,
  alle Größenklassen (Leiste, Mini, Schmal, Mittel, Breit), Zoom.
- `--demo` mit `DemoSource`, `--open timeline|select|photo|edit|thisday|albums|album|people|places|empty|loading`
  und `--now` für wiederholbare Bilder (Skill `torro-design`, Abschnitt 9).
- Hier nur Gerüst und Navigation; die Ansichten folgen in PH1.7–PH1.9. „Open Calendar ↗“ unten in der
  Seitenleiste ist der übliche App-Wechsel der Suite (`apps.open_app`).
- **Oberfläche:** Seitenleiste, Kopfleiste, App-Umschalter. **API:** `apps.open_app` (vorhanden) für den
  App-Wechsel, sonst keine (Demo, Anbindung in PH1.10). **MCP:** `apps_open_app` (vorhanden).
- **Abnahme:** startet mit `--demo` in allen Größen ohne Absturz; `scripts/design-check.sh` grün; Texte
  über `tr()`, Übersetzungen im Bericht.

### PH1.7 Zeitleiste und Auswahl
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.6 · **Dateien:** `crates/torrophotos/src/{timeline,select}.rs`,
  `crates/torrophotos/src/{main,app}.rs`
- Tage als Gruppen mit Datum und Ort, Raster mit 2 px Fuge, nur sichtbare Kacheln laden (virtuell),
  Sprungleiste der Jahre (`photos.list_days`), Years | Months | Days.
- Auswahl: Kreis auf der Kachel, Tag über seinen Kopf, Umschalt-Klick für Bereiche, `Escape` hebt auf.
  Die Kopfleiste wird zur Auswahlleiste (Add to album, Favorite, Tags, Date & place, Delete).
- Tastatur: Pfeile im Raster, `Enter` öffnet, `F` Favorit, `Delete` löschen mit „Undo“-Hinweis.
- **Oberfläche:** Zeitleiste, Favorites, Videos, Auswahlleiste.
  **API:** `photos.list_photos`, `photos.list_days`, `photos.get_preview`, `photos.set_favorite`,
  `photos.delete_photos`. **MCP:** `photos_list_photos`, `photos_list_days`, `photos_set_favorite`,
  `photos_delete_photos` (PH1.5).
- **Abnahme:** Sichtprüfung in allen Größen gegen den Entwurf; flüssiges Blättern mit 10 000 Demo-Fotos
  ohne merkliche Last im Ruhezustand.

### PH1.8 Betrachter, Details, Datum und Ort
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.7 · **Dateien:** `crates/torrophotos/src/{viewer,details,edit}.rs`,
  `crates/torrophotos/src/app.rs`
- Bild auf dem Grund, Pfeile, Filmstreifen; Eigenschaften-Spalte (Breit) bzw. Chips (Mittel, Schmal).
  Ort als Text, ohne Kartenkacheln (siehe [fotos.md](../fotos.md#sicherheit)).
- Dialog „Date and place“ mit Ortssuche (Orte aus dem Index), für ein Foto oder eine Auswahl, „Reset“
  stellt den Wert aus der Datei wieder her.
- „Open in Nextcloud“ öffnet die Datei im Browser (auch für Videos).
- **Oberfläche:** Betrachter, Dialog. **API:** `photos.get_photo`, `photos.get_preview`,
  `photos.set_details`, `photos.set_tags`, `photos.add_to_album`. **MCP:** `photos_get_photo`,
  `photos_set_details`, `photos_set_tags`, `photos_add_to_album`.
- **Abnahme:** Sichtprüfung gegen `?open=photo` und `?open=edit`; Tastatur ←/→, `I` für Details,
  `Escape` zurück.

### PH1.9 Alben, Orte, Tags, An diesem Tag, Mini und Leiste
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.6 · **Dateien:** `crates/torrophotos/src/{albums,places,tags,thisday,tile}.rs`,
  `crates/torrophotos/src/app.rs`
- Alben (All | Mine | Shared), Album mit Kopf (Zeitraum, Ort, Mitwirkende, öffentlicher Link), neues
  Album, umbenennen, löschen mit Rückfrage, Fotos hinzufügen; Orte als Raster; Tags mit Hinweis auf
  automatische Tags; „An diesem Tag“ je Jahr ein Streifen.
- Mini und Leiste zeigen „An diesem Tag“ (Anzeige, keine Bedienung).
- „Slideshow“ fehlt hier noch; sie kommt mit FR1.4 als „Show in frame“.
- **Oberfläche:** wie im Entwurf. **API:** `photos.list_albums`, `create_album`, `update_album`,
  `add_to_album`, `remove_from_album`, `delete_album`, `list_places`, `list_tags`, `list_photos` mit
  `on_this_day`. **MCP:** die gleichnamigen `photos_*`-Tools.
- **Abnahme:** Sichtprüfung gegen `?open=albums|album|places|thisday` und in Mini und Leiste.

### PH1.10 App an den Kern
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.4, PH1.7, PH1.8, PH1.9 · **Dateien:** `crates/torrophotos/src/{source,main}.rs`,
  `crates/torro-ipc/src/client.rs` (nur wenn ein Aufruf fehlt)
- `PhotosSource` über IPC, Änderungsmeldungen des Kerns statt Polling, Zustände leer, lädt
  (mit „Indexing … of …“), offline, Photos fehlt, kein Konto (`not-connected` → „Connect Nextcloud“).
- „Choose photo folders“ im Leerzustand (`photos.set_folders`).
- **Oberfläche:** alle Ansichten mit echten Daten. **API:** alle PH1-Befehle. **MCP:** unverändert.
- **Abnahme:** gegen einen echten Server manuell geprüft (Bericht); Sichtprüfung der Fehlerzustände;
  `torro-api-review`: keine Funktion der App ohne Request.

### PH1.11 Steuerzentrale: Foto-Dienst und Rechte
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.4 · **Dateien:** `crates/torroworkplace/src/ui/{accounts,agents,assistant}.rs`
- Kontoseite: Dienst „Photos“ mit Zustand, Foto-Ordnern, Recognize und Memories da oder nicht.
- Rechte-Zeile `photos` mit Kontenauswahl und Schalter „See image content“, darunter der Satz aus dem
  Entwurf. Beim Assistenten zusätzlich „Only local models“ | „Any model“.
- **Oberfläche:** Kontoseite, Rechte-Zeile. **API:** `photos.get_settings`, `photos.set_folders`;
  Rechte über die vorhandenen `clients.*` (nur App-Schlüssel). **MCP:** `photos_get_settings`,
  `photos_set_folders`; Rechte: Never (Punkt 2 in „Was nie nach außen geht“).
- **Abnahme:** Sichtprüfung gegen `?open=rights`; eine Änderung der Rechte wirkt ab der nächsten Anfrage.

### PH1.12 Assistent: Fotos im Kontext
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.4 · **Dateien:** `crates/torroworkplace/src/assistant/{prompt,tools,data_rule,engines/mod}.rs`
  und deren Tests
- Der Skill nennt die Foto-Tools und Beispiele („Fotos vom Urlaub in Italien 2024“, „Leg ein Album an“,
  „Was war heute vor einem Jahr?“).
- `image_content: LocalModels` gibt Vorschaubilder nur an Antriebe auf diesem Rechner oder im eigenen
  Netz; dafür die vorhandene Datenregel (`data_rule.rs`, [ki.md](../ki.md) Grundsatz 4) erweitern, nicht
  nachbauen. Ein entfernter Antrieb liest dann „withheld by your data rule“.
- **Oberfläche:** Chat des Assistenten. **API/MCP:** keine neuen Befehle.
- **Abnahme:** Test: Bildinhalt geht mit `LocalModels` an einen lokalen Antrieb und nicht an einen
  entfernten.

## PH2 Personen (Recognize)

### PH2.1 Personen: Requests, Anbieter, Kern
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** PH1.4 · **Dateien:**
  `crates/torro-ipc/src/{lib,api,photos}.rs`, `crates/torro-nextcloud/src/recognize.rs` (neu),
  `crates/torro-nextcloud/tests/fixtures/recognize/*`, `crates/torroworkplace/src/photos/{service,store,sync}.rs`,
  `crates/torroworkplace/src/{approvals,mcp}.rs`, `docs/api.md`, `docs/api-reference.md`
- Zuerst klären, wie der Kern an den `X-Recognize-Api-Key` kommt
  ([fotos.md](../fotos.md#offen-in-der-jeweiligen-aufgabe-klären)); Ergebnis in `fotos.md` festhalten.
  Der Schlüssel bleibt nur im Speicher des Kerns, als Typ `Secret` ohne `Debug`, nie in Fehlertexten.
- `faces/` und `unassigned-faces/` lesen; benennen (`MKCOL`), zuordnen (`MOVE`), ausblenden.
- **Requests:** `photos.list_people { account?, named?, hidden? }` (Read), `name_person`, `assign_face`,
  `hide_person` (Write).
- **Freigabe-Fakten:** „Name the person … Anna“, „Merge … into Anna“, „Assign a face on … to Anna“ aus dem
  Index, nie aus dem Text des Clients.
- **Oberfläche:** in PH2.2. **API:** die vier Befehle. **MCP:** `photos_list_people`,
  `photos_name_person`, `photos_assign_face`, `photos_hide_person`.
- **Abnahme:** Tests mit Fixtures; ohne Recognize oder ohne Schlüssel `unsupported`; `torro-api-review`
  ohne Befund.

### PH2.2 Personen in der App
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH2.1, PH1.10 · **Dateien:** `crates/torrophotos/src/{people,details}.rs`
- Ansicht „People“ (benannt, „Not named yet“, „Hidden“), Person öffnen, benennen, zusammenführen; im
  Betrachter „Name 1 more face“. Ohne Recognize der Hinweis aus dem Entwurf; „Check again“ schickt
  `accounts.sync`.
- **Oberfläche:** People, Betrachter. **API/MCP:** wie PH2.1.
- **Abnahme:** Sichtprüfung gegen `?open=people` und `?open=people-missing`.

## PH3 Alben teilen

### PH3.1 Teilen: Requests, Anbieter, Kern
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** PH1.4 · **Dateien:**
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-nextcloud/src/photos.rs`,
  `crates/torroworkplace/src/photos/service.rs`, `crates/torroworkplace/src/{approvals,mcp}.rs`,
  `docs/api.md`, `docs/api-reference.md`
- Zuerst klären, was beim Entfernen eines Mitwirkenden mit seinen Fotos passiert
  ([fotos.md](../fotos.md#offen-in-der-jeweiligen-aufgabe-klären)); Ergebnis in `fotos.md` festhalten.
- `nc:collaborators` per `PROPPATCH`: Personen, Gruppen, öffentlicher Link (Typ 3), Link aus der Antwort.
  Personen und Gruppen über die Sharee-Suche von Nextcloud (OCS) nachschlagen.
- **Requests:**
  - `photos.find_recipients { account, query }` (Read; der Suchbegriff kommt nicht ins Protokoll, die
    gefundenen Namen sind `untrusted: true`)
  - `photos.share_album` (**Admin**, wartet für Agenten immer auf eine Freigabe)
  - `photos.unshare_album` (**Delete**: ein abgeschalteter Link kommt nicht wieder)
- **Freigabe:** Der Kern löst die Empfänger **vor** der Freigabe zu festen Benutzer- und Gruppenkennungen
  auf; die Freigabe nennt Album, Zahl der Fotos, diese Empfänger und „creates a public link“ und führt
  genau sie aus.
- Die Adresse des öffentlichen Links bekommt nur der App-Schlüssel.
- **Oberfläche:** in PH3.2. **API:** die drei Befehle. **MCP:** `photos_find_recipients`,
  `photos_share_album`, `photos_unshare_album`.
- **Abnahme:**
  - Admin-Schalter aus → `forbidden`; an → `pending`, auch mit „Full access“
  - ein Client-Schlüssel sieht nie `public_url`, auch nicht in der Antwort von `share_album`
  - `torro-api-review` ohne Befund

### PH3.2 Teilen in der App
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH3.1, PH1.10 · **Dateien:** `crates/torrophotos/src/{share,albums}.rs`
- Dialog „Share“ am Album: Personen und Gruppen suchen (`photos.find_recipients`), öffentlicher Link an
  und aus, Link kopieren.
- **Oberfläche:** Dialog. **API/MCP:** wie PH3.1.
- **Abnahme:** Sichtprüfung; Link kopieren ohne ihn ins Protokoll zu schreiben.

## PH4 Karte und schnelle Zeitleiste (Memories)

### PH4.1 Memories: Requests, Anbieter, Kern
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`) · **Braucht:** PH1.4 · **Dateien:**
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-nextcloud/src/memories.rs` (neu),
  `crates/torroworkplace/src/photos/{service,sync}.rs`, `crates/torroworkplace/src/mcp.rs`,
  `docs/api.md`, `docs/api-reference.md`, `docs/fotos.md` (Entscheidung zu den Kartenkacheln)
- Zuerst die Quelle der Kartenkacheln klären ([fotos.md](../fotos.md#offen-in-der-jeweiligen-aufgabe-klären)).
- Erkennen über `GET /apps/memories/api/describe`; `map/clusters` für die Karte; wenn PH1.2 gezeigt hat,
  dass `SEARCH` zu langsam ist, auch `api/days` für den Abgleich.
- **Request:** `photos.list_map { account?, bounds, zoom }` (Read).
- **Oberfläche:** in PH4.2. **API:** `photos.list_map`. **MCP:** `photos_list_map`.
- **Abnahme:** Tests mit Fixtures; ohne Memories `unsupported`.

### PH4.2 Karte in der App
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH4.1, PH1.10 · **Dateien:** `crates/torrophotos/src/{map,places,details}.rs`
- „Places“ mit Grid | Map; Haufen mit Titelbild und Zahl; Klick öffnet die Fotos des Haufens. Kleine
  Karte im Betrachter, wenn eine Kachelquelle eingerichtet ist.
- **Oberfläche:** Karte. **API/MCP:** wie PH4.1.
- **Abnahme:** Sichtprüfung gegen `?open=map`; ohne Memories bleibt nur „Grid“.

## FR1 Bilderrahmen

### FR1.1 Gerüst: Rahmen-Befehle und Ziel `frame:`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** PH1.1 · **Dateien:** `crates/torro-ipc/src/{lib,api,photos}.rs`,
  `crates/torroworkplace/src/{approvals,mcp}.rs`, `docs/api.md` (Bereich `photos` und Fußnote zu `open_app`),
  `docs/api-reference.md`
- **Requests:** `photos.list_frames` (Read), `photos.save_frame` (Write), `photos.delete_frame` (Delete).
  Handler antworten bis FR1.2 mit `unsupported`.
- `apps.open_app` bekommt das Ziel `frame:<id>` für `torroframe` (wie `mail:` und `task:`, Fußnote ¹³):
  Der Dienst baut die Argumente selbst (`--frame <id>`); der Client braucht Read in `photos` und die
  Quelle des Rahmens im Blick (Konto in `photo_accounts`), sonst `not-found`.
- **Freigabe-Regel** für `open_app` mit `frame:`: Die Zusammenfassung nennt die Quelle des Rahmens aus dem
  Index („Open a frame with Tuscany 2024“), das Protokoll nur die App.
- **Oberfläche:** in FR1.3. **API:** die drei Befehle und das neue Ziel. **MCP:** `photos_list_frames`,
  `photos_save_frame`, `photos_delete_frame`, `apps_open_app` (vorhanden).
- **Abnahme:** Katalogtest grün; `open_app` mit fremdem Ziel → `error`; `torro-api-review` ohne Befund.

### FR1.2 Kern: Rahmen und Vorladen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FR1.1, PH1.4 · **Dateien:** neu `crates/torroworkplace/src/photos/frames.rs`,
  `crates/torroworkplace/src/photos/{service,previews}.rs`, `crates/torroworkplace/src/approvals.rs`
- Rahmen in `frames.json` im State-Verzeichnis (0600). Je Rahmen lädt der Kern die nächsten zehn Bilder in
  Kachelgröße vor, damit der Rahmen offline weiterläuft.
- Fällt die Quelle weg (Album gelöscht, Konto entfernt), bleibt der Rahmen und zeigt „Choose what to show“.
  Wird ein Album umbenannt, ziehen die Rahmen mit der neuen Kennung mit.
- **Filter:** `list_frames` zeigt nur Rahmen, deren Quelle der Client sieht; `save_frame` und
  `delete_frame` mit einer anderen Kennung antworten `not-found`.
- **Freigabe-Fakten** für `save_frame` und `delete_frame`: Quelle, Takt, „replaces …“ beim Ändern.
- **Oberfläche:** keine. **API/MCP:** wie FR1.1, jetzt mit Leben.
- **Abnahme:** Handler-Tests; Vorladen bleibt in der Cache-Grenze; ein Client ohne das Konto der Quelle
  bekommt `not-found`.

### FR1.3 App `torroframe`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** FR1.2 · **Dateien:** `crates/torroframe/` (neu), Wurzel-`Cargo.toml`,
  `crates/torro-ipc/src/lib.rs` (nur Eintrag der App in der App-Liste)
- Fensterklasse `torro-frame`, `--frame <id>`, `--demo`, `--open show|hover|setup|contain|empty|offline`.
- Ruhezustand nur Bild und Bildunterschrift; beim Überfahren Quelle, „Open in Photos“, Einstellungen,
  Zurück/Pause/Weiter, Fortschritt. Einstellungen als schwebende Karte (`photos.save_frame`).
- Ohne `--frame`: „Set up frame“; beim ersten Speichern Neustart mit `--frame <id>` an derselben Stelle.
- Bildwechsel mit Überblenden; Dauer mit beiden Werten aus „Offen“ ansehen und in `torro-design` festhalten.
  Zwischen den Wechseln kein Neuzeichnen.
- Größen: Mini, Schmal, Breit, Leiste, Hochkant; „Whole photo“ mit unscharfem Rand.
- **Oberfläche:** Rahmen. **API:** `photos.list_frames`, `save_frame`, `list_photos`, `get_preview`.
  **MCP:** wie FR1.1.
- **Abnahme:** Sichtprüfung gegen `frame.html` in allen Zuständen und Größen; CPU im Ruhezustand nahe null;
  eine Workspace-Vorlage stellt den Rahmen mit derselben Quelle wieder her.

### FR1.4 „Show in frame“ in der Foto-App
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** FR1.3, PH1.10 · **Dateien:** `crates/torrophotos/src/{albums,people,places,app}.rs`
- Aktion „Show in frame“ an Album, Person, Ort und Favoriten: `photos.save_frame`, dann `apps.open_app`
  mit `target: "frame:<id>"`. Der Knopf „Slideshow“ (Album, An diesem Tag) tut dasselbe.
- **Oberfläche:** Menüeintrag. **API:** `photos.save_frame`, `apps.open_app`. **MCP:** `photos_save_frame`,
  `apps_open_app`.
- **Abnahme:** Aktion öffnet einen Rahmen mit der richtigen Quelle; Texte über `tr()`.
