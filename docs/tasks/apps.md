# Aufgaben: Apps eigenständig starten und schließen (#279)

Ziel: Jede App der Suite startet direkt (Finder, Spotlight, Dock, Startmenü, Launcher), auch ohne
laufenden Dienst. Sie trägt ihr eigenes Icon, lässt sich normal schließen und läuft nur einmal.
Konzept und Entscheidungen: [lebenszyklus.md](../lebenszyklus.md). Regeln: [CLAUDE.md](../../CLAUDE.md),
Ablauf je Aufgabe: Skill `torro-task`.

Gemeinsam für alle Aufgaben:
- **Keine neuen Requests.** `apps.open_app` bleibt, wie er ist. Den Dienst starten und der Kanal
  „zweiter Start“ sind lokale Prozesse, keine Funktionen des Dienstes; `system.quit` bleibt `Never`.
  Wer doch einen Request braucht, hält an und fragt (Skill `torro-api`).
- Plattformcode hinter `#[cfg(target_os = "…")]`; was entschieden wird (Pfade, Argumente, Namen), ist eine
  reine Funktion und wird auf jedem System getestet, wie `reminders::reminder_program`.
- Kein `unsafe`, keine neue Abhängigkeit ohne Rückfrage. Tests starten keinen echten Dienst und kein
  echtes Programm (Temp-Verzeichnisse, `TORRO_*_HOME`, falsche Programme wie in `apps.rs`).
- Nutzertexte englisch mit `tr()`; Übersetzungen nennt die Aufgabe im Bericht.

Reihenfolge: AP.1 behebt das Schließen und die Neustarts und geht allein. AP.2 und AP.3 bauen darauf
auf, AP.4 auf AP.3, AP.5 auf AP.4. AP.6 (Entwürfe der Icons) ist unabhängig; AP.7 wartet auf die
Auswahl durch den Menschen. AP.8 schließt ab.

```
AP.1 (Schließen, Neustarts) ─┬─ AP.2 (eine Instanz je App) ─────────────────────────────┐
                             └─ AP.3 (Eingang: drei Programme) ─ AP.4 (Start über Bündel) ─ AP.5 ─┼─ AP.8 (Doku, Abnahme)
AP.6 (Icon-Entwürfe) ── Auswahl durch den Menschen ── AP.7 (Icons einbauen) ───────────────┘
```

## Aufgaben

### AP.1 Schließen: keine doppelten Instanzen, keine Neustarts nach Quit, keine Startschleife
- [x] erledigt
- **Modell:** Opus (Prozesse, Nebenläufigkeit, mehrere Crates) · **Braucht:** – · **Dateien:**
  `crates/torro-core/src/app.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/daemon.rs`,
  `crates/torroworkplace/src/mcp.rs`, `crates/torro-gui/src/source.rs`
- **Fenster wiederfinden (macOS, Windows):** `Core::open_app` und `show_control_centre` vergleichen heute
  `Window::class` mit `AppId::window_class`. Neu ist eine reine Funktion an `AppId`, etwa
  `AppId::owns_class(class)`, die auch die Bundle-ID `io.github.mahype.<window_class>` und den Namen der
  Programmdatei des Eintrags (ohne `.exe`) als Klasse dieses Eintrags annimmt; beide Aufrufer nutzen sie.
  Die Programmdatei liefert eine neue reine Funktion `AppId::program_file()`: `binary`, nur für Mail und
  App Feed deren Fensterklasse (`torro-inbox-mail`, `torro-inbox-notifications`, die Programme aus AP.3).
  `AppId` selbst und `binary` bleiben unverändert (`apps.list_apps` antwortet weiter mit `torroinbox`).
