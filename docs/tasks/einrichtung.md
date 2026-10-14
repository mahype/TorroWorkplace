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
- [ ] erledigt
- **Modell:** Opus · **Braucht:** – (Entscheidung 2: Varianten je Anbieter) · **Dateien:** `crates/torro-core/src/ai/**`,
  `crates/torroworkplace/src/account/ai*.rs`, `crates/torroworkplace/src/ui/wizard.rs`
- Katalog mit Name, Adresse, Protokoll, Schlüsselseite, Sitz, offenen Gewichten: OpenAI, Google Gemini, Mistral,
  DeepSeek, Qwen, Kimi, GLM, MiniMax (alle OpenAI-kompatibel). Anthropic kommt mit **KI.1** (eigenes Protokoll).
  Adressen bei der Umsetzung prüfen; internationale Endpunkte als Standard.
- **Oberfläche:** Accounts › Add › AI und Schritt AI · **API:** `ConnectAi` mit neuen Anbietern · Admin, nur
  App-Schlüssel · **MCP:** Never (Schlüssel)
- **Abnahme:** `httpmock`-Test je Protokoll; Schlüssel in keiner Antwort und keinem Protokoll.

### W.4 Hülle der Einrichtung
- [ ] erledigt
- **Modell:** Opus · **Braucht:** W.1 · **Dateien:** `crates/torroworkplace/src/ui/setup/**` (neu),
  `crates/torroworkplace/src/ui/{mod,launch}.rs`
- `torroworkplace ui --setup [--step <name>]`; Schrittliste, Kopf, Fuß, drei Größenklassen; Schritte
  Welcome, Components und Done; Demo-Zustände `--demo --open setup-<schritt>`.
- **Oberfläche:** Einrichtung · **API:** `SetSetting { Setup, Component, Language, Appearance }` · **MCP:** gibt es

### W.5 Schritt Nextcloud und Bausteine aus „Add account“
- [ ] erledigt
- **Modell:** Opus · **Braucht:** W.4 · **Dateien:** `crates/torroworkplace/src/ui/{wizard,setup/**}.rs`,
  `crates/torroworkplace/src/ui/connect/**` (neu)
- Die Wege Nextcloud, CalDAV, Mail, GitHub, ntfy, KI aus `wizard.rs` als Bausteine herauslösen, die Dialog und
  Einrichtung gemeinsam nutzen (Verhalten des Dialogs bleibt gleich, seine Tests laufen weiter).
  Schritt Nextcloud mit Liste der Dienste aus `AccountInfo::services`.

### W.6 Schritte Kalender und Aufgaben
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/{calendar,tasks}.rs`
- Sammlungen je Konto mit Schalter (`SetCollectionShown`), „Add a CalDAV calendar“ mit Anbieterwahl im Schritt,
  GitHub mit beiden Wegen.

### W.7 Schritte Mail und Eingang
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/{mail,inbox}.rs`
- TorroMail-Übernahme, Mail-Konto; Eingangsquellen mit Schalter, ntfy, „Quiet during busy events“.

### W.8 Schritt KI
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** W.3, W.5 · **Dateien:** `crates/torroworkplace/src/ui/setup/ai.rs`
- Lokale Suche beim Öffnen, Cloud-Anbieter mit Sitz und Schlüssel in der Zeile, „Use for: Assistant“ mit Modell.

### W.9 Auslöser
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** W.4 · **Dateien:** `crates/torroworkplace/src/{core,daemon}.rs`,
  `crates/torroworkplace/src/ui/{overview,general}.rs`, `crates/torro-gui/src/connect.rs`
- Dienst öffnet die Einrichtung beim ersten Start ohne Konto; Overview-Karte „Finish setup“; General
  „Run setup again“; „Open Settings“ der Apps führt in den passenden Schritt, solange die Einrichtung offen ist.

### W.10 Prüfen
- [ ] erledigt
- **Modell:** Sonnet · Sichtprüfung aller Schritte in allen Größen, hell und dunkel (`torro-design-review`),
  `torro-api-review` über W.1–W.9.
