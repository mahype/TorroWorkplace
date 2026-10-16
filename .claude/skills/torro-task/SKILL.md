---
name: torro-task
description: Ablauf für genau eine Aufgabe aus docs/tasks/ in TorroWorkplace (z. B. "1.14", "K.13") – lesen, umsetzen, prüfen, abhaken, committen. Nutzen, wenn eine Aufgabe aus docs/tasks/ erledigt werden soll.
argument-hint: <Aufgaben-ID, z. B. K.13>
---

# Eine Aufgabe abarbeiten

Aufgabe: **$ARGUMENTS**. Die Regeln aus `CLAUDE.md` gelten immer; dieser Ablauf ergänzt sie.

## 1. Verstehen (bevor du Code anfasst)

1. Finde die Aufgabe in `docs/tasks/*.md`. Lies **Modell**, **Braucht**, **Dateien** und den Text.
2. Prüfe, dass alles unter **Braucht** abgehakt ist (`- [x]`). Wenn nicht: anhalten und melden.
3. Lies die genannten Dateien ganz, vor allem die Doc-Kommentare an den Signaturen – sie sind
   Teil der Abnahme. Lies Nachbarcode, den du aufrufst, damit du Stil und vorhandene Helfer
   kennst (`torro-gui::widgets`, `torro-gui::theme`, `torro_core::i18n`).
4. Fasse in 3–5 Zeilen zusammen, was du ändern wirst und wie du die Abnahme prüfst.
   Unklarheit bei Schnittstellen → fragen, nicht raten.

## 2. Umsetzen

- Nur die Dateien aus **Dateien**. Brauchst du eine weitere: anhalten und fragen.
- Bestehende Signaturen, Typen und Traits nicht ändern.
- Erst die Tests aus der Abnahme schreiben (neben dem Code), dann die Umsetzung.
- Nutzertexte englisch mit `tr("…")` / `trf(…)`; deutsche Übersetzungen nur sammeln (Bericht),
  nicht selbst in `de.rs` eintragen, wenn parallel gearbeitet wird.
- Geht es um Oberfläche (Iced-Views, Widgets, Theme): Skill `torro-design` laden und befolgen.
- Kleine, lesbare Funktionen; Kommentare erklären das Warum; kein `unwrap`, `todo!`, `dbg!`.

## 3. Prüfen

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
TORRO_LANG=de cargo test --workspace
```

Alle vier müssen grün sein. Bei Oberflächenänderungen zusätzlich die App mit `--demo` starten
und ansehen (siehe Skill `torro-design`, Abschnitt „Sichtprüfung“) – oder den Agenten
`torro-design-review` darum bitten.

## 4. Abschließen

1. In `docs/tasks/…` die Aufgabe abhaken (`- [x] erledigt`).
2. `git diff --stat` ansehen: Nur erlaubte Dateien plus die Aufgabenliste dürfen auftauchen.
3. Ein Commit, Conventional Commits auf Englisch, z. B. `feat(torrocalendar): month view (K.10) #188`;
   die Issue-Nummer, wenn die Aufgabe zu einem Issue gehört (`docs/git-workflow.md`).
4. Kurzer Bericht:
   - Was umgesetzt ist, wie die Abnahme geprüft wurde.
   - Neue Nutzertexte mit Vorschlag für die deutsche Übersetzung (`"Today" → "Heute"`).
   - Was offen blieb oder nicht ging – ehrlich, mit Fehlermeldung.
