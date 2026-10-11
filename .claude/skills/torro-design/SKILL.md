---
name: torro-design
description: Design-Regeln für alle Oberflächen von TorroWorkplace (Iced-Views, Widgets, Theme, Tray-Menüs, Benachrichtigungen). Laden, bevor UI gebaut, geändert oder begutachtet wird – Farben, Abstände, Typografie, Zustände, Barrierefreiheit, Texte, Sichtprüfung per Screenshot.
---

# Torro-Design

## Das Ziel – an jeder Ansicht zu messen

1. **Professionell.** Jede Ansicht wirkt wie aus einer Hand und auf dem Niveau von Linear,
   Things 3, Fantastical oder Amie: ruhig, klar, dicht genug für Arbeit, nie überladen, nichts
   wirkt zufällig oder „Entwickler-Oberfläche“.
2. **Schöne, gut lesbare Schrift.** Eine moderne UI-Schrift (Abschnitt 3), saubere Hierarchie,
   genug Kontrast, nichts zu klein.
3. **Abstände immer richtig.** Jeder Abstand stammt aus der Skala (Abschnitt 2), gleiche Dinge
   haben überall gleiche Abstände, alles liegt auf einer Achse.
4. **In jeder Größe gut.** Die Apps laufen dauerhaft als Kacheln, von der Mini-Kachel bis zum
   halben 32:9-Monitor. Jede Größenklasse hat ein eigenes, durchdachtes Layout (Abschnitt 6).

Ist eines der vier Ziele verfehlt, ist die Ansicht nicht fertig – auch wenn alle Tests grün sind.

## 1. Tokens statt Einzelwerte

- Farben **nur** aus `torro_gui::theme` (`BACKGROUND`, `SURFACE`, `SURFACE_2`, `LINE`, `TEXT`,
  `MUTED`, `FAINT`, `NOW`, `attention()`, Palette-Farben). Kein `Color::from_rgb…` in den Apps.
  Fehlt eine Farbe: in `torro-gui/src/theme.rs` ergänzen (eigene Aufgabe/Rückfrage), nicht lokal.
- Quelle der Wahrheit ist `torro-design/tokens/tokens.json` → `torro-core/src/theme.rs` →
  `torro-gui/src/theme.rs`. (Das Repo `torro-design` liegt nicht im Workspace; bei Zweifeln fragen.)
- **Torro-Rot ist Marke, nie Status.** Ein lauter Moment pro Fenster: Auswahl, Primärknopf,
  Zähler. Fehler = `STATUS_ERROR`, Warten/Abweichung = Attention-Orange, OK = Grün, inaktiv = Grau.

## 2. Raster, Abstände, Radien

- 4er-Raster. Erlaubte Abstände: **4, 8, 12, 16, 24, 32, 48**. Keine 5, 7, 10, 15, 20.
- Innerhalb einer Gruppe enger (4–8), zwischen Gruppen weiter (16–24). Nähe zeigt Zugehörigkeit.
- Fensterrand und Karteninnenabstand: 16 (schmal) bzw. 24 (breit). Einheitlich pro App.
- Radien: 4 (Chips, kleine Flächen), 8 (Knöpfe, Eingaben, Zeilen), 12 (Karten, Dialoge),
  voll rund für Punkte/Avatare. Verschachtelte Radien: innen = außen − Innenabstand.
- Linien sparsam: lieber Fläche (`SURFACE`/`SURFACE_2`) und Abstand als Rahmen. Trennlinien in `LINE`, 1 px.

## 3. Typografie

- **Schrift: Inter** (SIL Open Font License, frei mitzuliefern), die verbreitetste moderne UI-Schrift
  und auf Bildschirmen sehr gut lesbar. Sie wird mit der App ausgeliefert (`include_bytes!` +
  `iced::application(…).font(…)` und `default_font`), damit jede App auf jedem System gleich aussieht.
  Nicht auf die Systemschrift verlassen – `Font::DEFAULT` ergibt je nach System z. B. Liberation
  Sans oder Arial. (Stand heute: noch Systemschrift; das Einbinden ist eine eigene Aufgabe in `torro-gui`.)
- Gewichte: Regular (400) für Text, Medium (500) für Hervorhebungen und Knöpfe, SemiBold (600)
  für Titel. Kein Bold/Black, keine Kursiven für UI.
- Größen: **12** (Abschnittstitel in Versalien, Metadaten), **14** (Fließtext, Listen, Knöpfe),
  **16** (hervorgehobene Zeile), **18** (Ansichts-/Dialogtitel), **24** (Kopfzahl, z. B. Datum).
