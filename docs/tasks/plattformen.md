# Aufgaben: macOS und Windows auf den Stand von Linux

Ziel: Kern und Apps laufen auf macOS und Windows wie unter Linux. Heute (2026-10-11) ist Linux mit
Hyprland die Referenz. macOS fehlen Tray, Fenstermanager-Anbindung und Autostart. Windows fehlt dazu die
Verbindung zwischen Apps und Dienst: `torro-ipc` kennt nur Unix-Sockets, Kalender und Aufgaben laufen dort
nur mit `--demo`. Hintergrund: [roadmap.md](../roadmap.md) (Phase 2), [release.md](../release.md)
(Abschnitte macOS und Windows). Regeln: [CLAUDE.md](../../CLAUDE.md), Ablauf je Aufgabe: Skill `torro-task`.

Gemeinsam für alle Aufgaben:
- Plattformcode hinter `#[cfg(target_os = "…")]`; Linux darf sich nicht ändern. Die CI baut und testet auf
  allen drei Systemen, Tests für ein System laufen nur dort (`#[cfg(windows)]`, `#[cfg(target_os = "macos")]`).
- Keine Aufgabe hier bringt neue Requests. Tray-Menüs und Fenstermanager nutzen die vorhandenen Befehle aus
  dem Katalog; **Oberfläche, API und MCP** bleiben gleich, nur das Verhalten erreicht weitere Systeme. Wer
  doch einen Request braucht, hält an und fragt (Skill `torro-api`).
- Neue Abhängigkeiten nur über `[workspace.dependencies]` und nur für das System, das sie braucht
  (`[target.'cfg(windows)'.dependencies]`).

Reihenfolge: **X.1** öffnet Windows für alles Weitere. X.2, X.3 und X.9 sind unabhängig. Tray (X.4) und
Fenstermanager (X.6, X.7) sind die großen Pakete. X.10 und X.11 schließen ab.

```
X.1 (Named Pipes) ──┬─ X.4 (Tray macOS + Windows) ── X.5 (Benachrichtigungen)
                    └──────────────────────────────┐
X.6 (AeroSpace) ─┬─ X.8 (Startbefehle)             ├─ X.10 (Windows-Signatur) ── X.11 (Abnahme)
X.7 (GlazeWM) ───┘                                 │
X.2 (Konsolenfenster), X.3 (Autostart), X.9 (Paket) ┘
```

## Aufgaben

### X.1 Windows: Apps und Dienst sprechen über Named Pipes
- [x] erledigt
- **Modell:** Opus (Sicherheit, Nebenläufigkeit) · **Braucht:** – · **Dateien:** `crates/torro-ipc/src/{lib,client,server}.rs`,
  `crates/torro-ipc/Cargo.toml`, `Cargo.toml` (nur `[workspace.dependencies]`)
- Heute geben `client::call_with_key` und `server::serve_with_key` außerhalb von Unix `IpcError::Unsupported`
  zurück. Unter Windows sprechen sie dasselbe Protokoll (eine JSON-Zeile hin, eine zurück) über eine Named Pipe,
  mit `tokio::net::windows::named_pipe`.
- **Name:** fest je Benutzer, z. B. `\\.\pipe\torro-workplace-<Benutzer-SID>`. `socket_path()` bleibt als
  Funktion; unter Windows liefert sie den Pipe-Namen (Doc-Kommentar anpassen). `app.key` bleibt im
  State-Verzeichnis, das Windows schon nur für den Benutzer lesbar hält.
- **Sicherheit (wie der Unix-Socket mit 0600):** Die Pipe bekommt eine Zugriffsliste nur für den eigenen
  Benutzer (Security Descriptor, nicht der Standard, der „Jeder“ lesen lässt). Sie wird mit
  `first_pipe_instance(true)` angelegt, damit kein fremder Prozess den Namen vorher besetzen und Anfragen
  samt App-Schlüssel abfangen kann. Remote-Clients werden abgelehnt (`reject_remote_clients`). Die
  Schlüsselprüfung im Kern bleibt unverändert.
