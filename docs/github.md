# GitHub in der Suite: Filterlisten, Verweise, Planungslisten

Stand: 2026-10-14. **Konzept, Grundzüge am 2026-10-11 besprochen, „Issues anlegen“ am 2026-10-14.** Was noch offen ist, steht am
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
  closed). Verweise in anderen Listen bleiben, wie sie geschrieben wurden, und Abhaken erledigt
  dort nur die Aufgabe (G.6c, siehe „Abhaken“). Verworfen: „nur Verweise, die diese Suite angelegt hat“ – das bräuchte eine
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
| Verweis auf Issue (private Liste) | erledigt den Verweis **und** schließt das Issue, mit „Undo“ | erledigt nur den Verweis |
| Verweis auf Issue (geteilte Liste) | erledigt nur den Verweis, das Issue bleibt offen (G.6c) | erledigt nur den Verweis |
| PR in Filterliste | kein Haken, nur Typ-Symbol; erledigt, wenn gemergt oder geschlossen | – |
| Verweis auf PR | erledigt nur den Verweis; wird von selbst erledigt, wenn der PR gemergt oder geschlossen ist | – |

PRs werden in Torro nie gemergt oder geschlossen: Das ist keine Nebenbei-Entscheidung.

**Geteilte Listen (G.6c, entschieden 2026-10-12, sichere Voreinstellung):** In Listen, die nicht
privat sind (siehe „Verweis“: nicht mein Konto, oder jemand anderes darf hineinschreiben), erledigt
der Haken an einem Verweis nur die Aufgabe – für die Suite wie für Clients. Sonst könnte jemand,
mit dem die Liste geteilt ist, dort einen Verweis ablegen, und mein Haken schlösse ein Issue, das
ich nicht meinte. Die Freigabe sagt dann „Change task …“, nie „close“; die App zeigt „Issue closed ·
Undo“ nur, wenn die Antwort des Kerns das Issue wirklich geschlossen meldet (`link.github`).

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
| Issue anlegen | Dialog „New issue“ (aus Mail, Eingang, Aufgaben, Kalender, „Report a problem“); im Chat die Issue-Entwurfs-Karte | `tasks.create_issue` (Write, Bereich `github`, nach außen) | `tasks_create_issue` |
| Ähnliche offene Issues finden | Dialog und Karte: „Possibly the same as …“ | `tasks.find_similar_issues` (Read, Bereich `github`) | `tasks_find_similar_issues` |
| Repository vorschlagen | Dialog und Karte: Repo vorausgewählt | `tasks.suggest_issue_repo` (Read, Bereich `github`) | `tasks_suggest_issue_repo` |
| Kommentar statt neuem Issue | „Add comment there instead“ | `tasks.comment_on_issue` (Write, Bereich `github`, nach außen) | `tasks_comment_on_issue` |
| Angaben für „Report a problem“ sammeln | Dialog „Report a problem“ | `system.collect_diagnostics` (Read; mit Log-Auszug nur App-Schlüssel) | `system_collect_diagnostics` (ohne Log-Auszug) |
| Absender mit Link antworten | „Reply with link“ | `mail.save_draft` (gibt es) | `mail_save_draft` |

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
  GitHub issue owner/repo#n“. In einer geteilten Liste nennt die Freigabe kein Issue (G.6c), weil
  dort nichts geschlossen wird.
- Neue Filterlisten kommen nie von selbst in eine Auswahl „nur diese“; Listen-Kennungen werden nie
  wiederverwendet, damit ein alter Eintrag in `clients.json` keine neue Liste freischaltet.
- Die Reichweitenprüfung im Kern (`out_of_reach`) behandelt jeden Request ausdrücklich, ohne
  `_`-Zweig, damit ein neuer Request nicht ungeprüft durchläuft.
- Das Protokoll schreibt nur Kennungen (Listen-Kennung, `owner/repo#123`), nie Suchanfragen oder
  Titel.
- Das Token geht über keinen Weg hinaus: Der Typ, der es im Request trägt, schwärzt sich in `Debug`;
  es steht nie in Freigaben, Protokoll oder Fehlermeldungen.

## Issues anlegen

Stand: 2026-10-14, Entscheidungen am selben Tag (unten). Bisher holt die Suite Issues nur herein
(Filterlisten, Verweise). Neu: **Issues anlegen**, aus allen Bereichen, in denen etwas passiert, das
eigentlich ein Issue ist: eine Fehlermeldung per Mail, ein Alarm im Eingang, eine Aufgabe, die
eigentlich ins Repo gehört, Aktionspunkte aus einer Besprechung, ein Fehler in Torro selbst.

### Ein Befehl, viele Einstiege

Alles läuft über **einen** Befehl `tasks.create_issue`; die Einstiege unterscheiden sich nur darin,
was sie vorausfüllen und welche Quelle sie mitgeben.

