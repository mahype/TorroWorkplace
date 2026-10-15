# Fotos: App `torrophotos` und Bilderrahmen `torroframe` auf Nextcloud

Stand: 2026-10-14. **Konzept, Entscheidungen vom 2026-10-14 eingearbeitet.** Aufgaben:
[tasks/fotos.md](tasks/fotos.md). Entwürfe: [prototypes/photos/](prototypes/photos/index.html)
(`photos.html`, `frame.html`, Zustände über `?open=…`).

## Ziel

Eine Foto-App der Suite auf **Nextcloud**: Zeitleiste, Alben, Personen, Orte und Favoriten ansehen und
ordnen. Dazu kommt eine kleine Nebenanwendung, der **Bilderrahmen**. Er zeigt in einer Kachel des
Workspaces ein Foto oder eine Diashow, etwa ein bestimmtes Album. Wie jede Funktion der Suite gibt es
alles dreimal: in der Oberfläche (`torrophotos`, `torroframe`), über die API (Bereich `photos`) und
über MCP (`photos_*`), also auch für den Assistenten.

## Was Nextcloud bietet

Fotos sind in Nextcloud **Dateien**. Es gibt keine Foto-Datenbank, die man als Ganzes abfragt. Die
Funktionen verteilen sich auf drei Schichten. Geprüft am Quellcode von `nextcloud/photos`
(Nextcloud 36), `nextcloud/recognize` und `pulsejet/memories` (10.1).

**1. Server (immer da), WebDAV `/remote.php/dav/files/<uid>/`**
- **Zeitleiste:** `SEARCH` mit `basicsearch` über die Foto-Ordner des Nutzers, gefiltert nach MIME-Typ.
  Sortiert und gefiltert wird nach `nc:metadata-photos-original_date_time` (Aufnahmezeit aus EXIF,
  Unix-Zeit). Seitenweise mit `firstResult`/`nbResults`. So macht es die Photos-App selbst
  (`src/services/PhotoSearch.ts`).
- **Metadaten als DAV-Eigenschaften:** `nc:metadata-photos-size` (Breite, Höhe),
  `-original_date_time`, `-exif`, `-ifd`, `-gps`, `-place`, dazu `oc:fileid`, `getcontenttype`,
  `getetag`, `oc:favorite`.
- **Favoriten:** `oc:favorite` per `PROPPATCH`.
- **Tags:** System-Tags über `/remote.php/dav/systemtags/` und `systemtags-relations/files/<fileid>`.
- **Vorschaubilder** in jeder Größe: `/index.php/core/preview?fileId=…&x=…&y=…&a=1` (Basic Auth).
- **Löschen** per `DELETE` landet im Papierkorb (`trashbin/<uid>/`).
- **Änderungen** meldet `notify_push` (`notify_file`), sonst Abgleich in Abständen.

**2. App „Photos“ (Standard-App), DAV `/remote.php/dav/photos/<uid>/`**
- `albums/`: Album anlegen mit `MKCOL`. Fotos kommen per `COPY` hinein, das ist nur ein Verweis und
  keine Kopie. Entfernen geht per `DELETE` im Album. Name, Ort, Titelbild und Filter ändert
  `PROPPATCH` (`nc:location`, `nc:last-photo`, `nc:filters`). `nc:dateRange` und `nc:nbItems`
  kommen vom Server.
- `sharedalbums/`: Alben, die andere mit mir teilen. Mitwirkende stehen in `nc:collaborators`
  (Personen, Gruppen, öffentlicher Link). Öffentliche Alben liegen unter `photospublic/<token>`.
- `places/`: Orte aus Rückwärts-Geocoding der GPS-Daten, nur lesend.
- **Datum und Ort korrigieren:** `PROPPATCH` auf `nc:metadata-photos-original_date_time` und
  `nc:metadata-photos-gps` (`MetadataPropPatchPlugin`). Die Werte stehen in den Metadaten von
  Nextcloud, die Bilddatei bleibt unverändert.
