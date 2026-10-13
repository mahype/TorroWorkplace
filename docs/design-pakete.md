# Design-Pakete: wie die Suite aussieht, austauschbar

Stand: 2026-10-13. **Entschieden:** Stufe 1 jetzt, Stufe 2 später (welche Schalter, klären wir dann),
Stufe 3 (freie Anordnung über eine Layout-Sprache) nicht. Aufgaben: [tasks/design-pakete.md](tasks/design-pakete.md).

## Worum es geht

Torro ist der Daily Driver für alle Aufgaben. Wer jeden Tag darin arbeitet, soll das Aussehen an sich anpassen
können: andere Farben, eine andere Schrift, mehr oder weniger Luft, ein eigenes Logo. Heute steckt das Design als
feste Werte im Code (`torro_gui::theme`, `typo`), nur hell und dunkel lassen sich umschalten. Künftig beschreibt ein
**Design-Paket** das Aussehen. Die Suite bringt ein Standardpaket mit („Torro“, das heutige Linear-Design), weitere
Pakete legt man in einen Ordner, später auch zum Herunterladen.

## Drei Stufen

| Stufe | Was ein Paket bestimmt | Stand |
|---|---|---|
| **1 Tokens** | Farben (hell und dunkel, auch Akzent und Status), Schriften, Schriftstufen, Abstände, Radien, Maße, Logo | **jetzt** |
| **2 Layout-Schalter** | feste Schalter, die jede App kennt, z. B. Seitenleiste links/rechts/oben, Listen als Zeilen oder Karten, Detail daneben/darüber | später, Auswahl offen |
| 3 Freie Anordnung | eine Layout-Sprache beschreibt beliebige Anordnungen | **nein**: jede Ansicht würde zum Interpreter, nicht prüfbar, kaputte Pakete machten die Apps unbedienbar |

Die Regeln von [torro-design](../.claude/skills/torro-design/SKILL.md) bleiben gültig, nur die **Werte** kommen aus
dem Paket: Ein Paket ändert, *wie* `ACCENT` aussieht, nicht, *wofür* er benutzt wird. Die sechs Größenklassen und
ihre Schwellen bleiben in Stufe 1 fest.

## Ein Paket

Ein Ordner unter `<data>/designs/<id>/` (Datenverzeichnis der Suite, `TORRO_DATA_HOME`). `<id>` ist der Ordnername:
klein, `a-z`, `0-9`, `-`, höchstens 32 Zeichen.

```
designs/plex/
  design.toml        Pflicht: Beschreibung und Werte
  logo.svg           optional: Logo in der Seitenleiste
  tray.svg           optional: Symbol im Tray
  preview.png        optional: Vorschau in der Steuerzentrale (höchstens 1200×800)
  fonts/             optional: Schriftdateien
  fonts/LICENSE.txt  Pflicht, sobald Schriften dabei sind
```

### `design.toml`

