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
- **Ersetzt durch F.1** in [design.md](design.md) (Inter plus Schriftstufen). Nicht mehr einzeln bearbeiten.
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

### K.18 Größenklassen und kleinere Mindestgröße
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.1
- **Dateien:** `crates/torro-gui/src/layout.rs` (neu), `crates/torro-gui/src/lib.rs`
- Warum: Die Apps laufen dauerhaft als Kacheln im Tiling-Manager, von der Mini-Kachel bis zum
  halben 32:9-Monitor. Jede Größe bekommt ein eigenes Layout (Design-Skill
  `.claude/skills/torro-design`, Abschnitt 6). Damit nicht jede Ansicht eigene Schwellen erfindet,
  bestimmt `torro-gui` die Größenklasse an einer Stelle. Die heutige Mindestgröße 360×320
  verhindert Mini und Leiste.
- `layout.rs` mit Doc-Kommentar oben und genau dieser Schnittstelle:
  - `pub enum SizeClass { Mini, Strip, Narrow, Medium, Wide }` (`Debug, Clone, Copy, PartialEq, Eq`),
    jede Variante mit Doc-Kommentar, wofür sie gedacht ist (Mini: auf einen Blick, ohne Bedienung;
    Strip: flacher, breiter Streifen; Narrow: eine Spalte, volle Bedienung; Medium: Hauptansicht;
    Wide: Hauptansicht mit Kontext daneben).
  - `pub fn of(size: iced::Size) -> SizeClass`, Regeln in dieser Reihenfolge (logische Pixel):
    1. Höhe < 280 und Breite ≥ 600 → `Strip`
    2. Breite < 360 oder Höhe < 280 → `Mini`
    3. Breite < 600 → `Narrow`
    4. Breite < 1024 → `Medium`
    5. sonst `Wide`
  - Die Schwellen als `pub const` (`MINI_MAX_WIDTH`, `STRIP_MAX_HEIGHT`, `NARROW_MAX_WIDTH`,
    `MEDIUM_MAX_WIDTH`), damit Ansichten und Tests dieselben Zahlen benutzen.
- `lib.rs`: `pub mod layout;` mit einer Zeile in der Modulübersicht; `min_size` in `window()` auf
  **200×120**. Die Standardgröße 1100×760 bleibt.
- Die Ansichten selbst ändern sich hier nicht. Bis zu den Mini- und Leisten-Ansichten der Apps
  (eigene Aufgaben) darf es in sehr kleinen Fenstern eng aussehen.
- **Abnahme:** Tests für die Beispielgrößen aus dem Design-Skill (240×160 Mini, 320×480 Mini,
  1200×200 Strip, 480×900 Narrow, 800×600 Medium, 1600×1000 Wide) und für jede Grenze
  (359/360 und 599/600 Breite, 279/280 Höhe bei 599 und 600 Breite, 1023/1024 Breite); fmt,
  clippy, test (auch `TORRO_LANG=de`) grün.

### K.19 Kalender in jeder Größe: Mini, Leiste, Schmal
- **Nach C.1** aus [design.md](design.md): Bausteine und Tokens von dort verwenden (Skill `torro-design`).
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.18 (K.17 hilfreich, nicht nötig)
- **Dateien:** `crates/torrocalendar/src/app.rs`, `crates/torrocalendar/src/views/{mod,glance}.rs`
  (`glance.rs` neu)
- Warum: Der Kalender läuft dauerhaft als Kachel, oft sehr klein. Regeln und Größenklassen:
  Design-Skill `.claude/skills/torro-design`, Abschnitt 6. Opus, weil hier das Muster entsteht,
  dem die Aufgaben-App (K.20) folgt.
- `App::view` bestimmt die Klasse mit `iced::widget::responsive` und `torro_gui::layout::of` und
  wählt das Layout. Anmelde- und Ladebildschirm bleiben in allen Klassen wie sie sind.
- **Mini** (`glance::mini`): keine Werkzeugleiste, keine Seitenleiste. Oben das Datum
  („Sun, 11 Oct“, übersetzt). Darunter der laufende Termin mit „ends in 19 min“ bzw. der nächste
  mit Uhrzeit und „in 1 h 20 min“ (Farbpunkt, Titel, Ort falls Platz), dann so viele weitere
  Termine von heute, wie in die Höhe passen. Ist heute nichts mehr: „Nothing else today“ und der
  nächste Termin danach. Leer und ruhig, aus der Distanz lesbar.
- **Leiste** (`glance::strip`): heute als waagerechter Zeitstrahl auf einem `Canvas` – Termine als
  abgerundete Blöcke mit Titel (gekürzt mit „…“), ganztägige als Chips links, die Jetzt-Linie in
  `theme::NOW`. Der gezeigte Zeitraum umfasst die Termine von heute, mindestens 08:00–18:00.
- Mini und Leiste sind **Anzeigen**: keine Formulare, keine Dialoge; Vergrößern übernimmt der
  Tiling-Manager. Neu gezeichnet wird nur im vorhandenen `Tick` und bei neuen Daten.
- **Schmal:** Seitenleiste ausgeblendet, statt ihrer ein Knopf in der Werkzeugleiste, der sie als
  Overlay zeigt; Woche und Monat werden als Tag bzw. Agenda dargestellt (die gewählte Ansicht
  bleibt gespeichert und kommt zurück, wenn das Fenster wieder breiter ist). Der Editor öffnet
  bildschirmfüllend statt als 460 breites Modal.
- **Mittel / Breit:** wie heute.
- Zustand (Ansicht, Datum, Auswahl, offener Editor) bleibt beim Größenwechsel erhalten.
- Neue Texte englisch mit `tr`/`trf`, im Bericht mit deutscher Übersetzung.
- **Abnahme:** Unit-Tests für die reine Logik von `glance` (welcher Termin läuft/kommt als
  nächstes, „ends in“/„in“-Texte, Zeitraum der Leiste, Auswahl bei „nichts mehr heute“) mit
  festen Zeiten; fmt, clippy, test (auch `TORRO_LANG=de`) grün; `torro-design-review` mit allen
  sechs Größen – alle vier Ziele mindestens „erfüllt“ für Mini, Leiste und Schmal.

### K.20 Aufgaben in jeder Größe: Mini, Leiste, Schmal
- **Nach T.1** aus [design.md](design.md): Bausteine und Tokens von dort verwenden (Skill `torro-design`).
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.19 (als Muster)
- **Dateien:** `crates/torrotasks/src/app.rs`, `crates/torrotasks/src/glance.rs` (neu),
  `crates/torrotasks/src/main.rs` (nur `mod glance;`)
- Warum und Regeln wie K.19; Aufbau (Klassenwahl in `App::view`, `glance::mini`/`glance::strip`)
  genau wie dort.
- **Mini:** oben „3 overdue · 5 today“ (nur Teile, die nicht 0 sind; Überfällige in
  Attention-Orange, nicht Rot). Darunter die nächsten offenen Aufgaben (überfällig, dann heute,
  dann nach Fälligkeit), so viele wie passen: Abhak-Kreis, Titel, Fälligkeit. **Abhaken ist die
  einzige Bedienung** in Mini – eine Aufgabe ohne Dialog erledigen ist der Hauptzweck der Kachel.
  Nichts offen: ruhiger Leerzustand („All done for today“).