| Einstieg | Wo | Vorausgefüllt | Danach |
|---|---|---|---|
| **Mail** | Mail-Fenster und Leser im Eingang: „Create issue…“ neben „Create task…“ | Betreff als Titel, Text als Zitat, am Ende eine Zeile mit dem Weg zurück zur Mail | Karte bietet „Reply with link“: normaler Mail-Entwurf an den Absender mit dem Link zum Issue |
| **Eingang** | jeder Eintrag (ntfy-Alarm, Nextcloud, Talk, GitHub-Check …): „Create issue…“ | Titel und Text des Eintrags, Link der Quelle | Eintrag wird erledigt, wie bei `create_task_from_item` |
| **Aufgaben** | Detail › „…“ › „Move to GitHub…“ | Titel, Beschreibung, Unteraufgaben als Checkliste | Die Aufgabe wird **immer** zum Verweis auf das neue Issue (nichts doppelt) |
| **Kalender** | Termin › „…“ › „Create issue…“ | Titel des Termins, Beschreibung, Datum | später auch je Aktionspunkt aus einem Besprechungsprotokoll |
| **Chat** | „Mach daraus ein Issue“ | was der Assistent aus der Unterhaltung oder einem gefundenen Eintrag macht | Issue-Entwurfs-Karte, siehe [assistent.md](assistent.md) |
| **Report a problem** | jede Torro-App, Menü des App-Umschalters | Version, System; Log-Auszug nur angehakt | Ziel ist fest `mahype/TorroWorkplace` |

Der **Dialog „New issue“** ist ein gemeinsamer Baustein aller Apps (`torro-gui`): Repo (vorgeschlagen),
Titel, Text (Markdown mit Vorschau), Labels, Zuständige und Meilenstein als Chips aus
`tasks.describe_github_repo` (G.9), darunter „Possibly the same as …“ und der Schalter **„Also add to
tasks“** (Standard aus; Liste wählbar). Primärknopf „Create issue“.

### Was der Kern dabei tut

- **Weg zurück:** Mit der Quelle (`from`: Mail, Eintrag im Eingang, Aufgabe, Termin, Unterhaltung)
  hängt der Kern eine Zeile an, die zur Quelle zurückführt (wie bei `create_task_from_item`), und
  merkt an der Quelle „Issue owner/repo#142 created“. Der Kern prüft, dass der Schlüssel die Quelle
  lesen darf; sonst `not-found`, damit niemand eine fremde Quelle vorschiebt.
- **Text aus Mail und Eingang bleibt Daten.** Er landet als Zitat im Issue, nie als Anweisung.
- **Öffentliches Repo, privater Text:** Ist das Ziel öffentlich und die Quelle eine Mail, eine
  Nachricht aus Talk oder ein Eintrag aus dem Eingang, sagt der Dialog bzw. die Karte deutlich „This
  repository is public. Everyone can read the issue.“ Das ist der häufigste Weg, wie Privates
  versehentlich öffentlich wird.
- **Ähnliche Issues:** Vor dem Anlegen sucht der Kern in den offenen Issues des Repos nach ähnlichen
  Titeln (`tasks.find_similar_issues`, GitHub-Suche `in:title`, höchstens 5). Gibt es welche, zeigt
  der Dialog sie mit Link und „Add comment there instead“ (`tasks.comment_on_issue`). Anlegen bleibt
  möglich.
- **Repo merken je Quelle:** Nach jedem Anlegen merkt sich der Kern das Repo für den Schlüssel der
  Quelle: Absenderadresse einer Mail, Quelle im Eingang, Aufgabenliste, Kalender.
  `tasks.suggest_issue_repo` schlägt dieses vor, sonst das zuletzt benutzte. Gespeichert in
  `<state>/github/issue-repos.json`, nur Schlüssel und `owner/repo`, keine Inhalte.
- **„Also add to tasks“:** legt nach dem Anlegen einen Verweis in der gewählten Liste an (wie
  `tasks.add_link`); braucht dafür Schreibrecht auf die Liste.
- **Grenzen:** Titel höchstens 256 Zeichen, Text höchstens 65 536 Zeichen (Grenze von GitHub),
  höchstens 20 Labels und 10 Zuständige; sonst `error`, bevor jemand gefragt wird.
- **Offen gebaut:** Der Request nennt `account` und `repo`, nicht „GitHub“. Was ein Konto anlegt,
  entscheidet der Kern nach der Art des Kontos; GitLab, Forgejo oder Nextcloud Deck können später als
  weitere Art dazukommen, ohne dass sich der Request ändert.

### Rechte

- `tasks.create_issue` und `tasks.comment_on_issue` brauchen `Write` im Bereich `github`, das Repo in
  `github_repos` (sonst `not-found`) **und** den Schalter **„Nach außen“** wie `mail.send_draft`
  (`Command::is_outbound`): Ein Issue ist für andere sichtbar und lässt sich nicht löschen, nur
  schließen. Voreinstellung „Mit Freigabe“: Assistent und Agenten legen Issues nur nach Freigabe an.
- **Öffentliches Repo: immer Freigabe.** Ist das Ziel öffentlich, wartet jeder Client auf Freigabe, auch
  mit „Nach außen: frei“ (wie „Bei jeder Änderung“). Ob ein Repo öffentlich ist, liest der Kern aus seinem
  Zwischenspeicher der Repos, nie aus dem Request. Sonst könnte ein Agent Mailtext in `body` kopieren, `from`
  weglassen und so den Hinweis „public“ umgehen.
