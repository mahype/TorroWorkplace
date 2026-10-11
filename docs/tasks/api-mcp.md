# Aufgaben: API, Rechte und MCP

Konzept: [api.md](../api.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Skill `torro-api`; bei
Oberfläche zusätzlich Skill `torro-design`. Nach jeder Aufgabe mit API- oder MCP-Anteil prüft der
Agent `torro-api-review`.

Entscheidungen: [api.md, „Entscheidungen“](../api.md#entscheidungen) (2026-10-11).

Reihenfolge in drei Meilensteinen:

1. **Alle bisherigen Funktionen über MCP, mit Rechten:** A.0 → (A.0b, A.1 parallel) → A.2 → A.4.
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
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** A.0 · **Dateien:** `crates/torroworkplace/src/ui/apps.rs`, `crates/torroworkplace/src/ui/accounts.rs`, `crates/torroworkplace/src/ui/general.rs`, `crates/torroworkplace/src/ui/mod.rs`
- Apps-Seite zeigt die Antwort von `ListApps` statt selbst zu suchen. „Kalender ein/aus“ in
  Accounts schickt `SetCollectionShown`, die Schalter in General schicken `SetSetting`.
  `UpdateSettings` schickt die Oberfläche danach nicht mehr.
- **Abnahme:** Tests prüfen, dass jede Aktion den gezielten Request schickt; Design-Prüfung
  der Apps-Seite.

### A.1 Schlüssel und Anmeldung
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.0 · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/client.rs`, `crates/torro-ipc/src/server.rs`, `crates/torroworkplace/src/clients.rs` (neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`
- Umschlag `{"v":1,"key":…,"request":…}`; App-Schlüssel `app.key` (0600) anlegen und im Client
  automatisch senden; `clients.json` mit SHA-256, Schlüssel im Schlüsselbund; Prüfung je Anfrage,
  `ping` ohne Schlüssel, sonst `unauthorized`; fail closed.
- **Abnahme:** Tests für gültig / falsch / widerrufen mitten in der Sitzung / kaputte `clients.json`.

### A.2 Rechte im Dienst
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.1 · **Dateien:** `crates/torroworkplace/src/clients.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/cli.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`
- Stufe je Bereich, `Admin`-Schalter, Auswahl der Sammlungen (Filter vor dem Lesen), Vorlagen
  *Read only*, *Read and write*, *Full access*.
- Requests `clients.list`, `clients.create` (gibt den Schlüssel genau einmal zurück),
  `clients.update` (Rechte), `clients.new_key`, `clients.remove`: im Katalog, `Mcp::Never`,
  nur mit App-Schlüssel.
- `torroworkplace ctl clients list|add|remove|new-key`, damit sich Agenten schon vor A.6 verbinden
  lassen.
- **Abnahme:** Tabelle-Test über alle Katalogeinträge × Stufen; fremde Sammlung = `not-found`;
  `clients.*` mit Client-Schlüssel = `forbidden`.

### A.3 Freigaben
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.2 · **Dateien:** `crates/torroworkplace/src/approvals.rs` (neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/notify.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`
- Offene Freigaben in `approvals.json`, Ablauf nach 10 Minuten, `pending`-Antwort mit
  Zusammenfassung, `approvals.list/status/decide` (decide nur App-Schlüssel), Benachrichtigung mit
  „Allow“/„Deny“, Rechte bei Ausführung erneut prüfen.
  `Admin`-Befehle eines Agenten brauchen immer eine Freigabe (Entscheidung 3).
- **Abnahme:** Agent kann sich nicht selbst freigeben; Neustart verliert nichts; Ablauf greift;
  Admin-Befehl eines Agenten wartet auch bei „Freigabe vorher: Nie“.

### A.4 MCP-Server
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.1 (zusammenführen erst nach A.2) · **Dateien:** `crates/torroworkplace/src/mcp.rs` (neu), `crates/torroworkplace/src/main.rs`, `crates/torroworkplace/Cargo.toml`, `Cargo.toml`
- `torroworkplace mcp` auf `rmcp`, stdio, Schlüssel aus `TORRO_TOKEN`. Tools aus dem Katalog
  (`Mcp::Tool`), Schema aus `schemars`, bequeme Eingaben für Termine und Aufgaben. Wartet bei
  `pending` bis 60 s. Startet den Dienst, wenn er nicht läuft.
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

### A.9 API-Referenz und Abschluss
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.4, A.6, A.8
- `docs/api-reference.md` aus dem Katalog erzeugen (Test mit `TORRO_BLESS=1`), Übersetzungen
  eintragen, Design-Prüfung der neuen Seiten, `torro-api-review` über alles.

### A.10 HTTP-API (erst bei einem ersten Nutzer)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.2
- `127.0.0.1`, Standard aus, `POST /v1/<bereich>/<befehl>`, Bearer-Schlüssel, `/v1/openapi.json`.
