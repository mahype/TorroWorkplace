---
name: torro-design
description: Design-Regeln für alle Oberflächen von TorroWorkplace (Iced-Views, Widgets, Theme, Tray-Menüs, Benachrichtigungen). Laden, bevor UI gebaut, geändert oder begutachtet wird – Farben, Abstände, Typografie, Zustände, Barrierefreiheit, Texte, Sichtprüfung per Screenshot.
---

# Torro-Design

Stand 2026-10-11: Das Fundament (Schrift, Tokens, Stile, Bausteine) entsteht gerade mit
`docs/tasks/design.md` (F.1–F.6). Die Werte unten gelten ab sofort als Ziel. Gibt es einen Token
oder Baustein noch nicht, ist das kein Freibrief für Einzelwerte: Aufgabe aus `design.md` nennen
oder nachfragen.

## Das Ziel – an jeder Ansicht zu messen

1. **Professionell und modern.** Jede Ansicht wirkt wie aus einer Hand und auf dem Niveau von
   Linear, Things 3, Fantastical, Notion Calendar, Amie und Raycast: ruhig, klar, dicht genug für
   Arbeit, nie überladen, nichts wirkt zufällig oder wie Iced-Standard.
2. **Schöne, gut lesbare Schrift.** Inter in festen Stufen (Abschnitt 3), saubere Hierarchie.
3. **Abstände immer richtig.** Jeder Abstand und jedes Maß stammt aus den Tokens (Abschnitt 2),
   gleiche Dinge haben überall gleiche Abstände, alles liegt auf einer Achse.
4. **In jeder Größe gut.** Die Apps laufen dauerhaft als Kacheln, von der Mini-Kachel bis zum
   halben 32:9-Monitor. Jede Größenklasse hat ein eigenes, durchdachtes Layout (Abschnitt 6).

Ist eines der vier Ziele verfehlt, ist die Ansicht nicht fertig – auch wenn alle Tests grün sind.

## 1. Tokens statt Einzelwerte

- **In Apps keine Zahlen** in `.size()`, `.spacing()`, `.padding()`, `.width()`/`.height()`,
  `.rounded()` und kein `Color::from_rgb…`: nur Tokens aus `torro_gui::{theme, typo}`.
  **Keine Iced-Standardstile** (`button::primary`, `button::secondary`, Standard-Checkbox …): nur
  `torro_gui::style` und die Bausteine aus `torro_gui::widgets`. `scripts/design-check.sh` prüft das
  (F.6). Fehlt ein Token oder Stil: in `torro-gui` ergänzen (eigene Aufgabe/Rückfrage), nie lokal.
- Markenfarben kommen aus `torro-design/tokens/tokens.json` → `torro-core/src/theme.rs`; die
  neutralen Flächen und Textstufen stehen in `torro-gui/src/theme.rs`. (Das Repo `torro-design`
  liegt nicht im Workspace; Änderungen dort als Rückfrage melden.)

### Farben dunkel (Standard)

| Token | Wert | Wofür |
|---|---|---|
| `L0` | `#101114` | Fenstergrund |
| `L1` | `#16171B` | Seitenleiste, Gruppen, Dialoge, Schnelleingabe |
| `L2` | `#1D1F24` | Hover, Grund von Eingabefeldern, `secondary`-Knopf |
| `L3` | `#24262C` | Auswahl, gedrückt, aktives Segment |
| `TEXT` | `#EDEEF0` | Text |
| `TEXT_2` | `#A0A3AB` | Sekundärtext, Metadaten, Zeitachsen |
| `TEXT_3` | `#8A8E97` | Platzhalter, Nebenmonate, leise Metadaten (≥ 4.5:1 auf L0–L3) |
| `FAINT` | `#3A3D44` | nur Dekor und Deaktiviertes, **nie** Text |
| `LINE` | Weiß 7 % | Trennlinien, Raster (immer 1 logischer Pixel) |
| `LINE_STRONG` | Weiß 12 % | Rahmen von Eingabefeldern, Linie neben dem Detail |
| `FOCUS_RING` | Weiß 35 % | Fokus, 2 px, 1 px Abstand |
| `NOW` | Weiß | Jetzt-Linie |
| `ACCENT` | Torro-Rot `#D50C0C`, Hover `#A50A0A` | **nur** Primärknopf-Fläche und Heute-Kreis |