- **Foto-Ordner** des Nutzers: Einstellung `photosSourceFolders` (Standard `["/Photos"]`), lesbar über
  die Seite der App, schreibbar über `PUT /apps/photos/api/v1/config/photosSourceFolders`.

**3. Zusatz-Apps (optional)**
- **Recognize:** Gesichter und Objekterkennung, lokal auf dem Server mit offenen Modellen. Kategorien
  („Strand“, „Hund“) landen als System-Tags. Personen liegen unter
  `/remote.php/dav/recognize/<uid>/faces/<Person>`: benennen per `MKCOL`, Gesicht zuordnen per `MOVE`,
  dazu `unassigned-faces/`. **Haken:** Standardmäßig verlangt Recognize den Header
  `X-Recognize-Api-Key`. Diesen Schlüssel erzeugt der Server verschlüsselt mit Zeitstempel
  (`ApiKeyManager`) und gibt ihn nur auf der Seite der Photos-App (Initial State `recognizeApiKey`)
  oder über Memories (`/apps/memories/api/recognize/api-key`) heraus. Für einen nativen Client ist
  das der wackligste Teil (siehe PH2).
- **Memories** (Drittanbieter, verbreitet): eigene REST-API unter `/apps/memories/api/`, unter anderem
  `days` (Zeitleiste nach Tagen), `clusters/{backend}` (Personen, Orte, Tags, Alben), `map/clusters`,
  `image/multipreview` (viele Vorschaubilder auf einmal), `image/set-exif`, Archiv, Live-Fotos,
  Video-Umwandlung. Memories hängt eng an bestimmten Nextcloud-Versionen (10.1: Nextcloud 34–35).

**Erkennen, was da ist:** Keine der drei Apps meldet sich in den Capabilities. Der Kern prüft darum
direkt: `PROPFIND` Tiefe 0 auf `dav/photos/<uid>/` und `dav/recognize/<uid>/` (404 = fehlt) und
`GET /apps/memories/api/describe` für Memories. Das Ergebnis steht am Konto (siehe „Befehle“).

## Entscheidungen (2026-10-14)

1. **Basis: Server plus Photos über WebDAV.** Recognize (Personen) und Memories (Karte, schnelle
   Zeitleiste) nutzt der Kern zusätzlich, wenn sie da sind. Fehlt eine App, sagt die Oberfläche das
   freundlich („People needs Recognize“) und zeigt keinen Fehler.
2. **Version 1 zeigt und ordnet nur.** Hochladen kommt später als eigene Stufe (z. B. ein Ordner, der
   beobachtet wird). Bis dahin laden die Nextcloud-Apps auf dem Telefon oder Dateien hoch.
3. **Bearbeiten nur über Metadaten:** Datum und Ort korrigieren, Favoriten, Tags, Alben. Keine
   Pixel-Bearbeitung, kein Drehen, kein Zuschneiden.
4. **Bildinhalte nur mit eigenem Recht, standardmäßig aus.** Ohne das Recht sieht ein Agent Datum,
   Ort, Alben, Personen, Tags und Dateiangaben, aber kein Bild (`photos.get_preview` antwortet
   `forbidden`). Für Agenten über MCP ist das Recht an oder aus. Beim Assistenten gibt es drei Stufen:
   aus, nur lokale Modelle (die Vorschau geht nur an ein Modell auf diesem Rechner oder im eigenen
   Netz, nach der Datenregel des Assistenten, die heute schon Mail bei lokalen Modellen hält:
   [ki.md](ki.md) Grundsatz 4, `assistant/data_rule.rs`), jedes Modell.
5. **Bilderrahmen als eigene kleine App** `torroframe` (Fensterklasse `torro-frame`). Seine Wahl
   (Quelle, Takt, Reihenfolge, Einpassen, Bildunterschrift) gehört zum Fenster und kommt mit einer
   Workspace-Vorlage zurück (siehe „Bilderrahmen“).
