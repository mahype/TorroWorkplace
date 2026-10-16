---
name: pullrequest
description: Erstellt einen Pull Request des aktuellen Branches gegen main, nach lokaler CI (scripts/ci-local.sh) und den Tests auf Deutsch
argument-hint: Optionaler PR-Titel
---

# Pull Request gegen `main` erstellen

Du erstellst für den aktuell ausgecheckten Branch einen Pull Request gegen den Branch `main` im Repo aus `docs/issue-workflow.md` (`mahype/TorroWorkplace`). Folge den Schritten genau in dieser Reihenfolge. `pr` ist ein Synonym für diesen Skill.

Optionaler vom Nutzer übergebener Titel: $ARGUMENTS

## 1. Status-Erhebung (parallel)

Führe die folgenden Befehle in **einem** Bash-Block aus, um den Branch-Zustand zu verstehen:

- `git branch --show-current` — aktueller Branch
- `git status` — Working-Tree-Zustand (NIEMALS `-uall` verwenden)
- `git log origin/main..HEAD --oneline` — Commits, die in den PR fließen
- `git diff origin/main...HEAD --stat` — Übersicht der Änderungen
- `git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || echo "no-upstream"` — Upstream-Status
- `gh pr list --head "$(git branch --show-current)" --base main --state open --json url,number` — existiert bereits ein PR?

## 2. Vorbedingungen prüfen

Brich ab und melde dem Nutzer, wenn:

- Der aktuelle Branch `main` ist → "Du bist auf `main`. Wechsle auf einen Feature-Branch."
- Keine Commits gegenüber `main` existieren → "Keine Änderungen gegenüber `main` vorhanden."
- Uncommitted Changes vorhanden sind → frage den Nutzer, ob sie vorher committet werden sollen. Committe NICHT ungefragt.
- Bereits ein offener PR existiert → gib die vorhandene PR-URL aus und beende den Skill. Erstelle KEINEN doppelten PR. Trägt das Issue noch `started-implementation`, vorher durch `pr-open` ersetzen (Befehl in Schritt 8).
- `origin/main` nicht im Branch enthalten ist (`git fetch origin main`, dann `git merge-base --is-ancestor origin/main HEAD`) → aktualisiere ihn: Ist der Branch noch nicht auf origin (`@{u}` aus Schritt 1 ist `no-upstream`), `git rebase origin/main`, sonst `git merge origin/main` (kein Force-Push). Konflikte löst du mit dem Nutzer, nicht eigenmächtig.

Lief in dieser Sitzung keine Review-Schleife aus `issue-implement` (Schritt 8 dort), biete vor dem Push ein Review durch einen Agenten mit frischem Kontext an, wie dort beschrieben. Das entfällt, wenn der Skill aus `quick-fix` heraus läuft (Direkt-Modus): dort folgt nach dem PR das leichte Review (`pr-review --light`).

## 3. Lokale CI

GitHub Actions läuft nur bei Releases; die Prüfung vor dem PR läuft hier:

```bash
scripts/ci-local.sh
TORRO_LANG=de cargo test --workspace
```

Beides muss grün sein. Schlägt etwas fehl, abbrechen und den Fehler mit Ausgabe melden; nichts pushen. Im Direkt-Modus (`quick-fix`) reicht `scripts/ci-local.sh --quick`, wenn nur Doku, Skills oder Skripte geändert sind; bei `de.rs` zusätzlich `TORRO_LANG=de cargo test -p torro-core`.

## 4. Issue-Nummer aus Branch-Namen ableiten

Branch-Konvention laut `docs/git-workflow.md`: `<typ>/<ISSUENUMBER>-<slug>` (z. B. `bug/168-icon-dimmen`, `feature/188-erinnerungen-vollbild`). Ein Direkt-PR aus `quick-fix` trägt stattdessen `<typ>/<slug>` ohne Nummer.

- Extrahiere die Nummer per Regex `^[a-z]+/(\d+)-`.
- Wenn keine Issue-Nummer ableitbar ist: Läuft der Skill aus `quick-fix` (**Direkt-Modus**), nicht fragen, es ist ein Direkt-PR ohne Issue. Sonst frage den Nutzer "Issue-Nummer oder Direkt-PR ohne Issue?". Ein Direkt-PR ohne `quick-fix` ist nur zulässig, wenn die Änderung das Kriterium aus `docs/issue-workflow.md` ("Direkt-PR ohne Issue") erfüllt.

