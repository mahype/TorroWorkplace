# Aufgaben: KI-Modellwahl mit zwei Plätzen

Ziel: Statt Modelle je Funktion zu suchen, gibt es zwei **Plätze**: *Chat* (Assistent und Bots) und *Sortieren*
(`decide`: Mail einordnen, später weitere kleine Entscheidungen). Torro misst den Rechner, schlägt je Platz ein
Modell vor (lokal zuerst), lädt fehlende lokale Modelle über Ollama herunter und lässt jede Mail-Erlaubnis je Platz
geben. Sortieren geht auch mit einem gewöhnlichen Chat-Modell (Ersatzweg). Konzept: [ki.md](../ki.md) (wird mit
MW.9 nachgezogen), Issue: #325. Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`,
Requests: Skill `torro-api`, Oberfläche: Skill `torro-design`.

Gemeinsam für alle Aufgaben:
- **Namen:** Platz `torro_core::ai::AiSlot` mit `Chat` und `Decide` (serde `chat`, `decide`); in der Oberfläche
  „Chat“ und „Sorting“. Einstellungsfelder `aiSlots` und `decideMailToCloud`.
- **Alles ist frei wählbar, Torro schlägt nur vor.** Ein Vorschlag setzt nie etwas von selbst.
- **Erlaubnis je Platz:** `mail_to_cloud` bleibt der Schalter für *Chat* (Assistent und Bots, Bedeutung wie
  bisher). Neu `decide_mail_to_cloud` für *Sortieren*: Er folgt der Wahl – ein Modell anderswo für *Sortieren* zu
  setzen setzt ihn, ein Modell hier nimmt ihn zurück. Einschalten nur Menschen, ausschalten jeder Client mit Rechten.
  **Ein Modell anderswo für Mail-Funktionen setzt nur ein Mensch** – als Platz *Sortieren* wie als Ausnahme
  `inbox.categorize` –, auch wenn die Erlaubnis schon an ist: Die Zustimmung gilt einem Anbieter, kein Agent leitet
  sie auf einen anderen um. Geprüft wird weiter zweimal: beim Setzen und beim Aufruf.
- **Eigene Zuordnung je Funktion bleibt Ausnahme** (`ai.set_assignment`): Sie geht dem Platz vor und bleibt
  unberührt, wenn sich der Platz ändert. Ohne Zuordnung nimmt eine Funktion das Modell ihres Platzes
  (`AiFunction::slot`).
- **Übernahme beim ersten Laden:** `assistantDefault` wird zum Platz *Chat*; die eingeschaltete Zuordnung
  `inbox.categorize` wird zum Platz *Sortieren* und als Ausnahme gelöscht (eine ausgeschaltete bleibt: Sortieren
  bleibt aus); `decideMailToCloud` fehlt → `mailToCloud`, aber nur, wenn dabei ein Platz *Sortieren* übernommen
  wurde, sonst `false`. `settings.set_setting assistant_default` bleibt als
  Alias und schreibt den Platz *Chat*; das Feld `assistantDefault` steht nicht mehr in `settings.get_settings`.
- Abhängigkeiten nur, was der Workspace hat (`sysinfo`, `winreg`, `toml`, `reqwest`, `httpmock`). Plattformcode
  hinter `cfg`. Tests ohne Netz, ohne echte Hardware, ohne laufendes Ollama.
- Nutzertexte englisch mit `tr()`/`trf()`; die Aufgaben laufen nacheinander, jede trägt ihre Übersetzungen selbst in
  `crates/torro-core/src/i18n/de.rs` ein.

```
MW.1 (Gerüst) ─┬─ MW.2 (Plätze wirken) ── MW.3 (Sortieren per Chat) ──┐
               ├─ MW.4 (Rechner messen) ──────────┐                     │
               └─ MW.5 (Empfehlungsliste) ─┬──────┴─ MW.6 (ai.recommend) ┼─ MW.8 (Oberfläche) ── MW.9 (Doku, Prüfung)
                                           └─ MW.7 (Herunterladen) ──────┘
```

