# Aufgaben: Steuerzentrale (erste Version)

Konzept: [settings.md](../settings.md). Regeln: [CLAUDE.md](../../CLAUDE.md), Design: Skill
`torro-design`. Die Steuerzentrale ist `torroworkplace ui` (auch ohne Befehl), Code in
`crates/torroworkplace/src/ui/`.

Gemeinsam für alle Seiten-Aufgaben:
- Jede Seite hat dieselbe Form (`State`, `Message`, `update(..) -> Action`, `view(..)`), siehe
  Doc-Kommentar in `ui/mod.rs`. Seiten sprechen nie selbst mit dem Dienst, sie geben eine
  `Action` zurück (`Send(Request)`, `OpenUrl`, `Select(Section)`).
- Texte englisch über `tr()`/`trf()`. Die deutschen Übersetzungen nennt die Aufgabe im Bericht;
  eingetragen werden sie zentral in `crates/torro-core/src/i18n/de.rs`.
- Seitenaufbau wie TorroWhisper: Gruppen mit Überschrift (`widgets::section_title`), Zeilen,
  darunter ein erklärender Satz in `MUTED`. Status als Symbol + Wort. Destruktives mit
  `widgets::modal` bestätigen. Leere Bereiche: ein Satz, was hier erscheint und was zu tun ist.

## Aufgaben

### S.0 Gerüst
- [x] erledigt
- **Modell:** Opus
- `torro_core::settings` (Typ), `settings_service.rs`, IPC-Befehle (`Settings`, `UpdateSettings`,
  `Accounts`, `RemoveAccount`, `RenameTemplate`, `OpenApp`) mit `AccountInfo`, Kern-Handler
  (Umbenennen und App öffnen fertig), Steuerzentrale-Hülle mit Seitenleiste, schmaler Ansicht,
  Laden über IPC, Seiten-Gerüste.

### S.1 Einstellungen speichern und anwenden
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torro-core/src/settings.rs`, `crates/torroworkplace/src/settings_service.rs`
- `load`/`save` (atomar, fehlende/kaputte Datei → Standard). `update`: speichern, „Start at login“
  als XDG-Autostart-Eintrag anlegen bzw. entfernen.

### S.2 Konten im Kern: Liste, Entfernen, ausgeblendete Sammlungen
- [x] erledigt
- **Modell:** Sonnet (Nextcloud) · **Dateien:** `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/account.rs`
- `accounts()` mit allen Sammlungen; `remove_account` mit Widerruf des App-Passworts auf dem
  Server (OCS); ausgeblendete Sammlungen aus `settings` in `Collections`, `Events`, `Todos` weglassen.

### S.4 Seite Overview
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/overview.rs`

### S.5 Seite Accounts
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`

### S.6 Assistent „Add account“
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/wizard.rs`

### S.7 Seite Workspaces
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/workspaces.rs`

### S.8 Seite Apps
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/apps.rs`

### S.9 Seite General
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/general.rs`

### S.10 Apps und Tray verweisen auf die Steuerzentrale
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torro-gui/src/connect.rs`, `crates/torrocalendar/src/app.rs`, `crates/torrotasks/src/app.rs`, `crates/torroworkplace/src/tray/linux.rs`, `crates/torroworkplace/src/daemon.rs`
- Statt Anmeldung in den Apps: Hinweis + „Open Settings“ (startet `torroworkplace ui --section accounts`).
  Tray: Eintrag „Settings…“.

### S.11 Abschluss
- [x] erledigt
- **Modell:** Opus
- `#![allow(...)]` in `ui/mod.rs` entfernen, Übersetzungen eintragen, Design-Prüfung
  (Agent `torro-design-review`) bei 1100 px, 500 px und 360 px, Fehler beheben.

### S.12 Remote Wipe für Nextcloud-Konten
- [x] erledigt
- **Modell:** Opus (Agent `nextcloud-expert`, Sicherheit) · **Dateien:**
  `crates/torro-pim-caldav/src/nextcloud.rs`, `crates/torroworkplace/src/account.rs`,
  `crates/torroworkplace/src/pim.rs`
