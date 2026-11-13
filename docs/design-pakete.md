# Design-Pakete: wie die Suite aussieht, austauschbar

Stand: 2026-10-13. **Entschieden:** Stufe 1 jetzt, Stufe 2 später (welche Schalter, klären wir dann),
Stufe 3 (freie Anordnung über eine Layout-Sprache) nicht. Aufgaben: [tasks/design-pakete.md](tasks/design-pakete.md).
**Ergänzt 2026-10-13:** eigene Farben für die Seitenleiste ([unten](#seitenleiste-mit-eigenen-farben), L.6), damit
Pakete wie Commitr und Ribbon (Issue #80) eine farbige Leiste neben hellem Inhalt haben können.
**Geplant 2026-10-20 (Issue #189):** Design, Schrift und Farbschema getrennt wählbar, jedes aus einem eigenen Ordner
([unten](#design-schrift-und-farbschema-getrennt), L.18–L.30).
**Geändert 2026-10-22 (Issue #291):** drei eingebaute Designs **Standard, Compact, Console**, je eine Datei
`designs/<id>/design.toml` im Repository; die sechs Beispiel-Designs entfallen, ihre Schriften und Farbschemata
bleiben. Neue Schlüssel `[list]`, `rail`, `rail_wide`, `reading_max`, `reading_padding`, `[about] mail_view`, im
Farbschema `dark_only`; `layout.density` entfällt (L.31–L.38).
**Geändert 2026-11-06 (Issue #484):** Ein Design regelt nur noch Abstände. Es gibt genau drei eingebaute Stufen
**Komfort, Standard, Dicht**; Compact und Console entfallen (gespeichert werden sie zu Dicht). Eigene Designs aus dem
Ordner `designs/` im Datenordner, `preview.png` und die Vorschläge `[about] font` und `[about] mail_view` entfallen.
Schriften und Farbschemata bleiben in ihren Ordnern ([unten](#drei-abstandsstufen-komfort-standard-dicht)).

## Worum es geht

Torro ist der Daily Driver für alle Aufgaben. Wer jeden Tag darin arbeitet, soll das Aussehen an sich anpassen
können: andere Farben, eine andere Schrift, mehr oder weniger Luft, ein eigenes Logo. Heute steckt das Design als
feste Werte im Code (`torro_gui::theme`, `typo`), nur hell und dunkel lassen sich umschalten. Künftig beschreibt ein
**Design-Paket** das Aussehen. Die Suite bringt ein Standardpaket mit („Torro“, das heutige Linear-Design), weitere
Pakete legt man in einen Ordner, später auch zum Herunterladen.

## Drei Stufen

| Stufe | Was ein Paket bestimmt | Stand |
|---|---|---|
| **1 Tokens** | Farben (hell und dunkel, auch Akzent und Status), Schriften, Schriftstufen, Abstände, Radien, Maße, Logo, Darstellung der Listen an derselben Stelle (`[list]`: mehrzeilig, eine Zeile oder Karten; Haarlinien; Auswahl; Avatar) | **jetzt** |
| **2 Layout-Schalter** | feste Schalter, die jede App kennt, z. B. Seitenleiste links/rechts/oben, Detail daneben/darüber | später, Auswahl offen |
| 3 Freie Anordnung | eine Layout-Sprache beschreibt beliebige Anordnungen | **nein**: jede Ansicht würde zum Interpreter, nicht prüfbar, kaputte Pakete machten die Apps unbedienbar |

Die Regeln von [torro-design](../.claude/skills/torro-design/SKILL.md) bleiben gültig, nur die **Werte** kommen aus
dem Paket: Ein Paket ändert, *wie* `ACCENT` aussieht, nicht, *wofür* er benutzt wird. Die sechs Größenklassen und
ihre Schwellen bleiben in Stufe 1 fest.

## Ein Paket

Ein Ordner im Datenverzeichnis der Suite (`TORRO_DATA_HOME`), heute für eine Schrift (`<data>/fonts/<id>/`) oder ein
Farbschema (`<data>/color-schemes/<id>/`). `<id>` ist der Ordnername: klein, `a-z`, `0-9`, `-`, höchstens 32 Zeichen;
`torro` ist vergeben.

Designs haben seit Issue #484 keinen Ordner mehr: Die Suite liest `<data>/designs/` nicht mehr und zeigt nichts daraus
an. Ein Ordner von vorher bleibt liegen, gelöscht wird nichts. Ein Design ist eine der drei eingebauten
[Abstandsstufen](#drei-abstandsstufen-komfort-standard-dicht).

> Der Baum unten und das Beispiel mit Schriften und Farben in einem `design.toml` zeigen das Format vor Aufgabe L.19.
> Heute stehen Schrift und Farben in eigenen Ordnern ([Drei Ordner](#drei-ordner)); das gültige `design.toml` steht
> im nächsten Abschnitt. Den Ordner `designs/<id>/` und `preview.png` gibt es seit Issue #484 nicht mehr.

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

Ein Design ist **eine Datei**: `designs/<id>/design.toml` im Repository, beim Bauen in `torro-core` eingebettet. Seit
Issue #484 gibt es genau drei: `torro` (Standard), `comfort` (Komfort) und `dense` (Dicht); eigene Designs im
Datenordner gibt es nicht mehr. Jeder Schlüssel ist optional außer `format`, `about.name` und `about.version`; was
fehlt, kommt vom Standard. Komfort und Dicht setzen nur Abstände und Höhen
([Regel](#drei-abstandsstufen-komfort-standard-dicht)). Die Vorlage nennt alle Schlüssel des Formats; ihre Werte sind
Beispiele (die des früheren Compact), die gültigen stehen in den drei Dateien.

```toml
format = 1                     # Version des Formats; unbekannte Version → Design abgelehnt

[about]
name = "Example"               # Pflicht; die Suite übersetzt die Namen der drei Stufen
version = "1.0.0"              # Pflicht
author = "Torro"               # optional
description = "Every key of the format"   # optional
# font = "jetbrains-mono"      # entfallen (#484): unbekannter Schlüssel, ein Design schlägt keine Schrift vor
# mail_view = "text"           # entfallen (#484): unbekannter Schlüssel, die Mail-Darstellung ist eine eigene Einstellung
# color_scheme = "nord"        # entfallen (L.33): unbekannter Schlüssel, Farben gehören nie zu einem Design

# Schriftstufen: Rolle (text | heading | numbers), Größe und Zeilenhöhe für Inter, Gewicht
# (regular | medium | semibold | bold). Eine Stufe, die fehlt, ist die des Standards.
[type.micro]                   # Zeitachse, Uhrzeit in Terminen (tabellarisch)
font = "numbers"
size = 11
line = 14
weight = "regular"
[type.caption]                 # Gruppentitel, Knöpfe, Chips
font = "text"
size = 11
line = 16
weight = "medium"
[type.meta]                    # Metadaten, Zähler, Fälligkeit
font = "text"
size = 12
line = 16
weight = "regular"
[type.body]                    # Listen, Seitenleiste, Eingaben, Fließtext
font = "text"
size = 13
line = 20
weight = "regular"
[type.body_strong]             # Zeilen- und Kopfleisten-Titel
font = "text"
size = 13
line = 20
weight = "medium"
[type.read]                    # Lesetext: die ganze Mail im Leser; nie kleiner als body
font = "text"
size = 14
line = 22
weight = "regular"
[type.title]                   # Karten- und Dialogtitel, Seitentitel
font = "heading"
size = 17
line = 24
weight = "semibold"
[type.headline]                # Detailtitel
font = "heading"
size = 22
line = 28
weight = "semibold"
[type.display]                 # Kopfzahl in Mini
font = "text"
size = 28
line = 32
weight = "semibold"
[type.hero]                    # Titel einer Erinnerung im Vollbild; fehlt er: Rolle und Gewicht von headline
font = "heading"
size = 48
line = 56
weight = "semibold"
[type.countdown]               # Countdown im Vollbild, Ziffernschrift
font = "numbers"
size = 28
line = 32
weight = "medium"

[space]                        # xs s m l xl xxl, wachsend
scale = [4, 8, 12, 16, 24, 32]

[radius]                       # xs sm md lg, nie fallend (gleich ist erlaubt: 4/4/4/4)
scale = [4, 4, 4, 4]

[layout]                       # Ränder [senkrecht, waagerecht] und Lücken nach Rolle
inset = 0                      # Rand um die Inhaltsfläche neben der Seitenleiste; 0 = bündig mit Linie
page_padding = [16, 16]        # Innenrand von Inhalt und Seiten
panel_padding = [12, 12]       # Paneele, Karten, Dialoge
row_padding = [10, 16]         # Listenzeilen (die Höhe kommt aus [size])
control_padding = [0, 8]       # Knöpfe, Chips, Eingaben
section_gap = 16               # zwischen Gruppen und Abschnitten
item_gap = 8                   # zwischen Elementen einer Gruppe
reading_padding = [32, 40]     # Rand der Mail im Leser, innerhalb der Lesespalte

[size]                         # Namen wie torro_gui::theme::size
control_sm = 30                # Knöpfe, Icon-Knöpfe, Segmente
control = 32                   # Eingaben, Eigenschaftszeilen
control_lg = 36                # Schnelleingabe, Primärknopf im Dialog, Knopf der Aktionsleiste
control_xl = 44                # Knöpfe einer Erinnerung im Vollbild
row = 40                       # Listenzeile (Aufgaben)
row_two = 48                   # zweizeilige Zeile
row_message = 76               # Mail-Zeile im Eingang; bestimmt auch die Dichte in der Liste der Designs
row_notice = 56                # andere Zeile im Eingang
row_sidebar = 28               # Seitenleisten- und Menüzeile
row_group = 32                 # Gruppenkopf in Listen
row_contact = 52               # Kontaktzeile (zwei Zeilen: Name, Funktion)
header = 48                    # Kopfleiste
sidebar = 216                  # Seitenleiste
props = 280                    # Eigenschaften-Spalte
panel = 360                    # Seitenpaneel für Details
list = 400                     # Liste neben dem Detail (breit)
list_medium = 340              # dasselbe in Mittel
hour = 40                      # Stunde im Kalender
axis = 52                      # Zeitachse
axis_narrow = 44               # Zeitachse in Schmal
content_max = 720              # Lesebreite von Listen; in Einstellungen die Textbreite einer Zeile
form_max = 640                 # Lesebreite von Formularen; Spalten-Umbrüche der Einstellungen
card_gap = 8                   # zwischen Karten und Teilen einer Karte
avatar = 28                    # Avatar einer Karte
avatar_lg = 32                 # großer Avatar
tile_sm = 20                   # Quellen-Kachel klein, mittel, groß
tile = 24
tile_lg = 32
command = 40                   # bleibt im Format, kein Baustein nutzt es
pill = 36                      # Avatar im Kopf des Lesers
rail = 52                      # eingeklappte Seitenleiste und Aktionsleiste; mindestens control_lg
rail_wide = 232                # ausgeklappte Aktionsleiste
reading_max = 640              # breiteste Lesespalte einer Mail

[list]                         # wie Listen gezeichnet werden – dieselben Zellen an derselben Stelle
style = "rows"                 # rows (mehrzeilig) | line (eine Zeile, feste Spalten) | cards (Karten); nur rows eingebaut
separators = true              # Haarlinie unter jeder Zeile
selection = "bar"              # bar (Strich links) | fill (ganze Zeile in accent, Text in on_accent); nur bar eingebaut
avatar = 28                    # Avatar in der Mail-Zeile und im Kopf des Lesers; 0 = keiner
preview_lines = 1              # Zeilen Vorschau unter dem Betreff, 1 oder 2
```

**Entfallen:** `layout.density` (seit Issue #291 ein unbekannter Schlüssel; Karten sind `[list] style = "cards"`,
`rows` und `line` sind die kompakte Dichte), `[about] color_scheme` (seit L.33 ein unbekannter Schlüssel: Das Design wird
mit Grund abgelehnt) sowie `[about] font` und `[about] mail_view` (seit Issue #484 unbekannte Schlüssel: Ein Design
schlägt nichts mehr vor). Schriften stehen in `fonts/<id>/font.toml`, Farben und Logo in `color-schemes/<id>/color-scheme.toml`.

Die Schlüssel für Farben unten (`[colors.dark]`, `[colors.light]`, Seitenleiste, Diagramm, Lesestatus, Arbeitszeit)
gehören heute ins Farbschema, nicht ins Design.

**Fehlt etwas, gilt der Wert des Standards.** Ein Farbschema, das nur `colors.dark.accent` setzt, ist gültig. Fehlt
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
Die **Dichte** des Eingangs (Issue #226) folgt seit Issue #291 der Darstellung der Listen: `[list] style = "cards"`
zeigt Karten mit Vorschau, Chips und Knöpfen, `rows` und `line` eine Zeile je Eintrag. Sie gehört zum Design und nicht
ins Fenster, damit alle Fenster der Suite gleich dicht stehen. Kein eingebautes Design zeigt Karten.

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
  des Kalenders, Listen mit Farbpunkten, Suche und „Neu“, Fußzeilen, die Knöpfe im Titelband). Die Shell zeichnet die
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

### Farbschema nur dunkel

Ein Farbschema kann nur dunkle Farben haben (Issue #291): `dark_only = true` oben in `color-scheme.toml`, neben
`format`. Dann ist `[colors.light]` ein Fehler (auch leer), die helle Palette ist eine Kopie der dunklen, und jede App
zeichnet das Schema in jedem Modus dunkel. Geprüft wird nur die dunkle Palette. Fehlt der Schlüssel, hat das Schema
beide Modi wie bisher. Die Steuerzentrale ordnet solche Schemata unter „Dark only“ ein (L.37).

```toml
format = 1
dark_only = true

[about]
name = "Night"
version = "1.0.0"

[colors.dark]
bg = "#050607"
```

### Knallige Farbschemata

Ein Farbschema kann sich als knallig kennzeichnen (Issue #515): `vivid = true` oben in `color-scheme.toml`, neben
`format`. Der Schlüssel ändert weder Farben noch Prüfung; er entscheidet nur, in welcher Gruppe die Steuerzentrale das
Schema im Menü Appearance › Farbschema zeigt: „Vivid“ (deutsch „Knallig“), nach „Dark only“. Ein Schema mit
`vivid = true` steht dort auch dann, wenn es zusätzlich `dark_only = true` hat. Fehlt der Schlüssel, bleibt das Schema
in seiner Gruppe. Ein Wert, der kein `true` oder `false` ist (`vivid = "yes"`), ist ein Syntaxfehler.

```toml
format = 1
vivid = true

[about]
name = "Nord Vivid"
version = "1.0.0"
```

### Diagrammfarben

Für Diagramme (zuerst die Insights der Inbox-Zentrale, E8.8b) hat jeder Modus vier **Reihenfarben** in fester
Reihenfolge und ein **Grau für „Other“**. Sie gehören nur Diagrammen: Säulen, Balken, Farbfelder in Legende und
Tooltip. Werte, Beschriftungen und Legendentexte bleiben in den Textfarben; Akzent und Statusfarben sind nie
Reihenfarben.

| Schlüssel | Wofür | Standard dunkel | Standard hell | fehlt → |
|---|---|---|---|---|
| `chart` | vier Reihen: Blau, Orange, Grün, Magenta | `#3B78D8` `#C9701E` `#2E9E6B` `#B5509A` | `#2F6BC9` `#B8621A` `#2A9466` `#A8448C` | eigene `calendar`-Farben in der Reihenfolge 1, 3, 2, 4; ohne eigene `calendar` die des Standards |
| `chart_other` | alles, was keine der vier Reihen ist | `#5C6068` | `#50545B` | eigenes `text_3`, wenn das Paket `panel` ändert; sonst das des Standards |
| `category` | fünf Gruppen der Mail-Kategorien: wartet auf mich, Belege, Info, unerwünscht, nicht eingeordnet (#338) | `#4C8DF0` `#CF8A2C` `#2BA39A` `#A99BD0` `#D9739A` | `#3D77CE` `#B1741F` `#238A82` `#756990` `#B34A6E` | die des Standards |

- **Reihenfolge ist Teil der Prüfung.** Die vier Farben wurden als Satz mit dem Dataviz-Validator geprüft
  (Helligkeitsband, Sättigung, Unterscheidbarkeit benachbarter Paare bei Rot-Grün-Schwäche ΔE ≥ 8 und bei normalem
  Sehen ΔE ≥ 15, ≥ 3:1 auf `panel`), das Grau gegen jede der vier. Eine Reihe bekommt ihre Farbe nach dem, was sie ist, nie nach
  ihrem Rang; eine fünfte Reihe wird „Other“, nie eine neue Farbe.
- **Eigene Diagrammfarben** prüft die Suite nur auf 3:1 auf `panel`. Ob sie für Farbenblinde unterscheidbar bleiben,
  misst kein Kontrast: Wer sie setzt, prüft sie in dieser Reihenfolge mit dem Validator, hell und dunkel.
- Warum das Zurückfallen auf eigene Farben: Die Standardfarben sind nur gegen das `panel` des Standards geprüft. Ein
  Paket mit eigenen Kalenderfarben hat sie schon auf seiner Fläche bestehen lassen; ein Paket mit eigener Fläche hat
  ein `text_3`, das dort trägt.

### Lesestatus eingehender Mail

| Schlüssel | Wofür | Standard dunkel | Standard hell | fehlt → |
|---|---|---|---|---|
| `unread` | blauer Punkt links an ungelesener Mail, unabhängig von Konto und Kategorie | `#66A3FF` | `#2563D8` | der Wert des Standards |

Der Punkt liegt auf Zeilen und Karten. Sein Wert muss deshalb auf `panel`, `raised`, `hover` und
`selected` mindestens 3:1 erreichen. Bei eigenen Flächen kann ein Paket den blauen Wert je Modus
überschreiben; der Punkt teilt seine Farbe nicht mit Kalendern, Diagrammen oder Kategorien.

### Arbeitszeit im Kalender

| Schlüssel | Wofür | Standard dunkel | Standard hell | fehlt → |
|---|---|---|---|---|
| `working_hours` | dünne Linie der Arbeitszeit in Tag, Woche, Zeitstrahl und Radar (K.33) | `#FFFFFF47` (Weiß 28 %) | `#00000052` (Schwarz 32 %) | `line_selected` des Schemas |

Die Linie ist eine Markierung hinter den Terminen, kein Text und kein Bedienelement; für sie gilt
keine Kontrastregel. Im Standard ist sie das neutrale Grau der Kanten von Gewähltem. Lässt ein
Schema den Schlüssel weg, nimmt es sein eigenes `line_selected`, damit ältere Pakete die Linie in
ihren Farben zeichnen. Weder Torro-Rot noch eine Kalenderfarbe: Die Linie darf nicht mit der
Jetzt-Linie oder dem Farbstreifen eines Termins verwechselt werden.

## Regeln, die jedes Paket einhalten muss

Die Suite prüft ein Paket beim Laden. Verstößt es gegen eine Regel, wird es **ganz abgelehnt** (nie halb
angewendet) und die Steuerzentrale nennt jeden Grund. Die Suite bleibt dann beim bisherigen Design. Für die drei
eingebauten Designs prüft ein Test die Regeln beim Bauen.

| Bereich | Regel |
|---|---|
| Format | `format = 1`; `name` und `version` vorhanden; unbekannte Schlüssel sind ein Fehler (Tippfehler sollen auffallen) |
| Nur Daten | Ein Paket enthält nie Code. Gelesen werden nur `design.toml`, SVG, PNG, TTF/OTF |
| Kontrast Text | `text`, `text_2`, `text_3` ≥ 4,5:1 auf `bg`, `panel`, `raised`, `hover`, `selected`, `menu` (hell und dunkel) |
| Kontrast Bedienelemente | `focus_ring`, `merged`, Kalenderfarben, Diagrammfarben (`chart`, `chart_other`) und Kategoriefarben (`category`) ≥ 3:1 auf `panel`; `unread` ≥ 3:1 auf `panel`, `raised`, `hover` und `selected`; Statusfarben ≥ 3:1 auf `panel` im dunklen Modus, `attention_text` und `error_text` ≥ 4,5:1 auf allen Flächen in beiden Modi; `line_selected` mindestens dreimal so deutlich wie `line_strong` und schwächer als `focus_ring` |
| Akzent | `on_accent` auf `accent` ≥ 4,5:1 (Text auf dem Primärknopf) |
| Seitenleiste | `sidebar_text`, `sidebar_text_2`, `sidebar_text_3` ≥ 4,5:1 auf `sidebar`, `sidebar_hover`, `sidebar_selected` (auch mit zurückgefallenen Werten). Hat die Leiste eine eigene Fläche (`sidebar` ≠ `bg`): `focus_ring`, `accent`, `merged` und Kalenderfarben ≥ 3:1 auf `sidebar`, Statusfarben im dunklen Modus ebenso. Ohne eigene Fläche gilt für Grafik nichts Neues, ältere Pakete bleiben gültig |
| Schrift | jede Stufe 11–40 groß; Zeilenhöhe ≥ Größe und ≤ 2 × Größe; `body` 12–18; `read` von `body` bis 20; `hero` 32–72 und `countdown` 20–48 (Erinnerung im Vollbild, ER1.5); jedes genutzte Gewicht hat eine Datei in der genutzten Schrift; `regular` ist Pflicht für `[fonts.text]` |
| Layout-Abstände | `inset` 0–24; Innenränder 0–48; `reading_padding` je 0–80; `section_gap` 8–64; `item_gap` 2–24 |
| Abstände | `space` aufsteigend, 2–64; `radius` nicht fallend (gleiche Stufen erlaubt, z. B. 4/4/4/4), 0–16 |
| Maße | Bedienelemente (`control_sm` … `control_xl`) und `row_group` ≥ 24 (Klickziel); `row` 24–64; `row_two` und `row_contact` 24–96; `row_message` 24–120 und `row_notice` 24–96 (Zeilen des Eingangs; die Form `line` braucht nur eine Zeile von 24); `row_sidebar` 24–40; `header` 32–64; `sidebar` 180–320; `props`, `panel` 280–520; `list`, `list_medium` 280–640; `rail` 40–80 und mindestens `control_lg` (der Knopf der Leiste passt hinein); `rail_wide` 180–320; `reading_max` 480–1200; `content_max` 560–1200; Karten (E8.2): `card_gap` 4–32, `avatar` und `avatar_lg` 24–64, `tile_sm` 16–32, `tile` 20–48, `tile_lg` 24–64, Avatare und Kacheln von klein nach groß wachsend; Mail-Fenster (E11.1): `command` 32–64, `pill` 28–64. Einstellungsseiten (#464) haben keine Obergrenze: `form_max` bestimmt ihre Spalten-Umbrüche (zwei ab 2 × `form_max` + `section_gap`, drei ab 3 × `form_max` + 2 × `section_gap`), `content_max` die Textbreite ihrer Zeilen und die Breite der Wertspalten einer Tabelle |
| Listen | `[list] style` nur `rows`, `line`, `cards` und `selection` nur `bar`, `fill` (ein anderes Wort lehnt schon das Lesen mit Zeilennummer ab); `avatar` 0 (aus) oder 20–48; `preview_lines` 1–2 |
| Farbschema nur dunkel | mit `dark_only = true` kein `[colors.light]`; geprüft wird nur die dunkle Palette |
| Dateien | Pfade nur innerhalb des Paketordners, keine Links nach außen; `design.toml` ≤ 64 KB, SVG ≤ 256 KB, PNG ≤ 2 MB, Schrift ≤ 5 MB, Paket ≤ 20 MB |
| SVG | ohne Verweise auf andere Dateien oder Adressen (`href` nur `#…` und `data:`), ohne Skripte |
| Schriften | lesbar als TrueType/OpenType; Familie aus `family` steht in der Datei; Lizenzdatei vorhanden |
| Logo | quadratisch (Seitenverhältnis 1 ± 0,05) |

Was ein Paket **nicht** kann: andere Schwellen der Größenklassen, Laufweite und Großschreibung, Animationen, Ton, Texte,
Tastenbelegung, Anordnung (Stufe 2).

## Ein Paket bauen

Seit Aufgabe L.20 besteht ein Aussehen aus drei Teilen in drei Ordnern ([Drei Ordner](#drei-ordner)): einem
**Design** (die Form), einer **Schrift** und einem **Farbschema**. Seit Issue #484 baut man nur Schrift und Farbschema
selbst; das Design ist eine der drei eingebauten [Abstandsstufen](#drei-abstandsstufen-komfort-standard-dicht). Am
Beispiel **Plex** (Aufgabe L.5): IBM Plex Sans und ein ruhiger Petrol-Akzent, im Repository `fonts/plex/` und
`color-schemes/plex/`.

| Ordner | Datei | trägt | dazu |
|---|---|---|---|
| `designs/<id>/` | `design.toml`, nur im Repository (die drei Stufen, Issue #484) | `format`, `[about]`, `[type.*]`, `[space]`, `[radius]`, `[layout]`, `[size]`, `[list]` | – |
| `fonts/<id>/` | `font.toml` | `format`, `license`, `size_adjust` (optional), `kind` (optional: `sans`, `serif`, `mono`), `[about]`, `[fonts.text\|heading\|numbers]` | die Schriftdateien, `LICENSE.txt` |
| `color-schemes/<id>/` | `color-scheme.toml` | `format`, `dark_only` (optional, [nur dunkel](#farbschema-nur-dunkel)), `vivid` (optional, [knallig](#knallige-farbschemata)), `[about]`, `[colors.light\|dark]` (mit Seitenleiste und Diagramm), `[logo] mark` | `logo.svg`, bei fremder Palette `LICENSE.txt` |

Jede Datei hat ihr eigenes `[about]` mit `name` und `version`; jeder Teil wird für sich geprüft. Wer nur einen Teil
ändern will, legt nur dessen Ordner an: ein eigenes Farbschema braucht keine eigene Schrift.

**Knallige Schwestern (Issue #515).** Fünf der Schemata gibt es ein zweites Mal mit leuchtendem Akzent, in der Gruppe
„Vivid“ (`vivid = true`, [Knallige Farbschemata](#knallige-farbschemata)): Grund, Text und Statusfarben sind die der
Vorlage, nur `accent`, `on_accent` und `focus_ring` sind neu. Die Töne (hell bei Tokyo Night Vivid ein Violett statt Gelb) sind so verteilt, dass keiner mit der Warn- oder
OK-Farbe seiner Vorlage verwechselt wird. Im hellen Modus ist jeder Ton dunkler, damit Weiß auf dem Akzent 4,5:1 hält
und Heute-Kreis und Jetzt-Linie auf `panel` und `selected` 3:1 erreichen; der Wunschton und der Grund stehen als
Kommentar über `[colors.light]`. Name „<Vorlage> Vivid“, die Lizenz ist die der Vorlage.

Die Suite legt ihre Beispiele in die Ordner `fonts/` und `color-schemes/` im Datenordner, der Dienst beim Start, die
Steuerzentrale bei „Open fonts folder“ und „Open color schemes folder“: Plex, Commitr, Channelr, Guildr, Quattro und Ribbon als Schrift und Farbschema, die freien
Schriften von Issue #299 nur als Schrift ([Mitgelieferte Schriften](#mitgelieferte-schriften)), Nord,
Solarized, Catppuccin, Gruvbox, Dracula und Everforest nur als Farbschema, dazu 21 Farbschemata nach freien Paletten und fünf knallige Schwestern davon ([Mitgelieferte Farbschemata](#mitgelieferte-farbschemata)). Beispiel-Designs gibt es seit Issue #291
nicht mehr: Die drei Designs der Suite sind eingebaut ([unten](#drei-abstandsstufen-komfort-standard-dicht)), und seit
Issue #484 liest die Suite den Ordner `designs/` im Datenordner gar nicht mehr; eine Kopie der alten Beispiel-Designs
dort bleibt liegen, wird aber nicht angezeigt. Jedes Beispiel wird **einmal angeboten**
(L.10): Die versteckte Datei `.torro-examples` in `fonts/` und `color-schemes/` hält fest, welche schon da waren, und eine neue
Fassung der Suite legt nur Beispiele nach, die dort noch nicht stehen und für die es keinen Ordner gleichen Namens gibt.
Jeder Ordner führt seine Liste für sich. Ein gelöschtes oder geändertes Beispiel bleibt gelöscht oder geändert;
überschrieben wird nie etwas. Die Dateien stecken in `torroworkplace` (`src/designs.rs`), nicht im Kern, damit nicht
jede App die Schriften mitschleppt.

### Schritte

1. **Ordner anlegen:** `<data>/fonts/<id>/` und `<data>/color-schemes/<id>/`, `<id>` klein (`a-z`, `0-9`, `-`), in
   beiden gleich, wenn sie zusammengehören. Am schnellsten: Steuerzentrale › Appearance › Font › „Open fonts folder“
   bzw. Colors › „Open color schemes folder“, dort `plex` kopieren und umbenennen.
2. **Design:** entfällt seit Issue #484. Ein eigenes `design.toml` liest die Suite nicht mehr; die Abstände wählt man
   in Appearance › Spacing (Komfort, Standard, Dicht).
3. **Schrift:** `font.toml` mit `format = 1`, `license` (SPDX, Plex: `"OFL-1.1"`) und `[about]`; die TTF/OTF-Dateien
   direkt in `fonts/<id>/`, die Lizenz als `LICENSE.txt` daneben. `family` ist der Familienname in der Datei (Plex:
   „IBM Plex Sans“; die Schnitte Medium und SemiBold tragen ihn als typografische Familie, das reicht). Pflicht ist
   nur `regular` von `[fonts.text]`; fehlt ein Gewicht, das ein Design nutzt, nimmt die Suite das nächste (Lato ohne
   Medium zeichnet Medium als Regular). **Ziffern:** Sind die Standardziffern verschieden breit (Inter), gehört eine
   Familie mit festen Ziffern nach `[fonts.numbers]`. Plex braucht keine: Alle Ziffern sind in allen vier Schnitten
   gleich breit (600 von 1000 Einheiten), `numbers` fällt auf `text` zurück. **Größenausgleich:** von selbst aus der
   x-Höhe (Plex 1,058: IBM Plex Sans wirkt bei gleicher Größe kleiner als Inter); `size_adjust` (0,85–1,25) setzt ihn
   von Hand.
4. **Größen meinen Inter.** Die Schriftstufen der Designs sind für Inter gedacht. Die Suite multipliziert jede Größe
   mit dem Größenausgleich der gewählten Schrift und rundet auf 0,5, nie unter 11: Plex (1,058) zeichnet `body` 14
   darum in 15. Die Zeilenhöhe bleibt die des Designs, mindestens aber 1,2 × die wirksame Größe; eine enge Stufe
   (Zeile unter 1,2 × Größe, Torro `display` 28/32) wird mit demselben Faktor kleiner geschrieben, sonst wird sie
   lockerer. Wirkt eine Schrift zu groß oder zu klein, korrigiert `size_adjust` das in `font.toml`, nicht ein Design.
5. **Farben hell und dunkel (`color-scheme.toml`):** `format = 1`, `[about]`, dann Flächen (`bg` bis `menu`),
   Textstufen, Linien, Akzent mit `on_accent`. Plex dunkel: `bg #0C1013`, `panel #12171B`, `selected #1B2226`, Text
   `#E3E8EB` / `#A9B4BB` / `#86929A`, Akzent `#3AA6A0` mit dunkler Schrift `#03201E` (5,8:1). Hell: `bg #ECF0F1`,
   `panel #FFFFFF`, `selected #DEE5E8`, Text `#141B1F` / `#475359` / `#59656C`, Akzent `#0C6B66` mit Weiß (6,3:1).
   Status- und Kalenderfarben bleiben die des Standards. Ein Schema nach einer fremden Palette bringt deren Lizenz
   als `LICENSE.txt` neben `color-scheme.toml` mit (nur MIT, Apache-2.0 oder gemeinfrei,
   [Mitgelieferte Farbschemata](#mitgelieferte-farbschemata)).
6. **Logo:** `color-schemes/<id>/logo.svg` und `[logo] mark = "logo.svg"`, quadratisch mit `viewBox`, nur Formen,
   keine Verweise; es steht auf hellem und dunklem Grund, also eine eigene Fläche mitbringen (Plex: Petrol-Quadrat
   mit drei weißen Balken). Das Logo gehört zum Farbschema, weil es in dessen Farben gezeichnet ist. Ein Tray-Symbol
   gibt es nicht mehr: Es ist für alle gleich (Issue #114), `tray` in `[logo]` ist ein unbekannter Schlüssel.
7. **Vorschau:** entfällt seit Issue #484 (`preview.png` gehörte zum eigenen Design). Die Steuerzentrale zeigt
   Schriften als Schriftprobe, Farbschemata als Farbmuster.
8. **Prüfen:** Steuerzentrale › Appearance › Font › „Reload“ (liest alle Listen neu): Schrift und Farbschema stehen in
   ihrer Auswahl, ungültig mit jedem Grund in einer eigenen Zeile. Ansehen ohne die eigene Einstellung zu ändern:
   `torrocalendar --demo --font <id> --color-scheme <id>` (ebenso `torrotasks`, `torroinbox`, `torroworkplace ui`),
   mit `--appearance light` und `dark`, dazu `--design comfort` und `--design dense`.

### Häufige Fehler

| Grund in der Steuerzentrale | Abhilfe |
|---|---|
| Unbekannter Schlüssel („Unknown setting …“) | Tippfehler (Namen wie in `torro_gui::theme::Palette` und `size`, z. B. `text_3`, `row_sidebar`) oder ein Abschnitt in der falschen Datei: Farben und Logo gehören nach `color-scheme.toml`, Schriften nach `font.toml` |
| `text_3` oder `error_text` unter 4,5:1 auf `selected` | `selected` ist die hellste Fläche im Dunkeln (die dunkelste im Hellen) und fällt zuerst durch. Bei Plex lag das Fehlerrot im Dunkeln erst bei 4,4:1 – `selected` wurde einen Hauch dunkler |
| `on_accent` zu schwach | Heller Akzent → dunkle Schrift darauf, dunkler Akzent → Weiß. Ein Akzent, der im Dunkeln leuchtet, braucht meist dunkle Schrift |
| Kante des Gewählten (`line_selected`) | muss mindestens dreimal so deutlich sein wie `line_strong` und schwächer als `focus_ring`; beim Ändern der Linien alle drei zusammen ansehen |
| Familie stimmt nicht | `family` wie in der Datei, nicht wie der Dateiname („IBM Plex Sans“, nicht „IBMPlexSans“) |
| Schrift ohne Lizenz | `LICENSE.txt` fehlt in `fonts/<id>/` oder `license` in `font.toml` |
| Logo nicht quadratisch, SVG mit Verweis | `viewBox="0 0 24 24"`; Bilder und Schriften nicht verlinken, sondern als Formen zeichnen |

Nicht als Grund gemeldet, aber im Bild zu sehen: Größen, die für die eigene Schrift statt für Inter eingetragen sind,
wirken um deren Ausgleich zu groß. Höhere Zeilen in der Seitenleiste brauchen Platz. Die Seitenleiste des Kalenders
rollt nicht; in niedrigen Fenstern schiebt sie dann „Settings“ unten aus dem Bild (gesehen mit Zeilen 32 bei 760
Höhe). Darum hat Plex Seitenleistenzeilen 30 statt 32.

## Design, Schrift und Farbschema getrennt

> **Plan zu Issue #189, Stand 2026-10-20.** Entschieden: die Schrift ist eine eigene, dritte Wahl (2026-10-15); drei
> Ordner statt eines Pakets mit Abschnitten, die Seite Appearance wird kompakt mit Kacheln, keine Übernahme alter
> Pakete und Einstellungen (2026-10-20). Aufgaben L.18–L.30 in [tasks/design-pakete.md](tasks/design-pakete.md). Bis zur
> Umsetzung gilt alles oben unverändert.

### Warum

Heute ist ein Paket immer alles zugleich: Schrift, Abstände, Maße *und* Farben. Von den dreizehn Beispielen sind sechs
(Nord, Solarized, Catppuccin, Gruvbox, Dracula, Everforest) eigentlich nur Farben auf der Form von Torro. Wer die Luft
von Plex mit den Farben von Nord will, Commitr in Dunkelgrün oder Torro in Lato, kann das nicht. Getrennt wählbar wird
aus *d* Designs, *s* Schriften und *f* Farbschemata *d × s × f* Looks, ohne dass jemand sie einzeln baut.

### Begriffe

| Begriff | Was es bestimmt | Englisch (Code, Oberfläche) |
|---|---|---|
| **Design** | Schriftstufen (Rolle, Größe, Zeilenhöhe, Gewicht), Abstände, Radien, Layout-Abstände, Maße; später die Layout-Schalter (Stufe 2). Seit Issue #484 nur noch die drei Abstandsstufen | `Design`, „Spacing“ |
| **Schrift** | die Familien der drei Rollen Text, Überschrift, Ziffern mit ihren Schnitten, dazu der Größenausgleich | `FontSet`, „Font“ |
| **Farbschema** | alle Farbrollen hell und dunkel (Flächen, Text, Linien, Akzent, Status, Kalender, Diagramm, Seitenleiste) und das Logo | `ColorScheme`, „Color scheme“ |
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
bisher das „T“ auf dem Akzent des Farbschemas. Die Vorschau `preview.png` des Designs entfällt seit Issue #484;
Schriften zeigt die Steuerzentrale als Schriftprobe, Farbschemata als Farbmuster.

### Drei Ordner

Jeder Teil hat einen eigenen Ordner, jeder Eintrag ist ein Unterordner mit einer Datei und `format = 1`. Seit Issue
#484 liegen die Designs nur noch im Repository (die drei eingebauten Stufen); im Datenordner gibt es nur `fonts/` und
`color-schemes/`:

| Ordner | Datei | bringt | dazu |
|---|---|---|---|
| `designs/<id>/` (nur Repository) | `design.toml`: `[about]`, `[type.*]`, `[space]`, `[radius]`, `[layout]`, `[size]`, `[list]` | ein **Design** | – |
| `fonts/<id>/` | `font.toml`: `[about]`, `license`, `size_adjust`, `kind`, `[fonts.text\|heading\|numbers]` | eine **Schrift** | Schriftdateien, `LICENSE.txt` |
| `color-schemes/<id>/` | `color-scheme.toml`: `dark_only` (optional), `vivid` (optional), `[about]`, `[colors.light\|dark]` mit Seitenleiste und Diagramm, `[logo]` | ein **Farbschema** | `logo.svg`, bei fremder Palette `LICENSE.txt` |

- Jede Datei hat ihr eigenes `[about]` (Name, Version, Autor, Beschreibung). Dieselbe id darf in mehreren Ordnern
  stehen (Commitr in allen dreien). `torro` ist alle drei, eingebaut, ohne Ordner.
- Neu und optional im `[about]` eines Designs: `font = "<id>"` und `color_scheme = "<id>"`, die Schrift und das
  Farbschema, die zum Design gedacht sind. Nur Vorschläge, gewählt wird getrennt. Seit Issue #291 kommt `mail_view`
  (`"html"`, `"text"`) dazu, und `color_scheme` ist seit L.33 entfallen: Farben gehören nie zu einem Design. Seit
  Issue #484 sind auch `font` und `mail_view` entfallen: Ein Design schlägt nichts vor, wer es wählt, ändert nur die
  Abstände.
- Jeder Teil wird für sich geprüft und für sich abgelehnt.
- Ein Tray-Symbol gibt es im neuen Format nicht mehr: Der Tray zeichnet immer das rote Raster (Issue #114).
- In der Liste der Schriften steht der Familienname der Textschrift („IBM Plex Sans“), im Tooltip der Name aus
  `[about]` („Plex“).
- **Beispiele im Repository** liegen genauso in `designs/`, `fonts/` und `color-schemes/` (L.20): Nord, Solarized,
  Catppuccin, Gruvbox, Dracula und Everforest nur als Farbschemata (Dracula und Everforest verlieren `[radius]`); Plex,
  Commitr, Channelr, Guildr, Quattro und Ribbon in allen drei Ordnern, ihre `[type.*]`-Größen auf Inter
  zurückgerechnet (geteilt durch den Ausgleich ihrer Schrift), damit sie mit der eigenen Schrift so groß bleiben wie
  heute. Jeder Ordner hat seine eigene `.torro-examples` (seit Issue #484 nur noch `fonts/` und `color-schemes/`); in einem neu angelegten Ordner gelten die ids aus
  `designs/.torro-examples` als schon angeboten, damit ein gelöschtes Beispiel nicht als Farbschema wiederkommt.

### Keine Übernahme alter Pakete

Es gibt noch keine ausgelieferte Version (Entscheidung 2026-10-20). Darum übernimmt die Suite weder alte
Komplettpakete noch alte Einstellungen: In `designs/` sind `[fonts.*]`, `[colors.*]` und `[logo]` unbekannte Schlüssel
wie jeder andere, und `font` und `color_scheme` beginnen mit `torro`. Wer noch einen Ordner `designs/` von vorher hat,
benennt ihn einmal um oder löscht ihn; beim nächsten Start legt die Suite die Beispiele in alle drei Ordner. Seit
Issue #484 liest die Suite `designs/` gar nicht mehr; der Ordner bleibt liegen.

### Wählen

- Drei Einstellungen: `design` (gibt es), neu `font` und `color_scheme`, alle mit `torro` als Standard.
- **Steuerzentrale**, Appearance, kompakt untereinander (Entwurf in Issue #189): **Color mode** als Segment mit
  Symbolen (Bildschirm, Sonne, Mond) · **Design** · **Font** · **Color scheme** · Interface. Die drei Wahlen sind
  Raster kleiner Kacheln: Design mit `preview.png` oder Farbmuster, Font mit „Aa 09“ in der Schrift, Color scheme mit
  Streifen aus `bg`, `panel`, `accent`, `text` im aktuellen Modus. Name unter der Kachel, Version und Autor im Tooltip,
  die gewählte umrandet mit Häkchen. Unbrauchbare sind ausgegraut mit Warnsymbol; ein Klick zeigt ihre Gründe unter dem
  Raster. Jede Gruppe hat „Open … folder“ rechts in der Überschrift. Seit Issue #484 hat die Seite vier Gruppen
  Spacing, Font, Colors und Display ([Wählen und wechseln](#wählen-und-wechseln)); Designs haben keinen Ordner mehr.
- Ein Design wählen ändert Schrift und Farbschema **nicht**. Schlägt das gewählte Design andere vor als die aktiven,
  trägt deren Kachel die Marke „Suggested“ (Tooltip „Suggested for Commitr“, ein Klick wählt sie). **Ab L.33 (Issue
  #291):** Wer ein Design wählt, bekommt dessen Vorschläge für Schrift (wenn es sie gibt) und Mail-Darstellung gleich
  mit; das Farbschema bleibt immer. **Seit Issue #484** wieder nicht: Ein Design schlägt nichts vor und setzt nichts
  mit, Schrift, Farbschema und Mail-Darstellung bleiben.
- Ungültige Teile, Wechsel ohne Wartezeit und Rückfall auf Torro gelten für alle drei wie unten beschrieben. Ein
  Wechsel der Schrift lädt Schriften (`iced::font::load`), ein Wechsel von Design oder Farbschema nicht.

### API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Designs auflisten | Appearance › Spacing | `settings.list_designs` · Read (gibt es; seit Issue #484 nur die drei Stufen, ohne Vorschläge) | `settings_list_designs` |
| Design wählen | Appearance › Spacing | `settings.set_setting { key: design }` · Admin (gibt es; seit Issue #484 nur `torro`, `comfort`, `dense`, dazu `compact`/`console` als `dense`) | `settings_set_setting` |
| Schriften auflisten (id, Familie, Paket, Version, Autor, Lizenz, Standard, gültig, Gründe, aktiv) | Appearance › Font | neu `settings.list_fonts` · Read | neu `settings_list_fonts` |
| Schrift wählen | Appearance › Font | `settings.set_setting { key: font }` · Admin | `settings_set_setting` |
| Farbschemata auflisten (id, Name, Version, Autor, Beschreibung, Standard, gültig, Gründe, aktiv) | Appearance › Color scheme | neu `settings.list_color_schemes` · Read | neu `settings_list_color_schemes` |
| Farbschema wählen | Appearance › Color scheme | `settings.set_setting { key: color_scheme }` · Admin | `settings_set_setting` |
| Ordner öffnen | „Open fonts folder“, „Open color schemes folder“ | – (lokale Aktion der Steuerzentrale; für Designs seit Issue #484 keine) | – |
| Gewählte lesen | – | `settings.get_settings` · Read (Felder `design`, `font`, `colorScheme`) | `settings_get_settings` |

Verträglich nach `docs/api.md`: neue Befehle, neue Schlüssel, neue Felder. Dass `list_designs` reine Schriften und
Farbschemata nicht mehr nennt, ändert den Inhalt, nicht die Form; ein Agent, der `design = "nord"` setzt, bekommt einen
Fehler, der ihn zu `font` oder `color_scheme` schickt, je nachdem, was das Paket mitbringt.

### Technik

- **`torro_core::design`:** `Design` verliert `fonts`, `colors` und `logo`, `About` verliert `font_license`. Neu
  `FontSet { id, about, license, fonts, size_adjust }` und `ColorScheme { id, about, colors, logo }`, je mit
  `standard()`. Neben `dir()` (`designs/`) neu `font_dir()` und `color_scheme_dir()`; je Ordner `load`/`list`,
  `load_font`/`list_fonts`, `load_color_scheme`/`list_color_schemes`. Seit Issue #484 entfällt `dir()`; `list()` und
  `load(id)` kennen nur die eingebauten Designs und nehmen keinen Ordner mehr. Wirksame Stufen (Größenausgleich, Ersatz
  der Gewichte) rechnet der Kern: `Selection { design, font, scheme }` mit `level(name)`, damit Kern, Steuerzentrale und
  Tests dasselbe sehen.
- **`torro_gui`:** drei aktive Teile statt einem (`design::active()`, `font::active()`, `scheme::active()`).
  `typo::body()` liefert die wirksame Stufe, `theme::font()` die Familie der aktiven Schrift, `palette(theme)` die
  Farben des aktiven Farbschemas. `design::Follow` folgt allen drei Einstellungen. `--demo` bekommt `--font <id>` und
  `--color-scheme <id>` neben `--design`.
- **Gleiches Bild:** Mit Torro/Torro/Torro sieht jede App aus wie vorher (exakt). Mit jedem Beispielpaket als alle drei
  Teile weichen die wirksamen Größen höchstens um 0,5 px von heute ab (Rundung des Ausgleichs).
- **Stufe 2:** Der Kandidat „Dichte“ fällt weg: Sie steht seit Issue #226 in Stufe 1, seit Issue #291 als
  `[list] style`.

### Mitgelieferte Designs

> Ersetzt durch die drei eingebauten Designs (Issue #291), seit Issue #484 die drei
> [Abstandsstufen](#drei-abstandsstufen-komfort-standard-dicht). Die Vorschläge Airy, Clear, Folio, Pebble und Square
> (L.25–L.30) entfallen; die sechs Beispiel-Designs Plex, Commitr, Channelr, Guildr, Quattro und Ribbon auch, ihre
> Schriften und Farbschemata bleiben.

### Mitgelieferte Farbschemata

Issue #292. Ein Schema nach fremder Palette steht unter MIT, Apache-2.0 oder ist gemeinfrei und bringt den Lizenztext
seiner Quelle als `LICENSE.txt` mit (oben die Quelle mit Abrufdatum, darunter der Text wörtlich, mit der
Copyright-Zeile). Name, Autor und Beschreibung sind neutral, wo die Quelle eine Marke ist: „Primer“ statt GitHub,
„Slate“ statt Radix, „One“ ohne Atom; VS Code, IBM Carbon, GNOME, KDE und Ubuntu kommen nicht vor. Nicht
mitgeliefert werden Schemata unter Copyleft (Adwaita und Breeze LGPL, Yaru GPL bzw. CC-BY-SA, Modus und Zenburn GPL)
und solche ohne klare oder mit kommerzieller Lizenz (Monokai, Material Theme, Dracula PRO).

Die neuen Schemata bringen kein Logo mit, die Suite zeichnet das „T“ auf ihrem Akzent. Je Variante kommen fünf
Farben aus der Palette der Quelle (Grund, Fläche, Text, gedämpfter Text, Akzent); alle übrigen Farbrollen sind daraus
abgeleitet. Wo eine Farbe der Quelle eine Regel verfehlt, ist sie verschoben und im Schema mit altem und neuem Wert
kommentiert (meist der gedämpfte Text, der 4,5:1 auf allen Flächen braucht). Die Farben der Kategorie-Pillen
(`category`) gehören nicht zum Format eines Farbschemas; wo die des Standards auf `panel` unter 3:1 bleiben (helle,
farbige Flächen), ist darum `panel` so weit aufgehellt bzw. abgedunkelt, dass sie es erreichen („Kategoriefarben“ in
der Tabelle). Über die Regeln des Kerns hinaus gilt für die abgeleiteten Farben (Sichtprüfung #292): Der Text
steht mindestens 1,5:1 vom gedämpften Text ab, damit Titel und Metadaten unterscheidbar bleiben (sonst wird `text`
kräftiger); `text` und `text_2` erreichen 4,5:1 auch auf den getönten Terminflächen; der Akzent erreicht als Marke
(Strich der gewählten Zeile, Heute, Jetzt-Linie) 3:1 auf `panel` und `selected`. Die Tabelle nennt die wichtigsten
Änderungen; jede verschobene Farbe steht mit altem und neuem Wert im Kommentar des Schemas.

| Name | id | Gruppe | Quelle | Lizenz | Anmerkung |
|---|---|---|---|---|---|
| Torro | `torro` | Torro | eigen, eingebaut | – | Standard, Torro-Rot |
| Plex | `plex` | Light and dark | eigen | – | kühle Flächen, Petrol-Akzent; mit Logo |
| Commitr | `commitr` | Light and dark | eigen, angelehnt an GitHub | – | neutrale Graus, grüner Primärknopf, dunkle Seitenleiste im Hellen; mit Logo |
| Ribbon | `ribbon` | Light and dark | eigen, angelehnt an Microsoft Office | – | Fluent-Graus, Office-Blau; mit Logo |
| Channelr | `channelr` | Light and dark | eigen, angelehnt an Slack | – | Seitenleiste in Aubergine; mit Logo |
| Guildr | `guildr` | Light and dark | eigen, angelehnt an Discord | – | Blurple auf dunklem Grau; mit Logo |
| Quattro | `quattro` | Light and dark | eigen, angelehnt an Google Workspace | – | Material-3-Flächen, blaue Pillen; mit Logo |
| Nord | `nord` | Light and dark | github.com/nordtheme/nord | MIT | dunkel Polar Night; hell aus Snow Storm abgeleitet (die Quelle hat nur dunkel); Akzent Frost; mit Logo |
| Solarized | `solarized` | Light and dark | github.com/altercation/solarized | MIT | beide Modi der Quelle, blauer Akzent mit dunkler Schrift; mit Logo |
| Gruvbox | `gruvbox` | Light and dark | github.com/morhetz/gruvbox | MIT (README und `package.json`, keine Lizenzdatei) | Akzent ohne Vorbild: dunkel Gelb `#FABD2F`, hell Orange `#AF3A03`; mit Logo |
| Dracula | `dracula` | Light and dark | github.com/dracula/dracula-theme | MIT | Akzent Lila, Fokus Pink; hell ist Alucard; mit Logo |
| Catppuccin | `catppuccin` | Light and dark | github.com/catppuccin/catppuccin | MIT | dunkel Mocha, hell Latte, Akzent Mauve; mit Logo |
| Everforest | `everforest` | Light and dark | github.com/sainnhe/everforest | MIT | Waldgrün auf warmen Flächen; mit Logo |
| Ayu | `ayu` | Light and dark | github.com/ayu-theme/ayu-colors | MIT | hell ayu Light, dunkel ayu Mirage; Akzent dunkel Gelb `#FFCC66`, hell Orange `#F29718` → `#A96609` (trägt sonst weder Weiß noch den Text); gedämpfter Text, Warn- und Fehlerfarbe verschoben |
| Base16 | `base16` | Light and dark | github.com/tinted-theming/schemes (Default von Chris Kempson) | MIT | Default Light und Default Dark; Akzent Blau `base0D`, hell `#7CAFC2` → `#447D93` (als Marke zu schwach auf dem grauen `panel`); gedämpfter Text `#B8B8B8`/`#585858` stark verschoben, dunkel Rot `#AB4642` → `#D4918F`, im Hellen Fokus, Status und Kalender dunkler |
| Edge | `edge` | Light and dark | github.com/sainnhe/edge | MIT | Stil light und default; Akzent Blau (hell `#5079BE` → `#4B75BC`); gedämpfter Text, Rot und im Hellen Warnfarbe verschoben; ohne Orange und Magenta (Gelb, Violett im Kalender) |
| Flexoki | `flexoki` | Light and dark | github.com/kepano/flexoki | MIT | Akzent ohne Vorbild: Blau 600 `#205EA6` hell, 400 `#4385BE` dunkel; Link stephango.com/flexoki in der Beschreibung (Bitte des Autors); nur gedämpfter Text und dunkel Warn-/Fehlerfarbe verschoben |
| Iceberg | `iceberg` | Light and dark | github.com/cocopon/iceberg.vim | MIT | beide Modi der Quelle, Akzent Blau; hell `panel` `#DCDFE7` → `#E0E2EA` (Kategoriefarben); gedämpfter Text und Rot verschoben |
| Kanagawa | `kanagawa` | Light and dark | github.com/rebelot/kanagawa.nvim | MIT | hell Lotus, dunkel Wave, Akzent Blau; hell `panel` `#E7DBA0` → `#ECE3B5` (Kategoriefarben); gedämpfter Text, Rot und Orange verschoben |
| Nightfox | `nightfox` | Light and dark | github.com/EdenEast/nightfox.nvim | MIT | hell Dayfox, dunkel Nightfox, Akzent Blau; hell `panel` `#E4DCD4` → `#E8E2DB` (Kategoriefarben); gedämpfter Text und dunkel Rot `#C94F6D` → `#D4748C` verschoben |
| Night Owl | `night-owl` | Light and dark | github.com/sdras/night-owl-vscode-theme | MIT | hell Light Owl (Akzent Türkis `#2AA298` → `#22837B`), dunkel Night Owl (Akzent Blau); gedämpfter Text und Status verschoben |
| One | `one` | Light and dark | github.com/atom/atom (`one-dark-syntax`, `one-light-syntax`) | MIT | hell One Light, dunkel One Dark, Akzent Blau `hue-2` (hell `#4078F2` → `#306DF1`); Farben der Quelle sind `hsl()`, hier als Hex; gedämpfter Text und Rot verschoben |
| Primer | `primer` | Light and dark | github.com/primer/primitives | MIT | helles und dunkles Theme der Primitives 11.10.0, Akzent `fgColor-accent`, Status aus `fgColor-success`, `-attention`, `-danger`, `-done`; nur dunkel Rot verschoben |
| Rosé Pine | `rose-pine` | Light and dark | github.com/rose-pine/palette | MIT | hell Dawn, dunkel Rosé Pine, Akzent Iris (hell `#907AA9` → `#846B9F`); gedämpfter Text ist `subtle`; ohne Grün und Orange (OK dunkel Foam, hell Pine) |
| Selenized | `selenized` | Light and dark | github.com/jan-warchol/selenized | MIT | Selenized light und dark, Akzent Blau (dunkel `#4695F7` → `#5EA3F8`); Text ist `fg_1` statt `fg_0` (`fg_0` trägt keine 4,5:1 auf den abgeleiteten Flächen); dunkel `panel` `#184956` → `#184855` (Kategoriefarben) |
| Slate | `slate` | Light and dark | github.com/radix-ui/colors | MIT | Slate-Skala hell und dunkel (Stufen 1, 2, 12, 11), Akzent Blue 9 `#0090FF` (hell → `#0077D2`, als Marke zu schwach auf `selected`), Status und Kalender Stufe 11 |
| Tango | `tango` | Light and dark | Tango Desktop Project | gemeinfrei | Palette mit drei Tönen je Farbe, hell die dunkleren, dunkel die helleren; hell `panel` `#D3D7CF` → `#E1E4DE` (Kategoriefarben), dunkel `panel` Aluminium 5 `#555753` → `#3A3E3F` (sonst weder gedämpfter noch roter Text lesbar); Hinweis auf die gemeinfreie Palette statt Lizenztext |
| Tokyo Night | `tokyo-night` | Light and dark | github.com/enkia/tokyo-night-vscode-theme | MIT | hell Tokyo Night Light, dunkel Tokyo Night, Akzent Blau; dunkel Text `editor.foreground` `#A9B1D6` und gedämpft der Kommentar `#51597D` der Quelle (der Entwurf hatte `#C0CAF5`/`#565F89` aus einer anderen Portierung), verschoben auf `#838BAF`; hell gedämpft `#707280` → `#575B70`; hell `panel` `#D6D8DF` → `#E1E2E7` (Kategoriefarben) |
| Tomorrow | `tomorrow` | Light and dark | github.com/chriskempson/tomorrow-theme | MIT | Tomorrow und Tomorrow Night, Akzent Blau; gedämpfter Text, Rot und im Hellen Orange verschoben |
| Cobalt2 | `cobalt2` | Dark only | github.com/wesbos/cobalt2-vscode | MIT | Akzent Gelb `#FFC600` mit dunkler Schrift; ohne Violett (`merged` ist sein Pink); nur Rot verschoben |
| Palenight | `palenight` | Dark only | github.com/whizkydee/vscode-palenight-theme | MIT | Akzent Lila `#C792EA`; gedämpfter Text (Kommentar `#697098`) stark aufgehellt, Rot und `merged` verschoben |
| Poimandres | `poimandres` | Dark only | github.com/drcmda/poimandres-theme | MIT | Akzent Mint `#5DE4C7`; `bg` und `panel` beide `#1B1E28`, wie in der Quelle (der Entwurf hatte `panel` `#232733`, das dort nicht vorkommt); gedämpfter Text und Rot verschoben |
| Sonokai | `sonokai` | Dark only | github.com/sainnhe/sonokai | MIT | Stil default, Akzent sein Blau `#76CCE0`; ohne Cyan und Magenta (Violett, Gelb im Kalender); gedämpfter Text und Rot verschoben |
| Catppuccin Vivid | `catppuccin-vivid` | Vivid | wie Catppuccin | MIT (Text der Vorlage) | Magenta: dunkel `#FF4DB8`, hell `#D6008A`; sonst Catppuccin; ohne Logo |
| Dracula Vivid | `dracula-vivid` | Vivid | wie Dracula | MIT (Text der Vorlage) | Cyan: dunkel `#00E5FF`, hell `#00798C`; Draculas Grün bleibt „OK“; erste Kalenderfarbe dunkel Blau `#6FA8FF` statt Hellcyan; ohne Logo |
| Gruvbox Vivid | `gruvbox-vivid` | Vivid | wie Gruvbox | MIT (Text der Vorlage) | Grün: dunkel `#2EE66B`, hell `#00852B`; Orange bleibt der Warnfarbe; ohne Logo |
| Nord Vivid | `nord-vivid` | Vivid | wie Nord | MIT (Text der Vorlage) | Orange: dunkel `#FF7A1A`, hell `#C24A00`; ohne Logo |
| Tokyo Night Vivid | `tokyo-night-vivid` | Vivid | wie Tokyo Night | MIT (Text der Vorlage) | Gelb: dunkel `#FFE500`; hell Violett `#8A3FE0` (Gelb trägt hell kein Weiß und ein Senfgelb ist von der Warnfarbe kaum zu trennen, Sichtprüfung #515); ohne Logo |
| Synthwave ’84 | `synthwave-84` | Dark only | github.com/robb0wen/synthwave-vscode | MIT | Akzent Pink `#FF7EDB`; Blau und Cyan sind ein Neon-Cyan (Gelb als fünfte Kalenderfarbe); gedämpfter Text und Rot verschoben |

### Schriften des Systems

Neben den Schriften von Torro kann die Suite die Schriften nutzen, die auf dem Rechner installiert sind (Issue #299).
Sie **nutzt sie nur mit und gibt sie nicht weiter**: Nichts wird kopiert, die Dateien werden nur gelesen, um sie zu
prüfen, und gespeichert wird nur die Id in `Settings::font`. Das Format der Einstellungen bleibt gleich.

- **Ids:** `system` ist der Eintrag „System“, die Oberflächenschrift des Betriebssystems, beim Laden jedes Mal neu
  aufgelöst: auf macOS `System Font` (SF, als Familie nicht noch einmal gelistet), auf Windows die erste von `Segoe UI Variable Text`, `Segoe UI Variable`,
  `Segoe UI`, auf Linux die Familie von `fc-match -f '%{family[0]}' sans-serif`. `system:<Familie>` ist eine
  installierte Familie (`system:Avenir Next`). Ordner-Ids sind `a-z0-9-`, keine kann mit `system:` beginnen;
  `system` selbst ist wie `torro` als Ordnername gesperrt.
- **Welche erscheinen:** Familien, deren Name nicht mit `.` beginnt, höchstens 64 Zeichen hat und keine Steuer-, Format-
  oder Zeilentrennzeichen enthält (der Name stammt aus einer fremden Datei und wird Teil der Id), mit einem aufrechten Schnitt,
  dessen regulärer Schnitt `A–Z a–z ä ö ü ß é` hat. Symbol-, Emoji- und nicht lateinische Schriften erscheinen gar
  nicht.
- **Regeln** (am aufrechten Schnitt mit Gewicht 400): Ohne ihn ist die Familie mit Grund ungültig. Schnitte gelten
  nur, wo ein aufrechter Schnitt genau das Gewicht hat, sonst der nächste (`Font::nearest`); ein variabler regulärer
  Schnitt mit Achse `wght` gilt als Regular, Medium und SemiBold, nicht als Bold (Iced zeichnet SF bei 500 und 600 aus
  der Achse und fällt bei 700 auf eine andere Schrift). Sind die zehn Ziffern gleich breit, bleibt `numbers` die
  Familie, sonst die eingebaute Inter Tabular; dann ist der Größenausgleich höchstens 1,11, damit die geliehenen
  Ziffern in die Zeitachse passen (sonst würden Schriften mit kleiner x-Höhe wie Georgia abgelehnt). Der Größenausgleich kommt aus der x-Höhe, die Regeln für Schriften
  gelten wie bei einem Ordner. Lizenz, Art (`kind`) und Version bleiben leer.
- **Gründe** nennen nur die Familie, nie einen Dateipfad (er verriete Benutzernamen und Home-Verzeichnis).
- **Fehlt die Schrift** auf einem Rechner (deinstalliert, anderer Rechner), zeichnen die Apps mit dem Standard;
  die Steuerzentrale zeigt den Grund.
- **Kein PDF:** Die PDF-Werkzeuge betten ihre Schrift ein. Eine Systemschrift einzubetten hieße, sie weiterzugeben;
  bei gewählter Systemschrift setzt das PDF deshalb in Inter.
- Ein Design schlägt seit Issue #484 gar keine Schrift mehr vor, also auch keine Systemschrift.

### Mitgelieferte Schriften

Die Schriften von Torro: alle unter OFL, mit `LICENSE.txt` (die `OFL.txt` der Schrift wörtlich). Ziffern wie in L.5
geprüft: gleich breit, sonst eine Ziffernschrift dazu. „Verändert“ heißt: Die Dateien sind nicht die der Quelle (feste
Schnitte aus einer variablen Schrift, eingefrorene Ziffern); eine veränderte Datei einer Schrift mit Reserved Font
Name (RFN) trägt einen anderen Familiennamen.

| Id | Familien | Art | Quelle | Lizenz | RFN | verändert | Ziffern |
|---|---|---|---|---|---|---|---|
| `torro` | Inter, Ziffern Inter Tabular | sans | github.com/rsms/inter | OFL-1.1 | – | ja (Inter Tabular) | Inter Tabular, eingefroren |
| `cantarell` | Cantarell (Regular, Bold), Ziffern Cantarell Figures | sans | google/fonts ofl/cantarell @e7db6ca, 1.004 | OFL-1.1 | – | ja (Cantarell Figures) | Cantarell Figures: Kopie des Regular, dessen Ziffern gleich breit sind (im Bold ist die 4 breiter) |
| `channelr` | Lato | sans | google/fonts | OFL-1.1 | „Lato“ | nein | gleich breit |
| `commitr` | Mona Sans, Ziffern Commitr Figures | sans | github/mona-sans 2.0.27 | OFL-1.1 | „Mona“ | ja (Commitr Figures) | Commitr Figures, eingefroren |
| `dm-sans` | DM Sans, Ziffern DM Sans Figures | sans | googlefonts/dm-fonts @4412393, 4.004 | OFL-1.1 | – | ja (DM Sans Figures) | DM Sans Figures, `tnum` eingefroren |
| `fira-code` | Fira Code | mono | tonsky/FiraCode 6.2 | OFL-1.1 | – | nein | gleich breit |
| `fira-sans` | Fira Sans, Ziffern Fira Sans Figures | sans | google/fonts ofl/firasans @e7db6ca, 4.203 | OFL-1.1 | – | ja (Fira Sans Figures) | Fira Sans Figures, `tnum` eingefroren |
| `geist` | Geist, Ziffern Geist Figures | sans | vercel/geist-font v1.8.2 | OFL-1.1 | – | ja (Geist Figures) | Geist Figures, `tnum` eingefroren |
| `geist-mono` | Geist Mono | mono | vercel/geist-font v1.8.2 | OFL-1.1 | – | nein | gleich breit |
| `guildr` | Figtree, Ziffern Figtree Tabular | sans | google/fonts | OFL-1.1 | – | ja (feste Schnitte, Figtree Tabular) | Figtree Tabular, eingefroren |
| `hyperlegible` | Atkinson Hyperlegible Next, Ziffern Hyperlegible Figures | sans | googlefonts/atkinson-hyperlegible-next @7925f50, 2.001 | OFL-1.1 | – | ja (Hyperlegible Figures) | Hyperlegible Figures, `tnum` eingefroren |
| `inconsolata` | Inconsolata | mono | google/fonts ofl/inconsolata/static @e7db6ca, 3.001 | OFL-1.1 | – | nein | gleich breit |
| `jetbrains-mono` | JetBrains Mono | mono | JetBrains/JetBrainsMono v2.304 | OFL-1.1 | – | nein | gleich breit |
| `lexend` | Lexend, Ziffern Lexend Figures | sans | googlefonts/lexend @7894f02, 1.008 | OFL-1.1 | nur „RevReading Lexend“ | ja (Lexend Figures) | Lexend Figures: die Standardziffern auf eine Breite gebracht und mittig (kein `tnum`) |
| `literata` | Literata, Ziffern Literata Figures | serif | googlefonts/literata 3.103 | OFL-1.1 | – | ja (Literata Figures) | Literata Figures, `tnum` eingefroren |
| `lora` | Lora, Ziffern Torro Quill Figures | serif | cyrealtype/Lora-Cyrillic v3.021 | OFL-1.1 | „Lora“ | ja (Torro Quill Figures, umbenannt) | Torro Quill Figures, `tnum` eingefroren |
| `manrope` | Manrope, Ziffern Manrope Figures | sans | googlefonts/manrope @468c0db, 4.505 | OFL-1.1 | – | ja (Manrope Figures) | Manrope Figures, `tnum` eingefroren |
| `merriweather` | Merriweather, Ziffern Torro Serif Figures | serif | EbenSorkin/Merriweather4 4.009 (feste Schnitte) | OFL-1.1 | „Merriweather“ | ja (Torro Serif Figures, umbenannt) | Torro Serif Figures, `tnum` eingefroren |
| `newsreader` | Newsreader 16pt | serif | productiontype/NewsReader @cfcb4f7 (feste Schnitte 16pt), 1.003 | OFL-1.1 | – | nein | gleich breit je Schnitt |
| `noto-sans` | Noto Sans | sans | notofonts/latin-greek-cyrillic NotoSans-v2.015 | OFL-1.1 | – | nein | gleich breit |
| `nunito` | Nunito | sans | googlefonts/nunito @8c6a9bb, 3.602 | OFL-1.1 | – | ja (feste Schnitte) | gleich breit |
| `open-sans` | Open Sans (ohne Medium) | sans | googlefonts/opensans @bd7e376, 3.003 | OFL-1.1 | – | nein | gleich breit |
| `plex` | IBM Plex Sans | sans | github.com/IBM/plex 1.1.0 | OFL-1.1 | „Plex“ | nein | gleich breit |
| `plex-mono` | IBM Plex Mono (Name „Plex Mono“) | mono | IBM/plex @ibm/plex-mono@2.5.0 | OFL-1.1 | „Plex“ | nein | gleich breit |
| `public-sans` | Public Sans, Ziffern Public Sans Figures | sans | uswds/public-sans v2.001 | OFL-1.1 | – | ja (Public Sans Figures) | Public Sans Figures: die Standardziffern auf eine Breite gebracht und mittig (`tnum` wäre nach dem Größenausgleich zu breit) |
| `quattro` | Roboto Flex | sans | google/fonts | OFL-1.1 | – | ja (feste Schnitte) | gleich breit |
| `ribbon` | Selawik | sans | microsoft/Selawik 1.01 | OFL-1.1 | „Selawik“ | nein | gleich breit |
| `roboto` | Roboto (ohne SemiBold) | sans | googlefonts/roboto-3-classic v3.016 | OFL-1.1 | – | nein | gleich breit je Schnitt |
| `rubik` | Rubik, Ziffern Rubik Figures | sans | googlefonts/rubik @e337a5f, 2.300 | OFL-1.1 | – | ja (feste Schnitte, Rubik Figures) | Rubik Figures, `tnum` eingefroren |
| `source` | Source Sans 3, Überschriften Source Serif 4 | sans | adobe-fonts/source-sans 3.052R, adobe-fonts/source-serif 4.005R | OFL-1.1 | „Source“ | nein | gleich breit je Schnitt |
| `source-code-pro` | Source Code Pro | mono | adobe-fonts/source-code-pro 2.042R | OFL-1.1 | „Source“ | nein | gleich breit |
| `work-sans` | Work Sans, Ziffern Work Sans Figures | sans | weiweihuanghuang/Work-Sans @b35c810, 2.012 | OFL-1.1 | – | ja (Work Sans Figures) | Work Sans Figures, `tnum` eingefroren |

Zu prüfen beim Bauen: Höhere Seitenleistenzeilen (Airy 32, Clear 36) schieben in niedrigen Fenstern „Settings“ aus der
Seitenleiste des Kalenders (siehe Plex). Clear braucht dort vielleicht eine rollende Seitenleiste, das wäre eine eigene
Aufgabe in `torro-gui`.

**Ideen für weitere Farbschemata**, nicht als Aufgabe: **High Contrast** (Text 7:1, kräftige Linien, für Clear),
**Graphite** (ohne bunten Akzent, nur Grau, zum Konzentrieren), **Paper** (warmes Sepia, für Folio), **Torro Blue**
(Torro mit blauem Akzent für die, denen Rot zu laut ist).

## Drei Abstandsstufen: Komfort, Standard, Dicht

> **Issue #484, Stand 2026-11-06.** Ersetzt die drei eingebauten Designs Standard, Compact und Console von Issue #291
> (Aufgaben L.31–L.38 in [tasks/design-pakete.md](tasks/design-pakete.md)). Entwurf der Seite:
> `docs/prototypes/abstaende/index.html`.

**Ein Design regelt nur Abstände:** wie viel Luft um Text und Elemente steht, nie *wo* etwas steht, nie Schrift,
Farben oder Mail-Darstellung. Die Suite bringt genau drei Stufen mit:

| Stufe | id | deutsch | Idee |
|---|---|---|---|
| **Comfort** | `comfort` | Komfort | viel Raum um Text und Elemente, ruhig |
| **Standard** | `torro` | Standard | der Standard der Suite, unverändert seit Issue #291: bündiger Inhalt, Haarlinien, Akzentstrich an der gewählten Zeile, Lesespalte mit viel Rand |
| **Dense** | `dense` | Dicht | eng gepackt, mehr auf dem Bildschirm |

- **Die Regel:** Komfort und Dicht setzen nur `[space]`, `[layout]` (ohne `inset`) und die Höhen unter `[size]`
  (`control_sm`, `control`, `control_lg`, `control_xl`, `row`, `row_two`, `row_message`, `row_notice`, `row_sidebar`,
  `row_group`, `row_contact`, `header`, `hour`, `card_gap`, `command`, `pill`, `rail`, `rail_wide`). Schriftstufen (Größe und
  Zeilenhöhe), Radien, Breiten (`sidebar`, `list`, `props`, `panel`, `content_max`, `reading_max` …) und `[list]` erben
  sie vom Standard. Der Text bleibt also in jeder Stufe gleich groß; nur der Rand um ihn wächst oder schrumpft.
- **Eingebaut, eine Datei je Stufe:** `designs/torro/design.toml`, `designs/comfort/design.toml`,
  `designs/dense/design.toml`, beim Bauen in `torro-core` eingebettet (`torro_core::design::BUILT_IN_IDS`, Reihenfolge
  Standard, Comfort, Dense). `list()` und `load(id)` beantworten sie ohne Platte und nehmen keinen Ordner mehr;
  `torro_core::design::dir()` entfällt. Eigene Designs gibt es nicht mehr ([Ein Paket](#ein-paket)).
- **Der Standard ist vollständig:** Seine Datei setzt jeden Schlüssel; `Design::standard()` liest sie einmal je
  Prozess. Fehlt ein Schlüssel oder bricht sie eine Regel, ist der Build kaputt: Ein Test hält ihn auf, das Programm
  bricht mit Grund ab. Werte stehen nicht doppelt im Code. Was Komfort und Dicht weglassen, ändert sich mit dem
  Standard.
- **Dichte in der Liste** rechnet der Kern aus der Höhe der Mail-Zeile (`Design::density_level`): ab 110 Airy, ab 88
  Balanced, ab 56 Dense. Komfort ist Airy, Standard Balanced, Dicht Dense.
- **Namen und Beschreibungen** der drei übersetzt die Suite.
- **Formen ohne Stufe:** `[list] style = "line"` (eine Zeile je Eintrag), `"cards"` (Karten) und
  `selection = "fill"` bleiben im Format und im Code, aber keine eingebaute Stufe schaltet sie ein. Ebenso `inset` > 0
  (die eingesetzte Inhaltsfläche).

**Werte** (Punkte; die Begründung abgeleiteter Werte steht als Kommentar in der jeweiligen Datei):

| Schlüssel | Komfort | Standard | Dicht |
|---|---|---|---|
| `size.row_message` (Mail-Zeile) | 116 | 100 | 78 |
| `size.row_notice` (andere Eingangszeile) | 92 | 80 | 56 |
| `size.row` (Aufgabenzeile) | 56 | 48 | 42 |
| `size.row_two` · `row_group` | 64 · 44 | 56 · 40 | 48 · 32 |
| `size.row_contact` (Kontaktzeile, #352) | 68 | 60 | 52 |
| `size.row_sidebar` | 36 | 32 | 28 |
| `size.control_sm` (Knopf) · `control` (Eingabe) | 40 · 42 | 36 · 38 | 30 · 32 |
| `size.control_lg` · `control_xl` | 44 · 52 | 40 · 48 | 36 · 44 |
| `size.header` · `hour` | 64 · 56 | 64 · 48 | 48 · 40 |
| `size.rail` · `rail_wide` | 64 · 264 | 60 · 248 | 52 · 232 |
| `layout.page_padding` · `panel_padding` | 32 · 20 | 24 · 16 | 16 · 12 |
| `layout.row_padding` | [24, 32] | [18, 24] | [6, 16] |
| `layout.section_gap` · `item_gap` | 32 · 14 | 24 · 10 | 16 · 6 |
| `layout.reading_padding` | [64, 80] | [52, 64] | [32, 40] |
| `space` (xs … xxl) | 4/10/16/20/32/40 | 4/8/12/16/24/32 | 2/6/8/12/16/24 |

Dicht hat `row` 42 statt der geplanten 40: Die Agenda des Kalenders braucht in einer Aufgabenzeile zwei Zeilen (22 und
18) mit `xs` darüber und darunter. Text (Grundgröße 14/22), Radien (4), Breiten und die Darstellung der Listen (`rows`,
Haarlinien, `bar`, Avatar 40) sind in allen drei Stufen die des Standards.

**Gespeicherte Einstellungen** werden beim Lesen umgestellt (`torro_core::design::known_id`, ebenso bei
`settings.update_settings`): `compact` und `console` werden `dense`, jede andere unbekannte id (ein Design aus dem alten
Ordner `designs/`) wird `torro`. `settings.set_setting design` nimmt `compact` und `console` als `dense` an und lehnt
jede andere unbekannte id mit Fehler ab. Wer Console mit JetBrains Mono oder Mails als Text hatte, behält Schrift und
Mail-Darstellung: Sie sind eigene Einstellungen.

Bis Issue #484 gab es statt Komfort und Dicht die Designs Compact und Console (Issue #291). Sie änderten auch Text,
Radien, Breiten und Listen (Console: Text 12/18, eckig, eine Zeile je Mail in `line` mit `fill`, Lesespalte 1200) und
Console schlug eine Schrift und Mails als Text vor. Seit L.34 ersetzt die Form des Standards die von vorher (Text
13/20, Zeile 40, Mail-Zeile 92, eingesetzte Fläche 8, Radien 4/6/8/10, `rail` 52, `rail_wide` 232, keine Lesebreite,
`reading_padding` 24 × 24, Avatar der Mail-Zeile 32).

## Wählen und wechseln

- Gewählt wird in der Steuerzentrale, **Appearance** (Entwurf `docs/prototypes/abstaende/index.html`), in vier
  Gruppen: **Spacing** („Abstände“: Umschalter Comfort | Standard | Dense, darunter eine kleine Mail-Vorschau der
  gewählten Stufe), **Font** („Schrift“: Auswahl, „Reload“, „Open fonts folder“), **Colors** („Farben“: Farbschema,
  Farbmodus System/Light/Dark, „Open color schemes folder“) und **Display** („Anzeige“: Zoom). Breit steht Spacing
  links, Font, Colors und Display rechts untereinander; schmal alle untereinander in dieser Reihenfolge. Einen Ordner
  für Designs gibt es nicht.
- Der Wechsel wirkt **sofort in allen offenen Fenstern**, wie heute hell/dunkel und der Zoom. Hell/dunkel bleibt eine
  eigene Einstellung: Jedes Farbschema hat beide Fassungen (außer „nur dunkel“).
- Eine Stufe wählen setzt nichts anderes mit. Ist die gespeicherte id keine Stufe, gilt die Umstellung oben. Ist eine
  gewählte Schrift oder ein Farbschema weg oder ungültig, gilt der Standard; die Einstellung bleibt, die
  Steuerzentrale zeigt den Grund.

## API und MCP

| Funktion | Oberfläche | API · Zugriff | MCP |
|---|---|---|---|
| Stufen auflisten (id, Name, Beschreibung, Dichte, aktiv) | Appearance › Spacing | `settings.list_designs` · Read | `settings_list_designs` |
| Stufe wählen | Appearance › Spacing | `settings.set_setting { key: design, value: "<id>" }` · Admin (gibt es) | `settings_set_setting` (gibt es) |
| Gewählte Stufe lesen | – | `settings.get_settings` (Feld `design`) | `settings_get_settings` |
| Schrift oder Farbschema installieren / entfernen | Ordner öffnen (Stufe 1) | später | später, frühestens Admin mit Freigabe |

Designs, Schriften und Farbschemata enthalten keine Geheimnisse; Agenten dürfen sie auflisten und (als Admin, mit
Freigabe) wählen.

## Technik (Stufe 1)

- **`torro_core::design`:** Modell, die eingebetteten Designs, Lesen der Ordner für Schriften und Farbschemata,
  Zusammenführen mit dem Standard, Prüfen. Ohne Iced, damit Kern, Steuerzentrale und Tests es nutzen.
- **`torro_gui`:** Die festen Werte werden zu Werten des aktiven Designs. `space::M` wird `space::m()`,
  `typo::BODY` wird `typo::body()`, `palette(theme)` liefert die Farben des aktiven Designs. Das aktive Design steht
  einmal je Prozess (`torro_gui::design::set`/`active`), gelesen beim Start, ersetzt beim Wechsel. Schriften: beim
  Start in `torro_gui::settings()`, beim Wechsel über `iced::font::load`.
- **Gleiches Bild:** Mit dem Standardpaket sieht jede App aus wie vorher. Das ist die Abnahme der Umstellung.
- **Später, Stufe 2:** Schalter in `design.toml` (Block `[arrangement]`), die jede App liest. Welche, klären wir, wenn
  Stufe 1 steht. Kandidaten: Seitenleiste links/rechts/als Reiter, Detail daneben/darüber, Kopfleiste mit Text oder
  nur Icons. Nicht mehr dabei: die Dichte und „Listen als Zeilen oder Karten“ – beides ist `[list] style` in
  Stufe 1 (Issues #226, #291).
- **Später, Herunterladen:** Ein Katalog (z. B. ein Git-Repo mit Paketen), „Install“ lädt, prüft und legt ab. Erst
  wenn es Pakete von anderen gibt.
