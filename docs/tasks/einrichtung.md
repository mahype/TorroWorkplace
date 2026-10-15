# Aufgaben: Einrichtung beim ersten Start (W.1–W.10)

Konzept: [einrichtung.md](../einrichtung.md), Entwurf: [prototypes/setup/wizard.html](../prototypes/setup/wizard.html).
Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`, Oberfläche: Skill `torro-design`. Wer
einen Request anfasst, lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`.

> **Stand 2026-10-14: freigegeben.** Entscheidungen im Konzept, Abschnitt „Entscheidungen“.

```
W.1 ──┬── W.2 (Komponenten wirken)
      ├── W.4 (Hülle) ──┬── W.5 (Nextcloud) ── W.6 (Kalender, Aufgaben)
      │                 ├── W.7 (Mail, Eingang)
      │                 └── W.8 (KI) ◄── W.3 (KI-Katalog)
      └── W.9 (Auslöser) nach W.4
W.10 Sichtprüfung und API-Prüfung am Ende
```

### W.1 Gerüst: Stand, Komponenten, Nextcloud-Dienste
- [x] erledigt (Contacts erkennt W.1 noch nicht: CardDAV wird nicht abgefragt; kommt, wenn die Suite Kontakte nutzt)
- **Modell:** Opus · **Dateien:** `crates/torro-core/src/settings.rs`, `crates/torro-ipc/src/{lib,api}.rs`,
  `crates/torroworkplace/src/{settings_service,pim}.rs`, `crates/torro-nextcloud/src/capabilities.rs`; mechanisch
  `services: None` in jedem `AccountInfo { … }` (`torro-ipc/src/pim.rs`, `torroworkplace/src/{account,core}.rs`,
  `account/{ai,ntfy}.rs`, `mail/service.rs`, `ui/{accounts,agents,glance,launch,overview,wizard}.rs`) und die zwei
  neuen Felder im Test von `ui/general.rs`
- `Settings::setup` (`state: pending|skipped|done`, `version`, `Setup::VERSION` = 1) und `Settings::components_off`
  (nur die abgeschalteten, JSON `componentsOff`; Standard: alle an); `SettingKey::Setup` (`"pending"`, `"skipped"`,
  `"done"`) und `SettingKey::Component` (`{"component": "mail", "enabled": false}`, Form wie `ListOrder`). Unbekannte
  Komponenten und ein kaputter Stand kosten die Datei nicht.
- `AccountInfo::services` (optional, `ServerService`, unbekannte Werte lesen als `other`): für Nextcloud und CalDAV
  `calendars`/`tasks` nach den Sammlungen, für Nextcloud dazu `notifications`, `talk`, `push`, `deck`, `notes` aus den
  Capabilities. Gelesen nach einem Sync, der geklappt hat, höchstens stündlich; `None` vor dem ersten Sync.
- **Oberfläche:** keine (W.4 ff.)
- **API:** `SetSetting { key: Setup | Component }` · `settings.set_setting` · Admin; `AccountInfo::services` in
  `accounts.list_accounts` · Read
- **MCP:** `settings_set_setting`, `accounts_list_accounts` (gibt es)
- **Abnahme:** alte `settings.json` ohne die Felder lädt mit Standard; Tests für die Dienste mit einem Fake-Server
  (mit Talk und Deck, ohne Capabilities, CalDAV-Konto wird nie nach Capabilities gefragt).

### W.2 Komponenten wirken
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** W.1 · **Dateien:** `crates/torro-gui/src/shell.rs` (App-Umschalter),
  `crates/torroworkplace/src/tray/menu.rs`, `crates/torroworkplace/src/ui/{apps,overview}.rs`,
  `crates/torroworkplace/src/{inbox,mail/service}.rs`
- Abgeschaltete Komponenten fehlen in Umschalter, Tray und Overview; Seite Apps mit Schalter je Komponente;
  keine Hintergrundarbeit, die nur ihr dient. Die Requests antworten weiter (Entscheidung 1).
- **Oberfläche:** Apps (Schalter) · **API/MCP:** wie W.1

### W.3 KI-Anbieter-Katalog
- [x] erledigt (offen: Z.ai soll laut einer fremden Quelle keine Modell-Liste haben; dann scheitert das Verbinden
  von GLM an der Prüfung über `/models` – mit einem echten Schlüssel nachprüfen)
- **Modell:** Opus · **Braucht:** – (Entscheidung 2: Varianten je Anbieter) · **Dateien:** `crates/torro-core/src/ai/{mod,openai}.rs`,
  `crates/torroworkplace/src/account/ai.rs`, `crates/torroworkplace/src/assistant/data_rule.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`, `crates/torro-ipc/src/{lib,api}.rs`, `crates/torro-core/src/i18n/de.rs`
