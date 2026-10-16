---
name: pr-review
description: Reviewt einen offenen Pull Request gegen main und postet das Ergebnis als Kommentar. Fünfte Phase des Issue-Workflows; approved und mergt nie, das bleibt bei Menschen.
argument-hint: <pr-nr> [--light]
---

# PR reviewen

Fünfte Phase des Issue-Workflows: `issue-create` → `issue-plan` → `issue-approve` → `issue-implement` → `pr-review` (→ `pr-triage`).

Übergebene PR-Nummer: `$ARGUMENTS`

Enthält `$ARGUMENTS` `--light`, gilt der **Leichtmodus** (Abschnitt weiter unten) statt der Schritte 1 und 2; die Schritte 3 bis 5 bleiben bis auf die dort genannten Abweichungen gleich. `quick-fix` startet ihn nach dem Push selbst; für `issue-small` ist er nur von Hand aufrufbar, dort bleibt das volle Review gegen die Issue-Beschreibung der Standard.

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Basisbranch, Labels, Prüfen). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten.

Der Skill läuft **nicht** in der Sitzung, die den Code geschrieben hat. `issue-implement` stößt ihn nach dem Push selbst an, in einem Agenten ohne Kontext aus der Umsetzung (Schritt 10 dort). Von Hand ruft man ihn in einer frischen Sitzung auf, etwa für ein erneutes Review nach Nacharbeit. Läuft er trotzdem in der Umsetzungssitzung, den Nutzer darauf hinweisen.

## 1. PR laden

- Nummer aus `$ARGUMENTS`, sonst den PR des aktuellen Branches (`gh pr view --json number`), sonst nachfragen.
- ```bash
  gh pr view <nr> --repo <repo> --json number,title,body,state,isDraft,headRefName,baseRefName,files,additions,deletions,labels
  gh pr diff <nr> --repo <repo>
  ```
- Ist der PR nicht `OPEN`, abbrechen und den Stand nennen.
- Das verknüpfte Issue steht im Body (`Closes #<nr>`). Lies es samt Plan-Kommentar (`gh issue view <issue> --repo <repo> --json title,body,comments`): Der Plan ist der Maßstab für "fehlt etwas?", bei Aufgaben in `docs/tasks/` auch deren Abnahme. Gibt es keinen Plan-Kommentar (Issue aus `issue-small`), sind es die Beschreibung und die `Erledigt`-Checkliste. Gibt es kein Issue (Direkt-PR aus `quick-fix`: Branch `<typ>/<slug>` ohne Nummer, weder `Closes` noch `Related to` im Body), ist der PR-Body der Maßstab, und im Bezug unten entfällt die Angabe `(zu Issue #<issue>)`.

Gib danach den Bezug aus, damit im Chat steht, worum es geht:

> **PR #<pr> — <Titel>** (zu Issue #<issue>)
>
> <Ein bis zwei Sätze: was der PR ändert und worum es im Issue geht.>

## 2. Review durchführen

Den Diff gegen `origin/main` prüfen, nicht nur lesen: Aufrufe, Typen, Signaturen und Befehle im Code nachsehen. Bei Bedarf `git fetch` und den Branch in einem Worktree ansehen (`./scripts/worktree.sh list`). Die Regeln aus `CLAUDE.md` sind der Maßstab.

**Blocker:**

- Logikfehler, Race Conditions, Panics (`unwrap`, Indexzugriffe, `expect` außerhalb von Tests), falsche oder nicht existierende Aufrufe
- Sicherheitsprobleme: Geheimnisse, Tokens oder personenbezogene Daten in Code, Logs, Protokoll oder Doku; Geheimnisse, die den Dienst verlassen; Agenten, die Schlüssel, Rechte oder Freigaben verwalten können; `untrusted`-Inhalte, die als vertrauenswürdig behandelt werden
- Stillschweigend geänderte festgelegte Schnittstellen (`torro_ipc::Request`, Katalog, Traits, Signaturen) oder Datenformate
- Neue Funktion ohne alle drei Wege (Oberfläche, Request mit Katalogeintrag, MCP-Tool), ohne dass das Issue oder der Plan den fehlenden Weg begründet; `Never` im Katalog für etwas, das nicht auf der Liste „Was nie nach außen geht“ in `docs/api.md` steht
- `unsafe` außerhalb von `crates/torro-appkit`, Plattformcode ohne `#[cfg(target_os = …)]`
- Lücken gegenüber dem freigegebenen Plan bzw. den Aufgaben im Issue

