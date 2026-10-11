# Zwei Modi: Tiling und Fenster

Vorgemerkt am 2026-10-11. Status: **Konzept**, Entscheidungen unten offen.

## Warum

Die Suite ist für Tiling-Window-Manager gedacht: Der Window Manager *ist* das Dashboard (siehe
[vision.md](vision.md)). Die meisten Menschen haben aber keinen Tiling-Manager – auf macOS und
Windows ist er die Ausnahme, unter Linux laufen GNOME und KDE meist schwebend. Für sie wären viele
kleine, lose Fenster mühsam: Sie überlappen, verschwinden hinter anderen und lassen sich nicht
zuverlässig wieder anordnen.

Darum gibt es zwei Modi. Beide sind gleichwertig; die Apps, Daten und Konten sind dieselben.

| | **Tiling-Modus** | **Fenster-Modus** |
|---|---|---|
| Für wen | Hyprland, AeroSpace, GlazeWM | alle anderen (GNOME, KDE, macOS, Windows ohne Tiling) |
| Anordnung | der Window Manager kachelt die App-Fenster | eine Torro-Fläche (Arbeitsfläche), auf der die Elemente frei liegen |
| Vorlagen | Workspace-Vorlagen über das WM-Backend (`torro-wm`) | Flächen-Vorlagen, von Torro selbst gespeichert und aufgebaut |
| Ein Element ist | ein eigenes Fenster mit fester App-ID | ein Element auf der Fläche; bei Bedarf auch als eigenes Fenster öffenbar |

## Der Fenster-Modus

Eine **Arbeitsfläche** ist ein einziges Fenster der Suite (eigene App-ID, z. B. `torro-board`).
Darauf liegen die Elemente der Suite – Kalender, Aufgaben, später Benachrichtigungen, Musik, Agenten
usw. Name für die Elemente noch offen (siehe unten); hier vorläufig „Element“.

- **Frei positionieren:** Elemente per Ziehen verschieben und an Kante oder Ecke in der Größe ändern.
  Sie rasten auf ein Raster ein (Vielfaches von 8 logischen Pixeln), damit Abstände immer stimmen –
  frei, aber nie schief. Überlappen ist nicht erlaubt; ein gezogenes Element schiebt andere nicht weg,
  sondern zeigt, wo es passt.
- **Bearbeiten und Benutzen getrennt:** Ein Schalter „Anordnen“ blendet Griffe, Raster und den
  Katalog ein. Im normalen Betrieb ist die Fläche ruhig und alle Elemente sind voll bedienbar.
- **Katalog:** alle verfügbaren Elemente mit Vorschau, per Ziehen auf die Fläche.
- **Mehrere Flächen** (z. B. „Tag“, „Fokus“, „Planung“) mit schnellem Umschalten, Tastenkürzel und
  Tray-Menü – das Gegenstück zu den Workspace-Vorlagen.
- **Startaufbau:** Beim ersten Start legt Torro eine fertige Fläche an (z. B. Kalender groß, daneben
  Aufgaben und Benachrichtigungen klein), damit niemand vor einer leeren Fläche sitzt. Weitere
  fertige Flächen zur Auswahl.
- **Größenklassen gelten genauso:** Jedes Element bekommt seine Größe auf der Fläche und wählt danach
  Mini, Leiste, Schmal, Mittel oder Breit (`torro_gui::layout`, Aufgabe K.18). Die Mini- und
  Leisten-Ansichten aus K.19/K.20 sind damit zugleich die kleinen Elemente der Fläche.
- **Herauslösen:** Ein Element lässt sich als eigenes Fenster öffnen (dieselbe App wie im
  Tiling-Modus), z. B. um den Kalender kurz groß zu sehen.
- Die Fläche selbst ist ein normales Fenster: maximiert, im Vollbild oder auf einem zweiten Monitor.
  Als Desktop-Hintergrund (Layer-Shell unter Wayland) später prüfen; nicht Voraussetzung.

## Warum eine eigene Fläche und nicht viele lose Fenster

- **Wayland:** Unter GNOME und KDE darf ein Programm seine Fenster nicht selbst positionieren. Eine
  Vorlage aus losen Fenstern ließe sich dort nicht wieder aufbauen. Innerhalb unserer eigenen Fläche
  bestimmen wir die Anordnung vollständig – auf jedem System gleich.