- `AiProvider` mit OpenAI, Gemini, Mistral, DeepSeek, Qwen, Kimi, GLM, MiniMax (alle OpenAI-kompatibel; Anthropic kommt
  mit **KI.1**). Je Anbieter: Adresse (international, geprüft am 2026-10-15), China-Adresse wo vorhanden, Schlüsselseite,
  Sitz (`Seat`: dieser Rechner, eigener Server, Cloud USA/EU/China), offene Gewichte. Den Denkaufwand bekommen nur
  OpenAI und Gemini (`reasoning_effort`); die anderen Cloud-Anbieter bekommen kein unbekanntes Feld.
- Dialog „Add account“ › AI: alle Cloud-Anbieter mit Sitz; ein Schlüssel-Schritt für jeden, mit „Use the platform for
  mainland China“ wo es eine gibt. Datenregel: jeder Cloud-Anbieter zählt als entfernt.
- **Oberfläche:** Accounts › Add › AI (die Einrichtung folgt in W.8) · **API:** `ConnectAi` mit neuen Anbietern · Admin,
  nur App-Schlüssel · **MCP:** Never (Schlüssel)
- **Abnahme:** `httpmock`-Test für einen Cloud-Anbieter (Prüfung über die Modelle mit Schlüssel, falscher Schlüssel,
  fehlender Schlüssel); Schlüssel in keiner Antwort.

