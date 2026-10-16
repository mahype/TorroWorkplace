---
name: quick-fix
description: Setzt eine winzige Änderung (Tippfehler, falscher Link, Übersetzung, kleine Korrektur in Text, Doku, Skript oder Skill) ohne Issue um, mit eigenem Worktree, PR ohne Closes und leichtem PR-Review. Abkürzung der Skill-Kette, nur für Änderungen im Kriterium "Direkt-PR".
argument-hint: Kurzbeschreibung der Änderung
---

# Kleinständerung ohne Issue

Abkürzung für Änderungen, bei denen ein Issue nur Lärm im Board wäre: Es entstehen kein Issue und kein Board-Eintrag, der Ablauf geht vom Branch direkt zum Pull Request. Für alles, was das Kriterium reißt, gibt es `issue-small` (Issue, ohne Plan) und die volle Kette.

Übergebene Beschreibung: `$ARGUMENTS`

Der Skill setzt `approved` nie und fasst kein Issue an. Den PR mergt ein Mensch selbst; KI-Agenten approven und mergen nie.

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Basisbranch, PR-Labels, Prüfen, Abschnitt "Direkt-PR ohne Issue"). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten. Ist `$ARGUMENTS` leer, nach der Änderung fragen.

In Herdr Phase anzeigen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr - quick`.

## 1. Kriterium "Direkt-PR"

**Alle** Punkte müssen zutreffen:

- höchstens 2–3 Dateien und wenige Zeilen
- erlaubt: Text, Doku, Kommentare, Übersetzungen in `crates/torro-core/src/i18n/de.rs`, kleine Korrekturen in `scripts/` und `.claude/skills/`
- nicht erlaubt: Programmlogik, Schnittstellen, Konfiguration, Abhängigkeiten, Änderungen an den Gates der Skill-Kette (`approved`, Merge-Regeln, Freigabe-Skills)

Trifft ein Punkt nicht zu oder bleibt ein Zweifel, **anhalten**, nichts anlegen und `/issue-small` (bzw. `/issue-create` mit `/issue-plan`) vorschlagen. Die Prüfung wird in Schritt 4 am tatsächlichen Umfang wiederholt.

## 2. Branch und Worktree

1. `git fetch origin main`. Branch `<typ>/<slug>` mit dem Conventional-Commit-Typ als Präfix (`docs`, `fix` oder `chore`, passend zum späteren Commit), Slug zwei bis drei Wörter, klein, ASCII, mit `-` getrennt. Der Slug **beginnt mit einem Buchstaben**: Sonst liest das Branch-Muster (`^[a-z]+/(\d+)-`) ihn als Issue-Nummer, und Kontext-Hook und `pullrequest` verknüpfen ein fremdes Issue.
2. Bestehende Arbeit suchen: `git worktree list`, `git branch --list '<typ>/*'` und `gh pr list --repo <repo> --state open --search "<stichwort>"`. Gibt es schon einen Branch oder einen offenen PR dazu, dort weitermachen (Schritt 7), nicht neu anfangen.
3. Worktree vom Haupt-Checkout aus anlegen. Das Worktree-Skript kennt nur Issue-Nummern, deshalb per git:

   ```bash
   MAIN=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
   git -C "$MAIN" worktree add ".worktrees/<typ>-<slug>" -b "<typ>/<slug>" --no-track origin/main
   ```

   In Herdr (`HERDR_ENV=1`) den Worktree als Workspace öffnen, damit seine Chats nicht unter dem Haupt-Checkout landen: `herdr worktree open --cwd "$MAIN" --path "$MAIN/.worktrees/<typ>-<slug>" --no-focus`. Scheitert das, nur warnen.

Läuft die Sitzung in Herdr (`HERDR_ENV=1`) und nicht im neuen Worktree (`git rev-parse --show-toplevel` ist nicht dessen Pfad), übergib an einen Tab im Worktree. Der Worktree hat keine Issue-Nummer, deshalb nimmt `agent` den absoluten Worktree-Pfad statt der Nummer. Der Agent-Name ist `quick-<slug>`, auf höchstens 32 Zeichen gekürzt:

```bash
./scripts/worktree.sh agent "$MAIN/.worktrees/<typ>-<slug>" quick-<slug> "/quick-fix <beschreibung>" --model sonnet
```

Bei Exit 0 melden, dass die Umsetzung im Herdr-Tab `quick-<slug>` läuft, und hier enden: Der Tab findet den Branch in Schritt 2.2 und macht dort weiter (Schritt 7). Bei Exit 3 (nicht in Herdr) oder jedem anderen Ausgang arbeitest du in dieser Sitzung weiter.

Die Umsetzung läuft sonst im Worktree: `git -C <worktree>` und absolute Pfade, im Haupt-Checkout wird nichts geändert.

## 3. Umsetzen

- Nur die beschriebene Änderung, die Regeln aus `CLAUDE.md` beachten (Dokumente in `docs/` auf Deutsch, Code und Kommentare auf Englisch).
- Ein Commit, Conventional Commits auf Englisch **ohne Issue-Nummer**, z.B. `docs: fix typo in roadmap`. Mehrere Zwecke sind ein Zeichen, dass das Kriterium nicht passt.
- Verifizieren, was sich billig prüfen lässt: Link aufrufen, Skript mit `--help` oder `bash -n`, `scripts/ci-local.sh --quick`; bei einer Übersetzung zusätzlich `TORRO_LANG=de cargo test -p torro-core`. Was nur von Hand prüfbar ist, nicht als erledigt melden, sondern für den Nutzer auflisten.

## 4. Kriterium am tatsächlichen Umfang prüfen

`git diff --name-only origin/main...HEAD` und `git diff --stat origin/main...HEAD`. Zeigt sich mehr als 2–3 Dateien oder wenige Zeilen, oder berührt die Änderung einen nicht erlaubten Bereich: anhalten, nichts pushen und `/issue-small` vorschlagen; die Commits bleiben im Worktree und lassen sich in das Issue übernehmen.

Danach `origin/main` holen und einbinden wie `pullrequest` Schritt 2 (Rebase, solange der Branch nicht auf origin liegt, sonst Merge, kein Force-Push).

## 5. Rückfrage vor Push und PR

Nenne dem Nutzer **vor** dem PR das Kriterium und die Dateien, in dieser Form:

1. **Erste Zeile ist die Rückfrage** mit Kurz-SHA und Branch: `<branch> (abc1234): pushen und PR erstellen?` Bei Nacharbeit an einem offenen PR nur `pushen?`.
2. **Kriterium:** ein Satz, warum es ein Direkt-PR ist (erlaubter Bereich, Zahl der Dateien und Zeilen).
3. **Dateien** als Liste aus `git diff --name-only`, je Datei ein Halbsatz.
4. **Von Hand zu prüfen** als Checkliste `- [ ] …`, entfällt ohne Handprüfung.

Ohne Ja wird nichts gepusht.

## 6. PR und Review

Bei Ja dem Skill `pullrequest` im Direkt-Modus folgen: kein Review-Angebot, keine Frage nach der Issue-Nummer, Titel ohne `#<nr>`, kurzer Body ohne `Closes`, nur Assignee und `awaiting-review`. Dann das leichte Review in einem Agenten ohne Kontext aus der Umsetzung anstoßen:

