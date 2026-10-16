---
name: issue-approve
description: Gibt den Implementierungsplan eines Issues frei (Label approved) und startet in Herdr die Umsetzung in einem eigenen Tab. Dritte Phase des Issue-Workflows, nur durch Menschen.
argument-hint: <issue-nr>
disable-model-invocation: true
---

# Plan freigeben

Dritte Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement`.

Übergebene Issue-Nummer: `$ARGUMENTS`

Die Freigabe ist eine menschliche Entscheidung. Dieser Skill läuft nur, wenn der Nutzer ihn selbst aufruft; ein Agent startet ihn nicht von sich aus.

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Labels, Worktree-Befehle, Herdr). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

## 1. Plan prüfen

- Nummer aus `$ARGUMENTS`, sonst aus dem Branch (Regex in der Konfiguration), sonst nachfragen.
- `gh issue view <nr> --repo <repo> --json title,labels,comments`
- Erste Ausgabe ist die Bezugszeile `#<nr> – <Titel>`, darunter ein bis zwei Sätze, worum es im Issue geht. So ist bei mehreren parallelen Tabs sofort klar, welches Issue hier freigegeben wird.
- In Herdr die Phase im Pane anzeigen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr <nr> approve`.
- Ohne Kommentar mit `## Implementierungsplan`: abbrechen und auf `/issue-plan <nr>` verweisen.
- Ist `approved` oder `started-implementation` gesetzt und `awaiting-approval` nicht: Der Plan ist schon freigegeben. Das melden und direkt mit Schritt 4 weitermachen.
- Sonst muss `awaiting-approval` gesetzt sein; fehlt es, abbrechen und auf `/issue-plan <nr>` verweisen.
- Zeig aus dem letzten Plan-Kommentar den Überblick (Entscheidungen, Oberfläche/API/MCP, von Hand zu prüfen). Nennt der Plan Aufgaben in `docs/tasks/`, zusätzlich deren IDs und Titel aus der Datei im Worktree.

## 2. Offene Fragen klären

Hat der Plan `### Offene Fragen`, frag sie einzeln ab. Die Antworten als Kommentar ins Issue posten (`gh issue comment <nr> --repo <repo> --body-file <datei>`, ohne feste Zeilenbreite), damit `issue-implement` sie findet. Ändert eine Antwort den Plan grundlegend, nicht freigeben, sondern auf `/issue-plan <nr>` verweisen.

## 3. Freigeben

Ausdrücklich fragen: „Plan freigeben? (ja/nein)". Nur bei Ja weiter; bei Nein nichts ändern und beenden. Der Aufruf des Skills allein ist keine Freigabe, der Nutzer soll den Überblick vorher gesehen haben.

```bash
gh issue edit <nr> --repo <repo> --remove-label awaiting-approval --add-label approved
```

Danach die Freigabe an den Kommentar binden, damit kein später geposteter Plan unter dieser Freigabe läuft: Kommentar `Plan freigegeben: <url-des-plan-kommentars>` posten (`gh issue comment <nr> --repo <repo> --body-file <datei>`). Dann den Herdr-Status setzen: `node scripts/agent/skill-phase.mjs herdr <nr> approve approved`.

## 4. Worktree prüfen

`./scripts/worktree.sh list`. Fehlt der Worktree, nicht selbst anlegen: melden und auf `/issue-plan <nr>` verweisen, das ihn samt Branch-Kommentar anlegt.

## 5. Umsetzung starten

Die Umsetzung in einem eigenen Tab starten, damit der Planungskontext nicht mitläuft:

```bash
./scripts/worktree.sh agent <nr> impl-<nr> "/issue-implement <nr>" --model <modell>
```

- `--model` immer angeben, nach Label: `opus` bei `strong-model-recommended`, sonst `sonnet`. `fable` nur, wenn der Nutzer es ausdrücklich verlangt, nie automatisch.
- Bewusst ohne `--exit-caller`: Die Freigabe-Sitzung bleibt für Nachfragen offen, falls die Umsetzung den Plan in Frage stellt.
- Kam die Sitzung über „schon freigegeben“ aus Schritt 1 hierher und gibt es den Agenten `impl-<nr>` schon (`herdr agent get impl-<nr>` endet mit Exit 0), diesen Schritt überspringen: Ein untätiger Umsetzungs-Chat bekäme sonst eine neue Runde `/issue-implement`.
- Exit 3 heißt: nicht in Herdr. Exit 4 heißt: Die Übergabe an einen neuen Tab ist abgeschaltet (Standard, `HERDR_HANDOFF_IMPL`); es wurde nichts gestartet. Exit 1 mit „läuft schon in Herdr“ heißt: `impl-<nr>` arbeitet gerade, der Prompt wurde nicht geschickt. Bei einem anderen Fehler die Ausgabe melden. In allen Fällen bleibt die Freigabe bestehen.

## 6. Abschluss

Gib aus:

- Issue-URL und Worktree-Pfad
- Genau ein Fall, nach der Ausgabe von Schritt 5 (Vorrang in dieser Reihenfolge):
  - Schritt 5 übersprungen, oder Exit 1 mit „läuft schon in Herdr“: Die Umsetzung läuft schon im Herdr-Tab `impl-<nr>`. Keinen zweiten Umsetzungs-Chat starten.
  - Exit 0: Die Umsetzung läuft im Herdr-Tab `impl-<nr>`; dieser Tab kann geschlossen werden, wenn keine Nachfragen mehr offen sind.
  - Exit 3, Exit 4 oder anderer Fehler: siehe nächster Punkt.
- Ohne Herdr, bei abgeschalteter Übergabe (Exit 4) oder nach einem Fehler in Schritt 5 als nächster Schritt: Im Worktree einen frischen Chat starten (oder `/clear`), damit der Planungskontext nicht in die Umsetzung mitläuft, dann `/issue-implement <nr>`. Vorher `/model opus`, wenn `strong-model-recommended` gesetzt ist, sonst `/model sonnet`.
- Als letzte Ausgabe den Abschlussblock (Format in der Konfiguration, Abschnitt "Abschlussblock").

## Wichtig

- Kein Code, keine Commits in dieser Phase.
- Der Skill ruft selbst keinen Editor auf.
- `disable-model-invocation` wirkt nur in Claude Code. In anderen Agenten gilt die Regel trotzdem: Freigeben nur, wenn der Nutzer es ausdrücklich verlangt.