- **Leiste:** dieselben Aufgaben als Chips nebeneinander (Abhak-Kreis + Titel, gekürzt mit „…“),
  links die Zahlen wie in Mini; was nicht passt, als „+4“ am Ende.
- **Schmal:** Seitenleiste ausgeblendet, Filter (Today, Planned, All, Listen) als Auswahlliste im
  Kopf; die Detailansicht ersetzt die Liste (mit „Back“) statt 340 breit daneben zu stehen.
- **Mittel / Breit:** wie heute.
- Zustand (Filter, Auswahl, offenes Detail, Schnelleingabe) bleibt beim Größenwechsel erhalten.
- **Abnahme:** Unit-Tests für die Logik von `glance` (Reihenfolge, Zählung, „+N“ bei Platzmangel)
  mit festem Datum; fmt, clippy, test (auch `TORRO_LANG=de`) grün; `torro-design-review` mit allen
  sechs Größen – alle vier Ziele mindestens „erfüllt“ für Mini, Leiste und Schmal.

### Neue Ansichten: Zeitstrahl und Radar (2026-10-11)

Konzept und Prototypen: [calendar-views.md](../calendar-views.md), Abschnitt „Vorschlag 2026-10-11“;
`docs/prototypes/calendar/zeitstrahl.html` und `radar.html` sind die Vorlage (Maße, Verhalten).
Beide sind **reine Ansichten**: Sie lesen Termine über den vorhandenen Request `Events` und legen
über `SaveEvent` an (Platzhalter → Editor wie im Wochenraster). Kein neuer Request, kein
Katalogeintrag, kein MCP-Tool – was sie zeigen, liefert Agenten schon `list_events`; Ansicht,
Scrollposition, aufgeklappte Nächte und Vorausschau sind Fensterzustand wie die Scrollposition.
K.21 und K.22 laufen parallel: in `app.rs`, `views/mod.rs` und `icon.rs` nur die genannten
Stellen anfassen; deutsche Übersetzungen nur im Bericht nennen (die Koordination trägt sie ein).

### K.21 Kalender: Zeitstrahl (Timeline)
- [x] erledigt
- **Modell:** Opus · **Dateien:** `crates/torrocalendar/src/views/timeline.rs` (neu, bei Bedarf
  `views/timeline/layout.rs` wie bei der Woche), `crates/torrocalendar/src/views/mod.rs` (nur
  `pub mod timeline;`), `crates/torrocalendar/src/app.rs` (`CalView::Timeline`, Taste `T`,
  `Open::Timeline`/`--open timeline`, `range`, Navigation, Titel, Anfangs-Scrollposition, Zustand
  der Nächte), `crates/torro-gui/src/theme.rs` (Token `night`), `crates/torro-gui/src/icon.rs` +
  `assets/icons/moon.svg` (Lucide, ISC, wie die anderen)
- **Oberfläche:** neue Ansicht „Timeline“ (Icon `ChartGantt`, Taste `T`) in Seitenleiste,
  Segment und Ansichtsmenü, nach „Agenda“. Waagerechter Filmstreifen auf einem `Canvas` in einem
  waagerechten `scrollable` (Mausrad scrollt waagerecht):
  - Zeitraum `anchor − 1 Tag` bis `anchor + 7 Tage`; ‹ › verschieben um 7 Tage; „Today“ und der
    Start scrollen so, dass „jetzt“ bei 30 % der Breite steht.
  - Stundenbreite 56; oben Tageskopf 28 (Wochentag + Zahl, heute im roten Kreis, klebt links,
    solange der Tag sichtbar ist), Stundenachse 24 (`MICRO`, `TEXT_3`, nicht neben der Jetzt-Zeit),
    Ganztägiges 28 (mehrtägig als durchgehender Chip, endet am Beginn der Nacht), dann die Termine,
    unten 28 für freie Zeit.
  - **Nacht 22–06 eingeklappt** (Konstanten) als Band 48 breit in `night` (dunkel `#121626`, hell
    `#E3E7F1`) mit gezackter Ober- und Unterkante (Zacken ~8 breit, 5 hoch), Mond-Icon und
    „Night · 8 h“ (gedreht, von unten lesbar); Termine in der Nacht als Punkte in Kalenderfarbe im
    Band plus „· 1 event“ / „· 2 events“. Klick auf das Band klappt diese Nacht auf (Stunden normal
    breit, Grund `night` 60 % über `PANEL`), oben ein Knopf „Collapse night“. Kopfleiste: Knopf
    „Nights“ (Mond) klappt alle auf bzw. zu. Mitternacht als Linie in `LINE_STRONG`.
  - **Termine** als volle Säulen über die Terminhöhe, Tönung + 3-px-Balken links wie im Raster,
    Titel (12/500) und Zeit (`MICRO`, `TEXT_2`) um −90° gedreht, von unten nach oben, gekürzt mit „…“;
    unter 40 breit nur der Titel; Mindestbreite 16. Überschneidungen teilen sich die **Höhe** in
    Bahnen. Unbestätigt gestrichelt, vergangen 55 %, geöffneter Termin mit Fokusring.
    Klick → `OpenEvent`; Klick in freie Zeit → `NewEvent` (auf 15 min gerundet); in der
    eingeklappten Nacht klappt der Klick nur auf.
  - Freie Zeit ab 2 h zwischen Terminen (nur außerhalb der Nacht) als „3 h free“ unten in `TEXT_3`.
  - Jetzt: rote Linie 1 px mit Uhrzeit-Chip in der Achse (wie im Raster).
  - Gedrehter Text: `Frame::rotate` zeichnet Text in Iced als Pfade. Per Screenshot prüfen, ob 12 px
    scharf genug ist; wenn nicht, Titel waagerecht oben in der Säule und das im Bericht sagen.
  - Größen: Breit/Mittel wie beschrieben. **Schmal zeigt die Tagesansicht** (`shown_in`); der
    senkrechte Zeitstrahl aus dem Prototyp kommt später. Mini/Leiste unverändert.
- **API / MCP:** keiner (siehe oben).
- **Abnahme:** Unit-Tests für die Geometrie (Zeit ↔ x mit eingeklappter und aufgeklappter Nacht,
  Bahnen, freie Zeit, Treffer: Termin, Nacht, freie Fläche) mit festen Zeiten; `--demo --open
  timeline --now 2026-10-13T14:20` funktioniert; fmt, clippy, test (auch `TORRO_LANG=de`) grün;
  Screenshots Breit und Mittel (hell und dunkel) ansehen und im Bericht bewerten.
- **Umgesetzt (abweichend):** Taste **Shift+T** statt `T` (`T` ist „Today“, in Schmal der einzige
  Weg zurück). Unbestätigte Termine nicht gestrichelt – `torro_pim::Event` kennt keinen Status
  (wie die Woche). `views/week.rs` nur in der Sichtbarkeit geöffnet (`pub(super)`), damit der
  Zeitstrahl die Helfer der Woche nutzt. `Icon::tinted` für das Mond-Icon im `Canvas`. Iced 0.14
  zeichnet Formen in `Frame::with_clip` nach waagerechtem Scrollen falsch; der Zeitstrahl zeichnet
  deshalb ohne Clip und kürzt Text selbst.