6. **Name und Crates (Vorschlag, mit diesem Konzept bestätigt):**
   - App `torrophotos`, Fensterklasse `torro-photos`, Bereich `photos`, MCP-Tools `photos_*`.
   - `torro-photos` (neu): neutrales Modell, Anbieter-Trait, Demo-Quelle, ohne Ein-/Ausgabe. Vorbild
     `torro-pim` bzw. `torro-notes`.
   - Nextcloud-Teil in das bestehende `torro-nextcloud` (`photos.rs`, später `recognize.rs`,
     `memories.rs`).
   - Kern-Teil nach `crates/torroworkplace/src/photos/`.
   - `torroframe` (neu) als eigene kleine App.
7. **Fremder Text ist `untrusted: true`:** Albennamen, Ortsnamen, Personennamen, Tags, Dateinamen,
   Ordnernamen und EXIF-Texte (Titel, Beschreibung, Kamera). Geteilte Alben und EXIF stammen von
   anderen Menschen und Geräten. Katalogbeschreibungen und MCP-Tools sagen, dass das Daten sind und
   keine Anweisungen.
8. **Teilen braucht immer die Person.** Ein Album teilen oder einen öffentlichen Link anlegen bringt
   Fotos aus der eigenen Cloud zu anderen. Das ist `Admin`: Der Schalter dafür ist je Agent aus, und
   ist er an, wartet jeder Aufruf trotzdem auf eine Freigabe. Im Entwurf heißt das „Always ask“.
   Bewusst nicht „Nach außen“ (`Rights.outbound`) wie beim Mailversand: Dort erlaubt die Stufe „Frei“
   Senden ohne Rückfrage, Teilen soll aber immer die Person bestätigen. Der Admin-Schalter in `photos`
   deckt außerdem die Foto-Ordner (`set_folders`); die Rechte-Zeile sagt beides. `find_recipients`
   bleibt `Read`: Es ändert nichts, Nextcloud zeigt dabei nur, was der Server für diesen Nutzer ohnehin
   freigibt (Einstellungen der Sharee-Suche), und als Admin bräuchte schon das Suchen eine Freigabe.
9. **Stufen:**
   - PH1: Ansehen und ordnen mit Photos und WebDAV, samt Zeitleiste, Alben, Orten, Tags, Favoriten,
     „An diesem Tag“, Datum und Ort und Löschen
   - PH2: Personen (Recognize)
   - PH3: Alben teilen
   - PH4: Karte und schnelle Zeitleiste (Memories)
   - FR1: Bilderrahmen. Er braucht nur PH1 und kann parallel zu PH2–PH4 entstehen.

   Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar.

## Aufbau

```
torrophotos ─┐                torroworkplace (Kern)
torroframe ──┴─ IPC ──▶        ├─ photos/service.rs   Befehle, Rechte-Filter, Freigabe-Fakten
                               ├─ photos/store.rs     SQLite: Fotos, Alben, Orte, Tags, Tage
                               ├─ photos/sync.rs      Abgleich je Konto
                               ├─ photos/previews.rs  Vorschau-Cache (Kacheln, Betrachter, Rahmen)
                               └─ photos/frames.rs    Rahmen und ihre Wahl
                                        │
                     torro_photos::PhotosProvider (Trait, Modell)
                        ├─ torro-nextcloud/photos.rs     WebDAV, Photos-DAV, Vorschau (PH1)
                        ├─ torro-nextcloud/recognize.rs  Personen (PH2)
                        └─ torro-nextcloud/memories.rs   Karte, Tage (PH4)
```

**Kennungen** (Konto vorn, getrennt am ersten `/`, wie bei Sammlungen und Notizen):

| Was | Kennung |
|---|---|
| Foto oder Video | `<konto>/<fileid>` |
| eigenes Album | `<konto>/album/<name>` |
| geteiltes Album | `<konto>/shared/<name> (<besitzer>)`, wie es der Server benennt |
| Ort | `<konto>/place/<name>` |
| Person (PH2) | `<konto>/person/<cluster-id>` |
| Tag | `<konto>/tag/<tag-id>` |
| Rahmen | `frame-<uuid>`, gehört dem Rechner, nicht einem Konto |

