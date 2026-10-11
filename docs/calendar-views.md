# Kalender: alternative Darstellungen

Stand: 2026-10-09. Ideensammlung für `torrocalendar` (TUI) und die spätere Kalender-GUI (Phase 3, Technik noch offen).
Noch nichts davon ist entschieden. Die Prototypen sind klickbare HTML-Seiten mit Beispieldaten
und dienen nur dazu, das Gefühl der Darstellung zu prüfen.

## Ausgangslage

Die üblichen Ansichten (Monatsraster, Wochenraster, Stundenliste) wirken überholt. Gesucht ist
eine Darstellung, die eher dem entspricht, wie man einen Tag oder ein Jahr im Kopf hat.

## Prototypen

| Prototyp | Datei | Status |
|---|---|---|
| Tagesspirale | [prototypes/calendar/tagesspirale.html](prototypes/calendar/tagesspirale.html) | **Favorit** für die Tagesansicht |
| Jahresring | [prototypes/calendar/jahresring.html](prototypes/calendar/jahresring.html) | Konzept für Zoom von Jahr bis Tag |

Lokal ansehen: Datei im Browser öffnen, sie braucht keinen Server.

### Tagesspirale (Favorit)

![Tagesspirale](prototypes/calendar/tagesspirale.png)

Ein normales 12-Stunden-Zifferblatt, auf dem die 24 Stunden als Spirale mit zwei Windungen
liegen. Die Spirale beginnt um 0 Uhr oben **innen**, läuft einmal herum und wechselt um 12 Uhr
auf die **äußere** Windung für Nachmittag und Abend.

Warum sie überzeugt:

- Sie entspricht dem Bild, das man im Kopf hat, wenn man durch den Tag geht: eine Uhr,
  aber ohne die Mehrdeutigkeit von Vormittag und Nachmittag.
- Innen nach außen: Die Nacht liegt auf der kürzesten Windung und nimmt wenig Fläche ein.
  Nachmittag und Abend, wo meist die meisten Termine liegen, haben die längste Windung.
- Der Stundenzeiger zeigt wie auf einer echten Uhr, ein leuchtender Punkt markiert die
  aktuelle Windung.

Gestaltungsregeln, die sich beim Ausprobieren ergeben haben:

- **Gleichmäßigkeit ist Pflicht.** Die Spur hat überall dieselbe Breite, und die Spiralform
  muss durchgehend sichtbar sein: feine Kante innen und außen, Nachtfarbe klar vom Hintergrund
  abgesetzt. Sonst entstehen optische Knicke.
- **Keine Kreisform gegen die Spirale stellen.** Ein umrandeter Mittelkreis lässt die
  Abstände ungleich wirken. Die Mitte bleibt deshalb ohne Rahmen.
- **Zeit nicht verzerren.** Jede Stunde bekommt denselben Winkel, sonst passt es nicht mehr zur Uhr.
- Termine als abgerundete Bögen, Beschriftung entlang des Bogens und immer aufrecht lesbar.
  Sehr kurze Termine werden zu Punkten.
- Überlappende Termine teilen sich die Spur in parallele Bahnen.
- Fristen als Markierung quer zur Spur.
- Tageslicht als weicher Farbverlauf der Spur, Tagesfortschritt als dünne Linie am Rand.
- In der Mitte: Uhrzeit, „läuft noch 19 min“ bzw. „frei für 1 h 20 min“ und der nächste Termin.

Ausprobiert und verworfen:

| Variante | Warum verworfen |
|---|---|
| Spirale von außen nach innen | Die innere Windung wird zur Mitternacht hin eng; die Abendtermine haben wenig Platz. Bleibt im Prototyp als Umschalter zum Vergleich. |
| Zwei konzentrische Ringe (0–12 außen, 12–24 innen) mit Übergang bei 12 Uhr | Wirkt geordnet, aber der harte Sprung an der Naht und das Teilen von Terminen über 12 Uhr überzeugen nicht. |
| Nacht schmal darstellen (Spur im Schlaf dünner) | Sieht unförmig aus. |
| Nacht zeitlich stauchen (Schlaf bekommt weniger Winkel) | Passt nicht mehr zu den Uhrzeiten und zum Gefühl für den Tag. Kommt nicht in Frage. |

### Jahresring

![Jahresring, Woche](prototypes/calendar/jahresring-woche.png)

