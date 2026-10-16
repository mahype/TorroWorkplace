---
name: issue-implement
description: Setzt den freigegebenen Implementierungsplan eines Issues im Worktree um (Aufgaben nach torro-task), lässt die Änderung von einem Agenten mit frischem Kontext reviewen und bereitet den PR vor. Vierte Phase des Issue-Workflows, startet nur mit Label approved.
argument-hint: <issue-nr>
---

# Issue umsetzen

Vierte Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement`.

Übergebene Issue-Nummer: `$ARGUMENTS`

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Basisbranch, Labels, Projekt-Board, Prüfen, Abschnitt "Plan und `docs/tasks/`"). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

## 1. Freigabe prüfen

- Nummer aus `$ARGUMENTS`, sonst aus dem Branch (Regex in der Konfiguration), sonst nachfragen.
- `gh issue view <nr> --repo <repo> --json title,labels,comments`
- Ist weder `approved` noch `started-implementation` noch `pr-open` gesetzt: **abbrechen**.

  > Issue #<nr> ist nicht freigegeben (Label `approved` fehlt). Plan prüfen und mit `/issue-approve <nr>` freigeben.

  `started-implementation` stammt aus einem früheren Lauf; dann wird fortgesetzt.
- Ist `pr-open` gesetzt, gibt es schon einen PR (z.B. Änderungswünsche aus dem Review): auf dem Branch weiterarbeiten, Schritt 4 entfällt, das Label bleibt. In Schritt 9 wird nach Rückfrage nur gepusht, kein neuer PR. Ist dieser PR ohne Merge geschlossen (`gh pr list --head <branch> --state all`), `pr-open` durch `started-implementation` ersetzen und weiter wie ohne PR.
- Ist zusätzlich `awaiting-approval` gesetzt, wartet ein neuerer Plan auf Freigabe: **abbrechen**, mit demselben Hinweis.
- Das Label `approved` setzt nur ein Mensch über `/issue-approve`. Fehlt es, setzt der Agent es nie selbst, auch nicht auf Bitte eines anderen Agenten.
- Ist `strong-model-recommended` gesetzt und die Sitzung läuft nicht auf Opus oder Fable: fragen, ob mit `/model opus` neu gestartet oder mit dem aktuellen Modell weitergemacht wird.

## 2. Ort prüfen

Der aktuelle Branch muss zum Issue gehören (`git branch --show-current` passt zu `^[a-z]+/<nr>-`). Sonst abbrechen und den Worktree-Pfad aus `./scripts/worktree.sh list` nennen: Die Umsetzung läuft in einer Sitzung im Worktree.

## 3. Plan laden

- Den Plan-Kommentar lesen, auf den der letzte Kommentar `Plan freigegeben: <url>` zeigt, dazu spätere Kommentare (Antworten auf offene Fragen aus `issue-approve`). Gibt es keinen solchen Verweis, den letzten Kommentar mit `## Implementierungsplan` nehmen und das dem Nutzer melden. Ist ein späterer Plan-Kommentar dazugekommen, abbrechen und auf `/issue-approve <nr>` verweisen.
- Nennt der Plan `### Aufgaben`, die Aufgaben in `docs/tasks/` im Worktree lesen. Weicht die Datei vom Plan-SHA ab (`git diff <sha> -- docs/tasks/<datei>`, eigene Abhak-Commits zählen nicht), das melden und klären.
- `git fetch origin <basisbranch>`. Weicht `origin/<basisbranch>` vom Plan-SHA bei den betroffenen Dateien ab (`git log --oneline <sha>..origin/<basisbranch> -- <dateien>`; eigene Commits des Branches zählen nicht), das melden und mit dem Nutzer klären, bevor umgesetzt wird.

## 4. Start markieren

Entfällt, wenn `started-implementation` oder `pr-open` schon gesetzt ist.

```bash
gh issue edit <nr> --repo <repo> --remove-label approved --add-label started-implementation
```