- „Läuft schon“ (`Ping` vor dem Start in `daemon.rs`) funktioniert wie unter Unix.
- Abnahme: Windows-Tests: Hin und zurück, mehrere Clients nacheinander und gleichzeitig, zweiter Server auf
  demselben Namen schlägt fehl, Client ohne laufenden Dienst bekommt `NotRunning`. Auf einem Windows-Rechner
  oder in der CI: `torroworkplace daemon --headless`, dann `torroworkplace status`, dann `torrocalendar` ohne
  `--demo` mit Konto.

### X.1b Windows: Client prüft den Besitzer der Pipe
- [x] erledigt
- **Modell:** Opus (Sicherheit, FFI) · **Braucht:** X.1 · **Dateien:** `crates/torro-ipc/src/{lib,client}.rs`,
  `crates/torro-ipc/Cargo.toml`, `Cargo.toml` (nur `[workspace.dependencies]`), `docs/api.md`, `docs/architecture.md`
- Der Client verbindet sich mit `\\.\pipe\torro-workplace-<SID>` und schickt dann den App-Schlüssel. Läuft der
  Kern nicht, könnte ein Prozess eines anderen Benutzers den Namen besetzt haben und Anfrage samt Schlüssel
  empfangen. Darum prüft der Client vor dem Senden, wem der Server-Prozess der Pipe gehört:
  `GetNamedPipeServerProcessId` auf dem Client-Handle → SID dieses Prozesses (`sysinfo`) → Vergleich mit der
  eigenen SID. Stimmt sie nicht oder ist sie nicht feststellbar: nichts senden, `IpcError::Io` mit
  `PermissionDenied` (wie der Kern, wenn ein fremdes Programm den Namen hält).
- `interprocess` liefert die Server-PID zwar sicher, öffnet die Pipe aber ohne Security-QoS (der Server dürfte
  den Benutzer imitieren); der Client bleibt deshalb bei tokio, und die PID kommt über einen einzigen
  FFI-Aufruf (`windows-sys`). Dafür hat `torro-ipc` eine eigene Lint-Tabelle mit `unsafe_code = "deny"` und
  genau eine `#[allow(unsafe_code)]`-Stelle mit `// SAFETY:`, nur unter Windows.
- `docs/api.md` und `docs/architecture.md`: Windows nutzt Named Pipes (Name, Zugriffsliste nur für den
  Benutzer, erste Instanz exklusiv, keine Remote-Clients, Client prüft den Besitzer).
- Abnahme: Vergleichslogik als plattformneutrale Funktion mit Test (gleiche SID ja; fremde, fehlende oder
  leere SID nein). Windows-Test (CI): Verbindung zum eigenen Server besteht die Prüfung. `cargo clippy -p
  torro-ipc --all-targets --target x86_64-pc-windows-msvc -- -D warnings` grün.

### X.2 Windows: kein Konsolenfenster neben den Apps
- [x] erledigt
- **Modell:** Opus (Weiche für den Kern) · **Braucht:** – · **Dateien:** `crates/torrocalendar/src/main.rs`,
  `crates/torrotasks/src/main.rs`, `crates/torroworkplace/src/main.rs`, `Cargo.toml` und
  `crates/torroworkplace/Cargo.toml` (falls `windows-sys` nötig)
- Kalender und Aufgaben: `#![cfg_attr(windows, windows_subsystem = "windows")]`. Fehlermeldungen beim Start
  gehen dann nicht mehr verloren: als Dialog oder Log-Datei (Weg im Bericht nennen).
