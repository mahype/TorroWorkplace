# Aufschieben zählen und auswerten

Konzept zu Issue #623. Wer Aufgaben immer wieder verschiebt, soll das auf Wunsch sehen können: wie oft eine
Aufgabe verschoben wurde und welche Arten von Aufgaben man am häufigsten aufschiebt. Ziel ist, eigene Muster zu
erkennen, nicht zu tadeln. Das Verschieben selbst („To tomorrow“, Ziehen auf einen Tag, Fokus-Plan auf morgen)
gehört zu #596 und steht schon; dieses Konzept zählt nur mit.

## Entscheidungen (Vorschläge zu den offenen Fragen)

Die vier offenen Fragen des Issues mit dem Vorschlag, nach dem der Kern gebaut ist. Jede lässt sich im Review
anders entscheiden; was sich dann ändert, steht jeweils dabei.

### 1. Wo liegen die Daten? Nur lokal im Dienst

Der Verlauf liegt nur auf diesem Rechner, im Zustandsverzeichnis des Dienstes:
`<state>/torroworkplace/postponements.json`, Datei 0600, Ordner nur für den Besitzer. Er geht nicht in die
Aufgabe (keine iCalendar-Eigenschaft) und nicht zum Server.

Warum:

- **Geteilte Listen.** Aufgabenlisten in Nextcloud werden geteilt. Ein Zähler als `X-TORRO-…` an der Aufgabe
  sähen alle, mit denen die Liste geteilt ist, und das widerspricht „nicht tadeln“.
- **Verhaltensdaten.** Wann jemand was aufschiebt, ist persönlicher als die Aufgabe selbst. Was den Rechner
  nicht verlässt, kann auch nicht anderswo ausgewertet werden.
- **Die Auswertung braucht Ereignisse**, keine Summe: Tag, Wochentag, Priorität und Fokus zum Zeitpunkt des
  Verschiebens. Eine Eigenschaft an der Aufgabe trüge nur eine Zahl.

Nachteil: Jeder Rechner zählt für sich. Wer an zwei Rechnern verschiebt, sieht an jedem nur einen Teil. Das ist
für eine Selbstbeobachtung hinnehmbar; ein späteres, ausdrücklich eingeschaltetes Synchronisieren (z. B. als
Datei in Nextcloud, nicht an der Aufgabe) bleibt möglich und ist nicht Teil von #623.

Jedes Ereignis hält fest: UID der Aufgabe, Liste, alter und neuer Fälligkeitstag, Zeitpunkt, Priorität und
Fokus-Stufe zu diesem Zeitpunkt. **Kein Titel, keine Beschreibung**: Titel holt die Auswertung beim Lesen aus
dem Cache des Kerns. Gezählt wird nach UID, nicht nach `href`, damit eine Aufgabe, die in eine andere Liste
wandert, ihren Zähler behält.

### 2. Was zählt als Aufschieben? Jede Verschiebung auf einen späteren Tag über Torro

Gezählt wird, wenn eine **offene** Aufgabe mit Fälligkeitsdatum **über den Dienst** einen **späteren
Fälligkeitstag** bekommt, gleich auf welchem Weg:

| Weg | zählt |
| --- | --- |
| `tasks.plan_todo` auf einen späteren Tag („To tomorrow“, Ziehen auf „Tomorrow“/„Later“, Alt+↓) | ja, einmal |
| `tasks.postpone_focus_plan` | ja, jede verschobene Aufgabe einmal |
| `tasks.save_todo` mit späterem Fälligkeitstag (Detail, Agent, Skript) | ja, einmal |
| überfällige Aufgabe auf heute | ja: der Tag, an dem sie fällig war, ist vorbei |
| andere Uhrzeit am selben Tag, Reihenfolge innerhalb des Tages | nein |
| früherer Tag | nein |
| erstes Datum für eine Aufgabe ohne Datum | nein |
| Datum entfernt | nein (offene Frage, siehe unten) |
| erledigte Aufgabe, Teilaufgabe | erledigte nein; Teilaufgaben ja, wenn sie über `save_todo` später fällig werden |
| Änderung in anderen Apps (Nextcloud Web, Handy), die per Sync ankommt | nein |

