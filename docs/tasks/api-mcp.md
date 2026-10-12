# Aufgaben: API, Rechte und MCP

Konzept: [api.md](../api.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Skill `torro-api`; bei
Oberfläche zusätzlich Skill `torro-design`. Nach jeder Aufgabe mit API- oder MCP-Anteil prüft der
Agent `torro-api-review`.

Entscheidungen: [api.md, „Entscheidungen“](../api.md#entscheidungen) (2026-10-11).

Reihenfolge in drei Meilensteinen:

1. **Alle bisherigen Funktionen über MCP, mit Rechten:** A.0 → (A.0b, A.0d parallel) → A.0c → A.1 →
   A.2 → (A.4, A.2b, A.2c parallel). Bis A.3 lehnt der Dienst ab, was eine Freigabe bräuchte.
   A.4 darf ab A.1 entstehen, wird aber erst nach A.2 zusammengeführt, damit es nie einen
   Agenten-Schlüssel ohne Rechteprüfung gibt.
2. **Sicherheit und Nachvollziehbarkeit:** A.3 und A.5 parallel.
3. **Oberfläche:** A.6 → A.7, A.8. Danach A.9. A.10 (HTTP) erst, wenn es einen Nutzer gibt.

Neue Abhängigkeiten (in `[workspace.dependencies]`): `schemars` (A.0), `sha2`, `getrandom` (A.1),
`rmcp` (A.4). Versionen beim Einbau prüfen.

## Aufgaben

### A.0 Befehlskatalog
- [x] erledigt
- **Modell:** Opus · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs` (neu), `crates/torro-ipc/src/pim.rs`, `Cargo.toml`, `crates/torro-ipc/Cargo.toml`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/settings_service.rs`, `crates/torroworkplace/src/daemon.rs`, `crates/torroworkplace/src/apps.rs` (neu), `crates/torroworkplace/src/main.rs`
- `Area`, `Access`, `Mcp`, `Operation`; `Request::operation()` als vollständiges `match`;
  `CATALOGUE` mit englischer Beschreibung je Befehl, Namen wie in der Tabelle in `api.md`.
- Zugriff nach den Entscheidungen: `update_template` = `Delete`; `load_template` = `Write`, mit
  `mode: replace` `Delete` (die Stufe kommt aus `Request::operation()`, nicht nur aus dem Namen).
- Neue Requests mit Kern-Handlern:
  - `CalendarCollections` und `TaskCollections` statt `Collections`; `PimClient::collections()`
    fragt beide und führt sie zusammen, damit die Apps unverändert bleiben.
  - `SetSetting { key, value }` (`settings.set_setting`, Admin) für `language` und
    `start_at_login`.
  - `SetCollectionShown { collection, shown }` (`settings.set_collection_shown`, Admin).
  - `ListApps` (`apps.list_apps`, Read): Suite-Apps mit „installiert“, ermittelt im Dienst
    (`apps.rs`, Logik aus `ui/apps.rs` übernehmen; die Oberfläche stellt A.0b um).
- `Account` und `DisconnectAccount` entfernen; `PimClient::account()` liest den Zustand aus
  `Accounts`.
- Fehlercodes `unauthorized`, `forbidden`, `pending` dokumentieren.
- **Abnahme:** Test „Namen eindeutig, jeder Fall im Katalog“; JSON-Schema je Request über `schemars`;
  Handler-Tests für die neuen Requests.

### A.0b Oberfläche auf die neuen Requests umstellen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** A.0 · **Dateien:** `crates/torroworkplace/src/ui/apps.rs`, `crates/torroworkplace/src/ui/accounts.rs`, `crates/torroworkplace/src/ui/general.rs`, `crates/torroworkplace/src/ui/mod.rs`
- Apps-Seite zeigt die Antwort von `ListApps` statt selbst zu suchen. „Kalender ein/aus“ in
  Accounts schickt `SetCollectionShown`, die Schalter in General schicken `SetSetting`.
  `UpdateSettings` schickt die Oberfläche danach nicht mehr.
- **Abnahme:** Tests prüfen, dass jede Aktion den gezielten Request schickt; Design-Prüfung
  der Apps-Seite.

### A.0c Volle Schemata für Termin, Aufgabe und Einstellungen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** A.0d · **Dateien:** `crates/torro-pim/Cargo.toml`, `crates/torro-pim/src/*.rs`, `crates/torro-core/Cargo.toml`, `crates/torro-core/src/settings.rs`, `crates/torro-core/src/theme.rs`, `crates/torro-setups/Cargo.toml`, `crates/torro-setups/src/template.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`, `Cargo.lock`
- `JsonSchema` für `Event`, `Todo`, `Settings`, `LoadMode` und deren Feldtypen ableiten (Entscheidung
  7); in `api.rs` die Platzhalter `{"type":"object"}` und `LoadModeSchema` ersetzen. Doc-Kommentare
  der Felder sind die Beschreibungen für Agenten, also prüfen, dass `href`, `etag` und `collection`
  erklärt sind.
- **Abnahme:** Schema-Test: `save_event`, `delete_event`, `save_todo`, `delete_todo`, `update_settings`,
  `load_template` haben keine leeren Objekte mehr.

### A.0d Nacharbeit aus dem API-Check von A.0
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.0 · **Dateien:** `crates/torro-ipc/src/api.rs`, `crates/torroworkplace/src/settings_service.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`
- Beschreibungen: `accounts.list_accounts` verspricht keine ausgeblendeten Sammlungen mehr;
  `delete_event`/`delete_todo`: „Pass the item as returned by list_… (needs `href` and `etag`).“;
  `connect_nextcloud`: „Show the URL to the person; never open it yourself.“; `sync`: Remote Wipe
  kann das Konto entfernen.
- `SetSetting`: Wert prüfen, bevor die Datei geladen wird; `settings_service` bekommt den Pfad
  (bzw. `AppPaths`) übergeben, damit Tests nie die echte `settings.json` lesen.
- Handler-Tests für `SetSetting` und `SetCollectionShown` über `Core::handle` im Temp-Verzeichnis.
- `remove_account` entfernt die Sammlungen des Kontos aus `hidden_collections`.
- `input_schema()` einmal erzeugen (`OnceLock`).
- **Abnahme:** kein Test liest oder schreibt die echte Konfiguration; neue Tests grün.

### A.1 Schlüssel und Anmeldung
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.0c · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`, `crates/torro-ipc/src/client.rs`, `crates/torro-ipc/src/server.rs`, `crates/torroworkplace/src/clients.rs` (neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`
- Umschlag `{"v":1,"key":…,"request":…}`; App-Schlüssel `app.key` (0600) anlegen und im Client
  automatisch senden; `clients.json` mit SHA-256, Schlüssel im Schlüsselbund; Prüfung je Anfrage,
  `ping` ohne Schlüssel, sonst `unauthorized`; fail closed.
- `method` im Umschlag ist der Katalogname (Entscheidung 6); das Feld `Command::method` mit den alten
  Draht-Namen entfällt.
- **Abnahme:** Tests für gültig / falsch / widerrufen mitten in der Sitzung / kaputte `clients.json`;
  jeder Request wird mit seinem Katalognamen serialisiert.

### A.2 Rechte im Dienst
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.1 · **Dateien:** `crates/torroworkplace/src/clients.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/cli.rs`, `crates/torroworkplace/src/daemon.rs`, `crates/torroworkplace/src/notify.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`, `crates/torro-ipc/src/server.rs`, `crates/torro-ipc/src/pim.rs`
- Stufe je Bereich, `Admin`-Schalter, Auswahl der Sammlungen (Filter vor dem Lesen), Vorlagen
  *Read only*, *Read and write*, *Full access*.
- Requests `clients.list_clients`, `clients.create_client` (gibt den Schlüssel genau einmal zurück),
  `clients.update_client` (Rechte), `clients.new_key`, `clients.remove_client`: im Katalog, `Mcp::Never`,
  nur mit App-Schlüssel.
- `torroworkplace ctl clients list|add|remove|new-key`, damit sich Agenten schon vor A.6 verbinden
  lassen.
- **Ausgeblendetes bleibt verborgen:** Für Client-Schlüssel filtert der Dienst ausgeblendete und nicht
  freigegebene Sammlungen auch aus `accounts.list_accounts` und `settings.get_settings`
  (`hidden_collections`). Nur der App-Schlüssel sieht alle. `set_collection_shown` geht für einen
  Agenten nur bei Sammlungen aus seiner Auswahl.
- Ob eine Stufe eine andere abdeckt (`Read` < `Write` < `Delete`, `Admin` als eigener Schalter),
  als Funktion an `Access`.
- Aus dem API-Check von A.1: `Response::Pong` meldet die API-Version (`api: u32`, optional,
  verträglich); Datei- und Schlüsselbund-Zugriffe in `clients.rs` über `spawn_blocking`, damit sie
  den Dienst nicht blockieren; Konto-Kennungen im Schlüsselbund dürfen nie mit `client-` beginnen.
- **Abnahme:** Tabelle-Test über alle Katalogeinträge × Stufen; fremde Sammlung = `not-found`;
  ausgeblendete Sammlung taucht für einen Client-Schlüssel in keiner Antwort auf;
  `clients.*` mit Client-Schlüssel = `forbidden`.

### A.2b Serien und Überschreiben im Kern absichern
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.2 · **Dateien:** `crates/torroworkplace/src/pim.rs`, `crates/torro-pim-caldav/src/write.rs`, `crates/torro-pim-caldav/src/*.rs` (nur falls für das Lesen eines Elements nötig)
- Aus dem API-Check von A.4: Ob ein Termin zu einer Serie gehört, prüft der Kern heute nur am
  mitgeschickten `recurring`. Künftig bestimmt er es selbst aus Cache oder Server anhand von `href`;
  `save_event`/`delete_event` auf eine Serie bleiben `unsupported`, egal was der Client schickt.
- Ändern oder Löschen eines vorhandenen Elements (mit `href`) ohne `etag` wird abgelehnt
  (`conflict` bzw. `error` mit klarer Meldung); kein PUT/DELETE ohne `If-Match` auf vorhandene
  Elemente. Neu anlegen bleibt `If-None-Match: *`.
- **Abnahme:** Tests: Serie mit `recurring: false` überschreiben → abgelehnt; Update ohne `etag` →
  abgelehnt; normales Update mit `etag` → geht.

### A.2c Tray-Befehle in der API
- [x] erledigt
- **Modell:** Haiku · **Braucht:** A.2 · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`, `crates/torroworkplace/src/apps.rs`, `crates/torroworkplace/src/tray/linux.rs`, `crates/torroworkplace/src/cli.rs`, `crates/torroworkplace/src/clients.rs`
- Entscheidung 8 in `api.md`:
  - **Oberfläche:** Tray „Settings…“ und „Quit“
  - **API:** `apps.open_app` nimmt auch die Steuerzentrale (`torroworkplace`, öffnet `torroworkplace ui`);
    `apps.list_apps` bleibt ohne sie. Neu `Request::Quit` · `system.quit` · Zugriff `Admin`, nur App-Schlüssel
    (Client-Schlüssel → `forbidden`, wie `clients.*`).
  - **MCP:** `apps_open_app` deckt die Steuerzentrale mit ab; `system.quit`: Never (Punkt 4 in „Was nie
    nach außen geht“).
- Tray schickt beide als Request an den eigenen Kern statt `daemon.open_settings()`/`quit()` direkt;
  `ctl quit` dazu.
- **Abnahme:** Tests: `system.quit` mit Client-Schlüssel → `forbidden`; Tray-Einträge bauen die Requests;
  Katalog- und Vertragstests grün.

### A.3 Freigaben
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.2, A.4 · **Dateien:** `crates/torroworkplace/src/approvals.rs` (neu), `crates/torroworkplace/src/mcp.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/notify.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`, `crates/torro-core/src/account.rs`
- Offene Freigaben in `approvals.json`, Ablauf nach 10 Minuten, `pending`-Antwort mit
  Zusammenfassung, `approvals.list/status/decide` (decide nur App-Schlüssel), Benachrichtigung mit
  „Allow“/„Deny“, Rechte bei Ausführung erneut prüfen. MCP-Tools warten bei `pending` bis 60 s auf
  die Entscheidung, sonst „waiting for approval“ mit Kennung.
  `Admin`-Befehle eines Agenten brauchen immer eine Freigabe (Entscheidung 3).
- Bis A.3 lehnt der Dienst alles, was eine Freigabe bräuchte, mit `forbidden` ab
  (`Refused::NeedsApproval` in `Core::handle_from`); A.3 ersetzt genau diesen Zweig durch `pending`.
- Bei der Ausführung einer Freigabe Rechte **und** Reichweite (`out_of_reach`, ausgeblendete
  Sammlungen) erneut prüfen; zwischen Anfrage und Entscheidung kann sich beides geändert haben.
- Aus dem API-Check von A.2: `torro-core/src/account.rs` lehnt Konto-Kennungen ab, die mit
  `client-` beginnen (beim Laden und in `keychain_entry()`), damit kein Konto den Schlüsselbund-
  Eintrag eines Agenten überschreiben oder löschen kann.
- **Abnahme:** Agent kann sich nicht selbst freigeben; Neustart verliert nichts; Ablauf greift;
  Admin-Befehl eines Agenten wartet auch bei „Freigabe vorher: Nie“.

### A.3b Freigaben absichern (aus der API-Prüfung von A.3)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.3 · **Dateien:** `crates/torroworkplace/src/{approvals,pim,core,notify,main}.rs`,
  `crates/torroworkplace/src/cli.rs` (nur `ctl approvals`), `crates/torro-ipc/src/api.rs` (nur Katalogbeschreibung),
  `docs/api.md`, `.claude/skills/torro-api/SKILL.md`
- **Hoch: Die Zusammenfassung darf nicht lügen.** Heute baut sie Titel und Datum aus den Angaben des Clients,
  ausgeführt wird aber über `href`. Für alle Befehle mit `href` (`DeleteEvent`, `DeleteTodo`, `SaveEvent`/`SaveTodo`
  mit `href`, Verschieben, `UnpinIssue`) kommen Titel, Beginn und Kalender bzw. Liste aus dem Cache des Kerns;
  unbekannt → ehrlich „unknown event“ bzw. „unknown task“; Änderungen zeigen „'alt' → 'neu'“. Sammlungen,
  Filterlisten und Vorlagen mit Anzeigenamen statt Kennung. Test: Client schickt falschen Titel → die
  Zusammenfassung zeigt den echten.
- **Mittel:** `quote()` entfernt auch Format-Zeichen (Unicode Cf: Zero-Width, Bidi-Steuerung) mit Test.
  Offene Freigaben beim Start des Dienstes erneut ankündigen. Bis A.8 ein Weg ohne Linux-Benachrichtigung:
  `torroworkplace ctl approvals` (Liste) und `ctl allow|deny <id>` (App-Schlüssel).
- **Mittel, Regeln:** `pending` ist eine Antwort-Variante, kein Fehlercode: Skill `torro-api` korrigieren;
  in `docs/api.md` „Versionen“ festhalten, dass Clients unbekannte `result`-Werte tolerieren müssen; MCP
  meldet „waiting“ weiter als Tool-Fehler mit Kennung (so dokumentieren).
- **Niedrig:** Ein-/Ausgabefehler gegenüber Clients ohne Pfad (Pfad nur ins Log); Benachrichtigung nach einer
  Entscheidung schließen und das Warten begrenzen; leerer Client-Name → Client-Kennung; Katalog
  `approval_status`: MCP-Tools warten schon bis 60 s selbst. In `notify.rs`/`api.md` vermerken, dass
  D-Bus-`ActionInvoked` nicht nach Absender gefiltert wird (Modell: keine Sandbox gegen Code desselben Nutzers).
- **API/MCP:** keine neuen Befehle (die `ctl`-Befehle nutzen `approvals.*`).
- **Abnahme:** Test mit falschem Titel; Cf-Test; Neustart kündigt an (Fake-Benachrichtiger); `ctl approvals`
  im Temp-Verzeichnis; `torro-api-review` ohne hohen Befund.

### A.4 MCP-Server
- [x] erledigt
- **Modell:** Opus · **Braucht:** A.1 (zusammenführen erst nach A.2) · **Dateien:** `crates/torroworkplace/src/mcp.rs` (neu), `crates/torroworkplace/src/main.rs`, `crates/torroworkplace/Cargo.toml`, `Cargo.toml`
- `torroworkplace mcp` auf `rmcp`, stdio, Schlüssel aus `TORRO_TOKEN`. Tools aus dem Katalog
  (`Mcp::Tool`), Schema aus `schemars`, bequeme Eingaben für Termine und Aufgaben. Startet den
  Dienst, wenn er nicht läuft. Das Warten auf `pending` (bis 60 s) kommt mit A.3, weil es erst dort
  `approvals.approval_status` gibt.
- **Abnahme:** Vertragstest „jedes Tool im Katalog und umgekehrt“; Ende-zu-Ende-Test gegen einen
  Dienst im Temp-Verzeichnis (`tools/list`, ein Lese-, ein Schreib-Tool, `forbidden`).

### A.5 Protokoll
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** A.1 · **Dateien:** `crates/torroworkplace/src/audit.rs` (neu), `crates/torroworkplace/src/core.rs`
- `audit.jsonl` (0600, ohne Inhalte, Rotation bei 5 MB), „zuletzt benutzt“ je Client,
  `clients.activity { id }`.

### A.6 Steuerzentrale: Agents & API
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** A.2, A.5 · **Dateien:** `crates/torroworkplace/src/ui/agents.rs` (neu), `crates/torroworkplace/src/ui/mod.rs`
- Karten, Detailseite mit Rechten je Bereich und Sammlungen, „Create new key“, „Disconnect…“.
- Seit G.2 ([github.md](../github.md), „Rechte für Agenten“): Bereich **GitHub** mit Stufe und der Auswahl
  `github_repos` (Repos `owner/name` oder ganze Konten `owner`, „All“ oder ausgewählte). Ohne Lesen auf GitHub
  sieht ein Agent keine Filterliste; die Auswahl filtert jeden Treffer.

### A.7 Assistent „Connect agent“
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** A.6 · **Dateien:** `crates/torroworkplace/src/ui/agent_wizard.rs` (neu), `crates/torroworkplace/src/client_setup.rs` (neu)
- Clients erkennen, Schlüssel eintragen (Claude Code per `claude mcp add`, Codex per CLI, JSON
  atomar mit 0600), „Script“: einmal anzeigen.
- **Abnahme:** Tests mit Konfigurationen unter `tests/fixtures/`, kein echtes Home-Verzeichnis.

### A.8 Freigaben in Oberfläche und Tray
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** A.3 · **Dateien:** `crates/torroworkplace/src/ui/approvals.rs` (neu), `crates/torroworkplace/src/ui/mod.rs`, `crates/torroworkplace/src/tray/linux.rs`
- Liste mit „Allow“/„Deny“, Zahl am Tray-Icon.

### A.11 Kleinkram aus den API-Checks
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** A.3 · **Dateien:** `crates/torro-pim-caldav/src/write.rs`, `crates/torroworkplace/src/pim.rs`, `crates/torro-ipc/src/api.rs`, `crates/torro-ipc/src/pim.rs`
- `write.rs` `stored_item`: ohne `ETag`-Header im GET mit `conflict` ablehnen statt per PROPFIND
  nachholen (Inhalt und etag aus derselben Version); `precondition` meldet fehlenden etag als
  `Conflict` statt `Other`.
- `pim.rs` `check_writable`: unbekannte Sammlungen auch für die Suite ablehnen (`knows_collection`).
- `stored_item` gibt die Art (VEVENT/VTODO) zurück; der Kern gleicht sie mit dem Befehl ab.
- Katalog: `tasks.save_todo`/`delete_todo` nennen, dass wiederkehrende Aufgaben noch nicht änderbar sind.
- `PimClient` reicht den Code `unsupported` als eigenen Fehler durch, damit die Apps einen übersetzten
  Text zeigen statt der englischen API-Meldung (Text in der App mit `tr()`).

### A.9 API-Referenz und Abschluss
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.4, A.6, A.8
- `docs/api-reference.md` aus dem Katalog erzeugen (Test mit `TORRO_BLESS=1`), Übersetzungen
  eintragen, Design-Prüfung der neuen Seiten, `torro-api-review` über alles.

### A.10 HTTP-API (erst bei einem ersten Nutzer)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.2
- `127.0.0.1`, Standard aus, `POST /v1/<bereich>/<befehl>`, Bearer-Schlüssel, `/v1/openapi.json`.
