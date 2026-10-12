# GitHub in der Suite: Filterlisten, Verweise, Planungslisten

Stand: 2026-10-11. **Konzept, Grundzüge am 2026-10-11 besprochen.** Was noch offen ist, steht am
Ende. Aufgaben: [tasks/github.md](tasks/github.md).

## Ziel

Issues und Pull Requests erscheinen in der Aufgaben-App wie Aufgaben, mit Link auf GitHub. Ich
wähle gezielt aus, was ich sehen will, und kann GitHub-Einträge mit eigenen Aufgaben in einer
Liste mischen, um zum Beispiel den Tag zu planen.

## Drei Bausteine

```
GitHub-Konto (Token im Schlüsselbund)
 │
 ├─ Filterliste      gespeicherte GitHub-Suche, hält sich selbst aktuell     (nur GitHub)
 │    "is:open is:issue repo:mahype/TorroWorkplace label:bug"  + angeheftete Issues
 │
 └─ Verweis          eine normale Aufgabe mit Link auf ein Issue/PR          (in jeder Liste)
      liegt in einer Planungsliste neben eigenen Aufgaben, mit eigener Reihenfolge und Datum
```

### 1. Filterliste: die Suche über den Issues

Eine Filterliste ist **eine gespeicherte GitHub-Suche**, genau die Zeile, die über jeder
Issue-Liste auf github.com steht. Die Suchanfrage ist die Wahrheit; alles andere hilft nur, sie
zu bauen.

- **Kopieren statt klicken:** Suchanfrage einfügen, oder die ganze Adresse einer Issue-Liste
  (`https://github.com/mahype/TorroWorkplace/issues?q=is%3Aopen+label%3Abug`). Der Kern nimmt
  `q=` heraus und setzt `repo:` aus dem Pfad dazu, wenn die Anfrage keins hat. Adressen von
  `/pulls`, `/issues` einer Organisation und der globalen Suche gehen genauso.
- **Klicken als Alternative:** Ein Formular mit Typ (Issues, Pull Requests, beide), Repos,
  Status, Labels (mit / ohne), Zuständig, Autor, Erwähnt, Review angefragt, Meilenstein,
  Sortierung. Es schreibt die Suchanfrage darunter, die man weiter von Hand ändern kann. Was das
  Formular nicht abbilden kann, bleibt einfach in der Anfrage stehen.
- **Vorschau:** „12 matches“ und die ersten Treffer, bevor die Liste gespeichert wird.
- **Vorlagen:** Assigned to me · Review requested · My open pull requests · Mentioned.
- **Mehrere Anfragen je Liste:** Innerhalb einer Anfrage gilt alles zusammen (UND, wie bei
  GitHub). Eine Liste darf mehrere Anfragen haben, ihre Treffer werden vereinigt (ODER), z. B.
  „Braucht mich“ = meine Issues + PRs mit Review-Anfrage.
- **Einzelne Issues anheften:** Link in die Schnelleingabe einer Filterliste einfügen → das Issue
  steht in der Liste, auch wenn es nicht zur Anfrage passt. Angeheftete lassen sich wieder lösen.
- **Bearbeiten:** Name, Anfragen und Angeheftete jederzeit änderbar. Die Liste zeigt immer genau
  das, was die Anfragen heute liefern: Wird ein Label entfernt oder ein Review abgegeben, fällt
  der Eintrag beim nächsten Abgleich heraus.

Die Definition (Name, Anfragen, Angeheftete, Farbe) liegt im Kern beim Konto, nicht auf GitHub.
GitHub kennt keine gespeicherten Suchen, die man über die API lesen könnte.

### 2. Verweis: ein Issue als Aufgabe in einer eigenen Liste

Ein **Verweis** ist eine ganz normale Aufgabe (VTODO) in einer Liste, die ich schreiben kann, also
heute einer Nextcloud-Liste. Sie trägt den Link auf das Issue (`URL`) und eine Kennung
(`X-TORRO-LINK:github:mahype/TorroWorkplace#123`).

- **Anlegen:** Link in die Schnelleingabe einer beliebigen Liste einfügen, oder in einer
  Filterliste „Add to list…“ / Ziehen auf eine Liste in der Seitenleiste.
- **Titel und Status kommen von GitHub:** Der Kern gleicht Verweise mit ab. Ändert sich der Titel
  auf GitHub, ändert er sich hier; wird das Issue geschlossen, ist der Verweis erledigt.