Ein runder Kalender, der wie eine Karte von **Jahr → Monat → Woche → Tag** zoomt. Ein Klick auf
ein Segment klappt es zum vollen Kreis auf. Scrollen zoomt, Ziehen dreht durch die Zeit.

Die Ringe sind wie Jahresringe eines Baums nach Typ getrennt, von innen nach außen:

1. **Termine** mit Uhrzeit, breitester Ring
2. **Fristen**
3. **Ganztägig** (Urlaub, Reisen, Konferenzen)
4. **Anlässe** (Geburtstage, Feiertage)
5. **Kontext** (Tageslicht, Wochenenden, Jahreszeiten), sehr dünn

Termine liegen innen, alles, was keine direkten Termine sind, außen und dünner.
Ganztägiges und Geburtstage überlagern keine Termine, weil sie eigene Ringe haben.

**Semantic Zoom:** Die Ringe ändern ihre Darstellung je Zoomstufe.

- Termine: im Tag und in der Woche als Bögen mit Titel, ab dem Monat als Heatmap der Auslastung
- Ganztägig und Anlässe: im Tag eingeklappt; was ansteht, erscheint in der Mitte als Chip
- Kontext: Tag/Nacht im Tag, Wochenenden in Woche und Monat, Jahreszeiten im Jahr

| Jahr | Tag | Spirale (Woche) |
|---|---|---|
| ![Jahr](prototypes/calendar/jahresring-jahr.png) | ![Tag](prototypes/calendar/jahresring-tag.png) | ![Spirale](prototypes/calendar/jahresring-spirale-woche.png) |

Offen:

- Gruppierung der Ringe **nach Kalender** statt nach Typ, sobald mehrere Kalender verbunden sind
  (im Prototyp als deaktivierte Option angelegt). Die meisten Nutzer haben nur einen Kalender,
  deshalb ist „nach Typ“ der Standard.
- Die Spiralvariante des Jahresrings (eine Windung = eine Einheit) wirkt im Jahr zu dünn.
- Die Tagesansicht des Jahresrings könnte durch die Tagesspirale ersetzt werden.

## Vorschlag 2026-10-11: Zeitstrahl und Radar

Noch nicht entschieden, steht zur Besprechung. Die Prototypen nutzen die Tokens aus
`docs/prototypes/linear/torro.css`. Ansehen: `python3 -m http.server` im Repo starten, dann
`/docs/prototypes/calendar/zeitstrahl.html` bzw. `radar.html` öffnen (Schriften werden relativ geladen).

| Prototyp | Datei | Ersetzt |
|---|---|---|
| Zeitstrahl | [prototypes/calendar/zeitstrahl.html](prototypes/calendar/zeitstrahl.html) | neue Ansicht „Timeline“ (im Linear-Entwurf schon vorgesehen) |
| Radar | [prototypes/calendar/radar.html](prototypes/calendar/radar.html) | Tagesspirale |

### Zeitstrahl nach dem Vorbild HEY Calendar

