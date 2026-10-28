# Pinboard: App `torropinboard` — das Schwarze Brett

Stand: 2026-10-28. **Konzept, Marktforschung und Entscheidungen vom 2026-10-28 eingearbeitet.**
Prototyp: [prototypes/pinboard](prototypes/pinboard/) (interaktiv, drei Gestaltungsrichtungen).
Stufe P1: Themen-Issue [#348](https://github.com/mahype/TorroWorkplace/issues/348) mit den Sub-Issues
#362–#371; dort stehen Plan und Arbeitsschritte.
Beantwortet die Alltagsfrage: Wo hängt das Kurzzeitige — Screenshots, Textfetzen, Erinnerungen,
die weder einen Kalendertermin noch eine Notiz verdienen?

## Ziel

Ein Brett in der Suite, an dem Kurzzeitiges hängt: Texte, Checklisten, Bilder und Screenshots,
Links, Dateien — frei angeordnet, Farbe als Kategorie, mit Ablaufdatum je Zettel. Es ist ein
**Platz neben der Arbeit, nicht noch ein Arbeitsplatz**: zum Blicken gemacht, nicht zum
Verwalten; **Blick, nicht Glocke** (Ablaufdatum statt Erinnerungs-Ping).

Wie jede Funktion der Suite dreimal: Oberfläche `torropinboard`, API (Bereich `pinboard`),
MCP (`pinboard_*`), also auch für Agenten.

## Was draußen existiert (Recherche 2026-10-28)

| Produkt | Kernidee | Was wir daraus nehmen |
|---|---|---|
| [Cork](https://usecorkboard.app/) (Mac, gratis, lokal) | Ambient-Schicht: eine Taste, das Brett gleitet vom Rand ein/aus; „zum Blicken, nicht zum Verwalten"; pinnt Text, Markdown, Checklisten, Bilder, Links, Dateien, Farbfelder; Karten verbinden (rote Schnur); komplett lokal | **Charakter** (Leitstern-Zitate), **globale Taste**, Verbindungen als spätere Idee |
| [StarMap](https://digitalsheriffdesigns.com/starmap/) (Windows, gratis, offline) | freie Milanote-Alternative: **Boards in Boards** (innen sichtbar ohne Öffnen), Brett als Vorlage, haftende Verbindungslinien, Daten als gewöhnliche Dateien im Ordner, `.starmap`-Einzeldatei | **Struktur**: Brett als Zettel, Vorlagen je Brett, Daten als gewöhnliche Dateien |
| [PinSlip](https://pinslip.app/) (offen, plattformübergreifend) | Haftnotizen mit magnetischen Gruppen, **eigener MCP-Server für Agenten** | bestätigt den Agenten-Weg |
| [Ciiboard](https://ciiboard.com/) (Windows) | Zwischenablage-Inseln, Screenshots über Fenstern, zeigt Agenten-Status | Screenshots als gleichwertige Bürger; Kontext-Board (unten) |
| [Tastemaker](https://www.tastemaker.site/) | Bild-Moodboards schwebend über der Arbeit | nichts Conkretes; stützt Mini-Kachel |
| Apple Stickies, Windows Sticky Notes, Google Keep | tippen und loslassen; Farbe als Kategorie, Erinnerungen | Farben ja, Glocke nein |
| Milanote, Miro, FigJam, Obsidian Canvas, Scapple | unendliche Leinwand, Zoom, Verbindungen, Team-Werkzeuge | **bewusst nicht**: das Brett ist Nebenbeibrett, kein Werkzeugkasten |
| PureRef | Bild-Pinnwände als Referenz | Screenshot-Zettel |
| [Cork (cork.is)](https://www.cork.is/) — **andere App**, Web-Pinnwand für Autoren | Indexkarten, `[[Links]]` | nur zur Namensverwechslung notiert |

## Entscheidungen (2026-10-28)

1. **Name:** App `torropinboard`, Bereich `pinboard`, MCP-Tools `pinboard_*`, Fensterklasse
   `torro-pinboard`. Englisch „Pinboard", deutsch „Schwarzes Brett" (`i18n/de.rs`). Nicht
   „Board": der Name ist im Fenster-Modus ([modes.md](modes.md)) vergeben.
2. **Lokal zuerst:** Zettel und Metadaten in SQLite im Datenverzeichnis des Kerns, Bilder als
   **gewöhnliche Dateien** daneben (Vorbild StarMap: Ordner kopieren genügt als Backup). Kein
   Konto, keine Anmeldung; Sync wäre später optional und nie Voraussetzung.
3. **Leitstern** (Cork übernommen): „Ein Platz neben der Arbeit, nicht noch ein Arbeitsplatz",
   „zum Blicken, nicht zum Verwalten". Das Brett ist Blick, nicht Glocke: Ein Ablaufdatum
   verblasst den Zettel, es pingt nicht.
4. **Charakter von Cork, Struktur von StarMap:** schmal und schnell im Umgang; die eine
   übernommene Struktur-Idee ist das **Brett als Zettel** (Stufe P4, s. u.).
5. **Typen:** Text (Klartext, kein Markdown — dafür gibt es Notizen), Checkliste, Bild und
   Screenshot, Link, Datei (Pfad). Farbfeld-Zettel (Cork) als spätere Idee.
6. **Vergänglichkeit:** Ablauf je Zettel („bis morgen", „bis nächste Woche", Datum); danach
   verblasst der Zettel (55 % Deckkraft). „Aufräumen" wischt Abgelaufenes und Abgehaktes fort
   (mit Undo). „Als Notiz speichern" hebt einen Zettel nach `torronotes` auf — das Brett ist
   das Vorzimmer der Notizen.
7. **Einfangen:** Einfügen (⌘V) und Ziehen von Dateien aufs Brett; später Screenshot direkt,
   **globale Taste über den Kern** (wie die Workspace-Tasten, [core.md](core.md): Druck holt
   das Brett, nochmal drücken schiebt es weg — Corks Kernidee) und „Pin to board…" im
   Tray-Menü.
8. **Farben:** die fünf Standardfarben der Suite, als Balken und Tönung wie Terminfarben im
   Kalender — nie Bedeutung allein über Farbe.
9. **Agenten:** `pinboard.create_pin` ohne Freigabe — es überschreibt nichts (wie
   `notes.append_to_note`). Inhalt überschreiben und löschen ist `Delete` mit Freigabe.
   Zettelinhalt ist immer `untrusted: true`: In Bretter landet Text von Agenten, Skripten und
   (über den Eingang) aus Mails.
10. **Gestaltungsrichtung B** (Ruhiges Raster), in der Planung von #348 entschieden. Vor der
    Umsetzung der App stimmen Mockups aller Größenklassen sie ab (#362).
11. **Lage als Anteil, Größe fest** (Planung #348): `x`/`y` sind Anteile 0 … 1 der Brettfläche,
    die Breite ist je Typ fest (Token), die Höhe folgt dem Inhalt. Der Magnet rastet beim Ablegen
    auf 8 px. P1 kennt keine Größe; sie käme später als optionales Feld.
12. **Öffnen** (Planung #348): Link-Zettel nur mit `http`/`https`; Datei-Zettel öffnen mit dem
    Programm des Systems, außer Dateien, die Programme starten könnten — dann den Ordner, wie
    Anhänge im Mail-Fenster.

## Aufbau

```
torropinboard (App)  ── IPC ──▶  torroworkplace (Kern)
                                ├─ pinboard/service.rs   Befehle, Rechte, Freigabe-Fakten
                                ├─ pinboard/store.rs      SQLite: Bretter, Zettel, FTS5-Suche
                                └─ pinboard/files.rs      Bilddateien im Datenverzeichnis (P2)
```

- **Kennungen:** Brett `board-<16 hex>`, Zettel `pin-<16 hex>`. Die Lage ist ein Anteil der
  Brettfläche (0 … 1), damit das Brett in jeder Kachelgröße gleich angeordnet ist; die Größe ist
  fest (Entscheidung 11).
- **Ablage:** `pinboard/pinboard.sqlite` im Datenverzeichnis des Kerns (Ordner 0700, Datei 0600).
  Es gibt immer mindestens ein Brett: Der Kern legt beim ersten Öffnen „Pinboard“ an, das letzte
  Brett lässt sich nicht löschen.
- **Kein Konto, kein Anbieter:** anders als Notizen braucht das Brett keinen Dienst. Gibt es
  später Sync, wäre er ein Ordner-Anbieter (N4-Gedanke aus [notizen.md](notizen.md)).
- **Sicherheit:** Kein Zetteltext im Protokoll (Suchbegriffe, Texte, Dateinamen nie; Bildpfade
  nur als Prüfsumme, `audit::ids` wie bei Notizen). Datenbank und Bilddateien sind 0600.
  Freigaben zeigen Zettel-Titel und „alt → neu" aus dem Cache des Kerns, nie aus dem Text des
  Clients.

## Befehle (Bereich `pinboard`)

| Stufe | Befehl | Zugriff | MCP |
|---|---|---|---|
| P1 | `pinboard.list_boards {}` (mit Zahlen) | Read | `pinboard_list_boards` |
| P1 | `pinboard.list_pins { board?, query?, kind?, color?, limit? }` | Read | `pinboard_list_pins` |
| P1 | `pinboard.create_pin { board?, content, position?, color? }` | Write | `pinboard_create_pin` |
| P1 | `pinboard.update_pin { id, content?, position?, color? }` | Write, mit `content` **Delete** | `pinboard_update_pin` |
| P1 | `pinboard.delete_pin { id }` | Delete | `pinboard_delete_pin` |
| P1 | `pinboard.create_board { name }` / `pinboard.rename_board { id, name }` | Write | `pinboard_create_board` … |
| P1 | `pinboard.delete_board { id }` | Delete | `pinboard_delete_board` |
| P3 | `pinboard.list_pins` mit `include_faded` · `pinboard.update_pin` mit `expiry` · `pinboard.tidy_board { board? }` (Aufräumen) | Read · Write · **Delete** | `pinboard_list_pins` · `pinboard_update_pin` · `pinboard_tidy_board` |
| P3 | `pinboard.save_pin_as_note { id, category? }` (legt in `notes` an) | Write | `pinboard_save_pin_as_note` |
| P4 | `pinboard.create_pin` mit `type: board` (Zettel, der ein Brett öffnet) | Write | `pinboard_create_pin` |
| P4 | `inbox.create_pin_from_item { id }` (wie „Save as note") | Write | `inbox_create_pin_from_item` |

Erläuterungen:

- **Kein Befehl steht auf `Never`.** Es gibt keinen Admin-Befehl: Das Brett liegt im
  Datenverzeichnis des Kerns, kein Befehl berührt Pfade außerhalb.
- **Bilder** (P2) gehen als `data` in `create_pin`, der Kern schreibt die Datei und antwortet
  mit der Kennung; es gibt keinen eigenen Anhang-Befehl.
- **`Rights.pinboard`** ist eine Stufe ohne Auswahl (kein Kategorien-Gefüge wie bei Notizen);
  `Rights::assistant()` bekommt `pinboard` mit Voll und `confirm: OnDelete`. Wie jeder später
  hinzugekommene Bereich bleibt `pinboard` für bestehende Clients zu, bis ein Mensch ihn öffnet.
- **`content`** ist getypt (`type`: `text`, `checklist`, `link`, `file`); Abhaken einer
  Checkliste ersetzt den Inhalt und ist für Agenten damit `Delete`. Jeder Zettel nennt mit `by`
  den Client, der ihn angelegt oder zuletzt im Inhalt geändert hat (leer: der Mensch). `by` ist nie ein
  Feld eines Requests; ein über Undo wieder angelegter Zettel eines Agenten hat danach kein `by`.
- **Freigaben** halten den Stand fest (wie `EntryPin` im Eingang): Hat sich der Zettel oder das Brett bis
  zur Ausführung geändert, endet sie mit `conflict`.

## Oberfläche

Nach dem Prototyp in [prototypes/pinboard](prototypes/pinboard/):

- **Fenster** (`torro_gui::shell`): Seitenleiste mit den Brettern und der Zeile
  „Local on this device“ („Archive“ erst mit P3); Kopfleiste mit Suche und „New pin“; das Brett
  mit Punktraster und **Magnet auf dem 8-px-Raster des Fenster-Modus** (frei anordnen, nie
  schief). Jeder Zettel zeigt beim Daraufzeigen Farbpunkte und Entfernen (Ablauf mit P3);
  Entfernen meldet sich mit Undo. Suche dimmt, was nicht passt. In Schmal stehen die Zettel als
  Liste untereinander (ohne Ziehen).
- **Mini-Kachel:** Kopfzahl offener Zettel, die neuesten drei, Schnelleingabe „Quick note…".
  Mini und Leiste sind Anzeigen; das große Fenster ist das Brett.
- **Steuerzentrale:** Rechte-Zeile `pinboard` (Stufe, `confirm: OnDelete`).
- **Tray** (P3): „Pin to board…" öffnet die Schnelleingabe.
- **Texte englisch zuerst** (`tr`/`trf`), Deutsch in `crates/torro-core/src/i18n/de.rs`:
  „Schwarzes Brett", „Zum Brett anpinnen…", „Zettel entfernt" usw.

## Gestaltungsrichtungen (Prototyp)

Gleiche Zettel, gleiche Handgriffe, drei Charaktere — im Prototyp oben umschaltbar:

| | A Korkbrett | B Ruhiges Raster (**gewählt**) | C Zonen |
|---|---|---|---|
| Bild | Holz, Papier, Pins, gedrehte Zettel | Punktraster, Karten wie Terminfarben | wie B, beschriftete Bereiche |
| Stärke | sofort verständlich, warm | Torro-Designsprache zu Ende gedacht, ruhig neben der Arbeit | Struktur ohne Zwang |
| Risiko | Spielzeug-Look, unruhig bei vielen Zetteln | weniger selbsterklärend | Zonen können als Pflicht gelesen werden |

## Stufen

| Stufe | Inhalt |
|---|---|
| P1 | Kern: SQLite, Bretter, Text-, Checklisten-, Link- und Datei-Zettel, Farben, Position; App mit Ziehen und Suche; API und MCP; Agenten pinnen an |
| P2 | Bilder und Screenshots: Dateien im Datenverzeichnis, Einfügen (⌘V), Ziehen von Dateien, aus Mail speichern |
| P3 | Ablauf und Aufräumen: `expiry`, Verblasen, `tidy_board`, „Als Notiz speichern"; globale Taste über den Kern; „Pin to board…" im Tray |
| P4 | Mehrere Ebenen: **Brett als Zettel** (flach, Tiefe 1–2, Vorbild StarMap), Brett als Vorlage speichern, Einfügen aus dem Eingang (`inbox.create_pin_from_item`), Verbindungslinien als Idee |

Jede Stufe ist für sich nutzbar und über alle drei Wege erreichbar.

## Einordnung in die Suite

| App | Ihre Frage | Verhältnis zum Brett |
|---|---|---|
| Notizen ([notizen.md](notizen.md)) | „Wo lege ich etwas ab?" | Dauerhafte Ablage. Das Brett ist das Vorzimmer: „Als Notiz speichern" hebt auf. |
| Eingang ([eingang.md](eingang.md)) | „Was ist reingekommen?" | Fluss. „Pin to board" hält fest, was über den Tag bleiben soll. |
| Kontext-Board (Idee im [App-Katalog](app-catalog.md)) | „Woran arbeiten die Agenten?" | Würde ein von Agenten gefülltes Brett — als **Brett als Zettel** darauf (P4). |
| Fenster-Modus ([modes.md](modes.md)) | „Wie verteile ich die Suite ohne Tiling?" | Arbeitsfläche für App-Ansichten; deshalb heißt diese App nicht „Board". |

## Offen (im jeweiligen Issue klären)

- **Anzeigename** deutsch: „Schwarzes Brett" für die App, „Zettel" statt „Pin"? (Vorschlag:
  App „Schwarzes Brett", Objekte „Zettel", Verb „anpinnen".)
- **Globale Taste:** Modifikator wie die Workspace-Tasten? Verhalten im Tiling: eigenes
  floating Fenster oder Fokus auf die Kachel?
- **Markdown in Text-Zetteln:** Vorschlag nein (Klartext), Notizen sind der Ort für Markdown.
- **Verbindungslinien** (rote Schnur, Cork/StarMap): ob und in welchem Umfang in P4.
- **Farbfeld-Zettel** (Cork): Idee für P4, wenn Bedarf entsteht.
- **Sync:** später optional als Ordner-Anbieter; nichts davon blockiert P1–P3.