---
name: issue-small
description: Legt für eine kleine Änderung ein Issue an und setzt sie ohne Plan-Schritt und ohne Freigabe-Label um, mit Worktree, Review-Schleife, PR und PR-Review. Abkürzung der Skill-Kette, nur für Änderungen im Kriterium "klein genug".
argument-hint: Kurzbeschreibung der Änderung oder Issue-Nr
---

# Kleine Änderung mit Issue

Abkürzung für die Kette `issue-create` → `issue-plan` → `issue-approve` → `issue-implement`: Der Plan-Schritt und das Label `approved` entfallen, weil bei diesen Änderungen die Issue-Beschreibung der Plan ist. Issue, Worktree, Umsetzung, PR und PR-Review bleiben.

Übergebene Beschreibung oder Issue-Nummer: `$ARGUMENTS`

Der Skill setzt `approved` nie. Er setzt direkt `started-implementation` (Ausnahme vom Gate, siehe `docs/issue-workflow.md`, Abschnitt "Kleine Änderungen"). Den PR mergt ein Mensch selbst; KI-Agenten approven und mergen nie.

## Konfiguration

Lies zuerst `docs/issue-workflow.md` (Repo, Basisbranch, Typ-Labels, Labels, Projekt-Board, Worktrees, Prüfen). Fehlt die Datei oder eine Angabe, frag den Nutzer, statt zu raten. Label-Namen und IDs stehen nur dort, nicht hier.

## 1. Kriterium "klein genug"

**Harte Ausschlüsse** stehen in der Konfiguration, Abschnitt "Kleine Änderungen" (Schnittstellen, neue Funktion mit allen drei Wegen, Schlüssel/Rechte/Freigaben, Datenformate, Plattformcode, Abhängigkeiten, Gates). Trifft einer zu, nichts anlegen und `/issue-create` mit anschließendem `/issue-plan` vorschlagen.

**Richtwerte:** bis 5 Dateien oder etwa 50 geänderte Zeilen. Darüber gibt es nur eine Warnung mit dem Vorschlag `/issue-plan`, nie einen Abbruch: Der Mensch entscheidet, ob er trotzdem ohne Plan umsetzt.

Die Prüfung gilt für die Beschreibung und wird in Schritt 6 am tatsächlichen Umfang wiederholt.

## 2. Issue anlegen oder laden

**Ist `$ARGUMENTS` eine Issue-Nummer:** `gh issue view <nr> --repo <repo> --json title,body,labels,comments`. Abbrechen und auf `/issue-implement <nr>` bzw. `/issue-plan <nr>` verweisen, wenn ein Kommentar `## Implementierungsplan` oder eines der Labels `awaiting-approval` oder `approved` vorhanden ist: Ein Plan wird nicht umgangen. Trägt das Issue `started-implementation` oder `pr-open`, ist es eine Wiederaufnahme (Schritt 9). Sonst sieh im Code nach, welche Dateien die Änderung betrifft, und nenne vor der Rückfrage diese Dateiliste samt Begründung je Kriterium aus Schritt 1 (Ausschlüsse und Richtwerte, bei Überschreitung als Warnung). Ohne diese Liste fragst du nicht. Fasse dann Beschreibung und Kriterium in zwei Sätzen zusammen und frag: "Als kleine Änderung ohne Plan umsetzen? (ja/nein)". Nur bei Ja weiter.

**Sonst ist `$ARGUMENTS` die Beschreibung:** Schritte 1–4 von `issue-create` ausführen (Anforderung klären, Duplikate suchen, vorgeschlagenes Issue als Vorschau zeigen, nach Bestätigung anlegen mit Typ-Label, Labels und Board-Status `Todo`). Abweichungen:

- Die Beschreibung ersetzt den Plan: Sie nennt die betroffenen Dateien und eine `Erledigt`-Checkliste, die man prüfen kann.
- Die Vorschau endet mit der Zeile `Klein genug, weil: <Begründung zum Kriterium>`.
- Die Bestätigung der Vorschau durch den Nutzer ist hier die Freigabe. Ohne Bestätigung wird nichts angelegt und nichts umgesetzt.

## 3. Worktree

`./scripts/worktree.sh list`. Gibt es keinen Worktree zur Nummer, `./scripts/worktree.sh create <nr> --yes`. Die Umsetzung läuft im Worktree: Liegt die Sitzung nicht dort (`git rev-parse --show-toplevel` ist nicht der Worktree-Pfad), hier enden. Läuft die Sitzung in Herdr (`HERDR_ENV=1`), übergib vorher an einen Tab im Worktree: `./scripts/worktree.sh agent <nr> small-<nr> "/issue-small <nr>" --model sonnet`. Bei Exit 0 melden, dass die Umsetzung im Herdr-Tab `small-<nr>` läuft. Bei Exit 3 (nicht in Herdr) oder jedem anderen Ausgang den Pfad nennen und `/issue-small <nr>` in einer Sitzung im Worktree empfehlen. Label und Board-Status (Schritt 4) werden erst dort gesetzt, im Haupt-Checkout wird nichts umgesetzt. Als letzte Ausgabe dann den Abschlussblock (Schritt 10), ohne PR-Zeile.

