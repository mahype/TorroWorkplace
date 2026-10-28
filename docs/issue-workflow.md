# Issue-Workflow

Den Issue-Workflow führen die Skills des Plugins **enon-tools** (Marketplace `enon-ai-tooling`, eingeschaltet in
`.claude/settings.json`), dieselben wie im Enon-Projekt. Ablauf, Phasen, Labels, Board-Status, Worktrees, Herdr und
Abschlussblock beschreibt die Referenz des Plugins (`references/workflow.md` im Plugin-Ordner); die Werte des Repos
(Board, Typen) stehen in `.enon-tools/konventionen.json` und kommen über `<cli> konventionen`.

Diese Datei enthält nur die **Regeln des Repos**: was in TorroWorkplace anders ist oder dazukommt. Die Skills des
Plugins lesen `CLAUDE.md` und die dort verlinkte Doku, also diese Datei. `<cli>` steht für
`node <Plugin-Ordner>/bin/enon-tools.mjs`.

| Phase | Skill | Ergänzung in Torro |
|---|---|---|
| Anlegen | `issue-create` | `docs/tasks/` mit durchsuchen, drei Wege bei Funktionen |
| Planen | `issue-plan` (Opus oder Fable) | `torro-api`/`torro-design`, Block „Oberfläche, API, MCP“, `Oberfläche: ja\|nein`, keine Einträge in `docs/tasks/` |
| Freigeben | `issue-approve` (nur durch Menschen) | keine lokale Instanz |
| Umsetzen | `issue-implement` | Regeln aus `CLAUDE.md`, `de.rs`, `torro-design-review` und `torro-api-review` |
| Klein | `issue-small` | wie Umsetzen, Grenzen im Abschnitt „Kleine Änderungen“ |
| Direkt | `quick-fix` | Kriterium im Abschnitt „Direkt-PR ohne Issue“ |
| PR | `pullrequest` (Synonym `pr`) | lokale CI und Tests auf Deutsch, Übersetzungen im Body |
| Reviewen | `pr-review` | Blocker und Anmerkungen im Abschnitt „Ergänzungen je Skill“ |
| Triagieren | `pr-triage` (optional) | – |
| Aufräumen | `worktree-cleanup` | – |

KI-Agenten setzen nie `approved`, approven und mergen nie.

## Skills im Repo

Unter `.claude/skills/` liegen nur Skills, die es im Plugin nicht gibt: `torro-api`, `torro-design`, `torro-task`,
`nextcloud`, `diagnosing-bugs` und das Synonym `pr`, das auf `enon-tools:pullrequest` zeigt. Ein Skill im Repo mit
dem Namen eines Plugin-Skills verdeckt diesen; deshalb gibt es dort keine Kopien der Workflow-Skills. Ideen und
Fehler zu den Plugin-Skills gehen mit `/skill-feedback` an das Plugin, nicht in eine eigene Kopie.

Einrichtung je Rechner: `/setup` im Haupt-Checkout auf `main` (Statusline, Herdr, Prüfung der Repo-Dateien). Board
und Typen sind in `.enon-tools/konventionen.json` festgelegt und committet; `/setup` legt sie nicht neu fest.

## Issues gehen vor, `docs/tasks/` ist Bestandsaufnahme

- `docs/tasks/` ist eine Bestandsaufnahme dessen, was wir gern machen möchten. Daraus entstehen bei Bedarf Issues.
  Gibt es ein Issue, gilt das Issue mit seinem freigegebenen Plan, nicht der Eintrag.
- Pläne schreiben keine Aufgaben nach `docs/tasks/`. Der Agententeil des Plan-Kommentars ist die Arbeitsanweisung,
  auch bei mehreren Schritten.
- Ein Issue aus einem Eintrag nennt ihn als Quelle (Datei und Aufgaben-ID). Der Eintrag bleibt unverändert.
- **Pläne vor #360** mit einem Abschnitt `### Aufgaben`: Die Aufgaben der genannten Datei beim Plan-SHA sind die
  Schritte. Jede läuft nach dem Skill `torro-task` (Tests zuerst, umsetzen, prüfen, `- [x]` abhaken), ein Commit je
  Aufgabe mit Aufgaben-ID und `#<nr>`. Weicht die Datei vom Plan-SHA ab, das melden und klären.

