# Issue-Workflow

Diese Datei enthält die **repo-spezifische Konfiguration** für den Issue-Workflow. `CLAUDE.md` und
`docs/git-workflow.md` verweisen hierher und enthalten nur die Kurzfassung. Die Skills sind dieselben wie im
Enon-Projekt und heißen gleich; nur die Werte hier unterscheiden sich.

Den Ablauf führen die Skills in `.claude/skills/`:

| Phase | Skill | Ort | Zustand danach |
|---|---|---|---|
| Anlegen | `issue-create` | Haupt-Checkout | Issue mit Typ-Label und `workflow`, Board-Status `Todo`; stuft die Komplexität ein (`Quick Fix`, `Klein`, `Normal`) und bietet bei `Quick Fix` `/quick-fix` statt eines Issues an, bei `Klein` `/issue-small <nr>`, bei `Normal` `/issue-plan <nr>` |
| Planen | `issue-plan` (Opus oder Fable) | beliebig; legt den Worktree an und liest den Code dort, in Herdr übergibt es an einen Tab `plan-<nr>` im Worktree | Plan-Kommentar, bei mehreren Aufgaben auch Einträge in `docs/tasks/` auf dem Branch, Label `awaiting-approval`, Board-Status `Awaiting Approval` |
| Freigeben | `issue-approve` (nur durch Menschen) | beliebig; setzt den Worktree aus `issue-plan` voraus | Label `approved`, Kommentar `Plan freigegeben: <url>`, bei eingeschalteter Übergabe in Herdr Tab `impl-<nr>` mit `issue-implement`, sonst Hinweis auf `issue-implement` |
| Umsetzen | `issue-implement` | Sitzung im Worktree | Aufgaben nach `torro-task`, Commits mit `#<nr>`, Review-Schleife, Board-Status `In Progress`, nach dem PR Label `pr-open` und Status `In Review` |
| Klein | `issue-small` | beliebig; legt Issue und Worktree selbst an, Umsetzung im Worktree | Issue, Label `started-implementation` statt `approved`, Commits mit `#<nr>`, Review-Schleife, Board-Status `In Progress`, nach dem PR Label `pr-open` und Status `In Review`; danach `pr-review` wie bei `issue-implement` |
| Direkt | `quick-fix` | beliebig; legt den Worktree selbst an, Umsetzung im Worktree | kein Issue, kein Board-Eintrag; Commit ohne Issue-Nummer, PR ohne `Closes` mit `awaiting-review`; danach `pr-review --light`; Details im Abschnitt "Direkt-PR ohne Issue" |
| Reviewen | `pr-review` (`--light` für Direkt-PRs; startet `issue-implement` nach dem Push selbst, als Agent ohne Kontext aus der Umsetzung) | beliebig | Review-Kommentar im PR, PR-Label `merge-ready` oder `changes-requested`, kein Approve, kein Merge |
| Triagieren | `pr-triage` (optional) | beliebig | Review-Funde außerhalb des PR-Scopes als eigene Issues |

`pr` ist ein Synonym für `pullrequest`. `diagnosing-bugs` ist keine Phase, sondern der Ablauf für hartnäckige
Fehler; er passt in die Umsetzung eines Bug-Issues.

Die Skills enthalten keine IDs und keine Repo-Namen, sondern lesen sie hier. Fehlt eine Angabe, fragen sie nach,
statt zu raten.

## Repository

