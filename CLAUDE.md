# TorroWorkplace – Regeln für die Arbeit im Repository

Eine Suite kleiner Desktop-Apps um einen Kern. Zuerst entsteht der Kern `torroworkplace`: ein
Dienst mit Tray-Icon, der Workspaces des Tiling-Managers als Vorlagen speichert und lädt.
Konzept: `docs/core.md`. Arbeit läuft über GitHub-Issues; `docs/tasks/` ist die Bestandsaufnahme.

## Arbeitsweise

- Arbeite immer **genau ein Issue** ab: Die Aufgabe ist das Issue mit seinem freigegebenen Plan. Ändere
  nur die Dateien, die der Plan nennt. Brauchst du mehr, halte an und frag nach.
- **Issues gehen immer vor.** `docs/tasks/` ist eine Bestandsaufnahme dessen, was wir gern machen möchten;
  daraus entstehen bei Bedarf Issues, und dann gilt das Issue mit seinem Plan, nicht der Eintrag. Pläne
  schreiben keine Aufgaben nach `docs/tasks/`.
- Schnittstellen (Typen, Traits, Funktionssignaturen), die schon im Code stehen, sind
  festgelegt. Ändere sie nicht stillschweigend; wenn eine nicht passt, frag nach.
- `apps/workspace-setups/` ist nur **Referenz** (Python). Lies es, um zu verstehen, wie etwas
  funktioniert; übersetze es nicht Zeile für Zeile und ändere dort nichts.
- Nur bei Plänen vor #360 mit Aufgaben aus `docs/tasks/`: jede Aufgabe nach dem Skill `torro-task`
  abarbeiten und abhaken (`- [x]`), wenn ihre Abnahme erfüllt ist.
- Ohne Plan: Bei `issue-small` ist die Issue-Beschreibung die Aufgabe, bei `quick-fix` die
  Kurzbeschreibung (siehe unten).

## Issue-Workflow

Änderungen laufen über GitHub-Issues, mit den Skills des Plugins **enon-tools** (dieselben wie im
Enon-Projekt): `issue-create` → `issue-plan` → `issue-approve` (nur Menschen) → `issue-implement` →
`pr-review` (→ `pr-triage`) → `worktree-cleanup`; Abkürzungen `issue-small` und `quick-fix`, PR mit
`pullrequest` (Synonym `pr` im Repo). Board und Typen: `.enon-tools/konventionen.json`.
**Regeln des Repos** für diese Skills (Prüfen, Ergänzungen je Skill, Grenzen für `issue-small` und
`quick-fix`): [`docs/issue-workflow.md`](docs/issue-workflow.md). Namen von Issues, Branches, Commits
und PRs: [`docs/git-workflow.md`](docs/git-workflow.md). KI-Agenten setzen nie `approved`, approven
und mergen nie.

## Jede Funktion dreimal: Oberfläche, API, MCP

Konzept: `docs/api.md`. Was Menschen in der Oberfläche tun können, können Programme über die API
und Agenten über MCP. Die Logik steht nur im Dienst.

- Jede Funktion der Oberfläche ist ein `torro_ipc::Request`. Oberflächen lesen und schreiben
  nichts selbst, sie schicken Requests.
- Jeder Request hat einen Eintrag im Befehlskatalog (`torro-ipc`, `api.rs`): Name, Bereich,
  Zugriff, MCP-Tool oder `Never`. `Never` nur für die Liste „Was nie nach außen geht“ in `api.md`.
- **Planung:** Jeder Plan einer neuen Funktion nennt Oberfläche, API-Befehl und MCP-Tool, oder
  begründet, warum ein Weg fehlt. Request und Katalogeintrag kommen in den ersten Schritt.
- Geheimnisse verlassen den Dienst auf keinem Weg. Agenten verwalten nie Schlüssel, Rechte oder
  Freigaben.
- Wer einen Request anfasst, lädt den Skill `torro-api`. Nach jeder Funktion prüft der Agent
  `torro-api-review`.

## Prüfen, bevor du fertig meldest

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Alle drei müssen ohne Fehler durchlaufen. Melde ehrlich, was nicht geht.

GitHub Actions läuft nur bei Releases (Tags), jede Minute kostet Geld. Einzige Ausnahme ist die Action
„Phasen-Labels“ des Plugins (`enon-phasen-labels.yml`, einige Sekunden je geschlossenem Issue).
Die CI läuft deshalb lokal: `scripts/ci-local.sh` (dieselben Schritte wie `ci.yml`). Einmal pro Klon `git config core.hooksPath .githooks`,
dann prüft `pre-commit` Format und Design, `pre-push` alles. Starte keine Workflows auf GitHub von Hand.

## Code

- Rust 2024, Workspace-Lints: kein `unsafe`, kein `unwrap()`, kein `todo!()`, kein `dbg!()`.
  Einzige Ausnahme: `crates/torro-appkit` (nur macOS, verschiebt die Ampel der Fenster) mit
  `unsafe_code = "deny"` statt `forbid` und wenigen markierten AppKit-Aufrufen. Kein `unsafe` an anderer
  Stelle; wer nativen Plattformcode braucht, legt ihn dort ab und fragt vorher nach.
  Fehler als `Result` mit eigenem Fehler-Enum zurückgeben; `expect` nur in Tests.
- Abhängigkeiten nur über `[workspace.dependencies]` in der Wurzel-`Cargo.toml` hinzufügen und im
  Crate mit `name.workspace = true` einbinden.
- Plattformcode hinter `#[cfg(target_os = "…")]`; der Workspace muss auf Linux, macOS und
  Windows bauen.
- Code, Bezeichner, Doc-Kommentare und Commit-Nachrichten auf Englisch; Dokumente in `docs/` auf
  Deutsch.
- **Die Software ist zuerst englisch, Deutsch ist eine Übersetzung.** Jeder Text, den Nutzer sehen
  (Menüs, Knöpfe, Meldungen, Benachrichtigungen), wird englisch geschrieben und mit
  `torro_core::i18n::tr("…")` bzw. `trf("{n} windows", &[("n", &n)])` umschlossen. Die deutsche
  Übersetzung steht in `crates/torro-core/src/i18n/de.rs`. Laufen mehrere Aufgaben parallel, trägt
  nur die Koordination dort ein; die Aufgabe nennt ihre Übersetzungen im Bericht.
- Tests, die Oberflächentexte prüfen, vergleichen mit `tr("…")` statt mit festen Texten, damit sie in
  jeder Systemsprache laufen. Prüfen in beiden Sprachen: `TORRO_LANG=de cargo test --workspace`.
- Doc-Kommentar (`//!`) oben in jeder Datei: wozu sie da ist. Kommentare erklären das Warum.
- Tests neben dem Code (`#[cfg(test)] mod tests`); Testdaten unter `tests/fixtures/` des Crates.
  Tests dürfen keinen laufenden Hyprland, kein Netz und kein echtes Home-Verzeichnis brauchen
  (`TORRO_DATA_HOME` usw. auf ein Temp-Verzeichnis setzen).
- Formatierung: `rustfmt.toml` (Zeilenlänge 120).

## Commits

Conventional Commits auf Englisch, z. B. `feat(torro-wm): parse hyprland clients #188`. Commits nach
Zweck getrennt, die Issue-Nummer in jeder Message (Ausnahme: `quick-fix`).
