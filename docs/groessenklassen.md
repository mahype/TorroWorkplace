# Größenklassen und Layout-Schwellen

Die Apps von TorroWorkplace laufen dauerhaft als Kacheln eines Tiling-Managers, von der winzigen
Kachel bis zum 32:9-Monitor. Welches Layout eine Ansicht zeigt, hängt deshalb an ihrer Größe. Diese
Seite sammelt alle Schwellen an einer Stelle. Was jede Klasse zeigen darf (Chrom, Ränder, Beispiele
je App), steht in den Design-Regeln: [`.claude/skills/torro-design/SKILL.md`](../.claude/skills/torro-design/SKILL.md),
Abschnitt 6 „Responsiv für Kachel-Fenster“.

Stimmt eine Angabe hier nicht mit dem Code überein, gilt der Code. Die Zeilennummern beziehen sich auf
den Stand, an dem diese Seite geschrieben wurde.

## Die Regel

**Schwellen gibt es nur über `torro_gui::layout::of(size)`.** Views vergleichen keine eigenen Breiten
oder Höhen, um eine Klasse zu bestimmen, sondern fragen `layout::of` und verzweigen nach der
`SizeClass`. Die Schwellen sind gemeinsame Konstanten, damit Views und Tests dasselbe meinen; Design-Pakete
ändern sie nicht (`crates/torro-gui/src/layout.rs:1-5`).

Eine Ansicht darf innerhalb einer Klasse feiner umbrechen (mehr Spalten, Detail neben der Liste), wenn
dafür eine benannte, getestete Schwelle auf der **Inhaltsbreite** besteht, wie in den Abschnitten unten.
Eine neue solche Schwelle bekommt eine Konstante oder Funktion mit Doc-Kommentar und einen Test, und wird
hier nachgetragen.

## Grundlagen

- **Logische Pixel.** Alle Klassen rechnen in logischen Pixeln (`crates/torro-gui/src/layout.rs:49`,
  `:64`). Ein Monitor mit 7680 × 2160 bei Skalierung 2 hat also 3840 × 1080.
- **Der Zoom wirkt vorher.** Der Zoom der Suite (Strg + Plus/Minus) skaliert das ganze Fenster über Iceds
  `scale_factor`; die Größen, die Iced meldet, sind logische Pixel *nach* dem Zoom
  (`crates/torro-gui/src/zoom.rs:6-9`). Die Klassen schalten deshalb wie Breakpoints im Browser: bei 150 %
  ist ein 1440 breites Fenster 960 logische Pixel breit, also Mittel. Stufen: 67, 75, 80, 90, 100, 110,
  125, 150, 175 und 200 % (`crates/torro-core/src/settings.rs:2835`). Eine gemerkte Größe rechnet
  `zoom::rescale` beim Zoomwechsel um (`crates/torro-gui/src/zoom.rs:291`).
- **Mindestgröße der Fenster: 200 × 120**, damit Mini und Leiste überhaupt möglich sind. Gesetzt in der
  Funktion `torro_gui::window` (`crates/torro-gui/src/lib.rs:136`; es gibt kein Modul `window.rs`).

## Die Schwellen in `layout.rs`

| Konstante | Wert | Zeile | Bedeutung |
|---|---|---|---|
| `MINI_MAX_WIDTH` | 360 | `crates/torro-gui/src/layout.rs:35` | Breite darunter: Mini |
| `STRIP_MAX_HEIGHT` | 280 | `crates/torro-gui/src/layout.rs:39` | Höhe darunter: flach, also Leiste oder Mini |
| `NARROW_MAX_WIDTH` | 600 | `crates/torro-gui/src/layout.rs:43` | Breite darunter: Schmal; ab hier kann eine flache Kachel Leiste sein |
| `MEDIUM_MAX_WIDTH` | 1024 | `crates/torro-gui/src/layout.rs:47` | Breite darunter: Mittel, ab hier Breit |

Alle Vergleiche sind „kleiner als“: 360 breit ist schon Schmal, 600 schon Mittel, 1024 schon Breit,
280 hoch nicht mehr flach.

## Die Klassen und ihre Prüfreihenfolge

`layout::of` (`crates/torro-gui/src/layout.rs:71-85`) prüft die Regeln in dieser Reihenfolge, die
erste passende gewinnt (dokumentiert in `:64-70`):

| # | Klasse (`SizeClass`) | Bedingung | Was das Fenster zeigt |
|---|---|---|---|
| 1 | **Leiste** (`Strip`) | Höhe < 280 **und** Breite ≥ 600 | flacher, breiter Streifen: Zeitstrahl oder eine Reihe, zum Ablesen |
| 2 | **Mini** (`Mini`) | Breite < 360 **oder** Höhe < 280 | Blick auf eine kleine Kachel: das Wichtigste, aus der Distanz lesbar, keine Bearbeitung |
| 3 | **Schmal** (`Narrow`) | Breite < 600 | eine Spalte mit voller Bedienung; die Seitenleiste nur als Schublade bzw. Menü |
| 4 | **Mittel** (`Medium`) | Breite < 1024 | volle Ansicht; Seitenleiste immer als Icon-Leiste (60 px) |
| 5 | **Breit** (`Wide`) | sonst | volle Ansicht mit Kontext: Seitenleiste 240 px (oder nach Wahl die Leiste), Eigenschaften-Spalte 300 px |

