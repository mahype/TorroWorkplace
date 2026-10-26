# Aufgaben: Dokumente mit ArtCraft (AC.1–AC.6)

Konzept: [erweiterungen.md](../erweiterungen.md) (Abschnitt „MCP-Server (#302)“, ab AC.6 auch „Dokumente mit ArtCraft
(#303)“), [api.md](../api.md) („Was nie nach außen geht“, Punkte 9 und 10), [assistent.md](../assistent.md)
(„Eigene Werkzeuge“, freigegebene Ordner #304). Issue #303, geplant am 2026-10-26. Regeln: [CLAUDE.md](../../CLAUDE.md),
Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen Request anfasst, lädt `torro-api`;
nach jeder Funktion prüft `torro-api-review`.

```
AC.1 (Gerüst) ── AC.2 (Katalog: vier Einträge, Format) ── AC.3 (Ordner je App, Grenzen beim Start) ── AC.4 (Pfadprüfung, Speichern als Kopie, Sperrliste) ──┬── AC.5 (Oberfläche)
                                                                                                                                                              └── AC.6 (Doku)
```

Reihenfolge in einem Strang: AC.1, AC.2, AC.3, AC.4, AC.5, AC.6.

**Festlegungen für alle AC-Aufgaben** (Stand der Quellen am 2026-10-26: WordCraft v0.11.0, DeckCraft v0.11.1, GridCraft
v0.12.1, PdfCraft v0.14.0, je `MIT OR Apache-2.0`, `unsafe_code = "forbid"`):

- **Vier Katalog-Einträge**, jede App ein eigener Server mit eigener Stufe je Server (`Rights::extensions`, gibt es
  seit #302): `wordcraft`, `deckcraft`, `gridcraft`, `pdfcraft`. Befehl `<app>-cli`, Argumente `["mcp"]`, bei PdfCraft
  `["mcp", "--compact"]`. Nie `--connect` (steuert die laufende App ohne Grenzen), nie `--root` bei PdfCraft (ältere
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
  `sign_id_create`, `js_run` und der Befehl `tools.js_console` (dahin führt `command_run` für JavaScript). Werkzeuge,
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
- [ ] erledigt
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
- [ ] erledigt
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
  - **PdfCraft** (`--compact`, 17 Werkzeuge): `read`: `doc_open` (`reads`), `doc_info`, `doc_close`, `page_render`,
    `text_extract`, `text_find`, `command_list`, `doc_inspect`, `render_preview`, `tool_search`; `write`: `doc_save`
    (`target`, Pflicht), `doc_combine` (`reads = ["paths"]`, `target = "out"`; ohne `out` schreibt es nichts),
    `edit_undo`; `delete`: `doc_split`, `command_run`, `command_batch`, `tool_call`.
- **Abnahme:** `the_built_in_catalog_is_valid` nennt sieben Einträge und prüft je ArtCraft-Eintrag Befehl, Argumente,
  `folder`, die Sperrliste und die Pflicht-Ziele; Tests für jede neue Regel (wie `broken_entries_are_refused`);
  `look_in` mit `{ProgramFiles}` in einem Temp-Ordner gefunden; Einträge ohne die neuen Felder lesen sich wie bisher.

### AC.3 Ordner je App und Grenzen beim Start
- [ ] erledigt
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
- [ ] erledigt
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
  `id = "tools.js_console"`, `execute` mit `command = "ui.click"`, innerer Lese-Ordner nach dem Wählen angelegt); Prozess-Test mit `artcraft.sh`:
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
  `extension-folder` (WordCraft) und `extension-unbounded` (GridCraft) in `--open`, im Skill `torro-design` nachtragen.
- **Abnahme:** Test: „Choose…“ schickt den Request mit Pfad, „None“ mit `null`; Sichtprüfung beider Zustände in allen
  Größenklassen und in Standard, Compact, Console (Agent `torro-design-review`); Übersetzungen.

### AC.6 Doku
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** AC.1–AC.5 · **Dateien:** `docs/erweiterungen.md`, `docs/assistent.md`,
  `docs/settings.md`, `docs/api.md`, `.claude/skills/torro-design/SKILL.md` (nur die Demo-Zustände)
- `erweiterungen.md`: Abschnitt „Dokumente mit ArtCraft (#303)“ mit den Festlegungen dieser Datei (auch die offenen
  Punkte: Sperren nach Namen, Allzweck-Befehle können überschreiben), unter „Abweichungen“ Startoptionen statt Roots,
  Neubewertung um
  2027-01-26 (Rechtliches, Rechte-Stufen und mehrere Ordner bei ArtCraft, Grenzen in GridCraft). `assistent.md` und
  `settings.md`: „ArtCraft später“ ersetzen, Zeile „Folder“ der Seite eines Servers mit `set_folder` (Never).
- **Abnahme:** Verweise stimmen; nichts widerspricht dem Code.