**Index statt Live-Abfragen.** Der Kern hält einen Index in SQLite (Datei 0600) mit dem, was die Listen
brauchen: Kennung, Aufnahmezeit, Art (Foto, Video, Live-Foto), Größe, Ort, Favorit, Alben, Tags,
`etag`. EXIF im Ganzen holt er erst beim Öffnen eines Fotos. So zeigt die App Zeitleiste und Zähler
sofort und auch offline.

**Abgleich:**
- beim Start, alle 15 Minuten, nach `accounts.sync`, nach jedem Schreiben und bei `notify_file`
- Ob sich etwas geändert hat, sagt das `etag` der Foto-Ordner (es ändert sich mit jedem Kind)
- Geändert: `SEARCH` mit `getlastmodified` größer als beim letzten Abgleich, in Seiten zu 500
- Gelöschtes findet ein voller Abgleich der Kennungen, höchstens einmal am Tag oder auf Wunsch
- Alben, Orte und Tags sind klein und werden jedes Mal ganz gelesen
- Erster Abgleich großer Sammlungen (zehntausende Fotos) läuft im Hintergrund; die App zeigt „Indexing
  2,140 of 13,292…“ und schon die fertigen Tage

**Vorschau-Cache** im Cache-Verzeichnis (Dateien 0600):
- drei Größen: Kachel 256, Betrachter bis 2048, Rahmen in Kachelgröße
- begrenzt (Standard 2 GB, Einstellung), älteste zuerst raus
- verschwindet mit dem Konto oder bei Remote Wipe
- **Originale lädt der Kern nicht** (Version 1): Der Betrachter zeigt die große Vorschau, „Open in
  Nextcloud“ den Rest

**Fehler** (feste Codes):

| Fall | Code |
|---|---|
| Server nicht erreichbar | `offline`; Listen und gecachte Vorschaubilder gehen weiter |
| 401 | `auth`, danach Remote-Wipe-Prüfung wie bei CalDAV |
| `etag` passt nicht mehr (412) | `conflict` |
| Photos, Recognize bzw. Memories fehlt | `unsupported` mit dem Namen der App |
| Recognize ohne gültigen Schlüssel (403) | `unsupported` mit „People needs a newer Recognize setup.“ |
| Album gleichen Namens gibt es schon | `conflict` |
| Bildinhalte ohne Recht | `forbidden` |
| Kein Konto mit Fotos | `not-connected` |