- Die Freigabe nennt, was läuft: „Create issue in owner/repo: “Titel”“, bei einem öffentlichen Repo
  „Create public issue …“. In den Details stehen die ersten 200 Zeichen des Textes, Labels, Zuständige
  und der Meilenstein (Name aus dem Zwischenspeicher), wie beim Senden einer Mail; dasselbe für
  Kommentare. Titel und Text stammen aus dem Request; sie sind das, was angelegt wird, und werden mit
  der Freigabe festgehalten. Ändert der Mensch etwas, ist es ein neuer Request; das Protokoll verknüpft
  dann beide Freigaben, damit erkennbar bleibt, dass der Anstoß vom Assistenten kam.
- **`tasks.find_similar_issues` und `tasks.suggest_issue_repo`:** Repos außerhalb `github_repos` →
  `not-found`, ohne GitHub zu fragen; der Vorschlag braucht Leserecht auf die Quelle und nennt auch beim
  Rückgriff auf „zuletzt benutzt“ nur Repos aus der Auswahl. Suchbegriffe kommen nie ins Protokoll. Die
  Schlüssel in `issue-repos.json` (Absenderadressen) liegen nur als SHA-256 auf der Platte.
- **`from: conversation`** dürfen nur der Assistent (für seine eigenen Unterhaltungen) und der
  App-Schlüssel setzen; für alle anderen `not-found`.
- Mit Quelle zusätzlich Leserecht auf die Quelle; für „Move to GitHub“ Schreibrecht auf die Liste der
  Aufgabe, für den Eingang Schreibrecht auf `inbox` (Eintrag erledigen).
- Das Token braucht „Issues: Read and write“ (wie schon fürs Schließen).
- **„Report a problem“ mit Log-Auszug** nur mit dem App-Schlüssel (`Request::kept_for_people`, wie das
  Mikrofon): Logs können Namen von Konten, Ordnern und Fehlertexte von Servern enthalten. Agenten
  bekommen über `system.collect_diagnostics` nur Version und System; Zahl und Art der Konten nur mit
  Leserecht in `accounts` (der Bereich `system` steht sonst jedem Client offen, auch Hook-Schlüsseln). Der Kern entfernt vor der
  Vorschau Tokens, Passwörter, Adressen mit Zugangsdaten und den Pfad des Home-Verzeichnisses (wird
  `~`). Ohne GitHub-Konto öffnet der Dialog die Seite „New issue“ auf github.com mit vorausgefülltem
  Titel und Text im Browser, dann **nie** mit Log-Auszug: Die Adresse landet im Verlauf des Browsers.
- Protokoll nur mit Konto, `owner/repo#nummer` und Art der Quelle, nie Titel oder Text.

### Entscheidungen

Getroffen am 2026-10-14.

1. **Assistent und Agenten dürfen Issues anlegen, mit Freigabe** („Nach außen“). Im Chat ist die
   Issue-Entwurfs-Karte die Freigabe.
2. **Zuerst GitHub**, der Request so gebaut, dass weitere Systeme dazukommen können.
3. **Alle Einstiege:** Mail und Eingang, Chat, Aufgaben und Kalender, „Report a problem“.
4. **„Also add to tasks“ als Schalter, Standard aus.** Wird eine Aufgabe zum Issue, wird sie immer
   zum Verweis.
5. **Ähnliche Issues als Hinweis** mit „Add comment there instead“; Anlegen bleibt möglich.
6. **Repo je Quelle merken**, sonst das zuletzt benutzte; immer per Klick änderbar.
7. **„Reply with link“ als Angebot** nach einem Issue aus einer Mail, als normaler Mail-Entwurf.
8. **„Report a problem“:** Version und System immer; Log-Auszug nur angehakt, in der Vorschau
   sichtbar, Geheimnisse und Home-Pfade vorher entfernt.

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
8. **Verweise in geteilten Listen** (entschieden, G.6c): Sie werden nicht abgeglichen (G.6b) und
   behalten den Titel vom Anlegen; Abhaken erledigt dort nur die Aufgabe, das Issue bleibt offen
   (siehe „Abhaken“). Wer dort ein Issue schließen will, tut das in einer Filterliste oder auf GitHub.
9. **Log-Auszug nur für Menschen** („Issues anlegen“, Rechte): Eine Regel nach dem Wert eines Requests
   (`kept_for_people`) gehört in „Was nie nach außen geht“ ([api.md](api.md)). Vorschlag: als Punkt 8
   dort aufnehmen; Admin allein reicht nicht, weil auch Skript-Schlüssel Admin haben können. Vor G.13
   entscheiden, weil G.13 den Request anlegt.
10. **Issues aus Besprechungsprotokollen:** Aktionspunkte aus einem Transkript (z. B. Plaud) je als
    Issue. Braucht erst eine Quelle für Protokolle; bis dahin nur „Create issue…“ am Termin.