- **Entschieden 2026-10-11:** Säulen höchstens 8 × `HOUR` = 384 hoch, „3 h free“ direkt darunter.
- **Offen (Design-Prüfung):** In Schmal zeigt das Menü „Day“ statt „Timeline“. Termine im
  `Canvas` nicht per Tastatur erreichbar (gleiche Lücke wie im Wochenraster).

### K.22 Kalender: Radar
- [x] erledigt
- **Modell:** Opus · **Dateien:** `crates/torrocalendar/src/views/radar.rs` (neu, bei Bedarf
  `views/radar/layout.rs`), `crates/torrocalendar/src/views/mod.rs` (nur `pub mod radar;`),
  `crates/torrocalendar/src/app.rs` (`CalView::Radar`, Taste `R`, `Open::Radar`/`--open radar`,
  `range`, Navigation, Titel, Zustand „Vorausschau“), `crates/torro-gui/src/icon.rs` +
  `assets/icons/radar.svg` (Lucide, ISC)
- **Oberfläche:** neue Ansicht „Radar“ (Icon `Radar`, Taste `R`) nach „Timeline“. Zifferblatt auf
  einem `Canvas`, Vorlage `radar.html` im Modus „Ausblenden“, 3/4:
  - **12-Stunden-Zifferblatt**, 12 oben, im Uhrzeigersinn. Ziffern zeigen die echte Stunde im
    Fenster (`15` an der 3); an 3/6/9/12 13/500 in `TEXT`, sonst 11/400 in `TEXT_2`; Ticks je
    halbe Stunde. Keine Ziffer neben der Jetzt-Zeit.
  - **Fenster** = `jetzt + Versatz` bis 12 h später. Die Linie am Fensterbeginn (rot 1.5 px,
    Uhrzeit-Chip rot außen) ist eine klare Kante. **Die ersten 9 h voll sichtbar**, die letzten 3 h
    **blenden aus** bis 0 kurz vor der Kante (Deckkraft `1 − smoothstep(f)^0.8`, `f` 0 → 1 über das
    Viertel): Ringe, Termine, Ticks und Ziffern dort in kleinen Stücken (≈ 2°) mit der Deckkraft
    ihrer Mitte zeichnen – Iced hat keine Masken.
  - **Ringe je Kalender** (nur Kalender mit Terminen mit Uhrzeit im Fenster), Reihenfolge wie in der
    Seitenleiste, der erste außen; gleich breit (höchstens 64), 4 Abstand; Spur `TEXT` 7 %.
    Überschneidungen teilen einen Ring in Bahnen. Termine als Ringsegmente mit Tönung, 3-px-Balken
    am Beginn, unbestätigt gestrichelt, geöffneter mit Fokusring.
  - **Titel entlang des Bogens** (Glyphe für Glyphe gedreht, untere Hälfte gespiegelt, damit er
    aufrecht steht), gekürzt mit „…“, nur wenn die Bahn ≥ 22 breit ist; zweite Zeile mit der Zeit ab
    40. Ist der Text per Screenshot zu unscharf: Titel weglassen (die Liste reicht) und berichten.
  - **Mitte:** Datum (`META`, `TEXT_3`), Uhrzeit (`DISPLAY`, tabellarisch), laufender Termin mit
    Farbpunkt und „40 min left“ bzw. „Free for 1 h 20 min“, „Next 15:00 · Titel“, Ganztägiges als
    Chips; beim Vorausschauen ein Knopf „Back to now“, die Linie dann `TEXT_2` statt rot.
  - **Vorausschauen:** Mausrad über dem Zifferblatt verschiebt den Versatz in 15-min-Schritten
    (−12 h … +24 h); ‹ › in der Kopfleiste um 1 h; „Today“ setzt auf 0.
  - **Liste daneben** (Breit 300, Mittel 260) bzw. **darunter** (Schmal): „Next 9 hours“ und
    „Fading out, until 02:20“ mit Zeit, Farbpunkt, Titel; Klick öffnet den Termin.
  - Klick auf ein Segment → `OpenEvent`. Mini/Leiste unverändert.
- **API / MCP:** keiner (siehe oben).
- **Abnahme:** Unit-Tests für die Geometrie (Zeit ↔ Winkel mit Versatz, Fenster und Deckkraft am
  Beginn/in der Mitte/am Ende des Viertels, Ziffernwerte, Ringe nur für Kalender mit Terminen,
  Bahnen, Treffer Winkel/Radius → Termin) mit festen Zeiten; `--demo --open radar --now
  2026-10-13T14:20` funktioniert; fmt, clippy, test (auch `TORRO_LANG=de`) grün; Screenshots
  Breit, Mittel und Schmal (hell und dunkel) ansehen und im Bericht bewerten.
- **Umgesetzt (abweichend):** Klick auf freie Fläche in einem Ring legt einen Termin an
  (`NewEvent`, auf 15 min gerundet), wie im Wochenraster. Unbestätigte Termine nicht gestrichelt
  (kein Status in `torro_pim::Event`). „Back to now“ ohne Icon. Die Termin-Tönung liegt über der
  Spur statt über `PANEL` (sonst wirkten dunkle Kalenderfarben schwächer als freie Zeit); das
  Ausblenden läuft in 1°-Stücken (2° zeigte in hell Streifen).
- **Entschieden 2026-10-11:** Zifferblatt höchstens 720 breit (`MAX_DIAMETER`).
- **Offen (Design-Prüfung):** Maße
  `LIST_MEDIUM` 260 und `NARROW_DIAL` 440 sind lokale Konstanten, gehören als Tokens nach
  `torro-gui`. In Schmal liegen Liste und Kopftitel nicht auf einer Achse.

### K.23 Radar: Tage blättern, Uhrzeit am Zifferblatt drehen
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.22 · **Dateien:** `crates/torrocalendar/src/app.rs`,
  `crates/torrocalendar/src/views/radar.rs`, `crates/torrocalendar/src/views/radar/layout.rs`
- Wunsch des Nutzers (2026-10-11): Mit ‹ › kam man nur stundenweise voran und höchstens bis morgen;
  zu einem fernen Datum ging es gar nicht. Die Kopfleiste soll wie überall Tage blättern, die
  Uhrzeit wird am Zifferblatt gesteuert.
