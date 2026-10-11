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
│ ↗ andere App · Settings     ││                                           │                       │
└─────────────────────────────┘└───────────────────────────────────────────┴───────────────────────┘
```

- **Oben links der App-Umschalter** („T Calendar ▾“, wie Linears Workspace-Menü): öffnet die
  anderen Apps der Suite (Kern-Befehl `OpenApp`), daneben Suche und „Neu“ (Kasten-Knopf).
- **Seitenleiste** auf dem Fenstergrund `BG`, **Inhalt** als eingesetzte Fläche `PANEL` mit 8 Rand
  oben/rechts/unten, Radius 8, 1-px-Linie. Ohne Seitenleiste (Mittel, Schmal) ist der Inhalt bündig
  ohne Rand und Radius.
- **Kopfleiste** 44 hoch im Inhalt: links Pfad bzw. Titel (`BODY_STRONG`) mit leiser Ergänzung in
  `TEXT_3`, dann Navigation; rechts Ansicht, Filter, genau ein Primärknopf.
- **Eigenschaften-Spalte** 300 breit rechts (Linear „Properties“): Gruppen mit `CAPTION`-Titel in
  `TEXT_3`, Zeilen 32 mit Icon und Wert.
- **Kleiner werden** in dieser Reihenfolge, nie etwas Funktion verlieren: (1) Seitenleiste ins Menü
  oben links („Today ▾“, „Week ▾“), (2) Eigenschaften als schwebende Karte bzw. als Chips unter dem
  Titel, (3) eine Ansicht ersetzt die andere (Detail statt Liste, Tag statt Woche) mit „‹ Zurück“.
- **Hell und dunkel** für alle Apps gemeinsam (Einstellung „Appearance: System / Light / Dark“ in
  der Steuerzentrale, über die API), live umschaltbar.

## 1. Tokens statt Einzelwerte

- **In Apps keine Zahlen** in `.size()`, `.spacing()`, `.padding()`, `.width()`/`.height()`,
  `.rounded()` und kein `Color::from_rgb…`: nur Tokens aus `torro_gui::{theme, typo}`.
  **Keine Iced-Standardstile** (`button::primary`, `button::secondary`, Standard-Checkbox …): nur
  `torro_gui::style` und die Bausteine aus `torro_gui::widgets`. `scripts/design-check.sh` prüft das
  (F.6). Fehlt ein Token oder Stil: in `torro-gui` ergänzen (eigene Aufgabe/Rückfrage), nie lokal.
- Markenfarben kommen aus `torro-design/tokens/tokens.json` → `torro-core/src/theme.rs`; die
  neutralen Flächen und Textstufen stehen in `torro-gui/src/theme.rs`. (Das Repo `torro-design`
  liegt nicht im Workspace; Änderungen dort als Rückfrage melden.)

### Farben

| Token | Dunkel | Hell | Wofür |
|---|---|---|---|
| `BG` | `#08090A` | `#F4F5F6` | Fenstergrund, Seitenleiste |
| `PANEL` | `#0F1011` | `#FFFFFF` | Inhaltsfläche |
| `RAISED` | `#141518` | `#F8F9FA` | Knöpfe `secondary`, Chips, Gruppenköpfe in Listen |
| `HOVER` | `#16171A` | `#F0F1F3` | Hover |
| `SELECTED` | `#1C1E22` | `#E9EAED` | Auswahl, gedrückt, aktives Segment |
| `MENU` | `#17181B` | `#FFFFFF` | Menüs, schwebende Karten, Dialoge (mit Schatten) |
| `TEXT` | `#EDEEF0` | `#18191B` | Text |
| `TEXT_2` | `#B4B8BF` | `#4F535A` | Sekundärtext, Beschreibungen |
| `TEXT_3` | `#8A8F98` | `#62666D` | Metadaten, Zähler, Gruppentitel, Platzhalter (≥ 4.5:1 auf allen Flächen) |
| `LINE` | Weiß 6 % | Schwarz 7 % | Trennlinien, Raster, Rand der Inhaltsfläche |
| `LINE_STRONG` | Weiß 10 % | Schwarz 11 % | Rahmen von Knöpfen, Eingaben, Chips, Menüs |
| `FOCUS_RING` | Weiß 35 % | Schwarz 35 % | Fokus, 2 px, 1 px Abstand |
| `ACCENT` | Torro-Rot `#D50C0C` | dto. | **nur** Primärknopf, Heute, Jetzt-Linie, App-Logo |
| Kalender/Listen | `#5B8CFF` `#3FB984` `#E8A33D` `#D96CA8` `#A78BFA` | `#3B6EF0` `#1F9D68` `#C9811A` `#C2508F` `#7C5CE6` | Standardfarben, Tönung 18 % (hell 14 %) |