- **Keine Startschleife:** `ensure_core` startet den Dienst nicht, wenn ein Programm der Suite das
  innerhalb von 10 s schon versucht hat. Dafür schreibt es eine Marke (`core-start`, Zeitpunkt) ins
  State-Verzeichnis des Dienstes, neben `core.sock`, und liest sie vor jedem Start. Der Dienst öffnet
  die Einrichtung (`Core::offer_setup` in `daemon::serve`) erst, wenn sein Socket auf `system.ping`
  antwortet. Scheitert der Socket, öffnet sich nichts.
- **Quit gilt (Entscheidung 4):** Beendet ein Mensch den Dienst, schreibt der Dienst die Marke `quit`
  ins State-Verzeichnis (0600, nur der Zeitpunkt); beim nächsten Start löscht er sie. Geschrieben wird
  an genau einer Stelle für beide Wege, in `daemon::end_after_answer`: Tray › Quit (`Daemon::run`, ohne
  Schlüssel) und `Request::Quit` über den Socket (`ctl quit`, nur mit dem App-Schlüssel). Endet der Dienst
  anders (Signal, `launchctl`/`systemctl stop`, Absturz), entsteht keine Marke. `TorroMcp::start_core`
  startet nichts, solange die Marke steht; das Tool antwortet dann mit dem Code `offline` und einer
  eigenen englischen Meldung (etwa „The person stopped Torro Workplace; it starts again when they open a
  Torro app.“, ohne Pfad), nicht mit `NO_DESKTOP`. Apps und Steuerzentrale achten nicht auf die Marke:
  Wer eine App öffnet, will den Dienst.
- Tests: `program_file` und `owns_class` für alle Einträge von `AppId::ALL` (Fensterklasse, Bundle-ID,
  Programmdatei, fremde Klasse; `torroinbox` trifft nur den Hub; eine fremde Bundle-ID trifft nicht); Marke `core-start`: zweiter Versuch innerhalb von 10 s unterbleibt, danach nicht (Zeit als
  Parameter); `quit`-Marke: geschrieben nach Quit über den Tray-Weg und über den Socket, gelöscht beim
  Start, MCP startet dann nicht und antwortet `offline` (`TorroMcp` mit Temp-Verzeichnis); Einrichtung
  erst nach Antwort des Sockets (Fake-Socket, der nicht antwortet: kein Start).
- Abnahme: Tests grün. Von Hand auf macOS: Dienst starten, im Tray dreimal „Calendar“ wählen → genau ein
  Kalender, beim zweiten und dritten Mal nach vorn geholt (mit AeroSpace); Tray › Quit, eine
  Agenten-Sitzung mit Torro-MCP ruft ein Tool → der Dienst bleibt aus; Kalender öffnen → der Dienst läuft
  wieder.

### AP.2 Eine Instanz je App
- [x] erledigt
- **Modell:** Opus (lokaler Kanal, Iced-Subscription, alle Apps) · **Braucht:** AP.1 · **Dateien:**
  `crates/torro-gui/src/instance.rs` (neu), `crates/torro-gui/src/lib.rs`, `crates/torro-ipc/src/lib.rs`
  (nur `pipe::user_sid` öffentlich machen, falls nötig), `crates/torrocalendar/src/main.rs`,
  `crates/torrocalendar/src/app.rs`, `crates/torrotasks/src/main.rs`, `crates/torrotasks/src/app.rs`,
  `crates/torroinbox/src/main.rs`, `crates/torroinbox/src/app.rs`, `crates/torroassistant/src/main.rs`,
  `crates/torroassistant/src/app.rs`, `crates/torroworkplace/src/ui/mod.rs`
- `torro_gui::instance`: Vor dem ersten Fenster fragt die App, ob ihre Fensterart schon läuft. Kanal je
  `AppId::window_class`: Unix-Socket `<state>/<window_class>.instance` im State-Verzeichnis der App
  (`AppPaths::for_app`), unter Windows eine Named Pipe `\\.\pipe\torro-<window_class>-<SID>`, angelegt mit
  `first_pipe_instance(true)`. Antwortet jemand, schickt die App eine Zeile `show`, endet mit Exit 0
  und öffnet kein Fenster. Sonst lauscht sie selbst. Eine Datei ohne Zuhörer (abgestürzte App) wird
  ersetzt.
