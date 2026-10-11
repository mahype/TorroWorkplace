# Aufgaben: API, Rechte und MCP

Konzept: [api.md](../api.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Skill `torro-api`; bei
Oberfläche zusätzlich Skill `torro-design`. Nach jeder Aufgabe mit API- oder MCP-Anteil prüft der
Agent `torro-api-review`.

Reihenfolge: A.0 → A.1 → A.2 → (A.3, A.4, A.5 parallel) → A.6, A.7 → A.8. A.9 erst nach
Entscheidung (offene Frage 1 in `api.md`).

Neue Abhängigkeiten (in `[workspace.dependencies]`): `schemars` (A.0), `sha2`, `getrandom` (A.1),
`rmcp` (A.4). Versionen beim Einbau prüfen.

## Aufgaben

### A.0 Befehlskatalog
- [ ] erledigt
- **Modell:** Opus · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs` (neu), `Cargo.toml`, `crates/torro-ipc/Cargo.toml`
- `Area`, `Access`, `Mcp`, `Operation`; `Request::operation()` als vollständiges `match`;
  `CATALOGUE` mit englischer Beschreibung je Befehl. `Collections` nach Bereich trennen
  (`CalendarCollections`, `TaskCollections`) und `SetSetting { key, value }` ergänzen, mit
  Kern-Handlern.
- Fehlercodes `unauthorized`, `forbidden`, `pending` dokumentieren.
- **Abnahme:** Test „Namen eindeutig, jeder Fall im Katalog“; JSON-Schema je Request über `schemars`.

### A.1 Schlüssel und Anmeldung
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.0 · **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/client.rs`, `crates/torro-ipc/src/server.rs`, `crates/torroworkplace/src/clients.rs` (neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`
- Umschlag `{"v":1,"key":…,"request":…}`; App-Schlüssel `app.key` (0600) anlegen und im Client
  automatisch senden; `clients.json` mit SHA-256, Schlüssel im Schlüsselbund; Prüfung je Anfrage,
  `ping` ohne Schlüssel, sonst `unauthorized`; fail closed.
- **Abnahme:** Tests für gültig / falsch / widerrufen mitten in der Sitzung / kaputte `clients.json`.

### A.2 Rechte im Dienst
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.1 · **Dateien:** `crates/torroworkplace/src/clients.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`
- Stufe je Bereich, `Admin`-Schalter, Auswahl der Sammlungen (Filter vor dem Lesen), Vorlagen
  *Read only*, *Read and write*, *Full access*. Befehle `clients.*` nur mit App-Schlüssel.
- **Abnahme:** Tabelle-Test über alle Katalogeinträge × Stufen; fremde Sammlung = `not-found`.

### A.3 Freigaben
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.2 · **Dateien:** `crates/torroworkplace/src/approvals.rs` (neu), `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/notify.rs`, `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/api.rs`
- Offene Freigaben in `approvals.json`, Ablauf nach 10 Minuten, `pending`-Antwort mit
  Zusammenfassung, `approvals.list/status/decide` (decide nur App-Schlüssel), Benachrichtigung mit
  „Allow“/„Deny“, Rechte bei Ausführung erneut prüfen.
- **Abnahme:** Agent kann sich nicht selbst freigeben; Neustart verliert nichts; Ablauf greift.

### A.4 MCP-Server
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.1 · **Dateien:** `crates/torroworkplace/src/mcp.rs` (neu), `crates/torroworkplace/src/main.rs`, `crates/torroworkplace/Cargo.toml`, `Cargo.toml`
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

### A.10 HTTP-API (nach Entscheidung)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** A.2
- `127.0.0.1`, Standard aus, `POST /v1/<bereich>/<befehl>`, Bearer-Schlüssel, `/v1/openapi.json`.