## Befehle (Bereich `photos`)

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| PH1 | `photos.list_photos { account?, from?, to?, album?, place?, person?, tag?, favorites?, kind?, on_this_day?, order?, cursor?, limit? }`: Fotos ohne Bild, `order: newest\|oldest` (Standard neueste zuerst) | Read | `photos_list_photos` |
| PH1 | `photos.list_days { account?, from?, to?, album?, … }`: Tage mit Zahl und Ort, für Zeitleiste und Sprung | Read | `photos_list_days` |
| PH1 | `photos.get_photo { id }`: alle Angaben (EXIF, GPS, Ort, Alben, Personen, Tags, Ordner) | Read | `photos_get_photo` |
| PH1 | `photos.get_preview { id, size: tile\|view }`: Vorschaubild (JPEG) | Read **und** Recht „Bildinhalte“ | `photos_get_preview` (liefert ein MCP-Bild) |
| PH1 | `photos.list_albums { account?, shared? }`: eigene und geteilte, mit Zeitraum, Zahl, Titelbild, Mitwirkenden, `public_link: true\|false` (die Adresse nur für den App-Schlüssel) | Read | `photos_list_albums` |
| PH1 | `photos.list_places { account? }` | Read | `photos_list_places` |
| PH1 | `photos.list_tags { account? }` | Read | `photos_list_tags` |
| PH1 | `photos.create_album { account, name, location? }` | Write | `photos_create_album` |
| PH1 | `photos.update_album { id, name?, location?, cover? }`: antwortet mit der neuen Kennung, wenn der Name sich ändert | Write | `photos_update_album` |
| PH1 | `photos.add_to_album { album, photos }` | Write | `photos_add_to_album` |
| PH1 | `photos.remove_from_album { album, photos }`: das Foto bleibt erhalten | Write, bei Fotos anderer Mitwirkender **Delete** | `photos_remove_from_album` |
| PH1 | `photos.set_favorite { photos, favorite }` | Write | `photos_set_favorite` |
| PH1 | `photos.set_tags { photos, add?, remove? }`: ein unbekannter Name legt einen neuen Tag an, der auf dem ganzen Server sichtbar ist | Write | `photos_set_tags` |
| PH1 | `photos.set_details { photos, taken?, place? }`: Datum und Ort korrigieren (`place` mit Koordinaten, z. B. aus einem bekannten Ort), die Datei bleibt | Write | `photos_set_details` |
| PH1 | `photos.delete_album { id }`: Fotos bleiben erhalten | Delete | `photos_delete_album` |
| PH1 | `photos.delete_photos { photos }`: in den Papierkorb von Nextcloud | Delete | `photos_delete_photos` |
| PH1 | `photos.get_settings { account }`: Foto-Ordner | Read | `photos_get_settings` |
| PH1 | `photos.set_folders { account, folders }` | Admin | `photos_set_folders` |
| PH2 | `photos.list_people { account?, named?, hidden? }` | Read | `photos_list_people` |
| PH2 | `photos.name_person { id, name }`: auch Zusammenführen, wenn der Name schon vergeben ist | Write | `photos_name_person` |
| PH2 | `photos.assign_face { photo, face, person }`: ein Gesicht einer Person zuordnen | Write | `photos_assign_face` |
| PH2 | `photos.hide_person { id, hidden }` | Write | `photos_hide_person` |
| PH3 | `photos.share_album { id, people?, groups?, public_link? }` | Admin | `photos_share_album` |
| PH3 | `photos.find_recipients { account, query }`: Personen und Gruppen zum Teilen suchen, Namen `untrusted` | Read | `photos_find_recipients` |
| PH3 | `photos.unshare_album { id, people?, groups?, public_link? }`: ein abgeschalteter Link kommt nicht wieder, und Fotos eines entfernten Mitwirkenden verlassen womöglich das Album | **Delete** | `photos_unshare_album` |
| PH4 | `photos.list_map { account?, bounds, zoom }`: Haufen für die Karte (Memories) | Read | `photos_list_map` |
| FR1 | `photos.list_frames {}` | Read | `photos_list_frames` |
| FR1 | `photos.save_frame { id?, source, every, order, fit, caption }` | Write | `photos_save_frame` |
| FR1 | `photos.delete_frame { id }` | Delete | `photos_delete_frame` |
| FR1 | `apps.open_app { app: "torroframe", target: "frame:<id>" }` (vorhanden, neues Ziel) | Write (`apps`) + Read in `photos` | `apps_open_app` |

Erläuterungen:
- **Kein Befehl steht auf `Never`.** Nichts davon fällt unter
  [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht). Der Recognize-Schlüssel ist ein
  Geheimnis im Sinn von Punkt 1 und erscheint in keiner Antwort, keinem Fehler und keinem Protokoll.
- **Bildinhalte:** `get_preview` prüft neben Read in `photos` das neue Recht
  `Rights.image_content` (Entscheidung 4). Der App-Schlüssel hat es immer, Apps und Rahmen zeigen also
  Bilder. Ohne das Recht antwortet der Dienst `forbidden`, auch wenn der Client das Foto sehen darf.
- **Welche Fotos ein Client sieht:** `Rights.photo_accounts: Selection`, wie `mail_accounts`. Was nicht
  freigegeben ist, gibt es für ihn nicht (`not-found`, vor dem Lesen gefiltert, auch Zähler, Alben,
  Orte und Tage). Feiner als je Konto (je Album oder Ordner) ist erst ein späterer Plan: Ein Foto
  liegt in Zeitleiste, Orten und mehreren Alben zugleich, und eine Auswahl je Album ließe sich über die
  Zeitleiste leicht umgehen.