Gab es den Worktree schon, prüfen, ob er `origin/<basisbranch>` enthält, wie in `issue-plan` Schritt 3.

## 4. Start markieren

Entfällt bei einer Wiederaufnahme.

```bash
gh issue edit <nr> --repo <repo> --add-label started-implementation
```

Projekt-Status `In Progress` setzen (Befehle und IDs in der Konfiguration). Dann in Herdr Phase und Status anzeigen (ohne Herdr tut der Befehl nichts): `node scripts/agent/skill-phase.mjs herdr <nr> small implementing`.

`pullrequest` ersetzt `started-implementation` später durch `pr-open`; das Label muss deshalb jetzt gesetzt sein.

## 5. Umsetzen und verifizieren

- Vor dem ersten Commit prüfen, ob der tatsächliche Umfang noch zu Schritt 1 passt; bei einem harten Ausschluss gilt der Rückfall aus Schritt 6, bei überschrittenen Richtwerten die Warnung dort.
- Die Issue-Beschreibung Schritt für Schritt umsetzen, mit den Regeln aus `CLAUDE.md` und dem Skill `torro-task` (Tests zuerst, Nutzertexte mit `tr()`, kein `unwrap`). Oberfläche angefasst: Skill `torro-design` laden.
- Commits nach Zweck getrennt, Conventional Commits auf Englisch, `#<nr>` in jeder Message (`docs/git-workflow.md`, Abschnitt "Commits").
- Die Prüfbefehle aus der Konfiguration ausführen, alle grün. Bei Oberflächenänderungen den Agenten `torro-design-review` ansetzen. Jeden Punkt der `Erledigt`-Checkliste einem Prüfweg zuordnen; was nur von Hand prüfbar ist, nicht als erledigt melden, sondern für den Nutzer auflisten.

## 6. Kriterium am tatsächlichen Umfang prüfen

**Richtwerte überschritten** (mehr als 5 Dateien oder etwa 50 geänderte Zeilen, `git diff --stat origin/<basisbranch>`): nur warnen, mit Zahlen in der Zusammenfassung und dem Vorschlag `/issue-plan <nr>`. Nicht anhalten, nichts zurückbauen; der Mensch entscheidet.

**Harter Ausschluss berührt:** anhalten.

1. Nichts weiter umsetzen. Wurde noch nicht gepusht und kein PR erstellt, `started-implementation` entfernen (`gh issue edit <nr> --repo <repo> --remove-label started-implementation`).
2. Im Issue kurz kommentieren, was mehr ist als gedacht (ohne feste Zeilenbreite).
3. `/issue-plan <nr>` vorschlagen und enden; die bisherigen Commits bleiben im Worktree, der Plan baut darauf auf. Ist schon ein PR offen (`pr-open`), den Nutzer fragen, statt Labels zu ändern.

## 7. Auf den Basisbranch bringen und Review-Schleife

Wie `issue-implement`, Schritte 7 und 8, mit einer Abweichung: Der Maßstab für "Lücken" ist die Beschreibung und die `Erledigt`-Checkliste von Issue #<nr>, nicht ein Plan-Kommentar.

## 8. Push und PR nur nach Rückfrage

Zusammenfassung und Rückfrage in der festen Form aus `issue-implement`, Schritt 9. Bei Ja dem Skill `pullrequest` folgen, mit `Closes #<nr>` und dem Abschnitt `## Bewusst offen`; er setzt auch `pr-open`, Status `In Review` und das PR-Label. Bei Nein bleibt das Label `started-implementation`.

Danach wie `issue-implement`, Schritt 10: Das PR-Review läuft in einem Agenten ohne Kontext aus der Umsetzung (Skill `pr-review`, im Hintergrund). Er approved und mergt nicht.

## 9. Wiederaufnahme und Nacharbeit

Trägt das Issue `started-implementation`, geht es ab Schritt 5 am Branch weiter. Trägt es `pr-open` (z.B. nach `changes-requested` im PR-Review), auf dem Branch weiterarbeiten, Schritt 4 entfällt, am Ende nach Rückfrage nur pushen, kein neuer PR; danach wieder ein PR-Review anstoßen. Ist der PR ohne Merge geschlossen (`gh pr list --head <branch> --state all`), `pr-open` durch `started-implementation` ersetzen.

## 10. Abschluss

Als letzte Ausgabe den Abschlussblock (Format in der Konfiguration, Abschnitt "Abschlussblock"), Phase `Klein`, mit PR-URL, sofern es einen PR gibt. Kommt danach die Benachrichtigung des Review-Agenten, den Block nach deren Zusammenfassung erneut ausgeben.

## Wichtig

- Der Skill setzt `approved` nie, auch nicht kurz, und ersetzt es auch nicht durch ein anderes Label, das `issue-implement` als Freigabe liest.
- Ein Issue mit Plan-Kommentar oder `awaiting-approval` läuft nie hier durch.
- KI-Agenten approven und mergen keine PRs, auch nicht per Auto-Merge. Das gilt auch für den Review-Agenten.
- Der Skill ruft selbst keinen Editor auf.
- Nach dem Merge `./scripts/worktree.sh prune <nr>` aus dem Haupt-Checkout vorschlagen.