- Höchstens drei Stufen pro Ansicht sichtbar. Hierarchie über Größe *und* Farbe (`TEXT` → `MUTED`),
  nicht über viele Gewichte.
- Zahlen, die sich ändern (Uhrzeiten, Zähler), sollen nicht springen: gleiche Breite einplanen.
- Lange Texte kürzen mit Auslassung statt Umbruch in Zeilen; in Detailansichten umbrechen.

## 4. Kontrast und Barrierefreiheit (WCAG 2.2 AA)

- Text ≥ 4.5:1, große Schrift (≥ 18) und Bedienelemente/Icons ≥ 3:1.
- Gemessen (dunkel, auf `BACKGROUND`): `TEXT` 16.5, `MUTED` 6.5, `FAINT` **2.4**, Torro-Rot **3.6**.
  → `FAINT` nur für Deaktiviertes und Dekor, **nie** für lesbaren Text (auch nicht Platzhalter).
  → Rot als Textfarbe nur groß oder als Fläche mit weißer Schrift (5.4:1).
- Achtung hell: `MUTED` auf hellem Hintergrund hat nur 2.7:1. In hellen Themes eigene
  Sekundärfarbe verwenden bzw. melden, dass sie fehlt.
- Information nie nur über Farbe: Status zusätzlich mit Text, Icon oder Form.
- Alles per Tastatur bedienbar: Tab-Reihenfolge logisch, `Enter` bestätigt, `Escape` schließt/bricht ab,
  Pfeiltasten in Listen und Rastern. Sichtbarer Fokus.
- Klickziele mindestens 24×24 px, besser 32.

## 5. Zustände – jede Ansicht, jedes Element

- Interaktive Elemente: normal, Hover, gedrückt, Fokus, deaktiviert – alle fünf gestalten.
- Ansichten: **leer** (`widgets::empty_state` mit hilfreichem Hinweis, was zu tun ist),
  **lädt** (ruhiger Platzhalter, kein Springen des Layouts), **Fehler** (was passiert ist + was
  man tun kann + Knopf „Retry“), **voll** (lange Titel, viele Einträge, Überschneidungen).
- Destruktive Aktionen: rückgängig machen anbieten oder bestätigen lassen, nie beides nötig.

## 6. Responsiv für Kachel-Fenster – von winzig bis riesig

Torro-Apps laufen **dauerhaft** als Kacheln in gespeicherten Workspace-Vorlagen, oft viele
nebeneinander. Beispiel beim Nutzer: ein 32:9-Monitor mit 7680×2160 bei Skalierung 2, also
**3840×1080 logische Pixel**. Darauf liegen z. B. Kalender, Aufgaben und Benachrichtigungen als
kleine Kacheln neben einer großen Musik- oder Arbeitsfläche. Jede Ansicht muss deshalb in
**jeder** Größe gut aussehen – nicht nur funktionieren.

**Größenklassen** (logische Pixel, Breite und Höhe getrennt bewerten):

| Klasse | Größe | Zweck | Beispiel Kalender / Aufgaben |
|---|---|---|---|
| **Mini** | Breite < 360 *oder* Höhe < 280 | Auf einen Blick, kaum Bedienung | nächster Termin + „frei bis 14:00“ / Zahl offener Aufgaben + die nächsten 3 |
| **Leiste** | Höhe < 280 *und* Breite ≥ 600 | Waagerechter Streifen | Termine von heute als Zeitstrahl / Aufgaben als Chips nebeneinander |
| **Schmal** | Breite 360–599 | Eine Spalte, volle Bedienung | Agenda oder Tag / Liste, Detail ersetzt die Liste |
| **Mittel** | Breite 600–1023 | Hauptansicht | Woche / Liste + Detail als Overlay |
| **Breit** | Breite ≥ 1024 | Mehr Kontext daneben | Woche + Seitenleiste mit Kalendern / Liste + Detail nebeneinander |

(Leiste geht vor Mini: ein flacher, breiter Streifen ist Leiste, nicht Mini.)

Regeln:

- **Das Layout wechselt, es schrumpft nicht.** Pro Klasse bewusst entscheiden, was gezeigt wird
  (Inhalt nach Wichtigkeit ordnen). Schrift nie kleiner als 12, Klickziele nie kleiner als 24.
- Reihenfolge beim Platzsparen: erst Ränder auf 8–12, dann Seitenleiste einklappen, dann
  Werkzeugleiste auf Icons (mit Tooltip), dann Nebeninfos weglassen. Nie abschneiden, nie
  überlappen, nie horizontal scrollen.