```toml
format = 1                     # Version des Formats; unbekannte Version → Paket abgelehnt

[about]
name = "Plex"                  # Pflicht
version = "1.0.0"              # Pflicht
author = "Sven Wagener"        # optional
description = "IBM Plex, more room, teal accent"   # optional
font_license = "OFL-1.1"       # Pflicht, sobald Schriften dabei sind

[fonts.text]                   # Schrift für Fließtext, Listen, Knöpfe; fehlt: Inter
family = "IBM Plex Sans"       # Name der Familie, wie er in den Dateien steht
regular = "fonts/IBMPlexSans-Regular.ttf"      # 400, Pflicht
medium = "fonts/IBMPlexSans-Medium.ttf"        # 500, optional
semibold = "fonts/IBMPlexSans-SemiBold.ttf"    # 600, optional
bold = "fonts/IBMPlexSans-Bold.ttf"            # 700, optional

[fonts.heading]                # optional: eigene Schrift für Überschriften; fehlt: wie text
family = "IBM Plex Serif"
semibold = "fonts/IBMPlexSerif-SemiBold.ttf"

[fonts.numbers]                # optional: Familie mit festen Ziffern (Uhrzeiten, Zähler); fehlt: wie text
family = "IBM Plex Sans Tabular"
regular = "fonts/IBMPlexSansTabular-Regular.ttf"
medium = "fonts/IBMPlexSansTabular-Medium.ttf"

# Jede Schriftstufe vollständig einstellbar: Schrift (text | heading | numbers), Größe,
# Zeilenhöhe, Gewicht (regular | medium | semibold | bold). Nur, was abweicht.
[type.micro]       # Zeitachse, Uhrzeit in Terminen
font = "numbers"
size = 11
line = 14
weight = "regular"
[type.caption]     # Gruppentitel, Knöpfe, Chips
size = 12
line = 16
weight = "medium"
[type.meta]        # Metadaten, Zähler, Fälligkeit
size = 12
line = 16
[type.body]        # Standard: Listen, Seitenleiste, Eingaben, Fließtext
size = 14
line = 22
[type.body_strong] # Zeilen- und Kopfleisten-Titel
size = 14
line = 22
weight = "medium"
[type.title]       # Karten- und Dialogtitel
font = "heading"
size = 17
line = 24
weight = "semibold"
[type.headline]    # Detailtitel
font = "heading"
size = 24
line = 30
weight = "semibold"
[type.display]     # Kopfzahl in Mini
size = 30
line = 36
weight = "semibold"

[space]                        # xs s m l xl xxl
scale = [4, 8, 12, 16, 24, 32]

[radius]                       # xs sm md lg
scale = [3, 5, 7, 9]

# Abstände des Layouts, allgemein (nicht je Element): Rand und Innenrand
[layout]
inset = 8                      # Rand um die Inhaltsfläche neben der Seitenleiste (margin)
page_padding = [24, 32]        # Innenrand von Inhalt und Seiten: senkrecht, waagerecht (padding)
panel_padding = [16, 16]       # Innenrand von Paneelen, Karten, Dialogen
row_padding = [8, 16]          # Innenrand von Listenzeilen
control_padding = [0, 10]      # Innenrand von Knöpfen, Chips, Eingaben
section_gap = 24               # Abstand zwischen Gruppen und Abschnitten
item_gap = 8                   # Abstand zwischen Elementen innerhalb einer Gruppe

[size]                         # nur, was abweicht; Namen wie torro_gui::theme::size
row = 44
row_sidebar = 30
sidebar = 248

[colors.dark]                  # nur, was abweicht; Namen wie torro_gui::theme::Palette
accent = "#0EA5A4"
on_accent = "#04201F"
bg = "#0B0D10"

[colors.light]
accent = "#0B7A79"

[logo]
mark = "logo.svg"              # fehlt: das „T“ des Standardpakets auf der Akzentfarbe
tray = "tray.svg"              # fehlt: das Raster aus vier Quadraten in der Akzentfarbe
```

**Fehlt etwas, gilt der Wert des Standardpakets.** Ein Paket, das nur `colors.dark.accent` setzt, ist gültig. Fehlt
ein eigenes Logo, zeichnet die Suite ihr Logo in der Akzentfarbe des Pakets: Ein neuer Akzent färbt also auch Logo
und Tray mit, nichts bleibt rot hängen.

Abgeleitet und nie im Paket: `attention_text` und `error_text` (aus Status- und Textfarbe, wie heute), die
Iced-Palette, Hover-Stufen der Knöpfe.

**Typografie ist vollständig einstellbar:** bis zu drei Schriften (Text, Überschriften, Ziffern) und für jede der acht
Stufen Schrift, Größe, Zeilenhöhe und Gewicht. Das Standardpaket setzt die heutigen Werte (Inter, Grundgröße 13,
Gewichte 400/500/600). Laufweite und Großschreibung gehen mit Iced 0.14 nicht und sind nicht Teil des Formats.

**Layout-Abstände sind zuerst allgemein:** ein Rand (`inset`) und wenige Innenränder und Lücken nach Rolle (Seite,
Paneel, Zeile, Bedienelement, Gruppe, Element). Die Apps nutzen dafür benannte Tokens statt der Skala, damit ein Paket
sie gezielt ändern kann. Abstände je einzelnem Element gibt es bewusst nicht, damit es am Anfang nicht zu bunt wird.

## Regeln, die jedes Paket einhalten muss

Die Suite prüft ein Paket beim Laden. Verstößt es gegen eine Regel, wird es **ganz abgelehnt** (nie halb
angewendet) und die Steuerzentrale nennt jeden Grund. Die Suite bleibt dann beim bisherigen Design.

