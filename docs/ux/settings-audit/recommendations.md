# Empfehlungen, Entscheidungen und Folge-Issues

**Es wurden keine Folge-Issues angelegt und nichts umgesetzt.** Diese Liste ist die Entscheidungsgrundlage. Nach Entscheidung können die Folge-Issues
(FI-xx) mit `issue-create` angelegt werden; die Beschreibung steht jeweils in einem Satz, damit sie direkt übernommen werden kann.

## Inhalt

1. [Entscheidungen für dich](#1-entscheidungen-für-dich)
2. [Priorisierte Maßnahmen](#2-priorisierte-maßnahmen)
3. [Reihenfolge](#3-vorgeschlagene-reihenfolge)
4. [Risiken und Abgrenzung](#4-risiken-und-abgrenzung)

## 1. Entscheidungen für dich

Je Entscheidung: Optionen, meine Empfehlung (**fett**), was daran hängt.

| Nr. | Frage | Optionen | Hängt daran |
|---|---|---|---|
| **E-01** | Steuerzentrale: Navigation ordnen? | **A** drei Gruppen (Connect · Access · This computer), General in die Gruppe · B flach lassen · C zweistufig mit aufklappbaren Gruppen | FI-04, M1, R3 |
| **E-02** | Einstellungssuche? | **A** eine Suche über Zentrale und Apps (⌘K) · B nur in der Zentrale · C keine (Sitemap genügt) | FI-04, R13 |
| **E-03** | Zeilenbreite auf Seiten mit Schaltern? | **A** Lesebreite 720, Rest `page_columns` (ändert #464) · B volle Breite lassen, aber Zeilen beim Überfahren hervorheben · C nichts ändern | FI-05, R4 |
| **E-04** | Konten in den Apps? | A so lassen · **B** Hub ohne eigene Accounts-Seite (Quelle zeigt Status und Link), andere Apps vorerst unverändert · C überall kompakt mit Link in die Zentrale | FI-12, R11 |
| **E-05** | Mail › Statistics? | A in den Einstellungen lassen · **B** in die Mail-Übersicht verlagern | FI-13 |
| **E-06** | Wo wohnen Einstellungen von Chat und Kontakten? | A in General lassen, nach App beschriftet, mit Reitern (M2) · **B** je App eine kleine Seite hinter dem Zahnrad (folgt der Regel „App regelt sich selbst“) | FI-10 |
| **E-07** | Rechte des Assistenten? | **A** „Used by the assistant“ in Access ziehen · B so lassen | FI-08 |
| **E-08** | Deutsche Texte vor der Umsetzung prüfen? | **A** ja: eine Prüfung der schmalen Ansicht mit `TORRO_LANG=de` vor FI-02/FI-03 · B nein | F-09 |
| **E-09** | In-App-Einstellungen: App-Seitenleiste einklappen? (F-02) | **A** ja, wie Mail · B nein | FI-02, R2 |

Keine dieser Entscheidungen ist für die P1-Maßnahmen nötig.

## 2. Priorisierte Maßnahmen

Wirkung: Wie viele Seiten bzw. Nutzer sind betroffen und wie sehr. Aufwand S/M/L wie in [findings.md](findings.md). Risiko: was kaputtgehen kann.

| Prio | Nr. | Maßnahme | Befunde | Betrifft | Wirkung | Aufwand | Risiko | Entscheidung |
|---|---|---|---|---|---|---|---|---|
| **P1** | FI-01 | Fehler beheben: Seitenaktion in schmaler Kopfzeile ins „⋯“-Menü; Abschnittsmenü ohne feste Höchstbreite; Freigaben-Text mit Ellipsis und Tooltip | F-08, F-09, F-10 | Mail, Kalender, Aufgaben, Assistent, Zentrale | hoch (sichtbare Fehler) | S | niedrig; Test mit `shots.sh` schmal hell/dunkel, Deutsch | – |
| **P1** | FI-09 | Prüfwerkzeug: `--tab <n>` für `shots.sh`, Mail-Backup-Zustände korrigieren, Apps ohne Hauptanwendung in der Demo | Lücken | alle | mittel (macht den Audit vollständig) | S | niedrig | – |
| P2 | FI-02 | Einstellungsmodus: App-Seitenleiste klappt zur Leiste ein | F-02 | Kalender, Aufgaben, Assistent, Hub | hoch für Orientierung | M | mittel: Zustand merken, Sprung zurück | E-09, E-08 |
| P2 | FI-03 | Kleine Seiten zusammenlegen: Kalender „Reminders and time“, Hub „General“ | F-03, F-05 | Kalender, Hub | mittel | S–M | niedrig (Routen `--settings night`, `working-hours` bleiben als Alias, wie bei `inbox`→General) | – |
| P2 | FI-04 | Steuerzentrale: Gruppen in der Navigation, Einstellungssuche | F-01, F-24 | alle | hoch (Auffindbarkeit) | M | mittel: Index aller Zeilen pflegen | E-01, E-02 |
| P2 | FI-05 | Lesebreite für Zeilen mit Bedienelement, `page_columns` auf breiten Fenstern | F-14, F-15 | alle Seiten mit Schaltern | mittel–hoch | S–M | mittel (ändert #464, viele Bilder) | E-03 |
| P2 | FI-06 | Zeilenaktionen vereinheitlichen; Automation auf Reiter; Signatur über `save_bar` | F-06, F-07, F-16 | Hub, Assistent, Mail | mittel | S–M | niedrig | – |
| P2 | FI-07 | Begriffsliste umsetzen (Fußlink, Reiter „Details“, Titel Hub) | F-05, F-17 | alle | mittel | S | niedrig; Übersetzungen in `de.rs` | – |
| P2 | FI-08 | Rechte des Assistenten zusammenführen, Skills-Hinweis entfernen | F-18, F-19 | Assistent | mittel | M | mittel (Rechte, `torro-api` prüfen) | E-07 |
| P2 | FI-10 | General: Reiter, „Advanced“; Heimat für Chat und Kontakte | F-11, F-12, F-13 | Zentrale, Chat, Kontakte | mittel | S–M | niedrig–mittel | E-06 |
| P2 | FI-11 | MCP-servers-Katalog: eine Statuspille, kürzere Beschreibung | F-26 | Zentrale | mittel | M | niedrig | – |
| P3 | FI-12 | Hub: „Accounts“ entfällt, Quelle zeigt Konto-Status | F-04 | Hub | mittel | M | mittel (mit #946 abstimmen) | E-04 |
| P3 | FI-13 | Mail: Statistics in die Mail-Übersicht | F-20 | Mail | niedrig | M | mittel | E-05 |
| P3 | FI-14 | Feinschliff: Leerzustände, Ersteinrichtung-Fuß, Konto-Detail-Kopf, Categorization-Zeilen | F-21 bis F-23, F-25, F-27 | verstreut | niedrig | S–M | niedrig | – |
| P3 | – | MCP-Client-Seite mit Reitern (**bereits #687**) | F-28 | Zentrale | mittel | M | niedrig | – (Issue existiert) |
| P3 | – | General › Reiter, Maps schmal (**bereits #548**) | F-11, F-12 | Zentrale | niedrig | S | niedrig | – (offene Punkte in #548) |

## 3. Vorgeschlagene Reihenfolge

1. **FI-01 und FI-09** sofort: klein, ohne Entscheidung, machen den Fehlerstand sauber und das Prüfwerkzeug vollständig.
2. Entscheidungen **E-01, E-02, E-03, E-09** treffen. Sie bestimmen FI-02, FI-04 und FI-05, die zusammen das Gerüst ändern.
3. **FI-02 + FI-03** (Gerüst in Apps), danach **FI-04** (Zentrale), danach **FI-05** (Breite) – jeweils mit gezielter Sichtprüfung (Skill, Abschnitt 9), nicht der volle Lauf, außer FI-05 (Tokens/Layout, voller Lauf nach Skill).
4. Inhaltliche Umbauten (FI-06 bis FI-11) parallel möglich, je App ein PR.
5. FI-12 und FI-13 nach den Entscheidungen E-04 und E-05.

Nichts davon ist eine großflächige Umsetzung in dieser Phase; es wurde kein Code geändert.

## 4. Risiken und Abgrenzung

- **Zielkonflikt mit #464** (volle Breite): F-14 fordert eine Lesebreite für Schalter-Zeilen. Wenn E-03 mit „C“ entschieden wird, bleiben F-14 und F-15 bewusst offen.
- **Zielkonflikt mit #410/#663** (Konten in jeder App): E-04 verändert nur den Hub; der Rest bleibt, bis es einen Anlass gibt.
- **Demo-Daten sind keine Echtdaten**: Doppelte „Needs your attention“-Zeile in Overview und die ausgegrauten Schalter folgen aus den Beispieldaten und sind nicht bewertet.
- **Reproduzierbarkeit**: `capture.sh` erzeugt alle Bilder neu; ändert sich die Oberfläche, wird das Inventar veraltet. Der Audit gilt für den Stand von `main` am 2026-11-23 (Commit `7789963d`).
- **Nicht geprüft**: siehe Lücken im [Inventar](inventory.md#lücken-nicht-erreichbar-oder-nicht-aufgenommen) und offene Prüfungen in [findings.md](findings.md#6-offene-prüfungen).
