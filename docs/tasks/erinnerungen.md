# Aufgaben: Erinnerungen an Termine, auch auf dem ganzen Bildschirm (ER0–ER4)

Konzept: [erinnerungen.md](../erinnerungen.md). Screens: [prototypes/calendar/erinnerung.html](../prototypes/calendar/erinnerung.html).
Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`. Wer einen Request anfasst,
lädt `torro-api`; nach jeder Funktion prüft `torro-api-review`. Wer iCalendar schreibt, lädt `nextcloud`
(Agent `nextcloud-expert` geeignet). Nach jeder UI-Änderung prüft `torro-design-review`.

**Freigegeben am 2026-10-15** (Entscheidungen im Konzept): neue Felder `Event.alarms` und
`Settings.reminders`, die fünf Requests aus ER1.2; Verwerfen nur lokal, Vollbild je Kalender, während eines
Videocalls nie Vollbild (Einstellung `never_during_call`, Standard an).

**Für alle Aufgaben gilt:**
- Titel, Ort, Notizen und Teilnehmer einer Erinnerung sind `untrusted`; ins Protokoll kommen nur Kennung
  und Befehl.
- Erinnerungen von Kalendern, die ein Client nicht sehen darf, sind für ihn `not-found`, gefiltert vor dem
  Lesen.
- Tests ohne Uhr: Der Planer bekommt „jetzt“ übergeben; keine Wartezeiten in Tests.

Aufgaben, die den Katalog ändern (ER1.2), laufen nie parallel zu anderen Katalog-Aufgaben.

```
ER0.1 (Versuch, sofort möglich)
ER1.1 ── ER1.2 ── ER1.3 ─┬─ ER1.4
                         ├─ ER2.3 · ER2.4 · ER2.5 · ER2.6
ER1.5 (Token, sofort) ───┴─ ER2.1 (braucht ER1.3) ── ER2.2 (braucht ER0.1)
ER3.1 (braucht ER1.4)       ER4 später
```

## ER0 Klären

### ER0.1 Versuch: Vollbild auf allen Bildschirmen
- [x] erledigt
- **Modell:** Opus · **Dateien:** nur `docs/erinnerungen.md` (Ergebnis) und ein Wegwerf-Beispiel unter
  `crates/torro-gui/examples/fullscreen_probe.rs`, das nicht committet wird
- Herausfinden und aufschreiben, wie ein Iced-Fenster je System **randlos über dem ganzen Bildschirm, über
  allen anderen Fenstern** und **auf einem bestimmten Bildschirm** erscheint, ohne die Arbeitsfläche zu
  wechseln: Hyprland (Fensterregeln über `torro-wm`: `fullscreen`, `pin`, Monitor), macOS (Ebene über
  Vollbild-Apps und allen Spaces; reicht `iced::window::Level::AlwaysOnTop` oder braucht es
  `torro-appkit`?), Windows (randlos, oben).
- Dazu: Bekommt das Fenster den Fokus, ohne ihn einer laufenden Eingabe zu stehlen? Erscheint es über
  einem gesperrten Bildschirm (soll es nicht)?
- **API/MCP:** keine.
- **Abnahme:** Abschnitt „Vollbild je System“ im Konzept mit dem Weg je System und dem, was nicht geht;
  Rückfrage, falls `unsafe` in `torro-appkit` nötig ist.

## ER1 Kern: Erinnerungen planen und auslösen

### ER1.1 Erinnerungen im Termin lesen und schreiben
- [x] erledigt
- **Modell:** Opus (Agent `nextcloud-expert` geeignet) · **Braucht:** – · **Dateien:**
  `crates/torro-pim/src/{model,demo}.rs`, `crates/torro-pim-caldav/src/{ical,write}.rs`,
  `crates/torro-pim-caldav/tests/fixtures/` (neue Fixtures)
- `Event.alarms: Vec<Alarm>` mit `#[serde(default)]`. `Alarm { trigger, style }`:
  `trigger` = `Before { minutes, of: Start | End }` oder `At(DateTime<Utc>)`; `style` = `None` (Art des
  Kalenders), `Notification`, `FullScreen`.
- Lesen: `VALARM` mit `ACTION:DISPLAY` oder `AUDIO`; `TRIGGER` relativ (`-PT15M`, `RELATED=END`) und
  absolut (`VALUE=DATE-TIME`); `X-TORRO-STYLE`. `ACTION:EMAIL` und kaputte Auslöser werden übersprungen,
  nicht als Fehler gemeldet.