## Oberfläche, API, MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Vorschlag lesen | Steuerzentrale › AI (Einrichtung: #326) | `Request::AiRecommend` · `ai.recommend` · Read | `ai_recommend` |
| Plätze lesen | Steuerzentrale › AI, Assistant | `ai.list_assignments` (neues Feld `slots`) · Read; `settings.get_settings` (`aiSlots`) · Read | `ai_list_assignments`, `settings_get_settings` |
| Platz setzen | Steuerzentrale › AI, Assistant, Settings › Categorization | `Request::SetAiSlot` · `ai.set_slot` · Admin; ein Modell anderswo für *Sortieren* nur Menschen | `ai_set_slot` (Freigabe) |
| Sortieren aus/an | Settings › Categorization (Schalter) | `ai.set_assignment inbox.categorize` mit `enabled: false` bzw. `engine: null` · Admin | `ai_set_assignment` (Freigabe) |
| Herunterladen | „Use suggestion“, „Download“ | `Request::PullAiModel` · `ai.pull_model` · Admin | `ai_pull_model` (Freigabe) |
| Stand der Downloads | dieselben Orte | `Request::AiPulls` · `ai.list_pulls` · Read | `ai_list_pulls` |
| Download abbrechen | „Cancel“ | `Request::CancelAiPull` · `ai.cancel_pull` · Write | `ai_cancel_pull` |
| Erlaubnis *Chat* | Schalter unter AI | `settings.set_setting mail_to_cloud` · Admin, einschalten nur Menschen | `settings_set_setting` nur `false` |
| Erlaubnis *Sortieren* | folgt der Wahl bei *Sortieren* | `settings.set_setting decide_mail_to_cloud` · Admin, einschalten nur Menschen; `ai.set_slot` setzt sie mit | `settings_set_setting` nur `false` |

## Aufgaben

### MW.1 Gerüst: Typen, Einstellungen, Requests, Katalog, MCP
- [x] erledigt
- **Modell:** Opus (Schnittstellen über vier Crates, Übernahme alter Werte) · **Braucht:** – · **Dateien:**
  `crates/torro-core/src/ai/{mod,hardware}.rs`, `crates/torro-core/src/settings.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{core,mcp,approvals,settings_service}.rs`, Leser von `assistant_default`
  (`crates/torroassistant/src/{app,chat}.rs`, `crates/torro-settings-ui/src/{assistant/general,assistant/mod,demo,lib}.rs`,
  `crates/torroworkplace/src/ui/setup/{mod,pages}.rs`), `docs/api-reference.md`
- `torro_core::ai`: `AiSlot { Chat, Decide }` mit `ALL`, `key()`, `label()` („Chat“, „Sorting“);
  `AiFunction::slot()` (`InboxCategorize` → `Decide`). `SlotModel { engine, model: Option, effort: Option }`:
  der heutige Inhalt von `torro_core::settings::AssistantDefault` samt `MAX_ID_BYTES`, `from_value`, `to_value`;
  `AssistantDefault` bleibt als `pub type AssistantDefault = SlotModel`.
- Neues Modul `torro_core::ai::hardware`, nur Typen (serde, `JsonSchema`): `Hardware { cpu: Option<String>,
  memory_bytes: Option<u64>, gpu: Option<Gpu>, unified_memory: bool }`, `Gpu { name: Option<String>, vram_bytes:
  Option<u64> }`. Was nicht messbar ist, ist `None`.
- `Settings`: `ai_slots: AiSlots { chat: Option<SlotModel>, decide: Option<SlotModel> }` (`aiSlots`, nachsichtig
  gelesen wie `assistant_default`), `decide_mail_to_cloud: Option<bool>` (`decideMailToCloud`), Methode
  `mail_to_cloud_in(AiSlot) -> bool`. Feld `assistant_default` umbenennen in `legacy_assistant_default`
  (`rename = "assistantDefault"`, nur gelesen, `skip_serializing`), damit der Compiler jeden Leser findet.
  `Settings::take_over_old_ai(&mut self)`: die drei Übernahmen aus dem Kopf dieser Datei; aufgerufen in beiden
  `load_from` (`torro-core/src/settings.rs`, `torroworkplace/src/settings_service.rs`) nach `fill_unnamed_parts`.
  Alle Leser von `assistant_default` auf `ai_slots.chat` umstellen (Verhalten gleich).
