---
name: torro-design
description: Design-Regeln für alle Oberflächen von TorroWorkplace (Iced-Views, Widgets, Theme, Tray-Menüs, Benachrichtigungen). Laden, bevor UI gebaut, geändert oder begutachtet wird – Farben, Abstände, Typografie, Zustände, Barrierefreiheit, Texte, Sichtprüfung per Bild ohne Fenster (shots.sh).
---

# Torro-Design

Stand 2026-10-11: **Vorbild ist Linear** (entschieden). Aufbau, Anordnung, Abstände, Schrift und
Symbole folgen Linear; Referenz sind die Entwürfe in `docs/prototypes/linear/` (`calendar.html`,
`tasks.html`, `torro.css`, Screenshots in drei Größen). Was dort steht, gilt; die Werte unten sind
daraus übernommen und seit Issue #291 die des neuen Standards (bündiger Inhalt, Text 14/22, Kopfleiste
64, Haarlinien, Akzentstrich an der gewählten Zeile, Lesespalte). Das Fundament in Iced entsteht mit `docs/tasks/design.md` (F.2–F.6). Gibt es
einen Token oder Baustein noch nicht, ist das kein Freibrief für Einzelwerte: Aufgabe nennen oder
nachfragen.

## Das Ziel – an jeder Ansicht zu messen

1. **Professionell und modern, wie Linear.** Jede Ansicht wirkt wie aus einer Hand: ruhig, klar,
   dicht genug für Arbeit, nie überladen, nichts wirkt zufällig oder wie Iced-Standard.
2. **Schöne, gut lesbare Schrift.** Inter in festen Stufen (Abschnitt 3), saubere Hierarchie.
3. **Abstände immer richtig.** Jeder Abstand und jedes Maß stammt aus den Tokens (Abschnitt 2),
   gleiche Dinge haben überall gleiche Abstände, alles liegt auf einer Achse.
4. **In jeder Größe gut.** Die Apps laufen dauerhaft als Kacheln, von der Mini-Kachel bis zum
   halben 32:9-Monitor. Jede Größenklasse hat ein eigenes, durchdachtes Layout (Abschnitt 6).

Ist eines der vier Ziele verfehlt, ist die Ansicht nicht fertig – auch wenn alle Tests grün sind.

## 0. Fensteraufbau (Linear, für jede App gleich)

Jede App ist ein eigenes Fenster (eine Kachel), aber alle sind gleich gebaut, damit die Suite wie
aus einem Guss wirkt:

```
┌ Seitenleiste 240 (Grund BG) ┐│ Inhalt: bündige Fläche PANEL, Linie zur Seitenleiste ──────────────┐
│ ● ● ●  [▯]                   ││ obere Zeile 40: Titel · Navigation … Ansicht · Filter · [+ Neu]  [☰] │
│                🔍  [✎]      │├───────────────────────────────────────────┬───────────────────────┤
│ Navigation (Zeilen 32)      ││                                           │                       │
│ Gruppe ▾ (CAPTION, TEXT_3)  ││ Inhalt (Liste, Raster, Detail)            │ Eigenschaften 300     │
│ Listen/Kalender mit Punkt   ││                                           │ (nur Breit)           │
│ …                           ││                                           │                       │
│ ↗ andere App (· Settings)   ││                                           │                       │
└─────────────────────────────┘└───────────────────────────────────────────┴───────────────────────┘
```