- Schreiben (`merge_ics_event`): Nur wenn `alarms` sich geändert hat, werden die `DISPLAY`/`AUDIO`-Alarme
  ersetzt; `EMAIL`-Alarme und fremde Eigenschaften in Alarmen (`UID`, `X-*`) bleiben.
- Fixtures: Nextcloud (Web), Thunderbird (`X-MOZ-LASTACK`), Apple (`X-APPLE-DEFAULT-ALARM`).
- **API/MCP:** kein neuer Request; `Event` wird reicher (Hinweis in `api.md`).
- **Abnahme:** Round-Trip-Tests an allen drei Fixtures; ein Termin ohne Alarme bleibt Byte für Byte.

### ER1.2 Gerüst: Einstellungen, Requests, Katalog
- [x] erledigt
- **Modell:** Opus · **Braucht:** ER1.1 · **Dateien:** `crates/torro-core/src/settings.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/{core,mcp}.rs` (Handler zunächst
  `unsupported`), `docs/api.md`, `docs/api-reference.md`
- `Settings.reminders` (`#[serde(default)]`): `enabled` (an), `default_lead` (1 min, `None` möglich),
  `calendars` (Kalender-Id → `Off | Notification | FullScreen`, ohne Eintrag `Notification`), `only_accepted`
  (an), `all_day` (aus), `free_events` (aus), `outside_work_hours` (an, wirkt erst mit K.33),
  `all_displays` (an), `never_during_call` (an), `sound` (`System | None`, Systemton). Schlüssel `reminders` für `settings.set_setting`.
- Requests mit Katalogeintrag (Bereich `calendar`):
  - `Request::ListReminders { due_only: bool }` · `calendar.list_reminders` · Read ·
    MCP `calendar_list_reminders` – fällige und die der nächsten 36 Stunden: Kennung, Termin, Auslösezeit,
    Art, Zustand (`upcoming | due | snoozed | dismissed`), `snoozed_until`, `join` (Dienst und Link).
  - `Request::SnoozeReminder { id, until: Snooze }` · `calendar.snooze_reminder` · Write ·
    MCP `calendar_snooze_reminder` – `Snooze::Minutes(1..=120)` oder `Snooze::UntilStart`.
  - `Request::DismissReminder { id }` · `calendar.dismiss_reminder` · Write · MCP `calendar_dismiss_reminder`.
  - `Request::PauseReminders { until: Option<DateTime<Utc>> }` · `calendar.pause_reminders` · Write ·
    MCP `calendar_pause_reminders` – `None` setzt fort.
  - `Request::PreviewReminder { href }` · `calendar.preview_reminder` · Write ·
    MCP `calendar_preview_reminder` – zeigt die Erinnerung eines Termins jetzt, als „Preview“.
- **Abnahme:** Rechte-Tabellentest und MCP-Vertragstest decken die fünf ab. (Eine `docs/api-reference.md`
  gibt es im Repository nicht; die Befehle stehen in `docs/api.md`, Fußnote 20.)
- **Nach der API-Prüfung (2026-10-15):** `set_setting reminders` ändert nur die genannten Felder und Kalender
  (`Reminders::merged`); Clients sehen und setzen nur Kalender ihrer Auswahl, `update_settings` lässt die
  Kalender-Arten stehen; `Response::Reminders` sagt `paused_until`; Schlummern 1–120 Minuten im Schema.

### ER1.3 Planer und Zustand
- [x] erledigt
- **Modell:** Opus · **Braucht:** ER1.2 · **Dateien:** `crates/torroworkplace/src/reminders.rs` (neu, Tests in
  `reminders/tests.rs`), `crates/torroworkplace/src/{core,daemon,pim,main,approvals,audit}.rs`, `Reminder.preview_by`
  in `crates/torro-ipc/src/lib.rs`, Übersetzungen in `crates/torro-core/src/i18n/de.rs`
- **Umgesetzt (2026-10-15):** „Nur zugesagte“ und „außerhalb der Arbeitszeit“ wirken erst mit K.26 bzw. K.33.
  Nie gezeigte Erinnerungen kommen höchstens 15 Minuten nach dem späteren von Auslösen und Beginn noch,
  gezeigte bleiben bis zum Ende des Termins. Der Dienst gibt fällige Erinnerungen an `reminders::Sink`;
  bis ER1.4 ist das `Silent`.
