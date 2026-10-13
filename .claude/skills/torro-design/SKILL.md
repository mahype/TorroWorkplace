---
name: torro-design
description: Design-Regeln für alle Oberflächen von TorroWorkplace (Iced-Views, Widgets, Theme, Tray-Menüs, Benachrichtigungen). Laden, bevor UI gebaut, geändert oder begutachtet wird – Farben, Abstände, Typografie, Zustände, Barrierefreiheit, Texte, Sichtprüfung per Screenshot.
---

# Torro-Design

Stand 2026-10-11: **Vorbild ist Linear** (entschieden). Aufbau, Anordnung, Abstände, Schrift und
Symbole folgen Linear; Referenz sind die Entwürfe in `docs/prototypes/linear/` (`calendar.html`,
`tasks.html`, `torro.css`, Screenshots in drei Größen). Was dort steht, gilt; die Werte unten sind
daraus übernommen. Das Fundament in Iced entsteht mit `docs/tasks/design.md` (F.2–F.6). Gibt es
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
┌ Seitenleiste 232 (Grund BG) ┐┌ Inhalt: eingesetzte Fläche PANEL, Rand 8, Radius 8, Linie ────────┐
│ [T] App ▾      🔍  [✎]      ││ Kopfleiste 44: Pfad/Titel · Navigation …… Ansicht · Filter · [+ Neu] │
│ Navigation (Zeilen 28)      │├───────────────────────────────────────────┬───────────────────────┤
│ Gruppe ▾ (CAPTION, TEXT_3)  ││ Inhalt (Liste, Raster, Detail)            │ Eigenschaften 300     │
│ Listen/Kalender mit Punkt   ││                                           │ (nur Breit)           │
│ …                           ││                                           │                       │
│ ↗ andere App (· Settings)   ││                                           │                       │
└─────────────────────────────┘└───────────────────────────────────────────┴───────────────────────┘
```

- **Oben links der App-Umschalter** („T Calendar ▾“, wie Linears Workspace-Menü): öffnet die
  anderen Apps der Suite (Kern-Befehl `OpenApp`), daneben Suche und „Neu“ (Kasten-Knopf).
- **Seitenleiste** auf dem Fenstergrund `BG`, **Inhalt** als eingesetzte Fläche `PANEL` mit 8 Rand
  oben/rechts/unten, Radius 8, 1-px-Linie. Ein Design-Paket kann der Seitenleiste eigene Farben geben
  (`sidebar…`, L.6): Die Shell zeichnet sie dann in einem eigenen Theme, `palette(theme)` liefert dort
  die Farben der Leiste. In der Seitenleiste also nie Farben am Theme vorbei festlegen, sondern immer
  über `palette(theme)` in der Stilfunktion; Fensterzustände (Anmelden, Laden) liegen auf
  `Surface::Window` (`BG`), Seitenleiste und Rail auf `Surface::Sidebar`. Ohne Seitenleiste (Schmal) ist der Inhalt bündig
  ohne Rand und Radius; neben der Leiste (Mittel) ist er eingesetzt wie in Breit.
- **Seitenleiste in drei Zuständen** (U.2, `shell::SidebarMode`): **ausgeklappt** (232), **Leiste**
  (`shell::rail()` = 52: nur Icons, Name und Zahl als Tooltip rechts, Gruppentitel als Linie,
  App-Umschalter nur als Logo, Mini-Monat und Versionszeile entfallen) und **Schublade** (Overlay).
  Breit wählt der Nutzer ausgeklappt oder Leiste – Knopf `panel-left` („Collapse sidebar“/„Expand
  sidebar“) und Taste `[` wie Linear; jede App merkt sich die Wahl in `ui.json` im State-Verzeichnis
  (`torro_gui::ui_state`). Mittel zeigt immer die Leiste, der Knopf öffnet die ausgeklappte
  Seitenleiste als Schublade; Schmal nur Schublade.
- **Fensterknöpfe im Titelband** (macOS, U.10/U.11, wie Claude/Slack): der Knopf `panel-left` rechts
  neben der Ampel, die Einstellungen (Zahnrad, „Settings“) in der Ecke oben rechts –
  `shell::TitleButtons` an `shell::titlebar`. Alles auf einer Linie: Ampel (verschoben mit
  `torro_gui::place_traffic_lights()` beim Start), Knöpfe und Zahnrad mit Mitte bei y = 20, mittig
  zwischen Oberkante und eingesetzter Fläche; Schließen-Knopf und Zahnrad je 20 vom Rand. Dort fehlen sie in Seitenleiste und Kopfleiste
  (`shell::has_band()`); auf anderen Systemen (kein Band) bleibt der Knopf oben in der Seitenleiste
  bzw. in der schmalen Kopfleiste und „Settings“ unten in der Seitenleiste. Eingeklappt rückt der Inhalt nach links, nichts überlagert. Zeilen einmal bauen, mit
  dem Modus als Parameter: `widgets::sidebar_item_in`, `sidebar_group_in`, `shell::sidebar_head`,
  `Shell::rail`.
- **Kopfleiste** 44 hoch im Inhalt: links Pfad bzw. Titel (`typo::body_strong()`) mit leiser Ergänzung in
  `TEXT_3`, dann Navigation; rechts Ansicht, Filter, genau ein Primärknopf.
- **Eigenschaften-Spalte** 300 breit rechts (Linear „Properties“): Gruppen mit `typo::caption()`-Titel in
  `TEXT_3`, Zeilen 32 mit Icon und Wert.
- **Kleiner werden** in dieser Reihenfolge, nie etwas Funktion verlieren: (1) Seitenleiste ins Menü
  oben links („Today ▾“, „Week ▾“), (2) Eigenschaften als schwebende Karte bzw. als Chips unter dem
  Titel, (3) eine Ansicht ersetzt die andere (Detail statt Liste, Tag statt Woche) mit „‹ Zurück“.
- **Hell und dunkel** für alle Apps gemeinsam (Einstellung „Appearance: System / Light / Dark“ in
  der Steuerzentrale, über die API), live umschaltbar.

## 1. Tokens statt Einzelwerte

- **Die Werte kommen aus dem aktiven Design-Paket** (`torro_gui::design`, Konzept
  [`docs/design-pakete.md`](../../../docs/design-pakete.md)); die Zahlen in diesem Skill sind die des
  Standardpakets „Torro“. Ein Paket ändert, *wie* ein Token aussieht, nie, *wofür* er benutzt wird – die
  Regeln hier gelten für jedes Paket. Darum sind Tokens Funktionen (`space::m()`, `size::row()`,
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

### Akzent und Status

- **Torro-Rot ist Marke, nie Status, nie Auswahl.** Pro Fenster: ein Primärknopf, Heute-Kreis,
  Jetzt-Linie mit Zeitlabel in der Achse, das App-Logo. Aktiver Reiter, gewählte
  Seitenleistenzeile, ausgewählte Zeile: neutral (`SELECTED` + `TEXT`), wie bei Linear.
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
- **Abstände nach Rolle** (`torro_gui::layout`): `layout::inset()` 8 (Rand der eingesetzten
  Inhaltsfläche), `page_padding()` 24 × 24 (Innenrand von Seiten), `panel_padding()` 16 × 16
  (Paneele, schwebende Karten), `row_padding()` 0 × 16 (Listen- und Einstellungszeilen),
  `control_padding()` 0 × 10 (Knöpfe, `style::button_padding()`), `section_gap()` 24 (zwischen Gruppen
  und Abschnitten), `item_gap()` 8 (innerhalb einer Gruppe). Nur dort, wo die Rolle genau passt;
  alles andere bleibt auf der Skala `space::*`.
- **Maße** (aus den Entwürfen):

| Token | Wert | Wofür |
|---|---|---|
| `size::control_sm()` | 28 | Knöpfe, Icon-Knöpfe, Segmente, Menü-Knöpfe in der Kopfleiste |
| `size::control()` | 32 | Eingaben, Eigenschaftszeilen |
| `size::control_lg()` | 36 | Schnelleingabe, Primärknopf im Dialog |
| `size::row()` | 40 | Listenzeile (Aufgaben) |
| `size::row_message()` | 88 | Mail-Zeile im Eingang: Avatar 32, Absender · Domain · Zeit, Betreff, Vorschau (E3.9) |
| `size::row_notice()` | 64 | Andere Zeile im Eingang: Kachel 28 mit Quell-Icon, Titel · Zeit, Text (E3.9) |
| `size::card_gap()` | 12 | Abstand zwischen Karten und zwischen den Teilen einer Karte (`widgets::card`, E8.2) |
| `size::avatar()` / `size::avatar_lg()` | 36 / 40 | Avatar einer Karte / Absender im Kopf des Lesers (E8.2); die kompakte Mail-Zeile bleibt bei 32 |
| `size::tile_sm()` / `size::tile()` / `size::tile_lg()` | 20 / 28 / 36 | Quellen-Kachel (`widgets::source_tile`, E8.2): neben Wörtern / Hinweiszeile, Spaltenkopf / Anhang, Kopf einer App-Karte |
| `size::row_sidebar()` | 28 | Seitenleisten- und Menüzeile (Menü 30) |
| `size::row_group()` | 32 | Gruppenkopf in Listen („Overdue 2“) |
| `size::header()` | 44 | Kopfleiste |
| `size::sidebar()` | 232 | Seitenleiste |
| `size::props()` | 300 | Eigenschaften-Spalte |
| `size::panel()` | 400 | Seitenpaneel für Details (`torro_gui::panel`): Breit daneben, in Schmal darüber in voller Höhe (die Aufgaben-App zeigt ihr Detail auch in Mittel daneben, Abschnitt 6) |
| `shell::rail()` | 52 | Eingeklappte Seitenleiste (Leiste, nur Icons) |
| `size::list()` / `size::list_medium()` | 460 / 380 | Liste neben dem Detail (Breit / Mittel) |
| `size::hour()` | 48 | Stundenhöhe im Kalender, Zeitachse 52 (Schmal 44) |
| `layout::inset()` | 8 | Rand der eingesetzten Inhaltsfläche |
| `size::content_max()` / `size::form_max()` | 720 / 640 | Lesbreite von Listen bzw. Formularen ohne Detail |

- Knöpfe: Höhe `size::control_sm()`, Innenabstand waagerecht 10, Text `typo::caption()`-Größe 12/500; Icon-Knöpfe
  28×28; Icons 16 (in Knöpfen 14), Strich 1.75.
- **Radien** `radius::xs()` 4 (Chips, Terminblöcke, Segment innen, Tasten-Hinweis) · `sm()` 6 (Knöpfe,
  Zeilen, Eingaben, Menüeinträge) · `md()` 8 (Inhaltsfläche, Menüs) · `lg()` 10 (schwebende Karten,
  Dialoge) · voll rund für Punkte, Ringe, Avatare, Heute.
- **Linien:** 1 logischer Pixel in `LINE`; Raster nur mit Innenlinien (keine Rahmen je Zelle,
  keine Doppellinien). Listenzeilen durch Linien getrennt, Seitenleiste ohne Linien.

## 3. Typografie

- **Schrift: Inter 4** (SIL OFL), mit der App ausgeliefert (`torro_gui::fonts()`,
  `torro_gui::settings()`), **Grundgröße 13** wie Linear. Nie `Font::DEFAULT`.
  Größer oder kleiner nur über den **Zoom** der Suite (Z.1, `torro_gui::zoom`): Strg + Plus /
  Minus / 0 in jeder App, Stufen wie im Browser (67–200 %), eine Einstellung für alle (`Settings::zoom`,
  Steuerzentrale › General › Interface). Umgesetzt mit Iceds `scale_factor`, nie über andere
  Schriftgrößen. Die Größenklassen rechnen in logischen Pixeln *nach* dem Zoom (1440 breit bei
  150 % = 960 = Mittel), wie Breakpoints im Browser. Jede App: `.scale_factor(…)` im Builder,
  `zoom.subscription()`, `zoom.view(…)` um die ganze Ansicht (kurze Anzeige „110%“ unten mittig).
- **Stufen** (`torro_gui::typo`), Zeilenhöhe immer absolut:

| Stufe | Größe / Zeile | Gewicht | Wofür |
|---|---|---|---|
| `typo::micro()` | 11 / 14 | 400 | Zeitachse, Uhrzeit in Terminen, Wochentag in der Wochenleiste (immer tabellarisch) |
| `typo::caption()` | 12 / 16 | 500 | Gruppentitel (Satzschreibung, `TEXT_3`), Knöpfe, Chips, Badges |
| `typo::meta()` | 12 / 16 | 400 | Metadaten, Zähler, Fälligkeit |
| `typo::body()` | 13 / 20 | 400 | Standard: Listen, Seitenleiste, Eingaben, Fließtext |
| `typo::body_strong()` | 13 / 20 | 500 | Kopfleisten-Titel, Zeilentitel, Termintitel |
| `typo::read()` | 14 / 22 | 400 | Lesetext: die ganze Mail im Leser des Eingangs (E8.2); nie für Zeilen, Labels, Eingaben |
| `typo::title()` | 16 / 22 | 600 | Karten- und Dialogtitel |
| `typo::headline()` | 22 / 28 | 600 | Detailtitel („Review API concept“), Ansichtstitel ohne Kopfleiste |
| `typo::display()` | 28 / 32 | 600 | Kopfzahl in Mini (Uhrzeit, Zahl offener Aufgaben) |

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

- **Jedes Bedienelement** hat fünf Zustände: normal; Hover = `HOVER`; gedrückt/ausgewählt =
  `SELECTED`; Fokus = `FOCUS_RING`; deaktiviert = 40 % Deckkraft, keine Hover-Fläche.
- **Knöpfe** (`style::button`): `primary` (Rot, weiß, höchstens einer pro Fenster), `secondary`
  (`RAISED` mit `LINE_STRONG`-Rand), `ghost` (transparent, `TEXT_2`, Hover `HOVER` + `TEXT` –
  Standard in Kopfleisten), `icon` (28×28 ghost), `boxed` (Icon-Knopf mit Rand, für „Neu“ in der
  Seitenleiste), `danger` (ghost mit `STATUS_ERROR`-Text). Wird der Primärknopf in Schmal zum Icon
  (`widgets::compact_button`), ist er 28×28 mit Icon 16 in `on_accent` – so bleibt das „+“ auf Rot
  kräftig (5,4:1). Ansichten wechseln: `segmented` (Grund
  `BG`, aktives Segment `SELECTED` mit `LINE_SELECTED`-Kante). Speichert ein Knopf, zeigt er
  `widgets::busy_button(tr("Creating…"), style)`: volle Farbe, Icon, nimmt keinen zweiten Klick.
- **Wählbare Chips** (G.10): `widgets::choice_chip(label, mark, on, on_press)` – 28 hoch, Radius 6,
  `RAISED` + `LINE_STRONG`; Hover `HOVER` + `TEXT`; gewählt `SELECTED`, Kante `LINE_SELECTED`, Häkchen 14
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
- **Zwei Dichten im Eingang** (E8.2, `ui_state::Density`): **Compact** (eine Zeile je Eintrag, Aktionsleiste
  beim Überfahren) und **Comfortable** (Karten). Umschalter `widgets::density_switch(dichte, wörter, nachricht)` in
  der Kopfleiste jeder Fensterart, Icons `List` / `LayoutGrid`, in Schmal nur Icons mit Tooltip. Gemerkt je
  Fensterart und Ausschnitt in `ui.json` (`ui_state::save_density_task`, Schlüssel wie `mail`, `hub/lane:info`);
  `--density compact|comfortable` beim Start hat Vorrang. Ohne Wahl: Comfortable, in Schmal Compact
  (`Density::default_for`). Mini und Leiste bleiben Anzeigen.
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
  `cargo run -p torro-gui --example gallery -- --cards` (dazu `--light`, `--design plex` mit `TORRO_DATA_HOME`).
- **Hinweis mit „Undo“** (E3.11, `widgets::toast`): nach einer Änderung, die sich zurücknehmen lässt
  (Done, Later, Hide …), unten mittig `space::xl()` über dem Fensterrand; Fläche `MENU`, Rand
  `LINE_STRONG`, Radius `radius::md()`, Schatten (`style::container::menu`), Text `typo::body()` in
  `TEXT`, höchstens ein Knopf (`widgets::ToastAction::undo`: ghost „Undo“ mit `kbd` „Ctrl Z“/„Cmd Z“),
  eine Zeile hoch `size::control_lg()`, höchstens `size::content_max()` breit. Ruhig: keine Animation,
  er erscheint und geht – nach `widgets::TOAST_FOR` (5 s) oder mit `Escape` (vor dem Paneel).
  `Ctrl+Z` (⌘Z) nimmt zurück, solange er „Undo“ zeigt (`widgets::undo_key`). Als eigene Ebene über
  dem Fenster stapeln, immer (leer ohne Hinweis), damit das Fenster seinen Zustand behält.
- **Menüs** (`widgets::menu`): Fläche `MENU`, Rand `LINE_STRONG`, Radius 8, Schatten, Innenrand 4;
  Einträge 30 hoch mit Icon, Text und rechts Zahl oder Tasten-Hinweis; Abschnitte mit `typo::caption()`-
  Titel; Trenner `LINE`.
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
- **Kleine Bausteine der Aufgaben-App** (`widgets`, R.1; Beispiele im Doc-Kommentar, Anschauen mit
  `cargo run -p torro-gui --example gallery -- --parts`): `prop_line(label, mark, value, on_press)` –
  Eigenschaftszeile, Bezeichnung 88 in `typo::meta()`/`TEXT_3`, Wert daneben (`prop_value`, `prop_placeholder`
  für „nicht gesetzt“ in `TEXT_3`), ganze Zeile ein Knopf, mind. 32 hoch; `label_chip(hex, name)` –
  GitHub-Label, 20 hoch, Radius 4, Farbe aus `#RRGGBB` (auch ohne `#`) **nur als Punkt**,
  `label_chip_row` umbrechend mit Abstand 4; `state_pill(mark, wort)` – Zustand mit Wort, 22 hoch,
  rund, Farbe nur im Icon; `progress_bar(done, total)` – 64×4, Spur `LINE_STRONG`, Füllung `ok`,
  daneben immer „2/4“; `kbd(taste)` – Tastenhinweis (gab es schon).
- **Seitenleiste:** Zeilen 28 mit Icon 16, Text `typo::body()` in `TEXT_2` (aktiv `TEXT` auf `SELECTED`),
  Zahl rechts `typo::meta()` tabellarisch in `TEXT_3`, Tasten-Hinweis als `kbd`. Gruppen „Lists ▾“ als
  `typo::caption()` in `TEXT_3` mit 12 Abstand darüber. Listen mit Ring (Aufgaben) bzw. Punkt (Kalender).
- **Aufgabenzeile:** 40 hoch, Kreis 16 (abgehakt gefüllt in Listenfarbe mit Häkchen), Priorität als
  drei Balken (dringend: oranges Quadrat mit „!“), Titel einzeilig, rechts Listen-Etikett (Pille mit
  Punkt; GitHub-Einträge `widgets::icon_pill(Icon::Github, repo)`) und Fälligkeit `typo::meta()` tabellarisch
  (überfällig Attention-Orange). Gruppenköpfe 32 auf `RAISED` mit Icon, Name, Zahl und „+“.
- **Priorität** überall aus einer Abbildung (`torrotasks::list::Prio`): iCalendar 1 = Urgent
  (Quadrat), 2–4 High, 5 Medium, 6–9 Low, sonst keine; gespeichert werden 1 / 3 / 5 / 9. Zeile,
  Gruppenkopf, Chip, Eigenschaft und Menü zeigen dasselbe Wort und dasselbe Zeichen.
- **Werte, die nicht passen,** (Eigenschaften, Chips, Titel) werden mit „…“ gekürzt
  (`typo::ellipsize`, in `prop_value` eingebaut) und zeigen den ganzen Wert als Tooltip – nie
  stumm abgeschnitten.
- **Termine:** Fläche Kalenderfarbe 18 % (hell 14 %) über `PANEL`, 3-px-Balken links in voller
  Farbe, Titel 12/500 in `TEXT`, Zeit `typo::micro()` in `TEXT_2`, Radius 4, kein Rahmen; kurze Termine
  einzeilig „09:30 Titel“; unbestätigt gestrichelter Rahmen ohne Fläche; Aufgaben im Raster als
  Umriss mit Kreis; vergangene 55 % Deckkraft; freie Zeit als „2 h free“ in `TEXT_3`.
  **Heute** = roter Kreis mit weißer Zahl; **Jetzt** = rote Linie 1 px mit Punkt und Zeitlabel in
  der Achse.
- **Einstellungsseiten:** nur `widgets::group` (Titel, Zeilen 44 bzw. 56 mit Untertitel, Trennlinien,
  erklärender Satz darunter in `TEXT_2`) – keine eigenen Karten. Eine sichtbare Aktion je Zeile, der
  Rest im „…“-Menü.
- **Ansichten:** **leer** (`empty_state`: was hier erscheint und was zu tun ist), **lädt**
  (`loading()`, ruhiger Platzhalter, kein Springen), **Fehler** (`error_state`: was passiert ist +
  „Retry“), **voll** (lange Titel, viele Einträge, Überschneidungen).
- **Formulare:** Fehler erst nach dem ersten Speicherversuch oder beim Verlassen des Feldes,
  nie vor der ersten Eingabe. Speichern bleibt aktiv und springt zum ersten Fehler.
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
| 3 | **Schmal** | Breite < 600 | bündig | Kopf 44, Navigation im Menü | ein Tag mit Wochenleiste / Liste, Detail ersetzt die Liste |
| 4 | **Mittel** | Breite < 1024 | eingesetzt 8 | Leiste 52 + Kopf 44, Navigation im Menü | Woche + schwebende Karte / Liste 380 **neben** dem Detail, Eigenschaften als Chips unter dem Titel |
| 5 | **Breit** | sonst | eingesetzt 8 | Seitenleiste 232 (oder Leiste 52) + Kopf 44 + Eigenschaften 300 | Woche + Eigenschaften / Liste 460 + Detail + Eigenschaften-Spalte 300 |

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
- **Groß heißt nicht gestreckt:** Listen und Seiten max. `size::content_max()`, Formulare `size::form_max()`,
  zentriert bzw. links mit Seitenleiste; zusätzlicher Platz geht an Inhalt (mehr Tage, Detail
  daneben), nicht an Leerraum zwischen Elementen.
- Zustand bleibt beim Größenwechsel erhalten (Auswahl, Scrollposition, Datum, gewählte Ansicht –
  in Schmal wird nur die *dargestellte* Ansicht angepasst).
- **Dauerbetrieb:** nichts blinkt oder bewegt sich ohne Anlass; neu zeichnen nur bei Änderung bzw.
  im Minutentakt; keine Spinner im Ruhezustand; geringe CPU- und GPU-Last.
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

## 9. Sichtprüfung (Pflicht bei UI-Änderungen)

Tests reichen bei Oberfläche nicht. Jede geänderte Ansicht in **allen Größenklassen** ansehen.
Immer nur **eine** App zur Zeit und nur Fenster mit der eigenen PID anfassen – die Fenster des
Nutzers nie.

```bash
app=torrocalendar   # bzw. torrotasks; Steuerzentrale: torroworkplace ui --demo (ab Q.1)
cargo build -p "$app"
"./target/debug/$app" --demo & pid=$!      # ab Q.1 auch: --open editor | detail | error | empty …
sleep 3
.claude/skills/torro-design/shoot.sh "$pid" "$app" "$SCRATCH/design"
kill "$pid"
```

`shoot.sh` stellt das Fenster schwebend, setzt nacheinander die sechs Größen Mini (240×160), Mini
hoch (320×480), Leiste (1200×200), Schmal (480×900), Mittel (800×600) und Breit (1600×1000) und
speichert je ein PNG. Hyprland ab 0.56 nimmt Befehle als Lua (`hl.dsp.window.float/resize/move`);
das Skript nutzt diese Form. Eine Ansicht wechseln geht per Tastenkürzel an das Fenster:
`hyprctl dispatch "hl.dsp.send_shortcut({ mods = '', key = 'm', window = 'pid:$pid' })"`.

**Zustände per Startparameter (Q.1), ohne zu klicken:** `--demo --open <zustand>` öffnet einen
Zustand direkt (`--open` und `--now` gehen nur mit `--demo`). Kalender: `day week month agenda
timeline radar night editor editor-new menu drawer switcher error empty loading` (`night`: Zeitstrahl
mit der Karte „Night“); Aufgaben: `detail pr more menu display drawer
switcher due error empty loading` (`pr`: Detail des ersten offenen Pull Requests, `more`: Detail mit
offenem „…“-Menü); Steuerzentrale (`torroworkplace ui --demo`, Beispieldaten im
Speicher, kein Dienst, nichts wird gespeichert): `wizard account-detail menu drawer switcher
remove delete-template rename-template design` (`design`: General › Design mit Beispielpaketen, eines
davon ungültig mit seinen Gründen) und `--section overview|accounts|workspaces|apps|general`.
`menu` ist das Menü der mittleren/schmalen Kopfleiste, `switcher` der App-Umschalter (dort im
Drawer) – in der Größe ohne dieses Menü zeigt der Zustand es nicht. `--appearance
light|dark|system` überschreibt nur für diesen Prozess die gespeicherte Einstellung (damit
entfällt jedes im Code erzwungene „hell“); `--design <id>` (nur mit `--demo`, alle Apps und die
Steuerzentrale) zeichnet mit dem Design-Paket `<id>` aus dem Design-Ordner (`torro` = Standard, z. B.
`plex` aus `designs/plex`; mit `TORRO_DATA_HOME` auf ein Temp-Verzeichnis mit `designs/plex` darin,
damit der echte Ordner unberührt bleibt) – jede Ansicht mit beiden Paketen ansehen; `--now 2026-10-13T10:30` (Kalender, Aufgaben) fixiert
„jetzt“ samt Demo-Daten für wiederholbare Bilder (auch im Aufgaben-Detail).
Unbekannte Werte: Fehlermeldung mit den gültigen Werten, Exit-Code 64. Beispiel:

```bash
./target/debug/torrocalendar --demo --open editor --appearance light --now 2026-10-13T10:30 & pid=$!
sleep 3; .claude/skills/torro-design/shoot.sh "$pid" cal-editor "$SCRATCH/design"; kill "$pid"
```

`$SCRATCH` = Scratchpad-Verzeichnis der Sitzung. Hat das Fenster nicht die angeforderte Größe
(Mindestgröße), ist das ein Befund. Die PNGs mit dem Read-Werkzeug öffnen und gegen diese Regeln
prüfen. Scheint fremder Text durch, fehlt die Deckkraft-Regel (F.7) – Befund, kein App-Fehler.
Läuft kein Hyprland, Sichtprüfung als „nicht möglich“ melden.

## 10. Prüfliste vor „fertig“

- [ ] Ziel 1–4 erfüllt: modern und aus einem Guss, Inter in den Stufen, Abstände aus den Tokens, in jeder Größe gut
- [ ] Keine Zahlen, Farben oder Iced-Standardstile in der App (`scripts/design-check.sh`)
- [ ] Rot nur Primärknopf und Heute; Auswahl neutral; Status nie nur über Farbe
- [ ] Icons aus `torro_gui::icon`, keine Unicode-Glyphen
- [ ] Kontraste eingehalten, kein Text in `FAINT`
- [ ] Leer-, Lade-, Fehlerzustand vorhanden; Formularfehler erst nach Speicherversuch
- [ ] Tastatur: Fokus sichtbar, Enter/Escape/Pfeile, Kurzbefehle
- [ ] In allen sechs Größen angesehen: Layout wechselt sinnvoll, nichts abgeschnitten, groß nicht gestreckt
- [ ] Alle Texte über `tr()`, englisch, Satzschreibung