- Warum: Nextcloud erwartet von Clients, dass sie ein Fernlöschen unterstützen. Markiert der
  Nutzer oder Admin das Gerät unter „Geräte & Sitzungen“ zum Löschen, lehnt der Server das
  App-Passwort ab, und der Client soll seine lokalen Daten entfernen.
- Antwortet der Server mit 401/403 (`ProviderError::Auth`), prüft der Kern einmal
  `POST /index.php/core/wipe/check` mit `token=<app-password>`. Bei `{"wipe": true}`: Konto,
  App-Passwort im Schlüsselbund und den Zwischenspeicher dieses Kontos löschen, dann
  `POST /index.php/core/wipe/success` mit demselben Token melden und eine Benachrichtigung zeigen
  („Your Nextcloud account was removed from this device at the server's request.“). Sonst (404 oder
  `wipe: false`): wie bisher „sign-in refused“, Nutzer neu anmelden lassen, nicht in Schleife wiederholen.
- Endpunkte vor der Umsetzung in der aktuellen Nextcloud-Doku nachlesen (Skill `nextcloud`).
- **Abnahme:** Tests ohne Netz mit austauschbarer Abfrage (Vorbild `poll_until_granted`): Wipe → alles
  gelöscht und Erfolg gemeldet; kein Wipe → Konto bleibt; Fehler bei der Prüfung → Konto bleibt.
  Manuell gegen die Nextcloud des Nutzers nur nach Rückfrage (löscht das Gerät dort wirklich).

### S.13 Konto- und Sync-Status in `status --json`
- [ ] erledigt
- **Modell:** Opus (erweitert die IPC-Schnittstelle) · **Dateien:** `crates/torro-ipc/src/lib.rs`,
  `crates/torroworkplace/src/{core,cli,pim}.rs`
- Warum: Leisten und Skripte sollen sehen, ob ein Konto verbunden ist und wann zuletzt
  synchronisiert wurde. Heute zeigt `status --json` nur Workspace und Vorlagen.
- `torro_ipc::Status` bekommt `accounts: Vec<AccountStatus>` (mit `#[serde(default)]`, damit ältere
  Clients weiter lesen können). `AccountStatus`: `id`, `kind`, `server`, `state`
  (`connected`, `signing_in`, `auth_failed`, `offline`), `last_sync` (Zeitpunkt oder leer), `error`
  (kurzer Text oder leer). Nie Passwörter oder Tokens.
- `status` ohne `--json` zeigt je Konto eine Zeile (`nextcloud  wagesve@cloud.example.org  connected, synced 12:04`).
- **Abnahme:** Tests für Serialisierung (alt ohne `accounts` lesbar) und die Textausgabe; fmt,
  clippy, test (auch `TORRO_LANG=de`) grün.

### S.14 Eigene Seite „Appearance“
- [x] erledigt (2026-10-14: Symbol `sun`, weil die Suite kein Paletten-Symbol hat; die Gruppe heißt dort
  „Color mode“, damit Seite und Gruppe nicht gleich heißen)
- **Modell:** Opus · **Dateien:** `crates/torroworkplace/src/ui/{mod,general,launch}.rs`,
  `crates/torro-core/src/i18n/de.rs`, `crates/torro-core/src/theme.rs` (Kommentar),
  `.claude/skills/torro-design/SKILL.md`, `docs/design-pakete.md`
- Warum (Wunsch 2026-10-14): Hell/Dunkel und das Design-Paket standen unten auf der Seite General, die nur über
  das Zahnrad erreichbar ist; man fand sie nicht.
- Neue Seite **Appearance** sichtbar in der Seitenleiste (vor General, das am Fuß bleibt), auch mit
  `torroworkplace ui --section appearance`: Gruppen „Color mode“ (System, Light, Dark), „Design“ (die Pakete) und
  „Interface“ (Zoom). General behält Sprache, Kalender und Start beim Anmelden. Die Pakete werden beim Öffnen
  von Appearance gelistet; `--open design` öffnet Appearance.
- **Oberfläche/API/MCP:** keine neue Funktion – dieselben Einstellungen (`settings.set_setting` für
  `appearance`, `design`, `zoom`; `settings.list_designs`), nur an anderer Stelle der Oberfläche.