- `SettingKey::DecideMailToCloud` (bool) mit Doc wie `MailToCloud`; in `Request::kept_for_people` wie
  `MailToCloud`. `SettingKey::AssistantDefault` schreibt `ai_slots.chat` (`settings_service.rs`, `Change`), Doc:
  Alias von `ai.set_slot chat` ohne dessen Prüfung (auch nicht, ob das Modell eingeschaltet ist), bleibt für ältere
  Clients. Unbedenklich für die Datenregel: keine Funktion mit Mail hat den Platz *Chat*, Assistent und Bots prüfen
  `mail_to_cloud` beim Aufruf.
- Requests (Doc mit Antwort und Fehlercodes): `SetAiSlot { slot, engine: Option, model: Option, effort: Option }`
  → `Response::AiAssignments`; `AiRecommend` → `Response::AiRecommendation { recommendation }`;
  `PullAiModel { account, model }`, `AiPulls`, `CancelAiPull { id }` → `Response::AiPulls { pulls }`.
  `Response::AiAssignments` bekommt `slots: Vec<AiSlotInfo>` (`#[serde(default)]`): Platz, Wort, `SlotModel`
  (geflacht), `mail_to_cloud`. Typen in `torro-ipc/src/lib.rs` neben `AiAssignmentInfo`: `AiRecommendation
  { hardware, tier: String, tier_label: String, slots: Vec<SlotSuggestion> }`, `SlotSuggestion { slot, engine:
  Option, provider: AiProvider, model: Option, local: bool, seat: String, download_bytes: Option<u64>, by_chat: bool,
  reason: String }`, `AiPull { id, account, model, state: running|done|failed|cancelled, completed: Option<u64>,
  total: Option<u64>, error: Option<String> }`.
- Katalog `crates/torro-ipc/src/api.rs`, Bereich `Area::Ai`, alle `Mcp::Tool`: `ai.set_slot` Admin, `ai.recommend`
  Read, `ai.pull_model` Admin, `ai.list_pulls` Read, `ai.cancel_pull` Write. Beschreibungen nach `torro-api`, beim
  Herunterladen: nur Ollama auf diesem Rechner, Größe kommt aus `ai.recommend`. `GET_SETTINGS`-Beschreibung:
  `aiSlots`, `decideMailToCloud` statt `assistantDefault`; `settings.set_setting` nennt `decide_mail_to_cloud`.
- MCP-Tools in `mcp.rs` je ein Request: `ai_set_slot`, `ai_recommend`, `ai_pull_model`, `ai_list_pulls`,
  `ai_cancel_pull`. Freigabe-Zusammenfassungen mit Namen aus dem Kern, nicht aus dem Text des Clients: in
  `core.rs` ein Facts-Zweig für `SetAiSlot` und `PullAiModel` neben dem für `Request::SetModelEnabled`
  (`ai.label_of(engine)`, `ai.described(engine, model)`), in `approvals.rs` Sätze wie `model_switch_summary` mit
  „runs on this computer“ bzw. „runs elsewhere“: „Let {engine} ({model}) be the model for {slot}“, „Let no model be the
  model for {slot}“ (`engine: None`), „Download {model} ({size}) to {engine}“ (Größe aus der Liste, MW.5).
- Handler in `core.rs` vorerst: `SetAiSlot` und die drei Pull-Requests `unsupported`, `AiRecommend` `unsupported`;
  `AiAssignments` füllt `slots` aus den Einstellungen.
- `TORRO_BLESS=1 cargo test -p torro-ipc`, `docs/api-reference.md` mitcommitten.
- Abnahme: Prüfbefehle grün; Tests: Übernahme aller drei Altwerte (Datei mit `assistantDefault`, Zuordnung an/aus,
  ohne `decideMailToCloud`), Datei ohne alte Werte bleibt gleich, `assistantDefault` wird nicht mehr geschrieben;
  `kept_for_people` für `decide_mail_to_cloud true`; Vertragstest Tool ↔ Katalog grün.