- **Ein Prozess, wenig Last:** Eine Fläche mit fünf Elementen braucht ein Fenster und eine
  GPU-Fläche statt fünf. Passt zu „Leicht“ aus den Prinzipien.
- **Gestaltung aus einem Guss:** gemeinsamer Hintergrund, gleiche Abstände zwischen den Elementen,
  keine fremden Fensterrahmen dazwischen.

## Was das für die Architektur bedeutet

- **Jede App wird eine Bibliothek plus dünnes Programm.** Zustand, `update` und `view` liegen im
  Bibliotheksteil (z. B. Crate `torrocalendar` mit `lib.rs` und `main.rs`), damit die Fläche dieselbe
  App als Element einbetten kann. Das Programm `torrocalendar` bleibt für den Tiling-Modus.
  **Je früher, desto billiger**, weil die Apps sonst weiter auf „ein Fenster“ zugeschnitten wachsen.
- **Ein gemeinsames Element-Interface** in `torro-gui`: Name, Icon, Mindest- und Standardgröße,
  `update`, `view(size_class)`, Abo für Zeitgeber und Daten. Kalender und Aufgaben implementieren es
  zuerst.
- **Die Fläche ist ein Client des Kerns** wie jede App (IPC). Sie startet den Kern, wenn er nicht
  läuft.
- **Flächen-Vorlagen** speichert der Kern neben den Workspace-Vorlagen (eigenes Format, versioniert):
  Fläche → Elemente mit Typ, Position und Größe in Rastereinheiten und ihren Einstellungen
  (z. B. welche Kalender ein Element zeigt, vgl. [settings.md](settings.md) „pro Fenster“).
- **Modus wählen:** Beim ersten Start erkennt der Kern den Window Manager (`torro_wm::detect`).
  Tiling-Manager gefunden → Tiling-Modus vorschlagen, sonst Fenster-Modus. Umschaltbar in der
  Steuerzentrale (Seite General). Die Fläche darf auch im Tiling-Modus genutzt werden, z. B. als eine
  Kachel mit mehreren kleinen Elementen.
- Tray-Menü, MCP und API zeigen je nach Modus Workspace- oder Flächen-Vorlagen.

## Umsetzung in Schritten

1. **Apps aufteilen** in Bibliothek + Programm (Kalender, Aufgaben), ohne Funktionsänderung.
2. **Element-Interface** in `torro-gui` festlegen (Opus), Kalender und Aufgaben anbinden.
3. **Fläche, nur anzeigen:** neue App `torro-board` mit fester Startfläche aus einer Vorlage.
4. **Anordnen:** Modus „Anordnen“ mit Raster, Ziehen, Größe ändern, Katalog.
5. **Flächen-Vorlagen** im Kern: speichern, laden, mehrere Flächen, Tray und Tastenkürzel.
6. **Modus-Wahl** beim ersten Start und in der Steuerzentrale; Startaufbau.
7. Design-Prüfung der Fläche (`torro-design-review`) mit mehreren Elementen in allen Größenklassen.

Schritt 1 und 2 sollten vor weiteren großen App-Aufgaben (C.*, T.* aus
[tasks/design.md](tasks/design.md)) kommen oder mit ihnen abgestimmt werden.

## Offen

- **Name der Elemente:** „Widgets“ (verbreitet, sofort verständlich), „Kacheln“ (passt zu Tiling, kann
  aber verwechselt werden), „Module“, „Karten“. Vorschlag: **Widgets** (Englisch wie Deutsch).
- **Name der Fläche:** „Board“, „Desk“, „Workplace“ (greift den Produktnamen auf: *der* Torro
  Workplace). Vorschlag: **Workplace**, Binary `torroworkplace board` oder eigenes `torroboard`.
- Elemente frei in Pixeln oder nur im Raster? Vorschlag: Raster (8 px), wirkt immer aufgeräumt.
- Dürfen Elemente einer Fläche Einstellungen teilen (z. B. ein Kalender-Filter für alle)?
- Desktop-Hintergrund-Modus (Layer-Shell, macOS-Desktop-Ebene) – lohnt sich das?
