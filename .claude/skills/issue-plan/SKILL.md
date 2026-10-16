---
name: issue-plan
description: Analysiert ein Issue im eigenen Worktree, postet einen Implementierungsplan als Kommentar (bei mehreren Schritten mit Aufgaben in docs/tasks/) und setzt das Label awaiting-approval. Zweite Phase des Issue-Workflows, nur mit Opus oder Fable.
argument-hint: <issue-nr>
---

# Issue planen

Zweite Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement`.

Übergebene Issue-Nummer: `$ARGUMENTS`

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Basisbranch, Labels, Worktree-Befehle, Abschnitt "Plan und `docs/tasks/`"). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

## 1. Modell prüfen

`issue-implement` setzt den Plan Schritt für Schritt um und prüft die Angaben darin nicht erneut. Der Plan braucht deshalb ein starkes Modell.

Läuft die Sitzung **nicht** auf Opus oder Fable, sofort abbrechen, ohne Issue zu laden oder etwas zu ändern:

> ⚠️ Für `issue-plan` ist Opus oder Fable nötig. Mit `/model opus` wechseln und `/issue-plan <nr>` erneut starten.

Ausnahme: Läuft die Sitzung in Herdr (`HERDR_ENV=1`) und nicht im Worktree des Issues, entfällt die Prüfung. Schritt 3 übergibt dann an einen Tab mit Opus.

## 2. Issue laden

- Nummer aus `$ARGUMENTS`, sonst aus dem Branch (Regex in der Konfiguration), sonst nachfragen.
- `gh issue view <nr> --repo <repo> --json title,body,labels,milestone,state,comments`
- Kurz ausgeben: Titel und worum es geht, in ein bis zwei Sätzen.
- Steht schon ein Kommentar mit `## Implementierungsplan` darin, frag, ob er ersetzt werden soll. Ein neuer Plan braucht eine neue Freigabe: In Schritt 8 werden dann die Freigabe- und Umsetzungs-Labels entfernt. Übergibt Schritt 3 an einen Herdr-Tab, stellt erst die Sitzung dort diese Frage.

## 3. Worktree sicherstellen

`./scripts/worktree.sh list`. Gibt es keinen Worktree zu der Nummer, anlegen:

```bash
./scripts/worktree.sh create <nr> --yes
```

Gab es den Worktree schon, prüfen, ob er den aktuellen Basisbranch enthält (`git -C <worktree> fetch origin <basisbranch>`, dann `git -C <worktree> merge-base --is-ancestor origin/<basisbranch> HEAD`). Fehlt er und hat der Branch keine eigenen Commits, mit `git -C <worktree> merge --ff-only origin/<basisbranch>` nachziehen; sonst den Rückstand melden und mit dem Nutzer klären.

Liegt die Sitzung nicht im Worktree (`git rev-parse --show-toplevel` ist nicht der Worktree-Pfad), die Planung an einen eigenen Tab übergeben:

```bash
./scripts/worktree.sh agent <nr> plan-<nr> "/issue-plan <nr>" --model opus
```

- Exit 0: melden, dass die Planung im Herdr-Tab `plan-<nr>` läuft, und hier beenden.
- Exit 3 (nicht in Herdr) oder Exit 4 (Übergabe an einen neuen Tab abgeschaltet): hier weiterplanen. Die Analyse liest den Code im Worktree, nicht im Haupt-Checkout.
- Anderer Fehler: Ausgabe melden und fragen, ob hier weitergeplant wird.

## 4. Codebase analysieren

Zuerst in Herdr Phase und Status anzeigen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr <nr> plan planning`.

- Betroffene Crates und Dateien finden, bestehende Muster und die Regeln aus `CLAUDE.md` beachten: festgelegte Schnittstellen, Plattformcode hinter `cfg`, Abhängigkeiten nur über `[workspace.dependencies]`, Nutzertexte englisch mit `tr()`, kein `unsafe` außerhalb von `torro-appkit`.
- Passende Konzepte in `docs/` und vorhandene Aufgaben in `docs/tasks/` lesen; der Plan baut darauf auf und widerspricht ihnen nicht stillschweigend.
- Neue oder geänderte Funktion: Skill `torro-api` laden. Der Plan nennt Oberfläche, `torro_ipc::Request`, Katalogeintrag und MCP-Tool oder begründet, warum ein Weg fehlt.
- Oberfläche betroffen: Skill `torro-design` laden; Entwürfe unter `docs/prototypes/` sind Maßstab.
- Nextcloud betroffen: Skill `nextcloud` laden.
- Widerlegt die Analyse eine Aussage der Issue-Beschreibung oder entscheidet sie einen dort offenen Punkt, die Beschreibung korrigieren (`gh issue edit <nr> --repo <repo> --body-file <datei>`), bevor der Plan gepostet wird. Lösungsvorschläge im Issue sind Annahmen, die geprüft werden, keine Vorgaben.
- Einschätzen, ob das Issue **komplex** ist: greift in mehrere Crates oder Schichten ein, ändert Schnittstellen, braucht nicht-triviale Architektur-Entscheidungen, hat Zustands- oder Nebenläufigkeitslogik mit hohem Fehlerrisiko oder betrifft Schlüssel, Rechte, Freigaben.
- Einschätzen, ob die Umsetzung **mehrere Aufgaben** braucht (Abschnitt "Plan und `docs/tasks/`" der Konfiguration) und ob sie die **Oberfläche** ändert.

## 5. Richtungsfragen klären

Hängt die Richtung des Plans an einer Entscheidung, die nur der Nutzer treffen kann (z.B. Umfang, welcher von zwei tragfähigen Wegen, Verhalten in der Oberfläche), diese per `AskUserQuestion` fragen, bevor der Plan geschrieben wird: höchstens vier Fragen, die empfohlene Option jeweils zuerst. Das gilt auch in einem Herdr-Tab. Die Antwort steht im Plan als Entscheidung mit dem Zusatz „(in der Planung entschieden)“.

Fragen, deren Antwort den Plan nicht ändert, gehören nicht hierher, sondern unter `### Offene Fragen`; `issue-approve` stellt sie bei der Freigabe.