- `torroworkplace` ist zugleich Oberfläche (`ui`, ohne Befehl) und Werkzeug für die Kommandozeile (`status`,
  `ctl`, `mcp`, `daemon`). Mit Subsystem „windows“ hängt sich das Programm bei Befehlen an die Konsole des
  Aufrufers (`AttachConsole(ATTACH_PARENT_PROCESS)`), damit `torroworkplace status` in PowerShell weiter
  Ausgaben zeigt. `mcp` muss über umgeleitete stdin/stdout funktionieren (Start durch einen MCP-Client).
- Abnahme: Auf Windows öffnet ein Doppelklick auf die drei `.exe` kein Konsolenfenster; `torroworkplace status`
  und `torroworkplace --version` geben in PowerShell und `cmd` Text aus; `torroworkplace mcp` antwortet auf
  `initialize`, wenn ein Programm es über Pipes startet. Linux und macOS unverändert.

### X.3 Start beim Anmelden auf macOS und Windows
- [x] erledigt
- **Modell:** Sonnet · **Braucht:** – · **Dateien:** `crates/torroworkplace/src/settings_service.rs`
- „Start at login“ ist heute nur ein XDG-Autostart-Eintrag (Linux). macOS: LaunchAgent
  `~/Library/LaunchAgents/io.github.mahype.torro-workplace.plist` (`RunAtLoad`, Programm `torroworkplace daemon`,
  Pfad des laufenden Programms). Windows: Wert `TorroWorkplace` unter
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Ausschalten entfernt den Eintrag.
- Der Pfad zeigt auf das Programm, das gerade läuft; liegt es in einer `.app`, auf das Programm im Bündel.
- Abnahme: Tests mit Temp-Verzeichnis (Inhalt der plist) bzw. hinter `#[cfg(windows)]` (Wert setzen und
  löschen, eigener Testschlüssel statt `Run`). Einmal von Hand: einschalten, ab- und anmelden, der Dienst läuft.

### X.4 Tray auf macOS und Windows
- [ ] erledigt
- **Modell:** Opus (Ereignisschleife, Threads) · **Braucht:** X.1 (für Windows; macOS kann vorher) · **Dateien:**
  `crates/torroworkplace/src/tray/{mod,linux,desktop}.rs` (neu: `desktop.rs`), `crates/torroworkplace/src/tray/icon.rs`,
  `crates/torroworkplace/src/daemon.rs`, `crates/torroworkplace/Cargo.toml`, `Cargo.toml`
- Das Paket `tray-icon` (mit `muda` für Menüs) deckt macOS und Windows gemeinsam ab. Es verlangt die
  Ereignisschleife im Haupt-Thread (macOS zwingend). Der Dienst läuft dann so: Haupt-Thread hat die Schleife,
  tokio läuft daneben, Menü-Klicks gehen als Requests über `Daemon::run`, wie bei `ksni`.
- **Ein Menü für alle Systeme:** Aufbau und Texte des Menüs aus `linux.rs` in ein gemeinsames Modell ziehen
  (Einträge, Untermenüs, aktiv/inaktiv), das `ksni` und `tray-icon` nur noch darstellen. Linux sieht danach
  genauso aus wie vorher.
- macOS: Vorlagen-Symbol (`template`), damit es sich der Menüleiste in hell und dunkel anpasst; kein
  Dock-Symbol für den Dienst (`ActivationPolicy::Accessory`). Windows: Symbol im Infobereich, Linksklick
  öffnet das Menü.
- Danach `#[cfg_attr(not(target_os = "linux"), allow(dead_code))]` an `Daemon` entfernen; `--headless`
  schaltet weiter das Tray ab.
- Abnahme: Test für das Menü-Modell (gleiche Einträge wie bisher unter Linux). Sichtprüfung auf macOS (hell,
  dunkel) und Windows: Menü öffnen, Vorlage speichern und laden (mit X.6/X.7) bzw. ohne Fenstermanager den
  Hinweis sehen, Steuerzentrale öffnen, Beenden.