- **Datum, Priorität, Reihenfolge und Unteraufgaben gehören dem Verweis.** GitHub-Issues haben kein
  Fälligkeitsdatum; das Planungsdatum setze ich hier. Eigene Unteraufgaben („Tests schreiben“)
  unter einem Verweis gehen auch.
- Weil der Verweis auf Nextcloud liegt, ist er auch auf dem Handy da (Nextcloud Tasks zeigt den
  Link), ganz ohne Torro.
- **Abgeglichen wird nur in privaten Listen (G.6b):** Listen, die dem angemeldeten Konto selbst
  gehören und in die sonst niemand schreiben darf. Sonst könnte jemand, der eine Liste mit mir
  teilt (oder in eine von mir geteilte schreiben darf), dort ein `X-TORRO-LINK` auf mein privates
  Issue ablegen und den Titel lesen, den der Kern mit meinem Token hineinschreibt. Der Kern liest
  dafür beim Abgleich der Sammlungen den Besitzer (`oc:owner-principal` von Nextcloud und
  `{DAV:}owner` aus RFC 3744, beide müssen der angemeldete Benutzer sein) und die Freigaben
  (`oc:invite`: keine mit `read-write`; Freigaben nur zum Lesen sind in Ordnung, weil dann nur ich
  Verweise anlegen kann). Meldet der Server das nicht, gilt die Liste als nicht privat (fail
  closed). Verweise in anderen Listen bleiben, wie sie geschrieben wurden; nur Abhaken schließt
  weiter das Issue. Verworfen: „nur Verweise, die diese Suite angelegt hat“ – das bräuchte eine
  Signatur im VTODO mit einem Schlüssel je Gerät, Verweise vom zweiten Gerät oder aus Nextcloud
  Tasks würden nie abgeglichen, und eine kopierte Aufgabe trüge die Signatur mit.

### 3. Planungsliste: gemischt und von Hand sortiert

Eine Planungsliste ist **keine neue Art Liste**, sondern eine normale Liste mit eigener
Reihenfolge: Aufgaben und Verweise gemischt, z. B. „Heute“. Dafür braucht die Aufgaben-App etwas,
das allen Listen fehlt: **Reihenfolge von Hand** (Ziehen, Alt+↑/↓), gespeichert in
`X-APPLE-SORT-ORDER` wie bei Nextcloud Tasks, damit beide dieselbe Reihenfolge zeigen. Je Liste
wählbar: „Manual order“ oder wie bisher nach Fälligkeit gruppiert.

## Abhaken

| Eintrag | Haken | Ohne Schreibrecht |
|---|---|---|
| Issue in Filterliste | schließt das Issue („completed“), Meldung mit „Undo“ (öffnet wieder) | Haken aus, Hinweis auf das fehlende Recht |
| Verweis auf Issue | erledigt den Verweis **und** schließt das Issue, mit „Undo“ | erledigt nur den Verweis |
| PR in Filterliste | kein Haken, nur Typ-Symbol; erledigt, wenn gemergt oder geschlossen | – |
| Verweis auf PR | erledigt nur den Verweis; wird von selbst erledigt, wenn der PR gemergt oder geschlossen ist | – |

PRs werden in Torro nie gemergt oder geschlossen: Das ist keine Nebenbei-Entscheidung.

## Was aus GitHub in die Aufgaben kommt

| Was | Rolle | Abbildung |
|---|---|---|
| **Issues** | Kern | Titel, Text (Markdown) → Beschreibung, geschlossen → erledigt, **Sub-Issues → Unteraufgaben**, Labels → Kategorien, Zuständige, Meilenstein, Issue-Typ |
| **Pull Requests** | Typ neben Issue | Entwurf, Review-Stand, CI ✓/✗, Konflikt; erledigt = gemergt/geschlossen |
| **Meilensteine** | Datum | Ihr Fälligkeitsdatum gilt für Issues darin, solange der Verweis kein eigenes hat |
| GitHub Projects | später | Tafeln/Tabellen über Issues mehrerer Repos mit eigenen Feldern (Status, Priorität, Datum, Sprint). Wird erst wichtig, wenn jemand sie nutzt; dann als Datums- und Filterquelle |
| Benachrichtigungen | nicht hier | gehören ins Benachrichtigungscenter ([eingang.md](eingang.md)) |
| Dependabot, Discussions, Releases, Actions | nein | höchstens das CI-Ergebnis am PR |