### Akzent und Status

- **Torro-Rot ist Marke, nie Status, nie Auswahl.** Pro Fenster: ein Primärknopf, Heute-Kreis,
  Jetzt-Linie mit Zeitlabel in der Achse, das App-Logo. Aktiver Reiter, gewählte
  Seitenleistenzeile, ausgewählte Zeile: neutral (`SELECTED` + `TEXT`), wie bei Linear.
- Status: Fehler `STATUS_ERROR`, Warten/Abweichung/überfällig Attention-Orange, OK Grün, inaktiv
  Grau – immer mit Wort oder Icon, Farbe nur im Punkt bzw. Icon.
- Kalender- und Listenfarben gehören dem Nutzer: als Farbbalken, Punkt, Ring oder Häkchen, nie als
  großflächige gesättigte Fläche.

## 2. Abstände, Maße, Radien, Linien

- **Abstände** `space::XS 4 · S 8 · M 12 · L 16 · XL 24 · XXL 32` (48 nur für große Leerzustände).
  Innerhalb von Bausteinen sind 2 und 6 erlaubt (Segment-Innenrand, Icon-Text-Abstand in Chips),
  sonst nur die Skala.
- Label → Feld 4, innerhalb einer Gruppe 8, Gruppe → Gruppe 16–24. Nähe zeigt Zugehörigkeit.
- **Maße** (aus den Entwürfen):

| Token | Wert | Wofür |
|---|---|---|
| `CONTROL_SM` | 28 | Knöpfe, Icon-Knöpfe, Segmente, Menü-Knöpfe in der Kopfleiste |
| `CONTROL` | 32 | Eingaben, Eigenschaftszeilen |
| `CONTROL_LG` | 36 | Schnelleingabe, Primärknopf im Dialog |
| `ROW` | 40 | Listenzeile (Aufgaben) |
| `ROW_SIDEBAR` | 28 | Seitenleisten- und Menüzeile (Menü 30) |
| `ROW_GROUP` | 32 | Gruppenkopf in Listen („Overdue 2“) |
| `HEADER` | 44 | Kopfleiste |
| `SIDEBAR` | 232 | Seitenleiste |
| `PROPS` | 300 | Eigenschaften-Spalte |
| `LIST` | 460 / 380 | Liste neben dem Detail (Breit / Mittel) |
| `HOUR` | 48 | Stundenhöhe im Kalender, Zeitachse 52 (Schmal 44) |
| `INSET` | 8 | Rand der eingesetzten Inhaltsfläche |
| `CONTENT_MAX` / `FORM_MAX` | 720 / 640 | Lesbreite von Listen bzw. Formularen ohne Detail |

- Knöpfe: Höhe `CONTROL_SM`, Innenabstand waagerecht 10, Text `CAPTION`-Größe 12/500; Icon-Knöpfe
  28×28; Icons 16 (in Knöpfen 14), Strich 1.75.
- **Radien** `radius::XS 4` (Chips, Terminblöcke, Segment innen, Tasten-Hinweis) · `SM 6` (Knöpfe,
  Zeilen, Eingaben, Menüeinträge) · `MD 8` (Inhaltsfläche, Menüs) · `LG 10` (schwebende Karten,
  Dialoge) · voll rund für Punkte, Ringe, Avatare, Heute.
- **Linien:** 1 logischer Pixel in `LINE`; Raster nur mit Innenlinien (keine Rahmen je Zelle,
  keine Doppellinien). Listenzeilen durch Linien getrennt, Seitenleiste ohne Linien.

## 3. Typografie

- **Schrift: Inter 4** (SIL OFL), mit der App ausgeliefert (`torro_gui::fonts()`,
  `torro_gui::settings()`), **Grundgröße 13** wie Linear. Nie `Font::DEFAULT`.
  Größere Darstellung später über die Einstellung „Interface size“ (Iced `scale_factor`), nicht über
  andere Schriftgrößen.
- **Stufen** (`torro_gui::typo`), Zeilenhöhe immer absolut:

| Stufe | Größe / Zeile | Gewicht | Wofür |
|---|---|---|---|
| `MICRO` | 11 / 14 | 400 | Zeitachse, Uhrzeit in Terminen, Wochentag in der Wochenleiste (immer tabellarisch) |
| `CAPTION` | 12 / 16 | 500 | Gruppentitel (Satzschreibung, `TEXT_3`), Knöpfe, Chips, Badges |
| `META` | 12 / 16 | 400 | Metadaten, Zähler, Fälligkeit |
| `BODY` | 13 / 20 | 400 | Standard: Listen, Seitenleiste, Eingaben, Fließtext |
| `BODY_STRONG` | 13 / 20 | 500 | Kopfleisten-Titel, Zeilentitel, Termintitel |
| `TITLE` | 16 / 22 | 600 | Karten- und Dialogtitel |
| `HEADLINE` | 22 / 28 | 600 | Detailtitel („Review API concept“), Ansichtstitel ohne Kopfleiste |
| `DISPLAY` | 28 / 32 | 600 | Kopfzahl in Mini (Uhrzeit, Zahl offener Aufgaben) |

