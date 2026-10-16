# Aufgaben: Fenster starten und laden schnell

Ziel: Jedes Fenster der Suite steht praktisch sofort und zeigt seine Daten aus dem lokalen Speicher, auch wenn
der Dienst erst mit ihm startet. Konzept, Zielwerte und Messwerte: [tempo.md](../tempo.md). Issue: #206.
Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`.

Gemeinsam für alle Aufgaben:
- Keine neuen Requests, kein Eingriff in den Befehlskatalog. Die Apps nutzen `system.ping`, das es schon gibt;
  **Oberfläche, API und MCP** bleiben gleich, API und MCP profitieren nur vom schnelleren Dienst.
- Keine neue Abhängigkeit. Zeitmarken gehen über `eprintln!`, wie die übrigen Meldungen der Suite.
- Erst messen, dann ändern: TP.1 vor allem anderen, TP.4 misst nachher mit demselben Skript.
- Zeitmarken sind Diagnose auf stderr, kein Nutzertext: kein `tr()`. Sie nennen nie Inhalte (Betreff, Absender,
  Ordnernamen), nur Namen von Schritten, Anzahlen und Dauern.

```
TP.1 (Zeitmarken, Messskript, Messung vorher) ─┬─ TP.2 (Dev-Profil)  ─┬─ TP.4 (Messung nachher, Abnahme)
                                               └─ TP.3 (Apps warten) ─┘
