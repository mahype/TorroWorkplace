---
name: pr-triage
description: Sortiert Review-Kommentare eines Pull Requests in PR-relevantes Feedback und eigenständige Issues und legt letztere nach Bestätigung an. Optionale sechste Phase des Issue-Workflows, nach pr-review.
argument-hint: <pr-nr>
---

# PR-Kommentare triagieren

Optionale sechste Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement` → `pr-review` → `pr-triage`.

Übergebene PR-Nummer: `$ARGUMENTS`

Klassifiziert jeden Review-Kommentar als **PR-relevant** (gehört zum Scope des PRs, wird im PR erledigt) oder **neues Issue** (eigenständige Arbeit, die separat getrackt wird).

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Typ-Labels, Projekt-Board). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

## 1. Kommentare laden

- Nummer aus `$ARGUMENTS`, sonst den PR des aktuellen Branches, sonst nachfragen.
- ```bash
  gh pr view <nr> --repo <repo> --json title,body,url,number,comments,reviews
  gh pr diff <nr> --repo <repo>
  gh api repos/<repo>/pulls/<nr>/comments --paginate --jq '.[] | {id, body, path, line, user: .user.login, in_reply_to_id}'
  ```
  Der letzte Aufruf liefert die Zeilen-Kommentare, `comments` im ersten die allgemeinen PR-Kommentare, darunter den Review-Kommentar aus `pr-review`. Aus diesem kommen die Funde, die nicht in den PR gehören.
- Nur Top-Level-Kommentare (ohne `in_reply_to_id`) und nur solche, die noch keine Antwort `Als eigenständiges Issue erfasst: #…` tragen.

## 2. Klassifizieren

**PR-relevant**, wenn der Kommentar sich auf die geänderten Zeilen bezieht, einen vom PR eingeführten Fehler benennt, den Ansatz hinterfragt oder Tests für den geänderten Code verlangt.

**Neues Issue**, wenn der Kommentar:

- Code oder Verhalten außerhalb des Diffs betrifft oder einen schon vorher vorhandenen Fehler beschreibt
- Umbau von unberührtem Code oder Tech Debt vorschlägt, die der PR nicht verursacht hat
- ein Feature jenseits des Issues vorschlägt ("wir sollten auch…", "wenn wir schon dabei sind…")

Dem neuen Issue ein Typ-Label aus `docs/issue-workflow.md` zuordnen: `bug` für einen Defekt, `feature` für eine Erweiterung, `task` für Aufräumen und Umbau, dazu passende Bereichs-Labels.

## 3. Tabelle zeigen und bestätigen lassen

| # | Kommentar (Auszug) | Autor | Einstufung | Typ-Label | Begründung |
|---|---|---|---|---|---|

**Auf Bestätigung warten**, bevor Issues angelegt, Antworten gepostet oder Threads aufgelöst werden. Änderungswünsche einarbeiten und erneut vorlegen.

## 4. Issues anlegen

Je bestätigtem `new-issue` nach `issue-create`, Schritt 4: Titel nach `docs/git-workflow.md`, Typ-Label, `workflow` und Bereichs-Labels, Projekt-Board mit Status `Todo` (Befehle in der Konfiguration). Der Body bleibt kurz, ohne feste Zeilenbreite, das Zitat steht am Ende:

```markdown
#### Symptom

Was der Fund konkret bedeutet, ein bis zwei Sätze.

#### Fundstelle

Permalinks auf die betroffenen Codestellen, nur wenn im Kommentar genannt oder eindeutig auffindbar.

#### Erledigt

- [ ] Prüfbares Kriterium.

<details>
<summary>Ursprung: Kommentar von @&lt;autor&gt; in PR #&lt;nr&gt;</summary>

> <zitierter Original-Kommentar>

</details>
```

Bei `feature` und `task` statt **Symptom** den Abschnitt **Ziel**. Nicht paraphrasieren: Das Zitat trägt den Kontext. Ein Issue ohne abhakbares Kriterium wird nicht angelegt.

## 5. Antworten und Threads auflösen

- Auf den Zeilen-Kommentar antworten (der Endpoint `/replies` existiert nicht):

  ```bash
  gh api repos/<repo>/pulls/<nr>/comments -f body="Als eigenständiges Issue erfasst: #<issue>" -F in_reply_to=<comment-id>
  ```

- Thread auflösen, nur für Zeilen-Kommentare, die ein neues Issue wurden (geht nur per GraphQL):

  ```bash
  gh api graphql -f query='query { repository(owner: "<owner>", name: "<name>") { pullRequest(number: <nr>) { reviewThreads(first: 50) { nodes { id isResolved comments(first: 1) { nodes { databaseId } } } } } } }'
  gh api graphql -f query='mutation { resolveReviewThread(input: { threadId: "<PRRT_…>" }) { thread { isResolved } } }'
  ```

  Die Thread-ID über die `databaseId` des ersten Kommentars zuordnen.
- Funde aus dem allgemeinen Review-Kommentar haben keinen Thread: dafür ein kurzer PR-Kommentar `Aus dem Review als Issue erfasst: #<a>, #<b>`.
- PR-relevante Kommentare bleiben offen; sie löst der Autor mit dem Fix.

## 6. Ergebnis

Fasse zusammen: Zahl der Kommentare, davon PR-relevant, davon neue Issues (mit Links), Zahl der aufgelösten Threads.

## Wichtig

- Vor der Bestätigung wird nichts angelegt, gepostet oder aufgelöst.
- Der Skill ändert keinen Code und mergt nichts.
- Keine Tokens, Passwörter oder personenbezogene Daten in neue Issues, auch nicht aus dem zitierten Kommentar.
