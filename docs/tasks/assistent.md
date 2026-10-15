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
