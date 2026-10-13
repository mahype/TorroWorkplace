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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/assistant/**`
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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.2 · **Dateien:** `crates/torroworkplace/src/assistant/prompt.rs`,
  `crates/torroworkplace/assets/assistant/skill.md` (neu)
- Fester Teil (englisch) und erzeugter Teil (Datum, Zeitzone, Sprache, erlaubte Bereiche) nach dem Konzept.
- **Abnahme:** Test, dass der Prompt keine Geheimnisse und keine Inhalte aus Sammlungen enthält und nur erlaubte
  Bereiche nennt; Test mit Mock-Modell, dass der Prompt ankommt.

### C.4 App `torroassistant`
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.1 (Test-Antrieb reicht) · **Dateien:** `crates/torroassistant/**` (neu),
  `Cargo.toml` (Wurzel, Mitglied), `crates/torro-core/src/app.rs` (`AppId::ASSISTANT`)
- Seitenleiste, Verlauf, Eingabefeld mit Konto/Modell und „without …“, Werkzeug-Zeilen aufklappbar, Freigabe-Karte
  (`approvals.decide_approval` per Klick), Stop, erster Start mit Auswahl der Rechte (öffnet C.6-Ansicht), `--demo`.
- **Eigene Chatfenster** (Abschnitt „Eigene Chatfenster“ im Konzept): `--chat <id>|new`, Fensterklasse
  `torro-assistant-chat`, „Open in new window“, Doppelklick; Workspace-Vorlagen merken das Argument; gelöschte
  Unterhaltung → neue. Dazu in `crates/torro-core/src/app.rs` die zweite Fensterart, in `apps.list_apps` als eigener
  Eintrag „Assistant chat“ nur, wenn die Vorlagen das brauchen (wie bei `torroinbox --view`).
- **Oberfläche/API:** nur Requests aus C.1 und vorhandene; **MCP:** keine neuen Tools.
- **Abnahme:** `torro-design-review` in allen Größenklassen; Texte über `tr()`, Übersetzungen im Bericht.

### C.5 MCP-Tools `assistant_*`
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Je Katalogeintrag ein Tool; `assistant_send_message` wartet bis zur Antwort oder 60 s, dann Stand mit `after`.
- **Abnahme:** Vertragstest; Test, dass ein Client der Art `assistant` `forbidden` bekommt.

### C.6 Steuerzentrale › Assistant
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.1 · **Dateien:** `crates/torroworkplace/src/ui/**`
- Seite „Assistant“: Standard-Antrieb, Aufbewahrung, „What the assistant may use“ (dieselbe Rechte-Ansicht wie bei
  Clients, Vorlagen, Sammlungen). Erreichbar auch aus dem Hinweis bei `forbidden` im Chat.
- **API:** `settings.set_setting` (gibt es), `clients.update_client` (gibt es) · **MCP:** `settings_set_setting`;
  Rechte **Never** (Punkt 2).
- **Abnahme:** `torro-design-review`; Änderung wirkt in einer laufenden Unterhaltung ab der nächsten Anfrage.

### C.7 Antrieb B: Claude Code und Codex
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.2 · **Dateien:** `crates/torroworkplace/src/assistant/engines/**`
- Claude Code mit `stream-json` in beide Richtungen, eigene `--mcp-config` mit `--strict-mcp-config`, nur
  `mcp__torroworkplace__*`, Schlüssel des Assistenten-Clients in `TORRO_TOKEN`, Skill über `--append-system-prompt`.
  Codex mit `codex exec --json`. Nicht installiert → Antrieb fehlt in `list_engines`.
- **Abnahme:** Tests mit einem Fake-Programm, das den Stream nachspielt; kein Test braucht einen echten Agenten.

### C.8 Skill-Datei und MCP-Prompt für fremde Agenten
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** C.3 (Text) · **Dateien:** `crates/torroworkplace/src/{mcp,client_setup}.rs`
- Der feste Teil aus C.3 als MCP-Prompt `torro_workplace`; „Connect agent“ legt ihn für Claude Code zusätzlich als
  Skill ab (zusammengeführt, wie die übrigen Einträge, und beim Trennen wieder entfernt).
- **Abnahme:** Test, dass der Prompt über MCP kommt; Setup-Test mit Temp-Home.

### C.9 Sprache: Weckwort, Spracherkennung, Sprachausgabe (lokal)
- [ ] erledigt
- **Modell:** Opus · **Braucht:** C.4 · **Dateien:** `crates/torroassistant/src/voice/**`, `crates/torro-ipc/src/api.rs`
  (Regel Mikrofon), `docs/api.md`, `Cargo.toml` (Wurzel: Audio, openWakeWord, whisper.cpp, Piper)
- `torroassistant --listen`: Weckwort lokal, Aufnahme, Spracherkennung lokal, `assistant.send_message`, Antwort per
  Piper und Schnellfenster. Standard aus; Tray zeigt Lauschen und Aufnahme; Kürzel zum Stummschalten.
- **Oberfläche:** Steuerzentrale › Assistant › Voice, Tray · **API:** `settings.set_setting` (gibt es) ·
  **MCP:** Mikrofon einschalten **Never** (Punkt 6 in „Was nie nach außen geht“), Weckwort zunächst fest
  „Hey Torro“ (eigenes Wort später), Stimme und Weckwort über `settings_set_setting`
- **Abnahme:** Tests mit Audiodateien unter `tests/fixtures/` (Weckwort erkannt / nicht erkannt, Text erkannt);
  kein Test braucht ein Mikrofon; Freigaben lassen sich nicht per Sprache erteilen.

### C.10 Persönlichkeit und Gedächtnis des Hauptbots
- [ ] erledigt
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

## Stufe 2: Bots (Planung folgt)

Konzept: Abschnitt „Bots“ in [assistent.md](../assistent.md). Grobe Aufteilung, Einzelheiten nach Stufe 1:

- **B.1 Gerüst:** Bereich `bots`, Client-Art `bot`, `bots.list_bots` / `save_bot` / `delete_bot` / `ask_bot`, Speicher
  je Bot, Schnittmenge der Rechte beim Abgeben, keine Ketten.
- **B.2 Hauptbot gibt ab, nur auf Ansage:** `bots.ask_bot` des Hauptbots nur, wenn die Person den Bot nennt oder
  einen Vorschlag bestätigt (im Kern geprüft); Schritt im Verlauf.
- **B.3 App:** Seitenleiste „Bots“, Bot-Editor mit Rechten, direkter Chat, Ansprache per Name/Weckwort,
  eigenes Chatfenster je Bot (`torroassistant --bot <id>`, in Workspace-Vorlagen merkbar).
- **B.4 MCP-Tools `bots_*`.**
