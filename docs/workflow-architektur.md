# Workflow-Architektur: Wo gehören die Regeln für KI-gestartete Issues hin?

Stand: 2026-11-23. **Entwurf und Bestandsaufnahme, nicht in Kraft.** Diese Datei ändert keine Regel, keinen Skill, kein
Label und keine Konfiguration. Sie hält fest, was es heute gibt, wo es sich doppelt, und was projektspezifisch in
TorroWorkplace bleiben sollte und was generisch ins spätere Tooling gehört. Der endgültige Ort der generischen Logik ist
**offen**: Das externe Skills-Repository (ENON AI Tooling) zieht um und wird voraussichtlich Toolwerk AI Tooling.
Bis zur gemeinsamen Entscheidung entsteht in TorroWorkplace **keine** zweite zentrale Prüflogik. Issue: #983.

## Was es heute gibt

| Ort | Enthält | Versioniert | Gehört zu |
|---|---|---|---|
| `CLAUDE.md`, `docs/issue-workflow.md`, `docs/git-workflow.md`, `docs/entscheidungsregeln.md` | Regeln des Repos: Prüfen, Grenzen für `issue-small` und `quick-fix`, harte Ausschlüsse, Branch-, Commit- und PR-Form, Regeln 1 bis 11 | ja, im Repo | TorroWorkplace |
| `.claude/skills/` (`torro-api`, `torro-design`, `torro-task`, `nextcloud`, `diagnosing-bugs`, `pr`), `.claude/agents/` (`torro-api-review`, `torro-design-review`, Coder, `nextcloud-expert`) | Anleitungen und Prüfer für die Umsetzung. Geprüft: Sie enthalten **keine** Kriterien, wann ein Issue gestartet werden darf (keine Treffer für `aufwand`, „klein genug“, „risikoarm“) | ja, im Repo | TorroWorkplace |
| Plugin `enon-tools` (Marketplace `enon-ai-tooling`, lokal unter `~/.claude/plugins/`) | Einstufung Quick Fix, Klein, Normal (`issue-create`), Labels `aufwand:*` und Phasen-Labels, Gate `plan-accepted`, Kriterium von `issue-small` (Standard, das Repo überschreibt es), `issue-autopilot` mit Reservierung (`autopilot`), Routine-Annahme und Plätzen, Dashboard | nein, extern | generisch, im Umzug |
| T3-Scheduled-Tasks (in T3 Code gespeichert, sichtbar über `list_scheduled_tasks`) | Dispatcher, Scout, Supervisor, Wochenauswertung, jeweils als Prompt | **nein** | T3, nicht im Repo |
| GitHub | Labels, Board `TorroWorkplace` (`.enon-tools/konventionen.json`), Action „Phasen-Labels“ | teils | gemischt |

Eine Entscheidung „dieses Issue darf eine KI selbst starten“ gibt es **nirgends** als Regel. Die Gates des Plugins
(`plan-accepted` durch Menschen, Routine-Annahme nur für `autopilot`) sind die einzigen.

## Die geplanten Aufgaben im Einzelnen

| Aufgabe | Takt | Zweck | Eigene Fachlogik im Prompt (Dopplung) |
|---|---|---|---|
| Dispatcher | stündlich | startet Coding-Agenten für geeignete Issues, danach Reviews | Auswahl („klar, klein, risikoarm“, Ausschlüsse) verdoppelt „Kleine Änderungen“ und Regel 11; Limit von 3 Agenten verdoppelt Regel 11; Draft-Pflicht und Präfix `[auto]` stehen **nur dort**; GUI-Pflicht mit Vorher-nachher-Bildern verdoppelt `torro-design` und die PR-Vorlage. Eigenes und berechtigt dort: Takt, Modellwahl je Anbieter, Zählen der T3-Threads, Review durch einen anderen Anbieter |
| Scout | stündlich, nur Bericht | nennt bis zu fünf geeignete Issues | Auswahl („klein, klar, risikoarm“) verdoppelt dieselben Kriterien |
| Supervisor | alle 15 Minuten | prüft Threads, beantwortet rein technische Rückfragen | keine Kriterien zur Reife; die Grenzen („nie Freigaben, nie Merge“) sind Sicherheitsregeln des Supervisors und gelten unabhängig |
| Wochenauswertung | montags 09:00 | liest Threads, PRs, Reviews, bewertet Modelle | keine Dopplung, nur lesend |