## 6. Plan schreiben

### Aufgaben in `docs/tasks/` (nur bei mehreren Aufgaben)

Braucht die Umsetzung mehrere Aufgaben, schreib sie im Worktree in `docs/tasks/<thema>.md`: in eine passende vorhandene Datei als neuen Abschnitt oder in eine neue Datei nach dem Muster der vorhandenen (Kopf mit Konzept-Verweis und Regeln, Abhängigkeitsbaum, je Aufgabe ID, **Modell** Haiku/Opus, **Braucht**, **Dateien**, Text, Abnahme, `- [ ] erledigt`). IDs folgen dem Schema der Datei. Braucht es ein neues Konzept, gehört es nach `docs/<thema>.md` (Dokumente auf Deutsch). Request und Katalogeintrag kommen in die erste Aufgabe.

Commit auf dem Branch des Issues: `docs(<bereich>): plan #<nr>`. Nicht pushen; das macht später `pullrequest`. Der Plan-Kommentar verweist auf die Datei mit dem SHA dieses Commits.

Bei einer einzigen Aufgabe entfällt das: Der Agententeil des Plans ist dann die Aufgabe.

### Plan-Kommentar

Format:

```markdown
## Implementierungsplan

### Überblick für die Freigabe

Kernaussage in 2–4 Sätzen: was und warum.

**Entscheidungen, die mit der Freigabe bestätigt werden**

1. Entscheidung, mit verworfener Alternative und Grund

**Oberfläche, API, MCP**

- Oberfläche: …; API: `Request::…`; MCP: `bereich_tool` (oder warum ein Weg fehlt)

**Von Hand zu prüfen**

- Was der Mensch selbst ansehen oder ausführen muss, mit erwartetem Ergebnis

**Nicht im Issue bedacht**

- Befund außerhalb des Issue-Umfangs, den der Freigebende kennen muss

### Offene Fragen

- Frage, die ohne Antwort des Nutzers nicht entscheidbar ist

<details>
<summary>Umsetzungsschritte für den Agenten (Stand <code>{SHA}</code>)</summary>

### Aufgaben

- `docs/tasks/<thema>.md` beim Stand `{SHA}`: <ID>, <ID>, <ID> (in dieser Reihenfolge)

### Betroffene Dateien

- `crates/…/datei.rs` — was sich ändert

### Schritte

1. Schritt mit Datei und Symbolname (Entscheidung N)

Modell: Haiku|Sonnet|Opus
Oberfläche: ja|nein

### Risiken / Hinweise

- Was hier konkret zutrifft und nachgesehen wurde

</details>

Branch erstellt: `<branch>`
```

- `{SHA}`: `git -C <worktree> rev-parse --short HEAD`, also der Stand, gegen den tatsächlich geplant wurde (nach dem Aufgaben-Commit).
- `### Aufgaben` nur bei mehreren Aufgaben; dann entfallen `### Betroffene Dateien` und `### Schritte`, weil die Aufgaben sie tragen. Bei einer Aufgabe entfällt `### Aufgaben`.
- **Oberfläche, API, MCP** nur bei neuen oder geänderten Funktionen.
- Nach `<summary>` und vor `</details>` steht eine Leerzeile, sonst rendert GitHub den eingeklappten Teil nicht als Markdown.
- `### Offene Fragen` steht außerhalb von `<details>` und wörtlich so; ohne offene Fragen entfällt der Abschnitt.
- `Oberfläche: ja|nein` steht wörtlich im Agententeil; `issue-implement` lässt bei `ja` die Sichtprüfung laufen.
- Die Zeile `Branch erstellt:` erfüllt die Regel aus `docs/git-workflow.md`, Abschnitt "Issue-Kommentare bei Branch/PR-Erstellung". Sie steht nur, wenn noch kein Kommentar des Issues `Branch erstellt:` mit diesem Branch enthält.
- Keine harten Zeilenumbrüche im Kommentartext: ein Absatz oder Listenpunkt ist eine Zeile.

