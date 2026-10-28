# Git-Workflow

Regeln für Issues, Branches, Commits und Pull Requests, für Menschen und KI-Assistenten. Den Ablauf führen die
Skills des Plugins enon-tools; Board und Typen stehen in `.enon-tools/konventionen.json`, die Regeln des Repos in
[issue-workflow.md](issue-workflow.md).

## Standardablauf für Änderungen

1. Issue erstellen (`issue-create`).
2. Issue-Branch in einem eigenen Worktree erstellen (`issue-plan` legt ihn an, von Hand `<cli> worktree create <nr>`).
3. Änderung im Worktree umsetzen.
4. Commits mit Issue-Nummer erstellen.
5. Vor dem Pull Request den aktuellen `main` holen und übernehmen. Ob der Branch schon auf origin liegt,
   entscheidet das Vorgehen (`git ls-remote --exit-code --heads origin <branch>`):

   ```bash
   git fetch origin main

   # Branch noch nicht gepusht: rebasen, keine Merge-Commits in der PR-Historie
   git rebase origin/main

   # Branch schon auf origin (Backup-Push, Draft-PR, offener PR): mergen
   git merge origin/main
   ```

   Rebase nur auf einem ungepushten Branch: Er schreibt Commits um, ein Force-Push wäre nötig.
6. `scripts/ci-local.sh` und die Tests auf Deutsch laufen lassen, dann Pull Request gegen `main` erstellen.
7. Nach Review in `main` mergen; das macht ein Mensch.
8. Nach dem Merge Worktree und Branch aufräumen: `/worktree-cleanup` in der Sitzung im Worktree.

Die Schritte 1 bis 8 liegen als Skills des Plugins enon-tools vor: `issue-create` legt das Issue an, `issue-plan` legt den Worktree an und
postet einen Implementierungsplan, `issue-approve` gibt ihn frei (nur durch Menschen), `issue-implement` setzt um,
lässt vor dem Push ein Review mit frischem Kontext laufen und erstellt den PR (Rückfrage nur bei Auffälligkeiten), `pr-review` reviewt
den offenen PR (ohne Approve, ohne Merge), `pr-triage` macht aus Review-Funden außerhalb des PR-Scopes eigene
Issues. Für Kleinigkeiten legt `issue-small` Issue und Worktree an und setzt ohne Plan und Freigabe um. Für winzige
Änderungen ohne Issue gibt es `quick-fix`, für das Aufräumen nach dem Merge `worktree-cleanup`. `pr` ist ein
Synonym für `pullrequest`. `<cli>` steht für `node <Plugin-Ordner>/bin/enon-tools.mjs`.

KI-Agenten dürfen Pull Requests vorbereiten und erstellen, aber niemals freigeben, als Approved reviewen oder selbst
mergen. Fällt nach dem Merge ein neues oder fortbestehendes Problem auf, wird dafür ein neues Issue erstellt.

## Direkt-PR ohne Issue

Für sehr kleine Änderungen (Tippfehler, falscher Link, Übersetzung, kleine Korrektur in einem Skript oder Skill)
geht der Ablauf direkt vom Branch zum PR, als Skill `quick-fix`. Kriterium und Grenzen:
[issue-workflow.md](issue-workflow.md), Abschnitt "Direkt-PR ohne Issue". Branch `<typ>/<slug>` ohne Nummer,
Commit ohne Issue-Nummer, z.B. `docs: fix typo in roadmap`, PR-Body kurz und ohne `Closes`.

## Issue-, Branch- und PR-Verknüpfung

**Issue-Titel:**
- Kurz, konkret und handlungsorientiert, auf Deutsch.
- Wenn möglich mit Bereich/Komponente beginnen, z.B. `Kalender: Erinnerungen auch über den ganzen Bildschirm`
  oder `Mail-Leser: Standard-Knöpfe oben, Torro-Leiste unten`.
- Keine Issue-Nummer in den Titel; GitHub vergibt sie.
- Jedes Issue bekommt genau ein Typ-Label (`feature`, `bug`, `task`, `update`, `docs`) und passende Bereichs-Labels.
- Issues, die durch die Skills laufen, tragen `en-workflow` und `aufwand:klein` bzw. `aufwand:normal`; Issues am
  Tooling selbst (Skills, Skripte, Hooks, `CLAUDE.md`) `ai-tooling`.
- Keine Milestones.

**Projektzuordnung und Status:**
- Jedes neue Issue kommt auf das Board `TorroWorkplace`, Status beim Anlegen `Todo`.
- Der Status folgt der Phase (`Todo`, `Awaiting Approval`, `In Progress`, `In Review`, `Done`); die Skills setzen
  ihn.

**Branch-Namen:**
- Muster: `<typ>/<issue-nr>-<kurzer-slug>`, Typ aus dem Typ-Label: `feature/`, `bug/`, `task/`, `update/`, `docs/`.
- Der Slug ist klein, ASCII, Wörter mit `-` getrennt, zwei bis drei beschreibende Wörter.
- Beispiel: `feature/188-erinnerungen-vollbild`. `<cli> worktree create` erzeugt den Namen selbst, den Slug aus der
  Zeile `Kurzname:` am Ende der Issue-Beschreibung, und setzt den Branch auf `origin/main` auf.

**Issue-Kommentare bei Branch/PR-Erstellung:**
- Wenn ein Branch ohne PR erstellt wird, im Issue kommentieren:

  ```markdown
  Branch erstellt: `feature/188-erinnerungen-vollbild`
  ```

- Wenn ein PR erstellt wird, im Issue kommentieren:

  ```markdown
  PR erstellt: #<pr-nr>
  Branch: `feature/188-erinnerungen-vollbild`
  ```

**Commits:**
- Conventional Commits auf Englisch (`CLAUDE.md`), die Issue-Nummer in jeder einzelnen Commit-Message, z.B.
  `feat(torrocalendar): month view (K.10) #188`. Ausnahme: Direkt-PR ohne Issue.
- Mehrere unabhängige Änderungen nach Zweck in getrennte Commits.
- Nur bei Plänen vor #360 mit Aufgaben aus `docs/tasks/`: ein Commit pro Aufgabe, die Aufgaben-ID im Betreff.
- Nur die beabsichtigten Dateien stagen. Nie `.env`, Secrets, lokal erzeugte Artefakte oder fremde lokale
  Änderungen.

**Pull Requests:**
- PRs gehen gegen `main`.
- Der Titel bleibt unter 70 Zeichen und enthält `#<issue-nr>`, z.B. `feat(torrocalendar): reminders #188`.
- Der Body hat `## Summary`, `## Test plan` (Checkliste) und am Ende `Closes #<issue-nr>`; soll das Issue offen
  bleiben, `Related to #<issue-nr>`.
- Pushen und PR erstellen: in der Skill-Kette wie im Plugin (ohne Rückfrage, wenn alles sauber ist), sonst nur nach
  Rückfrage beim Nutzer. Kein Force-Push ohne ausdrückliche Anweisung, keine Hooks überspringen (`--no-verify`).
- KI-Agenten genehmigen und mergen nicht, auch nicht per Auto-Merge.