```
Agent(
  description: "PR-Review (leicht) #<pr-nr>",
  subagent_type: "general-purpose",
  prompt: "Führe den Skill pr-review mit --light für PR #<pr-nr> im Repo <repo> aus (Arbeitsverzeichnis <worktree-pfad>). Der PR '<titel>' ist ein Direkt-PR ohne Issue und wurde soeben von quick-fix erstellt oder aktualisiert; du siehst ihn zum ersten Mal, ohne Kontext aus der Umsetzung. Folge dem Leichtmodus des Skills vollständig: Kommentar posten, PR-Label setzen. Ändere nichts am Code, approve und merge nicht.",
  run_in_background: true
)
```

Das Ergebnis kommt als eigene Benachrichtigung, diese Sitzung wartet nicht darauf.

## 7. Nacharbeit

Bei `changes-requested` oder Kommentaren im selben Worktree weiterarbeiten, Kriterium erneut prüfen, nach Rückfrage nur pushen (kein neuer PR) und das leichte Review erneut anstoßen. Ist der PR ohne Merge geschlossen, nichts weiter tun und den Nutzer fragen.

## 8. Nach dem Merge

Aus dem Haupt-Checkout vorschlagen (nicht selbst ausführen, ohne Rückfrage): `git worktree remove .worktrees/<typ>-<slug>` und `git branch -D <typ>/<slug>`. `./scripts/worktree.sh prune` greift hier nicht, es kennt nur Issue-Nummern.

## 9. Abschluss

Als letzte Ausgabe den Abschlussblock (Format in `docs/issue-workflow.md`, Abschnitt "Abschlussblock"), Phase `Direkt`, ohne die Zeilen `#<nr> – <Titel>` und `Issue:`, stattdessen mit einer Zeile `Branch: <branch>`, und mit PR-URL, sofern es einen PR gibt. Kommt danach die Benachrichtigung des Review-Agenten, den Block nach deren Zusammenfassung erneut ausgeben.

## Wichtig

- Kein Issue, kein Board-Eintrag, keine Issue-Labels. Diese PRs sind nur in der PR-Liste zu finden.
- Der Skill setzt `approved` nie und ändert nie die Gates der Skill-Kette.
- KI-Agenten approven und mergen keine PRs, auch nicht per Auto-Merge. Das gilt auch für den Review-Agenten.
- Pushen und PR erst nach Rückfrage.
- Der Skill ruft selbst keinen Editor auf.