### X.5 Benachrichtigungen auf macOS und Windows
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** X.4 · **Dateien:** `crates/torroworkplace/src/notify.rs`, ggf.
  `packaging/macos/Info.plist.in` (nennen)
- `notify-rust` nutzt auf macOS das Benachrichtigungssystem nur sauber aus einem Bündel mit Bundle-ID, auf
  Windows braucht ein Toast eine AppUserModelID (sonst erscheint „PowerShell“ als Absender oder nichts).
  Prüfen und richtig setzen: Absender „Torro Workplace“ mit eigenem Symbol.
- Abnahme: Auf beiden Systemen erscheint nach „Vorlage gespeichert“ eine Benachrichtigung mit richtigem
  Namen; ohne Berechtigung (macOS) fällt der Dienst nicht um.

### X.6 Fenstermanager AeroSpace (macOS)
- [x] erledigt
- **Modell:** Opus · **Braucht:** – · **Dateien:** `crates/torro-wm/src/aerospace/` (neu), `crates/torro-wm/src/{lib,detect}.rs`,
  `crates/torro-wm/tests/fixtures/aerospace-*.json` (neu)
- Neues Backend neben `hyprland/`, das `WindowManager` umsetzt. AeroSpace wird über seine Kommandozeile
  gesteuert (`aerospace list-windows --json`, `list-workspaces`, `list-monitors`, `focus --window-id`,
  `move-node-to-workspace`, `close`). Für Ereignisse vor dem Bau prüfen, was die aktuelle AeroSpace-Version
  anbietet (Abo über die Kommandozeile oder `exec-on-workspace-change`); sonst Abfrage im Takt mit Vergleich.
  Entscheidung im Bericht begründen.
- `detect` erkennt AeroSpace (läuft der Server, antwortet `aerospace list-workspaces --focused`).
  `Backend` bekommt die Variante `AeroSpace`.
- `prepare_insert` und `place`: AeroSpace kennt Baum-Layouts wie i3; was nicht geht, gibt `Unsupported`
  zurück, und die Wiederherstellung fällt auf „ans Ende“ zurück (wie bei Hyprland ohne Nachbar).
- Abnahme: Tests mit Fixtures für Fenster, Workspaces, Monitore und mit einem Fake-`aerospace` (Programm im
  Temp-Verzeichnis über eine Umgebungsvariable), damit die Tests ohne AeroSpace laufen. Von Hand: Vorlage
  speichern und laden auf einem Mac mit AeroSpace.

### X.7 Fenstermanager GlazeWM (Windows)
- [x] erledigt
- **Modell:** Opus · **Braucht:** – (für den Dienst unter Windows: X.1) · **Dateien:** `crates/torro-wm/src/glazewm/` (neu),
  `crates/torro-wm/src/{lib,detect}.rs`, `crates/torro-wm/tests/fixtures/glazewm-*.json` (neu), `crates/torro-wm/Cargo.toml`,
  `Cargo.toml`
- GlazeWM hat eine WebSocket-Schnittstelle auf `localhost` (Standard Port 6123): Abfragen (`query windows`,
  `query workspaces`, `query monitors`), Befehle (`command focus --container-id …`, Verschieben, Schließen)
  und Ereignis-Abos (`sub --events …`). Neues Backend, das `WindowManager` darüber umsetzt; WebSocket-Client
  als neue Abhängigkeit nur für Windows (Wahl im Bericht begründen).
- Befehle und Antwortformat vor dem Bau gegen die aktuelle GlazeWM-Version (v3) prüfen; Fixtures aus echten
  Antworten.
- `detect` erkennt GlazeWM (Port antwortet). `Backend` bekommt die Variante `GlazeWm`.
- Abnahme: Tests gegen einen Fake-WebSocket-Server mit den Fixtures (Abfragen, Befehl mit Erfolg und mit
  Fehler, Ereignisse). Von Hand auf Windows mit GlazeWM: Vorlage speichern und laden.

