# Aufgaben Phase 1: Kern-Prototyp unter Linux

Ziel und Abnahme der Phase: [core.md](../core.md#prototyp-umfang-und-abnahme). Regeln für die
Arbeit: [CLAUDE.md](../../CLAUDE.md).

## So arbeitest du mit diesen Aufgaben

- Jede Aufgabe ist klein: wenige Dateien, eine klare Abnahme. Sie ist so beschrieben, dass ein
  kleines Modell (Haiku) sie ohne weiteren Kontext erledigen kann.
- **Modell** gibt an, was reicht. **Opus** steht dort, wo Schnittstellen festgelegt werden oder
  mehrere Teile ineinandergreifen; dort lohnt das größere Modell.
- Reihenfolge: Eine Aufgabe beginnt erst, wenn alle unter **Braucht** erledigt sind. Aufgaben ohne
  gegenseitige Abhängigkeit können parallel laufen.
- Die Signaturen aus 1.0 stehen im Code, jeweils mit Doc-Kommentar. Der Doc-Kommentar ist Teil der
  Aufgabe: Was dort steht, soll die Umsetzung erfüllen.
- Startprompt für eine neue Sitzung:

  > Lies CLAUDE.md und erledige Aufgabe 1.X aus docs/tasks/phase-1.md. Halte dich an die
  > genannten Dateien und prüfe am Ende mit fmt, clippy und test.

## Überblick

```
1.0 Gerüst (Opus)
 ├─ 1.1 Tray-Versuch ─ 1.2 Icon
 ├─ 1.3 Hyprland-Socket ─ 1.4 JSON lesen ─ 1.5 Abfragen ─┬─ 1.6 Steuern ─ 1.8 Layout-Befehle
 │                                                       └─ 1.7 Ereignisse
 ├─ 1.9 Backend erkennen (nach 1.3)
 ├─ 1.10 Layout erfassen   1.11 Ablage   1.12 Startbefehle
 │        └──────────┬───────────┘──────────┘
 │               1.13 Erfassen ─ 1.14 Laden: Modi + herholen ─ 1.15 Laden: neu starten ─ 1.16 Laden: Layout
 ├─ 1.17 IPC ─ 1.18 ctl und status
 └─ 1.19 Tray-Menü verdrahten (Opus) ─ 1.20 Benachrichtigungen ─ 1.21 Autostart ─ 1.22 Abnahme
```

## Aufgaben

### 1.0 Gerüst und Schnittstellen
- [x] erledigt
- **Modell:** Opus · **Braucht:** –
- **Ziel:** Alles, worauf die anderen Aufgaben aufbauen, steht als kompilierender Code mit
  festgelegten Typen und Signaturen. Implementierungen geben vorerst `Err(…NotImplemented)` zurück.
- **Inhalt:**
  - Crate `crates/torro-wm`: Typen `WindowId`, `WorkspaceId`, `Rect`, `Monitor`, `Window`, `Split`
    (Richtung + Verhältnis), `WmEvent`, Fehler `WmError`; Trait `WindowManager` (Entwurf in
    core.md); Enum `Backend` mit Variante `Hyprland` und `Unsupported { name }`; Modul `fake` mit
    einem `FakeWm` für Tests, der Aufrufe mitschreibt.
  - Crate `crates/torro-setups`: Typen der Vorlage (Schema 3, serde), `LayoutTree`, `Strategy`
    (`New`, `Move`), `LoadMode` (`Empty`, `Add`, `Replace`), Fehler `SetupsError`; leere Module
    `layout`, `storage`, `launch`, `capture`, `restore` mit Signaturen.
  - `torro-ipc`: `Request`/`Response` für die Vorlagen-Befehle, Zeilenformat JSON.
  - Abhängigkeiten in der Wurzel-`Cargo.toml` (tokio, serde, serde_json, ksni, notify-rust …).
  - GitHub-Actions-Workflow: fmt, clippy, test auf Ubuntu, macOS, Windows.
- **Abnahme:** `cargo test --workspace` und clippy grün, CI-Workflow vorhanden.

### 1.1 Tray-Versuch mit ksni
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torroworkplace/src/tray/mod.rs`, `crates/torroworkplace/src/tray/linux.rs`,
  `crates/torroworkplace/src/main.rs` (nur der Zweig `daemon`)
- **Ziel:** `torroworkplace daemon` zeigt unter Linux ein Tray-Icon mit einem festen Menü:
  „Vorlage laden ▸ Beispiel ▸ Hier laden / Hinzufügen / Ersetzen“, Trenner, „Beenden“.
  Ein Klick auf einen Eintrag schreibt seinen Namen auf stderr; „Beenden“ beendet den Prozess.
- **Hinweise:** Crate `ksni` (StatusNotifierItem über D-Bus). Als Icon vorerst ein Standard-Icon-Name
  (`icon_name`). Auf anderen Systemen gibt `daemon` „tray not supported yet“ aus.
- **Abnahme:** Unter Omarchy erscheint das Icon in der Leiste, Untermenüs öffnen sich, Beenden
  funktioniert. Prüft der Mensch von Hand; Ergebnis hier notieren.

### 1.2 Eigenes Tray-Icon
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.1
- **Dateien:** `crates/torroworkplace/assets/tray.png` (oder SVG), `crates/torroworkplace/src/tray/linux.rs`
- **Ziel:** Das Icon ist ein schlichtes Torro-Symbol in Torro-Rot (`theme::TORRO_RED`), eingebettet
  mit `include_bytes!` und als `icon_pixmap` (ARGB32) übergeben. In 22 px und 32 px.
- **Abnahme:** Icon in der Omarchy-Leiste gut erkennbar, hell und dunkel.

### 1.3 Hyprland: Befehls-Socket
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-wm/src/hyprland/socket.rs`
- **Ziel:** Funktion `request(cmd: &str) -> Result<String, WmError>`, die über
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` einen Befehl sendet
  (z. B. `j/clients`) und die ganze Antwort liest. Pfadbau als eigene, testbare Funktion, die die
  Umgebung als Parameter bekommt.
- **Abnahme:** Unit-Test für den Pfad (fehlende Variablen → Fehler). Ein mit `#[ignore]`
  markierter Test fragt den echten Hyprland ab.

### 1.4 Hyprland: Antworten lesen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-wm/src/hyprland/json.rs`, `crates/torro-wm/tests/fixtures/hyprland-*.json`
- **Ziel:** serde-Strukturen für `j/clients`, `j/activeworkspace`, `j/monitors` und Umwandlung in die
  neutralen Typen (`Window`, `WorkspaceId`, `Monitor`). Fenster auf Spezial-Workspaces, ohne Klasse,
  versteckte und nicht gemappte Fenster fallen weg.
- **Referenz:** `apps/workspace-setups/workspace_setups/hyprland.py`, Funktion `normal_clients`.
- **Testdaten:** echte Ausgaben von `hyprctl -j clients` usw. als Fixtures speichern (Titel anonymisieren).
- **Abnahme:** Tests lesen die Fixtures und prüfen Felder und Filter.

### 1.5 Hyprland: Fenster und Workspace abfragen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.3, 1.4
- **Dateien:** `crates/torro-wm/src/hyprland/mod.rs`
- **Ziel:** `active_workspace()`, `windows(workspace)` und `monitors()` des Traits für Hyprland. `windows`
  ergänzt den Monitornamen jedes Fensters über `j/monitors` (das Feld `monitor` in `j/clients` ist nur eine Nummer).
- **Abnahme:** `#[ignore]`-Test gegen den echten Hyprland liefert die Fenster des aktiven Workspace.

### 1.6 Hyprland: Fenster steuern
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.5
- **Dateien:** `crates/torro-wm/src/hyprland/dispatch.rs`, `crates/torro-wm/src/hyprland/mod.rs`
- **Ziel:** `focus_window`, `move_to_workspace`, `close_window`. Hyprland 0.56 unter Omarchy 4 nimmt
  Dispatcher in **Lua-Syntax** an, gesendet als `dispatch <ausdruck>` über den Befehls-Socket, z. B.
  `hl.dsp.focus({ window = "address:0x55d0…" })` oder
  `hl.dsp.window.move({ window = "address:0x…", workspace = "3", follow = false })`.
- **Referenz:** `hyprland.py`: `dispatch`, `window_selector`, `_lua_string` (Escaping!).
- **Abnahme:** Unit-Tests für das Bauen der Ausdrücke inkl. Escaping und Prüfung der Fenster-Adresse.

### 1.7 Hyprland: Ereignisse
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.5
- **Dateien:** `crates/torro-wm/src/hyprland/events.rs`
- **Ziel:** `.socket2.sock` zeilenweise lesen (`openwindow>>…`, `closewindow>>…`, `movewindow>>…`,
  `workspace>>…`) und in `WmEvent` umwandeln; als tokio-Stream bzw. Kanal.
- **Abnahme:** Unit-Tests für das Zerlegen einzelner Zeilen; unbekannte Ereignisse werden ignoriert.

### 1.8 Hyprland: Layout-Befehle
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.6
- **Dateien:** `crates/torro-wm/src/hyprland/mod.rs`, `crates/torro-wm/src/hyprland/dispatch.rs`
- **Ziel:** `prepare_insert` (Fenster fokussieren, dann `hl.dsp.layout("preselect r|l|u|d")`) und
  `place` (`hl.dsp.layout("splitratio <wert> exact")`).
- **Referenz:** `apps/workspace-setups/workspace_setups/restore.py` um Zeile 400 (`preselect`,
  `splitratio`, Umrechnung des Verhältnisses).
- **Abnahme:** Unit-Tests für die erzeugten Ausdrücke.

### 1.9 Backend erkennen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.3
- **Dateien:** `crates/torro-wm/src/detect.rs`
- **Ziel:** `detect(env) -> Backend`: Hyprland, wenn `Hyprland::from_env(env)` gelingt; sonst
  `Unsupported` mit einem lesbaren Namen („AeroSpace (macOS)“, „GlazeWM (Windows)“, „unbekannt“).
- **Abnahme:** Unit-Tests mit gesetzter und fehlender Variable.

### 1.10 Layout aus Fensterrechtecken erfassen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-setups/src/layout.rs`
- **Ziel:** `infer_tree(&[(id, Rect)]) -> LayoutTree`: Binärbaum aus Teilungen mit Richtung und
  Verhältnis (4 Nachkommastellen), Floating-Fenster nicht dabei, Rückfallebene für nicht teilbare
  Anordnungen.
- **Referenz:** `apps/workspace-setups/workspace_setups/layout.py` (36 Zeilen, gleiche Idee).
- **Abnahme:** Tests: ein Fenster, zwei nebeneinander (50/50, 35/65), verschachtelt (links eins,
  rechts zwei übereinander), nicht teilbar → Rückfall.

### 1.11 Vorlagen ablegen
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-setups/src/storage.rs`
- **Ziel:** `list`, `load`, `save`, `delete` für Vorlagen in `<data>/templates/<id>.json`
  (Datenverzeichnis des Kerns aus `torro_core::paths`). Atomar schreiben (temporäre Datei + rename),
  vor dem Überschreiben eine Sicherung in `<data>/backups/`, höchstens 5 je Vorlage. IDs als Slug
  aus dem Namen, eindeutig.
- **Referenz:** `apps/workspace-setups/workspace_setups/storage.py`.
- **Abnahme:** Tests mit `TORRO_DATA_HOME` in einem Temp-Verzeichnis: speichern, auflisten, laden,
  überschreiben mit Sicherung, Sicherungen begrenzt, löschen.

### 1.12 Startbefehle unter Linux
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-setups/src/launch/linux.rs`, `crates/torro-setups/src/launch/mod.rs`
- **Ziel:** Aus PID und Fensterklasse einen Startbefehl ermitteln: `/proc/<pid>/cmdline`, bei
  bekannten Fällen den `.desktop`-Eintrag (`~/.local/share/applications`, `/usr/share/applications`).
  Argumente, die nach Zugangsdaten aussehen (`--password`, `token=` …), fallen weg. Das
  `/proc`-Wurzelverzeichnis und die Suchpfade sind Parameter, damit Tests ein Fake-Verzeichnis nutzen.
- **Referenz:** `apps/workspace-setups/workspace_setups/commands.py`.
- **Abnahme:** Tests mit Fake-`/proc` und Fake-`.desktop`-Dateien.

### 1.13 Workspace erfassen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.10, 1.11, 1.12
- **Dateien:** `crates/torro-setups/src/capture.rs`
- **Ziel:** `capture(wm, id, resolve, now) -> Template` (Signatur steht in `capture.rs`): Fenster des
  aktiven Workspace, Baum (1.10), Startbefehle über `resolve`, Name automatisch „Workspace 3, 11.10. 14:20“, Standardstrategie je Fenster
  (`Move` für Programme, die nur einmal laufen, z. B. Spotify, Slack, Signal; sonst `New`).
- **Referenz:** `apps/workspace-setups/workspace_setups/capture.py`.
- **Abnahme:** Test mit `FakeWm` und drei Fenstern ergibt die erwartete Vorlage.

### 1.14 Laden: Modi und „herholen“
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.13
- **Dateien:** `crates/torro-setups/src/restore.rs`
- **Ziel:** Ziel-Workspace zu Beginn festhalten. Modus `Empty` bricht bei belegtem Workspace mit
  einem eigenen Fehler ab; `Replace` schließt zuerst die Fenster des Workspace; `Add` lässt sie.
  Strategie `Move`: passendes offenes Fenster auf einem anderen Workspace finden und herholen.
- **Abnahme:** Tests mit `FakeWm` für alle drei Modi und für „herholen“.

### 1.15 Laden: Programme neu starten
- [ ] erledigt
- **Modell:** Haiku, bei Problemen Sonnet · **Braucht:** 1.14, 1.7
- **Dateien:** `crates/torro-setups/src/restore.rs`, `crates/torro-setups/src/launch/mod.rs` (`SystemLauncher`)
- **Ziel:** Strategie `New`: Programm losgelöst starten (eigene Prozessgruppe, Ausgabe verworfen),
  auf ein neues Fenster warten (Ereignisse, Zeitlimit z. B. 15 s) und es dem Eintrag zuordnen:
  zuerst über die PID-Abstammung, sonst über die Klasse. Fehler bei einem Fenster brechen nicht das
  ganze Laden ab; das Ergebnis listet sie auf.
- **Referenz:** `restore.py`: `process_ancestors`, `match_client`.
- **Abnahme:** Tests mit `FakeWm`, der nach dem Start ein Fenster-Ereignis liefert; Test für das Zeitlimit.

### 1.16 Laden: Layout aufbauen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.15, 1.8
- **Dateien:** `crates/torro-setups/src/restore.rs`
- **Ziel:** Den Baum **von oben nach unten** aufbauen, wie im Doc-Kommentar von `restore.rs`
  beschrieben: Bei einer Teilung kommt das erste Blatt von `second` neben das erste Blatt von
  `first` (`prepare_insert`, öffnen bzw. herholen, `place` mit dem Verhältnis), danach werden beide
  Hälften genauso aufgebaut.
- **Abnahme:** Test mit `FakeWm` prüft die Reihenfolge der Aufrufe für einen verschachtelten Baum.

### 1.17 IPC zwischen Dienst und Clients
- [x] erledigt
- **Modell:** Haiku · **Braucht:** 1.0
- **Dateien:** `crates/torro-ipc/src/lib.rs`, `crates/torro-ipc/src/server.rs`, `crates/torro-ipc/src/client.rs`
- **Ziel:** Unix-Socket `core.sock` im State-Verzeichnis (Windows: vorerst „nicht unterstützt“).
  Eine JSON-Zeile pro Anfrage und Antwort. Server ruft einen übergebenen Handler auf. Läuft schon
  ein Dienst, meldet der zweite Start das und beendet sich (Einzelinstanz).
- **Abnahme:** Test startet Server und Client in einem Temp-Verzeichnis und tauscht `Ping` aus;
  Test für die Einzelinstanz.

### 1.18 `ctl` und `status`
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.17
- **Dateien:** `crates/torroworkplace/src/cli.rs`, `crates/torroworkplace/src/main.rs`
- **Ziel:** `torroworkplace ctl setups list|save|update <name>|delete <name>|load <name> [--mode empty|add|replace]`
  und `torroworkplace status --json`, die über IPC mit dem Dienst sprechen. Ausgabe lesbar, mit
  `--json` maschinenlesbar. Läuft kein Dienst: klare Meldung, Exit-Code 1.
- **Abnahme:** Tests für das Zerlegen der Argumente; von Hand gegen den laufenden Dienst geprüft.

### 1.19 Tray-Menü verdrahten
- [ ] erledigt
- **Modell:** Opus · **Braucht:** 1.2, 1.9, 1.13–1.18
- **Dateien:** `crates/torroworkplace/src/daemon.rs`, `crates/torroworkplace/src/tray/linux.rs`
- **Ziel:** Der Dienst hält den Zustand (Backend, aktiver Workspace, Vorlagen), baut daraus das Menü
  aus core.md und führt Einträge als Befehle aus, dieselben wie über IPC. Haupt-Thread: Tray;
  tokio-Laufzeit für IPC, Ereignisse und Laden. Das Menü wird nach jeder Änderung neu gebaut.
  „Hier laden“ ist nur bei leerem Workspace aktiv.
- **Abnahme:** Von Hand unter Omarchy: speichern, Vorlage erscheint im Menü, laden (alle drei
  Modi), aktualisieren, löschen.

### 1.20 Benachrichtigungen
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.19
- **Dateien:** `crates/torroworkplace/src/notify.rs`, `crates/torroworkplace/src/daemon.rs`
- **Ziel:** Desktop-Benachrichtigung nach Speichern und Laden (Erfolg, Teilfehler mit Anzahl,
  Fehler) über `notify-rust`.
- **Abnahme:** Von Hand: Benachrichtigungen erscheinen unter Omarchy.

### 1.21 Autostart unter Hyprland
- [ ] erledigt
- **Modell:** Haiku · **Braucht:** 1.19
- **Dateien:** `integrations/hyprland/autostart.lua`, `integrations/hyprland/torroworkplace.service`,
  `integrations/README.md`
- **Ziel:** Ein Schnipsel für die Hyprland-Konfiguration (Omarchy 4, Lua) und alternativ ein
  systemd-User-Dienst, die `torroworkplace daemon` beim Anmelden starten; Anleitung im README.
- **Abnahme:** Nach Ab- und Anmelden erscheint das Icon.

### 1.22 Abnahme der Phase
- [ ] erledigt
- **Modell:** Mensch, mit Haiku zum Protokollieren · **Braucht:** alle
- **Ziel:** Die fünf Punkte aus [core.md](../core.md#prototyp-umfang-und-abnahme) durchgehen, Ergebnis
  und gefundene Fehler hier notieren; Fehler werden neue Aufgaben (1.23 ff.).

## Danach

Phase 2 (Steuerzentrale, Tastenplätze, AeroSpace, GlazeWM) wird zerlegt, wenn Phase 1 steht,
weil sie auf den dann festen Schnittstellen aufbaut.
