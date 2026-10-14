# Design-Pakete: wie die Suite aussieht, austauschbar

Stand: 2026-10-13. **Entschieden:** Stufe 1 jetzt, Stufe 2 später (welche Schalter, klären wir dann),
Stufe 3 (freie Anordnung über eine Layout-Sprache) nicht. Aufgaben: [tasks/design-pakete.md](tasks/design-pakete.md).
**Ergänzt 2026-10-13:** eigene Farben für die Seitenleiste ([unten](#seitenleiste-mit-eigenen-farben), L.6), damit
Pakete wie Commitr und Ribbon (Issue #80) eine farbige Leiste neben hellem Inhalt haben können.

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
card_gap = 12                  # Abstand zwischen Karten und innerhalb einer Karte (E8.2)
avatar = 36                    # Avatar einer Karte; avatar_lg 40 im Kopf des Lesers
tile = 28                      # Quellen-Kachel; tile_sm 20 neben Wörtern, tile_lg 36 für Anhänge

[colors.dark]                  # nur, was abweicht; Namen wie torro_gui::theme::Palette
accent = "#0EA5A4"
on_accent = "#04201F"
bg = "#0B0D10"
chart = ["#3B78D8", "#C9701E", "#2E9E6B", "#B5509A"]  # optional: Diagrammfarben in fester Reihenfolge (E8.8b)
chart_other = "#5C6068"        # optional: Grau für „Other“ im Diagramm

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
| Kontrast Bedienelemente | `focus_ring`, `merged`, Kalenderfarben, Diagrammfarben (`chart`, `chart_other`) ≥ 3:1 auf `panel`; Statusfarben ≥ 3:1 auf `panel` im dunklen Modus, `attention_text` und `error_text` ≥ 4,5:1 auf allen Flächen in beiden Modi; `line_selected` mindestens dreimal so deutlich wie `line_strong` und schwächer als `focus_ring` |
| Akzent | `on_accent` auf `accent` ≥ 4,5:1 (Text auf dem Primärknopf) |
| Seitenleiste | `sidebar_text`, `sidebar_text_2`, `sidebar_text_3` ≥ 4,5:1 auf `sidebar`, `sidebar_hover`, `sidebar_selected` (auch mit zurückgefallenen Werten). Hat die Leiste eine eigene Fläche (`sidebar` ≠ `bg`): `focus_ring`, `accent`, `merged` und Kalenderfarben ≥ 3:1 auf `sidebar`, Statusfarben im dunklen Modus ebenso. Ohne eigene Fläche gilt für Grafik nichts Neues, ältere Pakete bleiben gültig |
| Schrift | jede Stufe 11–40 groß; Zeilenhöhe ≥ Größe und ≤ 2 × Größe; `body` 12–18; `read` von `body` bis 20; jedes genutzte Gewicht hat eine Datei in der genutzten Schrift; `regular` ist Pflicht für `[fonts.text]` |
| Layout-Abstände | `inset` 0–24; Innenränder 0–48; `section_gap` 8–64; `item_gap` 2–24 |
| Abstände | `space` aufsteigend, 2–64; `radius` aufsteigend, 0–16 |
| Maße | Bedienelemente (`control_sm` …) ≥ 24 (Klickziel); `row` 32–64; `row_two` 32–96; `row_message` 72–120 und `row_notice` 48–96 (Zeilen des Eingangs); `row_sidebar` 24–40; `header` 36–64; `sidebar` 180–320; `props`, `panel`, `list` 280–520; `content_max` 560–1200; Karten (E8.2): `card_gap` 4–32, `avatar` und `avatar_lg` 24–64, `tile_sm` 16–32, `tile` 20–48, `tile_lg` 24–64, Avatare und Kacheln von klein nach groß wachsend |
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
  Stufe 1 steht. Kandidaten: Seitenleiste links/rechts/als Reiter, Listen als Zeilen oder Karten, Dichte, Detail
  daneben/darüber, Kopfleiste mit Text oder nur Icons.
- **Später, Herunterladen:** Ein Katalog (z. B. ein Git-Repo mit Paketen), „Install“ lädt, prüft und legt ab. Erst
  wenn es Pakete von anderen gibt.