### W.4 Hülle der Einrichtung
- [x] erledigt (Komponenten als Gruppe mit Schaltern statt Karten, wie die Design-Regel für Einstellungsseiten
  es will; Mini und Leiste zeigen weiter die Statusübersicht, „Setup isn't finished“ kommt mit W.9)
- **Modell:** Opus · **Braucht:** W.1 · **Dateien:** `crates/torroworkplace/src/ui/setup/**` (neu),
  `crates/torroworkplace/src/ui/{mod,launch}.rs`
- `torroworkplace ui --setup [--step <name>]`; Schrittliste, Kopf, Fuß, drei Größenklassen; Schritte
  Welcome, Components und Done; Demo-Zustände `--demo --open setup`, `setup-components`, `setup-done`.
  Schritte späterer Aufgaben stehen schon im Typ `Step`, werden aber erst gelistet, wenn `Step::built` sie
  nennt (W.5–W.8 schalten sie dort frei).
- **Oberfläche:** Einrichtung · **API:** `SetSetting { Setup, Component, Language, Appearance }` · **MCP:** gibt es

### W.5 Schritt Nextcloud und der erste Baustein aus „Add account“
- [x] erledigt
- **Modell:** Opus · **Braucht:** W.4 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,mod,launch}.rs`,
  `crates/torroworkplace/src/ui/setup/**`, `crates/torroworkplace/src/ui/connect/**` (neu),
  `crates/torro-core/src/i18n/de.rs`
- Der Nextcloud-Weg ist ein Baustein (`ui/connect/nextcloud.rs`: Adresse, Login Flow v2, Ende aus den Reloads,
  Wartebild). Dialog und Einrichtung nutzen ihn; der Dialog verhält sich wie zuvor, seine Tests laufen weiter.
- Schritt Nextcloud: ohne Konto die Adresse („Connect“, „I don't use Nextcloud“), dann Warten auf den Browser
  („Cancel“), danach die Dienste des Servers aus `AccountInfo::services` und „Add another Nextcloud“.
  Demo: `--open setup-nextcloud`, `setup-nextcloud-address`, `setup-nextcloud-waiting`.
- Entschieden beim Umsetzen: Die übrigen Wege (CalDAV, GitHub, Mail, ntfy, KI) ziehen nicht alle auf einmal um,
  sondern mit der Aufgabe, deren Schritt sie braucht (W.6–W.8). So bleibt jeder Umbau klein und geprüft.
- **API:** `ConnectNextcloud` · Admin · **MCP:** `accounts_connect_nextcloud` (gibt es)

### W.6 Schritte Kalender und Aufgaben
- [x] erledigt (Kalender-Abos per Link kommen später, Entscheidung 5)
- **Modell:** Sonnet · **Braucht:** W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/**`,
  `crates/torroworkplace/src/ui/connect/{mod,caldav,github}.rs`, `crates/torroworkplace/src/ui/{wizard,mod,launch}.rs`,
  `crates/torro-core/src/i18n/de.rs`
- Je Konto (Nextcloud, CalDAV, Mail) seine Kalender bzw. Listen mit Schalter (`SetCollectionShown`, sofort sichtbar);
  „Add a CalDAV account“ als Formular im Schritt (E-Mail, Passwort, unter „Advanced“ Server und Benutzername; der
  Kern findet den Server über die Adresse); auf dem Aufgaben-Schritt GitHub mit beiden Wegen.
- Bausteine: GitHub aus dem Dialog nach `ui/connect/github.rs` (der Dialog nutzt ihn), CalDAV neu in
  `ui/connect/caldav.rs` (der kurze Weg nur für Kalender und Aufgaben; der Mail-Weg des Dialogs bleibt).
  Demo: `--open setup-calendar`, `setup-calendar-caldav`, `setup-tasks`, `setup-tasks-github`.
- **API:** `SetCollectionShown` · Admin; `ConnectCalDav`, `SetGithubToken` · Admin, nur App-Schlüssel; `ConnectGithub` ·
  Admin · **MCP:** `settings_set_collection_shown`, `accounts_connect_github`; CalDAV und Token Never (Geheimnis)

### W.7 Schritte Mail und Eingang
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/**`,
  `crates/torroworkplace/src/ui/{mod,launch}.rs`, `crates/torro-core/src/i18n/de.rs`
- Mail: die Mail-Konten mit Zustand, „Add mail account“ und „Take over from TorroMail“. Eingang: je Quelle der
  verbundenen Konten (Nextcloud-Benachrichtigungen, Talk, GitHub, Mail, ntfy) ein Schalter (`inbox.mute_source`,
  sofort sichtbar), „Subscribe to ntfy topics“, „Quiet during busy events“.
- Entschieden beim Umsetzen: Mail-Weg, TorroMail-Übernahme und ntfy werden **nicht** herausgelöst. Die Einrichtung
  öffnet dafür den Dialog „Add account“ über sich, direkt auf dem passenden Weg. Der Mail-Weg (Suche, Passwort,
  Server, Dienste, TorroMail mit Rückfall aufs Passwort) ist zu groß, um ihn doppelt zu pflegen; so gibt es ihn
  einmal. Demo: `--open setup-mail`, `setup-mail-add`, `setup-inbox`.
- **API:** `ConnectMail`, `ImportTorroMailAccount`, `ConnectNtfy` · Admin, nur App-Schlüssel; `inbox.mute_source` ·
  Write; `SetSetting { QuietWhenBusy }` · Admin · **MCP:** `inbox_mute_source`, `settings_set_setting`; Mail und ntfy
  mit Geheimnis Never

### W.8 Schritt KI
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** W.3, W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/**`,
  `crates/torroworkplace/src/ui/{mod,launch}.rs`, `crates/torro-core/src/i18n/de.rs`
- Beim ersten Öffnen sucht der Schritt Ollama und LM Studio (`FindAiServers`), gefundene mit „Connect“ ohne Schlüssel;
  „Other server“. Cloud-Anbieter in drei Gruppen (offene Modelle, Anbieter, Anbieter in China) mit Sitz und „Add
  key“, das den Dialog „Add account“ beim Schlüssel des Anbieters öffnet (wie Mail in W.7, ein Weg statt zwei).
  „Use for · Assistant“: das KI-Konto, mit dem ein neuer Chat beginnt (`SetSetting { AssistantDefault }`); Modell und
  Aufwand unter Assistant. Demo: `--open setup-ai`.
- **API:** `FindAiServers` · Read, `ConnectAi` · Admin, nur App-Schlüssel, `SetSetting { AssistantDefault }` · Admin ·
  **MCP:** `ai_find_local_servers`, `settings_set_setting`; `ConnectAi` Never (Schlüssel)

### W.9 Auslöser
- [x] erledigt (die Overview-Kachel in Mini und Leiste zeigt die offene Einrichtung noch nicht)
- **Modell:** Haiku · **Braucht:** W.4 · **Dateien:** `crates/torroworkplace/src/{core,daemon}.rs`,
  `crates/torroworkplace/src/ui/{mod,launch,overview,general}.rs`, `crates/torro-gui/src/connect.rs`,
  `crates/{torrocalendar,torrotasks}/src/app.rs`, `crates/torro-core/src/i18n/de.rs`
- Der Dienst öffnet `torroworkplace ui --setup` beim Start mit Tray, solange die Einrichtung nie abgeschlossen oder
  verschoben wurde und kein Konto da ist (`offers_setup`); kopflos nie. Overview: „Finish setup“ (verschoben) bzw.
  „New in setup“ (neue Schritte seit dem letzten Durchlauf). General: „Run setup again“. Die Apps ohne Konto
  (Kalender, Aufgaben) öffnen mit „Open Settings“ `--setup-if-open --step <app>`: die Einrichtung bei ihrem Schritt,
  solange sie nicht fertig ist, sonst die Konten.
- **API/MCP:** nichts Neues (Stand über `settings.get_settings`)

### W.10 Prüfen
- [ ] erledigt
- **Modell:** Sonnet · Sichtprüfung aller Schritte in allen Größen, hell und dunkel (`torro-design-review`),
  `torro-api-review` über W.1–W.9.