- **Eine obere Zeile** (#459, `shell::TopRow` an `Shell::top_row`, Entwurf `docs/prototypes/mail-layout/`):
  40 pt hoch (doppelte Höhe der Ampel-Mitte), auf allen Systemen. Links Platz für die Ampel (nur macOS), direkt
  danach der Knopf `panel-left`, in jeder Form an derselben Stelle (ausgeklappt, Leiste, schmal); über der
  ausgeklappten Seitenleiste ist dieser Teil so breit wie sie. **Kein App-Umschalter** (#530): Zwischen den Apps
  wechselt man über das Tray-Menü, das Dock bzw. die Taskleiste und Steuerzentrale › Apps; im Fenster kommt dafür
  nichts dazu. Sprünge mit Bezug („Open in …“, „↗ andere App“) bleiben, ebenso der Kern-Befehl `OpenApp`. Daneben
  die **Kopfzeile der App** (`shell::header_bar`), ganz rechts die **Einstellungen** mit Schiebereglern
  (`shell::SETTINGS_ICON`, nie das Zahnrad; markiert, solange sie offen sind), `control_sm − space::s()` im Quadrat (28 im
  Standard, kleiner als die übrigen Symbolknöpfe, damit die runde Fensterecke ihn nicht anschneidet) und rechts so weit vom Rand wie oben
  und zur Linie unten (`settings_gap`, 5,5 bei Zoom 1, #474). **Jedes Fenster mit Einstellungen** zeigt diesen Knopf
  gleich und an derselben Stelle; Fenster ohne eigene Einstellungen (Anmelden, Laden, Mini- und Leisten-Kachel,
  Schreibfenster, Vorlagen-Dialog) haben nur die Ampel und keinen Knopf. Die freie Fläche zieht das
  Fenster, Dialoge und Schublade dimmen die Zeile (`Cover`). Wer seine Kopfzeile nach Spalten teilt, zieht
  `shell::head_inset` vom ersten Teil ab. Suche und „Neu“ stehen als erste Zeile der Seitenleiste
  (`shell::sidebar_tools`), schmal als erste Zeile der Schublade. Ohne Shell (Anmelden, Glance, Schreibfenster)
  bleibt `shell::titlebar`.
- **Seitenleiste** auf dem Fenstergrund `BG`, **Inhalt bündig** (#291, `layout::inset()` = 0 in allen
  eingebauten Designs): die Fläche `PANEL` reicht bis an die Fensterränder, eine 1-px-`LINE` trennt sie von
  Seitenleiste bzw. Leiste – kein Rand, kein Radius. Ein Design mit `inset` > 0 (bleibt im Format, keine Stufe setzt ihn, #484) bekäme die
  eingesetzte Fläche von vorher (dieser Rand oben/rechts/unten, Radius `md`, Linie rundum); `shell`
  entscheidet das, Apps bauen nichts davon selbst. Ein Farbschema kann der Seitenleiste eigene Farben geben
  (`sidebar…`, L.6): Die Shell zeichnet sie dann in einem eigenen Theme, `palette(theme)` liefert dort
  die Farben der Leiste. In der Seitenleiste also nie Farben am Theme vorbei festlegen, sondern immer
  über `palette(theme)` in der Stilfunktion; Fensterzustände (Anmelden, Laden) liegen auf
  `Surface::Window` (`BG`), Seitenleiste und Rail auf `Surface::Sidebar`. Ohne Seitenleiste (Schmal) ist der Inhalt bündig
  ohne Rand und Radius; neben der Leiste (Mittel) steht er wie in Breit.
- **Seitenleiste in drei Zuständen** (U.2, `shell::SidebarMode`): **ausgeklappt** (`size::sidebar()`, 240), **Leiste**
  (`shell::rail()` = `size::rail()`, 60: nur Icons, Name und Zahl als Tooltip rechts, Gruppentitel als Linie,
  Mini-Monat und Versionszeile entfallen) und **Schublade** (Overlay).
  Breit wählt der Nutzer ausgeklappt oder Leiste – Knopf `panel-left` („Collapse sidebar“/„Expand
  sidebar“) und Taste `[` wie Linear; jede App merkt sich die Wahl in `ui.json` im State-Verzeichnis
  (`torro_gui::ui_state`). Mittel zeigt immer die Leiste, der Knopf öffnet die ausgeklappte
  Seitenleiste als Schublade; Schmal nur Schublade.
- **Spalten sind ziehbar** (Pflicht, #604; zuerst #130 und #376): Jede ausgeklappte Seitenleiste, jede Grenze
  zwischen zwei Spalten und jedes Paneel neben dem Inhalt bekommt einen Griff, damit abgeschnittene Einträge
  lesbar werden: Doppelpfeil, Doppelklick = Standardbreite, beim Ziehen die Linie in `LINE_SELECTED`. Apps
  bauen den Griff nie selbst, sondern nehmen die Bausteine aus `torro-gui`: `Shell::sidebar_width` (Seitenleiste),
  `split::split` (linke Spalte, z. B. Liste|Detail), `split::split_end` bzw. `Shell::props_width` (rechte
  Spalte, Eigenschaften) und `Panel::resizable` (Paneel neben dem Inhalt). Grenzen: `split::limits_around`
  – jede Spalte ¾ bis 2 × ihren Standard, der Nachbar behält seinen Rest (`split::min_pane()`, bei der
  Seitenleiste was der Inhalt braucht); `shell::sidebar_limits`, `shell::props_limits`, `panel::beside_limits`
  sind die fertigen Grenzen. Eine gespeicherte Breite wird beim Anzeigen geklemmt, nicht überschrieben. Jede App
  merkt sich die Breiten in `ui.json` (`ui_state::save_split_task`, beim Start `ui_state::load_splits_task`),
  **je App und Grenze**, nicht je Liste oder Filter, damit beim Wechsel nichts springt; die Demo speichert
  nichts. Gezogen wird, wo zwei Spalten nebeneinander stehen, auch im mittleren Fenster (Aufgaben: Liste|Detail
  mit eigener Breite); wann eine Spalte wegfällt, entscheiden weiter die festen Schwellen, nicht die gezogene
  Breite einer Spalte. Gemessen wird dabei am Inhalt neben der Seitenleiste, wie breit sie gezogen ist: Eine breite
  Seitenleiste kann so die Eigenschaften-Spalte ausblenden, statt drei Spalten in zu wenig Platz zu drängen. Wo die App ihren Inhalt misst, zählt die gezogene Breite. **Fest bleiben** Leiste, Schublade,
  Titelband (`size::sidebar()` bzw. `shell::rail()`), Paneele über dem Inhalt (mittel, schmal) und Kacheln.
- **Ampel** (macOS, U.11): verschoben mit `torro_gui::place_traffic_lights()` beim Start, Mitte bei y = 20 (fest,
  kein Token) – die Mitte der oberen Zeile. Eingeklappt rückt der Inhalt nach links, nichts überlagert. Zeilen einmal
  bauen, mit dem Modus als Parameter: `widgets::sidebar_item_in`, `sidebar_group_in`, `shell::sidebar_tools`,
  `Shell::rail`.
- **Kein „In neuem Fenster öffnen“** (#600): Der Kopf einer Ansicht hat keinen Knopf, der dieselbe Ansicht noch einmal
  in einem eigenen Fenster öffnet. Ein Menüeintrag, der ein Fenster anderer Art öffnet (das Chat-Fenster des Assistenten),
  fällt nicht darunter.
- **Kopfzeile** in der oberen Zeile: links Titel (`typo::body_strong()`) mit leiser Ergänzung in `TEXT_3`, dann
  Navigation; rechts Ansicht, Filter, genau ein Primärknopf. Schmale Fenster haben neben Ampel und Knopf wenig Platz:
  dort entfällt zuerst die leise Ergänzung. Andere Knöpfe als die Einstellungen zeigen keine Schieberegler
  (Ansicht: `list-filter`). Ein **Pfad** („Settings › Accounts › ein Konto“): jeder Schritt vor dem letzten, der
  irgendwohin führt, ist ein `widgets::path_step` (Text `TEXT_3`, Hover wie `ghost`, #264), der letzte
  (`widgets::path_here`) fett, ohne Klick, mit demselben Rand und bei Platzmangel mit „…“; ein Menüknopf im Pfad
  ist `widgets::path_menu_button` (`TEXT_2`). Auf einer **Unterseite** bleibt die Navigation des Bereichs in jeder Größe (#473): Breit in der Liste
  (die Kinder unter dem Abschnitt, alle Geschwister wie alle Konten, #475), Mittel und Schmal als Menüknopf des
  Abschnitts vor dem Namen („‹ [☁ Accounts ▾] › ein Konto“, schmal „‹ [☁ ein Konto ▾]“), nie nur der Pfeil.
- **Mail-Ausnahme der oberen Zeile** (#564, angenommenes Zielbild): Die gesamte normale Kopffläche
  verwendet die aktive Seitenleistenfarbe samt Kontrastfarben (`TopRow::sidebar_surface`), auch in
  Mail-Einstellungen und Fokusansicht; keine fest dunkle Farbe. Die Suchfeld-Innenfläche bleibt als
  Eingabe erkennbar, Menüs und Tooltips behalten das Fenster-Theme. Nur breit mit tatsächlich
  ausgeklappter Seitenleiste steht `panel-left` oben rechts über ihr, mit `space::s()` Abstand vor
  der Trennlinie (`TopRow::sidebar_at_edge`); er folgt ihrer gezogenen Breite. Mit Icon-Leiste,
  schmal und im Drawer bleibt er neben der Ampel. Andere Apps behalten die oben beschriebene
  geteilte Kopffläche und feste Knopfposition. Mini und Leiste bleiben Anzeigen ohne diesen Kopf.
  Über einer offenen Mail (Mittel, Schmal) und dem Paneel der Übersicht (Mittel) bleiben `panel-left` und die
  Einstellungen an ihrer Stelle (`TopRow::keep_buttons`, #545); ein Klick schließt erst die Mail bzw. das Paneel, dann
  öffnet er Schublade bzw. Einstellungen. Der Rest der Zeile bleibt gedimmt bzw. abgedeckt. Andere Apps behalten die
  ganz abgedeckte Zeile (der Termin-Editor des Kalenders).
- **Mail-Fenster** (#451): über der Liste Suchfeld, Sortieren, Aktualisieren; über dem Leser Betreff, „6 von 9“, ⌃ ⌄
  (kein Knopf „In neuem Fenster“, #600). Die **Aktionsleiste schwebt** als Karte (Menü-Stil, Radius `lg`) senkrecht mittig am rechten
  Rand über der Mail (`widgets::floating_rail`); Text und Kopf halten `widgets::floating_rail_room()` Abstand (#545:
  die Leiste verdeckte sonst die Zeit am Ende des Kopfes). Die **Mail-Zeile** hat zwei Zeilen (Absender · Büroklammer · Zeit, Betreff · Markierungen), Tagesköpfe
  und vorn den Kreis des Absenders (`widgets::sender_circle`, Farbe aus `Palette::sender` über die Adresse). Die **Anhänge**
  stehen unter der Mail, nicht im Kopf (#491): breit als Kacheln (`widgets::split_file_chip`) in einem Block unten im
  Leser, mit Abstand zur schwebenden Leiste, mittel und schmal als Zeilen im festen Fuß des Paneels.
- **Eigenschaften-Spalte** 300 breit (Standard, ziehbar) rechts (Linear „Properties“): Gruppen mit `typo::caption()`-Titel in
  `TEXT_3`, Zeilen 32 mit Icon und Wert.
- **Kleiner werden** in dieser Reihenfolge, nie etwas Funktion verlieren: (1) Seitenleiste ins Menü
  oben links („Today ▾“, „Week ▾“), (2) Eigenschaften als schwebende Karte bzw. als Chips unter dem
  Titel, (3) eine Ansicht ersetzt die andere (Detail statt Liste, Tag statt Woche) mit „‹ Zurück“.
- **Hell und dunkel** für alle Apps gemeinsam (Einstellung „Color mode: System / Light / Dark“ in
  der Steuerzentrale › Appearance › Colors, über die API), live umschaltbar.

## 1. Tokens statt Einzelwerte

- **Die Werte kommen aus dem aktiven Design-Paket** (`torro_gui::design`, Konzept
  [`docs/design-pakete.md`](../../../docs/design-pakete.md)); die Zahlen in diesem Skill sind die des
  Standards (id `torro`, Name „Standard“, `designs/torro/design.toml`). Daneben gibt es seit #484 genau zwei
  weitere Abstandsstufen, Comfort (`comfort`, „Komfort“) und Dense (`dense`, „Dicht“): Sie ändern nur Abstände und
  Höhen (`[space]`, `[layout]`, Höhen unter `[size]`); Text, Radien, Breiten und `[list]` sind in allen Stufen die des
  Standards. Eigene Designs aus einem Ordner gibt es nicht mehr, Schrift und Farbschema schon. Ein Paket ändert,
  *wie* ein Token aussieht, nie, *wofür* er benutzt wird – die Regeln hier gelten für jede Stufe, jede Schrift und
  jedes Farbschema. Darum sind Tokens Funktionen (`space::m()`, `size::row()`,
  `typo::body()`, `theme::font()`, `palette(theme)`) und kommen nie in ein `const` oder `static`;
  abgeleitete Werte werden in einer Funktion berechnet.
- **In Apps keine Zahlen** in `.size()`, `.spacing()`, `.padding()`, `.width()`/`.height()`,
  `.rounded()` und kein `Color::from_rgb…`: nur Tokens aus `torro_gui::{theme, typo, layout}`.
  **Keine Iced-Standardstile** (`button::primary`, `button::secondary`, Standard-Checkbox …): nur
  `torro_gui::style` und die Bausteine aus `torro_gui::widgets`. `scripts/design-check.sh` prüft das
  (F.6). Fehlt ein Token oder Stil: in `torro-gui` ergänzen (eigene Aufgabe/Rückfrage), nie lokal.
- Markenfarben kommen aus `torro-design/tokens/tokens.json` → `torro-core/src/theme.rs`; alle Werte
  des Standardpakets (Flächen, Textstufen, Schrift, Abstände, Maße) stehen in
  `torro_core::design::Design::standard()`, `torro-gui` liest sie über `torro_gui::design`. (Das Repo `torro-design`
  liegt nicht im Workspace; Änderungen dort als Rückfrage melden.)

### Farben

| Token | Dunkel | Hell | Wofür |
|---|---|---|---|
| `BG` | `#08090A` | `#F4F5F6` | Fenstergrund, Seitenleiste (ein Paket kann ihr mit `sidebar` eine eigene Fläche geben) |
| `PANEL` | `#0F1011` | `#FFFFFF` | Inhaltsfläche |
| `RAISED` | `#141518` | `#F8F9FA` | Knöpfe `secondary`, Chips, Gruppenköpfe in Listen |
| `HOVER` | `#16171A` | `#F0F1F3` | Hover |
| `SELECTED` | `#1C1E22` | `#E9EAED` | Auswahl, gedrückt, aktives Segment |
| `MENU` | `#17181B` | `#FFFFFF` | Menüs, schwebende Karten, Dialoge (mit Schatten) |
| `SCRIM` | Schwarz 62 % | Schwarz 30 % | Abdunkeln hinter Dialogen |
| `TEXT` | `#EDEEF0` | `#18191B` | Text |
| `TEXT_2` | `#B4B8BF` | `#4F535A` | Sekundärtext, Beschreibungen |
| `TEXT_3` | `#8A8F98` | `#62666D` | Metadaten, Zähler, Gruppentitel, Platzhalter (≥ 4.5:1 auf allen Flächen) |
| `LINE` | Weiß 6 % | Schwarz 7 % | Trennlinien, Raster, Rand der Inhaltsfläche |
| `LINE_STRONG` | Weiß 10 % | Schwarz 11 % | Rahmen von Knöpfen, Eingaben, Chips, Menüs |
| `LINE_SELECTED` | Weiß 28 % | Schwarz 32 % | Kante von Gewähltem (Chip, aktives Segment) und Hover gerahmter Knöpfe (`Palette::line_selected`) |
| `FOCUS_RING` | Weiß 35 % | Schwarz 50 % | Fokus, 2 px, 1 px Abstand (≥ 3:1) |
| `ACCENT` | Torro-Rot `#D50C0C` | dto. | **nur** Primärknopf, Heute, Jetzt-Linie, App-Logo |
| `merged` | `#A78BFA` | `#7C5CE6` | GitHub „gemergt/erledigt geschlossen“ (Violett): **nur** Icon oder Punkt neben einem Wort (≥ 3:1 auf allen Flächen, `Palette::merged`), nie als Text |
| Kalender/Listen | `#5B8CFF` `#3FB984` `#E8A33D` `#D96CA8` `#A78BFA` | `#3B6EF0` `#1F9D68` `#B06F12` `#C2508F` `#7C5CE6` | Standardfarben, Tönung 18 % (hell 14 %) |
| `chart` | `#3B78D8` `#C9701E` `#2E9E6B` `#B5509A` | `#2F6BC9` `#B8621A` `#2A9466` `#A8448C` | Diagrammreihen in fester Reihenfolge (E8.8b), mit dem Dataviz-Validator geprüft; nur Marken, nie Text, nie Status oder Rot |
| `chart_other` | `#5C6068` | `#50545B` | Grau der Reihe „Other“ (`ChartColor::Other`) |

### Akzent und Status

- **Torro-Rot ist Marke, nie Status, nie Auswahl.** Pro Fenster: ein Primärknopf, Heute-Kreis,
  Jetzt-Linie mit Zeitlabel in der Achse, das App-Logo. Aktiver Reiter, gewählte
  Seitenleistenzeile, ausgewählte Zeile: neutral (`SELECTED` + `TEXT`), wie bei Linear.
- **Ausnahme: Akzent für die gewählte Zeile einer Liste** (#291, entschieden in der Planung). Das Design
  sagt, wie die gewählte Zeile (die im Detail/Leser) aussieht (`[list] selection`, `layout::list()`):
  `bar` (alle drei Stufen) – neutral `SELECTED` mit einem 2-px-Strich in `ACCENT` links
  (`widgets::selection_bar`); `fill` (bleibt im Format, keine eingebaute Stufe schaltet es ein) – die ganze Zeile in `ACCENT`, ihr Text und ihre Icons in
  `on_accent` (`style::button::list_row`, Zellen über `widgets::row_content`) – auch ihre farbigen Marken
  (Listenpunkt, Farbbalken eines Termins, Prioritätsbalken, Fälligkeit; entschieden zu L.32), sonst wären sie auf
  `ACCENT` nicht lesbar; Kreise, Pillen und Punkte bleiben rund. Nur diese eine Zeile, nie Seitenleiste, Reiter,
  Chips oder Hover; mit `bar` bleiben die eigenen Farben in der Zeile (Status, Listenfarbe).
- Status: Fehler `STATUS_ERROR`, Warten/Abweichung/überfällig Attention-Orange, OK Grün, inaktiv
  Grau – immer mit Wort oder Icon, Farbe nur im Punkt bzw. Icon.
- Kalender- und Listenfarben gehören dem Nutzer: als Farbbalken, Punkt, Ring oder Häkchen, nie als
  großflächige gesättigte Fläche.

## 2. Abstände, Maße, Radien, Linien

- **Abstände** `space::xs() 4 · s() 8 · m() 12 · l() 16 · xl() 24 · xxl() 32` (48 nur für große
  Leerzustände).
  Innerhalb von Bausteinen sind 2 und 6 erlaubt (Segment-Innenrand, Icon-Text-Abstand in Chips),
  sonst nur die Skala.
- Label → Feld 4, innerhalb einer Gruppe 8, Gruppe → Gruppe 16–24. Nähe zeigt Zugehörigkeit.
- **Abstände nach Rolle** (`torro_gui::layout`): `layout::inset()` 0 (bündiger Inhalt; > 0 wäre der Rand
  einer eingesetzten Inhaltsfläche), `page_padding()` 24 × 24 (Innenrand von Seiten), `panel_padding()`
  16 × 16 (Paneele, schwebende Karten), `row_padding()` 18 × 24 (Listen- und Einstellungszeilen: Zeilen
  fester Höhe, auch die mehrzeiligen des Eingangs, nehmen nur die Seiten und zentrieren ihren Inhalt),
  `control_padding()` 0 × 12 (Knöpfe, `style::button_padding()`), `section_gap()` 24 (zwischen Gruppen
  und Abschnitten), `item_gap()` 10 (innerhalb einer Gruppe), `reading_padding()` 52 × 64 (die Mail im
  Leser, in der Lesespalte `size::reading_max()`). Nur dort, wo die Rolle genau passt; alles andere
  bleibt auf der Skala `space::*`.
- **Abstandsstufen** (#484): Comfort und Dense ändern die Skala, die Abstände nach Rolle (außer `inset`) und die
  Höhen; Breiten (`sidebar`, `list`, `props`, `panel`, `content_max`, `reading_max` …) bleiben. Werte
  Comfort / Standard / Dense: Mail-Zeile 116 / 100 / 78, andere Eingangszeile 92 / 80 / 56, Aufgabenzeile
  56 / 48 / 42, Seitenleistenzeile 36 / 32 / 28, `control_sm` 40 / 36 / 30, `control` 42 / 38 / 32, `control_lg`
  44 / 40 / 36, Kopfleiste 64 / 64 / 48, Stunde 56 / 48 / 40, `page_padding` 32 / 24 / 16, `panel_padding` 20 / 16 / 12,
  `row_padding` 24×32 / 18×24 / 6×16, `section_gap` 32 / 24 / 16, `item_gap` 14 / 10 / 6, `reading_padding`
  64×80 / 52×64 / 32×40, Skala `[4,10,16,20,32,40]` / `[4,8,12,16,24,32]` / `[2,6,8,12,16,24]`, `rail` 64 / 60 / 52,
  `rail_wide` 264 / 248 / 232 (`designs/{comfort,dense}/design.toml`). Eine Ansicht muss in allen drei Stufen
  passen: Dieselbe Textzeile (22) steht in Dense mit weniger Luft, in Comfort mit mehr.
- **Maße** (Werte des Standards seit #291):

| Token | Wert | Wofür |
|---|---|---|
| `size::control_sm()` | 36 | Knöpfe, Icon-Knöpfe, Segmente, Menü-Knöpfe in der Kopfleiste |
| `size::control()` | 38 | Eingaben, Eigenschaftszeilen |
| `size::control_lg()` | 40 | Schnelleingabe, Primärknopf im Dialog, Knöpfe der Aktionsleiste der Mail |
| `size::control_xl()` | 48 | Knöpfe einer Erinnerung im Vollbild (ER1.5), Text `typo::body_strong()`; nirgends sonst (die Leiste der gewählten Mail war bis #227 so hoch, sie ist jetzt `control_sm()`) |
| `size::command()` | 46 | Bleibt im Paket-Format (E14, Entscheidung 9), kein Baustein nutzt es mehr: die Befehlsleiste des Lesers (E11.1) ist mit #241 entfernt |
| `size::pill()` | 40 | Avatar im Kopf des Lesers (E11.1); die Torro-Pillen, die so hoch waren, sind mit #241 entfernt, das Maß bleibt im Paket-Format |
| `size::row()` | 48 | Listenzeile (Aufgaben, `widgets::list_row`) |
| `size::row_message()` | 100 | Mail-Zeile im Eingang: Avatar (`[list] avatar`, 40), Absender · Domain · Zeit, Betreff, Vorschau – drei Zeilen à 22, über der Haarlinie zentriert mit je 16,5 Luft – die Zeile nimmt von `row_padding` nur die Seiten (E3.9, E11.1); in der Form `line` eine Zeile. Das Mail-Fenster hat eigene, schlanke Zeilen (#458, `list::slim_message_height()`: zwei Zeilen `body` + 2 × `space::m()` + Haarlinie) |
| `size::row_notice()` | 80 | Andere Zeile im Eingang: Kachel 28 mit Quell-Icon, Titel · Zeit, Text – zwei Zeilen à 22, über der Haarlinie zentriert mit je 17,5 Luft (E3.9) |
| `size::card_gap()` | 12 | Abstand zwischen Karten und zwischen den Teilen einer Karte (`widgets::card`, E8.2) |
| `size::avatar()` / `size::avatar_lg()` | 40 / 48 | Avatar einer Karte / Absender im Kopf des Lesers (E8.2); die Mail-Zeile hat ihren eigenen (`[list] avatar`) |
| `size::tile_sm()` / `size::tile()` / `size::tile_lg()` | 20 / 28 / 36 | Quellen-Kachel (`widgets::source_tile`, E8.2): neben Wörtern / Hinweiszeile, Spaltenkopf / Anhang, Kopf einer App-Karte |
| `size::row_sidebar()` | 32 | Seitenleisten- und Menüzeile (Menü 30) |
| `size::row_group()` | 40 | Gruppenkopf in Listen („Overdue 2“), mit Platz für seinen „+“-Knopf |
| `size::header()` | 64 | Kopfleiste |
| `size::sidebar()` | 240 | Seitenleiste (Standard, ziehbar) |
| `size::props()` | 300 | Eigenschaften-Spalte (Standard, ziehbar) |
| `size::panel()` | 400 | Seitenpaneel für Details (`torro_gui::panel`): Breit daneben, in Schmal darüber in voller Höhe (die Aufgaben-App zeigt ihr Detail auch in Mittel daneben, Abschnitt 6) |
| `shell::rail()` = `size::rail()` | 60 | Eingeklappte Seitenleiste (Leiste, nur Icons) und eingeklappte Aktionsleiste der Mail; mindestens `control_lg()` |
| `size::rail_wide()` | 248 | Ausgeklappte Aktionsleiste (`RailMode::Expanded`); das Mail-Fenster nutzt seit #456 die schwebende Leiste |
| `size::reading_max()` | 600 | Lesespalte: so breit steht der Text einer Mail höchstens im Leser (in allen Stufen) |
| `size::list()` / `size::list_medium()` | 440 / 364 | Liste neben dem Detail (Breit / Mittel) |
| `size::hour()` | 48 | Stundenhöhe im Kalender, Zeitachse 52 (Schmal 44) |
| `layout::inset()` | 0 | Rand der Inhaltsfläche; 0 = bündig mit Linie zur Seitenleiste |
| `size::content_max()` / `size::form_max()` | 720 / 640 | Lesbreite von Listen bzw. Formularen ohne Detail; in Einstellungsseiten (#464) `content_max` die Textbreite einer Zeile und die Breite der Wertspalten einer Tabelle, `form_max` die Spalten-Umbrüche (`widgets::page_columns`) |

- Knöpfe: Höhe `size::control_sm()`, Innenabstand waagerecht `control_padding()` (12), Text `typo::caption()`-Größe 12/500; Icon-Knöpfe
  `control_sm()` im Quadrat; Icons 16 (in Knöpfen 14), Strich 1.75 – auch in den größeren Knöpfen von
  Standard und der Aktionsleiste bleiben Icons 16.
- **Radien** `radius::xs()` (Chips, Terminblöcke, Segment innen, Tasten-Hinweis) · `sm()` (Knöpfe,
  Zeilen, Eingaben, Menüeinträge) · `md()` (Menüs, eingesetzte Inhaltsfläche) · `lg()` (schwebende Karten,
  Dialoge) · voll rund für Punkte, Ringe, Avatare, Heute. Alle drei Stufen haben überall 4; die Rollen bleiben,
  nur die Werte sind gleich.
- **Linien:** 1 logischer Pixel in `LINE`; Raster nur mit Innenlinien (keine Rahmen je Zelle,
  keine Doppellinien). Listenzeilen durch Linien getrennt, Seitenleiste ohne Linien.

## 3. Typografie

- **Schrift: Inter 4** (SIL OFL), mit der App ausgeliefert (`torro_gui::fonts()`,
  `torro_gui::settings()`), **Grundgröße 14** in allen drei Stufen (#291, #484). Nie `Font::DEFAULT`.
  Größer oder kleiner nur über den **Zoom** der Suite (Z.1, `torro_gui::zoom`): Strg + Plus /
  Minus / 0 in jeder App, Stufen wie im Browser (67–200 %), eine Einstellung für alle (`Settings::zoom`,
  Steuerzentrale › Appearance › Display). Umgesetzt mit Iceds `scale_factor`, nie über andere
  Schriftgrößen. Die Größenklassen rechnen in logischen Pixeln *nach* dem Zoom (1440 breit bei
  150 % = 960 = Mittel), wie Breakpoints im Browser. Jede App: `.scale_factor(…)` im Builder,
  `zoom.subscription()`, `zoom.view(…)` um die ganze Ansicht (kurze Anzeige „110%“ unten mittig).
- **Stufen** (`torro_gui::typo`), Zeilenhöhe immer absolut; die Werte des Standards, gleich in Comfort und Dense:

| Stufe | Größe / Zeile | Gewicht | Wofür |
|---|---|---|---|
| `typo::micro()` | 11 / 14 | 400 | Zeitachse, Uhrzeit in Terminen, Wochentag in der Wochenleiste (immer tabellarisch) |
| `typo::caption()` | 12 / 16 | 500 | Gruppentitel (Satzschreibung, `TEXT_3`), Knöpfe, Chips, Badges |
| `typo::meta()` | 13 / 18 | 400 | Metadaten, Zähler, Fälligkeit |
| `typo::body()` | 14 / 22 | 400 | Standard: Listen, Seitenleiste, Eingaben, Fließtext |
| `typo::body_strong()` | 14 / 22 | 500 | Kopfleisten-Titel, Zeilentitel, Termintitel |
| `typo::read()` | 16 / 26 | 400 | Lesetext: die ganze Mail im Leser des Eingangs (E8.2); nie für Zeilen, Labels, Eingaben |
| `typo::title()` | 22 / 28 | 600 | Karten- und Dialogtitel |
| `typo::headline()` | 28 / 36 | 600 | Detailtitel („Review API concept“), Ansichtstitel ohne Kopfleiste |
| `typo::display()` | 28 / 32 | 600 | Kopfzahl in Mini (Uhrzeit, Zahl offener Aufgaben) |
| `typo::countdown()` | 28 / 32 | 500 | Countdown und Uhr einer Erinnerung im Vollbild (ER1.5), Ziffernschrift; nirgends sonst |
| `typo::hero()` | 48 / 56 | 600 | Titel einer Erinnerung im Vollbild (ER1.5), höchstens zwei Zeilen; nirgends sonst |

- Gewichte nur 400/500/600; kein Bold, keine Kursive. Hierarchie über Stufe *und* Farbe
  (`TEXT` → `TEXT_2` → `TEXT_3`).
- **Keine Versalien** für Abschnittstitel: Satzschreibung in `typo::caption()` (wie Linear).
- **Ziffern, die sich ändern** (Uhrzeiten, Achsen, Zähler, Daten in Spalten): `.tabular()`.
- Lange Texte in Zeilen einzeilig mit Auslassung; in Detailansichten umbrechen. Kleinste Schrift 11
  (`typo::micro()`), nur für tabellarische Ziffern und Wochentagskürzel.

## 4. Kontrast und Barrierefreiheit (WCAG 2.2 AA)

- Text ≥ 4.5:1, große Schrift (≥ 18) und Bedienelemente/Icons ≥ 3:1.
- Gemessen dunkel: `TEXT` ≥ 14.4, `TEXT_2` ≥ 8.4, `TEXT_3` ≥ 5.1 auf allen Flächen; hell: `TEXT`
  ≥ 14.6, `TEXT_2` ≥ 6.4, `TEXT_3` ≥ 4.8. Weiß auf Torro-Rot 5.4; Rot auf dunklem Grund nur 3.5 →
  Rot nie als kleiner Text, nur als Fläche mit weißer Schrift.
- Information nie nur über Farbe: Status zusätzlich mit Text, Icon oder Form.
- Alles per Tastatur bedienbar: Tab-Reihenfolge logisch, `Enter` bestätigt, `Escape`
  schließt/bricht ab, Pfeiltasten in Listen und Rastern, Kurzbefehle für die Hauptaktionen
  (in Menüs als Tasten-Hinweis rechts). Fokus sichtbar (`FOCUS_RING`, nicht Rot).
- Klickziele mindestens 24×24, sonst 28.

## 5. Zustände und Bausteine

**Unmittelbare Rückmeldung ist Pflicht (#560).** Jede Aktion erhält sofort eine passende sichtbare
Reaktion. Laufend, erfolgreich und fehlgeschlagen bleiben unterscheidbar, mit Wort oder Icon statt
nur Farbe. Teilerfolg und Abbruch sind kein Erfolg. Beispiel Speicher-Untermenü: Es bleibt bis zum
bewussten Schließen offen; rechts am angeklickten Ziel drehen sich während der Speicherung die
`RefreshCw`-Pfeile. Erst nach Prüfung der tatsächlichen Zieldatei durch den Dienst erscheint ein grüner
`CircleCheck`. Fehler zeigen `CircleX` mit Erklärung, Teilerfolge und gekürzte Exporte `TriangleAlert`
mit Anzahl/Details. Zielwahl im Dialog und Abbruch haben einen ruhigen neutralen Hinweis. Laufende
Einträge bleiben voll lesbar und nehmen keinen zweiten Klick (`widgets::menu_item_feedback`);
Pfad, Vorschlag und Status gehören eindeutig zur Aktion, Mail und Ablage.

**Listen (`[list]`, #291, `layout::list()` → `ListLook`).** Ein Design bestimmt, *wie* Listen gezeichnet
werden, nie *wo* etwas steht: dieselben Zellen an derselben Stelle in jedem Design, nichts kommt hinzu
oder fällt weg (kein Spaltenkopf, keine Großbuchstaben). `style` – `rows` (Zeilen mit ihren Zeilen: alle drei
Stufen), `line` (eine Zeile je Eintrag, `size::row_message()` hoch) oder `cards`
(Karten, die bequeme Dichte, `layout::density()`); `line`, `cards` und `selection = "fill"` bleiben im Format und im
Code, aber keine eingebaute Stufe schaltet sie ein (#484); `separators` – Haarlinie unter jeder Zeile ja/nein;
`selection` – `bar` oder `fill` (Ausnahme oben); `avatar` – Kante des Avatars einer Mail-Zeile, 0 = keiner
(der Platz entfällt); `preview_lines` – Zeilen Vorschau, 1 oder 2. `widgets::list_row` folgt dem selbst
(Linie, Strich bzw. Fläche, Zellen nur mit dem Seitenrand von `row_padding()`, in der Höhe zentriert);
Ansichten, die ihre Zeilen selbst zeichnen, nehmen `style::button::list_row`, `widgets::selection_bar` und
`widgets::row_content` und lesen `layout::list()`. Werte: `rows · true · bar · 40 · 1` in allen drei Stufen (sie
erben `[list]` vom Standard).

**Die Zeile in `line`** (#291, nach der Sichtprüfung entschieden; bis #484 die Form von Console): dieselbe Reihenfolge wie die Zeilen
in Standard, nur auf einer Zeile – Markierungen bzw. Kachel · Absender mit Domain bzw. bei Hinweisen die
Quelle als Name (`SourceRef::label`) · Betreff bzw. Titel (· Zahl) mit der Vorschau bzw. dem Text in `TEXT_3` ·
Konto · Zeit rechts (`list::LineColumns`, `mail_line`, `notice_line`). Mail und Hinweise teilen dieselben
Spalten, eine gemischte Liste (Hub) hat ein Raster; die Titelspalte hängt nicht von der Zeit der Zeile ab.
Was nicht passt, wird mit „…“ gekürzt statt weggelassen (der Titel höchstens 60 % seiner Zelle, solange eine
Vorschau folgt). Schmal (unter 600) nimmt die Zeit ihre eigene Breite statt der Spalte der breitesten, ein
eigenes kurzes Zeitformat gibt es bewusst nicht.

**Kategorie-Pille (E12.7, `docs/kategorien.md`).** Was eine Mail ist, steht als `widgets::category_pill`
in Zeile, Leser und Paneel. Regel: **Farbe und Icon je Gruppe, Wort je Kategorie** – vier Farben
(`palette.category`, in der Reihenfolge von `CategoryGroup`) und vier Icons (`Inbox`, `Archive`, `Info`,
`ShieldAlert`); 23 Farben wären nicht mehr auseinanderzuhalten. Die Pille ist **gefüllt** (Issue #613): Fläche in
der Gruppenfarbe, nur so weit abgedunkelt, dass weißer Text 4,5:1 erreicht (`theme::category_fill`), Icon und Wort
weiß, ohne Rand. Eine Kategorie vom Modell sieht gleich aus und sagt es im Tooltip („Shipping · suggested“), eine aus
einer Kopfzeile oder vom Menschen hat keinen Tooltip (`docs/ki.md`, Grundsatz 5); keine Sparkles an der Pille. Die
schmale Liste des Mail-Fensters zeigt die Pille rechtsbündig am Ende der Betreffzeile, hinter den Markierungen (Issue
#635: so steht sie in jeder Zeile an derselben Stelle); behielte der Betreff dann weniger als die Hälfte des Platzes,
steht nur das Feld mit dem Icon (`list::category_fits`). Im Mail-Kopf steht sie rechtsbündig unter dem Eingangszeitpunkt,
am Ende der Adresszeile, mit derselben Regel. Die Pille
ist kein Knopf: Geklickt wird die Zeile, geändert wird über ihr Menü.


- **Jedes Bedienelement** hat fünf Zustände: normal; Hover = `HOVER`; gedrückt/ausgewählt =
  `SELECTED`; Fokus = `FOCUS_RING`; deaktiviert = 40 % Deckkraft, keine Hover-Fläche.
- **Knöpfe** (`style::button`): `primary` (Rot, weiß, höchstens einer pro Fenster), `secondary`
  (`RAISED` mit `LINE_STRONG`-Rand), `ghost` (transparent, `TEXT_2`, Hover `HOVER` + `TEXT` –
  Standard in Kopfleisten), `icon` (28×28 ghost), `boxed` (Icon-Knopf mit Rand, für „Neu“ in der
  Seitenleiste), `danger` (ghost mit `STATUS_ERROR`-Text). Wird der Primärknopf in Schmal zum Icon
  (`widgets::compact_button`), ist er 28×28 mit Icon 16 in `on_accent` – so bleibt das „+“ auf Rot
  kräftig (5,4:1). **Primärknopf des Mail-Fensters** ist „New email“ ganz oben in der Seitenleiste (#476): `primary`
  über die Breite der Leiste, Stift und Wort in `on_accent`, die Taste als `widgets::kbd_on_accent` (Schrift
  `on_accent`, Rand `on_accent` zur Hälfte), in der Leiste `compact_button` mit `primary`. Ohne Mail-Konto bleibt er
  `boxed` und abgeschaltet – ein blasses Rot wäre hell kaum lesbar. Die rote Antworten-Gruppe der Aktionsleiste
  (`RailGroup::accent`) steht daneben als Hauptgruppe der geöffneten Mail, nicht als zweiter Primärknopf. Ansichten
  wechseln: `segmented` (Grund
  `BG`, aktives Segment `SELECTED` mit `LINE_SELECTED`-Kante). Speichert ein Knopf, zeigt er
  `widgets::busy_button(tr("Creating…"), style)`: volle Farbe, Icon, nimmt keinen zweiten Klick.
- **Knopfleisten auf Karten** (E8.12): alle Knöpfe mit Wort oder keiner (`widgets::bar_words`, Schätzung
  mit `widgets::text_button_width`); nur die Hauptaktion vorn (Open, Go to window, Reply) darf ihr Wort
  allein behalten (`BarWords::Lead`). Sichtbare Kartenknöpfe sind `secondary`, was den Eintrag nur
  loslässt (Done, Dismiss, Not now) `ghost`; ✕ im Kartenkopf und die Glocke bleiben Icon-Knöpfe. Große
  Allow/Deny (`size::control_lg()`, halbe Kartenbreite) nur für Entscheidungen in Schmal, sonst nirgends.
  Menüs eines Kartenknopfs hängen an der Knopfzeile und öffnen über ihr, wenn unten kein Platz ist – nie
  über dem Auslöser (`Placement::Below`).
- **Wählbare Chips** (G.10): `widgets::choice_chip(label, mark, on, on_press)` – `size::control_sm()` hoch, Radius
  `radius::sm()`, `RAISED` + `LINE_STRONG`; Hover `HOVER` + `TEXT`; gewählt `SELECTED`, Kante `LINE_SELECTED`, Häkchen 14
  vor dem Text. GitHub-Labels zum Wählen: `widgets::label_choice(hex, name, on, on_press)` (Farbe nur als
  Punkt 8). Filter und Vorlagen werden angeklickt, nicht getippt.
- **Aktionsleiste einer Zeile** (E3.9, `widgets::row_actions`, für Karten mit Knopfzeile
  `widgets::card_actions`): schwebt rechts in der Zeile, beim Überfahren und immer an der Zeile unter
  dem Tastaturcursor; Fläche `MENU`, Rand `LINE_STRONG`, Radius `radius::sm()`, Innenrand 2, Knöpfe
  `size::control_sm()` mit Icon und Wort (`widgets::RowAction`: Icon, Wort, Taste, Nachricht), Tooltip
  mit Wort und `kbd`. Wörter nur, solange die Leiste höchstens die halbe Zeile braucht
  (`widgets::row_actions_words`), sonst nur Icons. Nur Aktionen, die für den Eintrag gehen; ein Knopf
  tut, was seine Taste an der Zeile tut. Zeilen mit mehreren Zeilen Text (Mail-Zeile, Hinweis mit
  Text) setzen die Leiste auf die erste Zeile, über die Zeit (`widgets::row_actions_at`, E3.11), damit
  Vorschau und Konto darunter frei bleiben.
- **Dichte im Eingang** (E8.2, seit #226 ein Wert des Designs: `torro_core::design::Density`,
  `torro_gui::layout::density()`): `Density::Compact` (eine Zeile je Eintrag, Aktionsleiste beim Überfahren) und
  `Density::Comfortable` (Karten). Sie folgt seit #291 `[list] style`: `cards` gibt Karten, `rows` und `line` eine
  Zeile je Eintrag; es gibt keinen Umschalter in der Kopfleiste, keinen Startschalter und nichts Gemerktes. Alle drei
  Abstandsstufen zeigen eine Zeile je Eintrag; Karten schaltet keine eingebaute Stufe ein. Das Design entscheidet in
  jeder Größenklasse; Mini und Leiste bleiben Anzeigen ohne Dichte. Alle Bausteine beider Dichten anschauen:
  `cargo run -p torro-gui --example gallery -- --cards`.
- **Karte** (E8.2, `widgets::card(teile, gewählt, on_press)`): Fläche `RAISED`, Rand `LINE_STRONG`, Radius
  `radius::lg()`, Innenrand `layout::panel_padding()` (16), Teile mit `size::card_gap()` (12) untereinander, Karten
  mit demselben Abstand. Gewählt (Cursor, im Leser) `SELECTED` mit Kante `LINE_SELECTED`; mit `on_press` ist die
  ganze Karte ein Knopf (Hover: Kante `LINE_SELECTED`), Knöpfe darin nehmen ihren Klick selbst. **Knöpfe der Karte
  sind immer sichtbar**: eine Reihe `text_button`/`icon_button` mit `space::s()`, nicht die Hover-Leiste.
  `widgets::card_head(lead, titel, meta, verwerfen)`: Kachel oder Avatar links, Titel `typo::body_strong()`, darunter
  Meta `typo::meta()` in `TEXT_3`, beide einzeilig mit „…“ und Tooltip; ✕ („Dismiss“) oben rechts nur, wo der
  Eintrag verschwinden darf (nicht bei Mail im Mail-Fenster, nicht bei Freigaben). Chips der Karte (Anhang,
  „Answer by Fri“): `widgets::state_pill` in `widgets::chip_row`, Farbe nur im Icon.
- **Quellen-Kachel** (E8.2, `widgets::source_tile(gesicht, größe, tönung)`): `TileFace::Icon(…)` oder
  `TileFace::letter(name)`, `TileSize::Small | Medium | Large` (20 / 28 / 36, Radius xs / sm / md). Neutral
  `SELECTED` mit Rand `LINE_STRONG`, Zeichen in `TEXT`; mit Farbe des Nutzers nur getönt (`Palette::tinted` über
  `PANEL`, ohne Rand), nie die volle Farbe als Fläche. Alle Bausteine der Dichten anschauen:
  `cargo run -p torro-gui --example gallery -- --cards` (dazu `--light`, `--design comfort`, `--design dense`).
- **Aktionsleiste der Mail** (E14, nur das Mail-Fenster, alle Größen; Entwurf
  `docs/prototypes/inbox/mail-aktionsleiste.html#d`): Alles, was man mit der geöffneten Mail tun kann, steht in
  **einer Leiste, die seit #456 über der Mail schwebt** (`widgets::floating_rail`, Entwurf
  `docs/prototypes/mail-layout/`): eine Karte im Menü-Stil mit Schatten, Radius `lg`, senkrecht mittig, `space::s()`
  vom rechten Rand des Lesers bzw. Paneels (`widgets::with_floating_rail`, `Panel::floating`); der Text der Mail hält
  `widgets::floating_rail_room()` Abstand, ebenso ihr Kopf (#545). Die Liste hat keine Knöpfe. Gruppen
  (`widgets::RailGroup`) in fester Reihenfolge Reply · File · Torro · Mark · Save, unten abgesetzt Mute · Display ·
  More (`RailGroup::foot`), jede nur mit dem, was für die Mail geht; was gerade nicht geht, steht aus (`RailItem` ohne
  Nachricht, 40 %, ohne `kbd`). **Feste Gruppen-Icons** (Reply, Check, ListTodo, Flag, Save, BellOff, Ellipsis), egal
  was das erste Item ist. **Save** (#501, Diskette): „Save as PDF“ und „Save as image“ öffnen je ein Menü (Downloads,
  Save as…, Ablageziele mit Pfad), „Save attachments (n)“ das Menü von „Save all“; eine Zeile, die ein Menü öffnet,
  endet statt `kbd` mit einem Chevron 12 in `TEXT_3` (`RailItem::opens_menu`). Im Kontextmenü der Liste stehen die
  beiden Formate als ein „Save mail as ›“ (`mail::context_groups`). **Reply** ist die Hauptgruppe (`RailGroup::accent`): eingeklappt eine gefüllte rote Fläche wie
  der Primärknopf, Icon in `on_accent`; ausgeklappt und im Klappfeld rotes Icon und Wort „Reply“ kräftig
  (`RailItem::lead`, `typo::body_strong()` in `TEXT`). KI-Sparkles nur als Icon in der Akzentfarbe
  (`RailItem::accent`). Ein Klick aufs Gruppen-Icon tut das erste Item, das geht – **nie ein destruktives**
  (`RailItem::danger`, Delete): ist es das, öffnet der Klick das Klappfeld. Mark zeigt eine markierte Mail am Icon
  (`RailGroup::tinted`, `attention`). More beginnt mit Copy text (ein Klick auf „…“ kopiert).
  Ein Icon je Gruppe (`size::control_lg()`, `space::xs()` Abstand); `SELECTED`, solange ihr Klappfeld, ein Menü/Dialog
  ihrer Items oder ihre Karte offen ist. Das Klappfeld (`widgets::rail_flyout`) öffnet erst, wenn der Zeiger **ca.
  150 ms** auf dem Icon ruht (die App hält Zeitpunkt und Timer, `widgets::RailEvent`); von einem offenen Feld zur
  Nachbargruppe sofort; Verlassen schließt sofort. Es liegt links neben dem Icon, für alle Gruppen gleich breit
  (`size::sidebar()`), Fläche und Rand wie ein Menü, Gruppenname als Abschnittstitel, Zeilen 30 mit Icon, Wort und
  `kbd`; die Zeile, die ein Klick aufs Icon auslöst, steht hervorgehoben. **Display** (`RailGroup::card`, Icon
  `contrast`) öffnet kein Klappfeld, sondern per Klick die Karte „Display“ (HTML/Text, Light/Dark/As app; die Wahl ist
  die Einstellung `mail_view` bzw. `mail_appearance`), sein Name steht im Tooltip. Die ausgeklappte Leiste
  (`RailMode::Expanded`), ihr Umschalter und die Taste `]` gibt es im Mail-Fenster nicht mehr. Einladungen (Accept,
  Maybe, Decline) bleiben als Karte im Kopf der Mail. Anschauen: `cargo run -p torro-gui --example gallery -- --mailbar` (dazu `--light`, `--design comfort`, `--design dense`).
- **Befehlsleiste und Leiste der Zeile** (E11.1, **mit #241 entfernt**): Die Befehlsleiste oben im Leser
  (`widgets::command_bar` mit `command_split`, `command_cell`, `command_more`, `command_words`), die Leiste der
  gewählten Zeile (`widgets::row_strip`, `row_strip_words`), die Torro-Pillen (`widgets::pill_button`) und der versenkte
  Ansicht-Schalter (`widgets::view_switch`) gibt es nicht mehr; das Mail-Fenster hat dafür die Aktionsleiste der Mail
  (oben). Geblieben sind die Anhänge als `widgets::split_file_chip` (32 hoch, Büroklammer in `TEXT_3`, Größe in
  `typo::meta()`) unten im Leser und das Suchfeld über der Liste `style::text_input_flat` (ohne Rand, Fokus bleibt
  sichtbar). Die Stile `style::button::cell`, `style::button::pill` und `style::container::track` stehen noch in
  `style.rs`, nutzt aber kein Baustein mehr. Anschauen: `cargo run -p torro-gui --example gallery -- --mailbar`.
- **Standard oben, Torro-Leiste unten** (E9.4, gilt nur noch für Zentrale und App Feed; das Mail-Fenster hat die
  Aktionsleiste der Mail, E14): Was jedes Programm dieser Art kann
  (Reply, Reply all, Forward | Archive, Delete; Flag, gelesen, Close als Icons), steht **fest oben** über dem
  Scrollbereich, mit Linie darunter; was Torro dazugibt (Erledigen · KI · Ansicht), **schwebt unten** als eigene
  Fläche über dem Inhalt. Knöpfe beider Leisten eine Stufe höher als in Kopfleisten: `widgets::bar_button`
  (`size::control()` 32, mit Wort oder quadratisch mit Tooltip), Schalter `widgets::bar_segmented`; mit Wort
  `secondary`, die Hauptaktion `primary`, ruhige Icons am Ende `style::button::icon`. Schwebende Leiste:
  `widgets::floating_bar` (Fläche `MENU`, Rand `LINE_STRONG`, Schatten, `radius::lg()`, Innenrand und Abstand
  `widgets::floating_bar_gap()` 4), mit `widgets::over_bar` unten mittig `space::m()` vom Rand; die Seite darunter
  bekommt unten `widgets::floating_bar_clearance(reihen)` als Polster, damit ihr Ende frei hervorscrollt. Gruppen
  mit senkrechtem Trennstrich; breit eine Reihe mit Wörtern, mittel zwei Reihen mit Wörtern, schmal Icons in so
  vielen Reihen wie nötig (nichts abgeschnitten, nichts im Menü versteckt). Menüs der Leiste öffnen über ihr
  (`Placement::Below` klappt um, wenn unten kein Platz ist). KI-Knöpfe tragen `Icon::Sparkles` in der
  Akzentfarbe (`widgets::bar_button_accent`) – Ausnahme von „Rot nur Primärknopf“, damit die KI als eigene
  Funktion erkennbar ist; nur das Icon, nie das Wort oder die Fläche. Im Paneel dieselbe Teilung, aber ohne
  schwebende Fläche: Standards oben im Inhalt, Torro-Funktionen im festen Fuß (`Panel::footer`).
- **Hinweis mit „Undo“** (E3.11, `widgets::toast`): nach einer Änderung, die sich zurücknehmen lässt
  (Done, Later, Hide …), unten mittig `space::xl()` über dem Fensterrand; Fläche `MENU`, Rand
  `LINE_STRONG`, Radius `radius::md()`, Schatten (`style::container::menu`), Text `typo::body()` in
  `TEXT`, höchstens ein Knopf (`widgets::ToastAction::undo`: ghost „Undo“ mit `kbd` „Ctrl Z“/„Cmd Z“),
  eine Zeile hoch `size::control_lg()`, höchstens `size::content_max()` breit. Ruhig: keine Animation,
  er erscheint und geht – nach `widgets::TOAST_FOR` (5 s) oder mit `Escape` (vor dem Paneel).
  `Ctrl+Z` (⌘Z) nimmt zurück, solange er „Undo“ zeigt (`widgets::undo_key`). Als eigene Ebene über
  dem Fenster stapeln, immer (leer ohne Hinweis), damit das Fenster seinen Zustand behält.
- **Menüs** (`widgets::menu`): Fläche `MENU`, Rand `LINE_STRONG`, Radius `radius::md()`, Schatten, Innenrand 4;
  Einträge 30 hoch mit Icon, Text und rechts Zahl oder Tasten-Hinweis; Abschnitte mit `typo::caption()`-
  Titel; Trenner `LINE`.
  Rechtsklick-Menüs (#497) öffnen am Mauszeiger, wie in macOS, Windows und GNOME: Die Zeile meldet die Stelle
  mit `widgets::context_area(inhalt, |punkt| …)`, das Menü hängt mit `Placement::At` an
  `anker.area(Rectangle::new(punkt, Size::ZERO))` (links oben am Zeiger, am rechten bzw. unteren Rand links bzw.
  über ihm; passt es auf keiner Seite, rückt es herein). Am Zeiger darf es über den Lesebereich reichen (etwa bei
  einem Klick nahe dem rechten Rand der Liste). Ohne Stelle hängt es mit `Placement::Below(Align::Start)` unter der Zeile, nie neben einer
  breiten Zeile über dem Lesebereich. Untermenüs daraus öffnen weiter an der Zeile. Im Kontextmenü aus der
  Leiste (`widgets::rail_context_menu`) tragen nur Gruppen mit mehreren Einträgen einen Titel; eine Gruppe mit
  einem Eintrag trennt der Strich, damit das Menü kurz bleibt und am Zeiger Platz findet.
- **Menü mit Suche** (#299, `widgets::search_menu(id, suche, on_input, on_submit, einträge, breite)`): für lange
  Auswahlen (die Schriften in Appearance, über 150 auf einem Mac). Karte wie ein Menü (`MENU`, `LINE_STRONG`,
  `style::MENU_INSET`, höchstens `size::panel()` hoch, darunter rollt es), oben ein flaches Suchfeld
  (`style::text_input_flat`, Icon `Search` in `TEXT_3`, Platzhalter „Search“, `size::control()` hoch), darunter eine
  `LINE`, darunter die Einträge des Aufrufers. Der Aufrufer filtert selbst (ohne Groß/Klein) und lässt Titel ohne
  Treffer weg; ohne Treffer steht „No matches“ in `TEXT_3`. Beim Öffnen fokussiert er das Feld (`id`), Enter wählt
  den ersten Treffer (`on_submit`), Escape schließt. Anschauen: `cargo run -p torro-gui --example gallery -- --blocks`.
- **Auswahl** (#529, gilt in jeder App und in den Einstellungen): Wie viele Optionen es gibt, entscheidet den
  Baustein, nicht der Platz.
  - **Bis etwa 4 Optionen:** `widgets::segmented` (eine Ansicht derselben Daten) oder Chips
    (`widgets::choice_chip`, auch für mehrere zugleich), alle Optionen sichtbar.
  - **Mehr als etwa 4:** ein Dropdown (`widgets::select_button` mit `widgets::menu`), der gewählte Wert steht im Knopf.
  - **Ab etwa 12:** ein Dropdown mit Suche (`widgets::search_menu`, oben).
  - **Der Baustein dafür** (#554): `widgets::Dropdown<K>` im State der Seite bzw. App (welches Menü offen ist, die
    Suche, ein Anker je Auslöser) mit der Nachricht `DropdownEvent<K>` (`Toggle`, `Query`, `Close`); Auslöser
    `Dropdown::button` bzw. `button_fill` (ein `select_button`), das Menü `Dropdown::view` im Overlay (über einem
    Dialog im selben Overlay darüber). `widgets::choice_menu` entscheidet selbst: unter `widgets::SEARCH_FROM` (12)
    Einträgen ein Menü, das ab `size::panel()` rollt, ab 12 ein `search_menu` (Filter ohne Groß/Klein, Abschnitte ohne
    Treffer fallen weg, Enter wählt den ersten Treffer). Öffnet `Dropdown::update` ein Menü (`true`), fokussiert der
    Aufrufer `widgets::DROPDOWN_SEARCH`; jede Wahl und Escape schließen. Werte ohne Icon als `MenuItem::plain`, der
    gewählte mit `.current(true)`. **Iceds `pick_list` gibt es in der Suite nicht** (`scripts/design-check.sh` meldet
    es), auch nicht über `parts`.
  - **Keine langen Zeilenlisten zum Anklicken** für eine Einzelauswahl (etwa die Chat-Modelle eines Anbieters oder
    eine Liste von Kalendern, aus der einer gewählt wird). Listen mit Zeilen sind für Dinge, die jedes für sich
    etwas tun (an/aus, öffnen, entfernen), nicht für „eins davon“.
- **Schwebende Karte** (`widgets::popover`): 320 breit, `MENU`, Radius 10, Innenrand 16, Kopf mit
  Kalender/Liste, Titel `typo::title()`, Eigenschaftszeilen, unten Aktionen.
- **Icons** nur aus `torro_gui::icon` (Lucide-SVG), nie Unicode-Glyphen wie ⟳ ‹ › ✓.
  Für GitHub (R.1): offen `CircleDot`/`GitPullRequest` in `ok`, Entwurf `GitPullRequestDraft` neutral,
  gemergt `GitMerge` bzw. geschlossenes Issue `CircleCheck` in `merged`, geschlossener PR
  `GitPullRequestClosed` in `error`; Checks `CircleDotDashed` (läuft, `attention`), `CircleCheck`
  (`ok`), `CircleX` (`error`), `CircleDashed` (wartet, `text_3`); Review angefordert `Eye`
  (`attention`). Hinweise in Zeilen: `ListChecks` (Unteraufgaben), `AlignLeft` (Notiz), `Repeat`,
  `Bell`; `GripVertical` (Ziehgriff), `SignalHigh` (Priorität im Menü), `ChevronUp`/`ChevronDown`
  (vorige/nächste Aufgabe). Lucide 0.469.0 liefert `Github` noch mit.
  Im Eingang (E8.10): `Bell` nur für Benachrichtigungen (Fensterart, Spalte „Notifications“), die Glocke
  zum Stummschalten („Mute…“ an Karte, Zeile und Paneel, „Unmute“ im Menü, Vorschläge in Insights) `BellOff`; Spur „Info“ `Info`
  (nicht `Eye`, das ist „Review angefordert“); „Insights“ `ChartColumn`; „Table view“ `Table`.
  Quellen im Eingang (E5.15, `list::source_icon`, nur nach der Art der Quelle, nie nach dem Namen eines
  Skripts): GitHub `Github`, Nextcloud `Cloud`, Talk `MessageCircle`, ntfy `RadioTower`, Skripte `AppWindow`.
  Das Konto steht nur bei zwei Konten derselben Art im Namen („GitHub · anna-berg“, `list::source_name`);
  die Metazeile nennt die Merkmale („torro/workplace · Review requested“, `list::facet_line`). Ein ntfy-Alarm
  (dringend) bekommt die Kachel in `attention` getönt und das Wort „Urgent“, nie Torro-Rot. Ein neues
  Icon: SVG aus Lucide 0.469.0 mit `stroke-width="1.75"` nach `assets/icons/`, Variante in `Icon` samt
  Eintrag in `Icon::ALL` – die Tests in `icon.rs` prüfen Datei, Strich und Namen.
- **Diagramme und Kennzahlen** (E8.8b, zuerst die Insights der Zentrale; Methode: Skill `dataviz`): Farben nur als
  `theme::ChartColor` (`Series(0..4)` in fester Reihenfolge, `Other`), in der Stilfunktion mit
  `palette(t).chart_color(…)` – eine Reihe bekommt ihre Farbe nach dem, was sie ist, nie nach dem Rang, eine fünfte wird
  „Other“. Bausteine: `widgets::stat_tile(label, wert, notiz)` (Kennzahl in `typo::display()` tabellarisch, `RAISED`,
  Rahmen `LINE_STRONG`, Radius `lg`), `widgets::chart_card(titel, notiz, inhalt)` (auf `PANEL`, der Fläche, gegen die die
  Farben geprüft sind), `widgets::column_chart(reihen, säulen)` (gestapelt, 2 px Fläche zwischen Segmenten, obere Ecken
  `radius::xs()`, Gitter `LINE`, Grundlinie `LINE_STRONG`, Achsen `typo::micro()` in `TEXT_3`, Tooltip über die ganze
  Säulenhöhe mit Titel, Summe und jedem Wert), `widgets::chart_legend` (ab zwei Reihen), `widgets::share_bar` (Anteil
  neutral in `TEXT_3`, kein Status) und `widgets::data_table(kopf, zeilen, summe)` für die Tabellenansicht – jedes
  Diagramm bietet dieselben Zahlen als Tabelle. Nie zwei y-Achsen, keine Zahl an jeder Säule.
- **Kleine Bausteine der Aufgaben-App** (`widgets`, R.1; Beispiele im Doc-Kommentar, Anschauen mit
  `cargo run -p torro-gui --example gallery -- --parts`): `prop_line(label, mark, value, on_press)` –
  Eigenschaftszeile, Bezeichnung 88 in `typo::meta()`/`TEXT_3`, Wert daneben (`prop_value`, `prop_placeholder`
  für „nicht gesetzt“ in `TEXT_3`), ganze Zeile ein Knopf, mind. 32 hoch; `label_chip(hex, name)` –
  GitHub-Label, 20 hoch, Radius 4, Farbe aus `#RRGGBB` (auch ohne `#`) **nur als Punkt**,
  `label_chip_row` umbrechend mit Abstand 4; `state_pill(mark, wort)` – Zustand mit Wort, 22 hoch,
  rund, Farbe nur im Icon; `progress_bar(done, total)` – 64×4, Spur `LINE_STRONG`, Füllung `ok`,
  daneben immer „2/4“; `kbd(taste)` – Tastenhinweis (gab es schon).
- **Seitenleiste:** Zeilen `size::row_sidebar()` (32 im Standard) mit Icon 16, Text `typo::body()` in `TEXT_2` (aktiv `TEXT` auf `SELECTED`),
  Zahl rechts `typo::meta()` tabellarisch in `TEXT_3`, Tasten-Hinweis als `kbd`. Gruppen „Lists ▾“ als
  `typo::caption()` in `TEXT_3` mit 12 Abstand darüber. Listen mit Ring (Aufgaben) bzw. Punkt (Kalender).
  Ein Zustand an einem Eintrag (ein Anbieter, dessen Konto Aufmerksamkeit braucht oder einen Fehler hat) ist
  ein `Mark::Badged(icon, farbe)`: das eigene Icon im Ton der Zeile mit einem Punkt 6 in der Statusfarbe auf
  der Ecke oben rechts, gleich in Spalte, Leiste und Chip – nie nur ein getöntes Icon (#248).
  Eine Gruppe mit Unterpunkten (die Kategoriegruppen der Mail, #300) ist `widgets::sidebar_fold_in`: Zeile wie
  jede andere, Chevron 12 in `TEXT_3` direkt hinter dem Namen (die Icon-Spalte bleibt bündig), in der Leiste
  ohne Chevron. Ihre Kinder stehen offen in `widgets::sidebar_branch` als `widgets::sidebar_child`: um
  `icon::SIZE` + `space::s()` (24) eingerückt, Icon `icon::SIZE_SM` (14), eine 1-px-Linie in der Farbe der
  Gruppe mittig unter deren Icon; die Leiste zeigt keine Kinder. Im Menü sind Kinder `MenuItem::nested()`
  (gleich eingerückt, kleineres Icon, ohne Linie).
  Eine zweite, leisere Zeile unter dem Namen (gepinnter Ordner mit der Adresse seines Kontos, #398) gibt
  `widgets::sidebar_item_detail_in`: Icon, Name und Zahl auf der ersten Zeile, die Unterzeile `typo::meta()` in `TEXT_3`
  darunter, eingerückt bis zum Namen (#476), beide Zeilen mit „…“ gekürzt, damit die
  Seitenleiste nicht breiter wird; die Zeile ist um eine Metazeile höher; in der Leiste steht die Unterzeile im Tooltip.
- **Aufgabenzeile:** `size::row()` hoch (48), Kreis 16 (abgehakt gefüllt in Listenfarbe mit Häkchen), dahinter das
  **Symbol für Priorität und Fokus** (#596, `widgets::focus_mark`, Zelle 28, alle Mittelpunkte auf einer Achse), Titel
  einzeilig, rechts Listen-Etikett (Pille mit Punkt; GitHub-Einträge `widgets::icon_pill(Icon::Github, repo)`) und
  Fälligkeit `typo::meta()` tabellarisch (überfällig Attention-Orange). Gruppenköpfe `size::row_group()` (40) auf `RAISED`
  mit Icon, Name, Zahl und „+“. **Fokus** gewichtet die Zeile: mit offenen Teilaufgaben eine zweite Zeile
  „Next: … · 2/5“ (`typo::meta()`, `TEXT_2`/`TEXT_3`, unter dem Titel eingerückt); starker Fokus ist eine Karte in der
  Zeile (`RAISED`, Rand `LINE`, `radius::sm()`, Innenrand `space::m()`, ihr Kreis auf der Achse der anderen), mit „Next“,
  bis drei Teilaufgaben (schmal zwei, „+n more“) und `widgets::progress_bar`; die Teilaufgaben stehen dann nur in der
  Karte (in „Manual“ bleiben sie Zeilen). Der Kopf „Today“ zeigt die belegten Fokus-Plätze (`widgets::slot_dots`) und
  „To tomorrow“ (ghost). In der Ansicht nach Fälligkeit tragen offene Hauptaufgaben schreibbarer Listen den Griff; die
  Marke steht nur am eigenen Tag oder auf den Köpfen „Tomorrow“ und „Later“.
- **Priorität und Fokus** (#596, Entwurf `docs/prototypes/tasks/fokus.html`): **Farbe = Priorität** aus `Palette::priority`
  (keine grau, niedrig hellgelb, mittel gelb, hoch orange, dringend rot; hell nachgedunkelt auf ≥ 3:1 gegen `PANEL` und
  `RAISED`, nur Marke, nie Text), **Form = Fokus**: Punkt Ø 9 (normal), Ring Ø 18 Strich 4 (Fokus), Ring Ø 28 Strich 5,5
  (starker Fokus). Der Tooltip sagt beides in Worten („High · Focus“), Farbe allein trägt nichts. Gruppenkopf, Chip,
  Eigenschaft und Menü zeigen die Priorität als Punkt (`widgets::priority_dot`) mit Wort. Die Abbildung bleibt eine
  (`torrotasks::list::Prio`): iCalendar 1 = Urgent, 2–4 High, 5 Medium, 6–9 Low, sonst keine; gespeichert werden
  1 / 3 / 5 / 9. In der gefüllten Auswahlzeile (`fill`) sind Symbol und Punkt `on_accent`.
- **Werte, die nicht passen,** (Eigenschaften, Chips, Titel) werden mit „…“ gekürzt
  (`typo::ellipsize`, in `prop_value` eingebaut) und zeigen den ganzen Wert als Tooltip – nie
  stumm abgeschnitten.
- **Termine:** Fläche Kalenderfarbe 18 % (hell 14 %) über `PANEL`, 3-px-Balken links in voller
  Farbe, Titel 12/500 in `TEXT`, Zeit `typo::micro()` in `TEXT_2`, Radius 4, kein Rahmen; kurze Termine
  einzeilig „09:30 Titel“; unbestätigt gestrichelter Rahmen ohne Fläche; Aufgaben im Raster als
  Umriss mit Kreis; vergangene 55 % Deckkraft; freie Zeit als „2 h free“ in `TEXT_3`.
  **Heute** = roter Kreis mit weißer Zahl; **Jetzt** = rote Linie 1 px mit Punkt und Zeitlabel in
  der Achse. Die Heute-Spalte der Woche ist **neutral** abgehoben (`SELECTED` über dem Grund, #543), nie rot getönt.
  In der Leiste steht der Titel eines zu kurzen Termins neben dem Block auf einer Platte in `PANEL`: Stunden- und
  Jetzt-Linie enden an der Schrift, die Platte endet mit dem Text und vor dem nächsten Block (#543).
- **Einstellungsseiten:** links im Inhalt ein **Untermenü** mit den Abschnitten der App (Host der Einstellungen,
  `docs/settings.md` „Wie es aussieht“), rechts die Seite. Jede Seite mit **mehr als drei Gruppen** bekommt Reiter
  (`widgets::tabs`, unten), nicht nur Detailseiten; bis drei Gruppen stehen sie untereinander. Gruppen derselben Art
  je Konto (die Kalender bzw. Listen eines Kontos, #543) zählen dafür nicht: Sie sind eine Liste, die mit den Konten
  wächst, keine Teile der Seite. Abschnittsseiten aus
  `widgets::group` (Titel, Zeilen mindestens `size::header()` bzw. 12 höher mit
  Untertitel, Trennlinien, erklärender Satz darunter in `TEXT_2`) – keine eigenen Karten. Der Titel einer Zeile bleibt
  einzeilig mit „…“, ihre erklärende Unterzeile bricht um, und die Zeile wächst dann mit `space::s()` oben und unten
  (`parts::setting_row`, #543). **Jede Seite beginnt mit dem
  Titel ihrer ersten Gruppe** (#554), ohne eigenen großen Titel oder Unterzeile darüber: Die Kopfzeile nennt die Seite
  schon. Auf Accounts ist der Anbieter der Gruppentitel, sein Satz steht unter der Gruppe; auf AI heißt die Gruppe
  „Accounts“, damit Kopfzeile und Gruppe nicht beide „AI“ sagen. Im schmalen Fenster zeigt die Kopfzeile den Abschnitt
  als Menüknopf („‹ [🔔 Reminders ▾]“) wie in Mittel; hat die Seite eigene Knöpfe, nur Icon und Chevron, der Name
  steht im Tooltip. Eine sichtbare Aktion je Zeile,
  der Rest im „…“-Menü. Was wohin gehört (App oder Steuerzentrale, auch die Rechte für API und MCP je App), regelt
  `docs/settings.md`, Abschnitt „Die Regel“; der Skill wiederholt es nicht. **Detailseiten** (dritte Ebene, #314, `docs/settings.md`) folgen einem festen Aufbau aus
  diesen Bausteinen (Anschauen: `cargo run -p torro-gui --example gallery -- --detail`, dazu `--light`,
  `--design comfort`, `--design dense`):
  - `detail_head(lead, title, meta, status, actions)`: Kachel `TileSize::Large`, Name `typo::title()`, Meta-Zeile
    `typo::meta()`/`TEXT_3` mit „…“ und Tooltip, rechts eine Status-Pille (`Status`, Farbe nur im Punkt) und
    `HeadActions` (eine `PageAction`, `secondary`, dann „…“, dessen Menü der Aufrufer mit `floating` öffnet). Unter
    `size::form_max()` rutschen Pille und Knöpfe unter die Meta-Zeile.
  - `tabs(labels, selected, on_select)`: Reiter mit Unterstrich, `size::control_lg()` hoch; gewählt `TEXT`,
    `body_strong`, Strich 2 px in `TEXT`, sonst `TEXT_2`; Zahl in `meta`/`TEXT_3`; eine `LINE` unter der Reihe. Passt die
    Reihe nicht (`tabs_width`), lässt sie sich seitlich schieben (#554), mit einem Verlauf `space::l()` breit am
    rechten Rand, der zeigt, dass mehr folgt; liegt der gewählte Reiter jenseits der Breite, steht die Reihe am
    rechten Ende. Nie eine Auswahlliste. Nie Rot, nie `segmented`
    (`segmented` wechselt die Ansicht derselben Daten, Reiter den Teil eines Objekts oder einer Seite). Erst ab mehr als
    drei Gruppen, dann aber auf jeder Seite; Gruppen derselben Art je Konto zählen nicht (#543).
  - `question_card(mark, title, detail, actions)`: was auf eine Entscheidung wartet, direkt unter den Reitern;
    `RAISED`, Rand `LINE_STRONG`, Radius `sm`, Icon in Attention, Knöpfe rechts oder darunter; mehrere mit
    `size::card_gap()`.
  - `stat_grid(Vec<StatFigure>)`: 2–4 `stat_tile` mit Bezug („59 unread“, „of 10 GB · 24%“) in einer Reihe, sonst zwei,
    in Mini eine (`stat_columns`).
  - `share_strip(parts, rest)` mit `SharePart`: Teile eines Ganzen (Speicher, Index-Zustände) als Balken `space::s()`
    hoch, 2 px Fläche dazwischen, Rest („Free“) als Spur in `LINE_STRONG`, Legende mit Wort, Wert und Anteil
    (`share_percent`: „24%“, „<1%“), höchstens vier Reihen und Other. `share_bar` bleibt für einen neutralen Anteil.
  - `setting_pair(title, why, control)` in `setting_pairs`: links Titel `body_strong` und Warum-Satz `meta`/`TEXT_2`,
    rechts das Bedienelement (1 : 1.6, die Wörter höchstens `size::content_max()`, das Bedienelement den Rest), unter
    `size::form_max()` untereinander; Paare mit `LINE` getrennt.
  - `save_bar(summary, discard, save, busy)`: nur auf Seiten, die nicht selbst speichern, und nur solange etwas
    geändert ist; im Einstellungs-Host über `Page::save_bar`.
  - `sidebar_branch_quiet(rows)`: das offene Objekt als Kind seines Abschnitts, Linie `LINE_STRONG`.
  - Formate: Größen `torro_gui::units::format_bytes` (Basis 1024, drei gültige Ziffern, „60.9 MB“, „10 GB“), Zahlen
    `format_count` („4,796“ / „4.796“), Zeitpunkte `torro_gui::datetime::format_moment` („Today 15:39“, „5 Oct 2020“,
    Ortszeit) mit dem genauen Wert `format_exact` als Tooltip. Keine eigenen `MiB`- oder `UTC`-Formate.
- **Ansichten:** **leer** (`empty_state`: was hier erscheint und was zu tun ist), **lädt**
  (`loading()`, ruhiger Platzhalter, kein Springen), **Fehler** (`error_state`: was passiert ist +
  „Retry“), **voll** (lange Titel, viele Einträge, Überschneidungen).
- **Formulare:** Fehler erst nach dem ersten Speicherversuch oder beim Verlassen des Feldes,
  nie vor der ersten Eingabe. Speichern bleibt aktiv und springt zum ersten Fehler.
- **Ruhige Zeilen** (#627, nach der Sichtprüfung des Schreibfensters entschieden; gilt für jedes Formular aus Zeilen
  wie Von/An/Betreff): Ein Formular wirkt nur ruhig, wenn alles darin gleich gebaut ist.
  - **Eine Höhe:** Jede Zeile ist `size::control()` hoch, Bezeichnung und Wert stehen senkrecht mittig auf derselben
    Linie (Eingaben bekommen oben und unten so viel Innenrand, dass ihr Text mittig sitzt). Wächst eine Zeile (Chips
    über mehrere Reihen), ist jede Reihe wieder so hoch.
  - **Eine Achse:** Alle Werte beginnen an derselben Kante hinter der Bezeichnung (`label_width` + `space::s()`): Text
    einer Eingabe ohne seitlichen Innenrand, Chips, Auswahl.
  - **Keine Rahmen in der Zeile:** Zeilen haben eine Haarlinie, sonst keine Kästen. Eine Auswahl ist Text mit einem
    Chevron 14 in `TEXT_3` (`style::button::bare`), kein gerahmter `select_button`; Knöpfe sind `ghost` oder Icon-Knöpfe.
    `bare` hat bewusst keine Hover-Fläche (Ausnahme von „fünf Zustände“): Chevron und Zeiger zeigen, dass er öffnet.
  - **Was zu einem Ding gehört, steht darin:** Das × eines Chips liegt in der Pille (`widgets::chip_removable`), nicht
    daneben.
  - **Werkzeuge an einer Stelle:** Was den Inhalt bearbeitet (Formate, Anhang, Bild), steht als eine Zeile direkt über
    dem Inhalt; unten bleiben nur die Abschlüsse (Senden, Verwerfen). Ein Schalter, der eine Leiste ein- und
    ausblendet, ist ein schlichter Icon-Knopf mit einem Tooltip, der sagt, was ein Klick tut – kein gewählter Rahmen.
  - **Fokus im Schreibfenster:** Die Zeilen und der Text des Schreibfensters zeigen keinen Fokusrahmen; die
    Bezeichnung der Zeile mit dem Cursor wird `TEXT`, sonst `TEXT_3`, und der Cursor zeigt, wo man schreibt
    (Ausnahme von Abschnitt 4, entschieden in #627).
- Destruktive Aktionen: rückgängig machen anbieten oder bestätigen lassen, nie beides nötig.

## 6. Responsiv für Kachel-Fenster – von winzig bis riesig

Torro-Apps laufen **dauerhaft** als Kacheln in gespeicherten Workspace-Vorlagen, oft viele
nebeneinander. Beispiel beim Nutzer: ein 32:9-Monitor mit 7680×2160 bei Skalierung 2, also
**3840×1080 logische Pixel**. Jede Ansicht muss in **jeder** Größe gut aussehen – nicht nur
funktionieren.

**Größenklassen** (logische Pixel), geprüft in dieser Reihenfolge – `torro_gui::layout::of(size)`
(K.18), nie eigene Schwellen in Views:

| # | Klasse | Bedingung | Rand | Chrom | Beispiel Kalender / Aufgaben |
|---|---|---|---|---|---|
| 1 | **Leiste** | Höhe < 280 und Breite ≥ 600 | 8 senkrecht, 12 waagerecht | keins | Zeitstrahl von heute / Chips |
| 2 | **Mini** | Breite < 360 oder Höhe < 280 | 12 | keins | nächster Termin + „free until 14:00“ / Zahl offener + nächste 3 |
| 3 | **Schmal** | Breite < 600 | bündig | Kopf 64, Navigation im Menü | ein Tag mit Wochenleiste / Liste, Detail ersetzt die Liste |
| 4 | **Mittel** | Breite < 1024 | bündig mit Linie (`inset` 0) | Leiste 60 + Kopf 64, Navigation im Menü | Woche + schwebende Karte / Liste 364 **neben** dem Detail, Eigenschaften als Chips unter dem Titel |
| 5 | **Breit** | sonst | bündig mit Linie (`inset` 0) | Seitenleiste 240 (oder Leiste 60) + Kopf 64 + Eigenschaften 300 | Woche + Eigenschaften / Liste 440 + Detail + Eigenschaften-Spalte 300 |

**Aufgaben-App** (Entwurf `docs/prototypes/tasks/redesign.html`, R.1–R.4): ohne offene Aufgabe steht
die Liste allein links (max. `size::content_max()`). Mit offener Aufgabe: **Breit** ab Inhaltsbreite 1320
drei Spalten – Liste `size::list()` 460 | Detail (Text max. 640, Innenrand 32) | Eigenschaften `size::props()` 300
(Blöcke durch `LINE` getrennt: Status, Priority, Due · „Organisation“ · „Source“), je eine `LINE`
dazwischen; schmaler wie Mittel. **Mittel:** Liste `size::list_medium()` 380 neben dem Detail, die Eigenschaften
als Chips unter dem Titel; bekommt das Detail daneben weniger als `size::props()` (Inhalt unter 683, also
Fenster unter etwa 740), ersetzt es die Liste wie in Schmal („‹ Today“, ↑↓). **Schmal:** Detail
ersetzt die Liste. Neben dem Detail verkürzen sich die Zeilen nach der Breite der Liste (erst Labels,
dann Hinweise, Pille wird Punkt, `repo#123` wird `#123`). Das Detail hat **keinen roten Fuß** und
keinen Speichern-Knopf (speichert selbst); Kopf: Pfad, ↑ ↓, „…“ (Copy link, Open on GitHub,
**Delete task** – nur dort, mit Rückfrage, bei GitHub-Einträgen gar nicht), Schließen.

Regeln:

- **Das Layout wechselt, es schrumpft nicht.** Pro Klasse bewusst entscheiden, was gezeigt wird.
  Schrift nie kleiner als 12, Klickziele nie kleiner als 24.
- Reihenfolge beim Platzsparen: Seitenleiste einklappen (Mittel: Leiste mit Icons, Schmal: Overlay), Werkzeugleiste auf Icons mit
  Tooltip, Nebeninfos in eine zweite Zeile oder weglassen. Nie abschneiden, nie überlappen, nie
  waagerecht scrollen.
- **Mini und Leiste sind Anzeigen, keine Mini-Apps:** keine Werkzeugleiste, keine Formulare, keine
  Dialoge. Höchstens eine Ein-Klick-Aktion, die den Zweck der Kachel ist (z. B. abhaken). Ruhig,
  aus der Distanz lesbar, Kopfzahl in `typo::display()`.
  **Ausnahme Assistent** (#547, entschieden): Mini und Leiste behalten ihr eine Eingabefeld, weil schnelles
  Fragen der Zweck der Kachel ist – ein Feld, kein Formular, keine Menüs. In der Leiste stehen Feld (unten)
  und letzte Zeile der Antwort bzw. Leertext auf einer Mittellinie.
- **Mini und Mini hoch auf dem Mac** (#554, entschieden): Oben bleibt der Streifen der Ampel frei (die Höhe der oberen
  Zeile, Mitte bei y = 20). Der Leerraum dort ist kein Befund; im Bild von `shots.sh` fehlt nur die Ampel selbst.
- **Groß heißt nicht gestreckt:** Leselisten (Mail, Aufgaben, Chat) und Fließtext max. `size::content_max()`,
  Formulare `size::form_max()`, zentriert bzw. links mit Seitenleiste; zusätzlicher Platz geht an Inhalt (mehr Tage,
  Detail daneben, mehr Spalten), nicht an Leerraum zwischen Elementen.
- **Einstellungs-, Übersichts- und Kennzahlenseiten** (#464, #529: Host der Einstellungen, Steuerzentrale,
  Mail-Übersicht, Insights, Detailseiten) haben **keine Obergrenze**: Die Seite steht bündig neben der Seitenleiste
  (Rand `page_margin`) und füllt die ganze Breite; nur Fließtext und Bedienelemente bleiben bei `content_max()`.
  Statt eines festen Maßes brechen die Inhalte nach Art um:
  - **Zeilen** (`widgets::group_row`, `parts::setting_row`): Zeile und Trennlinie über die ganze Breite, der Text
    wächst höchstens bis `size::content_max()` (`widgets::reading_cell`), das Bedienelement steht am Zeilenende in
    seiner eigenen Breite – Schalter, Auswahl und kurze Felder werden nie gestreckt, lange Felder und Editoren
    füllen ihre Spalte. Der erklärende Satz unter einer Gruppe bricht ebenfalls bei `content_max()` um.
  - **Listen und Tabellen** (`widgets::settings_table`, Konten, und ebenso `widgets::data_table` in Insights und
    Übersichten): volle Breite, die Namensspalte bekommt den Zuwachs, die Wertspalten bleiben so breit wie bei
    `size::content_max()`.
  - **Karten, Kennzahlen, Kacheln** (`stat_grid`): ein Raster, das mit der Breite mehr Spalten bekommt.
  - **Unabhängige Blöcke** einer Seite (`widgets::page_columns`): ab 2 × `size::form_max()` + `layout::section_gap()`
    zwei Spalten, ab 3 × `form_max()` + 2 × `section_gap()` drei, nie mehr Spalten als Blöcke; die Blöcke der Reihe
    nach, oben bündig. Gefahrenzone („Remove…“, „Revoke…“) steht immer einspaltig darunter. So umbrechen
    Darstellung, Allgemein und Workspaces der Steuerzentrale, die Seiten eines Kontos, eines Programmschlüssels und
    eines MCP-Clients, Mail „Lesen und Schreiben“, Übersicht und Sicherung eines Mail-Kontos, die Übersicht des Mail-Fensters
    („What comes in“ und „What's new“, #545) und Allgemein des Assistenten; alle anderen Seiten bleiben einspaltig über die ganze Breite.
  - Schmal und Mini, Dialoge (`form_max`) und Assistenten bleiben, wie sie sind.
- Zustand bleibt beim Größenwechsel erhalten (Auswahl, Scrollposition, Datum, gewählte Ansicht –
  in Schmal wird nur die *dargestellte* Ansicht angepasst).
- **Dauerbetrieb:** nichts blinkt oder bewegt sich ohne Anlass; neu zeichnen nur bei Änderung bzw.
  im Minutentakt; keine Spinner im Ruhezustand; geringe CPU- und GPU-Last.
  **Arbeitsfeedback** (#422, #560): Läuft ein Sync wirklich (oder wartet sein Lauf), drehen sich die Pfeile von
  „Syncing…“ (`PageAction::spinning`, `widgets::spin_angle`, ein Umlauf je 1,2 s); der Knopf nimmt dann keinen
  Klick. Nur dafür und nur solange: Der Takt zum Neuzeichnen (50 ms) läuft allein in dieser Zeit, danach steht
  alles still. Dasselbe gilt für sichtbare tatsächlich laufende Speicheraktionen; bei Dialogwahl,
  Erfolg, Teilerfolg, Fehler oder Abbruch läuft kein Animationstimer. Geschlossene Menüs brauchen
  keinen Speichertakt; ihre aktiven Auftragssperren bleiben erhalten. `busy_button` bleibt ruhig.
- **HiDPI:** alle Maße logisch; Icons als Vektor; Linien 1 logischer Pixel.
- Mindestgröße der Fenster 200×120 (`torro_gui::window`), damit Mini und Leiste möglich sind.
- Fenstertitel und Inhalt ohne Doppelungen; keine Fake-Titelleiste.

## 7. Texte

- Englisch zuerst, alles durch `tr()`/`trf()`. Satzanfang groß, Rest klein („New event“).
- Knöpfe mit Verb („Save“, „Delete event“), nicht „OK“/„Yes“.
- Fehlertexte ohne Schuld und ohne Fachjargon; sagen, was der Nutzer jetzt tun kann.
- Kurz. Wenn ein Wort reicht, kein Satz.

## 8. Bewegung

- Wenn Animation, dann 120–200 ms, ease-out, nur für Orientierung (öffnen, verschieben). Nie
  Pflicht, um etwas zu verstehen.
- Arbeitsanzeigen sind die Ausnahme: Nur während tatsächlicher Arbeit und nur sichtbar animieren
  (Abschnitt 6). Erfolg, Teilerfolg, Fehler und Abbruch stehen still; Dialogwarten ist keine Arbeit.

## 9. Sichtprüfung (Pflicht bei UI-Änderungen)

Tests reichen bei Oberfläche nicht. Jede geänderte Ansicht in **allen Größenklassen** ansehen.
Aktionsfeedback immer mitprüfen: laufend, bestätigter Erfolg, Fehler, Teilerfolg und Abbruch.
Für das Speicher-Untermenü gibt es in `torroinbox` die nebenwirkungsfreien Demo-Zustände
`save-running`, `save-success`, `save-error`, `save-partial`, `save-choosing`, `save-cancelled`,
`save-shortened`. Der Demo-Spinner hat einen festen Winkel ohne Timer; echte Dialoge und Bewegung
zusätzlich von Hand prüfen. Alle drei Abstandsstufen sowie Hell/Dunkel sind Pflicht.

**Der Weg auf jedem System: `shots.sh`, ohne Fenster (#492).** Die App zeichnet ihre Demo-Ansicht im
Speicher (`--demo --shots <ordner>`, `torro_gui::shots`) und legt sie als PNG ab: kein Fenster, kein
Symbol im Dock, kein Fokuswechsel, kein Hyprland. Die Arbeit des Nutzers wird nicht unterbrochen.

```bash
.claude/skills/torro-design/shots.sh torrocalendar "$SCRATCH/design" week editor -- --appearance dark
.claude/skills/torro-design/shots.sh torrocalendar "$SCRATCH/design" week editor -- --appearance light
```

`shots.sh <app> <ordner> [<zustand> …] [-- <parameter> …]` baut die App (`torrocalendar`,
`torrotasks`, `torroinbox`, `torroassistant` oder `torroworkplace`; für die Steuerzentrale setzt es
`ui` davor) und startet sie je Zustand einmal mit `--demo --open <zustand> <parameter> --shots
<ordner>`, ohne Zustand einmal ohne `--open`. Jeder Lauf schreibt sechs PNGs: Mini (240×160), Mini
hoch (320×480), Leiste (1200×200), Schmal (480×900), Mittel (800×600) und Breit (1600×1000), als
`<präfix>-<größe>-<B>x<H>.png`, z. B. `torrocalendar-week-dark-mini-240x160.png`. Das Präfix ist der
Programmname mit den Werten von `--view`, `--section`, `--settings`, `--open`, `--design`, `--font`,
`--color-scheme` und `--appearance`, danach der Flags, die ein Fenster, einen Eintrag oder eine Darstellung
wählen (`--chat`, `--bot`, `--lane`, `--source`, `--template`, `--task`, `--message`, `--reply`,
`--reply-all`, `--forward`, `--draft`, `--account`, `--step`, `--mail-view`, `--mail-colors`, `--density`,
`--now`), je mit dem ersten Wert, und `-setup`, `-setup-if-open` bzw. `-saved` bei den gleichnamigen Schaltern (Schreibfenster: `torroinbox-compose`, Vorlagen-Dialog:
`torroworkplace-template`). Jede Größe zeichnet ein eigener Prozess, damit keine Größe den Demo-Stand
der nächsten verändert (z. B. eine beim Öffnen gelesene Mail). Die Bilder haben die **doppelte Auflösung** wie ein
Retina-Bildschirm: Mini (240×160 logisch) ist ein Bild von 480×320 Pixeln. Gleichnamige Bilder werden
überschrieben, jeder Pfad steht auf stdout. Eigene Größen gibt `-- --shot-size 1000x700` (mehrfach, je
Kante bis 4096; ersetzt die sechs, Datei `<präfix>-1000x700.png`). Der erste Fehler bricht mit seinem Exit-Code ab
(64: unbekannter Parameter oder Wert).

- **Immer beide Erscheinungsbilder:** je Ansicht `--appearance dark` und `--appearance light`. Ohne
  Fenster kennt die App das Hell/Dunkel des Systems nicht.
- **Nicht im Bild:** die Fensterhülle (Ampel, Titelleiste), Hover-Zustände, die Mindestgröße des
  Fensters (jede Größe wird gezeichnet, auch eine, die das Fenster nicht zuließe), die Erinnerung über
  den ganzen Bildschirm (`--reminders`, ein eigenes Fenster je Bildschirm; mit `--shots` lehnt der
  Kalender ihre Zustände mit Exit 64 ab) und Schriftwechsel zur
  Laufzeit (eine Schrift, die erst nach dem Start geladen wird, zeigt eine Ersatzschrift). Dafür bleibt `shoot.sh` (unten).
- **Mail-Kopf (#564):** Kopf und Inhaltstrenner nach Ziehen der Sidebar-/Listenbreite auf einer
  Achse prüfen; ausgeklappt, Icon-Leiste, Drawer, Einstellungen und Fokusansicht ansehen, in
  allen Abstandsstufen hell/dunkel sowie mit eigener dunkler Sidebar im Hellmodus (z. B. `channelr`).
  Schalter und `[` müssen weiter umschalten. Native Ampel-Abstände und Fensterziehen bleiben
  ohne echtes Fenster ausdrücklich ungeprüft.
- Zeilen `[…/emulator.rs:…] action = …` auf stderr kommen vom Emulator, der manche Aktionen nicht
  ausführt (z. B. die Frage nach dem Erscheinungsbild des Systems), und sind kein Fehler.
  `… not settled after 10 s` heißt: Die Ansicht kam nicht zur Ruhe, das Bild zeigt den Stand nach
  10 s.

**Zustände per Startparameter (Q.1), ohne zu klicken:** `--demo --open <zustand>` öffnet einen
Zustand direkt (bei `shots.sh` stehen die Zustände vor `--`) (`--open` und `--now` gehen nur mit `--demo`). Kalender: `day week month agenda
timeline radar night editor editor-new menu drawer error empty loading` (`night`: Zeitstrahl
mit der Karte „Night“); Aufgaben: `detail pr more menu display drawer
due error empty loading` (`pr`: Detail des ersten offenen Pull Requests, `more`: Detail mit
offenem „…“-Menü); Steuerzentrale (`torroworkplace ui --demo`, Beispieldaten im
Speicher, kein Dienst, nichts wird gespeichert): `wizard-nextcloud wizard-mail wizard-mail-certificate
wizard-mail-certificate-public wizard-ai wizard-torromail account-detail account-detail-ai sorting-menu menu drawer remove delete-template rename-template design
design-schemes design-switched design-fonts design-fonts-search api-empty api-off api-detail api-new-key api-revoke create-key create-key-done mcp-empty mcp-off
mcp-detail mcp-hooks mcp-new-key mcp-disconnect mcp-bots mcp-orphans connect-client
connect-client-rights connect-client-done connect-client-other connections connections-builtin connections-own
connections-empty search-providers catalog-install
catalog-install-tools extension-detail extension-review extension-crashed extension-folder extension-unbounded
extension-unusable custom-server custom-server-paste custom-server-choose setup-documents` (`sorting-menu`: Settings › AI mit offenem Menü von „Sorting“, „Decision models (fast)“ vor „Other models“ (#421),
`connections`: MCP servers mit eigenen Werkzeugen, Katalog und eigenen Servern (#302), seit #548 auf dem Reiter
„Catalog“ (der Demo-Server GitHub ist abgestürzt), `connections-builtin` und `connections-own`: dieselbe auf den
Reitern „Built-in“ und „Your servers“,
`search-providers`: die Anbieter der Websuche unter dem Werkzeug „Web“ (#305),
`catalog-install`/`catalog-install-tools`: Installieren aus dem Katalog vor und nach dem Lesen der Werkzeuge,
`extension-review`: Seite eines Servers mit Werkzeugen, die auf „Allow tools“ warten, `extension-crashed`: dieselbe
nach einem Absturz, `extension-folder`: Seite von WordCraft mit der Gruppe „Folder“ und gesperrten Werkzeugen (#303),
`extension-unbounded`: Seite von GridCraft ohne gewählten Ordner, mit dem Hinweis, dass es sich an keinen Ordner hält,
`extension-unusable`: Seite von WordCraft, deren Ordner freigegeben, aber gerade nicht nutzbar ist,
`setup-documents`: Einrichtungsschritt „Documents“ mit je einer ArtCraft-App eingerichtet, installiert und fehlend, `custom-server-paste`: eigener Server mit eingefügtem `mcpServers`-Block,
`custom-server-choose`: Auswahl, wenn der Block mehrere Server nennt; `wizard-mail-certificate`: Frage „Trust this certificate?“ zu einem Server im lokalen Netz,
`wizard-mail-certificate-public`: dieselbe Frage mit Warnung für einen öffentlichen Host; `design`: Appearance mit den
drei Abstandsstufen und Beispiel-Schriften und -Farbschemata, je eins davon ungültig mit seinen Gründen (gezeichnet in
der Stufe, die `--design` nennt, sonst im Standard); `design-schemes`: dieselbe Seite mit offener
Auswahl der Farbschemata; `design-fonts`: dieselbe Seite mit offenem Dropdown der Schriften (#299: Schriften von Torro, Linie, Schriften des
Systems), `design-fonts-search`: dasselbe mit „mo“ im Suchfeld; `design-switched`: die Seite nach der Wahl von Dense,
das ganze Fenster in dieser Stufe, sonst nichts umgestellt (#484); `mcp-hooks`: Seite von Claude Code mit
„Notification Hub“; `mcp-orphans`: „Hooks without an agent“; alle Wörter nennt die Fehlermeldung zu
einem unbekannten) und `--section overview|accounts|ai|api|mcp|connections|approvals|workspaces|appearance|general`.
`menu` ist das Menü der mittleren/schmalen Kopfleiste – in der Größe ohne dieses Menü zeigt der Zustand es
nicht. `--appearance
light|dark|system` überschreibt nur für diesen Prozess die gespeicherte Einstellung (damit
entfällt jedes im Code erzwungene „hell“); `--design <id>` (nur mit `--demo`, alle Apps und die
Steuerzentrale) zeichnet mit der Abstandsstufe `<id>`: `torro` (Standard), `comfort` oder `dense` (#484; eigene
Designs aus einem Ordner gibt es nicht mehr) – jede Ansicht in allen drei Stufen ansehen, also auch mit
`--design comfort` und `--design dense`; `--font` und `--color-scheme` wählen Schrift (z. B. `plex`) und Farben
ebenso (eigene aus ihren Ordnern mit `TORRO_DATA_HOME` auf ein Temp-Verzeichnis, damit die echten unberührt bleiben); `--now 2026-10-13T10:30` (Kalender, Aufgaben) fixiert
„jetzt“ samt Demo-Daten für wiederholbare Bilder (auch im Aufgaben-Detail).
Unbekannte Werte: Fehlermeldung mit den gültigen Werten, Exit-Code 64. Beispiel:

```bash
.claude/skills/torro-design/shots.sh torrocalendar "$SCRATCH/design" editor -- --appearance light --now 2026-10-13T10:30
```

`$SCRATCH` = Scratchpad-Verzeichnis der Sitzung. Die PNGs mit dem Read-Werkzeug öffnen und gegen diese
Regeln prüfen. Scheint fremder Text durch, fehlt die Deckkraft-Regel (F.7) – Befund, kein App-Fehler.
Scheitert der Build, ist die Sichtprüfung „nicht möglich“.

**Rückfall unter Hyprland: `shoot.sh`.** Nur für das, was ohne Fenster fehlt (Mindestgröße, Hover,
die Erinnerung, Schriftwechsel), und nur unter Hyprland. Immer nur **eine** App zur Zeit und nur
Fenster mit der eigenen PID anfassen – die Fenster des Nutzers nie.

```bash
app=torrocalendar
cargo build -p "$app"
"./target/debug/$app" --demo --open week & pid=$!
sleep 3
.claude/skills/torro-design/shoot.sh "$pid" "$app" "$SCRATCH/design"
kill "$pid"
```

`shoot.sh` stellt das Fenster schwebend, setzt nacheinander die sechs Größen und speichert je ein PNG.
Hyprland ab 0.56 nimmt Befehle als Lua (`hl.dsp.window.float/resize/move`); das Skript nutzt diese
Form. Eine Ansicht wechseln geht per Tastenkürzel an das Fenster:
`hyprctl dispatch "hl.dsp.send_shortcut({ mods = '', key = 'm', window = 'pid:$pid' })"`. Hat das
Fenster nicht die angeforderte Größe (Mindestgröße), ist das ein Befund.

## 10. Prüfliste vor „fertig“

- [ ] Ziel 1–4 erfüllt: modern und aus einem Guss, Inter in den Stufen, Abstände aus den Tokens, in jeder Größe gut
- [ ] Keine Zahlen, Farben oder Iced-Standardstile in der App (`scripts/design-check.sh`)
- [ ] Rot nur Primärknopf und Heute; Auswahl neutral; Status nie nur über Farbe
- [ ] Icons aus `torro_gui::icon`, keine Unicode-Glyphen
- [ ] Kontraste eingehalten, kein Text in `FAINT`
- [ ] Leer-, Lade-, Fehlerzustand vorhanden; Formularfehler erst nach Speicherversuch
- [ ] Jede Aktion reagiert unmittelbar sichtbar; laufend/Erfolg/Fehler unterscheidbar, Teilerfolg und
      Abbruch nie als voller Erfolg; Erklärung erreichbar, keine Bewegung ohne tatsächliche Arbeit
- [ ] Tastatur: Fokus sichtbar, Enter/Escape/Pfeile, Kurzbefehle
- [ ] In allen sechs Größen angesehen: Layout wechselt sinnvoll, nichts abgeschnitten, groß nicht gestreckt
- [ ] Alle Texte über `tr()`, englisch, Satzschreibung

## 11. Aufräumen einer App

Für eine App, die es schon gibt (Audit, Aufräum-Issue), nicht für eine neue Änderung (dafür Abschnitt 10). Jede
Ansicht und jede Seite der Einstellungen einzeln, mit `shots.sh` in allen sechs Größen, hell und dunkel
(Abschnitt 9). Je Seite ein Befund der Form „Seite · Regel · Befund“.

- [ ] **Breite** (Abschnitt 6): Einstellungs-, Übersichts- und Kennzahlenseiten füllen die Breite neben der
  Seitenleiste; Fließtext und Bedienelemente bei `content_max()`; Kennzahlen und Karten bekommen mehr Spalten,
  unabhängige Blöcke stehen nebeneinander (`page_columns`); kein `max_width(size::content_max())` um eine ganze Seite.
- [ ] **Auswahl** (Abschnitt 5): bis etwa 4 Optionen `segmented` oder Chips, mehr `select_button`, ab etwa 12
  `search_menu`; keine langen Zeilenlisten für eine Einzelauswahl.
- [ ] **Reiter** (Abschnitt 5, Einstellungsseiten): Seiten mit mehr als drei Gruppen haben `tabs`; die Abschnitte
  stehen links im Untermenü.
- [ ] **Obere Zeile** (Abschnitt 0): kein App-Umschalter; Ampel, dann `panel-left`; der Einstellungs-Knopf mit
  `SETTINGS_ICON` ganz rechts, in jedem Fenster mit Einstellungen gleich.
- [ ] **Ort** (`docs/settings.md`, „Die Regel“): Was nur diese App betrifft, steht in der App; was die Suite teilt,
  in der Steuerzentrale (in der App nur ein Verweis, ohne Hauptanwendung die Seite selbst).
- [ ] **API und MCP**: Die App zeigt unter Einstellungen › API and MCP die Rechte ihres Bereichs (sofern sie einen
  hat), Anlegen und Schlüssel bleiben in der Steuerzentrale.
- [ ] **Prüfliste 10** vollständig für jede Seite.