- Reine Regeln (`Planner`, „jetzt“ als Argument): aus Terminen, Einstellungen und Zustand die Erinnerungen
  der nächsten 36 Stunden; Kennung = Prüfsumme aus Kalender, UID, Vorkommen, Auslöser. Filter aus dem
  Konzept (zugesagt, ganztägig, frei, ausgeblendete Kalender, Pause); läuft ein Termin mit Videolink und
  ist `never_during_call` an, wird aus Vollbild eine Benachrichtigung.
- Lauf im Kern: neu planen bei Sync, Änderung und im Minutentakt; auslösen zur Sekunde. Nach dem Aufwachen:
  laufende Termine nachholen, vorbei ist vorbei.
- Zustand `<state>/reminders.json` (0600): verworfen, geschlummert, angezeigt; Einträge für vorbei
  Termine nach 7 Tagen weg.
- Handler der fünf Requests; `preview_reminder` höchstens einmal je Minute und Client (`busy`), mit
  Name des Clients für „sent by“.
- Verträge aus ER1.2 umsetzen: Schlummern und Verwerfen nur für `due`/`snoozed` (sonst `conflict`),
  Minuten 1–120 (sonst `error`); Pause höchstens 24 Stunden (sonst `error`) und nur für Clients mit allen
  Kalendern (`forbidden`); Kennung höchstens 36 Hex-Zeichen (das Protokoll nimmt sie dann wie sie ist).
- Vorschau: das `href` schon in `PimService::out_of_reach` über den Zwischenspeicher prüfen (`not-found` vor
  einer Freigabe); eine Vorschau eines Agenten achtet auf Pause und `never_during_call`.
- Freigaben: Regeln in `approvals::Facts` für Schlummern, Verwerfen und Vorschau (Titel und Beginn aus dem
  Zwischenspeicher über Kennung bzw. `href`) und eine lesbare Zusammenfassung für `set_setting reminders`
  (Kalendernamen statt Kennungen); Datei `crates/torroworkplace/src/approvals.rs` kommt dazu.
- **Abnahme:** Tests: Standard und eigener Alarm, Schlummern bis Beginn, verschobener Termin = neue
  Erinnerung, Pause, Aufwachen nach 10 min, Client ohne Kalenderrecht bekommt `not-found`; Protokoll-Kennungen
  der fünf Befehle (`audit::ids`).

### ER1.4 Eingang und leise Benachrichtigung
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** ER1.3 · **Dateien:** `crates/torroworkplace/src/{inbox,inbox_notify,notify,reminders}.rs`,
  `crates/torroinbox/src/notifications.rs` (Knopf „In 5 min“)
- Jede ausgelöste Erinnerung ist ein Eintrag `reminder` der Quelle „Suite“, „Für mich“, mit ✕ · Open ·
  In 5 min (`calendar.snooze_reminder`). Verworfen oder Termin vorbei → erledigt sich selbst.
- Art `Notification`: Desktop-Benachrichtigung „{title} · in {n} min“ mit Join (falls Link), In 5 min,
  Dismiss. Ruhe während belegter Termine hält sie nicht zurück.
- **Abnahme:** Tests: Eintrag kommt und geht, Knopf schickt den richtigen Request, keine doppelte
  Benachrichtigung bei Vollbild.
- **Umgesetzt (2026-10-16):** neue Art `Kind::Reminder` in `torro-inbox` (freigegeben), nur vom Kern, für mich.
  Der Eintrag hat die Knöpfe jeder Meldung der Suite (✕, Öffnen = Kalender, Später); erledigt oder vertagt im
  Eingang verwirft bzw. schlummert die Erinnerung selbst (`Reminders::follow_inbox`), im Später-Menü steht
  „In 5 min“ zuerst (`torroinbox/src/app.rs`). Die allgemeinen Popups des Eingangs lassen Erinnerungen aus
  (`asks_elsewhere`); die eigene Benachrichtigung hat Join, In 5 min, Dismiss (Linux; macOS und Windows ohne
  Knöpfe). Ein Vollbild zeigt bis ER2.1 ebenfalls diese Benachrichtigung; dass es danach keine doppelte gibt,
  prüft ER2.1. Dateien zusätzlich: `torro-inbox/src/{item,rules}.rs`, `torroinbox/src/{hub,app}.rs`,
  `torroworkplace/src/{core,daemon}.rs`, `torro-core/src/i18n/de.rs`.

