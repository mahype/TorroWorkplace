# Design-Pakete: wie die Suite aussieht, austauschbar

Stand: 2026-10-13. **Entschieden:** Stufe 1 jetzt, Stufe 2 später (welche Schalter, klären wir dann),
Stufe 3 (freie Anordnung über eine Layout-Sprache) nicht. Aufgaben: [tasks/design-pakete.md](tasks/design-pakete.md).
**Ergänzt 2026-10-13:** eigene Farben für die Seitenleiste ([unten](#seitenleiste-mit-eigenen-farben), L.6), damit
Pakete wie Commitr und Ribbon (Issue #80) eine farbige Leiste neben hellem Inhalt haben können.
**Geplant 2026-10-20 (Issue #189):** Design, Schrift und Farbschema getrennt wählbar, jedes aus einem eigenen Ordner
([unten](#design-schrift-und-farbschema-getrennt), L.18–L.30).

## Worum es geht

Torro ist der Daily Driver für alle Aufgaben. Wer jeden Tag darin arbeitet, soll das Aussehen an sich anpassen
können: andere Farben, eine andere Schrift, mehr oder weniger Luft, ein eigenes Logo. Heute steckt das Design als
feste Werte im Code (`torro_gui::theme`, `typo`), nur hell und dunkel lassen sich umschalten. Künftig beschreibt ein
**Design-Paket** das Aussehen. Die Suite bringt ein Standardpaket mit („Torro“, das heutige Linear-Design), weitere
Pakete legt man in einen Ordner, später auch zum Herunterladen.

## Drei Stufen

| Stufe | Was ein Paket bestimmt | Stand |
|---|---|---|
| **1 Tokens** | Farben (hell und dunkel, auch Akzent und Status), Schriften, Schriftstufen, Abstände, Radien, Maße, Logo, Dichte des Eingangs (`layout.density`: Zeilen oder Karten) | **jetzt** |
| **2 Layout-Schalter** | feste Schalter, die jede App kennt, z. B. Seitenleiste links/rechts/oben, Detail daneben/darüber | später, Auswahl offen |
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
  tray.svg           veraltet: wird geprüft, aber nicht mehr gezeigt (das Tray-Symbol ist fest)
  preview.png        optional: Vorschau in der Steuerzentrale (höchstens 1200×800)
  fonts/             optional: Schriftdateien
  fonts/LICENSE.txt  Pflicht, sobald Schriften dabei sind
```

### `design.toml`

Alle Schlüssel des Formats auf einmal (ein ausgedachtes Paket mit drei Schriften); das echte Beispielpaket steht unter
[Ein Paket bauen](#ein-paket-bauen).

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
[type.read]        # Lesetext: ganze Mail im Leser des Eingangs (E8.2); nie kleiner als body
size = 15
line = 24
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
[type.hero]        # Titel einer Erinnerung im Vollbild (ER1.5); fehlt er, wie headline, 48/56
size = 48
line = 56
[type.countdown]   # Countdown im Vollbild, Ziffernschrift; fehlt er, Medium (sonst wie micro), 28/32
size = 28
line = 32

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
density = "compact"            # Zeilen oder Karten im Eingang (E8.2/#226), fehlt er: compact

[size]                         # nur, was abweicht; Namen wie torro_gui::theme::size
row = 44
row_sidebar = 30
sidebar = 248
card_gap = 12                  # Abstand zwischen Karten und innerhalb einer Karte (E8.2)
avatar = 36                    # Avatar einer Karte; avatar_lg 40 im Kopf des Lesers
tile = 28                      # Quellen-Kachel; tile_sm 20 neben Wörtern, tile_lg 36 für Anhänge
command = 46                   # Befehlsleiste des Lesers im Mail-Fenster (E11.1); pill 42 für Torro-Pillen

[colors.dark]                  # nur, was abweicht; Namen wie torro_gui::theme::Palette
accent = "#0EA5A4"
on_accent = "#04201F"
bg = "#0B0D10"
chart = ["#3B78D8", "#C9701E", "#2E9E6B", "#B5509A"]  # optional: Diagrammfarben in fester Reihenfolge (E8.8b)
chart_other = "#5C6068"        # optional: Grau für „Other“ im Diagramm
category = ["#4C8DF0", "#CF8A2C", "#2BA39A", "#A99BD0"]  # optional: je eine Farbe pro Kategoriegruppe (E12.7)

[colors.light]
accent = "#0B7A79"
sidebar = "#24292F"            # optional: Seitenleiste mit eigener Farbe (siehe unten); fehlt: bg
sidebar_text = "#F6F8FA"

[logo]
mark = "logo.svg"              # fehlt: das „T“ des Standardpakets auf der Akzentfarbe
tray = "tray.svg"              # veraltet (Issue #114): das Tray zeigt immer die vier roten Quadrate
```

**Fehlt etwas, gilt der Wert des Standardpakets.** Ein Paket, das nur `colors.dark.accent` setzt, ist gültig. Fehlt
ein eigenes Logo, zeichnet die Suite ihr Logo in der Akzentfarbe des Pakets: Ein neuer Akzent färbt also das Logo
mit. Nur das Tray-Symbol ist fest (Issue #114): immer vier rote Quadrate, damit man es in der Menüleiste unter jedem
Design wiederfindet.

Abgeleitet und nie im Paket: `attention_text` und `error_text` (aus Status- und Textfarbe, wie heute), die
Iced-Palette, Hover-Stufen der Knöpfe.

**Typografie ist vollständig einstellbar:** bis zu drei Schriften (Text, Überschriften, Ziffern) und für jede der neun
Stufen Schrift, Größe, Zeilenhöhe und Gewicht (die neunte, `read`, ist der Lesetext des Eingangs, E8.2). Das Standardpaket setzt die heutigen Werte (Inter, Grundgröße 13,
Gewichte 400/500/600). Laufweite und Großschreibung gehen mit Iced 0.14 nicht und sind nicht Teil des Formats.

**Layout-Abstände sind zuerst allgemein:** ein Rand (`inset`) und wenige Innenränder und Lücken nach Rolle (Seite,
Paneel, Zeile, Bedienelement, Gruppe, Element). Die Apps nutzen dafür benannte Tokens statt der Skala, damit ein Paket
sie gezielt ändern kann. Abstände je einzelnem Element gibt es bewusst nicht, damit es am Anfang nicht zu bunt wird.
Im selben Abschnitt steht die **Dichte** des Eingangs (`density`, Issue #226): `compact` zeigt eine Zeile je Eintrag,
`comfortable` Karten mit Vorschau, Chips und Knöpfen. Sie gehört zum Paket und nicht ins Fenster, damit alle Fenster
der Suite gleich dicht stehen. Kein mitgeliefertes Paket setzt `comfortable`; alle zeigen Zeilen.

### Seitenleiste mit eigenen Farben

Bisher liegt die Seitenleiste auf `bg` und nutzt dieselben Textfarben wie der Inhalt; `text` muss deshalb auf `bg`
*und* `panel` tragen. Eine dunkle Leiste neben hellem Inhalt (wie bei Slack, GitHub oder Office) geht so nicht. Darum
hat jeder Modus optional einen eigenen Satz für die **Rahmenfläche**:

| Schlüssel | Wofür | fehlt → |
|---|---|---|
| `sidebar` | Fläche der Seitenleiste, der Leiste (Rail), der Schublade, des Rands um die Inhaltsfläche (`inset`) und des Titelbands darüber | `bg` |
| `sidebar_hover` | Zeile unter dem Zeiger | `hover` |
| `sidebar_selected` | gewählte Zeile | `selected` |
| `sidebar_text` | Text der gewählten Zeile, Titel | `text` |
| `sidebar_text_2` | Text der Zeilen | `text_2` |
| `sidebar_text_3` | Gruppentitel, Zähler, Datumszahlen außerhalb des Monats | `text_3` |
| `sidebar_line` | Linien in und an der Leiste | `line` |

- **Zurückfallen auf die Werte desselben Pakets**, nicht des Standards: Setzt ein Paket nur `bg`, folgt die
  Seitenleiste seinem `bg`, und es sieht aus wie bisher.
- Was in der Seitenleiste steht, zeichnet sich ohne Zutun der Apps in diesen Farben (Navigation, Gruppen, Mini-Monat
  des Kalenders, Listen mit Farbpunkten, App-Umschalter, Fußzeilen, die Knöpfe im Titelband). Die Shell zeichnet die
  Leiste dafür in einem eigenen Theme; Menüs und Tooltips, die aus ihr aufgehen, bleiben im Theme des Fensters.
- **Eigene Fläche** heißt: `sidebar` ist nicht `bg`. Nur dann werden in der Leiste auch `panel` zu `sidebar`, `raised`
  zu `sidebar_hover` und `line_strong` zu `sidebar_line` (Flächen und Rahmen für den hellen Inhalt trügen die Schrift
  der dunklen Leiste nicht), und nur dann gelten die Regeln für Grafik auf `sidebar`. Akzent, Status- und
  Kalenderfarben bleiben dieselben und müssen auf beiden Flächen tragen.
- Achtungs- und Fehlertext (`attention_text`, `error_text`) leitet die Leiste nach ihrer eigenen Helligkeit ab: Eine
  dunkle Leiste im hellen Modus nimmt die Statusfarben wie der dunkle Modus. Sie werden dort nicht geprüft, denn die
  Leiste zeigt Status als Punkt oder Symbol, und ein Rot, das als Text auf Weiß *und* auf einer dunklen Leiste
  trägt, gibt es kaum.
- Zustände, die auf dem Fenster selbst liegen (Anmelden, Laden, Fehler vor dem Layout), bleiben auf `bg`
  (`shell::Surface::Window`); Seitenleiste und Rail laufen als `shell::Surface::Sidebar` bis ins Titelband.

### Diagrammfarben

Für Diagramme (zuerst die Insights der Inbox-Zentrale, E8.8b) hat jeder Modus vier **Reihenfarben** in fester
Reihenfolge und ein **Grau für „Other“**. Sie gehören nur Diagrammen: Säulen, Balken, Farbfelder in Legende und
Tooltip. Werte, Beschriftungen und Legendentexte bleiben in den Textfarben; Akzent und Statusfarben sind nie
Reihenfarben.

| Schlüssel | Wofür | Standard dunkel | Standard hell | fehlt → |
|---|---|---|---|---|
| `chart` | vier Reihen: Blau, Orange, Grün, Magenta | `#3B78D8` `#C9701E` `#2E9E6B` `#B5509A` | `#2F6BC9` `#B8621A` `#2A9466` `#A8448C` | eigene `calendar`-Farben in der Reihenfolge 1, 3, 2, 4; ohne eigene `calendar` die des Standards |
| `chart_other` | alles, was keine der vier Reihen ist | `#5C6068` | `#50545B` | eigenes `text_3`, wenn das Paket `panel` ändert; sonst das des Standards |
| `category` | vier Gruppen der Mail-Kategorien: wartet auf mich, Belege, Info, Müll | `#4C8DF0` `#CF8A2C` `#2BA39A` `#A99BD0` | `#3D77CE` `#B1741F` `#238A82` `#756990` | die des Standards |

- **Reihenfolge ist Teil der Prüfung.** Die vier Farben wurden als Satz mit dem Dataviz-Validator geprüft
  (Helligkeitsband, Sättigung, Unterscheidbarkeit benachbarter Paare bei Rot-Grün-Schwäche ΔE ≥ 8 und bei normalem
  Sehen ΔE ≥ 15, ≥ 3:1 auf `panel`), das Grau gegen jede der vier. Eine Reihe bekommt ihre Farbe nach dem, was sie ist, nie nach
  ihrem Rang; eine fünfte Reihe wird „Other“, nie eine neue Farbe.
- **Eigene Diagrammfarben** prüft die Suite nur auf 3:1 auf `panel`. Ob sie für Farbenblinde unterscheidbar bleiben,
  misst kein Kontrast: Wer sie setzt, prüft sie in dieser Reihenfolge mit dem Validator, hell und dunkel.
- Warum das Zurückfallen auf eigene Farben: Die Standardfarben sind nur gegen das `panel` des Standards geprüft. Ein
  Paket mit eigenen Kalenderfarben hat sie schon auf seiner Fläche bestehen lassen; ein Paket mit eigener Fläche hat
  ein `text_3`, das dort trägt.

## Regeln, die jedes Paket einhalten muss

Die Suite prüft ein Paket beim Laden. Verstößt es gegen eine Regel, wird es **ganz abgelehnt** (nie halb
angewendet) und die Steuerzentrale nennt jeden Grund. Die Suite bleibt dann beim bisherigen Design.

| Bereich | Regel |
|---|---|
| Format | `format = 1`; `name` und `version` vorhanden; unbekannte Schlüssel sind ein Fehler (Tippfehler sollen auffallen) |
| Nur Daten | Ein Paket enthält nie Code. Gelesen werden nur `design.toml`, SVG, PNG, TTF/OTF |
| Kontrast Text | `text`, `text_2`, `text_3` ≥ 4,5:1 auf `bg`, `panel`, `raised`, `hover`, `selected`, `menu` (hell und dunkel) |
| Kontrast Bedienelemente | `focus_ring`, `merged`, Kalenderfarben, Diagrammfarben (`chart`, `chart_other`) und Kategoriefarben (`category`) ≥ 3:1 auf `panel`; Statusfarben ≥ 3:1 auf `panel` im dunklen Modus, `attention_text` und `error_text` ≥ 4,5:1 auf allen Flächen in beiden Modi; `line_selected` mindestens dreimal so deutlich wie `line_strong` und schwächer als `focus_ring` |
| Akzent | `on_accent` auf `accent` ≥ 4,5:1 (Text auf dem Primärknopf) |
| Seitenleiste | `sidebar_text`, `sidebar_text_2`, `sidebar_text_3` ≥ 4,5:1 auf `sidebar`, `sidebar_hover`, `sidebar_selected` (auch mit zurückgefallenen Werten). Hat die Leiste eine eigene Fläche (`sidebar` ≠ `bg`): `focus_ring`, `accent`, `merged` und Kalenderfarben ≥ 3:1 auf `sidebar`, Statusfarben im dunklen Modus ebenso. Ohne eigene Fläche gilt für Grafik nichts Neues, ältere Pakete bleiben gültig |
| Schrift | jede Stufe 11–40 groß; Zeilenhöhe ≥ Größe und ≤ 2 × Größe; `body` 12–18; `read` von `body` bis 20; `hero` 32–72 und `countdown` 20–48 (Erinnerung im Vollbild, ER1.5); jedes genutzte Gewicht hat eine Datei in der genutzten Schrift; `regular` ist Pflicht für `[fonts.text]` |
| Layout-Abstände | `inset` 0–24; Innenränder 0–48; `section_gap` 8–64; `item_gap` 2–24; `density` nur `compact` oder `comfortable` (ein Wort, kein Bereich: ein anderes lehnt schon das Lesen mit Zeilennummer ab) |
| Abstände | `space` aufsteigend, 2–64; `radius` aufsteigend, 0–16 |
| Maße | Bedienelemente (`control_sm` … `control_xl`) ≥ 24 (Klickziel); `row` 32–64; `row_two` 32–96; `row_message` 72–120 und `row_notice` 48–96 (Zeilen des Eingangs); `row_sidebar` 24–40; `header` 36–64; `sidebar` 180–320; `props`, `panel`, `list` 280–520; `content_max` 560–1200; Karten (E8.2): `card_gap` 4–32, `avatar` und `avatar_lg` 24–64, `tile_sm` 16–32, `tile` 20–48, `tile_lg` 24–64, Avatare und Kacheln von klein nach groß wachsend; Mail-Fenster (E11.1): `command` 32–64 (Befehlsleiste des Lesers), `pill` 28–64 (Torro-Pillen) |
| Dateien | Pfade nur innerhalb des Paketordners, keine Links nach außen; `design.toml` ≤ 64 KB, SVG ≤ 256 KB, PNG ≤ 2 MB, Schrift ≤ 5 MB, Paket ≤ 20 MB |
| SVG | ohne Verweise auf andere Dateien oder Adressen (`href` nur `#…` und `data:`), ohne Skripte |
| Schriften | lesbar als TrueType/OpenType; Familie aus `family` steht in der Datei; Lizenzdatei vorhanden |
| Logo | quadratisch (Seitenverhältnis 1 ± 0,05) |

Was ein Paket **nicht** kann: andere Schwellen der Größenklassen, Laufweite und Großschreibung, Animationen, Ton, Texte,
Tastenbelegung, Anordnung (Stufe 2).

## Ein Paket bauen

Am Beispiel **Plex** (`designs/plex/` im Repository, Aufgabe L.5): IBM Plex Sans, mehr Luft, ein ruhiger
Petrol-Akzent. Die Suite legt ihre Beispiele (Plex, Dracula, Commitr, Ribbon) in den Design-Ordner, der Dienst beim
Start, die Steuerzentrale bei „Open designs folder“. Jedes Beispiel wird **einmal angeboten** (L.10): Die versteckte
Datei `.torro-examples` im Ordner hält fest, welche schon da waren, und eine neue Fassung der Suite legt nur Beispiele
nach, die dort noch nicht stehen und für die es keinen Ordner gleichen Namens gibt. Ein gelöschtes oder geändertes
Beispiel bleibt gelöscht oder geändert; überschrieben wird nie etwas. Die Dateien stecken in `torroworkplace`
(`src/designs.rs`), nicht im Kern, damit nicht jede App die Schriften mitschleppt.

### Schritte

1. **Ordner anlegen:** `<data>/designs/<id>/`, `<id>` klein (`a-z`, `0-9`, `-`). Am schnellsten: Steuerzentrale ›
   Appearance › Design › „Open designs folder“, dort `plex` kopieren und umbenennen.
2. **`design.toml` beginnen:** `format = 1`, `[about]` mit `name` und `version`. Damit ist das Paket schon gültig – es
   sieht aus wie Torro. Ab hier nur eintragen, was abweicht.
3. **Schrift:** TTF/OTF nach `fonts/`, die Lizenz als `fonts/LICENSE.txt`, `font_license` in `[about]`.
   `family` ist der Familienname in der Datei (Plex: „IBM Plex Sans“; die Schnitte Medium und SemiBold tragen ihn als
   typografische Familie, das reicht). Jedes Gewicht, das eine Stufe nutzt, braucht eine Datei; Torro nutzt
   regular, medium und semibold. **Ziffern:** Sind die Standardziffern verschieden breit (Inter), gehört eine Familie
   mit festen Ziffern nach `[fonts.numbers]`. Plex braucht keine: Alle Ziffern sind in allen vier Schnitten gleich
   breit (600 von 1000 Einheiten), `numbers` fällt auf `text` zurück.
4. **Stufen und Abstände:** Plex setzt `body` und `body_strong` auf 14/22, `meta` auf 12/18, `title` 17/24,
   `headline` 24/30, `display` 30/36; Radien `[2, 4, 6, 8]` (eckiger als Torro); `[layout]` inset 10, Seiten 32 × 40,
   Paneele 20 × 20, Zeilen 0 × 20, Knöpfe 0 × 12, Gruppen 32, Elemente 10; `[size]` Zeile 44, zweizeilig 56,
   Seitenleiste 30 und 248 breit, Gruppenkopf 36, Kopfleiste 48, Knöpfe 30/34/40, Stunde und Zeitachse 56.
5. **Farben hell und dunkel:** Flächen (`bg` bis `menu`), Textstufen, Linien, Akzent mit `on_accent`. Plex dunkel:
   `bg #0C1013`, `panel #12171B`, `selected #1B2226`, Text `#E3E8EB` / `#A9B4BB` / `#86929A`, Akzent `#3AA6A0` mit
   dunkler Schrift `#03201E` (5,8:1). Hell: `bg #ECF0F1`, `panel #FFFFFF`, `selected #DEE5E8`, Text `#141B1F` /
   `#475359` / `#59656C`, Akzent `#0C6B66` mit Weiß (6,3:1). Status- und Kalenderfarben bleiben die des Standards.
6. **Logo:** `logo.svg` quadratisch mit `viewBox`, nur Formen, keine Verweise; es steht auf hellem und dunklem
   Grund, also eine eigene Fläche mitbringen (Plex: Petrol-Quadrat mit drei weißen Balken). Ein `tray.svg` braucht es
   nicht mehr: Das Tray-Symbol ist für alle Pakete gleich (Issue #114).
7. **Vorschau:** `preview.png` höchstens 1200 × 800, am besten ein Ausschnitt des eigenen Demo-Fensters im
   Seitenverhältnis 3 : 2 (Plex: 600 × 400, 60 KB).
8. **Prüfen:** Steuerzentrale › Appearance › Design › „Reload“: Das Paket steht in der Liste, ungültig mit jedem Grund
   in einer eigenen Zeile. Ansehen ohne die eigene Einstellung zu ändern: `torrocalendar --demo --design <id>` (ebenso
   `torrotasks`, `torroinbox`, `torroworkplace ui`), mit `--appearance light` und `dark`.

### Häufige Fehler

| Grund in der Steuerzentrale | Abhilfe |
|---|---|
| Unbekannter Schlüssel („Unknown setting …“) | Tippfehler; Namen wie in `torro_gui::theme::Palette` und `size`, z. B. `text_3`, `row_sidebar` |
| `text_3` oder `error_text` unter 4,5:1 auf `selected` | `selected` ist die hellste Fläche im Dunkeln (die dunkelste im Hellen) und fällt zuerst durch. Bei Plex lag das Fehlerrot im Dunkeln erst bei 4,4:1 – `selected` wurde einen Hauch dunkler |
| `on_accent` zu schwach | Heller Akzent → dunkle Schrift darauf, dunkler Akzent → Weiß. Ein Akzent, der im Dunkeln leuchtet, braucht meist dunkle Schrift |
| Kante des Gewählten (`line_selected`) | muss mindestens dreimal so deutlich sein wie `line_strong` und schwächer als `focus_ring`; beim Ändern der Linien alle drei zusammen ansehen |
| Gewicht ohne Datei | Eine Stufe nutzt `semibold`, aber `[fonts.text]` nennt keine SemiBold-Datei |
| Familie stimmt nicht | `family` wie in der Datei, nicht wie der Dateiname („IBM Plex Sans“, nicht „IBMPlexSans“) |
| Schrift ohne Lizenz | `fonts/LICENSE.txt` fehlt oder liegt woanders |
| Logo nicht quadratisch, SVG mit Verweis | `viewBox="0 0 24 24"`; Bilder und Schriften nicht verlinken, sondern als Formen zeichnen |

Nicht als Grund gemeldet, aber im Bild zu sehen: Höhere Zeilen in der Seitenleiste brauchen Platz. Die Seitenleiste
des Kalenders rollt nicht; in niedrigen Fenstern schiebt sie dann „Settings“ unten aus dem Bild (gesehen mit
Zeilen 32 bei 760 Höhe). Darum hat Plex Seitenleistenzeilen 30 statt 32.

## Design, Schrift und Farbschema getrennt

> **Plan zu Issue #189, Stand 2026-10-20.** Entschieden: die Schrift ist eine eigene, dritte Wahl (2026-10-15); drei
> Ordner statt eines Pakets mit Abschnitten, alte Komplettpakete werden einmal kopiert, die Seite Appearance wird
> kompakt mit Kacheln (2026-10-20). Aufgaben L.18–L.30 in [tasks/design-pakete.md](tasks/design-pakete.md). Bis zur
> Umsetzung gilt alles oben unverändert.

### Warum

Heute ist ein Paket immer alles zugleich: Schrift, Abstände, Maße *und* Farben. Von den dreizehn Beispielen sind sechs
(Nord, Solarized, Catppuccin, Gruvbox, Dracula, Everforest) eigentlich nur Farben auf der Form von Torro. Wer die Luft
von Plex mit den Farben von Nord will, Commitr in Dunkelgrün oder Torro in Lato, kann das nicht. Getrennt wählbar wird
aus *d* Designs, *s* Schriften und *f* Farbschemata *d × s × f* Looks, ohne dass jemand sie einzeln baut.

### Begriffe

| Begriff | Was es bestimmt | Englisch (Code, Oberfläche) |
|---|---|---|
| **Design** | Schriftstufen (Rolle, Größe, Zeilenhöhe, Gewicht), Abstände, Radien, Layout-Abstände, Maße; später die Layout-Schalter (Stufe 2) | `Design`, „Design“ |
| **Schrift** | die Familien der drei Rollen Text, Überschrift, Ziffern mit ihren Schnitten, dazu der Größenausgleich | `FontSet`, „Font“ |
| **Farbschema** | alle Farbrollen hell und dunkel (Flächen, Text, Linien, Akzent, Status, Kalender, Diagramm, Seitenleiste) und das Logo | `ColorScheme`, „Colour scheme“ |
| **Aussehen** | Design + Schrift + Farbschema + Modus (hell, dunkel, System) | `Appearance`, „Appearance“ (heißt die Seite schon) |

Nicht „Layout“ für das Design: Das Wort gehört schon Stufe 2 und 3. Nicht „Theme“: Bei Iced meint es die Farben,
anderswo alles zusammen.

### Die Schnittstellen zwischen den Teilen

Jeder Teil passt zu jedem anderen, weil sie sich nur über feste **Rollen** treffen:

| zwischen | über | Beispiel |
|---|---|---|
| Design und Schrift | die drei **Schriftrollen** `text`, `heading`, `numbers` und die vier **Gewichte** | Das Design sagt „`title` ist `heading`, semibold, 16/22“; die Schrift sagt, welche Familie `heading` ist |
| Design und Farbschema | die **Farbrollen** von `Palette` | Das Design zeichnet Text in `text_2`; das Farbschema sagt, welche Farbe das ist |
| Schrift und Farbschema | nichts | – |

- **Jede Regel prüft nur einen Teil.** Kontrastregeln vergleichen Farben mit Farben, Maßregeln Maße mit Maßen,
  Schriftregeln Schriftdateien. Darum ist jede Kombination aus gültigen Teilen gültig, und keine Kombination wird
  eigens geprüft.
- **Das muss so bleiben.** Wo zwei Teile doch zusammenkommen (Ziffern in der Zeitachse, siehe unten), prüft jeder Teil
  gegen eine feste Grenze statt gegen den anderen.
- Fehlt einem Teil etwas, gilt wie bisher der Wert von Torro bzw. desselben Teils (Seitenleiste, Diagramm).

### Damit jede Schrift in jedes Design passt

Schrift, Größen und Abstände waren bisher ein abgestimmter Satz: Plex ist kleiner gezeichnet als Inter und brauchte
deshalb Text in 14 statt 13. Getrennt übernimmt das der Kern mit drei Regeln:

1. **Größen eines Designs meinen Inter.** Alle Schriftgrößen im Design sind für Inter gedacht, die Bezugsschrift.
2. **Größenausgleich der Schrift.** Jede Schrift hat einen Faktor, mit dem die Größen des Designs malgenommen werden,
   damit ihre Kleinbuchstaben so groß wirken wie die von Inter (wie `font-size-adjust` in CSS). Von selbst berechnet
   aus der x-Höhe der Textschrift (OS/2 `sxHeight`, über `ttf-parser`): `x-Höhe Inter ÷ x-Höhe Schrift`, begrenzt auf
   0,90–1,15. Ein Paket kann ihn mit `[fonts] size_adjust` (0,85–1,25) selbst setzen. Wirksame Größe = Größe ×
   Faktor, auf 0,5 gerundet, nie unter 11; wirksame Zeilenhöhe = die des Designs, mindestens aber 1,2 × wirksame
   Größe.
3. **Fehlende Gewichte werden ersetzt.** Nutzt das Design ein Gewicht, das die Schrift nicht hat, nimmt die Suite das
   nächste: medium → semibold → regular, semibold → bold → medium, bold → semibold. Lato und Selawik ohne Medium bleiben
   so gültig. Pflicht ist nur `regular` der Textschrift.

**Ziffern in der Zeitachse** sind die eine Stelle, an der Breite zählt: `00:00` in `numbers` muss in `axis` passen.
Darum gilt für jede Schrift eine Höchstbreite der Ziffern (in em, nach Ausgleich), für jedes Design eine Mindestbreite
der Zeitachse für genau diese Höchstbreite. Die Grenze wird in L.19 an den Beispielschriften gemessen (breiteste plus
Spielraum). Breite Schriften kürzen Beschriftungen in Listen eher mit „…“; das ist zu sehen, aber nicht kaputt.

### Warum das Logo zum Farbschema gehört

Die Logos der Beispiele sind Bilder in den festen Farben ihres Schemas (Nord: Berge in Frost-Blau auf Polar Night,
Dracula: violettes Quadrat). Ein violettes Logo über Nord-Farben passt nicht. Ohne eigenes Logo zeichnet die Suite wie
bisher das „T“ auf dem Akzent des Farbschemas. Die Vorschau `preview.png` zeigt die Form und gehört zum Design;
Schriften zeigt die Steuerzentrale als Schriftprobe, Farbschemata als Farbmuster.

### Drei Ordner

Jeder Teil hat einen eigenen Ordner im Datenordner, jeder Eintrag ist ein Unterordner mit einer Datei und `format = 1`:

| Ordner | Datei | bringt | dazu |
|---|---|---|---|
| `designs/<id>/` | `design.toml`: `[about]`, `[type.*]`, `[space]`, `[radius]`, `[layout]`, `[size]` | ein **Design** | `preview.png` |
| `fonts/<id>/` | `font.toml`: `[about]`, `license`, `size_adjust`, `[fonts.text\|heading\|numbers]` | eine **Schrift** | Schriftdateien, `LICENSE.txt` |
| `color-schemes/<id>/` | `color-scheme.toml`: `[about]`, `[colors.light\|dark]` mit Seitenleiste und Diagramm, `[logo]` | ein **Farbschema** | `logo.svg` |

- Jede Datei hat ihr eigenes `[about]` (Name, Version, Autor, Beschreibung). Dieselbe id darf in mehreren Ordnern
  stehen (Commitr in allen dreien). `torro` ist alle drei, eingebaut, ohne Ordner.
- Neu und optional im `[about]` eines Designs: `font = "<id>"` und `color_scheme = "<id>"`, die Schrift und das
  Farbschema, die zum Design gedacht sind. Nur Vorschläge, gewählt wird getrennt.
- Jeder Teil wird für sich geprüft und für sich abgelehnt.
- Ein Tray-Symbol gibt es im neuen Format nicht mehr: Der Tray zeichnet immer das rote Raster (Issue #114).
- In der Liste der Schriften steht der Familienname der Textschrift („IBM Plex Sans“), im Tooltip der Name aus
  `[about]` („Plex“).
- **Beispiele im Repository** liegen genauso in `designs/`, `fonts/` und `color-schemes/` (L.20): Nord, Solarized,
  Catppuccin, Gruvbox, Dracula und Everforest nur als Farbschemata (Dracula und Everforest verlieren `[radius]`); Plex,
  Commitr, Channelr, Guildr, Quattro und Ribbon in allen drei Ordnern, ihre `[type.*]`-Größen auf Inter
  zurückgerechnet (geteilt durch den Ausgleich ihrer Schrift), damit sie mit der eigenen Schrift so groß bleiben wie
  heute. Jeder Ordner hat seine eigene `.torro-examples`; in einem neu angelegten Ordner gelten die ids aus
  `designs/.torro-examples` als schon angeboten, damit ein gelöschtes Beispiel nicht als Farbschema wiederkommt.

### Alte Komplettpakete

Ein `design.toml` von früher in `designs/` kann noch `[fonts.*]`, `[colors.*]`, `[logo]` und `[about] font_license`
tragen. Darum, einmal beim Start, vor dem Nachlegen der Beispiele:

- Die Suite **kopiert** Farben und Logo nach `color-schemes/<id>/` und die Schrift mit ihren Dateien nach
  `fonts/<id>/`, nur wenn der Zielordner fehlt. Die Quelle wird nie geändert. Übernommene ids stehen in
  `designs/.torro-migrated`; ein später gelöschter Zielordner kommt nicht wieder.
- In `designs/` sind die alten Abschnitte danach kein Fehler, sie werden überlesen. Ein `design.toml` ohne
  Form-Abschnitt, das Farben oder Schriften hatte (eine alte Kopie von Nord), erscheint nicht mehr als Design.
- Ein altes Paket, das eine Regel bricht, wird trotzdem kopiert; seine Gründe zeigt dann der neue Ordner.

### Wählen

- Drei Einstellungen: `design` (gibt es), neu `font` und `color_scheme`, alle mit `torro` als Standard.
- **Übernahme alter Einstellungen**, einmal beim Lesen im Dienst (nach der Übernahme der Pakete), für `font` und
  `color_scheme` je für sich, wenn es fehlt: Hat `fonts/` bzw. `color-schemes/` die id des gewählten Designs, wird sie
  gewählt, sonst `torro`; hat `designs/` sie nicht als Design, wird `design` zu `torro`. Commitr →
  Commitr/Commitr/Commitr (sieht aus wie vorher), Nord → Torro/Torro/Nord.
- **Steuerzentrale**, Appearance, kompakt untereinander (Entwurf in Issue #189): **Color mode** als Segment mit
  Symbolen (Bildschirm, Sonne, Mond) · **Design** · **Font** · **Colour scheme** · Interface. Die drei Wahlen sind
  Raster kleiner Kacheln: Design mit `preview.png` oder Farbmuster, Font mit „Aa 09“ in der Schrift, Colour scheme mit
  Streifen aus `bg`, `panel`, `accent`, `text` im aktuellen Modus. Name unter der Kachel, Version und Autor im Tooltip,
  die gewählte umrandet mit Häkchen. Unbrauchbare sind ausgegraut mit Warnsymbol; ein Klick zeigt ihre Gründe unter dem
  Raster. Jede Gruppe hat „Open … folder“ rechts in der Überschrift.
- Ein Design wählen ändert Schrift und Farbschema **nicht**. Schlägt das gewählte Design andere vor als die aktiven,
  trägt deren Kachel die Marke „Suggested“ (Tooltip „Suggested for Commitr“, ein Klick wählt sie).
- Ungültige Teile, Wechsel ohne Wartezeit und Rückfall auf Torro gelten für alle drei wie unten beschrieben. Ein
  Wechsel der Schrift lädt Schriften (`iced::font::load`), ein Wechsel von Design oder Farbschema nicht.

### API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Designs auflisten | Appearance › Design | `settings.list_designs` · Read (gibt es; listet nur `designs/`, neue Felder `font` und `colorScheme` mit den Vorschlägen) | `settings_list_designs` |
| Design wählen | Appearance › Design | `settings.set_setting { key: design }` · Admin (gibt es; eine id ohne Design wird mit Hinweis auf `font`/`color_scheme` abgelehnt) | `settings_set_setting` |
| Schriften auflisten (id, Familie, Paket, Version, Autor, Lizenz, Standard, gültig, Gründe, aktiv) | Appearance › Font | neu `settings.list_fonts` · Read | neu `settings_list_fonts` |
| Schrift wählen | Appearance › Font | `settings.set_setting { key: font }` · Admin | `settings_set_setting` |
| Farbschemata auflisten (id, Name, Version, Autor, Beschreibung, Standard, gültig, Gründe, aktiv) | Appearance › Colour scheme | neu `settings.list_color_schemes` · Read | neu `settings_list_color_schemes` |
| Farbschema wählen | Appearance › Colour scheme | `settings.set_setting { key: color_scheme }` · Admin | `settings_set_setting` |
| Ordner öffnen | „Open … folder“ je Gruppe | – (lokale Aktion der Steuerzentrale wie heute „Open designs folder“) | – |
| Gewählte lesen | – | `settings.get_settings` · Read (Felder `design`, `font`, `colorScheme`) | `settings_get_settings` |

Verträglich nach `docs/api.md`: neue Befehle, neue Schlüssel, neue Felder. Dass `list_designs` reine Schriften und
Farbschemata nicht mehr nennt, ändert den Inhalt, nicht die Form; ein Agent, der `design = "nord"` setzt, bekommt einen
Fehler, der ihn zu `font` oder `color_scheme` schickt, je nachdem, was das Paket mitbringt.

### Technik

- **`torro_core::design`:** `Design` verliert `fonts`, `colors` und `logo`, `About` verliert `font_license`. Neu
  `FontSet { id, about, license, fonts, size_adjust }` und `ColorScheme { id, about, colors, logo }`, je mit
  `standard()`. Neben `dir()` (`designs/`) neu `font_dir()` und `color_scheme_dir()`; je Ordner `load`/`list`,
  `load_font`/`list_fonts`, `load_color_scheme`/`list_color_schemes`. Wirksame Stufen (Größenausgleich, Ersatz
  der Gewichte) rechnet der Kern: `Selection { design, font, scheme }` mit `level(name)`, damit Kern, Steuerzentrale und
  Tests dasselbe sehen.
- **`torro_gui`:** drei aktive Teile statt einem (`design::active()`, `font::active()`, `scheme::active()`).
  `typo::body()` liefert die wirksame Stufe, `theme::font()` die Familie der aktiven Schrift, `palette(theme)` die
  Farben des aktiven Farbschemas. `design::Follow` folgt allen drei Einstellungen. `--demo` bekommt `--font <id>` und
  `--color-scheme <id>` neben `--design`.
- **Gleiches Bild:** Mit Torro/Torro/Torro sieht jede App aus wie vorher (exakt). Mit jedem Beispielpaket als alle drei
  Teile weichen die wirksamen Größen höchstens um 0,5 px von heute ab (Rundung des Ausgleichs).
- **Stufe 2:** Der Kandidat „Dichte“ fällt weg: Sie steht seit Issue #226 als `layout.density` in Stufe 1.

### Mitgelieferte Designs (Vorschlag)

Namen sind Daten des Pakets und werden nicht übersetzt, darum englisch. Designs bringen nur Form, keine Schrift und
keine Farben, schlagen aber eine Schrift vor. Werte gegen Torro (Text 13/20, Zeile 40, Seitenleistenzeile 28, Knopf
32, Kopfleiste 44, Radien 4/6/8/10, Seitenrand 24), Größen für Inter:

| Design | Idee, für wen | Werte (Auswahl) | schlägt vor |
|---|---|---|---|
| **Torro** | Standard, wie heute | – | Inter |
| **Compact** (Kompakt) | viel auf einen Blick: volle Kalender, lange Listen, kleine Laptops | Text 12/16, Zeile 32, zweizeilig 44, Seitenleiste 24 und 216 breit, Knöpfe 24/28/32, Kopfleiste 36, Radien 3/4/6/8, Seitenrand 16, Paneele 12, Gruppen 16, Elemente 4, Stunde 40 | Inter |
| **Airy** (Locker) | ruhiges Arbeiten, große Bildschirme | Text 14/22, Lesetext 16/26, Zeile 48, zweizeilig 60, Seitenleiste 32 und 260 breit, Knöpfe 32/36/40, Kopfleiste 52, Radien 6/8/12/14, Seitenrand 32 × 40, Paneele 20 × 24, Gruppen 32, Elemente 12 | Inter |
| **Clear** (gut lesbar) | schlechte Augen, Abstand zum Bildschirm, Touch | Text 16/24, Lesetext 18/28, Knopfzeile 14, Zeitachse 13, Zeile 52, Seitenleiste 36 und 280 breit, Knöpfe 36/40/44 (Klickziel 44), Kopfleiste 56 | Hyperlegible (und Farbschema High Contrast, wenn es das gibt) |
| **Folio** | viel lesen: Mail, Notizen | Lesetext 16/26, `content_max` 680, Titel und Überschrift in der Rolle `heading`, sonst nahe Torro | Source |
| **Pebble** | weich und freundlich | Radien 8/12/16/16 (Knöpfe als Pillen: 16 bei Höhe 32), Zeile 44, Paneele 20, sonst nahe Torro | Nunito |
| **Square** | eckig und dicht wie ein Terminal | Radien 0/0/2/2, Zeile 36, Seitenleiste 26, Knöpfe 28, Paneele 12 | JetBrains Mono |
| Plex, Commitr, Channelr, Guildr, Quattro, Ribbon | gibt es schon | wie bisher, Größen auf Inter zurückgerechnet | die eigene |

### Mitgelieferte Schriften (Vorschlag)

Alle unter OFL, mit `LICENSE.txt`. Ziffern wie in L.5 prüfen: gleich breit, sonst eine Ziffernschrift dazu.

| Schrift | Familien | wozu |
|---|---|---|
| **Torro** | Inter, Ziffern Inter Tabular | Standard |
| **Hyperlegible** | Atkinson Hyperlegible Next (Braille Institute) | eindeutige Buchstaben (I, l, 1), für Clear |
| **Source** | Source Sans 3, Überschriften Source Serif 4 | Lesen, für Folio |
| **Nunito** | Nunito | runde Strichenden, für Pebble |
| **JetBrains Mono** | JetBrains Mono für alle Rollen | Terminal-Gefühl, für Square |
| Plex, Commitr, Channelr, Guildr, Quattro, Ribbon | IBM Plex Sans, Mona Sans, Lato, Figtree, Roboto Flex, Selawik | gibt es schon in den Paketen |

Zu prüfen beim Bauen: Höhere Seitenleistenzeilen (Airy 32, Clear 36) schieben in niedrigen Fenstern „Settings“ aus der
Seitenleiste des Kalenders (siehe Plex). Clear braucht dort vielleicht eine rollende Seitenleiste, das wäre eine eigene
Aufgabe in `torro-gui`.

**Ideen für weitere Farbschemata**, nicht als Aufgabe: **High Contrast** (Text 7:1, kräftige Linien, für Clear),
**Graphite** (ohne bunten Akzent, nur Grau, zum Konzentrieren), **Paper** (warmes Sepia, für Folio), **Torro Blue**
(Torro mit blauem Akzent für die, denen Rot zu laut ist).

## Wählen und wechseln

- Gewählt wird in der Steuerzentrale, **Appearance**: Liste der Pakete mit Vorschau (Bild oder Farbmuster und
  Schriftprobe), das aktive markiert, ungültige mit ihren Gründen. Dazu „Open designs folder“.
- Der Wechsel wirkt **sofort in allen offenen Fenstern**, wie heute hell/dunkel und der Zoom. Hell/dunkel bleibt eine
  eigene Einstellung: Jedes Paket hat beide Fassungen.
- Ist das gewählte Paket weg oder ungültig, gilt das Standardpaket; die Einstellung bleibt, die Steuerzentrale zeigt
  den Grund.

## API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Pakete auflisten (id, Name, Version, Autor, Beschreibung, Standard ja/nein, gültig, Gründe) | Appearance | `settings.list_designs` · Read | `settings_list_designs` |
| Paket wählen | Appearance | `settings.set_setting { key: design, value: "<id>" }` · Admin (gibt es) | `settings_set_setting` (gibt es) |
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
  Stufe 1 steht. Kandidaten: Seitenleiste links/rechts/als Reiter, Detail daneben/darüber, Kopfleiste mit Text oder
  nur Icons. Nicht mehr dabei: die Dichte und „Listen als Zeilen oder Karten“ – beides ist `layout.density` in
  Stufe 1 (Issue #226).
- **Später, Herunterladen:** Ein Katalog (z. B. ein Git-Repo mit Paketen), „Install“ lädt, prüft und legt ab. Erst
  wenn es Pakete von anderen gibt.
