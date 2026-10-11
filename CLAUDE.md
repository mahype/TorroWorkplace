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

## Prüfen, bevor du fertig meldest

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Alle drei müssen ohne Fehler durchlaufen. Melde ehrlich, was nicht geht.

## Code

- Rust 2024, Workspace-Lints: kein `unsafe`, kein `unwrap()`, kein `todo!()`, kein `dbg!()`.
  Fehler als `Result` mit eigenem Fehler-Enum zurückgeben; `expect` nur in Tests.
- Abhängigkeiten nur über `[workspace.dependencies]` in der Wurzel-`Cargo.toml` hinzufügen und im
  Crate mit `name.workspace = true` einbinden.
- Plattformcode hinter `#[cfg(target_os = "…")]`; der Workspace muss auf Linux, macOS und
  Windows bauen.
- Code, Bezeichner, Doc-Kommentare und Commit-Nachrichten auf Englisch; Dokumente in `docs/` auf
  Deutsch.
- Doc-Kommentar (`//!`) oben in jeder Datei: wozu sie da ist. Kommentare erklären das Warum.
- Tests neben dem Code (`#[cfg(test)] mod tests`); Testdaten unter `tests/fixtures/` des Crates.
  Tests dürfen keinen laufenden Hyprland, kein Netz und kein echtes Home-Verzeichnis brauchen
  (`TORRO_DATA_HOME` usw. auf ein Temp-Verzeichnis setzen).
- Formatierung: `rustfmt.toml` (Zeilenlänge 120).

## Commits

Conventional Commits auf Englisch, z. B. `feat(torro-wm): parse hyprland clients`. Ein Commit pro
Aufgabe.