- **Sicherheit des Kanals:** Er kann nur „Fenster nach vorn“ auslösen. Die App liest höchstens eine kurze
  Zeile, nimmt genau `show` an und verwirft alles andere. Argumente, Ziele, Pfade und Umgebung gehen nie
  darüber; die Antwort an den zweiten Start enthält keinen Zustand. Unix: Socket 0600 in einem
  0700-Verzeichnis; ist der Pfad für `sun_path` zu lang (104 Byte auf macOS), startet die App ohne Kanal,
  statt zu scheitern. Windows: Pipe-Optionen wie bei `core.sock` (`reject_remote_clients`, Zugriff nur
  für den Benutzer), als Client die Security-QoS „Identification“ (`torro-ipc`, `client.rs`).
  Restrisiko für den Bericht: Wer den Namen besetzt, kann den Start nur verhindern, erfährt aber nichts.
- Auf `show` holt die laufende App ihr Fenster nach vorn: eine Subscription liefert eine Nachricht, das
  `update` antwortet mit `iced::window::gain_focus` auf das Fenster (vorher aus dem Dock geholt, falls
  minimiert).
- Gilt für: Kalender und Aufgaben ohne Ziel, jede der drei Fensterarten des Eingangs (Kanal nach ihrer
  Fensterklasse), das Hauptfenster des Assistenten, die Steuerzentrale (`torroworkplace ui` ohne
  `--template`). Nicht für Starts mit Ziel oder Helfer: `--task`, `--message`, `--compose`, `--chat`,
  `--listen`, `--voice-*`, `--reminders`, `--settings`, `--template`, `--demo` (Sichtprüfung startet
  bewusst neben dem echten Fenster).
- Tests in `instance.rs` (Unix, Temp-Verzeichnis): erster Start lauscht, zweiter bekommt „läuft schon“
  und hat `show` zugestellt, verwaiste Datei wird ersetzt, verschiedene Fensterklassen stören sich
  nicht; Windows-Tests hinter `#[cfg(windows)]`. Je App ein Test, dass die Ausnahmen keinen Kanal öffnen
  (reine Funktion über die Argumente).
- Abnahme: Tests grün, `TORRO_LANG=de` auch. Von Hand: `torrocalendar` zweimal aus dem Terminal → ein
  Fenster, beim zweiten Mal vorn; dasselbe mit `torroinbox --view mail` neben `torroinbox` (zwei
  verschiedene Fenster). Auf macOS prüfen, ob das Nach-vorn-Holen gegen die Aktivierungsregeln des
  Systems ankommt. Wenn nicht, ist das ein Befund im Bericht, kein Grund zu nativem Code.

### AP.3 Eingang: drei Programme mit eigenem Icon
- [ ] erledigt
- **Modell:** Opus (Umbau in Bibliothek, neue Crates) · **Braucht:** AP.1 · **Dateien:** `Cargo.toml`
  (Mitglieder und `[workspace.dependencies]`), `crates/torroinbox/Cargo.toml`, `crates/torroinbox/src/lib.rs`
  (neu), `crates/torroinbox/src/main.rs`, `crates/torro-inbox-mail/` (neu: `Cargo.toml`, `src/main.rs`,
  `build.rs`), `crates/torro-inbox-notifications/` (neu, ebenso), `packaging/windows/torro-inbox-mail.ico`,
  `packaging/windows/torro-inbox-notifications.ico` (neu, Platzhalter), `crates/torroworkplace/src/core.rs`,
  `crates/torroworkplace/src/apps.rs`
- Warum (Entscheidung 8): Windows gruppiert die Taskleiste nach `.exe` und zeigt das Icon, das beim Bauen
  in die `.exe` kommt; `winresource` bettet es in alle Programme eines Crates gleich ein. Getrennte Icons
  gibt es also nur mit getrennten Crates. Auf macOS liegen dieselben Programme in den Bündeln, auf Linux
  starten die `.desktop`-Dateien sie.
