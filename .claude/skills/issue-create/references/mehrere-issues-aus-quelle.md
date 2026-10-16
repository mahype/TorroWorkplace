# Mehrere Issues aus einer Quelle

Gilt, wenn aus einer Quelle (Konzept in `docs/`, Protokoll, Mail-Verlauf, Gesprächsnotiz) mehrere Issues entstehen können. Ersetzt die Schritte 1–3 von `issue-create` für diese Fälle; Schritt 4 (Anlegen) läuft danach je Issue unverändert.

## 1. Kandidaten sammeln und sortieren

Geh die Quelle einmal durch und liste jeden Punkt, der nach einer Änderung klingt. Sortiere in drei Gruppen:

- **Issue:** konkrete Änderung in diesem Repo, deren Erledigung sich prüfen lässt.
- **Kein Issue:** Information, Entscheidung ohne Änderung, Wunsch ohne Handlungsbedarf, bereits Erledigtes ohne eigenes Issue.
- **Anderes Repo:** Themen, für die sich in diesem Repository nichts ändert (z.B. am Nextcloud-Server selbst oder an einem anderen Projekt).

Zeig die Gruppen "Kein Issue" und "Anderes Repo" mit je einem Satz Begründung, damit der Nutzer Fehleinschätzungen korrigieren kann.

## 2. Ein gemeinsamer Fragenkatalog

Sammle alle Rückfragen zu allen Kandidaten und stell sie in einer Runde, nicht je Issue einzeln. Fehlt zu einem Kandidaten zu viel, um ihn zu beschreiben, bleibt er offen und kommt nicht in die Vorschau.

## 3. Duplikate auch untereinander prüfen

Neben der Suche im Repo und in `docs/tasks/*.md` (Befehl mit `--json` aus `docs/issue-workflow.md`, Abschnitt "Dubletten-Status") die Kandidaten untereinander vergleichen: Zwei Kandidaten mit demselben Ziel werden eines, ein Kandidat, der nur ein Teil eines anderen ist, wird dessen Punkt in der `Erledigt`-Checkliste.

Zu jedem Treffer im Repo den Status ermitteln (offen, PR offen, erledigt, verworfen), Befehle und Zuordnung in `docs/issue-workflow.md`, Abschnitt "Dubletten-Status". Deckt ein bestehendes Issue den Kandidaten vollständig ab, verlässt er die Kandidatenliste:

- **Bereits erfasst:** das Issue ist offen, mit oder ohne offenen PR.
- **Bereits erledigt:** das Issue ist erledigt.

"Vollständig" heißt: ein einzelnes Issue deckt den Kandidaten allein ab. Decken mehrere Issues ihn nur zusammen ab, bleibt er Kandidat, und der Nutzer entscheidet anhand der Spalte "Dubletten". Decken mehrere Issues ihn jeweils allein ab, gilt das erste Zutreffende: ein offenes → "Bereits erfasst", sonst ein erledigtes → "Bereits erledigt". Verworfene Treffer allein halten ihn nie aus der Tabelle.

Überschneidet sich ein Kandidat nur teilweise mit einem bestehenden Issue oder ist das Issue verworfen, bleibt er Kandidat; Nummer und Status stehen dann in der Spalte "Dubletten" der Vorschau.

## 4. Gesammelte Vorschau mit einer Bestätigung

Zeig alle Kandidaten in einer Tabelle (gerendertes Markdown, kein Codeblock):

| # | Titel | Typ | Kurzbeschreibung | Dubletten |
| - | ----- | --- | ---------------- | --------- |

In der Spalte "Dubletten" steht je Treffer `#<nr> (<status>)`, z.B. `#3100 (PR offen #3112)`, sonst `–`. Unter der Tabelle, in dieser Reihenfolge:

- Die Labels, die für alle gleich gelten, einmal unter der Tabelle.
- Die Absätze "Kein Issue", "Anderes Repo", "Bereits erfasst" und "Bereits erledigt". In den beiden letzten steht jeder Kandidat mit allen Treffern (Issue-Nummer, Status, Titel), auch den teilweisen, das ausschlaggebende Issue zuerst.
- Kandidaten aus diesen Absätzen gehören nicht zu den anzulegenden Issues.

Der Nutzer bestätigt die Tabelle als Ganzes oder streicht und ändert einzelne Zeilen. Erst danach werden die Issues nach Schritt 4 von `issue-create` angelegt, eines nach dem anderen; scheitert eines, werden die übrigen trotzdem angelegt und der Fehler am Ende gemeldet.

### Zurückholen

Soll ein Kandidat ohne Nummer in die Tabelle, weil der Nutzer eine Einordnung für falsch hält (z.B. ein erledigtes Issue, aber das Gespräch nennt eine Regression) oder fehlende Angaben nachliefert, gilt immer dasselbe, egal wann und wie er es sagt:

1. Aus "Kein Issue" oder ein offener Kandidat aus Abschnitt 2 dieser Referenz: fehlende Angaben erfragen, dann Duplikatsuche und Status nach Abschnitt 3 dieser Referenz nachholen. Ist er dabei vollständig abgedeckt, kommt er nach "Bereits erfasst" bzw. "Bereits erledigt" statt in die Tabelle.
2. Aus "Anderes Repo": nur, wenn der Nutzer bestätigt, dass das Issue in dieses Repo gehört; sonst bleibt der Kandidat dort. Danach wie 1.
3. Aus "Bereits erfasst" oder "Bereits erledigt": alle Treffer kommen in die Spalte "Dubletten".
4. Die Tabelle vollständig neu nummeriert zeigen und neu bestätigen lassen; eine Bestätigung oder Auswahl von vorher gilt nicht mehr, weil sich die Nummern ändern können.

## 5. Quelle nennen, ohne sie zu kopieren

Jedes Issue nennt die Quelle mit Art und Datum (z.B. "Konzept docs/fotos.md" oder "Gesprächsnotiz vom 14.10."), ohne Inhalt daraus zu übernehmen:

- keine Auszüge aus dem Transkript, auch keine wörtlichen Zitate
- keine Kundennamen, Firmennamen oder andere personenbezogene Angaben; stattdessen neutral beschreiben ("eine Person, die die App nutzt")
- die Beschreibung stützt sich auf Ziel und Erledigt-Kriterium, nicht auf den Gesprächsverlauf

Die Quelle selbst gehört nicht ins Repo und nicht in ein Issue.
