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