## Befunde

1. **Drei Fassungen von „klein und risikoarm“:** Dispatcher-Prompt, Scout-Prompt und Regel 11 („offensichtlich nötig, jetzt
   umsetzbar“), dazu die harten Ausschlüsse von `issue-small` und die Einstufung des Plugins. Sie decken sich nicht.
2. **Regel 11 und Dispatcher widersprechen sich:** Regel 11 erlaubt einen Start je Lauf und sperrt bei drei ungeprüften
   PRs; der Dispatcher begrenzt auf drei gleichzeitige Agenten. Offen ist auch, ob der „Projekt-Bot“ aus Regel 11 und der
   Dispatcher dasselbe sind.
3. **Draft-Pflicht und `[auto]` standen nur im Prompt.** Bei der Bestandsaufnahme am 2026-11-23 waren die automatisch
   erstellten PRs #963 bis #967 keine Drafts. Die Konvention steht jetzt in `docs/git-workflow.md` (dieser PR).
4. **Kein Merkmal für „darf die KI starten“.** [#961](https://github.com/mahype/TorroWorkplace/issues/961) (3054 Zeilen, neue
   Requests, Journal) hatte kein Label, [#877](https://github.com/mahype/TorroWorkplace/issues/877) nur `en-workflow`.
5. **Veraltete Namen** (`issue-approve`, `approved`, `awaiting-approval`, Status `Awaiting Approval`) standen in
   `CLAUDE.md`, `docs/issue-workflow.md` und `docs/git-workflow.md`. Sie sind in diesem PR korrigiert.

## Vorschlag für die Trennung

**Generisch, später ins Tooling (Toolwerk AI Tooling, Ort offen):**

- Die Definition der Umsetzungsreife: zwei Wege (A: `aufwand:klein` und Prüfliste, B: `plan-accepted` durch Menschen),
  ein Menschen-Gate für alles andere.
- Die **deterministische Prüfung** als Befehl des Tooling, der nur liest: Label `aufwand:klein`, ausschließende Labels,
  Checkliste „Erledigt, wenn“ vorhanden, kein „Offene Punkte“-Abschnitt, kein offener PR, kein Phasen-Label, keine
  offene Abhängigkeit, Beschreibung von einem Mitglied. Der Befehl sagt nie „reif“, sondern „mechanisch bestanden“ und
  listet, was ein Agent noch beurteilt.
- Das Format des Protokollkommentars am Issue und die Regel, wann er geschrieben wird.
- Reservierung und Zählung der Plätze (das Plugin hat beides im Autopiloten) und die Regel „Zweifel heißt nicht reif“.
- Die Konvention für automatisch beauftragte PRs (Draft, Präfix, nie Auto-Merge) als einstellbarer Standard.
- Einstellbar je Repo (analog `.enon-tools/konventionen.json`): ausschließende Labels, Obergrenze der Agenten, Präfix.

**Projektspezifisch, bleibt in TorroWorkplace:**

- Die harten Ausschlüsse und Richtwerte („Kleine Änderungen“ in `docs/issue-workflow.md`): `torro_ipc`, Befehlskatalog,
  `torro-appkit`, Datenformate, Migration, Abhängigkeiten.
- Labels des Repos (`gui`, `app:*`, `core`, `ai-tooling`, `manual-check`) und die Frage, welche davon Weg A ausschließen.
- Skills und Prüfer (`torro-api`, `torro-design`, `torro-api-review`, `torro-design-review`), Prüfstufen
  (`scripts/ci-local.sh`), Übersetzungsregeln (`de.rs`), PR-Vorlage mit Screenshots.
- Die Entscheidungsregeln 1 bis 11 und dass Merge, Push auf `main` und Schließen bei Sven bleiben (Regel 10).
- Board und Typen (`.enon-tools/konventionen.json`).

**Nur T3, weder Repo noch Tooling:** Takt der Aufgaben, Modellwahl je Anbieter, Zählen der aktiven T3-Threads und der
Review durch einen anderen Anbieter. Das Plugin kennt herdr, nicht T3. Die Prompts sollten danach nur noch auslösen,
orchestrieren und auf die Regeln verweisen.

## Entwurf der Prüfliste (nicht in Kraft)

Jeder Punkt muss erfüllt sein, Zweifel heißt nicht reif. „Skript“ ist ein Punkt, den der spätere Befehl entscheidet.

| Punkt | Prüft |
|---|---|
| Issue offen, Label `aufwand:klein` von `issue-create` oder einem Menschen | Skript |
| Kein Label `gui`, `strong-model-recommended`, `manual-check`, `aufwand:normal` | Skript |
| Keine harten Ausschlüsse, keine sichtbare Änderung der Oberfläche | Agent |
| Checkliste „Erledigt, wenn“ vorhanden | Skript |
| Jedes Kriterium objektiv prüfbar (Test, Befehl, Dateiinhalt) | Agent |
| Kein Abschnitt „Offene Punkte“ | Skript |
| Keine offene Produkt-, Gestaltungs-, Sicherheits-, Datenschutz-, Architekturfrage (Regel 9) | Agent |
| Umfang absehbar, Richtwert bis 5 Dateien und etwa 50 Zeilen | Agent |
| Kein offener PR, kein Phasen- oder Reservierungs-Label, keine offene Abhängigkeit | Skript |
| Kein aktiver T3-Thread | Agent |
| Beschreibung von einem Mitglied des Repos | Skript |

Protokoll: ein Kommentar am Issue vor dem Start mit `Umsetzungsreife (KI): ja|nein`, Akteur, Modell-ID, Zeitpunkt, der
Prüfliste und einem Satz Grund. Alles, was größer oder unklar ist, startet erst nach `plan-accepted` durch einen Menschen.

## Offene Entscheidungen

1. **Zielort und Zeitpunkt:** Soll der generische Teil ins Toolwerk AI Tooling, und erst nach dem Umzug? Bis dahin
   wird in TorroWorkplace nichts implementiert.
2. **Form im Tooling:** eigener Befehl, Teil von `autopilot stand` oder Skill?
3. **Zählen der Agenten:** Das Plugin zählt herdr-Tabs, T3 zählt Threads. Wie geht beides in eine Obergrenze ein?
4. **Regel 11:** Sind „Projekt-Bot“ und Dispatcher derselbe? Welche Obergrenze gilt, und bleibt „ein Start je Lauf“?
5. **Oberfläche:** Der Entwurf lässt sie nur nach `plan-accepted` zu. Der heutige Dispatcher-Prompt erlaubt sie mit
   Designer-Review. Welche Fassung gilt?
6. **Label:** Der Entwurf kommt ohne neues Label aus (`aufwand:klein` plus Protokollkommentar). Reicht das?
7. **Übergang:** Bis zur Entscheidung gelten die Prompts und Regeln unverändert. Soll der Dispatcher zwischendurch
   vorsichtiger starten (nur `aufwand:klein` oder `plan-accepted`)? Das wäre eine Änderung der Aufgabe in T3 und liegt bei dir.

## Externe Schritte, die nicht Teil dieses PR sind

Nichts davon ist geschehen:

- Dispatcher- und Scout-Prompt auf Trigger, Takt, Modellwahl und Verweise auf die Regeln kürzen, sobald Ort und Regeln
  feststehen. Supervisor und Wochenauswertung brauchen keine Änderung.
- Das Tooling (Plugin, Umzug zum Toolwerk AI Tooling) wird nicht angefasst, auch kein Feedback-Issue dorthin, solange
  der Ort offen ist.