- **`set_details`** ist `Write`, nicht `Delete`: Nextcloud legt die Korrektur als Metadaten neben die
  EXIF-Werte der Datei; „Reset“ stellt den Wert aus der Datei wieder her (`taken: null`).
- **Freigaben** zeigen, was wirklich geschieht, aus dem Index des Kerns (`approvals::Facts`), nie aus
  dem Text des Clients: Albumname, Zahl der Fotos und ihr Zeitraum („Delete 3 photos from Oct 4, 2025“),
  bei `share_album` die Empfänger und ob ein öffentlicher Link entsteht.
- **Öffentliche Links** sind ein Weg zu den Bildern ohne das Recht „Bildinhalte“. Darum bekommt die
  Adresse eines Links nur der App-Schlüssel; Clients sehen nur, ob es einen gibt.
- **Freigaben für jeden Schreibbefehl:** Wer „Freigabe bei jeder Änderung“ wählt, sieht auch bei
  `set_favorite`, `add_to_album`, `save_frame` oder `name_person` Fakten aus dem Index. Bei
  `share_album` löst der Kern die Empfänger vorher zu festen Benutzer- und Gruppenkennungen auf; die
  Freigabe zeigt genau diese und führt genau sie aus.
- **Rahmen** sieht ein Client nur, wenn er die Quelle des Rahmens sieht; sonst `not-found`, auch bei
  `save_frame` und `delete_frame`. Benennt jemand ein Album um, ziehen die Rahmen mit.
- **„Check again“** bei fehlender Zusatz-App ist `accounts.sync`, das es schon gibt: Jeder Abgleich
  prüft neu, ob Photos, Recognize und Memories da sind.
- **„Slideshow“** in der Foto-App öffnet einen Bilderrahmen mit dieser Quelle (`save_frame` und
  `open_app`), statt eine zweite Diashow zu bauen. Bis FR1.4 fehlt der Knopf.
- **Zustand des Foto-Dienstes** je Konto (Photos, Recognize, Memories da oder nicht, Index läuft, Zahl)
  steht als neues optionales Feld in `accounts.list_accounts` (kommt in PH1.1); dafür gibt es keinen
  eigenen Befehl.
- **Vor, Pause und Zurück im Rahmen** sind Ansichtszustand wie das Blättern im Kalender, kein Befehl.
  Eine Steuerung von außen (`photos.control_frame`, etwa für ein Stream Deck) ist eine spätere Idee.

## Oberfläche

**`torrophotos`** nutzt `torro_gui::shell`, Entwurf `prototypes/photos/photos.html`:
- **Seitenleiste:** Timeline, Favorites, Videos, On this day, Gruppe „Library“ (Albums, People, Places,
  Tags), Gruppe „Albums“ (angeheftete Alben), Konten. Ab zwei Konten gruppiert je Konto.
- **Zeitleiste:** Tage als Gruppen mit Datum und Ort, Raster quadratischer Kacheln mit 2 px Fuge,
  Video-Dauer und Favorit auf der Kachel, Sprungleiste der Jahre rechts. Kopfleiste: Years | Months |
  Days, Filter, „New album“.
- **Auswahl:** Die Kopfleiste wird zur Auswahlleiste: Add to album, Favorite, Tags, Date & place,
  Delete. Ein Tag lässt sich über seinen Kopf ganz wählen; Umschalt-Klick wählt einen Bereich.
- **Betrachter:** Bild auf dem Grund, Pfeile, Filmstreifen; rechts die Eigenschaften (Aufnahme, Ort mit
  kleiner Karte, Kamera, Datei, Ordner, Personen, Alben, Tags, Konto). In Mittel und Schmal stehen die
  Eigenschaften als Chips unter dem Bild. Datum und Ort ändert ein Dialog mit Ortssuche.
- **Alben, Album, Personen, Orte, An diesem Tag** wie im Entwurf. Ohne Recognize zeigt „People“ den
  Hinweis aus Entscheidung 1.
- **Leer, lädt, Fehler:** „No photos yet“ mit „Choose photo folders“; beim ersten Index die fertigen
  Tage und darunter ruhige Platzhalter.
