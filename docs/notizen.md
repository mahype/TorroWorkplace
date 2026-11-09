# Notizen: App `torronotes` auf Nextcloud

Stand: 2026-11-09. **Konzept, Entscheidungen vom 2026-10-14 und 2026-11-09 eingearbeitet** (Issue
[#602](https://github.com/mahype/TorroWorkplace/issues/602): Nextcloud nachgeprüft, Vergleich mit bekannten Apps,
Inhaltsarten, Zulieferer, Abgrenzung zum Schwarzen Brett, [Mockups](prototypes/notes/index.html)). Änderungen an
früheren Entscheidungen stehen als Vorschlag in Entscheidung 11 und 12. Aufgaben:
[tasks/notizen.md](tasks/notizen.md). Beantwortet die Idee „Notizen / Scratchpad“ aus dem
[App-Katalog](app-catalog.md): Wo lege ich schnell etwas ab, auch für Agenten?

## Ziel

Eine Notizen-App der Suite, die auf **Nextcloud Notes** aufbaut. Sie funktioniert offline und wird
wie jede Funktion der Suite dreimal angeboten: in der Oberfläche (`torronotes`), über die API
(Bereich `notes`) und über MCP (`notes_*`), also auch für den Assistenten.

Seit Entscheidung 12 heißt „aufbauen“: Die Notizen sind die Dateien im Notizordner der
Notes-App, gelesen und geschrieben per WebDAV; die Notes-App selbst ist ein Zusatz.

## Was Nextcloud bietet

**Nextcloud Notes** (`nextcloud/notes`) legt jede Notiz als Datei (`.txt` oder `.md`) im Ordner
„Notes“ des Nutzers ab. Kategorien sind Unterordner, `/` trennt Unterkategorien. Andere Clients
(Nextcloud-Dateien, Text, die Notes-Apps für Android und iOS) sehen also dieselben Dateien.

Die App hat eine eigene REST-API unter `/index.php/apps/notes/api/v1/`, angemeldet mit Basic Auth
(Anmeldename und App-Passwort, wie DAV). Sie spricht JSON. Quellen:
[API v1](https://github.com/nextcloud/notes/blob/main/docs/api/v1.md),
[Übersicht und Versionen](https://github.com/nextcloud/notes/blob/main/docs/api/README.md).

| API | ab Notes | Was |
|---|---|---|
| 1.0 | 3.3 (2020) | `GET/POST /notes`, `GET/PUT/DELETE /notes/{id}`; Titel getrennt vom Inhalt |
| 1.1 | 3.4 | Filter `category` |
| 1.2 | 4.1 (2021) | `etag` je Notiz, `If-Match` mit **412** samt Serverfassung, `readonly`, Abruf in Teilen (`chunkSize`, `chunkCursor`), `GET/PUT /settings` |
| 1.3 | 4.5 | eigene Dateiendungen |
| 1.4 | 6.1 (2026) | Anhänge: `GET /attachment/{id}?path=…`, `POST /attachment/{id}` (multipart `file`) |

**Felder einer Notiz:** `id`, `etag`, `readonly`, `content` (Markdown), `title`, `category`
(leer = ohne Kategorie), `favorite`, `modified` (Unix-Zeit).

**Eigenheiten, auf die wir achten:**
- **Der Titel ist der Dateiname.** Der Server bereinigt ihn und hängt bei Gleichstand eine Zahl an.
  Wir übernehmen immer den Titel aus der Antwort.
- **`readonly`:** Andere haben die Notiz ohne Schreibrecht geteilt. Nur `favorite` lässt sich
  ändern, alles andere beantwortet der Server mit 403.
- **Abgleich:** `GET /notes` mit `pruneBefore` (der letzte `Last-Modified`) liefert unveränderte
  Notizen nur als `id`. Löschungen stehen erst in der **letzten** Teilantwort (die ohne
  `X-Notes-Chunk-Cursor`). Das `ETag` der Liste spart Übertragung.
- **Versionen erkennen:** Die Capabilities enthalten `notes.api_version`, zum Beispiel
  `["0.2", "1.4"]`, und `notes.version`. Jede Antwort enthält außerdem den Header
  `X-Notes-API-Versions`. Unbekannte Felder werden ignoriert, unbekannte Statuscodes wie ihre
  Klasse behandelt (405 wie 400).
- **Fehler:** 400 (ungültig oder Endpunkt unbekannt), 401, 403 (schreibgeschützt), 404, 412
  (geändert), 507 (Speicher voll). Bei gesperrten Dateien ist auch 423 möglich.

**Was die Notes-API nicht kann**, und wie wir es lösen:

| Fehlt | Weg |
|---|---|
| Volltextsuche | eigener Index im Kern (SQLite FTS5 wie bei Mail) |
| Versionen, Papierkorb | WebDAV `versions/<uid>/…`, `trashbin/<uid>/…`, Stufe N3 |
| Kategorie umbenennen | der Kern ändert `category` jeder Notiz der Kategorie |
| Teilen | OCS-Sharing der Dateien, **nicht** in diesem Plan |
| Vorlagen (Notes 6.1) | keine API gefunden, nicht in diesem Plan |
| Server ohne Notes-App | die Dateien direkt per WebDAV, Stufe N4 |

**Mindestversion: API 1.2** (Notes 4.1). Ohne `etag` und 412 können wir keine Konflikte erkennen.
Ältere Server behandeln wir wie „Notes-App fehlt“. Anhänge brauchen 1.4, sonst WebDAV (siehe N2).

### Nachgeprüft am 2026-11-09

Geprüft an Doku und Quellcode von Notes, Text und Server (Quellen am Ende des Abschnitts, alle abgerufen
2026-11-09). Was den Text oben berichtigt, steht hier; der Text oben bleibt als Stand vom 2026-10-14.

**Notes-App**
- **Version:** Notes 6.1.0 vom 2026-09-15, für Nextcloud 33–35 [2][3].
- **API 1.4 gibt es seit Notes 4.12.3** (August 2025), nicht erst seit 6.1 [1][7]. Neu in 6.1 sind:
  - Anhänge liegen im versteckten Ordner `.attachments.<id>/` mit ihrem Namen; vorher lagen sie mit
    zufälligem Namen offen im Kategorieordner.
  - **Anhänge löschen:** `DELETE /attachment/{id}?path=…`, nur innerhalb von `.attachments.<id>`.
- **`notes.api_version`** nennt nicht jede Minor-Version (heute `["0.2","1.3","1.4"]`, ohne „1.2“). Geprüft wird
  die höchste 1.x [5].
- **Die Notiz-`id` ist die Datei-ID** (`oc:fileid`) [16].
- **Das `etag` der Notes-API ist nicht das ETag der Datei.** Es ist eine Prüfsumme über Titel, Kategorie, Favorit,
  Inhalt und mehr [17]. Notes-API und WebDAV lassen sich darum nicht mischen.
- **Favoriten** speichert die App im Favoriten-Tag von Files, demselben Speicher wie `oc:favorite` per WebDAV
  [10][11].
- **Einstellungen** (`notesPath`, `fileSuffix`) liegen in den Benutzereinstellungen, nicht als Datei. Den Ordner
  nennt die Capability `notes.notes_path` (seit 4.12), die Endung nur `GET /settings` [5][13]. Der Standardordner
  ist **übersetzt** („Notizen“ auf Deutsch), nicht immer „Notes“.
- **Titel = Dateiname ohne Endung.** Die App entfernt `* | / \ : " < > ?`, führende Punkte und Leerzeichen,
  kürzt auf 100 Zeichen, ein leerer Titel wird „New note“, bei Gleichstand „ (2)“, „ (3)“ [15].
- **Als Notiz gelten** die Endungen `txt`, `md`, `markdown`, `org`, `note` und die eigene Endung; Ordner
  `.attachments.<n>` sind nie eine Kategorie [6].
- **Vorlagen:** keine Notes-API. Notes 6.1 nimmt die Vorlagen von Files (OCS
  `/ocs/v2.php/apps/files/api/v1/templates`, braucht die App Text) [8][9].

**Nextcloud Text**
- Markdown: CommonMark mit Tabellen, Aufgabenlisten `- [ ]`/`- [x]`, Durchstreichen, Callouts, Fußnoten, Mathe;
  kein HTML [18][19].
- **Bilder und Dateien** legt Text in `.attachments.<fileId>/` neben die Datei, relativ und URL-kodiert verlinkt
  (`![name](.attachments.123/datei%20name.png)`). Weil Notiz-`id` und Datei-ID gleich sind, nutzen Notes und Text
  **denselben Ordner** [6][20].
- **Text löscht Anhänge, auf die der Inhalt nicht verweist** (`cleanupAttachments`, nur bei `text/markdown`)
  [20]. Torro schreibt darum den Verweis vor oder gleich mit dem Hochladen. Wann das Aufräumen läuft, ist nicht
  bestätigt.

**Files und WebDAV**
- **PROPFIND** liefert `getetag`, `getlastmodified`, `oc:fileid`, `oc:favorite`, `oc:permissions`, `oc:tags`;
  `oc:favorite` lässt sich per `PROPPATCH` setzen [11][12].
- **Bedingtes Schreiben:** `If-Match` gibt **412**, wenn das ETag nicht passt, für PUT, MOVE und DELETE;
  `If-None-Match: *` gibt 412, wenn die Datei schon da ist [25][26].
- **Ordner-ETags ändern sich**, wenn sich darunter etwas ändert [30]. `Depth: infinity` ist in Nextcloud an [24].
- **`REPORT sync-collection` gibt es für Dateien nicht**, nur für Kalender und Adressbücher [29]. (Der Skill
  `nextcloud` sagt es anders; das ist dort zu berichtigen.)
- **Versionen** `versions/<uid>/versions/<fileid>` und **Papierkorb** `trashbin/<uid>/trash` wie im Konzept [27][28].
- **`notify_push`** meldet geänderte Datei-IDs (`notify_file_id`) [31].
- **Verbotene Namen** stehen seit Nextcloud 30 in den Capabilities (`files.forbidden_filename_characters` usw.) [32].

**Mobile Apps:** Die Notes-Apps für Android und iOS sprechen die Notes-API; **ohne Notes-App auf dem Server
laufen sie nicht** [33][34]. Mit Notes-App sehen sie Torros Dateien, wenn Torro im `notesPath` mit einer
erkannten Endung und nach den Titelregeln schreibt.

Quellen: [1] [Notes API v1](https://github.com/nextcloud/notes/blob/main/docs/api/v1.md) ·
[2] [Notes 6.1.0](https://github.com/nextcloud/notes/releases/tag/v6.1.0) ·
[3] [App Store](https://apps.nextcloud.com/apps/notes) ·
[5] [Capabilities.php](https://github.com/nextcloud/notes/blob/main/lib/AppInfo/Capabilities.php) ·
[6] [NotesService.php](https://github.com/nextcloud/notes/blob/main/lib/Service/NotesService.php) ·
[7] [PR 1998](https://github.com/nextcloud/notes/pull/1998) · [8] [PR 1971](https://github.com/nextcloud/notes/pull/1971) ·
[9] [Files OpenAPI](https://github.com/nextcloud/server/blob/master/apps/files/openapi.json) ·
[10] [TagService.php](https://github.com/nextcloud/notes/blob/main/lib/Service/TagService.php) ·
[11] [TagsPlugin.php](https://github.com/nextcloud/server/blob/master/apps/dav/lib/Connector/Sabre/TagsPlugin.php) ·
[12] [WebDAV basic](https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/basic.html) ·
[13] [SettingsService.php](https://github.com/nextcloud/notes/blob/main/lib/Service/SettingsService.php) ·
[15] [NoteUtil.php](https://github.com/nextcloud/notes/blob/main/lib/Service/NoteUtil.php) ·
[16] [Note.php](https://github.com/nextcloud/notes/blob/main/lib/Service/Note.php) ·
[17] [MetaService.php](https://github.com/nextcloud/notes/blob/main/lib/Service/MetaService.php) ·
[18] [Text markdownit](https://github.com/nextcloud/text/blob/main/src/markdownit/index.ts) ·
[19] [taskLists.ts](https://github.com/nextcloud/text/blob/main/src/markdownit/taskLists.ts) ·
[20] [AttachmentService.php](https://github.com/nextcloud/text/blob/main/lib/Service/AttachmentService.php) ·
[24] [Sabre Server.php](https://github.com/nextcloud/server/blob/master/apps/dav/lib/Connector/Sabre/Server.php) ·
[25] [sabre/dav Server.php](https://github.com/sabre-io/dav/blob/master/lib/DAV/Server.php) ·
[26] [Node.php](https://github.com/nextcloud/server/blob/master/apps/dav/lib/Connector/Sabre/Node.php) ·
[27] [Versionen](https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/versions.html) ·
[28] [Papierkorb](https://docs.nextcloud.com/server/latest/developer_manual/client_apis/WebDAV/trashbin.html) ·
[29] [Directory.php](https://github.com/nextcloud/server/blob/master/apps/dav/lib/Connector/Sabre/Directory.php) ·
[30] [Propagator.php](https://github.com/nextcloud/server/blob/master/lib/private/Files/Cache/Propagator.php) ·
[31] [notify_push](https://github.com/nextcloud/notify_push/blob/main/DEVELOPING.md) ·
[32] [PublicCapabilities.php](https://github.com/nextcloud/server/blob/master/apps/files/lib/PublicCapabilities.php) ·
[33] [notes-android](https://github.com/nextcloud/notes-android) · [34] [notes-ios](https://github.com/nextcloud/notes-ios)

## Basis: WebDAV, Notes-App als Zusatz

Offene Frage des Issues: Bleibt die Notes-App die Basis, oder reicht ein Ordner per WebDAV? **Antwort
(Entscheidung 12): Torro liest und schreibt die Notizen als Dateien im Notizordner per WebDAV. Die Notes-App ist
ein Zusatz, wenn sie installiert ist.** Die Recherche trägt das:

| Aufgabe | per WebDAV | was die Notes-App beiträgt |
|---|---|---|
| Liste, Abgleich | PROPFIND auf den Notizordner; erst `Depth: 0`, nur bei geändertem Ordner-ETag `Depth: 1` in die geänderten Unterordner, `Depth: infinity` nur beim ersten Laden; gelöscht oder verschoben ist, was an `oc:fileid` fehlt bzw. woanders auftaucht | Abruf in Teilen mit `pruneBefore` – Komfort, nicht nötig |
| Ändern, Konflikt | `PUT` mit `If-Match` (Datei-ETag) → 412; neu mit `If-None-Match: *` | eigenes `etag`, nicht mit WebDAV mischbar |
| Titel, Kategorie | Titel = Dateiname, Kategorie = Ordner; Umbenennen und Verschieben per `MOVE` mit `If-Match` und `Overwrite: F` | bereinigt Titel selbst – Torro übernimmt dieselben Regeln (oben) und die verbotenen Namen aus den Capabilities |
| Favorit | `oc:favorite` per `PROPPATCH` | derselbe Speicher |
| Schreibgeschützt | `oc:permissions` ohne `W` | `readonly` |
| Kennung | `oc:fileid` | dieselbe Zahl |
| Anhänge | `PUT`/`DELETE` in `.attachments.<fileid>/` | Endpunkte ab 1.4 bzw. 6.1 – nicht nötig |
| Versionen, Papierkorb | WebDAV wie bisher (N3) | – |
| Einstellungen | – | Ordner (Capability `notes.notes_path`) und Endung (`GET /settings`) – **das ist der Zusatz** |

**Was das bringt:**
- Läuft auf jeder Nextcloud, mit und ohne Notes-App; die bisherige Stufe N4 geht in N1 auf. Andere WebDAV-Server
  sind kein Ziel: Kennung, Favorit und Schreibschutz hängen an `oc:fileid`, `oc:favorite` und `oc:permissions`.
- Ein ETag-Modell, ein Anbieter statt zweier, eine Kennung für Notiz, Versionen, Teilen und `notify_push`.
- Unabhängig von Änderungen der Notes-API.

**Was Torro dafür selbst tut:**
- **Ordner und Endung:** Mit Notes-App übernimmt Torro `notesPath` und `fileSuffix`, damit Web und Handy dieselben
  Notizen sehen; `notes.update_settings` schreibt dann über die Notes-API. Ohne Notes-App wählt der Mensch den
  Ordner (Vorschlag: der übersetzte Standardname, also „Notes“ bzw. „Notizen“), Endung `.md`.
- **Titel bereinigen** nach den Regeln der Notes-App, sonst sähe das Handy eine andere Notiz.
- **Lesen** aller Endungen, die die Notes-App als Notiz zählt; Ordner und Dateien mit Punkt am Anfang sind keine
  Notizen.
- **412 unterscheiden:** Nur ein 412 auf `If-Match` heißt „woanders geändert“. Dann vergleicht der Kern erst den
  Inhalt (ob sich das Datei-ETag auch bei bloßen Metadaten ändert, ist nicht bestätigt), bevor er zusammenführt.
  Ein 412 auf `If-None-Match: *` (neu anlegen) oder auf `MOVE` mit `Overwrite: F` (umbenennen, verschieben) heißt
  „Name belegt“: Beim Anlegen zählt der Kern den Titel hoch wie die Notes-App („Meeting (2)“), beim Umbenennen
  antwortet er mit `conflict` ohne Serverfassung. Nie wird eine fremde Notiz als Serverfassung zusammengeführt.
- **Favoriten anderer Clients:** Ein Stern ist ein Tag in Files; ob er ETag von Datei oder Ordner ändert, ist nicht
  bestätigt. Darum holt jeder Abgleich die Favoriten zusätzlich mit einem `REPORT oc:filter-files` auf
  `oc:favorite` (eine Antwort mit allen Favoriten) und gleicht sie ab. N1.12 prüft das am Server.

**Risiken:** viele Dateien (darum der Abstieg über Ordner-ETags), ETags externer Speicher oder fremder Freigaben,
die verzögert kommen (nicht bestätigt), Favoriten ohne ETag-Änderung (nicht bestätigt, oben), und das Aufräumen von Anhängen durch Text (oben). Ohne Notes-App laufen die
mobilen Notes-Apps nicht; das ist eine Grenze der Handy-Apps, nicht von Torro.

## Vergleich mit bekannten Apps (2026-11-09)

Recherche vom 2026-11-09, Quellen am Ende des Abschnitts. Gefragt war: Was macht die Apps beliebt, was
davon passt zu Torro, und wie lässt es sich in Markdown-Dateien in Nextcloud abbilden?

| App | Beliebt wegen | Passt zu Torro | Weg in Nextcloud |
|---|---|---|---|
| **Apple Notes** | sehr schnelles Erfassen (Quick Note, Kurzbefehl, Teilen-Menü), `#tags` im Text, Smart Folders als gespeicherte Filter, Checklisten, Links auf andere Notizen | Quick Note, `#tags`, gespeicherte Filter, Checklisten | Tags als `#tag` im Text; gespeicherte Filter im Kern, nicht als Datei; Checklisten `- [ ]` |
| **Obsidian** | lokale `.md`-Dateien in einem gewöhnlichen Ordner, Live Preview als Standard (dazu Quelltext und Lesesicht), `[[Links]]` und Backlinks, Vorlagen, Web Clipper | am nächsten an Torro: offene Dateien, Live Preview, Vorlagen-Ordner | Unterordner = Kategorien; Vorlagen als `.md` in einem Ordner; Backlinks aus dem Index des Kerns |
| **Notion** | Block-Editor mit Markdown-Kürzeln, Datenbanken, Relationen, Teamarbeit; offline nur für vorher gewählte Seiten | kaum: nur Kürzel im Editor | Datenbanken passen nicht in Dateien |
| **Bear** | Markdown wird beim Tippen formatiert, die Zeichen bleiben sichtbar; verschachtelte Tags `#a/b`, `[[Links]]` | Vorbild für den Editor, verschachtelte Tags | `#a/b` im Text, nicht als Kategorie (Kategorien sind schon Ordner) |
| **Google Keep** | kleine Notizen in Sekunden, Widget, Checklisten, Labels, Farben, Anheften | schnelles Erfassen, Checklisten, Anheften | Anheften = `favorite`; Farben nicht |
| **OneNote** | freie Leinwand, Handschrift; Quick Note per globaler Taste; Mail an eine Adresse, `@Abschnitt` im Betreff wählt das Ziel | globale Taste, Ziel beim Ablegen wählen | Kategorie beim Anlegen |
| **Joplin** | Open Source, offline, Web Clipper, Sync auch über Nextcloud/WebDAV | Warnbeispiel für das Format: Dateinamen als Hex, Metadaten am Dateiende, die Notes-App kann sie nicht lesen | gerade **nicht** so: lesbare Dateinamen, keine Metadaten in der Datei |

**Übernehmen:**

| Was | Warum | Stufe |
|---|---|---|
| **Schnell erfassen:** Feld „Quick note“ in Mini und Leiste (Entscheidung 10), später eine globale Taste über den Kern | wichtigster Grund, warum diese Apps benutzt werden (Apple, OneNote, Keep) | N1, Taste später |
| **Tags als `#tag` im Text**, auch verschachtelt `#a/b` | steht in der Datei, kein Metadatenformat nötig; andere Clients zeigen es als Text | N1 (Index im Kern, Seitenleiste „Tags“) |
| **Checklisten** `- [ ]` / `- [x]` | GFM-Standard, Nextcloud Text zeigt sie als Kästchen | N1 |
| **Favoriten** | gibt es schon (`favorite`), entspricht „Anheften“ bei Keep | N1 |
| **Links zwischen Notizen** als gewöhnliche relative Markdown-Links `[Titel](../Kategorie/Titel.md)`; beim Tippen von `[[` öffnet sich eine Auswahl, eingefügt wird der normale Link | `[[…]]` lösen Notes und Text nicht auf (nicht bestätigt); Dateinamen ändern sich beim Umbenennen, darum zieht der Kern Links nach | N1 Links, Nachziehen und Backlinks später |
| **Backlinks** aus dem Index des Kerns, nichts davon in den Dateien | Obsidian, Bear | später (eigene Aufgabe) |
| **Gespeicherte Suchen** statt Smart Folders, in den Einstellungen des Kerns | Apple | später |
| **Live-Vorschau** im Editor (Entscheidung 11) | Obsidian, Bear, Nextcloud Text formatieren alle beim Tippen | N1.7 |
| **Vorlagen** als `.md` in einem eigenen Ordner | Obsidian; ob das zu den Vorlagen der Notes-App passt, ist nicht bestätigt | später (eigener Plan) |

**Bewusst nicht:**
- **Datenbanken, Relationen, Ansichten** (Notion): passen nicht in Markdown-Dateien; Aufgaben und Termine haben
  eigene Apps.
- **Freie Leinwand, Handschrift, Zeichnen** (OneNote, Apple).
- **Metadaten in der Datei** (Joplin, YAML-Frontmatter): Die Notes-App zeigt sie als Text, die Dateien blieben
  nicht lesbar.
- **Eigene Datenbank als Wahrheit** (Bear, Joplin): Die Wahrheit ist der Ordner in Nextcloud, der Speicher im Kern
  ist Cache und Warteschlange (Entscheidung 6).
- **Farben je Notiz** (Keep): kein Feld in der Datei. Farben gehören dem Schwarzen Brett.
- **Eigene Mail-Adresse für Notizen** (OneNote, Joplin): bräuchte einen Dienst und öffnete einen Weg für
  eingeschleusten Text. „Save as note“ in der Inbox reicht.
- **Volles WYSIWYG** (Notion, Nextcloud Text): normalisiert das Markdown beim Speichern und schriebe die Dateien
  anderer Clients um, also mehr Konflikte (typisch für ProseMirror, für Nextcloud Text nicht bestätigt).

Quellen (abgerufen 2026-11-09): [Obsidian: Edit and read](https://obsidian.md/help/edit-and-read),
[Obsidian Web Clipper](https://obsidian.md/help/web-clipper),
[Apple: Tags und Smart Folders](https://support.apple.com/en-nz/102288),
[Bear 2](https://bear.app/faq/whats-new-in-bear-2/),
[Bear und Markdown](https://blog.bear.app/2023/11/why-bear-is-built-on-markdown-and-what-that-means/),
[Notion offline](https://www.notion.com/de/blog/how-we-made-notion-available-offline),
[OneNote: Notizen per Mail](https://support.microsoft.com/en-au/office/use-email-to-send-notes-to-onenote-notebooks-f513b641-ab0d-41cc-8dab-2a66f5d141e7),
[Joplin: Dateiformat beim Nextcloud-Sync](https://discourse.joplinapp.org/t/nextcloud-sync-file-formats/1398),
[Nextcloud: Markdown-App Text](https://nextcloud.com/blog/the-ever-expanding-markdown-app/). Google Keep nach
[How-To Geek](https://www.howtogeek.com/694042/how-to-export-your-google-keep-notes-and-attachments/) (Drittquelle).

## Inhaltsarten

Eine Notiz ist **eine Markdown-Datei**. Alles, was sie enthält, steht als gewöhnliches Markdown darin, damit
die Notes-App, Nextcloud Text und die mobilen Apps dieselbe Notiz lesen können. Was Torro mehr zeigt (Kästchen,
Bilder, Chips), ist Darstellung, nie ein eigenes Format.

| Art | Form in der Datei | Speicherweg | Stufe | Was andere Clients sehen |
|---|---|---|---|---|
| **Text** | Markdown (CommonMark mit GFM: Überschriften, Listen, Zitat, Code, Tabellen) | die Datei selbst | N1 | dasselbe |
| **Checkliste** | `- [ ] offen`, `- [x] erledigt` | die Datei | N1 | Kästchen in Nextcloud Text und der Notes-App |
| **Tag** | `#tag`, verschachtelt `#a/b`, im Text | die Datei; Index im Kern | N1 | Text |
| **Link ins Netz** | `[Text](https://…)` | die Datei | N1 | Link |
| **Link auf eine Notiz** | `[Titel](../Kategorie/Titel.md)` relativ | die Datei | N1 | Link auf die Datei (nicht bestätigt, ob die Notes-App ihn öffnet) |
| **Bild** | `![Name](.attachments.<id>/bild.png)` | Datei im versteckten Ordner `.attachments.<id>/` neben der Notiz (wie Text und Notes 6.1), per WebDAV | N2 | Bild in Text und Notes-App |
| **Datei** | `[Vertrag.pdf](.attachments.<id>/Vertrag.pdf)` | wie Bild | N2 | Link auf die Datei |
| **Verweis auf Mail, Aufgabe, Termin, Notiz** | `[Lesbarer Titel](torro:mail:<adresse>:<Message-ID>)`, `torro:task:<uid>`; später `torro:event:<uid>`, `torro:note:<id>` (Vorschlag, Teile URL-kodiert) | die Datei | Mail und Aufgabe N1 (N1.7, N1.10), Termin und Notiz mit ihrem Ziel in `apps.open_app` | lesbarer Text als Link, der woanders nichts tut |

**Verweise (Vorschlag).** Die Suite hat schon Ziele für Verweise: `apps.open_app` öffnet mit `target`
`mail:<adresse>:<Message-ID>` und `task:<uid>`, und `inbox.create_task_from_item` legt dieselbe Form als `link.id`
ab. Vorschlag: ein gewöhnlicher Markdown-Link mit lesbarem Text, dessen Ziel `torro:` plus genau dieses Ziel ist,
die Teile URL-kodiert (`@` als `%40`, `#` als `%23`), damit der Link in jedem Markdown gültig
bleibt. Die Message-ID steht ohne spitze Klammern, wie `Link::mail` sie ablegt. Beispiel:
`[Invoice March – Acme GmbH](torro:mail:anna%40cloud.example:8790%40acme.example)`.
- **Kein zweites Format:** Die Message-ID bleibt gleich, wenn die Mail verschoben wird; Ordner und UID würden
  brechen.
- **Lesbar überall:** Der Text trägt die Bedeutung; in Nextcloud bleibt er ein Link, der nichts öffnet.
- **Torro öffnet nur über den Kern:** Die App zeigt den Verweis als Chip mit Symbol (Mail, Aufgabe, später Kalender
  und Notiz). Ein Klick schickt `apps.open_app` mit dem Ziel; der Kern prüft die Rechte des Lesers. Darf er den
  Eintrag nicht sehen oder gibt es ihn nicht mehr, bleibt es Text (gestrichelter Chip, „Not available“).
- **Termine und Notizen** haben noch kein Ziel in `apps.open_app`. Vorschlag: `event:<uid>` und `note:<id>` kommen
  als eigene Aufgabe (Request, Katalog, MCP-Beschreibung von `apps_open_app`); bis dahin zeigt Torro solche
  Verweise als Text. Notizen verlinken sich bis dahin mit dem relativen Dateilink (oben).
- **Nie eine Aktion:** Ein Verweis öffnet nur, er sendet, löscht oder ändert nichts. Notizinhalte sind fremder Text
  (Entscheidung 3); ein eingeschleuster Link darf nichts auslösen. Erlaubt sind nur die Ziele, die
  `apps.open_app` kennt und die nur anzeigen (`mail`, `task`, später `event`, `note`); `file:` und `template:`
  öffnen ein Schreibfenster bzw. einen Dialog und sind in Notizen **nicht** erlaubt. Jedes andere Schema bleibt
  Text. Links ins Netz öffnen wie bisher erst nach einem Klick im Browser (wie `web_link` im Assistenten).
- **Andere Einträge des Eingangs** (GitHub, Nextcloud, Talk, ntfy) bekommen keinen `torro:`-Verweis: GitHub als
  gewöhnlicher Link ins Netz auf Issue oder PR, die übrigen nur mit Quelle und Zeit im Text.

**Bilder aus dem Netz** (`![](https://…)`) lädt die Vorschau nicht (Abschnitt „Sicherheit“); sie zeigt einen
Platzhalter mit „Load once“.

## Zulieferer

Wer etwas in Notizen schickt, tut es über **dieselben Befehle** wie die App: `notes.create_note` legt an,
`notes.append_to_note` hängt an. Alles, was so hereinkommt, ist `untrusted: true`. Neue Befehle stehen hier nur
als Vorschlag; sie kommen mit dem Plan ihrer Quelle.

| Quelle | Inhalt | Oberfläche | API | MCP | Zugriff | Stufe |
|---|---|---|---|---|---|---|
| **Notizen selbst** | Text | „New note“, Feld „Quick note“ in Mini und Leiste | `notes.create_note` | `notes_create_note` | Write | N1 |
| **Eingang und Mail** | Titel, Text als Zitat, Verweis zurück | „Save as note“ (Blatt, Entscheidung 10) | `inbox.create_note_from_item` | `inbox_create_note_from_item` | Write (`inbox`) + Write in `notes` | N1.10 |
| **„Send to…“** ([#260](https://github.com/mahype/TorroWorkplace/issues/260)) | Text, Mail, Datei, Eintrag | gemeinsames Blatt; „Save as note“ geht dort als Ziel auf | `send.route` mit Ziel Notiz (Vorschlag, mit #260) | über `send.route` | Write in `notes`, Dateien wie N2 | mit #260 |
| **Assistent und Agenten** | Text, Zusammenfassungen, Mitschrift | – (Chat) | `notes.create_note`, `notes.append_to_note` | `notes_create_note`, `notes_append_to_note` | Anlegen und Anhängen ohne Freigabe; Inhalt ändern und Löschen mit Freigabe, Titel, Favorit und Kategorie ohne (Entscheidung 5) | N1.5 |
| **Kalender** | Besprechungsnotiz mit Verweis auf den Termin | „New meeting note“ im Termin (Vorschlag) | `notes.create_note` mit Verweis | `notes_create_note` | Write | später |
| **Aufgaben** | Notiz zur Aufgabe mit Verweis | „New note“ in der Aufgabe (Vorschlag) | `notes.create_note` mit Verweis | `notes_create_note` | Write | später |
| **Zwischenablage** | Text, Bild | Einfügen im Editor; „New note from clipboard“ (Vorschlag) | `notes.create_note`, `notes.add_attachment` | dieselben | Write | N1 Text, N2 Bild |
| **Bildschirmfoto** | Bild | „Insert screenshot“ im Editor (Vorschlag) | `notes.add_attachment` | `notes_add_attachment` | Write | N2 |
| **Schwarzes Brett** ([#348](https://github.com/mahype/TorroWorkplace/issues/348), P3) | Zettel als Notiz | „Save as note“ am Zettel | `pinboard.save_pin_as_note` (im Brett-Konzept) | `pinboard_save_pin_as_note` | Write in `notes` | mit P3 |

- **Die Zwischenablage liest nur die App**, auf eine Handlung des Menschen; ein Agent kommt nie an die
  Zwischenablage. Ihr API-Weg sind die gewöhnlichen Befehle.
- **Anhänge der Quelle** (der Anhang einer Mail) gehen nicht still mit. „Save as note“ zeigt sie und bietet
  „Attach files“ an; ab N2 lädt der Kern sie mit `notes.add_attachment` hoch.
- **[Teilen](teilen.md)** (TL3, `notes.share_card`) ist der umgekehrte Weg: aus einer Notiz heraus. Kein Widerspruch.

## Abgrenzung zum Schwarzen Brett

Das Schwarze Brett ([#348](https://github.com/mahype/TorroWorkplace/issues/348), zurückgestellt, Konzept nur in den
Kommentaren des Issues) hat wie die Notizen ein Feld „Quick note“. Die beiden beantworten verschiedene Fragen:

| | Schwarzes Brett | Notizen |
|---|---|---|
| Frage | Was hängt gerade neben meiner Arbeit? | Wo lege ich etwas ab, damit ich es wiederfinde? |
| Lebensdauer | kurz, mit Ablauf je Zettel (P3) | dauerhaft |
| Ort | lokal im Kern, kein Konto | Nextcloud, auf allen Geräten |
| Form | Text ohne Markdown, Checkliste, Bild und Screenshot, Link, Datei; frei angeordnet, Farbe als Kategorie | Markdown-Datei, Kategorien als Ordner, Tags |

- **„Quick note“ in Notizen** legt eine Notiz **ohne Kategorie** an; der Titel ist die erste Zeile. Das Feld
  steht in Mini und Leiste von `torronotes`.
- **„Quick note…“ am Brett** pinnt einen Zettel an das gewählte Brett. Es legt nie eine Notiz an.
- **Übergang Brett → Notiz:** „Save as note“ am Zettel (`pinboard.save_pin_as_note`, P3) hebt ihn als Notiz ohne
  Kategorie auf, oder in einer Kategorie, die der Mensch wählt. Bilder eines Zettels gehen als Anhang mit, also erst
  ab N2; vorher nur Text, Checkliste und Link. Das Brett ist das Vorzimmer der Notizen. Der Weg
  zurück (Notiz anpinnen) ist nicht geplant.
- **Wer was bekommt:** Was bleiben soll oder auf andere Geräte muss, geht in Notizen; was nur heute zählt, ans Brett.

## Entscheidungen (2026-10-14)

1. **Name:** App `torronotes`, Bereich `notes`, MCP-Tools `notes_*`, Fensterklasse `torro-notes`.
2. **Crates:** `torro-notes` (neu: neutrales Modell, Anbieter-Trait, Zusammenführen, Demo, ohne
   Ein-/Ausgabe, Vorbild `torro-pim`) und `torronotes` (neu: die App). Der Nextcloud-Teil kommt in
   das bestehende `torro-nextcloud` (`notes.rs`), der Kern-Teil nach `crates/torroworkplace/src/notes/`.
   Nextcloud ist ein Anbieter, nicht das Modell.
3. **Notizinhalte sind immer `untrusted: true`.** Das gilt für Titel, Inhalt, Kategorienamen und
   Anhangsnamen. In Notizen landet Text von Agenten, aus Mails und aus geteilten Notizen anderer.
   Würde der Assistent ihn als Anweisung lesen, bliebe eine eingeschleuste Anweisung dauerhaft
   wirksam. Katalogbeschreibungen und MCP-Tools sagen das, wie bei Mail und Eingang.
4. **Rechte je Kategorie**, wie heute je Kalender und Aufgabenliste: `Rights.note_categories: Selection`.
   - **Vererbt:** Ein Recht auf `Arbeit` gilt auch für `Arbeit/Kunde A`, auch für Unterkategorien,
     die später entstehen. `Arbeitszeit` deckt es nicht.
   - **Neue Kategorien auf oberster Ebene bleiben zu.** Wer sie sehen soll, braucht sie in der
     Auswahl oder `All`.
   - **„Ohne Kategorie“ ist ein eigener Eintrag.** Seine Kennung ist der leere Pfad `<konto>/`. Er
     deckt nur Notizen ohne Kategorie, **keine** Unterkategorien.
   - **Die Rechte-Vorlagen öffnen keine Kategorie.** „Read only“, „Read and write“ und „Full access“
     setzen nur die Stufe in `notes`; die Auswahl bleibt leer. Beim Verbinden eines Agenten sind die
     bestehenden Kategorien vorgewählt, und der Mensch bestätigt sie.
   - **Was ein Client nicht sehen darf, gibt es für ihn nicht:** Notizen und Kategorien außerhalb
     der Auswahl sind `not-found` und werden vor dem Lesen ausgefiltert. Das gilt auch für Suche,
     Zähler (auch die Zahl einer Elternkategorie), Papierkorb, Versionen und Anhänge.
   - **Konflikt:** Liegen die eigene Fassung, die Serverfassung und die Basis einer Notiz in
     verschiedenen Kategorien, sieht und ändert ein Client sie nur, wenn er alle diese Kategorien
     sieht.
   - **Anlegen, Verschieben und Umbenennen** gehen nur in Kategorien, die der Client sieht. Beim
     Verschieben braucht er Quelle und Ziel.
   - **Der Server hat das letzte Wort beim Namen:** Nextcloud bereinigt Kategorien. Der Kern prüft
     die Kategorie in der Antwort; liegt sie außerhalb der Auswahl, macht er die Änderung rückgängig
     und antwortet mit `error`. Pfade werden in NFC verglichen.
   - **Rechte folgen der Kategorie:**
     - **Umbenennen über die Suite:** Der Kern schreibt die Auswahlen aller Clients auf den neuen
       Pfad um. Ein Client kann nur innerhalb dessen umbenennen, was er schon sieht (Quelle und Ziel
       gedeckt). Er gewinnt dadurch nichts dazu.
     - **Umbenennen in eine bestehende Kategorie** wird abgelehnt (`conflict`). Sonst sähe ein Client,
       der nur das Ziel hat, plötzlich fremde Notizen.
     - **Wegfall:** Fällt eine Kategorie weg, auch durch Umbenennen direkt in Nextcloud, fällt sie
       aus allen Auswahlen. Eine später neu angelegte Kategorie gleichen Namens bleibt also zu.

   Viele Menschen legen Passwörter in Notizen ab. Darum gilt hier „nichts öffnet sich von selbst“.
5. **Der Assistent darf ohne Rückfrage lesen, suchen und neue Notizen anlegen. Inhalte ändern und
   löschen wartet auf eine Freigabe.** Umgesetzt mit den vorhandenen Mitteln, ohne Sonderregel:
   - Einen **Inhalt** zu ändern überschreibt Text des Menschen. Darum ist das `Delete`, wie
     `workspaces.update_template`.
   - Titel, Favorit und Kategorie zu ändern verliert nichts. Das ist `Write`. Die Stufe hängt also
     vom Request ab (`Request::operation()`, wie bei `load_template`).
   - **Anhängen** (`notes.append_to_note`) fügt Text am Ende an, ohne etwas zu überschreiben. Das ist
     `Write`. So können Agenten mitschreiben, ohne eine Freigabe zu brauchen.
   - **Einen Konflikt lösen** verwirft mit `keep: mine` die Serverfassung und mit `keep: theirs` die
     eigene, oft offline geschriebene Fassung. Beides ist `Delete`. Nur `keep: both` behält alles und
     ist `Write`.
   - `Rights::assistant()` bekommt `notes` mit Stufe Voll und `confirm: OnDelete` (wie bisher). Die
     Kategorienauswahl ist leer. In „What the assistant may use“ wählt der Mensch die Kategorien,
     vorausgewählt sind alle bestehenden.
6. **Offline bearbeiten:** Der Kern hält die Notizen in SQLite, merkt sich je Notiz die letzte
   Serverfassung (Basis) und eine Warteschlange offener Änderungen. Wer wieder online ist,
   schreibt mit `If-Match`. Auf 412 folgt eine Dreiwege-Zusammenführung, Feld für Feld, beim
   Inhalt Zeile für Zeile. Echte Konflikte entscheidet der Mensch, **nie** wird still
   überschrieben.
7. **Stufen:** N1 Kern, Abgleich, API, MCP und App · N2 Bilder und Anhänge · N3 Versionen und
   Papierkorb · N4 Ordner ohne Notes-App. Jede Stufe ist für sich nutzbar und über alle drei
   Wege erreichbar.
8. **Editor:** Markdown bearbeiten mit Umschalter auf die Vorschau, wie in der Notes-App
   (Iced `text_editor` und `markdown`). Kein WYSIWYG in diesem Plan.
9. **Notizen auch für Agenten als Antrieb des Assistenten** (entschieden 2026-10-14): `Area::Notes`
   kommt **nicht** in `NEVER_TO_AGENTS`. Läuft der Assistent mit Claude Code, gehen Notizen, die er
   liest, an dessen Anbieter. Was er sieht, regelt allein die Kategorienauswahl. Wer eine Kategorie
   nicht bei einem Anbieter haben will, lässt sie dort weg. Anders als bei Mail: Das Postfach ist
   als Ganzes Fremdtext von außen, Notizen schreibt der Mensch meist selbst, und die Auswahl ist
   fein genug.

## Entscheidungen (2026-11-09)

Aus Issue [#602](https://github.com/mahype/TorroWorkplace/issues/602), mit der Annahme des Plans bestätigt.
Änderungen an den Entscheidungen 1–9 stehen als Vorschlag mit Grund; sie gelten, wenn der Mensch Konzept und
Mockups im Issue abstimmt.

10. **Mini-Kachel und Leiste haben ein Feld „Quick note“**, als zweite Ausnahme neben dem Assistenten von der Regel
    „Mini und Leiste sind Anzeigen“: Schnelles Erfassen ist ihr Zweck. Enter legt eine Notiz ohne Kategorie an, der
    Titel ist die erste Zeile. Ein Feld, kein Formular, keine Menüs. Verworfen: reine Anzeige mit Knopf. Die
    Design-Regel im Skill `torro-design` kommt mit dem Bau der Kachel (N1.8).
11. **Eine Gestaltungsrichtung nach dem Standard der Suite**, Mockups in
    [prototypes/notes/](prototypes/notes/index.html). Nur der Editor zeigt zwei Formen.
    - **Vorschlag: ändert Entscheidung 8, weil** die Recherche („Vergleich mit bekannten Apps“) eine Live-Vorschau
      empfiehlt: Obsidian, Bear und Nextcloud Text formatieren beim Tippen, ein getrenntes Vorschau-Fenster wirkt
      heute veraltet. Vorschlag: **Live** als Standard. Überschriften, Kästchen, Links und Verweise erscheinen
      formatiert, die Markdown-Zeichen nur leise in der Zeile mit dem Cursor; die Datei bleibt genau das
      geschriebene Markdown (kein WYSIWYG, das normalisiert). Dazu ein Schalter auf den reinen Quelltext
      (`⌘E`/`Ctrl+E`). Mockup: `app.html?open=live` neben `edit`/`preview`.
    - **Machbarkeit offen:** Ob Iceds `text_editor` mit eigenem Hervorheben Größen, leise Zeichen und klickbare
      Kästchen kann, ist nicht geprüft. Geht es nicht, bleibt Entscheidung 8 (Umschalter) als erste Stufe, mit
      Hervorheben im Quelltext, und Live kommt als Ausbau. N1.7 klärt das zuerst.
12. **Torro spricht mit Nextcloud über die Dateien im Notizordner per WebDAV; die Notes-App ist ein Zusatz**, wenn
    sie installiert ist (Ordner und Endung übernehmen, Einstellungen schreiben). Begründung im Abschnitt „Basis“.
    Folgen als Vorschlag:
    - **Vorschlag: ändert Entscheidung 7, weil** der Ordner per WebDAV jetzt der Kern ist: N1 Kern, Abgleich per
      WebDAV, API, MCP und App · N2 Bilder und Anhänge · N3 Versionen und Papierkorb · N4 **lokaler Ordner**
      (ohne Nextcloud). Die bisherige N4 „Ordner ohne Notes-App“ geht in N1 auf.
    - **Vorschlag zur Mindestversion:** Die API 1.2 als Mindestversion entfällt. Ohne Notes-App oder mit einer alten
      läuft Torro trotzdem; nur Ordner und Endung wählt dann der Mensch.
    - **Vorschlag zu Entscheidung 2:** `torro-nextcloud` bekommt `notes.rs` als WebDAV-Anbieter für den
      Notizordner, mit den Einstellungen der Notes-App als Zusatz. Ein Anbieter, nicht zwei.
    - Aufbau, Abgleich, Fehler und Befehle unten tragen je einen Vorschlag dazu.

## Aufbau

```
torronotes (App)  ── IPC ──▶  torroworkplace (Kern)
                               ├─ notes/service.rs   Befehle, Rechte-Filter, Freigabe-Fakten
                               ├─ notes/store.rs     SQLite: Notizen, Basis, Warteschlange, FTS5
                               └─ notes/sync.rs      Abgleich je Konto, Zusammenführen bei 412
                                        │
                     torro_notes::NotesProvider (Trait, Modell, merge)
                        ├─ torro-nextcloud/notes.rs   Notes-API (N1, N2)
                        ├─ torro-nextcloud/…          WebDAV-Versionen, Papierkorb (N3)
                        └─ Ordner-Anbieter            WebDAV oder lokal (N4)
```

**Vorschlag (Entscheidung 12):** Ein Anbieter für Nextcloud statt zweier:

```
torro_notes::NotesProvider
   ├─ torro-nextcloud/notes.rs   Notizordner per WebDAV (N1), Anhänge (N2), Versionen und Papierkorb (N3);
   │                             Ordner und Endung aus der Notes-App, wenn sie da ist
   └─ Ordner-Anbieter            lokaler Ordner (N4)
```

Die `<server-id>` einer Notiz ist dann die Datei-ID (`oc:fileid`). Sie bleibt beim Umbenennen und Verschieben
gleich und ist dieselbe Zahl, die die Notes-App als `id` führt.

**Kennungen:**
- **Notiz:** `<konto-id>/<server-id>`, zum Beispiel `nextcloud:anna@cloud.example/4711`. Eine
  offline angelegte Notiz heißt `<konto-id>/local-<uuid>`, bis der Server sie kennt. Danach führt
  der Kern einen dauerhaften Verweis (in SQLite) von der alten auf die neue Kennung. Ein Client
  oder eine wartende Freigabe mit der alten Kennung findet die Notiz also weiter, auch nach einem
  Neustart.
- **Kategorie:** `<konto-id>/<pfad>`, getrennt am ersten `/`, wie die Sammlungen (M1.1).

**Abgleich:** beim Start, alle 5 Minuten, nach `accounts.sync` und nach jedem Schreiben.
Abgerufen wird in Teilen zu 50 Notizen. Später kann `notify_push` (`notify_file`) einen Abgleich
sofort auslösen. Die Notes-Funktion eines Kontos wird über die Capabilities erkannt. Fehlt sie,
zeigt die Kontoseite „Notes app not installed“, und in N4 übernimmt der Ordner-Anbieter.

**Vorschlag (Entscheidung 12):** Abgleich per PROPFIND statt Notes-API: zuerst das ETag des Notizordners
(`Depth: 0`); nur wenn es sich geändert hat, `Depth: 1` und weiter nur in Unterordner mit geändertem ETag.
`Depth: infinity` nur beim ersten Laden. Gelöschte und verschobene Notizen erkennt der Kern an `oc:fileid`.
Geschrieben wird mit `If-Match` (Datei-ETag), neue Dateien mit `If-None-Match: *`, Umbenennen und Verschieben per
`MOVE` mit `Overwrite: F`; ein 412 darauf heißt „Name belegt“, nicht Konflikt (Abschnitt „Basis“). Favoriten
kommen mit einem eigenen `REPORT` je Abgleich. `notify_push` (`notify_file_id`) löst später einen Abgleich der gemeldeten Dateien aus.
Die Notes-App erkennt der Kern weiter über die Capabilities, aber nur für Ordner und Endung; ohne sie zeigt die
Kontoseite „Notes folder: <ordner>“ statt eines Fehlers.

**Fehler** (feste Codes):

| Fall | Code |
|---|---|
| Server nicht erreichbar | `offline`, Änderungen bleiben in der Warteschlange |
| 401 | `auth`, danach Remote-Wipe-Prüfung wie bei CalDAV |
| 412 ohne automatische Lösung | `conflict` |
| `readonly` | `unsupported` mit „This note is shared read-only.“ |
| 423 | `busy` |
| 507 | `error` mit „Nextcloud storage is full.“ |
| Notes fehlt und kein Ordner | `not-connected` |

Ungültige Eingaben (`..`, zu große Dateien, ungültige Kategorie) beantwortet der Kern mit `error`.

**Vorschlag (Entscheidung 12) zu den Fehlern:** `readonly` kommt aus `oc:permissions` (ohne `W`). „Notes fehlt und
kein Ordner“ wird zu „kein Notizordner gewählt“ (`not-connected`); eine fehlende Notes-App ist kein Fehler mehr.
Ein Name, den der Server verbietet (Capabilities `files.forbidden_*`), ist `error` mit „This title is not allowed
in Nextcloud.“

**Sicherheit:**
- **Kein Inhalt im Protokoll:** Protokolliert werden Befehl und Konto. Notiz- und
  Kategorie-Kennungen, Anhangspfade, Anhangsnamen und Ordnerpfade stehen dort nur als Konto plus
  Prüfsumme (`audit::ids`), Suchbegriffe, Titel und Texte nie. Datenbank und Anhang-Cache sind
  0600 und verschwinden mit dem Konto oder bei einem Remote Wipe.
- **Freigaben** zeigen, was wirklich geschieht, aus dem Cache des Kerns, nachgeschlagen über die
  Kennung (`approvals::Facts`), nie aus dem Text des Clients: Titel, Kategorie, „alt → neu“ bei
  Umbenennen und Pfaden, welche Fassung bei einem Konflikt wegfällt, die Zeit einer Version.
- **Vorschau ohne Netz:** Die Markdown-Vorschau lädt keine Bilder aus dem Netz, weil sie den
  Leser verfolgen könnten, wie bei Mail. Links öffnen sich nur nach einem Klick.
- **Anhänge:**
  - **Erlaubte Pfade:** nur solche, auf die der Inhalt der Notiz verweist (`![](…)`), oder solche
    unter `.attachments.<id>/`. Ein Pfad muss außerdem in einer Kategorie liegen, die der Client
    sieht. `..`, absolute Pfade und URLs werden abgelehnt.
  - **Name beim Hochladen:** ein reiner Dateiname, ohne `/` und `\`, nicht `.` oder `..`, nicht auf
    `.md` oder `.txt` endend. Sonst entstünde nebenbei eine Notiz. Nach dem Nachprüfen vom 2026-11-09 gilt das für
    alle Endungen, die die Notes-App als Notiz zählt (`txt`, `md`, `markdown`, `org`, `note` und die eigene).
- **Ordner und Notizpfad:** `notes.update_settings` und `notes.connect_folder` lehnen das
  Home-Verzeichnis selbst, die Wurzel der Cloud (leerer Notizpfad) und die Verzeichnisse von Torro
  ab. Nach einer Pfadänderung verwirft der Kern die Kategorie-Auswahlen dieses Kontos.

## Befehle (Bereich `notes`)

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| N1 | `notes.list_notes { account?, category?, favorites?, tag?, query?, limit? }` (ohne Inhalt, mit Auszug; `tag?` Vorschlag) | Read | `notes_list_notes` |
| N1 | `notes.get_note { id }` | Read | `notes_get_note` |
| N1 | `notes.list_tags { account? }` (mit Zahlen; Vorschlag, Abschnitt „Inhaltsarten“) | Read | `notes_list_tags` |
| N1 | `notes.list_categories { account? }` (mit Zahlen) | Read | `notes_list_categories` |
| N1 | `notes.create_note { account?, title?, content, category?, favorite? }` | Write | `notes_create_note` |
| N1 | `notes.update_note { id, etag, title?, content?, category?, favorite? }` | Write, mit `content` **Delete** | `notes_update_note` |
| N1 | `notes.append_to_note { id, text }` | Write | `notes_append_to_note` |
| N1 | `notes.delete_note { id, etag }` | Delete | `notes_delete_note` |
| N1 | `notes.rename_category { account, from, to }` | Write | `notes_rename_category` |
| N1 | `notes.resolve_conflict { id, keep: mine\|theirs\|both, etag }` | Write, mit `mine`/`theirs` **Delete** | `notes_resolve_conflict` |
| N1 | `notes.get_settings { account }` | Read | `notes_get_settings` |
| N1 | `notes.update_settings { account, notes_path?, file_suffix? }` | Admin | `notes_update_settings` |
| N1 | `inbox.create_note_from_item { id, category }` (kommt in N1.10) | Write (`inbox`) + Write in `notes` | `inbox_create_note_from_item` |
| N2 | `notes.get_attachment { id, path }` | Read | `notes_get_attachment` |
| N2 | `notes.add_attachment { id, name, data }` | Write | `notes_add_attachment` |
| N3 | `notes.list_versions { id }` | Read | `notes_list_versions` |
| N3 | `notes.get_version { id, version }` (Inhalt einer Version) | Read | `notes_get_version` |
| N3 | `notes.restore_version { id, version }` | Delete | `notes_restore_version` |
| N3 | `notes.list_trash { account? }` | Read | `notes_list_trash` |
| N3 | `notes.restore_note { trash_id }` | Write | `notes_restore_note` |
| N4 | `notes.connect_folder { location }` | Admin | `notes_connect_folder` |
| N4 | `notes.remove_folder { id }` | Admin | `notes_remove_folder` |

Erläuterungen zur Tabelle:
- **Kein Befehl steht auf `Never`.** Nichts davon fällt unter
  [„Was nie nach außen geht“](api.md#was-nie-nach-außen-geht). Admin-Befehle warten für Agenten wie
  immer auf eine Freigabe.
- **`resolve_conflict`** braucht das `etag` der angezeigten Serverfassung. Ist der Server
  inzwischen weiter, antwortet der Kern mit `conflict`, statt eine Fassung zu verwerfen, die
  niemand gesehen hat.
- **Zustand des Notes-Dienstes** (verbunden, App fehlt, App zu alt) steht als neues optionales Feld
  in der Antwort von `accounts.list_accounts`; dafür gibt es keinen eigenen Befehl.
- **Tags (Vorschlag):** `notes.list_tags` liefert die Tags aus dem Index des Kerns samt Zahl, nur aus Notizen,
  die der Client sieht; `notes.list_notes` bekommt dazu den Filter `tag?` (ein Tag deckt seine Unterstufen,
  `#a` auch `#a/b`). Tags sind Inhalt, also `untrusted: true`.
- **Vorschlag (Entscheidung 12) zum Zustand:** Das Feld heißt dann Zustand des Notizordners: „Ordner gewählt“,
  „Ordner aus der Notes-App“, „kein Ordner“ (`not-connected`). Eine fehlende oder alte Notes-App ist kein Fehler.
- **`connect_folder`:** Die Freigabe zeigt den echten Pfad. Sonst könnte ein Agent beliebige
  Ordner wie `~/.ssh` als Notizen einbinden.
- **`restore_note`** stellt eine Notiz aus dem Papierkorb wieder her. Wer sie dort endgültig
  löschen will, tut das in Nextcloud. Das macht Nextcloud ohnehin nach Ablauf der Frist,
  `torronotes` bietet es nicht an.

**Vorschlag (Entscheidung 12) zu den Befehlen:** Die Tabelle bleibt bis auf zwei Punkte:
- `notes.get_settings` liefert auch, woher Ordner und Endung kommen (Notes-App oder Torro);
  `notes.update_settings` schreibt mit Notes-App über deren API, sonst nur in die Einstellungen des Kerns.
- `notes.connect_folder` und `notes.remove_folder` bleiben, aber nur noch für **lokale** Ordner (neue Stufe N4).
  Einen Nextcloud-Ordner wählt man über `update_settings` am Konto.

Neue Befehle aus „Zulieferer“ (`send.route` als Ziel, „New meeting note“) kommen mit dem Plan ihrer Quelle; sie
nutzen `notes.create_note` und `notes.append_to_note`.

## Oberfläche

Mockups in allen Größen, hell und dunkel: [prototypes/notes/](prototypes/notes/index.html) (Entscheidung 11). Sie
sind der Maßstab für N1.6–N1.8 und N1.10.

**`torronotes`** nutzt `torro_gui::shell`:
- **Seitenleiste:** **All notes**, **Favorites**, darunter die Kategorien als Baum, ab zwei Konten
  gruppiert je Konto (Muster aus [mail.md](mail.md)); darunter **Tags** aus dem Index. Unten der Stand des
  Abgleichs („Synced 2 min ago“, offline mit Zahl wartender Änderungen).
- **Liste:** Titel, Auszug, Datum, Stern, Kennzeichen für schreibgeschützt, offline geändert, Konflikt und „von
  einem Agenten angelegt“; Tagesköpfe.
- **Rechts:** die Notiz in einer Lesespalte (640), Titel, darunter Kategorie und Konto als Chips. Der Editor nach
  Entscheidung 8 bzw. dem Vorschlag in Entscheidung 11 (Live, Schalter auf den Quelltext). Hinweisband oben bei
  Offline-Änderungen, Konflikt (beide Fassungen nebeneinander, „Keep mine / Keep theirs / Keep both“) und
  schreibgeschützten Notizen („Copy to my notes“).
- **Größen:** Breit Seitenleiste 240, Liste 440, Notiz · Mittel Leiste 60, Liste 364, Notiz (Kopf nur mit dem
  Titel, die Kategorie steht als Chip darunter) · Schmal Liste oder
  Notiz · **Mini** Feld „Quick note“ und die letzten fünf · **Leiste** Feld und die letzten Titel auf einer
  Mittellinie (Entscheidung 10).
- **„Save as note“:** ein gemeinsames Blatt für alle Apps (Titel, Kategorie mit Vorschlägen, Konto, Vorschau; der
  Hinweis, dass der Text von außen kommt). Es kann später im „Send to…“ aus
  [#260](https://github.com/mahype/TorroWorkplace/issues/260) aufgehen. Die Einstiege in den anderen Apps sind
  nicht gezeichnet.
- **Steuerzentrale:** Kontoseite mit „Notes“-Dienst, Rechte-Zeile `notes` mit Kategorienauswahl
  (Baum, vererbt).
- **Eingang:** Aktion „Save as note“.

## Offen (in der jeweiligen Aufgabe klären)

- ~~**Hängt eine Notiz an einer Datei?**~~ Geklärt 2026-11-09: Die Notiz-`id` ist die Datei-ID (Abschnitt
  „Nachgeprüft“).
- ~~**Anhänge löschen?**~~ Geklärt 2026-11-09: ab Notes 6.1 `DELETE /attachment/{id}`, per WebDAV ohnehin. Ob
  `torronotes` Anhänge löscht, entscheidet N2.
- **Live-Editor in Iced** (Entscheidung 11): machbar mit `text_editor`? Klärt N1.7 zuerst.
- **Wann räumt Nextcloud Text Anhänge auf** (`cleanupAttachments`)? Gegen einen echten Server prüfen (N1.12).
- **Teilen und Vorlagen** sind spätere Pläne. Vorlagen gingen über die Vorlagen-API von Files.
- **Backlinks, gespeicherte Suchen, Links nachziehen beim Umbenennen, globale Taste** (Abschnitt „Vergleich“):
  spätere Aufgaben.