### ER1.5 Schriftstufe für das Vollbild
- [x] erledigt
- **Modell:** Haiku · **Dateien:** `crates/torro-core/src/design.rs`, `crates/torro-gui/src/{typo,theme}.rs`,
  `.claude/skills/torro-design/SKILL.md` (Tabelle Typografie)
- `typo::hero()` 48/56, 600, `typo::countdown()` 28/32, 500, tabellarisch, und `size::control_xl()` 44 für die
  Knöpfe (Text `typo::body_strong()`) – nur für das Vollbild.
- **Umgesetzt (2026-10-15):** auch in `design/pack.rs` (Schlüssel `type.hero`, `type.countdown`; ein Paket ohne
  sie leitet sie ab: Titel wie `headline`, Countdown Medium, sonst wie `micro`), `design/rules.rs` (hero 32–72,
  countdown 20–48, `control_xl` ≥ 24) und `docs/design-pakete.md`.
- **API/MCP:** keine.

## ER2 Oberflächen

### ER2.1 Vollbild-Fenster
- [x] erledigt
- **Modell:** Opus · **Braucht:** ER1.3, ER1.5 · **Dateien:** `crates/torrocalendar/src/{main,app}.rs`,
  `crates/torrocalendar/src/reminder.rs` (neu), `crates/torroworkplace/src/{reminders,apps}.rs` (starten)
- `torrocalendar --reminders` (Fensterklasse `torro-reminder`), vom Kern gestartet; liest
  `calendar.list_reminders { due_only: true }` alle 2 s, schließt sich, wenn nichts mehr fällig ist.
- Aufbau nach dem Entwurf: Kopf (Kalender, Art, Uhr), Titel `hero`, Countdown, Zeit, Knöpfe Join / Snooze ▾ /
  Dismiss, Eigenschaften (Ort, Teilnehmer nach K.26, Organisator, Links, Notizen), Fuß (Next, Also now).
  Größen „Normal“, „Breit“ (zwei Spalten), „Klein“; hell und dunkel; nach Beginn „Started … ago“.
- Tasten: `Enter`, `S` mit `1`/`5`/`0`/`B`, `Esc`, `←`/`→`, `Tab`; Tasten erst nach 600 ms.
- `--demo --open reminder|reminder-started|reminder-many|reminder-notes|reminder-light` für die Sichtprüfung.
- Texte (Übersetzungen im Bericht nennen): „Starts in {t}“, „Started {n} min ago“, „Join {service}“,
  „Snooze“, „Until it starts“, „Dismiss“, „Dismiss all“, „Next: {title}“, „Also now“, „Show all“,
  „Preview · sent by {client}“, „Open in calendar“, „Open in Maps“.
- **API/MCP:** nur Requests aus ER1.2.
- **Abnahme:** Sichtprüfung 1280×800, 1920×1080, 3840×1080, hell und dunkel; Tests für Tasten und Countdown.
- **Umgesetzt (2026-10-16):** `crates/torrocalendar/src/reminder.rs` (Tests in `reminder/tests.rs`), Start in
  `main.rs`; Bausteine `widgets::big_button` und `style::container::reminder_screen` in `torro-gui`. Der Kern
  startet das Fenster für Erinnerungen im Stil „full screen“ (`reminders::Desktop`, einmal, solange es läuft) und
  zeigt dann keine Benachrichtigung; startet es nicht, kommen Benachrichtigungen. Demo:
  `--demo --open reminder|reminder-started|reminder-many|reminder-notes|reminder-light|reminder-preview|
  reminder-onsite|reminder-snooze` mit `--size 1920x1080`. Sichtprüfung auf macOS bei 1280×720, 1440×900,
  1920×1080 und 3840×1080, hell und dunkel. Teilnehmer und Organisator fehlen, bis K.26 sie liest; `Tab` geht
  durch die Knöpfe noch nicht (Iced 0.14 kennt keinen Fokus für Knöpfe). Auf macOS deckt das Fenster den Bildschirm
  bis ER2.2 nur maximiert, ohne die Menüleiste.

### ER2.2 Ein Fenster je Bildschirm
- [x] erledigt
- **Modell:** Opus · **Braucht:** ER2.1, ER0.1 · **Dateien:** `crates/torrocalendar/src/{main,reminder}.rs`,
  `crates/torro-appkit/src/lib.rs`, Wurzel-`Cargo.toml` (Features `NSApplication`, `NSScreen` bei
  `objc2-app-kit`), `packaging/macos/` (Bundle „Torro Reminder.app“), `crates/torro-wm/src/hyprland/`,
  `crates/torro-wm/src/lib.rs` (neue Funktion, freigegeben), `crates/torroworkplace/src/reminders.rs`,
  `integrations/hyprland/`, `integrations/glazewm/` (neu, Regel `ignore`)