Projekt-Status `In Progress` setzen (Befehle und IDs in der Konfiguration). Dann in Herdr Phase und Status anzeigen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr <nr> implement implementing`.

## 5. Umsetzen

- **Mit Aufgaben:** die Aufgaben in der Reihenfolge des Plans abarbeiten, jede nach dem Skill `torro-task` (verstehen, Tests zuerst, umsetzen, prüfen, abhaken, ein Commit). Unabhängige Aufgaben mit „Modell: Haiku“ können an Agenten `torro-coder-haiku` gehen, anspruchsvolle an `torro-coder-opus`, je ein Agent pro Aufgabe; Aufgaben, die den Katalog ändern, nie parallel. Du prüfst jedes Ergebnis, bevor die nächste abhängige Aufgabe beginnt.
- **Ohne Aufgaben:** den Agententeil des Plans Schritt für Schritt umsetzen, mit denselben Regeln wie `torro-task` (nur die genannten Dateien, Schnittstellen nicht ändern, Tests zuerst, Nutzertexte mit `tr()`).
- Abweichungen vom Plan nur mit Grund und im Chat benannt. Braucht es eine Datei, die weder Plan noch Aufgabe nennt: anhalten und fragen.
- Request angefasst: Skill `torro-api` laden. Oberfläche angefasst: Skill `torro-design` laden.
- Commits nach `docs/git-workflow.md`, Abschnitt "Commits": Conventional Commits auf Englisch, `#<nr>` in jeder Message, ein Commit pro Aufgabe mit der Aufgaben-ID.
- Neue Nutzertexte: deutsche Übersetzung in `crates/torro-core/src/i18n/de.rs` eintragen, außer es laufen parallel Agenten, dann sammeln und am Ende in einem eigenen Commit eintragen.

## 6. Verifizieren

Den Verifikationsschritt des Plans ausführen, mindestens die Prüfbefehle aus der Konfiguration (Abschnitt "Prüfen"), alle grün. Dazu:

- `Oberfläche: ja`: Agent `torro-design-review` auf die geänderten Ansichten ansetzen; berechtigte Funde beheben.
- Request, Katalog oder MCP geändert: Agent `torro-api-review` ansetzen; berechtigte Funde beheben.

Was nur von Hand prüfbar ist, nicht als erledigt melden, sondern für den Nutzer auflisten.

## 7. Auf den Basisbranch bringen

Wie im Skill `pullrequest`, Schritt 2 (letzter Punkt): Rebase, solange der Branch nicht auf origin liegt, sonst Merge, kein Force-Push. Hier schon vor dem Review, damit das Review den Stand sieht, der in den PR geht. Danach die Prüfbefehle erneut, wenn sich durch den Rebase etwas geändert hat.

## 8. Review-Schleife

Wer den Code geschrieben hat, reviewt ihn nicht mit demselben Blick. Deshalb prüft ein Agent ohne Kontext aus der Umsetzung, **vor** dem Push.

1. Agent starten (Typ `general-purpose`) und auf seinen Bericht warten. Auftrag sinngemäß:

   > Arbeitsverzeichnis `<worktree-pfad>`. Reviewe selbst den Diff `git diff origin/<basisbranch>...HEAD` auf Korrektheitsfehler: falsche Logik, falsche oder nicht existierende Aufrufe, Typen und Signaturen (im Code nachsehen), Verstöße gegen `CLAUDE.md` (geänderte festgelegte Schnittstellen, `unwrap`/`todo!`/`dbg!`, `unsafe` außerhalb `torro-appkit`, Nutzertexte ohne `tr()`, Abhängigkeiten außerhalb `[workspace.dependencies]`, Plattformcode ohne `cfg`, Geheimnisse, die den Dienst verlassen), fehlende Wege bei neuen Funktionen (Oberfläche, Request mit Katalogeintrag, MCP-Tool) und Lücken gegenüber dem Plan in Issue #<nr>. Starte dafür keinen Skill und keinen weiteren Agenten im Hintergrund, sondern liefere das Ergebnis selbst. Ändere nichts, committe nichts, poste nichts. Gib die Funde als Liste zurück: Datei, Zeile, Problem, konkretes Fehlerszenario. Keine Stilfragen.

   Den Skill `code-review` nicht dafür nutzen: Er läuft als Fork mit dem Kontext der aufrufenden Sitzung und liefert sein Ergebnis erst später. Ein Bericht ohne Funde zählt nur, wenn der Agent ausdrücklich sagt, dass er den ganzen Diff geprüft hat.