- **Oberfläche:**
  - **Tage:** ‹ › und ← → blättern einen Tag, „Today“ führt zu jetzt. Ein Klick auf einen Tag im
    kleinen Monat (bzw. in der Wochenleiste in Schmal) öffnet das Radar an diesem Tag und bleibt
    in der Radar-Ansicht.
  - **Fensterbeginn:** heute (ohne Drehen) bei „jetzt“, rote Kante. An jedem anderen Tag um
    **08:00**, bei einem früheren Termin mit Uhrzeit an diesem Tag zur vollen Stunde davor, nie vor
    06:00 (Ende der Nacht); ganztägige Termine und solche vom Vortag zählen nicht. Kante dann
    `TEXT_2`, in der Mitte das Datum und „Back to now“.
  - **Uhrzeit drehen** (Versatz zum Fensterbeginn, weiter −12 h … +24 h): Mausrad wie bisher in
    15-min-Schritten; **↑ ↓** je eine Stunde; **Ziehen**: Drücken auf dem Zifferblatt (außerhalb
    eines Termins) und Bewegen dreht wie ein Drehknopf – im Uhrzeigersinn später, 360° = 12 h, aus
    dem Winkelunterschied zwischen zwei Mausbewegungen (über 0°/360° hinweg stetig), auf 5 min
    gerastet. Erst ab ~4 px Bewegung ist es ein Ziehen; ein Klick bleibt ein Klick (Termin öffnen,
    neuer Termin). Zeiger `Grab` über dem Zifferblatt, `Grabbing` beim Ziehen, `Pointer` über Terminen.
  - Titel der Kopfleiste: der Tag des Fensterbeginns.
- **API / MCP:** keiner – Ansicht und Versatz sind Fensterzustand (wie K.22).
- **Abnahme:** Unit-Tests: Fensterbeginn (heute, anderer Tag ohne Termine, früher Termin 06:30 →
  06:00, Termin 05:00 → 06:00, Termin 09:00 → 08:00, ganztägig ignoriert), ‹ › und ← → um einen Tag
  mit Versatz 0, Klick im kleinen Monat bleibt im Radar, ↑ ↓, Ziehen (Winkel → Minuten, über 0°
  hinweg, Raster, Schwelle Klick/Ziehen), Kantenfarbe; fmt, clippy, test (auch `TORRO_LANG=de`)
  grün; Sichtprüfung heute und an einem anderen Tag, hell und dunkel.
- **Umgesetzt (abweichend):** Schmal zeigt das Radar jetzt unter der Wochenleiste (vorher ohne).
  ‹ › blättern vom gewählten Tag aus; ist das Zifferblatt über Mitternacht gedreht, nennt der
  Titel schon den Folgetag. `Message::LookAhead` heißt jetzt `Message::Turn`. Termine öffnen
  beim Drücken, ein Klick in freie Zeit legt beim Loslassen an. Greifen geht vom halben Radius bis
  zum Rand des Zifferblatts. Ziehen nur per Unit-Test geprüft.

### K.24 Kalender: Agenda neu gestalten
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.23 (beide ändern `app.rs`) · **Dateien:**
  `crates/torrocalendar/src/views/agenda.rs`, `crates/torrocalendar/src/app.rs` (nur Eigenschaften-
  Spalte der Agenda in Breit)
- Vorlage: `docs/prototypes/calendar/agenda.html` (vom Nutzer freigegeben 2026-10-11).
- **Oberfläche:**
  - **Datumsspalte** links (56, Schmal 44) statt grauer Tagesbalken: Wochentag `CAPTION` in
    `TEXT_3`, darunter die Tageszahl `HEADLINE` tabellarisch; heute im roten Kreis 32;
    Wochenende Zahl in `TEXT_2`. Die Spalte klebt oben, solange der Tag sichtbar ist (falls Iced
    das im `scrollable` nicht hergibt: oben am Tag stehen lassen und das berichten). Neben ihr eine
    leise Kopfzeile „Today“/„Tomorrow“ links, „4 events“ rechts (`META`, `TEXT_3`). Tage durch
    `LINE` getrennt.
  - **Zeile** (≥ 44): Beginn `BODY` 500 tabellarisch, darunter die Dauer `META` `TEXT_3`
    („15 min“, „2 h“, „1 h 30 min“); 3-px-Balken in Kalenderfarbe über die Zeilenhöhe; Titel
    `BODY_STRONG`, darunter Ort `META` `TEXT_2` mit Icon (`MapPin`, bei Links/„Video“ `Video`).
    Hover `HOVER`, geöffneter Termin `SELECTED`, vergangen 55 %.
  - **Jetzt:** der laufende Termin in seiner Tönung hinterlegt, rechts Pille „40 min left“; danach
    die Jetzt-Linie (rot 1 px, Chip mit Uhrzeit links).
  - **Freie Zeit** ab 2 h zwischen zwei Terminen eines Tages: Zeile 28 „3 h free“ `META` `TEXT_3`
    mit gepunkteter Linie.
  - **Ganztägig** als Chips oben im Tag (mehrtägig mit „day 2 of 3“) statt als Zeile „all day“.
  - **Leere Tage** zwischen Tagen mit Terminen zu einer Zeile zusammengefasst:
    „3 d · Nothing planned · Tue 20 – Thu 22“. **Kalenderwoche** als leiser Trenner
    („This week · Week 42“, „Week 43“) mit Linie.
  - Breit: Inhalt höchstens `CONTENT_MAX`, rechts die Eigenschaften des geöffneten Termins wie in
    der Woche (wenn es den Baustein gibt, sonst berichten). Mittel/Schmal: ohne.
  - Ohne Daten bleibt weg: Teilnehmer-Avatare und „tentative“ (kommt mit K.26).
- **API / MCP:** keiner (reine Ansicht über `Events`).
- **Abnahme:** Unit-Tests der Logik (Gruppierung, Dauertexte, freie Zeit, leere Tage
  zusammenfassen, Wochenwechsel, laufender Termin und Jetzt-Position, mehrtägig) mit festen
  Zeiten; fmt, clippy, test (auch `TORRO_LANG=de`) grün; Sichtprüfung alle Größen hell/dunkel.

### K.25 Radar: auch ohne Termine lebendig
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.23 · **Dateien:** `crates/torrocalendar/src/views/radar.rs`,
  `crates/torrocalendar/src/views/radar/layout.rs`
- Wunsch des Nutzers (2026-10-11): Ohne Termine sieht das Radar aus wie eine leere Uhr.
- **Oberfläche:**
  - **Ringe für alle sichtbaren Kalender**, auch ohne Termine im Fenster (Reihenfolge wie in der
    Seitenleiste), damit die Bahnen immer zu sehen sind; feine Stundenlinien quer über die Ringe
    (`LINE`), wie die Stundenlinien im Raster.
  - **Nacht** 22–06 (dieselben Zeiten wie im Zeitstrahl) als Sektor in `night` hinter den Ringen,
    mit kleinem Mond-Icon am Rand der Nacht; blendet im letzten Viertel mit aus.
  - **Freie Zeit** ab 2 h (außerhalb der Nacht) als „3 h free“ in `TEXT_3` entlang des Bogens
    innen an den Ringen.
  - Leer-Zustand der Mitte: „Nothing planned“ / „Free until tomorrow 09:30“ statt nur Uhrzeit.
  - Gerne weitere ruhige Ideen aus dem Prototyp-Gefühl, aber nichts, was blinkt oder sich ohne
    Anlass bewegt; im Bericht nennen.
- **API / MCP:** keiner.
- **Abnahme:** Tests (Ringe ohne Termine, Nachtsektor im Fenster inkl. über Mitternacht, freie Zeit
  ohne Nacht), fmt/clippy/test grün, Sichtprüfung leerer Tag und voller Tag, hell/dunkel.