### MW.2 Plätze wirken: setzen, zurückfallen, Erlaubnis je Platz
- [x] erledigt
- **Modell:** Opus (Datenregel, Rechte, Freigabepfad) · **Braucht:** MW.1 · **Dateien:**
  `crates/torroworkplace/src/{core,settings_service}.rs`, `crates/torroworkplace/src/account/ai.rs`,
  `crates/torroworkplace/src/categorize.rs`
- `Core::set_ai_slot(slot, wanted, client: bool)`: prüft wie `Core::set_ai_assignment` (Konto da, Modell
  angeboten, eingeschaltet über `model_off_refusal`, für `Decide` ein Modell, das entscheidet **oder** Werkzeuge
  kann, siehe MW.3 – bis dahin nur `decision == Some(true)`); `engine: None` leert den Platz. Ort über
  `assistant::place_of`. Für `Decide`: Modell anderswo und `client` → `forbidden` mit `Lack::ForPeople`
  (Muster `set_model_enabled`); sonst schreibt derselbe Aufruf `decide_mail_to_cloud` = „anderswo“. Speichern über
  neue `SettingsStore::set_ai_slot` (einziger Weg, wie `set_ai_assignment`). Antwort wie `ai_assignments`.
- `Core::turns_on_elsewhere` erweitern um `SetAiSlot { slot: Decide, … }` und `SetAiAssignment` einer Funktion mit
  `reads_mail()` und `enabled`, jeweils mit Modell anderswo (`runs_elsewhere`); ohne Modell zählt der Sitz des Kontos
  (`provider.seat() != Seat::ThisComputer`), wie in `set_ai_assignment`. `switches_writable()` nur für
  `SetModelEnabled` aufrufen, nicht für die neuen Fälle. So wird ein Client vor der Freigabe abgewiesen; im Handler
  (`set_ai_slot`, `set_ai_assignment` mit neuem Parameter `client`) noch einmal mit der frischen Liste
  (`models(.., true)`, dann `runs_elsewhere`) wie in `set_model_enabled`, damit eine Freigabe nicht daran vorbeiläuft.
  Ein Mensch, der eine solche Ausnahme setzt, setzt `decide_mail_to_cloud` mit wie beim Platz.
  `switches_on_for_people`: `UpdateSettings` schaltet `decide_mail_to_cloud` ein.
- `SettingsStore::update_keeping_hidden`: `ai_slots` bleibt wie gespeichert, `decide_mail_to_cloud` nur
  gespeichert && neu.
- Zurückfallen: `decider_of` nimmt die Zuordnung, wenn eine gespeichert ist (auch ausgeschaltet = aus), sonst den
  Platz `Decide` als bereite Zuordnung; Datenregel mit `mail_to_cloud_in(Decide)` statt `mail_to_cloud`. Ebenso
  `category_job_decider`, `set_ai_assignment` (Prüfung und `considered` mit der Erlaubnis von `function.slot()`),
  `keep_used_models_on` (beide Plätze statt `assistant_default`), `Automatic::for_function`.
- Assistent und Bots: `DataRule` liest weiter `mail_to_cloud` (= Erlaubnis *Chat*); nachgesehen, sonst nichts
  nachzuziehen. Neuer Chat startet mit `ai_slots.chat` (schon MW.1).
- Abnahme: Kern-Tests (Temp-Verzeichnisse, Fake-Konten wie bei den bestehenden `set_ai_assignment`-Tests): Platz
  setzen und leeren; Client setzt *Sortieren* auf Modell anderswo → `forbidden`, App-Schlüssel → gesetzt und
  Erlaubnis an; lokales Modell nimmt sie zurück; Client mit Erlaubnis an setzt die Ausnahme `inbox.categorize` auf
  ein anderes Modell anderswo → `forbidden`, auch nach Freigabe; Einordnen ohne Zuordnung nimmt den Platz; eigene Zuordnung geht
  vor und bleibt beim Platzwechsel; Erlaubnis aus → Modell anderswo wird beim Aufruf nicht gefragt (`NotLocal`).