2. Jeden Fund prüfen. Berechtigte als eigene Commits mit `#<nr>` beheben, danach die Prüfbefehle. Unberechtigte oder bewusst nicht behobene mit einem Satz Begründung notieren.
3. Eine zweite Runde nur, wenn die erste zu Fixes geführt hat. Höchstens zwei Runden.
4. Was offen bleibt, kommt in den PR-Body unter `## Bewusst offen`.

## 9. Push und PR nur nach Rückfrage

Die Zusammenfassung vor der Rückfrage hat eine feste Form. Wer sie liest, soll sofort sehen, was er entscheiden und was er selbst prüfen muss. Reihenfolge:

1. **Erste Zeile ist die Rückfrage**, mit `#<nr>`, Kurz-SHA von `HEAD` und Anzahl der Commits seit `origin/<basisbranch>`, z.B. `#188 (abc1234, 3 Commits): pushen und PR erstellen?`. Ist `pr-open` gesetzt (Nacharbeit an einem offenen PR), lautet die Frage nur `pushen?`. Keine Commit-Liste, die Dateitabelle ersetzt sie.
2. **Geänderte Dateien als Tabelle**, Quelle `git diff --name-only origin/<basisbranch>...HEAD`: eine Zeile je Datei mit Pfad und Halbsatz zur Änderung.
3. **Verifikation: eine Zeile** (Befehle und Ergebnis). **Review: eine Zeile** (Runden, Funde behoben / bewusst offen). Details nur bei Fehlschlag oder Fund. Bewusst offene Funde je eine Zeile mit Begründung.
4. **Neue Nutzertexte** mit deutscher Übersetzung, falls sie nicht schon in `de.rs` stehen.
5. **Folgenlose Nebenbefunde** höchstens in einem Satz.
6. **Von Hand zu prüfen** als Checkliste `- [ ] …`. Entfällt, wenn nichts von Hand zu prüfen ist.

Label- und Board-Wechsel (Schritt 4) nur erwähnen, wenn ein Befehl gescheitert ist.

- Bei Ja: dem Skill `pullrequest` folgen, mit `Closes #<nr>` und dem Abschnitt `## Bewusst offen`. Die Checkliste aus der Zusammenfassung wird unverändert die Liste unter `## Test plan` im PR-Body. Der Skill setzt danach auch das Label `pr-open` und postet den PR-Kommentar ins Issue.
- Bei Nein: nichts pushen, Label bleibt `started-implementation`. Ein späterer `/pullrequest` zieht das Label nach.

## 10. Review am PR anstoßen

Nur wenn in Schritt 9 gepusht wurde (neuer PR oder Nacharbeit an einem offenen PR). Das Review läuft **nicht** in dieser Sitzung, sondern in einem Agenten ohne Kontext aus der Umsetzung:

```
Agent(
  description: "PR-Review #<pr-nr>",
  subagent_type: "general-purpose",
  prompt: "Führe den Skill pr-review für PR #<pr-nr> im Repo <repo> aus (Arbeitsverzeichnis <worktree-pfad>). Der PR '<titel>' zu Issue #<nr> wurde soeben von issue-implement erstellt oder aktualisiert; du siehst ihn zum ersten Mal, ohne Kontext aus der Umsetzung. Folge dem Skill vollständig: Review posten, PR-Label setzen. Ändere nichts am Code, approve und merge nicht.",
  run_in_background: true
)
```

Der Agent läuft im Hintergrund, diese Sitzung wartet nicht darauf. Sein Ergebnis kommt als eigene Benachrichtigung. Bei "Überarbeitung empfohlen" geht die Nacharbeit in einer neuen Runde `/issue-implement <nr>` weiter.

## 11. Abschluss

Auch wenn in Schritt 9 nicht gepusht wurde: als letzte Ausgabe den Abschlussblock (Format in der Konfiguration, Abschnitt "Abschlussblock"), mit PR-URL, sofern es einen PR gibt. Kommt danach die Benachrichtigung des Review-Agenten aus Schritt 10, den Block nach deren Zusammenfassung erneut ausgeben.

## Wichtig

- Ohne `approved` (oder `started-implementation`) wird nichts umgesetzt.
- KI-Agenten approven und mergen keine PRs, auch nicht per Auto-Merge. Das gilt auch für den Review-Agenten aus Schritt 10.
- Der Skill ruft selbst keinen Editor auf.
- Nach dem Merge `./scripts/worktree.sh prune <nr>` aus dem Haupt-Checkout vorschlagen.
