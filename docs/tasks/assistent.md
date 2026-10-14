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

## Stufe 2: Bots (B.1–B.4)

Konzept: Abschnitt „Bots“ in [assistent.md](../assistent.md). Geplant am 2026-10-14 nach Stufe 1.

```
B.1 ──┬── B.2
      ├── B.3
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
- [ ] erledigt
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
- [ ] erledigt
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
- [ ] erledigt
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
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** B.1 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Je Katalogeintrag ein Tool; `bots_ask_bot` wartet wie `assistant_send_message` bis zur Antwort oder 60 s.
- **Abnahme:** Vertragstest; ein Bot-Client bekommt `forbidden`; Warten und Stand mit `after`.
