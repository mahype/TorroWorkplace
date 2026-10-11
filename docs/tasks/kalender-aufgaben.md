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
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-gui/src/{theme,datetime,connect,widgets}.rs`
- `parse_date`, `parse_time`; Anmeldebildschirm (`connect.rs`); Theme verfeinern; Bausteine
  (Farbpunkt, Leerzustand, Abschnittsüberschrift). Details im jeweiligen Doc-Kommentar.

### K.2 Beispieldaten
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torro-pim/src/demo.rs` (nur `sample`)
- Zwei Kalender, zwei Aufgabenlisten; Termine rund um heute, darunter Überschneidungen, ganztägige,
  mehrtägige, ein wiederkehrendes Vorkommen; Aufgaben mit Fälligkeiten (überfällig, heute, später, keine),
  Prioritäten, Unteraufgaben, erledigte.

### K.9 Kalender: Tag und Woche
- [x] erledigt
- **Modell:** Haiku, bei Problemen Sonnet · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/week.rs`
- Zeitraster auf einem Iced-`Canvas`, siehe Doc-Kommentar.

### K.10 Kalender: Monat
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/month.rs`

### K.11 Kalender: Agenda
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrocalendar/src/views/agenda.rs`

### K.12 Kalender: Termin-Editor
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0 (für die Eingaben K.1)
- **Dateien:** `crates/torrocalendar/src/editor.rs`

### K.14 Aufgaben: Liste
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0
- **Dateien:** `crates/torrotasks/src/list.rs`

### K.15 Aufgaben: Detailansicht
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.0 (für die Eingaben K.1)
- **Dateien:** `crates/torrotasks/src/detail.rs`

### K.17 Schrift Inter in allen Apps
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** K.1
- **Dateien:** `crates/torro-gui/assets/fonts/` (neu), `crates/torro-gui/src/{lib,theme,connect}.rs`,
  `crates/torrocalendar/src/main.rs`, `crates/torrotasks/src/main.rs`
- Warum: Die Apps nutzen heute die Systemschrift (`Font::DEFAULT`), unter Linux z. B. Liberation
  Sans. Ziel des Design-Skills (`.claude/skills/torro-design`): eine schöne, gut lesbare UI-Schrift,
  auf jedem System gleich.
- Inter (SIL Open Font License 1.1) aus dem offiziellen Release (github.com/rsms/inter, aktuelle
  4.x) holen: die statischen TTF-Schnitte **Regular, Medium, SemiBold** nach
  `crates/torro-gui/assets/fonts/`, dazu die Lizenz als `OFL.txt`. Keine weiteren Schnitte.
- `torro-gui` stellt bereit: `theme::FONT` (Familie „Inter“, Regular), `theme::FONT_MEDIUM`,
  `theme::FONT_SEMIBOLD` sowie `torro_gui::fonts()` mit den eingebetteten Dateien
  (`include_bytes!`). Doc-Kommentare sagen, wofür welches Gewicht da ist (Text / Hervorhebung und
  Knöpfe / Titel).
- Beide Apps laden die Schriften (`.font(…)` je Datei) und setzen `.default_font(theme::FONT)`.
- Anmeldebildschirm (`connect.rs`): Überschrift von 26/Bold auf 24/`FONT_SEMIBOLD`.
- **Abnahme:** Test, dass jede eingebettete Datei eine gültige TTF ist (beginnt mit `00 01 00 00`)
  und nicht leer; fmt, clippy, test (auch `TORRO_LANG=de`) grün; Sichtprüfung beider Apps mit
  `--demo` über den Agenten `torro-design-review` zeigt Inter. Im Bericht nennen, welche Inter-Version
  eingebunden ist.

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
- [x] erledigt
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
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** –
- Recherche und Entwurf: bei einem Release-Tag Binaries für Linux, macOS und Windows bauen,
  Installer, Homebrew-Tap, AUR. Ergebnis: `docs/release.md` und Workflow-Entwurf. Nichts pushen,
  keine Repositories anlegen.