### MW.3 Sortieren per Chat-Modell (Ersatzweg)
- [x] erledigt
- **Modell:** Opus (Prompt-Grenze, strenges Lesen fremder Antworten) · **Braucht:** MW.2 · **Dateien:**
  `crates/torro-core/src/ai/{mod,decide_chat}.rs`, `crates/torroworkplace/src/{categorize,core}.rs`,
  `crates/torroworkplace/src/account/ai.rs`, `crates/torro-settings-ui/src/mail.rs`
- Neues reines Modul `torro_core::ai::decide_chat` (ohne Netz): `request(&DecisionRequest, effort) ->
  ChatRequest` übersetzt die Fragen in ein Werkzeug `answer` (Parameter-Schema: je `Choice` ein `enum` der
  Optionen, `Noul` boolean, `Score` integer 0..Stufen-1, alle Pflicht), Systemtext aus Anweisungen und Kriterien,
  der Zustand als Nachricht des Nutzers (der Zaun aus `categorize::state_of` bleibt), `max_tokens` klein.
  `read(&ChatReply, &DecisionRequest) -> Result<DecisionReply, AiError>`: Argumente des Werkzeugaufrufs `answer`,
  sonst ein JSON-Objekt im Text; nichts lesbar → `AiError::Format`. Eine Option, die nicht angeboten war, oder ein
  falscher Typ lässt die Frage weg (→ `other` in `categorize::read`), nie erfunden. Keine Konfidenz vom Chat-Modell:
  `Answer::Choice` mit `confidence` 1.0 und markiert über ein neues Feld `DecisionReply::by_chat: bool`.
- `categorize::read`/`decide`: bei `by_chat` gilt `CONFIDENCE_MIN` nicht, `MailCategory::confidence` ist `None`;
  die Kategorie bleibt vom Modell (Sparkles) wie bisher.
- Wer wie entscheidet: Modell mit `decision == Some(true)` direkt (`AiAccounts::decide`), sonst ein Modell mit
  `tools != Some(false)` über `AiAccounts::provider(..).chat` und `decide_chat`. `categorize::pick_model`: erst
  ein Entscheidermodell, dann ein Chat-Modell. `set_ai_assignment`/`set_ai_slot` nehmen für Funktionen und Platz
  `Decide` beide Arten an; Fehlertext für ein Modell, das keins von beiden kann. `AiAssignmentInfo::needs_decision`
  bleibt `true`, Doc: entscheidet direkt oder über den Ersatzweg.
- `crates/torro-settings-ui/src/mail.rs` (Settings › Categorization): bietet auch Chat-Modelle an, mit
  „answers in a fixed format, slower“ als Zusatz; Erlaubnis aus `mail_to_cloud_in(Decide)` statt `mail_to_cloud`.
  Die Modellwahl dort schickt `ai.set_slot decide` (statt `SetAiAssignment`), damit sie nach der Übernahme keine neue
  Ausnahme anlegt. Der Schalter „aus“ schickt `ai.set_assignment inbox.categorize` mit dem Konto und Modell des
  Platzes und `enabled: false`, „an“ dasselbe mit `engine: null` (Ausnahme weg, der Platz gilt). Steht eine Ausnahme
  mit eigenem Modell, zeigt die Seite sie als solche („Only for sorting mail“).
- Abnahme: Tests für `decide_chat`: gültige Antwort (Werkzeug), gültige Antwort als Text, Option nicht angeboten,
  falscher Typ, abgeschnittene Antwort (`Finish::Length`, ungültiges JSON), Text mit `</message>` bleibt im Zaun;
  Kern-Test mit `httpmock` (OpenAI-kompatibler Server ohne `systemone`) ordnet eine Mail ein; Kategorie ohne
  Konfidenz.

### MW.4 Rechner messen
- [x] erledigt
- **Modell:** Opus (Plattformcode für drei Systeme) · **Braucht:** MW.1 · **Dateien:**
  `crates/torroworkplace/src/account.rs`, `crates/torroworkplace/src/account/hardware.rs`, `crates/torroworkplace/tests/fixtures/hardware/`