**Überblick** (für den Menschen, der freigibt): keine Pfade, Symbole oder Zeilennummern. Entscheidungen nummerieren, jede mit verworfener Alternative und Grund. „Von Hand zu prüfen" steht nur hier. Richtwert ~1500 Zeichen.

**Nicht im Issue bedacht** steht nur, wenn die Analyse etwas findet, das das Issue nicht anspricht und das entweder die Entscheidung über die Freigabe ändern kann oder ohne das die Änderung scheitert oder Schaden anrichtet. Keine allgemeinen Risiken, keine Verbesserungsideen; ohne solchen Befund entfällt der Abschnitt. Ändert ein Befund die Richtung des Plans, ist er eine Richtungsfrage (Schritt 5).

**Agententeil** (Arbeitsanweisung, wird einmal gelesen und befolgt):

- Jeder Schritt nennt Datei und Symbolname; Zeilennummern nur, wo das Symbol nicht eindeutig ist.
- Exakte Werte, wo es welche gibt (Varianten, Konstanten, Tool-Namen), aber kein fertiger Code oder Wortlaut.
- Die Grenze der Änderung, wo sie nicht offensichtlich ist.
- Was nachzuziehen ist (Aufrufer, Katalog, MCP-Tool, Übersetzungen, Doku); auch „nachgesehen, nichts nachzuziehen" ist eine Zeile.
- Ein Verifikationsschritt am Ende: die Prüfbefehle aus der Konfiguration (Abschnitt "Prüfen") und woran der Erfolg zu erkennen ist; jeden „Erledigt"-Punkt des Issues einem Schritt und einem Prüfweg zuordnen, nur von Hand Prüfbares ausdrücklich so benennen.
- Keine Wiederholung des Issues, keine Begründungen, die schon als Entscheidung oben stehen, keine allgemeinen Risiken.
- Richtwert unter ~3000 Zeichen, bei komplexen Issues unter ~5000.

## 7. Plan gegenprüfen

Jede konkrete Angabe vor dem Posten am Code nachsehen, nicht aus der Erinnerung bestätigen: Datei und Symbol existieren, Signaturen stimmen, behauptete Nicht-Betroffenheit ist nachgesehen, jede Entscheidung findet sich in einem Schritt oder einer Aufgabe wieder. Stimmt etwas nicht, den Plan korrigieren.

Danach eine Frage durchgehen: Angenommen, die Änderung ist umgesetzt und scheitert beim ersten echten Einsatz oder richtet Schaden an, woran liegt es? Führt die Antwort auf einen konkreten, nachgesehenen Grund, den Plan korrigieren oder den Befund unter „Nicht im Issue bedacht“ aufnehmen. Führt sie nur auf Allgemeines, kommt nichts in den Plan.

Bei neuen oder geänderten Funktionen zusätzlich den Agenten `torro-api-review` auf den Plan ansetzen (Planungsmodus: fehlt ein Weg, verletzt der Plan eine Sicherheitsregel?) und berechtigte Funde einarbeiten.

## 8. Label setzen und posten

Erst die Labels, dann der Kommentar: So wird ein neuer Plan nie unter einer alten Freigabe sichtbar.

```bash
gh issue edit <nr> --repo <repo> --add-label awaiting-approval --add-assignee @me
gh issue comment <nr> --repo <repo> --body-file <plandatei>
```

Bei komplexem Issue im ersten Befehl zusätzlich `--add-label strong-model-recommended`.

Ersetzt der Plan einen früheren, im ersten Befehl zusätzlich `--remove-label approved --remove-label started-implementation --remove-label pr-open`: Eine Freigabe gilt nur für den Plan, den der Mensch gesehen hat. `strong-model-recommended` nach der neuen Einschätzung setzen oder entfernen.

Danach den Projekt-Status `Awaiting Approval` setzen (Befehle und IDs in der Konfiguration). Dann den Herdr-Status nachziehen: `node scripts/agent/skill-phase.mjs herdr <nr> plan awaiting-approval`.

## 9. Abschluss

Fasse im Chat den Überblick zusammen (Entscheidungen, von Hand zu prüfen, nicht im Issue bedacht, offene Fragen, bei mehreren Aufgaben die IDs) und gib die Kommentar-URL aus. Nächster Schritt: Der Nutzer prüft den Plan und gibt ihn mit `/issue-approve <nr>` frei, gern direkt in dieser Sitzung; in Herdr öffnet die Freigabe die Umsetzung in einem eigenen Tab. Bei komplexem Issue den Hinweis ergänzen, dass für die Umsetzung `/model opus` empfohlen ist.

Als letzte Ausgabe den Abschlussblock (Format in der Konfiguration, Abschnitt "Abschlussblock").

## Wichtig

- Der Plan ist ein Kommentar; die Issue-Beschreibung wird nur korrigiert, nicht durch den Plan ersetzt.
- Kein Code in dieser Phase. Einziger Commit ist der mit den Aufgaben in `docs/tasks/` (und ggf. dem Konzept), nur bei mehreren Aufgaben.
- Der Skill ruft selbst keinen Editor auf.