### K.26 Teilnehmer und Status von Terminen – und nichts mehr verlieren beim Speichern
- [ ] erledigt
- **Modell:** Opus (Agent `nextcloud-expert` geeignet) · **Dateien:** `crates/torro-pim/src/model.rs`,
  `crates/torro-pim/src/demo.rs`, `crates/torro-pim-caldav/src/{ical,write}.rs`, Anzeige in
  `crates/torrocalendar/src/views/{agenda,radar}.rs` und im Termin-Paneel (nennen)
- **Fehler, der zuerst behoben wird:** `write.rs` baut beim Speichern ein komplett neues
  `VCALENDAR` aus den Feldern von `Event`. Alles andere am Termin geht dabei verloren:
  `ATTENDEE`, `ORGANIZER`, `VALARM` (Erinnerungen), `CATEGORIES`, `URL`, `STATUS` usw. Bei einer
  Einladung, die der Nutzer selbst organisiert, kann der Server (Nextcloud/Sabre-Scheduling) dann
  sogar Absagen an die Teilnehmer schicken. Lösung: beim Lesen das Original-`VEVENT` behalten und
  beim Speichern nur die geänderten Eigenschaften ersetzen (Round-Trip), mit Tests an echten
  Nextcloud-Fixtures.
  **Erledigt mit P.1** ([github.md](github.md), Commit „keep foreign properties when writing
  items“): `CalDavClient::save_*` holt das gespeicherte Objekt und ersetzt nur geänderte Felder
  (`write::merge_ics_event`/`merge_ics_todo`), `ATTENDEE`, `ORGANIZER`, `VALARM`, `VTIMEZONE`,
  `X-*` bleiben. Für K.26 bleibt das Lesen und Anzeigen der Teilnehmer und des Status.
- **Neu im Modell:** `organizer` und `attendees` (Name, E-Mail, Teilnahme-Status, Rolle) sowie
  `status` (bestätigt, vorläufig, abgesagt), alle mit `#[serde(default)]`. Damit gehen sie über
  `Events` auch an Agenten (`list_events`, Bereich Kalender, Lesen) – Hinweis in `api.md`, dass das
  E-Mail-Adressen Dritter sind. Ändern von Teilnehmern ist **nicht** Teil dieser Aufgabe.
- **Oberfläche:** Avatare mit Initialen (höchstens 3 + „+N“) in Agenda-Zeile und im Paneel;
  im Radar kleine Avatare am Ende des Bogens, wenn Platz ist; „tentative“ gestrichelt (Agenda,
  Woche, Zeitstrahl, Radar), abgesagte durchgestrichen.
- **API / MCP:** kein neuer Request; `Event` wird reicher (Skill `torro-api`, Review danach).

### K.27 Nachtzeiten einstellen
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.21, K.25 · **Dateien:** `crates/torro-core/src/settings.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/settings_service.rs` (bzw. wo
  `SetSetting` behandelt wird), `crates/torroworkplace/src/ui/{general,apps}.rs` (eins davon, nennen),
  `crates/torrocalendar/src/app.rs`, `crates/torrocalendar/src/views/{timeline,radar}.rs` und ihre
  `layout.rs`, `docs/api-reference.md`; nachgetragen (Rückfrage): `crates/torrocalendar/Cargo.toml`
  (+ `Cargo.lock`, Abhängigkeit `torro-ipc`), `crates/torroworkplace/src/mcp.rs` (`object` im
  Eingabeschema von `settings_set_setting.value`)
- Wunsch des Nutzers (2026-10-11): die Nacht selbst festlegen, per Doppelklick auf den Mond und in den
  Einstellungen. Heute stehen 22–06 als Konstanten in Zeitstrahl und Radar.
- **Einstellung:** `Settings::night { from, to }` (Uhrzeiten auf 15 min, Standard 22:00–06:00,
  `#[serde(default)]`); über Mitternacht ist der Normalfall, `from == to` heißt „keine Nacht“ (nichts
  eingeklappt, kein Nachtsektor). Gilt suite-weit; offene Kalenderfenster übernehmen eine Änderung live
  (wie `appearance`/`zoom`).
- **Oberfläche:**
  - Kalender: **Doppelklick auf den Mond** – im Radar auf das Mond-Icon, im Zeitstrahl auf den Mond im
    eingeklappten Band (ein einfacher Klick klappt weiter auf) – öffnet eine schwebende Karte „Night“
    mit zwei Zeitfeldern „From“/„To“ (15-min-Raster), „Save“ und „No night“. Tooltip am Mond:
    „Double-click to set the night“. Zeitstrahl, Radar und die freie Zeit (die die Nacht auslässt)
    rechnen mit den eingestellten Zeiten.
  - Steuerzentrale: Zeile „Night“ mit denselben Feldern (Abschnitt General oder Apps › Calendar, je
    nachdem, wo es App-Einstellungen gibt; im Bericht nennen).
- **API:** `SettingKey::Night` mit Wert `{ "from": "22:00", "to": "06:00" }` über den vorhandenen
  `Request::SetSetting` · `settings.set_setting` · Admin; Katalogbeschreibung ergänzen; gelesen über
  `settings.get_settings`.
- **MCP:** vorhandenes Tool `settings_set_setting` (Beschreibung nennt `night`).
- **Abnahme:** Tests: Standard, über Mitternacht, `from == to`, ungültige Werte abgelehnt (`error` mit
  Grund), Zeitstrahl-Segmente und Radar-Nachtsektor mit anderen Zeiten (z. B. 23:30–07:00), freie Zeit
  ohne Nacht, Doppelklick-Erkennung; API-Referenz neu erzeugt; fmt/clippy/test (auch `TORRO_LANG=de`)
  grün; Sichtprüfung der Karte und der Einstellungszeile, hell/dunkel.

### Radar: ruhiger, farbiger, mit Mini-Radaren (2026-10-12)

Wunsch des Nutzers (2026-10-12): Die Mitte des Radars zeigt zu viel. „Free for 1 h 20 min“ und „3 h
free“ stimmen nicht: Ein Zeitraum ohne Termin ist nicht unbedingt freie Zeit. Termine sollen
kräftiger farbig sein, beim Überfahren einen Tooltip und den Zeiger `Pointer` zeigen und möglichst
ihren Titel tragen. Rechts sollen statt der Terminliste kleine Radare der nächsten Tage stehen, die
zeigen, wie voll diese Tage sind.

Alle drei Aufgaben sind **reine Ansichten** wie K.22: Termine kommen über `Events`, ein Klick auf ein
Mini-Radar nutzt das vorhandene `Message::GoTo`. Kein neuer Request, kein Katalogeintrag, kein
MCP-Tool – Agenten bekommen dieselben Termine über `list_events`; welcher Tag gezeigt wird, ist
Fensterzustand. Deutsche Übersetzungen nur im Bericht nennen (die Koordination trägt sie ein und
entfernt Einträge, die nicht mehr gebraucht werden).