Was HEY macht ([Kalender-Seite](https://www.hey.com/calendar/), [Video-Rundgang mit Jason Fried](https://www.youtube.com/watch?v=SztU4232u_o),
[Nighttime](https://help.hey.com/article/846-nighttime)):

- **Filmstreifen statt Raster:** Der Tag läuft waagerecht von links nach rechts, ohne Bruch in den
  nächsten Tag. Termine sind volle Säulen, der Titel steht gedreht darin.
- **Nacht eingeklappt:** 22–6 Uhr wird zu einem schmalen, dunklen Band mit gezackten Kanten
  („Night time“). Ein Klick klappt die Nacht auf, dann kann man dort Termine anlegen. Die Zeiten
  lassen sich bei HEY nicht einstellen.
- **Freie Zeit:** lange Lücken bekommen unten ein Etikett („3 hrs“, „Freetime!“).
- **Jetzt-Linie** mit Uhrzeit oben, **„Sometime this week“** als Chips unten (Aufgaben ohne Zeit).
- **„Maybe“-Termine** in Handschrift, **Journal** je Tag, **Day Labels**, **Fotos**, **Habits**,
  **Zeiterfassung**, **Countdowns**, **Highlights**.

**Stufe 1 (einfache Variante, reine Ansicht):**

- Waagerechter Zeitstrahl über mehrere Tage, Mausrad scrollt waagerecht, Strg+Mausrad zoomt die
  Stundenbreite (Standard 56, Bereich 36–112). „Today“ springt so, dass „jetzt“ bei 30 % steht.
- Kopf: Datum klebt links (`Wed 7 Today`, heute mit rotem Kreis), Stundenachse in `MICRO`,
  darunter ein Streifen für Ganztägiges (mehrtägig als durchgehender Chip).
- Termine als volle Säulen in Kalenderfarbe (Tönung 18 %, Balken links = Beginn), Titel und Zeit
  gedreht, von unten nach oben lesbar. Überschneidungen teilen sich die **Höhe** in Bahnen.
  Unbestätigt gestrichelt, vergangen 55 %, ausgewählt mit Fokusring.
- **Nacht eingeklappt** (Standard 22–6 Uhr, Breite 48) als Band mit gezackten Kanten, Mond-Icon,
  „Night · 8 h“. Termine in der Nacht erscheinen als Punkte im Band plus „· 1 event“, damit nichts
  verschwindet. Klick auf die Nacht klappt sie auf (Animation 160 ms); „Collapse night“ klappt sie
  wieder zu; „Nights“ in der Kopfleiste schaltet alle. Klick auf freie Zeit → Platzhalter
  „New event“ (15-min-Raster, 1 h), wie im Wochenraster.
- Freie Zeit ab 2 h als „3 h free“ unten in `TEXT_3` (wie schon in der Woche).
- Neues Token **`NIGHT`** (Vorschlag: dunkel `#121626`, hell `#E3E7F1`) – ein leises Nachtblau,
  sonst geht die Nacht im dunklen Theme neben `PANEL` unter. Muss in `torro-gui` (bzw. `torro-design`).
- Größen: Breit mit Eigenschaften-Spalte (gewählter Termin), Mittel ohne Spalte (schwebende Karte),
  **Schmal senkrecht** (Zeit läuft nach unten, Nacht als waagerechtes Band, Ganztägiges im
  Tageskopf), Leiste: der bestehende Zeitstrahl der Leiste bekommt dieselbe Nacht-Darstellung.
- Technik: Iced-`Canvas` wie die Woche. Gedrehter Text geht (`Frame::rotate`), wird von Iced aber
  als Pfade ohne Hinting gezeichnet – vor Stufe 1 mit einem Spike prüfen, ob 12 px scharf genug
  ist. Plan B: Titel waagerecht oben in der Säule, gekürzt.
- **API/MCP:** keine neue Funktion – lesen über `Events`, anlegen über `SaveEvent`. Nachtzeiten
  sind anfangs fest; werden sie einstellbar, ist das ein neuer `SettingKey` (Oberfläche:
  Einstellungen, API: `SetSetting`, MCP: vorhandenes Einstellungs-Tool).

**Stufe 2 (eigene Funktionen, je mit Request, Katalogeintrag und MCP-Tool):** Tagesnotiz
(Journal, Vorschlag: als `VJOURNAL` im CalDAV-Kalender, damit es mit Nextcloud synchronisiert),
Day Labels, „This week“ mit Aufgaben (steht schon in `docs/tasks/design.md`), Countdowns.
Habits, Zeiterfassung und Fotos erst einmal nicht.

### Radar statt Tagesspirale

Die Spirale wirkt unwuchtig, weil beide Windungen unterschiedlich lang sind. Das Radar behält das
Zifferblatt, zeigt aber nie mehr als eine Umdrehung:

- **Normales 12-Stunden-Zifferblatt:** 12 oben, ein Termin um 8 steht dort, wo die 8 steht. Die
  Ziffern zeigen die echte Stunde im Fenster (um 14:20 steht an der 3 „15“, an der 8 „20“, oben
  „00“). 3, 6, 9, 12 etwas größer.
- **Fenster ab jetzt:** Die Jetzt-Linie (rot, Zeit als roter Chip außen) ist eine **klare Kante**
  und der Anfang des sichtbaren Bereichs. **Drei Viertel des Kreises (9 h)** sind voll sichtbar,
  mit Titeln entlang des Bogens. Das **letzte Viertel blendet aus**, bis es kurz vor der
  Jetzt-Linie ganz verschwunden ist – so berühren sich Ende und Anfang nie (Rückmeldung 2026-10-11,
  ersetzt das helle Drittel mit gedimmtem Rest). Zwei Varianten im Prototyp: nur ausblenden, oder
  zusätzlich zunehmend unscharf (scharf → leicht → stark unscharf, dabei ausblendend). Zum
  Vergleich auch 2/3 sichtbar.
- Technik des Ausblendens: Masken aus feinen Keilen mit fallender Deckkraft; die Unschärfe als
  zwei weichgezeichnete Kopien mit eigenen Masken. In Iced gibt es keinen Weichzeichner im
  `Canvas` – die Unschärfe müsste über einen eigenen Shader laufen, das Ausblenden geht direkt.
- **Ringe wie Baumrinde, je Kalender einer:** Reihenfolge wie in der Seitenleiste, der erste außen
  (dort ist der Bogen am längsten). Alle Ringe gleich breit; Überschneidungen teilen einen Ring in
  Bahnen. Kalender ohne Termine mit Uhrzeit bekommen keinen Ring. Bei mehr als 5 Ringen werden die
  übrigen zu „Other“ zusammengefasst (offen).
- **Mitte:** Datum, Uhrzeit groß (`DISPLAY`), laufender Termin mit „40 min left“ oder
  „Free for 1 h 20 min“, „Next 15:00 · …“, Ganztägiges als Chips.
- **Vorausschauen:** Mausrad dreht das Fenster in 15-min-Schritten weiter (Linie dann grau statt
  rot), „Back to now“ in der Mitte. Fokus-Länge 3/4/6 h (Vorschlag: Einstellung).
- **Daneben** (Breit) bzw. **darunter** (Schmal) eine Liste „Next 4 hours“ / „Later until 02:20“ –
  dieselben Termine als Text, auch für Screenreader. Mini: nur das Zifferblatt mit Mitte, ohne Titel.
- Technik: Bögen sind im `Canvas` einfach. Text entlang des Bogens muss Glyphe für Glyphe gedreht
  werden (wieder Pfade); vorher Spike, sonst Titel nur in der Liste und als Tooltip.
- **API/MCP:** reine Ansicht, keine neue Funktion. Fokus-Länge als `SettingKey`, falls einstellbar.

Offen zur Besprechung:

1. Zeitstrahl als **eigene Ansicht** „Timeline“ neben der Agenda-Liste oder **statt** der Agenda?
2. Radar als eigene Ansicht „Radar“ oder als Darstellung der Tagesansicht (Tag ▾ Raster/Radar)?
3. Ende des Radars: nur ausblenden oder zusätzlich unscharf? 3/4 oder 2/3 voll sichtbar?
4. Nachtblau als neues Token in Ordnung, oder neutral bleiben?

## Weitere Ideen aus der Recherche

Noch nicht prototypisch umgesetzt, aber als Anregung festgehalten:

| Ansatz | Beispiele |
|---|---|
| Durchgehender Zeitstrom statt getrennter Ansichten | [HEY Calendar](https://hey.com/calendar), [Studie Hund et al. (UCL)](https://discovery.ucl.ac.uk/id/eprint/1408077/) |
| Zoombare Zeitachse ohne Tag/Woche/Monat | [Fluxcal](https://www.whub.io/startups/fluxcal), [Calizo](https://calizo.sourceforge.net/about.shtml) |
| 24-Stunden-Ring | [Sectograph](https://apps.apple.com/app/id1528168188), [DayZen](https://alternativeto.net/software/dayzen/about), [Radial Week View](https://21st.dev/@ruixen.ui/components/radial-week-view.md) |
| Timeline mit sichtbaren Lücken | [Structured](https://daveswift.com/structured/), [Tiimo](https://tiimoapp.com/product/visual-planning) |
| Termine und Aufgaben in einer Ansicht | [Amie](https://screensdesign.com/showcase/amie-todos-calendar) |
| Monat als Heatmap | [Timepage by Moleskine](https://www.macstories.net/reviews/timepage-a-beautiful-and-clever-calendar-app/) |
| Jahr linear oder als Spirale | [Lineár Calendar](https://apps.apple.com/app/id6757320187), [Obsidian Linear Calendar](https://community.obsidian.md/plugins/linear-year-calendar), [Spiral Calendar](https://github.com/jimblandy/spiral-calendar) |

## Nächste Schritte (wenn das Thema wieder aufgenommen wird)

- Entscheiden, ob die Tagesspirale die Tagesansicht der GUI wird und wie sie im Terminal
  (TUI) aussehen könnte, etwa als vereinfachte Liste mit Windungsangabe.
- Prüfen, ob sich Spirale und Zoom mit der gewählten GUI-Technik (z. B. Iced-Canvas) flüssig umsetzen lassen.
- Mit echten CalDAV-Daten testen: viele Überschneidungen, mehrtägige Termine, Zeitzonen.
