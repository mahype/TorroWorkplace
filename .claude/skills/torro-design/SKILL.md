---
name: torro-design
description: Design-Regeln für alle Oberflächen von TorroWorkplace (Iced-Views, Widgets, Theme, Tray-Menüs, Benachrichtigungen). Laden, bevor UI gebaut, geändert oder begutachtet wird – Farben, Abstände, Typografie, Zustände, Barrierefreiheit, Texte, Sichtprüfung per Screenshot.
---

# Torro-Design

Ziel: Jede Ansicht soll wirken wie aus einer Hand und auf dem Niveau von Linear, Things 3,
Fantastical oder Amie – ruhig, klar, dicht genug für Arbeit, nie überladen. Torro-Apps leben
gekachelt neben anderen Fenstern: Sie müssen auch schmal und klein gut aussehen.

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

## 6. Layout für Kachel-Fenster

- Muss bei **360×320** (Mindestgröße), ~**500** px Breite (halbe Kachel) und 1100×760 funktionieren.
- Werkzeugleisten umbrechen statt abschneiden; Seitenleisten einklappen, wenn schmal.
- Kein horizontales Scrollen in Listen. Inhalte links ausrichten, eine klare Achse pro Spalte.
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

Tests reichen bei Oberfläche nicht. Ansehen:

```bash
cargo build -p torrocalendar   # bzw. torrotasks
./target/debug/torrocalendar --demo &
sleep 2
geo=$(hyprctl clients -j | jq -r '.[] | select(.class=="torro-calendar") | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')
grim -g "$geo" "$SCRATCH/calendar.png"
```

(`torro-tasks` für Aufgaben. `$SCRATCH` = Scratchpad-Verzeichnis der Sitzung.) Das PNG mit dem
Read-Werkzeug öffnen und gegen diese Regeln prüfen. Für schmale Fenster vorher
`hyprctl dispatch setfloating class:torro-calendar` und
`hyprctl dispatch resizewindowpixel exact 500 700,class:torro-calendar`, dann neu messen und
aufnehmen. Danach die App beenden
(`pkill -x torrocalendar`). Läuft kein Hyprland, Sichtprüfung als „nicht möglich“ melden.

## 10. Prüfliste vor „fertig“

- [ ] Keine Farb-/Größenwerte außerhalb von Theme und den Skalen oben
- [ ] Ein Akzent pro Fenster; Rot nicht als Status
- [ ] Kontraste eingehalten, Status nicht nur über Farbe
- [ ] Leer-, Lade-, Fehlerzustand vorhanden
- [ ] Tastatur: Fokus sichtbar, Enter/Escape/Pfeile
- [ ] Bei 360 px und 500 px Breite angesehen
- [ ] Hell und dunkel bedacht
- [ ] Alle Texte über `tr()`, englisch, Satzschreibung