- Repo: `mahype/TorroWorkplace`
- Basisbranch: `main`
- Branch-Muster: `<typ>/<issue-nr>-<kurzer-slug>`, Regex für die Issue-Nummer `^[a-z]+/(\d+)-`. Ausnahme Direkt-PR (`quick-fix`): `<typ>/<kurzer-slug>` ohne Nummer, `<typ>` ist der Conventional-Commit-Typ (`docs`, `fix`, `chore`), der Slug beginnt mit einem Buchstaben, damit die Regex ihn nicht als Issue-Nummer liest (`docs/2026-roadmap` wäre Issue #2026)
- Issue-URL-Basis: `https://github.com/mahype/TorroWorkplace/issues`

## Prüfen

Die Prüfbefehle für jede Verifikation (Plan, Umsetzung, Review):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
TORRO_LANG=de cargo test --workspace
```

Vor Push und PR läuft zusätzlich `scripts/ci-local.sh` (dieselben Schritte wie `ci.yml`; GitHub Actions läuft nur
bei Releases). Bei Oberflächenänderungen gehört die Sichtprüfung dazu (Skill `torro-design`, Agent
`torro-design-review`), bei Änderungen an Requests, Katalog oder MCP der Agent `torro-api-review`.

## Dubletten-Status

Findet die Duplikatsuche bestehende Issues, zeigt die Vorschau deren Status. Status und Grund kommen aus der Suche
selbst:

```bash
gh issue list --repo mahype/TorroWorkplace --state all --search "<stichworte>" --limit 10 --json number,state,stateReason,title
```

Nur für offene Treffer zusätzlich die per `Closes` verknüpften PRs:

```bash
gh api graphql -f query='query($n:Int!){repository(owner:"mahype",name:"TorroWorkplace"){issue(number:$n){closedByPullRequestsReferences(first:5){nodes{number state}}}}}' -F n=<nr>
```

Die Liste enthält auch gemergte und geschlossene PRs; deshalb nur Knoten mit `state` `OPEN` zählen.

| Issue | Grund | Verknüpfter PR | Status in der Vorschau |
|---|---|---|---|
| `OPEN` | leer oder `REOPENED` | kein Knoten mit `state` `OPEN` | offen |
| `OPEN` | leer oder `REOPENED` | Knoten mit `state` `OPEN` | PR offen #<pr>, bei mehreren alle |
| `CLOSED` | `COMPLETED` | – | erledigt |
| `CLOSED` | `NOT_PLANNED` oder `DUPLICATE` | – | verworfen |

Auch `docs/tasks/*.md` durchsuchen: Eine Aufgabe, die dort schon steht (offen oder abgehakt), ist ebenfalls ein
Treffer.

## Issue-Typen

Das Repo gehört einem Nutzer, nicht einer Organisation; GitHub-Issue-Typen gibt es dort nicht. Der Typ ist deshalb
ein **Label**, genau eines je Issue. `scripts/worktree.sh` leitet daraus den Branch-Präfix ab; ohne Typ-Label
fragt es nach und nimmt mit `--yes` `feature/`.

| Typ | Label | Branch-Präfix |
|---|---|---|
| Feature | `feature` (älter: `enhancement`) | `feature/` |
| Bug | `bug` | `bug/` |
| Task | `task` | `task/` |
| Update | `update` | `update/` |
| Docs | `docs` (älter: `documentation`) | `docs/` |

Dazu kommen die Bereichs-Labels des Repos (`core`, `gui`, `app:calendar`, `app:inbox`, `caldav`, `ai` …), so viele
wie passen. `ai` heißt hier „Arbeiten mit KI-Agenten: MCP, Agenten-Übersicht“ und ist ein Bereich, kein Hinweis auf
den Workflow.

## Labels

| Label | Bedeutung |
|---|---|
| `workflow` | Issue läuft durch die Skills (`issue-create` bis `pr-review`); entspricht `en-workflow` in Enon |
| `ai-tooling` | Issue betrifft das Tooling selbst: Skills, Skripte, Hooks, Worktrees, `CLAUDE.md` (von Hand gesetzt) |
| `awaiting-approval` | Plan wartet auf Freigabe (`issue-plan`) |
| `approved` | Plan freigegeben (`issue-approve`); Gate für `issue-implement` |
| `started-implementation` | Umsetzung läuft (`issue-implement`, `issue-small`) |
| `pr-open` | PR erstellt, wartet auf Review (`pullrequest`, auch aus `issue-implement` heraus) |
| `strong-model-recommended` | Umsetzung mit Opus empfohlen (`issue-plan`) |

PR-Labels, am PR selbst und unabhängig von den Issue-Labels. Es ist immer höchstens eines gesetzt und zeigt den
Review-Stand:

| Label | Bedeutung |
|---|---|
| `awaiting-review` | PR wartet auf Review (`pullrequest`) |
| `merge-ready` | Review ohne Blocker, wartet auf die Merge-Entscheidung eines Menschen (`pr-review`) |
| `changes-requested` | Review verlangt Überarbeitung (`pr-review`) |

`pr-review` entfernt vor dem Setzen immer alle drei, ein erneutes Review ersetzt also das alte Fazit. Ein Push setzt
das Label nicht zurück, erst das nächste Review. `merge-ready` heißt nicht "freigegeben": Approve und Merge bleiben
bei Menschen.

Anders als in Enon räumt keine GitHub Action die Zwischenstand-Labels nach dem Schließen eines PRs auf: Actions
laufen hier nur bei Releases, jede Minute kostet Geld. Nach einem Merge schließt `Closes #<nr>` das Issue, die Labels
`pr-open` und `merge-ready` bleiben an geschlossenen Einträgen stehen und stören nicht. Wird ein PR ohne Merge
geschlossen, ersetzen `issue-implement` bzw. `issue-small` `pr-open` beim nächsten Lauf durch
`started-implementation`; ein neuer Plan (`issue-plan`) entfernt alle Workflow-Labels.

Der Workflow trägt Assignees ein, und zwar das lokal angemeldete `gh`-Konto (`@me`). Er ergänzt nur
(`--add-assignee`) und entfernt nie einen.

| Skill | Assignee |
|---|---|
| `issue-plan` | der Planende am Issue |
| `pullrequest` | der Autor am PR, auch wenn `issue-implement` den PR darüber erstellt |
| `pr-review` | der Reviewer am PR, nachdem das Ergebnis gepostet ist |

Das Label `approved` setzt nur ein Mensch über `issue-approve`, nie ein Agent von sich aus. Ersetzt `issue-plan`
einen Plan, entfernt es `approved`, `started-implementation` und `pr-open` wieder.

## Projekt-Board

- Projekt: `TorroWorkplace`, Nummer `2`, Owner `mahype` (https://github.com/users/mahype/projects/2)
- Project-ID: `PVT_kwHOAAogkc4BnVbb`
- Status-Feld: `PVTSSF_lAHOAAogkc4BnVbbzhl6iAM`

| Status | Option-ID | Gesetzt von |
|---|---|---|
| Todo | `b915817a` | `issue-create`, `issue-small`, `pr-triage` |
| Awaiting Approval | `ace82355` | `issue-plan` |
| In Progress | `3729d683` | `issue-implement`, `issue-small` |
| In Review | `6d849e15` | `pullrequest` |
| Done | `e88920ec` | Projekt-Workflow "Item closed", kein Skill |

Das Board ist nur die Übersicht. Der Status folgt der Phase, ein Mensch bedient ihn nicht von Hand. `issue-approve`
und `pr-review` setzen keinen Status: Die Freigabe zeigt das Label `approved`, das Review-Ergebnis die PR-Labels.
`In Review` gilt, bis der PR gemergt und das Issue geschlossen ist.

```bash
ITEM=$(gh project item-add 2 --owner mahype --url <issue-url> --format json -q .id)
gh project item-edit --project-id PVT_kwHOAAogkc4BnVbb --id "$ITEM" --field-id PVTSSF_lAHOAAogkc4BnVbbzhl6iAM --single-select-option-id <option-id>
```

`item-add` liefert auch für ein bereits zugeordnetes Issue die vorhandene Item-ID.

## Plan und `docs/tasks/`

In Torro arbeitet ein Agent immer genau eine Aufgabe aus `docs/tasks/` ab (`CLAUDE.md`, Skill `torro-task`). Der
Issue-Workflow baut darauf auf:

- **Ein Schritt:** Passt die Umsetzung in eine Aufgabe, ist der Agententeil des Plan-Kommentars die Aufgabe. Er
  nennt Modell, Dateien und Abnahme wie ein Eintrag in `docs/tasks/`.
- **Mehrere Schritte:** `issue-plan` schreibt die Aufgaben in `docs/tasks/<thema>.md` (neue Datei oder Abschnitt,
  IDs nach dem Schema der Datei), bei neuen Konzepten auch `docs/<thema>.md`, und committet das auf dem Branch des
  Issues (`docs(<bereich>): plan #<nr>`). Der Plan-Kommentar verweist auf die Datei beim Plan-SHA und listet die
  Aufgaben-IDs. Mit der Freigabe sind auch diese Aufgaben freigegeben.
- `issue-implement` arbeitet die Aufgaben in Reihenfolge nach `torro-task` ab, ein Commit pro Aufgabe, jeder mit
  `#<nr>`.
- `issue-small` und `quick-fix` brauchen keinen Eintrag in `docs/tasks/`: Dort ist die Issue-Beschreibung bzw. die
  Kurzbeschreibung die Aufgabe.
- Neue Funktionen nennen im Plan Oberfläche, API-Befehl und MCP-Tool oder begründen, warum ein Weg fehlt
  (`CLAUDE.md`, Skill `torro-api`). Request und Katalogeintrag kommen in die erste Aufgabe.

## Kleine Änderungen (`issue-small`)

`issue-small` ist eine bewusste Ausnahme vom Gate `approved`, nur für Änderungen ohne harte Ausschlüsse:

- keine Änderung an festgelegten Schnittstellen (`torro_ipc::Request`, Befehlskatalog, Traits, öffentliche
  Signaturen), keine neue Funktion, die alle drei Wege (Oberfläche, API, MCP) braucht
- nichts an Schlüsseln, Rechten, Freigaben oder Protokoll
- kein Format gespeicherter Daten, keine Migration
- kein nativer Plattformcode (`torro-appkit`), keine neue Abhängigkeit
- keine Änderung an den Gates der Skill-Kette (`approved`, `awaiting-approval`, Freigabe-Skills)

Als Richtwert gelten bis zu 5 Dateien oder etwa 50 geänderte Zeilen; mehr ist nur eine Warnung mit dem Vorschlag
`/issue-plan`, kein Abbruch. Die Issue-Beschreibung ist der Plan, die Bestätigung der Issue-Vorschau durch den
Menschen ersetzt die Freigabe. Der Skill setzt `approved` nie, sondern direkt `started-implementation`; Issues mit
Plan-Kommentar oder `awaiting-approval` nimmt er nicht an. Den PR mergt ein Mensch selbst. Icon in Sitzungstitel
und Herdr: 🎯.

## Direkt-PR ohne Issue (`quick-fix`)

`quick-fix` ist die zweite bewusste Ausnahme vom Ablauf "erst Issue", für sehr kleine Änderungen, bei denen ein
Issue nur Lärm im Board wäre. Kriterium "Direkt-PR" (alle Punkte): höchstens 2–3 Dateien und wenige Zeilen; erlaubt
sind Text, Doku, Kommentare, Übersetzungen in `crates/torro-core/src/i18n/de.rs` und kleine Korrekturen in
`scripts/` und `.claude/skills/`; nicht erlaubt sind Programmlogik, Schnittstellen, Konfiguration, Abhängigkeiten
und Änderungen an den Gates der Skill-Kette (`approved`, Merge-Regeln). Bei Zweifeln hält der Agent an und schlägt
`/issue-small` vor.

- Worktree per `git worktree add .worktrees/<typ>-<slug> -b <typ>/<slug> --no-track origin/main` aus dem
  Haupt-Checkout (`worktree.sh` kennt nur Issue-Nummern). Aufräumen nach dem Merge:
  `git worktree remove .worktrees/<typ>-<slug>` und `git branch -d <typ>/<slug>`.
- Kein Issue, kein Board-Eintrag, keine Issue-Labels. Der PR trägt `awaiting-review`, danach `merge-ready` oder
  `changes-requested` aus `pr-review --light`.
- Commit im Conventional-Commit-Stil ohne Issue-Nummer, PR-Body kurz ohne `Closes`. Pushen und PR erst nach
  Rückfrage, Review und Merge durch Menschen.
- Icon in Sitzungstitel und Herdr: ⚡ `Quick-Fix`; der Skill ruft `node scripts/agent/skill-phase.mjs herdr - quick`
  auf (`-` statt der Nummer).

## Worktrees

- Worktree anlegen: `./scripts/worktree.sh create <nr> --yes` (ohne `--yes` fragt das Skript nach dem Branch)
- Übersicht: `./scripts/worktree.sh list`
- Nach dem Merge aufräumen: `./scripts/worktree.sh prune <nr>` aus dem Haupt-Checkout (nur gemergter PR, keine
  eigenen Änderungen)
- Pfad: `.worktrees/torro-<nr>-<slug>` im Haupt-Checkout (gitignored). Die Worktrees der Subagenten unter
  `.claude/worktrees/` sind davon unabhängig.
- Jeder Worktree hat ein eigenes `target/`; der erste `cargo build` dort dauert entsprechend.

## Herdr

Läuft die Sitzung in Herdr (`HERDR_ENV=1`), startet jede Phase ab der Planung in einem eigenen Tab im
Herdr-Workspace des Worktrees:

```bash
./scripts/worktree.sh agent <nr|worktree-pfad> <name> "<prompt>" [--model <modell>]
```

Der Befehl öffnet den Tab, startet dort Claude Code und schickt den Prompt. `issue-plan` nutzt ihn aus dem
Haupt-Checkout heraus (`plan-<nr>`, immer mit Opus), `issue-approve` nach der Freigabe (`impl-<nr>`),
`issue-small` (`small-<nr>`, mit Sonnet) und `quick-fix` (`quick-<slug>`, mit Sonnet, Pfad statt Nummer). Außerhalb
von Herdr endet der Befehl mit Exit 3, und die Skills nennen den nächsten Befehl für eine neue Sitzung im Worktree.

Die Übergabe an einen neuen Tab ist für die Planung standardmäßig an und für die Umsetzung aus. Ist sie für eine
Phase aus, öffnet `agent` nichts und endet mit Exit 4. Umschalten in `scripts/worktree.local.env` im Haupt-Checkout
(gitignored, je Zeile `KEY=wert`):

```
HERDR_HANDOFF_PLAN=false
HERDR_HANDOFF_IMPL=true
HERDR_FOCUS_IMPL=false
HERDR_FOCUS_PLAN=true
```

Modell für die Umsetzung: `issue-approve` übergibt immer `--model`, `opus` bei `strong-model-recommended`, sonst
`sonnet`. `fable` nur auf ausdrückliche Anweisung des Nutzers. Läuft schon ein Agent gleichen Namens, startet der
Befehl keinen zweiten: Wartet er untätig, bekommt er nur den Prompt, sonst fokussiert der Befehl den Tab und endet
mit Exit 1.

### Phase im Sitzungstitel und in Herdr

Icon, Anzeigename und Status-Werte der Phasen stehen nur in `scripts/agent/skill-phase.mjs`.

- **Sitzungstitel:** Der `UserPromptSubmit`-Hook in `.claude/settings.json` erkennt `/issue-plan <nr>`,
  `/issue-approve <nr>`, `/issue-implement <nr>` und `/issue-small <nr>` am Anfang eines Prompts und setzt den
  Titel, z.B. `📋 Planning #188`; `/quick-fix` setzt `⚡ Quick-Fix`.
- **Herdr-Pane und Workspace-Status:** Die Skills rufen `node scripts/agent/skill-phase.mjs herdr <nr> <phase>
  [<status>]` auf. Ohne Herdr tut der Befehl nichts.

| Zeitpunkt | Status |
|---|---|
| `issue-plan` startet | `planning` |
| Plan gepostet | `awaiting approval` |
| `issue-approve` fertig | `approved` |
| `issue-implement` startet | `implementing` |
| PR erstellt (`pullrequest`) | `in review` |

### Issue-Kontext

Ein `SessionStart`-Hook (`scripts/agent/issue-context.mjs`) liest die Issue-Nummer aus dem Branch, holt das Issue
per `gh` und gibt es der Sitzung als Kontext. Auf `main` und auf Branches ohne Nummer schweigt er; ein Fehler
blockiert die Sitzung nie.

## Abschlussblock

`issue-plan`, `issue-approve`, `issue-implement`, `issue-small` und `quick-fix` enden mit demselben Block als
letzter Ausgabe der Sitzung (kommt danach noch eine Agenten-Benachrichtigung, wird er nach deren Zusammenfassung
wiederholt):

```
Phase: <Phase>
#<nr> – <Titel>
Issue: <issue-url>
PR: <pr-url>
```

- `Phase` ist der Wert der Spalte "Phase" aus der Tabelle am Anfang dieser Datei.
- Die Zeile `PR:` nur, wenn es zum Issue einen PR gibt.
- Bei `Direkt` (`quick-fix`) entfallen die Zeilen `#<nr> – <Titel>` und `Issue:`; stattdessen steht eine Zeile mit
  dem Branch.

## Verwandte Dokumentation

- `docs/git-workflow.md` – Branches, Issues, Commits, PRs
- `CLAUDE.md` – Regeln für Code, Prüfen und die drei Wege Oberfläche, API, MCP