- `torroinbox` wird Bibliothek und Programm: die Module wandern hinter `lib.rs`, das eine Funktion zum
  Starten mit den Argumenten anbietet (der heutige Inhalt von `main`); `main.rs` ruft sie nur auf. Kein
  Verhalten ändert sich, die Tests bleiben, wo sie sind.
- Zwei neue Crates `torro-inbox-mail` und `torro-inbox-notifications`, je ein Programm gleichen Namens
  (= Fensterklasse, = `AppId::program_file()` aus AP.1): `main` setzt `--view mail` bzw.
  `--view notifications` vor die übrigen Argumente, wenn kein `--view` dabei ist, und startet die
  Bibliothek. `#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]` wie in den anderen
  Apps. `build.rs` mit `winresource` wie in `crates/torrocalendar/build.rs`, mit eigenem `.ico`
  (Platzhalter nach `packaging/windows/README.md` aus einem neuen Platzhalter-SVG; AP.7 bringt die echten) und eigenem Produktnamen (`display_name`).
- Der Dienst startet Mail und App Feed über ihr eigenes Programm, wenn es installiert ist (`Core::open_app`,
  `target_command` für `Opened::Mail`), sonst wie bisher `torroinbox --view …`. Die Antwort von
  `apps.list_apps` bleibt (`binary` = `torroinbox`); `installed` ist für Mail und App Feed wahr, wenn
  `torroinbox` oder ihr eigenes Programm da ist. Das Verfassen (`--compose`) bleibt bei `torroinbox`.
- Tests: Argumente der beiden Programme (`--view` ergänzt, ein übergebenes `--view` bleibt); Befehlszeile
  des Dienstes mit und ohne eigenes Programm, mit Ziel `--message=…`; `list_in` mit nur `torro-inbox-mail`.
- Abnahme: Tests grün; `cargo build --workspace` baut die drei Programme; `torro-inbox-mail` öffnet das
  Mail-Fenster mit der Klasse `torro-inbox-mail`.

### AP.4 macOS: Apps und Dienst aus ihren Bündeln starten
- [ ] erledigt
- **Modell:** Opus (Plattformpfade, Startwege) · **Braucht:** AP.1, AP.3 · **Dateien:**
  `crates/torro-gui/src/source.rs`, `crates/torroworkplace/src/core.rs`, `crates/torroworkplace/src/reminders.rs`,
  `crates/torroworkplace/src/apps.rs`
- Reine Funktion `torro_gui::source::bundle_program(app, exe, home, exists)`: das Programm eines Eintrags
  im Bündel `<display_name>.app/Contents/MacOS/<AppId::program_file()>`. Gesucht wird neben dem
  eigenen Bündel (`exe.ancestors().nth(4)`, wie `reminder_program`), in `/Applications` und in
  `~/Applications`. Für den Dienst gilt `Torro Workplace.app/Contents/Library/Torro Workplace Service.app/
  Contents/MacOS/torroworkplace` (Entscheidung 7). `reminder_program` nutzt dieselbe Suche.
- `ensure_core` (macOS): erst das Bündel des Dienstes, dann wie bisher `sibling`/`PATH`.
- Dienst startet Apps (`Core::open_app`, `open_app_request` mit Ziel, `apps::list_in` für `installed`):
  Gibt es das Bündel, startet er `open -a <absoluter bündelpfad>` (mit Ziel `open -n -a <pfad> --args …`),
  nie mit dem Anzeigenamen, sonst könnte das System eine fremde App gleichen Namens starten. `open` wird
  direkt per `Command` aufgerufen, ohne Shell; das System aktiviert dann eine laufende App selbst. Ohne
  Bündel wie bisher. Die Argumente nach `--args` kommen unverändert aus `target_command`, also nur vom
  Kern nachgeschlagene Werte. `open` gibt keine Umgebung weiter; nichts, was die App braucht, darf über
  Umgebungsvariablen laufen, und kein Schlüssel kommt als Argument mit. `Ok` heißt dann „Start
  übergeben“; `open` mit Exit ≠ 0 ist `error`.