**Reihenfolge:** K.28, K.30 und K.31 laufen parallel (in `radar.rs` nur die genannten Stellen anfassen),
danach K.29. K.29 färbt auch die Mini-Radare aus K.30.

### K.28 Radar: ruhige Mitte, ohne „freie Zeit“
- [x] erledigt
- **Modell:** Haiku · **Braucht:** K.25, K.27 · **Dateien:** `crates/torrocalendar/src/views/radar.rs`
  (nur `middle`, `free_until`, `free_time`, `free_note`, `Plan::free`/`running`/`next`/`next_inside`/
  `all_day`, Kopfkommentar, Tests dazu), `crates/torrocalendar/src/views/radar/layout.rs` (nur
  `free` und seine Tests)
- **Oberfläche:**
  - **Mitte** nur noch: Datum (`META`, `TEXT_3`), Uhrzeit (`DISPLAY`, tabellarisch) und – beim
    Vorausschauen bzw. an einem anderen Tag – ein runder Icon-Knopf `Clock`
    (`widgets::icon_button`) mit Tooltip „Back to now“ (`widgets::with_tooltip`), der
    `Message::Today` schickt. Der Knopf hat denselben Text als Beschriftung für Screenreader.
  - **Fällt weg:** laufender Termin mit „40 min left“, „Free for …“, „Nothing planned“,
    „Free until …“, „Next 15:00 · Titel“ und die Ganztägig-Chips in der Mitte.
  - **Fällt weg:** „3 h free“ entlang des Bogens (`free_time`) samt `layout::free`. Wochenraster,
    Zeitstrahl und Agenda behalten ihre freie Zeit (`free_gaps` bleibt unberührt).
  - Die Terminliste daneben bleibt in dieser Aufgabe unverändert (die ersetzt K.30).
- **API / MCP:** keiner (siehe oben).
- **Abnahme:** Tests: Mitte ohne Termin-Texte (heute, anderer Tag mit Knopf, gedreht mit Knopf),
  keine Tests mehr für `layout::free`; im Bericht die Texte nennen, die aus `de.rs` entfallen können;
  fmt, clippy, test (auch `TORRO_LANG=de`) grün; `--demo --open radar --now 2026-10-13T14:20`
  Sichtprüfung heute und an einem anderen Tag, hell und dunkel.

### K.29 Radar: kräftige Termine mit Titel, Zeiger, Tooltip, Kalendernamen und Mond
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.28, K.30, K.31 · **Dateien:** `crates/torrocalendar/src/views/radar.rs`,
  `crates/torrocalendar/src/views/radar/layout.rs`, `crates/torrocalendar/src/views/radar/mini.rs`
  (nur die Farbe der Bögen), `crates/torrocalendar/src/app.rs` (nur die Kalendernamen an das Radar)
- **Oberfläche:**
  - **Farbe:** Termine im Radar deutlich kräftiger als die Tönung im Wochenraster: Füllung =
    Kalenderfarbe mit eigener Konstante `FILL` über der Spur (Startwert hell ~45 %, dunkel ~55 %,
    per Screenshot festlegen), Balken am Beginn voll. Der Titel muss auf jeder Kalenderfarbe der
    Demo lesbar bleiben (Kontrast ≥ 4.5:1); reicht `text` nicht, Text nach Helligkeit der Füllung
    hell oder dunkel wählen. Vergangene Termine bleiben verblasst, freie Spur bleibt leise. Gilt
    nur fürs Radar; die anderen Ansichten behalten `Palette::tinted`. Die Mini-Radare (K.30)
    nehmen dieselbe Konstante.
  - **Titel im Bogen:** öfter sichtbar. Schmale Bahnen ab 16 bekommen den Titel in `MICRO` statt
    gar keinen; `TEXT_MIN` und `MIN_TITLE_LETTERS` so wählen, dass in der Demo (Breit und Mittel)
    jeder Termin ab 45 min seinen Titel trägt. Ist er dann unscharf, im Bericht sagen.
  - **Zeiger:** über einem Termin `Pointer`, über dem übrigen Zifferblatt weiter `Grab`. Zuerst
    prüfen, warum der Nutzer keinen Pointer sieht (Verdacht: Ebenen über dem `Canvas` – `stack` mit
    der Mitte, `widgets::anchored` – oder Treffer-Radien), Ursache im Bericht nennen und beheben.
  - **Tooltip:** Ruht die Maus `timeline::TOOLTIP_DELAY` auf einem Termin, erscheint im `Canvas`
    eine Karte wie der Mond-Tooltip: Farbpunkt + Titel (`BODY_STRONG`, gekürzt mit „…“), darunter
    „14:00–15:00“ (`META`, tabellarisch, `TEXT_2`) und – falls vorhanden – der Ort mit `MapPin`.
    Neben dem Zeiger, nie über den Rand des `Canvas`. Verschwindet beim Verlassen, Drücken und
    Ziehen.
  - **Welcher Ring welcher Kalender ist** (Wunsch des Nutzers 2026-10-12):
    - Die Spur jedes Rings leise in seiner Kalenderfarbe getönt (statt `TEXT` 7 %; Startwert ~10 %,
      per Screenshot so, dass Termine weiter klar davor stehen und die Nacht erkennbar bleibt).
    - Der Kalendername steht in jedem Ring entlang des Bogens **kurz vor der Startkante**, wo das
      letzte Viertel ausgeblendet und der Ring leer ist: Farbpunkt + Name (`MICRO`, `TEXT_2`, voll
      deckend, nicht mit ausgeblendet), endet ~8 vor der Kante, gekürzt mit „…“; passt er nicht
      (Bahn < 14 oder zu wenig Bogen), nur der Farbpunkt. Er dreht mit dem Zifferblatt mit.
    - Ruht die Maus auf der leeren Spur eines Rings, sagt der Tooltip den Kalendernamen (der
      Zeiger bleibt `Grab`).
    - Die Namen kommen aus der Kalenderliste der App (`calendar_order` liefert heute nur Ids);
      dafür darf `app.rs` die Namen neben den Ids an `views::radar::view` geben (nur diese Stelle).
    - Tragen zwei sichtbare Kalender denselben Namen (z. B. „Persönlich“ in zwei Konten), steht
      hinter dem Namen der Host des Kontos (`PimAccount::host` aus K.31): „Persönlich ·
      cloud.imt24.de“, im Tooltip immer.
  - **Mond sichtbar machen** (Wunsch des Nutzers 2026-10-12, Screenshot: auf der dunklen Nacht kaum
    zu sehen): Icon 16 statt `SIZE_SM`, in `TEXT_2` auf einer kleinen runden Plakette (~22, `PANEL`
    mit `LINE`-Rand), Kontrast ≥ 3:1 gegen den Grund in hell und dunkel. Er steht an der ersten
    halben Stunde der Nacht, die **nicht** im ausgeblendeten Viertel liegt; nur wenn die ganze
    Nacht dort liegt, blendet er wie bisher mit aus. Ziel für Doppelklick und Tooltip wandern mit.
