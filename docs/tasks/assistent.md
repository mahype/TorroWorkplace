# Aufgaben: Assistent (C.1–C.10, Stufe 2: Bots)

Konzept: [assistent.md](../assistent.md), KI-Konten: [ki.md](../ki.md). Issue #78. Regeln für die Arbeit:
[CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer einen
Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

> **Stand 2026-10-13: freigegeben.** Alle offenen Punkte sind entschieden (Abschnitt „Entscheidungen“ im Konzept).
> Open First: Antrieb A (KI-Konto mit Ollama/OpenRouter) zuerst, Antrieb B (Claude Code, Codex) danach.

```
KI.1a ── C.1 ──┬── C.2 ── C.3 ──┬── C.4 (App)
               │                ├── C.5 (MCP)
               │                └── C.6 (Steuerzentrale)
               └── C.7 (Antrieb B: Claude Code, Codex)
C.8 (Skill-Datei und MCP-Prompt) parallel ab C.1
C.9 (Sprache, Weckwort) nach C.4
C.10 (Persönlichkeit, Gedächtnis) nach C.3
KI.1b ── KI.1c ── KI.1d (Anbieter Anthropic, Issue #218) parallel ab KI.1a
```

### KI.1a Kern: KI-Konten, OpenAI-kompatibel (vorgezogener Teil von KI.1)
- [x] erledigt
- **Modell:** Opus · **Dateien:** `crates/torro-core/src/ai/**` (neu), `crates/torroworkplace/src/account/**`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/{core,mcp}.rs`, `crates/torroworkplace/src/ui/**`
  (Accounts › Add › AI, entschieden 2026-10-13), `docs/api-reference.md`, `Cargo.toml` (Wurzel)
- `AccountKind::Ai`, Anbieter-Trait mit **einer** Umsetzung „OpenAI-kompatibel“ (Ollama, LM Studio, vLLM, OpenRouter).
  Chat mit Werkzeugen (Function Calling) und Streaming (SSE), Modell-Liste mit Fähigkeit „tools“ (Ollama `/api/show`,
  OpenRouter `supported_parameters`), Filter „open weights“ für OpenRouter (Standard an, abschaltbar). Lokale Server auf `localhost` finden.
- Anthropic-Protokoll, Zuordnung je Funktion und Zähler bleiben in KI.1.
- **Oberfläche:** Accounts › Add › AI (Adresse bzw. Schlüssel), lokale Anbieter oben, dann OpenRouter.
- **API:** `Request::ConnectAi` · `accounts.connect_ai` · Admin, nur App-Schlüssel · `Request::AiModels` ·
  `ai.list_models` · Read
- **MCP:** `accounts_connect_ai` **Never** (Punkt 5: Schlüssel nur über die Oberfläche) · `ai_list_models`
- **Abnahme:** Tests mit `httpmock` für Ollama und OpenRouter (Modelle, Werkzeug-Aufruf, Streaming, Fehler);
  Schlüssel erscheint in keiner Antwort und keinem Protokoll.

### KI.1b Anbieter Anthropic im Katalog und sein Format auf der Leitung (Issue #218)
- [x] erledigt (nachgetragen: `crates/torro-ipc/src/api.rs` — der Test
  `the_ai_key_is_a_write_only_string_and_optional` listet die Anbieterwerte des Schemas von Hand und wäre mit dem
  neuen Wert rot geblieben; `anthropic` steht dort wie im Katalog hinter `openrouter`)
- **Modell:** Opus · **Braucht:** KI.1a · **Dateien:** `crates/torro-core/src/ai/mod.rs`,
  `crates/torro-core/src/ai/anthropic.rs` (neu), `docs/einrichtung.md`, `docs/api-reference.md`
- `AiProvider::Anthropic`, serde und `id()` `anthropic`, `name()` `Anthropic`, `default_url()`
  `https://api.anthropic.com/v1`, `key_url()` `https://platform.claude.com/settings/keys` (geprüft am 2026-10-19;
  die alte Adresse unter `console.anthropic.com` leitet dorthin um), `seat()` `Seat::Us`,
  `china_url()` und `open_weights()` wie die anderen Anbieter der USA: keine, nein. In `ALL` und `CLOUD` steht
  Anthropic vor `OpenAi` (Reihenfolge des Katalogs in `docs/einrichtung.md`), die Längen der Felder wachsen auf 13
  bzw. 10.
- `ai::anthropic`: das Format der Messages-API als reine Funktionen über JSON und Bytes, ohne Netz, wie
  `ai::openai` – `chat_body`, `parse_reply`, `StreamDecoder` (`push`/`finish`), `parse_models`. Kopfzeilen und
  Pfade gehören dem Sender (KI.1c), hier steht nur, was in den Rumpf geht und was aus der Antwort kommt.
- **Rumpf** (`POST <base>/messages`): `model`, `max_tokens`, `stream`, `messages`, `tools`
  (`{name, description, input_schema}`). `Role::System` wandert aus `messages` in das Feld `system` (mehrere mit
  einer Leerzeile verbunden). Ein Werkzeug-Aufruf ist ein `tool_use`-Block der Antwort
  (`{type, id, name, input}`), `input` ein Objekt: beim Zurückschicken `ToolCall::arguments` als JSON lesen und
  nur ein Objekt übernehmen, sonst `{}`. `Role::Tool` wird eine `user`-Nachricht mit `tool_result`-Blöcken
  (`tool_use_id`, `content`). Aufeinanderfolgende Nachrichten derselben Rolle werden zu **einer** zusammengefasst,
  damit mehrere Werkzeug-Ergebnisse einer Runde in einer Nachricht stehen.
- Kein leerer Block geht hinaus, Anthropic lehnt ihn ab: Eine Assistant-Nachricht, die nur Werkzeuge aufruft
  (`ChatMessage::content` leer), bekommt keinen `text`-Block, und ein Werkzeug-Ergebnis ohne Text bekommt einen
  kurzen Platzhalter statt eines leeren `content`. Die erste Nachricht muss `user` sein; was davor stünde, fällt
  weg (nach dem Herausziehen des Systemtexts ist das der normale Fall).
- `max_tokens` ist Pflicht, und keine Stelle der Suite nennt heute eine Zahl: ohne `ChatRequest::max_tokens` gilt
  die Konstante `DEFAULT_MAX_TOKENS: u32 = 32_768` (in der Planung entschieden). Ihr Doc-Kommentar nennt beide
  Gründe: Die Denk-Token zählen auf dieselbe Grenze, und sie muss mit Abstand unter die kleinste Ausgabegrenze der
  angebotenen Modelle passen (64.000), damit ein neues Modell mit kleinerer Grenze keinen Fehler auslöst. Die
  Grenze ist eine Decke, keine Reservierung: Erreicht die Antwort sie, ist das `Finish::Length`.
- **Denkstufe:** mit `ChatRequest::effort` kommen `output_config: {"effort": "<Effort::id()>"}` und
  `thinking: {"type": "adaptive"}` in den Rumpf, ohne Denkstufe keines von beiden. Nicht `thinking.budget_tokens`:
  das lehnen die aktuellen Modelle mit HTTP 400 ab.
- **Strom** (SSE): Zeilen `data:` mit JSON, kein `[DONE]`; `event:`-Zeilen, Leerzeilen und `ping` werden
  übersprungen. Verzweigt über `type` des JSON: `message_start` (`message.usage.input_tokens`),
  `content_block_start` (`index`, bei `tool_use` `id` und `name`), `content_block_delta` (`text_delta.text` an
  `on_text`, `input_json_delta.partial_json` an den Aufruf des Blocks), `message_delta`
  (`delta.stop_reason`, `usage.output_tokens`), `message_stop`, `error` (Fehler wie im ganzen Rumpf).
  `thinking`-Blöcke und `thinking_delta` gehen nicht in `ChatReply::text`. `push` nimmt beliebige Stücke, auch
  mitten in einer Zeile oder einem UTF-8-Zeichen; ein Strom ohne jedes Stück ist `AiError::Format`.
- `stop_reason`: `end_turn` und `stop_sequence` `Finish::Stop`, `tool_use` `Finish::ToolCalls`, `max_tokens`
  `Finish::Length`, alles andere `Finish::Other`.
- **Modelle** (`GET <base>/models`, Antwort `{"data": [...]}`): `id`, `name` aus `display_name` (sonst die id),
  `context` aus `max_input_tokens`, `reasoning` aus `capabilities.effort.supported` (`None`, wenn das Feld fehlt),
  `tools` fest `Some(true)` – alle Claude-Modelle der Messages-API rufen Werkzeuge auf –, `open_weights` und
  `remote` bleiben `None` wie bei jedem anderen Cloud-Anbieter. `open_weights` darf **nicht** `Some(false)` sein,
  sonst filtern `open_weights_only`, `suggest::pick_model` und `ModelAccess::automatic` jedes Claude-Modell weg.
- Fehler: `openai::error_of` und `openai::scrub` werden weiterverwendet, sie lesen `/error/message` und sind von
  keinem Protokoll abhängig; der Doc-Kommentar beider sagt das dann.
- Doku: der Modulkopf von `ai/mod.rs` („One protocol for now“, „follows in task KI.1“), die Zeile zu Anthropic im
  Anbieter-Katalog von `docs/einrichtung.md` („Protokoll kommt mit KI.1“) und Entscheidung 2 desselben Dokuments
  („Anthropic mit KI.1“) stimmen danach nicht mehr und werden mitgeschrieben.
- `AiProvider` steht als schemars-Enum im Schema von `accounts.connect_ai`, deshalb wächst die erzeugte Referenz
  schon mit dieser Aufgabe (`docs/api-reference.md`, Zeile der Werte von `provider`): `TORRO_BLESS=1 cargo test -p
  torro-ipc` laufen lassen und die Datei mit committen, sonst bleibt `cargo test --workspace` rot. KI.1c schreibt
  sie danach noch einmal, weil sie die Beschreibungen ändert.
- Nachzuziehen im Crate: `name`, `id`, `default_url`, `key_url` und `seat` zählen die Anbieter vollständig auf
  (`seat`: Anthropic in den Zweig `Seat::Us`), ebenso `openai::chat_body_for` – dort kommt ein Zweig
  `AiProvider::Anthropic => {}` mit dem Grund dazu, dass Anthropic sein eigenes Format hat
  (`ai::anthropic::chat_body`) und nie durch diese Funktion läuft. `china_url` und `open_weights` haben einen
  Auffang-Zweig.
- Nachzuziehen außerhalb: nachgesehen, nichts – `place_of` hat einen Auffang-Zweig, und die Oberflächen lesen den
  Katalog (KI.1d nur dort, wo eine Liste von Hand steht).
- **Abnahme:** Tests in `ai/anthropic.rs` über Bytes: Rumpf mit System, Werkzeugen und Werkzeug-Ergebnis;
  Denkstufe als `output_config.effort` samt `adaptive`; Strom in beliebigen Stücken gibt Text in Reihenfolge und
  einen vollständigen Werkzeug-Aufruf; `parse_models` liest Fähigkeiten und lässt `open_weights` offen; leerer
  Strom und kaputtes JSON sind `AiError::Format`. Der Test aus `ai/mod.rs`
  (`every_provider_is_written_by_its_id_…`) deckt Anthropic mit. Dazu `TORRO_BLESS=1 cargo test -p torro-ipc`, dann
  `cargo fmt --all`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `TORRO_LANG=de cargo test --workspace`.

### KI.1c Anthropic senden: zweite Umsetzung von `Provider` (Issue #218)
- [x] erledigt (der Helfer heißt `Wire`, sein Schlüssel-Weg `Auth`; `Wire::chunks` nimmt ein `Send`-Closure,
  sonst wäre das Future von `chat` nicht mehr `Send`)
- **Modell:** Opus · **Braucht:** KI.1b · **Dateien:** `crates/torroworkplace/src/account/ai.rs`,
  `crates/torroworkplace/src/assistant/data_rule.rs` (nur Test), `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torro-core/src/ai/mod.rs` (Doku des Traits), `docs/api-reference.md`
- Die Leitungs-Arbeit, die heute in `OpenAiCompatible` steckt (`request`, `authorized`, `send`, `body`,
  `scrubbed`, die Schleife über `response.chunk()`), zieht in einen privaten Helfer desselben Moduls, den beide
  Umsetzungen halten; wie der Schlüssel in die Kopfzeile kommt, ist sein Parameter. Nur so steht `scrub`,
  „kein Redirect“ und „Kopfzeile vertraulich“ genau einmal im Code. Die öffentliche Oberfläche von
  `OpenAiCompatible` bleibt, wie sie ist.
- `struct Anthropic` mit `check()`, `list()` und `impl Provider`: `models()` ist `GET <base>/models` mit
  `?limit=1000` (eine Anfrage, die Grenze liegt über dem ganzen Katalog), `chat()` ist
  `POST <base>/messages` mit `stream: true`, gelesen als SSE mit `anthropic::StreamDecoder`; antwortet der Server
  doch ganz (Inhaltstyp nicht `text/event-stream`), dann über `anthropic::parse_reply`, wie es
  `OpenAiCompatible::stream` schon macht. `check()` ist `list()`: `/v1/models` braucht den Schlüssel, ein
  abgelehnter ist `auth`.
- **Kopfzeilen:** `x-api-key: <key>` und `anthropic-version: 2023-06-01`. Anthropic nimmt den Schlüssel dort, nicht
  in `Authorization`; der Wert wird als vertraulich markiert (`HeaderValue::set_sensitive`), wie `bearer_auth` es
  für den anderen Weg tut, damit er in keinem `Debug` und keinem Protokoll steht. Drei Doc-Kommentare behaupten
  heute `Authorization` oder „Anthropic kommt später“ und werden darauf geändert, dass der Schlüssel nur in die
  Schlüssel-Kopfzeile des Protokolls geht, nur an den Server des Kontos: der `Provider`-Trait in
  `torro-core/src/ai/mod.rs` („only in the `Authorization` header“, dazu „later by Anthropic's“ im Satz darüber),
  der Modulkopf von `account/ai.rs` und die Doku von `OpenAiCompatible::authorized`, die mit der Methode in den
  Helfer zieht. `ai::key_may_travel` gilt unverändert.
- `AiAccounts`: `client()` liefert beide Umsetzungen hinter einem Typ (ein `enum` des Moduls mit `check()` und
  `impl Provider`), `provider()` kastet ihn wie bisher, `connect()` prüft damit auch ein Anthropic-Konto.
  `find_local()` bleibt bei `OpenAiCompatible` – dort stehen nur Server dieses Rechners.
- `torro-ipc`: in der Doku von `Request::ConnectAi` kommt `anthropic` in die Liste der Werte von `provider`; der
  Satz, der „die OpenAI-kompatible Anbindung“ und „any other server by listing its models“ behauptet, nennt das
  Anthropic-Protokoll mit, und das Feld `server` darunter („the base address of the server's **OpenAI-compatible**
  API“) ebenso. Die Beschreibung von `accounts.connect_ai` im Katalog (`api.rs`) auch.
  Katalogeintrag, Bereich, Zugriff und `Mcp::Never` bleiben, wie sie sind; `ai.list_models` bleibt `Mcp::Tool`.
  Danach `TORRO_BLESS=1 cargo test -p torro-ipc` und `docs/api-reference.md` mit committen.
- Datenregel: `place_of` braucht keinen neuen Zweig, sein Auffang-Zweig gibt `Place::Remote`, und die Schleife über
  `AiProvider::CLOUD` in `local_servers_are_ollama_lm_studio_and_this_computer` deckt Anthropic von selbst mit,
  sobald es in `CLOUD` steht. Dazu kommt eine Zeile für eine Adresse dieses Rechners, wie die Zeile „a proxy“ für
  OpenRouter.
- Nachzuziehen: nachgesehen – `label()` nennt das Konto am Standardweg „Anthropic“, `Account::ai` und der
  Schlüsselbund brauchen nichts, `assistant.list_engines` zählt KI-Konten ohne Anbieterwissen.
- **Abnahme:** Tests mit `httpmock` in `account/ai.rs`: Modell-Liste mit Fähigkeiten; `connect` legt Konto und
  Schlüssel an und schickt ihn in `x-api-key`, nie in `Authorization`; Werkzeug-Aufruf im Strom; Text in beliebigen
  Stücken mit gezählten Token; 401 ist `auth` und speichert nichts, 429 ist `busy`, ein toter Port ist `offline`;
  kein Fehler und keine Antwort wiederholt den Schlüssel (wie
  `a_refused_key_is_auth_stores_nothing_and_never_shows_the_key`). Agent `torro-api-review`. `cargo fmt --all`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `TORRO_LANG=de cargo test --workspace`.

### KI.1d Anthropic in Accounts › Add › AI (Issue #218)
- [x] erledigt (Sichtprüfung per Screenshot nicht möglich: `shoot.sh` braucht Hyprland, die Sitzung lief auf
  macOS — steht auf der Handprüfliste des PRs)
- **Modell:** Sonnet · **Braucht:** KI.1c · **Dateien:** `crates/torroworkplace/src/ui/setup/pages.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`
- In `pages.rs` steht die Gruppe „Providers“ von Hand: Anthropic kommt dort an die erste Stelle, vor `OpenAi`.
  Name, Sitz („Cloud · US“) und der Knopf „Add key“ kommen aus dem Katalog, die Zeile braucht keinen eigenen Text.
- In `wizard.rs` zeigt `ai_step` die Anbieter aus `AiProvider::CLOUD`, Anthropic steht damit von selbst in „More
  providers“, und `ai_key_step` nimmt Name, Schlüsselseite und Sitz aus dem Katalog. Zu ergänzen ist nur der
  Platzhalter des Schlüsselfelds (`sk-ant-…`, wie `sk-or-v1-…` für OpenRouter) – kein Text für Menschen, keine
  Übersetzung.
- Keine neuen Texte über `tr`, also kein Eintrag in `crates/torro-core/src/i18n/de.rs`: „Cloud · US“, „Add key“
  und der Satz „{provider} runs its models in its own cloud ({seat}). …“ stehen dort schon.
- **Abnahme:** Der Test in `wizard.rs`, der einen Cloud-Anbieter wählt und seinen Schlüssel schickt, deckt
  Anthropic mit (ein Fall mehr, wie `AiProvider::Kimi`). Sichtprüfung mit dem Demo-Stand `wizard-ai`
  (`ui/launch.rs`) in allen Größenklassen, Skill `torro-design`, Agent `torro-design-review`. `cargo fmt --all`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `TORRO_LANG=de cargo test --workspace`.

### C.1 Gerüst: Bereich `assistant`, Requests, Katalog, Speicher, Client-Art
- [x] erledigt (`docs/api-reference.md` gibt es noch nicht, A.9; Test-Antrieb `test` nur in Tests und mit
  `TORRO_TEST_ENGINE=1`; Freigabe-Texte für `assistant.*` in `approvals.rs`, nachgetragen mit Freigabe)
- **Modell:** Opus · **Braucht:** KI.1a · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torroworkplace/src/assistant/**` (neu), `crates/torroworkplace/src/{core,clients}.rs`,
  `docs/api-reference.md`; nachgetragen (Rückfrage): `crates/torroworkplace/src/approvals.rs` (Freigabe-Texte),
  `crates/torro-core/src/settings.rs`, `crates/torroworkplace/src/{settings_service,audit,daemon,pim,main}.rs`,
  `crates/torroworkplace/src/ui/{agents,general}.rs` (Aufbewahrung, `match`-Zweige)
- `Area::Assistant`; Requests und Katalog nach der Tabelle „Drei Wege“ im Konzept: `assistant.list_engines` (Read),
  `assistant.list_conversations` (Read), `assistant.get_conversation { id, after }` (Read),
  `assistant.send_message { conversation?, engine, model?, text, without }` (Write), `assistant.cancel` (Write),
  `assistant.rename_conversation` (Write), `assistant.delete_conversation` (Delete),
  `assistant.open_window { conversation | new | bot }` (Write, startet `torroassistant --chat …` wie `apps.open_app`).
- Client-Art `assistant`; im Bereich `assistant` fest gesperrt bis auf das Gedächtnis aus C.10 (keine Schleife). Bereich
  `assistant` in den Vorlagen anderer Clients aus, auch in *Full access*. **Ein** Client „Assistant“ für alle Antriebe;
  der Antrieb steht im Protokoll. Aufbewahrung 30 Tage (Einstellung), angeheftete Unterhaltungen bleiben
  (`assistant.pin_conversation` · Write · MCP `assistant_pin_conversation`).
- Speicher `<data>/assistant/<id>.jsonl`, 0600; Protokoll ohne Inhalte.
- Handler zunächst mit einem Test-Antrieb (feste Antworten), damit C.4–C.6 parallel laufen können.
- **Abnahme:** Handler-Tests (anlegen, lesen ab `after`, abbrechen, löschen); Rechte-Tabelle deckt den Bereich;
  Vertragstest Tool ↔ Katalog; `TORRO_BLESS=1 cargo test -p torro-ipc`.

### C.2 Antrieb A: Unterhaltung mit KI-Konto und Werkzeugen
- [x] erledigt (Datenregel fest: Mail und Mail-Einträge des Eingangs nur an Ollama, LM Studio oder einen Server auf
  diesem Rechner, KI.1 ersetzt `assistant/data_rule.rs`; Werkzeuge über `Host::run` als Client des Assistenten;
  Systemprompt vorläufig in `assistant/prompt.rs`, C.3 ersetzt ihn)
- **Modell:** Opus · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/assistant/**`; nachgetragen
  (Koordination): `crates/torroworkplace/src/{core,audit}.rs` (`Host` für den Kern, `Core::start` mit `Arc`,
  Protokoll mit Antrieb), `crates/torroworkplace/src/account/ai.rs` (`AiAccounts::server`),
  `crates/torro-ipc/src/assistant.rs` (Doku: wachsende Antwort)
- Schleife: Modell ruft Werkzeug → Kern führt den Request **mit dem Client des Assistenten** aus (Rechte, Sammlungen,
  Freigaben wie bei jedem Client) → Ergebnis gekürzt zurück → bis zur Antwort. Höchstzahl Schritte je Nachricht.
- Werkzeuge nur aus erlaubten und nicht abgeschalteten Bereichen; bei kleinen Modellen zuerst nur lesende; ab einer
  Grenze `find_tools`. `pending` wird im Verlauf als Freigabe-Eintrag abgelegt, der Lauf wartet bzw. setzt nach
  der Entscheidung fort.
- Datenregel je Quelle ([ki.md](../ki.md)): Mail-Inhalte gehen nur an lokale Modelle, solange die Regel das sagt;
  sonst bekommt das Modell „withheld by your data rule“.
- **Abnahme:** Tests mit Mock-Modell: Werkzeug-Aufruf, `forbidden`, `pending` und Fortsetzen, Abbruch, Datenregel,
  zu viele Schritte.

### C.3 Kontext: Skill „Torro Workplace“
- [x] erledigt (Name der Person bleibt leer, solange die Suite keinen kennt; Persönlichkeit hat den Platz vor dem
  Skill, C.10 füllt ihn; nachgetragen: Verdrahtung in `assistant/model.rs`)
- **Modell:** Opus · **Braucht:** C.2 · **Dateien:** `crates/torroworkplace/src/assistant/prompt.rs`,
  `crates/torroworkplace/assets/assistant/skill.md` (neu)
- Fester Teil (englisch) und erzeugter Teil (Datum, Zeitzone, Sprache, erlaubte Bereiche) nach dem Konzept.
- **Abnahme:** Test, dass der Prompt keine Geheimnisse und keine Inhalte aus Sammlungen enthält und nur erlaubte
  Bereiche nennt; Test mit Mock-Modell, dass der Prompt ankommt.

### C.4 App `torroassistant`
- [x] erledigt (Chatfenster-Klasse `torro-assistant-chat` als `app::CHAT` in der App wie `torro-compose` der Inbox,
  kein eigener Eintrag in `apps.list_apps`: Vorlagen merken die Befehlszeile; „Set up access“ öffnet die
  Steuerzentrale über `apps.open_app`, die Seite „Assistant“ kommt mit C.6; `--bot` wird bis Stufe 2 abgelehnt;
  `--size WxH` nur mit `--demo` für Bilder ohne Tiling-Manager; Sichtprüfung auf macOS ohne Hyprland)
- **Modell:** Opus · **Braucht:** C.1 (Test-Antrieb reicht) · **Dateien:** `crates/torroassistant/**` (neu),
  `Cargo.toml` (Wurzel, Mitglied), `crates/torro-core/src/app.rs` (`AppId::ASSISTANT`); nachgetragen (Koordination,
  Listen der Apps): `crates/torro-gui/src/shell.rs` (Name und Symbol im App-Umschalter),
  `scripts/design-check.sh`, `scripts/macos-dev-sign.sh`, `crates/torroworkplace/src/core.rs` (Test, der die Apps
  zählt)
- Seitenleiste, Verlauf, Eingabefeld mit Konto/Modell und „without …“, Werkzeug-Zeilen aufklappbar, Freigabe-Karte
  (`approvals.decide_approval` per Klick), Stop, erster Start mit Auswahl der Rechte (öffnet C.6-Ansicht), `--demo`.
- **Eigene Chatfenster** (Abschnitt „Eigene Chatfenster“ im Konzept): `--chat <id>|new`, Fensterklasse
  `torro-assistant-chat`, „Open in new window“, Doppelklick; Workspace-Vorlagen merken das Argument; gelöschte
  Unterhaltung → neue. Dazu in `crates/torro-core/src/app.rs` die zweite Fensterart, in `apps.list_apps` als eigener
  Eintrag „Assistant chat“ nur, wenn die Vorlagen das brauchen (wie bei `torroinbox --view`).
- **Oberfläche/API:** nur Requests aus C.1 und vorhandene; **MCP:** keine neuen Tools.
- **Abnahme:** `torro-design-review` in allen Größenklassen; Texte über `tr()`, Übersetzungen im Bericht.

### C.5 MCP-Tools `assistant_*`
- [x] erledigt
- **Modell:** Haiku · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Je Katalogeintrag ein Tool; `assistant_send_message` wartet bis zur Antwort oder 60 s, dann Stand mit `after`.
- **Abnahme:** Vertragstest; Test, dass ein Client der Art `assistant` `forbidden` bekommt.

### C.6 Steuerzentrale › Assistant
- [x] erledigt (direkt erreichbar mit `torroworkplace ui --section assistant`; kein Standard-Antrieb als
  Einstellung, dafür fehlt ein `SettingKey` in `torro-ipc` – die Seite zeigt die Antriebe, ein neuer Chat nimmt
  den zuletzt benutzten; der Assistent steht in Agents & API mit Verweis auf diese Seite)
- **Modell:** Opus · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/ui/**`
- Seite „Assistant“: Standard-Antrieb, Aufbewahrung, „What the assistant may use“ (dieselbe Rechte-Ansicht wie bei
  Clients, Vorlagen, Sammlungen). Erreichbar auch aus dem Hinweis bei `forbidden` im Chat.
- **API:** `settings.set_setting` (gibt es), `clients.update_client` (gibt es) · **MCP:** `settings_set_setting`;
  Rechte **Never** (Punkt 2).
- **Abnahme:** `torro-design-review`; Änderung wirkt in einer laufenden Unterhaltung ab der nächsten Anfrage.

### C.7 Antrieb B: Claude Code und Codex
- [x] erledigt (nur Claude Code: Codex hat keine Positivliste für Werkzeuge, „Dateien und Shell immer aus“ lässt sich
  dort nicht zusichern; Schlüssel je Lauf geliehen, nur im Speicher und in der Umgebung des Agenten; Mail nie an
  Agenten (Datenregel); Verlauf wird je Nachricht mitgeschickt, keine Sitzung beim Agenten; nachgetragen
  (Koordination): `crates/torroworkplace/src/{clients,core}.rs` (geliehene Schlüssel, Protokoll mit Antrieb),
  `crates/torro-ipc/src/assistant.rs` (`EngineKind::Agent`))
- **Modell:** Opus · **Braucht:** C.2 · **Dateien:** `crates/torroworkplace/src/assistant/engines/**`
- Claude Code mit `stream-json` in beide Richtungen, eigene `--mcp-config` mit `--strict-mcp-config`, nur
  `mcp__torroworkplace__*`, Schlüssel des Assistenten-Clients in `TORRO_TOKEN`, Skill über `--append-system-prompt`.
  Codex mit `codex exec --json`. Nicht installiert → Antrieb fehlt in `list_engines`.
- **Abnahme:** Tests mit einem Fake-Programm, das den Stream nachspielt; kein Test braucht einen echten Agenten.

### C.8 Skill-Datei und MCP-Prompt für fremde Agenten
- [x] erledigt
- **Modell:** Haiku · **Braucht:** C.3 (Text) · **Dateien:** `crates/torroworkplace/src/{mcp,client_setup}.rs`
- Der feste Teil aus C.3 als MCP-Prompt `torro_workplace`; „Connect agent“ legt ihn für Claude Code zusätzlich als
  Skill ab (zusammengeführt, wie die übrigen Einträge, und beim Trennen wieder entfernt).
- **Abnahme:** Test, dass der Prompt über MCP kommt; Setup-Test mit Temp-Home.

### C.9 Sprache: Weckwort, Spracherkennung, Sprachausgabe (lokal)
- [x] erledigt (Weckwort, Aufnahme, Spracherkennung und Sprachausgabe als **externe lokale Programme**, zur Laufzeit
  gesucht: openWakeWord mit `hey_torro.onnx`, sonst Weckwort über whisper.cpp; `parec`/`arecord`/SoX/`ffmpeg`;
  `whisper-cli`; Piper mit `paplay`/`afplay`/PowerShell – keine neuen Systembibliotheken, nichts wird geladen.
  Neu: `settings.voice_status` (Read, MCP) und `settings.test_voice` (Admin, Never, Punkt 6); Einschalten nur per
  App-Schlüssel über `Rights::allows_request`. Globales Kürzel fehlt (bräuchte nativen Code); Stummschalten im Tray.)
- **Modell:** Opus · **Braucht:** C.4 · **Dateien:** `crates/torroassistant/**` (`src/voice/**`, `--listen`,
  `--voice-status`, `--voice-test`, `tests/fixtures/*.wav`), `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torro-core/src/settings.rs` (`Settings::voice`), `crates/torroworkplace/src/{apps,core,settings_service}.rs`,
  `crates/torroworkplace/src/tray/**`, `crates/torroworkplace/src/ui/assistant.rs`, `docs/api.md`; nachgetragen
  (zwingend mechanisch): `crates/torroworkplace/src/{daemon,audit,pim}.rs` (`match`-Zweige, Tray folgt dem Lauschen),
  `crates/torroworkplace/src/ui/{launch,general}.rs` (Demo-Antworten, Testdaten)
- `torroassistant --listen`: Weckwort lokal, Aufnahme, Spracherkennung lokal, `assistant.send_message`, Antwort per
  Piper und Schnellfenster. Standard aus; Tray zeigt Lauschen und Aufnahme; Kürzel zum Stummschalten.
- **Oberfläche:** Steuerzentrale › Assistant › Voice, Tray · **API:** `settings.set_setting` (gibt es; neue Schlüssel
  `voice_enabled`, `wake_word`, `microphone`, `speak_answers`, `stt_model`, `voice`) ·
  `Request::VoiceStatus` · `settings.voice_status` · Read · `Request::TestVoice` · `settings.test_voice` · Admin
- **MCP:** Mikrofon einschalten **Never** (Punkt 6 in „Was nie nach außen geht“; `voice_enabled: true` wird nach Wert
  abgelehnt), `settings_test_voice` **Never** (Punkt 6), `settings_voice_status`; Weckwort zunächst fest „Hey Torro“
  (eigenes Wort später), Stimme und Weckwort über `settings_set_setting`
- **Abnahme:** Tests mit Audiodateien unter `tests/fixtures/` (Weckwort erkannt / nicht erkannt, Text erkannt);
  kein Test braucht ein Mikrofon; Freigaben lassen sich nicht per Sprache erteilen.

### C.10 Persönlichkeit und Gedächtnis des Hauptbots
- [x] erledigt (Bild: Buchstabe oder ein Symbol der Suite, kein Hochladen; Notizen, die ein lokales Modell nach dem
  Lesen von Mail anlegt, sind `local_only`: nie an entfernte Modelle oder Agenten, für Clients nur mit Leserecht auf
  Mail, und sie warten immer auf eine Freigabe; Freigaben zeigen Persönlichkeit und Notizen ganz in den Details;
  `docs/api-reference.md` gibt es noch nicht, A.9; nachgetragen (Match-Zweige): `crates/torroworkplace/src/{audit,
  daemon,pim}.rs`)
- **Modell:** Opus · **Braucht:** C.3 · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torroworkplace/src/assistant/**`, `crates/torroworkplace/src/{mcp,core}.rs`, `crates/torroworkplace/src/ui/**`,
  `crates/torroassistant/src/**`, `docs/api-reference.md`
- Profil (Name, Bild, Beschreibung, Persönlichkeit in `<config>/assistant/persona.md`) mit Vorlagen; Prompt-Reihenfolge
  Persönlichkeit → fester Teil → erzeugter Teil. Gedächtnis als `memory.md`, nur auf Wunsch.
- **Oberfläche:** Steuerzentrale › Assistant › Profile und Memory; Name und Bild im Chat-Kopf
- **API:** `assistant.get_profile` (Read), `assistant.update_profile` (Admin), `assistant.list_memories` (Read),
  `assistant.remember` (Write), `assistant.forget` (Delete) · **MCP:** gleichnamige `assistant_*`
- **Abnahme:** Test, dass eine Persönlichkeit mit „ignore the rules“ die festen Regeln nicht verdrängt (Reihenfolge
  im Prompt); Profil- und Gedächtnis-Handler-Tests; `torro-design-review`.

### C.11 Nacharbeiten: Lücken aus C.2, C.6, C.7, C.9 und C.10 schließen
- [x] erledigt (Freigaben merken sich Antrieb und abgezogene Bereiche des geliehenen Schlüssels selbst und laufen auch
  nach Ende des Laufs oder einem Neustart ohne sie; vor C.11 gespeicherte Freigaben haben das nicht, laufen höchstens
  zehn Minuten nach. `assistant.remember` mit geliehenem Schlüssel wartet immer; GitHub zählt auch über Filterlisten
  und verknüpfte Aufgaben als `untrusted`. Eigener Schlüssel eines Clients bleibt bei der Regel aus C.10)
- **Modell:** Opus · **Braucht:** C.10 · **Dateien:** `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,clients,approvals,audit}.rs`, `crates/torroworkplace/src/assistant/**`,
  `crates/torroworkplace/src/ui/assistant.rs`, `crates/torroworkplace/src/tray/**`
- **Rechte gleich beim Anlegen** (aus C.6): `clients.create_client` bekommt ein optionales Feld `rights` (verträglich);
  die Seite „Assistant“ legt den Client damit in einem Schritt an. Keine Millisekunden mehr mit *Read only* auf alle
  Mail-Konten, kein Rest, wenn das Update scheitert.
- **Freigaben im Lauf eines Agenten** (aus C.7): Ein freigegebener Befehl eines geliehenen Schlüssels läuft mit den
  eingeschränkten Rechten des Laufs (ohne Mail, ohne abgeschaltete Bereiche), nicht mit den vollen Rechten des Clients.
- **Antrieb im Protokoll nach Freigaben** (aus C.2): Erlaubt oder lehnt ein Mensch einen Befehl des Assistenten ab,
  trägt auch diese Protokollzeile `engine:<id>`.
- **Notizen aus fremden Daten** (aus C.10): Hat die Unterhaltung Daten mit `untrusted` gelesen (Eingang von Agenten
  und Skripten, GitHub, Mail), braucht `assistant.remember` immer eine Freigabe, nicht nur nach Mail.
- **Stummschalten immer erreichbar** (aus C.9): „Mute microphone“ steht im Tray, solange `voice_enabled` an ist, auch
  wenn der Zuhör-Prozess abgestürzt ist.
- **Oberfläche/API/MCP:** nur das neue optionale Feld `rights` an `clients.create_client` (bleibt **Never**, Punkt 2).
- **Abnahme:** je Punkt ein Test; `torro-api-review` ohne Muss-Punkt.

### C.12 Antwort flüssig streamen (Issue #135)
- [x] erledigt (Push aus dem Dienst bewusst nicht: der IPC kennt nur eine Antwort je Verbindung; schnelles
  Nachfragen nur während eines Laufs reicht, der Glätter verteilt die Stücke. „Bewegung reduzieren“ gibt es in der
  Suite noch nicht, siehe unten)
- **Modell:** Opus · **Braucht:** C.4 · **Dateien:** `crates/torroassistant/src/{app,view,demo,main,stream}.rs`,
  `crates/torroassistant/src/app/tests.rs`, `crates/torroassistant/Cargo.toml`, `Cargo.toml` (`unicode-segmentation`)
- **Nachfragen:** alle 50 ms nur, solange eine Antwort läuft (`get_conversation` mit `after`); wartet sie auf eine
  Freigabe, alle 500 ms.
- **Glätter** (`stream.rs`): angekommener Text erscheint auf den Frames des Fensters (`iced::window::frames`, nur
  solange etwas aussteht), Graphem für Graphem; das Tempo folgt dem Rückstand (Rückstand / 0,25 s, fertig / 0,12 s,
  mindestens 60 je Sekunde), ein langer Aussetzer zählt höchstens 50 ms. Das letzte Graphem eines wachsenden Textes
  wartet auf das nächste Stück. Was beim Öffnen schon da war, steht sofort ganz da.
- **Ruhig:** Markdown wird nur so gezeichnet, dass es beim Wachsen nicht springt (Blockmarken warten auf ihr erstes
  Wort, offenes Fett und Code werden für die Zeichnung geschlossen); fertig gestreamt ist die Zeichnung dieselbe wie
  ohne Streamen. Einträge nach einer Antwort, die noch erscheint, warten auf sie. Der Verlauf folgt dem neuesten
  Eintrag nur, wenn er unten steht; hochgescrollt bewegt sich nichts. Kein Cursor am Textende (nichts blinkt).
- **Oberfläche/API/MCP:** nur Oberfläche; kein neuer Befehl. MCP bleibt bei `assistant_get_conversation` mit `after`.
- **Offen:** eine Einstellung „Reduce motion“ der Suite (Steuerzentrale, API) und ein Abo-Weg im IPC; dann zeigt
  `Smoother::finish` alles sofort.
- **Abnahme:** Tests des Glätters (gleichmäßig, holt Rückstand auf, endet exakt mit dem ganzen Text, Grapheme) und
  des Fensters (erscheint Buchstabe für Buchstabe, beim Öffnen ganz, Einträge danach warten).

### C.13 „Forbidden“ mit Grund (Issue #133)
- [x] erledigt
- **Modell:** Opus · **Braucht:** C.7, C.11 · **Dateien:** `crates/torro-ipc/src/{api,assistant}.rs`,
  `crates/torroworkplace/src/{clients,core,mcp}.rs`, `crates/torroworkplace/src/assistant/{mod,model,tools,prompt}.rs`,
  `crates/torroworkplace/src/assistant/engines/{mod,claude,tests}.rs`, `crates/torroworkplace/src/ui/{mod,agents}.rs`,
  `crates/torroassistant/src/{chat,view,demo,bots}.rs`, `crates/torro-core/src/i18n/de.rs`
- **Befund:** Rechte werden nicht zwischengespeichert. „Forbidden“ trotz Mail-Recht kam vor allem von Claude Code
  (`NEVER_TO_AGENTS`), dazu: Mail in der Unterhaltung aus, zu niedrige Stufe, `accounts.*` statt Mail, keine Mailkonten.
- **Grund im Fehler:** `api::Lack` (Bereich fehlt, Stufe, Admin, Senden aus, nur für Menschen, in der Unterhaltung aus,
  Engine bekommt es nie). Der Dienst nennt ihn im `forbidden` (`Refused::Lacks`, Text für Agenten), die Tool-Zeile
  trägt ihn (`EntryContent::Tool::lack`, neues optionales Feld) mit Worten für Menschen; der Chat zeigt den Grund und
  „Change access“ nur, wo eine Rechteänderung hilft.
- **Claude Code:** Die Datenregel bleibt (Mail nie an Agenten). Die Tools der entzogenen und abgeschalteten Bereiche
  werden gar nicht angeboten (`--disallowedTools`), der Prompt sagt es, der Picker zeigt „Never gets your mail“.
- **Mail einschalten** (Rechte-Ansicht): Ist kein Mailkonto angehakt, werden die jetzt angemeldeten angehakt (nicht
  „All“, damit spätere Konten aus bleiben).
- **Oberfläche/API/MCP:** kein neuer Befehl; jede `forbidden`-Antwort von API und MCP trägt den Grund im `message`.
- **Abnahme:** je Ursache ein Test (Gate, Antrieb A, Antrieb B, `Lack::of`, Chat-Hinweis, Rechte-Ansicht).
- **Offen:** Nachstellen mit echtem Ollama/OpenRouter und Claude Code (manuell).

### C.14 Eigene Rechte kennen, alle Rechte end-to-end (Issue #134)
- [x] erledigt
- **Modell:** Opus · **Braucht:** C.13 · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torroworkplace/src/{core,mcp,audit,daemon,pim}.rs`, `crates/torroworkplace/src/assistant/{model,tools,prompt}.rs`,
  `crates/torroworkplace/src/assistant/engines/mod.rs`, `crates/torroassistant/src/chat.rs`,
  `crates/torro-core/src/i18n/de.rs`, `docs/assistent.md`
- **Prompt** („This conversation“): je Bereich, den der Assistent nutzt, Stufe, Admin, Zahl der Kalender, Listen,
  Repositories, Quellen und Mailkonten, Senden; was auf Freigabe wartet; Bereiche, die die Engine nie bekommt, obwohl
  sie erlaubt sind.
- **Request** `assistant.my_access`: die **wirksamen** Rechte des fragenden Clients (`MyAccess`: Rechte, Engine,
  `withheld`, `switched_off`), nach Leihe, `without` und für Bots, die für den Assistenten laufen, `Rights::common`.
  Assistent und Bots dürfen ihn durch ihre Art (`allowed_by_kind`), andere Clients mit `assistant`; der App-Schlüssel
  bekommt `unsupported`. Rechte bleiben nur über den App-Schlüssel änderbar.
- **Oberfläche:** keine eigene; der Assistent antwortet im Chat („Looked up its access“), die Person sieht die Rechte
  in Agents & API bzw. Assistant.
- **API:** `Request::MyAccess` · `assistant.my_access` · Read
- **MCP:** `assistant_my_access`
- **Abnahme:** Prompt-Test; End-to-End im Dienst mit Fake-Modell (alle Rechte, Mail mit allen Konten, `without`)
  gegen `clients.list_clients` und das Protokoll; Leihe für Claude Code (ohne Mail, ohne abgeschaltete Bereiche);
  Bot für den Assistenten = `Rights::common`; MCP-End-to-End mit dem Schlüssel des Assistenten.
- **Offen:** manuell mit echtem Ollama/OpenRouter und Claude Code fragen „Welche Rechte hast du?“.

### C.15 Genaues Modell und Aufwand (Issue #131)
- [x] erledigt
- **Modell:** Opus · **Braucht:** C.7 · **Dateien:** `crates/torro-core/src/{ai/mod,ai/openai,settings}.rs`,
  `crates/torro-ipc/src/{lib,api,assistant}.rs`, `crates/torroworkplace/src/{account/ai,settings_service,mcp}.rs`,
  `crates/torroworkplace/src/assistant/**`, `crates/torroworkplace/src/ui/{mod,assistant,launch,general}.rs`,
  `crates/torroassistant/src/**`, `crates/torro-core/src/i18n/de.rs`; mechanisch nachgezogen (neue Felder in
  Literalen): `core.rs`, `audit.rs`, `approvals.rs`, `bots/tests.rs`
- **Aufwand:** `torro_core::ai::Effort { Low, Medium, High }` in `ChatRequest`, `assistant.send_message`,
  `EntryContent::Message` und `ConversationSummary` (alle optional). Je Anbieter: OpenRouter `reasoning.effort`,
  Ollama, LM Studio und andere OpenAI-kompatible `reasoning_effort` (Ollamas `/v1` bildet es auf `think` ab),
  Claude Code `--effort`. `AiModel::reasoning` sagt, ob ein Modell nachdenkt (OpenRouter `supported_parameters`,
  Ollama-Fähigkeit `thinking`); die Oberfläche bietet den Aufwand nur, wo das nicht `false` ist.
- **Claude Code:** feste Modelle (`fable`, `opus`, `sonnet`, `haiku`) in `Engine::models` von
  `assistant.list_engines`; Haiku ohne Aufwand. Das Modell geht wie bisher als `--model`.
- **Alle Modelle:** „Show all models“ im Picker fragt `ai.list_models { all: true }` (auch geschlossene Modelle);
  Modelle ohne Tools bleiben ausgeblendet.
- **Standard:** `SettingKey::AssistantDefault` (`{engine, model, effort}` oder `null` = zuletzt benutzt) in
  Steuerzentrale › Assistant („New chats“, „Model“, „Effort“). Der Chat nimmt ihn, solange die Engine angeboten wird.
- **Oberfläche:** Composer (Picker mit Claude-Code-Modellen und „Show all models“, Knopf „Effort: …“), Steuerzentrale
  › Assistant. **API:** `assistant.send_message` + `effort`; `settings.set_setting assistant_default` (Admin).
  **MCP:** `assistant_send_message` und `settings_set_setting` erben die Felder.
- **Nicht gemacht:** ein gespeicherter Aufwand je Bot (ein Bot-Chat sendet den Aufwand des Composers).
- **Abnahme:** Tests: Request-Body je Anbieter enthält den Aufwand; Claude-Code-Argumente enthalten Modell und
  `--effort`; Aufwand in jeder Anfrage einer Antwort; Standard der Steuerzentrale im Chat; Einstellung geprüft.

### C.16 Berechtigungsmodus im Chat (Issue #132)
- [x] erledigt
- **Modell:** Opus · **Braucht:** C.13, C.14 · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torroworkplace/src/{core,clients,approvals,mcp}.rs`, `crates/torroworkplace/src/assistant/**`,
  `crates/torroassistant/src/**`, `crates/torro-core/src/i18n/de.rs`; mechanisch nachgezogen (neues Feld in
  Literalen): `audit.rs`, `bots/tests.rs`
- **Modus** `torro_ipc::assistant::Mode`: `read_only` (jeder Bereich höchstens lesen, kein Admin, kein Senden; alles
  über Lesen `forbidden` mit Grund `Lack::ReadOnly`, auch das eigene Gedächtnis), `ask` (`confirm = OnEveryChange`:
  jede Änderung wartet auf eine Freigabe), `default` (gespeicherte Rechte). `Mode::narrow` macht nur enger.
- **Angewendet:** Antrieb A über `Host::run_in` (der Kern verengt den Client, die Freigabe behält den Modus in
  `approvals::Origin::mode`, ein späteres „Allow“ läuft verengt); Antrieb B über einen im Modus geliehenen Schlüssel
  (`Clients::lend_in_mode`, `Clients::lent_mode`) und `--disallowedTools` für schreibende Tools bei `read_only`; ein
  gefragter Bot läuft im Modus des Assistenten (`Delegation::mode`, `Host::ask_bot_in`). `assistant.my_access` nennt
  den Modus (`MyAccess::mode`).
- **Gemerkt** am Nachrichteneintrag wie `without` (`EntryContent::Message::mode`); eine wieder geöffnete
  Unterhaltung übernimmt ihn, ein neuer Chat beginnt mit „Default access“.
- **„Auto“ nicht angeboten:** ohne Rückfrage bis zur gespeicherten Grenze hieße bei `confirm = OnDelete` weiter
  machen als gespeichert; der Modus darf nur enger machen.
- **Oberfläche:** unter dem Eingabefeld Knopf mit dem Modus und daneben eine Zeile, was gerade erlaubt ist
  („Mail: Read · Calendar: Write · asks before changes“, aus `MyAccess`); in Bot-Chats nicht (der Bot hat eigene
  Rechte). **API:** `assistant.send_message` + `mode` (optional, verträglich). **MCP:** `assistant_send_message`
  erbt das Feld; der Modus engt nur ein, daher für Agenten erlaubt.
- **Abnahme:** Tests: Read only verbietet Schreiben (Antrieb A, geliehener Schlüssel, Werkzeugliste, Claude-Code-
  Argumente); Ask before changes erzeugt Freigaben, auch mit geliehenem Schlüssel; kein Modus öffnet, was die Rechte
  schließen (Tabellentest über den Katalog, Kern-Test); die Freigabe behält den Modus.

### C.17 Datenregel für Mail als Schalter (vorgezogener Teil von KI.1)
- [x] erledigt (Antworten aus Einträgen jeder Quelle bleiben auch mit Schalter lokal, weil der Kern ihre Quelle nicht
  nennt; Claude Code sah Talk- und Nextcloud-Einträge seiner Quellen schon vorher)
- **Modell:** Opus · **Braucht:** C.14, C.16 · **Dateien:** `crates/torro-core/src/{settings,i18n/de}.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/{core,clients,settings_service}.rs`,
  `crates/torroworkplace/src/assistant/**`, `crates/torroworkplace/src/ui/{assistant,agents,bots,general}.rs`,
  `docs/{api,ki,assistent}.md`
- **Anlass:** Die Person gab dem Assistenten Mail auf alle Konten, er antwortete mit Claude Code aber immer
  „forbidden“: Die feste Datenregel (Mail nur an Modelle auf den eigenen Rechnern) war stärker als die Rechte und
  ließ sich nirgends ändern.
- **Schalter:** Einstellung `mail_to_cloud` (`true`/`false`, Standard `false` = nur lokal). An: Mail darf auch an
  Claude Code und an Modelle außerhalb der eigenen Rechner (OpenRouter, Server eines Anbieters). Dann bekommt
  Claude Code den Bereich Mail (soweit die Rechte ihn geben), und Modelle anderswo lesen Mail-Antworten,
  Mail-Einträge und Regeln von Mail-Konten im Eingang, Aufgaben aus Mails und Mail-Verweise in Aufgaben. Für
  Cloud-Modelle bleiben lokal: Einträge und Regeln von Nextcloud und Talk, Antworten aus Einträgen jeder Quelle
  (Stummschalten, Statistik, Aufgabe/Termin aus Eintrag) und Notizen, die nach Mail entstanden (`local_only`).
- **Nur die Person schaltet ein** (wie das Mikrofon, Punkt 6 in „Was nie nach außen geht“): `true` nur mit dem
  App-Schlüssel (`Request::kept_for_people`, auch über `settings.update_settings`); ausschalten darf jeder, der
  Einstellungen ändern darf. Ein Agent kann sich so nie selbst Mail verschaffen.
- **Sichtbar bei den Rechten:** In den Rechten des Assistenten (Steuerzentrale › Assistant, dorthin führt auch seine
  Zeile in Agents & API) und jedes Bots steht direkt unter Mail der Schalter „Mail to Claude Code and cloud models“
  mit dem, was er bedeutet. Der Grund eines `forbidden` (`Lack::NeverToAgents`) und der Systemtext des Assistenten
  nennen den Schalter, die Werkzeugzeile im Chat sagt „(Datenregel)“.
- **Oberfläche:** Rechte des Assistenten und der Bots. **API:** `settings.set_setting
  mail_to_cloud` · Admin (einschalten nur App-Schlüssel). **MCP:** `settings_set_setting` erbt den Schlüssel, `true`
  ist für jeden Client `forbidden`.
- **Abnahme:** Tests: Einstellung gelesen/geprüft; Client darf nicht einschalten, aber ausschalten (auch über
  `update_settings`); mit Schalter an bekommt Claude Code Mail (kein Abzug beim geliehenen Schlüssel, Prompt ohne
  „never given“); Modell anderswo liest Mail, aber keine Talk-Einträge; Verlauf und Bots folgen dem Schalter;
  `torro-api-review` ohne Muss-Punkt.

## Stufe 2: Bots (B.1–B.5)

Konzept: Abschnitt „Bots“ in [assistent.md](../assistent.md). Geplant am 2026-10-14 nach Stufe 1.

```
B.1 ──┬── B.2
      ├── B.3 ── B.5
      └── B.4
```

**Festlegungen für alle B-Aufgaben** (konservativ, nach dem Konzept):
- Ein Bot liegt unter `<config>/assistant/bots/<id>/` (Ordner 0700, Dateien 0600): `bot.json` (Name, Beschreibung,
  Bild, Antrieb, Modell, Kennung seines Clients), `persona.md`, `instructions.md`, `skills/*.md` (nur Text). Grenzen:
  Name 40, Beschreibung 200, Persönlichkeit 4.000, Anweisungen 8.000 Zeichen, je Skill 8.000, höchstens 8 Skills,
  höchstens 50 Bots.
- Jeder Bot hat einen eigenen Client der Art `bot` und startet **ohne Rechte** (`Rights::none()` über das Feld
  `rights` von `clients.create_client`, C.11). Rechte vergibt nur der Mensch (Punkt 2, **Never**).
- Ein Bot-Client darf im Bereich `bots` und `assistant` nichts (keine Ketten, keine Schleife), fest im Kern.
- Unterhaltungen mit einem Bot sind Unterhaltungen im Speicher aus C.1 mit dem neuen optionalen Feld `bot`;
  `assistant.send_message` bekommt dafür ein optionales Feld `bot` (verträglich).
- Prompt eines Bots: Persönlichkeit des Bots → feste Regeln (Skill aus C.3) → Anweisungen und Skills des Bots →
  erzeugter Teil. Die Regeln gewinnen; Datenregel und Kennzeichnung fremder Daten wie beim Hauptbot.

### B.1 Gerüst: Bereich `bots`, Speicher, Client-Art, direkter Chat
- [x] erledigt (Kennung eines Bots 8 Hex-Zeichen; `bots.save_bot` ersetzt einen Bot ganz, Ändern braucht wie beim
  Entwurf das Recht zu löschen; `assistant.send_message { bot }` antwortet mit `engine`/`model` der Anfrage (ohne
  Modell das des Bots), `bots.ask_bot` mit denen des Bots; eine Unterhaltung gehört für immer ihrem Bot; ein Client
  braucht für `send_message { bot }` zusätzlich Schreiben in `bots`; der Hauptbot schlägt nur neue Bots vor, `ask_bot`
  bleibt bis B.2 `forbidden` (Haken `bots::assistant_may_ask`); Werkzeuge `bots_*` bekommt das Modell des Hauptbots
  erst mit B.2; `assistant.open_window { bot }` bleibt `unsupported` bis B.3; Freigaben eines Bots nennen ihn
  „Bot "…"“; `docs/api-reference.md` gibt es noch nicht, A.9; nachgetragen (zwingend mechanisch): `main.rs`
  (`mod bots`), `{daemon,pim,mcp}.rs`, `ui/agents.rs`, `crates/torroassistant/src/**` (`match`-Zweige, neue Felder))
- **Modell:** Opus · **Braucht:** C.11 · **Dateien:** `crates/torro-ipc/src/{lib,api,assistant}.rs`,
  `crates/torro-ipc/src/bots.rs` (neu), `crates/torroworkplace/src/assistant/**`, `crates/torroworkplace/src/bots/**`
  (neu), `crates/torroworkplace/src/{core,clients,approvals,audit}.rs`
- `Area::Bots`; Requests und Katalog: `bots.list_bots` (Read), `bots.get_bot` (Read), `bots.save_bot` (Write; ohne
  Rechte; legt beim ersten Speichern den Client an), `bots.delete_bot` (Delete; entfernt Client und Ordner, die
  Unterhaltungen bleiben als „deleted bot“), `bots.ask_bot { bot, text, conversation? }` (Write; startet einen Lauf
  des Bots und wartet nicht).
- Direkter Chat: `assistant.send_message { bot }` läuft mit dem Client des Bots und seinem Prompt. Antriebe wie beim
  Hauptbot (KI-Konto oder Claude Code mit geliehenem Schlüssel des Bot-Clients).
- Der Hauptbot (Client `assistant`) darf `bots.list_bots` und `bots.ask_bot`; `bots.save_bot` vom Hauptbot braucht immer
  eine Freigabe (Vorschlag eines Bots). Freigabe-Texte mit Namen aus dem Speicher.
- **Abnahme:** Handler-Tests (anlegen ohne Rechte, ändern, löschen, Grenzen, 0600), Bot-Client bekommt `forbidden`
  für `bots.*` und `assistant.*`, direkter Chat mit Test-Antrieb, Prompt-Reihenfolge, Rechte-Tabelle.

### B.2 Hauptbot gibt ab, nur auf Ansage; Schnittmenge der Rechte
- [x] erledigt (Nennung: Wörter des Namens ohne Artikel in Folge, ohne Groß/Klein, mit einfacher Endung
  „s/n/e/en/es/er/ns“; Bestätigung nur im Speicher des Kerns, gilt für eine Anfrage; die Anfrage des Hauptbots
  braucht keine Freigabe (die Worte der Person sind das Ja), die Befehle des Bots fragen nach ihren Rechten; der
  Hauptbot wartet, bis der Bot fertig ist; eine Unterhaltung des Bots je Unterhaltung des Hauptbots (im Speicher);
  Verweis auf sie in `input.conversation` der Zeile `bots.ask_bot`; nach einem Bot zählt die Unterhaltung als
  „hat Mail und fremde Daten gelesen“ für spätere Runden; Protokoll `bot:<id>` und `for:<Client des Hauptbots>`;
  nachgetragen (zwingend mechanisch): `{pim,daemon}.rs` (`match`-Zweige), Test in `mcp.rs` (Never im Bereich
  `assistant`))
- **Modell:** Opus · **Braucht:** B.1 · **Dateien:** `crates/torroworkplace/src/assistant/**`,
  `crates/torroworkplace/src/bots/**`, `crates/torroworkplace/src/{core,approvals}.rs`
- Werkzeug `bots_ask_bot` für den Hauptbot nur, wenn die **letzte Nachricht der Person** den Bot beim Namen nennt
  (ohne Groß/Klein, auch „den Reiseplaner“) oder die Person einen Vorschlag des Hauptbots im Chat bestätigt hat
  (`assistant.confirm_bot { conversation, bot }` · Write, nur App-Schlüssel, **Never**: eine Bestätigung gibt nur
  der Mensch). Sonst `forbidden` mit Hinweis für das Modell, den Bot vorzuschlagen.
- Läuft der Bot im Auftrag des Hauptbots, gelten die **Schnittmenge** der Rechte beider Clients und die abgeschalteten
  Bereiche der Unterhaltung; Freigaben speichern das (wie C.11).
- Ergebnis als Schritt im Verlauf des Hauptbots („Asked Travel planner · 3 steps“) mit Verweis auf die Unterhaltung
  des Bots.
- **Abnahme:** Tests: ohne Nennung `forbidden`; mit Nennung läuft es; Bestätigung nur per App-Schlüssel; Schnittmenge
  (Hauptbot darf Mail, Bot nicht → keine Mail; umgekehrt ebenso); Bot kann keinen Bot fragen; Protokoll nennt beide.

### B.3 App und Steuerzentrale: Bots anlegen, Rechte, Chat, Fenster
- [x] erledigt (Bot-Gespräche stehen in der Seitenleiste mit den anderen unter „Recent“, ein Klick auf einen Bot öffnet
  sein neuestes Gespräch; im Bot-Chat gibt es kein „without …“, der Bot hat eigene Rechte; der Editor ersetzt den
  Chat, „Save“ steht in der Kopfleiste; die Karte „Ask {bot}?“ schickt `assistant.confirm_bot` und danach die Nachricht
  „Yes, ask {bot}.“, damit der Assistent weitermacht; Bot-Clients bilden unter Agents & API eine eigene Gruppe, ihre
  Zeilen öffnen Assistant › Bots; `assistant.open_window { bot }` startet `torroassistant --bot <id>`, `not-found` für
  einen unbekannten Bot; nachgetragen (zwingend mechanisch): `crates/torroworkplace/src/core.rs` (Test erwartete
  `unsupported`); Sichtprüfung auf macOS mit AeroSpace statt Hyprland)
- **Modell:** Opus · **Braucht:** B.1 · **Dateien:** `crates/torroassistant/**`, `crates/torroworkplace/src/ui/**`
- App: Seitenleiste „Bots“ (Liste, „New bot“), Chat mit einem Bot (Kopf mit Bild und Name), Bot-Editor (Profil,
  Persönlichkeit, Anweisungen, Skills als Texte, Antrieb/Modell), „Access…“ öffnet die Rechte in der Steuerzentrale.
  `--bot <id>` öffnet das Chatfenster des Bots (neueste Unterhaltung oder neue), Fensterklasse `torro-assistant-chat`,
  Titel mit dem Namen des Bots; Workspace-Vorlagen merken das Argument. Karte „Confirm: ask {bot}?“ für Vorschläge des
  Hauptbots (schickt `assistant.confirm_bot`).
- Steuerzentrale: Seite „Assistant“ › Bots mit Liste und je Bot den Rechten (dieselbe Ansicht wie bei Agenten,
  `clients.update_client`), Direktlink `ui --section bot:<id>`.
- **Abnahme:** UI-Tests, `--demo`-Zustände, `scripts/design-check.sh`, `torro-design-review`.

### B.4 MCP-Tools `bots_*`
- [x] erledigt
- **Modell:** Haiku · **Braucht:** B.1 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Je Katalogeintrag ein Tool; `bots_ask_bot` wartet wie `assistant_send_message` bis zur Antwort oder 60 s.
- **Abnahme:** Vertragstest; ein Bot-Client bekommt `forbidden`; Warten und Stand mit `after`.

### B.5 Rechte gleich im Bot-Editor vergeben
- [x] erledigt (Regel beim Anschalten als `Rights::set_level` mit `present` als Closure, dazu `Rights::selection`,
  `api::present_sources` und `api::signed_in_mail`, damit Steuerzentrale und Editor dieselben Kandidaten anbieten;
  Detailzeile bei Mail „{n} of {m} mail accounts · sending: …“ (Senden ab „Write“) wie bei den anderen Auswahlen,
  GitHub „{n} repositories“; Zeile „Ask first“ mit eigener Detailzeile in derselben Gruppe; eine Zeile, die auf den
  gelesenen Stand zurückgestellt wird, gilt nicht als geändert; die Tests der App beantworten jede Anfrage mit der
  Abbildung, die das Fenster ihr gab (`App::replies`), statt sie aus dem Request zu raten; neuer `--demo`-Zustand
  `--open bot-access`; nachgetragen (zwingend mechanisch): `Cargo.lock` (neue Abhängigkeiten `torro-inbox`,
  `torro-pim` in `crates/torroassistant/Cargo.toml` für die gelesenen Antworten); Sichtprüfung auf macOS mit
  AeroSpace statt Hyprland, ohne `torro-design-review`)
- **Modell:** Opus · **Braucht:** B.3 · **Dateien:** `crates/torroassistant/**`, `crates/torro-ipc/src/api.rs`
  (gemeinsame Hilfen, siehe unten), `crates/torroworkplace/src/ui/agents.rs` (nutzt die Hilfen; Regel beim Anschalten,
  siehe unten), `crates/torro-core/src/i18n/de.rs` (nur über die Koordination)
- **Oberfläche:** Bot-Editor im Assistenten, neuer Abschnitt „Access“ unter Antrieb/Modell, beim Anlegen und beim
  Ändern. Je Bereich, den die Steuerzentrale für einen Bot zeigt (`shows_area` mit `BOT_CLIENT`: alle vergebbaren außer
  `assistant` und `bots`), eine Zeile mit Namen, Detailzeile und Segmenten der Stufen, die der Bereich anbietet
  (`levels`: „None“, „Read“, „Write“, „Full“). Darunter „Ask first“ (`Confirm`: „Never“, „On delete“, „On every
  change“). Am Ende ein Hinweis mit Link „More in Torro Workplace…“ (öffnet wie heute `ui --section bot:<id>`): welche
  Kalender, Listen, Repositories, Quellen und Mailkonten, Senden von Mail und Admin-Befehle stellt man dort ein. Ein
  neuer Bot zeigt alle Bereiche auf „None“ und „Ask first“ auf „On delete“ (`Rights::none`). Die Werte im Abschnitt
  entstehen nur durch Klicks der Person, nie aus einem Vorschlag des Hauptbots oder aus dem Chat.
- **API:** kein neuer Request, kein neuer Katalogeintrag. Rechte lesen mit `clients.list_clients` · Admin, setzen mit
  `clients.update_client` · Admin (beide gibt es, nur App-Schlüssel, wie die Steuerzentrale). Für Detailzeilen und die
  Regel beim Anschalten liest der Editor beim Öffnen `calendar.list_calendars`, `tasks.list_lists`,
  `inbox.list_sources` und `accounts.list_accounts` (alle gibt es). `bots.save_bot` bleibt ohne Rechte, damit auch der
  Hauptbot (MCP) beim Vorschlagen nie Rechte mitgibt.
- **MCP:** **Never** – Rechte vergibt nur ein Mensch (Punkt 2 in „Was nie nach außen geht“, `docs/api.md`).
- **Öffnen:** Der Editor liest `clients.list_clients` jedes Mal neu, zeigt also nie einen alten Stand.
- **Ablauf beim Speichern:** `bots.save_bot` → aus der Antwort `Bot::client`; der Editor übernimmt sofort die neue
  Kennung des Bots, damit ein zweites „Save“ keinen zweiten Bot anlegt. Nur wenn sich im Abschnitt etwas geändert hat:
  `clients.list_clients` neu lesen, die **geänderten** Zeilen (Stufen, „Ask first“) auf die aktuellen Rechte des
  Clients legen, `clients.update_client`. So überschreibt der Editor nichts, was inzwischen in der Steuerzentrale
  geändert wurde (Auswahl, Senden, Admin-Schalter bleiben). Bekannte Grenze: zwischen Lesen und Schreiben liegt ein
  kurzes Fenster, `update_client` ersetzt die Rechte ganz (wie in der Steuerzentrale); ein gezielter Befehl wäre eine
  spätere Aufgabe. Erst danach schließt der Editor und öffnet beim neuen Bot den Chat. Scheitert `update_client`, ist
  der Bot gespeichert: Hinweis „Bot saved, but its access couldn't be set.“ mit „Access…“, der Editor bleibt offen,
  ein zweites „Save“ versucht es erneut (mit `id`).
- **Bereich anschalten (gemeinsame Regel, auch für die Steuerzentrale):** Wird ein Bereich von „None“ auf eine Stufe
  gestellt und ist seine Auswahl leer (`Selection::Only([])`, wie bei jedem neuen Bot), bekommt er die **jetzt
  vorhandenen** Kennungen (`Only(ids)`): Kalender, Aufgabenlisten, Eingangsquellen und angemeldete Mailkonten – die
  Regel aus Issue #133, auf alle Auswahlen ausgeweitet. Später hinzugefügte bleiben aus, nie `All`. GitHub hat keine
  Kandidatenliste: die Auswahl bleibt leer, die Detailzeile sagt „No repositories · choose in Torro Workplace“. Eine
  schon gesetzte Auswahl bleibt, wie sie ist. Die Regel steht als reine Hilfe in `torro_ipc::api` und ersetzt den
  Mail-Fall in `change_rights`; die Steuerzentrale ändert sich damit bewusst (Kalender usw. anschalten gibt jetzt die
  vorhandenen statt keiner).
- **Detailzeile:** was gilt, z. B. „All calendars“, „{n} of {m} calendars“, „No mail account“, „{n} mail accounts ·
  sending: never“; steht eine Zeile auf „None“, ist ihr Admin-Schalter aber in der Steuerzentrale an: „Admin commands
  allowed“ (der Editor lässt `admin` beim Ändern der Stufe stehen, wie `change_rights`).
- **Gemeinsame Hilfen:** `has_command`, `levels`, `shows_area` und `active_level` ziehen aus `ui/agents.rs` nach
  `torro_ipc::api` (ohne Verhaltensänderung); `agents.rs` reicht sie mit `pub(super) use` weiter, damit `ui/bots.rs`
  und `ui/assistant.rs` unverändert bleiben. Die reine Logik des Abschnitts (Zeilen, Änderungen merken, auf aktuelle
  Rechte legen, Detailzeilen) steht in `crates/torroassistant/src/bots.rs` und ist ohne Fenster testbar; Typ heißt
  `BotAccess` (in `app.rs` gibt es schon ein `Access`). Texte der Bereiche und Stufen mit denselben englischen
  Schlüsseln wie in der Steuerzentrale, damit die vorhandenen Übersetzungen greifen.
- **Nicht in B.5:** Auswahl einzelner Kalender/Konten, Senden, Admin-Schalter und Vorlagen („Read only“ …) im
  Editor; das bleibt in der Steuerzentrale.
- **Abnahme:** Tests der reinen Logik: neuer Bot ohne Änderung schickt kein `update_client`; nur geänderte Zeilen
  landen in den Rechten, eine inzwischen geänderte Auswahl und `outbound` bleiben; Anschalten gibt die vorhandenen
  Kennungen (Mail ohne Konto → leer, „No mail account“; GitHub bleibt leer), eine vorhandene Auswahl bleibt;
  `assistant`/`bots` kommen nie vor; „None“ mit Admin-Schalter zeigt den Hinweis. Test in `agents.rs` für die
  ausgeweitete Regel. App-Tests mit Test-Backend: Reihenfolge `bots.save_bot` → `clients.list_clients` →
  `clients.update_client`; Fehler bei `update_client` → Hinweis, Editor offen, zweites „Save“ legt keinen zweiten Bot
  an; ein vom Hauptbot vorgeschlagener Bot zeigt beim Öffnen überall „None“. `--demo`-Zustand mit Abschnitt,
  `scripts/design-check.sh`, `torro-design-review` (schmal bis breit), `TORRO_LANG=de cargo test --workspace`;
  Übersetzungen im Bericht.