- `apps.list_apps` (`apps::list_in`): `installed` ist auch wahr, wenn nur das Bündel gefunden wird;
  `binary` bleibt für Hub, Mail und App Feed `torroinbox`, der Dateiname im Bündel geht nicht in die
  Antwort. Die Katalogbeschreibung von `apps.open_app` stimmt danach auf allen Systemen und bleibt;
  ändert sie sich doch, `api.rs` und `docs/api-reference.md` (`TORRO_BLESS=1 cargo test -p torro-ipc`)
  mitnehmen.
- Linux und Windows ändern sich hier nicht weiter (ihre Programme kommen aus AP.3). Windows: `ensure_core` startet den Dienst auch dort (losgelöst, ohne
  Konsole: `CommandExt::creation_flags` mit `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`, ohne `unsafe`), und
  `CORE_START_WAIT` gilt dann für alle Systeme.
- Tests: `bundle_program` für alle Einträge und den Dienst (neben dem Bündel, `/Applications`,
  `~/Applications`, keins → `None`); Befehlszeile mit und ohne Bündel, mit Ziel (absoluter Pfad, `--args`,
  keine Umgebung); `list_in` mit nur einem Bündel (`installed`, `binary` = `torroinbox`).
- Abnahme: Tests grün. Von Hand mit den Bündeln aus AP.5 (`scripts/macos-dev-bundles.sh`).

### AP.5 Paketierung: Bündel, Starter und Einträge für alle Apps
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** AP.4 · **Dateien:** `packaging/macos/make-app.sh`,
  `.github/workflows/publish-macos-app.yml`, `packaging/linux/*.desktop` (neu:
  `torro-inbox.desktop`, `torro-inbox-mail.desktop`, `torro-inbox-notifications.desktop`,
  `torro-assistant.desktop`), `packaging/linux/icons/` (Platzhalter für die neuen Einträge),
  `.github/workflows/publish-aur.yml`, `packaging/homebrew/torro-suite.rb`, `packaging/homebrew/Brewfile`,
  `packaging/scoop/torroworkplace.json` (→ Manifest der Suite), `packaging/windows/` (neue `.ico` für Hub und
  Assistant), `crates/torroinbox/build.rs` (neu), `crates/torroassistant/build.rs` (neu),
  `crates/torroinbox/Cargo.toml`, `crates/torroassistant/Cargo.toml`, `scripts/macos-dev-bundles.sh` (neu),
  `packaging/README.md`
- `make-app.sh`: neuer Schalter, der ein fertiges Agenten-Bündel unter `Contents/Library/` des Bündels
  einbettet (für „Torro Workplace Service.app“; Bundle-ID `io.github.mahype.torro-workplace-service`).
  Workflow-Matrix: Notification Hub (`torroinbox`), Mail (`torro-inbox-mail`), App Feed
  (`torro-inbox-notifications`), Assistant (`torroassistant`), dazu das
  Service-Bündel in Torro Workplace. Bundle-ID je Eintrag `io.github.mahype.<window_class>`, Titel
  `display_name`. Das eingebettete Bündel wird vor dem äußeren signiert.
- `.desktop` je neuem Eintrag: `Exec` = Programmdatei des Eintrags (`torro-inbox-mail` usw.),
  `StartupWMClass` = Fensterklasse. AUR-Pakete und Homebrew-Formeln für Eingang (alle drei Programme) und
  Assistent.
- Windows: `build.rs` mit `winresource` für Hub und Assistent wie beim Kalender (Mail und App Feed haben
  ihres aus AP.3); Scoop-Manifest der Suite mit allen Programmen und `shortcuts` für die sieben Einträge,
  ohne Argumente.