- `trait Probe: Send + Sync { fn measure(&self) -> BoxFuture<'_, Hardware> }` und `SystemProbe`: Arbeitsspeicher
  und CPU-Name über `sysinfo`; macOS mit `target_arch = "aarch64"` → `unified_memory: true`, GPU-Name aus dem
  CPU-Namen, kein VRAM; Linux und Windows: `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`
  (tokio, Zeitlimit 3 s, fehlt → weiter; unter Windows mit `CREATE_NO_WINDOW`, sonst blitzt beim Messen eine Konsole auf); Linux zusätzlich `/sys/class/drm/card*/device/mem_info_vram_total`
  (AMD); Windows ohne NVIDIA: `qwMemorySize` und `DriverDesc` unter
  `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\00NN` (`winreg`). Größte
  Karte gewinnt. Alles hinter `cfg`; was scheitert, bleibt `None`, nie ein Fehler.
- Reine Leser getrennt testbar: `parse_nvidia_smi(&str)`, `read_drm(root: &Path)`; Fixtures unter
  `tests/fixtures/hardware/`. Der Kern misst einmal je Lauf und merkt es sich (MW.6).
- Abnahme: Tests der Leser (eine, zwei, keine Karte, kaputte Zeile); `cargo build` auf allen drei Zielen
  über `cargo check --target` soweit installiert, sonst ehrlich melden.

### MW.5 Empfehlungsliste als Daten
- [x] erledigt
- **Modell:** Opus (Stufenlogik, Daten prüfen) · **Braucht:** MW.1 · **Dateien:**
  `crates/torroworkplace/assets/ai/recommendations.toml`, `crates/torroworkplace/src/account.rs`, `crates/torroworkplace/src/account/recommend.rs`
- TOML, mit `include_str!` eingebunden wie `assets/extensions/catalog.toml`: `[[tier]]` vom stärksten zum
  schwächsten mit `id`, `label` (englisch, durch `tr()` beim Anzeigen), Bedingungen `min_memory_gb`,
  `min_vram_gb`, `unified` (optional). Erste Stufen (Schätzung, beim Umsetzen prüfen): `mac_large` (unified ≥ 64),
  `gpu` (VRAM ≥ 12 und RAM ≥ 32), `large` (unified ≥ 32 oder RAM ≥ 16 ohne unified), `medium` (RAM ≥ 8), `small`
  (sonst und unbekannt). `[[local]]`: `tier`, `slot`, `provider` (`ollama` oder `strata`), `model`, `size_bytes`,
  `license`, `open_weights = true`. `[[cloud]]`: `provider`, `slot`, `model_prefix` (erstes Modell des Kontos mit
  diesem Anfang; Chat ein mittleres, Sortieren das kleinste). Kandidaten (Namen und Größen bei Ollama nachsehen):
  Chat `qwen3:4b` (medium), `qwen3:14b` (large, gpu ohne Strata), `qwen3:32b` (mac_large); Sortieren `nimble`
  (Entscheidermodell) ab `small`. `small` hat kein lokales Chat-Modell.
- `recommend.rs` rein: `tier_of(&Hardware, &List)`, `suggest(&Hardware, &List, accounts: &[Connected], mail:
  bool) -> AiRecommendation`; `Connected { id, provider, local: bool, models: Option<Vec<AiModel>> }`. Regeln:
  lokal vor Cloud; Strata-Eintrag nur mit verbundenem Strata, sonst der Ollama-Eintrag derselben Stufe; ohne
  verbundenes Ollama trotzdem der lokale Vorschlag mit `engine: None` und Grund „Needs Ollama“; Cloud nur, wenn
  die Stufe kein lokales Modell für den Platz hat oder kein lokaler Server da ist, Anbieter in der Reihenfolge von
  `AiProvider::ALL`; *Sortieren* nur mit verbundener Mail; `download_bytes` aus der Liste, wenn das Ollama-Konto
  das Modell nicht listet; `by_chat`, wenn der Vorschlag für *Sortieren* kein Entscheidermodell ist.