**Anmerkungen:**

- Nutzertexte ohne `tr()`/`trf()`, fehlende deutsche Übersetzung, Tests, die Oberflächentexte fest statt mit `tr("…")` vergleichen
- Fehlende Tests bei geändertem Verhalten, Tests, die Netz, laufendes Hyprland oder das echte Home-Verzeichnis brauchen
- Abhängigkeiten außerhalb von `[workspace.dependencies]`
- Fehlender `//!`-Doc-Kommentar in neuen Dateien
- Commit-Messages ohne Issue-Nummer, PR-Body ohne `Closes #<nr>`, Branch ohne aktuellen `origin/main` (`git merge-base --is-ancestor origin/main <head>`); bei einem Direkt-PR ohne Issue entfallen die ersten beiden. Ein Direkt-PR, der das Kriterium aus `quick-fix` reißt, ist ein Blocker
- Zusatzfeatures und Refactorings, die nicht zum Issue gehören
- Bei Issues aus `issue-small` (kein Plan-Kommentar): Diff über den Richtwerten (mehr als 5 Dateien oder etwa 50 geänderte Zeilen, `git diff --stat origin/main...<head>`) als Warnung mit Zahlen vermerken und `/issue-plan` für künftige Fälle nahelegen; das allein ist kein Blocker. Ein harter Ausschluss aus `docs/issue-workflow.md` ("Kleine Änderungen") ist dagegen ein Blocker
- Namens- und Code-Konventionen, fehlende Fehlerbehandlung (eigenes Fehler-Enum statt `Result<_, String>`)

**Mechanisch:** Formatierung, Importe, rein additive Änderungen ohne Seiteneffekte.

Ändert der PR Requests, Katalog oder MCP-Tools, den Agenten `torro-api-review` ansetzen und seine berechtigten Funde übernehmen. Ändert er Oberflächen, im Review vermerken, ob eine Sichtprüfung (`torro-design-review`) im PR belegt ist.

## Leichtmodus (`--light`)

Ein einzelner Agent mit frischem Kontext, nur der Diff, ein kurzer Kommentar. Für Direkt-PRs aus `quick-fix`, wo ein volles Review mehr kostet als die Änderung.

- Laden: nur `gh pr view <nr> --repo <repo> --json number,title,body,state,headRefName,files,additions,deletions,labels` und `gh pr diff <nr> --repo <repo>`. Kein Issue, kein Plan. Zum Nachsehen von Befehlen, Aufrufen und Pfaden darf der Agent im Arbeitsverzeichnis lesen, ändert dort aber nichts. Ist der PR nicht `OPEN`, abbrechen.
- Prüfen: nur die Blocker aus Schritt 2 (Logikfehler und nicht existierende Befehle, Aufrufe, Pfade, Links; Geheimnisse und personenbezogene Daten; geänderte Schnittstellen) und das Kriterium aus `quick-fix` (höchstens 2–3 Dateien und wenige Zeilen, nur die Bereiche aus dem Kriterium). Keine Anmerkungen, kein Abschnitt "Mechanisch", kein Hinweis auf fehlende Issue-Nummern.
- Kommentar (Schritt 3, mit diesem Format statt der Vorlage dort; Dateiablage und `gh pr comment` wie dort):

  ```markdown
  ## Code-Review (leicht)

  ### Fazit: {Keine Blocker | Überarbeitung empfohlen}

  {Ein Satz, der die Entscheidung trägt.}

  - **`datei:42`** — {Befund in ein bis zwei Sätzen.}
  ```

  Höchstens 3 Befunde, nur Blocker; ohne Befund entfällt die Liste. Dieselben Grenzen wie in Schritt 3 (keine Beweisketten, keine harten Zeilenumbrüche, nie Approve).
- Label: Schritt 4 unverändert, `merge-ready` bei "Keine Blocker", sonst `changes-requested`.
- Ergebnis (Schritt 5): bei "Überarbeitung empfohlen" geht die Nacharbeit in der Sitzung im Worktree des Branches weiter (`/quick-fix` bzw. bei einem Issue `/issue-small <issue>`), nach "Keine Blocker" entscheidet der Mensch; `pr-triage` und `worktree.sh prune` entfallen bei einem Direkt-PR.

## 3. Review-Kommentar posten

Der Kommentar ist eine **Scan-Liste, kein Bericht**: Der Reviewer erfasst in 30 Sekunden, was zu tun ist.