- **Abnahme:** Tests: Seite in der Navigation und per `--section appearance`; Öffnen von Appearance listet die
  Pakete, General nicht mehr; `--open design` landet auf Appearance. Sichtprüfung der Steuerzentrale mit
  `--demo --section appearance`, hell und dunkel.

## Stufe 2: Einstellungen in den Apps (S.15–S.22)

Konzept: [settings.md › Einstellungen in den Apps](../settings.md#einstellungen-in-den-apps), entschieden am
2026-10-14. Das Zahnrad einer App zeigt die Einstellungen dieser App im Inhaltsbereich desselben Fensters; die
Steuerzentrale behält, was die ganze Suite betrifft, und zeigt unter „Apps“ dieselben Seiten.

**Oberfläche, API, MCP (gilt für alle Aufgaben dieser Stufe):** keine neue Funktion. Jede Einstellung geht über
einen Request, den es schon gibt; die Aufgaben verlegen nur, wo die Oberfläche sie zeigt. Braucht eine Aufgabe doch
einen neuen Request, hält sie an und fragt nach (Skill `torro-api`). Nach S.17, S.18, S.19, S.20 und S.21 prüft der Agent
`torro-api-review`, dass keine Oberfläche selbst liest oder schreibt.

```
S.15 (Fehler, sofort)
S.16 ── S.17 ── S.18 (Assistent) ──┬── S.19 (Kalender, Aufgaben)
                                    ├── S.20 (Eingang, Mail)
                                    └── S.21 (Steuerzentrale: Gruppe „Apps“, Kontoseiten kurz) ── S.22
```

### S.15 „All rules“ öffnet die Seite Inbox
- [ ] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroinbox/src/app.rs`
- Warum: „All rules“ im Stumm-Menü des Eingangs (`app.rs` um Zeile 5623) schickt `Message::OpenSettings`, das
  `connect::open_settings()` aufruft und damit immer `--section accounts` öffnet. Die Regeln stehen auf der Seite
  Inbox.
- „All rules“ bekommt eine eigene Nachricht, die `torro_gui::connect::open_section("inbox")` aufruft. Das Zahnrad
  bleibt bis S.20 wie es ist.
- **Abnahme:** Test, dass „All rules“ die neue Nachricht schickt und das Zahnrad weiter `OpenSettings`; fmt,
  clippy, test (auch `TORRO_LANG=de`) grün.

### S.16 Gerüst: Bibliothek `torro-settings-ui` und Host für den Inhaltsbereich
- [ ] erledigt
- **Modell:** Opus (neues Crate, Schnittstelle für alle Apps) · **Dateien:** `Cargo.toml` (Wurzel, Mitglied und
  `[workspace.dependencies]`), `crates/torro-settings-ui/` (neu), `crates/torroworkplace/Cargo.toml`,
  `crates/torroworkplace/src/ui/mod.rs`, `crates/torro-gui/src/shell.rs`
- Neues Bibliotheks-Crate `torro-settings-ui` (hängt an `torro-core`, `torro-gui`, `torro-ipc`, nicht an
  `torroworkplace`). Darin:
  - die Zeilen-Bausteine aus `ui/mod.rs`: `setting_row`, `pressable`, `row_frame`, `chevron`, `dialog`,
    `confirm`. Die Steuerzentrale nutzt sie von dort; Verhalten und Aussehen bleiben gleich.
  - die Form einer Seite (`State`, `Message`, `update(..) -> Action`, `view(..)`, optional `header`, `overlay`)
    als Doc-Kommentar und die `Action` (`Send(Request)`, `OpenUrl`, `Open(Abschnitt)`, `AllSettings`; für
    Appearance `Zoom`, `ListDesigns`, `Design`). Was nur die Steuerzentrale kann (`OpenDesignsFolder`, legt einen
    Ordner an; `Copy`, `Focus`, `Setup` der Agenten), bleibt deren eigene Aktion und erscheint in den Apps nicht.
  - der **Host** für den Inhaltsbereich einer App: Abschnittsliste links im Inhalt, Kopfleiste 44 mit
    „← Settings › <Abschnitt> [› Unterseite]“, die Seite rechts; Mittel: Abschnitte als Menü in der Kopfleiste;
    Schmal: Liste, Abschnitt als eigene Seite mit „‹ Back“. Esc und der Pfeil schließen. Unten in der Liste
    „All settings ↗“ (öffnet `torroworkplace ui --section <eintrag>`). Der Host schickt die Requests einer
    `Action::Send` und lädt danach die Daten seiner Abschnitte neu.
  - **Den Weg zum Dienst bekommt der Host von der App** (ein Trait, z. B. `Backend`), nie selbst über
    `torro_ipc::client`: `torro_ipc::client` liest den App-Schlüssel, und eine Sichtprüfung mit `--demo` würde
    sonst echte Rechte und Einstellungen ändern. Im Demo-Betrieb antwortet ein Backend im Speicher, wie heute die
    Steuerzentrale mit `--demo`.
- `torro_gui::shell::TitleButtons` bekommt `settings_open(bool)` (nur hinzugefügt): das Zahnrad ist hervorgehoben,
  solange die Einstellungen offen sind.
- Noch keine App nutzt den Host; ein Beispiel-Abschnitt in den Tests genügt.
- **Abnahme:** Tests für den Host (Abschnitt wählen, Unterseite und Zurück, Esc schließt, `Send` führt zum
  Neuladen, Größenklassen wählen die richtige Form), ein Test, dass das Demo-Backend nie den Socket benutzt, und
  einer für `settings_open`; die Steuerzentrale sieht aus wie vorher
  (Sichtprüfung `--demo --section accounts`, hell und dunkel); fmt, clippy, test (auch `TORRO_LANG=de`) grün; baut
  auf Linux, macOS, Windows.

### S.17 Seiten des Assistenten in die Bibliothek, in Abschnitte geteilt
- [ ] erledigt
- **Modell:** Opus · **Dateien:** `crates/torro-settings-ui/src/` (neu: `assistant/`), `crates/torroworkplace/src/ui/
  {mod,assistant,bots,persona,agents,general}.rs`
- Die Seite Assistant (heute 14 Gruppen untereinander) wird zu den Abschnitten **General** (Status, an/aus, neue
  Chats mit Engine, Modell, Denkstufe, Liste der Engines), **Profile** (Profil, Gedächtnis), **Bots** (Liste; je Bot
  eine Unterseite mit „What the bot may use“), **Access** („What the assistant may use“, Approval, Auswahl der
  Sammlungen), **Voice** (Sprechen, Mikrofon, Programme auf diesem Rechner) und **History** (Unterhaltungen
  aufheben). Dazu der gemeinsame Abschnitt **Appearance** (Farbmodus, Design, Zoom) aus `general.rs`.
- Die Rechte-Ansicht (`agents::rights_groups`) zieht mit um; Agents & API in der Steuerzentrale nutzt sie von dort.
- Name und Beschreibung eines Bots kann ein Agent schreiben (`bots.save_bot`). Auf der Unterseite des Bots stehen sie
  nur als Text im Pfad und in der Überschrift, nie als Knopf oder Hinweis im Rechte-Editor.
- Solange der Assistent aus ist, zeigt General „Turn on assistant“ und die Vorlagen („Start from“), die übrigen
  Abschnitte sagen in einem Satz, dass sie nach dem Einschalten bereitstehen.
- Die Steuerzentrale zeigt unter Assistant dieselben Abschnitte über den Host (aus S.16), `--section assistant` und
  `--section bot:<id>` gehen weiter.
- **Abnahme:** die Tests der heutigen Seiten laufen in der Bibliothek weiter; Tests: jeder Abschnitt zeigt seine
  Gruppen, `bot:<id>` öffnet die Unterseite des Bots; Sichtprüfung der Steuerzentrale (`--demo --section assistant`
  und `--section bot:b0000001`) bei 1100, 500 und 360 px; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.18 Assistent: Einstellungen im Inhaltsbereich
- [ ] erledigt
- **Modell:** Opus · **Dateien:** `crates/torroassistant/Cargo.toml`, `crates/torroassistant/src/{app,view,bots}.rs`,
  `crates/torroassistant/src/main.rs` (Startargument)
- Das Zahnrad schaltet den Inhaltsbereich zwischen Chat und den Einstellungen des Assistenten um (Host aus S.16,
  Abschnitte aus S.17). Die Seitenleiste bleibt; ein Klick auf einen Chat oder Bot dort schließt die Einstellungen.
- Was heute die Steuerzentrale öffnet, öffnet den Abschnitt im Fenster: „Set up“ → General, „Change access“ unter
  einer abgelehnten Antwort → Access, „Access…“ eines Bots → seine Unterseite unter Bots. Nur Konten einrichten
  (Engines) bleibt „All settings ↗“ bzw. `--section accounts`.
- „Open chat“ auf der Unterseite eines Bots schaltet im selben Fenster auf seinen Chat um, statt über
  `assistant.open_window` ein zweites Fenster zu starten.
- `torroassistant --settings <abschnitt>` (auch `bot:<id>`) öffnet das Fenster mit offenen Einstellungen. Nur ein
  Mensch öffnet die Einstellungen: Antworttext kann keinen Abschnitt öffnen (Links in Antworten bleiben auf
  `http(s)` beschränkt, kein `--settings` und kein eigenes Schema aus Antworten).
- **Abnahme:** Tests: Zahnrad hin und zurück, Esc, die drei Hinweise landen im richtigen Abschnitt,
  `--settings access` startet dort, ein Link `torro://…` oder ein Pfad in einer Antwort öffnet nichts; Sichtprüfung mit `--demo --settings general|bots|access` bei 1100, 500 und
  360 px, hell und dunkel (Agent `torro-design-review`); fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.19 Kalender und Aufgaben: Einstellungen im Inhaltsbereich
- [ ] erledigt
- **Modell:** Sonnet · **Dateien:** `crates/torro-settings-ui/src/` (neu: `collections.rs`, `night.rs`),
  `crates/torrocalendar/{Cargo.toml,src/app.rs,src/main.rs}`, `crates/torrotasks/{Cargo.toml,src/app.rs,src/main.rs}`
- Kalender: Abschnitte **Calendars** (welche die Suite zeigt, je Konto gruppiert, `settings.set_collection_shown`),
  **Night** (wie heute General › Calendar) und Appearance. Die Karte am Mond in der Zeitleiste bleibt.
- Aufgaben: Abschnitte **Lists** (welche die Suite zeigt, je Konto gruppiert) und Appearance.
- Das Zahnrad und „Settings“ im Menü öffnen die Einstellungen im Fenster; `--settings <abschnitt>` in beiden Apps.
  Der Hinweis ohne Konto („Open Settings“) öffnet weiter die Steuerzentrale bei Accounts.
- **Abnahme:** Tests: Ausblenden in Calendars erreicht `SetCollectionShown`, Night sendet `SetSetting(Night)`,
  Zahnrad hin und zurück; Sichtprüfung beider Apps mit `--demo --settings …` bei 1100, 500 und 360 px;
  fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.20 Eingang und Mail: Einstellungen im Inhaltsbereich
- [ ] erledigt
- **Modell:** Opus (viele Quellen, große App) · **Dateien:** `crates/torro-settings-ui/src/` (neu: `inbox.rs`,
  `mail.rs`), `crates/torroinbox/{Cargo.toml,src/app.rs,src/main.rs}`, `crates/torroworkplace/src/ui/inbox.rs`
- Abschnitte **Notifications** (still bei Terminen), **Sources** (je Konto: Nextcloud-Benachrichtigungen und Talk,
  GitHub, ntfy-Themen mit Etikett, „Show new mail in Inbox“), **Rules** (Liste mit „Remove“), **Mail** (je
  Mail-Konto Absendername und Signatur, offene Ordnerfragen) und Appearance.
- „All rules“ (S.15) öffnet nun Rules im Fenster. Die Seite Inbox der Steuerzentrale zeigt dieselben Abschnitte.
- **Abnahme:** Tests: jede Quelle schickt den Request, den heute die Kontoseite schickt (`MuteSource`,
  `SetNtfyTopic`, `UpdateMailAccount`), Rules entfernt mit `RemoveRule`; Sichtprüfung mit `--demo --settings
  sources|rules|mail` bei 1100, 500 und 360 px; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.21 Steuerzentrale: Gruppe „Apps“, Kontoseiten kurz
- [ ] erledigt
- **Modell:** Opus · **Dateien:** `crates/torroworkplace/src/ui/{mod,accounts,apps,general,launch}.rs`
- Seitenleiste mit zwei Gruppen: **Suite** (Overview, Accounts, Agents & API, Approvals, Workspaces, Appearance,
  General) und **Apps** (Assistant, Calendar, Tasks, Inbox, Mail). Ein App-Eintrag zeigt oben die Gruppe „App“
  (installiert, „Open“) und darunter dieselben Abschnitte wie in der App. Die Seite Apps entfällt;
  `--section apps` öffnet die Gruppe beim ersten App-Eintrag, `--section calendar|tasks|inbox|mail` sind neu.
- General verliert „Calendar“ (jetzt Kalender › Night). Die Kontodetailseiten verlieren „Calendars and lists“,
  „Inbox“, „Topics“, Absender und Signatur; an ihrer Stelle je eine Zeile mit dem neuen Ort, die den App-Eintrag in
  der Steuerzentrale selbst aufschlägt (`Action::Select`, kein Request). „Open“ im App-Eintrag startet die App wie
  heute ohne Ziel (`apps.open_app`); ein Ziel `settings:<abschnitt>` für Agenten gibt es bewusst nicht, damit kein
  Agent einem Menschen den Rechte-Editor vorlegt.
- **Abnahme:** Tests für die Navigation, die alten und neuen `--section`-Wörter und die kürzeren Kontoseiten;
  Sichtprüfung `--demo` bei 1100, 500 und 360 px; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.22 Abschluss
- [ ] erledigt
- **Modell:** Opus
- Übersetzungen eintragen (`crates/torro-core/src/i18n/de.rs`), `docs/settings.md` an den gebauten Stand anpassen,
  Design-Prüfung aller vier Apps und der Steuerzentrale (Agent `torro-design-review`), `torro-api-review` über die
  ganze Stufe, Fehler beheben.

## Stufe 3: KI-Konten (S.23–S.26)

Konzept: [settings.md › KI-Konten](../settings.md#ki-konten), 2026-10-15. Anlass: Ein verbundenes Ollama erschien
weder unter „Agents & API“ noch auf seiner Kontoseite mit Modellen; dabei fiel auf, dass seine Cloud-Modelle für die
Datenregel als lokal gelten.

S.23 schließt eine Datenschutzlücke und kommt zuerst. S.24–S.26 sind kleine Schritte, die ohne Stufe 2 gehen; ihre
Dateien überschneiden sich nicht, sie können parallel laufen. S.24 und S.21 ändern beide `ui/accounts.rs`: nicht
gleichzeitig, und wer später kommt, übernimmt die Gruppe „Models“ bzw. die kurzen Kontoseiten.

```
S.23 (Datenregel, sofort)
S.24 (Modelle auf der Kontoseite) · S.25 (Katalog) · S.26 (Hinweis in „Connect agent“)
```

### S.23 Ollama-Cloud-Modelle gelten für die Datenregel als entfernt
- [x] erledigt
- **Modell:** Opus (Sicherheit) · **Dateien:** `crates/torro-core/src/ai/mod.rs`, `crates/torro-core/src/ai/openai.rs`,
  `crates/torroworkplace/src/account/ai.rs`, `crates/torroworkplace/src/assistant/data_rule.rs`,
  `crates/torroworkplace/src/assistant/model.rs`, `crates/torroworkplace/src/assistant/delegate.rs`,
  `crates/torroworkplace/src/core.rs`
- Warum: `data_rule::place_of` entscheidet je **Konto**: Ollama ist immer `Place::Local`. Modelle mit dem Zusatz
  `:cloud` (`glm-5.3:cloud`, `kimi-k2.6:cloud`) laufen aber über das lokale Ollama auf `ollama.com`. Solange
  `mail_to_cloud` aus ist (C.17, Standard), bekämen sie heute trotzdem Mail, Nextcloud- und Talk-Einträge zu lesen.
- **Schnittstellen (mit dieser Planung freizugeben):**
  - `AiModel` bekommt das optionale Feld `remote: Option<bool>`: `true`, wenn der Server sagt, dass das Modell
    anderswo rechnet (Ollama: `remote_host` in `GET <root>/api/tags`), `false` für ein lokales Modell eines lokalen
    Servers, `null`, wo niemand es sagt. Neue optionale Felder sind nach `api.md` verträglich; API-Referenz neu
    erzeugen.
  - `data_rule::place_of` nimmt das Modell dazu (Konto **und** Modell).
  - `Host::model(engine)` (`assistant/model.rs`, umgesetzt in `core.rs`) bekommt das Modell als Parameter, damit der
    Kern den Ort für das wirklich gefragte Modell bestimmt.
- **Ollama:** Die Modellliste liest zusätzlich `/api/tags` (eine Anfrage für alle Modelle) und setzt `remote`. Ein Name
  mit `:cloud` oder `-cloud` am Ende gilt auch ohne das Feld als entfernt. **Schlägt `/api/tags` fehl, gelten bei
  Ollama alle Modelle als entfernt** (im Zweifel sicher; mit `ollama cp` kopierte Cloud-Modelle tragen den Zusatz
  nicht mehr). `remote` lebt so lange wie die Fähigkeiten aus `/api/show` und wird nicht bei jeder Nachricht neu
  gefragt.
- **Erst das Modell, dann der Ort:** Heute bestimmt `model.rs` den Ort vor der Modellwahl; Prompt, Verlauf und die
  Markierung `local_only` des Gedächtnisses hängen daran. Künftig wählt der Kern zuerst das Modell (gewählt oder
  „Automatic“), dann den Ort, dann alles, was davon abhängt. Der `Reader` aus C.17 (Ort und Regel) bleibt, wie er ist.
- **„Automatic“** (`default_model` in `assistant/model.rs`) wählt heute das erste Modell mit Werkzeugen. Künftig
  zuerst ein lokales (Open First), ein entferntes nur, wenn es kein lokales gibt.
- **Bots** (`delegate.rs`): Ein Bot ohne Modell (`model: None`) löst vor der Prüfung der Datenregel dasselbe
  „Automatic“ auf wie `model.rs`; geprüft wird das Modell, das dann wirklich antwortet.
- **Verlauf:** `history_allowed` vergleicht heute nur die Engine (`ai:ollama@…`). Künftig Engine **und Modell**
  (`EntryContent::Answer` trägt `model`); fehlt das Modell einer früheren Antwort, gilt sie als von einem anderen
  Modell. So bekommt ein Cloud-Modell nicht, was ein lokales Modell desselben Kontos aus Mail nacherzählt hat.
- **Oberfläche:** keine in dieser Aufgabe (den Ort zeigt S.24).
- **API:** `Request::AiModels` · `ai.list_models` · Read (neues Feld `remote`)
- **MCP:** `ai_list_models` (dasselbe Tool; Beschreibung um „whether it runs elsewhere“ ergänzen)
- **Abnahme:** Tests ohne Netz (gefälschter Ollama wie in `account/ai.rs`):
  - ein `:cloud`-Modell mit `remote_host` ist `remote: true`, ein lokales `false`; ohne `/api/tags` sind alle `true`;
  - `place_of` für ein Cloud-Modell von Ollama ist `Remote`, für ein lokales `Local`;
  - mit `mail_to_cloud` aus wird ein Mail-Ergebnis an ein Cloud-Modell zu `withheld by your data rule`, an ein lokales
    nicht; mit `mail_to_cloud` an gilt C.17 unverändert;
  - Verlauf lokal → Cloud im selben Konto: die Antwort mit Mail-Inhalt wird ersetzt;
  - „Automatic“ und ein Bot ohne Modell nehmen bei gemischter Liste das lokale Modell;
  - eine Notiz, die nach Mail entstand, ist bei einem lokalen Modell `local_only` und erreicht das Cloud-Modell nicht;
  - fmt, clippy, test (auch `TORRO_LANG=de`) grün; Agent `torro-api-review`.

### S.24 Die Kontoseite eines KI-Kontos zeigt seine Modelle
- [ ] erledigt
- **Modell:** Sonnet · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`, `crates/torroworkplace/src/ui/mod.rs`
  · **Nach:** S.23 (für den Ort „Ollama Cloud“)
- Warum: Ein verbundenes Ollama zeigte nur Status und Server. Die Modelle sah man nur unter Assistant › New chats,
  nachdem man dort Ollama gewählt hatte.
- Zwischen „Status“ und „Connection“ die Gruppe **„Models“**: eine Zeile je Modell mit Name, darunter die Merkmale in
  einer Zeile („Tools · Thinking · 128k context“) und der Ort („On this computer“, „Ollama Cloud“ bei Ollama mit
  `remote`, sonst „Cloud“). Modelle mit `tools == Some(false)` blass mit „Can't use tools“. Rechts je Zeile „Use for
  new chats“ (sekundärer Knopf, nur bei Modellen mit Werkzeugen); das gewählte trägt stattdessen ein Häkchen und
  „Used for new chats“.
- Fußzeile: „The assistant offers the models that can use tools. Each chat can pick another.“
- Geladen werden die Modelle beim Öffnen der Kontoseite (`ai.list_models`, `all: false`), nicht bei jedem Takt;
  solange sie laden, „Loading models…“. Fehler (Server aus): der Satz des Kerns in der Gruppe, der Status bleibt.
  OpenRouter: dazu „Show closed models“ (`all: true`).
- „Use for new chats“ schickt `settings.set_setting assistant_default` mit Konto und Modell; die gespeicherte
  Denkstufe bleibt.
- **Oberfläche:** Steuerzentrale › Accounts › KI-Konto › „Models“
- **API:** `Request::AiModels` · `ai.list_models` · Read; `Request::SetSetting` · `settings.set_setting` · Admin
- **MCP:** `ai_list_models`, `settings_set_setting` (Admin, beim Agenten mit Freigabe)
- **Abnahme:** Tests mit Demo-Daten: Modelle mit Merkmalen und Ort, ein Modell ohne Werkzeuge ohne Knopf, „Use for
  new chats“ baut den richtigen `SetSetting`. Braucht die Sichtprüfung eine neue Demo-Ansicht in `ui/launch.rs`,
  vorher nachfragen. Sichtprüfung bei 1100, 500 und 360 px (Agent `torro-design-review`); fmt, clippy, test (auch
  `TORRO_LANG=de`) grün.

### S.25 KI-Anbieter stehen im Katalog oben
- [ ] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/wizard.rs`
- In `provider_step` kommt „AI providers“ an die erste Stelle, mit dem Untertitel „Ollama, LM Studio, OpenRouter and
  more“ statt „Summaries and agents“. Die übrigen Einträge behalten ihre Reihenfolge.
- **Oberfläche:** Steuerzentrale › Accounts › Add account
- **API / MCP:** unverändert (`accounts.connect_ai` bleibt `Never`, Geheimnisse nur über die Oberfläche;
  `ai.find_local_servers` / `ai_find_local_servers`)
- **Abnahme:** Test, dass die erste Zeile des Katalogs die KI-Anbieter sind; fmt, clippy, test (auch
  `TORRO_LANG=de`) grün.

### S.26 „Connect agent“ verweist auf die KI-Konten
- [ ] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torroworkplace/src/ui/agent_wizard.rs`, `crates/torroworkplace/src/ui/agents.rs`
- Warum: Wer Ollama verbinden will, sucht unter „Connect agent“. Dort gehören aber nur Programme hin, die Torro
  bedienen.
- Im ersten Schritt unter der Liste der Clients eine leise Zeile mit Pfeil: „Looking to use a model such as Ollama?
  Add an AI account“. Ein Klick schließt den Dialog und öffnet Accounts (`Action::Select(Section::Accounts)`); den
  Dialog „Add account“ gleich bei den KI-Anbietern zu öffnen braucht eine neue `Action` in `ui/mod.rs`: dann vorher
  nachfragen.
- Der leere Zustand der Seite nennt es auch: „… or to create a key for a script. AI models such as Ollama are added
  under Accounts.“
- **Oberfläche:** Steuerzentrale › Agents & API › Connect agent
- **API / MCP:** keine; reine Navigation in der Oberfläche.
- **Abnahme:** Test, dass der Klick die richtige `Action` ergibt; bestehende Tests der Seite grün; fmt, clippy, test
  (auch `TORRO_LANG=de`) grün.