Hell (später, gleiche Namen): `L0 #FFFFFF`, `L1 #F6F6F7`, `L2 #EFEFF1`, `L3 #E6E7EA`,
`TEXT #18191B`, `TEXT_2 #4F535A`, `TEXT_3 #62666D`, `LINE` Schwarz 8 %.

### Akzent und Status

- **Torro-Rot ist Marke, nie Status, nie Auswahl.** Pro Fenster höchstens: ein Primärknopf und
  der Heute-Kreis. Aktiver Reiter, gewählte Seitenleistenzeile, ausgewählte Zeile: neutral
  (`L3` + `TEXT`), wie bei Linear und macOS.
- Status: Fehler `STATUS_ERROR`, Warten/Abweichung/überfällig Attention-Orange, OK Grün, inaktiv
  Grau – immer mit Wort oder Icon, Farbe nur im Punkt bzw. Icon.
- Kalender- und Listenfarben gehören dem Nutzer: als Farbbalken, Punkt oder Häkchen, nie als
  großflächige gesättigte Fläche.

## 2. Abstände, Maße, Radien, Linien

- **Abstände** `space::XS 4 · S 8 · M 12 · L 16 · XL 24 · XXL 32` (48 nur für große Leerzustände).
  2, 6, 10, 18, 20 gibt es nicht.
- Label → Feld 4, innerhalb einer Gruppe 8, Gruppe → Gruppe 16–24. Nähe zeigt Zugehörigkeit.
- **Maße:**

| Token | Wert | Wofür (Vorbild) |
|---|---|---|
| `CONTROL_SM` | 28 | Werkzeugleiste, Segmente, Chips (Linear) |
| `CONTROL` | 32 | Knöpfe, Eingaben, Schnelleingabe (macOS, Things) |
| `CONTROL_LG` | 36 | Primärknopf im Dialog, Schmal-Modus |
| `ROW` | 32 | Listenzeile (Things, Todoist) |
| `ROW_SIDEBAR` | 28 | Seitenleistenzeile (Linear, macOS) |
| `HEADER` | 48 | Kopfleiste (Linear, Notion Calendar) |
| `SIDEBAR` | 240 | Seitenleiste (nur Breit; sonst eingeklappt) |
| `CONTENT_MAX` | 720 | Listen, Agenda, Einstellungsseiten |
| `FORM_MAX` | 640 | Formulare, Detailansichten |

- Knöpfe: Höhe `CONTROL`, Innenabstand waagerecht 12, Icon-Knöpfe quadratisch.
- **Radien** `radius::SM 4` (Chips, Terminblöcke) · `MD 6` (Knöpfe, Eingaben, Zeilen, Segmente) ·
  `LG 10` (Gruppen, Karten) · `XL 14` (Dialoge, Popover) · voll rund für Punkte, Avatare, Heute.
  Verschachtelt: innen = außen − Innenabstand.
- **Linien:** lieber Fläche und Abstand als Rahmen. Raster nur mit Innenlinien (keine Rahmen je
  Zelle, keine Doppellinien). Gruppen sind Flächen (`L1`) ohne Rahmen.

## 3. Typografie

- **Schrift: Inter 4** (SIL OFL), mit der App ausgeliefert (`torro_gui::fonts()`,
  `default_font(theme::FONT)`, Standardgröße 14). Nie `Font::DEFAULT` – das ergibt je nach System
  Liberation Sans oder Arial. Achtung Iced: ohne gesetzte Standardgröße ist alles ohne `.size()`
  16 groß.
- **Stufen** (`torro_gui::typo`), Zeilenhöhe immer absolut:

| Stufe | Größe / Zeile | Gewicht | Wofür |
|---|---|---|---|
| `CAPTION` | 12 / 16 | 500 | Abschnittstitel (Satzschreibung, `TEXT_2`), Badges |
| `META` | 12 / 16 | 400 | Metadaten, Zeiten in Zellen, Achsen |
| `BODY` | 14 / 20 | 400 | Standard: Listen, Eingaben, Fließtext |
| `BODY_STRONG` | 14 / 20 | 500 | Zeilentitel, Knöpfe, Termintitel |
| `TITLE` | 16 / 22 | 600 | Dialog-, Gruppen-, Detailtitel |
| `HEADLINE` | 20 / 26 | 600 | Ansichtstitel („October 2026“, „Today“) |
| `DISPLAY` | 28 / 32 | 600 | Kopfzahl in Mini (Uhrzeit, Zahl offener Aufgaben) |