Die Reihenfolge entscheidet bei flachen Kacheln: Regel 1 braucht mindestens 600 Breite, eine flache,
schmale Kachel (z. B. 359 × 200) ist deshalb Mini, nicht Leiste. Ebenso ist 599 × 279 Mini.

Woher die Maße der Seitenleiste kommen:

- Welche Seitenleiste neben dem Inhalt steht, entscheidet `shell::sidebar_mode`
  (`crates/torro-gui/src/shell.rs:188-194`): Breit nach Wahl des Nutzers, Mittel immer die Icon-Leiste,
  Schmal, Mini und Leiste keine (nur als Schublade).
- Die Maße stammen aus dem aktiven Design, im Standard `designs/torro/design.toml`: Seitenleiste 240
  (`:142`), Eigenschaften 300 (`:143`), Icon-Leiste 60 (`:160`). Ein Design-Paket kann diese Maße
  ändern, die Klassenschwellen nicht.

## Schwellen auf der Inhaltsbreite

Innerhalb einer Klasse brechen manche Ansichten nach der **Inhaltsbreite** um, also der Breite neben
Seitenleiste oder Icon-Leiste. Sie liefert `shell::content_width(class, choice, width)`
(`crates/torro-gui/src/shell.rs:200`). Views, die eigene Spalten schalten, vergleichen diese Breite und
nicht die des Fensters, damit eine Spalte nicht springt, während die Seitenleiste Platz nimmt oder
abgibt. Diese Schwellen ergänzen `layout::of`, sie ersetzen es nicht.

| Schwelle | Wert (Standard-Design) | Ort | Wirkung |
|---|---|---|---|
| `layout::wide_column_from()` | 1011 = 240 + 1 + 24 + 24 + 720 + 2 | `crates/torro-gui/src/layout.rs:149-152`, Test `:200` | ab hier passt eine Spalte so breit wie die Seitenleiste links auf die Seite, ohne die Lesebreite zu kürzen; darunter wird sie zur Icon-Leiste (#248). Genutzt z. B. für die Konten-Seiten (`crates/torro-settings-ui/src/accounts/mod.rs:2154`). Eine Funktion, weil die Maße aus dem Design kommen. |
| `THREE_COLUMNS_FROM` (Aufgaben) | 1320 | `crates/torrotasks/src/app.rs:483`, Anwendung `:625`, Test `:5055` | nur in Breit: Liste, Detail und Eigenschaften-Spalte nebeneinander; Mittel hat nie drei Spalten |
| `list_detail_from()` (Aufgaben) | 667 = 364 + 300 + 3 | `crates/torrotasks/src/app.rs:489` | darunter ersetzt das Detail die Liste, statt daneben zu stehen |
| `widgets::page_column_count` / `page_columns` | 2 Spalten ab 2 × 640 + 24 = 1304, 3 ab 3 × 640 + 2 × 24 = 1968 | `crates/torro-gui/src/widgets.rs:7150`, `:7185`, Test `page_columns_break_at_form_widths` | unabhängige Blöcke von Einstellungs- und Übersichtsseiten; nie mehr Spalten als Blöcke. Geht nach `size::form_max()` und `layout::section_gap()`, folgt also jedem Design. |
| `CONTACTS_LIST_MIN_WIDTH` (Chat) | 280 breit, 280 hoch | `crates/torrochat/src/app.rs:88`, Anwendung `:522-530` | Ausnahme #652: die Kontaktliste zeigt ab 280 × 280 die volle Liste statt der Kurzübersicht. Die Höhe ist `STRIP_MAX_HEIGHT`: `view_class` hebt die Breite auf mindestens `MINI_MAX_WIDTH` und fragt dann `layout::of`. |

## Tests

Die Klassengrenzen sind in `crates/torro-gui/src/layout.rs` getestet (`mod tests`, ab `:184`):

| Test | Zeile | prüft |
|---|---|---|
| `design_example_sizes_get_their_class` | `:220` | jede Größe der Sichtprüfung (`shots::SIZES`) fällt in die Klasse, nach der sie heißt |
| `width_boundary_at_360` | `:235` | 359 Mini, 360 Schmal |
| `width_boundary_at_600` | `:241` | 599 Schmal, 600 Mittel |
| `width_boundary_at_1024` | `:247` | 1023 Mittel, 1024 Breit |
| `height_boundary_at_280_for_width_599` | `:253` | unter 600 Breite ist flach Mini, nicht Leiste |
| `height_boundary_at_280_for_width_600` | `:260` | 600 × 279 Leiste, 600 × 280 Mittel |
| `short_window_below_mini_width_is_mini_not_strip` | `:266` | 359 × 200 bleibt Mini (Regel 1 braucht 600 Breite) |
| `layout_gaps_of_the_standard_design` | `:192` | u. a. `wide_column_from() == 1011` |

Dazu `three_columns_need_a_content_of_1320` in `crates/torrotasks/src/app.rs:5055` und
`page_columns_break_at_form_widths` in `crates/torro-gui/src/widgets.rs:8201`.

## Siehe auch

- [`.claude/skills/torro-design/SKILL.md`](../.claude/skills/torro-design/SKILL.md), Abschnitt 6: was jede
  Klasse zeigt, Ausnahmen (Assistent, Kontaktliste), Seiten ohne Obergrenze; Abschnitt 9: Sichtprüfung in
  allen Größenklassen mit `shots.sh`.
- [`design-pakete.md`](design-pakete.md): welche Maße ein Design-Paket ändern darf.