## Anzeige in der Aufgaben-App

- **Seitenleiste:** Gruppe je Konto, sobald es mehr als eins gibt; Filterlisten mit eigenem Symbol
  in der Gruppe „GitHub“. „New filter list…“ am Ende der Gruppe.
- **Zeile:** Typ-Symbol (Issue offen/geschlossen, PR Entwurf/offen/gemergt), Titel,
  `TorroWorkplace#123`, Labels als dezente Chips, Fälligkeit; bei PRs Review-Stand und CI.
  Verweise zeigen dasselbe, nur mit Haken.
- **Seitenpaneel:** Text als Markdown (nur lesen), Labels, Zuständige, Meilenstein, verknüpfte PRs,
  Hauptknopf „Open on GitHub“. Bei Verweisen darunter die eigenen Felder (Datum, Priorität, Notiz).
- **Gruppierung in Filterlisten:** nach Repo (Standard) oder ohne; Reihenfolge wie die Sortierung
  der Anfrage (`sort:updated-desc` usw.).
- **Smarte Listen:** Heute/Geplant zeigen alles mit Datum, Verweise eingeschlossen. Einträge aus
  Filterlisten haben nur über den Meilenstein ein Datum; je Filterliste abschaltbar.

## Kalender

Der Kalender zeigt heute **gar keine Aufgaben**, auch keine aus Nextcloud. Das kommt allgemein:
fällige Aufgaben als ganztägige Einträge mit Haken-Symbol, je Liste ein- und ausschaltbar,
Klick öffnet das Seitenpaneel. GitHub nutzt das mit: **Was in einer meiner Listen steht und ein
Datum hat, steht im Kalender**, also vor allem Verweise mit Planungsdatum. Später optional:
Meilensteine als ganztägige Termine je Filterliste.

## Technik

- **Voraussetzung: mehrere Konten (M1).** Heute nimmt der Kern das erste Nextcloud-Konto, und die
  Kennungen der Sammlungen sind nackte CalDAV-URLs. M1 schlüsselt den PIM-Dienst nach Konto und
  stellt das Konto vor jede Kennung (`<account-id>/<url>`); Einstellungen und Agenten-Rechte, die
  alte Kennungen enthalten, werden beim Laden einmal umgeschrieben.
- **Anmeldung** wie in [settings.md](settings.md): Token der GitHub CLI übernehmen (`gh auth token`)
  oder Token einfügen. Für den Haken braucht das Token Schreibrecht auf Issues (Classic `repo`,
  Fine-grained „Issues: Read and write“, „Pull requests: Read“). Die Kontoseite zeigt, was fehlt.
- **Abfrage über GraphQL** (`search(type: ISSUE)`, `nodes` für Angeheftete und Verweise), weil
  PR-Angaben und Sub-Issues dort in einem Aufruf kommen. Jede Filterliste wird **ganz** neu
  abgefragt (alle 5 Minuten, beim Öffnen, „Sync now“): Eine Abfrage nur nach Änderungen merkt
  nicht, wenn ein Issue nicht mehr zum Filter passt. Grenzen: 5 000 Punkte/Stunde, je Suche
  höchstens 1 000 Treffer; die Liste zeigt dann „Showing the first 1,000“.
- **Kennungen:** Sammlung `github:<login>/<list-id>`, Eintrag `github:<owner>/<repo>#<nummer>`.
  Filterlisten sind `Collection`s mit `writable: false` (keine neuen Einträge, kein Löschen);
  Abhaken geht trotzdem, über `tasks.save_todo` mit `completed`.
- **Modell (Schnittstellen-Änderung, siehe offene Punkte):** `Todo` bekommt `url`,
  `categories` (beide aus RFC 5545), `sort_order` und `link: Option<Link>` mit den GitHub-Angaben
  (Typ, Repo, Nummer, Status, Labels mit Farbe, Review/CI). `Collection` bekommt
  `source: Option<…>` für die Definition einer Filterliste.
- **CalDAV muss fremde Eigenschaften behalten.** Heute schreibt der Kern eine Aufgabe beim
  Speichern komplett neu und verwirft alles, was das Modell nicht kennt (`X-APPLE-SORT-ORDER`,
  `CATEGORIES`, `URL`, Erinnerungen von Nextcloud Tasks …). Das ist auch ohne GitHub ein
  Datenverlust und muss vor Verweisen und Reihenfolge behoben werden.