## Typen als Labels

Das Repo gehört einem Nutzerkonto; GitHub-Issue-Typen gibt es dort nicht. Der Typ ist deshalb ein **Label**, genau
eines je Issue. In `konventionen.json` stehen die Typen ohne `id`; das Plugin setzt den Typnamen in Kleinbuchstaben
als Label und liest daraus den Branch-Präfix.

| Typ | Label | Branch-Präfix |
|---|---|---|
| Feature | `feature` (älter: `enhancement`) | `feature/` |
| Bug | `bug` | `bug/` |
| Task | `task` | `task/` |
| Update | `update` | `update/` |
| Docs | `docs` (älter: `documentation`) | `docs/` |

Dazu kommen die Bereichs-Labels des Repos (`core`, `gui`, `app:calendar`, `app:inbox`, `caldav`, `ai` …), so viele
wie passen. `ai` heißt hier „Arbeiten mit KI-Agenten: MCP, Agenten-Übersicht“ und ist ein Bereich, kein Hinweis auf
den Workflow. Die Workflow-Labels (`en-workflow`, `aufwand:klein`, `aufwand:normal`, Phasen- und PR-Labels) sind die
des Plugins; `en-workflow` hieß hier früher `workflow`.

## Prüfen

Teststrategie für jede Verifikation (Plan, Umsetzung, Review):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
TORRO_LANG=de cargo test --workspace
```

- Vor Push und PR läuft zusätzlich `scripts/ci-local.sh` (dieselben Schritte wie `ci.yml`). Beides muss grün sein;
  schlägt etwas fehl, wird nicht gepusht.
- Sind nur Doku, Skills oder Skripte geändert, reicht `scripts/ci-local.sh --quick`; bei `de.rs` zusätzlich
  `TORRO_LANG=de cargo test -p torro-core`.
- Tests brauchen keinen laufenden Hyprland, kein Netz und kein echtes Home-Verzeichnis (`CLAUDE.md`).
- Bei Oberflächenänderungen gehört die Sichtprüfung dazu (Skill `torro-design`, Agent `torro-design-review`), bei
  Änderungen an Requests, Katalog oder MCP der Agent `torro-api-review`.
- GitHub Actions läuft nur bei Releases, jede Minute kostet Geld. Einzige Ausnahme ist die Action „Phasen-Labels“
  des Plugins (`.github/workflows/enon-phasen-labels.yml`): Sie läuft beim Schließen eines Issues einige Sekunden
  und entfernt dessen Phasen-Labels. Sie ist eine Vorlage des Plugins; `/setup` hält sie aktuell.

## Ergänzungen je Skill

**`issue-create`**

- Zusätzlich `docs/tasks/*.md` nach den Stichworten durchsuchen. Eine passende Aufgabe (offen oder abgehakt) ist ein
  Treffer wie ein Issue; ein Issue daraus nennt sie als Quelle, der Eintrag bleibt unverändert.
- Bei einer Funktion, die Menschen in der Oberfläche nutzen: Gibt es sie auch über API und MCP, oder warum fehlt ein
  Weg (`CLAUDE.md`, „Jede Funktion dreimal“)? Fehlt die Antwort, gehört sie unter die offenen Punkte.
- Wo es passt, steht in der Beschreibung ein Abschnitt „Für Menschen“ vorn, der ohne Pfade erklärt, was sich ändert.

**`issue-plan`**

- Regeln aus `CLAUDE.md` beachten: festgelegte Schnittstellen, Plattformcode hinter `cfg`, Abhängigkeiten nur über
  `[workspace.dependencies]`, Nutzertexte englisch mit `tr()`, kein `unsafe` außerhalb von `torro-appkit`.
- Passende Konzepte in `docs/` lesen; der Plan baut darauf auf und widerspricht ihnen nicht stillschweigend.
- Neue oder geänderte Funktion: Skill `torro-api` laden. Der Überblick bekommt nach den Entscheidungen einen Block
  **Oberfläche, API, MCP** (`- Oberfläche: …; API: Request::…; MCP: bereich_tool`, oder warum ein Weg fehlt).
  Request und Katalogeintrag kommen in den ersten Schritt. Danach den Agenten `torro-api-review` auf den Plan
  ansetzen (fehlt ein Weg, verletzt der Plan eine Sicherheitsregel?) und berechtigte Funde einarbeiten.
- Oberfläche betroffen: Skill `torro-design` laden; Entwürfe unter `docs/prototypes/` sind Maßstab.
- Im Agententeil steht neben `Instanz: nein` wörtlich `Oberfläche: ja|nein`; bei `ja` läuft in der Umsetzung die
  Sichtprüfung.
- Keine Einträge in `docs/tasks/` und kein Plan-Commit dorthin (Abschnitt „Issues gehen vor“).

**`issue-implement` und `issue-small`**

- Umsetzen nach den Regeln aus `CLAUDE.md` und dem Ablauf von `torro-task` (Tests zuerst, Nutzertexte mit `tr()`,
  kein `unwrap`, nur die genannten Dateien). Request angefasst: Skill `torro-api` laden; Oberfläche angefasst:
  Skill `torro-design` laden.
- Neue Nutzertexte: deutsche Übersetzung in `crates/torro-core/src/i18n/de.rs`. Laufen Agenten parallel, sammeln
  und am Ende in einem eigenen Commit eintragen.
- Verifizieren mit den Befehlen aus „Prüfen“. Bei `Oberfläche: ja` den Agenten `torro-design-review` auf die
  geänderten Ansichten ansetzen, bei Request, Katalog oder MCP den Agenten `torro-api-review`; berechtigte Funde
  beheben.
- Der Review-Agent vor dem Push prüft zusätzlich gegen `CLAUDE.md`: geänderte festgelegte Schnittstellen,
  `unwrap`/`todo!`/`dbg!`, `unsafe` außerhalb `torro-appkit`, Nutzertexte ohne `tr()`, Abhängigkeiten außerhalb
  `[workspace.dependencies]`, Plattformcode ohne `cfg`, Geheimnisse, die den Dienst verlassen, und fehlende Wege bei
  neuen Funktionen (Oberfläche, Request mit Katalogeintrag, MCP-Tool).
- Aufgaben mit „Modell: Haiku“ aus Plänen vor #360 können an Agenten `torro-coder-haiku` gehen, anspruchsvolle an
  `torro-coder-opus`, je ein Agent pro Aufgabe; Aufgaben, die den Katalog ändern, nie parallel.

**`pullrequest`**

- Vor dem Push `scripts/ci-local.sh` und `TORRO_LANG=de cargo test --workspace` (Abschnitt „Prüfen“); beide stehen
  abgehakt im `## Test plan`.
- Neue Nutzertexte ohne Eintrag in `de.rs` stehen im Body unter `## Übersetzungen` mit Vorschlag
  (`"Today" → "Heute"`).

**`pr-review`**

Maßstab sind die Regeln aus `CLAUDE.md`. Zusätzlich zu den Blockern des Plugins:

- Blocker: Panics (`unwrap`, Indexzugriffe, `expect` außerhalb von Tests); Geheimnisse, die den Dienst verlassen,
  Agenten, die Schlüssel, Rechte oder Freigaben verwalten können, `untrusted`-Inhalte, die als vertrauenswürdig
  behandelt werden; stillschweigend geänderte festgelegte Schnittstellen (`torro_ipc::Request`, Katalog, Traits,
  Signaturen) oder Datenformate; neue Funktion ohne alle drei Wege, ohne dass Issue oder Plan den fehlenden Weg
  begründen; `Never` im Katalog für etwas, das nicht auf der Liste „Was nie nach außen geht“ in `docs/api.md` steht;
  `unsafe` außerhalb von `crates/torro-appkit`, Plattformcode ohne `#[cfg(target_os = …)]`.
- Anmerkungen: Nutzertexte ohne `tr()`/`trf()`, fehlende deutsche Übersetzung, Tests, die Oberflächentexte fest statt
  mit `tr("…")` vergleichen; Tests, die Netz, laufendes Hyprland oder das echte Home-Verzeichnis brauchen;
  Abhängigkeiten außerhalb von `[workspace.dependencies]`; fehlender `//!`-Doc-Kommentar in neuen Dateien;
  `Result<_, String>` statt eines eigenen Fehler-Enums.
- Ändert der PR Requests, Katalog oder MCP-Tools, den Agenten `torro-api-review` ansetzen und seine berechtigten
  Funde übernehmen. Ändert er Oberflächen, im Review vermerken, ob eine Sichtprüfung (`torro-design-review`) im PR
  belegt ist.

## Kleine Änderungen (`issue-small`)

Harte Ausschlüsse (dann `/issue-plan`):

- keine Änderung an festgelegten Schnittstellen (`torro_ipc::Request`, Befehlskatalog, Traits, öffentliche
  Signaturen), keine neue Funktion, die alle drei Wege (Oberfläche, API, MCP) braucht
- nichts an Schlüsseln, Rechten, Freigaben oder Protokoll
- kein Format gespeicherter Daten, keine Migration
- kein nativer Plattformcode (`torro-appkit`), keine neue Abhängigkeit
- keine Änderung an den Gates der Skill-Kette (`approved`, `awaiting-approval`, Freigabe-Skills)

Richtwert bis zu 5 Dateien oder etwa 50 geänderte Zeilen; mehr ist nur eine Warnung mit dem Vorschlag
`/issue-plan`, kein Abbruch.

## Direkt-PR ohne Issue (`quick-fix`)

Kriterium „Direkt-PR“ (alle Punkte): höchstens 2–3 Dateien und wenige Zeilen; erlaubt sind Text, Doku, Kommentare,
Übersetzungen in `crates/torro-core/src/i18n/de.rs` und kleine Korrekturen in `scripts/` und `.claude/skills/`;
nicht erlaubt sind Programmlogik, Schnittstellen, Konfiguration, Abhängigkeiten und Änderungen an den Gates der
Skill-Kette (`approved`, Merge-Regeln). Bei Zweifeln hält der Agent an und schlägt `/issue-small` vor. Den Worktree
legt `/quick-fix` selbst an. Verifizieren mit `scripts/ci-local.sh --quick`, bei einer Übersetzung zusätzlich
`TORRO_LANG=de cargo test -p torro-core`.

## Lokale Instanz

TorroWorkplace hat keine lokale Instanz (kein `.enon-tools/hooks/start.mjs`): Pläne schreiben immer `Instanz: nein`,
`issue-approve` startet nichts. Die Apps startet man zum Ansehen mit `--demo` (Skill `torro-design`).

## Worktrees und Herdr

- Worktrees legt das Plugin an (`/issue-plan`, `/issue-small`, `/quick-fix`, von Hand `<cli> worktree create <nr>`),
  unter `.worktrees/TorroWorkplace-<nr>-<slug>` im Haupt-Checkout. Ältere Ordner `torro-<nr>-…` findet
  `<cli> worktree list` über den Branch. Nach dem Merge räumt `/worktree-cleanup` in der Sitzung im Worktree auf.
- Jeder Worktree hat ein eigenes `target/`; der erste `cargo build` dort dauert entsprechend.
- In Herdr übergibt jede Phase ab der Planung an einen eigenen Tab (`plan-<nr>`, `impl-<nr>` …); das gilt immer, ohne
  Schalter. Persönliche Einstellungen (Fokus neuer Tabs, Leerlauf-Rückfrage, Schließen der übergebenden Pane) stehen
  in `~/.enon/herdr.env`.
- Sitzungstitel, Phase in Herdr und den Issue-Kontext beim Sitzungsstart liefern die Hooks des Plugins.

## Verwandte Dokumentation

- `docs/git-workflow.md` – Branches, Issues, Commits, PRs
- `CLAUDE.md` – Regeln für Code, Prüfen und die drei Wege Oberfläche, API, MCP
