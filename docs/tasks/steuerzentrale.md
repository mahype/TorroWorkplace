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

> **Wartet auf S.29 (#210):** S.16–S.22 werden gegen die Bestandsaufnahme geprüft und danach übernommen, geändert
> oder gestrichen. Bis S.29 abgehakt ist, keine dieser Aufgaben beginnen.

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

## Stufe 3: KI (S.23–S.26)

> **S.25 und S.26 warten auf S.29 (#210):** Ort der Seite AI und der Modell-Auswahlen kann sich mit der neuen
> Struktur ändern. S.24 (Kern) hängt nicht daran.

Konzept: [settings.md › KI](../settings.md#ki), entschieden am 2026-10-15. Zentral richtet man Anbieter ein und schaltet
ihre Modelle an und ab (Steuerzentrale › AI); welches Modell ein Modul wofür nimmt, stellt man im Modul ein. Anlass: Ein
verbundenes Ollama erschien weder unter „Agents & API“ noch auf seiner Kontoseite mit Modellen; dabei fiel auf, dass
seine Cloud-Modelle für die Datenregel als lokal galten (S.23).

S.24 legt den Befehl und die Regel im Kern fest, danach laufen S.25 (Seite AI) und S.26 (Auswahlen der Module)
parallel. S.26 und S.17 berühren beide die Auswahl „New chats“: Wer später kommt, übernimmt den Filter bzw. den Umzug in
`torro-settings-ui`.

```
S.23 (Datenregel, erledigt)
S.24 (Modelle an/aus im Kern) ──┬── S.25 (Steuerzentrale › AI)
                                └── S.26 (Auswahlen bieten nur angeschaltete Modelle)
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
- **Oberfläche:** keine in dieser Aufgabe (den Ort zeigt S.25).
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

### S.24 Modelle an- und abschalten: Befehl und Regel im Kern
- [ ] erledigt
- **Modell:** Opus (neuer Befehl, Regel für alle Module, Datenregel) · **Dateien:** `crates/torro-ipc/src/lib.rs`,
  `crates/torro-ipc/src/api.rs`, `crates/torro-core/src/ai/mod.rs`, `crates/torroworkplace/src/account/ai.rs`,
  `crates/torroworkplace/src/assistant/model.rs`, `crates/torroworkplace/src/assistant/delegate.rs`,
  `crates/torroworkplace/src/assistant/engines/mod.rs`, `crates/torroworkplace/src/assistant/engines/claude.rs`,
  `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/daemon.rs`,
  `crates/torroworkplace/src/audit.rs`, `crates/torroworkplace/src/approvals.rs`, `crates/torroworkplace/src/mcp.rs`,
  `docs/api.md`, `docs/api-reference.md`
- **Neuer Befehl:** `Request::SetModelEnabled { account, model, enabled }` · `ai.set_model_enabled` · Bereich `ai` ·
  Admin. Schaltet ein Modell eines Anbieters für die ganze Suite an oder aus. Fehlercodes: `not-found` (kein solcher
  Anbieter; Claude Code nicht gefunden; Anschalten eines Modells, das der Anbieter nicht meldet; Ausschalten geht auch
  dann), `offline`, `auth`, `not-connected`, `busy` (Anschalten fragt den Server nach seinen Modellen),
  `forbidden`. Katalogbeschreibung englisch: „Turns a model of an AI provider on or off for the whole suite; modules
  offer only models that are on. Turning on a model that runs elsewhere is kept for people.“
- **Nur Menschen schalten entfernte Modelle an:** `enabled: true` für ein Modell, das anderswo rechnet (`remote`), gilt
  wie `mail_to_cloud: true` als `kept_for_people` (`docs/api.md` Punkt 6 ergänzen); ein Agent bekommt `forbidden` mit
  Satz. Ausschalten und das Anschalten eines Modells, das hier rechnet, sind gewöhnliche Admin-Befehle mit Freigabe.
  Die Zusammenfassung der Freigabe (`approvals::Facts`) nimmt Anbieter, Modell und Ort aus der Liste des Kerns, nie aus
  dem Text des Clients.
- **Lesen:** `ai.list_models` liefert je Modell das neue Feld `enabled` (verträglich, `#[serde(default)]`; der Kern
  setzt es aus der Ablage, `account/ai.rs` setzt es beim Abfragen des Servers nie). `account` darf auch die Engine
  `claude-code` sein, unter derselben Bedingung, unter der `assistant.list_engines` sie anbietet (Befehl gefunden **und**
  der Assistent hat seinen Client): dann die Modelle aus `engines::claude::models()` mit `remote: true`, `all` wird
  ignoriert; sonst `not-found`. Doc-Kommentar von `Request::AiModels` und Katalogtext von `ai.list_models` nennen das
  („or an engine id from assistant_list_engines“), auch dass Leserecht in `ai` damit verrät, ob Claude Code installiert
  ist.
- **Ablage:** eigene Datei `<config>/ai-models.json` (0600, atomar geschrieben): je Anbieter die angeschalteten Modelle und
  ob der Mensch dort schon etwas ausgeschaltet hat. **Nicht** in `settings.json`, damit `settings.update_settings`
  (Bereich `settings`) die Rechte in `ai` nicht umgeht und gleichzeitige Schaltvorgänge nicht überschreibt. Protokoll
  (`audit.rs`) nur mit Anbieter und `enabled`, ohne Modellnamen.
- **Regel am Anfang** (settings.md › KI): an sind die Modelle mit Werkzeugen, die auf diesem Rechner rechnen; aus sind
  Ollama Cloud, Claude Code, OpenRouter und Server im Netz. Beim ersten Start nach dem Update sind zusätzlich die
  Modelle an, die schon genutzt werden: `assistant_default` und die Modelle der Bots. Die pro Unterhaltung gewählten
  Modelle schalten nichts an; sie fallen wie unten auf „Automatic“, mit Hinweis.
- **Der Kern hält sich daran, in dieser Reihenfolge:** erst das Modell (ein gespeichertes, aber abgeschaltetes wird
  durch „Automatic“ unter den angeschalteten Modellen desselben Anbieters ersetzt, lokal zuerst wie S.23), dann der Ort,
  dann Datenregel, Prompt und `local_only` (wie S.23). Das gilt für `assistant.send_message`, Bots ohne oder mit
  abgeschaltetem Modell (`delegate.rs`) und Engine B: Für Claude Code übergibt der Kern immer ausdrücklich ein
  angeschaltetes Modell (`--model`), nie „Claude Code wählt selbst“.
- **Ein Anbieter ganz aus:** `assistant.list_engines` nennt nur Anbieter mit mindestens einem angeschalteten Modell
  (Katalogtext nachziehen: KI-Konten **und** Claude Code, mit `models`). `send_message` und `ask_bot` mit einer solchen
  Engine antworten `unsupported` mit „Turn on a model of {provider} in Settings › AI.“; kein Ausweichen auf einen
  anderen Anbieter. Die Beschreibungen von `assistant.send_message` und `bots.save_bot` sagen, dass ein abgeschaltetes
  Modell ersetzt wird und die Antwort das gewählte nennt.
- **Oberfläche:** Steuerzentrale › AI (S.25)
- **API:** `Request::SetModelEnabled` · `ai.set_model_enabled` · Admin; `Request::AiModels` · `ai.list_models` · Read
  (neues Feld `enabled`, Engine `claude-code`)
- **MCP:** `ai_set_model_enabled` (Admin, beim Agenten mit Freigabe; entfernte Modelle anschalten nur Menschen);
  `ai_list_models` (Beschreibung ergänzt)
- **Abnahme:** Handler-Tests (Temp-Verzeichnisse, gefälschter Ollama):
  - Schalten wirkt, überlebt einen Neustart und steht nicht in `settings.get_settings`;
  - Regel am Anfang: lokales Modell an, `:cloud`, OpenRouter und Claude Code aus; genutzter Standard und Bot-Modell an;
  - ein Agent kann ein entferntes Modell nicht anschalten (`forbidden`), ein lokales nur mit Freigabe;
  - ist ein lokales Modell aus und nur ein `:cloud`-Modell desselben Kontos an, nimmt „Automatic“ das Cloud-Modell **und**
    die Datenregel behandelt es als entfernt;
  - Claude Code bekommt immer ein angeschaltetes `--model`; ohne angeschaltetes Modell `unsupported`;
  - Rechte-Tabelle und MCP-Vertragstest grün (automatisch); API-Referenz neu erzeugt
    (`TORRO_BLESS=1 cargo test -p torro-ipc`); fmt, clippy, test (auch `TORRO_LANG=de`) grün; Agent `torro-api-review`.

### S.25 Steuerzentrale › AI: Anbieter und ihre Modelle
- [ ] erledigt
- **Modell:** Opus (neue Seite, Umzug aus Accounts) · **Braucht:** S.24 · **Dateien:**
  `crates/torroworkplace/src/ui/ai.rs` (neu), `crates/torroworkplace/src/ui/mod.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`, `crates/torroworkplace/src/ui/accounts.rs`,
  `crates/torroworkplace/src/ui/assistant.rs`, `crates/torroworkplace/src/ui/launch.rs`
- Neuer Abschnitt **AI** in der Seitenleiste nach Accounts (`Section::Ai`, `--section ai`, Symbol nach dem Design-Skill
  aus dem vorhandenen Satz). Aufbau nach settings.md › KI: Karten der Anbieter (Open First, Status, „3 of 10 models
  on“), „Add provider“, je Anbieter eine Unterseite mit Status, Verbindung, „Models“ mit Merkmalen, Ort („On this
  computer“, „Ollama Cloud“, „Cloud“) und Schalter, Suche und „Show closed models“ bei OpenRouter, „Remove provider…“.
  Claude Code als Karte, sobald gefunden, ohne „Remove“. Die Karten zeigen „3 models on“ aus der Ablage (über
  `ai.list_models` erst auf der Unterseite), nicht „3 of 10“, damit beim Öffnen der Seite nicht jeder Server gefragt
  wird. Der Ort steht je Modell: „On this computer“, „Ollama Cloud“, „Cloud (Anthropic)“, „Cloud“.
- „Add provider“ öffnet den vorhandenen Dialog (`wizard.rs`) gleich bei den KI-Anbietern; der Katalog „Add account“
  verliert den Eintrag „AI providers“, die Liste unter Accounts die KI-Konten. Wer `--section accounts` mit einem
  KI-Konto erwartet (alte Verweise), landet über `--section ai` richtig: Die Zeile „Engines“ der Assistentenseite und
  „No AI account yet“ öffnen künftig AI statt Accounts.
- Modelle werden beim Öffnen einer Unterseite geladen (`ai.list_models`), nicht bei jedem Takt; „Loading models…“,
  Fehler des Servers als Satz in der Gruppe.
- **Oberfläche:** Steuerzentrale › AI
- `accounts.list_accounts` liefert die KI-Konten weiter (Agenten brauchen ihre Kennungen für `ai_list_models`); nur die
  Liste unter Accounts in der Oberfläche blendet sie aus.
- S.25 und S.21 ändern beide `ui/accounts.rs`: nicht gleichzeitig; wer später kommt, übernimmt den Filter bzw. die kurzen
  Kontoseiten.
- **API:** `ai.list_models` · Read; `ai.set_model_enabled` · Admin; `ai.find_local_servers` · Read;
  `assistant.list_engines` · Read (Karte Claude Code); `accounts.connect_ai` · Admin; `accounts.remove_account` · Admin;
  `accounts.list_accounts` · Read
- **MCP:** `ai_list_models`, `ai_set_model_enabled`, `ai_find_local_servers`, `assistant_list_engines`,
  `accounts_remove_account`;
  `accounts.connect_ai` bleibt **Never** (Schlüssel nur über die Oberfläche)
- **Abnahme:** Tests mit Demo-Daten: Seite in der Navigation und per `--section ai`, Reihenfolge Open First, ein Schalter
  baut `SetModelEnabled`, Modelle ohne Werkzeuge ohne Schalter, Claude Code ohne „Remove“, Katalog ohne KI-Anbieter,
  Accounts ohne KI-Konten. Demo-Ansichten `--demo --section ai` und eine Unterseite für die Sichtprüfung. Sichtprüfung bei
  1100, 500 und 360 px, hell und dunkel (Agent `torro-design-review`); fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.26 Die Auswahlen der Module bieten nur angeschaltete Modelle
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** S.24 · **Dateien:** `crates/torroworkplace/src/ui/assistant.rs`,
  `crates/torroassistant/src/view.rs`, `crates/torroassistant/src/view/editor.rs`, `crates/torroassistant/src/app.rs`
- Assistant › New chats (heute in der Steuerzentrale, nach S.17 in `torro-settings-ui`), die Auswahl pro Chat und der
  Bot-Editor zeigen nur Modelle mit `enabled`. „Show all models“ im Chat entfällt; an seiner Stelle „Manage models…“,
  das die Steuerzentrale bei AI öffnet (vorhandener Weg `torro_gui::connect::open_section("ai")`, kein neuer Request).
- Ist bei keinem Anbieter ein Modell an: „No model is turned on. Turn on models in Settings › AI.“ mit demselben Knopf.
- Ein gespeichertes, aber abgeschaltetes Modell (Standard, Bot) steht oben in der Auswahl mit „Turned off in Settings ›
  AI“, damit man sieht, warum „Automatic“ antwortet.
- **Oberfläche:** Steuerzentrale › Assistant › New chats; Assistent › Auswahl im Chat; Bot-Editor
- **API:** keine neue Funktion; dieselben Befehle, gefiltert nach `enabled`: `ai.list_models` · Read,
  `assistant.list_engines` · Read, `settings.set_setting assistant_default` · Admin, `bots.save_bot` · Write (Ersetzen
  eines Bots wie im Katalog)
- **MCP:** die gleichnamigen Tools (unverändert)
- **Abnahme:** Tests: nur angeschaltete Modelle in allen drei Auswahlen, Hinweis ohne Modelle, abgeschaltetes Modell mit
  Hinweis; Sichtprüfung des Assistenten mit `--demo`; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

## Stufe 4: Bestand und neue Struktur (S.27–S.29, #210)

Wunsch vom 2026-10-16 (#210): Die Einstellungen sind nicht klar gegliedert. Erst entsteht eine Bestandsaufnahme jeder
Einstellung der Suite, dann eine neue Gliederung in drei Entwürfen, aus denen ein Mensch wählt; danach werden Konzept
und die offenen Aufgaben S.16–S.22 und S.25–S.26 angepasst. Gebaut wird erst mit einem neuen Plan zu #210.

Entschieden (#210): Das Zahnrad einer App öffnet nie ein neues Fenster, die Einstellungen der App erscheinen im selben
Fenster über dem Inhalt, zurück mit einem Klick. In der Steuerzentrale liegt nur, was die ganze Suite betrifft (Konten,
AI, Agents & API, Aussehen, Allgemein); Apps verweisen dorthin, ohne dass man suchen muss.

**Oberfläche, API, MCP:** keine neue Funktion in dieser Stufe, nur Dokumente. Wo die Bestandsaufnahme einen fehlenden
Weg findet, nennt S.29 ihn in der passenden Bau-Aufgabe mit Request, Katalogeintrag und MCP-Tool (Skill `torro-api`).

```
S.27 (Bestand) ── S.28 (drei Entwürfe) ── Wahl durch einen Menschen ── S.29 (Konzept und Aufgaben)
```

### S.27 Bestandsaufnahme aller Einstellungen
- [x] erledigt
- **Modell:** Opus (liest die ganze Oberfläche, Urteil je Einstellung) · **Dateien:** `docs/settings.md`
- Neuer Abschnitt **„Bestand (Stand <Datum>)“** in `docs/settings.md`, vor „Offene Fragen“. Je Ort eine Tabelle, eine
  Zeile je Einstellung: **Einstellung** (englischer Text der Oberfläche), **bewirkt** (ein Satz), **heute** (Seite ›
  Gruppe bzw. App › Menü), **Ablage** (`settings.json` mit Schlüssel, `accounts.json`, `ui.json`, Datei des Dienstes),
  **API/MCP** (Request und Tool aus dem Katalog `crates/torro-ipc/src/api.rs`, oder „fehlt“, oder „bewusst nie“ mit
  Verweis auf `docs/api.md`), **wer braucht sie**, **Urteil** (bleibt · zusammenlegen mit … · streichen · umziehen
  nach …, mit Grund in einem Halbsatz).
- Orte, alle nachsehen, nichts aus `docs/` übernehmen, ohne es im Code zu bestätigen:
  - Steuerzentrale, jede Seite in `crates/torroworkplace/src/ui/` (`Section::ALL`), auch Unterseiten (Kontodetail je
    Anbieter, Bot, Agent, Einrichtung, Assistent „Add account“);
  - die Apps `torroassistant`, `torrocalendar`, `torrotasks`, `torroinbox` (Eingang **und** Mail-Fenster): was dort
    wie eine Einstellung wirkt (Menüs, Schalter, Karte am Mond, Dichte, Reihenfolge, Ansicht HTML/Text, Tastenkürzel)
    und wohin Zahnrad und „Settings“ heute führen (`torro_gui::connect::open_settings`/`open_section`);
  - Tray (`crates/torroworkplace/src/tray/`) und Startargumente (`--section`, `--demo`, `--design` …);
  - geplant, aber nicht gebaut, als eigene Zeilen mit „geplant (Aufgabe)“: S.24–S.26 (Modelle an/aus), E10.2/E10.3
    (Tastenkürzel), #189 (Design, Schrift, Farbschema getrennt).
- Fensterzustand in `ui.json` (`torro_gui::ui_state`) steht in einer eigenen, kurzen Tabelle „Kein Einstellungs-Ort“:
  er bleibt in der App und geht durch keine API.
- Darunter **„Befunde“**: Doppeltes (z. B. die Nacht des Kalenders an zwei Stellen), Einstellungen ohne Erklärungssatz,
  fehlende API/MCP-Wege, Verweise, die an die falsche Stelle führen, und Abweichungen zwischen Konzept und Code.
- **Abnahme:** jede Variante von `SettingKey` (`crates/torro-ipc/src/lib.rs`) und jedes Feld von
  `torro_core::settings::Settings` steht in mindestens einer Zeile (im Bericht als Liste abgeglichen); jede Seite aus
  `Section::ALL` und jede der vier Apps hat ihre Tabelle; die Spalte API/MCP ist überall gefüllt. Nur Doku: `cargo fmt
  --all` und `scripts/ci-local.sh` laufen trotzdem durch.

### S.28 Drei Entwürfe der neuen Struktur
- [x] erledigt
- **Modell:** Opus · **Braucht:** S.27 · **Dateien:** `docs/settings.md` (nur der Link zu den Entwürfen)
- Skill `torro-design` laden; Maßstab sind die Größenklassen (Mini bis Breit), Abstände und Farben dort und die Skizze
  in `docs/settings.md` › Einstellungen in den Apps.
- Zuerst die **Regel**, was in die Steuerzentrale gehört und was in die App, in zwei, drei Sätzen; sie steht über allen
  drei Entwürfen. Grundlage: die Entscheidungen aus #210 oben und das Prinzip aus `docs/settings.md` › KI.
- Drei Entwürfe **A, B, C**, die sich in der Gliederung unterscheiden, nicht nur in der Farbe. Achsen, auf denen sie
  sich unterscheiden dürfen: ob die Steuerzentrale die Seiten der Apps unter „Apps“ noch einmal zeigt oder nur auf
  sie verweist; wo Erinnerungen, Benachrichtigungen und „still bei Terminen“ liegen (Kalender, Eingang oder Suite);
  ob Mail eigene Einstellungen hat oder unter Eingang steht; wo Appearance steht (nur Steuerzentrale oder auch in jeder
  App). Jeder Entwurf zeigt:
  - die Seitenleiste der Steuerzentrale und je App die Abschnitte hinter dem Zahnrad (Assistent, Kalender, Aufgaben,
    Eingang, Mail), jeweils mit den Einstellungen aus S.27, die dort landen;
  - eine App mit offenem Zahnrad in Breit, Mittel und Schmal, mit Kopfleiste, Zurück und dem Verweis in die
    Steuerzentrale;
  - eine Liste, was gestrichen oder zusammengelegt wird, und was mit S.16–S.22 und S.25–S.26 geschieht.
- Eine HTML-Seite als Artifact (wie bei #207): die Regel oben, die drei Entwürfe untereinander, hell und dunkel. Die
  Texte der Oberfläche englisch, die Erklärungen deutsch.
- Den Link als Zeile unter dem Abschnitt „Bestand“ in `docs/settings.md` eintragen und als Kommentar
  `## Entwürfe Einstellungen` an #210 posten, mit einer Frage: welcher Entwurf, oder was aus welchem kombiniert wird.
- **Halt:** Danach stoppt die Umsetzung. S.29 beginnt erst, wenn ein Mensch gewählt hat (Kommentar
  `## Entscheidung Struktur` an #210 oder Antwort in der Sitzung, die der Agent dann als solchen Kommentar postet).
- **Abnahme:** Artifact erreichbar, Kommentar gepostet, jede Zeile aus S.27 mit Urteil „bleibt“ oder „umziehen“
  findet in jedem Entwurf einen Ort.

### S.29 Konzept und Aufgaben an die gewählte Struktur anpassen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** S.28 und die Wahl · **Dateien:** `docs/settings.md`,
  `docs/tasks/steuerzentrale.md`, `docs/tasks/eingang.md` (nur E10.3)
- `docs/settings.md`: Abschnitte, „Einstellungen in den Apps“, „Was wohin gehört“ und die Skizze nach der Wahl
  umschreiben; die Regel aus S.28 als eigener Absatz; Kopf mit Stand und „entschieden am <Datum> (#210)“; offene
  Frage 2 beantworten, wenn die Wahl sie entscheidet, sonst stehen lassen. Der Abschnitt „Bestand“ bleibt, das
  Urteil jeder Zeile wird auf die Entscheidung gesetzt.
- `docs/tasks/steuerzentrale.md`: S.16–S.22 und S.25–S.26 je übernehmen, ändern oder streichen. Geänderte tragen
  „(geändert mit #210)“, gestrichene bleiben mit „gestrichen (#210): <Grund>“ und `- [x] entfällt` stehen, damit die
  IDs nicht neu vergeben werden. Neue Aufgaben ab S.30. Jede Bau-Aufgabe nennt Oberfläche, API und MCP; ein fehlender
  Weg aus S.27 bekommt Request und Katalogeintrag in der ersten Aufgabe, die ihn braucht. Die Zahnrad-Aufgaben decken
  alle fünf (Mail, Eingang, Kalender, Aufgaben, Assistent) und nennen die Sichtprüfung in allen Größen. Die Hinweise
  „Wartet auf S.29“ in Stufe 2 und 3 entfernen. S.15 abhaken (umgesetzt in `4e23c72`).
- `docs/tasks/eingang.md`: E10.3 („Braucht: S.20“, Datei `torro-settings-ui/src/mail.rs`) auf die Aufgabe und Datei
  umstellen, die nach der Wahl die Einstellungen des Mail-Fensters bauen.
- **Abnahme:** keine Aufgabe in Stufe 2 oder 3 verweist auf einen Abschnitt, den die gewählte Struktur nicht mehr hat
  (`grep` nach den alten Abschnittsnamen); Abhängigkeitsbaum stimmt mit „Braucht“ überein; `scripts/ci-local.sh` grün.
  Danach `/issue-plan 210` für den Bau.
