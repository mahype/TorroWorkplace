# Notizen: App `torronotes` auf Nextcloud Notes

Stand: 2026-10-14. **Konzept, Entscheidungen vom 2026-10-14 eingearbeitet.** Aufgaben:
[tasks/notizen.md](tasks/notizen.md). Beantwortet die Idee „Notizen / Scratchpad“ aus dem
[App-Katalog](app-catalog.md): Wo lege ich schnell etwas ab, auch für Agenten?

## Ziel

Eine Notizen-App der Suite, die auf **Nextcloud Notes** aufbaut. Sie funktioniert offline und wird
wie jede Funktion der Suite dreimal angeboten: in der Oberfläche (`torronotes`), über die API
(Bereich `notes`) und über MCP (`notes_*`), also auch für den Assistenten.

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
    `.md` oder `.txt` endend. Sonst entstünde nebenbei eine Notiz.
- **Ordner und Notizpfad:** `notes.update_settings` und `notes.connect_folder` lehnen das
  Home-Verzeichnis selbst, die Wurzel der Cloud (leerer Notizpfad) und die Verzeichnisse von Torro
  ab. Nach einer Pfadänderung verwirft der Kern die Kategorie-Auswahlen dieses Kontos.

## Befehle (Bereich `notes`)

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| N1 | `notes.list_notes { account?, category?, favorites?, query?, limit? }` (ohne Inhalt, mit Auszug) | Read | `notes_list_notes` |
| N1 | `notes.get_note { id }` | Read | `notes_get_note` |
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
- **`connect_folder`:** Die Freigabe zeigt den echten Pfad. Sonst könnte ein Agent beliebige
  Ordner wie `~/.ssh` als Notizen einbinden.
- **`restore_note`** stellt eine Notiz aus dem Papierkorb wieder her. Wer sie dort endgültig
  löschen will, tut das in Nextcloud. Das macht Nextcloud ohnehin nach Ablauf der Frist,
  `torronotes` bietet es nicht an.

## Oberfläche

**`torronotes`** nutzt `torro_gui::shell`:
- **Seitenleiste:** **All notes**, **Favorites**, darunter die Kategorien als Baum, ab zwei Konten
  gruppiert je Konto (Muster aus [mail.md](mail.md)).
- **Liste:** Titel, Auszug, Datum, Stern.
- **Rechts:** der Editor mit Umschalter „Edit / Preview“. Oben stehen Titel und Kategorie, unten
  ein Hinweis bei Offline-Änderungen oder einem Konflikt. Schmal zeigt das Fenster entweder die
  Liste oder die Notiz.
- **Mini-Kachel:** „Quick note“ mit Eingabefeld und den letzten Notizen. Das ist der Scratchpad
  aus dem Katalog.
- **Steuerzentrale:** Kontoseite mit „Notes“-Dienst, Rechte-Zeile `notes` mit Kategorienauswahl
  (Baum, vererbt).
- **Eingang:** Aktion „Save as note“.

## Offen (in der jeweiligen Aufgabe klären)

- **Hängt eine Notiz an einer Datei?** Die Notiz-`id` ist vermutlich die Datei-ID von Nextcloud.
  Das wäre nützlich für Versionen in N3. Gegen einen echten Server prüfen.
- **Anhänge löschen?** Notes 6.1 nennt im Changelog einen Endpunkt zum Löschen
  von Anhängen, die API-Doku nicht. Bis das geklärt ist, löschen wir keine Anhänge.
- **Teilen und Vorlagen** sind spätere Pläne.