Warum nicht nur „To tomorrow“: Ein Muster entsteht gerade dort, wo man eine Aufgabe in der Detailansicht auf
nächste Woche legt. Warum nicht der Sync: Der Kern sieht dann nur, dass sich ein Datum geändert hat, nicht, ob
die Person selbst verschoben hat oder jemand anderes in einer geteilten Liste.

Gezählt wird nur, was wirklich geschrieben wurde (Antwort des Servers). Bricht ein Befehl mit mehreren
Schreibvorgängen mittendrin ab, zählen die schon geschriebenen Aufgaben nicht mit; das ist selten und wird beim
erneuten Senden ohnehin gezählt.

### 3. Ein- und Ausschalten, Löschen, Aufbewahrung

- **Standard: aus.** Solange die Einstellung `postpone_stats` aus ist, wird **nichts** aufgezeichnet, und die
  Auswertung meldet `enabled: false` ohne Daten. Das Issue verlangt die Auswertung nur auf Wunsch; Verhalten
  aufzuzeichnen, das niemand sehen will, wäre Datensammeln auf Vorrat.
- **Einschalten nur durch einen Menschen** (App-Schlüssel, wie das Präsenz-Signal): Ein Agent soll nicht
  beschließen, dass das Verhalten der Person ab jetzt aufgezeichnet wird. **Ausschalten** darf jeder Client mit
  dem Recht, Einstellungen zu ändern.
- **Ausschalten löscht nicht.** Die Oberfläche fragt beim Ausschalten, ob der Verlauf gleich mitgelöscht werden
  soll. Wieder eingeschaltet, zählt der alte Verlauf weiter, sofern er nicht abgelaufen ist.
- **Löschen:** `tasks.clear_postponements` löscht den Verlauf, ein Client nur den der Listen, die er sehen darf.
- **Aufbewahrung: 365 Tage.** Ältere Ereignisse fallen beim Schreiben und Lesen weg. Ein Jahr reicht für Muster
  über Wochentage und Jahreszeiten, ohne ein Leben lang Buch zu führen.
- Ereignisse gelöschter Aufgaben bleiben bis zum Ablauf, damit „nach Liste“ und „nach Priorität“ nicht
  schöner aussehen, wenn man aufgeschobene Aufgaben einfach löscht. Sie erscheinen ohne Titel.

### 4. Darf ein Agent die Auswertung lesen? Ja, im Rahmen seiner Rechte

Nach „Drei Wege“ (`docs/api.md`) gibt es jede Funktion auch über MCP; die Liste „Was nie nach außen geht“ nennt
nur Geheimnisse, Rechte, Freigaben und das Beenden des Dienstes. Die Auswertung ist dort nicht zu begründen.
Gesteuert wird stattdessen über das, was schon da ist:

- `tasks_postpone_stats` braucht **Lesen** im Bereich Aufgaben und zeigt nur Ereignisse der Listen, die der
  Client sehen darf. Wer einem Agenten die Aufgaben nicht gibt, gibt ihm auch die Auswertung nicht.
- Solange die Einstellung aus ist, sieht auch ein Agent nichts, und nur ein Mensch schaltet sie ein.
- `tasks_clear_postponements` braucht **Löschen** (volle Rechte an den Aufgaben).

Der Nutzen ist der im Issue genannte: Ein Agent, der beim Planen hilft, kann sagen „Diese Aufgabe hast du schon
sechsmal verschoben; aufteilen?“.

## Drei Wege

| Funktion | Oberfläche | API | MCP |
| --- | --- | --- | --- |
| Auswertung und Zähler lesen | Zähler an der Aufgabe, Ansicht „Postponed“ | `tasks.postpone_stats` (Read) | `tasks_postpone_stats` |
| Verlauf löschen | Knopf in der Ansicht und beim Ausschalten | `tasks.clear_postponements` (Delete) | `tasks_clear_postponements` |
| Ein- und ausschalten | Schalter in den Einstellungen der Aufgaben | `settings.set_setting` `postpone_stats` (Admin; ein nur durch Menschen) | `settings_set_setting` |
| Verschieben (zählt mit) | wie #596 | `tasks.plan_todo`, `tasks.postpone_focus_plan`, `tasks.save_todo` | wie #596 |

Ein eigener Befehl „Verlauf lesen“ (einzelne Ereignisse) ist nicht vorgesehen: Die Auswertung trägt je Aufgabe
Zahl und letzten Zeitpunkt, das reicht für Zähler und Ansicht. Kommt eine Zeitleiste dazu, wird es ein eigener
Read-Befehl.

