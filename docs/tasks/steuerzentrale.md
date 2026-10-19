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
2026-10-14, geordnet am 2026-10-16 (#210, Entwurf B „Schlanke Zentrale“; Regel: [settings.md › Die
Regel](../settings.md#die-regel)). Das Zahnrad einer App zeigt die Einstellungen dieser App im Inhaltsbereich
desselben Fensters, und nur dort. Die Steuerzentrale behält, was die ganze Suite betrifft, zeigt die Seiten der Apps
nicht noch einmal und führt Assistent und Bots unter Agents & API. S.16–S.22 sind mit #210 geändert.

**Bau mit #210** (zweiter Plan, 2026-10-17): S.16–S.22 in dieser Reihenfolge, einer nach dem anderen (S.19 und S.20
ändern beide `torro-settings-ui`). Stufe 3 (S.24–S.26) gehört nicht zu #210; bis sie gebaut ist, gelten die
Ausweichwege der Aufgaben: Verweise auf `--section ai` führen zu `accounts`, die Datenregel bleibt in der
Rechte-Ansicht unter Agents & API.

**Oberfläche, API, MCP (gilt für alle Aufgaben dieser Stufe):** keine neue Funktion. Jede Einstellung geht über
einen Request, den es schon gibt; die Aufgaben verlegen nur, wo die Oberfläche sie zeigt. Das neue Startwort
`--settings` ist ein Startargument, kein Request. Braucht eine Aufgabe doch einen neuen
Request, hält sie an und fragt nach (Skill `torro-api`). Nach S.17, S.18, S.19, S.20 und S.21 prüft der Agent
`torro-api-review`, dass keine Oberfläche selbst liest oder schreibt.

```
S.15 (Fehler, erledigt)
S.16 ── S.17 ── S.18 (Assistent) ──┬── S.19 (Kalender, Aufgaben) ──┬── S.21 (Steuerzentrale: Seiten weg, ── S.22
                                   └── S.20 (Eingang, Mail) ───────┘           Agents & API, General › Apps)
```