## Oberfläche, API, MCP

| Funktion | Oberfläche | API (Zugriff) | MCP |
|---|---|---|---|
| GitHub-Konto verbinden (`gh`-Anmeldung übernehmen) | Steuerzentrale › Add account › GitHub | `accounts.connect_github` (Admin): der Dienst ruft selbst `gh auth token` auf | `accounts_connect_github`: öffnet den Assistenten beim Menschen und antwortet „opened“ |
| Token einfügen | Assistent, Feld „Token“ | `accounts.set_github_token` (Admin, **nur App-Schlüssel**) | **Never**: Geheimnis (Ausnahme 1 in api.md) |
| Filterliste anlegen | Aufgaben › „New filter list…“ | `tasks.create_filter_list` (Write, Bereich `github`) | `tasks_create_filter_list` |
| Filterliste ändern | Aufgaben › Liste › „Edit filter…“ | `tasks.update_filter_list` (Write, Bereich `github`, mit Versionsstempel) | `tasks_update_filter_list` |
| Filterliste löschen | Aufgaben › Liste › „Delete list…“ | `tasks.delete_filter_list` (Delete, Bereich `github`) | `tasks_delete_filter_list` |
| Anfrage ausprobieren (Vorschau) | Dialog der Filterliste | `tasks.preview_filter` (Read, Bereich `github`) | `tasks_preview_filter` |
| Issue anheften / Verweis anlegen | Link in die Schnelleingabe | `tasks.add_link` (Write) | `tasks_add_link` |
| Angeheftetes Issue lösen | Detail › „Unpin“ | `tasks.unpin_issue` (Write, Bereich `github`) | `tasks_unpin_issue` |
| Einträge lesen | Liste | `tasks.list_todos` (gibt es) | `tasks_list_todos` |
| Abhaken (Issue schließen) | Haken | `tasks.save_todo` (gibt es) | `tasks_save_todo` |
| Reihenfolge ändern | Ziehen, Alt+↑/↓ | `tasks.move_todo` (Write) | `tasks_move_todo` |
| Sortierart je Liste | Liste › „Sort“ | `settings.set_setting` (gibt es, Admin) | `settings_set_setting` |

## Rechte für Agenten

Das GitHub-Token reicht viel weiter als eine Aufgabenliste: Es sieht alle Repos des Menschen, auch
private. Die Auswahl der Aufgabenlisten (`task_lists`) allein schützt deshalb nicht. Ein Agent
könnte sonst mit der Vorschau beliebige Suchen ausführen, die Anfrage einer freigegebenen Liste
auf „alles“ umschreiben, fremde Issues anheften oder über einen Verweis in seiner Nextcloud-Liste
fremde Issues lesen und schließen.

Darum bekommt GitHub einen **eigenen Bereich `github`** mit Stufe (none / read / write / full) und
einer **Auswahl der Repos und Organisationen** (`github_repos: Selection`, neben `calendars` und
`task_lists` in `Rights`). Der Dienst prüft bei jedem Client:

- **Vor dem Fragen prüfen (seit G.2b):** Bei einer Auswahl „nur diese Repos“ muss jede Suche, die ein
  Client schickt (Vorschau, Anlegen, Ändern), mindestens ein `repo:`/`org:`/`user:` aus seiner Auswahl
  haben und kein positives außerhalb; `OR`, `NOT`, Klammern und Unklares werden abgelehnt. Sonst
  `forbidden`, ohne dass GitHub gefragt wird. GitHub sucht mit dem vollen Token: Erst danach zu
  filtern ließe Fehlercodes, Trefferzahlen und Antwortzeit Namen privater Repos verraten.
- **Jeder Treffer** wird zusätzlich gegen die Repo-Auswahl gefiltert, bevor er gelesen wird (zweite
  Sicherung). Eine Filterliste, deren Suchen nicht ganz in der Auswahl liegen, gibt es für den Client
  nicht. GitHub-Fehlermeldungen erreichen Clients nur als feste Meldung.
- `tasks.add_link`, Anheften und Abhaken (Issue schließen) nur bei Repos in der Auswahl; sonst
  `not-found`.
