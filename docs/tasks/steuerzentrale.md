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
- [ ] erledigt
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