## 5. PR-Titel bestimmen

- Wenn `$ARGUMENTS` nicht leer ist, nutze diesen als Titel.
- Sonst leite den Titel aus den Commit-Messages seit Divergenz von `main` ab, im Conventional-Commit-Stil auf Englisch (`feat(scope):`, `fix(scope):`, `docs:` …).
- Halte den Titel unter 70 Zeichen.
- Hänge `#<ISSUENUMBER>` an den Titel an, wenn er nicht bereits enthalten ist. Bei einem Direkt-PR ohne Issue entfällt das.

## 6. PR-Body bauen

Analysiere **alle** Commits zwischen `origin/main` und `HEAD` (nicht nur den letzten) sowie `git diff origin/main...HEAD`, um den Body zu schreiben. Format:

```
## Summary
- <1–3 Bullets, was sich inhaltlich ändert und warum>

## Test plan
- [x] scripts/ci-local.sh
- [x] TORRO_LANG=de cargo test --workspace
- [ ] <konkreter Schritt, der von Hand zu prüfen ist>

Closes #<ISSUENUMBER>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

Wenn keine Issue-Nummer ableitbar war und der Nutzer keine geliefert hat, lasse die `Closes`-Zeile weg. Bei einem Direkt-PR ohne Issue ist der Body kurz: `## Summary` mit einem Bullet und `## Test plan` nur, wenn etwas von Hand zu prüfen ist; keine `Closes`-Zeile.

Bleiben Review-Funde bewusst unbehoben (z.B. aus der Review-Schleife von `issue-implement`), steht vor der `Closes`-Zeile ein Abschnitt `## Bewusst offen` mit je einem Punkt und Begründung. Neue Nutzertexte ohne Eintrag in `de.rs` stehen unter `## Übersetzungen` mit Vorschlag (`"Today" → "Heute"`). Sonst entfallen beide.

## 7. Branch pushen und PR erstellen

- Ohne Upstream (Schritt 1 lieferte `no-upstream`): `git push -u origin <branch>`, sonst `git push`. Der `pre-push`-Hook prüft erneut; keine Hooks überspringen (`--no-verify`).
- Erzwinge NIE einen Push (`--force`, `--force-with-lease`) ohne explizite Anweisung des Nutzers.

```bash
gh pr create --base main --head "<branch>" --title "<title>" --body-file <datei>
```

Den Body vorher als Datei im Scratchpad schreiben, damit Umlaute und Backticks unversehrt bleiben.

## 8. Ergebnis ausgeben

Gib die von `gh pr create` zurückgegebene PR-URL aus.

Bei einem Direkt-PR ohne Issue entfallen Issue-Kommentar, Issue-Labels, Board-Status und Herdr-Status; es bleiben der Assignee und `awaiting-review`.

Bei abgeleiteter Issue-Nummer außerdem:

- Im Issue kommentieren, wie in `docs/git-workflow.md` ("Issue-Kommentare bei Branch/PR-Erstellung") verlangt: `PR erstellt: #<pr-nr>` und `Branch: \`<branch>\`` (`gh issue comment <nr> --repo <repo> --body-file <datei>`).
- Trägt das Issue das Label `started-implementation`, es durch `pr-open` ersetzen: `gh issue edit <nr> --repo <repo> --remove-label started-implementation --add-label pr-open`.
- Projekt-Status des Issues auf `In Review` setzen (Befehle und IDs in `docs/issue-workflow.md`).
- In Herdr den Status des Worktrees setzen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr <nr> implement in-review`.

Bei jedem PR: `gh pr edit <pr-nr> --repo <repo> --add-label awaiting-review --add-assignee @me`. Vorhandene Assignees bleiben; das Review-Label setzt danach `pr-review`.

## Wichtig

- Keine destruktiven Git-Befehle (`reset --hard`, `push --force`, `branch -D`) ohne ausdrückliche Anweisung.
- Keine Hooks überspringen (`--no-verify`).
- Niemals `git config` verändern.
- Niemals ungefragt committen.
- KI-Agenten approven und mergen nie.