- **Verweise setzen braucht GitHub-Lesen (G.6b):** Ein Client ohne Leserecht auf `github` darf
  keinen Verweis anlegen oder ändern, auch nicht über `tasks.save_todo` und auch nicht mit der
  Auswahl „alle Repos“: Der Abgleich schriebe sonst Titel und Zustand aus GitHub in die Aufgabe, die
  der Client liest. Antwort `forbidden`, bevor jemand um Freigabe gebeten wird. Einen Verweis, den
  der Mensch angelegt hat, darf er weiter abhaken (ohne Schreibrecht nur die Aufgabe).
- **Freigabetexte nennen, was läuft (G.6b):** `tasks.add_link` „Add a link to owner/repo#n to
  "Liste"“ bzw. „Pin owner/repo#n to filter list "Liste"“ (Liste aus dem Cache, Kennung aus der
  Adresse); eine unlesbare Adresse wird vorher mit `error` abgelehnt. Abhaken eines Verweises heißt
  nur dann „Close GitHub issue …“, wenn sich sonst nichts ändert; sonst „Change task … and close
  GitHub issue owner/repo#n“.
- Neue Filterlisten kommen nie von selbst in eine Auswahl „nur diese“; Listen-Kennungen werden nie
  wiederverwendet, damit ein alter Eintrag in `clients.json` keine neue Liste freischaltet.
- Die Reichweitenprüfung im Kern (`out_of_reach`) behandelt jeden Request ausdrücklich, ohne
  `_`-Zweig, damit ein neuer Request nicht ungeprüft durchläuft.
- Das Protokoll schreibt nur Kennungen (Listen-Kennung, `owner/repo#123`), nie Suchanfragen oder
  Titel.
- Das Token geht über keinen Weg hinaus: Der Typ, der es im Request trägt, schwärzt sich in `Debug`;
  es steht nie in Freigaben, Protokoll oder Fehlermeldungen.

## Offene Punkte

1. **Schnittstellen-Änderungen freigeben:** `PimSource::account()` → Liste der Konten (M1),
   `Todo` um `url`, `categories`, `sort_order`, `link`, `Collection` um `source`, `Rights` um
   `github_repos`, `Area` um `Github`. Die neuen Requests stehen in der Tabelle oben.
2. **Planungslisten ohne Nextcloud:** Verweise brauchen eine Liste, in die man schreiben kann. Wer
   nur GitHub hat, hätte keine. Später ein Konto „On this device“ mit lokalen Listen?
3. **Filterlisten auf mehreren Geräten:** Die Definitionen liegen lokal im Kern. Später über
   Nextcloud (WebDAV-Datei) abgleichen?
4. **Abhaken eines Verweises schließt das Issue.** Passt das für die Tagesplanung, oder heißt
   „heute erledigt“ manchmal nur „heute dran gearbeitet“? Dann bräuchte es eine zweite Aktion
   („Done for today“ ohne das Issue zu schließen). Dasselbe gilt für Agenten: Sie schließen
   Issues mit „write“ ohne Freigabe (rückgängig zu machen, aber für andere sichtbar). Soll das
   Schließen auf GitHub eine Freigabe brauchen?
5. **Auswahl aus GitHub im Formular** (Repos, Labels, Personen als Liste zum Anklicken) braucht
   einen Lese-Request im Bereich `github`. Vorerst nur Textfelder.
6. **Clients ohne GitHub-Lesen sehen Titel und Zustand von Verweisen, die der Mensch angelegt hat**
   (G.6b): Der Titel steht in der eigenen Aufgabe, die der Client lesen darf. Das ist gewollt – es
   sind die Aufgaben des Menschen; nur die Details (`link.github`) bekommt er nicht. Dasselbe gilt
   für Menschen, mit denen eine Liste nur zum Lesen geteilt ist.
7. **Mehrere GitHub-Konten:** Verweise in Aufgabenlisten liest und schließt der Kern mit dem ersten
   Konto, das ein Token hat. Später das Konto im Verweis mitführen.
8. **Verweise in geteilten Listen** werden nicht abgeglichen (G.6b) und behalten den Titel vom
   Anlegen. Abhaken schließt dort trotzdem das Issue, wenn der Mensch es abhakt – ein Verweis, den
   jemand anderes dort abgelegt hat, könnte so ein Issue schließen lassen, das der Mensch nicht
   meinte. Abhaken dort auf die Aufgabe beschränken?