- Abnahme: Tests: Datei liest, jeder Eintrag nennt eine bekannte Stufe, einen Platz und eine Größe > 0, jeder lokale
  hat `open_weights = true` und eine Lizenz aus einer offenen Liste (`apache-2.0`, `mit`, `qwen`, `llama`, `gemma`),
  jede Stufe hat ein Modell für *Sortieren*; unbekannte Hardware → `small`; Beispiele der Mockups (8 GB ohne
  Ollama mit Cloud-Konto, RTX 12 GB/64 GB mit Ollama, Mac 64 GB) geben den erwarteten Vorschlag.

### MW.6 `ai.recommend` im Kern
- [x] erledigt
- **Modell:** Opus · **Braucht:** MW.2, MW.4, MW.5 · **Dateien:** `crates/torroworkplace/src/core.rs`,
  `crates/torroworkplace/src/account/ai.rs`
- `Core` hält einen `Box<dyn Probe>` (Tests: fester Wert) und das Ergebnis der ersten Messung. `AiRecommend`:
  verbundene KI-Konten mit ihren Modellen (`AiAccounts::models(id, true)`, Fehler → `models: None`), Mail
  verbunden ja/nein, für einen Client nur aus den Mail-Konten, die seine Rechte erreichen (`Reach` wie bei
  `inbox.list_categories`; sonst verriete der Vorschlag für *Sortieren*, dass Mail verbunden ist), dann
  `recommend::suggest`. Liest nur, speichert nichts.
- Abnahme: Kern-Test mit Fake-Probe und `httpmock`-Ollama: Vorschlag mit und ohne vorhandenes Modell
  (`download_bytes`), ohne Mail kein Vorschlag für *Sortieren*, Client ohne Mail-Rechte ebenso.

### MW.7 Herunterladen über Ollama
- [x] erledigt
- **Modell:** Opus (laufende Aufgaben, Abbrechen) · **Braucht:** MW.1, MW.5 · **Dateien:**
  `crates/torro-core/src/ai/{mod,pull}.rs`, `crates/torroworkplace/src/account.rs`, `crates/torroworkplace/src/account/{ai,pull}.rs`,
  `crates/torroworkplace/src/core.rs`
- `torro_core::ai::pull` rein: Body für `POST <root>/api/pull` (`{"model", "stream": true}`, `root` über
  `openai::ollama_root`), Lesen einer NDJSON-Zeile → `PullEvent { completed, total, done, error }`; ein `error`
  des Servers wird nicht weitergereicht, sondern zu einem eigenen Satz.
- Herunterladen nur Modelle, die als lokaler Ollama-Eintrag in der Empfehlungsliste (MW.5) stehen, sonst
  `unsupported`: Ein freier Name könnte einen fremden Registry-Host nennen (`host.example/x/y`), von dem Ollama lädt.
- `account/pull.rs`: `Pulls` (Mutex, Kennung je Download, höchstens ein laufender je Konto und Modell → `busy`),
  Aufgabe streamt mit dem `reqwest`-Client der Konten, Abbrechen über das `AbortHandle` (Ollama bricht ab, wenn die
  Verbindung endet), fertige Einträge bleiben bis zum Ende des Laufs sichtbar. Nur `AiProvider::Ollama` an einer
  Adresse dieses Rechners (`is_this_computer`), sonst `unsupported`; unbekanntes Konto `not-found`. Nach Erfolg die
  Modell-Liste des Kontos neu lesen.
- Handler `PullAiModel`, `AiPulls`, `CancelAiPull` in `core.rs` statt der Platzhalter aus MW.1.
- Abnahme: Tests mit `httpmock` (Fortschritt in mehreren Zeilen, Erfolg, Fehlerzeile, Abbrechen mitten im Strom,
  zweiter Start → `busy`, Konto nicht lokal → `unsupported`, Name nicht in der Liste → `unsupported`).

### MW.8 Oberfläche: Steuerzentrale › AI und Assistant
- [x] erledigt
- **Modell:** Opus (Oberfläche mit mehreren Requests nacheinander) · **Braucht:** MW.3, MW.6, MW.7 · **Dateien:**
  `crates/torroworkplace/src/ui/{accounts,mod,launch}.rs` (`Data`, Demo-Daten und `--open`-Zustände der Steuerzentrale),
  `crates/torro-settings-ui/src/assistant/general.rs`, `crates/torro-core/src/i18n/de.rs`