### S.15 „All rules“ öffnet die Seite Inbox
- [x] erledigt (umgesetzt in `4e23c72`, abgehakt mit #210)
- **Modell:** Haiku · **Dateien:** `crates/torroinbox/src/app.rs`
- Warum: „All rules“ im Stumm-Menü des Eingangs (`app.rs` um Zeile 5623) schickt `Message::OpenSettings`, das
  `connect::open_settings()` aufruft und damit immer `--section accounts` öffnet. Die Regeln stehen auf der Seite
  Inbox.
- „All rules“ bekommt eine eigene Nachricht, die `torro_gui::connect::open_section("inbox")` aufruft. Das Zahnrad
  bleibt bis S.20 wie es ist.
- **Abnahme:** Test, dass „All rules“ die neue Nachricht schickt und das Zahnrad weiter `OpenSettings`; fmt,
  clippy, test (auch `TORRO_LANG=de`) grün.

### S.16 Gerüst: Bibliothek `torro-settings-ui` und Host für den Inhaltsbereich
- [x] erledigt (Bibliothek, Host, Demo-Backend und `settings_open`; Sichtprüfung der Steuerzentrale auf
  diesem Rechner nicht möglich – kein Hyprland, macOS –, die Bausteine sind unverändert umgezogen)
- **Modell:** Opus (neues Crate, Schnittstelle für alle Apps) · **Dateien:** `Cargo.toml` (Wurzel, Mitglied und
  `[workspace.dependencies]`), `crates/torro-settings-ui/` (neu), `crates/torroworkplace/Cargo.toml`,
  `crates/torroworkplace/src/ui/mod.rs`, `crates/torro-gui/src/shell.rs`
- Neues Bibliotheks-Crate `torro-settings-ui` (hängt an `torro-core`, `torro-gui`, `torro-ipc` und `torro-inbox` für
  `SourceRef` der Quellen in S.20, nicht an `torroworkplace`). Darin:
  - die Zeilen-Bausteine aus `ui/mod.rs`: `setting_row`, `pressable`, `row_frame`, `chevron`, `dialog`,
    `confirm`. Die Steuerzentrale nutzt sie von dort; Verhalten und Aussehen bleiben gleich. Die Steuerzentrale
    nutzt **nur** die Bausteine, nicht den Host.
  - die Form einer Seite (`State`, `Message`, `update(..) -> Action`, `view(..)`, optional `header`, `overlay`)
    als Doc-Kommentar und die `Action`: `Send(Vec<Request>)` (ein oder mehrere Requests, der Reihe nach, danach neu
    laden), `OpenUrl`, `Open(Abschnitt)`, `ControlCentre(section)` für die Verweise in die Steuerzentrale und
    `App(…)` für das, was nur die App selbst kann (z. B. den Bot-Editor öffnen; der Typ ist ein Parameter des Hosts).
  - der **Host** für den Inhaltsbereich einer App: Abschnittsliste links im Inhalt, Kopfleiste 44 mit
    „← Settings › <Abschnitt> [› Unterseite]“, die Seite rechts; Mittel: Abschnitte als Menü in der Kopfleiste;
    Schmal: Liste, Abschnitt als eigene Seite mit „‹ Back“. Esc und der Pfeil schließen. Unten in der Liste immer
    „Appearance ↗“ (`--section appearance`) und „All settings ↗“ (`--section general`); keine Seite Appearance in
    den Apps. Der Host schickt die Requests einer `Action::Send` und lädt danach die Daten seiner Abschnitte neu.
  - **Den Weg zum Dienst bekommt der Host von der App** (ein Trait, z. B. `Backend`), nie selbst über
    `torro_ipc::client`: `torro_ipc::client` liest den App-Schlüssel, und eine Sichtprüfung mit `--demo` würde
    sonst echte Rechte und Einstellungen ändern. Im Demo-Betrieb antwortet ein Backend im Speicher, wie heute die
    Steuerzentrale mit `--demo`; es liegt in `torro-settings-ui` und kennt Beispieldaten für alle Abschnitte, die Apps
    reichen es mit `--demo` nur durch.
- `torro_gui::shell::TitleButtons` bekommt `settings_open(bool)` (nur hinzugefügt): das Zahnrad ist hervorgehoben,
  solange die Einstellungen offen sind.
- Noch keine App nutzt den Host; ein Beispiel-Abschnitt in den Tests genügt.
- **Abnahme:** Tests für den Host (Abschnitt wählen, Unterseite und Zurück, Esc schließt, `Send` führt zum
  Neuladen, Größenklassen wählen die richtige Form, die zwei Verweise unten bauen `ControlCentre`), ein Test, dass
  das Demo-Backend nie den Socket benutzt, einer für `settings_open`; die Steuerzentrale sieht aus wie vorher (Sichtprüfung `--demo --section accounts`, hell und
  dunkel); fmt, clippy, test (auch `TORRO_LANG=de`) grün; baut auf Linux, macOS, Windows.

### S.17 Seiten des Assistenten in die Bibliothek, in Abschnitte geteilt
- [x] erledigt (geändert mit #210; die Abschnitte General, Profile, Bots, Voice und History liegen in
  `crates/torro-settings-ui/src/assistant/`. Der Host reicht Antworten durch – `Action::Ask` hin,
  `Outcome::Answer` an die App und von dort an `assistant::answered` –, darum steht „Test the microphone“
  mit „Say something“ im Abschnitt Voice. Sichtprüfung erst mit S.18 möglich: vorher hängt kein Fenster die
  Abschnitte ein, und dieser Rechner hat kein Hyprland)
- **Modell:** Opus · **Dateien:** `crates/torro-settings-ui/src/` (neu: `assistant/`); `crates/torroworkplace/src/ui/
  {assistant,persona}.rs` nur lesen (die Seite der Steuerzentrale bleibt unverändert bis S.21)
- Die Seite Assistant (heute bis zu 18 Gruppen) wird zu den Abschnitten **General** (Status an/aus als Anzeige mit
  „Turn on in Torro Workplace ↗“ bzw. „Change in Torro Workplace ↗“, neue Chats mit Engine, Modell, Denkstufe), **Profile** (Profil, Gedächtnis), **Bots**
  (Liste; je Bot „Edit“, das im Assistenten den Bot-Editor öffnet, und „Access ↗“ nach Agents & API beim Bot),
  **Voice** (Sprechen, Antworten vorlesen, Mikrofon, Programme auf diesem Rechner) und **History** (Unterhaltungen
  aufheben).
- **Nicht** in die Bibliothek: die Rechte („What the assistant may use“, Approval, Auswahl der Sammlungen), die
  Datenregel und die Gruppe „Engines“. Die Rechte bleiben in `agents::rights_groups` der Steuerzentrale (S.21), die
  Datenregel zieht nach AI (S.25), „Engines“ entfällt (der Hinweis ohne KI-Konto verweist mit `ControlCentre` auf
  `--section ai`, vor S.25 auf `accounts`).
- **Ein- und Ausschalten bleibt in der Steuerzentrale** (`clients.create_client` und `clients.remove_client` nur dort,
  damit der Schlüssel eines neuen Clients nie in einen App-Prozess gelangt): General zeigt den Status und verweist mit
  `ControlCentre` auf `--section assistant` (bis S.21 die Seite Assistant, danach Agents & API beim Assistenten).
- Solange der Assistent aus ist, sagen die übrigen Abschnitte in einem Satz, dass sie nach dem Einschalten
  bereitstehen.
- Die Seite Assistant der Steuerzentrale bleibt bis S.21 unverändert stehen, damit es zwischen S.17 und S.18 keinen
  Abschnitt ohne Ort gibt; S.21 löscht sie.
- **Abnahme:** die Tests der heutigen Seiten, soweit sie umziehen, laufen in der Bibliothek weiter; Tests: jeder
  Abschnitt zeigt seine Gruppen, keiner zeigt Rechte oder Datenregel, kein Abschnitt schickt `CreateClient`,
  `RemoveClient` oder `UpdateClient`, „Access ↗“ eines Bots baut `ControlCentre` mit `bot:<id>`; fmt, clippy, test (auch
  `TORRO_LANG=de`) grün.

### S.18 Assistent: Einstellungen im Inhaltsbereich
- [x] erledigt (geändert mit #210; das Zahnrad schaltet den Inhaltsbereich zwischen Chat und Einstellungen um und ist
  dabei hervorgehoben, `--settings <abschnitt>` startet dort, Escape geht erst an den offenen Abschnitt und dann an
  den Host. Der Bot-Editor hat statt des Abschnitts „Access“ eine Zeile „Access ↗“ und schickt nur noch
  `bots.save_bot`; der Demo-Zustand `--open bot-access` ist damit entfallen. Sichtprüfung auf diesem macOS-Rechner:
  `shoot.sh` braucht Hyprland und `screencapture` darf nicht aufnehmen („could not create image from display“) –
  stattdessen offscreen über Iceds `window::screenshot` (wegwerf-Patch, nicht commitiert, wie bei U.13):
  `--demo --settings general|bots|voice` bei 1100, 800, 500 und 360 px, hell und dunkel, alle vier Größenklassen
  sauber (breit Abschnittsliste, mittel Rail + Menü „General ▾“, schmal Schritte mit „‹“). **Offen:** die neue Zeile
  „Access ↗“ im Bot-Editor steht unter dem Falz, und ohne `--open bot-access` kommt kein Bild davon zustande – das
  Formular ist höher als ein Fenster auf diesem Bildschirm (max. 965 px). Dazu zwei Befunde aus der Bibliothek
  (nicht aus dieser Aufgabe), die S.22 aufnimmt: lange Zeilentexte in `parts::setting_row` werden hart
  abgeschnitten statt mit „…“, und die Textfelder des Bot-Editors zeichnen im Screenshot der ersten Frames leer)
- **Modell:** Opus · **Dateien:** `crates/torroassistant/Cargo.toml`, `crates/torroassistant/src/{app,view,bots}.rs`,
  `crates/torroassistant/src/view/editor.rs`, `crates/torroassistant/src/app/bots.rs`, `crates/torroassistant/src/app/tests.rs`,
  `crates/torroassistant/src/backend.rs` (setzt das Backend des Hosts um), `crates/torroassistant/src/main.rs`,
  `crates/torroassistant/src/launch.rs`
- Das Zahnrad schaltet den Inhaltsbereich zwischen Chat und den Einstellungen des Assistenten um (Host aus S.16,
  Abschnitte aus S.17). Die Seitenleiste bleibt; ein Klick auf einen Chat oder Bot dort schließt die Einstellungen.
- Was heute die Steuerzentrale öffnet: „Set up access“ (erster Start) und „Change access“ führen beide zur
  Steuerzentrale `--section assistant`, wo man ihn einschaltet bzw. die Rechte ändert (wie heute `Message::SetUp`);
  „Change access“ unter einer
  abgelehnten Antwort → Steuerzentrale `--section assistant`; „Access…“ und „More in Torro Workplace…“ eines
  Bots → `--section bot:<id>` (beide Wörter gibt es heute; S.21 lässt sie nach Agents & API führen); „Add an AI account“ → `--section ai`
  (vor S.25 `accounts`).
- Der Bot-Editor verliert seinen Abschnitt „Access“ (Stufen je Bereich, „Ask first“); an seiner Stelle eine Zeile
  „Access ↗“ mit demselben Ziel. `bots.save_bot` bleibt, `clients.update_client` schickt der Editor nicht mehr.
- `torroassistant --settings <abschnitt>` öffnet das Fenster mit offenen Einstellungen. Nur ein Mensch öffnet
  die Einstellungen: Antworttext kann keinen Abschnitt öffnen (Links in Antworten bleiben auf `http(s)`
  beschränkt, kein `--settings` und kein eigenes Schema aus Antworten).
- **Abnahme:** Tests: Zahnrad hin und zurück, Esc, die Hinweise landen am richtigen Ziel, der Bot-Editor schickt
  kein `UpdateClient` mehr, `--settings voice` startet dort, ein Link `torro://…` oder ein Pfad in einer Antwort
  öffnet nichts; Sichtprüfung mit `--demo --settings general|bots|voice` bei 1100, 500 und 360 px, hell und dunkel
  (Agent `torro-design-review`); fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.19 Kalender und Aufgaben: Einstellungen im Inhaltsbereich
- [x] erledigt (geändert mit #210; Kalender bekommt die drei Abschnitte aus `collections.rs`
  (Calendars, `Role::Calendar`) und `calendar.rs` (Reminders, Night) in einer eigenen `SETTINGS_SECTIONS`
  in `torrocalendar/src/app.rs` – die Bibliothek hat kein eigenes Kalender-Modul, weil Calendars sich
  `collections.rs` mit Aufgabens Lists teilt. Aufgaben bekommt nur den einen Abschnitt Lists, direkt aus
  `collections::Role::TaskList`. Beide Apps: Zahnrad/„Settings“ hin und zurück, `--settings <abschnitt>`
  ohne `--demo`-Zwang, ein Klick in der Seitenleiste, der den Kalender bzw. die Liste wieder zeigt, schließt
  die Einstellungen (`shows_calendar`/`shows_list`), Escape zuletzt. Dabei einen bestehenden Fehler in beiden
  Apps gefunden und mitbehoben: die Escape- und die Tastatur-Subscription waren nicht an die Einstellungen
  gekoppelt, sonst hätte Escape bei offenen Einstellungen gar nichts getan. **Offen:** Sichtprüfung auf diesem
  Rechner nicht möglich – kein Hyprland, macOS, und den nicht committeten Wegwerf-Patch aus S.18 für Iceds
  `window::screenshot` habe ich für diese Aufgabe nicht neu angelegt; S.22 holt die Design-Prüfung der Stufe nach.)
- **Modell:** Sonnet · **Dateien:** `crates/torro-settings-ui/src/` (neu: `collections.rs`, `calendar.rs`),
  `crates/torrocalendar/{Cargo.toml,src/app.rs,src/main.rs}`, `crates/torrotasks/{Cargo.toml,src/app.rs,src/main.rs}`
- Kalender: Abschnitte **Calendars** (welche die Suite zeigt, je Konto gruppiert, `settings.set_collection_shown`;
  je gezeigtem Kalender die Art der Erinnerung Off/Notification/Full screen, `set_setting reminders`),
  **Reminders** (Show reminders, Default reminder, On all displays, Sound; Gruppe „Which events“ mit erklärendem
  Satz, den es heute nicht gibt) und **Night** (wie heute General › Calendar). Die Karte am Mond bleibt. Die
  Haken der Seitenleiste bleiben, wie sie sind (offene Frage 3 in settings.md).
- Aufgaben: Abschnitt **Lists** (welche die Suite zeigt, je Konto gruppiert).
- Das Zahnrad und „Settings“ im Menü öffnen die Einstellungen im Fenster; `--settings <abschnitt>` in beiden Apps.
  Der Hinweis ohne Konto („Open Settings“) öffnet weiter die Steuerzentrale bei Accounts.
- General der Steuerzentrale behält Calendar und Reminders bis S.21.
- **Abnahme:** Tests: Ausblenden in Calendars erreicht `SetCollectionShown`, die Art je Kalender und jeder Schalter
  unter Reminders senden `SetSetting(Reminders)` mit genau ihrem Feld, Night sendet `SetSetting(Night)`, Zahnrad hin
  und zurück; Sichtprüfung beider Apps mit `--demo --settings …` bei 1100, 500 und 360 px; fmt, clippy, test
  (auch `TORRO_LANG=de`) grün.

### S.20 Eingang und Mail: Einstellungen im Inhaltsbereich
- [x] erledigt (geändert mit #210; Notification Hub und App Feed zeigen Notifications, Sources und Rules aus
  `torro-settings-ui/src/inbox.rs`, das Mail-Fenster Accounts und Reading aus `mail.rs` – für „Keyboard
  shortcuts“ (E10.3) ist nur Platz gelassen. Jede Fensterart kennt nur ihre Abschnitte
  (`Kind::settings_sections`); Zahnrad, „Settings“ in Seitenleiste und Menü schalten den Inhaltsbereich um,
  `--settings <abschnitt>` nimmt nur die Wörter der eigenen Fensterart, Escape und jeder Klick, der wieder
  Einträge zeigt, schließen sie. „All rules“ öffnet Rules im Fenster; das Mail-Fenster, das den Abschnitt
  nicht hat, holt mit `apps.open_app` das Fenster des Notification Hub nach vorn („All rules in the
  Notification Hub ↗“), nicht die Steuerzentrale. Ein Eintrag mit `Link::App` auf `torroworkplace` hat
  eine eigene Nachricht (`OpenControlCentre`) und öffnet nie einen Abschnitt. Eine Mail-Quelle, die früher mit
  `MuteSource` stumm wurde, zeigt „Off“; beim Anschalten gehen `UpdateMailAccount` **und**
  `MuteSource { muted: false }` raus. Sichtprüfung über Iceds `window::screenshot` mit einem nicht
  committeten Wegwerf-Patch wie bei S.18: alle Abschnitte bei 1100, 800, 500 und 360 px, hell und dunkel.
  **Offen:** die deutschen Übersetzungen (sammelt S.22); `torro-settings-ui` hat dafür `torro-mail` als
  Abhängigkeit bekommen (`Notify`, `MailFormat`, `Role` der Mail-Einstellungen), was die Aufgabe nicht
  genannt hatte)
- **Modell:** Opus (viele Quellen, große App) · **Dateien:** `crates/torro-settings-ui/src/` (neu: `inbox.rs`,
  `mail.rs`), `crates/torroinbox/{Cargo.toml,src/app.rs,src/main.rs}`
- Fensterarten Notification Hub und App Feed: Abschnitte **Notifications** (still bei Terminen), **Sources** (je
  Konto: Nextcloud-Benachrichtigungen und Talk, GitHub, je ntfy-Konto (Server) ein Schalter über
  `inbox.mute_source` – stummschalten lässt sich nur der ganze Server, nicht ein Thema, je Mail-Konto
  „Show new mail“ mit All/Only „For me“/Off über `accounts.update_mail_account` Feld `notify`) und **Rules**
  (Liste mit „Remove“). Abo und Etikett der ntfy-Themen bleiben am Konto.
- Mail-Quellen, die früher über die Einrichtung mit `MuteSource` stumm wurden: Die Zeile zeigt dann „Off“; wer sie
  anschaltet, schickt `UpdateMailAccount` (`notify`) **und** `MuteSource { muted: false }`, damit keine alte Regel
  übrig bleibt.
- Fensterart Mail: Abschnitte **Accounts** (je Mail-Konto Absendername, Signatur, „Write new mail as“, offene
  Ordnerfragen) und **Reading** („Show mail as“, „Mail colors“). Den Abschnitt „Keyboard shortcuts“ baut E10.3.
- „All rules“ öffnet nun Rules im Fenster, das Zahnrad und „Settings“ die Abschnitte der Fensterart. Ein Eintrag mit
  `Link::App` auf `torroworkplace` (`open_entry`) öffnet weiter die Steuerzentrale und **nie** die Einstellungen im
  Fenster: Einträge können keinen Abschnitt aufschlagen (eigene Nachricht statt `OpenSettings`).
  `--settings <abschnitt>` für alle Fensterarten.
- Die Seite Notification Hub der Steuerzentrale und die Gruppen an den Kontoseiten bleiben bis S.21.
- **Abnahme:** Tests: jede Quelle schickt den Request, den heute die Kontoseite schickt (`MuteSource`,
  `UpdateMailAccount`; eine alte Mail-Stummschaltung wird beim Anschalten aufgehoben), ein Eintrag mit Link auf
  `torroworkplace` öffnet die Einstellungen nicht, Rules entfernt mit `RemoveRule`, Reading sendet `SetSetting(MailView|MailAppearance)`, jede
  Fensterart zeigt nur ihre Abschnitte; Sichtprüfung mit `--demo --settings sources|rules` (Notification Hub) und
  `--view mail --settings accounts|reading` bei 1100, 500 und 360 px; fmt, clippy, test (auch `TORRO_LANG=de`)
  grün.

### S.21 Steuerzentrale: nur noch die Suite
- [x] erledigt (geändert mit #210; die Seiten Apps, Assistant und Notification Hub sind gelöscht
  (`apps.rs`, `assistant.rs`, `bots.rs`, `persona.rs`, `inbox.rs`), die Seitenleiste hat nur noch
  Overview, Accounts, Agents & API, Approvals, Workspaces, Appearance und General am Fuß.
  **Agents & API** führt den Assistenten und jeden Bot wie jedes Programm: eigene Gruppen in der
  Liste, dieselbe Rechte-Ansicht in der Detailseite, „Turn off assistant…“ statt „Disconnect…“ beim
  Assistenten, beim Bot **kein zerstörender Knopf** (nachgezogen mit dem Review zu #210: von hier
  ließe sich sein Client nicht wieder anlegen, nur das nächste `bots.save_bot` im Assistenten;
  dafür hat die Bot-Seite wieder „Open chat“ wie vorher) und **kein Schlüssel-Block** – der Kern hält ihre Schlüssel, ein neuer würde
  sie nur zerreißen (`Category::has_key`). Ist der Assistent aus, steht seine Zeile trotzdem da und
  führt auf seinen ersten Start („Start from“ und die angebotenen Rechte, „Turn on assistant“ im
  Kopf, `clients.create_client` mit genau diesen Rechten); sein Schlüssel wird wie bisher verworfen.
  `--section assistant` und `bot:<id>` landen dort (`Launch::client`), `apps` und `inbox` auf General
  (`Section::from_arg`). **General** hat die Gruppe Apps (Schalter je Komponente und „Open“ mit der
  Fensterklasse, `torro-inbox-mail` für Mail; Workspaces hat kein Fenster und zeigt seine Seite) und
  keine Kalender- und Erinnerungsgruppen mehr; **Appearance** keine Gruppe Mail. Die Kontoseiten
  zeigen an Stelle der umgezogenen Gruppen je eine Zeile mit dem neuen Ort und „Open Calendar“,
  „Open Tasks“, „Open Notification Hub“ bzw. „Open Mail“ (`apps.open_app`, kein Ziel für einen
  Abschnitt). Die Einrichtung liest die Quelle Mail aus `notify` und schickt beim Anschalten
  `UpdateMailAccount` **und** `MuteSource { muted: false }` (dafür hat `Action` ein `SendAll`
  bekommen, der einzige Ort mit zwei Requests). `ui/general.rs` nutzt jetzt `parts::choice` und
  `parts::switch`; `ui/wizard.rs` stand nicht in der Dateiliste und bleibt bei seinem eigenen `field`.
  `ui/setup/pages.rs` brauchte keine Änderung: die Zeile fragt `State::is_muted`, das umgestellt
  wurde. Sichtprüfung über Iceds `window::screenshot` mit nicht committetem Wegwerf-Patch wie bei
  S.18/S.20: General, Appearance, Agents & API (Liste, Assistent an und aus, Bot, Agent, leer),
  Kontoseite, Mail-Konto und der Schritt Notification Hub der Einrichtung bei 1100, 500 und 360 px,
  hell und dunkel, dazu ans Seitenende gescrollte Bilder der langen Rechte-Seiten.
  **Offen:** die deutschen Übersetzungen (sammelt S.22, Liste im Bericht), und der Befund aus S.18
  gilt weiter: lange Zeilentexte in `parts::setting_row` werden bei 360 px hart abgeschnitten statt
  mit „…“)
- **Modell:** Opus · **Dateien:** `crates/torroworkplace/src/ui/{mod,launch,accounts,apps,assistant,bots,persona,
  inbox,general,agents}.rs`, `crates/torroworkplace/src/ui/setup/{mod,pages}.rs`
- **Braucht:** S.18, S.19, S.20 (die Abschnitte haben ihren neuen Ort, bevor die alten verschwinden)
- Seitenleiste: Overview, Accounts, (AI nach S.25), Agents & API, Approvals, Workspaces, Appearance; General am Fuß.
  Die Seiten **Apps**, **Assistant** und **Notification Hub** entfallen (`assistant.rs`, `bots.rs`, `persona.rs`,
  `inbox.rs`, `apps.rs` werden gelöscht; was die Seiten nutzen, haben S.17–S.20 in `torro-settings-ui` nachgebaut).
- **General** verliert Calendar und Reminders (jetzt im Kalender) und bekommt die Gruppe **Apps**: je Komponente
  der Schalter „On“ (`set_setting component`, wie heute Apps) und „Open“ (`apps.open_app` mit der Fensterklasse, also
  `torro-inbox-mail` für Mail; holt ein laufendes Fenster nach vorn; deaktiviert, solange die App nicht installiert
  ist), darunter der Satz „Its settings are behind the gear in its window.“ Workspaces hat kein Fenster und verweist
  auf die Seite Workspaces. **Appearance** verliert die Gruppe Mail.
- **Agents & API** führt den Assistenten und jeden Bot wie jedes Programm: Detailseite mit `agents::rights_groups`
  statt Weiterleitung zur Seite Assistant (`Action::OpenBot` entfällt). Ist der Assistent aus, zeigt seine Zeile
  „Turn on assistant“ mit „Start from“ wie heute die Seite Assistant (`CreateClient`), seine Detailseite „Turn off
  assistant…“ (`RemoveClient`). Die Zeile der Datenregel bleibt in der Rechte-Ansicht, bis S.25 sie nach AI holt. `--section assistant` und
  `bot:<id>` führen zu diesen Detailseiten, `apps` und `inbox` zu General.
- **Kontoseiten** verlieren „Calendars and lists“, die Gruppe „Notification Hub“ (Nextcloud, GitHub), Absender,
  Signatur, „Write new mail as“, „Show new mail in the Notification Hub“ und offene Ordnerfragen; an ihrer Stelle je
  eine Zeile mit dem neuen Ort („Calendars and lists are chosen in Calendar and Tasks, under the gear.“) und je App
  ein Knopf „Open Calendar“, „Open Tasks“ bzw. „Open Notification Hub“, „Open Mail“ (`apps.open_app`). Abo, Etikett und
  Login der ntfy-Themen, Dienste und Verbindung bleiben.
- **Einrichtung:** Die Quelle „Mail“ im Schritt Notification Hub liest ihren Zustand aus `notify` (statt aus den
  Quellen, `is_muted` in `setup/mod.rs`) und schickt beim Anschalten wie S.20 `UpdateMailAccount` **und**
  `MuteSource { muted: false }`, beim Ausschalten nur `UpdateMailAccount` (zusammengelegt, settings.md › Was wohin
  gehört).
- Ein Ziel `settings:<abschnitt>` für `apps.open_app` gibt es bewusst nicht: Nur ein Mensch öffnet die Einstellungen
  (settings.md › Technik). Die Einstellungen in einem schon laufenden Fenster direkt aufzuschlagen bräuchte einen neuen
  Weg vom Kern zur App; das ist nicht Teil dieser Stufe.
- **Abnahme:** Tests für die Navigation, die alten und neuen `--section`-Wörter, Assistent und Bot als Programme
  unter Agents & API mit Rechten, General › Apps (Schalter und „Open“ mit Fensterklasse), die kürzeren Kontoseiten und die Quelle Mail
  der Einrichtung; Sichtprüfung `--demo` bei 1100, 500 und 360 px; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.22 Abschluss
- [x] erledigt (2026-10-19: 40 Übersetzungen eingetragen, 32 verwaiste Einträge entfernt; `docs/settings.md`
  datiert den Bestand als Stand vor Stufe 2 und nennt den gebauten; offene Frage 2 beantwortet („Connect program“),
  offene Frage 3 bleibt offen. Sichtprüfung auf macOS über Iceds `window::screenshot` mit einem nicht committeten
  Wegwerf-Patch, 78 Bilder bei 1100/500/360 px hell und dunkel. Behoben: `parts::setting_row` kürzt mit „…“ und
  Tooltip statt hart abzuschneiden, und im Kalender rückt die Erinnerungsart unter den Namen, sobald er sonst
  verschwindet (`collections.rs`); dazu der Hinweis der API-Prüfung, dass der Socket-Wächter von
  `torro-settings-ui` auch `InboxClient`, `PimClient` und `socket_path` kennen muss.
  **Offen, je ein eigenes Issue wert:** Kopfleiste und Seiteninhalt liegen nicht auf einer Achse (Shell, gilt für
  alle Apps); eine Zeile mit Marke rückt gegen ihre Nachbarn ein (`parts::setting_row` kennt die Gruppe nicht);
  Name und Fehlergründe eines unbrauchbaren Design-Pakets mit zu wenig Kontrast und Fachjargon (`ui/general.rs`,
  aus L.4); zwei sichtbare Aktionen je Zeile unter General › Apps (so im Konzept verlangt, dem Design-Skill
  entgegen); der App Feed zeigt die Texte des Notification Hub (eine `SECTIONS` für beide Fensterarten, S.20);
  gerade Anführungszeichen in „Only "For me"“ (aus E4.1). Der leere `text_editor` des Bot-Editors aus S.18 war
  an diesen Bildern nicht prüfbar: Die Unterseite hat kein `--settings`-Wort.)
- **Modell:** Opus · **Dateien:** `crates/torro-core/src/i18n/de.rs`, `docs/settings.md`, `docs/assistent.md`,
  `docs/eingang.md`, `docs/einrichtung.md`
- Übersetzungen eintragen (`crates/torro-core/src/i18n/de.rs`), `docs/settings.md` an den gebauten Stand anpassen,
  Design-Prüfung aller Zahnrad-Ziele (Assistent, Kalender, Aufgaben, Notification Hub und App Feed, Mail-Fenster) in
  allen Größen und der Steuerzentrale (Agent `torro-design-review`), `torro-api-review` über die
  ganze Stufe, Fehler beheben.
- Die übrigen Konzeptdokumente nennen noch die Orte, die mit S.21 verschwinden: `docs/assistent.md`
  („Steuerzentrale › Assistant“ bei Rechten, Voice, Profil, Bots), `docs/eingang.md` („Steuerzentrale ›
  Notification Hub › Rules“) und `docs/einrichtung.md` („Seite Apps“). Sie werden auf den neuen Ort umgeschrieben;
  abgehakte Aufgaben in `docs/tasks/` bleiben, wie sie sind, sie beschreiben den damaligen Stand.
- **Abnahme:** `grep -rn "Steuerzentrale › Assistant\|Steuerzentrale › Notification Hub\|Seite Apps" docs/` findet
  nur noch abgehakte Aufgaben; fmt, clippy, test (auch `TORRO_LANG=de`) grün, `scripts/ci-local.sh` grün.

## Stufe 3: KI (S.23–S.26)


Konzept: [settings.md › KI](../settings.md#ki), entschieden am 2026-10-15. Zentral richtet man Anbieter ein und schaltet
ihre Modelle an und ab (Steuerzentrale › AI); welches Modell ein Modul wofür nimmt, stellt man im Modul ein. Anlass: Ein
verbundenes Ollama erschien weder unter „Agents & API“ noch auf seiner Kontoseite mit Modellen; dabei fiel auf, dass
seine Cloud-Modelle für die Datenregel als lokal galten (S.23).

Nicht Teil von #210 (in dessen zweitem Plan entschieden, 2026-10-17): Stufe 3 braucht ein eigenes Issue, weil S.24 einen
neuen Befehl bringt. Sie baut auf Stufe 2 auf, wenn die schon steht (Hinweise „bis S.21“ und „sobald S.17 da ist“).

S.24 legt den Befehl und die Regel im Kern fest, danach laufen S.25 (Seite AI) und S.26 (Auswahlen der Module)
parallel. S.26 und S.17 berühren beide die Auswahl „New chats“: Wer später kommt, übernimmt den Filter bzw. den Umzug in
`torro-settings-ui`.

```
S.23 (Datenregel, erledigt)
S.24 (Modelle an/aus im Kern) ──┬── S.25 (entfällt mit #231, ersetzt durch S.31 in Stufe 5)
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
  Anbieter; Anschalten eines Modells, das der Anbieter nicht meldet; Ausschalten geht auch
  dann), `offline`, `auth`, `not-connected`, `busy` (Anschalten fragt den Server nach seinen Modellen),
  `forbidden`. Katalogbeschreibung englisch: „Turns a model of an AI provider on or off for the whole suite; modules
  offer only models that are on. Turning on a model that runs elsewhere is kept for people.“
- **Nur Menschen schalten entfernte Modelle an:** `enabled: true` für ein Modell, das anderswo rechnet (`remote`), gilt
  wie `mail_to_cloud: true` als `kept_for_people` (`docs/api.md` Punkt 6 ergänzen); ein Agent bekommt `forbidden` mit
  Satz. Ausschalten und das Anschalten eines Modells, das hier rechnet, sind gewöhnliche Admin-Befehle mit Freigabe.
  Die Zusammenfassung der Freigabe (`approvals::Facts`) nimmt Anbieter, Modell und Ort aus der Liste des Kerns, nie aus
  dem Text des Clients.
- **Lesen:** `ai.list_models` liefert je Modell das neue Feld `enabled` (verträglich, `#[serde(default)]`; der Kern
  setzt es aus der Ablage, `account/ai.rs` setzt es beim Abfragen des Servers nie). `account` ist immer ein KI-Konto:
  Die Kennung `claude-code` nimmt `ai.list_models` **nicht** (geändert mit #219, C.18 — Antrieb B wird nicht mehr
  angeboten, und Leserecht in `ai` soll nicht verraten, ob ein Befehl auf dem Rechner liegt); für ein unbekanntes
  `account` bleibt es `not-found`.
- **Ablage:** eigene Datei `<config>/ai-models.json` (0600, atomar geschrieben): je Anbieter die angeschalteten Modelle und
  ob der Mensch dort schon etwas ausgeschaltet hat. **Nicht** in `settings.json`, damit `settings.update_settings`
  (Bereich `settings`) die Rechte in `ai` nicht umgeht und gleichzeitige Schaltvorgänge nicht überschreibt. Protokoll
  (`audit.rs`) nur mit Anbieter und `enabled`, ohne Modellnamen.
- **Regel am Anfang** (settings.md › KI): an sind die Modelle mit Werkzeugen, die auf diesem Rechner rechnen; aus sind
  Ollama Cloud, OpenRouter, Anthropic und Server im Netz. Beim ersten Start nach dem Update sind zusätzlich die
  Modelle an, die schon genutzt werden: `assistant_default` und die Modelle der Bots. Die pro Unterhaltung gewählten
  Modelle schalten nichts an; sie fallen wie unten auf „Automatic“, mit Hinweis.
- **Der Kern hält sich daran, in dieser Reihenfolge:** erst das Modell (ein gespeichertes, aber abgeschaltetes wird
  durch „Automatic“ unter den angeschalteten Modellen desselben Anbieters ersetzt, lokal zuerst wie S.23), dann der Ort,
  dann Datenregel, Prompt und `local_only` (wie S.23). Das gilt für `assistant.send_message` und Bots ohne oder mit
  abgeschaltetem Modell (`delegate.rs`). Engine B fällt weg: Sie wird seit #219 (C.18) nicht mehr angeboten.
- **Ein Anbieter ganz aus:** `assistant.list_engines` nennt nur Anbieter mit mindestens einem angeschalteten Modell
  (Katalogtext nachziehen: KI-Konten, mit `models`). `send_message` und `ask_bot` mit einer solchen
  Engine antworten `unsupported` mit „Turn on a model of {provider} in Settings › Accounts.“ (geändert mit #231); kein Ausweichen auf einen
  anderen Anbieter. Die Beschreibungen von `assistant.send_message` und `bots.save_bot` sagen, dass ein abgeschaltetes
  Modell ersetzt wird und die Antwort das gewählte nennt.
- **Oberfläche:** Steuerzentrale › Accounts › Kontoseite des KI-Kontos (S.31; geändert mit #231, S.25 entfällt)
- **API:** `Request::SetModelEnabled` · `ai.set_model_enabled` · Admin; `Request::AiModels` · `ai.list_models` · Read
  (neues Feld `enabled`)
- **MCP:** `ai_set_model_enabled` (Admin, beim Agenten mit Freigabe; entfernte Modelle anschalten nur Menschen);
  `ai_list_models` (Beschreibung ergänzt)
- **Abnahme:** Handler-Tests (Temp-Verzeichnisse, gefälschter Ollama):
  - Schalten wirkt, überlebt einen Neustart und steht nicht in `settings.get_settings`;
  - Regel am Anfang: lokales Modell an, `:cloud`, OpenRouter und Anthropic aus; genutzter Standard und Bot-Modell an;
  - ein Agent kann ein entferntes Modell nicht anschalten (`forbidden`), ein lokales nur mit Freigabe;
  - ist ein lokales Modell aus und nur ein `:cloud`-Modell desselben Kontos an, nimmt „Automatic“ das Cloud-Modell **und**
    die Datenregel behandelt es als entfernt;
  - Rechte-Tabelle und MCP-Vertragstest grün (automatisch); API-Referenz neu erzeugt
    (`TORRO_BLESS=1 cargo test -p torro-ipc`); fmt, clippy, test (auch `TORRO_LANG=de`) grün; Agent `torro-api-review`.

### S.25 Steuerzentrale › AI: Anbieter und ihre Modelle
- [x] entfällt – gestrichen (#231): KI-Konten bleiben unter Accounts, die Modelle stehen auf der Kontoseite (S.31), die
  Datenregel unter der Gruppe AI (S.30). Der Text bleibt zum Nachlesen stehen.
- **Modell:** Opus (neue Seite, Umzug aus Accounts) · **Braucht:** S.24 · **Dateien:**
  `crates/torroworkplace/src/ui/ai.rs` (neu), `crates/torroworkplace/src/ui/mod.rs`,
  `crates/torroworkplace/src/ui/wizard.rs`, `crates/torroworkplace/src/ui/accounts.rs`,
  `crates/torroworkplace/src/ui/assistant.rs` und `crates/torroworkplace/src/ui/bots.rs` (beide nur bis S.21),
  `crates/torroworkplace/src/ui/agents.rs`,
  `crates/torroworkplace/src/ui/launch.rs`
- Neuer Abschnitt **AI** in der Seitenleiste nach Accounts (`Section::Ai`, `--section ai`, Symbol nach dem Design-Skill
  aus dem vorhandenen Satz). Aufbau nach settings.md › KI: Karten der Anbieter (Open First, Status, „3 of 10 models
  on“), „Add provider“, je Anbieter eine Unterseite mit Status, Verbindung, „Models“ mit Merkmalen, Ort („On this
  computer“, „Ollama Cloud“, „Cloud“) und Schalter, Suche und „Show closed models“ bei OpenRouter, „Remove provider…“.
  Keine Karte „Claude Code“: Antrieb B wird seit #219 (C.18) nicht mehr angeboten, Claude ist ein KI-Konto
  (Anthropic). Die Karten zeigen „3 models on“ aus der Ablage (über
  `ai.list_models` erst auf der Unterseite), nicht „3 of 10“, damit beim Öffnen der Seite nicht jeder Server gefragt
  wird. Der Ort steht je Modell: „On this computer“, „Ollama Cloud“, „Cloud (Anthropic)“, „Cloud“.
- „Add provider“ öffnet den vorhandenen Dialog (`wizard.rs`) gleich bei den KI-Anbietern; der Katalog „Add account“
  verliert den Eintrag „AI providers“, die Liste unter Accounts die KI-Konten. Wer `--section accounts` mit einem
  KI-Konto erwartet (alte Verweise), landet über `--section ai` richtig: Die Zeile „Engines“ der Assistentenseite (bis S.21)
  und „No AI account yet“ öffnen künftig AI statt Accounts.
- **Datenregel** (#210): Gruppe „Mail“ auf der Seite AI mit dem Schalter „Mail to cloud models“ (C.20)
  (`set_setting mail_to_cloud`, anschalten nur mit dem App-Schlüssel wie heute) und einem Satz, dass er für den
  Assistenten und alle Bots gilt. Die Zeile verschwindet aus der Rechte-Ansicht (`agents::access_group`),
  damit sie nicht mehr wie ein Recht eines Programms aussieht.
- Modelle werden beim Öffnen einer Unterseite geladen (`ai.list_models`), nicht bei jedem Takt; „Loading models…“,
  Fehler des Servers als Satz in der Gruppe.
- **Oberfläche:** Steuerzentrale › AI
- `accounts.list_accounts` liefert die KI-Konten weiter (Agenten brauchen ihre Kennungen für `ai_list_models`); nur die
  Liste unter Accounts in der Oberfläche blendet sie aus.
- S.25 und S.21 ändern beide `ui/accounts.rs`: nicht gleichzeitig; wer später kommt, übernimmt den Filter bzw. die kurzen
  Kontoseiten.
- **API:** `ai.list_models` · Read; `ai.set_model_enabled` · Admin; `ai.find_local_servers` · Read;
  `accounts.connect_ai` · Admin; `accounts.remove_account` · Admin;
  `accounts.list_accounts` · Read
- **MCP:** `ai_list_models`, `ai_set_model_enabled`, `ai_find_local_servers`,
  `accounts_remove_account`;
  `accounts.connect_ai` bleibt **Never** (Schlüssel nur über die Oberfläche)
- **Abnahme:** Tests mit Demo-Daten: Seite in der Navigation und per `--section ai`, Reihenfolge Open First, ein Schalter
  baut `SetModelEnabled`, Modelle ohne Werkzeuge ohne Schalter, keine Karte „Claude Code“, Katalog ohne KI-Anbieter,
  Accounts ohne KI-Konten, die Datenregel nur auf AI und nicht in der Rechte-Ansicht. Demo-Ansichten `--demo --section ai` und eine Unterseite für die Sichtprüfung. Sichtprüfung bei
  1100, 500 und 360 px, hell und dunkel (Agent `torro-design-review`); fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.26 Die Auswahlen der Module bieten nur angeschaltete Modelle
- [ ] erledigt (geändert mit #210: Ort von New chats; mit #231: Verweise auf Accounts statt AI)
- **Modell:** Sonnet · **Braucht:** S.24 · **Dateien:** `crates/torroworkplace/src/ui/assistant.rs` (bis S.21) und
  `crates/torro-settings-ui/src/assistant/` (sobald S.17 da ist), `crates/torro-settings-ui/src/mail.rs` (#231),
  `crates/torroassistant/src/view.rs`, `crates/torroassistant/src/view/editor.rs`, `crates/torroassistant/src/app.rs`
- Assistant › New chats (heute in der Steuerzentrale, nach S.17 in `torro-settings-ui`), die Auswahl pro Chat und der
  Bot-Editor zeigen nur Modelle mit `enabled`. „Show all models“ im Chat entfällt; an seiner Stelle „Manage models…“,
  das die Steuerzentrale bei Accounts öffnet (vorhandener Weg `torro_gui::connect::open_section("accounts")`, kein
  neuer Request; `--section ai` gibt es mit #231 nicht).
- Ist bei keinem Anbieter ein Modell an: „No model is turned on. Turn on models in Settings › Accounts.“ mit demselben
  Knopf.
- Ein gespeichertes, aber abgeschaltetes Modell (Standard, Bot) steht oben in der Auswahl mit „Turned off in Settings ›
  Accounts“, damit man sieht, warum „Automatic“ antwortet. Auch die Auswahl der Inbox für „Mail einordnen“
  (`torro-settings-ui/src/mail.rs`, `categorization::offered`) bietet nur angeschaltete Modelle (#231).
- **Oberfläche:** Assistent › Einstellungen › General › New chats und, bis S.21, Steuerzentrale › Assistant › New
  chats (beide gefiltert); Assistent ›
  Auswahl im Chat; Bot-Editor
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
- [x] erledigt (2026-10-16: Entwurf B gewählt; E10.3 brauchte keine Änderung, S.20 legt `mail.rs` weiter an)
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

## Stufe 5: Accounts, API und MCP (S.30–S.35, #231)

Konzept: [settings.md › Accounts, API und MCP](../settings.md#accounts-api-und-mcp-231), [api.md](../api.md) (Rechte,
Transport, Entscheidungen 9–12), [ki.md](../ki.md). Entschieden am 2026-10-19 (#231): drei Abschnitte Accounts, API und
MCP statt „Agents & API“; „Add“ je Gruppe statt Katalog; KI-Konten mit ihren Modellen unter Accounts (S.25 entfällt);
ein Rechte-Editor mit Schaltern je Befehl für API, MCP und die Rechte von Assistent und Bots, die ins Assistent-Fenster
ziehen; Hooks auf der Seite ihres Agenten; Schalter „Allow API keys“ und „Allow MCP“.

S.24 und S.26 aus Stufe 3 gehören dazu (Modelle an/aus). Alle Änderungen an Requests, Antworten, `Rights` und
Einstellungen stehen in S.32 (Gerüst); die Oberflächen-Aufgaben bauen darauf. `crates/torro-core/src/i18n/de.rs`
trägt nur die Koordination ein; jede Aufgabe nennt ihre Übersetzungen im Bericht.

```
S.24 (Modelle an/aus im Kern) ──┬── S.26 (Auswahlen filtern)
                                └── S.31 (KI-Konto: Models) ◄── S.30
S.30 (Accounts: Gruppen, Add je Gruppe) ──┬── S.31
                                          └── S.33
S.32 (Kern: Sperrliste, Weg, Schalter) ──┬── S.33 (Rechte-Editor) ── S.34 (Access im Assistenten) ── S.35 (API und MCP)
                                         └── S.35
```

S.30 und S.32 können parallel laufen (gemeinsam nur `ui/launch.rs`: S.30 die Demo-Wörter, S.32 die Demo-Antworten auf
`CreateClient`; wer später kommt, führt zusammen). S.24 und S.32 ändern dieselben Kerndateien (`lib.rs`, `api.rs`,
`core.rs`, `mcp.rs`) und laufen nacheinander, S.24 zuerst. S.31 und S.33 ändern beide `ui/mod.rs` und laufen
nacheinander. Empfohlene Reihenfolge: S.30 ∥ S.24 → S.32 → S.31 ∥ S.26 → S.33 → S.34 → S.35.

### S.30 Accounts: feste Gruppen, „Add“ je Gruppe, TorroMail-Import, Datenregel
- [ ] erledigt
- **Modell:** Sonnet · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`, `crates/torroworkplace/src/ui/wizard.rs`,
  `crates/torroworkplace/src/ui/setup/mod.rs`, `crates/torroworkplace/src/ui/mod.rs`,
  `crates/torroworkplace/src/ui/launch.rs`, `crates/torroworkplace/src/ui/agents.rs` (nur die Datenregel heraus)
- **Gruppen:** `accounts::providers` bildet feste Gruppen in der Reihenfolge Nextcloud, Mail (Arten `mail` und `caldav`),
  GitHub, AI (`AI_KIND`), ntfy (`NTFY_KIND`); jede Gruppe erscheint auch ohne Konto, mit einem Satz je Gruppe, was
  hierher gehört. Unbekannte Arten bleiben als eigene Gruppe am Ende. Die Zeile eines CalDAV-Kontos nennt „CalDAV“ als
  Art (wie heute auf der Kontoseite). `empty_state` der ganzen Liste entfällt.
- **„+ Add“ je Gruppe:** neue `Message::Add(wizard::Provider)`; ein Helfer `wizard::State::at(provider, data) -> (State,
  Action)` macht, was heute `setup::open_dialog(Some(p))` tut (`State::new()` und `ChooseProvider(p)`, inklusive
  `FindAiServers` bei AI); `setup/mod.rs` nutzt ihn mit. Der Knopf „Add account“ in `accounts::header` entfällt; die
  Kopfleiste der Liste hat keinen Primärknopf mehr, die Gruppen tragen „Add“ (Stil nach Skill `torro-design`, ein
  Knopf je Gruppe, kein Rot).
- **Dialog ohne Katalog:** Ein Dialog, der bei einem Anbieter beginnt, zeigt auf dessen erstem Schritt kein „Back“
  (`footer`), und `Message::Back` führt dort nicht zu `Step::Provider`. Die Zeile „Slack – Coming soon“ im Katalog
  entfällt (`coming_soon_row` bleibt für die Dienste des Mail-Wegs). `Step::Provider` darf bleiben, wenn Tests oder die
  Einrichtung ihn brauchen, ist aber von Accounts aus nicht mehr erreichbar.
- **TorroMail:** neuer Schritt `Step::TorroMail`, der nur die Liste zum Übernehmen zeigt (`offer_group`, die Logik aus
  `Offer` und `offer_answered` unverändert), geöffnet über `wizard::State::torromail() -> (State, Action)` (sendet
  `Request::TorroMailAccounts`). Die Gruppe Mail zeigt „Import from TorroMail…“ nur, wenn es Konten gibt, die
  `Offer::can_use` erlaubt; dafür fragt die Seite `Request::TorroMailAccounts` beim Öffnen von Accounts und nach dem
  Schließen des Dialogs (nicht bei jedem Neuladen) und merkt die Antwort im `State` der Seite. „Take over from TorroMail“
  der Einrichtung öffnet denselben Schritt. „Enter password“ einer fehlgeschlagenen Übernahme führt wie heute in den
  Mail-Weg.
- **Datenregel:** unter der Gruppe AI eine Zeile „Mail to cloud models“ mit Schalter und Satz (für Assistent und alle
  Bots), `Request::SetSetting { key: MailToCloud, … }` wie heute; `data_rule_row`, `shows_data_rule` und
  `Message::MailToCloud` verlassen `agents.rs`.
- **Demos:** die `--open`-Wörter `wizard-*` öffnen den Dialog beim jeweiligen Anbieter; neu `wizard-torromail` für den
  Schritt TorroMail; `wizard` (Katalog) entfällt aus der Liste der gültigen Wörter, die Fehlermeldung nennt die neuen.
- **Oberfläche:** Steuerzentrale › Accounts; Einrichtung › Mail
- **API:** keine neue Funktion; dieselben Requests (`accounts.connect_*`, `accounts.list_torromail_accounts`,
  `accounts.import_torromail_account`, `settings.set_setting mail_to_cloud`) an anderer Stelle
- **MCP:** unverändert (`accounts_list_torromail_accounts`; die Verbindungs-Befehle bleiben Never)
- **Abnahme:** Tests: feste Reihenfolge der Gruppen auch ohne Konten (ersetzt
  `one_group_per_provider_in_order_of_appearance`), CalDAV in Mail, „Add“ jeder Gruppe öffnet den richtigen ersten
  Schritt (AI sendet `FindAiServers`), kein „Back“ auf dem ersten Schritt, Import-Knopf nur mit übernehmbaren Konten,
  Schritt TorroMail übernimmt wie bisher; die Tests des Katalogs (`five_providers_open_the_catalog`,
  `disabled_providers_do_nothing`, `adding_is_possible_with_accounts`, `the_dialog_takes_only_its_own_answers`, die
  Back-zu-`Step::Provider`-Prüfungen) passen auf den neuen Ablauf; Datenregel nur unter AI. Sichtprüfung Accounts
  (leer, mit Konten, Dialog bei Mail und bei TorroMail) in allen Größen, hell und dunkel (Agent `torro-design-review`);
  fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.31 KI-Konto: Modelle an- und abschalten auf der Kontoseite
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** S.24, S.30 · **Dateien:** `crates/torroworkplace/src/ui/accounts.rs`,
  `crates/torroworkplace/src/ui/mod.rs`, `crates/torroworkplace/src/ui/launch.rs`
- Ersetzt S.25. Die Kontoseite eines KI-Kontos (`accounts::detail` mit `AI_KIND`) bekommt zwischen Status und
  Verbindung die Gruppe **„Models“** nach settings.md › KI: eine Zeile je Modell mit Name, Merkmalen (Tools, Thinking aus
  `reasoning`, Context), Ort („On this computer“, „Ollama Cloud“, „Cloud (Anthropic)“, „Cloud“ aus `remote` und
  Anbieter) und Schalter (`Request::SetModelEnabled`); Modelle ohne Werkzeuge blass mit „Can't use tools“ ohne Schalter.
  Bei OpenRouter Suche und „Show closed models“ (`open_weights`).
- Die Modelle lädt die Seite beim Öffnen der Kontoseite (`Request::AiModels { account, all: true }`), nicht bei jedem
  Neuladen; „Loading models…“; ein Fehler des Servers als Satz in der Gruppe. Ein Schalter zeigt den neuen Stand sofort
  und nimmt ihn zurück, wenn der Kern ablehnt (Satz unter der Gruppe).
- Die Zeile des Kontos in der Gruppe AI nennt „{n} models on“ aus der letzten Antwort, sonst nur den Status.
- Der Anbieter steht nur in der Kennung (`ai:<provider>@<host>`); ein vorhandener Helfer zum Zerlegen wird benutzt,
  sonst einer in `accounts.rs` mit Test.
- **Oberfläche:** Steuerzentrale › Accounts › KI-Konto
- **API:** `ai.list_models` · Read; `ai.set_model_enabled` · Admin (aus S.24)
- **MCP:** `ai_list_models`, `ai_set_model_enabled` (aus S.24)
- **Abnahme:** Tests mit Demo-Daten: Gruppe Models, ein Schalter baut `SetModelEnabled`, Modell ohne Werkzeuge ohne
  Schalter, Ort je Modell, Suche und „Show closed models“ bei OpenRouter, Rücknahme nach Ablehnung. Demo
  `--open account-detail-ai`. Sichtprüfung in allen Größen, hell und dunkel; fmt, clippy, test (auch `TORRO_LANG=de`)
  grün.

### S.32 Kern: Sperrliste je Befehl, Weg im Umschlag, Schalter für API und MCP
- [ ] erledigt
- **Modell:** Opus (Rechte, Schlüssel, Schnittstellen) · **Dateien:** `crates/torro-ipc/src/api.rs`,
  `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/client.rs`, `crates/torro-ipc/src/server.rs`,
  `crates/torro-core/src/settings.rs`, `crates/torroworkplace/src/settings_service.rs`,
  `crates/torroworkplace/src/clients.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`,
  `crates/torroworkplace/src/mcp.rs`, `crates/torroworkplace/src/hook.rs`, `crates/torroworkplace/src/assistant/tools.rs`, `crates/torroworkplace/src/ui/launch.rs`
  (nur die Demo-Antworten auf `CreateClient`), `docs/api-reference.md`
- **Schnittstellen (mit dieser Planung freizugeben):**
  - `Rights::denied: BTreeSet<String>` (`#[serde(default, skip_serializing_if = "BTreeSet::is_empty")]`): Katalognamen
    gesperrter Befehle. `Rights::allows` lehnt einen Befehl ab, dessen `command.name` darin steht; `system.ping` nie.
    `Rights::common` vereinigt die Sperrlisten. `clients.rs` verwirft beim Laden **und** bei `clients.update_client`
    Namen, die `CATALOGUE` nicht kennt, und `system.ping`. Ausnahme, ausdrücklich: Befehle, die eine Art schon wegen
    ihrer Art darf (`api::allowed_by_kind`: Gedächtnis, `bots.*`, `my_access` des Assistenten), sperrt `denied` nicht;
    der Editor zeigt diese Bereiche ohnehin nicht (`shows_area`).
  - Feld `via` im Umschlag mit dem Typ `enum Door { Api, Mcp }` (snake_case; nicht `Via`, das gibt es privat schon in
    `core.rs`): `Envelope::via: Option<Door>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), ebenso
    in `RawEnvelope`; fehlt es, gilt `Api`. Neue Signaturen: `Core::handle_from(key, door, request)` und der Handler von
    `serve_with_key` `Fn(Option<Key>, Door, Request)`. `client.rs` bekommt einen Weg, `via` mitzuschicken (z. B.
    `call_with_key_via`); `mcp.rs` und `hook.rs` senden `Door::Mcp` (der Hook nimmt ohne Schlüsseldatei `TORRO_TOKEN`,
    das ein Agent-Key sein kann), alle anderen bleiben ohne.
  - Einstellungen `allow_api_keys` und `allow_mcp` (`Settings`, Standard `true` über eine eigene Default-Funktion),
    `SettingKey::AllowApiKeys` und `SettingKey::AllowMcp`, in `settings_service.rs` wie `MailToCloud`.
  - `clients.create_client` mit der Art `assistant` antwortet ohne Schlüssel: neue Antwort `Response::Client { client:
    ClientInfo }` (neue Antwortart, verträglich nach api.md „Versionen“); die Katalogbeschreibung sagt das.
  - Neue Client-Art `mcp` („Other MCP client“) neben `script`; `create_client` nimmt sie an. Wie bei `script` ist der
    Name ihre Quelle im Eingang (`post_source`: `agent:mcp:<name>` statt eines gemeinsamen `agent:mcp`) und muss
    eindeutig sein (`script_name_taken` erweitert).
  - **Arten geschlossen:** `create_client` lehnt Arten außer `script`, `mcp`, den fünf Agenten-Arten, `hook` und
    `assistant` mit `error` ab (`bot` legt weiter nur `bots.save_bot` an). Eine unbekannte Art aus einer älteren
    `clients.json` wird bei Weg und Schaltern wie `script` geprüft.
  - `clients.new_key` lehnt die Arten `assistant` und `bot` mit `unsupported` ab (bisher verhinderte das nur die
    Oberfläche).
- **Prüfung im Kern** (`Core::handle_from`, direkt nachdem der Schlüssel einem Client zugeordnet ist, vor den Rechten,
  vor den Befehlen, die eine Art von selbst darf, und auch vor `system.ping` mit Schlüssel, damit ein abgeschalteter Weg
  immer gleich mit `unauthorized` antwortet; Regel in api.md › Transport): Art
  `script` nur mit `allow_api_keys` und über `Api`; MCP-Arten (`claude-code`, `codex`, `gemini`, `claude-desktop`,
  `cursor`, `mcp`) nur mit `allow_mcp` und über `Mcp`; `hook` nur mit `allow_mcp`, über beide Wege; die Arten `assistant`
  und `bot` ohne diese Prüfung (nur sie bekommen geliehene Schlüssel, `Clients::lent_for`); jede andere Art wie
  `script`. Ablehnung `unauthorized` mit Satz
  („The API is turned off in Torro Workplace, Settings › API.“ bzw. „… MCP …“, „This key belongs to an MCP client.“),
  Protokollzeile wie bei anderen Ablehnungen, keine Freigabe.
- **Freigaben:** `run_approved` prüft vor dem Ausführen die Schalter nach der Art erneut (die Sperrliste prüft es mit
  den Rechten schon): Eine Freigabe, die angelegt wurde, solange MCP an war, läuft nach „Allow“ nicht, wenn MCP
  inzwischen aus ist oder der Befehl inzwischen gesperrt.
- **Wo die Sperre sichtbar wird:** `Lack::of_in` (`clients.rs`) bekommt einen eigenen Grund („turned off for this
  command“), damit die Meldung nicht zu einer höheren Stufe rät; `MyAccess::of` nennt gesperrte Befehle; die
  Werkzeugliste des Assistenten (`assistant/tools.rs`) lässt gesperrte Befehle weg.
- **Nur Menschen schalten ein:** `Request::kept_for_people` um `AllowApiKeys` und `AllowMcp` mit Wert ungleich `false`
  erweitern, `switches_on_for_people` um beide Felder (wie `mail_to_cloud`); `docs/api.md` Punkt 6 steht schon.
- **Texte des MCP-Servers:** `INSTRUCTIONS` und `KEY_HINT` in `mcp.rs` nennen „Torro Workplace, Settings › MCP“ statt
  „Agents & API“.
- **Oberfläche:** keine in dieser Aufgabe (S.30, S.33–S.35)
- **API:** keine neuen Befehle. `clients.update_client` · Admin, Never (trägt `denied` in den ganzen Rechten);
  `clients.create_client` · Admin, Never (Antwort `client` für `assistant`, Art `mcp`); `settings.set_setting` · Admin
  (`allow_api_keys`, `allow_mcp`; einschalten nur App-Schlüssel); `settings.get_settings` · Read (neue Felder)
- **MCP:** `settings_set_setting` (ausschalten mit Freigabe, einschalten `forbidden`); `clients.*` bleiben Never
- **Abnahme:** Tests in `torro-ipc` und im Kern (Temp-Verzeichnisse):
  - ein gesperrter Befehl ist `forbidden`, obwohl die Stufe ihn erlaubt; ein nicht gesperrter Befehl desselben Bereichs
    geht; `system.ping` lässt sich nicht sperren; unbekannte Namen verschwinden beim Laden; alte `clients.json` ohne
    `denied` lädt unverändert; `update_client` verwirft unbekannte Namen und `system.ping`; Befehle aus
    `allowed_by_kind` bleiben trotz Eintrag erlaubt; die Meldung nennt den Grund „turned off“; `my_access` und die
    Werkzeugliste des Assistenten lassen gesperrte Befehle weg;
  - Umschlag ohne `via` ist `Api`; `mcp.rs` schickt `Mcp` (Test, dass das Tool den Umschlag so baut);
  - Script über `Api` geht, über `Mcp` nicht; Agent über `Mcp` geht, über `Api` nicht; Hook über beide Wege; der
    Hook-Befehl mit einem Agent-Key aus `TORRO_TOKEN` meldet weiter (er schickt `Mcp`); mit
    `allow_api_keys` aus wird Script abgelehnt, Agent nicht; mit `allow_mcp` aus werden Agent und Hook abgelehnt, ein
    geliehener Schlüssel des Assistenten nicht; eine unbekannte Art aus `clients.json` verhält sich wie `script`; mit
    abgeschaltetem Weg antwortet auch `system.ping` mit Schlüssel `unauthorized`;
  - Freigabe angelegt, danach Befehl gesperrt bzw. `allow_mcp` aus, Mensch erlaubt → nicht ausgeführt (`forbidden`
    bzw. `unauthorized`);
  - ein Client kann `allow_mcp`/`allow_api_keys` nicht einschalten (`forbidden`), ausschalten mit Admin und Freigabe;
  - `create_client` mit `assistant` liefert `Response::Client` ohne Schlüssel, mit `mcp` einen Schlüssel, mit einer
    unbekannten Art `error`; zwei Clients der Art `mcp` haben getrennte Quellen; `new_key` für `assistant`/`bot` ist
    `unsupported`;
  - Rechte-Tabelle und MCP-Vertragstest grün; API-Referenz neu (`TORRO_BLESS=1 cargo test -p torro-ipc`); fmt, clippy,
    test (auch `TORRO_LANG=de`) grün; Agent `torro-api-review`.

### S.33 Ein Rechte-Editor in `torro-settings-ui`, mit Schaltern je Befehl
- [ ] erledigt
- **Modell:** Opus (großer Umzug, geteilte Logik) · **Braucht:** S.30, S.32 · **Dateien:**
  `crates/torro-settings-ui/src/rights.rs` (neu), `crates/torro-settings-ui/src/lib.rs`,
  `crates/torro-settings-ui/src/demo.rs`, `crates/torroworkplace/src/ui/agents.rs`, `crates/torroworkplace/src/ui/mod.rs`
- Die Rechte-Ansicht aus `agents.rs` zieht nach `torro_settings_ui::rights`: `Pick`, `change_rights`, `access_group`,
  `confirm_group`, `outbound_rows`, `selection_group` mit Kandidaten, die Beschriftungen (`area_label`, `admin_label`,
  `rights_summary` …), `segment_slot`/`control_column` und der Entwurf mit Autosave nach `SAVE_DELAY` (500 ms,
  `clients.update_client` mit den ganzen Rechten) samt `subscription`. Form wie die anderen Seiten der Bibliothek
  (`State`, `Message`, `update -> Action`, `view`), dazu ein `Naming { Endpoints, Tools }` für die Namen der Befehle und
  eine Struktur der Kandidaten (Kalender, Listen, Mail-Konten, Inbox-Quellen, Repositories), die Steuerzentrale und
  Assistent-Fenster füllen. `torro_settings_ui::Data` bekommt Kalender und Listen (`Response::Collections` in
  `Data::take`).
- **Neu: Schalter je Befehl.** Jede Bereichszeile mit Zugriff lässt sich aufklappen („Show commands“): eine Zeile je
  Katalogeintrag des Bereichs mit `Mcp::Tool`, den Stufe und Admin-Schalter erlauben, mit Name nach `Naming`
  (`command.name` bzw. `tool_name()`), der ersten Zeile der Beschreibung und einem Schalter, der den Namen in
  `Rights::denied` ein- oder austrägt. Die Bereichszeile nennt gesperrte Befehle („Write · 2 off“). Senkt man die
  Stufe, bleiben Einträge für Befehle, die nicht mehr erlaubt sind, in `denied` (harmlos) und erscheinen wieder, wenn
  die Stufe steigt.
- Die Datenregel ist nicht mehr Teil der Rechte (S.30). `agents.rs` nutzt bis S.35 den neuen Editor (Naming `Tools`
  für Agenten, `Endpoints` für Skripte), damit die Steuerzentrale zwischen den Aufgaben funktioniert.
- **Oberfläche:** derselbe Editor in Steuerzentrale (bis S.35 Agents & API) und später Assistent › Access
- **API:** keine neue Funktion; `clients.update_client` · Admin, Never
- **MCP:** Never (`clients.*`, api.md Punkt 2)
- **Abnahme:** die Tests der Rechte-Ansicht aus `agents.rs` laufen in `rights.rs` (Stufen, Admin, Auswahlen, Ask first,
  Send on my behalf, Autosave); neue Tests: aufgeklappter Bereich zeigt nur erlaubte Befehle, ein Schalter trägt in
  `denied` ein und aus, Namen nach `Naming`; Galerie bzw. Demo zeigt einen aufgeklappten Bereich. Sichtprüfung des
  Editors (Steuerzentrale, Detailseite eines Agenten) in allen Größen; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### S.34 Assistent und Bots: Access im Assistent-Fenster
- [ ] erledigt
- **Modell:** Opus (Rechte in einem App-Prozess) · **Braucht:** S.33 · **Dateien:**
  `crates/torro-settings-ui/src/assistant/access.rs` (neu), `crates/torro-settings-ui/src/assistant/mod.rs`,
  `crates/torro-settings-ui/src/assistant/bots.rs`, `crates/torro-settings-ui/src/assistant/general.rs`,
  `crates/torro-settings-ui/src/demo.rs`, `crates/torroassistant/src/app.rs`, `crates/torroassistant/src/launch.rs`,
  `crates/torroassistant/src/bots.rs`, `crates/torroassistant/src/view/editor.rs`, `crates/torroassistant/src/chat.rs`,
  `crates/torroassistant/src/demo.rs`
- Neuer Abschnitt **Access** (`SECTIONS`, `--settings access`, `--settings access --bot <id>`): oben der Assistent – ohne
  Client „Turn on assistant“ mit „Start from“ (`Preset` aus `agents.rs` hierher) und `clients.create_client` (Antwort
  `Response::Client`, kein Schlüssel); mit Client der Rechte-Editor (Naming `Tools`) und „Turn off assistant…“
  (`clients.remove_client`, bestätigt). Darunter eine Zeile je Bot, die den Rechte-Editor des Bot-Clients öffnet (kein
  Knopf zum Entfernen). Die Bereiche `assistant` und `bots` bleiben ausgeblendet (`api::shows_area`).
- Bots › „Access“, der Bot-Editor („What the bot may use“, `Message::BotAccess`) und „Change access“ unter einer
  abgelehnten Antwort öffnen Access im selben Fenster statt der Steuerzentrale; General verweist für an/aus auf Access.
  `chat.rs` `area_name` für `Area::Clients`: „API and MCP“.
- **Grenze im App-Prozess:** Der Test `no_section_touches_clients_rights_or_the_data_rule` gilt weiter für alle Abschnitte
  außer `assistant/access.rs`; dort sind nur `UpdateClient`, `ClientActivity` und für die Art `assistant`
  `CreateClient`/`RemoveClient` erlaubt, nie `NewClientKey` (Test). Der Test erlaubt `rights::` (den Editor aus S.33,
  der `UpdateClient` und `api::Rights` enthält) nur in `assistant/access.rs`; ein zweiter Test deckt
  `crates/torroassistant/src/` ab. Kommt doch eine Antwort mit Schlüssel, verwirft der Abschnitt sie ungelesen. Die
  Grenze ist eine Regel im Code, keine Prüfung im Kern: Der App-Schlüssel darf ohnehin alles.
- **Oberfläche:** Assistent › Einstellungen › Access; Bot-Editor; Chat („Change access“)
- **API:** keine neue Funktion; `clients.update_client`, `clients.create_client`, `clients.remove_client`,
  `clients.activity` · Admin, nur App-Schlüssel
- **MCP:** Never (api.md Punkt 2)
- **Abnahme:** Tests: Access mit und ohne Assistent-Client, Einschalten ohne Schlüssel in der Antwort, Bot-Zeile öffnet
  den Editor des Bots, Bot-Editor und „Change access“ öffnen Access, verbotene Requests in anderen Abschnitten und
  `NewClientKey` in Access (Test); Sichtprüfung `torroassistant --demo --settings access` (auch mit `--bot`) in allen
  Größen, hell und dunkel; fmt, clippy, test (auch `TORRO_LANG=de`) grün; Agent `torro-api-review`.

### S.35 Steuerzentrale: Abschnitte API und MCP, Hooks beim Agenten
- [ ] erledigt
- **Modell:** Opus · **Braucht:** S.32, S.33, S.34 · **Dateien:** `crates/torroworkplace/src/ui/api.rs` (neu),
  `crates/torroworkplace/src/ui/mcp.rs` (neu), `crates/torroworkplace/src/ui/agents.rs` (entfällt),
  `crates/torroworkplace/src/ui/agent_wizard.rs`, `crates/torroworkplace/src/ui/mod.rs`,
  `crates/torroworkplace/src/ui/launch.rs`, `crates/torroworkplace/src/client_setup.rs`,
  `crates/torroworkplace/assets/assistant/skill.md`, `crates/torro-gui/src/widgets.rs` (nur Kommentar),
  `docs/einrichtung.md`, `docs/assistent.md`, `docs/eingang.md`, `docs/erweiterungen.md`, `docs/mail.md`
- `Section::Agents` wird zu `Section::Api` und `Section::Mcp` (Seitenleiste: Overview, Accounts, API, MCP, Approvals,
  Workspaces, Appearance; General am Fuß; Symbole nach Skill `torro-design` aus dem vorhandenen Satz). `--section api`,
  `--section mcp`; `agents` bleibt als Wort für `mcp`; `assistant` und `bot:<id>` holen das Assistent-Fenster nach vorn
  (`apps.open_app`) statt einer Seite.
- **API** (`ui/api.rs`): Gruppe „API“ mit „Allow API keys“ (`SetSetting AllowApiKeys`); Gruppe „Keys“ mit den Clients
  der Art `script`; „Create key“ (Dialog: Name wie bisher geprüft, Vorlage, `CreateClient { kind: "script" }`, Key einmal
  mit „Copy“); Detailseite mit Rechte-Editor (Naming `Endpoints`), letzten Aufrufen, „Create new key“, „Revoke…“.
- **MCP** (`ui/mcp.rs`): Gruppe „MCP server“ mit „Allow MCP“ (`SetSetting AllowMcp`) und der Befehlszeile; Gruppe
  „Connected clients“ mit allen MCP-Arten; „Connect client“ öffnet `agent_wizard` ohne „Script“, mit „Other MCP client“
  (Art `mcp`, Name wie bei Skripten geprüft, Key einmal mit Befehlszeile). Detailseite mit Rechte-Editor (Naming `Tools`), Gruppe „Notification Hub“
  (nur Claude Code, Codex, Gemini CLI): Schalter „Report when waiting or done“ – an: `CreateClient { kind: hook, agent }`
  und `Job::Hook { owner }` wie im Dialog heute; aus: `Job::Disconnect` für den Hook-Client (nimmt nur seine Hooks
  heraus) und `RemoveClient`; Stand aus `client_setup.json` (eingerichtet, Codex muss vertrauen, Fehler mit Grund).
  Hook-Clients erscheinen in keiner Liste; ein Hook-Client ohne Agent (verwaist) steht als Zeile „Hooks without an
  agent“ mit „Remove“ unter MCP.
- Assistent und Bots erscheinen in keinem Abschnitt (S.34). Die Demo-Zustände (`AgentDetail`, `ScriptDetail`,
  `HookDetail`, `NewKey`, `Disconnect`, `AgentWizard*` …) heißen nach dem neuen Ort (`api-detail`, `mcp-detail`,
  `mcp-hooks`, `connect-client` …); die `Assistant*`-Zustände entfallen hier.
- Texte: kein „Agents & API“ mehr in `crates/` (außer Tests, die das Fehlen prüfen); `docs/`-Verweise außerhalb von
  `docs/tasks/` zeigen auf API, MCP bzw. Assistent › Access.
- **Oberfläche:** Steuerzentrale › API, › MCP
- **API:** keine neue Funktion; `clients.*` · Admin, Never; `settings.set_setting allow_api_keys|allow_mcp` · Admin
  (einschalten nur App-Schlüssel)
- **MCP:** `clients.*` Never; `settings_set_setting` wie in S.32
- **Abnahme:** Tests: Abschnitte und `--section`-Wörter (alte Wörter führen richtig), Listen je Art ohne Hook-, Assistent-
  und Bot-Clients, „Create key“ zeigt den Key einmal, „Connect client“ ohne Script und mit „Other MCP client“, der
  Hook-Schalter erzeugt bzw. entfernt Hook-Client und Hooks (mit gefälschtem Runner wie heute), Schalter senden
  `SetSetting`; `grep -rn "Agents & API" crates docs --include=*.rs --include=*.md` findet nur `docs/tasks/` und Tests.
  Sichtprüfung API und MCP (Liste, Detail, Hooks, Dialoge) in allen Größen, hell und dunkel (Agent
  `torro-design-review`); fmt, clippy, test (auch `TORRO_LANG=de`) grün; `scripts/ci-local.sh`; Agent `torro-api-review`.