```

## Aufgaben

### TP.1 Zeitmarken und Messskript, Messung vorher
- [x] erledigt
- **Modell:** Opus (viele Crates, Skript mit Fenstererkennung) · **Braucht:** – · **Dateien:**
  `crates/torro-core/src/{lib,timing}.rs`, `crates/torroinbox/src/{main,app,compose}.rs`,
  `crates/torrocalendar/src/{main,app}.rs`, `crates/torrotasks/src/{main,app}.rs`,
  `crates/torroassistant/src/{main,view}.rs`, `crates/torroworkplace/src/{main,daemon}.rs`,
  `crates/torroworkplace/src/ui/mod.rs`, `scripts/measure-start.sh`, `docs/tempo.md`
- Neues Modul `torro_core::timing`: tut nichts, solange `TORRO_TIMING` nicht gesetzt oder leer ist (einmal
  gelesen). Gesetzt schreibt es Zeilen `torro-timing <programm> +<ms>ms <schritt>` auf stderr, die Zeit ab
  `timing::start()`. Funktionen: `start()` (ganz oben in `main`), `mark(schritt)`, `once(schritt)` (nur beim
  ersten Aufruf je Schritt) und `slow(schritt, seit: Instant)` (nur ab 16 ms, mit Dauer). Tests: ohne Variable
  keine Ausgabe, `once` nur einmal; die Ausgabe über einen austauschbaren Schreiber prüfen, nicht über stderr.
- Marken je App (`torroinbox` mit Mail, Mitteilungen, Hub und Compose, `torrocalendar`, `torrotasks`,
  `torroassistant`, Steuerzentrale `torroworkplace ui`): `start` in `main`, `first frame` per `once` am Anfang
  von `view`, `first data` per `once` beim ersten erfolgreichen Laden (Inbox: `Message::Loaded` mit `Ok` in
  `App::handle`), `load` mit Dauer bei jedem Laden, `slow` um `update` und `view`.
- Dienst (`daemon.rs`, `serve`): `mark` nach jedem Schritt vor dem Socket (`clients`, `store`, `designs`,
  `pim`, `core`), dann `socket task started`.
- `scripts/measure-start.sh <app> [args…] [--cold] [--runs N]`: startet die App N-mal (Standard 5) mit
  `TORRO_TIMING=1`, beendet sie nach der Marke `first data` (spätestens nach 15 s) und gibt je Lauf die Marken
  `first frame`, `first data` und die Antwort des Dienstes aus, dazu den Median. `--cold` beendet vorher den
  Dienst (`torroworkplace ctl quit`) und wartet, bis der Socket weg ist. Nimmt die Programme aus
  `target/debug` oder mit `--release` aus `target/release`.
- Messen und in `docs/tempo.md` unter „Vorher und nachher“ eintragen: alle fünf Apps, warm und kalt, Debug und
  Release, dazu die Dauer von `load`, `update` und `view` beim Neuladen des Mail-Fensters (Timer abwarten).
- Abnahme: Tests grün; ohne `TORRO_TIMING` keine zusätzliche Ausgabe; `docs/tempo.md` enthält die Tabelle
  „vorher“ für alle fünf Apps.

### TP.2 Dev-Profil: Abhängigkeiten optimiert bauen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** TP.1 · **Dateien:** `Cargo.toml`, `docs/tempo.md`
- In der Wurzel-`Cargo.toml` ein Abschnitt `[profile.dev.package."*"]` mit `opt-level = 2`, mit Kommentar
  warum (Iced, wgpu, Blitz, SQLite im Debug-Build vielfach langsamer). Der eigene Code des Workspaces bleibt
  ohne Optimierung und debugbar; `[profile.dist]` und `release` bleiben unverändert.
- Mit `scripts/measure-start.sh` die Debug-Werte (warm, Mail-Fenster und Kalender) neu messen und in
  `docs/tempo.md` als eigene Zeile „Debug, Abhängigkeiten optimiert“ eintragen.
- Abnahme: `cargo build --workspace` und die Prüfbefehle laufen; Debug-Werte warm erreichen die Zielwerte.

### TP.3 Apps warten beim Start kurz auf den Dienst
- [x] erledigt
- **Modell:** Opus (alle Apps, Startablauf) · **Braucht:** TP.1 · **Dateien:** `crates/torro-gui/src/source.rs`,
  `crates/torroinbox/src/{app,compose}.rs`, `crates/torrocalendar/src/app.rs`, `crates/torrotasks/src/app.rs`,
  `crates/torroassistant/src/app.rs`, `crates/torroworkplace/src/ui/mod.rs`, `docs/tempo.md`
- Heute startet `ensure_core` den Dienst und die App fragt sofort; die Antwort fehlt, das Fenster zeigt einen
  Fehler und fragt erst mit dem nächsten Timer wieder (Inbox 10 s). Neu in `torro_gui::source`:
  `pub async fn wait_for_core(socket: &Path, limit: Duration) -> bool`, fragt `Request::Ping` alle 50 ms, bis
  der Dienst antwortet oder `limit` um ist; Konstante `CORE_START_WAIT` = 10 s. Dazu `Source::ready(&self)`:
  bei `Source::Demo` sofort, bei `Source::Core` `wait_for_core` mit `CORE_START_WAIT`. Die Signatur von
  `ensure_core` bleibt.
- Das erste Laden jeder App wartet zuerst darauf (Task, nicht blockierend): `torroinbox` `App::new` (vor
  `app.load()`) und `compose::Window::new`, `torrocalendar` `App::new`, `torrotasks` `App::new`,
  `torroassistant` `App::new`, Steuerzentrale `Ui::new`. Während des Wartens zeigt das Fenster seinen
  Ladezustand, keinen Fehler. Antwortet der Dienst nach `CORE_START_WAIT` nicht, geht es weiter wie heute
  (Fehler, Timer fragt wieder). Spätere Ladevorgänge (Timer, Refresh) warten nicht.
- Nicht ändern: `torro_gui::design::start` (wartet schon höchstens 1 s, ein Kaltstart beginnt mit dem
  Standard-Design und wechselt beim ersten Abfragen; das bleibt).
- Tests in `source.rs`: `wait_for_core` gibt `true`, sobald ein Test-Socket antwortet (Server startet nach
  einer kurzen Pause), und `false` nach `limit` ohne Server; Socket unter einem Temp-Verzeichnis. In der Inbox
  ein Test, dass der Start mit Demo-Quelle ohne Wartezeit lädt.
- Kaltstart messen (`measure-start.sh --cold`, alle fünf Apps, Release) und in `docs/tempo.md` eintragen.
- Abnahme: Prüfbefehle grün; Kaltstart in Release erreicht die Zielwerte; Sichtprüfung des Ladezustands im
  Mail-Fenster beim Kaltstart (kein Fehlerhinweis).

### TP.4 Messung nachher und Abnahme
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** TP.2, TP.3 · **Dateien:** `docs/tempo.md`
- Alle Messungen aus TP.1 wiederholen und die Tabelle „nachher“ neben „vorher“ stellen. Für jeden Zielwert
  aus `docs/tempo.md` vermerken: erreicht oder nicht, mit Wert.
- Neuladen: Mail-Fenster mit allen offenen Einträgen, Release, über mindestens drei Timer-Runden; keine
  `slow`-Marke über 50 ms. Ist eine darüber, Wert und Schritt in `docs/tempo.md` eintragen und im Bericht als
  Folge-Issue vorschlagen, nicht selbst umbauen.
- Liegt der Dienst beim Kaltstart vor dem Socket über 300 ms (Marken aus `serve`), den größten Schritt im
  Bericht nennen und als Folge-Issue vorschlagen; nicht in dieser Aufgabe umbauen.
- Abnahme: `docs/tempo.md` vollständig (vorher, nachher, erreicht ja/nein); Prüfbefehle grün.
