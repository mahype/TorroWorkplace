# TorroWorkplace – Regeln für die Arbeit im Repository

Eine Suite kleiner Desktop-Apps um einen Kern. Zuerst entsteht der Kern `torroworkplace`: ein
Dienst mit Tray-Icon, der Workspaces des Tiling-Managers als Vorlagen speichert und lädt.
Konzept: `docs/core.md`. Aufgaben: `docs/tasks/`.

## Arbeitsweise

- Arbeite immer **genau eine Aufgabe** aus `docs/tasks/` ab. Ändere nur die Dateien, die die
  Aufgabe nennt. Brauchst du mehr, halte an und frag nach.
- Schnittstellen (Typen, Traits, Funktionssignaturen), die schon im Code stehen, sind
  festgelegt. Ändere sie nicht stillschweigend; wenn eine nicht passt, frag nach.
- `apps/workspace-setups/` ist nur **Referenz** (Python). Lies es, um zu verstehen, wie etwas
  funktioniert; übersetze es nicht Zeile für Zeile und ändere dort nichts.
- Hake die Aufgabe in `docs/tasks/` ab (`- [x]`), wenn ihre Abnahme erfüllt ist.

## Jede Funktion dreimal: Oberfläche, API, MCP

Konzept: `docs/api.md`. Was Menschen in der Oberfläche tun können, können Programme über die API
und Agenten über MCP. Die Logik steht nur im Dienst.

- Jede Funktion der Oberfläche ist ein `torro_ipc::Request`. Oberflächen lesen und schreiben
  nichts selbst, sie schicken Requests.
- Jeder Request hat einen Eintrag im Befehlskatalog (`torro-ipc`, `api.rs`): Name, Bereich,
  Zugriff, MCP-Tool oder `Never`. `Never` nur für die Liste „Was nie nach außen geht“ in `api.md`.
- **Planung:** Jede neue Funktion in `docs/tasks/` nennt Oberfläche, API-Befehl und MCP-Tool,
  oder begründet, warum ein Weg fehlt. Request und Katalogeintrag kommen in die erste Aufgabe.
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

GitHub Actions läuft nur bei Releases (Tags), jede Minute kostet Geld. Die CI läuft deshalb lokal:
`scripts/ci-local.sh` (dieselben Schritte wie `ci.yml`). Einmal pro Klon `git config core.hooksPath .githooks`,
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

Conventional Commits auf Englisch, z. B. `feat(torro-wm): parse hyprland clients`. Ein Commit pro
Aufgabe.