- Weg je System nach „Vollbild je System“ im Konzept (Entscheidungen 6–8): Fenster unsichtbar öffnen, je
  Bildschirm einrichten, dann zeigen.
  - **macOS:** in `torro-appkit` Ebene 101, `CanJoinAllSpaces | FullScreenAuxiliary | Stationary |
    IgnoresCycle`, Rahmen des Bildschirms; Hauptbildschirm mit Fokus, die anderen ohne. Kein neues `unsafe`.
    Gestartet aus „Torro Reminder.app“ (`LSUIElement`); ohne Bundle (Entwicklung) nur auf dem Schreibtisch.
  - **Hyprland:** `torro-wm` meldet je Bildschirm eine benannte Regel an (Klasse `torro-reminder`, Titel
    `Reminder · <Monitor>`, `monitor`, `fullscreen`, `no_initial_focus` für die anderen), vor jedem Start;
    der Kern zieht die Fenster beim Wechsel der Arbeitsfläche nach.
  - **Windows:** nur Iced (`AlwaysOnTop`, randlos, `skip_taskbar`, `Fullscreen`); ohne GlazeWM nur der
    Hauptbildschirm, mit GlazeWM alle aus `torro-wm`.
- Hauptbildschirm volle Fassung, die anderen die ruhige (nur mit `all_displays`). Ein Klick irgendwo
  wirkt überall.
- **API/MCP:** keine neuen Requests; die neue Funktion in `torro-wm` ist intern.
- **Abnahme:** auf zwei Bildschirmen geprüft, je System, das zur Hand ist (Hyprland und Windows sind bis
  hier nur recherchiert); über einer Vollbild-App (macOS) und mit gesperrtem Bildschirm; was fehlt, im Bericht.
- **Umgesetzt (2026-10-16):** Das Fenster ist ein `iced::daemon` mit einem Fenster je Bildschirm (`--all-displays`
  aus der Einstellung, Hyprland-Monitore mit `--monitors`); die anderen zeigen die ruhige Fassung (Demo
  `reminder-second`). macOS: `torro_appkit::cover_screen` / `screen_count` (kein neues `unsafe`, der vorhandene
  Aufruf steckt jetzt in `ns_window`), Bundle „Torro Reminder.app“ über `AGENT_APP=1 make-app.sh` und den
  Release-Workflow, der Kern startet es bevorzugt (`reminders::reminder_program`). Hyprland:
  `WindowManager::cover_monitors` mit `hl.window_rule` je Monitor, `Core::keep_reminder_windows_in_view` zieht die
  Fenster beim Wechsel der Arbeitsfläche nach. GlazeWM-Regel in `integrations/glazewm/`.
  **Geprüft:** macOS mit einem Bildschirm (5120×1440): Fenster auf Ebene 101 über Menüleiste und Dock; das Bundle
  startet als Erinnerungsfenster. **Nicht geprüft:** zwei Bildschirme, über einer Vollbild-App, gesperrter
  Bildschirm, Hyprland und Windows (nur Tests der erzeugten Befehle) – hier nicht zur Hand.

### ER2.3 Termin-Editor: Erinnerungen
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** ER1.1 · **Dateien:** `crates/torrocalendar/src/editor.rs`
- Eigenschaftszeile „Reminders“: je Alarm ein Chip „15 min before · Full screen“ mit ✕, „Add reminder“
  öffnet ein Menü (At start, 1, 5, 10, 15, 30 min, 1 h, 1 day; dann Art: Calendar default, Notification,
  Full screen). Ohne eigene Alarme leise „Default: 1 min before“.
- Im „…“-Menü des Paneels: „Preview reminder“ (`calendar.preview_reminder`).
- **Abnahme:** Speichern über `calendar.save_event`, Tests für Chip-Texte.
- **Umgesetzt (2026-10-16):** Abschnitt „Reminders“ mit Zeilen je Erinnerung (✕), „Add reminder“ als Menü mit
  „When“ und „Style“ (die gewählte Art gilt für die nächste Zeit, ein Klick auf eine Zeit fügt hinzu und speichert).
  Ohne eigene leise „The default reminder of the settings“ (der Editor kennt die Minuten nicht). „Preview
  reminder“ steht im Fuß des Paneels neben „Delete event“, da das Paneel kein „…“-Menü hat; in `app.rs` geht es als
  `calendar.preview_reminder` an den Kern.

