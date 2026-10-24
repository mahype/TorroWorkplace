# Aufgaben: MCP-Anbindungen (MA.1–MA.11)

Konzept: [erweiterungen.md](../erweiterungen.md) (Erweiterungen mit Code als MCP-Server, der Dienst als Client),
[api.md](../api.md) („Was nie nach außen geht“, Rechte, Freigaben), [assistent.md](../assistent.md) („Eigene
Werkzeuge“), [settings.md](../settings.md). Issue #302, geplant am 2026-10-24. Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

```
MA.1 (Gerüst) ──┬── MA.2 (Katalog, Voraussetzungen) ──────────────┐
                ├── MA.3 (Speicher, Geheimnisse, Umgebung) ── MA.4 (Client, Prozessaufsicht) ── MA.5 (Befehle, Rechte, Freigaben) ──┬── MA.6 (Angebot an Assistent, Bots, MCP)
                ├── MA.7 (Abschnitt MCP connections, eigene Werkzeuge ziehen um)                                                   │
                └── MA.10 (Rechte-Editor: Bereiche ext:<id>)                                                                       │
MA.2 + MA.5 + MA.7 ── MA.8 (Katalog, Installieren, Seite eines Servers) ── MA.9 (Eigener Server, mcpServers einfügen) ────────────┘
MA.11 (Doku) zuletzt
```

Reihenfolge der Umsetzung in einem Strang: MA.1, MA.2, MA.3, MA.4, MA.5, MA.6, MA.7, MA.10, MA.8, MA.9, MA.11.

**Festlegungen für alle MA-Aufgaben:**

- **Bereich `extensions`** (neu, `Area::Extensions`, Name `extensions`, nach `Tools`): Verwalten der Server
  (auflisten, Katalog, abschalten, entfernen). In den Vorlagen der Rechte wie jeder Bereich.
- **Rechte je Server** („Bereich `ext:<id>`“): `Rights::extensions: BTreeMap<String, Level>` (Kennung → Stufe,
  `#[serde(default)]`, fehlt = Kein Zugriff). `Area` bekommt **keinen** Fall je Server. Die Vorlagen
  (`RightsTemplate::rights`) öffnen nie einen Server, auch *Full access* nicht (wie `assistant`, `bots`). Ein
  Werkzeug eines Servers lässt sich einzeln sperren: `Rights::denied` nimmt `ext.<id>.<werkzeug>` an.
- **Kennung** eines Servers: `[a-z0-9-]`, 1–24 Zeichen, beginnt mit einem Buchstaben, eindeutig. **Werkzeugname
  nach außen:** `ext_<id>_<werkzeug>`, höchstens 64 Zeichen, nur `[A-Za-z0-9_-]`; ein Werkzeug des Servers, das
  das nicht erfüllt, wird nicht angeboten und in seinem Status genannt. Zurück wird an der ersten `_` nach `ext_`
  getrennt (die Kennung hat keinen `_`).
- **Zugriff je Werkzeug** nur aus dem Katalog-Eintrag (`Read`, `Write`, `Delete`), ohne Angabe `Delete`. `Admin`
  gibt es für Werkzeuge nicht. Hinweise des Servers (`readOnlyHint`, `destructiveHint`) zählen nicht.
- **Zustimmen:** Installieren startet den Server einmal, liest seine Werkzeuge und speichert ihn **aus** mit Status
  `review`. Erst „Allow tools“ (`extensions.confirm_tools` mit dem Fingerabdruck der gezeigten Liste) merkt sich
  die Werkzeuge und schaltet ein. Fingerabdruck je Werkzeug: SHA-256 über kanonisches JSON aus Name, Beschreibung
  und Eingabe-Schema. Meldet der Server später neue oder geänderte Werkzeuge (Start oder
  `notifications/tools/list_changed`), werden genau diese nicht angeboten, bis ein Mensch wieder zustimmt;
  unveränderte laufen weiter, verschwundene fallen weg.