- **API / MCP:** keiner.
- **Abnahme:** Tests: Farbe/Kontrast der Füllung über hell und dunkel (feste Demo-Farben), Titel-Regel
  (Bahnbreite → Schrift/kein Titel), `mouse_interaction` über Termin/Spur/außen, Tooltip-Zustand
  (Verzögerung, Wechsel des Termins, Verschwinden beim Drücken), Lage am Rand, Kalendername je Ring
  (Lage vor der Kante, gekürzt, nur Punkt bei schmaler Bahn, Tooltip auf leerer Spur); fmt, clippy, test
  (auch `TORRO_LANG=de`) grün; Screenshots Breit, Mittel, Schmal hell/dunkel; `torro-design-review`.
- **Umgesetzt (abweichend):** Füllung in zwei Konstanten `mini::FILL_LIGHT` 0,45 und `FILL_DARK` 0,50
  (mehr lässt im Dunkeln mittelhelle Farben wie `#7FB2FF` und das Standard-Orange für keine
  Schriftfarbe 4,5:1); Spur 10 % in der Kalenderfarbe (`TRACK_TINT`), Zeit = Titelfarbe zu 90 % über
  der Füllung. Text im Bogen beginnt 4 statt 6 hinter dem Balken, `TEXT_MIN` 20, `MIN_TITLE_LETTERS` 3.
  Der Tooltip liegt auf einem zweiten `Canvas` über der Mitte (Iced zeichnet Text eines Canvas über alle
  seine Flächen; auf dem Zifferblatt schienen Ziffern durch die Karte); diese obere Ebene nimmt alle
  Eingaben und lässt den Zeiger in der Mitte zum Knopf „Back to now“ durch. Der Host steht bei
  gleichnamigen Kalendern im Ring und im Tooltip, sonst nirgends. Pointer: auf macOS über Terminen
  sichtbar, Ursache beim Nutzer nicht nachgestellt; abgesichert gegen einen hängenden Druck nach
  Fokusverlust (Bericht). In Mittel tragen überlappende Termine (Bahn < 16) weiter keinen Titel.

### K.30 Radar: Mini-Radare der nächsten Tage
- [x] erledigt
- **Modell:** Opus · **Braucht:** K.25 · **Dateien:** `crates/torrocalendar/src/views/radar/mini.rs`
  (neu), `crates/torrocalendar/src/views/radar.rs` (nur `view`, `list`, `narrow`, `ListAt`,
  `list_view`, `list_row`, `time_width`, `Plan::listed`, `mod mini;`, Tests dazu),
  `crates/torrocalendar/src/views/radar/layout.rs` (nur Sichtbarkeit von Helfern, wenn nötig),
  `crates/torrocalendar/src/app.rs` (nur `range` für `CalView::Radar` und `props`)
- **Oberfläche:**
  - Wo heute die Liste „Next 9 hours“ / „Fading out“ steht (Breit: Eigenschaften-Spalte, solange kein
    Termin geöffnet ist; Mittel: rechts daneben; Schmal: unter dem Zifferblatt), stehen jetzt unter
    der Überschrift „Coming days“ (`prop_section`) **Mini-Radare der 7 Tage nach dem gezeigten Tag**.
    Die Liste fällt weg.
  - **Raster:** Breit und Mittel 2 Spalten, Schmal so viele Spalten à mindestens 104, wie passen.
    Kachel: oben „Wed 14“ (`CAPTION`, `TEXT_2`; morgen „Tomorrow“), darunter das Mini-Zifferblatt.
  - **Mini-Zifferblatt** (vereinfacht, ein `Canvas` je Kachel oder einer für alle): dasselbe
    12-h-Fenster, mit dem das große Radar diesen Tag öffnen würde (`layout::day_start`, Versatz 0),
    damit der Klick genau das zeigt. **Ein** Ring (~12) mit allen Kalendern, Überschneidungen in
    höchstens 2 Bahnen, Termine als Bögen in Kalenderfarbe (Konstante teilen, K.29 stellt sie
    nach); Nacht als Sektor in `night`; Striche nur bei 12/3/6/9; keine Ziffern, kein Text im Bogen,
    kein Ausblenden. In der Mitte die Zahl der Termine mit Uhrzeit (`BODY_STRONG`) bzw. „–“;
    ganztägige als bis zu 3 Farbpunkte darunter.
  - **Bedienung:** Hover hinterlegt die Kachel (`HOVER`) mit Zeiger `Pointer`; Klick →
    `Message::GoTo(tag)` (bleibt im Radar, Versatz 0). Kachel per Tab erreichbar (Knopf), Tooltip
    und Beschriftung „Wednesday, 14 October · 3 events“.
  - **Laden:** `range` des Radars reicht bis zum Ende des siebten Tages (plus den Abend davor, wie
    heute), damit die Mini-Radare ohne eigenes Laden voll sind.
- **API / MCP:** keiner.
- **Abnahme:** Tests: die 7 Tage (auch über Monats- und Jahresende), Fensterbeginn je Kachel gleich
  dem des großen Radars, Bahnen bei Überschneidung, Zählung (ganztägig nicht mitgezählt), Klick →
  `GoTo` und Radar bleibt, `range` deckt 7 Tage, Spaltenzahl je Breite; fmt, clippy, test (auch
  `TORRO_LANG=de`) grün; Screenshots Breit, Mittel, Schmal hell/dunkel; `torro-design-review`.

### K.31 Konten in den Seitenleisten nur mit Host
- [x] erledigt
- **Modell:** Haiku · **Braucht:** – · **Dateien:** `crates/torro-pim/src/account.rs` (neue Methode),
  `crates/torrocalendar/src/app.rs` (nur Gruppentitel der Seitenleiste und des Kalendermenüs),
  `crates/torrotasks/src/app.rs` (nur `account_title`)
- Wunsch des Nutzers (2026-10-12): In der Seitenleiste steht „wagesve@nextcloud.rheinschmiede.net“ –
  zu lang, wird abgeschnitten. Es reicht der Host.
- **Neu:** `PimAccount::host(&self) -> &str`: der Teil von `label` nach dem letzten `@`, sonst
  `label` (auch leer oder ohne `@`). `label` selbst bleibt unverändert (die Steuerzentrale zeigt
  weiter die volle Adresse).
- **Oberfläche:** Kalender und Aufgaben zeigen als Gruppentitel der Konten (Seitenleiste, Rail,
  Menü in Mittel/Schmal) den Host: „nextcloud.rheinschmiede.net“, „cloud.imt24.de“. Haben zwei
  gezeigte Konten denselben Host, bleibt bei diesen die volle Adresse. GitHub-Konten bleiben
  „GitHub · login“. Der Tooltip bzw. die Beschriftung für Screenreader nennt die volle Adresse,
  wenn der Baustein das hergibt (sonst berichten).
- **API / MCP:** keiner – Anzeige; `label` geht unverändert über `PimAccounts`.
- **Abnahme:** Tests: `host` (mit `@`, mehrere `@`, ohne, leer), Gruppentitel in beiden Apps mit
  zwei Hosts und mit gleichem Host; fmt, clippy, test (auch `TORRO_LANG=de`) grün.