### `tasks.postpone_stats`

Eingabe: optional `since` (Tag `YYYY-MM-DD`), dann nur Ereignisse ab diesem Tag. Antwort `postpone-stats`:

```json
{
  "enabled": true,
  "since": "2026-02-03",
  "total": 14,
  "tasks": [{"uid": "…", "href": "/…/a.ics", "collection": "…", "title": "Angebot Schmidt", "count": 6,
             "last": "2026-11-20T17:02:11Z", "done": false}],
  "by_priority": [{"key": "high", "count": 8}],
  "by_focus": [{"key": "strong", "count": 6}],
  "by_list": [{"key": "<list id>", "count": 9}],
  "by_weekday": [{"key": "mon", "count": 5}]
}
```

- `since` in der Antwort: der Tag des ältesten gezählten Ereignisses (fehlt ohne Ereignisse).
- `tasks`: am häufigsten verschoben zuerst, bei Gleichstand das jüngere; `href`, `title` und `done` aus dem
  Cache, leer, wenn die Aufgabe nicht mehr da ist.
- Priorität in den fünf Klassen der App (`urgent`, `high`, `medium`, `low`, `none`), Fokus `normal`, `focus`,
  `strong`, jeweils zum Zeitpunkt des Verschiebens. Wochentag ist der Tag, **an dem die Aufgabe fällig war** –
  der Tag, an dem sie liegen blieb.
- Gruppen nur mit Ereignissen, nach Zahl absteigend.

## Oberfläche (beschrieben, nicht entworfen)

Das Design entsteht in einem eigenen Schritt mit `torro-design`; hier steht nur, was die Oberfläche können muss.

- **Zähler an der Aufgabe** (torrotasks): nur bei eingeschalteter Auswertung, ab zwei Verschiebungen, klein und
  neutral neben dem Datum (z. B. „↷ 3“), keine Warnfarbe. In der Detailansicht ein Satz „Postponed 3 times,
  last on Fri 20 Nov“.
- **Ansicht „Postponed“** im Menü „View“ von torrotasks, nur bei eingeschalteter Auswertung: oben die am häufigsten
  verschobenen Aufgaben (Klick öffnet die Aufgabe), darunter die Verteilung nach Priorität, Fokus, Liste und
  Wochentag als einfache Balken, ein Zeitraum (30 Tage, 90 Tage, alles) und „Clear history“.
- **Einstellung** in den Einstellungen der Aufgaben: Schalter „Count postponed tasks“ mit einem Satz, dass die
  Daten nur auf diesem Rechner liegen und nach einem Jahr verfallen. Beim Ausschalten die Frage, ob der Verlauf
  gelöscht werden soll.
- Ton: beschreibend, nie wertend („You postponed this 3 times“, nicht „You keep putting this off“).

## Umsetzung im Kern (#623)

- `torro-ipc`: Requests `tasks.postpone_stats` und `tasks.clear_postponements`, Antwort `postpone-stats`,
  Einstellung `postpone_stats`, Katalogeinträge; Einschalten nur durch Menschen (`kept_for_people`).
- `torro-core`: Feld `postpone_stats` in den Einstellungen (Standard aus).
- `torroworkplace`: Modul `postponements` (Datei, Aufbewahrung, Erkennen, Auswertung); der Kern zeichnet nach
  `tasks.plan_todo`, `tasks.postpone_focus_plan` und `tasks.save_todo` auf und beantwortet die beiden Befehle;
  MCP über den Katalog.
- Oberfläche (Zähler, Ansicht, Schalter) folgt in einem eigenen Issue nach dem Design.

## Offene Fragen an das Review

1. Soll **Datum entfernen** als Aufschieben zählen? Vorschlag: nein, das ist eher „irgendwann“ als „später“.
2. Soll die Aufzeichnung **immer laufen** und nur die Anzeige eingeschaltet werden? Dann hätte man beim
   Einschalten schon Daten; der Vorschlag ist aus Datensparsamkeit „aus heißt aus“.
3. Sollen Agenten die Auswertung nur mit einem **eigenen Schalter** lesen dürfen (z. B. `postpone_stats_agents`)?
   Vorschlag: nein, Rechte an den Aufgaben und der Schalter genügen.
4. Ist **ein Jahr** Aufbewahrung richtig, oder soll sie einstellbar sein?
