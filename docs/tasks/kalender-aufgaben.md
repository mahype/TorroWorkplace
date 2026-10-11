# Aufgaben: Kalender und To-dos (erste Version)

Ziel: zwei Iced-Apps, `torrocalendar` und `torrotasks`, auf dem Kern `torroworkplace`.
Die Daten kommen aus der Nextcloud des Nutzers (CalDAV, Anmeldung über Login Flow v2).
Termine und Aufgaben lassen sich anzeigen, anlegen, ändern und löschen. Der Kalender hat
die Ansichten Tag, Woche, Monat und Agenda.

Entschieden am 2026-10-11: Die Apps kommen vor dem Rest des Kerns auf allen Systemen
(Roadmap Phase 2). Gebaut wird in zwei Spuren parallel: Die Oberflächen entstehen mit
Beispieldaten (`--demo`), der Kern bekommt gleichzeitig Konto, CalDAV und Zwischenspeicher.
Danach werden beide Spuren verbunden.

Regeln für die Arbeit: [CLAUDE.md](../../CLAUDE.md). Arbeitsweise wie in [phase-1.md](phase-1.md).

## Aufbau

```
torrocalendar / torrotasks (Iced)        torro-gui: Theme, Datenquelle, Anmeldebildschirm
            │  PimSource                            │
            ├── Demo: torro_pim::demo::DemoSource   (--demo, Beispieldaten im Speicher)
            └── Kern: torro_ipc::pim::PimClient ── Socket ── torroworkplace daemon
                                                            ├── account.rs (Login Flow v2, Schlüsselbund)
                                                            └── pim.rs (Zwischenspeicher, Sync) ── torro-pim-caldav
```

- Die Apps kennen nur `torro_pim::PimSource`. Mit `--demo` laufen sie ohne Kern.
- Der Kern hält alle Sammlungen, alle Aufgaben und die Termine von 3 Monaten zurück bis
  12 Monate voraus im Speicher. Er holt sie beim Start, alle 5 Minuten und auf Anforderung.
  Lesen geht nie ins Netz. Schreiben geht zuerst an den Server, ETag-geschützt.
- Wiederkehrende Termine erscheinen als einzelne Vorkommen und sind vorerst nur lesbar.

## Aufgaben

### K.0 Gerüst
- [x] erledigt
- **Modell:** Opus
- Modell mit Serde, `href`/`etag`; `PimSource`; `DemoSource`-Mechanik; IPC-Befehle; `PimClient`-Gerüst;
  `torro-gui` (Quelle, Theme, Datumsfunktionen); beide Apps mit Zustand, Navigation, Laden
  und Speichern; Gerüste für Ansichten, Editor, Liste und Detail; Kern-Gerüste `pim.rs` und `account.rs`;
  CalDAV-Code aus dem pausierten Prototyp (Login Flow v2, Lesen von Terminen).

### Spur Oberflächen (Beispieldaten)

### K.1 torro-gui: Theme, Eingaben, Anmeldebildschirm
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-gui/src/{theme,datetime,connect,widgets}.rs`
- `parse_date`, `parse_time`; Anmeldebildschirm (`connect.rs`); Theme verfeinern; Bausteine
  (Farbpunkt, Leerzustand, Abschnittsüberschrift). Details im jeweiligen Doc-Kommentar.

### K.2 Beispieldaten
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-pim/src/demo.rs` (nur `sample`)
- Zwei Kalender, zwei Aufgabenlisten; Termine rund um heute, darunter Überschneidungen, ganztägige,
  mehrtägige, ein wiederkehrendes Vorkommen; Aufgaben mit Fälligkeiten (überfällig, heute, später, keine),
  Prioritäten, Unteraufgaben, erledigte.

### K.9 Kalender: Tag und Woche
- [ ] erledigt
- **Modell:** Haiku, bei Problemen Sonnet · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/week.rs`
- Zeitraster auf einem Iced-`Canvas`, siehe Doc-Kommentar.

### K.10 Kalender: Monat
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/month.rs`

### K.11 Kalender: Agenda
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/agenda.rs`

### K.12 Kalender: Termin-Editor
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0 (für die Eingaben K.1)
- **Dateien:** `crates/torrocalendar/src/editor.rs`

### K.14 Aufgaben: Liste
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrotasks/src/list.rs`

### K.15 Aufgaben: Detailansicht
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.0 (für die Eingaben K.1)
- **Dateien:** `crates/torrotasks/src/detail.rs`

### Spur Kern (Nextcloud)

### K.3 CalDAV lesen: Aufgaben, href und ETag
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-pim-caldav/src/{client,ical}.rs`
- `events` liefert `href` und `etag` mit; `todos(collection)` mit VTODO-Lesen (DUE, PRIORITY,
  STATUS/COMPLETED, RELATED-TO, DESCRIPTION).

### K.4 CalDAV schreiben
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-pim-caldav/src/write.rs`

### K.6 Konto im Kern
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torroworkplace/src/account.rs`

### K.7 PimClient über den Socket
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-ipc/src/pim.rs`

### K.5 Zwischenspeicher und Sync im Kern
- [ ] erledigt
- **Modell:** Opus · **Braucht:** K.3, K.4, K.6
- **Dateien:** `crates/torroworkplace/src/pim.rs`, `crates/torroworkplace/src/daemon.rs`
- Konto laden, Anmeldung im Hintergrund, Zwischenspeicher, Sync-Schleife, alle PIM-Befehle beantworten.

### Zusammenführen

### K.13 Verdrahten und abnehmen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** alle
- Apps starten den Kern, wenn er nicht läuft; Tray-Menü „Kalender“ und „Aufgaben“ öffnen die Apps
  und zeigen den Kontostatus; `.desktop`-Starter mit App-ID; `#![allow(dead_code)]` in den Apps
  entfernen; Test mit der Nextcloud des Nutzers.

### K.16 Release-Pipeline für alle Systeme
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** –
- Recherche und Entwurf: bei einem Release-Tag Binaries für Linux, macOS und Windows bauen,
  Installer, Homebrew-Tap, AUR. Ergebnis: `docs/release.md` und Workflow-Entwurf. Nichts pushen,
  keine Repositories anlegen.