- `scripts/macos-dev-bundles.sh`: baut aus `target/debug` alle Bündel nach `target/debug/bundles/` (oder
  in einen übergebenen Ordner), mit `make-app.sh` und der Entwicklungs-Signatur
  (`macos-dev-sign.sh`). So lässt sich alles von Hand prüfen, ohne Release.
- Abnahme: `scripts/ci-local.sh` grün; `scripts/macos-dev-bundles.sh ~/Applications/Torro\ Dev` erzeugt neun
  Bündel (sieben Apps, Workplace mit eingebettetem Dienst, Reminder); `plutil -lint` auf alle
  `Info.plist`; `desktop-file-validate` auf die `.desktop`-Dateien, falls vorhanden.

### AP.6 Icons: Entwürfe zur Auswahl
- [x] erledigt
- **Modell:** Opus (Gestaltung) · **Braucht:** – · **Dateien:** `docs/prototypes/icons/` (neu: SVGs und
  `index.html`)
- Ein einheitliches Set für Torro Workplace, Notification Hub, Mail, App Feed, Calendar, Tasks und Assistant:
  gemeinsame Grundform und Raster (1024er Fläche, macOS-Squircle-Maß), Torro-Rot als Markenfarbe nach den
  Regeln im Skill `torro-design`, je App ein klares Motiv (Lucide-nah, nicht kopiert). Zwei bis drei
  Varianten des ganzen Sets.
- `index.html` zeigt jede Variante in 16, 32, 128 und 512 px auf hellem und dunklem Grund und als
  nachgestelltes Dock.
- Abnahme: Seite öffnet ohne Netz; Bericht nennt die Varianten. **Danach hält die Umsetzung an:** Der Mensch
  wählt eine Variante (Kommentar im Issue), erst dann folgt AP.7.

### AP.7 Icons einbauen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** AP.5, AP.6 und die Auswahl · **Dateien:** `packaging/linux/icons/*.svg`,
  `packaging/macos/icons/*.png` (neu, 1024 px), `packaging/windows/*.ico`,
  `.github/workflows/publish-macos-app.yml`, `scripts/macos-dev-bundles.sh`, `packaging/windows/README.md`
- Die gewählte Variante ersetzt die Platzhalter: SVG für Linux, 1024-px-PNG für `make-app.sh` (letztes
  Argument, im Workflow und im Entwicklungs-Skript), `.ico` nach `packaging/windows/README.md`.
- Abnahme: Bündel aus `scripts/macos-dev-bundles.sh` zeigen im Finder die neuen Icons; `.ico` enthalten alle
  sieben Größen.

### AP.8 Doku und Abnahme
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** AP.1–AP.5, AP.7 · **Dateien:** `docs/lebenszyklus.md`, `docs/core.md`,
  `docs/architecture.md`, `docs/release.md`, `docs/api.md` (nur Verweis), `packaging/README.md`
- `lebenszyklus.md` auf den gebauten Stand bringen (Abschnitt „Ausgangslage“ wird „Vorher“). Autostart-Tabellen in
  `core.md` und `architecture.md` um „erste App“ und das Service-Bündel ergänzen. `release.md`: sieben Bündel,
  das Service-Bündel und die echten Icons, offene Frage 7 erledigt.
- Abnahme von Hand auf macOS mit den Entwicklungs-Bündeln (Liste in `lebenszyklus.md`, „Je System“): jede App
  aus Finder und Spotlight starten, ohne laufenden Dienst; Dock zeigt Name und Icon der App; zweiter Start
  holt sie nach vorn; ⌘Q, rotes Fenster-Symbol und Dock › Beenden schließen sie; der Dienst bleibt im Tray;
  Doppelklick auf Torro Workplace bei laufendem Dienst öffnet die Steuerzentrale. Linux und Windows soweit
  verfügbar, sonst als offen im Bericht.
