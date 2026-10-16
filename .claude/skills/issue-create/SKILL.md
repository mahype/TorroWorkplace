---
name: issue-create
description: Legt ein GitHub-Issue nach den Repo-Regeln an — mit Typ-Label, Bereichs-Labels, Label workflow und Board-Status Todo. Erste Phase des Issue-Workflows.
argument-hint: Kurzbeschreibung der Aufgabe
---

# Issue anlegen

Erste Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement`.

Übergebene Beschreibung: `$ARGUMENTS`

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Typ-Labels, Labels, Projekt-Board). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

Der Skill läuft im Haupt-Checkout und legt keinen Worktree an; das macht `issue-plan`.

## 1. Anforderung klären

- Was soll sich ändern, und woran ist zu erkennen, dass es erledigt ist?
- Geht es um eine Funktion, die Menschen in der Oberfläche nutzen: Gibt es sie auch über API und MCP, oder warum fehlt ein Weg (`CLAUDE.md`, „Jede Funktion dreimal“)? Fehlt die Antwort, gehört sie unter die offenen Punkte.
- Ist `$ARGUMENTS` leer oder unklar, frag nach. Nicht raten.

Können aus der Beschreibung oder einer übergebenen Quelle (Konzept in `docs/`, Protokoll, Mail-Verlauf) mehrere Issues entstehen, lies `references/mehrere-issues-aus-quelle.md` und folge dort statt den Schritten 1–3.

## 2. Duplikate suchen

```bash
gh issue list --repo <repo> --state all --search "<stichworte>" --limit 10 --json number,state,stateReason,title
```

Zusätzlich `docs/tasks/*.md` nach den Stichworten durchsuchen. Gibt es ein passendes offenes Issue oder eine passende Aufgabe, zeig sie und frag, ob stattdessen dort weitergearbeitet wird.

## 3. Vorschau zeigen und bestätigen lassen

Zeig im Chat:

- Titel nach den Regeln in `docs/git-workflow.md` (kurz, handlungsorientiert, auf Deutsch, mit Bereich vorne, ohne Issue-Nummer)
- Typ-Label (`feature`, `bug`, `task`, `update`, `docs`) aus der Konfiguration
- Labels: das Typ-Label, `workflow`, dazu passende vorhandene Bereichs-Labels (`gh label list --repo <repo>`)
- Komplexität: `Quick Fix`, `Klein` oder `Normal`, mit einem Satz Begründung. Die Kriterien stehen nur in den Skills `quick-fix` (Abschnitt "Kriterium Direkt-PR") und `issue-small` (Abschnitt "Kriterium klein genug"); schlag sie dort nach, kopier sie nicht hierher. Sieh dafür kurz im Code nach, welche Dateien die Änderung betrifft. Im Zweifel die höhere Stufe.
- Beschreibung: Ziel, Hintergrund, Abgrenzung, bei Funktionen die drei Wege (Oberfläche, API, MCP), ggf. „Erledigt"-Checkliste und offene Punkte. Wo es passt, wie in bestehenden Issues einen Abschnitt „Für Menschen“ voranstellen, der ohne Pfade erklärt, was sich ändert.

Format der Vorschau: gerendertes Markdown, **kein Codeblock**. Titel fett, Labels/Board als kleine Tabelle, die Beschreibung mit echten Überschriften (`###`), Listen und Checkboxen. Die Beschreibung für das Issue-Body-File entsteht erst in Schritt 4 aus dieser Vorschau.

Bei `Quick Fix` frag vor dem Anlegen, ob statt eines Issues direkt `/quick-fix` laufen soll. Bei Ja entsteht kein Issue, und du folgst dem Skill `quick-fix` mit der Beschreibung. Bei Nein geht es wie gewohnt weiter.

Erst nach Bestätigung anlegen. Änderungswünsche einarbeiten und erneut vorlegen.

## 4. Issue anlegen

1. Beschreibung in eine temporäre Datei im Scratchpad schreiben, ohne feste Zeilenbreite (GitHub rendert jeden Zeilenumbruch sichtbar).
2. Anlegen:

   ```bash
   gh issue create --repo <repo> --title "<titel>" --body-file <datei> --label <typ> --label workflow [--label <bereich> …]
   ```

3. Ins Projekt-Board aufnehmen und Status `Todo` setzen (Befehle und IDs in der Konfiguration).

Scheitert Schritt 3, melde das deutlich, das Issue bleibt bestehen. Ohne Typ-Label rät das Worktree-Skript den Branch-Präfix nicht; es muss vor `/issue-plan` gesetzt sein.

## 5. Ergebnis

Gib die Issue-URL aus und gehe nach der Komplexität aus Schritt 3 vor:

- `Normal`, oder `Quick Fix` nach einem Nein in Schritt 3: nenne den nächsten Schritt `/issue-plan <nr>` (mit Opus oder Fable).
- `Klein`: frag "Direkt mit `/issue-small <nr>` umsetzen?". Bei Ja folge dem Skill `issue-small` mit dieser Nummer, bei Nein nenne `/issue-plan <nr>`.

## Wichtig

- Keine Tokens, Passwörter oder personenbezogenen Daten ins Issue.
- Kein Worktree, kein Branch, keine Code-Änderung in dieser Phase.
