# Aufgaben: Dokumente mit ArtCraft (AC.1–AC.7)

Konzept: [erweiterungen.md](../erweiterungen.md) (Abschnitt „MCP-Server (#302)“, ab AC.7 auch „Dokumente mit ArtCraft
(#303)“), [api.md](../api.md) („Was nie nach außen geht“, Punkte 9 und 10), [assistent.md](../assistent.md)
(„Eigene Werkzeuge“, freigegebene Ordner #304). Issue #303, geplant am 2026-10-26. Regeln: [CLAUDE.md](../../CLAUDE.md),
Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen Request anfasst, lädt `torro-api`;
nach jeder Funktion prüft `torro-api-review`.

```
AC.1 (Gerüst) ── AC.2 (Katalog: vier Einträge, Format) ── AC.3 (Ordner je App, Grenzen beim Start) ── AC.4 (Pfadprüfung, Speichern als Kopie, Sperrliste) ── AC.5 (Oberfläche) ── AC.6 (Einrichtung: Schritt „Documents“) ── AC.7 (Doku)
```

Reihenfolge in einem Strang: AC.1, AC.2, AC.3, AC.4, AC.5, AC.6, AC.7.

**Festlegungen für alle AC-Aufgaben** (Stand der Quellen am 2026-10-26: WordCraft v0.11.0, DeckCraft v0.11.1, GridCraft
v0.12.1, PdfCraft v0.14.0, je `MIT OR Apache-2.0`, `unsafe_code = "forbid"`):

- **Vier Katalog-Einträge**, jede App ein eigener Server mit eigener Stufe je Server (`Rights::extensions`, gibt es
  seit #302): `wordcraft`, `deckcraft`, `gridcraft`, `pdfcraft`. Befehl `<app>-cli`, Argumente `["mcp"]`, bei PdfCraft
  ohne `--compact` (nach der Freigabe der ersten Fassung entschieden: alle Werkzeuge einzeln eingestuft, damit
  Kommentare, Lesezeichen und Formularfelder ohne Freigabe gehen; `--compact` hätte alles außer 15 Werkzeugen über
  `tool_call` geschickt, also mit Freigabe). Nie `--connect` (steuert die laufende App ohne Grenzen), nie `--root` bei PdfCraft (ältere
  Grenze; die Grenzen kommen über die beiden `--automation-*`-Optionen). Link: die Releases der App
  (`https://github.com/storytold/<app>/releases`), `experimental = true`.
- **Ein Ordner je App** (in der Planung entschieden): WordCraft, DeckCraft und PdfCraft nehmen je
  `--automation-read-root` und `--automation-write-root` genau einen Ordner (ein zweiter ist ein Fehler). Die Person
  wählt darum auf der Seite der App einen der freigegebenen Ordner (#304). Torro startet den Server mit diesem Ordner
  als Lesegrenze und, wenn der Ordner „Lesen und Schreiben“ hat, auch als Schreibgrenze. Hat er nur „Lesen“, wird der
  eigene Arbeitsordner `<data>/extensions/<id>/work` die Schreibgrenze (laut Quellen sperrt schon eine fehlende
  Schreibgrenze das Schreiben, bei DeckCraft ist das nicht belegt). Ohne gewählten oder ohne gültigen Ordner (entfernt,
  nicht eingehängt, nicht mehr wählbar) ist der Arbeitsordner beide Grenzen, nie keine: Ohne jede Grenze schreibt
  ArtCraft überall. Jeder Aufruf wird dann abgelehnt. Die Grenzen sind Startoptionen, keine MCP-Roots (Abweichung von
  „Roots kommen mit #304“ in [erweiterungen.md](../erweiterungen.md)): ArtCraft meldet keine Roots-Fähigkeit und
  ignoriert `roots/list_changed`. Entfernen eines Servers nimmt seinen Ordner mit; neu installiert hat er keinen.
- **Wählbar** ist ein freigegebener Ordner, der nichts Gesperrtes enthält (Sperrliste von #304: Schlüsselspeicher,
  Ordner der Suite, Schlüsseldateien der Clients) und, bei „Lesen und Schreiben“, keinen inneren Ordner mit „Lesen“.
  Grund: ArtCraft kennt die Sperrliste nicht, und seine Allzweck-Befehle erreichen jeden Pfad unter der Grenze. Geprüft
  beim Wählen, bei jedem Start und bei jedem Aufruf (ein später angelegter innerer Lese-Ordner zählt sofort). Offen und
  so dokumentiert: Sperren nach Namen (`BACKUP_SUFFIX`, `.Trash-*`) im Ordner findet die Prüfung nicht; symbolische
  Links im Ordner löst ArtCraft beim Prüfen seiner Grenze auf (`fs::canonicalize`, laut Quellen bei allen drei).
- **Torro prüft die Pfade selbst**, bei jeder App, vor der Freigabe und beim Ausführen: Argumente, die der Katalog als
  gelesene Pfade oder als Ziel nennt, und in beliebiger Tiefe jeder Wert unter den Schlüsseln `path`, `paths`,
  `out`, `out_dir`, `folder`, `file`, `image`, `directories`. Jeder solche Pfad ist absolut, liegt im **gewählten**
  Ordner (nicht bloß in irgendeinem freigegebenen) und ist nicht gesperrt (`tools::folders::Allowed`). Bei GridCraft
  ist das die einzige Grenze; darum ist dort `execute_command` gesperrt: Es nimmt Pfade unter Schlüsseln, die Torro
  nicht kennt, und relativ zum Arbeitsordner des Prozesses (in der Planung entschieden, Alternative „Delete mit
  Freigabe“ verworfen).
- **Speichern nur als neue Datei:** Ein Ziel (`target` im Katalog) darf es nicht geben, auch nicht als symbolischen
  Link (`conflict`). Wo ein fehlendes Ziel „über das Original“ heißt (`save_document`, `save_presentation`,
  `save_workbook`, `doc_save`), ist es Pflicht (`error`). Werkzeuge mit Ausgaben, deren Namen Torro nicht kennt
  (mehrere Dateien), sind `delete`. Allzweck-Befehle können trotzdem überschreiben; sie sind `delete`, ihre Freigabe
  zeigt die Argumente.
- **Gesperrt** (nie angeboten, nie zustimmbar, und als Wert unter `name`, `id`, `command` oder `method` in den
  Argumenten eines Allzweck-Befehls abgelehnt, ohne Rücksicht auf Groß- und Kleinschreibung): WordCraft `screenshot`,
  `click`, `key`, `ui_inspect`, Befehle `ui.*`; DeckCraft `screenshot`, `key`, `pointer`, `call_app`, Befehle `ui.*`;
  GridCraft `screenshot` (schreibt ein PNG an jeden Pfad), `execute_command`; PdfCraft `sign_trust`,
  `sign_id_create`, `js_run`, dazu `sign_document`, `sign_keychain_ids`, `sign_windows_ids` (sie greifen auf die
  Schlüssel im Schlüsselbund bzw. Zertifikatsspeicher des Systems zu, Agenten verwalten nie Schlüssel) und die Befehle
  `tools.js_console` und `sign.digital` (dahin führt `command_run`). Werkzeuge,
  die der Katalog nicht nennt, bleiben `delete` wie seit #302.
- **Den Ordner sehen** der App-Schlüssel und jeder Client mit einer Stufe für diesen Server
  (`ExtensionInfo::folder`): Ein Agent braucht ihn, um absolute Pfade darin zu bilden; er ist eine Grenze, kein
  Geheimnis. Clients ohne Stufe sehen den Server gar nicht (`extension_unseen`).
- **Fremde Version:** Die Person installiert ArtCraft selbst; Torro legt keine Version fest. `version` im Katalog ist
  die geprüfte Version („tested with“), die Regel „eine Argument nennt die Version“ gilt nur für `npx`, `uvx`, `docker`.
- **Wo `<app>-cli` liegt:** Linux deb/rpm `/usr/bin` (auf dem `PATH`); macOS nur im eigenen Zip
  `<app>-cli-<version>-macos-universal.zip`, nicht in der `.app` (die Beschreibung sagt: nach `/usr/local/bin` oder
  `~/.local/bin`, beides sucht `prereq` schon); Windows `%ProgramFiles%\<App>\<app>-cli.exe`, nicht auf dem `PATH`
  (neues Katalogfeld `look_in`).
- Tests brauchen kein ArtCraft, kein Netz: ein POSIX-Skript `tests/fixtures/extensions/artcraft.sh` bildet die
  Werkzeugnamen nach, meldet seine Startargumente (Werkzeug `args`) und schreibt jeden Aufruf nach `./calls.log` im
  Arbeitsordner (`#[cfg(unix)]`). Ordner in Temp-Verzeichnissen.

### AC.1 Gerüst: Ordner je Server, Typen, Request, Katalogeintrag
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-ipc/src/{lib,api,extensions}.rs`,
  `crates/torroworkplace/src/{core,mcp,audit,approvals,daemon,pim}.rs` (nur `match`-Arme, Handler, Tests; Logzeile in
  `daemon.rs` ohne Pfad, etwa „choose folder of mcp server {id}“),
  `crates/torroworkplace/src/extensions/{mod,store}.rs`, `crates/torroworkplace/src/ui/*.rs` und
  `crates/torro-settings-ui/src/*.rs` (nur `match`-Arme und Testdaten), `docs/api.md`, `docs/api-reference.md`
- **Oberfläche:** Seite eines Servers in MCP servers (AC.5) · **API:** `Request::SetExtensionFolder { id, folder:
  Option<String> }` · `extensions.set_folder` · Zugriff Admin · **MCP:** Never (Punkt 10 in „Was nie nach außen geht“:
  einen Ordner vergeben heißt Rechte vergeben, wie `allowed_folder`). Antwort `Response::Extension`.
- `torro_ipc::extensions`: `FolderCheck` (`both`: Server und Torro prüfen, `torro_only`: nur Torro, GridCraft),
  `CatalogEntry::folder: Option<FolderCheck>`, `ExtensionInfo::folder: Option<String>` (der gewählte Pfad, für den
  App-Schlüssel und Clients mit einer Stufe für den Server), `ToolState::Blocked` (vom Katalog gesperrt, nie angeboten, nie zustimmbar); alle
  neuen Felder `#[serde(default, skip_serializing_if = …)]`.
- `store::StoredExtension::folder: Option<String>` (`#[serde(default)]`, alte Dateien lesen sich weiter).
- Handler vorerst `unsupported`; `extensions_request` gibt jedem Client `forbidden` (wie `InstallExtension`).
- `docs/api.md` Punkt 10 um `extensions.set_folder` ergänzen.
- **Abnahme:** `samples()` deckt den neuen Request; `names_follow_the_rules` mit ihm in `kept_back`; Test: jeder Client
  bekommt `forbidden`, auch mit Admin; eine `extensions.json` ohne `folder` liest sich; `TORRO_BLESS=1 cargo test -p
  torro-ipc` und `docs/api-reference.md`.

### AC.2 Katalog: Format und die vier Einträge
- [x] erledigt
- **Modell:** Opus · **Braucht:** AC.1 · **Dateien:** `crates/torroworkplace/assets/extensions/catalog.toml`,
  `crates/torroworkplace/src/extensions/{catalog,prereq,mod}.rs`
- Neue Felder in `catalog.toml` (`deny_unknown_fields` bleibt): je Server `[server.folder]` mit `read_root` und
  `write_root` (Name der Option, beide oder keine; leere Tabelle = `torro_only`), `look_in` (Ordner, Platzhalter nur
  `{ProgramFiles}`, fällt weg, wo die Variable fehlt), `blocked` (Werkzeugnamen), `blocked_commands` (Befehls-Kennungen
  der Allzweck-Befehle; ein Eintrag mit `.*` am Ende gilt für alle mit diesem Anfang, `ui.*`); je Werkzeug `reads` (Argumentnamen), `target` (ein Argumentname), `target_required` (bool).
  `catalog::Entry` trägt sie, `CatalogEntry::folder` wird daraus gefüllt.
- Regeln in `check`: Versionsregel nur für `npx`, `uvx`, `docker`; kein Argument `--connect` oder `--root` bei einem
  Eintrag mit `folder`; `reads`/`target` nur bei Einträgen mit `folder`; ein Werkzeug steht nicht zugleich in `tools`
  und `blocked`; `target_required` nur mit `target`.
- `prereq::find`/`missing` sehen auch in `look_in` nach (`Extensions::catalog` und der Start eines Katalog-Servers).
- Die vier Einträge mit Kopfkommentar (Quellen, Datum, Pfade der CLI) nach den Festlegungen. Zugriff:
  - **WordCraft:** `read`: `list_commands`, `get_text`, `inspect_document`, `render_page`, `list_documents`, `parity`,
    `open_document` (`reads = ["path"]`); `write`: `new_document`, `type_text`, `select_text`, `save_document`
    (`target = "path"`, Pflicht); `delete`: `execute`, `batch`.
  - **DeckCraft:** `read`: `list_commands`, `inspect_document`, `inspect_slide`, `render_slide`, `go_to_slide`,
    `open_presentation` (`reads`); `write`: `new_presentation`, `add_slide`, `add_shape`, `add_text_box`, `set_text`,
    `insert_picture` (`reads`), `insert_table`, `insert_chart`, `select`, `select_tool`, `type_text`,
    `save_presentation` (`target`, Pflicht); `delete`: `export` (mehrere Dateien bei `all`), `run_command`, `batch`.
  - **GridCraft:** `read`: `list_windows`, `list_commands`, `inspect_workbook`, `read_range`, `get_cell`,
    `evaluate_formula`, `list_functions`, `open_workbook` (`reads`); `write`: `write_range`, `set_cell`, `select`,
    `format_range`, `insert_chart`, `create_table`, `sort_range`, `filter`, `new_workbook`, `undo`, `redo`; `delete`:
    `save_workbook` (`target`, Pflicht: jedes Schreiben einer Datei mit Freigabe); gesperrt `screenshot`,
    `execute_command`.
  - **PdfCraft** (ohne `--compact`, 181 Werkzeuge in `crates/mcp/src/tools.rs` von v0.14.0; vor dem Eintragen dort
    nachzählen). Regel: Änderungen am geöffneten Dokument sind `write`, auch löschende (`page_delete`,
    `redact_apply`, `doc_flatten` …): Sie wirken erst mit dem Speichern, und das Original bleibt, weil Speichern nur
    eine neue Datei anlegt. `delete` ist, was mehrere oder unbekannt benannte Dateien schreibt, druckt, Skripte in ein
    Dokument legt oder alles erreicht.
    - `read` (50): `doc_open`, `doc_list`, `doc_info`, `doc_close`, `page_render`, `object_list`, `text_word_count`,
      `text_extract`, `text_find`, `bookmark_list`, `form_fields`, `doc_hidden_info`, `doc_audit_redactions`,
      `printers`, `printer_options`, `content_list`, `link_list`, `link_open`, `measure_info`, `measure_list`,
      `measure_snap`, `comment_list`, `sign_list`, `comment_image_preview`, `accessibility_autotag_preview`,
      `accessibility_check`, `accessibility_find_untagged`, `accessibility_review_load`, `accessibility_figures`,
      `accessibility_tags`, `accessibility_tag_content`, `js_document_scripts`, `pdfa_verify`, `print_readiness`,
      `action_list`, `doc_compare`, `form_actions`, `ocr_status`, `doc_audit_space`, `text_lines`, `image_hit`,
      `page_images`, `text_font_files`, `text_fonts`, `text_paragraphs`, `text_edit_layout`, `command_list`,
      `doc_inspect`, `render_preview`, `tool_search` (`tool_search` und `tool_call` nur, falls gemeldet).
    - `delete` (16): `doc_batch`, `page_extract` (schreibt mit `separate` einen Ordner), `doc_create_multiple`,
      `doc_split`, `doc_export_images`, `doc_export_all_images`, `ocr_recognize_files` (ersetzt gleichnamige Dateien),
      `action_run`, `doc_print` (echter Druckauftrag über `lp`), `js_set_document_script`, `form_set_script`,
      `form_set_actions`, `js_enabled`, `command_run`, `command_batch`, `tool_call`.
    - `write`: die übrigen 109 (181 = 50 + 16 + 109 + 6 gesperrte). Ziele (`target`): `doc_save`,
      `doc_export_data`, `doc_export_office`, `doc_export_text`, `doc_reduce`, `doc_optimize`, `image_save`,
      `doc_compare_report`, `accessibility_report`, `accessibility_review_record`, `accessibility_review_html`,
      `accessibility_review_save`, `form_merge_data` je `path`; `measure_export` `out`; Pflicht überall außer bei
      `doc_combine` und `comments_summarize` (`target = "out"`, ohne `out` schreiben sie nichts).
    - `reads` nach der Spalte „R:“ der Quellen: `path` bei `doc_open`, `attachment_add`, `page_insert_file`,
      `stamp_custom`, `form_set_image`, `doc_import_data`, `page_add_image`, `comment_add`, `page_replace`,
      `accessibility_review_load`, `fill_sign_add`, `image_edit`, `text_font_import`; `paths` bei `doc_combine`,
      `form_merge_data`, `doc_create` (dazu `path`); `file` bei `doc_watermark`, `doc_background`; `image` bei
      `content_update`; `directories` bei `text_font_files`. Ziele und gelesene Pfade der `delete`-Werkzeuge deckt die
      Prüfung der Pfad-Schlüssel in beliebiger Tiefe ab.
- **Abnahme:** `the_built_in_catalog_is_valid` nennt sieben Einträge und prüft je ArtCraft-Eintrag Befehl, Argumente,
  `folder`, die Sperrliste und die Pflicht-Ziele; Tests für jede neue Regel (wie `broken_entries_are_refused`);
  `look_in` mit `{ProgramFiles}` in einem Temp-Ordner gefunden; Einträge ohne die neuen Felder lesen sich wie bisher.

### AC.3 Ordner je App und Grenzen beim Start
- [x] erledigt
- **Modell:** Opus · **Braucht:** AC.2 · **Dateien:** `crates/torroworkplace/src/extensions/{mod,supervisor}.rs`,
  `crates/torroworkplace/src/tools/{folders,mod}.rs`, `crates/torroworkplace/src/core.rs`,
  `crates/torroworkplace/tests/fixtures/extensions/artcraft.sh` (neu), `crates/torro-core/src/i18n/de.rs`
- `Extensions::with_allowed(Arc<dyn Fn() -> Allowed + Send + Sync>)`: liefert die freigegebenen Ordner von jetzt mit
  der Sperrliste (`Allowed::of(&settings.tools.folders, …)` mit `Roots::excluded` der Werkzeuge, dafür neu
  `Tools::excluded()`); gesetzt beim Bau des Kerns und in
  `Core::with_extensions`. Ohne ihn: keine Ordner.
- `Allowed::holds_lock(&Path) -> bool` (nur Gesperrtes, nicht die freigegebenen Ordner) und eine Hilfe „hat einen
  inneren Ordner mit Lesen“. Daraus `Extensions::usable_folder(stored) -> Option<(PathBuf, FolderAccess)>`: der
  gewählte Ordner, aufgelöst unter `Allowed::roots()`, wählbar nach den Festlegungen; ausgewertet beim Start und in
  AC.4 bei jedem Aufruf.
- `Launch::bounds: Vec<String>` (Argumente der Grenzen; `PartialEq` zählt sie, ein anderer Ordner startet den Server
  also neu), angehängt in `Process::spawn` nach den Argumenten. `Supervisor::running` meldet weiter nur `server`,
  damit `keep_in` die Werkzeuglisten behält. Jede Stelle, die einen `Launch` baut (`install`, `update`, `launch`),
  füllt `bounds` über eine Hilfe: Eintrag mit `read_root`/`write_root` → Lesegrenze gewählter Ordner oder
  Arbeitsordner, Schreibgrenze gewählter Ordner nur bei „Lesen und Schreiben“, sonst Arbeitsordner; ohne `folder`
  (Time, GitHub, eigene Server) und GridCraft → leer. `launch_for_call` vergleicht unter `changing` auch `folder`, damit
  ein Wechsel während eines Aufrufs nicht mit den alten Grenzen läuft.
- `Extensions::set_folder(id, folder)` für `Request::SetExtensionFolder`: `not-found` für unbekannte Kennung und für
  einen Pfad, der kein freigegebener Ordner ist; `unsupported` für einen Server ohne `folder`; `forbidden` mit
  Grund für einen nicht wählbaren Ordner; `None` nimmt die Wahl zurück. Speichert unter `changing`, antwortet
  `Response::Extension`. `info` füllt `ExtensionInfo::folder` für den App-Schlüssel und Clients mit Stufe.
- Wird ein freigegebener Ordner entfernt oder auf „Lesen“ gesetzt, gilt das beim nächsten Start; ein laufender Server
  mit anderen `bounds` wird beim nächsten Aufruf neu gestartet (Verhalten des Supervisors, nachgesehen).
- **Abnahme:** Prozess-Tests mit `artcraft.sh` als gespeicherter Katalog-Server `wordcraft` (Spec zeigt auf das
  Skript): Ordner mit „Lesen und Schreiben“ → beide Optionen mit diesem Ordner; „Lesen“ → Lesegrenze der Ordner,
  Schreibgrenze der Arbeitsordner; kein Ordner → Arbeitsordner für beide; Ordner geändert → neuer Prozess mit neuen Argumenten; Time ohne Zusatz. Handler:
  Ordner mit gesperrtem Inhalt und RW-Ordner mit innerem Lese-Ordner `forbidden`, fremder Pfad `not-found`, Client
  `forbidden`.

### AC.4 Pfadprüfung, Speichern als Kopie, Sperrliste
- [x] erledigt
- **Modell:** Opus · **Braucht:** AC.3 · **Dateien:** `crates/torroworkplace/src/extensions/{files,mod,client}.rs`
  (`files.rs` neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/tests/fixtures/extensions/artcraft.sh`,
  `crates/torro-core/src/i18n/de.rs`
- `extensions::files::refusal(entry, folder, allowed, tool, arguments) -> Option<Response>`, in dieser Reihenfolge:
  gesperrtes Werkzeug → wie `no_tool`; gesperrter Name unter `name`/`id`/`command`/`method` in beliebiger Tiefe →
  `forbidden`; kein nutzbarer Ordner (`usable_folder` jetzt) → `forbidden` („Choose a folder for <App> …“); `reads` → vorhanden, im Ordner,
  nicht gesperrt (`Allowed::existing`); `target` → Pflicht fehlt `error`, sonst `Allowed::new_target` mit
  `ReadWrite`, im Ordner, `symlink_metadata` muss `NotFound` sein, sonst `conflict`; jeder Wert unter den Pfad-Schlüsseln
  der Festlegungen absolut, im gewählten Ordner (beginnt mit dessen aufgelöster Wurzel), nicht gesperrt. Meldungen
  ohne Pfad aus dem Request.
- Aufgerufen in `Extensions::call` vor dem Start und in `Core::extension_refusal` als **letzte** Prüfung, nach
  `extension_unseen` und `allows_extension` (sonst erführe ein Client ohne Stufe etwas über den Server); dort vor der
  Freigabe und bei ihrer Ausführung (`run_approved`) noch einmal. Nur für Katalog-Server mit `folder`.
- `client::tool_states` bekommt die Sperrliste: gesperrte Werkzeuge `ToolState::Blocked`; `confirm` lässt sie aus,
  `offered` und `allowed_now` bieten sie nie an; `notes` nennt sie.
- **Abnahme:** Einheitstests für jede Regel (Temp-Ordner, Link aus dem Ordner heraus, `..`, relativer Pfad, gesperrter
  Pfad innen, Ziel vorhanden, Ziel als Link, Pflicht-Ziel fehlt, Pfad in einem anderen freigegebenen Ordner,
  verschachteltes `commands[].params.path`, `tool_call` mit `name = "JS_RUN"`, `command_run` mit
  `id = "tools.js_console"` und `id = "sign.digital"`, `execute` mit `command = "ui.click"`, innerer Lese-Ordner nach dem Wählen angelegt); Prozess-Test mit `artcraft.sh`:
  ein abgelehnter Aufruf steht nicht in `calls.log`, ein erlaubter schon; gesperrte Werkzeuge erscheinen als `blocked`
  und werden bei `confirm_tools` nicht zugestimmt.

### AC.5 Oberfläche: Ordner, Hinweise, Katalog
- [ ] erledigt
- **Modell:** Opus · **Braucht:** AC.4 · **Dateien:** `crates/torroworkplace/src/ui/{connections,mod}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- Seite eines Servers mit `folder`: Gruppe „Folder“ vor „Tools“: gewählter Ordner (gekürzt mit Tooltip) oder „No folder
  chosen“ mit Attention-Punkt, Knopf „Choose…“ mit Menü der freigegebenen Ordner (mit Stufe) und „None“; sendet
  `Request::SetExtensionFolder`, eine Ablehnung zeigt die Seite als Hinweis. Ohne freigegebene Ordner ein Satz mit Weg
  zu „Allowed folders“. Fußzeile: die App arbeitet nur dort, Torro speichert Änderungen als neue Datei.
- `torro_only` (GridCraft): eine Zeile in Katalog, Installieren-Dialog und Seite, dass die App Ordner nicht selbst
  begrenzt, Torro die Pfade prüft und Speichern auf Freigabe wartet.
- Katalogzeile und Dialoge: für Einträge mit einem `Program` als Voraussetzung „tested with version …“ statt
  „version …“; Werkzeugliste zeigt `blocked` als „Blocked by Torro“.
- Demo: WordCraft (Ordner gewählt) und GridCraft im Katalog und als installierte Server; neue Zustände
  `extension-folder` (WordCraft) und `extension-unbounded` (GridCraft) in `--open`, im Skill `torro-design` nachtragen (AC.7).
- **Abnahme:** Test: „Choose…“ schickt den Request mit Pfad, „None“ mit `null`; Sichtprüfung beider Zustände in allen
  Größenklassen und in Standard, Compact, Console (Agent `torro-design-review`); Übersetzungen.

### AC.6 Einrichtung: Schritt „Documents“
- [ ] erledigt
- **Modell:** Opus · **Braucht:** AC.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/{mod,pages}.rs`,
  `crates/torroworkplace/src/ui/mod.rs` (Katalog in der Einrichtung laden, Demo), `crates/torro-core/src/settings.rs`
  (`Setup::VERSION`), `crates/torro-gui/src/icon.rs` und `crates/torro-gui/assets/icons/` (neue Icons),
  `crates/torro-core/src/i18n/de.rs`
- **Oberfläche:** Einrichtung, neuer Schritt nach „Web search“ · **API:** keine neue; der Schritt liest
  `extensions.list_catalog` (gibt es, Read) · **MCP:** `extensions_list_catalog` (gibt es). Er stellt nur vor und
  speichert nichts; eingerichtet wird unter MCP servers (in der Planung entschieden, Alternative „im Wizard
  installieren“ verworfen: zweite Umsetzung des Installierens, und ohne freigegebenen Ordner ginge es nicht weiter).
- `Step::Documents` zwischen `Step::Web` und `Step::Done`, `arg` `documents`, Name „Documents“, Icon `FileText`,
  `component()` = `Component::Assistant` (die Werkzeuge sind die des Assistenten und der Agenten). `Setup::VERSION`
  auf 3 mit Doc-Kommentar „3: the step Documents (#303)“; die Overview zeigt damit „New in setup“ (gibt es).
- Beim Betreten des Schritts (auch mit `--step documents`) den Katalog laden wie MCP servers (`load_catalog`).
- Seite: Titel und Einleitung, dann eine Gruppe „ArtCraft apps“ mit je einer Zeile für `wordcraft`, `deckcraft`,
  `gridcraft`, `pdfcraft` aus dem Katalog: Kachel mit Icon (`FileText`, `Presentation`, `Sheet`, `FileType`; neu aus
  Lucide 0.469.0 nach den Regeln in `icon.rs`), Name mit der Pille „Experimental“, darunter Art und Beispiele, rechts
  der Zustand: als Server installiert → `ok`-Punkt „Set up“; gefunden (`missing` leer) → `ok`-Punkt „Installed“;
  sonst ghost-Knopf „Get <App>“ mit `ArrowUpRight` (öffnet `url` des Eintrags, `Action::OpenUrl`). Fußzeile der
  Gruppe und ein Satz darunter wie im Text unten. Lädt der Katalog nicht: `loading()` bzw. ein Satz ohne Zeilen,
  „Continue“ geht immer. Kein Primärknopf außer „Continue“; „Skip“ wie bei jedem Schritt.
- Text (englisch im Code mit `tr()`, die deutsche Fassung nach `de.rs`; Abweichungen im Wortlaut nur, wo die Breite
  es verlangt):
  - Titel: „Work with your documents“ / „Mit deinen Dokumenten arbeiten“
  - Einleitung: „Let the assistant open, read and edit your documents, presentations, spreadsheets and PDFs. The free
    ArtCraft apps do the work right here on your computer, and Torro keeps your originals safe: every change is saved
    as a new file.“ / „Lass den Assistenten deine Dokumente, Präsentationen, Tabellen und PDFs öffnen, lesen und
    bearbeiten. Die freien ArtCraft-Apps erledigen das direkt auf deinem Rechner, und Torro schützt deine Originale:
    Jede Änderung wird als neue Datei gespeichert.“ (Nicht „ohne hochzuladen“: Was der Assistent liest, geht an das
    gewählte Modell, auch in die Cloud, `docs/api.md` „Datenregel“.)
  - WordCraft: „Documents (.docx) · Summarise a contract, fix the wording of a letter, draft a reply from your notes“
    / „Dokumente (.docx) · Einen Vertrag zusammenfassen, einen Brief sprachlich glätten, aus Notizen eine Antwort
    entwerfen“
  - DeckCraft: „Presentations (.pptx) · Turn an outline into slides, add a chart, tidy up every slide's text“ /
    „Präsentationen (.pptx) · Aus einer Gliederung Folien machen, ein Diagramm einfügen, die Texte aller Folien
    glätten“
  - GridCraft: „Spreadsheets (.xlsx, .csv) · Read out figures, fill in a table, sort and chart your data“ /
    „Tabellen (.xlsx, .csv) · Zahlen auslesen, eine Tabelle ausfüllen, Daten sortieren und als Diagramm zeigen“
  - PdfCraft: „PDF · Find a passage, fill in a form, comment, combine or split files“ / „PDF · Eine Stelle finden, ein
    Formular ausfüllen, kommentieren, Dateien zusammenfügen oder teilen“
  - Fußzeile der Gruppe: „Open source (MIT or Apache 2.0) and still young: the apps call themselves experimental. Each
    one works only in a folder you choose; changes are saved as new files, and anything that could replace a file asks
    you first. What the assistant reads goes to the AI model you chose.“ / „Quelloffen (MIT oder Apache 2.0) und noch
    jung: Die Apps nennen sich selbst experimentell. Jede arbeitet nur in einem Ordner, den du wählst; Änderungen
    werden als neue Dateien gespeichert, und was eine Datei ersetzen könnte, fragt dich vorher. Was der Assistent
    liest, geht an das KI-Modell, das du gewählt hast.“
  - Satz darunter: „Installed one? Set it up later in Settings › MCP servers › Catalog.“ / „Eine installiert? Richte
    sie später unter Einstellungen › MCP-Server › Katalog ein.“
  - Zustände: „Installed“ / „Installiert“, „Set up“ / „Eingerichtet“, „Get {app}“ / „{app} holen“.
- Demo: `--open setup-documents` mit allen vier Einträgen, gemischt (einer eingerichtet, einer installiert, zwei
  fehlen); im Skill `torro-design` nachtragen (AC.7).
- **Abnahme:** Tests: Schritt nur bei eingeschaltetem Assistenten in `steps`; „Get <App>“ öffnet die `url` des
  Eintrags; Zustand aus `missing` und installierten Servern; `Setup::VERSION` 3 und „New in setup“ für eine mit
  Version 2 abgeschlossene Einrichtung; Sichtprüfung in allen Größenklassen und in Standard, Compact, Console
  (`torro-design-review`); Übersetzungen.

### AC.7 Doku
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** AC.1–AC.6 · **Dateien:** `docs/erweiterungen.md`, `docs/assistent.md`,
  `docs/settings.md`, `docs/api.md`, `docs/einrichtung.md`, `.claude/skills/torro-design/SKILL.md` (nur die Demo-Zustände)
- `erweiterungen.md`: Abschnitt „Dokumente mit ArtCraft (#303)“ mit den Festlegungen dieser Datei (auch die offenen
  Punkte: Sperren nach Namen, Allzweck-Befehle können überschreiben), unter „Abweichungen“ Startoptionen statt Roots,
  Neubewertung um
  2027-01-26 (Rechtliches, Rechte-Stufen und mehrere Ordner bei ArtCraft, Grenzen in GridCraft). `assistent.md` und
  `settings.md`: „ArtCraft später“ ersetzen, Zeile „Folder“ der Seite eines Servers mit `set_folder` (Never).
  `einrichtung.md`: Schritt 10 „Documents“ (Done wird 11), Zeile in „API und MCP“ (`extensions.list_catalog`).
- **Abnahme:** Verweise stimmen; nichts widerspricht dem Code.