### ER2.4 Steuerzentrale: Calendar › Reminders
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** ER1.2 · **Dateien:** `crates/torroworkplace/src/ui/general.rs`
- **Ort umgeplant (2026-10-15):** Gibt es die Einstellungen im Kalender schon (S.19 in
  [steuerzentrale.md](steuerzentrale.md), Bibliothek `torro-settings-ui`), kommen die Gruppen als
  Abschnitt „Reminders“ dorthin statt nach General. Sonst wie hier, und sie ziehen später mit um.
- Gruppen nach dem Entwurf: „Reminders“ (an/aus, Default reminder, Show on, Sound, Preview),
  „Which events“ (Only accepted, All-day, Free, Outside work hours, Never full screen during a video call), „Calendars“ (je Kalender Off ·
  Notification · Full screen).
- **Abnahme:** Jede Zeile setzt nur ihr Feld (Test wie bei der Nacht).
- **Umgesetzt (2026-10-16):** drei Gruppen auf der Seite „General“ unter „Calendar“; jede Zeile schickt
  `settings.set_setting reminders` mit genau ihrem Feld (`ReminderChange::value`). „Show on“ ist ein Schalter „On all
  displays“, der Ton ein Schalter. Die Vorschau steht im Termin (ER2.3), nicht hier: Die Seite kennt keinen Termin.

### ER2.5 Tray: Nächster Termin und Pause
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** ER1.3 · **Dateien:** `crates/torroworkplace/src/tray/{menu,desktop,linux}.rs`
- „Next: {title} · in {n} min“ (öffnet den Termin), „Pause reminders ▸ For 1 hour · Until tomorrow ·
  Resume“, in der Pause „Reminders paused until {time}“.
- **Umgesetzt (2026-10-16):** „Next: {time} {title}“ mit der Uhrzeit statt „in {n} min“, weil das Menü nur bei
  Änderungen neu entsteht (eine Minutenzahl würde veralten); öffnet den Kalender (ein bestimmter Termin lässt sich
  noch nicht öffnen). Der Abschnitt steht nur da, wenn eine Erinnerung kommt oder pausiert ist. „Until tomorrow“
  pausiert bis 8 Uhr; die Zeit wird beim Klick berechnet (`menu::Pause`). Zusätzlich `Snapshot` in `core.rs` und
  `watch_reminders` in `daemon.rs`.

### ER2.6 MCP-Tools und bequeme Eingabe
- [x] erledigt
- **Modell:** Haiku · **Braucht:** ER1.3 · **Dateien:** `crates/torroworkplace/src/mcp.rs`
- Die fünf Tools aus ER1.2; `calendar_save_event` kurz mit `reminder_minutes` (eine Erinnerung, Art des
  Kalenders). Test, dass die bequeme Eingabe den richtigen Request baut.

## ER3 Zusammen mit Nextcloud

### ER3.1 Nextcloud-Erinnerungen nicht doppelt
- [x] erledigt
- **Modell:** Sonnet (Agent `nextcloud-expert` geeignet) · **Braucht:** ER1.4 · **Dateien:**
  `crates/torro-inbox/src/rules.rs`, `crates/torroworkplace/src/sources/nextcloud.rs`
- Sind Torro-Erinnerungen an, geht eine `dav`-Benachrichtigung „calendar_reminder“ im Torro-Eintrag
  desselben Termins auf (Zuordnung über Kalender und UID aus `object_id`), sonst bleibt sie wie heute.
- **Umgesetzt (2026-10-16):** Nextcloud setzt `object_type = dav` und `object_id = SHA-256(UID)`
  (`PushProvider.php`). Solche Benachrichtigungen werden kein eigener Eintrag, solange Torro an den Termin selbst
  erinnert (`sources::nextcloud::reminded_here`); sie bleiben auf dem Server (das Handy bekommt sie weiter).
  `rules.rs` blieb unverändert: Die Zuordnung braucht die Termine, die nur der Kern kennt.

## ER4 Später

- Aufgaben mit Fälligkeit und `VALARM` erinnern genauso (`torrotasks`, Bereich `tasks`).
- Reisezeit, „Leave now“; Ausnahmeregeln nach Titel; Weiterreichen über ntfy.