- Gewichte nur 400/500/600; kein Bold, keine Kursive. Hierarchie über Stufe *und* Farbe
  (`TEXT` → `TEXT_2` → `TEXT_3`).
- **Keine Versalien** für Abschnittstitel: Iced kann keine Laufweite setzen, ungesperrte
  Versalien wirken gedrängt. Satzschreibung in `CAPTION` (wie Linear, macOS).
- **Ziffern, die sich ändern** (Uhrzeiten, Achsen, Zähler, Daten in Spalten): `FONT_TABULAR`, damit
  nichts springt und Spalten bündig sind.
- Lange Texte in Zeilen einzeilig mit Auslassung; in Detailansichten umbrechen.

## 4. Kontrast und Barrierefreiheit (WCAG 2.2 AA)

- Text ≥ 4.5:1, große Schrift (≥ 18) und Bedienelemente/Icons ≥ 3:1.
- Gemessen (dunkel): `TEXT` 16.3 auf L0; `TEXT_2` 7.5 auf L0, 6.0 auf L3; `TEXT_3` 5.8 auf L0,
  4.6 auf L3; Weiß auf Torro-Rot 5.4; Torro-Rot auf L0 nur 3.5 → Rot nie als kleiner Text;
  `FAINT` 1.7 → nie Text, auch nicht Platzhalter.
- Information nie nur über Farbe: Status zusätzlich mit Text, Icon oder Form.
- Alles per Tastatur bedienbar: Tab-Reihenfolge logisch, `Enter` bestätigt, `Escape`
  schließt/bricht ab, Pfeiltasten in Listen und Rastern, Kurzbefehle für die Hauptaktionen.
  Fokus sichtbar (`FOCUS_RING`, nicht Rot – Rot liest sich wie ein Fehler).
- Klickziele mindestens 24×24, sonst 28–32.

## 5. Zustände und Bausteine

- **Jedes Bedienelement** hat fünf Zustände: normal; Hover = eine Ebene heller (L1→L2); gedrückt =
  L3; Fokus = `FOCUS_RING`; deaktiviert = 40 % Deckkraft, keine Hover-Fläche.
- **Knöpfe** (`style::button`): `primary` (Rot, weiß, `BODY_STRONG`, höchstens einer pro Fenster),
  `secondary` (L2, `TEXT`), `ghost` (transparent, Hover L2 – Standard für Werkzeugleisten),
  `danger` (ghost mit `STATUS_ERROR`-Text). Umschalter zwischen Ansichten: `segmented`
  (Grund L1, aktives Segment L3).
- **Icons** nur aus `torro_gui::icon` (Lucide-SVG), nie Unicode-Glyphen wie ⟳ ‹ › ✓.
- **Häkchen:** Aufgaben rund 18 px, Rahmen `TEXT_3`, abgehakt gefüllt in Listenfarbe (Things,
  Erinnerungen). Kalender-Auswahl quadratisch `SM` in Kalenderfarbe.
- **Termine:** Fläche Kalenderfarbe 18 %, 3-px-Balken links in voller Farbe, Titel `META`/500 in
  `TEXT`, Zeit `META` in `TEXT_2`, Radius `SM`, kein Rahmen; vergangene 55 % Deckkraft (Notion
  Calendar, Amie, Fantastical). **Heute** = roter Kreis mit weißer Zahl; **Jetzt** = `NOW`-Linie
  1 px mit 6-px-Punkt.
- **Einstellungsseiten:** nur `widgets::group` (Titel, Zeilen 44–56 mit Trennlinien, erklärender
  Satz darunter in `TEXT_2`) – keine eigenen Karten. Eine sichtbare Aktion je Zeile, der Rest im
  „…“-Menü.
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
| 3 | **Schmal** | Breite < 600 | 12 | Kopf 48 mit Icon-Knöpfen | Tag oder Agenda / Liste, Detail ersetzt die Liste |
| 4 | **Mittel** | Breite < 1024 | 16 | Kopf 48 | Woche / Liste + Detail als Overlay |
| 5 | **Breit** | sonst | 24 im Inhalt | Kopf 48 + Seitenleiste 240 | Woche + Seitenleiste / Liste + Detail daneben |

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