- Gewichte nur 400/500/600; kein Bold, keine Kursive. Hierarchie über Stufe *und* Farbe
  (`TEXT` → `TEXT_2` → `TEXT_3`).
- **Keine Versalien** für Abschnittstitel: Satzschreibung in `CAPTION` (wie Linear).
- **Ziffern, die sich ändern** (Uhrzeiten, Achsen, Zähler, Daten in Spalten): `.tabular()`.
- Lange Texte in Zeilen einzeilig mit Auslassung; in Detailansichten umbrechen. Kleinste Schrift 11
  (`MICRO`), nur für tabellarische Ziffern und Wochentagskürzel.

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
  Seitenleiste), `danger` (ghost mit `STATUS_ERROR`-Text). Ansichten wechseln: `segmented` (Grund
  `BG`, aktives Segment `SELECTED` mit `LINE_STRONG`-Kante).
- **Menüs** (`widgets::menu`): Fläche `MENU`, Rand `LINE_STRONG`, Radius 8, Schatten, Innenrand 4;
  Einträge 30 hoch mit Icon, Text und rechts Zahl oder Tasten-Hinweis; Abschnitte mit `CAPTION`-
  Titel; Trenner `LINE`.
- **Schwebende Karte** (`widgets::popover`): 320 breit, `MENU`, Radius 10, Innenrand 16, Kopf mit
  Kalender/Liste, Titel `TITLE`, Eigenschaftszeilen, unten Aktionen.
- **Icons** nur aus `torro_gui::icon` (Lucide-SVG), nie Unicode-Glyphen wie ⟳ ‹ › ✓.
- **Seitenleiste:** Zeilen 28 mit Icon 16, Text `BODY` in `TEXT_2` (aktiv `TEXT` auf `SELECTED`),
  Zahl rechts `META` tabellarisch in `TEXT_3`, Tasten-Hinweis als `kbd`. Gruppen „Lists ▾“ als
  `CAPTION` in `TEXT_3` mit 12 Abstand darüber. Listen mit Ring (Aufgaben) bzw. Punkt (Kalender).
- **Aufgabenzeile:** 40 hoch, Kreis 16 (abgehakt gefüllt in Listenfarbe mit Häkchen), Priorität als
  drei Balken (dringend: oranges Quadrat mit „!“), Titel einzeilig, rechts Listen-Etikett (Pille mit
  Punkt) und Fälligkeit `META` tabellarisch (überfällig Attention-Orange). Gruppenköpfe 32 auf
  `RAISED` mit Icon, Name, Zahl und „+“.
- **Termine:** Fläche Kalenderfarbe 18 % (hell 14 %) über `PANEL`, 3-px-Balken links in voller
  Farbe, Titel 12/500 in `TEXT`, Zeit `MICRO` in `TEXT_2`, Radius 4, kein Rahmen; kurze Termine
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
| 4 | **Mittel** | Breite < 1024 | bündig | Kopf 44, Navigation im Menü | Woche + schwebende Karte / Liste + Detail mit Chips |
| 5 | **Breit** | sonst | eingesetzt 8 | Seitenleiste 232 + Kopf 44 + Eigenschaften 300 | Woche + Eigenschaften / Liste + Detail + Eigenschaften |

Regeln:

- **Das Layout wechselt, es schrumpft nicht.** Pro Klasse bewusst entscheiden, was gezeigt wird.
  Schrift nie kleiner als 12, Klickziele nie kleiner als 24.
- Reihenfolge beim Platzsparen: Seitenleiste einklappen (Overlay), Werkzeugleiste auf Icons mit
  Tooltip, Nebeninfos in eine zweite Zeile oder weglassen. Nie abschneiden, nie überlappen, nie
  waagerecht scrollen.
- **Mini und Leiste sind Anzeigen, keine Mini-Apps:** keine Werkzeugleiste, keine Formulare, keine
  Dialoge. Höchstens eine Ein-Klick-Aktion, die den Zweck der Kachel ist (z. B. abhaken). Ruhig,
  aus der Distanz lesbar, Kopfzahl in `DISPLAY`.
- **Groß heißt nicht gestreckt:** Listen und Seiten max. `CONTENT_MAX`, Formulare `FORM_MAX`,
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