- **Mini und Leiste sind Anzeigen, keine Mini-Apps:** keine Werkzeugleiste, keine Formulare, keine
  Dialoge. Höchstens eine Ein-Klick-Aktion, die den Zweck der Kachel ist (z. B. Aufgabe abhaken).
  Vergrößern übernimmt der Tiling-Manager. Ruhig, ohne Deko, aus der Distanz lesbar.
- **Groß heißt nicht gestreckt:** Textspalten max. ~70 Zeichen; Formulare und Detailansichten
  max. ~640 breit; zusätzlicher Platz geht an Inhalt (mehr Tage, Seitenleiste), nicht an
  Leerraum zwischen Elementen.
- Zustand bleibt beim Größenwechsel erhalten (Auswahl, Scrollposition, gewähltes Datum).
- In Iced liefert `iced::widget::responsive(|size| …)` die verfügbare Größe. Die Klasse daraus an
  **einer** Stelle bestimmen: `torro_gui::layout::of(size)` → `SizeClass` (Aufgabe K.18), nicht
  in jeder View eigene Schwellen.
- **Dauerbetrieb:** nichts blinkt oder bewegt sich ohne Anlass; neu zeichnen nur bei Änderung bzw.
  im Minutentakt (Uhrzeit); keine Spinner im Ruhezustand; geringe CPU- und GPU-Last.
- **HiDPI:** alle Maße in logischen Pixeln; Icons als Vektor (SVG/Canvas), keine Rastergrafik;
  Linien 1 logischer Pixel; bei Skalierung 1 und 2 sauber.
- Die Mindestgröße (`torro_gui::window`, heute 360×320) darf Mini und Leiste nicht verhindern –
  Ziel etwa 200×120.
- Fenstertitel und Inhalt ohne Doppelungen; keine Fake-Titelleiste (der Tiling-Manager zeichnet keine).

## 7. Texte

- Englisch zuerst, alles durch `tr()`/`trf()`. Satzanfang groß, Rest klein („New event“, nicht „New Event“).
- Knöpfe mit Verb („Save“, „Delete event“), nicht „OK“/„Yes“.
- Fehlertexte ohne Schuld und ohne Fachjargon; sagen, was der Nutzer jetzt tun kann.
- Kurz. Wenn ein Wort reicht, kein Satz.

## 8. Bewegung

- Wenn Animation, dann 120–200 ms, ease-out, nur für Orientierung (öffnen, verschieben). Nie Pflicht,
  um etwas zu verstehen.

## 9. Sichtprüfung (Pflicht bei UI-Änderungen)

Tests reichen bei Oberfläche nicht. Jede geänderte Ansicht in **allen Größenklassen** ansehen:

```bash
app=torrocalendar   # bzw. torrotasks
cargo build -p "$app"
"./target/debug/$app" --demo & pid=$!   # nur dieses Fenster anfassen, nicht die des Nutzers
sleep 3
hyprctl dispatch setfloating "pid:$pid"
for s in "240 160" "320 480" "1200 200" "480 900" "800 600" "1600 1000"; do
  hyprctl dispatch resizewindowpixel "exact $s,pid:$pid"; sleep 0.5
  geo=$(hyprctl clients -j | jq -r --argjson p "$pid" '.[] | select(.pid==$p) | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')
  grim -g "$geo" "$SCRATCH/$app-${s/ /x}.png"
done
kill "$pid"
```

Größen: Mini (240×160), Mini hoch (320×480), Leiste (1200×200), Schmal (480×900), Mittel
(800×600), Breit (1600×1000). `$SCRATCH` = Scratchpad-Verzeichnis der Sitzung. Verhindert die
Mindestgröße der App ein Format, ist das ein Befund. Die PNGs mit dem Read-Werkzeug öffnen und
gegen diese Regeln prüfen. Läuft kein Hyprland, Sichtprüfung als „nicht möglich“ melden.

## 10. Prüfliste vor „fertig“

- [ ] Ziel 1–4 erfüllt: professionell, gut lesbare Schrift (Inter, Skala, Gewichte), Abstände aus der Skala, in jeder Größe gut
- [ ] Keine Farb-/Größenwerte außerhalb von Theme und den Skalen oben
- [ ] Ein Akzent pro Fenster; Rot nicht als Status
- [ ] Kontraste eingehalten, Status nicht nur über Farbe
- [ ] Leer-, Lade-, Fehlerzustand vorhanden
- [ ] Tastatur: Fokus sichtbar, Enter/Escape/Pfeile
- [ ] In allen Größenklassen angesehen (Mini, Leiste, Schmal, Mittel, Breit): Layout wechselt sinnvoll, nichts abgeschnitten, groß nicht gestreckt
- [ ] Hell und dunkel bedacht
- [ ] Alle Texte über `tr()`, englisch, Satzschreibung