- **Mini und Leiste** sind Anzeigen für „An diesem Tag“: ein Bild mit „3 years ago“ bzw. ein Streifen.
- **Videos** zeigt Version 1 als Kachel mit Dauer; Abspielen öffnet Nextcloud im Browser (Iced hat
  keinen Videospieler).
- **Steuerzentrale:**
  - Kontoseite mit dem Dienst „Photos“ (Zustand, Foto-Ordner, Recognize und Memories da oder nicht)
  - Rechte-Zeile `photos` mit Kontenauswahl und dem Schalter „See image content“
  - beim Assistenten zusätzlich „Only local models“

## Bilderrahmen

**`torroframe`**, Entwurf `prototypes/photos/frame.html`. Eine Kachel, die ein Foto oder eine Diashow
zeigt.

- **Ruhezustand:** nur das Bild, randlos, unten links eine leise Bildunterschrift (Album, Datum, Ort;
  abschaltbar). Keine Bedienelemente ohne Mauszeiger.
- **Beim Überfahren:**
  - oben: Quelle als Menü („Tuscany 2024 ▾“), „Open in Photos“, Einstellungen
  - unten: Datum und Ort, „38 / 214“, Zurück, Pause, Weiter, darunter ein dünner Fortschrittsbalken
- **Einstellungen** (schwebende Karte, gespeichert mit dem Fenster):
  - Show: One photo | Slideshow
  - From: Album, Person, Place, Favorites, On this day, All photos (bei „One photo“: ein Foto wählen)
  - Next photo every: 10 s, 1 min, 5 min, 1 h, Daily
  - Order: By date | Shuffle
  - Fit: Fill tile | Whole photo (der Rand zeigt eine unscharfe Kopie statt Schwarz)
  - Caption: Off | Date | Date and place
- **Größen:** Mini (nur Icons beim Überfahren), Schmal, Breit und Leiste (Panorama-Ausschnitt). Ein
  Hochformat in breiter Kachel wird bei „Fill tile“ beschnitten, bei „Whole photo“ eingepasst.
- **Leer:** „Choose what to show“ mit „Set up frame“. **Offline:** zeigt die gecachten Bilder weiter, mit
  dem Hinweis „Offline · showing saved photos“.
- **Dauerbetrieb:** Ein Bildwechsel alle n Sekunden mit kurzem Überblenden; dazwischen zeichnet die App
  nichts neu. Kein langsamer Zoom (Ken-Burns), weil er die GPU dauernd beschäftigt. Bilder in
  Kachelgröße aus dem Vorschau-Cache, nie Originale.
- **Wie die Wahl in die Vorlage kommt:** Ein Rahmen ist ein Eintrag im Kern (`photos.save_frame`) mit
  Kennung. Das Fenster startet immer mit `torroframe --frame <id>`, und das hält eine Workspace-Vorlage
  als Startbefehl fest. Startet jemand `torroframe` ohne Kennung (Starter, Terminal), zeigt der Rahmen
  „Set up frame“; beim ersten Speichern legt er den Eintrag an und startet sich mit `--frame <id>` neu,
  an derselben Stelle. Ändert man später etwas, ändert sich nur der Eintrag, der Startbefehl bleibt.
- **Aus der Foto-App:** „Show in frame“ an Album, Person, Ort und Favoriten legt einen Rahmen an und
  öffnet ihn (`photos.save_frame`, dann `apps.open_app` mit `target: "frame:<id>"`).
- **Für Agenten:** „Zeig mir die Toskana-Fotos als Diashow“ wird zu `photos_save_frame` und
  `apps_open_app`. Der Agent erfährt dabei nur, was läuft, nie die Bilder; das Recht „Bildinhalte“
  bleibt davon unberührt, weil der Rahmen mit dem App-Schlüssel liest.

## Sicherheit

- **Kein Inhalt im Protokoll:** Protokolliert werden Befehl und Konto. Kennungen von Fotos, Alben,
  Orten, Personen und Tags stehen nur als Konto plus Prüfsumme da (`audit::ids`). Namen, Daten und
  Orte nie, ebenso wenig Suchbegriffe (`find_recipients`), Datumsfilter und die Ordner aus
  `set_folders`.