### K.32 Woche: heutige Spalte hervorheben
- [x] erledigt
- **Modell:** Haiku · **Braucht:** – · **Dateien:** `crates/torrocalendar/src/views/week.rs`
- Wunsch des Nutzers (2026-10-12): In der Woche soll man sofort sehen, wo heute ist – nicht nur am
  roten Kreis im Kopf.
- **Oberfläche:** Mit mehreren Tagen bekommt die Spalte von heute einen Hauch Torro-Rot über ihrem
  Grund (dunkel 10 %, hell 5 %): im Tageskopf, in der Ganztagsleiste und im Stundenraster, als
  durchgehendes Band. Fällt heute aufs Wochenende, liegt der Hauch über der Wochenendtönung. Die
  Tagesansicht (eine Spalte) bleibt ohne. Rot bleibt „Heute“ (Design-Skill, Abschnitt 1).
- **API / MCP:** keiner – Darstellung.
- **Abnahme:** Test, welche Spalte getönt wird (Woche mit heute, Wochenende, andere Woche,
  Tagesansicht, hell und dunkel); fmt, clippy, test (auch `TORRO_LANG=de`) grün; Screenshot Breit
  hell/dunkel.

### K.33 Arbeitszeit einstellen und anzeigen
- [ ] erledigt
- **Modell:** Opus · **Braucht:** K.27 · **Dateien:** `crates/torro-core/src/settings.rs`,
  `crates/torro-ipc/src/{lib,api}.rs`, `crates/torroworkplace/src/settings_service.rs`,
  `crates/torroworkplace/src/ui/general.rs`, `crates/torroworkplace/src/mcp.rs` (nur die
  Beschreibung von `settings_set_setting`), `crates/torrocalendar/src/app.rs`,
  `crates/torrocalendar/src/views/{week,timeline,radar}.rs` und ihre `layout.rs`,
  `docs/api-reference.md`
- Wunsch des Nutzers (2026-10-12, Issue #66): in den Einstellungen eine Arbeitszeit angeben, der
  Kalender zeigt ihren ganzen Bereich als dünne, gedämpfte Linie. Entschieden: gilt für jeden Tag
  gleich (keine Arbeitstage), eine Arbeitszeit für alle Kalender und Konten.
- **Einstellung:** `Settings::working_hours: WorkingHours { from, to }` aus zwei `Quarter` wie
  `Night`, Standard 08:00–17:00, `#[serde(default)]`, beim Lesen nachsichtig wie `Night` (ungültig
  gibt den Standard). Anders als die Nacht nie über Mitternacht: `from < to`; `from == to` heißt
  „keine Arbeitszeit“ (keine Linie). `WorkingHours::from_value` lehnt alles andere mit Grund ab,
  auch `from > to`. Gilt suite-weit; offene Kalenderfenster übernehmen eine Änderung live (wie
  `night`).
- **Oberfläche:**
  - Kalender: eine dünne, gedämpfte senkrechte Linie über den ganzen Bereich der Arbeitszeit, von
    Beginn bis Ende – in Woche und Tag am linken Rand jeder Tagesspalte, im Zeitstrahl entlang der
    Zeitachse, im Radar als dünner Bogen am Ring. Monat, Agenda, Mini-Ansichten und Mini-Radare (K.30)
    bleiben ohne. Die Linie liegt hinter den Terminen und ist nicht mit der Jetzt-Linie, der
    Nacht-Schattierung oder dem Farbstreifen eines Termins zu verwechseln (Farbe, Stärke und genaue
    Lage nach `torro-design`; im Bericht nennen). Fällt ein Teil der Arbeitszeit in die eingeklappte
    Nacht, endet die Linie am Rand des eingeklappten Bands.
  - Steuerzentrale: Zeile „Working hours“ mit „From“/„To“ (15-min-Raster) unter „Night“ in General
    und „No working hours“ wie bei der Nacht. Wählt man ein „From“ nach „To“, rückt das andere Feld
    mit (kein Fehlerzustand in der Oberfläche).
  - Texte: „Working hours“, „No working hours“ (deutsch im Bericht: „Arbeitszeit“, „Keine
    Arbeitszeit“).
- **API:** `SettingKey::WorkingHours` mit Wert `{ "from": "08:00", "to": "17:00" }` über den
  vorhandenen `Request::SetSetting` · `settings.set_setting` · Admin; Katalogbeschreibung ergänzen;
  gelesen über `settings.get_settings`. Kein neuer Request.
- **MCP:** vorhandenes Tool `settings_set_setting`; Beschreibung nennt `working_hours`.
- **Abnahme:** Tests: Standard, `from == to`, `from > to` und ungültige Werte abgelehnt (`error`
  mit Grund), Lesen einer kaputten Datei gibt den Standard, Anfang und Ende der Linie in Woche,
  Zeitstrahl (auch bei eingeklappter Nacht, z. B. Arbeitszeit 05:00–09:00) und Radar-Bogen mit
  anderen Zeiten (z. B. 07:30–16:00); API-Referenz neu erzeugt; `torro-api-review` ohne Befund;
  fmt/clippy/test (auch `TORRO_LANG=de`) grün; Screenshots Woche, Zeitstrahl, Radar und
  Einstellungszeile, hell/dunkel.

### K.34 Zeitstrahl der Leiste: Titel auch für kurze Termine
- [ ] erledigt
- **Modell:** Opus · **Braucht:** K.29 · **Dateien:** `crates/torrocalendar/src/views/glance.rs`
- Befund aus der Sichtprüfung von U.8 (Leiste 1100×220, Demo am 13.10. um 10:30): Termine von
  30 Minuten sind im Zeitstrahl der Leiste nur ein schmaler Block ohne Text – „Daily Standup“ um
  09:30 und „1:1 with Lena“ um 14:30. `block` kürzt den Titel auf die Blockbreite, und bei rund
  25 Punkten bleibt nicht einmal Platz für „…“.
- Ist der Block zu schmal für einen lesbaren Titel (mindestens ein paar Zeichen und „…“), steht der
  Titel **rechts neben dem Block** auf der freien Spur, in derselben Bahn (`lanes`), bis kurz vor den
  nächsten Block dieser Bahn bzw. das Ende der Spur, und wird dort mit „…“ gekürzt. Ohne Fläche
  dahinter: `CAPTION` in `TEXT` bzw. verblasst wie der Block, wenn er vorbei ist (`Tone`). Die
  Uhrzeit folgt nur, wenn noch eine Zeile Platz hat. Passt auch daneben nichts Lesbares, bleibt es
  beim Block (wie heute).
- Die Jetzt-Linie und ihr Etikett bleiben obenauf; Text neben dem Block darf sie kreuzen, aber nie
  das Etikett verdecken.
- Leiste ist eine Anzeige: keine Tooltips, keine neue Bedienung.
- Neue Texte: keine.
- Abnahme: Tests für die Lage des Titels (im Block, daneben, gar nicht) mit festen Breiten,
  darunter zwei kurze Termine direkt hintereinander in derselben Bahn und ein kurzer Termin am Ende
  der Spur; Sichtprüfung `torrocalendar --demo --now 2026-10-13T10:30` in der Leiste hell und
  dunkel.

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