- **Fremder Text:** Beschreibungen, Schemas und Antworten eines Servers sind `untrusted`, überall.
- **Nicht angeboten:** Elicitation (in der Planung entschieden), Sampling und Roots (kommen mit #304). Torro meldet
  diese Fähigkeiten beim `initialize` nicht an.
- **HTTP:** nur Streamable HTTP, Adresse `https://`, `http://` nur für `localhost`, `127.0.0.1`, `::1`; keine
  Weiterleitungen; Anmeldung nur über Header (in der Planung entschieden), OAuth kommt als eigenes Issue.
- **Geheimnisse** (Umgebungsvariablen und Header, die der Mensch als geheim markiert): im Schlüsselbund über
  `torro_core::account::vault`, Kennung `ext:<id>:<NAME>`; auf der Platte und in jeder Antwort nur der Name mit
  „gespeichert“/„fehlt“. Sie gehen nur beim Start an genau diesen Server.
- **Umgebung des Kindprozesses:** leer (`env_clear`), dann nur `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_ALL`, `TMPDIR`
  (Windows zusätzlich `SystemRoot`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATHEXT`,
  `ComSpec`), dann der `PATH`, den Torro setzt (Ordner des gefundenen Programms, PATH des Dienstes, übliche
  Ordner aus MA.2), dann die Variablen des Servers. **Sperrliste** (Groß-/Kleinschreibung egal, abgelehnt beim
  Installieren mit `error`): `PATH`, `PATHEXT`, `LD_*`, `DYLD_*`, `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`,
  `NODE_OPTIONS`, `NODE_PATH`, `PERL5LIB`, `PERL5OPT`, `RUBYOPT`, `RUBYLIB`, `BASH_ENV`, `ENV`, `IFS`, `TORRO_*`.
- **Prozess:** Start bei Bedarf (erster Aufruf, Installieren, Zustimmen), Startzeit höchstens 30 s, beim
  Installieren und Aktualisieren 180 s (der erste Lauf von `npx`, `uvx` bzw. `docker run` lädt das Paket erst),
  Aufruf höchstens 60 s, nach 10 min ohne Aufruf beendet. Nach einem Absturz neu beim nächsten Bedarf, höchstens 3 Starts in 5 min,
  dann Status `crashed` bis zum nächsten Einschalten. Beenden nimmt den ganzen Prozessbaum mit (Unix:
  Prozessgruppe; Windows: Job Object), auch beim Beenden des Dienstes. Unter Windows `CREATE_NO_WINDOW`.
  Arbeitsordner `<data>/extensions/<id>/work` (0700). Ausgaben auf stderr: die letzten 200 Zeilen bzw. 64 KB im
  Speicher, nie im Protokoll.
- **Speicher:** `<data>/extensions/extensions.json` (Ordner 0700, Datei 0600): je Server Kennung, Name, Quelle
  (Katalog-Eintrag mit Version oder eigener), Befehl bzw. Adresse, Variablen und Header (Werte nur, wenn nicht
  geheim), an/aus, zugestimmte Werkzeuge mit Fingerabdruck und Zugriff.
- **Protokoll und Log** nennen Kennung des Servers, Werkzeugname und Fehlerart, nie Argumente, Antworten oder
  Werte von Variablen.
- Tests brauchen kein Netz, kein Node, kein Python: Protokoll-Tests mit einem Fixture-Server in Rust über
  `tokio::io::duplex`, Prozess-Tests mit einem POSIX-Skript unter `crates/torroworkplace/tests/fixtures/extensions/`
  (`#[cfg(unix)]`; Windows wird nur gebaut). Alle Wurzeln (Daten, Home, PATH) als Parameter.

### MA.1 Gerüst: Bereich `extensions`, Requests, Katalog, Rechte
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torro-ipc/src/extensions.rs` (neu), `crates/torroworkplace/src/{core,audit,daemon,pim,mcp,clients}.rs`,
  `crates/torroworkplace/src/assistant/{tools,prompt}.rs`, `crates/torroworkplace/src/ui/*.rs` und
  `crates/torro-settings-ui/src/rights.rs` (nur `match`-Arme und Testdaten), `crates/torroassistant/src/chat.rs`
  (`area_name`), `crates/torroassistant/src/view.rs` (`area_icon`), `docs/api.md`, `docs/api-reference.md`
- `torro_ipc::extensions`: `ServerSpec` (`Stdio { command, args, env }` / `Http { url, headers }`), `Value`
  (`Plain(String)` / `Secret(Secret)` beim Installieren; gespeichert und gemeldet nur `Stored { secret: bool }`),
  `ExtensionInfo { id, name, source, server, enabled, status, tools, secrets, notes }` (`server` mit `Value::Stored`
  für Geheimnisse, damit das Formular „Update“ vorbefüllen kann), `Status` (`stopped`, `starting`, `running`,
  `review`, `crashed { reason }`, `missing { what }`; `reason` nur feste Fehlerarten `exited`, `start_timeout`,
  `spawn_failed`, `protocol`, nie Text des Servers, sonst umginge `get_extension` das gesperrte `read_logs`), `ExtensionTool { extension, name, tool_name,
  description, input_schema, access, state }` (`state`: `allowed`, `review`, `unsupported`), `CatalogEntry { id,
  name, publisher, license, url, description, version, experimental, requires, secrets, tools, missing }`,
  `Prerequisite` (`npx`, `uvx`, `docker`, `program { name }`), `ToolResult { content, is_error }` mit Text und
  Bild; Hilfen `valid_id`, `tool_name(id, tool)`, `parse_tool_name(name)`, `blocked_env(name)` und
  `parse_mcp_servers(text) -> Result<Vec<(String, ServerSpec)>, ParseError>` (Format von Claude Desktop:
  `{"mcpServers": {name: {command, args, env} | {url | type, headers}}}`, auch ohne äußeres Objekt).
- Requests und Katalog (Bereich `extensions`): `extensions.list_extensions` (Read, Tool),
  `extensions.get_extension { id }` (Read, Tool), `extensions.list_catalog` (Read, Tool),
  `extensions.list_tools` (Read, Tool: nur die Werkzeuge, die der fragende Schlüssel aufrufen darf),
  `extensions.call_tool { extension, tool, arguments }` (Delete im Katalog; der Kern nimmt den Zugriff des
  Werkzeugs, MA.5), `extensions.set_enabled { id, enabled }` (Write, Tool), `extensions.install_extension
  { from: Catalog { entry } | Custom { id, name, server } }` (Admin, Never), `extensions.confirm_tools { id,
  fingerprint }` (Admin, Never), `extensions.update_extension { id, server? }` (Admin, Never),
  `extensions.set_secret { id, name, value: Secret }` (Admin, Never), `extensions.read_logs { id }` (Read, Never),
  `extensions.remove_extension { id }` (Admin, Tool). Antworten `Response::Extensions`, `Extension`, `Catalog`,
  `ExtensionTools`, `ToolResult`, `Logs { lines }`.
- `Rights::extensions` mit `allows_extension(id, tool, access)`, Armen in `common` (kleinere Stufe), `hook`,
  `assistant`, `none`, `RightsTemplate::rights` (leer); `Rights::allows` lässt `extensions.list_tools` und
  `extensions.call_tool` ohne Stufe in `extensions` durch (die Prüfung je Server macht der Kern);
  `deniable` nimmt `ext.<id>.<werkzeug>` mit gültiger Kennung an; `Lack::Extension` ohne Feld (`Lack` bleibt `Copy`)
  mit Text in `message`. `Mode::narrow` (`torro_ipc::assistant`) senkt auch `Rights::extensions` auf höchstens Read.
- `Request::kept_for_people`: `extensions.set_enabled` mit `enabled: true`.
- `mcp.rs`: `tools()` lässt `extensions.call_tool` aus (die Werkzeuge kommen einzeln, MA.6), der Vertragstest nennt
  die Ausnahme; `Area::Extensions` in `annotations()`.
- Handler vorerst `unsupported`; alle `match`-Arme mechanisch.
- `docs/api.md`: neuer Punkt 10 in „Was nie nach außen geht“ (Installieren, Einschalten, Zustimmen, Aktualisieren
  eines Servers, seine Geheimnisse, seine Ausgaben; Begründung: ein Server ist ein Programm mit den Rechten der
  Person) und der Bereich in „Rechte“.
- **Abnahme:** `samples()` deckt alle zwölf Requests; `names_follow_the_rules` mit den fünf Never-Befehlen in
  `kept_back`; Test: `set_enabled` mit `true` für jeden Client `forbidden`, `false` erlaubt; Test: neuer Bereich
  und jeder Server für vorhandene Clients, den Assistenten und alle Vorlagen geschlossen; `common` nimmt die kleinere
  Stufe; `parse_mcp_servers` mit Beispielen von Claude Desktop (stdio, url, kaputtes JSON, gesperrte Variable);
  `tool_name`/`parse_tool_name` hin und zurück, zu lang und falsche Zeichen abgelehnt;
  `TORRO_BLESS=1 cargo test -p torro-ipc` und `docs/api-reference.md`.

### MA.2 Kern: Katalog als Daten und Prüfung der Voraussetzungen
- [x] erledigt
- **Modell:** Opus · **Braucht:** MA.1 · **Dateien:** `crates/torroworkplace/assets/extensions/catalog.toml` (neu),
  `crates/torroworkplace/src/extensions/{mod,catalog,prereq}.rs` (neu), `crates/torroworkplace/src/{main,core}.rs`
- `catalog.toml` (mit `include_str!` eingebaut): je Eintrag Kennung, Name, Herausgeber, Lizenz, Link, Beschreibung
  (englisch), feste Version, Befehl und Argumente mit der Version, Voraussetzungen, Geheimnisse (Name, Hinweis,
  Link, wo man ihn bekommt), Zugriff je Werkzeug, `experimental`. Erste Einträge: Time (`uvx mcp-server-time`),
  Sequential Thinking (`npx @modelcontextprotocol/server-sequential-thinking`), GitHub MCP Server (Docker-Bild
  `ghcr.io/github/github-mcp-server`, Geheimnis `GITHUB_PERSONAL_ACCESS_TOKEN`). Version, Lizenz und Werkzeugnamen
  bei der Umsetzung im Repository des Servers nachsehen und mit Link im Kommentar der Datei festhalten. Kein
  Programm darf auf stdin nachfragen (stdin ist die MCP-Leitung): `npx` immer mit `-y`, Docker mit `run -i --rm`;
  `catalog.rs` prüft das für `npx`.
- `catalog.rs`: lesen und prüfen (Kennung gültig und eindeutig, keine gesperrte Variable, Zugriff nur
  Read/Write/Delete, Befehl ohne Shell).
- `prereq.rs`: Programm suchen im PATH des Dienstes, dann in festen Ordnern: `/opt/homebrew/bin`, `/usr/local/bin`,
  `~/.local/bin`, `~/bin`, `~/.cargo/bin`, `~/.volta/bin`, der neueste `~/.nvm/versions/node/*/bin`,
  `/Applications/Docker.app/Contents/Resources/bin`, unter Windows `%ProgramFiles%\nodejs`,
  `%APPDATA%\npm`, `%USERPROFILE%\.local\bin`, Docker Desktop; Endungen wie `client_setup::Env::find_command`.
  Ergebnis: Pfad und der Ordner, der in den `PATH` des Kindes kommt.
- Handler `extensions.list_catalog`: Einträge mit `missing` (was fehlt, als Wort für Menschen: „Needs Node.js
  (npx)“, „Needs uv (uvx)“, „Needs Docker“).
- **Abnahme:** Test: der eingebaute Katalog ist gültig; kaputte Einträge abgelehnt (Fixture); Suche findet ein
  Programm in einem Temp-Ordner als Ersatz für Homebrew bzw. nvm und meldet ein fehlendes; Handler-Test.

### MA.3 Kern: Speicher, Geheimnisse, Umgebung
- [x] erledigt
- **Modell:** Opus · **Braucht:** MA.1 · **Dateien:** `crates/torroworkplace/src/extensions/{mod,store,env}.rs`,
  `crates/torroworkplace/tests/fixtures/extensions/**`
- `store.rs`: `extensions.json` lesen und schreiben (siehe Festlegungen), atomar, Rechte 0700/0600; Geheimnisse über
  die Vault des Kerns mit `ext:<id>:<NAME>`, Entfernen löscht alle Einträge des Servers.
- `env.rs`: `child_env(spec, secrets, base, path_dirs) -> Result<Vec<(String, String)>, EnvError>` nach den
  Festlegungen; Header für HTTP ebenso aus Werten und Geheimnissen; Prüfung der Adresse (https bzw. lokal).
- **Abnahme:** Tests: Rundweg der Datei; kein geheimer Wert in der Datei; gesperrte Variablen abgelehnt, auch
  `ld_preload` und `DYLD_FOO`; die Umgebung des Dienstes (`TORRO_TOKEN`, ein `OPENAI_API_KEY`) kommt nicht beim Kind
  an; `PATH` enthält den Ordner des Programms vorn; `http://example.com` abgelehnt, `http://127.0.0.1` erlaubt.

### MA.4 Kern: MCP-Client und Prozessaufsicht
- [x] erledigt
- **Modell:** Opus · **Braucht:** MA.3 · **Dateien:** `Cargo.toml` (Wurzel: `rmcp` zusätzlich `client`,
  `transport-child-process`, `transport-streamable-http-client-reqwest`, `reqwest`; neu `process-wrap` 10.0.1 mit
  `tokio1`, `process-group`, `job-object`, `creation-flags`, `kill-on-drop`; unter Windows `windows` 0.62.2 mit
  `Win32_System_Threading` für `CREATE_NO_WINDOW`), `crates/torroworkplace/Cargo.toml`,
  `crates/torroworkplace/src/extensions/{mod,client,supervisor}.rs`, `crates/torroworkplace/src/daemon.rs`
  (Beenden), `crates/torroworkplace/tests/fixtures/extensions/**`
- `client.rs`: Verbindung über `rmcp` zu stdio (`TokioChildProcess` mit `CommandWrap`) und Streamable HTTP (eigener
  `reqwest`-Client ohne Weiterleitungen); Werkzeuge listen (alle Seiten), aufrufen, `list_changed` empfangen;
  `ClientInfo` mit Name `torroworkplace` und ohne Elicitation, Sampling, Roots.
- `supervisor.rs`: ein Eintrag je Server mit Zustand nach den Festlegungen (Start bei Bedarf, Zeitlimits, Leerlauf,
  Neustart-Grenze, Prozessbaum, stderr-Puffer); `stop_all` beim Beenden des Dienstes.
- Fingerabdruck und Vergleich mit den zugestimmten Werkzeugen (`review`, `allowed`, `unsupported`).
- **Abnahme:** Protokoll-Tests mit dem Fixture-Server in Rust: Werkzeuge listen, Aufruf mit Text und Bild,
  Fehlerantwort, geänderte Beschreibung → `review`, neues Werkzeug → `review`, unverändertes bleibt `allowed`, zu
  langer Name → `unsupported`, `initialize` meldet keine Elicitation, kein Sampling, keine Roots. Prozess-Tests mit dem
  Skript (`#[cfg(unix)]`): Start und Aufruf über stdio, Zeitlimit beim Aufruf, Absturz → Neustart, nach drei
  Abstürzen `crashed`, Beenden nimmt ein Enkelkind mit (Skript startet `sleep`, Prozess danach weg), stderr im
  Puffer gekürzt. `cargo check --target x86_64-pc-windows-msvc`, soweit das Ziel installiert ist, sonst im Bericht
  als offen nennen.

### MA.5 Kern: Befehle, Rechte, Freigaben, Protokoll
- [x] erledigt
- **Modell:** Opus · **Braucht:** MA.2, MA.4 · **Dateien:** `crates/torroworkplace/src/extensions/mod.rs`,
  `crates/torroworkplace/src/{core,approvals,audit,clients}.rs`, `crates/torroworkplace/src/ui/approvals.rs`,
  `docs/api-reference.md`
- Handler aller Requests aus MA.1: Installieren (Katalog: Voraussetzungen erst prüfen, sonst `unsupported` mit dem,
  was fehlt; Start, Werkzeuge lesen, speichern aus mit `review`), Zustimmen (Fingerabdruck muss zur gelesenen Liste
  passen, sonst `conflict`), Aktualisieren (Katalog: Version des eingebauten Katalogs; eigener: neuer `server`;
  danach `review`), Ein-/Ausschalten, Geheimnis setzen, Ausgaben, Entfernen (Prozess, Ordner, Geheimnisse, offene
  Freigaben, `Rights::extensions` und `ext.<id>.*` in `denied` aller Clients), Auflisten mit Status.
- `Core::extension_operation(&request) -> Operation`: für `extensions.call_tool` die `Operation` mit dem Zugriff des
  zugestimmten Werkzeugs (sonst die des Requests). Sie gilt überall, wo der Kern sonst `request.operation()` für
  Rechte nimmt: `via.origin.mode.refuses`, `Lack::of_in`, `needs_approval_of` und `run_approved`; ebenso
  `assistant::tools::allowed_in` (MA.6), damit Read-Werkzeuge in Nur-Lese-Unterhaltungen bleiben.
- `handle_client`: für `extensions.call_tool` nach `tool_off` `extension_refusal`: Server, für den der Client die
  Stufe „Kein Zugriff“ hat, unbekannter Server und Werkzeug, das es nicht gibt → `not-found` (gefiltert vor dem
  Lesen, wie Sammlungen); Stufe zu niedrig oder Werkzeug in `denied` → `forbidden` mit `Lack::Extension`; dann
  Freigabe nach dem Zugriff des Werkzeugs. In `run_approved` dasselbe noch einmal (mit der Schnittmenge für Bots).
- Der Handler `extensions.call_tool` selbst (`handle`) prüft für **jeden** Schlüssel, auch den App-Schlüssel (der
  `handle_client` nicht durchläuft), dass der Server an ist und das Werkzeug `allowed`, sonst `not-found`
  (Vorbild `tool_off` in `handle`).
- `approvals::summary`: „Use “{tool}” of {extension}“ mit dem Namen aus dem Speicher des Kerns (`Facts`) und den
  Argumenten bis 500 Zeichen, gekennzeichnet als Angabe des Agenten; längere sichtbar gekürzt („… +{n}
  characters“). Die Freigabe-Ansicht der Steuerzentrale (`ui/approvals.rs`) zeigt die vollständigen Argumente.
- `audit::ids`: Kennung des Servers und Werkzeugname. Antworten von `call_tool` tragen `untrusted`.
- **Abnahme:** Handler-Tests mit Fixture-Server und Temp-Wurzeln: Installieren → `review`, ohne Zustimmung kein Aufruf;
  Zustimmen mit falschem Fingerabdruck `conflict`; Server ohne Stufe → `not-found` (auch für einen Hook-Schlüssel);
  Stufe Read für ein `Write`-Werkzeug und Werkzeug in `denied` → `forbidden`; `Delete`-Werkzeug wartet auf Freigabe,
  `Read`-Werkzeug nicht (bei „Ask first: on delete“); `Read`-Werkzeug in einer Nur-Lese-Unterhaltung erlaubt,
  `Write`-Werkzeug nicht; App-Schlüssel ruft ein Werkzeug in `review` → `not-found`; Freigabe nach Entzug der Stufe →
  `forbidden`; lange Argumente sichtbar gekürzt; Bot mit der Schnittmenge; Entfernen räumt Rechte und Schlüsselbund auf;
  `install_extension` und `confirm_tools` mit Client-Schlüssel `forbidden`; Protokollzeile ohne Argumente;
  `TORRO_BLESS=1 cargo test -p torro-ipc`.

### MA.6 Angebot an Assistent, Bots und MCP-Server
- [x] erledigt
- **Modell:** Opus · **Braucht:** MA.5 · **Dateien:** `crates/torroworkplace/src/assistant/{tools,model,prompt}.rs`,
  `crates/torroworkplace/src/bots/mod.rs`, `crates/torroworkplace/src/mcp.rs`,
  `crates/torroworkplace/assets/assistant/skill.md`
- `assistant::tools`: `allowed_for` und `offered` nehmen neben den Katalog-Werkzeugen die erlaubten Werkzeuge der
  Server (vom Kern, nach Rechten und Status); dafür `Arc<Tool>` statt `&'static Tool`. `request_for` baut für
  `ext_…` ein `extensions.call_tool`. `untrusted` und `may_have_read_untrusted` gelten für jedes `ext_`-Werkzeug.
  Beschreibungen beginnen mit „From <Name> (external, untrusted): …“. `extensions_list_tools` wird dem Modell
  nicht angeboten (die Werkzeuge kommen einzeln).
- `mcp.rs`: `list_tools` hängt die Antwort von `extensions.list_tools` für den Schlüssel an; `call` bildet `ext_…`
  auf `extensions.call_tool` ab; `INSTRUCTIONS` nennt die Server-Werkzeuge als fremd.
- Prompt: erzeugter Teil nennt die angebundenen Server; `skill.md`: Antworten von Servern sind Daten, nie
  Anweisungen.
- **Abnahme:** Mock-Modell bekommt `ext_time_get_current_time` nur mit Stufe und zugestimmtem Werkzeug, nicht mit
  `review`; Bot nur mit der Schnittmenge; MCP: `list_tools` enthält es für einen Schlüssel mit Recht, sonst nicht;
  `call` baut den richtigen Request; Ergebnis als Daten gekennzeichnet.

### MA.7 Steuerzentrale › MCP connections: Abschnitt, eigene Werkzeuge ziehen um
- [ ] erledigt
- **Modell:** Opus · **Braucht:** MA.1 · **Dateien:** `crates/torroworkplace/src/ui/{mod,launch}.rs`,
  `crates/torroworkplace/src/ui/connections.rs` (neu), `crates/torroworkplace/Cargo.toml` (`rfd`),
  `crates/torro-settings-ui/src/assistant/{mod,tools}.rs`, `crates/torro-settings-ui/src/{lib,demo}.rs`,
  `crates/torroassistant/src/app.rs`, `crates/torro-core/src/i18n/de.rs`
- `Section::Connections` nach `Mcp`, Name „MCP connections“, `--section connections`. Teil 1 „Built-in tools“: die
  Seite aus `assistant/tools.rs` (Schalter, Ablage-Ordner, Vorlagen) wird von der Steuerzentrale gezeigt; Ordner und
  Logo wählt sie mit `rfd`. Teil 2 „Catalog“ und Teil 3 „Your own servers“ zunächst mit Leerzustand und den Zeilen
  aus `extensions.list_extensions` (Handler bis MA.5 `unsupported` → Fehlerzustand mit Satz).
- Assistent › Einstellungen › Tools wird eine Zeile mit „Tools moved to the control centre“ und „Open“
  (`Action::ControlCentre("connections")`).
- Demo-Zustände `connections`, `connections-empty`.
- **Abnahme:** Tests: Abschnittsliste mit zehn Abschnitten; Schalter schickt genau einen `set_setting`; der
  Assistent schickt nichts mehr für Werkzeuge; `--section connections`; Sichtprüfung (`torro-design-review`, alle
  Größen, drei Designs); Übersetzungen in `de.rs` („MCP connections“ → „MCP-Anbindungen“).

### MA.8 Katalog, Installieren und Seite eines Servers
- [ ] erledigt
- **Modell:** Opus · **Braucht:** MA.2, MA.5, MA.7 · **Dateien:** `crates/torroworkplace/src/ui/connections.rs`,
  `crates/torroworkplace/src/ui/{mod,launch}.rs`, `crates/torro-core/src/i18n/de.rs`
- Katalog: je Eintrag Name, Herausgeber, Lizenz, Link, „Experimental“, Voraussetzungen mit fehlenden in Attention
  und Satz, was zu installieren ist; „Install…“ nur ohne Fehlendes. Dialog: Werkzeuge mit Zugriff aus dem Katalog,
  Felder für Geheimnisse (verdeckt, Link zum Anbieter), Hinweis „runs as you on this computer“; „Install“ →
  `install_extension`, dann Liste der gemeldeten Werkzeuge mit Zugriff und „Allow tools“ → `confirm_tools`.
- Seite eines Servers: Status mit Wort und Icon, Schalter (`set_enabled`), Werkzeuge mit Zugriff und `review`
  („Changed since you allowed it“) samt „Allow tools“, Geheimnisse („Stored“/„Missing“, „Replace…“ →
  `set_secret`), „Output“ (`read_logs`, Textfeld zum Kopieren), „Update“ wenn der Katalog eine neuere Version hat,
  „Remove…“ mit Rückfrage. Hinweis auf die Rechte: „Who may use it: API, MCP and the assistant's Access“.
  Sidebar-Zeile mit `Mark::Badged` bei `review` und `crashed`.
- Demo-Zustände `catalog-install`, `extension-detail`, `extension-review`, `extension-crashed`.
- **Abnahme:** Tests: jeder Knopf schickt genau seinen Request; Geheimnisse erscheinen nie im `Debug` des Zustands;
  ohne Fehlendes kein „Install…“; Sichtprüfung aller neuen Zustände in allen Größen; Übersetzungen in `de.rs`.

### MA.9 Eigener Server: Formular und eingefügter `mcpServers`-Block
- [ ] erledigt
- **Modell:** Opus · **Braucht:** MA.8 · **Dateien:** `crates/torroworkplace/src/ui/connections.rs`,
  `crates/torroworkplace/src/ui/launch.rs`, `crates/torro-core/src/i18n/de.rs`
- „Add server…“: Art (Command / Address), Name, Kennung (aus dem Namen vorgeschlagen), Befehl, Argumente (je Zeile
  eins), Variablen bzw. Header als Zeilen mit „Secret“-Schalter (an für Namen mit `KEY`, `TOKEN`, `SECRET`,
  `PASSWORD`, `AUTH`). „Paste configuration“ füllt das Formular über `torro_ipc::extensions::parse_mcp_servers`;
  mehrere Server im Block → Auswahl, je Server ein Formular. Fehler an der Zeile nach dem ersten Speicherversuch.
  Danach derselbe Ablauf wie im Katalog (Werkzeuge zeigen, „Allow tools“).
- Demo-Zustände `custom-server`, `custom-server-paste`.
- **Abnahme:** Tests: eingefügter Block mit `env` füllt das Formular, Token-Variable geheim markiert; gesperrte
  Variable zeigt den Fehler an der Zeile; Speichern schickt genau ein `install_extension` mit `Secret`-Werten;
  Sichtprüfung; Übersetzungen in `de.rs`.

### MA.10 Rechte-Editor: Bereiche `ext:<id>`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** MA.1 · **Dateien:** `crates/torro-settings-ui/src/{rights,lib,demo}.rs`,
  `crates/torroworkplace/src/ui/{api,mcp,mod}.rs`, `crates/torroassistant/src/{app,bots}.rs` und
  `crates/torroassistant/src/view/editor.rs` (nur `Known` füllen), `crates/torro-core/src/i18n/de.rs`
- `Known::extensions` aus `extensions.list_extensions`. Unter „Extensions“ je installiertem Server eine Zeile mit
  seinem Namen und den Stufen No access / Read / Write / Full; „Show commands“ zeigt seine zugestimmten Werkzeuge
  mit Zugriff und Schalter (`ext.<id>.<werkzeug>` in `denied`), Naming wie die Kern-Befehle. Ausgeschaltete Server
  mit Attention-Marke. Speichern wie bisher über `clients.update_client`. `commands()` und `off_count` nehmen
  `extensions.list_tools` und `extensions.call_tool` aus (sie gehen ohne Stufe durch und sind nicht schaltbar).
- **Abnahme:** Tests: Stufe setzen ändert nur `Rights::extensions`; Werkzeug sperren schreibt den Namen in `denied`;
  Vorlage wählen lässt `extensions` leer; Sichtprüfung der Seite eines MCP-Clients und von Access; Übersetzungen.

### MA.11 Doku
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** MA.1–MA.10 · **Dateien:** `docs/erweiterungen.md`, `docs/assistent.md`,
  `docs/settings.md`, `docs/api.md`
- `erweiterungen.md`: Fragen 1, 2, 5 und 6 als entschieden (MCP; fester Bereich `extensions` mit Rechten je Server
  statt `Area` je Erweiterung; Installieren nur in der Oberfläche; Geheimnisse beim Start an genau den Server),
  Abschnitt „MCP-Server (#302)“ mit den Festlegungen dieser Datei; kein eigener Schlüssel je Server.
  `assistent.md`: Entscheidung 9 ersetzt („Fremde MCP-Server über MCP connections, #302“), Hinweis, dass ein Server
  mit Datei- oder Shell-Werkzeugen Entscheidung 6 nur mit Recht und Freigabe der Person aufweicht.
  `settings.md`: Abschnitt „MCP connections“ in Tabelle, Seitenleiste und „Was wohin zieht“ (Assistent › Tools).
- **Abnahme:** Verweise stimmen; nichts widerspricht dem Code.