| Bereich | Regel |
|---|---|
| Format | `format = 1`; `name` und `version` vorhanden; unbekannte Schlüssel sind ein Fehler (Tippfehler sollen auffallen) |
| Nur Daten | Ein Paket enthält nie Code. Gelesen werden nur `design.toml`, SVG, PNG, TTF/OTF |
| Kontrast Text | `text`, `text_2`, `text_3` ≥ 4,5:1 auf `bg`, `panel`, `raised`, `hover`, `selected`, `menu` (hell und dunkel) |
| Kontrast Bedienelemente | `focus_ring`, `line_selected`, Statusfarben, `merged`, Kalenderfarben ≥ 3:1 auf `panel` |
| Akzent | `on_accent` auf `accent` ≥ 4,5:1 (Text auf dem Primärknopf) |
| Schrift | jede Stufe 11–40 groß; Zeilenhöhe ≥ Größe und ≤ 2 × Größe; `body` 12–18; jedes genutzte Gewicht hat eine Datei in der genutzten Schrift; `regular` ist Pflicht je Schrift |
| Layout-Abstände | `inset` 0–24; Innenränder 0–48; `section_gap` 8–64; `item_gap` 2–24 |
| Abstände | `space` aufsteigend, 2–64; `radius` aufsteigend, 0–16 |
| Maße | Bedienelemente (`control_sm` …) ≥ 24 (Klickziel); `row` 32–64; `row_sidebar` 24–40; `header` 36–64; `sidebar` 180–320; `props`, `panel`, `list` 280–520; `content_max` 560–1200 |
| Dateien | Pfade nur innerhalb des Paketordners, keine Links nach außen; `design.toml` ≤ 64 KB, SVG ≤ 256 KB, PNG ≤ 2 MB, Schrift ≤ 5 MB, Paket ≤ 20 MB |
| SVG | ohne Verweise auf andere Dateien oder Adressen (`href` nur `#…` und `data:`), ohne Skripte |
| Schriften | lesbar als TrueType/OpenType; Familie aus `family` steht in der Datei; Lizenzdatei vorhanden |
| Logo | quadratisch (Seitenverhältnis 1 ± 0,05) |

Was ein Paket **nicht** kann: andere Schwellen der Größenklassen, andere Gewichte, Animationen, Ton, Texte,
Tastenbelegung, Anordnung (Stufe 2).

## Wählen und wechseln

- Gewählt wird in der Steuerzentrale, **General › Appearance**: Liste der Pakete mit Vorschau (Bild oder Farbmuster und
  Schriftprobe), das aktive markiert, ungültige mit ihren Gründen. Dazu „Open designs folder“.
- Der Wechsel wirkt **sofort in allen offenen Fenstern**, wie heute hell/dunkel und der Zoom. Hell/dunkel bleibt eine
  eigene Einstellung: Jedes Paket hat beide Fassungen.
- Ist das gewählte Paket weg oder ungültig, gilt das Standardpaket; die Einstellung bleibt, die Steuerzentrale zeigt
  den Grund.

## API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Pakete auflisten (id, Name, Version, Autor, Beschreibung, Standard ja/nein, gültig, Gründe) | General › Appearance | `settings.list_designs` · Read | `settings_list_designs` |
| Paket wählen | General › Appearance | `settings.set_setting { key: design, value: "<id>" }` · Admin (gibt es) | `settings_set_setting` (gibt es) |
| Gewähltes Paket lesen | – | `settings.get_settings` (Feld `design`) | `settings_get_settings` |
| Paket installieren / entfernen | Ordner öffnen (Stufe 1) | später | später, frühestens Admin mit Freigabe |

Pakete enthalten keine Geheimnisse; Agenten dürfen sie auflisten und (als Admin, mit Freigabe) wählen.

## Technik (Stufe 1)

- **`torro_core::design`:** Modell, Lesen aus dem Ordner, Standardpaket (die heutigen Werte), Zusammenführen mit dem
  Standard, Prüfen. Ohne Iced, damit Kern, Steuerzentrale und Tests es nutzen.
- **`torro_gui`:** Die festen Werte werden zu Werten des aktiven Designs. `space::M` wird `space::m()`,
  `typo::BODY` wird `typo::body()`, `palette(theme)` liefert die Farben des aktiven Designs. Das aktive Design steht
  einmal je Prozess (`torro_gui::design::set`/`active`), gelesen beim Start, ersetzt beim Wechsel. Schriften: beim
  Start in `torro_gui::settings()`, beim Wechsel über `iced::font::load`.
- **Gleiches Bild:** Mit dem Standardpaket sieht jede App aus wie vorher. Das ist die Abnahme der Umstellung.
- **Später, Stufe 2:** Schalter in `design.toml` (Block `[arrangement]`), die jede App liest. Welche, klären wir, wenn
  Stufe 1 steht. Kandidaten: Seitenleiste links/rechts/als Reiter, Listen als Zeilen oder Karten, Dichte, Detail
  daneben/darüber, Kopfleiste mit Text oder nur Icons.
- **Später, Herunterladen:** Ein Katalog (z. B. ein Git-Repo mit Paketen), „Install“ lädt, prüft und legt ab. Erst
  wenn es Pakete von anderen gibt.