- Steuerzentrale › AI (`accounts.rs`, Liste mit `Group::Ai`): über „Data rule“ eine Gruppe „Models“ mit zwei Zeilen
  *Chat* und *Sorting* (`widgets::group`, `setting_row`): gewähltes Modell als `pick_list` aller eingeschalteten
  Modelle, die den Platz bedienen (Chat: Werkzeuge nicht `false`; Sorting: entscheidet oder Werkzeuge, mit Zusatz
  „decides directly“ bzw. „answers in a fixed format“), „None“ zum Leeren. Darunter je Platz der Vorschlag (Modell
  · Ort · Grund) mit „Use suggestion“ bzw. „Use suggestion · 5.2 GB“ (`torro_gui::units::format_bytes`); kein Dialog.
  Kopfzeile der Gruppe: gemessener Rechner wie im Mockup („GeForce RTX 4070 · 12 GB video memory · 64 GB memory“).
- „Use suggestion“: bei `download_bytes` `ai.pull_model`, Fortschritt in der Zeile (`widgets::progress_bar`
  bzw. Prozent) mit „Cancel“ (`ai.cancel_pull`), `ai.list_pulls` im Sekundentakt nur, solange einer läuft; danach
  `ai.set_model_enabled` (falls aus) und `ai.set_slot`. Ohne Ollama steht statt des Knopfs der Grund.
- Ist *Sorting* ein Modell anderswo oder ein solcher Vorschlag sichtbar, sagt die Zeile im selben Moment, wohin Mail
  geht: „Mail goes to {provider} ({seat}) for sorting only. The chat still gets no mail.“ (zweiter Satz nur, solange
  die Erlaubnis *Chat* aus ist). „Data rule“: der Schalter heißt „Chat may read mail in the cloud“, Unterzeile wie
  bisher; darunter die Erlaubnis *Sorting* als Text mit „Keep mail here“ (`set_setting decide_mail_to_cloud false`), solange
  sie an ist.
- Leer-, Lade- und Fehlerzustand: ohne KI-Konto keine Gruppe; Vorschlag lädt ruhig; Fehler mit „Retry“.
- Assistant › General (`general.rs`): liest `ai_slots.chat`, schreibt `ai.set_slot chat` statt `assistant_default`.
- Demo der Steuerzentrale (`ui/mod.rs`, Zustand `ai-models` in `launch.rs`): Vorschlag, laufender Download und ein Platz *Sorting* in der Cloud;
  `--section ai` zeigt die Gruppe, `--open ai-models` den laufenden Download.
- Abnahme: Tests der Nachrichten (Requests in der richtigen Reihenfolge, Abbrechen, Fehler eines Schritts hält die
  Kette an); Sichtprüfung `torroworkplace ui --demo --section ai` und `--open ai-models` in allen Größen und drei Designs, hell und dunkel
  (Skill `torro-design`, Agent `torro-design-review`).

### MW.9 Doku und Prüfung
- [x] erledigt
- **Modell:** Haiku · **Braucht:** MW.8 · **Dateien:** `docs/ki.md`, `docs/settings.md`, `docs/kategorien.md`,
  `docs/api.md`
- `docs/ki.md`: Abschnitt „Plätze“ (Chat, Sortieren, Zurückfallen, Ausnahme je Funktion), „Empfehlung je Rechner“
  (Stufen, Liste als Daten mit jeder Version, lokal zuerst), „Erlaubnis je Platz“ (ersetzt den einen Schalter in
  Grundsatz 4), Ersatzweg beim Sortieren; Tabelle „API und MCP“ um die neuen Befehle. `docs/kategorien.md`:
  Reihenfolge feste Merkmale → Regeln → Modell (Entscheider oder Chat) → `other`. `docs/settings.md` KI: `aiSlots`,
  `decideMailToCloud`. `docs/api.md` „Was nie nach außen geht“: `decide_mail_to_cloud` neben `mail_to_cloud`.
- Agent `torro-api-review` über alle Änderungen; Befunde beheben.
- Abnahme: Prüfbefehle grün, auch `TORRO_LANG=de`; `torro-api-review` ohne Befund.