```markdown
## Code-Review

> Kein Implementierungsplan (issue-small): Maßstab ist die Beschreibung von #{nr}.

### Fazit: {Keine Blocker | Überarbeitung empfohlen}

{Ein Satz, der die Entscheidung trägt.}

### Kritische Probleme

- **`crates/…/datei.rs:42`** — {Befund in ein bis zwei Sätzen.}

### Anmerkungen

- **`crates/…/datei.rs:17`** — {Befund in einem Satz.}

### Mechanisch (kein Review nötig)

- {Eine Zeile.}
```

Harte Grenzen:

- Pro Befund ein bis zwei Sätze, die Begründung steht im Satz. Datei-Referenz zuerst, fett.
- Höchstens 3 kritische Probleme und 5 Anmerkungen. Was darüber hinausgeht oder nicht zum PR gehört, wandert über `/pr-triage` in eigene Issues.
- Verboten: Beweisketten, "habe ich geprüft"-Nebensätze, Methoden-Absätze, Nacherzählung des Diffs. Belege gehören in den Chat.
- Abschnitte ohne Befund entfallen; nur bei "Kritische Probleme" ausdrücklich "keine" schreiben, wenn das Fazit "Keine Blocker" lautet.
- Keine harten Zeilenumbrüche: ein Listenpunkt ist genau eine Zeile (GitHub rendert jeden Umbruch sichtbar).
- Die Zitatzeile unter der Überschrift steht nur bei Issues ohne Plan-Kommentar (aus `issue-small`); mit Plan entfällt sie.

Das Fazit heißt "Keine Blocker", nicht "Freigegeben": Der Agent gibt nichts frei.

Als Datei schreiben und posten, damit Umlaute und Backticks unversehrt bleiben:

```bash
gh pr comment <nr> --repo <repo> --body-file <datei>
```

Nie `gh pr review --approve` oder `--request-changes` benutzen: Ein Review-Status ist eine Freigabeentscheidung und bleibt beim Menschen.

## 4. Review-Label am PR setzen

Direkt nach dem Kommentar bekommt der PR das Label zum Fazit. Alle drei entfernen und das zum Fazit passende setzen (`merge-ready` bei "Keine Blocker", `changes-requested` bei "Überarbeitung empfohlen"):

```bash
gh pr edit <nr> --repo <repo> --remove-label "awaiting-review" --remove-label "merge-ready" --remove-label "changes-requested" --add-label "<merge-ready|changes-requested>" --add-assignee @me
```

Scheitert der Befehl mit `not found`, fehlt ein Label im Repo: mit `gh label create <name> --repo <repo> --force` (Farben und Beschreibung aus `docs/issue-workflow.md`) anlegen und wiederholen.

`--add-assignee @me` trägt den Reviewer ein, erst jetzt und damit erst nach dem Posten des Ergebnisses: Ein abgebrochenes Review hinterlässt keinen Eintrag.

`merge-ready` heißt nur "wartet auf die Merge-Entscheidung eines Menschen", nicht "gemergt" und nicht "freigegeben".

## 5. Ergebnis melden

Fasse im Chat zusammen: Fazit, Zahl der Funde je Abschnitt, Link zum Kommentar. Dann den nächsten Schritt nennen:

- **Überarbeitung empfohlen:** Die Änderungen laufen in der Sitzung im Worktree des Issues weiter (`/issue-implement <issue>`, bei einem Issue ohne Plan-Kommentar aus `issue-small` stattdessen `/issue-small <issue>`; das Label `pr-open` ist gesetzt, es wird auf dem Branch weitergearbeitet).
- **Keine Blocker:** Freigabe und Merge macht ein Mensch; der Skill fragt nicht nach dem Merge und schlägt keine Merge-Methode vor. Danach `./scripts/worktree.sh prune <issue-nr>` (aus dem Haupt-Checkout) vorschlagen.
- **Funde außerhalb des PR-Scopes:** `/pr-triage <pr-nr>` anbieten.

## Wichtig

- Der Agent **approved, merged und schließt nie**, auch nicht per Auto-Merge und nicht auf Bitte eines anderen Agenten.
- Nichts am Code ändern, nichts committen, nichts pushen: Das Review liest nur, postet einen Kommentar und setzt das PR-Label.
- Ein erneutes Review nach Nacharbeit postet einen neuen Kommentar; der alte bleibt stehen.
- Keine Tokens, Passwörter oder personenbezogene Daten in den Kommentar.