### X.8 Startbefehle für Fenster auf macOS und Windows
- [ ] erledigt
- **Modell:** Sonnet · **Braucht:** X.6, X.7 · **Dateien:** `crates/torro-setups/src/launch/{mod,macos,windows}.rs` (neu: `macos.rs`,
  `windows.rs`)
- `command_for(window)` ist heute nur für Linux echt (liest `/proc`). Damit gespeicherte Vorlagen fehlende
  Programme wieder starten können: macOS über den Prozess zur PID (Pfad des Programms; liegt es in einer
  `.app`, `open -a <Bündel>`), Windows über `QueryFullProcessImageName` und die Kommandozeile des Prozesses.
- Abnahme: Tests mit dem eigenen Testprozess (PID bekannt) auf dem jeweiligen System. Von Hand: Vorlage mit
  einem geschlossenen Programm laden, es startet und landet am richtigen Platz.

### X.9 Windows-Paket: Symbol und Versionsangaben in der `.exe`
- [x] erledigt
- **Modell:** Haiku · **Braucht:** – · **Dateien:** `crates/{torroworkplace,torrocalendar,torrotasks}/build.rs` (neu),
  deren `Cargo.toml`, `packaging/windows/` (neu: `.ico` aus `packaging/linux/icons/*.svg`), `Cargo.toml`
- Jede `.exe` bekommt Symbol, Produktname, Version und Hersteller als Windows-Ressource (z. B. mit
  `winresource` als Build-Abhängigkeit nur für Windows), damit Explorer, Taskleiste und SmartScreen den
  richtigen Namen zeigen.
- Abnahme: Auf Windows zeigen „Eigenschaften → Details“ und die Taskleiste Name, Version und Symbol. Linux-
  und macOS-Build unverändert.

### X.10 Windows-Signatur
- [ ] erledigt
- **Modell:** Opus, mit dem Maintainer · **Braucht:** X.1, X.2, X.4 · **Dateien:** `dist-workspace.toml`,
  `.github/workflows/release.yml` (nur über `dist generate`), `docs/release.md`
- Erst, wenn Windows benutzbar ist. Entscheiden und einrichten: Azure Artifact Signing (Monatsabo; `dist` ab
  0.33, also `cargo-dist-version` anheben; vorher prüfen, ob es für Einzelentwickler in Deutschland offen ist)
  oder klassisches OV-Zertifikat (Schlüssel in Hardware oder Cloud; `dist` 0.32 kann SSL.com eSigner).
- Auch mit Signatur warnt SmartScreen, bis die Dateien genug Downloads gesammelt haben; das in
  `docs/release.md` festhalten.
- Abnahme: Ein Release-Kandidat liefert signierte `.exe` (`signtool verify /pa` bzw. Eigenschaften →
  Digitale Signaturen); Checkliste in `docs/release.md` abgehakt.

### X.11 Abnahme auf echten Rechnern und Doku
- [ ] erledigt
- **Modell:** Sonnet, mit dem Maintainer · **Braucht:** alle oben · **Dateien:** `docs/release.md`, `docs/roadmap.md`,
  `README.md`, `docs/tasks/plattformen.md`
- Rundgang auf einem Mac (mit und ohne AeroSpace) und einem Windows-Rechner (mit und ohne GlazeWM), jeweils
  aus dem Release installiert: Konto verbinden, Kalender und Aufgaben mit echten Daten, Tray, Vorlage
  speichern und laden, Start beim Anmelden, Benachrichtigungen, MCP aus einem Client. Design-Prüfung der
  Apps auf beiden Systemen (Agent `torro-design-review`: Schrift, Fensterrahmen, Skalierung).
- Funde als eigene Aufgaben hier eintragen. Danach: „Windows (experimentell)“ in `docs/release.md` und im
  README streichen, Plattform-Tabelle aktualisieren.
- Abnahme: Rundgang auf beiden Systemen ohne offene Funde, Doku aktualisiert.