- **Ortsdaten sind empfindlich.** GPS steht nur in `get_photo` und in den Ortsnamen, nicht in
  `list_photos` (dort nur der Ortsname). Ein Agent mit Leserecht sieht trotzdem, wo jemand wann war.
  Das sagt die Rechte-Zeile in der Steuerzentrale („Read: dates, places, albums, people …“).
- **Vorschau-Cache und Index** liegen 0600 im Cache- bzw. Datenverzeichnis und verschwinden mit dem
  Konto oder bei Remote Wipe.
- **Keine Bilder aus fremden Quellen:** Vorschaubilder kommen nur vom eigenen Server. Die kleine Karte
  im Betrachter und die Karte in PH4 laden Kartenkacheln von außen; welche Quelle, klärt PH4 (siehe
  „Offen“). Bis dahin zeigt der Betrachter den Ort als Text ohne Karte.
- **Recognize-Schlüssel** nur im Speicher des Kerns, nie auf der Platte, nie in einer Antwort, als Typ
  `Secret` ohne `Debug` und nie in Fehlertexten (auch nicht im HTML-Auszug, aus dem er gelesen wird).
- **Bilder sind auch Daten:** Text in einem Foto (ein Schild, ein Screenshot) kann eine eingeschleuste
  Anweisung sein. Die Beschreibung von `photos_get_preview` sagt das.

## Offen (in der jeweiligen Aufgabe klären)

- **Recognize-Schlüssel ohne Memories (PH2.1):** Der Schlüssel steht im Initial State der Photos-Seite.
  Ihn aus dem HTML zu lesen ist zerbrechlich. Prüfen, ob es einen stabileren Weg gibt (eigene Route,
  `occ`-Einstellung `require_api_key`, Rückfrage im Recognize-Projekt). Bis dahin: Personen nur, wenn
  Memories da ist oder der Admin den Schlüssel abgeschaltet hat; sonst der Hinweis aus Entscheidung 1.
- **Kartenkacheln (PH4):** OpenStreetMap erlaubt Apps keine starke Nutzung seiner Kacheln, und jede
  Kachel verrät dem Anbieter, wohin man schaut. Prüfen: die Kachel-URL aus Memories übernehmen, eine
  Einstellung für einen eigenen Kachel-Server, oder Vektorkarten lokal.
- **Bekommt ein Agent als Antrieb des Assistenten Fotos?** Läuft der Assistent mit Claude Code, gehen
  Antworten an dessen Anbieter. Vorschlag: Metadaten ja (wie Kalender), Bildinhalte nur mit der Stufe
  „jedes Modell“. Vor PH1.1 entscheiden; kommt `Area::Photos` doch in `NEVER_TO_AGENTS`, ändert PH1.1
  auch `crates/torroworkplace/src/assistant/engines/mod.rs`.
- **Wie schnell ist `SEARCH` bei großen Sammlungen?** Gegen einen Server mit über 50 000 Fotos messen
  (PH1.2). Ist es zu langsam, nutzt der Abgleich `memories/api/days`, wenn Memories da ist.
- **Überblenden im Rahmen:** Die Design-Regeln erlauben 120–200 ms. Für einen Bildwechsel wirken
  400–600 ms ruhiger. In FR1.3 mit beiden Werten ansehen und dann in `torro-design` festhalten.
- **„Reset“ von Datum und Ort (PH1.2):** Löscht `PROPPATCH` mit leerem Wert die Korrektur auf dem
  Server, oder schreibt der Kern den Wert aus `nc:metadata-photos-exif` zurück? Gegen einen Server prüfen.
- **Was beim Entfernen eines Mitwirkenden passiert (PH3.1):** Vermutlich verlassen seine Fotos das Album
  (`AlbumMapper` der Photos-App). Prüfen; bis dahin ist `unshare_album` `Delete`.
- **Hochladen** ist ein späterer Plan (Entscheidung 2), ebenso Live-Fotos und Video im Rahmen.
